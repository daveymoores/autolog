use crate::cli::RcHelpPrompt;
use crate::client_repositories::ClientRepositories;
use crate::link_builder;
use crate::repository::Repository;
use std::cell::{Ref, RefCell, RefMut};
use std::process;
use std::rc::Rc;

type OptionsTuple<'a> = (Option<&'a String>, Option<&'a String>, Option<&'a String>);

/// Creates and modifies the config file. Config does not directly hold the information
/// contained in the config file, but provides the various operations that can be
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
    fn fetch_interaction_data(
        mut client_repositories: RefMut<Vec<ClientRepositories>>,
        repository: Ref<Repository>,
    ) {
        client_repositories[0]
            .set_values(repository)
            .exec_generate_timesheets_from_git_history()
            .compare_logs_and_set_timesheets();
    }

    /// Find and update client if sheet exists, otherwise write a new one
    fn write_to_config_file(
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        deserialized_config: Option<&mut Vec<ClientRepositories>>,
    ) {
        // get path for where to write the config file
        let config_path = crate::file_reader::get_filepath(crate::file_reader::get_home_path());
        let json = crate::file_reader::serialize_config(
            Rc::clone(&client_repositories),
            deserialized_config,
        )
        .unwrap_or_else(|err| {
            eprintln!("Error serializing json: {}", err);
            std::process::exit(exitcode::CANTCREAT);
        });

        crate::file_reader::write_json_to_config_file(json, config_path).unwrap_or_else(|err| {
            eprintln!("Error writing data to file: {}", err);
            std::process::exit(exitcode::CANTCREAT);
        });
    }

    // Check for repo by path or by namespace
    fn check_for_client_or_repo_in_buffer<'a>(
        self,
        deserialized_config: &'a mut Vec<ClientRepositories>,
        repo_path: Option<&String>,
        repo_namespace: Option<&String>,
        client_name: Option<&String>,
    ) -> Result<(Option<&'a Repository>, Option<&'a ClientRepositories>), Box<dyn std::error::Error>>
    {
        // function should return either a repository, a client repository, or both
        let mut namespace: Option<String> = repo_namespace.map(|x| x.to_owned());

        if let Some(path) = repo_path {
            let mut temp_repository = Repository {
                repo_path: Option::from(path.to_owned()),
                ..Default::default()
            };

            // get namespace of working repository
            temp_repository
                .find_git_path_from_directory_from()?
                .find_namespace_from_git_path()?;

            namespace = temp_repository.namespace;
        }

        let mut option: (Option<&Repository>, Option<&ClientRepositories>) =
            (Option::None, Option::None);
        // if a client name is passed, get ClientRepositories from that
        // if this is true, repo_path and repo_namespace will be None
        if let Some(c) = client_name {
            for i in 0..deserialized_config.len() {
                if deserialized_config[i]
                    .client
                    .as_ref()
                    .unwrap()
                    .client_name
                    .to_lowercase()
                    == c.to_owned().to_lowercase()
                {
                    option = (Option::None, Option::from(&deserialized_config[i]));
                } else if i == &deserialized_config.len() - 1 {
                    // if the client is passed but not found
                    //TODO - if this happens it would be good to give options - i.e list of clients, and list of repos
                    eprintln!(
                        "The client, or client + namespace combination you passed has not be found.");
                    std::process::exit(exitcode::CANTCREAT);
                }
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

    fn check_for_config_file(
        self,
        options: OptionsTuple,
        buffer: &mut String,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    ) {
        // pass a prompt for if the config file doesn't exist
        crate::file_reader::read_data_from_config_file(buffer, prompt.clone()).unwrap_or_else(
            |err| {
                eprintln!("Error initialising timesheet-gen: {}", err);
                std::process::exit(exitcode::CANTCREAT);
            },
        );

        // if the buffer is empty, there is no existing file, user has been onboarded
        // and Repository state holds the data. Write this data to file.
        if buffer.is_empty() {
            Config::fetch_interaction_data(client_repositories.borrow_mut(), repository.borrow());
            Config::write_to_config_file(client_repositories, None);
            crate::help_prompt::HelpPrompt::show_write_new_config_success();
            return;
        }

        // ..if the there is an existing config file, check whether the (passed path or namespace) repository exists under any clients
        // if it does pass Repository values to Repository
        let mut deserialized_config: Vec<ClientRepositories> = serde_json::from_str(&buffer)
            .expect("Initialisation of ClientRepository struct from buffer failed");

        let repo_client_tuple = self
            .check_for_client_or_repo_in_buffer(
                &mut deserialized_config,
                options.0,
                options.1,
                options.2,
            )
            .unwrap_or_else(|err| {
                eprintln!("Error trying to read from config file: {}", err);
                std::process::exit(exitcode::DATAERR);
            });

        if repo_client_tuple.1.is_some() {
            // if it exists, get the client + repos and the repo we're editing
            // and update the git log data based on all repositories
            let ts_clone = repo_client_tuple.clone();

            // ...and fetch a new batch of interaction data
            client_repositories.borrow_mut()[0]
                .set_values_from_buffer(ts_clone.1.unwrap())
                .exec_generate_timesheets_from_git_history()
                .compare_logs_and_set_timesheets();

            // if it's been found, set the working repo to the timesheet struct as it may be operated on
            if ts_clone.0.is_some() {
                repository
                    .borrow_mut()
                    .set_values_from_buffer(ts_clone.0.unwrap());
            }
        } else {
            // if it doesn't, onboard them and check whether (passed path or namespace) repo
            // should exist under an existing client
            prompt
                .borrow_mut()
                .prompt_for_client_then_onboard(&mut deserialized_config)
                .unwrap_or_else(|err| {
                    eprintln!("Error adding repository to client: {}", err);
                    std::process::exit(exitcode::CANTCREAT);
                });

            // ...and fetch a new batch of interaction data
            Config::fetch_interaction_data(client_repositories.borrow_mut(), repository.borrow());
            Config::write_to_config_file(
                client_repositories,
                Option::from(&mut deserialized_config),
            );
            crate::help_prompt::HelpPrompt::show_write_new_repo_success();
        }
    }
}

pub trait Init {
    /// Generate a config file with user variables
    fn init(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    );
}

impl Init for Config {
    fn init(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    ) {
        // try to read config file. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.check_for_config_file(
            (Option::from(&options[0]), Option::None, Option::None),
            &mut buffer,
            Rc::clone(&repository),
            client_repositories,
            prompt,
        );

        crate::help_prompt::HelpPrompt::repo_already_initialised();
    }
}

pub trait Make {
    /// Edit a day entry within the repository
    fn make(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    );
}

impl Make for Config {
    #[tokio::main]
    async fn make(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    ) {
        // try to read config file. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.check_for_config_file(
            (
                Option::from(&".".to_string()),
                Option::None,
                Option::from(&options[0]),
            ),
            &mut buffer,
            Rc::clone(&repository),
            Rc::clone(&client_repositories),
            prompt.clone(),
        );

        if crate::utils::config_file_found(&mut buffer) {
            prompt
                .borrow_mut()
                .add_project_numbers(Rc::clone(&client_repositories))
                .unwrap_or_else(|err| {
                    eprintln!("Error parsing project number: {}", err);
                    std::process::exit(exitcode::CANTCREAT);
                });
            // generate timesheet-gen.io link using existing config
            // TODO - this shouldn't build if there are no repositories under the client
            link_builder::build_unique_uri(Rc::clone(&client_repositories), options)
                .await
                .unwrap_or_else(|err| {
                    eprintln!("Error building unique link: {}", err);
                    std::process::exit(exitcode::CANTCREAT);
                });
        }
    }
}

pub trait Edit {
    /// Generate a config file with user variables
    fn edit(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    );
}

impl Edit for Config {
    fn edit(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    ) {
        // try to read config file. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.check_for_config_file(
            (Option::None, Option::from(&options[0]), Option::None),
            &mut buffer,
            Rc::clone(&repository),
            Rc::clone(&client_repositories),
            Rc::clone(&prompt),
        );

        if crate::utils::config_file_found(&mut buffer) {
            // otherwise lets set the repository struct values
            // and fetch a new batch of interaction data
            repository
                .borrow_mut()
                .update_hours_on_month_day_entry(&options)
                .unwrap_or_else(|err| {
                    eprintln!("Error editing timesheet: {}", err);
                    process::exit(exitcode::DATAERR);
                });

            client_repositories.borrow_mut()[0]
                .set_values(repository.borrow())
                .exec_generate_timesheets_from_git_history()
                .compare_logs_and_set_timesheets();

            Config::write_to_config_file(client_repositories, None);
            crate::help_prompt::HelpPrompt::show_edited_config_success();
        }
    }
}

pub trait Remove {
    /// Update client or repository details
    fn remove(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    );
}

impl Remove for Config {
    fn remove(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    ) {
        // try to read config file. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.check_for_config_file(
            (
                Option::None,
                Option::from(&options[1]),
                Option::from(&options[0]),
            ),
            &mut buffer,
            Rc::clone(&repository),
            Rc::clone(&client_repositories),
            Rc::clone(&prompt),
        );

        // Find repo or client and remove them from config file
        if crate::utils::config_file_found(&mut buffer) {
            let mut deserialized_config: Vec<ClientRepositories> =
                serde_json::from_str(&mut buffer)
                    .expect("Initialisation of ClientRepository struct from buffer failed");

            prompt
                .borrow_mut()
                .prompt_for_client_repo_removal(&mut deserialized_config, options)
                .expect("Remove failed");

            // pass modified config as new client_repository and thus write it straight to file
            Config::write_to_config_file(Rc::new(RefCell::new(deserialized_config)), None);
        }
    }
}

pub trait Update {
    /// Update client or repository details
    fn update(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    );
}

impl Update for Config {
    fn update(
        &self,
        options: Vec<Option<String>>,
        repository: Rc<RefCell<Repository>>,
        client_repositories: Rc<RefCell<Vec<ClientRepositories>>>,
        prompt: RcHelpPrompt,
    ) {
        // try to read config file. Write a new one if it doesn't exist
        let mut buffer = String::new();
        self.check_for_config_file(
            (Option::None, Option::None, Option::from(&options[0])),
            &mut buffer,
            Rc::clone(&repository),
            client_repositories,
            Rc::clone(&prompt),
        );

        if crate::utils::config_file_found(&mut buffer) {}
    }
}

#[cfg(test)]
mod tests {
    use crate::client_repositories::ClientRepositories;
    use crate::config::{Config, New};
    use crate::repository::Repository;
    use std::cell::RefCell;

    fn create_mock_client_repository(client_repository: &mut ClientRepositories) {
        let repo = RefCell::new(Repository {
            client_name: Option::from("alphabet".to_string()),
            client_address: Option::from("Spaghetti Way, USA".to_string()),
            client_contact_person: Option::from("John Smith".to_string()),
            name: Option::from("Jim Jones".to_string()),
            email: Option::from("jim@jones.com".to_string()),
            namespace: Option::from("timesheet-gen".to_string()),
            ..Default::default()
        });

        client_repository.set_values(repo.borrow());
    }

    #[test]
    fn it_checks_for_repo_in_buffer_by_path_and_returns_a_tuple() {
        let mut deserialized_config = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut deserialized_config);

        let config: Config = Config::new();

        if let Some(repository) = config
            .check_for_client_or_repo_in_buffer(
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
                "timesheet-gen".to_string()
            )
        }
    }

    #[test]
    fn it_checks_for_repo_in_buffer_by_namespace_and_returns_a_tuple() {
        let mut deserialized_config = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut deserialized_config);

        let config: Config = Config::new();

        if let Some(repository) = config
            .check_for_client_or_repo_in_buffer(
                &mut vec![deserialized_config],
                Option::None,
                Option::from(&"timesheet-gen".to_string()),
                Option::None,
            )
            .unwrap()
            .0
        {
            assert_eq!(
                *repository.namespace.as_ref().unwrap(),
                "timesheet-gen".to_string()
            )
        }
    }

    #[test]
    fn it_checks_for_repo_in_buffer_by_client_and_returns_a_tuple() {
        let mut deserialized_config = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut deserialized_config);

        let config: Config = Config::new();

        if let Some(client_repo) = config
            .check_for_client_or_repo_in_buffer(
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
