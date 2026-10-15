use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HISTORY_LINES: usize = 100_000;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub timestamp: u64,
    pub visits: u32,
}

use std::collections::HashMap;

pub struct HistoryManager {
    file_path: PathBuf,
    entries: Vec<HistoryEntry>,
    url_to_index: HashMap<String, usize>,
}

impl HistoryManager {
    pub fn new(file_path: PathBuf) -> Self {
        let mut hm = Self { file_path, entries: Vec::new(), url_to_index: HashMap::new() };
        hm.load().unwrap_or_default();
        hm
    }

    pub fn load(&mut self) -> io::Result<()> {
        if !self.file_path.exists() {
            return Ok(());
        }
        let file = fs::File::open(&self.file_path)?;
        let reader = BufReader::new(file);
        
        self.entries.clear();
        self.url_to_index.clear();
        for line_res in reader.lines() {
            let line = line_res?;
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                if let (Ok(ts), Ok(vis)) = (parts[2].parse(), parts[3].parse()) {
                    let url = parts[0].to_string();
                    self.url_to_index.insert(url.clone(), self.entries.len());
                    self.entries.push(HistoryEntry {
                        url,
                        title: parts[1].to_string(),
                        timestamp: ts,
                        visits: vis,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn save(&self) -> io::Result<()> {
        let mut file = fs::File::create(&self.file_path)?;
        for entry in self.entries.iter().rev().take(MAX_HISTORY_LINES).rev() {
            writeln!(file, "{}\t{}\t{}\t{}", entry.url, entry.title, entry.timestamp, entry.visits)?;
        }
        Ok(())
    }

    pub fn add_visit(&mut self, url: &str, title: &str) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if let Some(&pos) = self.url_to_index.get(url) {
            self.entries[pos].title = title.to_string();
            self.entries[pos].timestamp = now;
            self.entries[pos].visits += 1;
        } else {
            if self.entries.len() >= MAX_HISTORY_LINES {
                let removed = self.entries.remove(0);
                self.url_to_index.remove(&removed.url);
                // We would need to update all indices, which is slow.
                // For simplicity, just rebuild the index map.
                self.url_to_index.clear();
                for (i, e) in self.entries.iter().enumerate() {
                    self.url_to_index.insert(e.url.clone(), i);
                }
            }
            self.url_to_index.insert(url.to_string(), self.entries.len());
            self.entries.push(HistoryEntry {
                url: url.to_string(),
                title: title.to_string(),
                timestamp: now,
                visits: 1,
            });
        }
    }

    pub fn search(&self, prefix: &str) -> Vec<HistoryEntry> {
        let query = prefix.to_lowercase();
        let mut results: Vec<&HistoryEntry> = self.entries.iter()
            .filter(|e| e.url.to_lowercase().contains(&query) || e.title.to_lowercase().contains(&query))
            .collect();
            
        // Sort by visits (descending), then by recency (descending)
        results.sort_by(|a, b| b.visits.cmp(&a.visits).then(b.timestamp.cmp(&a.timestamp)));
        
        results.into_iter().take(10).cloned().collect()
    }

    pub fn delete_all(&mut self) -> io::Result<()> {
        self.entries.clear();
        self.url_to_index.clear();
        self.save()
    }

    pub fn delete_last_hour(&mut self) -> io::Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let one_hour_ago = now.saturating_sub(3600);
        self.entries.retain(|e| e.timestamp < one_hour_ago);
        self.url_to_index.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.url_to_index.insert(e.url.clone(), i);
        }
        self.save()
    }
}

pub struct SessionManager {
    file_path: PathBuf,
    pub active: bool,
}

impl SessionManager {
    pub fn new(file_path: PathBuf, active: bool) -> Self {
        Self { file_path, active }
    }

    pub fn save_session(&self, urls: &[String]) -> io::Result<()> {
        if !self.active { return Ok(()); }
        let mut file = fs::File::create(&self.file_path)?;
        for url in urls {
            writeln!(file, "{}", url)?;
        }
        Ok(())
    }

    pub fn restore_session(&self) -> io::Result<Vec<String>> {
        if !self.active || !self.file_path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&self.file_path)?;
        let reader = BufReader::new(file);
        let mut urls = Vec::new();
        for line in reader.lines() {
            urls.push(line?);
        }
        Ok(urls)
    }
}
