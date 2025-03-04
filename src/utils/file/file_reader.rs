use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::Repository;
use crate::interface::help_prompt::{ConfigurationDoc, Onboarding};
use crate::utils::is_test_mode;
use serde_json::json;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use tempfile::tempfile;

const CONFIG_FILE_NAME: &str = ".autolog";

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
    let config_data = match (client_repositories, deserialized_config) {
        // No client repos and no config - error case
        (None, None) => {
            eprintln!("Tried to create a JSON literal but nothing was passed");
            std::process::exit(exitcode::DATAERR);
        }

        // New client, no existing config
        (Some(client_repo), None) => {
            json!(vec![client_repo])
        }

        // No new client, just existing config
        (None, Some(config)) => {
            json!(config)
        }

        // Both new client and existing config
        (Some(client_repos), Some(config)) => merge_client_into_config(client_repos, config),
    };

    let json = serde_json::to_string(&config_data)?;
    Ok(json)
}

// Merges a client repository into the config, either adding new or updating existing client
fn merge_client_into_config(
    client_repos: &ClientRepositories,
    config: &mut ConfigurationDoc,
) -> serde_json::Value {
    // Get client details
    let client = client_repos.client.clone();
    let client_name = client.as_ref().map(|c| c.client_name.to_lowercase());

    if let Some(client_name) = &client_name {
        if is_client_in_config(client_name, config) {
            update_existing_client(client_repos, config);
        } else {
            add_new_client(client_repos, config);
        }
    } else {
        // Handle case where client_name is missing
        eprintln!("Client name is missing in repository");
        std::process::exit(exitcode::DATAERR);
    }

    json!(config)
}

// Checks if a client with the given name exists in the config
fn is_client_in_config(client_name: &str, config: &ConfigurationDoc) -> bool {
    config
        .iter()
        .any(|x| x.get_client_name().map(|n| n.to_lowercase()) == Some(client_name.to_string()))
}

// Updates an existing client with new repositories
fn update_existing_client(client_repos: &ClientRepositories, config: &mut ConfigurationDoc) {
    let client_name = client_repos
        .client
        .as_ref()
        .unwrap()
        .client_name
        .to_lowercase();
    let new_repos = client_repos.repositories.as_ref().unwrap();

    for config_client in config.iter_mut() {
        if config_client.get_client_name().map(|n| n.to_lowercase()) == Some(client_name.clone()) {
            // Update client details if needed
            config_client.client = client_repos.client.clone();
            config_client.user = client_repos.user.clone();
            config_client.approver = client_repos.approver.clone();
            config_client.requires_approval = client_repos.requires_approval;

            // Merge repositories
            let existing_repos = config_client.repositories.get_or_insert_with(Vec::new);
            merge_repositories(existing_repos, new_repos);
            break;
        }
    }
}

// Adds a completely new client to the config
fn add_new_client(client_repos: &ClientRepositories, config: &mut ConfigurationDoc) {
    config.push(client_repos.clone());
}

// Merges repositories, avoiding duplicates
fn merge_repositories(existing_repos: &mut Vec<Repository>, new_repos: &[Repository]) {
    for new_repo in new_repos {
        let exists = existing_repos
            .iter()
            .any(|r| is_same_repository(r, new_repo));

        if !exists {
            existing_repos.push(new_repo.clone());
        }
    }
}

// Determines if two repositories are the same
fn is_same_repository(repo1: &Repository, repo2: &Repository) -> bool {
    match (
        &repo1.namespace,
        &repo1.repo_path,
        &repo2.namespace,
        &repo2.repo_path,
    ) {
        (Some(ns1), Some(path1), Some(ns2), Some(path2)) => {
            ns1.to_lowercase() == ns2.to_lowercase() && path1 == path2
        }
        _ => false,
    }
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
    use crate::data::client_repositories::{Client, ClientRepositories, User};
    use crate::data::repository::Repository;
    use crate::helpers::mocks;
    use envtestkit::lock::lock_test;
    use envtestkit::set_env;
    use nanoid::nanoid;
    use std::error::Error;
    use std::ffi::OsString;
    use std::path::Path;

    // Helper function to create a test client with optional repositories
    fn test_client(name: &str, repos: Option<Vec<&str>>) -> ClientRepositories {
        let mut client = ClientRepositories {
            client: Some(Client {
                id: format!("{}_id", name),
                client_name: name.to_string(),
                client_address: format!("{} address", name),
                client_contact_person: format!("{} contact", name),
            }),
            user: Some(User {
                id: format!("{}_user_id", name),
                name: format!("{} user", name),
                email: format!("{}@example.com", name),
                is_alias: false,
                thumbnail: None,
            }),
            repositories: None,
            requires_approval: Some(false),
            approver: None,
        };

        if let Some(repo_names) = repos {
            client.repositories = Some(
                repo_names
                    .iter()
                    .map(|&name| Repository {
                        id: Some(format!("{}_id", name)),
                        namespace: Some(name.to_string()),
                        repo_path: Some(format!("/path/to/{}", name)),
                        git_path: Some(format!("/path/to/{}/.git", name)),
                        name: Some("Test User".to_string()),
                        email: Some("test@example.com".to_string()),
                        client_id: Some(format!("{}_id", name)),
                        client_name: Some(name.to_string()),
                        client_contact_person: Some("Test Contact".to_string()),
                        ..Default::default()
                    })
                    .collect(),
            );
        }

        client
    }

    #[test]
    fn test_client_detection() {
        let config = vec![test_client("Acme", None)];

        assert!(
            is_client_in_config("acme", &config),
            "Should find client case-insensitive"
        );
        assert!(
            !is_client_in_config("globex", &config),
            "Should not find nonexistent client"
        );
    }

    #[test]
    fn test_repository_comparison() {
        // Create repos with same namespace but different paths
        let repo1 = Repository {
            namespace: Some("project-a".to_string()),
            repo_path: Some("/path/1".to_string()),
            ..Default::default()
        };

        let repo2 = Repository {
            namespace: Some("Project-A".to_string()), // Different case
            repo_path: Some("/path/1".to_string()),
            ..Default::default()
        };

        let repo3 = Repository {
            namespace: Some("project-a".to_string()),
            repo_path: Some("/path/2".to_string()), // Different path
            ..Default::default()
        };

        assert!(
            is_same_repository(&repo1, &repo2),
            "Same repo, different case"
        );
        assert!(!is_same_repository(&repo1, &repo3), "Different paths");
    }

    #[test]
    fn test_repository_merging() {
        let mut existing = vec![Repository {
            namespace: Some("repo1".to_string()),
            repo_path: Some("/path/1".to_string()),
            ..Default::default()
        }];

        let new_repos = vec![
            Repository {
                namespace: Some("repo1".to_string()), // Duplicate
                repo_path: Some("/path/1".to_string()),
                ..Default::default()
            },
            Repository {
                namespace: Some("repo2".to_string()), // New
                repo_path: Some("/path/2".to_string()),
                ..Default::default()
            },
        ];

        merge_repositories(&mut existing, &new_repos);
        assert_eq!(existing.len(), 2, "Should only add non-duplicate repo");
    }

    #[test]
    fn test_client_operations() {
        // Test adding a new client
        let mut config = vec![test_client("acme", None)];
        add_new_client(&test_client("globex", None), &mut config);
        assert_eq!(config.len(), 2, "Should add new client");

        // Test updating a client
        let updated_client = test_client("acme", Some(vec!["new-repo"]));
        update_existing_client(&updated_client, &mut config);

        let acme_repos = &config[0].repositories.as_ref().unwrap();
        assert_eq!(acme_repos.len(), 1, "Should add the repository");
        assert_eq!(
            acme_repos[0].namespace.as_ref().unwrap(),
            "new-repo",
            "Should have correct namespace"
        );
    }

    #[test]
    fn test_serialize_config_scenarios() {
        // New client, no config
        let mut new_client = test_client("acme", Some(vec!["repo1"]));
        let result = serialize_config(Some(&mut new_client), None).unwrap();

        assert!(result.contains("acme"), "Should include client name");
        assert!(result.contains("repo1"), "Should include repo name");

        // Update existing client
        let mut config = vec![test_client("acme", Some(vec!["repo1"]))];
        let mut updated = test_client("acme", Some(vec!["repo2"]));

        let result = serialize_config(Some(&mut updated), Some(&mut config)).unwrap();

        assert!(result.contains("repo1"), "Should keep existing repo");
        assert!(result.contains("repo2"), "Should add new repo");

        // Count occurrences of "client_name":"acme" pattern
        let client_name_pattern = "\"client_name\":\"acme\"";
        let count = result.matches(client_name_pattern).count();
        assert_eq!(count, 1, "Should have only one client");
    }

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
        assert_eq!(get_filepath(path_buf).unwrap(), "/path/to/usr/.autolog");
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
