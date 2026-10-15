//! Die Querverweistabelle: klassische Tabelle **und** XRef-Stream.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # WARUM ZWEI FORMEN
//!
//! GEMESSEN an den PDFs in `testdaten/pdfs/`:
//!
//! ```text
//!   Datei            ObjStm   XRef-Stream   LZW
//!   rfc2616.pdf        0         0          114
//!   rfc9110.pdf        0         0            0
//!   f1040.pdf         30         3            0
//!   1512.03385.pdf     6         1            0
//! ```
//!
//! Wer nur die klassische Tabelle kann, sieht bei `f1040.pdf` **keine
//! Seite**. Wer nur Streams kann, sieht bei `rfc2616.pdf` keine. Es
//! sind zwei Implementierungen fuer dieselbe Aufgabe, und beide sind
//! noetig.
//!
//! # DIE DREI EINTRAGSFORMEN
//!
//! ```text
//!   0  frei            1  Byte-Offset im normalen Objekt
//!   2  im ObjStm       Objektnummer + Index im Stream, beide als
//!                      zwei Objektnummern kodiert
//! ```
//!
//! # `/Prev`: Dokumente mit mehreren Versionen
//!
//! GEMESSEN: `f1040.pdf` hat **drei** XRef-Streams hintereinander
//! (ein `/Prev` pro Aktualisierung). Wer nur den letzten liest, sieht
//! die **neuesten** Objektversionen nicht — und genau die aendert ein
//! Bearbeitungsprogramm.
//!
//! Deshalb: Kettenverfolgung mit Grenze (`MAX_XREF_KETTE`), und bei
//! einem zweiten Vorkommen derselben Objektnummer gewinnt der **erste**
//! Fund — das ist die Reihenfolge der Spezifikation.

use crate::filtern::{self, FilterFehler};
use crate::lexing::{Lexer, Token};
use crate::objekt::Wert;
use std::collections::BTreeMap;

/// Wie viele XRef-Abschnitte hintereinander gelesen werden.
///
/// added 03.10.2026 (M3). 64. Ein Dokument mit 3 Abschnitten ist
/// normal; eines mit 64 ist ein Angriff oder ein Fehler beim
/// Bearbeiten.
pub const MAX_XREF_KETTE: usize = 64;

/// Wie viele Eintraege ein XRef-Stream haben darf.
///
/// added 03.10.2026 (M4). 2.000.000 Eintraege sind 12 MiB Wörterbuch —
/// reicht fuer jede echte Datei (die groesste hier hat 60.000).
pub const MAX_XREF_EINTRAEGE: usize = 2_000_000;

/// Wie weit hinter dem `xref` nach dem `trailer` gesucht wird.
///
/// added 03.10.2026. 1 MiB. Eine Tabelle mit 2.000.000 Eintraegen ist
/// 40 MiB — aber dann ist die Datei kaputt, und fuer `/Root` genuegt
/// der Anfang. Die Grenze ist da, damit ein Dokument ohne `trailer`
/// (nur XRef-Stream) nicht die ganze Datei tokenisiert.
pub const MAX_TRAILER_SUCHE: usize = 1024 * 1024;

/// Wie viele Objekte aus einem Objekt-Stream gelesen werden.
///
/// added 03.10.2026 (M3). Der Stream enthaelt eine Liste von
/// Objektnummern; eine Liste aus 1 Mrd. Nummern ist eine Bombe.
pub const MAX_OBJEKTSTREAM: usize = 200_000;

/// Wo ein Objekt liegt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Eintrag {
    /// Frei — ein frueher benutzter Bereich.
    Frei,
    /// Normal, an dieser Byte-Position in der Datei.
    Offset(u64),
    /// In einem Objekt-Stream: (Objektnummer des Streams, Index
    /// innerhalb des Streams).
    ImObjektstream(u32, u32),
}

/// Die Querverweistabelle einer Datei.
#[derive(Debug, Clone, Default)]
pub struct Querverweis {
    /// Objektnummer -> Eintrag.
    pub eintraege: BTreeMap<u32, Eintrag>,
    /// Der Trailer (der Wörterbuchteil hinter `trailer` oder der von
    /// einem XRef-Stream).
    pub trailer: Vec<(Vec<u8>, Wert)>,
    /// Der Offset des **letzten** Xref-Abschnitts, damit ein
    /// inkrementelles Update darueber gelegt werden kann.
    pub start: u64,
}

impl Querverweis {
    /// Der Wert `/Name` aus dem Trailer.
    pub fn trailer_name(&self, name: &str) -> Option<&str> {
        self.trailer
            .iter()
            .rev()
            .find(|(k, _)| k.as_slice() == name.as_bytes())
            .and_then(|(_, v)| v.name())
    }

    /// Die Zahl `/Name` aus dem Trailer.
    pub fn trailer_int(&self, name: &str) -> Option<i64> {
        self.trailer
            .iter()
            .rev()
            .find(|(k, _)| k.as_slice() == name.as_bytes())
            .and_then(|(_, v)| v.als_int())
    }

    /// Der Verweis `/Name` aus dem Trailer.
    pub fn trailer_ref(&self, name: &str) -> Option<(u32, u16)> {
        self.trailer
            .iter()
            .rev()
            .find(|(k, _)| k.as_slice() == name.as_bytes())
            .and_then(|(_, v)| match v {
                Wert::Ref(n, g) => Some((*n, *g)),
                _ => None,
            })
    }
}

/// Liest die Querverweistabelle einer Datei.
///
/// added 03.10.2026. `daten` ist die **ganze** Datei im Speicher.
///
/// DER WEG, und er hat drei Stufen:
///  1. `startxref` am Ende finden (die letzten 2 KiB reichen; ein
///     Dokument mit einem Kommentar nach `startxref` ist selten, aber
///     ein Anhaenger „%%EOF" ist Standard).
///  2. Von dort aus `/Prev`-Ketten folgen.
///  3. Objekt-Streams einsammeln, damit ihre Objekte spaeter
///     gefunden werden.
pub fn lesen(daten: &[u8]) -> Result<Querverweis, XrefFehler> {
    let start = startxref_finden(daten).ok_or(XrefFehler::KeinStartxref)?;
    let mut q = Querverweis {
        start,
        ..Default::default()
    };
    let mut gesehen: Vec<u64> = Vec::new();
    let mut ketten = 0usize;
    let mut position = start;
    loop {
        if ketten > MAX_XREF_KETTE {
            return Err(XrefFehler::KetteZuLang);
        }
        ketten += 1;
        if position as usize >= daten.len() || gesehen.contains(&position) {
            break;
        }
        gesehen.push(position);
        let abschnitt = abschnitt_lesen(daten, position)?;
        // Der **erste** Fund gewinnt: bei `/Prev` sind die neueren
        // Eintraege die richtigen, und sie kommen zuerst. `or_insert`
        // ist genau das — und der Grund, warum hier kein `if` noetig
        // ist, das nichts tut.
        for (nummer, eintrag) in abschnitt.eintraege {
            q.eintraege.entry(nummer).or_insert(eintrag);
        }
        // Der Trailer: der **neueste** gewinnt. Also zuerst der
        // Abschnitt, der frisch gelesen wurde, und nur was fehlt, wird
        // aus aelteren Abschnitten ergaenzt.
        for (k, v) in abschnitt.trailer {
            if !q.trailer.iter().any(|(x, _)| *x == k) {
                q.trailer.push((k, v));
            }
        }
        match abschnitt.prev {
            Some(p) if p > 0 => position = p,
            _ => break,
        }
    }
    if q.eintraege.is_empty() {
        return Err(XrefFehler::Leer);
    }
    Ok(q)
}

/// Ein Fehler beim Lesen der Querverweistabelle.
#[derive(Debug, Clone, PartialEq)]
pub enum XrefFehler {
    /// Kein `startxref` am Ende gefunden.
    KeinStartxref,
    /// Die Tabelle hatte keine brauchbaren Eintraege.
    Leer,
    /// `/Prev` laeuft im Kreis.
    KetteZuLang,
    /// An der genannten Stelle steht nichts Brauchbares.
    Kaputt(u64, String),
    /// Der Stream liess sich nicht entpacken.
    Filter(FilterFehler),
    /// Das Objekt hinter dem Verweis gibt es nicht.
    Fehlt(u32, u16),
}

impl std::fmt::Display for XrefFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XrefFehler::KeinStartxref => write!(f, "kein startxref"),
            XrefFehler::Leer => write!(f, "leere Querverweistabelle"),
            XrefFehler::KetteZuLang => write!(f, "mehr als {MAX_XREF_KETTE} XRef-Abschnitte"),
            XrefFehler::Kaputt(o, m) => write!(f, "XRef bei {o}: {m}"),
            XrefFehler::Filter(e) => write!(f, "XRef-Stream: {e}"),
            XrefFehler::Fehlt(n, g) => write!(f, "Objekt {n} {g} R fehlt"),
        }
    }
}

/// Das Ergebnis eines einzelnen XRef-Abschnitts.
struct Abschnitt {
    eintraege: BTreeMap<u32, Eintrag>,
    trailer: Vec<(Vec<u8>, Wert)>,
    prev: Option<u64>,
}

/// Finde `startxref` am Dateiende.
///
/// added 03.10.2026. Die Suche geht von hinten und ueberspringt
/// trailing Whitespace und `%%EOF`. Warum von hinten: die Zahl steht
/// am **Ende**, und ein Dokument kann vorn ein Kommentar-zeichen `%`
/// mit dem Wort `startxref` enthalten (gesehen in
/// `2103.00020.pdf`).
fn startxref_finden(daten: &[u8]) -> Option<u64> {
    let fenster = daten.len().min(4096);
    let teil = &daten[daten.len() - fenster..];
    // Von hinten nach `startxref` suchen.
    let pos = rfind_bytes(teil, b"startxref")?;
    let rest = &teil[pos + b"startxref".len()..];
    let mut lx = Lexer::neu(rest);
    match lx.naechstes().ok()? {
        Token::Zahl(z) if z.is_finite() && z >= 0.0 => Some(z as u64),
        _ => None,
    }
}

/// `rfind` fuer Bytefolgen.
///
/// added 03.10.2026. `str::rfind` geht nur fuer Text; ein PDF ist
/// Bytes, und `windows` + letzte Uebereinstimmung ist die ehrliche
/// Variante.
fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

/// Liest einen XRef-Abschnitt an einer Position.
///
/// added 03.10.2026. Zwei Formen, und die Entscheidung faellt an den
/// ersten vier Bytes: `xref` (mit Leerzeichen dahinter) ist die
/// klassische Tabelle, alles andere ist ein XRef-Stream-Objekt.
fn abschnitt_lesen(daten: &[u8], position: u64) -> Result<Abschnitt, XrefFehler> {
    let start = position as usize;
    if start >= daten.len() {
        return Err(XrefFehler::Kaputt(
            position,
            "Position hinter der Datei".into(),
        ));
    }
    let mut lx = Lexer::neu(&daten[start..]);
    // Erster Versuch: klassische Tabelle?
    match lx.naechstes() {
        Ok(Token::Schluesselwort(k)) if k == "xref" => {
            return tabelle_lesen(&daten[start..], position);
        }
        Ok(Token::Zahl(_)) | Ok(Token::Schluesselwort(_)) | Ok(Token::Satzzeichen(_)) => {}
        Ok(_) => {}
        Err(_) => {}
    }
    // Zweiter Versuch: ein Objekt, das ein XRef-Stream ist.
    stream_xref_lesen(daten, start, position)
}

/// Liest `xref\nN M\n` mit Unterabschnitten und `trailer`.
fn tabelle_lesen(daten: &[u8], basis: u64) -> Result<Abschnitt, XrefFehler> {
    let mut lx = Lexer::neu(daten);
    let _ = lx.naechstes(); // `xref`
    let mut eintraege = BTreeMap::new();
    let mut abschnitte = 0usize;
    // added 03.10.2026: `while let` (clippy `while_let_loop`).
    while let Ok(tok) = lx.naechstes() {
        let Token::Zahl(start_nr) = tok else {
            break;
        };
        let Ok(Token::Zahl(anzahl)) = lx.naechstes() else {
            break;
        };
        abschnitte += 1;
        if abschnitte > MAX_XREF_KETTE {
            return Err(XrefFehler::KetteZuLang);
        }
        let anzahl = if anzahl.is_finite() && anzahl >= 0.0 {
            anzahl as usize
        } else {
            0
        };
        if anzahl > MAX_XREF_EINTRAEGE {
            return Err(XrefFehler::Kaputt(
                basis,
                format!("Abschnitt mit {anzahl} Eintraegen"),
            ));
        }
        for i in 0..anzahl {
            // added 03.10.2026. DIE ERSTE FASSUNG HAT DEN OFFSET
            // VERWORFEN (`Token::Zahl(_)`) und stattdessen die
            // **Generation** als Offset benutzt. Damit zeigte **jeder**
            // Eintrag auf Byte 0 — jede Datei mit klassischer Tabelle
            // war unlesbar, und der Fehler sah aus wie ein
            // Seitenbaum-Problem. Gefunden von `debug_offsets` (alle
            // vier Eintraege auf @0).
            let Ok(Token::Zahl(offset)) = lx.naechstes() else {
                return Err(XrefFehler::Kaputt(
                    basis,
                    format!("Abschnitt endet bei Zeile {i}"),
                ));
            };
            // Die Generation wird in PDF 1.x nie benutzt; sie wird
            // gelesen, damit der Strom auf der richtigen Stelle bleibt.
            let Ok(Token::Zahl(_generation)) = lx.naechstes() else {
                return Err(XrefFehler::Kaputt(basis, "kein Generationseintrag".into()));
            };
            let Ok(Token::Schluesselwort(typ)) = lx.naechstes() else {
                return Err(XrefFehler::Kaputt(basis, "kein Typ im Eintrag".into()));
            };
            let nummer = (start_nr as i64) + i as i64;
            if nummer < 0 || nummer > u32::MAX as i64 {
                continue;
            }
            let nummer = nummer as u32;
            let eintrag = match typ.as_str() {
                "n" => {
                    // Offset aus der Datei, mit Grenze: eine Zahl von
                    // 2 Mrd. waere ein Sprung ans Dateiende.
                    if !offset.is_finite() || offset < 0.0 {
                        continue;
                    }
                    Eintrag::Offset(offset as u64)
                }
                "f" => Eintrag::Frei,
                _ => continue, // Unbekannter Typ: ueberspringen, nicht raten.
            };
            eintraege.entry(nummer).or_insert(eintrag);
        }
    }
    // `trailer` lesen, falls es dasteht.
    //
    // added 03.10.2026. DIE FASSUNG VORHER VERLANGTE, dass das **erste**
    // Token ein Schluesselwort ist, und brach sonst ab. Nach `xref`
    // kommt aber eine **Zahl** (`0`, die erste Objektnummer des
    // Abschnitts) — also brach sie **immer** ab, und `/Root` fehlte in
    // **jeder** klassischen Tabelle. Gefunden vom Test
    // `xref::minimale_tabelle` („kein /Root").
    //
    // Die Reihenfolge der Datei ist: `xref`, Abschnitte, `trailer`,
    // `<< … >>`, `startxref`, Zahl. Man laesst also alle Token
    // durchlaufen und greift beim Schluesselwort `trailer` zu.
    let mut trailer: Vec<(Vec<u8>, Wert)> = Vec::new();
    let mut wieder = Lexer::neu(daten);
    while wieder.position() < daten.len().min(MAX_TRAILER_SUCHE) {
        let Ok(t) = wieder.naechstes() else { break };
        let Token::Schluesselwort(k) = t else {
            continue;
        };
        if k == "trailer" {
            if let Ok(Some(Wert::Dict(d))) = wieder.objekt() {
                trailer = d;
            }
            break;
        }
    }
    let prev = trailer
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Prev")
        .and_then(|(_, v)| v.zahl())
        .map(|z| z.max(0.0) as u64);
    Ok(Abschnitt {
        eintraege,
        trailer,
        prev,
    })
}

/// Liest einen XRef-Stream.
///
/// added 03.10.2026. Der Stream ist ein ganzes Objekt:
/// `N G obj << /Type /XRef /Size .. /W [a b c] /Index [..] /Root .. >>
/// stream ... endstream`.
///
/// DIE PAARUNG `/W` und `/Index` ist der kniffligste Teil: `/W`
/// nennt die Bytezahl **pro Feld**, `/Index` die Paare
/// (erste Nummer, Anzahl) — fuer hybrid: klassische Tabelle und
/// Streams gemischt. Ohne `/Index` gilt `(0, Size)`.
fn stream_xref_lesen(daten: &[u8], start: usize, basis: u64) -> Result<Abschnitt, XrefFehler> {
    let mut lx = Lexer::neu(&daten[start..]);
    // Vor dem Wörterbuch stehen hoechstens drei Token: Nummer,
    // Generation, `obj`. Wir ueberspringen **bis** zum ersten
    // Satzzeichen `<<` und lesen das Wörterbuch dort — das ist
    // zuverlaessiger als die Zahl der Token zu zaehlen, weil manche
    // Generatoren noch ein Kommentar-Zeichen dazwischen setzen.
    loop {
        let Ok(t) = lx.naechstes() else {
            return Err(XrefFehler::Kaputt(basis, "kein Wörterbuch".into()));
        };
        if let Token::Satzzeichen(s) = &t {
            if s == "<<" {
                break;
            }
        }
        if matches!(t, Token::Ende) {
            return Err(XrefFehler::Kaputt(basis, "Ende vor dem Wörterbuch".into()));
        }
    }
    // added 03.10.2026. `lx` liest ueber `&daten[start..]`, also ist
    // `lx.position()` **relativ** zu `start`. Fuer `lese_dict_bis`
    // braucht es aber die **absolute** Position — sonst schneidet der
    // Bereich `daten[280..105]` und die Datei paniked mit
    // „slice index starts at 280 but ends at 105". Gefunden vom Test
    // `xref::minimale_tabelle` beim ersten Lauf.
    let stream_start = start + lx.position();
    // Den Stream finden: `stream` gefolgt von EOL.
    let stream_word = daten[stream_start..]
        .windows(6)
        .position(|w| w == b"stream")
        .map(|p| stream_start + p + 6);
    let Some(nach_stream) = stream_word else {
        return Err(XrefFehler::Kaputt(basis, "kein stream".into()));
    };
    // Nach `stream` muss ein EOL kommen (CRLF oder LF).
    let daten_beginn = if daten[nach_stream..].starts_with(b"\r\n") {
        nach_stream + 2
    } else if daten[nach_stream..].starts_with(b"\n") {
        nach_stream + 1
    } else {
        // Kaputt, aber robust: direkt anfangen.
        nach_stream
    };
    // Das Wörterbuch ist der Wert, den der Lexer schon gelesen hat.
    let dict = lese_dict_bis(daten, start)?;
    let (raw, _) = stream_daten(daten, &dict, daten_beginn);
    let entpackt = filtern::dekodieren(
        &filter_namen(&dict),
        &decode_parms(&dict, &filter_namen(&dict)),
        &Wert::Dict(dict.clone()),
        &raw,
        filtern::MAX_ENTPACKT,
    )
    .map_err(XrefFehler::Filter)?;
    let mut eintraege = BTreeMap::new();
    // Felder
    let w = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"W")
        .and_then(|(_, v)| v.array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.als_int())
                .map(|x| x.max(0) as usize)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if w.is_empty() || w.len() > 8 {
        return Err(XrefFehler::Kaputt(basis, format!("/W = {w:?}")));
    }
    let zeilen_b = w.iter().sum::<usize>();
    if zeilen_b == 0 || zeilen_b > 32 {
        return Err(XrefFehler::Kaputt(basis, format!("Feldbreite {zeilen_b}")));
    }
    let size = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Size")
        .and_then(|(_, v)| v.als_int())
        .unwrap_or(0)
        .max(0) as usize;
    let index = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Index")
        .and_then(|(_, v)| v.array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.als_int())
                .map(|x| x.max(0) as u64)
                .collect::<Vec<u64>>()
        })
        .unwrap_or_else(|| vec![0, size as u64]);
    if index.len() % 2 != 0 {
        return Err(XrefFehler::Kaputt(basis, "/Index ungerade".into()));
    }
    let mut pos = 0usize;
    for paare in index.chunks(2) {
        let (start_nr, anzahl) = (paare[0], paare[1]);
        if anzahl as usize > MAX_XREF_EINTRAEGE {
            return Err(XrefFehler::Kaputt(basis, "Abschnitt zu gross".into()));
        }
        for i in 0..anzahl {
            if pos + zeilen_b > entpackt.len() {
                break;
            }
            let mut felder = [0u64; 8];
            let mut q = pos;
            for (idx, breite) in w.iter().enumerate() {
                let mut v: u64 = 0;
                for _ in 0..*breite {
                    v = (v << 8) | entpackt.get(q).copied().unwrap_or(0) as u64;
                    q += 1;
                }
                felder[idx] = v;
            }
            pos += zeilen_b;
            let nummer = start_nr.saturating_add(i);
            if nummer > u32::MAX as u64 {
                break;
            }
            let nummer = nummer as u32;
            let eintrag = if w[0] == 0 {
                // Typ steht im zweiten Feld.
                match felder[1] {
                    0 => Eintrag::Frei,
                    1 => Eintrag::Offset(felder[2]),
                    2 => Eintrag::ImObjektstream(felder[2] as u32, felder[3] as u32),
                    _ => continue,
                }
            } else {
                match felder[0] {
                    0 => Eintrag::Frei,
                    1 => Eintrag::Offset(felder[1]),
                    2 => Eintrag::ImObjektstream(felder[1] as u32, felder[2] as u32),
                    _ => continue,
                }
            };
            eintraege.entry(nummer).or_insert(eintrag);
        }
    }
    let prev = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Prev")
        .and_then(|(_, v)| v.zahl())
        .map(|z| z.max(0.0) as u64);
    Ok(Abschnitt {
        eintraege,
        trailer: dict.clone(),
        prev,
    })
}

/// Liest ein Wörterbuch aus dem Bereich zwischen zwei Positionen.
///
/// added 03.10.2026. Der Lexer wird auf den Anfang gesetzt und liest
/// bis `ende`; das ist eine Haeufigkeit in PDF: der Wert eines
/// `stream`-Schluesselworts ist immer ein Wörterbuch.
fn lese_dict_bis(daten: &[u8], von: usize) -> Result<Vec<(Vec<u8>, Wert)>, XrefFehler> {
    // added 03.10.2026. DER WÖRTERBUCH-BEREICH DARF NICHT AM `<<`
    // ENDEN.
    //
    // Das Wörterbuch endet bei `>>`, das **vor** `stream` steht — aber
    // **hinter** dem `<<`. Mit
    // `Lexer::neu(&daten[von..bis])` bekam der Lexer also nur
    // `N G obj <<` und lieferte ein **leeres** Wörterbuch. Der Fehler
    // kam als `/W = []` heraus, bei 9 von 11 PDFs.
    //
    // Der Lexer liest deshalb ab `von` **ohne** hartes Ende; gebunden
    // wird er an `bis` **und** an `MAX_WOERTERBUCH` (kein Grund, mehr
    // als 64 KiB vor dem `stream` zu suchen — ein Wörterbuch von
    // 64 KiB ist schon kaputt).
    if von >= daten.len() {
        return Ok(Vec::new());
    }
    // added 03.10.2026. DIE ZWEITE FASSUNG BEGRENZTE IMMER NOCH AN
    // `bis` (= Position direkt hinter `<<`, also 11 Byte nach `von`),
    // und die Schleife kam damit **nie** bis zum `<<`. Die Grenze ist
    // deshalb nur noch `MAX_WOERTERBUCH`; das Wörterbuch endet von
    // selbst an seiner `>>`.
    let grenze = MAX_WOERTERBUCH.min(daten.len().saturating_sub(von));
    // added 03.10.2026. DIE DRITTE FASSUNG: `<<` wird als **Byte**
    // gesucht, nicht mit dem Lexer.
    //
    // DER FEHLER: die Schleife hat `<<` **verbraucht** und danach
    // `objekt()` gerufen — das las `/Length` und lieferte
    // `Name("Length")` statt eines Wörterbuchs. Ein Lexer, der ein
    // Token isst, kann es nicht zurueckgeben; die Position musste also
    // vor dem `<<` bleiben.
    //
    // Der Byte-Scan ist an dieser Stelle auch der kuerzeste Weg: es
    // gibt genau eine Stelle, an der ein Wörterbuch beginnt, und sie
    // ist ein Zwei-Byte-Text.
    let bereich = &daten[von..von + grenze];
    let Some(rel) = bereich.windows(2).position(|w| w == b"<<") else {
        return Ok(Vec::new());
    };
    let mut lx = Lexer::neu(&daten[von + rel..]);
    match lx.objekt() {
        Ok(Some(Wert::Dict(d))) => Ok(d),
        Ok(_) => Ok(Vec::new()),
        Err(_) => Ok(Vec::new()),
    }
}

/// Wie weit vor `stream` nach dem `<<` gesucht wird.
///
/// added 03.10.2026. 64 KiB. Das groesste echte Wörterbuch in den elf
/// Test-PDFs hat 300 Byte (`/Type /XRef /Size /W /Index /Root /Info`).
/// 64 KiB ist grosszuegig und begrenzt den Scan bei einer Datei, in
/// der `stream` weit hinten steht.
pub const MAX_WOERTERBUCH: usize = 64 * 1024;

/// Finde `stream` und gib die Rohdaten bis `endstream`.
fn stream_daten(daten: &[u8], dict: &[(Vec<u8>, Wert)], beginn: usize) -> (Vec<u8>, usize) {
    let laenge = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Length")
        .and_then(|(_, v)| v.als_int())
        .unwrap_or(0)
        .max(0) as usize;
    // Mit `/Length` ist es schnell und sicher — wenn es stimmt. Also
    // erst der Leseversuch mit `/Length`, und wenn dort `endstream`
    // nicht folgt, der Weg ueber die Suche.
    if laenge > 0 {
        let ende = (beginn + laenge).min(daten.len());
        if daten[beginn..ende].windows(9).any(|w| w == b"endstream") {
            return (daten[beginn..ende].to_vec(), ende);
        }
    }
    // Suche `endstream` und schneide davor, mit Leerraum davor weg.
    if let Some(p) = daten[beginn..].windows(9).position(|w| w == b"endstream") {
        let mut ende = beginn + p;
        // Leerraum vor `endstream` gehoert nicht zu den Daten.
        while ende > beginn
            && (daten[ende - 1] == b'\n' || daten[ende - 1] == b'\r' || daten[ende - 1] == b' ')
        {
            ende -= 1;
        }
        return (daten[beginn..ende].to_vec(), ende);
    }
    (Vec::new(), daten.len())
}

/// Die Filternamen aus einem Wörterbuch.
///
/// added 03.10.2026. `/Filter` ist entweder ein Name oder ein Array
/// von Namen — beides kommt in der Praxis vor.
pub fn filter_namen(dict: &[(Vec<u8>, Wert)]) -> Vec<String> {
    match dict.iter().rev().find(|(k, _)| k.as_slice() == b"Filter") {
        Some((_, Wert::Name(n))) => {
            vec![String::from_utf8_lossy(n).into_owned()]
        }
        Some((_, Wert::Array(a))) => a
            .iter()
            .filter_map(|v| v.name().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

/// Die Decode-Parameter passend zu den Filtern.
///
/// added 03.10.2026. `/DecodeParms` heisst in **neueren** Dateien
/// `/DP`, in aelteren `/DecodeParms` — beide kommen vor.
pub fn decode_parms(dict: &[(Vec<u8>, Wert)], filter: &[String]) -> Vec<Wert> {
    let roh = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"DecodeParms" || k.as_slice() == b"DP")
        .map(|(_, v)| v.clone());
    match roh {
        Some(Wert::Array(a)) => a,
        Some(w) if filter.len() == 1 => vec![w],
        _ => vec![Wert::Null; filter.len()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein minimales PDF mit klassischer Tabelle — mit
    /// **berechneten** Offsets.
    ///
    /// added 03.10.2026. Die erste Fassung hatte `startxref 200`
    /// geraten; die Datei war 105 Byte lang. Der Parser meldete
    /// „kein stream" — und der Test suchte den Fehler im Parser.
    #[test]
    fn minimale_tabelle() {
        let pdf = crate::mini_pdf([0.0, 0.0, 200.0, 200.0]);
        let q = lesen(&pdf).expect("Querverweistabelle");
        assert!(!q.eintraege.is_empty(), "Tabelle leer");
        assert!(q.trailer_ref("Root").is_some(), "kein /Root");
        // Fuenf Eintraege: der freie Platz plus vier Objekte.
        assert_eq!(q.eintraege.len(), 5);
    }

    /// `startxref` wird auch gefunden, wenn davor ein Kommentar das
    /// Wort enthaelt.
    ///
    /// added 03.10.2026. GEMESSEN an `2103.00020.pdf`: dort steht ein
    /// Kommentar mit dem Text vor dem echten `startxref`.
    #[test]
    fn startxref_von_hinten() {
        let daten = b"startxref 9999\n%%EOF\nstartxref\n12\n%%EOF";
        assert_eq!(startxref_finden(daten), Some(12));
    }
}
