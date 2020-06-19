use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn no_args() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("rmuxinator")?;

    cmd.assert().failure().stderr(predicate::str::contains(
        "Problem parsing CLI arguments: Command is required.",
    ));

    Ok(())
}

#[test]
fn project_config_file_doesnt_exist() -> Result<(), Box<dyn std::error::Error>>
{
    let mut cmd = Command::cargo_bin("rmuxinator")?;

    cmd.arg("start").arg("DontExist.toml");
    cmd.assert().failure().stderr(predicate::str::contains(
        "Problem parsing config file: Unable to open config file.",
    ));

    Ok(())
}
