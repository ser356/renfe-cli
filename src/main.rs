mod api;
mod cli;
mod commands;
mod config;
mod models;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Stations(args) => commands::stations::run(args, cli.json),
        Command::Search(args) => commands::search::run(args, cli.json, cli.profile.as_deref()),
        Command::Track(args) => commands::track::run(args, cli.json),
        Command::Profile(args) => commands::profile::run(args, cli.json),
        Command::Whoami => commands::profile::whoami(cli.json),
        Command::Buy(args) => commands::buy::run(args, cli.json, cli.profile.as_deref()),
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
