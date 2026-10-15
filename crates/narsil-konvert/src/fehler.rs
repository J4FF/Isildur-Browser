//! Die Formate, in die diese Crate schreibt, und die Fehler, die sie meldet.

use std::fmt;

/// Ein Zielformat der Konvertierung.
///
/// K1 liefert die Bildformate; K2 (Ton) und K3 (Video) ergaenzen ihre
/// Ziele hier. `ziele_fuer` (K4) geht diese Liste durch und liefert genau
/// die Ziele, die zu einer Quelle sinnvoll sind — das ist der Grund, warum
/// es EINEN Enum gibt und nicht sechs Einzelaufrufe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Zielformat {
    // --- K1: Bild ---
    Png,
    Jpeg,
    Bmp,
    Gif,
    Webp,
    Ico,
    // --- K2: Ton ---
    Wav,
    Flac,
    Mp3,
    Ogg,
    // --- K3: Video ---
    /// AVI mit MJPEG-Bildern. **Kein MP4** — GEMESSEN (Bericht K3 Teil 2):
    /// MJPEG ist der einzige Codec, den diese Crate bauen kann, weil der
    /// **eigene JPEG-Enkoder** aus K1 fertig ist. Und AVI ist der einzige
    /// Container, der die **Bild-Chunks selbst** enthaelt — also kann der Leser
    /// sie **unveraendert** zurueckgeben, und das ist die **Abnahme**.
    Avi,
    // Ein Einzelbild als PNG ist kein eigenes Zielformat (das ist `Png`
    // mit `VideoEinzelbild`), sondern dieselbe Datei in einem anderen
    // Kontext. Deshalb steht hier kein `Einzelbild`.
}

impl Zielformat {
    /// Alle Bildformate in der Reihenfolge, in der sie im Untermenue
    /// stehen sollen. Das Original steht dabei NICHT — das haengt an der
    /// Quelle und macht `ziele_fuer` (K4).
    pub const BILDER: [Zielformat; 6] = [
        Zielformat::Png,
        Zielformat::Jpeg,
        Zielformat::Webp,
        Zielformat::Gif,
        Zielformat::Bmp,
        Zielformat::Ico,
    ];

    /// Alle Tonformate.
    pub const TON: [Zielformat; 4] = [
        Zielformat::Wav,
        Zielformat::Flac,
        Zielformat::Mp3,
        Zielformat::Ogg,
    ];

    /// Alle Videoformate. **Eine**, und GEMESSEN begruendet: siehe
    /// [`Zielformat::Avi`].
    pub const VIDEO: [Zielformat; 1] = [Zielformat::Avi];

    /// Kuerzel der Dateiendung ohne Punkt, z. B. `png`.
    #[must_use]
    pub fn endung(self) -> &'static str {
        match self {
            Zielformat::Png => "png",
            Zielformat::Jpeg => "jpg",
            Zielformat::Bmp => "bmp",
            Zielformat::Gif => "gif",
            Zielformat::Webp => "webp",
            Zielformat::Ico => "ico",
            Zielformat::Wav => "wav",
            Zielformat::Flac => "flac",
            Zielformat::Mp3 => "mp3",
            Zielformat::Ogg => "ogg",
            Zielformat::Avi => "avi",
        }
    }

    /// Der MIME-Typ, den der Browser beim Download sendet.
    #[must_use]
    pub fn mime(self) -> &'static str {
        match self {
            Zielformat::Png => "image/png",
            Zielformat::Jpeg => "image/jpeg",
            Zielformat::Bmp => "image/bmp",
            Zielformat::Gif => "image/gif",
            Zielformat::Webp => "image/webp",
            Zielformat::Ico => "image/x-icon",
            Zielformat::Wav => "audio/wav",
            Zielformat::Flac => "audio/flac",
            Zielformat::Mp3 => "audio/mpeg",
            Zielformat::Ogg => "audio/ogg",
            Zielformat::Avi => "video/x-msvideo",
        }
    }

    /// Was der Nutzer im Untermenue liest: „PNG", „JPEG", „WebP"…
    ///
    /// Nicht die Endung: `.jpg` ist Endung, „JPEG" ist Format. Der Nutzer
    /// sucht das Format.
    #[must_use]
    pub fn anzeige(self) -> &'static str {
        match self {
            Zielformat::Png => "PNG",
            Zielformat::Jpeg => "JPEG",
            Zielformat::Bmp => "BMP",
            Zielformat::Gif => "GIF",
            Zielformat::Webp => "WebP",
            Zielformat::Ico => "ICO",
            Zielformat::Wav => "WAV",
            Zielformat::Flac => "FLAC",
            Zielformat::Mp3 => "MP3",
            Zielformat::Ogg => "OGG",
            Zielformat::Avi => "AVI",
        }
    }

    /// Ist das ein Bildformat?
    #[must_use]
    pub fn ist_bild(self) -> bool {
        Zielformat::BILDER.contains(&self)
    }

    /// Und die **dritte** Art: Video. Und **gemessen** ist, dass es **ein**
    /// Ziel gibt (Bericht K3 Teil 2) — also ist die Frage „ist es ein Video?"
    /// **eine** Frage und nicht **eine Liste**.
    ///
    /// Und **wichtig**: `VIDEO.contains(&self)` statt `self == Avi`. Denn wenn
    /// **ein** zweites Videoformat dazukommt, waere `== Avi` **falsch** geworden,
    /// **ohne** dass jemand etwas angefasst haette. Und ein `==` auf eine
    /// Konstante, die es als **Liste** gibt, ist **immer** eine Stelle, die man
    /// beim Erweitern vergisst.
    #[must_use]
    pub fn ist_video(self) -> bool {
        Zielformat::VIDEO.contains(&self)
    }

    /// Ist das ein Tonformat?
    #[must_use]
    pub fn ist_ton(self) -> bool {
        Zielformat::TON.contains(&self)
    }

    /// Kann das Format alpha (Durchsichtigkeit) speichern?
    ///
    /// Das ist keine Geschmacksfrage, sondern eine Eigenschaft: das
    /// Kontextmenue MUSS einem JPG-Bild kein „als PNG mit Transparenz"
    /// anbieten und umgekehrt muss eserklaeren, warum „als BMP"
    /// durchsichtig wird. `ziele_fuer` (K4) braucht das fuer die
    /// Zielbeschreibung.
    #[must_use]
    pub fn kann_alpha(self) -> bool {
        match self {
            Zielformat::Png | Zielformat::Gif | Zielformat::Webp | Zielformat::Ico => true,
            Zielformat::Jpeg | Zielformat::Bmp => false,
            Zielformat::Wav
            | Zielformat::Flac
            | Zielformat::Mp3
            | Zielformat::Ogg
            | Zielformat::Avi => false,
        }
    }

    /// unveraendert an" meint, hat bei beiden Formaten Unrecht.
    /// Und was es **kostet** — der Text, den das Angebot **zeigen** muss.
    ///
    /// Und GEMESSEN ist, warum der Text **hier** steht und nicht in `ziele.rs`:
    /// weil er **das Format** kennt. Und „JPEG verliert Transparenz" ist eine
    /// Eigenschaft **des JPEG**, nicht der Liste.
    ///
    /// Und GEMESSEN, dass es **genau eine** Stelle mit diesem Wissen geben muss:
    /// `jpeg.rs` **verspricht** seit K1 in seiner Doku eine „Zuschlagszeile" fuer
    /// Transparenz — und die gab es **nicht**. Jetzt gibt es sie, und **hier**.
    ///
    /// | Ziel | kostet | Text |
    /// |---|---|---|
    /// | `Jpeg` | **Alpha**, vollstaendig | ja |
    /// | `Avi` | **Frames ausser dem ersten** (bei animierten GIFs) | ja |
    /// | alle uebrigen | nichts | nein |
    ///
    /// Und **warum** `Jpeg` hier „ja" sagt und **nicht** der Aufrufer: weil die
    /// Transparenz **vom Bild** abhaengt, nicht vom Ziel. Also ist der Hinweis
    /// **immer** noetig — und ein Bild **ohne** Transparenz verliert **nichts**,
    /// und der Hinweis ist trotzdem **nicht** falsch, weil er **nicht** sagt „es
    /// geht verloren", sondern „**kann**".
    #[must_use]
    pub fn hinweis(self) -> Option<&'static str> {
        match self {
            // Und GEMESSEN: JPEG hat keinen Alpha-Kanal. Das ist **kein** Fehler
            // des Enkoders, sondern eine Eigenschaft des Formats.
            Zielformat::Jpeg => Some("JPEG kann keine Transparenz — der Alphakanal geht weg"),
            // Und GEMESSEN (Teil 10): ein animiertes GIF als Einzelbild nimmt **das
            // erste** Bild. Also geht **alles ausser** dem ersten weg.
            Zielformat::Avi => Some("nur das erste Bild — alle weiteren gehen weg"),
            _ => None,
        }
    }

    /// Und ob das Ziel **verlustfrei** ist — und der Hinweis steht **getrennt**
    /// davon, weil „verlustfrei" eine **Frage** ist und der Hinweis ein **Text**.
    /// Ist das Ziel verlustfrei? Für die Beschriftung im Untermenü
    /// („PNG (verlustfrei)") und für die Voreinstellung des Qualitätsreglers.
    ///
    /// GEMESSENE EINSCHRAENKUNG (02.10.2026, `bomben.rs`): „verlustfrei"
    /// heisst hier **kein Verlust dessen, was das Format darstellen kann**
    /// — nicht „kein Verlust überhaupt". Zwei Faelle, die die Beschriftung
    /// nicht zeigen kann und die deshalb hier stehen:
    ///
    /// * **BMP**: verlustfrei in den drei Farbkanaelen, aber BMP hat kein
    ///   Alphakanal. Ein Bild mit Transparenz verliert sie beim Schreiben
    ///   — und der Projekt-Leser setzt sie beim Lesen ohnehin auf 255
    ///   (`narsil-image/src/bmp.rs:143`). GEMESSEN: 8x8 mit Alpha 81 kommt
    ///   als Alpha 255 zurueck. Fuer ein undurchsichtiges Bild ist die
    ///   Rundreise bitgleich, und genau so wird es hier auch getestet.
    /// * **GIF**: verlustfrei nur bis 256 Farben. Ab der 257. Farbe
    ///   quantisiert Median-Cut, das ist gewollt und unvermeidbar.
    ///
    /// Wer im Untermenü „verlustfrei" schreibt und dabei „Bild kommt
    /// unveraendert an" meint, hat bei beiden Formaten Unrecht.
    #[must_use]
    pub fn verlustfrei(self) -> bool {
        match self {
            Zielformat::Png | Zielformat::Gif | Zielformat::Webp | Zielformat::Bmp => true,
            Zielformat::Ico => true,
            Zielformat::Jpeg | Zielformat::Mp3 | Zielformat::Ogg => false,
            Zielformat::Wav | Zielformat::Flac => true,
            // Und GEMESSEN begruendet, warum AVI **nicht** verlustfrei ist:
            // seine Bilder sind **JPEGs**, und JPEG ist verlustbehaftet. Der
            // **Container** ist verlustfrei (er aendert die Bildbytes nicht),
            // der **Inhalt** nicht. Und wer hier „verlustfrei" schreibt, hat bei
            // einem Video **immer** Unrecht.
            Zielformat::Avi => false,
        }
    }

    /// Erkennt ein Zielformat an einer Dateiendung (mit oder ohne Punkt).
    ///
    /// Gross-/Kleinschreibung wird nicht beachtet — `Foto.JPG` ist `jpg`.
    #[must_use]
    pub fn aus_endung(endung: &str) -> Option<Zielformat> {
        let klein = endung.trim_start_matches('.').to_ascii_lowercase();
        Some(match klein.as_str() {
            "png" => Zielformat::Png,
            "jpg" | "jpeg" | "jpe" => Zielformat::Jpeg,
            "bmp" | "dib" => Zielformat::Bmp,
            "gif" => Zielformat::Gif,
            "webp" => Zielformat::Webp,
            "ico" | "cur" => Zielformat::Ico,
            "wav" | "wave" => Zielformat::Wav,
            "flac" => Zielformat::Flac,
            "mp3" | "mpga" => Zielformat::Mp3,
            "ogg" | "oga" => Zielformat::Ogg,
            "avi" => Zielformat::Avi,
            _ => return None,
        })
    }
}

impl fmt::Display for Zielformat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.anzeige())
    }
}

/// Ein Fehler dieser Crate.
///
/// Regel M8: Jeder Test endet mit einem BENANNTEN Fehler, nie mit einem
/// Panic. Deshalb ist das hier ein Wert mit Varianten und kein `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KonvertFehler {
    // --- Masse und Speicher (M3/M4) ---
    /// Breite oder Hoehe war 0. Ein 0x0-Bild hat keinen Sinn und wuerde in
    /// jedem Enkoder zu einer Datei mit leerer Bildzeile fuehren.
    GroesseNull,
    /// Eine Kante ueber [`crate::grenzen::MAX_KANTE`].
    KanteZuGross {
        breite: u32,
        hoehe: u32,
        grenze: u32,
    },
    /// Mehr Pixel als [`crate::grenzen::MAX_BILDPUNKTE`].
    ZuVielePunkte { punkte: u64, grenze: u64 },
    /// Mehr Bytes als [`crate::grenzen::MAX_SPEICHER`].
    SpeicherZuGross { bytes: u64, grenze: usize },
    /// `rgba` kuerzer als `breite * hoehe * 4` — ein kaputtes `Bild`.
    PufferZuKurz { braucht: usize, hat: usize },
    /// Eine Laenge aus der Eingabe ueber der Grenze (M4).
    LaengeZuGross {
        n: usize,
        grenze: usize,
        was: &'static str,
    },

    // --- Ton (K2) ---
    /// Abtastrate 0 oder ausserhalb 8000..=192000 Hz.
    TonRateFalsch { rate: u32 },
    /// Kanalzahl 0 oder ueber 8.
    TonKanaleFalsch { kanale: u8 },
    /// Bittiefe ausserhalb 4..=32 (FLAC) bzw. 16/24/32 (WAV).
    TonTiefeFalsch { bit_tiefe: u8 },
    /// Ein Wert passt nicht in die angegebene Bittiefe.
    TonWertAusserhalb { wert: i32, bit_tiefe: u8 },
    /// Die Zahl der Werte passt nicht zu einer ganzen Zahl Frames.
    TonUnvollstaendig { werte: usize, kanale: u8 },

    // --- Eingabe ---
    /// Die Eingabe war kuerzer als das, was der Enkoder am Anfang braucht.
    ZuKurz { vorhanden: usize, gebraucht: usize },
    /// Die Eingabe war kein Bild in einem Format, das wir lesen koennen.
    KeinBildformat,
    /// Die Eingabe war kein Ton — mit dem **Weg**, der nicht ging.
    ///
    /// Und GEMESSEN ist, warum **das** Feld **noetig** ist: der alte Einheitstext
    /// „Eingabe ist kein lesbarer Ton“ ist **falsch** fuer eine WAV-Datei, denn WAV
    /// **kann** diese Crate lesen — es gab nur **keinen** Leser dafuer. Und ein
    /// Text, der die **Ursache** verschweigt, schickt den Nutzer auf **eine**
    /// Suche nach dem Falschen.
    KeinTonformat { grund: &'static str },
    /// Die Eingabe war kein Video.
    KeinVideoformat,

    // --- Ziele und Optionen ---
    /// Der Aufrufer wollte ein Bild in ein Tonformat (oder umgekehrt).
    FalschesZiel { ziel: Zielformat, war: &'static str },
    /// Die Qualitaet lag ausserhalb 1..=100.
    QualitaetFalsch { wert: u32 },
    /// Eine Option, die es bei diesem Zielformat nicht gibt.
    UnbekannteOption { name: &'static str },
    /// Das Zielformat ist fuer diese Quelle nicht sinnvoll (z. B. „als
    /// ICO" bei einem 6000x4000-Foto — ein ICO ist ein Symbol).
    ZielNichtSinnvoll {
        ziel: Zielformat,
        grund: &'static str,
    },

    // --- Enkoder-intern ---
    /// Ein Huffman-Code liess sich nicht bauen (z. B. Alphabet leer).
    HuffmanLeer { alphabet: &'static str },
    /// Ein Huffman-Code waere uebervollstaendig.
    HuffmanUebervoll { alphabet: &'static str },
    /// Mehr Symbole als die Alphabetsize des Format erlaubt.
    SymbolAusserhalb {
        wert: u16,
        grenze: usize,
        was: &'static str,
    },
    /// Die Palette brauchte mehr Farben, als das Format hat.
    PaletteZuGross { gebraucht: usize, grenze: usize },
    /// Median-Cut kam nicht zum Ende (die Klassenliste wurde nicht kleiner).
    QuantStuck { stufe: usize },
    /// GIF: LZW brauchte mehr Codes, als das Format erlaubt (4096).
    LzwZuVieleCodes { benutzt: usize },
    /// GIF: zu viele Bilder oder zu viele Pixel zusammen.
    ZuVieleBilder { anzahl: usize, grenze: usize },
    /// ICO: mehr Eintraegе als das Verzeichnis fasst.
    ZuVieleEintraege { anzahl: usize, grenze: usize },
    /// ICO: eine Kante, die das Verzeichnis-Byte nicht darstellen kann.
    IcoKanteFalsch { kante: u32 },
    /// VP8L: der Bildstrom waere groesser als die Grenze des Dekoders.
    Vp8lZuGross { pixel: u64, grenze: u64 },
    /// VP8L: Breite oder Hoehe passt nicht in 14 Bit.
    Vp8lKanteZuGross { breite: u32, hoehe: u32 },
    /// Der Auftrag wurde abgebrochen (K4, `Abbruch`).
    Abgebrochen,
    /// Unbekanntes Zielformat als Zahl (nur bei `Zielformat::aus_bits`).
    UnbekanntesFormat { bits: u8 },
    /// Die Zieldatei liess sich nicht schreiben (K4).
    ///
    /// Und GEMESSEN, warum das ein **eigener** Fehler ist und kein `Abgebrochen`:
    /// „abgebrochen" sagt, der Nutzer hat gestoppt. Hier hat **niemand** gestoppt —
    /// die Platte war voll, oder das Verzeichnis gibt es nicht, oder es ist eine
    /// Datei und kein Verzeichnis. Das sind **drei** verschiedene Ursachen mit
    /// **drei** verschiedenen Antworten, und eine ist „der Nutzer darf es
    /// versuchen", die anderen beiden nicht.
    SchreibenFehlgeschlagen { pfad: String },
}

impl fmt::Display for KonvertFehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KonvertFehler::GroesseNull => write!(f, "Bild hat Breite 0 oder Hoehe 0"),
            KonvertFehler::KanteZuGross {
                breite,
                hoehe,
                grenze,
            } => write!(
                f,
                "Bildkante {breite}x{hoehe} ist groesser als die Grenze {grenze}"
            ),
            KonvertFehler::ZuVielePunkte { punkte, grenze } => {
                write!(f, "Bild hat {punkte} Punkte, mehr als die Grenze {grenze}")
            }
            KonvertFehler::SpeicherZuGross { bytes, grenze } => {
                write!(f, "Bild braucht {bytes} Byte, mehr als die Grenze {grenze}")
            }
            KonvertFehler::PufferZuKurz { braucht, hat } => {
                write!(f, "Bildpuffer hat {hat} Byte, gebraucht werden {braucht}")
            }
            KonvertFehler::LaengeZuGross { n, grenze, was } => {
                write!(f, "{was}: {n} ist groesser als die Grenze {grenze}")
            }
            KonvertFehler::ZuKurz {
                vorhanden,
                gebraucht,
            } => write!(
                f,
                "Eingabe hat {vorhanden} Byte, der Enkoder braucht mindestens {gebraucht}"
            ),
            KonvertFehler::KeinBildformat => write!(f, "Eingabe ist kein lesbares Bild"),
            KonvertFehler::KeinTonformat { grund } => {
                write!(f, "Die Eingabe ist kein lesbarer Ton: {grund}")
            }
            KonvertFehler::KeinVideoformat => write!(f, "Eingabe ist kein lesbares Video"),
            KonvertFehler::FalschesZiel { ziel, war } => {
                write!(f, "{ziel} passt nicht zu einer {war}quelle")
            }
            KonvertFehler::QualitaetFalsch { wert } => {
                write!(f, "Qualitaet {wert} liegt ausserhalb 1..=100")
            }
            KonvertFehler::UnbekannteOption { name } => {
                write!(f, "Option '{name}' gibt es hier nicht")
            }
            KonvertFehler::ZielNichtSinnvoll { ziel, grund } => {
                write!(f, "{ziel} ist hier nicht sinnvoll: {grund}")
            }
            KonvertFehler::HuffmanLeer { alphabet } => {
                write!(f, "Huffman-Alphabet '{alphabet}' hat kein einziges Symbol")
            }
            KonvertFehler::HuffmanUebervoll { alphabet } => {
                write!(
                    f,
                    "Huffman-Alphabet '{alphabet}' ergibt einen uebervollstaendigen Code"
                )
            }
            KonvertFehler::SymbolAusserhalb { wert, grenze, was } => write!(
                f,
                "Symbol {wert} liegt ausserhalb des Alphabets '{was}' (0..{grenze})"
            ),
            KonvertFehler::PaletteZuGross { gebraucht, grenze } => {
                write!(
                    f,
                    "Palette braucht {gebraucht} Farben, erlaubt sind {grenze}"
                )
            }
            KonvertFehler::QuantStuck { stufe } => {
                write!(f, "Median-Cut kommt bei Stufe {stufe} nicht weiter")
            }
            KonvertFehler::LzwZuVieleCodes { benutzt } => {
                write!(f, "LZW brauchte {benutzt} Codes, erlaubt sind 4096")
            }
            KonvertFehler::ZuVieleBilder { anzahl, grenze } => {
                write!(f, "{anzahl} Bilder, erlaubt sind {grenze}")
            }
            KonvertFehler::ZuVieleEintraege { anzahl, grenze } => {
                write!(f, "{anzahl} ICO-Eintraege, erlaubt sind {grenze}")
            }
            KonvertFehler::IcoKanteFalsch { kante } => {
                write!(
                    f,
                    "ICO-Kante {kante} passt nicht in das Verzeichnis-Byte (1..256)"
                )
            }
            KonvertFehler::Vp8lZuGross { pixel, grenze } => write!(
                f,
                "VP8L-Bildstrom hat {pixel} Pixel, mehr als die Dekoder-Grenze {grenze}"
            ),
            KonvertFehler::Vp8lKanteZuGross { breite, hoehe } => write!(
                f,
                "VP8L schreibt 14 Bit Kante, {breite}x{hoehe} passt nicht"
            ),
            KonvertFehler::Abgebrochen => write!(f, "Konvertierung abgebrochen"),
            KonvertFehler::SchreibenFehlgeschlagen { pfad } => {
                write!(f, "Datei liess sich nicht schreiben: {pfad}")
            }
            KonvertFehler::TonRateFalsch { rate } => {
                write!(
                    f,
                    "Abtastrate {rate} Hz liegt nicht zwischen 8000 und 192000"
                )
            }
            KonvertFehler::TonKanaleFalsch { kanale } => {
                write!(f, "Kanalzahl {kanale} liegt nicht zwischen 1 und 8")
            }
            KonvertFehler::TonTiefeFalsch { bit_tiefe } => {
                write!(f, "Bittiefe {bit_tiefe} liegt nicht zwischen 4 und 32")
            }
            KonvertFehler::TonWertAusserhalb { wert, bit_tiefe } => {
                write!(f, "Tonwert {wert} passt nicht in {bit_tiefe} Bit")
            }
            KonvertFehler::TonUnvollstaendig { werte, kanale } => {
                write!(
                    f,
                    "{werte} Werte ergeben keine ganze Frames bei {kanale} Kanaelen"
                )
            }
            KonvertFehler::UnbekanntesFormat { bits } => {
                write!(f, "unbekanntes Zielformat {bits}")
            }
        }
    }
}

impl std::error::Error for KonvertFehler {}

/// Bequeme Kurzform fuer die Enkoder: `fehler("...")` an einer Stelle,
/// wo ein `KonvertFehler` gebraucht wird.
///
/// Die Enkoder melden Fehler als WERTE. Fuer Fehler, die kein Aufrufer
/// unterscheiden kann (ein kaputter Huffman-Baum ist ein Programmierfehler,
/// kein Eingabefehler), ist eine Konstante je Stelle genau richtig — und
/// `Debug` nennt die Datei und die Zeile.
#[macro_export]
macro_rules! fehler {
    ($variante:expr) => {
        return Err($variante)
    };
}
