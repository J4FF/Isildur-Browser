pub mod dnr;
pub mod storage;
pub mod json;
pub mod manifest;
pub mod api;
pub mod matcher;
/// G6b: die Content-Scripts einer URL zuordnen (siehe dort, warum das
/// bisher nirgends geschah).
pub mod inhalt;

pub use manifest::{Manifest, ContentScript, RunAt, Background};
pub use api::ExtensionEngine;
pub use inhalt::{SkriptTreffer, passt_ein_muster};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    
    #[test]
    fn test_manifest_parsing() {
        let json = r#"{
            "manifest_version": 3,
            "name": "TestExt",
            "version": "1.0",
            "description": "A test",
            "permissions": ["storage", "tabs"],
            "host_permissions": ["*://*.example.com/*"],
            "content_scripts": [
                {
                    "matches": ["<all_urls>"],
                    "js": ["content.js"],
                    "run_at": "document_idle"
                }
            ],
            "background": {
                "service_worker": "bg.js"
            }
        }"#;
        
        let manifest = Manifest::parse(json).unwrap();
        assert_eq!(manifest.name, "TestExt");
        assert_eq!(manifest.manifest_version, 3);
        assert_eq!(manifest.permissions, vec!["storage", "tabs"]);
        assert_eq!(manifest.content_scripts.len(), 1);
        assert_eq!(manifest.content_scripts[0].matches, vec!["<all_urls>"]);
        assert_eq!(manifest.background.unwrap().service_worker.unwrap(), "bg.js");
    }
}
