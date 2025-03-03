use crate::data::client_repositories::ClientRepositories;
use crate::interface::help_prompt::{ConfigurationDoc, Onboarding};
use crate::utils::is_test_mode;
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use tempfile::tempfile;

const CONFIG_FILE_NAME: &str = ".autolog.txt";

/// Find the path to the users home directory
pub fn get_home_path() -> PathBuf {
    match dirs::home_dir() {
        Some(dir) => dir,
        None => panic!("Home directory not found"),
    }
}

/// Create filepath to config file
pub fn get_filepath(path: PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    return if is_test_mode() {
        let path_string = &*format!("./testing-utils/{}", CONFIG_FILE_NAME);
        Ok(path_string.to_owned())
    } else {
        let home_path = path.to_str();
        Ok(home_path.unwrap().to_owned() + "/" + CONFIG_FILE_NAME)
    };
}

/// Read config file or throw error and call error function
fn read_file<T>(
    buffer: &mut String,
    path: &str,
    prompt: &mut T,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: Onboarding,
{
    match File::open(path) {
        Ok(mut file) => {
            file.read_to_string(buffer)?;
        }
        Err(_) => {
            prompt.onboarding(true)?;
        }
    };

    Ok(())
}

pub fn read_data_from_config_file<T>(
    buffer: &mut String,
    prompt: &mut T,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: Onboarding,
{
    let config_path = get_filepath(get_home_path())?;
    let path = &config_path;
    read_file(buffer, path, prompt)?;

    Ok(())
}

pub fn delete_config_file() -> Result<(), Box<dyn std::error::Error>> {
    if is_test_mode() {
        return Ok(());
    }

    let config_path = get_filepath(get_home_path())?;
    std::fs::remove_file(config_path)?;

    Ok(())
}

pub fn write_json_to_config_file(
    json: String,
    config_path: String,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_test_mode() {
        let mut file = tempfile()?;
        file.write_all(json.as_bytes())?;
        return Ok(());
    }

    let mut file = File::create(config_path)?;

    file.write_all(json.as_bytes())?;

    Ok(())
}

pub fn serialize_config(
    client_repositories: Option<&mut ClientRepositories>,
    deserialized_config: Option<&mut ConfigurationDoc>,
) -> Result<String, Box<dyn std::error::Error>> {
    let config_data = match deserialized_config {
        // if deserialized_config doesn't exist, then create fresh json for file
        None => match client_repositories {
            None => {
                eprintln!("Tried to create a JSON literal but nothing was passed");
                std::process::exit(exitcode::DATAERR);
            }
            Some(client_repo) => {
                json!(vec![client_repo])
            }
        },
        // if it does exist, lets add it to the existing client repositories, or
        // push it into the vec to add a new client
        Some(config) => {
            match client_repositories {
                None => json!(config),
                Some(client_repos) => {
                    // get the values from the current repo so that it can be merged back into the config
                    // if deserialized_config is none, there is only one value in the vec so we can safely pull it out
                    let client = client_repos.client.clone();
                    let user = client_repos.user.clone();
                    let approver = client_repos.approver.clone();
                    let repository = client_repos.repositories.as_ref().unwrap()[0].clone();
                    let client_name = &client.as_ref().unwrap().client_name;
                    let requires_approval = client_repos.requires_approval;

                    let config_data: ConfigurationDoc = if config
                        .iter_mut()
                        .any(|x| &x.get_client_name().unwrap() == client_name)
                    {
                        let x: ConfigurationDoc = config
                            .iter_mut()
                            .map(|c| {
                                if &c.get_client_name().unwrap() == client_name {
                                    return ClientRepositories {
                                        requires_approval,
                                        approver: approver.clone(),
                                        client: client.clone(),
                                        user: user.clone(),
                                        repositories: Some(
                                            [
                                                c.clone().repositories.unwrap(),
                                                vec![repository.to_owned()],
                                            ]
                                            .concat(),
                                        ),
                                        ..Default::default()
                                    };
                                }
                                c.clone()
                            })
                            .collect();

                        x
                    } else {
                        config.push(client_repos.clone());
                        config.to_vec()
                    };

                    json!(config_data)
                }
            }
        }
    };

    let json = serde_json::to_string(&config_data)?;

    Ok(json)
}

pub fn get_canonical_path(path: &str) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|err| {
        println!("Canonicalization of repo path failed: {}", err);
        std::process::exit(exitcode::CANTCREAT);
    });
    path.to_str().map(|x| x.to_string()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::client_repositories::Client;
    use crate::helpers::mocks;
    use envtestkit::lock::lock_test;
    use envtestkit::set_env;
    use nanoid::nanoid;
    use std::error::Error;
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn it_serializes_a_config_and_adds_to_an_existing_client() {
        let mut client_repositories = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut client_repositories);

        let json_string = serialize_config(
            Option::from(&mut client_repositories.clone()),
            Option::from(&mut vec![client_repositories.clone()]),
        )
        .unwrap();

        let constructed_client_repos: ConfigurationDoc =
            serde_json::from_str(&json_string).unwrap();

        //before
        assert_eq!(
            &client_repositories
                .repositories
                .as_ref()
                .unwrap()
                .iter()
                .len(),
            &1
        );
        //after
        assert_eq!(
            &constructed_client_repos[0]
                .repositories
                .as_ref()
                .unwrap()
                .iter()
                .len(),
            &2
        );
    }

    #[test]
    fn it_serializes_a_config_and_adds_a_new_client() {
        let mut client_repositories = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut client_repositories);

        let mut deserialized_config = vec![ClientRepositories {
            client: Some(Client {
                id: nanoid!(),
                client_name: "New client".to_string(),
                client_address: "Somewhere".to_string(),
                client_contact_person: "Jim Jones".to_string(),
            }),
            user: None,
            repositories: None,
            ..Default::default()
        }];

        let length_before = &deserialized_config.len();

        let json_string = serialize_config(
            Option::from(&mut client_repositories),
            Option::Some(&mut deserialized_config),
        )
        .unwrap();

        let constructed_client_repos: ConfigurationDoc =
            serde_json::from_str(&json_string).unwrap();

        //before
        assert_eq!(length_before, &1);
        //after
        assert_eq!(&constructed_client_repos.len(), &2);
    }

    #[test]
    fn get_filepath_returns_path_with_file_name() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "false");

        let path_buf = PathBuf::from("/path/to/usr");
        assert_eq!(get_filepath(path_buf).unwrap(), "/path/to/usr/.autolog.txt");
    }

    #[test]
    fn get_home_path_should_return_a_path() {
        let path_buf = get_home_path();
        let path = path_buf.to_str().unwrap();

        assert!(Path::new(path).exists());
    }

    #[test]
    fn read_file_returns_a_buffer() {
        #[derive(Clone)]
        struct MockPrompt {}

        impl Onboarding for MockPrompt {
            fn onboarding(&mut self, _new_user: bool) -> Result<(), Box<dyn Error>> {
                assert!(false);
                Ok(())
            }
        }

        let mut mock_prompt = MockPrompt {};

        let mut buffer = String::new();

        read_file(&mut buffer, "./testing-utils/.hello.txt", &mut mock_prompt).unwrap();

        assert_eq!(buffer.trim(), "hello");
    }

    #[test]
    fn read_file_calls_the_error_function() {
        #[derive(Clone)]
        struct MockPrompt {}

        impl Onboarding for MockPrompt {
            fn onboarding(&mut self, _new_user: bool) -> Result<(), Box<dyn Error>> {
                assert!(true);
                Ok(())
            }
        }

        let mut mock_prompt = MockPrompt {};

        let mut buffer = String::new();

        read_file(
            &mut buffer,
            "./testing-utils/.timesheet.txt",
            &mut mock_prompt,
        )
        .unwrap();
    }

    #[test]
    fn it_writes_a_config_file_when_file_exists() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

        let mut client_repositories = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut client_repositories);

        // creates mock directory that is destroyed when it goes out of scope
        let dir = tempfile::tempdir().unwrap();
        let mock_config_path = dir.path().join("my-temporary-note.txt");

        let file = File::create(&mock_config_path).unwrap();
        let string_path_from_temp_dir = mock_config_path.to_str().unwrap().to_owned();

        let json = serialize_config(Option::from(&mut client_repositories), None).unwrap();

        assert!(write_json_to_config_file(json, string_path_from_temp_dir).is_ok());

        drop(file);
        dir.close().unwrap();
    }

    #[test]
    fn it_throws_an_error_when_writing_config_if_file_doesnt_exist() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "false");

        let mut client_repositories = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut client_repositories);

        let json = serialize_config(Option::from(&mut client_repositories), None).unwrap();

        assert!(write_json_to_config_file(json, "./a/fake/path".to_string()).is_err());
    }

    #[test]
    fn it_finds_and_updates_a_client() {
        let mut client_repositories = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut client_repositories);

        let json = serialize_config(Option::from(&mut client_repositories.clone()), None).unwrap();
        let value: ConfigurationDoc = serde_json::from_str(&*json).unwrap();

        assert_eq!(
            value[0].repositories.as_ref().unwrap()[0]
                .client_contact_person
                .as_ref()
                .unwrap(),
            &"John Smith".to_string()
        );
    }
}
