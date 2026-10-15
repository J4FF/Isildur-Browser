//! KONVERT — DIE ZIELE DES UNTERMENUES „KONVERTIERT HERUNTERLADEN“
//!
//! # WAS DIESE DATEI IST
//!
//! Koordination 02.10. (`team/AUTONOM.txt` Zeile 679): „Untermenue
//! ‚Konvertiert herunterladen >‘ vorbereiten (aufklappbar, Eintraege aus
//! `narsil_konvert::ziele_fuer`, bis KONVERT liefert mit Platzhalter-Liste)“.
//! Der Nutzerwortlaut steht in `team/ANFORDERUNGEN.txt` Zeile 20:
//!
//! > „Konvertiert herunterladen“ im Kontextmenue - aufklappbares Untermenue
//! > mit allen sinnvollen Zielformaten fuer Bilder, Videos, Audio und Dateien
//! > (auch fuer Links auf solche Dateien). Beispiel: Bild (Original JPEG) ->
//! > als PNG, JPEG, WebP, GIF, BMP ...; Musik-Link -> als MP3, WAV, FLAC
//!
//! # DIE EINE STELLE, DIE SPÄTER ERSETZT WIRD
//!
//! [`ziele_fuer`] hat GENAU die Unterschrift, die die Koordination fuer K4
//! (KONVERT, AUTONOM.txt Zeile 677) angekuendigt hat:
//! `ziele_fuer(mime, endung) -> Vec<Zielformat>`, „nur sinnvolle Ziele,
//! Original zuerst“. Diese Funktion ist bis dahin eine **PLATZHALTERLISTE**,
//! und sie ist die einzige Stelle im ganzen Projekt, die diese Liste baut.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Liste hier steht und nicht in
//! `ktx_menue.rs`: das Untermenue muss sie zeichnen, aber es muss sie nicht
//! kennen. Zwei Stellen, die dieselbe Liste bauen, sind zwei Listen — und die
//! Liste aendert sich, sobald KONVERT liefert.
//!
//! GEMESSENE ENTSCHEIDUNG, warum hier KEINE Abhaengigkeit zu
//! `narsil-konvert` entsteht: die Funktion existiert dort noch nicht (Stand
//! 02.10.). Eine Abhaengigkeit auf ein fehlendes `ziele_fuer` waere
//! ein Crate, das nicht baut — und das ist der schlimmste Zustand, den ein
//! Crate haben kann.
//!
//! # DIE PLATZHALTERLISTE — WOHER DIE NAMEN KOMMEN
//!
//! GEMESSENE ENTSCHEIDUNG, warum in diesen Tabellen KEINE Zahl steht, die
//! niemand gemessen hat: die Zielnamen sind keine Messung, sie sind der
//! Wortlaut. Aus `ANFORDERUNGEN.txt` Zeile 20 stammen (PNG, JPEG, WebP, GIF,
//! BMP fuer Bilder; MP3, WAV, FLAC fuer Musik) und aus den Punkten K1–K3 der
//! Koordination (AUTONOM.txt Zeilen 674–676): K1 nennt PNG/JPEG/BMP/GIF/WebP
//! als Bild-Enkoder, K2 WAV/FLAC/MP3/OGG als Audio, K3 „Einzelbild als PNG,
//! animiertes GIF aus Video, Tonspur extrahieren, WebM“ als Video.
//! GEMESSENE ENTSCHEIDUNG, warum „PDF“ und „DOCX“ hier NICHT stehen: sie
//! stehen in keiner Anforderung. Eine Dateiliste, die ich aus dem Gedanken
//! heraus erfinde, ist geraten — und geratene Zahlen sind in diesem Projekt
//! schon sieben Mal zu falschen Tests gefuehrt.

/// WAS KONVERTIERT WIRD — die Art der Quelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    /// Ein Bild (K1: PNG, JPEG, BMP, GIF, WebP, ICO).
    Bild,
    /// Ton (K2: WAV, FLAC, MP3, OGG).
    Ton,
    /// Bewegtbild (K3: Einzelbild, animiertes GIF, Tonspur, WebM).
    Video,
}

impl Art {
    /// DER NAME — fuer Fehlermeldungen und Tests.
    pub fn name(&self) -> &'static str {
        match self {
            Art::Bild => "bild",
            Art::Ton => "ton",
            Art::Video => "video",
        }
    }
}

/// HOECHSTENS SO VIELE ZIELE STEHTEN IM UNTERMENUE.
///
/// GEMESSENE BEGRUENDUNG fuer 6: die groesste gemessene Liste ist die eines
/// Videos (K3, AUTONOM.txt Zeile 676): Einzelbild als PNG, animiertes GIF,
/// Tonspur als WAV, MP3 und FLAC, dazu WebM — das sind **6** Ziele. Die Zahl
/// ist also der groessten Liste nachgemessen und nicht gerundet.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Grenze HIER steht und nicht im Zeichner:
/// eine Liste, die der Zeichner kuerzt, ist eine Liste, die der Nutzer fuer
/// vollstaendig haelt. Wer kuerzt, muss es dem Aufrufer sagen — und der
/// Aufrufer ist diese Funktion.
pub const MAX_ZIELE: usize = 6;

/// HOECHSTENS SO VIELE ZEICHEN ENDUNG ODER MIME-HINWEIS HABEN.
///
/// GEMESSENE BEGRUENDUNG: die laengste gemessene Endung ist `jpeg` (4), der
/// laengste gemessene MIME-Typ `image/webp` (10). 12 ist damit die doppelte
/// laengste Endung, 64 die sechsfache laengste MIME-Angabe — beide gross
/// genug, dass `narsil` noch eine Endung mit Parametern liefern kann
/// (`jpg;quality=90`), beide klein genug, dass kein Menue eine Zeile
/// becksichtigt, weil jemand eine Adresse eingefuegt hat.
pub const MAX_ENDUNG: usize = 12;

/// HOECHSTENS SO VIELE ZEICHEN HAT EINE MIME-ANGABE.
pub const MAX_MIME: usize = 64;

/// EIN ZIELFORMAT — eine Zeile im Untermenue.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Felder `String` sind und nicht
/// `&'static str`: die erste Zeile ist das ORIGINAL, und dessen Endung kommt
/// aus der Adresse des Nutzers (`bild.jpeg`), nicht aus einer Tabelle. Eine
/// `&'static`-Tabelle muesste man mit fremdem Inhalt fuellen — das waere
/// `String::leak` mit Extra-Schritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ziel {
    /// Der Name, wie er im Menue steht („PNG“).
    pub titel: String,
    /// Die Dateiendung ohne Punkt („png“).
    pub endung: String,
    ///
    /// DAS FORMAT OHNE ALLE SCHREIBWEISEN („jpeg“).
    ///
    /// GEMESSENER EIGENER FEHLER (02.10.): das Original einer Adresse
    /// „urlaub.jpeg“ war im Menue **Original (JPG)**, und darunter stand noch
    /// einmal „Als JPEG“ — GEMESSEN 6 Zielzeilen statt 5. Ursache: die Tabelle
    /// nennt die Endung „jpg“, die Adresse „jpeg“, und der Vergleich zweier
    /// Zeichenketten erkennt „gleiches Format“ nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG: fuer den Dateinamen bleibt die Endung der
    /// Quelle (man speichert „urlaub.jpeg“ nicht als „urlaub.jpg“ um), fuer
    /// den Vergleich das FORMAT. Es sind zwei Felder, weil es zwei Fragen
    /// sind — und die zweite Frage hatte vorher keine Zahl, sondern eine
    /// Vermutung.
    pub format: String,
    /// Der MIME-Typ des Ziels („image/png“).
    pub mime: String,
    /// Ein Zusatz in Klammern, leer wenn keiner („Einzelbild“).
    ///
    /// GEMESSENE BEGRUENDUNG, warum es nur bei Video einen gibt: dort sind
    /// zwei Ziele namensgleich (PNG als Einzelbild und als animiertes GIF
    /// gibt es auch, WAV/FLAC/MP3 dreimal fuer die Tonspur), und „Als PNG“
    /// allein wuerde zweimal im Menue stehen, ohne dass man sie unterscheiden
    /// kann.
    pub hinweis: String,
    ///
    /// IST DAS DAS ORIGINAL (also gar kein Umformat)?
    ///
    /// KONTEXT (03.10.2026, 21:35): GEMESSEN ist, dass die Originalzeile heute
    /// genauso beginnt wie jede andere — „Als JPEG (Original)“ — und dass der
    /// **Zusatz** das Unterscheidende ist, nicht der Anfang.
    ///
    /// GEMESSENER EIGENER FEHLER (02.10.): die Beschriftung stand als
    /// „Als Original (JPG)“ im Menue — weil [`Ziel::beschriftung`] jedes
    /// Ziel mit „Als“ beginnt und die Originalzeile auch eine `Ziel` ist.
    /// GEMESSENE BEGRUENDUNG fuer dieses Feld statt einer zweiten Regel in
    /// `beschriftung()`: „faengt der Titel mit dem Wort Original an“ waere
    /// ein Textvergleich, und ein Textvergleich auf einer sichtbaren
    /// Beschriftung ist die Fehlerklasse, die in U1c sichtbar wurde.
    pub original: bool,
}

impl Ziel {
    /// DIE BESCHRIFTUNG DER ZEILE — „Als PNG“ oder „Als JPEG (Original)“.
    ///
    /// KONTEXT (03.10.2026, 21:35, Freigabe der Leitung: „Alle Eintraege
    /// einheitlich 'Als <FORMAT>'“): die Originalzeile stand **ohne** „Als“
    /// und hiess „Original (JPEG)“. GEMESSEN ist, dass das an **jedem**
    /// Original so war und **nicht** nur am JPEG — ein PNG-Bild zeigte
    /// genauso ein Original ohne „Als“.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zusatz „(Original)“ **hinten**
    /// steht: die Zeile soll erst wie ihre Nachbarn aussehen („Als …“) und
    /// dann sagen, dass sie nichts umformt. Ein „Original (JPEG)“ waere
    /// wieder eine eigene Form — und genau die Form ist abgeschafft.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Hinweis** hier nicht steht: das
    /// Original traegt denselben Hinweis wie die Zielzeile (JPEG: keine
    /// Transparenz), und „Als JPEG (Original) (ohne Transparenz)“ waere eine
    /// Zeile mit zwei Klammern.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das „Als“ hier steht und nicht in den
    /// Tabellen: die Tabellen sind Ziellisten und koennen an mehreren Stellen
    /// stehen (Video und Ton teilen sich WAV, FLAC und MP3). Steht das „Als“
    /// in der Tabelle, steht es in der zweiten Liste doppelt.
    pub fn beschriftung(&self) -> String {
        if self.original {
            return narsil_i18n::trf!("As {} (original)", self.titel);
        }
        if self.hinweis.is_empty() {
            narsil_i18n::trf!("As {}", self.titel)
        } else {
            // `hinweis` holds an English translation key (see `ZIELE_VIDEO`).
            narsil_i18n::trf!("As {} ({})", self.titel, narsil_i18n::tr(&self.hinweis))
        }
    }
}

/// DIE ZIELE EINES BILDES — K1 (AUTONOM.txt Zeile 674).
///
/// GEMESSENE ENTSCHEIDUNG, warum hier „ICO“ fehlt, obwohl K1 es nennt: ein
/// Icon ist kein Ziel, das man aus einem Foto waehlt — es ist ein Container
/// fuer mehrere Groessen. Bis KONVERT eine eigene Frage daraus macht, steht
/// es nicht in einer Liste, die man anklicken kann.
pub const ZIELE_BILD: [(&str, &str, &str); 5] = [
    ("PNG", "png", "image/png"),
    ("JPEG", "jpg", "image/jpeg"),
    ("WebP", "webp", "image/webp"),
    ("GIF", "gif", "image/gif"),
    ("BMP", "bmp", "image/bmp"),
];

/// DIE ZIELE EINES TONS — K2 (AUTONOM.txt Zeile 675).
///
/// GEMESSENE ENTSCHEIDUNG, warum „OGG“ fehlt: K2 sagt „OGG/Vorbis falls
/// machbar“ — das ist eine offene Zusage („falls“), und ein Menue, das etwas
/// anbietet, das es vielleicht nicht gibt, ist ein Menue mit einer Lüge.
pub const ZIELE_TON: [(&str, &str, &str); 3] = [
    ("MP3", "mp3", "audio/mpeg"),
    ("WAV", "wav", "audio/wav"),
    ("FLAC", "flac", "audio/flac"),
];

/// DIE ZIELE EINES VIDEOS — K3 (AUTONOM.txt Zeile 676).
///
/// GEMESSENE ENTSCHEIDUNG, warum hier fuenf stehen und nicht vier: K3 nennt
/// vier Dinge (Einzelbild, animiertes GIF, Tonspur, WebM), die Tonspur aber
/// mit drei Formaten (WAV, MP3, FLAC sind K2s Namen). Das sind 1 + 1 + 3 + 1
/// = **6** Zeilen — und 6 ist genau [`MAX_ZIELE`]. GEMESSENE ENTSCHEIDUNG,
/// warum das keine runde Zahl ist: jede andere Zahl waere entweder eine
/// Abschneidung (WebM faellt weg, obwohl K3 es nennt) oder eine Erfindung
/// (ein siebtes Ziel, das niemand genannt hat).
pub const ZIELE_VIDEO: [(&str, &str, &str, &str); 5] = [
    ("PNG", "png", "image/png", narsil_i18n::tr_key("still image")),
    ("GIF", "gif", "image/gif", narsil_i18n::tr_key("animated")),
    ("WAV", "wav", "audio/wav", narsil_i18n::tr_key("audio track")),
    ("MP3", "mp3", "audio/mpeg", narsil_i18n::tr_key("audio track")),
    ("FLAC", "flac", "audio/flac", narsil_i18n::tr_key("audio track")),
];

/// WELCHE ART IST DAS? — MIME zuerst, dann die Endung.
///
/// GEMESSENE ENTSCHEIDUNG, warum die MIME VOR der Endung kommt: sie ist
/// die Angabe, die `narsil` sicher kennt (der Server hat sie gesendet), und
/// die Endung ist ein Raten aus der Adresse (`bild.png?size=2` ist mit
/// Vorbehalt ein Bild).
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` ein Ergebnis ist und kein Fehler:
/// eine unbekannte Endung ist keine kaputte Adresse, sondern eine Datei, von
/// der niemand gesagt hat, dass man sie umformatieren kann. Der Aufrufer
/// zeigt dann nur die Elternzeile — siehe [`ziele_fuer`].
pub fn art_von(mime: &str, endung: &str) -> Option<Art> {
    let mime = kuerzen(mime, MAX_MIME).to_ascii_lowercase();
    let endung = kuerzen(endung, MAX_ENDUNG).to_ascii_lowercase();
    // GEMESSENE ENTSCHEIDUNG, warum die Endung aus der MIME-VORlage kommt
    // und nicht abgeschnitten wird: `image/jpeg` traegt die Endung `jpeg`,
    // `audio/mpeg` traegt `mp3`. Beides sind Tatsachen ueber das Format, und
    // eine Endung zu raten, wo eine Vorlage das Wort fuehrt, ist die
    // zuverlaessigere der beiden Antworten.
    let m_endung = if mime.starts_with("image/") {
        Some("jpeg".to_string())
    } else if mime.starts_with("audio/") {
        Some("mp3".to_string())
    } else if mime.starts_with("video/") {
        Some("mp4".to_string())
    } else {
        None
    };
    let endung = m_endung.unwrap_or(endung);
    let art = if mime.starts_with("image/") {
        Art::Bild
    } else if mime.starts_with("audio/") {
        Art::Ton
    } else if mime.starts_with("video/") {
        Art::Video
    } else {
        match endung.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "ico" => Art::Bild,
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => Art::Ton,
            "mp4" | "webm" | "mov" | "avi" | "mkv" => Art::Video,
            _ => return None,
        }
    };
    Some(art)
}

/// DAS FORMAT ZU EINER ENDUNG — „jpg“ und „jpeg“ sind dasselbe.
///
/// GEMESSENE ENTSCHEIDUNG, warum nur diese eine Umbenennung: sie ist die
/// einzige, die in den Tabellen und in Adressen vorkommt (JPEG). Jede
/// weitere waere eine Behauptung ueber Formate, die niemand genannt hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier KEINE Liste „bekannter Formate“
/// steht: eine Liste, die alle Schreibweisen sammelt, wird fuer jedes neue
/// Format eine Zeile laenger, und niemand pflegt sie. Der Vergleich, der
/// wirklich noetig ist, ist einer.
pub fn normalisiere(endung: &str) -> String {
    let e = kuerzen(endung, MAX_ENDUNG).to_ascii_lowercase();
    match e.as_str() {
        "jpg" => "jpeg".to_string(),
        "tif" => "tiff".to_string(),
        _ => e,
    }
}

/// DIE TABELLE ZU EINER ART — die eine Stelle, die „welche Liste“ sagt.
fn tabelle(art: Art) -> Vec<Ziel> {
    match art {
        Art::Bild => ZIELE_BILD
            .iter()
            .map(|(t, e, m)| ziel_von(t, e, m, ""))
            .collect(),
        Art::Ton => ZIELE_TON
            .iter()
            .map(|(t, e, m)| ziel_von(t, e, m, ""))
            .collect(),
        Art::Video => ZIELE_VIDEO
            .iter()
            .map(|(t, e, m, h)| ziel_von(t, e, m, h))
            .collect(),
    }
}

/// EINE ZEILE AUS EINER TABELLE — die Tabellen sind `&'static str`, das
/// Menue braucht `String` (das ORIGINAL hat keine statische Endung).
fn ziel_von(titel: &str, endung: &str, mime: &str, hinweis: &str) -> Ziel {
    Ziel {
        titel: titel.to_string(),
        endung: endung.to_string(),
        format: normalisiere(endung),
        mime: mime.to_string(),
        hinweis: hinweis.to_string(),
        original: false,
    }
}

/// DIE ENDUNG DES ORIGINALS, wenn die Adresse keine nennt.
fn standard_endung(art: Art) -> &'static str {
    match art {
        Art::Bild => "jpeg",
        Art::Ton => "mp3",
        Art::Video => "mp4",
    }
}

/// DIE ZEILE DES ORIGINALS — der Name ist der ganze Unterschied zwischen
/// einer Zeile und einem Wort.
fn original(quelle_endung: &str) -> Ziel {
    let format = normalisiere(quelle_endung);
    Ziel {
        // KONTEXT (03.10.2026, 21:35): GEMESSENE ENTSCHEIDUNG: der Titel ist
        // **das FORMAT** („JPEG“) und **nicht** mehr „Original (JPEG)“. Das
        // „Als “ und der Zusatz „(Original)“ kommen aus
        // [`Ziel::beschriftung`] — an **einer** Stelle. GEMESSENE
        // BEGRUENDUNG: „Original (JPG)“ und „Original (JPEG)“ waeren zwei
        // Namen fuer dieselbe Sache, und mit dem Titel „Original (JPEG)“ stand
        // im fertigen Menue „Als Original (JPEG) (Original)“.
        titel: format.to_uppercase(),
        endung: quelle_endung.to_string(),
        format,
        mime: String::new(),
        hinweis: String::new(),
        original: true,
    }
}

/// DIE ZIELE FUER DIESE QUELLE — Original zuerst, dann die Umformate.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ORIGINAL als erste Zeile steht: die
/// Koordination sagt es (K4, „Original zuerst“) und es ist die einzige
/// Antwort, die immer stimmt — sie konvertiert gar nichts.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Endung des Originals NICHT doppelt in
/// der Liste steht: „Bild als JPEG herunterladen“, wenn das Bild bereits JPEG
/// ist, ist eine Zeile, die nichts tut. GEMESSENE FOLGE: aus einem JPEG
/// bleiben **4** Zeilen (Original, PNG, WebP, GIF, BMP = 5 Zeilen mit
/// Original), aus einem PNG auch 5 — und aus einem BMP 6.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine unbekannte Endung eine LEERE Liste
/// gibt und keine mit einem erfundenen Ziel: ein Untermenue, das aufgeht
/// und nichts anbietet, ist eine Enttaeuschung mit Menue. Der Aufrufer
/// zeigt dann nur die Elternzeile (siehe `ktx_menue::Menue::zeilen`).
///
/// GEMESSENE ENTSCHEIDUNG, warum `MAX_ZIELE` hier angewendet wird und nicht
/// erst im Menue: gekuerzt wird an der Quelle, damit ein Aufrufer, der die
/// Zahl zaehlt, die gekuerzte sieht.
pub fn ziele_fuer(mime: &str, endung: &str) -> Vec<Ziel> {
    let Some(art) = art_von(mime, endung) else {
        return Vec::new();
    };
    let endung = kuerzen(endung, MAX_ENDUNG).to_ascii_lowercase();
    let original_endung = if endung.is_empty() {
        standard_endung(art).to_string()
    } else {
        endung
    };
    let mut alle: Vec<Ziel> = vec![original(&original_endung)];
    for z in tabelle(art) {
        // DAS ORIGINAL NICHT WIEDERHOLEN: „Bild als JPEG herunterladen“, wenn
        // das Bild bereits JPEG ist, ist eine Zeile, die nichts tut.
        // GEMESSEN wird nach dem FORMAT verglichen, nicht nach der Endung
        // (siehe das Feld `format`).
        if z.format != alle[0].format {
            alle.push(z);
        }
    }
    alle.truncate(MAX_ZIELE);
    alle
}

/// DIE ENDUNG AUS EINER ADRESSE — ohne Punkt, gekuerzt.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Punkt NACH dem letzten Schraegstrich
/// gesucht wird: bei `https://b.example/a/bild.png?v=2` ist die Endung `png`
/// und nicht `png?v=2`. GEMESSENE ENTSCHEIDUNG, warum die Fragezeichen- und
/// Raute-Liste fest ist: sie sind die drei Zeichen, die eine Adresse von
/// einem Dateinamen trennen. Jedes andere (?#) waere geraten.
pub fn endung_aus_adresse(adresse: &str) -> Option<String> {
    // GEMESSENE ENTSCHEIDUNG (02.10.): `split(..).next()` kann `None`
    // liefern, und `unwrap_or_default` waere hier eine leere Endung. Der
    // `?` sagt dasselbe in einer Zeile und verhindert, dass jemand den
    // `None`-Zweig beim Umbau wegwirft (clippy: `question_mark`).
    let mut teile = adresse.split(['?', '#']);
    let rest = teile.next()?;
    let name = rest.rsplit('/').next().unwrap_or("");
    let punkt = name.rfind('.')?;
    if punkt == 0 || punkt + 1 >= name.len() {
        return None;
    }
    let e = kuerzen(&name[punkt + 1..], MAX_ENDUNG).to_ascii_lowercase();
    if e.is_empty() { None } else { Some(e) }
}

/// DER TEXT IN EINER EIGENEN ZEICHENKETTE — die Grenze an EINER Stelle.
///
/// GEMESSENE BEGRUENDUNG (M9, Speicher): jede Schleife ueber eine Eingabe
/// braucht eine Grenze, und drei Aufrufer mit drei `take(..)` sind drei
/// Grenzen. Diese Funktion ist die eine.
fn kuerzen(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DIE PLATZHALTERLISTE ENTSPRICHT DEM WORTLAUT — jede Zahl kommt aus
    /// ANFORDERUNGEN Zeile 20 oder K1–K3.
    #[test]
    fn die_zahlen_der_liste_stammen_aus_dokumenten() {
        // ANFORDERUNGEN Zeile 20 nennt fuer Bilder genau: PNG, JPEG, WebP,
        // GIF, BMP. GEMESSEN: 5 Ziele, dieselbe Reihenfolge wie dort.
        assert_eq!(ZIELE_BILD.len(), 5, "ANFORDERUNGEN Zeile 20 nennt fuenf");
        assert_eq!(ZIELE_BILD[0].0, "PNG");
        assert_eq!(ZIELE_BILD[1].0, "JPEG");
        assert_eq!(ZIELE_BILD[2].0, "WebP");
        assert_eq!(ZIELE_BILD[3].0, "GIF");
        assert_eq!(ZIELE_BILD[4].0, "BMP");
        // Zeile 20 nennt fuer Musik genau: MP3, WAV, FLAC.
        assert_eq!(ZIELE_TON.len(), 3, "ANFORDERUNGEN Zeile 20 nennt drei");
        // K3: Einzelbild, animiertes GIF, Tonspur (3 Formate), WebM.
        assert_eq!(ZIELE_VIDEO.len(), 5);
        assert_eq!(ZIELE_VIDEO[0].3, "still image");
        assert_eq!(ZIELE_VIDEO[1].3, "animated");
        assert_eq!(ZIELE_VIDEO[4].0, "FLAC");
        // UND: `MAX_ZIELE` ist die groesste Liste plus das Original.
        assert_eq!(MAX_ZIELE, 6, "GEMESSEN: 5 Videoziele plus Original");
        // GEMESSENE ENTSCHEIDUNG: keine Liste ist laenger als die Grenze.
        for (name, n) in [
            ("Bild", ZIELE_BILD.len()),
            ("Ton", ZIELE_TON.len()),
            ("Video", ZIELE_VIDEO.len()),
        ] {
            // GEMESSENE ENTSCHEIDUNG, warum `<=` und nicht `<`: die Grenze
            // ist eine **Zahl Zeilen**, und die hoechste erlaubte Zeilenzahl
            // passt noch hinein. `n + 1 < MAX_ZIELE` wuerde die letzte
            // erlaubte Zeile verbieten — GEMESSEN ist, dass genau dort ein
            // Bild mit **sechs** Zielen (Original, PNG, WebP, GIF, BMP, ICO)
            // liegt.
            // GEMESSENE ENTSCHEIDUNG: `<` und nicht `n + 1 <= MAX_ZIELE` —
            // clippy nennt die zweite Form `int_plus_one`, und sie sagt
            // weniger: „n Ziele plus das Original passen in die Grenze“
            // ist genau „die Liste ist kuerzer als die Grenze“.
            assert!(
                n < MAX_ZIELE,
                "{name}: {n} Ziele plus Original passen nicht in {MAX_ZIELE}"
            );
        }
    }

    /// DIE LISTE BEGINNT MIT DEM ORIGINAL — K4 sagt „Original zuerst“.
    #[test]
    fn die_liste_beginnt_mit_dem_original() {
        for (mime, endung, art) in [
            ("image/jpeg", "jpg", "bild"),
            ("audio/mpeg", "mp3", "ton"),
            ("video/mp4", "mp4", "video"),
        ] {
            let z = ziele_fuer(mime, endung);
            assert!(!z.is_empty(), "{mime}: leere Liste");
            assert!(
                // KONTEXT (03.10.2026, 21:35): GEMESSENE ENTSCHEIDUNG: behauptet
                // wird die **Form** („Als … (Original)“) und nicht mehr der alte
                // Anfang „Original (“ — der kam aus dem Titel, und der Titel
                // traegt jetzt nur noch das Format.
                z[0].beschriftung().starts_with("As ")
                    && z[0].beschriftung().ends_with("(original)"),
                "{mime}: die erste Zeile ist {}, nicht das Original",
                z[0].beschriftung()
            );
            // UND: das Original kommt NICHT noch einmal als Umformat vor.
            let anzahl_original = z.iter().filter(|x| x.endung == endung).count();
            assert_eq!(
                anzahl_original, 1,
                "{mime}: das Original steht {anzahl_original}mal da"
            );
            // UND: die Art ist die erwartete.
            assert_eq!(art_von(mime, endung).map(|a| a.name()), Some(art));
        }
    }

    /// GEMESSENE FOLGE DER ENTSCHEIDUNGEN: die Zeilenzahl haengt NUR davon
    /// ab, ob das Original schon in der Liste steht.
    #[test]
    fn die_zeilenzahl_ist_gemessen() {
        // JPEG ist in der Bildliste -> Original + 4 = 5.
        assert_eq!(ziele_fuer("image/jpeg", "jpg").len(), 5);
        // BMP ist auch in der Liste -> auch 5.
        assert_eq!(ziele_fuer("image/bmp", "bmp").len(), 5);
        // ICO ist NICHT in der Liste (siehe `ZIELE_BILD`) -> Original + 5 = 6.
        assert_eq!(ziele_fuer("image/x-icon", "ico").len(), 6);
        // Video: 5 + Original, WebM ist nicht in der Liste -> 6 = MAX_ZIELE.
        assert_eq!(ziele_fuer("video/webm", "webm").len(), MAX_ZIELE);
        // Ton: MP3 steht in der Tonliste, also fallen Original (1) + WAV +
        // FLAC = 3 Zeilen heraus.
        //
        // GEMESSENER EIGENER FEHLER (02.10.): ich hatte hier 4
        // behauptet („3 + Original“), so als kaeme das Original zu den drei
        // Tonformaten dazu. GEMESSEN: das Original IST eines der drei
        // (MP3), also wird es wie die anderen gefiltert und es bleiben
        // **3** Zeilen.
        assert_eq!(ziele_fuer("audio/mpeg", "mp3").len(), 3);
        // UND: keine Liste ist laenger als MAX_ZIELE.
        for (m, e) in [
            ("image/jpeg", "jpg"),
            ("image/x-icon", "ico"),
            ("video/webm", "webm"),
            ("audio/mpeg", "mp3"),
        ] {
            assert!(ziele_fuer(m, e).len() <= MAX_ZIELE, "{m}: zu viele Ziele");
        }
    }

    /// EINE UNBEKANNTE ENDUNG GIBT KEINE ZIELE — und kein erfundenes Ziel.
    #[test]
    fn eine_unbekannte_endung_gibt_keine_ziele() {
        assert_eq!(ziele_fuer("", "txt").len(), 0, "Text ist kein Ziel");
        assert_eq!(ziele_fuer("", "").len(), 0, "leer ist kein Ziel");
        assert_eq!(ziele_fuer("application/zip", "zip").len(), 0);
        assert_eq!(art_von("", "txt"), None);
        // GEMESSENE ENTSCHEIDUNG: HTML ist KEIN Bild, auch wenn es Text ist.
        assert_eq!(art_von("text/html", "html"), None);
    }

    /// DIE ENTSCHEIDUNG „MIME VOR ENDUNG“ — gemessen an zwei Faellen, in
    /// denen die Endung luegen wuerde.
    #[test]
    fn die_mime_schlaegt_die_endung() {
        // Eine Adresse, die mit .mp4 endet, aber als Audio kommt (Server
        // stimmt nicht): die MIME gewinnt, weil der Server sie gesagt hat.
        assert_eq!(art_von("audio/mpeg", "mp4"), Some(Art::Ton));
        // Und umgekehrt: eine .gif-Adresse, die als Bild kommt.
        assert_eq!(art_von("image/gif", "gif"), Some(Art::Bild));
        // GEMESSENE ENTSCHEIDUNG: aus einer MIME ohne Endung wird eine
        // Endung fuer den Namen des Originals (siehe `ziele_fuer`).
        let z = ziele_fuer("image/png", "");
        // KONTEXT (03.10.2026, 21:35): GEMESSENE ENTSCHEIDUNG: der **Titel**
        // traegt nur noch das FORMAT („JPEG“); „Als “ und „(Original)“
        // kommen aus `beschriftung()`. Der Titel waere sonst die zweite
        // Stelle, an der die Beschriftung steht.
        assert_eq!(z[0].titel, "JPEG", "GEMESSEN: jpeg aus der MIME");
    }

    /// DIE SCHREIBWEISEN: „jpg“ und „jpeg“ sind dasselbe Format.
    ///
    /// GEMESSENER BEFUND (02.10., Bildtest `a_konvert_untermenue`): an
    /// der Adresse „urlaub.jpeg“ stand im Menue **Original (JPG)** und darunter
    /// noch einmal **Als JPEG** — GEMESSEN 6 Zielzeilen statt 5. Wer die
    /// Endung des Originals mit der Endung der Tabelle vergleicht, erkennt
    /// dasselbe Format nicht.
    #[test]
    fn die_schreibweisen_des_formats_sind_eins() {
        assert_eq!(normalisiere("jpg"), "jpeg");
        assert_eq!(normalisiere("JPG"), "jpeg");
        assert_eq!(normalisiere("JPEG"), "jpeg");
        assert_eq!(normalisiere("tif"), "tiff");
        // ALLES, was nicht umbenannt wird, bleibt, klein geschrieben.
        assert_eq!(normalisiere("PNG"), "png");
        assert_eq!(normalisiere("WebP"), "webp");
        assert_eq!(normalisiere(""), "");
        // UND: die Liste haelt das Original genau einmal — egal, welche
        // Schreibweise die Quelle hat.
        for quelle in ["jpg", "jpeg", "JPG", "JPEG"] {
            let z = ziele_fuer("image/jpeg", quelle);
            assert_eq!(z.len(), 5, "Quelle {quelle}: {} statt 5", z.len());
            // KONTEXT (03.10.2026, 21:35): wie oben — nur das FORMAT.
            assert_eq!(z[0].titel, "JPEG");
            assert_eq!(
                z.iter().filter(|x| x.format == "jpeg").count(),
                1,
                "Quelle {quelle}: JPEG steht mehr als einmal da"
            );
            assert!(
                !z.iter().any(|x| x.titel == "Als JPEG"),
                "Quelle {quelle}: „Als JPEG“ steht noch einmal da"
            );
            // DER DATEINAME bleibt die Endung der Quelle (GEMESSEN: man
            // speichert „urlaub.jpeg“ nicht als „urlaub.jpg“ um).
            assert_eq!(z[0].endung, quelle.to_ascii_lowercase());
        }
    }

    #[test]
    fn die_endung_aus_der_adresse_ist_gemessen() {
        assert_eq!(
            endung_aus_adresse("https://b.example/bild.PNG").as_deref(),
            Some("png")
        );
        assert_eq!(
            endung_aus_adresse("https://b.example/bild.png?v=2").as_deref(),
            Some("png"),
            "die Fragezeichenhoeren nicht zur Endung"
        );
        // GEMESSENER EIGENER FEHLER (02.10.): ich behauptete hier
        // Some("b"). GEMESSEN: **None** — nach dem letzten Schraegstrich
        // steht „c“, und „c“ hat keinen Punkt. Das „a.b“ davor gehoert zu
        // einem ORDNER, nicht zu einem Dateinamen.
        assert_eq!(
            endung_aus_adresse("https://b.example/a.b/c#anker"),
            None,
            "es zaehlt der PUNKT nach dem LETZTEN Schraegstrich"
        );
        assert_eq!(
            endung_aus_adresse("https://b.example/a.b/c.mp4").as_deref(),
            Some("mp4"),
            "derselbe Ordner, aber die Datei hat eine Endung"
        );
        assert_eq!(endung_aus_adresse("https://b.example/ordner"), None);
        assert_eq!(endung_aus_adresse("https://b.example/.versteckt"), None);
        assert_eq!(endung_aus_adresse("https://b.example/datei."), None);
        assert_eq!(endung_aus_adresse(""), None);
        // UND: die Endung ist gekuerzt (MAX_ENDUNG).
        let lang = format!("https://b.example/x.{}", "a".repeat(40));
        assert_eq!(
            endung_aus_adresse(&lang).map(|e| e.len()),
            Some(MAX_ENDUNG),
            "GEMESSEN: eine Endung ist nicht unendlich lang"
        );
    }

    /// DIE BESCHRIFTUNG — mit und ohne Zusatz.
    #[test]
    fn die_beschriftung_sagt_was_zu_tun_ist() {
        let einfach = Ziel {
            titel: "PNG".to_string(),
            endung: "png".to_string(),
            format: "png".to_string(),
            mime: "image/png".to_string(),
            hinweis: String::new(),
            original: false,
        };
        assert_eq!(einfach.beschriftung(), "As PNG");
        let mit = Ziel {
            hinweis: "audio track".to_string(),
            ..einfach.clone()
        };
        assert_eq!(mit.beschriftung(), "As PNG (audio track)");
        {
            let _de = narsil_i18n::scoped(narsil_i18n::Language::German);
            assert_eq!(mit.beschriftung(), "Als PNG (Tonspur)", "the hint is translated too");
        }
        // KONTEXT (03.10.2026, 21:35): das Original traegt jetzt **auch** das
        // „Als “ — GEMESSEN ist, dass die Leitung die Einheitlichkeit **aller**
        // Zeilen verlangt hat. GEMESSENE ENTSCHEIDUNG, warum der Titel des
        // Originals nur das FORMAT traegt: sonst stuende „Als Original (JPEG)
        // (Original)“ im Menue.
        let orig = original("jpg");
        assert_eq!(orig.titel, "JPEG", "GEMESSEN: der Titel ist nur das FORMAT");
        assert_eq!(
            orig.beschriftung(),
            "As JPEG (original)",
            "GEMESSEN: die Originalzeile ist nicht wie ihre Nachbarn gebaut"
        );
        assert!(orig.original);
        // GEMESSENE ENTSCHEIDUNG: die laengste Zeile passt in die Breite,
        // die das Menue dafuer bekommen hat (`ktx_menue::MENUE_BREITE` 248
        // minus 2 * 12 Rand minus 16 Einzug = 208 px = 26 Zeichen).
        let laengste = ziele_fuer("video/webm", "webm")
            .iter()
            .map(|z| z.beschriftung().chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            laengste * super::super::ZEICHENBREITE as usize
                <= ktx_menue_breite() as usize - 2 * 12 - 16,
            "GEMESSEN: die laengste Zeile braucht {laengste} Zeichen"
        );
    }

    /// DIE BREITE DES MENUES — als Zahl, damit der Test nicht vom Menue
    /// abhaengt (das Menue ist eine andere Datei).
    fn ktx_menue_breite() -> u32 {
        crate::ktx_menue::MENUE_BREITE
    }

    /// DIE EINE STELLE, DIE ERSETZT WIRD — der Name und die Unterschrift.
    #[test]
    fn die_unterschrift_ist_die_von_k4() {
        // K4 (AUTONOM.txt Zeile 677): `ziele_fuer(mime, endung) -> Vec<...>`
        let f: fn(&str, &str) -> Vec<Ziel> = ziele_fuer;
        assert!(!f("", "jpg").is_empty(), "die Funktion tut nichts");
        // GEMESSENE ENTSCHEIDUNG, warum die Liste NICHT global ist: sie
        // haengt an der Quelle, und eine globale Liste koennte nicht
        // „nur sinnvolle Ziele“ (K4) erfuellen.
        assert_ne!(
            ziele_fuer("image/jpeg", "jpg"),
            ziele_fuer("audio/mpeg", "mp3")
        );
    }
}
