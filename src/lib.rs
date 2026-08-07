use clap::{App, AppSettings, Arg, SubCommand};
use derivative::Derivative;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::str::FromStr;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

extern crate toml;

// The following TmuxCommandRunner business exists only to facilitate mocking.
// Coming from a dynamic language background, this does not smell right to me
// but I don't see any way around it.
// I may yet wind up yanking this out in favor of a different mocking library
// or strategy,
// We're mocking run_tmux_command instead of Command (via some combination of
// CommandFactor and CommandRunner), which feels slightly ... less bad because
// at least it's something we _own_ (in TDD parlance) but it's admittedly a bit
// weird and contrived.
// So, unless we _really_ want to verify that the conditional in
// run_tmux_command is dispatching the correct arguments to Command, this is
// probably good enough. In an ideal world, we would mock Command/args/output
// but doing so using mockall will require at least one additional layer of
// indirection and dependency injection and mocking to stub the command struct
// _and_ the instance it returns and the methods
// - ethagnawl

fn binary_exists(name: &str) -> bool {
    Command::new(name)
        .arg("-V")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_tmux_command(
    terminal_multiplexer: &String,
    command: &[String],
    wait: bool,
) -> Result<Output, Box<dyn Error>> {
    // TODO: Validate Command status and either panic or log useful error
    // message.
    // TODO: This fn should also accept an optional tmux config file to use with `-f`

    // TODO: This is very lazy as it repeatedly shells out to terminal_multiplexer.
    // A better solution would be to introduce something akin to tmuxinator's
    // Doctor class which does basic validation about the environment (tmux, term, etc.)
    // only once at the beginning of the top-level routine.
    if !binary_exists(terminal_multiplexer) {
        panic!(
            "'{}' does not seem to be a compatible terminal multiplexer",
            terminal_multiplexer
        );
    }

    let mut tmux = Command::new(terminal_multiplexer);
    if wait {
        let child = tmux.args(command).spawn()?;
        let output: Output = child.wait_with_output()?;
        Ok(output)
    } else {
        Ok(tmux.args(command).output()?)
    }
}

trait TmuxCommandRunner {
    fn run_tmux_command(
        &self,
        terminal_multiplexer: &String,
        command: &[String],
        wait: bool,
    ) -> Result<Output, Box<dyn Error>>;
}

struct TmuxWrapper;

impl TmuxCommandRunner for TmuxWrapper {
    fn run_tmux_command(
        &self,
        terminal_multiplexer: &String,
        command: &[String],
        wait: bool,
    ) -> Result<Output, Box<dyn Error>> {
        run_tmux_command(terminal_multiplexer, command, wait)
    }
}

// The following exists to drive a whole project setup as a single, ordered
// conversation with one persistent `tmux -C` (control mode) process, instead
// of spawning a brand new tmux process per command (see run_tmux_command
// above, still used for the one-shot base-index query and the final
// interactive attach, neither of which need to be part of this
// conversation). This removes the race between "N independent tmux
// invocations" and "a session that may not exist/be fully built yet".
//
// Control mode's wire format (tmux(1), CONTROL MODE section): a client
// writes one tmux command per line to the process' stdin; each command
// produces one output block on stdout, delimited by `%begin <ts> <cmd-num>
// <flags>` and a matching `%end ...` (success) or `%error ...` (failure)
// carrying the same <cmd-num>. Asynchronous notifications (%output,
// %window-add, etc.) can be interleaved between blocks (but never inside
// one) once the session is live, so a reader has to tolerate and discard
// them while waiting for the block it's after.

enum ReaderEvent {
    Line(String),
    Eof,
    Err(String),
}

// Runs on a dedicated thread for the lifetime of the child process, so reads
// and writes never contend on the same thread -- without this, a pane that
// prints output during setup could wedge a naive write-then-read loop.
fn spawn_line_reader<R: std::io::Read + Send + 'static>(reader: R) -> Receiver<ReaderEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf_reader = BufReader::new(reader);
        loop {
            let mut line = String::new();
            match buf_reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(ReaderEvent::Eof);
                    break;
                }
                Ok(_) => {
                    if line.ends_with('\n') {
                        line.pop();
                        if line.ends_with('\r') {
                            line.pop();
                        }
                    }
                    if tx.send(ReaderEvent::Line(line)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(ReaderEvent::Err(error.to_string()));
                    break;
                }
            }
        }
    });
    rx
}

// Wraps every argv element in single quotes so it round-trips through tmux's
// own command-line parser as exactly one literal token, regardless of
// content (spaces, quotes, `;`, `#`, `$`, backticks, leading `~`). Single
// quotes are used specifically because -- unlike double quotes -- tmux never
// performs $VAR or leading-~ expansion inside them; a pane command like
// `echo $HOME` must reach the pane's shell unmolested, not get expanded by
// tmux itself first.
fn quote_control_mode_token(s: &str) -> String {
    let mut quoted = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            // Close the quote, emit an escaped literal quote, reopen it.
            quoted.push_str("'\\''");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

fn quote_control_mode_line(argv: &[String]) -> String {
    argv.iter()
        .map(|token| quote_control_mode_token(token))
        .collect::<Vec<String>>()
        .join(" ")
}

// Parses a `%begin`/`%end`/`%error <ts> <cmd-number> <flags>` line, returning
// the keyword and command number. Any other line (block content, or an
// async notification like `%output`) returns None.
fn parse_control_mode_marker(line: &str) -> Option<(&str, i64)> {
    let mut parts = line.splitn(4, ' ');
    let keyword = parts.next()?;
    if keyword != "%begin" && keyword != "%end" && keyword != "%error" {
        return None;
    }
    let _timestamp = parts.next()?;
    let command_number = parts.next()?.parse::<i64>().ok()?;
    Some((keyword, command_number))
}

#[derive(Debug, PartialEq)]
struct ControlModeResponse {
    ok: bool,
    output: Vec<String>,
}

trait ControlModeRunner {
    // Spawns `tmux <tmux_option_tokens> -C <initial_command>` and blocks
    // until that command's own response block resolves. `initial_command`
    // must result in this client being attached to a session (e.g.
    // `new-session` without `-d`), or tmux will run it and exit rather than
    // staying resident.
    fn open(
        &mut self,
        terminal_multiplexer: &str,
        tmux_option_tokens: &[String],
        initial_command: &[String],
    ) -> Result<ControlModeResponse, Box<dyn Error>>;

    // Quotes `argv` into one control-mode line, writes it, and blocks until
    // that command's response block resolves.
    fn send(&mut self, argv: &[String]) -> Result<ControlModeResponse, Box<dyn Error>>;

    // Detaches this control client (leaving the underlying tmux session,
    // which the server owns independently, running) and waits for the child
    // process to exit. Idempotent -- a no-op if the session isn't open.
    fn close(&mut self) -> Result<(), Box<dyn Error>>;
}

struct ControlModeSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout_rx: Option<Receiver<ReaderEvent>>,
    stderr_rx: Option<Receiver<ReaderEvent>>,
}

impl ControlModeSession {
    fn new() -> Self {
        ControlModeSession {
            child: None,
            stdin: None,
            stdout_rx: None,
            stderr_rx: None,
        }
    }

    fn response_timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    fn stderr_snapshot(&self) -> String {
        let mut lines = Vec::new();
        if let Some(rx) = &self.stderr_rx {
            while let Ok(ReaderEvent::Line(line)) = rx.try_recv() {
                lines.push(line);
            }
        }
        lines.join("\n")
    }

    // Waits for the next `%begin`, discarding any async notifications seen
    // first, then reads that block through to its matching `%end`/`%error`.
    fn read_response(&mut self) -> Result<ControlModeResponse, Box<dyn Error>> {
        loop {
            match self.next_stdout_event()? {
                ReaderEvent::Line(line) => {
                    if let Some(("%begin", command_number)) = parse_control_mode_marker(&line) {
                        return self.read_block(command_number);
                    }
                    // An async notification (%output, %window-add, etc.)
                    // that arrived between blocks -- discard and keep
                    // waiting.
                }
                _ => unreachable!("next_stdout_event only returns Line or an Err"),
            }
        }
    }

    fn read_block(&mut self, begin_command_number: i64) -> Result<ControlModeResponse, Box<dyn Error>> {
        let mut output = Vec::new();
        loop {
            match self.next_stdout_event()? {
                ReaderEvent::Line(line) => {
                    if let Some((keyword, command_number)) = parse_control_mode_marker(&line) {
                        if (keyword == "%end" || keyword == "%error")
                            && command_number == begin_command_number
                        {
                            return Ok(ControlModeResponse {
                                ok: keyword == "%end",
                                output,
                            });
                        }
                    }
                    output.push(line);
                }
                _ => unreachable!("next_stdout_event only returns Line or an Err"),
            }
        }
    }

    fn next_stdout_event(&mut self) -> Result<ReaderEvent, Box<dyn Error>> {
        let timeout = self.response_timeout();
        let rx = self
            .stdout_rx
            .as_ref()
            .ok_or("tmux control mode session is not open")?;
        match rx.recv_timeout(timeout) {
            Ok(ReaderEvent::Line(line)) => Ok(ReaderEvent::Line(line)),
            Ok(ReaderEvent::Eof) => Err(format!(
                "tmux control mode connection closed unexpectedly.{}",
                match self.stderr_snapshot().as_str() {
                    "" => String::new(),
                    stderr => format!(" stderr: {}", stderr),
                }
            )
            .into()),
            Ok(ReaderEvent::Err(error)) => {
                Err(format!("error reading from tmux control mode connection: {}", error).into())
            }
            Err(_) => Err("timed out waiting for a response from tmux control mode".into()),
        }
    }
}

impl ControlModeRunner for ControlModeSession {
    fn open(
        &mut self,
        terminal_multiplexer: &str,
        tmux_option_tokens: &[String],
        initial_command: &[String],
    ) -> Result<ControlModeResponse, Box<dyn Error>> {
        let mut child = Command::new(terminal_multiplexer)
            .args(tmux_option_tokens)
            .arg("-C")
            .args(initial_command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child.stdin.take().ok_or("unable to open tmux's stdin")?;
        let stdout = child.stdout.take().ok_or("unable to open tmux's stdout")?;
        let stderr = child.stderr.take().ok_or("unable to open tmux's stderr")?;

        self.stdout_rx = Some(spawn_line_reader(stdout));
        self.stderr_rx = Some(spawn_line_reader(stderr));
        self.stdin = Some(stdin);
        self.child = Some(child);

        self.read_response()
    }

    fn send(&mut self, argv: &[String]) -> Result<ControlModeResponse, Box<dyn Error>> {
        let line = quote_control_mode_line(argv);
        let stdin = self
            .stdin
            .as_mut()
            .ok_or("tmux control mode session is not open")?;
        // NOTE: ChildStdin is an unbuffered pipe handle -- writes go
        // straight to the OS pipe. If this is ever wrapped in a BufWriter,
        // every write must be followed by an explicit flush() or commands
        // will sit in userspace while tmux waits on stdin forever.
        writeln!(stdin, "{}", line)?;
        stdin.flush()?;
        self.read_response()
    }

    fn close(&mut self) -> Result<(), Box<dyn Error>> {
        if self.stdin.is_none() {
            return Ok(());
        }

        // Best-effort: detach this control client. Sessions are independent,
        // server-owned objects, so this leaves the session itself running.
        let _ = self.send(&[String::from("detach-client")]);

        self.stdin = None;
        self.stdout_rx = None;
        self.stderr_rx = None;

        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }

        Ok(())
    }
}

impl Drop for ControlModeSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn build_pane_args(session_name: &str, window_index: usize) -> Vec<Vec<String>> {
    // NOTE: Like tmuxinator does, we want to continually apply the tiled layout
    // after splitting and then _only_ at the very end of the window config
    // constructor context apply the specified or inherited layout. This
    // prevents errors like, "no space for new pane" which occur if the
    // window's current size/layout won't (otherwise) allow it to be split
    // again.
    // We should document that tiled _is_ used by default when splitting panes
    // and, that if no other layouts are specified, it will result in tiled
    // output.
    // See:
    // - https://github.com/ethagnawl/rmuxinator/issues/45
    // - https://web.archive.org/web/20250709171739/https://www.mail-archive.com/tmux-users@googlegroups.com/msg01241.html
    vec![
        vec![
            String::from("split-window"),
            String::from("-t"),
            format!("{}:{}", session_name, window_index.to_string()),
        ],
        vec![
            String::from("select-layout"),
            String::from("-t"),
            format!("{}:{}", session_name, window_index.to_string()),
            String::from("tiled"),
        ],
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

trait EnvProvider {
    fn get_var(&self, key: &str) -> Option<String>;
}

struct SystemEnv;
impl EnvProvider for SystemEnv {
    fn get_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

fn in_tmux_context(env: &dyn EnvProvider) -> bool {
    env.get_var("TMUX")
        // NOTE: This is naive but if a value is present and non-sensical
        // (e.g.  999), then all bets are off, anyways.
        .map(|val| !val.is_empty())
        .unwrap_or(false)
}

fn build_attach_command_args(session_name: &str) -> Vec<String> {
    let session_op = if in_tmux_context(&SystemEnv) {
        String::from("switch-client")
    } else {
        String::from("attach-session")
    };
    vec![
        String::from("-u"),
        String::from(session_op),
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

fn build_commands_with_tmux_options_prefix(
    tmux_options: String,
    commands: Vec<(Vec<String>, bool)>,
) -> Vec<(Vec<String>, bool)> {
    // NOTE: If the full string (e.g. -f /tmp/custom.conf) is used it results
    // in an error. There's something up with the whitespace or similar which
    // results in the flag being consumed and the shell trying to execute the
    // path to the config file.
    // TODO: Ideally we'd do _some_ amount of validation of these arguments.
    // Right now, anything and everything is being passed through and can/will
    // cause tmux errors.
    let tmux_options_parts: Vec<&str> = tmux_options.split(" ").collect();
    let tmux_option_strs: Vec<String> = tmux_options_parts
        .clone()
        .into_iter()
        .map(|x| x.to_string())
        .collect();
    commands
        .into_iter()
        .map(|(nested_vec, bool)| {
            // (["new-session", "-d", "-s", "new-sesh", "-n", "one"], false)
            // becomes
            // (["-f", "/tmp/tmux.custom.conf", "new-session", "-d", "-s", "new-sesh", "-n", "one"], false)
            let mut command = tmux_option_strs.clone();
            command.extend(nested_vec.iter().cloned());
            (command, bool)
        })
        .collect()
}

fn convert_config_to_tmux_commands(
    config: &Config,
    base_indices: TmuxBaseIndices,
) -> Vec<(Vec<String>, bool)> {
    // TODO: We should consider adding sensible line endings
    // to clearly delineate command boundaries.
    let mut commands = vec![];

    let session_name = &config.name;

    let session_start_directory = build_session_start_directory(&config);

    let first_window = if let Some(window) = config.windows.first() {
        window.name.clone()
    } else {
        None
    };

    let create_session_args =
        build_session_args(session_name, first_window, &session_start_directory);
    commands.push((create_session_args, false));

    for hook in config.hooks.iter() {
        let hook_command = build_hook_args(&hook);
        commands.push((hook_command, false));
    }

    for (window_iterator_index, window) in config.windows.iter().enumerate() {
        // The "first" window is created by create_session because tmux always
        // creates a window when creating a session.
        // The alternative would be to create all of the project windows and
        // then kill the first/default one, but I saw unexpected behavior
        // (first window's commands not being run) when attempting that -- I
        // think it's because the indexes get shuffled.
        // The alternative approach would be more explicit and preferable, so
        // maybe it's worth revisiting.
        let window_index = base_indices.base_index + window_iterator_index;
        if window_iterator_index > 0 {
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

            commands.push((create_window_args, false));
        }

        for (pane_iterator_index, pane) in window.panes.iter().enumerate() {
            let pane_index = base_indices.pane_base_index + pane_iterator_index;
            // The "first" pane is created by default by the containing window
            if pane_iterator_index > 0 {
                let pane_args = build_pane_args(session_name, window_index);
                for pane_arg in pane_args {
                    commands.push((pane_arg, false));
                }
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
                commands.push((pane_command_args, false));
            }

            for (_, command) in pane.commands.iter().enumerate() {
                let pane_command_args =
                    build_pane_command_args(session_name, &window_index, &pane_index, command);
                commands.push((pane_command_args, false));
            }

            let rename_pane_args = build_rename_pane_args(
                session_name,
                window_index,
                pane_index,
                &config.pane_name_user_option,
                &pane.name.clone(),
            );
            if let Some(rename_pane_args_) = rename_pane_args {
                commands.push((rename_pane_args_, false));
            }
        }

        let window_layout_args =
            build_window_layout_args(session_name, &window_index, &config.layout, &window.layout);

        if let Some(window_layout_args_) = window_layout_args {
            commands.push((window_layout_args_, false));
        }
    }

    // TODO: It's not ideal that logic for constructing this command lives in
    // multiple places (i.e. here and in build_attach_command_args)
    // I think ideally this would be a _tell, don't ask_ situation and we
    // either validate that the output is valid (i.e. via Option) and let the
    // function figure out whether or how to compute the command. This is
    // probably also something we should do for all of these helper functions.
    if config.attached {
        let attach_args = build_attach_command_args(&config.name);
        commands.push((attach_args, true));
    }

    if let Some(tmux_options) = config.tmux_options.clone() {
        commands = build_commands_with_tmux_options_prefix(tmux_options, commands);
    }

    commands
}

#[derive(Debug, PartialEq)]
struct TmuxBaseIndices {
    base_index: usize,
    pane_base_index: usize,
}

fn get_tmux_base_indices(
    config: &Config,
    tmux_command_runner: &dyn TmuxCommandRunner,
) -> TmuxBaseIndices {
    // `args` will result in the following command:
    // `tmux start-server\; show-option -g base-index\; show-window-option -g pane-base-index`

    let mut commands = vec![(
        vec![
            "start-server".to_string(),
            ";".to_string(),
            "show-option".to_string(),
            "-g".to_string(),
            "base-index".to_string(),
            ";".to_string(),
            "show-window-option".to_string(),
            "-g".to_string(),
            "pane-base-index".to_string(),
        ],
        false,
    )];

    if let Some(tmux_options) = config.tmux_options.clone() {
        commands = build_commands_with_tmux_options_prefix(tmux_options, commands);
    }

    let output = tmux_command_runner.run_tmux_command(
        &config.terminal_multiplexer,
        &commands[0].0,
        commands[0].1,
    );
    let pane_base_index_re = Regex::new(r"(?:base-index (?P<base_index>\d+))?(?:.*\n)?(?:pane-base-index (?P<pane_base_index>\d+))?").unwrap();

    // NOTE: This is a bit redundant but feels _better_ than using Option
    // values and then immediately setting them to Some(N).
    let mut base_index = 0;
    let mut pane_base_index = 0;

    if let Some(captures) =
        pane_base_index_re.captures(&String::from_utf8(output.unwrap().stdout).unwrap())
    {
        base_index = captures
            .name("base_index")
            .map_or("0", |m| m.as_str())
            .parse::<usize>()
            .unwrap();

        pane_base_index = captures
            .name("pane_base_index")
            .map_or("0", |m| m.as_str())
            .parse::<usize>()
            .unwrap();
    }

    let tmux_base_indices = TmuxBaseIndices {
        base_index: base_index,
        pane_base_index: pane_base_index,
    };

    tmux_base_indices
}

// `new-window`/`split-window` each spawn a new shell in a new pane. Typing
// into that pane immediately (as convert_config_to_tmux_commands' pane
// commands do) can win a race against the shell's own startup (loading rc
// files, etc.): the keystrokes get caught by the kernel's tty-cooked-mode
// echo before the shell is reading its input, then get replayed once it is,
// which looks like garbled, duplicated pane output. The previous
// per-command-process transport masked this by accident (process spawn
// overhead gave the shell time to start); sending everything over one
// already-open connection is fast enough to reliably lose the race.
//
// A fixed delay here is a guess, and an unreliable one -- it either wastes
// time on fast-starting shells or still loses the race against slow ones.
// Instead, poll the pane's content (which is blank until its shell starts
// writing to it) until it stops changing, which adapts to however long that
// particular shell actually takes, up to a safety cap so a pathological
// case can't hang the whole run.
const PANE_SETTLE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const PANE_SETTLE_QUIET_PERIOD: Duration = Duration::from_millis(40);
const PANE_SETTLE_MAX_WAIT: Duration = Duration::from_secs(3);

fn wait_for_pane_to_settle(control_mode_runner: &mut dyn ControlModeRunner, target: &str) {
    let deadline = Instant::now() + PANE_SETTLE_MAX_WAIT;
    let mut last_content: Option<Vec<String>> = None;
    let mut unchanged_since: Option<Instant> = None;

    loop {
        let response = match control_mode_runner.send(&[
            "capture-pane".to_string(),
            "-p".to_string(),
            "-t".to_string(),
            target.to_string(),
        ]) {
            // capture-pane itself failing (a transport error, or tmux
            // rejecting it, e.g. because the target doesn't exist) means
            // there's nothing more to learn by retrying -- give up
            // immediately rather than spinning until PANE_SETTLE_MAX_WAIT.
            Err(_) => return,
            Ok(response) if !response.ok => return,
            Ok(response) => response,
        };

        let has_content = response.output.iter().any(|line| !line.trim().is_empty());
        if has_content {
            if last_content.as_ref() == Some(&response.output) {
                if unchanged_since.map_or(false, |since| since.elapsed() >= PANE_SETTLE_QUIET_PERIOD) {
                    return;
                }
                if unchanged_since.is_none() {
                    unchanged_since = Some(Instant::now());
                }
            } else {
                last_content = Some(response.output);
                unchanged_since = None;
            }
        }

        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(PANE_SETTLE_POLL_INTERVAL);
    }
}

// Extracts the `-t <target>` value from a `new-window`/`split-window`
// command built by build_create_window_args/build_pane_args, both of which
// always emit it as their second and third tokens.
fn pane_creation_target(argv: &[String]) -> Option<&str> {
    match argv.first().map(String::as_str) {
        Some("new-window") | Some("split-window") => argv.get(2).map(String::as_str),
        _ => None,
    }
}

// Separately from the pane-creation race above: a shell's line editor
// (readline, zle, ...) restores the pty to cooked/canonical mode while a
// command runs and only re-enables its own raw-mode input handling right
// before reading the next line. Two `send-keys` sent back-to-back with
// near-zero gap -- which a persistent control-mode connection can easily
// do, unlike the old per-command-process transport -- can land the second
// one inside that brief window, with the same garbled-echo symptom as the
// pane-creation race. A poll-until-stable check is overkill for this (the
// window is small and roughly constant, not open-ended like shell
// startup); empirically a 10ms gap was already enough to reliably clear it,
// so this uses a more conservative (but still small) fixed pause instead.
const SEND_KEYS_SETTLE_DURATION: Duration = Duration::from_millis(20);

fn run_start_(
    config: Config,
    tmux_command_runner: &dyn TmuxCommandRunner,
    control_mode_runner: &mut dyn ControlModeRunner,
) -> Result<(), Box<dyn Error>> {
    // Base indices are a read-only, session-independent query, so it's run
    // as a one-shot process ahead of the control-mode connection rather than
    // over it -- there's no atomicity to gain by including it.
    let base_indices = get_tmux_base_indices(&config, tmux_command_runner);
    let mut commands = convert_config_to_tmux_commands(&config, base_indices).into_iter();

    // `tmux_options` (e.g. `-f`/`-L`) is prefixed onto every command by
    // convert_config_to_tmux_commands for run_debug_'s benefit. For control
    // mode it's only valid as a real CLI flag on the process spawn, so it's
    // stripped back off of each command here and passed to `open` once.
    let tmux_option_token_count = config
        .tmux_options
        .as_ref()
        .map_or(0, |tmux_options| tmux_options.split(' ').count());

    let (create_session_args, _) = commands
        .next()
        .expect("convert_config_to_tmux_commands always returns a session-creation command");
    let tmux_option_tokens = create_session_args[..tmux_option_token_count].to_vec();
    let mut initial_command = create_session_args[tmux_option_token_count..].to_vec();
    // `-d` (always at this position -- see build_session_args) suppresses
    // attachment, but the control-mode client needs to attach in order to
    // stay resident for the rest of the setup sequence.
    initial_command.remove(1);

    control_mode_runner.open(&config.terminal_multiplexer, &tmux_option_tokens, &initial_command)?;
    // The initial command always creates the session's first pane, spawning
    // a new shell in it. Settling is deferred until (if) we're about to type
    // into it -- see the loop below -- rather than done eagerly here, so a
    // pane nothing ever gets sent to (no start_directory, no commands)
    // doesn't pay for a wait it doesn't need.
    let mut pending_settle_target = Some(config.name.clone());

    let mut attach_command = None;
    let mut commands = commands.peekable();
    while let Some((argv, wait)) = commands.next() {
        if wait {
            // The final interactive attach, if any, is always the last
            // command and has to remain a normal process spawn with
            // inherited stdio -- a control-mode client's stdin/stdout are
            // pipes we created, not the user's real terminal, so there's no
            // way to hand this connection off to become it in place.
            attach_command = Some(argv);
            continue;
        }

        let interior = &argv[tmux_option_token_count..];
        let is_send_keys = interior.first().map(String::as_str) == Some("send-keys");

        // A freshly created pane only needs settling right before we
        // actually type into it -- see wait_for_pane_to_settle for why.
        if is_send_keys {
            if let Some(target) = pending_settle_target.take() {
                wait_for_pane_to_settle(control_mode_runner, &target);
            }
        }

        let response = control_mode_runner.send(interior)?;
        if !response.ok {
            let _ = control_mode_runner.close();
            return Err(format!(
                "tmux command failed: {}\n{}",
                interior.join(" "),
                response.output.join("\n")
            )
            .into());
        }

        if let Some(target) = pane_creation_target(interior) {
            pending_settle_target = Some(target.to_string());
        } else if is_send_keys {
            // Only pace if the next command is another send-keys -- pane
            // commands are always contiguous (see convert_config_to_tmux_
            // commands), so if it isn't, nothing is left racing this pane
            // before we move on to something else.
            let next_is_send_keys = commands.peek().map_or(false, |(next_argv, next_wait)| {
                !next_wait
                    && next_argv.get(tmux_option_token_count).map(String::as_str)
                        == Some("send-keys")
            });
            if next_is_send_keys {
                thread::sleep(SEND_KEYS_SETTLE_DURATION);
            }
        }
    }

    control_mode_runner.close()?;

    if let Some(attach_args) = attach_command {
        tmux_command_runner.run_tmux_command(&config.terminal_multiplexer, &attach_args, true)?;
    }

    Ok(())
}

pub fn run_start(config: Config) -> Result<(), Box<dyn Error>> {
    // NOTE: This exists to prevent the public API from having to change in
    // order pass in an optional TmuxCommandRunner (e.g. `None` in the
    // application/non-test environment). As noted above this indirection
    // exists soley to facilitate mocking run_tmux_command in the test env.
    // This is the best approach I've hit upon yet but I'm still not convinced
    // it's a good, worthwhile idea.
    // - ethagnawl
    run_start_(config, &TmuxWrapper, &mut ControlModeSession::new())
}

fn run_debug_(
    config: Config,
    tmux_command_runner: &dyn TmuxCommandRunner,
) -> Result<(), Box<dyn Error>> {
    let base_indices = get_tmux_base_indices(&config, tmux_command_runner);
    for command in convert_config_to_tmux_commands(&config, base_indices) {
        println!("{} {}", &config.terminal_multiplexer, command.0.join(" "));
    }

    Ok(())
}

pub fn run_debug(config: Config) -> Result<(), Box<dyn Error>> {
    // See run_start docstring for TmuxWrapper rationale.
    // - ethagnawl
    run_debug_(config, &TmuxWrapper)
}

pub fn parse_args<I, T>(args: I) -> CliArgs
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let project_config_file_arg = Arg::with_name("PROJECT_CONFIG_FILE")
        .help("The path to the project config file")
        .required(true);
    let app_matches = App::new(clap::crate_name!())
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .about(clap::crate_description!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("debug")
            .about("Print the tmux commands that would be used to start and configure a tmux session using a path to a project config file")
            .arg(&project_config_file_arg)
        )
        .subcommand(
            SubCommand::with_name("start")
            .about("Start a tmux session using a path to a project config file")
            .arg(&project_config_file_arg)
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
            panic!("{}", error.to_string());
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
    // TODO: `project_name` is currently overloaded and also used as the config
    // path. We should either make this more explicit or introduce separate
    // args.
    pub project_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", untagged)]
pub enum Layout {
    // NOTE: This does not attempt to do _any_ validation on the provided
    // string. I'm not even sure if that's possible? We could attept to do
    // basic shape validation using a regex or something but it seems like
    // overkill.
    Custom(String),
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let layout_name = match self {
            Layout::Custom(value) => value.clone(),
            _ => {
                let pascal_case_layout_name = format!("{:?}", self);
                convert_pascal_case_to_kebab_case(&pascal_case_layout_name)
            }
        };
        write!(f, "{}", layout_name)
    }
}

type StartDirectory = Option<String>;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Pane {
    pub commands: Vec<String>,
    pub name: Option<String>,
    pub start_directory: StartDirectory,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Window {
    pub layout: Option<Layout>,
    pub name: Option<String>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    pub start_directory: StartDirectory,
}

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
pub struct Hook {
    command: String,
    name: HookName,
}

#[derive(Derivative, Debug, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "ConfigDefaultValues::default_as_tmux")]
    pub terminal_multiplexer: String,
    // TODO: add base_index w/ default?
    #[serde(default = "ConfigDefaultValues::default_as_true")]
    pub attached: bool,
    pub pane_name_user_option: Option<String>,
    #[serde(default)]
    pub hooks: Vec<Hook>,
    pub layout: Option<Layout>,
    pub name: String,
    pub start_directory: StartDirectory,
    #[derivative(Default(value = "None"))]
    pub tmux_options: Option<String>,
    #[serde(default)]
    pub windows: Vec<Window>,
}

impl Config {
    pub fn new_from_file_path(config_path: &String) -> Result<Config, String> {
        // Need to return String in failure case because toml::from_str may
        // return a toml::de::Error.
        let mut config_file = match File::open(&config_path) {
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

struct ConfigDefaultValues;
impl ConfigDefaultValues {
    pub fn default_as_tmux() -> String {
        "tmux".to_string()
    }
    pub fn default_as_true() -> bool {
        true
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
    use mockall::mock;
    use mockall::predicate::*;
    use std::collections::HashMap;

    struct MockEnv(HashMap<String, String>);

    impl EnvProvider for MockEnv {
        fn get_var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    fn create_dummy_output_instance(status: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> Output {
        // NOTE: There's no simple way to create an arbitrary ExitStatus
        // instance, so we actually have to shell out. We could mock ExitStatus
        // but that would require introducing a trait of our own which wraps
        // ExitStatus and is used throughout the program, which seems like
        // overkill.
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("exit {}", status))
            .status()
            .expect("failed to execute process");
        Output {
            status,
            stdout,
            stderr,
        }
    }

    mock! {
        TmuxCommandRunner {}
        impl TmuxCommandRunner for TmuxCommandRunner {
            fn run_tmux_command(&self, terminal_multiplexer: &String, command: &[String], wait: bool) -> Result<Output, Box<dyn Error>>;
        }
    }

    fn create_ok_control_mode_response() -> ControlModeResponse {
        ControlModeResponse {
            ok: true,
            output: vec![],
        }
    }

    fn create_failed_control_mode_response() -> ControlModeResponse {
        ControlModeResponse {
            ok: false,
            output: vec![],
        }
    }

    // run_start_ always calls wait_for_pane_to_settle at least once (right
    // after opening the session). It polls via `send`, so a failed response
    // makes it give up immediately instead of spinning, keeping these tests
    // fast and deterministic -- their point is to exercise run_start_'s
    // command sequencing, not the settle-check itself.
    fn stub_pane_settle_checks(control_mode_runner: &mut MockControlModeRunner) {
        control_mode_runner
            .expect_send()
            .withf(|argv: &[String]| argv.first().map(String::as_str) == Some("capture-pane"))
            .returning(|_| Ok(create_failed_control_mode_response()));
    }

    mock! {
        ControlModeRunner {}
        impl ControlModeRunner for ControlModeRunner {
            fn open(&mut self, terminal_multiplexer: &str, tmux_option_tokens: &[String], initial_command: &[String]) -> Result<ControlModeResponse, Box<dyn Error>>;
            fn send(&mut self, argv: &[String]) -> Result<ControlModeResponse, Box<dyn Error>>;
            fn close(&mut self) -> Result<(), Box<dyn Error>>;
        }
    }

    #[test]
    fn test_in_tmux_context_returns_true_when_env_var_is_non_empty_string() {
        let env = MockEnv(HashMap::from([(
            String::from("TMUX"),
            String::from("/tmp/tmux-1000/default,12345,0"),
        )]));
        assert_eq!(true, in_tmux_context(&env));
    }

    #[test]
    fn test_in_tmux_context_returns_false_when_env_var_is_empty_string() {
        let env = MockEnv(HashMap::from([(String::from("TMUX"), String::from(""))]));
        assert_eq!(false, in_tmux_context(&env));
    }

    #[test]
    fn test_in_tmux_context_returns_false_when_env_var_not_set() {
        let env = MockEnv(HashMap::new());
        assert_eq!(false, in_tmux_context(&env))
    }

    #[test]
    fn test_it_returns_default_base_index_when_no_value_found_in_tmux_session() {
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .times(1)
            .with(always(), always(), eq(false))
            .returning(|_x, _y, _z| {
                Ok(create_dummy_output_instance(
                    0,
                    "nope".bytes().collect(),
                    vec![],
                ))
            });
        let config = Config::default();
        let indices = get_tmux_base_indices(&config, &tmux_command_runner);
        let expected = 0;
        let actual = indices.base_index;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_it_returns_default_pane_base_index_when_no_value_found_in_tmux_session() {
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .times(1)
            .with(always(), always(), eq(false))
            .returning(|_x, _y, _z| {
                Ok(create_dummy_output_instance(
                    0,
                    "nope".bytes().collect(),
                    vec![],
                ))
            });
        let config = Config::default();
        let indices = get_tmux_base_indices(&config, &tmux_command_runner);
        let expected = 0;
        let actual = indices.pane_base_index;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_it_returns_default_base_index_when_good_value_found_in_tmux_session() {
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .times(1)
            .with(always(), always(), eq(false))
            .returning(|_x, _y, _z| {
                Ok(create_dummy_output_instance(
                    0,
                    "base-index 0".bytes().collect(),
                    vec![],
                ))
            });

        let config = Config::default();
        let indices = get_tmux_base_indices(&config, &tmux_command_runner);
        let expected = 0;
        let actual = indices.base_index;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_it_returns_default_pane_base_index_when_good_value_found_in_tmux_session() {
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .times(1)
            .with(always(), always(), eq(false))
            .returning(|_x, _y, _z| {
                Ok(create_dummy_output_instance(
                    0,
                    "pane-base-index 0".bytes().collect(),
                    vec![],
                ))
            });
        let config = Config::default();
        let indices = get_tmux_base_indices(&config, &tmux_command_runner);
        let expected = 0;
        let actual = indices.base_index;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_it_returns_custom_base_index_when_good_value_found_in_tmux_session() {
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .times(1)
            .with(always(), always(), eq(false))
            .returning(|_x, _y, _z| {
                Ok(create_dummy_output_instance(
                    0,
                    "base-index 99".bytes().collect(),
                    vec![],
                ))
            });
        let config = Config::default();
        let indices = get_tmux_base_indices(&config, &tmux_command_runner);
        let expected = 99;
        let actual = indices.base_index;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_it_returns_custom_pane_base_index_when_good_value_found_in_tmux_session() {
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], bool| {
                *bool == false
                    && *command
                        == vec![
                            "start-server".to_string(),
                            ";".to_string(),
                            "show-option".to_string(),
                            "-g".to_string(),
                            "base-index".to_string(),
                            ";".to_string(),
                            "show-window-option".to_string(),
                            "-g".to_string(),
                            "pane-base-index".to_string(),
                        ]
            })
            .returning(|_x, _y, _z| {
                Ok(create_dummy_output_instance(
                    0,
                    "pane-base-index 99".bytes().collect(),
                    vec![],
                ))
            });
        let config = Config::default();
        let indices = get_tmux_base_indices(&config, &tmux_command_runner);
        let expected = 99;
        let actual = indices.pane_base_index;
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_it_passes_tmux_options_to_tmux_when_present() {
        let tmux_options = "-f another-one.conf".to_string();
        let config = Config {
            attached: false,
            name: "foo".to_string(),
            tmux_options: Some(tmux_options.clone()),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
            ..Config::default()
        };

        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], _| {
                *command
                    == vec![
                        "-f".to_string(),
                        "another-one.conf".to_string(),
                        "start-server".to_string(),
                        ";".to_string(),
                        "show-option".to_string(),
                        "-g".to_string(),
                        "base-index".to_string(),
                        ";".to_string(),
                        "show-window-option".to_string(),
                        "-g".to_string(),
                        "pane-base-index".to_string(),
                    ]
            })
            .returning(|_x, _y, _z| Ok(create_dummy_output_instance(0, vec![], vec![])));

        let mut control_mode_runner = MockControlModeRunner::new();
        stub_pane_settle_checks(&mut control_mode_runner);
        control_mode_runner
            .expect_open()
            .once()
            .withf(
                |_terminal_multiplexer: &str,
                 tmux_option_tokens: &[String],
                 initial_command: &[String]| {
                    *tmux_option_tokens == vec!["-f".to_string(), "another-one.conf".to_string()]
                        && *initial_command
                            == vec![
                                "new-session".to_string(),
                                "-s".to_string(),
                                "foo".to_string(),
                                "-n".to_string(),
                                "a window".to_string(),
                            ]
                },
            )
            .returning(|_, _, _| Ok(create_ok_control_mode_response()));
        control_mode_runner.expect_close().once().returning(|| Ok(()));

        let _ = run_start_(config, &tmux_command_runner, &mut control_mode_runner);
    }

    #[test]
    fn test_it_passes_multiple_tmux_options_to_tmux_when_present() {
        let tmux_options = "-f another-one.conf -L custom-socket".to_string();
        let config = Config {
            attached: false,
            name: "foo".to_string(),
            tmux_options: Some(tmux_options.clone()),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
            ..Config::default()
        };

        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], _| {
                *command
                    == vec![
                        "-f".to_string(),
                        "another-one.conf".to_string(),
                        "-L".to_string(),
                        "custom-socket".to_string(),
                        "start-server".to_string(),
                        ";".to_string(),
                        "show-option".to_string(),
                        "-g".to_string(),
                        "base-index".to_string(),
                        ";".to_string(),
                        "show-window-option".to_string(),
                        "-g".to_string(),
                        "pane-base-index".to_string(),
                    ]
            })
            .returning(|_x, _y, _z| Ok(create_dummy_output_instance(0, vec![], vec![])));

        let mut control_mode_runner = MockControlModeRunner::new();
        stub_pane_settle_checks(&mut control_mode_runner);
        control_mode_runner
            .expect_open()
            .once()
            .withf(
                |_terminal_multiplexer: &str,
                 tmux_option_tokens: &[String],
                 initial_command: &[String]| {
                    *tmux_option_tokens
                        == vec![
                            "-f".to_string(),
                            "another-one.conf".to_string(),
                            "-L".to_string(),
                            "custom-socket".to_string(),
                        ]
                        && *initial_command
                            == vec![
                                "new-session".to_string(),
                                "-s".to_string(),
                                "foo".to_string(),
                                "-n".to_string(),
                                "a window".to_string(),
                            ]
                },
            )
            .returning(|_, _, _| Ok(create_ok_control_mode_response()));
        control_mode_runner.expect_close().once().returning(|| Ok(()));

        let _ = run_start_(config, &tmux_command_runner, &mut control_mode_runner);
    }

    #[test]
    fn test_it_uses_a_custom_terminal_multiplexer_when_provided() {
        let terminal_multiplexer = String::from("tmux-yolo");
        let config = Config {
            terminal_multiplexer: terminal_multiplexer.clone(),
            ..Config::default()
        };

        let mut tmux_command_runner = MockTmuxCommandRunner::new();

        tmux_command_runner
            .expect_run_tmux_command()
            .with(eq(terminal_multiplexer.clone()), always(), always())
            .times(1..)
            .returning(|_, _, _| Ok(create_dummy_output_instance(0, vec![], vec![])));

        let mut control_mode_runner = MockControlModeRunner::new();
        stub_pane_settle_checks(&mut control_mode_runner);
        let expected_terminal_multiplexer = terminal_multiplexer.clone();
        control_mode_runner
            .expect_open()
            .withf(move |terminal_multiplexer: &str, _, _| {
                terminal_multiplexer == expected_terminal_multiplexer
            })
            .times(1..)
            .returning(|_, _, _| Ok(create_ok_control_mode_response()));
        control_mode_runner.expect_close().returning(|| Ok(()));

        let _ = run_start_(config, &tmux_command_runner, &mut control_mode_runner);
    }

    #[test]
    #[should_panic(expected = "'not-tmux' does not seem to be a compatible terminal multiplexer")]
    fn test_run_tmux_command_does_basic_validation_on_terminal_multiplexer() {
        let terminal_multiplexer = String::from("not-tmux");
        let command = vec![];
        let wait = false;
        let _ = run_tmux_command(&terminal_multiplexer, &command, wait);
    }

    #[test]
    fn test_it_doesnt_pass_tmux_options_to_tmux_when_absent() {
        let config = Config {
            attached: false,
            name: "foo".to_string(),
            tmux_options: None,
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
            ..Config::default()
        };

        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], _| {
                *command
                    == vec![
                        "start-server".to_string(),
                        ";".to_string(),
                        "show-option".to_string(),
                        "-g".to_string(),
                        "base-index".to_string(),
                        ";".to_string(),
                        "show-window-option".to_string(),
                        "-g".to_string(),
                        "pane-base-index".to_string(),
                    ]
            })
            .returning(|_x, _y, _z| Ok(create_dummy_output_instance(0, vec![], vec![])));

        let mut control_mode_runner = MockControlModeRunner::new();
        stub_pane_settle_checks(&mut control_mode_runner);
        control_mode_runner
            .expect_open()
            .once()
            .withf(
                |_terminal_multiplexer: &str,
                 tmux_option_tokens: &[String],
                 initial_command: &[String]| {
                    *tmux_option_tokens == Vec::<String>::new()
                        && *initial_command
                            == vec![
                                "new-session".to_string(),
                                "-s".to_string(),
                                "foo".to_string(),
                                "-n".to_string(),
                                "a window".to_string(),
                            ]
                },
            )
            .returning(|_, _, _| Ok(create_ok_control_mode_response()));
        control_mode_runner.expect_close().once().returning(|| Ok(()));

        let _ = run_start_(config, &tmux_command_runner, &mut control_mode_runner);
    }

    #[test]
    fn test_run_tmux_command_does_not_receive_an_attach_command_when_attached_false() {
        let config = Config {
            attached: false,
            name: "foo".to_string(),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
            ..Config::default()
        };
        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], _| {
                *command
                    == vec![
                        "start-server".to_string(),
                        ";".to_string(),
                        "show-option".to_string(),
                        "-g".to_string(),
                        "base-index".to_string(),
                        ";".to_string(),
                        "show-window-option".to_string(),
                        "-g".to_string(),
                        "pane-base-index".to_string(),
                    ]
            })
            .returning(|_x, _y, _z| Ok(create_dummy_output_instance(0, vec![], vec![])));

        // No further tmux_command_runner expectations are registered, so an
        // attach call (which would go through run_tmux_command with
        // wait=true) here would panic as unexpected.
        let mut control_mode_runner = MockControlModeRunner::new();
        stub_pane_settle_checks(&mut control_mode_runner);
        control_mode_runner
            .expect_open()
            .once()
            .returning(|_, _, _| Ok(create_ok_control_mode_response()));
        control_mode_runner.expect_close().once().returning(|| Ok(()));

        let _ = run_start_(config, &tmux_command_runner, &mut control_mode_runner);
    }

    #[test]
    fn test_run_tmux_command_does_receive_an_attach_command_when_attached_true() {
        let config = Config {
            attached: true,
            name: "foo".to_string(),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
            ..Config::default()
        };

        let mut tmux_command_runner = MockTmuxCommandRunner::new();
        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], bool| {
                *bool == false
                    && *command
                        == vec![
                            "start-server".to_string(),
                            ";".to_string(),
                            "show-option".to_string(),
                            "-g".to_string(),
                            "base-index".to_string(),
                            ";".to_string(),
                            "show-window-option".to_string(),
                            "-g".to_string(),
                            "pane-base-index".to_string(),
                        ]
            })
            .returning(|_x, _y, _z| Ok(create_dummy_output_instance(0, vec![], vec![])));

        tmux_command_runner
            .expect_run_tmux_command()
            .once()
            .withf(|_terminal_multiplexer: &String, command: &[String], bool| {
                *bool == true && *command == vec!["-u", "attach-session", "-t", "foo"]
            })
            .returning(|_x, _y, _z| Ok(create_dummy_output_instance(0, vec![], vec![])));

        let mut control_mode_runner = MockControlModeRunner::new();
        stub_pane_settle_checks(&mut control_mode_runner);
        control_mode_runner
            .expect_open()
            .once()
            .returning(|_, _, _| Ok(create_ok_control_mode_response()));
        control_mode_runner.expect_close().once().returning(|| Ok(()));

        let _ = run_start_(config, &tmux_command_runner, &mut control_mode_runner);
    }

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
            config_layout.clone().unwrap().to_string(),
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
            window_layout.clone().unwrap().to_string(),
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
            window_layout.clone().unwrap().to_string(),
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
        let actual = build_attach_command_args(&session_name);
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
    fn it_converts_custom_layout_to_string() {
        let custom_layout = String::from(
            "9959,213x59,0,0[213x24,0,0,4,213x2,0,25,5,213x31,0,28{166x31,0,28,6,46x31,167,28,7}]",
        );
        let layout = Layout::Custom(custom_layout.clone());
        let expected = custom_layout;
        let actual = layout.to_string();
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_no_start_directory_when_none_present_for_session_start_directory() {
        let config = Config {
            name: String::from("foo"),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: None,
            }],
            ..Config::default()
        };

        let actual = build_session_start_directory(&config);
        assert!(actual.is_none());
    }

    #[test]
    fn it_uses_configs_start_directory_when_no_window_start_directory_present_for_session_start_directory(
    ) {
        let config = Config {
            name: String::from("foo"),
            start_directory: Some(String::from("/foo/bar")),
            ..Config::default()
        };
        let expected = Some(String::from("/foo/bar"));
        let actual = build_session_start_directory(&config);
        assert_eq!(expected, actual);
    }

    #[test]
    fn it_uses_windows_start_directory_over_configs_start_directory_for_session_start_directory() {
        let config = Config {
            name: String::from("foo"),
            start_directory: Some(String::from("/this/is/ignored")),
            windows: vec![Window {
                layout: None,
                name: Some(String::from("a window")),
                panes: Vec::new(),
                start_directory: Some(String::from("/bar/baz")),
            }],

            ..Config::default()
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
            name: String::from("most basic config"),
            ..Config::default()
        };

        let expected = vec![(
            vec![
                String::from("new-session"),
                String::from("-d"),
                String::from("-s"),
                String::from("most basic config"),
            ],
            false,
        )];
        let base_indices = TmuxBaseIndices {
            base_index: 0,
            pane_base_index: 0,
        };
        let actual = convert_config_to_tmux_commands(&config, base_indices);
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
