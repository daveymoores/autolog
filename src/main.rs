use std::process;
mod config;
mod data;
mod helpers;
mod interface;
mod utils;

fn main() {
    let cli = interface::cli::Cli::new();
    cli.run().unwrap_or_else(|err| {
        eprintln!("Problem parsing arguments: {}", err);
        process::exit(1);
    });
}
