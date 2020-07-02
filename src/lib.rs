use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;
use std::str::FromStr;

use clap::{App, AppSettings, Arg, SubCommand};
use serde::Deserialize;

extern crate toml;

fn run_tmux_command(command: &[String], error_message: &str) {
    Command::new("tmux")
        .args(command)
        .output()
        .expect(error_message);
}

fn build_pane_args(session_name: &str, window_index: &usize) -> Vec<String> {
    vec![
        String::from("split-window"),
        String::from("-t"),
        format!("{}:{}", session_name, window_index.to_string()),
    ]
}

fn build_window_layout_args(
    session_name: &str,
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
            layout.to_string(),
        ])
    } else {
        None
    }
}

fn build_create_window_args(
    session_name: &str,
    window_index: usize,
    window_name: &Option<String>,
    start_directory: &Option<String>,
) -> Vec<String> {
    let mut create_window_args = vec![
        String::from("new-window"),
        String::from("-t"),
        format!("{}:{}", session_name, window_index.to_string()),
    ];

    if let Some(_window_name) = window_name {
        create_window_args.push(String::from("-n"));
        create_window_args.push(_window_name.to_string());
    }

    if let Some(start_directory_) = start_directory {
        create_window_args.push(String::from("-c"));
        create_window_args.push(String::from(start_directory_));
    }

    create_window_args
}

fn build_session_args(
    session_name: &str,
    window_name: Option<String>,
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
    ];

    if let Some(_window_name) = window_name {
        session_args.push(String::from("-n"));
        session_args.push(_window_name);
    }

    if let Some(start_directory_) = start_directory {
        session_args.push(String::from("-c"));
        session_args.push(String::from(start_directory_));
    }

    session_args
}

fn build_pane_command_args(
    session_name: &str,
    window_index: &usize,
    pane_index: &usize,
    command: &str,
) -> Vec<String> {
    vec![
        String::from("send-keys"),
        String::from("-t"),
        format!("{}:{}.{}", session_name, window_index, pane_index),
        String::from(command),
        String::from("Enter"),
    ]
}

fn build_attach_args(session_name: &str) -> Vec<String> {
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
    if !config.windows.is_empty() {
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

fn build_hook_args(hook: &Hook) -> Vec<String> {
    vec![
        String::from("set-hook"),
        String::from("-a"),
        hook.name.to_string(),
        hook.command.to_string(),
    ]
}

fn build_rename_pane_args(
    session_name: &str,
    window_index: usize,
    pane_index: usize,
    pane_name_user_option: &Option<String>,
    pane_name: &Option<String>,
) -> Option<Vec<String>> {
    // requires tmux >= 3.0a and some variation of the following in
    // tmux.conf:
    // e.g. `set -g pane-border-format "#{@user_option}"`
    // TODO: Is it worth sniffing out user option support?
    if pane_name.is_some() && pane_name_user_option.is_some() {
        Some(vec![
            String::from("set-option"),
            String::from("-p"),
            String::from("-t"),
            format!("{}:{}.{}", session_name, window_index, pane_index),
            format!("@{}", pane_name_user_option.clone().unwrap()),
            pane_name.clone().unwrap(),
        ])
    } else {
        None
    }
}

pub fn test_for_tmux(tmux_command: &str) -> bool {
    // TODO: an optarg would be nice, but they're not currently supported.
    // This parameter exists only to facilitate testing and the main caller
    // will never _need_ to pass anything non-standard.
    let mut shell = Command::new("sh");
    let output = shell
        .arg("-c")
        .arg(format!("command -v {}", tmux_command))
        .output()
        .expect("Unable to test for tmux.");
    output.status.success()
}

fn convert_config_to_tmux_commands(config: &Config) -> Vec<Vec<String>> {
    let mut commands = vec![];

    let session_name = &config.name;

    let session_start_directory = build_session_start_directory(&config);

    let first_window = if let Some(window) = config.windows.get(0) {
        window.name.clone()
    } else {
        None
    };

    let create_session_args =
        build_session_args(session_name, first_window, &session_start_directory);
    commands.push(create_session_args);

    for hook in config.hooks.iter() {
        let hook_command = build_hook_args(&hook);
        commands.push(hook_command.clone());
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
            let window_start_directory =
                build_window_start_directory(&config.start_directory, &window.start_directory);
            let create_window_args = build_create_window_args(
                session_name,
                window_index,
                &window.name,
                &window_start_directory,
            );

            commands.push(create_window_args.clone());
        }

        for (pane_index, pane) in window.panes.iter().enumerate() {
            // Pane 0 is created by default by the containing window
            if pane_index > 0 {
                let pane_args = build_pane_args(session_name, &window_index);

                commands.push(pane_args.clone());
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
                let pane_command_args =
                    build_pane_command_args(session_name, &window_index, &pane_index, &command);

                commands.push(pane_command_args.clone());
            }

            for (_, command) in pane.commands.iter().enumerate() {
                let pane_command_args =
                    build_pane_command_args(session_name, &window_index, &pane_index, command);
                commands.push(pane_command_args.clone());
            }

            let rename_pane_args = build_rename_pane_args(
                session_name,
                window_index,
                pane_index,
                &config.pane_name_user_option,
                &pane.name.clone(),
            );
            if let Some(rename_pane_args_) = rename_pane_args {
                commands.push(rename_pane_args_.clone());
            }
        }

        let window_layout_args =
            build_window_layout_args(session_name, &window_index, &config.layout, &window.layout);

        if let Some(window_layout_args_) = window_layout_args {
            commands.push(window_layout_args_.clone());
        }
    }

    commands
}

pub fn run_start(config: Config) -> Result<(), Box<dyn Error>> {
    let commands = convert_config_to_tmux_commands(&config);

    for command in commands.iter() {
        run_tmux_command(&command, "error");
    }

    // TODO: Move this into helper. First attempt resulted in error caused by
    // return type. I think I either need to return the command and then spawn
    // or return the result of calling spawn.
    let attach_args = build_attach_args(&config.name);
    let _attach_output = Command::new("tmux").args(&attach_args).spawn()?.wait();

    Ok(())
}

pub fn run_debug(config: Config) -> Result<(), Box<dyn Error>> {
    let commands = convert_config_to_tmux_commands(&config)
        .iter()
        .map(|v| v.join(" "))
        .collect::<Vec<String>>();
    for command in commands.iter() {
        println!("tmux {}", command);
    }

    Ok(())
}

pub fn parse_args<I, T>(args: I) -> CliArgs
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let app_matches = App::new(clap::crate_name!())
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .about(clap::crate_description!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("debug")
                .about("Review the tmux commands that would be used to start and configure a tmux session using a path to a project config file")
                .arg(
                    Arg::with_name("PROJECT_CONFIG_FILE")
                        .help("The path to the project config file")
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("start")
                .about("Start a tmux session using a path to a project config file")
                .arg(
                    Arg::with_name("PROJECT_CONFIG_FILE")
                        .help("The path to the project config file")
                        .required(true),
                ),
        )
        .get_matches_from(args);

    let (command_name, command_matches) = match app_matches.subcommand() {
        (name, Some(matches)) => (name, matches),
        (_, None) => {
            panic!("Subcommand should be forced by clap");
        }
    };

    let command = match CliCommand::from_str(command_name) {
        Ok(command) => command,
        Err(error) => {
            panic!(error.to_string());
        }
    };

    let project_name = command_matches
        .value_of("PROJECT_CONFIG_FILE")
        .expect("project file is required by clap")
        .to_string();

    CliArgs {
        command,
        project_name,
    }
}

#[derive(Debug, PartialEq)]
pub enum CliCommand {
    Debug,
    Start,
}

#[derive(Debug)]
pub struct ParseCliCommandError;

// TODO: this boilerplate can be cut down by using a third-party crate
impl fmt::Display for ParseCliCommandError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Missing implementation for subcommand, please file a bug report"
        )
    }
}

impl Error for ParseCliCommandError {}

impl FromStr for CliCommand {
    type Err = ParseCliCommandError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "debug" => Ok(Self::Debug),
            "start" => Ok(Self::Start),
            // This should only ever be reached if subcommands are added to
            // clap and not here
            _ => Err(ParseCliCommandError),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct CliArgs {
    pub command: CliCommand,
    pub project_name: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Layout {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pascal_case_hook_name = format!("{:?}", self);
        let kebab_case_hook_name = convert_pascal_case_to_kebab_case(&pascal_case_hook_name);
        write!(f, "{}", kebab_case_hook_name)
    }
}

type StartDirectory = Option<String>;

#[derive(Debug, Default, Deserialize)]
struct Pane {
    commands: Vec<String>,
    name: Option<String>,
    start_directory: StartDirectory,
}

#[derive(Debug, Default, Deserialize)]
struct Window {
    layout: Option<Layout>,
    name: Option<String>,
    #[serde(default)]
    panes: Vec<Pane>,
    start_directory: StartDirectory,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum HookName {
    // TODO: Does this make sense? If not, document exclusion.
    // AfterNewSession,

    // TODO: why doesn't this fire? It seems to be valid, but isn't ever
    // triggered.
    // AfterKillPane,
    // AfterKillWindow,
    AfterBindKey,
    AfterCapturePane,
    AfterCopyMode,
    AfterCursorDown,
    AfterDisplayPanes,
    AfterListClients,
    AfterListKeys,
    AfterListPanes,
    AfterListSessions,
    AfterListWindows,
    AfterNewWindow,
    AfterPipePane,
    AfterRefreshClient,
    AfterRenameSession,
    AfterRenameWindow,
    AfterResizePane,
    AfterResizeWindow,
    AfterSelectLayout,
    AfterSelectPane,
    AfterSelectWindow,
    AfterSendKeys,
    AfterSetOption,
    AfterShowMessages,
    AfterShowOptions,
    AfterSplitWindow,
    AfterUnbindKey,
    AlertActivity,
    AlertBell,
    AlertSilence,
    ClientAttached,
    ClientDetached,
    ClientResized,
    ClientSessionChanged,
    LayoutChange,
    Output,
    PaneDied,
    PaneExited,
    PaneFocusIn,
    PaneFocusOut,
    PaneModeChanged,
    PaneSetClipboard,
    SessionChanged,
    SessionClosed,
    SessionCreated,
    SessionRenamed,
    SessionWindowChanged,
    SessionsChanged,
    UnlinkedWindowAdd,
    WindowAdd,
    WindowClose,
    WindowLinked,
    WindowPaneChanged,
    WindowRenamed,
    WindowUnlinked,
}

impl fmt::Display for HookName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pascal_case_hook_name = format!("{:?}", self);
        let kebab_case_hook_name = convert_pascal_case_to_kebab_case(&pascal_case_hook_name);
        write!(f, "{}", kebab_case_hook_name)
    }
}

#[derive(Debug, Deserialize)]
struct Hook {
    command: String,
    name: HookName,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pane_name_user_option: Option<String>,
    #[serde(default)]
    hooks: Vec<Hook>,
    layout: Option<Layout>,
    name: String,
    start_directory: StartDirectory,
    #[serde(default)]
    windows: Vec<Window>,
}

impl Config {
    pub fn new(cli_args: &CliArgs) -> Result<Config, String> {
        // Need to return String in failure case because toml::from_str may
        // return a toml::de::Error.
        let mut config_file = match File::open(&cli_args.project_name) {
            Ok(file) => file,
            Err(_) => return Err(String::from("Unable to open config file.")),
        };
        let mut contents = String::new();

        match config_file.read_to_string(&mut contents) {
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

/// Convert a PascalCase string to a kebab-case string
fn convert_pascal_case_to_kebab_case(input: &str) -> String {
    // Split string by uppercase characters and join with '-'
    // TODO: This can be simplified once `.split_inclusive()` stablizes
    input.chars().fold(String::from(""), |mut acc, mut c| {
        // Separate uppercase letters by '-' after the first
        if !acc.is_empty() && c.is_ascii_uppercase() {
            acc.push('-');
        }
        c.make_ascii_lowercase();
        acc.push(c);
        acc
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_converts_a_pascal_case_string_to_a_kebab_case_string() {
        let pascal = "KebabCase";
        let expected = "kebab-case";
        let actual = convert_pascal_case_to_kebab_case(&pascal);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_no_ops_on_a_non_pascal_case_string() {
        let pascal = "foo";
        let expected = "foo";
        let actual = convert_pascal_case_to_kebab_case(&pascal);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_session_args_without_start_directory() {
        let session_name = "a session";
        let window_name = Some(String::from("a window"));
        let start_directory = None;
        let expected = vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from(session_name),
            String::from("-n"),
            window_name.clone().unwrap(),
        ];
        let actual = build_session_args(&session_name, window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_session_args_with_window_name() {
        let session_name = String::from("a session");
        let window_name = Some(String::from("a window"));
        let start_directory = None;
        let expected = vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from(&session_name),
            String::from("-n"),
            window_name.clone().unwrap(),
        ];
        let actual = build_session_args(&session_name, window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_session_args_without_window_name() {
        let session_name = String::from("a session");
        let window_name = None;
        let start_directory = None;
        let expected = vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from(&session_name),
        ];
        let actual = build_session_args(&session_name, window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_session_args_with_start_directory() {
        let session_name = "a session";
        let window_name = Some(String::from("a window"));
        let start_directory_ = String::from("/foo/bar");
        let start_directory = Some(start_directory_.clone());
        let expected = vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from(session_name),
            String::from("-n"),
            window_name.clone().unwrap(),
            String::from("-c"),
            String::from(start_directory_),
        ];
        let actual = build_session_args(&session_name, window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_window_layout_args_without_a_window_layout_or_a_config_layout() {
        let session_name = "foo";
        let window_index = 2;
        let config_layout = None;
        let window_layout = None;
        let actual =
            build_window_layout_args(&session_name, &window_index, &config_layout, &window_layout);
        assert!(actual.is_none());
    }

    #[test]
    fn it_builds_window_layout_args_with_a_config_layout_and_no_window_layout() {
        let session_name = "foo";
        let window_index = 2;
        let config_layout = Some(Layout::EvenHorizontal);
        let window_layout = None;
        let expected = vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            config_layout.unwrap().to_string(),
        ];
        let actual =
            build_window_layout_args(&session_name, &window_index, &config_layout, &window_layout);
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_builds_window_layout_args_with_a_window_layout_and_no_config_layout() {
        let session_name = "foo";
        let window_index = 2;
        let config_layout = None;
        let window_layout = Some(Layout::Tiled);
        let expected = vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            window_layout.unwrap().to_string(),
        ];
        let actual =
            build_window_layout_args(&session_name, &window_index, &config_layout, &window_layout);
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_builds_window_layout_args_with_a_window_layout_and_a_config_layout() {
        let session_name = "foo";
        let window_index = 2;
        let config_layout = Some(Layout::Tiled);
        let window_layout = Some(Layout::EvenHorizontal);
        let expected = vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            window_layout.unwrap().to_string(),
        ];
        let actual =
            build_window_layout_args(&session_name, &window_index, &config_layout, &window_layout);
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_builds_window_args_without_a_start_directory() {
        let session_name = "a session";
        let window_name = Some(String::from("a window"));
        let window_index = 42;
        let start_directory = None;
        let expected = vec![
            String::from("new-window"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("-n"),
            window_name.clone().unwrap(),
        ];
        let actual =
            build_create_window_args(&session_name, window_index, &window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_window_args_with_a_start_directory() {
        let session_name = "a session";
        let window_name = Some(String::from("a window"));
        let window_index = 42;
        let start_directory = Some(String::from("/tmp/neat"));

        let expected = vec![
            String::from("new-window"),
            String::from("-t"),
            format!("{}:{}", &session_name, &window_index),
            String::from("-n"),
            window_name.clone().unwrap(),
            String::from("-c"),
            String::from("/tmp/neat"),
        ];
        let actual =
            build_create_window_args(&session_name, window_index, &window_name, &start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_attach_args() {
        let session_name = "a session";
        let expected = vec![
            String::from("-u"),
            String::from("attach-session"),
            String::from("-t"),
            String::from(session_name),
        ];
        let actual = build_attach_args(&session_name);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_converts_layout_to_string() {
        let layout = Layout::Tiled;
        let expected = String::from("tiled");
        let actual = layout.to_string();
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_start_directory_when_none_present_for_session_start_directory() {
        let config = Config {
            pane_name_user_option: None,
            hooks: Vec::new(),
            layout: None,
            name: String::from("foo"),
            start_directory: None,
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
        };

        let actual = build_session_start_directory(&config);
        assert!(actual.is_none());
    }

    #[test]
    fn it_uses_configs_start_directory_when_no_window_start_directory_present_for_session_start_directory(
    ) {
        let config = Config {
            pane_name_user_option: None,
            hooks: Vec::new(),
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
    fn it_uses_windows_start_directory_over_configs_start_directory_for_session_start_directory() {
        let config = Config {
            pane_name_user_option: None,
            hooks: Vec::new(),
            layout: None,
            name: String::from("foo"),
            start_directory: Some(String::from("/this/is/ignored")),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: Some(String::from("/bar/baz")),
            }],
        };
        let expected = Some(String::from("/bar/baz"));
        let actual = build_session_start_directory(&config);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_start_directory_when_none_present_for_window_start_directory() {
        let config_start_directory = None;
        let window_start_directory = None;

        let actual = build_window_start_directory(&config_start_directory, &window_start_directory);
        assert!(actual.is_none());
    }

    #[test]
    fn it_uses_windows_start_directory_over_configs_start_directory_for_window_start_directory() {
        let config_start_directory = Some(String::from("/this/is/ignored"));
        let window_start_directory = Some(String::from("/bar/baz"));

        let expected = window_start_directory.clone();
        let actual = build_window_start_directory(&config_start_directory, &window_start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_configs_start_directory_when_no_window_start_directory_present_for_window_start_directory(
    ) {
        let config_start_directory = Some(String::from("/foo/bar"));
        let window_start_directory = None;

        let expected = config_start_directory.clone();
        let actual = build_window_start_directory(&config_start_directory, &window_start_directory);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_none_and_config_sd_is_none() {
        let config_start_directory = None;
        let window_start_directory = None;
        let pane_start_directory = Some(String::from("/foo/bar"));

        let expected = pane_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_some_and_config_sd_is_none() {
        let config_start_directory = None;
        let window_start_directory = Some(String::from("/bar/baz"));
        let pane_start_directory = Some(String::from("/foo/bar"));

        let expected = pane_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_none_and_config_sd_is_some() {
        let config_start_directory = Some(String::from("/bar/baz"));
        let window_start_directory = None;
        let pane_start_directory = Some(String::from("/foo/bar"));

        let expected = pane_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_pane_sd_when_window_sd_is_some_and_config_sd_is_some() {
        let config_start_directory = Some(String::from("/bar/baz"));
        let window_start_directory = Some(String::from("/bar/baz"));
        let pane_start_directory = Some(String::from("/foo/bar"));

        let expected = pane_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_window_sd_when_pane_sd_is_none_and_config_sd_is_none() {
        let config_start_directory = None;
        let window_start_directory = Some(String::from("/foo/bar"));
        let pane_start_directory = None;

        let expected = window_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_window_sd_when_pane_sd_is_none_and_config_sd_is_some() {
        let config_start_directory = Some(String::from("/bar/baz"));
        let window_start_directory = Some(String::from("/foo/bar"));
        let pane_start_directory = None;

        let expected = window_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_config_sd_when_pane_sd_is_none_and_config_sd_is_none() {
        let config_start_directory = Some(String::from("/foo/bar"));
        let window_start_directory = None;
        let pane_start_directory = None;

        let expected = config_start_directory.clone();
        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_pane_sd_when_none_are_set() {
        let config_start_directory = None;
        let window_start_directory = None;
        let pane_start_directory = None;

        let actual = build_pane_start_directory(
            &config_start_directory,
            &window_start_directory,
            &pane_start_directory,
        );
        assert!(actual.is_none());
    }

    #[test]
    fn it_builds_hook_arguments() {
        let hook = Hook {
            command: String::from("run \"echo hi\""),
            name: HookName::PaneFocusIn,
        };
        let expected = vec![
            String::from("set-hook"),
            String::from("-a"),
            String::from("pane-focus-in"),
            String::from("run \"echo hi\""),
        ];
        let actual = build_hook_args(&hook);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_builds_rename_pane_args_when_pane_name_and_pane_name_user_option_present() {
        let session_name = "session-name";
        let window_index = 3;
        let pane_index = 4;
        let pane_name_user_option = Some(String::from("pane_name_user_option"));
        let pane_name = Some(String::from("pane-name"));
        let expected = vec![
            String::from("set-option"),
            String::from("-p"),
            String::from("-t"),
            format!("{}:{}.{}", session_name, window_index, pane_index),
            String::from("@pane_name_user_option"),
            String::from("pane-name"),
        ];
        let actual = build_rename_pane_args(
            &session_name,
            window_index,
            pane_index,
            &pane_name_user_option,
            &pane_name,
        );
        assert_eq!(expected, actual.unwrap());
    }

    #[test]
    fn it_doesnt_build_rename_pane_args_when_no_pane_name_present() {
        let session_name = "session-name";
        let window_index = 3;
        let pane_index = 4;
        let pane_name_user_option = Some(String::from("pane_name_user_option"));
        let pane_name = None;
        let actual = build_rename_pane_args(
            &session_name,
            window_index,
            pane_index,
            &pane_name_user_option,
            &pane_name,
        );
        assert!(actual.is_none());
    }

    #[test]
    fn it_doesnt_build_rename_pane_args_when_no_pane_name_user_option_present() {
        let session_name = "session-name";
        let window_index = 3;
        let pane_index = 4;
        let pane_name_user_option = None;
        let pane_name = Some(String::from("pane-name"));
        let actual = build_rename_pane_args(
            &session_name,
            window_index,
            pane_index,
            &pane_name_user_option,
            &pane_name,
        );
        assert!(actual.is_none());
    }

    #[test]
    fn it_computes_the_expected_commands() {
        let config = Config {
            hooks: vec![],
            layout: None,
            name: String::from("most basic config"),
            pane_name_user_option: None,
            start_directory: None,
            windows: vec![],
        };
        let expected = vec![vec![
            String::from("new-session"),
            String::from("-d"),
            String::from("-s"),
            String::from("most basic config"),
        ]];
        let actual = convert_config_to_tmux_commands(&config);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_accepts_valid_cli_command_arg() {
        let expected = CliCommand::Start;
        let actual = CliCommand::from_str("start").unwrap();
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_rejects_invalid_cli_command_arg() {
        let actual = CliCommand::from_str("xtart");
        assert!(actual.is_err());
    }

    #[test]
    fn it_accepts_correct_cli_args() {
        let expected = CliArgs {
            command: CliCommand::Start,
            project_name: String::from("Foo.toml"),
        };
        let args = vec!["rmuxinator", "start", "Foo.toml"];
        let actual = parse_args(args);
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_for_tmux_returns_true_when_tmux_exists() {
        let actual = test_for_tmux("tmux");
        assert!(actual);
    }

    #[test]
    fn test_for_tmux_returns_false_when_tmux_doesnt_exist() {
        let actual = test_for_tmux("xmux");
        assert!(!actual);
    }
}
