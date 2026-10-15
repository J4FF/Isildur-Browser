//! ES-Module (J10): `import`/`export`, `<script type="module">`.
//!
//! GEMESSEN, warum das die groesste offene Luecke war: in J6 liefen 10
//! echte Seiten an, 13 Restfehler — **7 davon waren Module**, 6 mit
//! `type="module"`. Die Oberflaeche von github.com besteht aus Modulen;
//! ohne diesen Weg zeigt der Browser dort eine leere Seite.
//!
//! # AUFBAU: EIN PROGRAMM, DESUGARTE MODULE
//!
//! GEMESSEN/ENTSCHIEDEN, warum so und nicht mit einem Programm je Modul:
//! ein Funktionswert traegt nur `fn_index` — den Index in die
//! Funktions-Tabelle des **gerade geladenen** Programms
//! (`Engine::aufruf_kopf` liest `self.programm`). Mit einem Programm je
//! Modul rief ein Import `hi('a')` den falschen Rumpf auf (GEMESSEN: `hi`
//! lieferte das Ergebnis von `plus`, `'a1'`). Also bekommen alle Module
//! **ein** gemeinsames Programm: der Loader stellt jedes Modul als eine
//! synthetische Funktion dar und entzuckert (`desugar`) Import/Export in
//! gewoehnliche Anweisungen:
//!
//! ```text
//! import { hi } from './lib.js';   ->  const hi = __jsvm_import_("…/lib.js", "hi");
//! import d from './z.js';          ->  const d  = __jsvm_import_("…/z.js", "default");
//! import * as ns from './m.js';    ->  const ns = __jsvm_import_alle("…/m.js");
//! export const NAME = 'app';       ->  … const NAME = 'app'; __jsvm_export_("main", "NAME", NAME);
//! export * from './m.js';          ->  __jsvm_export_alle_("…/m.js");
//! ```
//!
//! Die vier `__jsvm_…`-Helfer sind **Natives des Motors** (kein globales
//! Objekt-Feld — sie stehen nur in der globalen *Umgebung*, damit
//! `Object.keys(globalThis)` sie nicht zeigt). Sie lesen und schreiben die
//! Exporttabelle des Laders zur Laufzeit, deshalb braucht es keine
//! Bindungsphase: der Import laeuft, wenn der Rumpf laeuft, und die
//! Reihenfolge (Abhaengigkeiten zuerst) haelt der Aufrufer.
//!
//! # WIE DER HOST MITMACHT (und warum)
//!
//! Ein Modul laedt andere Module ueber Netz — und **Netz gehoert dem
//! Host** (der Browser hat keinen Socket, der Motor schon gar nicht). Also
//! laeuft der Lader in drei Schritten, die der Host nacheinander ruft:
//!
//! ```text
//! engine.module_starten(url, quelle)      -> Ok(())          // Startmodul uebergeben
//! for fehlend in engine.module_offene() {                     // der Host holt jede Quelle
//!     engine.module_quelle_setzen(fehlend, text_vom_http)      //   (sein eigener Client, sein Cache)
//! }
//! engine.module_laden()                   -> Ok(fehlerliste)  // alles ausfuehren, binden
//! ```
//!
//! GEMESSEN/ENTSCHIEDEN, warum nicht `fetch` im Motor: `fetch` legt eine
//! Anfrage in die Warteschlange und der Host beantwortet sie
//! **asynchron** (`netz_abarbeiten` -> `netz_ergebnis` -> `netz_schleife`).
//! Ein Import muss aber **vor** dem ersten Befehl des importierenden
//! Moduls fertig sein — sonst laeuft der Rumpf mit leeren Bindungen. Die
//! Warteschlange von Hand zu treiben wuerde jeden Importpfad in ein
//! eigenes Coroutine-Rennen verwandeln; der Host liefert Quellen, der
//! Motor verknuepft sie.
//!
//! # WAS DER LADER KANN (und was nicht)
//!
//! * `import d from 'm'`, `import { a, b as c } from 'm'`,
//!   `import * as ns from 'm'`, `import 'm'`, und alle Mischformen.
//! * `export const/let/var`, `export function`, `export class`,
//!   `export default` (Ausdruck, Funktion, Klasse),
//!   `export { a, b as c }`, `export { a as b } from 'm'`,
//!   `export * from 'm'`, `export * as ns from 'm'`.
//! * Reihenfolge: Abhaengigkeiten **zuerst** (Tiefensuche), jedes Modul
//!   genau einmal (Cache = die Modul-URL), ein Fehler in einem Modul
//!   nimmt den anderen nichts weg (jeder Rumpf laeuft in `try`/`catch`,
//!   die Meldung steht in der Konsole des Hosts).
//! * Jeder Modulrumpf bekommt eine eigene Umgebung; der Rest loest gegen
//!   den globalen Bereich auf (`console`, `document` …).
//!
//! # BEKANNTE GRENZEN (bewusst offen, im Bericht benannt)
//!
//! * **Keine lebenden Bindungen**: der Wert einer Bindung wird kopiert.
//!   `export let n = 0` + `n++` im importierenden Modul ist fuer den
//!   Exportierenden nicht sichtbar (in node schon). Ein echter Fix braucht
//!   Zellen statt Kopien (jede Bindung eine Box) — das fasst jede
//!   Zuweisung im Motor an.
//! * **Kein dynamisches `import()`** und **kein Top-Level-`await`**: der
//!   Parser sagt beides ausdruecklich, statt eine andere Fehlermeldung zu
//!   geben.
//! * **Keine `import.meta`**, keine Import-Attribute (`with { type: 'json' }`).
//! * **Kein Strict-Modus fuer Module** (die zwei test262-Luecken
//!   strict/sloppy sind offen).

use std::collections::HashMap;

use crate::ast::{Anweisung, Ausdruck, DeklArt, FunktionsBau, ImportArt, Muster, Programm};
use crate::fehler::JsFehler;
use crate::heap::GVal;
use crate::interp::Stoerung;
use crate::wert::Wert;

/// Jede Quelle einer Importkette wird hoechstens einmal geholt — die Grenze
/// haelt eine Endloskette (`a` importiert `b`, `b` importiert `a` …) im
/// Rahmen, und die Herkunftsgrenze des Hosts die Groesse.
pub(crate) const MAX_MODULE: usize = 512;

/// Der interne Name, unter dem der desugarte Import laeuft.
pub(crate) const N_IMPORT: &str = "__jsvm_import__";
/// … fuer `import * as ns`.
pub(crate) const N_ALLE: &str = "__jsvm_import_alle__";
/// … fuer `export` (Wert melden).
pub(crate) const N_EXPORT: &str = "__jsvm_export__";
/// … fuer `export * from`.
pub(crate) const N_EXPORT_ALLE: &str = "__jsvm_export_alle__";
/// J12: der **dynamische** Import `import('…')` — ein Motors-Nativ, weil
/// er ein Versprechen zurueckgibt und den Lader anmeldet.
pub(crate) const N_IMPORT_DYN: &str = "__jsvm_import_dyn__";
/// … fuer die Fehlermeldung eines Modulrumpfs.
pub(crate) const N_MELDUNG: &str = "__jsvm_meldung__";

/// Der Zustand eines Moduls im Lader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zustand {
    /// Quelle ist da, noch nicht ausgefuehrt.
    Geparst,
    /// Ausgefuehrt, Exporte liegen vor.
    Fertig,
}

/// Ein Modul im Lader.
pub(crate) struct Eintrag {
    ast: Programm,
    zustand: Zustand,
    /// J16: Laenge der hinterlegten Quelle (0 = noch keine).
    /// GEMESSEN/ENTSCHIEDEN, warum das noetig war: ein Host, der erst
    /// `module_starten(url, "")` und dann `module_quelle_setzen(url, q)`
    /// aufruft, bekam sonst **stillschweigend nichts** — `module_eintragen`
    /// liess den Cache gewinnen, das Modul blieb leer, `module_laden`
    /// lief ohne Fehler, und der Host glaubte, die Seite laeuft. Genau das
    /// ist beim End-to-End-Lauf mit einem echten github-Bündel passiert
    /// (7 KiB rspack-Code, 0 Fehler, 0 Konsole, 0 Importe — nichts davon
    /// war echt gelaufen). Mit der Laenge darf eine **leere** Quelle
    /// spaeter gefuellt werden; eine laufende nie (der Cache gewinnt,
    /// sonst laedt ein zweiter `import` derselben Datei noch einmal).
    quelle_len: usize,
    /// Die Exporte: externer Name -> Wert (nach dem Lauf).
    export: Vec<(String, GVal)>,
    /// Release 10.10.2026: DAS Namensraum-Objekt dieses Moduls (`import * as
    /// ns`) — einmal angelegt, danach von jeder Exportmeldung
    /// nachgefuehrt (siehe `n_modul_export`). Vorher baute jeder Import
    /// eine Momentaufnahme.
    namensraum: Option<crate::heap::HeapRef>,
    /// Quellen, die dieses Modul braucht (aufgeloeste URLs, ohne sich
    /// selbst).
    pub(crate) braucht: Vec<String>,
}

/// Der Lader: alle bekannten Module.
pub(crate) struct ModulLader {
    module: HashMap<String, Eintrag>,
    /// Die Reihenfolge des Hinzufuegens (fuer stabile Meldungen).
    reihenfolge: Vec<String>,
    /// J12: wartende dynamische Importe — `(aufgeloeste URL, Versprechen)`,
    /// in der Reihenfolge des Eintrags. Sie werden erfuellt, sobald das
    /// Modul geladen ist (`module_laden`).
    wartend: Vec<(String, crate::heap::HeapRef)>,
    /// Release 10.10.2026: die Import-Map der Seite (siehe `ImportKarte`).
    pub(crate) karte: ImportKarte,
    /// Release 12.10.2026: Adressen, deren Quelle der Host nicht bekam (je
    /// Engine, also je Seite; vorher hielt der Host sie threadweit).
    gescheitert: std::collections::HashSet<String>,
}

impl ModulLader {
    pub(crate) fn neu() -> Self {
        ModulLader {
            module: HashMap::new(),
            reihenfolge: Vec::new(),
            wartend: Vec::new(),
            karte: ImportKarte::default(),
            gescheitert: std::collections::HashSet::new(),
        }
    }

    pub(crate) fn anzahl(&self) -> usize {
        self.module.len()
    }

    /// Die URLs, deren Quelle dem Host noch fehlt (sortiert, ohne
    /// Doppelungen).
    ///
    /// GEMESSEN, warum die Liste aus den **Abhaengigkeiten** kommt und
    /// nicht aus der Reihenfolge des Hinzufuegens: eine abhaengige URL
    /// steht nie in `reihenfolge` (der Host hat sie noch nicht
    /// geliefert) — mit der Reihenfolge lieferte die erste Messung `[]`
    /// zurueck, obwohl zwei Importe offen waren.
    pub(crate) fn offene(&self) -> Vec<String> {
        let mut r: Vec<String> = Vec::new();
        for e in self.module.values() {
            for u in &e.braucht {
                if !self.module.contains_key(u) && !r.contains(u) {
                    r.push(u.clone());
                }
            }
        }
        // J12: ein dynamischer Import (`import('./x.js')`) ist noch gar
        // nicht im Lader — der Host muss seine Quelle holen, sonst bleibt
        // das Versprechen offen.
        for (u, _) in &self.wartend {
            if !self.module.contains_key(u) && !r.contains(u) {
                r.push(u.clone());
            }
        }
        r.sort();
        r
    }

    pub(crate) fn eintrag(&self, url: &str) -> Option<&Eintrag> {
        self.module.get(url)
    }
}

// ---------------------------------------------------------------------------
// URL-Aufloesung (Module brauchen echte URLs als Schluessel)
// ---------------------------------------------------------------------------

/// `./b.js`, `../lib/x.js`, `/a.js`, `b.js`, `//host/x.js` und absolute
/// URLs gegen die URL des importierenden Moduls aufloesen.
///
/// GEMESSEN, warum das hier und nicht im Host: der Cache-Schluessel ist die
/// aufgeloeste URL — zwei Schreibweisen derselben Datei muessen **ein**
/// Modul sein (`./a.js` aus `/x/` und `a.js` aus `/x/`). Wer die Aufloesung
/// dem Host ueberlaesst, bekommt genau das nicht (der Host sieht die
/// Anweisung ja gar nicht mehr).
pub(crate) fn url_auflosen(basis: &str, ziel: &str) -> String {
    let ziel = ziel.trim();
    // E (06.10.2026, Masterplan): DER GEMEINSAME PARSER.
    //
    // GEMESSEN an der alten Fassung: `import './x.js?v=1'` wurde zu
    // `.../x.js` (die Anfrage fiel weg — zwei verschiedene Module wurden
    // EIN Cache-Eintrag), und ein Ziel ohne Schraegstrich (`import 'b'`)
    // wurde zur Wurzel statt zum Nachbarn. `narsil_url::loese_auf` macht die
    // RFC-3986-Aufloesung (Verzeichnis, `.`/`..`, Query, Fragment) und
    // wird auch von `fetch`, der Adressleiste und Formularen benutzt —
    // eine Wahrheit ueber URLs.
    // An absolute URL is parsed as well: the module map key is the
    // serialized URL (a space in the query becomes %20). MEASURED before:
    // `import "http://h/m.js?pipe=header(A, *)"` stayed "missing" — the
    // fetched module was entered under another spelling (WPT
    // workers/modules/*-import-data-url*).
    if ziel.contains("://") {
        return narsil_url::loese_auf(ziel, ziel).unwrap_or_else(|_| ziel.to_string());
    }
    if !basis.contains("://") {
        return ziel.to_string();
    }
    narsil_url::loese_auf(basis, ziel).unwrap_or_else(|_| ziel.to_string())
}

// ---------------------------------------------------------------------------
// Release 10.10.2026: Import-Maps (HTML §8.1.5.3, „resolve a module
// specifier")
// ---------------------------------------------------------------------------

/// `<script type="importmap">{"imports": {…}, "scopes": {…}}</script>`.
///
/// GEMESSEN (github.com): die Seite importiert `react`, `react-dom`,
/// `react/jsx-runtime` … als NACKTE Namen, die Karte bildet sie auf
/// `https://github.githubassets.com/assets/react-<hash>.js` ab. Ohne Karte
/// wurde `react` neben dem Modul gesucht (`…/assets/react`), der Server
/// lieferte seine HTML-Fehlerseite, und sieben Module scheiterten mit
/// „unerwartetes Zeichen '@'" — die ganze React-Oberflaeche blieb aus.
///
/// Schluessel und Werte sind schon normalisiert (gegen die Adresse der
/// Karte aufgeloest) und absteigend sortiert — so kommt der laengste
/// Praefix zuerst, wie es die Norm verlangt.
#[derive(Default, Clone, Debug)]
pub(crate) struct ImportKarte {
    imports: Vec<(String, String)>,
    /// `(Bereichspraefix, Eintraege)`, der genaueste Bereich zuerst.
    scopes: Vec<(String, Vec<(String, String)>)>,
}

/// URL-artig im Sinne der Norm: `/`, `./`, `../` oder eine absolute URL
/// (Schema + `:`). Alles andere ist ein NACKTER Name (`react`).
fn url_artig(s: &str) -> bool {
    if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") {
        return true;
    }
    match s.find(':') {
        Some(k) if k > 0 => {
            let schema = &s[..k];
            schema.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && schema
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        _ => false,
    }
}

fn karten_treffer(liste: &[(String, String)], normal: &str) -> Option<String> {
    for (k, v) in liste {
        if k == normal {
            return Some(v.clone());
        }
        // Praefix-Eintrag: `"lib/": "https://cdn/lib/"` — der Rest wird
        // gegen den Wert aufgeloest (der Wert MUSS dann auf `/` enden).
        if k.ends_with('/') && normal.starts_with(k.as_str()) && v.ends_with('/') {
            return Some(url_auflosen(v, &normal[k.len()..]));
        }
    }
    None
}

impl ImportKarte {
    pub(crate) fn ist_leer(&self) -> bool {
        self.imports.is_empty() && self.scopes.is_empty()
    }

    /// „Resolve a module specifier": erst die Bereiche, die auf die
    /// Adresse des importierenden Moduls passen, dann `imports`, sonst die
    /// gewoehnliche URL-Aufloesung (wie bisher, auch fuer nackte Namen).
    pub(crate) fn aufloesen(&self, basis: &str, ziel: &str) -> String {
        let ziel = ziel.trim();
        if self.ist_leer() {
            return url_auflosen(basis, ziel);
        }
        let als_url = url_artig(ziel).then(|| url_auflosen(basis, ziel));
        let normal = als_url.clone().unwrap_or_else(|| ziel.to_string());
        for (praefix, liste) in &self.scopes {
            let passt = praefix == basis || (praefix.ends_with('/') && basis.starts_with(praefix.as_str()));
            if passt && let Some(u) = karten_treffer(liste, &normal) {
                return u;
            }
        }
        if let Some(u) = karten_treffer(&self.imports, &normal) {
            return u;
        }
        als_url.unwrap_or_else(|| url_auflosen(basis, ziel))
    }

    /// Eine weitere Karte dazunehmen (HTML „merge existing and new import
    /// maps"): schon bekannte Schluessel gewinnen.
    fn dazu(&mut self, neu: ImportKarte) {
        for (k, v) in neu.imports {
            if !self.imports.iter().any(|(a, _)| *a == k) {
                self.imports.push((k, v));
            }
        }
        for (p, liste) in neu.scopes {
            match self.scopes.iter_mut().find(|(a, _)| *a == p) {
                Some((_, alt)) => {
                    for (k, v) in liste {
                        if !alt.iter().any(|(a, _)| *a == k) {
                            alt.push((k, v));
                        }
                    }
                    alt.sort_by(|a, b| b.0.cmp(&a.0));
                }
                None => self.scopes.push((p, liste)),
            }
        }
        self.imports.sort_by(|a, b| b.0.cmp(&a.0));
        self.scopes.sort_by(|a, b| b.0.cmp(&a.0));
    }
}

/// Ein JSON-Wert, so weit die Karte ihn braucht.
enum KartenJson {
    Objekt(Vec<(String, KartenJson)>),
    Text(String),
    Sonst,
}

/// Kleiner JSON-Leser fuer die Karte (RFC 8259; Zahlen/Felder/Literale
/// werden gelesen und verworfen). Gedeckelt: Tiefe 32.
fn karten_json(z: &[char], i: &mut usize, tiefe: u32) -> Result<KartenJson, String> {
    fn leer(z: &[char], i: &mut usize) {
        while *i < z.len() && matches!(z[*i], ' ' | '\t' | '\n' | '\r') {
            *i += 1;
        }
    }
    fn text(z: &[char], i: &mut usize) -> Result<String, String> {
        *i += 1;
        let mut s = String::new();
        while *i < z.len() {
            let c = z[*i];
            *i += 1;
            match c {
                '"' => return Ok(s),
                '\\' => {
                    let e = *z.get(*i).ok_or("Escape am Ende")?;
                    *i += 1;
                    match e {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        'r' => s.push('\r'),
                        'b' => s.push('\u{8}'),
                        'f' => s.push('\u{c}'),
                        'u' => {
                            let h: String = z.get(*i..*i + 4).ok_or("\\u am Ende")?.iter().collect();
                            *i += 4;
                            let v = u32::from_str_radix(&h, 16).map_err(|_| "ungueltiges \\u")?;
                            s.push(char::from_u32(v).unwrap_or('\u{FFFD}'));
                        }
                        c => s.push(c),
                    }
                }
                c => s.push(c),
            }
        }
        Err("Zeichenkette nicht geschlossen".to_string())
    }
    if tiefe > 32 {
        return Err("zu tief verschachtelt".to_string());
    }
    leer(z, i);
    match z.get(*i).copied() {
        Some('{') => {
            *i += 1;
            let mut paare = Vec::new();
            loop {
                leer(z, i);
                match z.get(*i).copied() {
                    Some('}') => {
                        *i += 1;
                        return Ok(KartenJson::Objekt(paare));
                    }
                    Some(',') if !paare.is_empty() => *i += 1,
                    Some('"') => {
                        let k = text(z, i)?;
                        leer(z, i);
                        if z.get(*i) != Some(&':') {
                            return Err("':' erwartet".to_string());
                        }
                        *i += 1;
                        let v = karten_json(z, i, tiefe + 1)?;
                        paare.push((k, v));
                    }
                    _ => return Err("Objekt erwartet".to_string()),
                }
            }
        }
        Some('[') => {
            *i += 1;
            loop {
                leer(z, i);
                match z.get(*i).copied() {
                    Some(']') => {
                        *i += 1;
                        return Ok(KartenJson::Sonst);
                    }
                    Some(',') => *i += 1,
                    Some(_) => {
                        karten_json(z, i, tiefe + 1)?;
                    }
                    None => return Err("Feld nicht geschlossen".to_string()),
                }
            }
        }
        Some('"') => Ok(KartenJson::Text(text(z, i)?)),
        Some(c) if c == '-' || c.is_ascii_alphanumeric() => {
            while *i < z.len() && (z[*i].is_ascii_alphanumeric() || matches!(z[*i], '-' | '+' | '.')) {
                *i += 1;
            }
            Ok(KartenJson::Sonst)
        }
        _ => Err("Wert erwartet".to_string()),
    }
}

/// „Sort and normalize a module specifier map": URL-artige Schluessel und
/// alle Werte gegen `basis`; ein Wert, der keine URL ist, faellt weg.
fn karten_liste(paare: Vec<(String, KartenJson)>, basis: &str) -> Vec<(String, String)> {
    let mut r: Vec<(String, String)> = Vec::new();
    for (k, v) in paare {
        let KartenJson::Text(v) = v else { continue };
        if k.is_empty() || !url_artig(v.trim()) {
            continue;
        }
        let k = if url_artig(&k) { url_auflosen(basis, &k) } else { k };
        let v = url_auflosen(basis, v.trim());
        if !r.iter().any(|(a, _)| *a == k) {
            r.push((k, v));
        }
    }
    r.sort_by(|a, b| b.0.cmp(&a.0));
    r
}

fn karte_lesen(json: &str, basis: &str) -> Result<ImportKarte, String> {
    let z: Vec<char> = json.chars().collect();
    let mut i = 0usize;
    let KartenJson::Objekt(oben) = karten_json(&z, &mut i, 0)? else {
        return Err("die Import-Map ist kein Objekt".to_string());
    };
    let mut karte = ImportKarte::default();
    for (k, v) in oben {
        match (k.as_str(), v) {
            ("imports", KartenJson::Objekt(p)) => karte.imports = karten_liste(p, basis),
            ("scopes", KartenJson::Objekt(bereiche)) => {
                for (praefix, liste) in bereiche {
                    if let KartenJson::Objekt(p) = liste {
                        karte.scopes.push((url_auflosen(basis, &praefix), karten_liste(p, basis)));
                    }
                }
            }
            _ => {}
        }
    }
    karte.scopes.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(karte)
}

// ---------------------------------------------------------------------------
// Entzuckerung: aus einem Modul-Quelltext wird eine synthetische Funktion
// ---------------------------------------------------------------------------

/// Der Import-Aufruf als AST: `__jsvm_import_("url", "name")`.
fn ruf_import(url: &str, name: &str) -> Ausdruck {
    aufruf(
        Ausdruck::Name(N_IMPORT.to_string()),
        vec![
            Ausdruck::Text(url.to_string()),
            Ausdruck::Text(name.to_string()),
        ],
    )
}

/// `__jsvm_import_alle_("url")`.
fn ruf_import_alle(url: &str) -> Ausdruck {
    aufruf(
        Ausdruck::Name(N_ALLE.to_string()),
        vec![Ausdruck::Text(url.to_string())],
    )
}

/// `__jsvm_export_("url", "name", wert)`.
fn ruf_export(url: &str, name: &str, wert: Ausdruck) -> Ausdruck {
    aufruf(
        Ausdruck::Name(N_EXPORT.to_string()),
        vec![
            Ausdruck::Text(url.to_string()),
            Ausdruck::Text(name.to_string()),
            wert,
        ],
    )
}

/// `__jsvm_export_alle_("ziel", "quelle")`.
///
/// GEMESSEN, warum **zwei** URLs: die erste Messung hatte eine — damit
/// landeten die Exporte des Quellmoduls wieder im Quellmodul
/// (`export * from './a.js'` in `main.js` lieferte `undefined`, `D` aus
/// `main.js` selbst kam an). Das Ziel ist das Modul, dessen Exporte
/// wachsen; die Quelle das, von dem sie kopiert werden.
fn ruf_export_alle(ziel: &str, quelle: &str) -> Ausdruck {
    aufruf(
        Ausdruck::Name(N_EXPORT_ALLE.to_string()),
        vec![
            Ausdruck::Text(ziel.to_string()),
            Ausdruck::Text(quelle.to_string()),
        ],
    )
}

/// `__jsvm_meldung_("url", e)`.
fn ruf_meldung(url: &str, e: Ausdruck) -> Ausdruck {
    aufruf(
        Ausdruck::Name(N_MELDUNG.to_string()),
        vec![Ausdruck::Text(url.to_string()), e],
    )
}

/// `ziel(args…)` als `Anweisung::Ausdruck`.
///
/// GEMESSEN/ENTSCHIEDEN, warum hier kein zweites `umschliessen` passiert:
/// die `ruf_*`-Helfer liefern **fertige** `Ausdruck::Aufruf`-Knoten.
/// Ein Aufruf um einen fertigen Aufruf ergibt `(f())()` — und das ist
/// genau die Meldung „Aufrufziel ist keine Funktion", die die erste
/// Messung des Laders brachte (`Export` A war da, der zweite Export
/// fehlte, und danach kam der Fehler).
fn aufruf_anweisung(ziel: Ausdruck, _args: Vec<Ausdruck>) -> Anweisung {
    Anweisung::Ausdruck(ziel)
}

/// `ziel(args…)` als `Anweisung::Ausdruck` (frisch gebauter Aufruf).
fn aufruf_anweisung_neu(ziel: Ausdruck, args: Vec<Ausdruck>) -> Anweisung {
    Anweisung::Ausdruck(aufruf(ziel, args))
}

/// `ziel(args…)` als `Ausdruck::Aufruf`.
fn aufruf(ziel: Ausdruck, args: Vec<Ausdruck>) -> Ausdruck {
    let n = args.len();
    Ausdruck::Aufruf {
        ziel: Box::new(ziel),
        args,
        streu: vec![false; n],
        optional: false,
    }
}

/// Einen `import(…)`-Aufruf auf oberster Ebene um die Modul-URL ergaenzen.
/// Der Name der Bindung, in der ein Modulrumpf seine **eigene** URL
/// hinterlegt (J12). Ein dynamischer Import darin loest sich dagegen auf.
///
/// GEMESSEN/ENTSCHIEDEN, warum eine Bindung und kein Umschreiben des
/// ASTs: der erste Versuch haengte die Modul-URL als zweites Argument an
/// jeden `import(…)`-Aufruf — aber nur an die auf **oberster Ebene**
/// (`Anweisung::Ausdruck(Ausdruck::Aufruf)`). GEMESSEN:
/// `import('./x.js').then(ns => …)` loeste sich deshalb gegen die
/// Dokumentadresse auf (`/spaet.js` statt `/app/unter/spaet.js`). Eine
/// Bindung im Modul-Scope deckt **jede** Verschachtelung ab, weil der
/// Native ueber den aktuellen Rahmen dorthin sieht.
pub(crate) const N_MODUL_URL: &str = "__jsvm_modul_url__";

/// J15: die Bindung hinter `import.meta` — ein Objekt mit `url` (die URL
/// des Moduls). `import.meta.url` ist in node die Modul-Adresse; fuer den
/// Browser ist die Modul-URL genau das Richtige (Asset-URLs, dynamische
/// Importe, Web-Worker aus Bündler-Output lesen sie so).
/// `import.meta.env` (Vite) und `import.meta.hot` (HMR) bleiben
/// `undefined` — sie sind Build-Konventionen, keine Sprache.
pub(crate) const N_IMPORT_META: &str = "__jsvm_import_meta__";

/// Release 11.10.2026: die Marke, mit der der Host Inline-Module
/// unterscheidet (jedes ist ein eigenes Modul, keines steht im Cache des
/// anderen). GEMESSEN vorher: alle Inline-Module einer Seite hatten die
/// Seitenadresse als Schluessel — ab dem zweiten lief keines mehr (WPT
/// `module/type.html` u. a.).
pub const INLINE_MODUL_MARKE: &str = "#narsil-inline-";

/// Release 11.10.2026: die Marke fuer den Modultyp aus den Import-
/// Attributen (`…/m.json#narsil-typ=json`). Der Host holt die Adresse ohne
/// Marke und prueft den MIME-Typ passend zum Typ.
pub const MODUL_TYP_MARKE: &str = "#narsil-typ=";

/// Ein JSON-/CSS-/Text-Modul als Quelltext eines synthetischen Moduls
/// (HTML §8.1.5.4 „create a JSON module script", „… CSS module script").
fn synthetisches_modul(typ: &str, quelle: &str) -> Option<String> {
    let mut text = String::with_capacity(quelle.len() + 16);
    text.push('"');
    for ch in quelle.chars() {
        match ch {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\u{2028}' => text.push_str("\\u2028"),
            '\u{2029}' => text.push_str("\\u2029"),
            c if (c as u32) < 0x20 => text.push_str(&format!("\\u{:04x}", c as u32)),
            c => text.push(c),
        }
    }
    text.push('"');
    Some(match typ {
        "json" => format!("export default JSON.parse({text});"),
        "css" => format!("const __narsil_blatt = new CSSStyleSheet(); __narsil_blatt.replaceSync({text}); export default __narsil_blatt;"),
        "text" => format!("export default {text};"),
        _ => return None,
    })
}

/// `const name = wert;`
fn konst(name: &str, wert: Ausdruck) -> Anweisung {
    Anweisung::Deklaration {
        art: crate::ast::DeklArt::Const,
        eintraege: vec![(Muster::Name(name.to_string()), Some(wert))],
    }
}

/// Die Anweisungen **eines** Moduls: Importe als `const`, der Rumpf wie
/// er war, am Ende die Export-Meldungen.
fn desugar(url: &str, ast: &Programm, karte: &ImportKarte) -> Vec<Anweisung> {
    let mut r: Vec<Anweisung> = Vec::new();
    let Some(m) = ast.modul.as_ref() else {
        return ast.anweisungen.clone();
    };
    // 1. Importe -> Bindungen mit einem Aufruf als Wert.
    for i in &m.import {
        let q = karte.aufloesen(url, &i.quelle);
        match &i.art {
            ImportArt::Leer => {}
            ImportArt::Vorgabe { name } => r.push(konst(name, ruf_import(&q, "default"))),
            ImportArt::Liste { paare } => {
                for (extern_, intern) in paare {
                    r.push(konst(intern, ruf_import(&q, extern_)));
                }
            }
            ImportArt::Alles { name } => r.push(konst(name, ruf_import_alle(&q))),
            ImportArt::Gemischt {
                vorgabe,
                paare,
                alles,
            } => {
                if let Some(n) = vorgabe {
                    r.push(konst(n, ruf_import(&q, "default")));
                }
                for (extern_, intern) in paare {
                    r.push(konst(intern, ruf_import(&q, extern_)));
                }
                if let Some(n) = alles {
                    r.push(konst(n, ruf_import_alle(&q)));
                }
            }
        }
    }
    // 2. Die eigene URL als Bindung (J12) — danach der Rumpf, wie er
    //    war. Die Bindung liegt im Scope des Modulrumpfs; ein dynamischer
    //    Import darin liest sie (siehe `N_MODUL_URL`).
    r.push(konst(N_MODUL_URL, Ausdruck::Text(url.to_string())));
    // J15: `import.meta` = `{ url: <Modul-URL> }` (siehe `N_IMPORT_META`).
    // Release 11.10.2026: ein Inline-Modul traegt im Lader eine eigene
    // Kennung (`#narsil-inline-…`); seine `import.meta.url` ist die Adresse
    // des Dokuments (HTML §8.1.3.2 „module script base URL").
    let meta_url = match url.find(INLINE_MODUL_MARKE) {
        Some(i) => &url[..i],
        None => url,
    };
    r.push(konst(
        N_IMPORT_META,
        Ausdruck::Gegenstand(vec![crate::ast::GegenstandTeil::Paar(
            crate::ast::EigenschaftName::Text("url".to_string()),
            Box::new(Ausdruck::Text(meta_url.to_string())),
        )]),
    ));
    // Release 10.10.2026: eigene Exporte FRUEH melden — Funktionen
    // (gehoben) gleich am Anfang, alles andere direkt nach der Anweisung,
    // die den Namen bindet. Am Ende wird wie bisher noch einmal gemeldet
    // (der Endwert gewinnt). GEMESSEN (github.com, rspack): jedes Buendel
    // importiert SICH SELBST (`import * as U from "./landing-pages-….js";
    // r.C(U)` als letzte Anweisung) und liest `U.__rspack_esm_ids` — mit
    // Meldung erst nach dem Rumpf war der Namensraum leer: „kein Zugriff
    // auf Undefiniert (Eigenschaft length)" in sechs Modulen, keine
    // React-Oberflaeche.
    let mut eigene: HashMap<&str, Vec<&str>> = HashMap::new();
    for x in &m.export {
        if x.quelle.is_none() {
            eigene.entry(x.intern.as_str()).or_default().push(x.extern_.as_str());
        }
    }
    let melde = |r: &mut Vec<Anweisung>, name: &str| {
        if let Some(aussen) = eigene.get(name) {
            for a in aussen {
                r.push(aufruf_anweisung(
                    ruf_export(url, a, Ausdruck::Name(name.to_string())),
                    vec![],
                ));
            }
        }
    };
    if !eigene.is_empty() {
        for a in &ast.anweisungen {
            if let Anweisung::Funktion { name: Some(n), .. } = a.inhalt() {
                melde(&mut r, n);
            }
        }
    }
    for a in &ast.anweisungen {
        r.push(a.clone());
        if eigene.is_empty() {
            continue;
        }
        match a.inhalt() {
            Anweisung::Deklaration { eintraege, .. } => {
                for (muster, _) in eintraege {
                    for n in crate::compile::muster_namen(muster) {
                        melde(&mut r, &n);
                    }
                }
            }
            Anweisung::Klasse { name: Some(n), .. } => melde(&mut r, n),
            _ => {}
        }
    }
    // 3. Exporte melden.
    for x in &m.export {
        // Jede Form liefert einen **fertigen** Aufruf (siehe
        // `aufruf_anweisung`).
        let anweisung = match (&x.quelle, x.intern.as_str()) {
            // `export * from 'm'`: alle Exporte uebernehmen.
            (Some(q), "*") if x.extern_ == "*" => {
                aufruf_anweisung(ruf_export_alle(url, &karte.aufloesen(url, q)), vec![])
            }
            // Release 10.10.2026: `export * as ns from 'm'` ist EIN Name —
            // der Namensraum von `m`. Vorher wurden alle Exporte flach
            // uebernommen und `ns` fehlte.
            (Some(q), "*") => aufruf_anweisung(
                ruf_export(url, &x.extern_, ruf_import_alle(&karte.aufloesen(url, q))),
                vec![],
            ),
            // `export { a as b } from 'm'`: den Wert im Quellmodul holen.
            (Some(q), _) => {
                let qq = karte.aufloesen(url, q);
                aufruf_anweisung(
                    ruf_export(url, &x.extern_, ruf_import(&qq, &x.intern)),
                    vec![],
                )
            }
            // Eigener Wert aus der Modul-Umgebung.
            (None, _) => aufruf_anweisung(
                ruf_export(url, &x.extern_, Ausdruck::Name(x.intern.clone())),
                vec![],
            ),
        };
        r.push(anweisung);
    }
    r
}

// ---------------------------------------------------------------------------
// Engine: oeffentliche API
// ---------------------------------------------------------------------------

impl crate::Engine {
    /// J10: ein Modul **anmelden** (Quelle uebergeben). Der Importgraph
    /// wird daraufhin aufgebaut; fehlende Quellen nennt
    /// [`Engine::module_offene`].
    pub fn module_starten(&mut self, url: &str, quelle: &str) -> Result<(), JsFehler> {
        let url = self.module_url(url);
        self.module_eintragen(url, quelle)
    }

    /// Release 10.10.2026: eine Import-Map der Seite eintragen
    /// (`<script type="importmap">`, HTML §8.1.5.3). `basis` ist die
    /// Adresse des Dokuments. Mehrere Karten werden zusammengefuegt;
    /// schon bekannte Schluessel gewinnen (siehe `ImportKarte`).
    pub fn importmap_setzen(&mut self, json: &str, basis: &str) -> Result<(), String> {
        // 512 KiB (frueher `MAX_NETZ_URL * 64`, unabhaengig von der URL-Grenze).
        if json.len() > 512 * 1024 {
            return Err("Import-Map zu gross".to_string());
        }
        let neu = karte_lesen(json, basis)?;
        self.module.karte.dazu(neu);
        Ok(())
    }

    /// J10: die Quelle eines **Folgemoduls** nachreichen (der Host hat sie
    /// geholt). Gleiche Form wie [`Engine::module_starten`].
    pub fn module_quelle_setzen(&mut self, url: &str, quelle: &str) -> Result<(), JsFehler> {
        let url = self.module_url(url);
        self.module_eintragen(url, quelle)
    }

    /// Absolute URL fuer den Lader (Host-URL, sonst die Herkunft als
    /// Basis).
    fn module_url(&self, url: &str) -> String {
        if url.contains("://") {
            url.to_string()
        } else {
            let basis = self.herkunft_text().unwrap_or_default();
            url_auflosen(&basis, url)
        }
    }

    fn module_eintragen(&mut self, url: String, quelle: &str) -> Result<(), JsFehler> {
        // Release 11.10.2026: JSON/CSS/Text aus Import-Attributen.
        let synthetisch;
        let quelle = match url.find(MODUL_TYP_MARKE) {
            Some(i) => {
                let typ = &url[i + MODUL_TYP_MARKE.len()..];
                synthetisch = synthetisches_modul(typ, quelle).ok_or_else(|| JsFehler::Syntax {
                    zeile: 1,
                    spalte: 1,
                    nachricht: format!("Modultyp '{typ}' wird nicht unterstuetzt"),
                })?;
                synthetisch.as_str()
            }
            None => quelle,
        };
        // M4: Quelle und Tokenzahl sind gedeckelt (wie bei `ausfuehren`).
        if url.len() > crate::netz::MAX_NETZ_URL {
            return Err(JsFehler::Speicher("Modul-URL zu lang".to_string()));
        }
        if self.module.module.len() >= MAX_MODULE {
            return Err(JsFehler::Speicher(format!(
                "zu viele Module (Grenze {MAX_MODULE})"
            )));
        }
        // Schon bekannt: der Cache gewinnt (zweites `import` derselben
        // Datei laedt nicht noch einmal) — AUSSER der Eintrag ist leer
        // und noch nie gelaufen (J16, siehe `Eintrag::quelle_len`).
        if let Some(e) = self.module.module.get(&url) {
            let leer_und_unlaufen =
                e.quelle_len == 0 && e.zustand == crate::module::Zustand::Geparst;
            if !leer_und_unlaufen {
                return Ok(());
            }
            self.module.module.remove(&url);
        }
        let tokens = crate::lex::lexen_modul(
            quelle,
            self.grenzen.max_quelltext_bytes,
            self.grenzen.max_token,
        )?;
        let ast = crate::parse::parsen_als_modul_mit_quelle(
            tokens,
            self.grenzen.max_parser_tiefe,
            &quelle,
        )?;
        if ast.modul.is_none() {
            return Err(JsFehler::Syntax {
                zeile: 1,
                spalte: 1,
                nachricht: "Modul ohne import/export ist unmoeglich".to_string(),
            });
        }
        // Die Abhaengigkeiten gegen diese URL aufloesen.
        let mut braucht: Vec<String> = Vec::new();
        if let Some(m) = ast.modul.as_ref() {
            for q in m
                .import
                .iter()
                .map(|i| &i.quelle)
                .chain(m.export.iter().filter_map(|e| e.quelle.as_ref()))
            {
                let q = self.module.karte.aufloesen(&url, q);
                if q != url && !braucht.contains(&q) {
                    braucht.push(q);
                }
            }
        }
        self.module.module.insert(
            url.clone(),
            Eintrag {
                ast,
                zustand: Zustand::Geparst,
                quelle_len: quelle.len(),
                export: Vec::new(),
                namensraum: None,
                braucht,
            },
        );
        self.module.reihenfolge.push(url);
        Ok(())
    }

    /// J10: die Quellen, die dem Host noch fehlen (der Importgraph).
    pub fn module_offene(&self) -> Vec<String> {
        self.module.offene()
    }

    /// J10: wie viele Module der Lader kennt.
    pub fn module_anzahl(&self) -> usize {
        self.module.anzahl()
    }

    /// J10: die Namen, die ein Modul exportiert (nach dem Laden).
    pub fn module_namen(&self, url: &str) -> Vec<String> {
        match self.module.eintrag(url) {
            Some(e) => e.export.iter().map(|(n, _)| n.clone()).collect(),
            None => Vec::new(),
        }
    }

    /// J10: der Wert eines Exports (fuer Host und Tests).
    pub fn modul_wert(&mut self, url: &str, name: &str) -> Result<Wert, String> {
        let wert = self
            .module
            .eintrag(url)
            .and_then(|e| e.export.iter().find(|(n, _)| n == name).map(|(_, v)| *v))
            .ok_or_else(|| format!("Modul {url} exportiert '{name}' nicht"))?;
        self.gval_als_wert(wert)
            .ok_or_else(|| format!("Export '{name}' von {url} laesst sich nicht als Wert lesen"))
    }

    /// J10: **alles** ausfuehren und verknuepfen.
    ///
    /// Reihenfolge: Abhaengigkeiten zuerst (Tiefensuche), jedes Modul
    /// einmal. `Ok(fehler)` — die Liste der Module, deren Rumpf einen
    /// Fehler geworfen hat (der Lader laeuft weiter, wie im Browser: ein
    /// kaputtes Modul nimmt den anderen nichts weg; die Meldung steht auch
    /// in der Konsole). `Err` nur fuer strukturelle Probleme.
    pub fn module_laden(&mut self) -> Result<Vec<String>, String> {
        if self.module.module.is_empty() {
            return Ok(Vec::new());
        }
        let offen = self.module.offene();
        if !offen.is_empty() {
            return Err(format!(
                "Module fehlen noch ({}): {}",
                offen.len(),
                offen.join(", ")
            ));
        }
        let reihenfolge = self.module_reihenfolge();
        let karte = self.module.karte.clone();
        // Ein gemeinsames Programm: jede Modulquelle wird eine synthetische
        // Funktion, aufgerufen wird in Abhaengigkeitsreihenfolge — jeder
        // Rumpf in `try`/`catch`, damit ein Fehler die anderen nicht stoppt.
        let mut anweisungen: Vec<Anweisung> = Vec::new();
        for (i, url) in reihenfolge.iter().enumerate() {
            // Release 10.10.2026: ein FERTIGES Modul wird nicht noch einmal
            // uebersetzt. Vorher baute jeder Lauf ALLE Ruempfe neu (die
            // Funktion blieb fuer „stabile Indizes" — gebraucht werden sie
            // nur innerhalb DIESES Programms, Importe gehen ueber die
            // Exporttabelle). GEMESSEN (github.com mit Import-Map, ~20
            // Module, React): jeder Nachlade-Lauf uebersetzte den ganzen
            // Graphen wieder, 19 Laeufe scheiterten mit „Programm groesser
            // als das Heap-Limit (M4)", `scheduler` wurde nie geholt, LOAD
            // 99 s.
            let ast = match self.module.eintrag(url) {
                Some(e) if e.zustand == Zustand::Fertig => continue,
                Some(e) => e.ast.clone(),
                None => continue,
            };
            anweisungen.push(Anweisung::Deklaration {
                art: DeklArt::Let,
                eintraege: vec![(
                    Muster::Name(format!("__jsvm_modul_{i}__")),
                    Some(Ausdruck::Funktion {
                        name: None,
                        funktion: Box::new(FunktionsBau {
                            params: Vec::new(),
                            rest: None,
                            rumpf: desugar(url, &ast, &karte),
                            pfeil: false,
                            asynchron: false,
                            generator: false,
                            konstruktor: false,
                            quelle: None,
                            // J28: eine aus `desugar` erzeugte Funktion hat keine
                            // Quelltextzeile (0 = unbekannt, im Panel sichtbar).
                            zeile: 0,
                        }),
                    }),
                )],
            });
        }
        for (i, url) in reihenfolge.iter().enumerate() {
            // J12: ein Modul laeuft **einmal**. GEMESSEN/GEFUNDEN, warum
            // das vorher falsch war: `module_laden` baut alle Ruempe in ein
            // Programm und ruft **jeden** erneut auf — der Zustand wurde
            // nirgends geprueft. Im zweiten Lauf startete main.js also
            // noch einmal, sein `import('./spaet.js')` meldete ein
            // zweites Versprechen an (GEMESSEN: zwei wartende Versprechen
            // fuer dieselbe URL), und der erste `then`-Ruecklauf lief
            // nie. Die Funktion wird weiterhin gebaut (die Indizes bleiben
            // stabil), aber nur **neue** Module werden gerufen.
            if self.module.eintrag(url).map(|e| e.zustand) == Some(Zustand::Fertig) {
                continue;
            }
            anweisungen.push(Anweisung::Versuch {
                rumpf: vec![aufruf_anweisung_neu(
                    Ausdruck::Name(format!("__jsvm_modul_{i}__")),
                    vec![],
                )],
                fang_name: Some(Muster::Name("__jsvm_fehler__".to_string())),
                fang: vec![aufruf_anweisung(
                    ruf_meldung(url, Ausdruck::Name("__jsvm_fehler__".to_string())),
                    vec![],
                )],
                endlich: Vec::new(),
                hat_catch: true,
            });
        }
        let ast = Programm {
            anweisungen,
            // J14: Module sind laut Spec **immer strikt** (sec-15.2.1).
            // GEMESSEN/ENTSCHIEDEN, warum das jetzt stimmt und nicht schon
            // in J14: der Motor kannte nur eine Programmlage aus der
            // Direktive; ein Modul hat keine. Mit dem Rahmen-Kennzeichen
            // aus J14 genuegt `strikt: true` — der Hauptlauf und alle
            // Modulfunktionen erben es, lockere Skripte bleiben locker
            // (der `self.strikt` wird nur fuer **dieses** Programm
            // gesetzt und nach dem Lauf zurueckgenommen, `starten`).
            strikt: true,
            modul: None,
        };
        // Alles als **ein** Programm uebersetzen und starten.
        self.start = std::time::Instant::now();
        self.lauf_ende = 0;
        let prog = self.uebersetzen(&ast).map_err(|e| e.to_string())?;
        let fehler = match self.starten(prog) {
            Ok(_) => Vec::new(),
            Err(e) => {
                // Ein Fehler ganz ausserhalb der Modulruempe (Uebersetzung,
                // Heap) betrifft den ganzen Graphen.
                self.konsole_satz(format!("JS: Module: {e}"));
                vec![format!("JS: Module: {e}")]
            }
        };
        // Alle Module als „fertig" merken — der naechste `module_laden`
        // soll sie nicht noch einmal laufen lassen.
        for e in self.module.module.values_mut() {
            e.zustand = Zustand::Fertig;
        }
        // J12: die wartenden dynamischen Importe erfuellen (jetzt liegen
        // die Exporte vor) — und **erst danach** die Mikrotasks.
        //
        // GEMESSEN/GEFUNDEN, warum die Reihenfolge hier entscheidend ist:
        // mit dem Mikrotask-Lauf vor diesem Block blieb der `then`-Ruecklauf
        // aus (`import('./x.js').then(…)` lief nie), weil die Reaktionen
        // erst beim Erfuellen in die Schlange kommen — nach dem Lauf.
        let wartend = std::mem::take(&mut self.module.wartend);
        for (url, versprechen) in wartend {
            self.wurzeln.push(GVal::Ref(versprechen));
            if self.module.eintrag(&url).is_some() {
                let ns = self.modul_alle_als_wert(&url);
                self.versprechen_settle(versprechen, true, ns);
            } else {
                // Die Quelle steht noch aus: das Versprechen bleibt offen
                // und wartet auf den naechsten `module_laden`.
                // GEMESSEN/ENTSCHIEDEN: **nicht** ablehnen — der Import
                // wird ja waehrend des Ladens bekannt, die Quelle kommt
                // danach; ein Ablehnen wuerde jede
                // `import('./x').then(…)`-Kette zerstoeren, obwohl der
                // Import noch kommt. Der Host bricht bewusst ab mit
                // `module_import_abbrechen`.
                self.module.wartend.push((url, versprechen));
            }
        }
        // Mikrotasks **nach** dem Erfuellen (siehe Kommentar oben).
        if let Some(e) = self.schlange_abarbeiten() {
            return Err(e.to_string());
        }
        self.timer_autonom_abarbeiten().map_err(|e| e.to_string())?;
        Ok(fehler)
    }

    /// J12: alle wartenden dynamischen Importe **ablehnen** — der Weg
    /// fuer einen Host, der die Quelle nicht bekommen kann (Netzfehler,
    /// 404, Timeout). Rueckgabe: wie viele Versprechen abgelehnt wurden;
    /// die `.catch`-Ruecklaeufe der Seite laufen danach.
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum der Host das entscheidet und nicht der
    /// Motor: „die Quelle kommt nicht mehr" ist eine Aussage ueber das
    /// Netz des Hosts — der Motor kann sie nicht wissen (er hat keinen
    /// Socket).
    /// Release 12.10.2026: EINE Adresse ist nicht zu laden (Netz, 404, Typ,
    /// Syntax) — sie wird gemerkt, und die dynamischen Importe genau dieser
    /// Adresse werden abgelehnt. GEMESSEN vorher (WPT dynamic-import, 32
    /// Zeitueberschreitungen): ein gescheiterter Import blieb fuer immer offen.
    pub fn module_url_scheitern(&mut self, url: &str, grund: &str) -> usize {
        if self.module.gescheitert.len() < 4096 {
            self.module.gescheitert.insert(url.to_string());
        }
        let (weg, bleiben): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.module.wartend).into_iter().partition(|(u, _)| u == url);
        self.module.wartend = bleiben;
        let n = weg.len();
        for (_url, versprechen) in weg {
            self.wurzeln.push(GVal::Ref(versprechen));
            let f = self.fehler_wert("TypeError", grund);
            self.versprechen_settle(versprechen, false, f);
        }
        if n > 0 {
            let _ = self.schlange_abarbeiten();
        }
        n
    }

    /// Release 12.10.2026: ist diese Adresse schon gescheitert?
    pub fn module_ist_gescheitert(&self, url: &str) -> bool {
        self.module.gescheitert.contains(url)
    }

    /// Release 12.10.2026: wartet noch ein dynamischer Import?
    pub fn module_wartende(&self) -> usize {
        self.module.wartend.len()
    }

    pub fn module_import_abbrechen(&mut self, grund: &str) -> usize {
        let wartend = std::mem::take(&mut self.module.wartend);
        let n = wartend.len();
        for (_url, versprechen) in wartend {
            self.wurzeln.push(GVal::Ref(versprechen));
            let f = self.fehler_wert("TypeError", grund);
            self.versprechen_settle(versprechen, false, f);
        }
        // Die `catch`-Ruecklaeufe muessen danach laufen: der Host ruft
        // diese Methode **ausserhalb** eines Skriptlaufs, es gibt also
        // keinen laufenden Rahmen, der sie abarbeitet.
        if n > 0 {
            let _ = self.schlange_abarbeiten();
        }
        n
    }

    /// J10: anmelden + alles laden — der kurze Weg fuer den Host, wenn
    /// alle Quellen schon da sind.
    pub fn module_ausfuehren(&mut self, url: &str, quelle: &str) -> Result<Vec<String>, String> {
        self.module_starten(url, quelle)
            .map_err(|e| e.to_string())?;
        self.module_laden()
    }

    /// Die Reihenfolge der Ausfuehrung: Abhaengigkeiten zuerst
    /// (Tiefensuche ueber `braucht`), danach die uebrigen in der
    /// Anmelde-Reihenfolge. Zyklen brechen an der `besucht`-Liste ab.
    ///
    /// GEMESSEN, warum der **Start** den Durchlauf bestimmt: `braucht`
    /// steht in **Quelltext-Reihenfolge**, die Anmelde-Reihenfolge nicht
    /// (der Host reicht die Quellen in der Reihenfolge nach, in der
    /// `module_offene` sie nennt — das ist sortiert). Ohne den Start als
    /// Wurzel liefen zwei Importe in Alphabetreihenfolge statt in der
    /// Reihenfolge des Skripts: GEMESSEN `c z` vor `b lib`.
    fn module_reihenfolge(&self) -> Vec<String> {
        let mut fertig: Vec<String> = Vec::new();
        let mut besucht: Vec<String> = Vec::new();
        fn tiefe(l: &ModulLader, url: &str, fertig: &mut Vec<String>, besucht: &mut Vec<String>) {
            if besucht.iter().any(|u| u == url) || fertig.iter().any(|u| u == url) {
                return;
            }
            besucht.push(url.to_string());
            if let Some(e) = l.eintrag(url) {
                // `braucht` ist eine Kopie der Import-Reihenfolge im
                // Quelltext.
                for a in &e.braucht {
                    tiefe(l, a, fertig, besucht);
                }
            }
            if !fertig.iter().any(|u| u == url) {
                fertig.push(url.to_string());
            }
        }
        for u in &self.module.reihenfolge {
            tiefe(&self.module, u, &mut fertig, &mut besucht);
        }
        fertig
    }

    /// Eine Zeile in die Konsole des Hosts (mit Pufferdeckel, wie
    /// `console.log`).
    fn konsole_satz(&mut self, zeile: String) {
        if self.konsole.len() >= 10_000 {
            self.konsole.remove(0);
        }
        self.konsole.push(zeile.clone());
        eprintln!("{zeile}");
    }
}

/// J10: die GC-Wurzeln der Module (Exporte). Ohne sie sammelt ein GC
/// zwischen zwei Laeufen genau die weg, die das naechste Modul beim Import
/// noch braucht.
///
/// GEMESSEN/ENTSCHIEDEN, warum das im Lader und nicht im `Engine` steht:
/// der Lader besitzt die Modultabelle; `Engine::sammeln` sieht sie nur als
/// Feld.
pub(crate) fn module_wurzeln_sammeln(lader: &ModulLader, wurzeln: &mut Vec<GVal>) {
    for e in lader.module.values() {
        for (_, v) in &e.export {
            wurzeln.push(*v);
        }
        if let Some(n) = e.namensraum {
            wurzeln.push(GVal::Ref(n));
        }
    }
    // J12: die wartenden dynamischen Importe sind ebenfalls Wurzeln.
    // GEMESSEN/ENTSCHIEDEN, warum das noetig war: zwischen dem ersten
    // `module_laden` (Import bekannt, Quelle fehlt) und dem zweiten
    // (Quelle da) alloziert der Host — ein GC raeumte das Versprechen weg,
    // weil es nur in der Lader-Liste stand. Danach blieb der `then`-
    // Ruecklauf aus, ohne Fehler.
    for (_, v) in &lader.wartend {
        wurzeln.push(GVal::Ref(*v));
    }
}
// ---------------------------------------------------------------------------
// Die vier Natives (Anhaenger aus `builtin2.rs`)
// ---------------------------------------------------------------------------

impl crate::Engine {
    /// `__jsvm_import__(url, name)` — ein Wert aus der Exporttabelle.
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum ein fehlender Export `undefined` ist und
    /// **kein** Fehler: ein Tippfehler im Namen darf nicht das ganze Modul
    /// (und damit die Seite) toeten — so ist es im Browser auch.
    pub(crate) fn n_modul_import(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let url = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let name = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        Ok(self.export_lesen(&url, &name))
    }

    /// `__jsvm_import_alle__(url)` — ein Objekt mit allen Exporten.
    pub(crate) fn n_modul_import_alle(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let url = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        Ok(self.modul_alle_als_wert(&url))
    }

    /// `__jsvm_export__(url, name, wert)` — den Export merken.
    pub(crate) fn n_modul_export(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let url = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let name = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let wert = self.arg(args, 2);
        let marke = self.wurzel_merke();
        self.wurzeln.push(wert);
        // Doppelte Exporte: der erste gewinnt (wie im Browser).
        // Release 10.10.2026: eine spaetere Meldung desselben Namens
        // ueberschreibt (fruehe Meldung nach der Deklaration, Endwert nach
        // dem Rumpf — siehe `desugar`), und der Namensraum geht mit.
        let mut ns = None;
        if let Some(e) = self.module.module.get_mut(&url) {
            match e.export.iter_mut().find(|(n, _)| *n == name) {
                Some((_, v)) => *v = wert,
                None => e.export.push((name.clone(), wert)),
            }
            ns = e.namensraum;
        }
        if let Some(ns) = ns {
            let _ = self.setze(GVal::Ref(ns), &name, wert);
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Undefiniert)
    }

    /// `__jsvm_export_alle__(url)` — alle Exporte eines Moduls uebernehmen
    /// (ohne `default`, so sagt es die Spec).
    pub(crate) fn n_modul_export_alle(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let url = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let quelle_url = self.zu_text(self.arg(args, 1)).unwrap_or_default();
        let quelle = self
            .module
            .eintrag(&quelle_url)
            .map(|e| e.export.clone())
            .unwrap_or_default();
        // Alles als Wurzeln markieren, waehrend wir die Liste im Ziel
        // ablegen (die Werte stehen nur in Rust-Variablen).
        let marke = self.wurzel_merke();
        for (_, v) in &quelle {
            self.wurzeln.push(*v);
        }
        let mut neu: Vec<(String, GVal)> = Vec::new();
        let mut ns = None;
        if let Some(ziel) = self.module.module.get_mut(&url) {
            for (n, v) in quelle {
                if n != "default" && !ziel.export.iter().any(|(en, _)| *en == n) {
                    ziel.export.push((n.clone(), v));
                    neu.push((n, v));
                }
            }
            ns = ziel.namensraum;
        }
        if let Some(ns) = ns {
            for (n, v) in neu {
                let _ = self.setze(GVal::Ref(ns), &n, v);
            }
        }
        self.wurzel_zurueck(marke);
        Ok(GVal::Undefiniert)
    }

    /// `__jsvm_meldung__(url, fehler)` — der Rumpf hat geworfen. Die
    /// Meldung geht in die Konsole des Hosts (Statuszeile) und der Lader
    /// laeuft weiter.
    pub(crate) fn n_modul_meldung(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let url = self.zu_text(self.arg(args, 0)).unwrap_or_default();
        let wert = self.arg(args, 1);
        let text = self.wurf_text(wert);
        self.konsole_satz(format!("JS: Modul {url}: {text}"));
        // A host that reports script errors as `error` events (workers:
        // `messaging.js`) defines `__narsilModuleError(url, error)`; an
        // exception in a module body then reaches it like one in a classic
        // script (HTML §8.1.4.6 "report an exception").
        let global = GVal::Ref(self.global_objekt);
        if let Some(e) = self.eigene_lese(self.global_objekt, "__narsilModuleError")
            && self.ist_aufrufbar(e)
        {
            let u = self.text_wert(&url).unwrap_or(GVal::Undefiniert);
            self.wurzeln.push(u);
            let _ = self.aufruf_synchron(e, global, &[u, wert], false);
        }
        Ok(GVal::Undefiniert)
    }

    /// J12: `import('…')` — der dynamische Import.
    ///
    /// Reihenfolge wie im Browser: der Import **startet sofort** die
    /// Anmeldung der Quelle (der Host sieht sie bei `module_offene()`),
    /// das Versprechen erfuellt sich, wenn das Modul geladen wurde. Also
    /// `import('./a.js').then(ns => ns.f())` laeuft so:
    ///   1. Skript: `import()` → Promise, `module_offene()` nennt die URL.
    ///   2. Host: `module_quelle_setzen(url, text)`.
    ///   3. Host: `module_laden()` → der Rumpf laeuft, die Exporte liegen
    ///      vor, das Versprechen wird erfuellt, die Mikrotasks laufen.
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum die Quelle vom Host kommt und nicht
    /// `fetch`: siehe Modul-Kopf (asynchroner Netzweg, Import muss vorher
    /// fertig sein). Fuer ein **Skript** (nicht Modul) ist die Basis die
    /// Herkunft der Seite; in einem Modul die Modul-URL (der Entzucker
    /// ruft das Native mit beiden auf).
    /// Release 12.10.2026: die Basis fuer `import()` ausserhalb eines Moduls —
    /// `document.currentScript.src` (externes Skript), sonst `document.baseURI`.
    fn dokument_basis(&mut self) -> Option<String> {
        let global = GVal::Ref(self.global_objekt);
        let doc = self.hole(global, "document").ok()?;
        if !matches!(doc, GVal::Ref(_)) {
            // A worker has no document: its base is the script's URL
            // (`self.location.href`, HTML §10.2.4 "set up a worker
            // environment settings object").
            let ort = self.hole(global, "location").ok()?;
            if !matches!(ort, GVal::Ref(_)) {
                return None;
            }
            let href = self.hole(ort, "href").ok()?;
            let t = self.zu_text(href).ok()?;
            return (t.contains("://") || t.starts_with("data:") || t.starts_with("blob:")).then_some(t);
        }
        if let Ok(skript) = self.hole(doc, "currentScript")
            && matches!(skript, GVal::Ref(_))
            && let Ok(src) = self.hole(skript, "src")
            && let Ok(t) = self.zu_text(src)
            && !t.is_empty()
            && t != "undefined"
        {
            return Some(t);
        }
        let b = self.hole(doc, "baseURI").ok()?;
        let t = self.zu_text(b).ok()?;
        (!t.is_empty() && t != "undefined").then_some(t)
    }

    pub(crate) fn n_modul_import_dyn(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let arg = self.arg(args, 0);
        let ziel = self.zu_text(arg)?;
        if ziel.is_empty() {
            let f = self.fehler_wert("TypeError", "import() ohne Adresse");
            return Err(Stoerung::Wurf(f));
        }
        // Basis: die eigene Modul-URL, wenn der Import **in einem Modul**
        // steht (der Entzucker legt sie als Bindung ab — das deckt jede
        // Verschachtelung); sonst die Herkunft der Seite. In node wird
        // genauso aufgeloest (Modul-URL im Modul, Dokumentadresse im
        // Skript).
        // Release 12.10.2026: in einem klassischen Skript die Adresse des
        // laufenden externen Skripts, sonst die Basis des Dokuments (HTML
        // §8.1.3.6 „resolve a module specifier": base URL des Skripts).
        // GEMESSEN vorher: die HERKUNFT — `import('./x.js')` aus
        // `/a/b/seite.html` holte `/x.js`.
        let basis = match self.modul_url_im_rahmen() {
            Some(b) => b,
            None => self.dokument_basis().or_else(|| self.herkunft_text()).unwrap_or_default(),
        };
        let mut url = self.module.karte.aufloesen(&basis, &ziel);
        // Release 11.10.2026: `import(u, { with: { type: "json" } })`.
        let opt = self.arg(args, 1);
        if matches!(opt, GVal::Ref(_)) {
            let mit = self.hole(opt, "with").unwrap_or(GVal::Undefiniert);
            let mit = if matches!(mit, GVal::Undefiniert) { self.hole(opt, "assert").unwrap_or(GVal::Undefiniert) } else { mit };
            if matches!(mit, GVal::Ref(_)) {
                let typ = self.hole(mit, "type").unwrap_or(GVal::Undefiniert);
                if !matches!(typ, GVal::Undefiniert) {
                    let t = self.zu_text(typ)?;
                    if !matches!(t.as_str(), "json" | "css" | "text") {
                        let f = self.fehler_wert("TypeError", &format!("Modultyp '{t}' wird nicht unterstuetzt"));
                        let p = self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen);
                        if let GVal::Ref(pr) = p {
                            self.versprechen_settle(pr, false, f);
                        }
                        return Ok(p);
                    }
                    url = format!("{url}{MODUL_TYP_MARKE}{t}");
                }
            }
        }
        // (1) Schon geladen? Dann sofort erfuellen.
        if let Some(e) = self.module.eintrag(&url)
            && e.zustand == crate::module::Zustand::Fertig
        {
            let ns = self.modul_alle_als_wert(&url);
            return Ok(self.versprechen_erfuellt(ns));
        }
        // (2) Sonst: Versprechen anlegen und warten. Die Quelle holt der
        // Host (sie steht in `module_offene()`), danach erfuellt der
        // naechste `module_laden` das Versprechen. Bricht der Host ab,
        // macht er das mit `module_import_abbrechen`.
        let p = self.versprechen_anlegen(crate::heap::VersprechenZustand::Offen);
        self.wurzeln.push(p);
        if let GVal::Ref(pr) = p {
            self.module.wartend.push((url, pr));
        }
        // Der Import zaehlt als offene Quelle — der Host sieht sie.
        // (3) Die Anmeldung: als „fehlende Quelle“ melden, damit der Host
        // sie holt. Ist die Quelle schon im Lader (z.B. als Import einer
        // anderen Seite), genuegt `module_laden`.
        Ok(p)
    }

    /// J12: die Modul-URL aus dem Scope des **aktuellen** Rahmens
    /// (`__jsvm_modul_url__`), falls der Import in einem Modul steht.
    fn modul_url_im_rahmen(&mut self) -> Option<String> {
        let umg = self.rahmen.last().map(|r| r.umg)?;
        match self.lese(umg, N_MODUL_URL) {
            Ok(v) if !matches!(v, GVal::Undefiniert) => Some(self.zu_text(v).unwrap_or_default()),
            _ => None,
        }
    }

    /// Der Wert eines Exports (fehlender Export = `undefined`).
    fn export_lesen(&mut self, url: &str, name: &str) -> GVal {
        self.module
            .eintrag(url)
            .and_then(|e| e.export.iter().find(|(n, _)| n == name).map(|(_, v)| *v))
            .unwrap_or(GVal::Undefiniert)
    }

    /// Ein Objekt mit allen Exporten des Moduls (`import * as ns`).
    ///
    /// GEMESSEN/ENTSCHIEDEN, warum ein **Schnappschuss**-Objekt und kein
    /// Proxy: ein Proxy mit `ownKeys`/`getOwnPropertyDescriptor` waere der
    /// ehrliche Weg, kostet aber je Zugriff eine Falle in den Lader. Der
    /// Unterschied faellt nur auf, wenn ein Modul seinen Export **nach**
    /// dem Import aendert — bis dahin verhalten sich beide gleich.
    fn modul_alle_als_wert(&mut self, url: &str) -> GVal {
        // Release 10.10.2026: EIN Namensraum je Modul (siehe
        // `Eintrag::namensraum`) — `import * as a` zweimal ist dasselbe
        // Objekt, und spaetere Exporte erscheinen darin.
        if let Some(n) = self.module.eintrag(url).and_then(|e| e.namensraum) {
            return GVal::Ref(n);
        }
        let paare: Vec<(String, GVal)> = self
            .module
            .eintrag(url)
            .map(|e| e.export.clone())
            .unwrap_or_default();
        let r = match self.neues_objekt(Some(self.proto_objekt)) {
            Ok(r) => r,
            Err(_) => return GVal::Undefiniert,
        };
        if let Some(e) = self.module.module.get_mut(url) {
            e.namensraum = Some(r);
        }
        let marke = self.wurzel_merke();
        self.wurzeln.push(GVal::Ref(r));
        for (n, v) in paare {
            self.wurzeln.push(v);
            // Eine kaputte Eigenschaft darf den Import nicht sprengen.
            let _ = self.setze(GVal::Ref(r), &n, v);
        }
        self.wurzel_zurueck(marke);
        GVal::Ref(r)
    }
}
