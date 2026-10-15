//! GIF SCHREIBEN (87a/89a, LZW, globale Palette, Animation).
//!
//! GIF ist das aelteste Format, das der Browser anzeigt — und das mit den
//! meisten Fallen. Drei davon entscheiden darueber, ob eine GIF-Datei
//! ueberhaupt angezeigt wird:
//!
//! 1. **Der LZW-Diktorindexzaehler.** In GIF beginnt der naechste Code bei
//!    `min_code + 2` (nicht bei `min_code + 1` wie in TIFF), und nach
//!    JEDEM geschriebenen Code wird ein neues Woerterbuch-Eintrag
//!    hinzugefuegt — auch im „KwKwK"-Fall. Vergisst man eines von beidem,
//!    laeuft der Datenstrom ab einem gewissen Punkt auseinander: der
//!    Leser gibt dann Farbindex 0 fuer den Rest des Bildes aus, das Bild
//!    sieht „abgeschnitten" aus, und niemand findet den Grund.
//! 2. **Die Mindestcodebreite.** Bei genau 2^n Codes muss die Breite
//!    ERHÖHT werden, BEVOR das 2^n+1. Code geschrieben wird. Ein
//!    Ein-Bit-Versatz macht den ganzen Rest des Bildes farbig falsch.
//! 3. **`0x00` ist ein FREIES Bild** und `0x01` steht fuer „keine
//!    Transparenz". Transparenz loest man also nicht ueber einen Paletten-
//!    index, sondern ueber das GCE-Flag.
//!
//! # Verlustfreiigkeit
//!
//! GIF hat 256 Farben und 1-Bit-Transparenz. Ein Bild mit weniger Farben
//! und ohne Alpha wird **bitgleich** gerundet — das ist die Abnahme des
//! Auftrags. Alles andere wird quantisiert, und [`farbe::quantisiere`]
//! sagt vorher, ob es verlustfrei war.

use crate::Bild;
use crate::farbe::{self, Palette};
use crate::fehler::KonvertFehler;
use crate::grenzen;

/// Das GIF89a-Kopfzeichen („GIF87a" geht auch, bietet aber keine
/// Erweiterungen; 89a ist der sichere Weg).
const KOPF: &[u8; 6] = b"GIF89a";

/// Der Blockart-Byte fuer „Erweiterung einfuehren".
const ERWEITERUNG: u8 = 0x21;
/// Der Blockart-Byte fuer „Bild einfuehren".
const BILD: u8 = 0x2C;
/// Das Ende der Datei.
const ENDE: u8 = 0x3B;

/// Hoechste LZW-Codebreite (12 Bit => 4096 Codes).
const MAX_LZW_LAENGE: u8 = 12;
/// Hoechste Zahl LZW-Codes im Woerterbuch (2^12).
const MAX_LZW_CODES: u16 = 4096;

/// Das GCE-Flag fuer „diese Grafik hat Transparenz".
const FLAG_TRANSPARENT: u8 = 1;

/// Ein einzelnes Bild einer GIF-Datei.
#[derive(Debug, Clone)]
pub struct GifBild {
    /// Das Bild in voller Logik-Bildgroesse (nicht der Teilfenstergroesse).
    pub bild: Bild,
    /// Verzoegerung in ms (das GIF-Feld ist 100tel Sekunden, also Vielfache von 10).
    pub verzoegerung_ms: u32,
    /// Versatz der linken oberen Ecke im Logik-Bild.
    pub versatz: (u32, u32),
    /// Wie soll der Bildschirm nach diesem Bild aussehen?
    pub entsorgung: Entsorgung,
}

/// Was mit dem Bildschirm nach einem Bild geschehen soll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Entsorgung {
    /// Unveraendert lassen (Bilder werden uebereinander gelegt).
    #[default]
    Behalten,
    /// Auf die Hintergrundfarbe zuruecksetzen.
    Aufraeumen,
    /// Auf den Zustand vor diesem Bild zuruecksetzen.
    Zuruecknehmen,
}

impl Entsorgung {
    /// Das Byte im GCE-Block. `2` ist der reservierte Wert fuer „nichts".
    fn byte(self) -> u8 {
        match self {
            Entsorgung::Behalten => 0,
            Entsorgung::Aufraeumen => 2,
            Entsorgung::Zuruecknehmen => 3,
        }
    }
}

/// Wie soll geschrieben werden?
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GifOptionen {
    /// Hoechste Zahl Farben in der Palette (1..=256).
    pub farben: usize,
    /// Floyd-Steinberg an (Standard: ja).
    ///
    /// Ohne Dithering bekommt ein Verlauf mit 256 Stufen sichtbare Baender
    /// („Banding"). Das ist bei GIF der Normalfall, weil 256 Farben fuer
    /// einen Verlauf wenig sind.
    pub dither: bool,
    /// Das Alpha als Farbe mitzaehlen (nicht nur als 1-Bit-Transparenz).
    ///
    /// `false` (Standard): Alpha 0 wird der transparente Index, alles
    /// andere undurchsichtig. Das ist die GIF-Art Deckkraft, und es ist
    /// eine Vereinfachung — ein weicher Schatten wird abgeschnitten.
    /// `true`: halbtransparente Pixel werden zu Palettenfarben mit
    /// diesem Alpha-Wert. Kostet Farben und ist trotzdem nur so gut wie
    /// die Palette hergibt.
    pub alpha_als_farbe: bool,
    /// Endlosschleife (0 = endlos, wie im GIF der Normalfall).
    pub wiederholungen: u32,
}

impl Default for GifOptionen {
    fn default() -> Self {
        GifOptionen {
            farben: 256,
            dither: true,
            alpha_als_farbe: false,
            wiederholungen: 0,
        }
    }
}

/// Schreibt eine GIF-Datei aus einer Liste von Bildern.
///
/// # Fehler
/// - [`KonvertFehler::ZuVieleBilder`] wenn mehr Bilder als
///   [`grenzen::MAX_GIF_BILDER`] kommen.
/// - [`KonvertFehler::PaletteZuGross`] wenn `optionen.farben` > 256.
/// - [`KonvertFehler::UnbekannteOption`] wenn `farben` 0 ist.
/// - Siehe [`crate::grenzen::pruefe_bild`].
pub fn schreiben(bilder: &[GifBild], optionen: GifOptionen) -> Result<Vec<u8>, KonvertFehler> {
    if bilder.is_empty() {
        return Err(KonvertFehler::ZuVieleBilder {
            anzahl: 0,
            grenze: grenzen::MAX_GIF_BILDER,
        });
    }
    if bilder.len() > grenzen::MAX_GIF_BILDER {
        return Err(KonvertFehler::ZuVieleBilder {
            anzahl: bilder.len(),
            grenze: grenzen::MAX_GIF_BILDER,
        });
    }
    if optionen.farben == 0 {
        return Err(KonvertFehler::UnbekannteOption { name: "farben = 0" });
    }
    grenzen::pruefe_laenge(optionen.farben, grenzen::MAX_PALETTE, "GIF-Palette")?;
    for b in bilder {
        grenzen::pruefe_bild(&b.bild)?;
    }
    // M3/M4: die Summe aller Pixel ist begrenzt (ein Vektor aus Bildern
    // waere sonst eine Bombe).
    let summe: u64 = bilder
        .iter()
        .map(|b| b.bild.breite as u64 * b.bild.hoehe as u64)
        .sum();
    if summe > grenzen::MAX_GIF_PIXEL {
        return Err(KonvertFehler::ZuVieleBilder {
            anzahl: bilder.len(),
            grenze: grenzen::MAX_GIF_PIXEL as usize,
        });
    }
    // Das Logik-Bild: so gross wie das groesste Einzelbild.
    let mut logik_breite = 0u32;
    let mut logik_hoehe = 0u32;
    for b in bilder {
        logik_breite = logik_breite.max(b.versatz.0 + b.bild.breite);
        logik_hoehe = logik_hoehe.max(b.versatz.1 + b.bild.hoehe);
    }
    // M3: Logik-Bild auf 65535 klemmen (das sind die Felder des GIF-Kopfs).
    if logik_breite > 65535 || logik_hoehe > 65535 {
        return Err(KonvertFehler::KanteZuGross {
            breite: logik_breite,
            hoehe: logik_hoehe,
            grenze: 65535,
        });
    }

    // --- Quantisierung: EINE Palette fuer alle Bilder ---
    // Eine gemeinsame Palette ist kein Optimismus, sondern eine Notwendigkeit:
    // GIF erlaubt eine globale Farbtabelle, und wenn man sie pro Bild neu
    // schreibt, braucht jede Palette 768 Byte und der Leser muss sie
    // tausendmal laden.
    let gemeinsam = gemeinsame_palette(bilder, optionen)?;

    // --- Datei ---
    // M4: Groesse vorab. Pro Bild: Logik-Deskriptor (10) + GCE (8) +
    // LZW-Daten (im schlimmsten Fall 9 Bit je Pixel -> 9/8 * Pixel + 64).
    let mut erwartet = 13 + 4 * 256 + 3 * 256 + 13usize; // Kopf + GCT + (LZW-Tabelle, falls nötig)
    for b in bilder {
        let pixel = b.bild.breite as usize * b.bild.hoehe as usize;
        erwartet = erwartet
            .checked_add(10 + 8)
            .and_then(|v| v.checked_add(pixel * 9 / 8 + 64))
            .ok_or(ueberlauf())?;
    }
    grenzen::pruefe_laenge(
        erwartet,
        grenzen::MAX_SPEICHER,
        "GIF-Ausgabe vorab reserviert",
    )?;
    let mut out: Vec<u8> = Vec::with_capacity(erwartet);

    // --- Die Groesse der Farbtabelle muss VOR dem Kopf bekannt sein ---
    //
    // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 an PIL:
    // „cannot identify image file", bei jedem GIF mit mehr als einem
    // Pixel). Zwei Fehler in einer Zeile:
    //
    // 1. In die drei Bits der Tabellen-GROESSE stand die Bild-Bittiefe
    //    (`spurweite - 1`). Bei einem 320x240-Bild ist das 7, und 7
    //    bedeutet 256 Tabelleneintraege — der Leser las also 768 Byte
    //    Palette aus einer Datei, die 48 Byte Palette enthaelt.
    // 2. Das Packed-Byte stand doppelt: einmal an Offset 10 (wo es
    //    hingehört) und noch einmal an Offset 13 als „zweites Packed
    //    Byte". Das zweite Byte landete als PALETTENEINTRAG in der
    //    Tabelle — und alle drei folgenden Eintraege um eins verschoben.
    //
    // Beides ist dieselbe Fehlerklasse: die Datei ist gueltig, solange
    // man nicht weiterliest. `file` erkennt sie, und der Leser des
    // Projekts liest dieselben drei Bits (`narsil-image/src/gif.rs:330`),
    // aber PIL haelt beim Identifizieren an. Genau deshalb gehoert
    // `file`/PIL/ffprobe zur Abnahme und nicht nur der eigene Rundlauf.
    let tabellen_eintraege = aufrunden_zweierpotenz(gemeinsam.palette.len());
    let groessen_code = tabellen_eintraege.trailing_zeros() - 1;

    // --- Kopf ---
    out.extend_from_slice(KOPF);
    out.extend_from_slice(&(logik_breite as u16).to_le_bytes());
    out.extend_from_slice(&(logik_hoehe as u16).to_le_bytes());
    // Packed field: Bit 7 = globale Farbtabelle, Bit 1 = Tabelle
    // sortiert, Bit 0 = Tabelle in Groesse 2^(Code+1). Bits 6..4 = die
    // Farbauflosung, ein reiner Hinweis (der Leser des Projekts
    // uebersieht sie, PIL nicht).
    let aufloesung = spurweite(logik_breite).max(spurweite(logik_hoehe)) - 1;
    out.push(0x80 | (((aufloesung & 0x07) << 4) as u8) | ((groessen_code & 0x07) as u8));
    out.push(0); // Hintergrundindex (0 = der erste Paletteneintrag)
    out.push(0); // Seitenverhaeltnis (0 = nicht gesetzt)

    // --- Globale Farbtabelle ---
    for i in 0..tabellen_eintraege {
        let f = gemeinsam
            .palette
            .farben
            .get(i as usize)
            .copied()
            .unwrap_or([0, 0, 0, 0]);
        out.push(f[1]);
        out.push(f[2]);
        out.push(f[3]);
    }
    // Das Alphafeld der Palette wird als GCT-Transparenz benutzt.
    let transparent = gemeinsam.transparenter_index;

    // --- Schleife ---
    if optionen.wiederholungen > 1 {
        // NETSCAPE2.0-Anwendungserweiterung: der Weg, eine Schleifenzahl
        // zu schreiben. Ohne sie ist GIF immer endlos.
        out.push(ERWEITERUNG);
        out.push(0xFF); // Application Extension
        out.push(11); // Blockgroesse
        out.extend_from_slice(b"NETSCAPE2.0");
        out.push(3); // Sub-Block: Unterblockgroesse
        out.push(1); // Schleifen-Id
        out.extend_from_slice(&(optionen.wiederholungen.saturating_sub(1) as u16).to_le_bytes());
        out.push(0); // Ende der Unterbloecke
    }

    for b in bilder {
        // --- Graphic Control Extension ---
        // Steht VOR jedem Bild und ist nicht optional, sobald man
        // Verzoegerung, Transparenz oder Entsorgung will.
        out.push(ERWEITERUNG);
        out.push(0xF9);
        out.push(4); // Blockgroesse
        let mut flag = (b.entsorgung.byte() & 0x07) << 2;
        if transparent.is_some() {
            flag |= FLAG_TRANSPARENT;
        }
        out.push(flag);
        out.extend_from_slice(
            &((b.verzoegerung_ms / 10).min(u16::MAX as u32) as u16).to_le_bytes(),
        );
        out.push(transparent.unwrap_or(0));
        out.push(0); // Ende der Unterbloecke

        // --- Bilddeskriptor ---
        out.push(BILD);
        out.extend_from_slice(&(b.versatz.0.min(65535) as u16).to_le_bytes());
        out.extend_from_slice(&(b.versatz.1.min(65535) as u16).to_le_bytes());
        out.extend_from_slice(&(b.bild.breite.min(65535) as u16).to_le_bytes());
        out.extend_from_slice(&(b.bild.hoehe.min(65535) as u16).to_le_bytes());
        out.push(0); // Keine lokale Farbtabelle, keine Verschachtelung

        // --- LZW ---
        // JEDES Bild wird gegen die GEMEINSAME Palette indiziert — auch das
        // erste. Sonst traegt Bild 1 Indizes, die nur fuer seine eigene
        // Palette gelten.
        let indizes = neu_indizieren(&b.bild, &gemeinsam, optionen.dither);
        lzw_schreiben(&mut out, &indizes, spurweite(logik_breite).max(2))?;
    }

    out.push(ENDE);
    Ok(out)
}

/// Schreibt ein einzelnes Bild als einzelbildige GIF.
///
/// Die haeufigste Form des Aufrufs („Bild als GIF") — ohne Animation,
/// ohne Verzoegerung, mit den Vorgaben.
pub fn einzelbild(bild: &Bild, optionen: GifOptionen) -> Result<Vec<u8>, KonvertFehler> {
    schreiben(
        &[GifBild {
            bild: bild.clone(),
            verzoegerung_ms: 0,
            versatz: (0, 0),
            entsorgung: Entsorgung::Behalten,
        }],
        optionen,
    )
}

/// Die gemeinsame Palette aller Bilder plus der transparente Index.
struct GemeinsamePalette {
    palette: Palette,
    /// Welcher Palettenindex bedeutet „durchsichtig"?
    transparenter_index: Option<u8>,
    // KEIN `alpha_gilt`-Feld mehr (02.10.2026): es wurde gesetzt und nie
    // gelesen. `gemeinsame_palette` braucht die Angabe nur lokal, um zu
    // entscheiden, ob der transparente Index reserviert wird (siehe
    // `irgendein_alpha && !alpha_gilt`), und das Ergebnis dieser
    // Entscheidung steckt bereits in `transparenter_index`. Ein Feld
    // daneben zu fuehren hiesse, dieselbe Antwort zweimal zu notieren —
    // und die beiden Koepfe koennten auseinanderlaufen.
}

/// Baut EINE Palette ueber alle Bilder.
///
/// Warum ueber alle: GIF hat eine globale Farbtabelle, und eine Datei mit
/// 1000 Bildern und 1000 lokalen Tabellen waere 768 000 Byte nur an
/// Tabellen. Ausserdem sieht eine Animation mit wechselnder Palette
/// „flackernd" aus.
fn gemeinsame_palette(
    bilder: &[GifBild],
    optionen: GifOptionen,
) -> Result<GemeinsamePalette, KonvertFehler> {
    // Hat irgendein Bild Transparenz?
    let irgendein_alpha = bilder.iter().any(|b| {
        (0..b.bild.breite as usize * b.bild.hoehe as usize).any(|p| b.bild.rgba[p * 4 + 3] == 0)
    });
    let alpha_gilt = optionen.alpha_als_farbe;

    // Die Palette kommt aus ALLEN Bildern, nicht aus dem ersten.
    //
    // GEMESSEN 02.10.2026: `gif_animation_drei_bilder` schlug bei Rahmen 1
    // fehl — Pixel (8,0), Kanal 1: sollte 0, war 255. Ursache: die Palette
    // enthielt nur die Farben des ersten Bildes, und Rahmen 1 hat eine
    // Farbe mehr. Der Enkoder hat sie auf den naechstgelegenen Ton
    // abgebildet, und das war 255 statt 0.
    //
    // Bei einer Animation aus einem Video ist das der Normalfall: jedes
    // Bild hat tausende eigene Farben, keine zwei sind gleich. „Das erste
    // Bild ist die Referenz" ist dann schlicht falsch.
    let bilder_refs: Vec<&Bild> = bilder.iter().map(|b| &b.bild).collect();
    let palette = Palette::aus_bildern(&bilder_refs, optionen.farben, alpha_gilt)?;

    // Der transparente Index: der erste Paletteneintrag mit Alpha 0.
    // Ohne eine durchsichtige Farbe in der Palette gibt es keinen
    // transparenten Index — das ist kein Fehler, sondern heisst „dieses
    // GIF hat keine Transparenz".
    let transparenter_index = if irgendein_alpha && !alpha_gilt {
        // Wir brauchen einen Palettenplatz dafuer. Wir nehmen den ersten
        // und verschieben die uebrigen Farben um eins — sonst waere der
        // Index 0 (oder 1) doppelt belegt und ein Pixel mit dieser Farbe
        // waere unsichtbar.
        Some(0)
    } else if irgendein_alpha {
        palette
            .farben
            .iter()
            .position(|f| f[0] == 0)
            .map(|i| i as u8)
    } else {
        None
    };

    let mut palette = palette;
    if transparenter_index == Some(0) {
        palette.farben.insert(0, [0, 0, 0, 0]);
        palette.farben.truncate(grenzen::MAX_PALETTE);
    }
    Ok(GemeinsamePalette {
        palette,
        transparenter_index,
    })
}

/// Indiziert ein Bild gegen die GEMEINSAME Palette.
///
/// Das ist der Weg fuer JEDES Bild (auch das erste): die Palette ist
/// global, und ein Bild, das gegen seine eigene Palette indiziert
/// wuerde, bekäme Indizes, die an anderer Stelle etwas anderes bedeuten.
///
/// Drei Faelle, alle benannt:
///
/// * **verlustfrei**: die Palette enthaelt exakt die Bildfarben (Farbindex
///   mit exakter Tabelle). Jedes Pixel findet seinen Index ohne
///   Naeherung — das ist die Rundreise-Abnahme.
/// * **quantisiert mit Dithering**: Floyd-Steinberg gegen die gemeinsame
///   Palette.
/// * **transparent**: Alpha 0 bekommt den transparenten Index, ohne eine
///   Palette zu fragen.
fn neu_indizieren(bild: &Bild, gemeinsam: &GemeinsamePalette, dither: bool) -> Vec<u8> {
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;

    // Die exakte Tabelle kommt aus der PALETTE, nicht aus den Bildfarben:
    // bei einer Animation hat jedes Bild nur einen Teil der globalen
    // Palette, aber die Palette enthaelt jede vorkommende Farbe genau.
    let tab = farbe::Farbindex::neu_exakt(gemeinsam.palette.clone(), gemeinsam.palette.verlustfrei);

    let mut indizes = vec![0u8; breite * hoehe];
    let transparent = gemeinsam.transparenter_index;
    let passt_0 = gemeinsam.palette.len() > 1;

    if dither && !tab.ist_exakt() {
        // --- Floyd-Steinberg gegen die gemeinsame Palette ---
        let mut fehler_oben: Vec<i32> = vec![0; breite * 3];
        let mut fehler_unten: Vec<i32> = vec![0; breite * 3];
        for y in 0..hoehe {
            fehler_oben.copy_from_slice(&fehler_unten);
            fehler_unten.iter_mut().for_each(|v| *v = 0);
            for x in 0..breite {
                let i = (y * breite + x) * 4;
                if bild.rgba[i + 3] == 0 {
                    indizes[y * breite + x] = transparent.unwrap_or(0);
                    continue;
                }
                let mut rgb = [
                    bild.rgba[i] as i32,
                    bild.rgba[i + 1] as i32,
                    bild.rgba[i + 2] as i32,
                ];
                for k in 0..3 {
                    rgb[k] = (rgb[k] + fehler_oben[x * 3 + k]).clamp(0, 255);
                }
                let mut idx = tab.index_rgb_exakt(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
                if transparent == Some(0) && idx == 0 && passt_0 {
                    idx = 1;
                }
                indizes[y * breite + x] = idx;
                let ziel = gemeinsam.palette.farben[idx as usize];
                for k in 0..3 {
                    let fehler = rgb[k] - ziel[k + 1] as i32;
                    if x > 0 {
                        fehler_oben[(x - 1) * 3 + k] += fehler * 7 / 16;
                    }
                    if x + 1 < breite {
                        fehler_oben[(x + 1) * 3 + k] += fehler * 3 / 16;
                    }
                    if y + 1 < hoehe {
                        fehler_unten[x * 3 + k] += fehler * 5 / 16;
                        if x > 0 {
                            fehler_unten[(x - 1) * 3 + k] += fehler / 16;
                        }
                    }
                }
            }
        }
    } else {
        for y in 0..hoehe {
            for x in 0..breite {
                let i = (y * breite + x) * 4;
                if bild.rgba[i + 3] == 0 {
                    indizes[y * breite + x] = transparent.unwrap_or(0);
                    continue;
                }
                let mut idx = tab.index_rgb_exakt(bild.rgba[i], bild.rgba[i + 1], bild.rgba[i + 2]);
                // Der Index 0 ist fuer Transparenz reserviert: ein
                // undurchsichtiges Pixel darf ihn nicht bekommen.
                if transparent == Some(0) && idx == 0 && passt_0 {
                    idx = 1;
                }
                indizes[y * breite + x] = idx;
            }
        }
    }
    indizes
}

/// Schreibt die LZW-Daten in Unterbloecke von hoechstens 255 Byte.
///
/// Die Unterbloecke sind das, was die Blockgroesse begrenzt; jeder
/// Block ist `[Laenge][Daten...]`, und die Kette endet mit einem
/// Laengenbyte 0.
fn lzw_schreiben(
    out: &mut Vec<u8>,
    indizes: &[u8],
    mindest_breite: u32,
) -> Result<(), KonvertFehler> {
    // Die Mindestcodebreite muss mindestens 2 sein (1 Bit kann keine
    // Farbpalette beschreiben; GIF erlaubt ab 2).
    let mindest = mindest_breite.clamp(2, 8) as u8;
    out.push(mindest);

    let mut s = LzwSchreiber {
        bitpuffer: 0,
        bitzahl: 0,
        unterblock: Vec::with_capacity(255),
        out,
    };

    // --- Der Enkoder ---
    //
    // GIF-LZW, wortgleich nach dem Leser in `narsil-image/src/gif.rs:205`:
    //
    //   * `naechster` beginnt bei `clear + 2` (NICHT `clear + 1`).
    //   * Nach JEDEM ausgegebenen Code kommt EIN Eintrag dazu — auch im
    //     „KwKwK"-Fall. Das ist der eine Schritt, an dem die erste Fassung
    //     des Lesers gescheitert ist (`vp8.rs:298`), und er gilt fuer den
    //     Enkoder genauso.
    //   * DIE BREITENREGEL: `if naechster == (1 << breite) && breite < 12`.
    //     **Gleichheit, nicht groesser.**
    //
    //     GEMESSEN 02.10.2026: der Test
    //     `gif_lzw_breitenwechsel_bei_rauschen` bekam mit `>` ein Bild,
    //     das der Leser als „zu viele Datenbloecke in einer Kette" ablehnte
    //     — der Leser hat den Datenstrom nach der Breitenerhoehung um
    //     ein Bit verschoben gelesen und alles daraus verwertet. Der
    //     Unterschied zwischen `>` und `==` ist EIN Bit, und es ist der
    //     Unterschied zwischen einer lesbaren und einer kaputten Datei.
    let clear = 1u16 << mindest;
    let ende = clear + 1;
    let mut dikt: std::collections::HashMap<(u16, u8), u16> =
        std::collections::HashMap::with_capacity(4096);
    let mut breite = mindest + 1;
    let mut naechster = clear + 2;

    if indizes.is_empty() {
        // Leerer Indexstrom: Clear, dann Ende. Das ist ein gueltiges
        // leeres GIF-Bild und kein Fehler.
        s.zeichen(clear, breite);
        s.zeichen(ende, breite);
        return s.abschliessen();
    }
    s.zeichen(clear, breite);

    // DER EINE REIHENFOLGEPUNKT, an dem GIF-LZW scheitert
    // --------------------------------------------------
    //
    // Der Leser (`narsil-image/src/gif.rs:205`) traegt einen neuen
    // Woerterbucheintrag **NACH** dem Lesen des Codes, zu dem er gehoert:
    //
    //   lese C_1 (vorher = None -> kein Eintrag)
    //   lese C_2 -> Eintrag (C_1, erstes Zeichen von C_2), dann evtl. breiter
    //   lese C_3 -> Eintrag (C_2, erstes Zeichen von C_3), dann evtl. breiter
    //
    // Der Enkoder muss also: C_1 schreiben, C_2 schreiben, DANN den Eintrag
    // zu C_1 eintragen, DANN C_3 schreiben. Ein Enkoder, der den Eintrag
    // sofort nach dem Schreiben von C_1 eintraegt, erhoeht die Breite
    // EINEN Code zu frueh — und ab dort ist jedes Bit um eins verschoben.
    //
    // GEMESSEN 02.10.2026: genau so sah es aus. `file` und der Leser des
    // Projekts erkannten die Datei, PIL meldete „broken data stream", und
    // der Fehler trat erst auf, sobald das Woerterbuch genug Eintraege fuer
    // eine Breitenerhoehung hatte (deshalb funktionierten 1x1 und 2x1 und
    // 9x1 nicht, 1x9 schon nicht mehr).
    //
    // `offen` traegt den Eintrag, der nach dem NAECHSTEN Schreibvorgang
    // faellig wird: (Code von vorher, erstes Zeichen des naechsten Codes).
    // `seit_clear` zaehlt, wie viele Codes seit dem letzten Clear schon
    // geschrieben wurden. Der Leser traegt fuer den ERSTEN Code nach einem
    // Clear keinen Eintrag ein (`vorher == None`), und genau das muss der
    // Enkeder nachbilden.
    let mut offen: Option<(u16, u8)> = None;
    let mut seit_clear: u32 = 0;
    let mut praefix: u16 = indizes[0] as u16;
    let mut i = 1usize;
    while i < indizes.len() {
        let k = indizes[i];
        match dikt.get(&(praefix, k)).copied() {
            // Bekannte Erweiterung: den laufenden Code weiterschieben,
            // ohne etwas auszugeben.
            Some(c) => {
                praefix = c;
                i += 1;
            }
            // Neue Erweiterung: den laufenden Code ausgeben, DANN den
            // Eintrag des VORHERIGEN Codes eintragen, dann mit `k`
            // weitermachen.
            None => {
                // DER CODE ZUERST, DANN DER EINTRAG.
                //
                // Der Leser traegt den Eintrag zu Code_{j-1} erst NACH dem
                // Lesen von Code_j ein. Der Enkoder muss ihn also auch erst
                // nach dem Schreiben von Code_j eintragen. Tragt er ihn
                // vorher ein, erhoeht er die Breite einen Code zu frueh —
                // und der Leser liest ab der ersten Breitenerhoehung jedes
                // Codewort um eine Stelle zu weit.
                //
                // GEMESSEN 02.10.2026 an PIL, „broken data stream": der
                // 320x240-Vergleich mit dem Bild, das der Leser daraus
                // machte, zeigte nach dem zweiten Clear (Pixel 59686 von
                // 76800) den Code 634 an, den der Leser gar nicht kannte —
                // 634 = 122 + 512, also ein um eine Stelle zu weit gelesenes
                // 9-Bit-Codewort.
                s.zeichen(praefix, breite);
                let mut zurueckgesetzt = false;
                if seit_clear > 0 {
                    let (c_vor, z_vor) = offen.expect("seit_clear > 0 heisst: offen ist gesetzt");
                    if naechster < MAX_LZW_CODES {
                        dikt.insert((c_vor, z_vor), naechster);
                        naechster += 1;
                        if naechster as usize == (1usize << breite) && breite < MAX_LZW_LAENGE {
                            breite += 1;
                        }
                    } else {
                        // Woerterbuch voll: loeschen und neu anfangen. Ohne
                        // das wuerde `naechster` 4096 ueberschreiten und der
                        // Leser wuerde einen Code lesen, den es nicht gibt.
                        s.zeichen(clear, breite);
                        dikt.clear();
                        breite = mindest + 1;
                        naechster = clear + 2;
                        seit_clear = 0;
                        zurueckgesetzt = true;
                    }
                }
                // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 an
                // PIL, „broken data stream", beim 320x240-Vergleich mit
                // 76800 Bildpunkten): `seit_clear` wurde auch nach einem
                // Clear um eins erhoeht und `offen` wieder gesetzt. Damit
                // damit legte der Enkoder fuer den ERSTEN Code nach einem
                // einen Woerterbucheintrag, den der Leser nicht anlegt
                // (`vorher == None`). Nach jedem Clear war der Enkoder
                // genau einen Eintrag zu weit — und bei der naechsten
                // Breitenerhoehung las der Leser ein um eine Stelle zu
                // weites Codewort.
                if !zurueckgesetzt {
                    seit_clear += 1;
                    offen = Some((praefix, k));
                }
                praefix = k as u16;
                i += 1;
            }
        }
    }
    // Der letzte laufende Code.
    s.zeichen(praefix, breite);
    // Und sein Eintrag: der Leser traegt ihn ein, BEVOR er den
    // Endcode liest — also muss die Breite hier schon stimmen.
    if seit_clear > 0 {
        let (c_vor, z_vor) = offen.expect("seit_clear > 0 heisst: offen ist gesetzt");
        if naechster < MAX_LZW_CODES {
            dikt.insert((c_vor, z_vor), naechster);
            naechster += 1;
            if naechster as usize == (1usize << breite) && breite < MAX_LZW_LAENGE {
                breite += 1;
            }
        }
    }
    s.zeichen(ende, breite);
    s.abschliessen()
}

/// Der Bit- und Blockschreiber des LZW-Teils.
struct LzwSchreiber<'a> {
    bitpuffer: u32,
    bitzahl: u32,
    unterblock: Vec<u8>,
    out: &'a mut Vec<u8>,
}

impl LzwSchreiber<'_> {
    /// Schreibt einen Code mit gegebener Breite. GIF schreibt Codes
    /// **LSB zuerst** — das Gegenteil von JPEG. Verwechselt man das, wird
    /// jedes Bild zu farbigem Rauschen (und der Leser bricht nach den
    /// ersten Block ab).
    fn zeichen(&mut self, code: u16, breite: u8) {
        self.bitpuffer |= (code as u32) << self.bitzahl;
        self.bitzahl += u32::from(breite);
        while self.bitzahl >= 8 {
            self.byte((self.bitpuffer & 0xFF) as u8);
            self.bitpuffer >>= 8;
            self.bitzahl -= 8;
        }
    }

    fn byte(&mut self, b: u8) {
        self.unterblock.push(b);
        if self.unterblock.len() == 255 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if !self.unterblock.is_empty() {
            self.out.push(self.unterblock.len() as u8);
            self.out.extend_from_slice(&self.unterblock);
            self.unterblock.clear();
        }
    }

    /// Fuellt das letzte Byte auf (mit Nullen), schreibt die restlichen
    /// Bloecke und die Endemarke 0.
    fn abschliessen(mut self) -> Result<(), KonvertFehler> {
        if self.bitzahl > 0 {
            let b = (self.bitpuffer & 0xFF) as u8;
            self.byte(b);
        }
        self.flush();
        self.out.push(0);
        // M4: Die Ausgabe ist aus der Eingabe gewachsen; pruefen, dass sie
        // die Grenze nicht gesprengt hat.
        grenzen::pruefe_laenge(self.out.len(), grenzen::MAX_SPEICHER, "GIF-LZW-Ausgabe")?;
        Ok(())
    }
}

/// Die Zahl der Bits je Spalte fuer ein Bild der Breite `breite`.
///
/// Das GIF-Feld im Bilddeskriptor ist die „minimale Codebreite minus 1".
/// Es ist die Breite, die fuer EINE ZEILE noetig ist — nicht fuer das
/// ganze Bild. Wer die Breite des ganzen Bildes nimmt, laesst die
/// spaeteren Zeilen mit zu grossen Codes lesen.
#[must_use]
pub fn spurweite(breite: u32) -> u32 {
    let mut b = 1u32;
    while b < breite.max(1) {
        b <<= 1;
    }
    // GIF erlaubt 2..8 Bit, also muss das Ergebnis in [2, 8] liegen.
    b.clamp(2, 8)
}

/// Rundet auf die naechste Zweierpotenz auf — die Farbtabellengroesse im
/// GIF muss eine Zweierpotenz sein.
#[must_use]
pub fn aufrunden_zweierpotenz(n: usize) -> u32 {
    let mut k = 1u32;
    while (k as usize) < n && k < 256 {
        k <<= 1;
    }
    k.max(2)
}

fn ueberlauf() -> KonvertFehler {
    KonvertFehler::SpeicherZuGross {
        bytes: u64::MAX,
        grenze: grenzen::MAX_SPEICHER,
    }
}
