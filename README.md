# hopr

[![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Neovim](https://img.shields.io/badge/Neovim-0.8%2B-green?style=flat-square&logo=neovim&logoColor=white)](https://neovim.io)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg?style=flat-square)](https://opensource.org/licenses/MIT)
[![Tests](https://img.shields.io/badge/tests-passing-brightgreen?style=flat-square)](https://github.com/shadowmkj/hopr)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-blue?style=flat-square)](https://github.com/shadowmkj/hopr)

> **A lightning-fast, frecency-based file hopper for Neovim.**  
> Jump straight into your most relevant files across projects with partial keywords, powered by a [zoxide](https://github.com/ajeetdsouza/zoxide)-inspired frecency engine.


---

## Overview

While tools like `zoxide` or `autojump` make navigating directories effortless, opening specific files across different projects still typically requires typing out long paths, searching with fuzzy finders, or `cd`-ing first.

**hopr** solves this for Neovim users:
- Jump directly into files from anywhere using simple, fuzzy-like sequential keywords (e.g. `hopr config tmux` -> `~/.config/tmux/tmux.conf`).
- Automatically ranks files using **frecency** (a hybrid of frequency of access and recency).
- Seamlessly sets Neovim's working directory to the target file's parent folder, ensuring buffer navigation, relative paths, and language servers work out of the box.
- Operates with near-zero latency by replacing the process via Unix `exec()`.

---

## Features

- ⚡ **Instant Sequential Matching:** Match complex paths using multiple terms in order (e.g., `hopr myapp route` matches `/projects/myapp/src/routes/index.ts`).
- 📈 **Smart Frecency Algorithm:** Tracks file access patterns over time in `~/.hopr_history`. Frequently and recently accessed files rank higher, while older entries gracefully decay.
- 🎯 **Direct Path Priority:** If you specify a path that exists on disk relative to your current directory (e.g. `hopr src main.rs` or `hopr Cargo.toml`), `hopr` resolves it directly without scanning history.
- 🛡️ **Binary File Exclusion:** Automatically filters out non-text binary files (`.png`, `.so`, `.wasm`, `.sqlite`, `.DS_Store`, etc.) from history queries so your editor never opens corrupt binary blobs.
- 📁 **Context-Aware Directory Switching:** Launches `nvim` with the working directory set to the target file's parent folder, making your LSP and file trees immediately relevant.
- 💾 **Compact Binary Storage:** Uses [postcard](https://github.com/jamesmunns/postcard) serialization for microsecond database reads and writes.
- 🚀 **Zero Process Overhead:** Directly invokes `execvp` on Unix to replace the CLI process with `nvim` instead of spawning extra wrapper subshells.

---

## Installation

### Prerequisites
- [Neovim](https://neovim.io/) (`nvim`) installed and available in your `$PATH`.
- [Rust toolchain](https://rust-lang.org/) (Cargo 1.85+ recommended).

### Build from Source

Clone the repository and install the binary using `cargo`:

```sh
git clone https://github.com/shadowmkj/hopr.git
cd hopr
cargo install --path .
```

Make sure Cargo's binary directory (typically `~/.cargo/bin`) is in your system `$PATH`:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

---

## Usage

### 1. Jump to a Tracked File

Run `hopr` with one or more keywords describing the path:

```sh
# Matches ~/.config/tmux/tmux.conf
hopr config tmux

# Matches ~/.dotfiles/fish/config.fish
hopr fish config

# Matches /projects/backend/src/routes/auth.rs
hopr backend auth
```

### 2. Disambiguate Across Projects

When multiple files share identical filenames (like `README.md` or `main.rs`), provide directory keywords to narrow down the target:

```sh
# Opens README.md in the 'core' project directory
hopr core readme

# Opens README.md in the 'frontend' project directory
hopr frontend readme
```

### 3. Open Direct Relative or Absolute Paths

If you supply a path that exists relative to your current working directory, `hopr` opens it immediately and records it in your history database:

```sh
# Opens ./src/main.rs directly
hopr src main.rs

# Opens an absolute path
hopr /etc/hosts
```

### 4. Create or Edit New Files

If keywords do not match any existing history entry and do not form an existing path on disk, `hopr` passes the combined path (`keywords.join("/")`) to `nvim` so you can create a new file on the fly:

```sh
# Opens a new buffer for notes/meeting.md in nvim
hopr notes meeting.md
```

### 5. Inspect Tracked Records (`list`)

To view all tracked files and their current raw frecency scores:

```sh
hopr list
```

Example output:
```text
"/Users/username/.config/tmux/tmux.conf": 128
"/Users/username/projects/api/src/server.rs": 32
"/Users/username/.dotfiles/fish/config.fish": 16
"/Users/username/notes/todo.md": 4
```

---

## How It Works

### The Query Resolution Pipeline

When you run `hopr <keywords...>`, the query passes through a strict two-stage pipeline:

```text
┌─────────────────────────────────────────────────────────────┐
│                      hopr <keywords>                        │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
        ┌─────────────────────────────────────────────┐
        │ Stage 1: Direct Path Check                  │
        │ - Try keywords.join("/") relative to CWD    │
        │ - Try keywords.join(" ") or single path     │
        └──────────────────────┬──────────────────────┘
                               │
               Exists on disk? ├────────────► [ YES ] ──► Open in nvim &
                               │                          Save / Boost Record
                               ▼ [ NO ]
        ┌─────────────────────────────────────────────┐
        │ Stage 2: Frecency History Lookup            │
        │ - Load ~/.hopr_history                      │
        │ - Filter by sequential matches_keywords     │
        │ - Exclude binary extensions                 │
        │ - Rank by frecency score(now)               │
        └──────────────────────┬──────────────────────┘
                               │
             Matching record?  ├────────────► [ YES ] ──► Open in nvim &
                               │                          Double Score
                               ▼ [ NO ]
        ┌─────────────────────────────────────────────┐
        │ Stage 3: Fallback Creation                  │
        │ - Open nvim with combined keyword path      │
        └─────────────────────────────────────────────┘
```

1. **Direct Path Resolution (Stage 1):**  
   Checks if the keywords directly form a valid file on disk (relative to your current working directory or absolute). If found, it opens the file immediately and records it in the database.
2. **Frecency History Match (Stage 2):**  
   Searches all entries in `~/.hopr_history` using sequential keyword matching. Filters out binary extensions and ranks matches using their time-decayed frecency score. The top candidate is selected, its score is doubled, and `hopr` opens it.
3. **Fallback (Stage 3):**  
   If no history record matches and no file exists on disk, `hopr` joins the keywords into a file path and opens `nvim` so you can create the new file.

---

## The Frecency Scoring Model

`hopr` calculates file importance using **frecency** ($Score \times TimeMultiplier$):

$$\text{Effective Score} = \text{Raw Score} \times \text{Recency Multiplier}$$

- **Access Boost:** Each time a file is accessed through `hopr`, its base raw score doubles ($Score \leftarrow Score \times 2.0$) and `last_accessed` timestamp is updated to the current time.
- **Time Decay Multipliers:**
  | Time Since Last Access | Multiplier | Rationale |
  | :--- | :--- | :--- |
  | **< 1 Hour** | $\times 4.0$ | Active working set (highest priority) |
  | **< 1 Day** | $\times 2.0$ | Today's recent files |
  | **< 1 Week** | $\times 0.5$ | Weekly working files |
  | **≥ 1 Week** | $\times 0.25$ | Older historical files |

This ensures active files easily outrank files you haven't opened in months, while still remembering frequently visited files.

---

## Sequential Matching Engine

`hopr` enforces strict left-to-right sequential matching across the entire path string (case-insensitively):

```text
Query: ["ABC", "README"]

Path A: /projects/ABC/docs/README.md   ==> MATCH  ("ABC" appears, then "README")
Path B: /projects/DEF/README.md        ==> NO MATCH (missing "ABC")
Path C: /README/projects/ABC.md        ==> NO MATCH (wrong order: README before ABC)
```

---

## Shell Integration & Tips

Add an alias to your shell configuration (`~/.zshrc`, `~/.bashrc`, or `~/.config/fish/config.fish`) for faster hopping:

```sh
# Bash / Zsh (~/.zshrc or ~/.bashrc)
alias h="hopr"

# Fish (~/.config/fish/config.fish)
alias h="hopr"
```

Now you can quickly jump anywhere in your terminal:

```sh
h tmux
h nvim init
h fish
```

---

## Project Structure

```text
hopr/
├── Cargo.toml         # Package metadata and dependencies
├── src/
│   ├── main.rs        # CLI argument parsing, nvim process spawning
│   ├── db.rs          # Database, Record, frecency scoring, and query pipeline
│   ├── utils.rs       # Sequential keyword matching, binary filtering, atomic IO
│   └── lib.rs         # Library module declarations
└── tasks/             # Planning documents and test checklists
```

---

## Running Tests

Run the built-in test suite covering frecency buckets, sequential matching, persistence round-tripping, and edge cases:

```sh
cargo test
```

---

## License

This project is licensed under the [MIT License](LICENSE) (or Apache 2.0 at your option).
