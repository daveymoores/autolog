extern crate clap;
use crate::config;
use crate::config::{Edit, Init, List, Make, New, Remove, Update};
use crate::data::client_repositories::ClientRepositories;
use crate::data::repository;
use crate::data::repository::Repository;
use crate::interface::help_prompt::HelpPrompt;
use crate::utils::db::db_reader;
use chrono::prelude::*;
use clap::{App, Arg, ArgMatches, Error};
use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq)]
pub enum Commands {
    Init,
    Make,
    Edit,
    Remove,
    Update,
    List,
}

#[derive(Debug, Default)]
pub struct Cli<'a> {
    matches: ArgMatches<'a>,
    command: Option<Commands>,
    options: Vec<Option<String>>,
}

impl Cli<'_> {
    pub fn new() -> Self {
        Self::new_from(std::env::args_os()).unwrap_or_else(|e| e.exit())
    }

    fn new_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: Iterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let namespace_arg = Arg::with_name("namespace")
            .short("n")
            .long("namespace")
            .value_name("namespace")
            .help(
                "Pass optional namespace/project name of git repository. Defaults \n\
                            to project within the current directory",
            );

        let hour_arg = Arg::with_name("hour")
            .short("h")
            .long("hour")
            .value_name("xx")
            .help(
                "sets the hour value. When the day/month/year \n\
                    isn't set, it defaults to the current day",
            )
            .required(true);

        let day_arg = Arg::with_name("day")
            .requires("hour")
            .short("d")
            .long("day")
            .value_name("xx")
            .help(
                "sets the day value. When the month/year \n\
                    isn't set, it defaults to the current day",
            );

        let month_arg = Arg::with_name("month")
            .requires("day")
            .short("m")
            .long("month")
            .value_name("xx")
            .help(
                "sets the day value. When the month/year \n\
                    isn't set, it defaults to the current day",
            );

        let year_arg = Arg::with_name("year")
            .requires("month")
            .short("y")
            .long("year")
            .value_name("xxxx")
            .help(
                "sets the day value. When the month/year \n\
                    isn't set, it defaults to the current day",
            );

        let app: App = App::new("autolog")
            .version("0.1")
            .author("Davey Moores")
            .about(
                "Minimal configuration, simple timesheets for sharing via pdf download or unique link.",
            ).subcommand(
            App::new("init")
                .about("Initialise for current or specified repository")
                .arg(Arg::with_name("path")
                    .short("p")
                    .long("path")
                    .value_name("path")
                    .help(
                        "Pass optional 'path' to git repository. Defaults \n\
                            to current directory",
                    )))
            .subcommand(App::new("edit")
                .about("Change the hours worked value for a given day")
                .arg(&namespace_arg)
                .arg(&hour_arg)
                .arg(&day_arg)
                .arg(&month_arg)
                .arg(&year_arg))
            .subcommand(App::new("remove")
                .about("Remove a client or repository")
                .arg(Arg::with_name("client")
                    .short("c")
                    .long("client")
                    .value_name("client")
                    .help("Required client name. If the namespace isn't passed, this command \n\
                    will remove the passed client (after a prompt)",
                    ).required(true))
                .arg(Arg::with_name("namespace")
                    .requires("client")
                    .short("n")
                    .long("namespace")
                    .value_name("namespace")
                    .help(
                        "Pass an optional namespace/project name of git repository. Defaults \n\
                            to project within the current directory",
                    )))
            .subcommand(App::new("update")
                .about("Update details for a client or repository")
                .arg(Arg::with_name("client")
                    .short("c")
                    .long("client")
                    .value_name("client")
                    .help("Required client name. If the namespace isn't passed, this command \n\
                    will edit the passed client",
                    ).required(true))
                .arg(Arg::with_name("namespace")
                    .requires("client")
                    .short("n")
                    .long("namespace")
                    .value_name("namespace")
                    .help(
                        "Pass an optional namespace/project name of the git repository",
                    )))
            .subcommand(App::new("list")
                .about("List all clients and associated repositories"))
            .subcommand(App::new("make")
                .about("Generate a new timesheet on a unique link")
                .arg(Arg::with_name("client")
                    .short("c")
                    .long("client")
                    .value_name("client")
                    .help(
                        "Pass optional client name. Defaults \n\
                            to client of current directory",
                    ))
                .arg(Arg::with_name("month")
                    .short("m")
                    .long("month")
                    .value_name("xx")
                    .help(
                        "sets the month value. When the month \n\
                    isn't set, it defaults to the current day",
                    ))
                .arg(&year_arg));

        // extract the matches
        let matches = app.get_matches_from_safe(args)?;

        Ok(Cli {
            matches,
            command: None,
            options: vec![None],
        })
    }

    pub fn parse_commands(&self, matches: &ArgMatches) -> Result<Cli, clap::Error> {
        let mut options: Vec<Option<String>> = vec![];
        let command;

        // provide fallback values if day/month/year isn't provided
        let date_time: DateTime<Local> = Local::now();
        let year = date_time.year().to_string();
        let month = date_time.month().to_string();
        let day = date_time.day().to_string();

        let current_repo_path = db_reader::get_canonical_path(".");
        let mut temp_repository = Repository {
            repo_path: Option::from(current_repo_path.clone()),
            ..Default::default()
        };

        // get namespace of working repository
        temp_repository
            .find_git_path_from_directory_from()
            .unwrap_or_else(|err| {
                eprintln!("Error finding git path from project directory: {}", err);
                std::process::exit(exitcode::CANTCREAT);
            })
            .find_namespace_from_git_path()
            .unwrap_or_else(|err| {
                eprintln!("Error finding namespace from git path: {}", err);
                std::process::exit(exitcode::CANTCREAT);
            });

        let current_repository_namespace: String = temp_repository.namespace.unwrap();

        if let Some(init) = matches.subcommand_matches("init") {
            // This will onboard so no need to pass the path here
            options.push(Some(
                init.value_of("path")
                    .unwrap_or(&current_repo_path)
                    .to_string(),
            ));
            command = Some(Commands::Init);
        } else if let Some(make) = matches.subcommand_matches("make") {
            // set default value of current month
            options.push(make.value_of("client").map(String::from));
            options.push(Some(make.value_of("month").unwrap_or(&month).to_string()));
            options.push(Some(make.value_of("year").unwrap_or(&year).to_string()));
            command = Some(Commands::Make);
        } else if let Some(edit) = matches.subcommand_matches("edit") {
            // this will error out if the preceding date value isn't passed
            // so I can happily set default here knowing that just the day/month/year will make it through
            options.push(Some(
                edit.value_of("namespace")
                    .unwrap_or(&current_repository_namespace)
                    .to_string(),
            ));
            options.push(Some(edit.value_of("hour").unwrap().to_string()));
            options.push(Some(edit.value_of("day").unwrap_or(&day).to_string()));
            options.push(Some(edit.value_of("month").unwrap_or(&month).to_string()));
            options.push(Some(edit.value_of("year").unwrap_or(&year).to_string()));
            command = Some(Commands::Edit);
        } else if let Some(remove) = matches.subcommand_matches("remove") {
            options.push(Some(remove.value_of("client").unwrap().to_string()));
            options.push(remove.value_of("namespace").map(String::from));
            command = Some(Commands::Remove);
        } else if let Some(update) = matches.subcommand_matches("update") {
            options.push(Some(update.value_of("client").unwrap().to_string()));
            options.push(update.value_of("namespace").map(String::from));
            command = Some(Commands::Update);
        } else if matches.subcommand_matches("list").is_some() {
            command = Some(Commands::List);
        } else {
            return Err(Error {
                message: "No matches for inputs".to_string(),
                kind: clap::ErrorKind::EmptyValue,
                info: None,
            });
        }

        Ok(Cli {
            options,
            command,
            ..Default::default()
        })
    }

    pub fn run(&self) -> Result<(), clap::Error> {
        let mut config: config::Config = config::Config::new();
        let mut repository = repository::Repository::new();
        let mut client_repositories = ClientRepositories::new();
        let matches = &self.matches;
        let cli: Cli = self.parse_commands(matches)?;

        // pass the path for init so that I already know it if user is being onboarded
        if let Some(Commands::Init) = &cli.command {
            if let Some(path) = cli.options.get(0).and_then(|path| path.clone()) {
                repository.set_repo_path(path);
            }
        }

        let mut prompt = crate::interface::help_prompt::HelpPrompt::new(
            &mut repository,
            &mut client_repositories,
        );

        Self::run_command(cli, &mut config, &mut prompt);

        Ok(())
    }

    pub fn run_command<T>(cli: Cli<'_>, config: &mut T, prompt: &mut HelpPrompt)
    where
        T: Init + Make + Edit + Update + Remove + List,
    {
        match cli.command {
            None => {
                panic!("The programme shouldn't be able to get here");
            }
            Some(commands) => match commands {
                Commands::Init => config.init(cli.options, prompt),
                Commands::Make => config.make(cli.options, prompt),
                Commands::Edit => config.edit(cli.options, prompt),
                Commands::Remove => config.remove(cli.options, prompt),
                Commands::Update => config.update(cli.options, prompt),
                Commands::List => config.list(prompt),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{New, Remove};
    use crate::data::repository::Repository;
    use std::fmt::Debug;
    use std::str::FromStr;

    fn unwrap_iter_with_option<T: FromStr + Clone>(args: Vec<Option<String>>) -> Vec<T>
    where
        <T as FromStr>::Err: Debug,
    {
        args.into_iter()
            .map(|x| {
                x.unwrap_or("None".to_string())
                    .parse::<T>()
                    .unwrap()
                    .clone()
            })
            .collect()
    }

    fn call_command_from_mock_config<I, T, K>(commands: I, mut mock_config: K)
    where
        I: Iterator<Item = T>,
        T: Into<OsString> + Clone,
        K: Init + Make + Edit + Update + Remove + List,
    {
        let cli = Cli::new_from(commands).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let response = new_cli.unwrap();

        let mut repository = Repository::new();
        let mut client_repositories = ClientRepositories::new();

        let mut prompt = crate::interface::help_prompt::HelpPrompt::new(
            &mut repository,
            &mut client_repositories,
        );

        Cli::run_command(response, &mut mock_config, &mut prompt);
    }

    struct MockConfig {}
    impl New for MockConfig {
        fn new() -> Self {
            MockConfig {}
        }
    }
    impl Init for MockConfig {
        fn init(&self, _options: Vec<Option<String>>, _prompt: &mut HelpPrompt) {
            assert!(true);
        }
    }

    impl Edit for MockConfig {
        fn edit(&self, _options: Vec<Option<String>>, _prompt: &mut HelpPrompt) {
            assert!(true);
        }
    }

    impl Make for MockConfig {
        fn make(&self, _options: Vec<Option<String>>, _prompt: &mut HelpPrompt) {
            assert!(true);
        }
    }

    impl Update for MockConfig {
        fn update(&self, _options: Vec<Option<String>>, _prompt: &mut HelpPrompt) {
            assert!(true);
        }
    }

    impl Remove for MockConfig {
        fn remove(&self, _options: Vec<Option<String>>, _prompt: &mut HelpPrompt) {
            assert!(true);
        }
    }

    impl List for MockConfig {
        fn list(&self, _prompt: &mut HelpPrompt) {
            assert!(true);
        }
    }

    #[test]
    fn calls_config_init_with_a_init_command() {
        call_command_from_mock_config(["exename", "init"].iter(), MockConfig::new());
    }

    #[test]
    fn calls_config_make_with_a_make_command() {
        call_command_from_mock_config(["exename", "make"].iter(), MockConfig::new());
    }

    #[test]
    fn calls_config_edit_with_a_edit_command() {
        call_command_from_mock_config(["exename", "edit", "-h5"].iter(), MockConfig::new());
    }

    #[test]
    #[should_panic]
    fn calls_config_remove_without_required_argument_and_errors() {
        call_command_from_mock_config(["exename", "remove"].iter(), MockConfig::new());
    }

    #[test]
    fn calls_config_remove_with_required_argument() {
        call_command_from_mock_config(
            ["exename", "remove", "--client=tomato"].iter(),
            MockConfig::new(),
        );
    }

    #[test]
    #[should_panic]
    fn calls_config_update_without_required_argument_and_errors() {
        call_command_from_mock_config(["exename", "update"].iter(), MockConfig::new());
    }

    #[test]
    fn calls_config_update_with_required_argument() {
        call_command_from_mock_config(
            ["exename", "update", "--client=tomato"].iter(),
            MockConfig::new(),
        );
    }

    #[test]
    fn calls_config_update_with_required_argument_and_optional_argument() {
        call_command_from_mock_config(
            ["exename", "update", "--client=tomato", "--namespace=potato"].iter(),
            MockConfig::new(),
        );
    }

    #[test]
    fn returns_an_error_when_no_command_args_are_passed() {
        let cli = Cli::new_from([""].iter()).unwrap();
        let result = cli.parse_commands(&cli.matches);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, clap::ErrorKind::EmptyValue);
    }

    #[test]
    fn finds_a_match_without_options_from_command_args() {
        let cli = Cli::new_from(["exename", "init"].iter()).unwrap();
        let m = cli.matches.subcommand_matches("init").unwrap();
        let (command, _value) = m.subcommand();
        assert_eq!(command, "");
    }

    #[test]
    fn returns_the_correct_enum_for_init() {
        let cli: Cli = Cli::new_from(["exename", "init"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();

        assert_eq!(result.command.unwrap().clone(), Commands::Init);
    }

    #[test]
    fn returns_the_passed_path_for_init() {
        let cli: Cli = Cli::new_from(["exename", "init", "-p/this/is/a/path"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["/this/is/a/path".to_string()]);
        assert_eq!(result.command.unwrap().clone(), Commands::Init);
    }

    #[test]
    fn returns_a_default_option_for_make() {
        let date_time: DateTime<Local> = Local::now();
        let month = date_time.month().to_string();
        let year = date_time.year().to_string();

        let cli: Cli = Cli::new_from(["exename", "make"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["None".to_string(), month, year]);
        assert_eq!(result.command.unwrap().clone(), Commands::Make);
    }

    #[test]
    fn returns_a_passed_value_for_make() {
        let cli: Cli =
            Cli::new_from(["exename", "make", "--client=Alphabet", "-m10", "-y2020"].iter())
                .unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["Alphabet", "10", "2020"]);
    }

    #[test]
    fn returns_an_error_when_no_arg_is_present_for_edit() {
        let result = Cli::new_from(["exename", "edit"].iter());
        assert!(result.is_err());
    }

    #[test]
    fn returns_an_error_when_a_day_is_passed_without_an_hour() {
        let result = Cli::new_from(["exename", "edit", "-d8"].iter());
        assert!(result.is_err());
    }

    #[test]
    fn returns_an_error_when_a_month_is_passed_without_a_day() {
        let result = Cli::new_from(["exename", "edit", "-m8"].iter());
        assert!(result.is_err());
    }

    #[test]
    fn returns_an_error_when_a_year_is_passed_to_edit_without_a_month() {
        let result = Cli::new_from(["exename", "edit", "-y2020"].iter());
        assert!(result.is_err());
    }

    #[test]
    fn returns_a_passed_value_for_edit() {
        let cli: Cli =
            Cli::new_from(["exename", "edit", "-h5", "-d15", "-m12", "-y2021"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["autolog", "5", "15", "12", "2021"]);
    }

    #[test]
    fn returns_an_error_when_a_year_is_passed_to_remove_without_a_month() {
        let result = Cli::new_from(["exename", "remove", "-y2020"].iter());
        assert!(result.is_err());
    }

    #[test]
    fn returns_an_error_when_an_hour_is_passed_to_remove() {
        let result = Cli::new_from(["exename", "remove", "-h5"].iter());
        assert!(result.is_err());
    }

    #[test]
    fn returns_a_passed_value_for_remove_with_a_none_optional_value() {
        let cli: Cli = Cli::new_from(["exename", "remove", "-c=tomato"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["tomato", "None"]);
    }

    #[test]
    fn returns_a_passed_values_for_remove() {
        let cli: Cli =
            Cli::new_from(["exename", "remove", "-c=tomato", "-n=genius"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["tomato", "genius"]);
    }

    #[test]
    fn returns_a_value_for_update_with_a_none_optional_value() {
        let cli: Cli = Cli::new_from(["exename", "update", "--client=tomato"].iter()).unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["tomato", "None"]);
    }

    #[test]
    fn returns_a_value_for_update_with_client_and_namespace() {
        let cli: Cli =
            Cli::new_from(["exename", "update", "--client=tomato", "--namespace=genius"].iter())
                .unwrap();
        let new_cli = cli.parse_commands(&cli.matches);
        let result = new_cli.unwrap();
        let values = unwrap_iter_with_option::<String>(result.options);
        assert_eq!(values, vec!["tomato", "genius"]);
    }

    #[test]
    fn throws_an_error_if_an_incorrect_arg_is_passed_in_update() {
        let result: Result<Cli, Error> = Cli::new_from(["exename", "update", "nn"].iter());
        assert!(result.is_err());
    }
}
