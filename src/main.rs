extern crate rmuxinator;

use rmuxinator::{parse_args, run_debug, run_start, test_for_tmux, CliCommand, Config};
use std::env;

fn main() -> Result<(), String> {
    let tmux_exists = test_for_tmux("tmux");

    if !tmux_exists {
        return Err(String::from(
            "Unable to find tmux. Is it installed and available on $PATH?",
        ));
    }

    let cli_args = parse_args(env::args_os());

    let config = Config::new_from_file_path(&cli_args.project_name)
        .map_err(|error| format!("Problem parsing config file: {}", error))?;

    match cli_args.command {
        CliCommand::Start => {
            run_start(config).map_err(|error| format!("Application error: {}", error))
        }
        CliCommand::Debug => {
            run_debug(config).map_err(|error| format!("Application error: {}", error))
        }
    }
}
