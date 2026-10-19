use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use clap::Parser;
use hopr::db::Database;
use hopr::utils;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Search terms or command (e.g., `list` or `config tmux`)
    #[arg(required = true)]
    keywords: Vec<String>,
}

fn get_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nvim".to_string())
}

fn nvim_oldfiles() -> Vec<std::path::PathBuf> {
    let output = match Command::new("nvim")
        .args([
            "--headless",
            "-u",
            "NONE",
            "-c",
            "lua for _, path in ipairs(vim.v.oldfiles) do print(path) end",
            "-c",
            "qa!",
        ])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
        .collect()
}

#[cfg(unix)]
fn open_in_editor(editor: &str, selected: &str, dir: Option<&Path>) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(editor);
    cmd.arg(selected);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let err = cmd.exec();
    Err(anyhow::Error::from(err)
        .context(format!("while attempting to execute editor '{}'", editor)))
}

#[cfg(windows)]
fn open_in_editor(editor: &str, selected: &str, dir: Option<&Path>) -> Result<()> {
    let mut cmd = Command::new(editor);
    cmd.arg(selected);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("while attempting to spawn editor '{}'", editor))?;
    let status = child
        .wait()
        .with_context(|| format!("while attempting to wait for editor '{}'", editor))?;
    if !status.success() {
        anyhow::bail!(
            "editor '{}' exited with non-zero status: {:?}",
            editor,
            status.code()
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let history_path =
        utils::get_history_path().context("while attempting to resolve history database path")?;
    let mut database = Database::new(history_path.clone());
    let mut database = if history_path.exists() {
        database
            .load()
            .context("while attempting to load history database")?
    } else {
        database
    };

    database.import_paths(nvim_oldfiles())?;

    if args.keywords.len() == 1 && args.keywords[0] == "list" {
        println!("{}", database);
        return Ok(());
    }

    let selected = database.query(&args.keywords);

    let (to_open, dir) = match selected {
        Ok(v) => (
            v.path.to_string_lossy().to_string(),
            v.path.parent().map(|p| p.to_path_buf()),
        ),
        Err(_) => {
            let combined = args.keywords.join("/");
            let path = std::path::PathBuf::from(&combined);
            let parent = path.parent().map(|p| p.to_path_buf());
            (combined, parent)
        }
    };

    let editor = get_editor();
    open_in_editor(&editor, &to_open, dir.as_deref())
        .context("while attempting to open target in editor")?;
    Ok(())
}
