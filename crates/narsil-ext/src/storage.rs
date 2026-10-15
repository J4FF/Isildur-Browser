use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct LocalStorage {
    dir: PathBuf,
}

impl LocalStorage {
    pub fn new(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    fn file_path(&self, ext_id: &str) -> PathBuf {
        self.dir.join(format!("{}.tsv", ext_id))
    }

    pub fn set(&self, ext_id: &str, key: &str, value: &str, max_bytes: usize) -> Result<(), String> {
        let mut map = self.load_all(ext_id);
        map.insert(key.to_string(), value.to_string());
        
        let mut total_size = 0;
        let mut lines = String::new();
        for (k, v) in &map {
            let line = format!("{}\t{}\n", k, v);
            total_size += line.len();
            lines.push_str(&line);
        }
        
        if total_size > max_bytes {
            return Err("QuotaExceeded".to_string());
        }
        
        fs::write(self.file_path(ext_id), lines).map_err(|e| e.to_string())
    }

    pub fn get(&self, ext_id: &str, key: &str) -> Option<String> {
        let map = self.load_all(ext_id);
        map.get(key).cloned()
    }

    fn load_all(&self, ext_id: &str) -> HashMap<String, String> {
        let mut map = HashMap::new();
        if let Ok(content) = fs::read_to_string(self.file_path(ext_id)) {
            for line in content.lines() {
                if let Some((k, v)) = line.split_once('\t') {
                    map.insert(k.to_string(), v.to_string());
                }
            }
        }
        map
    }
}
