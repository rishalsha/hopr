use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use std::{env, mem};

#[cfg(windows)]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};

use crate::db::Epoch;

pub const SECOND: Epoch = 1;
pub const MINUTE: Epoch = 60 * SECOND;
pub const HOUR: Epoch = 60 * MINUTE;
pub const DAY: Epoch = 24 * HOUR;
pub const WEEK: Epoch = 7 * DAY;
pub const MONTH: Epoch = 30 * DAY;

/// Similar to [`fs::write`], but atomic (best effort on Windows).
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<()> {
    let path = path.as_ref();
    let contents = contents.as_ref();
    let dir = path.parent().unwrap();

    // Create a tmpfile.
    let (mut tmp_file, tmp_path) = tmpfile(dir)?;
    let result = (|| {
        // Write to the tmpfile.
        _ = tmp_file.set_len(contents.len() as u64);
        tmp_file
            .write_all(contents)
            .with_context(|| format!("could not write to file: {}", tmp_path.display()))?;

        // Set the owner of the tmpfile (UNIX only).
        #[cfg(unix)]
        if let Ok(metadata) = path.metadata() {
            use std::os::unix::fs::MetadataExt;

            use nix::unistd::{self, Gid, Uid};

            let uid = Uid::from_raw(metadata.uid());
            let gid = Gid::from_raw(metadata.gid());
            _ = unistd::fchown(&tmp_file, Some(uid), Some(gid));
        }

        // Close and rename the tmpfile.
        // In some cases, errors from the last write() are reported only on close().
        // Rust ignores errors from close(), since it occurs inside `Drop`. To
        // catch these errors, we manually call `File::sync_all()` first.
        tmp_file
            .sync_all()
            .with_context(|| format!("could not sync writes to file: {}", tmp_path.display()))?;
        mem::drop(tmp_file);
        rename(&tmp_path, path)
    })();
    // In case of an error, delete the tmpfile.
    if result.is_err() {
        _ = fs::remove_file(&tmp_path);
    }
    result
}

/// Atomically create a tmpfile in the given directory.
fn tmpfile(dir: impl AsRef<Path>) -> Result<(File, PathBuf)> {
    const MAX_ATTEMPTS: usize = 5;
    const TMP_NAME_LEN: usize = 16;
    let dir = dir.as_ref();

    let mut attempts = 0;
    loop {
        attempts += 1;

        // Generate a random name for the tmpfile.
        let mut name = String::with_capacity(TMP_NAME_LEN);
        name.push_str("tmp_");
        while name.len() < TMP_NAME_LEN {
            name.push(fastrand::alphanumeric());
        }
        let path = dir.join(name);

        // Atomically create the tmpfile.
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break Ok((file, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempts < MAX_ATTEMPTS => {}
            Err(e) => {
                break Err(e).with_context(|| format!("could not create file: {}", path.display()));
            }
        }
    }
}

/// Similar to [`fs::rename`], but with retries on Windows.
fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();

    const MAX_ATTEMPTS: usize = if cfg!(windows) { 5 } else { 1 };
    let mut attempts = 0;

    loop {
        match fs::rename(from, to) {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempts < MAX_ATTEMPTS => {
                attempts += 1
            }
            result => {
                break result.with_context(|| {
                    format!(
                        "could not rename file: {} -> {}",
                        from.display(),
                        to.display()
                    )
                });
            }
        }
    }
}

pub fn canonicalize(path: impl AsRef<Path>) -> Result<PathBuf> {
    dunce::canonicalize(&path)
        .with_context(|| format!("could not resolve path: {}", path.as_ref().display()))
}

pub fn current_dir() -> Result<PathBuf> {
    env::current_dir().context("could not get current directory")
}

pub fn current_time() -> Result<Epoch> {
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("system clock set to invalid time")?
        .as_secs();

    Ok(current_time)
}

/// Resolve the user's home directory across Unix and Windows platforms.
#[cfg(unix)]
pub fn home_dir() -> Result<PathBuf> {
    env::var("HOME")
        .map(PathBuf::from)
        .context("while attempting to read HOME environment variable")
}

/// Resolve the user's home directory across Unix and Windows platforms.
#[cfg(windows)]
pub fn home_dir() -> Result<PathBuf> {
    if let Ok(userprofile) = env::var("USERPROFILE") {
        return Ok(PathBuf::from(userprofile));
    }
    if let (Ok(drive), Ok(path)) = (env::var("HOMEDRIVE"), env::var("HOMEPATH")) {
        return Ok(PathBuf::from(format!("{}{}", drive, path)));
    }
    if let Ok(home) = env::var("HOME") {
        return Ok(PathBuf::from(home));
    }
    bail!("could not determine home directory: neither USERPROFILE nor HOMEDRIVE/HOMEPATH is set")
}

/// Returns the default history database file path (~/.hopr_history).
pub fn get_history_path() -> Result<PathBuf> {
    let home = home_dir().context("while attempting to determine user home directory")?;
    Ok(home.join(".hopr_history"))
}

pub fn path_to_str(path: &impl AsRef<Path>) -> Result<&str> {
    let path = path.as_ref();
    path.to_str()
        .with_context(|| format!("invalid unicode in path: {}", path.display()))
}

/// Returns the absolute version of a path. Like
/// [`std::path::Path::canonicalize`], but doesn't resolve symlinks.
pub fn resolve_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let base_path;

    let mut components = path.components().peekable();
    let mut stack = Vec::new();

    // initialize root
    if cfg!(windows) {
        use std::path::Prefix;

        fn get_drive_letter(path: impl AsRef<Path>) -> Option<u8> {
            let path = path.as_ref();
            let mut components = path.components();

            match components.next() {
                Some(Component::Prefix(prefix)) => match prefix.kind() {
                    Prefix::Disk(drive_letter) | Prefix::VerbatimDisk(drive_letter) => {
                        Some(drive_letter)
                    }
                    _ => None,
                },
                _ => None,
            }
        }

        fn get_drive_path(drive_letter: u8) -> PathBuf {
            format!(r"{}:\", drive_letter as char).into()
        }

        fn get_drive_relative(drive_letter: u8) -> Result<PathBuf> {
            let path = current_dir()?;
            if Some(drive_letter) == get_drive_letter(&path) {
                return Ok(path);
            }

            if let Some(path) = env::var_os(format!("={}:", drive_letter as char)) {
                return Ok(path.into());
            }

            let path = get_drive_path(drive_letter);
            Ok(path)
        }

        match components.peek() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::Disk(drive_letter) => {
                    let disk = components.next().unwrap();
                    if components.peek() == Some(&Component::RootDir) {
                        let root = components.next().unwrap();
                        stack.push(disk);
                        stack.push(root);
                    } else {
                        base_path = get_drive_relative(drive_letter)?;
                        stack.extend(base_path.components());
                    }
                }
                Prefix::VerbatimDisk(drive_letter) => {
                    components.next();
                    if components.peek() == Some(&Component::RootDir) {
                        components.next();
                    }

                    base_path = get_drive_path(drive_letter);
                    stack.extend(base_path.components());
                }
                _ => bail!("invalid path: {}", path.display()),
            },
            Some(Component::RootDir) => {
                components.next();

                let current_dir = env::current_dir()?;
                let drive_letter = get_drive_letter(&current_dir).with_context(|| {
                    format!("could not get drive letter: {}", current_dir.display())
                })?;
                base_path = get_drive_path(drive_letter);
                stack.extend(base_path.components());
            }
            _ => {
                base_path = current_dir()?;
                stack.extend(base_path.components());
            }
        }
    } else if components.peek() == Some(&Component::RootDir) {
        let root = components.next().unwrap();
        stack.push(root);
    } else {
        base_path = current_dir()?;
        stack.extend(base_path.components());
    }

    for component in components {
        match component {
            Component::Normal(_) => stack.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                if stack.last() != Some(&Component::RootDir) {
                    stack.pop();
                }
            }
            Component::Prefix(_) | Component::RootDir => unreachable!(),
        }
    }

    Ok(stack.iter().collect())
}

/// Convert a string to lowercase, with a fast path for ASCII strings.
pub fn to_lowercase(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    if s.is_ascii() {
        s.to_ascii_lowercase()
    } else {
        s.to_lowercase()
    }
}

/// Checks whether all query `keywords` appear sequentially within `path` (case-insensitively).
///
/// Analogous to zoxide's path matching logic, each keyword must match a substring of `path`
/// at or after the previous keyword's match location.
pub fn matches_keywords(path: &Path, keywords: &[impl AsRef<str>]) -> bool {
    if keywords.is_empty() {
        return true;
    }

    let path_str = to_lowercase(path.to_string_lossy());
    let mut search_idx = 0;

    for kw in keywords {
        let kw_lower = to_lowercase(kw);
        if kw_lower.is_empty() {
            continue;
        }

        match path_str[search_idx..].find(&kw_lower) {
            Some(rel_idx) => {
                search_idx += rel_idx + kw_lower.len();
            }
            None => return false,
        }
    }

    true
}

const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    ".pnpm",
    ".ignored",
    ".next",
    ".nuxt",
    ".cache",
    "target",
    "dist",
    "build",
    "vendor",
    "tmp",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "bmp", "tiff", "webp", "pdf", "zip", "tar", "gz", "7z",
    "bz2", "xz", "rar", "exe", "dll", "so", "dylib", "a", "o", "pyc", "wasm", "db", "sqlite",
    "bin", "dat", "ds_store",
];

/// Returns true if `path` has a known binary file extension or binary metadata filename.
pub fn is_binary_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = to_lowercase(ext);
        if BINARY_EXTENSIONS.contains(&ext_lower.as_str()) {
            return true;
        }
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if to_lowercase(&file_name) == ".ds_store" {
        return true;
    }
    false
}

/// Recursively search `dir` for files whose paths match `keywords`, returning the best candidate.
///
/// Ignores build/dependency directories and binary files. Candidates are ranked by:
/// 1. Path depth (shallower paths preferred)
/// 2. Exact match of a keyword in the file stem/name
/// 3. Path length
pub fn find_file_in_dir(dir: &Path, keywords: &[impl AsRef<str>]) -> Option<PathBuf> {
    if keywords.is_empty() {
        return None;
    }

    let kw_lowers: Vec<String> = keywords.iter().map(to_lowercase).collect();
    let mut candidates = Vec::new();

    fn walk(
        current: &Path,
        keywords: &[impl AsRef<str>],
        kw_lowers: &[String],
        depth: usize,
        candidates: &mut Vec<PathBuf>,
    ) {
        if depth > 6 {
            return;
        }

        let entries = match fs::read_dir(current) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();
            let name_lower = to_lowercase(&name_str);

            if path.is_dir() {
                if IGNORED_DIRS.contains(&name_lower.as_str())
                    && !kw_lowers.iter().any(|k| k == &name_lower)
                {
                    continue;
                }
                walk(&path, keywords, kw_lowers, depth + 1, candidates);
            } else if path.is_file() {
                if is_binary_file(&path) {
                    continue;
                }
                if matches_keywords(&path, keywords) {
                    candidates.push(path);
                }
            }
        }
    }

    walk(dir, keywords, &kw_lowers, 0, &mut candidates);

    if candidates.is_empty() {
        return None;
    }

    // Rank candidates:
    // 1. Shallower depth (fewer path components)
    // 2. Direct match between keyword and file stem
    // 3. Shorter path string length
    candidates.sort_by(|a, b| {
        let depth_a = a.components().count();
        let depth_b = b.components().count();

        if depth_a != depth_b {
            return depth_a.cmp(&depth_b);
        }

        let stem_a = a
            .file_stem()
            .map(|s| to_lowercase(s.to_string_lossy()))
            .unwrap_or_default();
        let stem_b = b
            .file_stem()
            .map(|s| to_lowercase(s.to_string_lossy()))
            .unwrap_or_default();

        let exact_a = kw_lowers.iter().any(|k| k == &stem_a);
        let exact_b = kw_lowers.iter().any(|k| k == &stem_b);

        if exact_a != exact_b {
            return exact_b.cmp(&exact_a);
        }

        a.to_string_lossy().len().cmp(&b.to_string_lossy().len())
    });

    Some(candidates.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_keywords_empty() {
        let empty_kw: Vec<&str> = vec![];
        assert!(matches_keywords(Path::new("/home/user/test"), &empty_kw));
    }

    #[test]
    fn test_matches_keywords_single() {
        let path = Path::new("/home/user/.config/tmux/tmux.conf");
        assert!(matches_keywords(path, &["tmux"]));
        assert!(matches_keywords(path, &["TMUX"]));
        assert!(!matches_keywords(path, &["zsh"]));
    }

    #[test]
    fn test_matches_keywords_sequential_multi() {
        let path = Path::new("/home/user/.config/tmux/tmux.conf");
        assert!(matches_keywords(path, &["config", "tmux"]));
        assert!(matches_keywords(path, &["CONFIG", "TMUX.CONF"]));
        assert!(!matches_keywords(path, &["tmux", "config"]));
    }

    #[test]
    fn test_find_file_in_dir() {
        let cwd = current_dir().expect("current directory must exist");
        let found = find_file_in_dir(&cwd, &["src", "main.rs"]);
        assert!(found.is_some());
        assert!(
            found
                .expect("file exists")
                .to_string_lossy()
                .contains("main.rs")
        );
    }

    #[test]
    fn test_is_binary_file() {
        assert!(is_binary_file(Path::new("image.png")));
        assert!(is_binary_file(Path::new("lib.so")));
        assert!(is_binary_file(Path::new(".DS_Store")));
        assert!(!is_binary_file(Path::new("README.md")));
        assert!(!is_binary_file(Path::new("main.rs")));
    }

    #[test]
    fn test_home_dir_resolution() {
        let home = home_dir();
        assert!(
            home.is_ok(),
            "home_dir should resolve successfully in test environment"
        );
        let home_path = home.expect("home_dir returned Ok");
        assert!(!home_path.as_os_str().is_empty());
    }

    #[test]
    fn test_get_history_path() {
        let history_path = get_history_path();
        assert!(
            history_path.is_ok(),
            "get_history_path should resolve successfully"
        );
        let path = history_path.expect("get_history_path returned Ok");
        assert!(path.ends_with(".hopr_history"));
    }
}
