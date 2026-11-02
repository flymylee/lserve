use crate::config::Config;
use anyhow::{Context, Result};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, UtcOffset, macros::format_description};
use tokio::{
    fs::{self, File, OpenOptions},
    io::AsyncWriteExt,
    sync::mpsc,
};

#[derive(Clone)]
pub struct Logger {
    tx: mpsc::UnboundedSender<LogMessage>,
}

enum LogMessage {
    Access(String),
    Error(String),
}

impl Logger {
    pub async fn new(config: &Config) -> Result<Self> {
        let access = match &config.access_log {
            Some(path) => Some(
                RotatingFile::open(path.clone(), config.log_max_size, config.log_generations)
                    .await?,
            ),
            None => None,
        };
        let error = match &config.error_log {
            Some(path) => Some(
                RotatingFile::open(path.clone(), config.log_max_size, config.log_generations)
                    .await?,
            ),
            None => None,
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut access = access;
            let mut error = error;
            while let Some(msg) = rx.recv().await {
                match msg {
                    LogMessage::Access(line) => {
                        println!("{line}");
                        if let Some(file) = access.as_mut()
                            && let Err(e) = file.write_line(&line).await
                        {
                            eprintln!("{} logger access-file error: {e:#}", timestamp());
                        }
                    }
                    LogMessage::Error(line) => {
                        eprintln!("{line}");
                        if let Some(file) = error.as_mut()
                            && let Err(e) = file.write_line(&line).await
                        {
                            eprintln!("{} logger error-file error: {e:#}", timestamp());
                        }
                    }
                }
            }
        });
        Ok(Self { tx })
    }

    pub fn access(&self, message: impl Into<String>) {
        let _ = self.tx.send(LogMessage::Access(format!(
            "{} {}",
            timestamp(),
            message.into()
        )));
    }

    pub fn error(&self, message: impl Into<String>) {
        let _ = self.tx.send(LogMessage::Error(format!(
            "{} {}",
            timestamp(),
            message.into()
        )));
    }
}

pub fn timestamp() -> String {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let now = OffsetDateTime::now_utc().to_offset(offset);
    now.format(&format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]"
    ))
    .unwrap_or_else(|_| now.unix_timestamp().to_string())
}

fn rotation_timestamp() -> String {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let now = OffsetDateTime::now_utc().to_offset(offset);
    now.format(&format_description!(
        "[year][month][day]_[hour][minute][second]"
    ))
    .unwrap_or_else(|_| now.unix_timestamp().to_string())
}

struct RotatingFile {
    path: PathBuf,
    max_size: u64,
    generations: usize,
    size: u64,
    file: Option<File>,
}

impl RotatingFile {
    async fn open(path: PathBuf, max_size: u64, generations: usize) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create log directory {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("failed to open log file {}", path.display()))?;
        let size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            max_size,
            generations,
            size,
            file: Some(file),
        })
    }

    async fn write_line(&mut self, line: &str) -> Result<()> {
        let bytes = line.as_bytes();
        let write_len = bytes.len() as u64 + 1;
        if self.size > 0 && self.size.saturating_add(write_len) > self.max_size {
            self.rotate().await?;
        }
        let file = self.file.as_mut().expect("log file open");
        file.write_all(bytes).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        self.size = self.size.saturating_add(write_len);
        Ok(())
    }

    async fn rotate(&mut self) -> Result<()> {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush().await;
            drop(file);
        }

        if self.generations == 0 {
            match fs::remove_file(&self.path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        } else if fs::metadata(&self.path).await.is_ok() {
            let rotated = self.unique_rotated_path().await;
            fs::rename(&self.path, &rotated).await.with_context(|| {
                format!(
                    "failed to rotate {} to {}",
                    self.path.display(),
                    rotated.display()
                )
            })?;
            self.cleanup_old().await?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        self.file = Some(file);
        self.size = 0;
        Ok(())
    }

    async fn unique_rotated_path(&self) -> PathBuf {
        let stamp = rotation_timestamp();
        let base = format!("{}.{}", self.file_name(), stamp);
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let first = parent.join(&base);
        if fs::metadata(&first).await.is_err() {
            return first;
        }
        for n in 1usize.. {
            let candidate = parent.join(format!("{base}.{n}"));
            if fs::metadata(&candidate).await.is_err() {
                return candidate;
            }
        }
        unreachable!()
    }

    async fn cleanup_old(&self) -> Result<()> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let mut dir = match fs::read_dir(parent).await {
            Ok(dir) => dir,
            Err(e) => return Err(e.into()),
        };
        let prefix = format!("{}.", self.file_name());
        let mut rotated = Vec::new();
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name();
            let name = match name.to_str() {
                Some(name) => name,
                None => continue,
            };
            if let Some(suffix) = name.strip_prefix(&prefix)
                && let Some((stamp, sequence)) = parse_rotation_suffix(suffix)
            {
                rotated.push((stamp.to_string(), sequence, entry.path()));
            }
        }
        // Timestamps sort lexicographically. The numeric collision suffix must be
        // compared numerically so `.10` is newer than `.9` within the same second.
        rotated.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        for (_, _, path) in rotated.into_iter().skip(self.generations) {
            match fs::remove_file(&path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn file_name(&self) -> &str {
        self.path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("lserve.log")
    }
}

fn parse_rotation_suffix(s: &str) -> Option<(&str, usize)> {
    let mut parts = s.split('.');
    let stamp = parts.next()?;
    if stamp.len() != 15 || stamp.as_bytes().get(8) != Some(&b'_') {
        return None;
    }
    if !stamp[..8].bytes().all(|b| b.is_ascii_digit())
        || !stamp[9..].bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let sequence = match parts.next() {
        None => 0,
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => n.parse().ok()?,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((stamp, sequence))
}

#[cfg(test)]
fn is_rotation_suffix(s: &str) -> bool {
    parse_rotation_suffix(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_names() {
        assert!(is_rotation_suffix("20260924_191105"));
        assert!(is_rotation_suffix("20260924_191105.1"));
        assert!(!is_rotation_suffix("backup"));
        assert!(!is_rotation_suffix("20260924-191105"));
        assert_eq!(
            parse_rotation_suffix("20260924_191105.10"),
            Some(("20260924_191105", 10))
        );
    }
}
