use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

// TODO:
// - Test success scenarios
// - Figure out how to mock tmux or use optional env var in main when testing
// for its presence.

const LONG_HELP: &'static str = r#"rmuxinator 0.1.0
Peter Doherty <pdoherty@protonmail.com>
tmux project configuration CLI utility

USAGE:
    rmuxinator <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    help     Prints this message or the help of the given subcommand(s)
    start    Start a new tmux session"#;

#[test]
fn no_args() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("rmuxinator")?;

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains(LONG_HELP));

    Ok(())
}

#[test]
fn bad_command() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("rmuxinator")?;

    cmd.arg("bork").arg("Example.toml");
    cmd.assert().failure().stderr(predicate::str::contains(
        r#"error: Found argument 'bork' which wasn't expected, or isn't valid in this context

USAGE:
    rmuxinator <SUBCOMMAND>

For more information try --help"#,
    ));

    Ok(())
}

#[test]
fn missing_project() -> Result<(), Box<dyn std::error::Error>> {
    Command::cargo_bin("rmuxinator")?
        .arg("start")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            r#"error: The following required arguments were not provided:
    <PROJECT_FILE>

USAGE:
    rmuxinator start <PROJECT_FILE>

For more information try --help"#,
        ));

    Ok(())
}

// TODO: This fails with: "open terminal failed: not a terminal"
// #[test]
// fn project_config_file_exists() -> Result<(), Box<dyn std::error::Error>>
// {
//     let mut cmd = Command::cargo_bin("rmuxinator")?;

//     cmd.arg("start").arg("Example.toml");
//     cmd.assert().failure().stderr(predicate::str::contains(
//         "Problem parsing config file: Unable to open config file.",
//     ));

//     Ok(())
// }

#[test]
fn project_config_file_doesnt_exist() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("rmuxinator")?;

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

    let mut cmd = Command::cargo_bin("rmuxinator")?;
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

    let mut cmd = Command::cargo_bin("rmuxinator")?;
    cmd.arg("start").arg(file.path());
    cmd.assert().failure().stderr(predicate::str::contains(
        "Problem parsing config file: missing field `name`",
    ));

    Ok(())
}
