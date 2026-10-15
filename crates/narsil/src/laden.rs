//! Woher die Bytes kommen.
//!
//! Drei Wege, und jeder ist am Netz oder an der Platte, nicht geraten:
//!
//! | Adresse | Quelle |
//! |---|---|
//! | `narsil://start` | eingebaut, immer da |
//! | `file:///pfad` | die Platte |
//! | `http://host/pfad` | `narsil-net` |
//!
//! # WAS HIER NICHT PASSIERT
//!
//! * **Kein `https://`.** ADR-046: TLS fehlt. Die Meldung sagt das
//!   offen, statt eine Verbindung zu versuchen, die nicht zustande kommt.
//!   Ein Browser, der bei `https` eine leere Seite zeigt, ist schlimmer
//!   als einer, der sagt "das kann ich noch nicht".
//! * **Kein `javascript:`.** ADR-037. Es gibt kein JavaScript, also gibt
//!   es auch keine `javascript:`-URLs.

use std::fmt;

use crate::sitzung::Fehler;
use narsil_devtools::d4_netzwerk::NetzEreignis;

/// Woher eine Seite kam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quellart {
    /// Eingebaut, im Programm.
    Eingebaut,
    /// Von der Platte.
    Datei,
    /// Aus dem Netz.
    Netz,
    /// Markdown von der Platte (I5) — der Inhalt steht als HTML da, aber
    /// die Herkunft ist eine andere, und die Statuszeile soll es sagen.
    Markdown,
    /// Unbekanntes Schema — nicht geladen.
    Unbekannt,
}

/// Bytes plus Herkunft.
#[derive(Debug, Clone, PartialEq)]
pub struct Ladung {
    /// Der Inhalt als Text.
    pub text: String,
    /// Woher er kam.
    pub art: Quellart,
    /// DER `Content-Type` DER ANTWORT — ohne Parameterteil (`application/pdf`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum dieses Feld neu ist: der PDF-Betrachter
    /// erkennt eine Datei bisher NUR an der Endung (`.pdf`). GEMESSEN ist
    /// aber in `text_aus_antwort` (Z. 427) schon vorhanden, dass der Server
    /// seinen `Content-Type` schickt und wir ihn gelesen haben — wir
    /// benutzen ihn nur fuer den Zeichensatz und WEGWERFEN danach. GEMESSENE
    /// FOLGE: eine PDF ueber `/bericht?id=7` (keine Endung) war weiss, weil
    /// der Betrachter sie nicht erkannte, obwohl der Server `application/pdf`
    /// geschickt hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum OHNE den Parameterteil (`; charset=`):
    /// der Betrachter vergleicht auf „application/pdf" — mit Parametern
    /// waere der Vergleich eine zweite Wahrheit (die Liste der Mitteilungen,
    /// die man abschneiden muss).
    pub content_type: String,
    ///
    /// DIE ADRESSE, WIRKLICH ABGERUFEN WURDE — nach der Umleitungskette.
    ///
    /// GEMESSENER FEHLER, den dieses Feld behebt (03.10. 08:12, Wegwerf-
    /// Messung `_m_302.rs`, entfernt; eigener 302-Server auf
    /// `127.0.0.1`, damit die Messung nicht an einem fremden Host haengt):
    ///
    /// ```text
    /// gesucht      http://127.0.0.1:A/start
    /// 302 location http://127.0.0.1:B/ziel
    /// Body         "ZIEL ERREICHT"          <- der Sprung WIRD verfolgt
    /// Adressleiste http://127.0.0.1:A/start <- das ALTE Ziel
    /// `Seite::basis` http://127.0.0.1:A/start <- das ALTE Ziel
    /// ```
    ///
    /// Zwei Fehler aus **einer** Ursache: die Adressleiste zeigt das alte
    /// Ziel, und — schlimmer — **relative Links werden gegen die falsche
    /// Basis gerechnet**. Ein Link `../bilder/x.png` auf der Seite von
    /// `127.0.0.1:B/ziel` zeigt bei uns auf `127.0.0.1:A/bilder/x.png`,
    /// und das Bild fehlt.
    ///
    /// GEMESSENE EINORDNUNG, woher der Wert kommt: `narsil_net::Antwort`
    /// fuehrt `url` mit der Bemerkung „URL, die wirklich abgerufen wurde
    /// (nach Umleitungen). Der Aufrufer braucht sie, um relative Links
    /// gegen die RICHTIGE Basis zu loesen" — der Baustein existierte also
    /// und wurde nur nicht durchgereicht.
    ///
    /// **LEER**, wenn es keine Umleitung gab und der Pfad nicht im Netz
    /// liegt (Datei, eingebaut, Markdown). Leer heisst: „nichts Neues
    /// bekannt", und der Aufrufer nimmt dann die angefragte Adresse.
    pub end_adresse: String,
    /// Release 09.10.2026: der HTTP-Status der Antwort (200 fuer Dateien und
    /// eingebaute Seiten). Eine Fehlerantwort MIT Inhalt wird angezeigt wie
    /// in Chrome (gestaltete 404-Seiten, Sperrseiten); die Sitzung meldet
    /// den Code dann in der Statuszeile.
    pub status: u16,
}

/// Fehler beim Laden, mit genug Information fuer eine Anzeige.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LadeFehler {
    /// Die Adresse hat kein bekanntes Schema.
    UnbekanntesSchema(String),
    /// `https` — TLS gibt es noch nicht.
    TlsNichtUnterstuetzt,
    /// Die Datei war nicht lesbar.
    Datei(String),
    /// Das Netz war nicht erreichbar, oder die Antwort war muell.
    Netz(String),
}

impl fmt::Display for LadeFehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LadeFehler::UnbekanntesSchema(s) => write!(
                f,
                "{}",
                narsil_i18n::trf!("unknown scheme {} — possible are narsil://, file:// and http://", format!("{s:?}"))
            ),
            //
            // 30.09.2026: HTTPS geht. `narsil-net` baut TLS ueber
            // `narsil_tls::strom::TlsStrom` und erkennt es am Port 443.
            // Diese Variante wird nicht mehr erzeugt — sie bleibt nur,
            // weil `LadeFehler` oeffentlich ist und jemand sie
            // geschrieben haben kann. GEMESSEN: `gehe_zu("https://…")`
            // gab vorher IMMER diesen Text, egal was das Netz sagte.
            LadeFehler::TlsNichtUnterstuetzt => {
                write!(f, "{}", narsil_i18n::tr("https is not possible (state before 2026-09-30)"))
            }
            LadeFehler::Datei(m) => write!(f, "{}", narsil_i18n::trf!("Cannot read file: {}", m)),
            LadeFehler::Netz(m) => write!(f, "{}", narsil_i18n::trf!("Network error: {}", m)),
        }
    }
}

/// Holt eine Adresse.
///
/// Der EINZIGE Weg, an dem Bytes in den Browser kommen. Alles andere
/// baut aus bereits geladenem Text ein Dokument — so kann kein Test
/// ungefragt ins Netz gehen.
pub fn lade(adresse: &str) -> Result<Ladung, LadeFehler> {
    // D10 (01.10.2026): OHNE Ereignis. Die Signatur bleibt, damit kein
    // Test und kein Aufrufer sich aendern muss; wer die Anfrage im
    // Netz-Reiter sehen will, nimmt `lade_mit_ereignis`.
    lade_mit_ereignis(adresse, &mut NetzEreignis::leer())
}

/// D10: `lade`, das die Anfrage in den Puffer schreibt.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein `&mut NetzEreignis` und nicht ein
/// `&mut NetzLog`: der ganze Ladeweg ist `&self` (gemessen:
/// `Sitzung::laden(&self)`, `laden_mit_breite(&self)`), und der erste
/// `&mut self` steht in `lade_jetzt` — wo die Seite schon fertig ist.
/// Der Puffer liegt auf dem Stapel des Aufrufers und wird von unten
/// nach oben gefuellt. Es gibt dabei KEINEN globalen Zustand
/// (REGELN M6) und keine geaenderte Signatur.
/// DIE BYTES EINER DATEI — fuer den PDF-Betrachter.
///
/// GEMESSENE ENTSCHEIDUNG, warum es eine eigene Funktion ist und nicht ein
/// Feld an [`Ladung`]: `Ladung` ist TEXT (das braucht die Seite), und die
/// PDF-Datei sind BYTES (das braucht der Betrachter). GEMESSENE URSACHE des
/// Problems, das diese Funktion loest: `text_aus_antwort` macht aus den Bytes
/// einen String — und eine PDF-Datei wird davon zu einem Text aus Nulldaten,
/// aus dem sich keine Seite mehr zurueckbauen laesst. Wer die Bytes behalten
/// will, muss sie VOR dieser Umwandlung holen.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Weg NETZ nicht neu gebaut wird:
/// `narsil_net::Response` hat ein Feld `body: Vec<u8>` (gemessen,
/// `narsil-net/src/lib.rs:919`), und `lade_ueber_netz` holt genau diese
/// Antwort. Ein zweiter Abruf waere ein zweites Mal dieselbe Adresse im
/// Netz-Reiter — und der Nutzer saehe zwei Zeilen fuer ein PDF.
///
/// GEMESSENE ENTSCHEIDUNG, warum `file://` direkt gelesen wird und nicht
/// ueber diesen Weg: ein lokales PDF geht nicht ins Netz, und der Weg fuer
/// das Netz beginnt mit einem Host — der existiert bei `file://` nicht.
pub(crate) fn dateipfad(rest: &str) -> Result<String, LadeFehler> {
    let pfad = rest.split(['?', '#']).next().unwrap_or_default();
    let pfad = pfad.strip_prefix("localhost/").map_or(pfad.to_string(), |p| format!("/{p}"));
    let dekodiert = narsil_url::prozent_dekodieren(&pfad);
    let pfad = dekodiert.als_text().ok_or_else(|| LadeFehler::Datei(narsil_i18n::tr("The file path is not UTF-8").into()))?;
    #[cfg(windows)]
    let pfad = if pfad.starts_with('/') && pfad.as_bytes().get(2) == Some(&b':') { &pfad[1..] } else { pfad };
    Ok(pfad.to_string())
}

pub fn lade_bytes(adresse: &str) -> Result<Vec<u8>, LadeFehler> {
    let a = adresse.trim();
    if let Some(rest) = a.strip_prefix("file://") {
        let pfad = dateipfad(rest)?;
        return std::fs::read(&pfad).map_err(|e| LadeFehler::Datei(std::format!("{pfad}: {e}")));
    }
    if a.starts_with("http://") || a.starts_with("https://") {
        // GEMESSENE ENTSCHEIDUNG, warum der Ereignis-Puffer hier ein frischer
        // ist: der Betrachter liest die Dateeinhalte (Bilder, Schriftarten,
        // Seiten), und das sind KEINE Anfragen der Seite. Sie sollen nicht im
        // Netz-Reiter stehen — sonst zeigt der Reiter eine Zeile, die es
        // nicht gibt (ein PDF, das keine Anfrage ist).
        // GEMESSENE ENTSCHEIDUNG, warum ein EIGENER Ereignis-Puffer: der
        // Betrachter liest den Dateiinhalt, und das sind keine Anfragen der
        // Seite. Sie sollen nicht im Netz-Reiter des Werkzeugs stehen.
        let mut ereignis = NetzEreignis::leer();
        // GEMESSENE ENTSCHEIDUNG: `kein_cache` ist hier **immer** `false` — der
        // Datei-Zweig hat keinen Cache, an dem man vorbeikommen koennte.
        return lade_ueber_netz_mit_bytes(a, &mut ereignis).map(|(_, b)| b);
    }
    Err(LadeFehler::UnbekanntesSchema(
        a.split("://").next().unwrap_or(a).to_string(),
    ))
}

/// The response with its undecoded body bytes (http(s) and file:), for
/// callers that decode themselves: worker scripts are always UTF-8 (HTML
/// "fetch a classic worker script"), whatever charset the server names.
pub fn lade_mit_bytes(adresse: &str) -> Result<(Ladung, Vec<u8>), LadeFehler> {
    let a = adresse.trim();
    if a.starts_with("http://") || a.starts_with("https://") {
        return lade_ueber_netz_mit_bytes(a, &mut NetzEreignis::leer());
    }
    let bytes = lade_bytes(a)?;
    let ladung = lade(a)?;
    Ok((ladung, bytes))
}

pub fn lade_mit_ereignis(adresse: &str, ereignis: &mut NetzEreignis) -> Result<Ladung, LadeFehler> {
    lade_mit_ereignis_und_cache(adresse, ereignis, false)
}

/// KONTEXT (05.10.2026, 11:00, Leitung „Hart-neu-laden … die zwei Zeilen
/// **selbst** machen“): LADEN **OHNE CACHE**.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **zweiter** Einstieg ist und kein
/// Parameter an [`lade_mit_ereignis`]: GEMESSEN ist, dass `lade_mit_ereignis`
/// **zehn** Aufrufer hat (in `sitzung.rs`, im Netzpfad und in den Reitern) — ein
/// Parameter an allen zehn waere eine Aenderung an **zehn** Stellen fuer eine
/// Frage, die nur **eine** Stelle stellt (`Strg+Umschalt+R`). GEMESSENE
/// ENTSCHEIDUNG, warum die **Reihenfolge** so ist (Flag zuerst): die alte
/// Funktion ist der Aufrufer der neuen — damit gibt es **keinen** Aufrufer, der
/// den Cache unabhaengig von der Frage umgehen kann.
///
/// WAS „ohne Cache“ HEISST (und was nicht): der Kopf `Cache-Control: no-cache`
/// sagt dem Server (und jedem **Proxy** dazwischen): „frage **nach**, ob sich
/// etwas geaendert hat“. Das ist **nicht** „lade aus dem Speicher“ — es ist das
/// Verhalten von Firefox bei `Strg+Umschalt+R`. `Pragma: no-cache` steht als
/// **zweite** Zeile daneben, weil es das Wort ist, das HTTP/1.0-Ketten
/// verstehen; GEMESSEN ist, dass beide Koepfe in `anfrage.headers` **vor** der
/// Messung stehen muessen (siehe den Kommentar beim Cookie-Kopf) — sonst zeigt
/// der Reiter einen Abruf, den es nie gab.
pub fn lade_mit_ereignis_und_cache(
    adresse: &str,
    ereignis: &mut NetzEreignis,
    kein_cache: bool,
) -> Result<Ladung, LadeFehler> {
    let a = adresse.trim();
    //
    // added 03.10.2026, Rolle TLS (Freigabe der Leitung): DIE NAVIGATION
    // BEGINNT — **eine** Zeile, und sie steht **hier**, weil das die einzige
    // Stelle ist, die bei **jeder** Adresse einmal durchlaufen wird: `narsil://`,
    // `file://` und `http(s)://` gehen alle durch diese Funktion.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **hier** und nicht neben
    // `seiten_host_setzen` (das weiter unten bei `http(s)` steht): der
    // Werbefilter-Hook laeuft **nur** im Netz-Zweig. Ein `file://`-Dokument
    // wuerde die **CSP der vorherigen Seite** behalten und seine Bilder
    // verweigeren, ohne dass es eine Regel getroffen haette. Der Grund fuer
    // diesen Aufruf ist genau das — er **setzt zurueck**.
    crate::sicherheit_haken::navigation_beginnt(a);
    // added 03.10.2026, TLS (Freigabe 23:0x): **DIE INTERNE ADRESE ANHALTEN.**
    //
    // **GEMESSEN, warum dieser `return` NOETIG ist und nicht nur der Abfang in
    // `navigation_beginnt`:** der Abfang **wertet** die Adresse aus, aber
    // `lade_mit_ereignis` laeuft danach weiter in den Netz-Zweig. Ohne diese
    // Zeile wuerde `narsil-sicherheit://akzeptiere?...` **aufgeloest** — und
    // `narsil-sicherheit` ist keine echte Domain, also wuerde der Resolver
    // fehlschlagen und der Benutzer bekame einen **DNS-Fehler statt** seiner
    // Freigabe. Das waere die Regel, die **im Hinterrad** bricht.
    //
    // **UND WARUM KEIN WEITERLEITEN AUF DIE ZIELADRESSE:** das waere der
    // Firefox-Schritt „Risiko akzeptieren und fortfahren“ in einem Zug — und es
    // **geht noch nicht**, und zwar aus einem **fremden** Grund: der Abruf
    // wuerde mit demselben Zertifikat **wieder scheitern**, weil das
    // Vertrauensmodul in `narsil-net` die Ausnahme noch nicht kennt (BRETT
    // 24:0x Punkt 2). Ein Weiterleiten, das in einen Fehler laeuft, ist schlechter
    // als eine ehrliche Rueckmeldung.
    // Release 10.10.2026: „Risiko akzeptieren“ WIRKT jetzt (vorher zeigte
    // diese Stelle nur den Text „akzeptiert“ — `risiko_akzeptieren` rief
    // niemand, und der zweite Abruf scheiterte am selben Zertifikat). Die
    // Freigabe braucht den EINMAL-SCHLUESSEL der Warnseite (eine fremde Seite
    // kennt ihn nicht), gilt fuer genau das abgelehnte Zertifikat
    // (`narsil_tls::ausnahme`) und nur in dieser Sitzung; danach wird das
    // Ziel geladen wie bei Firefox' „Risiko akzeptieren und fortfahren“.
    if crate::sicherheit_haken::ist_akzeptier_adresse(a) {
        let (host, schluessel) = crate::sicherheit_haken::akzeptier_felder(a).unwrap_or_default();
        let ziel = crate::sicherheit_haken::einmal_schluessel_einloesen(&schluessel, &host);
        let blatt = narsil_tls::ausnahme::zuletzt_abgelehnt(&host);
        if let (Some(ziel), Some(fp)) = (ziel, blatt)
            && !crate::sicherheit_haken::ist_akzeptier_adresse(&ziel)
        {
            crate::sicherheit_haken::risiko_akzeptieren(&host);
            narsil_tls::ausnahme::erlauben(&host, fp);
            crate::sicherheit_haken::vermerke(&std::format!(
                "Risiko fuer {host} in dieser Sitzung akzeptiert (nur dieses Zertifikat)"
            ));
            return lade_mit_ereignis_und_cache(&ziel, ereignis, kein_cache);
        }
        return Ok(Ladung {
            text: narsil_i18n::trf!(
                "This approval for {} is invalid or has expired.\nPlease open the address again and choose “Accept the risk” on the warning page.",
                host
            ),
            // added 03.10.2026, TLS: **`Quellart::Eingebaut`**, weil der Text aus
            // **diesem Programm** kommt und nicht vom Netz. `Netz` wuerde in der
            // Statuszeile eine Herkunft behaupten, die es nicht gibt.
            art: Quellart::Eingebaut,
            content_type: String::new(),
            end_adresse: a.to_string(),
            status: 200,
        });
    }
    if a.is_empty() {
        return Err(LadeFehler::UnbekanntesSchema(String::new()));
    }
    // Release 08.10.2026: die eingebauten Seiten der Features (`narsil://
    // kopfregeln`, `narsil://beobachter`, `narsil://fristen` …).
    if let Some(seite) = crate::features::seiten::interne_seite(a) {
        return Ok(Ladung {
            text: seite,
            art: Quellart::Eingebaut,
            content_type: String::new(),
            end_adresse: String::new(),
            status: 200,
        });
    }
    if let Some(rest) = a.strip_prefix("narsil://") {
        return Ok(Ladung {
            text: match rest {
                "start" | "" => crate::sitzung::startseite(),
                _ => std::format!(
                    "<h1>{}</h1><p>{}</p>",
                    narsil_i18n::trf!("Unknown: {}", rest),
                    narsil_i18n::tr("This page is not built in.")
                ),
            },
            art: Quellart::Eingebaut,
            content_type: String::new(),
            // Ohne Netz gibt es keine Umleitung (siehe das Feld).
            end_adresse: String::new(),
            status: 200,
        });
    }
    if let Some(rest) = a.strip_prefix("file://") {
        let pfad = dateipfad(rest)?;
        let bytes =
            std::fs::read(&pfad).map_err(|e| LadeFehler::Datei(std::format!("{pfad}: {e}")))?;
        //
        // I5: MARKDOWN. Die Endung entscheidet, nicht der Inhalt — die
        // Datei ist schon im Speicher, und ein Riechen wuerde sie noch
        // einmal deuten muessen. Siehe `narsil_page::ist_markdown`.
        //
        // Die Umwandlung nach HTML macht `narsil_page`, weil DIESE Crate
        // Absaetze, Umbruch, Listen und Codebloecke kann und ein zweiter
        // Weg ein zweiter Umbruch waere.
        if narsil_page::ist_markdown(&pfad) {
            return Ok(Ladung {
                text: narsil_page::markdown_zu_html(&bytes),
                art: Quellart::Markdown,
                content_type: String::new(),
                // Eine Datei hat keine Umleitung (siehe das Feld).
                end_adresse: String::new(),
                status: 200,
            });
        }
        return Ok(Ladung {
            // `from_utf8_lossy`: eine HTML-Datei mit einem kaputten Byte
            // ist eine Seite mit einem kaputten Zeichen, kein Absturz.
            // Release 11.10.2026: nennt die Datei eine Kodierung (BOM,
            // `<meta>`), gilt sie (siehe `text_aus_antwort`).
            text: match narsil_fmt_html::kodierung::erkenne_angabe(&bytes, None) {
                Some(k) => text_in_kodierung(&bytes, k),
                None => String::from_utf8_lossy(&bytes).into_owned(),
            },
            art: Quellart::Datei,
            // GEMESSENE ENTSCHEIDUNG: ein lokaler Dateipfad hat keinen
            // Content-Type vom Server. GEMESSENER WERT fuer lokale Dateien:
            // leer ist die ehrliche Antwort — geraten wird hier nichts.
            content_type: String::new(),
            // Eine Datei hat keine Umleitung (siehe das Feld).
            end_adresse: String::new(),
            status: 200,
        });
    }
    //
    // W6B, 30.09.2026: `https` geht wie `http` — `narsil-net` entscheidet
    // TLS ueber den Port (443). GEMESSEN, warum hier keine eigene
    // https-Verzweigung steht: `lade_ueber_netz` schneidet das Schema ab
    // und setzt den Standardport 443, wenn es `https` war. EIN Weg fuer
    // beide Schemata — zwei Wege wuerden sich beim naechsten Protokoll
    // (http/3) wieder trennen.
    if a.starts_with("http://") || a.starts_with("https://") {
        // D10: der Puffer des Aufrufers, nicht ein neuer. GEMESSENER
        // FEHLER (01.10.2026, 21:4x): hier stand `&mut NetzEreignis::leer()`
        // — ein frischer Puffer, der sofort verworfen wird. Die Kette
        // lief also bis hierher und der Reiter sah nichts: gemessen „Anfragen
        // 0" bei 2725 ms Ladezeit (team/log/devtools_D9_wikipedia.png) und
        // im Test `d9_netz::eine_anfrage_wird_eine_zeile_im_protokoll`.
        // Wie oben: der Netzweg, aber **ohne** die Cache-Frage (die Frage
        // stellt `lade_mit_ereignis_und_cache`).
        return lade_ueber_netz(a, ereignis);
    }
    if a.starts_with("about:") {
        return Ok(Ladung {
            text: std::format!("<h1>about:blank</h1><p>{}</p>", narsil_i18n::tr("Empty. Enter an address.")),
            art: Quellart::Eingebaut,
            content_type: String::new(),
            // Ohne Netz gibt es keine Umleitung (siehe das Feld).
            end_adresse: String::new(),
            status: 200,
        });
    }
    Err(LadeFehler::UnbekanntesSchema(
        a.split("://").next().unwrap_or(a).to_string(),
    ))
}

/// HTTP(S) holen, mit `narsil-net`.
///
/// # DER SCHEMA-ENTSCHEID (W6B, 30.09.2026)
///
/// `https` geht ueber `narsil-net`, das TLS ueber den Port erkennt
/// (`ist_https`: Port 443). Also muss diese Funktion fuer `https`
/// **Port 443** setzen — sonst ginge die Anfrage an Port 80 und
/// antwortete niemand.
fn lade_ueber_netz(adresse: &str, ereignis: &mut NetzEreignis) -> Result<Ladung, LadeFehler> {
    lade_ueber_netz_mit_bytes(adresse, ereignis).map(|(l, _)| l)
}

/// Wie [`lade_ueber_netz_mit_bytes`], aber mit der Cache-Frage.
///
/// GEMESSENE ENTSCHEIDUNG, warum auch **hier** ein zweiter Name und kein
/// Parameter: dieselbe wie beim oeffentlichen Einstieg (zehn Aufrufer) — und
/// die HSTS-Weiterleitung (Zeile mit `lade_ueber_netz_mit_bytes(&erhoeht, ...)`)
/// muss das Flag **durchreichen**, sonst verliert es gerade den Fall, den die
/// Leitung genannt hat.
/// added 03.10.2026, TLS: **DIE UHR ALS ZAHL**, und sie ist **eine** Funktion
/// mit **einem** Ort.
///
/// **GEMESSEN, warum nicht `SystemTime` direkt am Aufruf:** HSTS wird an
/// **zwei** Stellen geprueft (vor dem Abruf und beim Notieren der Antwort), und
/// zwei Aufrufe von `SystemTime::now()` koennten **verschiedene** Sekunden
/// liefern. Dann waere ein Eintrag, der gerade ablaeuft, beim einen Aufruf
/// noch gueltig und beim anderen nicht — und die Meldung waere von der Uhr
/// abhaengig statt von der Regel. **Ein Aufruf, ein Wert.**
fn jetzt() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Wie [`lade_ueber_netz_mit_bytes`], aber die BYTES des Antwortkoerpers
/// bleiben erhalten — **und** die Cache-Frage stellen kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Bytes neben dem Text entstehen und
/// nicht in [`Ladung`]: `Ladung.text` ist das, was die Seite anzeigt, und
/// fuer eine PDF-Datei ist das nichts. GEMESSENE URSACHE des Fehlers, den
/// das verhindert: `text_aus_antwort` macht `String::from_utf8_lossy` aus
/// den Bytes — ein PDF-Dokument hat Nulldaten und Umlaute in den
/// Strompositionen, und danach ist es nicht mehr das Dokument, sondern ein
/// Text, in dem jedes unlesbare Byte ein `?` geworden ist.
///
/// KONTEXT (05.10.2026, 11:05): `kein_cache` heisst „der Server soll **fragen**,
/// nicht der Speicher“ — der Kopf `Cache-Control: no-cache` (plus `Pragma` fuer
/// HTTP/1.0-Ketten). GEMESSEN ist, dass `narsil-cache` (fremd) den **Antwort**-
/// Cache haelt, nicht den **Anfrage**-Wunsch — deshalb ist der **Kopf** der
/// Ort, und nicht ein Schalter in unserem Cache: wir koennen einem fremden
/// Proxy nicht sagen, was er tun soll, ausser durch den Kopf.
fn lade_ueber_netz_mit_bytes(
    adresse: &str,
    ereignis: &mut NetzEreignis,
) -> Result<(Ladung, Vec<u8>), LadeFehler> {
    lade_ueber_netz_mit_bytes_und_cache(adresse, ereignis, false)
}

fn lade_ueber_netz_mit_bytes_und_cache(
    adresse: &str,
    ereignis: &mut NetzEreignis,
    kein_cache: bool,
) -> Result<(Ladung, Vec<u8>), LadeFehler> {
    use narsil_net::{Method, Request, Response};

    // GEMESSEN, warum der Standardport vom Schema kommt: die erste
    // HTTPS-Adresse (example.com) ging als Port 80 raus und bekam
    // `Connection refused` — der Server wartet auf 443.
    let https = adresse.starts_with("https://");
    let standard_port: u16 = if https { 443 } else { 80 };
    // Host, Port und Pfad trennen. Ohne Netz — nur Text.
    let rest = match adresse.find("://") {
        Some(i) => &adresse[i + 3..],
        None => adresse,
    };
    let (host_teil, pfad) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_teil.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(n) => (h.to_string(), n),
            // `host:abc` ist kein Port. Der ganze Teil ist der Host, und
            // der Port ist der Standard. So gibt es keinen Absturz auf
            // einer Adresse, die ein Mensch getippt hat.
            Err(_) => (host_teil.to_string(), standard_port),
        },
        None => (host_teil.to_string(), standard_port),
    };

    if host.is_empty() {
        return Err(LadeFehler::Netz(narsil_i18n::tr("no host in the address").into()));
    }

    // A-ADBLOCK (Koordination 01.10.2026, 21:45): DER SEITEN-HOST.
    //
    // GEMESSENE ENTSCHEIDUNG, warum HIER: Das ist die erste Stelle, an
    // der die Adresse einer Seite im Klartext vorliegt. Der Filter
    // braucht den Seiten-Host fuer `$domain=` und `@@` — und ohne ihn
    // blockiert der Filter entweder nichts oder das Falsche.
    //
    // GEMESSENE KOSTEN, wenn diese Zeile fehlt: `@@||ads.example^
    // $domain=wikipedia.org` haelt die Nachrichtenseiten nicht mehr
    // frei, und der Filter nimmt ihnen die Bilder. Das ist genau die
    // Luecke, die `narsil-adblock` in `tests/regeln.rs` als
    // „Gegenbeweis-Test" fuehrt.
    //
    // GEMESSENE ENTSCHEIDUNG, warum VOR `fetch` und nicht danach: nach
    // dem Abruf waere die erste Seite schon geladen, und ihre
    // Unterressourcen kaemen ohne den Host aus.
    //
    // GEMESSENE ENTSCHEIDUNG, warum nur `host` und nicht die ganze URL:
    // `seiten_host_setzen` zerlegt die Adresse ueber `Anfrage::neu`
    // selbst und schneidet dabei den Port ab. Ich hatte zuerst ein
    // `format!("{}://{}{}", …)` gebaut — das setzt eine URL zusammen,
    // die der Aufrufer gleich wieder zerlegt. `host` ist dieselbe
    // Angabe ohne den Umweg.
    narsil_page::werbung::seiten_host_setzen(&host);

    // added 03.10.2026, TLS (Freigabe 23:0x): **HSTS/HTTPS-AUFRUECKUNG, und
    // die steht VOR dem Bau der Anfrage** — nicht danach. Eine Aufrueckung, die
    // erst nach dem Abruf geschieht, waere **ein zweiter** Abruf.
    //
    // **UND DIE REGEL IST `hsts_aufruecken` aus `sicherheit.rs`**, nicht eine
    // eigene Bedingung hier. GEMESSEN, warum das wichtig ist: in
    // `narsil-net/src/lib.rs:2936` (`hsts_anheben`) steht **nur**
    // `anfrage.port = 443` und **nie** `anfrage.https` — die Hochstufung wirkt
    // dort also **nur indirekt** ueber `ist_https` und auf einem
    // Nicht-Standardport **gar nicht** (BRETT 24:0x Punkt 1). Wer hier die
    // **Adresse** umbaut, macht es richtig: Schema, Port, Pfad, Abfrage und
    // Anker bleiben zusammen.
    if let Some(erhoeht) = narsil_net::sicherheit::hsts_aufruecken(
        adresse,
        crate::sicherheit_haken::hsts_erzwungen(&host, jetzt()),
    ) {
        // added 03.10.2026, TLS: **UND DIE NEUE ADRESSE WIRD AUCH GEMELDET**,
        // damit Adressleiste und Meldung dasselbe sagen. Eine Aufrueckung, die
        // nur im Netz stattfindet, waere unsichtbar.
        crate::sicherheit_haken::vermerke(&std::format!(
            "HSTS: {} -> {} (aufgerueckt)",
            adresse,
            erhoeht
        ));
        //
        // KONTEXT (05.10.2026, 11:15): DAS FLAG **DURCHREICHEN**. GEMESSENE
        // ENTSCHEIDUNG, warum es hier nicht `false` ist: der HSTS-Fall ist
        // genau der Fall, in dem jemand „hart neu laden“ gedrueckt hat, weil
        // die Seite alt war — ein `false` waere an dieser einen Stelle die
        // Antwort „bitte frag trotzdem beim Server nach“.
        return lade_ueber_netz_mit_bytes_und_cache(&erhoeht, ereignis, kein_cache);
    }

    // Release 10.10.2026: das Schema ausdruecklich mitgeben — `narsil-net`
    // erkennt TLS sonst nur an Port 443 (`ist_https`). GEMESSEN vorher:
    // `https://localhost:8443/` ging als KLARTEXT-HTTP an den TLS-Port.
    let mut anfrage = Request {
        method: Method::Get,
        host: host.clone(),
        port,
        path: pfad.to_string(),
        https: Some(https),
        ..Request::get(&host, port, pfad)
    };

    // added 04.10.2026, TLS (Freigabe der Leitung 04.10.): **DER
    // `Cookie`-KOPF, und er kommt VOR die Messung.**
    //
    // **GEMESSEN, warum es vorher sein muss:** `AnfrageAngaben::aus_request`
    // liest `anfrage.headers`, und die DevTools zeigen **genau das**, was
    // gesendet wurde. Wer den Kopf **nach** `beginne` anhaengt, zeigt dem
    // Benutzer einen Abruf, den es so nie gab — und genau da schaut man bei
    // „warum bin ich nicht eingeloggt“.
    //
    // **UND DIE OBERSTE SEITE IST DER HOST SELBST:** ein Dokumentabruf **ist**
    // die oberste Navigation (Top-Level), also **nie** ein Drittanbieterzugriff.
    // Die Drittanbieter-Sperre (`keks.rs::DRITTEANBIER_STANDARD`) darf hier
    // nichts abschneiden — und das ist kein Sonderfall, sondern die Regel:
    // `SameSite=Lax` heisst ausdruecklich „beim Sprung von aussen ja“.
    crate::keks::anfrage_bei_kekse(&mut anfrage, &adresse, &host);

    //
    // KONTEXT (05.10.2026, 11:10, Leitung „Hart-neu-laden = Cache-Control:
    // no-cache Anfrage-Kopf“): DER KOPF STEHT HIER.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **hier** (und nicht weiter unten bei
    // `fetch`): GEMESSEN ist der Kommentar direkt oberhalb — die DevTools zeigen
    // **genau das**, was in `anfrage.headers` steht. Ein Kopf nach der Messung
    // waere ein Abruf, den der Reiter **nicht** zeigt, obwohl er rausgeht:
    // genau die Sorte Fehler, die man bei „warum ist die Seite alt?“ stundenlang
    // sucht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **`Pragma`** daneben steht: HTTP/1.0-Ketten
    // (alte Proxys) kennen `Cache-Control` nicht. Es ist eine Zeile, und sie
    // kostet nichts — die Regel „ein Wort, zwei Woerter“ gibt es nicht.
    if kein_cache {
        anfrage
            .headers
            .insert("Cache-Control".to_string(), "no-cache".to_string());
        anfrage
            .headers
            .insert("Pragma".to_string(), "no-cache".to_string());
    }

    // D10 (01.10.2026): DIE ANFRAGE MESSEN.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Puffer VOR `fetch` gefuellt wird:
    // die Zeit, die im Reiter steht, ist die Zeit von „hier" bis „die
    // Antwort ist da" — und `fetch` ist der einzige Aufruf dazwischen.
    // Wer erst danach misst, haelt die Zeit fuer 0.
    ereignis.beginne(&narsil_devtools::d4_netzwerk::AnfrageAngaben::aus_request(
        &anfrage, "dokument",
    ));
    let geholt = narsil_net::fetch(&anfrage);
    // D10 (04.10.2026, 0:4x): DIE PHASEN der Netz-Crate uebernehmen.
    //
    // Die Koordination hat gemeldet (team/WUNSCH_DEVTOOLS.txt, letzte
    // Zeile): `narsil-net` liefert `Phasen { dns, tcp, tls, erstes_byte,
    // gesamt }` je Abruf, und der Netz-Reiter soll sie ANZEIGEN — ohne
    // einen eigenen zweiten Haken. Genau das ist diese Zeile: gelesen
    // direkt nach `fetch`, aus dem Thread-Zustand der Netz-Crate.
    //
    // GEMESSENE ENTSCHEIDUNG, warum SOFORT und nicht erst beim Abschluss:
    // `phasen()` ist der Wert DIESES Threads (dort bewusst
    // `thread_local!`). Zwischen hier und `abschluss_ereignis` (in
    // `Sitzung::lade_jetzt`) laeuft noch `aus_ist` — und im Abruf einer
    // Seite werden Bilder und Skripte geholt. Wer die Phasen erst dort
    // liest, zeigt die Zeit der LETZTEN Anfrage in der Zeile der
    // ersten. Der Puffer `ereignis` ist der Ort, an dem „zu MEINER
    // Anfrage" festgehalten wird.
    ereignis.setze_phasen(narsil_net::phasen());
    // Der Puffer bekommt IMMER ein Ergebnis — auch beim Fehler. Eine
    // Zeile, die beim Fehler fehlt, waere genau die Zeile, die der
    // Nutzer sucht („warum ist die Seite nicht geladen?").
    match &geholt {
        Ok(r) => ereignis.ende(narsil_devtools::d4_netzwerk::Ergebnis::Antwort(
            narsil_devtools::d4_netzwerk::AntwortPaket::aus_response(r, None),
        )),
        Err(e) => ereignis.ende(narsil_devtools::d4_netzwerk::Ergebnis::Fehler(
            narsil_devtools::d4_netzwerk::NetFehler::aus_text(e),
        )),
    }
    // added 03.10.2026, TLS (Freigabe 23:0x): **DIE ZERTIFIKATSWARNSEITE**, und
    // sie ist die erste Stelle, an der ein Zertifikatsfehler **fuer den
    // Benutzer** sichtbar wird. **BISHER** war er ein `LadeFehler::Netz` mit
    // einem Text — und ein Text ist kein Fenster.
    //
    // **DIE DREI BEDINGUNGEN, und alle drei sind noetig:**
    //  1. **Der Fehler ist ein Kettenfehler.** Jeder andere Netzfehler bleibt ein
    //     Fehler — ein DNS-Fehler ist keine Warnseite.
    //  2. **Der Host ist NICHT freigegeben.** Nach der Freigabe soll der Abruf
    //     **stattfinden**, nicht wieder eine Warnseite erzeugen.
    //  3. **Die Antwort wird GEBAUT statt geholt.** Die Warnseite kommt aus
    //     diesem Programm, nicht vom Server — das ist der ganze Unterschied zu
    //     `narsil-net`, das die Seite **ablehnt**.
    let antwort: Response = match geholt {
        Ok(r) => {
            // added 04.10.2026, TLS: **DAS `Set-Cookie` SPEICHERN**, und es
            // steht **vor** dem Match-Arm, der die Warnseite baut.
            //
            // **UND ES STEHT NICHT IM `Err`-ZWEIG:** ein abgelehnter
            // Zertifikatsfehler hat keine `Set-Cookie`-Koepfe, und ein
            // Netzfehler schon gar nicht.
            //
            // **GEMESSENE LÜCKE, die ich hier NICHT schliesse (BRETT):**
            // `narsil_net::fetch` reicht **keine** Sitzung durch
            // (`narsil-net/src/lib.rs:1643` → `fetch_mit_mit(request, limits, None)`),
            // deshalb gehen die `Set-Cookie`-Koepfe der **Zwischensprünge**
            // verloren — `hole_und_sende_innen` meldet sie zwar je Sprung ueber
            // `s.set_cookie_sehen` (`lib.rs:1834`), aber **an niemanden**, weil
            // `sitzung` `None` ist. **WAS MIR FEHLT, ist eine `Response` mit den
            // Koepfen aller Sprünge**, und die kann nur `narsil-net` liefern.
            crate::keks::antwort_bei_kekse(&r, &adresse);
            r
        }
        // Release 10.10.2026: IMMER die Warnseite — auch fuer einen schon
        // freigegebenen Host: die Ausnahme gilt nur fuer genau ein Zertifikat
        // (`narsil_tls::ausnahme`); scheitert die Kette trotzdem, ist es ein
        // ANDERES, und das muss wieder warnen.
        Err(narsil_net::NetError::Tls(ref grund)) => {
            // added 04.10.2026, TLS: **DIE TEXT-BRUECKE, und sie ist
            // **ersatzweise** — GEMESSEN: `narsil_net::NetError` hat **keine**
            // Variante fuer einen Kettenfehler, die Kette laeuft ueber
            // `NetError::Tls(e.to_string())` (`narsil-net/src/lib.rs:2389`).
            // **DIE VARIANTE IST BESSER** und liegt als
            // `narsil_tls::info::zertifikatswarnung(KetteFehler, host)`
            // bereit; sie braucht nur `NetError::Kette(KetteFehler)` in
            // `narsil-net` (BRETT 25:0x Punkt 6).
            let w = narsil_tls::info::zertifikatswarnung_aus_text(grund, &host);
            crate::sicherheit_haken::vermerke(&std::format!(
                "Warnseite ({:?}): {}",
                w.art,
                w.erklaerung
            ));
            Response {
                status: narsil_net::StatusLine {
                    version: "HTTP/1.1".to_string(),
                    code: 200,
                    reason: "OK".to_string(),
                },
                headers: std::collections::HashMap::from([(
                    "content-type".to_string(),
                    "text/html; charset=utf-8".to_string(),
                )]),
                // Release 10.10.2026: mit Einmal-Schluessel (siehe oben bei
                // `ist_akzeptier_adresse`).
                body: match crate::sicherheit_haken::einmal_schluessel_neu(&host, &adresse) {
                    Some(k) => narsil_tls::info::warnseite_html_mit_schluessel(&w, &k),
                    None => narsil_tls::info::warnseite_html(&w),
                }
                .into_bytes(),
                trailers: std::collections::HashMap::new(),
                set_cookie: Vec::new(),
                // added 04.10.2026, TLS: **DIE DREI FELDER, und sie sind nicht
                // Kosmetik.** GEMESSEN am Compilerfehler „missing fields
                // `body_quelle`, `entpackt`, `keep_alive`“:
                //  * `body_quelle: ContentLength` — die Laenge **ist**
                //    bekannt (wir haben sie gebaut), also wird sie **gemeldet**
                //    und nicht geraten.
                //  * `keep_alive: Schliesst` — die Verbindung ist **weg**; sie
                //    wieder zu benutzen waere eine Leiche.
                body_quelle: narsil_net::BodyQuelle::ContentLength,
                keep_alive: narsil_net::KeepAlive::Schliesst,
                // added 04.10.2026, TLS: **`entpackt: false`, und die Begruendung
                // ist wichtig, weil `Response` es an zwei Stellen liest** (Zeile
                // 1125 und 1137, GEMESSEN). Die Warnseite ist **unkomprimiert**
                // und **unverpackt** — wir haben sie gebaut, nicht empfangen.
                // `true` wuerde den Aufrufern sagen, sie muessen noch entpacken.
                entpackt: false,
                // added 04.10.2026, TLS: **`url` IST DIE EIGENE ADRESSE**, weil
                // die aufrufende Adresse **http** sein kann (HSTS) und der
                // Bericht sonst eine Adresse nennt, die es nicht gab.
                //
                // **UND `adresse`, nicht `a`:** GEMESSEN am Compilerfehler
                // `cannot find value 'a' in this scope` — `a` ist die
                // **gekappte** Adresse in `lade_mit_ereignis`, und diese
                // Funktion heisst `lade_ueber_netz_mit_bytes(adresse, ...)`.
                // Zwei Aehnlichkeiten und eine Funktion, und der Compiler
                // brauchte drei Anlaeufe, bis ich die **richtige** gefunden
                // hatte — nach **zwei** falschen. Das ist die dritte
                // Zusammenfassung dieser Schicht: ich suchte nach dem **Ort**
                // statt nach der **Signatur**.
                url: adresse.to_string(),
            }
        }
        Err(e) => {
            return Err(LadeFehler::Netz(std::format!("{host}:{port}{pfad} — {e}")));
        }
    };
    //
    // GEMESSEN, warum die Basis gesetzt wird: `narsil-net` schickt sonst
    // einen `Host:`-Header ohne Port und loest die Umleitung von
    // http://example.com nach https://example.com falsch auf.

    // Release 09.10.2026: eine Fehlerantwort MIT Inhalt ist eine Seite —
    // Chrome zeigt die 404-/403-Seite des Servers (gestaltet, mit Links),
    // nur ohne Inhalt die eigene Fehlermeldung. GEMESSEN vorher: zeit.de
    // (403 mit Sperrseite) zeigte nur „Netzfehler: HTTP 403 Forbidden".
    if antwort.status.code >= 400 && antwort.body.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(LadeFehler::Netz(std::format!(
            "HTTP {} {}",
            antwort.status.code,
            antwort.status.reason
        )));
    }

    let text = text_aus_antwort(&antwort);

    //
    // added 03.10.2026, Rolle TLS (Freigabe der Leitung): DIE ANTWORT DES
    // DOKUMENTS — **eine** Zeile, und sie steht **hier**, weil das die erste
    // Stelle ist, an der die **Antwort** existiert. Sie liefert genau eine
    // Angabe weiter, die keine Unterressource selbst sehen kann: den
    // `Content-Security-Policy`-Wert.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **nach** der `status.code`-Pruefung: eine
    // 404-Seite laedt keine Unterressourcen, und eine Richtlinie fuer ein
    // Dokument zu setzen, das es nicht gibt, waere eine Behauptung.
    crate::sicherheit_haken::dokument_antwort(&antwort);

    Ok((
        Ladung {
            text,
            art: Quellart::Netz,
            // DER CONTENT-TYPE OHNE PARAMETER. GEMESSENE ENTSCHEIDUNG, warum
            // hier der Header gelesen wird und nicht in `text_aus_antwort`:
            // dort wird er fuer den Zeichensatz benutzt und dann vergessen —
            // und der Betrachter braucht ihn fuer die Erkennung (siehe das
            // Feld `content_type`).
            content_type: content_type_aus_antwort(&antwort),
            // GEMESSEN (03.10. 08:12): das ist die Endadresse nach der
            // Umleitungskette — siehe das Feld.
            end_adresse: antwort.url.clone(),
            status: antwort.status.code,
        },
        // DIE ORIGINALBYTES. GEMESSENE ENTSCHEIDUNG, warum `antwort.body`
        // und nicht eine Kopie aus dem Text: der Text ist schon umgewandelt,
        // und der Betrachter braucht das Original (siehe `lade_bytes`).
        antwort.body,
    ))
}

/// W7B (30.09.2026, 23:05): DIE ANTWORT IN TEXT — IN JEDEM ZEICHENSATZ.
///
/// # DER BEFUND DER KOORDINATION
///
/// `https://www.google.com` scheiterte mit **„Antwort ist kein Text:
/// invalid utf-8 sequence"**. Der Grund ist ein Byte: `0xE4` ist in
/// UTF-8 kein Zeichen, sondern der Anfang einer kaputten Folge. Google
/// liefert je nach Region ISO-8859-1, und der Browser hat die ganze
/// Seite verworfen — **weil eines von 2000 Bytes kein UTF-8 war**.
///
/// # DIE REGEL
///
/// **Nie wegen des Zeichensatzes eine Seite ablehnen.** Jede Byte-Folge
/// ergibt Text. Es gibt hier keinen `Err`-Zweig, und das ist Absicht:
/// die Alternative ist eine Fehlermeldung, die dem Nutzer sagt „diese
/// Seite ist kaputt", wenn sie nur in einer anderen Sprache geschrieben
/// ist.
///
/// # DIE REIHENFOLGE (Koordination, 23:00) — und warum so
///
/// | # | Quelle | Beispiel | Warum |
/// |---|---|---|---|
/// | 0 | **BOM** | `EF BB BF` | maschinell, steht VOR allem |
/// | 1 | `Content-Type: …; charset=` | `ISO-8859-1` | der Server hat die Bytes selbst gelesen |
/// | 2 | `<meta charset=…>` | `windows-1252` | die Seite selbst |
/// | 3 | raten | UTF-8, sonst windows-1252 | `Satz::Unbekannt` |
///
/// GEMESSEN, warum der Transportweg VOR dem Meta steht: der Header ist
/// die Angabe des Servers ueber die Bytes, die er selbst geschrieben
/// hat; das `<meta>` ist eine Angabe im Dokument, die auch falsch sein
/// kann (und bei Google war es die, die nicht stimmte — der Header
/// sagte ISO-8859-1). Wer die Reihenfolge umdreht, zeigt
/// „Grüße" als „Gr??e" auf genau den Seiten, die sich selbst
/// falsch beschreiben.
///
/// Die Erkennung macht `narsil_page::hteile::Satz` (W4/H6): dort steht
/// die cp1252-Tabelle mit den 128 druckbaren Zeichen zwischen `0x80`
/// und `0x9F` (Euro, typografische Anfuehrungszeichen, Pfeile). Eine
/// zweite Tabelle hier waere die zweite Wahrheit ueber dasselbe Byte.
fn text_aus_antwort(antwort: &narsil_net::Response) -> String {
    use narsil_page::hteile::{Satz, zu_text};

    // Release 11.10.2026: nennen BOM, Content-Type oder `<meta>` eine
    // Kodierung, gilt sie — mit den Dekodierern des WHATWG Encoding Standard
    // (`narsil_fmt_html::kodierung`: shift_jis, euc-kr, gbk/gb18030, big5,
    // euc-jp, iso-2022-jp, alle Einbyte-Kodierungen). GEMESSEN vorher: nur
    // UTF-8, windows-1252 und Latin-1 wurden erkannt; Seiten in shift_jis,
    // euc-kr oder gbk wurden als UTF-8 gelesen (Zeichensalat). Ohne Angabe
    // bleibt es beim Weg unten.
    let typ = antwort.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("content-type")).map(|(_, w)| w.as_str());
    if let Some(k) = narsil_fmt_html::kodierung::erkenne_angabe(&antwort.body, typ) {
        return text_in_kodierung(&antwort.body, k);
    }

    // Das BOM gehoert NICHT zum Text: `String::from_utf8_lossy` laesst
    // es als U+FEFF stehen, und das ist ein Zeichen, das man nicht sieht
    // und das die erste Spalte verschiebt.
    let bytes: &[u8] = match antwort.body.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(ohne) => ohne,
        None => &antwort.body,
    };

    // 1. Der Transportweg. Die Kopfzeilen werden GROSS geschrieben
    //    gespeichert, deshalb wird der Name klein gesucht — ein
    //    `Content-Type` und ein `content-type` sind derselbe Header.
    let mut satz = Satz::Unbekannt;
    for (name, wert) in &antwort.headers {
        if name.eq_ignore_ascii_case("content-type") {
            satz = satz_aus_charset(wert);
            if satz != Satz::Unbekannt {
                break;
            }
        }
    }
    // 2./3. BOM ist schon abgeschnitten; der Rest ist die Seite selbst.
    if satz == Satz::Unbekannt {
        satz = Satz::erkennen(bytes, None).0;
    }
    // 4. `zu_text` macht aus `Unbekannt`: UTF-8-Versuch, sonst
    //    windows-1252. Und aus JEDER Eingabe Text — kein Zweig, der
    //    ablehnt.
    zu_text(bytes, satz)
}

/// DER MEDIENTYP aus dem `Content-Type`, ohne den Parameterteil.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist und kein
/// Abschneiden an der Aufrufstelle: der Abschneidepunkt ist der `;`, und der
/// steht an zwei Stellen (hier und im Browser, wenn er es braucht). Wer ihn
/// zweimal hinschreibt, hat ihn einmal falsch.
///
/// GEMESSENE ENTSCHEIDUNG, warum KLEIN geschrieben wird: `Application/PDF`
/// ist gueltig, und der Vergleich auf „application/pdf" waere sonst falsch.
/// Release 11.10.2026: Bytes in einer erkannten Kodierung als Text; die BOM
/// gehoert nicht zum Text (der UTF-8-Dekodierer liest sie sonst mit).
fn text_in_kodierung(bytes: &[u8], k: narsil_fmt_html::kodierung::Kodierung) -> String {
    let bytes = if k.ist_utf8() { bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes) } else { bytes };
    narsil_fmt_html::kodierung::dekodiere(bytes, k)
}

fn content_type_aus_antwort(antwort: &narsil_net::Response) -> String {
    for (name, wert) in &antwort.headers {
        if name.eq_ignore_ascii_case("content-type") {
            return wert
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
        }
    }
    String::new()
}

/// Ob eine Antwort ein **Download** ist — und wenn ja, **warum**.
///
/// # DIE DREI GRUENDE, und warum sie in **dieser** Reihenfolge
///
/// 1. `Content-Disposition: attachment` — GEMESSENE BEGRUENDUNG, warum das der
///    **erste** Grund ist: der Server sagt **ausdruecklich**, dass die Antwort
///    gespeichert statt gezeigt werden soll. Alles andere ist eine **Deutung**.
/// 2. Ein **nicht darstellbarer** Typ — GEMESSENE BEGRUENDUNG, warum das ueber
///    den Typ und **nicht** ueber `Content-Disposition` laeuft: viele Server
///    schicken **gar keine** `Content-Disposition`, und ein Browser **muss**
///    trotzdem speichern statt ein 40-MB-PDF anzuzeigen.
/// 3. `<a download>` — kommt **nicht** hierher, sondern aus dem **Dokument**
///    (es ist eine Eigenschaft des **Ankers**). GEMESSENE ENTSCHEIDUNG, warum
///    das **getrennt** bleibt: die URL ist hier **nur** die Antwort, und das
///    `download`-Attribut kennt nur der **Klick**, der die Antwort ausgeloest hat.
///
/// # WARUM DIE **DARSTELLBAREN** eine Liste und **nicht** ein Negativtest
///
/// GEMESSENE BEGRUENDUNG: „alles ausser `text/*` und `image/*`“ waere eine
/// kuerzere Regel und **falsch** — `application/pdf`, `video/mp4` und
/// `audio/mpeg` **kann** der Browser anzeigen, und ein Download davon waere ein
/// **Fehler**, den der Nutzer als „der Browser ist kaputt" liest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadGrund {
    /// Der Server hat es **gesagt**.
    Header,
    /// Der Typ **kann** nicht angezeigt werden.
    Typ,
}

/// Und die Typen, die der Browser **anzeigen** kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier `application/pdf` **drin** ist: der
/// Browser **hat** einen PDF-Betrachter (`narsil-chrome/src/pdf_viewer.rs`), und
/// GEMESSEN ist, dass er `.pdf`-Adressen **darin** oeffnet. Also ist ein PDF
/// **darstellbar** — und ein Download waere **falsch**.
const DARSTELLBAR: [&str; 12] = [
    "text/html",
    "text/plain",
    "text/css",
    "text/markdown",
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/svg+xml",
    "image/x-icon",
    "application/pdf",
];

/// Und die **Entscheidung** — und GEMESSENE ENTSCHEIDUNG, warum sie **eine**
/// Funktion ist und **nicht** an drei Stellen im Code steht: „soll gespeichert
/// werden?“ ist eine Frage, die der **Ladeweg** (einmal), der **Klick** (einmal)
/// und ein **Test** (oft) stellen, und drei Antworten darauf sind **drei**
/// Regeln.
///
/// GEMESSENE ENTSCHEIDUNG, warum `text/html` **nie** ein Download ist, auch
/// wenn der Typ es nicht sagt: eine HTML-Datei **kann** man anzeigen, und sie
/// **immer** zu speichern ist die haeufigste **Fehlbeschreibung** von „nicht
/// darstellbar“.
pub fn ist_download(headers: &[(String, String)], content_type: &str) -> Option<DownloadGrund> {
    // (1) Der Header. GEMESSEN, warum **vor** allem anderen: er ist eine
    // **Auskunft** des Servers, und eine Auskunft schlaegt jede Deutung.
    for (name, wert) in headers {
        if name.eq_ignore_ascii_case("content-disposition")
            && wert
                .to_ascii_lowercase()
                .split(';')
                .any(|t| t.trim().eq_ignore_ascii_case("attachment"))
        {
            return Some(DownloadGrund::Header);
        }
    }
    // (2) Der Typ. GEMESSEN, warum ein **leerer** Typ **kein** Download ist: ein
    // Server, der **gar keinen** Typ schickt, ist kaputt — und „alles ist ein
    // Download" waere die **falsche** Antwort, denn `text/html` ist der haeufigste
    // Fall ohne Typ-Header bei lokalen Dateien.
    if content_type.is_empty() {
        return None;
    }
    if DARSTELLBAR.contains(&content_type) {
        return None;
    }
    Some(DownloadGrund::Typ)
}

/// Der Zeichensatz aus einem `Content-Type`-Wert.
///
/// `text/html; charset=ISO-8859-1` -> `Satz::Latin1`.
/// `text/html; charset="utf-8"` -> `Satz::Utf8`.
/// Alles andere -> `Satz::Unbekannt` (und damit: raten).
fn satz_aus_charset(content_type: &str) -> narsil_page::hteile::Satz {
    use narsil_page::hteile::Satz;
    let klein = content_type.to_ascii_lowercase();
    let Some(i) = klein.find("charset=") else {
        return Satz::Unbekannt;
    };
    let name: String = klein[i + 8..]
        .trim_start()
        .trim_start_matches('"')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    Satz::benannt(&name)
}

/// W7 P2: EIN FORMULAR ALS `POST` SENDEN (`x-www-form-urlencoded`).
///
/// # WARUM EINE EIGENE FUNKTION
///
/// `lade()` kann nur GET: es hat keine Koerperangabe, und ein POST als
/// GET waere ein Formular, das im Suchfeld landet statt in der
/// Datenbank. Der Body ist eine eigene Sache mit eigener Grenze.
///
/// # DIE KODIERUNG
///
/// `narsil_url::formular_kodieren` macht die Bytes — die EINE Stelle, die
/// weiss, wie ein Formular kodiert wird (Rolle URL). Eine eigene
/// Kodierung hier waere die zweite Wahrheit ueber dasselbe Byte-Format.
///
/// # DIE GRENZEN (M4)
///
/// `dom::MAX_POST_BODY` (64 KiB) fuer den Body, `dom::MAX_EINGABEN` (256)
/// Felder, `MAX_TIPP`/`MAX_EINGABE_WERT` (128/4096 Zeichen) je Feld.
/// Alle werden VOR dem Senden geprueft; ein zu grosses Formular wird
/// mit einer benannten Meldung abgelehnt und nicht gesendet.
///
/// Der Pfad wird wie bei `lade_ueber_netz` gebaut (Host, Port, Schema).
/// Release 10.10.2026: ein Name im `Content-Disposition`-Kopf (HTML
/// §4.10.21.8: `"` als `%22`, Zeilenenden als `%0D`/`%0A`).
fn multipart_name(n: &str) -> String {
    n.replace('"', "%22").replace('\r', "%0D").replace('\n', "%0A")
}

/// Release 10.10.2026: der Koerper fuer `multipart/form-data` (RFC 7578) —
/// die Felder in Dokumentreihenfolge, die Dateien mit Namen, Typ und Inhalt.
/// Gibt Koerper und Grenze zurueck.
pub fn multipart_bauen(
    felder: &[narsil_url::FormularFeld],
    dateien: &[crate::dom::DateiTeil],
) -> Result<(Vec<u8>, String), String> {
    let zufall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
        ^ (felder.len() as u128).rotate_left(17);
    let grenze = format!("----NarsilFormBoundary{:016x}", zufall as u64);
    let mut koerper: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < felder.len() {
        if let Some(d) = dateien.iter().find(|d| d.start == i) {
            let name = multipart_name(&d.name);
            if d.dateien.is_empty() {
                koerper.extend_from_slice(format!(
                    "--{grenze}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n"
                ).as_bytes());
            }
            for datei in &d.dateien {
                let inhalt = std::fs::read(&datei.pfad).map_err(|e| format!("{}: {e}", datei.name))?;
                if koerper.len() + inhalt.len() > crate::dom::MAX_MULTIPART_BODY {
                    return Err(narsil_i18n::trf!("Form larger than {} bytes", crate::dom::MAX_MULTIPART_BODY));
                }
                let typ = if datei.typ.is_empty() { "application/octet-stream" } else { datei.typ.as_str() };
                koerper.extend_from_slice(format!(
                    "--{grenze}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: {typ}\r\n\r\n",
                    multipart_name(&datei.name)
                ).as_bytes());
                koerper.extend_from_slice(&inhalt);
                koerper.extend_from_slice(b"\r\n");
            }
            i += d.anzahl.max(1);
            continue;
        }
        let f = &felder[i];
        koerper.extend_from_slice(format!(
            "--{grenze}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n",
            multipart_name(&f.name)
        ).as_bytes());
        // Zeilenenden als CRLF (HTML §4.10.21.8).
        koerper.extend_from_slice(f.wert.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n").as_bytes());
        koerper.extend_from_slice(b"\r\n");
        i += 1;
    }
    koerper.extend_from_slice(format!("--{grenze}--\r\n").as_bytes());
    Ok((koerper, grenze))
}

pub fn sende_formular_post(
    basis: &str,
    form: &crate::dom::Formular,
    felder: &[narsil_url::FormularFeld],
    dateien: &[crate::dom::DateiTeil],
) -> Result<Ladung, LadeFehler> {
    use narsil_net::{Method, Request};
    if felder.len() > crate::dom::MAX_EINGABEN {
        return Err(LadeFehler::Netz(narsil_i18n::trf!(
            "Form: {} fields (allowed {})",
            felder.len(),
            crate::dom::MAX_EINGABEN
        )));
    }
    // M4: der Body wird geprueft, BEVOR er gebaut wird.
    let kodiert = narsil_url::formular_kodieren(felder)
        .map_err(|e| LadeFehler::Netz(std::format!("Formular: {e}")))?;
    if kodiert.len() > crate::dom::MAX_POST_BODY {
        return Err(LadeFehler::Netz(narsil_i18n::trf!(
            "Form: {} bytes (allowed {})",
            kodiert.len(),
            crate::dom::MAX_POST_BODY
        )));
    }
    // Die Adresse: `action` oder die Seite selbst.
    //
    // 02.10. 04:20: DAS `action` WIRD AUFGELOEST (vorher: roh uebernommen).
    //
    // GEMESSENER ANLASS (`www.duckduckgo.com/html/`, 04:05): die Seite traegt
    // `<form name="x" id="search_form_homepage" class="search"
    // action="/html/" method="post">`. GEMESSEN: tippen geht, Enter **nicht** —
    // die Adresse bleibt, keine Navigation. GEMESSENE URSACHE: `form.aktion`
    // ist das ROHE Attribut, und die Pruefung unten verlangt `http(s)` am
    // Anfang — `/html/` faellt durch, der POST bricht ab, **bevor** eine
    // Anfrage rausgeht.
    //
    // GEMESSENE EINORDNUNG, warum das kein Randfall ist: `action="/pfad"`
    // und `action="pfad"` sind die Regel (HTML 4, 17.2: das Attribut ist eine
    // **URI-Referenz**, und die wird gegen die Basis der Seite aufgeloest).
    // GEMESSENER Beleg, dass der GET-Weg es richtig macht: `get_url`
    // (narsil-page/hteile.rs:590) loest mit `loese_link` auf, und darum
    // funktioniert die wikipedia-Suche — GEMESSEN 04:05: Enter ergibt
    // `https://de.wikipedia.org/w/index.php?search=Rust&title=Spezial%3A…`.
    // Zwei Wege fuer dieselbe Angabe, und einer von beiden war falsch.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `loese_link` und kein Handbauen: es ist
    // die Funktion, die jede andere Adresse in dieser Crate aufloest
    // (Schrift, Bild, Skript, Link). Eine eigene Aufloesung waere die
    // Fehlerklasse „drei Zahlenraeume" fuer Adressen.
    let ziel = if form.aktion.is_empty() {
        basis.to_string()
    } else {
        narsil_page::loese_link(&form.aktion, basis)
    };
    if !ziel.starts_with("http://") && !ziel.starts_with("https://") {
        return Err(LadeFehler::Netz(narsil_i18n::trf!(
            "POST only works over http(s): {}",
            ziel
        )));
    }
    let https = ziel.starts_with("https://");
    let standard_port: u16 = if https { 443 } else { 80 };
    let rest = match ziel.find("://") {
        Some(i) => &ziel[i + 3..],
        None => &ziel,
    };
    let (host_teil, pfad) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_teil.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(n) if !h.is_empty() => (h.to_string(), n),
            _ => (host_teil.to_string(), standard_port),
        },
        None => (host_teil.to_string(), standard_port),
    };
    if host.is_empty() {
        return Err(LadeFehler::Netz(narsil_i18n::tr("POST: no host in the address").into()));
    }
    //
    // A-ADBLOCK (Koordination 01.10.2026, 21:45, „vor jedem
    // Unterressourcen-Abruf"): DER FILTER, auch vor einem POST.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein POST ueberhaupt geprueft wird:
    // ein Formular geht an eine andere Adresse als die Seite, und
    // genau dort liegen Tracking-Endpunkte. Ausserdem ist `Ziel` hier
    // schon die vollstaendige URL, es kostet also nichts.
    //
    // GEMESSENE EIGENHEIT: Die Regel wird fuer `Art::Sonstige`
    // geprueft. `$xhr` und `$submit` fallen beide in diese Art — sie zu
    // trennen waere eine eigene Enum-Variante, und die Nutzlast im
    // eingebauten Filter ist null. Die Unterscheidung steht in WUNSCH.
    // 05.10. 23:22: `&ziel` (GEMESSENER Typfehler, `bau.sh narsil check`):
    // `werbung::erlaubt(url: &str, ...)` (werbung.rs:445) verlangt eine
    // LEIHZEIT, und `ziel` ist ein `String`. GEMESSENE EINORDNUNG, warum
    // ich das gefixt habe und nicht gemeldet und gewartet: es sind zwei
    // Zeichen, sie stehen in einer Datei, in der die Rolle CSS gerade
    // arbeitet, und ohne sie baut WEDER `narsil` NOCH `narsil-probe` — also
    // auch keine Abnahme und kein 30-Seiten-Lauf. GEMELDET in
    // `team/PROBLEME.txt` 23:12 und 23:22. GEMESSENE EINORDNUNG, warum
    // `&ziel` die richtige Form ist und nicht etwa der Seiten-Host: das
    // dritte Argument IST der Seiten-Host, und es steht schon eine Zeile
    // darunter.
    if !narsil_page::werbung::erlaubt(
        &ziel,
        narsil_page::werbung::Art::Sonstige,
        &narsil_page::werbung::seiten_host(),
    ) {
        return Err(LadeFehler::Netz(narsil_i18n::tr("blocked by the ad filter").into()));
    }
    // added 01.10.2026, R0: `.art(Daten)`. Ein Formular schickt Daten
    // (`application/x-www-form-urlencoded`), keine Seite; `Accept:
    // text/html` waere eine falsche Zusage. DER REGELFALL IST `Dokument`,
    // deshalb steht es hier ausdruecklich.
    let mut anfrage = Request::get(&host, port, pfad).art(narsil_net::RessourceArt::Daten);
    anfrage.method = Method::Post;
    // Release 10.10.2026: das Schema ausdruecklich mitgeben — `narsil-net`
    // erkennt TLS sonst nur an Port 443 (`ist_https`). GEMESSEN vorher:
    // `https://localhost:8443/` ging als KLARTEXT-HTTP an den TLS-Port.
    anfrage.https = Some(https);
    // Release 10.10.2026: `enctype="multipart/form-data"` schickt die Dateien
    // mit (vorher ging jedes Formular urlencoded, ein Hochladen kam nie an).
    if form.enctype == "multipart/form-data" {
        let (koerper, grenze) =
            multipart_bauen(felder, dateien).map_err(|e| LadeFehler::Netz(std::format!("Formular: {e}")))?;
        anfrage.body = Some(koerper);
        anfrage.headers.insert(
            "Content-Type".to_string(),
            std::format!("multipart/form-data; boundary={grenze}"),
        );
    } else {
        anfrage.body = Some(kodiert.into_bytes());
        anfrage.headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
    }
    let antwort = narsil_net::fetch(&anfrage)
        .map_err(|e| LadeFehler::Netz(std::format!("POST {host}:{port}{pfad} — {e}")))?;
    if antwort.status.code >= 400 && antwort.body.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(LadeFehler::Netz(std::format!(
            "HTTP {} {}",
            antwort.status.code,
            antwort.status.reason
        )));
    }
    // W7B: derselbe Weg wie beim GET — ein POST ist eine Seite, und eine
    // Seite wird nicht abgelehnt, weil ihr Zeichensatz ungewoehnlich ist.
    let text = text_aus_antwort(&antwort);
    Ok(Ladung {
        text,
        art: Quellart::Netz,
        content_type: content_type_aus_antwort(&antwort),
        // GEMESSEN (03.10. 08:12): siehe das Feld `end_adresse`.
        end_adresse: antwort.url.clone(),
        status: antwort.status.code,
    })
}

/// Die Meldung, die der Browser fuer einen Ladefehler zeigt.
///
/// Getrennt von [`LadeFehler`], weil die Anzeige eine Entscheidung
/// ist (welcher Text steht im Bild) und der Fehler eine Tatsache. Aus
/// der einen die andere zu bauen hiesse, der Fehler wuesste, wie er
/// aussehen soll.
pub fn meldung_fuer(fehler: &LadeFehler) -> Fehler {
    Fehler::Seite(std::format!("{fehler}"))
}

#[cfg(test)]
mod tests {
    //! W7B: DER ZEICHENSATZ (23:05).
    //!
    //! Die Faelle hier sind die, an denen der Browser vorher die Seite
    //! verworfen hat — jede ein Byte, das in UTF-8 kein Zeichen ist.

    use super::*;

    /// Eine Antwort mit Bytes und einem `Content-Type`.
    fn antwort(bytes: &[u8], content_type: &str) -> narsil_net::Response {
        let mut r = narsil_net::Response::leer(200);
        r.body = bytes.to_vec();
        r.headers
            .insert("Content-Type".to_string(), content_type.to_string());
        r
    }

    /// DER BEFUND: `google.com` liefert ISO-8859-1, und `0xE4` ist `ae`.
    #[test]
    fn iso_8859_1_aus_dem_header_wird_lesbarer_text() {
        // "Baer" als ISO-8859-1: B, a, 0xE4, r
        let bytes = b"<html><body><p>Ba\xE4r</p></body></html>";
        let text = text_aus_antwort(&antwort(bytes, "text/html; charset=ISO-8859-1"));
        assert!(
            text.contains("Ba\u{e4}r"),
            "0xE4 wurde nicht zu 'ae': {text}"
        );
    }

    /// Umlaute ALLE drei, weil in der Meldung "ae/oe/ue" stand und ein
    /// einzelnes Zeichen kein Beweis ist (0xE4 koennte auch etwas
    /// anderes sein).
    #[test]
    fn alle_drei_umlaute_ueber_den_header() {
        let bytes = b"<p>\xE4\xF6\xFC</p>";
        let text = text_aus_antwort(&antwort(bytes, "text/html;charset=iso-8859-1"));
        assert!(text.contains("\u{e4}\u{f6}\u{fc}"), "nicht aeo-ue: {text}");
    }

    /// Release 11.10.2026: ostasiatische Seiten in ihrer Kodierung — ueber
    /// `<meta>` (shift_jis) und ueber den Header (euc-kr, big5, gbk).
    /// GEMESSEN vorher: alle vier als UTF-8 gelesen, nur U+FFFD.
    #[test]
    fn ostasiatische_kodierungen_werden_gelesen() {
        let mut b = b"<meta charset=\"shift_jis\"><p>".to_vec();
        b.extend_from_slice(&[0x93, 0xFA, 0x96, 0x7B]);
        assert!(text_aus_antwort(&antwort(&b, "text/html")).ends_with("\u{65E5}\u{672C}"));
        let t = |bytes: &[u8], typ: &str| text_aus_antwort(&antwort(bytes, typ));
        assert_eq!(t(&[0xC7, 0xD1, 0xB1, 0xB9], "text/html; charset=EUC-KR"), "\u{D55C}\u{AD6D}");
        assert_eq!(t(&[0xA4, 0xA4, 0xA4, 0xE5], "text/html; charset=big5"), "\u{4E2D}\u{6587}");
        assert_eq!(t(&[0xD6, 0xD0, 0xCE, 0xC4], "text/html; charset=gb2312"), "\u{4E2D}\u{6587}");
    }

    /// Ohne Header-Angabe: das `<meta>` sagt es. (Der Weg, den die
    /// meisten Seiten von 2005 gehen.)
    #[test]
    fn meta_charset_wird_gelesen() {
        let bytes = b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\"></head><body><p>Stra\xDFe</p></body></html>";
        let text = text_aus_antwort(&antwort(bytes, "text/html"));
        assert!(
            text.contains("Stra\u{df}e"),
            "0xDF wurde nicht zu ss: {text}"
        );
    }

    /// Ohne jede Angabe: der Versuch ist UTF-8, sonst windows-1252.
    /// 0x80 ist im Fallback das EUROZEICHEN — der Unterschied zu
    /// ISO-8859-1, in dem es ein Steuerzeichen waere.
    #[test]
    fn ohne_angabe_wird_geraten_und_nicht_verworfen() {
        let bytes = b"<p>100\x80 und 0xE4=\xE4</p>";
        let text = text_aus_antwort(&antwort(bytes, "text/html"));
        assert!(
            text.contains('\u{20AC}') && text.contains('\u{e4}'),
            "weder Euro noch ae: {text}"
        );
    }

    /// Das BOM steht NICHT im Text. Ohne das steht in Zeile 1 ein
    /// unsichtbares Zeichen, das die erste Spalte verschiebt.
    #[test]
    fn bom_steht_nicht_im_text() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<p>Wort</p>");
        let text = text_aus_antwort(&antwort(&bytes, "text/html; charset=utf-8"));
        assert!(!text.starts_with('\u{FEFF}'), "BOM im Text: {text:?}");
        assert!(text.contains("Wort"), "Text fehlt: {text}");
    }

    /// DER GANZE PUNKT: **kein Byte der Welt lehnt ab.** 0x00..0xFF
    /// ohne jede Angabe ist gueltiger windows-1252-Text, und 0x00 ist
    /// ein control, aber kein Grund fuer eine Fehlermeldung.
    #[test]
    fn kein_byte_lehnt_ab() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let text = text_aus_antwort(&antwort(&bytes, "application/octet-stream"));
        // 256 Byte, und jeder wurde zu genau einem Zeichen.
        assert_eq!(
            text.chars().count(),
            256,
            "es gingen Bytes verloren oder kamen welche dazu"
        );
    }

    /// W7B ABNAHME (Regel 2d): DER MUESER FUER DEN ZEICHENSATZ.
    ///
    /// GEMESSEN, warum das noetig ist: `narsil-fuzz` ist eine fremde
    /// Crate, und ihre Ziele fuetteren **Text** in die Parser
    /// (`from_utf8_lossy`). Der Zeichensatz ist aber genau der Weg, der
    /// mit BYTES anfängt — kein Fuzz-Ziel der Welt kommt bei ihm vor:
    /// `ziel_html` haette `0xE4` als `U+00C3 U+00A4` gesehen, und die
    /// Seite waere UTF-8 gewesen. Ohne diesen Test gaebe es fuer den
    /// Fix, der google.com zurueckgebracht hat, keinen Beleg.
    ///
    /// Behauptet wird: 300 Zufalls-Antworten mit zufaelligem
    /// Zeichensatz im Header ergeben IMMER Text, das BOM steht nie im
    /// Text, und die Ausgabe waechst nicht ueber das Dreifache der
    /// Eingabe hinaus (M4).
    #[test]
    fn mueller_ueber_alle_zeichen_saetze() {
        const FAELLE: usize = 300;
        const MAX_BYTES: usize = 8 * 1024;
        struct Zufall(u64);
        impl Zufall {
            fn n(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                self.0 = x;
                x.wrapping_mul(0x2545_F491_4F6C_DD1D)
            }
        }
        // Die Saetze, die ein Server angeben kann — plus Unsinn, der
        // zurueckfallen muss.
        let saetze = [
            "text/html; charset=utf-8",
            "text/html; charset=ISO-8859-1",
            "text/html;charset=windows-1252",
            "text/html; charset=\"utf-8\"",
            "text/html; CHARSET=Latin1",
            "text/html",
            "text/html; charset=klingon-9000",
            "",
            "application/octet-stream",
        ];
        let mut z = Zufall(0x5eed_5678);
        for f in 0..FAELLE {
            let laenge = (z.n() as usize) % (MAX_BYTES + 1);
            let mut bytes: Vec<u8> = (0..laenge).map(|_| (z.n() & 0xff) as u8).collect();
            // Bei jedem vierten Fall ein BOM und/oder ein `<meta>`: die
            // beiden Angaben, die im Byte-ANFANG stehen.
            if f % 4 == 1 {
                let mut v = vec![0xEF, 0xBB, 0xBF];
                v.extend_from_slice(&bytes);
                bytes = v;
            }
            if f % 4 == 2 {
                let mut v = b"<meta charset=iso-8859-1>".to_vec();
                v.extend_from_slice(&bytes);
                bytes = v;
            }
            let satz = saetze[(z.n() as usize) % saetze.len()];
            let gross = bytes.len();
            let text = text_aus_antwort(&antwort(&bytes, satz));
            assert!(
                !text.is_empty() || gross == 0,
                "Fall {f}: {gross} Byte mit `{satz}` ergaben KEINEN Text"
            );
            // M4: hoechstens das Dreifache — 3 Byte je kaputtem Byte.
            assert!(
                text.len() <= gross * 3 + 16,
                "Fall {f}: {gross} Byte wurden zu {} Byte Text",
                text.len()
            );
            assert!(!text.starts_with('\u{FEFF}'), "Fall {f}: BOM im Text");
        }
    }
}
