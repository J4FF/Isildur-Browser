//! WebP LESEN: der RIFF-Behaelter und der verlustfreie Zweig VP8L.
//!
//! # Stand 01.10.2026, 03:10: VP8L ist NICHT fertig. Bewusst ein Stub.
//!
//! Es gab einen ~700-zeiligen Decoder, der fast alles konnte: RIFF, die
//! vier Transformationen, den kanonischen Praefixcode, den Farb-Cache,
//! die Meta-Praefixcodes. Er lieferte aber **keine korrekten Pixel** —
//! der LZ77-Schritt (Rueckverweislaenge und -distanz) laeuft auseinander,
//! und nach zehn Fehlerkorrekturen war der Fehler nicht eingekreist.
//!
//! Ein Decoder, der stillschweigend falsche Bilder liefert, ist schlimmer
//! als einer, der einen benannten Fehler gibt. Genau das ist heute Nacht
//! beim GIF passiert, und die Folge war ein Rollback. Deshalb:
//! [`webp_lesen`] gibt `WebpDatenFalsch` und sonst nichts.
//!
//! Wer das aufnimmt, hat mit [`crate::webp`] einen gemessenen
//! Ausgangspunkt und muss nicht bei null anfangen. Was gesichert ist:
//!
//! # Was gesichert ist (alle Punkte GEMESSEN am 01.10.2026)
//!
//! ## Der RIFF-Behaelter — fertig, benutzt
//!
//! `RIFF` + Groesse + `WEBP`, dann Chunks aus `FourCC` + `uint32`-Groesse.
//! GEMESSEN an sechs von `cwebp 1.5.0` erzeugten Dateien: die
//! **Chunk-Groesse ist NICHT auf gerade gerundet** — `vp8l_alpha.webp`
//! nennt 101 (ungerade) und die Datei ist 122 Byte, es folgt also ein
//! Fuellbyte. Gerade ist die RIFF-**Datei**groesse. Wer
//! `nutzlast + groesse` ohne den Fuellbyte rechnet, laeuft bei ungeraden
//! Chunks in den naechsten Chunk hinein oder ans Dateiende vorbei.
//!
//! ## Der VP8L-Kopf — fertig
//!
//! Byte `0x2F`, dann 14 Bit `Breite-1`, 14 Bit `Hoehe-1`, 1 Bit
//! `alpha_is_used`, 3 Bit Version (nur 0). 14 Bit heisst: groesstens
//! 16384x16384.
//!
//! ## Der Farb-Cache — ein Widerspruch in der Spezifikation
//!
//! Entwurf draft-zern-webp-11 Abschnitt 3.6.2.3: `color_cache_code_bits`
//! liegt in [1..11], und „Compliant decoders MUST indicate a corrupted
//! bitstream for other values". GEMESSEN mit cwebp 1.5.0: **6 von 6**
//! erzeugten Dateien schreiben den Wert **12**, und dwebp 1.5.0
//! dekodiert sie einwandfrei. Der Wert 12 ist also gangbar, obwohl der
//! Entwurf ihn verbietet. [`MAX_CACHE_BITS`] = 12 nimmt ihn an (alles
//! darueber wird mit benanntem Fehler abgewiesen; 2^12 = 16 KiB Cache).
//!
//! ## Drei Stellen, an denen der IETF-Entwurf irrefuehrend ist
//!
//! Alles gegen `libwebp main`, `src/dec/vp8l_dec.c`, gegengelesen:
//!
//! 1. **Einfacher Code**: `code_lengths[symbol] = 1` — auch bei EINEM
//!    Symbol. Die Ausnahme „single leaf node tree" (unvollstaendiger Baum)
//!    ist erlaubt. Beim Lesen werden so viele Bit verbraucht, wie die
//!    Laenge des Symbols ist — nicht null, und nicht immer eins.
//! 2. **`max_symbol`**: der Entwurf (3.7.2.1) sagt, ohne das 1-Bit-Flag
//!    gelte `num_code_lengths` als Obergrenze. Die Referenz uebergibt
//!    aber `num_symbols = alphabet_size` an `ReadHuffmanCodeLengths` und
//!    benutzt **das**. GEMESSEN: mit der Entwurfslesart ist der
//!    Bildstrom nach dem ersten Praefixcode verschoben und jede Datei
//!    bricht ab. Ab `max_symbol > alphabet_size` ist die Datei falsch.
//! 3. **Wiederholungen** (Codes 16/17/18) muessen **ablehnen**, wenn sie
//!    ueber das Alphabet hinauslaufen — sie stillschweigend kuerzen waere
//!    eine abgeschnittene Laengenliste als gueltig anzunehmen.
//!
//! Ausserdem: `4 + ReadBits(4)` (max 19) ist `num_code_lengths` und wird
//! VOR der Vorstufe gelesen; verwechselt man beides, liest man die
//! falsche Bitzahl.
//!
//! ## Die Reihenfolge, die alles bestimmt
//!
//! * Fuenf Praefixcodes je Gruppe: **gruen (+Laenge +Cache), rot, blau,
//!   alpha, distanz**. Die Reihenfolge bestimmt, wohin die Bits gelesen
//!   werden; sie ist in libwebp `kAlphabetSize` und im Entwurf gleich.
//! * Innerhalb eines Literalpunkts: gruen, dann **rot, blau, alpha**.
//!   Alpha zuerst zu lesen zerlegt jede Datei ab dem zweiten Punkt.
//! * Die **Daten einer Transformation stehen an der Stelle der
//!   Transformation** im Strom (Prädiktorbild, Farbelementbild,
//!   Farbtabelle) — nicht danach. Sonst ist der ganze Bildstrom um die
//!   Laenge des Prädiktorbildes verschoben (16 Byte bei 16x16, 96 bei
//!   96x96).
//! * Transformationen werden **umgekehrt** aufgehoben: der zuerst
//!   gelesene Transform ist der aeusserste.
//! * Meta-Praefixcodes: `max((pixel >> 8) & 0xffff) + 1` Gruppen, und
//!   der Meta-Code ist das **gruene** Byte des Entropie-Punktes. Das
//!   Entropiebild selbst hat **kein** Meta-Bit und einen **eigenen**
//!   Farb-Cache.
//! * Die **Farbindex-Transformation verengt den Bildstrom** vor dem
//!   Entropiekodieren: bei 32x32 mit 4 Farben werden 8x32 Punkte
//!   kodiert. Nimmt man die volle Breite, stimmen ab der halben Hoehe
//!   die Punkte nicht **und** die LZ77-Distanzen rechnen mit der
//!   falschen Zeilenlaenge.
//!
//! ## Die beiden offenen Punkte
//!
//! 1. **Rueckverweis-Distanz** (`GetCopyDistance`, Entwurf 3.6.2.2):
//!    `if (code < 4) code + 1; extra = (code-2) >> 1; offset =
//!    (2 + (code & 1)) << extra; return offset + ReadBits(extra) + 1;`
//!    und dann die 120er-Nachbarschaftstabelle
//!    (`(xi, yi) = map[i-1]; dist = xi + yi * xsize; if (dist < 1) dist = 1`).
//!    GEMESSEN bei `vp8l_gross.webp` (96x96): nach 11 Punkten kam eine
//!    Distanz von 96 heraus — bei nur 11 dekodierten Punkten. Entweder
//!    stimmt die Abbildung nicht, oder die Laengen-/Distanzcodes werden
//!    vertauscht gelesen, oder der Cache-Zugriff davor verschiebt den
//!    Strom. **Nicht eingekreist.**
//! 2. **Die Prädiktoren 0..13** sind nach dem Entwurf (3.5.1) und
//!    `PredictorAdd()` in `src/dec/alpha_dec.c`/`vp8l_dec.c` abgeschrieben,
//!    aber nie gegen ein Bild verifiziert, weil der Decoder vorher
//!    abbricht.
//!
//! # Testdaten sind da und geprueft
//!
//! `/home/jaff/mutation_md/bild/mkwebp.py` erzeugt sechs echte Dateien mit
//! `cwebp 1.5.0`, Orakel ist `dwebp 1.5.0`:
//!
//! | Datei                    | Chunk | Groesse | Besonderheit                    |
//! |--------------------------|-------|---------|---------------------------------|
//! | `vp8l_einfach.webp`      | VP8L  |  66 B   | Praediktor + Farbindex (43 Farben) |
//! | `vp8l_alpha.webp`        | VP8L  | 122 B   | Gruen-abziehen, Cache 2 Bit, **Meta** |
//! | `vp8l_palette.webp`      | VP8L  |  84 B   | Farbindex, 4 Farben, **Meta**   |
//! | `vp8l_gross.webp`        | VP8L  | 308 B   | 96x96, Cache 12 Bit             |
//! | `vp8l_lossy_q90.webp`    | `VP8 `| 174 B   | verlustbehaftet                 |
//! | `vp8l_lossy_q50.webp`    | `VP8 `| 128 B   | verlustbehaftet                 |
//!
//! Dazu je eine PPM mit den Punkten von dwebp.
//!
//! # Der andere gemessene Fund: was „verlustfrei" heisst
//!
//! libwebp schreibt im verlustfreien Modus bei **vollstaendig
//! durchsichtigen** Punkten absichtlich Unsinns-RGB hin, um Bits zu
//! sparen. GEMESSEN an `vp8l_alpha.webp`: **52 von 256 Punkten** weichen
//! ab, alle mit Alpha 0. Die richtige Zusicherung ist darum: **Alpha exakt,
//! und RGB exakt ueberall dort, wo `alpha != 0`.** Wer auf RGB-Gleichheit
//! ohne diese Einschraenkung prueft, haelt einen richtigen Dekoder fuer
//! falsch.
//!
//! # Grenzen (M3, M4)
//!
//! * [`MAX_CACHE_BITS`] = 12 (GEMESSEN, siehe oben).
//! * [`MAX_TRANSFORMS`] = 4 — jede Art genau einmal, mehr kann nicht
//!   vorkommen.

use crate::{Bild, BildFehler};

/// Hoechste Zahl Bit fuer den Farb-Cache (GEMESSEN: cwebp 1.5.0 = 12,
/// obwohl der Entwurf nur [1..11] erlaubt).
pub const MAX_CACHE_BITS: u32 = 12;

/// Hoechste Zahl Transformationen je Bild.
pub const MAX_TRANSFORMS: usize = 4;

/// Was steht in einer WebP-Datei?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebpArt {
    /// `VP8L` — verlustfrei.
    Verlustfrei,
    /// `VP8 ` — verlustbehaftet (Key Frame nach RFC 6386).
    Verlustbehaftet,
    /// `VP8X` — erweitertes Format, z. B. mit Animation oder Alpha-Chunk.
    Erweitert,
}

/// Liest die Chunk-Liste, ohne Bilddaten zu dekodieren.
///
/// Das ist der billige Teil, und ein Browser braucht genau das, um zu
/// entscheiden, welchen Zweig er ueberhaupt nehmen muss. Es ist
/// **fertig und geprueft**: [`crate::webp::tests`] testet es gegen
/// `webpinfo`-Gemessenes.
///
/// M3: hoechstens 1024 Chunks je Datei. Der kleinste sinnvolle Chunk
/// besteht aus 8 Byte Kopf, das passt also 512 Mal in 4096 Byte.
pub fn webp_chunks(daten: &[u8]) -> Result<Vec<([u8; 4], usize, usize)>, BildFehler> {
    if daten.len() < 12 {
        return Err(BildFehler::ZuKurz {
            vorhanden: daten.len(),
            gebraucht: 12,
        });
    }
    if &daten[..4] != b"RIFF" {
        return Err(BildFehler::WebpDatenFalsch {
            grund: "kein RIFF-Kopf",
        });
    }
    if &daten[8..12] != b"WEBP" {
        return Err(BildFehler::WebpDatenFalsch {
            grund: "RIFF-Name ist nicht WEBP",
        });
    }
    let riiff = u32::from_le_bytes([daten[4], daten[5], daten[6], daten[7]]) as usize;
    // M4: die RIFF-Groesse darf nicht ueber die Datei hinauszeigen.
    // GEMESSEN 01.10.2026: cwebp schreibt genau Dateigroesse - 8.
    if riiff + 8 > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: 4,
            gebraucht: riiff + 8 - daten.len(),
        });
    }
    let ende = riiff + 8;
    chunk_kette(daten, 12, ende, 1024)
}

/// Die Chunk-Kette zwischen `pos` und `ende` (beide Byte-Offsets in
/// `daten`).
///
/// M3: hoechstens `grenze` Chunks. Jeder Chunk besteht aus mindestens
/// 8 Byte Kopf, die Grenze haengt also nicht von der Dateigroesse ab.
/// M4: ein Chunk, dessen Nutzlast ueber `ende` hinausliegt, ist ein
/// benannter Fehler (`Abgeschnitten`) und kein Ueberlauf.
fn chunk_kette(
    daten: &[u8],
    pos: usize,
    ende: usize,
    grenze: usize,
) -> Result<Vec<([u8; 4], usize, usize)>, BildFehler> {
    if ende > daten.len() {
        return Err(BildFehler::Abgeschnitten {
            position: ende,
            gebraucht: ende - daten.len(),
        });
    }
    let mut raus = Vec::new();
    let mut pos = pos;
    while pos + 8 <= ende {
        if raus.len() >= grenze {
            return Err(BildFehler::WebpDatenFalsch {
                grund: "mehr Chunks als erlaubt",
            });
        }
        let viercc = [daten[pos], daten[pos + 1], daten[pos + 2], daten[pos + 3]];
        let groesse = u32::from_le_bytes([
            daten[pos + 4],
            daten[pos + 5],
            daten[pos + 6],
            daten[pos + 7],
        ]) as usize;
        let nutzlast = pos + 8;
        // M4: `checked_add` — sonst wuerde ein Chunk mit_fast_ maximaler
        // Groesse ueberlaufen und `nutzlast + groesse` kleiner als
        // `nutzlast` werden, der Vergleich ginge dann durch.
        let ende_chunk = match nutzlast.checked_add(groesse) {
            Some(e) => e,
            None => {
                return Err(BildFehler::WebpDatenFalsch {
                    grund: "Chunk-Groesse laeuft ueber",
                });
            }
        };
        if ende_chunk > ende {
            return Err(BildFehler::Abgeschnitten {
                position: nutzlast,
                gebraucht: groesse,
            });
        }
        raus.push((viercc, nutzlast, groesse));
        // Auf ungerade Groessen folgt ein Fuellbyte.
        pos = match ende_chunk.checked_add(groesse & 1) {
            Some(p) => p,
            None => break,
        };
    }
    Ok(raus)
}

/// Liest eine WebP-Datei.
///
/// `VP8L` (verlustfrei) ist **fertig**: der Dekoder steht in
/// [`crate::vp8l`] und ist ein Zeile-fuer-Zeile-Port der geprueften
/// Python-Referenz (`/home/jaff/mutation_md/bild/vp8l_ref.py`).
///
/// `VP8 ` (verlustbehaftet) und `VP8X` (Animation, Alpha-Chunk) sind nicht
/// begonnen. Deren Fehler nennt den fehlenden Zweig — nicht „unbekanntes
/// Verfahren", denn das Verfahren ist hier bekannt.

#[cfg(test)]
mod tests {
    use super::*;

    fn daten(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("daten")
                .join(name),
        )
        .unwrap_or_else(|e| {
            panic!(
                "Testdaten fehlt: {name} ({e}) — bitte python3 \
                     /home/jaff/mutation_md/bild/mkwebp.py \
                     crates/narsil-image/tests/daten ausfuehren"
            )
        })
    }

    const VERLUSTFREI: [&str; 4] = [
        "vp8l_einfach.webp",
        "vp8l_alpha.webp",
        "vp8l_palette.webp",
        "vp8l_gross.webp",
    ];

    #[test]
    fn chunk_tabelle_stimmt_mit_webpinfo_ueberein() {
        // GEMESSEN 01.10.2026 mit `webpinfo`:
        //   vp8l_einfach  66 B, Chunk VP8L ab 12, Laenge 46  (gerade)
        //   vp8l_alpha   122 B, Chunk VP8L ab 12, Laenge 110 (gerade)
        //   vp8l_gross   308 B, Chunk VP8L ab 12, Laenge 288 (gerade)
        // Diese drei sind ZUFALLSMAESSIG gerade. `vp8l_alpha` hat aber
        // in der Datei selbst 101 (ungerade) und damit ein Fuellbyte —
        // genau der Fall, an dem eine Rechnung ohne Fuellbyte daneben
        // liege. Deshalb wird hier beides geprueft: die Nutzlast reicht
        // bis ans Ende, und der Sprung zum naechsten Chunk ist
        // `+ (groesse & 1)`.
        let mut ende_richtig = true;
        for name in VERLUSTFREI {
            let d = daten(name);
            let c = webp_chunks(&d).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(c.len(), 1, "{name}: genau ein Chunk");
            assert_eq!(&c[0].0, b"VP8L", "{name}: Chunk-Typ");
            assert_eq!(c[0].1, 20, "{name}: Nutzlast ab Byte 20");
            if c[0].1 + c[0].2 + (c[0].2 & 1) != d.len() {
                ende_richtig = false;
            }
        }
        assert!(
            ende_richtig,
            "Nutzlast + Fuellbyte muss die Dateigroesse ergeben"
        );

        // Und die Odditaet ist nicht theoretisch: vp8l_alpha hat sie.
        let d = daten("vp8l_alpha.webp");
        let c = webp_chunks(&d).expect("liest");
        assert_eq!(
            c[0].2, 101,
            "GEMESSEN: ungerade Chunk-Groesse, ein Fuellbyte folgt"
        );
        assert_eq!(c[0].1 + c[0].2 + 1, d.len(), "122 Byte Datei");
    }

    #[test]
    fn verlustbehaftete_chunks_werden_erkannt_nicht_gelesen() {
        // Die Datei hat den Chunk `VP8 ` (mit Leerzeichen) — der vierte
        // Buchstabe ist ASCII 0x20, kein Nullbyte.
        let d = daten("vp8_lossy_q90.webp");
        let c = webp_chunks(&d).expect("liest");
        assert_eq!(&c[0].0, b"VP8 ", "der vierte Buchstabe ist ein Leerzeichen");
        assert_eq!(c[0].0[3], 0x20, "und zwar 0x20");
    }

    #[test]
    fn kaputte_containers_werden_benannt_abgelehnt() {
        assert_eq!(webp_chunks(b"RIFF").unwrap_err().name(), "ZuKurz");
        assert_eq!(webp_chunks(b"").unwrap_err().name(), "ZuKurz");

        let mut d = daten("vp8l_einfach.webp");
        d[0] = b'X';
        assert_eq!(webp_chunks(&d).unwrap_err().name(), "WebpDatenFalsch");

        let mut d = daten("vp8l_einfach.webp");
        d[8..12].copy_from_slice(b"WEBQ");
        assert_eq!(webp_chunks(&d).unwrap_err().name(), "WebpDatenFalsch");

        // RIFF-Groesse groesser als die Datei.
        let mut d = daten("vp8l_einfach.webp");
        d[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(webp_chunks(&d).unwrap_err().name(), "Abgeschnitten");

        // Ein Chunk, der aus der Datei herausragt.
        let mut d = daten("vp8l_einfach.webp");
        d[16..20].copy_from_slice(&9_000u32.to_le_bytes());
        assert_eq!(webp_chunks(&d).unwrap_err().name(), "Abgeschnitten");
    }

    #[test]
    fn vp8l_wird_gelesen_und_die_fehlenden_zweige_nennen_sich() {
        // M7: nicht „kein Fehler", sondern der NAME des Fehlers, und der
        // Text muss sagen, welcher Zweig fehlt.
        //
        // BIS 01.10.2026 galt dieser Test fuer ALLE WebP-Dateien, weil
        // `webp_lesen` immer `WebpDatenFalsch` gab. VP8L ist jetzt fertig
        // (Modul `vp8l`), also prueft der Test die EINZIGEN Zweige, die es
        // noch nicht gibt — und jede der vier VP8L-Dateien muss durchgehen.
        for name in [
            "vp8l_einfach.webp",
            "vp8l_alpha.webp",
            "vp8l_palette.webp",
            "vp8l_gross.webp",
        ] {
            let b = webp_lesen(&daten(name))
                .unwrap_or_else(|e| panic!("{name}: VP8L wird jetzt gelesen, aber: {e}"));
            assert!(!b.rgba.is_empty(), "{name}: leeres Bild");
        }
        // VP8 ist nun auch implementiert, daher sollten diese Dateien gelesen werden.
        for name in ["vp8_lossy_q90.webp", "vp8_lossy_q50.webp"] {
            let b = webp_lesen(&daten(name)).unwrap();
            assert!(!b.rgba.is_empty(), "{name}: leeres Bild");
        }
    }

    #[test]
    fn ein_webp_ohne_bildchunk_wird_benannt_abgewiesen() {
        // RIFF/WEBP, aber ohne VP8L/VP8/VP8X: das ist kein „kaputtes
        // Bild", sondern eine Datei ohne Bild. Der Fehler muss das sagen
        // und nicht raten.
        let mut d = Vec::new();
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&4u32.to_le_bytes());
        d.extend_from_slice(b"WEBP");
        let e = webp_lesen(&d).expect_err("ohne Bildchunk gibt es kein Bild");
        assert_eq!(e.name(), "WebpDatenFalsch");
        assert!(
            e.to_string().contains("VP8L"),
            "die Meldung muss die gesuchten Chunks nennen, war: {e}"
        );
    }

    #[test]
    fn grenzen_sind_festgelegt() {
        assert_eq!(MAX_CACHE_BITS, 12);
        assert_eq!(MAX_TRANSFORMS, 4);
    }
}
/// Der Alphakanal aus einem `ALPH`-Chunk: eine Zahl je Pixel.
///
/// Der Chunk beginnt mit einem Kopfbyte, dessen Felder in
/// `src/dec/alpha_dec.c` Zeile 80-83 stehen:
///
/// ```text
/// method         = (kopf >> 0) & 3   0 = rohe Samples, 1 = verlustfrei
/// filter         = (kopf >> 2) & 3   0 keine, 1 waagerecht,
///                                      2 senkrecht, 3 Gradient
/// pre_processing = (kopf >> 4) & 3   0 keine, 1..2 Farbraumstufen
/// ```
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B3) mit `cwebp` an
/// `y18.svg`-freien Testbildern: die Vorgabe von `cwebp -q 90
/// -alpha_q 100` liefert den Kopf `0x05` — also `method = 1`
/// (verlustfrei) und `filter = 1` (waagerecht). Mit `-alpha_filter
/// none` steht dort `0x01`.
///
/// Der verlustfreie Weg ist ein VP8L-Bildstrom **ohne** den
/// fuenf Byte grossen Kopf (Signatur, Breite, Hoehe, Alpha-Bit,
/// Version); die Groesse steht im Container. Die Alphawerte stehen im
/// GRUENEN Kanal.
///
/// Die rueckwaerts gerichteten Filter stehen in `src/dsp/filters.c`
/// (`HorizontalUnfilter_C`, `VerticalUnfilter_C`,
/// `GradientUnfilter_C`, `GradientPredictor_C`) — dort auch die
/// Klemmung auf 8 Bit: `((g & ~0xff) == 0) ? g : (g < 0) ? 0 : 255`.
///
/// M3/M4: `breite * hoehe` ist mit `checked_mul` gerechnet und gegen
/// [`crate::MAX_BILDPUNKTE`] geprueft, jeder Feldzugriff gegen `get`.
/// Ein `pre_processing` groesser 0 ist ein BENANNTER Fehler.
pub fn alph_ebene(daten: &[u8], breite: u32, hoehe: u32) -> Result<Vec<u8>, BildFehler> {
    let kopf = *daten.first().ok_or(BildFehler::Abgeschnitten {
        position: 0,
        gebraucht: 1,
    })?;
    let method = kopf & 3;
    let filter = (kopf >> 2) & 3;
    let pre = (kopf >> 4) & 3;
    if pre != 0 {
        return Err(BildFehler::WebpDatenFalsch {
            grund: "ALPH mit Farbraum-Vorverarbeitung wird nicht gelesen",
        });
    }
    if method > 1 {
        return Err(BildFehler::WebpDatenFalsch {
            grund: "ALPH mit unbekanntem Kompressionsverfahren",
        });
    }
    let n = (breite as usize)
        .checked_mul(hoehe as usize)
        .ok_or(BildFehler::WebpDatenFalsch {
            grund: "Bildgroesse laeuft ueber",
        })?;
    let roh = &daten[1..];
    let mut ebene: Vec<u8> = if method == 0 {
        if roh.len() < n {
            return Err(BildFehler::Abgeschnitten {
                position: 1,
                gebraucht: n - roh.len(),
            });
        }
        roh[..n].to_vec()
    } else {
        // Die Alphawerte stehen im gruenen Kanal — GEMESSEN an sechs
        // `cwebp -q 90 -alpha_q 100`-Dateien gegen `dwebp`, siehe
        // `vp8l_strom_ohne_kopf`.
        let px = crate::vp8l::vp8l_strom_ohne_kopf(roh, breite, hoehe)?;
        if px.len() < n {
            return Err(BildFehler::WebpDatenFalsch {
                grund: "ALPH-Bildstrom hat zu wenige Punkte",
            });
        }
        // Die Alphawerte stehen im gruenen Byte. Bei einem mit Palette
        // kodierten Alphastrom hat `vp8l_strom_ohne_kopf` den Wert schon
        // an die richtige Stelle geschrieben (`ARGBIndexToAlpha`).
        px[..n].iter().map(|p| ((p >> 8) & 0xFF) as u8).collect()
    };
    match filter {
        0 => {}
        1 => {
            // GEMESSEN 02.10.2026 (Rolle BILD, B3) an `f_rechteck.webp`
            // und `webp_alphatest.webp` gegen `dwebp`: die Summe laeuft
            // mod 256, sie wird NICHT bei 255 abgeschnitten. libwebp
            // schreibt `out[i] = (uint8_t)(pred + in[i])`
            // (`src/dsp/filters.c` Zeile 170-179). Mit `min(255)` war
            // jede Stelle falsch, an der der Alphawert SPRINGT statt zu
            // steigen — beim waagerechten Filter kommen solche Sprünge
            // als Delta 255 minus Sprunghoehe: ab Spalte 16 steht das
            // Bild bei `f_rechteck.webp` wieder auf dem Verlauf, nach dem
            // Rechteck (255) muss der Wert also wieder fallen. Wir
            // klemmten auf 255 und blieben dort stehen: 10 von 32 Zeilen
            // falsch, die ersten 6 exakt.
            let b = breite as usize;
            for y in 0..(hoehe as usize) {
                let zeile = y * b;
                for x in 0..b {
                    let links: u8 = if x == 0 { 0 } else { ebene[zeile + x - 1] };
                    ebene[zeile + x] = links.wrapping_add(ebene[zeile + x]);
                }
            }
        }
        2 => {
            // GEMESSEN 02.10.2026 (Rolle BILD, B3) an
            // `/tmp/g_kreis.webp` (ALPH-Kopfbyte 0x09, also filter 2)
            // gegen `dwebp`: libwebps `VerticalUnfilter_C`
            // (`src/dsp/filters.c` Zeile 181-189) macht fuer die ERSTE
            // Zeile `if (prev == NULL) HorizontalUnfilter_C(NULL, in,
            // out, width)` — die erste Zeile wird also WAAGERECHT
            // gerechnet, erst die folgenden senkrecht. Wir hatten das
            // nur fuer den Gradienten (dort stimmte es) und liessen die
            // erste Zeile unveraendert: die Zeile blieb roh (0, 6, 7,
            // 6, 7, ...) statt der Summe (0, 6, 13, 19, 26, ...), und
            // damit war das ganze Bild falsch.
            let b = breite as usize;
            for y in 0..(hoehe as usize) {
                let zeile = y * b;
                for x in 0..b {
                    let vorhersage: u32 = if y == 0 {
                        // Erste Zeile: waagerecht, Vorhersage startet 0.
                        if x == 0 {
                            0
                        } else {
                            ebene[zeile + x - 1] as u32
                        }
                    } else {
                        ebene[zeile - b + x] as u32
                    };
                    // Auch hier mod 256, siehe Filter 1.
                    ebene[zeile + x] = (vorhersage as u8).wrapping_add(ebene[zeile + x]);
                }
            }
        }
        3 => {
            // `GradientUnfilter_C`: fuer die erste Zeile gilt der
            // waagerechte Filter, danach die Gradientenvorhersage
            // `a + b - c` mit a = links, b = oben, c = links oben.
            let b = breite as usize;
            for y in 0..(hoehe as usize) {
                let zeile = y * b;
                let (mut links, mut links_oben) = (0u32, 0u32);
                for x in 0..b {
                    let oben = if y == 0 {
                        0
                    } else {
                        ebene[zeile - b + x] as u32
                    };
                    let vorhersage = if y == 0 {
                        links
                    } else {
                        (links + oben).saturating_sub(links_oben)
                    };
                    // GEMESSEN: auch beim Gradientenfilter laeuft die
                    // Summe in `GradientUnfilter_C` mod 256
                    // (`left = (uint8_t)(in[i] + GradientPredictor_C(...))`,
                    // `src/dsp/filters.c` Zeile 192-205). Nur die
                    // Vorhersage selbst wird geklemmt (`g < 0 ? 0 : g > 255
                    // ? 255`), das ist `saturating_sub` oben.
                    let v = vorhersage + ebene[zeile + x] as u32;
                    ebene[zeile + x] = v as u8;
                    links = ebene[zeile + x] as u32;
                    links_oben = oben;
                }
            }
        }
        _ => {
            return Err(BildFehler::WebpDatenFalsch {
                grund: "ALPH mit unbekanntem Filter",
            });
        }
    }
    Ok(ebene)
}

/// Traegt einen Alphakanal in ein Bild ein (nur den Alphakanal; die
/// Farbkanaele bleiben, wie der Bilddecoder sie geliefert hat).
pub fn alpha_eintragen(bild: &mut Bild, alph: &[u8]) {
    for (i, p) in bild.rgba.chunks_exact_mut(4).enumerate() {
        if let Some(&a) = alph.get(i) {
            p[3] = a;
        }
    }
}

/// Liest eine WebP-Datei als EIN Bild.
///
/// Bei einer Animation wird das ERSTE zusammengesetzte Bild geliefert —
/// das ist, was ein Browser anzeigt, solange die Zeit stehen bleibt.
/// GEMESSEN 02.10.2026 (Rolle BILD, B3): vorher fand diese Funktion bei
/// einer animierten Datei keinen Bildchunk und meldete
/// `WebpDatenFalsch { grund: "kein VP8L- oder VP8-Chunk gefunden" }`, weil
/// das Bild INNERHALB der ANMF-Chunks steckt. Der Fehler ist BENANNT,
/// aber eine Animation ist damit nicht lesbar.
pub fn webp_lesen(daten: &[u8]) -> Result<Bild, BildFehler> {
    let chunks = webp_chunks(daten)?;
    if chunks
        .iter()
        // GEMESSENER FEHLER 02.10.2026 (Rolle BILD, B3): hier stand
        // `ANMF || VP8X`. Damit landeten auch die Alphabilder im
        // Animationszweig — `vp8_alpha_q80` und die anderen drei
        // schlugen danach in `tests/vp8_pixel.rs` fehl, denn ein VP8X
        // mit Alpha-Bit ohne ANMF ist KEINE Animation. Der Zweig
        // gehoert allein an ANMF.
        .any(|(t, _, _)| *t == *b"ANMF")
    {
        return match webp_lesen_alle(daten)?.bild(0) {
            Some(b) => Ok(b),
            None => Err(BildFehler::WebpDatenFalsch {
                grund: "VP8X-Datei ohne Bild",
            }),
        };
    }
    for &(tag, nutzlast, groesse) in &chunks {
        if tag == *b"VP8L" {
            return mit_alpha(
                daten,
                &chunks,
                crate::vp8l::vp8l_lesen(&daten[nutzlast..nutzlast + groesse])?,
            );
        }
    }
    for &(tag, nutzlast, groesse) in &chunks {
        if tag == *b"VP8 " {
            return mit_alpha(
                daten,
                &chunks,
                crate::vp8::vp8_lesen(&daten[nutzlast..nutzlast + groesse])?,
            );
        }
    }
    Err(BildFehler::WebpDatenFalsch {
        grund: "kein VP8L- oder VP8-Chunk gefunden",
    })
}

/// Haengt einen `ALPH`-Chunk an ein bereits gelesenes Bild, wenn die
/// Datei einen hat.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B3): die vier Alphabilder der Crate
/// (`vp8_alpha_q80`, `vp8_alpha_filter`, `vp8_alpha_q100`,
/// `vp8_alphaweich`) enthalten alle `VP8X ALPH VP8`. Der `ALPH`-Chunk
/// wurde nie gelesen — es fiel nur nicht auf, weil diese Bilder voll
/// staendig deckend sind und `tests/vp8_pixel.rs` nur RGB vergleicht.
/// GEMESSEN an `webp_alphatest.webp` (40x32, weicher Alpha-Verlauf, mit
/// `cwebp -q 90 -alpha_q 100` erzeugt): vorher hatte jeder Pixel Alpha
/// 255, nachher stimmt der Alphakanal pixelweise mit dem, was PIL aus
/// derselben Datei liest.
fn mit_alpha(
    daten: &[u8],
    chunks: &[([u8; 4], usize, usize)],
    mut bild: Bild,
) -> Result<Bild, BildFehler> {
    if let Some(&(_, nutzlast, groesse)) = chunks.iter().find(|(t, _, _)| t == b"ALPH") {
        let alph = alph_ebene(
            &daten[nutzlast..nutzlast + groesse],
            bild.breite,
            bild.hoehe,
        )?;
        alpha_eintragen(&mut bild, &alph);
    }
    Ok(bild)
}

/// Hoechste Zahl Bilder je WebP-Datei (M3, wie bei GIF).
pub const MAX_WEBP_BILDER: usize = 1000;

/// Was mit dem Bildschirm vor dem Zeichnen des naechsten Bildes passiert.
///
/// Nach der WebP-Spezifikation (VP8X / ANMF, „Animation“): nur zwei
/// Werte sind belegt, `0` (nichts tun) und `1` (das Rechteck des Bildes
/// auf die Hintergrundfarbe setzen). Der Wert `2` ist in der Spezifikation
/// reserviert und wird wie `0` behandelt — das ist keine Auslegung, das
/// steht so dort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebpEntsorgung {
    /// 0: das Bild bleibt stehen.
    Behalten,
    /// 1: das Rechteck des Bildes wird vor dem naechsten Bild auf die
    /// Hintergrundfarbe gesetzt.
    Hintergrund,
    /// 2 oder mehr: reserviert, wie 0.
    Unbekannt,
}

/// Ein Bild einer WebP-Animation (ein ANMF-Chunk).
#[derive(Debug, Clone, PartialEq)]
pub struct WebpFrame {
    /// Laufende Nummer dieses Bildes.
    pub nummer: usize,
    /// Das dekodierte Einzelbild in RGBA8, noch nicht zusammengesetzt.
    pub bild: Bild,
    /// Dauer dieses Bildes in Millisekunden.
    pub verzoegerung_ms: u32,
    /// Position der linken oberen Ecke im Bildschirm.
    pub versatz_x: u32,
    pub versatz_y: u32,
    /// Was nach diesem Bild passiert (ANMF-Flags, Bits 0 und 1).
    pub entsorgung: WebpEntsorgung,
    /// `true` heisst „mit Alpha mischen“, `false` heisst „ersetzen“
    /// (ANMF-Flags, Bit 1 — GEMESSEN mit `webpinfo`, siehe
    /// `webp_lesen_alle`).
    pub mischen: bool,
}

/// Alles, was eine WebP-Datei mit Animation enthaelt.
#[derive(Debug, Clone, PartialEq)]
pub struct Webp {
    /// Bildschirmbreite und -hoehe aus `VP8X` (24 Bit je Wert, plus 1).
    pub breite: u32,
    pub hoehe: u32,
    /// Die Bilder in Reihenfolge (je ein ANMF-Chunk).
    pub frames: Vec<WebpFrame>,
    /// Schleifenzahl aus `ANIM`; 0 heisst endlos.
    pub wiederholungen: u32,
    /// Hintergrundfarbe aus `ANIM` als RGBA.
    pub hintergrund: [u8; 4],
}

impl Webp {
    /// Das zusammengesetzte Bild nach `index` Bildern (0 = das erste
    /// Bild). `None`, wenn `index` ausserhalb liegt.
    ///
    /// Die Zusammensetzung folgt der Reihenfolge der Spezifikation: der
    /// Bildschirm wird mit der Hintergrundfarbe gefuellt, dann wird jedes
    /// Bild an seine Position gesetzt — gemischt mit Alpha, wenn das
    /// Mischen-Flag gesetzt ist, sonst ersetzt es sein Rechteck. Vor dem
    /// naechsten Bild wird das Rechteck des Vorgängers auf die
    /// Hintergrundfarbe gesetzt, wenn dieser „Aufraeumen“ verlangt.
    ///
    /// GEMESSEN 02.10.2026 (Rolle BILD, B3) an einer von PIL erzeugten
    /// Animation (3 Bilder, 32x32): die zusammengesetzten Bilder stimmen
    /// pixelweise mit den von PIL gerenderten Bildern ueberein (0 von
    /// 3072 Kanalwerten abweichend).
    pub fn bild(&self, index: usize) -> Option<Bild> {
        if index >= self.frames.len() {
            return None;
        }
        let mut schirm = Bild::new(self.breite, self.hoehe);
        for p in schirm.rgba.chunks_exact_mut(4) {
            p.copy_from_slice(&self.hintergrund);
        }
        for f in self.frames.iter().take(index + 1) {
            if f.mischen {
                schirm.ueberlagern(&f.bild, f.versatz_x, f.versatz_y);
            } else {
                schirm.ersetzen(&f.bild, f.versatz_x, f.versatz_y);
            }
            if f.entsorgung == WebpEntsorgung::Hintergrund && index != f.nummer {
                // Das Rechteck dieses Bildes wird vor dem naechsten Bild
                // auf die Hintergrundfarbe gesetzt. Beim allerletzten
                // Bild ist das nur sichtbar, wenn danach noch gefragt
                // wird — deshalb die Ausnahme.
                schirm.feld_aufraeumen(
                    f.versatz_x,
                    f.versatz_y,
                    f.bild.breite,
                    f.bild.hoehe,
                    self.hintergrund,
                );
            }
        }
        Some(schirm)
    }

    /// Alle zusammengesetzten Bilder in Reihenfolge.
    pub fn bilder(&self) -> Vec<Bild> {
        (0..self.frames.len())
            .filter_map(|i| self.bild(i))
            .collect()
    }

    /// Dauer einer Runde in Millisekunden.
    pub fn gesamtdauer_ms(&self) -> u64 {
        self.frames.iter().map(|f| f.verzoegerung_ms as u64).sum()
    }
}

/// Liest 24 Bit little endian.
fn lies24(daten: &[u8], pos: usize) -> u32 {
    (daten[pos] as u32) | ((daten[pos + 1] as u32) << 8) | ((daten[pos + 2] as u32) << 16)
}

/// Liest eine WebP-Datei mit Animation: alle ANMF-Bilder.
///
/// GEMESSEN 02.10.2026 (Rolle BILD, B3), Aufbau nach der Spezifikation
/// „WebP Container Specification“:
///
/// ```text
/// VP8X  10 Byte: 1 Byte Flags (Bit 1 = Animation, Bit 4 = Alpha),
///                3 Byte Breite - 1, 3 Byte Hoehe - 1
/// ANIM   6 Byte: 4 Byte Hintergrundfarbe (BGRA), 2 Byte Schleifenzahl
/// ANMF  16 Byte Bildkopf + Bilddaten:
///                3 Byte x, 3 Byte y, 3 Byte Breite - 1, 3 Byte Hoehe - 1,
///                3 Byte Dauer, 1 Byte Flags (Bits 0..1 Entsorgung,
///                Bit 2 Mischen), danach Chunks (VP8 / VP8L / ALPH)
/// ```
///
/// M3/M4: hoechstens [`MAX_WEBP_BILDER`] Bilder, der Bildschirm ist auf
/// [`crate::gif::MAX_PIXEL_ALLE_BILDER`] Pixel begrenzt (beides Werte aus
/// dem Programm, nicht aus der Datei), und jeder Feldzugriff ist gegen
/// `checked_add` abgesichert. Fehlt ein Bildchunk, ist das ein BENANNTER
/// Fehler.
pub fn webp_lesen_alle(daten: &[u8]) -> Result<Webp, BildFehler> {
    let chunks = webp_chunks(daten)?;
    let mut bildschirm: Option<(u32, u32)> = None;
    let mut hintergrund = [0u8, 0, 0, 0];
    let mut wiederholungen = 0u32;
    let mut frames: Vec<WebpFrame> = Vec::new();
    for &(tag, nutzlast, groesse) in &chunks {
        match &tag {
            b"VP8X" => {
                if groesse < 10 {
                    return Err(BildFehler::WebpDatenFalsch {
                        grund: "VP8X ist kleiner als 10 Byte",
                    });
                }
                bildschirm = Some((
                    lies24(daten, nutzlast + 4) + 1,
                    lies24(daten, nutzlast + 7) + 1,
                ));
            }
            b"ANIM" => {
                if groesse < 6 {
                    return Err(BildFehler::WebpDatenFalsch {
                        grund: "ANIM ist kleiner als 6 Byte",
                    });
                }
                // Die Hintergrundfarbe steht als BGRA.
                hintergrund = [
                    daten[nutzlast + 2],
                    daten[nutzlast + 1],
                    daten[nutzlast],
                    daten[nutzlast + 3],
                ];
                wiederholungen = daten[nutzlast + 4] as u32 | ((daten[nutzlast + 5] as u32) << 8);
            }
            b"ANMF" => {
                if groesse < 16 {
                    return Err(BildFehler::WebpDatenFalsch {
                        grund: "ANMF ist kleiner als 16 Byte",
                    });
                }
                if frames.len() >= MAX_WEBP_BILDER {
                    return Err(BildFehler::WebpDatenFalsch {
                        grund: "mehr Bilder als MAX_WEBP_BILDER",
                    });
                }
                let x = lies24(daten, nutzlast);
                let y = lies24(daten, nutzlast + 3);
                let w = lies24(daten, nutzlast + 6) + 1;
                let h = lies24(daten, nutzlast + 9) + 1;
                let dauer = lies24(daten, nutzlast + 12);
                let flags = daten[nutzlast + 15];
                // GEMESSEN 02.10.2026 (Rolle BILD, B3) mit `webpinfo`
                // an sechs Einzeldateien mit den Flag-Werten 0x00, 0x01,
                // 0x02, 0x03, 0x04 und 0x08:
                //     Flags 0x00: Dispose 0, Blend 0
                //     Flags 0x01: Dispose 1, Blend 0
                //     Flags 0x02: Dispose 0, Blend 1
                //     Flags 0x03: Dispose 1, Blend 1
                //     Flags 0x04: Dispose 0, Blend 0
                //     Flags 0x08: Dispose 0, Blend 0
                // Das Flagsbyte ist also: Bit 0 = Entsorgung (0 = nichts,
                // 1 = auf den Hintergrund zuruecksetzen), Bit 1 = Mischen
                // (0 = mit Alpha mischen, 1 = ersetzen), Rest reserviert.
                //
                // Zuvor stand hier `flags & 0x04` fuer das Mischen und
                // `flags & 0x03` fuer die Entsorgung — beides nach der
                // aelteren Fassung der Beschreibung, in der Bit 2 das
                // Mischen war. GEMESSENE Folge: jede Datei mit gesetztem
                // Mischen wurde wie „ersetzen“ behandelt.
                let entsorgung = match flags & 0x01 {
                    0 => WebpEntsorgung::Behalten,
                    _ => WebpEntsorgung::Hintergrund,
                };
                let mischen = flags & 0x02 == 0;
                // Chunks im Bild: erst der Bildinhalt, dann Alpha.
                let ende_bild = nutzlast + groesse;
                let bild_chunks = chunk_kette(daten, nutzlast + 16, ende_bild, 8)?;
                let mut bild_daten: Option<Vec<u8>> = None;
                let mut verlustfrei = false;
                for &(btag, bnutz, bgroesse) in &bild_chunks {
                    match &btag {
                        b"VP8L" => {
                            verlustfrei = true;
                            bild_daten = Some(daten[bnutz..bnutz + bgroesse].to_vec());
                        }
                        b"VP8 " => {
                            bild_daten = Some(daten[bnutz..bnutz + bgroesse].to_vec());
                        }
                        b"ALPH" => {
                            return Err(BildFehler::WebpDatenFalsch {
                                grund: "ALPH-Chunk (verlustbehaftetes Alpha) wird noch nicht gelesen",
                            });
                        }
                        _ => {}
                    }
                }
                let roh = match bild_daten {
                    Some(d) => d,
                    None => {
                        return Err(BildFehler::WebpDatenFalsch {
                            grund: "ANMF ohne VP8- oder VP8L-Chunk",
                        });
                    }
                };
                let mut bild = if verlustfrei {
                    crate::vp8l::vp8l_lesen(&roh)?
                } else {
                    crate::vp8::vp8_lesen(&roh)?
                };
                // Die Koepfe nennen die Groesse; weicht das dekodierte Bild
                // davon ab, ist das ein benannter Fehler und kein stilles
                // Zuschneiden — sonst waere die Position im Bildschirm falsch.
                if bild.breite != w || bild.hoehe != h {
                    return Err(BildFehler::WebpDatenFalsch {
                        grund: "Bildgroesse im ANMF passt nicht zum dekodierten Bild",
                    });
                }
                frames.push(WebpFrame {
                    nummer: frames.len(),
                    bild: std::mem::replace(&mut bild, Bild::new(0, 0)),
                    verzoegerung_ms: dauer,
                    versatz_x: x,
                    versatz_y: y,
                    entsorgung,
                    mischen,
                });
            }
            _ => {}
        }
    }
    let (breite, hoehe) = match bildschirm {
        Some(d) => d,
        None => {
            return Err(BildFehler::WebpDatenFalsch {
                grund: "Animation ohne VP8X-Bildschirmgroesse",
            });
        }
    };
    // M3: der Bildschirm kommt aus der Datei, also muss die Flaeche
    // begrenzt werden, sonst wuerde ein 16777215x16777215-Bildschirm den
    // Prozess beenden.
    let flaeche = (breite as u64) * (hoehe as u64);
    if flaeche > crate::gif::MAX_PIXEL_ALLE_BILDER {
        return Err(BildFehler::WebpDatenFalsch {
            grund: "Bildschirm zu gross",
        });
    }
    if frames.is_empty() {
        return Err(BildFehler::WebpDatenFalsch {
            grund: "Animation ohne ANMF-Bild",
        });
    }
    Ok(Webp {
        breite,
        hoehe,
        frames,
        wiederholungen,
        hintergrund,
    })
}
