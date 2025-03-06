use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::Repository;
use crate::interface::help_prompt::ConfigurationDoc;
use crate::interface::help_prompt::HelpPrompt;
use crate::utils::exit_process;
use crate::utils::file::file_reader::serialize_config;
use crate::utils::link::link_builder;
extern crate google_calendar3 as calendar3;
use crate::utils::db::db_reader;
use calendar3::{hyper, hyper_rustls, oauth2};
use dirs::home_dir;
use std::path::PathBuf;
use std::process;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

/// Creates and modifies the  db Config does not directly hold the information
/// contained in the  db but provides the various operations that can be
/// performed on it. The data is a stored within the Repository struct.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {}

pub trait New {
    fn new() -> Self;
}

impl New for Config {
    fn new() -> Self {
        Config {}
    }
}

impl Config {
    fn update_client_repositories(
        new_client_repos: &mut ConfigurationDoc,
        deserialized_config: ConfigurationDoc,
        prompt: &mut HelpPrompt,
    ) {
        let client_id = prompt.client_repositories().get_client_id();

        for item in &deserialized_config {
            if item.get_client_id() == client_id {
                new_client_repos.push(prompt.client_repositories().clone())
            } else {
                new_client_repos.push(item.clone())
            }
        }
    }

    fn set_prompt_with_found_values(
        prompt: &mut HelpPrompt,
        found_repo: Option<&Repository>,
        found_client_repo: Option<&ClientRepositories>,
    ) {
        // ...and fetch a new batch of interaction data
        if let Some(found_client_repo) = found_client_repo {
            prompt
                .client_repositories()
                .set_values_from_buffer(found_client_repo)
                .exec_generate_timesheets_from_git_history()
                .compare_logs_and_set_timesheets();
        }

        // if it's been found, set the working repo to the timesheet struct as it may be operated on
        if let Some(found_repo) = found_repo {
            prompt.repository().set_values_from_buffer(found_repo);
        }
    }

    fn fetch_interaction_data(
        client_repositories: &mut ClientRepositories,
        repository: &mut Repository,
    ) {
        client_repositories
            .set_values(repository)
            .exec_generate_timesheets_from_git_history()
            .compare_logs_and_set_timesheets();
    }

    /// Find and update client if sheet exists, otherwise write a new one
    fn write_to_db(
        client_repositories: Option<&mut ClientRepositories>,
        deserialized_config: Option<&mut ConfigurationDoc>,
    ) {
        // Convert to JSON first (for now, to minimize changes)
        let json = match serialize_config(client_repositories, deserialized_config) {
            Ok(json) => json,
            Err(err) => {
                eprintln!("Error serializing configuration: {}", err);
                std::process::exit(exitcode::DATAERR);
            }
        };

        // Write to database
        crate::utils::db::db_reader::write_config_to_db(json).unwrap_or_else(|err| {
            eprintln!("Error writing to database: {}", err);
            std::process::exit(exitcode::CANTCREAT);
        });
    }

    // Check for repo by path or by namespace
    fn find_client_or_repo_in_buffer<'a>(
        self,
        deserialized_config: &'a mut ConfigurationDoc,
        repo_path: Option<&String>,
        repo_namespace: Option<&String>,
        client_name: Option<&String>,
    ) -> Result<(Option<&'a Repository>, Option<&'a ClientRepositories>), Box<dyn std::error::Error>>
    {
        let namespace: Option<String> = match repo_namespace {
            Some(ns) => Some(ns.to_owned()),
            None => {
                // Only try to get namespace from repository if no namespace was provided
                if let Some(path) = repo_path {
                    let mut temp_repository = Repository {
                        repo_path: Some(path.to_owned()),
                        ..Default::default()
                    };

                    temp_repository
                        .find_git_path_from_directory_from()?
                        .find_namespace_from_git_path()?;

                    temp_repository.namespace
                } else {
                    None
                }
            }
        };

        let mut option: (Option<&Repository>, Option<&ClientRepositories>) =
            (Option::None, Option::None);
        // if client_name is passed, find the client from the config
        // and set it to a value in the tuple
        if let Some(c) = client_name {
            let mut found = false;
            for i in 0..deserialized_config.len() {
                if let Some(client) = deserialized_config[i].get_client_name() {
                    if client.to_lowercase() == c.to_owned().to_lowercase() {
                        option = (Option::None, Option::from(&deserialized_config[i]));
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                //TODO - if the client is passed but not found
                //TODO - it would be good to give options - i.e list of clients, and list of repos
                eprintln!(
                    "The client, or client + namespace combination you passed has not be found."
                );
                std::process::exit(exitcode::CANTCREAT);
            }
        } else {
            // otherwise check whether any clients contain the namespace
            // and return the repository and the client
            for client in deserialized_config.iter() {
                option = match client
                    .repositories
                    .as_ref()
                    .unwrap()
                    .iter()
                    .find(|repository| {
                        repository.namespace.as_ref().unwrap().to_lowercase()
                            == namespace.as_ref().unwrap().to_lowercase()
                    }) {
                    Some(repository) => (Option::from(repository), Option::from(client)),
                    None => option,
                };
            }
        }

        Ok(option)
    }

    fn find_or_create_db(self, buffer: &mut String, prompt: &mut HelpPrompt) {
        // pass a prompt for if the  dbdoesn't exist
        crate::utils::db::db_reader::read_data_from_db(buffer, prompt).unwrap_or_else(|err| {
            eprintln!("Error initialising autolog: {}", err);
            std::process::exit(exitcode::CANTCREAT);
        });

        let mut repository = prompt.repository().clone();
        let mut client_repositories = prompt.client_repositories().clone();

        // if the buffer is empty, there is no existing file, user has been onboarded
        // and Repository state holds the data. Write this data to file.
        if buffer.is_empty() {
            Config::fetch_interaction_data(&mut client_repositories, &mut repository);
            Config::write_to_db(Option::Some(&mut client_repositories), None);
            crate::interface::help_prompt::HelpPrompt::show_write_new_config_success();
        }
    }

    async fn save_token(
        self,
        token: &oauth2::AccessToken,
        path: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let token_json = serde_json::to_string(token)?;
        let mut file = File::create(path).await?;
        file.write_all(token_json.as_bytes()).await?;
        Ok(())
    }

    async fn load_token(
        self,
        path: &PathBuf,
    ) -> Result<oauth2::AccessToken, Box<dyn std::error::Error>> {
        let token_json = tokio::fs::read_to_string(path).await?;
        let token: oauth2::AccessToken = serde_json::from_str(&token_json)?;
        Ok(token)
    }

    async fn create_authenticator(
        self,
        secret: oauth2::ApplicationSecret,
        token_path: PathBuf,
    ) -> oauth2::authenticator::Authenticator<
        hyper_rustls::HttpsConnector<hyper::client::HttpConnector>,
    > {
        oauth2::InstalledFlowAuthenticator::builder(
            secret,
            oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk(token_path) // Automatically save tokens to disk
        .build()
        .await
        .unwrap()
    }
}

pub trait Init {
    /// Generate a db with user variables
    fn init(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt);
}

impl Init for Config {
    fn init(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        // try to read db. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.find_or_create_db(&mut buffer, prompt);

        // ..if the there is an exis dbfile, check whether the (passed path or namespace) repository exists under any clients
        // if it does pass Repository values to Repository
        if crate::utils::config_file_found(&mut buffer) {
            let mut deserialized_config: ConfigurationDoc = serde_json::from_str(&buffer)
                .expect("Initialisation of ClientRepository struct from buffer failed");

            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut deserialized_config,
                    Option::from(&options[0]),
                    Option::None,
                    Option::None,
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from db: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            if found_repo.is_some() & found_client_repo.is_some() {
                crate::interface::help_prompt::HelpPrompt::repo_already_initialised();
            } else {
                // Otherwise onboard them and check whether (passed path or namespace) repo
                // should exist under an existing client
                prompt
                    .prompt_for_client_then_onboard(&mut deserialized_config)
                    .unwrap_or_else(|err| {
                        eprintln!("Error adding repository to client: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    });

                let mut client_repositories = prompt.client_repositories().clone();
                let mut repository = prompt.repository().clone();

                // ...and fetch a new batch of interaction data
                Config::fetch_interaction_data(&mut client_repositories, &mut repository);
                Config::write_to_db(
                    Option::Some(&mut client_repositories),
                    Option::from(&mut deserialized_config),
                );

                crate::interface::help_prompt::HelpPrompt::show_write_new_repo_success();
            }
        }
    }
}

pub trait Make {
    /// Edit a day entry within the repository
    fn make(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt);
}

impl Make for Config {
    #[tokio::main]
    async fn make(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        // try to read db. Write a new one if it doesn't exist
        let mut buffer = String::new();
        let current_repo_path = db_reader::get_canonical_path(".");

        self.find_or_create_db(&mut buffer, prompt);

        if crate::utils::config_file_found(&mut buffer) {
            let mut deserialized_config: ConfigurationDoc = serde_json::from_str(&buffer)
                .expect("Initialisation of ClientRepository struct from buffer failed");

            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut deserialized_config,
                    Option::from(&current_repo_path),
                    Option::None,
                    Option::from(&options[0]),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from db: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            Self::set_prompt_with_found_values(prompt, found_repo, found_client_repo);

            if found_client_repo.is_some() {
                prompt
                    .add_project_numbers()
                    .unwrap_or_else(|err| {
                        eprintln!("Error parsing project number: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    })
                    .prompt_for_manager_approval()
                    .unwrap_or_else(|err| {
                        eprintln!("Error setting manager approval: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    });

                // generate autolog.dev link using existing config
                link_builder::build_unique_uri(prompt.client_repositories(), options)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("Error building unique link: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    });

                Config::write_to_db(
                    Option::Some(prompt.client_repositories()),
                    Option::Some(&mut deserialized_config),
                );
            } else {
                crate::interface::help_prompt::HelpPrompt::client_or_repository_not_found();
            }
        }
    }
}

pub trait Edit {
    /// Generate a db withuser variables
    fn edit(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt);
}

impl Edit for Config {
    fn edit(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        // try to read db. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.find_or_create_db(&mut buffer, prompt);

        if crate::utils::config_file_found(&mut buffer) {
            let mut deserialized_config: ConfigurationDoc = serde_json::from_str(&buffer)
                .expect("Initialisation of ClientRepository struct from buffer failed");

            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut deserialized_config,
                    Option::None,
                    Option::from(&options[0]),
                    Option::None,
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from db: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            Self::set_prompt_with_found_values(prompt, found_repo, found_client_repo);

            if found_client_repo.is_some() {
                prompt
                    .repository()
                    .update_hours_on_month_day_entry(&options)
                    .unwrap_or_else(|err| {
                        eprintln!("Error editing timesheet: {}", err);
                        process::exit(exitcode::DATAERR);
                    });

                let mut repository_clone = prompt.repository().clone();

                prompt
                    .client_repositories()
                    .set_values(&mut repository_clone)
                    .exec_generate_timesheets_from_git_history()
                    .compare_logs_and_set_timesheets();

                let mut new_client_repos: ConfigurationDoc = vec![];
                Self::update_client_repositories(
                    &mut new_client_repos,
                    deserialized_config,
                    prompt,
                );

                Config::write_to_db(None, Option::Some(&mut new_client_repos));
                crate::interface::help_prompt::HelpPrompt::show_edited_config_success();
            } else {
                crate::interface::help_prompt::HelpPrompt::client_or_repository_not_found();
            }
        }
    }
}

pub trait Remove {
    /// Update client or repository details
    fn remove(
        &self,
        options: Vec<Option<String>>,
        prompt: &mut HelpPrompt,
        deserialized_config: &mut ConfigurationDoc,
    );
}

impl Remove for Config {
    fn remove(
        &self,
        options: Vec<Option<String>>,
        prompt: &mut HelpPrompt,
        deserialized_config: &mut ConfigurationDoc,
    ) {
        // try to read db. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.find_or_create_db(&mut buffer, prompt);

        // Find repo or client and remove them fro dbm
        if crate::utils::config_file_found(&mut buffer) {
            let config: ConfigurationDoc = serde_json::from_str(&buffer)
                .expect("Initialisation of ClientRepository struct from buffer failed");

            for item in &config {
                deserialized_config.push(item.clone());
            }

            let (_found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    deserialized_config,
                    Option::None,
                    Option::from(&options[1]),
                    Option::from(&options[0]),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from db: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            if found_client_repo.is_some() {
                prompt
                    .prompt_for_client_repo_removal(options, deserialized_config)
                    .expect("Remove failed");

                // if there are no clients, lets remove the file and next time will be onboarding
                //TODO - would be nice to improve this
                if deserialized_config.is_empty() {
                    crate::utils::db::db_reader::delete_db()
                        .expect("Con db empty so autolog tried to remove it. That failed.");
                    exit_process();
                    return;
                }

                // pass modified config as new client_repository and thus write it straight to file
                Config::write_to_db(None, Option::Some(deserialized_config));
            } else {
                crate::interface::help_prompt::HelpPrompt::client_or_repository_not_found();
            }
        }
    }
}

pub trait Update {
    /// Update client or repository details
    fn update(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt);
}

impl Update for Config {
    fn update(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        // try to read db. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.find_or_create_db(&mut buffer, prompt);

        if crate::utils::config_file_found(&mut buffer) {
            let mut deserialized_config: ConfigurationDoc = serde_json::from_str(&buffer)
                .expect("Initialisation of ClientRepository struct from buffer failed");

            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut deserialized_config,
                    Option::None,
                    Option::from(&options[1]),
                    Option::from(&options[0]),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from db: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            Self::set_prompt_with_found_values(prompt, found_repo, found_client_repo);

            if found_client_repo.is_some() {
                prompt.prompt_for_update(options).expect("Update failed");

                let mut new_client_repos = vec![];
                Self::update_client_repositories(
                    &mut new_client_repos,
                    deserialized_config,
                    prompt,
                );

                // pass modified config as new client_repository and thus write it straight to file
                Config::write_to_db(None, Option::Some(&mut new_client_repos));
                crate::interface::help_prompt::HelpPrompt::show_updated_config_success();
            } else {
                crate::interface::help_prompt::HelpPrompt::client_or_repository_not_found();
            }
        }
    }
}

pub trait List {
    /// List repositories under each client
    fn list(&self, prompt: &mut HelpPrompt);
}

impl List for Config {
    fn list(&self, prompt: &mut HelpPrompt) {
        // try to read db. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.find_or_create_db(&mut buffer, prompt);

        if crate::utils::config_file_found(&mut buffer) {
            let deserialized_config: ConfigurationDoc = serde_json::from_str(&buffer)
                .expect("Initialisation of ClientRepository struct from buffer failed");

            prompt.list_clients_and_repos(deserialized_config);
        }
    }
}

pub trait Link {
    /// List repositories under each client
    fn link(&self, options: Vec<Option<String>>);
}

impl Link for Config {
    #[tokio::main]
    async fn link(&self, options: Vec<Option<String>>) {
        let home_dir = home_dir().expect("Could not find home directory");
        let token_path = home_dir.join(".autolog.token");

        crate::interface::help_prompt::HelpPrompt::oauth2_authenticating();

        if let Some(service) = options.get(0).and_then(|s| s.as_deref()) {
            match service {
                "gcal" => {
                    // Load the credentials file
                    let secret = oauth2::read_application_secret("client_secret.json")
                        .await
                        .expect("client_secret.json file not found");

                    if token_path.exists() {
                        // Load the token from the file
                        let token = self
                            .load_token(&token_path)
                            .await
                            .expect("Failed to load token");

                        // Check if the token is valid
                        if token.is_expired() {
                            crate::interface::help_prompt::HelpPrompt::show_oauth2_expired_token(
                                service,
                            );
                            let auth = self.create_authenticator(secret, token_path.clone()).await;
                            let token = auth
                                .token(&["https://www.googleapis.com/auth/calendar"])
                                .await
                                .unwrap();
                            self.save_token(&token, &token_path)
                                .await
                                .expect("Failed to save token");
                            crate::interface::help_prompt::HelpPrompt::show_oauth2_success(service)
                        } else {
                            crate::interface::help_prompt::HelpPrompt::oauth2_link_valid(service)
                        }
                    } else {
                        // Create a new token
                        let auth = self.create_authenticator(secret, token_path.clone()).await;
                        let token = auth
                            .token(&["https://www.googleapis.com/auth/calendar"])
                            .await
                            .unwrap();
                        self.save_token(&token, &token_path)
                            .await
                            .expect("Failed to save token");
                        crate::interface::help_prompt::HelpPrompt::show_oauth2_success(service)
                    }
                }
                _ => {
                    println!("Unsupported service: {}", service);
                }
            }
        } else {
            println!("No service specified.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Config, Edit, New, Remove};
    use crate::data::client_repositories::ClientRepositories;
    use crate::data::repository::Repository;
    use crate::helpers::mocks;
    use crate::interface::help_prompt::ConfigurationDoc;
    use envtestkit::lock::lock_test;
    use envtestkit::set_env;
    use serde_json::{Number, Value};
    use std::ffi::OsString;

    #[test]
    fn it_modifies_the_hour_entry_in_a_client_repository_day_entry() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

        let config = Config::new();
        let options = vec![
            Option::from("autolog".to_string()),
            Option::from("20".to_string()),
            Option::from("1".to_string()),
            Option::from("11".to_string()),
            Option::from("2021".to_string()),
        ];

        let mut client_repos = ClientRepositories {
            ..Default::default()
        };

        let mut repo = Repository {
            ..Default::default()
        };

        let mut prompt =
            crate::interface::help_prompt::HelpPrompt::new(&mut repo, &mut client_repos);

        config.edit(options, &mut prompt);

        let month = prompt
            .repository()
            .timesheet
            .as_ref()
            .unwrap()
            .get("2021")
            .unwrap()
            .get("11")
            .unwrap()
            .clone();

        let hour_value = month[0].get("hours").unwrap().to_string();
        let edited_value = month[0].get("user_edited").unwrap();

        assert_eq!(hour_value, Number::from_f64(20.0).unwrap().to_string());
        assert_eq!(edited_value, &Value::Bool(true));
    }

    fn is_repo_in_deserialized_config(config: &ConfigurationDoc, namespace: &String) -> bool {
        config.iter().any(|client| {
            client.repositories.as_ref().unwrap().iter().any(|repo| {
                repo.namespace.as_ref().unwrap().to_lowercase() == namespace.to_lowercase()
            })
        })
    }

    fn is_client_in_deserialized_config(config: &ConfigurationDoc, client_name: &String) -> bool {
        config.iter().any(|client| {
            client.client.as_ref().unwrap().client_name.to_lowercase() == client_name.to_lowercase()
        })
    }

    #[test]
    fn it_removes_a_repository() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

        let mut buffer = String::new();
        let namespace = "pila-app".to_string();
        let config = Config::new();
        let options = vec![
            Option::from("apple".to_string()),
            Option::from(namespace.clone()),
        ];

        let mut client_repos = ClientRepositories {
            ..Default::default()
        };

        let mut repo = Repository {
            ..Default::default()
        };

        let mut prompt =
            crate::interface::help_prompt::HelpPrompt::new(&mut repo, &mut client_repos);

        crate::utils::db::db_reader::read_data_from_db(&mut buffer, &mut prompt)
            .expect("Read of test data failed");

        let before_deserialized_config: ConfigurationDoc = serde_json::from_str(&mut buffer)
            .expect("Initialisation of ClientRepository struct from buffer failed");

        assert_eq!(
            is_repo_in_deserialized_config(&before_deserialized_config, &namespace),
            true
        );

        // internally this will find the same test db as above
        let mut after_deserialized_config: ConfigurationDoc = vec![];

        config.remove(options, &mut prompt, &mut after_deserialized_config);

        assert_eq!(
            is_repo_in_deserialized_config(&after_deserialized_config, &namespace),
            false
        );
    }

    #[test]
    fn it_removes_a_client() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

        let mut buffer = String::new();
        let client = "apple".to_string();
        let config = Config::new();
        let options = vec![Option::from(client.clone()), Option::None];

        let mut client_repos = ClientRepositories {
            ..Default::default()
        };

        let mut repo = Repository {
            ..Default::default()
        };

        let mut prompt =
            crate::interface::help_prompt::HelpPrompt::new(&mut repo, &mut client_repos);

        crate::utils::db::db_reader::read_data_from_db(&mut buffer, &mut prompt)
            .expect("Read of test data failed");

        let before_deserialized_config: ConfigurationDoc = serde_json::from_str(&mut buffer)
            .expect("Initialisation of ClientRepository struct from buffer failed");

        assert_eq!(
            is_client_in_deserialized_config(&before_deserialized_config, &client),
            true
        );

        // internally this will find the same test db as above
        let mut after_deserialized_config: ConfigurationDoc = vec![];

        config.remove(options, &mut prompt, &mut after_deserialized_config);

        assert_eq!(
            is_client_in_deserialized_config(&after_deserialized_config, &client),
            false
        );
    }

    #[test]
    fn it_checks_for_repo_in_buffer_by_path_and_returns_a_tuple() {
        let mut deserialized_config = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut deserialized_config);

        let config: Config = Config::new();

        if let Some(repository) = config
            .find_client_or_repo_in_buffer(
                &mut vec![deserialized_config],
                Option::from(&".".to_string()),
                Option::None,
                Option::None,
            )
            .unwrap()
            .0
        {
            assert_eq!(
                *repository.namespace.as_ref().unwrap(),
                "autolog".to_string()
            )
        }
    }

    #[test]
    fn it_checks_for_repo_in_buffer_by_namespace_and_returns_a_tuple() {
        let mut deserialized_config = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut deserialized_config);

        let config: Config = Config::new();

        if let Some(repository) = config
            .find_client_or_repo_in_buffer(
                &mut vec![deserialized_config],
                Option::None,
                Option::from(&"autolog".to_string()),
                Option::None,
            )
            .unwrap()
            .0
        {
            assert_eq!(
                *repository.namespace.as_ref().unwrap(),
                "autolog".to_string()
            )
        }
    }

    #[test]
    fn it_checks_for_repo_in_buffer_by_client_and_returns_a_tuple() {
        let mut deserialized_config = ClientRepositories {
            ..Default::default()
        };

        mocks::create_mock_client_repository(&mut deserialized_config);

        let config: Config = Config::new();

        if let Some(client_repo) = config
            .find_client_or_repo_in_buffer(
                &mut vec![deserialized_config],
                Option::None,
                Option::None,
                Option::from(&"alphabet".to_string()),
            )
            .unwrap()
            .1
        {
            assert_eq!(
                *client_repo.client.as_ref().unwrap().client_name,
                "alphabet".to_string()
            )
        }
    }
}
