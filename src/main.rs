extern crate rmuxinator;

use rmuxinator::CliArgs;
use rmuxinator::Config;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let cli_args = CliArgs::new(&args).unwrap_or_else(|error| {
        eprintln!("Problem parsing arguments: {}", error);
        process::exit(1);
    });

    println!("main.rs {:?}", cli_args);

    let args = [
        cli_args.project_name,
        String::from("~"),
        String::from("one"),
    ];

    let config = Config::new(&args).unwrap_or_else(|error| {
        eprintln!("Problem parsing arguments: {}", error);
        process::exit(1);
    });

    // TODO: remove legacy config fields; add new fields
    // TODO: use config to build script
    if let Err(error) = rmuxinator::run(config) {
        println!("Application error: {}", error);
        process::exit(1);
    }
}
