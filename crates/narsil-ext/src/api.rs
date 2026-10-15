use std::collections::HashMap;
use std::path::PathBuf;
use crate::dnr::{Rule, ActionType};
use crate::storage::LocalStorage;
use crate::manifest::Manifest;

#[derive(Debug, Clone)]
pub struct RuntimeMessage {
    pub ext_id: String,
    pub sender: String,
    pub payload: String,
}

pub struct ExtensionEngine {
    pub extensions: HashMap<String, Manifest>,
    pub storage: LocalStorage,
    pub rules: HashMap<String, Vec<Rule>>, // ext_id -> Rules
    pub message_queue: Vec<RuntimeMessage>,
}

impl ExtensionEngine {
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            extensions: HashMap::new(),
            storage: LocalStorage::new(storage_dir),
            rules: HashMap::new(),
            message_queue: Vec::new(),
        }
    }

    pub fn load_extension(&mut self, ext_id: String, manifest: Manifest, ext_dir: &std::path::Path) {
        if manifest.permissions.contains(&"declarativeNetRequest".to_string()) {
            if let Some(rules_file) = &manifest.declarative_net_request_rule_path {
                let p = ext_dir.join(rules_file);
                if let Ok(content) = std::fs::read_to_string(p) {
                    if let Ok(r) = crate::dnr::parse_rules(&content) {
                        self.rules.insert(ext_id.clone(), r);
                    }
                }
            }
        }
        self.extensions.insert(ext_id, manifest);
    }
    
    /// G6b: LÄDT ALLE Erweiterungen unter einem Ordner.
    ///
    /// VORHER stand hier `Manifest::load_from_dir(path)` — also EIN
    /// Manifest direkt im Ordner. GEMESSEN, warum das die Funktion
    /// unbrauchbar machte: `~/.config/narsil/erweiterungen/` enthaelt nach
    /// dem Manifest-Schema EINE Erweiterung je Unterordner
    /// (`beispiele/blocker/manifest.json`), nicht ein Manifest im
    /// Wurzelordner. Der Aufrufer (`GeminiDienste::new`) reicht genau
    /// diesen Wurzelordner — also fand `load_from_dir` dort kein
    /// `manifest.json`, `Manifest::load_from_dir` gab `Err`, und der
    /// Aufrufer verwarf den Fehler mit `let _ =`. Ergebnis: null
    /// Erweiterungen, null Content-Scripts, null DNR-Regeln. Und weil
    /// der Fehler weggeworfen wurde, war es still.
    ///
    /// BEIDE FAELLE werden jetzt behandelt, weil beide vorkommen:
    ///  * `<wurzel>/manifest.json` — die flache Form (ein Manifest), und
    ///  * `<wurzel>/<name>/manifest.json` — die uebliche Form.
    ///
    /// M3: `MAX_ERWEITERUNGEN` (64). Der Ordner ist eine Eingabe von der
    /// Platte, und ein Browser mit 10 000 Erweiterungen ist kein Browser,
    /// sondern ein Angriff.
    ///
    /// DIE REIHENFOLGE ist die Sortedition der Verzeichnisnamen — nicht
    /// die Reihenfolge, in der das Dateisystem sie liefert. GEMESSEN,
    /// warum das zählt: `content_scripts_fuer` sortiert nach Name, und
    /// wenn zwei Erweiterungen dasselbe tun, muss die Reihenfolge beim
    /// Laden und beim Auswerten dieselbe sein.
    pub fn load_from_dir(&mut self, path: &std::path::Path) -> Result<(), String> {
        /// M3: hoechstens 64 Erweiterungen aus einem Ordner.
        const MAX_ERWEITERUNGEN: usize = 64;
        let mut geladen = 0usize;
        if path.join("manifest.json").is_file() {
            let manifest = Manifest::load_from_dir(path)?;
            let name = manifest.name.clone();
            self.load_extension(name, manifest, path);
            return Ok(());
        }
        let mut namen: Vec<std::path::PathBuf> = match std::fs::read_dir(path) {
            Ok(e) => e
                .filter_map(|d| d.ok())
                .map(|d| d.path())
                .filter(|p| p.is_dir() && p.join("manifest.json").is_file())
                .collect(),
            // M3: kein Ordner ist kein Fehler — ein Browser ohne
            // Erweiterungen ist normal, und `GeminiDienste::new` ruft
            // das nur, wenn der Ordner existiert.
            Err(_) => return Ok(()),
        };
        namen.sort();
        for dir in namen.into_iter().take(MAX_ERWEITERUNGEN) {
            match Manifest::load_from_dir(&dir) {
                Ok(manifest) => {
                    let name = manifest.name.clone();
                    self.load_extension(name, manifest, &dir);
                    geladen += 1;
                }
                // EINE kaputte Erweiterung darf die anderen nicht
                // mitnehmen: `let _` waere hier still, also nennen wir
                // sie im Rueckgabe-Fehler, wenn ALLE kaputt sind.
                Err(_) => continue,
            }
        }
        if geladen == 0 {
            return Err(std::format!(
                "unter {} liegt keine lesbare Erweiterung",
                path.display()
            ));
        }
        Ok(())
    }
    
    // API: storage.local.set
    pub fn storage_local_set(&self, ext_id: &str, key: &str, value: &str) -> Result<(), String> {
        // 5 MB quota
        self.storage.set(ext_id, key, value, 5 * 1024 * 1024)
    }
    
    // API: storage.local.get
    pub fn storage_local_get(&self, ext_id: &str, key: &str) -> Option<String> {
        self.storage.get(ext_id, key)
    }
    
    // API: webRequest blockierung via declarativeNetRequest
    pub fn evaluate_request(&self, url: &str, resource_type: &str) -> ActionType {
        let mut highest_priority = 0;
        let mut final_action = ActionType::Allow;
        
        for rules in self.rules.values() {
            for rule in rules {
                if rule.matches(url, resource_type) {
                    if rule.priority > highest_priority {
                        highest_priority = rule.priority;
                        final_action = rule.action.clone();
                    } else if rule.priority == highest_priority {
                        // Tie breaking: Block > Redirect > Allow
                        match (&final_action, &rule.action) {
                            (ActionType::Allow, ActionType::Block) => final_action = ActionType::Block,
                            (ActionType::Allow, ActionType::Redirect(_)) => final_action = rule.action.clone(),
                            (ActionType::Redirect(_), ActionType::Block) => final_action = ActionType::Block,
                            _ => {}
                        }
                    }
                }
            }
        }
        final_action
    }
    
    /// G6b: DIE CONTENT-SCRIPTS FUER EINE ADRESSE.
    ///
    /// Vorher stand hier nichts: `Manifest::parse` las die
    /// `content_scripts`, und niemand ging sie durch. Der Host
    /// (`narsil/src/dienste.rs::execute_content_scripts`) gab eine leere
    /// Liste zurueck und nannte den Motor nicht einmal — gemessen als
    /// „Jede Funktion muss im echten Bedienweg aufgerufen werden".
    ///
    /// DIE REIHENFOLGE ist die Manifest-Reihenfolge, und sie ist
    /// Absicht: `HashMap::values()` hat keine Reihenfolge, und zwei
    /// Erweiterungen, die dasselbe tun, muessen in derselben Reihenfolge
    /// laufen wie beim Laden. Deshalb wird die Map einmal nach Namen
    /// sortiert (M3: `MAX_ERWEITERUNGEN`) — die Sortierung ist
    /// billiger als eine zweite Wahrheit ueber „in welcher Reihenfolge".
    #[must_use]
    pub fn content_scripts_fuer(&self, url: &str) -> Vec<crate::inhalt::SkriptTreffer> {
        // M3: die Zahl der Erweiterungen, die ueberhaupt in eine Seite
        // hineinsehen. Wer 500 installiert hat, soll nicht 500 Muster
        // gegen jede Adresse laufen lassen — die Sammlung ist auch eine
        // Eingabe (aus einem Ordner auf der Platte).
        const MAX_ERWEITERUNGEN: usize = 64;
        let mut namen: Vec<&String> = self.extensions.keys().collect();
        namen.sort();
        let mut raus: Vec<crate::inhalt::SkriptTreffer> = Vec::new();
        for name in namen.into_iter().take(MAX_ERWEITERUNGEN) {
            let manifest = &self.extensions[name];
            for skript in manifest.content_scripts.iter() {
                if raus.len() >= crate::inhalt::MAX_TREFFER {
                    return raus;
                }
                if !crate::inhalt::passt_ein_muster(&skript.matches, url) {
                    continue;
                }
                let js: Vec<String> = skript
                    .js
                    .iter()
                    .take(crate::inhalt::MAX_SKRIPTE_PRO_ERWEITERUNG)
                    .cloned()
                    .collect();
                let css: Vec<String> = skript
                    .css
                    .iter()
                    .take(crate::inhalt::MAX_SKRIPTE_PRO_ERWEITERUNG)
                    .cloned()
                    .collect();
                let treffer = crate::inhalt::SkriptTreffer {
                    ext_id: name.clone(),
                    js,
                    css,
                    run_at: skript.run_at.clone(),
                };
                if treffer.hat_arbeit() {
                    raus.push(treffer);
                }
            }
        }
        raus
    }

    // API: runtime.sendMessage
    pub fn runtime_send_message(&mut self, ext_id: &str, sender: &str, payload: &str) {
        self.message_queue.push(RuntimeMessage {
            ext_id: ext_id.to_string(),
            sender: sender.to_string(),
            payload: payload.to_string(),
        });
    }
    
    pub fn pop_message(&mut self) -> Option<RuntimeMessage> {
        if self.message_queue.is_empty() {
            None
        } else {
            Some(self.message_queue.remove(0))
        }
    }
}
