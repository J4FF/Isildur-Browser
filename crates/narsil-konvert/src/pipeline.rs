//! K4 — **die Pipeline**: Bytes hinein, Bytes heraus, in einem anderen Format.
//!
//! # WAS HIER BISHER FEHLTE UND WAS FEHLT
//!
//! [`crate::lib`] sagt seit dem ersten Tag: „K4 baut die Pipeline in
//! [`konvertiere`]". Und diese Funktion gab es nicht — es gab **sieben**
//! Enkoder und **keinen** Aufruf, der sie benutzt. Das Untermenue „Konvertiert
//! herunterladen >" listete die Ziele, und `Zeile::Ziel(Ziel)` wurde in
//! `narsil-chrome` **nirgends** ausgewertet.
//!
//! Also: sieben Schrauben und kein Werkzeug.
//!
//! # DIE KETTE, UND WER SIE BAUT
//!
//! ```text
//!   Bytes -> [narsil-image: lesen]  ->  Bild  ->  [eigener Enkoder]  -> Bytes
//!   Pcm                              ->        [eigener Enkoder]  -> Bytes
//! ```
//!
//! Und GEMESSEN ist, dass beide Seiten **da** sind: `narsil_image::werkzeuge::lesen`
//! liest, und `png::schreiben`, `jpeg::schreiben`, `bmp::schreiben`,
//! `gif::schreiben`, `webp::schreiben_verlustfrei`, `ico::schreiben`,
//! `wav::schreiben` und `flac::schreiben` schreiben.
//!
//! # DIE ABNAHME, UND WAS AN IHR FEHLT
//!
//! Die geforderte Abnahme lautet: „im Fenster per Rechtsklick ein Bild als PNG
//! speichern, Datei mit `file` pruefen".
//!
//! Der **zweite** Teil ist hier erfuellt und **gemessen** (`tests/k4_pipeline.rs`):
//! die geschriebene Datei wird mit `file` **im Test** geoeffnet und ihre
//! **Beschreibung** geprueft. Das ist keine Augenschein-Pruefung an einem
//! selbstgebauten Leser, sondern das **Werkzeug des Systems**.
//!
//! Der **erste** Teil — der Klick — ist **nicht** hier, sondern im Fenster. Und
//! er braucht einen laufenden Browser; das ist die naechste Aufgabe, und sie
//! ist im STATUS als **offen** vermerkt.
//!
//! > **Eine Kette, deren letztes Glied man messen kann, ist halb fertig. Eine
//! > Kette, deren Ende man nur selbst beurteilt, ist gar nicht fertig.**

use crate::Bild;
use crate::fehler::{KonvertFehler, Zielformat};
use crate::ton::Pcm;
use crate::ziele::Optionen as Standard;

/// **Ein** Aufruf fuer alle sieben Ziele — das ist der ganze Punkt von K4.
///
/// Und es gibt genau **einen** Aufruf statt sieben, weil sieben Aufrufe sieben
/// Orte sind, an denen ein Bildformat vergessen werden kann. [`Zielformat`] ist
/// der eine Ort, und das Menue, die Pipeline und der Test gehen **alle drei**
/// dieselbe Liste.
/// Und die drei **Hüllen** — sie rufen **nur** [`konvertiere`].
///
/// Und GEMESSEN begründet, warum sie **nicht** wegfallen: sie sind die
/// **kurzen** Wege, die ein Aufrufer tatsaechlich schreibt — und ein Aufruf, den
/// man immer mit einer Aufzaehlung vollstopft, wird **nicht** benutzt.
pub fn konvertiere_bild(ziel: Zielformat, quelle: &[u8]) -> Result<Vec<u8>, KonvertFehler> {
    bild_oder_ton(konvertiere(
        "",
        &Eingabe::Bytes(quelle.to_vec()),
        ziel,
        Standard::default(),
    ))
}

/// Derselbe Aufruf, aber mit einem Bild, das man **schon** hat.
pub fn bild_nach(bild: &Bild, ziel: Zielformat) -> Result<Vec<u8>, KonvertFehler> {
    bild_oder_ton(konvertiere(
        "",
        &Eingabe::Bild(bild.clone()),
        ziel,
        Standard::default(),
    ))
}

/// Und derselbe Aufruf fuer Ton.
///
/// **Und hier ist eine Einschraenkung, die man zur Laufzeit spuert und nicht
/// erst beim Lesen**: der Ton geht **nicht** ueber Bytes, sondern ueber [`Pcm`].
/// Denn `narsil-audio` hat nachweislich **weder** einen FLAC- **noch** einen
/// MP3-Dekoder (siehe Bericht K1/K2 und `team/PROBLEME.txt`) — und eine
/// Pipeline, die WAV und FLAC schreiben kann, aber WAV und FLAC **nicht lesen**
/// kann, ist halb gebaut.
///
/// Also ist der Aufruf **offen** und heisst nicht `konvertiere_ton(bytes)`,
/// sondern `ton_nach(pcm, ziel)`. Der Unterschied im Namen ist der Unterschied
/// im Wissen.
///
/// > **Ein Aufruf, dessen Name etwas verspricht, das er nicht kann, ist eine
/// > Falle mit sauberer Typisierung.**
pub fn ton_nach(pcm: &Pcm, ziel: Zielformat) -> Result<Vec<u8>, KonvertFehler> {
    bild_oder_ton(konvertiere(
        "",
        &Eingabe::Pcm(pcm.clone()),
        ziel,
        Standard::default(),
    ))
}

/// Und die **eine** Bruecke von [`PipelineFehler`] nach [`KonvertFehler`].
///
/// GEMESSEN begründet, warum es eine ist und kein `From`: die beiden Fehler
/// haben **verschiedene** Anwendungen. `KonvertFehler` ist der **Enkoder**
/// (sechs Unterformate, neun Fehler), `PipelineFehler` ist die **Pipeline**
/// (drei Faelle). Und „der Aufrufer kennt beides nicht" ist **die** Ausrede,
/// mit der man in einem Projekt zwei Fehlertypen erfindet.
fn bild_oder_ton(r: Result<Vec<u8>, PipelineFehler>) -> Result<Vec<u8>, KonvertFehler> {
    r.map_err(|e| match e {
        PipelineFehler::ZielNichtGebaut { ziel } => KonvertFehler::ZielNichtSinnvoll {
            ziel,
            grund: "dieses Ziel ist nicht gebaut",
        },
        PipelineFehler::TonBytesNichtLesbar => KonvertFehler::KeinTonformat {
            grund: "kein Ton-Leser in diesem Weg",
        },
        // Und GEMESSEN ist, warum **Video** **auch** ein Bildfehler wird und
        // **kein** neuer `KonvertFehler`: `KonvertFehler` kennt **Bild** und
        // **Ton** als **Endarten**, und ein Video ist fuer den **Aufrufer** am
        // Ende genau das, was es ist — **kein** lesbares Bild. Und **der** Text
        // steht in `Display`, und **der** ist der, den der Nutzer liest.
        PipelineFehler::VideoBytesNichtLesbar { .. }
        | PipelineFehler::ZielAusBildNichtMoeglich { .. } => KonvertFehler::KeinBildformat,
        PipelineFehler::BildBytesNichtLesbar => KonvertFehler::KeinBildformat,
        PipelineFehler::QuelleNichtLesbar { grund, .. } => KonvertFehler::KeinTonformat { grund },
        // Und GEMESSEN: der Grund **nennt** den Grund, weil „Ziel nicht gebaut"
        // hier **falsch** waere — `Png` **ist** gebaut.
        //
        // Und GEMESSEN, warum **auch** dieser Film-Fehler `ZielNichtSinnvoll` ist
        // und **nicht** `ZielNichtGebaut`: `Wav` **ist** gebaut, also waere
        // „nicht gebaut" **gelogen** — und ein Fehler, der die falsche Ursache
        // nennt, wird **geglaubt**.
        PipelineFehler::FilmOhneTon { ziel } => KonvertFehler::ZielNichtSinnvoll {
            ziel,
            grund: "der Film enthaelt keinen Ton — also waere die Datei **leer**",
        },
        PipelineFehler::FilmNurVideo { ziel, film_ziel } => KonvertFehler::ZielNichtSinnvoll {
            ziel,
            grund: match film_ziel {
                Zielformat::Avi => {
                    "ein Film aus mehreren Bildern und Ton ist nur als \
                                    Video sinnvoll — als Einzelbild wuerde der Ton \
                                    verworfen"
                }
                _ => "ein Film ist nur als Video sinnvoll",
            },
        },
    })
}

pub fn konvertiere_und_speichere(
    verzeichnis: &std::path::Path,
    quelle_name: &str,
    quelle: &[u8],
    ziel: Zielformat,
) -> Result<(std::path::PathBuf, Vec<u8>), KonvertFehler> {
    let inhalt = konvertiere_bild(ziel, quelle)?;
    let pfad = verzeichnis.join(dateiname(quelle_name, ziel));
    std::fs::write(&pfad, &inhalt).map_err(|_| KonvertFehler::SchreibenFehlgeschlagen {
        pfad: pfad.to_string_lossy().into_owned(),
    })?;
    Ok((pfad, inhalt))
}

/// **WAS REINKOMMT** — und diese Aufzählung ist die **ehrlichste** Stelle der
/// ganzen Pipeline.
///
/// `AUTONOM.txt` Zeile 677 verlangt **einen** Aufruf
/// `konvertiere(bytes, quelle, ziel, optionen)`. Und **ein** Aufruf ist
/// machbar — aber nur, wenn man sagt, **was** hineinkommt.
///
/// Und GEMESSEN ist, warum das nicht `&[u8]` sein kann:
///
/// | Eingabe | geht? | weil |
/// |---|---|---|
/// | Bild-**Bytes** | ja | `narsil_image::werkzeuge::lesen` liest sie |
/// | ein fertiges [`Bild`] | ja | dann braucht man **nicht** noch einmal zu lesen |
/// | Ton-**Bytes** (WAV/FLAC) | **nein** | `narsil-audio` hat **weder** FLAC- **noch** MP3-Dekoder |
/// | ein fertiges [`Pcm`] | ja | dann entfaellt das Lesen |
///
/// Und der dritte Fall ist **kein Implementierungsdetail**, sondern eine
/// **Lücke**: eine Pipeline, die WAV und FLAC **schreiben** kann, aber nicht
/// **lesen**, ist **halb** gebaut.
///
/// > **Eine Schnittstelle, die eine Eingabe nicht annehmen kann, sollte das
/// > **benennen** statt zu verstecken.** Ein `&[u8]`, das bei Ton `KeinTonformat`
/// > sagt, waere eine **Luege mit sauberer Typisierung** — der Aufrufer hat
/// > Bytes **uebergeben**, also hat er sie gekannt.
///
/// Und GEMESSEN begründet, warum `Bild` und `Pcm` trotzdem **hinter** Bytes
/// liegen: sie sind der **schnellere** Weg, und ein Aufrufer, der das Bild gerade
/// decodiert hat, soll es **nicht** zweimal tun.
#[derive(Debug, Clone)]
pub enum Eingabe {
    /// Die Datei, wie sie auf der Platte liegt.
    Bytes(Vec<u8>),
    /// Schon **fertig** — der Fall, in dem die Seite gerade decodiert hat.
    Bild(Bild),
    /// Schon **fertig**.
    Pcm(Pcm),
    /// Eine **Bildfolge** — der Video-Weg.
    ///
    /// Und GEMESSEN begründet, warum das eine **eigene** Variante ist und nicht
    /// `Vec<Vec<u8>>` mit JPEGs: der Aufrufer, der Bilder **hat**, soll sie nicht
    /// erst in JPEGs verwandeln muessen, um sie wieder dekodieren zu lassen.
    ///
    /// Und **das** ist der Grund, warum die Variante **Bild** und nicht
    /// **Byte** heisst: die **Bildmasse** ist die **Eingangsgrenze** (Bericht K4
    /// Teil 6), also muss sie **vor** dem Enkoder geprueft werden — und das geht
    /// nur mit Bildern.
    Bilder(Vec<Bild>),
    /// Ein **Film**: Bilder **und** Ton.
    ///
    /// Und **das** ist die Variante, die Teil 8 **fehlte** — und GEMESSEN
    /// begruendet, warum sie fehlte: `Bilder` hat **keinen** Ton und `Pcm`
    /// **keine** Bilder. Also **konnte** man nicht beides uebergeben, und ein
    /// zweiter Weg (`schreiben_mit_ton`) war der **Ausweg** — und der war
    /// **gemessen** ein **zweiter Weg**.
    ///
    /// Und **jetzt** ist es **ein** Weg: `konvertiere` ist der **einzige**
    /// Einstieg, und diese Variante ist der **innere** Aufbau — genau wie
    /// `bild_nach_inner`.
    ///
    /// > **Ein zweiter Weg wird dadurch zum ersten, dass man die fehlende
    /// > Information in die Eingabe legt.** Und nicht dadurch, dass man den
    /// > zweiten Weg traegt.
    ///
    /// Und **warum** eine **eigene** Variante statt `Film(Box<(Vec<Bild>,
    /// Pcm)>)`: weil der Aufrufer sie **nicht** zusammenpacken soll. Sie ist
    /// **zwei** Felder, damit man sie **nicht** verwechseln kann.
    Film { bilder: Vec<Bild>, ton: Pcm },
    /// **Nur Ton, keine Bilder** — und das ist **kein** `Film` mit leerem
    /// `bilder`.
    ///
    /// GEMESSEN, warum es eine **eigene** Variante sein muss: `konvertiere`
    /// ruft im `Film`-Zweig `bilder_pruefen`, und das gibt bei **leerem**
    /// `bilder` `BildBytesNichtLesbar` zurueck. „nur Ton“ aus einem WebM hat
    /// aber **keine** Bilder — der Ton ist alles, was der Aufrufer wollte, und
    /// ein Bild-Fehler dafuer waere **gelogen**.
    ///
    /// Und `Film { bilder: vec![], ton }` waere genau diese Luege in
    /// sauberer Typisierung: der Aufrufer hat **nicht** nach Bildern gefragt.
    NurTon(Pcm),
}

/// Was bei [`konvertiere`] **schiefgehen** kann, das **nicht** der Enkoder
/// gesagt hat — die zwei Faelle, in denen die **Pipeline** etwas weiss, was der
/// Enkoder nicht weiss.
///
/// Und GEMESSEN begründet, warum sie hier stehen und nicht in `fehler.rs`:
///
/// - `ZielNichtSinnvoll` steht **schon** in `fehler.rs`, aber **ohne** Erzeuger;
/// - und ein Fehler, den niemand erzeugt, ist eine **Dokumentation mit Enum**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineFehler {
    /// Dieses Ziel ist **nicht gebaut** — mit dem Namen des Grundes.
    ///
    /// GEMESSEN: `Mp3` und `Ogg` stehen in `Zielformat`, sind aber nicht gebaut.
    /// Also ist diese Meldung **der Grund**, warum sie **nicht** im Menue stehen.
    ZielNichtGebaut { ziel: Zielformat },
    /// Ein Film **ohne Ton**, aber ein Ziel, das **Ton** verlangt („nur Ton“).
    ///
    /// Und GEMESSEN begruendet, warum das **eigener** Fehler ist: `ZielNichtGebaut`
    /// wuerde sagen „das Ziel ist nicht gebaut“ — und `Wav` **ist** gebaut. Und
    /// `FilmNurVideo` wuerde sagen „ein Film kann nur Video“ — und **das** ist
    /// seit K5 **falsch**. Also braucht es **einen** dritten Namen, und der
    /// traegt den **tatsaechlichen** Grund: es gibt **keinen** Ton.
    ///
    /// > **Ein Film, aus dem der Ton fehlt, ist kein Film mit leisem Ton** —
    /// > und der Aufrufer soll das **wissen**, bevor er dem Nutzer eine **leere**
    /// > Datei anbietet.
    FilmOhneTon { ziel: Zielformat },
    /// Ein Film, aber ein Ziel, das **kein** Video ist — mit **beiden** Zielen im
    /// Text, damit der Aufrufer dem Nutzer **sagen** kann, was er gewaehlt hat.
    ///
    /// Und GEMESSEN begruendet, warum das **nicht** `ZielNichtGebaut` ist: dort
    /// heisst „dieses Ziel ist **nicht gebaut**" — und `Png` **ist** gebaut. Der
    /// Grund ist **ein anderer**: ein Film hat **kein** Einzelbild.
    ///
    /// > **Ein benannter Fehler, der die falsche Ursache nennt, ist schlimmer als
    /// > `Err` ohne Text** — denn er wird **geglaubt**.
    FilmNurVideo {
        ziel: Zielformat,
        film_ziel: Zielformat,
    },
    /// Bytes, die **kein** Ton sind — weil Ton-Bytes **nicht** gelesen werden.
    ///
    /// Und der Text nennt den Grund, weil „kein Tonformat" **falsch** waere:
    /// die Bytes **sind** ein Ton, wir koennen sie nur nicht lesen.
    TonBytesNichtLesbar,
    ///
    /// **Video**-Bytes, die auf diesem Weg **nicht** gelesen werden können.
    ///
    /// Und GEMESSEN ist, warum das **ein eigener** Fehler ist und **nicht**
    /// `TonBytesNichtLesbar`: der alte Wächter unterschied **Bild** von
    /// **Nicht-Bild**, also fiel **jeder** Video-Container durch **dieselbe**
    /// Tür — und der Nutzer las „`narsil-audio` hat keinen FLAC-Dekoder“, während
    /// der wahre Grund ein **Video** war.
    ///
    /// > **Ein Wächter, der *zwei* Dinge über einen Vergleich entscheidet, meldet
    /// > *eines* von beiden — und der Aufrufer repariert das *falsche*.**
    ///
    /// Und **der** wahre Grund steht hier, und **er** ist **zwei** Zeilen:
    /// `Eingabe::Bytes` geht über `narsil_image::werkzeuge::lesen`, und das **kann**
    /// **keinen** AVI lesen (K6 Teil 0, GEMESSEN 0 AVI-Bildleser im Workspace).
    /// Also ist der **Weg** `video::lesen` + `Eingabe::Film`.
    VideoBytesNichtLesbar { endung: String },
    ///
    /// Ein Ziel, das aus **einem Bild** nicht entstehen kann — und **warum** es
    /// nicht entsteht, steht in `weg`.
    ///
    /// Und GEMESSEN ist, warum das **ein** Fehler mit **einem** Weg-Text pro Fall
    /// ist und **nicht** `ZielNichtGebaut`: der `_`-Arm von `bild_nach_inner`
    /// sammelte **drei** verschiedene Ursachen.
    ///
    /// | Ziel | gebaut? | alter Text | **richtiger** Text |
    /// |------|---------|------------|---------------------|
    /// | `Avi` | **ja** | „nicht gebaut“ | aus einem Bild entsteht **kein** Film |
    /// | `Wav`, `Flac` | **ja** | „nicht gebaut“ | aus einem Bild entsteht **kein** Ton |
    /// | `Mp3`, `Ogg` | **nein** | „nicht gebaut“ | **richtig** |
    ///
    /// > **Ein Sammelarm ist billiger als drei Arme** — und **teurer** im Fehler,
    /// > denn er **nennt** für zwei Fälle eine Ursache, die **nicht** stimmt.
    ZielAusBildNichtMoeglich { ziel: Zielformat, weg: &'static str },
    /// Und Bild-Bytes, die kein Bild sind.
    BildBytesNichtLesbar,
    /// Und **Ton**-Bytes, die **kein** Ton sind — mit dem **Grund**.
    ///
    /// Und GEMESSEN ist, warum **das** eine **eigene** Variante ist und nicht der
    /// alte Einheitsarm `TonBytesNichtLesbar`: dieser sagte bei einer **WAV**-Datei
    /// „`narsil-audio` hat keinen FLAC- und keinen MP3-Dekoder“ — und **das** ist
    /// **falsch** (K8: eine irrefuehrende Fehlermeldung ist schlimmer als keine,
    /// denn sie schickt den Nutzer auf **eine** Suche, die **nichts** findet).
    ///
    /// | Quelle | alter Text | **richtiger** Text |
    /// |--------|-----------|---------------------|
    /// | `wav` | „kein FLAC-Dekoder“ | der WAV-Weg, mit dem Grund |
    /// | `flac` | „kein FLAC-Dekoder“ | **richtig**, aber **ohne** Weg |
    ///
    /// Und GEMESSEN ist, warum **das** traegt: `grund` ist **kein** Text fuer
    /// Nutzer, sondern der **Weg**, der nicht ging — und **ein** benannter Weg
    /// laesst sich bauen, ein **Sammelarm** nicht.
    QuelleNichtLesbar { endung: String, grund: &'static str },
}

impl std::fmt::Display for PipelineFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineFehler::ZielNichtGebaut { ziel } => {
                write!(f, "{ziel:?} ist nicht gebaut")
            }
            PipelineFehler::TonBytesNichtLesbar => write!(
                f,
                "Ton-Bytes koennen nicht gelesen werden: `narsil-audio` hat keinen \
                 FLAC- und keinen MP3-Dekoder"
            ),
            PipelineFehler::VideoBytesNichtLesbar { endung } => write!(
                f,
                "`{endung}` ist ein **Video**, und dieser Weg liest **keine** Videos: \
                 der Bildleser kann keine Bild-Chunks aus einem Container holen. \
                 Der Weg dahin ist `video::lesen` und dann `Eingabe::Film`"
            ),
            PipelineFehler::ZielAusBildNichtMoeglich { ziel, weg } => write!(
                f,
                "Aus **einem** Bild entsteht kein {ziel:?} — und der Enkoder ist \
                 **gebaut**: {weg}"
            ),
            PipelineFehler::QuelleNichtLesbar { endung, grund } => {
                write!(
                    f,
                    "`.{endung}` ist nicht lesbar: {grund} — und dieser Weg hat keinen \
                     Decoder fuer diese Datei"
                )
            }
            PipelineFehler::BildBytesNichtLesbar => {
                write!(f, "Die Bytes sind kein lesbares Bild")
            }
            PipelineFehler::FilmOhneTon { ziel } => {
                // Und der Text sagt **beides**: was der Nutzer gewaehlt hat, und
                // warum das **nicht** geht. Und GEMESSEN ist, dass „leer" das
                // **beste** Wort ist — ein stiller Erfolg waere eine Datei, die
                // niemand hoert.
                write!(
                    f,
                    "„Nur Ton“ aus diesem Film: der Film hat **keinen** Ton —                      als {ziel:?} entstuende eine **leere** Datei"
                )
            }
            PipelineFehler::FilmNurVideo { ziel, film_ziel } => {
                // Und GEMESSEN: der Text nennt **beide** Ziele, weil der Aufrufer
                // dem Nutzer sonst nur sagen kann „das geht nicht" — und nicht
                // „das geht nicht, **das** hier geht".
                write!(
                    f,
                    "Ein Film aus mehreren Bildern und Ton ist nur als Video sinnvoll: \
                     als {ziel:?} wuerde der Ton verworfen — moeglich ist {film_ziel:?}"
                )
            }
        }
    }
}

/// # DER EINE AUFRUF
///
/// `AUTONOM.txt` Zeile 677:
///
/// > `konvertiere(bytes, quelle, ziel, optionen) -> Ergebnis`
///
/// Und GEMESSEN, warum das **nicht** trivial war: es waren vorher **drei**
/// Aufrufe, und **jeder** war eine eigene Stelle, an der eine Option fehlen
/// konnte.
///
/// Und die Regel dahinter ist die Verallgemeinerung von sieben Messungen pro
/// Kanal (Teil 5): **ein Aufruf pro Sache, und „eine Sache" heisst: was der
/// Aufrufer als *eine* Handlung sieht.**
///
/// Und GEMESSEN, warum das **nicht** Kosmetik ist: `ton_nach` nahm **keine**
/// Optionen an, weil es keine brauchte — und `bild_nach` auch nicht. Bei einem
/// **fünften** Ziel wäre die Frage „welche Optionen gelten hier?" **neu**, und sie
/// wäre an **fünf** Orten zu beantworten.
///
/// # UND DIE DREI HUELLEN BLEIBEN — sie rufen **nur** hierhin
///
/// Das ist der Punkt: sie sind jetzt **kein zweiter Weg** mehr, sondern eine
/// **Zeile**. Und der Test
/// `der_eine_aufruf_und_drei_huellen_ergeben_dasselbe` prüft das **gemessen**,
/// Byte für Byte — nicht durch Lesen, sondern durch Vergleich.
pub fn konvertiere(
    quelle_name: &str,
    eingabe: &Eingabe,
    ziel: Zielformat,
    optionen: crate::ziele::Optionen,
) -> Result<Vec<u8>, PipelineFehler> {
    // **1. Bau.** Ein Menue, das etwas anbietet, das nicht existiert, ist
    // schlimmer als ein Menue mit weniger — und hier ist die **Pipeline**, die
    // es merkt.
    if !crate::ziele::produzierbar(ziel) {
        return Err(PipelineFehler::ZielNichtGebaut { ziel });
    }
    // **2. Optionen.** Und **vor** der Arbeit, wie die Speichergrenze in Teil 3.
    optionen
        .pruefe(ziel)
        .map_err(|_| PipelineFehler::ZielNichtGebaut { ziel })?;

    // **3. Sinnvoll?** Und **erst**, wenn die Masse **da** sind — also nach dem
    // Lesen und **vor** dem Enkoden. GEMESSEN: der Enkoder lehnt nichts ab
    // (siehe `ziel_sinnvoll`), also waere diese Zeile **wirkungslos**, wenn sie
    // nicht **vor** ihm stuende.
    //
    // Und GEMESSEN begründet, warum **nicht** vorher: die Grenze ist eine
    // **Bildmasse**, und die kennt man vor dem Lesen **nicht**. Also ist „vor der
    // Arbeit" hier **nach** dem Lesen — und das ist der **einzige** Ort, wo die
    // Reihenfolge stimmt.
    match eingabe {
        Eingabe::Bytes(b) => {
            let endung = quelle_name.rsplit_once('.').map_or("", |(_, e)| e);
            //
            // Und **hier** ist die **zweite** Bildquelle, und sie ist
            // **gemessen** statt erfunden: `narsil-image` liest **animierte** GIFs
            // (`gif_lesen_alle`, `frames: Vec<GifFrame>` mit `verzoegerung_ms`).
            // Also ist ein GIF **kein Bild**, sondern eine **Bildfolge mit
            // Zeit** — und das ist genau die Quelle, die K3 gebraucht hat.
            //
            // GEMESSEN: die Bildrate kommt aus den **Verzoegerungen**, nicht aus
            // einer Vorgabe. Und die Vorgabe `fps = 10` gilt **nur**, wenn der
            // Aufrufer sie **wirklich** will — also gewinnt der Wert aus dem GIF,
            // **ausser** der Aufrufer hat einen **anderen** gesetzt.
            //
            // Und **hier** war GEMESSEN ein **Fehler**, und er ist der Grund,
            // warum `GIF -> Png` ein **AVI** lieferte:
            //
            // ```text
            //   GEMESSEN: `GIF -> Png` -> Anfang [52, 49, 46, 46] = "RIFF"
            // ```
            //
            // Der Zweig lief fuer **jedes** Ziel — und `schreiben_aus_bildern`
            // baut **immer** ein AVI. Also kam bei `Png` ein **AVI** heraus.
            //
            // > **Wenn ein Aufrufer `Png` sagt und `Avi` bekommt, ist der
            // > Aufrufer nicht kaputt — er ist **belogen**.** Und ein Aufrufer,
            // > der belogen wird, kann keinen **Fehler** behandeln, weil er
            // > **keinen** sieht.
            //
            // Und **die** Reihenfolge ist damit fest: erst `ist_video()` — also
            // **was** der Aufrufer will — und **dann**, **ob** es ein GIF ist.
            // Vorher war es umgekehrt, und die **Quelle** gewann gegen das **Ziel**.
            //
            // KONVERT_9: die GIF-Quelle wurde am **Namen** erkannt
            // (`endung.eq_ignore_ascii_case("gif")`) — und `narsil` uebergibt
            // **keinen** Namen mit Endung (`ktx_konvert.rs` Z. 403: `stamm` ist
            // der Name **ohne** Endung). GEMESSEN: `endung` war also `""`, die
            // Bedingung **nie** wahr, und ein **animiertes** GIF kam als
            // **Einzelbild** heraus — bei **jedem** Ziel, **ohne** Fehler.
            //
            // GEMESSEN (k17): GIF-anim mit 3 Bildern -> Gif = **1** Bild,
            // 543 B, dieselbe Groesse wie ein Standbild. Sechs von 42 Zellen der
            // Bild-Matrix verlieren Bilder **still**.
            //
            // Also wird die GIF-Quelle jetzt an der **Signatur** erkannt, wie der
            // Ton es oben schon tut — und **beide** Ziele bekommen **alle**
            // Bilder: `Gif` als Animation mit den Verzoegerungen, `Avi` als Film.
            if let Some(g) = narsil_image::gif::gif_lesen_alle(b)
                .ok()
                .filter(|g| g.frames.len() > 1)
            {
                if ziel == Zielformat::Gif {
                    let bilder: Vec<crate::gif::GifBild> = g
                        .frames
                        .iter()
                        .map(|f| crate::gif::GifBild {
                            bild: f.bild.clone(),
                            verzoegerung_ms: u32::from(f.verzoegerung_ms),
                            versatz: (u32::from(f.versatz_x), u32::from(f.versatz_y)),
                            // Und **GEMESSEN**, warum hier **gemaess** werden
                            // muss: `narsil_image::Entsorgung` hat **fuenf**
                            // Faelle (mit `Vormerken` und `Unbekannt`), meine
                            // `gif::Entsorgung` hat **drei**. Der Compiler sagt
                            // es laut — „similar names, but are actually
                            // distinct types" — und das ist eine **gute** Meldung,
                            // weil sie eine Frage stellt statt einer Annahme.
                            //
                            // Und die **Frage** ist eine Frage der Norm: `0..2`
                            // sind Behalten/Aufraeumen/Zuruecknehmen, und `3..7`
                            // sind reserviert und werden wie `0` behandelt.
                            entsorgung: match f.entsorgung {
                                narsil_image::Entsorgung::Behalten => {
                                    crate::gif::Entsorgung::Behalten
                                }
                                narsil_image::Entsorgung::Aufraeumen => {
                                    crate::gif::Entsorgung::Aufraeumen
                                }
                                narsil_image::Entsorgung::Zuruecknehmen => {
                                    crate::gif::Entsorgung::Zuruecknehmen
                                }
                                // Und GEMESSEN ist die Regel fuer den Rest: der
                                // Leser hat sie als „wie 0“ dokumentiert, also
                                // **behalten** — und **nicht** raten.
                                _ => crate::gif::Entsorgung::Behalten,
                            },
                        })
                        .collect();
                    return crate::gif::schreiben(&bilder, Default::default())
                        .map_err(|_| PipelineFehler::ZielNichtGebaut { ziel });
                }
                if ziel.ist_video() {
                    let (bilder, fps_aus_gif) = crate::video::aus_gif(b)
                        .map_err(|_| PipelineFehler::BildBytesNichtLesbar)?;
                    let (breite, hoehe) = (bilder[0].breite, bilder[0].hoehe);
                    return crate::video::schreiben_aus_bildern(
                        &bilder,
                        breite,
                        hoehe,
                        if optionen.fps == 10 {
                            fps_aus_gif
                        } else {
                            optionen.fps
                        },
                        optionen.qualitaet,
                    )
                    .ok_or(PipelineFehler::ZielNichtGebaut { ziel });
                }
            }
            //
            // Und der **alte** Weg bleibt fuer die Faelle, in denen der Name
            // **doch** stimmt — er ist jetzt nur noch der Rueckfall.
            if ziel.ist_video()
                && endung.eq_ignore_ascii_case("gif")
                && narsil_image::gif::gif_lesen_alle(b).is_ok_and(|g| g.frames.len() > 1)
            {
                let (bilder, fps_aus_gif) =
                    crate::video::aus_gif(b).map_err(|_| PipelineFehler::BildBytesNichtLesbar)?;
                let (breite, hoehe) = (bilder[0].breite, bilder[0].hoehe);
                return crate::video::schreiben_aus_bildern(
                    &bilder,
                    breite,
                    hoehe,
                    if optionen.fps == 10 {
                        fps_aus_gif
                    } else {
                        optionen.fps
                    },
                    optionen.qualitaet,
                )
                .ok_or(PipelineFehler::ZielNichtGebaut { ziel });
            }
            //
            // Und **hier** ist GEMESSEN ein **echter** Fehler, und **der** ist
            // aus K8 Teil 1: der alte Wächter sagte nur „**kein** Bild“, also fiel
            // **jeder** Video-Container durch **dieselbe** Tür **wie** eine
            // Audiodatei — und der Nutzer las einen **Ton**-Fehler bei einem
            // **Video**.
            //
            // | Endung | `ist_bild()` | alter Fehler | richtiger Fehler |
            // |--------|---------------|---------------|-----------------|
            // | `wav`, `flac` | nein | `TonBytesNichtLesbar` | **richtig**    |
            // | `avi` | nein | `TonBytesNichtLesbar` | **falsch**      |
            //
            // > **Bevor ein Wächter *ablehnt*, muss er sagen, *was* er ist.** Und
            // > „Nicht-Bild“ ist **keine** Ursache, sondern eine **Eigenschaft** —
            // > und **Eigenschaften** trennen **keine** Ursachen.
            //
            // KONVERT_8: **hier** war die Luecke, und sie ist **von mir**
            // dokumentiert. Z. 183 derselben Datei sagt es woertlich:
            //
            // > „eine Pipeline, die WAV und FLAC **schreiben** kann, aber nicht
            // >  **lesen**, ist **halb** gebaut.“
            //
            // GEMESSEN war der Befund: `Eingabe::Bytes(wav)` mit Ziel `Flac`
            // endete hier mit `TonBytesNichtLesbar` — und `ziele_mit_hinweis`
            // **verspricht** genau diese Zeile im Menue.
            //
            // Und **GEMESSEN**, dass es geht: `narsil-audio/src/wav.rs` Z. 374 hat
            // `pub fn dekodiere(daten) -> Ergebnis<Ton>`, `flac.rs` Z. 1100
            // ebenfalls, und `ogg_flac.rs` Z. 246 auch. Drei Leser, **keiner**
            // benutzt.
            //
            // Die Reihenfolge ist **gemessen** an den Signaturen und **nicht**
            // geraten: RIFF … WAVE → `wav`, `fLaC` → `flac`, `OggS` → `ogg_flac`.
            if ziel.ist_ton() {
                if let Some(t) = ton_aus_bytes(b) {
                    return ton_nach_inner(&ton_als_pcm(&t), ziel, optionen);
                }
            }
            if let Some(z) = crate::ziele::zielformat_von_endung(endung) {
                if z.ist_ton() {
                    return Err(PipelineFehler::TonBytesNichtLesbar);
                }
                if z.ist_video() {
                    return Err(PipelineFehler::VideoBytesNichtLesbar {
                        endung: endung.to_string(),
                    });
                }
            }
            let bild = narsil_image::werkzeuge::lesen(b)
                .map_err(|_| PipelineFehler::BildBytesNichtLesbar)?;
            ziel_sinnvoll(ziel, bild.breite, bild.hoehe)?;
            bild_nach_inner(&bild, ziel, optionen)
        }
        Eingabe::Bild(bild) => {
            ziel_sinnvoll(ziel, bild.breite, bild.hoehe)?;
            bild_nach_inner(bild, ziel, optionen)
        }
        Eingabe::Pcm(pcm) => ton_nach_inner(pcm, ziel, optionen),
        Eingabe::Bilder(bilder) => {
            // Und **hier** ist die Bildmasse **da** — also wird sie **jetzt**
            // geprueft, und **vor** dem Enkoder.
            let (breite, hoehe) = bilder_pruefen(bilder, ziel)?;
            // Und **der Zweig, der gefehlt hat** — GEMESSEN in KONVERT_1:
            //
            // ```text
            //   GEMESSEN: `GIF -> Png` -> Anfang [52, 49, 46, 46] = "RIFF"
            // ```
            //
            // `schreiben_aus_bildern` baut **immer** ein AVI. Also kam bei
            // `Gif` ein **AVI** heraus, und der Aufrufer bekam **falschen**
            // Erfolg. Die Reihenfolge ist damit **gemessen** fest: erst das
            // Ziel **fragen**, dann den Container **wählen**.
            if ziel == Zielformat::Gif {
                let verzoegerung_ms = ((1000 / optionen.fps.max(1)) / 10 * 10).max(10);
                let gif_bilder: Vec<crate::gif::GifBild> = bilder
                    .iter()
                    .map(|b| crate::gif::GifBild {
                        bild: b.clone(),
                        verzoegerung_ms,
                        versatz: (0, 0),
                        entsorgung: crate::gif::Entsorgung::Behalten,
                    })
                    .collect();
                return crate::gif::schreiben(&gif_bilder, Default::default())
                    .map_err(|_| PipelineFehler::ZielNichtGebaut { ziel });
            }
            crate::video::schreiben_aus_bildern(
                bilder,
                breite,
                hoehe,
                optionen.fps,
                optionen.qualitaet,
            )
            .ok_or(PipelineFehler::ZielNichtGebaut { ziel })
        }
        Eingabe::NurTon(pcm) => ton_nach_inner(pcm, ziel, optionen),
        Eingabe::Film { bilder, ton } => {
            //
            // Und **hier** ist der **zweite** Ort, an dem `bilder_pruefen`
            // seine **beiden** Dinge zurueckgibt — und **hier** wird die
            // **Groesse** gebraucht, weil das animierte GIF sie braucht.
            let (breite, hoehe) = bilder_pruefen(bilder, ziel)?;
            let _ = (breite, hoehe);
            //
            // Und **GEMESSEN** ist, dass ein Film **vier** Ziele bedienen kann,
            // und **alle vier** sind der LEITUNG (04.10.2026) geschuldet:
            //
            // | Menueeintrag | Weg | Verlust |
            // |---|---|---|
            // | Film (AVI)   | `schreiben_mit_ton` | keiner |
            // | nur Ton (WAV)| `ton_nach_inner`    | die Bilder |
            // | animiert (GIF)| `gif::schreiben`   | der Ton |
            // | Standbild (PNG) | `bild_nach_inner` | **alle Bilder bis auf eins** |
            //
            // Und **die** alte Regel — „ein Film kann **nur** Video“ — ist
            // **gemessen** die **falsche** Regel, und **der Grund** ist
            // **dieselbe** Regel, die diese Crate **anderswo** schon richtig
            // anwendet: Teil 10 hat fuer ein **animiertes GIF** festgelegt, dass
            // ein Einzelbild-Format **das erste** Bild nimmt — **mit** Hinweis.
            // Ein Film ist **auch** eine Bildfolge, also gilt **dieselbe** Regel
            // fuer ihn. Und **das** ist die Messung, die die Entscheidung kippt:
            //
            // > **Dieselbe Frage, zweimal gestellt, einmal richtig und einmal
            // > falsch beantwortet, ist **ein** Fehler — nicht zwei.**
            //
            // Und GEMESSEN ist ausserdem, dass **kein einziger Test**
            // `FilmNurVideo` erwartet — die alte Regel war **nie**
            // abgesichert. Ein Test, der eine Regel **nicht** prueft, ist kein
            // Test, sondern eine **Absichtserklaerung**.
            //
            // --- 1. der Film selbst ---
            if ziel == Zielformat::Avi {
                // Und **hier** ist der zweite Weg **keiner mehr** — er ist der
                // **innere** Aufbau, und `schreiben_mit_ton` ist `pub(crate)`.
                return crate::video::schreiben_mit_ton(
                    bilder,
                    ton,
                    optionen.fps,
                    optionen.qualitaet,
                )
                .ok_or(PipelineFehler::ZielNichtGebaut { ziel });
            }

            // --- 2. „nur Ton“ ---
            if ziel.ist_ton() {
                //
                // Und **hier** ist der Grund, warum **leerer** Ton ein **eigener**
                // Fehler ist und **nicht** `ZielNichtGebaut`: der Aufrufer hat
                // „nur Ton“ gewaehlt und **bekommt** einen — nur mit **keinem**
                // Ton darin. Das ist **nicht** „das Ziel ist nicht gebaut“.
                if ton.werte.is_empty() {
                    return Err(PipelineFehler::FilmOhneTon { ziel });
                }
                return ton_nach_inner(ton, ziel, optionen);
            }

            // --- 3. „animiert“: ein GIF aus **allen** Bildern ---
            if ziel == Zielformat::Gif {
                //
                // Und GEMESSEN ist, woher die Verzoegerung kommt: das GIF-Feld
                // ist **Hundertstelsekunden**, und `Optionen::fps` ist **Bilder je
                // Sekunde**. Also `1000 / fps` ms — und auf **zehn** gerundet,
                // weil das Feld nur Vielfache von 10 liest. Und GEMESSEN ist der
                // Rand: bei `fps = 0` waere die Verzoegerung unendlich, also
                // **eins** Hundertstelsekunde-Feld minimum.
                let verzoegerung_ms = ((1000 / optionen.fps.max(1)) / 10 * 10).max(10);
                let gif_bilder: Vec<crate::gif::GifBild> = bilder
                    .iter()
                    .map(|b| crate::gif::GifBild {
                        bild: b.clone(),
                        verzoegerung_ms,
                        versatz: (0, 0),
                        entsorgung: crate::gif::Entsorgung::Behalten,
                    })
                    .collect();
                return crate::gif::schreiben(&gif_bilder, Default::default())
                    .map_err(|_| PipelineFehler::ZielNichtGebaut { ziel });
            }

            // --- 4. „Standbild“: **das erste** Bild ---
            //
            // Und GEMESSEN ist, warum das **kein** stiller Verlust ist: Teil 10
            // hat fuer das animierte GIF **denselben** Verlust gewaehlt und ihn
            // **einen Hinweis** genannt. Also steht er auch hier.
            bild_nach_inner(
                bilder.first().ok_or(PipelineFehler::BildBytesNichtLesbar)?,
                ziel,
                optionen,
            )
        }
    }
}

/// Die Bildpruefung, die **beide** Filmwege brauchen — und **einmal**, damit sie
/// nicht **zwei** Kopien hat.
///
/// Und **das** ist der Grund fuer diese Funktion: Teil 8 hat **zwei** Wege
/// gehabt, und der Grund war nicht „zwei Wege sind verboten", sondern „**ein**
/// Weg soll die Information **haben**". Und eine Regel, die man **zwei** Mal
/// hinschreibt, ist **eine** Regel und **zwei** Fehlerquellen.
fn bilder_pruefen(bilder: &[Bild], ziel: Zielformat) -> Result<(u32, u32), PipelineFehler> {
    // GEMESSEN (Teil 6): bei Video ist die Groesse die **einzige** Sache, die der
    // Container nicht selber traegt (AVI hat **keine** Bildgroessengrenze).
    if bilder.len() > crate::grenzen::MAX_VIDEO_BILDER {
        return Err(PipelineFehler::ZielNichtGebaut { ziel });
    }
    let (breite, hoehe) = match bilder.first() {
        None => return Err(PipelineFehler::BildBytesNichtLesbar),
        Some(b) => (b.breite, b.hoehe),
    };
    for b in bilder.iter() {
        if b.breite != breite || b.hoehe != hoehe {
            // GEMESSEN: gemischte Bildgroessen sind fuer einen AVI-Stream
            // **nicht** moeglich — `strf` nennt **eine** Groesse. Also wird es
            // **abgelehnt** statt **stillschweigend** beschnitten.
            return Err(PipelineFehler::ZielNichtGebaut { ziel });
        }
    }
    Ok((breite, hoehe))
}

/// Der **innere** Bildweg — damit [`konvertiere`] der **einzige** Ort ist, an
/// dem die Option **angewandt** wird.
/// Release 08.10.2026: ein Bild mit Alphakanal auf weissen Grund legen
/// (`Farbe · a + Weiss · (1 − a)`); ein deckendes Bild bleibt unberuehrt.
fn auf_weiss(bild: &Bild) -> std::borrow::Cow<'_, Bild> {
    if bild.rgba.chunks_exact(4).all(|p| p[3] == 255) {
        return std::borrow::Cow::Borrowed(bild);
    }
    let mut raus = bild.clone();
    for p in raus.rgba.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        for c in &mut p[..3] {
            *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    std::borrow::Cow::Owned(raus)
}

/// Release 08.10.2026: GIF kennt nur „ganz durchsichtig" oder „deckend".
/// Halbdurchsichtige Pixel werden auf Weiss gemischt und deckend, nur die
/// fast unsichtbaren (Alpha < 128) bleiben durchsichtig. GEMESSEN vorher:
/// ein Alpha-Verlauf kam mit seiner reinen Farbe oder als Loch heraus
/// (Farbabstand 81 von 255 gegen die Quelle auf Weiss).
fn gif_matte(bild: &Bild) -> std::borrow::Cow<'_, Bild> {
    if bild.rgba.chunks_exact(4).all(|p| p[3] == 255 || p[3] == 0) {
        return std::borrow::Cow::Borrowed(bild);
    }
    let mut raus = bild.clone();
    for p in raus.rgba.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a < 128 {
            p[3] = 0;
            continue;
        }
        for c in &mut p[..3] {
            *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    std::borrow::Cow::Owned(raus)
}

fn bild_nach_inner(
    bild: &Bild,
    ziel: Zielformat,
    optionen: crate::ziele::Optionen,
) -> Result<Vec<u8>, PipelineFehler> {
    match ziel {
        Zielformat::Png => crate::png::schreiben(bild, crate::png::PngModus::Rgba),
        // Und **hier** ist die Option zum ersten Mal **wirksam**. Vorher stand
        // hier eine Konstante, und „die Option wirkt nicht" waere **unauffindbar``
        // gewesen.
        //
        // Release 08.10.2026: „ohne Transparenz" heisst auf WEISS (wie eine
        // Seite ohne eigenen Grund und wie die gaengigen Umwandler).
        // GEMESSEN vorher (`narsil-chrome/tests/konvert_matrix.rs`): die
        // durchsichtigen Pixel kamen mit ihrer Restfarbe heraus — meist
        // SCHWARZ; ein GIF mit durchsichtigem Grund wurde als JPEG zu einem
        // schwarzen Kasten (mittlerer Farbabstand 169 von 255).
        Zielformat::Jpeg => crate::jpeg::schreiben(
            &auf_weiss(bild),
            optionen.qualitaet,
            crate::jpeg::Abtastung::Voll,
        ),
        Zielformat::Bmp => crate::bmp::schreiben(&auf_weiss(bild), crate::bmp::BmpArt::Auto),
        Zielformat::Gif => crate::gif::schreiben(
            &[crate::gif::GifBild {
                bild: gif_matte(bild).into_owned(),
                verzoegerung_ms: 0,
                versatz: (0, 0),
                entsorgung: crate::gif::Entsorgung::Behalten,
            }],
            Default::default(),
        ),
        Zielformat::Webp => crate::webp::schreiben_verlustfrei(bild),
        Zielformat::Ico => crate::ico::schreiben(bild),
        //
        // Und **hier** war GEMESSEN **ein** Sammelarm fuer **drei** Ursachen:
        // `Avi` (gebaut, aber nicht aus einem Bild), `Wav`/`Flac` (gebaut, aber
        // nicht aus einem Bild) und `Mp3`/`Ogg` (**nicht** gebaut). Und also sagte
        // **alles drei** „nicht gebaut“ — und **zwei** davon **gelogen**.
        //
        // Und **die** Reihenfolge ist **gemessen** und nicht **beliebig**: **erst**
        // „nicht gebaut“, **weil** das die **einzige** Antwort ist, die **immer**
        // stimmt, und **erst** danach die **beiden** Faelle, in denen der Enkoder
        // **da** ist und **nur** der **Weg** fehlt. Andernfalls waere `Mp3` mit
        // „der Weg ist `Eingabe::Pcm`“ **repariert** worden — und **das** waere die
        // **dritte** Luege.
        _ if !crate::ziele::produzierbar(ziel) => {
            return Err(PipelineFehler::ZielNichtGebaut { ziel });
        }
        _ if ziel.ist_video() => {
            return Err(PipelineFehler::ZielAusBildNichtMoeglich {
                ziel,
                weg: "der Film-Weg ist `Eingabe::Bilder` oder `Eingabe::Film`",
            });
        }
        _ if ziel.ist_ton() => {
            return Err(PipelineFehler::ZielAusBildNichtMoeglich {
                ziel,
                weg: "der Ton-Weg ist `Eingabe::Pcm`",
            });
        }
        _ => return Err(PipelineFehler::ZielNichtGebaut { ziel }),
    }
    .map_err(|_| PipelineFehler::BildBytesNichtLesbar)
}

/// Und der **innere** Tonweg.
/// Und **Ton-Bytes lesen** — die drei Leser, die GEMESSEN vorhanden sind.
///
/// # DIE REIHENFOLGE IST GEMESSEN, NICHT GERATEN
///
/// Jeder Leser ist an einer **Dateisignatur** erkennbar, und die drei Signaturen
/// schliessen einander aus:
///
/// | Signatur | Datei | Leser |
/// |---|---|---|
/// | `RIFF` … `WAVE` | WAV | `narsil_audio::wav::dekodiere` |
/// | `fLaC` | natives FLAC | `narsil_audio::flac::dekodiere` |
/// | `OggS` | Ogg mit FLAC | `narsil_audio::ogg_flac::dekodiere` |
///
/// Und **warum** die Signatur und nicht die **Endung**: `narsil` ruft
/// `pipeline::konvertiere(&stamm, …)`, und `stamm` ist der Name **ohne**
/// Endung (Z. 387 derselben Datei). Wer auf die Endung prueft, bekommt **immer**
/// „unbekannt“ — und das ist der Fehler, den K8 Teil 1 schon einmal als
/// „Bildleser, der kein Bild ist“ beschrieben hat.
pub(crate) fn ton_aus_bytes(daten: &[u8]) -> Option<narsil_audio::ton::Ton> {
    if daten.len() >= 12 && &daten[..4] == b"RIFF" && &daten[8..12] == b"WAVE" {
        if let Ok(t) = narsil_audio::wav::dekodiere(daten) {
            return Some(t);
        }
    }
    if daten.len() >= 4 && &daten[..4] == b"fLaC" {
        if let Ok(t) = narsil_audio::flac::dekodiere(daten) {
            return Some(t);
        }
    }
    if daten.len() >= 4 && &daten[..4] == b"OggS" {
        if let Ok(t) = narsil_audio::ogg_flac::dekodiere(daten) {
            return Some(t);
        }
    }
    // Release 08.10.2026 („Konvertiert herunterladen" Ende zu Ende): alles,
    // was der eigene SPIELER liest — Ogg Vorbis und MP3 dazu, ueber dieselbe
    // Formaterkennung wie `<audio>` (`Format::erkenne`). GEMESSEN vorher
    // (`narsil-chrome/tests/konvert_matrix.rs`): `.ogg` (Vorbis) und `.mp3`
    // nach WAV/FLAC/OGG endeten alle mit „Die Bytes sind kein lesbares
    // Bild" — der Ton wurde gar nicht erst gelesen.
    let format = narsil_audio::Format::erkenne(daten).ok()?;
    narsil_audio::dekodiere_format(format, daten).ok()
}

/// Und `narsil_audio::Ton` → mein `Pcm` — und die **Umordnung** ist **nicht**
/// kosmetisch.
///
/// GEMESSEN (`narsil-audio/src/vorbis.rs` Z. 513ff, woertlich): Ogg/Vorbis liefert
/// „links alle Samples, dann rechts alle Samples“. Und `wav::schreiben` will
/// **interleavt**. Also:
pub(crate) fn ton_als_pcm(t: &narsil_audio::ton::Ton) -> Pcm {
    let kz = t.kanaele.max(1);
    let mut werte = Vec::with_capacity(t.samples * kz);
    for i in 0..t.samples {
        for kanal in &t.kanaele_daten {
            let w = kanal.get(i).copied().unwrap_or(0.0);
            // GEMESSEN: `Ton::als_i16` rundet `wert * 32768` und klemmt bei
            // 32767 / -32768. Genau das tun wir — sonst waere der Weg durch den
            // Leser **ein** Denoising-Schritt, und niemand haette es gemerkt.
            let b = (w * 32768.0).round();
            werte.push(if b > 32767.0 {
                32767
            } else if b < -32768.0 {
                -32768
            } else {
                b as i32
            });
        }
    }
    Pcm {
        rate: t.rate,
        kanale: kz as u8,
        bit_tiefe: 16,
        werte,
    }
}

fn ton_nach_inner(
    pcm: &Pcm,
    ziel: Zielformat,
    optionen: crate::ziele::Optionen,
) -> Result<Vec<u8>, PipelineFehler> {
    match ziel {
        // Und **hier**: die Option **aendert** den PCM, und das ist **ein**
        // Aufruf an **einer** Stelle. Vorher stand hier `WavArt::Pcm16`, und
        // `ton_bits` waere **toter Code** gewesen — und toter Code sieht aus wie
        // funktionierender.
        Zielformat::Wav => {
            let art = match optionen.ton_bits {
                16 => crate::wav::WavArt::Pcm16,
                24 => crate::wav::WavArt::Pcm24,
                32 => crate::wav::WavArt::Pcm32,
                _ => return Err(PipelineFehler::ZielNichtGebaut { ziel }),
            };
            crate::wav::schreiben(pcm, art)
        }
        // FLAC nimmt 4..=32 **aus dem PCM selbst** — also **ignoriert**
        // `ton_bits`. GEMESSEN: das ist kein Fehler, sondern die Richtigkeit
        // (Bericht Teil 4, Test `die_optionen_werden_geprueft`).
        Zielformat::Flac => crate::flac::schreiben(pcm),
        // Und **Ogg** — KONVERT_12 Teil 2. Und der Weg ist **FLAC im
        // Ogg-Container**, weil ein **Vorbis-Paketschreiber** gemessen ein
        // **Codec** ist (`vorbis.rs:119`, `Setup { codebucher, … }`) und FLAC
        // **verlustfrei** ist.
        //
        // GEMESSEN: `ffprobe` -> `Audio: flac, 44100 Hz, mono, s16`, **ohne**
        // Warnung; `file` -> „Ogg data, FLAC audio“; und `ffmpeg` liest 22050
        // Proben mit der Summe |Differenz| = **0** zurueck.
        Zielformat::Ogg => crate::ogg::flac_in_ogg(pcm),
        _ => return Err(PipelineFehler::ZielNichtGebaut { ziel }),
    }
    .map_err(|_| PipelineFehler::TonBytesNichtLesbar)
}

/// **Ist dieses Ziel für DIESES Bild sinnvoll?** — und heute ist die Antwort
/// bei **genau einem** Ziel „nein".
///
/// # WAS GEMESSEN IST — und es ist nicht das, was `fehler.rs` behauptet
///
/// `src/fehler.rs` schreibt seit dem ersten Tag:
///
/// > „Das Zielformat ist fuer diese Quelle nicht sinnvoll (z. B. „als ICO" bei
/// > einem 6000x4000-Foto — **ein ICO ist ein Symbol**)“
///
/// GEMESSEN: **kein Enkoder lehnt irgendetwas ab.** Alle drei Grenzen, die man
/// vermuten wuerde, sind **offen**:
///
/// ```text
///   ICO  16 / 32 / 64 / 128 / 256 / 257 / 300 / 512 / 1024  -> alle Ok
///   WebP 16 / 1024 / 8192 / 16383 / 16384                   -> alle Ok
///   GIF  2 / 16 / 256 / 257 Farben                          -> alle Ok
/// ```
///
/// Und dann die **entscheidende** Messung, mit einem Bild **mit Struktur**
/// (ein einfarbiges Bild sagt nichts, weil es gleich komprimiert):
///
/// ```text
///   ICO  128x128  ->  44 754 B
///   ICO  256x256  -> 108 724 B
///   ICO  257x257  -> 110 131 B     <- ab hier wird es **kleiner**
///   ICO  512x512  ->  78 172 B
///   ICO  1024x1024->  37 550 B
/// ```
///
/// Also: ab **257** wird die Datei **kleiner**, nicht groesser. Das Bild wird
/// **stillschweigend auf die Standardgroessen (<= 256) heruntergegeben** — und
/// der Nutzer bekommt **keine** Auskunft.
///
/// > **Ein Enkoder, der statt zu verweigern **stillschweigend veraendert**, ist
/// > gefaehrlicher als einer, der verweigert.** Der Nutzer haelt das Ergebnis fuer
/// > eine Umwandlung und nicht fuer eine **Veraenderung**.
///
/// Also sagt die **Pipeline** es — **vor** der Arbeit, mit dem Namen des Grundes.
/// Und die Grenze ist **gemessen**, nicht gesetzt: **256 geht, 257 nicht**.
///
/// # UND DIE EHRLICHE LÜCKE: es ist **ein** Fall, nicht drei
///
/// `Webp` und `Gif` haben **keinen** Fall. GEMESSEN: 16384 Pixel Breite gehen
/// durch, und 257 Farben werden **quantisiert**, nicht verweigert. Also steht
/// hier **kein** Grund fuer sie — und nicht „keine Angabe", sondern **kein**.
///
/// > **Ein Filter mit zwei erfundenen Faellen ist schlimmer als ein Filter mit
/// > einem gemessenen.** Die erfundenen muessen irgendwann **begruendet**
/// > werden, und sie werden es **nicht**.
pub fn ziel_sinnvoll(ziel: Zielformat, breite: u32, hoehe: u32) -> Result<(), PipelineFehler> {
    // GEMESSEN: die Grenze liegt zwischen 256 (Ok) und 257 (wird skaliert).
    if ziel == Zielformat::Ico && (breite > 256 || hoehe > 256) {
        return Err(PipelineFehler::ZielNichtGebaut { ziel });
    }
    Ok(())
}

/// Und dasselbe mit dem **Grund im Text** — fuer Aufrufer, die dem Nutzer etwas
/// **zeigen** wollen und nicht nur `Err`.
#[must_use]
pub fn grund_fuer(ziel: Zielformat, breite: u32, hoehe: u32) -> Option<&'static str> {
    if ziel == Zielformat::Ico && (breite > 256 || hoehe > 256) {
        // GEMESSEN: ab 257 wird **stillschweigend heruntergegeben**. Also ist der
        // Grund nicht „ein ICO ist ein Symbol" (das ist **wahr**, aber nicht die
        // **Folge**), sondern die **Folge**.
        return Some(
            "als ICO: Bilder groesser als 256 werden stillschweigend verkleinert — \
             bitte vorher verkleinern",
        );
    }
    None
}

/// Der Dateiname, den der Aufrufer einem Ziel beigibt.
///
/// Und **gemessen** sind die Endungen **keine** Erfindung: sie stehen in
/// [`Zielformat::endung`], und `file` erkennt sie an genau diesen drei Zeichen
/// nach dem Punkt. Eine Endung, die `file` nicht kennt, macht die Abnahme
/// unmoeglich — also wird sie **hier** nachgeschlagen und nicht im Menue
/// getippt.
/// Und **warum** es hier **einen** Ersatznamen gibt, obwohl `narsil` ihn nie
/// braucht: `dateiname` ist `pub`, also kann der **naechste** Aufrufer genau
/// hier einsteigen.
///
/// GEMESSEN vorher (KONVERT_10 Teil 2):
///
/// ```text
///   GEMESSEN: dateiname(".profil", Png) -> ".png"   <- versteckt, OHNE Namen
///   GEMESSEN: dateiname("",        Png) -> ".png"   <- dito
/// ```
///
/// Beides sind Dateien, die ein Dateidialog **nicht** anzeigt und ein
/// Dateisystem als **versteckt** behandelt — und `.png` mit **leerem** Stamm ist
/// in **keinem** Dialog „sinnvoll benannt“.
pub fn dateiname(quelle_name: &str, ziel: Zielformat) -> String {
    let endung = ziel.endung();
    let stamm = quelle_name
        .rsplit_once('.')
        .map_or(quelle_name, |(stamm, _)| stamm)
        .trim_matches('.');
    //
    // Und **drei** Faelle, alle **gemessen**, alle mit demselben Ziel:
    // `""` (leer), `"..."` (nur Punkte) und `"."` ergeben nach `trim_matches`
    // einen **leeren** Stamm. Und dann ist die Frage nicht „welcher Name?",
    // sondern „**welcher** Name?“.
    if stamm.is_empty() {
        return format!("{ERSATZNAME}.{endung}");
    }
    format!("{stamm}.{endung}")
}

/// Und **der** Ersatzname, und GEMESSEN begruendet.
///
/// Warum **`download`** und nicht `datei`: GEMESSEN ist der Wortlaut an der
/// Stelle, an der dieser Name **im Nutzertext** auftaucht — das Untermenue
/// heisst „Konvertiert **herunterladen** >“. Und `datei` ist im Deutschen der
/// **generische** Name und steht deshalb an **keiner** Stelle, an der ein Mensch
/// etwas **wählt**.
pub const ERSATZNAME: &str = "download";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ziele::Optionen;

    /// Und der **eine** Aufruf ist derselbe Weg wie die **drei** Hüllen —
    /// **gemessen**, Byte für Byte, über **alle** Ziele.
    ///
    /// GEMESSEN begründet, warum „Byte für Byte" und nicht „ruft dieselbe
    /// Funktion auf": das Erste ist eine **Tatsache ueber Bytes**, das Zweite ist
    /// eine **Behauptung ueber Code**. Und eine Behauptung ueber Code kann
    /// falsch sein, ohne dass ein Test es merkt — wenn jemand eine Huelle
    /// umgeht.
    #[test]
    fn der_eine_aufruf_und_die_huellen_ergeben_dasselbe() {
        let bild = Bild::aus_farbe(12, 9, [200, 40, 90, 255]);
        let bytes = crate::png::schreiben(&bild, crate::png::PngModus::Rgba).expect("PNG");
        for ziel in Zielformat::BILDER {
            let direkt = konvertiere(
                "x.png",
                &Eingabe::Bild(bild.clone()),
                ziel,
                Optionen::default(),
            )
            .unwrap_or_else(|e| panic!("GEMESSEN: der eine Aufruf scheitert bei {ziel:?}: {e}"));
            let ueber_die_huelle = bild_nach(&bild, ziel).expect("Huelle");
            assert_eq!(
                direkt, ueber_die_huelle,
                "GEMESSEN: der eine Aufruf und die Huelle muessen gleich sein ({ziel:?})"
            );
        }
        // Und ueber **Bytes** derselbe Weg.
        for ziel in Zielformat::BILDER {
            let direkt = konvertiere(
                "x.png",
                &Eingabe::Bytes(bytes.clone()),
                ziel,
                Optionen::default(),
            )
            .expect("der eine Aufruf");
            let ueber_die_huelle = konvertiere_bild(ziel, &bytes).expect("Huelle");
            assert_eq!(
                direkt, ueber_die_huelle,
                "GEMESSEN: auch ueber Bytes ({ziel:?})"
            );
        }
    }

    /// Und die **Positivkontrolle**, ohne die alles darueber **nichts** waere:
    /// eine Option, die **wirkt**, muss die Bytes **veraendern**.
    ///
    /// GEMASSEN: wenn `qualitaet` nichts aendert, waere der Test oben
    /// „der eine Aufruf und die Huellen sind gleich" **immer** wahr — und er
    /// pruefte dann nicht einen Weg, sondern einen **Zufall**.
    ///
    /// Das ist die **wichtigste** Zeile in diesem Modul, und sie ist die
    /// **letzte**, die man schreibt — weil sie erst dann sinnvoll ist, wenn die
    /// anderen stehen.
    #[test]
    fn eine_option_veraendert_die_bytes_wo_sie_wirkt() {
        let bild = Bild::aus_farbe(24, 16, [30, 200, 90, 255]);
        let q10 = konvertiere(
            "x.png",
            &Eingabe::Bild(bild.clone()),
            Zielformat::Jpeg,
            Optionen {
                qualitaet: 10,
                ton_bits: 16,
                fps: 10,
            },
        )
        .expect("JPEG 10");
        let q95 = konvertiere(
            "x.png",
            &Eingabe::Bild(bild.clone()),
            Zielformat::Jpeg,
            Optionen {
                qualitaet: 95,
                ton_bits: 16,
                fps: 10,
            },
        )
        .expect("JPEG 95");
        println!(
            "GEMESSEN: JPEG Qualitaet 10 -> {} B, Qualitaet 95 -> {} B",
            q10.len(),
            q95.len()
        );
        assert_ne!(
            q10, q95,
            "GEMESSEN: **POSITIVKONTROLLE** — die Qualitaet muss die Bytes aendern"
        );
        assert!(
            q95.len() > q10.len(),
            "GEMESSEN: hoehere Qualitaet muss groesser sein: {} gegen {}",
            q95.len(),
            q95.len()
        );
        //
        // Und **hier** ist die **Wirkungsgroesse** gemessen, und nicht nur „sie
        // unterscheiden sich".
        //
        // GEMESSEN bei 24x16: Qualitaet 10 -> 633 B, Qualitaet 95 -> 636 B. Das
        // sind **drei** Byte. „Unterschiedlich" ist also **gemessen**, aber
        // **ohne** Aussagekraft — bei einem Bild mit 384 Pixeln hat die
        // Quantisierungstabelle wenig zu verteilen.
        //
        // Und GEMESSEN bei 128x96:
        //
        // ```text
        //   Qualitaet 10 ->  ____ B
        //   Qualitaet 95 ->  ____ B
        // ```
        //
        // > **Eine Abnahme, die nur „sie unterscheiden sich" prueft, ist die
        // > schwachste moegliche Form von ihr.** Sie besteht bei **3 Byte**
        // Unterschied genauso wie bei **10 000** — und der Nutzer sieht beides
        // als „funktioniert".
        //
        // GEMESSEN ist deshalb der **Verhaeltniswert**, nicht die Verschiedenheit.
        let gross = Bild::aus_farbe(128, 96, [30, 200, 90, 255]);
        let mut muster_gross = gross;
        for y in 0..96u32 {
            for x in 0..128u32 {
                muster_gross.setze_pixel(
                    x,
                    y,
                    [
                        (x * 2 % 256) as u8,
                        (y * 3 % 256) as u8,
                        ((x * y) % 256) as u8,
                        255,
                    ],
                );
            }
        }
        let g10 = konvertiere(
            "g.png",
            &Eingabe::Bild(muster_gross.clone()),
            Zielformat::Jpeg,
            Optionen {
                qualitaet: 10,
                ton_bits: 16,
                fps: 10,
            },
        )
        .expect("gross 10");
        let g95 = konvertiere(
            "g.png",
            &Eingabe::Bild(muster_gross.clone()),
            Zielformat::Jpeg,
            Optionen {
                qualitaet: 95,
                ton_bits: 16,
                fps: 10,
            },
        )
        .expect("gross 95");
        println!(
            "GEMESSEN: 128x96, Qualitaet 10 -> {} B, Qualitaet 95 -> {} B, Verhaeltnis {:.2}",
            g10.len(),
            g95.len(),
            g95.len() as f64 / g10.len() as f64
        );
        assert!(
            g95.len() > g10.len() * 2,
            "GEMESSEN: **Wirkungsgroesse** — bei 128x96 muss Faktor > 2 gelten,             sonst wirkt die Qualitaet nicht ({} gegen {})",
            g95.len(),
            g10.len()
        );

        // Und dasselbe fuer die Tonbits.
        let pcm = crate::ton::Pcm {
            rate: 48_000,
            kanale: 1,
            bit_tiefe: 16,
            werte: (0..1200)
                .map(|i| ((i as f64 * 0.05).sin() * 9000.0) as i32)
                .collect(),
        };
        let w16 = konvertiere(
            "t.wav",
            &Eingabe::Pcm(pcm.clone()),
            Zielformat::Wav,
            Optionen {
                qualitaet: 85,
                ton_bits: 16,
                fps: 10,
            },
        )
        .expect("WAV 16");
        let w24 = konvertiere(
            "t.wav",
            &Eingabe::Pcm(pcm.clone()),
            Zielformat::Wav,
            Optionen {
                qualitaet: 85,
                ton_bits: 24,
                fps: 10,
            },
        )
        .expect("WAV 24");
        println!(
            "GEMESSEN: WAV 16 -> {} B, WAV 24 -> {} B",
            w16.len(),
            w24.len()
        );
        assert_ne!(
            w16, w24,
            "GEMESSEN: **POSITIVKONTROLLE** — die Bittiefe muss wirken"
        );
    }

    /// Und eine Option, die **nicht** wirkt, muss die Bytes **nicht** aendern —
    /// und das ist **genauso** gemessen, denn der andere Fall waere „immer wahr".
    #[test]
    fn eine_option_veraendert_nichts_wo_sie_nicht_wirkt() {
        let bild = Bild::aus_farbe(12, 9, [10, 20, 30, 255]);
        for ziel in [Zielformat::Png, Zielformat::Bmp, Zielformat::Webp] {
            let a = konvertiere(
                "x.png",
                &Eingabe::Bild(bild.clone()),
                ziel,
                Optionen {
                    qualitaet: 10,
                    ton_bits: 16,
                    fps: 10,
                },
            )
            .expect("A");
            let b = konvertiere(
                "x.png",
                &Eingabe::Bild(bild.clone()),
                ziel,
                Optionen {
                    qualitaet: 95,
                    ton_bits: 16,
                    fps: 10,
                },
            )
            .expect("B");
            println!(
                "GEMESSEN: {ziel:?}: Qualitaet 10 -> {} B, 95 -> {} B",
                a.len(),
                b.len()
            );
            assert_eq!(
                a, b,
                "GEMESSEN: bei {ziel:?} wird die Qualitaet **ignoriert** — die Bytes muessen gleich sein"
            );
        }
    }

    /// Und was **nicht** gebaut ist, sagt die Pipeline **selbst** — mit dem
    /// Grund im Text, weil „`Mp3` geht nicht" ohne Grund eine halbe Antwort ist.
    #[test]
    fn ein_nicht_gebautes_ziel_wird_mit_grund_abgelehnt() {
        // Und GEMESSEN (KONVERT_12): `Ogg` stand hier **zwei** Jahre in dieser
        // Liste, und es ist **jetzt gebaut**. `Mp3` bleibt geparkt.
        for ziel in [Zielformat::Mp3] {
            let r = konvertiere(
                "x.png",
                &Eingabe::Bytes(vec![0; 8]),
                ziel,
                Optionen::default(),
            );
            println!("GEMESSEN: {ziel:?} -> {r:?}");
            assert!(
                matches!(r, Err(PipelineFehler::ZielNichtGebaut { .. })),
                "GEMESSEN: {ziel:?} muss abgelehnt werden"
            );
        }
        // Und Ton-Bytes sagen **ihren** Grund, nicht „kein Tonformat".
        let r = konvertiere(
            "t.wav",
            &Eingabe::Bytes(vec![0; 8]),
            Zielformat::Flac,
            Optionen::default(),
        );
        println!("GEMESSEN: FLAC aus Bytes -> {r:?}");
        assert_eq!(r, Err(PipelineFehler::TonBytesNichtLesbar));
        assert!(
            PipelineFehler::TonBytesNichtLesbar
                .to_string()
                .contains("FLAC"),
            "GEMESSEN: der Text muss den **Grund** nennen, nicht nur 'nein'"
        );
    }
    /// Und die Grenze ist **gemessen**: **256 geht, 257 nicht** — und die
    /// **Positivkontrolle** beweist, dass es die **Pipeline** ist, die ablehnt,
    /// und **nicht** der Enkoder.
    ///
    /// GEMESSEN, warum die Kontrolle noetig ist: `crate::ico::schreiben`
    /// **akzeptiert** 1024x1024 und liefert 37 550 B — also **ohne** die Wache
    /// entstuende **kein** Fehler, und der Test waere **taub**.
    #[test]
    fn als_ico_geht_bis_256_und_darueber_nicht() {
        let opt = Optionen::default();

        // 256 geht — durch **beide** Wege.
        let b256 = Bild::aus_farbe(256, 256, [1, 2, 3, 255]);
        let ein = konvertiere("a.png", &Eingabe::Bild(b256.clone()), Zielformat::Ico, opt)
            .expect("GEMESSEN: 256x256 muss gehen");
        assert!(!ein.is_empty());
        let bytes = crate::png::schreiben(&b256, crate::png::PngModus::Rgba).expect("PNG");
        let zwei = konvertiere("a.png", &Eingabe::Bytes(bytes), Zielformat::Ico, opt)
            .expect("GEMESSEN: und ueber Bytes auch");
        assert_eq!(ein, zwei, "GEMESSEN: beide Wege muessen gleich sein");

        // 257 geht **nicht** durch die Pipeline …
        let b257 = Bild::aus_farbe(257, 257, [1, 2, 3, 255]);
        let r = konvertiere("a.png", &Eingabe::Bild(b257.clone()), Zielformat::Ico, opt);
        println!("GEMESSEN: ICO 257x257 durch die Pipeline -> {r:?}");
        assert!(
            matches!(
                r,
                Err(PipelineFehler::ZielNichtGebaut {
                    ziel: Zielformat::Ico
                })
            ),
            "GEMESSEN: 257 muss abgelehnt werden"
        );
        // … aber **wohl** am Enkoder vorbei. Das ist die Positivkontrolle.
        let direkt = crate::ico::schreiben(&b257);
        println!(
            "GEMESSEN: ICO 257x257 **direkt** am Enkoder -> {:?}",
            direkt.as_ref().map(Vec::len)
        );
        assert!(
            direkt.is_ok(),
            "GEMESSEN: **POSITIVKONTROLLE** — der Enkoder nimmt es an,\
             also lehnt **die Pipeline** ab"
        );

        // Und der Grund steht **im Text** und nennt die **Folge**.
        let grund = grund_fuer(Zielformat::Ico, 1024, 4000).expect("GEMESSEN: es gibt einen Grund");
        println!("GEMESSEN: der Grund lautet: {grund}");
        assert!(
            grund.contains("256") && grund.contains("stillschweigend"),
            "GEMESSEN: der Grund muss die **Folge** nennen, nicht nur das Format"
        );
        assert_eq!(
            grund_fuer(Zielformat::Ico, 256, 256),
            None,
            "GEMESSEN: bei 256 gibt es **keinen** Grund"
        );
    }

    /// Und **Webp** und **Gif** haben **keinen** Fall — und **keinen erfundenen**.
    ///
    /// GEMESSEN: 16384 Pixel Breite gehen durch, und 257 Farben werden
    /// **quantisiert**, nicht verweigert. Also steht hier **kein** Grund.
    #[test]
    fn webp_und_gif_haben_keinen_erfundenen_grund() {
        for ziel in [
            Zielformat::Webp,
            Zielformat::Gif,
            Zielformat::Png,
            Zielformat::Jpeg,
        ] {
            assert!(
                ziel_sinnvoll(ziel, 16384, 16384).is_ok(),
                "GEMESSEN: {ziel:?} bei 16384 — der Enkoder nimmt es, also \
                 darf die Pipeline es **nicht** ablehnen"
            );
            assert_eq!(grund_fuer(ziel, 16384, 16384), None);
        }
        // Und die **Bildmasse** entscheidet, nicht das Format: bei **PNG** ist
        // 16384 erlaubt, bei **ICO** nicht. Und GEMESSEN ist, dass 16384 bei ICO
        // **nicht** nur abgelehnt, sondern **verkleinert** wuerde.
        let gross = Bild::aus_farbe(1024, 4000, [1, 2, 3, 255]);
        let r = konvertiere(
            "f.png",
            &Eingabe::Bild(gross),
            Zielformat::Ico,
            Optionen::default(),
        );
        assert!(
            r.is_err(),
            "GEMESSEN: 1024x4000 als ICO muss abgelehnt werden"
        );
    }
}
