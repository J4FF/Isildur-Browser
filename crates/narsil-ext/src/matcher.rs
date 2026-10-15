#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    AllUrls,
    Pattern {
        scheme: String, // e.g. "*", "http", "https"
        host: String,   // e.g. "*", "*.example.com", "example.com"
        path: String,   // e.g. "/*", "/foo/*"
    }
}

impl MatchPattern {
    pub fn parse(s: &str) -> Option<Self> {
        if s == "<all_urls>" {
            return Some(MatchPattern::AllUrls);
        }
        let scheme_end = s.find("://")?;
        let scheme = &s[0..scheme_end];
        if scheme != "*" && scheme != "http" && scheme != "https" && scheme != "file" && scheme != "ftp" {
            return None;
        }
        
        let rest = &s[scheme_end+3..];
        let path_start = rest.find('/');
        
        let (host, path) = if let Some(i) = path_start {
            (&rest[0..i], &rest[i..])
        } else {
            return None;
        };
        
        if host.is_empty() && scheme != "file" {
            return None;
        }
        
        if host.contains('*') && host != "*" && !host.starts_with("*.") {
            return None;
        }

        Some(MatchPattern::Pattern {
            scheme: scheme.to_string(),
            host: host.to_string(),
            path: path.to_string(),
        })
    }

    pub fn matches(&self, url: &str) -> bool {
        match self {
            MatchPattern::AllUrls => {
                url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ws://") || url.starts_with("wss://") || url.starts_with("ftp://") || url.starts_with("file://")
            }
            MatchPattern::Pattern { scheme, host, path } => {
                let url_scheme_end = match url.find("://") {
                    Some(idx) => idx,
                    None => return false,
                };
                let url_scheme = &url[0..url_scheme_end];
                if scheme != "*" && scheme != url_scheme {
                    return false;
                }
                if scheme == "*" && url_scheme != "http" && url_scheme != "https" {
                    return false;
                }
                
                let rest = &url[url_scheme_end+3..];
                let path_start = rest.find('/').unwrap_or(rest.len());
                let url_host = &rest[0..path_start];
                let url_path = if path_start < rest.len() { &rest[path_start..] } else { "/" };
                
                if host != "*" {
                    if host.starts_with("*.") {
                        let suffix = &host[2..];
                        if url_host != suffix && !url_host.ends_with(&format!(".{}", suffix)) {
                            return false;
                        }
                    } else if host != url_host {
                        return false;
                    }
                }
                
                // Path matching using simple glob (only '*' supported)
                Self::match_path(path, url_path)
            }
        }
    }
    
    fn match_path(pattern: &str, text: &str) -> bool {
        let mut p_idx = 0;
        let mut t_idx = 0;
        let p_chars: Vec<char> = pattern.chars().collect();
        let t_chars: Vec<char> = text.chars().collect();
        
        let mut star_idx = None;
        let mut match_idx = 0;
        
        while t_idx < t_chars.len() {
            if p_idx < p_chars.len() && p_chars[p_idx] == t_chars[t_idx] {
                p_idx += 1;
                t_idx += 1;
            } else if p_idx < p_chars.len() && p_chars[p_idx] == '*' {
                star_idx = Some(p_idx);
                match_idx = t_idx;
                p_idx += 1;
            } else if let Some(idx) = star_idx {
                p_idx = idx + 1;
                match_idx += 1;
                t_idx = match_idx;
            } else {
                return false;
            }
        }
        
        while p_idx < p_chars.len() && p_chars[p_idx] == '*' {
            p_idx += 1;
        }
        
        p_idx == p_chars.len()
    }
}
