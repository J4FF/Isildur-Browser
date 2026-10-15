use crate::json::{parse_json, JsonValue};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub manifest_version: u32,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub content_scripts: Vec<ContentScript>,
    pub background: Option<Background>,
    pub declarative_net_request_rule_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContentScript {
    pub matches: Vec<String>,
    pub js: Vec<String>,
    pub css: Vec<String>,
    pub run_at: RunAt,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Background {
    pub scripts: Vec<String>,
    pub service_worker: Option<String>,
}

impl Manifest {
    pub fn load_from_dir(path: &Path) -> Result<Self, String> {
        let manifest_path = path.join("manifest.json");
        let content = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self, String> {
        let json = parse_json(content)?;
        
        let manifest_version = match json.get("manifest_version") {
            Some(JsonValue::Number(n)) if *n == 3.0 => 3,
            _ => return Err("manifest_version must be 3".to_string()),
        };
        
        let name = json.get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "name is required".to_string())?
            .to_string();
            
        let version = json.get("version")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "version is required".to_string())?
            .to_string();
            
        let description = json.get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
            
        let permissions = parse_string_array(json.get("permissions"));
        let host_permissions = parse_string_array(json.get("host_permissions"));
        
        let mut content_scripts = Vec::new();
        if let Some(JsonValue::Array(scripts)) = json.get("content_scripts") {
            for script in scripts {
                let matches = parse_string_array(script.get("matches"));
                if matches.is_empty() {
                    return Err("content_scripts must have 'matches'".to_string());
                }
                let js = parse_string_array(script.get("js"));
                let css = parse_string_array(script.get("css"));
                
                let run_at = match script.get("run_at").and_then(|v| v.as_str()) {
                    Some("document_start") => RunAt::DocumentStart,
                    Some("document_end") => RunAt::DocumentEnd,
                    Some("document_idle") | None => RunAt::DocumentIdle,
                    Some(other) => return Err(format!("Invalid run_at: {}", other)),
                };
                
                content_scripts.push(ContentScript { matches, js, css, run_at });
            }
        }
        
        let background = if let Some(bg) = json.get("background") {
            let scripts = parse_string_array(bg.get("scripts"));
            let service_worker = bg.get("service_worker").and_then(|v| v.as_str()).map(|s| s.to_string());
            Some(Background { scripts, service_worker })
        } else {
            None
        };
        
        let mut declarative_net_request_rule_path = None;
        if let Some(dnr) = json.get("declarative_net_request") {
            if let Some(JsonValue::Array(rules)) = dnr.get("rule_resources") {
                for rule in rules {
                    if let Some(path) = rule.get("path").and_then(|v| v.as_str()) {
                        declarative_net_request_rule_path = Some(path.to_string());
                        break;
                    }
                }
            }
        }
        
        Ok(Manifest {
            name,
            version,
            manifest_version,
            description,
            permissions,
            host_permissions,
            content_scripts,
            background,
            declarative_net_request_rule_path,
        })
    }
}

fn parse_string_array(val: Option<&JsonValue>) -> Vec<String> {
    let mut res = Vec::new();
    if let Some(JsonValue::Array(arr)) = val {
        for item in arr {
            if let JsonValue::String(s) = item {
                res.push(s.clone());
            }
        }
    }
    res
}
