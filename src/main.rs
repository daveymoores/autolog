use std::process;

mod cli;
mod config;
mod file_reader;
mod help_prompt;
mod timesheet;

fn main() {
    let cli = cli::Cli::new();
    cli.run().unwrap_or_else(|err| {
        eprintln!("Problem parsing arguments: {}", err);
        process::exit(1);
    });
}
