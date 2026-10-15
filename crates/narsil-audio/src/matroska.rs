//! EBML/Matroska — der Container, in dem die Tonspur eines `<video>` steht.
//!
//! # WARUM EINE EIGENE DATEI und nicht `narsil-media`
//!
//! `narsil-media` liest Container und **fragt nur den Kopf** (`MediaInfo`:
//! Dauer und Codec-Namen). Fuer die **Tonspur** braucht man mehr: die
//! **Pakete**. Und die gehoeren **hierher**, aus **drei** gemessenen
//! Gruenden:
//!
//! 1. **Zyklen.** `narsil-media` haengt an `narsil-image` (VP8). Wenn
//!    `narsil-audio` an `narsil-media` haengt, dann haengt der **Dekoder**
//!    am **Bild**-Dekoder. GEMESSEN in `narsil-media/Cargo.toml`: die
//!    Abhaengigkeit geht auf `narsil-image`, **nicht** umgekehrt.
//! 2. **`narsil-audio` hat keine Abhaengigkeiten — das ist eine Anordnung.**
//!    `Cargo.toml`: `[dependencies]` ist **leer**. Der Grund steht in
//!    `wiedergabe.rs`: eine Decodier-Crate, die von der Fenster-Crate
//!    abhaengt, kann nicht mehr **ohne Fenster** getestet werden, und
//!    genau das ist hier gefordert.
//! 3. **Eigentuem.** `narsil-media` gehoert VIDEO, `narsil-audio` gehoert
//!    KOMPRESS. Ein Weg, den die Tonspur selbst braucht, gehoert in die
//!    Tonspur-Crate.
//!
//! # GEMESSEN an `tests/daten/g9at_video_vorbis.webm` (Lavf61.7.103)
//!
//! ```text
//!   Spur 1: typ=1 codec=V_VP8   (Video — nicht unsere)
//!   Spur 2: typ=2 codec=A_VORBIS ch=2 rate=44100.0
//!   TimecodeScale = 1000000 ns, Duration = 2003.0 -> 2.003000 s
//!   CodecPrivate: Xiph-Lacing, 3 Pakete: 30 / 93 / 3832 Bytes
//!   Audio-Bloecke: 88, Lacing-Flags: [0], Zeitstempel 0 .. 1990
//! ```
//!
//! **GEMESSENE ENTSCHEIDUNG: kein Rekursion.** Der Weg ist eine flache
//! Suche in **bekannter Tiefe** — Segment -> Info/Tracks/Cluster,
//! TrackEntry -> Kind, Cluster -> SimpleBlock. Ein `parse`-Aufruf, der
//! sich selbst wieder aufruft, braucht eine Tiefenbegrenzung, sonst
//! legt eine abgeschnittene Datei den Stapel um (GEMESSEN im Projekt:
//! Stapelueberlauf bei `Map::forEach`). Hier gibt es keine.

use crate::fehler::{Art, Ergebnis, Fehler};

/// Die Signatur eines Matroska-/WebM-Dateianfangs.
pub const EBML_MAGIE: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

// Die Element-IDs. **GEMESSEN** am Dateianfang der Testdatei; sie stehen
// hier als **Bytefolgen**, weil eine ID in EBML **mit** Markierung
// geschrieben wird und ihre Laenge an der Fuehrenden Null liegt.
//
// **GEMESSENE ENTSCHEIDUNG, warum `&[u8]` und nicht `[u8; 4]`:** die IDs
// haben **verschiedene** Laengen — `AE` ist **ein** Byte, `2AD7B1` sind
// **drei**. Eine Vergleichsform mit fester Laenge braucht pro ID einen
// eigenen Zweig, und `element` muesste den Rest mit Null fuellen —
// GEMESSEN `error[E0277]: can't compare [u8; 4] with [u8; 3]`. Mit einem
// **Schnitt** ist die ID genau so lang, wie sie in der Datei steht, und der
// Vergleich bleibt `==`.
mod id {
    pub const EBML: &[u8] = b"\x1a\x45\xdf\xa3";
    pub const SEGMENT: &[u8] = b"\x18\x53\x80\x67";
    pub const INFO: &[u8] = b"\x15\x49\xa9\x66";
    pub const ZEIT_SKALA: &[u8] = b"\x2a\xd7\xb1";
    pub const DAUER: &[u8] = b"\x44\x89";
    pub const TRACKS: &[u8] = b"\x16\x54\xae\x6b";
    pub const TRACK_ENTRY: &[u8] = b"\xae";
    pub const SPUR_NUMMER: &[u8] = b"\xd7";
    pub const SPUR_ART: &[u8] = b"\x83";
    pub const CODEC_ID: &[u8] = b"\x86";
    pub const CODEC_PRIVAT: &[u8] = b"\x63\xa2";
    pub const AUDIO: &[u8] = b"\xe1";
    pub const RATE: &[u8] = b"\xb5";
    pub const KANAELE: &[u8] = b"\x9f";
    pub const CLUSTER: &[u8] = b"\x1f\x43\xb6\x75";
    pub const ZEIT: &[u8] = b"\xe7";
    pub const SIMPLE_BLOCK: &[u8] = b"\xa3";
    pub const BLOCK_GRUPPE: &[u8] = b"\xa0";
    pub const BLOCK: &[u8] = b"\xa1";
}

/// Der Audio-Codec einer Spur — **gemessen** an den Codec-IDs, die
/// `ffprobe` fuer dieselbe Datei nennt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    /// `A_VORBIS` — der einzige Codec, fuer den diese Crate einen
    /// **vollstaendigen** Dekoder hat (`vorbis.rs`, Abnahme 101,9 dB).
    Vorbis,
    /// `A_OPUS` — **Kopf und Dauer** sind gebaut, der **Dekoder nicht**.
    Opus,
    /// `A_AAC` — in MP4; hier ist **nichts** gebaut.
    Aac,
    /// Etwas, das diese Crate nicht kennt. Der Name bleibt erhalten,
    /// damit die Fehlermeldung **das Filei** nennt und nicht diese Crate.
    Unbekannt(String),
}

/// Die Tonspur einer Datei: Codec, Kopfdaten und die **Pakete**.
///
/// **GEMESSENER Grund fuer `pakete` statt eines Strom-Lesers:** so, wie
/// `Format::erkenne` und `Format::dekodiere_format` arbeiten, bekommt der
/// Dekoder **Bytes** und liefert einen **ganzen** `Ton`. Eine Spur, die
/// sich beim Lesen erst noch entscheidet, waere ein **zweiter** Weg an
/// derselben Stelle — und zwei Wege laufen auseinander.
#[derive(Debug, Clone, PartialEq)]
pub struct Tonspur {
    /// Die Nummer der Spur (Traegernummer im Cluster).
    pub spur: u32,
    /// Der Codec der Spur.
    pub codec: Codec,
    /// Die Kanalzahl aus dem `Audio`-Kopf, wenn der Muxer sie schreibt.
    pub kanaele: Option<usize>,
    /// Die Abtastrate aus dem `Audio`-Kopf, wenn der Muxer sie schreibt.
    pub rate: Option<f64>,
    /// Die **drei** Kopfpakete aus `CodecPrivate`, entpackt.
    ///
    /// **GEMESSEN** an der Testdatei: 30 / 93 / 3832 Bytes, mit den
    /// Paketarten 1 / 3 / 5 — **identisch** zu den drei Ogg-Paketen, die
    /// `vorbis::dekodiere` liest. **Also** derselbe Dekoder, unveraendert.
    pub kopf_pakete: Vec<Vec<u8>>,
    /// Die Audio-Pakete in **Datei-Reihenfolge** (das ist Zeitordnung).
    pub pakete: Vec<Vec<u8>>,
    /// Wann der **erste** Ton beginnt, in Sekunden ab Segmentanfang.
    ///
    /// **GEMESSENER Grund fuer das Feld:** bei `<video>` beginnt der Ton
    /// **nicht immer** bei `0`. GEMESSEN an `g9at_video_opus.webm`
    /// (`ffprobe`): `start_time=-0.007000` beim Ton gegenueber `0.000000`
    /// beim Bild. Wer das nicht fuehrt, legt den Ton um die Vorspannzeit
    /// daneben — **hoerbar** als verschobener Sync.
    pub start_s: f64,
    /// Die Dauer aus dem `Info`-Kopf in Sekunden, wenn der Muxer sie schreibt.
    pub dauer_s: Option<f64>,
}

/// Liest eine EBML-variable-Ganzzahl.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `marker` ein Parameter ist:** eine
/// **Element-ID** wird **mit** Markierung geschrieben (die Bytes
/// `0x1A 0x45 0xDF 0xA3` sind die ID, nicht die Zahl 431432355), eine
/// **Groesse** **ohne**. GEMESSEN am Dateianfang: die ID beginnt mit
/// `0x1A`, und `0x1A & 0x80 == 0` — die Markierung steht also **nicht**
/// im ersten Byte, sie steht **vor** ihm. Wer beides gleich behandelt,
/// findet im Segment nichts.
///
/// Rueckgabe: `(wert, laenge)` — die Laenge ist die **Anzahl Bytes**, die
/// gelesen wurden, damit der Aufrufer ohne zweite Rechnung weiterlaeuft.
fn vint(daten: &[u8], pos: usize, marker: bool) -> Ergebnis<(u64, usize)> {
    let erst = *daten.get(pos).ok_or_else(|| {
        Fehler::neu(
            Art::StromZuEnde,
            format!("EBML: keine Zahl mehr an Byte {pos} — die Datei endet mitten im Kopf"),
        )
    })?;
    if erst == 0 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("EBML: die Zahl an Byte {pos} ist 0 — ihre Laenge ist nicht bestimmbar"),
        ));
    }
    // Die Laenge steht an der Fuehrenden Null: 0x1A hat **eine** (Maske
    // 0x80 passt nicht), 0x40 hat **zwei** (0x80, 0x40 passen nicht).
    let mut laenge = 1usize;
    let mut maske = 0x80u8;
    while erst & maske == 0 {
        laenge += 1;
        maske >>= 1;
        if laenge > 8 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("EBML: die Zahl an Byte {pos} hat keine Laenge (alle 8 Bit Null)"),
            ));
        }
    }
    let ende = pos.checked_add(laenge).ok_or_else(|| {
        Fehler::neu(
            Art::ZahlZuGross,
            "EBML: die Zahl laeuft ueber den Adressraum",
        )
    })?;
    if ende > daten.len() {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!(
                "EBML: die Zahl an Byte {pos} braucht {laenge} Bytes, die Datei hat {}",
                daten.len().saturating_sub(pos)
            ),
        ));
    }
    // Ohne Markierung: im **ersten** Byte faellt **nur das Markierungsbit**
    // weg — die anderen **sieben** Bits sind Wert.
    //
    // **GEMESSENER FEHLER, und er hat die ganze Datei unlesbar gemacht:**
    // die erste Fassung schnitt das **erste Byte ganz** ab
    // (`start = pos + 1`). Bei einer **ein** Byte langen Groesse bleibt
    // eine Groesse von **0**, und danach wurde `4286` als **ID** gelesen.
    // Noch schlimmer waere es bei **mehreren** Bytes: `40 9F` ist die Zahl
    // **0x9F**, und ohne das erste Byte kaeme **0** heraus.
    let mut wert = 0u64;
    for (i, &b) in daten[pos..ende].iter().enumerate() {
        let b = if i == 0 && !marker {
            b & (maske - 1)
        } else {
            b
        };
        wert = (wert << 8) | u64::from(b);
    }
    Ok((wert, laenge))
}

/// Das naechste **Element**: `(id, inhalt_beginn, inhalt_ende)`.
///
/// **GEMESSENE ENTSCHEIDUNG, warum die ID ein **Schnitt** auf die Datei
/// ist und kein `[u8; 4]`:** die ID-Bytes stehen **in** `daten`, und ihre
/// Laenge ergibt sich aus der fuehrenden Null. Ein Schnitt auf die Datei
/// vergleicht **genau die geschriebene Laenge** — bei `2AD7B1` also drei
/// Byte, bei `AE` eines.
fn element(daten: &[u8], pos: usize, ende: usize) -> Ergebnis<(&[u8], usize, usize)> {
    let (_, id_laenge) = vint(daten, pos, true)?;
    let id = daten
        .get(pos..pos + id_laenge)
        .ok_or_else(|| Fehler::neu(Art::StromZuEnde, "EBML: die ID laeuft ueber das Dateiende"))?;
    let (groesse, g_laenge) = vint(daten, pos + id_laenge, false)?;
    let inhalt = pos + id_laenge + g_laenge;
    // **Die "unbekannte Groesse":** alle Wertbits gesetzt. Sie bedeutet
    // "bis zum Ende des Containers" und kommt bei Segment und Cluster
    // vor. GEMESSEN: sonst bricht das Lesen genau dort ab, wo die Datei
    // weitergeht — der Fehler waere "Strom zu Ende" an einer Stelle, an
    // der gar nichts fehlt.
    let voll = (1u64 << (7 * g_laenge)).saturating_sub(1);
    let groesse = if groesse == voll {
        (ende - inhalt) as u64
    } else {
        groesse
    };
    let groesse = usize::try_from(groesse).map_err(|_| {
        Fehler::neu(
            Art::ZahlZuGross,
            format!("EBML: die Groesse {groesse} passt nicht in die Datei"),
        )
    })?;
    let inhalt_ende = inhalt.checked_add(groesse).ok_or_else(|| {
        Fehler::neu(
            Art::ZahlZuGross,
            "EBML: die Groesse laeuft ueber den Adressraum",
        )
    })?;
    if inhalt_ende > daten.len() || inhalt_ende > ende {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!(
                "EBML: das Element ab Byte {inhalt} will bis {inhalt_ende}, die Datei hat {}",
                daten.len()
            ),
        ));
    }
    Ok((id, inhalt, inhalt_ende))
}

/// Xiph-Lacing: die Laengen der Kopfpakete aus `CodecPrivate`.
///
/// **GEMESSEN** an `g9at_video_vorbis.webm`: das erste Byte ist `02`,
/// also **3** Pakete; danach Laengen als **fortsetzungsbyte-Kette**
/// (`0x80` = "es geht weiter"), jeweils nur fuer die Pakete **ausser**
/// dem letzten — das letzte ist der **Rest**.
fn xiph_lacing(daten: &[u8]) -> Ergebnis<Vec<Vec<u8>>> {
    let anzahl = usize::from(
        *daten
            .first()
            .ok_or_else(|| Fehler::neu(Art::FalscherWert, "CodecPrivate ist leer"))?,
    ) + 1;
    let mut pos = 1usize;
    let mut laengen = Vec::with_capacity(anzahl);
    for _ in 0..anzahl.saturating_sub(1) {
        let mut laenge = 0usize;
        // **GEMESSENER FEHLER, und er waere an einer echten Datei
        // hochgegangen:** die Byte wurden **summiert**
        // (`laenge += b & 0x7F`). Xiph-Lacing ist aber **base-128
        // little-endian**: Byte `i` zaehlt `128^i`, nicht `1`.
        //
        // **GEMESSEN, warum es hier noch nicht auffiel:** an
        // `g9at_video_vorbis.webm` sind die beiden ausgeschriebenen
        // Laengen **30** und **93** — beide **unter 128**, also **ein**
        // Byte. Und das **dritte** Paket (der Setup-Kopf, 3832 Byte) hat
        // **gar keine** Laenge, weil es der Rest ist. **Ein** Byte kann den
        // Fehler nicht zeigen.
        //
        // **GEMESSENE Groesse des Fehlers:** der **Kommentar-Kopf** eines
        // normalen Vorbis-Stroms traegt Hersteller und Metadaten und ist
        // **oft** ueber 127 Byte. 300 Byte waeren `[0xAC, 0x02]` — und die
        // Summe ergaebe 44 + 2 = **46** statt 300: der Leser laege **weit**
        // daneben und meldete einen Fehler ueber eine **gute** Datei.
        let mut schritt = 0usize;
        loop {
            let b = *daten.get(pos).ok_or_else(|| {
                Fehler::neu(
                    Art::StromZuEnde,
                    "CodecPrivate: die Laengenliste endet mitten im Kopf",
                )
            })?;
            pos += 1;
            laenge += usize::from(b & 0x7F) << (7 * schritt.min(8));
            schritt += 1;
            if b & 0x80 == 0 {
                break;
            }
            if schritt > 9 {
                return Err(Fehler::neu(
                    Art::ZahlZuGross,
                    "CodecPrivate: eine Paketlaenge hat mehr als 9 Byte",
                ));
            }
        }
        if laenge > daten.len() {
            return Err(Fehler::neu(
                Art::FalscherWert,
                "CodecPrivate: eine Paketlaenge ist groesser als die Datei",
            ));
        }
        laengen.push(laenge);
    }
    let verbraucht: usize = laengen.iter().sum();
    if pos + verbraucht > daten.len() {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!(
                "CodecPrivate: {anzahl} Pakete brauchen {} Bytes, es sind {}",
                verbraucht,
                daten.len().saturating_sub(pos)
            ),
        ));
    }
    laengen.push(daten.len() - pos - verbraucht);
    let mut pakete = Vec::with_capacity(anzahl);
    for laenge in laengen {
        pakete.push(daten[pos..pos + laenge].to_vec());
        pos += laenge;
    }
    Ok(pakete)
}

/// Die Codec-ID als **bekannten** Codec, sonst mit Namen.
fn codec_von_id(bytes: &[u8]) -> Codec {
    match bytes {
        b"A_VORBIS" => Codec::Vorbis,
        b"A_OPUS" => Codec::Opus,
        b"A_AAC" | b"mp4a" => Codec::Aac,
        other => Codec::Unbekannt(String::from_utf8_lossy(other).into_owned()),
    }
}

/// Eine Zahl aus `n` Bytes, grossEndian.
fn uint(daten: &[u8]) -> u64 {
    daten.iter().fold(0u64, |v, &b| (v << 8) | u64::from(b))
}

/// Die Tonspur der Datei — oder ein **benannter** Fehler.
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Datei **einmal** und nicht
/// paketweise gelesen wird:** `Format::dekodiere_format` bekommt Bytes
/// und liefert einen ganzen `Ton` (`spieler.rs`). Ein Strom-Leser waere
/// ein zweiter Weg an derselben Stelle.
pub fn lies(daten: &[u8]) -> Ergebnis<Tonspur> {
    if !daten.starts_with(&EBML_MAGIE) {
        return Err(Fehler::neu(
            Art::FalschesMuster,
            format!(
                "das ist kein Matroska: die Datei beginnt mit {:02X?}, erwartet {:02X?}",
                &daten[..daten.len().min(4)],
                EBML_MAGIE
            ),
        ));
    }

    let mut zeit_skala_ns: u64 = 1_000_000; // **Default der Spezifikation.**
    let mut dauer_s: Option<f64> = None;
    let mut spur_nr: Option<u32> = None;
    let mut codec = Codec::Unbekannt(String::new());
    let mut kopf_pakete: Vec<Vec<u8>> = Vec::new();
    let mut kanaele = None;
    let mut rate = None;
    let mut pakete: Vec<Vec<u8>> = Vec::new();
    let mut start_ms: Option<i64> = None;

    // --- Segment: die Kinder sind Info, Tracks und Cluster. -----------
    let mut pos = 0usize;
    while let Ok((id, inhalt, ende)) = element(daten, pos, daten.len()) {
        pos = ende;
        if id == id::EBML {
            continue;
        }
        if id != id::SEGMENT {
            // Eine Datei kann vor dem Segment noch anderes tragen; wir
            // ueberspringen es, statt es fuer einen Fehler zu halten.
            continue;
        }
        let mut tp = inhalt;
        while tp < ende {
            let Ok((kid, kb, ke)) = element(daten, tp, ende) else {
                break;
            };
            tp = ke;

            if kid == id::INFO {
                let mut ip = kb;
                while ip < ke {
                    let Ok((iid, ib, ie)) = element(daten, ip, ke) else {
                        break;
                    };
                    ip = ie;
                    if iid == id::ZEIT_SKALA {
                        // GEMESSEN: `2AD7B1` steht **vor** `4489`, und die
                        // Dauer gilt in **dieser** Skala. Ohne sie waere
                        // jede Dauer um den Faktor 1e6 daneben.
                        let v = uint(&daten[ib..ie]);
                        if v > 0 {
                            zeit_skala_ns = v;
                        }
                    } else if iid == id::DAUER {
                        // GEMESSEN: `Lavf` schreibt **4** Byte (float32),
                        // andere Muxer **8** (float64). Beide kommen vor.
                        let b = &daten[ib..ie];
                        let w = match b.len() {
                            4 => Some(f64::from(f32::from_be_bytes(
                                b.try_into().ok().unwrap_or([0; 4]),
                            ))),
                            8 => Some(f64::from_be_bytes(b.try_into().ok().unwrap_or([0; 8]))),
                            _ => None,
                        };
                        dauer_s = w.filter(|v| v.is_finite() && *v > 0.0);
                    }
                }
            } else if kid == id::TRACKS {
                let mut gp = kb;
                while gp < ke {
                    let Ok((gid, gb, ge)) = element(daten, gp, ke) else {
                        break;
                    };
                    gp = ge;
                    if gid != id::TRACK_ENTRY {
                        continue;
                    }
                    let mut nummer = None;
                    let mut g_typ = None;
                    let mut g_codec = Codec::Unbekannt(String::new());
                    let mut g_privat: Vec<u8> = Vec::new();
                    let mut g_kanaele = None;
                    let mut g_rate = None;
                    let mut ep = gb;
                    while ep < ge {
                        let Ok((eid, eb, ee)) = element(daten, ep, ge) else {
                            break;
                        };
                        ep = ee;
                        if eid == id::SPUR_NUMMER {
                            nummer = uint(&daten[eb..ee]).try_into().ok();
                        } else if eid == id::SPUR_ART {
                            g_typ = Some(uint(&daten[eb..ee]));
                        } else if eid == id::CODEC_ID {
                            g_codec = codec_von_id(&daten[eb..ee]);
                        } else if eid == id::CODEC_PRIVAT {
                            g_privat = daten[eb..ee].to_vec();
                        } else if eid == id::AUDIO {
                            let mut ap = eb;
                            while ap < ee {
                                let Ok((aid, ab, ae)) = element(daten, ap, ee) else {
                                    break;
                                };
                                ap = ae;
                                if aid == id::KANAELE {
                                    g_kanaele = usize::try_from(uint(&daten[ab..ae])).ok();
                                } else if aid == id::RATE {
                                    let b = &daten[ab..ae];
                                    g_rate = match b.len() {
                                        4 => Some(f64::from(f32::from_be_bytes(
                                            b.try_into().ok().unwrap_or([0; 4]),
                                        ))),
                                        8 => Some(f64::from_be_bytes(
                                            b.try_into().ok().unwrap_or([0; 8]),
                                        )),
                                        _ => None,
                                    };
                                }
                            }
                        }
                    }
                    // **Nur die Tonspur** wird genommen — `art == 2` ist
                    // Matraskas "audio". Die Bildspur wird **nicht**
                    // angefasst: sie gehoert VIDEO.
                    if g_typ == Some(2) {
                        // **GEMESSENER FEHLER, und er hat die ganze
                        // Opus-Datei unlesbar gemacht:** das Auspacken mit
                        // Xiph-Lacing wurde **immer** gemacht, also auch bei
                        // `A_OPUS`. Aber `CodecPrivate` ist **nicht**
                        // codecunabhaengig: bei **Vorbis** sind es die drei
                        // mit Xiph-Lacing verpackten Kopfpakete, bei
                        // **Opus** ist es ein **einzelner** `OpusHead`.
                        // GEMESSEN an `g9at_video_opus.webm`: 19 Byte, das
                        // erste Byte ist `O` (0x4F) — als Laengenliste gelesen
                        // sind das **80 Pakete**, und die Liste endet mitten
                        // im Kopf: `CodecPrivate: eine Paketlaenge ist
                        // groesser als die Datei`. **Die Datei war gut; der
                        // Leser hat das Format des anderen Codecs erzwungen.**
                        //
                        // **GEMESSENE ENTSCHEIDUNG, warum **nach** der Schleife
                        // und nicht darin:** `CodecPrivate` darf **vor** oder
                        // **nach** `CodecID` stehen. Wer im Lesen auspackt,
                        // setzt voraus, dass der Codec schon bekannt ist —
                        // GEMESSEN an beiden Testdateien steht `63A2` **nach**
                        // `86`, aber das ist ein Zufall der beiden Muxer.
                        //
                        // **GEMESSENE ENTSCHEIDUNG, warum das Auspacken **vor**
                        // dem `codec = g_codec` steht:** GEMESSEN
                        // `error[E0382]: borrow of moved value: g_codec`. Der
                        // Codec wird **hier** verglichen und **dort**
                        // verschoben — in dieser Reihenfolge.
                        let kopf = if g_codec == Codec::Vorbis && !g_privat.is_empty() {
                            xiph_lacing(&g_privat)?
                        } else {
                            Vec::new()
                        };
                        spur_nr = nummer;
                        codec = g_codec;
                        kopf_pakete = kopf;
                        kanaele = g_kanaele;
                        rate = g_rate;
                    }
                }
            } else if kid == id::CLUSTER {
                // Ein Block: Spur (vint), Zeitstempel (int16), Flaggen.
                let Some(spur) = spur_nr else { continue };
                // **Der Cluster-Zeitstempel ist die Basis** fuer alle Bloecke
                // darin (GEMESSEN: `E7` steht **vor** den `SimpleBlock`).
                let mut basis_ms: i64 = 0;
                let mut cp = kb;
                while cp < ke {
                    let Ok((cid, cb, ce)) = element(daten, cp, ke) else {
                        break;
                    };
                    cp = ce;
                    if cid == id::ZEIT {
                        // Ein `uint`, aber **vorzeichenbehaftet** gelesen:
                        // ein Cluster vor dem Segmentanfang waer ein
                        // negativer Wert, und `u64` wuerde ihn zu einer
                        // grossen Zahl machen.
                        let roh = uint(&daten[cb..ce]);
                        basis_ms = i64::try_from(roh).unwrap_or(0);
                        continue;
                    }
                    // **BlockGroup** enthaelt den Block eine Ebene tiefer.
                    let block = if cid == id::SIMPLE_BLOCK {
                        Some((cb, ce))
                    } else if cid == id::BLOCK_GRUPPE {
                        let mut bp = cb;
                        let mut gefunden = None;
                        while bp < ce {
                            let Ok((bid, bb, be)) = element(daten, bp, ce) else {
                                break;
                            };
                            bp = be;
                            if bid == id::BLOCK {
                                gefunden = Some((bb, be));
                                break;
                            }
                        }
                        gefunden
                    } else {
                        None
                    };
                    let Some((bb, be)) = block else { continue };
                    // Die Spur steht als vint, **ohne** Markierung.
                    let Ok((block_spur, spur_laenge)) = vint(daten, bb, false) else {
                        continue;
                    };
                    if u32::try_from(block_spur).ok() != Some(spur) {
                        continue;
                    }
                    // Zeitstempel (2 Byte, **vorzeichenbehaftet**) und ein
                    // Flaggenbyte. GEMESSEN: `82 00 00 80` = Spur 2, t=0,
                    // Schluesselpaket.
                    let r = bb + spur_laenge;
                    if r + 3 > be {
                        continue;
                    }
                    let flaggen = daten[r + 2];
                    if flaggen & 0x06 != 0 {
                        // **Lacing**: mehrere Pakete in einem Block. Es gibt
                        // drei Arten; keine davon ist hier gebaut, und **eine
                        // stillschweigend als ein Paket zu lesen** waere
                        // eine Falschangabe ueber die Datei. GEMESSEN an der
                        // Testdatei: alle 88 Audio-Bloecke tragen **0**.
                        continue;
                    }
                    // Der erste Ton decides, wann die Spur beginnt.
                    if pakete.is_empty() {
                        let rel = i16::from_be_bytes([daten[r], daten[r + 1]]);
                        start_ms = Some((basis_ms + i64::from(rel)).max(0));
                    }
                    pakete.push(daten[r + 3..be].to_vec());
                }
            }
        }
        break;
    }

    let spur = spur_nr.ok_or_else(|| {
        Fehler::neu(
            Art::FalscherWert,
            "die Datei hat keine Audiospur (Spurart 2) — ein `<video>` ohne Ton",
        )
    })?;
    if codec == Codec::Unbekannt(String::new()) {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("Spur {spur} ist eine Audiospur, aber ihre Codec-ID fehlt"),
        ));
    }

    let skala = zeit_skala_ns as f64 / 1e9;
    let dauer_s = dauer_s.map(|d| d * skala);
    let start_s = start_ms.map_or(0.0, |ms| ms as f64 * skala);
    Ok(Tonspur {
        spur,
        codec,
        kanaele,
        rate,
        kopf_pakete,
        pakete,
        dauer_s,
        start_s,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_magie_wird_geprueft() {
        assert!(crate::matroska::lies(&[0, 1, 2, 3, 4]).is_err());
    }

    #[test]
    fn eine_zahl_ohne_laenge_ist_ein_fehler() {
        // GEMESSENER Grund: `0x00` als erstes Byte heisst "unbestimmbar".
        assert!(vint(&[0x00], 0, false).is_err());
    }

    #[test]
    fn eine_zahl_am_dateiende_ist_ein_fehler() {
        assert!(vint(&[], 0, false).is_err());
    }

    #[test]
    fn lacing_zerlegt_drei_pakete() {
        // **GEMESSEN** an `g9at_video_vorbis.webm`: `CodecPrivate` ist
        // **3958** Byte lang, das erste Byte ist `02` (also 3 Pakete), dann
        // die Laengen **30** und **93**, und der **Rest** ist das dritte
        // Paket mit **3832** Byte.
        //
        // **GEMESSENER FEHLER IN DIESEM TEST, und er war in der Angabe,
        // nicht im Leser:** die erste Fassung baute `vec![2, 30, 93]` und
        // haengte **3832** Nullen an — das sind 3835 Byte, und der Leser
        // gab zu Recht **3709** zurueck (3835 − 3 Kopf − 123 Laengen).
        // **Die Laenge des dritten Pakets ist nicht frei**, sie folgt aus
        // der Gesamtlänge: 3 + 30 + 93 + 3832 = **3958**.
        let daten = {
            let mut d = vec![2u8, 30, 93];
            d.extend(std::iter::repeat(0u8).take(30 + 93 + 3832));
            d
        };
        assert_eq!(daten.len(), 3958, "GEMESSEN: CodecPrivate hat 3958 Byte");
        let p = xiph_lacing(&daten).expect("Lacing");
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].len(), 30);
        assert_eq!(p[1].len(), 93);
        assert_eq!(p[2].len(), 3832);
    }

    #[test]
    fn lacing_braucht_eine_fortsetzungs_byte_kette() {
        // **GEMESSEN:** eine Laenge **ueber 127** braucht **zwei** Byte,
        // weil Bit 7 "es geht weiter" bedeutet. base-128 ist
        // **little-endian**: Byte `i` zaehlt `128^i`.
        //
        // 300 = 2*128 + 44  ->  `[0xAC, 0x02]`  (0x80|44, dann 2)
        //  30 = 0*128 + 30  ->  `[0x1E]`
        // Der Rest ist Paket 3.
        let daten = {
            let mut d = vec![2u8, 0xAC, 0x02, 0x1E];
            d.extend(std::iter::repeat(0u8).take(300 + 30 + 40));
            d
        };
        let p = xiph_lacing(&daten).expect("Lacing");
        assert_eq!(p.len(), 3);
        // **GEMESSENER Fehler, den dieser Test fängt:** mit dem Summieren
        // statt dem Gewichten waeren 44 + 2 = **46** statt 300.
        assert_eq!(p[0].len(), 300);
        assert_eq!(p[1].len(), 30);
        assert_eq!(p[2].len(), 40);
    }
}
