use crate::data::client_repositories::{Approver, Client, ClientRepositories, User};
use crate::data::repository::Repository;
use crate::db;
use crate::utils::date::date_parser::{check_for_valid_month, check_for_valid_year};
use chrono::{DateTime, Month, Utc};
use dotenv;
use mongodb::bson::doc;
use num_traits::cast::FromPrimitive;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::error::Error;
use std::process;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct Timesheet {
    namespace: String,
    timesheet: TimesheetHoursForMonth,
    total_hours: f64,
    project_number: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TimesheetDocument {
    creation_date: DateTime<Utc>,
    random_path: String,
    month_year: String,
    client: Option<Client>,
    user: Option<User>,
    approver: Option<Approver>,
    timesheets: Vec<Timesheet>,
}

pub type TimesheetHoursForMonth = Vec<Map<String, Value>>;

fn get_string_month_year(
    month: &Option<String>,
    year: &Option<String>,
) -> Result<String, Box<dyn Error>> {
    let month_u32 = check_for_valid_month(month)?;
    let year_string = check_for_valid_year(year)?;

    Ok(format!(
        "{}, {}",
        Month::from_u32(month_u32).unwrap().name(),
        year_string
    ))
}

fn find_month_from_timesheet<'a>(
    sheet: &'a Repository,
    options: &'a [Option<String>],
) -> Result<Option<&'a TimesheetHoursForMonth>, Box<dyn Error>> {
    // safe to unwrap options here as it would have been caught above
    let option = sheet
        .timesheet
        .as_ref()
        .unwrap()
        .get(&options[2].as_ref().unwrap().to_string())
        .and_then(|year| {
            year.get(&options[1].as_ref().unwrap().to_string())
                .and_then(Option::from)
        });
    Ok(option)
}

fn build_document<'a>(
    creation_date: DateTime<Utc>,
    random_path: &'a str,
    month_year_string: &'a str,
    timesheets: &'a [Timesheet],
    client_repositories: &'a ClientRepositories,
) -> TimesheetDocument {
    let repos = client_repositories;
    // When this is serialised, it can't take references to data
    // so make it all owned
    TimesheetDocument {
        creation_date,
        random_path: random_path.to_owned(),
        month_year: month_year_string.to_owned(),
        user: repos.user.clone(),
        client: repos.client.clone(),
        approver: repos.approver.clone(),
        timesheets: timesheets.to_owned(),
    }
}

fn calculate_total_hours(timesheet_month: &TimesheetHoursForMonth) -> f64 {
    let hours: Vec<f64> = timesheet_month
        .iter()
        .map(|x| x.get("hours").unwrap().as_f64().unwrap())
        .collect();

    let total_hours: f64 = hours.iter().copied().sum();
    total_hours
}

fn generate_timesheet_vec(
    client_repositories: &mut ClientRepositories,
    options: Vec<Option<String>>,
    month_year_string: &String,
) -> Result<Vec<Timesheet>, Box<dyn Error>> {
    let mut timesheets: Vec<Timesheet> = vec![];
    let repos_option = &client_repositories.repositories;
    let repos = repos_option.as_ref().unwrap();

    // for each repo, find the specified timesheet month and push into vec
    for repo in repos.iter() {
        let namespace = &repo.namespace;
        let project_number = &repo.project_number;

        let timesheet_hours_for_month =
            find_month_from_timesheet(repo, &options).unwrap_or_else(|err| {
                eprintln!("Error finding year/month in timesheet data: {}", err);
                std::process::exit(exitcode::DATAERR);
            });

        if let Some(timesheet) = timesheet_hours_for_month {
            timesheets.push(Timesheet {
                namespace: namespace.as_ref().map(|x| x.to_owned()).unwrap(),
                timesheet: timesheet.to_owned(),
                total_hours: calculate_total_hours(timesheet),
                project_number: project_number.to_owned(),
            });
        }
    }

    // prevent this from building a document if there aren't timesheets for the month
    if timesheets.is_empty() {
        eprintln!(
            "No days worked for any repositories in {}. \n\
            Timesheet not generated.",
            &month_year_string
        );

        std::process::exit(exitcode::DATAERR);
    }

    Ok(timesheets)
}

pub async fn build_unique_uri(
    client_repositories: &mut ClientRepositories,
    options: Vec<Option<String>>,
) -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    let month_year_string = get_string_month_year(&options[1], &options[2])?;
    let timesheets = generate_timesheet_vec(client_repositories, options, &month_year_string)?;

    crate::interface::help_prompt::HelpPrompt::show_generating_timesheet_message(
        &month_year_string,
    );

    let mongodb_db = env!("MONGODB_DB");
    let mongodb_collection = env!("MONGODB_COLLECTION");

    let db = db::Db::new().await?;
    let collection = db
        .client
        .database(mongodb_db)
        .collection(mongodb_collection);

    let random_path: String = db.generate_random_path(&collection).await?;
    let document = build_document(
        Utc::now(),
        &random_path,
        &month_year_string,
        &timesheets,
        &client_repositories,
    );

    // Check for existing index for TTL on the collection
    let index_names = collection.list_index_names().await?;

    let expire_time_seconds: i32 = env!("EXPIRE_TIME_SECONDS")
        .parse()
        .expect("Expire time can't be parsed to i32");

    if !index_names.contains(&String::from("expiration_date")) {
        // create TTL index to expire documents after expire_time_seconds
        db.client
            .database(mongodb_db)
            .run_command(
                doc! {
                    "createIndexes": &mongodb_collection,
                    "indexes": [
                        {
                            "key": { "creation_date": 1 },
                            "name": "expiration_date",
                            "expireAfterSeconds": expire_time_seconds,
                            "unique": true
                        },
                    ]
                },
                None,
            )
            .await?;
    }

    collection.insert_one(document.clone(), None).await?;

    let timesheet_gen_uri: String = format!("{}/{}", env!("AUTOLOG_URI"), &random_path);

    crate::interface::help_prompt::HelpPrompt::show_new_link_success(
        expire_time_seconds / 60,
        &timesheet_gen_uri,
    );

    process::exit(exitcode::OK);
}

#[cfg(test)]
mod test {
    use crate::data::client_repositories::{Approver, Client, ClientRepositories, User};
    use crate::data::repository::Repository;
    use crate::helpers::mocks;
    use crate::utils::link::link_builder::{
        Timesheet, TimesheetDocument, build_document, calculate_total_hours,
        find_month_from_timesheet, generate_timesheet_vec, get_string_month_year,
    };
    use chrono::{TimeZone, Utc};
    use expect_test::expect_file;
    use nanoid::nanoid;
    use serde_json::json;

    #[test]
    fn it_generates_timesheet_vec() {
        let options = vec![
            Option::None,
            Option::from("10".to_owned()),
            Option::from("2021".to_owned()),
        ];

        let mut client_repository = ClientRepositories {
            repositories: Option::from(vec![mocks::create_mock_repository()]),
            ..Default::default()
        };

        let timesheets = generate_timesheet_vec(
            &mut client_repository,
            options,
            &"February, 2021".to_string(),
        )
        .unwrap();

        let expected =
            expect_file!["../../../testing-utils/snapshots/it_generates_timesheet_vec.txt"];
        expected.assert_debug_eq(&timesheets.get(0));
    }

    #[test]
    fn it_builds_document() {
        let timesheet_for_month = mocks::create_mock_timesheet_hours_for_month();

        let client = Option::from(Client {
            id: nanoid!(),
            client_name: "alphabet".to_string(),
            client_address: "Spaghetti Way, USA".to_string(),
            client_contact_person: "John Smith".to_string(),
        });

        let user = Option::from(User {
            id: nanoid!(),
            name: "Jim Jones".to_string(),
            email: "jim@jones.com".to_string(),
            is_alias: false,
            thumbnail: Option::None,
        });

        let approver = Option::from(Approver {
            approvers_name: Option::Some("Bob Brown".to_string()),
            approvers_email: Option::Some("bob@brown.com".to_string()),
        });

        let timesheets = vec![Timesheet {
            namespace: "Some project".to_string(),
            timesheet: timesheet_for_month,
            total_hours: 50.0,
            project_number: None,
        }];

        let document = TimesheetDocument {
            creation_date: Utc.with_ymd_and_hms(2014, 11, 28, 12, 0, 9).unwrap(),
            random_path: "fbfxhs".to_string(),
            month_year: "November, 2021".to_string(),
            client: client.clone(),
            user: user.clone(),
            approver: approver.clone(),
            timesheets: timesheets.clone(),
        };

        let generated_document = build_document(
            Utc.with_ymd_and_hms(2014, 11, 28, 12, 0, 9).unwrap(),
            &"fbfxhs".to_string(),
            &"November, 2021".to_string(),
            &timesheets,
            &ClientRepositories {
                client,
                user,
                approver,
                repositories: Option::from(vec![Repository {
                    ..Default::default()
                }]),
                ..Default::default()
            },
        );

        assert_eq!(json!(generated_document), json!(document));
    }

    #[test]
    fn it_calculates_total_hours() {
        let month = mocks::create_mock_timesheet_hours_for_month();
        assert_eq!(calculate_total_hours(&month), 24.0);
    }

    #[test]
    fn it_throws_error_getting_string_month_year_with_incorrect_month() {
        let options = vec![
            Option::from("blah blah".to_owned()),
            Option::from("2021".to_owned()),
        ];
        assert!(get_string_month_year(&options[0], &options[1]).is_err());
    }

    #[test]
    fn it_throws_error_getting_string_month_year_with_incorrect_year() {
        let options = vec![
            Option::from("10".to_owned()),
            Option::from("blah blah".to_owned()),
        ];
        assert!(get_string_month_year(&options[0], &options[1]).is_err());
    }

    #[test]
    fn it_throws_error_getting_string_month_year_with_year_that_doesnt_exist() {
        let options = vec![
            Option::from("10".to_owned()),
            Option::from("1345".to_owned()),
        ];
        assert!(get_string_month_year(&options[0], &options[1]).is_err());
    }

    #[test]
    fn it_throws_error_getting_string_month_year_with_month_that_doesnt_exist() {
        let options = vec![
            Option::from("15".to_owned()),
            Option::from("1345".to_owned()),
        ];
        assert!(get_string_month_year(&options[0], &options[1]).is_err());
    }

    #[test]
    fn it_gets_string_for_month_year() {
        let options = vec![
            Option::from("10".to_owned()),
            Option::from("2021".to_owned()),
        ];
        assert_eq!(
            get_string_month_year(&options[0], &options[1]).unwrap(),
            "October, 2021".to_string()
        );
    }

    #[test]
    fn returns_none_if_month_cannot_be_found() {
        let options = vec![
            Option::None,
            Option::from("2".to_owned()),
            Option::from("2021".to_owned()),
        ];

        let timesheet = mocks::create_mock_repository();
        assert_eq!(
            find_month_from_timesheet(&timesheet, &options).unwrap(),
            Option::None
        );
    }

    #[test]
    fn it_returns_month_from_timesheet() {
        let options = vec![
            Option::None,
            Option::from("10".to_owned()),
            Option::from("2021".to_owned()),
        ];

        let timesheet = mocks::create_mock_repository();
        assert!(find_month_from_timesheet(&timesheet, &options).is_ok());
        assert_eq!(
            find_month_from_timesheet(&timesheet, &options)
                .unwrap()
                .unwrap()
                .len(),
            31
        );
    }
}
