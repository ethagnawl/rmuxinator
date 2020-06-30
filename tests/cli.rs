use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::env;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

// TODO:
// - Test success scenarios
// - Figure out how to mock tmux or use optional env var in main when testing
// for its presence.

#[test]
fn it_returns_the_expected_debug_output() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = NamedTempFile::new()?;
    writeln!(file, "name = \"debug project\"")?;

    Command::cargo_bin(env!("CARGO_PKG_NAME"))?
        .arg("debug")
        .arg(file.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("new-session -d -s debug project"));

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
    debug    Review the commands that would be used to start a tmux session using a path to a project config file
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
