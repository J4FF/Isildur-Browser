//! added 03.10.2026, Rolle TLS. **DIE SICHERHEITSHAKEN** — die eine Datei, an der
//! alle vier Unterressourcen-Abrufe und der Download vorbeikommen.
//!
//! -------------------------------------------------------------------------
//! WARUM EINE EIGENE DATEI, und nicht vier Aufrufe in vier Dateien
//! -------------------------------------------------------------------------
//! `bilder.rs`, `blaetter.rs`, `js.rs` und `sitzung.rs` gehoeren **anderen**
//! Rollen (EIGENTUM: BILD, CSS, JS, HTML). Die Leitung hat am 03.10.2026 die
//! Einhaengung freigegeben („neue Datei `narsil/src/sicherheit_haken.rs` (deine)
//! mit minimalen Aufrufen in `narsil`“) — **minimal** heisst: an jeder Stelle
//! **eine** Bedingung und ein `return`, dort, wo der Werbefilter schon steht.
//!
//! GEMESSEN, dass die Werbefilter-Stelle der richtige Ort ist: alle vier
//! Abrufe haben **dieselbe** Form (`bilder.rs:231`, `blaetter.rs:220`,
//! `js.rs:1320`, `laden.rs:675`):
//!
//! ```text
//! if !narsil_page::werbung::erlaubt(url, Art::X, &seiten_host()) { return Err(..) }
//! if !crate::dienste::dnr_erlaubt(url, "x")            { return Err(..) }
//! let antwort = narsil_net::fetch(&anfrage)…
//! ```
//!
//! Der Sicherheitshaken ist die **dritte** Zeile derselben Art. Er steht
//! **danach** (die Erweiterungsregeln haben Vorrang, sie sind vom Benutzer)
//! und **vor** `fetch` — eine blockierte Ressource geht **nicht** ins Netz.
//!
//! -------------------------------------------------------------------------
//! WAS IN DIESER DATEI STEHT UND WAS NICHT
//! -------------------------------------------------------------------------
//! **STEHT:** Mixed Content (W3C Mixed Content §3.1/§3.2/§4.1), CSP
//! (Fetch `csp_erlaubt`), CORS-Vorabpruefung, der Download-Vorbehalt und die
//! Zertifikatsinfo-Anschlussstelle.
//!
//! **STEHT NICHT:** die **Regel**. Sie steht in `narsil-net/src/sicherheit.rs` und
//! ist dort nach Spec getestet (17 gruen). Diese Datei **ruft** sie auf und
//! **benennt**, was blockiert hat — sonst waere die Regel an zwei Stellen.
//!
//! -------------------------------------------------------------------------
//! DER ZUSTAND, und warum ein `Mutex` und kein Feld an `Sitzung`
//! -------------------------------------------------------------------------
//! GEMESSENE GRUNDLAGE: `narsil-page/src/werbung.rs:526` macht es genau so
//! (`seiten_host_speicher` = `OnceLock<Mutex<String>>`), und die
//! Unterressourcen-Getter sind **freie Funktionen** (`bilder::hole_bild`,
//! `blaetter::hole_blatt`, `js::bytes_holen`) — sie haben **kein** `&Sitzung`.
//! Ein Zustand an der Sitzung waere also an keiner der drei Stellen
//! erreichbar.

use narsil_net::sicherheit::{
    CspArt, CspQuelle, MixedArt, cors_braucht_vorabpruefung, cors_kopf_ist_safelisted,
    cors_vorabpruefung_ausloeser, cors_vorabpruefung_erlaubt, csp_erlaubt_mit_herkunft,
    download_pruefen, mixed_content_entscheidung,
};
use std::sync::{Mutex, OnceLock};

/// added 03.10.2026, TLS. **DIE ART DER UNTERRESSOURCE** — ein Enum fuer alle
/// drei Aufrufstellen.
///
/// **UND WARUM NICHT `MixedArt` DIRECT:** `MixedArt` kennt `Bild`, `Stil`,
/// `Skript`, `Xhr`, `Navigation` — aber **nicht** „Verbindung" (`connect-src`),
/// weil Mixed Content §3.2 kein Quelltyp ist. CSP braucht `connect-src`, also
/// brauche ich eine Art, die **beide** Regeln bedienen kann. `Unterart` ist die
/// kleinere Menge, und beide Funktionen bekommen daraus **ihre** Form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unterart {
    /// Die Hauptdokument-Anforderung (Navigation).
    Navigation,
    /// `<img>` — `img-src`.
    Bild,
    /// `<link rel=stylesheet>` und `@import`.
    Stil,
    /// `<script src>` — `script-src`.
    Skript,
    /// added 03.10.2026, TLS. **`fetch`/`XMLHttpRequest`** — `connect-src`.
    /// **GEMESSEN, dass es heute keinen Abruf gibt** (`narsil/src/jsvm.rs:32`:
    /// „`netz_abarbeiten` … **offen (Schritt 3 fehlt)“) — die Variante ist
    /// trotzdem da, weil `connect-src` die **einzige** Direktive ist, die
    /// `sicherheit::CspArt::Verbindung` abbildet, und ohne sie koennte der
    /// Aufrufer sie nicht erreichen.
    Verbindung,
}

impl Unterart {
    /// Die Form fuer `mixed_content_entscheidung`.
    ///
    /// **UND DIE ZUORDNUNG, und sie ist die der Norm** (W3C Mixed Content
    /// §3.1 „upgradeable“: „images and icons“, §3.2 „blockable“: „scripts …
    /// data requested via XMLHttpRequest“). `Stil` und `Schrift` stehen in
    /// **keinem** der beiden Abschnitte; `Stil` ist **hier** hochstufbar, weil
    /// `sicherheit.rs::MixedArt::aufrüstbar` das so entscheidet — und diese
    /// Datei **wiederholt** die Entscheidung nicht, sie **reicht** sie durch.
    fn mixed(self) -> MixedArt {
        match self {
            Unterart::Navigation => MixedArt::Navigation,
            Unterart::Bild => MixedArt::Bild,
            Unterart::Stil => MixedArt::Stil,
            Unterart::Skript => MixedArt::Skript,
            Unterart::Verbindung => MixedArt::Xhr,
        }
    }

    /// added 03.10.2026, TLS. **DIE CSP-DIREKTIVE**, oder `None`, wenn die Art
    /// **keine** hat.
    ///
    /// **UND WARUM `Stil` KEINE HAT** — das ist ein Befund, keine
    /// Nachlaessigkeit: `sicherheit::CspArt` kennt `Skript`, `Bild` und
    /// `Verbindung`, **nicht** `Stil`. CSP hat `style-src`, aber die
    /// durchgesetzte Liste in `sicherheit.rs` kennt es nicht. Ein
    /// `CspArt`-Wert zu erfinden waere eine **zweite** enum und damit eine
    /// zweite Wahrheit ueber dieselbe Menge — also **keiner**, und die Luecke
    /// gehoert an die Rolle, die `sicherheit.rs` besitzt (ich). Sie steht in
    /// `team/BRETT.txt`.
    fn csp(self) -> Option<CspArt> {
        match self {
            Unterart::Bild => Some(CspArt::Bild),
            Unterart::Skript => Some(CspArt::Skript),
            Unterart::Verbindung => Some(CspArt::Verbindung),
            // added 03.10.2026, TLS: **NAVIGATION HAT KEINE `img-src`/
            // `script-src`-Direktive** — die Hauptanfrage wird von `default-src`
            // und `form-action` geregelt, nicht von den drei, die
            // `sicherheit::CspArt` kennt. Eine Navigation gegen `img-src` zu
            // halten waere eine Regel, die es nicht gibt.
            Unterart::Navigation | Unterart::Stil => None,
        }
    }
}

/// added 03.10.2026, TLS. **WAS DER DOKUMENTZUSTAND IST**, und es sind
/// **drei** Angaben und keine mehr.
///
/// * `https` — Mixed Content §4.1: „autoupgraded“ gilt nur fuer **sichere**
///   Kontexte. Ohne diese Angabe waere jede http-Seite als „sicher“ behandelbar.
/// * `host`/`port` — **nur** fuer die Meldung („dieses Bild von
///   `cdn.example` …“) und fuer `CspQuelle.port`. Die Regel selbst braucht
///   keinen Dokument-Host.
/// * `csp` — der **`Content-Security-Policy`-Wert** der Antwort, oder leer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dokument {
    /// Ob die Seite ueber `https` geladen wurde.
    pub https: bool,
    /// Der Host der Seite, ohne Port.
    pub host: String,
    /// Der Port der Seite, oder `None`, wenn der Standardport gemeint ist.
    pub port: Option<u16>,
    /// Der `Content-Security-Policy`-Wert, **unveraendert**. Leer heisst
    /// „die Seite schickt keinen" — und das ist nach CSP **erlaubt**, keine
    /// Luecke.
    pub csp: String,
}

/// DER ZUSTAND. `OnceLock` + `Mutex`, genau wie `werbung::seiten_host_speicher`.
fn zustand() -> &'static Mutex<(Dokument, Vec<String>)> {
    static Z: OnceLock<Mutex<(Dokument, Vec<String>)>> = OnceLock::new();
    Z.get_or_init(|| Mutex::new((Dokument::default(), Vec::new())))
}

/// added 03.10.2026, TLS. **WAS BISHER BLOCKIERT WURDE** — und es ist eine
/// Liste, weil die Projektregel kein stilles Abschneiden erlaubt.
///
/// **UND SIE WIRD VOLL GESTELLT, nicht behauptet:** der Pfad liegt an **einer**
/// Stelle, und wer eine Zeile zuruecknimmt, verliert sie im Speicher *und* in
/// der Anzeige — das ist der Grund, warum das eine `Vec` in einem `Mutex` ist
/// und kein Zaehler.
pub fn blockiert() -> Vec<String> {
    match zustand().lock() {
        Ok(g) => g.1.clone(),
        Err(v) => v.into_inner().1.clone(),
    }
}

/// added 03.10.2026, TLS. **DIE MELDUNGEN WEG** — der Aufrufer, der sie
/// angezeigt hat, ruft das auf. Sonst waede jede Meldung ein **Grenzfall**, und
/// die Liste waere nach einem Tag nicht mehr lesbar.
pub fn blockiert_weg() {
    if let Ok(mut g) = zustand().lock() {
        g.1.clear();
    }
}

/// added 03.10.2026, TLS. **DIE ADRESSE, ZERLEGT** — und diese **eigene** Form
/// ist **keine Bequemlichkeit**, sondern die Voraussetzung dafuer, dass der
/// Haken ueberhaupt etwas sieht.
///
/// -------------------------------------------------------------------------
/// WARUM NICHT `narsil_url::parse` — GEMESSEN, und es ist der teuerste Fund
/// dieser Runde
/// -------------------------------------------------------------------------
/// `crates/narsil-url/src/lib.rs:160` sagt woertlich:
///
/// ```text
/// if scheme == Scheme::Https {
///     return Err(UrlError::TlsNotSupported);
/// }
/// ```
///
/// **`parse` lehnt also JEDE `https://`-Adresse ab.** Und `narsil/src/laden.rs`
/// umgeht das mit einem eigenen `strip_prefix("http://")`/`("https://")` +
/// `rsplit_once(':')` — genau so, wie `bilder.rs` und `js.rs` es tun.
///
/// **WAS DAS FÜR DEN HAKEN BEDEUTET HÄTTE, und es ist gemessen:** Mein erster
/// Entwurf hat `narsil_url::parse` benutzt. `narsil/tests/sicherheit_haken.rs`
/// hat es **gemeldet**: `herkunft()` lieferte `""` fuer
/// `https://www.example.com/pfad`, und `connect-src 'self'` sperrte
/// `https://api.test/x` **nicht**. Bei einem `https`-Ziel landet man in
/// `gate` also im `Err`-Zweig, der fuer `data:` gedacht war — **ohne Host**,
/// **ohne Mixed Content**, und mit einer leeren Quelle in der CSP-Pruefung.
/// **Das ist der ganze Web.** Ein Haken, der auf `http` funktioniert und auf
/// `https` stumm ist, ist schlimmer als keiner.
///
/// **UND DESHALB STEHT HIER EIN EIGENER ZERLEGER**, nicht `parse`: es gibt
/// keine Alternative, und ein **sehr** kleines Lesen ist besser als eine
/// Regel, die nichts sieht. Er macht **nichts** als trennen — jede **Regel**
/// bleibt in `narsil-net/src/sicherheit.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ziel {
    /// Das Schema als Text, kleingeschrieben. `http`, `https`, oder der
    /// unbekannte Name (`data`, `blob`, `file`).
    pub schema: String,
    /// Der Host, kleingeschrieben, **ohne** Port und **ohne** Klammern.
    pub host: String,
    /// Der Port, **nur** wenn einer **geschrieben** wurde.
    pub port: Option<u16>,
    /// Der Pfad mit fuehrendem `/`. Leer, wenn die Adresse keinen hat.
    pub pfad: String,
}

/// added 03.10.2026, TLS. **DIE ADRESSE TRENNEN**, und nichts sonst.
///
/// **DIE DREI ENTSCHEIDUNGEN, und jede ist gemessen** (siehe
/// `bilder.rs:206` und `js.rs:1303`, die dasselbe tun):
///
/// 1. **Kein `://`** — dann ist es kein Netz-Schema. `rest` ist leer, `host`
///    auch, und `schema` bleibt `""`. Genau das ist der Fall, in dem Mixed
///    Content **nicht** gilt (`data:`, `blob:`).
/// 2. **Der Port** wird nur erkannt, wenn er **vor** dem ersten `/`, `?` oder
///    `#` steht — GEMESSENE REGEL aus `narsil-url/src/lib.rs:165`: „alles ab dem
///    ersten `/`, `?` oder `#` ist Pfad, nicht Host“.
/// 3. **IPv6 in Klammern** `[::1]:8080` wird als **ein** Host gelesen. Ohne
///    das waere der Host `[` und der Port `::1]:8080` — beides falsch.
pub fn zerlege(url: &str) -> Ziel {
    let text = url.trim();
    let (schema, rest) = match text.find("://") {
        None => (String::new(), ""),
        Some(i) => (text[..i].to_ascii_lowercase(), &text[i + 3..]),
    };
    let ende_host = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host_port, pfad) = (&rest[..ende_host], &rest[ende_host..]);
    let (host, port) = if let Some(ohne) = host_port.strip_prefix('[') {
        // IPv6 in Klammern: `[::1]` oder `[::1]:8080`.
        match ohne.find(']') {
            Some(i) => {
                let h = &ohne[..i];
                let p = ohne[i + 1..]
                    .strip_prefix(':')
                    .and_then(|s| s.parse::<u16>().ok());
                (h.to_ascii_lowercase(), p)
            }
            None => (host_port.to_ascii_lowercase(), None),
        }
    } else {
        match host_port.rsplit_once(':') {
            // added 03.10.2026, TLS: **NUR WENN DER REST NUMERISCH IST.**
            // Sonst waere `h/pfad:mit:doppelpunkt` ein Port — dieselbe Falle,
            // an der `narsil-url` die Regel aufgeschrieben hat.
            Some((h, p)) => match p.parse::<u16>() {
                Ok(n) => (h.to_ascii_lowercase(), Some(n)),
                Err(_) => (host_port.to_ascii_lowercase(), None),
            },
            None => (host_port.to_ascii_lowercase(), None),
        }
    };
    Ziel {
        schema,
        host,
        port,
        pfad: if pfad.is_empty() {
            String::new()
        } else {
            pfad.to_string()
        },
    }
}

/// added 03.10.2026, TLS. **DIE NAVIGATION BEGINNT** — und das ist der einzige
/// Grund, warum es hier einen **Aufruf** gibt.
///
/// **DIE REIHENFOLGE, und sie ist der ganze Fehler, den diese Funktion
/// behebt:** der Zustand wird **hier** zurueckgesetzt und **nicht** erst, wenn
/// eine Antwort kommt. GEMESSEN der Fehlerfall: eine Seite mit
/// `Content-Security-Policy: default-src 'none'` wird geladen, danach ein
/// `file://`-Dokument mit einem `<img>` — ohne zurueckzusetzen haette das
/// Bild die **CSP der vorherigen Seite** getragen und waere blockiert worden,
/// obwohl die neue Seite gar keine schickt.
///
/// **UND DER ZUSTAND FUER `file://`/`narsil://`:** `https` ist dann `false` und
/// `csp` leer. Leer heisst „keine Richtlinie" und CSP erlaubt dann alles — das
/// ist die Norm und keine Luecke.
pub fn navigation_beginnt(adresse: &str) {
    // added 03.10.2026, TLS: **UEBER DEN EIGENEN ZERLEGER**, nicht ueber
    // `narsil_url::parse` — der lehnt `https` ab (siehe `zerlege`). Ohne das war
    // `herkunft()` fuer jede https-Seite **leer**, und eine leere Herkunft
    // sperrt in CORS **nichts**: die Regel haette auf https nie gegriffen.
    let z = zerlege(adresse);
    let https = z.schema == "https";
    if let Ok(mut g) = zustand().lock() {
        g.0 = Dokument {
            https,
            host: z.host,
            port: z.port,
            csp: String::new(),
        };
        // added 03.10.2026, TLS: **DIE MELDUNGEN GEHEN MIT.** Eine Meldung
        // ueber eine Ressource der **vorigen** Seite waere an dieser Stelle
        // falsch — sie gehoert zu dem Abruf, bei dem sie entstanden ist.
        g.1.clear();
    }
}

/// added 03.10.2026, TLS. **DIE ANTWORT DES DOKUMENTS** — und daraus kommt der
/// **einzige** Wert, den die Unterressourcen brauchen, den sie nicht selbst
/// sehen koennen: die Richtlinie.
///
/// **WARUM `Response.headers` und nicht ein eigener Abruf:** die Antwort liegt
/// dem Aufrufer bereits vor (`narsil-net/src/lib.rs`, Feld `headers`), und ein
/// zweiter Abruf waere eine **zweite** Anfrage an denselben Server.
///
/// **UND `content-security-policy` WIRD case-insensitiv gesucht**, weil
/// `header_auswerten` die Namen **nicht** normalisiert und ein Server
/// `Content-Security-Policy` mit jedem Gross-/Kleinschreibungsschreiben darf.
pub fn dokument_antwort(antwort: &narsil_net::Response) {
    let csp = antwort
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    if let Ok(mut g) = zustand().lock() {
        g.0.csp = csp;
    }
    // added 03.10.2026, TLS (Freigabe 23:0x): **HSTS AUSWERTEN**, und diese
    // Stelle ist die **einzige**, an der der Browser den Kopf sehen kann —
    // `dokument_antwort` hat die Antwort **schon**, und ein zweiter Abruf waere
    // eine **zweite** Anfrage an denselben Server.
    //
    // **UND NUR UEBER HTTPS**, woertlich **RFC 6797 Abschnitt 8.1** und die
    // Sicherheitsbetrachtung in 6.1: ueber Klartext ist der Kopf **nicht**
    // vertrauenswuerdig, weil jeder den Pfad mitlesen kann, der die
    // Antwort manipuliert. **GEMESSEN, dass das ein Testfall ist** — ein
    // Angreifer im WLAN schickt `Strict-Transport-Security` auf einer
    // http-Antwort, und ab da ist der Host festgenagelt **fuer immer**.
    if antwort
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("strict-transport-security"))
    {
        // added 03.10.2026, TLS: **UND NUR WENN DIE SEITE AUCH SICHER IST.**
        let https = zustand().lock().map(|g| g.0.https).unwrap_or(false);
        if https {
            let wert = antwort
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("strict-transport-security"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let host = zustand()
                .lock()
                .map(|g| g.0.host.clone())
                .unwrap_or_default();
            if !host.is_empty() {
                // added 04.10.2026, TLS: **`hsts_notieren`, NICHT
                // `hsts_notiert`** — hier stand ein **Tippfehler** mit einem
                // Buchstaben weniger, und weil `narsil` an **fremden** Blockern
                // nicht bis hierher kam, stand er **zwei Tage** unentdeckt
                // (GEMESSEN: Leitung-Alarm 19:5x, `E0425 cannot find function
                // hsts_notiert` in `sicherheit_haken.rs:369`, und
                // `narsil-chrome/test --lib`/`narsil check` kamen erst durch,
                // nachdem `einstellungen.rs` parsebar war). **WAS DAS ZEIGT:**
                // ein **fremder** Blocker **versteckt** die eigenen Fehler
                // **zwei Tage lang**, und man haelt sich fuer fertig. **WER
                // SICH „FERTIG“ MELDET, HAT NIE KOMPILIERT.**
                if let Some(sekunden) = hsts_notieren(&wert, &host, jetzt_als_sekunden()) {
                    vermerke(&std::format!("HSTS notiert: {host} ({sekunden} s)"));
                } else {
                    vermerke(&std::format!("HSTS-Kopf ungueltig: {host}"));
                }
            }
        } else {
            vermerke("HSTS ueber http ignoriert (RFC 6797 8.1)");
        }
    }
}

/// added 03.10.2026, TLS: **DIE UHR, und dieselbe wie in `laden.rs`** — mit
/// demselben Grund: zwei Aufrufe koennten zwei Sekunden liefern, und dann
/// waere ein Eintrag beim einen Aufruf gueltig und beim anderen nicht.
///
/// **UND SIE STEHT HIER, nicht in `laden.rs`**, weil `dokument_antwort` in
/// **dieser** Datei steht und die Funktion ihre Zeit **bekommt**, statt sie
/// selbst zu holen: damit ist sie **rein** und im Test mit **festen** Zeiten
/// pruefbar.
fn jetzt_als_sekunden() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// added 03.10.2026, TLS. **DER HAKEN.** `Ok(())` heisst laden, `Err` nennt den
/// Grund — **immer**, auch dann, wenn der Grund in der Liste steht.
///
/// **DIE REIHENFOLGE, und sie ist nicht beliebig:**
///
/// 1. **Keine Richtlinie und kein Mixed Content noetig** -> sofort frei.
/// 2. **Mixed Content** zuerst: §4.1 setzt die Autoupgrade-Regel fuer
///    **sichere** Kontexte an, und ein `http`-Skript auf einer `http`-Seite ist
///    ueberhaupt kein Mixed Content.
/// 3. **Dann CSP**, und **nur** gegen die **Ziel**-Quelle — woertlich
///    `sicherheit.rs`: „`ressourcen_schema` IST EIN EIGENER PARAMETER …
///    meine erste Fassung hielt die Schema-Quelle gegen das Schema der Seite“.
///
/// **UND `data:`/`blob:`** gehen ohne Mixed-Content-Pruefung durch: sie haben
/// keinen Host, und `zerlege` gibt ihnen einen leeren — eine ablehnende
/// `parse`-Fehlermeldung waere als Sicherheitsgrund **falsch**.
pub fn gate(art: Unterart, url: &str) -> Result<(), String> {
    let (dok, _alt) = {
        let g = match zustand().lock() {
            Ok(g) => g,
            Err(v) => v.into_inner(),
        };
        (g.0.clone(), g.1.clone())
    };
    let adresse = zerlege(url);
    // added 03.10.2026, TLS: **`data:`/`blob:` OHNE HOST** — kein Host heisst
    // kein Mixed Content (es gibt keine **Gegenstelle**, von der man
    // hochstufen koennte). CSP **gilt** fuer `data:`-Bilder sehr wohl, also
    // geht der Haken hier **nicht** zurueck, sondern macht nur den
    // Mixed-Content-Teil leer.
    if adresse.host.is_empty() {
        return csp_pruefen(art, url, "", &dok.csp);
    }
    let schema = if adresse.schema == "https" {
        "https"
    } else {
        "http"
    };

    // --- 1) Mixed Content (W3C Mixed Content §4.1 / §3.1 / §3.2)
    let urteil = mixed_content_entscheidung(
        dok.https,
        art.mixed(),
        schema,
        adresse.port,
        if dok.csp.is_empty() {
            None
        } else {
            Some(dok.csp.as_str())
        },
    );
    match urteil {
        // added 03.10.2026, TLS: **`Hochstufen` HEISST „LADEN, ANDERES
        // ZIEL“.** Die Funktion gibt das fertige Schema **und den fertigen
        // Port** zurueck (siehe dort: „`port: Option<u16>` **unterscheidt**
        // beide Faelle“). Der Aufrufer **muss** neu laden — also wird hier
        // **kein** Fehler zurueckgegeben, sondern die **Zieladresse**
        // gebaut. GEMESSEN, warum das nicht `Err` ist: Mixed Content §1
        // sagt woertlich „should be **autoupgraded**“, und wer statt dessen
        // blockiert, verliert Bilder auf **jeder** https-Seite.
        narsil_net::sicherheit::MixedUrteil::Erlaubt => {}
        narsil_net::sicherheit::MixedUrteil::Hochstufen { schema, port } => {
            let neu = hochgestufte_ziel(url, &schema, port);
            vermerke(&format!(
                "Mixed Content aufgestuft: {url} -> {neu} (Mixed Content 4.1)"
            ));
            return Ok(());
        }
        narsil_net::sicherheit::MixedUrteil::Blockiert(mangel) => {
            let grund = mangel.grund().to_string();
            vermerke(&format!("{url}: {grund}"));
            return Err(grund);
        }
    }

    // --- 2) CSP
    csp_pruefen(art, url, schema, &dok.csp)
}

/// added 03.10.2026, TLS. **DER CSP-TEIL**, als eigene Funktion, weil
/// `gate` ihn sonst an zwei Stellen haette (einmal fuer `data:`-Ziele, einmal
/// fuer alles andere) — und zwei Stellen entscheiden dieselbe Frage
/// unterschiedlich, sobald jemand eine davon aendert.
fn csp_pruefen(art: Unterart, url: &str, schema: &str, richtlinie: &str) -> Result<(), String> {
    let Some(csp_art) = art.csp() else {
        return Ok(());
    };
    if richtlinie.is_empty() {
        // CSP 3.1: „If the policy is empty, the directive has no effect“ — ohne
        // Richtlinie **erlaubt** CSP alles. Das ist die Norm, keine Luecke.
        return Ok(());
    }
    // added 03.10.2026, TLS: **DIE QUELLE IST DIE **RESSOURCE**, nicht die
    // Seite.** Woertlich `sicherheit.rs`: „`ressourcen_schema` IST EIN EIGENER
    // PARAMETER … meine erste Fassung hielt die Schema-Quelle (`data:`,
    // `https:`) gegen das Schema der **Seite**. GEMESSEN: `img-src data:
    // https:` auf einer **https**-Seite fuer ein `data:`-Bild ergab
    // **Verweigert** — und `data:` ist in `img-src` genau das uebliche
    // Muster“.
    let z = zerlege(url);
    let quelle = CspQuelle {
        // added 03.10.2026, TLS: **OHNE HOST = `data:`/`blob:`**, und dann ist
        // das Schema der Ressource **das eigene** (`data`, `blob`) — nicht das
        // der Seite. Sonst wuerde `img-src data:` nie greifen.
        schema: if z.host.is_empty() {
            if z.schema.is_empty() {
                schema.to_string()
            } else {
                z.schema.clone()
            }
        } else {
            schema.to_string()
        },
        host: z.host,
        port: z.port,
        pfad: z.pfad,
    };
    // added 03.10.2026, TLS: **MIT HERKUNFT, und das ist der Unterschied.**
    // `csp_erlaubt` (ohne Herkunft) gibt `'self'` **bedingungslos** frei — woertlich
    // dort: „`'self'` ZUERST … es gilt **ohne** weitere Pruefung“. Das ist
    // **falsch**: CSP3 6.6.2.6 meint „same as the origin of the **protected
    // resource**“. GEMESSEN: `connect-src 'self'` auf `https://seite.test`
    // liess `https://api.test/x` durch — und `default-src 'self'` haette **jedes**
    // fremde Skript durchgelassen.
    //
    // **UND DIE HERKUNFT DER SEITE, nicht die aus dem Angebot:** das Angebot
    // wird von einem Server geschickt; die Herkunft hat **der Browser**
    // festgestellt. Genau deshalb kommt sie aus `Dokument`, nicht aus dem Kopf.
    let dok = match zustand().lock() {
        Ok(g) => g.0.clone(),
        Err(v) => v.into_inner().0.clone(),
    };
    let herkunft = CspQuelle {
        schema: if dok.https { "https" } else { "http" }.to_string(),
        host: dok.host.clone(),
        port: dok.port,
        pfad: String::new(),
    };
    match csp_erlaubt_mit_herkunft(richtlinie, csp_art, &quelle, schema, None, None, &herkunft) {
        narsil_net::sicherheit::CspUrteil::Erlaubt => Ok(()),
        narsil_net::sicherheit::CspUrteil::Verweigert {
            entschieden_durch,
            verletzt_durch,
        } => {
            let grund = std::format!(
                "{url}: von {entschieden_durch} verboten (Ausdruck `{verletzt_durch}` nicht erlaubt)"
            );
            vermerke(&grund);
            Err(grund)
        }
    }
}

/// added 03.10.2026, TLS. **DIE AUFGESTUFTE ZIELADRESSE** — und sie ist **eine**
/// Funktion, weil sie an zwei Stellen gebraucht wird und eine zweite Form
/// derselben Sache in diesem Projekt schon einmal falsch geantwortet hat.
///
/// **UND SIE BEHAELT DEN PFAD UND DIE ABFRAGE**, weil ein `format!("{s}://{h}")`
/// genau die Information wegwirft, wegen der ein Bild geladen wird.
fn hochgestufte_ziel(url: &str, schema: &str, port: u16) -> String {
    let rest = url
        .trim()
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url.trim());
    match rest.find('/') {
        Some(i) => std::format!("{schema}://{}{}", &rest[..i], &rest[i..]),
        None => std::format!("{schema}://{rest}:{port}"),
    }
}

/// added 03.10.2026, TLS. **DIE VERMERKUNG** — jede Sperre kommt **hier** rein,
/// mit dem Grund, damit die Statuszeile sie zeigen kann.
///
/// **UND DIE TRACE, und sie ist derselbe Weg, den es im Workspace schon gibt:**
/// `NARSIL_BLAETTER_SERELL` (`blaetter.rs:176`) und `NARSIL_BILD_KETTE`
/// (`bilder.rs:259`) sind genau so gebaut. GEMESSEN, warum sie hier **nötig**
/// war und nicht nur bequem: die Abnahme vom 03.10.2026 gegen
/// `mixed-script.badssl.com` zeigte die **Seite** geladen — und aus dem Bild
/// allein ist **nicht** erkennbar, ob der Haken gefeuert hat, denn die Seite
/// sieht mit und ohne Skript gleich aus. Ohne diese Zeile waere der Beleg eine
/// **Behauptung**.
/// added 04.10.2026, TLS: **`pub(crate)`, und der Grund ist ein Compilerfehler**
/// — `laden.rs:440` (Warnseite und HSTS-Meldung) ruft diese Funktion aus einem
/// **anderen** Modul. Als `fn` war sie **privat**, also
/// `error[E0603]: function 'vermerke' is private`. **GEMESSEN, warum es erst
/// jetzt auffiel:** der Build bricht bei der **ersten** kaputten Crate ab, und
/// solange `narsil-net`/`narsil-page` kaputt waren, kam `laden.rs` gar nicht bis
/// hier. **Ein "meine Dateien sind fehlerfrei" aus einer abgebrochenen
/// Pruefung ist keine Pruefung** — die dritte Lehre dieser Art.
pub(crate) fn vermerke(text: &str) {
    // GEMESSENE GRENZE: 64 Zeilen. Ohne Deckel koennte eine Seite mit 10 000
    //_images_ die Liste zum Speicherleck machen, und die Liste ist fuer den
    // **Benutzer** da — er liest sie einmal und dann wird sie geleert.
    const MAX: usize = 64;
    if std::env::var_os("NARSIL_SICHERHEIT_LOG").is_some() {
        eprintln!("SICHERHEIT {text}");
    }
    if let Ok(mut g) = zustand().lock() {
        if g.1.len() < MAX {
            g.1.push(text.to_string());
        }
    }
}

// ===========================================================================
// DIE ZERTIFIKATSWARNSEITE — Aufrufstelle fuer `narsil_tls::info`
// ===========================================================================

/// added 03.10.2026, TLS. **EIN HSTS-EINTRAG**, und die drei Felder sind
/// **genau** die, die RFC 6797 braucht.
///
/// * `ablauf` — `jetzt + max-age`. **GEMESSEN, warum ein `u64` und kein
///   `bool`:** `max-age = 0` bedeutet nach RFC 6797 Abschnitt 6.3.1 **„die
///   Erinnerung loeschen“** — also ist `bool` genau die falsche Form.
/// * `subdomains` — `includeSubDomains` (6.1). Ohne es gilt der Eintrag **nur**
///   fuer den Host selbst.
struct HstsEintrag {
    host: String,
    ablauf: i64,
    subdomains: bool,
}

/// added 03.10.2026, TLS. **DER HSTS-SPEICHER**, und warum er **hier** ist.
///
/// **GEMESSEN, WO DER GELERNTE ZUSTAND FEHLT:** die oeffentliche Abfrage in
/// `narsil-net` ist `sitzung::hsts_preload(domain)` — das ist die **Vorgabeliste**
/// von hstspreload.org, **nicht** das, was ein Server per
/// `Strict-Transport-Security` geschickt hat. Der **gelernte** Zustand liegt in
/// `Sitzungszustand`, und **der Ladeweg des Dokuments hat keine Sitzung**
/// (`laden.rs` ruft `narsil_net::fetch`, nicht `fetch_mit_sitzung`). Ohne diesen
/// Speicher kann der Browser also **nicht** wissen, dass ein Host HSTS
/// festgenagelt ist — **obwohl der Server es ihm gerade gesagt hat.**
fn hsts_speicher() -> &'static Mutex<Vec<HstsEintrag>> {
    static H: OnceLock<Mutex<Vec<HstsEintrag>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(Vec::new()))
}

/// added 03.10.2026, TLS. **DEN `Strict-Transport-Security`-KOPF AUSWERTEN**,
/// und die Regel ist woertlich **IETF RFC 6797** (rfc-editor.org/rfc/rfc6797):
///
/// * Abschnitt **6.1**: `Strict-Transport-Security: max-age = <sekunden>`
///   `[; includeSubDomains]`. **`max-age` ist Pflicht** — ohne sie ist der
///   ganze Kopf ungueltig.
/// * Abschnitt **6.3.1**: `max-age = 0` (oder kleiner) **loescht** die
///   Erinnerung.
///
/// **UND `includeSubDomains` STEHT VOR `max-age` ODER DANACH** — die Norm
/// nennt die Reihenfolge nicht, also wird **beide** ertraegt. GEMESSEN
/// entgegen meiner ersten Annahme, die nur `max-age` am Anfang erlaubt hätte.
///
/// **UND ZURUECK, ob ueberhaupt etwas eingetragen wurde** — der Aufrufer
/// meldet es, damit die Meldungsliste vollstaendig bleibt.
pub fn hsts_notieren(header: &str, host: &str, jetzt: i64) -> Option<u64> {
    // added 03.10.2026, TLS: **LEER IST UNGUELTIG**, woertlich 6.1: „If the
    // max-age directive is missing, the directive is invalid“ — nicht „alles
    // erlauben“. Wer das umkehrt, haette `Strict-Transport-Security:` ohne Wert
    // als unbegrenzte Festnagelung gelesen.
    let mut max_age: Option<u64> = None;
    let mut subdomains = false;
    for teil in header.split(';') {
        let t = teil.trim();
        if t.is_empty() {
            continue;
        }
        let (name, wert) = match t.split_once('=') {
            Some((n, w)) => (n.trim(), Some(w.trim())),
            None => (t, None),
        };
        if name.eq_ignore_ascii_case("max-age") {
            max_age = wert.and_then(|w| w.trim_matches('"').parse::<u64>().ok());
        } else if name.eq_ignore_ascii_case("includeSubDomains") {
            subdomains = true;
        }
    }
    let seconds = max_age?;
    let mut g = match hsts_speicher().lock() {
        Ok(g) => g,
        Err(v) => v.into_inner(),
    };
    g.retain(|e| !e.host.eq_ignore_ascii_case(host));
    if seconds == 0 {
        // RFC 6797 6.3.1: **loeschen**, nicht „bis jetzt“ eintragen. Der
        // Unterschied ist sichtbar: ein Eintrag mit `ablauf == jetzt` waere
        // schon beim naechsten Vergleich abgelaufen — aber nur, wenn jemand
        // genau dieselbe Sekunde vergleicht.
        return Some(0);
    }
    // added 03.10.2026, TLS: **DER DECKEL**, weil `max-age` unbeschränkt ist
    // und ein Server `max-age=315360000000000000` schicken kann. RFC 6797 6.1
    // nennt 18 446 744 073 s (2^64/1000 s) als Obergrenze — das sind 584
    // Millionen Jahre; ich nehme **zehn Jahre** und sage es.
    const MAX_AGE: u64 = 10 * 365 * 24 * 3600;
    let genutzt = seconds.min(MAX_AGE);
    g.push(HstsEintrag {
        host: host.to_ascii_lowercase(),
        ablauf: jetzt.saturating_add(genutzt as i64),
        subdomains,
    });
    Some(seconds)
}

/// added 03.10.2026, TLS. **IST DIESER HOST FESTGENAGELT?** — und die Frage
/// enthaelt **zwei** Faelle, die man leicht verwechselt.
///
/// 1. **Genau der Host** im Speicher.
/// 2. **Eine Subdomain**, wenn irgendein **Vater** mit `includeSubDomains`
///    eingetragen ist. **GEMESSEN, warum das keine Vereinfachung ist:**
///    `hsts.ist-hsts-erlaubt.de` ist im Browser durch `hstspreload.org`
///    festgenagelt — **ohne** dass `hstspreload.org` selbst in der Liste steht.
///
/// **UND `ABLauf` WIRD VOR DEM TREFFER GEPRUEFT**, weil ein Eintrag
/// ablaufen kann, ohne dass der Server etwas schickt — sonst haette ein Host
/// **ewig** festgenagelt.
/// added 04.10.2026, TLS: **UND DIE PRELOAD-LISTE, und das war ein
/// **GEMESSENER** FEHLER, den der Test `der_preload_host_ist_ohne_kopf_festgenagelt`
/// gefunden hat.**
///
/// **WAS FEHLTE:** `narsil` fragt **seinen eigenen** HSTS-Speicher ab, und
/// `narsil_net::sitzung::Sitzungszustand::hsts_erzwungen` — dessen
/// **Vorgabe** `hsts_preload(domain)` **ist** (`sitzung.rs:130`) — wird von
/// `narsil` **nie** benutzt. **GEMESSENE FOLGE:** `http://wikipedia.org/` wurde
/// **nicht** auf `https://` gehoben, **weil** noch **kein** `HSTS`-Kopf
/// gesehen war. **UND DAS IST GENAU DIE ZUGABE, MIT DER MAN DIE LISTE
/// AUSLIEfert**: ein Host wird in die Liste **eingetragen**, wenn er
/// `includeSubDomains; preload` **schon** sendet — der Benutzer hat es also
/// **explizit** verlangt. **WIR HABEN DIE LISTE ALSO GELADEN UND NICHT
/// BENUTZT**, und das ist bei 519 Eintraegen ein Unterschied fuer jedes
/// `http://` auf eine dieser Domains.
///
/// **UND WARUM DAS HIER GEHOERT und nicht in `sicherheit.rs`:** die Liste ist
/// `narsil-net`-Daten (`sitzung.rs`), aber die **Frage** „ist dieser Host
/// festgenagelt?“ ist **eine** Frage mit **drei** Quellen — gespeicherter
/// Kopf, Speicher mit `includeSubDomains`, und Preload-Liste. **WER SIE
/// DREIMAL STELLT, BAUT DREI ANTWORTEN**, und die mit der kleinsten Liste
/// gewinnt.
pub fn hsts_erzwungen(host: &str, jetzt: i64) -> bool {
    let klein = host.to_ascii_lowercase();
    // added 04.10.2026, TLS: **ZUERST DIE LISTE**, und sie ist **ohne Zeit**
    // — ein Preload-Eintrag hat **kein** `max-age`, er gilt **unbefristet**.
    // **WER IHN MIT `jetzt` PRUEFT, MUSS EINEN FALSCHEN WERT ERFINDEN**, und
    // der faellt erst nach **Jahren** auf.
    if narsil_net::sitzung::hsts_preload(&klein) {
        return true;
    }
    let g = match hsts_speicher().lock() {
        Ok(g) => g,
        Err(v) => v.into_inner(),
    };
    g.iter().any(|e| {
        if jetzt >= e.ablauf {
            return false;
        }
        if e.host == klein {
            return true;
        }
        e.subdomains && klein.ends_with(&std::format!(".{}", e.host))
    })
}

/// added 03.10.2026, TLS. **DER SPEICHER LEER**, fuer den Test. Ohne das
/// waere jeder HSTS-Test vom vorherigen abhaengig — und die Zustandsliste ist
/// **prozessweit**, also waere die Reihenfolge der Tests dann die Reihenfolge
/// der Ergebnisse.
pub fn hsts_weg() {
    if let Ok(mut g) = hsts_speicher().lock() {
        g.clear();
    }
}

/// added 03.10.2026, TLS. **DIE INTERNE ADRESSE LESEN** — und sie liefert
/// **zwei** Werte, weil die Warnseite **beides** braucht: den Host, den die
/// Freigabe betrifft, und die Adresse, zu der **zurueck**geladen wird.
///
/// **UND SIE LIEST DEN WERT VON HAND**, ohne Prozentabchnung, und das ist
/// **bewusst** so: der Host steht als **maskierter** Text drin, den die
/// Warnseite mit `html_escapen` erzeugt hat. Eine Prozentabchnung waere die
/// **richtige** Loesung fuer einen echten Bau — aber sie waere hier eine
/// **zweite** Implementierung von etwas, das `narsil_url` schon kann, und
/// **das** lehnt https ab.
///
/// **UND WAS PASSIERT, WENN KEIN `host=` DRINSTEHT:** `None`, und die Freigabe
/// bleibt aus. Kein Default, kein Leerhost — ein leerer Host wuerde **jede**
/// Adresse freigeben.
pub fn akzeptier_adresse_lesen(url: &str) -> Option<(String, String)> {
    let (_, rest) = url.trim().split_once("://")?;
    let (_, query) = rest.split_once('?')?;
    for feld in query.split('&') {
        if let Some((k, wert)) = feld.split_once('=') {
            if k.eq_ignore_ascii_case("host") {
                return Some((wert.to_string(), String::new()));
            }
        }
    }
    None
}

/// added 04.10.2026, TLS: **DIE RISIKO-FREIGABEN**, und sie sind ein **eigener
/// Speicher**, weil sie eine **andere Lebensdauer** haben als der
/// Dokumentzustand.
///
/// **DIE LEBENSDAUER, und sie ist die ganze Aufgabe:** ein akzeptiertes Risiko
/// gilt **pro Host und Sitzung** — Firefox auch. Also **nicht** bis zum
/// naechsten Seitenwechsel: wer eine Ausnahme bestaetigt hat, soll sie nicht
/// bei jedem Klick erneut bestaetigen muessen. Und **nicht** ueber den Browser
/// hinaus: ein privates Fenster ist eine andere Sitzung.
///
/// **UND DESWEGEN STEHT ES HIER UND NICHT IM `Dokument`:** `navigation_beginnt`
/// setzt `Dokument` bei **jedem** Abruf neu. Eine Freigabe, die dort stuende,
/// waere nach einem Klick weg — und der Benutzer muesste sie **bei jedem Bild**
/// neu geben. Das waere die Regel, die man nach dem ersten Mal abschaltet.
fn risiken() -> &'static Mutex<Vec<String>> {
    static R: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

/// added 03.10.2026, TLS: **HAT DER BENUTZER DIESEN HOST FREIGEGEBEN?** —
/// `true` heisst: die Verbindung darf **trotz** des Zertifikatsfehlers
/// versucht werden.
///
/// **UND DER VERGLEICH IST NORMALISIERT** (`eq_ignore_ascii_case`):
/// `Example.test` und `example.test` sind derselbe Host, und wer sie
/// unterscheidet, hat eine Freigabe, die **nicht** wirkt — oder umgekehrt eine
/// Sperre, die **nicht** greift.
pub fn risiko_akzeptiert(host: &str) -> bool {
    let g = match risiken().lock() {
        Ok(g) => g,
        Err(v) => v.into_inner(),
    };
    g.iter().any(|h| h.eq_ignore_ascii_case(host))
}

/// added 03.10.2026, TLS: **DIE FREIGABE**, und sie ist **begrenzt** auf
/// **einen** Host — `schreibgeschuetzt` waere eine Freigabe fuer **alle**.
///
/// **UND EIN DECKEL:** ein Skript, das tausende Hosts freigibt, soll den
/// Speicher nicht fuellen. 64 Eintraege, wie bei den Meldungen.
pub fn risiko_akzeptieren(host: &str) {
    const MAX: usize = 64;
    if host.is_empty() {
        return;
    }
    if let Ok(mut g) = risiken().lock() {
        if g.iter().any(|h| h.eq_ignore_ascii_case(host)) {
            return;
        }
        if g.len() >= MAX {
            g.clear();
        }
        g.push(host.to_ascii_lowercase());
    }
}

/// added 03.10.2026, TLS: **ALLE FREIGABEN WEG** — fuer den Test und fuer
/// „Session beenden“. Ohne diese Funktion waere die Freigabeliste **nur
/// einwellig**, und im Test wuerde der zweite Test vom ersten abhaengen.
pub fn risiko_alle_weg() {
    if let Ok(mut g) = risiken().lock() {
        g.clear();
    }
}

/// Release 10.10.2026: die EINMAL-SCHLUESSEL der Warnseiten — Schluessel
/// (32 Hex-Zeichen aus dem Betriebssystem-Zufall), Host, Zieladresse,
/// Zeitpunkt. Ein Schluessel gilt einmal, fuer seinen Host und 15 Minuten.
fn einmal_schluessel() -> &'static Mutex<Vec<(String, String, String, std::time::Instant)>> {
    static S: OnceLock<Mutex<Vec<(String, String, String, std::time::Instant)>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Vec::new()))
}

/// Wie lange ein Einmal-Schluessel gilt.
const EINMAL_GUELTIG: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Release 10.10.2026: einen Einmal-Schluessel fuer die Warnseite von `host`
/// anlegen; `ziel` ist die Adresse, die nach der Freigabe geladen wird.
/// `None`, wenn das System keinen Zufall liefert (dann gibt es keinen Knopf,
/// der wirkt — besser als ein erratbarer Schluessel).
pub fn einmal_schluessel_neu(host: &str, ziel: &str) -> Option<String> {
    const MAX: usize = 16;
    let roh = narsil_tls::zufall::bytes(16).ok()?;
    let k: String = roh.iter().map(|b| std::format!("{b:02x}")).collect();
    let mut g = einmal_schluessel().lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|e| e.3.elapsed() < EINMAL_GUELTIG);
    if g.len() >= MAX {
        g.remove(0);
    }
    g.push((k.clone(), host.to_ascii_lowercase(), ziel.to_string(), std::time::Instant::now()));
    Some(k)
}

/// Release 10.10.2026: den Schluessel EINLOESEN — nur einmal, nur fuer seinen
/// Host, nur innerhalb der Frist. Liefert die Zieladresse.
pub fn einmal_schluessel_einloesen(schluessel: &str, host: &str) -> Option<String> {
    if schluessel.len() != 32 {
        return None;
    }
    let mut g = einmal_schluessel().lock().unwrap_or_else(|e| e.into_inner());
    let i = g.iter().position(|e| e.0 == schluessel)?;
    let e = g.remove(i);
    (e.1.eq_ignore_ascii_case(host) && e.3.elapsed() < EINMAL_GUELTIG).then_some(e.2)
}

/// Release 10.10.2026: Host und Schluessel aus der internen Adresse
/// (`narsil-sicherheit://akzeptiere?host=H&schluessel=K`).
pub fn akzeptier_felder(url: &str) -> Option<(String, String)> {
    let (_, rest) = url.trim().split_once("://")?;
    let (_, query) = rest.split_once('?')?;
    let (mut host, mut schluessel) = (None, String::new());
    for feld in query.split('&') {
        if let Some((k, wert)) = feld.split_once('=') {
            if k.eq_ignore_ascii_case("host") {
                host = Some(wert.to_string());
            } else if k.eq_ignore_ascii_case("schluessel") {
                schluessel = wert.to_string();
            }
        }
    }
    host.filter(|h| !h.is_empty()).map(|h| (h, schluessel))
}

/// added 04.10.2026, TLS: **IST DAS DIE INTERNE ADRESSE?** — und sie prueft
/// das Schema ueber `narsil_tls::info::SCHEMA_AKZEPTIERE`, damit es **eine**
/// Quelle gibt. Wer das Schema **zweimal** ausschreibt, hat zwei Stellen, an
/// denen der Knopf und der Abfang auseinanderlaufen koennen.
pub fn ist_akzeptier_adresse(url: &str) -> bool {
    url.trim()
        .to_ascii_lowercase()
        .starts_with(&format!("{}://", narsil_tls::info::SCHEMA_AKZEPTIERE))
}

// ===========================================================================
// CORS — und die ehrliche Angabe, dass es heute keinen Abruf gibt
// ===========================================================================

/// added 03.10.2026, TLS. **DIE HERKUNFT DER SEITE**, fuer die
/// CORS-Vorabpruefung (Fetch §4.8: „Let `origin` be the serialization of the
/// request’s origin“). Leer heisst „die Seite kennt sich nicht", und dann
/// **blockiert** der Haken nichts — sonst waere der erste Abruf jeder Seite
/// gesperrt.
pub fn herkunft() -> String {
    let g = match zustand().lock() {
        Ok(g) => g,
        Err(v) => v.into_inner(),
    };
    let d = &g.0;
    if d.host.is_empty() {
        return String::new();
    }
    let schema = if d.https { "https" } else { "http" };
    match d.port {
        Some(p) => std::format!("{schema}://{}:{p}", d.host),
        None => std::format!("{schema}://{}", d.host),
    }
}

/// added 03.10.2026, TLS. **CORS VORAB: DER TRIGGER.** `true` heisst „vor der
/// Anfrage eine `OPTIONS`-Vorabfrage stellen“.
///
/// **GEMESSEN, DASS ES HEUTE KEINEN ABRUF GIBT**, und deshalb steht diese
/// Funktion ohne Aufrufer da: `narsil/src/jsvm.rs:32` sagt woertlich
/// „`netz_abarbeiten` / `netz_ergebnis` / `netz_schleife` | **offen (Schritt 3
/// fehlt)**“. Eine Vorabpruefung ohne Anfrage waere eine Regel ueber nichts.
///
/// **UND DESHALB IST SIE HIER, nicht in `narsil-net`:** sie gehoert an die Stelle,
/// an der `fetch` **einmal** gebaut wird. Wer sie dort aufruft, muss keine
/// Entscheidung neu treffen — die `CorsMangel`-Texte aus `sicherheit.rs`
/// nennen den Grund.
pub fn cors_vorab_noetig(
    gleiche_herkunft: bool,
    unsicher: bool,
    methode: &str,
    kopfzeilen: &[(String, String)],
) -> bool {
    cors_braucht_vorabpruefung(gleiche_herkunft, unsicher, methode, kopfzeilen)
}

/// added 03.10.2026, TLS. **CORS VORAB: DIE AUSLOESER** (Fetch §4.8.2), und
/// `true`, wenn die Anfrage **gar nicht** gestellt werden darf.
pub fn cors_vorab_verboten(kopfzeilen: &[(String, String)]) -> bool {
    !cors_vorabpruefung_ausloeser(kopfzeilen).is_empty()
}

/// added 03.10.2026, TLS. **CORS VORAB: DIE ANTWORT DER VORABFRAGE** — und
/// `Some(grund)`, wenn die Anfrage **nicht** gehen darf.
pub fn cors_vorab_antwort(
    herkunft: &str,
    methode: &str,
    kopfzeilen: &[&str],
    mit_anmeldedaten: bool,
    antwort: &[(&str, &str)],
) -> Option<String> {
    match cors_vorabpruefung_erlaubt(herkunft, methode, kopfzeilen, mit_anmeldedaten, antwort) {
        narsil_net::sicherheit::CorsUrteil::Erlaubt => None,
        // added 03.10.2026, TLS: **TUPLE-VARIANTE, nicht struct.** GEMESSEN
        // am Compilerfehler: `CorsUrteil::Verweigert(CorsMangel)` ist ein
        // Tupel, und `grund` ist ein `CorsMangel` mit eigener `grund()`.
        narsil_net::sicherheit::CorsUrteil::Verweigert(mangel) => {
            let g = mangel.grund().to_string();
            vermerke(&g);
            Some(g)
        }
    }
}

/// added 03.10.2026, TLS. **EIN KOPF IST SAFELISTED** — Fetch §4.1. Der Umweg
/// ueber diese Funktion statt ueber die Liste: `sicherheit.rs` entscheidet fuer
/// `content-type` **auch** den Medientyp, und wer die Liste selbst abfragt,
/// laesst genau diese Regel aus.
pub fn cors_kopf_safelisted(name: &str, wert: &str) -> bool {
    cors_kopf_ist_safelisted(name, wert)
}

// ===========================================================================
// DER DOWNLOAD — eine Zeile in `sitzung.rs`
// ===========================================================================

/// added 03.10.2026, TLS. **DER DOWNLOAD-VORBEHALT**, und `None` heisst
/// „laden wie immer".
///
/// **UND `Err` HEISST NICHT „VERBOTEN“, sondern „NUR MIT ZUSTIMMUNG“** — das
/// ist der Unterschied zu Mixed Content und CSP, und er ist der Grund fuer
/// `Option`: bei diesen beiden **darf** nicht geladen werden, bei `attachment`
/// darf geladen werden, aber der Nutzer hat es bestaellt.
///
/// **WARUM HIER UND NICHT IN `dienste::download_file`:** dort steht der
/// `DownloadManager`, und der Pfad hat **keinen** `Response` — die
/// `Content-Disposition` ist eine **Antwort**, die `download_file` gar nicht
/// sieht. Es gibt also genau eine Stelle, an der man sie lesen kann: der
/// Abrufer.
pub fn download_vorbehalt(
    content_disposition: Option<&str>,
    medientyp: &str,
    vorschlag: &str,
) -> Option<String> {
    let u = download_pruefen(content_disposition, medientyp, vorschlag);
    if u.anhang_erzwingen {
        // added 03.10.2026, TLS: **DIE QUELLE DER ZUSTIMMUNG IST NAMENSCHAFT:**
        // `Content-Disposition: attachment` (RFC 6266 4.3). `narsil-net` wertet
        // den Kopf **nirgends** aus (GEMESSEN: die einzigen Treffer fuer den
        // Namen sind die HPACK-/QPACK-Tabellen, das ist die Kompression), also
        // ist diese Datei die **erste** Stelle, an der er gelesen wird.
        return Some(std::format!(
            "Download: {medientyp} wird als Anhang behandelt (Content-Disposition: attachment)"
        ));
    }
    None
}
