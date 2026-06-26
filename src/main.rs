mod api;
mod cli;
mod commands;
mod config;
mod models;
mod wizard;

use clap::Parser;
use cli::{Cli, Command};
use colored::Colorize;

fn main() {
    let cli = Cli::parse();

    if let Err(e) = wizard::maybe_run(cli.json, &cli.command) {
        eprintln!("{} {e:#}", "error:".red().bold());
        std::process::exit(1);
    }

    let result = match cli.command {
        Command::Stations(args) => commands::stations::run(args, cli.json),
        Command::Search(args) => commands::search::run(args, cli.json, cli.profile.as_deref()),
        Command::Track(args) => commands::track::run(args, cli.json),
        Command::Profile(args) => commands::profile::run(args, cli.json),
        Command::Whoami => commands::profile::whoami(cli.json),
        Command::Buy(args) => commands::buy::run(args, cli.json, cli.profile.as_deref()),
    };

    if let Err(e) = result {
        eprintln!("{} {e:#}", "error:".red().bold());
        std::process::exit(1);
    }
}
