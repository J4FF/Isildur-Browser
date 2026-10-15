#[derive(Debug, Clone, PartialEq)]
pub enum MediaSource {
    DirectDownload(String),
    StreamNotDownloadable(String), // HLS/DASH
    NotFound,
}

#[derive(Debug, PartialEq)]
struct SrcsetCandidate {
    url: String,
    width: Option<u32>,
    density: Option<f32>,
}

fn parse_srcset(srcset: &str) -> Vec<SrcsetCandidate> {
    let mut candidates = Vec::new();
    let parts = srcset.split(',');

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let mut tokens = part.split_whitespace();
        if let Some(url) = tokens.next() {
            let mut width = None;
            let mut density = None;

            for token in tokens {
                if token.ends_with('w') {
                    if let Ok(w) = token[..token.len() - 1].parse::<u32>() {
                        width = Some(w);
                    }
                } else if token.ends_with('x') {
                    if let Ok(d) = token[..token.len() - 1].parse::<f32>() {
                        density = Some(d);
                    }
                }
            }
            candidates.push(SrcsetCandidate {
                url: url.to_string(),
                width,
                density,
            });
        }
    }
    candidates
}

fn best_srcset(candidates: &[SrcsetCandidate]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    let mut best = &candidates[0];
    for c in candidates.iter().skip(1) {
        if let (Some(w1), Some(w2)) = (c.width, best.width) {
            if w1 > w2 {
                best = c;
                continue;
            }
        }
        if let (Some(d1), Some(d2)) = (c.density, best.density) {
            if d1 > d2 {
                best = c;
                continue;
            }
        }
    }
    Some(best.url.clone())
}

pub fn is_hls_dash(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains(".m3u8") || lower.contains(".mpd") || lower.contains("manifest")
}

pub fn find_media_source(
    element_tag: &str,
    attributes: &[(String, String)],
    inner_html: &str,
) -> MediaSource {
    match element_tag {
        "img" => {
            let mut src = None;
            let mut srcset = None;
            for (k, v) in attributes {
                if k == "src" {
                    src = Some(v.clone());
                }
                if k == "srcset" {
                    srcset = Some(v.clone());
                }
            }
            if let Some(s) = srcset {
                let candidates = parse_srcset(&s);
                if let Some(best) = best_srcset(&candidates) {
                    return MediaSource::DirectDownload(best);
                }
            }
            if let Some(s) = src {
                return MediaSource::DirectDownload(s);
            }
            MediaSource::NotFound
        }
        "video" | "audio" => {
            let mut src = None;
            for (k, v) in attributes {
                if k == "src" {
                    src = Some(v.clone());
                }
            }

            // Extract from <source src="..."> inside inner_html
            if src.is_none() {
                let mut current_idx = 0;
                while let Some(start) = inner_html[current_idx..].find("<source") {
                    let source_tag_start = current_idx + start;
                    let end = inner_html[source_tag_start..]
                        .find(">")
                        .unwrap_or(inner_html[source_tag_start..].len());
                    let tag = &inner_html[source_tag_start..source_tag_start + end];

                    if let Some(src_idx) = tag.find("src=\"") {
                        let val_start = src_idx + 5;
                        if let Some(val_len) = tag[val_start..].find("\"") {
                            let url = &tag[val_start..val_start + val_len];
                            if is_hls_dash(url) {
                                return MediaSource::StreamNotDownloadable(url.to_string());
                            }
                            if src.is_none() {
                                src = Some(url.to_string());
                            }
                        }
                    }
                    current_idx = source_tag_start + end;
                }
            }

            if let Some(s) = src {
                if is_hls_dash(&s) {
                    return MediaSource::StreamNotDownloadable(s);
                } else {
                    return MediaSource::DirectDownload(s);
                }
            }
            MediaSource::NotFound
        }
        _ => MediaSource::NotFound,
    }
}

#[cfg(test)]
include!("media_tests.rs");
