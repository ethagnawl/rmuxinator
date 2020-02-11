# rmuxinator

## What is this?
This project aims to be a clone of [tmuxinator](https://github.com/tmuxinator/tmuxinator), which allows users to define
tmux project profiles (e.g. open two windows, split each into three panes and
run specific commands in each). It is written in Rust and will be _much_
more performant, portable and simpler to install. It's also a great excuse for
me to learn more about Rust, its ecosystem and compiling and distributing
binaries for a number of platforms.

## How does it work?
- install tmux (>= 3.0a), rust and cargo
- build and run with: `cargo build && ./target/debug/rmuxinator start Foo.toml`

## Status
This project is currently just a proof of concept and I'll be duplicating
features as I can find time. Right now, it's capable of:
- parsing a TOML project config file
- starting a named tmux session
- setting a default layout for project windows
- setting the default cwd
- creating windows
- setting cwd for windows
- creating panes
- setting cwd for panes
- setting a pane title using a "user option" (requires >= tmux 3.0a and related
pane-border-format)
- running pane commands
- wire up hooks and callbacks

## Still TODO:
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
