use serde::Deserialize;
// use std::env;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

extern crate toml;

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let session_name = config.name;
    let args = ["new-session", "-s", &session_name, "-A"];
    let output = Command::new("tmux").args(&args).spawn()?.wait();

    println!("{:#?}", output);

    Ok(())
}

#[derive(Debug)]
pub struct CliArgs {
    pub command: String,
    pub project_name: String,
}

impl CliArgs {
    pub fn new(args: &[String]) -> Result<CliArgs, &'static str> {
        println!("CliArgs: {:#?}", args);

        Ok(CliArgs {
            command: args[1].clone(),
            project_name: args[2].clone(),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub name: String,
    pub root: String,
    pub windows: String,
}

impl Config {
    pub fn new(cli_args: CliArgs) -> Result<Config, &'static str> {
        let mut f = match File::open(cli_args.project_name) {
            Ok(x) => x,
            Err(_) => return Err("Unable to open config file."),
        };
        let mut contents = String::new();

        match f.read_to_string(&mut contents) {
            Ok(_) => (),
            Err(_) => return Err("Unable to read config file."),
        }

        let decoded: Config = toml::from_str(&contents).unwrap();
        println!("decoded: {:#?}", decoded);
        Ok(decoded)
    }
}

// #[cfg(test)]
// mod test {
//     use super::*;

//     #[test]
//     fn case_sensitive() {
//         let query = "duct";
//         let contents = "\
// Rust:
// safe, fast, productive.
// Pick three.";
//         assert_eq!(vec!["safe, fast, productive."], search(query, contents));
//     }

//     #[test]
//     fn case_insensitive() {
//         let query = "Duct";
//         let contents = "\
// Rust:
// safe, fast, productive.
// Pick three.";
//         assert_eq!(
//             vec!["safe, fast, productive."],
//             search_case_insensitive(query, contents)
//         );
//     }
// }
