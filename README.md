# hopr

A fast, frecency-based file hopper for Neovim inspired by [zoxide](https://github.com/ajeetdsouza/zoxide).

`hopr` lets you jump directly into files in `nvim` by typing partial keywords or paths, tracking your most frequent and recent files over time.

## Features

- **Frecency Tracking:** Automatically tracks accessed files and scores them based on frequency and recency in `~/.hopr_history`.
- **Sequential Multi-Keyword Matching:** Match files using multiple sequential terms (e.g. `hopr config tmux` matches `~/.config/tmux/tmux.conf` or `hopr ABC README` matches `.../ABC/README.md`).
- **Direct Path Priority:** Resolves relative or absolute paths immediately if they exist on disk (e.g. `hopr src main.rs`).
- **Binary File Exclusion:** Automatically excludes non-text binary files from history matches.
- **Automatic Context Directory:** Opens `nvim` with its working directory set to the target file's parent folder.

## Installation

Ensure you have [Rust and Cargo](https://www.rust-lang.org/) installed:

```sh
cargo install --path .
```

## Usage

### Open a file

Jump to a file using keywords:

```sh
# Open tmux.conf from history or matching path
hopr config tmux

# Open a specific file in a subdirectory
hopr ABC README

# Open a direct path
hopr src main.rs
```

### List tracked files

View tracked files and their current frecency scores:

```sh
hopr list
```

## How It Works

1. **Direct Path Resolution:** If the keywords form an existing relative or absolute file path on disk (e.g. `src/main.rs`), it opens that file immediately and records it to history.
2. **Frecency History Lookup:** If no direct path matches, `hopr` queries your global history (`~/.hopr_history`) for non-binary files matching all keywords in sequence, selecting the entry with the highest frecency score.
3. **Fallback:** If no match is found, `hopr` passes the combined path to `nvim` so you can create or edit the file directly.
