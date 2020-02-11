# rmuxinator

## What is this?
This project aims to be a clone of [tmuxinator](https://github.com/tmuxinator/tmuxinator), which allows users to define
tmux project profiles (e.g. open two windows, split each into three panes and
run specific commands in each). It is written in Rust and will be more
performant, dependable and simpler to install. It's also a great excuse for
me to learn more about Rust, its ecosystem and distributing binaries for
various platforms.

## How does it work?
- install tmux (>= 3.0a), rust and cargo
- build and run with: `cargo build && ./target/debug/rmuxinator start Foo.toml`

## Status
This project is currently a proof of concept and I'll be duplicating tmuxinator
features (and some additional improvements) as I can find time. Right now, it's
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
- wiring up tmux event hooks/callbacks

## Still TODO:
- add after command hooks (e.g. after-split-window) need to find/create
comprehensive list
- CliArgs.project_name should change to reflect that it's a file path
- use enum for CliArgs.command
- move rename_pane_args into helper
- looks like format doesn't consume values, so refs aren't (always?) necessary
- consider creating layout type alias Option<Layout>
- use run_tmux_command for layout -- need handle conditional
- use feature detection to conditionally apply/opt out of certain features
(user options)
- integration tests which verify compound/derived values (e.g. start_directory)
- integration tests which verify calls to tmux?
- handle shell failures -- `tmux kill-window` was failing silently
- Can commands can all be moved into structs and computed up front? This might
require writing a custom Serde deserializer for the Config type.
- select window on attach (can this be handled by a pre-existing hook?)
- attach if session exists instead of creating sesssion
- search for project name instead of parsing config (i'm not convinced this is
necessary)
- other CLI commands? (create, edit, stop, delete, etc.)
- use named args in calls to format!
