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

    pub fn query(&mut self, keywords: &[impl AsRef<str>]) -> Result<&Record> {
        let now = utils::current_time().context("while attempting to get current time")?;
        let best_idx = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| utils::matches_keywords(&record.path, keywords))
            .max_by(|(_, a), (_, b)| a.score(now).total_cmp(&b.score(now)))
            .map(|(idx, _)| idx);

        if let Some(idx) = best_idx {
            self.records[idx].score *= 2.0;
            self.records[idx].last_accessed = now;
            self.save().context("while attempting to save database after query update")?;
            Ok(&self.records[idx])
        } else {
            // If single keyword and points to a file on disk, add it to database history
            if keywords.len() == 1 {
                if let Ok(canon_path) = canonicalize(keywords[0].as_ref()) {
                    let rec = Record::new(canon_path);
                    self.records.push(rec);
                    let last_idx = self.records.len() - 1;
                    self.records[last_idx].last_accessed = now;
                    self.save().context("while attempting to save database after adding path")?;
                }
            }

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
        let mut db = Database::new(PathBuf::from("/tmp/dummy_db"));
        db.records.push(Record {
            path: PathBuf::from("/home/user/.config/nvim/init.lua"),
            score: 1.0,
            last_accessed: 100,
        });

        assert!(db.query(&["nonexistent", "path"]).is_err());
    }
}

