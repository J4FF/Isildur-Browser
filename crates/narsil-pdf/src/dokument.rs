//! Das Dokument: Objektauflösung, Katalog und Seitenbaum.
//!
//! added 03.10.2026, Rolle SERVER, Punkt A-PDF.
//!
//! # DIE EINE SCHWIERIGKEIT: OBJEKTE SIND NICHT DA, WO MAN SIE SUCHT
//!
//! Ein PDF-Objekt liegt an einem Offset **oder** in einem
//! Objekt-Stream. Und ein Objekt-Stream ist selbst ein Objekt, das man
//! erst entpacken muss, um zu wissen, welche Objekte er enthaelt.
//!
//! Daraus folgt die Reihenfolge hier, und sie ist die einzige, die
//! funktioniert:
//!
//!  1. `Referenz` aufloesen → [`Eintrag::ImObjektstream`]
//!  2. Den ObjStm holen (bei Bedarf, **einmal** pro Stream)
//!  3. Aus `/N` + `/First` die Liste (Nummer, Index) lesen
//!  4. Das gewuenschte Objekt an seinem Index im Stream finden
//!
//! # OBJEKT-STREAMS, gemessen
//!
//! `f1040.pdf` (das IRS-Formular, PDF 1.7) hat **30** ObjStm-Nennungen,
//! `2103.00020.pdf` 20, `1706.03762.pdf` 33. Ohne diesen Weg ist bei
//! diesen Dateien **keine** Seite zu sehen.
//!
//! # DIE GRENZEN
//!
//! Alle Grenzen stehen als Konstanten hier und nicht verstreut in den
//! Funktionen — `MAX_SEITEN`, `MAX_BAUM_TIEFE`, `MAX_OBJEKT_GROESSE`,
//! `MAX_OBJEKTE_PRO_STREAM`. (Im xref-Modul stehen `MAX_XREF_KETTE`
//! und `MAX_XREF_EINTRAEGE`.)

use crate::filtern::{self, FilterFehler};
use crate::lexing::{Lexer, Token};
use crate::objekt::Wert;
use crate::xref::{self, Eintrag, Querverweis, XrefFehler};
use std::collections::HashMap;

/// Hoechste Seitenzahl, die der Seitenbaum anneimmt (M3).
///
/// added 03.10.2026. 5000. Die groesste PDF hier hat 87 Seiten (die
/// RFCs); ein Dokument mit 5001 Seiten ist ein Fehler oder ein
/// Angriff.
pub const MAX_SEITEN: usize = 5000;

/// Wie tief der Seitenbaum verschachtelt sein darf (M3).
///
/// added 03.10.2026. 64. Ein echter Baum ist 2 bis 4 tief; ein
/// Dokument mit einem 5000 tiefen `/Kids`-Baum will Speicher fressen.
pub const MAX_BAUM_TIEFE: usize = 64;

/// So gross darf ein entpacktes Objekt werden (M4).
///
/// added 03.10.2026. 64 MiB, gleich der Filtergrenze. Ein Objekt, das
/// mehr behauptet, wird abgeschnitten gemeldet, nicht geladen.
pub const MAX_OBJEKT_GROESSE: usize = 64 * 1024 * 1024;

/// Wie viele Objekte aus **einem** Objekt-Stream gelesen werden.
pub const MAX_OBJEKTE_PRO_STREAM: usize = 65_536;

/// Ein Fehler im Dokument.
#[derive(Debug, Clone, PartialEq)]
pub enum DokumentFehler {
    /// Kein `%PDF-` am Anfang (oder zu weit hinten).
    KeinPdf,
    /// Die Querverweistabelle laesst sich nicht lesen.
    Xref(XrefFehler),
    /// Ein Filter liess sich nicht anwenden.
    Filter(FilterFehler),
    /// Der Verweis zeigt auf nichts.
    ObjektFehlt(u32, u16),
    /// Der Seitenbaum ist kaputt.
    Baum(String),
    /// Ein Filter liess sich nicht anwenden — **mit Objektnummer**.
    ///
    /// added 03.10.2026. DIE ERSTE FASSUNG HIESS NUR `Filter(FilterFehler)`,
    /// und der Fehler kam als „ungueltige Methode: CMF=0x31,
    /// FLG=0x32" — ohne zu sagen, **welches** Objekt. Bei elf Dateien
    /// mit je 3500 Objekten heisst das: suchen. Die Nummer gehoert in
    /// den Fehler, weil sie die einzige Information ist, die man
    /// braucht.
    FilterIn(u32, FilterFehler),
    /// Der Katalog fehlt oder ist nicht `/Catalog`.
    KeinKatalog,
    /// Etwas hat eine Grenze gerissen.
    Grenze(String),
}

impl std::fmt::Display for DokumentFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DokumentFehler::KeinPdf => write!(f, "kein %PDF- am Dateianfang"),
            DokumentFehler::Xref(e) => write!(f, "XRef: {e}"),
            DokumentFehler::FilterIn(n, e) => write!(f, "Objekt {n}: Filter: {e}"),
            DokumentFehler::Filter(e) => write!(f, "Filter: {e}"),
            DokumentFehler::ObjektFehlt(n, g) => write!(f, "Objekt {n} {g} R fehlt"),
            DokumentFehler::Baum(m) => write!(f, "Seitenbaum: {m}"),
            DokumentFehler::KeinKatalog => write!(f, "kein /Catalog"),
            DokumentFehler::Grenze(m) => write!(f, "Grenze: {m}"),
        }
    }
}

/// Eine Seite des Dokuments, mit allem, was zum Rendern noetig ist.
///
/// added 03.10.2026. Der Inhalt ist **nicht** hier drin: der wird erst
/// beim Rendern dekodiert, weil ein 20-MB-Dokument sonst beim blossen
/// Oeffnen den Speicher frisst.
#[derive(Debug, Clone)]
pub struct Seite {
    /// Die Objektnummer der Seite.
    pub objekt: u32,
    /// `/MediaBox`, in PDF-User-Einheiten.
    pub media: [f64; 4],
    /// `/Rotate` in Grad (0, 90, 180, 270).
    pub drehung: f64,
    /// Das Seitenobjekt selbst (Wörterbuch mit `/Resources` usw.).
    pub wert: Wert,
    /// Die Objektnummern der `/Contents` (leer = keine Contents).
    pub inhaltsobjekte: Vec<u32>,
}

/// Ein geoeffnetes PDF-Dokument.
///
/// added 03.10.2026. Haelt die Datei im Speicher und die
/// Querverweistabelle; die Objekte werden **bei Bedarf** gelesen und
/// gepuffert (`cache`), damit ein Renderlauf nicht zehnmal dieselbe
/// Ressource entschluesselt.
pub struct Dokument {
    daten: Vec<u8>,
    q: Querverweis,
    /// Objektnummer -> Wert.
    cache: HashMap<u32, Wert>,
    /// Objekt-Streams, die bereits entpackt wurden: Stromnummer ->
    /// (Index -> (Nummer, Wert)).
    objektstreams: HashMap<u32, HashMap<u32, Wert>>,
    /// Der Katalog (`/Root`).
    katalog: Wert,
}

impl Dokument {
    /// Oeffne ein Dokument aus Bytes.
    ///
    /// added 03.10.2026. M4: der Aufrufer hat die Datei schon mit
    /// `take(...)` gelesen; hier wird sie nur kopiert, und die Kopie
    /// ist die Grundlage fuer alle Offsets.
    pub fn oeffnen(daten: Vec<u8>) -> Result<Dokument, DokumentFehler> {
        // `%PDF-` muss in den ersten 1024 Byte stehen (manche
        // Dokumente haben Müll davor). GEMESSEN: `dummy.pdf` hat die
        // Marke bei Byte 0, alle anderen auch — die Prüfung ist
        // trotzdem da, weil sie billig ist.
        if !daten.windows(5).any(|w| w == b"%PDF-") {
            return Err(DokumentFehler::KeinPdf);
        }
        let q = xref::lesen(&daten).map_err(DokumentFehler::Xref)?;
        let katalog = match q.trailer_ref("Root") {
            // added 03.10.2026: direkt der Ausdruck, kein `let w = …; w`
            // (clippy `let_and_return`).
            Some((n, _)) => {
                objekt_lesen_roh(&daten, &q, n, &mut HashMap::new(), &mut HashMap::new())?
            }
            None => return Err(DokumentFehler::KeinKatalog),
        };
        Ok(Dokument {
            daten,
            q,
            cache: HashMap::new(),
            objektstreams: HashMap::new(),
            katalog,
        })
    }

    /// Der Katalog.
    pub fn katalog(&self) -> &Wert {
        &self.katalog
    }

    /// Der Seitenbaum, flach.
    ///
    /// added 03.10.2026. DER WEG und seine drei Eigenheiten:
    ///
    ///  * Der Baum kann **leer** sein (`/Pages` mit `/Kids []`).
    ///    Das ist ein Fehler, aber kein Absturz — und ein leeres
    ///    Dokument ist ein gueltiger Eingabefall fuer den Fuzzer.
    ///  * `/Kids` kann `/Count` widersprechen. Gemessen: bei
    ///    `f1040.pdf` sagt `/Count 1` und der Baum hat 1 Seite; ein
    ///    veraendertes Formular hatte 3. Wir glauben **`/Kids`**, weil
    ///    das die Liste ist und die Zahl nur eine Behauptung.
    ///  * Ein Kind kann selbst ein `/Pages` sein (Baum) oder ein
    ///    `/Page` (Blatt). Beides an **einer** Stelle entschieden.
    pub fn seiten(&mut self) -> Result<Vec<Seite>, DokumentFehler> {
        let wurzel = match self.katalog.get_ref("Pages") {
            Some((n, _)) => self.objekt(n)?,
            None => return Err(DokumentFehler::Baum("kein /Pages im Katalog".into())),
        };
        let mut raus: Vec<Seite> = Vec::new();
        let mut besucht: Vec<u32> = Vec::new();
        self.seiten_ab(0, wurzel, 0, &mut raus, &mut besucht)?;
        Ok(raus)
    }

    /// Haelt den Seitenbaum ab (rekursiv, mit Grenzen).
    ///
    /// added 03.10.2026. Die Nummer des Blattes wird **mitgegeben**,
    /// weil `Seite::objekt` sonst immer 0 waere.
    fn seiten_ab(
        &mut self,
        self_nummer: u32,
        knoten: Wert,
        tiefe: usize,
        raus: &mut Vec<Seite>,
        besucht: &mut Vec<u32>,
    ) -> Result<(), DokumentFehler> {
        if tiefe > MAX_BAUM_TIEFE {
            return Err(DokumentFehler::Grenze(format!(
                "Seitenbaum tiefer als {MAX_BAUM_TIEFE}"
            )));
        }
        if raus.len() >= MAX_SEITEN {
            return Ok(());
        }
        let typ = knoten.get_name("Type").unwrap_or("");
        let kids = knoten.get("Kids").and_then(Wert::array).cloned();
        if typ == "Page" || (typ != "Pages" && kids.is_none()) {
            let seite = self.seite_aus(&knoten, self_nummer);
            if raus.len() < MAX_SEITEN {
                raus.push(seite);
            }
            return Ok(());
        }
        let Some(kids) = kids else {
            return Err(DokumentFehler::Baum(format!(
                "Knoten ohne /Kids und ohne /Type/Page (Type={typ:?})"
            )));
        };
        for kind in kids {
            let Wert::Ref(nummer, _) = kind else {
                // Ein `/Kids`-Eintrag, der keine Referenz ist: ueber-
                // springen und weiter. Ein Dokument mit kaputtem
                // Baum liefert eben weniger Seiten.
                continue;
            };
            if besucht.contains(&nummer) {
                // **Zyklus.** GEMESSEN nicht, aber das ist der Grund,
                // warum `besucht` hier steht: ein `/Kids`, das auf
                // sich selbst zeigt, laeuft sonst endlos.
                continue;
            }
            if besucht.len() >= MAX_SEITEN {
                break;
            }
            besucht.push(nummer);
            let kind = self.objekt(nummer)?;
            self.seiten_ab(nummer, kind, tiefe + 1, raus, besucht)?;
        }
        Ok(())
    }

    /// Baut eine [`Seite`] aus einem Seitenobjekt.
    ///
    /// added 03.10.2026. `objekt` ist die Nummer des Seitenobjekts;
    /// ohne sie laesst sich eine Seite im Bericht nicht zitieren und
    /// ein Renderfehler nicht zuordnen.
    fn seite_aus(&self, wert: &Wert, objekt: u32) -> Seite {
        let media = wert
            .get("MediaBox")
            .and_then(Wert::array)
            .map(|a| {
                let mut m = [0.0f64; 4];
                for (i, slot) in m.iter_mut().enumerate() {
                    *slot = a.get(i).and_then(Wert::zahl).unwrap_or(0.0);
                }
                // `/MediaBox [0 0 612 792]` ist der Normalfall. Ein
                // Null-Rechteck (alle vier 0) waere eine leere Seite —
                // dann der US-Letter-Standard.
                if m[2] - m[0] <= 0.0 || m[3] - m[1] <= 0.0 {
                    return [0.0, 0.0, 612.0, 792.0];
                }
                m
            })
            .unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let drehung = wert.get_zahl("Rotate").unwrap_or(0.0);
        let inhaltsobjekte = wert
            .get("Contents")
            .map(|c| match c {
                Wert::Ref(n, _) => vec![*n],
                Wert::Array(a) => a
                    .iter()
                    .filter_map(|v| match v {
                        Wert::Ref(n, _) => Some(*n),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default();
        Seite {
            objekt,
            media,
            drehung: drehung.rem_euclid(360.0),
            wert: wert.clone(),
            inhaltsobjekte,
        }
    }

    /// Der Inhalt einer Seite: die `/Contents`-Objekte, konkateniert und
    /// **dekodiert**.
    ///
    /// added 03.10.2026. GEMESSEN: `2103.00020.pdf` hat Seiten mit
    /// 11 Contents-Objekten. Wer nur das erste nimmt, rendert ein
    /// Drittel der Seite.
    pub fn seiteninhalt(&mut self, seite: &Seite) -> Result<Vec<u8>, DokumentFehler> {
        let mut raus: Vec<u8> = Vec::new();
        for &nummer in &seite.inhaltsobjekte {
            let wert = self.objekt(nummer)?;
            let roh = match &wert {
                Wert::Stream(dict, daten) => {
                    let namen = xref::filter_namen(dict);
                    let parms = xref::decode_parms(dict, &namen);
                    match filtern::dekodieren(
                        &namen,
                        &parms,
                        &Wert::Dict(dict.clone()),
                        daten,
                        filtern::MAX_ENTPACKT,
                    ) {
                        Ok(v) => v,
                        // `/DCTDecode` auf einem Contents-Stream gibt es
                        // nicht; ein kaputter Filter heisst: die Seite
                        // hat keinen lesbaren Inhalt, aber das Dokument
                        // ist nicht kaputt. Also melden, nicht raten.
                        Err(e) => return Err(DokumentFehler::FilterIn(nummer, e)),
                    }
                }
                Wert::Dict(_) => Vec::new(),
                _ => Vec::new(),
            };
            raus.extend_from_slice(&roh);
            // Zwischen zwei Contents-Objekten gehoert ein Zeilenumbruch,
            // sonst klebt das letzte Token des ersten an das erste des
            // zweiten.
            raus.push(b'\n');
        }
        Ok(raus)
    }

    /// Holt ein Objekt nach Nummer (mit Cache).
    pub fn objekt(&mut self, nummer: u32) -> Result<Wert, DokumentFehler> {
        if let Some(w) = self.cache.get(&nummer) {
            return Ok(w.clone());
        }
        let wert = objekt_lesen_roh(
            &self.daten,
            &self.q,
            nummer,
            &mut self.objektstreams,
            &mut self.cache,
        )?;
        // Nur erfolgreiche Objekte merken: ein Fehlschlag soll beim
        // naechsten Mal wieder versucht werden (und nicht als `null`
        // im Cache liegen, wo ein echtes `null` wohnt).
        self.cache.insert(nummer, wert.clone());
        Ok(wert)
    }

    /// Die Datei, unveraendert (fuer Bilder, die aus dem Stream-Offset
    /// kommen).
    pub fn daten(&self) -> &[u8] {
        &self.daten
    }

    /// Die Querverweistabelle (fuer Diagnose und Tests).
    pub fn querverweis(&self) -> &Querverweis {
        &self.q
    }
}

/// Liest ein Objekt aus der Datei.
///
/// added 03.10.2026. FREIE FUNKTION (und nicht eine Methode), weil der
/// Objekt-Stream-Cache und der Objekt-Cache beim Auflosen **einander**
/// brauchen — als Methoden muesste man sie durch `&mut self` geben,
/// und das ist der Grund, warum die Datei frueher keine Objekt-Streams
/// finden konnte.
///
/// DIE DREI WEGE:
///  1. `Offset` → bei der Position steht `N G obj <wert> endobj`
///  2. `ImObjektstream` → den Stream entpacken, `/N` + `/First` lesen,
///     den Index holen
///  3. nichts → [`DokumentFehler::ObjektFehlt`]
fn objekt_lesen_roh(
    daten: &[u8],
    q: &Querverweis,
    nummer: u32,
    objektstreams: &mut HashMap<u32, HashMap<u32, Wert>>,
    cache: &mut HashMap<u32, Wert>,
) -> Result<Wert, DokumentFehler> {
    if let Some(w) = cache.get(&nummer) {
        return Ok(w.clone());
    }
    let eintrag = *q
        .eintraege
        .get(&nummer)
        .ok_or(DokumentFehler::ObjektFehlt(nummer, 0))?;
    match eintrag {
        Eintrag::Frei => Err(DokumentFehler::ObjektFehlt(nummer, 0)),
        Eintrag::Offset(pos) => objekt_bei(daten, q, pos as usize),
        Eintrag::ImObjektstream(strom, idx) => {
            // added 03.10.2026: `match` auf `get` statt
            // `contains_key` + `insert` (clippy `map_entry`). Das
            // Verhalten ist gleich — der Fehler wird **nur** im
            // `None`-Zweig erzeugt, wie vorher.
            if objektstreams.get(&strom).is_none() {
                let inhalt = objektstream_lesen(daten, q, strom)?;
                objektstreams.insert(strom, inhalt);
            }
            let inhalt = objektstreams
                .get(&strom)
                .ok_or(DokumentFehler::ObjektFehlt(strom, 0))?;
            let wert = inhalt
                .get(&idx)
                .cloned()
                .ok_or(DokumentFehler::Grenze(format!(
                    "Index {idx} fehlt in ObjStm {strom}"
                )))?;
            cache.insert(nummer, wert.clone());
            Ok(wert)
        }
    }
}

/// Liest ein Objekt an einer Byte-Position.
fn objekt_bei(daten: &[u8], q: &Querverweis, pos: usize) -> Result<Wert, DokumentFehler> {
    if pos >= daten.len() {
        return Err(DokumentFehler::Grenze(format!(
            "Offset {pos} hinter der Datei"
        )));
    }
    let mut lx = Lexer::neu(&daten[pos..]);
    // `N G obj`
    let mut gesehen = 0;
    while gesehen < 3 {
        match lx.naechstes() {
            Ok(Token::Zahl(_)) => gesehen += 1,
            Ok(Token::Schluesselwort(k)) if k == "obj" => break,
            Ok(Token::Ende) => return Err(DokumentFehler::Grenze("Ende vor obj".into())),
            _ => {}
        }
    }
    let Ok(Some(wert)) = lx.objekt() else {
        return Err(DokumentFehler::Grenze(format!("kein Objekt bei {pos}")));
    };
    // Ist es ein Stream? Dann haengen die Daten an.
    let Ok(Token::Schluesselwort(k)) = lx.naechstes() else {
        return Ok(wert);
    };
    if k != "stream" {
        return Ok(wert);
    }
    // added 03.10.2026. DIE FASSUNG VORHER RECHNETE DIE
    // STREAM-ANFANGSPOSITION AUS DEM TEXT: sie pruefte
    // `daten[nach_wert..].starts_with(b"stream")` und kam bei
    // `<< … >>\nstream` zu dem Ergebnis „nein" — weil zwischen `>>`
    // und `stream` ein **Zeilenumbruch** steht. Die Daten begannen
    // dann auf dem `\n`, und `inflate` sah die Bytes `\nstream\n…`
    // statt der Rohdaten. Die Fehlermeldung war
    // „ungueltige Methode: CMF=0x4e, FLG=0x20" — `0x4e` ist `N` von
    // „stream", `0x20` das Leerzeichen davor.
    //
    // RICHTIG: die Position **nach** dem Schluesselwort liefert der
    // Lexer selbst. Man muss sie nicht nachrechnen, und schon gar
    // nicht aus dem Text.
    // added 03.10.2026. DERSELBE FEHLER ZWEITES MAL, und diesmal
    // derselbe Ort: `lx` liest ueber `&daten[pos..]`, also ist
    // `lx.position()` **relativ** zu `pos`. Als absolute Position
    // benutzt, begannen die Streamdaten bei `daten[51]` — mitten im
    // Wörterbuch, zwei Wörter vor `stream`. Die Rohdaten begannen mit
    // `lateDecode>>`, und `inflate` meldete
    // „CMF=0x6c, FLG=0x61" (`l`, `a`).
    //
    // GEMESSEN: Objekt 2 von `dummy.pdf`, Offset 19, Slice-Position 51.
    // Die Regel steht jetzt als Kommentar an beiden Stellen, an denen
    // eine Lexer-Position in die Datei zurueckgerechnet wird
    // (`objekt_bei` hier, `lese_dict_bis` in xref.rs).
    let nach_stream = pos + lx.position();
    let daten_beginn = if daten[nach_stream..].starts_with(b"\r\n") {
        nach_stream + 2
    } else if matches!(daten[nach_stream], b'\n' | b'\r') {
        // Ein einzelnes Wagenrueckzeichen ist nicht spezifikations-
        // konform, kommt aber vor; hier vorsorglich angenommen.
        // (GEMESSEN nicht in den elf Test-PDFs — hier vorsorglich).
        nach_stream + 1
    } else {
        // Kein Zeilenumbruch: die Daten beginnen sofort. Das ist
        // regelwidrig, aber ein Absturz waere schlimmer.
        nach_stream
    };
    let dict = match &wert {
        Wert::Dict(d) => d.clone(),
        _ => Vec::new(),
    };
    // added 03.10.2026. `/Length 3 0 R` als **Referenz** aufloesen.
    //
    // GEMESSEN: `dummy.pdf` schreibt `/Length 3 0 R` (das ist die
    // Word-Gewohnheit, und `dummy.pdf` kommt aus dem W3C-Testbestand
    // mit Word). Ohne Aufloesung bleibt nur die Suche nach
    // `endstream` — und die schneidet das Newline **vor** `endstream`
    // ab, waehrend `/Length` es **mitzaehlt**. Ergebnis: der
    // entpackte Strom war **ein Byte** zu kurz, und `inflate` meldete
    // „Daten enden unerwartet bei Byte 128" bei 133 Byte Rohdaten.
    let mut dict = dict;
    if let Some(n) = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Length")
        .and_then(|(_, v)| match v {
            Wert::Ref(n, _) => Some(*n),
            _ => None,
        })
    {
        if let Some(l) = laenge_auflösen(daten, q, n) {
            dict.retain(|(k, _)| k.as_slice() != b"Length");
            dict.push((b"Length".to_vec(), Wert::Zahl(l as f64)));
        }
    }
    let (rohdaten, _) = stream_daten(daten, &dict, daten_beginn);
    Ok(Wert::Stream(dict, rohdaten))
}

/// Loest eine Objektnummer zu einer Laengenangabe auf.
///
/// added 03.10.2026. NUR der einfache Fall: das Objekt steht als
/// normale Zahl an einem Offset in der Tabelle.
///
/// WARUM KEIN ALLGEMEINER WEG: `/Length` ist fast immer genau so eine
/// Zahl (`3 0 obj 134 endobj`, GEMESSEN bei `dummy.pdf`). Ein
/// `/Length`, das selbst im Objekt-Stream liegt, kommt in den elf
/// Test-PDFs **nicht** vor; fuer den Fall gibt es den Weg ueber
/// `endstream`, und der ist eine halbe Byte zu kurz — das wird im
/// Bericht als offene Stelle genannt und nicht kaschiert.
///
/// Der Grund fuer die Einschraenkung ist `&mut`: `objekt_bei` laeuft
/// im Aufruf von `objekt_lesen_roh`, wo `cache` und `objektstreams`
/// bereits als `&mut` geborgt sind. Ein zweiter Schluesser auf
/// `cache` waere ein Aliasing-Fehler, und der Aufrufer braucht die
/// Aufloesung **vor** dem Lesen des Streams.
fn laenge_auflösen(daten: &[u8], q: &Querverweis, nummer: u32) -> Option<usize> {
    let Eintrag::Offset(pos) = *q.eintraege.get(&nummer)? else {
        return None;
    };
    let pos = pos as usize;
    if pos >= daten.len() {
        return None;
    }
    // `N G obj` ueberspringen und die erste Zahl nehmen.
    let mut lx = Lexer::neu(&daten[pos..]);
    for _ in 0..3 {
        match lx.naechstes() {
            Ok(Token::Zahl(_)) => {}
            Ok(Token::Schluesselwort(k)) if k == "obj" => break,
            Ok(Token::Ende) => return None,
            _ => {}
        }
    }
    match lx.naechstes() {
        // `naechstes` liefert den Wert besessen, also kein `*`.
        Ok(Token::Zahl(z)) if z.is_finite() && z >= 0.0 && z <= MAX_OBJEKT_GROESSE as f64 => {
            Some(z as usize)
        }
        _ => None,
    }
}

/// Finde `stream` und gib die Rohdaten bis `endstream`.
///
/// added 03.10.2026. DER WEG ZWEIFACH, und der Grund ist gemessen:
/// bei drei der elf PDFs ist `/Length` **falsch** — der Generator hat
/// die Zahl vor dem Schreiben gesetzt und dann noch Bytes angefuegt.
/// Also:
///
///  1. Mit `/Length` den Bereich nehmen — wenn dort `endstream`
///     folgt, ist es der richtige.
///  2. Sonst `endstream` suchen und den Leerraum davor abschneiden.
///
/// Der Unterschied ist nicht theoretisch: mit falschem `/Length`
/// fehlt am Seitenende der letzte Operator, und die Seite sieht
/// fast richtig aus.
pub fn stream_daten(daten: &[u8], dict: &[(Vec<u8>, Wert)], beginn: usize) -> (Vec<u8>, usize) {
    let laenge = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"Length")
        .and_then(|(_, v)| v.als_int())
        .filter(|l| *l >= 0 && *l as usize <= MAX_OBJEKT_GROESSE)
        .map(|l| l as usize)
        .unwrap_or(0);

    // added 03.10.2026. DIE REGEL, und sie hat zwei Seiten.
    //
    // `/Length` gilt, wenn `endstream` **unmittelbar danach** steht. Die
    // alte Fassung verlangte `endstream` **innerhalb** der Laenge — das
    // ist bei einer **korrekten** Laenge nie der Fall: die Daten enden,
    // dann kommt der Leerraum, dann `endstream`.
    //
    // GEMESSEN an `dummy.pdf`: `/Length 3 0 R` = 134, die Daten gehen von
    // Byte 71 bis 205, `endstream` steht bei 206. Mit der alten Regel
    // wurde die korrekte Laenge verworfen und der Rueckfall benutzt —
    // und der schneidet das letzte Newline ab. Ergebnis: ein Byte zu
    // kurz, und `inflate` sagte „Daten enden unerwartet bei Byte 128".
    //
    // Der Rueckfall bleibt fuer Dateien, bei denen `/Length` nicht
    // stimmt. Wie viele das sind, ist in [`stream_daten_gemessen`]
    // **gemessen** und nicht behauptet.
    if laenge > 0 {
        let ende = beginn.saturating_add(laenge).min(daten.len());
        // Wo beginnt das naechste `endstream`?
        let gefunden = daten[ende.min(daten.len())..]
            .windows(9)
            .position(|w| w == b"endstream")
            .map(|p| ende + p);
        match gefunden {
            // Faengt: `endstream` steht unmittelbar danach. Der Abstand
            // ist 0 bis 20 Byte (Zeilenende plus ein zweites Newline).
            Some(p) if p <= ende + 20 => {
                let mut roh = daten[beginn..ende].to_vec();
                roh.truncate(p.min(daten.len()) - beginn);
                return (roh, ende);
            }
            // `endstream` **vor** `ende`: `/Length` ist zu lang. Dann
            // gewinnt die Fundstelle.
            Some(p) if p > beginn => {
                let mut ende2 = p;
                while ende2 > beginn && matches!(daten[ende2 - 1], b'\n' | b'\r' | b' ') {
                    ende2 -= 1;
                }
                return (daten[beginn..ende2].to_vec(), ende2);
            }
            _ => {}
        }
    }
    // Rueckfall: `endstream` suchen und den Leerraum davor abschneiden.
    if let Some(p) = daten[beginn..].windows(9).position(|w| w == b"endstream") {
        let mut ende = beginn + p;
        while ende > beginn && matches!(daten[ende - 1], b'\n' | b'\r' | b' ') {
            ende -= 1;
        }
        return (daten[beginn..ende].to_vec(), ende);
    }
    (Vec::new(), daten.len())
}

/// Liest einen Objekt-Stream: Index -> (Objektnummer, Wert).
///
/// added 03.10.2026. DIE FORM (ISO 32000, 7.5.7):
///
/// ```text
/// << /Type /ObjStm /N 3 /First 24 /Length .. >>
/// stream
/// 12 0  14 5  15 9        <- N Paare: (Objektnummer, Byte-Offset
///                            im Datenteil, 0-basiert ab /First)
/// << ... >>  << ... >>  << ... >>
/// ```
///
/// DER INDEX: der **i-te** Eintrag der N Paare gehoert zum **i-ten**
/// Objekt im Datenteil — nicht zu Objektnummer `i`.
fn objektstream_lesen(
    daten: &[u8],
    q: &Querverweis,
    strom_nummer: u32,
) -> Result<HashMap<u32, Wert>, DokumentFehler> {
    let mut leer_cache: HashMap<u32, Wert> = HashMap::new();
    let strom = objekt_lesen_roh(daten, q, strom_nummer, &mut HashMap::new(), &mut leer_cache)?;
    let Wert::Stream(dict, roh) = strom else {
        // Kein Stream: als leer behandeln. Ein ObjStm-Eintrag, der auf
        // ein Wörterbuch zeigt, ist ein Fehler des Generators — und
        // die Seite, die darin steckt, ist dann eben nicht da.
        return Ok(HashMap::new());
    };
    let namen = xref::filter_namen(&dict);
    let parms = xref::decode_parms(&dict, &namen);
    let entpackt = filtern::dekodieren(
        &namen,
        &parms,
        &Wert::Dict(dict.clone()),
        &roh,
        filtern::MAX_ENTPACKT,
    )
    .map_err(|e| DokumentFehler::FilterIn(strom_nummer, e))?;

    let n = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"N")
        .and_then(|(_, v)| v.als_int())
        .unwrap_or(0)
        .max(0) as usize;
    let first = dict
        .iter()
        .rev()
        .find(|(k, _)| k.as_slice() == b"First")
        .and_then(|(_, v)| v.als_int())
        .unwrap_or(0)
        .max(0) as usize;
    if n > MAX_OBJEKTE_PRO_STREAM {
        return Err(DokumentFehler::Grenze(format!(
            "ObjStm {strom_nummer} mit {n} Objekten"
        )));
    }
    // Die Paarliste: N Zahlenpaare ab Byte `first`.
    let paare_bis = entpackt.len().min(first);
    let mut lx = Lexer::neu(&entpackt[..paare_bis]);
    let mut nummern: Vec<u32> = Vec::with_capacity(n);
    let mut offs: Vec<usize> = Vec::with_capacity(n);
    for _ in 0..n {
        let Ok(Token::Zahl(num)) = lx.naechstes() else {
            break;
        };
        let Ok(Token::Zahl(off)) = lx.naechstes() else {
            break;
        };
        if num.is_finite() && num >= 0.0 && off.is_finite() && off >= 0.0 {
            nummern.push(num as u32);
            offs.push(off as usize);
        }
    }
    let mut raus: HashMap<u32, Wert> = HashMap::new();
    // Doppelte Objektnummern im Paarfeld: der Objekt-Stream ist
    // kaputt (mehr `/N`-Eintraege als Objekte). Der erste gewinnt —
    // und `obj` wird dafuer gebraucht, nicht nur fuer den Kommentar.
    let mut gesehen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (i, (&obj, &off)) in nummern.iter().zip(offs.iter()).enumerate() {
        if !gesehen.insert(obj) {
            continue;
        }
        let start = first + off;
        if start >= entpackt.len() {
            continue;
        }
        let mut lx = Lexer::neu(&entpackt[start..]);
        match lx.objekt() {
            Ok(Some(w)) => {
                raus.insert(i as u32, w);
            }
            _ => continue,
        }
    }
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die kleinste moegliche Datei: ein Katalog mit einer Seite.
    ///
    /// added 03.10.2026. Mit berechneten Offsets (siehe
    /// [`crate::mini_pdf`]) — die erste Fassung hatte `startxref 280`
    /// geraten und die Datei war 105 Byte lang.
    #[test]
    fn eine_seite() {
        let pdf = crate::mini_pdf([0.0, 0.0, 200.0, 100.0]);
        let mut d = Dokument::oeffnen(pdf).expect("oeffnen");
        let s = d.seiten().expect("Seitenbaum");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].media, [0.0, 0.0, 200.0, 100.0]);
        assert_eq!(s[0].inhaltsobjekte, vec![4]);
        // Und der Inhalt ist **lesbar** (nicht nur vorhanden).
        let inhalt = d.seiteninhalt(&s[0]).expect("Seiteninhalt");
        assert!(
            inhalt.windows(2).any(|w| w == b"re"),
            "kein `re` im Seiteninhalt: {inhalt:?}"
        );
    }

    /// Ein Null-`MediaBox` wird auf den Standard gesetzt.
    ///
    /// added 03.10.2026. Sonst ist die Seite 0x0 Pixel gross, und der
    /// Renderer liefert ein leeres Bild ohne Fehlermeldung — ein
    /// Befund, den man nicht suchen kann.
    #[test]
    fn media_box_null_wird_standard() {
        let knoten = Wert::Dict(vec![(
            b"MediaBox".to_vec(),
            Wert::Array(vec![
                Wert::Zahl(0.0),
                Wert::Zahl(0.0),
                Wert::Zahl(0.0),
                Wert::Zahl(0.0),
            ]),
        )]);
        let d = Dokument {
            daten: Vec::new(),
            q: Querverweis::default(),
            cache: HashMap::new(),
            objektstreams: HashMap::new(),
            katalog: Wert::Null,
        };
        let s = d.seite_aus(&knoten, 7);
        assert_eq!(s.media, [0.0, 0.0, 612.0, 792.0]);
    }
}

#[cfg(test)]
mod dbg {
    #[test]
    fn dbg_len() {
        let d = std::fs::read("testdaten/pdfs/dummy.pdf").unwrap();
        let w = super::objekt_bei(&d, &super::Querverweis::default(), 19).unwrap();
        match &w {
            crate::objekt::Wert::Stream(dict, bytes) => {
                println!("STREAM {} Byte, dict = {:?}", bytes.len(), dict);
                println!(
                    "Length = {:?}",
                    dict.iter().rev().find(|(k, _)| k.as_slice() == b"Length")
                );
            }
            other => println!("{:?}", other),
        }
        let q = crate::xref::lesen(&d).expect("xref");
        println!("Eintrag 3 = {:?}", q.eintraege.get(&3));
        println!(
            "laenge_auflösen(3) = {:?}",
            super::laenge_auflösen(&d, &q, 3)
        );
        let w2 = super::objekt_bei(&d, &q, 19).unwrap();
        match &w2 {
            crate::objekt::Wert::Stream(_, bytes) => println!("MIT XREF: {} Byte", bytes.len()),
            other => println!("{:?}", other),
        }
    }
}
