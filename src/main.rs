extern crate rmuxinator;

use rmuxinator::CliArgs;
use rmuxinator::Config;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let cli_args = CliArgs::new(&args).unwrap_or_else(|error| {
        eprintln!("Problem parsing CLI arguments: {}", error);
        process::exit(1);
    });

    println!("main.rs {:?}", cli_args);

    let config = Config::new(cli_args).unwrap_or_else(|error| {
        eprintln!("Problem parsing config file: {}", error);
        process::exit(1);
    });

    if let Err(error) = rmuxinator::run(config) {
        println!("Application error: {}", error);
        process::exit(1);
    }
}
