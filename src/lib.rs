use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

extern crate toml;

fn run_tmux_command(command: &Vec<String>, error_message: &String) {
    Command::new("tmux")
        .args(command)
        .output()
        .expect(error_message);
}

fn build_pane_args(session_name: &String, window_index: &usize) -> Vec<String> {
    vec![
        String::from("split-window"),
        String::from("-t"),
        format!("{}:{}", session_name, window_index.to_string()),
    ]
}

fn build_window_layout_args(
    session_name: &String,
    window_index: &usize,
    config_layout: &Option<Layout>,
    window_layout: &Option<Layout>,
) -> Option<Vec<String>> {
    let maybe_layout = if window_layout.is_some() {
        &window_layout
    } else if config_layout.is_some() {
        &config_layout
    } else {
        &None
    };

    if let Some(layout) = maybe_layout {
        Some(vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", session_name, window_index.to_string()),
            String::from(layout.to_string()),
        ])
    } else {
        None
    }
}

fn set_window_layout(window_layout_args: Option<Vec<String>>) {
    if window_layout_args.is_some() {
        let args = window_layout_args.unwrap();
        Command::new("tmux")
            .args(&args)
            .output()
            .expect("Unable to set window layout.");
    }
}

fn build_create_window_args(
    session_name: &String,
    window_index: usize,
    window_name: &String,
    start_directory: &Option<String>,
) -> Vec<String> {
    let mut create_window_args = vec![
        String::from("new-window"),
        String::from("-t"),
        format!("{}:{}", session_name, window_index.to_string()),
        String::from("-n"),
        String::from(window_name),
    ];

    if let Some(start_directory_) = start_directory {
        create_window_args.push(String::from("-c"));
        create_window_args.push(String::from(start_directory_));
    }

    create_window_args
}

fn build_session_args(
    session_name: &String,
    window_name: &String,
    start_directory: &StartDirectory,
) -> Vec<String> {
    // Pass first window name to new-session, otherwise a default window gets
    // created that would need to be killed at a later point. I tried doing
    // this, but saw unexpected behavior -- most likely because the indexes get
    // shuffled.
    let mut session_args = vec![
        String::from("new-session"),
        String::from("-d"),
        String::from("-s"),
        String::from(session_name),
        String::from("-n"),
        String::from(window_name),
    ];

    if let Some(start_directory_) = start_directory {
        session_args.push(String::from("-c"));
        session_args.push(String::from(start_directory_));
    }

    session_args
}

fn build_pane_command_args(
    session_name: &String,
    window_index: &usize,
    pane_index: &usize,
    command: &String,
) -> Vec<String> {
    vec![
        String::from("send-keys"),
        String::from("-t"),
        format!("{}:{}.{}", session_name, window_index, pane_index),
        String::from(command),
        String::from("Enter"),
    ]
}

fn build_attach_args(session_name: &String) -> Vec<String> {
    vec![
        String::from("-u"),
        String::from("attach-session"),
        String::from("-t"),
        String::from(session_name),
    ]
}

fn build_session_start_directory(config: &Config) -> StartDirectory {
    // Compute start_directory for session/first window using:
    // window.start_directory || config.start_directory
    if config.windows.len() > 0 {
        config.windows[0].start_directory.clone()
    } else {
        config.start_directory.clone()
    }
}

fn build_window_start_directory(
    config_start_directory: &StartDirectory,
    window_start_directory: &StartDirectory,
) -> StartDirectory {
    let config_start_directory_ = config_start_directory.clone();
    let window_start_directory_ = window_start_directory.clone();
    window_start_directory_.or(config_start_directory_)
}

fn build_pane_start_directory(
    config_start_directory: &StartDirectory,
    window_start_directory: &StartDirectory,
    pane_start_directory: &StartDirectory,
) -> StartDirectory {
    let config_start_directory_ = config_start_directory.clone();
    let window_start_directory_ = window_start_directory.clone();
    let pane_start_directory_ = pane_start_directory.clone();
    pane_start_directory_
        .or(window_start_directory_)
        .or(config_start_directory_)
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let session_name = &config.name;

    let session_start_directory = build_session_start_directory(&config);

    let create_session_args = build_session_args(
        session_name,
        &config.windows[0].name,
        &session_start_directory,
    );
    let error_message = String::from("Unable to create session.");
    run_tmux_command(&create_session_args, &error_message);

    for hook in &config.hooks {
        let hook_error_message = format!("Unable to run set hook command");
        let hook_command = vec![
            String::from("set-hook"),
            hook.name.to_string(),
            hook.command.to_string(),
        ];
        run_tmux_command(&hook_command, &hook_error_message);
    }

    for (window_index, window) in config.windows.iter().enumerate() {
        // The first window is created by create_session because tmux always
        // creates a window when creating a session.
        // The alternative would be to create all of the project windows and
        // then kill the first/default one, but I saw unexpected behavior
        // (first window's commands not being run) when attempting that -- I
        // think it's because the indexes get shuffled.
        // The alternative approach would be more explicit and preferable, so
        // maybe it's worth revisiting.
        if window_index != 0 {
            // TODO: This is heavy handed and this logic is _sort of_ duped
            // in a few places. Maybe each type should have a method which is
            // able to compute its own starting directory?
            let window_start_directory = build_window_start_directory(
                &config.start_directory,
                &window.start_directory,
            );
            let create_window_args = build_create_window_args(
                session_name,
                window_index,
                &window.name,
                &window_start_directory,
            );
            let error_message =
                String::from("Unable to run create window command.");
            run_tmux_command(&create_window_args, &error_message);
        }

        for (pane_index, pane) in window.panes.iter().enumerate() {
            // Pane 0 is created by default by the containing window
            if pane_index > 0 {
                let pane_args = build_pane_args(session_name, &window_index);
                let error_message =
                    String::from("Unable to run create pane command.");
                run_tmux_command(&pane_args, &error_message);
            }

            // Conditionally set start_directory for pane.
            // Unfortunately, this can't be done cleanly using create_pane
            // because pane 0 is created implicitly.
            let pane_start_directory = build_pane_start_directory(
                &config.start_directory,
                &window.start_directory,
                &pane.start_directory,
            );
            if let Some(pane_start_directory) = pane_start_directory {
                let command = format!("cd {}", pane_start_directory);
                let pane_command_args = build_pane_command_args(
                    session_name,
                    &window_index,
                    &pane_index,
                    &command,
                );

                let error_message = String::from(
                    "Unable to run set start_directory command for pane.",
                );
                run_tmux_command(&pane_command_args, &error_message);
            }

            for (_, command) in pane.commands.iter().enumerate() {
                let pane_command_args = build_pane_command_args(
                    session_name,
                    &window_index,
                    &pane_index,
                    command,
                );
                let error_message = String::from("Unable to run pane command.");
                run_tmux_command(&pane_command_args, &error_message);
            }

            // requires tmux >= 3.0a and some variation of the following in
            // tmux.conf:
            // set -g pane-border-format "#{@mytitle}"
            // TODO: consider setting pane-border-format user option to
            // something unique and dynamic to prevent collisions
            // TODO: sniff out user option support
            // TODO: make user option configurable
            if let Some(pane_name) = pane.name.clone() {
                // TODO: move into helper
                let rename_pane_args = vec![
                    String::from("set-option"),
                    String::from("-p"),
                    String::from("-t"),
                    format!("{}:{}.{}", session_name, window_index, pane_index),
                    String::from("@mytitle"),
                    String::from(pane_name),
                ];
                let error_message = String::from("Unable to run pane command.");
                run_tmux_command(&rename_pane_args, &error_message);
            }
        }

        let window_layout_args = build_window_layout_args(
            session_name,
            &window_index,
            &config.layout,
            &window.layout,
        );
        set_window_layout(window_layout_args)
    }

    // TODO: Move this into helper. First attempt resulted in error caused by
    // return type. I think I either need to return the command and then spawn
    // or return the result of calling spawn.
    let attach_args = build_attach_args(&session_name);
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
        Ok(CliArgs {
            command: args[1].clone(),
            project_name: args[2].clone(),
        })
    }
}

#[derive(Debug, Deserialize)]
pub enum Layout {
    #[serde(rename = "even-horizontal")]
    EvenHorizontal,
    #[serde(rename = "even-vertical")]
    EvenVertical,
    #[serde(rename = "main-horizontal")]
    MainHorizontal,
    #[serde(rename = "main-vertical")]
    MainVertical,
    #[serde(rename = "tiled")]
    Tiled,
}

impl Layout {
    fn to_string(&self) -> String {
        match self {
            Self::EvenHorizontal => String::from("even-horizontal"),
            Self::EvenVertical => String::from("even-vertical"),
            Self::MainHorizontal => String::from("main-horizontal"),
            Self::MainVertical => String::from("main-vertical"),
            Self::Tiled => String::from("tiled"),
        }
    }
}

type StartDirectory = Option<String>;

#[derive(Debug, Deserialize)]
pub struct Pane {
    pub commands: Vec<String>,
    pub name: Option<String>,
    pub start_directory: StartDirectory,
}

#[derive(Debug, Deserialize)]
pub struct Window {
    pub layout: Option<Layout>,
    pub name: String,
    pub panes: Vec<Pane>,
    pub start_directory: StartDirectory,
}

#[derive(Debug, Deserialize)]
pub enum HookName {
    #[serde(rename = "pane-focus-in")]
    PaneFocusIn,
}

impl HookName {
    fn to_string(&self) -> String {
        match self {
            Self::PaneFocusIn => String::from("pane-focus-in"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Hook {
    name: HookName,
    command: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub hooks: Vec<Hook>,
    pub layout: Option<Layout>,
    pub name: String,
    pub start_directory: StartDirectory,
    pub windows: Vec<Window>,
}

impl Config {
    pub fn new(cli_args: CliArgs) -> Result<Config, String> {
        let mut f = match File::open(cli_args.project_name) {
            Ok(x) => x,
            Err(_) => return Err(String::from("Unable to open config file.")),
        };
        let mut contents = String::new();

        match f.read_to_string(&mut contents) {
            Ok(_) => (),
            Err(_) => return Err(String::from("Unable to read config file.")),
        }

        let decoded = toml::from_str(&contents);

        match decoded {
            Ok(config) => Ok(config),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds_session_args_without_start_directory() {
        let session_name = String::from("a session");
        let window_name = String::from("a window");
        let start_directory = None;
        let expected = vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from(&session_name),
            String::from("-n"),
            String::from(&window_name),
        ];
        let actual =
            build_session_args(&session_name, &window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_session_args_with_start_directory() {
        let session_name = String::from("a session");
        let window_name = String::from("a window");
        let start_directory_ = String::from("/foo/bar");
        let start_directory = Some(start_directory_.clone());
        let expected = vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from(&session_name),
            String::from("-n"),
            String::from(&window_name),
            String::from("-c"),
            String::from(&start_directory_),
        ];
        let actual =
            build_session_args(&session_name, &window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_window_layout_args_without_a_window_layout_or_a_config_layout()
    {
        let session_name = String::from("foo");
        let window_index = 2;
        let config_layout = None;
        let window_layout = None;
        let expected = None;
        let actual = build_window_layout_args(
            &session_name,
            &window_index,
            &config_layout,
            &window_layout,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_window_layout_args_with_a_config_layout_and_no_window_layout()
    {
        let session_name = String::from("foo");
        let window_index = 2;
        let config_layout = Some(Layout::EvenHorizontal);
        let window_layout = None;
        let expected = vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("even-horizontal"), // <~~ TODO: LAZY
        ];
        let actual = build_window_layout_args(
            &session_name,
            &window_index,
            &config_layout,
            &window_layout,
        );
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_builds_window_layout_args_with_a_window_layout_and_no_config_layout()
    {
        let session_name = String::from("foo");
        let window_index = 2;
        let config_layout = None;
        let window_layout = Some(Layout::Tiled);
        let expected = vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("tiled"), // <~~ TODO: LAZY
        ];
        let actual = build_window_layout_args(
            &session_name,
            &window_index,
            &config_layout,
            &window_layout,
        );
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_builds_window_layout_args_with_a_window_layout_and_a_config_layout() {
        let session_name = String::from("foo");
        let window_index = 2;
        let config_layout = Some(Layout::Tiled);
        let window_layout = Some(Layout::EvenHorizontal);
        let expected = vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("even-horizontal"), // <~~ TODO: LAZY
        ];
        let actual = build_window_layout_args(
            &session_name,
            &window_index,
            &config_layout,
            &window_layout,
        );
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_builds_window_args_without_a_start_directory() {
        let session_name = String::from("a session");
        let window_name = String::from("a window");
        let window_index = 42;
        let start_directory = None;
        let expected = vec![
            String::from("new-window"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("-n"),
            String::from(&window_name),
        ];
        let actual = build_create_window_args(
            &session_name,
            window_index,
            &window_name,
            &start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_window_args_with_a_start_directory() {
        let session_name = String::from("a session");
        let window_name = String::from("a window");
        let window_index = 42;
        let start_directory = Some(String::from("/tmp/neat"));

        let expected = vec![
            String::from("new-window"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("-n"),
            String::from(&window_name),
            String::from("-c"),
            String::from("/tmp/neat"),
        ];
        let actual = build_create_window_args(
            &session_name,
            window_index,
            &window_name,
            &start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_attach_args() {
        let session_name = String::from("a session");
        let expected = vec![
            String::from("-u"),
            String::from("attach-session"),
            String::from("-t"),
            String::from(&session_name),
        ];
        let actual = build_attach_args(&session_name);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_converts_layout_to_string() {
        let layout = Layout::Tiled;
        let expected = layout.to_string();
        let actual = String::from("tiled");
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_start_directory_when_none_present_for_session_start_directory(
    ) {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: Vec::new(),
                start_directory: None,
            }],
        };

        let expected = None;
        let actual = build_session_start_directory(&config);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_configs_start_directory_when_no_window_start_directory_present_for_session_start_directory(
    ) {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/foo/bar")),
            windows: Vec::new(),
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_session_start_directory(&config);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_windows_start_directory_over_configs_start_directory_for_session_start_directory(
    ) {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/this/is/ignored")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: Vec::new(),
                start_directory: Some(String::from("/bar/baz")),
            }],
        };
        let expected = Some(String::from("/bar/baz"));
        let actual = build_session_start_directory(&config);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_start_directory_when_none_present_for_window_start_directory()
    {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: Vec::new(),
                start_directory: None,
            }],
        };
        let expected = None;
        let actual = build_window_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_windows_start_directory_over_configs_start_directory_for_window_start_directory(
    ) {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/this/is/ignored")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: Vec::new(),
                start_directory: Some(String::from("/bar/baz")),
            }],
        };
        let expected = Some(String::from("/bar/baz"));
        let actual = build_window_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_configs_start_directory_when_no_window_start_directory_present_for_window_start_directory(
    ) {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/foo/bar")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: Vec::new(),
                start_directory: None,
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_window_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_none_and_config_sd_is_none() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: Some(String::from("/foo/bar")),
                }],
                start_directory: None,
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_some_and_config_sd_is_none() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: Some(String::from("/foo/bar")),
                }],
                start_directory: Some(String::from("/bar/baz")),
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_none_and_config_sd_is_some() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/bar/baz")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: Some(String::from("/foo/bar")),
                }],
                start_directory: None,
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_some_and_config_sd_is_some() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/bar/baz")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: Some(String::from("/foo/bar")),
                }],
                start_directory: Some(String::from("/bar/baz")),
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_window_sd_when_pane_sd_is_none_and_config_sd_is_none() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: None,
                }],
                start_directory: Some(String::from("/foo/bar")),
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_window_sd_when_pane_sd_is_none_and_config_sd_is_some() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/bar/baz")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: None,
                }],
                start_directory: Some(String::from("/foo/bar")),
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_config_sd_when_pane_sd_is_none_and_config_sd_is_none() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/foo/bar")),
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: None,
                }],
                start_directory: None,
            }],
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_pane_sd_when_none_are_set() {
        let config = Config {
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: String::from("a window"),
                panes: vec![Pane {
                    commands: vec![],
                    name: None,
                    start_directory: None,
                }],
                start_directory: None,
            }],
        };
        let expected = None;
        let actual = build_pane_start_directory(
            &config.start_directory,
            &config.windows[0].start_directory,
            &config.windows[0].panes[0].start_directory,
        );
        assert_eq!(expected, actual);
    }
}
