use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

// TODO:
// - Test success scenarios
// - Figure out how to mock tmux or use optional env var in main when testing
// for its presence.

#[test]
fn it_returns_the_expected_debug_output() -> Result<(), Box<dyn std::error::Error>> {
    // NOTE: Use known/good pane/base-index via a temporary tmux config file
    // because host system's tmux config can cause this test to fail if
    // non-default values are used.
    // Also NOTE: If a tmux server using non-standard values is already
    // running, it may need to be killed in order for these values to be
    // applied.

    let mut temp_tmux_config = NamedTempFile::new()?;
    let file_contents = r#"
set -g base-index 0
setw -g pane-base-index 0
    "#;
    writeln!(temp_tmux_config, "{}", file_contents)?;

    let temp_tmux_config_file_flag = format!(
        "-f /tmp/{}",
        temp_tmux_config
            .path()
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap()
    );

    let mut config_file = NamedTempFile::new()?;
    let file_contents = format!(
        r#"
name = "debug"
tmux_options = "{}"
[[windows]]
  name = "one"
[[windows]]
  name = "two"
        "#,
        temp_tmux_config_file_flag
    );
    writeln!(config_file, "{}", file_contents)?;

    let expected = vec![
        format!(
            "tmux {} new-session -d -s debug -n one",
            temp_tmux_config_file_flag
        ),
        format!(
            "tmux {} new-window -t debug:1 -n two",
            temp_tmux_config_file_flag
        ),
    ]
    .join("\n");
    Command::cargo_bin(env!("CARGO_PKG_NAME"))?
        .arg("debug")
        .arg(config_file.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(expected));

    Ok(())
}

#[test]
fn no_args() -> Result<(), Box<dyn std::error::Error>> {
    let long_help = format!(
        r#"{} {}
{}
{}

USAGE:
    {} <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    debug    Print the tmux commands that would be used to start and configure a tmux session using a path to a
             project config file
    help     Prints this message or the help of the given subcommand(s)
    start    Start a tmux session using a path to a project config file"#,
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        env!("CARGO_PKG_DESCRIPTION"),
        env!("CARGO_PKG_NAME"),
    );
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains(long_help));

    Ok(())
}

#[test]
fn bad_command() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

    let bad_command_help = format!(
        r#"error: Found argument 'bork' which wasn't expected, or isn't valid in this context

USAGE:
    {} <SUBCOMMAND>

For more information try --help"#,
        env!("CARGO_PKG_NAME"),
    );

    cmd.arg("bork").arg("Example.toml");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains(bad_command_help));

    Ok(())
}

#[test]
fn missing_project() -> Result<(), Box<dyn std::error::Error>> {
    let bad_arg_help = format!(
        r#"error: The following required arguments were not provided:
    <PROJECT_CONFIG_FILE>

USAGE:
    {} start <PROJECT_CONFIG_FILE>

For more information try --help"#,
        env!("CARGO_PKG_NAME")
    );
    Command::cargo_bin(env!("CARGO_PKG_NAME"))?
        .arg("start")
        .assert()
        .failure()
        .stderr(predicate::str::contains(bad_arg_help));

    Ok(())
}

// TODO: This fails with: "open terminal failed: not a terminal"
// #[test]
// fn project_config_file_exists() -> Result<(), Box<dyn std::error::Error>>
// {
//     let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

//     cmd.arg("start").arg("Example.toml");
//     cmd.assert().failure().stderr(predicate::str::contains(
//         "Problem parsing config file: Unable to open config file.",
//     ));

//     Ok(())
// }

#[test]
fn project_config_file_doesnt_exist() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;

    cmd.arg("start").arg("DontExist.toml");
    cmd.assert().failure().stderr(predicate::str::contains(
        "Problem parsing config file: Unable to open config file.",
    ));

    Ok(())
}

#[test]
fn invalid_toml() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = NamedTempFile::new()?;
    writeln!(file, "Toml ain't Yaml")?;

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;
    cmd.arg("start").arg(file.path());
    cmd.assert().failure().stderr(predicate::str::contains(
        "Problem parsing config file: expected an equals, found an identifier at line 1",
    ));

    Ok(())
}

#[test]
fn invalid_project_toml() -> Result<(), Box<dyn std::error::Error>> {
    // This single example is not comprehensive, but is validation that the
    // program will exit hard and fast if there are missing required fields or
    // similar.
    let mut file = NamedTempFile::new()?;
    writeln!(
        file,
        "xname = \"this won't work because 'name' is required\""
    )?;

    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME"))?;
    cmd.arg("start").arg(file.path());
    cmd.assert().failure().stderr(predicate::str::contains(
        "Problem parsing config file: missing field `name`",
    ));

    Ok(())
}
