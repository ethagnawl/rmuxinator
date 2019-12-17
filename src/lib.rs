use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

extern crate toml;

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    println!("Config: {:#?}", config);

    let session_name = config.name;

    let create_session_args = [
        "new-session",
        "-d",
        "-s",
        &session_name,
        // can this be done when the other windows are created?
        "-n",
        &config.windows[0].name,
    ];
    let _create_session_output = Command::new("tmux")
        .args(&create_session_args)
        .output()
        .expect("Unable to create session.");

    for (_, window) in config.windows.iter().enumerate() {
        // TODO: create window
        // for window.panes ...
        // for pane.commands ...
        for (_, command) in window.commands.iter().enumerate() {
            let window_command_args = [
                "send-keys",
                "-t",
                &format!("{}:0.0", session_name),
                &command,
                "Enter",
            ];
            let _window_command_output = Command::new("tmux")
                .args(&window_command_args)
                .output()
                .expect("Unable to run window command.");
        }
    }

    let attach_args = ["-u", "attach-session", "-t", &session_name];
    let _attach_output =
        Command::new("tmux").args(&attach_args).spawn()?.wait();

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
pub struct Window {
    pub name: String,
    pub root: String,
    pub commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub name: String,
    pub root: String,
    pub windows: Vec<Window>,
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
