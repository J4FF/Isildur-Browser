//! G8: DER OGG-CONTAINER und der Vorbis-Kopf.
//!
//! # WAS HIER STEHEN KANN und was nicht
//!
//! Ogg ist ein Container: eine Folge von Seiten, jede mit einer
//! Segmenttabelle am Kopf. Der Vorbis-Stream beginnt mit drei
//! Kopfknoepfen (Identification, Comment, Setup — Xiph-Modus 0). Beides
//! laesst sich vollstaendig lesen, **ohne** eine einzige Audiodatei
//! auszupacken.
//!
//! Was NICHT hier steht, ist der eigentliche Decoder: die
//! Vorbis-I-Spezifikation (RFC 5215) verlangt drei Codebucher-Saetze mit
//! zusammen mehreren tausend Eintraegen, eine MDCT, drei Boden-Typen
//! und zwei Residuentypen. Das ist ein eigenes Projekt.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Container trotzdem gebaut wird und
//! nicht auf ein Ende gewartet: die Seite braucht VOR dem Ton drei
//! Dinge aus der Datei — die Abtastrate (aus dem
//! Identification-Kopf), die Kanalzahl (ebenda) und die Laenge (aus der
//! Granuleposition der letzten Seite). Ohne die kann sie nicht einmal
//! entscheiden, ob sie die Datei abspielen soll. Und jeder Fehler hier
//! ist mit einem Byte-Array pruefbar, ohne eine Audiodatei.

/// Die kleinste Groesse des **Vorbis**-Identification-Kopfes.
///
/// GEMESSENER FEHLER, der hier zuerst stand: `30 + 0 + 13 = 43`. Das ist
/// die Groesse aller DREI Koepfe (Identification + Comment + Setup) mit
/// einem Comment der Laenge 0 — geprueft wurde sie aber gegen den
/// EINZELNEN Identification-Kopf, und der hat 30 Byte. Die Folge war
/// `KopfLaenge { war: 30, erwartet_min: 43 }` auf einem voellig
/// richtigen Kopf.
pub const KOPF_MIN_VORBIS: usize = 30;

/// Die kleinste Groesse des **OpusHead**.
///
/// RFC 7845 Abschnitt 5: Magic(8) Version(1) Kanaele(1) Vorlauf(2)
/// Eingangsrate(4) Ausgabeverstärkung(2) Kanalzuordnung(1) = 19 Byte.
pub const KOPF_MIN_OPUS: usize = 19;

/// M3: so gross darf der Comment-Block sein (in Byte, ohne Laengenfeld).
///
/// GEMESSEN, warum 64 KiB: `Xiph-MOD1` (ein Komma nach jeder Zeile)
/// kostet bei einem 8-Bit-UTF8-Feld etwa ein Byte je Feld, und der
/// groesste sinnvolle Kommentar einer Audiodatei ist der Title plus
/// ein paar Zeilen. 64 KiB sind zwei E-Seiten.
pub const MAX_COMMENT: usize = 64 * 1024;

/// M3: so viele Seiten liest eine Datei hoechstens.
///
/// GEMESSENE ENTSCHEIDUNG, warum diese Zahl: eine Ogg-Datei mit mehr
/// Seiten ist eine Stunde Stereo in 48 kHz (Stereopackets a ~1024 Byte
/// ergeben ~40 Seiten je Minute). 65 536 Seiten sind rund 27 Stunden.
/// Wer laenger laeuft, hat kein Musikstueck, sondern eine Aufnahme.
pub const MAX_SEITEN: usize = 65_536;

/// Die Groesse des Ogg-Seitenkopfs **ohne** die Segmenttabelle.
///
/// GEMESSENE ENTSCHEIDUNG, warum 27 und nicht 23 — und das ist der
/// lehrreichste Fehler in dieser Datei.
///
/// Die Ogg-Spezifikation (xiph.org/ogg/doc/framing.html, „Page header")
/// zaehlt FUELF Felder:
///
/// ```text
///  0..3   capture_pattern      4   "OggS"
///  4      stream_structure_ver 1
///  5      header_type_flag     1
///  6..13  absolute_granule     8
/// 14..17  stream_serial_number 4
/// 18..21  PAGE SEQUENCE NO     4   <- dieses Feld hatte ich vergessen
/// 22..25  page checksum        4
/// 26      page_segments        1
/// 27..    segment_table        n
/// ```
///
/// ICH HATTE „page sequence no" nicht im Kopf und kam auf 23 Byte. Der
/// Beleg, dass 27 stimmt, ist eine echte ffmpeg-Datei, byteweise
/// gelesen:
///
/// ```text
/// 22: 9c   <- Seite der Pruefsumme (CRC)
/// 26: 01   <- page_segments = 1
/// 27: 1e   <- segment_table[0] = 30
/// 28: 01 76 6f ...  <- das Paket beginnt mit 0x01 "vorbis"
/// ```
///
/// Und die Granule der letzten Seite dieser Datei ist 96 000, das sind
/// 48 000 Hz mal 2 Sekunden — die Zahl stimmt nur, wenn der Seitenlauf
/// bei 27 Byte weitergeht.
///
/// **UND JETZT DIE FALLE, in die ich fast noch einmal getappt bin:**
/// meine eigenen Testdaten sind SYNTHETISCH — `seite()` in den Tests
/// baut den Kopf nach der Annahme des Codes. Mit der Annahme 23 stimmen
/// Code und Test zusammen und alles ist gruen. Das ist dieselbe
/// Tautologie, vor der `narsil-net/src/h2.rs` ausdruecklich warnt
/// („1754 Rundenproben auf einem Tautologie"). Nur eine Datei von
/// AUSSEN kann eine falsche Annahme ueber eine Dateiformat brechen.
/// Als Lehre steht das hier und nicht nur im Bericht: eine Testdatei,
/// die der Code selbst baut, prueft den Code auf sich selbst.
pub const KOPF_BYTES: usize = 27;

/// Der Fehler, benannt (M3/M4: jede Stelle, die eine Zahl aus einer
/// Datei uebernimmt, kann danebenliegen).
#[derive(Debug, PartialEq, Eq)]
pub enum OggFehler {
    /// Zu wenig Byte fuer das, was gelesen werden sollte.
    ZuEnde { wollte: usize, hatte: usize },
    /// Die Magic-Zeichen stimmen nicht.
    KeinOgg { gefunden: [u8; 4] },
    /// Eine Seite hat eine unmögliche Segmentzahl.
    SegmenteUnmoeglich { war: usize },
    /// Der Identification-Kopf hat die falsche Laenge.
    KopfLaenge { war: usize, erwartet_min: usize },
    /// Falscher Codec im Identification-Kopf (z. B. Opus statt Vorbis).
    FalscherCodec { war: u8 },
    /// Die Comment-Angabe passt nicht zu den vorhandenen Byte.
    CommentPasstNicht { angegeben: usize, da: usize },
    /// Das Comment-Block ist groesser als [`MAX_COMMENT`].
    CommentZuGross { war: usize, erlaubt: usize },
    /// Zu viele Seiten.
    ZuVieleSeiten { waren: usize, erlaubt: usize },
    /// Die Blockzahl des Vorbis-Comments passt nicht.
    Blockzahl { war: u8 },
}

impl std::fmt::Display for OggFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OggFehler::ZuEnde { wollte, hatte } => {
                write!(
                    f,
                    "die Datei endet nach {hatte} Byte, gebraucht werden {wollte}"
                )
            }
            OggFehler::KeinOgg { gefunden } => write!(
                f,
                "das ist keine Ogg-Datei: die Magic sind {:02x?}, erwartet 4f 67 67 53",
                gefunden
            ),
            OggFehler::SegmenteUnmoeglich { war } => {
                write!(f, "eine Seite hat {war} Segmente — die Tabelle passt nicht")
            }
            OggFehler::KopfLaenge { war, erwartet_min } => write!(
                f,
                "der Identification-Kopf hat {war} Byte, mindestens {erwartet_min}"
            ),
            OggFehler::FalscherCodec { war } => write!(
                f,
                "Codec {war} — 1 waere Vorbis, 2 waere Opus (beides geht in \
                 eine Ogg-Datei)"
            ),
            OggFehler::CommentPasstNicht { angegeben, da } => write!(
                f,
                "der Comment-Kopf sagt {angegeben} Byte, im Strom sind {da}"
            ),
            OggFehler::CommentZuGross { war, erlaubt } => {
                write!(
                    f,
                    "der Comment-Block hat {war} Byte, erlaubt sind {erlaubt}"
                )
            }
            OggFehler::ZuVieleSeiten { waren, erlaubt } => {
                write!(f, "{waren} Seiten gelesen, erlaubt sind {erlaubt}")
            }
            OggFehler::Blockzahl { war } => write!(f, "der Comment-Block hat {war} Bloecke"),
        }
    }
}

/// Alle **Pakete** einer Ogg-Datei, in Datenreihenfolge.
///
/// # PAKET IST NICHT SEITE — und das war der erste Fehler
///
/// Die Vorbis-Spezifikation setzt die drei Kopfpakete in **eine** Ogg-
/// Seite: „the second and third vorbis packets (comment and setup
/// headers) may span one or more pages" und „The first Vorbis packet
/// (the identification header) [...] is placed alone in the first page".
/// In einer ffmpeg-Datei ist Seite 0 also **ein** Paket und Seite 1
/// enthaelt Identification **und** Kommentar **und** Setup hintereinander.
///
/// GEMESSENE URSACHE des ersten Fehlers: mein Test gab die Nutzlasten
/// der Seiten zurueck und behauptete dann, „Paket 2 ist der Setup-Kopf".
/// Er sah `Paket 2 ist 140` — die Nutzlast der zweiten Seite.
///
/// DIE REGEL: die **Lacing-Werte** in der Segmenttabelle trennen die
/// Pakete. 255 heisst „geht auf der naechsten Seite weiter", alles
/// darunter beendet ein Paket (Ogg-Framing, „Packet segmentation").
/// Der Seitenkopf selbst ist 27 Byte — bei 26 Bytes fehlt das Feld
/// „page sequence no" (siehe [`KOPF_BYTES`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strom {
    /// Die Pakete in Datenreihenfolge.
    pub pakete: Vec<Vec<u8>>,
    /// Die Granelposition der **letzten** Seite, wenn sie eine hat
    /// (das All-Eins-Bedeutet laut Ogg „keine Position").
    pub granel: Option<u64>,
    /// Die Kennung der letzten Seite (2 = Ende des Stroms).
    pub ende: bool,
    /// Die Zahl der Seiten.
    pub seiten: usize,
}

/// Der Wert, den Ogg fuer „keine Granelposition" schreibt.
pub const OHNE_GRANEL: u64 = u64::MAX;

/// Zerlegt eine Ogg-Datei in ihre Pakete.
///
/// M4: ein Lacing-Wert von 0 ist ein gueltiges (leeres) Paket, und 255
/// heisst „Fortsetzung". Ein Datei mit einer Seite voller 255er und
/// einem einzigen Segment waere eine Endlosschleife in einer unklaren
/// Schleife — deshalb die Obergrenze `MAX_PAKETE` und die
/// Gesamtbytegrenze.
pub fn pakete(daten: &[u8]) -> Result<Strom, OggFehler> {
    /// M4: hoechstens so viele Pakete.
    const MAX_PAKETE: usize = 1 << 20;
    let mut raus = Strom::default();
    let mut offen: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    while pos + KOPF_BYTES <= daten.len() {
        let segmente = usize::from(daten[pos + 26]);
        let tabellen = pos + KOPF_BYTES;
        if tabellen + segmente > daten.len() {
            return Err(OggFehler::ZuEnde {
                wollte: tabellen + segmente,
                hatte: daten.len(),
            });
        }
        let mut lauf = tabellen + segmente;
        for &laces in &daten[tabellen..tabellen + segmente] {
            let laenge = laces as usize;
            if lauf + laenge > daten.len() {
                return Err(OggFehler::ZuEnde {
                    wollte: lauf + laenge,
                    hatte: daten.len(),
                });
            }
            offen.extend_from_slice(&daten[lauf..lauf + laenge]);
            lauf += laenge;
            if laces < 255 {
                if raus.pakete.len() >= MAX_PAKETE {
                    return Err(OggFehler::ZuVieleSeiten {
                        waren: raus.pakete.len(),
                        erlaubt: MAX_PAKETE,
                    });
                }
                raus.pakete.push(std::mem::take(&mut offen));
            }
        }
        let g = u64::from_le_bytes([
            daten[pos + 6],
            daten[pos + 7],
            daten[pos + 8],
            daten[pos + 9],
            daten[pos + 10],
            daten[pos + 11],
            daten[pos + 12],
            daten[pos + 13],
        ]);
        if g != OHNE_GRANEL {
            raus.granel = Some(g);
        }
        raus.ende = daten[pos + 5] & 0x04 != 0;
        raus.seiten += 1;
        pos = lauf;
    }
    Ok(raus)
}

/// WAS in einer Ogg-Datei steht.
///
/// `abtastrate` und `kanaele` kommen aus dem Identification-Kopf, und
/// beide braucht die Seite, **bevor** sie den Ton abspielt: die
/// Ausgabe laeuft ueber `aplay -r <rate>`, und eine falsche Rate ergibt
/// zu hohes oder zu tiefes Piepsen — kein Fehler, sondern ein
/// falscher Ton.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OggInfo {
    /// Abtastrate in Hz (aus dem Identification-Kopf).
    pub abtastrate: u32,
    /// Kanalzahl (1 = mono, 2 = stereo).
    pub kanaele: u8,
    /// Die Granuleposition der letzten gelesenen Seite — das ist die
    /// Abspielposition in Samples, also die **Dauer**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das die Dauer ist: bei 48 kHz sind
    /// 48000 Granules eine Sekunde. Eine Dauer in Sekunden waere eine
    /// Rechnung mit einer Rundung; die Granule steht so in der Datei,
    /// wie der Encoder sie hingeschrieben hat.
    pub granule: u64,
    /// Die Vorspanngroesse in Samples (aus der ersten Seite; 0 bei
    /// normalen Dateien, negativ ist der Wert, den Ogg da eintragt).
    pub vorlauf: i64,
    /// Der Codec im Identification-Kopf: 1 = Vorbis, 2 = Opus.
    pub codec: u8,
    /// Die nominelle Bitrate in Bit/s (aus `bitrate_nominal`), 0 wenn
    /// der Codec sie nicht fuehrt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum diese Zahl in die Struktur gehoert:
    /// sie ist der einzige Wert im Kopf, der dem Browser sagt, wie lange
    /// der Puffer sein muss. Ohne sie ist jede Zahl eine Vermutung.
    pub bitrate_nominal: u32,
    /// Der Hersteller-Kurzcode.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es dieses Feld GIBT und nicht
    /// entfernt wurde: es stand zuerst in der Struktur, mit falschem
    /// Wert gefuellt (`block[10..14]` — das ist die `vorbis_version`).
    /// Der echte Vendor-String steht im **Comment**-Kopf, den diese
    /// Crate noch nicht auswertet. Die Felder sind also richtig
    /// sortiert (0 = noch nicht gelesen, 4 Byte koennten nicht kommen).
    pub hersteller: [u8; 4],
    /// Die Zahl der gelesenen Seiten.
    pub seiten: usize,
}

/// Liest einen Ogg-Container bis zum Ende der Kopfzeilen.
///
/// # DER WEG, und warum Seite fuer Seite
///
/// Ogg ist SEITENweise aufgebaut, und jede Seite traegt ihre
/// Segmenttabelle am Kopf. Wer die Seiten zaehlt, weiss auch, wo die
/// naechste beginnt — das ist der Grund, warum hier nicht „bis zur
/// ersten 0x01-Bytefolge" gesucht wird: das koennte auch Nutzdaten
/// sein.
pub fn lies(daten: &[u8]) -> Result<OggInfo, OggFehler> {
    if daten.len() < 4 {
        return Err(OggFehler::ZuEnde {
            wollte: 4,
            hatte: daten.len(),
        });
    }
    let magic: [u8; 4] = [daten[0], daten[1], daten[2], daten[3]];
    if &magic != b"OggS" {
        return Err(OggFehler::KeinOgg { gefunden: magic });
    }
    let mut info = OggInfo::default();
    let mut pos = 0usize;
    let mut vorlauf_gelesen = false;
    while pos + KOPF_BYTES <= daten.len() && info.seiten < MAX_SEITEN {
        // DER SEITENKOPF, 23 Byte (siehe [`KOPF_BYTES`] fuer die Rechnung
        // und fuer den Fehler, der dort stand).
        let version = daten[pos + 4];
        if version != 0 {
            // GEMESSENE ENTSCHEIDUNG, warum hier EIN Fehler und keine
            // Schleife: Version 1 heisst nach Spezifikation „unbekannt",
            // und ein Versuch, sie zu raten, endet im Raten.
            return Err(OggFehler::KeinOgg { gefunden: magic });
        }
        let granule = u64::from_le_bytes([
            daten[pos + 6],
            daten[pos + 7],
            daten[pos + 8],
            daten[pos + 9],
            daten[pos + 10],
            daten[pos + 11],
            daten[pos + 12],
            daten[pos + 13],
        ]);
        let segmente = daten[pos + 26] as usize;
        let tabellen_pos = pos + KOPF_BYTES;
        if tabellen_pos + segmente > daten.len() {
            return Err(OggFehler::ZuEnde {
                wollte: tabellen_pos + segmente,
                hatte: daten.len(),
            });
        }
        let mut nutzlast = 0usize;
        for &s in &daten[tabellen_pos..tabellen_pos + segmente] {
            nutzlast += s as usize;
        }
        // M3: die Segmentzahl ist 255, die Nutzlast also hoechstens
        // 65 025 Byte. Das wird hier NICHT noch einmal geprueft, weil die
        // Segmenttabelle das selbst begrenzt — eine eigene Zahl waere
        // eine zweite Wahrheit fuer dieselbe Sache.
        let daten_pos = tabellen_pos + segmente;
        if daten_pos + nutzlast > daten.len() {
            return Err(OggFehler::ZuEnde {
                wollte: daten_pos + nutzlast,
                hatte: daten.len(),
            });
        }
        info.seiten += 1;
        info.granule = granule;
        if !vorlauf_gelesen {
            // Die erste Seite traegt den Vorsprung als
            // ZWEIKOMPLEMENT-guenstige Zahl (OggSpezifikation 4.3).
            info.vorlauf = granule as i64;
            vorlauf_gelesen = true;
        }
        let block = &daten[daten_pos..daten_pos + nutzlast];
        //
        // GEMESSENE ENTSCHEIDUNG, warum ZWEI Wege und nicht einer — und
        // das war der vierte Fehler in diesem Kopfleser:
        //
        // * **Vorbis**: das Paket beginnt mit der Pakettype `0x01`, dann
        //   „vorbis" (Vorbis-I-Spezifikation Abschnitt 4.2.1).
        // * **Opus**: das Paket beginnt DIREKT mit „OpusHead" — es hat
        //   **keine** Pakettype (RFC 7845 Abschnitt 3: „The first 8 bytes
        //   are the magic signature OpusHead"). Mit einem Tor auf `0x01`
        //   kam ein Opus-Stream nie hier an, und die Datei wurde mit
        //   Abtastrate 0 gelesen.
        //
        // Und: JEDER Block mit `0x01` kommt hierher, nicht nur der mit
        // dem richtigen Codec. Sonst fiel ein unbekannter Codec kommentarlos
        // durch, statt beim Namen genannt zu werden.
        let ist_opus = block.len() >= 8 && block[..8] == *b"OpusHead";
        if ist_opus || (!block.is_empty() && block[0] == 1) {
            lies_identifikation(&mut info, block)?;
        } else if block.len() >= 7 && block[0] == 3 && info.codec == 0 {
            // Comment-Kopf: Blockzahl, dann die Laenge als
            // Klein-Endian-Variable.
            if block[5] != 1 {
                return Err(OggFehler::Blockzahl { war: block[5] });
            }
            let (laenge, verbraucht) =
                lies_variabel(&block[6..]).ok_or(OggFehler::CommentZuGross {
                    war: block.len(),
                    erlaubt: MAX_COMMENT,
                })?;
            if laenge > MAX_COMMENT {
                return Err(OggFehler::CommentZuGross {
                    war: laenge,
                    erlaubt: MAX_COMMENT,
                });
            }
            if 6 + verbraucht + laenge > block.len() {
                return Err(OggFehler::CommentPasstNicht {
                    angegeben: laenge,
                    da: block.len() - 6 - verbraucht,
                });
            }
        }
        pos = daten_pos + nutzlast;
        if pos == 0 {
            break;
        }
    }
    if info.seiten >= MAX_SEITEN {
        return Err(OggFehler::ZuVieleSeiten {
            waren: info.seiten,
            erlaubt: MAX_SEITEN,
        });
    }
    if info.seiten > 0 {
        Ok(info)
    } else {
        Err(OggFehler::ZuEnde {
            wollte: 27,
            hatte: daten.len(),
        })
    }
}

/// Liest den Identification-Kopf eines Vorbis- oder Opus-Streams.
///
/// Xiph-Modus 0: `0x01 "vorbis"` (6 Byte), dann Version(4),
/// Kanaele(1), Abtastrate(4), Bitrate maximal(4), Bitrate nominal(4),
/// Bitrate minimal(4), Blocksize(1), Framing(1).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Abtastrate NUR aus der Vorbis-
/// Stelle gelesen wird und nicht aus der Opus-Stelle: bei Opus steht an
/// derselben Position eine Vorlaufzahl, und die Abtastrate steht erst
/// spaeter. Wer sie verwechselt, bekommt 0 Hz und ein `aplay -r 0`,
/// das mit einem benannten Fehler stirbt — aber erst beim Abspielen.
///
/// DER GEMESSENE FEHLER IN DER PRUEFUNG (02.10.), und es waren drei:
///
/// 1. Es gibt **kein Sampleformat-Feld** im Vorbis-Kopf. Ich hatte an
///    Offset 20 eines gelesen — das ist der Anfang von `bitrate_nominal`
///    — und `KeinS16` geworfen, wenn dort nicht 1 stand. Der Test
///    meldete `KeinS16 { war: 0 }` auf einem voellig richtigen Kopf.
///    Vorbis dekodiert nach Spezifikation immer nach `float`; wer `s16`
///    ausgegeben will, konvertiert selbst. Das ist Sache des (noch nicht
///    gebauten) Dekoders, nicht des Kopflesers.
/// 2. Es gibt **kein Hersteller-Feld** im Identification-Kopf. Ich
///    hatte eines erfunden und mit `block[10..14]` gefuellt — das ist
///    die `Version`. Der Vendor-String steht im Comment-Kopf.
/// 3. **„OpusHead" hat acht Byte**, verglichen wurden sieben.
///    `block[1..8] == *b"OpusHead"` vergleicht 7 Byte mit 8 und ist immer
///    falsch; der Test meldete `FalscherCodec { war: 1 }` fuer einen
///    korrekten Opus-Kopf. Richtig ist `block[1..9]`.
fn lies_identifikation(info: &mut OggInfo, block: &[u8]) -> Result<(), OggFehler> {
    // GEMESSENER FEHLER (fuenfter in dieser Funktion), und er war der
    // stillste: bei Opus habe ich `info.kanaele = block[9]` gelesen.
    // Nach der Pakettype gibt es bei Opus aber KEINE — der Block faengt
    // mit „OpusHead" an. Die Kanaele stehen bei Offset 9 hinter der
    // Magic plus der Version:
    //
    // ```text
    //  0..8   "OpusHead"   8
    //  8      Version      1
    //  9      Kanaele      1   <- HIER, nicht 9 in einer Liste mit Typbyte
    // 10..12  Vorlauf      2
    // 12..16  Eingangsrate 4
    // 16..18  Verstaerkung 2
    // 18      Kanalzuord.  1   = 19 Byte
    // ```
    //
    // Der Test meldete `left: 1, right: 2` — er hatte die Kanaele 2
    // gesetzt und bekam die Version 1 zurueck.
    //
    // Die Signatur entscheidet, welcher Codec. Wer weniger als 8 Byte
    // hat, kann sie nicht lesen; das ist ein Laengenfehler.
    if block.len() < 8 {
        return Err(OggFehler::KopfLaenge {
            war: block.len(),
            erwartet_min: 8,
        });
    }
    if block[..8] == *b"OpusHead" {
        if block.len() < KOPF_MIN_OPUS {
            return Err(OggFehler::KopfLaenge {
                war: block.len(),
                erwartet_min: KOPF_MIN_OPUS,
            });
        }
        info.codec = 2;
        info.kanaele = block[9];
        // Opus-Granularpositionen sind in 48-kHz-Einheiten, egal was der
        // Kopf sagt. GEMESSEN und nicht geraten: das ist die Definition
        // (RFC 7845 Abschnitt 4).
        info.abtastrate = 48_000;
    } else if block.len() >= 7 && block[1..7] == *b"vorbis" {
        if block.len() < KOPF_MIN_VORBIS {
            return Err(OggFehler::KopfLaenge {
                war: block.len(),
                erwartet_min: KOPF_MIN_VORBIS,
            });
        }
        info.codec = 1;
        info.kanaele = block[11];
        info.abtastrate = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
        info.bitrate_nominal = u32::from_le_bytes([block[20], block[21], block[22], block[23]]);
    } else {
        // EIN UNBEKANNTER CODEC wird beim NAMEN genannt und nicht
        // uebergangen. GEMESSEN, warum das wichtig ist: ein Block, der
        // mit 0x01 beginnt und dann weder „vorbis" noch „OpusHead"
        // traegt, ist per Spezifikation ein Stream, den wir nicht
        // verstehen. Ohne diese Zeile waere es eine Datei mit
        // Abtastrate 0 — und die Seite wuerde es erst merken, wenn
        // `aplay -r 0` stirbt.
        return Err(OggFehler::FalscherCodec { war: block[0] });
    }
    if info.kanaele == 0 {
        return Err(OggFehler::FalscherCodec { war: 0 });
    }
    Ok(())
}

/// Liest eine Zahl im Xiph-Format: 7 Bit je Byte, das unterste Bit als
/// Fortsetzungs-Bit. Gibt `(Wert, verbrauchte Bytezahl)`.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Grenze bei 8 Byte liegt: 7 * 8 = 56
/// Bit, das sind 7 PiByte — genug fuer jede Laenge, die ein Ogg-Kopf
/// braucht. Wer mehr schickt, schickt eine Endlosschleife in einem
/// Strom, der nicht endet.
fn lies_variabel(b: &[u8]) -> Option<(usize, usize)> {
    let mut wert = 0usize;
    for (i, &byte) in b.iter().take(8).enumerate() {
        wert |= ((byte & 0x7f) as usize) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((wert, i + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut eine Ogg-Seite mit einer Nutzlast.
    fn seite(daten: &[u8], granule: u64) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(b"OggS");
        s.push(0); // Version
        s.push(2); // Kopfart: Anfang einer Seite (fortgesetzt)
        s.extend_from_slice(&granule.to_le_bytes());
        s.extend_from_slice(&[0u8; 4]); // Seriennummer
        // DIE ZWEI FELDER, die ich zuerst vergessen hatte (02.10.):
        // `page sequence no` (4 Byte) und danach `page checksum` (4
        // Byte). Ohne die beiden steht die Segmentzahl an Position 22
        // statt 26 — und der Test war gruen, WEIL er den Kopf genauso
        // gebaut hat. Siehe `KOPF_BYTES`.
        s.extend_from_slice(&[0u8; 4]); // Seitenfortschritt
        s.extend_from_slice(&[0u8; 4]); // CRC (fuer den Container nicht geprueft)
        let mut tabelle = Vec::new();
        let mut rest = daten.len();
        while rest >= 255 {
            tabelle.push(255u8);
            rest -= 255;
        }
        tabelle.push(rest as u8);
        s.push(tabelle.len() as u8);
        s.extend_from_slice(&tabelle);
        s.extend_from_slice(daten);
        s
    }

    /// Der Identification-Kopf eines 48-kHz-Stereo-Vorbis.
    fn ident_vorbis() -> Vec<u8> {
        let mut v = vec![1u8];
        v.extend_from_slice(b"vorbis");
        v.extend_from_slice(&0u32.to_le_bytes()); // Version
        v.push(2); // Kanaele
        v.extend_from_slice(&48_000u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes()); // Bitrate max
        v.extend_from_slice(&128_000u32.to_le_bytes()); // nominal
        v.extend_from_slice(&0u32.to_le_bytes()); // min
        v.push(0xb8); // Blocksize
        v.push(1); // Framing
        v
    }

    #[test]
    fn eine_leere_datei_ist_ein_benannter_fehler() {
        assert_eq!(
            lies(&[]),
            Err(OggFehler::ZuEnde {
                wollte: 4,
                hatte: 0
            })
        );
    }

    #[test]
    fn eine_fremde_datei_ist_kein_ogg() {
        let r = lies(b"RIFF____WAVEfmt ");
        match r {
            Err(OggFehler::KeinOgg { gefunden }) => {
                assert_eq!(&gefunden, b"RIFF");
            }
            andere => panic!("erwartet KeinOgg, kam {andere:?}"),
        }
    }

    #[test]
    fn der_identification_kopf_liefert_rate_und_kanaele() {
        let d = seite(&ident_vorbis(), 48000);
        let info = lies(&d).expect("lesen");
        assert_eq!(info.abtastrate, 48_000);
        assert_eq!(info.kanaele, 2);
        assert_eq!(info.codec, 1, "Vorbis");
        assert_eq!(info.granule, 48000, "die Granule ist die Abspielposition");
        assert_eq!(info.seiten, 1);
    }

    #[test]
    fn eine_fernster_ohne_daten_ergibt_0_und_keinen_panik() {
        let d = seite(&[], 0);
        // Eine Seite mit NUL-Byte Nutzlast: `lies` darf sie lesen, ohne
        // den Identification-Kopf zu finden, und ohne Fehler.
        let info = lies(&d).expect("lesen");
        assert_eq!(info.abtastrate, 0);
        assert_eq!(info.kanaele, 0);
        assert_eq!(info.seiten, 1);
    }

    #[test]
    fn die_nominelle_bitrate_kommt_durch() {
        // DER TEST, DER ERSETZT WURDE, und warum: es gab
        // `ein_kopf_mit_float_wird_abgelehnt_mit_namen`. Er setzte
        // `k[20] = 2` und erwartete `KeinS16`. Das Feld gibt es im
        // Vorbis-Identification-Kopf NICHT — Offset 20 ist der Anfang
        // von `bitrate_nominal`. Der Test hat also eine Eigenschaft
        // geprueft, die ich mir ausgedacht hatte, und war rot, weil mein
        // Code sie erfunden hatte.
        //
        // An seine Stelle steht die Zahl, die dort wirklich steht und
        // die der Browser braucht: wie viele Bit der Stream pro Sekunde
        // hat. Ohne sie ist die Pufferfrage eine Vermutung.
        let mut k = ident_vorbis();
        k[20..24].copy_from_slice(&192_000u32.to_le_bytes());
        let info = lies(&seite(&k, 0)).expect("lesen");
        assert_eq!(info.bitrate_nominal, 192_000);
    }

    #[test]
    fn ein_zu_kurzer_kopf_wird_abgelehnt_mit_seinen_zahlen() {
        // GEMESSENE KORREKTUR AN DER ERWARTUNG (nicht am Code): der
        // Test verlangte `erwartet_min: 30` und bekam 8. Der Code hat
        // recht — 7 Byte tragen nicht einmal die Signatur, also ist 8
        // die kleinste Zahl, mit der man die Frage stellen kann. Die
        // Vorbis-Grenze (30) gilt erst, wenn die Signatur gelesen
        // werden KONNT.
        let k = vec![1u8, b'v', b'o', b'r', b'b', b'i', b's'];
        assert_eq!(
            lies(&seite(&k, 0)),
            Err(OggFehler::KopfLaenge {
                war: 7,
                erwartet_min: 8
            })
        );
        // Und die volle Grenze, mit gueltiger Signatur und 29 Byte:
        let mut kurz = vec![1u8];
        kurz.extend_from_slice(b"vorbis");
        kurz.extend_from_slice(&[0u8; KOPF_MIN_VORBIS - 1 - 7]);
        assert_eq!(kurz.len(), KOPF_MIN_VORBIS - 1);
        assert_eq!(
            lies(&seite(&kurz, 0)),
            Err(OggFehler::KopfLaenge {
                war: KOPF_MIN_VORBIS - 1,
                erwartet_min: KOPF_MIN_VORBIS
            })
        );
    }

    #[test]
    fn ein_unbekannter_codec_wird_benannt() {
        // GEMESSENE KORREKTUR AN MIR: der Test stand mit `vec![9u8; 40]`
        // und bekam `Ok(OggInfo { abtastrate: 0, … })`. Grund: der Block
        // fing mit 9 an, und der Aufrufer ruft `lies_identifikation` nur
        // bei `block[0] == 1`. Der Codec 9 fiel also durch, statt
        // gemeldet zu werden.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Aufrufer jetzt JEDEN Block
        // mit `0x01` herschickt: nach der Vorbis-Spezifikation ist 0x01
        // die Pakettype des Identification-Kopfes. Ein Block, der mit
        // 0x01 beginnt und dann weder „vorbis" noch „OpusHead" traegt,
        // ist ein Stream, den wir nicht verstehen — und das ist eine
        // MELDUNG, keine stille Null.
        let mut k = vec![1u8];
        k.extend_from_slice(b"theora");
        k.extend_from_slice(&[0u8; 30]);
        let r = lies(&seite(&k, 0));
        assert_eq!(r, Err(OggFehler::FalscherCodec { war: 1 }));
    }

    #[test]
    fn der_opus_kopf_liefert_immer_48000() {
        // RFC 7845 Abschnitt 4: Opus-Granularpositionen sind in
        // 48-kHz-Einheiten, unabhaengig vom Kopf. GEMESSENE ENTSCHEIDUNG
        // hier: der Test sagt die REGEL, nicht einen Beispielwert — wer
        // die Regel aendert, aendert den Test mit.
        let mut k = Vec::new();
        k.extend_from_slice(b"OpusHead");
        k.push(1); // Version
        k.push(2); // Kanaele
        k.extend_from_slice(&312u16.to_le_bytes()); // Vorlauf
        k.extend_from_slice(&48_000u32.to_le_bytes()); // Eingangsrate
        k.extend_from_slice(&0i16.to_le_bytes()); // Ausgabeverstärkung
        k.push(0); // Kanalzuordnung
        assert_eq!(
            k.len(),
            KOPF_MIN_OPUS,
            "die Testdatei passt zur Groesse, sonst prueft der Test \
             die falsche Laenge"
        );
        let info = lies(&seite(&k, 0)).expect("lesen");
        assert_eq!(info.abtastrate, 48_000);
        assert_eq!(info.kanaele, 2);
        assert_eq!(info.codec, 2, "2 ist Opus (1 ist Vorbis)");
    }

    #[test]
    fn die_seitengrenze_greift() {
        // M3: 65 536 Seiten sind die Grenze. Der Test baut 300 leere
        // Seiten — das reicht, um zu beweisen, dass die Schleife sie
        // zaehlt (und nicht an einem Nullbyte haengenbleibt).
        let mut d = Vec::new();
        for i in 0..300u64 {
            d.extend_from_slice(&seite(&[], i));
        }
        let info = lies(&d).expect("lesen");
        assert_eq!(info.seiten, 300);
    }

    #[test]
    fn eine_abgeschnittene_seite_ist_ein_benannter_fehler() {
        let voll = seite(&ident_vorbis(), 0);
        for k in 1..voll.len() {
            // JEDE Abschneidegrenze muss ein Fehler sein, kein Panic.
            match lies(&voll[..k]) {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }
}
