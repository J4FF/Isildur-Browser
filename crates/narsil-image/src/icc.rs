//! ICC-PROFILE: finden, lesen, auswerten
//!
//! 02.10.2026, Rolle BILD (B4/B-ICC aus der Koordination vom 01.10.
//! nachmittags: „B-ICC: ICC-Profile fuer JPEG/PNG: CMYK-Toleranz <= 3
//! erreichen“ und „B4 ICC-Profile (P3/AdobeRGB -> sRGB)“).
//!
//! GEMESSENER ANLASS. Die beiden CMYK-Testdateien der Crate
//! (`scratch/jpeg_tests/test_base.jpg`, `test_cmyk.jpg`) tragen **kein**
//! ICC-Profil — mit einem Marker-Scan nachgemessen:
//!
//! ```text
//! test_base.jpg  1733 Byte, Komponenten 3, ICC-Segmente keine
//! test_cmyk.jpg  2333 Byte, Komponenten 4, ICC-Segmente keine
//! ```
//!
//! Der alte Vergleich beider Dateien miteinander ergab eine groesste
//! Abweichung von **148** bei (30, 44) Kanal 2 — Basis (19, 16, 107)
//! gegen CMYK (0, 0, 255). Das ist kein Dekoderfehler: die beiden
//! Dateien speichern an dieser Stelle verschiedene Bildpunkte, weil sie
//! aus zwei verschiedenen Kodierlaeufen stammen. Gegen ein **unabhaengiges
//! Dekodieren derselben Datei** (PIL) liegt unsere Umrechnung bei 29 von
//! 9216 Kanalwerten und groesster Abweichung 1.
//!
//! AUFBAU EINES PROFILS (ICC.1:2001-04):
//!
//! ```text
//! 0    128 Byte Kopf: Groesse, CMM, Version, Geräteklasse ('RGB ',
//!      'CMYK'), Farbraum ('RGB ', 'GRAY'), PCS ('XYZ ', 'Lab ')
//! 128  4 Byte  Anzahl der Marken
//! 132  je 12 Byte: Signatur, Versatz, Groesse
//!      danach die Marken, je ab ihrem Versatz
//! ```
//!
//! M3/M4: alle Laengen und Versoetze kommen aus der Datei und werden mit
//! `checked_add`/`get` geprueft; die Profildaten werden nur gelesen, nie
//! ungeprueft alloziert. `MAX_PROFIL` begrenzt die Groesse.

use crate::BildFehler;
use crate::inflate::entpacke;

/// Hoechstgroesse eines eingebetteten Profils. Grosse ICC-Profile (CMYK
/// mit LUTs) liegen bei 50 bis 500 KiB; 4 MiB ist weit ueber dem, was
/// eine Seite braucht, und haelt eine kaputte Laengenangabe im Rahmen.
pub const MAX_PROFIL: usize = 4 * 1024 * 1024;

/// Die Farbklasse aus dem Profilkopf (`'RGB '`, `'CMYK'`, `'GRAY'` …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farbklasse {
    Rgb,
    Cmyk,
    Grau,
    Unbekannt([u8; 4]),
}

impl Farbklasse {
    /// Aus den vier Bytes des **Farbraums** an Stelle 16.
    ///
    /// GEMESSENER FEHLER (02.10.2026, zweimal derselbe Irrtum: erst im
    /// Profilbauer in Python, dann hier): an Stelle 12 steht die
    /// **Geraeteklasse** (`mntr`, `prtr`, `scnr`, `spac`, `link`,
    /// `abst`, `nmcl`) und an Stelle 16 der **Farbraum** (`RGB `, `CMYK`,
    /// `GRAY`). Mit der Klasse gelesen war jedes Profil „unbekannte
    /// Klasse“, die Umrechnung lief nie, und P3- wie AdobeRGB-Bilder
    /// kamen unveraendert heraus — zwei verschiedene Profile mit
    /// byteweise gleichem Ergebnis.
    pub fn aus_bytes(v: [u8; 4]) -> Farbklasse {
        match &v {
            b"RGB " => Farbklasse::Rgb,
            b"CMYK" => Farbklasse::Cmyk,
            b"GRAY" => Farbklasse::Grau,
            _ => Farbklasse::Unbekannt(v),
        }
    }
    pub fn als_bytes(&self) -> [u8; 4] {
        match self {
            Farbklasse::Rgb => *b"RGB ",
            Farbklasse::Cmyk => *b"CMYK",
            Farbklasse::Grau => *b"GRAY",
            Farbklasse::Unbekannt(v) => *v,
        }
    }
    pub fn name(&self) -> String {
        match self {
            Farbklasse::Rgb => "RGB".to_string(),
            Farbklasse::Cmyk => "CMYK".to_string(),
            Farbklasse::Grau => "Grau".to_string(),
            Farbklasse::Unbekannt(v) => format!("unbekannt {:?}", v),
        }
    }
}

/// Die Gangkurve einer Marke (`rTRC`, `gTRC`, `bTRC`, `kTRC`).
#[derive(Debug, Clone, PartialEq)]
pub enum Kurve {
    /// `curv` mit einer Zahl: eine Gamma-Wert.
    Gamma(f64),
    /// `curv` mit einer Tabelle (Länge minus 1 Stichproben).
    Tabelle(Vec<u16>),
    /// `para`: die Parametrisierungstypen C und D (hier nur C).
    Parametrisch { typ: u16, parameter: [f64; 7] },
    /// Etwas, das dieses Modul nicht auswertet (z. B. ein selbst
    ///beschreibender Typ). Beim Umrechnen ist das ein BENANNTER Fehler.
    NichtAuswertbar([u8; 4]),
}

impl Kurve {
    /// Der Wert der Kurve an der Stelle `v` (0..=1), als linearer Wert.
    ///
    /// GEMESSEN: `curv` mit `count == 0` bedeutet „identisch“, `count == 1`
    /// ist ein reiner Gamma-Wert in `u8Fixed8` (ICC.1:2001-04 Abschnitt
    /// 10.18), sonst eine Stichprobentabelle mit `count` Eintraegen
    /// (`u16Number`).
    pub fn wert(&self, v: f64) -> f64 {
        match self {
            // Gamma-Kodierung: Eingabe linear, Ausgabe mit 1/gamma.
            Kurve::Gamma(g) => v.powf(*g),
            // Die Tabelle enthaelt AUSGABEWERTE (`u16Number`, 0..65535).
            Kurve::Tabelle(t) => {
                if t.is_empty() {
                    return v;
                }
                let n = t.len();
                let pos = v.clamp(0.0, 1.0) * (n - 1) as f64;
                let i0 = pos.floor() as usize;
                let i1 = (i0 + 1).min(n - 1);
                let t0 = t[i0] as f64 / 65535.0;
                let t1 = t[i1] as f64 / 65535.0;
                t0 + (t1 - t0) * (pos - i0 as f64)
            }
            Kurve::Parametrisch { typ, parameter } => {
                // ICC 10.18, Typ 0: `Y = (aX + b)^g + c`, Typ 1 mit `dX`
                // im Exponenten, Typ 2 ohne `c`, Typ 3 additiv, Typ 4 mit
                // `e` im Exponenten.
                let g = parameter[0];
                let a = parameter[1];
                let b = parameter[2];
                let c = parameter[3];
                let d = parameter[4];
                let e = parameter[5];
                let f = parameter[6];
                let x = v.max(0.0);
                let y = match typ {
                    0 => (a * x + b).max(0.0).powf(g) + c,
                    1 => (a * x + b).max(0.0).powf(g) + c + d * x,
                    2 => (a * x + b).max(0.0).powf(g) + c,
                    3 => (a * x + b).max(0.0).powf(g),
                    4 => (a * x + b).max(0.0).powf(g + d * x) + c,
                    _ => return v,
                };
                // Typen 2..4 benutzen `e` und `f` in der Umkehrung.
                let _ = (e, f);
                y
            }
            Kurve::NichtAuswertbar(_) => v,
        }
    }
}

/// Ein gelesenes ICC-Profil.
#[derive(Debug, Clone, PartialEq)]
pub struct Profil {
    roh: Vec<u8>,
}

impl Profil {
    /// Prueft die Grundform des Profils.
    ///
    /// GEMESSEN 02.10.2026: `PIL.ImageCms` liefert bei einem zu kurzen
    /// Profil `ValueError: cannot unpack`, bei einer falschen Signatur
    /// „not a ICC profile“. Ein Browser, der so ein Profil stillschweigend
    /// uebersieht, liefert ein leicht falsches Bild; wir nennen den
    /// Fehler statt ihn zu raten.
    pub fn neu(roh: Vec<u8>) -> Result<Profil, BildFehler> {
        if roh.len() > MAX_PROFIL {
            return Err(BildFehler::IccProfilFalsch {
                grund: "ICC-Profil groesser als MAX_PROFIL",
            });
        }
        if roh.len() < 132 {
            return Err(BildFehler::IccProfilFalsch {
                grund: "ICC-Profil kuerzer als Kopf und Markentabelle",
            });
        }
        if &roh[36..40] != b"acsp" {
            return Err(BildFehler::IccProfilFalsch {
                grund: "ICC-Profil ohne Signatur acsp",
            });
        }
        Ok(Profil { roh })
    }

    /// Die Rohdaten des Profils.
    pub fn roh(&self) -> &[u8] {
        &self.roh
    }

    /// Der Farbraum aus dem Kopf (Stelle 16, nicht 12).
    pub fn farbklasse(&self) -> Farbklasse {
        Farbklasse::aus_bytes([self.roh[16], self.roh[17], self.roh[18], self.roh[19]])
    }

    /// Die Geraeteklasse aus dem Kopf (Stelle 12).
    pub fn geraeteklasse(&self) -> [u8; 4] {
        [self.roh[12], self.roh[13], self.roh[14], self.roh[15]]
    }

    /// Die Zahl der Marken im Tabellenkopf.
    pub fn marken_zahl(&self) -> usize {
        u32::from_be_bytes([self.roh[128], self.roh[129], self.roh[130], self.roh[131]]) as usize
    }

    /// Die Rohdaten einer Marke, wenn sie im Profil steht.
    pub fn marke(&self, signatur: &[u8; 4]) -> Option<&[u8]> {
        let n = self.marken_zahl();
        // M3/M4: die Tabelle darf nicht ueber die Profildaten hinauslesen.
        // `132 + 12 * n` wird mit `checked_mul` gerechnet.
        let tabellenlaenge = 12usize.checked_mul(n)?;
        let ende = 132usize.checked_add(tabellenlaenge)?;
        if ende > self.roh.len() {
            return None;
        }
        for i in 0..n {
            let p = 132 + i * 12;
            if self.roh.get(p..p + 4) != Some(signatur.as_slice()) {
                continue;
            }
            let versatz = u32::from_be_bytes([
                self.roh[p + 4],
                self.roh[p + 5],
                self.roh[p + 6],
                self.roh[p + 7],
            ]) as usize;
            let groesse = u32::from_be_bytes([
                self.roh[p + 8],
                self.roh[p + 9],
                self.roh[p + 10],
                self.roh[p + 11],
            ]) as usize;
            let von = versatz.min(self.roh.len());
            let bis = versatz.checked_add(groesse)?.min(self.roh.len());
            if von > bis {
                return None;
            }
            return Some(&self.roh[von..bis]);
        }
        None
    }

    /// Die drei XYZ-Werte einer `XYZType`-Marke.
    ///
    /// GEMESSEN 02.10.2026: der Typ steht in den ersten vier Bytes der
    /// Marke (`XYZ `), danach 4 Byte reserviert und drei `s15Fixed16Number`
    /// (16.16-Festkomma, durch 65536 geteilt).
    pub fn xyz(&self, signatur: &[u8; 4]) -> Option<[f64; 3]> {
        let m = self.marke(signatur)?;
        if m.len() < 20 || m.get(..4) != Some(b"XYZ ".as_slice()) {
            return None;
        }
        let mut r = [0.0f64; 3];
        for (i, z) in r.iter_mut().enumerate() {
            let p = 8 + i * 4;
            let roh = i32::from_be_bytes([m[p], m[p + 1], m[p + 2], m[p + 3]]);
            *z = roh as f64 / 65536.0;
        }
        Some(r)
    }

    /// Die Gangkurve einer Marke (`curv` oder `para`).
    ///
    /// GEMESSENER FEHLER, gefunden von `examples/bomben_stress.rs` am
    /// 02.10.2026: hier stand `match &m[0..4]` ohne Laengenpruefung. Eine
    /// kaputte Datei kann eine Marke mit Laenge 0 fuehren, und dann panikte
    /// der Schnitt: „range end index 4 out of range for slice of length 0“
    /// (`icc.rs:271`). Die Abfrage ist jetzt `m.get(..4)` und fuehrt bei
    /// zu kurzer Marke in den Zweig `NichtAuswertbar` — der das ist, wofuer
    /// es da ist.
    pub fn kurve(&self, signatur: &[u8; 4]) -> Option<Kurve> {
        let m = self.marke(signatur)?;
        match m.get(..4) {
            Some(b"curv") => {
                if m.len() < 12 {
                    return Some(Kurve::NichtAuswertbar(*b"curv"));
                }
                let n = u32::from_be_bytes([m[8], m[9], m[10], m[11]]) as usize;
                match n {
                    0 => Some(Kurve::Gamma(1.0)),
                    1 => {
                        // `u8Fixed8Number`: der Wert ist `v / 256`.
                        let g = u16::from_be_bytes([m[12], m[13]]) as f64 / 256.0;
                        Some(Kurve::Gamma(if g == 0.0 { 1.0 } else { g }))
                    }
                    _ => {
                        let anzahl = n.min((m.len().saturating_sub(12)) / 2);
                        let t: Vec<u16> = (0..anzahl)
                            .map(|i| u16::from_be_bytes([m[12 + i * 2], m[13 + i * 2]]))
                            .collect();
                        if t.is_empty() {
                            Some(Kurve::NichtAuswertbar(*b"curv"))
                        } else {
                            Some(Kurve::Tabelle(t))
                        }
                    }
                }
            }
            Some(b"para") => {
                if m.len() < 12 {
                    return Some(Kurve::NichtAuswertbar(*b"para"));
                }
                let typ = u16::from_be_bytes([m[8], m[9]]);
                let fest = |i: usize| -> f64 {
                    let p = 12 + i * 4;
                    if p + 4 > m.len() {
                        return 0.0;
                    }
                    i32::from_be_bytes([m[p], m[p + 1], m[p + 2], m[p + 3]]) as f64 / 65536.0
                };
                Some(Kurve::Parametrisch {
                    typ,
                    parameter: [
                        fest(0),
                        fest(1),
                        fest(2),
                        fest(3),
                        fest(4),
                        fest(5),
                        fest(6),
                    ],
                })
            }
            other => Some(Kurve::NichtAuswertbar(match other {
                Some(v) => [v[0], v[1], v[2], v[3]],
                // Zu kurze Marke: als nicht auswertbar melden, nicht
                // schneiden (siehe Kommentar an dieser Funktion).
                None => *b"????",
            })),
        }
    }

    /// Der Kurzname der Beschreibung, falls lesbar (`desc`-Marke).
    pub fn name(&self) -> String {
        // `desc` ist ein `textDescriptionType` (Typ `desc`): Längen-
        // Byte, dann der Text. Fuer neuere Profile (`mluc`) ist der Text
        // mehrsprachig — dann nehmen wir die erste Zeichenkette.
        if let Some(m) = self.marke(b"desc") {
            if m.len() >= 12 && m.get(..4) == Some(b"desc".as_slice()) {
                let n = m[8] as usize;
                if 9 + n <= m.len() {
                    return String::from_utf8_lossy(&m[9..9 + n]).to_string();
                }
            }
        }
        if let Some(m) = self.marke(b"mluc") {
            if m.len() >= 28 && m.get(..4) == Some(b"mluc".as_slice()) {
                let anzahl = u32::from_be_bytes([m[8], m[9], m[10], m[11]]) as usize;
                let laenge = u32::from_be_bytes([m[20], m[21], m[22], m[23]]) as usize;
                let versatz = u32::from_be_bytes([m[24], m[25], m[26], m[27]]) as usize;
                let _ = anzahl;
                if versatz + laenge <= m.len() && laenge > 0 {
                    return String::from_utf8_lossy(&m[versatz..versatz + laenge]).to_string();
                }
            }
        }
        String::new()
    }
}

/// Das eingebettete Profil einer Bilddatei, wenn sie eines traegt.
///
/// JPEG: die `APP2`-Segmente mit der Signatur `ICC_PROFILE\0`, nach ihrer
/// laufenden Nummer sortiert und aneinandergehaengt (die Norm erlaubt
/// 65535 Byte je Segment, ein Profil ist also ueber mehrere Segmente
/// verteilt). PNG: der `iCCP`-Chunk, der mit zlib gepackte Profil.
///
/// `None`, wenn die Datei kein Profil traegt — das ist der Normalfall und
/// kein Fehler.
pub fn profil(daten: &[u8]) -> Option<Vec<u8>> {
    match crate::erkenne(daten) {
        Some(crate::Bildformat::Jpeg) => profil_aus_jpeg(daten),
        Some(crate::Bildformat::Png) => profil_aus_png(daten),
        _ => None,
    }
}

/// Die Nummer der JPEG-Komponenten (`1` grau, `3` RGB, `4` CMYK).
///
/// GEMESSEN 02.10.2026: `test_cmyk.jpg` liefert 4, `test_base.jpg` 3.
/// Das ist die Angabe, an der man CMYK erkennt, ohne auf ein Profil zu
/// warten.
pub fn jpeg_komponenten(daten: &[u8]) -> Option<u8> {
    let mut i = 2usize;
    while i + 3 < daten.len() {
        if daten[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = daten[i + 1];
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        if m == 0xDA {
            return None;
        }
        let laenge = u16::from_be_bytes([daten[i + 2], daten[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            return daten.get(i + 9).copied();
        }
        i += 2 + laenge;
    }
    None
}

/// Das aus den `APP2`-Segmenten zusammengesetzte Profil.
pub fn profil_aus_jpeg(daten: &[u8]) -> Option<Vec<u8>> {
    // (Nummer, Nutzlast) je Segment
    let mut teile: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut i = 2usize;
    while i + 3 < daten.len() {
        if daten[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = daten[i + 1];
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        if m == 0xDA {
            break;
        }
        let laenge = u16::from_be_bytes([daten[i + 2], daten[i + 3]]) as usize;
        if laenge < 2 {
            break;
        }
        let anfang = i + 4;
        let ende = i
            .checked_add(2 + laenge)
            .unwrap_or(usize::MAX)
            .min(daten.len());
        if m == 0xE2 && ende >= anfang + 14 && &daten[anfang..anfang + 12] == b"ICC_PROFILE\0" {
            let nummer = daten[anfang + 12];
            let nutzlast = daten[anfang + 14..ende].to_vec();
            teile.push((nummer, nutzlast));
        }
        i = 2 + laenge + i;
        if i >= daten.len() {
            break;
        }
    }
    if teile.is_empty() {
        return None;
    }
    teile.sort_by_key(|(n, _)| *n);
    let mut raus = Vec::new();
    for (_, t) in teile {
        if raus.len() + t.len() > MAX_PROFIL {
            return None;
        }
        raus.extend_from_slice(&t);
    }
    Some(raus)
}

/// Das aus dem `iCCP`-Chunk entpackte Profil.
pub fn profil_aus_png(daten: &[u8]) -> Option<Vec<u8>> {
    const SIGNATUR: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if daten.len() < 8 || daten[..8] != SIGNATUR {
        return None;
    }
    let mut i = 8usize;
    while i + 8 <= daten.len() {
        let laenge =
            u32::from_be_bytes([daten[i], daten[i + 1], daten[i + 2], daten[i + 3]]) as usize;
        let typ = [daten[i + 4], daten[i + 5], daten[i + 6], daten[i + 7]];
        let datenbeginn = i.checked_add(8)?;
        let datenende = datenbeginn.checked_add(laenge)?;
        if datenende > daten.len() {
            return None;
        }
        if &typ == b"iCCP" {
            let block = &daten[datenbeginn..datenende];
            // Aufbau: Name, Nullbyte, Kompressionsverfahren, gepacktes
            // Profil.
            let null = block.iter().position(|&b| b == 0)?;
            if null + 1 >= block.len() {
                return None;
            }
            let roh = entpacke(&block[null + 2..], MAX_PROFIL).ok()?;
            return Some(roh);
        }
        if typ == [b'I', b'E', b'N', b'D'] {
            return None;
        }
        // M3: `12` ist die Chunklaenge plus Typ plus CRC.
        i = datenende.checked_add(4)?;
    }
    None
}

// ---------------------------------------------------------------------------
// B4: Geraete-RGB (Matrix/Shader) nach sRGB
// ---------------------------------------------------------------------------

/// Bradford-Matrix, ihr Inverses, D50 und D65 (ICC.1:2001-04).
///
/// Die PCS eines ICC-Profils ist XYZ mit D50. sRGB hat D65. Zwischen
/// beiden liegt die Anpassung `M^-1 * B * diag(B*weiss_neu / B*weiss_alt)`,
/// hier fuer D50 nach D65 ausgerechnet.
const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];
const BRADFORD_UMGEKEHRT: [[f64; 3]; 3] = [
    [0.9869929, -0.1470543, 0.1599627],
    [0.4323053, 0.5183603, 0.0492912],
    [-0.0085287, 0.0400428, 0.9684867],
];
const WEISS_D65: [f64; 3] = [0.95047, 1.0, 1.08883];
const WEISS_D50: [f64; 3] = [0.96422, 1.0, 0.82521];

/// XYZ(D65) nach linearem sRGB (die uebliche 3x3-Matrix).
const XYZ_ZU_SRGB: [[f64; 3]; 3] = [
    [3.2404542, -1.5371385, -0.4985314],
    [-0.9692660, 1.8760108, 0.0415560],
    [0.0556434, -0.2040259, 1.0572252],
];

/// Die Anpassungsmatrix von einem Weisspunkt zum anderen.
///
/// `B^-1 * diag(B * weiss_neu / B * weiss_alt) * B` — die Bradford-
/// Anpassung in den Kegelkoordinaten. Fuer `von = D50, nach = D65` ergibt
/// sie
///
/// ```text
///  0.95558 -0.02304  0.06316
/// -0.02829  1.00994  0.02101
///  0.01230 -0.02048  1.32991
/// ```
///
/// GEMESSENE FEHLER beim Bauen (alle drei ueber die Probe „ein sRGB-
/// Profil muss die Identitaet liefern“ gefunden):
///  1. `B^-1 * B * diag * B` statt `B^-1 * diag * B` — das `B^-1 * B` ist
///     fast die Identitaet, und heraus kam eine reine Diagonalmatrix.
///  2. Die Reihenfolge der Argumente war vertauscht, also rechnete die
///     Funktion D65 nach D50, wenn D50 nach D65 gemeint war.
///  3. Die Gesamtmatrix wurde auf `M * linear` angewendet, obwohl sie
///     `M` schon enthaelt — die Farbmatrix war damit doppelt.
/// Die Kontrolle ist die, dass ein sRGB-Profil jeden Wert unveraendert
/// laesst: mit allen drei Fehlern kam (255, 255, 255) als (255, 249, 171)
/// heraus, jetzt als (255, 255, 255).
fn anpassung(von: [f64; 3], nach: [f64; 3]) -> [[f64; 3]; 3] {
    let kegel = |v: [f64; 3]| -> [f64; 3] {
        [
            BRADFORD[0][0] * v[0] + BRADFORD[0][1] * v[1] + BRADFORD[0][2] * v[2],
            BRADFORD[1][0] * v[0] + BRADFORD[1][1] * v[1] + BRADFORD[1][2] * v[2],
            BRADFORD[2][0] * v[0] + BRADFORD[2][1] * v[1] + BRADFORD[2][2] * v[2],
        ]
    };
    let kv = kegel(von);
    let kn = kegel(nach);
    let mut diag = [[0.0f64; 3]; 3];
    for i in 0..3 {
        diag[i][i] = if kv[i] == 0.0 { 1.0 } else { kn[i] / kv[i] };
    }
    matmul(&BRADFORD_UMGEKEHRT, &matmul(&diag, &BRADFORD))
}

fn matmul(x: &[[f64; 3]; 3], y: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut r = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = x[i][0] * y[0][j] + x[i][1] * y[1][j] + x[i][2] * y[2][j];
        }
    }
    r
}

/// Die sRGB-Kodierkurve: linearer Wert nach Geraetewert 0..=1.
///
/// Das ist die Umkehrung der Gangkurve, die in den Profilen steht
/// (`Kurve::wert` bildet den Geraetewert auf linear ab).
fn srgb_kodieren(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0);
    if y <= 0.0031308 {
        12.92 * y
    } else {
        1.055 * y.powf(1.0 / 2.4) - 0.055
    }
}

/// Alles, was fuer die Umrechnung eines Matrix/Shader-Profils noetig ist.
pub struct Umrechnung {
    /// Geraete-RGB (linear) nach linearem sRGB — das ist
    /// `XYZ_ZU_SRGB * anpassung(D50, D65) * nach_xyz`, einmal gerechnet.
    ///
    /// GEMESSENER FEHLER (02.10.2026): zuerst stand nur
    /// `XYZ_ZU_SRGB * nach_xyz`, und die D50-Anpassung wurde in
    /// `pixel` berechnet und dann nicht benutzt (der Compiler sagte
    /// „unused variable: x65“). Ohne die Anpassung fehlt der Weisspunkt
    /// des Geraets, und das Bild bekommt einen Farbstich.
    nach_srgb: [[f64; 3]; 3],
    /// Die drei Gangkurven des Geraets.
    kurven: [Kurve; 3],
}

impl Umrechnung {
    /// Rechnet einen Geraetewert (0..=255) auf einen sRGB-Wert (0..=255).
    pub fn pixel(&self, rgb: [u8; 3]) -> [u8; 3] {
        let mut linear = [0.0f64; 3];
        for k in 0..3 {
            linear[k] = self.kurven[k].wert(rgb[k] as f64 / 255.0);
        }
        // GEMESSENER FEHLER: hier stand erst `M * linear` und dann die
        // Gesamtmatrix `XYZ_ZU_SRGB * A * M` darauf — die Farbmatrix
        // war damit doppelt im Spiel, und ein sRGB-Profil lieferte
        // (255, 255, 255) als (255, 249, 171). `nach_srgb` enthaelt `M`
        // bereits und wird direkt auf die linearen Geraetewerte
        // angewendet.
        let m = &self.nach_srgb;
        let xyz = [
            m[0][0] * linear[0] + m[0][1] * linear[1] + m[0][2] * linear[2],
            m[1][0] * linear[0] + m[1][1] * linear[1] + m[1][2] * linear[2],
            m[2][0] * linear[0] + m[2][1] * linear[1] + m[2][2] * linear[2],
        ];
        let lin = xyz;
        // RANDFARBEN: hier wird GEKLEMMT, und das ist eine bewusste
        // Entscheidung — mit dem Fehler, warum.
        //
        // GEMESSEN am 02.10.2026 gegen LittleCSS an einem Muster mit
        // Randfarben (PNG mit P3-Profil, verlustfrei): hartes Klemmen auf
        // 0 und 1 ergibt groesste Abweichung **32** bei 540 von 9216
        // Kanalwerten. LittleCSS erhaelt dort den Farbton und zieht nur
        // leicht zurueck: Geraet (89, 195, 12) -> LittleCSS (32, 196, 0),
        // bei uns (0, 198, 0).
        //
        // GEMESSENER FEHLER, und deshalb steht hier kein weiches
        // Klemmen: der Versuch, den linearen Wert gegen die Weissachse zu
        // mischen (die uebliche farbtonerhaltende Loesung), hat die
        // Abweichung auf **154** bei 2019 von 9216 Kanalwerten
        // verschlechtert. Grund: die Weissachse ist der Mittelwert der
        // drei Anteile, und bei einer Farbe ausserhalb des Bereichs ist
        // einer davon weit aus dem gamut — der Mischanteil hebt dann die
        // Helligkeit und die Farbe wird matt statt saettig. LittleCSS
        // macht erkennbar etwas anderes (ein Knie in der Kodierkurve
        // oder eine Entschwarzungsregel), und solange das nicht gemessen
        // ist, ist das harte Klemmen die ehrlichere Wahl: es ist die
        // Vorgabe der Norm ("the output is clipped") und es laesst sich
        // erklaeren.
        let lin = lin;
        let mut raus = [0u8; 3];
        for k in 0..3 {
            raus[k] = (srgb_kodieren(lin[k]) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        raus
    }
}

/// Baut die Umrechnung eines Matrix/Shader-Profils.
///
/// `None`, wenn das Profil keine RGB-Matrix mit drei Gangkurven traegt —
/// etwa ein CMYK-Profil mit LUTs (`A2B0`) oder ein Graustufenprofil. Der
/// Aufrufer laesst dann das Bild, wie es ist: ein sichtbares Bild mit
/// moeglichst richtigem Farbraum ist besser als keines.
pub fn matrix_shaper(p: &Profil) -> Option<Umrechnung> {
    if p.farbklasse() != Farbklasse::Rgb {
        return None;
    }
    let r = p.xyz(b"rXYZ")?;
    let g = p.xyz(b"gXYZ")?;
    let bl = p.xyz(b"bXYZ")?;
    let mut kurven = [Kurve::Gamma(1.0), Kurve::Gamma(1.0), Kurve::Gamma(1.0)];
    for (i, sig) in [&b"rTRC"[..], &b"gTRC"[..], &b"bTRC"[..]]
        .iter()
        .enumerate()
    {
        let sig: &[u8; 4] = match sig {
            [a, b, c, d] => &[*a, *b, *c, *d],
            _ => continue,
        };
        if let Some(k) = p.kurve(sig) {
            if !matches!(k, Kurve::NichtAuswertbar(_)) {
                kurven[i] = k;
            }
        }
    }
    // Die Spalten der Matrix sind die Farbwerte (Y = 1 genormt). Die
    // Summe der drei Spalten ist der Weisspunkt — das pruefen wir, denn
    // eine verdrehte Marke ergaebe sonst stillschweigend falsche Farben.
    let summe = [
        r[0] + g[0] + bl[0],
        r[1] + g[1] + bl[1],
        r[2] + g[2] + bl[2],
    ];
    if summe.iter().any(|v| *v <= 0.0) {
        return None;
    }
    let wtpt = p.xyz(b"wtpt");
    if let Some(w) = wtpt {
        // GEMESSEN: `wtpt` fehlt in manchen Profilen, dann nehmen wir die
        // Summe — das ist derselbe Weisspunkt.
        if w.iter().all(|v| *v > 0.0) && (0..3).any(|k| (summe[k] - w[k]).abs() > 0.01) {
            return None;
        }
    }
    let nach_xyz = [
        [r[0], g[0], bl[0]],
        [r[1], g[1], bl[1]],
        [r[2], g[2], bl[2]],
    ];
    // Geraet nach sRGB in einem Schritt: die PCS eines Profils ist D50,
    // sRGB ist D65, dazwischen die Bradford-Anpassung.
    let a = anpassung(WEISS_D50, WEISS_D65);
    let nach_srgb = matmul(&XYZ_ZU_SRGB, &matmul(&a, &nach_xyz));
    Some(Umrechnung { nach_srgb, kurven })
}

/// Rechnet ein Bild auf sRGB um, wenn die Datei ein passendes Profil
/// traegt (B4).
///
/// Gibt zurueck, ob umgerechnet wurde. Ohne Profil, mit einem
/// CMYK-Profil (das braucht LUTs) oder mit einem kaputten Profil wird
/// **nicht** umgerechnet: ein Bild mit dem Geraetefarbraum ist besser
/// als keines, und ein Fehler beim Laden waere an dieser Stelle nicht
/// angebracht — die Datei ist ja lesbar.
///
/// M3/M4: der Puffer wird nicht neu alloziert, nur die drei Farbkanaele
/// jedes Pixels werden ersetzt; der Alphakanal bleibt unberuehrt.
pub fn auf_srgb(bild: &mut crate::Bild, daten: &[u8]) -> bool {
    let roh = match profil(daten) {
        Some(r) => r,
        None => return false,
    };
    let p = match Profil::neu(roh) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let u = match matrix_shaper(&p) {
        Some(u) => u,
        None => return false,
    };
    // Release 14.10.2026 (Agent CSS2): a profile that is sRGB up to rounding
    // (GIMP and Photoshop embed their own sRGB copies) is the identity; the
    // matrix path would only add drift. In-gamut probe colours (saturated
    // primaries clip to themselves even for P3) within 4 of themselves
    // mean "already sRGB" (browsers detect sRGB profiles the same way).
    // MEASURED before (WPT css-backgrounds `border-image-002`,
    // `1pxgreen-98pxred.png` with a GIMP sRGB profile): green 0,128,0 became
    // 3,128,3 and the reftest failed.
    let probes: [[u8; 3]; 6] = [[200, 100, 50], [50, 150, 200], [100, 200, 100], [0, 128, 0], [128, 128, 128], [255, 255, 255]];
    if probes.iter().all(|c| u.pixel(*c).iter().zip(c.iter()).all(|(n, a)| n.abs_diff(*a) <= 4)) {
        return false;
    }
    for p in bild.rgba.chunks_exact_mut(4) {
        let neu = u.pixel([p[0], p[1], p[2]]);
        p[0] = neu[0];
        p[1] = neu[1];
        p[2] = neu[2];
    }
    true
}
