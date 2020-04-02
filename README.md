# rmuxinator

## What is this?
This project aims to be a clone of [tmuxinator](https://github.com/tmuxinator/tmuxinator), which allows users to define
tmux project profiles (e.g. open two windows, split each into three panes and run specific commands in each). It is written in Rust and will be more dependable (config is typechecked where possible) and simpler to install. It's also a great excuse for me to learn more about Rust, its ecosystem and compiling/distributing binaries for various platforms.

## How does it work?
- install tmux (preferably >= 3.0a), rust and cargo
- build and run with: `cargo build && ./target/debug/rmuxinator start Example.toml`

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
- `hooks` (array; see dedicated entry)
- `layout` (string; preset tmux layouts: "even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled")
- `pane_name_user_option` (string; must have matching entry in .tmux.conf (e.g.  `set -g pane-border-format "#{@custom_pane_title}"`)
- `start_directory` (string)

##### Hooks
- `command` (string; must use tmux's `run_shell`; see tmux docs)
- `name` (string; must match existing tmux hook (e.g. `after-select-pane`); see tmux docs)

##### Windows
- `name` (string)
- `panes` (array; see dedicated entry)

###### Optional
- `layout` (string; preset tmux layouts: "even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled")
- `start_directory` (string)

##### Panes
- `commands` (array of strings)

###### Optional
- `name` (string)
- `start_directory` (string)

## Status
This project is currently a proof of concept and I'll be duplicating tmuxinator
features (and adding additional improvements) as I can find time. Right now, it's
capable of:
- parsing a TOML project config file
- starting a named tmux session
- setting a default layout for project windows
- setting the default cwd
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
- make pub fn the exception in lib constructs
- consider building up and executing a single script (a la tmuxinator) instead
of shelling out many times
- make window name optional
- support custom layouts?
- break lib into components files (Config, CliArgs, etc.)
- Do we need custom hooks, like tmuxinator uses for pre_window, project_start,
etc.? I was hoping to leverage tmux's hooks and save the trouble, but the
mapping is not 1:1 and users could have to result to hacks like having hooks
remove themselves in order to prevent duplicate events.
- remove/replace Debugs
- CliArgs.project_name should change to reflect that it's a file path
- consider presenting list of valid cli commands if constructor fails
- looks like format doesn't consume values, so refs aren't (always?) necessary
- use feature detection to conditionally apply/opt out of certain features
(user options)
- integration tests which verify compound/derived values (e.g. start_directory)
- integration tests which verify calls to tmux?
- handle shell failures -- `tmux kill-window` was failing silently
- Can commands can all be moved into structs and computed up front? This might
require writing a custom Serde deserializer for the Config type.
- select window on attach (can this be handled by a pre-existing hook?)
- attach if session exists instead of creating sesssion
- search for project name instead of parsing config (I'm not convinced this is
necessary)
- other CLI commands? (create, edit, stop, delete, etc.)
- use named args in calls to format! where possible
- document config options and provide sample
- cut v0.0.1 release and publish binaries

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
