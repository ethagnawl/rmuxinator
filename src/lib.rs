use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process::Command;

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
    window_name: &str,
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
    session_name: &str,
    window_name: &str,
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

pub fn test_for_tmux(tmux_command: String) -> bool {
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

    let hook_error_message = String::from("Unable to run set hook command");
    for hook in config.hooks {
        let hook_command = build_hook_args(&hook);
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

            let rename_pane_args = build_rename_pane_args(
                session_name,
                window_index,
                pane_index,
                &config.pane_name_user_option,
                &pane.name.clone(),
            );
            let error_message =
                String::from("Unable to run rename pane command.");
            if let Some(rename_pane_args_) = rename_pane_args {
                run_tmux_command(&rename_pane_args_, &error_message);
            }
        }

        let window_layout_args = build_window_layout_args(
            session_name,
            &window_index,
            &config.layout,
            &window.layout,
        );

        if let Some(window_layout_args_) = window_layout_args {
            let error_message = String::from("Unable to set window layout.");
            run_tmux_command(&window_layout_args_, &error_message)
        }
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
enum CliCommand {
    Start,
}

impl CliCommand {
    fn new(maybe_command: &str) -> Result<CliCommand, String> {
        match maybe_command {
            "start" => Ok(Self::Start),
            // TODO: present static list of valid options instead?
            _ => Err(format!("Command ({}) not recognized.", maybe_command)),
        }
    }
}

#[derive(Debug)]
pub struct CliArgs {
    command: CliCommand,
    project_name: String,
}

impl CliArgs {
    pub fn new(args: &[String]) -> Result<CliArgs, String> {
        // TODO: None of this scales very well, but I wanted to see if I could
        // avoid using a third-party library. Maybe it's worth trying clap or
        // quicli.

        let args_ = args.to_owned();
        // drop entrypoint (e.g. ./rmuxinator)
        let mut args_ = args_.iter().skip(1);
        let command_ = args_.next();
        let project_ = args_.next();

        if command_.is_none() {
            return Err(String::from("Command is required."));
        }

        if project_.is_none() {
            return Err(String::from("Project is required."));
        }

        let maybe_command = CliCommand::new(command_.unwrap());
        if let Ok(maybe_command_) = maybe_command {
            Ok(CliArgs {
                command: maybe_command_,
                project_name: project_.unwrap().clone(),
            })
        } else {
            Err(maybe_command.unwrap_err())
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Layout {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

impl Layout {
    fn to_string(&self) -> String {
        // Get arm name from Debug
        let arm_name = format!("{:?}", self);
        // Make the string kebab-case to match tmux's usage
        convert_pascal_case_to_kebab_case(&arm_name)
    }
}

type StartDirectory = Option<String>;

#[derive(Debug, Deserialize)]
struct Pane {
    commands: Vec<String>,
    name: Option<String>,
    start_directory: StartDirectory,
}

#[derive(Debug, Deserialize)]
struct Window {
    layout: Option<Layout>,
    name: String,
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

impl HookName {
    fn to_string(&self) -> String {
        // Get arm name from Debug
        let arm_name = format!("{:?}", self);
        // Make the string kebab-case to match tmux's usage
        convert_pascal_case_to_kebab_case(&arm_name)
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
    windows: Vec<Window>,
}

impl Config {
    pub fn new(cli_args: CliArgs) -> Result<Config, String> {
        // Need to return String in failure case because toml::from_str may
        // return a toml::de::Error.
        let mut config_file = match File::open(cli_args.project_name) {
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
        let pascal = String::from("KebabCase");
        let expected = String::from("kebab-case");
        let actual = convert_pascal_case_to_kebab_case(&pascal);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_no_ops_on_a_non_pascal_case_string() {
        let pascal = String::from("foo");
        let expected = String::from("foo");
        let actual = convert_pascal_case_to_kebab_case(&pascal);
        assert_eq!(expected, actual);
    }

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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
    fn it_uses_windows_start_directory_over_configs_start_directory_for_session_start_directory(
    ) {
        let config = Config {
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
            pane_name_user_option: None,
            hooks: Vec::new(),
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
    fn it_builds_rename_pane_args_when_pane_name_and_pane_name_user_option_present(
    ) {
        let session_name = String::from("session-name");
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
        let session_name = String::from("session-name");
        let window_index = 3;
        let pane_index = 4;
        let pane_name_user_option = Some(String::from("pane_name_user_option"));
        let pane_name = None;
        let expected = None;
        let actual = build_rename_pane_args(
            &session_name,
            window_index,
            pane_index,
            &pane_name_user_option,
            &pane_name,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_doesnt_build_rename_pane_args_when_no_pane_name_user_option_present()
    {
        let session_name = String::from("session-name");
        let window_index = 3;
        let pane_index = 4;
        let pane_name_user_option = None;
        let pane_name = Some(String::from("pane-name"));
        let expected = None;
        let actual = build_rename_pane_args(
            &session_name,
            window_index,
            pane_index,
            &pane_name_user_option,
            &pane_name,
        );
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_accepts_valid_cli_command_arg() {
        let expected = true;
        let actual = CliCommand::new(&String::from("start")).is_ok();
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_rejects_valid_cli_command_arg() {
        let expected = true;
        let actual = CliCommand::new(&String::from("xtart")).is_ok();
        assert_ne!(expected, actual);
    }

    #[test]
    fn cli_args_requires_a_command_arg() {
        let args = vec![String::from("rmuxinator")];
        let expected = String::from("Command is required.");
        let actual = CliArgs::new(&args);
        assert_eq!(expected, actual.unwrap_err());
    }

    #[test]
    fn cli_args_requires_a_project_arg() {
        let args = vec![String::from("rmuxinator"), String::from("start")];
        let expected = String::from("Project is required.");
        let actual = CliArgs::new(&args);
        assert_eq!(expected, actual.unwrap_err());
    }

    #[test]
    fn test_for_tmux_returns_true_when_tmux_exists() {
        let expected = true;
        let actual = test_for_tmux(String::from("tmux"));
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_for_tmux_returns_false_when_tmux_doesnt_exist() {
        let expected = false;
        let actual = test_for_tmux(String::from("xmux"));
        assert_eq!(expected, actual);
    }
}
