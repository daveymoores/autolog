use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::Repository;
use crate::interface::help_prompt::ConfigurationDoc;
use crate::interface::help_prompt::HelpPrompt;
use crate::interface::help_prompt::Onboarding;
use crate::utils::db::db_reader;
use crate::utils::exit_process;
use crate::utils::link::link_builder;
use semver::Version;
use std::process;

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
    fn write_to_db(config_doc: &ConfigurationDoc) {
        match db_reader::save_config_doc_to_db(config_doc) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Error writing to database: {}", err);
                std::process::exit(exitcode::CANTCREAT);
            }
        }
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
                // Skip clients without repositories
                if let Some(repositories) = client.repositories.as_ref() {
                    if !repositories.is_empty() {
                        option = match repositories.iter().find(|repository| {
                            // Handle repositories without a namespace
                            if let Some(repo_namespace) = &repository.namespace {
                                repo_namespace.to_lowercase()
                                    == namespace.as_ref().unwrap().to_lowercase()
                            } else {
                                false // Skip repositories without a namespace
                            }
                        }) {
                            Some(repository) => (Option::from(repository), Option::from(client)),
                            None => option,
                        };
                    }
                }
                // If client has no repositories, keep the current option value unchanged
            }
        }

        Ok(option)
    }

    async fn check_for_homebrew_update(self) -> Result<(), Box<dyn std::error::Error>> {
        // Get the current version from Cargo.toml
        let current_version_str = env!("CARGO_PKG_VERSION");
        let current_version_str = current_version_str.trim_start_matches('v');
        let current_version = Version::parse(current_version_str)?;

        // Check if we should check for updates based on the cache
        let should_check = crate::utils::db::db_reader::should_check_for_updates()?;

        if should_check {
            // Cache is stale or non-existent, do a fresh check from Homebrew
            let output = tokio::process::Command::new("brew")
                .args(["info", "--json", "autolog"])
                .output()
                .await?;

            if !output.status.success() {
                return Err(format!(
                    "Failed to check homebrew: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }

            // Parse the JSON output from brew
            let output_str = String::from_utf8_lossy(&output.stdout);
            let brew_info: Vec<serde_json::Value> = serde_json::from_str(&output_str)?;

            if brew_info.is_empty() {
                return Err("No homebrew information found for autolog".into());
            }

            // Extract the latest version
            if let Some(version_str) = brew_info[0]
                .get("versions")
                .and_then(|v| v.get("stable"))
                .and_then(|v| v.as_str())
            {
                // Remove any 'v' prefix if present
                let version_str = version_str.trim_start_matches('v');

                // Update the cache with the latest version
                crate::utils::db::db_reader::update_version_cache(version_str)?;

                match Version::parse(version_str) {
                    Ok(latest_version) => {
                        // Compare versions semantically
                        if latest_version > current_version {
                            println!(
                                "A new version of autolog is available: {}. You are using {}. Please update using `brew upgrade autolog`.",
                                version_str, current_version_str
                            );
                        }
                        Ok(())
                    }
                    Err(e) => {
                        // If we can't parse the version, just log it but don't fail
                        eprintln!("Failed to parse version '{}': {}", version_str, e);
                        Ok(())
                    }
                }
            } else {
                Err("Could not find version information in homebrew output".into())
            }
        } else {
            // Use cached version
            if let Some(cached_version) = crate::utils::db::db_reader::get_cached_version()? {
                match Version::parse(&cached_version) {
                    Ok(latest_version) => {
                        // Compare versions semantically
                        if latest_version > current_version {
                            println!(
                                "A new version of autolog is available: {}. You are using {}. Please update using `brew upgrade autolog`.",
                                cached_version, current_version_str
                            );
                        }
                        Ok(())
                    }
                    Err(_) => {
                        // Just silently ignore parsing errors for cached versions
                        Ok(())
                    }
                }
            } else {
                // No cache available, shouldn't happen but handle gracefully
                Ok(())
            }
        }
    }

    async fn check_for_update(self) -> Result<(), Box<dyn std::error::Error>> {
        match self.check_for_homebrew_update().await {
            Ok(_) => Ok(()),
            Err(e) => {
                // Fall back to displaying the current version if homebrew check fails
                eprintln!("Failed to check for updates: {}", e);
                println!(
                    "You are using autolog version {}",
                    env!("CARGO_PKG_VERSION")
                );
                Ok(())
            }
        }
    }

    fn find_or_create_db(self, prompt: &mut HelpPrompt) -> ConfigurationDoc {
        // Try to load existing config from the database
        match db_reader::load_config_doc_from_db() {
            Ok(doc) if !doc.is_empty() => {
                // Database exists and has data
                doc
            }
            Ok(_) | Err(_) => {
                // Either database is empty or doesn't exist
                // In both cases, we need to create new configuration
                eprintln!("Creating new autolog configuration");

                // Run onboarding
                self.create_new_configuration(prompt)
            }
        }
    }

    fn create_new_configuration(&self, prompt: &mut HelpPrompt) -> ConfigurationDoc {
        prompt.onboarding(true).unwrap_or_else(|err| {
            eprintln!("Error during onboarding: {}", err);
            std::process::exit(exitcode::CANTCREAT);
        });

        let mut repository = prompt.repository().clone();
        let mut client_repositories = prompt.client_repositories().clone();

        // Create a new configuration with the user data
        Config::fetch_interaction_data(&mut client_repositories, &mut repository);
        // Save to database
        let new_config = vec![client_repositories];
        match db_reader::save_config_doc_to_db(&new_config) {
            Ok(_) => {
                crate::interface::help_prompt::HelpPrompt::show_write_new_config_success();
                new_config
            }
            Err(err) => {
                eprintln!("Error initialising autolog: {}", err);
                std::process::exit(exitcode::CANTCREAT);
            }
        }
    }
}

pub trait Init {
    /// Generate a db with user variables
    fn init(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt);
}

impl Init for Config {
    #[tokio::main]
    async fn init(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        if let Err(e) = self.check_for_update().await {
            eprintln!("Failed to check for updates: {}", e);
        }

        // Load or create the database
        let mut config_doc = self.find_or_create_db(prompt);

        // If config_doc is not empty, check if the repository exists
        if !config_doc.is_empty() {
            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut config_doc,
                    Option::from(&options[0]),
                    Option::None,
                    Option::None,
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from database: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            if found_repo.is_some() & found_client_repo.is_some() {
                crate::interface::help_prompt::HelpPrompt::repo_already_initialised();
            } else {
                // Onboard new repository and allow user to choose a client or create a new one
                prompt
                    .prompt_for_client_then_onboard(&mut config_doc)
                    .unwrap_or_else(|err| {
                        eprintln!("Error adding repository to client: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    });

                let mut client_repositories = prompt.client_repositories().clone();
                let mut repository = prompt.repository().clone();

                // Fetch interaction data
                Config::fetch_interaction_data(&mut client_repositories, &mut repository);

                // check if this is a new client
                let client_id = client_repositories.get_client_id();
                let mut client_exists = false;

                // Update the client repository in the config document
                for client in config_doc.iter_mut() {
                    if client.get_client_id() == client_id {
                        *client = client_repositories.clone();
                        client_exists = true;
                        break;
                    }
                }

                // If this is a new client, add it to config_doc
                if !client_exists {
                    config_doc.push(client_repositories.clone());
                }

                // Write updated config back to database
                Config::write_to_db(&config_doc);
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
        if let Err(e) = self.check_for_update().await {
            eprintln!("Failed to check for updates: {}", e);
        }

        let current_repo_path = db_reader::get_canonical_path(".");
        let mut config_doc = self.find_or_create_db(prompt);

        if !config_doc.is_empty() {
            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut config_doc,
                    Option::from(&current_repo_path),
                    Option::None,
                    Option::from(&options[0]),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from database: {}", err);
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

                // Generate autolog.dev link using existing config
                link_builder::build_unique_uri(prompt.client_repositories(), options)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("Error building unique link: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    });

                // Update the client repository in the config document
                let client_id = prompt.client_repositories().get_client_id();
                for client in config_doc.iter_mut() {
                    if client.get_client_id() == client_id {
                        *client = prompt.client_repositories().clone();
                        break;
                    }
                }

                // Write to database
                Config::write_to_db(&config_doc);
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
    #[tokio::main]
    async fn edit(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        if let Err(e) = self.check_for_update().await {
            eprintln!("Failed to check for updates: {}", e);
        }

        // Load or create the database, getting a ConfigurationDoc directly
        let mut config_doc = self.find_or_create_db(prompt);

        if !config_doc.is_empty() {
            // Find the repository to edit by namespace
            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut config_doc,
                    Option::None,
                    Option::from(&options[0]),
                    Option::None,
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from database: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            // Set the prompt with found values to prepare for editing
            Self::set_prompt_with_found_values(prompt, found_repo, found_client_repo);

            if found_client_repo.is_some() {
                // Update the hours in the repository's timesheet
                prompt
                    .repository()
                    .update_hours_on_month_day_entry(&options)
                    .unwrap_or_else(|err| {
                        eprintln!("Error editing timesheet: {}", err);
                        process::exit(exitcode::DATAERR);
                    });

                // Clone repository to update the client repositories
                let mut repository_clone = prompt.repository().clone();

                // Update timesheet data in the client repositories
                prompt
                    .client_repositories()
                    .set_values(&mut repository_clone)
                    .exec_generate_timesheets_from_git_history()
                    .compare_logs_and_set_timesheets();

                // Find and update the matching client repository in the config document
                let client_id = prompt.client_repositories().get_client_id();
                let mut updated = false;

                for client in config_doc.iter_mut() {
                    if client.get_client_id() == client_id {
                        *client = prompt.client_repositories().clone();
                        updated = true;
                        break;
                    }
                }

                // If the client wasn't found, add it to the config
                if !updated {
                    config_doc.push(prompt.client_repositories().clone());
                }

                // Write the updated configuration back to the database
                match db_reader::save_config_doc_to_db(&config_doc) {
                    Ok(_) => {
                        crate::interface::help_prompt::HelpPrompt::show_edited_config_success();
                    }
                    Err(err) => {
                        eprintln!("Error writing to database: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    }
                }
            } else {
                crate::interface::help_prompt::HelpPrompt::client_or_repository_not_found();
            }
        }
    }
}

pub trait Remove {
    /// Update client or repository details
    fn remove(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt);
}

impl Remove for Config {
    #[tokio::main]
    async fn remove(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        if let Err(e) = self.check_for_update().await {
            eprintln!("Failed to check for updates: {}", e);
        }

        // Load or create the database, getting a ConfigurationDoc directly
        let mut config_doc = self.find_or_create_db(prompt);

        if !config_doc.is_empty() {
            // Find the repository or client to remove
            let (_found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut config_doc,
                    Option::None,
                    Option::from(&options[1]),
                    Option::from(&options[0]),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from database: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            if found_client_repo.is_some() {
                // Prompt for confirmation and handle removal
                prompt
                    .prompt_for_client_repo_removal(options, &mut config_doc)
                    .unwrap_or_else(|err| {
                        eprintln!("Error during removal: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    });

                // If there are no clients left, delete the entire database
                if config_doc.is_empty() {
                    match crate::utils::db::db_reader::delete_db() {
                        Ok(_) => {
                            println!("Database removed as it's now empty.");
                            exit_process();
                            return;
                        }
                        Err(err) => {
                            eprintln!("Failed to remove empty database: {}", err);
                            std::process::exit(exitcode::CANTCREAT);
                        }
                    }
                }

                // Write the updated configuration back to the database
                match db_reader::save_config_doc_to_db(&config_doc) {
                    Ok(_) => {
                        println!("Successfully removed the requested item.");
                    }
                    Err(err) => {
                        eprintln!("Error writing to database: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    }
                }
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
    #[tokio::main]
    async fn update(&self, options: Vec<Option<String>>, prompt: &mut HelpPrompt) {
        if let Err(e) = self.check_for_update().await {
            eprintln!("Failed to check for updates: {}", e);
        }

        // Load or create the database, getting a ConfigurationDoc directly
        let mut config_doc = self.find_or_create_db(prompt);

        if !config_doc.is_empty() {
            // Find the repository or client to update by namespace or client name
            let (found_repo, found_client_repo) = self
                .find_client_or_repo_in_buffer(
                    &mut config_doc,
                    Option::None,
                    Option::from(&options[1]),
                    Option::from(&options[0]),
                )
                .unwrap_or_else(|err| {
                    eprintln!("Error trying to read from database: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

            // Set the prompt with found values to prepare for updating
            Self::set_prompt_with_found_values(prompt, found_repo, found_client_repo);

            if found_client_repo.is_some() {
                // Prompt user for updated information
                prompt.prompt_for_update(options).unwrap_or_else(|err| {
                    eprintln!("Update failed: {}", err);
                    std::process::exit(exitcode::DATAERR);
                });

                // Find and update the matching client repository in the config document
                let client_id = prompt.client_repositories().get_client_id();
                let mut updated = false;

                for client in config_doc.iter_mut() {
                    if client.get_client_id() == client_id {
                        // Replace the client with the updated version
                        *client = prompt.client_repositories().clone();
                        updated = true;
                        break;
                    }
                }

                // If the client wasn't found (unusual, but possible if IDs change during update),
                // add the updated version to the config
                if !updated {
                    config_doc.push(prompt.client_repositories().clone());
                }

                // Write the updated configuration back to the database
                match db_reader::save_config_doc_to_db(&config_doc) {
                    Ok(_) => {
                        crate::interface::help_prompt::HelpPrompt::show_updated_config_success();
                    }
                    Err(err) => {
                        eprintln!("Error writing to database: {}", err);
                        std::process::exit(exitcode::CANTCREAT);
                    }
                }
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
    #[tokio::main]
    async fn list(&self, prompt: &mut HelpPrompt) {
        if let Err(e) = self.check_for_update().await {
            eprintln!("Failed to check for updates: {}", e);
        }

        // Load or create the database, getting a ConfigurationDoc directly
        let config_doc = self.find_or_create_db(prompt);

        if !config_doc.is_empty() {
            // List all clients and their repositories
            prompt.list_clients_and_repos(config_doc);
        } else {
            println!("No clients or repositories found in the database.");
            println!("Use 'autolog init' to set up your first repository.");
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

    fn is_repo_in_configuration_doc(config: &ConfigurationDoc, namespace: &String) -> bool {
        config.iter().any(|client| {
            client.repositories.as_ref().unwrap().iter().any(|repo| {
                repo.namespace.as_ref().unwrap().to_lowercase() == namespace.to_lowercase()
            })
        })
    }

    fn is_client_in_configuration_doc(config: &ConfigurationDoc, client_name: &String) -> bool {
        config.iter().any(|client| {
            client.client.as_ref().unwrap().client_name.to_lowercase() == client_name.to_lowercase()
        })
    }

    #[test]
    fn it_removes_a_repository() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

        let namespace = "autolog".to_string();
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

        // Get initial configuration using find_or_create_db
        let before_config_doc = config.find_or_create_db(&mut prompt);

        // Verify repository exists before removal
        assert_eq!(
            is_repo_in_configuration_doc(&before_config_doc, &namespace),
            true
        );

        // Perform the removal
        config.remove(options, &mut prompt);

        // Check if the repository has been removed from the client_repositories in the prompt
        let client_repos = prompt.client_repositories();

        // Verify repository has been removed from the prompt
        let repo_exists = if let Some(repositories) = &client_repos.repositories {
            repositories.iter().any(|repo| {
                repo.namespace.as_ref().unwrap().to_lowercase() == namespace.to_lowercase()
            })
        } else {
            false
        };

        assert_eq!(
            repo_exists, false,
            "Repository should be removed from the prompt"
        );
    }

    #[test]
    fn it_removes_a_client() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

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

        // Get initial configuration using find_or_create_db
        let before_config_doc = config.find_or_create_db(&mut prompt);

        // Verify client exists before removal
        assert_eq!(
            is_client_in_configuration_doc(&before_config_doc, &client),
            true
        );

        // Perform the removal
        config.remove(options, &mut prompt);

        // Access the client repositories from prompt
        // The prompt should have the updated state after removal
        let client_repositories = prompt.client_repositories();

        // Check if the client repositories is now empty
        // If the client was removed and it was the only client, client_repositories should be empty
        assert!(
            client_repositories.client.is_none()
                || client_repositories.client.as_ref().unwrap().client_name != client,
            "Client should be removed from prompt"
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
