use serde::Deserialize;
// use std::env;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

extern crate toml;

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    // this should happen in the config constructor
    let mut f = File::open(config.filename)?;
    let mut contents = String::new();

    f.read_to_string(&mut contents)?;

    let decoded: Config = toml::from_str(&contents).unwrap();
    println!("decoded: {:#?}", decoded);

    // validate command $1 (e.g. start new debug)
    // find config file $2
    let session_name = decoded.name;
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
    pub filename: String,
    pub name: String,
    pub root: String,
    pub windows: String,
}

impl Config {
    pub fn new(args: &[String]) -> Result<Config, &'static str> {
        if args.len() < 3 {
            return Err("not enough arguments.");
        }

        println!("args: {:#?}", args);

        Ok(Config {
            filename: args[0].clone(),
            name: args[0].clone(),
            root: args[1].clone(),
            windows: args[2].clone(),
        })
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
