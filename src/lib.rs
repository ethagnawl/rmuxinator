use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

extern crate toml;

fn set_window_layout(_window_index: usize, _layout: &Layout) {
    let set_window_layout_args = ["select-layout", "-t", "foo:2", "tiled"];
    let _set_window_layout_output = Command::new("tmux")
        .args(&set_window_layout_args)
        .output()
        .expect("Unable to set window layout.");
}

fn create_window(
    session_name: &String,
    window_index: usize,
    window_name: &String,
) {
    let create_window_command_args = [
        "new-window",
        "-t",
        &format!("{}:{}", session_name, window_index),
        "-n",
        &window_name,
    ];

    let _create_window_command_output = Command::new("tmux")
        .args(&create_window_command_args)
        .output()
        .expect("Unable to run create window command.");
}

fn create_session(session_name: &String) {
    let create_session_args = ["new-session", "-d", "-s", session_name];
    let _create_session_output = Command::new("tmux")
        .args(&create_session_args)
        .output()
        .expect("Unable to create session.");
}

fn run_command(session_name: &String, window_index: &usize, command: &String) {
    let window_command_args = [
        "send-keys",
        "-t",
        &format!("{}:{}.0", session_name, window_index),
        &command,
        "Enter",
    ];
    let _window_command_output = Command::new("tmux")
        .args(&window_command_args)
        .output()
        .expect("Unable to run window command.");
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    println!("Config: {:#?}", config);

    let session_name = config.name;

    create_session(&session_name);

    for (window_index, window) in config.windows.iter().enumerate() {
        create_window(&session_name, window_index, &window.name);

        for (_, command) in window.commands.iter().enumerate() {
            run_command(&session_name, &window_index, &command);
        }

        match &window.layout {
            Some(layout) => set_window_layout(window_index, layout),
            None => (),
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
pub enum Layout {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

#[derive(Debug, Deserialize)]
pub struct Window {
    pub name: String,
    pub root: String,
    pub commands: Vec<String>,
    pub layout: Option<Layout>,
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
