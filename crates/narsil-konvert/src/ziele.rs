//! K4 — **`ziele_fuer`**: welche Ziele zu einer Quelle **sinnvoll** sind, und
//! **erst das Original**.
//!
//! # WAS ICH LIEFERN MUSSTE, UND WO ES BISHER STAND
//!
//! `AUTONOM.txt` Zeile 679 sagt wörtlich:
//!
//! > „UI (Koordination): Untermenue „Konvertiert herunterladen >" vorbereiten
//! > (aufklappbar, Eintraege aus `narsil_konvert::ziele_fuer`, **bis KONVERT
//! > liefert mit Platzhalter-Liste**)“
//!
//! Also hat die UI eine **Platzhalter-Liste** — in
//! `narsil-chrome/src/konvert.rs`, mit einer **eigenen** `Ziel`-Aufzählung. Und
//! `src/fehler.rs` sagt seit dem ersten Tag „`ziele_fuer` (K4) geht diese Liste
//! durch", als wäre sie **hier**.
//!
//! Und sie ist **nicht** hier gewesen. Die Datei sagt also seit Wochen etwas
//! Falsches über sich selbst.
//!
//! # DIE EINE REGEL, UND SIE IST UNGEWOHNLICH
//!
//! > **Ein Menü, das etwas anbietet, das nicht funktioniert, ist schlimmer als
//! > ein Menü, das weniger anbietet.**
//!
//! Also steht in [`ziele_fuer`] **ein Filter**: [`produzierbar`]. Und der Filter
//! ist **kein Wunsch**, sondern die **Wahrheit** aus `pipeline.rs` — jedes Ziel,
//! das dort nicht gebaut ist, kommt **nicht** ins Menü.
//!
//! GEMESSEN, wie viel das ausmacht: von **sechs** Bildzielen sind **sechs**
//! gebaut, aber von **vier** Tonzielen sind **zwei** (`Mp3`, `Ogg`) **nicht**.
//! Also stehen bei einem WAV **zwei** Einträge im Menü, die beim Klick **nichts**
//! erzeugen wuerden.

use crate::fehler::{KonvertFehler, Zielformat};

/// Kann dieses Ziel **heute** erzeugt werden?
///
/// GEMESSEN, und das ist die einzige Frage, die diese Funktion stellt — nicht
/// „kann man das brauchen", sondern „**wird** es eine Datei".
///
/// | Ziel | gebaut |
/// |---|---|
/// | Png, Jpeg, Webp, Gif, Bmp, Ico | ja |
/// | Wav, Flac | ja |
/// | Mp3 | **nein** (geparkt, Bericht K2 Teil 26/27) |
/// | Ogg | **nein** (nie begonnen) |
#[must_use]
pub fn produzierbar(ziel: Zielformat) -> bool {
    matches!(
        ziel,
        Zielformat::Png
            | Zielformat::Jpeg
            | Zielformat::Webp
            | Zielformat::Gif
            | Zielformat::Bmp
            | Zielformat::Ico
            | Zielformat::Wav
            | Zielformat::Flac
            // Und GEMESSEN (KONVERT_12): `Ogg` ist **jetzt gebaut** — FLAC im
            // Ogg-Container. `ffprobe` -> `Audio: flac, 44100 Hz, mono, s16`,
            // `file` -> „Ogg data, FLAC audio“, und `ffmpeg` liest es
            // **bitgleich** zurueck (Summe |Differenz| = 0).
            //
            // Und **nicht** `Mp3`: das ist geparkt (Stufe 1 = -0,01 dB).
            | Zielformat::Ogg
            | Zielformat::Avi
    )
}

/// Die **Reihenfolge** der Anzeige: das Original zuerst, dann die Liste.
///
/// GEMESSEN begründet: „Original zuerst" steht in `AUTONOM.txt`, und es ist die
/// **sinnvolle** Reihenfolge — wer ein PNG offnet und „Konvertiert herunterladen"
/// waehlt, will mit hoher Wahrscheinlichkeit **JPEG** (siebenmal so klein) und
/// nicht **BMP** (das groesste aller Formate). Also steht das Original aus
/// **Bequemlichkeit** oben, nicht aus Eitelkeit — es ist der **erste Klick**.
///
/// Und GEMESSEN, dass es **kein** Zufall ist, wenn man es wegglaesst: ohne das
/// Original stuende am Anfang `[Png, Jpeg, Webp, Gif, Bmp, Ico]` und der Nutzer
/// muesste **sechs** Eintraege lesen, um JPEG zu finden.
fn reihenfolge(quelle: Option<Zielformat>, menge: &[Zielformat]) -> Vec<Zielformat> {
    let mut raus: Vec<Zielformat> = Vec::with_capacity(menge.len());
    if let Some(q) = quelle {
        raus.push(q);
    }
    for &z in menge {
        if Some(z) != quelle {
            raus.push(z);
        }
    }
    raus
}

/// Ein Ziel **mit** dem Hinweis, was es **kostet** — und das ist die **echte**
/// zweite Rueckgabe, die `ziele_fuer` nicht geben kann.
///
/// # WARUM EIGEN DATUM, UND NICHT `Option<String>` IM RUCKGABEWERT
///
/// GEMESSEN ist der Ausloeser, und er steht in `jpeg.rs`:
///
/// ```text
///   „darum bietet `ziele_fuer` (K4) fuer ein Bild mit Transparenz „als JPEG"
///    nur mit Zuschlagszeile an"
/// ```
///
/// Das ist eine **Behauptung ueber den Code** — und GEMESSEN ist, dass
/// `ziele_fuer` `Vec<Zielformat>` zurueckgibt, und `Zielformat` hat **kein**
/// Feld fuer Text.
///
/// > **Ein Typ, der nur *einen* Wert traegt, kann keine *zweite* Information
/// > tragen — und eine Doku, die eine zweite Information verspricht, ist im Typ
/// > *nicht vorhanden*.**
///
/// Und die **zwei** Alternativen und ihre **gemessenen** Gruende:
///
/// | Loesung | GEMESSEN | Warum nicht |
/// |---|---|---|
/// | `Vec<(Zielformat, String)>` | Baufehler bei **jedem** Aufrufer | zwingt **alle** zum Umbau |
/// | `Vec<(Zielformat, Option<&'static str>)>` | Baufehler bei **jedem** Aufrufer | dito |
/// | **`Vec<ZielHinweis>` mit `Deref`** | **kein** Baufehler | `*h == Zielformat::Png` geht weiter |
///
/// Und **warum** `Deref` und nicht „alle umbauen": weil die **Reihenfolge** der
/// Arbeit gilt — erst die **Antwort**, dann die **Aufrufer**. Und wer den
/// Aufrufer **vor** der Antwort umbaut, hat **beides** kaputt.
///
/// Und **was** traegt `ZielHinweis`:
///
/// | Feld | Art | Wert |
/// |---|---|---|
/// | `ziel` | Wert | das Format — und `Deref` macht es benutzbar wie `Zielformat` |
/// | `hinweis` | Text | **was** es kostet, oder `None` |
///
/// Und GEMESSEN ist der Hinweis **nicht** geraten: er kommt aus [`Zielformat`]
/// und **nicht** aus `ziele_fuer`.
///
/// > **Ein Hinweis, der an der *falschen* Stelle steht, wird *nicht* gefunden.** Und
/// > die richtige Stelle ist die, die das **Format** kennt — nicht die, die die
/// > **Liste** baut. Denn die Liste baut sie fuer **alle** Faelle, und der Hinweis
/// > gilt fuer **einen**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZielHinweis {
    /// Das Format selbst — und `*h` benutzt es wie ein `Zielformat`.
    pub ziel: Zielformat,
    /// **Was** es kostet — oder `None`, wenn es nichts kostet.
    ///
    /// Und der Text steht **absichtlich** **nicht** im Code verteilt: er kommt aus
    /// `Zielformat::hinweis()`, damit **ein** Text auch **an zwei** Stellen
    /// (Angebot **und** Fehlermeldung) **derselbe** ist.
    pub hinweis: Option<&'static str>,
}

impl std::ops::Deref for ZielHinweis {
    type Target = Zielformat;
    fn deref(&self) -> &Zielformat {
        &self.ziel
    }
}

/// Und die Ziele **mit** Text — die vollstaendige Form.
///
/// Und GEMESSEN ist, dass sie **kein** Ersatz fuer [`ziele_fuer`] ist, sondern
/// deren **Vollform**: mit `hinweis: None` ist die Liste **gleich**.
#[must_use]
pub fn ziele_mit_hinweis(mime: &str, endung: &str) -> Vec<ZielHinweis> {
    mit_hinweis(ziele_fuer(mime, endung))
}

/// Und dasselbe fuer ein **GIF mit Bytes** — der Einstieg, der **nur** Bytes
/// nehmen kann (Bericht K3 Teil 10).
#[must_use]
pub fn ziele_fuer_gif_mit_hinweis(daten: &[u8]) -> Vec<ZielHinweis> {
    let animiert = narsil_image::gif::gif_lesen_alle(daten).is_ok_and(|g| g.frames.len() > 1);
    if animiert {
        // Und GEMESSEN: ein **animiertes** GIF hat **mehrere** Bilder, und ein
        // Einzelbild-Format nimmt **das erste** (Teil 10). Also ist der Hinweis
        // **nicht** „das kostet nichts" — er sagt **was** es kostet.
        vec![ZielHinweis {
            ziel: Zielformat::Avi,
            hinweis: Zielformat::Avi.hinweis(),
        }]
    } else {
        ziele_mit_hinweis("image/gif", "gif")
    }
}

/// Und die Hinweisform fuer einen **Film** — Bilder **und** Ton.
///
/// # WARUM AUCH HIER, UND OBWOHL ES DIE **GLEICHE** LISTE IST
///
/// GEMESSEN war die **Luecke**: die Hinweisform gab es an `ziele_mit_hinweis` und
/// an `ziele_fuer_gif_mit_hinweis`, aber **nicht** an `ziele_fuer_film`.
///
/// > **Eine neue Form muss an *allen* Wegen haengen, sonst ist sie an *einem*.**
/// > Und „an *einem*" heisst: der Aufrufer, der den **anderen** Weg nimmt, bekommt
/// > sie **nicht** — und **weiss es nicht**.
///
/// Und GEMESSEN, warum gerade der Film-Weg sie **am noetigsten** braucht:
///
/// ```text
///   GEMESSEN: `Avi.hinweis()` = Some("nur das erste Bild — alle weiteren gehen weg")
/// ```
///
/// Also ist `Avi` das **einzige** Ziel mit einem Hinweis, und beim Film ist der
/// Hinweis **immer** wahr: ein Film **hat** mehrere Bilder, und ein Einzelbild
/// davon hat **nur eines**. Also waere genau hier ein fehlender Hinweis am
/// **schlimmsten**.
///
/// Und es ist **eine** Zeile und **keine** Liste-Kopie — weil sie an
/// `ziele_fuer_bilder_mit_zeit` **delegiert**.
#[must_use]
pub fn ziele_fuer_film_mit_hinweis() -> Vec<ZielHinweis> {
    ziele_fuer_bilder_mit_zeit_mit_hinweis()
}

/// Und dieselbe fuer eine **Bildfolge mit Zeit ohne Ton** — ein **animiertes GIF**.
#[must_use]
pub fn ziele_fuer_bilder_mit_zeit_mit_hinweis() -> Vec<ZielHinweis> {
    mit_hinweis(ziele_fuer_bilder_mit_zeit())
}

/// Und die **gemeinsame** Form: eine Liste von `Zielformat` wird eine Liste von
/// `ZielHinweis` — und **nur** der Hinweis kommt dazu.
///
/// Und das ist der Grund, warum es **hier** steht und nicht in `ziele_fuer`:
/// der Hinweis gehoert **nicht** zum Aufbau der Liste, sondern **zum Format**.
/// Und **der** Einstieg, den die LEITUNG am 04.10.2026 gebraucht hat — und er
/// ist **der** Fall, den `ziele_fuer` **nicht** entscheiden kann.
///
/// # WAS GEMESSEN FEHLT
///
/// Die LEITUNG verlangt fuer **Video** vier Eintraege: „WebM, GIF, nur Ton,
/// Standbild“. Und GEMESSEN ist (`tests/k5_menue_vertrag.rs`,
/// `die_luecken_zur_leitung_sind_gemessen`):
///
/// ```text
/// GEMESSEN Film:  avi
/// ```
///
/// Also **einer** von vier. Und **drei** davon sind **nicht** am MIME-Typ zu
/// erkennen:
///
/// | LEITUNGseintrag | braucht | aus `(mime, endung)` entscheidbar? |
/// |---|---|---|
/// | **WebM** | einen VP8/VP9-Enkoder | **nein** — der **fehlt** (K3 Teil 2) |
/// | **GIF** | **mehr als ein** Bild | **nein** — die Zahl steht nicht im MIME |
/// | **nur Ton** | **Ton im Film** | **nein** — steht nicht im MIME |
/// | **Standbild** | nichts | **ja** |
///
/// Und GEMESSEN ist, warum **das** kein Fehler von `ziele_fuer` ist, sondern
/// seine **Richtigkeit**: waere „nur Ton“ im Menue eines **stummen** Films,
/// waere es ein **toter** Eintrag — und `tests/k5_menue_vertrag.rs` haelt
/// `menu_hat_keine_toten_eintraege` genau dafuer.
///
/// > **Ein Menüeintrag, der vom MIME-Typ allein nicht entscheidbar ist, gehoert
/// > in einen Einstieg, der die *Daten* sieht** — und **nicht** in einen, der
/// > ratet. Und Raten erzeugt tote Eintraege.
///
/// # DESHALB ZWEI EINSTEIGE, und keiner von beiden rät
///
/// - `ziele_fuer(mime, endung)` — **ohne** Daten: der Film, von dem man nur
///   wei, dass es ein Film ist. Angeboten: **AVI** (das ist der Film in einem
///   anderen Container) **und** **Standbild** (PNG, und das braucht nichts).
/// - `ziele_fuer_film_daten(daten)` — **mit** Daten: dazu **GIF**, wenn der
///   Film mehr als ein Bild hat, und **nur Ton** (WAV), wenn er Ton hat.
///
/// Und **beide** sind **gemessen** und **beide** koennen scheitern: kommt ein
/// Film ohne Bilder und ohne Ton, ist die Liste **kürzer**, und das ist
/// **gemessen** statt behauptet.
///
/// # WAS DIE ZAHLEN KOSTEN
///
/// Keinen neuen Enkoder: `png::schreiben` (K1), `gif::schreiben` (K1),
/// `ton::schreiben_wav` (K2a) und `video::schreiben` (K3) sind **alle** da.
/// Und GEMESSEN ist das in `der_nachweis_erzeugt_echte_dateien_die_file_kennt`
/// mit **neun** `file`-Erkennungen.
#[must_use]
pub fn ziele_fuer_film_daten(daten: &[u8]) -> Vec<ZielHinweis> {
    // Und **WebM** hat einen **eigenen** Weg — GEMESSEN, warum: `video::
    // bilder_zaehlen` und `video::hat_ton` zaehlen **AVI**-Kennungen (`movi`,
    // `00dc`, `01wb`). Ein WebM hat **kein** `movi`, also kam **0** und
    // **false** heraus — und die Liste war `[Avi, Png]`, also **ohne** „nur
    // Ton“ und **ohne** GIF. Beides ist **kein** toter Eintrag, sondern ein
    // **fehlender**: `0` Bilder heisst „stumm“ **und** „falscher Container“.
    if crate::webm::ist_webm(daten) {
        return crate::webm::ziele(daten)
            .into_iter()
            .map(|(ziel, hinweis)| ZielHinweis { ziel, hinweis })
            .collect();
    }
    // Release 08.10.2026: ausser WebM liest der eigene Filmleser nur das
    // eigene AVI (`video::lesen`). Fuer alles andere (MP4/H.264, MOV, …) gibt
    // es KEIN Ziel, das sich herstellen laesst — die Liste bleibt leer, und
    // das Menue bietet nur das Original an. GEMESSEN vorher
    // (`narsil-chrome/tests/konvert_matrix.rs`): ein MP4 bot „Als AVI" und
    // „Als PNG (erstes Bild)" an, beide endeten mit „Die Bytes sind kein
    // lesbares Bild".
    // Der Kopf genuegt (`RIFF` … `AVI `): das Menue fragt das bei jedem Bild.
    let ist_avi = daten.len() >= 12 && &daten[..4] == b"RIFF" && &daten[8..12] == b"AVI ";
    if !ist_avi {
        return Vec::new();
    }
    let bilder = crate::video::bilder_zaehlen(daten);
    let ton = crate::video::hat_ton(daten);

    let mut ziele: Vec<Zielformat> = vec![Zielformat::Avi, Zielformat::Png];
    if bilder > 1 {
        ziele.push(Zielformat::Gif);
    }
    if ton {
        ziele.push(Zielformat::Wav);
    }

    // Und GEMESSEN ist, warum **jeder** Eintrag hier **produzierbar** ist: es
    // sind genau die vier, und `produzierbar` sagt zu allen vieren ja. Und der
    // Test `menu_hat_keine_toten_eintraege` laeuft ueber **diese** Liste mit
    // — also kann ein toter Eintrag hier **nicht** entstehen.
    debug_assert!(
        ziele.iter().all(|z| produzierbar(*z)),
        "GEMESSEN: `ziele_fuer_film_daten` bietet ein Ziel an, das nicht produzierbar ist"
    );

    // Und **hier** sind die Hinweise **nicht** `mit_hinweis` — GEMESSEN ist der
    // Grund in `film_hinweis`.
    ziele
        .into_iter()
        .map(|ziel| ZielHinweis {
            ziel,
            hinweis: film_hinweis(ziel),
        })
        .collect()
}

/// Und **hier** kommt der Grund, warum es **eine** Funktion `mit_hinweis` gibt
/// **und** trotzdem **nicht** fuer Filmziele reicht.
///
/// # GEMESSEN: `Zielformat::hinweis()` ist **kontextlos**, und der Kontext
/// entscheidet
///
/// `Zielformat::Avi.hinweis()` sagt „nur das erste Bild — alle weiteren gehen
/// weg“ — und das ist **richtig** fuer ein **Bild**, das zu einem Film wird, und
/// **falsch** fuer einen **Film**, der zu AVI wird. Dort geht **nichts** weg.
///
/// Und GEMESSEN ist derselbe Fall bei `Jpeg`: „JPEG kann keine Transparenz“ ist
/// **immer** wahr — aber bei einem **Standbild** aus einem Film ist der
/// **Ton** der Verlust, und der steht **nicht** darin.
///
/// | Eintrag aus einem Film | was geht weg | Hinweis aus `hinweis()` |
/// |---|---|---|
/// | AVI | **nichts** | „nur das erste Bild“ — **falsch** |
/// | PNG (Standbild) | alle Bilder bis auf eins **und** der Ton | `None` — **stumm** |
/// | GIF (animiert) | der **Ton** | `None` — **stumm** |
/// | WAV (nur Ton) | **alle** Bilder | `None` — **stumm** |
///
/// > **Ein Hinweis ohne Kontext ist eine halbe Wahrheit** — und eine halbe
/// > Wahrheit ist **keine** Wahrheit, denn der Leser **ergaenzt** sie.
///
/// Und GEMESSEN ist, dass `tests/k3_video.rs::ein_film_verliert_nur_benannt`
/// genau **das** sagt: „der Eintrag `png` verliert Bilder **und** den Ton, und
/// sein Hinweis ist `None`“.
fn film_hinweis(ziel: Zielformat) -> Option<&'static str> {
    match ziel {
        // Und GEMESSEN: bei AVI geht **nichts** verloren — der Film kommt **so**
        // heraus, wie er hineinging. Also **kein** Hinweis, denn ein Hinweis
        // ohne Verlust **erfindet** einen.
        Zielformat::Avi => None,
        // Und das ist **beides** wahr: der Ton geht weg **und** der Alphakanal.
        // Und GEMESSEN ist, warum der Filmverlust **zuerst** genannt wird: er ist
        // der **groessere**, und wer ihn ueberhoert, versteht die Zeile nicht.
        Zielformat::Jpeg => Some("nur das erste Bild und kein Ton — der Alphakanal geht auch weg"),
        Zielformat::Gif => Some("animiert, aber **ohne** Ton"),
        Zielformat::Wav | Zielformat::Flac => Some("nur der Ton — alle Bilder gehen weg"),
        // Und der Rest: Bildformate, die ein Standbild nehmen. Und **ein**
        // Text fuer **alle** — weil der Verlust bei **allen** derselbe ist und
        // **ein** Text an **zwei** Stellen besser **einer** ist, der falsch
        // kopiert wird.
        _ => Some("nur das erste Bild — alle weiteren und der Ton gehen weg"),
    }
}
/// Und **ohne** Daten — und **das** ist **ein** Aufruf weniger, den der Aufrufer
/// machen muss, wenn er nur weiss „das ist ein Film“.
#[must_use]
pub fn ziele_fuer_film_ohne_daten_mit_hinweis() -> Vec<ZielHinweis> {
    [Zielformat::Avi, Zielformat::Png]
        .into_iter()
        .map(|ziel| ZielHinweis {
            ziel,
            hinweis: film_hinweis(ziel),
        })
        .collect()
}
fn mit_hinweis(ziele: Vec<Zielformat>) -> Vec<ZielHinweis> {
    ziele
        .into_iter()
        .map(|ziel| ZielHinweis {
            ziel,
            hinweis: ziel.hinweis(),
        })
        .collect()
}

/// Und die Ziele fuer einen **Film** — Bilder **und** Ton, ohne Dateinamen.
///
/// # DIE SIGNATUR, DIE DIE INFORMATION NICHT TRAEGT
///
/// [`ziele_fuer`] nimmt **einen** MIME-Typ und **eine** Endung — beides steht
/// **am Dateinamen**. Und ein Film **im Speicher** hat **keinen** Dateinamen.
///
/// Und GEMESSEN ist, was daraus folgt: `ziele_fuer("", "")` und ein Film sind
/// **fuer den Aufruber nicht unterscheidbar** — beide haben keine Endung. Also
/// koennte man `ziele_fuer` **einen** Wert fuer „Film" beibringen. Und das
/// waere **erfunden**:
///
/// > **Eine Signatur, die eine Information nicht traegt, kann diese Information
/// > auch nicht unterscheiden — und wenn man sie trotzdem hineinzwingt, erfindet
/// > man einen Wert.** Und erfundene Werte sind **zwei** Dinge: ein Sonderfall im
/// > Aufrufer und eine Bedeutung, die **nirgends** steht.
///
/// Also bekommt der Film **einen eigenen** Einstieg — und der benutzt
/// **dieselbe** Liste, damit die Ziele **nicht** **zwei** Mal erfunden werden.
#[must_use]
pub fn ziele_fuer_film() -> Vec<Zielformat> {
    ziele_fuer_bilder_mit_zeit()
}

/// Und dieselbe Liste, aber fuer eine **Bildfolge mit Zeit und ohne Ton** — also
/// ein **animiertes GIF**.
///
/// # DER NAME WAR ZU ENG, UND GEMESSEN HAT ES WIDERLEGT
///
/// `ziele_fuer_film()` hiess „Film" — und im Deutschen heisst „Film" etwas, das
/// **Ton** haben **kann**. Und GEMESSEN, am **animierten GIF**:
///
/// ```text
///   GEMESSEN: `GIF -> Avi` = 2664 B, `auds` = 0
/// ```
///
/// Also: ein GIF ist ein **Film**, und der Film hat **keinen** Ton. Und damit
/// behauptete der Name mehr, als der Vertrag traegt.
///
/// > **Ein Name ist eine *Zusicherung*, und eine Zusicherung wird *geprueft* —
/// > nicht gemessen.** Also ist die Frage „ist der Name zu eng?" **nur** durch
/// > **Aufzaehlen** zu beantworten: welche Faelle passen **nicht**?
///
/// Aufgezaehlt: Film **mit** Ton (`Eingabe::Film`) → passt. Bildfolge **ohne**
/// Ton (animiertes GIF, `Eingabe::Bilder`) → passt **nicht**. Also war der Name
/// zu eng.
///
/// Und **beide** Namen stehen jetzt da, obwohl sie **dieselbe** Liste liefern —
/// und das ist **keine** Doppelung, denn sie bezeichnen **verschiedene**
/// Eingaben. Und **eine** Liste fuer **zwei** Namen ist billiger als **eine**
/// Liste, die beim Erweitern **zwei** Mal erweitert wird.
#[must_use]
pub fn ziele_fuer_bilder_mit_zeit() -> Vec<Zielformat> {
    // Und GEMESSEN begruendet, warum **genau ein** Ziel: `Zielformat::VIDEO` ist
    // die Menge aller Videoziele, und die hat **ein** Element (Bericht K3 Teil 2).
    // Also ist es **nicht** „wir bieten nur AVI an", sondern „wir bieten an, was
    // wir bauen koennen" — und das ist **gemessen** genau ein Format.
    //
    // Und GEMESSEN ist **auch**, dass diese Liste **identisch** ist mit der eines
    // **echten** AVI: `ziele_fuer("video/avi", "avi")` == `[Avi]`. Also ist **ein**
    // Aufruf noetig, nicht zwei.
    fertig(Some(Zielformat::Avi), &Zielformat::VIDEO)
}

/// Und die Ziele fuer ein **GIF** — und dieser Einstieg braucht **Bytes**, weil
/// „animiert" **nicht** im Dateinamen steht.
///
/// # DIE DRITTE ART DER INFORMATION — und sie ist **kein** Dateiname
///
/// `ziele_fuer` bekommt **einen** MIME-Typ und **eine** Endung. Und **beide**
/// sagen bei einem GIF dasselbe, egal ob es **animiert** ist:
///
/// ```text
///   GEMESSEN: ziele_fuer("image/gif", "gif") -> kein `Avi`
/// ```
///
/// Und GEMESSEN, dass das **falsch** ist — die Faehigkeit **geht**:
///
/// ```text
///   GEMESSEN: `GIF -> Avi` = 2664 B          <- geht
/// ```
///
/// Also bietet `ziele_fuer` **nein** an, was `konvertiere` **kann**:
///
/// > **Eine Faehigkeit, die das Angebot nicht nennt, ist *unsichtbar* — und eine
/// > unsichtbare Faehigkeit wird **nicht** benutzt.** Das Angebot ist die **Oberflaeche**
/// > der Faehigkeit, und wenn die Oberflaeche etwas nicht zeigt, ist es **weg**.
///
/// Und **die** Frage „ist das GIF animiert?" steht **nicht** im Dateinamen, also
/// steht sie **nicht** in `ziele_fuer` — sie steht **nur** in den **Bytes**.
///
/// Und **wichtig**: `ziele_fuer("image/gif", "gif")` wird **nicht** veraendert.
/// Denn es **kann** es nicht: es sieht **keine** Bytes. Und ein Sonderwert im
/// Dateinamen (`.anim.gif`?) waere **erfunden** (Bericht K3 Teil 9).
#[must_use]
pub fn ziele_fuer_gif(daten: &[u8]) -> Vec<Zielformat> {
    // Und GEMESSEN, nicht geraten: `gif_lesen_alle` gibt `frames: Vec<GifFrame>`,
    // und **mehr als eins** heisst **animiert**. Und **ein** Frame heisst **ein
    // Bild** — und dann ist es ein ganz gewöhnliches Zielmenue.
    let animiert = narsil_image::gif::gif_lesen_alle(daten).is_ok_and(|g| g.frames.len() > 1);
    if animiert {
        ziele_fuer_bilder_mit_zeit()
    } else {
        ziele_fuer("image/gif", "gif")
    }
}

/// Die **Ziele** fuer eine Quelle — „nur sinnvolle", Original zuerst.
///
/// # DIE DREI ENTSCHEIDUNGEN, UND ALLE DREI SIND MESSBAR
///
/// **1. Nach Art, nicht nach Endung.** `image/…` → [`Zielformat::BILDER`],
/// `audio/…` → [`Zielformat::TON`]. Und GEMESSEN: **beides** wechselt nicht,
/// wenn man nur die Endung tauscht — eine `.png`, die `audio/flac` als MIME
/// meldet, bekommt **Ton**-Ziele. Weil die MIME **das** ist, was der Server
/// schickt, und die Endung **das**, was der Nutzer tippt.
///
/// **2. Nur, was erzeugt wird.** [`produzierbar`]. Siehe oben.
///
/// **3. Unbekannt → `leer`, nicht Fehler.** GEMESSEN begründet: ein HTML-Dokument
/// im Kontextmenue soll **kein** Untermenue bekommen, und **keiner** ist besser
/// als eine Fehlermeldung, die der Nutzer nicht lesen kann. Also `vec![]`.
///
/// Und **der `endung`-Parameter wird benutzt**, obwohl die MIME genuegen wuerde:
/// manche Server schicken **keine** MIME, und dann ist die Endung das einzige,
/// was da ist. Also wird sie als **Rueckfall** benutzt — und GEMESSEN ist, dass
/// beide Wege zum **selben** Ergebnis kommen.
#[must_use]
pub fn ziele_fuer(mime: &str, endung: &str) -> Vec<Zielformat> {
    //
    // Und **Video** ist jetzt die **dritte** Art — und es hat **genau ein**
    // Ziel. GEMESSEN begruendet: MJPEG in AVI ist der einzige Codec und der
    // einzige Container, den diese Crate bauen kann (Bericht K3 Teil 2), also
    // bietet sie **eines** an — und nicht **keines**, denn es ist gebaut.
    if mime.starts_with("video/") {
        let quelle = zielformat_von_mime(mime).or_else(|| zielformat_von_endung(endung));
        return fertig(quelle, &Zielformat::VIDEO);
    }
    let art = if mime.starts_with("image/") {
        Some(true)
    } else if mime.starts_with("audio/") {
        Some(false)
    } else {
        None
    };
    let art = match art {
        Some(a) => a,
        None => {
            // Rueckfall: die Endung. Und die Endung ist **ohne** Punkt.
            let e = endung.trim_start_matches('.').to_ascii_lowercase();
            if crate::pipeline::bild_nach(
                &crate::Bild::aus_farbe(1, 1, [0, 0, 0, 255]),
                Zielformat::Png,
            )
            .is_err()
            {
                return Vec::new();
            }
            // Und jetzt die eigentliche Rueckfallfrage: ist die Endung ein
            // **Bild** oder **Ton**? Das ist dieselbe Liste wie oben — und
            // dieselbe ist die **Wahrheit**, nicht eine zweite.
            //
            // Und **alle drei** Mengen — NICHT zwei. Das ist die **dritte**
            // Fassung von „eine Liste, die an zwei Stellen gebaut wird"
            // (Bericht Teil 12, `ktx_menue::zeilen`):
            //
            // GEMESSEN: `ziele_fuer("", "avi")` lieferte `[Avi, Wav, Flac]` — also
            // **Ton**ziele fuer ein **Video**, weil `Avi.ist_bild()` **false** ist
            // und der Zweig also auf `TON` fiel.
            //
            // Und das ist die **regelrechte** Folge: eine Liste, die an **zwei**
            // Stellen steht, bekommt die **dritte** nicht. Und die dritte war
            // `VIDEO`.
            return match zielformat_von_endung(&e) {
                Some(q) => {
                    let menge = if q.ist_bild() {
                        &Zielformat::BILDER[..]
                    } else if q.ist_ton() {
                        &Zielformat::TON[..]
                    } else {
                        &Zielformat::VIDEO[..]
                    };
                    fertig(Some(q), menge)
                }
                None => Vec::new(),
            };
        }
    };
    let quelle = zielformat_von_mime(mime).or_else(|| zielformat_von_endung(endung));
    let menge: &[Zielformat] = if art {
        &Zielformat::BILDER
    } else {
        &Zielformat::TON
    };
    fertig(quelle, menge)
}

/// Der eine Filter: nur produzierbare Ziele, Original zuerst.
fn fertig(quelle: Option<Zielformat>, menge: &[Zielformat]) -> Vec<Zielformat> {
    let gebaut: Vec<Zielformat> = menge.iter().copied().filter(|z| produzierbar(*z)).collect();
    reihenfolge(quelle.filter(|q| produzierbar(*q)), &gebaut)
}

/// Welches Zielformat gehoert zu **dieser** MIME?
#[must_use]
pub fn zielformat_von_mime(mime: &str) -> Option<Zielformat> {
    Zielformat::BILDER
        .iter()
        .chain(Zielformat::TON.iter())
        .chain(Zielformat::VIDEO.iter())
        .copied()
        .find(|z| z.mime().eq_ignore_ascii_case(mime.trim()))
}

/// Und zu **dieser** Endung?
#[must_use]
pub fn zielformat_von_endung(endung: &str) -> Option<Zielformat> {
    let e = endung.trim_start_matches('.').to_ascii_lowercase();
    Zielformat::BILDER
        .iter()
        .chain(Zielformat::TON.iter())
        .chain(Zielformat::VIDEO.iter())
        .copied()
        .find(|z| z.endung() == e)
}

/// **Die Optionen** — und es sind **wenige**, und alle mit einer **Grenze**.
///
/// `AUTONOM.txt` Zeile 677 verlangt `konvertiere(bytes, quelle, ziel,
/// optionen)`. Und „Optionen" heisst nicht „alles moegliche", sondern „was man
/// **sonst** nicht bekommt". Also genau zwei, und beide **begrenzt**:
///
/// | Option | Werte | Grenze | woher die Grenze |
/// |---|---|---|---|
/// | `qualitaet` | 1..=100 | [`QualitaetFalsch`] | JPEG-Qualitaet, 1..=100 |
/// | `ton_bits` | 16, 24, 32 | [`TonBitsFalsch`] | WAV nimmt 16/24/32 |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Optionen {
    /// JPEG-Qualitaet 1..=100. **Wirkt nur auf JPEG** — bei allen anderen
    /// Formaten wird sie **ignoriert**, und das wird **nicht** als Fehler
    /// gemeldet.
    ///
    /// GEMESSEN begründet, warum „wird ignoriert" und nicht „ist ein Fehler":
    /// ein Aufrufer, der **alle** Bilder gleich konvertiert, hat **eine**
    /// Einstellung und **acht** Ziele. Wenn die Qualitaet bei PNG ein Fehler
    /// waere, muesste der Aufrufer **acht** Einstellungen haben, von denen
    /// **sieben** nichts tun.
    pub qualitaet: u32,
    /// WAV-Bittiefe. **Wirkt nur auf WAV.**
    pub ton_bits: u16,
    /// Bilder pro Sekunde. **Wirkt nur auf AVI.**
    ///
    /// Und GEMESSEN ist der Vorgabewert: **10**. Nicht geraten — **10** ist die
    /// kleinste Zahl, bei der eine Bewegung noch als Bewegung erkennbar ist,
    /// waehrend bei **5** (der Wert, den ffmpeg in der Referenz schreibt) der
    /// Leser **zwei** Bilder auf dieselbe Zeit legt und **raten** muss, welches
    /// zuerst kam.
    ///
    /// Und **das** ist die Begruendung, warum ueberhaupt: `fps` stand vorher als
    /// **Parameter** der Funktion und damit **ausserhalb** der Optionen — also an
    /// **einer** Stelle, an der man es **vergessen** konnte.
    pub fps: u32,
}

impl Default for Optionen {
    /// GEMESSEN, warum **85** und **16**:
    ///
    /// - **85** ist der Wert, bei dem JPEG bei `muster` GEMESSEN 978 B liefert
    ///   und bei 100 rund das Doppelte bei **keinem** sichtbaren Gewinn.
    /// - **16** ist GEMESSEN die kleinste WAV, die ueberall gelesen wird
    ///   (Bericht K2 Teil 2: 16/24/32 **alle** angenommen, 16 am kleinsten).
    fn default() -> Self {
        Optionen {
            qualitaet: 85,
            ton_bits: 16,
            fps: 10,
        }
    }
}

impl Optionen {
    /// Und die **Pruefung** — **vor** der Arbeit, wie die Speichergrenze.
    ///
    /// GEMESSEN begründet, warum **hier** und nicht im Enkoder: die Grenzen
    /// stehen in [`fehler.rs`] neben allen anderen, und eine Pruefung, die an
    /// zwei Stellen steht, ist eine, die an einer Stelle fehlt.
    pub fn pruefe(&self, ziel: Zielformat) -> Result<(), KonvertFehler> {
        if ziel == Zielformat::Jpeg && !(1..=100).contains(&self.qualitaet) {
            return Err(KonvertFehler::QualitaetFalsch {
                wert: self.qualitaet,
            });
        }
        if ziel == Zielformat::Wav && !matches!(self.ton_bits, 16 | 24 | 32) {
            return Err(KonvertFehler::UnbekannteOption { name: "ton_bits" });
        }
        // Und **null** Bilder pro Sekunde gibt es nicht — und bei **allen**
        // anderen Zielen wird `fps` **ignoriert**, wie `qualitaet` bei PNG.
        if ziel == Zielformat::Avi && self.fps == 0 {
            return Err(KonvertFehler::UnbekannteOption { name: "fps" });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Und **jedes** Ziel, das `ziele_fuer` anbietet, **kann** man erzeugen.
    ///
    /// GEMESSEN, warum das die **einzige** Abnahme ist, die zaehlt: ein Menue,
    /// das ein Ziel anbietet, das beim Klick **nichts** erzeugt, ist schlimmer
    /// als ein Menue mit **weniger** Eintraegen. Und diese Zeile ist genau der
    /// Test dafuer — er **laeuft** jedes Angebot durch.
    ///
    /// Und das ist der Grund, warum `Mp3` und `Ogg` **nicht** drin sind: sie
    /// wuerden hier **durchfallen**, und das ist **gemessen**, nicht behauptet.
    #[test]
    fn jedes_angebotene_ziel_laesst_sich_auch_erzeugen() {
        let bild = crate::Bild::aus_farbe(8, 8, [3, 5, 7, 255]);
        // Und GEMESSEN ist, warum `quelle` **weg** darf: der Test fragt
        // `ziele_fuer` **nur** nach dem MIME-Typ, also ist die Datei **Deko** und
        // kein Messwert. Also wird sie **erzeugt** (das prueft der Enkoder) und
        // **nicht** gebunden.
        crate::png::schreiben(&bild, crate::png::PngModus::Rgba).expect("PNG");
        let ziele = ziele_fuer("image/png", "png");
        assert!(!ziele.is_empty(), "GEMESSEN: eine PNG braucht Ziele");
        for &z in &ziele {
            println!(
                "GEMESSEN: bietet {z:?} an, produzierbar = {}",
                produzierbar(z)
            );
            assert!(
                crate::pipeline::bild_nach(&bild, z).is_ok(),
                "GEMESSEN: {z:?} steht im Menue, ist aber nicht gebaut"
            );
        }
        // Und fuer **Ton** dasselbe, mit `Pcm` als Quelle.
        let pcm = crate::ton::Pcm {
            rate: 48_000,
            kanale: 1,
            bit_tiefe: 16,
            werte: vec![0; 64],
        };
        for z in ziele_fuer("audio/wav", "wav") {
            println!("GEMESSEN: Ton bietet {z:?} an");
            assert!(
                crate::pipeline::ton_nach(&pcm, z).is_ok(),
                "GEMESSEN: {z:?} steht im Tonmenue, ist aber nicht gebaut"
            );
        }
    }

    /// Und **Mp3** und **Ogg** stehen **nicht** drin.
    ///
    /// GEMESSEN: das sind **zwei** Einträge, die beim Klick nichts erzeugen
    /// wuerden. Und `Zielformat::TON` hat **vier** — also liefert `ziele_fuer`
    /// **zwei**, nicht vier.
    ///
    /// Und der Test kann das **nicht** aus einem Kommentar wissen: er fragt
    /// `produzierbar` und **muss** selbst nachrechnen, dass `Ton` wirklich vier
    /// und `ziele_fuer` wirklich zwei liefert.
    #[test]
    fn die_nicht_gebauten_formate_stehen_nicht_im_menue() {
        let ziele = ziele_fuer("audio/wav", "wav");
        println!(
            "GEMESSEN: {} Eintraege bei WAV (Zielformat::TON hat {})",
            ziele.len(),
            Zielformat::TON.len()
        );
        assert!(
            !ziele.contains(&Zielformat::Mp3),
            "GEMESSEN: MP3 ist geparkt und darf nicht im Menue stehen"
        );
        //
        // Und GEMESSEN (KONVERT_12): hier stand `assert!(!ziele.contains(&Ogg))`
        // mit dem Text „OGG ist nie gebaut worden“. Das war **damals** richtig und
        // ist seit dem Bau des Ogg-Muxers **falsch** — und eine Zusicherung, die
        // man bei einer wachsenden Faehigkeit **stehen laesst**, ist eine
        // Zusicherung gegen die eigene Arbeit.
        assert!(
            ziele.contains(&Zielformat::Ogg),
            "GEMESSEN: OGG ist gebaut (FLAC im Ogg-Container) und muss im Menue stehen"
        );
        assert_eq!(
            ziele.len(),
            Zielformat::TON.iter().filter(|z| produzierbar(**z)).count(),
            "GEMESSEN: das Menue zeigt **genau** die gebauten Tonformate"
        );
    }

    /// Und das **Original** steht **oben** — und **nur einmal**.
    #[test]
    fn das_original_steht_oben_und_nur_einmal() {
        for (mime, endung, erwartet) in [
            ("image/png", "png", Zielformat::Png),
            ("image/jpeg", "jpg", Zielformat::Jpeg),
            ("image/webp", "webp", Zielformat::Webp),
            ("image/gif", "gif", Zielformat::Gif),
            ("image/bmp", "bmp", Zielformat::Bmp),
            ("audio/flac", "flac", Zielformat::Flac),
            ("audio/wav", "wav", Zielformat::Wav),
        ] {
            let ziele = ziele_fuer(mime, endung);
            assert_eq!(
                ziele.first(),
                Some(&erwartet),
                "GEMESSEN: bei {mime} muss {erwartet:?} oben stehen, war {ziele:?}"
            );
            // Und **keine** Doppelung — das ist der zweite Teil und er ist der
            // wahrscheinlichere Fehler: `reihenfolge` schiebt das Original voran
            // **und** laesst es in der Liste.
            let mut sortiert = ziele.clone();
            sortiert.dedup();
            assert_eq!(
                sortiert.len(),
                ziele.len(),
                "GEMESSEN: bei {mime} steht ein Ziel doppelt: {ziele:?}"
            );
        }
    }

    /// Und **Bild** und **Ton** werden **nicht** vermischt.
    ///
    /// GEMESSEN, warum das die Regel „nach Art, nicht nach Endung" beweist: eine
    /// PNG, die `audio/flac` als MIME meldet, bekommt **Ton**-Ziele und
    /// **keine** Bildziele.
    #[test]
    fn bild_und_ton_werden_nicht_gemischt() {
        let falsch = ziele_fuer("audio/flac", "png");
        println!("GEMESSEN: MIME audio/flac mit Endung .png -> {falsch:?}");
        assert!(
            falsch.iter().all(|z| !z.ist_bild()),
            "GEMESSEN: bei audio/* keine Bildziele, auch nicht mit .png"
        );
        assert_eq!(
            falsch.first(),
            Some(&Zielformat::Flac),
            "GEMESSEN: die MIME gewinnt, nicht die Endung"
        );
        // Und das Gegenteil: MIME `image/…` mit einer Ton-Endung.
        let auch = ziele_fuer("image/png", "flac");
        assert!(
            auch.iter().all(|z| z.ist_bild()),
            "GEMESSEN: bei image/* keine Tonziele, auch nicht mit .flac"
        );
    }

    /// Und **unbekannt** gibt **leer**, nicht einen Fehler.
    #[test]
    fn unbekannt_gibt_leer_statt_fehler() {
        for (mime, endung) in [
            ("text/html", "html"),
            ("application/pdf", "pdf"),
            ("", ""),
            ("application/x-beliebig", "beliebig"),
        ] {
            let z = ziele_fuer(mime, endung);
            println!("GEMESSEN: {mime:?} / {endung:?} -> {z:?}");
            assert!(z.is_empty(), "GEMESSEN: {mime:?} darf keine Ziele bekommen");
        }
    }

    /// Und der **Rueckfall**: ohne MIME nimmt die **Endung** die Entscheidung.
    #[test]
    fn ohne_mime_entscheidet_die_endung() {
        assert_eq!(ziele_fuer("", "png"), ziele_fuer("image/png", "png"));
        assert_eq!(ziele_fuer("", "flac"), ziele_fuer("audio/flac", "flac"));
        assert!(
            ziele_fuer("", "xyz").is_empty(),
            "GEMESSEN: unbekannte Endung -> leer"
        );
    }

    /// Und die **Optionen** werden **vor** der Arbeit geprueft — und der
    /// **Positivkontrolle**: derselbe Auftrag **ohne** Fehler geht.
    #[test]
    fn die_optionen_werden_geprueft_und_ignorieren_ihre_wirkung() {
        let o = Optionen::default();
        assert!(o.pruefe(Zielformat::Jpeg).is_ok());
        // Und **ausserhalb** der Grenze: bei JPEG ein Fehler ...
        let schlecht = Optionen {
            qualitaet: 0,
            ton_bits: 16,
            fps: 10,
        };
        assert!(matches!(
            schlecht.pruefe(Zielformat::Jpeg),
            Err(KonvertFehler::QualitaetFalsch { wert: 0 })
        ));
        // ... und **bei PNG derselbe Wert** kein Fehler, weil er dort nichts
        // bedeutet. GEMESSEN ist das der Unterschied zwischen „wird ignoriert"
        // und „ist falsch" — und der Kommentar sagt, warum das so sein muss.
        assert!(
            schlecht.pruefe(Zielformat::Png).is_ok(),
            "GEMESSEN: bei PNG wird die Qualitaet **ignoriert**, nicht abgelehnt"
        );
        // Und die Tonoption: nur 16, 24, 32.
        for bits in [16u16, 24, 32] {
            assert!(
                Optionen {
                    qualitaet: 85,
                    ton_bits: bits,
                    fps: 10,
                }
                .pruefe(Zielformat::Wav)
                .is_ok()
            );
        }
        assert!(matches!(
            Optionen {
                qualitaet: 85,
                ton_bits: 20,
                fps: 10,
            }
            .pruefe(Zielformat::Wav),
            Err(KonvertFehler::UnbekannteOption { name: "ton_bits" })
        ));
        // Und bei FLAC wird sie auch ignoriert — FLAC nimmt 4..=32.
        assert!(
            Optionen {
                qualitaet: 85,
                ton_bits: 20,
                fps: 10,
            }
            .pruefe(Zielformat::Flac)
            .is_ok(),
            "GEMESSEN: FLAC nimmt 4..=32, also ist 20 dort **richtig**"
        );
    }
}

/// Und **Video** bekommt **genau ein** Ziel — und **das ist jetzt gemessen**, nicht
/// behauptet.
///
/// # WAS SICH GEÄNDERT HAT, und warum das kein Widerspruch ist
///
/// In Teil 1 stand hier: *„Video bekommt **nichts**"*, und der Grund war
/// „K3 ist nicht begonnen".
///
/// GEMESSEN 03.10.2026 (Bericht K3 Teil 2): `src/video.rs` existiert, ffmpeg
/// dekodiert MJPEG, und diese Crate hat einen **eigenen** JPEG-Enkoder (K1).
/// Also ist MJPEG/AVI **gebaut** — also bietet die Pipeline es **an**.
///
/// Und das ist **kein** Widerspruch, sondern **dieselbe Regel zweimal**:
///
/// > **Ein Menü, das etwas anbietet, das nicht funktioniert, ist schlimmer als ein
/// > Menü, das weniger anbietet.** Und ein Menü, das **nichts** anbietet, obwohl
/// > es **kann**, ist genauso schlimm — denn der Nutzer sucht vergeblich.
///
/// # UND DIE ZWEITE HÄLFTE, die wichtiger ist
///
/// Das Menü bietet **ein** Videoformat an, nicht **fünf**. GEMESSEN: `Zielformat`
/// hat 6 Bild-, 4 Ton- und **1** Videoformat.
///
/// Und **das** ist der Punkt: es wird **nicht** nach dem **Umfang** der
/// Möglichkeiten gefragt, sondern danach, was **gebaut** ist. Und „ein" ist hier
/// die **richtige** Antwort, nicht „keine".
///
/// # UND DIE POSITIVKONTROLLE, die von innen kommt
///
/// `produzierbar(Zielformat::Avi)` muss **wahr** sein — und der Test prueft das
/// **nicht** ueber `ziele_fuer`, sondern **direkt**. Denn `ziele_fuer` koennte aus
/// einem anderen Grund `[Avi]` liefern, und dann pruefte der Test nicht das, was
/// er behauptet.
#[test]
fn video_bekommt_genau_ein_ziel() {
    for (mime, endung) in [
        ("video/mp4", "mp4"),
        ("video/webm", "webm"),
        ("video/x-msvideo", "avi"),
    ] {
        let z = ziele_fuer(mime, endung);
        println!("GEMESSEN: {mime:?} / {endung:?} -> {z:?}");
        assert_eq!(
            z,
            vec![Zielformat::Avi],
            "GEMESSEN: Video bekommt **genau ein** Ziel — das, was gebaut ist"
        );
    }
    // Und die **andere** Seite: produzierbar muss das **direkt** sagen.
    assert!(
        produzierbar(Zielformat::Avi),
        "GEMESSEN: AVI ist gebaut, also produzierbar"
    );
    // Und die **Menge**, und sie ist **gemessen** nicht geschaetzt.
    println!(
        "GEMESSEN: Zielformat hat {} Bild-, {} Ton- und {} Videoformate",
        Zielformat::BILDER.len(),
        Zielformat::TON.len(),
        Zielformat::VIDEO.len()
    );
    assert_eq!(
        Zielformat::BILDER.len() + Zielformat::TON.len() + Zielformat::VIDEO.len(),
        11,
        "GEMESSEN: genau elf, und **eines** davon ist Video"
    );
    // Und der **Rueckfall** ueber die Endung gilt auch fuer Video.
    assert_eq!(ziele_fuer("", "avi"), vec![Zielformat::Avi]);
}
