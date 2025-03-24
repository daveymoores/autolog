use crate::data::repository::{GitLogDates, Repository};
use chrono::{NaiveDate, TimeZone, Utc};
use regex::Regex;
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::error::Error;
use std::io::ErrorKind;
use std::{io, process};

fn return_worked_hours_from_worked_days(
    worked_days: &[u32],
    day: &u32,
    adjacent_days_in_month: &[HashSet<u32>],
    default_hours: Option<f64>,
) -> f64 {
    let hours = if let Some(h) = default_hours { h } else { 8.0 };
    // if day exists in adjacent days, then split the number of hours by number of occurrences
    let frequency_of_day_worked_in_adjacent_timesheets: f64 = (adjacent_days_in_month
        .iter()
        .map(|month| month.get(day))
        .filter(|e| e.is_some())
        .collect::<Vec<Option<&u32>>>()
        .len()
        + 1) as f64;

    let worked_day = worked_days.contains(day);
    match worked_day {
        true => hours / frequency_of_day_worked_in_adjacent_timesheets,
        false => 0.0,
    }
}

pub fn is_weekend(date_tuple: &(i32, u32, u32), day: u32) -> bool {
    let day_of_week_index = Utc
        .with_ymd_and_hms(date_tuple.0, date_tuple.1, day, 0, 0, 0)
        .unwrap()
        .format("%u")
        .to_string();

    matches!(day_of_week_index.parse().unwrap(), 6 | 7)
}

pub type DayMap = [(String, Value); 3];

pub fn create_single_day_object(weekend: bool, hours: f64, edited: bool) -> DayMap {
    [
        ("weekend".to_string(), Value::Bool(weekend)),
        (
            "hours".to_string(),
            Value::Number(Number::from_f64(hours).unwrap()),
        ),
        ("user_edited".to_string(), Value::Bool(edited)),
    ]
}

fn set_day_map(weekend: bool, hours: f64, edited: bool, day_map: &mut Map<String, Value>) {
    day_map.extend(create_single_day_object(weekend, hours, edited));
}

fn parse_hours_from_date(
    date_tuple: (i32, u32, u32),
    worked_days: Vec<u32>,
    repository: &mut Repository,
    adjacent_days_in_month: Vec<HashSet<u32>>,
) -> Vec<Map<String, Value>> {
    // iterate through the number of days in the month
    // for each day return the calendar day
    // if its a Weekend or day that isn't worked, set to zero, otherwise 8
    let mut vector = vec![];

    for day in 1..date_tuple.2 + 1 {
        let is_weekend: bool = is_weekend(&date_tuple, day);
        let mut day_map = Map::new();
        let hours_worked = return_worked_hours_from_worked_days(
            &worked_days,
            &day,
            &adjacent_days_in_month,
            repository.default_hours,
        );

        // Each day denotes whether it is a Weekend, what the hours worked are
        // and whether it has been manually edited by the user to prevent these
        // changes being overwritten when the data is synced
        match repository.timesheet {
            // if there is no timesheet at all, then just add the days in
            None => {
                set_day_map(is_weekend, hours_worked, false, &mut day_map);
            }
            // if there is a timesheet then lets check whether the day value has been edited
            // before setting the hour value
            Some(_) => {
                let day_index: usize = usize::try_from(day).unwrap();
                let is_user_edited = match repository.get_timesheet_entry(
                    &date_tuple.0.to_string(),
                    &date_tuple.1,
                    day_index,
                    "user_edited".to_string(),
                ) {
                    Ok(result) => result,
                    Err(err) => {
                        eprintln!("Error retrieving timesheet entry: {}", err);
                        process::exit(exitcode::DATAERR);
                    }
                };

                // if it hasn't been edited or the month isn't in the old data, then just set it
                if is_user_edited.unwrap_or(&Value::Bool(false)) == &Value::Bool(false) {
                    set_day_map(is_weekend, hours_worked, false, &mut day_map);
                } else {
                    // otherwise get the existing value from the timesheet
                    let hours_worked_for_user_edited_day = match repository.get_timesheet_entry(
                        &date_tuple.0.to_string(),
                        &date_tuple.1,
                        day_index,
                        "hours".to_string(),
                    ) {
                        Ok(result) => result,
                        Err(err) => {
                            eprintln!("Error retrieving timesheet entry: {}", err);
                            process::exit(exitcode::DATAERR);
                        }
                    };

                    set_day_map(
                        is_weekend,
                        hours_worked_for_user_edited_day.unwrap().as_f64().unwrap(),
                        true,
                        &mut day_map,
                    );
                }
            }
        }

        vector.push(day_map);
    }

    vector
}

pub type TimesheetMonths = HashMap<String, Vec<Map<String, Value>>>;
pub type TimesheetYears = HashMap<String, HashMap<String, Vec<Map<String, Value>>>>;

fn get_adjacent_git_log_days_for_month<'a>(
    adjacent_git_log_days: Vec<GitLogDates>,
    year: &'a i32,
    month: &'a u32,
) -> Vec<HashSet<u32>> {
    let mut repo_days = vec![];
    for log_day in adjacent_git_log_days.iter() {
        if let Some(year) = log_day.get(year) {
            if let Some(month) = year.get(month) {
                repo_days.push(month.clone());
            }
        }
    }

    repo_days
}

pub fn get_timesheet_map_from_date_hashmap(
    git_log_dates: GitLogDates,
    repository: &mut Repository,
    adjacent_git_log_dates: Vec<GitLogDates>,
) -> TimesheetYears {
    let timesheet_years: TimesheetYears = git_log_dates
        .into_iter()
        .map(|(year, months)| {
            let month_map: TimesheetMonths = months
                .clone()
                .into_iter()
                .map(|(month, days)| {
                    let mut worked_days = days.into_iter().collect::<Vec<u32>>();
                    worked_days.sort();
                    let days_in_month = get_days_from_month(year, month);
                    let adjacent_days_in_month = get_adjacent_git_log_days_for_month(
                        adjacent_git_log_dates.clone(),
                        &year,
                        &month,
                    );

                    let worked_hours_for_month = parse_hours_from_date(
                        (year, month, days_in_month),
                        worked_days,
                        repository,
                        adjacent_days_in_month,
                    );
                    (month.to_string(), worked_hours_for_month)
                })
                .collect();
            (year.to_string(), month_map)
        })
        .collect();

    timesheet_years
}

pub fn get_days_from_month(year: i32, month: u32) -> u32 {
    NaiveDate::from_ymd_opt(
        match month {
            12 => year + 1,
            _ => year,
        },
        match month {
            12 => 1,
            _ => month + 1,
        },
        1,
    )
    .expect("Valid date expected")
    .signed_duration_since(NaiveDate::from_ymd_opt(year, month, 1).expect("Valid date expected"))
    .num_days() as u32
}

pub fn check_for_valid_day(
    day: &Option<String>,
    month: u32,
    year: i32,
) -> Result<&String, Box<dyn Error>> {
    let day_string = day
        .as_ref()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "Day not found".to_string()))?;

    let days_in_month = get_days_from_month(year, month);
    let day_regex = Regex::new(r"^(3[0-1]|2[0-9]|1[0-9]|[1-9])$").unwrap();

    if !day_regex.is_match(day_string) {
        // into implements a conversion between &str and Box<dyn Error>
        return Err("Day index in the month doesn't exist".into());
    }

    if days_in_month < day_string.parse().unwrap() {
        return Err("The day given doesn't exist for the given month/year".into());
    }

    Ok(day_string)
}

pub fn check_for_valid_month(month: &Option<String>) -> Result<u32, Box<dyn Error>> {
    let month_u32 = month
        .as_ref()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "Month not found".to_string()))?
        .parse::<u32>()?;

    let month_regex = Regex::new(r"^(1[0-2]|[1-9])$").unwrap();

    if !month_regex.is_match(&month_u32.to_string()) {
        return Err("Not a real month".into());
    }

    Ok(month_u32)
}

pub fn check_for_valid_year(year: &Option<String>) -> Result<&String, Box<dyn Error>> {
    let year_string = year
        .as_ref()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "Year not found".to_string()))?;

    let year_regex = Regex::new(r"^((19|20)\d{2})$").unwrap();

    if !year_regex.is_match(year_string) {
        // into implements a conversion between &str and Box<dyn Error>
        return Err("Not a real year".into());
    }

    Ok(year_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::repository::GitLogDates;
    use serde_json::{Map, Number, Value};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn it_returns_worked_hours_from_worked_days() {
        let adjacent_days_in_month = vec![HashSet::from([1, 2, 3]), HashSet::from([2, 3, 4])];
        assert_eq!(
            return_worked_hours_from_worked_days(
                &vec![1, 3, 6, 22],
                &2,
                &adjacent_days_in_month,
                Option::None
            ),
            0.0
        );
        assert_eq!(
            return_worked_hours_from_worked_days(
                &vec![1, 3, 6, 22],
                &22,
                &adjacent_days_in_month,
                Option::None
            ),
            8.0
        );
        assert_eq!(
            return_worked_hours_from_worked_days(
                &vec![1, 3, 4, 22],
                &4,
                &adjacent_days_in_month,
                Option::None
            ),
            4.0
        );
        assert_eq!(
            return_worked_hours_from_worked_days(
                &vec![1, 3, 4, 22],
                &3,
                &adjacent_days_in_month,
                Option::None
            ),
            2.6666666666666665
        );
    }

    #[test]
    fn is_weekend_returns_truth_if_weekend() {
        assert_eq!(is_weekend(&(2021, 11, 6), 6), true);
        assert_eq!(is_weekend(&(2021, 11, 28), 28), true);
    }

    #[test]
    fn is_weekend_returns_false_if_not_weekend() {
        assert_eq!(is_weekend(&(2021, 11, 8), 8), false);
        assert_eq!(is_weekend(&(2021, 11, 23), 23), false);
    }

    #[test]
    fn it_parses_hours_from_date() {
        let adjacent_days_in_month = vec![HashSet::from([1, 2, 3]), HashSet::from([2, 3, 4])];

        let mut weekday_map = Map::new();
        weekday_map.extend(vec![
            (
                "hours".to_string(),
                Value::Number(Number::from_f64(8.0).unwrap()),
            ),
            ("user_edited".to_string(), Value::Bool(false)),
            ("Weekend".to_string(), Value::Bool(false)),
        ]);

        let mut weekend_map = Map::new();
        weekend_map.extend(vec![
            (
                "hours".to_string(),
                Value::Number(Number::from_f64(0.0).unwrap()),
            ),
            ("user_edited".to_string(), Value::Bool(false)),
            ("Weekend".to_string(), Value::Bool(true)),
        ]);

        let day_vec = parse_hours_from_date(
            (2021 as i32, 10 as u32, 31 as u32),
            vec![1, 4, 6],
            &mut Default::default(),
            adjacent_days_in_month,
        );

        assert_eq!(
            *day_vec[0].get("hours").unwrap(),
            Value::Number(Number::from_f64(4.0).unwrap())
        );
        assert_eq!(
            *day_vec[1].get("hours").unwrap(),
            Value::Number(Number::from_f64(0.0).unwrap())
        );
        assert_eq!(
            *day_vec[3].get("hours").unwrap(),
            Value::Number(Number::from_f64(4.0).unwrap())
        );
        assert_eq!(
            *day_vec[5].get("hours").unwrap(),
            Value::Number(Number::from_f64(8.0).unwrap())
        );
        assert_eq!(day_vec.len(), 31);
    }

    #[test]
    fn it_finds_adjacent_git_log_days_for_a_given_month() {
        let git_log_dates: Vec<GitLogDates> = vec![
            HashMap::from([(
                2019,
                HashMap::from([(1, HashSet::from([1, 2, 3])), (2, HashSet::from([1, 2, 3]))]),
            )]),
            HashMap::from([(
                2019,
                HashMap::from([(1, HashSet::from([2, 3, 4])), (2, HashSet::from([2, 3, 4]))]),
            )]),
        ];

        let vec_of_days = get_adjacent_git_log_days_for_month(git_log_dates, &2019, &2);
        assert_eq!(
            vec_of_days,
            vec![HashSet::from([1, 2, 3]), HashSet::from([2, 3, 4])]
        )
    }

    #[test]
    fn it_finds_the_number_of_days_for_a_specific_month_and_year() {
        assert_eq!(get_days_from_month(2021, 10), 31);
        assert_eq!(get_days_from_month(1989, 2), 28);
        assert_eq!(get_days_from_month(1945, 6), 30);
        // leap year
        assert_eq!(get_days_from_month(2024, 2), 29);
    }

    #[test]
    fn it_checks_for_valid_day_and_throws() {
        // days is a string
        assert!(check_for_valid_day(&Option::from("foo".to_string()), 10, 2021).is_err());
        // days don't exist for any month
        assert!(check_for_valid_day(&Option::from("32".to_string()), 10, 2021).is_err());
        // days don't exist for november
        assert!(check_for_valid_day(&Option::from("31".to_string()), 11, 2021).is_err());
    }

    #[test]
    fn it_checks_for_valid_day() {
        assert!(check_for_valid_day(&Option::from("31".to_string()), 10, 2021).is_ok());
    }

    #[test]
    fn it_checks_for_valid_month_and_throws() {
        // month is a string that can't be parsed
        assert!(check_for_valid_month(&Option::from("foo".to_string())).is_err());
        // month doesn't exist
        assert!(check_for_valid_month(&Option::from("99".to_string())).is_err());
        // month doesn't exist
        assert!(check_for_valid_month(&Option::from("0".to_string())).is_err());
    }

    #[test]
    fn it_checks_for_valid_month() {
        assert!(check_for_valid_month(&Option::from("12".to_string())).is_ok());
    }

    #[test]
    fn it_checks_for_valid_year_and_throws() {
        // year is a string that can't be parsed
        assert!(check_for_valid_year(&Option::from("foo".to_string())).is_err());
        // year doesn't exist
        assert!(check_for_valid_year(&Option::from("3000".to_string())).is_err());
        // year is unlikely
        assert!(check_for_valid_year(&Option::from("1898".to_string())).is_err());
    }

    #[test]
    fn it_checks_for_valid_year() {
        assert!(check_for_valid_year(&Option::from("1998".to_string())).is_ok());
        assert!(check_for_valid_year(&Option::from("2020".to_string())).is_ok());
        assert!(check_for_valid_year(&Option::from("2099".to_string())).is_ok());
    }
}
