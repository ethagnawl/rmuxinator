# rmuxinator

<p align="center">
  <img src="https://raw.githubusercontent.com/ethagnawl/rmuxinator/master/rmuxinator-screenshot.png" alt="Screenshot" width="80%" />
</p>

## What is this?
This project aims to be a successor to [tmuxinator](https://github.com/tmuxinator/tmuxinator), which allows users to
define tmux project profiles (e.g. open two windows, split each into three
panes and run a series of commands in each). It is written in Rust and will be
more dependable (config is typechecked where possible) and simpler to install.
It's also a great excuse for me to learn more about Rust, its ecosystem and
compiling/distributing binaries for various platforms.

## TLDR; How do I use it?
- install tmux (>= 2.8), [rust and cargo](https://rustup.rs/)

### Cargo
- install: `cargo install rmuxinator`
- run: `rmuxinator start Example.toml`

### Source
#### `cargo build`
- build: `cargo build && ./target/debug/rmuxinator start Example.toml`
- run: `./target/debug/rmuxinator start Example.toml`
#### `cargo run`
- run: `cargo run start Example.toml`

## Documentation

### Project Config
Projects are defined using toml.

For example:
```
layout = "main-horizontal"
name = "example"
pane_name_user_option = "custom_pane_title"
start_directory = "/home/peter/projects/vim"

[[hooks]]
  command = "run-shell \"tmux display-message 'Hi from pane-focus-in hook!'\""
  name = "pane-focus-in"

[[windows]]
  layout = "tiled"
  name = "one"
  start_directory = "/home/peter/projects/sample-project"

  [[windows.panes]]
  commands = ["echo pane-one"]
  name = "Work"

  [[windows.panes]]
  commands = ["echo pane-two"]
  name = "Music"
  start_directory = "/home/peter/projects/rmuxinator/src"

  [[windows.panes]]
  commands = ["echo pane-three"]
  name = "RSS"

  [[windows.panes]]
  commands = ["echo hi one", "echo intermediate one", "echo bye one"]

[[windows]]
  name = "two"
  start_directory = "/home/peter/projects/sample-project"

  [[windows.panes]]
  commands = ["echo pane-one"]

  [[windows.panes]]
  commands = ["echo pane-two"]
  start_directory = "/home/peter/projects/rmuxinator/src"

  [[windows.panes]]
  commands = ["echo pane-three"]

  [[windows.panes]]
  commands = ["echo hi one", "echo intermediate one", "echo bye one"]
```
#### Configuration Options
Optional attributes will be noted below.

##### Project
- `name` (string)
- `windows` (array; see dedicated entry)

###### Optional
-  attached (bool; whether or not to attach to newly created tmux session)
- `hooks` (array; see dedicated entry)
- `layout` (string; preset tmux layouts: "even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled")
- `pane_name_user_option` (string; must have matching entry in .tmux.conf (e.g.  `set -g pane-border-format "#{@custom_pane_title}"`)
- `start_directory` (string)

##### Hooks
- `command` (string; must use tmux's `run_shell`; see tmux docs)
- `name` (string; must match existing tmux hook (e.g. `after-select-pane`); see tmux docs)

##### Windows
- `panes` (array; see dedicated entry)

###### Optional
- `layout` (string; preset tmux layouts: "even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled")
- `name` (string)
- `start_directory` (string)

##### Panes
- `commands` (array of strings)

###### Optional
- `name` (string)
- `start_directory` (string)

### Commands
#### `debug`
Print the tmux commands that would be used to start and configure a tmux
session using a path to a project config file:
`rmuxinator debug Example.toml`

#### `start`
Start a tmux session using a path to a project config file:
`rmuxinator start Example.toml`

### Use as a library
rmuxinator can also be used as a library by other programs.

There are two ways to achieve this:

#### Config::new_from_config_path
This option accepts a path to an rmuxinator config file and is how the rmuxinator binary works. This is how this project's binary entrypoint works.

Example:

```
let config = rmuxinator::Config::new_from_config_path(&String::from("/home/pi/foo.toml")).map_err(|error| format!("Problem parsing config file: {}", error))?;
rmuxinator::run_start(config).map_err(|error| format!("Application error: {}", error));
```

#### Config constructor
This option allows the caller to create an rmuxinator `Config` struct and then pass it to the `run_start` function.

The [pi-wall-utils](https://github.com/ethagnawl/pi-wall-utils) project (also maintained by [ethagnawl](https://github.com/ethagnawl)) does this and can be used as a reference.


Example:

```
let rmuxinator_config = rmuxinator::Config {
    hooks: vec![],
    layout: None,
    name: String::from("rmuxinator-library-example"),
    windows: vec![
        rmuxinator::Window {
            layout: None,
            name: None,
            panes: vec![rmuxinator::Pane {
                commands: vec![
                    String::from("echo 'hello!'"),
                ],
                name: None,
                start_directory: None,
            }],
            start_directory: None,
    }
    ];

};
rmuxinator::run_start(rmuxinator_config).map_err(|error| format!("Rmuxinator error: {}", error))
```

## Known Issues and Workarounds
### Indexes
rmuxinator currently assumes that both `base-index` and `pane-base-index` are
0. This will be handled programatically in a future release, but for the time
being, it can be worked around by either removing overrides from .tmux.conf or
by adding the following:
```
set -g base-index 0
set -g pane-base-index 0
```

## Status
This project is currently a proof of concept and I'll be duplicating tmuxinator
features and adding additional improvements as I can find time. Right now, it's
capable of:
- parsing a TOML project config file
- starting a named tmux session
- setting a default layout for project windows
- setting the default working directory
- creating windows
- setting cwd for windows
- setting window layout
- creating panes
- setting cwd for panes
- setting a pane title using a "user option" (requires >= tmux 3.0a and related
pane-border-format config option)
- running pane commands
- wiring up optional tmux event hooks/callbacks

## Still TODO:
- Consider building up and executing a single script (a la tmuxinator) instead
of shelling out many times
- Support custom layouts?
- Break lib into component files (Config, CliArgs, etc.)
- Do we need custom hooks, like tmuxinator uses for pre_window, project_start,
etc.? I was hoping to leverage tmux's hooks and save the trouble, but the
mapping is not 1:1 and users could have to result to hacks like having hooks
remove themselves in order to prevent duplicate events.
- CliArgs.project_name should change to reflect that it's a file path
- Looks like format doesn't consume values, so refs aren't (always?) necessary
- Use feature detection to conditionally apply/opt out of certain features
(user options)
- Integration tests which verify compound/derived values (e.g. start_directory)
- Integration tests which verify calls to tmux?
- Handle shell failures -- `tmux kill-window` was failing silently
- Can commands can all be moved into structs and computed up front? This might
require writing a custom Serde deserializer for the Config type.
- Select window on attach (can this be handled by a pre-existing hook?)
- Attach if session exists instead of creating sesssion
- Search for project config file on disk (XDG_CONFIG?) instead of parsing
config (I'm not convinced this is necessary)
- Other CLI commands? (stop session, create/edit/delete project)
- Use named args in calls to format! where possible

## Platforms
Here are the platforms rmuxinator is known to work on:
- x86_64 GNU/Linux
- x86_64 GNU/Linux (Windows Subshell)
- armv6l GNU/Linux (RPi Zero; I was able to successfully cross-compile from
Debian x86_64 => armv6l using the arm-linux-gnueabihf linker provided in the
raspberrypi/tools repository. The Debian package did not work; I was able to
compile successfully, but the program segfaulted immediately when executed.)

## Resources
- https://github.com/raspberrypi/tools
- https://old.reddit.com/r/rust/comments/9io0z8/run_crosscompiled_code_on_rpi_0/
- https://medium.com/@wizofe/cross-compiling-rust-for-arm-e-g-raspberry-pi-using-any-os-11711ebfc52b
- https://devel.tech/tips/n/tMuXz2lj/the-power-of-tmux-hooks/
- https://rust-cli.github.io/book/tutorial/testing.html
