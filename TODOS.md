# TODOS

1. [Pico build file-system implementation](TODOS#file-system)
2. [Command line interface](TODOS#cli)

## file-system

- [ ] Decide on 'tab-logic'; the current implementation (based on o.g.) just creates a folder for each tab.
  - [ ] Figure out if 15 is max tabs in editor-only, or if the actual runtime supports more in a file (in this case we are fine)

This is a big question-mark with the entire app.

The runtime might actually not care (or track how many) tabs there are all in all.
There are many ideas I have:
- We could enforce some directory structure on initialization, perhaps?
  - A `pico-build new` command for example. Fits very well with the `cli` and usage of `clap`. Also with refactor of ratatui code, adding a new command and action is not that painful.
  - Two main paths to consider here: files vs. subfolders
    - We could just have files with a name corresponding to each tab,
    - We could have subfolders with any files chosen by the user, and then flatten them into the output (I much prefer this idea. It would also greatly simplify converting existing code/projects)

## cli

- [ ] Figure out a solution for logging-panel
  - [x] Intercept messages from the `tracing` crate, to use them in ratatui cli-code
  - [ ] Find out how to display them on screen (currently having weird issues, might need to force redraw)

### commands
- [ ] New command: sets up cart-project, dependant on solution to [file system](TODOS#file-system) question
  - Alternatively could do more options for user?
- [ ] Info command: print information about loaded cart project
