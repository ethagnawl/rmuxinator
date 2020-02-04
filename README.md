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
- creating windows
- creating panes

Still TODO:
- handle shell failures -- `tmux kill-window` was failing silently
- set default cwd for project when creating the session (this is how tmuxinator
works)
- set default cwd for window
- run pane commands
- set cwd for pane
- set pane layout
- set pane name
- separate tmux arg construction and shell calls. the args can all be moved
into structs and computed up front
