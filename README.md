# rmuxinator

## What is this?
This project aims to be a clone of [tmuxinator](https://github.com/tmuxinator/tmuxinator), which allows users to define
tmux project profiles (e.g. open two windows, split each into three panes and
run specific commands in each). It is written in Rust and will be more
performant, dependable and simpler to install. It's also a great excuse for
me to learn more about Rust, its ecosystem and distributing binaries for
various platforms.

## How does it work?
- install tmux (preferably >= 3.0a), rust and cargo
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
- validate project name presence before referencing
- hooks must be optional
- Do we need custom hooks, like tmuxinator uses for pre_window, project_start,
etc.? I was hoping to leverage tmux's hooks and save the trouble, but the
mapping is not 1:1 and users could have to result to hacks like having hooks
remove themselves in order to prevent duplicate events.
- remove/replace Debugs
- add after command hooks (e.g. after-split-window) need to find/create
comprehensive list
- CliArgs.project_name should change to reflect that it's a file path
- consider presenting list of valid cli commands if constructor fails
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

## Platforms
Here are the platforms rmuxinator is known to work on:
- x86_64 GNU/Linux
- armv6l GNU/Linux (I was able to successfully cross-compile from debian x86_64
=> armv6l using the arm-linux-gnueabihf linker provided in the
raspberrypi/tools repository. The Debian package did not work; I was able to
compile successfully, but the program segfaulted immediately when executed.)

## Resources
- https://github.com/raspberrypi/tools
- https://old.reddit.com/r/rust/comments/9io0z8/run_crosscompiled_code_on_rpi_0/
- https://medium.com/@wizofe/cross-compiling-rust-for-arm-e-g-raspberry-pi-using-any-os-11711ebfc52b
- https://devel.tech/tips/n/tMuXz2lj/the-power-of-tmux-hooks/
