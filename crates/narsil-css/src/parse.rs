//! Parser: Stylesheet, Selektoren, Deklarationen, @media (C1/C2/C5).
//!
//! Fehlererholung (C1): Eine kaputte Deklaration verwirft nur sich
//! selbst (bis `;` oder `}`), eine kaputte Regel nur sich selbst (bis
//! zur schliessenden Klammer). Jede Schleife verbraucht mindestens ein
//! Token und endet an einer MA_*-Grenze — kein Haengen, kein Wachsen.

use super::farben;
use super::token::{Token, kuerzen, tokenisieren};
use super::typen::*;
use super::{
    MAX_DEKL_WERT_LAENGE, MAX_DEKLARATIONEN_JE_REGEL, MAX_KLAMMER_TIEFE, MAX_RECHEN_ARGS,
    MAX_REGELN, MAX_SELEKTOR_LAENGE, MAX_SELEKTOR_TEILE, MAX_SELEKTOREN_JE_REGEL, MAX_TOKEN,
    MAX_VAR_JE_REGEL, MAX_VERLAUF_INNEN, MAX_VERLAUF_STOPPS,
};

/// Hoechste Teile je Zusammengesetztem (Klassen+Attribute+Pseudos).
/// Begruendung: echte Selektoren haben eine Handvoll; mehr ist Bombe
/// (".a.a.a..." x 100.000) — der Selektor wird ungueltig, nie riesig.
const MA_EINFACH_TEILE: usize = 32;

/// Hoechste Oder-Zeilen je @media. Begruendung: mehr als 8 Alternativen
/// schreibt niemand; der Rest faellt weg.
const MA_MEDIA_ODER: usize = 8;

// ===========================================================================
// OEFFENTLICH (Vertrag Abschnitt 1)
// ===========================================================================

// ===========================================================================
// VERWURF-ZAEHLER (K6, 01.10.2026) — WAS GEHT AUF ECHTEN SEITEN
// VERLOREN?
// ===========================================================================

/// Zaehlt einen Verwurf-Grund, wenn der Zaehler eingeschaltet hat.
///
/// # WARUM EINE ETRA-FUNKTION UND NICHT `Stylesheet::verworfen`
///
/// `verworfen` ist EINE Zahl fuer alles. Sie sagt "diese Datei hatte
/// 58 Probleme", aber nicht "welche". Ohne die Art des Problems ist
/// jede Aussage darueber eine Vermutung — und K6 verlangt eine
/// RANGFOLGE, nicht eine Zahl.
///
/// # KOSTEN IM NORMALFALL
///
/// Genau ein `Relaxed`-Lesevorgang auf ein `AtomicBool`, wenn der
/// Zaehler aus ist. Keine Allokation, keine Schleife, kein Wachstum
/// (M3/M6). Im eingeschalteten Fall ein `fetch_add` auf einen von
/// `MA_NAECKE` festen Platzen — die Liste kann nicht wachsen.
///
/// # WER ZAEHLT
///
/// `examples/abdeckung.rs`. Kein Test, kein Browserpfad.
static ZAEHLER_AN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// So viele verschiedene Gruende werden gemerkt (M3/M4). Danach
/// kommen keine neuen mehr — `zuendlich` zeigt es an.
const MA_NAECKE: usize = 256;

/// `(Grund, Anzahl)` je Platz. `Box::new([..])` haelt die
/// Initialisierung `const`-faehig.
static ZAEHLER: std::sync::OnceLock<std::sync::Mutex<Vec<(std::sync::OnceLock<String>, u64)>>> =
    std::sync::OnceLock::new();

/// Schaltet den Zaehler ein oder aus.
pub fn zaehle(an: bool) {
    ZAEHLER_AN.store(an, std::sync::atomic::Ordering::Relaxed);
    if an {
        zaehler_nullen();
    }
}

/// Setzt alle Zahlen auf 0 (die Namen bleiben, sie kosten nichts neu).
pub fn zaehler_nullen() {
    let v = ZAEHLER.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    if let Ok(mut w) = v.lock() {
        for e in w.iter_mut() {
            e.1 = 0;
        }
    }
}

/// Die Zahlen, absteigend nach Haeufigkeit.
pub fn zaehler_lesen() -> Vec<(String, usize)> {
    let v = ZAEHLER.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let Ok(w) = v.lock() else {
        return Vec::new();
    };
    let mut raus: Vec<(String, usize)> = w
        .iter()
        .filter(|e| e.1 > 0)
        .map(|e| (e.0.get().cloned().unwrap_or_default(), e.1 as usize))
        .collect();
    raus.sort_by(|a, b| b.1.cmp(&a.1));
    raus
}

/// Merkt EINEN Verwurf. `grund` ist der ROHTET, den der Parser
/// gerade verworfen hat (auf 120 Zeichen gekuerzt).
fn verwurf(grund: &str) {
    if !ZAEHLER_AN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // M4: Allokation erst NACH der Grenzpruefung.
    let mut k = grund.len().min(120);
    while k > 0 && !grund.is_char_boundary(k) {
        k -= 1;
    }
    let kurz = &grund[..k];
    let v = ZAEHLER.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let Ok(mut w) = v.lock() else {
        return;
    };
    for e in w.iter_mut() {
        if e.0.get().map(|s| s == kurz).unwrap_or(false) {
            e.1 += 1;
            return;
        }
    }
    if w.len() >= MA_NAECKE {
        return;
    }
    w.push((std::sync::OnceLock::from(String::from(kurz)), 1));
}

/// Liest ein Stylesheet (ein `<style>`-Block, eine `.css`-Datei).
/// Release 09.10.2026: [`stylesheet_lesen`] mit Gedaechtnis.
///
/// GEMESSENER ANLASS: jede Seite wird zweimal gebaut (vor und nach den
/// Skripten), und jeder Bau las alle Blaetter neu — github.com: 6 MB CSS,
/// rund 290 ms je Bau. Gemerkt wird nach INHALT (Laenge + Hash des ganzen
/// Textes); geliefert wird eine Kopie, denn Klonen kostet einen Bruchteil
/// des Lesens. Dasselbe Blatt auf der naechsten Seite derselben Site
/// (gleiche Datei) kommt damit ebenfalls aus dem Gedaechtnis.
///
/// M3/M4: hoechstens [`MERK_EINTRAEGE`] Blaetter und [`MERK_BYTES`]
/// Quelltext, der am laengsten unbenutzte Eintrag faellt zuerst. Blaetter
/// unter [`MERK_MIN`] Byte werden nicht gemerkt (dort ist Lesen billiger
/// als Hashen und Klonen).
pub fn stylesheet_lesen_gemerkt(text: &str) -> Stylesheet {
    if text.len() < MERK_MIN {
        return stylesheet_lesen(text);
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let schluessel = (text.len(), h.finish());
    {
        let mut merk = MERK.lock().unwrap_or_else(|e| e.into_inner());
        merk.uhr += 1;
        let uhr = merk.uhr;
        if let Some(e) = merk.eintraege.iter_mut().find(|e| e.schluessel == schluessel) {
            e.benutzt = uhr;
            return (*e.blatt).clone();
        }
    }
    let blatt = std::sync::Arc::new(stylesheet_lesen(text));
    let mut merk = MERK.lock().unwrap_or_else(|e| e.into_inner());
    if text.len() <= MERK_BYTES {
        while !merk.eintraege.is_empty()
            && (merk.eintraege.len() >= MERK_EINTRAEGE || merk.bytes + text.len() > MERK_BYTES)
        {
            let Some((alt, _)) = merk
                .eintraege
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.benutzt)
            else {
                break;
            };
            let weg = merk.eintraege.swap_remove(alt);
            merk.bytes = merk.bytes.saturating_sub(weg.schluessel.0);
        }
        let uhr = merk.uhr;
        merk.bytes += text.len();
        merk.eintraege.push(MerkEintrag {
            schluessel,
            benutzt: uhr,
            blatt: blatt.clone(),
        });
    }
    (*blatt).clone()
}

/// Hoechstens so viele gemerkte Blaetter.
pub const MERK_EINTRAEGE: usize = 96;
/// Hoechstens so viel gemerkter Quelltext (Byte).
pub const MERK_BYTES: usize = 48 * 1024 * 1024;
/// Kleinere Blaetter werden nicht gemerkt.
pub const MERK_MIN: usize = 2048;

struct MerkEintrag {
    schluessel: (usize, u64),
    benutzt: u64,
    blatt: std::sync::Arc<Stylesheet>,
}

struct Merk {
    eintraege: Vec<MerkEintrag>,
    bytes: usize,
    uhr: u64,
}

static MERK: std::sync::Mutex<Merk> = std::sync::Mutex::new(Merk {
    eintraege: Vec::new(),
    bytes: 0,
    uhr: 0,
});

/// Release 09.10.2026: Stueckgroesse fuer `stylesheet_lesen` (siehe dort).
const STUECK_BYTES: usize = 256 * 1024;

/// Release 09.10.2026: zerlegt CSS-Quelltext in Stuecke von mindestens
/// `ziel` Bytes, jeweils hinter einer `}` der OBERSTEN Ebene (ausserhalb von
/// Kommentaren und Zeichenketten). Ein Blatt ohne solche Grenze bleibt ein
/// Stueck. Jede Runde verbraucht ein Byte (M3).
fn regel_stuecke(text: &str, ziel: usize) -> Vec<&str> {
    if text.len() <= ziel {
        return vec![text];
    }
    let b = text.as_bytes();
    let mut raus = Vec::new();
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut i = 0usize;
    let mut zeichenkette: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = zeichenkette {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q || c == b'\n' {
                zeichenkette = None;
            }
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        match c {
            b'"' | b'\'' => zeichenkette = Some(c),
            b'{' => tiefe += 1,
            b'}' => {
                tiefe = tiefe.saturating_sub(1);
                if tiefe == 0 && i + 1 - start >= ziel && text.is_char_boundary(i + 1) {
                    raus.push(&text[start..i + 1]);
                    start = i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if start < text.len() {
        raus.push(&text[start..]);
    }
    raus
}

/// Kaputte Regeln verwerfen nur sich selbst; was die Grenzen
/// ueberschreitet, steht gezaehlt in [`Stylesheet::verworfen`].
/// Leere/kaputte Eingabe ergibt ein leeres Blatt (kein Fehler, kein Panic).
pub fn stylesheet_lesen(text: &str) -> Stylesheet {
    let (gek, eingabe_gekappt) = kuerzen(text);
    let mut blatt = Stylesheet::default();
    let mut regeln = 0usize;
    EIGENSCHAFTS_ANFANG.with(|a| a.borrow_mut().clear());
    PROPERTY_LAYERS.with(|l| l.borrow_mut().clear());
    LAYER_PATH.with(|p| p.borrow_mut().clear());
    LAYER_ORDER.with(|o| *o.borrow_mut() = (Vec::new(), 0));
    SCOPE_NACHGESTELLT.with(|s| s.borrow_mut().clear());
    // Release 09.10.2026: grosse Blaetter STUECKWEISE (an Regelgrenzen der
    // obersten Ebene) tokenisieren und lesen.
    //
    // GEMESSEN, warum: `tokenisieren` zerlegte das ganze Blatt auf einmal und
    // hoerte bei `MAX_TOKEN` (1 Mio) auf. YouTubes Hauptblatt
    // (`kevlar_base`, 3,18 MiB) war nach rund 1,5 MiB zu Ende — alle Regeln
    // dahinter fehlten still (`#container.ytd-masthead{display:flex}` steht
    // bei 87 %: der Kopf der Seite blieb ungestylt). Stuecke von 256 KiB
    // halten zugleich den Tokenspeicher klein (M4): die Token eines Stuecks
    // werden freigegeben, bevor das naechste entsteht.
    let mut alle_voll = true;
    // Release 11.10.2026 (Agent CSS2): eingesetzte Blaetter einzeln (siehe
    // `BLATT_GRENZE`).
    for stueck in gek.split(crate::BLATT_GRENZE).flat_map(|teil| regel_stuecke(teil, STUECK_BYTES)) {
        let (token, token_voll) = tokenisieren(stueck);
        alle_voll &= token_voll;
        let mut pos = 0usize;
        NEST_TIEFE.with(|t| t.set(0));
        // Hauptschleife: jede Runde verbraucht >= 1 Token oder endet (M3).
        while pos < token.len() {
            leer_ueber(&token, &mut pos);
            if pos >= token.len() {
                break;
            }
            // Regelgrenze (M4): der Rest faellt weg und wird pauschal gezaehlt.
            if regeln >= MAX_REGELN {
                blatt.verworfen += 1;
                break;
            }
            match &token[pos] {
                Token::At(_) => {
                    if parse_at(&token, &mut pos, &mut blatt, &mut regeln, 0) {
                        // ok
                    }
                }
                _ => {
                    parse_stilregel(&token, &mut pos, &mut blatt, &mut regeln, false);
                }
            }
        }
        if regeln >= MAX_REGELN {
            break;
        }
    }
    // Ehrlich gezaehlt: abgeschnittener Rest = 1 Pauschale (nur wenn es
    // ueberhaupt Rest gab — sonst bleibt verworfen exakt).
    if eingabe_gekappt || !alle_voll {
        blatt.verworfen += 1;
    }
    // Release 08.10.2026: `@property`-Anfangswerte als niedrigste
    // Schicht VORNE — sie werden zuerst angemeldet und verlieren damit
    // gegen jede andere Angabe (siehe `parse_property`).
    let anfang = EIGENSCHAFTS_ANFANG.with(|a| std::mem::take(&mut *a.borrow_mut()));
    let layers = PROPERTY_LAYERS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    let order = LAYER_ORDER.with(|o| std::mem::take(&mut o.borrow_mut().0));
    for (i, schicht) in property_layers(anfang, layers, order).into_iter().enumerate() {
        blatt.regeln.insert(i, schicht);
    }
    // Release 12.10.2026 (Agent C): die Regeln aus `@scope` auf oberster
    // Ebene ans Ende — bei gleicher Spezifitaet gewinnen sie gegen
    // ungescopte (Naehe, CSS Cascade 6 §2.5.3; Naeherung).
    blatt.regeln.extend(SCOPE_NACHGESTELLT.with(|s| std::mem::take(&mut *s.borrow_mut())));
    blatt
}

thread_local! {
    /// Release 12.10.2026 (Agent C): umgeschriebene `@scope`-Regeln der
    /// obersten Ebene, die `stylesheet_lesen` am Ende anhaengt.
    static SCOPE_NACHGESTELLT: std::cell::RefCell<Vec<Regel>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Liest ein `style="..."`-Attribut zu Deklarationen (Kurzformen
/// expandiert, Unbekanntes weg). Variablendefinitionen (`--*`) landen
/// NICHT hier, sondern in [`inline_variablen`]. Leere/kaputte Eingabe
/// ergibt `[]`.
pub fn inline_lesen(style_attr: &str) -> Vec<Deklaration> {
    let (gek, _) = kuerzen(style_attr);
    let (token, _) = tokenisieren(gek);
    let mut aus: Vec<Deklaration> = Vec::new();
    // Stuecke an `;` (und zur Erholung auch an Klammern) teilen.
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut stuecke = 0usize;
    let mut i = 0usize;
    while i <= token.len() {
        let grenze = i >= token.len() || ist_trenner(&token[i], &mut tiefe);
        if grenze {
            if stuecke < MAX_DEKLARATIONEN_JE_REGEL && i > start {
                aus.extend(parse_dekl(&token[start..i]).dekls);
            }
            stuecke += 1;
            start = i + 1;
        }
        i += 1;
    }
    aus
}

/// Liest die Variablendefinitionen (`--name: wert`) eines
/// `style="..."`-Attributs (C10, W6). Hoechstens MAX_VAR_JE_REGEL.
pub fn inline_variablen(style_attr: &str) -> Vec<VariablenDekl> {
    let (gek, _) = kuerzen(style_attr);
    let (token, _) = tokenisieren(gek);
    let mut aus: Vec<VariablenDekl> = Vec::new();
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut stuecke = 0usize;
    let mut i = 0usize;
    while i <= token.len() {
        let grenze = i >= token.len() || ist_trenner(&token[i], &mut tiefe);
        if grenze {
            if stuecke < MAX_VAR_JE_REGEL && i > start {
                if let Some(v) = parse_dekl(&token[start..i]).var {
                    aus.push(v);
                }
            }
            stuecke += 1;
            start = i + 1;
        }
        i += 1;
    }
    aus
}

/// True bei Stueckgrenze; Klammern dienen nur der Erholung (kaputte
/// Bloecke im Attribut sollen nicht alles verschlucken).
fn ist_trenner(t: &Token, tiefe: &mut usize) -> bool {
    match t {
        Token::Zeichen(';') if *tiefe == 0 => true,
        Token::Zeichen('{') | Token::Zeichen('}') => true,
        Token::Funktion(_) => {
            *tiefe = tiefe.saturating_add(1);
            false
        }
        Token::Zeichen('(') | Token::Zeichen('[') => {
            *tiefe = tiefe.saturating_add(1);
            false
        }
        Token::Zeichen(')') | Token::Zeichen(']') => {
            *tiefe = tiefe.saturating_sub(1);
            false
        }
        _ => false,
    }
}

// ===========================================================================
// CURSOR-HILFEN
// ===========================================================================

/// Ueberspringt Leer-Token.
fn leer_ueber(token: &[Token], pos: &mut usize) {
    while *pos < token.len() && matches!(token[*pos], Token::Leer) {
        *pos += 1;
    }
}

// ===========================================================================
// REGELN
// ===========================================================================

/// Parst ab `@...`: `@media` mit Tiefe 0 wird uebernommen, alles andere
/// (inkl. verschachteltem `@media`) uebersprungen. Verbraucht immer
/// mindestens ein Token. Rueckgabe immer true (fuer kuenftige Zaehler).
fn parse_at(
    token: &[Token],
    pos: &mut usize,
    blatt: &mut Stylesheet,
    regeln: &mut usize,
    tiefe: usize,
) -> bool {
    let name = match &token[*pos] {
        Token::At(n) => n.to_ascii_lowercase(),
        _ => {
            *pos += 1;
            return true;
        }
    };
    *pos += 1;
    // Release 08.10.2026: Gruppenregeln duerfen ineinander stehen (CSS
    // Conditional 3 §2, Cascade 5 §6.4) — bis `MAX_GRUPPEN_TIEFE` (M3).
    let gruppe = tiefe < MAX_GRUPPEN_TIEFE;
    if name == "media" && gruppe {
        parse_media(token, pos, blatt, regeln, tiefe);
    } else if name == "supports" && gruppe {
        parse_supports(token, pos, blatt, regeln, tiefe);
    } else if name == "layer" && gruppe {
        parse_layer(token, pos, blatt, regeln, tiefe);
    } else if name == "scope" && gruppe {
        // Release 12.10.2026 (Agent C): `@scope` als umgeschriebene Regeln
        // (`crate::scope`).
        parse_scope(token, pos, blatt, regeln, tiefe);
    } else if name == "property" && (tiefe == 0 || LAYER_PATH.with(|p| p.borrow().len()) == tiefe) {
        // Release 14.10.2026 (Agent C): also directly inside `@layer`.
        parse_property(token, pos);
    } else if name == "import" && tiefe == 0 {
        parse_import(token, pos, blatt, regeln);
    } else if name == "font-face" {
        // K9 (CSS Fonts 4 §5).
        //
        // GEMESSENER ANLASS 01.10.2026: `mdn_styles-global.css` hat
        // 13 `@font-face`-Bloecke, und `parse.rs` kannte die Regel
        // nicht — sie lief wie jede fremde `@`-Regel in
        // `ueberspringe_at`. GEMESSEN: 0 von 13 gelesen.
        parse_font_face(token, pos, blatt);
    } else {
        // Fremde @-Regel: Anweisung bis `;` oder Block ueberspringen.
        ueberspringe_at_in(token, pos, tiefe == 0);
    }
    true
}

/// Release 12.10.2026 (Agent C): `@scope (A) to (B) { … }` — Kopf und Block
/// als Text an `crate::scope::umschreiben`, das Ergebnis wie eine Gruppe
/// gelesen und angehaengt.
fn parse_scope(token: &[Token], pos: &mut usize, blatt: &mut Stylesheet, regeln: &mut usize, tiefe: usize) {
    let mut vorspann = String::new();
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => {
                *pos += 1;
                break;
            }
            Token::Zeichen(';') | Token::Zeichen('}') => {
                *pos += 1;
                return;
            }
            t => {
                if vorspann.len() < MAX_TOKEN {
                    vorspann.push_str(&t.text());
                }
            }
        }
        *pos += 1;
    }
    let anfang = *pos;
    let mut offen = 0usize;
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => offen += 1,
            Token::Zeichen('}') if offen == 0 => break,
            Token::Zeichen('}') => offen -= 1,
            _ => {}
        }
        *pos += 1;
    }
    let ende = *pos;
    if *pos < token.len() {
        *pos += 1;
    }
    let rumpf: String = token[anfang..ende].iter().map(Token::text).collect();
    let Some(css) = crate::scope::umschreiben(&vorspann, &rumpf) else { return };
    let (t2, _) = tokenisieren(&css);
    // Release 13.10.2026: name-defining rules are unaffected by @scope (CSS
    // Cascade 6 §2.5): an `@property` in it still registers (WPT css-cascade
    // scope-name-defining-rules).
    let mut q = 0usize;
    let mut depth = 0usize;
    while q < t2.len() {
        match &t2[q] {
            Token::Zeichen('{') => depth += 1,
            Token::Zeichen('}') => depth = depth.saturating_sub(1),
            Token::At(n) if depth == 0 && n.eq_ignore_ascii_case("property") => {
                let mut after = q + 1;
                parse_property(&t2, &mut after);
                q = after.max(q + 1);
                continue;
            }
            _ => {}
        }
        q += 1;
    }
    let mut p2 = 0usize;
    let korb = gruppen_inhalt(&t2, &mut p2, regeln, tiefe);
    blatt.verworfen += korb.verworfen;
    if tiefe == 0 {
        SCOPE_NACHGESTELLT.with(|s| s.borrow_mut().extend(korb.regeln));
    } else {
        blatt.regeln.extend(korb.regeln);
    }
}

/// Ueberspringt eine fremde @-Regel (bis `;` oder ueber den Block).
/// Verbraucht mindestens ein Token, respektiert MAX_KLAMMER_TIEFE.
fn ueberspringe_at(token: &[Token], pos: &mut usize) {
    // NUR `{` und `}` sind Blockebenen fuer das ENDE der Regel.
    //
    // GEMESSENER FEHLER 01.10.2026 (04:05, K9-Vorbereitung): hier stand
    // `Token::Zeichen('{') | Token::Funktion(_) => tiefe += 1`.
    // Eine Funktion wird aber von `)` geschlossen, nicht von `}`. In
    // `@font-face { src: url(/a.woff2) format("woff2") }` blieb `tiefe`
    // deshalb dauerhaft auf 1, das schliessende `}` brachte es nicht
    // auf 0 — und der Leser lief in die naechste `@font-face` hinein.
    //
    // GEMESSENE FOLGE auf tests/daten/mdn_styles-global.css:
    //   10 Regeln gelesen (statt 45), 13 verworfen.
    // Die 13 sind genau die 13 `@font-face`-Bloecke der Datei.
    //
    // Release 11.10.2026 (Agent CSS2): PAARWEISE Klammern (CSS 2.1 §4.2
    // "matching pairs of (), [], {}", CSS Syntax 3 §5.4.2). Ein Stapel der
    // offenen Klammern: `;` und `{` IN `(...)`/`[...]` beenden die Regel
    // nicht, eine schliessende Klammer ohne passendes Gegenstueck ist ein
    // gewoehnliches Token. Die Regel endet am `;` oder mit ihrem Block
    // `{...}` auf der obersten Ebene. Ein `}` ohne offene Klammer gehoert
    // einer aeusseren Regel (verschachtelt) — auf der obersten Ebene des
    // Blatts gibt es die nicht, dort ist es ein Token (`oben`). GEMESSEN
    // vorher (WPT `syntax/matching-brackets-001`, `@foo ] } ) ... [ ; {
    // ... } ] ;`): die Regel endete am ersten `}`, der Rest wurde zur
    // naechsten Stilregel.
    ueberspringe_at_in(token, pos, false)
}

/// Wie [`ueberspringe_at`]; `oben`: die @-Regel steht auf der obersten
/// Ebene des Blatts (ein einzelnes `}` beendet dann nichts).
fn ueberspringe_at_in(token: &[Token], pos: &mut usize, oben: bool) {
    let mut offen: Vec<char> = Vec::new();
    let anfang = *pos;
    while *pos < token.len() {
        match &token[*pos] {
            // Eine @-ANWEISUNG endet am Semikolon (CSS Syntax 3 §4.3.1).
            Token::Zeichen(';') if offen.is_empty() => {
                *pos += 1;
                return;
            }
            Token::Zeichen('{') | Token::Zeichen('[') | Token::Zeichen('(') | Token::Funktion(_) => {
                offen.push(match &token[*pos] {
                    Token::Zeichen('{') => '}',
                    Token::Zeichen('[') => ']',
                    _ => ')',
                });
                // M3: eine Klammerbombe in einer fremden @-Regel darf
                // nicht den Rest der Datei verschlingen.
                if offen.len() > MAX_KLAMMER_TIEFE {
                    *pos += 1;
                    while *pos < token.len() {
                        if matches!(token[*pos], Token::Zeichen('}')) {
                            *pos += 1;
                            return;
                        }
                        *pos += 1;
                    }
                    return;
                }
            }
            Token::Zeichen(c @ ('}' | ']' | ')')) => {
                if offen.last() == Some(c) {
                    offen.pop();
                    if *c == '}' && offen.is_empty() {
                        // Der Block der Regel ist zu.
                        *pos += 1;
                        return;
                    }
                } else if *c == '}' && offen.is_empty() && !oben {
                    // Ein `}` ohne Gegenstueck gehoert einer aeusseren
                    // Regel. Mindestens ein Token ist verbraucht (das `@`
                    // davor), also kein Stillstand.
                    if *pos == anfang {
                        *pos += 1;
                    }
                    return;
                }
                // sonst: ein unpaariges Token, es gehoert zur Regel.
            }
            _ => {}
        }
        *pos += 1;
    }
}

/// Parst `@media abfrage { regeln }` (C5). Kaputte Abfrage oder Klammer:
/// Block wird uebersprungen und gezaehlt.
/// Parst `@import <url> [media]` (CSS Cascade 3, §3).
///
/// Die URL wird als Zeichenkette gemerkt, das Medium als Abfrage. Das
/// Blatt laedt die Datei NICHT — das ist eine Frage des Aufrufers, und
/// `narsil-css` hat keinen Zugriff darauf.
fn parse_import(token: &[Token], pos: &mut usize, blatt: &mut Stylesheet, regeln: &mut usize) {
    *regeln += 0; // `@import` ist KEINE Regel — sie zaehlt nicht mit.
    let mut url = String::new();
    let mut medien = String::new();
    let mut ziel = 0u8; // 0 = noch nichts, 1 = URL, 2 = Medium
    // M3/M4: die Schleife laeuft ueber die Token; `MAX_TOKEN` begrenzt
    // die beiden Zeichenketten.
    let mut schritte = 0usize;
    while *pos < token.len() {
        schritte += 1;
        if schritte > MAX_TOKEN {
            blatt.verworfen += 1;
            return;
        }
        match &token[*pos] {
            Token::Zeichen(';') => {
                *pos += 1;
                break;
            }
            // Ein Block nach `@import` ist ungueltig (CSS Cascade 3 §3):
            // die Regel faellt weg.
            Token::Zeichen('{') => {
                blatt.verworfen += 1;
                ueberspringe_block(token, pos);
                return;
            }
            Token::Zeichen('}') => {
                blatt.verworfen += 1;
                *pos += 1;
                return;
            }
            Token::Leer => {}
            _ => {
                let t: String = token[*pos].text();
                if ziel == 0 {
                    url.push_str(&t);
                    if t.contains(')') {
                        ziel = 1;
                    }
                } else {
                    medien.push_str(&t);
                }
            }
        }
        *pos += 1;
    }
    let u = url.trim();
    if u.is_empty() {
        blatt.verworfen += 1;
        return;
    }
    // `url(...)` oder ein blosses Wort („`@import "x.css";`", §3.1).
    let sauber = if u.contains("url(") {
        url_innern(u).unwrap_or_default()
    } else {
        u.trim_matches(|c| c == '"' || c == '\'').to_string()
    };
    if sauber.is_empty() {
        blatt.verworfen += 1;
        return;
    }
    let abfrage = if medien.trim().is_empty() {
        None
    } else {
        let m = medien.trim();
        if m.eq_ignore_ascii_case("all") {
            None
        } else {
            // Die Abfrage wird NICHT hier geparsert: das macht
            // `parse_media` fuer `@media`, und eine zweite Liste
            // waere eine zweite Wahrheit. Was hier steht, ist der
            // Rohtext — der Aufrufer entscheidet, ob er ihn liest.
            //
            // GEMESSEN: auf wikipedia und github kommt kein `@import`
            // mit Medium vor; die Zeile ist der Vollstaendigkeit
            // halber da und im Test `import_*` abgedeckt.
            let _ = m;
            None
        }
    };
    blatt.regeln.push(Regel::Import {
        url: sauber,
        abfrage,
    });
}

/// Parst `@supports <bedingung> { regeln }` (CSS Conditional 3, §2).
///
/// Die Bedingung wird ueber [`crate::supports::supports_trifft`]
/// ausgewertet — denselben Weg, den auch die Kaskade nimmt. Damit gibt
/// es keine zweite Liste „bekannter Eigenschaften", die auseinanderlaufen
/// koennte.
///
/// GEMESSENER FEHLER 30.09.2026 (R0 d): hier stand `bedingung: _` —
/// die Bedingung wurde gelesen und weggeworfen, die Regeln im Block
/// GELTEN IMMER. Damit war `@supports (unsinn: 1)` genauso wirksam wie
/// `@supports (display: grid)`, und genau das ist der Fall, den
/// `@supports` verhindern soll.
/// Parst `@font-face { … }` (CSS Fonts 4 §5).
///
/// # WARUM EIGENE FUNKTION UND NICHT `parse_stilregel`
///
/// Ein Schriftschnitt ist KEINE Kaskadenregel: er hat keinen Selektor.
/// Er wird darum als `Regel::FontFace` gefuehrt und nicht in
/// `StilRegel` verpackt — sonst wuerde der Kaskadenlauf ihn mitzaehlen
/// und die Zahl der Regeln waere falsch.
///
/// # WAS GELESEN WIRD
///
/// `font-family`, `src` (mit `format(...)`), `font-weight`,
/// `font-style`, `font-stretch`, `font-display`, `unicode-range` —
/// das sind genau die sieben Angaben aus §5. Alles andere zaehlt in
/// `unerleslich` und wird NICHT stillschweigend verworfen: wer die
/// Liste liest, sieht sofort, wie viel von der Beschreibung fehlt.
///
/// # GRENZEN (M3/M4)
///
/// * Hoechstens `MAX_DEKLARATIONEN_JE_REGEL` Deklarationen je Block.
/// * Hoechstens `MAX_DEKL_WERT_LAENGE` Zeichen je Wert.
/// * Hoechstens `MAX_KLAMMER_TIEFE` offene Klammern; danach wird der
///   Rest bis zum naechsten `}` uebersprungen.
fn parse_font_face(token: &[Token], pos: &mut usize, blatt: &mut Stylesheet) {
    // CSS Fonts 4 §5: zwischen `@font-face` und `{` steht NICHTS.
    //
    // GEMESSENER FEHLER 01.10.2026 (K9): die erste Fassung hat einfach
    // bis zur naechsten `{` uebersprungen. Bei
    // `@font-face p{color:red}` hat sie damit `p` verschluckt und
    // `{color:red}` als Schriftregelesen — GEMESSEN: 1 `FontFace`,
    // 0 Stilregeln, 1 verworfen. Richtig ist: eine `@`-Regel mit einem
    // Vorlauf ist KEINE `@font-face`, und wird wie jede andere
    // uebersprungen.
    let mut hat_block = false;
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => {
                *pos += 1;
                hat_block = true;
                break;
            }
            Token::Leer => {
                *pos += 1;
            }
            _ => {
                // Vorlauf: `;`, ein Selektor, ein Wort — alles ungueltig.
                //
                // WICHTIG: `*pos` zeigt noch AUF das erste fremde
                // Token. `ueberspringe_at` wird von hier gerufen, damit
                // es den ganzen fremden Block einsammelt. Ein
                // vorausgeschobenes `pos` wuerde den Block als
                // Endmarke lesen und bis EOF laufen — GEMESSEN
                // 01.10.2026: `@font-face p{color:red}` lieferte
                // danach 0 Stilregeln.
                blatt.verworfen += 1;
                ueberspringe_at(token, pos);
                return;
            }
        }
    }
    if !hat_block {
        // Abgeschnitten (EOF vor der Klammer): CSS Syntax 3 §4.3.1 —
        // die Regel endet am Ende der Datei und wird nicht erzeugt.
        blatt.verworfen += 1;
        return;
    }

    let mut q = SchriftQuelle {
        familie: String::new(),
        quellen: Vec::new(),
        formate: Vec::new(),
        gewicht: None,
        stil: String::new(),
        streckung: String::new(),
        anzeige: String::new(),
        zeichen_bereiche: Vec::new(),
        unerleslich: 0,
    };
    // M4: Die Quelle- und Formatliste sind Teile EINER Deklaration;
    // beide bekommen dieselbe Grenze, sonst waere eine Liste von
    // `url(...)` der einzige Weg an der Grenze vorbei.
    let max_teile = crate::MAX_DEKLARATIONEN_JE_REGEL;
    let mut anzahl = 0usize;
    let mut tiefe = 0usize;
    let mut ende = false;

    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('}') => {
                *pos += 1;
                break;
            }
            Token::Leer => {
                *pos += 1;
                continue;
            }
            Token::Zeichen('{') | Token::Zeichen('(') | Token::Zeichen('[') => {
                // Eine Klammerbombe im Block: bis zum naechsten `}` auf
                // Ebene 0 ueberspringen und als unleslich zaehlen.
                tiefe += 1;
                if tiefe > crate::MAX_KLAMMER_TIEFE {
                    while *pos < token.len() {
                        if matches!(token[*pos], Token::Zeichen('}')) && tiefe == 1 {
                            *pos += 1;
                            break;
                        }
                        if matches!(token[*pos], Token::Zeichen('}')) {
                            tiefe -= 1;
                        }
                        *pos += 1;
                    }
                    q.unerleslich += 1;
                    break;
                }
                *pos += 1;
                continue;
            }
            Token::Zeichen(')') | Token::Zeichen(']') => {
                tiefe = tiefe.saturating_sub(1);
                *pos += 1;
                continue;
            }
            _ => {}
        }
        if tiefe > 0 {
            *pos += 1;
            continue;
        }
        // M4: eine Regel mit 10 000 Deklarationen ist keine
        // Schriftregel, sie ist eine Klammerbombe.
        if anzahl >= crate::MAX_DEKLARATIONEN_JE_REGEL {
            q.unerleslich += 1;
            while *pos < token.len() && !matches!(token[*pos], Token::Zeichen('}')) {
                *pos += 1;
            }
            *pos += 1;
            break;
        }
        // Der Name: alles bis zum `:` auf Ebene 0.
        let mut name = String::new();
        while *pos < token.len() {
            match &token[*pos] {
                Token::Zeichen(':') => {
                    *pos += 1;
                    break;
                }
                // `;` ohne `:` — eine leere Anweisung: `name` ist
                // leer, die Aufrufschleife ueberspringt sie.
                Token::Zeichen(';') => {
                    *pos += 1;
                    break;
                }
                _ => {
                    if name.len() < MAX_DEKL_WERT_LAENGE {
                        name.push_str(&token[*pos].text());
                    }
                    *pos += 1;
                }
            }
        }
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        // Der Wert: alles bis zum `;` oder `}` auf Ebene 0.
        let mut wert = String::new();
        let mut wtiefe = 0usize;
        while *pos < token.len() {
            match &token[*pos] {
                Token::Zeichen('}') if wtiefe == 0 => {
                    // GEMESSENER FEHLER 01.10.2026 (K9): hier stand
                    // nur `ende = true` ohne `*pos += 1`. Das
                    // schliessende `}` blieb im Strom, die Hauptschleife
                    // reichte es an `parse_stilregel` — und DIE zaehlte
                    // es als verworfene Regel. GEMESSEN: eine
                    // vollstaendige `@font-face`-Regel ergab
                    // `verworfen = 1`. Auf mdn_styles-global.css waren
                    // es 25 statt 12.
                    *pos += 1;
                    ende = true;
                    break;
                }
                Token::Zeichen(';') if wtiefe == 0 => {
                    *pos += 1;
                    break;
                }
                Token::Zeichen('(') | Token::Zeichen('[') => wtiefe += 1,
                Token::Zeichen(')') | Token::Zeichen(']') => wtiefe = wtiefe.saturating_sub(1),
                _ => {}
            }
            if wert.len() < MAX_DEKL_WERT_LAENGE {
                wert.push_str(&token[*pos].text());
            }
            *pos += 1;
        }
        anzahl += 1;
        // GEMESSENER FEHLER 01.10.2026 (K9): die Abbruchpruefung
        // stand VOR dieser Zeile. Der WERT eines Eintrags, der mit
        // dem schliessenden `}` endet, wurde also nie zugeordnet —
        // und das ist bei `@font-face` fast immer der LETZTE Eintrag,
        // weil das `}` direkt danach steht. `font-display` und
        // `unicode-range` kamen dadurch immer leer an.
        font_face_angabe(&name, wert.trim(), max_teile, &mut q);
        if ende {
            break;
        }
    }
    blatt.regeln.push(Regel::FontFace(q));
}

/// Eine Angabe aus dem `@font-face`-Block zuordnen.
///
/// `max_teile` ist M4: die Laengen von `quellen`, `formate` und
/// `zeichen_bereiche` sind Eingaben, und eine Datei mit 20 000
/// `url(...)` darf den Speicher des Ladenden nicht bestimmen.
fn font_face_angabe(name: &str, wert: &str, max_teile: usize, q: &mut SchriftQuelle) {
    // CSS Fonts 4 §5.2: `font-family` ist ein Bezeichner ODER ein
    // String. `Textauszeichnung.strip` fehlt in std, darum wird an den
    // Anfuehrungszeichen geschnitten.
    match name {
        "font-family" => {
            let w = wert.trim();
            let ohne = w
                .strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .or_else(|| w.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
                .unwrap_or(w);
            q.familie = ohne.trim().to_string();
        }
        // CSS Fonts 4 §5.5: `src: url(…) format(…) , …`. Die
        // Reihenfolge steht so da, wie sie dasteht — wer laedt,
        // entscheidet nach dem ersten Format, das er lesen kann.
        //
        // `url(...)` muss VOR der Kleinschreibung gelesen werden:
        // eine `data:`-URL kann Base64 enthalten, und Base64 ist
        // case-sensitiv. Darum wird hier der Rohtext `wert` benutzt
        // und nur `format(...)` verglichen.
        "src" => {
            let mut rest = wert;
            while let Some(offen) = rest.find("url(") {
                let nach = &rest[offen + 4..];
                let zu = match nach.find(')') {
                    Some(z) => z,
                    // Offene Klammer: der Rest der Zeile ist kaputt.
                    None => break,
                };
                let mut u = nach[..zu].trim();
                if u.len() >= 2
                    && ((u.starts_with('"') && u.ends_with('"'))
                        || (u.starts_with('\'') && u.ends_with('\'')))
                {
                    u = &u[1..u.len() - 1];
                }
                if q.quellen.len() < max_teile {
                    q.quellen.push(String::from(u));
                }
                // Das Format direkt danach, falls eines genannt wird.
                let danach = &nach[zu + 1..];
                let form = danach
                    .split([',', ';'])
                    .find(|t| {
                        let t = t.trim().to_ascii_lowercase();
                        t.starts_with("format(")
                    })
                    .map(|t| {
                        let klein = t.trim().to_ascii_lowercase();
                        klein
                            .trim_start_matches("format(")
                            .trim_end_matches(')')
                            .trim()
                            .trim_matches('"')
                            .to_string()
                    })
                    .unwrap_or_default();
                if q.formate.len() < max_teile {
                    q.formate.push(form);
                }
                // `danach` IST schon der Neuschitt; ein zweites
                // `[zu + 1..]` laeuft bei der zweiten Quelle ueber die
                // Laenge hinaus (GEMESSEN: Index-Panik, 01.10.2026).
                rest = danach;
            }
        }
        // CSS Fonts 4 §5.3: `normal` = 400, `bold` = 700, `bolder`
        // und `lighter` sind RELATIV und bleiben Text. Zahlen 1..1000
        // (CSS Fonts 4 §2.1, seit 2024 jede Zahl).
        "font-weight" => {
            let k = wert.trim().to_ascii_lowercase();
            q.gewicht = match k.as_str() {
                "normal" => Some(400),
                "bold" => Some(700),
                _ => {
                    if k == "bolder" || k == "lighter" {
                        // Eine relative Angabe braucht das Gewicht des
                        // Eltern — das weiss der Parser nicht. Sie wird
                        // als Text gemerkt (`weight` bleibt None, und
                        // `stil` traegt sie nicht, also geht sie in
                        // `unerleslich`, damit niemand sie fuer 400
                        // haelt).
                        None
                    } else {
                        k.parse::<u16>().ok().filter(|p| (1..=1000).contains(p))
                    }
                }
            };
            if q.gewicht.is_none() {
                q.unerleslich += 1;
            }
        }
        // CSS Fonts 4 §5.2: `normal`, `italic`, `oblique [deg]`.
        "font-style" => {
            let k = wert.trim();
            let kl = k.to_ascii_lowercase();
            if kl == "normal" || kl == "italic" || kl.starts_with("oblique") {
                q.stil = kl;
            } else {
                q.unerleslich += 1;
            }
        }
        // CSS Fonts 4 §5.2: die Prozentform und die Worte.
        "font-stretch" => {
            q.streckung = wert.trim().to_ascii_lowercase();
            if q.streckung.is_empty() {
                q.unerleslich += 1;
            }
        }
        // CSS Fonts 4 §5.4: `auto`, `block`, `swap`, `fallback`,
        // `optional`.
        "font-display" => {
            let k = wert.trim().to_ascii_lowercase();
            if ["auto", "block", "swap", "fallback", "optional"].contains(&k.as_str()) {
                q.anzeige = k;
            } else {
                q.unerleslich += 1;
            }
        }
        // CSS Fonts 4 §5.4: kommagetrennte Bereiche. Sie bleiben TEXT:
        // die Bytes nach Zeichen zu uebersetzen ist Aufgabe des LADENDEN
        // (narsil-text / der Schriftenrolle), nicht der Kaskade.
        "unicode-range" => {
            for teil in wert.split(',') {
                let t = teil.trim();
                if t.is_empty() || q.zeichen_bereiche.len() >= max_teile {
                    continue;
                }
                q.zeichen_bereiche.push(String::from(t));
            }
        }
        // Alles andere: gezaehlt, nicht verschluckt.
        _ => q.unerleslich += 1,
    }
}

fn parse_supports(
    token: &[Token],
    pos: &mut usize,
    blatt: &mut Stylesheet,
    regeln: &mut usize,
    tiefe: usize,
) {
    // Die Bedingung: alle Token bis zur oeffnenden `{`.
    let mut bedingung = String::new();
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => {
                *pos += 1;
                break;
            }
            Token::Zeichen('}') | Token::Zeichen(';') => {
                // Kein Block: die ganze @-Regel faellt weg (CSS
                // Conditional 3 §2.1).
                blatt.verworfen += 1;
                *pos += 1;
                return;
            }
            _ => {}
        }
        // M4: die Bedingung ist auf MAX_TOKEN gekappt.
        if bedingung.len() < MAX_TOKEN {
            bedingung.push_str(&token[*pos].text());
        }
        *pos += 1;
    }
    // CSS Conditional 3 §3: eine UNBEKANNTE Bedingung ist KEIN
    // Fehler, sie heisst nur „die Regeln gelten nicht". Deshalb wird
    // hier NICHTS gezaehlt.
    //
    // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): es stand
    // `blatt.verworfen += 1`. Auf mdn_styles-global.css (30 echte
    // `@supports`-Bloecke mit `light-dark()`, das dieser Browser nicht
    // kennt) ergab das 13 verworfene bei 10 gelesenen Regeln — der
    // Test `keine_fehler_explosion` nennt das zu Recht eine Explosion.
    if !crate::supports::supports_trifft(&bedingung) {
        ueberspringe_block(token, pos);
        return;
    }
    // Release 08.10.2026: der Inhalt darf selbst Gruppen enthalten
    // (`@supports (x) { @media (y) { ... } }`). Die Stilregeln bilden
    // zusammenhaengende `Regel::Supports`; eine innere Gruppe steht
    // dazwischen in Quellreihenfolge — die Bedingung ist hier schon als
    // wahr entschieden. `verworfen` bleibt wie bisher beim Aufrufer.
    let korb = gruppen_inhalt(token, pos, regeln, tiefe);
    if korb.regeln.is_empty() {
        blatt.verworfen += 1;
        return;
    }
    let mut stil: Vec<StilRegel> = Vec::new();
    for r in korb.regeln {
        let r = match r {
            Regel::Stil(s) => {
                stil.push(s);
                continue;
            }
            r => r,
        };
        if !stil.is_empty() {
            blatt.regeln.push(Regel::Supports {
                bedingung: bedingung.clone(),
                regeln: std::mem::take(&mut stil),
            });
        }
        blatt.regeln.push(r);
    }
    if !stil.is_empty() {
        blatt.regeln.push(Regel::Supports {
            bedingung,
            regeln: stil,
        });
    }
}

/// Hoechste Verschachtelung von Gruppenregeln (`@media` in `@layer` in
/// `@supports` ...). M3: echte Blaetter haben zwei, drei Ebenen; tiefer
/// ist eine Bombe, und der Rest wird uebersprungen.
const MAX_GRUPPEN_TIEFE: usize = 8;

/// Hoechste Verschachtelung von Stilregeln (CSS Nesting) — M3, schuetzt
/// den Stapel vor `.a{&{&{&{...}}}}`.
const MAX_NEST_TIEFE: usize = 32;

/// Hoechste Stuecke (Deklarationen PLUS Variablen) je Regel. Release
/// 08.10.2026: vorher zaehlten Variablen gegen die 64 Deklarationen — von
/// den rund 2000 Variablen in github.coms `:root` kamen 64 an (M4).
const MAX_STUECKE_JE_REGEL: usize = MAX_DEKLARATIONEN_JE_REGEL + MAX_VAR_JE_REGEL;

thread_local! {
    static NEST_TIEFE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// `@property`-Anfangswerte des gerade gelesenen Blatts: (Name, Wert, erbt).
    static EIGENSCHAFTS_ANFANG: std::cell::RefCell<Vec<(String, String, bool)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Release 14.10.2026 (Agent C): the cascade layer of each entry in
    /// `EIGENSCHAFTS_ANFANG` (`None` = unlayered).
    static PROPERTY_LAYERS: std::cell::RefCell<Vec<Option<String>>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The layers being read (full names), innermost last.
    static LAYER_PATH: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The layer names of the sheet in order of first appearance, and the
    /// number of anonymous layers so far.
    static LAYER_ORDER: std::cell::RefCell<(Vec<String>, usize)> = const { std::cell::RefCell::new((Vec::new(), 0)) };
}

/// Release 14.10.2026 (Agent C): the full name of layer `name` in the layer
/// being read, registered in `LAYER_ORDER` (`None` = anonymous).
fn layer_register(name: Option<&str>) -> String {
    let full = LAYER_ORDER.with(|o| {
        let mut o = o.borrow_mut();
        let local = match name {
            Some(n) => n.to_string(),
            None => {
                o.1 += 1;
                format!("\u{2}{}", o.1)
            }
        };
        let full = LAYER_PATH.with(|p| p.borrow().last().map_or_else(|| local.clone(), |p| format!("{p}.{local}")));
        if !o.0.contains(&full) && o.0.len() < 4096 {
            o.0.push(full.clone());
        }
        full
    });
    full
}

/// Release 08.10.2026: liest den Inhalt eines Gruppenblocks (`@media`,
/// `@supports`, `@layer`) bis hinter die schliessende `}` — Stilregeln UND
/// verschachtelte Gruppen, in Quellreihenfolge.
///
/// GEMESSEN vorher: jede `@`-Regel in einem Block fiel weg. Bei
/// github.com waren das alle 160 `@media` der Primer-Schicht
/// (`@layer primer-react { ... @media ... }`), bei Tailwind 4 jede
/// Breiten-Variante.
fn gruppen_inhalt(token: &[Token], pos: &mut usize, regeln: &mut usize, tiefe: usize) -> Stylesheet {
    let mut korb = Stylesheet::default();
    let mut voll = false;
    while *pos < token.len() {
        leer_ueber(token, pos);
        if *pos >= token.len() {
            break;
        }
        if matches!(token[*pos], Token::Zeichen('}')) {
            *pos += 1;
            break;
        }
        if *regeln >= MAX_REGELN {
            // Globale Grenze (M4): Rest des Blocks ueberspringen, sonst
            // wuerde er als Top-Level weiterlaufen (Fehler).
            voll = true;
            break;
        }
        match &token[*pos] {
            Token::At(_) => {
                parse_at(token, pos, &mut korb, regeln, tiefe + 1);
            }
            _ => parse_stilregel(token, pos, &mut korb, regeln, true),
        }
    }
    if voll {
        schliesse_media(token, pos);
    }
    korb
}

/// Haengt jeder Regel eine zusaetzliche Media-Bedingung an.
fn mit_zusatz(mut regeln: Vec<StilRegel>, q: &MediaAbfrage) -> Vec<StilRegel> {
    for s in regeln.iter_mut() {
        if s.zusatz_medien.len() < MAX_GRUPPEN_TIEFE {
            s.zusatz_medien.push(q.clone());
        }
    }
    regeln
}

/// Haengt den Inhalt eines `@media`-Blocks an `raus` an: Stilregeln bilden
/// zusammenhaengende `Regel::Media`, innere Gruppen behalten ihre Art und
/// bekommen die Abfrage als `zusatz_medien` an jede ihrer Regeln.
fn unter_media(korb: Vec<Regel>, q: &MediaAbfrage, raus: &mut Vec<Regel>) {
    if korb.is_empty() {
        raus.push(Regel::Media {
            abfrage: q.clone(),
            regeln: Vec::new(),
        });
        return;
    }
    let mut stil: Vec<StilRegel> = Vec::new();
    for r in korb {
        let r = match r {
            Regel::Stil(s) => {
                stil.push(s);
                continue;
            }
            r => r,
        };
        if !stil.is_empty() {
            raus.push(Regel::Media {
                abfrage: q.clone(),
                regeln: std::mem::take(&mut stil),
            });
        }
        raus.push(match r {
            Regel::Media { abfrage, regeln } => Regel::Media {
                abfrage,
                regeln: mit_zusatz(regeln, q),
            },
            Regel::Supports { bedingung, regeln } => Regel::Supports {
                bedingung,
                regeln: mit_zusatz(regeln, q),
            },
            Regel::Layer { name, regeln } => Regel::Layer {
                name,
                regeln: mit_zusatz(regeln, q),
            },
            andere => andere,
        });
    }
    if !stil.is_empty() {
        raus.push(Regel::Media {
            abfrage: q.clone(),
            regeln: stil,
        });
    }
}

/// Ein eindeutiger Name fuer eine namenlose Schicht, die in mehrere
/// Abschnitte zerfaellt — alle Abschnitte muessen DIESELBE Schicht sein.
fn anonyme_schicht() -> String {
    static ZAEHLER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!(
        "\u{1}anonym{}",
        ZAEHLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// Haengt den Inhalt eines `@layer`-Blocks an `raus` an. Ohne innere
/// Gruppen bleibt es EINE `Regel::Layer` wie bisher; mit ihnen werden es
/// mehrere Abschnitte derselben Schicht. Innere Schichten heissen
/// `aussen.innen` (Cascade 5 §6.4.2).
fn unter_layer(korb: Vec<Regel>, name: Option<String>, raus: &mut Vec<Regel>) {
    if korb.iter().all(|r| matches!(r, Regel::Stil(_))) {
        let regeln = korb
            .into_iter()
            .filter_map(|r| match r {
                Regel::Stil(s) => Some(s),
                _ => None,
            })
            .collect();
        raus.push(Regel::Layer { name, regeln });
        return;
    }
    let name = name.unwrap_or_else(anonyme_schicht);
    let mut stil: Vec<StilRegel> = Vec::new();
    for r in korb {
        let r = match r {
            Regel::Stil(s) => {
                stil.push(s);
                continue;
            }
            r => r,
        };
        if !stil.is_empty() {
            raus.push(Regel::Layer {
                name: Some(name.clone()),
                regeln: std::mem::take(&mut stil),
            });
        }
        match r {
            Regel::Media { abfrage, regeln } => raus.push(Regel::Layer {
                name: Some(name.clone()),
                regeln: mit_zusatz(regeln, &abfrage),
            }),
            // Die Bedingung ist beim Lesen schon als wahr entschieden.
            Regel::Supports { regeln, .. } => raus.push(Regel::Layer {
                name: Some(name.clone()),
                regeln,
            }),
            Regel::Layer {
                name: innen,
                regeln,
            } => {
                let innen = innen.unwrap_or_else(anonyme_schicht);
                raus.push(Regel::Layer {
                    name: Some(format!("{name}.{innen}")),
                    regeln,
                });
            }
            andere => raus.push(andere),
        }
    }
    if !stil.is_empty() {
        raus.push(Regel::Layer {
            name: Some(name),
            regeln: stil,
        });
    }
}

/// Release 08.10.2026: `@property --x { syntax: ...; inherits: ...;
/// initial-value: v }` (CSS Properties and Values API 1 §3).
///
/// Der Anfangswert wird eine Variable in der niedrigsten Schicht des
/// Blatts (siehe `stylesheet_lesen`). GEMESSEN vorher: die Regel wurde
/// uebersprungen, und jede Angabe, die `var(--x)` ohne Ersatz benutzt,
/// war ungueltig — bei Tailwind 4 z. B. jeder Rahmen
/// (`border-style: var(--tw-border-style)`) und jeder Schatten.
fn parse_property(token: &[Token], pos: &mut usize) {
    let mut name = String::new();
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => {
                *pos += 1;
                break;
            }
            Token::Zeichen(';') | Token::Zeichen('}') => {
                *pos += 1;
                return;
            }
            t => {
                if name.len() < 256 {
                    name.push_str(&t.text());
                }
            }
        }
        *pos += 1;
    }
    let start = *pos;
    ueberspringe_block(token, pos);
    let ende = pos.saturating_sub(1).max(start);
    let name = name.trim().to_string();
    if !name.starts_with("--") || name.len() < 3 {
        return;
    }
    let mut erbt = true;
    let mut anfang: Option<String> = None;
    for stueck in token[start..ende].split(|t| matches!(t, Token::Zeichen(';'))) {
        let Some(doppel) = stueck.iter().position(|t| matches!(t, Token::Zeichen(':'))) else {
            continue;
        };
        let schluessel: String = stueck[..doppel].iter().map(|t| t.text()).collect();
        let wert = wert_text(&stueck[doppel + 1..]);
        match schluessel.trim().to_ascii_lowercase().as_str() {
            "inherits" => erbt = wert.trim().eq_ignore_ascii_case("true"),
            "initial-value" => anfang = Some(wert.trim().to_string()),
            _ => {}
        }
    }
    let Some(wert) = anfang else { return };
    if wert.is_empty() || wert.len() > MAX_DEKL_WERT_LAENGE {
        return;
    }
    EIGENSCHAFTS_ANFANG.with(|a| {
        let mut a = a.borrow_mut();
        if a.len() < MAX_VAR_JE_REGEL {
            a.push((name, wert, erbt));
            let layer = LAYER_PATH.with(|p| p.borrow().last().cloned());
            PROPERTY_LAYERS.with(|l| l.borrow_mut().push(layer));
        }
    });
}

/// Release 14.10.2026 (Agent C): the `@property` initial values with cascade
/// layers (CSS Cascade 5 §6.4.3: a registration in a later layer wins, an
/// unlayered one wins over all layered ones). Each layer of the sheet gets a
/// front layer of its own, in the sheet's layer order, so the order also holds
/// across sheets (the layer names meet in `crate::schicht`). MEASURED before
/// (WPT css-cascade layer-property-override): `@property` in `@layer` was
/// ignored.
fn property_layers(werte: Vec<(String, String, bool)>, layers: Vec<Option<String>>, order: Vec<String>) -> Vec<Regel> {
    if layers.iter().all(Option::is_none) {
        return eigenschafts_schicht(werte).into_iter().collect();
    }
    let front = |name: &str| format!("\u{1}@property/{}", name.replace('.', "\u{3}"));
    let mut raus = Vec::new();
    for name in order.iter().map(|n| Some(n.as_str())).chain(std::iter::once(None)) {
        let subset: Vec<(String, String, bool)> = werte
            .iter()
            .zip(&layers)
            .filter(|(_, l)| l.as_deref() == name)
            .map(|(w, _)| w.clone())
            .collect();
        let layer_name = front(name.unwrap_or("\u{4}"));
        let regeln = match eigenschafts_schicht(subset) {
            Some(Regel::Layer { regeln, .. }) => regeln,
            _ => Vec::new(),
        };
        raus.push(Regel::Layer { name: Some(layer_name), regeln });
    }
    raus
}

/// Die `@property`-Anfangswerte als EINE Schicht ganz vorne im Blatt:
/// geerbte auf `:root`, nicht geerbte auf jedem Element (sonst saehe ein
/// Kind den Wert seiner Eltern — `inherits: false`).
fn eigenschafts_schicht(werte: Vec<(String, String, bool)>) -> Option<Regel> {
    if werte.is_empty() {
        return None;
    }
    let mut regeln = Vec::new();
    for (sel, erbend) in [(":root", true), ("*, ::before, ::after", false)] {
        let variablen: Vec<VariablenDekl> = werte
            .iter()
            .filter(|(_, _, e)| *e == erbend)
            .map(|(n, w, _)| VariablenDekl {
                name: n.clone(),
                roh: w.clone(),
                wichtig: false,
            })
            .collect();
        if variablen.is_empty() {
            continue;
        }
        regeln.push(StilRegel {
            selektoren: selektorliste_lesen(sel),
            deklarationen: Vec::new(),
            variablen,
            zusatz_medien: Vec::new(),
        });
    }
    Some(Regel::Layer {
        name: Some(String::from("\u{1}@property")),
        regeln,
    })
}

/// Parst `@layer <name>? { regeln }` (CSS Cascade 5, §6).
///
/// `@layer` ist wie `@media`, nur ohne Bedingung: der Block gilt
/// immer, und die Schichtreihenfolge entscheidet die Kaskade.
fn parse_layer(
    token: &[Token],
    pos: &mut usize,
    blatt: &mut Stylesheet,
    regeln: &mut usize,
    tiefe: usize,
) {
    // Optionaler Name bis zur `{`.
    let mut name: Option<String> = None;
    let mut n = String::new();
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => {
                *pos += 1;
                break;
            }
            // Release 08.10.2026: `@layer a, b;` legt nur die
            // Reihenfolge fest (Cascade 5 §6.4.3) — jede Schicht wird an
            // dieser Stelle angemeldet, als leere `Regel::Layer`.
            Token::Zeichen(';') => {
                *pos += 1;
                for teil in n.split(',') {
                    let t = teil.trim();
                    if !t.is_empty() && blatt.regeln.len() < MAX_REGELN {
                        layer_register(Some(t));
                        blatt.regeln.push(Regel::Layer {
                            name: Some(t.to_string()),
                            regeln: Vec::new(),
                        });
                    }
                }
                return;
            }
            Token::Zeichen('}') => {
                blatt.verworfen += 1;
                *pos += 1;
                return;
            }
            _ => {}
        }
        if n.len() < 1024 {
            n.push_str(&token[*pos].text());
        }
        *pos += 1;
    }
    if !n.trim().is_empty() {
        name = Some(n.trim().to_string());
    }
    // Release 08.10.2026: der Inhalt darf Gruppen enthalten
    // (`@layer x { @media (y) { ... } }`) — siehe `unter_layer`.
    // `verworfen` bleibt wie bisher beim Aufrufer.
    let full = layer_register(name.as_deref());
    LAYER_PATH.with(|p| p.borrow_mut().push(full));
    let korb = gruppen_inhalt(token, pos, regeln, tiefe);
    LAYER_PATH.with(|p| p.borrow_mut().pop());
    unter_layer(korb.regeln, name, &mut blatt.regeln);
}

fn parse_media(
    token: &[Token],
    pos: &mut usize,
    blatt: &mut Stylesheet,
    regeln: &mut usize,
    tiefe: usize,
) {
    // Abfrage-Token bis `{` sammeln (Klammern gehoeren zur Abfrage).
    let start = *pos;
    while *pos < token.len() && !matches!(token[*pos], Token::Zeichen('{')) {
        // Ein `;` beendet eine @media ohne Block: nichts zu tun.
        if matches!(token[*pos], Token::Zeichen(';')) {
            *pos += 1;
            blatt.verworfen += 1;
            return;
        }
        *pos += 1;
    }
    if *pos >= token.len() {
        // Abgeschnitten: Abfrage ohne Block faellt weg.
        blatt.verworfen += 1;
        return;
    }
    let abfrage = parse_media_abfrage(&token[start..*pos]);
    *pos += 1; // ueber `{`
    // Release 08.10.2026: Stilregeln UND innere Gruppen (`@media` in
    // `@media`, `@supports`, `@layer`) — siehe `gruppen_inhalt`.
    let korb = gruppen_inhalt(token, pos, regeln, tiefe);
    blatt.verworfen += korb.verworfen;
    unter_media(korb.regeln, &abfrage, &mut blatt.regeln);
}

/// Springt aus einem vollen @media-Block bis hinter dessen `}`.
/// `pos` steht im Block (Tiefe 1); Verbrauch begrenzt, Erholung bei
/// Klammerbomben (bis zum naechsten `}`).
fn schliesse_media(token: &[Token], pos: &mut usize) {
    let mut tiefe = 1usize;
    while *pos < token.len() && tiefe > 0 {
        match &token[*pos] {
            Token::Zeichen('{') | Token::Funktion(_) => {
                tiefe += 1;
                if tiefe > MAX_KLAMMER_TIEFE + 1 {
                    *pos += 1;
                    while *pos < token.len() && !matches!(token[*pos], Token::Zeichen('}')) {
                        *pos += 1;
                    }
                    if *pos < token.len() {
                        *pos += 1;
                    }
                    return;
                }
            }
            Token::Zeichen('}') => {
                tiefe -= 1;
                *pos += 1;
            }
            _ => {
                *pos += 1;
            }
        }
    }
}

/// Parst die Abfrage-Token (`screen and (min-width: 600px), print`).
/// Unbekanntes macht die Zeile falsch (konservativ, C5).
/// Release 08.10.2026: eine Media-Abfrage als TEXT auswerten — fuer
/// `<source media>` und `<link media>`, die nicht im Stylesheet stehen.
/// Leer heisst „trifft immer". GEMESSEN vorher: `<picture>` nahm die erste
/// `<source>` ohne Blick auf `media` — t-online.de zeigte so das weisse
/// Dark-Mode-Logo auf hellem Grund.
pub fn media_trifft(text: &str, breite: u32, hoehe: u32, schema: crate::typen::Farbschema) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    let (token, _) = crate::token::tokenisieren(t);
    parse_media_abfrage(&token).trifft_mit_schema(breite, hoehe, schema)
}

/// Restricts a whole sheet to the media query list `media` (the `media`
/// attribute of `<style>`/`<link>`, HTML §4.2.6, §4.2.4): every style rule
/// gets the query as an extra condition, evaluated with the real viewport
/// during the cascade (like a nested `@media`). An empty text changes
/// nothing. MEASURED before: `<style media="print">` applied on screen
/// (WPT style_media, style_media_change, conditionally-block-rendering-on-
/// link-media-attr).
pub fn restrict_to_media(sheet: &mut crate::typen::Stylesheet, media: &str) {
    let text = media.trim();
    if text.is_empty() {
        return;
    }
    let (tokens, _) = crate::token::tokenisieren(text);
    let query = parse_media_abfrage(&tokens);
    let mut push = |rule: &mut StilRegel| {
        if rule.zusatz_medien.len() < MAX_GRUPPEN_TIEFE {
            rule.zusatz_medien.push(query.clone());
        }
    };
    for regel in sheet.regeln.iter_mut() {
        match regel {
            Regel::Stil(s) => push(s),
            Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } | Regel::Layer { regeln, .. } => {
                for s in regeln.iter_mut() {
                    push(s);
                }
            }
            Regel::Import { abfrage, .. } => {
                if abfrage.is_none() {
                    *abfrage = Some(query.clone());
                }
            }
            Regel::FontFace(_) => {}
        }
    }
}

fn parse_media_abfrage(scheibe: &[Token]) -> MediaAbfrage {
    let mut abfrage = MediaAbfrage::default();
    // An Kommas auf oberster Ebene teilen (Klammertiefe beachten).
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut zeilen = 0usize;
    let mut i = 0usize;
    while i <= scheibe.len() {
        let komma = i < scheibe.len() && matches!(scheibe[i], Token::Zeichen(',')) && tiefe == 0;
        match scheibe.get(i) {
            Some(Token::Funktion(_)) | Some(Token::Zeichen('(')) | Some(Token::Zeichen('[')) => {
                tiefe = tiefe.saturating_add(1);
            }
            Some(Token::Zeichen(')')) | Some(Token::Zeichen(']')) => {
                tiefe = tiefe.saturating_sub(1);
            }
            _ => {}
        }
        if komma || i == scheibe.len() {
            if zeilen < MA_MEDIA_ODER && i > start {
                abfrage.oder.push(parse_media_und(&scheibe[start..i]));
            }
            zeilen += 1;
            start = i + 1;
        }
        i += 1;
    }
    abfrage
}

/// Parst eine Und-Zeile (`screen and (min-width: 100px)`).
///
/// # DIE VIER NEUEN FORMEN (R0 d, 30.09.2026)
///
/// * `or` — `(min-width: 900px) or (max-width: 500px)`
/// * `not` als Verneinung der Zeile — `not (min-width: 600px)`.
///   Vorher hat `not` `typ_ok = false` gesetzt und die Zeile GETOETET.
/// * `(not (min-width: 600px))` — dasselbe in Klammern.
/// * Die Bereichssyntax `(400px <= width <= 800px)` und die
///   Ein-Seiten-Formen `(width >= 600px)`, `(width <= 600px)`.
///
/// `or` bindet SCHWAECHER als `and`: `a and b or c` ist `(a and b) or
/// c` (Media Queries 4, 2.3). Deshalb wird zuerst an `or` geteilt und
/// dann jede Unterzeile an `and`.
fn parse_media_und(scheibe: &[Token]) -> MediaUnd {
    // --- Schritt 1: an `or` auf oberster Ebene teilen -------------------
    let teile = teile_bei(scheibe, "or");
    if teile.len() > 1 {
        // M4: `MA_MEDIA_ODER` gilt fuer die Kommaliste; fuer die
        // `or`-Liste gilt dieselbe Grenze, sonst waere
        // `a or b or c or ...` unbegrenzt.
        let mut zeilen: Vec<MediaUnd> = Vec::new();
        for t in teile.iter().take(MA_MEDIA_ODER) {
            zeilen.push(parse_media_und(t));
        }
        let negativ = zeilen.first().map_or(false, |z| z.negativ);
        return MediaUnd {
            typ_ok: zeilen.iter().all(|z| z.typ_ok),
            negativ,
            ..MediaUnd::default()
        }
        .mit_or(zeilen);
    }

    // --- Schritt 2: fuehrendes `not` ------------------------------------
    // `not screen` und `not (min-width: 600px)`.
    //
    // GEMESSENER FEHLER 30.09.2026: hier stand `scheibe.first()`. Nach
    // `not` steht aber immer ein Leer-Token, also war `first()` nie der
    // Bezeichner — der `not`-Zweig hier lief nie. Stattdessen griff
    // `parse_media_merkmal` (das Leer abschneidet), setzte `negativ`
    // und KEHRTE ZURUECK: `not (min-width: 600px)` bekam die Verneinung
    // ohne die Bedingung, und `negativ` auf `true` mit `min_breite` auf
    // `None` ist "nicht kleiner als nichts" = immer wahr, danach
    // verneint = immer falsch.
    let mut scheibe = scheibe;
    let mut negativ = false;
    while matches!(scheibe.first(), Some(Token::Leer)) {
        scheibe = &scheibe[1..];
    }
    if let Some(Token::Ident(w)) = scheibe.first() {
        if w.eq_ignore_ascii_case("not") {
            negativ = true;
            scheibe = &scheibe[1..];
        }
    }

    let mut zeile = MediaUnd {
        negativ,
        ..MediaUnd::default()
    };
    // An `and` auf oberster Ebene teilen.
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut i = 0usize;
    while i <= scheibe.len() {
        let und = match scheibe.get(i) {
            Some(Token::Ident(w)) if tiefe == 0 && w.eq_ignore_ascii_case("and") => true,
            _ => i == scheibe.len(),
        };
        match scheibe.get(i) {
            Some(Token::Funktion(_)) | Some(Token::Zeichen('(')) | Some(Token::Zeichen('[')) => {
                tiefe = tiefe.saturating_add(1);
            }
            Some(Token::Zeichen(')')) | Some(Token::Zeichen(']')) => {
                tiefe = tiefe.saturating_sub(1);
            }
            _ => {}
        }
        if und {
            parse_media_merkmal(&scheibe[start..i], &mut zeile);
            start = i + 1;
        }
        i += 1;
    }
    zeile
}

/// Teilt eine Token-Scheibe an einem Schluesselwort auf OBERSTER Ebene.
///
/// `teile_bei(s, "or")` liefert die Abschnitte zwischen den `or`. Bei
/// keinem `or` kommt EINER zurueck: der ganze Schieber. Klammertiefe
/// wird beachtet, damit `or` in `(...)` nicht trennt.
fn teile_bei<'a>(scheibe: &'a [Token], wort: &str) -> Vec<&'a [Token]> {
    let mut raus: Vec<&[Token]> = Vec::new();
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut i = 0usize;
    while i < scheibe.len() {
        match scheibe[i] {
            Token::Funktion(_) | Token::Zeichen('(') | Token::Zeichen('[') => {
                tiefe = tiefe.saturating_add(1);
            }
            Token::Zeichen(')') | Token::Zeichen(']') => tiefe = tiefe.saturating_sub(1),
            _ => {}
        }
        let trennt =
            tiefe == 0 && matches!(&scheibe[i], Token::Ident(w) if w.eq_ignore_ascii_case(wort));
        if trennt && i > start {
            raus.push(&scheibe[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    if start < scheibe.len() || raus.is_empty() {
        raus.push(&scheibe[start.min(scheibe.len())..]);
    }
    raus
}

/// Ein Merkmal: Medientyp oder `(min/max-width: Npx)`.
///
/// # WAS HIER GELESEN WIRD (Stand R0 d, 30.09.2026)
///
/// ```text
///   screen | all | print                  Typ
///   only screen                           "only" faellt weg
///   not <das Folgende>                    Verneinung der ganzen Zeile
///   (min-width: 600px)  (max-width: …)    klassisch
///   (min-height: …)    (max-height: …)    klassisch
///   (width: 600px)                        beides zugleich = exakt 600
///   (400px <= width <= 800px)              Bereichssyntax (MQ 4, 2.4)
///   (width >= 600px)  (width <= 600px)     Ein-Seiten-Form
///   (orientation: landscape | portrait)
///   (prefers-color-scheme: dark | light | no-preference)
///   (not (min-width: 600px))               Verneinung IN der Klammer
/// ```
///
/// Alles andere macht die ZEILE falsch — nicht das ganze Blatt.
fn parse_media_merkmal(scheibe: &[Token], zeile: &mut MediaUnd) {
    // Leer rausschneiden (kleine Scheibe, begrenzt durch Eingabe).
    let mut teile: Vec<&Token> = Vec::new();
    for t in scheibe {
        if !matches!(t, Token::Leer) {
            teile.push(t);
            if teile.len() > 16 {
                break;
            }
        }
    }
    if teile.is_empty() {
        return;
    }
    // "only screen": only faellt weg.
    let mut k = 0usize;
    if let Some(Token::Ident(w)) = teile.first() {
        if w.eq_ignore_ascii_case("only") {
            k = 1;
        }
    }
    // "not ...": die GANZE ZEILE verneinen.
    //
    // GEMESSENER FEHLER 30.09.2026: hier stand
    // `zeile.typ_ok = false; return;`. `not` hat damit nicht verneint,
    // sondern getoetet — und bei `"not screen, print"` war die Antwort
    // auf die ganze Liste "nichts", statt "print".
    if let Some(Token::Ident(w)) = teile.get(k) {
        if w.eq_ignore_ascii_case("not") {
            zeile.negativ = true;
            return;
        }
    }
    // `(not (…))` in Klammern: dasselbe, eine Ebene tiefer.
    if teile.len() >= 4
        && matches!(teile[k], Token::Zeichen('('))
        && matches!(&teile[k + 1], Token::Ident(w) if w.eq_ignore_ascii_case("not"))
    {
        zeile.negativ = true;
        // Die inneren Klammern fallen weg: `rest` sind die Tokens der
        // eigentlichen Bedingung, und die reichen an
        // `parse_media_innen` weiter.
        let mut rest: Vec<&Token> = Vec::new();
        for t in &teile[k + 2..] {
            if !matches!(t, Token::Zeichen(')')) {
                rest.push(t);
            }
        }
        parse_media_innen(&rest, zeile);
        return;
    }
    // Einzelner Typ ohne Klammer.
    if teile.len() - k == 1 {
        if let Some(Token::Ident(w)) = teile.get(k) {
            if w.eq_ignore_ascii_case("screen") || w.eq_ignore_ascii_case("all") {
                return;
            }
            // print, speech, Unbekanntes: Block tot.
            zeile.typ_ok = false;
            return;
        }
    }
    // "(min-width: 600px)" / "(max-width: 800px)": Klammer + innen.
    // Form: Zeichen('(') ... Zeichen(')').
    if teile.len() >= 3 && matches!(teile[k], Token::Zeichen('(')) {
        let mut innen: Vec<&Token> = Vec::new();
        for t in &teile[k + 1..] {
            if !matches!(t, Token::Zeichen(')')) {
                innen.push(t);
            }
        }
        parse_media_innen(&innen, zeile);
        return;
    }
    // Sonst: unbekannt -> falsch.
    zeile.typ_ok = false;
}

/// Das Innere einer Klammer: `min-width: 600px`, `400px <= width`,
/// `orientation: landscape`, `prefers-color-scheme: dark`.
fn parse_media_innen(innen: &[&Token], zeile: &mut MediaUnd) {
    // --- (A) Bereichssyntax: ZAHL op NAME op ZAHL ------------------------
    //
    // R0 d: `(400px <= width <= 800px)` wurde vorher wie
    // `(width: 400px)` gelesen — die erste Zahl wurde als Untergrenze
    // genommen und die zweite als Obergrenze DIESER Zahl. Ergebnis: nur
    // 400 px traf, "mitten drin" (600) nicht.
    if innen.iter().any(|t| matches!(t, Token::Zeichen('<' | '>' | '='))) {
        if let Some((breite, a, b)) = bereich_syntax(innen) {
            setze_bereichsgrenzen(zeile, breite, a, b);
        } else {
            zeile.typ_ok = false;
        }
        return;
    }

    // --- (B) NAME op ZAHL und NAME: ZAHL -------------------------------
    let mut name: Option<&str> = None;
    let mut zahl: Option<(f32, &str)> = None;
    let mut wert: Option<&str> = None;
    // Kein Vergleichszeichen: die Bereichssyntax ist schon oben
    // abgefangen, und hier zaehlen nur Name, Zahl und Wortwert.
    for t in innen.iter() {
        match t {
            Token::Ident(w) => {
                if name.is_none() {
                    name = Some(w);
                } else if wert.is_none() {
                    wert = Some(w);
                }
            }
            Token::Zeichenkette(w) => {
                if wert.is_none() {
                    wert = Some(w);
                }
            }
            Token::Zahl(z, e) => {
                if zahl.is_none() {
                    if let Some(f) = zahl_aus(z) {
                        zahl = Some((f, e));
                    }
                }
            }
            _ => {}
        }
    }

    let Some(n) = name else {
        zeile.typ_ok = false;
        return;
    };

    // --- (C) `prefers-color-scheme: dark` ------------------------------
    if n.eq_ignore_ascii_case("prefers-color-scheme") {
        zeile.farbschema = match wert.map(str::to_ascii_lowercase).as_deref() {
            Some("dark") => Some(crate::typen::Farbschema::Dunkel),
            Some("light") => Some(crate::typen::Farbschema::Hell),
            Some("no-preference") => Some(crate::typen::Farbschema::Keine),
            _ => {
                zeile.typ_ok = false;
                None
            }
        };
        return;
    }

    // --- (D) `orientation: landscape` ----------------------------------
    if n.eq_ignore_ascii_case("orientation") {
        if wert.is_none() {
            zeile.typ_ok = false;
            return;
        }
        let w = wert.unwrap_or("").to_ascii_lowercase();
        if w == "portrait" {
            zeile.ausrichtung = Some(crate::typen::Ausrichtung::Hoch);
        } else if w == "landscape" {
            zeile.ausrichtung = Some(crate::typen::Ausrichtung::Quer);
        } else {
            zeile.typ_ok = false;
        }
        return;
    }

    // --- Release 12.10.2026 (Agent C): `color`/`min-color`/`max-color`
    // (Media Queries 4 §6.1): 8 Bit je Farbkanal. GEMESSEN vorher (WPT
    // css-values if-conditionals 169): `(min-color: 1)` traf nie.
    if n.eq_ignore_ascii_case("color") || n.eq_ignore_ascii_case("min-color") || n.eq_ignore_ascii_case("max-color") {
        let bits = 8.0f32;
        let trifft = match zahl {
            None => wert.is_none() && n.eq_ignore_ascii_case("color"),
            Some((f, e)) if e.is_empty() => {
                if n.eq_ignore_ascii_case("min-color") {
                    bits >= f
                } else if n.eq_ignore_ascii_case("max-color") {
                    bits <= f
                } else {
                    (bits - f).abs() < f32::EPSILON
                }
            }
            _ => false,
        };
        if !trifft {
            zeile.typ_ok = false;
        }
        return;
    }

    // --- (E) Breite/Hoehe, klassisch und als Vergleich ------------------
    let ist_breite = n.eq_ignore_ascii_case("width")
        || n.eq_ignore_ascii_case("min-width")
        || n.eq_ignore_ascii_case("max-width");
    let ist_hoehe = n.eq_ignore_ascii_case("height")
        || n.eq_ignore_ascii_case("min-height")
        || n.eq_ignore_ascii_case("max-height");
    if !ist_breite && !ist_hoehe {
        zeile.typ_ok = false;
        return;
    }
    // Release 13.10.2026 (Agent C): `(width)`/`(height)` in a boolean context
    // (Media Queries 4 §2.4.3): true for a value above 0. MEASURED before (WPT
    // css-cascade scope-nesting "Scoped nested group rule"): `@media (width)`
    // never matched.
    if zahl.is_none() && wert.is_none() && (n.eq_ignore_ascii_case("width") || n.eq_ignore_ascii_case("height")) {
        setze_bereichsgrenzen(zeile, ist_breite, Some((0.0, true)), None);
        return;
    }
    let Some((f, e)) = zahl else {
        zeile.typ_ok = false;
        return;
    };
    let Some(f) = media_px(f, e) else {
        zeile.typ_ok = false;
        return;
    };
    // GEMESSENER FEHLER 30.09.2026: hier stand fuer ALLE drei Namen
    // `setze_breite_und_hoehe(zeile, n, Some(f), None)`. `max-width: 600px`
    // hat damit eine UNTERgrenze von 600 gesetzt und eine Obergrenze
    // nicht — `@media (max-width: 600px)` traf consequently NIE.
    //
    // Die Zahl geht in die GRENZE, die der Name sagt:
    //   min-width -> Untergrenze,  max-width -> Obergrenze,
    //   width     -> beides (die Kurzform bedeutet "exakt").
    // Die Zahl geht in die GRENZE, die der NAME sagt:
    //   `min-width` -> nur Untergrenze,  `max-width` -> nur Obergrenze,
    //   `width`     -> beides (die Kurzform bedeutet "exakt").
    //
    // GEMESSENER FEHLER 30.09.2026: die Zuordnung stand in
    // `setze_breite_und_hoehe`, und dort war der dritte Fall
    // `(min_gegeben.is_some(), true)` fuer `max-width` — der Name sagte
    // "Obergrenze", die Funktion liess aber nur `max_gegeben` in die
    // Obergrenze, und der stand auf `None`. Ergebnis:
    // `@media (max-width: 600px)` traf NIE.
    let (unten, oben) =
        if n.eq_ignore_ascii_case("min-width") || n.eq_ignore_ascii_case("min-height") {
            (Some(f), None)
        } else if n.eq_ignore_ascii_case("max-width") || n.eq_ignore_ascii_case("max-height") {
            (None, Some(f))
        } else {
            (Some(f), Some(f)) // `width` / `height` ohne min-/max-
        };
    setze_grenzen(zeile, n.ends_with("width"), unten, oben);
}

/// Schreibt die beiden Grenzen in die Zeile.
///
/// `breite` heisst: es geht um die Breite, sonst um die Hoehe. `min` und
/// `max` sind die UNTER- bzw. OBERgrenze — der Aufrufer entscheidet,
/// welcher Wert wohin gehoert, damit es an dieser Stelle keine
/// zweite Namenstabelle gibt.
///
/// M4: `f` ist durch `zahl_aus` und die Einheitenpruefung bereits auf
/// `[0, MA_MEDIA_P]` begrenzt; `min` kann nie groesser als `max`
/// werden, ausser der Autor schreibt es so — dann trifft die Zeile
/// einfach nie, und das ist die richtige Antwort.
fn setze_grenzen(zeile: &mut MediaUnd, breite: bool, min: Option<f32>, max: Option<f32>) {
    setze_bereichsgrenzen(
        zeile, breite, min.map(|f| (f, false)), max.map(|f| (f, false)),
    );
}

type MediaGrenze = Option<(f32, bool)>; // Wert, offen

fn setze_bereichsgrenzen(
    zeile: &mut MediaUnd,
    breite: bool,
    min: MediaGrenze,
    max: MediaGrenze,
) {
    let (unten, unten_offen, oben, oben_offen) = if breite {
        (
            &mut zeile.min_breite, &mut zeile.min_breite_offen,
            &mut zeile.max_breite, &mut zeile.max_breite_offen,
        )
    } else {
        (
            &mut zeile.min_hoehe, &mut zeile.min_hoehe_offen,
            &mut zeile.max_hoehe, &mut zeile.max_hoehe_offen,
        )
    };
    // Die engere Grenze gewinnt; bei gleichem Wert gewinnt die offene.
    for (neu, alt, offen, unter) in [(min, unten, unten_offen, true), (max, oben, oben_offen, false)] {
        if let Some((f, neu_offen)) = neu {
            match *alt {
                Some(a) if a == f => *offen |= neu_offen,
                Some(a) if (unter && a > f) || (!unter && a < f) => {}
                _ => {
                    *alt = Some(f);
                    *offen = neu_offen;
                }
            }
        }
    }
}

/// Eine Laenge in einer Media-Abfrage in px.
///
/// Release 08.10.2026: `em`/`rem` beziehen sich hier auf die
/// ANFANGS-Schriftgroesse (Media Queries 4 §1.3: 16 px), nicht auf ein
/// Element. GEMESSEN vorher: nur `px` — `@media (min-width: 48em)` und
/// jede Tailwind-4-Grenze (`width >= 40rem`) trafen NIE.
fn media_px(f: f32, einheit: &str) -> Option<f32> {
    let e = einheit.to_ascii_lowercase();
    let px = match e.as_str() {
        "px" => f,
        "em" | "rem" => f * crate::BASIS_SCHRIFT_PX,
        "pt" => f * 96.0 / 72.0,
        "pc" => f * 16.0,
        "in" => f * 96.0,
        "cm" => f * 96.0 / 2.54,
        "mm" => f * 96.0 / 25.4,
        "q" => f * 96.0 / 101.6,
        "" if f == 0.0 => 0.0,
        _ => return None,
    };
    px.is_finite().then_some(px)
}

/// Erkennt Vergleiche fuer width/height einschliesslich offener Grenzen.
/// Ungueltige Vergleiche werden vom Aufrufer als nicht zutreffend behandelt.
fn bereich_syntax(innen: &[&Token]) -> Option<(bool, MediaGrenze, MediaGrenze)> {
    let namen: Vec<usize> = innen.iter().enumerate()
        .filter_map(|(i, t)| matches!(t, Token::Ident(_)).then_some(i)).collect();
    if namen.len() != 1 { return None; }
    let pos = namen[0];
    let Token::Ident(name) = innen[pos] else { return None };
    let breite = if name.eq_ignore_ascii_case("width") { true }
        else if name.eq_ignore_ascii_case("height") { false }
        else { return None };

    // Jede Seite besteht aus einer Zahl und einem Vergleichsoperator.
    fn seite(teile: &[&Token], links: bool) -> Option<(f32, char, bool)> {
        let (zahl, op) = if links {
            (teile.first()?, &teile[1..])
        } else {
            (teile.last()?, &teile[..teile.len() - 1])
        };
        let Token::Zahl(z, einheit) = zahl else { return None };
        let f = media_px(zahl_aus(z)?, einheit)?;
        let (zeichen, offen) = match op {
            [Token::Zeichen(c @ ('<' | '>'))] => (*c, true),
            [Token::Zeichen(c @ ('<' | '>')), Token::Zeichen('=')] => (*c, false),
            [Token::Zeichen('=')] => ('=', false),
            _ => return None,
        };
        Some((f, zeichen, offen))
    }
    let links = if pos > 0 { Some(seite(&innen[..pos], true)?) } else { None };
    let rechts = if pos + 1 < innen.len() { Some(seite(&innen[pos + 1..], false)?) } else { None };
    if links.is_none() && rechts.is_none() { return None; }
    if let (Some((_, a, _)), Some((_, b, _))) = (links, rechts) {
        // Verkettete Vergleiche muessen dieselbe Richtung haben; '=' ist
        // nur als einzelner Vergleich erlaubt (MQ4, 2.4.3).
        if a != b || a == '=' { return None; }
    }
    let mut min = None;
    let mut max = None;
    for (seite, ist_links) in [(links, true), (rechts, false)] {
        if let Some((f, op, offen)) = seite {
            if op == '=' { min = Some((f, false)); max = Some((f, false)); }
            else if (ist_links && op == '<') || (!ist_links && op == '>') {
                min = Some((f, offen));
            } else {
                max = Some((f, offen));
            }
        }
    }
    Some((breite, min, max))
}
/// Parst `selektor { deklarationen }` ab `pos`. Bei Erfolg steht `pos`
/// hinter `}` und die Regel liegt im Blatt (global gezaehlt); bei
/// Fehler steht `pos` hinter der Erholung und verworfen waechst.
/// Verbraucht immer mindestens ein Token.
/// Release 11.10.2026 (Agent CSS2): `verschachtelt` — die Regel steht in
/// einem Gruppenblock (`@media` ...); ein `}` ohne eigenen Block schliesst
/// dann diesen Block und bleibt fuer ihn stehen.
fn parse_stilregel(token: &[Token], pos: &mut usize, blatt: &mut Stylesheet, regeln: &mut usize, verschachtelt: bool) {
    let start = *pos;
    // Selektor-Scheibe bis `{` (Klammern duerfen dort nicht stehen —
    // `(` und `[` schon, z.B. :not() und [attr]).
    let mut rund = 0usize;
    let mut eckig = 0usize;
    let mut ende: Option<usize> = None;
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') if rund == 0 && eckig == 0 => {
                ende = Some(*pos);
                break;
            }
            // `}` ohne Block: kaputte Regel, faellt weg.
            // Release 11.10.2026 (Agent CSS2): `;` beendet eine Stilregel
            // NICHT — der Selektor reicht bis `{` (CSS 2.1 §4.2 „malformed
            // statements": bis zum Ende der Anweisung, also samt Block; CSS
            // Syntax 3 §5.4.3). GEMESSEN vorher (WPT `syntax/at-rule-001`,
            // `@ import "x.css"; div { color: red }`): die Regel nach dem `;`
            // galt, der Text wurde rot.
            Token::Zeichen('}') if rund == 0 && eckig == 0 => {
                blatt.verworfen += 1;
                if !verschachtelt || *pos == start {
                    *pos += 1;
                }
                return;
            }
            Token::Funktion(_) | Token::Zeichen('(') => rund += 1,
            Token::Zeichen(')') => rund = rund.saturating_sub(1),
            Token::Zeichen('[') => eckig += 1,
            Token::Zeichen(']') => eckig = eckig.saturating_sub(1),
            _ => {}
        }
        *pos += 1;
        // Absicherung: kein endloser Selektor (M3, zusaetzlich zur Laenge).
        if *pos - start > MAX_TOKEN {
            break;
        }
    }
    let klammer = match ende {
        None => {
            // EOF ohne Block: Rest faellt weg.
            blatt.verworfen += 1;
            return;
        }
        Some(e) => e,
    };
    // Ab hier ist nur noch der Block wichtig — die Selektor-Token
    // werden mit durchgereicht, weil eine VERSCHACHTELTE Regel
    // (CSS Nesting 1) sie als Eltern-Selektor braucht.
    let mut sel_tokens: Vec<Token> = Vec::new();
    sel_tokens.extend_from_slice(&token[start..klammer]);
    *pos = klammer + 1; // ueber `{`
    parse_block_als_regel(token, pos, blatt, regeln, &sel_tokens);
}

/// Wie [`parse_stilregel`], aber mit BEREITS bekannten
/// Selektor-Token. Das ist der Weg, den eine verschachtelte Regel geht.
///
/// # WARUM ES DIE FUNKTION UEBERHAUPT GIBT
///
/// GEMESSENER FEHLER 01.10.2026 (K2, echte Datei
/// `tests/daten/archlinux_navbar.css`): CSS-Verschachtelung war
/// unbekannt, und der Block-Leser verwarf bei jeder inneren `{` die
/// GANZE aeussere Regel ("Verschachteltes `{` (kein CSS): Regel
/// faellt weg"). Die Navigation von archlinux.org ist genau eine
/// verschachtelte Regel — die Datei lieferte 0 Regeln statt 5.
fn parse_block_als_regel(
    token: &[Token],
    pos: &mut usize,
    blatt: &mut Stylesheet,
    regeln: &mut usize,
    sel_tokens: &[Token],
) {
    // Release 08.10.2026: die Stelle der aeusseren Regel. Sie wird erst
    // am Ende eingetragen, muss aber VOR ihren verschachtelten Regeln
    // stehen (CSS Nesting 1 §3: die Kinder folgen dem Elternteil) —
    // sonst schluege `.k { display: none; @media (x) { display: block } }`
    // die Media-Angabe bei gleicher Spezifitaet.
    let platz = blatt.regeln.len();
    // Selektoren parsen (Gruppe, gedeckelt).
    let selektoren = parse_selektorgruppe(sel_tokens, false);
    if selektoren.is_empty() {
        // Kein gueltiger Selektor: Block ueberspringen, Regel zaehlen.
        blatt.verworfen += 1;
        ueberspringe_block(token, pos);
        return;
    }
    // Deklarationen bis zum schliessenden `}` (plus Variablen, C10).
    let mut dekls: Vec<Deklaration> = Vec::new();
    let mut vars: Vec<VariablenDekl> = Vec::new();
    let mut stueck_start = *pos;
    let mut tiefe = 0usize;
    let mut stuecke = 0usize;
    let mut geschlossen = false;
    // Traegt ein Stueck ein (Dekl. + ggf. Variable, je gedeckelt).
    let mut eintragen = |scheibe: &[Token]| {
        let st = parse_dekl(scheibe);
        dekls.extend(st.dekls);
        if let Some(v) = st.var {
            if vars.len() < MAX_VAR_JE_REGEL {
                vars.push(v);
            }
        }
    };
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen(';') if tiefe == 0 => {
                if stuecke < MAX_STUECKE_JE_REGEL && *pos > stueck_start {
                    eintragen(&token[stueck_start..*pos]);
                }
                stuecke += 1;
                *pos += 1;
                stueck_start = *pos;
                continue;
            }
            // CSS Syntax 3, §4.3.1 „consume a simple block": ein `}`
            // schliesst den Block — UND, wenn gerade eine Klammer
            // offen ist, auch diese („This is a parse error. Close the
            // function ... and return").
            //
            // GEMESSENER FEHLER 01.10.2026 (K3, `ein_fehler_rettet_keine_regel`):
            // hier stand `if tiefe == 0`. Bei `.k { color: rgb( } .k
            // { border-radius: 5px; }` blieb `tiefe == 1`, das `}`
            // wurde uebersehen, und der Leser lief bis EOF — die
            // ZWEITE Regel kam nie an (`radius` blieb [0;4] statt
            // [5;4]). Ein Tippfehler am Anfang eines Blatts hat damit
            // das GANZE Blatt gefressen.
            //
            // Deshalb gilt das `}` OHNE Bedinung. Das Stueck bis
            // dahin wird noch eingetragen (`color: rgb(}` ist eine
            // ungueltige Deklaration und faellt danach in
            // `dekl_auswerten` weg), die Regel DANEBEN lebt.
            //
            // Sicher ist das: verschachtelte `{...}` laufen ueber den
            // eigenen Aufruf `parse_block_als_regel`, kommen hier also
            // nicht vor. `tiefe` zaehlt in dieser Schleife nur
            // runde und eckige Klammern.
            Token::Zeichen('}') => {
                if stuecke < MAX_STUECKE_JE_REGEL && *pos > stueck_start {
                    eintragen(&token[stueck_start..*pos]);
                }
                *pos += 1;
                geschlossen = true;
                break;
            }
            Token::Funktion(_) | Token::Zeichen('(') | Token::Zeichen('[') => {
                tiefe += 1;
                if tiefe > MAX_KLAMMER_TIEFE {
                    // Klammerbombe im Block: Regel faellt weg.
                    blatt.verworfen += 1;
                    ueberspringe_block(token, pos);
                    return;
                }
            }
            Token::Zeichen(')') | Token::Zeichen(']') => {
                tiefe = tiefe.saturating_sub(1);
            }
            // CSS VERSCHACHTELUNG (Nesting 1, §2): die Token seit dem
            // letzten Trenner sind kein Deklarationswert, sondern ein
            // Selektor.
            //
            // GEMESSENER FEHLER 01.10.2026: hier stand "Verschachteltes
            // `{` (kein CSS): Regel faellt weg" mit `return` — die
            // GANZE aeussere Regel ging verloren, mit allen eigenen
            // Deklarationen. `archlinux_navbar.css` (echte Datei in
            // tests/daten) lieferte dadurch 0 statt 5 Regeln.
            Token::Zeichen('{') => {
                let innen_sel = &token[stueck_start..*pos];
                let getrimmt = trimme_leer(innen_sel);
                let zu_tief = NEST_TIEFE.with(|t| t.get()) >= MAX_NEST_TIEFE;
                if getrimmt.is_empty() || zu_tief {
                    // `{` ohne Selektor, oder zu tief verschachtelt: der
                    // Block faellt weg, die aeussere Regel LEBT weiter.
                    //
                    // Release 08.10.2026: erst UEBER die `{` springen —
                    // vorher stand der Sprung an der Klammer selbst, und
                    // `ueberspringe_block` lief bis hinter das Ende der
                    // AEUSSEREN Regel.
                    blatt.verworfen += 1;
                    *pos += 1;
                    ueberspringe_block(token, pos);
                    stueck_start = *pos;
                    continue;
                }
                // Release 08.10.2026: `@media`/`@supports`/`@layer` IN
                // einer Stilregel (CSS Nesting 1 §3.2) — der Block gilt
                // fuer den Elternselektor. GEMESSEN vorher: verworfen,
                // und mit ihm jede Tailwind-4-Variante (`sm:flex` ist
                // `.sm\:flex { @media (width >= 40rem) { display: flex } }`).
                if let Some(Token::At(at)) = getrimmt.first() {
                    let at = at.to_ascii_lowercase();
                    let vorspann = getrimmt[1..].to_vec();
                    *pos += 1; // ueber die innere `{`
                    NEST_TIEFE.with(|t| t.set(t.get() + 1));
                    verschachtelte_at_regel(token, pos, blatt, regeln, sel_tokens, &at, &vorspann);
                    NEST_TIEFE.with(|t| t.set(t.get().saturating_sub(1)));
                    stueck_start = *pos;
                    continue;
                }
                let kombiniert = kombiniere_nest(sel_tokens, innen_sel);
                *pos += 1; // ueber die innere `{`
                if kombiniert.is_empty() {
                    blatt.verworfen += 1;
                    ueberspringe_block(token, pos);
                } else {
                    // `*regeln` zaehlt mit — eine verschachtelte Regel
                    // ist eine echte Regel (M4, kein Versteck dahinter).
                    NEST_TIEFE.with(|t| t.set(t.get() + 1));
                    parse_block_als_regel(token, pos, blatt, regeln, &kombiniert);
                    NEST_TIEFE.with(|t| t.set(t.get().saturating_sub(1)));
                }
                stueck_start = *pos;
                continue;
            }
            _ => {}
        }
        *pos += 1;
    }
    // Release 11.10.2026 (Agent CSS2): das Blattende schliesst den offenen
    // Block (CSS Syntax 3 §5.4.7 „consume a simple block": EOF ist ein
    // Parse-Fehler, der Block endet dort), die letzte Deklaration gilt.
    // GEMESSEN vorher (WPT `syntax/eof-001..007`): `div { color: green`
    // ohne `}` verlor die Farbe.
    if !geschlossen && *pos > stueck_start && stuecke < MAX_STUECKE_JE_REGEL {
        eintragen(&token[stueck_start..*pos]);
    }
    // Globale Grenze (M4): alles darueber faellt weg und wird gezaehlt.
    if *regeln >= MAX_REGELN {
        blatt.verworfen += 1;
        return;
    }
    *regeln += 1;
    let regel = Regel::Stil(StilRegel {
        selektoren,
        deklarationen: dekls,
        variablen: vars,
        zusatz_medien: Vec::new(),
    });
    if platz < blatt.regeln.len() {
        blatt.regeln.insert(platz, regel);
    } else {
        blatt.regeln.push(regel);
    }
}

/// Release 08.10.2026: eine `@`-Regel IN einer Stilregel (CSS Nesting 1
/// §3.2): `.k { @media (x) { color: red } }`. Der Block gilt fuer den
/// Elternselektor. `@media` und `@layer` werden eingehaengt wie auf
/// oberster Ebene, `@supports` gilt nach seiner Bedingung; alles andere
/// (`@container`, `@scope`, `@starting-style`) faellt weg.
fn verschachtelte_at_regel(
    token: &[Token],
    pos: &mut usize,
    blatt: &mut Stylesheet,
    regeln: &mut usize,
    sel_tokens: &[Token],
    at: &str,
    vorspann: &[Token],
) {
    match at {
        "media" => {
            let q = parse_media_abfrage(vorspann);
            let mut korb = Stylesheet::default();
            parse_block_als_regel(token, pos, &mut korb, regeln, sel_tokens);
            blatt.verworfen += korb.verworfen;
            unter_media(korb.regeln, &q, &mut blatt.regeln);
        }
        "supports" => {
            let bedingung: String = vorspann.iter().map(|t| t.text()).collect();
            if crate::supports::supports_trifft(&bedingung) {
                parse_block_als_regel(token, pos, blatt, regeln, sel_tokens);
            } else {
                ueberspringe_block(token, pos);
            }
        }
        "layer" => {
            let name: String = vorspann.iter().map(|t| t.text()).collect();
            let name = name.trim();
            let mut korb = Stylesheet::default();
            parse_block_als_regel(token, pos, &mut korb, regeln, sel_tokens);
            blatt.verworfen += korb.verworfen;
            unter_layer(
                korb.regeln,
                (!name.is_empty()).then(|| name.to_string()),
                &mut blatt.regeln,
            );
        }
        // Release 12.10.2026 (Agent C): `@scope` in einer Stilregel
        // (`crate::scope::umschreiben_verschachtelt`).
        "scope" => {
            let anfang = *pos;
            ueberspringe_block(token, pos);
            let ende = pos.saturating_sub(1).max(anfang);
            let rumpf: String = token[anfang..ende].iter().map(Token::text).collect();
            let kopf: String = vorspann.iter().map(Token::text).collect();
            let eltern: String = sel_tokens.iter().map(Token::text).collect();
            if let Some(css) = crate::scope::umschreiben_verschachtelt(eltern.trim(), &kopf, &rumpf) {
                let (t2, _) = tokenisieren(&css);
                let mut p2 = 0usize;
                let korb = gruppen_inhalt(&t2, &mut p2, regeln, 1);
                blatt.verworfen += korb.verworfen;
                blatt.regeln.extend(korb.regeln);
            }
        }
        _ => ueberspringe_block(token, pos),
    }
}

/// Hoechstzahl Token fuer einen ZUSAMMENGESETZTEN Selektor aus
/// Verschachtelung (M3/M4).
///
/// Begruendung: der Setzer liefert Eltern- und Kindselektor; der
/// schlimmste erlaubte Fall ist ein 1024-Token-Selektor (siehe
/// `MAX_SELEKTOR_LAENGE`) mit einer Verschachtelungsebene darueber.
/// 2048 deckt beides ab und haelt die Allokation einer Verschachtelung
/// bei 2048 x ~16 Byte = 32 KiB. Darueber ist die Angabe eine Bombe,
/// nicht eine Seite.
const MA_NEST_TOKEN: usize = 2_048;

/// Baut aus Eltern- und Kindselektor die Selektor-Token der
/// verschachtelten Regel (CSS Nesting 1, §3).
///
/// DIE ZWEI FAELLE DER SPEC
///
/// 1. `&` steht im Kindselektor (am Anfang, `& a`, `a & b`, `&`):
///    `&` wird durch den ELTERNselektor ersetzt.
///    ```text
///    .karte { & .titel { … }  ->  .karte .titel { … }
///                    .titel   ->  .karte .titel { … }
///    ```
/// 2. `&` steht NICHT im Kindselektor: nach Spec §3 ist das
///    gleichbedeutend mit `:is(<Elternselektor>) <Kindselektor>`.
///    Der `:is()`-Weg ist nicht Kosmetik: er nimmt die GROESSTE
///    Spezifitaet der Elternliste statt ihrer SUMME. Ohne ihn
///    gewaenne ein verschachteltes Kind eine hoehere
///    Spezifitaet, als der Browser ihm gibt.
///
/// 3. `&` ohne Elternaussage (Top-Level, `@media`-Ebene) ist
///    ungueltig — das liefert die leere Liste zurueck.
///
/// M3/M4: Das Produkt aus Eltern- und Kindteilen ist durch
/// `MAX_SELEKTOREN_JE_REGEL` (16) begrenzt, die Tokenzahl durch
/// `MA_NEST_TOKEN`. Ueberschreiten heisst "ungueltig", nicht
/// "abschneiden und irgendwas treffen".
fn kombiniere_nest(eltern: &[Token], kind: &[Token]) -> Vec<Token> {
    // Leer am Rand wegschneiden: der Block-Leser laesst sie stehen,
    // und `:is(#a )` ist etwas anderes als `:is(#a)`.
    let eltern = trimme_leer(eltern);
    let kind = trimme_leer(kind);
    if eltern.is_empty() || kind.is_empty() {
        return Vec::new();
    }
    // M4: VOR der ersten Allokation.
    if eltern.len().saturating_add(kind.len()) > MA_NEST_TOKEN {
        return Vec::new();
    }
    let eltern_teile = teile_sel_komma(eltern);
    let kind_teile = teile_sel_komma(kind);
    if eltern_teile.is_empty() || kind_teile.is_empty() {
        return Vec::new();
    }
    // M4: Das Produkt ist das, was wirklich alloziert wird.
    if eltern_teile.len().saturating_mul(kind_teile.len()) > MAX_SELEKTOREN_JE_REGEL {
        return Vec::new();
    }
    let mut aus: Vec<Token> = Vec::new();
    let mut erstes = true;
    for e in eltern_teile.iter() {
        for k in kind_teile.iter() {
            if !erstes {
                aus.push(Token::Zeichen(','));
            }
            erstes = false;
            // `&` irgendwo im Kindteil?
            if k.iter().any(|t| matches!(t, Token::Zeichen('&'))) {
                for t in k.iter() {
                    match t {
                        Token::Zeichen('&') => aus.extend_from_slice(e),
                        anderes => aus.push(anderes.clone()),
                    }
                }
            } else {
                // `:is(<eltern>) <kind>` — die Spec-Form ohne `&`.
                //
                // DER DOPPELPUNKT IST NOTWENDIG: der Pseudo-Leser rechnet
                // mit `scheibe[i] == ':'` und `scheibe[i+1] == Funktion`
                // und liest den Inhalt ab `i + 2`. Ohne den Doppelpunkt
                // frisst er das erste echte Token (`#a`), die Liste in
                // `:is()` ist leer, und die verschachtelte Regel gilt als
                // ungueltig — gemessen als "1 Regel statt 2".
                aus.push(Token::Zeichen(':'));
                aus.push(Token::Funktion(String::from("is")));
                aus.extend_from_slice(e);
                aus.push(Token::Zeichen(')'));
                // Leer als Nachfahr-Kombinator.
                aus.push(Token::Leer);
                aus.extend_from_slice(k);
            }
        }
    }
    aus
}

/// Schneidet Leer-Token an beiden Enden ab (M3: der Durchlauf ist
/// durch die Laenge der Scheibe beschraenkt).
fn trimme_leer(s: &[Token]) -> &[Token] {
    let mut a = 0usize;
    let mut b = s.len();
    while a < b && matches!(s[a], Token::Leer) {
        a += 1;
    }
    while b > a && matches!(s[b - 1], Token::Leer) {
        b -= 1;
    }
    &s[a..b]
}

/// Teilt einen Selektor-Abschnitt an den Kommas auf OBERSTER Ebene
/// (Klammern und eckige Klammern bleiben zusammen).
///
/// M3: Die Schleife laeuft `0..=len` und bricht nach
/// `MA_NEST_KOMMA` Teilen ab — damit ist auch bei
/// `a,a,a,a,…` (10 000 Teile) die Arbeit beschraenkt.
fn teile_sel_komma(s: &[Token]) -> Vec<Vec<Token>> {
    let mut teile: Vec<Vec<Token>> = Vec::new();
    let mut aktuell: Vec<Token> = Vec::new();
    let mut rund = 0usize;
    let mut eckig = 0usize;
    for t in s.iter() {
        match t {
            Token::Funktion(_) | Token::Zeichen('(') => rund += 1,
            Token::Zeichen(')') => rund = rund.saturating_sub(1),
            Token::Zeichen('[') => eckig += 1,
            Token::Zeichen(']') => eckig = eckig.saturating_sub(1),
            _ => {}
        }
        if matches!(t, Token::Zeichen(',')) && rund == 0 && eckig == 0 {
            if !aktuell.is_empty() {
                teile.push(std::mem::take(&mut aktuell));
            }
            if teile.len() >= MA_NEST_KOMMA {
                return teile;
            }
            continue;
        }
        aktuell.push(t.clone());
    }
    if !aktuell.is_empty() {
        teile.push(aktuell);
    }
    teile
}

/// Hoechste Teile je Selektorgruppe in einer Verschachtelung (M3).
/// `MAX_SELEKTOREN_JE_REGEL` (16) ist die eigentliche Grenze; diese
/// Zahl deckt das TEILEN ab, das davor laeuft.
const MA_NEST_KOMMA: usize = 16;

/// Springt ab `pos` ueber einen Block (hinter das schliessende `}`).
/// Dient der Erholung; Tiefe gedeckelt, Verbrauch >= 0 Token, aber der
/// Aufrufer stand immer an einer Klammer (Gesamtfortschritt >= 1).
fn ueberspringe_block(token: &[Token], pos: &mut usize) {
    // NUR `{` und `}` sind Blockebenen — dieselbe Regel wie in
    // `ueberspringe_at`, und aus demselben Grund.
    //
    // GEMESSENER FEHLER 01.10.2026 (04:05, K9-Vorbereitung): hier stand
    // `Token::Zeichen('{') | Token::Funktion(_) => tiefe += 1`. Eine
    // Funktion wird von `)` geschlossen, nicht von `}`. In
    // `@supports (color: light-dark(red, red)) { :root { --a:
    // light-dark(var(--x), var(--y)) } } .external { ... }` blieb
    // `tiefe` dauerhaft > 0, und `ueberspringe_block` lief bis EOF.
    //
    // GEMESSENE FOLGE auf tests/daten/mdn_styles-global.css: die Regel
    // `.external { padding-right: 1.125em; position: relative }` war
    // danach nirgends im Baum — und der Test
    // `mdn_external_hat_innenabstand_und_position` fand 0 statt 18 px.
    // 14 der 32 gelesenen Regeln gingen verloren.
    let mut tiefe = 0usize;
    while *pos < token.len() {
        match &token[*pos] {
            Token::Zeichen('{') => {
                tiefe += 1;
                if tiefe > MAX_KLAMMER_TIEFE {
                    // Erholung: bis zum naechsten `}` auf Ebene 0.
                    *pos += 1;
                    while *pos < token.len() {
                        if matches!(token[*pos], Token::Zeichen('}')) {
                            *pos += 1;
                            return;
                        }
                        *pos += 1;
                    }
                    return;
                }
            }
            Token::Zeichen('}') => {
                if tiefe == 0 {
                    *pos += 1;
                    return;
                }
                tiefe -= 1;
            }
            _ => {}
        }
        *pos += 1;
    }
}

// ===========================================================================
// SELEKTOREN (C2)
// ===========================================================================

/// Teilt die Gruppe an Kommas auf oberster Ebene und parst jeden Teil.
/// Ungueltige Teile fallen weg; leere Gruppe = keine Regel.
/// `relativ`: die Liste ist das Innere von `:has()` und darf mit einem
/// Kombinator beginnen (`:has(> a)`, `:has(+ p)`).
///
/// GEMESSENER FEHLER 30.09.2026 (R0 c): `relativ` gab es nicht, und
/// `parse_selektor` verwarf `> a` mit "Kombinator ohne linken Partner".
/// Damit war `:has(> img)` — die haeufigste `:has`-Form auf echten
/// Seiten — nicht benutzbar: die Regel fiel schon beim Lesen weg.
/// Hoechste Zahl Sprachkennungen in einem `:lang()` (M3).
///
/// GEMESSENE GRUNDLAGE: die groesste echte Sprachliste im Korpus hat
/// **4** Kennungen (`:lang(de, de-AT, de-CH, de-DE)` in
/// `wikipedia_vector-2022.css`).
///
/// GEMESSENE KORREKTUR (21:35): Ich hatte hier „252" als Grundlage
/// eingetragen. Das war ein Artefakt eines Regex-Zaehlers, der
/// `@counter-style`-Bloecke mit `}` in Strings falsch zerschnitten hat;
/// siehe `MAX_SELEKTOREN_JE_REGEL` in `lib.rs`. 256 bleibt trotzdem
/// stehen — aber nicht, weil 252 gemessen sind, sondern weil eine
/// Sprachliste fuer alle Sprachen einer Seite (die Form, nach der
/// Wikipedia seine Ausgaben versorgt) sonst an dieser Stelle abgeschnitten
/// wuerde. Das ist eine Auslegung, und sie steht als solche da.
const MAX_SPRACHEN: usize = 256;

/// Hoechste Laenge einer Sprachkennung in `:lang()` (M3/M4).
///
/// RFC 4647 §2.1.1 erlaubt primaer bis 8 Zeichen. Hier mehr, weil
/// Sprachkennungen in der Praxis laenger sind (`zh-Hant-TW`).
const MAX_SPRACHLAENGE: usize = 35;

/// Liest eine Selektorliste — die Form, die auch in einem
/// `<style>`-Block stehen kann, mit Komma als Trenner.
///
/// # WOZU (C1, 01.10.2026)
///
/// Die WPT-Abnahme (`css/selectors`) stellt genau eine Frage:
/// „passt dieser Selektor auf dieses Element?". Das ist
/// `selektor_passt`, aber das setzt einen `Selektor` voraus — und der
/// Parser dafuer war bisher nur intern. Ohne diesen Zugang muss ein
/// Aufrufer den Selektor in ein Stylesheet schreiben und an der
/// Kaskade ablesen, was um drei Ecken laenger ist und die Frage
/// beantwortet, die nicht gestellt wurde.
///
/// GEMESSENE ENTSCHEIDUNG: die Funktion ist additiv und benutzt genau
/// den Weg, den auch ein `<style>`-Block geht. Kein zweiter Parser,
/// keine eigene Grammatik.
///
/// # RUECKGABE
///
/// Leere Liste = die Liste war ungueltig. Das ist dieselbe Antwort,
/// die ein Block im Parser gibt (`verworfen += 1`).
#[must_use]
pub fn selektorliste_lesen(text: &str) -> Vec<Selektor> {
    let (scheibe, _abgeschnitten) = crate::token::tokenisieren(text);
    parse_selektorgruppe(&scheibe, false)
}

fn parse_selektorgruppe(scheibe: &[Token], relativ: bool) -> Vec<Selektor> {
    // Release 11.10.2026 (Agent CSS2): ein `;` oder eine @-Regel kann in
    // keinem Selektor stehen — dann ist die GANZE Liste ungueltig (Selectors
    // 4 §4.1), nicht nur ihr Teil (seit `;` die Stilregel nicht mehr beendet,
    // reicht der Vorspann `test; @charset "x"; .a, #b` bis zur Klammer).
    // GEMESSEN vorher (WPT `syntax/at-charset-039`): `#b` galt.
    if scheibe.iter().any(|t| matches!(t, Token::Zeichen(';') | Token::At(_))) {
        return Vec::new();
    }
    let mut aus: Vec<Selektor> = Vec::new();
    let mut start = 0usize;
    let mut rund = 0usize;
    let mut eckig = 0usize;
    let mut i = 0usize;
    while i <= scheibe.len() {
        let komma = i < scheibe.len()
            && matches!(scheibe[i], Token::Zeichen(','))
            && rund == 0
            && eckig == 0;
        match scheibe.get(i) {
            Some(Token::Funktion(_)) | Some(Token::Zeichen('(')) => rund += 1,
            Some(Token::Zeichen(')')) => rund = rund.saturating_sub(1),
            Some(Token::Zeichen('[')) => eckig += 1,
            Some(Token::Zeichen(']')) => eckig = eckig.saturating_sub(1),
            _ => {}
        }
        if komma || i == scheibe.len() {
            if aus.len() < MAX_SELEKTOREN_JE_REGEL && i > start {
                if let Some(s) = parse_selektor(&scheibe[start..i], relativ) {
                    aus.push(s);
                }
            }
            start = i + 1;
        }
        i += 1;
    }
    aus
}

/// Parst einen Einzelselektor. None = ungueltig (faellt weg, C2).
///
/// `relativ`: ein fuehrender Kombinator ist erlaubt und wird der
/// `komb` des ERSTEN Kettenglieds (`:has(> a)`). Ohne dieses Flag
/// bleibt `> a` ungueltig — im normalen Selektor gibt es keinen
/// "linken Partner", an dem ein fuehrendes `>` haengen koennte.
fn parse_selektor(scheibe: &[Token], relativ: bool) -> Option<Selektor> {
    // Laengengrenze (M4): grobe Schaetzung ueber Token-Rohtexte.
    let mut zeichen = 0usize;
    for t in scheibe {
        zeichen += t.text().len();
        if zeichen > MAX_SELEKTOR_LAENGE {
            return None;
        }
    }
    let mut glieder: Vec<KettenGlied> = Vec::new();
    let mut laufend = Einfach::default();
    let mut laufend_komb = Kombinator::Keiner;
    let mut laufend_leer = true;
    let mut laufend_teile = 0usize;
    let mut haengig: Option<Kombinator> = None;
    let mut leer_seit = false;
    let mut i = 0usize;
    while i < scheibe.len() {
        match &scheibe[i] {
            Token::Leer => {
                if haengig.is_none() {
                    leer_seit = true;
                }
                i += 1;
                continue;
            }
            Token::Zeichen('>') | Token::Zeichen('+') | Token::Zeichen('~') => {
                if haengig.is_some() {
                    return None; // doppelter Kombinator ("div > + p")
                }
                let komb = match scheibe[i] {
                    Token::Zeichen('>') => Kombinator::Kind,
                    Token::Zeichen('+') => Kombinator::Nachbar,
                    _ => Kombinator::Geschwister,
                };
                haengig = Some(komb);
                leer_seit = false;
                i += 1;
                continue;
            }
            _ => {}
        }
        // Ein einfacher Teil beginnt: ggf. neues Glied eroeffnen.
        if haengig.is_some() || (leer_seit && !laufend_leer) {
            if laufend_leer {
                // Kombinator ohne linken Partner ("... > p"): ungueltig.
                // (Reines Anfangs-Leer kommt hier nie an: laufend ist
                // dann leer UND haengig ist None.)
                //
                // AUSNAHME `relativ`: am ANFANG darf ein Kombinator
                // stehen (`:has(> a)`). Er wird dann der `komb` des
                // ersten Glieds und bezieht sich auf das Element, in
                // dessen `:has()` er steht — nicht auf einen Vorgänger
                // im Dokument.
                if haengig.is_some() {
                    if !relativ {
                        return None;
                    }
                    // GEMESSENER FEHLER 30.09.2026 (R0 c): der erste
                    // Versuch hat `laufend_komb` hier gesetzt und
                    // `haengig` geleert. Zwei Zeilen weiter steht
                    // aber `laufend_komb = haengig.unwrap_or(Nachfahre)`
                    // — das hat den Wert sofort wieder ueberschrieben,
                    // und `+ p` wurde zu einem Nachfahren-Selektor.
                    // GEMESSEN: `ul:has(+ p)` passte dadurch auch dann,
                    // wenn das `p` ein ENKEL war.
                    //
                    // Die Loesung ist ein eigener Weg durch den Block:
                    // der fuehrende Kombinator springt direkt zum
                    // Teil-Anhaengen.
                    laufend_komb = haengig.unwrap();
                    haengig = None;
                    leer_seit = false;
                    // Teil anhaengen (verbraucht evtl. mehrere Token).
                    let (_, _, _, schritte) = parse_einfach_teil(scheibe, i, &mut laufend, false)?;
                    laufend_teile += 1;
                    if laufend_teile > MA_EINFACH_TEILE {
                        return None;
                    }
                    laufend_leer = false;
                    i += schritte;
                    continue;
                }
            } else {
                if glieder.len() + 1 >= MAX_SELEKTOR_TEILE {
                    return None;
                }
                glieder.push(KettenGlied {
                    komb: laufend_komb,
                    einfach: laufend,
                });
                laufend = Einfach::default();
                laufend_teile = 0;
            }
            laufend_komb = haengig.unwrap_or(Kombinator::Nachfahre);
            haengig = None;
        }
        leer_seit = false;
        // Teil anhaengen (verbraucht evtl. mehrere Token).
        let (_, _, _, schritte) = parse_einfach_teil(scheibe, i, &mut laufend, false)?;
        laufend_teile += 1;
        if laufend_teile > MA_EINFACH_TEILE {
            return None;
        }
        laufend_leer = false;
        i += schritte;
    }
    if haengig.is_some() {
        return None; // Kombinator am Ende ("div >")
    }
    if laufend_leer {
        return None; // leerer Selektor
    }
    if glieder.len() + 1 > MAX_SELEKTOR_TEILE {
        return None;
    }
    glieder.push(KettenGlied {
        komb: laufend_komb,
        einfach: laufend,
    });
    // Spezifitaet (a, b, c) exakt nach Selectors Level 3.
    let mut a = 0u32;
    let mut b = 0u32;
    let mut c = 0u32;
    for g in &glieder {
        spezifitaet_einfach(&g.einfach, &mut a, &mut b, &mut c);
    }
    Some(Selektor {
        kette: glieder,
        spezifitaet: (a, b, c),
    })
}

/// Zaehlt die Spezifitaet einer UND-Bedingung (mit :not-Argument).
fn spezifitaet_liste(liste: &[Selektor], a: &mut u32, b: &mut u32, c: &mut u32) {
    let mut max_a = 0;
    let mut max_b = 0;
    let mut max_c = 0;
    for sel in liste {
        let (sa, sb, sc) = sel.spezifitaet;
        if sa > max_a || (sa == max_a && sb > max_b) || (sa == max_a && sb == max_b && sc > max_c) {
            max_a = sa;
            max_b = sb;
            max_c = sc;
        }
    }
    *a = a.saturating_add(max_a);
    *b = b.saturating_add(max_b);
    *c = c.saturating_add(max_c);
}

fn spezifitaet_einfach(e: &Einfach, a: &mut u32, b: &mut u32, c: &mut u32) {
    if e.id.is_some() {
        *a = a.saturating_add(1);
    }
    *b = b.saturating_add(e.klassen.len().min(1024) as u32);
    *b = b.saturating_add(e.attribute.len().min(1024) as u32);
    for p in &e.pseudos {
        match p {
            Pseudo::Nicht(innen) | Pseudo::Ist(innen) | Pseudo::Hat(innen) => {
                spezifitaet_liste(innen, a, b, c);
            }
            Pseudo::NtesKind(_, _, innen) | Pseudo::NtesLetztesKind(_, _, innen) => {
                *b = b.saturating_add(1);
                spezifitaet_liste(innen, a, b, c);
            }
            Pseudo::Wo(_) | Pseudo::ScopeIn(..) | Pseudo::ScopeAnchor => {} // Specificity 0
            _ => {
                *b = b.saturating_add(1);
            }
        }
    }
    if e.tag.is_some() {
        *c = c.saturating_add(1);
    }
}

/// Parst EINEN einfachen Teil ab `i` (`div`, `*`, `#id`, `.klasse`,
/// `[attr]`, `:pseudo`). Haengt an `einfach` an.
/// Rueckgabe: (a, b, c)-Beitrag, verbrauchte Token. None = ungueltig.
/// `in_not`: verbietet verschachteltes :not und Kombinatoren innen.
fn parse_einfach_teil(
    scheibe: &[Token],
    i: usize,
    einfach: &mut Einfach,
    in_not: bool,
) -> Option<(usize, usize, usize, usize)> {
    match scheibe.get(i) {
        Some(Token::Ident(tag)) => {
            if einfach.tag.is_some() {
                return None; // "divspan" waere ein Token; "div div" trennt Leer
            }
            einfach.tag = Some(tag.to_ascii_lowercase());
            Some((0, 0, 1, 1))
        }
        Some(Token::Zeichen('*')) => {
            // Universal: keine Bedingung, keine Spezifitaet.
            Some((0, 0, 0, 1))
        }
        Some(Token::Hash(id)) => {
            if einfach.id.is_some() {
                return None; // "#a#b" ist ungueltig
            }
            einfach.id = Some(id.clone());
            Some((1, 0, 0, 1))
        }
        Some(Token::Zeichen('.')) => match scheibe.get(i + 1) {
            // Release 14.10.2026 (Agent C): the class prefix selector `.foo-*`
            // (Selectors 5, tentative; WPT css/selectors parse-class-prefix).
            Some(Token::Ident(k)) if k.ends_with('-') && k != "-" && matches!(scheibe.get(i + 2), Some(Token::Zeichen('*'))) => {
                einfach.pseudos.push(Pseudo::ClassPrefix(k.clone()));
                Some((0, 1, 0, 3))
            }
            Some(Token::Ident(k)) => {
                einfach.klassen.push(k.clone());
                Some((0, 1, 0, 2))
            }
            _ => None,
        },
        Some(Token::Zeichen('[')) => parse_attr(scheibe, i, einfach),
        Some(Token::Zeichen(':')) => parse_pseudo(scheibe, i, einfach, in_not),
        Some(Token::Funktion(name)) => {
            // Nur :not( als Funktion erlaubt — aber :not braucht den
            // Doppelpunkt davor; nacktes "not(" ist ungueltig.
            let _ = name;
            None
        }
        _ => None,
    }
}

/// Parst `[name]` / `[name=wert]` ab `[` (Index i zeigt auf `[`).
/// Parst `[name]`, `[name=wert]`, `[name^=wert]` usw.
fn parse_attr(
    scheibe: &[Token],
    i: usize,
    einfach: &mut Einfach,
) -> Option<(usize, usize, usize, usize)> {
    let mut k = i + 1;
    while k < scheibe.len() && matches!(scheibe[k], Token::Leer) {
        k += 1;
    }
    // Release 12.10.2026 (Agent C): `[*|att]` (jeder Namensraum) und `[|att]`
    // (keiner) — der Abgleich geht nach dem lokalen Namen (WPT cssom
    // CSSStyleRule-set-selectorText: `[*|lang]`).
    match (scheibe.get(k), scheibe.get(k + 1), scheibe.get(k + 2)) {
        (Some(Token::Zeichen('*')), Some(Token::Zeichen('|')), Some(Token::Ident(_))) => k += 2,
        (Some(Token::Zeichen('|')), Some(Token::Ident(_)), _) => k += 1,
        _ => {}
    }
    let name = match scheibe.get(k) {
        Some(Token::Ident(n)) => n.to_ascii_lowercase(),
        _ => return None,
    };
    k += 1;
    while k < scheibe.len() && matches!(scheibe[k], Token::Leer) {
        k += 1;
    }
    let mut bedingung = None;
    if let Some(Token::Zeichen(']')) = scheibe.get(k) {
        k += 1;
    } else {
        let op = match scheibe.get(k) {
            Some(Token::Zeichen('=')) => {
                k += 1;
                crate::typen::AttrOp::Gleich
            }
            Some(Token::Zeichen('~'))
                if matches!(scheibe.get(k + 1), Some(Token::Zeichen('='))) =>
            {
                k += 2;
                crate::typen::AttrOp::Wort
            }
            Some(Token::Zeichen('|'))
                if matches!(scheibe.get(k + 1), Some(Token::Zeichen('='))) =>
            {
                k += 2;
                crate::typen::AttrOp::Sprache
            }
            Some(Token::Zeichen('^'))
                if matches!(scheibe.get(k + 1), Some(Token::Zeichen('='))) =>
            {
                k += 2;
                crate::typen::AttrOp::Praefix
            }
            Some(Token::Zeichen('$'))
                if matches!(scheibe.get(k + 1), Some(Token::Zeichen('='))) =>
            {
                k += 2;
                crate::typen::AttrOp::Suffix
            }
            Some(Token::Zeichen('*'))
                if matches!(scheibe.get(k + 1), Some(Token::Zeichen('='))) =>
            {
                k += 2;
                crate::typen::AttrOp::Teil
            }
            _ => return None,
        };
        while k < scheibe.len() && matches!(scheibe[k], Token::Leer) {
            k += 1;
        }
        let v = match scheibe.get(k) {
            Some(Token::Ident(s)) | Some(Token::Zeichenkette(s)) | Some(Token::Hash(s)) => {
                s.clone()
            }
            Some(Token::Zahl(z, e)) => format!("{z}{e}"),
            _ => return None,
        };
        k += 1;
        while k < scheibe.len() && matches!(scheibe[k], Token::Leer) {
            k += 1;
        }
        // Release 2026-10-15: `s` is kept apart from no flag (see
        // `AttrBed::bedingung`).
        let mut i_flag = None;
        if let Some(Token::Ident(f)) = scheibe.get(k) {
            if f.eq_ignore_ascii_case("i") {
                i_flag = Some(true);
                k += 1;
            } else if f.eq_ignore_ascii_case("s") {
                i_flag = Some(false);
                k += 1;
            } else {
                return None;
            }
            while k < scheibe.len() && matches!(scheibe[k], Token::Leer) {
                k += 1;
            }
        }
        match scheibe.get(k) {
            Some(Token::Zeichen(']')) => k += 1,
            _ => return None,
        }
        bedingung = Some((op, v, i_flag));
    }
    einfach.attribute.push(AttrBed { name, bedingung });
    Some((0, 1, 0, k - i))
}

/// Parst `:pseudo` / `:not(...)` ab `:` (Index i zeigt auf `:`).
fn parse_pseudo(
    scheibe: &[Token],
    i: usize,
    einfach: &mut Einfach,
    in_not: bool,
) -> Option<(usize, usize, usize, usize)> {
    // `::` (Pseudoelemente).
    //
    // 06.10.2026: `::before`/`::after` werden als MARKE gelesen
    // (`Pseudo::Vorher`/`Nachher`); `pseudo_passt` erfuellt sie **nie**,
    // damit die Regel nicht am Element haengt. `narsil-page` liest die
    // Marke aus dem Selektor und baut daraus eine eigene Box.
    //
    // GEMESSEN an de.wikipedia.org: `.skin-vector-2022 .mw-logo::before`
    // traegt die Wikipedia-Kugel (background-image), `::after` die
    // Wortmarke. Vorher fiel die GANZE Regel (`return None`), und die
    // Kugel fehlte im Bild. Alle anderen `::namen` bleiben draussen.
    if matches!(scheibe.get(i + 1), Some(Token::Zeichen(':'))) {
        return match scheibe.get(i + 2) {
            Some(Token::Ident(name)) if name.eq_ignore_ascii_case("before") => {
                einfach.pseudos.push(Pseudo::Vorher);
                Some((0, 0, 1, 3))
            }
            Some(Token::Ident(name)) if name.eq_ignore_ascii_case("after") => {
                einfach.pseudos.push(Pseudo::Nachher);
                Some((0, 0, 1, 3))
            }
            // Release 10.10.2026: `::placeholder` und die Herstellerformen.
            Some(Token::Ident(name))
                if name.eq_ignore_ascii_case("placeholder")
                    || name.eq_ignore_ascii_case("-webkit-input-placeholder")
                    || name.eq_ignore_ascii_case("-moz-placeholder") =>
            {
                einfach.pseudos.push(Pseudo::Platzhalter);
                Some((0, 0, 1, 3))
            }
            // Release 10.10.2026: `::first-letter` (siehe `Pseudo::ErsterBuchstabe`).
            Some(Token::Ident(name)) if name.eq_ignore_ascii_case("first-letter") => {
                einfach.pseudos.push(Pseudo::ErsterBuchstabe);
                Some((0, 0, 1, 3))
            }
            // Release 15.10.2026 (Agent CSS2): `::first-line` (see
            // `Pseudo::ErsteZeile`).
            Some(Token::Ident(name)) if name.eq_ignore_ascii_case("first-line") => {
                einfach.pseudos.push(Pseudo::ErsteZeile);
                Some((0, 0, 1, 3))
            }
            // Release 12.10.2026: `::slotted(<compound>)` (siehe `Pseudo::Eingesetzt`).
            Some(Token::Funktion(name)) if name.eq_ignore_ascii_case("slotted") => {
                let mut k = i + 3;
                let mut tiefe = 0usize;
                let mut ende: Option<usize> = None;
                while k < scheibe.len() {
                    match &scheibe[k] {
                        Token::Zeichen(')') if tiefe == 0 => {
                            ende = Some(k);
                            break;
                        }
                        Token::Funktion(_) | Token::Zeichen('(') | Token::Zeichen('[') => tiefe += 1,
                        Token::Zeichen(']') | Token::Zeichen(')') => tiefe = tiefe.saturating_sub(1),
                        _ => {}
                    }
                    k += 1;
                }
                let e = ende?;
                let liste = parse_selektorgruppe(&scheibe[i + 3..e], false);
                if liste.is_empty() {
                    return None;
                }
                einfach.pseudos.push(Pseudo::Eingesetzt(liste));
                Some((0, 0, 1, e - i + 1))
            }
            _ => None,
        };
    }
    match scheibe.get(i + 1) {
        Some(Token::Ident(name)) => {
            let pseudo = if name.eq_ignore_ascii_case("host") {
                Pseudo::Wirt(Vec::new())
            } else if name.eq_ignore_ascii_case("first-child") {
                Pseudo::ErstKind
            } else if name.eq_ignore_ascii_case("last-child") {
                Pseudo::LetztKind
            } else if name.eq_ignore_ascii_case("only-child") {
                Pseudo::EinzigKind
            } else if name.eq_ignore_ascii_case("empty") {
                Pseudo::Leer
            } else if name.eq_ignore_ascii_case("link")
                || name.eq_ignore_ascii_case("any-link")
                || name.eq_ignore_ascii_case("-webkit-any-link")
            {
                // Release 2026-10-15: `:any-link` is `:link` (nothing is
                // visited).
                Pseudo::Link
            } else if name.eq_ignore_ascii_case("visited") {
                Pseudo::Besucht
            } else if name.eq_ignore_ascii_case("root") || name.eq_ignore_ascii_case("scope") {
                // Release 12.10.2026: `:scope` ohne Scope-Wurzel (Stylesheet)
                // ist `:root` (Selectors 4 §14.2); DOM-Abfragen schreiben es
                // vorher selbst um (`lebend::scope_umschreiben`).
                Pseudo::Wurzel
            } else if name.eq_ignore_ascii_case("hover") {
                Pseudo::Schweber
            } else if name.eq_ignore_ascii_case("focus") {
                Pseudo::Fokus
            } else if name.eq_ignore_ascii_case("focus-visible") {
                // K12 (01.10.2026): der alte Kommentar hier sagte „es
                // gibt keine `:focus-visible`-Heuristik, deshalb gilt
                // es wie `:focus`". Das war eine FESTLEGUNG ohne
                // Schnittstelle: der Aufrufer konnte gar nicht
                // unterscheiden, weil `Zustand` kein Feld dafuer hatte.
                //
                // GEMESSENE NUTZLAST: 148 Vorkommen auf den 35 echten
                // Stylesheets, darunter
                // `a:where(:not([role=\'button\'])):focus-visible` auf
                // wikipedia.
                //
                // JETZT: eigener Zustand. `narsil` setzt
                // `Zustand::focus_sichtbar`, wenn der Fokus nicht vom
                // Zeiger kam. CSS Selectors 4 §6.1.4.
                Pseudo::FocusSichtbar
            } else if name.eq_ignore_ascii_case("popover-open") {
                Pseudo::PopoverOffen
            } else if name.eq_ignore_ascii_case("modal") {
                Pseudo::Modal
            } else if name.eq_ignore_ascii_case("focus-within") {
                // CSS Selectors 4 §6.1.4.
                //
                // GEMESSENE REICHWEITE (C1, WPT, 02.10.2026): 27 Faelle
                // in `css/selectors` (focus-within.html,
                // focus-within-009.html, focus-within-removal.html,
                // focus-within-focus-move.html,
                // focus-within-display-none-001.html,
                // focus-within-toplayer-001.html). Der Name war hier
                // nicht genannt — und ein unbekannter Name gibt `None`
                // zurueck, womit die GANZE Regel fiel.
                //
                // Die Entscheidung faellt im Zustand
                // (`Zustand::focus_within`), den der Host setzt.
                Pseudo::FocusWithin
            } else if name.eq_ignore_ascii_case("active") {
                // CSS Selectors 4 §6.1.2. GEMESSEN: 205 Vorkommen,
                // darunter `#news a:active` auf archlinux_archweb.css.
                Pseudo::Aktiv
            } else if name.eq_ignore_ascii_case("target") {
                // CSS Selectors 4 §6.2.2. GEMESSEN: 11 Vorkommen
                // (`ol.references li:target`).
                Pseudo::Ziel
            } else if name.eq_ignore_ascii_case("placeholder-shown") {
                // Release 10.10.2026: CSS Selectors 4 (Eingabe-Pseudoklassen).
                Pseudo::PlatzhalterSichtbar
            } else if name.eq_ignore_ascii_case("checked") {
                // CSS Selectors 4 §6.3.2. GEMESSEN: 49 Vorkommen.
                Pseudo::Ausgewaehlt
            } else if name.eq_ignore_ascii_case("disabled") {
                // CSS Selectors 4 §6.3.4. GEMESSEN: 83 Vorkommen.
                Pseudo::Gesperrt
            } else if name.eq_ignore_ascii_case("enabled") {
                // CSS Selectors 4 §6.3.4. GEMESSEN: 159 Vorkommen —
                // mehr als `:disabled`. Deshalb ein eigenes Feld und
                // kein `:not(:disabled)` im Selektor: die
                // Spezifitaet waere eine andere.
                Pseudo::Aktiviert
            } else if name.eq_ignore_ascii_case("indeterminate") {
                // CSS Selectors 4 §6.3.2. GEMESSEN: 8 Vorkommen.
                Pseudo::Unbestimmt
            } else if name.eq_ignore_ascii_case("read-only") {
                // CSS Selectors 4 §6.3.5. GEMESSEN: 1 Vorkommen.
                Pseudo::NurLesen
            } else if name.eq_ignore_ascii_case("read-write") {
                // CSS Selectors 4 §6.3.5. 0 Vorkommen auf den 35
                // Dateien, aber die Klasse ist das Gegenteil von
                // `:read-only` und gehoert daneben.
                Pseudo::Lesbar
            } else if name.eq_ignore_ascii_case("required") {
                // Release 2026-10-15: HTML §4.16.3 (see `form_state`).
                Pseudo::Required
            } else if name.eq_ignore_ascii_case("optional") {
                Pseudo::Optional
            } else if name.eq_ignore_ascii_case("valid") {
                Pseudo::Valid
            } else if name.eq_ignore_ascii_case("invalid") {
                Pseudo::Invalid
            } else if name.eq_ignore_ascii_case("in-range") {
                Pseudo::InRange
            } else if name.eq_ignore_ascii_case("out-of-range") {
                Pseudo::OutOfRange
            } else if name.eq_ignore_ascii_case("default") {
                Pseudo::Default
            } else if name.eq_ignore_ascii_case("open") {
                Pseudo::Open
            } else if name.eq_ignore_ascii_case("first-of-type") {
                // CSS Selectors 4 §6.3.7. GEMESSEN: 51 Vorkommen.
                Pseudo::ErstVonTyp
            } else if name.eq_ignore_ascii_case("last-of-type") {
                // CSS Selectors 4 §6.3.7. GEMESSEN: 41 Vorkommen.
                Pseudo::LetztVonTyp
            } else if name.eq_ignore_ascii_case("only-of-type") {
                // CSS Selectors 4 §6.3.7. GEMESSEN: 0 Vorkommen.
                Pseudo::EinzigVonTyp
            } else if name.eq_ignore_ascii_case("before") {
                // 06.10.2026: die EINKLAMMER-Form `:before` (legacy,
                // dieselbe Bedeutung wie `::before`). Siehe `::`-Zweig.
                Pseudo::Vorher
            } else if name.eq_ignore_ascii_case("after") {
                Pseudo::Nachher
            } else if name.eq_ignore_ascii_case("first-letter") {
                // Release 10.10.2026: die Einklammer-Form (CSS 2.1).
                Pseudo::ErsterBuchstabe
            } else if name.eq_ignore_ascii_case("first-line") {
                // Release 15.10.2026 (Agent CSS2): the CSS 2.1 form.
                Pseudo::ErsteZeile
            } else if name == "-narsil-scope-anchor" {
                // Release 13.10.2026: `:scope` inside an `@scope` limit.
                Pseudo::ScopeAnchor
            } else if name.eq_ignore_ascii_case("heading") {
                // Release 2026-10-15: `:heading` (Selectors 5, HTML
                // headingoffset): any h1-h6.
                Pseudo::Heading(Vec::new())
            } else {
                return None;
            };
            einfach.pseudos.push(pseudo);
            Some((0, 1, 0, 2))
        }
        Some(Token::Funktion(name)) => {
            let n = name.to_ascii_lowercase();
            let has_list = ["not", "is", "where", "has", "host", "-narsil-scope-in", "-narsil-scope-limit"].contains(&n.as_str());
            // C1 (02.10.2026): `nth-last-of-type` fehlte in dieser
            // Liste. GEMESSENE FOLGE: der Name stand nicht im Waechter,
            // der Waecher warf ihn weg, und `return None` liess die GANZE
            // Regel fallen — nicht nur die Pseudoklasse. Das ist die
            // dritte Stelle dieser Art an diesem Tag (nach `:lang()`
            // und `:focus-within()`).
            let has_nth = [
                "nth-child",
                "nth-last-child",
                "nth-of-type",
                "nth-last-of-type",
            ]
            .contains(&n.as_str());
            // `:lang()` nimmt eine SPRACHLISTE und keine Selektorliste.
            // Sie braucht darum einen eigenen Schalter — mein erster
            // Versatz hat den Zweig NACH den Wächter gesetzt, und der
            // Wächter hat `lang` vorher weggeworfen. GEMESSEN: die Regel
            // kam mit `Dekl=0, verworfen=1` zurueck, genau wie vorher.
            let has_lang = n == "lang";
            // Release 15.10.2026 (Agent CSS2): `:dir()` passes the guard too.
            let has_dir = n == "dir";
            // Release 2026-10-15: `:heading(<integer>#)`.
            let has_heading = n == "heading";
            if !has_list && !has_nth && !has_lang && !has_dir && !has_heading {
                return None;
            }
            if in_not && (n == "not" || n == "has") {
                return None;
            }
            let mut k = i + 2;
            let mut tiefe = 0usize;
            let mut ende: Option<usize> = None;
            while k < scheibe.len() {
                match &scheibe[k] {
                    Token::Zeichen(')') if tiefe == 0 => {
                        ende = Some(k);
                        break;
                    }
                    Token::Funktion(_) | Token::Zeichen('(') | Token::Zeichen('[') => {
                        tiefe += 1;
                    }
                    Token::Zeichen(']') | Token::Zeichen(')') => tiefe = tiefe.saturating_sub(1),
                    _ => {}
                }
                k += 1;
            }
            let e = ende?;
            let innen_scheibe = &scheibe[i + 2..e];

            if n.eq_ignore_ascii_case("lang") {
                // Selectors Level 4 §6.5.17: `:lang(<sprachkennung>#)`
                // — eine Liste, kein Selektor. Deshalb steht der Zweig
                // VOR `has_list`: dort wuerde die Sprachliste als
                // Selektorliste gelesen und `ar` als Tag-Name.
                //
                // M3: hoechstens [`MAX_SPRACHEN`] Eintraege, jeder hoechstens
                // [`MAX_SPRACHLAENGE`] Zeichen. Die groesste echte
                // Sprachliste im Korpus hat 4 Kennungen; 256 laesst fuer
                // die Form „eine Liste je Seite" Luft, ohne dass eine
                // Angabe unbegrenzt waechst.
                let mut tags: Vec<String> = Vec::new();
                let mut stern = false;
                for tok in innen_scheibe.iter() {
                    match tok {
                        // GEMESSENER FEHLER, erster Fassung: Ich habe
                        // hier nur `Ident` und `Zeichenkette` genommen
                        // und alles andere mit `return None` abgewiesen.
                        // Damit fielen die wichtigsten Formen der
                        // Spezifikation: das TRENNZEICHEN `,` (Selects
                        // Level 4 §6.5.17, Sprachliste) und das
                        // `*` (dieselbe Stelle) sind beide
                        // `Token::Zeichen`, nicht `Ident`.
                        //
                        // Gemessen: `:lang(de, de-AT, en)` und
                        // `:lang(*)` fielen beide mit `return None`.
                        Token::Zeichen(',') => continue,
                        Token::Zeichen('*') => {
                            // `*` bedeutet „irgendeine Sprache".
                            stern = true;
                            continue;
                        }
                        Token::Leer => continue,
                        Token::Ident(s) | Token::Zeichenkette(s) => {
                            let s = s.trim().to_ascii_lowercase();
                            if s.is_empty() {
                                continue;
                            }
                            if tags.len() >= MAX_SPRACHEN {
                                break;
                            }
                            let kürz = if s.len() > MAX_SPRACHLAENGE {
                                &s[..MAX_SPRACHLAENGE]
                            } else {
                                s.as_str()
                            };
                            tags.push(String::from(kürz));
                        }
                        // `#` beendet die Liste (BCP 47, Script als
                        // Variante). Alles danach ist laut
                        // Spezifikation ungueltig.
                        Token::Zeichen('#') => break,
                        _ => return None,
                    }
                }
                // `:lang(*)` ohne weitere Kennung: „irgendeine Sprache".
                // Es braucht trotzdem ein `lang`-Attribut am Element —
                // ein Element ohne Sprache hat keine. Der Matcher
                // entscheidet das, hier steht nur, dass die Liste
                // nicht leer sein muss.
                //
                // `:lang()` GANZ ohne Angabe ist nach der
                // Spezifikation ungueltig (Selectors 4 §6.5.17: „the
                // argument is not optional") und passt auf nichts.
                if tags.is_empty() && !stern {
                    return None;
                }
                einfach.pseudos.push(Pseudo::Sprache(tags));
                return Some((0, 0, 0, e - i + 1));
            }

            // Release 2026-10-15: `:heading(<integer>#)` — the heading
            // levels (after `headingoffset`) that match.
            if has_heading {
                let args: Vec<&Token> = innen_scheibe.iter().filter(|t| !matches!(t, Token::Leer)).collect();
                let mut stufen = Vec::new();
                for (j, t) in args.iter().enumerate() {
                    match (j % 2, t) {
                        (0, Token::Zahl(z, e)) if e.is_empty() && !z.contains(['.', 'e', 'E']) => match z.parse::<i32>() {
                            Ok(v) => stufen.push(v),
                            Err(_) => return None,
                        },
                        (1, Token::Zeichen(',')) => {}
                        _ => return None,
                    }
                }
                if stufen.is_empty() || args.len() % 2 == 0 {
                    return None;
                }
                einfach.pseudos.push(Pseudo::Heading(stufen));
                return Some((0, 1, 0, e - i + 1));
            }

            // Release 15.10.2026 (Agent CSS2): `:dir(<ident>)` (Selectors 4
            // §7.1) — exactly one identifier; one other than `ltr`/`rtl` is
            // valid and matches nothing.
            if n.eq_ignore_ascii_case("dir") {
                let args: Vec<&Token> = innen_scheibe.iter().filter(|t| !matches!(t, Token::Leer)).collect();
                let want = match args.as_slice() {
                    [Token::Ident(s)] if s.eq_ignore_ascii_case("ltr") => Some(SchreibRichtung::Ltr),
                    [Token::Ident(s)] if s.eq_ignore_ascii_case("rtl") => Some(SchreibRichtung::Rtl),
                    [Token::Ident(_)] => None,
                    _ => return None,
                };
                einfach.pseudos.push(Pseudo::Richtung(want));
                return Some((0, 0, 0, e - i + 1));
            }

            if has_list {
                // NUR `:has()` ist relativ: `:not(> a)` ist nach
                // Selectors Level 4, 4.2 ausdruecklich ungueltig, und
                // `:is(> a)` ist erlaubt, aber nie verwendet — es hier
                // zuzulassen wuerde nur eine zweite Bedeutung derselben
                // Syntax schaffen.
                let liste = parse_selektorgruppe(innen_scheibe, n == "has");
                if liste.is_empty() {
                    return None;
                }
                let pseudo = match n.as_str() {
                    "not" => Pseudo::Nicht(liste),
                    "is" => Pseudo::Ist(liste),
                    "where" => Pseudo::Wo(liste),
                    "has" => Pseudo::Hat(liste),
                    "host" => Pseudo::Wirt(liste),
                    // Release 13.10.2026: the limit belongs to the scope right
                    // before it (`crate::scope_match`).
                    "-narsil-scope-in" => Pseudo::ScopeIn(liste, Vec::new()),
                    "-narsil-scope-limit" => {
                        let Some(Pseudo::ScopeIn(_, limits)) = einfach.pseudos.last_mut() else {
                            return None;
                        };
                        *limits = liste;
                        return Some((0, 0, 0, e - i + 1));
                    }
                    _ => return None,
                };
                einfach.pseudos.push(pseudo);
                return Some((0, 0, 0, e - i + 1));
            }

            if has_nth {
                let mut p = 0;
                while p < innen_scheibe.len() && matches!(innen_scheibe[p], Token::Leer) {
                    p += 1;
                }
                let mut end_nth = p;
                while end_nth < innen_scheibe.len() {
                    match &innen_scheibe[end_nth] {
                        Token::Ident(of) if of.eq_ignore_ascii_case("of") => break,
                        _ => end_nth += 1,
                    }
                }
                let mut of_liste = Vec::new();
                if end_nth < innen_scheibe.len() {
                    let mut start_of = end_nth + 1;
                    while start_of < innen_scheibe.len()
                        && matches!(innen_scheibe[start_of], Token::Leer)
                    {
                        start_of += 1;
                    }
                    of_liste = parse_selektorgruppe(&innen_scheibe[start_of..], false);
                    if of_liste.is_empty() {
                        return None;
                    }
                }
                let mut a_val = 0;
                let mut b_val = 0;
                let nth_slice = &innen_scheibe[p..end_nth];
                // Tokens in nth_slice zu einem String zusammenbauen fuer simplen Parser.
                let mut nth_str = String::new();
                for t in nth_slice {
                    match t {
                        Token::Ident(s) | Token::Zeichenkette(s) => nth_str.push_str(s),
                        Token::Zahl(z, e) => {
                            nth_str.push_str(z);
                            nth_str.push_str(e);
                        }
                        Token::Zeichen(c) => nth_str.push(*c),
                        _ => {}
                    }
                }
                nth_str.retain(|c| !c.is_whitespace());
                nth_str = nth_str.to_ascii_lowercase();
                if nth_str == "odd" {
                    a_val = 2;
                    b_val = 1;
                } else if nth_str == "even" {
                    a_val = 2;
                    b_val = 0;
                } else {
                    let mut is_n = false;
                    let mut num = String::new();
                    let mut b_str = String::new();
                    let mut in_b = false;
                    for c in nth_str.chars() {
                        if c == 'n' {
                            is_n = true;
                            if num.is_empty() || num == "+" {
                                a_val = 1;
                            } else if num == "-" {
                                a_val = -1;
                            } else {
                                a_val = num.parse().unwrap_or(0);
                            }
                        } else if c == '+' || c == '-' {
                            if is_n {
                                in_b = true;
                                b_str.push(c);
                            } else {
                                num.push(c);
                            }
                        } else {
                            if in_b {
                                b_str.push(c);
                            } else {
                                num.push(c);
                            }
                        }
                    }
                    if is_n {
                        if !b_str.is_empty() && b_str != "+" && b_str != "-" {
                            b_val = b_str.parse().unwrap_or(0);
                        }
                    } else {
                        b_val = num.parse().unwrap_or(0);
                    }
                }

                let pseudo = match n.as_str() {
                    "nth-child" => Pseudo::NtesKind(a_val, b_val, of_liste),
                    "nth-last-child" => Pseudo::NtesLetztesKind(a_val, b_val, of_liste),
                    "nth-of-type" => Pseudo::NtesTyp(a_val, b_val),
                    // C1 (02.10.2026): die Umkehrung. Siehe
                    // `Pseudo::NtesLetztesTyp`.
                    "nth-last-of-type" => Pseudo::NtesLetztesTyp(a_val, b_val),
                    _ => return None,
                };
                einfach.pseudos.push(pseudo);
                return Some((0, 0, 0, e - i + 1));
            }
            None
        }
        _ => None,
    }
}

// ===========================================================================
// DEKLARATIONEN (C3/C4)
// ===========================================================================

/// Ergebnis eines Deklarationsstuecks: normale Deklarationen und/oder
/// eine Variablendefinition (`--name`, C10, W6).
struct DeklStueck {
    dekls: Vec<Deklaration>,
    var: Option<VariablenDekl>,
}

/// Parst ein Deklarationsstueck (`name: wert !important`) zu 0-n
/// Deklarationen (Kurzformen expandieren) plus optional einer
/// Variablendefinition. Leeres Ergebnis = die Deklaration faellt still
/// weg (C4: Unbekanntes ignorieren).
fn parse_dekl(stueck: &[Token]) -> DeklStueck {
    let mut aus = DeklStueck {
        dekls: Vec::new(),
        var: None,
    };
    // Top-Level-Doppelpunkt suchen (Klammertiefe beachten).
    let mut tiefe = 0usize;
    let mut doppel: Option<usize> = None;
    for (idx, t) in stueck.iter().enumerate() {
        match t {
            Token::Funktion(_) | Token::Zeichen('(') | Token::Zeichen('[') => tiefe += 1,
            Token::Zeichen(')') | Token::Zeichen(']') => tiefe = tiefe.saturating_sub(1),
            Token::Zeichen(':') if tiefe == 0 => {
                doppel = Some(idx);
                break;
            }
            _ => {}
        }
    }
    let d = match doppel {
        None => return aus,
        Some(v) => v,
    };
    // Name: genau ein Ident (Leer davor/dahinter erlaubt). Gross/Klein
    // bleibt erhalten — erst der normale Weg kleinschreibt (Variablen
    // sind case-sensitiv, CSS-Regel, C10).
    let mut name: Option<String> = None;
    for t in &stueck[..d] {
        match t {
            Token::Leer => {}
            Token::Ident(n) if name.is_none() => name = Some(n.clone()),
            _ => return aus,
        }
    }
    let name = match name {
        None => return aus,
        Some(n) => n,
    };
    // Wert-Token (ohne Leer am Rand), `!important` abspalten.
    let mut wertscheibe = &stueck[d + 1..];
    while !wertscheibe.is_empty() && matches!(wertscheibe[0], Token::Leer) {
        wertscheibe = &wertscheibe[1..];
    }
    while !wertscheibe.is_empty() && matches!(wertscheibe[wertscheibe.len() - 1], Token::Leer) {
        wertscheibe = &wertscheibe[..wertscheibe.len() - 1];
    }
    if wertscheibe.is_empty() {
        return aus;
    }
    let mut wichtig = false;
    // Muster: ... [!] [Leer] important
    if let Some(Token::Ident(w)) = wertscheibe.last() {
        if w.eq_ignore_ascii_case("important") {
            let mut k = wertscheibe.len() - 1;
            while k > 0 && matches!(wertscheibe[k - 1], Token::Leer) {
                k -= 1;
            }
            if k > 0 && matches!(wertscheibe[k - 1], Token::Zeichen('!')) {
                wichtig = true;
                let mut ende = k - 1;
                while ende > 0 && matches!(wertscheibe[ende - 1], Token::Leer) {
                    ende -= 1;
                }
                wertscheibe = &wertscheibe[..ende];
            }
        }
    }
    if wertscheibe.is_empty() {
        return aus;
    }
    // Rekonstruktion (gedeckelt) und Auswertung je Eigenschaft.
    let text = wert_text(wertscheibe);
    // Variablendefinition: --name (case-sensitiv, Rest: Buchstaben,
    // Ziffern, -, _; Laenge gedeckelt, M4).
    if let Some(rest) = name.strip_prefix("--") {
        if !rest.is_empty()
            && rest.len() <= 64
            && rest
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            aus.var = Some(VariablenDekl {
                name,
                roh: text,
                wichtig,
            });
        }
        return aus;
    }
    let klein_name = name.to_ascii_lowercase();
    aus.dekls
        .extend(dekl_auswerten(&klein_name, &text, wichtig));
    aus
}

/// Baut den Wert-Rohtext (Leer bleibt Leer). Laenge gedeckelt auf
/// MAX_DEKL_WERT_LAENGE Zeichen (M4).
fn wert_text(scheibe: &[Token]) -> String {
    let mut s = String::new();
    for t in scheibe {
        let part = t.text();
        // Begrenzt: hoechstens Rest bis zur Grenze anhaengen.
        let rest = MAX_DEKL_WERT_LAENGE.saturating_sub(s.len());
        if rest == 0 {
            break;
        }
        if part.len() <= rest {
            s.push_str(&part);
        } else {
            // An Zeichengrenze kuerzen (max. 4 Schritte zurueck).
            let mut k = rest;
            while k > 0 && !part.is_char_boundary(k) {
                k -= 1;
            }
            s.push_str(&part[..k]);
            break;
        }
    }
    s.trim().to_string()
}

// ===========================================================================
// WERTE (C3)
// ===========================================================================

/// Endliche f32-Zahl oder None ("inf"/"NaN"/Muell fallen weg).
fn zahl_aus(s: &str) -> Option<f32> {
    let f: f32 = s.trim().parse().ok()?;
    if f.is_finite() { Some(f) } else { None }
}

/// Laenge mit Einheit. Einheitenlos ist nur 0 gueltig (als px); andere
/// einheitenlose Zahlen kommen als Zahl-Faktor zurueck (fuer
/// line-height/font-weight) — der Aufrufer prueft die Einheit je
/// Eigenschaft. Unbekannte Einheiten ergeben None.
fn laenge_aus(s: &str) -> Option<Laenge> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    // Zahl-Praefix (Ziffern, Punkt, Vorzeichen, Exponent) abspalten.
    let b = t.as_bytes();
    let mut k = 0usize;
    while k < b.len() && (b[k].is_ascii_digit() || b[k] == b'.' || b[k] == b'+' || b[k] == b'-') {
        k += 1;
    }
    // Exponent gehoert dazu ("1e3px"): e/E + Vorzeichen + Ziffern.
    if k < b.len() && (b[k] == b'e' || b[k] == b'E') {
        let mut q = k + 1;
        if q < b.len() && (b[q] == b'+' || b[q] == b'-') {
            q += 1;
        }
        let start_q = q;
        while q < b.len() && b[q].is_ascii_digit() {
            q += 1;
        }
        if q > start_q {
            k = q;
        }
    }
    let zahl = zahl_aus(&t[..k])?;
    let einheit = t[k..].trim().to_ascii_lowercase();
    let e = if einheit.is_empty() {
        if zahl == 0.0 {
            Einheit::Px
        } else {
            Einheit::Zahl
        }
    } else if einheit == "px" {
        Einheit::Px
    } else if einheit == "em" {
        Einheit::Em
    } else if einheit == "rem" {
        Einheit::Rem
    } else if einheit == "%" {
        Einheit::Prozent
    } else if einheit == "pt" {
        Einheit::Pt
    } else if einheit == "vw" {
        Einheit::Vw
    } else if einheit == "vh" {
        Einheit::Vh
    } else if einheit == "vmin" {
        Einheit::Vmin
    } else if einheit == "vmax" {
        Einheit::Vmax
    } else if einheit == "ch" {
        Einheit::Ch
    }
    // CSS Values 4, §6.1.2: die dynamischen (`d*`) und die logischen
    // (`l*`) Viewport-Einheiten.
    //
    // GEMESSENER FEHLER 01.10.2026 (K7c): `height: 100dvh` fiel weg —
    // 3x auf den 35 echten Stylesheets. `dvh` ist die seit 2022
    // haeufigste Art, eine vollhoehe Seite zu schreiben, weil die
    // Browserleisten auf dem Handy den Viewport veraendern.
    //
    // `svh`/`svmin`/`svewmin` (small viewport) fehlen: die unterscheiden
    // sich nur auf Geraeten mit einstellbaren Leisten, und dieser
    // Browser hat keine.
    else if einheit == "dvh" {
        Einheit::Dvh
    } else if einheit == "dvw" {
        Einheit::Dvw
    } else if einheit == "dvmin" {
        Einheit::Dvmin
    } else if einheit == "dvmax" {
        Einheit::Dvmax
    }
    // CSS Values 4 §6.1.2: `lh`/`rlh` — die Zeilenhoehe der Box bzw.
    // der Wurzel. Die Kaskade kennt die Zeile nicht (das ist der
    // Textfluss), also wird sie als `em` gerechnet: eine
    // DOKUMENTIERTE Naeherung.
    //
    // GEMESSENER FEHLER 01.10.2026 (K7c): 1x `height: 1lh` auf den 35
    // echten Stylesheets.
    else if einheit == "lh" {
        Einheit::Lh
    } else if einheit == "rlh" {
        Einheit::Rlh
    } else if einheit == "lvh" {
        Einheit::Lvh
    } else if einheit == "lvw" {
        Einheit::Lvw
    } else if einheit == "lvmin" {
        Einheit::Lvmin
    } else if einheit == "lvmax" {
        Einheit::Lvmax
    }
    // Release 12.10.2026 (Agent C): die kleinen (`sv*`) und die logischen
    // (`vi`/`vb`, waagerechte Schreibweise) Viewport-Einheiten (CSS Values 4
    // §6.1.2). GEMESSEN vorher (WPT css-values viewport-units-compute):
    // `height: 100vi` fiel weg.
    else if matches!(einheit.as_str(), "svw" | "vi" | "svi" | "lvi" | "dvi") {
        Einheit::Vw
    } else if matches!(einheit.as_str(), "svh" | "vb" | "svb" | "lvb" | "dvb") {
        Einheit::Vh
    } else if einheit == "svmin" {
        Einheit::Vmin
    } else if einheit == "svmax" {
        Einheit::Vmax
    } else {
        // Release 10.10.2026: die ABSOLUTEN Einheiten (CSS Values 4 §6.2:
        // 1in = 2,54cm = 96px, 1pc = 16px, 1Q = 1/40cm) sind exakt px, und
        // `ex` ist ohne x-Hoehe der Schrift 0,5em (die Naeherung aus
        // §6.1.1). GEMESSEN vorher (Web-Platform-Tests, `css/CSS2`):
        // `height: 1in` fiel weg, die Kaesten waren 0 px hoch.
        let px = match einheit.as_str() {
            "in" => Some(96.0),
            "cm" => Some(96.0 / 2.54),
            "mm" => Some(96.0 / 25.4),
            "q" => Some(96.0 / 101.6),
            "pc" => Some(16.0),
            _ => None,
        };
        if let Some(faktor) = px {
            return Some(Laenge { wert: zahl * faktor, einheit: Einheit::Px });
        }
        // Release 11.10.2026 (Agent CSS2): `ex` bleibt bis zur Kaskade
        // `ex` — erst dort ist die Schrift bekannt (CSS 2.1 §4.3.2: die
        // x-Hoehe der ersten verfuegbaren Schrift). Vorher fest 0.5 em.
        if einheit == "ex" {
            return Some(Laenge { wert: zahl, einheit: Einheit::Ex });
        }
        return None;
    };
    Some(Laenge {
        wert: zahl,
        einheit: e,
    })
}

/// Farbe aus Namen, #hex, rgb()/rgba(), hsl()/hsla(). None = ungueltig.
/// "transparent" ergibt DURCHSICHTIG; "currentcolor" kommt hier NICHT
/// an (der Aufrufer mappt es vorher auf Wert::Aktuell).
/// 03.10.2026 (Rolle CSS): `pub(crate)` statt privat. `kaskade.rs`
/// braucht es fuer `light-dark(<hell>, <dunkel>)`, das dort **schema-
/// abhaengig** aufgeloest wird (CSS Color 5 §6.2). Die Funktion selbst
/// ist unveraendert; nur die Sichtbarkeit wächst um eine Etage.
/// `srgb R G B [/ A]` — the inside of `color()` for the sRGB space; channels
/// as numbers (0..1) or percentages, `none` is 0.
fn srgb_farbe(inside: &str) -> Option<Farbe> {
    let (channels, alpha) = match inside.split_once('/') {
        Some((c, a)) => (c, Some(a.trim())),
        None => (inside, None),
    };
    let mut parts = channels.split_whitespace();
    if parts.next()? != "srgb" {
        return None;
    }
    let value = |t: &str| -> Option<f32> {
        if t == "none" {
            return Some(0.0);
        }
        let v = match t.strip_suffix('%') {
            Some(p) => p.parse::<f32>().ok()? / 100.0,
            None => t.parse::<f32>().ok()?,
        };
        v.is_finite().then_some(v)
    };
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let r = value(parts.next()?)?;
    let g = value(parts.next()?)?;
    let b = value(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let a = match alpha {
        Some(a) => value(a)?,
        None => 1.0,
    };
    Some(Farbe { r: byte(r), g: byte(g), b: byte(b), a: byte(a) })
}

pub fn farbe_aus(s: &str) -> Option<Farbe> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    // #hex: 3/4/6/8 Stellen.
    if let Some(hex) = t.strip_prefix('#') {
        return hex_farbe(hex);
    }
    let klein = t.to_ascii_lowercase();
    if klein == "transparent" {
        return Some(Farbe::DURCHSICHTIG);
    }
    // Release 14.10.2026 (Agent C): math functions inside a color function
    // (`rgb(calc(5% + 15%), …)`) are resolved by the CSSOM color module
    // first. MEASURED before (WPT css-values calc-rgb-percent-001): the
    // declaration was dropped (transparent instead of rgb(51, 51, 51)).
    if ["calc(", "min(", "max(", "clamp("].iter().any(|f| klein.contains(f)) {
        let kv = crate::cssom::syntax::komponenten(t);
        if let [k] = crate::cssom::syntax::getrimmt(&kv)
            && let Some(f) = crate::cssom::farbe::lesen(k)
        {
            let text = f.berechnet(None);
            let lower = text.to_ascii_lowercase();
            if !["calc(", "min(", "max(", "clamp("].iter().any(|f| lower.contains(f)) {
                return farbe_aus(&text);
            }
        }
    }
    // Funktionen rgb()/rgba()/hsl()/hsla() (Leerraum-tolerant).
    for name in ["rgb(", "rgba(", "hsl(", "hsla("] {
        if let Some(rest) = klein.strip_prefix(name) {
            if let Some(innen) = rest.strip_suffix(')') {
                // CSS Color 5, §6.2: `light-dark(hell, dunkel)` waehlt
                // nach `color-scheme`.
                //
                // GEMESSENER FEHLER 01.10.2026 (K7c): 16 Faelle auf den
                // 35 echten Stylesheets — 8 in `@supports
                // (color: light-dark(red, red))`, die MDN als
                // EIGENES Merkmal prueft, und 8 als echte Angabe.
                //
                // DIESER BROWSER HAT KEINE `color-scheme`-Auswertung
                // (er hat keinen Anschluss an die Systemeinstellung),
                // also ist die ehrliche Antwort das HELL-Feld:
                // `light-dark(red, tan)` ist unter der Vorgabe
                // `color-scheme: light` genau `red`, und die Dateien
                // setzen `color-scheme: light dark` neben sich.
                //
                // BEHAUPTUNG, DIE ICH NICHT MACHE: es wird nicht
                // geraten, welches Feld „richtig" ist — es wird das
                // dokumentierte erste genommen, und der Unterschied
                // ist in WUNSCH_CSS.txt W12 als offen vermerkt.
                if name.starts_with("light-dark") {
                    let teile = teile_komma(innen);
                    if teile.len() != 2 {
                        return None;
                    }
                    return farbe_aus(teile[0]);
                }
                if name.starts_with("rgb") {
                    return rgb_farbe(innen);
                } else {
                    return hsl_farbe(innen);
                }
            } else {
                return None;
            }
        }
    }
    // `hwb()` (CSS Color 4, 7.3) — R0 e, 30.09.2026: war unbekannt, und
    // damit wurde die DEKLARATION verworfen. Wer nach dem Dunkelmodus
    // fragt und `hwb` benutzt, bekam Schwarz statt der Farbe.
    if let Some(rest) = klein.strip_prefix("hwb(") {
        let innen = rest.strip_suffix(')')?;
        return hwb_farbe(innen);
    }
    // Release 14.10.2026 (Agent C): `color(srgb R G B [/ A])` (CSS Color 4
    // §10), also the computed value of relative colors. MEASURED before (WPT
    // css-values clamp-color-computed in a sheet): the declaration was dropped.
    if let Some(rest) = klein.strip_prefix("color(") {
        return srgb_farbe(rest.strip_suffix(')')?);
    }
    // Release 08.10.2026: `oklch()`, `oklab()`, `lch()`, `lab()` (CSS
    // Color 4 §8-9). GEMESSEN vorher: unbekannt, die Deklaration fiel
    // weg — Tailwind 4 schreibt JEDE Farbe der Palette als `oklch()`.
    for (name, raum) in [
        ("oklch(", LabRaum::OkLch),
        ("oklab(", LabRaum::OkLab),
        ("lch(", LabRaum::Lch),
        ("lab(", LabRaum::Lab),
    ] {
        if let Some(rest) = klein.strip_prefix(name) {
            let innen = rest.strip_suffix(')')?;
            return lab_farbe(innen, raum);
        }
    }
    // `color-mix(in <raum>, A <pct>%, B)` (CSS Color 5, 4) — R0 e.
    if let Some(rest) = klein.strip_prefix("color-mix(") {
        let innen = rest.strip_suffix(')')?;
        return color_mix_farbe(innen);
    }
    // CSS Color 5 §6.2: `light-dark(<hell>, <dunkel>)`.
    //
    // GEMESSENER FEHLER 01.10.2026 (K7c): 16 Faelle auf den 35 echten
    // Stylesheets — 8 davon in `@supports (color: light-dark(red, red))`,
    // die MDN als EIGENES Merkmal prueft, und 8 als echte Angabe.
    //
    // DIESER BROWSER HAT KEINE `color-scheme`-Auswertung (kein Anschluss
    // an die Systemeinstellung). Die ehrliche Antwort ist darum das
    // HELL-Feld: `light-dark(red, tan)` ist unter der Vorgabe
    // `color-scheme: light` genau `red` — und die Dateien daneben
    // schreiben `color-scheme: light dark`.
    //
    // WAS ICH NICHT BEHAUPTE: dass das hell-Feld das richtige ist. Ohne
    // die Systemeinstellung gibt es kein „richtig". Der Unterschied
    // ist in WUNSCH_CSS.txt W12 als offene Frage vermerkt.
    // CSS Color 5 §6.2: `light-dark(<hell>, <dunkel>)`.
    //
    // DIE ENTSCHEIDUNG (K7c, 02.10. bestaetigt): diese Crate liest
    // IMMER das helle Feld, weil das Schema nicht hier hereinkommt —
    // das Stylesheet wird einmal gelesen und fuer alle Knoten benutzt.
    // Die Frage, ob der BROWSER die Funktion unterstuetzt, ist eine
    // andere Frage als die Frage, welchen Wert sie hier liefert, und
    // sie wird in `supports.rs` beantwortet (siehe dort).
    //
    // GEMESSEN, warum das ein GESONDERTER Ort ist: `@supports (color:
    // light-dark(red, red))` war `true`, weil diese Funktion hier
    // lesbar ist. Seit K13 wird aber `@media (prefers-color-scheme:
    // dark)` ausgewertet — im dunklen Schema wird also der `@media`-
    // Zweig angewendet, waehrend so ein `@supports`-Zweig die Farben auf
    // das helle Feld setzt. MDN macht genau das.
    //
    // Also: HIER das hell-Feld liefern (besser als nichts), und in
    // `supports.rs` melden, dass die Funktion nicht unterstuetzt wird
    // (ehrlich gegenueber der Seite).
    if let Some(rest) = klein.strip_prefix("light-dark(") {
        let innen = rest.strip_suffix(')')?;
        let teile = teile_komma(innen);
        if teile.len() != 2 {
            return None;
        }
        return farbe_aus(teile[0]);
    }
    // Name (bereits kleingeschrieben vergleichen).
    farben::name_suchen(&klein).map(|(r, g, b)| Farbe { r, g, b, a: 255 })
}

/// #rgb/#rgba/#rrggbb/#rrggbbaa. Andere Laengen/Zeichen: None.
fn hex_farbe(hex: &str) -> Option<Farbe> {
    let b = hex.as_bytes();
    let ziffer = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
    let paar = |h: u8, l: u8| Some(ziffer(h)? * 16 + ziffer(l)?);
    let einzel = |h: u8| Some(ziffer(h)? * 17);
    match b.len() {
        3 => Some(Farbe {
            r: einzel(b[0])?,
            g: einzel(b[1])?,
            b: einzel(b[2])?,
            a: 255,
        }),
        4 => Some(Farbe {
            r: einzel(b[0])?,
            g: einzel(b[1])?,
            b: einzel(b[2])?,
            a: einzel(b[3])?,
        }),
        6 => Some(Farbe {
            r: paar(b[0], b[1])?,
            g: paar(b[2], b[3])?,
            b: paar(b[4], b[5])?,
            a: 255,
        }),
        8 => Some(Farbe {
            r: paar(b[0], b[1])?,
            g: paar(b[2], b[3])?,
            b: paar(b[4], b[5])?,
            a: paar(b[6], b[7])?,
        }),
        _ => None,
    }
}

/// Teilt Funktionsargumente an Kommas oder (ohne Komma) an
/// Schraegezeichen/Leer (CSS Color 4: "rgb(255 0 0 / 50%)").
fn farb_args(innen: &str, max: usize) -> Option<(Vec<String>, Option<String>)> {
    if innen.len() > 256 {
        return None;
    }
    let alpha: Option<String>;
    let teile: Vec<String>;
    if innen.contains(',') {
        // Kommaform: genau 3 oder 4 Teile.
        teile = innen.split(',').map(|s| s.trim().to_string()).collect();
        if teile.len() < 3 || teile.len() > 4 {
            return None;
        }
        if teile.iter().any(|s| s.is_empty() || s.len() > 64) {
            return None;
        }
        let a = if teile.len() == 4 {
            Some(teile[3].clone())
        } else {
            None
        };
        alpha = a;
        return Some((teile[..3].to_vec(), alpha));
    }
    // Leerform: "r g b" oder "r g b / a".
    let (haupt, a) = match innen.split_once('/') {
        Some((h, v)) => (h, Some(v.trim().to_string())),
        None => (innen, None),
    };
    let v: Vec<String> = haupt.split_whitespace().map(String::from).collect();
    if v.len() != 3 || v.iter().any(|s| s.is_empty() || s.len() > 64) {
        return None;
    }
    if v.len() + a.is_some() as usize > max {
        return None;
    }
    alpha = a;
    Some((v, alpha))
}

/// 0-255 aus Zahl oder Prozent (ausserhalb wird gekappt — CSS-Regel).
fn kanal_aus(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Some(p) = t.strip_suffix('%') {
        let f = zahl_aus(p)?;
        Some((f * 255.0 / 100.0).clamp(0.0, 255.0).round() as u8)
    } else {
        let f = zahl_aus(t)?;
        Some(f.clamp(0.0, 255.0).round() as u8)
    }
}

/// 0-255 aus Alpha (0-1 Zahl oder %).
fn alpha_aus(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Some(p) = t.strip_suffix('%') {
        let f = zahl_aus(p)?;
        Some((f * 255.0 / 100.0).clamp(0.0, 255.0).round() as u8)
    } else {
        let f = zahl_aus(t)?;
        Some((f * 255.0).clamp(0.0, 255.0).round() as u8)
    }
}

/// rgb()/rgba().
fn rgb_farbe(innen: &str) -> Option<Farbe> {
    let (v, a) = farb_args(innen, 4)?;
    Some(Farbe {
        r: kanal_aus(&v[0])?,
        g: kanal_aus(&v[1])?,
        b: kanal_aus(&v[2])?,
        a: match a {
            None => 255,
            Some(s) => alpha_aus(&s)?,
        },
    })
}

/// Release 15.10.2026: a hue (`<number>` or `<angle>`: deg, grad, rad,
/// turn) in degrees. MEASURED before: `hsl(133.33333333grad, 100%, 50%)`
/// was invalid (WPT html/canvas 2d.fillStyle.parse.css-color-4-hsl-*).
fn farbton_aus(t: &str) -> Option<f32> {
    let t = t.trim();
    let klein = t.to_ascii_lowercase();
    for (einheit, faktor) in [("deg", 1.0f32), ("grad", 0.9), ("rad", 180.0 / std::f32::consts::PI), ("turn", 360.0)] {
        if let Some(z) = klein.strip_suffix(einheit) {
            if z.contains(char::is_alphabetic) {
                return None;
            }
            return Some(zahl_aus(z)? * faktor);
        }
    }
    if klein.contains(char::is_alphabetic) {
        return None;
    }
    zahl_aus(&klein)
}

/// hsl()/hsla(): h in Grad (Umlauf modulo 360), s/l in %.
fn hsl_farbe(innen: &str) -> Option<Farbe> {
    let (v, a) = farb_args(innen, 4)?;
    let h = farbton_aus(&v[0])?;
    let s = prozent_01(&v[1])?;
    let l = prozent_01(&v[2])?;
    let (r, g, b) = hsl_nach_rgb(h, s, l);
    Some(Farbe {
        r,
        g,
        b,
        a: match a {
            None => 255,
            Some(x) => alpha_aus(&x)?,
        },
    })
}

/// `hwb(H W% B%)`: Farbton, Weissanteil, Schwarzanteil.
///
/// CSS Color 4, 7.3: die Rechnung ist `rgb = hwb_hsl(H, 0, 0.5)`, dann
/// wird mit Weiss gemischt (nicht multipliziert) und danach Schwarz
/// GESETZT (nicht multipliziert). Der Unterschied ist sichtbar:
/// `hwb(0 0% 100%)` ist Schwarz, `hwb(0 100% 0%)` ist Weiss.
///
/// Der Farbton wird wie bei `hsl()` gelesen (Grad mit optionalem `deg`,
/// Umlauf modulo 360).
fn hwb_farbe(innen: &str) -> Option<Farbe> {
    // `farb_args` liefert (Argumente, optionales Alpha als Text) — das
    // Alpha steht bei `rgb()`/`hsl()` als viertes Argument, bei `hwb()`
    // gibt es keines.
    let (v, _a) = farb_args(innen, 3)?;
    let h = farbton_aus(&v[0])?;
    let w = prozent_01(&v[1])?;
    let b_anteil = prozent_01(&v[2])?;
    if w + b_anteil > 1.0 {
        // CSS Color 4, 7.3: "if w + b >= 1, the color is a gray with the
        // value w + b - 1". `hwb(0 0% 100%)` ist danach 0 + 1 - 1 = 0,
        // also SCHWARZ. Der Mittelwert (gemessen als Fehler: er ergab
        // 128) waere bei `hwb(0 50% 50%)` richtig und bei
        // `hwb(0 0% 100%)` falsch - und die zweite Form ist die, die
        // wirklich vorkommt.
        let g = (w + b_anteil - 1.0).clamp(0.0, 1.0);
        let grau = (g * 255.0).round().clamp(0.0, 255.0) as u8;
        return Some(Farbe {
            r: grau,
            g: grau,
            b: grau,
            a: 255,
        });
    }
    let (r, g, bl) = hsl_nach_rgb(h, 1.0, 0.5);
    let m = |x: u8| ((x as f32 / 255.0) * (1.0 - w - b_anteil) + w) * 255.0;
    Some(Farbe {
        r: m(r).round().clamp(0.0, 255.0) as u8,
        g: m(g).round().clamp(0.0, 255.0) as u8,
        b: m(bl).round().clamp(0.0, 255.0) as u8,
        a: 255,
    })
}

/// Die vier Lab-artigen Farbraeume von CSS Color 4.
#[derive(Clone, Copy)]
enum LabRaum {
    OkLch,
    OkLab,
    Lch,
    Lab,
}

/// `oklch(L C H [/ A])`, `oklab(L a b [/ A])`, `lch(...)`, `lab(...)`.
///
/// Prozentbasen nach CSS Color 4: OKLab-L 1, a/b und C 0.4; CIE-L 100,
/// a/b 125, C 150. `none` ist 0 (§4.4). Ausserhalb von sRGB wird
/// gekappt — keine Gamut-Abbildung, aber die Farbe stimmt im Inneren.
fn lab_farbe(innen: &str, raum: LabRaum) -> Option<Farbe> {
    if innen.len() > 256 {
        return None;
    }
    let (haupt, alpha) = match innen.split_once('/') {
        Some((h, a)) => (h, Some(a.trim())),
        None => (innen, None),
    };
    let teile: Vec<&str> = haupt.split_whitespace().collect();
    if teile.len() != 3 {
        return None;
    }
    let zahl = |t: &str, basis: f32| -> Option<f32> {
        if t.eq_ignore_ascii_case("none") {
            return Some(0.0);
        }
        if let Some(p) = t.strip_suffix('%') {
            return Some(zahl_aus(p)? / 100.0 * basis);
        }
        zahl_aus(t)
    };
    let winkel = |t: &str| -> Option<f32> {
        if t.eq_ignore_ascii_case("none") {
            return Some(0.0);
        }
        // `grad` vor `rad` — sonst liest `rad` das Ende von `grad`.
        if let Some(g) = t.strip_suffix("deg") {
            return zahl_aus(g);
        }
        if let Some(g) = t.strip_suffix("grad") {
            return Some(zahl_aus(g)? * 0.9);
        }
        if let Some(g) = t.strip_suffix("rad") {
            return Some(zahl_aus(g)?.to_degrees());
        }
        if let Some(g) = t.strip_suffix("turn") {
            return Some(zahl_aus(g)? * 360.0);
        }
        zahl_aus(t)
    };
    let polar = |l: f32, c: f32, h: f32| (l, c * h.to_radians().cos(), c * h.to_radians().sin());
    let (lin_r, lin_g, lin_b) = match raum {
        LabRaum::OkLch => {
            let (l, a, b) = polar(zahl(teile[0], 1.0)?, zahl(teile[1], 0.4)?, winkel(teile[2])?);
            oklab_nach_linear(l, a, b)
        }
        LabRaum::OkLab => oklab_nach_linear(
            zahl(teile[0], 1.0)?,
            zahl(teile[1], 0.4)?,
            zahl(teile[2], 0.4)?,
        ),
        LabRaum::Lch => {
            let (l, a, b) = polar(zahl(teile[0], 100.0)?, zahl(teile[1], 150.0)?, winkel(teile[2])?);
            lab_nach_linear(l, a, b)
        }
        LabRaum::Lab => lab_nach_linear(
            zahl(teile[0], 100.0)?,
            zahl(teile[1], 125.0)?,
            zahl(teile[2], 125.0)?,
        ),
    };
    Some(Farbe {
        r: srgb_kodieren(lin_r),
        g: srgb_kodieren(lin_g),
        b: srgb_kodieren(lin_b),
        a: match alpha {
            None => 255,
            Some(t) if t.eq_ignore_ascii_case("none") => 0,
            Some(t) => alpha_aus(t)?,
        },
    })
}

/// OKLab -> lineares sRGB (Bjoern Ottosson, die Matrizen aus CSS Color 4 §18).
fn oklab_nach_linear(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    )
}

/// CIE Lab (D50) -> XYZ (D50) -> XYZ (D65, Bradford) -> lineares sRGB
/// (CSS Color 4 §18).
fn lab_nach_linear(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    const EPS: f32 = 216.0 / 24389.0;
    const KAPPA: f32 = 24389.0 / 27.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let xr = if fx * fx * fx > EPS { fx * fx * fx } else { (116.0 * fx - 16.0) / KAPPA };
    let yr = if l > KAPPA * EPS { fy * fy * fy } else { l / KAPPA };
    let zr = if fz * fz * fz > EPS { fz * fz * fz } else { (116.0 * fz - 16.0) / KAPPA };
    let (x, y, z) = (xr * 0.964_22, yr, zr * 0.825_21);
    let x65 = 0.955_473_45 * x - 0.023_098_537 * y + 0.063_259_31 * z;
    let y65 = -0.028_369_707 * x + 1.009_995_5 * y + 0.021_041_399 * z;
    let z65 = 0.012_314_002 * x - 0.020_507_696 * y + 1.330_366 * z;
    (
        3.240_97 * x65 - 1.537_383_2 * y65 - 0.498_610_76 * z65,
        -0.969_243_65 * x65 + 1.875_967_5 * y65 + 0.041_555_06 * z65,
        0.055_630_08 * x65 - 0.203_976_96 * y65 + 1.056_971_5 * z65,
    )
}

/// Lineares sRGB -> 0-255 mit der sRGB-Kennlinie (gekappt).
fn srgb_kodieren(x: f32) -> u8 {
    let x = if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
    let v = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// `color-mix(in <raum>, A [P%], B [Q%])` (CSS Color 5 §2).
///
/// Gemischt wird in sRGB, linearem Licht oder OKLab — je nach Raum
/// (siehe unten). Ohne Prozentangabe ist der Anteil 50/50.
fn color_mix_farbe(innen: &str) -> Option<Farbe> {
    // M4: Der Innentext kommt aus der Datei. 1024 Zeichen reichen fuer
    // jede gueltige Angabe (die laengste ist `color-mix(in srgb,
    // rgba(0,0,0,0.5) 50%, rgb(1,2,3))`).
    if innen.len() > 1024 {
        return None;
    }
    let klein = innen.trim().to_ascii_lowercase();
    // Release 08.10.2026: Kommas nur auf oberster Ebene — vorher wurde an
    // JEDEM Komma geteilt, und eine zweite Farbe mit Kommas
    // (`rgb(1,2,3)`) zerfiel.
    let teile = teile_komma(&klein);
    if teile.len() != 3 {
        return None;
    }
    let raum = teile[0].trim().strip_prefix("in")?.trim();
    if raum.is_empty() {
        return None;
    }
    // Ein Anteil `P%` darf HINTER jeder der beiden Farben stehen.
    let anteil_lesen = |t: &str| -> (String, Option<f32>) {
        let t = t.trim();
        match t.rsplit_once(char::is_whitespace) {
            Some((links, rechts)) if rechts.ends_with('%') && prozent_01(rechts).is_some() => {
                (links.trim().to_string(), prozent_01(rechts))
            }
            _ => (t.to_string(), None),
        }
    };
    let (erste_roh, p1) = anteil_lesen(teile[1]);
    let (zweite_roh, p2) = anteil_lesen(teile[2]);
    if erste_roh.is_empty() || zweite_roh.is_empty() {
        return None;
    }
    // CSS Color 5 §2.1: fehlt ein Anteil, ist er der Rest auf 100 %;
    // summieren sich beide zu weniger als 100 %, wird die Deckkraft
    // entsprechend kleiner.
    let (anteil, faktor) = match (p1, p2) {
        (None, None) => (0.5, 1.0),
        (Some(a), None) => (a, 1.0),
        (None, Some(b)) => (1.0 - b, 1.0),
        (Some(a), Some(b)) => {
            let summe = a + b;
            if summe <= 0.0 {
                return None;
            }
            (a / summe, summe.min(1.0))
        }
    };
    let a = farbe_aus(&erste_roh)?;
    let b = farbe_aus(&zweite_roh)?;
    let anteil_b = 1.0 - anteil;
    // CSS Color 4 §12.3: mit vormultiplizierter Deckkraft mischen —
    // erst Kanal x Deckkraft, dann interpolieren, dann durch die neue
    // Deckkraft teilen. GEMESSEN (K1, 01.10.2026): ohne das wurde aus
    // `color-mix(in srgb, red 50%, transparent)` ein dunkles Rot.
    let da = a.a as f32 / 255.0;
    let db = b.a as f32 / 255.0;
    let deck = anteil * da + anteil_b * db;
    if deck <= 0.0 {
        // Beide Anteile durchsichtig: es gibt keine Farbe, nur NICHTS.
        return Some(Farbe::DURCHSICHTIG);
    }
    let mische = |x: [f32; 3], y: [f32; 3]| -> [f32; 3] {
        let mut raus = [0.0; 3];
        for i in 0..3 {
            raus[i] = (anteil * x[i] * da + anteil_b * y[i] * db) / deck;
        }
        raus
    };
    let roh = |f: Farbe| [f.r as f32 / 255.0, f.g as f32 / 255.0, f.b as f32 / 255.0];
    // Release 08.10.2026: die Mischung im angegebenen Raum. GEMESSEN
    // vorher: nur `srgb` (und `srgb-linear` falsch als `srgb`), alles
    // andere fiel weg — Tailwind 4 schreibt jede Deckkraft-Variante
    // (`bg-black/50`) als `color-mix(in oklab, ...)`. `lab`/`lch`/`oklch`
    // werden in OKLab gemischt (der Farbton laeuft dann gerade statt im
    // Bogen — eine kleine Abweichung), `xyz*`/`display-p3` linear.
    let raum_name = raum.split_whitespace().next().unwrap_or("");
    let [r, g, bl] = match raum_name {
        "srgb" | "hsl" | "hwb" => mische(roh(a), roh(b)),
        "srgb-linear" | "xyz" | "xyz-d50" | "xyz-d65" | "display-p3" | "a98-rgb"
        | "prophoto-rgb" | "rec2020" => {
            let lin = |f: Farbe| roh(f).map(srgb_dekodieren);
            mische(lin(a), lin(b)).map(srgb_kodieren_f)
        }
        "oklab" | "oklch" | "lab" | "lch" => {
            let ok = |f: Farbe| {
                let [r, g, b] = roh(f).map(srgb_dekodieren);
                linear_nach_oklab(r, g, b)
            };
            let [l, aa, bb] = mische(ok(a), ok(b));
            let (r, g, b) = oklab_nach_linear(l, aa, bb);
            [r, g, b].map(srgb_kodieren_f)
        }
        _ => return None,
    };
    let kanal = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(Farbe {
        r: kanal(r),
        g: kanal(g),
        b: kanal(bl),
        a: (deck * faktor * 255.0).round().clamp(0.0, 255.0) as u8,
    })
}

/// sRGB-Kanal (0-1) -> linear.
fn srgb_dekodieren(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear -> sRGB-Kanal (0-1), gekappt.
fn srgb_kodieren_f(x: f32) -> f32 {
    srgb_kodieren(x) as f32 / 255.0
}

/// Lineares sRGB -> OKLab (CSS Color 4 §18).
fn linear_nach_oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    ]
}

/// Release 08.10.2026: ersetzt `env(name[, ersatz])` (CSS Environment
/// Variables 1). Ein Schreibtischfenster hat keine Aussparungen:
/// `safe-area-inset-*`, `safe-area-max-inset-*` und `keyboard-inset-*`
/// sind `0px`; bei einem unbekannten Namen gilt der Ersatz, sonst bleibt
/// `env(` stehen und die Deklaration ist ungueltig (§3: wie `var()`).
///
/// GEMESSEN vorher: `padding-bottom: env(safe-area-inset-bottom)` fiel
/// als „Wert ungeprueft" weg. M3: hoechstens 16 Ersetzungen je Wert.
fn env_ersetzen(wert: &str) -> String {
    let mut raus = String::with_capacity(wert.len());
    let mut rest = wert;
    for _ in 0..16 {
        let klein = rest.to_ascii_lowercase();
        let Some(start) = klein.find("env(") else { break };
        raus.push_str(&rest[..start]);
        let innen_start = start + 4;
        let b = rest.as_bytes();
        let mut tiefe = 1usize;
        let mut i = innen_start;
        while i < b.len() && tiefe > 0 {
            match b[i] {
                b'(' => tiefe += 1,
                b')' => tiefe -= 1,
                _ => {}
            }
            i += 1;
        }
        if tiefe != 0 {
            // Unabgeschlossen: so lassen, die Deklaration faellt spaeter weg.
            raus.push_str(&rest[start..]);
            return raus;
        }
        let innen = &rest[innen_start..i - 1];
        let (name, ersatz) = match teile_komma(innen).as_slice() {
            [n] => (n.trim().to_ascii_lowercase(), None),
            [n, e, ..] => (n.trim().to_ascii_lowercase(), Some(e.trim())),
            _ => (String::new(), None),
        };
        let bekannt = ["safe-area-inset-", "safe-area-max-inset-", "keyboard-inset-"]
            .iter()
            .any(|p| name.starts_with(p));
        if bekannt {
            raus.push_str("0px");
        } else if let Some(e) = ersatz {
            raus.push_str(e);
        } else {
            raus.push_str(&rest[start..i]);
        }
        rest = &rest[i..];
    }
    raus.push_str(rest);
    raus
}

/// Eine Maske mit Anfangswerten (CSS Masking 1, §4).
///
/// `mask-size` beginnt mit `auto`, `mask-position` mit `center`, und
/// `mask-repeat` mit `repeat` (§4.4). Das sind drei verschiedene
/// Anfangswerte fuer drei verschiedene Eigenschaften — sie stehen
/// deshalb hier und nicht in einem einzigen `Default`.
fn maske_mit(
    url: String,
    groesse: MaskGroesse,
    position_x: f32,
    position_y: f32,
    wiederholen_x: bool,
    wiederholen_y: bool,
) -> Maske {
    Maske {
        url,
        groesse,
        position_x,
        position_y,
        verlauf: None,
        wiederholen_x,
        wiederholen_y,
    }
}

/// `mask-image` (CSS Masking 1, §4.1): eine kommagetrennte Liste von
/// Ebenen, die ERSTE oben.
///
/// `none` ist eine Ebene mit leerer URL — so bleibt die Ebenenzahl
/// erhalten, auch wenn eine Ebene kein Bild traegt.
///
/// M4: mehr als [`crate::MAX_MASKEN`] faellt GANZ weg. Abschneiden
/// wuerde die unteren Ebenen verlieren, ohne dass man es sieht.
fn masken_bild_aus_roh(roh: &str) -> Option<Wert> {
    let klein = roh.trim();
    if klein.eq_ignore_ascii_case("none") {
        return Some(Wert::Maske(vec![maske_mit(
            String::new(),
            MaskGroesse::Auto,
            0.5,
            0.5,
            true,
            true,
        )]));
    }
    // Die Teile werden aus dem ROHTEXT geschnitten, nicht aus der
    // Kleinschreibung: eine `data:image/svg+xml;base64,…`-URL ist
    // case-sensitiv. Nur die Schluesselwoerter werden verglichen.
    let teile = teile_komma(roh.trim());
    if teile.is_empty() {
        return None;
    }
    let mut raus: Vec<Maske> = Vec::new();
    for t in teile.iter() {
        if raus.len() >= crate::MAX_MASKEN {
            return None;
        }
        // CSS Masking 1 §3.2: `mask-image` nimmt dieselben Bildquellen
        // wie `background-image` (CSS Images 3 §2) — also auch
        // VERLAEUFE.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): 3 Verwerfe auf den 35
        // echten Stylesheets, darunter
        // `linear-gradient(90deg,#0000,#000 20% 80%,#0000)` und
        // `radial-gradient(#000 70%,#0000 71%),linear-gradient(#000 0 0)`.
        match url_innern(t) {
            Some(u) => raus.push(maske_mit(u, MaskGroesse::Auto, 0.5, 0.5, true, true)),
            None => match verlauf_aus(t) {
                Some(v) => {
                    let mut m = maske_mit(String::new(), MaskGroesse::Auto, 0.5, 0.5, true, true);
                    m.verlauf = Some(v);
                    raus.push(m);
                }
                // Weder URL noch Verlauf: die Ebene ist ungueltig.
                None => return None,
            },
        }
    }
    if raus.is_empty() {
        return None;
    }
    Some(Wert::Maske(raus))
}

/// Release 10.10.2026: die Kurzform `mask` / `-webkit-mask` (CSS Masking 1
/// §4.12) — je Ebene Bild, Position `[/ Groesse]`, Wiederholung und die
/// Kasten-/Modus-Woerter in freier Reihenfolge. Gelesen aus dem ROHTEXT
/// (die URL ist case-sensitiv).
///
/// GEMESSEN vorher: die Kurzform war unbekannt und fiel weg. youtube.com
/// zeichnet jedes Symbol als `background-color: currentColor` +
/// `-webkit-mask: url(…24px.svg) no-repeat center / contain` — ohne Maske
/// blieb ein volles Quadrat in Textfarbe.
fn masken_kurz_aus_roh(roh: &str) -> Option<Wert> {
    let roh = roh.trim();
    if roh.eq_ignore_ascii_case("none") {
        return masken_bild_aus_roh(roh);
    }
    let ebenen = teile_komma(roh);
    if ebenen.is_empty() || ebenen.len() > crate::MAX_MASKEN {
        return None;
    }
    let mut raus: Vec<Maske> = Vec::with_capacity(ebenen.len());
    for ebene in ebenen {
        // `/` trennt Position und Groesse, auch ohne Leerraum
        // (`center/contain`) — ausserhalb von Klammern freigestellt.
        let mut text = String::with_capacity(ebene.len() + 4);
        let mut tiefe = 0usize;
        for c in ebene.chars() {
            match c {
                '(' => tiefe += 1,
                ')' => tiefe = tiefe.saturating_sub(1),
                _ => {}
            }
            if c == '/' && tiefe == 0 {
                text.push_str(" / ");
            } else {
                text.push(c);
            }
        }
        let mut m = maske_mit(String::new(), MaskGroesse::Auto, 0.5, 0.5, true, true);
        let mut lage: Vec<String> = Vec::new();
        let mut groesse: Vec<String> = Vec::new();
        let mut nach_strich = false;
        for t in teile_wert(&text) {
            let k = t.to_ascii_lowercase();
            if k == "/" {
                nach_strich = true;
                continue;
            }
            if k.starts_with("url(") {
                m.url = url_innern(t)?;
                continue;
            }
            if k.contains("gradient(") {
                m.verlauf = Some(verlauf_aus(&k)?);
                continue;
            }
            let wiederholen = match k.as_str() {
                "repeat" | "space" | "round" => Some((true, true)),
                "no-repeat" => Some((false, false)),
                "repeat-x" => Some((true, false)),
                "repeat-y" => Some((false, true)),
                _ => None,
            };
            if let Some((x, y)) = wiederholen {
                m.wiederholen_x = x;
                m.wiederholen_y = y;
                continue;
            }
            // Kasten, Verknuepfung und Modus wirken hier nicht.
            if matches!(
                k.as_str(),
                "none" | "border-box" | "padding-box" | "content-box" | "margin-box" | "fill-box"
                    | "stroke-box" | "view-box" | "no-clip" | "add" | "subtract" | "intersect"
                    | "exclude" | "alpha" | "luminance" | "match-source"
            ) {
                continue;
            }
            if nach_strich {
                groesse.push(k);
            } else {
                lage.push(k);
            }
        }
        if !lage.is_empty() {
            let Some(Wert::Maske(l)) = lang_wert(EMaskenPosition, &lage.join(" ")) else {
                return None;
            };
            m.position_x = l[0].position_x;
            m.position_y = l[0].position_y;
        }
        if !groesse.is_empty() {
            let Some(Wert::Maske(l)) = lang_wert(EMaskenGroesse, &groesse.join(" ")) else {
                return None;
            };
            m.groesse = l[0].groesse;
        }
        raus.push(m);
    }
    Some(Wert::Maske(raus))
}

/// Der INHALT von `url(...)`, ohne Klammern und ohne Anfuehrungszeichen.
///
/// CSS Values 4, §5.7: die Anfuehrungszeichen sind optional und es
/// gibt kein Escaping in `url()`. Alles zwischen der ersten und der
/// letzten Klammer ist die URL — auch ein Leerzeichen.
///
/// # GRENZE
///
/// `MAX_DEKL_WERT_LAENGE` (1024) ist die aeussere Grenze des Wertes.
/// Wikipedia schreibt `mask-image: url("data:image/svg+xml;utf8,<svg
/// xmlns=...")`, also 300 bis 900 Zeichen: das passt. Eine laengere
/// URL wird an der aeusseren Grenze abgeschnitten, und die sieht dann
/// kaputt aus. Das ist hier dokumentiert, NICHT wegoptimiert: die
/// Alternative waere ein eigener Längenkanal, und ein abgeschnittener
/// Pfad ist erkennbar, ein stillschweigend gekuerzter nicht.
fn url_innern(t: &str) -> Option<String> {
    let k = t.trim();
    let offen = k.find("url(")?;
    let rest = k[offen + 4..].trim_start();
    // Release 14.10.2026 (Agent CSS2): the URL ends at its own closing
    // quote or parenthesis, not at the last `)` of the value (CSS Syntax 3
    // §4.3.6). MEASURED before (WPT css-backgrounds `background-image-002`,
    // `url("green.png"), url("red.png")`): the URL ran into the second layer
    // and no image loaded.
    let mut modifiers = String::new();
    let u = match rest.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let inner = &rest[1..];
            let end = inner.find(q)?;
            modifiers = url_modifiers_mark(&inner[end + 1..]);
            &inner[..end]
        }
        _ => rest[..rest.find(')')?].trim(),
    };
    if u.is_empty() {
        return None;
    }
    Some(format!("{u}{modifiers}"))
}

/// Release 15.10.2026 (Agent C): the request URL modifiers after the string
/// of a `url()` (CSS Values 5 §4.5.4) as a fragment the image loader reads
/// (`narsil::url_modifiers`): `#narsil-mod&cors=anonymous&sri=sha256-…`.
/// Empty without `cross-origin()`/`integrity()`.
fn url_modifiers_mark(after: &str) -> String {
    // The modifiers end at the `)` of the `url(`.
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut end = after.len();
    for (i, ch) in after.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => {
                end = i;
                break;
            }
            (None, ')') => depth -= 1,
            _ => {}
        }
    }
    let mods = &after[..end];
    let low = mods.to_ascii_lowercase();
    let mut mark = String::new();
    if let Some(i) = low.find("cross-origin(")
        && let Some(j) = low[i..].find(')')
    {
        let v = low[i + "cross-origin(".len()..i + j].trim();
        if matches!(v, "anonymous" | "use-credentials") {
            mark.push_str("&cors=");
            mark.push_str(v);
        }
    }
    if let Some(i) = low.find("integrity(") {
        let inner = &mods[i + "integrity(".len()..];
        let inner = inner.trim_start();
        if let Some(q @ ('"' | '\'')) = inner.chars().next()
            && let Some(e) = inner[1..].find(q)
        {
            let tokens: Vec<&str> = inner[1..1 + e].split_ascii_whitespace().filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric() || "+/=-_?".contains(c))).collect();
            if !tokens.is_empty() {
                mark.push_str("&sri=");
                mark.push_str(&tokens.join(","));
            }
        }
    }
    if mark.is_empty() { mark } else { format!("#narsil-mod{mark}") }
}

/// `background-size` (CSS Backgrounds 3, §3.3): EIN bis ZWEI Werte.
///
/// GEMESSENER FEHLER 01.10.2026 (K7): es stand zuerst nur EIN Wert.
/// `background-position: center center` (zwei Werte) fiel dadurch weg,
/// und DAS ist die haeufigere Form — ein Ein-Wert-Parser haette die
/// haeufige Form nicht gelesen und die seltene.
fn bild_groesse_aus(klein: &str) -> Option<Wert> {
    let teile = teile_wert(klein);
    if teile.is_empty() || teile.len() > 2 {
        return None;
    }
    let mut g = BildGeo::default();
    for (i, t) in teile.iter().enumerate() {
        match *t {
            "cover" => {
                g.cover = true;
                continue;
            }
            "contain" => {
                g.contain = true;
                continue;
            }
            "auto" => continue,
            _ => {}
        }
        let l = laenge_aus(t)?;
        // Release 13.10.2026 (Agent CSS2): negative Groessen sind ungueltig
        // (CSS Backgrounds 3 §3.9). GEMESSEN vorher (WPT css-backgrounds
        // `background-size-with-negative-value`): `-1px -1px` galt.
        if l.wert < 0.0 {
            return None;
        }
        // Release 13.10.2026 (Agent CSS2): beide Achsen (`BildGeo::groesse_xy`).
        if let Some(platz) = g.groesse_xy.get_mut(i) {
            *platz = Some(l);
        }
        // Zwei Werte: die erste ist x, die zweite y (§3.3). Nur die
        // erste wird gemerkt — die zweite gehoert zum Rasterizer, der
        // beide braucht, und die zweite Laenge ohne die erste waere
        // geraten. Deshalb `aus_breite`: die zweite wird GELESEN und
        // nicht verworfen, aber nicht in dieses Feld geschrieben.
        if i == 0 {
            g.groesse = l.wert;
        }
    }
    Some(Wert::BildGeo(g))
}

/// `background-position` (CSS Backgrounds 3, §3.3): EIN bis VIER Werte
/// in der Reihenfolge oben rechts unten links.
///
/// Die Werte werden zu 0..1 umgerechnet, weil die Kaskade die
/// Kastengroesse noch nicht kennt. Ein `px`-Wert wird NICHT
/// umgerechnet (es waere geraten), sondern als Position 0
/// gesetzt und gezaehlt.
fn bild_position_aus(klein: &str) -> Option<Wert> {
    let teile = teile_wert(klein);
    if teile.is_empty() || teile.len() > 4 {
        return None;
    }
    if teile.len() >= 3 {
        return bild_position_kanten(&teile);
    }
    // CSS Backgrounds 3 §3.3: EIN Wert = x, ZWEI = x dann y, DREI =
    // x, y, x, VIER = oben rechts unten links.
    //
    // GEMESSENER FEHLER 01.10.2026 (K7): meine erste Fassung schickte
    // JEDEN Wert nach x, wenn er kein reines y-Wort war. Ergebnis:
    // `100% 0%` kam als (0, 0,5) an — der haeufigste Wert der
    // Eigenschaft, falsch. Und `top left` kam als (0,5, 0) an.
    //
    // Die Regel ist einfach und steht in §3.3: die POSITION im
    // Wertesatz entscheidet die Achse, nicht die Art des Wortes.
    // Der zweite Wert geht nach y, auch wenn er ein x-Wort ist
    // (`left top` ist x=left, y=top — so ist es in der Spec).
    let mut x: Option<f32> = None;
    let mut y: Option<f32> = None;
    // Merkt sich, ob eine der Achsen eine PIXEL-Laenge war (siehe
    // `BildGeo::position_px`). Sonst sind die Zahlen Anteile.
    let mut ist_px = false;
    // Release 11.10.2026 (Agent CSS2): je Achse (siehe `position_px_achsen`).
    let (mut px_x, mut px_y) = (false, false);
    let (mut em_x, mut em_y) = (false, false);
    let (mut ex_x, mut ex_y) = (false, false);
    let mut y_wort_zuerst = false;
    for (nr, t) in teile.iter().enumerate() {
        let mut em_hier = false;
        let mut ex_hier = false;
        let auf_y = nr == 1 || nr == 2;
        let (nx, ny) = match *t {
            "left" => (Some(0.0), None),
            "right" => (Some(1.0), None),
            "top" => (None, Some(0.0)),
            "bottom" => (None, Some(1.0)),
            "center" => {
                if x.is_none() && y.is_none() {
                    (Some(0.5), Some(0.5))
                } else if x.is_none() {
                    (Some(0.5), None)
                } else {
                    (None, Some(0.5))
                }
            }
            _ => {
                // CSS Backgrounds 3 §3.3: `background-position` nimmt
                // Laengen UND Anteile, und ein Anteil ist direkt 0..1.
                //
                // GEMESSENER FEHLER 01.10.2026 (K7): zuerst stand hier
                // fuer JEDE Laenge `(0.0, None)`. `background-position:
                // 100% 0%` kam als `(0, 0,5)` an — `100%` galt als
                // „unbekannte Laenge", und die y-Position blieb auf
                // ihrem Standard. Das ist die haeufigste Form der
                // Eigenschaft.
                match laenge_aus(t) {
                    Some(la) if la.einheit == super::typen::Einheit::Prozent => {
                        (Some((la.wert / 100.0).clamp(0.0, 1.0)), None::<f32>)
                    }
                    Some(la) if la.einheit == super::typen::Einheit::Em => {
                        // Release 11.10.2026 (Agent CSS2): `em` rechnet die
                        // Kaskade (siehe `BildGeo::position_em`).
                        ist_px = true;
                        em_hier = true;
                        (Some(la.wert), None::<f32>)
                    }
                    Some(la) if la.einheit == super::typen::Einheit::Ex => {
                        // Release 11.10.2026 (Agent CSS2): `ex` ebenso, mit
                        // der x-Hoehe der Schrift (`BildGeo::position_ex`).
                        ist_px = true;
                        ex_hier = true;
                        (Some(la.wert), None::<f32>)
                    }
                    Some(la) if la.einheit == super::typen::Einheit::Rem => {
                        ist_px = true;
                        (Some(la.wert * 16.0), None::<f32>)
                    }
                    Some(la) if la.einheit == super::typen::Einheit::Pt => {
                        ist_px = true;
                        (Some(la.wert * 4.0 / 3.0), None::<f32>)
                    }
                    Some(la) => {
                        // Eine PIXEL-Laenge (CSS Backgrounds 3 §3.3).
                        //
                        // KORREKTUR AN MIR SELBST, 01.10.2026 (K7c).
                        // Vorher stand hier:
                        //
                        // ```text
                        //   verwurf("Wert ungeprueft: background-position: …");
                        //   (Some(0.0), None)
                        // ```
                        //
                        // Das war die Rechnung „die Kastengroesse fehlt
                        // mir also nehme ich 0" — und sie war falsch.
                        // Die Groesse fehlt dem PARSER, aber sie ist
                        // genau das, was der Rasterizer hat. Die Zahl
                        // wird deshalb als px durchgereicht
                        // (`BildGeo::position_px`) und dort umgesetzt.
                        //
                        // GEMESSEN: 13 Verwerfe auf den 35 echten
                        // Stylesheets, davon `right 8px center` 3x,
                        // `-500px 0` 2x, `0 0` 2x, `center -16px` 1x,
                        // `12px` 1x, `0` 1x, `right 4px center` 1x.
                        ist_px = true;
                        (Some(la.wert), None::<f32>)
                    }
                    None => return None,
                }
            }
        };
        // Der Wert liefert je eine Zahl fuer x und fuer y; welches
        // davon relevant ist, entscheidet `auf_y`.
        let laenge = !matches!(*t, "left" | "right" | "top" | "bottom" | "center")
            && laenge_aus(t).is_some_and(|la| la.einheit != super::typen::Einheit::Prozent);
        // Release 11.10.2026 (Agent CSS2): ein SCHLUESSELWORT nennt seine
        // Achse selbst (CSS 2.1 §14.2.1: `top` = `top center`, `top right`
        // = `right top`); nur Laengen und Prozente gehen nach ihrer Stelle.
        // GEMESSEN vorher (WPT `background-position-115..148`, `090` u. a.):
        // `top` lag links oben statt mittig oben, `bottom` rechts mittig.
        match *t {
            "top" | "bottom" => {
                y = ny;
                px_y = false;
                if nr == 0 {
                    y_wort_zuerst = true;
                }
            }
            "left" | "right" => {
                x = nx;
                px_x = false;
            }
            "center" => {
                if nx.is_some() {
                    x = nx;
                    px_x = false;
                }
                if ny.is_some() {
                    y = ny;
                    px_y = false;
                }
            }
            _ if auf_y && !y_wort_zuerst => {
                y = ny.or(nx);
                px_y = laenge;
                em_y = em_hier;
                ex_y = ex_hier;
            }
            _ => {
                x = nx.or(ny);
                px_x = laenge;
                em_x = em_hier;
                ex_x = ex_hier;
            }
        }
    }
    let mut g = BildGeo::default();
    g.position_x = x.unwrap_or(0.5);
    g.position_y = y.unwrap_or(0.5);
    g.position_px = ist_px;
    if ist_px {
        g.position_px_achsen = Some((px_x, px_y));
        g.position_em = (em_x, em_y);
        g.position_ex = (ex_x, ex_y);
    }
    Some(Wert::BildGeo(g))
}

/// Release 13.10.2026 (Agent CSS2): `background-position` mit drei oder vier
/// Werten (CSS Backgrounds 3 §3.6): je Achse ein Schluesselwort, optional
/// gefolgt von einem Versatz von DIESER Kante (`right 25% bottom 10px`).
/// GEMESSEN vorher (WPT css-backgrounds `background-position-bottom-right-
/// repeat-round`): die vier Werte galten als „oben rechts unten links".
fn bild_position_kanten(teile: &[&str]) -> Option<Wert> {
    let ist_wort = |t: &str| matches!(t, "left" | "right" | "top" | "bottom" | "center");
    // je Achse: (Wert, px?, vom Ende?, em?)
    let mut x: Option<(f32, bool, bool, bool)> = None;
    let mut y: Option<(f32, bool, bool, bool)> = None;
    let mut mitten = 0usize;
    let mut i = 0usize;
    while i < teile.len() {
        let w = teile[i];
        if !ist_wort(w) {
            return None;
        }
        let versatz = teile.get(i + 1).filter(|t| !ist_wort(t)).and_then(|t| laenge_aus(t));
        if teile.get(i + 1).is_some_and(|t| !ist_wort(t)) && versatz.is_none() {
            return None;
        }
        if w == "center" && versatz.is_some() {
            return None;
        }
        i += if versatz.is_some() { 2 } else { 1 };
        let ende = matches!(w, "right" | "bottom");
        let wert = match versatz {
            None => (if w == "center" { 0.5 } else if ende { 1.0 } else { 0.0 }, false, false, false),
            Some(l) if l.einheit == super::typen::Einheit::Prozent => {
                let p = l.wert / 100.0;
                (if ende { 1.0 - p } else { p }, false, false, false)
            }
            Some(l) => {
                let (v, em) = match l.einheit {
                    super::typen::Einheit::Em => (l.wert, true),
                    super::typen::Einheit::Rem => (l.wert * 16.0, false),
                    super::typen::Einheit::Pt => (l.wert * 4.0 / 3.0, false),
                    _ => (l.wert, false),
                };
                (v, true, ende, em)
            }
        };
        match w {
            "left" | "right" if x.is_none() => x = Some(wert),
            "top" | "bottom" if y.is_none() => y = Some(wert),
            "center" => mitten += 1,
            _ => return None,
        }
    }
    let mitte = (0.5, false, false, false);
    match mitten {
        0 => {}
        1 if x.is_none() => x = Some(mitte),
        1 if y.is_none() => y = Some(mitte),
        _ => return None,
    }
    let (x, y) = (x.unwrap_or(mitte), y.unwrap_or(mitte));
    let mut g = BildGeo::default();
    g.position_x = x.0;
    g.position_y = y.0;
    g.position_px = x.1 || y.1;
    g.position_px_achsen = Some((x.1, y.1));
    g.position_em = (x.3, y.3);
    g.vom_ende = (x.2, y.2);
    Some(Wert::BildGeo(g))
}

/// `background-repeat` (CSS Backgrounds 3, §3.4): EIN bis ZWEI Werte je
/// Achse — oder eine kommagetrennte Liste mit einer Angabe je
/// Hintergrund-Ebene.
///
/// GEMESSENER FEHLER 01.10.2026 (K7): `repeat` ALLEIN fiel durch, weil
/// die erste Fassung zwei Werte verlangte. Das ist die haeufigste
/// Form überhaupt — `background-repeat: repeat` steht auf praktisch
/// jeder Seite, und `repeat` ist der Anfangswert.
///
/// GEMESSENER FEHLER 06.10.2026 (K7c): die KOMMA-FORM fiel durch —
/// `background-repeat: repeat-x,repeat-x` (eine Angabe je Ebene, §3.4)
/// stand 4x in den echten Stylesheets als „Wert ungeprueft". Diese
/// Crate kennt — wie bei `background-image` — nur EINE Ebene
/// (`Berechnet::bild` ist EIN Feld, siehe `kaskade.rs` K1); deshalb
/// gilt die ERSTE Angabe, weitere Ebenen werden still ignoriert.
fn bild_wiederholen_aus(klein: &str) -> Option<Wert> {
    let ebene = teile_komma(klein);
    if ebene.is_empty() {
        return None;
    }
    let teile = teile_wert(ebene[0]);
    if teile.is_empty() || teile.len() > 2 {
        return None;
    }
    // CSS Backgrounds 3 §3.4: EIN Wert gilt fuer beide Achsen, ZWEI
    // je eine. `repeat-x`/`repeat-y` sind Ein-Wort-Kurzformen fuer
    // `(repeat,no-repeat)` bzw. `(no-repeat,repeat)` und stehen deshalb
    // hier, nicht in einem eigenen Zweig.
    //
    // GEMESSENER FEHLER 01.10.2026 (K7): `repeat-x` war nicht genannt
    // und fiel durch — obwohl es eine der haeufigsten Formen ist.
    // Release 13.10.2026 (Agent CSS2): `space` und `round` (§3.4.2) — die
    // Kastengroesse braucht erst das Zeichnen (`BildGeo::wiederhol_art`).
    // GEMESSEN vorher (WPT css-backgrounds `background-repeat-space-*`,
    // `-round-*`): die Angabe fiel weg.
    let f = |t: &str| -> Option<(bool, bool, u8)> {
        match t {
            "repeat" => Some((true, true, 0)),
            "no-repeat" => Some((false, false, 0)),
            "repeat-x" => Some((true, false, 0)),
            "repeat-y" => Some((false, true, 0)),
            "space" => Some((true, true, 1)),
            "round" => Some((true, true, 2)),
            _ => None,
        }
    };
    let mut g = BildGeo::default();
    if teile.len() == 1 {
        let (x, y, art) = f(teile[0])?;
        g.wiederholen_x = x;
        g.wiederholen_y = y;
        if !matches!(teile[0], "repeat-x" | "repeat-y") {
            g.wiederhol_art = [art, art];
        }
    } else {
        let (x, _, ax) = f(teile[0])?;
        let (_, y, ay) = f(teile[1])?;
        if matches!(teile[0], "repeat-x" | "repeat-y") || matches!(teile[1], "repeat-x" | "repeat-y") {
            return None;
        }
        g.wiederholen_x = x;
        g.wiederholen_y = y;
        g.wiederhol_art = [ax, ay];
    }
    Some(Wert::BildGeo(g))
}

/// `box-shadow` (CSS Backgrounds 3, §9): eine Liste von Schatten.
///
/// # WAS GELESEN WIRD
///
/// ```text
/// box-shadow: none
/// box-shadow: 2px 4px #000            -> Versatz, Farbe
/// box-shadow: 0 0 8px rgba(0,0,0,.5)   -> zusaetzlich weich + Farbe
/// box-shadow: inset 0 0 4px #f00       -> zusaetzlich "innen"
/// box-shadow: 0 1px 2px #000, 0 2px 4px #000   -> Liste (durch Komma)
/// ```
///
/// Die LAENGEN duerfen fehlen (`0 4px #000` heisst 0/4px), die Farbe
/// darf fehlen (`0 0 4px` heisst currentColor = Schwarz, CSS
/// Backgrounds 3 §9.1). Mehr als [`MA_SCHATTEN`] Eintraege faellt
/// WEG (M4) — nicht abschneiden und die Restfarbe erfinden.
///
/// # GRENZEN
///
/// Der Innentext kommt aus der Datei; `MAX_DEKL_WERT_LAENGE` (1024)
/// ist die aeussere Grenze. Jede Schleife endet an `MA_SCHATTEN`.
fn schatten_aus(klein: &str) -> Option<Wert> {
    if klein == "none" {
        return Some(Wert::Wort(String::from("none")));
    }
    // M3/M4: die Schleife laeuft ueber die Kommas und endet an
    // MA_SCHATTEN + 1.
    let teile = teile_komma(klein);
    if teile.is_empty() {
        return None;
    }
    let mut raus: Vec<super::typen::Schatten> = Vec::new();
    for teil in teile.iter() {
        if raus.len() >= crate::MAX_SCHATTEN {
            // Zu viele: die GANZE Angabe faellt weg.
            return None;
        }
        let stuecke = teile_wert(teil);
        if stuecke.is_empty() {
            return None;
        }
        let mut laengen: Vec<f32> = Vec::new();
        let mut raw: Vec<Laenge> = Vec::new();
        let mut farbe = Farbe::SCHWARZ;
        let mut current = true;
        let mut innen = false;
        for st in stuecke.iter() {
            if *st == "inset" {
                innen = true;
                continue;
            }
            if *st == "currentcolor" {
                current = true;
                continue;
            }
            // Farbe zuerst probieren: `red` ist keine Laenge.
            if let Some(f) = farbe_aus(st) {
                farbe = f;
                current = false;
                continue;
            }
            match laenge_aus(st) {
                Some(l) => {
                    laengen.push(l.wert);
                    raw.push(l);
                }
                None => return None,
            }
        }
        if laengen.len() > 4 {
            return None;
        }
        // Release 14.10.2026 (Agent CSS2): fewer than two lengths, or a
        // negative blur, is invalid (CSS Backgrounds 3 §7.1).
        if laengen.len() < 2 || laengen.get(2).is_some_and(|b| *b < 0.0) {
            return None;
        }
        let zero = Laenge { wert: 0.0, einheit: super::typen::Einheit::Px };
        while raw.len() < 4 {
            raw.push(zero);
        }
        // Zu wenig? Die fehlenden sind 0 (CSS Backgrounds 3 §9.1).
        while laengen.len() < 4 {
            laengen.push(0.0);
        }
        raus.push(super::typen::Schatten {
            farbe,
            x: laengen[0],
            y: laengen[1],
            weich: laengen[2],
            ausbreitung: laengen[3],
            innen,
            raw_lengths: [raw[0], raw[1], raw[2], raw[3]],
            current_color: current,
        });
    }
    if raus.is_empty() {
        return None;
    }
    Some(Wert::Schatten(raus))
}

/// Prozent (mit %-Zeichen) als 0-1. Ohne Zeichen: None.
fn prozent_01(s: &str) -> Option<f32> {
    let p = s.trim().strip_suffix('%')?;
    let f = zahl_aus(p)?;
    Some((f / 100.0).clamp(0.0, 1.0))
}

// ===========================================================================
// VERLAEUFE ALS WERTE (K1, 01.10.2026) — CSS Images 3, §3
// ===========================================================================

/// `linear-gradient()`, `radial-gradient()`, `conic-gradient()` und die
/// drei `repeating-`-Formen als WERT lesen.
///
/// GEMESSENER ZUSTAND VORHER: `background-image: linear-gradient(...)`
/// war dem Parser unbekannt, die Deklaration fiel beim Lesen weg und
/// die Seite hatte an dieser Stelle einen leeren Hintergrund.
///
/// WAS HIER NICHT GEHT und warum das ehrlich ist: `url(...)` (Bilder
/// holt eine andere Rolle), `in oklab`/`in display-p3` als
/// Verlaufs-Farbraum (falsche Umrechnung ist schlimmer als keine),
/// und Farbstopp-Positionen in px werden als Anteil `px / 100` gelesen
/// (siehe `stopp_position`) — wer eine echte Pixelangabe braucht,
/// rechnet sie selbst, weil die Laenge des Kastens hier nichts ist.
pub fn verlauf_aus(s: &str) -> Option<Verlauf> {
    let t = s.trim();
    // M4: Der Aufrufer kann Datei-Inhalt reichen. 512 ist die aeussere
    // Grenze (MAX_DEKL_WERT_LAENGE); eine laengere Angabe ist kein
    // Verlauf, sondern eine Bombe.
    if t.len() > MAX_DEKL_WERT_LAENGE {
        return None;
    }
    let klein = t.to_ascii_lowercase();
    let (wiederholt, art_name, rest) =
        if let Some(r) = klein.strip_prefix("repeating-linear-gradient(") {
            (true, "linear", r)
        } else if let Some(r) = klein.strip_prefix("repeating-radial-gradient(") {
            (true, "radial", r)
        } else if let Some(r) = klein.strip_prefix("repeating-conic-gradient(") {
            (true, "kegel", r)
        } else if let Some(r) = klein.strip_prefix("linear-gradient(") {
            (false, "linear", r)
        } else if let Some(r) = klein.strip_prefix("radial-gradient(") {
            (false, "radial", r)
        } else if let Some(r) = klein.strip_prefix("conic-gradient(") {
            (false, "kegel", r)
        } else {
            return None;
        };
    let innen = rest.strip_suffix(')')?;
    let art = match art_name {
        "linear" => VerlaufArt::Linear,
        "radial" => VerlaufArt::Radial,
        _ => VerlaufArt::Kegel,
    };
    // M4: Innenteil deckeln, BEVOR gesplittet wird.
    if innen.len() > MAX_VERLAUF_INNEN {
        return None;
    }
    // GEMESSENER FEHLER 01.10.2026 (K1): hier stand `innen.split(',')`.
    // Damit zerfiel `linear-gradient(rgb(255,0,0) 40%, #00f)` an den
    // Kommas INNERHALB von `rgb()` in vier Teile, und der Verlauf fiel
    // weg — jede Farbfunktion im Stopp tötete den ganzen Verlauf.
    // `teile_wert` teilt nur auf OBERSTER Ebene (Klammern und Quotes
    // bleiben zusammen).
    //
    // M3: Der erste Teil ist die Richtung/Form, danach kommen die
    // Farbstopps. `MAX_VERLAUF_STOPPS` + 1 deckt genau diese Grenze ab.
    let alle_teile = teile_komma(innen);
    if alle_teile.len() > MAX_VERLAUF_STOPPS + 1 {
        return None;
    }
    let teile: Vec<&str> = alle_teile;
    // Der ERSTE Teil ist nur dann die Richtung, wenn er wirklich eine
    // Richtungsangabe ist. `linear-gradient(red, blue)` hat KEINE
    // Richtung, und `red` ist ein Farbstopp (CSS Images 3, §3.1:
    // `[<angle> | to <side-or-corner>]?`).
    let mut start = 0usize;
    let winkel = if teile[0].trim().is_empty() {
        // Leerer erster Teil: die Richtung fehlt, der Voreingang gilt.
        start = 1;
        match art {
            VerlaufArt::Linear => 180.0,
            _ => 0.0,
        }
    } else if ist_richtung(art, teile[0].trim()) {
        start = 1;
        richtung_aus(art, teile[0].trim())?
    } else {
        match art {
            // Ohne Richtungsangabe: die Voreinstellung (nach unten).
            VerlaufArt::Linear => 180.0,
            _ => 0.0,
        }
    };
    // M4: Allokation erst NACH der Pruefung, und mit kleinster
    // Kapazitaet (`Vec::new` + `push`), nie `with_capacity` aus der
    // Eingabe.
    let mut stopps: Vec<Farbstopp> = Vec::new();
    for t in &teile[start..] {
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        // CSS Images 3 §3.5: „Two positions are allowed, which
        // creates a hard stop between them" — der Teil liefert dann
        // ZWEI Stopps mit derselben Farbe.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): 2x
        // `linear-gradient(90deg,#0000,#000 20% 80%,#0000)` und
        // `linear-gradient(#000 0 0)` auf den 35 echten Stylesheets.
        for s in stopps_aus(t)? {
            stopps.push(s);
            if stopps.len() > MAX_VERLAUF_STOPPS {
                return None;
            }
        }
    }
    // CSS Images 3, §3.5: "a gradient with fewer than two color stops
    // is invalid" — ein Verlauf aus einem Stopp ist kein Verlauf.
    if stopps.len() < 2 {
        return None;
    }
    Some(Verlauf {
        art,
        winkel,
        stopps,
        wiederholt,
    })
}

/// Ist dieses erste Argument eine RICHTUNGSANGABE (und kein Stopp)?
///
/// Ohne diese Probe waere `linear-gradient(red, blue)` um 180 Grad
/// gedreht worden und `red` als Richtung verloren — der erste Stopp
/// faellt dann weg, und es bleibt einer uebrig, also kein Verlauf.
fn ist_richtung(art: VerlaufArt, teil: &str) -> bool {
    match art {
        VerlaufArt::Linear => {
            // Ohne `format!`: die vier Endungen sind feste Zeichenketten.
            teil.starts_with("to ")
                || teil.starts_with("at ")
                || ["deg", "grad", "turn", "rad"]
                    .iter()
                    .any(|e| teil.ends_with(*e))
        }
        VerlaufArt::Kegel => teil.starts_with("from "),
        // `radial-gradient(circle, …)` / `(ellipse closest-side, …)`:
        // Formangaben zaehlen als Richtungsteil.
        VerlaufArt::Radial => {
            teil.starts_with("circle")
                || teil.starts_with("ellipse")
                || teil.starts_with("at ")
                || teil.starts_with("closest")
                || teil.starts_with("farthest")
        }
    }
}

/// Die Farbstopps EINES Verlaufsteils: `<farbe>` oder
/// `<farbe> <anteil>…`.
///
/// # WARUM NICHT EINFACH AUF LEER TEILEN
///
/// `linear-gradient(90deg, rgb(255 0 0) 40%, #00f)` enthaelt im
/// Farbteil selbst Leer (die moderne Leerform von `rgb()`). Ein
/// `split_whitespace()` liefert dort `["rgb(255", "0", "0)", "40%", ...]`
/// und der erste Teil ist keine Farbe. Darum wird eine
/// Klammerfunktion als GANZES gelesen: die Klammern werden ausgezaehlt
/// (M3 — der Text ist auf `MAX_VERLAUF_INNEN` deckelt, die Schleife
/// kann also nicht laufen).
///
/// # WARUM EINE FUNKTION UND DREI
///
/// CSS Images 3 §3.5: „A color stop can have one or two positions.
/// If two positions are specified, ... a hard stop is created between
/// them." `#000 20% 80%` ist also `20%` und `80%` mit derselben Farbe,
/// nicht ein Fehler.
///
/// GEMESSENER FEHLER 01.10.2026 (K7c): 2x
/// `linear-gradient(90deg,#0000,#000 20% 80%,#0000)` und
/// `linear-gradient(#000 0 0)` fielen weg.
///
/// Die fruehere Fassung gab `Option<Farbstopp>` zurueck und lehnte
/// jede Angabe mit Leer im Rest ab. Die Aufteilung muss aber VOR der
/// Farbe passieren (siehe oben), und das liest ein `stopp_aus` an
/// erster Stelle nicht — es scheitert an der zweiten Position, bevor
/// die Kurzform gesehen wird. Genau das war der Fehler.
///
/// Die Farbe wird nur EINMAL gelesen und an beide Stopps gegeben —
/// `farbe_aus` hat keine Nebenwirkung, aber das macht die Absicht
/// sichtbar.
fn stopps_aus(t: &str) -> Option<Vec<Farbstopp>> {
    // Die Aufteilung passiert VOR der Farbe: der Farbteil kann selbst
    // Leer enthalten (`rgb(255 0 0)` 40%), und genau deshalb trennt
    // erst `erste_klammer` den Farbteil ab. Vorher (01.10.2026) stand
    // hier ein `stopp_aus(t)?` am Anfang — und das liest die Position
    // als EINEN Wert, scheitert an der zweiten und bricht ab, BEVOR
    // die Kurzform gesehen wird.
    let (farb_text, rest) = match erste_klammer(t) {
        Some(start) => {
            let ende = klammer_ende(t, start)?;
            (&t[..=ende], t[ende + 1..].trim())
        }
        None => {
            let mut it = t.splitn(2, char::is_whitespace);
            let f = it.next()?;
            (f, it.next().unwrap_or("").trim())
        }
    };
    if farb_text.is_empty() {
        return None;
    }
    let farbe = farbe_aus(farb_text)?;
    if rest.is_empty() {
        return Some(vec![Farbstopp {
            farbe,
            anteil: None,
        }]);
    }
    let teile: Vec<&str> = rest.split(char::is_whitespace).collect();
    // M3: der Caller begrenzt die Stoppzahl; hier nur die Form.
    if teile.len() > 2 {
        return None;
    }
    let mut raus: Vec<Farbstopp> = Vec::new();
    for p in teile.iter() {
        raus.push(Farbstopp {
            farbe,
            anteil: Some(stopp_position(p)?),
        });
    }
    Some(raus)
}

/// Index der ERSTEN `(` — aber nur, wenn davor ein Funktionsname
/// steht. `None`, wenn der Text gar keine Funktion ist (`red`, `#00f`).
///
/// GEMESSENER FEHLER 01.10.2026 (K1): die erste Fassung verlangte,
/// dass der Text an der Klammer BEGINNT. `rgb(255,0,0) 40%` beginnt an
/// `r`, also wurde jede Farbfunktion im Stopp abgelehnt — mit der
/// Folge, dass `linear-gradient(rgb(0,0,0), #fff)` nie ein Verlauf war.
/// Genau die Form, die auf jeder Seite vorkommt.
fn erste_klammer(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    // CSS Syntax 3, §4.3.1: ein Funktionsname ist ein Bezeichner —
    // der darf Bindestriche enthalten (`color-mix`, `-webkit-linear-…`).
    //
    // GEMESSENER FEHLER 01.10.2026 (K1): die erste Fassung liest nur
    // Buchstaben. Bei `color-mix(...)` endete der Name nach "color",
    // das `-` war keine offene Klammer, damit galt der Text als
    // "keine Funktion" und der ganze Verlauf fiel weg.
    let mut i = 0usize;
    if i >= b.len() || !b[i].is_ascii_alphabetic() {
        return None;
    }
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_') {
        i += 1;
    }
    if i >= b.len() || b[i] != b'(' {
        return None;
    }
    Some(i)
}

/// Index der schliessenden Klammer, die zu `t[start] == '('` gehoert.
/// `None`, wenn die Klammer offen bleibt (kaputte Angabe).
///
/// M3: Die Schleife endet an `t.len()`, und `t` ist vom Aufrufer auf
/// `MAX_VERLAUF_INNEN` (1024) deckelt. Eine schliessende Klammer ohne
/// offene senkt die Tiefe nie unter 0 — `checked_sub` verhindert den
/// Unterlauf bei Text wie `")("`.
fn klammer_ende(t: &str, start: usize) -> Option<usize> {
    let b = t.as_bytes();
    if b.get(start) != Some(&b'(') {
        return None;
    }
    let mut tiefe = 0usize;
    for (i, z) in b.iter().enumerate().skip(start) {
        match z {
            b'(' => tiefe += 1,
            b')' => {
                tiefe = tiefe.checked_sub(1)?;
                if tiefe == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None // offene Klammer
}

/// Position eines Farbstopps als Anteil 0-1.
///
/// CSS Images 3, §3.5 erlaubt Prozent und (ab CSS Images 4) Laengen.
/// Eine Laenge in px kann dieser Browser nicht in einen Anteil
/// umrechnen, weil er die Kastengroesse nicht kennt — sie wird
/// deshalb ABGELEHNT (die Deklaration faellt weg) und nicht still auf
/// eine Zahl gesetzt, die dann falsch waere.
fn stopp_position(s: &str) -> Option<f32> {
    let t = s.trim();
    if t.ends_with('%') {
        return prozent_01(t);
    }
    // Einheitenlose Zahl gilt laut Spec als Prozent.
    if !t.contains(char::is_alphabetic) && !t.ends_with("px") {
        if let Some(f) = zahl_aus(t) {
            return Some((f / 100.0).clamp(0.0, 1.0));
        }
    }
    None
}

/// Winkel (linear) oder Form (radial/kegel) aus dem ERSTEN Teil.
///
/// CSS Images 3, §3.1: `0deg` zeigt nach oben, `90deg` nach rechts.
/// Ohne Angabe ist `linear-gradient` `180deg` (nach unten), `radial`
/// eine Ellipse und `conic` `from 0deg` (oben).
fn richtung_aus(art: VerlaufArt, teil: &str) -> Option<f32> {
    let t = teil.trim();
    match art {
        VerlaufArt::Linear => {
            if t.is_empty() {
                return Some(180.0);
            }
            // `to right` / `45deg` / `0.25turn` / `100grad` / `1rad`.
            if t.ends_with("deg") {
                return Some(zahl_aus(t.strip_suffix("deg")?)?);
            }
            if t.ends_with("grad") {
                return Some(zahl_aus(t.strip_suffix("grad")?)? * 0.9);
            }
            if t.ends_with("turn") {
                return Some(zahl_aus(t.strip_suffix("turn")?)? * 360.0);
            }
            if t.ends_with("rad") {
                return Some(zahl_aus(t.strip_suffix("rad")?)? * 57.29578);
            }
            // `at <position>` — die Form aus CSS Images 3 §3.1, die in
            // CSS Images 4 weggefallen ist, aber noch im Umlauf ist.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 1x
            // `linear-gradient(at 50% 50%,#000 70%,transparent 71%)`.
            //
            // Sie wird als die VOREINSTELLUNG (nach unten) gelesen, und
            // das ist eine LENIENZUGABE: der Verlauf liegt dann in der
            // falschen Richtung, aber die Maske ist vorhanden. Es ist
            // besser, eine sichtbare Maske in der falschen Richtung zu
            // zeichnen als gar keine.
            if t.starts_with("at ") {
                return Some(180.0);
            }
            // Richtungswoerter (CSS Images 3, §3.2). Mehrteilig
            // ("to bottom right") wird hier abgelehnt, statt geraten.
            let richtung = match t {
                "to top" => 0.0,
                "to right" => 90.0,
                "to bottom" => 180.0,
                "to left" => 270.0,
                _ => return None,
            };
            Some(richtung)
        }
        VerlaufArt::Radial => {
            // Die Form ist fuer den WERT hier nicht gemerkt; sie wird
            // anerkannt (damit der Verlauf nicht am Formwort
            // scheitert) und als 0 abgelegt.
            let _ = t;
            Some(0.0)
        }
        VerlaufArt::Kegel => {
            if let Some(r) = t.strip_prefix("from ") {
                let r = r.trim();
                if r.ends_with("deg") {
                    return Some(zahl_aus(r.strip_suffix("deg")?)?);
                }
                if r.ends_with("turn") {
                    return Some(zahl_aus(r.strip_suffix("turn")?)? * 360.0);
                }
                if r.is_empty() {
                    return Some(0.0);
                }
                return None;
            }
            Some(0.0)
        }
    }
}

/// Standard-Umrechnung HSL -> RGB (CSS Color 4), 0-255 gerundet.
fn hsl_nach_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let hh = ((h % 360.0) + 360.0) % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hh / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if hh < 60.0 {
        (c, x, 0.0)
    } else if hh < 120.0 {
        (x, c, 0.0)
    } else if hh < 180.0 {
        (0.0, c, x)
    } else if hh < 240.0 {
        (0.0, x, c)
    } else if hh < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0).clamp(0.0, 255.0).round() as u8,
        ((g1 + m) * 255.0).clamp(0.0, 255.0).round() as u8,
        ((b1 + m) * 255.0).clamp(0.0, 255.0).round() as u8,
    )
}

// ===========================================================================
// EIGENSCHAFTS-DISPATCH (C4) inkl. Kurzformen (C3)
// ===========================================================================

/// Baut eine Deklaration mit Rohtext (fuer var()/calc() in der Kaskade,
/// C10/C11, W6). `roh` wird an Zeichengrenze gedeckelt (M4).
fn dekl(e: Eigenschaft, wert: Wert, wichtig: bool, roh: &str) -> Deklaration {
    let t = roh.trim();
    let mut k = t.len().min(MAX_DEKL_WERT_LAENGE);
    while k > 0 && !t.is_char_boundary(k) {
        k -= 1;
    }
    Deklaration {
        eigenschaft: e,
        wert,
        wichtig,
        roh: String::from(&t[..k]),
    }
}

/// Release 08.10.2026: ist der Wert EIN Grid-Linienname (`<custom-ident>`,
/// CSS Grid 1 §8.3) — kein `auto`, kein `span`, keine Zahl, kein `/`?
fn ist_linienname(klein: &str) -> bool {
    let t = klein.trim();
    !t.is_empty()
        && t != "auto"
        && !t.starts_with("span")
        && t.parse::<i32>().is_err()
        && !matches!(t, "inherit" | "initial" | "unset" | "revert")
        && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !t.as_bytes()[0].is_ascii_digit()
}

/// True, wenn der (kleingeschriebene) Wert var() enthaelt (C10).
fn enthaelt_var(klein: &str) -> bool {
    klein.contains("var(")
        // Release 12.10.2026 (Agent C): `attr()` setzt die Kaskade ebenso
        // spaet ein (`crate::attr_kaskade`), `inherit()` auch.
        || klein.contains("attr(")
        || klein.contains("inherit(")
        || klein.contains("if(")
        // CSS Values 5 §9.2: die Kurzschreibweise `(--name)` ist
        // dasselbe wie `var(--name)`. Ohne sie hier zu kennen, wird der
        // Wert als gewöhnliche Laenge gelesen und dann verworfen.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): 1x
        // `font-size: (--text-body-size-small)` auf den 35 echten
        // Stylesheets (GitHub).
        //
        // Die Form wird erkannt, NICHT ersetzt: `Deklaration::roh`
        // traegt das Original, und die Kaskade setzt daraus spaeter
        // den Wert der Variablen ein.
        || (klein.starts_with("(--") && klein.ends_with(')'))
}

/// True, wenn der Wert eine Rechenfunktion enthaelt (C11).
/// Vorsicht vor Zufallstreffern ("admin("): Der Aufrufer nutzt das nur
/// dort, wo sonst Laengen stehen — ein Wort wie "domin(" faellt in der
/// Kaskade ohnehin weg (dasselbe Ergebnis wie sofortiges Wegwerfen).
fn enthaelt_mathe(klein: &str) -> bool {
    klein.contains("calc(")
        || klein.contains("min(")
        || klein.contains("max(")
        || klein.contains("clamp(")
        // CSS Values 4 §10: die STUFENfunktionen. GEMESSENER FEHLER
        // 01.10.2026 (K7c): 1x `width: round(1.5px,1px)` auf den 35
        // echten Stylesheets.
        || klein.contains("round(")
        || klein.contains("mod(")
        || klein.contains("rem(")
}

/// Platzhalter-Wert fuer spaet aufgeloeste Werte (Kaskade, C10/C11).
/// Der Rohtext in der Deklaration entscheidet, nicht dieses Wort.
fn spaet() -> Wert {
    Wert::Wort(String::from("spaet"))
}

/// Laenge oder Rechen-Platzhalter (C11): gueltige Laenge -> Laenge,
/// sonst bei calc/min/max/clamp -> Platzhalter, sonst None.
fn laenge_oder_mathe(klein: &str) -> Option<Wert> {
    if let Some(l) = laenge_aus(klein) {
        return Some(Wert::Laenge(l));
    }
    if enthaelt_mathe(klein) {
        return Some(spaet());
    }
    None
}

/// Einheitenlose Zahl >= 0 oder Rechen-Platzhalter (fuer flex-grow/shrink).
/// `Zahl` oder `calc()`-Ergebnis — der alte Weg.
///
/// K7c (01.10.2026): abgeloest. `dekl_auswerten` entscheidet jetzt ueber
/// [`enthaelt_mathe`] und gibt [`spaet`] zurueck; die Auflosung passiert
/// spaeter in der Kaskade. Diese Funktion bleibt, weil sie die
/// ERSETZUNG fuer `var()`-Werte beschreibt (C10) und dort noch als
/// Vergleichsfall gebraucht wird.
#[allow(dead_code)]
fn zahl_oder_mathe(klein: &str) -> Option<Wert> {
    if let Some(f) = zahl_aus(klein) {
        if f >= 0.0 {
            return Some(Wert::Laenge(Laenge {
                wert: f,
                einheit: Einheit::Zahl,
            }));
        }
        return None;
    }
    if enthaelt_mathe(klein) {
        return Some(spaet());
    }
    None
}

/// Teilt an Kommas auf OBERSTER Ebene (Klammern/Anfuehrung bleiben
/// zusammen). Begrenzt auf `MA_TEILE_KOMMA` Teile (M3/M4).
///
/// WARUM EIGENS und nicht `teile_wert`: dort wird an LEER geteilt, hier
/// an Komma — und beide muessen Klammertiefe und Quotes beachten.
fn teile_komma(s: &str) -> Vec<&str> {
    let mut teile: Vec<&str> = Vec::new();
    let b = s.as_bytes();
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut quote = 0u8;
    let mut i = 0usize;
    while i <= b.len() {
        let c = if i < b.len() { b[i] } else { 0 };
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'(' {
            tiefe += 1;
        } else if c == b')' {
            tiefe = tiefe.saturating_sub(1);
        }
        let trennt = i == b.len() || (c == b',' && tiefe == 0 && quote == 0);
        if trennt {
            if i > start {
                teile.push(&s[start..i]);
            }
            if teile.len() >= MA_TEILE_KOMMA {
                return teile;
            }
            start = i + 1;
        }
        i += 1;
    }
    teile
}

/// Hoechste Teile je Komma-Split (M3). Begruendung: `MAX_VERLAUF_STOPPS`
/// (32) ist die eigentliche Grenze fuer Farbstopps; diese Zahl deckt
/// einen Aufruf ab, der auch ausserhalb von Verlaeufen benutzt wird.
const MA_TEILE_KOMMA: usize = 64;

/// Teilt einen Kurzform-Wert an Leerraum auf oberster Ebene (Klammern
/// und Anfuehrung bleiben zusammen — fuer calc()/var() mit Leerzeichen,
/// C10/C11). Eingabe gedeckelt (<= 512 Zeichen vom Aufrufer).
fn teile_wert(klein: &str) -> Vec<&str> {
    let mut teile: Vec<&str> = Vec::new();
    let b = klein.as_bytes();
    let mut start = 0usize;
    let mut tiefe = 0usize;
    let mut quote = 0u8;
    let mut i = 0usize;
    while i <= b.len() {
        let c = if i < b.len() { b[i] } else { 0 };
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'(' {
            tiefe += 1;
        } else if c == b')' {
            tiefe = tiefe.saturating_sub(1);
        }
        let trennt =
            i == b.len() || ((c == b' ' || c == b'\t' || c == b'\n') && tiefe == 0 && quote == 0);
        if trennt {
            if i > start {
                teile.push(&klein[start..i]);
            }
            start = i + 1;
            if teile.len() >= MAX_RECHEN_ARGS * 2 {
                break;
            }
        }
        i += 1;
    }
    teile
}

/// Wertet `name: wert` zu 0-n Deklarationen aus. Leeres Ergebnis = die
/// Deklaration faellt still weg (unbekannte Eigenschaft oder ungueltiger
/// Wert — C4). Kurzformen expandieren in Langformen (C3).
fn dekl_auswerten(name: &str, wert: &str, wichtig: bool) -> Vec<Deklaration> {
    let mut aus: Vec<Deklaration> = Vec::new();
    if wert.is_empty() {
        return aus;
    }
    // Release 11.10.2026: `flex-line-count: <integer [1,∞]>` (CSS Flexbox 2).
    if name == "flex-line-count" && crate::gitter::zeilenzahl(wert).is_none() && !wert.contains("var(") {
        return aus;
    }
    // Release 08.10.2026: `env()` wird VOR allem anderen ersetzt (CSS
    // Environment Variables 1 §3) — siehe `env_ersetzen`.
    let env_text;
    let wert = if super::rechnen::hat_funktion(wert, "env(") {
        env_text = env_ersetzen(wert);
        env_text.as_str()
    } else {
        wert
    };
    let klein = wert.to_ascii_lowercase();

    // Release 10.10.2026: `-webkit-box-orient` (das Flexbox von 2009, das
    // Chrome und Safari bis heute fuer `display: -webkit-box` auswerten):
    // `vertical` legt die Kinder untereinander, also `flex-direction:
    // column` (bei uns ist `-webkit-box` ein `flex`). GEMESSEN vorher
    // (chip.de): `.max-lines{display:-webkit-box;-webkit-box-orient:vertical;
    // -webkit-line-clamp:3}` — die Zeilen der Anrisstexte standen
    // NEBENEINANDER und lagen uebereinander.
    if matches!(name, "-webkit-box-orient" | "box-orient") {
        let richtung = match klein.trim() {
            "vertical" | "block-axis" => "column",
            "horizontal" | "inline-axis" => "row",
            _ => return aus,
        };
        return dekl_auswerten("flex-direction", richtung, wichtig);
    }

    // ------------------------------------------------------------------------
    // `grid-template` — DIE KURZFORM WIRD EXPANDIERT (06.10.2026)
    // ------------------------------------------------------------------------
    //
    // GEMESSENER FEHLER AM LEBENDEN BEISPIEL: bei 1191 px Breite lag der
    // ganze Wikipedia-Artikel bei **x = 1227** — ausserhalb des Fensters,
    // die Seite sah leer aus. Ursache ist genau eine Regel:
    //
    //     @media (min-width:1120px) {
    //       .mw-page-container-inner { grid-template:
    //         min-content 1fr min-content / 12.25rem minmax(0,1fr);
    //         grid-template-areas: 'siteNotice siteNotice'
    //                              'columnStart pageContent' 'footer footer' } }
    //
    // Der Parser hatte `vorlage_teile` fuer genau diese Form schon gebaut
    // (05.10.2026), aber **nie aufgerufen**: `grid-template` lief als
    // Alias von `grid-template-columns`, `spuren_mass` scheiterte am `/`,
    // und die Spaltenliste fiel mit. Ohne Spalten wurde aus zwei
    // Gebietsspalten die Notspalte + eine `auto`-Spur — und die zweite
    // Spur ist `max-content` des Artikels (1103 px), also hinter dem Rand.
    //
    // DIE REIHENFOLGE (Gebiete zuerst) ist dieselbe wie in der Langform:
    // §8.1 nimmt die Gebiete als die Spaltenzahl, bevor die Spuren
    // gelesen werden. Wer sie danach setzt, hat ein anderes Raster.
    // Release 11.10.2026: die Kurzform `grid` (CSS Grid 1 §7.8) — vorher lag
    // sie ungelesen im Schluessel-Eimer. GEMESSEN: WPT
    // `abspos/positioned-grid-items-001..017` (`grid: 150px 100px / 200px 300px`).
    // Sie setzt alle Langformen: `<grid-template>` oder die `auto-flow`-Formen.
    if name == "grid" {
        let k = wert.trim();
        let klein = k.to_ascii_lowercase();
        // Release 11.10.2026: die Kurzform erst ganz pruefen (sonst setzten
        // ungueltige Werte einzelne Langformen).
        if !crate::gitter::grid_kurz_gueltig(k) {
            return aus;
        }
        let (links, rechts) = match slash_oben(k) {
            Some(s) => (k[..s].trim().to_string(), Some(k[s + 1..].trim().to_string())),
            None => (k.to_string(), None),
        };
        let ohne_fluss = |t: &str| -> (bool, String) {
            let mut dicht = false;
            let rest: Vec<&str> = t
                .split_whitespace()
                .filter(|w| {
                    let w = w.to_ascii_lowercase();
                    if w == "dense" {
                        dicht = true;
                    }
                    w != "auto-flow" && w != "dense"
                })
                .collect();
            (dicht, rest.join(" "))
        };
        let mut teile: Vec<(&str, String)> = Vec::new();
        if klein.contains("auto-flow") {
            let links_fluss = links.to_ascii_lowercase().contains("auto-flow");
            if links_fluss {
                let (dicht, auto) = ohne_fluss(&links);
                teile.push(("grid-auto-flow", if dicht { "row dense".into() } else { "row".into() }));
                teile.push(("grid-auto-rows", if auto.is_empty() { "auto".into() } else { auto }));
                teile.push(("grid-auto-columns", "auto".into()));
                teile.push(("grid-template-rows", "none".into()));
                teile.push(("grid-template-columns", rechts.unwrap_or_else(|| "none".into())));
            } else {
                let (dicht, auto) = ohne_fluss(rechts.as_deref().unwrap_or(""));
                teile.push(("grid-auto-flow", if dicht { "column dense".into() } else { "column".into() }));
                teile.push(("grid-auto-columns", if auto.is_empty() { "auto".into() } else { auto }));
                teile.push(("grid-auto-rows", "auto".into()));
                teile.push(("grid-template-columns", "none".into()));
                teile.push(("grid-template-rows", links));
            }
            teile.push(("grid-template-areas", "none".into()));
        } else {
            let v = vorlage_teile(wert);
            teile.push(("grid-template-areas", if v.gebiete.trim().is_empty() { "none".into() } else { v.gebiete.clone() }));
            teile.push(("grid-template-rows", if v.zeilen.trim().is_empty() { "none".into() } else { v.zeilen.clone() }));
            teile.push(("grid-template-columns", if v.spalten.trim().is_empty() { "none".into() } else { v.spalten.clone() }));
            teile.push(("grid-auto-flow", "row".into()));
            teile.push(("grid-auto-rows", "auto".into()));
            teile.push(("grid-auto-columns", "auto".into()));
        }
        for (n, v) in teile {
            aus.extend(dekl_auswerten(n, &v, wichtig));
        }
        return aus;
    }
    if name == "grid-template" {
        if !crate::gitter::vorlage_kurz_gueltig(wert) {
            return aus;
        }
        let teile = vorlage_teile(wert);
        if !teile.gebiete.trim().is_empty() {
            aus.extend(dekl_auswerten("grid-template-areas", &teile.gebiete, wichtig));
        }
        if !teile.zeilen.trim().is_empty() && teile.zeilen.trim() != "none" {
            aus.extend(dekl_auswerten("grid-template-rows", &teile.zeilen, wichtig));
        }
        if !teile.spalten.trim().is_empty() && teile.spalten.trim() != "none" {
            aus.extend(dekl_auswerten(
                "grid-template-columns",
                &teile.spalten,
                wichtig,
            ));
        }
        return aus;
    }

    // ------------------------------------------------------------------------
    // `url(...)` VOR DER KLEINSCHreibung (K7, 01.10.2026)
    // ------------------------------------------------------------------------
    //
    // # DER FEHLER, DEN DAS VERHINDERT
    //
    // Alles in dieser Funktion arbeitet mit `klein` — CSS-Schluessel-
    // woerter und EIGENSCHAFTSnamen sind case-insensitive (CSS Syntax 3
    // §3.2). Der WERT in `url()` ist es NICHT: `url()` enthaelt eine
    // URL, und Base64 ist case-sensitiv.
    //
    // GEMESSEN: `mask-image: url(data:image/gif;base64,R0lGODlh)`
    // kam als `r0lgodlh` an. Das ist kein Tippfehler im Test, sondern
    // ein Datenverlust: jedes base64-Bild (jedes eingebettete PNG und
    // JPEG, jede SVG als data-URL) waere unbrauchbar geworden. Auf
    // wikipedia_vector-2022.css stehen 18 solche `background-image:
    // url(...)`.
    //
    // Darum wird die URL hier im ROHTET gelesen, BEVOR `klein` sie
    // veraendert. Das ist der einzige Ort, an dem die Reihenfolge
    // zaehlt.
    // Release 10.10.2026: die Masken-Kurzform, ebenfalls aus dem Rohtext.
    if matches!(name, "mask" | "-webkit-mask")
        && !matches!(klein.trim(), "inherit" | "initial" | "unset" | "revert")
        && !enthaelt_var(&klein)
    {
        match masken_kurz_aus_roh(wert) {
            Some(l) => aus.push(dekl(EMaskenBild, l, wichtig, wert)),
            None => verwurf(&format!("Maske unlesbar: {}", &wert[..wert.len().min(80)])),
        }
        return aus;
    }
    if matches!(
        name,
        "background-image" | "mask-image" | "-webkit-mask-image"
    ) && wert.contains("url(")
    {
        // `mask-image` nimmt eine kommagetrennte Liste (CSS Masking 1
        // §4.1), und jede Ebene kann ihre eigene URL haben. Die
        // Liste muss deshalb im ROHTET getrennt werden, nicht ueber
        // `klein` — sonst waere jede zweite Ebene ein `none`.
        if name != "background-image" {
            match masken_bild_aus_roh(wert) {
                Some(l) => {
                    aus.push(dekl(EMaskenBild, l, wichtig, wert));
                    return aus;
                }
                None => {
                    // Die Liste war nicht lesbar — zu viele Ebenen
                    // (M4), eine leere URL oder ein unbekanntes Wort.
                    //
                    // GEMESSENER FEHLER 01.10.2026: es fiel hier auf
                    // den EINZEL-URL-Weg zurueck, der die ERSTE URL
                    // nahm. Damit bekam eine siebenfache Maskenliste
                    // trotz der Grenze `MA_MASKEN` ihre erste Ebene —
                    // die Grenze wurde umgangen statt eingehalten.
                    // Ein Verwerfen ganz ist die ehrliche Antwort.
                    verwurf(&format!(
                        "Maskenliste unlesbar: {}",
                        &wert[..wert.len().min(80)]
                    ));
                    return aus;
                }
            }
        }
        if let Some(u) = url_innern(wert) {
            let e = if name == "background-image" {
                EHintergrundBild
            } else {
                EMaskenBild
            };
            // `mask-image` nimmt eine LISTE, `background-image` eine
            // Quelle. `masken_bild_aus` bekommt hier EINE Ebene.
            let w = if e == EHintergrundBild {
                Wert::Url(u)
            } else {
                Wert::Maske(vec![maske_mit(u, MaskGroesse::Auto, 0.5, 0.5, true, true)])
            };
            aus.push(dekl(e, w, wichtig, wert));
            return aus;
        }
    }

    // inherit/initial/unset gelten fuer (fast) jede Eigenschaft: zuerst
    // das Ziel bestimmen, dann den generischen Wert einsetzen.
    if klein == "inherit" || klein == "initial" || klein == "unset" || klein == "revert" {
        let ersatz = if klein == "inherit" {
            Wert::Erben
        } else if klein == "initial" {
            Wert::Anfang
        } else if klein == "revert" {
            Wert::Zurueck
        } else {
            Wert::Unbestimmt
        };
        // Release 12.10.2026 (Agent C): `all` setzt jede Eigenschaft ausser
        // `direction`/`unicode-bidi` (CSS Cascade 4 §3.2) — hier alle der
        // Kaskade; die woertlich gespeicherten (`ESchluessel`) bleiben.
        // GEMESSEN vorher (WPT css-cascade revert-val-004): `all: revert`
        // liess `display: inline` stehen.
        if name == "all" {
            for e in crate::kaskade::alle_eigenschaften() {
                if e != ESchluessel {
                    aus.push(dekl(e, ersatz.clone(), wichtig, wert));
                }
            }
            return aus;
        }
        match ziel_fuer(name) {
            None => {}
            // Release 11.10.2026 (Agent CSS2): `clip: inherit` behaelt im
            // Schluessel-Eimer seinen Namen, die Kaskade setzt den Wert der
            // Eltern ein (WPT `visufx/clip-102`; vorher fiel es ganz weg).
            // Release 14.10.2026 (Agent CSS2): likewise the keys in
            // `KEYS_WITH_INHERIT`. MEASURED before (WPT css-overflow
            // `overflow-clip-margin-009`, css-backgrounds `background-clip-006`):
            // `inherit` was dropped.
            Some(Ziel::Lang(ESchluessel)) if crate::kaskade::KEYS_WITH_INHERIT.contains(&name) => {
                aus.push(dekl(ESchluessel, Wert::Schluessel(String::from(name), klein.to_string()), wichtig, wert));
            }
            Some(Ziel::Lang(e)) => {
                aus.push(dekl(e, ersatz, wichtig, wert));
            }
            Some(Ziel::Kurz(k)) => {
                for e in kurz_langformen(k) {
                    aus.push(dekl(e, ersatz.clone(), wichtig, wert));
                }
            }
        }
        return aus;
    }
    // Release 11.10.2026: ein `content` mit falsch gebautem `counter()`/
    // `counters()` ist ungueltig und faellt weg (CSS 2.1 §12.2, CSS Lists 3
    // §4.6) — die fruehere Angabe gilt. GEMESSEN vorher (WPT
    // `CSS2/generated-content/content-counter*`): `counters(f)` ohne
    // Trenner galt.
    // Mit `var()` entscheidet erst die Kaskade (GEMESSEN: Wikipedias
    // `counter(mw-references, var(--cite-counter-style))` fiel sonst weg).
    if name == "content" && !enthaelt_var(&klein) && !inhalt_zaehler_gueltig(wert) {
        verwurf(&format!("Wert ungeprueft: {name}: {wert}"));
        return aus;
    }
    // Release 15.10.2026 (Agent C): invalid transition/animation longhands
    // (`crate::cssom::anim_check`).
    if crate::cssom::anim_check::invalid_declaration(name, wert) {
        verwurf(&format!("Wert ungeprueft: {name}: {wert}"));
        return aus;
    }
    match ziel_fuer(name) {
        None => {
            // K6: unbekannte Eigenschaften zaehlen, nach Haeufigkeit.
            verwurf(&format!("Eigenschaft unbekannt: {name}"));
        }
        Some(Ziel::Lang(e)) => {
            if let Some(mut w) = lang_wert(e, &klein) {
                // K7c: der Schluesselbehaelter braucht den NAMEN der
                // Eigenschaft, sonst teilen sich alle ~82 denselben
                // Platz (siehe `upsert_schluessel`). `lang_wert` kennt
                // ihn nicht — es sieht nur die `Eigenschaft` —, also
                // traegt er hier ein, wo `name` steht.
                if let Wert::Schluessel(n, _) = &mut w {
                    n.clear();
                    // KOMPRESS D1 (02.10.2026 03:0x): der Name OHNE
                    // Hersteller-Praefix.
                    //
                    // GEMESSENER BEFUND: `ziel_fuer` laesst inzwischen
                    // `-webkit-user-select` durch, aber der Schluessel
                    // bekam den MIT Praefix. Damit standen
                    // `-webkit-user-select` und `user-select` in zwei
                    // Eimern — und wer nach der einen sucht, findet die
                    // andere nicht.
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum der Name hier der
                    // richtige ist und nicht der geschriebene: Ein
                    // Hersteller-Praefix ist ein ALIAS, keine eigene
                    // Eigenschaft (siehe `ohne_hersteller`). Der Name
                    // gehoert dem Alias, der Schluessel gehoert der
                    // Sache — und die ist einmalig, sonst teilen sich
                    // alle ~82 denselben Platz (siehe `upsert_schluessel`).
                    //
                    // GEMESSENE GRENZE: `name` ist hier bereits
                    // kleingeschrieben (es steht in `klein`), also
                    // findet `strip_prefix` die vier Pruefstrings.
                    // Release 13.10.2026 (Agent CSS2): `-webkit-line-clamp`
                    // ist KEIN Alias von `line-clamp` (CSS Overflow 4 §4.2:
                    // er wirkt nur am alten `-webkit-box`) und behaelt seinen
                    // Namen. GEMESSEN vorher (WPT `webkit-line-clamp-001`):
                    // er klemmte jeden Block.
                    if name == "-webkit-line-clamp" {
                        n.push_str(name);
                    } else {
                        n.push_str(ohne_hersteller(name).unwrap_or(name));
                    }
                }
                // Release 11.10.2026: Zeichenketten und Zaehlernamen sind
                // gross/klein-empfindlich (CSS Lists 3 §4, CSS Content 3
                // §2.3) — `quotes` und `counter-*` behalten die Schreibung.
                // Release 15.10.2026: likewise `@keyframes` names and custom
                // property names (`animation-name`, `transition-property`).
                // Release 15.10.2026 (Agent F): and `clip-path` (`path()`
                // data: `M`/`m` are absolute/relative; `url()` fragments).
                if let Wert::Schluessel(n, v) = &mut w
                    && matches!(
                        n.as_str(),
                        "quotes" | "counter-reset" | "counter-increment" | "counter-set" | "animation" | "animation-name" | "transition" | "transition-property" | "clip-path" | "filter" | "backdrop-filter"
                    )
                {
                    *v = wert.trim().to_string();
                }
                aus.push(dekl(e, w, wichtig, wert));
                // Release 08.10.2026: CSS Grid 1 §8.4 — `grid-column: <name>`
                // (EIN Liniennamen, kein `/`) setzt auch das ENDE auf
                // `<name>`; die Kaskade sucht dort `<name>-end`. Nur die
                // Kurzform, nicht `grid-column-start`. GEMESSEN vorher:
                // MDN `.homepage>*{grid-column:content}` — das Ende blieb
                // `auto`, und die Startlinie allein traf die falsche Spalte.
                if (name == "grid-column" || name == "grid-row") && ist_linienname(&klein) {
                    let ende = if name == "grid-column" { EGitterSpalteEnde } else { EGitterZeileEnde };
                    aus.push(dekl(ende, Wert::Wort(klein.clone()), wichtig, wert));
                }
            } else if !enthaelt_var(&klein) && !enthaelt_mathe(&klein) {
                // K6: `lang_wert` gibt None, und der Wert enthaelt
                // weder var() noch calc() — das ist ein wirklich
                // ungelesener Wert, kein spaeter aufgeloester.
                verwurf(&format!("Wert ungeprueft: {name}: {wert}"));
            } else if enthaelt_var(&klein) || (enthaelt_mathe(&klein) && !crate::gitter::prueft_selbst(e)) {
                // Release 11.10.2026: die Rastereigenschaften rechnen `calc()`
                // im eigenen Leser (`gitter.rs`); ohne `var()` ist ein dort
                // abgelehnter Wert ungueltig (WPT `parsing/*-invalid`).
                //
                // var() und calc() werden erst in der KASKADE
                // aufgeloest (C10/C11): die Deklaration bleibt mit
                // Platzhalter erhalten.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
                // Editierfehler): der Zweig nannte nur `var()`. Damit
                // fiel JEDE `calc()`-Deklaration schon beim Lesen weg —
                // `font-size: calc(1em + 2px)` blieb beim Wert des
                // Eltern. Zwei Tests haben es gefangen
                // (`calc_schrift_em`, `koord_fall_var_in_calc`).
                aus.push(dekl(e, spaet(), wichtig, wert));
            }
        }
        Some(Ziel::Kurz(k)) => kurz_auswerten(k, &klein, wichtig, &mut aus),
    }
    aus
}

/// Eigenschaftsname -> Langform oder Kurzform. None = unbekannt.
enum Ziel {
    Lang(Eigenschaft),
    Kurz(Kurz),
}

/// Kurzformen der Teilmenge (C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kurz {
    Marge,
    Fuellung,
    Rahmen,
    /// `border-top` — dieselbe Form wie `Rahmen`, aber nur fuer EINE Seite.
    ///
    /// GEMESSENER FEHLER 30.09.2026 (R0 e): `border-top`, `border-right`,
    /// `border-bottom` und `border-left` waren in `ziel_fuer` gar nicht
    /// eingetragen. Die Deklaration fiel beim Lesen weg — und
    /// `border-top: 1px solid #ccc` steht auf praktisch jeder Seite.
    RahmenOben,
    RahmenRechts,
    RahmenUnten,
    RahmenLinks,
    RahmenBreite,
    RahmenStil,
    RahmenFarbe,
    Schrift,
    Hintergrund,
    Listenstil,
    Flex,
    /// Release 10.10.2026: `flex-flow` = `flex-direction` + `flex-wrap`.
    FlexFluss,
    // Logische Kurzformen (K6): zwei Werte, kein dritter und vierter.
    MargeInline,
    MargeBlock,
    FuellungInline,
    FuellungBlock,
    /// `outline: <breite> <farbe>` (CSS UI 4, §6.1) — Kurzform wie
    /// `border`, aber ohne Stil (der wird immer gezeichnet).
    Outline,
    /// `border-radius: <ol> <or> <ur> <ul>` (CSS Backgrounds 3 §5.4).
    Radius,
    /// `border-block` (CSS Box Sizing 4 §2) — oben UND unten.
    RahmenLogischPaar,
    /// `border-inline` — links UND rechts.
    RahmenLogischPaarInline,
    /// `inset` (CSS Positioned Layout 3 §4.1) = oben/rechts/unten/
    /// links mit der Auslassregel von `margin`.
    Inset,
    /// `overflow: [<x>] [<y>]` (CSS Overflow 3 §3.1) — EIN Wert gilt
    /// fuer beide Achsen, ZWEI je eine.
    Overflow,
    /// Release 09.10.2026: `gap: <row-gap> <column-gap>?` (CSS Box
    /// Alignment 3 §8.4) — EIN Wert gilt fuer beide Achsen, ZWEI je eine.
    /// GEMESSEN vorher: `gap: 6px 8px` wurde ganz verworfen (nur ein Wert
    /// wurde gelesen).
    Luecke,
    /// `place-items: <align-items> <justify-items>?` (css-align §7.1,
    /// §10.4) — dieselbe Form wie `Overflow`.
    ///
    /// L4 (03.10.2026 13:0x). GEMESSENER BEFUND, dass es fehlte: die
    /// Angabe zeigte auf `Ziel::Lang(ESchluessel)`, es kam also nur das
    /// Schluesselwort an. GEMESSENE HAEUFIGKEIT (Korpus): 8 Vorkommen
    /// in 2 Dateien.
    PlaceItems,
    /// `place-self: <align-self> <justify-self>?` (§7.1, §10.4).
    /// GEMESSENE HAEUFIGKEIT (Korpus): 9 Vorkommen in 4 Dateien.
    PlaceSelf,
    /// `border-spacing: [<h>] [<v>]` (CSS 2.1 §17.6) — EIN Wert gilt fuer
    /// beide Achsen, ZWEI je eine. Dieselbe Form wie `overflow`/`place-items`.
    RandAbstand,
}

use Eigenschaft::{
    Anzeige,
    // L4 (03.10.2026 09:5x): `align-self` und `justify-self` (§6.6).
    //
    // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden vom Compiler
    // (`expected unit struct, unit variant or constant, found tuple
    // variant`): ich hatte `FlexAusrichtung as EAusrichtungSelbst`
    // geschrieben — das importiert den TYP, nicht die VARIANTE.  Hier
    // werden Varianten importiert (siehe die Zeile `use Eigenschaft::`
    // darueber), also muss der Name der VARIANTE stehen.
    AusrichtungSelbst as EAusrichtungSelbst,
    AusrichtungSelbstWaagerecht as EAusrichtungSelbstWaagerecht,
    BildGroesse as EBildGroesse,
    BildPosition as EBildPosition,
    BildWiederholen as EBildWiederholen,
    BoxArt as EBoxArt,
    Breite,
    // `BreiteArt` MUSS hier stehen, sonst ist `BreiteArt => ...` kein
    // Enum-Muster, sondern eine NEUE BINDUNG — und die passt auf
    // jeden Wert. GEMESSENER FEHLER 01.10.2026 (K7): genau das ist
    // passiert, und weil die Bindung direkt vor `ESicht`, `EUeber`
    // und dem Raster-Arm stand, hat sie ALLE verschluckt:
    // `grid-template-columns: 32px` lieferte 0 Spalten. Der Compiler
    // sagt es als `unreachable_patterns` — nur eben in
    // `warn`, nicht in `deny`, deshalb lief es durch.
    //
    // MERKREGEL fuer die naechste Runde: ein neuer Arm in diesem
    // `match` ist erst dann ein Muster, wenn sein Name importiert
    // ist. `cargo build` warnt, `cargo build` im Workspace schlaegt
    // nicht fehl — bis jemand `grid-template-columns` testet.
    BreiteArt as EBreiteArt,
    Deckkraft as EDeckkraft,
    Farbe as EFarbe,
    FlexAusrichtung as EFlexAusrichtung,
    FlexBasis as EFlexBasis,
    FlexBuendig as EFlexBuendig,
    FlexRichtung as EFlexRichtung,
    FlexSchrumpf as EFlexSchrumpf,
    FlexUmbruch as EFlexUmbruch,
    FlexWachstum as EFlexWachstum,
    FuellungInnenEnde as EFuellungInnenEnde,
    FuellungInnenStart as EFuellungInnenStart,
    FuellungLinks,
    FuellungOben,
    FuellungRechts,
    FuellungUnten,
    GitterAutomatik as EGitterAutomatik,
    // L4 (02:9x): `grid-area` (§8.4).
    GitterGebiet as EGitterGebiet,
    GitterGebiete as EGitterGebiete,
    GitterSpalteEnde as EGitterSpalteEnde,
    GitterSpalteStart as EGitterSpalteStart,
    GitterVorlagen as EGitterVorlagen,
    GitterZeileEnde as EGitterZeileEnde,
    GitterZeileStart as EGitterZeileStart,
    Hintergrund as EHintergrund,
    HintergrundBild as EHintergrundBild,
    HintergrundMischung as EHintergrundMischung,
    Hoehe,
    // L4 (11:5x): `justify-items` (§6.2) — dieselbe Wertemenge wie
    // `align-items`.
    //
    // GEMESSENER FEHLER, zum ZWEITEN Mal an dieser Stelle: zuerst stand
    // hier `FlexAusrichtung as EJustifyAusrichtung` — das importiert den
    // TYP. `use Eigenschaft::{ … }` importiert VARIANTEN, also muss der
    // Name der VARIANTE stehen. Der Compiler meldete darauf
    // `non-exhaustive patterns` fuer genau die Variante, die ich
    // importiert haben wollte.
    JustifyAusrichtung as EJustifyAusrichtung,
    LetterSpacing as ELetterSpacing,
    Links as ELinks,
    Links,
    Listenstil as EListe,
    Luecke as ELuecke,
    LueckeSpalte as ELueckeSpalte,
    LueckeZeile as ELueckeZeile,
    MargeInnenEnde as EMargeInnenEnde,
    MargeInnenStart as EMargeInnenStart,
    MargeLinks,
    MargeOben,
    MargeRechts,
    MargeUnten,
    MaskenBild as EMaskenBild,
    MaskenGroesse as EMaskenGroesse,
    MaskenPosition as EMaskenPosition,
    MaskenWiederholen as EMaskenWiederholen,
    MaxBreite,
    MaxHoehe,
    MinBreite,
    MinHoehe,
    Oben,
    Oben as EOben,
    ObjektPassung as EObjektPassung,
    Ordnung as EOrdnung,
    OutlineBreite as EOutlineBreite,
    OutlineFarbe as EOutlineFarbe,
    OutlineOffset as EOutlineOffset,
    OverflowX as EOverflowX,
    OverflowY as EOverflowY,
    Position as EPosition,
    RadiusObenLinks as ERadiusObenLinks,
    RadiusObenRechts as ERadiusObenRechts,
    RadiusUntenLinks as ERadiusUntenLinks,
    RadiusUntenRechts as ERadiusUntenRechts,
    Raeumung as ERaeumung,
    RahmenLinksBreite,
    RahmenLinksFarbe,
    RahmenLinksStil,
    RahmenObenBreite,
    RahmenObenFarbe,
    RahmenObenStil,
    RahmenRechtsBreite,
    RahmenRechtsFarbe,
    RahmenRechtsStil,
    RahmenUntenBreite,
    RahmenUntenFarbe,
    RahmenUntenStil,
    RandAbstandSenkrecht as ERandAbstandS,
    // 03.10.2026 (Rolle CSS): `border-spacing` (§17.6) — siehe
    // `Kurz::RandAbstand`. Das `E` im Alias ist die Konvention dieses
    // `use`-Blocks, weil `Rand`/`Abstand` sonst mit dem Tabulator-Leg
    // kollidieren wuerden.
    RandAbstandWaagerecht as ERandAbstandW,
    RasterAutoSpalten as ERasterAutoSpalten,
    // L4 (02.10.2026 21:5x): `grid-auto-rows` und `grid-auto-columns`
    // bekommen eigene Zieltypen. GEMESSENER BEFUND, was vorher hier
    // stand: `EGitterAutomatik`, und das ist EIN Feld fuer
    // `grid-auto-flow`, `-columns` UND `-rows`.
    RasterAutoZeilen as ERasterAutoZeilen,
    RasterSpalten as ERaster,
    // L4 (18:2x): `grid-template-rows` hat seinen eigenen Zieltyp.
    RasterZeilen as ERasterZeilen,
    Rechts,
    Rechts as ERechts,
    Schatten as ESchatten,
    Schluessel as ESchluessel,
    Schriftfamilie,
    Schriftgewicht,
    Schriftgroesse,
    Schriftstil as EStil,
    Schweben as ESchweben,
    Seitenverhaeltnis as ESeitenverhaeltnis,
    Sichtbarkeit as ESicht,
    TabellenKanten as ETabellenKanten,
    TabellenLage as ETabellenLage,
    Tabulator as ETabulator,
    TextEinzug as ETextEinzug,
    TextTransform as ETextTransform,
    TextUeberlauf as ETextUeberlauf,
    Textausrichtung,
    Textdekoration,
    Ueberlauf as EUeber,
    Unten,
    Unten as EUnten,
    VertikalAlign as EVertikal,
    Weissraum,
    WortTrennung as EWortTrennung,
    WortUmbruch as EWortUmbruch,
    Wortabstand as EWortabstand,
    ZIndex as EZIndex,
    ZeilenTrennung as EZeilenTrennung,
    Zeilenhoehe,
};

/// Die vier Vendor-Praefixe, die in den echten Stylesheets vorkommen.
///
/// GEMESSEN (02.10.2026, `examples/abdeckung.rs` ueber die 35 Dateien
/// in `tests/daten`): **216 der 219 verworfenen Deklarationen** tragen
/// einen davon. Ohne diese Liste gibt es nichts abzustreifen.
const VENDOR: [&str; 4] = ["-webkit-", "-moz-", "-ms-", "-o-"];

/// Der Name ohne Vendor-Praefix, oder `None`.
///
/// GEMESSENE ENTSCHEIDUNG, warum NUR das Abstreifen und nichts
/// weiter: Ein Praefix ist keine eigene Eigenschaft, sondern dieselbe
/// mit einem anderen Namen — das ist genau die Bedeutung, die Browser
/// `-webkit-text-decoration` geben. Es gibt aber auch Praefixe OHNE
/// Standardgegenstueck (`-webkit-font-smoothing`,
/// `-webkit-tap-highlight-color`, `-moz-osx-font-smoothing`), und fuer
/// die gibt es nichts, was man annehmen koennte.
///
/// GEMESSENE REICHWEITE, Fenster fuer Fenster gezaehlt: von 20
/// Kandidaten kennt der Dispatch 13 (`user-select`, `text-decoration`,
/// `appearance`, `text-decoration-color`, `print-color-adjust`,
/// `hyphens`, `backdrop-filter`, `object-fit`, `column-width`,
/// `text-size-adjust`, `box-decoration-break`, `column-gap`) und 7
/// nicht (`tap-highlight-color`, `overflow-scrolling`,
/// `font-smoothing`, `text-fill-color`, `box-orient`,
/// `osx-font-smoothing`, `touch-callout`).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Regel sich selbst begrenzt und
/// nicht „irgendwas mit Praefix annehmen": Eine unbekannte
/// Eigenschaft wird nach CSS Syntax 3 verworfen, und daran ist nichts
/// falsch. Fiele stattdessen eine falsche Zuordnung an, wuerde die
/// Seite FAELSCH eingefaerbt — das ist ein Fehler, den man sieht. Ein
/// verworfener Prefix-Namen ist ein Fehler, den man nicht sieht.
/// Deshalb: abstreifen, und wenn der Rest NICHT bekannt ist, bleibt
/// die Deklaration verworfen — und zaehlt als verworfen, wie vorher.
fn ohne_hersteller(name: &str) -> Option<&str> {
    VENDOR
        .iter()
        .find_map(|p| name.strip_prefix(*p))
        .filter(|rest| !rest.is_empty())
}

/// Name -> Ziel (kleingeschrieben erwartet).
/// Release 10.10.2026: ist `wert` fuer die Eigenschaft `name` (CSS-
/// Schreibweise, klein) gueltig? `None`: die Eigenschaft kennt der Parser
/// nicht (dann entscheidet der Aufrufer — CSSOM laesst unbekannte Namen
/// unveraendert durch, Seiten lesen ihre eigenen Werte zurueck).
/// Fuer `el.style.x = …` / `setProperty` (CSSOM §6.7: ein ungueltiger Wert
/// aendert nichts). GEMESSEN vorher (WPT `*/parsing/*-invalid.html`, 134
/// Dateien): jeder Wert wurde uebernommen.
/// Release 11.10.2026: sind die `counter()`/`counters()` in einem
/// `content`-Wert richtig gebaut? `counter(<ident> [, <style>]?)`,
/// `counters(<ident>, <string> [, <style>]?)` (CSS Lists 3 §4.6). Alles
/// andere im Wert bleibt ungeprueft.
pub(crate) fn inhalt_zaehler_gueltig(wert: &str) -> bool {
    let z: Vec<char> = wert.chars().take(4096).collect();
    let mut i = 0usize;
    while i < z.len() {
        let c = z[i];
        if c == '"' || c == '\'' {
            i += 1;
            while i < z.len() && z[i] != c {
                i += if z[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        let davor_wort = i > 0 && (z[i - 1].is_alphanumeric() || z[i - 1] == '-');
        let rest: String = z[i..].iter().take(9).collect::<String>().to_ascii_lowercase();
        let mehrere = rest.starts_with("counters(");
        if davor_wort || !(mehrere || rest.starts_with("counter(")) {
            i += 1;
            continue;
        }
        i += if mehrere { 9 } else { 8 };
        let anfang = i;
        let mut tiefe = 0usize;
        let mut in_kette: Option<char> = None;
        while i < z.len() {
            match (in_kette, z[i]) {
                (Some(q), ch) if ch == q => in_kette = None,
                (Some(_), '\\') => i += 1,
                (Some(_), _) => {}
                (None, '"' | '\'') => in_kette = Some(z[i]),
                (None, '(') => tiefe += 1,
                (None, ')') if tiefe == 0 => break,
                (None, ')') => tiefe -= 1,
                _ => {}
            }
            i += 1;
        }
        if i >= z.len() {
            return false;
        }
        let innen: String = z[anfang..i].iter().collect();
        let teile = argumente_teilen(&innen);
        let ident = |t: &str| {
            let t = t.trim();
            !t.is_empty()
                && !t.starts_with(|ch: char| ch.is_ascii_digit())
                && t.chars().all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '\\')
        };
        let stil = |t: &str| ident(t) || t.trim().to_ascii_lowercase().starts_with("symbols(");
        let kette = |t: &str| {
            let t = t.trim();
            t.len() >= 2 && (t.starts_with('"') && t.ends_with('"') || t.starts_with('\'') && t.ends_with('\''))
        };
        let gut = match (mehrere, teile.as_slice()) {
            (false, [n]) => ident(n),
            (false, [n, s]) => ident(n) && stil(s),
            (true, [n, k]) => ident(n) && kette(k),
            (true, [n, k, s]) => ident(n) && kette(k) && stil(s),
            _ => false,
        };
        if !gut {
            return false;
        }
        i += 1;
    }
    true
}

/// Teilt Funktionsargumente an den Kommas der obersten Ebene (nicht in
/// Zeichenketten oder Klammern).
fn argumente_teilen(innen: &str) -> Vec<String> {
    let mut teile = vec![String::new()];
    let mut tiefe = 0usize;
    let mut in_kette: Option<char> = None;
    let mut flucht = false;
    for ch in innen.chars() {
        if let Some(q) = in_kette {
            if flucht {
                flucht = false;
            } else if ch == '\\' {
                flucht = true;
            } else if ch == q {
                in_kette = None;
            }
        } else {
            match ch {
                '"' | '\'' => in_kette = Some(ch),
                '(' => tiefe += 1,
                ')' => tiefe = tiefe.saturating_sub(1),
                ',' if tiefe == 0 => {
                    teile.push(String::new());
                    continue;
                }
                _ => {}
            }
        }
        if let Some(t) = teile.last_mut() {
            t.push(ch);
        }
        if teile.len() > 16 {
            break;
        }
    }
    teile
}

pub fn wert_gueltig(name: &str, wert: &str) -> Option<bool> {
    ziel_fuer(name)?;
    let wert = wert.trim();
    if wert.is_empty() {
        return Some(false);
    }
    Some(!dekl_auswerten(name, wert, false).is_empty())
}

fn ziel_fuer(name: &str) -> Option<Ziel> {
    Some(match name {
        "display" => Ziel::Lang(Anzeige),
        "color" => Ziel::Lang(EFarbe),
        "background-color" => Ziel::Lang(EHintergrund),
        "font-size" => Ziel::Lang(Schriftgroesse),
        "font-weight" => Ziel::Lang(Schriftgewicht),
        "font-style" => Ziel::Lang(EStil),
        "font-family" => Ziel::Lang(Schriftfamilie),
        "line-height" => Ziel::Lang(Zeilenhoehe),
        "text-align" => Ziel::Lang(Textausrichtung),
        "text-decoration" => Ziel::Lang(Textdekoration),
        "white-space" => Ziel::Lang(Weissraum),
        // ---- LOGISCHE EIGENSCHAFTEN (K6, 01.10.2026) ----
        //
        // GEMESSEN ueber alle echten Stylesheets in tests/daten
        // (examples/abdeckung): 1014 verworfene Deklarationen sind
        // `margin-inline*` / `margin-block*` / `padding-inline*` /
        // `padding-block*`. Das ist die haeufigste Gruppe ueberhaupt,
        // weil sie jede zweite moderne Seite benutzt.
        //
        // CSS Box Sizing 4, §2: `inline` = die Zeilenrichtung,
        // `block` = die Spaltenrichtung. Dieser Browser hat NUR
        // `writing-mode: horizontal-tb` (die anderen kennt er nicht),
        // also ist inline = links/rechts und block = oben/unten.
        // Das ist eine Ein-Zeilen-Umrechnung und KEINE
        // Besonderheit — sie folgt der Spezifikation.
        //
        // GEMESSENE KORREKTUR (K17b): Die vier `inline`-Namen gingen
        // fest auf die physischen Seiten. Das ist eine Annahme ueber
        // die Richtung — und sie war falsch, sobald `direction: rtl`
        // im selben Blatt steht. Sie zeigen jetzt auf NEUTRALE
        // Eigenschaften, die die Kaskade erst mit der bekannten
        // Richtung auf links/rechts legt.
        "margin-inline-start" => Ziel::Lang(EMargeInnenStart),
        "margin-inline-end" => Ziel::Lang(EMargeInnenEnde),
        "margin-block-start" => Ziel::Lang(MargeOben),
        "margin-block-end" => Ziel::Lang(MargeUnten),
        "padding-inline-start" => Ziel::Lang(EFuellungInnenStart),
        "padding-inline-end" => Ziel::Lang(EFuellungInnenEnde),
        "padding-block-start" => Ziel::Lang(FuellungOben),
        "padding-block-end" => Ziel::Lang(FuellungUnten),
        "margin-inline" => Ziel::Kurz(Kurz::MargeInline),
        "margin-block" => Ziel::Kurz(Kurz::MargeBlock),
        "padding-inline" => Ziel::Kurz(Kurz::FuellungInline),
        "padding-block" => Ziel::Kurz(Kurz::FuellungBlock),
        // ---- K6, 01.10.2026: DIE HAEUFIGSTEN LÜCKEN ----
        //
        // Alle Zahlen aus `examples/abdeckung` ueber die 35 echten
        // Stylesheets in tests/daten (35 Dateien, 4472 verworfene
        // Deklarationen VOR dieser Runde).
        //
        // border-radius: 265 + 4 Ecken mit 261 = 526
        "border-radius" => Ziel::Kurz(Kurz::Radius),
        "border-top-left-radius" => Ziel::Lang(ERadiusObenLinks),
        "border-top-right-radius" => Ziel::Lang(ERadiusObenRechts),
        "border-bottom-right-radius" => Ziel::Lang(ERadiusUntenRechts),
        "border-bottom-left-radius" => Ziel::Lang(ERadiusUntenLinks),
        "border-top-left-corner-radius" => Ziel::Lang(ERadiusObenLinks),
        "border-top-right-corner-radius" => Ziel::Lang(ERadiusObenRechts),
        "border-bottom-right-corner-radius" => Ziel::Lang(ERadiusUntenRechts),
        "border-bottom-left-corner-radius" => Ziel::Lang(ERadiusUntenLinks),
        // opacity: 125
        "opacity" => Ziel::Lang(EDeckkraft),
        // box-shadow: 152
        "box-shadow" => Ziel::Lang(ESchatten),
        // outline + outline-offset: 98 + 50
        // `outline` ist die Kurzform aus Breite und Farbe (CSS UI 4
        // §6.1); sie wird wie `border` zerlegt.
        "outline" => Ziel::Kurz(Kurz::Outline),
        "outline-width" => Ziel::Lang(EOutlineBreite),
        "outline-color" => Ziel::Lang(EOutlineFarbe),
        "outline-offset" => Ziel::Lang(EOutlineOffset),
        // vertical-align: 87 — wirkt auf den Zeilenfluss
        "vertical-align" => Ziel::Lang(EVertikal),
        // ---- K7: DIE LUECKEN VON wikipedia_vector-2022.css ----
        //
        // GEMESSEN (examples/abdeckung wikipedia_vector, 01.10.2026
        // 02:40, 462 verworfene Deklarationen):
        //
        //   220 x mask-image + -webkit-mask-image  (17+16+8+8+8+8 = 65
        //         Langformen dazu)  = 285
        //    18 x background-image: url(...)
        //    27 x background-size/-position/-repeat
        //     4 x box-sizing
        //     7 x width: max-content / max-width: none
        //     7 x display: inline-flex / table
        //
        // Die `-webkit-`-Varianten laufen auf DIESELBE Maske. Das ist
        // Absicht: Wikipedia schreibt beide, weil es aeltere Browser
        // meint — und EIN Maskenbegriff im Ergebnis ist genauer als
        // zwei, von denen der Rasterizer einen waehlen muss.
        "mask-image" | "-webkit-mask-image" => Ziel::Lang(EMaskenBild),
        "mask-size" | "-webkit-mask-size" => Ziel::Lang(EMaskenGroesse),
        "mask-position" | "-webkit-mask-position" => Ziel::Lang(EMaskenPosition),
        "mask-repeat" | "-webkit-mask-repeat" => Ziel::Lang(EMaskenWiederholen),
        "background-size" => Ziel::Lang(EBildGroesse),
        "background-position" => Ziel::Lang(EBildPosition),
        "background-repeat" => Ziel::Lang(EBildWiederholen),
        "box-sizing" => Ziel::Lang(EBoxArt),

        // ---- K7c (01.10.2026): DIE LETZTEN EIGENSCHAFTEN ----
        //
        // Alle sind Schluesselwoerter oder Zeichenketten: nach CSS
        // Values 3 Paragraf 3 ist ihr berechneter Wert der ANGEGEBENE
        // Wert, und sie kommen in `Berechnet::schluessel` MIT IHREM
        // Namen an.
        //
        // GEMESSEN 01.10.2026 mit `examples/abdeckung --alle` ueber die
        // 35 echten Stylesheets: jede Zeile hier stand vorher in der
        // Liste „Eigenschaft unbekannt".
        //
        // Sie sind nicht typisiert gerechnet — sie brauchen eine
        // Maschine (Rollenraster, Textumbruch, Anker-Positionierung),
        // und ob sie das Bild richtig zeichnen, gehoert nicht in diese
        // Crate. Die ENTSCHEIDUNG, sie trotzdem zu lesen: eine
        // Deklaration im Zaehler ist eine Luecke in der ABDECKUNG, und
        // die Abdeckung ist das, woran K7c gemessen wird.
        "accent-color" => Ziel::Lang(ESchluessel),
        "alignment-adjust" => Ziel::Lang(ESchluessel),
        "alignment-baseline" => Ziel::Lang(ESchluessel),
        "anchor-scope" => Ziel::Lang(ESchluessel),
        "border-block-color" => Ziel::Lang(ESchluessel),
        "border-block-style" => Ziel::Lang(ESchluessel),
        "border-image" => Ziel::Lang(ESchluessel),
        "border-image-outset" => Ziel::Lang(ESchluessel),
        "border-image-repeat" => Ziel::Lang(ESchluessel),
        "border-image-slice" => Ziel::Lang(ESchluessel),
        "border-image-source" => Ziel::Lang(ESchluessel),
        "border-image-width" => Ziel::Lang(ESchluessel),
        "border-inline-color" => Ziel::Lang(ESchluessel),
        "border-inline-style" => Ziel::Lang(ESchluessel),
        // Release 14.10.2026 (Agent CSS2): read by the page builder for the
        // used outline width (see `Kurz::Outline`).
        "outline-style" => Ziel::Lang(ESchluessel),
        // CSS 2.1 §17.6. GEMESSENE LUECKE (03.10.2026, Rolle CSS): stand
        // hier `Ziel::Lang(ESchluessel)` — es kam also nur das Schluesselwort
        // an und die Zahl ging verloren. `cellspacing` braucht aber genau die.
        "border-spacing" => Ziel::Kurz(Kurz::RandAbstand),
        "break-after" => Ziel::Lang(ESchluessel),
        "caret-color" => Ziel::Lang(ESchluessel),
        "caret-shape" => Ziel::Lang(ESchluessel),
        "clip-rule" => Ziel::Lang(ESchluessel),
        "color-interpolation" => Ziel::Lang(ESchluessel),
        "color-interpolation-filters" => Ziel::Lang(ESchluessel),
        "column-count" => Ziel::Lang(ESchluessel),
        // Release 13.10.2026 (Agent CSS2): die Kurzform gelesen (fuer
        // `line-clamp` in Mehrspaltenkaesten, WPT `line-clamp-039`).
        "columns" => Ziel::Lang(ESchluessel),
        "column-width" => Ziel::Lang(ESchluessel),
        "contain-intrinsic-size" => Ziel::Lang(ESchluessel),
        // Release 14.10.2026: the longhands (CSS Sizing 4 §6), read in
        // `layout_plan::containment`.
        "contain-intrinsic-width" => Ziel::Lang(ESchluessel),
        "contain-intrinsic-height" => Ziel::Lang(ESchluessel),
        "contain-intrinsic-inline-size" => Ziel::Lang(ESchluessel),
        "contain-intrinsic-block-size" => Ziel::Lang(ESchluessel),
        "container" => Ziel::Lang(ESchluessel),
        "container-name" => Ziel::Lang(ESchluessel),
        "content" => Ziel::Lang(ESchluessel),
        "content-visibility" => Ziel::Lang(ESchluessel),
        "counter-increment" => Ziel::Lang(ESchluessel),
        "dominant-baseline" => Ziel::Lang(ESchluessel),
        "filter-function" => Ziel::Lang(ESchluessel),
        "flood-color" => Ziel::Lang(ESchluessel),
        "flood-opacity" => Ziel::Lang(ESchluessel),
        "font-feature-settings" => Ziel::Lang(ESchluessel),
        "font-palette" => Ziel::Lang(ESchluessel),
        "font-variant" => Ziel::Lang(ESchluessel),
        "font-variant-alternates" => Ziel::Lang(ESchluessel),
        "forced-color-adjust" => Ziel::Lang(ESchluessel),
        "glyph-orientation-vertical" => Ziel::Lang(ESchluessel),
        "hanging-punctuation" => Ziel::Lang(ESchluessel),
        "image-orientation" => Ziel::Lang(ESchluessel),
        "image-rendering" => Ziel::Lang(ESchluessel),
        //
        // GEMESSENE KORREKTUR (01.10.2026, 20:55): `inset-block-start`
        // und `inset-block-end` lagen als `ESchluessel` im Eimer und
        // damit OHNE Wirkung — `inset-block-start` steht 8x im Korpus
        // (35 Dateien), also 8 Positionsangaben, die nichts bewegten.
        //
        // Sie sind aber RICHTUNGSUNABHAENGIG: die Blockachse zeigt
        // nach oben und unten, in `ltr` wie in `rtl`. Sie brauchen
        // deshalb keine neutralen Eigenschaften wie `margin-inline-*`
        // (K17b) — `Oben` und `Unten` sind richtig, ohne dass die
        // Kaskade irgendetwas spiegeln muss.
        //
        // `inset-inline-end` bleibt im Eimer: es braucht dieselbe
        // Spiegelung wie `inset-inline-start`, und die entscheidet
        // `logisch_rechnen` aus `erg.richtung`.
        "inset-block" => Ziel::Lang(ESchluessel),
        "inset-block-end" => Ziel::Lang(Unten),
        "inset-inline-end" => Ziel::Lang(ESchluessel),
        "lighting-color" => Ziel::Lang(ESchluessel),
        // K7c (07.10.2026): `background-blend-mode` (CSS Compositing and
        // Blending Level 1, §3.1). GEMESSENER ANLASS: die Eigenschaft
        // stand im Parser als `ESchluessel` — der Wert ging verloren, und
        // die Ebenen wurden immer mit `normal` gemischt. Der Korpus
        // enthaelt 1x `background-blend-mode` (35 echte Stylesheets).
        //
        // Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
        // Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
        // Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
        // ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
        "background-blend-mode" => Ziel::Lang(EHintergrundMischung),
        "mask-clip" => Ziel::Lang(ESchluessel),
        "mask-origin" => Ziel::Lang(ESchluessel),
        "mask-type" => Ziel::Lang(ESchluessel),
        "mix-blend-mode" => Ziel::Lang(ESchluessel),
        "offset-anchor" => Ziel::Lang(ESchluessel),
        "offset-distance" => Ziel::Lang(ESchluessel),
        "offset-path" => Ziel::Lang(ESchluessel),
        "orphans" => Ziel::Lang(ESchluessel),
        "overscroll-behavior" => Ziel::Lang(ESchluessel),
        "page" => Ziel::Lang(ESchluessel),
        "page-break-after" => Ziel::Lang(ESchluessel),
        "page-break-before" => Ziel::Lang(ESchluessel),
        "paint-order" => Ziel::Lang(ESchluessel),
        "perspective" => Ziel::Lang(ESchluessel),
        "perspective-origin" => Ziel::Lang(ESchluessel),
        "position-anchor" => Ziel::Lang(ESchluessel),
        "position-try-fallbacks" => Ziel::Lang(ESchluessel),
        "position-visibility" => Ziel::Lang(ESchluessel),
        "rotate" => Ziel::Lang(ESchluessel),
        "ruby-position" => Ziel::Lang(ESchluessel),
        "scroll-margin-block-start" => Ziel::Lang(ESchluessel),
        "scroll-margin-inline" => Ziel::Lang(ESchluessel),
        "scroll-padding" => Ziel::Lang(ESchluessel),
        "scroll-snap-align" => Ziel::Lang(ESchluessel),
        "scroll-snap-stop" => Ziel::Lang(ESchluessel),
        "scroll-snap-type" => Ziel::Lang(ESchluessel),
        "scrollbar-gutter" => Ziel::Lang(ESchluessel),
        "stop-color" => Ziel::Lang(ESchluessel),
        "stop-opacity" => Ziel::Lang(ESchluessel),
        "text-anchor" => Ziel::Lang(ESchluessel),
        "text-emphasis" => Ziel::Lang(ESchluessel),
        "text-emphasis-color" => Ziel::Lang(ESchluessel),
        "text-emphasis-style" => Ziel::Lang(ESchluessel),
        "text-spacing-trim" => Ziel::Lang(ESchluessel),
        "transform-box" => Ziel::Lang(ESchluessel),
        "translate" => Ziel::Lang(ESchluessel),
        "vector-effect" => Ziel::Lang(ESchluessel),
        "widows" => Ziel::Lang(ESchluessel),
        "will-change" => Ziel::Lang(ESchluessel),
        // Die letzten sieben, GEMESSEN 01.10.2026 (K7c) mit
        // `examples/abdeckung --alle`: jede stand in der Liste
        // „Eigenschaft unbekannt".
        //
        // `grid` ist die Kurzform `grid-template` (CSS Grid 1 §6.1),
        // `line-clamp` gehoert zu `overflow` (CSS Overflow 3 §4.1).
        // Beide kommen in den Schluesselbehaelter: die Kaskade
        // berechnet sie nicht, und die Zeichnung gehoert nicht in
        // diese Crate.
        "stroke-linecap" => Ziel::Lang(ESchluessel),
        // `animation-direction` (CSS Animations 1 §2.4): eine
        // Animationseigenschaft, die die Kaskade nicht berechnet.
        // GEMESSEN 01.10.2026 (K7c): 1x auf den 35 echten Stylesheets.
        "animation-direction" => Ziel::Lang(ESchluessel),
        "stroke-linejoin" => Ziel::Lang(ESchluessel),
        "font-variant-ligatures" => Ziel::Lang(ESchluessel),
        "font-variant-position" => Ziel::Lang(ESchluessel),
        "font-kerning" => Ziel::Lang(ESchluessel),
        "font-optical-sizing" => Ziel::Lang(ESchluessel),
        "text-decoration-skip-ink" => Ziel::Lang(ESchluessel),
        "scroll-padding-block" => Ziel::Lang(ESchluessel),
        "scroll-padding-inline" => Ziel::Lang(ESchluessel),
        "scroll-padding-left" => Ziel::Lang(ESchluessel),
        "scroll-padding-right" => Ziel::Lang(ESchluessel),
        "scroll-padding-bottom" => Ziel::Lang(ESchluessel),
        "line-clamp" => Ziel::Lang(ESchluessel),
        // Release 13.10.2026 (Agent CSS2): `text-align-last` (CSS Text 3 §7.2),
        // gelesen in `kaskade::ausrichtung_letzte_schluessel`.
        "text-align-last" => Ziel::Lang(ESchluessel),
        "text-justify" => Ziel::Lang(ESchluessel),
        "-webkit-line-clamp" => Ziel::Lang(ESchluessel),
        "grid" => Ziel::Lang(ESchluessel),

        // ---- K7c (01.10.2026): 0 % fuer Standard-CSS ----
        //
        // GEMESSEN ueber alle 35 echten Stylesheets in tests/daten
        // (3,1 MB, `examples/abdeckung --alle`): 161 verschiedene
        // Eigenschaften wurden verworfen, davon 20 mit Hersteller-
        // praefix. Die 20 bleiben VERWORFEN — so machen es echte
        // Browser auch (sie kennen `-webkit-` nur, wo es noetigt ist),
        // und AUTONOM.txt erlaubt genau das.
        //
        // Die restlichen 141 sind jetzt aufgeteilt:
        //
        //   * 59 auf ein EIGENES typisiertes Feld (siehe `Berechnet`)
        //     — alles, was eine spaetere Stufe als Zahl oder
        //     Schluesselwort BRAUCHT: eine Zeile Abstand, eine
        //     Rasterplatzierung, eine Ueberlaufachse.
        //   * 82 auf `ESchluessel`: gelesen, typisiert gespeichert.
        //
        // WARUM DAS KEIN WEGWERFEN IST: CSS Values 3 §3 definiert den
        // berechneten Wert einer Eigenschaft, die weder Zahl noch
        // Farbe ist, als den ANGEGEBENEN Wert. Der berechnete Wert
        // von `cursor: pointer` ist `pointer`. `Berechnet::schluessel`
        // haelt genau das — mit Namen, damit 82 Eigenschaften sich
        // nicht gegenseitig ueberschreiben.
        //
        // Neu gebaute Kurzformen (die es vorher nicht gab):
        "aspect-ratio" => Ziel::Lang(ESeitenverhaeltnis),
        "block-size" => Ziel::Lang(Hoehe),
        // CSS Box Sizing 4 §2: `block` = die Spaltenrichtung,
        // `inline` = die Zeilenrichtung. In `horizontal-tb` (der
        // einzigen Schreibrichtung dieses Browsers) ist block =
        // oben/unten und inline = links/rechts — dieselbe Umrechnung
        // wie bei `margin-inline*` (K6), nur fuer den Rahmen.
        //
        // EINE Variante fuer alle sechs, entschieden ueber den
        // Namen: sechs Varianten waeren sechs Aeste in
        // `kurz_auswerten` fuer dieselbe Rechnung.
        "border-block" => Ziel::Kurz(Kurz::RahmenLogischPaar),
        "border-inline" => Ziel::Kurz(Kurz::RahmenLogischPaarInline),
        // Die Vier Ein-Seiten-Formen sind die vorhandenen
        // Seiten-Kurzformen — nur mit der LOGISCHEN Seite. In
        // `horizontal-tb`: block-start = oben, block-end = unten,
        // inline-start = links, inline-end = rechts (CSS Box Sizing
        // 4 §2, dieselbe Umrechnung wie bei `margin-inline*`, K6).
        "border-block-start" => Ziel::Kurz(Kurz::RahmenOben),
        "border-block-end" => Ziel::Kurz(Kurz::RahmenUnten),
        "border-inline-start" => Ziel::Kurz(Kurz::RahmenLinks),
        "border-inline-end" => Ziel::Kurz(Kurz::RahmenRechts),
        "border-block-end-width" => Ziel::Lang(RahmenUntenBreite),
        "border-block-start-width" => Ziel::Lang(RahmenObenBreite),
        // Release 08.10.2026: Farbe und Stil der logischen Seiten (in
        // `horizontal-tb` wie oben). GEMESSEN vorher: „Eigenschaft
        // unbekannt: border-block-start-color" auf wikipedia.
        "border-block-start-color" => Ziel::Lang(RahmenObenFarbe),
        "border-block-end-color" => Ziel::Lang(RahmenUntenFarbe),
        "border-inline-start-color" => Ziel::Lang(RahmenLinksFarbe),
        "border-inline-end-color" => Ziel::Lang(RahmenRechtsFarbe),
        "border-block-start-style" => Ziel::Lang(RahmenObenStil),
        "border-block-end-style" => Ziel::Lang(RahmenUntenStil),
        "border-inline-start-style" => Ziel::Lang(RahmenLinksStil),
        "border-inline-end-style" => Ziel::Lang(RahmenRechtsStil),
        "border-collapse" => Ziel::Lang(ETabellenKanten),
        "border-end-end-radius" => Ziel::Lang(ERadiusUntenRechts),
        "border-end-start-radius" => Ziel::Lang(ERadiusUntenLinks),
        "border-inline-end-width" => Ziel::Lang(RahmenRechtsBreite),
        "border-inline-start-width" => Ziel::Lang(RahmenLinksBreite),
        "border-start-end-radius" => Ziel::Lang(ERadiusObenRechts),
        "border-start-start-radius" => Ziel::Lang(ERadiusObenLinks),
        "column-gap" => Ziel::Lang(ELueckeSpalte),
        // Release 10.10.2026: `flex-flow` ist `flex-direction` + `flex-wrap`
        // (CSS Flexbox 1 §5.3), NICHT `flex`. GEMESSEN vorher (Web-Platform-
        // Tests, `css-flexbox/gap-011`): `flex-flow: row wrap` wurde als
        // `flex: row wrap` gelesen, war ungueltig, und nichts brach um.
        "flex-flow" => Ziel::Kurz(Kurz::FlexFluss),
        // L4 (02.10.2026 21:5x): eigene Ziele. GEMESSENER BEFUND, was
        // vorher hier stand: `EGitterAutomatik` — zusammen mit
        // `grid-auto-flow` und `grid-auto-rows`. `gitter_automatik: String`
        // kann nur einen der drei Werte tragen (GEMESSEN in
        // `tests/position.rs`, `l4_grid_auto_teilt_sich_ein_feld_mit_dem_
        // fluss`: `grid-auto-flow:column; grid-auto-columns:200px` laesst
        // genau einen im Feld).
        "grid-auto-columns" => Ziel::Lang(ERasterAutoSpalten),
        "grid-auto-flow" => Ziel::Lang(EGitterAutomatik),
        // Release 11.10.2026: Masonry (CSS Grid 3) — Richtung der Spuren und
        // die Toleranz bei der Wahl der kuerzesten.
        "grid-lanes-direction" => Ziel::Lang(ESchluessel),
        "flow-tolerance" => Ziel::Lang(ESchluessel),
        "grid-lanes-pack" => Ziel::Lang(ESchluessel),
        // L4 (02.10.2026 21:5x): eigenes Ziel, siehe
        // `grid-auto-columns`.
        "grid-auto-rows" => Ziel::Lang(ERasterAutoZeilen),
        "grid-column-end" => Ziel::Lang(EGitterSpalteEnde),
        "grid-column-gap" => Ziel::Lang(ELueckeSpalte),
        "grid-column-start" => Ziel::Lang(EGitterSpalteStart),
        // Release 11.10.2026: `grid-gap` ist die alte Kurzform von `gap`
        // (CSS Grid 1 §10.1, Alias) — beide Achsen, nicht nur die Spalten.
        "grid-gap" => Ziel::Kurz(Kurz::Luecke),
        "grid-row-end" => Ziel::Lang(EGitterZeileEnde),
        "grid-row-gap" => Ziel::Lang(ELueckeZeile),
        "grid-row-start" => Ziel::Lang(EGitterZeileStart),
        "grid-template" => Ziel::Lang(ERaster),
        "grid-template-areas" => Ziel::Lang(EGitterGebiete),
        "grid-template-columns" => Ziel::Lang(ERaster),
        // L4 (02.10.2026 18:2x): `grid-template-rows` hat einen eigenen
        // Zieltyp. GEMESSENER BEFUND, was vorher hier stand: `ERaster`,
        // und das ist ein Alias fuer `RasterSpalten` — die Zeilen
        // landeten in den SPALTENbreiten (GEMESSEN:
        // `grid-template-rows: 200px 200px` ergab
        // `[Fest(200), Fest(200)]` als Spalten).
        "grid-template-rows" => Ziel::Lang(ERasterZeilen),
        // Release 15.10.2026 (Agent CSS2): `hyphens` is its own property (it
        // was read as `line-break`, so `hyphens: auto` reset `line-break:
        // anywhere`); kept verbatim, inherited (see `kaskade.rs`).
        "hyphens" => Ziel::Lang(ESchluessel),
        "hyphenate-character" => Ziel::Lang(ESchluessel),
        "inline-size" => Ziel::Lang(Breite),
        // CSS Positioned Layout 3, §4.1: `inset` ist die Kurzform aus
        // `top`/`right`/`bottom`/`left` mit derselben Auslassregel wie
        // bei `margin` — es ist also `Kurz::Marge` auf den
        // Versatz-Eigenschaften.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): es gibt auf wikipedia
        // 19x `inset`, davon 9 `inset-inline-start` — die Position
        // eines Kastens geht an vier Seiten verloren.
        "inset" => Ziel::Kurz(Kurz::Inset),
        "letter-spacing" => Ziel::Lang(ELetterSpacing),
        "line-break" => Ziel::Lang(EZeilenTrennung),
        // CSS Lists 3 §3.3: `none` oder ein `url(...)`. Als
        // Schluessel gespeichert; das Bild holt eine andere Rolle.
        "list-style-image" => Ziel::Lang(ESchluessel),
        "max-block-size" => Ziel::Lang(MaxHoehe),
        "max-inline-size" => Ziel::Lang(MaxBreite),
        "min-block-size" => Ziel::Lang(MinHoehe),
        "min-inline-size" => Ziel::Lang(MinBreite),
        "object-fit" => Ziel::Lang(EObjektPassung),
        "order" => Ziel::Lang(EOrdnung),
        "overflow-wrap" => Ziel::Lang(EWortUmbruch),
        "overflow-x" => Ziel::Lang(EOverflowX),
        "overflow-y" => Ziel::Lang(EOverflowY),
        "row-gap" => Ziel::Lang(ELueckeZeile),
        "tab-size" => Ziel::Lang(ETabulator),
        "table-layout" => Ziel::Lang(ETabellenLage),
        // CSS Text Decoration 4 §3.2/§3.3: Farbe und Staerke der
        // Unterstreichung. Sie sind Teile der Kurzworte
        // `text-decoration-line` (das es schon gab) und bekommen
        // darum den Schluesselbehaelter.
        "text-decoration-color" => Ziel::Lang(ESchluessel),
        "text-decoration-thickness" => Ziel::Lang(ESchluessel),
        "text-indent" => Ziel::Lang(ETextEinzug),
        "text-overflow" => Ziel::Lang(ETextUeberlauf),
        "text-shadow" => Ziel::Lang(ESchatten),
        "text-transform" => Ziel::Lang(ETextTransform),
        // CSS Text Decoration 4 §3.4/§3.5.
        "text-underline-offset" => Ziel::Lang(ESchluessel),
        "text-underline-position" => Ziel::Lang(ESchluessel),
        // CSS Text 4 §4.4: `wrap`, `balance`, `pretty`, `stable`. Das
        // ist KEIN `text-transform` — dort stehen `uppercase` und Co.
        // `balance` stand 6x auf den echten Seiten und fiel durch,
        // weil `ETextTransform` es nicht kennt (GEMESSEN 01.10.2026).
        "text-wrap" => Ziel::Lang(ESchluessel),
        // Release 15.10.2026 (Agent CSS2): the longhand of the wrap style
        // (CSS Text 4 §6.2), kept verbatim like `text-wrap` (it was dropped,
        // so `text-wrap-style: stable` could not stop an inherited balance).
        "text-wrap-style" => Ziel::Lang(ESchluessel),
        "word-break" => Ziel::Lang(EWortTrennung),
        "word-spacing" => Ziel::Lang(EWortabstand),
        "word-wrap" => Ziel::Lang(EWortUmbruch),
        "align-content" => Ziel::Lang(ESchluessel),
        "align-left" => Ziel::Lang(ESchluessel),
        // L4 (09:5x): `align-self` hat ein eigenes Ziel bekommen. Es lag
        // im Schluessel-Behaelter, und der Rasterizer liest den nicht.
        "align-self" => Ziel::Lang(EAusrichtungSelbst),
        "anchor-name" => Ziel::Lang(ESchluessel),
        "animation" => Ziel::Lang(ESchluessel),
        "animation-delay" => Ziel::Lang(ESchluessel),
        "animation-duration" => Ziel::Lang(ESchluessel),
        "animation-fill-mode" => Ziel::Lang(ESchluessel),
        "animation-iteration-count" => Ziel::Lang(ESchluessel),
        "animation-name" => Ziel::Lang(ESchluessel),
        "animation-play-state" => Ziel::Lang(ESchluessel),
        "animation-timeline" => Ziel::Lang(ESchluessel),
        "animation-timing-function" => Ziel::Lang(ESchluessel),
        "appearance" => Ziel::Lang(ESchluessel),
        "backdrop-filter" => Ziel::Lang(ESchluessel),
        "backface-visibility" => Ziel::Lang(ESchluessel),
        "background-clip" => Ziel::Lang(ESchluessel),
        // Release 13.10.2026 (Agent CSS2): `background-origin` (§3.7), gelesen
        // in `narsil_page::bild_geo_mit_rand`.
        "background-origin" => Ziel::Lang(ESchluessel),
        "background-attachment" => Ziel::Lang(ESchluessel),
        "-webkit-background-clip" => Ziel::Lang(ESchluessel),
        "box-decoration-break" => Ziel::Lang(ESchluessel),
        "break-inside" => Ziel::Lang(ESchluessel),
        "caption-side" => Ziel::Lang(ESchluessel),
        // Release 11.10.2026 (Agent CSS2): CSS 2.1 §17.6.1.1.
        "empty-cells" => Ziel::Lang(ESchluessel),
        "clip" => Ziel::Lang(ESchluessel),
        // Release 11.10.2026: `flex-line-count` (CSS Flexbox 2), ausgewertet
        // in `gitter::schluessel_setzen`.
        "flex-line-count" => Ziel::Lang(ESchluessel),
        "clip-path" => Ziel::Lang(ESchluessel),
        "color-scheme" => Ziel::Lang(ESchluessel),
        "contain" => Ziel::Lang(ESchluessel),
        "container-type" => Ziel::Lang(ESchluessel),
        "counter-reset" => Ziel::Lang(ESchluessel),
        // Release 11.10.2026: CSS Lists 3 §4.3 und CSS Content 3 §2.3.
        "counter-set" => Ziel::Lang(ESchluessel),
        "quotes" => Ziel::Lang(ESchluessel),
        "cursor" => Ziel::Lang(ESchluessel),
        "direction" => Ziel::Lang(ESchluessel),
        // Release 14.10.2026: kept as a keyword (read in
        // `layout_plan::layout_stil`, `LayoutStil::vertical_writing`).
        "writing-mode" => Ziel::Lang(ESchluessel),
        "field-sizing" => Ziel::Lang(ESchluessel),
        "fill" => Ziel::Lang(ESchluessel),
        "fill-opacity" => Ziel::Lang(ESchluessel),
        "filter" => Ziel::Lang(ESchluessel),
        "font-stretch" => Ziel::Lang(ESchluessel),
        "font-synthesis" => Ziel::Lang(ESchluessel),
        "font-variant-numeric" => Ziel::Lang(ESchluessel),
        "font-variation-settings" => Ziel::Lang(ESchluessel),
        // L4 (03.10.2026 02:9x): `grid-area` bekommt ein eigenes Ziel.
        //
        // GEMESSENER BEFUND, was vorher hier stand: `ESchluessel`, und das
        // ist `Berechnet::schluessel: Vec<(String, String)>` — ein Rohwert
        // als Text, den niemand uebersetzt. GEMESSEN im Korpus: 184
        // Vorkommen, davon 169 ein NAME und 15 vierteilige Zahlen.
        "grid-area" => Ziel::Lang(EGitterGebiet),
        // L4 (02.10.2026 22:3x): die Kurzformen `grid-row` und
        // `grid-column` (Grid 1 §8.4) haben ihr eigenes Ziel.
        //
        // GEMESSENER BEFUND, was vorher hier stand: `ESchluessel`, und das
        // ist `Berechnet::schluessel: Vec<(String, String)>` — ein
        // Rohwert als Text. GEMESSEN in
        // `tests/position.rs::l4_span_ueber_zeilen_macht_ein_kind_hoch`:
        // `grid-row:span 2` liess `gitter_zeile_start == 0`, denn der
        // Rohwert wurde von niemandem in eine Zahl uebersetzt.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Kurzform ein eigenes Ziel
        // bekommt und nicht in `grid-row-start` umgeschrieben wird:
        // §8.4 macht die Kurzform abhaengig von der Zahl der Teile
        // (`<grid-line> [ / <grid-line> ]?`), und diese Entscheidung
        // gehoert an die Kaskade — dort steht `gitter_linie`.
        "grid-row" => Ziel::Lang(EGitterZeileStart),
        "grid-column" => Ziel::Lang(EGitterSpalteStart),
        "inset-block-start" => Ziel::Lang(Oben),
        "inset-inline" => Ziel::Lang(ESchluessel),
        "inset-inline-start" => Ziel::Lang(ESchluessel),
        "interpolate-size" => Ziel::Lang(ESchluessel),
        "isolation" => Ziel::Lang(ESchluessel),
        // L4 (11:5x): `justify-items` hat ein eigenes Ziel bekommen — es
        // war `ESchluessel` (wie `justify-self` vor 09:5x), und den liest
        // die Raster-Platzierung nicht.
        "justify-items" => Ziel::Lang(EJustifyAusrichtung),
        // L4 (09:5x): dasselbe fuer `justify-self` (§6.2, §10.3).
        "justify-self" => Ziel::Lang(EAusrichtungSelbstWaagerecht),
        "mask-composite" => Ziel::Lang(ESchluessel),
        "object-position" => Ziel::Lang(ESchluessel),
        "offset" => Ziel::Lang(ESchluessel),
        "overflow-anchor" => Ziel::Lang(ESchluessel),
        "overflow-clip-margin" => Ziel::Lang(ESchluessel),
        "page-break-inside" => Ziel::Lang(ESchluessel),
        "place-content" => Ziel::Lang(ESchluessel),
        // L4 (13:0x): die beiden Kurzformen mit vorhandenen Langformen.
        // `place-content` bleibt `ESchluessel`, weil `align-content`
        // (§8.4/§12) als eigene Eigenschaft noch fehlt — eine Kurzform
        // auf zwei halbe Eigenschaften waere geraten.
        "place-items" => Ziel::Kurz(Kurz::PlaceItems),
        "place-self" => Ziel::Kurz(Kurz::PlaceSelf),
        "pointer-events" => Ziel::Lang(ESchluessel),
        "print-color-adjust" => Ziel::Lang(ESchluessel),
        "resize" => Ziel::Lang(ESchluessel),
        "scale" => Ziel::Lang(ESchluessel),
        "scroll-behavior" => Ziel::Lang(ESchluessel),
        "scroll-margin-top" => Ziel::Lang(ESchluessel),
        "scroll-padding-top" => Ziel::Lang(ESchluessel),
        "scrollbar-width" => Ziel::Lang(ESchluessel),
        "shape-rendering" => Ziel::Lang(ESchluessel),
        "stroke" => Ziel::Lang(ESchluessel),
        "stroke-dasharray" => Ziel::Lang(ESchluessel),
        "stroke-dashoffset" => Ziel::Lang(ESchluessel),
        "stroke-opacity" => Ziel::Lang(ESchluessel),
        "stroke-width" => Ziel::Lang(ESchluessel),
        "text-rendering" => Ziel::Lang(ESchluessel),
        "text-size-adjust" => Ziel::Lang(ESchluessel),
        "touch-action" => Ziel::Lang(ESchluessel),
        "transform" => Ziel::Lang(ESchluessel),
        "transform-origin" => Ziel::Lang(ESchluessel),
        "transform-style" => Ziel::Lang(ESchluessel),
        "transition" => Ziel::Lang(ESchluessel),
        "transition-delay" => Ziel::Lang(ESchluessel),
        "transition-duration" => Ziel::Lang(ESchluessel),
        "transition-property" => Ziel::Lang(ESchluessel),
        "transition-timing-function" => Ziel::Lang(ESchluessel),
        "unicode-bidi" => Ziel::Lang(ESchluessel),
        "user-select" => Ziel::Lang(ESchluessel),
        "margin-top" => Ziel::Lang(MargeOben),
        "margin-right" => Ziel::Lang(MargeRechts),
        "margin-bottom" => Ziel::Lang(MargeUnten),
        "margin-left" => Ziel::Lang(MargeLinks),
        "padding-top" => Ziel::Lang(FuellungOben),
        "padding-right" => Ziel::Lang(FuellungRechts),
        "padding-bottom" => Ziel::Lang(FuellungUnten),
        "padding-left" => Ziel::Lang(FuellungLinks),
        "border-top-width" => Ziel::Lang(RahmenObenBreite),
        "border-right-width" => Ziel::Lang(RahmenRechtsBreite),
        "border-bottom-width" => Ziel::Lang(RahmenUntenBreite),
        "border-left-width" => Ziel::Lang(RahmenLinksBreite),
        "border-top-style" => Ziel::Lang(RahmenObenStil),
        "border-right-style" => Ziel::Lang(RahmenRechtsStil),
        "border-bottom-style" => Ziel::Lang(RahmenUntenStil),
        "border-left-style" => Ziel::Lang(RahmenLinksStil),
        "border-top-color" => Ziel::Lang(RahmenObenFarbe),
        "border-right-color" => Ziel::Lang(RahmenRechtsFarbe),
        "border-bottom-color" => Ziel::Lang(RahmenUntenFarbe),
        "border-left-color" => Ziel::Lang(RahmenLinksFarbe),
        "width" => Ziel::Lang(Breite),
        "max-width" => Ziel::Lang(MaxBreite),
        "min-width" => Ziel::Lang(MinBreite),
        "height" => Ziel::Lang(Hoehe),
        // K1, 01.10.2026: `min-height`/`max-height` gab es nicht, obwohl
        // `@media (min-height: …)` sie schon auswertete. `min-height:
        // 100vh` steht auf jeder Seite mit klebendem Kopf.
        "min-height" => Ziel::Lang(MinHoehe),
        "max-height" => Ziel::Lang(MaxHoehe),
        "list-style-type" => Ziel::Lang(EListe),
        "visibility" => Ziel::Lang(ESicht),
        // CSS Overflow 3 §3.1: EIN Wert gilt fuer BEIDE Achsen, ZWEI
        // je eine. `overflow: auto hidden` steht 15x auf den 35 echten
        // Stylesheets und fiel vorher weg, weil hier nur EIN Wert
        // gelesen wurde — GEMESSEN 01.10.2026 mit
        // `examples/abdeckung`.
        "overflow" => Ziel::Kurz(Kurz::Overflow),
        "flex-direction" => Ziel::Lang(EFlexRichtung),
        "justify-content" => Ziel::Lang(EFlexBuendig),
        "align-items" => Ziel::Lang(EFlexAusrichtung),
        "gap" => Ziel::Kurz(Kurz::Luecke),
        "flex-grow" => Ziel::Lang(EFlexWachstum),
        "flex-shrink" => Ziel::Lang(EFlexSchrumpf),
        "flex-basis" => Ziel::Lang(EFlexBasis),
        "flex-wrap" => Ziel::Lang(EFlexUmbruch),
        "flex" => Ziel::Kurz(Kurz::Flex),
        "position" => Ziel::Lang(EPosition),
        "top" => Ziel::Lang(EOben),
        "right" => Ziel::Lang(ERechts),
        "bottom" => Ziel::Lang(EUnten),
        "left" => Ziel::Lang(ELinks),
        "z-index" => Ziel::Lang(EZIndex),
        "float" => Ziel::Lang(ESchweben),
        "clear" => Ziel::Lang(ERaeumung),
        "margin" => Ziel::Kurz(Kurz::Marge),
        "padding" => Ziel::Kurz(Kurz::Fuellung),
        "border" => Ziel::Kurz(Kurz::Rahmen),
        "border-width" => Ziel::Kurz(Kurz::RahmenBreite),
        // Die Seitenvarianten (R0 e, 30.09.2026) — siehe `Kurz::RahmenOben`.
        "border-top" => Ziel::Kurz(Kurz::RahmenOben),
        "border-right" => Ziel::Kurz(Kurz::RahmenRechts),
        "border-bottom" => Ziel::Kurz(Kurz::RahmenUnten),
        "border-left" => Ziel::Kurz(Kurz::RahmenLinks),
        "border-style" => Ziel::Kurz(Kurz::RahmenStil),
        "border-color" => Ziel::Kurz(Kurz::RahmenFarbe),
        "font" => Ziel::Kurz(Kurz::Schrift),
        "background" => Ziel::Kurz(Kurz::Hintergrund),
        // K1, 01.10.2026: `background-image` gab es nicht. Ohne diese
        // Zeile fiel jeder Verlauf schon beim Lesen weg.
        "background-image" => Ziel::Lang(EHintergrundBild),
        "list-style" => Ziel::Kurz(Kurz::Listenstil),
        _ => {
            // KOMPRESS D1 / C1 (02.10.2026 03:0x): DER HERSTELLER-PRAEFIX.
            //
            // GEMESSENE LUECKE, `examples/abdeckung.rs` ueber die 35
            // echten Stylesheets: **219 verworfene Deklarationen, davon
            // 216 mit einem Hersteller-Praefix.** Die groessten:
            //
            //   -webkit-text-decoration      75
            //   -webkit-user-select           40
            //   -webkit-font-smoothing        13
            //   -webkit-text-fill-color       11
            //   -o-object-fit                 10
            //   -webkit-text-decoration-color   9
            //   -moz-osx-font-smoothing        9
            //   -webkit-overflow-scrolling      7
            //   -webkit-tap-highlight-color     6
            //   -moz-column-gap                6
            //
            // GEMESSENE ENTSCHEIDUNG, warum der Rueckfall hier und nicht
            // in den K6-Zaehler: Der Zaehler soll LUECKEN zeigen. Eine
            // Luecke, die man in drei Zeilen schliessen kann, gehoert
            // geschlossen, nicht gezaehlt.
            //
            // GEMESSENER WIRKUNGSNACHWEIS, und der ist der Grund fuer
            // diesen Punkt: `text-decoration: underline` ergibt
            // `Berechnet::dekoration = 1`, `-webkit-text-decoration:
            // underline` ergibt **0** und landet nicht einmal im
            // Schluessel-Eimer. Es geht also nicht darum, einen
            // Zaehler zu schoenen — 75 Angaben auf echten Seiten
            // schalten eine modellierte Eigenschaft nicht.
            //
            // GEMESSENE GRENZE, und sie ist die Regel selbst: Ein
            // Praefix ohne Standardgegenstueck wird weiterhin
            // verworfen. `-webkit-font-smoothing` (13),
            // `-webkit-text-fill-color` (11) und
            // `-moz-osx-font-smoothing` (9) bleiben draussen, weil es
            // kein `font-smoothing` in der Liste gibt. Das ist
            // gemessen und nicht geraten — siehe `ohne_hersteller`.
            return match ohne_hersteller(name).and_then(ziel_fuer) {
                Some(z) => Some(z),
                None => None,
            };
        }
    })
}

/// Langformen einer Kurzform (fuer inherit/initial/unset).
fn kurz_langformen(k: Kurz) -> Vec<Eigenschaft> {
    match k {
        Kurz::Marge => vec![MargeOben, MargeRechts, MargeUnten, MargeLinks],
        Kurz::Fuellung => vec![FuellungOben, FuellungRechts, FuellungUnten, FuellungLinks],
        // K7c: die logischen Paar-Formen. Es gibt keine eigenen
        // Langformen — sie laufen auf dieselben wie `border`.
        Kurz::RahmenLogischPaar | Kurz::RahmenLogischPaarInline => vec![
            RahmenObenBreite,
            RahmenRechtsBreite,
            RahmenUntenBreite,
            RahmenLinksBreite,
        ],
        Kurz::Inset => vec![Oben, Rechts, Unten, Links],
        // CSS Overflow 3 §3.1: die Kurzform IST die Kombination der
        // beiden Achsen.
        Kurz::Overflow => vec![EOverflowX, EOverflowY],
        Kurz::Luecke => vec![ELuecke, ELueckeZeile, ELueckeSpalte],
        // §7.1: `place-items` ist genau die Kombination der beiden
        // Achsen — dieselbe Form wie `overflow` darüber.
        Kurz::PlaceItems => vec![EFlexAusrichtung, EJustifyAusrichtung],
        Kurz::PlaceSelf => vec![EAusrichtungSelbst, EAusrichtungSelbstWaagerecht],
        // §17.6: die Kurzform IST die Kombination der beiden Achsen.
        Kurz::RandAbstand => vec![ERandAbstandW, ERandAbstandS],
        Kurz::Rahmen => vec![
            RahmenObenBreite,
            RahmenRechtsBreite,
            RahmenUntenBreite,
            RahmenLinksBreite,
            RahmenObenStil,
            RahmenRechtsStil,
            RahmenUntenStil,
            RahmenLinksStil,
            RahmenObenFarbe,
            RahmenRechtsFarbe,
            RahmenUntenFarbe,
            RahmenLinksFarbe,
        ],
        Kurz::RahmenOben => vec![RahmenObenBreite, RahmenObenStil, RahmenObenFarbe],
        Kurz::RahmenRechts => vec![RahmenRechtsBreite, RahmenRechtsStil, RahmenRechtsFarbe],
        Kurz::RahmenUnten => vec![RahmenUntenBreite, RahmenUntenStil, RahmenUntenFarbe],
        Kurz::RahmenLinks => vec![RahmenLinksBreite, RahmenLinksStil, RahmenLinksFarbe],
        Kurz::RahmenBreite => {
            vec![
                RahmenObenBreite,
                RahmenRechtsBreite,
                RahmenUntenBreite,
                RahmenLinksBreite,
            ]
        }
        Kurz::RahmenStil => {
            vec![
                RahmenObenStil,
                RahmenRechtsStil,
                RahmenUntenStil,
                RahmenLinksStil,
            ]
        }
        Kurz::RahmenFarbe => {
            vec![
                RahmenObenFarbe,
                RahmenRechtsFarbe,
                RahmenUntenFarbe,
                RahmenLinksFarbe,
            ]
        }
        Kurz::Schrift => {
            vec![
                EStil,
                Schriftgewicht,
                Schriftgroesse,
                Zeilenhoehe,
                Schriftfamilie,
            ]
        }
        // Release 10.10.2026: ALLE Langformen (CSS Backgrounds 3 §3.10) —
        // `background: inherit` erbte vorher nur Farbe und Bild, `repeat-x`
        // und die Position des Elternteils fehlten (WPT `background-328`).
        Kurz::Hintergrund => {
            vec![EHintergrund, EHintergrundBild, EBildGroesse, EBildPosition, EBildWiederholen, EHintergrundMischung]
        }
        // GEMESSENE KORREKTUR (K17b): Die `inline`-Paare sind neutral.
        // `margin-inline: 0 0.25em` heisst „Anfangsseite 0, Endseite
        // 0.25em" — in `rtl` ist das eine ANDERE Seite als in `ltr`,
        // und vorher stand hier fest `(links, rechts)`.
        Kurz::MargeInline => vec![EMargeInnenStart, EMargeInnenEnde],
        Kurz::MargeBlock => vec![MargeOben, MargeUnten],
        Kurz::FuellungInline => vec![EFuellungInnenStart, EFuellungInnenEnde],
        Kurz::FuellungBlock => vec![FuellungOben, FuellungUnten],
        Kurz::Outline => vec![EOutlineBreite, EOutlineFarbe],
        // Die Reihenfolge ist die aus §5.4: oben-links, oben-rechts,
        // unten-rechts, unten-links. Mit EINER, ZWEI, DREI oder VIER
        // Werten (CSS Backgrounds 3 §5.4 gibt alle vier Formen an).
        Kurz::Radius => {
            vec![
                ERadiusObenLinks,
                ERadiusObenRechts,
                ERadiusUntenRechts,
                ERadiusUntenLinks,
            ]
        }
        Kurz::Listenstil => vec![EListe],
        Kurz::Flex => vec![EFlexWachstum, EFlexSchrumpf, EFlexBasis],
        Kurz::FlexFluss => vec![EFlexRichtung, EFlexUmbruch],
    }
}

/// Wert einer Langform pruefen (kleingeschrieben erwartet). None = weg.
/// crate-intern auch fuer var()-substituierte Texte (C10, W6).
/// Release 10.10.2026: `display` in der Zwei-Wort-Form (CSS Display 3 §2:
/// aussen `block`/`inline`/`run-in`, innen `flow`/`flow-root`/`flex`/
/// `grid`/`table`/`ruby`, dazu `list-item`) und die Einzelwoerter, die wir
/// auf eine Grundform abbilden (`run-in` -> Block, `ruby*` -> Inline,
/// `grid-lanes` -> Raster). GEMESSEN vorher (WPT `display-valid.html`):
/// `inline flex` & Co. fielen als ungueltig weg.
fn anzeige_mehrteilig(klein: &str) -> Option<&'static str> {
    match klein {
        "run-in" => return Some("block"),
        "ruby" | "ruby-base" | "ruby-text" | "ruby-base-container" | "ruby-text-container" => return Some("inline"),
        "grid-lanes" => return Some("grid-lanes"),
        "inline-grid-lanes" => return Some("inline-grid-lanes"),
        // Release 11.10.2026: `math` ist innen `flow`, wenn das Element kein
        // MathML ist (MathML Core §3.1.1) — und aussen inline.
        "math" => return Some("inline"),
        _ => {}
    }
    let (mut aussen, mut innen, mut liste) = (None, None, false);
    let mut anzahl = 0;
    for w in klein.split_ascii_whitespace() {
        anzahl += 1;
        match w {
            "block" | "inline" | "run-in" if aussen.is_none() => aussen = Some(w),
            "flow" | "flow-root" | "flex" | "grid" | "table" | "ruby" | "grid-lanes" if innen.is_none() => innen = Some(w),
            "math" if innen.is_none() => innen = Some("flow"),
            "list-item" if !liste => liste = true,
            _ => return None,
        }
    }
    if anzahl < 2 {
        return None;
    }
    let inline = aussen == Some("inline");
    if liste {
        return matches!(innen, None | Some("flow") | Some("flow-root")).then_some("list-item");
    }
    Some(match (inline, innen.unwrap_or("flow")) {
        (_, "ruby") => "inline",
        (false, "flow") => "block",
        (true, "flow") => "inline",
        (false, "flow-root") => "flow-root",
        (true, "flow-root") => "inline-block",
        (false, "flex") => "flex",
        (true, "flex") => "inline-flex",
        (false, "grid") => "grid",
        (true, "grid") => "inline-grid",
        (false, "grid-lanes") => "grid-lanes",
        (true, "grid-lanes") => "inline-grid-lanes",
        (false, _) => "table",
        (true, _) => "inline-table",
    })
}

/// Release 10.10.2026: Ausrichtungswerte auf die Grundform (CSS Box
/// Alignment 3 §4.4): `safe`/`unsafe` regeln nur den Ueberlauf, `flow-start`/
/// `flow-end` sind `start`/`end` in Flussrichtung, `legacy` ist `normal`
/// (mit einem physischen Wort: dieses Wort). GEMESSEN vorher (WPT
/// `parse-justify-*`/`align-*`): `safe center`, `normal`, `flow-start`
/// fielen weg.
fn ausrichtung_vereinfachen(klein: &str) -> &str {
    let mut k = klein.trim();
    for p in ["unsafe ", "safe "] {
        if let Some(r) = k.strip_prefix(p) {
            k = r.trim_start();
            break;
        }
    }
    match k {
        "flow-start" => "start",
        "flow-end" => "end",
        "legacy" => "normal",
        "legacy left" | "left legacy" => "left",
        "legacy right" | "right legacy" => "right",
        "legacy center" | "center legacy" => "center",
        _ => k,
    }
}

pub(crate) fn lang_wert(e: Eigenschaft, klein: &str) -> Option<Wert> {
    // Release 11.10.2026: die Rastereigenschaften syntaktisch pruefen (CSS
    // Grid 1 §7/§8) — eine ungueltige Angabe wird verworfen, statt als Text
    // durchzugehen. GEMESSEN vorher: WPT `css-grid/parsing/*-invalid`.
    if !crate::gitter::gueltig_fuer(e, klein) {
        return None;
    }
    match e {
        // K7: `inline-flex` und `table` waren hier nicht genannt und
        // wurden darum VERWORFEN — GEMESSEN viermal `inline-flex` und
        // dreimal `table` auf wikipedia_vector-2022.css.
        //
        // `inline-flex` ist keine Dekoration: es entscheidet, ob ein
        // Element seine Kinder als Flex-Items legt (CSS Flexbox 1 §2),
        // und es entscheidet, ob es selbst eine eigene Zeile bekommt
        // (CSS Display 3 §2.5).
        Anzeige => match klein {
            "block" | "inline" | "inline-block" | "list-item" | "flex" | "inline-flex" | "grid"
            | "inline-grid" | "table" | "inline-table" | "table-row" | "table-cell"
            | "table-row-group" | "table-header-group" | "table-footer-group" | "table-caption"
            | "table-column" | "table-column-group" | "flow-root" | "contents" | "none" => {
                Some(Wert::Wort(String::from(klein)))
            }
            // Die drei `-webkit-`-Formen, die es noch gibt. Sie sind
            // KEINE Schreibfehler, sondern der Stand von 2016/2018,
            // den Safari und Chrome parallel zur Spec weiterverwendet
            // haben.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 4x `display:
            // -webkit-box` auf den 35 echten Stylesheets. Waere das ein
            // Praefix und damit nach AUTONOM.txt erlaubt, koennte es
            // hier stehen bleiben — aber die Form ist harmlos und
            // eindeutig, und der Unterschied zu `flex` ist die
            // Ein-Achsen-Lage von 2016, die keine Zeile benutzt.
            "-webkit-box" => Some(Wert::Wort(String::from("flex"))),
            "-webkit-inline-box" => Some(Wert::Wort(String::from("inline-flex"))),
            _ => anzeige_mehrteilig(klein).map(|w| Wert::Wort(String::from(w))),
        },
        // K7: `box-sizing` (CSS Box Sizing 3, §2).
        EBoxArt => match klein {
            "content-box" | "border-box" => Some(Wert::BoxArt(match klein {
                "border-box" => BoxArt::Rand,
                _ => BoxArt::Inhalt,
            })),
            _ => None,
        },
        // K7: die Masken (CSS Masking 1, §4).
        EMaskenBild => masken_bild_aus_roh(&klein),
        EMaskenGroesse => {
            let teile = teile_wert(&klein);
            if teile.is_empty() || teile.len() > 2 {
                return None;
            }
            let g = |t: &str| -> Option<MaskGroesse> {
                match t {
                    "auto" => Some(MaskGroesse::Auto),
                    "cover" => Some(MaskGroesse::Cover),
                    "contain" => Some(MaskGroesse::Contain),
                    _ => laenge_aus(t).map(|l| MaskGroesse::Wert(l.wert)),
                }
            };
            Some(Wert::Maske(vec![maske_mit(
                String::new(),
                g(teile[0])?,
                0.0,
                0.0,
                true,
                true,
            )]))
        }
        EMaskenPosition => {
            // CSS Masking 1, §4.1: EIN bis VIER Werte in der Reihenfolge
            // oben rechts unten links, wie bei `background-position`
            // (CSS Backgrounds 3 §3.3).
            let teile = teile_wert(&klein);
            if teile.is_empty() || teile.len() > 4 {
                return None;
            }
            // Jedes Wort liefert eine x- UND eine y-Angabe; welches
            // davon zaehlt, entscheidet die Position im Wertesatz
            // (CSS Backgrounds 3 §3.3).
            let f = |t: &str| -> Option<(Option<f32>, Option<f32>)> {
                match t {
                    "left" => Some((Some(0.0), None)),
                    "right" => Some((Some(1.0), None)),
                    "top" => Some((None, Some(0.0))),
                    "bottom" => Some((None, Some(1.0))),
                    "center" => Some((Some(0.5), Some(0.5))),
                    _ => laenge_aus(t).map(|l| (Some(l.wert), None)),
                }
            };
            // CSS Masking 1 §4.1: die WERTE wie bei
            // `background-position` (CSS Backgrounds 3 §3.3) — EIN Wert
            // = x, ZWEI = x dann y, DREI und VIER entsprechend.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7): hier stand
            // `f(teile[0])` zweimal, also ging der ZWEITE Wert
            // verloren: `mask-position: top left` ergab (0,5; 0,0)
            // statt (0;0).
            let mut x: f32 = 0.5;
            let mut y: f32 = 0.5;
            for (nr, t) in teile.iter().enumerate() {
                let (nx, ny) = f(t)?;
                let auf_y = nr == 1 || nr == 2;
                if auf_y {
                    // Beim ZWEITEN Wert zaehlt, ob er eine x- oder eine
                    // y-Angabe ist: `left top` ist x=left, y=top (§3.3).
                    y = if nr == 1 {
                        nx.or(ny).unwrap_or(y)
                    } else {
                        ny.unwrap_or(y)
                    };
                } else {
                    // Beim ERSTEN und beim DRITTEN Wert zaehlt die
                    // x-Angabe; fehlt sie, ist es eine y-Angabe
                    // (`top left` = x 0, y 0).
                    x = nx.or(ny).unwrap_or(0.5);
                }
            }
            Some(Wert::Maske(vec![maske_mit(
                String::new(),
                MaskGroesse::Auto,
                x,
                y,
                true,
                true,
            )]))
        }
        // CSS Masking 1 §4.4: EIN Wert gilt fuer beide Achsen, ZWEI
        // je eine — mit Komma. `no-repeat,no-repeat` ist dasselbe wie
        // `no-repeat`, aber es steht so in der Praxis.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): 1x auf den 35 echten
        // Stylesheets.
        EMaskenWiederholen if klein.contains(',') => {
            let teile = teile_komma(&klein);
            if teile.len() != 2 {
                return None;
            }
            let eine = |t: &str| -> Option<(bool, bool)> {
                match t.trim() {
                    "repeat" => Some((true, true)),
                    "repeat-x" => Some((true, false)),
                    "repeat-y" => Some((false, true)),
                    "no-repeat" => Some((false, false)),
                    _ => None,
                }
            };
            let (x, _) = eine(teile[0])?;
            let (_, y) = eine(teile[1])?;
            return Some(Wert::Maske(vec![maske_mit(
                String::new(),
                MaskGroesse::Auto,
                0.0,
                0.0,
                x,
                y,
            )]));
        }
        EMaskenWiederholen => {
            let (x, y) = match klein {
                "repeat" => (true, true),
                "repeat-x" => (true, false),
                "repeat-y" => (false, true),
                "no-repeat" => (false, false),
                // `space` und `round` sind echt (CSS Masking 1 §4.4),
                // aber sie brauchen die Kastengroesse, die es an dieser
                // Stelle nicht gibt. Sie werden ABGELEHNT und gezaehlt,
                // nicht auf `repeat` geraten — ein geratenes `space`
                // erzeugt Nahtlose.
                "space" | "round" => {
                    verwurf(&format!("Wert ungeprueft: mask-repeat: {klein}"));
                    return None;
                }
                _ => return None,
            };
            Some(Wert::Maske(vec![maske_mit(
                String::new(),
                MaskGroesse::Auto,
                0.0,
                0.0,
                x,
                y,
            )]))
        }
        // K7: die Hintergrund-Langformen (CSS Backgrounds 3 §3.3-§3.5).
        //
        // Sie landen in EINEM `Wert::BildGeo`, weil sie alle dasselbe
        // Bild beschreiben — drei Felder wuerden den Zustand
        // „Groesse aus der einen, Position aus der anderen" erlauben.
        EBildGroesse => bild_groesse_aus(&klein),
        EBildPosition => bild_position_aus(&klein),
        EBildWiederholen => bild_wiederholen_aus(&klein),
        EFarbe | EHintergrund => {
            if klein == "currentcolor" {
                return Some(Wert::Aktuell);
            }
            farbe_aus(klein).map(Wert::Farbe)
        }
        // K1, 01.10.2026: `background-image: none` (der Anfangswert,
        // CSS Backgrounds 3, §2.1) und die Verlaufsarten. `url(...)`
        // bleibt hier bewusst weg — Bilder sind nicht Teil dieser
        // Crate, und eine Datei-URL als "Farbe" waere geraten.
        EHintergrundBild => {
            if klein == "none" {
                return Some(Wert::Wort(String::from("none")));
            }
            // K7: `url(...)` (CSS Backgrounds 3 §3.1). Vorher stand
            // hier BEWUSST nichts — Bilder sind eine andere Rolle. Das
            // war richtig fuer das BILD und falsch fuer die QUELLE:
            // die Zeichenkette wird jetzt gemerkt, damit die Kaskade
            // weiss, dass es da ein Bild gibt.
            //
            // GEMESSEN 01.10.2026: 18x auf wikipedia_vector-2022.css,
            // und alle 18 waren verloren.
            if let Some(u) = url_innern(&klein) {
                return Some(Wert::Url(u));
            }
            verlauf_aus(klein).map(Wert::Verlauf)
        }
        // `font-size` und `line-height` brauchen den `RechenCtx` (die
        // `%`-Basis ist die Schriftgroesse des Eltern, CSS Values 3
        // §5.1) — den hat `lang_wert` nicht. Beide werden deshalb in
        // der KASKADE gerechnet; hier wird nur die FORM anerkannt.
        //
        // GEMESSENER FEHLER 01.10.2026: `Zeilenhoehe` stand hier mit
        // einem `lang_wert`-Aufruf ohne Kontext — die Funktion
        // brauchte ihn, der Aufrufer hatte ihn nicht.
        // Die Groessen-Namen (CSS 2.1 §15.8) sind Woerter. Sie
        // werden als Wort durchgelassen; die UMSETZUNG in px macht die
        // Kaskade, weil sie die Groessen-Tabelle des Browsers traegt.
        //
        // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): hier stand nur
        // `laenge_aus`, also fiel `font-size: large` weg — und der
        // Test `schriftgroessen` fand 16 statt 18.
        Schriftgroesse => match laenge_aus(klein) {
            Some(l) => Some(Wert::Laenge(l)),
            None => match klein {
                "xx-small" | "x-small" | "small" | "medium" | "large" | "x-large" | "xx-large"
                | "larger" | "smaller" => Some(Wert::Wort(String::from(klein))),
                _ => None,
            },
        },
        Schriftgewicht => schriftgewicht_wert(klein),
        EStil => match klein {
            "normal" => Some(Wert::Wort(String::from("normal"))),
            "italic" | "oblique" => Some(Wert::Wort(String::from("kursiv"))),
            _ => None,
        },
        Schriftfamilie => schriftfamilie_wert(klein),
        Zeilenhoehe => {
            // `normal` ist der ANFANGSWERT (CSS 2.1 §10.5), und ein
            // Anfangswert zu verwerfen ist ein Fehler: `line-height`
            // ist GEERBT, also heisst `div{line-height:normal}` in
            // einem Block mit `p{line-height:1.4}` ausdruecklich
            // „zurueck auf normal". Verwirft man das `normal`, bleibt
            // die 1.4 stehen — das Gegenteil der Absicht.
            //
            // GEMESSENER BEFUND (K19, 01.10.2026 21:35): `abdeckung.rs`
            // meldete 2x `Wert ungeprueft: line-height: normal`. Die
            // KASKADE hatte den Fall schon (kaskade.rs:2831 rechnet
            // `Wert::Wort("normal")` zu `em * 1.2`) — nur der Parser
            // liess das Wort nie durch. Der Zweig war da und unerreichbar.
            if klein == "normal" {
                return Some(Wert::Wort(String::from("normal")));
            }
            // Faktor (nackte Zahl) oder Laenge. Die umrechnung in px
            // macht die Kaskade.
            if !klein.contains(char::is_alphabetic) && !klein.ends_with('%') {
                return zahl_aus(klein).map(|z| {
                    Wert::Laenge(Laenge {
                        wert: z,
                        einheit: Einheit::Zahl,
                    })
                });
            }
            laenge_aus(klein).map(Wert::Laenge)
        }
        // K17 (02.10.2026): `start` und `end` sind KEINE festen Seiten.
        // Sie haengen an der Schreibrichtung (CSS Writing Modes 3 §2.4),
        // und die kennt der Parser nicht — er sieht kein Element, nur
        // eine Zeichenkette.
        //
        // GEMESSENER BEFUND: hier standen sie in einer Zeile mit `left`
        // bzw. `right`, also wurde `text-align: start` zu
        // `Wort("links")`. Die Richtung war damit ENTSCHIEDEN, bevor die
        // Kaskade lief, und ein spaeteres `direction: rtl` auf demselben
        // Element konnte sie nicht mehr umdrehen. Das war der Grund,
        // warum `text-align: start` auf einer arabischen Seite
        // linksbuendig stand.
        //
        // Die beiden Woerter kommen deshalb unveraendert durch; wer sie
        // aufloest, ist die Kaskade (`loese_eigenschaft`,
        // `Eigenschaft::Textausrichtung`), weil nur sie das Element und
        // damit die Richtung kennt.
        Textausrichtung => match klein {
            "left" => Some(Wert::Wort(String::from("links"))),
            "right" => Some(Wert::Wort(String::from("rechts"))),
            "start" => Some(Wert::Wort(String::from("start"))),
            "end" => Some(Wert::Wort(String::from("end"))),
            "center" => Some(Wert::Wort(String::from("zentriert"))),
            "justify" => Some(Wert::Wort(String::from("blocksatz"))),
            // Release 10.10.2026: `justify-all` (CSS Text 3 §7.1) ist
            // Blocksatz auch in der letzten Zeile — hier wie `justify`;
            // `match-parent` erbt die Ausrichtung des Eltern (wie `start`
            // bei gleicher Richtung).
            "justify-all" => Some(Wert::Wort(String::from("blocksatz-alle"))),
            // Release 15.10.2026 (Agent CSS2): `match-parent` is the
            // parent's value with start/end resolved by the PARENT's
            // direction (CSS Text 3 §7.1); handled in the cascade.
            "match-parent" | "-webkit-match-parent" => Some(Wert::Wort(String::from("match-parent"))),
            _ => None,
        },
        Textdekoration => textdekoration_wert(klein),
        // CSS Text 3 Paragraf 3.1 / CSS Text 4 Paragraf 4.4.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): `break-spaces` stand in
        // keinem Zweig und fiel weg (2x auf den 35 echten
        // Stylesheets), ebenso `balance`, `pretty` und `wrap`.
        Weissraum => match klein {
            "break-spaces" | "wrap" | "balance" | "pretty" | "stable" | "break-word" => {
                Some(Wert::Wort(String::from(klein)))
            }
            "normal" => Some(Wert::Wort(String::from("normal"))),
            "pre" => Some(Wert::Wort(String::from("pre"))),
            "nowrap" => Some(Wert::Wort(String::from("nowrap"))),
            "pre-wrap" => Some(Wert::Wort(String::from("pre-wrap"))),
            "pre-line" => Some(Wert::Wort(String::from("pre-line"))),
            _ => None,
        },
        // ---- K6: WERTE DER NEUEN EIGENSCHAFTEN ----
        ERadiusObenLinks | ERadiusObenRechts | ERadiusUntenLinks | ERadiusUntenRechts => {
            // `calc()`/`var()` zuerst: der Wert hat Leer um das
            // Rechenzeichen herum, und `teile_wert` wuerde ihn in drei
            // Stuecke zerlegen und die Angabe verwerfen.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 3x `border-radius:
            // calc(var(--borderradius-medium) - var(--segmentedcontrol-
            // ...))` auf den 35 echten Stylesheets.
            if enthaelt_var(klein) || enthaelt_mathe(klein) {
                return Some(spaet());
            }
            // CSS Backgrounds 3, §5.4: eine Laenge ODER zwei mit
            // Schraegstrich (`10px / 20px`). Die Form gehoert dem
            // Rasterizer; die Kaskade merkt sich die ERSTE Laenge und
            // sagt das im Kommentar. Ein zweiter Wert, der still
            // verschwindet, waere eine erfundene Angabe — deshalb
            // wird er hier NICHT angenommen.
            let erst = teile_wert(klein);
            if erst.is_empty() || erst.len() > 2 {
                return None;
            }
            // Release 10.10.2026: die elliptische Ecke (`30px 40%`) — wie in
            // `Kurz::Radius` der KLEINERE Radius bei gleicher Einheit, sonst
            // der waagerechte (vorher fiel die Angabe ganz weg, WPT
            // `border-radius-valid`).
            let h = laenge_aus(erst[0])?;
            if erst.len() == 2 {
                let v = laenge_aus(erst[1])?;
                // Release 15.10.2026 (Agent F): the vertical radius is kept
                // (the rasterizer draws elliptical corners).
                if v != h {
                    return Some(Wert::Radius(h, v));
                }
            }
            Some(Wert::Laenge(h))
        }
        EDeckkraft => {
            // CSS Color 4, §3.2: Zahl 0-1 ODER Prozent.
            //
            // GEMESSENER FEHLER 01.10.2026 (K6): zuerst stand hier
            // `laenge_aus(klein)` — und `laenge_aus` nimmt eine
            // nackte Zahl NUR fuer `0` an (die Regel fuer Laengen).
            // `opacity: 0.5` kam darum nicht an und die Deckkraft
            // blieb 1. Der Prozentzweig wurde nie erreicht.
            //
            // Deshalb: erst die Zahl, dann der Anteil. Und eine
            // Prozentangabe wird zu derselben Zahl.
            if klein.ends_with('%') {
                let p = prozent_01(klein)?;
                return Some(Wert::Laenge(Laenge {
                    wert: p,
                    einheit: Einheit::Zahl,
                }));
            }
            let z = zahl_aus(klein)?;
            Some(Wert::Laenge(Laenge {
                wert: z,
                einheit: Einheit::Zahl,
            }))
        }
        ESchatten => schatten_aus(&klein),
        // `outline-color` wird wie `border-color` gelesen (CSS UI 4 §6.1).
        EOutlineFarbe => {
            if klein == "currentcolor" {
                return Some(Wert::Aktuell);
            }
            farbe_aus(klein).map(Wert::Farbe)
        }
        EOutlineBreite => {
            if klein == "none" || klein == "0" {
                return Some(Wert::Laenge(Laenge {
                    wert: 0.0,
                    einheit: Einheit::Px,
                }));
            }
            rahmenbreite_wert(klein).map(Wert::Laenge)
        }
        EOutlineOffset => laenge_aus(klein).map(Wert::Laenge),
        // CSS Inline 3 §6: Schluesselwoerter UND Laengen. `top: 2px`
        // ist eine gueltige Angabe (die Inline-Box wird um 2 px
        // verschoben), kein Rechenfehler.
        //
        // GEMESSENER FEHLER 01.10.2026: hier standen nur die
        // Schluesselwoerter, und `vertical-align: -1px` sowie
        // `-.075em` fielen weg — zusammen 4x auf den 35 echten
        // Stylesheets (`examples/abdeckung`).
        EVertikal => match klein {
            "baseline" | "top" | "middle" | "bottom" | "text-top" | "text-bottom" | "sub"
            | "super" => Some(Wert::Wort(String::from(klein))),
            _ => laenge_aus(klein).map(Wert::Laenge),
        },
        // ---- DIE 13 ARM, DIE AM 01.10. 04:50 MIT VERSCHWUNDEN SIND ----
        //
        // Sie waren Teil von `lang_wert` und fielen dem Editierfehler
        // zum Opfer. Sie stehen hier wieder, jeweils mit der
        // Spezifikationsstelle.
        //
        // `gap` (CSS Box Sizing 4 §2): ein Abstand, negativ ist
        // ungueltig (CSS Flexbox 1 §5.1) — `normal` = 0.
        ELuecke => match laenge_oder_mathe(klein) {
            Some(Wert::Laenge(l)) if l.wert >= 0.0 => Some(Wert::Laenge(l)),
            _ if klein == "normal" => Some(Wert::Laenge(Laenge {
                wert: 0.0,
                einheit: Einheit::Px,
            })),
            _ => None,
        },
        // `flex-grow`/`flex-shrink` (CSS Flexbox 1 §7.2/§7.3):
        // ZAHLEN ohne Einheit. `laenge_aus` nimmt eine nackte Zahl nur
        // fuer 0 an, deshalb der Sonderweg.
        EFlexWachstum | EFlexSchrumpf => {
            if !klein.contains(char::is_alphabetic) {
                if let Some(z) = zahl_aus(klein) {
                    if z.is_finite() && z >= 0.0 {
                        return Some(Wert::Laenge(Laenge {
                            wert: z,
                            einheit: Einheit::Zahl,
                        }));
                    }
                }
            }
            None
        }
        // `flex-basis` (CSS Flexbox 1 §7.2): `auto`, `content` oder eine
        // Laenge.
        EFlexBasis => {
            if klein == "auto" {
                return Some(Wert::Auto);
            }
            // Release 10.10.2026: die Messauftraege (CSS Flexbox 1 §7.2.3)
            // wie `content` (die Inhaltsgroesse). GEMESSEN vorher (WPT
            // `flex-basis-valid`): `max-content` fiel weg.
            // Release 11.10.2026: das Wort bleibt (der neue Flex-Algorithmus
            // unterscheidet sie, CSS Flexbox 1 §7.3.3).
            if matches!(klein, "content" | "min-content" | "max-content" | "fit-content") {
                return Some(Wert::Wort(String::from(klein)));
            }
            laenge_oder_mathe(klein)
        }
        // `flex-wrap` (CSS Flexbox 1 §5.4).
        EFlexUmbruch => match klein {
            "nowrap" => Some(Wert::Wort(String::from("nowrap"))),
            "wrap" => Some(Wert::Wort(String::from("wrap"))),
            "wrap-reverse" => Some(Wert::Wort(String::from("wrap-reverse"))),
            // Release 11.10.2026: `balance` (CSS Flexbox 2: `nowrap | [wrap |
            // wrap-reverse] || balance`), kanonisch `balance` bzw.
            // `wrap-reverse balance`. GEMESSEN vorher: WPT
            // `css-flexbox/balance/*` (der Wert wurde verworfen).
            _ => {
                let (mut richtung, mut ausgleich, mut n) = (None, false, 0);
                for t in klein.split_ascii_whitespace() {
                    n += 1;
                    match t {
                        "wrap" | "wrap-reverse" if richtung.is_none() => richtung = Some(t),
                        "balance" if !ausgleich => ausgleich = true,
                        _ => return None,
                    }
                }
                if !ausgleich || n > 2 {
                    return None;
                }
                Some(Wert::Wort(String::from(if richtung == Some("wrap-reverse") { "wrap-reverse balance" } else { "balance" })))
            }
        },
        // `position` (CSS Positioned Layout 3 §2).
        //
        // `sticky` und `fixed` sind fuer die Kaskade dasselbe wie
        // `relative` — die Entscheidung faellt beim Scrollen, nicht
        // beim Rechnen. Sie stehen trotzdem als eigenes Wort im Wort,
        // damit die Rolle Layout sie unterscheiden kann.
        EPosition => match klein {
            "static" | "relative" | "absolute" | "fixed" | "sticky" => {
                Some(Wert::Wort(String::from(klein)))
            }
            _ => None,
        },
        // `top`/`right`/`bottom`/`left` (CSS Positioned Layout 3 §4).
        EOben | ERechts | EUnten | ELinks => {
            if klein == "auto" {
                return Some(Wert::Auto);
            }
            // `anchor(<name>)` (CSS Anchor Positioning §3.1): der Rand
            // haengt an einem Anker, den der Rasterizer kennt — das
            // ist genau dessen Aufgabe.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 2x `left:
            // anchor(left)` auf den 35 echten Stylesheets.
            //
            // Es wird als Wort DURCHGEREICHT, nicht als Laenge erfunden:
            // `Wert::Wort` traegt den Namen, und der Rasterizer
            // entscheidet. Eine Zahl zu behaupten waere geraten.
            if klein.starts_with("anchor(") {
                return Some(Wert::Wort(String::from(klein)));
            }
            laenge_oder_mathe(klein)
        }
        // `z-index` (CSS Positioned Layout 3 §5): eine GANZZAHL oder
        // `auto`. Eine Zahl mit Einheit ist ungueltig.
        EZIndex => {
            if klein == "auto" {
                return Some(Wert::Auto);
            }
            if klein.contains(char::is_alphabetic) {
                return None;
            }
            zahl_aus(klein).map(|z| {
                Wert::Laenge(Laenge {
                    wert: z.clamp(-2147483648.0, 2147483647.0),
                    einheit: Einheit::Zahl,
                })
            })
        }
        // `float` (CSS 2.1 §9.5) und `clear` (§9.5.2).
        //
        // WICHTIG: die Kaskade erwartet die KANONISCHEN deutschen
        // Woerter (`links`, `rechts`, `beide`), weil die
        // Rahmenstil-Umrechnung sie benutzt. Hier stehen die
        // englischen Schluesselwoerter der Spec — CSS 2.1 §9.5.
        //
        // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): hier standen
        // die englischen Woerter, und `clear: left` lieferte `Keins`.
        // Die Tests `c14_clear_aus_der_kaskade` und
        // `c14_float_aus_der_kaskade` haben es gefangen.
        ESchweben => match klein {
            "none" => Some(Wert::Wort(String::from("keins"))),
            "left" => Some(Wert::Wort(String::from("links"))),
            "right" => Some(Wert::Wort(String::from("rechts"))),
            "inline-start" => Some(Wert::Wort(String::from("links"))),
            "inline-end" => Some(Wert::Wort(String::from("rechts"))),
            _ => None,
        },
        ERaeumung => match klein {
            "none" => Some(Wert::Wort(String::from("keins"))),
            "left" => Some(Wert::Wort(String::from("links"))),
            "right" => Some(Wert::Wort(String::from("rechts"))),
            "both" => Some(Wert::Wort(String::from("beide"))),
            "inline-start" => Some(Wert::Wort(String::from("links"))),
            "inline-end" => Some(Wert::Wort(String::from("rechts"))),
            _ => None,
        },
        // `grid-template-columns` (CSS Grid 1 §7.2).
        //
        // Der Wert geht als TEXT an die Kaskade: `repeat()` muss VOR
        // dem Ausbauen ausgewertet werden (`raster_spalten`), sonst
        // waere `repeat(10000, 1fr)` eine Liste von 10 000, die erst
        // dann auf 32 gekuerzt wird. Der Test `c15_repeat_bombe` in
        // tests/bomben.rs haelt das fest.
        //
        // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
        // Editierfehler um 04:50): dieser Arm fehlte, und
        // `grid-template-columns: repeat(32, 1fr)` lieferte 0 Spalten.
        ERaster => Some(Wert::Wort(String::from(klein))),
        // L4 (18:2x): `grid-template-rows` traegt seine Liste wie die
        // Spalten — dieselbe Syntax, dieselbe Aufloesung (§8.2/§8.3
        // nennen beide `<track-list>`).
        ERasterZeilen => Some(Wert::Wort(String::from(klein))),
        // L4 (02:9x): `grid-area` traegt seinen Namen wie eine Laenge.
        EGitterGebiet => Some(Wert::Wort(String::from(klein))),
        // L4 (21:5x): die impliziten Spuren tragen ihre Liste wie die
        // Vorlagen — dieselbe Syntax `<track-size>` (§7.3, §7.5).
        ERasterAutoZeilen => Some(Wert::Wort(String::from(klein))),
        ERasterAutoSpalten => Some(Wert::Wort(String::from(klein))),
        // ---- K7c: DIE 26 NEUEN EIGENSCHAFTEN ----
        //
        // GEMESSEN ueber 35 echte Stylesheets. Wo ein Wert eine Zahl
        // ist, wird eine Laenge gelesen; wo er ein Schluesselwort ist,
        // das Wort. `ESchluessel` nimmt ALLES als Wort an — das ist
        // die Vertretungsform aus CSS Values 3 §3 (der berechnete
        // Wert eines nicht-numerischen Werts ist der angegebene).
        ELetterSpacing | EWortabstand | ETextEinzug | ETextUeberlauf | EWortUmbruch
        | EWortTrennung | EZeilenTrennung | EGitterGebiete | EGitterVorlagen | EGitterAutomatik
        | EOrdnung | ELueckeSpalte | ELueckeZeile | EOverflowX | EOverflowY | EObjektPassung
        | ESeitenverhaeltnis | ETabellenKanten | ETabellenLage => {
            // Release 11.10.2026: `overflow-x`/`-y` nehmen genau die fuenf
            // Woerter aus CSS Overflow 3 §3.1. GEMESSEN vorher: `auto` wurde
            // hier zur Laenge 0 und damit zu `visible` — jeder Kasten mit
            // `overflow-y: auto` rollte und schnitt nicht (WPT cssom-view
            // scrollintoview-*, und auf echten Seiten jede Seitenleiste).
            if matches!(e, EOverflowX | EOverflowY) {
                return matches!(klein, "visible" | "hidden" | "clip" | "scroll" | "auto")
                    .then(|| Wert::Wort(String::from(klein)));
            }
            // `normal` heisst „unverändert"; es kommt hier als 0 an.
            if klein == "normal" || klein == "auto" {
                if matches!(
                    e,
                    ELetterSpacing
                        | EWortabstand
                        | ELueckeSpalte
                        | ELueckeZeile
                ) {
                    return Some(Wert::Laenge(Laenge {
                        wert: 0.0,
                        einheit: Einheit::Px,
                    }));
                }
                return Some(Wert::Wort(String::from(klein)));
            }
            if let Some(l) = laenge_aus(klein) {
                return Some(Wert::Laenge(l));
            }
            Some(Wert::Wort(String::from(klein)))
        }
        ETabulator => {
            // CSS Text 4 §8.2: Zahl ODER Laenge. `laenge_aus` nimmt
            // eine nackte Zahl nur fuer 0 an, deshalb der Sonderweg.
            // Release 12.10.2026 (Agent CSS2): nur NICHTNEGATIV und ohne
            // Prozent (CSS Text 3 §4.2: `<number [0,∞]> | <length [0,∞]>`).
            // GEMESSEN vorher (WPT `css-text/tab-size/tab-size-integer-002`,
            // `-length-002`, `-percent-001`): `-4`, `-1em` und `100%` galten.
            if !klein.contains(char::is_alphabetic) && !klein.contains('%') {
                if let Some(z) = zahl_aus(klein) {
                    return (z >= 0.0).then_some(Wert::Laenge(Laenge {
                        wert: z,
                        einheit: Einheit::Zahl,
                    }));
                }
            }
            laenge_aus(klein)
                .filter(|l| l.wert >= 0.0 && l.einheit != Einheit::Prozent)
                .map(Wert::Laenge)
        }
        ETextTransform => match klein {
            "none" | "uppercase" | "lowercase" | "capitalize" | "full-width" => {
                Some(Wert::Wort(String::from(klein)))
            }
            // Release 10.10.2026: die Kombinationen (CSS Text 3 §2.1:
            // hoechstens EIN Fall-Wort, dazu `full-width`/`full-size-kana`)
            // und `math-auto` — gerechnet wird das Fall-Wort (sonst `none`).
            _ => {
                if klein == "math-auto" {
                    return Some(Wert::Wort(String::from("none")));
                }
                let (mut fall, mut breit, mut kana) = (None, false, false);
                for w in klein.split_ascii_whitespace() {
                    match w {
                        "uppercase" | "lowercase" | "capitalize" if fall.is_none() => fall = Some(w),
                        "full-width" if !breit => breit = true,
                        "full-size-kana" if !kana => kana = true,
                        _ => return None,
                    }
                }
                Some(Wert::Wort(String::from(fall.unwrap_or(if breit { "full-width" } else { "none" }))))
            }
        },
        // CSS Grid 1 §8: `auto`, `span N`, `N` oder ein LINIENNAME.
        // Der Name ist Text, kein Zahlwert — er wird deshalb als Wort
        // weitergegeben und in `gitter_linie` (Kaskade) auf 0 gesetzt
        // und GEZAEHLT.
        EGitterSpalteStart | EGitterSpalteEnde | EGitterZeileStart | EGitterZeileEnde => {
            Some(Wert::Wort(String::from(klein)))
        }
        // ALLES, was in den Schluesselbehaelter geht.
        //
        // Der NAME steht hier noch nicht: `lang_wert` bekommt nur die
        // `Eigenschaft`, und alle 82 Schluessel-Eigenschaften sind
        // dieselbe Variante. `dekl_auswerten` hat den Namen und
        // fuellt ihn ein — siehe dort.
        ESchluessel => Some(Wert::Schluessel(String::new(), String::from(klein))),
        // K17b: die vier logischen Spitzen verhalten sich wie ihre
        // physischen Gegenstuecke — `auto` ist bei `margin-inline-start`
        // genauso erlaubt wie bei `margin-left`. Es sind die NEUTRALE
        // Eigenschaften, die erst die Kaskade spiegelt.
        MargeOben | MargeRechts | MargeUnten | MargeLinks | EMargeInnenStart | EMargeInnenEnde => {
            if klein == "auto" {
                return Some(Wert::Auto);
            }
            laenge_oder_mathe(klein)
        }
        FuellungOben | FuellungRechts | FuellungUnten | FuellungLinks | EFuellungInnenStart
        | EFuellungInnenEnde => {
            // padding: auto ist ungueltig (wird weggeworfen).
            // Release 12.10.2026 (Agent CSS2): ein negativer Wert ebenso
            // (CSS 2.1 §8.4: "Values for padding values cannot be
            // negative"). GEMESSEN vorher (WPT `margin-padding-clear/
            // padding-*-089`, `padding-left: -1%`): LADEFEHLER, die Seite brach
            // im Layout ab.
            match laenge_oder_mathe(klein) {
                Some(Wert::Laenge(l)) if l.wert >= 0.0 => Some(Wert::Laenge(l)),
                _ => None,
            }
        }
        // 03.10.2026 (Rolle CSS): `border-spacing` teilt den Padding-Arm, mit
        // EINEM Unterschied: §17.6 verbietet negative Werte, §8.2 nicht. Der
        // Unterschied ist hier nur ein `>= 0` — aber er ist derselbe Grund,
        // aus dem das hier nicht einfach `FuellungOben` ist.
        ERandAbstandW | ERandAbstandS => match laenge_oder_mathe(klein) {
            Some(Wert::Laenge(l)) if l.wert >= 0.0 => Some(Wert::Laenge(l)),
            _ => None,
        },
        // L4 (09:5x): `align-self`/`justify-self` teilen den Arm von
        // `EFlexAusrichtung` weiter oben — §6.6 nennt fuer alle drei
        // dieselbe Wertemenge.
        // L4 (09:5x): §6.3 — `auto` ist der ANFANGSWERT von `align-self`
        // und bei `justify-self` genauso (CSS Box Alignment 3 §4.2).  Es
        // verhaelt sich wie `stretch`, solange die Spur nicht ueberlaeuft.
        //
        // GEMESSENER FEHLER, gefunden von `k7c_keine_echte_luecke_mehr`:
        // `align-self: auto` stand in der Liste der verworfenen Werte —
        // also hat der Test die Deklaration gesehen und mein Arm sie
        // weggeworfen.  GEMESSEN im Korpus: `align-self: auto` kommt vor.
        EAusrichtungSelbst | EAusrichtungSelbstWaagerecht => match ausrichtung_vereinfachen(klein) {
            "flex-start" | "start" | "self-start" => Some(Wert::Wort(String::from("start"))),
            "left" if e == EAusrichtungSelbstWaagerecht => Some(Wert::Wort(String::from("start"))),
            "center" => Some(Wert::Wort(String::from("mitte"))),
            "flex-end" | "end" | "self-end" => Some(Wert::Wort(String::from("ende"))),
            "right" if e == EAusrichtungSelbstWaagerecht => Some(Wert::Wort(String::from("ende"))),
            // L4 (11:5x): `auto` ist ein EIGENER Wert, kein `stretch` —
            // §6.3: `auto` bedeutet „den Wert des Containers uebernehmen",
            // `stretch` heisst „selber fuellen". Vorher standen beide in
            // einer Zeile, und damit war `align-self: auto` nicht mehr von
            // `align-self: stretch` zu unterscheiden.
            "stretch" | "normal" => Some(Wert::Wort(String::from("strecke"))),
            "auto" => Some(Wert::Wort(String::from("auto"))),
            "baseline" | "first baseline" | "last baseline" => {
                Some(Wert::Wort(String::from("grundlinie")))
            }
            _ => None,
        },
        RahmenObenBreite | RahmenRechtsBreite | RahmenUntenBreite | RahmenLinksBreite => {
            rahmenbreite_wert(klein).map(Wert::Laenge)
        }
        RahmenObenStil | RahmenRechtsStil | RahmenUntenStil | RahmenLinksStil => {
            rahmenstil_wert(klein).map(|s| Wert::Wort(String::from(s)))
        }
        RahmenObenFarbe | RahmenRechtsFarbe | RahmenUntenFarbe | RahmenLinksFarbe => {
            if klein == "currentcolor" {
                return Some(Wert::Aktuell);
            }
            farbe_aus(klein).map(Wert::Farbe)
        }
        // `min-height: auto` ist gueltig (CSS Sizing 3, §4.1) und
        // heisst "keine Untergrenze" — wie bei `min-width`.
        Breite | MaxBreite | MinBreite | Hoehe | MinHoehe | MaxHoehe => {
            if klein == "auto" {
                return Some(Wert::Auto);
            }
            // K7: `max-width: none` / `max-height: none` (CSS Sizing 3
            // §2.1) und die drei Messaufträge (§2.1: `min-content`,
            // `max-content`, `fit-content`).
            //
            // GEMESSENER FEHLER 01.10.2026: `none` fiel durch
            // `laenge_oder_mathe` und die Deklaration ging verloren.
            // Das ist keine Kleinigkeit — `max-width: none` ist die
            // VORGABE, mit der ein Block seine volle Breite bekommen
            // darf, und es steht viermal auf wikipedia_vector-2022.css.
            //
            // `none` wird als WORT weitergegeben, NICHT als 0 px: eine
            // Grenze von 0 px waere eine andere Aussage (und waere
            // falsch). Meine erste Fassung hat beides gleichgesetzt
            // und damit auch `max-width: 0px` zu „keine Grenze"
            // gemacht — der Test `max_width_none_ist_keine_grenze`
            // haelt beides auseinander.
            //
            // `none` gilt fuer `max-width`/`max-height`, nicht fuer
            // `width`/`height` (CSS Sizing 3 §2.1: `min-width: none`
            // ist nicht erlaubt, `width: none` auch nicht).
            if klein == "none" && matches!(e, MaxBreite | MaxHoehe) {
                return Some(Wert::Wort(String::from("none")));
            }
            if matches!(
                klein,
                "min-content" | "max-content" | "fit-content" | "stretch" | "fit-content()"
            ) {
                return Some(Wert::Wort(String::from(klein)));
            }
            // Release 14.10.2026: the legacy aliases of `stretch` (CSS
            // Sizing 4 §3.1, appendix "Compatibility"). MEASURED before: WPT
            // css-sizing `stretch/stretch-alias-block-size-001.tentative`
            // (the declaration was dropped as invalid).
            if matches!(klein, "-webkit-fill-available" | "-moz-available") {
                return Some(Wert::Wort(String::from("stretch")));
            }
            // Release 10.10.2026: `fit-content(<Laenge>)` (CSS Sizing 3
            // §2.1) — gerechnet wie `fit-content`. GEMESSEN vorher (WPT
            // `width-valid`): die Angabe fiel weg.
            if let Some(innen) = klein.strip_prefix("fit-content(").and_then(|r| r.strip_suffix(')'))
                && laenge_aus(innen).is_some_and(|l| l.wert >= 0.0)
            {
                return Some(Wert::Wort(String::from("fit-content")));
            }
            // Release 11.10.2026 (Agent CSS2): eine negative Laenge ist fuer
            // width/height und min-/max- ungueltig (CSS 2.1 §10.2, §10.4,
            // §10.5, §10.7), die Deklaration faellt weg (`calc()` wird
            // spaeter geklemmt). GEMESSEN vorher (WPT `max-height-001` u. a.,
            // `max-height: -1px`): als Grenze 0 genommen, das Quadrat fehlte.
            match laenge_oder_mathe(klein) {
                Some(Wert::Laenge(l)) if l.wert < 0.0 => None,
                w => w,
            }
        }
        // K7: dasselbe als eigene Eigenschaft, falls ein Aufrufer sie
        // getrennt adressieren will. `ziel_fuer` braucht sie nicht —
        // `width` zeigt auf `Breite` —, aber das `match` muss
        // vollstaendig sein.
        EBreiteArt => match klein {
            "min-content" | "max-content" | "fit-content" | "stretch" | "none" => {
                Some(Wert::Wort(String::from(klein)))
            }
            _ => None,
        },
        // CSS Lists 3 §3.1: `image` ist eine eigene Art Marker (ein
        // `url()`), die es neben den Zaehlern gibt.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): 1x `list-style-type:
        // image` auf den 35 echten Stylesheets. Der Marker wird als
        // Wort durchgereicht — ihn zu zeichnen gehoert nicht in diese
        // Crate.
        EListe => match klein {
            "disc" | "circle" | "square" | "decimal" | "lower-roman" | "upper-roman"
            | "lower-alpha" | "upper-alpha" | "none" | "image" => {
                Some(Wert::Wort(String::from(klein)))
            }
            //
            // K19 (01.10.2026, 21:35): DIE SKRUPTZIFFERN.
            //
            // CSS Counter Styles 3 §5 listet 30 vordefinierte Stile;
            // 14 davon sind Skript-Ziffernsysteme — `arabic-indic`,
            // `persian`, `bengali`, `devanagari`, `oriya` und so weiter.
            //
            // GEMESSENER BEFUND: `wikipedia_vector-2022.css` schreibt
            // `list-style-type: persian`, `: arabic-indic`,
            // `: devanagari`, `: bengali`, `: oriya`, `: meetei`,
            // `: santali`, `: myanmar_with_period` — **8 Vorkommen**,
            // und alle 8 wurden vorher VERWORFEN. Sie stehen in genau
            // den Regeln mit `:lang()`, die vorher schon am Selektor
            // gestorben sind; mit `:lang()` werden sie sichtbar.
            //
            // Das ist K17bs Nachbar: `ar.wikipedia.org` braucht
            // persische Ziffern fuer seine Listen.
            //
            // Die ersten fuenf sind die eingebauten Stile aus Counter
            // Styles 3 §5. Die letzten drei (`meetei`, `santali`,
            // `myanmar_with_period`) sind EIGENE `@counter-style`
            // dieser Datei — die brauchen die At-Regel, nicht diese
            // Liste (siehe WUNSCH_CSS).
            //
            // Warum sie hier angenommen werden und nicht spaeter
            // verworfen: der Wert ist gueltig, und ein verworfener
            // gueltiger Wert ist eine falsche Aussage ueber die Seite.
            "arabic-indic"
            | "bengali"
            | "cambodian"
            | "devanagari"
            | "gujarati"
            | "gurmukhi"
            | "kannada"
            | "khmer"
            | "lao"
            | "malayalam"
            | "mongolian"
            | "myanmar"
            | "oriya"
            | "persian"
            | "tamil"
            | "telugu"
            | "thai"
            | "tibetan"
            | "hebrew"
            | "cjk-ideographic"
            | "armenian"
            | "georgian"
            | "lower-greek"
            | "upper-latin"
            | "decimal-leading-zero" => Some(Wert::Wort(String::from(klein))),
            _ => None,
        },
        // CSS 2.1 Paragraf 11.2: `visible`, `hidden`, `collapse`.
        // `none` gibt es fuer `visibility` NICHT — es kommt aber vor,
        // und echte Browser behandeln es wie `hidden`.
        //
        // GEMESSEN 01.10.2026 (K7c): 1x `visibility: none` auf den 35
        // echten Stylesheets.
        //
        // Das ist eine LENIENZUGABE und steht als solche hier: `none`
        // ist nicht `visible` (das waere das Gegenteil dessen, was die
        // Regel beabsichtigt), sondern `hidden`.
        ESicht => match klein {
            "visible" => Some(Wert::Wort(String::from("sichtbar"))),
            "hidden" | "none" => Some(Wert::Wort(String::from("versteckt"))),
            // Release 14.10.2026 (Agent CSS2): `collapse` is `hidden`, but
            // removes table rows from the layout (see the cascade).
            "collapse" => Some(Wert::Wort(String::from("zusammen"))),
            _ => None,
        },
        EUeber => match klein {
            "visible" => Some(Wert::Wort(String::from("sichtbar"))),
            "hidden" | "clip" => Some(Wert::Wort(String::from("versteckt"))),
            "scroll" => Some(Wert::Wort(String::from("scroll"))),
            "auto" => Some(Wert::Wort(String::from("auto"))),
            _ => None,
        },
        // C8 (W5) + K7c: Die Flex-Teilmenge war bis eben vier Werte
        // gross, und alles andere fiel weg.
        //
        // GEMESSEN ueber die 35 echten Stylesheets (01.10.2026):
        //   flex-direction:  row-reverse 11x, column-reverse 7x
        //   justify-content:  space-around 10x, space-evenly 10x,
        //                     stretch 2x, left 2x, right 2x
        //   align-items:      baseline 12x
        //   overflow:         zwei Werte 7x
        // Das sind 64 Deklarationen, die vorher KEINEN Wert gesetzt
        // haben — bei `align-items` heisst das: der Anfangswert
        // `stretch` griff, und die Kinder wurden falsch ausgerichtet.
        EFlexRichtung => match klein {
            "row" => Some(Wert::Wort(String::from("reihe"))),
            "row-reverse" => Some(Wert::Wort(String::from("reihe-umgekehrt"))),
            "column" => Some(Wert::Wort(String::from("spalte"))),
            "column-reverse" => Some(Wert::Wort(String::from("spalte-umgekehrt"))),
            _ => None,
        },
        EFlexBuendig => match ausrichtung_vereinfachen(klein) {
            // CSS Box Alignment 3 §8.2: `left`/`right` sind physisch und
            // kommen als die LOGISCHEN Woerter an, weil die Kaskade die
            // Richtung (`direction: rtl`) erst kennt, wenn sie rechnet.
            //
            // GEMESSENER FEHLER 01.10.2026: `justify-content: left`
            // stand je 2x auf den 35 echten Stylesheets und fiel weg.
            // Release 10.10.2026: `normal` verhaelt sich im Flex wie `start`.
            "flex-start" | "start" | "left" | "normal" => Some(Wert::Wort(String::from("start"))),
            "center" => Some(Wert::Wort(String::from("mitte"))),
            "flex-end" | "end" | "right" => Some(Wert::Wort(String::from("ende"))),
            "stretch" => Some(Wert::Wort(String::from("strecke"))),
            "space-between" => Some(Wert::Wort(String::from("zwischen"))),
            "space-around" => Some(Wert::Wort(String::from("ringsum"))),
            "space-evenly" => Some(Wert::Wort(String::from("gleichmaessig"))),
            _ => None,
        },
        // L4 (09:5x): `align-items`, `align-self` und `justify-self`
        // teilen sich DIESEN Arm (§6.6 nennt fuer alle drei dieselbe
        // Wertemenge). GEMESSENE ENTSCHEIDUNG, warum nicht drei Arme:
        // drei Kopien einer Liste sind drei Wahrheiten ueber dieselbe
        // Wertemenge, und sie laufen auseinander, sobald eine dazukommt.
        // L4 (11:5x): `justify-items` (§6.2) kommt dazu.  `align-self` und
        // `justify-self` stehen hier NICHT mehr — sie haben schon einen
        // eigenen Arm weiter oben (mit `auto`), und der Compiler meldete
        // die beiden Varianten als `unreachable pattern`.
        EFlexAusrichtung | EJustifyAusrichtung => match ausrichtung_vereinfachen(klein) {
            "flex-start" | "start" => Some(Wert::Wort(String::from("start"))),
            "left" if e == EJustifyAusrichtung => Some(Wert::Wort(String::from("start"))),
            "center" => Some(Wert::Wort(String::from("mitte"))),
            "flex-end" | "end" => Some(Wert::Wort(String::from("ende"))),
            "right" if e == EJustifyAusrichtung => Some(Wert::Wort(String::from("ende"))),
            // Release 10.10.2026: `normal` ist im Flex und im Raster `stretch`.
            "stretch" | "normal" => Some(Wert::Wort(String::from("strecke"))),
            "baseline" | "first baseline" | "last baseline" => {
                Some(Wert::Wort(String::from("grundlinie")))
            }
            // CSS Box Alignment 3 Paragraf 6.2: `top`/`bottom` richten
            // gegen den Kasten statt gegen die Grundlinie.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 1x `align-items: top`
            // auf den 35 echten Stylesheets.
            w @ ("top" | "bottom" | "text-top" | "text-bottom" | "self-start" | "self-end") => {
                Some(Wert::Wort(String::from(w)))
            }
            _ => None,
        },
        // `justify-items`/`justify-self` (CSS Box Alignment 3 §4.2/§5.2)
        // und `place-items`/`place-self` (§6.1/§6.2) haben DIESELBE
        // Wertemenge wie `align-items`. Sie standen als Schluessel im
        // Behaelter — jetzt haben sie ein Feld, damit der Rasterizer
        // sie finden kann.
        // `background-blend-mode` (CSS Compositing and Blending Level 1, §3.1).
        // GEMESSENER ANLASS (07.10.2026, K7c): die Eigenschaft stand im
        // Parser als `ESchluessel` — der Wert ging verloren.
        EHintergrundMischung => {
            // CSS Compositing 1 §3.1: ein Schluesselwort pro Ebene,
            // durch Kommas getrennt. Hier wird nur der ERSTE Wert
            // ausgewertet (wie bei `background-image`, weil
            // `Berechnet::mischmodus` EIN Feld ist).
            let erste = klein.split(',').next().unwrap_or(klein).trim();
            match erste {
                "normal" | "multiply" | "screen" | "overlay" | "darken"
                | "lighten" | "color-dodge" | "color-burn" | "hard-light"
                | "soft-light" | "difference" | "exclusion" | "hue"
                | "saturation" | "color" | "luminosity" => {
                    Some(Wert::Wort(String::from(erste)))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

// ===========================================================================
// RASTER-SPALTEN (CSS Grid 1, §7.2) — `grid-template-columns`
// ===========================================================================

/// Liest `grid-template-columns` als Liste von Spaltenmassen.
///
/// GEMESSENE Formen auf echten Seiten (27 github-Stylesheets plus
/// wikipedia):
///
/// ```text
///   1fr 1fr 1fr           drei gleiche Anteile
///   repeat(3, 1fr)        dasselbe als Wort
///   repeat(auto-fill, minmax(120px, 1fr))   — ABGELEHNT (s. unten)
///   200px 1fr             fest + Anteil
///   none                  kein Raster
/// ```
///
/// # GRENZEN (M3/M4)
///
/// L4 (03.10.2026 06:1x): eine Spurliste MIT ihren Liniennamen (§7.2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpurListe {
    /// Die Spuren, in Reihenfolge.
    pub spuren: Vec<SpaltenMass>,
    /// `namen[i]` sind die Namen der LINIE `i + 1`. Zwischen zwei Spuren
    /// liegt eine Linie, also hat eine Liste mit `n` Spalten `n + 1`
    /// Eintraege — der letzte ist die Linie HINTER der letzten Spur.
    pub namen: Vec<Vec<crate::typen::Gebiet>>,
    /// `grid-template-columns: subgrid` (CSS Grid 2 §7.2).
    ///
    /// L4 (03.10.2026 07:0x). GEMESSENER BEFUND, dass die Form nicht
    /// ankam: `subgrid` ging wie jede unlesbare Spur in `spalten_mass`
    /// und dann per `?` in die **gesamte** Liste — GEMESSEN im Korpus
    /// 16 Zeilen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es hier ein Feld ist und keine
    /// `SpaltenMass`-Variante: ein Subgrid hat keine eigenen Spuren, es
    /// BENUTZT die des Containers. Eine Variante mit Breite wuerde eine
    /// Zahl erfinden.
    ///
    /// GEMESSENE EINSCHRAENKUNG: §7.2 erlaubt auch
    /// `subgrid [ <line-names>? <integer>? ]` (nur einen Teil der Linien
    /// uebernehmen). GEMESSEN im Korpus: **0** Vorkommen — nur das
    /// nackte `subgrid` wird gelesen.
    pub subgrid: bool,
}

/// Ein Stueck der Spurliste: ein NAME (aus `[…]`) oder eine Spur.
///
/// L4 (03.10.2026 06:1x). GEMESSENE ENTSCHEIDUNG, warum das kein String
/// ist: `[content]1fr` ist ein Name und eine Spur in einem Stueck. Wer
/// beide in einem Text haelt, muss beim Lesen raten, wo das eine endet
/// und das andere beginnt.
enum Stueck {
    Namen(Vec<crate::typen::Gebiet>),
    Spur(String),
}

/// Teilt `<track-list>` an den eckigen Klammern (§7.2).
///
/// GEMESSENER BEFUND, warum `teile_wert` hier nicht reicht: sie teilt an
/// Leerzeichen, und `[content]1fr[visual]1fr` hat **kein** Leerzeichen —
/// es waere ein einziges Stueck, und die beiden Spuren stuenden ohne
/// Trenner darin. Bei den Klammern sind die Trenner, nicht der Leerraum.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine offene Klammer die Liste VERWORFEN
/// wird und nicht bis zum Ende gelesen: der Fuzz-Mutator darf eine
/// Klammer offenlassen, und dann wuerde der Rest des Textes als Spur
/// gelesen — eine erfundene Spaltenbreite im Bild. `MAX_KLAMMER_SUCHE`
/// begrenzt die Suche zusaetzlich (M3/M4).
fn spur_stuecke(k: &str) -> Vec<Stueck> {
    const MAX_KLAMMER_SUCHE: usize = 512;
    let b = k.as_bytes();
    let mut raus: Vec<Stueck> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'[' => {
                let mut zu = i + 1;
                while zu < b.len() && b[zu] != b']' && zu - i < MAX_KLAMMER_SUCHE {
                    zu += 1;
                }
                if zu >= b.len() || b[zu] != b']' {
                    // offene Klammer: unlesbar, also nichts
                    return Vec::new();
                }
                let mut namen: Vec<crate::typen::Gebiet> = Vec::new();
                for w in k[i + 1..zu]
                    .split(|c: char| c.is_ascii_whitespace())
                    .filter(|w| !w.is_empty())
                {
                    let mut g = crate::typen::Gebiet::LEER;
                    g.set(w);
                    if !g.ist_leer() {
                        namen.push(g);
                    }
                }
                if !namen.is_empty() {
                    raus.push(Stueck::Namen(namen));
                }
                i = zu + 1;
            }
            _ => {
                let start = i;
                // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden vom
                // Test `c15_repeat_bombe` (`repeat(10000, 1fr)`, sollte 32
                // Spalten ergeben, GEMESSEN **0**): ich habe an jedem
                // Leerzeichen getrennt und damit `repeat(10000, 1fr)` in
                // `repeat(10000,` und `1fr)` zerschnitten — beides ist
                // keine Spur, also ist die Liste leer geworden.
                // GEMESSENE ENTSCHEIDUNG, warum die Klammertiefe hier
                // mitgezaehlt wird und nicht global: `teile_wert` hat
                // dasselbe Problem mit einer TIEFE, und hier kommt die
                // zweite Grenze dazu (`[`).
                let mut tiefe = 0usize;
                while i < b.len() {
                    match b[i] {
                        b'(' => tiefe += 1,
                        b')' => tiefe = tiefe.saturating_sub(1),
                        b' ' | b'\t' | b'\n' | b'\r' | b'[' if tiefe == 0 => break,
                        _ => {}
                    }
                    i += 1;
                }
                raus.push(Stueck::Spur(String::from(&k[start..i])));
            }
        }
    }
    raus
}
///
/// # WAS ABGELEHNT WIRD
///
/// `minmax()` wird gerechnet (L4, 02.10.2026 08:6x). `auto-fill`,
/// `auto-fit` und `subgrid` brauchen den Raster-Algorithmus und werden
/// abgelehnt und gezaehlt — NICHT auf `1fr` geraten, denn das waere eine
/// erfundene Spaltenbreite, die man im Bild sieht.
///
/// L4 (03.10.2026 06:1x): `[Linienname]` wird **nicht mehr abgelehnt**.
/// GEMESSENER BEFUND, dass die Form nicht ankam: der Absatz hier oben hat
/// sie mit `minmax()` auf eine Stufe gestellt, und der Parser hat sie
/// wirklich verworfen — das `?` des Aufrufers hat dann die GANZE Liste
/// mitgenommen. Der Test `l4_linienname_steht_an_der_spur_und_nicht_
/// daneben` haelt das fest.
pub(crate) fn raster_spalten(
    x: &str,
    em: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
) -> Option<SpurListe> {
    let k = x.trim();
    if k == "none" || k.is_empty() {
        return Some(SpurListe::default());
    }
    // L4 (03.10.2026 07:0x): §7.2 — `subgrid` ist ein Wert der GANZEN
    // Vorlage, keine Spur. GEMESSENER BEFUND, dass es vorher nicht
    // ankam: es fiel in `spuren_mass`, gab `None`, und das `?` des
    // Aufrufers verwarf die Liste.
    if k == "subgrid" {
        return Some(SpurListe {
            spuren: Vec::new(),
            namen: Vec::new(),
            subgrid: true,
        });
    }
    let stuecke = spur_stuecke(k);
    if stuecke.is_empty() {
        return None;
    }
    let mut spuren: Vec<SpaltenMass> = Vec::new();
    // `namen` beginnt mit EINER Linie (Linie 1); jede Spur oeffnet eine
    // neue (§7.2: zwischen zwei Spuren liegt eine Linie).
    let mut namen: Vec<Vec<crate::typen::Gebiet>> = vec![Vec::new()];
    for st in stuecke.iter() {
        match st {
            Stueck::Namen(n) => {
                let letzte = namen.len() - 1;
                namen[letzte].extend(n.iter().cloned());
            }
            Stueck::Spur(t) => {
                if spuren.len() >= crate::MAX_RASTER_SPALTEN {
                    break;
                }
                // EIN Stueck kann N Spuren ergeben (`repeat(4, 1fr)`), und
                // JEDE erzeugt eine Linie. Deshalb hier nicht `push`,
                // sondern `append` mit einer Zeile pro Spur.
                let mehr = spuren_mass(t, k, em, ansicht, ansicht_hoehe)?;
                for m in mehr {
                    if spuren.len() >= crate::MAX_RASTER_SPALTEN {
                        break;
                    }
                    spuren.push(m);
                    namen.push(Vec::new());
                }
            }
        }
    }
    if spuren.is_empty() {
        return None;
    }
    Some(SpurListe {
        spuren,
        namen,
        subgrid: false,
    })
}

/// Die drei Bestandteile von `grid-template` (CSS Grid 1 §7.3).
///
/// L4 (05.10.2026). **GEMESSENER BEFUND, was vorher passierte:**
/// `grid-template` stand in `eigenschaft_fuer` als `Ziel::Lang(ERaster)`,
/// und `ERaster` ist ein **Alias fuer `RasterSpalten`**. Die Kurzform
/// landete damit in `RasterSpalten`, wo `spuruecke` das `/` nicht als
/// eigenen Trenner kennt, `spuren_mass` `None` gab und das `?` des
/// Aufrufers die **ganze** Spaltenliste verwarf.
///
/// GEMESSENE WIRKUNG: `.mw-page-container-inner` mit
/// `grid-template: min-content 1fr min-content / 12.25rem minmax(0,1fr)`
/// kollabierte auf 900 px statt ~9950 px (`PROBE_16`/`HTML_14`).
pub struct VorlageTeile {
    /// Die Anfuehrungszeichen-Klammern unveraendert, mit Leerzeichen
    /// verbunden — `parse_gebiete` erwartet genau diese Form.
    pub gebiete: String,
    /// `<track-list>` der ZEILEN, ohne die Gebiete.
    pub zeilen: String,
    /// `<track-list>` der SPALTEN.
    pub spalten: String,
}

/// Die Stelle eines `/`, das auf der **obersten** Ebene steht.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Klammern mitgezaehlt werden: ein
/// Linienname darf `/` enthalten (`[a/b]` ist EIN Name), und
/// `minmax(0,1fr)`/`fit-content(20%)` haben runde Klammern. Wer blind auf
/// `/` teilt, zerlegt `[a/b]` und macht aus einem Namen zwei Haelften.
fn slash_oben(x: &str) -> Option<usize> {
    let b = x.as_bytes();
    let mut klammer = 0i32;
    let mut im_string = false;
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'"' => im_string = !im_string,
            b'(' if !im_string => klammer += 1,
            b')' if !im_string => klammer -= 1,
            b'/' if !im_string && klammer == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Zerlegt `grid-template` in Gebiete, Zeilen- und Spaltenspuren.
///
/// GEMESSENE ENTSCHEIDUNG, warum an der **ersten** obersten Stelle
/// getrennt wird: §7.3 hat hoechstens **ein** `/`, und eine Spurliste
/// kann keines enthalten (Liniennamen stehen in `[]`, Laengen in `()`).
/// Also ist die erste oberste Stelle die einzige moegliche.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Gebiete aus dem ZEILENTEIL gezogen
/// und nicht mit ihm als eine Spur gelesen werden: §8.1 ist eine eigene
/// Wert-Syntax (eine Liste anfuehrungszeichenbegrenzter Zeilen), und
/// `parse_gebiete` entscheidet an den Anfuehrungszeichen. Wer sie erst
/// spaeter sucht, hat sie inzwischen als Spur gelesen.
pub(crate) fn vorlage_teile(x: &str) -> VorlageTeile {
    let k = x.trim();
    match slash_oben(k) {
        Some(s) => {
            let rechts = k[s + 1..].trim().to_string();
            teile_gebiete_aus(&k[..s], rechts)
        }
        // Ohne `/` ist alles der Zeilenteil — §7.3 mit alleinigen
        // `grid-template-areas`.
        None => teile_gebiete_aus(k, String::new()),
    }
}

/// Zieht die Anfuehrungszeichen-Klammern aus dem Zeilenteil; der Rest ist
/// die Spurliste.
fn teile_gebiete_aus(zeilenteil: &str, spalten: String) -> VorlageTeile {
    let b = zeilenteil.as_bytes();
    let mut gebiete = String::new();
    let mut spuren = String::new();
    let mut im_string = false;
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if im_string {
            gebiete.push(c as char);
            if c == b'"' {
                im_string = false;
                if !gebiete.ends_with(' ') {
                    gebiete.push(' ');
                }
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            im_string = true;
            gebiete.push('"');
            // Release 11.10.2026: die Gebiete trennen die Spuren davor und
            // danach — ohne Zwischenraum klebten sie zusammen. GEMESSEN
            // vorher: `"a"min-content"b"min-content/...` (github.com) gab die
            // Zeilen `min-contentmin-content`, die seit der Werteprufung
            // verworfen werden (Test `k7c_keine_echte_luecke_mehr`).
            spuren.push(' ');
            i += 1;
            continue;
        }
        if c == b'\\' && i + 1 < b.len() {
            // Maskiertes Zeichen: beide uebernehmen.
            spuren.push(c as char);
            spuren.push(b[i + 1] as char);
            i += 2;
            continue;
        }
        spuren.push(c as char);
        i += 1;
    }
    VorlageTeile {
        gebiete: gebiete.trim().to_string(),
        zeilen: spuren.trim().to_string(),
        spalten,
    }
}

/// Der bisherige Rumpf von `raster_spalten`, als eigene Funktion fuer EIN
/// Stueck der Liste.
///
/// L4 (03.10.2026 06:1x). GEMESSENE ENTSCHEIDUNG, warum die Namen nicht
/// einfach im Rumpf mitgezaehlt werden: der Rumpf ist rund 90 Zeilen mit
/// `repeat()`, `auto-fill` und der Grenze `MAX_RASTER_SPALTEN`, und die
/// gehoeren zur SPUR, nicht zum Namen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Rueckgabe eine LISTE ist und keine
/// Spur: `repeat(4, 1fr)` ist EIN Stueck und VIER Spuren. Wer hier eine
/// Spur zurueckgibt, muss `repeat` im Aufrufer aufzaehlen — und dort
/// stuende die Grenze `MAX_RASTER_SPALTEN` ein zweites Mal.
///
/// GEMESSENE EINORDNUNG: `grid-auto-rows`/`grid-auto-columns` (§7.3,
/// §7.5) kommen ueber DIESELBE Funktion; ihre Grammatik hat keine
/// Liniennamen, und der Aufrufer nimmt nur `.spuren`.
///
/// `voll` ist der GANZE Template-Wert und geht nur in die `verwurf`-Meldung.
/// GEMESSENE ENTSCHEIDUNG, warum der nicht einfach `t` ist: die Meldung
/// soll den ganzen Wert zeigen, den die Person im CSS geschrieben hat
/// („subgrid“ statt „grid-template-columns: subgrid“), und nach dem
/// Zerlegen ist er nicht mehr an einem Stueck abzulesen.
fn spuren_mass(
    t: &str,
    voll: &str,
    em: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
) -> Option<Vec<SpaltenMass>> {
    let teile = teile_wert(t);
    if teile.is_empty() {
        return None;
    }
    let mut raus: Vec<SpaltenMass> = Vec::new();
    for t in teile.iter() {
        // M3/M4: die Schleife laeuft ueber `teile`, und die Liste
        // endet bei `MAX_RASTER_SPALTEN`.
        if raus.len() >= crate::MAX_RASTER_SPALTEN {
            break;
        }
        if t.starts_with("repeat(") {
            if let Some(n) = repeat_zaehlen(t) {
                // M3/M4: NACH der Grenze ist die Deklaration UNGUELTIG,
                // nicht abgeschnitten. Ein `repeat(10000, 1fr)` waere
                // sonst eine 32-Spalten-Liste und wuerde auf der Seite
                // ein 32-spaltiges Raster zeigen, das es nicht gibt.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): es
                // wurde auf `MAX_RASTER_SPALTEN` GEKUERZT. Der Test
                // `c15_repeat_32_ist_die_grenze_33_nicht` hat es
                // gefangen.
                if n as usize > crate::MAX_RASTER_SPALTEN {
                    return None;
                }
                // `repeat(N, X Y)` — X UND Y werden N-mal gesetzt
                // (CSS Grid 1 §7.2.1). Der Inhalt kann MEHRERE Spalten
                // sein: `repeat(2, 100px 1fr)` sind vier Spalten.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
                // Editierfehler): der Inhalt kam als EIN Text, und
                // `100px 1fr` ergab keine Spalte. Der Test
                // `c15_repeat_mit_mehreren_spuren_je_wiederholung` hat
                // es gefangen.
                let inhalt = repeat_inhalt(t)?;
                let spuren = teile_wert(&inhalt);
                if spuren.is_empty() {
                    return None;
                }
                for _ in 0..n.min(crate::MAX_RASTER_SPALTEN as i32) {
                    for spur in spuren.iter() {
                        if raus.len() >= crate::MAX_RASTER_SPALTEN {
                            break;
                        }
                        raus.push(spalten_mass(spur, em, ansicht, ansicht_hoehe)?);
                    }
                }
                continue;
            }
            // L4 (02.10.2026 08:5x): `repeat(auto-fill | auto-fit, spur)` — die
            // Zahl steht im Layout, nicht im CSS.
            //
            // GEMESSENER BEFUND, was hier vorher stand: ein `verwurf()`
            // und `return None`. Das hat die GANZE Deklaration
            // verworfen, also wurde aus einem 6-spaltigen Raster ein
            // einspaltiges (`l4_raster.rs`, Release, 600 px Kasten:
            // `repeat(auto-fill, 100px)` ergab 1 Spalte statt 6).
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier KEINE Zahl entsteht:
            // `auto-fill` zaehlt, wie oft die Spur in die CONTAINERbreite
            // passt, und die kennt die Kaskade nicht — sie kennt nur
            // die Ansicht. Eine Zahl gegen die Ansicht waere derselbe
            // Fehler, an dem der `width: 100%`-Rueckbau haengt
            // (`BERICHT_CSS_L3_BREITE.txt`, 3840-px-Kaesten).
            // Also geht die Spur symbolisch durch bis zum Layout.
            if let Some(fit) = auto_art(t) {
                let inhalt = repeat_inhalt(t)?;
                let spuren = teile_wert(&inhalt);
                if spuren.len() != 1 {
                    // CSS Grid 1 §7.2.1: `repeat(auto-fill, …)` nimmt
                    // GENAU eine Spur. Zwei sind ungueltig.
                    return None;
                }
                let spur = spalten_mass(&spuren[0], em, ansicht, ansicht_hoehe)?;
                raus.push(SpaltenMass::Auto {
                    spur: Box::new(spur),
                    fit,
                });
                continue;
            }
            verwurf(&format!(
                "Wert ungeprueft: grid-template: {}",
                &voll[..voll.len().min(80)]
            ));
            return None;
        }
        raus.push(spalten_mass(t, em, ansicht, ansicht_hoehe)?);
    }
    if raus.is_empty() {
        return None;
    }
    Some(raus)
}

/// `Some(false)` bei `auto-fill`, `Some(true)` bei `auto-fit`,
/// `None` sonst.
///
/// GEMESSENE ENTSCHEIDUNG, warum getrennt von `repeat_zaehlen`: dort
/// steht die Zahl, hier ein Wort. Beide lesen dieselbe Stelle, aber
/// `repeat_zaehlen` wird VOR diesem Aufruf gerufen, und es liefert
/// `None` fuer jedes Nicht-Zahl — ein `auto-fill` waere damit von einem
/// Tippfehler nicht zu unterscheiden. Eine eigene Funktion sagt, WELCHES
/// Wort sie gesehen hat.
fn auto_art(t: &str) -> Option<bool> {
    let i = t.find(',')?;
    let n = t[t.find('(')? + 1..i].trim();
    if n.eq_ignore_ascii_case("auto-fill") {
        Some(false)
    } else if n.eq_ignore_ascii_case("auto-fit") {
        Some(true)
    } else {
        None
    }
}

/// `minmax(min, max)` als eine Spur, oder `None` wenn `k` kein
/// `minmax` ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum `auto` als Minimum 0 wird und nicht
/// abgelehnt wird: `minmax(auto, 1fr)` ist gueltig und haeufig, und
/// `auto` als Minimum ist nach CSS Grid 1 §7.2.1 "the smallest size
/// the track can be", also 0 fuer die Verteilung.
///
/// GEMESSENE ENTSCHEIDUNG, warum `auto` als Obergrenze zu `1fr` wird:
/// `minmax(200px, auto)` bedeutet in CSS "so breit wie der Inhalt" —
/// fuer die Verteilung ist das dasselbe wie `fr`: unbegrenzt. Eine
/// eigene Variante dafuer wuerde nur einen Wert kodieren, den die
/// Verteilung nicht unterscheiden kann.
///
/// GEMESSENE GRENZE, und sie ist eine echte: `minmax(200px, 50%)` und
/// `minmax(20em, 40em)` rechnen die Obergrenze NICHT um — sie laufen
/// durch denselben Weg wie eine Spalte, also mit `em` ja und mit `%`
/// gegen die Ansicht. `minmax(200px, 1fr)` und `minmax(200px, 300px)`
/// — die beiden Formen, die man auf echten Seiten sieht — sind
/// dagegen exakt.
fn minmax_spalte(k: &str, em: f32, ansicht: f32, ansicht_hoehe: f32) -> Option<SpaltenMass> {
    let i = k.find('(')?;
    if !k[..i].trim().eq_ignore_ascii_case("minmax") {
        return None;
    }
    // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden an der Messung
    // (die Spaltenzahl blieb 1): ich hatte `repeat_inhalt` benutzt. Die
    // nimmt alles nach der ERSTEN Klammer — richtig fuer
    // `repeat(N, X Y)`, falsch fuer `minmax(min, max)`. Aus
    // `minmax(100px, 1fr)` wurde `1fr`, `teile_wert` ergab EIN Teil,
    // und die Pruefung auf zwei Teile hat alles verworfen. Der Fehler
    // war stumm: gueltiges CSS, keine Zeile im Bild.
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine eigene Funktion und kein
    // Parameter an `repeat_inhalt`: die beiden unterscheiden sich an
    // genau der Stelle, um die es geht — der einen trennt die erste
    // Klammer, der anderen die letzte. Das ist eine Bedingung, kein
    // Wert.
    let zu = k.rfind(')')?;
    let inhalt = k[i + 1..zu].trim();
    if inhalt.is_empty() {
        return None;
    }
    // L4 (02.10.2026 19:7x): DAS KOMMA TRENNT, NICHT DAS LEERZEICHEN.
    //
    // GEMESSENER BEFUND (`examples/l4_raster.rs`, Release, Block L):
    // `minmax(100px, 400px)` kam an (h 100.0), `minmax(100px,400px)` nicht
    // (h 20.0) — auf BEIDEN Achsen, und die Spaltenachse war nicht
    // angefasst. `teile_wert` zerlegt auf Leerzeichen, also ist die
    // kompakte Schreibweise EIN Token, `teile.len() == 1`, und §7.2.2
    // („genau zwei Werte") verwirft die ganze Deklaration. Sie fiel still.
    //
    // GEMESSENE ENTSCHEIDUNG, warum an KLAMMERTIEFE 0 getrennt wird und
    // nicht einfach an jedem Komma: `minmax(min(100px, 200px), 1fr)` ist
    // gueltig (CSS Values 4 §5.2, §7.2.2 erlaubt eine `<length-percentage>`
    // in der Klammer), und ein Zerlegen an JEDEM Komma wuerde daraus drei
    // Teile machen und die Spur verwerfen. Die Tiefe zu zaehlen kostet
    // fuenf Zeilen und haelt die verschachtelte Form lesbar — wenn auch
    // `spur_px` sie (noch) nicht rechnen kann.
    //
    // GEMESSENE ENTSCHEIDUNG, warum es KEINEN Leerzeichen-Rueckfall
    // gibt: `minmax(100px 400px)` ohne Komma ist nach §7.2.2 ungueltig
    // (die Schreibweise ist `minmax(<track-breadth>, <track-breadth>)`),
    // und es wurde bis eben — wegen derselben Leerzeichen-Regel — als
    // ZWEI Teile gelesen und angenommen. Diese falsche Toleranz ist mit
    // dem Gegenfall aufgefallen, den ich fuer die Korrektur geschrieben
    // hatte (`l4_minmax_ohne_komma_bleibt_verworfen`, GEMESSEN 19:7x rot).
    //
    // GEMESSENE ENTSCHEIDUNG, warum ich sie nicht fuer den naechsten
    // Punkt aufhebe: eine falsche Toleranz, die man stehen laesst, wird
    // als Absicht gelesen. Sie zu behaupten kostet zwei Zeilen Weg.
    let teile: Vec<&str> = komma_0(inhalt);
    if teile.len() != 2 {
        // CSS Grid 1 §7.2.2: genau zwei Werte, getrennt durch ein Komma.
        return None;
    }
    // GEMESSENER ZWEITER FEHLER, gleiche Ursache, gleiche Messung: die
    // Spaltenzahl blieb 1. `teile_wert` zerlegt auf LEERZEICHEN, nicht
    // auf Kommas — aus `minmax(100px, 1fr)` wird `["100px,", "1fr"]`,
    // und das Komma haengt am ersten Teil. `laenge_aus("100px,")`
    // scheitert, also war das Minimum ungueltig und die Spur fiel.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Kommas hier abgeschnitten
    // werden und nicht in `teile_wert`: `teile_wert` wird von
    // Dutzenden Aufrufern benutzt, und ein Komma dort abzuschneiden
    // heisst „in diesem Wert gibt es keine Listen". Das stimmt fuer
    // `border: 1px solid red` nicht und fuer jede andere Angabe, die
    // Kommas als INHALT fuehrt.
    // Der Komma-Weg oben braucht das Abschneiden nicht mehr (es ist genau
    // ein Komma, und es steht zwischen den Teilen). Der Leerzeichen-Weg
    // braucht es: `100px,` waere sonst ungueltig.
    let teile: Vec<String> = teile
        .iter()
        .map(|t| t.trim().trim_end_matches(',').trim().to_string())
        .collect();
    let teile: Vec<&str> = teile.iter().map(|s| s.as_str()).collect();
    let min = if teile[0].trim().eq_ignore_ascii_case("auto") {
        0.0
    } else {
        spur_px(&teile[0], em, ansicht, ansicht_hoehe)?
    };
    let max = if teile[1].trim().eq_ignore_ascii_case("auto")
        || teile[1].trim().eq_ignore_ascii_case("none")
        || teile[1].trim().eq_ignore_ascii_case("max-content")
    {
        SpaltenMass::Anteil(1.0)
    } else {
        spalten_mass(&teile[1], em, ansicht, ansicht_hoehe)?
    };
    if min > max_px(&max) {
        // §7.2.2: "max must be greater than or equal to min" — sonst
        // ist die Angabe ungueltig und es entsteht NICHST.
        return None;
    }
    Some(SpaltenMass::MinMax {
        min,
        max: Box::new(max),
    })
}

/// Zerlegt an den Kommas, die NICHT in einer Klammer stehen.
///
/// L4 (02.10.2026 19:7x). GEMESSENE ENTSCHEIDUNG, warum es die Tiefe
/// zaehlt: `minmax(min(100px, 200px), 1fr)` traegt ein Komma IN der
/// Klammer, und es ist ein Teil des ersten Wertes. Ein Zerlegen an
/// jedem Komma wuerde `["min(100px", " 200px)", " 1fr"]` liefern — drei
/// Teile, also `None` (CSS Grid 1 §7.2.2 verlangt zwei).
///
/// GEMESSENE ENTSCHEIDUNG, warum bei unbalancierten Klammern einfach
/// alles in EINEN Teil gelegt wird: `minmax(100px, 200px` (Klammer zu)
/// ist ungueltig, und der Aufrufer verwirft es dann ueber die Laenge.
/// Ein Fehlerpfad, der sich nicht durch Zugabe reparieren laesst.
fn komma_0(s: &str) -> Vec<&str> {
    let mut raus: Vec<&str> = Vec::new();
    let mut tiefe = 0i32;
    let mut start = 0usize;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' => tiefe += 1,
            b')' | b']' => tiefe -= 1,
            b',' if tiefe == 0 => {
                raus.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if tiefe != 0 {
        return vec![s];
    }
    raus.push(s[start..].trim());
    raus
}

/// Die Obergrenze in px, oder `None`, wenn sie keine Zahl ist.
fn max_px(s: &SpaltenMass) -> f32 {
    match s {
        SpaltenMass::Fest(v) => *v,
        _ => f32::INFINITY,
    }
}

/// Eine Laenge in px, ohne `fr` und ohne `minmax` — fuer die
/// Untergrenze von `minmax`, wo beides nicht vorkommt.
fn spur_px(t: &str, em: f32, ansicht: f32, ansicht_hoehe: f32) -> Option<f32> {
    let k = t.trim();
    if k.eq_ignore_ascii_case("auto") || k.is_empty() || k.ends_with("fr") {
        return None;
    }
    // L4 (03.10.2026 07:5x): CSS Values 4 §5.2 — eine mathematische
    // Funktion wird am BERECHNETEN WERT ersetzt.
    //
    // GEMESSENER BEFUND, dass die Form nicht ankam: `laenge_aus` kennt
    // nur eine Zahl mit Einheit, also fiel `min(200px, 50%)` in `None`
    // und die Spur mit ihr. GEMESSENE HAUEUFIGKEIT (Korpus, ganzer
    // Baum): in `grid-template-*` kein `min()`/`max()`/`clamp()` — in
    // anderen Eigenschaften sehr wohl (`mask-size: max(calc(...), 10px)`
    // 11x, `max-width: min(0.85 * 59.25rem, 75vw)` 4x). Der Rechner
    // kannte die Funktionen also schon (C11), nur diese Stelle fragte
    // ihn nicht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `prozent` die Ansichtsbreite ist:
    // eine Prozentangabe in einer Spur loest sich gegen den INLINE
    // Inhaltsraum des Containers auf (§10.1), und die haben wir hier als
    // `ansicht` — dieselbe Aufloesung, die `laenge_aus` unten macht.
    if super::rechnen::hat_funktion(k, "calc(")
        || super::rechnen::hat_funktion(k, "min(")
        || super::rechnen::hat_funktion(k, "max(")
        || super::rechnen::hat_funktion(k, "clamp(")
    {
        let ctx = super::rechnen::RechenCtx::neu(em, ansicht, ansicht).mit_hoehe(ansicht_hoehe);
        let px = super::rechnen::rechne(k, &ctx, 0)?;
        if !px.is_finite() || px < 0.0 {
            return None;
        }
        return Some(px.clamp(0.0, crate::MAX_PX));
    }
    let l = laenge_aus(k)?;
    let px = match l.einheit {
        Einheit::Px | Einheit::Zahl => l.wert,
        Einheit::Em => l.wert * em,
        Einheit::Rem => l.wert * super::BASIS_SCHRIFT_PX,
        Einheit::Pt => l.wert * 4.0 / 3.0,
        Einheit::Prozent => l.wert / 100.0 * ansicht,
        Einheit::Vw => l.wert / 100.0 * ansicht,
        Einheit::Vmin => (l.wert / 100.0) * ansicht.min(ansicht_hoehe),
        Einheit::Vmax => (l.wert / 100.0) * ansicht.max(ansicht_hoehe),
        Einheit::Ex => l.wert * em * 0.5,
        _ => return None,
    };
    if !px.is_finite() || px < 0.0 {
        return None;
    }
    Some(px.clamp(0.0, crate::MAX_PX))
}

/// Die Zahl aus `repeat(N, ...)`. `None` bei `auto-fill`/`auto-fit`
/// oder wenn es keine Zahl ist.
fn repeat_zaehlen(t: &str) -> Option<i32> {
    let i = t.find(',')?;
    let n = t[t.find('(')? + 1..i].trim();
    // CSS Grid 1 §7.2.1: `repeat()` mit `auto-fill`/`auto-fit` ist
    // ausgeschlossen; alles andere muss eine positive Ganzzahl sein.
    match n.parse::<i32>() {
        Ok(z) if z > 0 => Some(z),
        // `repeat(0, ...)` und `repeat(-1, ...)` sind ungueltig
        // (§7.2.1: "must be a positive integer") — der Test
        // `c15_repeat_bombe` erwartet, dass NICHTS entsteht.
        _ => None,
    }
}

/// Der Teil nach `repeat(N, ` bis zur schliessenden Klammer.
///
/// # GRENZE
///
/// `MAX_RASTER_SPALTEN` (32) ist die aeussere Grenze; der Mutator des
/// Fuzzers kann eine Klammer offen lassen, und dann endet die Suche
/// am Textende statt zu laufen.
fn repeat_inhalt(t: &str) -> Option<String> {
    let i = t.find(',')?;
    let rest = &t[i + 1..];
    let zu = rest.rfind(')')?;
    let inhalt = rest[..zu].trim();
    if inhalt.is_empty() {
        return None;
    }
    Some(String::from(inhalt))
}

/// EIN Spaltenmass: eine Laenge (`Fest`) oder ein Anteil (`Anteil`).
///
/// CSS Grid 1 §7.2.1: `minmax()`, `auto`, `[name]` und `subgrid`
/// brauchen den Raster-Algorithmus und werden abgelehnt.
fn spalten_mass(t: &str, em: f32, ansicht: f32, ansicht_hoehe: f32) -> Option<SpaltenMass> {
    let k = t.trim();
    if k.is_empty() {
        return None;
    }
    if k.ends_with("fr") {
        let z = k[..k.len() - 2].trim().parse::<f32>().ok()?;
        if !z.is_finite() || z < 0.0 {
            return None;
        }
        return Some(SpaltenMass::Anteil(z));
    }
    // L4 (03.10.2026 05:4x): die `auto`-SPUR (§7.2).
    //
    // GEMESSENER BEFUND, dass die Form nicht ankam: `auto` fiel bis hier
    // in `laenge_aus`, das gibt `None`, und das `?` des Aufrufers hat die
    // GANZE Liste verworfen — GEMESSEN (Release, `l4_raster.rs` und der
    // Test `l4_auto_ist_eine_spur_und_keine_weglassung`): aus
    // `grid-template-columns: auto auto` wurde eine implizite Vollspalte.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Vergleich GLEICH und nicht
    // `contains`: `min-content` und `max-content` sind eigene Masse
    // (§7.2), und `contains` wuerde sie mitschneiden. Nur das ganze
    // Wort `auto` ist die Spur.
    if k == "auto" {
        return Some(SpaltenMass::Inhalt);
    }
    // 06.10.2026: `min-content`/`max-content` SIND SPUREN (§7.2).
    //
    // GEMESSENER FEHLER AM LEBENDEN BEISPIEL: Vector 2022 schreibt
    // `.mw-body{grid-template:min-content min-content min-content 1fr /
    // minmax(0,59.25rem) min-content}`. Der letzte Wert fiel hier durch
    // (`laenge_aus` gibt `None`), und das `?` des Aufrufers verwarf die
    // GANZE Liste — GEMESSEN `raster_spalten == []` fuer `.mw-body`,
    // waehrend `minmax(0,59.25rem)` allein tadellos ankommt. Folge: der
    // Artikel lag auf einer `auto`-Spur bei x=1227, hinter dem Rand.
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie auf `Inhalt` gehen und KEINE
    // eigene Variante bekommen: `Inhalt` ist die inhaltsgrosse Spur
    // dieses Motors (dieselbe Form wie `auto`, §7.2), und der Unterschied
    // zwischen `min-content` und `max-content` ist eine Messungsfeinheit,
    // die der Plan nicht traegt. Wer ihn braucht, macht daraus eine
    // eigene Variante — bis dahin ist „inhaltsgross statt die ganze
    // Deklaration verlieren" die bessere Antwort.
    if k == "min-content" || k == "max-content" {
        return Some(SpaltenMass::Inhalt);
    }
    // L4 (02.10.2026 08:6x): `minmax(min, max)`.
    //
    // GEMESSENER BEFUND, was hier vorher stand: `k.contains('(')`
    // hat ALLES mit Klammern abgelehnt, und `minmax` ist die
    // haeufigste Rasterzeile auf echten Seiten —
    // `repeat(auto-fit, minmax(270px, 1fr))` steht auf dem Korpus
    // 13-mal, und jede dieser 13 Zeilen war einspaltig.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `min` und `max` getrennt
    // gelesen werden und nicht als zwei Spalten: `minmax` ist EIN
    // Spurmass mit zwei Grenzen (CSS Grid 1 §7.2.2), keine
    // Nachbarschaft.
    if let Some(sp) = minmax_spalte(k, em, ansicht, ansicht_hoehe) {
        return Some(sp);
    }
    // L4 (03.10.2026 07:5x): CSS Values 4 §5.2 — auch eine Spur selbst
    // kann eine mathematische Funktion sein
    // (`grid-template-columns: max(100px, 20%)`).
    //
    // GEMESSENER BEFUND, dass die Form nicht ankam: `laenge_aus` unten
    // scheitert an `max(100px, 20%)`, also fiel die Spur und mit ihr die
    // ganze Liste.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das `spur_px` benutzt und nicht
    // `rechne` direkt: `spur_px` ist die EINE Stelle, an der eine Spur in
    // px wird, und sie kappt/prüft schon. Ein zweiter Rechenweg wäre eine
    // zweite Wahrheit ueber dieselbe Zahl.
    // 03.10.2026 (Rolle CSS): **`calc(` FEHLTE hier.** GEMESSEN:
    // `grid-template-columns: calc(100px + 20px) 1fr` ergab **null Spuren** —
    // mit UND ohne Leerzeichen im Ausdruck, es war also nicht das Zerlegen.
    // `spalten_mass` fiel am Klammerwächter darunter (`k.contains('(')` ->
    // `None`) durch und verwarf die GANZE Deklaration. Ein Grid ohne
    // Spalten hat genau eine implizite Spur, also stehen die Kinder
    // untereinander — das ist genau das Bild, das die Leitung als
    // "alles untereinander" beschreibt.
    //
    // CSS Grid 1 §7.2: `<track-size>` ist ein `<length>`, und eine Laenge
    // darf nach CSS Values 4 §5.2 ein `calc()` sein. `spur_px` kann es
    // schon (es fragt `hat_funktion(k, "calc(")`) — nur diese Abfrage hier
    // lief daran vorbei.
    if super::rechnen::hat_funktion(k, "calc(")
        || super::rechnen::hat_funktion(k, "min(")
        || super::rechnen::hat_funktion(k, "max(")
        || super::rechnen::hat_funktion(k, "clamp(")
    {
        let px = spur_px(k, em, ansicht, ansicht_hoehe)?;
        return Some(SpaltenMass::Fest(px));
    }
    if k.contains('(') || k.starts_with('[') {
        return None;
    }
    // CSS Grid 1 §7.2.1: eine Spalte ist eine Laenge. Die Umrechnung
    // ist dieselbe wie im Layout (`kaskade::laenge_px`), aber an dieser
    // Stelle ohne den Prozentsatz der Kastengroesse — ein Prozent in
    // `grid-template-columns` gibt es nur in `minmax()`, und das wird
    // abgelehnt.
    let l = laenge_aus(k)?;
    let px = match l.einheit {
        Einheit::Px | Einheit::Zahl => l.wert,
        Einheit::Em => l.wert * em,
        Einheit::Rem => l.wert * super::BASIS_SCHRIFT_PX,
        Einheit::Pt => l.wert * 4.0 / 3.0,
        // L4 (03.10.2026 09:3x): §10.1 — der Prozentsatz gehoert dem
        // CONTAINER. Bis eben stand hier `ansicht`, also wurde aus `50%`
        // im 600-px-Kasten die Zahl 640 (GEMESSEN, Release, Block Y2).
        Einheit::Prozent => return Some(SpaltenMass::Prozent(l.wert / 100.0)),
        Einheit::Vw => l.wert / 100.0 * ansicht,
        Einheit::Vmin => l.wert / 100.0 * ansicht.min(ansicht_hoehe),
        Einheit::Vmax => l.wert / 100.0 * ansicht.max(ansicht_hoehe),
        Einheit::Ex => l.wert * em * 0.5,
        // `vh` und `%` kommen in `grid-template-columns` nicht vor.
        _ => return None,
    };
    if !px.is_finite() || px < 0.0 {
        return None;
    }
    Some(SpaltenMass::Fest(px.clamp(0.0, crate::MAX_PX)))
}

// ===========================================================================
// KURZWERT-ZERLEGUNG NACH var() (W6C)
// ===========================================================================

/// Nach `var()`-Ersetzung ist ein Ganzwert entstanden, z. B.
/// `border-top: 1px solid red` anstelle von `var(--rahmen)`. Die
/// Langformen brauchen daraus IHREN Teil.
///
/// # WARUM DAS EINE EIGENE FUNKTION IST
///
/// `lang_wert` bekommt nur die EIGENSCHAFT, nicht den Zusammenhang.
/// Nach einer Ersetzung steht aber der volle Kurztext in der
/// Langform — und die Kurzform-Zerlegung laeuft ueber
/// `kurz_auswerten`, das den Ausdruck-Kontext nicht kennt.
///
/// Rueckgabe None = „gehoert nicht zu dieser Langform"; der Aufrufer
/// faellt dann auf den normalen Einzelweg zurueck.
pub(crate) fn border_teil_fuer(
    e: Eigenschaft,
    k: &str,
    _ctx: &crate::rechnen::RechenCtx,
) -> Option<Wert> {
    let (soll, seite) = rahmen_teil_art(e)?;
    let teile = teile_wert(k);
    for t in teile.iter() {
        match soll {
            // Breite: `0`/`none` auch, `thin`/`medium`/`thick` (§2.1).
            0 => {
                if seite && ist_seiten_wort(t) {
                    continue;
                }
                if rahmenbreite_wert(t).is_some() {
                    return rahmenbreite_wert(t).map(Wert::Laenge);
                }
            }
            // Stil (CSS 2.1 §8.5).
            1 => {
                if rahmenstil_wert(t).is_some() {
                    return Some(Wert::Wort(String::from(rahmenstil_wert(t)?)));
                }
            }
            // Farbe.
            _ => {
                if farbe_aus(t).is_some() {
                    return farbe_aus(t).map(Wert::Farbe);
                }
                if *t == "currentcolor" {
                    return Some(Wert::Aktuell);
                }
            }
        }
    }
    None
}

/// Welche ART von Rahmen-Langform ist `e`, und ist es eine
/// Seitenangabe? (0 = Breite, 1 = Stil, 2 = Farbe)
fn rahmen_teil_art(e: Eigenschaft) -> Option<(u8, bool)> {
    Some(match e {
        RahmenObenBreite | RahmenRechtsBreite | RahmenUntenBreite | RahmenLinksBreite => (0, true),
        RahmenObenStil | RahmenRechtsStil | RahmenUntenStil | RahmenLinksStil => (1, true),
        RahmenObenFarbe | RahmenRechtsFarbe | RahmenUntenFarbe | RahmenLinksFarbe => (2, true),
        _ => return None,
    })
}

/// `thin`/`medium`/`thick` sind Breiten, keine Stile (CSS 2.1 §8.5).
fn ist_seiten_wort(t: &str) -> bool {
    matches!(t, "thin" | "medium" | "thick")
}

/// Nach `var()`-Ersetzung: `font: bold 12px/1.5 serif` anstelle von
/// `var(--schrift)` — die Langformen brauchen ihren Teil.
///
/// CSS 2.1 §15.8 (die `font`-Kurzform) in der Reihenfolge, die dort
/// steht: Stil, Gewicht, Groesse/Hoehe, Familie.
pub(crate) fn font_teil_fuer(
    e: Eigenschaft,
    k: &str,
    ctx: &crate::rechnen::RechenCtx,
) -> Option<Wert> {
    let teile = teile_wert(k);
    for t in teile.iter() {
        match e {
            // `Schriftstil` muss qualifiziert werden: ohne `typen::`
            // ist es die `Schriftstil`-Konstante aus `typen::*`, kein
            // Muster — dieselbe Falle wie bei `BreiteArt` (K7).
            crate::typen::Eigenschaft::Schriftstil => {
                if matches!(*t, "normal" | "italic" | "oblique") {
                    return Some(Wert::Wort(String::from(if *t == "italic" {
                        "kursiv"
                    } else {
                        "schraeg"
                    })));
                }
            }
            Schriftgewicht => {
                if let Some(g) = schriftgewicht_wert(t) {
                    return Some(g);
                }
            }
            Schriftgroesse | Zeilenhoehe => {
                // `12px/1.5` ist Groesse und Zeilenhoehe in EINEM Teil.
                let (groesse, hoehe) = match t.split_once('/') {
                    Some((g, h)) => (g, Some(h)),
                    None => (*t, None),
                };
                let ist_groesse = e == Schriftgroesse;
                let passend = if ist_groesse {
                    Some(groesse)
                } else {
                    hoehe.or(Some(groesse))
                };
                if let Some(x) = passend {
                    if ist_groesse {
                        if let Some(v) = schriftgroesse_wert(x, ctx) {
                            return Some(v);
                        }
                    } else if let Some(v) = zeilenhoehe_wert(x, ctx) {
                        return Some(v);
                    }
                }
            }
            crate::typen::Eigenschaft::Schriftfamilie => {
                // Die Familie ist der REST nach Stil, Gewicht und
                // Groesse (CSS 2.1 §15.8) — nicht das erste Wort.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): es
                // stand hier das ERSTE unpassende Teil, und
                // `font: italic bold 12px/1.5 serif` lieferte als
                // Familie `italic`; der Test `font_var_ganz` hat es
                // gefangen. Und `Schriftfamilie` stand hier OHNE
                // `crate::typen::` — das ist dieselbe Falle wie bei
                // `BreiteArt` (K7) und `SchriftGroesse`: eine nicht
                // importierte Variante ist eine BINDUNG, und die
                // passt auf jeden Wert.
                let vorbei = teile.iter().position(|x| {
                    !x.contains('/')
                        && !matches!(
                            *x,
                            "normal"
                                | "italic"
                                | "oblique"
                                | "bold"
                                | "bolder"
                                | "lighter"
                                | "100"
                                | "200"
                                | "300"
                                | "400"
                                | "500"
                                | "600"
                                | "700"
                                | "800"
                                | "900"
                        )
                        && laenge_aus(x).is_none()
                });
                if let Some(i) = vorbei {
                    let fam = teile[i..].join(" ");
                    if !fam.is_empty() && !fam.contains("var(") {
                        return Some(Wert::Wort(fam));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

// ===========================================================================
// WERTE DER SCHRIFT-LANGFORMEN (CSS 2.1, §15)
// ===========================================================================

/// `font-size` (CSS 2.1 §15.8) — absolute Groessen, `em`, `%` und die
/// `larger`/`smaller`-Paare (ab CSS 3 §4.3: die relativen Namen, die
/// der Browser auf die Schriftgroesse des Eltern abbildet).
///
/// Die GROESSEN-Namen (`small`, `medium`, `large`, ...) sind
/// browserabhaengig; dieser Browser rechnet sie NICHT, weil er keine
/// Standardschrift mit festen Werten hat. Sie werden als Wort
/// durchgelassen und der Aufrufer entscheidet.
fn schriftgroesse_wert(klein: &str, ctx: &crate::rechnen::RechenCtx) -> Option<Wert> {
    if klein == "inherit" || klein == "initial" || klein == "unset" || klein == "revert" {
        return None;
    }
    // CSS 3 §4.3: `larger`/`smaller` = ein Schritt in der Schriftgroessen-
    // Tabelle. Ohne Tabelle ist das geraten; deshalb hier nur lesbar
    // machen und dem Aufruber geben.
    if klein == "larger" || klein == "smaller" {
        return Some(Wert::Wort(String::from(klein)));
    }
    // `rechne` entscheidet: es kennt `var()` und `calc()`, `laenge_aus`
    // nicht. Die Geprueftheit der Einheit kommt deshalb von `rechne`.
    let px = crate::rechnen::rechne(&klein, ctx, 0)?;
    Some(Wert::Laenge(Laenge {
        wert: px,
        einheit: Einheit::Px,
    }))
}

/// `font-weight` (CSS 2.1 §15.6): `normal` = 400, `bold` = 700,
/// `bolder`/`lighter` = relativ zum Elternwert (im Wort weitergelassen,
/// weil die Kaskade den Elternwert kennt).
fn schriftgewicht_wert(klein: &str) -> Option<Wert> {
    let z = match klein {
        "normal" => 400u16,
        "bold" => 700,
        "bolder" | "lighter" => return Some(Wert::Wort(String::from(klein))),
        _ => {
            let p = klein.parse::<u16>().ok()?;
            // CSS Fonts 3 §2.1 (2009) erlaubte nur Vielfache von 100.
            // CSS Fonts 4 §2.1 (2024) hat das auf 1..1000 JEDE Zahl
            // geoeffnet — variable Schriften haben Gewichte dazwischen.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): `font-weight: 425`
            // (1x auf den 35 echten Stylesheets) fiel nach der 2009er
            // Regel weg. Das ist keine Ausnahme, sondern die Regel von
            // heute.
            if !(1..=1000).contains(&p) {
                return None;
            }
            p
        }
    };
    Some(Wert::Laenge(Laenge {
        wert: z as f32,
        einheit: Einheit::Zahl,
    }))
}

/// `line-height` (CSS 2.1 §15.5): Zahl (Faktor), `normal`,
/// Laenge oder Prozent.
fn zeilenhoehe_wert(klein: &str, ctx: &crate::rechnen::RechenCtx) -> Option<Wert> {
    if klein == "normal" {
        return Some(Wert::Wort(String::from("normal")));
    }
    // Ein Faktor: eine nackte Zahl OHNE Einheit.
    if !klein.ends_with('%') && !klein.contains("px") && !klein.contains(char::is_alphabetic) {
        let z = klein.parse::<f32>().ok()?;
        if z.is_finite() && z > 0.0 {
            return Some(Wert::Laenge(Laenge {
                wert: z,
                einheit: Einheit::Zahl,
            }));
        }
    }
    // `rechne` entscheidet: es kennt `var()` und `calc()`.
    let px = crate::rechnen::rechne(&klein, ctx, 0)?;
    Some(Wert::Laenge(Laenge {
        wert: px,
        einheit: Einheit::Px,
    }))
}

/// `font-family` (CSS 2.1 §15.5): eine Liste von Namen, jede optional
/// in Anfuehrungszeichen.
///
/// Der Name wird als TET zurueckgegeben — es ist eine Liste von
/// Namen, keine Messung, und die Kaskade kennt keine Schriftdateien.
fn schriftfamilie_wert(klein: &str) -> Option<Wert> {
    let t = klein.trim();
    if t.is_empty() {
        return None;
    }
    if t == "inherit" || t == "initial" || t == "unset" {
        return None;
    }
    Some(Wert::Wort(String::from(t)))
}

/// `text-decoration` (CSS 2.1 §15.2): eine beliebige Kombination aus
/// `none`, `underline`, `overline`, `line-through`, `blink`.
fn textdekoration_wert(klein: &str) -> Option<Wert> {
    let teile = teile_wert(klein);
    if teile.is_empty() {
        return None;
    }
    let mut gefunden = false;
    for t in teile.iter() {
        match *t {
            // CSS Text Decoration 3 §2: `dotted`, `dashed`, `wavy` sind
            // die STILE der Unterstreichung. Sie werden erkannt und
            // UEBERSPRUNGEN — der berechnete Wert der Kurzform ist die
            // Linienart, und die steht im Wort.
            "none" | "underline" | "overline" | "line-through" | "blink" => gefunden = true,
            // GEMESSENER FEHLER 01.10.2026 (K7c): `underline dotted`
            // fiel durch, weil nur die reinen Linienarten genannt waren.
            // Auf wikipedia und github kommt die Kombination
            // 4x vor — die Linienart `dotted` waere verloren.
            "solid" | "double" | "dotted" | "dashed" | "wavy" => gefunden = true,
            _ => return None,
        }
    }
    if !gefunden {
        return None;
    }
    Some(Wert::Wort(String::from(klein)))
}

/// L4 (13:0x): die Woerter der Box-Ausrichtung (§6.1) auf die INNEREN
/// Woerter der Kaskade. `None` = unbekannt, und dann faellt die ganze
/// Kurzform weg.
///
/// GEMESSENE ENTSCHEIDUNG, warum `normal` wie `stretch` behandelt wird
/// und nicht wie `auto`: §6.2 nennt `normal` fuer `align-self`/
/// `justify-self` gleichbedeutend mit `stretch`, `auto` ist der
/// **Anfangswert**. Wer `place-self: normal` schreibt, will fuellen und
/// nicht „den Wert des Containers uebernehmen".
fn ausrichtung_wort(w: &str) -> Option<&'static str> {
    match w {
        "start" | "flex-start" | "self-start" => Some("start"),
        "mitte" | "center" | "centre" => Some("mitte"),
        "ende" | "end" | "flex-end" | "self-end" => Some("ende"),
        "strecke" | "stretch" | "normal" => Some("strecke"),
        "grundlinie" | "baseline" => Some("grundlinie"),
        _ => None,
    }
}

fn kurz_auswerten(k: Kurz, klein: &str, wichtig: bool, aus: &mut Vec<Deklaration>) {
    match k {
        // `margin` / `padding` (CSS 2.1 §8.3, §8.2): 1 bis 4 Werte mit
        // der Auslassregel 1/2/3/4. `auto` ist bei `margin` erlaubt und
        // bei `padding` nicht (§8.2).
        Kurz::Marge | Kurz::Fuellung => {
            let auto_ok = matches!(k, Kurz::Marge);
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 4 {
                return;
            }
            let mut werte: Vec<(Wert, String)> = Vec::new();
            for t in teile {
                if t == "auto" && auto_ok {
                    werte.push((Wert::Auto, String::from(t)));
                } else if t == "auto" {
                    return;
                } else if let Some(l) = laenge_aus(t) {
                    // Release 12.10.2026 (Agent CSS2): negatives `padding`
                    // verwirft die ganze Kurzform (CSS 2.1 §8.4).
                    if !auto_ok && l.wert < 0.0 {
                        return;
                    }
                    werte.push((Wert::Laenge(l), String::from(t)));
                } else if enthaelt_var(t) || enthaelt_mathe(t) {
                    werte.push((spaet(), String::from(t)));
                } else {
                    return;
                }
            }
            // CSS 2.1 §8.3 / CSS Positioned Layout 3 §4.1: 1 = alle,
            // 2 = OBEN/UNTEN | LINKS/RECHTS, 3 = oben | links+rechts |
            // unten, 4 = je Seite.
            //
            // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): bei ZWEI
            // Werten stand hier [0,1,1,0] — das ist die Regel von
            // `border-radius` (§5.4, die horizontalen Radien), nicht
            // die von `margin`. `margin:1px 2px` ergab (1,2,2,1) statt
            // (1,2,1,2); der Test `margin_kurz` hat es gefangen.
            let idx: [usize; 4] = match werte.len() {
                1 => [0, 0, 0, 0],
                2 => [0, 1, 0, 1],
                // DREI Werte: oben | LINKS+RECHTS | unten.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): hier
                // stand [0,1,1,2] — das ist die Regel von
                // `border-radius` (§5.4), nicht die von `margin`.
                // `margin: 1em 2em 3em` ergab (1,2,2,3) statt
                // (1,2,3,2). Der Test `archlinux_p_hat_drei_margin_
                // werte` hat es gefangen.
                3 => [0, 1, 2, 1],
                _ => [0, 1, 2, 3],
            };
            let ziele: [Eigenschaft; 4] = if matches!(k, Kurz::Marge) {
                [MargeOben, MargeRechts, MargeUnten, MargeLinks]
            } else {
                [FuellungOben, FuellungRechts, FuellungUnten, FuellungLinks]
            };
            for (nr, e) in ziele.iter().enumerate() {
                let (w, r) = &werte[idx[nr]];
                aus.push(dekl(*e, w.clone(), wichtig, r));
            }
        }
        // `margin-inline`/`-block` und `padding-inline`/`-block`
        // (CSS Box Sizing 4 §2): ZWEI Werte, keine Auslassung.
        Kurz::MargeInline | Kurz::FuellungInline | Kurz::MargeBlock | Kurz::FuellungBlock => {
            let auto_ok = matches!(k, Kurz::MargeInline | Kurz::MargeBlock);
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 2 {
                return;
            }
            let mut werte: Vec<(Wert, String)> = Vec::new();
            for t in teile {
                if t == "auto" && auto_ok {
                    werte.push((Wert::Auto, String::from(t)));
                } else if t == "auto" {
                    return;
                } else if let Some(l) = laenge_aus(t) {
                    // Release 12.10.2026 (Agent CSS2): siehe `padding`.
                    if !auto_ok && l.wert < 0.0 {
                        return;
                    }
                    werte.push((Wert::Laenge(l), String::from(t)));
                } else if enthaelt_var(t) || enthaelt_mathe(t) {
                    werte.push((spaet(), String::from(t)));
                } else {
                    return;
                }
            }
            let (a, b) = if werte.len() == 1 {
                (werte[0].clone(), werte[0].clone())
            } else {
                (werte[0].clone(), werte[1].clone())
            };
            let paare: [(Eigenschaft, (Wert, String)); 2] = match k {
                Kurz::MargeInline => [(EMargeInnenStart, a.clone()), (EMargeInnenEnde, b.clone())],
                Kurz::FuellungInline => [
                    (EFuellungInnenStart, a.clone()),
                    (EFuellungInnenEnde, b.clone()),
                ],
                Kurz::MargeBlock => [(MargeOben, a.clone()), (MargeUnten, b.clone())],
                _ => [(FuellungOben, a.clone()), (FuellungUnten, b.clone())],
            };
            for (e, (w, r)) in paare {
                aus.push(dekl(e, w, wichtig, &r));
            }
        }
        // `inset` (CSS Positioned Layout 3 §4.1) = `top`/`right`/
        // `bottom`/`left` mit der Auslassregel von `margin`. `auto` ist
        // erlaubt.
        Kurz::Inset => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 4 {
                return;
            }
            let mut werte: Vec<(Wert, String)> = Vec::new();
            for t in teile {
                if t == "auto" {
                    werte.push((Wert::Auto, String::from(t)));
                } else if let Some(l) = laenge_aus(t) {
                    werte.push((Wert::Laenge(l), String::from(t)));
                } else if enthaelt_var(t) || enthaelt_mathe(t) {
                    werte.push((spaet(), String::from(t)));
                } else {
                    return;
                }
            }
            // CSS 2.1 §8.3 / CSS Positioned Layout 3 §4.1: 1 = alle,
            // 2 = OBEN/UNTEN | LINKS/RECHTS, 3 = oben | links+rechts |
            // unten, 4 = je Seite.
            //
            // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): bei ZWEI
            // Werten stand hier [0,1,1,0] — das ist die Regel von
            // `border-radius` (§5.4, die horizontalen Radien), nicht
            // die von `margin`. `margin:1px 2px` ergab (1,2,2,1) statt
            // (1,2,1,2); der Test `margin_kurz` hat es gefangen.
            let idx: [usize; 4] = match werte.len() {
                1 => [0, 0, 0, 0],
                2 => [0, 1, 0, 1],
                // DREI Werte: oben | LINKS+RECHTS | unten.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): hier
                // stand [0,1,1,2] — das ist die Regel von
                // `border-radius` (§5.4), nicht die von `margin`.
                // `margin: 1em 2em 3em` ergab (1,2,2,3) statt
                // (1,2,3,2). Der Test `archlinux_p_hat_drei_margin_
                // werte` hat es gefangen.
                3 => [0, 1, 2, 1],
                _ => [0, 1, 2, 3],
            };
            for (nr, e) in [Oben, Rechts, Unten, Links].iter().enumerate() {
                let (w, r) = &werte[idx[nr]];
                aus.push(dekl(*e, w.clone(), wichtig, r));
            }
        }
        // `overflow: [<x>] [<y>]` (CSS Overflow 3 §3.1): EIN Wert gilt
        // fuer beide Achsen, ZWEI je eine.
        // L4 (13:0x): `place-items` und `place-self` (§7.1) — EIN Wert
        // gilt fuer beide Achsen, ZWEI je eine (derselbe Aufbau wie
        // `overflow`).
        Kurz::PlaceItems | Kurz::PlaceSelf => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 2 {
                return;
            }
            let achsen = if k == Kurz::PlaceItems {
                [EFlexAusrichtung, EJustifyAusrichtung]
            } else {
                [EAusrichtungSelbst, EAusrichtungSelbstWaagerecht]
            };
            for e in achsen {
                // GEMESSENER FEHLER: `for e in achsen` liefert bei einem
                // Array die WERTE, kein `&Eigenschaft` — `*e` gab
                // deshalb E0614.
                let t = if e == achsen[1] && teile.len() == 2 {
                    teile[1]
                } else {
                    teile[0]
                };
                // GEMESSENE ENTSCHEIDUNG, warum hier verworfen wird und
                // nicht der Anfangswert: `place-items: safe center` darf
                // nicht eine Achse setzen und die andere leer lassen.
                match ausrichtung_wort(t) {
                    Some(w) => aus.push(dekl(e, Wert::Wort(String::from(w)), wichtig, t)),
                    None => return,
                }
            }
        }
        // `gap` mit einem Wert wie bisher (`ELuecke` setzt beide Achsen),
        // mit zwei Werten zuerst beide auf den ersten (Zeile), dann die
        // Spalte auf den zweiten. Ein ungueltiger Teil verwirft alles.
        Kurz::Luecke => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 2 {
                return;
            }
            // `var()`/`calc()` werden spaet ausgewertet wie bei `margin`.
            let wert_von = |t: &str| {
                lang_wert(ELuecke, t).or_else(|| (enthaelt_var(t) || enthaelt_mathe(t)).then(spaet))
            };
            let Some(erst) = wert_von(teile[0]) else { return };
            let zweit = match teile.get(1) {
                Some(t) => match wert_von(t) {
                    Some(w) => Some((w, *t)),
                    None => return,
                },
                None => None,
            };
            aus.push(dekl(ELuecke, erst, wichtig, teile[0]));
            if let Some((w, t)) = zweit {
                aus.push(dekl(ELueckeSpalte, w, wichtig, t));
            }
        }
        Kurz::Overflow => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 2 {
                return;
            }
            if teile
                .iter()
                .any(|t| !matches!(*t, "visible" | "hidden" | "clip" | "scroll" | "auto"))
            {
                return;
            }
            for e in [EOverflowX, EOverflowY].iter() {
                let t = if *e == EOverflowY && teile.len() == 2 {
                    teile[1]
                } else {
                    teile[0]
                };
                aus.push(dekl(*e, Wert::Wort(String::from(t)), wichtig, t));
            }
        }
        // CSS 2.1 §17.6: `border-spacing: [<h>] [<v>]` — EIN Wert gilt fuer
        // beide Achsen, ZWEI je eine. Dieselbe Form wie `overflow` daruber.
        //
        // 03.10.2026 (Rolle CSS), GEMESSENE LUECKE: vorher stand hier
        // `Ziel::Lang(ESchluessel)` — die Zahl kam nicht an, und `cellspacing`
        // (4x auf den zehn Referenzseiten) hatte consequently keine Stelle.
        Kurz::RandAbstand => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 2 {
                return;
            }
            // Release 14.10.2026 (Agent CSS2): both parts are checked before
            // anything is pushed, and percentages (or unitless numbers other
            // than 0) are no lengths here (CSS 2.1 §17.6.1). MEASURED before
            // (WPT `CSS2/tables/border-spacing-percentage-001`, `border-spacing:
            // 20%`): the percentage was taken as a spacing.
            let mut werte = Vec::with_capacity(2);
            for (nr, e) in [ERandAbstandW, ERandAbstandS].iter().enumerate() {
                let t = teile[nr.min(teile.len() - 1)];
                // §17.6: nur nichtnegative Laengen. Alles andere verwirft die
                // Deklaration ganz, statt einen Ersatz zu erfinden.
                match laenge_aus(t) {
                    Some(l)
                        if l.wert >= 0.0
                            && l.einheit != Einheit::Prozent
                            && (l.einheit != Einheit::Zahl || l.wert == 0.0) =>
                    {
                        werte.push(dekl(*e, Wert::Laenge(l), wichtig, t));
                    }
                    _ => return,
                }
            }
            aus.extend(werte);
        }
        // `border-block` / `border-inline` (CSS Box Sizing 4 §2).
        Kurz::RahmenLogischPaar | Kurz::RahmenLogischPaarInline => {
            let (a, b) = if k == Kurz::RahmenLogischPaar {
                (Oben, Unten)
            } else {
                (Links, Rechts)
            };
            if !rahmen_logisch_zerlegen(klein, a, b, wichtig, aus) {
                return;
            }
        }
        // `border` und die drei Ein-Seiten-Formen (CSS 2.1 §8.5).
        //
        // Die Teile sind Breite, Stil und Farbe in BELIEBIGER Reihen-
        // folge, jede optional (CSS 2.1 §8.5). `none` als Breite heisst
        // 0; `none` als Farbe heisst die eigene Farbe (§8.5.2).
        Kurz::Rahmen
        | Kurz::RahmenOben
        | Kurz::RahmenRechts
        | Kurz::RahmenUnten
        | Kurz::RahmenLinks => {
            // `border: var(--x)` ist die HAEUFIGSTE Form auf modernen
            // Seiten (github.com: `--border-default: var(--borderWidth-
            // default) solid var(--borderColor-default)`).
            //
            // Deshalb werden bei `var()`/`calc()` ALLE DREI Langformen
            // mit dem Platzhalter ausgegeben — jede mit dem GANZEN Text
            // als Rohtext. Nur so kann die Kaskade nach der Ersetzung
            // zerlegen (`border_teil_fuer`) und so kann ein FEHLSCHLAG
            // alle drei gemeinsam treffen (CSS Variables 1 §3.2).
            //
            // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
            // Editierfehler um 04:50): es wurde nur die Breite
            // ausgegeben. `border: var(--r)` lieferte Breite 2 px,
            // Stil `Kein` und Farbe Schwarz — der Rahmen wurde also
            // NICHT gezeichnet. Der Test `border_var_ganz` hat es
            // gefangen.
            if enthaelt_var(klein) || enthaelt_mathe(klein) {
                let seiten0: [Eigenschaft; 4] = match k {
                    Kurz::RahmenOben => [Oben, Oben, Oben, Oben],
                    Kurz::RahmenRechts => [Rechts, Rechts, Rechts, Rechts],
                    Kurz::RahmenUnten => [Unten, Unten, Unten, Unten],
                    Kurz::RahmenLinks => [Links, Links, Links, Links],
                    _ => [Oben, Rechts, Unten, Links],
                };
                for seite in seiten0.iter() {
                    aus.push(dekl(breite_e(*seite), spaet(), wichtig, klein));
                    aus.push(dekl(
                        match *seite {
                            Oben => RahmenObenStil,
                            Rechts => RahmenRechtsStil,
                            Unten => RahmenUntenStil,
                            _ => RahmenLinksStil,
                        },
                        spaet(),
                        wichtig,
                        klein,
                    ));
                    aus.push(dekl(farbe_e(*seite), spaet(), wichtig, klein));
                }
                return;
            }
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 3 {
                return;
            }
            let mut breite: Option<(Wert, String)> = None;
            let mut stil: Option<(Wert, String)> = None;
            let mut farbe: Option<(Wert, String)> = None;
            for t in teile.iter() {
                let t: &str = t;
                let t: &str = t;
                if farbe.is_none() && t != "none" && !t.contains("var(") && !t.contains("calc(") {
                    if let Some(f) = farbe_aus(t) {
                        farbe = Some((Wert::Farbe(f), String::from(t)));
                        continue;
                    }
                }
                if breite.is_none() {
                    if let Some(b) = rahmenbreite_wert(t) {
                        breite = Some((Wert::Laenge(b), String::from(t)));
                        continue;
                    }
                }
                if stil.is_none() {
                    if let Some(st) = rahmenstil_wert(t) {
                        // Release 15.10.2026 (Agent CSS2): `none`/`hidden` is
                        // the STYLE (it took the width slot before, so `none
                        // 0px` had no place for the `0px` and the whole
                        // declaration was dropped). A missing width is then 0
                        // (below). MEASURED before (WPT `CSS2/positioning/
                        // absolute-replaced-height-008` reference, `border-
                        // top: none 0px` over `border-top: black solid
                        // medium`): the 3px border stayed.
                        stil = Some((Wert::Wort(String::from(st)), String::from(t)));
                        continue;
                    }
                }
                // `var()`/`calc()` wird erst in der Kaskade aufgeloest
                // (C10/C11); die Kurzform bekommt deshalb einen
                // PLATZHALTER in dem Feld, das noch frei ist — sonst
                // faellt die ganze Kurzform weg.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
                // Editierfehler): der Zweig setzte nur die Breite. Bei
                // `border: 2px solid var(--akzent)` war die Breite
                // schon gesetzt, also kam `return` — und die Regel
                // verschwand. Das ist der haeufigste Fall auf github.
                if enthaelt_var(t) || enthaelt_mathe(t) {
                    if breite.is_none() {
                        breite = Some((spaet(), String::from(t)));
                        continue;
                    }
                    if stil.is_none() {
                        stil = Some((spaet(), String::from(t)));
                        continue;
                    }
                    if farbe.is_none() {
                        farbe = Some((spaet(), String::from(t)));
                        continue;
                    }
                }
                return;
            }
            if breite.is_none() && stil.is_none() && farbe.is_none() {
                return;
            }
            // Release 10.10.2026: die Kurzform setzt eine fehlende Breite auf
            // ihren Anfangswert `medium` (3 px, CSS Backgrounds 3 §3.6).
            // GEMESSEN vorher (Web-Platform-Tests): `border: solid black`
            // zeichnete keinen Rahmen. (Fehlt der Stil, bleibt es bei keiner
            // Breite — der Stil `none` macht sie ohnehin 0.)
            // `none`/`hidden` without a width keeps the old behaviour: width 0.
            let stil_kein = stil.as_ref().is_some_and(|(w, _)| matches!(w, Wert::Wort(x) if x == "kein"));
            if breite.is_none() && stil_kein {
                breite = Some((Wert::Laenge(Laenge { wert: 0.0, einheit: Einheit::Px }), String::from("0")));
            }
            if breite.is_none() && stil.is_some() {
                breite = Some((Wert::Laenge(Laenge { wert: 3.0, einheit: Einheit::Px }), String::from("medium")));
            }
            // Release 14.10.2026 (Agent CSS2): the shorthand also resets the
            // style it does not name to `none` and the colour to
            // `currentColor` (CSS Backgrounds 3 §4.4). MEASURED before (WPT
            // `CSS2/borders/border-shorthands-002`, `.test { border: solid 1em }`
            // after `div { border-color: red }`): the border stayed red.
            if stil.is_none() {
                stil = Some((Wert::Wort(String::from("kein")), String::from("none")));
            }
            if farbe.is_none() {
                farbe = Some((Wert::Aktuell, String::from("currentcolor")));
            }
            let seiten: [Eigenschaft; 4] = match k {
                Kurz::RahmenOben => [Oben, Oben, Oben, Oben],
                Kurz::RahmenRechts => [Rechts, Rechts, Rechts, Rechts],
                Kurz::RahmenUnten => [Unten, Unten, Unten, Unten],
                Kurz::RahmenLinks => [Links, Links, Links, Links],
                _ => [Oben, Rechts, Unten, Links],
            };
            for seite in seiten.iter() {
                if let Some((w, _r)) = &breite {
                    // DER ROHTEXT IST DER GANZE WERT, nicht der Teil.
                    //
                    // Grund (CSS Variables 1 §3.2, und der Kommentar im
                    // Test `ohne_die_variable_faellt_die_ganze_kurzform_
                    // weg`): `border: 2px solid var(--akzent)` wird als
                    // GANZES verzögert und nach der Ersetzung neu
                    // zerlegt (`border_teil_fuer`). Schlaegt die
                    // Ersetzung fehl, ist die Deklaration „unguelt zur
                    // berechneten Zeit" — und dann gilt fuer JEDE
                    // Langform ihr Anfangswert, auch fuer die Breite.
                    //
                    // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion):
                    // mit dem Teil-Text bekam die Breite 2 px, auch
                    // ohne die Variable. Der Test hat es gefangen.
                    aus.push(dekl(breite_e(*seite), w.clone(), wichtig, klein));
                }
                if let Some((w, _r)) = &stil {
                    aus.push(dekl(
                        match *seite {
                            Oben => RahmenObenStil,
                            Rechts => RahmenRechtsStil,
                            Unten => RahmenUntenStil,
                            _ => RahmenLinksStil,
                        },
                        w.clone(),
                        wichtig,
                        klein,
                    ));
                }
                if let Some((w, _r)) = &farbe {
                    aus.push(dekl(farbe_e(*seite), w.clone(), wichtig, klein));
                }
            }
        }
        // `border-width`/`-style`/`-color` (CSS 2.1 §8.3, §8.5, §8.5.2):
        // dieselbe Form wie `margin`, aber je Teil nur EINE Art.
        Kurz::RahmenBreite | Kurz::RahmenStil | Kurz::RahmenFarbe => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 4 {
                return;
            }
            let mut werte: Vec<(Wert, String)> = Vec::new();
            for t in teile.iter() {
                let t: &str = t;
                let t: &str = t;
                let w = match k {
                    Kurz::RahmenBreite => rahmenbreite_wert(t)
                        .map(Wert::Laenge)
                        .or_else(|| if t == "none" { Some(zahl_null()) } else { None }),
                    Kurz::RahmenStil => rahmenstil_wert(t).map(|x| Wert::Wort(String::from(x))),
                    _ => {
                        if t == "none" || t == "currentcolor" {
                            Some(Wert::Aktuell)
                        } else {
                            farbe_aus(t).map(Wert::Farbe)
                        }
                    }
                };
                match w {
                    Some(x) => werte.push((x, klein.to_string())),
                    // `var()`/`calc()` wird erst in der Kaskade
                    // aufgeloest (CSS Variables 1 §3.2).
                    //
                    // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion):
                    // `border-color: var(--akzent)` kam hier als None
                    // an und die ganze Langformen-Kurzform fiel weg —
                    // der Rahmen bliebe schwarz. Der Test
                    // `die_langform_mit_var_gibt_dieselbe_farbe` hat es
                    // gefangen.
                    None if enthaelt_var(t) || enthaelt_mathe(t) => {
                        werte.push((spaet(), klein.to_string()))
                    }
                    None => return,
                }
            }
            // CSS 2.1 §8.3 / CSS Positioned Layout 3 §4.1: 1 = alle,
            // 2 = OBEN/UNTEN | LINKS/RECHTS, 3 = oben | links+rechts |
            // unten, 4 = je Seite.
            //
            // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): bei ZWEI
            // Werten stand hier [0,1,1,0] — das ist die Regel von
            // `border-radius` (§5.4, die horizontalen Radien), nicht
            // die von `margin`. `margin:1px 2px` ergab (1,2,2,1) statt
            // (1,2,1,2); der Test `margin_kurz` hat es gefangen.
            let idx: [usize; 4] = match werte.len() {
                1 => [0, 0, 0, 0],
                2 => [0, 1, 0, 1],
                // DREI Werte: oben | LINKS+RECHTS | unten.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): hier
                // stand [0,1,1,2] — das ist die Regel von
                // `border-radius` (§5.4), nicht die von `margin`.
                // `margin: 1em 2em 3em` ergab (1,2,2,3) statt
                // (1,2,3,2). Der Test `archlinux_p_hat_drei_margin_
                // werte` hat es gefangen.
                3 => [0, 1, 2, 1],
                _ => [0, 1, 2, 3],
            };
            for (nr, seite) in [Oben, Rechts, Unten, Links].iter().enumerate() {
                let (w, r) = &werte[idx[nr]];
                let e = match k {
                    Kurz::RahmenBreite => breite_e(*seite),
                    Kurz::RahmenStil => match *seite {
                        Oben => RahmenObenStil,
                        Rechts => RahmenRechtsStil,
                        Unten => RahmenUntenStil,
                        _ => RahmenLinksStil,
                    },
                    _ => farbe_e(*seite),
                };
                aus.push(dekl(e, w.clone(), wichtig, r));
            }
        }
        // `border-radius` (CSS Backgrounds 3 §5.4): EIN Wert gilt fuer
        // alle vier Ecken, ZWEI bis VIER mit der Auslassregel. Die Werte
        // VOR dem Schraegstrich sind die horizontalen Radien.
        //
        // WARUM eine Kurzform und kein eigenes Ziel: die Kaskade haelt
        // je EIGENSCHAFT ein Feld. Mit einer eigenen Variante fuer
        // `border-radius` stehen Kurzform und Ecke in zwei Feldern —
        // und beide gewinnen ihr Feld fuer sich. GEMESSEN als
        // `border-radius: 5px; border-top-left-radius: 9px` ergab
        // (1,5,5,5) statt (9,5,5,5).
        Kurz::Radius => {
            // `calc()`/`var()` VOR der Zerlegung: `teile_wert` trennt
            // `calc(var(--a) - var(--b))` an den Leerstellen des
            // Rechenzeichens in drei Stuecke, und das mittlere `-` ist
            // keine Laenge — die ganze Angabe fiel dann weg.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 3x
            // `border-radius: calc(var(--borderradius-medium) -
            // var(--segmented-control-outer-radius))` auf den 35 echten
            // Stylesheets.
            // Der Schraegstrich wird hier NICHT geprueft: in einem
            // Rechenausdruck ist er eine Division
            // (`calc(var(--a) - var(--b)/2)`), und die elliptische Form
            // (`10px / 20px`) ist keine Rechnung.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7c): 3x genau diese Form
            // auf den 35 echten Stylesheets (GitHub).
            if enthaelt_var(klein) || enthaelt_mathe(klein) {
                for e in [
                    ERadiusObenLinks,
                    ERadiusObenRechts,
                    ERadiusUntenRechts,
                    ERadiusUntenLinks,
                ] {
                    aus.push(dekl(e, spaet(), wichtig, klein));
                }
                return;
            }
            // Release 10.10.2026: die elliptische Form `waagerecht /
            // senkrecht` (§5.5). Der Rasterizer zeichnet Ecken kreisfoermig;
            // je Ecke gilt der KLEINERE der beiden Radien (der eingeschriebene
            // Kreis) — bei gleicher Einheit, sonst der waagerechte. Vorher
            // fiel die ganze Angabe weg (WPT `border-radius-valid`), und
            // `50% / 10%` (fast eckig) waere mit dem waagerechten Radius
            // eine Pille geworden.
            let mut senkrecht_werte: Option<Vec<Laenge>> = None;
            let klein = match klein.split_once('/') {
                Some((waagerecht, senkrecht)) => {
                    let s = teile_wert(senkrecht.trim());
                    let l: Vec<Laenge> = s.iter().filter_map(|t| laenge_aus(t)).collect();
                    if s.is_empty() || s.len() > 4 || senkrecht.contains('/') || l.len() != s.len() {
                        verwurf(&format!("Wert ungeprueft: border-radius: {klein}"));
                        return;
                    }
                    senkrecht_werte = Some(l);
                    waagerecht.trim()
                }
                None => klein,
            };
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 4 {
                return;
            }
            let mut werte: Vec<(Wert, String)> = Vec::new();
            for t in teile.iter() {
                let t: &str = t;
                if enthaelt_var(t) || enthaelt_mathe(t) {
                    werte.push((spaet(), String::from(t)));
                } else if let Some(l) = laenge_aus(t) {
                    werte.push((Wert::Laenge(l), String::from(t)));
                } else {
                    return;
                }
            }
            // §5.4: die Reihenfolge ist oben-links, oben-rechts,
            // UNTEN-RECHTS, unten-links.
            let idx: [usize; 4] = match werte.len() {
                1 => [0, 0, 0, 0],
                2 => [0, 1, 1, 0],
                3 => [0, 1, 2, 1],
                _ => [0, 1, 2, 3],
            };
            for (nr, e) in [
                ERadiusObenLinks,
                ERadiusObenRechts,
                ERadiusUntenRechts,
                ERadiusUntenLinks,
            ]
            .iter()
            .enumerate()
            {
                let (w, r) = &werte[idx[nr]];
                let mut w = w.clone();
                if let (Some(s), Wert::Laenge(h)) = (&senkrecht_werte, &w) {
                    let s_idx: [usize; 4] = match s.len() {
                        1 => [0, 0, 0, 0],
                        2 => [0, 1, 1, 0],
                        3 => [0, 1, 2, 1],
                        _ => [0, 1, 2, 3],
                    };
                    let v = s[s_idx[nr]];
                    // Release 15.10.2026 (Agent F): the vertical radius is
                    // kept (elliptical corners).
                    if v != *h {
                        w = Wert::Radius(*h, v);
                    }
                }
                aus.push(dekl(*e, w, wichtig, r));
            }
        }
        // `outline: <breite> [farbe]` (CSS UI 4 §6.1). Anders als
        // `border` hat `outline` keinen Stil — der Ring wird immer
        // gezeichnet — und hoechstens zwei Teile (plus ein Stilwort, das
        // in der Praxis vorkommt, weil man `border` denkt).
        Kurz::Outline => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 3 {
                return;
            }
            let mut breite: Option<(Wert, String)> = None;
            let mut farbe: Option<(Wert, String)> = None;
            // Release 14.10.2026 (Agent CSS2): the style word becomes
            // `outline-style` (stored verbatim, `none` without one). The width
            // keeps its value: with `none` only the USED width is 0 (CSS UI 4
            // §6.1, the CSSWG dropped the computed-value zeroing; WPT
            // `CSS2/ui/outline-width-096` inherits 20px from an
            // `outline: black none 20px` parent and expects a 20px ring; before,
            // `none` counted as the width and the whole declaration was dropped).
            let mut style: Option<&str> = None;
            for t in teile.iter() {
                let t: &str = t;
                if matches!(
                    t,
                    "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge" | "outset" | "inset" | "auto"
                        | "hidden" | "none"
                ) {
                    style = Some(t);
                    continue;
                }
                if t == "currentcolor" && breite.is_some() && farbe.is_none() {
                    farbe = Some((Wert::Aktuell, String::from(t)));
                    continue;
                }
                if farbe.is_none() && !t.contains("var(") && !t.contains("calc(") {
                    if let Some(f) = farbe_aus(t) {
                        farbe = Some((Wert::Farbe(f), String::from(t)));
                        continue;
                    }
                }
                if breite.is_none() {
                    if t == "none" {
                        breite = Some((zahl_null(), String::from(t)));
                        continue;
                    }
                    if let Some(b) = rahmenbreite_wert(t) {
                        breite = Some((Wert::Laenge(b), String::from(t)));
                        continue;
                    }
                    if enthaelt_var(t) || enthaelt_mathe(t) {
                        breite = Some((spaet(), String::from(t)));
                        continue;
                    }
                }
                return;
            }
            if let Some((w, r)) = &breite {
                aus.push(dekl(EOutlineBreite, w.clone(), wichtig, r));
            } else if matches!(style, Some("none" | "hidden")) {
                // `outline: none` keeps the old meaning "no ring, width 0"
                // for readers of the width (k6 `outline_none_heisst_kein_ring`).
                aus.push(dekl(EOutlineBreite, zahl_null(), wichtig, klein));
            }
            aus.push(dekl(
                ESchluessel,
                Wert::Schluessel(String::from("outline-style"), String::from(style.unwrap_or("none"))),
                wichtig,
                klein,
            ));
            if let Some((w, r)) = &farbe {
                aus.push(dekl(EOutlineFarbe, w.clone(), wichtig, r));
            }
        }
        // `background` (CSS 2.1 §14.2.1): Farbe und/oder Bild. Reihen-
        // folge beliebig, jede Teil.
        Kurz::Hintergrund => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > MAX_DEKLARATIONEN_JE_REGEL {
                return;
            }
            // Release 10.10.2026: Wiederholung, Position und `/ Groesse`
            // (CSS Backgrounds 3 §3.10). Die Woerter der LETZTEN Ebene (nach
            // dem letzten Komma) — dieselbe Ebene, deren Bild am Ende gilt.
            let mut wiederholung: Vec<&str> = Vec::new();
            // Release 11.10.2026: die Kasten-Woerter (`<box>`, CSS Backgrounds 3
            // §3.10): eines gilt fuer origin UND clip, bei zweien ist das
            // zweite der clip; `text` ist nur clip.
            let mut kasten: Vec<&str> = Vec::new();
            // Release 14.10.2026 (Agent CSS2): `fixed`/`scroll`/`local` of the
            // last layer.
            let mut attachment: Option<&str> = None;
            let mut lage: Vec<&str> = Vec::new();
            let mut groesse: Vec<&str> = Vec::new();
            let mut nach_strich = false;
            for t in teile.iter() {
                let t: &str = t;
                // `var()`/`calc()` zuerst: der Teil ist dann die GANZE
                // Angabe, und die Kaskade loest sie auf (CSS Variables 1
                // §3.2). Ohne diesen Zweig waere `background: var(--bg)`
                // ein unbekanntes Schluesselwort — und die Seite bliebe
                // weiss.
                //
                // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
                // Editierfehler): ohne diesen Zweig ergab
                // `background: var(--bg)` die durchsichtige Anfangsfarbe
                // statt `blue`; der Test `background_var` hat es gefangen.
                if enthaelt_var(t) || enthaelt_mathe(t) {
                    aus.push(dekl(EHintergrund, spaet(), wichtig, klein));
                    continue;
                }
                if t == "none" {
                    aus.push(dekl(
                        EHintergrund,
                        Wert::Wort(String::from("none")),
                        wichtig,
                        klein,
                    ));
                    continue;
                }
                if t.contains("url(") {
                    if let Some(u) = url_innern(t) {
                        aus.push(dekl(EHintergrundBild, Wert::Url(u), wichtig, t));
                        continue;
                    }
                }
                if enthaelt_mathe(t) {
                    if let Some(v) = crate::rechnen::rechne(
                        t,
                        &crate::rechnen::RechenCtx::neu(16.0, 0.0, 0.0),
                        0,
                    ) {
                        aus.push(dekl(
                            EHintergrund,
                            Wert::Farbe(skalierungs_farbe(v)),
                            wichtig,
                            t,
                        ));
                        continue;
                    }
                }
                if t == "currentcolor" {
                    aus.push(dekl(EHintergrund, Wert::Aktuell, wichtig, klein));
                    continue;
                }
                if let Some(f) = farbe_aus(t) {
                    aus.push(dekl(EHintergrund, Wert::Farbe(f), wichtig, klein));
                    continue;
                }
                // K1: ein Verlauf als Wert.
                if let Some(v) = verlauf_aus(t) {
                    aus.push(dekl(EHintergrundBild, Wert::Verlauf(v), wichtig, t));
                    continue;
                }
                // Der Rest: Wiederholung, Lage, `/ Groesse`, Anhaftung und
                // Kasten. GEMESSEN vorher (Web-Platform-Tests,
                // `background-328`): die Woerter landeten als Schluessel in
                // `background-position` und wirkten nie — `repeat-x`,
                // `no-repeat`, `center/cover` gingen verloren.
                let mut rest = t;
                if let Some(ohne) = rest.strip_suffix(',') {
                    rest = ohne;
                }
                for stueck in rest.split_inclusive('/') {
                    let (wort, strich) = match stueck.strip_suffix('/') {
                        Some(w) => (w, true),
                        None => (stueck, false),
                    };
                    match wort {
                        "" => {}
                        "repeat" | "repeat-x" | "repeat-y" | "no-repeat" | "space" | "round" => wiederholung.push(wort),
                        "fixed" | "scroll" | "local" => attachment = Some(wort),
                        "border-box" | "padding-box" | "content-box" | "text" => kasten.push(wort),
                        w if nach_strich => groesse.push(w),
                        w => lage.push(w),
                    }
                    if strich {
                        nach_strich = true;
                    }
                }
                if t.ends_with(',') {
                    wiederholung.clear();
                    lage.clear();
                    groesse.clear();
                    kasten.clear();
                    attachment = None;
                    nach_strich = false;
                }
            }
            // Eine Kurzform setzt die nicht genannten Langformen auf den
            // Anfangswert (`repeat`, `0% 0%`, `auto`).
            if !enthaelt_var(klein) {
                let rep = if wiederholung.is_empty() { String::from("repeat") } else { wiederholung.join(" ") };
                let pos = if lage.is_empty() { String::from("0% 0%") } else { lage.join(" ") };
                let gr = if groesse.is_empty() { String::from("auto") } else { groesse.join(" ") };
                for (e, text) in [(EBildWiederholen, rep), (EBildPosition, pos), (EBildGroesse, gr)] {
                    if let Some(w) = lang_wert(e, &text) {
                        aus.push(dekl(e, w, wichtig, klein));
                    }
                }
            }
            // Release 08.10.2026: eine Kurzform setzt JEDE Langform, auch die
            // nicht genannten, auf den Anfangswert (CSS Cascade 4 §2.2) —
            // `background: 0 0` heisst „Farbe durchsichtig, kein Bild".
            // GEMESSEN vorher: die Menueknoepfe von github.com
            // (`background: 0 0; border: 0`) behielten das Weiss des
            // UA-Knopfes und verdeckten ihre eigene Schrift.
            // Mit `var()` steht die Angabe erst in der Kaskade fest — dann
            // wird hier nichts zurueckgesetzt (das Bild koennte in der
            // Variablen stehen).
            let spaet_aufgeloest = enthaelt_var(klein);
            if !spaet_aufgeloest {
                let clip = kasten.last().copied().unwrap_or("border-box");
                aus.push(dekl(
                    ESchluessel,
                    Wert::Schluessel(String::from("background-clip"), String::from(clip)),
                    wichtig,
                    klein,
                ));
                // Release 13.10.2026 (Agent CSS2): das erste Kasten-Wort ist
                // auch `background-origin` (§3.10).
                let ursprung = kasten.first().copied().filter(|w| *w != "text").unwrap_or("padding-box");
                aus.push(dekl(
                    ESchluessel,
                    Wert::Schluessel(String::from("background-origin"), String::from(ursprung)),
                    wichtig,
                    klein,
                ));
                // Release 14.10.2026 (Agent CSS2): and `background-attachment`
                // (§3.10). MEASURED before (WPT css-backgrounds
                // `attachment-local-clipping-*`, `background: green local
                // border-box`): the word was read and dropped.
                aus.push(dekl(
                    ESchluessel,
                    Wert::Schluessel(String::from("background-attachment"), String::from(attachment.unwrap_or("scroll"))),
                    wichtig,
                    klein,
                ));
            }
            if !spaet_aufgeloest && !aus.iter().any(|d| d.eigenschaft == EHintergrund) {
                aus.push(dekl(EHintergrund, Wert::Farbe(Farbe::DURCHSICHTIG), wichtig, klein));
            }
            if !spaet_aufgeloest
                && !aus.iter().any(|d| {
                    d.eigenschaft == EHintergrundBild
                        || d.eigenschaft == EHintergrund && matches!(&d.wert, Wert::Wort(w) if w == "none")
                })
            {
                if let Some(w) = lang_wert(EHintergrundBild, "none") {
                    aus.push(dekl(EHintergrundBild, w, wichtig, klein));
                }
            }
        }
        // `list-style` (CSS 2.1 §12.5.1): Typ, Position und Bild in
        // beliebiger Reihenfolge.
        Kurz::Listenstil => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 3 {
                return;
            }
            for t in teile.iter() {
                let t: &str = t;
                if let Some(typ) = listen_typ(t) {
                    aus.push(dekl(EListe, Wert::Wort(String::from(typ)), wichtig, t));
                    continue;
                }
                if t.contains("url(") {
                    if let Some(u) = url_innern(t) {
                        aus.push(dekl(
                            ESchluessel,
                            Wert::Schluessel(String::from("list-style-image"), u),
                            wichtig,
                            t,
                        ));
                        continue;
                    }
                }
                aus.push(dekl(
                    ESchluessel,
                    Wert::Schluessel(String::from("list-style"), String::from(t)),
                    wichtig,
                    t,
                ));
            }
        }
        // `flex` (CSS Flexbox 1 §7): die Kurzform der DREI
        // Flex-Eigenschaften. `none` = `0 1 auto` (§9.7).
        // CSS Flexbox 1 §5.3: Richtung und Umbruch in beliebiger
        // Reihenfolge, je hoechstens einmal; das Fehlende auf den
        // Anfangswert (`row`, `nowrap`).
        Kurz::FlexFluss => {
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 2 {
                return;
            }
            let (mut richtung, mut umbruch) = (None, None);
            for t in teile.iter() {
                if richtung.is_none()
                    && let Some(w) = lang_wert(EFlexRichtung, t)
                {
                    richtung = Some(w);
                    continue;
                }
                if umbruch.is_none()
                    && let Some(w) = lang_wert(EFlexUmbruch, t)
                {
                    umbruch = Some(w);
                    continue;
                }
                return;
            }
            let richtung = richtung.or_else(|| lang_wert(EFlexRichtung, "row"));
            let umbruch = umbruch.or_else(|| lang_wert(EFlexUmbruch, "nowrap"));
            if let Some(w) = richtung {
                aus.push(dekl(EFlexRichtung, w, wichtig, klein));
            }
            if let Some(w) = umbruch {
                aus.push(dekl(EFlexUmbruch, w, wichtig, klein));
            }
        }
        Kurz::Flex => {
            // CSS Flexbox 1 §9.7: `none` ist `0 0 auto` — der
            // Schrumpf ist NULL, nicht 1.
            //
            // GEMESSENER FEHLER 01.10.2026: hier stand 1.0; der Test
            // `flex_kurzform` hat es gefangen.
            if klein == "none" {
                aus.push(dekl(EFlexWachstum, zahl_null(), wichtig, klein));
                aus.push(dekl(EFlexSchrumpf, zahl_null(), wichtig, klein));
                aus.push(dekl(EFlexBasis, Wert::Auto, wichtig, klein));
                return;
            }
            // Release 10.10.2026: `auto` = `1 1 auto`, `initial` = `0 1 auto`
            // (CSS Flexbox 1 §7.1.1). GEMESSEN vorher (github.com,
            // `.MarketingHeader-module__menu { flex: auto }`): `auto` setzte
            // nur die Basis, das Wachstum blieb 0 — das Menue wuchs nicht,
            // „Sign in/up" standen bei x=900 statt am rechten Rand.
            let eins = || Wert::Laenge(Laenge { wert: 1.0, einheit: Einheit::Zahl });
            if klein == "auto" || klein == "initial" {
                let w = if klein == "auto" { eins() } else { zahl_null() };
                aus.push(dekl(EFlexWachstum, w, wichtig, klein));
                aus.push(dekl(EFlexSchrumpf, eins(), wichtig, klein));
                aus.push(dekl(EFlexBasis, Wert::Auto, wichtig, klein));
                return;
            }
            let teile = teile_wert(klein);
            if teile.is_empty() || teile.len() > 3 {
                return;
            }
            let mut wachstum: Option<(Wert, String)> = None;
            let mut schrumpf: Option<(Wert, String)> = None;
            let mut basis: Option<(Wert, String)> = None;
            let mut zuletzt_wachstum = false;
            for t in teile.iter() {
                let t: &str = t;
                // `1 1 auto`: Zahlen sind Wachstum/Schrumpf, eine
                // Laenge oder `auto` ist die Basis (§7.2).
                if !t.contains(char::is_alphabetic) {
                    // CSS Flexbox 1 §7.2: die erste Zahl ist
                    // `flex-grow`, die zweite `flex-shrink`.
                    //
                    // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): es
                    // gab nur EINEN Platz fuer eine Zahl, und
                    // `flex: 2 3 40px` fiel als Ganzes weg. Der Test
                    // `flex_kurzform` hat es gefangen.
                    if let Some(z) = zahl_aus(t) {
                        let v = (
                            Wert::Laenge(Laenge {
                                wert: z,
                                einheit: Einheit::Zahl,
                            }),
                            String::from(t),
                        );
                        // Release 11.10.2026: `flex-shrink` steht direkt hinter
                        // `flex-grow` (§7.2: `<'flex-grow'> <'flex-shrink'>?`).
                        // GEMESSEN vorher: WPT `flex-shorthand-flex-basis-middle`
                        // (`flex: 1 auto 1` galt statt verworfen zu werden).
                        if wachstum.is_none() {
                            wachstum = Some(v);
                            zuletzt_wachstum = true;
                        } else if schrumpf.is_none() && zuletzt_wachstum {
                            schrumpf = Some(v);
                            zuletzt_wachstum = false;
                        } else {
                            return;
                        }
                        continue;
                    }
                }
                zuletzt_wachstum = false;
                if t == "auto" {
                    basis = Some((Wert::Auto, String::from(t)));
                    continue;
                }
                // Release 11.10.2026: `content` und die Groessen-Schluesselwoerter
                // als Basis (CSS Flexbox 1 §7.2). GEMESSEN vorher: WPT
                // `flex-basis-content-percentage-height` (`flex: 1 1 content`
                // wurde verworfen).
                if basis.is_none() && matches!(t, "content" | "min-content" | "max-content" | "fit-content") {
                    basis = Some((Wert::Wort(String::from(t)), String::from(t)));
                    continue;
                }
                if let Some(l) = laenge_aus(t) {
                    if basis.is_none() {
                        basis = Some((Wert::Laenge(l), String::from(t)));
                        continue;
                    }
                }
                return;
            }
            // §7: EIN Zahl = `flex-grow`, der Schrumpf bleibt 1.
            // Release 10.10.2026: ohne Zahl (`flex: 100px`) ist das Wachstum
            // 1 (§7.1.1: „flex: <'flex-basis'>" -> `1 1 <basis>`); ohne
            // Basis (`flex: 1`, `flex: 2 3`) ist die Basis 0 (§7.2: „When
            // omitted from the flex shorthand, its specified value is 0").
            // Vorher: Basis `auto` — `flex: 1`-Spalten teilten den Platz
            // nach Inhalt statt zu gleichen Teilen. Und `flex: 2 100px`
            // setzte den Schrumpf auf 2 statt auf den Anfangswert 1.
            if wachstum.is_none() && basis.is_some() {
                wachstum = Some((eins(), String::from(klein)));
            }
            // Release 11.10.2026: die weggelassene Basis ist `0%` wie in allen
            // Browsern (CSS Flexbox 1 §7.2 schreibt inzwischen `0px`, WPT
            // folgt den Browsern: `flex-one-sets-flex-basis-to-zero-px` —
            // `flex: 1` in einer Spalte ohne definite Hoehe ist 14 px hoch,
            // `flex: 1 1 0px` 0 px; `flex-direction-column-overlap-001`).
            // `percentage-heights-017` haelt die Kaskade (narsil-page: ein
            // Element mit Prozentbasis in einer unbestimmten Spalte ist nicht
            // definit).
            if basis.is_none() {
                basis = Some((
                    Wert::Laenge(Laenge { wert: 0.0, einheit: Einheit::Prozent }),
                    String::from(klein),
                ));
            }
            if schrumpf.is_none() {
                schrumpf = Some((
                    Wert::Laenge(Laenge {
                        wert: 1.0,
                        einheit: Einheit::Zahl,
                    }),
                    String::from(klein),
                ));
            }
            if basis.is_none() {
                basis = Some((Wert::Auto, String::from(klein)));
            }
            if let Some((w, r)) = &wachstum {
                aus.push(dekl(EFlexWachstum, w.clone(), wichtig, r));
            }
            if let Some((w, r)) = &schrumpf {
                aus.push(dekl(EFlexSchrumpf, w.clone(), wichtig, r));
            }
            if let Some((w, r)) = &basis {
                aus.push(dekl(EFlexBasis, w.clone(), wichtig, r));
            }
        }
        // `font` (CSS 2.1 §15.8): Stil, Gewicht, Groesse/Hoehe und
        // Familie in dieser Reihenfolge. Jeder Teil ausser der
        // Familie ist optional.
        Kurz::Schrift => {
            // `font: var(--x)` — alle fuenf Langformen mit dem
            // Platzhalter (CSS Variables 1 §3.2).
            //
            // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion): ohne
            // diesen Zweig kam nur die FAMILIE an, und der Test
            // `font_var_ganz` fand weder Stil noch Groesse.
            if enthaelt_var(klein) || enthaelt_mathe(klein) {
                for e in [
                    EStil,
                    Schriftgewicht,
                    Schriftgroesse,
                    Zeilenhoehe,
                    Schriftfamilie,
                ] {
                    aus.push(dekl(e, spaet(), wichtig, klein));
                }
                return;
            }
            // Release 13.10.2026 (Agent CSS2): `16px / 32px` mit Leerraum um den
            // Schraegstrich ist dieselbe Angabe wie `16px/32px` (CSS Fonts 4
            // §2.8: `<font-size> [ / <line-height> ]`, Leerraum beliebig).
            // GEMESSEN vorher (WPT css-overflow `line-clamp/*`, alle mit `font:
            // 16px / 32px serif`): die Zeilenhoehe fiel weg und `/ 32px serif`
            // wurde die Familie.
            let ohne_leer_am_strich;
            let klein_font = if klein.contains(" /") || klein.contains("/ ") {
                let mut t = klein.to_string();
                while t.contains(" /") {
                    t = t.replace(" /", "/");
                }
                while t.contains("/ ") {
                    t = t.replace("/ ", "/");
                }
                ohne_leer_am_strich = t;
                ohne_leer_am_strich.as_str()
            } else {
                klein
            };
            let teile = teile_wert(klein_font);
            if teile.is_empty() {
                return;
            }
            let start = aus.len();
            let mut i = 0usize;
            // M3: die Schleife laeuft ueber `teile`, das ist durch
            // MAX_DEKLARATIONEN_JE_REGEL gedeckelt.
            while i < teile.len() {
                let t: &str = teile[i].as_ref();
                // Release 14.10.2026 (Agent CSS2): style, variant, weight and
                // stretch come in any order before the size; `normal` may stand
                // for any of them (CSS Fonts 4 §2.8). MEASURED before (WPT
                // `CSS2/fonts/font-011..016`, `font: italic small-caps bold 1in
                // Ahem`): only a leading style word was read, `small-caps` ended
                // the list and the whole rest became the family.
                if matches!(t, "italic" | "oblique")
                    && !aus[start..].iter().any(|d: &Deklaration| d.eigenschaft == EStil)
                {
                    aus.push(dekl(EStil, Wert::Wort(String::from("kursiv")), wichtig, klein));
                    i += 1;
                    continue;
                }
                if matches!(
                    t,
                    "normal"
                        | "small-caps"
                        | "ultra-condensed"
                        | "extra-condensed"
                        | "condensed"
                        | "semi-condensed"
                        | "semi-expanded"
                        | "expanded"
                        | "extra-expanded"
                        | "ultra-expanded"
                ) {
                    i += 1;
                    continue;
                }
                if matches!(
                    t,
                    "bold"
                        | "bolder"
                        | "lighter"
                        | "100"
                        | "200"
                        | "300"
                        | "400"
                        | "500"
                        | "600"
                        | "700"
                        | "800"
                        | "900"
                ) && !aus
                    .iter()
                    .any(|d: &Deklaration| matches!(d.eigenschaft, Schriftgewicht))
                {
                    if let Some(g) = schriftgewicht_wert(t) {
                        aus.push(dekl(Schriftgewicht, g, wichtig, klein));
                        i += 1;
                        continue;
                    }
                }
                // Release 14.10.2026 (Agent CSS2): a size keyword
                // (`small`, `larger`, …) is the size as well (CSS Fonts 4 §2.8).
                if matches!(
                    t,
                    "xx-small" | "x-small" | "small" | "medium" | "large" | "x-large" | "xx-large" | "xxx-large"
                        | "smaller" | "larger"
                ) && let Some(g) = lang_wert(Schriftgroesse, t)
                {
                    aus.push(dekl(Schriftgroesse, g, wichtig, t));
                    i += 1;
                    continue;
                }
                // `12px/1.5` oder `12px`: Groesse und Zeilenhoehe.
                // Release 14.10.2026 (Agent CSS2): any length, not only `px`.
                // MEASURED before (WPT `CSS2/text/white-space-001..006`, `font:
                // 1em Ahem`): `1em` became part of the family and Ahem was lost.
                if !t.contains(char::is_alphabetic) || t.contains("/") || t.ends_with("px") || laenge_aus(t).is_some() {
                    let (groesse, hoehe) = match teile[i].split_once('/') {
                        Some((g, h)) => (g, Some(h)),
                        None => (t, None),
                    };
                    if let Some(g) = laenge_aus(groesse) {
                        aus.push(dekl(Schriftgroesse, Wert::Laenge(g), wichtig, groesse));
                        if let Some(h) = hoehe {
                            match laenge_aus(h) {
                                Some(x) => {
                                    aus.push(dekl(Zeilenhoehe, Wert::Laenge(x), wichtig, klein))
                                }
                                None => {
                                    if !h.contains(char::is_alphabetic) {
                                        if let Some(z) = zahl_aus(h) {
                                            aus.push(dekl(
                                                Zeilenhoehe,
                                                Wert::Laenge(Laenge {
                                                    wert: z,
                                                    einheit: Einheit::Zahl,
                                                }),
                                                wichtig,
                                                klein,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                        i += 1;
                        continue;
                    }
                }
                break;
            }
            // Release 14.10.2026 (Agent CSS2): without size and family the
            // shorthand is invalid; the style and weight it does not name are
            // reset to `normal` (CSS Fonts 4 §2.8).
            let has_size = aus[start..].iter().any(|d: &Deklaration| d.eigenschaft == Schriftgroesse);
            if !has_size || i >= teile.len() {
                aus.truncate(start);
                return;
            }
            if !aus[start..].iter().any(|d: &Deklaration| d.eigenschaft == EStil) {
                aus.push(dekl(EStil, Wert::Wort(String::from("normal")), wichtig, klein));
            }
            if !aus[start..].iter().any(|d: &Deklaration| d.eigenschaft == Schriftgewicht)
                && let Some(g) = schriftgewicht_wert("normal")
            {
                aus.push(dekl(Schriftgewicht, g, wichtig, klein));
            }
            // Der Rest ist die Familie (CSS 2.1 §15.8).
            if i < teile.len() {
                let fam = teile[i..].join(" ");
                // Der Name geht zweimal hinein: als Wert und als Rohtext
                // (die Kaskade braucht ihn fuer `inherit`).
                aus.push(dekl(Schriftfamilie, Wert::Wort(fam), wichtig, klein));
            }
        }
    }
}

/// `border-width` (CSS 2.1, §8.3): `thin`, `medium`, `thick` oder eine
/// Laenge.
///
/// Die drei Woerter haben KEINE browseruebergreifenden Werte — CSS 2.1
/// sagt nur „thin (1 px) / medium (3 px) / thick (5 px) sind
///Empfehlungen". `medium` ist der Anfangswert, und dieses Browser
///zeichnet Rahmen immer durchgehend, also ist `thin`/`medium`/`thick`
/// fuer ihn EINE Dicke.
///
/// Release 10.10.2026: CSS Backgrounds 3 §3.3 legt die Woerter fest —
/// `thin` 1 px, `medium` 3 px, `thick` 5 px (so rechnen Chrome und
/// Firefox). Vorher (01.10.2026) standen hier die Annahmen 1/1/2 px;
/// GEMESSEN (Web-Platform-Tests, `css/CSS2`): jede Referenz mit `border:
/// black solid medium` war 2 px zu schmal.
fn rahmenbreite_wert(klein: &str) -> Option<Laenge> {
    let px = |wert| Some(Laenge { wert, einheit: Einheit::Px });
    match klein {
        "thin" => px(1.0),
        "medium" => px(3.0),
        "thick" => px(5.0),
        // Release 11.10.2026 (Agent CSS2): nur nichtnegative Laengen, keine
        // Prozente (CSS 2.1 §8.5.1 `<border-width>`). GEMESSEN vorher (WPT
        // `border-left-width-001` u. a., `border-left-width: -1px`): 0 statt
        // des Anfangswerts `medium`.
        _ => laenge_aus(klein).filter(|l| l.wert >= 0.0 && l.einheit != Einheit::Prozent),
    }
}

/// Die Breiten-Eigenschaft einer Seite (fuer die logischen Kurzformen).
fn breite_e(e: Eigenschaft) -> Eigenschaft {
    match e {
        Oben => RahmenObenBreite,
        Rechts => RahmenRechtsBreite,
        Unten => RahmenUntenBreite,
        _ => RahmenLinksBreite,
    }
}

/// Die Farb-Eigenschaft einer Seite.
fn farbe_e(e: Eigenschaft) -> Eigenschaft {
    match e {
        Oben => RahmenObenFarbe,
        Rechts => RahmenRechtsFarbe,
        Unten => RahmenUntenFarbe,
        _ => RahmenLinksFarbe,
    }
}

/// Die Stil-Eigenschaft einer Seite.
///
/// GEMESSENE NOTWENDIGKEIT: `rahmen_logisch_zerlegen` hat Breite und
/// Farbe eingetragen und den STIL nie. Bei
/// `border-inline: 1px solid #ccc` kam damit an beiden Seiten
/// `breite=1, stil=Kein` heraus — und der Rasterizer zeichnet keinen
/// Rahmen ohne Stil. GEMESSEN, gegen die physische Form:
///
/// ```text
/// border-left:1px solid #ccc     l: breite=1 stil=Durchgehend farbe=#cccccc
/// border-inline-start:1px solid  l: breite=1 stil=Durchgehend farbe=#cccccc
/// border-inline:1px solid #ccc   l: breite=1 stil=Kein         farbe=#cccccc
///                                r: breite=1 stil=Kein         farbe=#cccccc
/// ```
///
/// Im Korpus (35 Dateien) steht `border-inline` 27 Mal. Vor diesem
/// Fix waren das 27 Regeln mit einer Breite ohne Stil — also 27
/// Rahmen, die nicht gezeichnet wurden.
fn stil_e(e: Eigenschaft) -> Eigenschaft {
    match e {
        Oben => RahmenObenStil,
        Rechts => RahmenRechtsStil,
        Unten => RahmenUntenStil,
        _ => RahmenLinksStil,
    }
}

/// Zerlegt `border-block` / `border-inline` auf die ZWEI Seiten einer
/// logischen Achse (CSS Box Sizing 4 §2).
///
/// Rueckgabe false = die Angabe war nicht lesbar; dann wird nichts
/// eingetragen (die Kurzform faellt ganz weg, nicht halb).
fn rahmen_logisch_zerlegen(
    klein: &str,
    a: Eigenschaft,
    b: Eigenschaft,
    wichtig: bool,
    aus: &mut Vec<Deklaration>,
) -> bool {
    let teile = teile_wert(klein);
    if teile.is_empty() || teile.len() > 3 {
        return false;
    }
    let mut breite: Option<(Wert, String)> = None;
    let mut farbe: Option<(Wert, String)> = None;
    let mut stil: Option<(&'static str, String)> = None;
    for t in teile.iter() {
        let t: &str = t;
        // Die Farbe zuerst: `red` ist keine Breite.
        if farbe.is_none() && !t.contains("var(") && !t.contains("calc(") {
            if let Some(f) = farbe_aus(t) {
                farbe = Some((Wert::Farbe(f), String::from(t)));
                continue;
            }
        }
        if breite.is_none() {
            if let Some(b) = rahmenbreite_wert(t) {
                breite = Some((Wert::Laenge(b), String::from(t)));
                continue;
            }
        }
        // `none`/`hidden` heissen Breite 0 (CSS 2.1 §8.5); die anderen
        // Stile werden erkannt und als Stil eingetragen. Vorher wurden
        // sie nur erkannt und WEGGELASSEN — siehe `stil_e`.
        if let Some(s) = rahmenstil_wert(t) {
            if breite.is_none() && (t == "none" || t == "hidden" || t == "0") {
                breite = Some((
                    Wert::Laenge(Laenge {
                        wert: 0.0,
                        einheit: Einheit::Px,
                    }),
                    String::from(t),
                ));
            }
            if stil.is_none() {
                stil = Some((s, String::from(t)));
            }
            continue;
        }
        if enthaelt_var(t) || enthaelt_mathe(t) {
            if breite.is_none() {
                breite = Some((spaet(), String::from(t)));
                continue;
            }
        }
        return false;
    }
    // GEMESSENER FEHLER, den dieser Block behoben hat: `solid` ist ein
    // Stil, aber KEINE Breite. Ohne diese Zeile waere
    // `border-inline: 1px solid #ccc` eine Angabe ohne Stil, und ein
    // Rahmen ohne Stil wird nicht gezeichnet.
    if breite.is_none() && farbe.is_none() && stil.is_none() {
        return false;
    }
    for e in [a, b].iter() {
        if let Some((w, r)) = &breite {
            aus.push(dekl(breite_e(*e), w.clone(), wichtig, r));
        }
        if let Some((w, r)) = &farbe {
            aus.push(dekl(farbe_e(*e), w.clone(), wichtig, r));
        }
        if let Some((s, r)) = &stil {
            aus.push(dekl(stil_e(*e), Wert::Wort(String::from(*s)), wichtig, r));
        }
    }
    true
}

/// `border-*-style` (CSS 2.1, §8.5).
///
/// **BILD_37 — DIE ZUSAMMENFASSUNG IST AUFGEHOBEN, und das ist eine
/// ausdruecklich benannte Umkehr einer vorherigen Entscheidung.** Bis hierher
/// haben **alle sieben** Musterstile auf **einen** Wert `"gemustert"` geführt.
/// Die damalige GEMESSENE Begruendung lautete woertlich: *„dieser Browser
/// ZEICHNET Rahmen immer durchgehend, die细分 werden also zusammengefasst …
/// der Rasterizer hat keine gestrichelte Kante."*
///
/// **Diese Begruendung traegt nicht mehr**, und zwar **zwei Messungen**:
///
/// 1. `narsil-raster/src/rahmen.rs` hat seit BILD_34 **eine** Kante je Stil
///    (`RahmenStil` mit `Gestrichelt`/`Gepunktet`/`Doppelt`/`Furche`/
///    `Wulst`/`Innen`/`Aussen` in `narsil-core/src/style.rs`).
/// 2. **Firefox malt die Stile Unterschiedliche**, und ich habe es an den
///    Referenzen `team/testseiten/deko_ff/24..27.png` **gemessen**:
///    `dashed` hat eine Luecke von **exakt 3× der Rahmenbreite** (24 px bei
///    8 px), `double` ist `3 / 2 / 3`, `groove` ist aussen `4 px` in
///    **`Farbe × 2/3`** und innen `4 px` in der Farbe.
///
/// **Warum das kein Ruestckgriff auf eine fremde Crate ist:** `Gemustert`
/// bleibt als Variante erhalten — es bricht also **nichts**. Nur der Parser
/// liefert ab jetzt **je Stil seinen eigenen** Namen.
fn rahmenstil_wert(klein: &str) -> Option<&'static str> {
    Some(match klein {
        "none" | "hidden" => "kein",
        "solid" => "durchgehend",
        // BILD_37: je Stil sein eigener Wert, damit die Kaskade sie
        // unterscheiden kann. Die Namen sind die der Variante in `typen.rs`.
        "dashed" => "gestrichelt",
        "dotted" => "gepunktet",
        "double" => "doppelt",
        "groove" => "furche",
        "ridge" => "wulst",
        "inset" => "innen",
        "outset" => "aussen",
        _ => return None,
    })
}

/// Die Zahl 0 als Wert (CSS Flexbox 1 §9.7: `flex: none` = `0 1 auto`).
fn zahl_null() -> Wert {
    Wert::Laenge(Laenge {
        wert: 0.0,
        einheit: Einheit::Zahl,
    })
}

/// `background: rgb(...)` und Verwandte: der Wert wird als Zahl im
/// Bereich 0-255 (oder 0-1 bei Prozent) gerechnet; die Kanäle sind
/// getrennt, deshalb gibt es hier KEINE Farbe zurueck.
///
/// GEMESSENER HINWEIS: `background: rgb(0 0 0 / 50%)` geht hier nicht
/// — die Form mit Schraegstrich gehoert in `farbe_aus`, und der
/// `background`-Zweig probiert die Farbe ZUERST. Dieser Zweig ist nur
/// der Notfall fuer `background: <zahl>`.
fn skalierungs_farbe(_v: f32) -> Farbe {
    Farbe::SCHWARZ
}

/// Der Typ aus `list-style` (CSS 2.1 §12.5.1).
/// Ein Listentyp in der Kurzform `list-style`, unverändert als Schlüsselwort.
///
/// Release 07.10.2026, GEMESSENER FEHLER: hier wurde `none` zu `"keiner"` und
/// jeder andere Typ zu `"punktfeld"` übersetzt — Wörter, die die Kaskade
/// (`Eigenschaft::Listenstil`) nicht kennt. `list-style: none` ergab damit
/// eine Scheibe, und jede Navigationsleiste aus `<ul>` bekam Marken.
fn listen_typ(t: &str) -> Option<&'static str> {
    Some(match t {
        "none" => "none",
        "disc" => "disc",
        "circle" => "circle",
        "square" => "square",
        "decimal" => "decimal",
        "decimal-leading-zero" => "decimal-leading-zero",
        "lower-roman" => "lower-roman",
        "upper-roman" => "upper-roman",
        "lower-alpha" => "lower-alpha",
        "upper-alpha" => "upper-alpha",
        "lower-latin" => "lower-latin",
        "upper-latin" => "upper-latin",
        _ => return None,
    })
}
