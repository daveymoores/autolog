#![allow(unused_imports)]
use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::{GitLogDates, Repository};
use crate::utils::date::date_parser::{TimesheetYears, get_timesheet_map_from_date_hashmap};
use crate::utils::link::link_builder::TimesheetHoursForMonth;
use serde_json::{Map, Number, Value};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
pub fn create_mock_client_repository(client_repository: &mut ClientRepositories) {
    let mut repo = Repository {
        client_name: Option::from("alphabet".to_string()),
        client_address: Option::from("Spaghetti Way, USA".to_string()),
        client_contact_person: Option::from("John Smith".to_string()),
        name: Option::from("Jim Jones".to_string()),
        email: Option::from("jim@jones.com".to_string()),
        namespace: Option::from("autolog".to_string()),
        ..Default::default()
    };

    client_repository.set_values(&mut repo);
}

#[cfg(test)]
pub fn get_mock_year_map() -> TimesheetYears {
    let mut year_map: TimesheetYears = HashMap::new();

    let mut map = Map::new();
    map.extend([
        ("weekend".to_string(), Value::Bool(false)),
        (
            "hours".to_string(),
            Value::Number(Number::from_f64(0 as f64).unwrap()),
        ),
        ("user_edited".to_string(), Value::Bool(true)),
    ]);

    year_map.insert(
        "2021".to_string(),
        vec![("11".to_string(), vec![map.clone(), map.clone()])]
            .into_iter()
            .collect::<HashMap<String, Vec<Map<String, Value>>>>(),
    );

    year_map
}

#[cfg(test)]
pub fn get_timesheet_hashmap() -> GitLogDates {
    let date_hashmap: GitLogDates = vec![
        (2020, vec![(8, vec![1])]),
        (2019, vec![(1, vec![3])]),
        (2021, vec![(10, vec![23, 20, 21]), (9, vec![8])]),
    ]
    .into_iter()
    .map(|x| {
        let y: HashMap<u32, HashSet<u32>> =
            x.1.into_iter()
                .map(|k| {
                    let n: HashSet<u32> = k.1.into_iter().collect();
                    (k.0, n)
                })
                .collect();
        (x.0, y)
    })
    .collect();

    date_hashmap
}

#[cfg(test)]
pub fn create_mock_repository() -> Repository {
    // testing utility that returns
    // {2021: {10: {20, 23, 21}, 9: {8}}, 2020: {8: {1}}, 2019: {1: {3}}}
    let date_hashmap: GitLogDates = get_timesheet_hashmap();
    let timesheet =
        get_timesheet_map_from_date_hashmap(date_hashmap, &mut Default::default(), vec![]);

    let repository = Repository {
        namespace: Option::from("autolog".to_owned()),
        timesheet: Option::from(timesheet),
        ..Default::default()
    };

    repository
}

#[cfg(test)]
pub fn create_mock_timesheet_hours_for_month() -> TimesheetHoursForMonth {
    let f64_value = Value::Number(Number::from_f64(8.0).unwrap());

    let mut map = Map::new();
    map.extend(vec![("hours".to_string(), f64_value)]);

    let month: TimesheetHoursForMonth = vec![map.clone(), map.clone(), map.clone()];
    month
}

#[cfg(test)]
// Generate git log dates that overlap by one day each month to test hours being split equally
pub fn generate_project_git_log_dates(days: [u32; 3]) -> GitLogDates {
    HashMap::from([
        (
            2019,
            HashMap::from([(1, HashSet::from(days)), (2, HashSet::from(days))]),
        ),
        (
            2020,
            HashMap::from([(5, HashSet::from(days)), (2, HashSet::from(days))]),
        ),
        (
            2021,
            HashMap::from([(9, HashSet::from(days)), (2, HashSet::from(days))]),
        ),
    ])
}
