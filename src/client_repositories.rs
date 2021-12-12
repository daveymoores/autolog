use crate::config::New;
use crate::repository::{GitLogDates, Repository};
use serde::{Deserialize, Serialize};
use std::cell::Ref;
use std::ops::Deref;
use std::process;
use std::process::Command;

/// Repositories are modified at a Repository level and a client level.
/// ClientRepositories  holds the client and the repositories when they are found in the buffer
/// Storing the data here allows the repository  being currently operated on to be cross referenced
/// against all the repos under the same client, and hence generate the correct working hours.

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Approver {
    pub approvers_name: Option<String>,
    pub approvers_email: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Client {
    pub id: String,
    pub client_name: String,
    pub client_address: String,
    pub client_contact_person: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub is_alias: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ClientRepositories {
    pub client: Option<Client>,
    pub user: Option<User>,
    pub repositories: Option<Vec<Repository>>,
    pub requires_approval: Option<bool>,
    pub user_signature: Option<String>,
    pub approver_signature: Option<String>,
    pub approver: Option<Approver>,
}

impl New for ClientRepositories {
    fn new() -> Self {
        Self {
            ..Default::default()
        }
    }
}

impl Default for ClientRepositories {
    fn default() -> Self {
        Self {
            client: None,
            user: None,
            approver: None,
            requires_approval: None,
            user_signature: None,
            approver_signature: None,
            repositories: None,
        }
    }
}

impl ClientRepositories {
    pub fn set_values(&mut self, repository: Ref<Repository>) -> &mut Self {
        self.client = Option::from(Client {
            id: repository.client_id.clone().unwrap_or("None".to_string()),
            client_name: repository.client_name.clone().unwrap_or("None".to_string()),
            client_address: repository
                .client_address
                .clone()
                .unwrap_or("None".to_string()),
            client_contact_person: repository
                .client_contact_person
                .clone()
                .unwrap_or("None".to_string()),
        });

        let should_set_user = match self.user.as_ref() {
            None => true,
            Some(user) => !user.is_alias,
        };

        // if an alias hasn't been, or there isn't a user yet, set the user from repo
        if should_set_user {
            self.user = Option::from(User {
                id: repository.user_id.clone().unwrap_or("None".to_string()),
                name: repository.name.clone().unwrap_or("None".to_string()),
                email: repository.email.clone().unwrap_or("None".to_string()),
                is_alias: false,
            });
        }

        self.repositories = Option::from(vec![repository.deref().clone()]);
        self
    }

    pub fn get_client_name(&self) -> String {
        self.client.as_ref().unwrap().clone().client_name
    }

    pub fn get_client_id(&self) -> String {
        self.client.as_ref().unwrap().clone().id
    }

    pub fn update_client_name(&mut self, value: String) -> &mut Self {
        self.client
            .as_mut()
            .map(|mut client| client.client_name = value.clone());
        self.repositories.as_mut().map(|repos| {
            repos
                .iter_mut()
                .map(|repo| {
                    repo.client_name = Some(value.clone());
                    repo
                })
                .collect::<Vec<&mut Repository>>()
        });
        self
    }

    pub fn update_client_address(&mut self, value: String) -> &mut Self {
        self.client
            .as_mut()
            .map(|mut client| client.client_address = value.clone());
        self.repositories.as_mut().map(|repos| {
            repos
                .iter_mut()
                .map(|repo| {
                    repo.client_address = Some(value.clone());
                    repo
                })
                .collect::<Vec<&mut Repository>>()
        });
        self
    }

    pub fn update_client_contact_person(&mut self, value: String) -> &mut Self {
        self.client
            .as_mut()
            .map(|mut client| client.client_contact_person = value.clone());
        self.repositories.as_mut().map(|repos| {
            repos
                .iter_mut()
                .map(|repo| {
                    repo.client_contact_person = Some(value.clone());
                    repo
                })
                .collect::<Vec<&mut Repository>>()
        });
        self
    }

    pub fn set_values_from_buffer(
        &mut self,
        client_repositories: &ClientRepositories,
    ) -> &mut ClientRepositories {
        *self = client_repositories.clone();
        self
    }

    pub fn remove_repository_by_namespace(&mut self, namespace: &String) -> &mut Self {
        self.repositories.as_mut().map(|repos| {
            repos.retain(|repo| {
                repo.namespace.as_ref().unwrap().to_lowercase() != namespace.to_lowercase()
            })
        });

        self
    }

    pub fn set_approvers_name(&mut self, value: String) -> &mut Self {
        if let Some(_) = self.approver {
            self.approver
                .as_mut()
                .map(|approver| approver.approvers_name = Option::from(value));
        } else {
            self.approver = Option::Some(Approver {
                approvers_name: Option::from(value),
                approvers_email: Option::None,
            });
        }

        self
    }

    pub fn set_approvers_email(&mut self, value: String) -> &mut Self {
        if let Some(_) = self.approver {
            self.approver
                .as_mut()
                .map(|approver| approver.approvers_email = Option::from(value));
        } else {
            self.approver = Option::Some(Approver {
                approvers_name: Option::None,
                approvers_email: Option::from(value),
            });
        }

        self
    }

    pub fn set_requires_approval(&mut self, value: bool) -> &mut Self {
        self.requires_approval = Option::Some(value);
        self
    }

    pub fn set_user_name(&mut self, value: String) -> &mut Self {
        self.user.as_mut().map(|user| user.name = value);
        self
    }

    pub fn set_user_email(&mut self, value: String) -> &mut Self {
        self.user.as_mut().map(|user| user.email = value);
        self
    }

    pub fn set_is_user_alias(&mut self, value: bool) -> &mut Self {
        self.user.as_mut().map(|user| user.is_alias = value);
        self
    }

    pub fn set_user_id(&mut self, value: String) -> &mut Self {
        self.user.as_mut().map(|user| user.id = value);
        self
    }

    pub fn exec_generate_timesheets_from_git_history(&mut self) -> &mut Self {
        if let Some(repositories) = &mut self.repositories {
            for repository in repositories {
                let command = String::from("--author");

                // can safely unwrap here as name would have been set in the previous step
                let author = [command, repository.name.as_ref().unwrap().to_string()].join("=");

                let output = Command::new("git")
                    .arg("-C")
                    .arg(repository.git_path.as_ref().unwrap().to_string())
                    .arg("log")
                    .arg("--date=rfc")
                    .arg(author)
                    .arg("--all")
                    .output()
                    .expect("Failed to execute command");

                let output_string = crate::utils::trim_output_from_utf8(output)
                    .unwrap_or_else(|_| "Parsing output failed".to_string());

                repository.parse_git_log_dates_from_git_history(output_string);
            }
        }

        self
    }

    pub fn compare_logs_and_set_timesheets(&mut self) -> &mut Self {
        if let Some(repositories) = &mut self.repositories {
            for i in 0..repositories.len() {
                // for each repository, build a vec of the git_log_dates from the other repositories
                let adjacent_git_log_dates: Vec<GitLogDates> = repositories
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| index != &i)
                    .map(|(_, repo)| repo.git_log_dates.as_ref().unwrap().clone())
                    .collect();

                let timesheet = match &repositories[i].git_log_dates {
                    Some(git_log_dates) => crate::date_parser::get_timesheet_map_from_date_hashmap(
                        git_log_dates.clone(),
                        &mut repositories[i],
                        adjacent_git_log_dates,
                    ),
                    None => {
                        eprintln!("No dates parsed from git log");
                        process::exit(exitcode::DATAERR);
                    }
                };

                repositories[i].set_timesheet(timesheet);
            }
        }

        self
    }
}

#[cfg(test)]
mod tests {
    use crate::client_repositories::{Client, ClientRepositories, User};
    use crate::repository::{GitLogDates, Repository};
    use nanoid::nanoid;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    // Generate git log dates that overlap by one day each month to test hours being split equally
    fn generate_project_git_log_dates(days: [u32; 3]) -> GitLogDates {
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
    fn it_set_requires_approval() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_requires_approval(true);
        assert_eq!(client_repo.requires_approval.unwrap(), true);
    }

    #[test]
    fn it_set_user_name() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_user_name("potato".to_string());
        assert_eq!(
            client_repo.user.as_ref().unwrap().name,
            "potato".to_string()
        );
    }

    #[test]
    fn it_set_user_email() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_user_email("potato@tomato.com".to_string());
        assert_eq!(
            client_repo.user.as_ref().unwrap().email,
            "potato@tomato.com".to_string()
        );
    }

    #[test]
    fn it_set_is_user_alias() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_is_user_alias(true);
        assert_eq!(client_repo.user.as_ref().unwrap().is_alias, true);
    }

    #[test]
    fn it_set_user_id() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_user_id("123456".to_string());
        assert_eq!(client_repo.user.as_ref().unwrap().id, "123456".to_string());
    }

    #[test]
    fn it_gets_clients_name() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        let name = client_repo.get_client_name();
        assert_eq!(name, "alphabet");
    }

    #[test]
    fn it_updates_client_name() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.update_client_name("James".to_string());
        assert_eq!(
            client_repo.client.as_ref().unwrap().client_name,
            "James".to_string()
        );
        assert_eq!(
            client_repo.repositories.as_ref().unwrap()[0]
                .client_name
                .as_ref()
                .unwrap(),
            &"James".to_string()
        );
    }

    #[test]
    fn it_updates_client_address() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.update_client_address("Something, Somewhere, USA".to_string());
        assert_eq!(
            client_repo.client.as_ref().unwrap().client_address,
            "Something, Somewhere, USA".to_string()
        );
        assert_eq!(
            client_repo.repositories.as_ref().unwrap()[0]
                .client_address
                .as_ref()
                .unwrap(),
            &"Something, Somewhere, USA".to_string()
        );
    }

    #[test]
    fn it_updates_client_contact_person() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.update_client_contact_person("Jimmy Bones".to_string());
        assert_eq!(
            client_repo.client.as_ref().unwrap().client_contact_person,
            "Jimmy Bones".to_string()
        );
        assert_eq!(
            client_repo.repositories.as_ref().unwrap()[0]
                .client_contact_person
                .as_ref()
                .unwrap(),
            &"Jimmy Bones".to_string()
        );
    }

    #[test]
    fn it_updates_approvers_name() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_approvers_name("Jimmy Bones".to_string());
        assert_eq!(
            client_repo
                .approver
                .as_ref()
                .unwrap()
                .approvers_name
                .as_ref()
                .unwrap(),
            &"Jimmy Bones".to_string()
        );
    }

    #[test]
    fn it_updates_approvers_email() {
        let mut client_repo = ClientRepositories {
            ..Default::default()
        };

        create_mock_client_repository(&mut client_repo);

        client_repo.set_approvers_email("jimmy@bones.com".to_string());
        assert_eq!(
            client_repo
                .approver
                .as_ref()
                .unwrap()
                .approvers_email
                .as_ref()
                .unwrap(),
            &"jimmy@bones.com".to_string()
        );
    }

    #[test]
    fn it_sets_values() {
        let repo_id: String = nanoid!();
        let client_id: String = nanoid!();
        let user_id: String = nanoid!();

        let mut client_repositories = ClientRepositories {
            ..Default::default()
        };

        let repository = RefCell::new(Repository {
            client_id: Option::from(client_id.clone()),
            client_name: Option::from("Alphabet".to_string()),
            client_address: Option::from("Alphabet way".to_string()),
            client_contact_person: Option::from("John Jones".to_string()),
            user_id: Option::from(user_id.clone()),
            name: Option::from("Jim Jones".to_string()),
            email: Option::from("jim@jones.com".to_string()),
            id: Option::from(repo_id.clone()),
            ..Default::default()
        });

        client_repositories.set_values(repository.borrow());

        assert_eq!(
            json!(client_repositories.client),
            json!(Client {
                id: client_id.clone(),
                client_name: "Alphabet".to_string(),
                client_address: "Alphabet way".to_string(),
                client_contact_person: "John Jones".to_string(),
            })
        );

        assert_eq!(
            json!(client_repositories.user),
            json!(User {
                id: user_id.clone(),
                name: "Jim Jones".to_string(),
                email: "jim@jones.com".to_string(),
                is_alias: false,
            })
        );

        assert_eq!(
            json!(client_repositories.repositories.as_ref().unwrap()[0]),
            json!(Repository {
                client_id: Option::from(client_id.clone()),
                client_name: Option::from("Alphabet".to_string()),
                client_address: Option::from("Alphabet way".to_string()),
                client_contact_person: Option::from("John Jones".to_string()),
                user_id: Option::from(user_id.clone()),
                name: Option::from("Jim Jones".to_string()),
                email: Option::from("jim@jones.com".to_string()),
                id: Option::from(repo_id.clone()),
                ..Default::default()
            })
        );
    }

    #[test]
    fn it_compares_git_logs_and_sets_timesheets() {
        let mut client_repositories: ClientRepositories = ClientRepositories {
            client: Option::Some(Client {
                id: nanoid!(),
                client_name: "Alphabet".to_string(),
                client_address: "Alphabet way".to_string(),
                client_contact_person: "John Jones".to_string(),
            }),
            user: Option::Some(User {
                id: nanoid!(),
                name: "Jim Jones".to_string(),
                email: "jim@jones.com".to_string(),
                is_alias: false,
            }),
            repositories: Option::Some(vec![
                Repository {
                    client_name: Option::Some("Alphabet".to_string()),
                    namespace: Option::Some("Project_1".to_string()),
                    git_log_dates: Option::Some(generate_project_git_log_dates([1, 2, 3])),
                    ..Default::default()
                },
                Repository {
                    client_name: Option::Some("Alphabet".to_string()),
                    namespace: Option::Some("Project_2".to_string()),
                    git_log_dates: Option::Some(generate_project_git_log_dates([2, 3, 4])),
                    ..Default::default()
                },
                Repository {
                    client_name: Option::Some("Alphabet".to_string()),
                    namespace: Option::Some("Project_3".to_string()),
                    git_log_dates: Option::Some(generate_project_git_log_dates([3, 4, 5])),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        client_repositories.compare_logs_and_set_timesheets();

        let repositories = client_repositories.repositories.unwrap();
        // Check project 1 has hours split on overlapping days
        let repository = repositories[0].clone();
        let timesheet = &repository
            .timesheet
            .as_ref()
            .unwrap()
            .get("2021")
            .unwrap()
            .get("2")
            .unwrap()[0..3];

        let ts = timesheet
            .into_iter()
            .map(|day| day.get("hours").unwrap().clone().as_f64().unwrap())
            .collect::<Vec<f64>>();

        assert_eq!(ts, vec![8.0, 4.0, 2.6666666666666665]);
    }
}
