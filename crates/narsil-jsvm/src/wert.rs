//! Oeffentlicher Wert-Typ: Momentaufnahme eines JS-Werts fuer den Host
//! (DOM-Anbindung spaeter). Heap-frei kopierbar, daher keine GC-Fallen
//! an der Schnittstelle.

/// JS-Wert aus Host-Sicht. Objekte/Felder werden als JSON-nahe
/// Struktur kopiert (Funktionen als `"[function Name]"`, Zyklen als
/// `"[zyklus]"`, Tiefe dabei auf 32 begrenzt).
#[derive(Debug, Clone, PartialEq)]
pub enum Wert {
    /// `undefined`.
    Undefiniert,
    /// `null`.
    Null,
    /// Boolescher Wert.
    Wahr(bool),
    /// Zahl (f64, inkl. NaN/Infinity).
    Zahl(f64),
    /// Zeichenkette.
    Text(String),
    /// Feld (Array).
    Feld(Vec<Wert>),
    /// A byte view copied over the host ABI, retaining its JS type.
    ByteFeld { daten: Vec<u8>, geklemmt: bool },
    /// Objekt (Schluessel in Einfuege-Reihenfolge, naeherungsweise).
    Gegenstand(Vec<(String, Wert)>),
    /// EINE NATIVE FUNKTION, und zwar eine, die der Host unter diesem
    /// Namen registriert hat (W6D: `ev.preventDefault`).
    ///
    /// GEMESSEN, warum das ein Name und keine Nummer ist (14:50): ein
    /// Name ist beim Import pruefbar (`host_funktionen` enthaelt ihn),
    /// eine Nummer nicht. Ein unbekannter Name wird beim Import mit
    /// einer benannten Meldung abgelehnt — ein `Wert`, der beim Import
    /// `undefined` ergibt, waere genau der Fehler, den W6D bei
    /// `addEventListener` gefunden hat.
    Nativ(String),
    /// EINE FUNKTION (W6D): der Heap-Index ihres Werts.
    ///
    /// GEMESSEN, warum das noetig ist (13:40): `addEventListener` bekam
    /// fuer `() => {…}` den Wert `undefined` und nahm den Handler nicht
    /// an — `gval_als_wert` gibt fuer einen `Ref` auf ein
    /// `HObj::Funktion` `None` zurueck, weil `Wert` keine Funktion
    /// kannte. Das liess sich nur auf zwei Wegen aendern: eine neue
    /// Variante hier, oder eine eigene Aufruf-Schnittstelle. Die
    /// Variante ist kleiner und geht durch ALLE Host-Funktionen.
    ///
    /// DER INDEX, nicht der Quelltext: der Handler ist eine CLOSUE. Sie
    /// zu ueberleben reicht ihr Name nicht — sie muss im Heap bleiben,
    /// und der Heap gehoert der Engine. Wer sie aufruft, muss also die
    /// Engine behalten (siehe `Engine::handler_aufrufen`).
    ///
    /// M6: es ist ein Index, keine Kopie — die Engine haelt die Wurzel
    /// (`Engine::handler_halten`), also kann der Heap die Funktion nicht
    /// einsammeln, solange die Seite lebt.
    Funktion(u32),
}

impl Wert {
    /// J6: ein **Host-Objekt, das eine Methode ist** — der Wert, den ein
    /// Hook zurueckgibt, wenn das Skript `knopf.meldung(...)` schreibt.
    ///
    /// GEMESSEN, warum das noetig war: ohne diesen Weg gab es nur die
    /// Zahl-Form (`__methode` = Nummer aus einer host-eigenen Tabelle),
    /// und ein Host ohne Tabelle bekam `TypeError: Aufrufziel ist keine
    /// Funktion`. Die Oberflaeche braucht genau das fuer ihre Knoepfe
    /// (Fenster.zurueck(), Karten.schliessen() …).
    ///
    /// GEMESSEN, warum es ein Objekt und keine Funktion ist: `Wert` hat
    /// keine Funktions-Variante fuer Host-Code (nur `Nativ(name)` fuer
    /// vom Host registrierte Funktionen und `Funktion(index)` fuer
    /// Skriptfunktionen). Zwei Felder im Objekt sind der dokumentierte
    /// Weg (W6B), und der Motor erkennt sie beim Aufruf.
    pub fn host_methode(id: u32, name: &str) -> Wert {
        Wert::Gegenstand(vec![
            (
                crate::hostobjekt::HOST_MERKE.to_string(),
                Wert::Zahl(id as f64),
            ),
            (
                crate::hostobjekt::METHODE.to_string(),
                Wert::Text(name.to_string()),
            ),
        ])
    }
}

impl Wert {
    /// JS-nahe Textdarstellung (`String(wert)`-Semantik): wird in
    /// `tests/sprache.rs` als erwartete Ausgabe verglichen.
    pub fn als_text(&self) -> String {
        match self {
            // GEMESSEN, warum `String(fn)` nicht der Name ist: `narsil`
            // schreibt den Handler in Meldungen, und der Name eines
            // Pfeiltextes ist leer. Der Text sagt nur: das ist eine
            // Funktion (W6D).
            Wert::Funktion(_) => "[Funktion]".to_string(),
            // GEMESSEN, warum auch hier der Name und nicht `[nativ]`:
            // die Meldung soll sagen, WELCHE Funktion fehlt.
            Wert::Nativ(n) => std::format!("[nativ {n}]"),
            Wert::Undefiniert => "undefined".to_string(),
            Wert::Null => "null".to_string(),
            Wert::Wahr(true) => "true".to_string(),
            Wert::Wahr(false) => "false".to_string(),
            Wert::Zahl(n) => zahl_text(*n),
            Wert::Text(s) => s.clone(),
            Wert::Feld(e) => e
                .iter()
                .map(|v| match v {
                    Wert::Undefiniert | Wert::Null => String::new(),
                    x => x.als_text(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Wert::ByteFeld { daten, .. } => daten.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","),
            Wert::Gegenstand(_) => "[object Object]".to_string(),
        }
    }
}

/// Zahl -> kanonischer JS-Text (wie `String(1/3)`, `String(1e21)`).
/// Begruendung: Tests vergleichen Ausgaben als Text; die Darstellung muss
/// deterministisch und JS-nah sein.
pub(crate) fn zahl_text(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if n == 0.0 {
        return "0".to_string();
    }
    // GEMESSEN (A5): `1e20` wurde ueber `n as i64` zu
    // 9223372036854775807 (i64-Ueberlauf!) und `1e-6` blieb „1e-6" statt
    // „0.000001". Jetzt die JS-Regel aus ECMA-262 (Number::toString):
    // kuerzeste Ziffernfolge, Exponentform nur bei n < -6 oder n >= 21.
    zahl_js(n)
}

/// Zahl -> Text nach der JS-Regel.
///
/// `{:e}` liefert die kuerzeste rundungstreue Ziffernfolge mit Exponent
/// (z. B. „1.5e-6", „1e21"); daraus wird die JS-Form gebaut.
fn zahl_js(n: f64) -> String {
    let neg = n < 0.0;
    let a = n.abs();
    let roh = format!("{a:e}");
    let (mant, exp) = match roh.split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
        None => (roh.clone(), 0),
    };
    let ziffern: String = mant.chars().filter(|c| *c != '.').collect();
    let k = ziffern.len() as i32;
    // Stellenzahl: Wert = 0.<ziffern> * 10^pos
    let pos = exp + 1;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if k <= pos && pos <= 21 {
        out.push_str(&ziffern);
        for _ in 0..(pos - k) {
            out.push('0');
        }
    } else if 0 < pos && pos <= 21 {
        out.push_str(&ziffern[..pos as usize]);
        out.push('.');
        out.push_str(&ziffern[pos as usize..]);
    } else if -6 < pos && pos <= 0 {
        out.push_str("0.");
        for _ in 0..(-pos) {
            out.push('0');
        }
        out.push_str(&ziffern);
    } else {
        out.push_str(&ziffern[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&ziffern[1..]);
        }
        out.push('e');
        let e = pos - 1;
        if e >= 0 {
            out.push('+');
        }
        out.push_str(&e.to_string());
    }
    out
}
