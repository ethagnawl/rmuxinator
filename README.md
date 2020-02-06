# rmuxinator

## What is this?
This project aims to be a clone of [tmuxinator](https://github.com/tmuxinator/tmuxinator), which allows users to define
tmux project profiles (e.g. open two windows, split each into three panes and
run specific commands in each). It is written in Rust and will be _much_
more performant, portable and simpler to install. It's also a great excuse for
me to learn more about Rust, its ecosystem and compiling and distributing
binaries for a number of platforms.

## How does it work?
- install rust and cargo
- build and run with: `cargo build && ./target/debug/rmuxinator start Foo.toml`

## Status
This project is currently just a proof of concept and I'll be duplicating
features as I can find time. Right now, it's capable of:
- parsing a TOML project config file
- starting a named tmux session
- setting the default cwd
- creating windows
- setting cwd for windows
- creating panes
- setting cwd for panes
- running pane commands

Still TODO:
- window layout helper
- integration tests which verify compound/derived values (e.g. start_directory)
- integration tests which verify calls to tmux?
- handle shell failures -- `tmux kill-window` was failing silently
- set pane name using `tmux set-option -g 'pane-border-format' foo`
- separate tmux arg construction and shell calls. the args can all be moved
into structs and computed up front.
- better handling of parse errors (prettier error messages)
- hooks
- project layout
- select window on attach
