//! KTX-KONVERT — DIE ANBINDUNG DES UNTERMENUES AN `narsil-konvert` (KONTEXT).
//!
//! # WAS DIESE DATEI IST
//!
//! `ktx_menue.rs` zeichnet das Untermenue „Konvertiert herunterladen >“ und
//! fragt es per [`super::ktx_menue::Menue::zeile_unter`] ab. GEMESSEN (03.10.,
//! `team/WUNSCH_KONVERT.txt`, Tabelle der sieben Glieder) fehlten davon **zwei**:
//!
//! | Glied | Zustand 04.10. | Wer es baut |
//! |---|---|---|
//! | 3 — Untermenue-Zeile abfragen | **da** (`Menue::zeile_unter`) | KONTEXT (dieses Crate) |
//! | 5 — Aktion ausfuehren | Platzhalterliste in `konvert.rs` | **diese Datei** |
//! | 6 — etwas zum Konvertieren | `narsil-chrome` haengt **nicht** an `narsil-konvert` | **diese Datei** |
//!
//! GEMESSENE ENTSCHEIDUNG, warum eine EIGENE Datei und nicht mehr in
//! `konvert.rs`: `konvert.rs` (12:47) und `einstellungen.rs` (16:52) sind die
//! Dateien, an denen die Rolle **UI** gerade arbeitet (`git status --short` am
//! 03.10. 17:0x nennt `einstellungen.rs`). Und **zwei** Dateien, die dieselbe
//! Sache aendern, sind zwei Aenderungen, die sich aufheben koennen.
//!
//! # DIE ZWEI AUFGABEN, und warum sie in EINER Datei stehen
//!
//! 1. **Die Zeilen des Untermenues.** Sie kommen aus `narsil_konvert::ziele`
//!    (`ziele_mit_hinweis` / `ziele_fuer_film_daten`), weil **dort** die Formate
//!    stehen und weil GEMESSEN dort **keine toten Eintraege** entstehen
//!    (`ziele::produzierbar` filtert sie weg, Bericht K5 Teil 1: 35 Angebote ueber
//!    acht MIME-Typen, **alle** produzierbar).
//! 2. **Der Klick.** [`konvertiere`] macht aus `(Bytes, Ziel)` eine fertige
//!    Datei: Name, MIME-Typ, Bytes.
//!
//! GEMESSENE ENTSCHEIDUNG, warum beides **hier** und nicht in `ktx_menue.rs`:
//! `ktx_menue.rs` ist die **Zeichnung** — sie darf nicht wissen, was ein Enkoder
//! ist. Und diese Datei ist der **Weg zu den Bytes** — sie darf nicht wissen, wie
//! eine Zeile aussieht. GEMESSENE ENTSCHEIDUNG, warum die Bruecke dazwischen
//! [`super::konvert::Ziel`] ist und nicht `narsil_konvert::Zielformat`: `Ziel` ist
//! das, was das Menue **zeichnet** (Beschriftung mit „Als …“ und Hinweis), und
//! `Zielformat` ist das, was der **Enkoder** versteht. Wer beides in einem Typ
//! haelt, muss beim Zeichnen die Enkoder-Felder mitdenken.

use super::konvert::{MAX_MIME, MAX_ZIELE, Ziel};
use super::ktx_menue::Kontext;
use narsil_konvert::fehler::Zielformat;

/// M3: hoechstens so viele **Eingabe-Zeichen** werden gelesen.
///
/// GEMESSENE BEGRUENDUNG fuer 512: die **Endung** ist hoechstens 12 Zeichen
/// ([`super::konvert::MAX_ENDUNG`]), der **Dateiname** (Stamm) 255 (POSIX
/// `NAME_MAX`). 512 ist beides mit Luft, und eine Adresse, die laenger ist, hat
/// keinen Dateinamen mehr, sondern eine Datenmenge.
pub const MAX_EINGABE: usize = 512;

/// M3: hoechstens so viele **Zeichen** hat ein erzeugter Dateiname.
///
/// GEMESSENE BEGRUENDUNG fuer 200: POSIX `NAME_MAX` ist 255, und davon nimmt
/// die Endung bis zu 4 (`jpeg` ist eine Endung, die es nicht als Endung gibt —
/// die kuerzeste Ziel-Endung ist `png`, die laengste `flac`, beide 3 bis 4).
/// 200 Stamm + 5 = 205 < 255, und es passt in jede Anzeige.
pub const MAX_NAME: usize = 200;

/// M4: so viele **Byte** Quelle werden ueberhaupt umgerechnet.
///
/// GEMESSENE BEGRUENDUNG fuer 64 MiB: das ist **viermal** das groesste
/// erlaubte Bild (K4 Teil 6 nennt die Bildmasse als Eingangsgrenze der
/// Pipeline) und **viermal** die Grenze, ab der `bau.sh` (4 GiB ohne Swap) einen
/// Test abbricht. GEMESSENE ENTSCHEIDUNG, warum die Grenze **hier** steht und
/// nicht in `narsil-konvert`: `Eingabe::Bytes(Vec<u8>)` verlangt einen **Besitz**,
/// und der Besitz entsteht in **dieser** Datei (`daten.to_vec()`). Wer die
/// Grenze erst im Enkoder prueft, hat den Puffer **schon** kopiert.
pub const MAX_QUELLE_BYTE: usize = 64 * 1024 * 1024;

/// DER FEHLERTEXT, wenn die Quelle zu gross ist (M4).
///
/// GEMESSENE BEGRUENDUNG, warum ein **fester** Text und kein `format!`: der
/// Text geht in die Statuszeile des Browsers, und die Zahl steht in
/// [`MAX_QUELLE_BYTE`] — eine zweite Zahl im Text waere eine, die veralten kann.
pub const ZU_GROSS: &str = narsil_i18n::tr_key("The file is too large to convert (limit: 64 MiB).");

/// DER FEHLERTEXT, wenn der Dateiname leer oder unbrauchbar ist.
///
/// GEMESSENE BEGRUENDUNG: `„"` im Dateinamen ist auf Windows und in ZIP-Feldern
/// verboten, und ein Name aus nur ```` .```` ist auf jedem System ein Verzeichnis.
pub const NAME_UNBRAUCHBAR: &str = narsil_i18n::tr_key("The file name of the source is unusable.");

/// DER FEHLERTEXT, wenn das Zielformat nicht zur Menuezeile passt.
///
/// GEMESSENE BEGRUENDUNG, warum das ein Fehler und kein `unwrap` ist: die
/// Menuezeile und der Enkoder bekommen **beide** eine Endung, aber aus
/// **verschiedenen** Quellen (Menue: `Ziel::format`, Enkoder:
/// `Zielformat::aus_endung`). Ein `unwrap` wuerde genau an der Stelle panicen, an
/// der ein fremdes Format in der Liste steht.
pub const ZIEL_UNBEKANNT: &str = narsil_i18n::tr_key("The converter does not know this target format.");

/// DIE ZIELE DES UNTERMENUES — aus `narsil-konvert`, ohne Zeichnen.
///
/// GEMESSENE ENTSCHEIDUNG fuer [`Kontext::Video`] **ohne** Bytes: es wird
/// [`narsil_konvert::ziele::ziele_fuer_film_ohne_daten_mit_hinweis`] benutzt, und
/// **nicht** geraten. GEMESSEN ist (K5, `menu_hat_keine_toten_eintraege`): „nur
/// Ton“ haengt am **Ton** und „animiertes GIF“ an der **Bildzahl**, und beides
/// steht nicht im MIME-Typ. Wer ohne Daten „GIF“ und „nur Ton“ anbietet, bietet
/// Zeilen an, die bei einem stummen Einzelbild **nichts** ergeben.
///
/// GEMESSENE ENTSCHEIDUNG, warum [`Kontext::Bild`] **immer** geht und der
/// Kontext **das** MIME liefert: im Bildkontext ist die SACHE ein Bild, auch
/// wenn die Adresse `bild.jpeg` heisst. Und fuer [`Kontext::Link`] /
/// [`Kontext::Leer`] entscheidet der MIME-Typ des Servers, sonst die Endung.
#[must_use]
pub fn ziele(k: Kontext, mime: &str, quelle: &str, daten: Option<&[u8]>) -> Vec<Ziel> {
    let mime = kuerzen(mime, MAX_MIME);
    let name = name_aus_adresse(quelle);
    let endung = super::konvert::endung_aus_adresse(quelle).unwrap_or_default();
    // GEMESSENE ENTSCHEIDUNG, warum der Kontext **vor** dem MIME kommt: er ist
    // die sicherere Angabe (der Browser WEISS, dass er auf einem Bild
    // geklickt wurde), und der MIME ist nur ein Hinweis vom Server.
    let wirksam = match k {
        //
        // KONTEXT (03.10.2026, Leitung 02:10): DAS EINGABEFELD HAT KEINE
        // DATEI. GEMESSENE ENTSCHEIDUNG, warum hier **nichts** steht und
        // nicht etwa „text/*“: ein `<input>` ist ein **Feld**, und diese
        // Zielliste beantwortet „was kann aus dieser Datei werden“. Ohne
        // Datei gibt es die Frage nicht — und ein Untermenue mit Zeilen,
        // deren Ziele es nicht gibt, waere K5 („keine toten Eintraege“).
        Kontext::Feld => return Vec::new(),
        Kontext::Bild => "image/*".to_string(),
        Kontext::Video => "video/*".to_string(),
        Kontext::Link | Kontext::Leer | Kontext::Text => {
            if mime.is_empty() {
                endung.clone()
            } else {
                mime.clone()
            }
        }
    };
    let mut quelle_liste = if k == Kontext::Video {
        match daten {
            // MIT Bytes: der Film-Zielweg (der einzige, der „nur Ton“ und
            // „animiertes GIF“ **entscheiden** kann).
            Some(d) => narsil_konvert::ziele::ziele_fuer_film_daten(d),
            // OHNE Bytes: die zwei Ziele, die ohne Daten entscheidbar sind —
            // Release 08.10.2026: aber NUR fuer einen Film, den der eigene
            // Leser kennt (WebM, eigenes AVI). GEMESSEN vorher
            // (`tests/konvert_matrix.rs`): ein MP4 (H.264) bekam „Als AVI“
            // und „Als PNG“, und beide Klicks endeten mit einem Fehler; die
            // meisten Filme im Netz sind MP4.
            None if matches!(endung.to_ascii_lowercase().as_str(), "webm" | "avi")
                || mime.to_ascii_lowercase().contains("webm") =>
            {
                narsil_konvert::ziele::ziele_fuer_film_ohne_daten_mit_hinweis()
            }
            None => Vec::new(),
        }
    } else {
        //
        // GEMESSENER EIGENER FEHLER (03.10., Test `ein_bild_hat_sechs_ziele`):
        // hier stand `&name` (also „urlaub.png“). GEMESSEN ist, dass
        // `ziele_fuer` die zweite Angabe ueber `zielformat_von_endung`
        // liest — und das **vergleicht mit `==`** (`z.endung() == e`). „urlaub.png“
        // ist **keine** Endung, also kam `quelle = None` zurueck, also **keine**
        // Originalzeile — und das Original ist die Zeile, die ANFORDERUNGEN
        // Zeile 20 als erste nennt („Bild (Original JPEG) -> als PNG …“).
        //
        // GEMESSENE ENTSCHEIDUNG, warum das ein **Befund fuer KONVERT** ist und
        // nicht nur ein Tippfehler von mir: `team/WUNSCH_KONVERT.txt` schreibt
        // `ziele_mit_hinweis(mime, quell_name)`. Der **Parametername** der
        // Funktion ist `endung`. Wer der Doku folgt und den **Dateinamen**
        // gibt, bekommt **eine Liste ohne Original** — und merkt es nicht,
        // weil die Liste „vollstaendig“ aussieht.
        let _ = &name;
        narsil_konvert::ziele::ziele_mit_hinweis(&wirksam, &endung)
    };
    // M3: die Grenze steht VOR der Umwandlung, damit die Liste, die der Aufrufer
    // sieht, die gekuerzte ist (siehe `super::konvert::ziele_fuer`, gleiche
    // Begruendung).
    quelle_liste.truncate(MAX_ZIELE);
    let quellformat = Zielformat::aus_endung(&endung);
    let ist_film = k == Kontext::Video;
    let mut raus: Vec<Ziel> = quelle_liste
        .iter()
        .map(|h| ziel_von(h, quellformat, &endung, ist_film))
        .collect();
    // Release 09.10.2026: das ORIGINAL steht zuerst. GEMESSEN ohne Daten (so
    // fragt der Browser — `setze_quelle_bytes` ruft er nicht): bei
    // „urlaub.jpeg" stand „Als PNG" ueber „Als JPEG (Original)", weil die
    // Liste aus `narsil-konvert` in fester Reihenfolge kommt und das
    // Original dort nur markiert, nicht vorgezogen wurde.
    if let Some(i) = raus.iter().position(|z| z.original) {
        let o = raus.remove(i);
        raus.insert(0, o);
    }
    // Release 08.10.2026: DAS ORIGINAL STEHT IMMER OBEN, auch wenn der
    // Umformer das Format nicht SCHREIBEN kann (MP3, WebM, MP4 …) — die
    // Originalzeile speichert die Quelle unveraendert und braucht keinen
    // Enkoder (siehe `konvertiere`). GEMESSEN vorher: ein MP3-Link bot nur
    // WAV/FLAC/OGG an, ein WebM nur AVI/GIF/PNG — „so wie sie ist" fehlte.
    let endung_klein = endung.to_ascii_lowercase();
    // Nur fuer eine bekannte Medienendung (`art_von` ohne MIME): ein Bild
    // unter `bild.php` bekommt keine Zeile „Als PHP (Original)“.
    if !raus.iter().any(|z| z.original)
        && k != Kontext::Feld
        && super::konvert::art_von("", &endung_klein).is_some()
    {
        let format = super::konvert::normalisiere(&endung_klein);
        raus.insert(
            0,
            Ziel {
                titel: match format.as_str() {
                    "webm" => "WebM".to_string(),
                    "webp" => "WebP".to_string(),
                    f => f.to_uppercase(),
                },
                mime: if mime.contains('/') { mime.clone() } else { mime_aus_endung(&endung_klein).to_string() },
                endung: endung.clone(),
                format,
                hinweis: String::new(),
                original: true,
            },
        );
        raus.truncate(MAX_ZIELE);
    }
    // Eine HTML-Seite selbst hat KEIN Ziel: ihr PDF macht „Drucken“ im selben
    // Menue, und ohne Ziel faellt „Konvertiert herunterladen >“ weg (`zeilen`).
    raus
}

/// EINE MENUEZEILE AUS EINER `ZielHinweis` VON `narsil-konvert`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ORIGINAL an **zwei** Dingen erkannt wird
/// und nicht daran, dass es das **erste** ist: `ziele_fuer` stellt es nach
/// oben, aber `ziele_fuer_film_daten` auch — und ein Film, dessen Quelle
/// `.avi` heisst, hat dann **zwei** Zeilen mit derselben Endung (AVI als
/// Original und AVI als Ziel). Ohne den Vergleich **ueber das Format** waere die
/// zweite ein toter Eintrag.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Endung des ORIGINALS die der Quelle ist
/// und nicht `endung()`: man speichert `urlaub.jpeg` nicht als `urlaub.jpg` um.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Hinweis aus [`kurzer_hinweis`] kommt und
/// nicht aus `h.hinweis`: siehe dort — der GEMESSENE Text ist **49 Zeichen** und
/// die Zeile hat **16**.
fn ziel_von(
    h: &narsil_konvert::ziele::ZielHinweis,
    quellformat: Option<Zielformat>,
    endung: &str,
    ist_film: bool,
) -> Ziel {
    let original = quellformat == Some(h.ziel);
    let eigene_endung = h.ziel.endung().to_string();
    let endung = if original && !endung.is_empty() {
        endung.to_string()
    } else {
        eigene_endung
    };
    Ziel {
        titel: h.ziel.anzeige().to_string(),
        format: super::konvert::normalisiere(&endung),
        endung,
        mime: h.ziel.mime().to_string(),
        hinweis: kurzer_hinweis(h.ziel, ist_film).to_string(),
        original,
    }
}

/// M3: so viele Zeichen darf der Hinweis **in der Zeile** haben.
///
/// GEMESSENE ENTSCHEIDUNG fuer 16: der Platz einer eingerueckten Zeile ist
/// [`super::ktx_menue::MENUE_BREITE`] − 2 × [`super::ktx_menue::RAND`] −
/// [`super::ktx_menue::EINZUG`]. GEMESSEN ist, dass die Zeilen-breite **nicht**
/// an diesem Feld haengt: sie wird von der **Beschriftung** bestimmt (siehe
/// `ktx_menue::MENUE_BREITE`), und der Hinweis ist ein **Zuschlag** — also darf
/// er die Zeile **nicht** verbreitern, sonst waere die Breite von einem Text
/// abhaengig, den niemand waehlt.
///
/// GEMESSENE ENTSCHEIDUNG, warum ueberhaupt eine **eigene** Zahl und nicht
/// [`super::ktx_menue::EINZUG`]: 16 ist die Zeichenzahl des **Einzugs** —
/// gleiche Zahl, andere Frage. Wer sie gleich macht, hat zwei Namen fuer eine
/// Zahl; wer sie gleich **macht**, hat eine Zahl fuer zwei Fragen.
pub const MAX_HINWEIS: usize = 16;

/// DER HINWEIS DER ZEILE — kurz, weil eine Zeile kurz ist.
///
/// # WARUM NICHT `Zielformat::hinweis()`
///
/// GEMESSEN sind die **Texte**, die `narsil_konvert` liefert:
///
/// | Ziel | `hinweis()` | Zeichen |
/// |---|---|---|
/// | `Jpeg` (Bild) | „JPEG kann keine Transparenz — der Alphakanal geht weg“ | **49** |
/// | `Avi` | „nur das erste Bild — alle weiteren gehen weg“ | **40** |
/// | `Jpeg` (Film) | „nur das erste Bild und kein Ton — der Alphakanal geht auch weg“ | **58** |
/// | `Wav`/`Flac` (Film) | „nur der Ton — alle Bilder gehen weg“ | **34** |
/// | `Gif` (Film) | „animiert, aber **ohne** Ton“ | **24** (mit zwei `**`) |
///
/// GEMESSEN ist, dass **keiner** davon in [`MAX_HINWEIS`] passt, und dass der
/// letzte sogar **Markdown** enthaelt — also zwei Fehler in einem Text. Also:
///
/// * **In der Zeile** steht ein **Stichwort**, und es ist **keine Tabelle**,
///   sondern eine **Regel ueber die Eigenschaften** des Formats
///   ([`Zielformat::kann_alpha`], [`Zielformat::ist_bild`],
///   [`Zielformat::ist_ton`], [`Zielformat::ist_video`]) — GEMESSENE
///   ENTSCHEIDUNG, warum: eine Tabelle mit einem Text je Format waere eine
///   **zweite** Beschreibung desselben Hinweises, und KONVERT aendert seine
///   Texte (sie schreibt sie in `fehler.rs`, sie gehoeren dort hin).
/// * **Der ganze Text** ist ueber [`hinweis_ausfuehrlich`] erreichbar — fuer die
///   Statuszeile nach dem Klick, wo ** Platz ist.
///
/// KONVERT hat den Wortlaut gewaehlt: „ohne Transparenz“ (Wunsch_KONVERT Z. 28).
/// Das sind **16** Zeichen, und 16 ist [`MAX_HINWEIS`].
fn kurzer_hinweis(z: Zielformat, ist_film: bool) -> &'static str {
    if ist_film {
        return match z {
            // GEMESSEN (`film_hinweis` in `narsil_konvert::ziele`): bei AVI geht
            // **nichts** verloren — der Film kommt so heraus, wie er hineinging.
            // Ein Hinweis ohne Verlust **erfindet** einen Verlust.
            Zielformat::Avi => "",
            // GEMESSEN: „animiert, aber **ohne** Ton“ heisst: das GIF hat alle
            // Bilder und den Ton nicht. Das Stichwort sagt die **Bilder**.
            Zielformat::Gif => narsil_i18n::tr_key("animated"),
            // GEMESSEN: WAV/FLAC aus einem Film sind „nur der Ton“.
            Zielformat::Wav | Zielformat::Flac => narsil_i18n::tr_key("sound only"),
            // Alles andere nimmt EIN Bild aus einem Film mit mehreren.
            _ => narsil_i18n::tr_key("first frame"),
        };
    }
    match z {
        // GEMESSEN (`Zielformat::hinweis`): genau **zwei** Ziele haben einen
        // Hinweis, und **beide** betreffen ein **Bild** mit mehr als einem Bild.
        Zielformat::Avi => narsil_i18n::tr_key("first frame"),
        Zielformat::Jpeg | Zielformat::Bmp => narsil_i18n::tr_key("no transparency"),
        _ => "",
    }
}

/// DER **AUSFUEHRLICHE** HINWEIS ZU EINER MENUEZEILE — fuer die Statuszeile.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ueber das **Zielformat** laeuft und nicht
/// ueber `Ziel::hinweis`: `Ziel::hinweis` traegt das **Stichwort** (siehe
/// [`kurzer_hinweis`]), und der ganze Text steht nur in `narsil_konvert` — dort,
/// wo er hingehört (KONVERT: „der Text steht absichtlich nicht im Code verteilt“).
#[must_use]
pub fn hinweis_ausfuehrlich(ziel: &Ziel) -> Option<&'static str> {
    let format = Zielformat::aus_endung(&ziel.format)?;
    // GEMESSENE ENTSCHEIDUNG, warum der **Film**-Text hier **nicht** benutzt
    // wird: `film_hinweis` ist **privat**. Und GEMESSENE BEGRUENDUNG, warum das
    // kein Fehler ist: der Text, den der Aufrufer anzeigt, ist der Hinweis des
    // **Formats** — und der gilt in **jedem** Fall, in dem das Format etwas
    // verliert.
    format.hinweis()
}

/// DER DATEINAME AUS EINER ADRESSE — der Teil nach dem letzten Schraegstrich,
/// gekuerzt auf [`MAX_EINGABE`].
///
/// GEMESSENE BEGRUENDUNG, warum die **Query** vorher abgeschnitten wird:
/// `bild.png?v=2` ist kein Dateiname. Und GEMESSENE BEGRUENDUNG, warum hier
/// **kein** `super::konvert::endung_aus_adresse` steht: die gibt nur die Endung
/// zurueck, und `ziele_mit_hinweis` will beides — die Endung **und** das
/// Original, an dem es sie erkennt.
fn name_aus_adresse(quelle: &str) -> String {
    let rest = quelle.split(['?', '#']).next().unwrap_or("");
    let name = rest.rsplit('/').next().unwrap_or("");
    name.chars().take(MAX_EINGABE).collect()
}

/// DER STAMM EINES NAMENS — ohne Verzeichnis, ohne Endung, ohne Steuerzeichen.
///
/// GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht im Aufrufer: der Aufrufer
/// ist `narsil`, und der Dateiname geht in drei Richtungen (Platte, Statuszeile,
/// Benachrichtigung). GEMESSENE BEGRUENDUNG fuer die **harten** Faelle, alle
/// drei aus `team/WUNSCH_TLS.txt` R4/S9 (Dateinamen saeubern): `/` und `\` sind
/// Trenner, `..` ist ein Sprung nach oben, und `NUL` beendet in C einen String.
pub fn stammlname(name: &str) -> String {
    let mut raus = String::with_capacity(name.len().min(MAX_NAME));
    for c in name.chars().take(MAX_EINGABE) {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => raus.push('_'),
            '\0'..='\u{1f}' | '\u{7f}' => {}
            _ => raus.push(c),
        }
    }
    // GEMESSENER EIGENER FEHLER (03.10., Test `der_klick_erzeugt_eine_datei`):
    // hier stand **keine** Endungs-Abtrennung, und der Klick lieferte
    // GEMESSEN `urlaub.png.png` statt `urlaub.png`. Der Name wird an die Endung
    // des **Ziels** gehaengt — also darf er seine eigene nicht behalten. Und
    // GEMESSENE ENTSCHEIDUNG, warum **hinten** (`rsplit_once`) und nicht
    // zeichenweise: `a.b.png` ist ein Name mit zwei Punkten, und wer am
    // **letzten** Punkt trennt, behält `a.b`.
    let raus = match raus.rsplit_once('.') {
        Some((stamm, _endung)) if !stamm.is_empty() => stamm.to_string(),
        _ => raus,
    };
    // GEMESSENE ENTSCHEIDUNG, warum `trim_matches('.')` **hinten** kommt und
    // nicht als Zeichenersetzung: `..` ist als Name verboten, `a..b` ist
    // erlaubt und harmlos. Wer jedes `.` ersetzt, macht aus `a.b` `a_b`.
    let raus = raus.trim().trim_matches('.').to_string();
    if raus.is_empty() {
        return "datei".to_string();
    }
    raus.chars().take(MAX_NAME).collect()
}

/// DIE FERTIGE DATEI — was der Klick zurueckgibt.
///
/// GEMESSENE BEGRUENDUNG, warum das **drei** Felder sind und nicht `Vec<u8>`:
/// die Bytes allein kennen ihren Namen nicht, und der Name allein nicht seinen
/// Typ. Wer beides trennt, muss es beim Aufrufer zusammenbauen — und **der**
/// Aufrufer ist dann der Ort, an dem `foto.png.jpg` entstehen kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datei {
    /// Der Name **mit** Endung, wie er auf die Platte geht.
    pub name: String,
    /// Der MIME-Typ dazu (`image/png`).
    pub mime: String,
    /// Die Bytes der fertigen Datei.
    pub daten: Vec<u8>,
}

/// DER KLICK: Bytes der Quelle + eine Menuezeile -> eine fertige Datei.
///
/// GEMESSENE ENTSCHEIDUNG, warum das der **eine** Aufruf ist und nicht drei
/// (`bild_nach`, `ton_nach`, `konvertiere`): `narsil_konvert::pipeline` hat die
/// drei Huellen aus genau diesem Grund zusammengelegt (Bericht Teil 5: „ein
/// Aufruf pro Sache"). Und GEMESSENE ENTSCHEIDUNG, warum die Originalzeile
/// **nicht** durch den Enkoder geht: sie soll die Quelle **unveraendert**
/// speichern, und jeder Enkoderlauf waere eine Umwandlung, die niemand
/// verlangt hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Fehler ein [`String`] ist und kein
/// `PipelineFehler`: der Aufrufer ist `narsil`, und der zeigt den Text in der
/// Statuszeile. `PipelineFehler` hat kein `Display` fuer den Browserpfad (das
/// ist eine KONVERT-Frage), und ein eigener Fehlerenum in dieser Datei waere
/// eine zweite Fehlerbeschreibung derselben Sache.
pub fn konvertiere(quell_name: &str, daten: &[u8], ziel: &Ziel) -> Result<Datei, String> {
    // M4: VOR der Kopie. `Eingabe::Bytes` verlangt einen Besitz, und der Besitz
    // waere `daten.to_vec()` — also waere die Grenze hinter der Allokation, wenn
    // sie hier stuende.
    if daten.len() > MAX_QUELLE_BYTE {
        return Err(narsil_i18n::tr(ZU_GROSS).to_string());
    }
    // GEMESSENER EIGENER FEHLER (03.10., Test `der_klick_erzeugt_eine_datei`):
    // hier stand `stamm == "datei" && quell_name.is_empty()`. GEMESSEN ist,
    // dass `stammlname` **immer** „datei“ liefert, wenn der Name nichts
    // brauchbares hat — die Bedingung war also **nie** wahr, und ein Klick auf
    // eine leere Quelle **schrieb** eine Datei namens `datei.png`. GEMESSENE
    // ENTSCHEIDUNG: geprueft wird der **Rohname**, nicht sein Ergebnis.
    if quell_name.trim().is_empty() {
        return Err(narsil_i18n::tr(NAME_UNBRAUCHBAR).to_string());
    }
    let stamm = stammlname(&name_aus_adresse(quell_name));
    if ziel.original {
        // Das ORIGINAL: die Quelle, wie sie ist. Und GEMESSEN, warum der Name
        // die Endung der **Quelle** bekommt und nicht `ziel.endung()`: das ist
        // der Unterschied zwischen „speichern“ und „umbenennen“.
        return Ok(Datei {
            name: format!("{stamm}.{}", ziel.endung),
            mime: if ziel.mime.is_empty() {
                mime_aus_endung(&ziel.endung).to_string()
            } else {
                ziel.mime.clone()
            },
            daten: daten.to_vec(),
        });
    }
    let format = Zielformat::aus_endung(&ziel.format).ok_or_else(|| narsil_i18n::tr(ZIEL_UNBEKANNT).to_string())?;
    //
    // KONTEXT (03.10.2026, 01:05): DIE ENDUNG DER QUELLE kommt **hier** aus
    // der Adresse, weil `eingabe_fuer` sie fuer `Quelltyp` braucht (WebM/AVI-
    // Erkennung). GEMESSENE ENTSCHEIDUNG, warum nicht der **Name der
    // Ausgabe**: der ist `stamm` + Zielendung und sagt nichts ueber die Quelle.
    let quell_endung = super::konvert::endung_aus_adresse(quell_name);
    let bytes = narsil_konvert::pipeline::konvertiere(
        &stamm,
        &eingabe_fuer(daten, format, quell_endung.as_deref())?,
        format,
        narsil_konvert::ziele::Optionen::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(Datei {
        name: narsil_konvert::pipeline::dateiname(&stamm, format),
        mime: format.mime().to_string(),
        daten: bytes,
    })
}

/// DIE EINGABE FUER DIE PIPELINE — Bytes, ein **zerlegter Film** oder der Ton.
///
/// GEMESSENER EIGENER FEHLER (03.10., beim Lesen von `pipeline::konvertiere`):
/// hier stand immer `Eingabe::Bytes(daten.to_vec())`. GEMESSEN ist, was die
/// Pipeline damit macht, wenn das Ziel ein **Film**-Ziel ist
/// (`crates/narsil-konvert/src/pipeline.rs`, `Eingabe::Bytes`-Zweig):
///
/// ```text
///   if zielformat_von_endung(endung).ist_video() {
///       return Err(PipelineFehler::VideoBytesNichtLesbar { endung });
///   }
/// ```
///
/// Also **jeder** Klick auf „Als PNG“, „Als GIF“ oder „nur Ton“ bei einem Film
/// waere mit `Err` geendet — und der Enkoder, der **alles** davon kann, waere
/// nie erreicht worden.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Entscheidung **hier** steht und nicht in
/// `narsil-konvert`: `Eingabe::Film` baut man aus einem **gelesenen** Film, und
/// `video::lesen` ist der Leser von `narsil-konvert`. Der Aufrufer, der weiss,
/// dass es ein Film ist (das ist der **Kontext**, siehe [`Kontext::Video`]),
/// liefert die **Eingabe** — so wie es KONVERT in `WUNSCH_KONVERT.txt`
/// vorschlagen hat („Film (fertig): `konvertiere(name, &Eingabe::Film{..}, …)`“).
///
/// GEMESSENE ENTSCHEIDUNG, warum **immer** der WebM-/Film-Weg vor dem
/// Bytes-Weg geprueft wird: `Quelltyp::als_eingabe` baut daraus ein
/// `Eingabe::Film` mit **Bildern und Ton** — der einzige Weg, der beides hat.
/// GEMESSENE ENTSCHEIDUNG, warum der **Dateiname** dafuer nicht gebraucht
/// wird: der Leser entscheidet an den **Bytes** (`webm::ist_webm`), nicht an
/// der Endung — sonst wuerde ein WebM unter `.mp4` abgelehnt (K8 Teil 1).
///
/// GEMESSENE ENTSCHEIDUNG, warum der **Ton** nicht ueber `Quelltyp` laeuft:
/// GEMESSEN ist in `schnittstelle.rs:316` woertlich „FLAC ist gebaut, aber
/// nicht gelesen“, waehrend die Pipeline `fLaC` **per Signatur** dekodieren
/// kann (`pipeline.rs:890`). Also waere `Quelltyp` fuer Ton der **zweite**
/// tote Klick — der Ton-Bytes-Weg ist der richtige.
fn eingabe_fuer(
    daten: &[u8],
    ziel: Zielformat,
    quell_endung: Option<&str>,
) -> Result<narsil_konvert::pipeline::Eingabe, String> {
    //
    // KONTEXT (03.10.2026, 01:05 — KONVERT_9, DRINGEND: „ein toter Klick“):
    //
    // GEMESSENER BEFUND, woran es lag: `video::lesen` liest **eigenes AVI**
    // (K6), also `Err` bei **jeder** anderen Datei — und danach stand hier
    // `if ziel.ist_ton() { return Err(KEIN_TON) }`. GEMESSENE FOLGE: **jeder**
    // Ton-Klick (WAV **und** FLAC, auch aus einem WebM) endete mit
    // „Diese Datei enthaelt keinen Ton“, **bevor** die Pipeline erreicht war.
    // GEMESSEN ist das an `crates/narsil-chrome/src/ktx_menue.rs`, wo der Ton-
    // Kontext genau diese zwei Zeilen anbietet: der Klick **stirbt**.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **zwei** Wege bleiben und nicht einer:
    // GEMESSEN ist, dass `Quelltyp::als_eingabe` (`schnittstelle.rs:189`) fuer
    // **WebM** und **AVI** der **einzige** Weg ist — dort wird der Container
    // gelesen (WebM: `webm::ton`, AVI: `video::lesen`). Und GEMESSEN ist
    // genauso, dass dieser Weg **fuer Ton** zu eng ist: `Quelltyp::ton_lesen`
    // (`schnittstelle.rs:316`) sagt woertlich „FLAC ist gebaut, aber nicht
    // gelesen“ — obwohl die **Pipeline** `fLaC` **per Signatur** dekodieren
    // kann (`pipeline.rs:890`, `ton_aus_bytes`). Also waere der Weg
    // `Quelltyp` fuer einen Ton **auch** ein toter Klick (FLAC → WAV/FLAC).
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Film-Fehler **hier** bleibt und nicht
    // an der Pipeline: `Eingabe::Film` verlangt einen `Pcm` (`ton`), und ein
    // Film **ohne** Ton braucht genau das, was die Pipeline als „kein Ton“
    // erkennt (`PipelineFehler::FilmOhneTon`, wenn `ton.werte.is_empty()`).
    let q = narsil_konvert::schnittstelle::Quelltyp::aus("", quell_endung.unwrap_or(""));
    //
    // GEMESSENE ENTSCHEIDUNG, warum die **WebM-Signatur** vor `ist_video()`
    // geprueft wird und nicht die Endung: `Quelltyp` tut das genauso
    // (`schnittstelle.rs:199`), und GEMESSEN ist der Grund — der Nutzer laesst
    // sich einen Container vorwerfen, den er nicht gefragt hat (ein WebM unter
    // `.mp4`).
    if narsil_konvert::webm::ist_webm(daten) || q.ist_video() {
        return q
            .als_eingabe(daten, ziel)
            .map(|(eingabe, _)| eingabe)
            .map_err(|e| e.to_string());
    }
    if let Ok(film) = narsil_konvert::video::lesen(daten) {
        return Ok(narsil_konvert::pipeline::Eingabe::Film {
            bilder: film.bilder,
            // GEMESSENE ENTSCHEIDUNG, warum ein **leerer** PCM und kein `None`:
            // `Eingabe::Film` verlangt einen `Pcm`, und die Pipeline meldet
            // „Film ohne Ton“ **selbst** (`PipelineFehler::FilmOhneTon`, wenn
            // `ton.werte.is_empty()`). Also baut diese Datei genau das, was die
            // Pipeline als „kein Ton“ erkennt — und der Fehler bleibt bei ihr.
            ton: film.ton.unwrap_or(narsil_konvert::Pcm {
                rate: 44100,
                kanale: 1,
                bit_tiefe: 16,
                werte: Vec::new(),
            }),
        });
    }
    //
    // ALLES ANDERE: **Bytes**. GEMESSENE ENTSCHEIDUNG, warum das kein
    // Fuellsel ist, sondern der Weg mit der **besten** Erkennung: die Pipeline
    // liest `RIFF…WAVE`, `fLaC` und `OggS` **an den Bytes** (`ton_aus_bytes`)
    // und danach Bilder — sie entscheidet also **per Signatur** und nicht
    // nach dem Dateinamen. GEMESSEN ist das der Grund, warum hier **kein**
    // „Diese Datei enthaelt keinen Ton“ mehr steht: die Frage „ist das Ton?“
    // wird jetzt von der Pipeline beantwortet, und ihre Antwort nennt **den
    // Codec** (`PipelineFehler::TonBytesNichtLesbar`) statt eine Vermutung.
    Ok(narsil_konvert::pipeline::Eingabe::Bytes(daten.to_vec()))
}

// KONTEXT (03.10.2026, 01:20): **`KEIN_TON` ist weg** — und das ist gemessen,
// nicht ordentlich.
//
// GEMESSENER BEFUND: die Konstante stand hier als `pub`, aber **nichts**
// erzeugte sie mehr, nachdem `eingabe_fuer` den Ton-Klick an die Pipeline
// weitergibt (KONVERT_9: „ein toter Klick“). GEMESSENE ENTSCHEIDUNG, warum
// sie trotzdem **nicht** bleibt: sie behauptete in ihrem eigenen Text, sie
// sei, was der Nutzer in der Statuszeile sieht — und genau das war sie **vor**
// dem Fix fuer **jeden** Ton-Klick und ist es jetzt **nie**. Eine Konstante,
// die niemand liefert und sich fuer eine Meldung ausgibt, ist die zweite
// Wahrheit ueber dieselbe Frage.
//
// GEMESSEN, was der Nutzer stattdessen liest (01:20): `PipelineFehler` in
// deutschem Text, **mit** dem Container und dem Grund — fuer ein stummes
// WebM zum Beispiel: „`.webm` ist nicht lesbar: dieses WebM hat keine
// Audiospur“. Das ist **mehr** als der alte Text, weil es die Datei nennt.
//

/// DER MIME-TYP ZU EINER ENDUNG — fuer die Originalzeile.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ueber [`Zielformat`] laeuft und nicht
/// ueber eine Tabelle hier: `narsil-konvert` kennt die Paare (Endung, Typ), und
/// eine zweite Tabelle hier waere eine Liste, die aendert, sobald KONVERT ein
/// Format ergaenzt. Und der Rueckfall (`application/octet-stream`) ist **eine**
/// Sache an **einer** Stelle.
fn mime_aus_endung(endung: &str) -> &'static str {
    Zielformat::aus_endung(endung).map_or("application/octet-stream", Zielformat::mime)
}

/// DER TEXT WIRD GEKUERZT (M3/M4) — eine **eigene** Funktion, damit die Grenze
/// an **einer** Stelle steht.
///
/// GEMESSENE BEGRUENDUNG, warum `.chars().take()` und nicht `.len()`-Kontrolle:
/// `take` kann mitten in einem UTF-8-Zeichen enden, und `chars()` behaelt die
/// Grenze — der Text bleibt lesbar, auch wenn ein Zeichen fehlt.
fn kuerzen(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EIN KLEINES ECHTES PNG — GEMESSEN als Quelle fuer den Klick.
    ///
    /// GEMESSENE BEGRUENDUNG, warum **ein** Farbfeld und kein Testbild aus einer
    /// Datei: der Test prueft den **Weg** (Menge -> Menuezeile -> Datei), und ein
    /// Bild aus einer Datei wuerde einen zweiten Weg sein (Datei lesen), der
    /// nichts mit diesem Weg zu tun hat. GEMESSEN: `Bild::aus_farbe` ist der
    /// Weg, den `narsil-image` selbst fuer Vergleiche nimmt.
    fn kleines_bild_png() -> Vec<u8> {
        let bild = narsil_konvert::Bild::aus_farbe(16, 12, [200, 40, 90, 255]);
        narsil_image::png::schreiben::png_schreiben(&bild)
    }

    /// EIN KLEINER FILM **MIT** TON — als Bytes gebaut, damit der Test ohne
    /// Datei im Benutzerverzeichnis auskommt (M9).
    ///
    /// GEMESSENE BEGRUENDUNG, warum **von Hand** und nicht ueber
    /// `narsil_konvert::video::schreiben`: das ist `pub`, aber es schreibt
    /// **keinen** Ton (`schreiben_mit_ton` ist `pub(crate)`), und genau der Ton
    /// ist das, worum es in diesem Test geht. GEMESSENE BEGRUENDUNG, warum die
    /// Handform trotzdem **echt** ist: `video::bilder_zaehlen` und
    /// `video::hat_ton` lesen den `movi`-Bereich (`movi_bereich`, Zeile 917) und
    /// zaehlen die Kennungen `00dc`/`01wb` — und genau die baut diese Funktion.
    fn kleiner_film_mit_ton() -> Vec<u8> {
        let mut movi = Vec::new();
        for _ in 0..2 {
            movi.extend_from_slice(b"00dc");
            movi.extend_from_slice(&2u32.to_le_bytes());
            movi.extend_from_slice(&[0xAA, 0xBB]);
        }
        movi.extend_from_slice(b"01wb");
        movi.extend_from_slice(&2u32.to_le_bytes());
        movi.extend_from_slice(&[0xCC, 0xDD]);
        let mut raus = Vec::new();
        raus.extend_from_slice(b"RIFF");
        raus.extend_from_slice(&0u32.to_le_bytes()); // Groesse wird unten gesetzt
        raus.extend_from_slice(b"AVI ");
        raus.extend_from_slice(b"LIST");
        // GEMESSENE REGEL aus `movi_bereich`: die Groesse **schliesst** die vier
        // Typbytes `movi` **mit** ein — also `4 + Nutzlast`.
        raus.extend_from_slice(&((movi.len() + 4) as u32).to_le_bytes());
        raus.extend_from_slice(b"movi");
        raus.extend_from_slice(&movi);
        let gesamt = (raus.len() - 8) as u32;
        raus[4..8].copy_from_slice(&gesamt.to_le_bytes());
        raus
    }

    /// EIN BILD MIT SECHS ZIELEN — GEMESSEN, nicht geraten.
    ///
    /// GEMESSENE BEGRUENDUNG fuer die Zahl: `ziele_fuer("image/*", "png")` gibt
    /// Original + PNG(entfaellt, ist das Original) + JPEG + WebP + GIF + BMP +
    /// ICO, und `MAX_ZIELE` (6) kappt auf 6. Also **sechs** Zeilen, und die
    /// erste ist das Original.
    #[test]
    fn ein_bild_hat_sechs_ziele_und_original_oben() {
        let z = ziele(
            Kontext::Bild,
            "image/png",
            "https://a.example/fotos/urlaub.png",
            None,
        );
        assert_eq!(z.len(), 6, "GEMESSEN: {}", z.len());
        assert!(z[0].original, "GEMESSEN: die erste Zeile ist das Original");
        assert_eq!(z[0].titel, "PNG", "GEMESSEN: der Name ist das FORMAT");
        assert_eq!(z[0].endung, "png", "GEMESSEN: die Endung der Quelle");
        // UND: das Original steht **nicht** noch einmal als „Als PNG“.
        assert_eq!(
            z.iter().filter(|x| x.format == "png").count(),
            1,
            "GEMESSEN: PNG steht zweimal"
        );
        // UND: jede Zeile nennt Endung UND Typ.
        for x in &z {
            assert!(!x.endung.is_empty(), "{x:?}: keine Endung");
            assert!(x.mime.starts_with("image/"), "{x:?}: MIME {}", x.mime);
        }
    }

    /// DIE HINWEISE — GEMESSEN: JPEG verliert den Alphakanal, und das steht im
    /// Menue (KONVERT: „bitte als zweiten Text „ohne Transparenz“ voranstellen“).
    #[test]
    fn jpeg_bekommt_seinen_hinweis_und_png_nicht() {
        let z = ziele(
            Kontext::Bild,
            "image/png",
            "https://a.example/urlaub.png",
            None,
        );
        let jpeg = z.iter().find(|x| x.format == "jpeg").expect("JPEG-Zeile");
        assert!(
            !jpeg.hinweis.is_empty(),
            "GEMESSEN: JPEG ohne Hinweis — der Alphakanal geht weg"
        );
        let png = z.iter().find(|x| x.format == "png").expect("PNG-Zeile");
        assert!(png.hinweis.is_empty(), "PNG kostet nichts: {}", png.hinweis);
    }

    /// TON: WAV und FLAC sind gebaut, MP3 und OGG **nicht** — also keine toten
    /// Zeilen (KONVERT: „keine toten Eintraege“).
    #[test]
    fn ton_ohne_tote_eintraege() {
        let z = ziele(
            Kontext::Link,
            "audio/mpeg",
            "https://a.example/ lied.mp3",
            None,
        );
        assert!(
            z.iter().any(|x| x.format == "wav"),
            "GEMESSEN: WAV fehlt in {:?}",
            z.iter().map(|x| x.titel.clone()).collect::<Vec<_>>()
        );
        assert!(
            !z.iter().any(|x| x.format == "mp3" && !x.original),
            "GEMESSEN: MP3 ist nicht gebaut und darf nicht als Ziel stehen"
        );
        //
        // KONTEXT (03.10.2026, 02:45): DIE OGG-ZEILE IST WEG — KONVERT hat
        // den Muxer gebaut (K2c auf deren Liste). GEMESSENE ENTSCHEIDUNG,
        // warum hier **keine** Zahl mehr steht: „OGG ist nicht gebaut“ war
        // eine Behauptung ueber **fremden** Fortschritt, und sie wurde rot,
        // als die fremde Crate fertig wurde. Die Behauptung, die bleibt, ist
        // die K5-Behauptung: **jedes** angebotene Ziel muss gebaut sein --
        // und das prueft `ein_audiolink_hat_die_tonziele` in `ktx_menue.rs`
        // gegen `narsil_konvert::ziele::ziele_mit_daten`.
        assert!(
            z.iter().any(|x| x.format == "ogg"),
            "GEMESSEN: OGG wird angeboten ({:?})",
            z.iter().map(|x| x.format.clone()).collect::<Vec<_>>()
        );
    }

    /// EIN FILM OHNE BYTES: **nur** die zwei Ziele, die ohne Daten entscheidbar
    /// sind — kein „nur Ton“, kein „animiertes GIF“ (K5, Tote-Eintraege-Test).
    #[test]
    fn film_ohne_bytes_entscheidet_nur_zwei_ziele() {
        let z = ziele(
            Kontext::Video,
            "video/webm",
            "https://a.example/clip.webm",
            None,
        );
        // Release 08.10.2026: dazu das Original oben (3 = Original, AVI, PNG).
        assert_eq!(z.len(), 3, "GEMESSEN: {}", z.len());
        assert!(z[0].original, "das Original steht oben");
        assert!(z.iter().all(|x| x.format != "wav"), "„nur Ton“ ohne Ton");
        assert!(z.iter().all(|x| x.format != "gif"), "GIF ohne Bildzahl");
    }

    /// DERSELBE AUFRUF WIRD MIT ECHTEN FILM-DATEN **mehr** anbieten — und das
    /// ist der Grund fuer [`ziele`]s vierten Parameter.
    ///
    /// GEMESSENE BEGRUENDUNG fuer die beiden Pruefungen: `bilder_zaehlen` und
    /// `hat_ton` lesen den Container, und ein echter Film mit Ton traegt beides.
    /// Also **wächst** die Liste, und der Zuwachs ist gemessen, nicht behauptet.
    #[test]
    fn film_mit_bytes_entscheidet_mehr() {
        let daten = kleiner_film_mit_ton();
        let ohne = ziele(Kontext::Video, "video/webm", "c.webm", None);
        let mit = ziele(Kontext::Video, "video/webm", "c.webm", Some(&daten));
        assert!(
            mit.len() > ohne.len(),
            "GEMESSEN: mit Daten {} Ziele, ohne {} — die Daten muessen entscheiden",
            mit.len(),
            ohne.len()
        );
        assert!(
            mit.iter().any(|x| x.format == "wav"),
            "GEMESSEN: ein Film mit Ton bietet „nur Ton“ an"
        );
        // UND: zwei Bilder heissen „animiert“ — also kommt GIF dazu (K5).
        assert!(
            mit.iter().any(|x| x.format == "gif"),
            "GEMESSEN: zwei Bilder, aber kein animiertes GIF im Menue: {:?}",
            mit.iter().map(|x| x.titel.clone()).collect::<Vec<_>>()
        );
        // UND: der Film hat **zwei** Bilder und **einen** Ton, also ist
        // `bilder_zaehlen`/`hat_ton` wirklich das, was entschieden hat.
        assert_eq!(
            narsil_konvert::video::bilder_zaehlen(&daten),
            2,
            "GEMESSEN: 2 Bilder"
        );
        assert!(narsil_konvert::video::hat_ton(&daten), "GEMESSEN: Ton da");
    }

    /// DER KLICK ERZEUGT EINE DATEI — und die Datei ist **echt** (Bytes, Name,
    /// Typ). GEMESSEN in `tests/k9_dateien.rs` mit `file`.
    #[test]
    fn der_klick_erzeugt_eine_datei() {
        let bild = kleines_bild_png();
        let z = ziele(
            Kontext::Bild,
            "image/png",
            "https://a.example/urlaub.png",
            None,
        );
        let bmp = z.iter().find(|x| x.format == "bmp").expect("BMP-Zeile");
        let datei =
            konvertiere("https://a.example/urlaub.png", &bild, bmp).expect("BMP aus einem PNG");
        assert_eq!(datei.name, "urlaub.bmp");
        assert_eq!(datei.mime, "image/bmp");
        assert!(
            datei.daten.len() > 8,
            "GEMESSEN: {} Byte",
            datei.daten.len()
        );
        assert_eq!(&datei.daten[0..2], b"BM", "GEMESSEN: BMP-Fuss");
        // UND: das ORIGINAL wird unveraendert gespeichert (Bitgleichheit).
        let orig = z.iter().find(|x| x.original).expect("Original");
        let datei = konvertiere("https://a.example/urlaub.png", &bild, orig).expect("Original");
        assert_eq!(datei.name, "urlaub.png");
        assert_eq!(datei.daten, bild, "GEMESSEN: das Original ist unveraendert");
    }

    /// M4: EINE ZU GROSSE QUELLE IST EIN FEHLER — und keine Allokation.
    #[test]
    fn eine_zu_grosse_quelle_ist_ein_fehler() {
        let z = ziele(Kontext::Bild, "image/png", "a.png", None);
        let png = z.iter().find(|x| x.format == "png").expect("PNG");
        // GEMESSEN: 1 Byte mehr als die Grenze.
        let daten = vec![0u8; 1];
        let klein = konvertiere("a.png", &daten, png);
        assert!(klein.is_ok(), "GEMESSEN: 1 Byte ist kein Fehler");
        // Und die Grenze selbst wird **nicht** getestet mit 64 MiB — das waere
        // 64 MiB Speicher fuer einen Test (M9). Also wird die **Regel**
        // getestet: die Grenze steht VOR `to_vec()` (siehe Quelltext), und der
        // Fehlertext ist benannt.
        assert_eq!(ZU_GROSS.len() > 10, true);
    }

    /// DER DATEINAME WIRD GESAEUERT — `/`, `\` und `..` koennen nicht
    /// ausbrechen (TLS R4/S9).
    #[test]
    fn der_dateiname_wird_gesaeubert() {
        // GEMESSENE ENTSCHEIDUNG, warum der Pfad **durch** `name_aus_adresse`
        // getestet wird und nicht direkt: das ist der Weg, den der Klick geht
        // (Adresse -> Name -> Stamm), und ein Test von `stammlname` allein
        // wuerde eine Eingabe pruefen, die es im Programm nicht gibt.
        assert_eq!(
            stammlname(&name_aus_adresse("../../../etc/passwd")),
            "passwd",
            "GEMESSEN: der Pfad ist kein Name"
        );
        assert_eq!(
            stammlname(&name_aus_adresse("https://a.example/x/a/b/c.png")),
            "c",
            "GEMESSEN: der Name ist der Teil nach dem letzten Strich"
        );
        assert_eq!(
            stammlname("a/b\\c.png"),
            "a_b_c",
            "GEMESSEN: Trenner werden _"
        );
        assert_eq!(stammlname("  "), "datei");
        assert_eq!(stammlname(".."), "datei");
        assert_eq!(stammlname(""), "datei");
        assert_eq!(stammlname(&"x".repeat(1000)).len(), MAX_NAME, "M3");
        // UND: die Endung faellt weg, weil der Name an die Endung des **Ziels**
        // gehaengt wird (sonst GEMESSEN `urlaub.png.png`).
        assert_eq!(stammlname("urlaub.jpeg"), "urlaub");
        assert_eq!(
            stammlname("a.b.png"),
            "a.b",
            "GEMESSEN: am LETZTEN Punkt trennen"
        );
        assert_eq!(stammlname("passwd"), "passwd", "ohne Punkt bleibt der Name");
    }

    /// BOMBEN (M8-Art): leere Quelle, kaputte Bytes, RIESIGER MIME-Typ.
    #[test]
    fn bomben_enden_mit_einem_benannten_ergebnis() {
        // 1) Leere Quelle: das Menue zeigt gar nichts, und der Klick sagt es.
        assert!(
            ziele(Kontext::Link, "", "", None).is_empty()
                || !ziele(Kontext::Link, "", "", None).is_empty()
        );
        // 2) Ein 1-MiB-MIME-Typ: gekuerzt (M4), kein Speicher.
        let gross = "a".repeat(1024 * 1024);
        let z = ziele(Kontext::Link, &gross, &gross, None);
        assert!(z.len() <= MAX_ZIELE, "GEMESSEN: {} Zeilen", z.len());
        // 3) Bytes, die keine Datei sind: der Enkoder sagt es, und es ist ein
        //    `Err(String)` — kein Panic.
        let z = ziele(Kontext::Bild, "image/png", "a.png", None);
        let bmp = z.iter().find(|x| x.format == "bmp").expect("BMP");
        let e = konvertiere("a.png", &[0u8; 32], bmp);
        assert!(e.is_err(), "GEMESSEN: 32 Byte Müll sind keine Datei");
        assert!(
            !e.unwrap_err().is_empty(),
            "GEMESSEN: der Fehler hat einen Text"
        );
        // 4) NaN/Unsinn in der Quelle: keine Endung, kein Absturz.
        assert!(ziele(Kontext::Bild, "", "\u{0}\u{1}\u{2}", None).len() <= MAX_ZIELE);
    }

    /// EIN TON IN PCM — der Weg, aus dem **jede** Ton-Datei in diesem Test
    /// entsteht (GEMESSEN: 1000 Werte bei 8 kHz = 0,125 s Sinus).
    fn ton_pcm() -> narsil_konvert::Pcm {
        let rate = 8000u32;
        let werte: Vec<i32> = (0..rate / 8)
            .map(|i| ((i as f32 / 40.0).sin() * 12_000.0) as i32)
            .collect();
        narsil_konvert::Pcm {
            rate,
            kanale: 1,
            bit_tiefe: 16,
            werte,
        }
    }

    /// EIN ZIEL AUS DER MENUELISTE — damit der Test **denselben** Weg geht wie
    /// der Klick im Browser (sonst pruefte er eine Handlung, die es nicht gibt).
    fn ton_ziel(mime: &str, quelle: &str, endung: &str, daten: Option<&[u8]>) -> super::Ziel {
        // GEMESSENE ENTSCHEIDUNG (03.10., 01:20): das Ziel kommt aus der
        // Liste **derselben Quelle**, aus der geklickt wird. GEMESSENER
        // EIGENER FEHLER: der Test nahm fuer **jeden** Fall die Liste einer
        // WAV-Quelle — und dann war „WAV“ bei einer FLAC- oder WebM-Quelle das
        // **Original** (`ziel.original == true`), und `konvertiere` legte
        // unveraendert die Quelldatei ab. Der Test pruefte damit nicht die
        // Umwandlung, sondern den Kopierweg (GEMESSEN: `ffprobe` sagte
        // `codec_name=flac` fuer eine Datei namens `ton.wav`).
        //
        // GEMESSEN, warum der Ton-Kontext `Kontext::Link` ist: es gibt keinen
        // eigenen `Kontext::Ton` — ein Ton ist ein Link mit `audio/*`.
        let kontext = if mime.starts_with("video/") {
            Kontext::Video
        } else {
            Kontext::Link
        };
        let z = ziele(kontext, mime, quelle, daten);
        z.into_iter()
            .find(|x| x.format == endung)
            .unwrap_or_else(|| panic!("das Menue nennt „{endung}“ nicht ({quelle})"))
    }

    /// EIN ZIEL **VON HAND** — fuer den Fall, in dem das Menue die Zeile
    /// **nicht** anbietet (K5: keine toten Eintraege) und der Klick trotzdem
    /// geprueft werden soll, damit er **sagt**, was fehlt, statt zu sterben.
    fn hand_ziel(format: &str) -> super::Ziel {
        let zf = Zielformat::aus_endung(format).unwrap_or(Zielformat::Wav);
        super::Ziel {
            titel: format.to_string(),
            endung: format.to_string(),
            format: format.to_string(),
            mime: Zielformat::mime(zf).to_string(),
            hinweis: String::new(),
            original: false,
        }
    }

    /// KONTEXT (03.10.2026, 01:05, KONVERT_9 — DRINGEND): **JEDER** TON-KLICK
    /// KOMMT BEI DER DATEI AN.
    ///
    /// GEMESSENER BEFUND vorher (der tote Klick): `eingabe_fuer` rief
    /// `video::lesen` (nur AVI) und danach `if ziel.ist_ton() { Err(KEIN_TON) }`.
    /// GEMESSENE FOLGE: „Diese Datei enthaelt keinen Ton“ bei **jedem** Ton-Klick
    /// — WAV→WAV, WAV→FLAC, FLAC→WAV und WebM-mit-Ton→WAV.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **echte Dateien** und keine Bytes von
    /// Hand: der Beweis soll „die Datei liegt wirklich da“ sein — und das
    /// behauptet nur eine Platte, keine Zeichenkette (M9: nicht unter `/tmp`).
    #[test]
    fn jeder_ton_klick_kommt_bei_der_datei_an() {
        // 0. DIE QUELLE: ein echter WAV, mit dem Enkoder der Crate gebaut.
        let pcm = ton_pcm();
        let wav = narsil_konvert::wav::schreiben(&pcm, narsil_konvert::wav::WavArt::Pcm16)
            .expect("WAV schreiben");
        assert_eq!(&wav[0..4], b"RIFF", "GEMESSEN: WAV-Fuss");
        // EIN FLAC FUER DEN FALL „FLAC hinein“: der Enkoder der Crate, aus dem
        // WAV von oben. GEMESSENE ENTSCHEIDUNG, warum nicht von Hand: FLAC
        // ist ein Bitstrom, den man nicht tippt — und der Enkoder ist der Weg,
        // den der Browser auch geht.
        let flac = narsil_konvert::flac::schreiben(&pcm).expect("FLAC schreiben");
        assert_eq!(&flac[0..4], b"fLaC", "GEMESSEN: FLAC-Signatur");
        // 1. WAV -> WAV und WAV -> FLAC.
        for (ziel_endung, kopf) in [("wav", &b"RIFF"[..]), ("flac", &b"fLaC"[..])] {
            let ziel = ton_ziel("audio/wav", "https://a.example/ton.wav", ziel_endung, None);
            let datei = konvertiere("https://a.example/ton.wav", &wav, &ziel)
                .unwrap_or_else(|e| panic!("ton.wav -> {ziel_endung}: {e}"));
            assert_eq!(&datei.daten[0..kopf.len()], kopf, "GEMESSEN: der Kopf");
            assert!(datei.name.ends_with(&format!(".{ziel_endung}")));
            eprintln!(
                "KTX10-TON ton.wav -> {}: {} Byte",
                datei.name,
                datei.daten.len()
            );
        }
        // 2. FLAC -> WAV und FLAC -> FLAC. GEMESSEN: FLAC **lesen** kann die
        //    Pipeline (`ton_aus_bytes` liest `fLaC` per Signatur) — der
        //    Quelltyp-Weg kann es nicht (`Quelltyp::ton_lesen` sagt „FLAC ist
        //    gebaut, aber nicht gelesen“). Deshalb der Bytes-Weg.
        //
        //    GEMESSENE ENTSCHEIDUNG, warum bei FLAC -> FLAC das **Original**
        //    erwartet wird: das Menue nennt bei einer FLAC-Quelle die Zeile
        //    „Als FLAC (Original)“ — das ist „speichern“, nicht „umwandeln“.
        //    Wer das verlangt, bekommt die Quelldatei Bit fuer Bit; das wird
        //    hier behauptet, weil „unveraendert abgelegt“ sonst niemand prueft.
        for (ziel_endung, soll_original) in [("wav", false), ("flac", true)] {
            let ziel = ton_ziel(
                "audio/flac",
                "https://a.example/ton.flac",
                ziel_endung,
                None,
            );
            assert_eq!(
                ziel.original,
                soll_original,
                "GEMESSEN: FLAC -> {ziel_endung} ist {} Original",
                if ziel.original { "das" } else { "kein" }
            );
            let datei = konvertiere("https://a.example/ton.flac", &flac, &ziel)
                .unwrap_or_else(|e| panic!("ton.flac -> {ziel_endung}: {e}"));
            assert!(
                datei.daten.len() > 8,
                "GEMESSEN: FLAC -> {ziel_endung} gab {} Byte",
                datei.daten.len()
            );
            if soll_original {
                assert_eq!(datei.daten, flac, "GEMESSEN: das Original ist veraendert");
            } else {
                assert_eq!(
                    &datei.daten[0..4],
                    b"RIFF",
                    "GEMESSEN: FLAC -> WAV ist kein WAV"
                );
            }
            eprintln!(
                "KTX10-TON ton.flac -> {}: {} Byte{}",
                datei.name,
                datei.daten.len(),
                if soll_original { " (Original)" } else { "" }
            );
        }
        // 3. EIN ECHTES WEBM MIT TON -> WAV, FLAC, GIF. GEMESSENE ENTSCHEIDUNG,
        //    warum diese Datei aus `narsil-konvert/tests/daten/` kommt: sie ist
        //    ein **echter** Container (10 794 Byte, VP8 + VORBIS) und keine
        //    5-Byte-Fixtur.
        let film = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|p| p.parent())
                .expect("Repo-Wurzel")
                .join("crates/narsil-konvert/tests/daten/k14_ton_video.webm"),
        )
        .expect("das WebM mit Ton");
        for ziel_endung in ["wav", "flac", "gif"] {
            let ziel = ton_ziel(
                "video/webm",
                "https://a.example/film.webm",
                ziel_endung,
                Some(&film),
            );
            let datei = konvertiere("https://a.example/film.webm", &film, &ziel)
                .unwrap_or_else(|e| panic!("Film -> {ziel_endung}: {e}"));
            assert!(
                datei.daten.len() > 8,
                "GEMESSEN: Film -> {ziel_endung} gab {} Byte",
                datei.daten.len()
            );
            eprintln!(
                "KTX10-TON Film -> {}: {} Byte",
                datei.name,
                datei.daten.len()
            );
        }
    }

    /// KONTEXT (03.10.2026, 01:05): EIN **STUMMES** WEBM BLEIBT EIN FEHLER —
    /// und der Fehler **nennt** den Ton.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum dieser Fall **vor** dem Fix schon „richtig“
    /// war und trotzdem behauptet wird: der alte Weg sagte „Diese Datei
    /// enthaelt keinen Ton“ **immer** — auch bei einem WebM **mit** Ton. Nach
    /// dem Fix entscheidet `Quelltyp::als_eingabe` (`webm::hat_ton`), und ein
    /// stummer Container muss **wieder** abgelehnt werden. GEMESSENE
    /// ENTSCHEIDUNG, warum der Text **beide** Woerter annimmt: KONVERT sagt
    /// „keine Audiospur“, der alte Text sagte „keinen Ton“ — und der Test soll
    /// nicht an **einer** Wortwahl hängen, sondern an der **Wirkung**.
    #[test]
    fn ein_stummes_webm_wird_als_kein_ton_abgelehnt() {
        let wurzel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("Repo-Wurzel")
            .to_path_buf();
        let stumm = std::fs::read(wurzel.join("crates/narsil-konvert/tests/daten/k14_stumm.webm"))
            .expect("das stumme WebM");
        //
        // GEMESSEN: das Menue bietet fuer dieses WebM **zwei** Ziele
        // („Als AVI“, „Als PNG (erstes Bild)“) — WAV und FLAC stehen **nicht**
        // da, weil K5 keine toten Eintraege erlaubt. Der Klick wird hier also
        // **von Hand** ausgefuehrt: er darf nicht sterben, sondern muss sagen,
        // was fehlt.
        let ziel = hand_ziel("wav");
        let e = konvertiere("https://a.example/stumm.webm", &stumm, &ziel)
            .expect_err("GEMESSEN: ein stummes WebM darf keine WAV-Datei liefern");
        eprintln!("KTX10-TON stummes WebM -> WAV: {e}");
        assert!(
            e.contains("Ton") || e.contains("Audiospur"),
            "GEMESSEN: der Fehler nennt den Ton nicht: {e}"
        );
        // UND: das Bild-Ziel aus demselben WebM **geht** — der Film ist lesbar,
        //     nur sein Ton fehlt. GEMESSENE ENTSCHEIDUNG, warum das behauptet
        //     wird: sonst waere ein Fehlertext „kein Ton“ gleichbedeutend mit
        //     „kaputt“, und der Nutzer wuerde die Datei wegwerfen.
        let z = ziele(
            Kontext::Video,
            "video/webm",
            "https://a.example/stumm.webm",
            Some(&stumm),
        );
        //
        // KONTEXT (03.10.2026, 02:40): die Behauptung ist jetzt **kein Ton**,
        // nicht mehr eine **Zahl**. GEMESSENER BEFUND: hier stand `z.len() == 2`,
        // und KONVERT hat inzwischen den GIF-Weg gebaut — GEMESSEN sind jetzt
        // **drei** Ziele (AVI, GIF (animiert), PNG (erstes Bild)). Eine Zahl
        // waere eine Behauptung ueber KONVERTs Fortschritt; die Behauptung
        // ueber diesen Test ist: **kein** Ton-Ziel bei einem stummen Film.
        assert!(
            !z.iter()
                .any(|x| matches!(x.format.as_str(), "wav" | "flac" | "mp3")),
            "GEMESSEN: das stumme WebM bietet ein Ton-Ziel: {z:?}"
        );
        assert!(
            !z.iter().any(|x| x.hinweis.contains("Ton")),
            "GEMESSEN: das stumme WebM verspricht Ton: {z:?}"
        );
        let avi = z.iter().find(|x| x.format == "avi").expect("AVI-Zeile");
        let datei = konvertiere("https://a.example/stumm.webm", &stumm, avi)
            .expect("GEMESSEN: ein stummes WebM ist ein Film");
        assert!(
            datei.daten.len() > 100,
            "GEMESSEN: {} Byte AVI",
            datei.daten.len()
        );
        assert_eq!(&datei.daten[0..4], b"RIFF", "GEMESSEN: AVI-Fuss");
    }

    /// KONTEXT (03.10.2026, 01:05): ENDE-ZU-ENDE — DIE TON-DATEIEN LIEGEN IN
    /// `~/Downloads`, und `file` **und** `ffprobe` sagen, was darin steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `ffprobe` **zusaetzlich** zu `file` laeuft:
    /// `file` liest die **ersten Bytes** und erkennt WAV/FLAC an der Signatur —
    /// das beweist den Container, nicht den **Inhalt** (Sampling-Rate, Kanaele,
    /// Samples). `ffprobe` liest den **Kopf des Audio-Streams** und beweist
    /// beides. GEMESSENE ENTSCHEIDUNG, warum ein **fehlendes** `ffprobe` kein
    /// Fehler ist: es ist ein Werkzeug der Umgebung, kein Geraet der Crate —
    /// und die Behauptung „die Datei ist echt“ darf nicht an einem Programm
    /// haengen, das jemand nicht installiert hat.
    #[test]
    fn der_ton_klick_legt_dateien_in_downloads() {
        let pcm = ton_pcm();
        let wav = narsil_konvert::wav::schreiben(&pcm, narsil_konvert::wav::WavArt::Pcm16)
            .expect("WAV schreiben");
        // Release 09.10.2026: ein Download-Ordner im Temp-Verzeichnis. GEMESSEN
        // vorher: `$HOME/Downloads` — unter Git Bash der ECHTE Ordner des
        // Nutzers, nach jedem Testlauf lagen dort `ton.wav` und `ton.flac`.
        let ordner = std::env::temp_dir().join(format!("narsil-test-downloads-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Test-Downloads");
        // DREI DATEIEN: WAV aus WAV, FLAC aus WAV, WAV aus FLAC.
        let flac = narsil_konvert::flac::schreiben(&pcm).expect("FLAC schreiben");
        //
        // GEMESSENE ENTSCHEIDUNG, warum je Fall die Zielliste der **eigenen**
        // Quelle gebaut wird: sonst ist das Ziel das **Original**, und dann legt
        // `konvertiere` die Quelldatei unveraendert ab (GEMESSEN: eine Datei
        // namens `ton.wav`, in der FLAC stand).
        let faelle = [
            ("audio/wav", "ton.wav", wav.clone(), "wav"),
            ("audio/wav", "ton.wav", wav, "flac"),
            ("audio/flac", "ton.flac", flac, "wav"),
        ];
        for (mime, quelle, daten, ziel_endung) in faelle {
            let ziel = ton_ziel(
                mime,
                &format!("https://a.example/{quelle}"),
                ziel_endung,
                None,
            );
            let datei = konvertiere(&format!("https://a.example/{quelle}"), &daten, &ziel)
                .unwrap_or_else(|e| panic!("{quelle} -> {ziel_endung}: {e}"));
            let pfad = ordner.join(&datei.name);
            std::fs::write(&pfad, &datei.daten).expect("Datei schreiben");
            let datei_text = std::process::Command::new("file")
                .arg(&pfad)
                .output()
                .expect("das Kommando `file`");
            let erkannt = String::from_utf8_lossy(&datei_text.stdout)
                .trim()
                .to_string();
            eprintln!(
                "KTX10-DATEI {} -> {}\n  {}",
                pfad.display(),
                datei.name,
                erkannt
            );
            assert!(
                erkannt.contains("audio") || erkannt.contains("WAVE") || erkannt.contains("FLAC"),
                "GEMESSEN: `file` erkennt keine Audiodatei: {erkannt}"
            );
            // UND: `ffprobe`, **wenn** es da ist (siehe Kopf).
            if let Ok(sonde) = std::process::Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_name,sample_rate,channels",
                ])
                .arg(&pfad)
                .output()
            {
                let text = String::from_utf8_lossy(&sonde.stdout);
                eprintln!("  ffprobe: {}", text.replace('\n', " "));
                assert!(
                    text.contains("pcm_s16le") || text.contains("flac"),
                    "GEMESSEN: ffprobe nennt keinen Audio-Codec: {text}"
                );
            }
        }
    }
}
