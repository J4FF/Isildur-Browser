pub fn parse_content_disposition(header: &str) -> Option<String> {
    let mut filename = None;

    // Split by semicolons
    let parts: Vec<&str> = header.split(';').collect();

    for part in parts {
        let part = part.trim();
        if part.to_lowercase().starts_with("filename*=") {
            let val = part["filename*=".len()..]
                .trim_matches('\'')
                .trim_matches('"');
            // Expected format: UTF-8''filename.ext
            if val.to_lowercase().starts_with("utf-8''") {
                let encoded = &val["utf-8''".len()..];
                if let Ok(decoded) = percent_decode(encoded) {
                    filename = Some(decoded);
                    break; // filename* takes precedence over filename
                }
            }
        } else if part.to_lowercase().starts_with("filename=") && filename.is_none() {
            let val = part["filename=".len()..].trim_matches('"');
            filename = Some(val.to_string());
        }
    }

    filename
}

fn percent_decode(s: &str) -> Result<String, String> {
    let mut res = String::new();
    let mut chars = s.chars().peekable();
    let mut bytes = Vec::new();

    while let Some(c) = chars.next() {
        if c == '%' {
            if let (Some(a), Some(b)) = (chars.next(), chars.next()) {
                let hex: String = vec![a, b].into_iter().collect();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    bytes.push(byte);
                } else {
                    return Err("Invalid percent encoding".to_string());
                }
            } else {
                return Err("Truncated percent encoding".to_string());
            }
        } else {
            if !bytes.is_empty() {
                res.push_str(&String::from_utf8_lossy(&bytes));
                bytes.clear();
            }
            res.push(c);
        }
    }
    if !bytes.is_empty() {
        res.push_str(&String::from_utf8_lossy(&bytes));
    }

    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_content_disposition() {
        assert_eq!(
            parse_content_disposition("attachment; filename=\"example.html\""),
            Some("example.html".to_string())
        );
        assert_eq!(
            parse_content_disposition("inline; filename=\"foo.html\""),
            Some("foo.html".to_string())
        );
        assert_eq!(
            parse_content_disposition("attachment; filename*=UTF-8''%e2%82%ac%20rates"),
            Some("€ rates".to_string())
        );
        // filename* takes precedence
        assert_eq!(
            parse_content_disposition(
                "attachment; filename=\"EURO rates\"; filename*=utf-8''%e2%82%ac%20rates"
            ),
            Some("€ rates".to_string())
        );
        // missing quotes
        assert_eq!(
            parse_content_disposition("attachment; filename=example.html"),
            Some("example.html".to_string())
        );
    }
}
