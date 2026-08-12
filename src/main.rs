use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use hopr::db::Database;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Search terms or command (e.g., `list` or `config tmux`)
    #[arg(required = true)]
    keywords: Vec<String>,
}

fn open_in_nvim_unix(selected: String, dir: Option<&Path>) {
    if let Some(d) = dir {
        let _ = Command::new("nvim").arg(selected).current_dir(d).exec();
    } else {
        let _ = Command::new("nvim").arg(selected).exec();
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let home = std::env::var("HOME").context("while attempting to read HOME environment variable")?;
    let history = format!("{}/.hopr_history", home);
    let mut database = Database::new(history.into());
    let mut database = match database.load() {
        Ok(v) => v,
        Err(_) => database,
    };

    if args.keywords.len() == 1 && args.keywords[0] == "list" {
        println!("{}", database);
        return Ok(());
    }

    let selected = database.query(&args.keywords);

    let (to_open, dir) = match selected {
        Ok(v) => (v.path.to_string_lossy().to_string(), v.path.parent().map(|p| p.to_path_buf())),
        Err(_) => {
            let combined = args.keywords.join("/");
            let path = std::path::PathBuf::from(&combined);
            let parent = path.parent().map(|p| p.to_path_buf());
            (combined, parent)
        }
    };

    open_in_nvim_unix(to_open, dir.as_deref());
    Ok(())
}

