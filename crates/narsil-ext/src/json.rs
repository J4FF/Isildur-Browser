use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get(key),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }
}

pub fn parse_json(input: &str) -> Result<JsonValue, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0;
    parse_value(&chars, &mut pos)
}

fn skip_whitespace(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn parse_value(chars: &[char], pos: &mut usize) -> Result<JsonValue, String> {
    skip_whitespace(chars, pos);
    if *pos >= chars.len() {
        return Err("Unexpected EOF".to_string());
    }
    
    match chars[*pos] {
        '{' => parse_object(chars, pos),
        '[' => parse_array(chars, pos),
        '"' => parse_string(chars, pos).map(JsonValue::String),
        't' => {
            if chars[*pos..].starts_with(&['t','r','u','e']) {
                *pos += 4;
                Ok(JsonValue::Bool(true))
            } else {
                Err("Invalid true".to_string())
            }
        }
        'f' => {
            if chars[*pos..].starts_with(&['f','a','l','s','e']) {
                *pos += 5;
                Ok(JsonValue::Bool(false))
            } else {
                Err("Invalid false".to_string())
            }
        }
        'n' => {
            if chars[*pos..].starts_with(&['n','u','l','l']) {
                *pos += 4;
                Ok(JsonValue::Null)
            } else {
                Err("Invalid null".to_string())
            }
        }
        '-' | '0'..='9' => parse_number(chars, pos),
        _ => Err(format!("Unexpected character: {}", chars[*pos])),
    }
}

fn parse_object(chars: &[char], pos: &mut usize) -> Result<JsonValue, String> {
    *pos += 1; // skip '{'
    let mut map = HashMap::new();
    skip_whitespace(chars, pos);
    
    if *pos < chars.len() && chars[*pos] == '}' {
        *pos += 1;
        return Ok(JsonValue::Object(map));
    }
    
    loop {
        skip_whitespace(chars, pos);
        if *pos >= chars.len() || chars[*pos] != '"' {
            return Err("Expected string key in object".to_string());
        }
        let key = parse_string(chars, pos)?;
        
        skip_whitespace(chars, pos);
        if *pos >= chars.len() || chars[*pos] != ':' {
            return Err("Expected ':' in object".to_string());
        }
        *pos += 1;
        
        let value = parse_value(chars, pos)?;
        map.insert(key, value);
        
        skip_whitespace(chars, pos);
        if *pos >= chars.len() {
            return Err("Unexpected EOF in object".to_string());
        }
        if chars[*pos] == '}' {
            *pos += 1;
            break;
        } else if chars[*pos] == ',' {
            *pos += 1;
        } else {
            return Err(format!("Expected ',' or '}}' in object, found {}", chars[*pos]));
        }
    }
    
    Ok(JsonValue::Object(map))
}

fn parse_array(chars: &[char], pos: &mut usize) -> Result<JsonValue, String> {
    *pos += 1; // skip '['
    let mut arr = Vec::new();
    skip_whitespace(chars, pos);
    
    if *pos < chars.len() && chars[*pos] == ']' {
        *pos += 1;
        return Ok(JsonValue::Array(arr));
    }
    
    loop {
        let value = parse_value(chars, pos)?;
        arr.push(value);
        
        skip_whitespace(chars, pos);
        if *pos >= chars.len() {
            return Err("Unexpected EOF in array".to_string());
        }
        if chars[*pos] == ']' {
            *pos += 1;
            break;
        } else if chars[*pos] == ',' {
            *pos += 1;
        } else {
            return Err(format!("Expected ',' or ']' in array, found {}", chars[*pos]));
        }
    }
    
    Ok(JsonValue::Array(arr))
}

fn parse_string(chars: &[char], pos: &mut usize) -> Result<String, String> {
    *pos += 1; // skip '"'
    let mut s = String::new();
    while *pos < chars.len() {
        match chars[*pos] {
            '"' => {
                *pos += 1;
                return Ok(s);
            }
            '\\' => {
                *pos += 1;
                if *pos >= chars.len() { return Err("Unexpected EOF in string escape".to_string()); }
                match chars[*pos] {
                    '"' | '\\' | '/' => s.push(chars[*pos]),
                    'b' => s.push('\x08'),
                    'f' => s.push('\x0C'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    'u' => {
                        *pos += 1;
                        if *pos + 4 > chars.len() { return Err("Unexpected EOF in unicode escape".to_string()); }
                        let hex: String = chars[*pos..*pos+4].iter().collect();
                        if let Ok(val) = u32::from_str_radix(&hex, 16) {
                            if let Some(c) = std::char::from_u32(val) {
                                s.push(c);
                            } else {
                                s.push('\u{FFFD}');
                            }
                        }
                        *pos += 3;
                    }
                    _ => return Err("Invalid string escape".to_string()),
                }
            }
            c => s.push(c),
        }
        *pos += 1;
    }
    Err("Unexpected EOF in string".to_string())
}

fn parse_number(chars: &[char], pos: &mut usize) -> Result<JsonValue, String> {
    let start = *pos;
    while *pos < chars.len() {
        match chars[*pos] {
            '-' | '+' | '0'..='9' | '.' | 'e' | 'E' => *pos += 1,
            _ => break,
        }
    }
    let s: String = chars[start..*pos].iter().collect();
    if let Ok(n) = s.parse::<f64>() {
        Ok(JsonValue::Number(n))
    } else {
        Err(format!("Invalid number: {}", s))
    }
}
