use std::{
    fmt::Display,
    fs::{self, canonicalize},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow};
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};

use crate::utils::{self, DAY, HOUR, WEEK};

#[derive(Debug, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct Record {
    pub path: PathBuf,
    pub score: Rank,
    pub last_accessed: Epoch,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Database {
    pub path: PathBuf,
    pub records: Vec<Record>,
}

impl Display for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for r in &self.records {
            let _ = writeln!(f, "{:?}: {}", r.path, r.score);
        }
        Ok(())
    }
}

impl Record {
    pub fn new(path: PathBuf) -> Self {
        Record {
            path,
            score: 1.0,
            last_accessed: 0,
        }
    }

    pub fn score(&self, now: Epoch) -> Rank {
        // The older the entry, the lesser its importance.
        let duration = now.saturating_sub(self.last_accessed);
        if duration < HOUR {
            self.score * 4.0
        } else if duration < DAY {
            self.score * 2.0
        } else if duration < WEEK {
            self.score * 0.5
        } else {
            self.score * 0.25
        }
    }
}

impl Database {
    pub fn new(path: PathBuf) -> Self {
        Database {
            path,
            records: vec![],
        }
    }

    pub fn load(&mut self) -> Result<Self> {
        let file_bytes = fs::read(&self.path)
            .with_context(|| format!("while attempting to read database from {}", self.path.display()))?;
        let decoded_db: Database = from_bytes(&file_bytes)
            .context("while attempting to decode database bytes")?;
        Ok(decoded_db)
    }

    pub fn add(&mut self, file: PathBuf) {
        let file = Record::new(file);
        self.records.push(file);
    }

    pub fn save(&mut self) -> Result<()> {
        let encoded_bytes = to_allocvec(self)
            .context("while attempting to encode database")?;
        fs::write(&self.path, encoded_bytes)
            .with_context(|| format!("while attempting to write database to {}", self.path.display()))?;
        Ok(())
    }

    fn get_or_insert_record(&mut self, path: PathBuf, now: Epoch) -> Result<&Record> {
        let existing_idx = self.records.iter().position(|r| r.path == path);
        let idx = match existing_idx {
            Some(i) => {
                self.records[i].score *= 2.0;
                self.records[i].last_accessed = now;
                i
            }
            None => {
                let mut rec = Record::new(path);
                rec.last_accessed = now;
                self.records.push(rec);
                self.records.len() - 1
            }
        };
        self.save().context("while attempting to save database after record update")?;
        Ok(&self.records[idx])
    }

    pub fn query(&mut self, keywords: &[impl AsRef<str>]) -> Result<&Record> {
        let now = utils::current_time().context("while attempting to get current time")?;

        // 1. Prioritize direct relative/absolute path match on disk
        let path_slash = keywords.iter().map(|k| k.as_ref()).collect::<Vec<_>>().join("/");
        let path_space = keywords.iter().map(|k| k.as_ref()).collect::<Vec<_>>().join(" ");

        let direct_path = canonicalize(&path_slash)
            .ok()
            .or_else(|| canonicalize(&path_space).ok())
            .or_else(|| {
                if keywords.len() == 1 {
                    canonicalize(keywords[0].as_ref()).ok()
                } else {
                    None
                }
            })
            .filter(|p| p.is_file());

        if let Some(canon_path) = direct_path {
            return self.get_or_insert_record(canon_path, now);
        }

        // 2. Prioritize matching file in the current working directory or its subdirectories
        if let Ok(cwd) = utils::current_dir() {
            if let Some(local_path) = utils::find_file_in_dir(&cwd, keywords) {
                if let Ok(canon_local) = canonicalize(local_path) {
                    if canon_local.is_file() {
                        return self.get_or_insert_record(canon_local, now);
                    }
                }
            }
        }

        // 3. Fallback to global history records by frecency score (ignoring binary files)
        let best_idx = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| !utils::is_binary_file(&record.path) && utils::matches_keywords(&record.path, keywords))
            .max_by(|(_, a), (_, b)| a.score(now).total_cmp(&b.score(now)))
            .map(|(idx, _)| idx);

        if let Some(idx) = best_idx {
            self.records[idx].score *= 2.0;
            self.records[idx].last_accessed = now;
            self.save().context("while attempting to save database after query update")?;
            Ok(&self.records[idx])
        } else {
            Err(anyhow!("Did not find record matching query"))
        }
    }
}

pub type Epoch = u64;
pub type Rank = f64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_query_multi_keyword_matching() {
        let mut db = Database::new(PathBuf::from("/tmp/dummy_db"));
        db.records.push(Record {
            path: PathBuf::from("/home/user/.config/nvim/init.lua"),
            score: 1.0,
            last_accessed: 100,
        });
        db.records.push(Record {
            path: PathBuf::from("/home/user/.config/tmux/tmux.conf"),
            score: 2.0,
            last_accessed: 100,
        });
        db.records.push(Record {
            path: PathBuf::from("/home/user/tmux/notes.txt"),
            score: 5.0,
            last_accessed: 100,
        });

        // Querying ["config", "tmux"] should match /home/user/.config/tmux/tmux.conf
        // even though notes.txt has a higher raw score, because notes.txt does not contain "config".
        let matched = db.query(&["config", "tmux"]).expect("should match tmux.conf");
        assert_eq!(matched.path, PathBuf::from("/home/user/.config/tmux/tmux.conf"));
    }

    #[test]
    fn test_db_query_frecency_ranking() {
        let mut db = Database::new(PathBuf::from("/tmp/dummy_db"));
        db.records.push(Record {
            path: PathBuf::from("/home/user/projects/tmux-plugin/tmux.conf"),
            score: 1.0,
            last_accessed: 100,
        });
        db.records.push(Record {
            path: PathBuf::from("/home/user/.config/tmux/tmux.conf"),
            score: 10.0,
            last_accessed: 100,
        });

        let matched = db.query(&["tmux", "conf"]).expect("should match higher score tmux.conf");
        assert_eq!(matched.path, PathBuf::from("/home/user/.config/tmux/tmux.conf"));
    }

    #[test]
    fn test_db_query_no_match() {
        let mut db = Database::new(PathBuf::from("/tmp/dummy_db_nomatch"));
        db.records.push(Record {
            path: PathBuf::from("/home/user/.config/nvim/init.lua"),
            score: 1.0,
            last_accessed: 100,
        });

        assert!(db.query(&["nonexistent_xyz_123", "path"]).is_err());
    }

    #[test]
    fn test_db_query_fallback_file_on_disk() {
        let mut db = Database::new(PathBuf::from("/tmp/dummy_db_fallback"));
        // DB starts empty, but "src/main.rs" exists on disk
        let matched = db.query(&["src", "main.rs"]).expect("should locate src/main.rs on disk");
        assert!(matched.path.to_string_lossy().contains("src/main.rs"));
        assert_eq!(db.records.len(), 1);
    }

    #[test]
    fn test_db_query_prioritizes_cwd_file_over_history() {
        let mut db = Database::new(PathBuf::from("/tmp/dummy_db_cwd_prio"));
        // History contains a main.rs from another directory with high score
        db.records.push(Record {
            path: PathBuf::from("/other/path/src/main.rs"),
            score: 1000.0,
            last_accessed: 100,
        });

        // Querying ["src", "main.rs"] from current repo should match current repo's src/main.rs
        let matched = db.query(&["src", "main.rs"]).expect("should locate current dir src/main.rs");
        assert!(matched.path.to_string_lossy().contains("hopr/src/main.rs"));
    }
}



