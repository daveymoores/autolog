use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::Repository;
use crate::utils::file::file_reader;
use ansi_term::Style;
use ascii_table::AsciiTable;
/// Help prompt handles all of the interactions with the user.
/// It writes to the std output, and returns input data or a boolean
use dialoguer::{Confirm, Editor, Input, Select};
use nanoid::nanoid;
use regex::Regex;
use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;

pub type ConfigurationDoc = Vec<ClientRepositories>;
pub type RCRepository = Rc<RefCell<Repository>>;
pub type RCClientRepositories = Rc<RefCell<ClientRepositories>>;

//TODO - consider using termion https://docs.rs/termion/1.5.6/termion/
#[derive(Debug, Clone)]
pub struct HelpPrompt {
    repository: RCRepository,
    client_repositories: RCClientRepositories,
}

pub trait Onboarding {
    fn onboarding(&self, new_user: bool) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait ExistingClientOnboarding {
    fn existing_client_onboarding(
        &self,
        deserialized_config: &mut ConfigurationDoc,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

impl Onboarding for HelpPrompt {
    fn onboarding(&self, new_user: bool) -> Result<(), Box<dyn Error>> {
        self.confirm_repository_path(new_user)?
            .search_for_repository_details(Option::None)?
            .add_client_details()?
            .show_details();
        Ok(())
    }
}

impl ExistingClientOnboarding for HelpPrompt {
    fn existing_client_onboarding(
        &self,
        deserialized_config: &mut ConfigurationDoc,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.confirm_repository_path(false)?
            .search_for_repository_details(Option::Some(deserialized_config))?
            .show_details();
        Ok(())
    }
}

impl HelpPrompt {
    pub fn new(repository: RCRepository, client_repositories: RCClientRepositories) -> Self {
        Self {
            repository,
            client_repositories,
        }
    }

    pub fn repo_already_initialised() {
        println!(
            "\u{1F916} autolog has already been initialised for this repository.\n\
    Try 'autolog make' to create your first timesheet \n\
    or 'autolog help' for more options."
        );
        std::process::exit(exitcode::OK);
    }

    pub fn show_write_new_config_success() {
        println!(
            "\n{}",
            Style::new()
                .bold()
                .paint("autolog initialised \u{1F389} \n")
        );
        println!(
            "Try 'autolog make' to create your first timesheet \n\
            or 'autolog help' for more options."
        );
        std::process::exit(exitcode::OK);
    }

    pub fn show_write_new_repo_success() {
        println!(
            "\n{}",
            Style::new()
                .bold()
                .paint("New repository added \u{1F389} \n")
        );
        println!(
            "Try 'autolog make' to create your first timesheet \n\
            or 'autolog help' for more options."
        );
        std::process::exit(exitcode::OK);
    }

    pub fn show_edited_config_success() {
        println!("\nautolog successfully edited \u{1F389}");
        crate::utils::exit_process();
    }

    pub fn show_updated_config_success() {
        println!("\nautolog successfully updated \u{1F389}");
        crate::utils::exit_process();
    }

    pub fn show_generating_timesheet_message(month_year_string: &str) {
        let text = Self::dim_text(&format!(
            "\n\u{1F916} Generating timesheet for {}...",
            month_year_string
        ));
        println!("{}", text);
    }

    pub fn client_or_repository_not_found() {
        println!("\n\u{1F916} Client or repository not found.");
        crate::utils::exit_process();
    }

    pub fn show_new_link_success(expire_time: i32, uri: &str) {
        Self::print_question(&format!(
            "Timesheet now available for {} minutes @ {} \u{1F389}",
            expire_time, uri
        ));
    }

    pub fn show_oauth2_success(service: &str) {
        println!("\n{} successfully authenticated \u{1F389}", service);
        crate::utils::exit_process();
    }

    pub fn show_oauth2_expired_token(service: &str) {
        println!(
            "\nToken for {} has expired. Please re-authenticate.",
            service
        );
        crate::utils::exit_process();
    }

    fn take_and_validate_email<'a>(initial_text: Option<&str>) -> futures::io::Result<String> {
        let text = initial_text.unwrap_or_default();

        Input::new()
            .with_initial_text(text)
            .validate_with(|input: &String| -> Result<(), &str> {
                let re = Regex::new(r"^([a-zA-Z0-9_\-.]+)@([a-zA-Z0-9_\-.]+)\.([a-zA-Z]{2,5})$")
                    .unwrap();
                if re.is_match(input) {
                    Ok(())
                } else {
                    Err("This is not a mail address")
                }
            })
            .interact_text()
    }

    pub fn prompt_for_update(
        &mut self,
        options: Vec<Option<String>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut client_repositories = self.client_repositories.borrow_mut();
        let mut repository = self.repository.borrow_mut();
        let client = client_repositories.client.as_ref().unwrap();
        let user = client_repositories.user.as_ref().unwrap();

        if options[1].is_some() {
            Self::print_question(&format!(
                "Updating project '{}' for client '{}'. What would you like to update?",
                &options[1].as_ref().unwrap(),
                &options[0].as_ref().unwrap()
            ));

            let opt = vec!["Namespace", "Repository path"];
            let selection: usize = Select::new().items(&opt).interact()?;
            let value = opt[selection];

            // changing the repo path will automatically update the repo username/email and namespace
            // but the namespace alias can be updated separately here
            match value {
                "Namespace" => {
                    let input: String = Input::new()
                        .with_initial_text(repository.namespace.as_ref().unwrap())
                        .interact_text()?;
                    repository.set_namespace_alias(input);
                }
                "Repository path" => {
                    let input: String = Input::new()
                        .with_initial_text(repository.repo_path.as_ref().unwrap())
                        .interact_text()?;
                    repository
                        .set_repo_path(input)
                        .find_repository_details_from()?;
                }
                _ => {}
            };
        } else {
            Self::print_question(&format!(
                "Updating client '{}'. What would you like to update?",
                &options[0].as_ref().unwrap()
            ));

            let opt = vec![
                "Approver name",
                "Approver email",
                "Client company name",
                "Client contact person",
                "Client address",
                "User name",
                "User email",
            ];
            let selection: usize = Select::new().items(&opt).interact()?;
            let value = opt[selection];

            match value {
                "Approver name" => {
                    println!("Approver's name");
                    let approvers_name = match &client_repositories.approver {
                        None => String::new(),
                        Some(approver) => match &approver.approvers_name {
                            None => String::new(),
                            Some(approvers_name) => approvers_name.to_string(),
                        },
                    };

                    let input: String = Input::new()
                        .with_initial_text(approvers_name)
                        .interact_text()?;
                    client_repositories.set_approvers_name(input);
                    client_repositories.set_requires_approval(true);
                }
                "Approver email" => {
                    println!("Approver's email");
                    let approvers_email = match &client_repositories.approver {
                        None => String::new(),
                        Some(approver) => match &approver.approvers_email {
                            None => String::new(),
                            Some(approvers_name) => approvers_name.to_string(),
                        },
                    };

                    let input: String =
                        Self::take_and_validate_email(Option::Some(&*approvers_email))?;
                    client_repositories.set_approvers_email(input);
                    client_repositories.set_requires_approval(true);
                }
                "Client company name" => {
                    println!("Client company name");
                    let input: String = Input::new()
                        .with_initial_text(&client.client_name)
                        .interact_text()?;
                    client_repositories.update_client_name(input);
                }
                "Client contact person" => {
                    println!("Client contact person");
                    let input: String = Input::new()
                        .with_initial_text(&client.client_contact_person)
                        .interact_text()?;
                    client_repositories.update_client_contact_person(input);
                }
                "Client address" => {
                    println!("Client address");
                    if let Some(input) = Editor::new().edit(&client.client_address)? {
                        client_repositories.update_client_address(input);
                    }
                }
                "User name" => {
                    println!("User name");
                    let input: String =
                        Input::new().with_initial_text(&user.name).interact_text()?;
                    client_repositories.set_user_name(input);
                    client_repositories.set_is_user_alias(true);
                }
                "User email" => {
                    println!("User email");
                    let input: String = Self::take_and_validate_email(Option::Some(&user.email))?;
                    client_repositories.set_user_email(input);
                    client_repositories.set_is_user_alias(true);
                }
                _ => {}
            };
        }

        Ok(())
    }

    pub fn prompt_for_client_then_onboard(
        &mut self,
        deserialized_config: &mut ConfigurationDoc,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::print_question("\u{1F916} Initialising new repository.");

        let mut clients: Vec<String> = deserialized_config
            .iter()
            .map(|client| client.get_client_name().clone())
            .collect();

        // If the clients array is empty, lets just onboard
        if clients.is_empty() {
            self.onboarding(false)?;
        }

        let no_client_value = "Create a new client".to_string();
        clients.push(no_client_value.clone());

        Self::print_question("Would you like to add it to any of these existing clients?");
        let selection: usize = Select::new().items(&clients).interact()?;
        let client_name = &clients[selection];

        // if this is a new client, onboard as normal
        if client_name == &no_client_value {
            self.onboarding(false)?;
        } else {
            // otherwise pre-populate the client and user details
            let client = deserialized_config
                .iter()
                .find(|client| &client.get_client_name() == client_name)
                .unwrap();

            self.client_repositories
                .borrow_mut()
                .set_values_from_buffer(client);

            let unwrapped_client = client.client.as_ref().unwrap();

            self.repository
                .borrow_mut()
                .set_client_id(unwrapped_client.id.clone())
                .set_client_name(unwrapped_client.client_name.clone())
                .set_client_address(unwrapped_client.client_address.clone())
                .set_client_contact_person(unwrapped_client.client_contact_person.clone());

            self.existing_client_onboarding(deserialized_config)?;
        }

        Ok(())
    }

    fn print_question(text: &str) {
        println!("\n{}", Style::new().bold().paint(text));
    }

    pub fn confirm_repository_path(
        &self,
        new_user: bool,
    ) -> Result<&Self, Box<dyn std::error::Error>> {
        let repo_path = self.repository.clone();
        let mut borrow = repo_path.borrow_mut();
        let path = match borrow.repo_path.as_ref() {
            Some(path) => path.clone(),
            None => {
                println!(
                    "A configuration file hasn't been found, which suggests \n\
                    autolog hasn't been initialised yet, or that all \n\
                    clients and repositories were removed. \n\
                    \n\
                    Run autolog init to add your first client."
                );
                std::process::exit(exitcode::OK);
            }
        };

        if new_user {
            let ascii_table = AsciiTable::default();
            let logo = [[Style::new().bold().paint("A U T O L O G")]];
            ascii_table.print(logo);
        }

        let current_repo_path = file_reader::get_canonical_path(".");
        if path == current_repo_path {
            Self::print_question("Initialise for current repository?");
        } else {
            Self::print_question(&format!("With the project at this path {}?", path));
        };

        if Confirm::new().default(true).interact()? {
            borrow.set_repo_path(path);
        } else {
            Self::print_question("Give a path to the repository you would like to use");

            let path = crate::utils::file::file_reader::get_home_path()
                .to_str()
                .unwrap()
                .to_string();

            let input: String = Input::new().with_initial_text(path).interact_text()?;

            borrow.set_repo_path(input);
        }

        Ok(self)
    }

    pub fn prompt_for_setting_user_alias(
        &self,
        name: String,
        email: String,
    ) -> Result<&Self, Box<dyn std::error::Error>> {
        let mut client_borrow = self.client_repositories.borrow_mut();

        println!("\nThe git config name or email found for this repository differs from the one being used for your user details.");
        println!(
            "{}",
            Self::dim_text(
                "Your user details will be used on any timesheets generated for this client."
            )
        );
        println!("\nCurrent settings:");
        let ascii_table = AsciiTable::default();
        ascii_table.print(vec![
            vec![Self::dim_text("Name"), name],
            vec![Self::dim_text("Email"), email],
        ]);

        println!(
            "\nYour new settings will overwrite these.\n\
            \nAlternatively you can set a user/email alias that will be consistent across repositories under this client.",
        );
        Self::print_question("Set an alias for this client?");
        println!(
            "{}",
            Self::dim_text("Note: These can be updated by running autolog update.")
        );

        if Confirm::new().default(true).interact()? {
            Self::print_question("User name");
            let name: String = Input::new().interact_text()?;
            client_borrow.set_user_name(name);

            Self::print_question("User email");
            let email: String = Input::new().interact_text()?;
            client_borrow.set_user_email(email);

            client_borrow.set_is_user_alias(true);
            client_borrow.set_user_id(nanoid!());

            println!("\nUser alias created \u{1F389}");
        }

        Ok(self)
    }

    pub fn search_for_repository_details(
        &self,
        deserialized_config: Option<&ConfigurationDoc>,
    ) -> Result<&Self, Box<dyn std::error::Error>> {
        let mut repository_borrow = self.repository.borrow_mut();
        repository_borrow.find_repository_details_from()?;

        repository_borrow.set_user_id(nanoid!());
        repository_borrow.set_repository_id(nanoid!());

        println!("{}", Self::dim_text("\u{1F916} Repository details found."));

        // check whether an alias should be created if there isn't one already
        if let Some(config) = deserialized_config {
            // get the client that has been set to the repository
            let client = config.iter().find(|client| {
                &client.get_client_name() == repository_borrow.client_name.as_ref().unwrap()
            });

            if let Some(client) = client {
                let user = client.user.as_ref().unwrap();
                let name = &user.name;
                let email = &user.email;
                let is_alias = &user.is_alias;

                // if the user details differ, prompt for alias
                if repository_borrow.has_different_user_details(name, email) & !is_alias {
                    self.prompt_for_setting_user_alias(name.to_string(), email.to_string())?;
                }
            }
        }

        Ok(self)
    }

    pub fn add_client_details(&self) -> Result<&Self, std::io::Error> {
        Self::print_question("Client company name");
        let input: String = Input::new().interact_text()?;
        self.repository.borrow_mut().set_client_name(input);

        Self::print_question("Client contact person");
        let input: String = Input::new().interact_text()?;
        self.repository
            .borrow_mut()
            .set_client_contact_person(input);

        Self::print_question("Would you like to add a Client address?");
        if Confirm::new().default(true).interact()? {
            if let Some(input) = Editor::new().edit("Enter an address").unwrap() {
                self.repository.borrow_mut().set_client_address(input);
            }
        }
        self.repository.borrow_mut().set_client_id(nanoid!());

        Ok(self)
    }

    pub fn prompt_for_manager_approval(&self) -> Result<&Self, Box<dyn Error>> {
        let mut client_repositories = self.client_repositories.borrow_mut();

        let prompt_for_approver = match client_repositories.requires_approval {
            None => true,
            Some(requires_approval) => !requires_approval,
        };

        if prompt_for_approver {
            Self::print_question("Do timesheets under this client require approval?");
            println!("{}", Self::dim_text(
            "(This will enable signing functionality, see https://autolog.dev/docs/signing)",
            ));

            if Confirm::new().default(true).interact()? {
                Self::print_question("Approvers name");
                let input: String = Input::new().interact_text()?;
                client_repositories.set_approvers_name(input);

                Self::print_question("Approvers email");
                let input: String = Input::new().interact_text()?;
                client_repositories.set_approvers_email(input);

                // TODO - check the above are set before setting this
                client_repositories.set_requires_approval(true);
            } else {
                client_repositories.set_requires_approval(false);
            }
        }

        Ok(self)
    }

    pub fn add_project_numbers(&self) -> Result<&Self, Box<dyn Error>> {
        let mut client_repositories = self.client_repositories.borrow_mut();

        println!(
            "{}",
            Self::dim_text(&format!(
                "\u{1F916} Finding project data for '{}'...",
                client_repositories.get_client_name()
            ))
        );

        for i in 0..client_repositories.repositories.as_ref().unwrap().len() {
            Self::print_question(&format!(
                "Does '{}' require a project/PO number?",
                client_repositories.repositories.as_ref().unwrap()[i]
                    .namespace
                    .as_ref()
                    .unwrap()
            ));
            if Confirm::new().default(true).interact()? {
                Self::print_question("Project number");
                let input: String = Input::new().interact_text()?;
                client_repositories
                    .repositories
                    .as_mut()
                    .map(|repo| repo[i].set_project_number(input));
            }
        }

        Ok(self)
    }

    pub fn prompt_for_client_repo_removal(
        &self,
        options: Vec<Option<String>>,
        deserialized_config: &mut ConfigurationDoc,
    ) -> Result<&Self, Box<dyn Error>> {
        if options[1].is_some() {
            Self::print_question(&format!(
                "Remove '{}' from client '{}'?",
                &options[1].as_ref().unwrap(),
                &options[0].as_ref().unwrap()
            ));
            // remove the namespace from a client
            if crate::utils::confirm()? {
                for i in 0..deserialized_config.len() {
                    if deserialized_config[i]
                        .client
                        .as_ref()
                        .unwrap()
                        .client_name
                        .to_lowercase()
                        == options[0].as_ref().unwrap().to_lowercase()
                    {
                        let repo_len = deserialized_config[i].repositories.as_ref().unwrap().len();
                        deserialized_config[i]
                            .remove_repository_by_namespace(options[1].as_ref().unwrap());

                        if repo_len != deserialized_config[i].repositories.as_ref().unwrap().len() {
                            Self::print_question(&format!(
                                "'{}' removed  \u{1F389}",
                                &options[1].as_ref().unwrap()
                            ));

                            return Ok(self);
                        } else {
                            Self::print_question("Repository not found. Nothing removed.");
                            crate::utils::exit_process();
                        }
                    }
                }
            }
        } else {
            Self::print_question(&format!(
                "Remove client '{}'?",
                &options[0].as_ref().unwrap()
            ));
            // client is required and will be set, so remove from deserialized config
            if crate::utils::confirm()? {
                let config_len = deserialized_config.len();
                deserialized_config.retain(|client_repo| {
                    client_repo
                        .client
                        .as_ref()
                        .unwrap()
                        .client_name
                        .to_lowercase()
                        != options[0].as_ref().unwrap().to_lowercase()
                });

                if config_len != deserialized_config.len() {
                    Self::print_question(&format!(
                        "'{}' removed \u{1F389}",
                        &options[0].as_ref().unwrap()
                    ));
                    return Ok(self);
                } else {
                    Self::print_question("Client not found. Nothing removed.");
                    crate::utils::exit_process();
                }
            }
        }

        Ok(self)
    }

    fn dim_text(text: &str) -> String {
        format!("{}", Style::new().dimmed().paint(text))
    }

    pub fn show_details(&self) -> &Self {
        Self::print_question("These are the details associated with this repository:");
        let ascii_table = AsciiTable::default();
        let mut data = vec![];

        if let Some(namespace) = self.repository.borrow().namespace.as_ref() {
            let row = vec![Self::dim_text("Namespace:"), namespace.clone()];
            data.append(&mut vec![row]);
        }
        if let Some(repo_path) = self.repository.borrow().repo_path.as_ref() {
            let row = vec![Self::dim_text("Repository path:"), repo_path.clone()];
            data.append(&mut vec![row]);
        }
        if let Some(git_path) = self.repository.borrow().git_path.as_ref() {
            let row = vec![Self::dim_text("Git path:"), git_path.clone()];
            data.append(&mut vec![row]);
        }
        if let Some(email) = self.repository.borrow().email.as_ref() {
            let row = vec![Self::dim_text("Email:"), email.clone()];
            data.append(&mut vec![row]);
        }
        if let Some(name) = self.repository.borrow().name.as_ref() {
            let row = vec![Self::dim_text("Name:"), name.clone()];
            data.append(&mut vec![row]);
        }
        if let Some(client_name) = self.repository.borrow().client_name.as_ref() {
            let row = vec![Self::dim_text("Client company name:"), client_name.clone()];
            data.append(&mut vec![row]);
        }
        if let Some(client_contact_person) = self.repository.borrow().client_contact_person.as_ref()
        {
            let row = vec![
                Self::dim_text("Client contact person:"),
                client_contact_person.clone(),
            ];
            data.append(&mut vec![row]);
        }
        if let Some(client_address) = self.repository.borrow().client_address.as_ref() {
            let row = vec![
                Self::dim_text("Client address:"),
                client_address.clone().replace('\n', " "),
            ];
            data.append(&mut vec![row]);
        }

        ascii_table.print(data);

        self
    }

    pub fn list_clients_and_repos(&self, config: ConfigurationDoc) -> &Self {
        for client in config {
            println!("\n {}", Style::new().bold().paint(client.get_client_name()));
            let ascii_table = AsciiTable::default();
            let mut rows = vec![];

            if let Some(repositories) = client.repositories {
                for repo in repositories {
                    rows.push(vec![repo.namespace.unwrap()]);
                }

                ascii_table.print(rows);
            }
        }

        self
    }
}
