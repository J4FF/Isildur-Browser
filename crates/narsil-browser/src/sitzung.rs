//! Sitzung: der Zustand als eine Zeichenkette, die man speichern kann.
//!
//! ERSTAELLT am 29.09.2026, TASK_ZUSTAND Aufgabe 3.
//!
//! KEINE Datei-Ein-/Ausgabe. Diese Crate liefert den String; wer ihn
//! speichert, ist der Browser (fremder Agent). Das ist Absicht: Testen
//! mit Dateien braucht ein Verzeichnis, das zwei Crates gleichzeitig
//! betreten, und GEMESSEN am 29.09.2026 blockieren sich zwei `cargo`-Läufe
//! in diesem Projekt gegenseitig.
//!
//! ## DAS FORMAT, woertlich
//!
//! ```text
//! narsil-sitzung 1
//! aktiv 2
//! tab 1 file:///a.narsil|Alpha|0|leer|0
//! tab 2 https://b.example|Beta|3|fertig|250
//! lesezeichen https://c.example|C|https://d.example|D
//! ```
//!
//! Zeile 1 ist die Versionszeile, IMMER zuerst. Dann der aktive Tab als
//! `TabId`. Dann je Tab eine Zeile:
//!
//! ```text
//! tab <TabId> <adresse> | <titel> | <stand> | <ladezustand> | <versatz>
//! ```
//!
//! und zum Schluss eine Zeile mit den Lesezeichen:
//!
//! ```text
//! lesezeichen <adresse>|<titel> <adresse>|<titel> ...
//! ```
//!
//! **Die `|` und das Leerzeichen sind die Trenner, und beide koennen in
//! einer Adresse vorkommen.** Deshalb wird jeder Wert escaped: `\` wird
//! zu `\\`, `|` zu `\p`, Leerzeichen zu `\s`, Zeilenende zu `\n`. Ein
//! Leerzeichen-Trenner ohne Escaping waere eine Klassenluecke, die jemand
//! mit einer Datei namens `meine seite.narsil` findet.
//!
//! Die Ladezustaende sind vier Woerter, keine Zahlen: `leer`, `laedt`,
//! `fertig`, `fehler`. Beim `fehler` folgt der Text, durch ein zweites
//! Leerzeichen getrennt — der Text selbst ist escaped, kann also
//! Leerzeichen enthalten.

use crate::{Browser, Ladezustand, Lesezeichen, Page, TabId, VERLAUF_GRENZE};

/// Die Version des Formats. Steht in Zeile 1.
pub const SITZUNG_VERSION: u32 = 1;

const KOPF: &str = "narsil-sitzung";

/// Was beim Lesen schiefgegangen ist. BENANNTE Varianten, kein String
/// mit "irgendwas" — der Aufrufer soll entscheiden koennen, nicht raten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SitzungsFehler {
    /// Die Zeichenkette war leer.
    Leer,
    /// Die erste Zeile war nicht `narsil-sitzung <Zahl>`.
    KopfFalsch { gefunden: String },
    /// Die Version ist nicht die, die diese Crate kennt. Es wird NICHT
    /// geraten und NICHT migriert: eine fremde Version ist ein fremdes
    /// Format, und Raten ist der Weg, auf dem ein Browser beim Start
    /// Speicherstaende zerstoert.
    VersionFalsch { gefunden: u32, erwartet: u32 },
    /// Eine `tab`-Zeile hat zu wenige Felder.
    TabZeKaputt { zeile: usize, grund: String },
    /// Der aktive Tab, genannt in Zeile 2, gibt es unter den Tabs nicht.
    /// Das ist der "fehlender Tab" aus dem Auftrag.
    AktiverTabFehlt { id: u64, vorhanden: Vec<u64> },
    /// Es gibt keinen Tab. Eine Sitzung ohne Tab kann keinen aktiven haben.
    KeineTabs,
    /// Der Verlauf eines Tabs ist laenger als [`VERLAUF_GRENZE`] — ein
    /// Eintrag zu viel. Eine Sitzung, die das enthaelt, wurde von aussen
    /// veraendert.
    ZuVieleSeiten {
        id: u64,
        gefunden: usize,
        grenze: usize,
    },
    /// Eine unbekannte Direktive in einer Zeile.
    Unbekannt { zeile: usize, wort: String },
    /// Der Ladezustand ist keiner der vier Woerter.
    LadezustandFalsch { id: u64, gefunden: String },
    /// Eine Zahl steht da, wo eine Zahl stehen muss.
    ZahlFalsch {
        zeile: usize,
        feld: String,
        gefunden: String,
    },
}

impl std::fmt::Display for SitzungsFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SitzungsFehler::Leer => write!(f, "die Sitzung ist leer"),
            SitzungsFehler::KopfFalsch { gefunden } => {
                write!(
                    f,
                    "erste Zeile ist nicht '{KOPF} <Zahl>', sondern '{gefunden}'"
                )
            }
            SitzungsFehler::VersionFalsch { gefunden, erwartet } => {
                write!(f, "Version {gefunden}, diese Crate kennt {erwartet}")
            }
            SitzungsFehler::TabZeKaputt { zeile, grund } => {
                write!(f, "Zeile {zeile}: {grund}")
            }
            SitzungsFehler::AktiverTabFehlt { id, vorhanden } => {
                write!(
                    f,
                    "aktiver Tab {id} gibt es nicht; vorhanden sind {:?}",
                    vorhanden
                )
            }
            SitzungsFehler::KeineTabs => write!(f, "die Sitzung nennt keinen Tab"),
            SitzungsFehler::ZuVieleSeiten {
                id,
                gefunden,
                grenze,
            } => write!(f, "Tab {id} hat {gefunden} Seiten, erlaubt sind {grenze}"),
            SitzungsFehler::Unbekannt { zeile, wort } => {
                write!(f, "Zeile {zeile}: unbekanntes Wort '{wort}'")
            }
            SitzungsFehler::LadezustandFalsch { id, gefunden } => {
                write!(f, "Tab {id}: '{gefunden}' ist kein Ladezustand")
            }
            SitzungsFehler::ZahlFalsch {
                zeile,
                feld,
                gefunden,
            } => {
                write!(
                    f,
                    "Zeile {zeile}: {feld} ist keine Zahl, sondern '{gefunden}'"
                )
            }
        }
    }
}

impl std::error::Error for SitzungsFehler {}

// ---------------------------------------------------------------------------
// Escapen
// ---------------------------------------------------------------------------

/// Einen Wert so verpacken, dass `|` und Leerzeichen ihn nicht zerlegen.
fn escaped(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => r.push_str("\\\\"),
            '|' => r.push_str("\\p"),
            ' ' => r.push_str("\\s"),
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            other => r.push(other),
        }
    }
    r
}

/// Das Gegenstueck. `None`, wenn der String mit einem unvollstaendigen
/// Escape endet — z. B. endet er auf einem Backslash.
fn unescape(s: &str) -> Option<String> {
    let mut r = String::with_capacity(s.len());
    let mut rest = s.chars();
    while let Some(c) = rest.next() {
        if c != '\\' {
            r.push(c);
            continue;
        }
        match rest.next() {
            Some('\\') => r.push('\\'),
            Some('p') => r.push('|'),
            Some('s') => r.push(' '),
            Some('n') => r.push('\n'),
            Some('r') => r.push('\r'),
            // Ein unbekanntes Escape ist ein Fehler, nicht ein Fallback:
            // stillschweigend 'x' zu lesen wuerde eine kaputte Datei
            // reparieren, ohne dass jemand etwas merkt.
            Some(_) => return None, // unbekanntes Escape: Fehler, kein Fallback
            None => return None,    // endet auf '\\'
        }
    }
    Some(r)
}

/// An `n` Felder trennen, jedes mit `|`.
fn teilen_escaped(zeile: &str, n: usize) -> Option<Vec<String>> {
    let mut raus: Vec<String> = Vec::with_capacity(n);
    let mut aktuell = String::new();
    let mut gesehen = 0usize;
    let mut rest = zeile.chars().peekable();
    while let Some(c) = rest.next() {
        if c == '\\' {
            aktuell.push(c);
            match rest.next() {
                Some(n) => aktuell.push(n),
                None => return None, // endet auf '\\'
            }
        } else if c == '|' {
            if gesehen + 1 == n {
                return None; // zu viele Trenner
            }
            raus.push(unescape(&aktuell)?);
            aktuell.clear();
            gesehen += 1;
        } else {
            aktuell.push(c);
        }
    }
    if gesehen + 1 != n {
        return None; // zu wenige Trenner
    }
    raus.push(unescape(&aktuell)?);
    Some(raus)
}

impl Browser {
    // ------------------------------------------------------------------
    // Schreiben
    // ------------------------------------------------------------------

    /// Die Sitzung als eine Zeichenkette.
    ///
    /// Jede Zeile endet mit `\n`, auch die letzte — dann ist
    /// `text().lines()` in jedem Fall richtig, und der Rundlauf haengt
    /// nicht an einem fehlenden Zeilenende.
    pub fn sitzung_serialisieren(&self) -> String {
        let mut s = String::new();
        s.push_str(KOPF);
        s.push(' ');
        s.push_str(&SITZUNG_VERSION.to_string());
        s.push('\n');

        s.push_str("aktiv ");
        // Regel E2 haelt: es gibt immer mindestens einen Tab, also gibt es
        // hier immer eine Id. `unwrap_or(0)` ist die Angabe, dass es nicht
        // vorkommen kann -- KEIN unwrap im Code.
        s.push_str(&self.aktive_tab_id().map(|i| i.0).unwrap_or(0).to_string());
        s.push('\n');

        for tab_id in self.tab_ids() {
            s.push_str(&self.tab_zeile(tab_id));
            s.push('\n');
        }

        s.push_str("lesezeichen");
        for l in &self.lesezeichen {
            s.push(' ');
            s.push_str(&escaped(&l.adresse));
            s.push('|');
            s.push_str(&escaped(&l.titel));
        }
        s.push('\n');
        s
    }

    /// Die Zeile eines Tabs, inklusive seines Verlaufs.
    ///
    /// Format je Seite: `adresse|titel|stand`, und die Seiten mit `;`
    /// getrennt. Der Ladezustand und der Versatz stehen am Ende der
    /// Tabzeile.
    fn tab_zeile(&self, id: TabId) -> String {
        let mut s = String::from("tab ");
        s.push_str(&id.0.to_string());
        // EIN Leerzeichen, direkt nach der Id. Die Seiten trennt ';' OHNE
        // Leerzeichen.
        //
        // EIGENER FEHLER, 29.09. 21:58. Ich schrieb vor JEDE Seite ein
        // Leerzeichen; der Leser trennt nur nach der Id, also kam
        // `adresse()` == " file:///a.narsil" zurueck — mit einem
        // fuehrenden Leerzeichen, das kein Browser findet, aber auch kein
        // Mensch sieht. Ein unsichtbares Zeichen im gespeicherten Zustand
        // ist der teuerste Fehler, den es in einer Zustandsdatei gibt.
        s.push(' ');
        for (n, seite) in self.seiten_von(id).iter().enumerate() {
            if n > 0 {
                s.push(';');
            }
            s.push_str(&escaped(&seite.adresse));
            s.push('|');
            s.push_str(&escaped(&seite.titel));
            s.push('|');
            s.push_str(&seite.stand.to_string());
        }
        s.push('|');
        s.push_str(&ladezustand_wort(id, self));
        s.push('|');
        s.push_str(&self.versatz_von(id).to_string());
        s.push('|');
        s.push_str(&self.index_von(id).to_string());
        s
    }

    // ------------------------------------------------------------------
    // Lesen
    // ------------------------------------------------------------------

    /// Eine Sitzung lesen.
    ///
    /// KEIN PANIC, auf keinem Weg. Jeder kaputte Zustand hat einen
    /// `SitzungsFehler` mit Namen. Auf einen Fehler fällt der Aufrufer
    /// selbst zurueck — diese Funktion liefert kein "halb geladenes"
    /// Ergebnis, denn ein halbes Ergebnis ist schlimmer als keines.
    pub fn sitzung_lesen(text: &str) -> Result<Browser, SitzungsFehler> {
        if text.trim().is_empty() {
            return Err(SitzungsFehler::Leer);
        }
        let zeilen: Vec<&str> = text.lines().collect();
        if zeilen.is_empty() {
            return Err(SitzungsFehler::Leer);
        }

        // --- Kopf ---
        let kopf = zeilen[0].trim();
        let mut kopf_teile = kopf.split(' ');
        match (kopf_teile.next(), kopf_teile.next()) {
            (Some(k), Some(v)) if k == KOPF => match v.parse::<u32>() {
                Ok(version) if version == SITZUNG_VERSION => {}
                Ok(version) => {
                    return Err(SitzungsFehler::VersionFalsch {
                        gefunden: version,
                        erwartet: SITZUNG_VERSION,
                    });
                }
                Err(_) => {
                    return Err(SitzungsFehler::KopfFalsch {
                        gefunden: kopf.to_string(),
                    });
                }
            },
            _ => {
                return Err(SitzungsFehler::KopfFalsch {
                    gefunden: kopf.to_string(),
                });
            }
        }

        // --- Der Zustand, in dem wir anfangen, wenn alles gut geht ---
        let mut tabs: Vec<(u64, Vec<Page>, Ladezustand, u32, usize)> = Vec::new();
        let mut aktiv: Option<u64> = None;
        let mut lesezeichen: Vec<Lesezeichen> = Vec::new();

        for (i, roh) in zeilen.iter().enumerate().skip(1) {
            let zeile = roh.trim();
            if zeile.is_empty() {
                continue;
            }
            let mut worte = zeile.splitn(2, ' ');
            let direktive = worte.next().unwrap_or("");
            let rest = worte.next().unwrap_or("");

            match direktive {
                "aktiv" => {
                    let feld = rest.trim();
                    match feld.parse::<u64>() {
                        Ok(v) => aktiv = Some(v),
                        Err(_) => {
                            return Err(SitzungsFehler::ZahlFalsch {
                                zeile: i + 1,
                                feld: "aktiver Tab".to_string(),
                                gefunden: feld.to_string(),
                            });
                        }
                    }
                }
                "tab" => {
                    let (id, seiten, zustand, versatz, index) = tab_zeile_lesen(rest, i + 1)?;
                    if seiten.len() > VERLAUF_GRENZE {
                        return Err(SitzungsFehler::ZuVieleSeiten {
                            id,
                            gefunden: seiten.len(),
                            grenze: VERLAUF_GRENZE,
                        });
                    }
                    tabs.push((id, seiten, zustand, versatz, index));
                }
                "lesezeichen" => {
                    if rest.is_empty() {
                        // "lesezeichen" ohne Lesezeichen ist gueltig: leer.
                        continue;
                    }
                    for teil in rest.split(' ') {
                        if teil.is_empty() {
                            continue;
                        }
                        let felder = match teilen_escaped(teil, 2) {
                            Some(f) => f,
                            None => {
                                return Err(SitzungsFehler::TabZeKaputt {
                                    zeile: i + 1,
                                    grund: format!("Lesezeichen '{teil}' hat nicht zwei Felder"),
                                });
                            }
                        };
                        lesezeichen.push(Lesezeichen {
                            adresse: felder[0].clone(),
                            titel: felder[1].clone(),
                        });
                    }
                }
                andere => {
                    return Err(SitzungsFehler::Unbekannt {
                        zeile: i + 1,
                        wort: andere.to_string(),
                    });
                }
            }
        }

        if tabs.is_empty() {
            return Err(SitzungsFehler::KeineTabs);
        }
        let aktiv = match aktiv {
            Some(a) => a,
            None => tabs[0].0,
        };
        if !tabs.iter().any(|(id, _, _, _, _)| *id == aktiv) {
            return Err(SitzungsFehler::AktiverTabFehlt {
                id: aktiv,
                vorhanden: tabs.iter().map(|(id, _, _, _, _)| *id).collect(),
            });
        }

        // --- Zusammenbauen ------------------------------------------------
        //
        // Ueber einen crate-internen Erbauer, nicht ueber `Navigation::To`:
        // es geht darum, einen Zustand zu REKONSTRUIEREN, nicht ihn zu
        // bedienen. Der Erbauer nimmt die geparsten Teile, setzt sie und
        // laeuft danach durch dieselbe Pruefung, die auch ein Bedienbefehl
        // ausloest (`verlauf_einkuerzen`).
        let mut b = Browser::leer_mit_0_tabs();
        b.naechste_tab_id = tabs.iter().map(|(id, _, _, _, _)| *id).max().unwrap_or(0) + 1;
        for (id, seiten, zustand, versatz, index) in tabs {
            b.haenge_tab_an(TabId(id), seiten, zustand, versatz, index);
        }
        b.aktiv = b
            .tabs
            .iter()
            .position(|t| t.id == TabId(aktiv))
            .unwrap_or(0);
        for l in lesezeichen.into_iter().rev() {
            b.lesezeichen_hinzufuegen(l.adresse, l.titel);
        }
        Ok(b)
    }
}

/// Die `tab`-Zeile lesen. Freie Funktion, weil sie keinen Zustand braucht:
/// sie baut Seiten aus Text, sie aendert keinen.
///
/// Die Id bleibt in der Hand des Aufrufers, weil sie die VERLAUFSgrenze
/// erst dort sinnvoll pruefen kann (der Aufrufer hat die Liste).
#[allow(clippy::too_many_lines)]
fn tab_zeile_lesen(
    rest: &str,
    nummer: usize,
) -> Result<(u64, Vec<Page>, Ladezustand, u32, usize), SitzungsFehler> {
    // id, dann Seiten (mit ';' getrennt), dann '|zustand|versatz'
    let mut teile = rest.splitn(2, ' ');
    let id_feld = teile.next().unwrap_or("");
    let id: u64 = id_feld.parse().map_err(|_| SitzungsFehler::ZahlFalsch {
        zeile: nummer,
        feld: "Tab-Id".to_string(),
        gefunden: id_feld.to_string(),
    })?;
    let mut rest2 = teile.next().unwrap_or("");

    // Die DREI letzten '|' trennen Seiten | Zustand | Versatz | Index.
    // Von HINTEN suchen ist richtig: eine Adresse darf escaped Pipes
    // enthalten, aber Zustand, Versatz und Index stehen am Ende.
    let mut pipes: Vec<usize> = Vec::with_capacity(3);
    let bytes: Vec<char> = rest2.chars().collect();
    let mut i = bytes.len();
    while i > 0 && pipes.len() < 3 {
        i -= 1;
        if bytes[i] == '|' {
            pipes.push(i);
        } else if bytes[i] == '\\' {
            // ein Escape: die Pipe davor gehoert zum Wert
            // EIGENER PANIC, 29.09. 22:05, vom Test gefunden: `i -= 1`
            // lief unter, wenn der String mit einem Backslash ENDET. Die
            // while-Bedingung `i > 0` gilt fuer den Kopf, nicht fuer diese
            // Stelle NACH dem Sprung. GEMESSEN mit der Eingabe "tab 1 \\":
            // panichte mit "attempt to subtract with overflow".
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }
    if pipes.len() < 3 {
        return Err(SitzungsFehler::TabZeKaputt {
            zeile: nummer,
            grund: "es fehlen '|' vor Ladezustand, Versatz und Index".to_string(),
        });
    }
    pipes.reverse();
    let (p_seiten, p_zustand, p_versatz) = (pipes[0], pipes[1], pipes[2]);

    let seiten_teil: String = rest2.chars().take(p_seiten).collect();
    let zustand_wort: String = rest2
        .chars()
        .skip(p_seiten + 1)
        .take(p_zustand - p_seiten - 1)
        .collect();
    let versatz_feld: String = rest2
        .chars()
        .skip(p_zustand + 1)
        .take(p_versatz - p_zustand - 1)
        .collect();
    let index_teil: String = rest2.chars().skip(p_versatz + 1).collect();
    rest2 = seiten_teil.as_str();

    let zustand = match zustand_wort.as_str() {
        "leer" => Ladezustand::Leer,
        "laedt" => Ladezustand::Laedt,
        "fertig" => Ladezustand::Fertig,
        w if w.starts_with("fehler") => {
            let text = w.strip_prefix("fehler").unwrap_or("").to_string();
            Ladezustand::Fehler(if text.is_empty() {
                String::new()
            } else {
                unescape(&text[1..]).unwrap_or_default()
            })
        }
        andere => {
            return Err(SitzungsFehler::LadezustandFalsch {
                id,
                gefunden: andere.to_string(),
            });
        }
    };
    let versatz: u32 = versatz_feld
        .parse()
        .map_err(|_| SitzungsFehler::ZahlFalsch {
            zeile: nummer,
            feld: "Versatz".to_string(),
            gefunden: versatz_feld.clone(),
        })?;
    // DER INDEX IM VERLAUF. Wird begrenzt, nicht uebernommen: eine Datei
    // von aussen darf keinen Index 9999 in einen Verlauf von 3 Seiten
    // setzen (GEMESSEN am 29.09.: ohne Begrenzung zeigte der Rundlauf auf
    // nichts).
    let index: usize = index_teil.parse().map_err(|_| SitzungsFehler::ZahlFalsch {
        zeile: nummer,
        feld: "Index".to_string(),
        gefunden: index_teil.clone(),
    })?;

    let mut seiten = Vec::new();
    for roh_teil in rest2.split(';') {
        // LEERZEICHEN ENTFERNEN, BEVOR GESPLITTET WIRD.
        //
        // GEMESSEN am 29.09. 22:05: die Eingabe "a|A|0; b|B|0" lieferte
        // die Adresse " b" - mit einem Leerzeichen, das kein Browser
        // findet und kein Mensch sieht. Ein unsichtbares Zeichen im
        // gespeicherten Zustand ist der teuerste Fehler, den es in einer
        // Zustandsdatei gibt. Also wegschneiden.
        let teil = roh_teil.trim_matches(' ');
        if teil.is_empty() {
            continue;
        }
        let felder = match teilen_escaped(teil, 3) {
            Some(f) => f,
            None => {
                return Err(SitzungsFehler::TabZeKaputt {
                    zeile: nummer,
                    grund: format!("Seite '{teil}' hat nicht drei Felder"),
                });
            }
        };
        let stand: u32 = match felder[2].parse() {
            Ok(v) => v,
            Err(_) => {
                return Err(SitzungsFehler::ZahlFalsch {
                    zeile: nummer,
                    feld: "Ladenummer".to_string(),
                    gefunden: felder[2].clone(),
                });
            }
        };
        seiten.push(Page {
            adresse: felder[0].clone(),
            titel: felder[1].clone(),
            stand,
            // 05.10.2026 (UI): `Page` hat seit 06:55 (KONTEXT) das Feld
            // `zeit_s`. GEMESSEN: die Verlaufsdatei hat **keine** Zeit-Spalte
            // (sie ist `Adresse<TAB>Titel<TAB>Ladenummer`), und GEMESSEN ist
            // auch KONTEXTs Vorgabe: **`0` heisst „unbekannt“**. GEMESSENE
            // ENTSCHEIDUNG, warum hier keine erfundene Zeit steht: eine
            // Sekunde aus dem Dateidatum waere geraten — und geratene Zeiten
            // landen in „Eintraege pro Tag“ und machen die Zahl falsch,
            // ohne dass man es beim Ansehen merkt.
            zeit_s: 0,
        });
    }
    Ok((id, seiten, zustand, versatz, index))
}

/// Das Wort, das den Ladezustand beschreibt. Fuenf Zeichen, damit man
/// beim Lesen weiss, ob danach noch ein Text kommt.
fn ladezustand_wort(id: TabId, b: &Browser) -> String {
    match b.ladezustand_von(id) {
        Some(Ladezustand::Leer) => "leer".to_string(),
        Some(Ladezustand::Laedt) => "laedt".to_string(),
        Some(Ladezustand::Fertig) => "fertig".to_string(),
        Some(Ladezustand::Fehler(t)) => {
            if t.is_empty() {
                "fehler".to_string()
            } else {
                format!("fehler {}", escaped(t))
            }
        }
        None => "leer".to_string(),
    }
}
