use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadState {
    Pending,
    Downloading { downloaded: u64, total: Option<u64> },
    Paused { downloaded: u64, total: Option<u64> },
    Finished(PathBuf),
    Failed(String),
}

pub struct DownloadManager {
    pub download_dir: PathBuf,
}

impl DownloadManager {
    pub fn new(download_dir: PathBuf) -> Self {
        fs::create_dir_all(&download_dir).unwrap_or_default();
        Self { download_dir }
    }

    pub fn resolve_filename(&self, suggested: &str) -> PathBuf {
        let mut path = self.download_dir.join(suggested);
        let mut counter = 1;
        while path.exists() {
            let stem = Path::new(suggested)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let ext = Path::new(suggested)
                .extension()
                .unwrap_or_default()
                .to_string_lossy();
            let new_name = if ext.is_empty() {
                format!("{} ({})", stem, counter)
            } else {
                format!("{} ({}).{}", stem, counter, ext)
            };
            path = self.download_dir.join(new_name);
            counter += 1;
        }
        path
    }

    // A simplified synchronous download function for the test.
    // In a real async browser, this would be non-blocking.
    pub fn download_sync(&self, url: &str, suggested_name: &str) -> DownloadState {
        let final_path = self.resolve_filename(suggested_name);
        let part_path = final_path.with_extension(format!(
            "{}.teil",
            final_path.extension().unwrap_or_default().to_string_lossy()
        ));

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)
            .unwrap();
        let existing_size = file.metadata().unwrap().len();

        let host = url
            .strip_prefix("http://")
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap();
        let path = url[url.find(host).unwrap() + host.len()..].to_string();
        let path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };

        let mut stream = match TcpStream::connect(host) {
            Ok(s) => s,
            Err(e) => return DownloadState::Failed(e.to_string()),
        };

        let request = if existing_size > 0 {
            format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nRange: bytes={}-\r\nConnection: close\r\n\r\n",
                path, host, existing_size
            )
        } else {
            format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                path, host
            )
        };

        if let Err(e) = stream.write_all(request.as_bytes()) {
            return DownloadState::Failed(e.to_string());
        }

        let mut response = Vec::new();
        if let Err(e) = stream.read_to_end(&mut response) {
            return DownloadState::Failed(e.to_string());
        }

        // Parse HTTP response
        let header_end = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(0);
        let headers = &response[..header_end];
        let body = &response[header_end + 4..];

        let header_str = String::from_utf8_lossy(headers);
        let content_length = header_str
            .lines()
            .find(|l| l.to_lowercase().starts_with("content-length:"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);

        if header_str.starts_with("HTTP/1.1 200") || header_str.starts_with("HTTP/1.1 206") {
            file.write_all(body).unwrap();
            let new_size = file.metadata().unwrap().len();
            let expected_total = if header_str.starts_with("HTTP/1.1 206") {
                content_length + existing_size
            } else {
                content_length
            };

            if expected_total > 0 && new_size < expected_total {
                // Interrupted
                DownloadState::Failed("Interrupted".to_string())
            } else {
                fs::rename(&part_path, &final_path).unwrap();
                DownloadState::Finished(final_path)
            }
        } else {
            DownloadState::Failed(format!(
                "HTTP error: {}",
                header_str.lines().next().unwrap_or("")
            ))
        }
    }
}
