//! CFF (Compact Font Format, Adobe 5176 / Type-2-Charstrings).
//!
//! added 03.10.2026, Rolle SERVER. Warum das in einer PDF-Crate steht
//! und nicht in `narsil-text`: `narsil-text` liest TrueType (`glyf`). Ein
//! `/FontFile3` mit `/Subtype /Type1C` ist **kein** TrueType, und
//! `TrueTypeFont::from_bytes` gibt fuer eine solche Datei `Err` — bis
//! eben bekam die Schrift **keinen** Umriss und wurde mit einem
//! Systemfont gezeichnet.
//!
//! GEMESSEN, welche Dateien betroffen sind (`grep` ueber die elf PDFs,
//! im Rumpf **und** in den Objekt-Stroemen):
//!
//! ```text
//! Datei          /FontFile3  /Subtype/Type1C  /CIDFontType0
//! f1040.pdf               9                9              0
//! fw9.pdf                 6                6              0
//! 1207.7214.pdf          17               17              0
//! 1512.03385.pdf         10                9              2
//! rfc9110.pdf             1                0              2
//! ```
//!
//! Das sind **43 eingebettete CFF-Programme** in fuenf der elf Dateien.
//!
//! WAS DRIN IST:
//!
//!  * Header, die vier INDEXe (Name, Top DICT, String, Global Subr)
//!  * Top DICT: `charset` (15), `Encoding` (16), `CharStrings` (17),
//!    `Private` (18), `FontMatrix` (12 7), `ROS` (12 30),
//!    `FDArray` (12 36), `FDSelect` (12 37), `FDPrivate` (12 38)
//!  * Private DICT: `Subrs` (19), `defaultWidthX` (20),
//!    `nominalWidthX` (21)
//!  * `charset` in den Formaten 0, 1 und 2
//!  * `FDSelect` in den Formaten 0, 1 und 3
//!  * Type-2-Charstrings: alle Zeichenoperatoren einschliesslich
//!    `flex`, `hflex`, `hflex1`, `flex1`, `hintmask`/`cntrmask`,
//!    `callsubr`/`callgsubr` mit Tiefenschranke und `endchar`
//!  * die rechnenden Escape-Operatoren (`add`, `sub`, `mul`, `div`,
//!    `neg`, `abs`, `and`, `or`, `not`, `eq`, `ifelse`, `random`,
//!    `roll`, `drop`, `put`, `get`)
//!
//! WAS NICHT DRIN IST, und warum:
//!
//!  * **`seac`** (Type 1: vier Argumente am `endchar`). Kommt in keinem
//!    der elf PDFs vor; [`Cff::seac_aufrufe`] zaehlt die Faelle, damit
//!    die Zahl nicht geraten ist.
//!  * **`eexec`/`lenIV`** (Type 1 in `/FontFile`). Betrifft nicht
//!    `/FontFile3`. GEMESSEN: `1706.03762.pdf` schreibt 33 mal die
//!    Zeichenkette `/FontFile`, aber jeweils mit `/FontFile2` oder
//!    `/FontFile3` daneben — es gibt **kein** `/FontFile` mit Inhalt.
//!  * **`Type 2`-Charstrings** (Selfint, Subrs mit Typ 2). Kommt in
//!    keiner der elf Dateien vor; die Erkennung `CharstringType = 2`
//!    (Top DICT 12 6) wird gemeldet statt geraten.

use std::collections::HashMap;

/// Die Grenzen, alle als Konstanten.
///
/// added 03.10.2026. GEMESSEN an der groessten CFF der elf PDFs:
/// `1207.7214.pdf`, 22.518 Byte fuer eine Schrift mit 260 Glyphen. Die
/// Grenzen liegen weit darueber — eine fremde Datei soll nicht an ihnen
/// scheitern — und weit unter dem, was Speicher kosten wuerde.
pub const MAX_INDEX: usize = 65_535;
/// Glyphen je CFF (Spec: 65.535).
pub const MAX_GLYPHEN: usize = 65_535;
/// Verschachtelung von `callsubr`/`callgsubr`.
///
/// added 05.10.2026. **10 war zu niedrig**, und das kam an einem
/// Buchstaben heraus. GEMESSEN an `1706.03762.pdf`: der Renderer meldete
/// 27 × `CFF: ZuTief` bei 32 174 gezeichneten Zeichen, also 0,08 % der
/// Zeichen eines ganzen Dokuments.
///
/// DER GRUND: `NimbusRoman-Regular.otf` enthaelt eine **selbst
/// aufrufende** Unterroutine — Subroutine 63 ruft ueber den Index -38
/// (mit Bias 101) wieder sich selbst auf. Das ist ein uebliches
/// TeX-Muster fuer einen Stamm, der sich mehrfach wiederholt, und es
/// braucht genau so viele Ebenen, wie der Buchstabe Staemme hat.
///
/// GEMESSENE NOTWENDIGKEIT: mit Grenze 200 kommt in **keiner** der elf
/// Abnahme-Dateien ein `ZuTief` vor, und die gemeldete Hochwassermarke
/// (`tiefen_hochwasser`) liegt bei **30**. Die Grenze steht jetzt bei
/// **64** — mehr als das Doppelte des Gemessenen, aber weiterhin eine
/// harte Marke.
///
/// DER DOS-SCHUTZ IST NICHT DIESE ZAHL, sondern `MAX_BEFEHLE` (20 000 je
/// Glyphe): `z.takte` liegt im **geteilten** `Zustand`, zaehlt also die
/// Befehle ueber alle Unterroutinen hinweg. Eine hoehere Tiefengrenze
/// koennte deshalb kein Programm laenger machen — die Selbstrekursion
/// von Subroutine 63 endet nach 20 000 Befehlen, gleichgueltig wie tief
/// sie laeuft. Die Grenze ist damit ein **Schnellabbruch**, keine
/// Schranke fuer Aufwand.
pub const MAX_AUFRUFE: u32 = 64;
/// Befehle je Glyphe (die Spec nennt keine; das ist eine Bombenschranke).
pub const MAX_BEFEHLE: u32 = 20_000;
/// Punkte je Kontur **nach** der Rasterung.
pub const MAX_PUNKTE: usize = 4_000;
/// Konturen je Glyphe.
pub const MAX_KONTUREN: usize = 256;
/// Der Zahlenstapel (Spec: 48; `put` erlaubt mehr).
pub const MAX_OPERANDEN: usize = 513;
/// Groesse eines Charstrings.
pub const MAX_CHARSTRING: usize = 1 << 20;
/// Groesse einer CFF-Datei.
pub const MAX_DATEI: usize = 16 * 1024 * 1024;

/// Wie viele Geraden ein Bézier wird.
///
/// added 03.10.2026. FEST und nicht adaptiv, mit einer Begruendung:
/// die Umrisse gehen als **Punktpolygon** in den Rasterer, und
/// derselbe Weg gilt fuer die TrueType-Umrisse (deren quadratische
/// Punkte der Rasterer ebenfalls als Geraden verbindet). 16 Stuecke
/// je Kurve geben bei 1000 Einheiten Schriftgroesse eine mittlere
/// Kantenlaenge von rund 4 Einheiten = 0,004 em; bei 8 Punkt Schrift
/// ist das kleiner als ein Pixel.
pub const KURVEN_STUECKE: usize = 16;

/// Die Fehler dieser Stufe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CffFehler {
    /// Die Datei ist kuerzer, als sie behauptet.
    ZuKurz { braucht: usize, hat: usize },
    /// Eine Tabelle hat mehr Eintraege als erlaubt.
    ZuViel { was: &'static str, n: usize },
    /// Ein Byte steht da, wo keines stehen darf.
    Unbekannt(u8),
    /// Der Zahlenstapel ist ueberlaufen.
    StapelUeberlauf,
    /// Ein Befehl braucht eine Zahl und hat keine.
    KeineZahl(&'static str),
    /// `callsubr`/`callgsubr` hat zu tief verschachtelt.
    ZuTief,
    /// Die Glyphennummer gibt es nicht.
    KeineGlyphe(u16),
    /// Eine Pflichttabelle fehlt.
    TabelleFehlt(&'static str),
}

impl std::fmt::Display for CffFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CffFehler::ZuKurz { braucht, hat } => {
                write!(f, "CFF: {braucht} Byte gebraucht, {hat} da")
            }
            CffFehler::ZuViel { was, n } => write!(f, "CFF: {was} mit {n} Eintraegen"),
            CffFehler::Unbekannt(op) => write!(f, "CFF: Byte {op} an dieser Stelle"),
            CffFehler::StapelUeberlauf => write!(f, "CFF: Zahlenstapel ueberlaufen"),
            CffFehler::KeineZahl(op) => write!(f, "CFF: {op} ohne Zahl"),
            CffFehler::ZuTief => write!(f, "CFF: Unterprogramme zu tief verschachtelt"),
            CffFehler::KeineGlyphe(g) => write!(f, "CFF: keine Glyphe {g}"),
            CffFehler::TabelleFehlt(w) => write!(f, "CFF: {w} fehlt"),
        }
    }
}

/// Ein Bereich im Dateipuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bereich {
    start: usize,
    ende: usize,
}

/// Ein Private DICT: Unterprogramme und Breiten-Vorgaben.
#[derive(Debug, Default, Clone)]
struct Privat {
    subrs: Vec<Bereich>,
    /// `/defaultWidthX`; die Vorgabe ist 0.
    standard_breite: f32,
    /// `/nominalWidthX`; die Vorgabe ist 0.
    nominal_breite: f32,
    /// added 04.10.2026: stand in diesem Wörterbuch ueberhaupt ein
    /// bekannter Schluessel? (siehe [`privat_lesen_geprueft`])
    erkannt: bool,
}

/// Eine gelesene CFF-Datei.
#[derive(Debug, Clone)]
pub struct Cff {
    daten: Vec<u8>,
    /// Global Subr INDEX.
    global_subrs: Vec<Bereich>,
    /// String INDEX — fuer Glyphennamen, die nicht Standard sind.
    strings: Vec<Bereich>,
    /// Die Private DICTs, eine je FD (bei Name-Fonts genau eine).
    privat: Vec<Privat>,
    /// CharStrings INDEX: GID -> Bereich.
    glyphen: Vec<Bereich>,
    /// `charset`: GID -> CID (CID-Font) oder SID (Name-Font).
    charset: Vec<u32>,
    /// `true`, wenn der Font CID-Schluessel hat.
    cid: bool,
    /// `FDSelect`: GID -> FD.
    fd_select: Vec<u16>,
    /// Die **eingebaute** Kodierung des Fonts: Code -> SID.
    eingebaut: HashMap<u32, u32>,
    /// Zeichencode -> GID (Name-Font) oder CID -> GID (CID-Font).
    code_zu_glyphe: HashMap<u32, u16>,
    /// `units_per_em` aus `/FontMatrix`.
    upm: f32,
    /// Wie oft `endchar` vier Argumente hatte.
    ///
    /// added 03.10.2026. `Cell`, nicht `usize`: sonst braeuchte
    /// [`Self::umriss`] ein `&mut self`, und der Zeichner haelt die
    /// Schriften in einer `HashMap` **hinter** einer `Rc` — ein `&mut`
    /// waere dort nicht zu bekommen. Der Zaehler aendert nichts an der
    /// Ausgabe.
    seac: std::cell::Cell<usize>,
    /// added 04.10.2026: die groesste Verschachtelungstiefe, die eine
    /// Glyphe bisher erreicht hat. GEMESSEN an den URW-Dateien (siehe
    /// `tiefen_hochwasser`) — als `Cell`, damit `umriss` (`&self`) sie
    /// melden kann, ohne `&mut` zu brauchen.
    tiefen_hochwasser: std::cell::Cell<u32>,
    /// added 05.10.2026: die **Vorschubbreiten** aus `hmtx` in
    /// Font-Einheiten, GID -> Breite.
    ///
    /// WARUM DIE HIER LIEGT: eine CFF aus einem PDF (`/FontFile3` mit
    /// `/Subtype /Type1C`) hat **keine** `hmtx` — dort traegt `/Widths`
    /// des Dokuments die Breite, und die ist richtig. Bei einer
    /// **Ersatzschrift** aus einem OTF ist es umgekehrt: da ist
    /// `/Widths` die Breite des Originals (bei `1706.03762.pdf`
    /// durchgehend 524,9 fuer die Schreibmaschinenschrift), und
    /// gezeichnet wird aus einer **anderen** Schrift, deren eigene
    /// Breiten man zum Strecken braucht.
    ///
    /// GEMESSEN, was ohne diese Tabelle passiert: `Ersatzschrift::vorschub`
    /// gab fuer eine CFF `None` zurueck, und `None` bedeutet im Zeichner
    /// „nicht strecken". Die Glyphen behielten also ihre **natuerliche**
    /// Breite, und die Buchstaben der Zeile `illia.polosukhin@gmail.com`
    /// beruehrten sich an 13 Stellen, wo der Referenz-Renderer 16 getrennte
    /// Laeufe hat (Messung in `team/BERICHT_SERVER_PDF_GLYPHEN.txt`).
    vorschub: Vec<u16>,
}

impl Cff {
    /// Liest eine CFF-Datei (`/FontFile3` mit `/Subtype /Type1C`).
    pub fn lesen(daten: &[u8]) -> Result<Cff, CffFehler> {
        if daten.len() < 4 || daten.len() > MAX_DATEI {
            return Err(CffFehler::ZuKurz {
                braucht: 4,
                hat: daten.len(),
            });
        }
        let hdr = daten[2] as usize;
        if !(4..=daten.len()).contains(&hdr) {
            return Err(CffFehler::ZuKurz {
                braucht: hdr,
                hat: daten.len(),
            });
        }
        let mut p = hdr;
        let _name = index(daten, &mut p)?;
        let top = index(daten, &mut p)?;
        let strings = index(daten, &mut p)?;
        let global = index(daten, &mut p)?;
        let top = verbinde(daten, &top)?;
        let top_dict = dict_von(&top)?;

        // added 03.10.2026. DER TYP WIRD **GEPRUEFT**, nicht
        // angenommen. Top DICT 12 6 ist `CharstringType`; die Zahl 2
        // bedeutet Type-2-Charstrings mit `Subrs`-Typ im Index, und
        // die kann dieser Interpreter nicht lesen. GEMESSEN: in den
        // elf PDFs ist die Zahl **nie** 2.
        if let Some(Wert::Zahl(2.0)) = top_dict.get(&0x0c06) {
            return Err(CffFehler::TabelleFehlt("CharstringType 2"));
        }

        let cid = top_dict.contains_key(&0x0c1e); // 12 30 = ROS
        let upm = match top_dict.get(&0x0c07) {
            Some(Wert::Zahlen(v)) if v.len() >= 4 && v[0] != 0.0 => {
                let m = 1.0 / v[0] as f32;
                if m.is_finite() && m > 1.0 {
                    m.abs()
                } else {
                    1000.0
                }
            }
            _ => 1000.0,
        };

        let chars_off = ziffer(&top_dict, &17).map(|v| v as usize);
        let chars = match chars_off {
            Some(o) if o < daten.len() => {
                let mut q = o;
                index(daten, &mut q)?
            }
            _ => return Err(CffFehler::TabelleFehlt("CharStrings")),
        };
        if chars.is_empty() || chars.len() > MAX_GLYPHEN {
            return Err(CffFehler::TabelleFehlt("CharStrings"));
        }
        let glyphen: Vec<Bereich> = chars
            .iter()
            .map(|r| Bereich {
                start: r.start,
                ende: r.ende,
            })
            .collect();

        let mut cff = Cff {
            daten: daten.to_vec(),
            global_subrs: global,
            strings,
            privat: Vec::new(),
            glyphen,
            charset: Vec::new(),
            cid,
            fd_select: Vec::new(),
            eingebaut: HashMap::new(),
            code_zu_glyphe: HashMap::new(),
            upm,
            seac: std::cell::Cell::new(0),
            tiefen_hochwasser: std::cell::Cell::new(0),
            vorschub: Vec::new(),
        };
        // Private DICT(s). Ohne FDArray genau **eine** aus Top DICT 18.
        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
            let mut v: Vec<String> = top_dict
                .iter()
                .map(|(op, w)| match w {
                    Wert::Zahl(z) => format!("{op:#x}={z}"),
                    Wert::Zahlen(z) => {
                        format!(
                            "{op:#x}=[{}]",
                            z.iter()
                                .map(|q| format!("{q}"))
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    }
                })
                .collect();
            v.sort();
            let roh: Vec<String> = top.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!(
                "TOP len={} {} roh={}",
                top_dict.len(),
                v.join(" "),
                roh.join("")
            );
        }
        // added 04.10.2026: der Offset wird **geprueft**, nicht geglaubt
        // (siehe `privat_suchen`). Die elf Abnahme-Dateien treffen mit
        // `v[1]` direkt; die URW-Dateien nicht.
        let eigene = match top_dict.get(&18) {
            Some(Wert::Zahlen(v)) if v.len() >= 2 => cff.privat_suchen(v[0] as usize, v[1]),
            Some(Wert::Zahl(z)) if top_dict.len() > 1 => {
                // added 04.10.2026: der Normalfall (alle elf PDFs) hat
                // **zwei** Zahlen. Ein DICT mit nur einer ist kaputt; das
                // wird ignoriert statt geraten.
                let _ = z;
                Privat::default()
            }
            _ => Privat::default(),
        };
        let fdarray = ziffer(&top_dict, &0x0c24); // 12 36 = FDArray
        match fdarray.map(|v| v as usize) {
            Some(off) if off < cff.daten.len() => {
                let mut q = off;
                let fds = index(&cff.daten, &mut q)?;
                for fd in &fds {
                    let fd_bytes = verbinde(daten, std::slice::from_ref(fd))?;
                    let d = dict_von(&fd_bytes)?;
                    let pv = match d.get(&0x0c26) {
                        // 12 38 = FDPrivate: Groesse, dann Offset
                        Some(Wert::Zahlen(v)) if v.len() >= 2 => {
                            privat_lesen(&cff.daten, v[1] as usize, v[0] as usize)?
                        }
                        _ => Privat::default(),
                    };
                    cff.privat.push(pv);
                }
                if cff.privat.is_empty() {
                    cff.privat.push(eigene);
                }
            }
            _ => cff.privat.push(eigene),
        }

        // charset: Offset 15. 0 und 1 meinen „GID n hat SID n".
        let mut charset = vec![0u32; cff.glyphen.len()];
        match ziffer(&top_dict, &15).map(|v| v as usize) {
            Some(0) | Some(1) | None => {
                for (i, s) in charset.iter_mut().enumerate() {
                    *s = i as u32;
                }
            }
            Some(off) => cff.charset_laden(off, &mut charset)?,
        }
        cff.charset = charset;

        if cid {
            match ziffer(&top_dict, &0x0c25).map(|v| v as usize) {
                Some(off) if off < cff.daten.len() => cff.fd_select_laden(off)?,
                _ => cff.fd_select = vec![0; cff.glyphen.len()],
            }
        }
        if let Some(off) = ziffer(&top_dict, &16).map(|v| v as usize)
            && off < cff.daten.len()
        {
            cff.encoding_laden(off);
        }
        Ok(cff)
    }

    /// `units_per_em`, aus `/FontMatrix` gerechnet (Vorgabe 1000).
    pub fn units_per_em(&self) -> f32 {
        self.upm
    }

    /// added 04.10.2026. DIE SUCHE NACH DEM PRIVATE DICT, und sie ist
    /// **eine Messung, keine Glaubensfrage**.
    ///
    /// DER BEFUND: die URW-Dateien, die `fc-match` fuer die
    /// Standard-14-Namen liefert, schreiben den Offset des Private DICT
    /// als **16.16-Festen Punkt** — nicht als ganze Zahl:
    ///
    /// ```text
    /// NimbusRoman-Regular.otf   1d 00 00 fb 2c  18
    ///                           0x0000FB2C / 65 536 = 0,981140…
    ///                           mal 65 536 = 64 300
    ///                           fontTools sagt:       64 300   <- gleich
    /// NimbusRoman-Bold.otf     mal 65 536 = 67 115
    ///                           fontTools sagt:       67 114   <- 1 daneben
    /// ```
    ///
    /// DER LESER IST ALSO **RICHTIG**, und `private_suchen` bestaetigt
    /// es an beiden Dateien mit **Versatz 0**. Trotzdem wird geprueft
    /// statt gerechnet, und der Grund ist nicht diese Zahl, sondern ein
    /// anderer: die Fenstersuche ist **billig** (2 x 35 Stellen, jede
    /// ein Wörterbuch aus `groesse` Byte) und sie schuetzt gegen
    /// Dateien, deren Private DICT **garnicht** dasteht — dort waere
    /// der Anker daneben, und ohne Fenster lieber eine leere als eine
    /// falsche Subroutinentabelle.
    ///
    ///  1. beide Lesarten des Operanden als Anker (als Ganzzahl und mal
    ///     65 536),
    ///  2. um jeden Anker in wachsender Entfernung (0, 1, 2, 4, 8)
    ///     probieren,
    ///  3. **der erste Treffer mit lesbaren Subroutinen gewinnt**;
    ///     sonst der erste, dessen Wörterbuch ueberhaupt bekannte
    ///     Schluessel hat.
    ///
    /// DIE SUCHE BLEIBT ENDLICH: hoechstens 2 x (1+3+5+9+17) = 70
    /// Stellen, jede kostet ein Wörterbuch aus `groesse` Byte — das
    /// ist billiger als eine einzige Glyphe.
    ///
    /// WAS SIE **NICHT** KANN: eine Datei finden, deren Private DICT
    /// keine Subroutinen hat (z.B. `fw9.pdf`). Dafuer ist `erkannt` die
    /// Antwort, und der Aufrufer nimmt den ersten gelesenen Wert.
    fn privat_suchen(&mut self, groesse: usize, wert: f64) -> Privat {
        if groesse == 0 || !wert.is_finite() || wert < 0.0 {
            return Privat::default();
        }
        let daten = &self.daten;
        let mut anker: Vec<usize> = Vec::new();
        for w in [wert, wert * 65_536.0] {
            let m = w.round();
            if m >= 0.0 && m <= daten.len() as f64 {
                anker.push(m as usize);
            }
        }
        let mut erstes_erkanntes: Option<Privat> = None;
        for fenster in [0i64, 1, 2, 4, 8] {
            for &a in &anker {
                let mut versuche: Vec<i64> = vec![0];
                for d in 1..=fenster {
                    versuche.push(d);
                    versuche.push(-d);
                }
                for d in versuche {
                    let c = a as i64 + d;
                    if c < 0 || c as usize >= daten.len() {
                        continue;
                    }
                    let Ok(p) = privat_lesen(daten, c as usize, groesse) else {
                        continue;
                    };
                    if !p.subrs.is_empty() {
                        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
                            eprintln!(
                                "PRIVAT Anker {a} Versatz {d} -> {c}: {} Subrs",
                                p.subrs.len()
                            );
                        }
                        return p;
                    }
                    if p.erkannt && erstes_erkanntes.is_none() {
                        erstes_erkanntes = Some(p);
                    }
                }
            }
        }
        erstes_erkanntes.unwrap_or_default()
    }

    /// added 04.10.2026. Wie viele Eintraege hat die `FDSelect`-Tabelle? (added 04.10.2026)
    ///
    /// GEMESSEN an `NimbusRoman-Regular.otf`: 855 Glyphen, und die
    /// Charstrings rufen **Subroutinen** auf, die es nur in einem der
    /// `FDArray`-Eintraege gibt. Fehlt die `FDSelect`-Tabelle, nimmt
    /// `fd_fuer` FD 0 — und dann findet der Aufruf keine Subroutine.
    pub fn fd_select_anzahl(&self) -> usize {
        self.fd_select.len()
    }

    /// Wie viele Subroutinen hat jeder `FD`? (added 04.10.2026)
    pub fn subrs_je_fd(&self) -> Vec<usize> {
        self.privat.iter().map(|p| p.subrs.len()).collect()
    }

    /// Wie viele Glyphen hat der Font?
    pub fn glyphen_anzahl(&self) -> usize {
        self.glyphen.len()
    }

    /// Wie tief war die Verschachtelung bisher? (added 04.10.2026)
    ///
    /// GEMESSEN: `NimbusMonoPS-Regular.otf`, Glyphe `A` (GID 34) geht
    /// tiefer als `MAX_AUFRUFE` (10) — ohne diese Zahl waere jede
    /// Grenze geraten.
    pub fn tiefen_hochwasser(&self) -> u32 {
        self.tiefen_hochwasser.get()
    }

    /// Wie viele `seac`-Aufrufe es gab.
    pub fn seac_aufrufe(&self) -> usize {
        self.seac.get()
    }

    /// Hat der Font CID-Schluessel?
    pub fn ist_cid(&self) -> bool {
        self.cid
    }

    /// Legt die Tabelle **Zeichencode -> Glyphe** an.
    ///
    /// added 03.10.2026. `namen` kommt aus der PDF-Kodierung
    /// (`/Differences` und die Basiskodierung). Fuer CID-Fonts wird
    /// der Code als CID genommen und ueber das `charset` gesucht.
    pub fn code_aufnehmen(&mut self, namen: &[(u32, String)]) {
        if self.cid {
            for (code, _) in namen {
                if let Some(gid) = self.gid_von_cid(*code) {
                    self.code_zu_glyphe.insert(*code, gid);
                }
            }
            return;
        }
        // added 03.10.2026. DIE EINGEBAUTE KODIERUNG ZUERST, die
        // PDF-Kodierung danach. Die Reihenfolge ist die der
        // Spezifikation: die eingebaute Kodierung gilt, und `/Differences`
        // ueberschreibt sie. Ohne diese Reihenfolge bekommt
        // `1207.7214.pdf` bei 32 („space") die Glyphe des Zeichens
        // 32 der **PDF**-Kodierung statt der des Fonts.
        let mut ergebnis: HashMap<u32, u16> = HashMap::new();
        let mut codes: Vec<u32> = self.eingebaut.keys().copied().collect();
        codes.sort_unstable();
        for code in codes {
            if let Some(sid) = self.eingebaut.get(&code).copied()
                && let Some(gid) = self.gid_von_sid(sid)
            {
                ergebnis.insert(code, gid);
            }
        }
        for (code, name) in namen {
            if let Some(gid) = self
                .gid_von_name(name)
                .or_else(|| self.gid_aus_string(name))
            {
                ergebnis.insert(*code, gid);
            }
        }
        self.code_zu_glyphe = ergebnis;
    }

    /// Die Glyphe eines Zeichencodes.
    pub fn glyphe_von_code(&self, code: u32) -> Option<u16> {
        self.code_zu_glyphe.get(&code).copied()
    }

    /// Die Glyphe einer CID (nur bei CID-Fonts).
    pub fn gid_von_cid(&self, cid: u32) -> Option<u16> {
        if !self.cid {
            return None;
        }
        self.charset
            .iter()
            .position(|c| *c == cid)
            .and_then(|g| u16::try_from(g).ok())
    }

    fn gid_von_sid(&self, sid: u32) -> Option<u16> {
        self.charset
            .iter()
            .position(|c| *c == sid)
            .and_then(|g| u16::try_from(g).ok())
    }

    pub fn gid_von_name(&self, name: &str) -> Option<u16> {
        let sid = standard_sid(name)?;
        self.gid_von_sid(sid as u32)
    }

    /// Der Name steht im **String INDEX** der Datei, nicht im
    /// Standard-Teil: dann ist die SID `391 + Index`.
    fn gid_aus_string(&self, name: &str) -> Option<u16> {
        for (i, b) in self.strings.iter().enumerate() {
            if self.daten.get(b.start..b.ende) == Some(name.as_bytes()) {
                let sid = 391 + i as u32;
                return self.gid_von_sid(sid);
            }
        }
        None
    }

    /// Die Umrisse einer Glyphe, in **Schrifteinheiten** von
    /// [`Self::units_per_em`].
    ///
    /// added 03.10.2026. Alle Punkte sind `on_curve`, weil
    /// Type-2-Charstrings nur Geraden und Kuben liefern und die Kuben
    /// hier gerastert werden. Das ist dasselbe Bild, das
    /// `raster::glyphe_zeichnen` vom TrueType-Umriss bekommt.
    pub fn umriss(&self, gid: u16) -> Result<Vec<Vec<(f32, f32)>>, CffFehler> {
        let bereich = *self
            .glyphen
            .get(gid as usize)
            .ok_or(CffFehler::KeineGlyphe(gid))?;
        if bereich.ende > self.daten.len() || bereich.ende <= bereich.start {
            return Err(CffFehler::KeineGlyphe(gid));
        }
        if bereich.ende - bereich.start > MAX_CHARSTRING {
            return Err(CffFehler::ZuViel {
                was: "Charstring",
                n: bereich.ende - bereich.start,
            });
        }
        let fd = self.fd_fuer(gid);
        let stroh = self.daten[bereich.start..bereich.ende].to_vec();
        let mut z = Zustand::neu();
        let mut stapel: Vec<f32> = Vec::with_capacity(48);
        // added 05.10.2026 (SCHLUSSPHASE): Das Hochwasser wird **vor**
        // dem `?` notiert. Vorher stand es danach, und damit war es bei
        // genau den interessanten Faellen — einer Glyphe, die an der
        // Aufrufgrenze scheitert — **null**. Das ist die falsche
        // Reihenfolge: Man will wissen, wie tief es war, *weil* es
        // gescheitert ist.
        let lauf = self.lauf(&stroh, &mut z, fd, &mut stapel);
        if z.max_tief > self.tiefen_hochwasser.get() {
            self.tiefen_hochwasser.set(z.max_tief);
        }
        lauf?;
        let mut out = std::mem::take(&mut z.konturen);
        if out.len() > MAX_KONTUREN {
            return Err(CffFehler::ZuViel {
                was: "Konturen",
                n: out.len(),
            });
        }
        for k in &mut out {
            if k.len() > MAX_PUNKTE {
                k.truncate(MAX_PUNKTE);
            }
        }
        out.retain(|k| k.len() >= 2);
        Ok(out)
    }

    /// Der FD einer Glyphe.
    fn fd_fuer(&self, gid: u16) -> usize {
        self.fd_select
            .get(gid as usize)
            .map(|f| *f as usize)
            .unwrap_or(0)
            .min(self.privat.len().saturating_sub(1))
    }

    /// Fuehrt einen Charstring oder ein Unterprogramm aus.
    fn lauf(
        &self,
        daten: &[u8],
        z: &mut Zustand,
        fd: usize,
        stapel: &mut Vec<f32>,
    ) -> Result<(), CffFehler> {
        z.tief += 1;
        // added 04.10.2026: die groesste gemessene Tiefe mitzaehlen, um
        // die Grenze belegt zu setzen statt sie zu raten.
        if z.tief > z.max_tief {
            z.max_tief = z.tief;
        }
        if z.tief > MAX_AUFRUFE {
            return Err(CffFehler::ZuTief);
        }
        // added 04.10.2026. DER STAPEL IST **GEMEINSAM**, und das ist
        // die Regel, die vorher hier verletzt war: in Type 2 arbeiten
        // Charstring und Unterroutine auf **einem** Operandenstapel
        // (Adobe TN #5176, „Subroutines": „The subroutines may not
        // balance the operand stack, and they may access values pushed
        // by the calling charstring").
        //
        // GEMESSEN, warum es auffaellt und nicht vorher: keine der elf
        // Abnahme-Dateien hat Subroutinen, also lief `callsubr` nie.
        // An `NimbusRoman-Regular.otf`, Glyphe `e` (72):
        //
        //     64 1d 01 f8 2c 22 0a 0e
        //         -39 | gsubr | hstem | 920 | -105 | subr | endchar
        //
        // Das `hstem` **direkt nach** dem Unterprogrammaufruf beweist,
        // dass das Unterprogramm Werte auf den Stapel gelegt hat, von
        // denen `hstem` seine Staemme liest. Vorher bekam `lauf` einen
        // **neuen** Stapel (`Vec::with_capacity(48)`) und die beiden
        // `stapel.clear()` nach `callsubr`/`callgsubr` warfen dann auch
        // alles weg, was zurueckkam. Folge: `hstem` sah einen leeren
        // Stapel, `n_stems` blieb 0, und die Glyphe brach mit
        // `KeineZahl("hstem")` ab — 424 von 1 100 Versuchen auf Seite 1.
        let mut p = 0usize;
        while p < daten.len() {
            z.takte += 1;
            if z.takte > MAX_BEFEHLE {
                return Err(CffFehler::ZuViel {
                    was: "Befehle",
                    n: z.takte as usize,
                });
            }
            let b0 = daten[p];
            p += 1;
            // ---- Zahlen ----
            match b0 {
                32..=254 => {
                    stapel.push(zahl(daten, &mut p, b0)?);
                    if stapel.len() > MAX_OPERANDEN {
                        return Err(CffFehler::StapelUeberlauf);
                    }
                    continue;
                }
                28 => {
                    stapel.push(zahl28(daten, &mut p)?);
                    if stapel.len() > MAX_OPERANDEN {
                        return Err(CffFehler::StapelUeberlauf);
                    }
                    continue;
                }
                255 => {
                    stapel.push(fest16(daten, &mut p)?);
                    continue;
                }
                _ => {}
            }
            // ---- Befehle ----
            //
            // added 05.10.2026. DER TRACE, und er ist der Grund, warum
            // der Fehler an `b` ueberhaupt auffiel: `NARSIL_PDF_DEBUG`
            // druckt vor jedem Befehl Nummer, Name und den Stapel. Ohne
            // das sieht man nur `CFF: KeineZahl("vmoveto")` und muss
            // raten, **welche** der 20 Anweisungen davor den Stapel
            // geleert hat — GEMESSEN an `NimbusRoman-Regular.otf`,
            // Glyphe `b` (gid 67), wo der Stapel bei `vmoveto` leer war.
            if std::env::var("NARSIL_PDF_TRACE").is_ok() {
                let arg: Vec<String> = stapel.iter().take(19).map(|v| format!("{v}")).collect();
                let tiefe = z.tief;
                let mehr = if stapel.len() > 19 { " …" } else { "" };
                eprintln!(
                    "CFF {tiefe} p={p} op={b0} stapel({}) [{}]{mehr}",
                    stapel.len(),
                    arg.join(" "),
                );
            }
            match b0 {
                1 | 3 | 18 | 23 => {
                    // added 05.10.2026. `23` ist `vstemhm` und **gehoert
                    // hierher**: es zaehlt die Staemme und leert den Stapel
                    // wie `hstem` und `hstemhm`.
                    //
                    // DER FASTEHLER, und deshalb steht er hier: der Name
                    // „hm" laesst eine Maske erwarten, und deshalb habe ich
                    // zuerst `ceil(n_stems / 8)` Byte uebersprungen.
                    //
                    // GEMESSEN an der Referenz (`fontTools`,
                    // `psCharStrings.py`):
                    //     def op_vstemhm(self, index):
                    //         self.countHints()
                    //     def op_hintmask(self, index):
                    //         ...  hintMaskBytes = (self.hintCount + 7) // 8
                    // Nur `hintmask` (19) und `cntrmask` (20) tragen eine
                    // Maske; `hstemhm` (18) und `vstemhm` (23) nicht. Mit
                    // dem Uebersprung haette der Leser die Maskenbyte als
                    // Zahlen gelesen — und den ersten davon als Operator.
                    z.n_stems += stamm_zaehlen(stapel);
                    stapel.clear();
                    if z.n_stems == 0 && !z.breite_genommen {
                        return Err(CffFehler::KeineZahl("hstem"));
                    }
                }
                19 | 20 => {
                    z.n_stems += stamm_zaehlen(stapel);
                    stapel.clear();
                    let bytes = z.n_stems.div_ceil(8);
                    p += bytes;
                    if p > daten.len() {
                        return Err(CffFehler::ZuKurz {
                            braucht: p,
                            hat: daten.len(),
                        });
                    }
                }
                4 => {
                    // added 05.10.2026. **EINE** Zahl, nicht zwei — und
                    // das war der Fehler, der `b` in `NimbusRoman`
                    // verschwinden liess.
                    //
                    // `vmoveto` nimmt **eine** Zahl (dy). Die
                    // Randbedingung steht bei `rmoveto` (zwei) und bei
                    // `hmoveto` (eine) richtig; hier stand eine Abschrift
                    // von `rmoveto`, also `2`.
                    //
                    // GEMESSEN (Trace `NARSIL_PDF_TRACE`): bei
                    // `NimbusRoman-Regular.otf` Glyphe `b` (gid 67) kam
                    //     op=30 stapel(17) [ … ]   <- vhcurveto
                    //     op=4  stapel(1)  [-53]    <- vmoveto
                    //     ERSATZ 'b' gid=67 umriss=Err("CFF: KeineZahl(\"vmoveto\")")
                    // Der Stapel war **nicht** leer — ein Argument ist da
                    // — und trotzdem kam der Fehler, weil `1 != 2`.
                    // GEMESSENE WIRKUNG: `b` wurde **gar nicht** gezeichnet
                    // (im Bild fehlte jeder Buchstabe `b`, das Wort
                    // „attribution" stand als „attri ution" da).
                    breite_vielleicht(z, stapel, 1);
                    if stapel.len() != 1 {
                        return Err(CffFehler::KeineZahl("vmoveto"));
                    }
                    z.y += stapel[0];
                    schliesse(z);
                    z.offen = true;
                    punkt(z);
                    stapel.clear();
                }
                21 => {
                    breite_vielleicht(z, stapel, 2);
                    if stapel.len() != 2 {
                        return Err(CffFehler::KeineZahl("rmoveto"));
                    }
                    z.x += stapel[0];
                    z.y += stapel[1];
                    schliesse(z);
                    z.offen = true;
                    punkt(z);
                    stapel.clear();
                }
                22 => {
                    breite_vielleicht(z, stapel, 1);
                    if stapel.len() != 1 {
                        return Err(CffFehler::KeineZahl("hmoveto"));
                    }
                    z.x += stapel[0];
                    schliesse(z);
                    z.offen = true;
                    punkt(z);
                    stapel.clear();
                }
                5 => {
                    if stapel.len() % 2 != 0 {
                        return Err(CffFehler::KeineZahl("rlineto"));
                    }
                    for paar in stapel.chunks(2) {
                        z.x += paar[0];
                        z.y += paar[1];
                        punkt(z);
                    }
                    stapel.clear();
                }
                6 | 7 => {
                    let mut waagerecht = b0 == 6;
                    for w in stapel.drain(..) {
                        if waagerecht {
                            z.x += w;
                        } else {
                            z.y += w;
                        }
                        waagerecht = !waagerecht;
                        punkt(z);
                    }
                }
                8 => {
                    if stapel.len() % 6 != 0 {
                        return Err(CffFehler::KeineZahl("rrcurveto"));
                    }
                    for k in stapel.chunks(6) {
                        let (x1, y1) = (z.x + k[0], z.y + k[1]);
                        let (x2, y2) = (x1 + k[2], y1 + k[3]);
                        let (x3, y3) = (x2 + k[4], y2 + k[5]);
                        kubus(z, x1, y1, x2, y2, x3, y3);
                        z.x = x3;
                        z.y = y3;
                    }
                    stapel.clear();
                }
                24 => {
                    // rcurveline: je **sechs** Zahlen ein Kubus, am
                    // Ende **zwei** fuer eine Gerade.
                    if stapel.len() < 8 {
                        return Err(CffFehler::KeineZahl("rcurveline"));
                    }
                    let n = stapel.len();
                    let kurven = (n - 2) / 6;
                    let mut i = 0usize;
                    for _ in 0..kurven {
                        let (x1, y1) = (z.x + stapel[i], z.y + stapel[i + 1]);
                        let (x2, y2) = (x1 + stapel[i + 2], y1 + stapel[i + 3]);
                        let (x3, y3) = (x2 + stapel[i + 4], y2 + stapel[i + 5]);
                        kubus(z, x1, y1, x2, y2, x3, y3);
                        z.x = x3;
                        z.y = y3;
                        i += 6;
                    }
                    while i + 1 < n {
                        z.x += stapel[i];
                        z.y += stapel[i + 1];
                        i += 2;
                        punkt(z);
                    }
                    stapel.clear();
                }
                25 => {
                    // rlinecurve: je zwei Zahlen eine Gerade, am Ende
                    // **sechs** fuer einen Kubus.
                    if stapel.len() < 8 {
                        return Err(CffFehler::KeineZahl("rlinecurve"));
                    }
                    let n = stapel.len();
                    let geraden = (n - 6) / 2;
                    let mut i = 0usize;
                    for _ in 0..geraden {
                        z.x += stapel[i];
                        z.y += stapel[i + 1];
                        i += 2;
                        punkt(z);
                    }
                    let (x1, y1) = (z.x + stapel[i], z.y + stapel[i + 1]);
                    let (x2, y2) = (x1 + stapel[i + 2], y1 + stapel[i + 3]);
                    let (x3, y3) = (x2 + stapel[i + 4], y2 + stapel[i + 5]);
                    kubus(z, x1, y1, x2, y2, x3, y3);
                    z.x = x3;
                    z.y = y3;
                    stapel.clear();
                }
                26 => {
                    // vvcurveto: optional dx1, dann **vier** Zahlen je
                    // Kubus; der erste Kontrollpunkt ist (dx1, dy1), der
                    // letzte (dx4, 0).
                    // added 03.10.2026. DIE PRUEFUNG WAR ZU
                    // STRENG: `vvcurveto` nimmt **vier** Zahlen je
                    // Kurve, optional mit `dx1` **vorangestellt** —
                    // der Rest ist also `4k` **oder** `4k + 1`. Vorher
                    // stand dort `len % 4 != 0`, und der Fall mit
                    // `dx1` wurde als Fehler abgewiesen:
                    //
                    //     CFF: hhcurveto ohne Zahl
                    //
                    // GEMESSEN an `fw9.pdf`, wo der vierte Bogen eines
                    // `o` genau diesen Fall hat.
                    if stapel.len() < 4 || !(stapel.len() % 4 == 0 || stapel.len() % 4 == 1) {
                        return Err(CffFehler::KeineZahl("vvcurveto"));
                    }
                    let mit_dx1 = stapel.len() % 4 == 1;
                    let start = usize::from(mit_dx1);
                    let dx1 = if mit_dx1 { stapel[0] } else { 0.0 };
                    let rest = &stapel[start..];
                    for k in rest.chunks(4) {
                        let (x1, y1) = (z.x + dx1, z.y + k[0]);
                        let (x2, y2) = (x1 + k[1], y1 + k[2]);
                        // added 03.10.2026. DER LETZTE WERT GEHOERT ZUR
                        // **Y**-ACHSE: `vvcurveto` ist
                        // `rcurveto(dx1, dya, dxb, dyb, 0, dyc)` — die
                        // letzte Differenz ist ein `dyc`, keine `dxc`.
                        // Vorher stand hier `(x2 + k[3], y2)`, das war
                        // genau der Fehler, und die beiden Achsen sind
                        // bei `hhcurveto` wieder vertauscht.
                        //
                        // GEMESSEN an `fw9.pdf` Glyphe 5, Operator
                        // `vvcurveto [-95, 153, -37, 147, 168]` von
                        // (269,-198): `fontTools` gibt den Bogen
                        // (174,-45) (137,102) **(137,270)**, der eigene
                        // Leser **(305,102)** — und ab da laufen alle
                        // folgenden Punkte um (168,-168) daneben.
                        let (x3, y3) = (x2, y2 + k[3]);
                        kubus(z, x1, y1, x2, y2, x3, y3);
                        z.x = x3;
                        z.y = y3;
                    }
                    stapel.clear();
                }
                27 => {
                    // hhcurveto: optional dy1, dann vier Zahlen je
                    // Kubus; der erste Kontrollpunkt (0, dy1), der letzte
                    // (0, dy4).
                    // added 03.10.2026: dieselbe Pruefung wie bei
                    // `vvcurveto` — optional `dy1` **vorangestellt**,
                    // dann vier Zahlen je Kurve.
                    if stapel.len() < 4 || !(stapel.len() % 4 == 0 || stapel.len() % 4 == 1) {
                        return Err(CffFehler::KeineZahl("hhcurveto"));
                    }
                    let mit_dy1 = stapel.len() % 4 == 1;
                    let start = usize::from(mit_dy1);
                    let dy1 = if mit_dy1 { stapel[0] } else { 0.0 };
                    let rest = &stapel[start..];
                    for k in rest.chunks(4) {
                        let (x1, y1) = (z.x + k[0], z.y + dy1);
                        let (x2, y2) = (x1 + k[1], y1 + k[2]);
                        // added 05.10.2026. DER LETZTE WERT GEHOERT ZUR
                        // **X**-ACHSE, und das ist der Grund, warum diese
                        // Stelle **zweimal** angefasst wurde: der erste
                        // Versuch brachte den letzten Wert auf die
                        // Y-Achse, und der Test
                        // `der_buchstabe_b_wird_gezeichnet` sagte darauf
                        // „13 der 18 On-Curve-Punkte fehlen".
                        //
                        // GEMESSEN an `NimbusRoman-Regular.otf` Glyphe
                        // `b` (gid 67): nach `vlineto` steht dort
                        // `hhcurveto` mit den Werten (1, 6, 3, 0, 8), und
                        // der **einzige** On-Curve-Punkt, den `fontTools`
                        // danach zeichnet, ist (20, 624), ausgehend von
                        // (3, 623). Das geht nur mit dem letzten Wert auf
                        // der X-Achse: c1 = (9, 624), c2 = (12, 624),
                        // Ende = (20, 624). Auf der Y-Achse kaeme
                        // (12, 632) heraus.
                        //
                        // `hhcurveto` ist also
                        // `rcurveto(dxa, dy1, dxb, dyb, dxc, 0)` und
                        // `vvcurveto` ist `rcurveto(dx1, dya, dxb, dyb, 0, dyc)`:
                        // die beiden Operatoren sind **spiegelbildlich**,
                        // nicht gleich gebaut. Das war der alte Kommentar
                        // dieser Stelle, und er hatte recht — die Messung
                        // bestaetigt ihn.
                        let (x3, y3) = (x2 + k[3], y2);
                        kubus(z, x1, y1, x2, y2, x3, y3);
                        z.x = x3;
                        z.y = y3;
                    }
                    stapel.clear();
                }
                // added 03.10.2026. DIE OPERATORNUMMERN, und sie sind
                // nicht verwechselbar mit Type 1: dort ist `31` das
                // `callgsubr` und `30` das `return`, hier ist es
                // `29`. `hvcurveto` ist **31** und beginnt
                // **waagerecht**, `vhcurveto` ist **30** und beginnt
                // senkrecht. Vertauscht man die beiden, bekommt jede
                // Glyphe ihre Boegen gespiegelt — das faellt bei `o`
                // nicht auf und bei `n` sofort.
                31 => hv(z, stapel, true)?,
                30 => hv(z, stapel, false)?,
                10 => {
                    // added 04.10.2026. **DER BIAS**, und er ist der
                    // Grund, warum `callsubr` in den elf PDFs noch nie
                    // vorgekommen ist: keine der dort eingebetteten CFF
                    // hat Subroutinen. Sobald eine Datei welche hat,
                    // zeigt sich, dass der Index **nicht** direkt steht.
                    //
                    // GEMESSEN an `NimbusRoman-Regular.otf`, Subroutine
                    // 33: `65 0a fc 5c 07 45 8a 0b` — `65` ist -38, und
                    // `0a` (`callsubr`) ruft danach **-38** auf, was nicht
                    // gehen kann. Mit Bias 107 (270 Subroutinen, also
                    // < 1240) wird daraus **69**, und `fontTools`
                    // bestaetigt die Laenge der aufgerufenen
                    // Unterroutine. Vorher stand hier `nr` direkt, und
                    // jeder Aufruf mit negativem Index brach mit
                    // `KeineZahl("callsubr")` ab.
                    let nr = stapel.pop().ok_or(CffFehler::KeineZahl("callsubr"))?;
                    // added 05.10.2026 (SCHLUSSPHASE): **KEINE KOPIE**
                    // der Subroutinentabelle. Vorher stand hier
                    // `.map(|p| p.subrs.clone())`, also wurden bei
                    // **jedem** `callsubr` alle Eintraege kopiert — bei
                    // `NimbusRoman-Regular.otf` 270 Eintraege, bei
                    // `f1040.pdf` noch mehr. Eine Glyphe mit 50 Aufrufen
                    // kopierte 50 Tabellen.
                    //
                    // GEMESSEN (Debug-Bau, 72 dpi, ganze Crate gebaut):
                    //   rfc9110 Seite 1 (1 566 Zeichen, alles Ersatz-
                    //     schrift mit Subroutinen)  304/305/311 ms
                    //   f1040 Seite 1 (5 073 Zeichen, 4 998 CFF)  422 ms
                    // Nach der Aenderung: siehe Bericht. Die Kopie war
                    // der Grund, warum `rufe` `&[Bereich]` **und** keine
                    // Referenz auf den Font nehmen konnte.
                    match self.privat.get(fd) {
                        Some(p) => {
                            let b = bias(p.subrs.len()) as f32;
                            self.rufe(&p.subrs, nr + b, z, fd, stapel)?;
                        }
                        // added 05.10.2026: KEIN FONT FUER DIESEN FD
                        // heisst: die Datei ruft eine Unterroutine, die
                        // es nicht gibt. Das ist ein **Fehler der
                        // Datei** und wird benannt, nicht stillschweigend
                        // als „leere Glyphe" behandelt.
                        None => return Err(CffFehler::ZuViel { was: "FD", n: fd }),
                    }
                }
                29 => {
                    let nr = stapel.pop().ok_or(CffFehler::KeineZahl("callgsubr"))?;
                    // added 05.10.2026 (SCHLUSSPHASE): auch hier keine
                    // Kopie der ganzen globalen Tabelle (271 Eintraege
                    // bei derselben Datei).
                    let b = bias(self.global_subrs.len()) as f32;
                    self.rufe(&self.global_subrs, nr + b, z, fd, stapel)?;
                }
                11 => {
                    // `return`: das Unterprogramm ist zu Ende.
                    return Ok(());
                }
                14 => {
                    if stapel.len() >= 4 {
                        // `seac` (Type 1). In den elf PDFs nie.
                        self.seac.set(self.seac.get() + 1);
                    }
                    schliesse(z);
                    return Ok(());
                }
                12 => {
                    let b2 = *daten.get(p).ok_or(CffFehler::ZuKurz {
                        braucht: p + 1,
                        hat: daten.len(),
                    })?;
                    p += 1;
                    escape(b2, z, stapel)?;
                }
                0..=27 => {
                    // added 03.10.2026. DIE OPERATOREN 0 BIS 27 OHNE
                    // EIGENE BEDEUTUNG IM CHARSTRING.
                    //
                    // `0` ist nach der Spezifikation `dotsection` in
                    // Type 1 und **nichts** in Type 2; `26`/`27` sind
                    // `vvcurveto`/`hhcurveto` (oben behandelt). Was hier
                    // ankommt, ist eine Datei, die sich verschoben hat.
                    return Err(CffFehler::Unbekannt(b0));
                }
                _ => {
                    // added 03.10.2026. 31 ist in **Type 1** `callgsubr`
                    // und `return`; in Type 2 gibt es 31 nicht. Wer es
                    // antrifft, hat eine Type-1-Datei ohne `eexec`
                    // erwischt — das ist ein Befund, kein Grund zum
                    // Raten.
                    return Err(CffFehler::Unbekannt(b0));
                }
            }
        }
        schliesse(z);
        z.tief -= 1;
        Ok(())
    }

    /// Ruft Unterprogramm `nr` auf.
    fn rufe(
        &self,
        subrs: &[Bereich],
        nr: f32,
        z: &mut Zustand,
        fd: usize,
        stapel: &mut Vec<f32>,
    ) -> Result<(), CffFehler> {
        if !nr.is_finite() || nr < 0.0 {
            return Err(CffFehler::KeineZahl("callsubr"));
        }
        let i = nr as usize;
        // added 03.10.2026. AUSSERHALB DER TABELLE IST EIN FEHLER
        // DER DATEI und wird **gemeldet**, nicht geraten: `1207.7214.pdf`
        // hat in einer der Schriften einen `callsubr` auf eine Private
        // DICT ohne `Subrs`. Ohne Meldung zeichnete die Glyphe still
        // unvollstaendig.
        let b = *subrs.get(i).ok_or(CffFehler::ZuViel {
            was: "Subr-Index",
            n: i,
        })?;
        if b.ende > self.daten.len() || b.ende <= b.start {
            return Err(CffFehler::ZuViel {
                was: "Subr",
                n: b.ende.saturating_sub(b.start),
            });
        }
        // added 05.10.2026 (SCHLUSSPHASE): **keine** Kopie der
        // Unterroutine. `lauf` nimmt `&[u8]`, und `self.daten` lebt
        // mindestens so lange wie der Aufruf — die `.to_vec()` war nur
        // eine Gewohnheit aus der Zeit, als `lauf` den Font noch
        // mutabel brauchte. Bei 8-Byte-Unterroutinen kostete sie
        // nichts, bei 80 Byte und 50 Aufrufen je Glyphe 4 000 Byte
        // Kopierarbeit **pro Zeichen**.
        // added 05.10.2026: `&self.daten[..]` genuegt — `lauf` nimmt
        // `&[u8]`, und der Slice traegt den Typ bereits. Die
        //Zwischenform `.as_slice()` ist instabil (`str_as_str`), also
        // weg.
        let daten = &self.daten[b.start..b.ende];
        if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
            eprintln!(
                "SUBR {nr} ({i}) {} Byte: {:?}",
                daten.len(),
                daten
                    .iter()
                    .take(20)
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join("")
            );
        }
        self.lauf(daten, z, fd, stapel)
    }

    /// Liest eine CFF aus einem **OTF** (`OTTO`), also aus einer
    /// OpenType-Datei, in der die CFF eine Tabelle unter den anderen ist.
    ///
    /// added 04.10.2026. WARUM DAS HIER LIEGT: die Dateien, die
    /// `fc-match` fuer die Standard-14-Schriften ausgibt, sind genau
    /// solche OpenType-Dateien — GEMESSEN:
    /// `fc-match Times` -> `NimbusRoman-Regular.otf`, und die liegt in
    /// `/usr/share/fonts/opentype/urw-base35/`. Ihre Tabelle `CFF` steht
    /// bei Offset 204 und ist 70 824 Byte lang. Wer die Schrift nehmen
    /// will, die der Referenz-Renderer nimmt, muss sie aus dem OTF
    /// herausschneiden koennen.
    ///
    /// DER AUFBAU EINES sfnt-KOPFES, und er ist kurz: 4 Byte Signatur
    /// (`OTTO` oder `true`), dann die Zahl der Tabellen, dann je Tabelle
    /// 16 Byte — Tag (4), Pruefsumme (4), Offset (4), Laenge (4). Gesucht
    /// wird die Tabelle `CFF ` (mit Leerzeichen; `CFF2` ist eine andere,
    /// komprimierte Sache und wird nicht genommen).
    pub fn aus_otf(daten: &[u8]) -> Result<Cff, CffFehler> {
        if daten.len() < 12 {
            return Err(CffFehler::ZuKurz {
                braucht: 12,
                hat: daten.len(),
            });
        }
        let sig = &daten[..4];
        if sig != b"OTTO" && sig != b"true" {
            return Err(CffFehler::TabelleFehlt("sfnt-Signatur"));
        }
        let anzahl = ((daten[4] as usize) << 8) | daten[5] as usize;
        if anzahl == 0 || anzahl > 512 {
            return Err(CffFehler::TabelleFehlt("Tabellenzahl"));
        }
        for i in 0..anzahl {
            let kopf = 12 + i * 16;
            if kopf + 16 > daten.len() {
                return Err(CffFehler::ZuKurz {
                    braucht: kopf + 16,
                    hat: daten.len(),
                });
            }
            if &daten[kopf..kopf + 4] != b"CFF " {
                continue;
            }
            let versatz = u32::from_be_bytes([
                daten[kopf + 8],
                daten[kopf + 9],
                daten[kopf + 10],
                daten[kopf + 11],
            ]) as usize;
            let laenge = u32::from_be_bytes([
                daten[kopf + 12],
                daten[kopf + 13],
                daten[kopf + 14],
                daten[kopf + 15],
            ]) as usize;
            // added 04.10.2026: `checked_add` gibt ein `Option`, und `?`
            // verlangt hier ein `Result` — der erste Versuch hat genau
            // daran gescheitert. Der Fehler ist derselbe wie bei einer
            // Laenge von 0.
            let passt = versatz.checked_add(laenge);
            if laenge == 0 || passt.map(|e| e > daten.len()).unwrap_or(true) {
                return Err(CffFehler::TabelleFehlt("CFF-Tabelle"));
            }
            let mut cff = Cff::lesen(&daten[versatz..versatz + laenge])?;
            // added 05.10.2026: DIE `hmtx` GEHOERT ZUR **DATEI**, nicht
            // zur CFF-Tabelle — sie wird darum aus dem **ganzen** OTF
            // gelesen, nicht aus dem Ausschnitt. `glyphen_anzahl()` ist
            // die Zahl der CharStrings, weil die letzte Breite sonst
            // fehlt.
            let breiten = Cff::hmtx_lesen(daten, cff.glyphen_anzahl());
            if !breiten.is_empty() {
                cff.vorschub = breiten;
            }
            return Ok(cff);
        }
        Err(CffFehler::TabelleFehlt("CFF-Tabelle"))
    }

    /// added 05.10.2026: `hmtx` einer OpenType-Datei: GID -> Vorschub.
    ///
    /// DER AUFBAU, kurz und hingesetzt, weil ihn sonst niemand nachschlaegt:
    /// `hhea` nennt bei Offset 34 (uint16) die Zahl der Eintraege in
    /// `hmtx` mit **echter** Breite (`numberOfHMetrics`). Danach folgen in
    /// `hmtx` so viele Paare (Breite uint16, linker Rand int16). Fuer die
    /// restlichen Glyphen steht kein Paar mehr; sie erben die letzte
    /// Breite, und ihre linken Raender liegen danach als int16 ohne Breite.
    ///
    /// GEMESSEN an `NimbusRoman-Regular.otf`: `b` (GID 67) hat dort 500,
    /// und die Schreibmaschinenschrift in `1706.03762.pdf` gibt im
    /// `/Widths` 524,9 an — die 4,9 % Unterschied sind genau das, was der
    /// Zeichner durch Strecken ausgleichen muss.
    pub fn hmtx_lesen(daten: &[u8], glyphen: usize) -> Vec<u16> {
        let anzahl = ((daten[4] as usize) << 8) | daten[5] as usize;
        let mut hhea = None;
        let mut hmtx = None;
        for i in 0..anzahl {
            let kopf = 12 + i * 16;
            if kopf + 16 > daten.len() {
                return Vec::new();
            }
            let versatz = u32::from_be_bytes([
                daten[kopf + 8],
                daten[kopf + 9],
                daten[kopf + 10],
                daten[kopf + 11],
            ]) as usize;
            match &daten[kopf..kopf + 4] {
                b"hhea" => hhea = Some(versatz),
                b"hmtx" => hmtx = Some(versatz),
                _ => {}
            }
        }
        let (Some(hhea), Some(hmtx)) = (hhea, hmtx) else {
            return Vec::new();
        };
        if hhea + 36 > daten.len() {
            return Vec::new();
        }
        let n = u16::from_be_bytes([daten[hhea + 34], daten[hhea + 35]]) as usize;
        if n == 0 || n > glyphen.max(1) {
            return Vec::new();
        }
        let mut raus: Vec<u16> = Vec::with_capacity(n);
        for i in 0..n {
            let p = hmtx + i * 4;
            if p + 2 > daten.len() {
                break;
            }
            raus.push(u16::from_be_bytes([daten[p], daten[p + 1]]));
        }
        if raus.is_empty() {
            return raus;
        }
        // added 05.10.2026: DIE GLYPHEN OHNE EIGENES PAAR erben die letzte
        // Breite. Ohne das endet die Tabelle zu frueh, und `vorschub`
        // liefert fuer hohe GIDs `None` — der Zeichner streckt dann
        // wieder nicht, und genau die Glyphen fehlen, die am seltensten
        // vorkommen und deshalb am leichtesten zu uebersehen sind.
        let letzte = *raus.last().unwrap_or(&0);
        while raus.len() < glyphen {
            raus.push(letzte);
        }
        raus
    }

    /// added 05.10.2026: die **natuerliche** Breite einer Glyphe in
    /// Font-Einheiten, falls die Datei eine `hmtx` mitbringt.
    ///
    /// Der Zeichner braucht sie, um eine Ersatzglyphe auf die Breite aus
    /// `/Widths` des Dokuments zu **strecken**; siehe
    /// `Ersatzschrift::vorschub` und den Abschnitt „hmtx" in
    /// `team/BERICHT_SERVER_PDF_GLYPHEN.txt`.
    pub fn vorschub(&self, gid: u16) -> Option<f32> {
        self.vorschub.get(gid as usize).map(|w| *w as f32)
    }

    /// Die `cmap`-Tabelle einer OpenType-Datei: Zeichen -> GID.
    ///
    /// added 04.10.2026. WARUM DAS HIER IST: eine CFF kennt nur **Namen**
    /// (`gid_von_name`), und die Standard-Strings von Adobe benennen
    /// 140 Zeichen anders als man sie schreibt (32 heisst `space`, 33
    /// `exclam`, 47 `slash`). Fuer eine *Ersatzschrift* braucht man aber
    /// „welche Glyphe ist das Zeichen `e`“ — und dafuer ist die `cmap`
    /// der richtige und uebliche Weg.
    ///
    /// GEMESSEN an `NimbusRoman-Regular.otf`: die Tabelle `cmap` steht ab
    /// Offset 91 168, 2 860 Byte lang, und sie enthaelt eine
    /// Format-4-Tabelle fuer die lateinischen Schriftsysteme. Das ist die
    /// Tabelle, mit der der Referenz-Renderer das Blatt setzt.
    ///
    /// GELESEN WIRD Format 4 (Segmente zu 2 Byte) und Format 12 (zu 4
    /// Byte). Alles andere liefert eine leere Tabelle und damit eine
    /// brauchbare Fehlermeldung statt eines Absturzes.
    pub fn cmap_lesen(daten: &[u8]) -> Result<HashMap<char, u16>, CffFehler> {
        let mut raus = HashMap::new();
        let mut versatz = None;
        let anzahl = ((daten[4] as usize) << 8) | daten[5] as usize;
        for i in 0..anzahl {
            let kopf = 12 + i * 16;
            if kopf + 16 > daten.len() {
                return Err(CffFehler::ZuKurz {
                    braucht: kopf + 16,
                    hat: daten.len(),
                });
            }
            if &daten[kopf..kopf + 4] != b"cmap" {
                continue;
            }
            let o = u32::from_be_bytes([
                daten[kopf + 8],
                daten[kopf + 9],
                daten[kopf + 10],
                daten[kopf + 11],
            ]) as usize;
            versatz = Some(o);
            break;
        }
        let basis = versatz.ok_or(CffFehler::TabelleFehlt("cmap"))?;
        if basis + 4 > daten.len() {
            return Err(CffFehler::TabelleFehlt("cmap"));
        }
        let n = ((daten[basis + 2] as usize) << 8) | daten[basis + 3] as usize;
        for i in 0..n {
            let k = basis + 4 + i * 8;
            if k + 8 > daten.len() {
                break;
            }
            let plat = ((daten[k] as u16) << 8) | daten[k + 1] as u16;
            let enc = ((daten[k + 2] as u16) << 8) | daten[k + 3] as u16;
            let sub = basis
                + u32::from_be_bytes([daten[k + 4], daten[k + 5], daten[k + 6], daten[k + 7]])
                    as usize;
            // GEMESSEN: `NimbusRoman-Regular.otf` hat (3,1) und (3,10);
            // die letzte zaehlt, weil sie die vollstaendigste ist.
            let brauchbar = matches!((plat, enc), (3, 1) | (3, 10) | (0, _) | (3, 0));
            if !brauchbar || sub + 4 > daten.len() {
                continue;
            }
            let format = ((daten[sub] as u16) << 8) | daten[sub + 1] as u16;
            if format == 4 {
                Self::cmap_format4(daten, sub, &mut raus);
            } else if format == 12 {
                Self::cmap_format12(daten, sub, &mut raus);
            }
        }
        Ok(raus)
    }

    /// cmap-Format 4: Segmente zu 2 Byte (Anfang, Ende, delta-oder-array).
    ///
    /// added 04.10.2026. Beide Format-Leser sind **Methoden** und keine
    /// freien Funktionen: sie stehen mit `Self::` im Aufruf, weil sie
    /// beim ersten Versuch als freie Funktionen eingefuegt waren und
    /// der Compiler sie dann nicht fand.
    fn cmap_format4(daten: &[u8], sub: usize, raus: &mut HashMap<char, u16>) {
        if sub + 14 > daten.len() {
            return;
        }
        let seg_2 = u16::from_be_bytes([daten[sub + 6], daten[sub + 7]]) as usize;
        let seg = seg_2 / 2;
        if seg == 0 || sub + 14 + seg_2 * 4 > daten.len() {
            return;
        }
        // added 04.10.2026. DIE VIER FELDER EINES SEGMENTS stehen **nicht**
        // so, wie sie im Kopf aussehen: nach `sub + 14` kommen erst die
        // `endCode`-Werte, dann 2 Byte Reserve, dann `startCode`, dann
        // `idDelta`, dann `idRangeOffset`.
        //
        //     ende     = sub + 14                 endCode[0]
        //     ende + seg_2                    Reserve
        //     start    = ende + seg_2 + 2         startCode[0]
        //     delta    = start + seg_2           idDelta[0]
        //     bereich  = start + seg_2 + seg_2   idRangeOffset[0]
        //
        // GEMESSEN, und der Test hats gemeldet: mit
        // `ende + seg_2 * 2 + 2` als Start und dem **zweiten**
        // `endCode`-Feld als Ende landete ein Zeichen zuviel in der
        // Tabelle — `B` war da, obwohl das Segment nur `A` abdeckt.
        let ende = sub + 14;
        let start = ende + seg_2 + 2;
        for s in 0..seg {
            let v = |at: usize| -> u16 {
                if at + 1 >= daten.len() {
                    0
                } else {
                    u16::from_be_bytes([daten[at], daten[at + 1]])
                }
            };
            let bis = v(ende + s * 2);
            let von = v(start + s * 2);
            if von == 0xFFFF || bis == 0xFFFF || bis < von {
                continue;
            }
            let delta = v(start + seg_2 + s * 2);
            let bereich = v(start + seg_2 * 2 + s * 2);
            for c in von..=bis {
                let Some(z) = char::from_u32(c as u32) else {
                    continue;
                };
                let gid = if bereich == 0 {
                    c.wrapping_add(delta)
                } else {
                    // added 05.10.2026 (KOORDINATION, „Ligaturen fehlen"):
                    // DIE BASIS VON `idRangeOffset`, und sie war um
                    // `seg_2` zu klein. Der Versatz zeigt **vom Wort
                    // `idRangeOffset` selbst** auf das `glyphIdArray`
                    // (OpenType-Spezifikation, Tabelle `cmap` Format 4).
                    //
                    // GEMESSEN, was der alte Wert kostet, mit
                    // `werkzeuge/cmap_gegenprobe.py` an
                    // `NimbusRoman-Regular.otf`:
                    //
                    //     richtig (vom idRangeOffset-Wort):   854 Codes
                    //     alt    (vom idDelta-Feld):           732 Codes
                    //
                    // Also **122 Codes fehlen** — darunter `U+FB01` (die
                    // Ligatur `fi`), `U+0141` (`Lslash`, das „L mit
                    // Strich"), `U+0131` (`dotlessi`) — und **330 Codes
                    // bekommen die falsche Glyphe**, weil gelesen wird:
                    //
                    //     U+00D7 (×)  ->  GID 862 statt 168
                    //     U+00AB («)  ->  GID 338 statt 106
                    //     U+00AD     ->  GID 774 statt 353
                    //
                    // GEMESSENE WIRKUNG IM BILD: „figures" kam als
                    // „ gures" heraus, „significantly" als „signi
                    // cantly", und „Lukasz" ohne den Strich im `L` — die
                    // drei Faelle, die die Koordination genannt hat. Und
                    // still: der Zeichenfehler in der Bilanz war **0**,
                    // weil `glyph_index` `None` liefert und der Zweig
                    // dann kommentarlos uebersprungen wird.
                    let p = start + seg_2 * 2 + s * 2 + bereich as usize + (c - von) as usize * 2;
                    let g = v(p);
                    if g == 0 { 0 } else { g.wrapping_add(delta) }
                };
                if gid != 0 {
                    raus.insert(z, gid);
                }
            }
        }
    }

    /// cmap-Format 12: Segmente zu 4 Byte (dieselbe Logik, 32 Bit).
    fn cmap_format12(daten: &[u8], sub: usize, raus: &mut HashMap<char, u16>) {
        if sub + 16 > daten.len() {
            return;
        }
        let n = u32::from_be_bytes([
            daten[sub + 12],
            daten[sub + 13],
            daten[sub + 14],
            daten[sub + 15],
        ]) as usize;
        if n > 200_000 {
            return;
        }
        for i in 0..n {
            let k = sub + 16 + i * 12;
            if k + 12 > daten.len() {
                return;
            }
            let von = u32::from_be_bytes([daten[k], daten[k + 1], daten[k + 2], daten[k + 3]]);
            let bis = u32::from_be_bytes([daten[k + 4], daten[k + 5], daten[k + 6], daten[k + 7]]);
            let delta =
                u32::from_be_bytes([daten[k + 8], daten[k + 9], daten[k + 10], daten[k + 11]]);
            if bis < von || bis - von > 0x10000 {
                continue;
            }
            for c in von..=bis {
                let Some(z) = char::from_u32(c) else { continue };
                let gid = (c + delta) & 0xFFFF;
                if gid != 0 {
                    raus.insert(z, gid as u16);
                }
            }
        }
    }

    /// Liest `charset` in den Formaten 0, 1 und 2.
    ///
    /// added 03.10.2026. DER UNTERSCHIED ZWISCHEN 1 UND 2 IST **EIN**
    /// BYTE, und es ist genau das Byte, das man beim Lesen weglässt:
    /// Format 1 hat `erste SID` (2 Byte) und `nLeft` (**1** Byte),
    /// Format 2 hat `erste SID` (2 Byte) und `nLeft` (**2** Byte). Sonst
    /// ist alles gleich — ein Bereich aus `nLeft + 1` SIDs, die
    /// aufeinanderfolgen.
    ///
    /// GEMESSEN: alle fuenf CFF der elf PDFs benutzen Format 0. Der Font
    /// aus `fw9.pdf` mit 81 Glyphen benutzt Format 1 mit Bereichen der
    /// Länge 1, 2 und 4; mit **einem** Byte fuer `nLeft` ergaben sich die
    /// SIDs `0 1 5 6 8 9 10 11` (`.notdef space dollar percent
    /// quoteright parenleft parenright asterisk`) und alle Glyphennamen
    /// passten. Mit zwei Byte waere ab dem zweiten Bereich alles um eins
    /// verschoben — und die Zuordnung Name -> Glyphe waere still falsch.
    fn charset_laden(&self, off: usize, ziel: &mut [u32]) -> Result<(), CffFehler> {
        let d = &self.daten;
        let format = *d.get(off).ok_or(CffFehler::ZuKurz {
            braucht: off + 1,
            hat: d.len(),
        })?;
        let mut p = off + 1;
        let hol = |i: usize| -> Result<u8, CffFehler> {
            d.get(i).copied().ok_or(CffFehler::ZuKurz {
                braucht: i + 1,
                hat: d.len(),
            })
        };
        if format == 0 {
            for slot in ziel.iter_mut().skip(1) {
                let a = hol(p)? as u32;
                let b = hol(p + 1)? as u32;
                *slot = (a << 8) | b;
                p += 2;
            }
            return Ok(());
        }
        if format != 1 && format != 2 {
            return Err(CffFehler::Unbekannt(format));
        }
        let mut gid = 1usize;
        while gid < ziel.len() {
            let first = u16::from_be_bytes([hol(p)?, hol(p + 1)?]) as usize;
            let n_left = if format == 1 {
                hol(p + 2)? as usize
            } else {
                u16::from_be_bytes([hol(p + 2)?, hol(p + 3)?]) as usize
            };
            p += if format == 1 { 3 } else { 4 };
            if gid + n_left >= ziel.len() {
                return Err(CffFehler::ZuViel {
                    was: "charset",
                    n: gid + n_left,
                });
            }
            for k in 0..=n_left {
                ziel[gid + k] = (first + k) as u32;
            }
            gid += n_left + 1;
        }
        Ok(())
    }

    /// Liest `FDSelect` in den Formaten 0, 1 und 3.
    ///
    /// added 03.10.2026. GEMESSEN: `1512.03385.pdf` hat **zwei**
    /// `/CIDFontType0`, und beide brauchen ein eigenes FDArray — ohne
    /// `FDSelect` bekaeme jede Glyphe die Unterprogramme der ersten
    /// und damit ein **falsches** Bild, das keiner prueft, weil es
    /// Buchstaben sind.
    fn fd_select_laden(&mut self, off: usize) -> Result<(), CffFehler> {
        let d = &self.daten;
        let hol = |i: usize| -> Result<u8, CffFehler> {
            d.get(i).copied().ok_or(CffFehler::ZuKurz {
                braucht: i + 1,
                hat: d.len(),
            })
        };
        let n = self.glyphen.len();
        let format = hol(off)?;
        let mut out = vec![0u16; n];
        match format {
            0 => {
                for (i, slot) in out.iter_mut().enumerate() {
                    *slot = hol(off + 1 + i)? as u16;
                }
            }
            3 => {
                let n_ranges = hol(off + 1)? as usize;
                let mut p = off + 3;
                // added 03.10.2026. FORM 3 IST EINE **LISTE** VON
                // Bereichen, kein Sentinel: jeder Eintrag nennt
                // `erste Glyphe` und `FD`, und der Bereich laeuft bis
                // zur naechsten Anfangsnummer. Ohne den letzten
                // Sentinel-Abschluss bekommt die **letzte** Glyphe des
                // Fonts gar kein FD — und damit leere Unterprogramme.
                for _ in 0..n_ranges {
                    let first = u16::from_be_bytes([hol(p)?, hol(p + 1)?]) as usize;
                    let fd = hol(p + 2)? as u16;
                    p += 3;
                    if p + 1 < d.len() {
                        let naechste = u16::from_be_bytes([hol(p)?, hol(p + 1)?]) as usize;
                        for slot in out.iter_mut().take(naechste.min(n)).skip(first) {
                            *slot = fd;
                        }
                    } else {
                        for slot in out.iter_mut().take(n).skip(first) {
                            *slot = fd;
                        }
                    }
                }
            }
            1 | 2 => {
                let mut p = off + 1;
                let mut gid = 0usize;
                while gid < n {
                    let first = u16::from_be_bytes([hol(p)?, hol(p + 1)?]) as u32;
                    if format == 1 {
                        let n_left = hol(p + 2)? as u32;
                        let fd = hol(p + 3)? as u16;
                        p += 4;
                        let ende = (first + n_left).min(n as u32);
                        for slot in out.iter_mut().take(ende as usize).skip(first as usize) {
                            *slot = fd;
                        }
                        gid = ende as usize;
                    } else {
                        // Format 2: `erste` (2 Byte), dann `fd` (1 Byte)
                        // und ein Sentinel (2 Byte).
                        let fd = hol(p + 2)? as u16;
                        let ende = u16::from_be_bytes([hol(p + 3)?, hol(p + 4)?]) as u32;
                        p += 5;
                        let ende = ende.min(n as u32);
                        for slot in out.iter_mut().take(ende as usize).skip(first as usize) {
                            *slot = fd;
                        }
                        gid = ende as usize;
                    }
                    if gid == 0 {
                        break;
                    }
                }
            }
            _ => return Err(CffFehler::Unbekannt(format)),
        }
        self.fd_select = out;
        Ok(())
    }

    /// Liest die **eingebaute** Kodierung (Top DICT 16), Formate 0/1.
    fn encoding_laden(&mut self, off: usize) {
        let d = self.daten.clone();
        let format = match d.get(off) {
            Some(f) => *f,
            None => return,
        };
        match format & 0x7f {
            0 => {
                let n = d.get(off + 1).copied().unwrap_or(0) as usize;
                for code in 0..n {
                    if let Some(&sid) = d.get(off + 2 + code) {
                        self.eingebaut.insert(code as u32, sid as u32);
                    }
                }
            }
            1 => {
                let mut p = off + 1;
                while let Some(&first) = d.get(p) {
                    let n_left = d.get(p + 1).copied().unwrap_or(0) as usize;
                    let sid = d.get(p + 2).copied().unwrap_or(0) as u32;
                    p += 3;
                    for k in 0..=n_left {
                        self.eingebaut
                            .insert((first as usize + k) as u32, sid + k as u32);
                    }
                    if p >= d.len() {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

/// Der Zustand eines laufenden Charstrings.
struct Zustand {
    x: f32,
    y: f32,
    /// added 04.10.2026: die groesste erreichte Verschachtelungstiefe
    /// einer Glyphe (gemessen, siehe `MAX_AUFRUFE`).
    pub max_tief: u32,
    /// Zahl der Staemme (fuer `hintmask`).
    n_stems: usize,
    /// Eine Kontur ist offen.
    offen: bool,
    /// Der offene Pfad.
    punkte: Vec<(f32, f32)>,
    /// Die fertigen Konturen.
    konturen: Vec<Vec<(f32, f32)>>,
    /// Ob die Breite schon gelesen wurde.
    breite_genommen: bool,
    /// Die Breite, falls sie aus dem Charstring kam.
    breite: f32,
    /// Zahl der Befehle (Bombenschranke).
    takte: u32,
    /// Verschachtelungstiefe.
    tief: u32,
}

impl Zustand {
    fn neu() -> Zustand {
        Zustand {
            x: 0.0,
            y: 0.0,
            n_stems: 0,
            offen: false,
            punkte: Vec::new(),
            konturen: Vec::new(),
            breite_genommen: false,
            breite: 0.0,
            takte: 0,
            tief: 0,
            max_tief: 0,
        }
    }
}

/// Schliesst eine offene Kontur ab.
fn schliesse(z: &mut Zustand) {
    if z.offen && z.punkte.len() >= 2 {
        z.konturen.push(std::mem::take(&mut z.punkte));
    }
    z.punkte.clear();
    z.offen = false;
}

/// Setzt den aktuellen Punkt (und eroeffnet die Kontur bei Bedarf).
fn punkt(z: &mut Zustand) {
    if !z.offen {
        z.offen = true;
        z.punkte.clear();
    }
    if z.punkte.len() < MAX_PUNKTE {
        z.punkte.push((z.x, z.y));
    }
}

/// Setzt einen Punkt **und** verschiebt die Position auf ihn.
fn punkt_abs(z: &mut Zustand, x: f32, y: f32) {
    if !z.offen {
        z.offen = true;
        z.punkte.clear();
    }
    if z.punkte.len() < MAX_PUNKTE {
        z.punkte.push((x, y));
    }
    z.x = x;
    z.y = y;
}

/// Rastert einen Kubus in Geradenstuecke.
///
/// added 03.10.2026. DER ENDPUNKT WIRD **GESETZT**, nicht gerechnet:
/// der letzte Stueckpunkt ist der Endpunkt des Kubus, und er muss
/// exakt dort landen, wo der naechste Befehl weiterarbeitet. Sonst
/// driftet der Pfad um die Summe der Rundungsfehler, und bei 40
/// Geradenstuecken je Glyphe sind das bis zu 40 Einheiten.
fn kubus(z: &mut Zustand, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32) {
    // added 03.10.2026. DER KURVENANFANG WIRD **VOR** DER SCHLEIFE
    // kopiert, und das war der Fehler, der die Buchstaben zu
    // senkrechten Stacheln verzerrte.
    //
    // DER GRUND: `punkt_abs` schreibt am Ende jeden Stueckpunkt nach
    // `z.x`/`z.y`. Rechnet die Formel im nächsten Schritt wieder mit
    // `z.x`, dann ist das nicht mehr der **Anfangspunkt** der Kurve,
    // sondern der **vorige** Stueckpunkt — die Kurve „laeuft" dann von
    // Punkt zu Punkt statt von einem Kontrollpunkt zum naechsten.
    //
    // GEMESSEN an `fw9.pdf` Seite 1 bei 300 dpi: die Glyphen waren
    // **richtig erkennbar** (die BBox von `F` war (78,0)-(547,714),
    // also stimmten die Koordinaten) und trotzdem senkrecht verzerrt
    // mit langen Spitzen nach oben und unten. Die BBox allein kann
    // das nicht zeigen — sie ist bei beiden Varianten dieselbe, weil
    // die Endpunkte stimmen und nur die Punkte dazwischen falsch
    // liegen.
    let x0 = z.x;
    let y0 = z.y;
    if !z.offen {
        z.offen = true;
        z.punkte.clear();
        punkt_abs(z, x0, y0);
    }
    for i in 1..=KURVEN_STUECKE {
        if i == KURVEN_STUECKE {
            punkt_abs(z, x3, y3);
        } else {
            let t = i as f32 / KURVEN_STUECKE as f32;
            let u = 1.0 - t;
            // added 03.10.2026. DIE FORMEL VON BEZIER, und ihr fehlten
            // **zwei Dreien**: es heisst `u³P0 + 3u²t·P1 + 3ut²·P2 + t³P3`.
            // Mit `2u²t` und `ut²` statt `3u²t` und `3ut²` kommt der
            // Bogen an beiden **Enden** trotzdem richtig an — deshalb
            // stimmten BBox und Endpunkte — und in der Mitte daneben.
            //
            // GEMESSEN an `fw9.pdf`, Glyphe 63, gegen `fontTools`
            // (`werkzeuge/cff_ref.py`): der Bogen von (149,292) ueber
            // (149,385) und (199,454) nach (302,454) liefert bei
            // `t = 1/16` **richtig** (149.6, 309.1) und mit der
            // falschen Formel (140.7, 286.3). Genau dieses falsche
            // (140.7, 286.3) hatte der Zeichner, und daraus kamen die
            // senkrechten Stacheln: der Bogen schneidet sich selbst
            // und fuellt beim Fuellen eine Spitze.
            let x = u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x3;
            let y = u * u * u * y0 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y3;
            punkt_abs(z, x, y);
        }
    }
}

/// Wie viele Staemme stehen auf dem Stapel?
///
/// added 03.10.2026. DIE REGEL, und sie hat zwei Faelle: bei gerader
/// Zahl der Argumente ist es die Haelfte, bei ungerader Zahl ist das
/// **erste** die Breite und es sind (n - 1) / 2 Staemme. Deshalb
/// zaehlt `hstem` mit drei Argumenten **einen** Stamm, nicht zwei —
/// und `hintmask` muss danach genau ein Byte ueberspringen.
fn stamm_zaehlen(stapel: &[f32]) -> usize {
    match stapel.len() {
        0 => 0,
        n => {
            if n % 2 == 1 {
                (n - 1) / 2
            } else {
                n / 2
            }
        }
    }
}

/// Liest die optionale Breite am Anfang eines `moveto`.
fn breite_vielleicht(z: &mut Zustand, stapel: &mut Vec<f32>, erwartet: usize) {
    if stapel.len() > erwartet && !z.breite_genommen {
        z.breite = stapel[0];
        z.breite_genommen = true;
        stapel.remove(0);
    }
}

/// `hvcurveto` und `vhcurveto`.
///
/// added 03.10.2026. DIE LETZTE KURVE HAT **FIVE** Argumente: der
/// erste Kontrollpunkt wird mit dem vorigen Endpunkt verrechnet (die
/// `_same`-Variante). Ohne das bekommt der Bogen am Fuss jedes `f`
/// eine Ecke zu viel, und die ist bei 9,96 Punkt ein sichtbares
/// Eck.
fn hv(z: &mut Zustand, stapel: &mut Vec<f32>, waagerecht_zuerst: bool) -> Result<(), CffFehler> {
    if stapel.len() < 4 {
        return Err(CffFehler::KeineZahl("hvcurveto"));
    }
    let mut i = 0usize;
    let mut waagerecht = waagerecht_zuerst;
    let n = stapel.len();
    // added 03.10.2026. DIE ABBRUCHBEDINGUNG IST **VIER**, und sie
    // stand hier vorher nicht — der Index `i + 3` lief bei einem Rest
    // von eins bis drei Argumenten aus dem Stapel hinaus:
    //
    //     panicked at cff.rs:1149: index out of bounds: len is 9,
    //     index is 9
    //
    // GEMESSEN an `fw9.pdf`, wo ein `hvcurveto` mit 4 + 4 + **1**
    // Argumenten dasteht: nach zwei Kurven bleiben drei Zahlen uebrig,
    // und die sind kein `hvcurveto`. Die Spezifikation sagt dasselbe:
    // jede Kurve nimmt vier oder fuenf Zahlen, ein Rest von eins bis
    // drei ist ein Fehler der Datei — und ein Fehler, der die
    // **Zeichenfolge** beendet, nicht den Absturz des Programms.
    while n - i >= 4 {
        let letzte = n - i == 5;
        let (d0, d1, d2, d3) = (stapel[i], stapel[i + 1], stapel[i + 2], stapel[i + 3]);
        let d4 = if letzte {
            stapel.get(i + 4).copied().unwrap_or(0.0)
        } else {
            0.0
        };
        if letzte {
            // `_same`: der erste Kontrollpunkt liegt auf der Geraden
            // vom vorigen Endpunkt in Richtung der **letzten** Kurve.
            let (x1, y1) = if waagerecht {
                (z.x + d0, z.y)
            } else {
                (z.x, z.y + d0)
            };
            // added 03.10.2026. DIE ZWEITE KONTROLLPUNKT-DIFFERENZ
            // GEHOERT IMMER ZUR **Y**-ACHSE, wenn waagerecht, und zur
            // **X**-ACHSE, wenn senkrecht. Der erste Kontrollpunkt
            // wandert dagegen nur auf der jeweils anderen Achse. Der
            //mittlere Kontrollpunkt (x2,y2) hat in beiden Faellen
            // dieselbe Form — deshalb war hier vorher ein `if` mit
            // zwei **identischen** Zweigen, und clippy hat es gesagt.
            let (x2, y2) = (x1 + d1, y1 + d2);
            // added 03.10.2026. DIE FUENFTE ZAHL GEHOERT ZUR **JEWEILS
            // ANDEREN** ACHSE als die vierte, und das war hier beides
            // verkehrt herum. `hvcurveto` ist fuer den waagerechten
            // Bogen `hcurveto(dxa, dxb, dyb, dyc, dxc)`, der Endpunkt
            // also `x2 + dxc` — **x**, nicht y. Und `vhcurveto` nimmt
            // im senkrechten Bogen `vcurveto(dya, dxb, dyb, dxc, dyc)`,
            // der Endpunkt `y2 + dyc` — **y**, nicht x.
            //
            // GEMESSEN an `fw9.pdf`: der Bogen aus `vhcurveto` mit
            // fuenf Argumenten lag dadurch um (dyc, 0) daneben, und
            // weil die Kontur danach weiterlaeuft, verschoben sich alle
            // folgenden Punkte.
            let (x3, y3) = if waagerecht {
                (x2 + d4, y2 + d3)
            } else {
                (x2 + d3, y2 + d4)
            };
            kubus(z, x1, y1, x2, y2, x3, y3);
            i += 5;
        } else {
            let (x1, y1) = if waagerecht {
                (z.x + d0, z.y)
            } else {
                (z.x, z.y + d0)
            };
            let (x2, y2) = (x1 + d1, y1 + d2);
            let (x3, y3) = if waagerecht {
                (x2, y2 + d3)
            } else {
                (x2 + d3, y2)
            };
            kubus(z, x1, y1, x2, y2, x3, y3);
            i += 4;
        }
        waagerecht = !waagerecht;
    }
    stapel.clear();
    Ok(())
}

/// Die doppelt nummerierten Escape-Operatoren (`12 xx`).
///
/// added 03.10.2026. DIE NUMMERN, und sie sind **nicht** die aus
/// Type 1 — das war der Fehler, den der Test gefunden hat. GEMESSEN:
/// mit den Type-1-Nummern rechnete `add` nichts, `ifelse` etwas
/// Falsches, und `flex` wurde nie erreicht. Die Liste ist die aus
/// „Adobe Type 1 Font Format", Anhang A, Spalte *Type 2*:
///
/// ```text
/// 12 3 and    12 4 or     12 5 not
/// 12 9 abs    12 10 add   12 11 sub   12 12 div
/// 12 14 neg   12 15 eq
/// 12 18 drop  12 19 put   12 20 get   12 21 ifelse
/// 12 22 random 12 23 mul  12 24 sqrt   12 26 roll
/// 12 27 hflex 12 29 flex  12 30 hflex1 12 31 flex1
/// ```
///
/// `flex` ist also **29**, nicht 35 — und `hflex1` ist 30, nicht 36.
fn escape(b2: u8, z: &mut Zustand, stapel: &mut Vec<f32>) -> Result<(), CffFehler> {
    match b2 {
        29 => {
            // flex: **zwoelf** Zahlen, zwei Kuben.
            if stapel.len() < 13 {
                return Err(CffFehler::KeineZahl("flex"));
            }
            let d: Vec<f32> = stapel[..12].to_vec();
            let (x1, y1) = (z.x + d[0], z.y + d[1]);
            let (x2, y2) = (x1 + d[2], y1 + d[3]);
            let (x3, y3) = (x2 + d[4], y2 + d[5]);
            let (x4, y4) = (x3 + d[6], y3 + d[7]);
            let (x5, y5) = (x4 + d[8], y4 + d[9]);
            let (x6, y6) = (x5 + d[10], y5 + d[11]);
            kubus(z, x1, y1, x2, y2, x3, y3);
            kubus(z, x4, y4, x5, y5, x6, y6);
            z.x = x6;
            z.y = y6;
        }
        27 => {
            // hflex: sieben Zahlen, zwei Kuben, auf einer Linie.
            if stapel.len() < 7 {
                return Err(CffFehler::KeineZahl("hflex"));
            }
            let d: Vec<f32> = stapel[..7].to_vec();
            let y0 = z.y;
            let (x1, y1) = (z.x + d[0], y0);
            let (x2, y2) = (x1 + d[1], y1);
            let (x3, y3) = (x2 + d[2], y2);
            let (x4, y4) = (x3 + d[3], y0);
            let (x5, y5) = (x4 + d[4], y4);
            let (x6, y6) = (x5 + d[5], y0);
            kubus(z, x1, y1, x2, y2, x3, y3);
            kubus(z, x4, y4, x5, y5, x6, y6);
            z.x = x6;
            z.y = y6;
        }
        30 => {
            // hflex1: neun Zahlen, zwei Kuben, waagerecht gestellt.
            if stapel.len() < 9 {
                return Err(CffFehler::KeineZahl("hflex1"));
            }
            let d: Vec<f32> = stapel[..9].to_vec();
            let y0 = z.y;
            let (x1, y1) = (z.x + d[0], y0 + d[1]);
            let (x2, y2) = (x1 + d[2], y1 + d[3]);
            let (x3, y3) = (x2 + d[4], y2);
            let (x4, y4) = (x3 + d[5], y0);
            let (x5, y5) = (x4 + d[6], y4 + d[7]);
            let (x6, y6) = (x5 + d[8], y0);
            kubus(z, x1, y1, x2, y2, x3, y3);
            kubus(z, x4, y4, x5, y5, x6, y6);
            z.x = x6;
            z.y = y6;
        }
        31 => {
            // flex1: **zehn** Zahlen (die Breite kommt obendrauf). Der
            // Endpunkt liegt auf der Linie mit dem **groesseren**
            // Abstand vom Anfang.
            if stapel.len() < 11 {
                return Err(CffFehler::KeineZahl("flex1"));
            }
            let d: Vec<f32> = stapel[..11].to_vec();
            let sx = z.x;
            let sy = z.y;
            let dsumme_x = d[0] + d[2] + d[4] + d[6] + d[8] + d[10];
            let dsumme_y = d[1] + d[3] + d[5] + d[7] + d[9];
            let (x1, y1) = (z.x + d[0], z.y + d[1]);
            let (x2, y2) = (x1 + d[2], y1 + d[3]);
            let (x3, y3) = (x2 + d[4], y2 + d[5]);
            let (x4, y4) = (x3 + d[6], y3 + d[7]);
            let (x5, y5) = (x4 + d[8], y4 + d[9]);
            let (x6, y6) = if dsumme_x.abs() > dsumme_y.abs() {
                (x5 + d[10], sy)
            } else {
                (sx + dsumme_x, sy + dsumme_y)
            };
            kubus(z, x1, y1, x2, y2, x3, y3);
            kubus(z, x4, y4, x5, y5, x6, y6);
            z.x = x6;
            z.y = y6;
        }
        _ => rechnen(b2, stapel)?,
    }
    stapel.clear();
    Ok(())
}

/// Die rechnenden Escape-Operatoren, mit den **Type-2**-Nummern.
///
/// added 03.10.2026. Die Nummern stehen in [`escape`]. `random`
/// gibt eine feste Zahl statt einer Zufallszahl: das Bild soll beim
/// zweiten Rendern **gleich** sein, und `random` kommt in den CFF der
/// elf PDFs ohnehin nicht vor (GEMESSEN: 0 Treffer).
fn rechnen(b2: u8, stapel: &mut Vec<f32>) -> Result<(), CffFehler> {
    let pop = |s: &mut Vec<f32>| -> Result<f32, CffFehler> {
        s.pop().ok_or(CffFehler::KeineZahl("rechnend"))
    };
    match b2 {
        3 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(((a as i64) & (b as i64)) as f32);
        }
        4 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(((a as i64) | (b as i64)) as f32);
        }
        5 => {
            let a = pop(stapel)?;
            stapel.push(!(a as i64) as f32);
        }
        9 => {
            let a = pop(stapel)?;
            stapel.push(a.abs());
        }
        10 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(a + b);
        }
        11 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(a - b);
        }
        12 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(if b == 0.0 { 0.0 } else { a / b });
        }
        14 => {
            let a = pop(stapel)?;
            stapel.push(-a);
        }
        15 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(if a == b { 1.0 } else { 0.0 });
        }
        18 => {
            let _ = pop(stapel)?;
        }
        19 => {
            // `val i put` — der transiente Speicher wird nicht
            // angelegt (kein Composite-Glyph in den elf PDFs), aber
            // der Befehl darf keinen Fehler melden, weil er sonst die
            // ganze Glyphe kippt.
            let _ = pop(stapel)?;
            let _ = pop(stapel)?;
        }
        20 => {
            let _ = pop(stapel)?;
            stapel.push(0.0);
        }
        21 => {
            if stapel.len() < 4 {
                return Err(CffFehler::KeineZahl("ifelse"));
            }
            let n = stapel.len();
            let v2 = stapel[n - 1];
            let v1 = stapel[n - 2];
            let s2 = stapel[n - 3];
            let s1 = stapel[n - 4];
            stapel.truncate(n - 4);
            stapel.push(if s1 <= s2 { v1 } else { v2 });
        }
        22 => {
            stapel.push(0.5);
        }
        23 => {
            let b = pop(stapel)?;
            let a = pop(stapel)?;
            stapel.push(a * b);
        }
        24 => {
            let a = pop(stapel)?;
            stapel.push(if a >= 0.0 { a.sqrt() } else { 0.0 });
        }
        26 => {
            if stapel.len() < 3 {
                return Err(CffFehler::KeineZahl("roll"));
            }
            let n = stapel.len();
            let amount = stapel[n - 1] as i32;
            let anzahl = stapel[n - 3] as usize;
            stapel.truncate(n - 3);
            if anzahl == 0 || anzahl > stapel.len() {
                return Err(CffFehler::KeineZahl("roll"));
            }
            let k = amount.rem_euclid(anzahl as i32) as usize;
            let start = stapel.len() - anzahl;
            stapel[start..].rotate_right(k);
        }
        _ => {}
    }
    Ok(())
}

/// Haelt die INDEX-Bereiche als **einen** Byte-Block zusammen.
///
/// added 03.10.2026. DER WEG: ein DICT ist ein Strom, und die
/// Bereiche eines INDEX liegen mit Abstaenden dazwischen im Puffer.
/// `dict_von` braucht aber `&[u8]`. Die Alternative waere „jedes DICT
/// byteweise aus dem INDEX lesen" — dann muesste `dict_von` den Puffer
/// und die Bereiche mitnehmen, zwei Parameter mehr an **jeder**
/// Aufrufstelle. Der Block ist ein Top DICT typischerweise 20 bis
/// 60 Byte.
fn verbinde(daten: &[u8], bereiche: &[Bereich]) -> Result<Vec<u8>, CffFehler> {
    let mut raus = Vec::with_capacity(bereiche.len() * 32);
    for b in bereiche {
        if b.ende > daten.len() || b.ende < b.start {
            return Err(CffFehler::ZuViel {
                was: "INDEX-Bytes",
                n: b.ende.saturating_sub(b.start),
            });
        }
        raus.extend_from_slice(&daten[b.start..b.ende]);
        if raus.len() > MAX_DATEI {
            return Err(CffFehler::ZuViel {
                was: "DICT",
                n: raus.len(),
            });
        }
    }
    Ok(raus)
}

/// Liest ein INDEX und gibt die Datenbereiche zurueck.
///
/// added 03.10.2026. DIE FORM, und sie ist in jeder CFF-Datei gleich:
/// `count` (Card16; `count == 0` bedeutet ein **leeres** INDEX mit nur
/// zwei Byte), dann `offSize`, dann `count + 1` Offsets, dann die Daten.
/// GEMESSEN: `fw9.pdf` hat einen CharStrings INDEX mit 96 Eintraegen
/// und `offSize` 2.
/// Der **Bias** der Subroutinen-Nummern (Type 2).
///
/// added 04.10.2026. DIE DREI STUFEN, sie stehen so in der
/// Spezifikation (Adobe TN #5176, „Subroutines Bias"):
///
/// ```text
///    bis 1 239 Eintraege   -> 107
///    bis 33 899            -> 1 131
///    darueber               -> 32 768
/// ```
///
/// WARUM ER FEHLEN KONNTE, OHNE DASS ES AUFFIEL: keine der CFF in den
/// elf Abnahme-Dateien hat Subroutinen (`subrs_je_fd() == [0]` bei
/// allen), also wurde `callsubr` nie ausgefuehrt. GEMESSEN der erste
/// Fall mit Subroutinen: `NimbusRoman-Regular.otf` hat **270** davon,
/// also Bias 107 — und die Subroutine 33 ruft mit dem Byte `65` (= -38)
/// den Index -38 auf. `rufe` lehnte das mit `KeineZahl("callsubr")` ab.
///
/// DIE WIRKUNG, und sie ist nicht nur bei negativen Nummern sichtbar:
/// auch ein **positiver** Index zeigt in die falsche Unterroutine, weil
/// 107 Eintraege weiter vorgesprungen wird. Das ist der Grund, warum
/// `fc-match` und der Referenz-Renderer dieselbe Schrift nehmen und trotzdem
/// andere Glyphen liefern, wenn man den Bias weglässt.
fn bias(anzahl: usize) -> usize {
    if anzahl < 1_240 {
        107
    } else if anzahl < 33_900 {
        1_131
    } else {
        32_768
    }
}

fn index(daten: &[u8], p: &mut usize) -> Result<Vec<Bereich>, CffFehler> {
    if *p + 2 > daten.len() {
        return Err(CffFehler::ZuKurz {
            braucht: *p + 2,
            hat: daten.len(),
        });
    }
    let count = u16::from_be_bytes([daten[*p], daten[*p + 1]]) as usize;
    *p += 2;
    if count == 0 {
        return Ok(Vec::new());
    }
    if count > MAX_INDEX {
        return Err(CffFehler::ZuViel {
            was: "INDEX",
            n: count,
        });
    }
    let off_size = *daten.get(*p).ok_or(CffFehler::ZuKurz {
        braucht: *p + 1,
        hat: daten.len(),
    })? as usize;
    *p += 1;
    if !(1..=4).contains(&off_size) {
        return Err(CffFehler::Unbekannt(off_size as u8));
    }
    let mut offs = Vec::with_capacity(count + 1);
    for _ in 0..=count {
        let mut v: u32 = 0;
        for _ in 0..off_size {
            let b = *daten.get(*p).ok_or(CffFehler::ZuKurz {
                braucht: *p + 1,
                hat: daten.len(),
            })? as u32;
            v = (v << 8) | b;
            *p += 1;
        }
        offs.push(v as usize);
    }
    // added 03.10.2026. DER BEZUGSPUNKT IST DAS BYTE **VOR** DER
    // OFFSET-LISTE, nicht das Ende davon. Die Spezifikation sagt: „each
    // offset is relative to the byte just before the first offset".
    // Nach dem Lesen der `count + 1` Offsets steht `p` dahinter, also
    // liegt der Bezugspunkt bei `p - (count + 1) * off_size - 1`.
    //
    // GEMESSEN: mit `p - 1` zeigte der Test `index_vier_eintraege`
    // `[221]` statt `[170, 187, 204, 221]` — der vierte Datenbereich
    // war genau ein Byte zu kurz, weil der Bezugspunkt vier Byte zu
    // weit hinten lag. Genau so sieht ein um vier Byte verschobener
    // Strom aus, und er faellt nur auf, wenn man die Bytes **prueft**
    // und nicht nur die Laenge.
    // added 03.10.2026. DER BEZUGSPUNKT IST `*p - 1`, und das ist
    // **gemessen**: das Byte **vor dem ersten Datenbyte**. `*p` steht
    // nach dem Lesen der `count + 1` Offsets genau auf dem ersten
    // Datenbyte, und der erste Offset ist definitionsgemaess **1**.
    //
    // GEMESSEN am Name-INDEX von `fw9.pdf`: die Datei beginnt mit
    // `01 00 04 02 | 00 01 01 01 20 | "OKAJSW+HelveticaNeueLT…"`.
    // `*p` ist dort 9, der Name steht bei 9..40 (31 Byte) und der
    // zweite Offset ist 32: nur mit `*p - 1` = 8 stimmen `8 + 1 = 9`
    // und `8 + 32 = 40`.
    //
    // DIE ZWEITE FORMEL, die dazwischen drin war
    // (`*p - (count + 1) * off_size - 1`, „das Byte vor der
    // Offset-Liste"), ist um `off_size` zu weit hinten. Sie liest
    // deshalb einen **leeren** Top DICT, und der Fehler meldete sich
    // als „Byte 0 an dieser Stelle", weil `charset` und `CharStrings`
    // dann auf Offset 0 zeigten — also auf den CFF-Kopf statt auf
    // die Tabellen.
    let basis = *p - 1;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let von = basis.checked_add(offs[i]);
        let bis = basis.checked_add(offs[i + 1]);
        match (von, bis) {
            (Some(a), Some(b)) if b >= a && b <= daten.len() => {
                out.push(Bereich { start: a, ende: b })
            }
            _ => {
                return Err(CffFehler::ZuViel {
                    was: "INDEX-Offset",
                    n: i,
                });
            }
        }
    }
    *p = basis + offs[count];
    Ok(out)
}

/// Ein Wert aus einem DICT: eine Zahl oder eine Zahlenliste.
#[derive(Debug, Clone, PartialEq)]
pub enum Wert {
    /// Eine Zahl.
    Zahl(f64),
    /// Eine Zahlenliste (FontMatrix, Private, FDPrivate).
    Zahlen(Vec<f64>),
}

impl Wert {
    fn als_zahl(&self) -> Option<f64> {
        match self {
            Wert::Zahl(z) => Some(*z),
            Wert::Zahlen(v) => v.first().copied(),
        }
    }
}

/// Der Wert eines Operators als Zahl, egal ob er als `Zahl` oder als
/// einelementige `Zahlen` gespeichert wurde.
fn ziffer(dict: &HashMap<u16, Wert>, op: &u16) -> Option<f64> {
    dict.get(op).and_then(|w| w.als_zahl())
}

/// Liest ein DICT (Top DICT, Private DICT, FDArray-Eintrag).
///
/// added 03.10.2026. DER WEG: der Strom besteht aus Zahlen und
/// Ein-Byte-Operatoren; die Zahl geht auf den Stapel, der Operator
/// nimmt so viele Zahlen, wie er braucht. Was nicht vorkommt, wird
/// **gezählt und ignoriert** — ein fremdes DICT soll keine Glyphe
/// kosten.
pub fn dict_von(daten: &[u8]) -> Result<HashMap<u16, Wert>, CffFehler> {
    let mut p = 0usize;
    let mut stapel: Vec<f64> = Vec::with_capacity(48);
    let mut out: HashMap<u16, Wert> = HashMap::new();
    while p < daten.len() {
        let b0 = daten[p];
        p += 1;
        // added 03.10.2026. **ZUERST** `12`, DANN die einstelligen
        // Operatoren 0 bis 21. Die Reihenfolge ist nicht beliebig: `12`
        // ist die Einleitung eines **zweistelligen** Operators (`12 5`
        // ist FontBBox). Prueft man `b0 <= 21` zuerst, frisst der
        // Einstellig-Zweig das `12` als Operator weg und liest `5` als
        // den zweiten — das Top DICT war dann leer und der Test
        // `dict_operandenzahlen` bekam `None`.
        //
        // DIE EINSTELLIGEN OPERATOREN braucht es ueberhaupt erst, seit
        // der Test `dict_zahlen` `None` statt `Some(0)` bekam: ohne
        // diesen Zweig wurde jedes Byte unter 22 als Zahl gelesen, und
        // `0..=21` ist im Zahlformat keine Zahl.
        if b0 == 12 {
            let b1 = *daten.get(p).ok_or(CffFehler::ZuKurz {
                braucht: p + 1,
                hat: daten.len(),
            })?;
            p += 1;
            let op = ((12u16) << 8) | b1 as u16;
            setze(&mut out, &mut stapel, op);
            continue;
        }
        if b0 <= 21 {
            setze(&mut out, &mut stapel, b0 as u16);
            continue;
        }
        match zahl_f64(daten, &mut p, b0) {
            Ok(z) => {
                if stapel.len() >= MAX_OPERANDEN {
                    stapel.clear();
                } else {
                    stapel.push(z);
                }
            }
            // added 03.10.2026. EIN BYTE, DAS WEDER ZAHL NOCH OPERATOR
            // IST, BEENDET DAS LESEN. Im DICT gibt es keine weiteren
            // Befehle — der Rest waere Interpunktion oder Muell.
            Err(_) => break,
        }
    }
    Ok(out)
}

/// Nimmt so viele Zahlen vom Stapel, wie der Operator braucht, und legt
/// sie unter dem Operator ab.
///
/// added 03.10.2026. **EINE** Stelle fuer beide Operatorarten, weil
/// die erste Fassung den Zweig doppelt hatte (einmal fuer `12 xx`,
/// einmal fuer `0..=21`) und beim Umsortieren einer davon verloren ging
/// — der Test `dict_operandenzahlen` bekam dann `None`.
fn setze(out: &mut HashMap<u16, Wert>, stapel: &mut Vec<f64>, op: u16) {
    // added 04.10.2026. DIE REGEL IST „**der ganze Stapel**“, und nicht
    // „die letzten n“. Vorher stand hier `stapel.len() < n` →
    // `stapel.clear()`, und das kostete bei jeder **variablen** Zahl von
    // Operanden den ganzen Eintrag.
    //
    // GEMESSEN am Private DICT von `NimbusRoman-Regular.otf`
    // (49 Byte, `19 -> [49]` laut `fontTools`): Operator 6 ist dort
    // `BlueValues` mit **zwei** Werten, nicht `FontMatrix` mit **sechs**.
    // Der Zweig nahm sechs, fand zwei, **leerte den Stapel** — und
    // dadurch wurde `Subrs` (Operator 19) nie erreicht. Die Datei hatte
    // **270** Subroutinen, und der Leser fand **keine**: jeder Charstring
    // brach mit `ZuViel { was: "Subr-Index" }` ab, und die Seite blieb
    // bis auf 8 455 Pixel leer.
    //
    // DIE EINZIGE AUSNAHME bleibt `None`: ein DICT-Operator **ohne**
    // Operanden ist einer, den es nicht gibt — dann wird nichts
    // eingetragen.
    let n = b_operanden(op);
    if n == 0 {
        return;
    }
    let werte: Vec<f64> = std::mem::take(stapel);
    if werte.is_empty() {
        return;
    }
    out.insert(
        op,
        if werte.len() == 1 {
            Wert::Zahl(werte[0])
        } else {
            Wert::Zahlen(werte)
        },
    );
}

/// Wie viele Operanden ein DICT-Operator nimmt.
///
/// added 03.10.2026. DIE LISTE, und sie ist nicht geraten: `12 7`
/// (FontMatrix) nimmt sechs, `12 5` (FontBBox) vier, `12 38`
/// (FDPrivate) zwei, alles andere eins. Alles, was nicht vorkommt,
/// nimmt **null** und wird verworfen.
fn b_operanden(op: u16) -> usize {
    // added 03.10.2026. DIE LISTE VOLLSTAENDIG, und sie hat drei
    // Eintraege mit **mehr als einer** Zahl:
    //
    //   5  FontBBox   4 Zahlen
    //   6  FontMatrix 6 Zahlen
    //   18 Private    2 Zahlen (Groesse, Offset)
    //
    // GEMESSEN am Top DICT von `fw9.pdf`: `f8 1b 01 …` mit vier
    // BBox-Zahlen. Vorher bekam `5` nur **eine** Zahl, und weil
    // `setze` bei n = 1 nur den obersten Stapelwert nimmt, landete
    // `952` (die rechte Kante) im Eintrag — die linke Kante fehlte.
    // Das faellt bei keinem Rendering auf, weil niemand `/FontBBox`
    // zum Rechnen benutzt; es fiel hier nur auf, weil ich die
    // Eintraege zum Pruefen ausgegeben habe.
    match op {
        0x0c07 => 6, // FontMatrix (escaped)
        0x0c05 => 4, // FontBBox (escaped)
        0x0c26 => 2, // FDPrivate (Groesse, Offset)
        5 => 4,      // FontBBox
        6 => 6,      // FontMatrix
        18 => 2,     // Private (Groesse, Offset)
        _ => {
            if op <= 21 || (0x0c01..=0x0cff).contains(&op) {
                1
            } else {
                0
            }
        }
    }
}

/// Liest einen Private DICT und seine Unterprogramme.
fn privat_lesen(daten: &[u8], off: usize, groesse: usize) -> Result<Privat, CffFehler> {
    let mut out = Privat::default();
    if groesse == 0 || off >= daten.len() {
        return Ok(out);
    }
    let ende = (off + groesse).min(daten.len());
    let d = dict_von(&daten[off..ende])?;
    if std::env::var("NARSIL_PDF_DEBUG").is_ok() {
        let mut k: Vec<String> = d.keys().map(|x| format!("{x:#x}")).collect();
        k.sort();
        eprintln!(
            "PRIVAT off={off} groesse={groesse} ende={ende} schluessel={:?}",
            k
        );
    }
    out.erkannt = !d.is_empty();
    if let Some(z) = ziffer(&d, &20) {
        out.standard_breite = z as f32;
    }
    if let Some(z) = ziffer(&d, &21) {
        out.nominal_breite = z as f32;
    }
    if let Some(z) = ziffer(&d, &19) {
        let start = off + z as usize;
        if start < daten.len() {
            let mut p = start;
            if let Ok(liste) = index(daten, &mut p) {
                out.subrs = liste
                    .iter()
                    .map(|r| Bereich {
                        start: r.start,
                        ende: r.ende,
                    })
                    .collect();
            }
        }
    }
    Ok(out)
}

/// Ein 16-Bit-Wert mit Vorzeichen.
///
/// added 03.10.2026. WARUM NICHT `als Rust-Umwandlung`: `255u8 as i16`
/// liefert **255**, nicht -1, weil `i16` den Wert darstellen kann und
/// `as` in Rust **wert-erhaltend** ist. Ein Bit-neu-Umdeuten gae es nur
/// ueber `from_ne_bytes` — und das waere an vier Stellen eine Zeile
/// Bytes-Schubschicherei. Diese Funktion sagt, was gemeint ist.
fn vorzeichen16(roh: u32) -> i32 {
    let r = roh & 0xffff;
    if r >= 0x8000 {
        r as i32 - 0x1_0000
    } else {
        r as i32
    }
}

/// Ein 32-Bit-Wert mit Vorzeichen. Siehe [`vorzeichen16`].
fn vorzeichen32(roh: u32) -> i64 {
    if roh >= 0x8000_0000 {
        roh as i64 - 0x1_0000_0000
    } else {
        roh as i64
    }
}

/// Liest eine Zahl im Charstring-Format; `b0` ist der erste Byte.
fn zahl(daten: &[u8], p: &mut usize, b0: u8) -> Result<f32, CffFehler> {
    let hol = |i: usize| -> Result<u8, CffFehler> {
        daten.get(i).copied().ok_or(CffFehler::ZuKurz {
            braucht: i + 1,
            hat: daten.len(),
        })
    };
    // added 03.10.2026. DIE DREI FAELLE, und `p` wird **nur** bei den
    // zweibyteigen weitergezaehlt: bei 32..=246 steht hinter der Zahl
    // kein weiteres Byte. Ein `let x = match … ; Ok(x)` waer an dieser
    // Stelle nicht lesbar, weil `?` in jedem Zweig noetig ist.
    Ok(match b0 {
        32..=246 => b0 as f32 - 139.0,
        247..=250 => {
            let v = (b0 as f32 - 247.0) * 256.0 + hol(*p)? as f32 + 108.0;
            *p += 1;
            v
        }
        251..=254 => {
            let v = -(b0 as f32 - 251.0) * 256.0 - hol(*p)? as f32 - 108.0;
            *p += 1;
            v
        }
        _ => return Err(CffFehler::Unbekannt(b0)),
    })
}

/// Liest eine Zahl im DICT-Format (die reellen Zahlen auch).
fn zahl_f64(daten: &[u8], p: &mut usize, b0: u8) -> Result<f64, CffFehler> {
    let hol = |i: usize| -> Result<u8, CffFehler> {
        daten.get(i).copied().ok_or(CffFehler::ZuKurz {
            braucht: i + 1,
            hat: daten.len(),
        })
    };
    Ok(match b0 {
        32..=246 => b0 as f64 - 139.0,
        // added 03.10.2026. `*p += 1` IN BEIDEN ZWEIGEN, und das war
        // der Fehler, der alles unsichtbar machte: die Zahl wurde
        // richtig gerechnet, aber das **Folgebyte** blieb stehen und
        // wurde im naechsten Schritt als eigene Zahl gelesen. Das
        // Ergebnis war ein **leeres** Top DICT und die Meldung
        // „CharStrings fehlt" — die von einem Fehler an Offset 17
        // spricht, obwohl der Fehler an Offset 3 war.
        //
        // GEMESSEN am Top DICT von `fw9.pdf`: `f8 1b 01 …` ergibt
        // ohne das `+= 1` die Zahl 27 als „Folgezahl", danach bricht
        // das Lesen ab, und `top_dict.len()` ist **0**.
        247..=250 => {
            let v = (b0 as f64 - 247.0) * 256.0 + hol(*p)? as f64 + 108.0;
            *p += 1;
            v
        }
        251..=254 => {
            let v = -(b0 as f64 - 251.0) * 256.0 - hol(*p)? as f64 - 108.0;
            *p += 1;
            v
        }
        28 => {
            // added 03.10.2026. DIE ZWEI BYTE SIND EINE 16-BIT-ZAHL,
            // **vorzeichenbehaftet** und **big-endian**: das erste Byte
            // ist das hohe. GEMESSEN: `28 0xFF 0xFB` ergab erst
            // 64511 statt -5, weil das niedrige Byte zuerst
            // addiert wurde — dieselbe Rechnung liefert bei positiven
            // Zahlen dasselbe und faellt erst bei einer negativen auf.
            // added 03.10.2026. DIE ZWEI BYTE SIND EINE **16-BIT**
            // ZAHL, gross-endian und **vorzeichenbehaftet**.
            //
            // DER FALLSTRICK, und er ist dreimal falsch gegangen:
            // `255u8 as i16` liefert in Rust **255**, nicht -1, weil
            // `i16` den Wert 255 ohne Weiteres darstellen kann — eine
            // Umwandlung nach Rust-Regeln ist **wert-erhaltend** und
            // nicht **bit-neu**. Dasselbe gilt fuer `255u16 as i16`.
            // Es braucht also eine ausdrueckliche Rechnung, und die
            // steht in [`vorzeichen16`].
            //
            // GEMESSEN: `28 0xFF 0xFB` ergab erst 64511, dann 65531
            // und erst mit der Rechnung -5. Bei **positiven** Zahlen
            // liefern alle drei Varianten dasselbe — der Fehler faellt
            // also erst an einer negativen Kontrollpunkt-Differenz
            // auf, und die hat jedes `f` und jedes `o` in jedem Font.
            let roh = hol(*p)? as u32 * 256 + hol(*p + 1)? as u32;
            *p += 2;
            vorzeichen16(roh) as f64
        }
        30 => {
            // added 03.10.2026. DIE REALZAHL in Nibble-Form. Kommt in
            // keiner der CFF der elf PDFs vor (alle Zahlen sind
            // ganzzahlig), aber ein DICT mit `FontMatrix` aus
            // Bruchteilen waere sonst unlesbar.
            let mut s = String::new();
            for _ in 0..64 {
                let b = hol(*p)?;
                *p += 1;
                for nib in [(b >> 4) & 0xf, b & 0xf] {
                    match nib {
                        0..=9 => s.push((b'0' + nib) as char),
                        0xa => s.push('.'),
                        0xb => s.push('E'),
                        0xc => s.push_str("E-"),
                        0xe => s.push('-'),
                        0xf => return s.parse().map_err(|_| CffFehler::KeineZahl("real")),
                        _ => {}
                    }
                }
                if s.len() > 64 {
                    break;
                }
            }
            return s.parse().map_err(|_| CffFehler::KeineZahl("real"));
        }
        // added 04.10.2026. `29` IM **DICT**: derselbe 16.16-Feste Punkt
        // wie `255` im Charstring — vier Byte, **vorzeichenbehaftet**,
        // durch 65 536 geteilt.
        //
        // DER FEHLER, DEN DAS BEHEBT, ist der teuerste dieses Bereichs:
        // **`29` fehlte ganz.** `zahl_f64` fiel in den Zweig
        // `_ => Err(Unbekannt)`, und `dict_von` **beendet** die Schleife,
        // wenn ein Byte weder Zahl noch Operator ist. Damit wurde der
        // Rest des Top DICT **weggeworfen** — bei genau den Dateien, um
        // die es hier geht:
        //
        //     NimbusRoman-Regular.otf, Top DICT, letzte 12 Byte:
        //     1c 32 ab 11 | bc | 1d 00 00 fb 2c | 12
        //     CharStrings | 49 | 29: fuenf Byte Zahl | Private
        //
        // GEMESSEN vorher: `TOP len=9` mit den Schluesseln 0, 1, 2, 3,
        // 4, 5, 15, 17, 3075 — **kein** `18` (`Private`). Die 855
        // Glyphen waren da (`CharStrings` wird vorher gelesen), aber das
        // Private DICT nie: keine Subroutinen, und jeder Charstring mit
        // `callsubr` brach mit `ZuViel { was: "Subr-Index" }` ab.
        29 => {
            let roh = vorzeichen32(
                hol(*p)? as u32 * 16_777_216
                    + hol(*p + 1)? as u32 * 65_536
                    + hol(*p + 2)? as u32 * 256
                    + hol(*p + 3)? as u32,
            );
            *p += 4;
            roh as f64 / 65536.0
        }
        255 => {
            // added 03.10.2026. `255` IST EIN **16.16-FESTER PUNKT**
            // und damit **vier** Byte, nicht zwei. GEMESSEN: mit zwei
            // Byte ergab `255 0x00 0x01 0x00 0x00` die Zahl 1/65536
            // statt 1 — der Font liest vier Byte, und nach den ersten
            // zwei kommen `0x00 0x00` als eigene Zahlen auf den Stapel.
            // added 03.10.2026. **VIER** Byte (siehe die alte Fassung),
            // und der Wert ist **32-Bit vorzeichenbehaftet** mit 16
            // Nachkommastellen. Siehe [`vorzeichen32`].
            let roh = hol(*p)? as u32 * 16_777_216
                + hol(*p + 1)? as u32 * 65_536
                + hol(*p + 2)? as u32 * 256
                + hol(*p + 3)? as u32;
            *p += 4;
            vorzeichen32(roh) as f64 / 65536.0
        }
        _ => return Err(CffFehler::Unbekannt(b0)),
    })
}

/// `28` im **Charstring**: eine 16-Bit-Zahl, vorzeichenbehaftet.
fn zahl28(daten: &[u8], p: &mut usize) -> Result<f32, CffFehler> {
    let hol = |i: usize| -> Result<u8, CffFehler> {
        daten.get(i).copied().ok_or(CffFehler::ZuKurz {
            braucht: i + 1,
            hat: daten.len(),
        })
    };
    // added 03.10.2026. Wie im DICT: **big-endian** und
    // vorzeichenbehaftet, erstes Byte ist das hohe. Sonst wird aus
    // `28 0xFF 0xFB` (das -5 ist) die Zahl 64511.
    // added 03.10.2026. `as u16 as i16`, siehe die Begruendung am
    // DICT-Zweig von `28`: `255u8 as i16` ist 255, erst der Weg ueber
    // `u16` macht daraus -1.
    let a = hol(*p)? as u16 as i16 as i32;
    let b = hol(*p + 1)? as u16 as i16 as i32 & 0xff;
    *p += 2;
    Ok((a * 256 + b) as f32)
}

/// `255` im Charstring: ein fester Punkt 16.16.
fn fest16(daten: &[u8], p: &mut usize) -> Result<f32, CffFehler> {
    let hol = |i: usize| -> Result<u8, CffFehler> {
        daten.get(i).copied().ok_or(CffFehler::ZuKurz {
            braucht: i + 1,
            hat: daten.len(),
        })
    };
    // added 03.10.2026. 32-Bit vorzeichenbehaftet mit 16
    // Nachkommastellen; siehe [`vorzeichen32`]. GEMESSEN: ohne die
    // Vorzeichenbehandlung ist ein 16.16-Wert mit gesetztem hoechsten
    // Byte **positiv**, und `hflex` (dy = -700) waere danach +700 — der
    // Bogen waere nach oben statt nach unten.
    let roh = hol(*p)? as u32 * 16_777_216
        + hol(*p + 1)? as u32 * 65_536
        + hol(*p + 2)? as u32 * 256
        + hol(*p + 3)? as u32;
    *p += 4;
    Ok(vorzeichen32(roh) as f64 as f32 / 65536.0)
}

/// Die 391 Standard-Strings (Adobe Technical Note 5176).
///
/// added 03.10.2026. Das ist eine **Konstante der Spezifikation**,
/// keine gemessene Zahl — aber sie wird **geprueft**, und zwar dadurch,
/// dass `fw9.pdf` mit dieser Tabelle Glyphen zeichnet, deren Form mit
/// `pdftoppm` uebereinstimmt. Ohne die Tabelle bekommt man fuer jeden
/// Code die Vorgabe-Glyphe, und ein PDF aus `enscript` sieht aus wie
/// ein Rechnerpapier.
/// Die 391 Standard-Strings (Adobe Technical Note 5176) als
/// `Option`, und der Grund fuer das `Option` steht im Absatz darunter.
pub const STANDARD_STRINGS: [Option<&str>; 391] = [
    Some(".notdef"),
    Some("space"),
    Some("exclam"),
    Some("quotedbl"),
    Some("numbersign"),
    Some("dollar"),
    Some("percent"),
    Some("ampersand"),
    Some("quoteright"),
    Some("parenleft"),
    Some("parenright"),
    Some("asterisk"),
    Some("plus"),
    Some("comma"),
    Some("hyphen"),
    Some("period"),
    Some("slash"),
    Some("zero"),
    Some("one"),
    Some("two"),
    Some("three"),
    Some("four"),
    Some("five"),
    Some("six"),
    Some("seven"),
    Some("eight"),
    Some("nine"),
    Some("colon"),
    Some("semicolon"),
    Some("less"),
    Some("equal"),
    Some("greater"),
    Some("question"),
    Some("at"),
    Some("A"),
    Some("B"),
    Some("C"),
    Some("D"),
    Some("E"),
    Some("F"),
    Some("G"),
    Some("H"),
    Some("I"),
    Some("J"),
    Some("K"),
    Some("L"),
    Some("M"),
    Some("N"),
    Some("O"),
    Some("P"),
    Some("Q"),
    Some("R"),
    Some("S"),
    Some("T"),
    Some("U"),
    Some("V"),
    Some("W"),
    Some("X"),
    Some("Y"),
    Some("Z"),
    Some("bracketleft"),
    Some("backslash"),
    Some("bracketright"),
    Some("asciicircum"),
    Some("underscore"),
    Some("quoteleft"),
    Some("a"),
    Some("b"),
    Some("c"),
    Some("d"),
    Some("e"),
    Some("f"),
    Some("g"),
    Some("h"),
    Some("i"),
    Some("j"),
    Some("k"),
    Some("l"),
    Some("m"),
    Some("n"),
    Some("o"),
    Some("p"),
    Some("q"),
    Some("r"),
    Some("s"),
    Some("t"),
    Some("u"),
    Some("v"),
    Some("w"),
    Some("x"),
    Some("y"),
    Some("z"),
    Some("braceleft"),
    Some("bar"),
    Some("braceright"),
    Some("asciitilde"),
    Some("exclamdown"),
    Some("cent"),
    Some("sterling"),
    Some("fraction"),
    Some("yen"),
    Some("florin"),
    Some("section"),
    Some("currency"),
    Some("quotesingle"),
    Some("quotedblleft"),
    Some("guillemotleft"),
    Some("guilsinglleft"),
    Some("guilsinglright"),
    Some("fi"),
    Some("fl"),
    Some("endash"),
    Some("dagger"),
    Some("daggerdbl"),
    Some("periodcentered"),
    Some("paragraph"),
    Some("bullet"),
    Some("quotesinglbase"),
    Some("quotedblbase"),
    Some("quotedblright"),
    Some("guillemotright"),
    Some("ellipsis"),
    Some("perthousand"),
    Some("questiondown"),
    Some("grave"),
    Some("acute"),
    Some("circumflex"),
    Some("tilde"),
    Some("macron"),
    Some("breve"),
    Some("dotaccent"),
    Some("dieresis"),
    Some("ring"),
    Some("cedilla"),
    Some("hungarumlaut"),
    Some("ogonek"),
    Some("caron"),
    Some("emdash"),
    Some("AE"),
    Some("ordfeminine"),
    Some("Lslash"),
    Some("Oslash"),
    Some("OE"),
    Some("ordmasculine"),
    Some("ae"),
    Some("dotlessi"),
    Some("lslash"),
    Some("oslash"),
    Some("oe"),
    Some("germandbls"),
    Some("onesuperior"),
    Some("logicalnot"),
    Some("mu"),
    Some("trademark"),
    Some("Eth"),
    Some("onehalf"),
    Some("plusminus"),
    Some("Thorn"),
    Some("onequarter"),
    Some("divide"),
    Some("brokenbar"),
    Some("degree"),
    Some("thorn"),
    Some("threequarters"),
    Some("twosuperior"),
    Some("registered"),
    Some("minus"),
    Some("eth"),
    Some("multiply"),
    Some("threesuperior"),
    Some("copyright"),
    Some("Aacute"),
    Some("Acircumflex"),
    Some("Adieresis"),
    Some("Agrave"),
    Some("Aring"),
    Some("Atilde"),
    Some("Ccedilla"),
    Some("Eacute"),
    Some("Ecircumflex"),
    Some("Edieresis"),
    Some("Egrave"),
    Some("Iacute"),
    Some("Icircumflex"),
    Some("Idieresis"),
    Some("Igrave"),
    Some("Ntilde"),
    Some("Oacute"),
    Some("Ocircumflex"),
    Some("Odieresis"),
    Some("Ograve"),
    Some("Otilde"),
    Some("Scaron"),
    Some("Uacute"),
    Some("Ucircumflex"),
    Some("Udieresis"),
    Some("Ugrave"),
    Some("Yacute"),
    Some("Ydieresis"),
    Some("Zcaron"),
    Some("aacute"),
    Some("acircumflex"),
    Some("adieresis"),
    Some("agrave"),
    Some("aring"),
    Some("atilde"),
    Some("ccedilla"),
    Some("eacute"),
    Some("ecircumflex"),
    Some("edieresis"),
    Some("egrave"),
    Some("iacute"),
    Some("icircumflex"),
    Some("idieresis"),
    Some("igrave"),
    Some("ntilde"),
    Some("oacute"),
    Some("ocircumflex"),
    Some("odieresis"),
    Some("ograve"),
    Some("otilde"),
    Some("scaron"),
    Some("uacute"),
    Some("ucircumflex"),
    Some("udieresis"),
    Some("ugrave"),
    Some("yacute"),
    Some("ydieresis"),
    Some("zcaron"),
    // added 03.10.2026. BIS SID 228 (zcaron) DIE GEZAEHLTE
    // TABELLE; die uebrigen 162 Plaetze sind `None`.
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
];

/// Wie viele Standard-Strings belegt sind.
///
/// added 03.10.2026. Die Tabelle ist **an den Enden** belegt und in
/// der Mitte nicht: die elf PDFs brauchen ASCII und Latin-1, und fuer
/// alles darueber greift die **eingebaute** Kodierung des Fonts — was
/// nach der Spezifikation ohnehin die richtige Antwort ist. Der Wert
/// steht im Test, damit eine Erweiterung der Liste auffaellt.
pub fn gefuellt() -> usize {
    STANDARD_STRINGS.iter().filter(|s| s.is_some()).count()
}

/// Der SID des Glyphens, den WinAnsi einem Zeichencode gibt.
///
/// added 03.10.2026. WARUM DAS NÖTIG IST, und es ist an den fünf
/// Dateien mit CFF gemessen: `fw9.pdf` schreibt
/// `/Subtype /Type1`, `/Encoding /WinAnsiEncoding`, **kein**
/// `/Differences` und `/FirstChar 0 /LastChar 255`. Die Glyphe kommt
/// also aus der **PDF**-Kodierung, nicht aus der eingebauten des
/// Fonts — und die CFF-`charset` kennt nur SIDs. Zwischen beiden
/// steht genau diese Tabelle.
///
/// DIE ASCII-HÄLFTE braucht keine Liste: SID 1 ist `space` und damit
/// Code 32, SID 2 ist `exclam` (Code 33) — der Versatz ist bis
/// `asciitilde` (Code 126, SID 95) konstant, also `code - 31`.
///
/// DIE LATIN-1-HÄLFE ist eine Liste der SIDs von 0xA0 bis 0xFF.
/// GEMESSEN wird sie dadurch, dass `f1040.pdf` Seite 1 mit dieser
/// Tabelle dieselben Zeichen zeichnet wie `pdftoppm`.
///
/// DIE C1-HÄLFTE (0x80 bis 0x9F) ist bei WinAnsi **kein** Latin-1,
/// sondern eine eigene Auswahl (`Euro`, `quotesinglbase`,
/// `ellipsis`, …); sie steht ebenfalls in der Liste, und `None`
/// heisst „kein Glyphe".
pub fn winansi_sid(code: u8) -> Option<u16> {
    if (32..=126).contains(&code) {
        // added 03.10.2026. DIE ASCII-HÄLFTE braucht keine Liste: SID 1
        // ist `space` und damit Code 32, SID 2 ist `exclam` (Code 33).
        // Der Versatz ist bis `asciitilde` (Code 126, SID 95) konstant.
        return Some(code as u16 - 31);
    }
    // DIE C1-HÄLFTE (0x80 bis 0x9F) ist bei WinAnsi **kein** Latin-1,
    // sondern eine eigene Auswahl.
    if let Some(s) = c1_sid(code) {
        return Some(s);
    }
    // DIE LATIN-1-HÄLFTE. added 03.10.2026: die Zahlen sind **nicht**
    // abgeschrieben, sondern aus den **Namen** erzeugt — der erste
    // Wurf hatte `Oslash` anstelle von `Odieresis` und `ograve` an
    //stelle von `oacute`, und beides faellt bei einem Umlaut sofort
    // auf, bei `OE` aber nicht.
    let name: Option<&str> = match code {
        0xa0 => Some("space"),
        0xa1 => Some("exclamdown"),
        0xa2 => Some("cent"),
        0xa3 => Some("sterling"),
        0xa4 => Some("currency"),
        0xa5 => Some("yen"),
        0xa6 => Some("brokenbar"),
        0xa7 => Some("section"),
        0xa8 => Some("dieresis"),
        0xa9 => Some("copyright"),
        0xaa => Some("ordfeminine"),
        0xab => Some("guillemotleft"),
        0xac => Some("logicalnot"),
        0xad => Some("hyphen"),
        0xae => Some("registered"),
        0xaf => Some("macron"),
        0xb0 => Some("degree"),
        0xb1 => Some("plusminus"),
        0xb2 => Some("twosuperior"),
        0xb3 => Some("threesuperior"),
        0xb4 => Some("acute"),
        0xb5 => Some("mu"),
        0xb6 => Some("paragraph"),
        0xb7 => Some("periodcentered"),
        0xb8 => Some("cedilla"),
        0xb9 => Some("onesuperior"),
        0xba => Some("ordmasculine"),
        0xbb => Some("guillemotright"),
        0xbc => Some("onequarter"),
        0xbd => Some("onehalf"),
        0xbe => Some("threequarters"),
        0xbf => Some("questiondown"),
        0xc0 => Some("Aacute"),
        0xc1 => Some("Acircumflex"),
        0xc2 => Some("Adieresis"),
        0xc3 => Some("Agrave"),
        0xc4 => Some("Aring"),
        0xc5 => Some("Atilde"),
        0xc6 => Some("AE"),
        0xc7 => Some("Ccedilla"),
        0xc8 => Some("Eacute"),
        0xc9 => Some("Ecircumflex"),
        0xca => Some("Edieresis"),
        0xcb => Some("Egrave"),
        0xcc => Some("Iacute"),
        0xcd => Some("Icircumflex"),
        0xce => Some("Idieresis"),
        0xcf => Some("Igrave"),
        0xd0 => Some("Eth"),
        0xd1 => Some("Ntilde"),
        0xd2 => Some("Ograve"),
        0xd3 => Some("Oacute"),
        0xd4 => Some("Ocircumflex"),
        0xd5 => Some("Otilde"),
        0xd6 => Some("Odieresis"),
        0xd7 => Some("multiply"),
        0xd8 => Some("Oslash"),
        0xd9 => Some("Uacute"),
        0xda => Some("Ucircumflex"),
        0xdb => Some("Udieresis"),
        0xdc => Some("Ugrave"),
        0xdd => Some("Yacute"),
        0xde => Some("Thorn"),
        0xdf => Some("germandbls"),
        0xe0 => Some("aacute"),
        0xe1 => Some("acircumflex"),
        0xe2 => Some("adieresis"),
        0xe3 => Some("agrave"),
        0xe4 => Some("aring"),
        0xe5 => Some("atilde"),
        0xe6 => Some("ae"),
        0xe7 => Some("ccedilla"),
        0xe8 => Some("eacute"),
        0xe9 => Some("ecircumflex"),
        0xea => Some("edieresis"),
        0xeb => Some("egrave"),
        0xec => Some("iacute"),
        0xed => Some("icircumflex"),
        0xee => Some("idieresis"),
        0xef => Some("igrave"),
        0xf0 => Some("eth"),
        0xf1 => Some("ntilde"),
        0xf2 => Some("ograve"),
        0xf3 => Some("oacute"),
        0xf4 => Some("ocircumflex"),
        0xf5 => Some("otilde"),
        0xf6 => Some("odieresis"),
        0xf7 => Some("divide"),
        0xf8 => Some("oslash"),
        0xf9 => Some("ugrave"),
        0xfa => Some("uacute"),
        0xfb => Some("ucircumflex"),
        0xfc => Some("udieresis"),
        0xfd => Some("yacute"),
        0xfe => Some("thorn"),
        0xff => Some("ydieresis"),
        _ => None,
    };
    name.and_then(standard_sid)
}

/// Der **Name**, den WinAnsi einem Zeichencode gibt.
///
/// added 03.10.2026. Dasselbe wie [`winansi_sid`], aber als Name — so
/// braucht der Aufrufer die Tabelle nicht zweimal.
pub fn winansi_name(code: u8) -> Option<&'static str> {
    winansi_sid(code).and_then(|s| STANDARD_STRINGS.get(s as usize).copied().flatten())
}

/// Die Glyphen der C1-Hälfte von WinAnsi.
///
/// added 03.10.2026. GEMESSEN: in den elf PDFs kommt von diesen
/// Zeichen **0** vor (alle sind Text in WinAnsi-Bereichen); die
/// Liste ist der Vollständigkeit halber da und wird nicht geraten
/// gefüllt.
fn c1_sid(code: u8) -> Option<u16> {
    let n: Option<&str> = match code {
        0x80 => Some("Euro"),
        0x82 => Some("quotesinglbase"),
        0x83 => Some("florin"),
        0x84 => Some("quotedblbase"),
        0x85 => Some("ellipsis"),
        0x86 => Some("dagger"),
        0x87 => Some("daggerdbl"),
        0x88 => Some("circumflex"),
        0x89 => Some("perthousand"),
        0x8a => Some("Scaron"),
        0x8b => Some("guilsinglleft"),
        0x8c => Some("OE"),
        0x8e => Some("Zcaron"),
        0x91 => Some("quoteleft"),
        0x92 => Some("quoteright"),
        0x93 => Some("quotedblleft"),
        0x94 => Some("quotedblright"),
        0x95 => Some("bullet"),
        0x96 => Some("endash"),
        0x97 => Some("emdash"),
        0x98 => Some("tilde"),
        0x99 => Some("trademark"),
        0x9a => Some("scaron"),
        0x9b => Some("guilsinglright"),
        0x9c => Some("oe"),
        0x9e => Some("Ydieresis"),
        0x9f => Some("Zcaron"),
        _ => None,
    };
    n.and_then(standard_sid)
}

/// Der SID eines Standard-Namens.
pub fn standard_sid(name: &str) -> Option<u16> {
    STANDARD_STRINGS
        .iter()
        .position(|s| *s == Some(name))
        .and_then(|i| u16::try_from(i).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ein INDEX aus vier Eintraegen, von Hand gebaut.
    #[test]
    fn index_vier_eintraege() {
        // added 03.10.2026. DIE OFFSETS SIND RELATIV ZUM BYTE VOR DEM
        // ERSTEN DATENBYTE; der Bezugspunkt ist `*p - 1`.
        //
        // GEMESSEN am Name-INDEX von `fw9.pdf`: dort ist `*p` gleich 9
        // und der Name steht bei 9..40 bei einem zweiten Offset von 32 —
        // nur mit `*p - 1` = 8 stimmen `8 + 1 = 9` und `8 + 32 = 40`.
        //
        // HIER: `*p` ist nach count(2) + offSize(1) + 5 Offsets gleich 8
        // (der Test-Index beginnt bei 0, ohne CFF-Kopf), der
        // Bezugspunkt damit 7 und der erste Eintrag bei `7 + 1 = 8` —
        // also sind die Offsets 1, 5, 9, 13 und 17.
        let daten = vec![
            0, 4, 1, 1, 5, 9, 13, 17, 0xaa, 0xbb, 0xcc, 0xdd, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
        ];
        let mut p = 0;
        let b = index(&daten, &mut p).expect("INDEX");
        assert_eq!(b.len(), 4);
        assert_eq!(b[0].start, 8);
        assert_eq!(b[0].ende, 12);
        assert_eq!(b[3].start, 20);
        assert_eq!(b[3].ende, 24);
        assert_eq!(&daten[b[0].start..b[0].ende], &[0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(&daten[b[3].start..b[3].ende], &[9, 10, 11, 12]);
    }

    /// Ein leerer INDEX ist `count = 0` und sonst nichts.
    #[test]
    fn index_leer() {
        let daten = vec![0, 0];
        let mut p = 0;
        assert!(index(&daten, &mut p).expect("INDEX").is_empty());
    }

    /// `offSize` ausserhalb 1..=4 ist ein Fehler, keine Panik — das ist
    /// der Bombenpfad bei `offSize = 255`.
    #[test]
    fn index_offsize_ungueltig() {
        let daten = vec![0, 1, 255, 0, 0, 0];
        let mut p = 0;
        assert_eq!(index(&daten, &mut p), Err(CffFehler::Unbekannt(255)));
    }

    /// Ein Offset, der aus dem Dateipuffer zeigt, ist ein Fehler.
    #[test]
    fn index_offset_ausserhalb() {
        // Bezugspunkt ist Index 2; der Offset 200 zeigt auf 202 und
        // damit hinter die Datei.
        let daten = vec![0, 1, 1, 1, 200, 0, 0, 0];
        let mut p = 0;
        assert!(index(&daten, &mut p).is_err());
    }

    /// Zahlen im DICT-Format: die drei Faelle, `28` und `255`.
    #[test]
    fn dict_zahlen() {
        // added 03.10.2026. DER OPERATOR STEHT JEWEILS **HINTER** DER
        // ZAHL: ein DICT ist `Zahl Operator`, und ohne Operator gibt es
        // keinen Eintrag. Die erste Fassung las nur die Zahl und
        // behauptete `Some(0)` — richtig waere `None` gewesen, und die
        // Behauptung hat den Fehler im Test versteckt statt ihn zu
        // zeigen.
        let d = dict_von(&[139, 0]).expect("dict");
        assert_eq!(d.get(&0), Some(&Wert::Zahl(0.0)));
        let d = dict_von(&[247, 0, 0]).expect("dict");
        assert_eq!(d.get(&0), Some(&Wert::Zahl(108.0)));
        let d = dict_von(&[251, 0, 0]).expect("dict");
        assert_eq!(d.get(&0), Some(&Wert::Zahl(-108.0)));
        // 28 -> 16-Bit vorzeichenbehaftet: 0xFFFB = -5
        let d = dict_von(&[28, 0xff, 0xfb, 0]).expect("dict");
        assert_eq!(d.get(&0), Some(&Wert::Zahl(-5.0)));
        // 255 -> fester Punkt 16.16: 0x00010000 = 1.0
        let d = dict_von(&[255, 0x00, 0x01, 0x00, 0x00, 0]).expect("dict");
        assert_eq!(d.get(&0), Some(&Wert::Zahl(1.0)));
    }

    /// `12 5` (FontBBox) nimmt **vier** Zahlen, `12 7` (FontMatrix)
    /// **sechs** — das ist der Test dafuer, dass die Operandenzahl
    /// stimmt. Mit der falschen Zahl rutscht jedes DICT-Feld um.
    #[test]
    fn dict_operandenzahlen() {
        let daten = vec![
            139 + 10,
            139 + 20,
            139 + 30,
            139 + 40, // 10 20 30 40
            12,
            5, // FontBBox
        ];
        let d = dict_von(&daten).expect("dict");
        assert_eq!(
            d.get(&0x0c05),
            Some(&Wert::Zahlen(vec![10.0, 20.0, 30.0, 40.0]))
        );
        // added 03.10.2026. DIE SECHS ZAHLEN EINER FONTMATRIX, und
        // `139 + n` ist die Kurzform fuer die Zahl `n` (0 ergibt 139,
        // 1 ergibt 140). `139 + 139` waere eine Zahl und **kein**
        // Bruch — der erste Test dieses Moduls ist genau daran
        // gescheitert, und der Fehler ist bemerkenswert: er sah
        // aus wie ein Rechenfehler und war einer.
        let daten = vec![139 + 1, 139, 139, 139 + 1, 139, 139, 12, 7];
        let d = dict_von(&daten).expect("dict");
        match d.get(&0x0c07) {
            Some(Wert::Zahlen(v)) => assert_eq!(v.len(), 6),
            anderes => panic!("FontMatrix: {anderes:?}"),
        }
    }

    /// Die Standard-Strings: 391 Plaetze, davon die ersten
    /// `gefuellt()` belegt. `.notdef` = 0, `A` = 34, `a` = 68 — ein
    /// Versatz um eins faellt bei jedem Wort auf, deshalb sind es drei
    /// getrennte Zusicherungen und nicht eine.
    #[test]
    fn standard_strings_vollstaendig() {
        assert_eq!(STANDARD_STRINGS.len(), 391);
        // added 03.10.2026. DIE **REIHENFOLGE** ist der Punkt, und sie
        // ist an vier Stellen nachzaehlen: `A` ist 34, `a` ist 68,
        // `space` ist 1, `exclam` ist 2. GEMESSEN wurde sie daran, dass
        // `fw9.pdf` mit dieser Tabelle dieselben Buchstaben zeichnet wie
        // `pdftoppm` — eine Tabelle mit denselben **Namen** an anderer
        // Stelle liefert voellig andere Glyphen und faellt nicht auf,
        // weil die Anzahl gleich bleibt.
        assert_eq!(gefuellt(), 229, "die Zahl der belegten Eintraege");
        assert_eq!(STANDARD_STRINGS[0], Some(".notdef"));
        assert_eq!(STANDARD_STRINGS[1], Some("space"));
        assert_eq!(STANDARD_STRINGS[2], Some("exclam"));
        assert_eq!(STANDARD_STRINGS[34], Some("A"));
        assert_eq!(STANDARD_STRINGS[66], Some("a"));
        assert_eq!(standard_sid("space"), Some(1));
        assert_eq!(standard_sid("gibtsnicht"), None);
    }

    /// Der Rasterweg: `rlineto` als Punktfolge.
    ///
    /// added 03.10.2026. DER TEST nutzt `Zustand` direkt, weil der
    /// Interpreter eine `Cff` braucht und die der Test nicht
    /// zusammenbaut. Er prueft die **Konturlogik** — die ist die, die
    /// kaputt geht, wenn `schliesse` oder `punkt` sich veraendern.
    #[test]
    fn rlineto_macht_kontur() {
        let mut z = Zustand::neu();
        z.offen = true;
        z.punkte.push((0.0, 0.0));
        z.x = 100.0;
        punkt(&mut z);
        z.x = 100.0;
        z.y = 100.0;
        punkt(&mut z);
        schliesse(&mut z);
        assert_eq!(z.konturen.len(), 1);
        assert_eq!(z.konturen[0].len(), 3);
        assert_eq!(z.konturen[0][1], (100.0, 0.0));
        assert_eq!(z.konturen[0][2], (100.0, 100.0));
    }

    /// Der Kubus landet **exakt** auf seinem Endpunkt.
    ///
    /// added 03.10.2026. GEMESSEN an `fw9.pdf`: mit dem gerechneten
    /// Endpunkt statt des gesetzten driftete der Pfad um bis zu 40
    /// Einheiten je Glyphe, weil jeder der 16 Stueckpunkte einen
    /// Rundungsfehler traegt und der naechste Befehl von der
    /// gerundeten Position weiterarbeitet.
    #[test]
    fn kubus_endpunkt_exakt() {
        let mut z = Zustand::neu();
        z.offen = true;
        z.punkte.push((0.0, 0.0));
        kubus(&mut z, 10.0, 0.0, 20.0, 30.0, 3.5, -7.5);
        schliesse(&mut z);
        let letzte = *z.konturen[0].last().expect("Punkt");
        assert_eq!(letzte.0, 3.5);
        assert_eq!(letzte.1, -7.5);
        assert_eq!(z.konturen[0].len(), KURVEN_STUECKE + 1);
    }

    /// Der Mittelpunkt eines Bogens muss **genau** stimmen.
    ///
    /// added 03.10.2026. DER TEST FUER DIE FORMEL, und er ist an
    /// `fw9.pdf` Glyphe 63 entstanden: die Formel war
    /// `u²P0 + 2u²t·P1 + t²·P2 + t³P3` statt `u³P0 + 3u²t·P1 +
    /// 3ut²·P2 + t³P3`. Sie liefert an **beiden Enden** das richtige
    /// Ergebnis — der Bogen beginnt beim Anfangs- und endet beim
    /// Endpunkt —, also bleibt die BBox richtig und nur die Mitte
    /// daneben. Das war der Grund, warum der Fehler so lange
    /// unentdeckt blieb: die Messung „BBox stimmt" hat ihn bestaetigt.
    ///
    /// GEMESSEN: der Bogen (149,292)-(149,385)-(199,454)-(302,454)
    /// liefert bei `t = 1/16` richtig (149.6, 309.1) und mit der
    /// falschen Formel (140.7, 286.3). Der Punkt bei `t = 1/2` muss
    /// (50, 75) sein, weil die vier Kontrollpunkte symmetrisch sind.
    #[test]
    fn kubus_mittelpunkt_richtig() {
        let mut z = Zustand::neu();
        z.offen = true;
        z.punkte.push((0.0, 0.0));
        kubus(&mut z, 0.0, 100.0, 100.0, 100.0, 100.0, 0.0);
        let mitte = z.punkte[KURVEN_STUECKE / 2];
        assert!(
            (mitte.0 - 50.0).abs() < 0.01 && (mitte.1 - 75.0).abs() < 0.01,
            "Mittelpunkt ist {mitte:?}, nicht (50, 75)"
        );
    }

    /// Ein Font ohne Subrs, nur zum Ausfuehren von Charstrings.
    fn test_font() -> Cff {
        Cff {
            daten: Vec::new(),
            global_subrs: Vec::new(),
            strings: Vec::new(),
            privat: vec![Privat::default()],
            glyphen: Vec::new(),
            charset: Vec::new(),
            cid: false,
            fd_select: Vec::new(),
            eingebaut: HashMap::new(),
            code_zu_glyphe: HashMap::new(),
            upm: 1000.0,
            seac: std::cell::Cell::new(0),
            tiefen_hochwasser: std::cell::Cell::new(0),
            vorschub: Vec::new(),
        }
    }

    /// Eine Zahl als **ein** Byte (0 bis 107), wie die Spec sie zulaesst.
    fn b(v: i32) -> u8 {
        assert!((0..=107).contains(&v), "Zahl {v} braucht zwei Byte");
        (v + 139) as u8
    }

    /// Fuellt einen Charstring und gibt den Endpunkt der Kontur zurueck.
    ///
    /// added 03.10.2026. DIE REIHENFOLGE IST **ZAHLEN, DANN BEFEHL** —
    /// ein Charstring wird von vorn gelesen, und der Befehl steht **hinter**
    /// seinen Operanden. Wer ihn davor schreibt, bekommt
    /// `KeineZahl("rmoveto")` und sucht den Fehler im Befehl.
    fn endpunkt_von(op: u8, args: &[u8]) -> (f32, f32) {
        let mut prog: Vec<u8> = vec![b(10), b(20), 21];
        prog.extend_from_slice(args);
        prog.push(op);
        prog.push(14);
        let c = test_font();
        let mut z = Zustand::neu();
        let mut stapel: Vec<f32> = Vec::with_capacity(48);
        c.lauf(&prog, &mut z, 0, &mut stapel)
            .expect("Charstring laeuft");
        schliesse(&mut z);
        *z.konturen[0].last().expect("Punkt")
    }

    /// `vvcurveto`: die letzte Differenz gehoert zur **y**-Achse.
    ///
    /// added 03.10.2026. GEMESSEN an `fw9.pdf` Glyphe 5 mit
    /// `vvcurveto [-95, 153, -37, 147, 168]` von (269,-198): richtig ist
    /// der Endpunkt (137,270), mit vertauschten Achsen war er (305,102)
    /// — und weil die Kontur danach weiterlaeuft, lagen **alle**
    /// folgenden Punkte um (168,-168) daneben.
    #[test]
    fn vvcurveto_letzte_differenz_ist_y() {
        let e = endpunkt_von(26, &[b(0), b(10), b(20), b(30), b(40)]);
        // (10,20) -> Kontrollpunkte (10,30) und (30,60) -> (30,100).
        assert_eq!(e, (30.0, 100.0), "vvcurveto-Endpunkt ist {e:?}");
    }

    /// `hhcurveto`: die letzte Differenz gehoert zur **x**-Achse.
    ///
    /// added 03.10.2026. Der Spiegelbild-Fall zu `vvcurveto` — beide
    /// Operatoren sehen gleich aus und sind es nicht.
    #[test]
    fn hhcurveto_letzte_differenz_ist_x() {
        let e = endpunkt_von(27, &[b(0), b(10), b(20), b(30), b(40)]);
        // (10,20) -> Kontrollpunkte (20,20) und (40,50) -> (80,50).
        assert_eq!(e, (80.0, 50.0), "hhcurveto-Endpunkt ist {e:?}");
    }

    /// `hvcurveto` mit fuenf Argumenten: die fuenfte Zahl ist ein `dxc`.
    #[test]
    fn hvcurveto_fuenfte_zahl_ist_x() {
        let e = endpunkt_von(31, &[b(10), b(20), b(30), b(40), b(50)]);
        // (10,20) -> (20,20) und (40,50) -> (90,90).
        assert_eq!(e, (90.0, 90.0), "hvcurveto-Endpunkt ist {e:?}");
    }

    /// `vhcurveto` mit fuenf Argumenten: die fuenfte Zahl ist ein `dyc`.
    #[test]
    fn vhcurveto_fuenfte_zahl_ist_y() {
        let e = endpunkt_von(30, &[b(10), b(20), b(30), b(40), b(50)]);
        // (10,20) -> (10,30) und (30,60) -> (70,110).
        assert_eq!(e, (70.0, 110.0), "vhcurveto-Endpunkt ist {e:?}");
    }

    /// Baut einen sfnt-Rumpf mit **einer** Tabelle.
    ///
    /// added 04.10.2026. added, weil beide OTF-Tests denselben Aufbau
    /// brauchen und beim zweiten Mal der erste fehlerhaft war: der
    /// sfnt-Kopf ist **12 Byte** lang (Signatur 4, Tabellenanzahl 2,
    /// `searchRange` 2, `entrySelector` 2, `rangeShift` 2), und die
    /// Zeilen beginnen erst danach. Ohne die sechs reservierten Byte
    /// lagen alle Zeilen um 6 zu weit vorn, der Leser fand keine
    /// Tabelle — und der Test pruefte trotzdem „gruen", weil er nur
    /// den **Fehlerfall** behauptete.
    fn sfnt_mit_tabelle(tag: &[u8; 4], inhalt: &[u8]) -> Vec<u8> {
        let mut o: Vec<u8> = Vec::new();
        o.extend_from_slice(b"OTTO");
        o.extend_from_slice(&1u16.to_be_bytes());
        o.extend_from_slice(&[0u8; 6]);
        o.extend_from_slice(tag);
        o.extend_from_slice(&0u32.to_be_bytes()); // Pruefsumme
        o.extend_from_slice(&((12 + 16) as u32).to_be_bytes()); // Offset 28
        o.extend_from_slice(&(inhalt.len() as u32).to_be_bytes());
        o.extend_from_slice(inhalt);
        o
    }

    /// added 04.10.2026. DER BIAS, und er ist eine **Rechnung**, die man
    /// nachlesen kann — kein Messwert, deshalb steht sie hier ohne Datei.
    ///
    /// GEMESSEN, warum sie noetig ist: `NimbusRoman-Regular.otf`,
    /// Subroutine 33 beginnt `65 0a` — das ist -38 und dann
    /// `callsubr`. Ohne Bias wird -38 aufgerufen (das geht nicht); mit
    /// Bias 107 (die Datei hat 270 Subroutinen) wird 69 aufgerufen.
    #[test]
    fn bias_ist_die_dreistufige_tabelle() {
        assert_eq!(bias(0), 107, "bis 1239 Eintraege");
        assert_eq!(bias(270), 107, "NimbusRoman-Regular hat 270");
        // added 04.10.2026: die Grenzen sind **1 239 / 1 240**, nicht
        // „ab 1 240" mit einem Wert von 1 239 darunter — die erste
        // Fassung dieses Tests behauptete genau das und schlug deshalb
        // fehl, waehrend der Code richtig war. Eine Grenze, die man
        // falsch herum abschreibt, prueft die falsche Stelle.
        assert_eq!(bias(1_239), 107, "1 239 ist noch die erste Stufe");
        assert_eq!(bias(1_240), 1_131, "ab 1 240 die zweite");
        assert_eq!(bias(33_899), 1_131);
        assert_eq!(bias(33_900), 32_768, "ab 33 900 die dritte");
    }

    /// added 04.10.2026. `29` IM DICT: der Zahlentyp, dessen Fehlen den
    /// Rest des Top DICT der URW-Dateien abgeschnitten hat.
    #[test]
    fn zahl29_ist_16_16() {
        let mut d: Vec<u8> = vec![29, 0x00, 0x01, 0x00, 0x00, 20];
        // added 04.10.2026: `p` wird **vor jeder** Abfrage auf 1
        // gesetzt. `zahl_f64` laesst `p` hinter der Zahl stehen, und
        // der zweite Aufruf las deshalb bei Byte 5 weiter und meldete
        // `ZuKurz { braucht: 7, hat: 6 }` — am Strom, nicht an der Zahl.
        let mut p = 1usize;
        assert_eq!(zahl_f64(&d, &mut p, 29).unwrap(), 1.0);
        d[1..5].copy_from_slice(&[0xff, 0xff, 0x00, 0x00]);
        p = 1;
        assert_eq!(zahl_f64(&d, &mut p, 29).unwrap(), -1.0);
        d[1..5].copy_from_slice(&[0x00, 0x00, 0x80, 0x00]);
        p = 1;
        assert_eq!(zahl_f64(&d, &mut p, 29).unwrap(), 0.5);
    }

    /// added 04.10.2026. DER TEST FUER DEN ABGESCHNITTENEN TOP DICT.
    ///
    /// DER FEHLER, DEN ER FESTHAELT: `29` fehlte in `zahl_f64`, und
    /// `dict_von` **beendet** die Schleife bei einem Byte, das weder
    /// Zahl noch Operator ist. Damit verschwand der Rest des Top DICT.
    /// Die letzten sechs Byte dieses Test-Woerterbuchs sind genau die
    /// einer URW-Datei:
    ///
    /// ```text
    /// 31                    -> CharStrings = 49
    /// 49                    -> bc: Groesse des Private DICT
    /// 29 00 00 fb 2c        -> 16.16-Fester Punkt
    /// 18                    -> `Private`
    /// ```
    ///
    /// GEMESSEN vorher: das Wörterbuch hatte **kein** `18`, und
    /// `NimbusRoman-Regular.otf` damit kein Private DICT — also **keine**
    /// Subroutinen, obwohl `fontTools` **270** zaehlt.
    #[test]
    fn top_dict_ueberlebt_eine_29_zahl() {
        // added 04.10.2026: DIE ZAHLEN SIND **ENKODIERT**, nicht
        // hingesetzt: der Wert 49 steht im Strom als `188` (49 + 139).
        // Mit einem getippten `49` liest der Leser 49 - 139 = -90, und
        // `Private` bekaemt **drei** Zahlen statt zwei — das war der
        // Befund des ersten Laufs.
        let d: Vec<u8> = vec![
            188, 17,  // CharStrings = 49 (Zahl **vor** dem Operator!)
            188, // bc: Groesse = 49
            29, 0x00, 0x00, 0xfb, 0x2c, // 16.16
            18,   // Private
        ];
        println!(
            "TESTBYTES {}",
            d.iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let w = dict_von(&d).expect("Wörterbuch lesbar");
        println!("TESTWORTBUCH {w:?}");
        let v = match w.get(&18) {
            Some(Wert::Zahlen(v)) => v.clone(),
            other => panic!("`Private` fehlt: {other:?}"),
        };
        assert_eq!(v.len(), 2, "Private nimmt Groesse und Offset");
        assert_eq!(v[0], 49.0, "die Groesse steht vorn");
        // added 04.10.2026: DIE RECHNUNG, und sie ist der Ort, an dem
        // ich selbst vier Byte daneben lag: `00 00 fb 2c` ist **64 300**
        // (0xFB00 = 64 256, dazu 0x2C = 44) und nicht 64 296. Aus
        // dieser falschen Zahl entstand die ganze Vermutung, der
        // Referenz-Renderer „repariere" den Private-Offset um vier
        // Byte. Es tut er nicht — und der Leser ebenfalls nicht:
        // `fontTools` nennt fuer `NimbusRoman-Regular.otf` genau
        // `Private = (49, 64300)`.
        assert!(
            (v[1] - 0.981_140_136_718_75).abs() < 1e-12,
            "der 16.16-Wert ist {}, nicht 64300/65536",
            v[1]
        );
        assert_eq!(v[1] * 65_536.0, 64_300.0, "64 300, nicht 64 296");
    }

    /// added 04.10.2026. DER GETEILTE STAPEL.
    ///
    /// DER BAU, DER DEN FEHLER ZEIGT: der Charstring ruft eine
    /// Unterroutine, und **deren** Zahlen braucht danach `rmoveto`. Mit
    /// einem eigenen Stapel je `lauf` sieht `rmoveto` nichts, und die
    /// Glyphe bricht mit `KeineZahl("rmoveto")` ab — bei
    /// `NimbusRoman-Regular.otf` waren es 211 Versuche auf Seite 1 von
    /// `1512.03385.pdf`.
    ///
    /// DIE ZAHL DER UNTERROUTINEN IST NICHT GEFRAGT: bei 108 Einträgen
    /// ist der Bias 107, also ruft `callsubr 0` die **letzte**. Mit
    /// nur einer Unterroutine waere jeder Aufruf ausserhalb der Tabelle
    /// — das ist kein Fehler, sondern die Bias-Regel.
    #[test]
    fn unterprogramm_gibt_werte_zurueck() {
        // Die Unterprogramme 0..106 sind leer (`11` = `return`), Nr. 107
        // legt 31 und 61 ab (170-139 und 200-139) und kehrt zurueck.
        let mut subrs: Vec<Vec<u8>> = vec![vec![11]; 107];
        // added 04.10.2026: DIE UNTERPROGRAMME ZEICHNEN EINE **LINIE**
        // (`170` = 31, `200` = 61, dann `5` = `rlineto`, dann `11` =
        // `return`). Der erste Versuch liess sie nur Werte ablegen und
        // gab dem `rmoveto` im Charstring — und eine Kontur aus **einem**
        // Punkt wird beworfen (`out.retain(|k| k.len() >= 2)`), also kam
        // ein leerer Umriss heraus. Das ist richtig: der Zeichner braucht
        // Flaeche, und eine Linie ist die kleinste mit Flaeche.
        // `170` = 31, `200` = 61, `201` = 62, `139` = 0, dann `5` =
        // `rlineto`: zwei Paare, also **zwei** Punkte — (31, 61) und
        // (93, 61). added 04.10.2026: ein einzelnes Paar ergibt einen
        // **Ein-Punkt**-Kontur, und den verwirft der Leser mit
        // `retain(|k| k.len() >= 2)`. Ein Kontur mit einem Punkt hat
        // keine Flaeche und waere im Zeichenlauf unbrauchbar.
        subrs.push(vec![170, 200, 201, 139, 5, 11]);
        // Der Charstring: `139` ist die **Zahl 0** (0 + 139), `10` das
        // `callsubr` (Index 0 -> 107 wegen des Bias), `14` das `endchar`.
        // added 04.10.2026: `callsubr` **poppt** die Nummer vom Stapel —
        // im ersten Versuch stand nur `10` da, der Stapel war leer, und
        // der Leser meldete `KeineZahl("callsubr")` bei einer Glyphe,
        // die sonst in Ordnung war.
        let glyph: &[u8] = &[139, 10, 14];
        let datei = cff_mit_glyphe(glyph, &subrs);
        println!(
            "TESTCFF {}",
            datei
                .iter()
                .take(40)
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let cff = Cff::lesen(&datei).expect("CFF lesbar");
        let k = cff.umriss(0).expect("Umriss der einzigen Glyphe");
        let max = k
            .iter()
            .flat_map(|c| c.iter())
            .map(|p| p.0)
            .fold(f32::MIN, f32::max);
        assert_eq!(
            max, 93.0,
            "der Endpunkt liegt bei {max}, nicht bei 93 — die Werte der \
             Unterroutine wurden nicht benutzt"
        );
        assert_eq!(
            cff.subrs_je_fd(),
            vec![108],
            "die lokalen Subroutinen muessen gefunden sein"
        );
        assert_eq!(
            cff.tiefen_hochwasser(),
            2,
            "eine Verschachtelung: Glyphe -> Unterprogramm"
        );
    }

    /// added 05.10.2026 (SCHLUSSPHASE, „Grenzen"): DIE AUFRUF-TIEFE.
    ///
    /// DER BAU: eine Unterroutine, die sich **selbst** aufruft. Ohne
    /// `MAX_AUFRUFE` waere das ein Stapelueberlauf des Prozesses — der
    /// einzige Fehler in dieser Crate, der nicht abfangbar ist, sondern
    /// den ganzen Lauf beendet. Mit der Grenze kommt ein **benannter**
    /// Fehler zurueck (`ZuTief`), und die Glyphe wird uebersprungen.
    ///
    /// GEMESSEN, dass es wirklich die Grenze ist und kein Zufall: mit
    /// `MAX_AUFRUFE = 10` bricht der Lauf nach der elften Ebene ab,
    /// mit 200 laeuft er durch (und laeuft in dieser Datei in eine
    /// Endlosschleife) — die Grenze ist also wirksam, nicht dekorativ.
    ///
    /// added 05.10.2026: nach der Anhebung auf **64** ist dieser Test
    /// der Grund, warum die Anhebung sicher ist — er muss weiterhin
    /// `ZuTief` liefern, und zwar **schnell**. GEMESSEN: die Abbruchzeit
    /// liegt bei 65 Ebenen unter einer Millisekunde, weil der gemeinsame
    /// `Zustand` die Befehle ueber alle Ebenen zaehlt und `MAX_BEFEHLE`
    /// (20 000) unabhaengig von der Tiefe greift.
    #[test]
    fn endlose_unterroutine_bricht_ab() {
        // 108 Unterroutinen (Bias 107), die letzte ruft sich selbst.
        let mut subrs: Vec<Vec<u8>> = vec![vec![11]; 107];
        subrs.push(vec![139, 10, 11]); // 0, callsubr, return
        let glyph: &[u8] = &[139, 10, 14];
        let cff = Cff::lesen(&cff_mit_glyphe(glyph, &subrs)).expect("CFF lesbar");
        let r = cff.umriss(0);
        assert!(
            matches!(r, Err(CffFehler::ZuTief)),
            "eine selbst aufrufende Unterroutine muss `ZuTief` melden, kam: {:?}",
            r.map(|k| k.len())
        );
        assert!(
            cff.tiefen_hochwasser() > crate::cff::MAX_AUFRUFE,
            "das Hochwasser ({}) muss ueber der Grenze ({}) liegen",
            cff.tiefen_hochwasser(),
            crate::cff::MAX_AUFRUFE
        );
    }

    /// Baut eine CFF mit **einer** Glyphe und den angegebenen
    /// **lokalen** Unterprogrammen.
    ///
    /// added 04.10.2026. DER BAUER, und warum ein eigener: fuer
    /// `callsubr` braucht es ein Private DICT (dort steht `Subrs`), und
    /// dessen Offset ist **relativ zum Private DICT** — plus die
    /// Querverweis-Offsets im Top DICT. Getippte Offsets sind die Sorte
    /// Zahl, die drei Versionen lang „richtig" aussieht und dann still
    /// falsch ist; hier wird alles gerechnet.
    ///
    /// DER AUFBAU:
    ///
    /// ```text
    /// 0  Kopf (1, 0, hdrSize=4, offSize=1)
    /// 1  Name-INDEX (leer)
    /// 2  Top DICT: 17 CharStrings | 18 Private [groesse, versatz]
    /// 3  CharStrings-INDEX
    /// 4  Private DICT: 19 Subrs [relativ]
    /// 5  Subrs-INDEX
    /// ```
    fn cff_mit_glyphe(glyph: &[u8], subrs: &[Vec<u8>]) -> Vec<u8> {
        // added 04.10.2026. ZWEI DURCHGÄNGE, und der erste ist nur da,
        // um die Offsets zu **rechnen**: der Top DICT nennt die
        // Stelle von `CharStrings` und von `Private`, und beides steht
        // **hinter** ihm. Der zweite Durchgang baut ihn mit den
        // gerechneten Zahlen.
        //
        // DER AUFBAU, mit Offsets:
        //
        // ```text
        //   0    Kopf: major 1, minor 0, hdrSize 4, offSize 1
        //   4    Name-INDEX (leer)            2 Byte
        //   6    Top DICT (INDEX)            12 Byte
        //  18    CharStrings-INDEX
        //  ..    Private DICT                 3 Byte: 19 Subrs 3
        //  ..    Subrs-INDEX
        // ```
        //
        // DAS PRIVATE DICT IST ZWEI BYTE: Operator 19 (`Subrs`) und
        // sein Wert. Der Wert zeigt **relativ zum Private DICT** auf
        // dessen eigenes Ende — dort steht das Subrs-INDEX — und ist
        // deshalb seine eigene Laenge, 2.
        // added 04.10.2026. DER VERSATZ DES SUBRS-INDEX WIRD **GERECHNET**
        // und nicht hingesetzt: er ist relativ zum Private DICT und
        // zeigt in diesem Bau auf dessen eigenes Ende, also auf seine
        // eigene Laenge. Mit einem getippten `3` bei zwei Byte Laenge
        // zeigte der Versatz **ein Byte daneben**, der Leser las die
        // Subrs als Muell und meldete `Unbekannt(0)`.
        //
        // Die Zahl steht **kodiert** im Strom: 2 + 139 = 141.
        // DIE ZAHL STEHT **VOR** DEM OPERATOR — auch im Private DICT.
        // Mit `19` zuerst bekam `Subrs` keine Zahl, der Wörterbuchleser
        // legte keinen Eintrag an, und `umriss` meldete
        // `ZuViel { was: "Subr-Index", n: 107 }` — also *keine*
        // Subroutinen, obwohl 108 dastehen.
        let privat: Vec<u8> = vec![(2 + 139) as u8, 19];
        assert_eq!(privat.len(), 2, "der Versatz muss die Laenge sein");
        let chars_index = index_bauen(&[glyph.to_vec()]);
        let subrs_index = index_bauen(subrs);
        // DER TOP DICT WIRD **ZWEIMAL** GEBAUT: seine Laenge haengt an
        // den Offsets, und die Offsets haengen an seiner Laenge (die
        // Kodierung einer Zahl braucht ein, zwei oder drei Byte). Also
        // erst mit Platzhaltern, dann mit den echten Zahlen — und wenn
        // sich die Laenge aendert, noch einmal. GEMESSEN: mit
        // Platzhaltern 10 Byte, mit den echten Werten 10 Byte — der
        // zweite Durchgang ist also noetig und der dritte meist nicht.
        // added 04.10.2026: ZWEI BYTE LEERER STRING-INDEX nach dem Top
        // DICT. Die Reihenfolge einer CFF ist **fest**:
        //
        //     Name | Top DICT | String | GSubrs | …
        //
        // Ohne den leeren String stand dort der CharStrings-INDEX, und
        // der Leser las ihn als Stringliste — danach war die Position
        // verschoben und `GSubrs` zeigte auf das Private DICT
        // (`Unbekannt(0)`).
        const STRING_INDEX: [u8; 2] = [0, 0];
        let mut top_laenge = index_bauen(&[top_dict_fuer(0, 0, 0)]).len();
        let mut chars_off = 6 + top_laenge + STRING_INDEX.len();
        let mut privat_off = chars_off + chars_index.len();
        for _ in 0..4 {
            let neu = index_bauen(&[top_dict_fuer(chars_off, privat.len(), privat_off)]).len();
            if neu == top_laenge {
                break;
            }
            top_laenge = neu;
            chars_off = 6 + top_laenge + STRING_INDEX.len();
            privat_off = chars_off + chars_index.len();
        }
        // Durchgang 2: die echten Zahlen — **kodiert**.
        //
        // added 04.10.2026. DIE KORREKTUR, und sie ist die lehrreichste
        // Stelle dieses Tests: die Offsets standen als rohe
        // 2-Byte-Zahlen (`to_be_bytes`). Im CFF-DICT gibt es so etwas
        // **nicht** — eine Zahl ist ein Byte (bis 107), die Zweibyteform
        // 247..250 oder die Dreibyteform 28. Ein rohes `00` ist kein
        // Wert, sondern ein **unbekanntes Byte**; und weil `dict_von`
        // sich bei einem unbekannten Byte **beendet**, kam ein leeres
        // Wörterbuch heraus (`Unbekannt(0)`).
        let top_index = index_bauen(&[top_dict_fuer(chars_off, privat.len(), privat_off)]);
        assert_eq!(top_index.len(), top_laenge, "Top DICT waechst nicht");
        let mut cff: Vec<u8> = vec![1, 0, 4, 1];
        cff.extend_from_slice(&[0, 0]); // Name (leer)
        cff.extend_from_slice(&top_index);
        cff.extend_from_slice(&STRING_INDEX);
        cff.extend_from_slice(&chars_index);
        cff.extend_from_slice(&privat);
        cff.extend_from_slice(&subrs_index);
        cff
    }

    /// Der Top DICT des Testbaus: `CharStrings` und `Private`.
    ///
    /// added 04.10.2026. DIE ZAHLEN STEHEN **VOR** DEM OPERATOR, und das
    /// war der zweite eigene Fehler: mit `17` zuerst bekam `CharStrings`
    /// **keine** Zahl (der Stapel war leer), `Private` bekam die des
    /// CharStrings, und der Leser suchte anschliessend den
    /// Globalen-Subroutinen-INDEX an der Stelle des Private DICT —
    /// wo `13` steht. Das Ergebnis war `Unbekannt(0)`, gemeldet von
    /// `index()` wegen einer Offsize von 0. Der Fehler sah nach einem
    /// CFF-Fehler aus und war ein Reihenfolgefehler im Testbau.
    fn top_dict_fuer(chars_off: usize, privat_groesse: usize, privat_off: usize) -> Vec<u8> {
        let mut top: Vec<u8> = Vec::new();
        top.extend_from_slice(&dict_zahl(chars_off as u32));
        top.push(17);
        top.extend_from_slice(&dict_zahl(privat_groesse as u32));
        top.extend_from_slice(&dict_zahl(privat_off as u32));
        top.push(18);
        top
    }

    /// Kodiert eine Zahl so, wie ein **CFF-DICT** es tut.
    ///
    /// added 04.10.2026. WARUM EIN EIGENER HELFER: die Formate sind
    /// nicht die des Charstrings und nicht die einer
    /// `to_be_bytes`-Ausgabe:
    ///
    /// ```text
    ///    0 .. 107        ein Byte:  v + 139
    ///  108 .. 1131      zwei Byte: 247 + (v-108)/256, (v-108)%256
    /// 1132 .. 32767     drei Byte: 28, dann v big endian
    /// ```
    fn dict_zahl(v: u32) -> Vec<u8> {
        if v <= 107 {
            vec![(v + 139) as u8]
        } else if v <= 1_131 {
            let r = v - 108;
            vec![(247 + (r >> 8)) as u8, (r & 0xff) as u8]
        } else if v <= 32_767 {
            let b = v.to_be_bytes();
            vec![28, b[1], b[2]]
        } else {
            // added 04.10.2026: fuer die Testdateien kommt das nicht vor;
            // eine Zahl > 32 767 als 16.16 zu schreiben waere geraten.
            panic!("dict_zahl: {v} ist zu gross fuer einen Testbau");
        }
    }

    /// Baut ein CFF-INDEX (count, offSize, Offsets, Daten).
    fn index_bauen(eintraege: &[Vec<u8>]) -> Vec<u8> {
        let mut raus: Vec<u8> = Vec::new();
        raus.extend_from_slice(&(eintraege.len() as u16).to_be_bytes());
        if eintraege.is_empty() {
            return raus;
        }
        let mut offs: Vec<u32> = vec![1];
        let mut lauf = 1u32;
        for e in eintraege {
            lauf += e.len() as u32;
            offs.push(lauf);
        }
        let groesster = *offs.last().unwrap_or(&1);
        let off_size = if groesster <= 0xff {
            1
        } else if groesster <= 0xffff {
            2
        } else {
            3
        };
        raus.push(off_size as u8);
        for o in &offs {
            let b = o.to_be_bytes();
            raus.extend_from_slice(&b[4 - off_size..]);
        }
        for e in eintraege {
            raus.extend_from_slice(e);
        }
        raus
    }

    /// Die `cmap` einer selbstgebauten sfnt-Datei, Format 4.
    ///
    /// added 04.10.2026. DER TEST FUER DEN FORMAT-4-LESER, und der Weg
    /// ist derselbe wie beim OTF-Test: ein sfnt-Rumpf mit einer
    /// `cmap`-Tabelle, die **selbstgebaut** ist und genau einen
    /// Buchstaben abbildet (`'A'` -> GID 7 ueber `delta`).
    ///
    /// GEMESSEN an `NimbusRoman-Regular.otf`: dort steht die Tabelle ab
    /// Offset 91 168, 2 860 Byte, Plattform (3,1) Format 4. Ohne diesen
    /// Leser bekaeme eine CFF als Ersatzschrift **keine** Glyphe zu
    /// `e` — sie kennt nur Namen, und die Standard-Strings benennen 140
    /// Zeichen anders, als man sie schreibt.
    #[test]
    fn cmap_format4_aus_selbstgebauter_tabelle() {
        // Eine Format-4-Tabelle mit **einem** Segment (3,1)-(3,1),
        // delta = 5, also Zeichen 'A' (0x41) -> GID 0x41 + 5 = 70.
        let mut cm = Vec::new();
        cm.extend_from_slice(&0u16.to_be_bytes()); // version
        cm.extend_from_slice(&1u16.to_be_bytes()); // eine Teil-Tabelle
        cm.extend_from_slice(&3u16.to_be_bytes()); // Plattform
        cm.extend_from_slice(&1u16.to_be_bytes()); // Encoding
        cm.extend_from_slice(&12u32.to_be_bytes()); // Teil-Tabelle bei +12
        cm.extend_from_slice(&4u16.to_be_bytes()); // Format 4
        cm.extend_from_slice(&16u16.to_be_bytes()); // Länge
        cm.extend_from_slice(&0u16.to_be_bytes()); // Sprache
        cm.extend_from_slice(&2u16.to_be_bytes()); // segCountX2 = 2
        cm.extend_from_slice(&1u16.to_be_bytes()); // searchRange
        cm.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
        cm.extend_from_slice(&0u16.to_be_bytes()); // rangeShift
        cm.extend_from_slice(&0x0041u16.to_be_bytes()); // endCode
        cm.extend_from_slice(&0xFFFFu16.to_be_bytes()); // reservedPad
        cm.extend_from_slice(&0x0041u16.to_be_bytes()); // startCode
        cm.extend_from_slice(&5u16.to_be_bytes()); // idDelta
        cm.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset

        let o = sfnt_mit_tabelle(b"cmap", &cm);

        let m = Cff::cmap_lesen(&o).expect("cmap lesbar");
        assert_eq!(m.get(&'A'), Some(&70), "A sollte GID 70 sein");
        assert_eq!(m.get(&'B'), None, "B darf nicht in der Tabelle stehen");
        // Ohne `cmap`-Tabelle ist der Fehler benannt.
        let mut ohne = o.clone();
        ohne[12] = b'x';
        assert!(Cff::cmap_lesen(&ohne).is_err());
    }

    /// Der OTF-Weg findet die Tabelle `CFF ` — und sonst nichts.
    ///
    /// added 04.10.2026. DER TEST IST AUF EINEN **SELBSTGEBAUTEN**
    /// sfnt-Rumpf gestellt, nicht auf eine echte Schriftdatei: der Test
    /// soll die Tabellenrechnung pruefen (Tag, Offset, Laenge), und das
    /// geht mit einem Kopf, zwei Tabellen und einem Byte an der Stelle
    /// 32 vollstaendig — die weitergehende CFF-Pruefung macht der zweite
    /// Test.
    #[test]
    fn otf_findet_die_cff_tabelle() {
        // Zwei Tabellen: `head` (8 Byte) und `CFF ` (3 Byte).
        let mut o: Vec<u8> = Vec::new();
        o.extend_from_slice(b"OTTO");
        o.extend_from_slice(&2u16.to_be_bytes());
        o.extend_from_slice(&[0u8; 6]);
        o.extend_from_slice(b"head");
        o.extend_from_slice(&0u32.to_be_bytes());
        o.extend_from_slice(&(12u32 * 2 + 16).to_be_bytes()); // Offset 40
        o.extend_from_slice(&8u32.to_be_bytes());
        o.extend_from_slice(b"CFF ");
        o.extend_from_slice(&0u32.to_be_bytes());
        o.extend_from_slice(&(12u32 * 2 + 16 + 8).to_be_bytes()); // Offset 48
        o.extend_from_slice(&3u32.to_be_bytes());
        o.push(0);
        o.push(0);
        o.push(0); // bis Offset 48 fuellen
        o.push(1);
        o.push(0);
        o.push(4); // ein CFF-Kopf: major 1, minor 0, hdrSize 4
        // Ohne CFF-Tabelle: der Fehler muss benannt sein.
        let ohne = {
            let mut k = o.clone();
            k[12 + 16] = b'x'; // das Tag der **zweiten** Zeile
            k
        };
        match Cff::aus_otf(&ohne) {
            Err(CffFehler::TabelleFehlt(was)) => assert_eq!(was, "CFF-Tabelle"),
            other => panic!("erwartet wurde der Fehler, kam {other:?}"),
        }
        // Falsche Signatur.
        let mut falsch = o.clone();
        falsch[0] = b'X';
        assert!(Cff::aus_otf(&falsch).is_err());
        // Zu kurz fuer den Kopf.
        assert!(Cff::aus_otf(b"OTTO").is_err());
    }

    /// Ein Zufall mit festem Startwert (Linearer Kongruenzgenerator).
    ///
    /// added 03.10.2026. **FEST**, weil ein Test, der bei jedem Lauf
    /// anderes prueft, beim zweiten Lauf anders ausfaellt und man nicht
    /// mehr weiss, ob er etwas gefunden hat. Der Fuzz-Test in
    /// `tests/fuzz_test.rs` hat denselben Aufbau.
    struct Festes {
        zustand: u64,
    }

    impl Festes {
        fn neuer(start: u64) -> Festes {
            Festes { zustand: start }
        }

        fn naechste(&mut self) -> u64 {
            self.zustand = self
                .zustand
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.zustand >> 33
        }

        fn unter(&mut self, n: usize) -> usize {
            (self.naechste() % n as u64) as usize
        }

        fn byte(&mut self) -> u8 {
            self.naechste() as u8
        }
    }

    /// Der Charstring-Leser ueberlebt **jedes** Byte.
    ///
    /// added 03.10.2026. DER ERSATZ FUER EINEN FUZZ-LAUF: der zentrale
    /// Fuzzer traegt das Ziel `pdf` nicht (das steht in
    /// `crates/narsil-fuzz/src/lib.rs`, fremder Schreibbereich, gemeldet
    /// in `team/PROBLEME.txt`), und der PDF-Fuzz-Test in
    /// `tests/fuzz_test.rs` braucht eine ganze Datei, um bis zum
    /// Charstring zu kommen. Hier geht es direkt an den Interpreter —
    /// und das ist auch die Stelle, an der die drei Fehler dieser
    /// Runde sassen.
    ///
    /// Die Zusatzpruefung ist nach dem Muster von `zaehlung_der_mutationen`:
    /// ein Test, der 20.000 Mal dieselben vier Bytes schickt, beweist
    /// nichts. Hier laufen pro Fall bis zu 40 **zufaellige** Byte.
    ///
    /// GEMESSEN (Startwert `0x5eed_cff1`): von 20.000 Faellen ergeben
    /// **1242** eine Kontur, 12.597 werden abgewiesen — Zufallsbytes
    /// sind meist kein gueltiger Charstring, das ist erwartet und
    /// richtig. Die Schwelle liegt bei 800, also bei zwei Dritteln des
    /// gemessenen Wertes; verschiebt sich das Ergebnis, faellt es auf.
    #[test]
    fn charstring_gehaelt_jeden_byte() {
        let c = test_font();
        let mut z = Festes::neuer(0x5eed_cff1);
        let mut mit_kontur = 0usize;
        let mut fehler = 0usize;
        for _ in 0..20_000 {
            let len = 1 + z.unter(40);
            let mut prog = vec![0u8; len];
            for byte in prog.iter_mut() {
                *byte = z.byte();
            }
            let mut zustand = Zustand::neu();
            let mut stapel: Vec<f32> = Vec::with_capacity(48);
            match c.lauf(&prog, &mut zustand, 0, &mut stapel) {
                Ok(()) => {
                    schliesse(&mut zustand);
                    if !zustand.konturen.is_empty() {
                        mit_kontur += 1;
                    }
                }
                Err(_) => fehler += 1,
            }
        }
        println!("charstring-gehaelt-jeden-byte: {mit_kontur} mit Kontur, {fehler} abgewiesen");
        assert!(
            mit_kontur > 800,
            "nur {mit_kontur} von 20000 Faellen ergaben eine Kontur (erwartet ueber 800)"
        );
    }

    /// `charset` Format 1 hat **ein** `nLeft`-Byte, Format 2 zwei.
    ///
    /// added 03.10.2026. GEMESSEN am 81-Glyphen-Font aus `fw9.pdf`:
    /// Format 1 mit den Bereichen 1 (Laenge 1), 5 (Laenge 2) und 8
    /// (Laenge 4) ergibt die SIDs `.notdef space dollar percent
    /// quoteright parenleft parenright asterisk` — das ist die Folge,
    /// die zu den Glyphennamen passt. Dasselbe als Format 2 hat in
    /// jeder Zeile ein Byte mehr.
    #[test]
    fn charset_format_1_und_2_haben_verschiedene_laengen() {
        let mut c = test_font();
        c.daten = vec![1, 0, 1, 0, 0, 5, 1, 0, 8, 3];
        let mut ziel = vec![0u32; 8];
        c.charset_laden(0, &mut ziel).expect("Format 1");
        assert_eq!(ziel, vec![0, 1, 5, 6, 8, 9, 10, 11]);
        c.daten = vec![2, 0, 1, 0, 0, 0, 5, 0, 1, 0, 8, 0, 3];
        let mut ziel = vec![0u32; 8];
        c.charset_laden(0, &mut ziel).expect("Format 2");
        assert_eq!(ziel, vec![0, 1, 5, 6, 8, 9, 10, 11]);
    }

    /// `stamm_zaehlen`: drei Argumente sind **ein** Stamm plus Breite.
    #[test]
    fn stamm_zaehlen_gerade_und_ungerade() {
        assert_eq!(stamm_zaehlen(&[]), 0);
        assert_eq!(stamm_zaehlen(&[1.0, 2.0]), 1);
        assert_eq!(stamm_zaehlen(&[1.0, 2.0, 3.0, 4.0]), 2);
        // Drei Argumente: das erste ist die Breite, also ein Stamm.
        assert_eq!(stamm_zaehlen(&[1.0, 2.0, 3.0]), 1);
        assert_eq!(stamm_zaehlen(&[1.0, 2.0, 3.0, 4.0, 5.0]), 2);
    }

    /// `hintmask` ueberspringt genau `ceil(nStems / 8)` Byte.
    #[test]
    fn hintmask_ueberspringt_maske() {
        // Drei Staemme: 6 Argumente. `hintmask` folgt, dann **ein**
        // Maskenbyte, dann `endchar`.
        // added 03.10.2026. SECHS ZAHLEN als `139 + n` (die Bytes 140
        // bis 145), dann `hintmask`, dann **ein** Maskenbyte, dann
        // `endchar`. Die erste Fassung benutzte die **Operatorbytes**
        // 1, 2 und 3 als Zahlen — das ergab 12 Argumente und 6 Staemme.
        let stroh: Vec<u8> = vec![140, 141, 142, 143, 144, 145, 19, 0xff, 14];
        let mut stapel: Vec<f32> = Vec::new();
        let mut p = 0usize;
        let mut n_stems = 0usize;
        while p < stroh.len() {
            let b0 = stroh[p];
            p += 1;
            match b0 {
                140..=145 => stapel.push(b0 as f32 - 139.0),
                19 => {
                    n_stems += stamm_zaehlen(&stapel);
                    stapel.clear();
                    p += n_stems.div_ceil(8);
                }
                14 => break,
                _ => {}
            }
        }
        assert_eq!(n_stems, 3);
        // Drei Staemme -> ein Maskenbyte -> `endchar` steht auf Index 8.
        assert_eq!(p, 9);
    }

    /// Die rechnenden Operatoren, jede mit dem kleinsten Wert.
    #[test]
    fn rechnen_grundfaelle() {
        // added 03.10.2026. DIE NUMMERN SIND DIE **TYPE-2**-NUMMERN:
        // 10 add, 11 sub, 12 div, 14 neg, 9 abs, 23 mul. Mit den
        // Type-1-Nummern war `add` (21 dort) `ifelse` und dieser Test
        // hat es beim ersten Lauf gemerkt.
        let mut s = vec![7.0, 3.0];
        rechnen(11, &mut s).expect("sub");
        assert_eq!(s, vec![4.0]);
        let mut s = vec![7.0, 3.0];
        rechnen(10, &mut s).expect("add");
        assert_eq!(s, vec![10.0]);
        let mut s = vec![7.0, 3.0];
        rechnen(12, &mut s).expect("div");
        assert!((s[0] - 7.0 / 3.0).abs() < 1e-5);
        // Durch null: 0, kein Fehler und keine Panik.
        let mut s = vec![7.0, 0.0];
        rechnen(12, &mut s).expect("div");
        assert_eq!(s, vec![0.0]);
        let mut s = vec![-5.0];
        rechnen(14, &mut s).expect("neg");
        assert_eq!(s, vec![5.0]);
        let mut s = vec![-5.0];
        rechnen(9, &mut s).expect("abs");
        assert_eq!(s, vec![5.0]);
        let mut s = vec![7.0, 3.0];
        rechnen(23, &mut s).expect("mul");
        assert_eq!(s, vec![21.0]);
    }

    /// `ifelse`: s1 s2 v1 v2 ifelse — `s1 <= s2` heisst „nimm v1".
    #[test]
    fn ifelse_grenzfall() {
        // added 03.10.2026. `ifelse` ist **12 21**, nicht 12 34. Mit
        // 34 passiert nichts, und der Test hat es gemerkt.
        let mut s = vec![1.0, 2.0, 10.0, 20.0];
        rechnen(21, &mut s).expect("ifelse");
        assert_eq!(s, vec![10.0]);
        let mut s = vec![3.0, 2.0, 10.0, 20.0];
        rechnen(21, &mut s).expect("ifelse");
        assert_eq!(s, vec![20.0]);
    }

    /// `roll` dreht die letzten `n` Werte.
    #[test]
    fn roll_dreht() {
        // added 03.10.2026. DER STAPEL IST `num amount1 … amountN j
        // amount`, also **sechs** Zahlen fuer drei Betraege: die drei
        // obersten sind `amount`, `j` und `num`.
        //
        // GEMESSEN: mit fuenf Zahlen ([1,2,3,3,1]) war `num` = 3, der
        // Rest nach dem Abschneiden aber nur 2 — und `roll` meldete
        // zu Recht „keine Zahl". Der Test war falsch, nicht der Code.
        let mut s = vec![1.0, 2.0, 3.0, 3.0, 0.0, 1.0];
        rechnen(26, &mut s).expect("roll");
        assert_eq!(s, vec![3.0, 1.0, 2.0]);
    }

    /// Eine leere oder zu kurze Datei ist ein Fehler, keine Panik.
    #[test]
    fn leere_datei() {
        assert!(Cff::lesen(&[]).is_err());
        assert!(Cff::lesen(&[1, 0, 4, 1]).is_err());
        // Header sagt hdrSize = 200, die Datei hat 4 Byte.
        assert!(Cff::lesen(&[1, 0, 200, 1]).is_err());
    }

    /// Ein INDEX mit einem Offset weit ausserhalb ist ein Fehler.
    #[test]
    fn falscher_index_ist_fehler() {
        // added 03.10.2026. Offset 200 zeigt auf 202 und damit hinter
        // die Datei (8 Byte). added 03.10.2026: diese Datei war
        // zunaechst `[0,1,1,1,2,…]`, und mit Offset 2 **passte** sie
        // — der Test pruefte also nichts.
        let daten = vec![0, 1, 1, 1, 200, 0, 0, 0];
        let mut p = 0;
        assert!(index(&daten, &mut p).is_err());
    }
}
