# rmuxinator

## What is it?
This project aims to be a clone of tmuxinator, which allows users to define
tmux project profiles (e.g. open two windows, split each into three panes and
run specific commands in each), but it is written in Rust and will be _much_
more performant, portable and simpler to install.

## Status
This project is currently just a proof of concept. It's only capable of parsing
a project config file, starting a tmux session, creating windows and running
commands in those windows.
