use crate::json::{JsonValue, parse_json};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub enum ActionType {
    Block,
    Allow,
    Redirect(String), // url
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: u32,
    pub priority: u32,
    pub action: ActionType,
    pub url_filter: Option<String>,
    pub resource_types: Vec<String>,
}

impl Rule {
    pub fn matches(&self, url: &str, resource_type: &str) -> bool {
        if !self.resource_types.is_empty() && !self.resource_types.contains(&resource_type.to_string()) {
            return false;
        }
        if let Some(filter) = &self.url_filter {
            // Simplified match: just contains for now
            if !url.contains(filter) {
                return false;
            }
        }
        true
    }
}

pub fn parse_rules(json_str: &str) -> Result<Vec<Rule>, String> {
    let parsed = parse_json(json_str)?;
    let arr = parsed.as_array().ok_or("Rules must be an array")?;
    let mut rules = Vec::new();

    for item in arr {
        let obj = item;
        
        let id = match obj.get("id") {
            Some(JsonValue::Number(n)) => *n as u32,
            _ => continue,
        };
        
        let priority = match obj.get("priority") {
            Some(JsonValue::Number(n)) => *n as u32,
            _ => 1,
        };
        
        let action = obj.get("action").ok_or("Missing action")?;
        let action_type_str = action.get("type").and_then(|v| v.as_str()).unwrap_or("");
        
        let action_enum = match action_type_str {
            "block" => ActionType::Block,
            "allow" => ActionType::Allow,
            "redirect" => {
                let url = action.get("redirect")
                    .and_then(|r| r.get("url"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                ActionType::Redirect(url)
            }
            _ => continue,
        };
        
        let condition = obj.get("condition").ok_or("Missing condition")?;
        let url_filter = condition.get("urlFilter").and_then(|v| v.as_str()).map(|s| s.to_string());
        
        let mut resource_types = Vec::new();
        if let Some(JsonValue::Array(rt_arr)) = condition.get("resourceTypes") {
            for rt in rt_arr {
                if let JsonValue::String(s) = rt {
                    resource_types.push(s.clone());
                }
            }
        }
        
        rules.push(Rule {
            id,
            priority,
            action: action_enum,
            url_filter,
            resource_types,
        });
    }
    
    Ok(rules)
}
