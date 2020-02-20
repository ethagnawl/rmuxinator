extern crate rmuxinator;

use rmuxinator::{run, test_for_tmux, CliArgs, Config};
use std::{env, process};

fn main() {
    let tmux_exists = test_for_tmux(String::from("tmux"));

    if !tmux_exists {
        eprintln!(
            "Unable to find tmux. Is it installed and available on $PATH?"
        );
        process::exit(1);
    }

    let raw_cli_args: Vec<String> = env::args().collect();

    let cli_args = CliArgs::new(&raw_cli_args).unwrap_or_else(|error| {
        eprintln!("Problem parsing CLI arguments: {}", error);
        process::exit(1);
    });

    let config = Config::new(cli_args).unwrap_or_else(|error| {
        eprintln!("Problem parsing config file: {}", error);
        process::exit(1);
    });

    if let Err(error) = run(config) {
        eprintln!("Application error: {}", error);
        process::exit(1);
    }
}
