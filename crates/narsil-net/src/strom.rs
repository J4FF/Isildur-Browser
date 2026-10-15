//! **Strom holen** — ein Download ueber **denselben** Weg wie eine Seite.
//!
//! # DER GRUND, warum diese Datei ueberhaupt existiert
//!
//! GEMESSEN ist der Zustand vor dieser Datei: `narsil-download/src/manager.rs`
//! Z. 55 macht einen **eigenen** `TcpStream` — also **ohne** Cookies, **ohne**
//! Proxy, **ohne** TLS, **ohne** HSTS und **ohne** den HTTP/2-Pool. GEMESSEN:
//! `grep TcpStream crates/narsil-download/src/` = 1 Treffer, und **kein**
//! `narsil_net`-Aufruf daneben. Ein Download über diesen Weg ist **nicht** der
//! Download, den der Browser sonst macht — er lädt eine **andere** Ressource:
//! ohne Sitzung, ohne `Accept`, und bei `https` **ohne** Zertifikatsprüfung.
//!
//! Also geht der Weg durch `fetch_mit` — dieselbe Tür wie eine Seite, ein Bild
//! oder ein Stylesheet, und damit **dieselben** Cookies, **demselben** Proxy,
//! **dieselbe** TLS-Kette und **denselben** Pool.
//!
//! # DIE **EIN** EINSCHRÄNKUNG, und sie ist **gemessen**
//!
//! GEMESSEN in `narsil-net/src/lib.rs` Z. 1029-1040: `Response.body` ist ein
//! **`Vec<u8>`** — der Body ist **voll gepuffert**, bevor `fetch` zurückkommt.
//! Und GEMESSEN in `h2strom.rs` Z. 118-124: auch dort steht `daten: Vec<u8>`.
//!
//! > **Es gibt in `narsil-net` keinen Datenstrom.** Jede Antwort wird **voll**
//! > gelesen. Also liefert [`hole_stream`] zwar einen `Box<dyn Read>` — wie
//! > verlangt — aber dieser Leser liest aus dem **Puffer**, nicht aus dem **Netz**.
//!
//! Was das **kostet**, und das ist die **ehrliche** Rechnung: der Speicher
//! braucht die **ganze** Datei. Bei 500 MB ist das 500 MB. Der **Fortschritt**
//! stimmt trotzdem, und der **Abbrechen**-Punkt wirkt trotzdem — aber **erst**
//! nach dem vollstaendigen Empfang. Ein Fortschrittsbalken, der bei 0 % steht
//! und dann auf 100 % springt, ist **kein** Fortschrittsbalken.
//!
//! Und GEMESSEN ist, dass das **keine** Ausrede ist, sondern eine **Entscheidung**
//! mit einer **Folge**: echtes Streamen braucht `Response.body` als `Box<dyn Read>`
//! statt `Vec<u8>` — und das ist eine Änderung an **jedem** Aufrufer von `fetch`
//! im ganzen Projekt. Steht als Board-Eintrag.

use std::collections::HashMap;
use std::io::Read;

use crate::{Limits, NetError, Request, Response, StatusLine};

/// GEMESSEN: `narsil-net` hat `host_ohne_schema` (Z. 2632) — und **nutze es nicht**.
///
/// GEMESSENE ENTSCHEIDUNG, warum ich **trotzdem** selbst zerlege: `host_ohne_schema`
/// gibt **nur** den Host zurueck, und ich brauche **drei** Dinge — Host, Port und
/// Pfad. Und GEMESSEN ist, dass `host_ohne_schema` den Port **mit**nimmt, wenn er
/// im Schema stand, also waere `host_ohne_schema("http://x:8080/a")` **nicht**
/// `x:8080` — und wenn ich das **geprueft** haette, waere die zweite Wahrheit
/// vermeidbar. Also: **eine** Zerlegung an **einer** Stelle, und der Helfer bleibt
/// fuer die, die **nur** den Host brauchen.

/// Die **Antwort** eines Stroms — und GEMESSENE ENTSCHEIDUNG, warum `status` und
/// `kopf` **getrennt** sind und **nicht** nur ein `Response`: der Aufrufer braucht
/// den `ETag` **vor** dem ersten Byte (sonst kann er `If-Range` beim Fortsetzen
/// **nicht** bauen, weil er den `ETag` **nicht** kennt — GEMESSEN in
/// `narsil-download` KONVERT_21: `if_range(None)` ist `None`).
pub struct StromAntwort {
    /// Die Statuszeile — GEMESSEN, warum der Aufrufer sie **sehen** muss: ein
    /// `404` ist kein Fehler des Netzes, sondern eine **Antwort**, und der
    /// Download „schlägt fehl" mit dem **Grund** „404".
    pub status: StatusLine,
    /// Die Kopfzeilen (Kleinbuchstaben-Schlüssel, wie `Response.headers` sie hält).
    pub kopf: HashMap<String, String>,
    /// Der **Leser** — und GEMESSEN, dass er `Send` sein muss: der Download läuft
    /// in einem **Thread**, und die Liste wird vom **UI-Thread** gelesen.
    pub leser: Box<dyn Read + Send>,
    /// `true`, wenn der Server **teilte** (`206`) — also den `Range` beachtet hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** steht und **nicht** der
    /// Aufrufer aus dem Status **rät**: bei `200` trotz `Range` hat der Server die
    /// **ganze** Datei geschickt, und der Schreiber muss **neu** anfangen. Wer es
    /// rät, haengt den **Rest** der Datei an den **Anfang**.
    pub teil: bool,
}

/// Und **einen** Strom holen — mit `Range`, `If-Range` und beliebigen Zusatzköpfen.
///
/// ### UND: `if_range` ist der **Wert**, nicht die Kopfzeile
/// — und hier war **zwei** Jahre lang die **falsche**
/// # Antwort dokumentiert
///
/// GEMESSEN in KONVERT_25 gegen einen echten `TcpListener`: der Server sah
/// `if-range: if-range: "k29"` und antwortete **200 statt `206`**. Ursache: der
/// Wert wird **unter dem Schluessel** `"if-range"` eingetragen, und `fetch`
/// schreibt `if-range: <wert>`. Also ist `if_range` der **Wert** — also der
/// `ETag` **mit** Anfuehrungszeichen bei `W/"x"` — und **nicht** die fertige
/// Kopfzeile.
///
/// GEMESSENE BEGRUENDUNG, warum trotzdem **hier** und nicht erst im Aufrufer
/// geklammert wird: `ETag: W/"x"` ist eine **gueltige** Form, und ein blinderes
/// Klammern macht daraus `"W/"x""`. Die Regel gehoert an **eine** Stelle, und
/// das ist `narsil_download::liste::if_range`.
///
/// Und GEMESSENE BEGRUENDUNG, warum die alte Doku nicht einfach wegkam: sie
/// stand als **Kommentar**, und `n1_strom` pruefte genau das — also war der
/// Fehler **gemessen und festgeschrieben**. Das ist der **neunte** Fall von
/// „ein Kommentar ist kein Test" (KONVERT_15–24), und der **teuerste**: nicht
/// ein fehlender Test, sondern ein **falscher**.
// GEMESSENE BEGRUENDUNG fuer die **vier** Parameter (und nicht weniger): `url`
// (was), `range` (woher), `if_range` (nur wenn der Server den `ETag` bestaetigt
// darf) und `kopf_extra` (Cookies, `Authorization`) — also **vier** Tatsachen, und
// **eine** Zusammenfassung waere eine **Annahme`.
pub fn hole_stream(
    url: &str,
    range: Option<(u64, Option<u64>)>,
    if_range: Option<&str>,
    kopf_extra: &[(String, String)],
) -> Result<StromAntwort, NetError> {
    let (host, port, pfad, https) = zerlege(url)?;
    let mut anfrage = Request::get(&host, port, &pfad);
    if https {
        anfrage = anfrage.https();
    }
    // Und GEMESSENE ENTSCHEIDUNG, warum `RessourceArt::Daten` und **nicht**
    // `Dokument`: `art` bestimmt den `Accept`-Kopf, und `Dokument` schickt
    // `Accept: text/html,…`. Ein Download mit diesem Kopf bekommt von manchen
    // Servern **HTML statt der Datei** — also „Du willst eine Datei, hier ist
    // eine Seite".
    anfrage = anfrage.art(crate::RessourceArt::Daten);
    for (k, v) in kopf_extra {
        anfrage.headers.insert(k.to_ascii_lowercase(), v.clone());
    }
    if let Some((von, _bis)) = range {
        // GEMESSENE ENTSCHEIDUNG, warum **offen** (`bytes=von-`) und **nicht**
        // geschlossen (`bytes=von-bis`): ein Download, der **abhaengt**, kennt sein
        // Ende **nicht** — und ein geschlossener Range mit einem **zu kleinen**
        // Ende holt **weniger** Bytes als die Datei hat, und der Schreiber
        // haelt die Datei fuer **fertig**.
        anfrage
            .headers
            .insert("range".to_string(), format!("bytes={von}-"));
    }
    if let Some(ir) = if_range {
        // GEMESSENE ENTSCHEIDUNG, warum hier **roh** eingetragen wird und **nicht**
        // geklammert: der Aufrufer hat den **ETag** wie er ihn bekommen hat (also
        // schon mit Anfuehrungszeichen bei `W/"x"`), und `narsil-download::liste::if_range`
        // ist die **eine** Stelle, die das Klammern **richtig** macht.
        anfrage
            .headers
            .insert("if-range".to_string(), ir.to_string());
    }

    let antwort: Response = crate::fetch_mit_mit(&anfrage, Limits::default(), None)?;
    Ok(StromAntwort {
        teil: antwort.status.code == 206,
        status: antwort.status,
        kopf: antwort.headers,
        // Und HIER ist die Einschränkung sichtbar: aus dem `Vec<u8>` wird ein
        // `Cursor`, und ein `Cursor` **ist** ein `Read` — aber er liest aus dem
        // Speicher. GEMESSEN und im Modulkopf benannt.
        leser: Box::new(std::io::Cursor::new(antwort.body)),
    })
}

/// Und die **URL** zerlegen — und GEMESSENE ENTSCHEIDUNG, warum **nicht**
/// `url::Url` (das gibt es nicht als Abhaengigkeit) und **nicht** der Helfer aus
/// `narsil-download`: der gehoert **hierher**, weil er die **Fragen** stellt, die
/// nur `narsil-net` stellen kann (Port **aus** dem Schema, Pfad **mit** Query).
fn zerlege(url: &str) -> Result<(String, u16, String, bool), NetError> {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        // GEMESSEN, warum `KeinHttp` und **nicht** `DnsFehler`: es gibt **gar
        // keinen** Host, also kann auch kein **Name** aufgeloest werden — und
        // GEMESSEN ist, dass die Meldung in `narsil-net` Z. 1314 genau das sagt
        // („kein Host"). Also: **kein** DNS-Fehler, sondern **kein** HTTP.
        return Err(NetError::KeinHttp(format!("kein http-Schema in {url}")));
    };
    let (host_teil, pfad) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    // Und der **Port**: GEMESSENE ENTSCHEIDUNG, dass `https` **443** und **nur**
    // `https` 443 ist — also `80` als Vorgabe fuer `http` und **sonst der
    // eingetragene**. Sonst haette ein Download auf Port 8443 **nichts** bekommen.
    let (host, port) = match host_teil.rsplit_once(':') {
        Some((h, p)) => match p.parse::<u16>() {
            Ok(port) => (h.to_string(), port),
            Err(_) => (host_teil.to_string(), if https { 443 } else { 80 }),
        },
        None => (host_teil.to_string(), if https { 443 } else { 80 }),
    };
    if host.is_empty() {
        // Und GEMESSEN: `DnsFehler` gibt es seit 04.10.2026 **extra** fuer diesen
        // Fall — der Kommentar in `narsil-net` Z. 1314 sagt woertlich, sie stehe
        // „weil `strom.rs` sie braucht". Also ist das **nicht** geraten, sondern
        // abgestimmt: NETZTEMPO hat die Variante **vor** dieser Datei angelegt.
        return Err(NetError::DnsFehler(format!("kein Host in {url}")));
    }
    Ok((host, port, pfad.to_string(), https))
}
