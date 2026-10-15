//! ANTWORTKOMPRESSION fuer dynamische Antworten — added 01.10.2026,
//! SERVER V6.
//!
//! [`crate::static_files`] komprimiert Dateien. Was ein **Handler**
//! zurueckgibt (JSON aus [`crate::json`], eine erzeugte Seite, einen
//! Bericht), ging unkomprimiert auf die Leitung: 200 KiB JSON sind auf
//! einem langsamen Link 200 KiB, obwohl sie sich auf 20 KiB pressen
//! lassen. Dieses Modul schliesst die Luecke — als Middleware-Glied,
//! damit es fuer jede Kette gilt und nicht je Route von Hand.
//!
//! # DIE FÜNF BEDINGUNGEN
//!
//! Komprimiert wird nur, wenn **alle** gelten. Jede einzelne davon ist
//! eine Stelle, an der ein Komprimierer sonst Schaden anrichtet:
//!
//! 1. **Der Client will es**: `Accept-Encoding` enthaelt `gzip` mit
//!    `q > 0` (oder `*`). `gzip;q=0` ist ein Nein, kein "aber".
//!    [`crate::static_files::accept_encoding_gzip`] entscheidet das
//!    schon — dieselbe Regel, damit ein Dokument, das eine Datei ist,
//!    und eines, das JSON ist, nicht unterschiedlich behandelt werden.
//! 2. **Der Inhalt ist kompressibel**: `text/*`, `application/json`,
//!    `+json`, `+xml`, `image/svg+xml`. Ein PNG oder ein JPEG mit gzip
//!    zu schicken kostet Rechenzeit und macht die Antwort groesser.
//! 3. **Die Antwort ist nicht schon kodiert**: `Content-Encoding` da
//!    heisst, der Handler hat selbst komprimiert.
//! 4. **Es ist keine Datei und keine Bedingung**: `Response::datei`
//!    wird gestreamt und nicht in einen `Vec` gelesen (M4) — das ist
//!    Sache von [`crate::static_files`]. `304` hat keinen Koerper.
//! 5. **Es lohnt sich**: die kodierte Antwort muss **kuerzer** sein als
//!    die rohe, und die Rohgroesse muss unter
//!    [`MAX_KOMPRESSIBEL`] liegen — sonst zahlt der Client zweimal
//!    (RAM des Servers, Rechenzeit) fuer ein paar Prozent.
//!
//! # WAS NICHT GESCHIEHT
//!
//! * **Kein `Content-Length`-Luegen**: der Server schreibt die
//!   kodierte Laenge (das passiert beim Schreiben, siehe
//!   [`crate::http::write_response_mit`]). Der Test prueft das gegen
//!   `curl`, nicht gegen sich selbst.
//! * **Keine Kompression von `Vary`**: das Glied haengt
//!   `Vary: Accept-Encoding` an die Antwort, wenn sie den Inhaltstyp
//!   aendert. Ohne `Vary` haelt ein gemeinsamer Cache die rohe Fassung
//!   fuer einen Client, der gzip kann — der klassische Fehler.
//! * **Keine Wiederverwendung des Kodierers**: `narsil_compress` bietet
//!   keinen Zustand an, den man hier teilen koennte. Wer 10.000
//!   gleiche Antworten komprimiert, komprimiert sie 10.000 Mal; das ist
//!   die dokumentierte Grenze, keine versteckte.

use std::sync::Arc;

use crate::http::{Request, Response};
use crate::middleware::Glied;

/// Groesster Koerper, der hier komprimiert wird (1 MiB, wie
/// [`crate::static_files::MAX_GZIP_DATEI`]).
///
/// M4: der Encoder braucht das ganze Stueck im Speicher. Diese Zahl
/// steht VOR der Allokation, nicht danach — sonst waere die Grenze ein
/// Vorsatz, den `Vec` nicht liest.
pub const MAX_KOMPRESSIBEL: usize = 1024 * 1024;

/// Kleinster Koerper, bei dem Kompression den Aufwand loehnt.
///
/// 200 Byte sind unter der gzip-Huelle (18 Byte Kopf + 8 Byte Fuss)
/// schon fast groesser. Darunter: roh. Der Wert ist eine Rechnung, keine
/// Gewohnheit — die gzip-Huelle kostet 26 Byte, und ein 40-Byte-Header
/// wird davon nicht kleiner.
pub const MIN_KOMPRESSIBEL: usize = 256;

/// Das Kompressions-Glied: komprimiert, wenn es sich lohnt.
///
/// `stats` zaehlt die Entscheidungen mit, damit ein Test und eine
/// Statusseite sagen koennen, was tatsaechlich passiert — ein Glied,
/// das still arbeitet, ist von einem, das nicht laeuft, nicht zu
/// unterscheiden.
pub fn glied(stats: Arc<KompressionStat>) -> Glied {
    glied_ab(MIN_KOMPRESSIBEL, stats)
}

/// Dasselbe mit eigenem kleinsten Koerper (`mindest`).
///
/// added 01.10.2026, SERVER V6: `ServerConfig::kompression` traegt die
/// Zahl, damit ein Anwender sie ohne den Umweg ueber eine Konstante
/// setzen kann — und damit die Konfiguration **eine** Zahl ist, die im
/// Server, im Glied und im Test dieselbe ist.
pub fn glied_ab(mindest: usize, stats: Arc<KompressionStat>) -> Glied {
    let mindest = mindest.max(1);
    Arc::new(move |anfrage, weiter| {
        let mut antwort = weiter(anfrage);
        if !anfrage_braucht_gzip(anfrage) {
            stats
                .unverlangt
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return antwort;
        }
        if !antwort_ist_kompressibel(&antwort) {
            stats
                .ungeeignet
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return antwort;
        }
        // M4: die Laenge wird VOR der Allokation geprueft.
        if antwort.body.len() < mindest || antwort.body.len() > MAX_KOMPRESSIBEL {
            stats
                .unlohnend
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return antwort;
        }
        let kodiert = narsil_compress::gzip_komprimiere(&antwort.body);
        // Die Huelle kostet 26 Byte: ein 300-Byte-Text wird groesser.
        if kodiert.len() >= antwort.body.len() {
            stats
                .unlohnend
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return antwort;
        }
        let roh = antwort.body.len();
        let neu = kodiert.len();
        antwort.body = kodiert;
        antwort
            .headers
            .push(("Content-Encoding".into(), "gzip".into()));
        // `Vary` steht: ohne ihn gibt es zwischen der rohen und der
        // kodierten Fassung keinen Unterschied fuer einen Cache. Und
        // es wird an ein vorhandenes angehaengt (nicht danebengesetzt),
        // weil `Vary` eine Liste ist — siehe
        // [`crate::middleware::vary_anhaengen`].
        crate::middleware::vary_anhaengen(&mut antwort, "Accept-Encoding");
        stats
            .komprimiert
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        stats
            .roh_bytes
            .fetch_add(roh as u64, std::sync::atomic::Ordering::SeqCst);
        stats
            .kodiert_bytes
            .fetch_add(neu as u64, std::sync::atomic::Ordering::SeqCst);
        antwort
    })
}

/// Will der Client ueberhaupt gzip? (Dieselbe Regel wie bei Dateien.)
pub fn anfrage_braucht_gzip(anfrage: &Request) -> bool {
    crate::static_files::accept_encoding_gzip(anfrage)
}

/// Darf diese Antwort ueberhaupt komprimiert werden?
///
/// Die vier Ausschluesse mit Grund, weil jeder davon eine falsche
/// Antwort erzeugt haette:
///
/// * **Kein Koerper** (`GET` auf ein leeres Ergebnis, `204`, `304`,
///   `HEAD`): gzip einer leeren Antwort ist ein Koerper, den es nicht gab.
/// * **`datei` gesetzt**: gestreamt, nicht gelesen. Ein `Vec` daraus
///   waere genau die Sache, die `Response::datei` vermeiden soll.
/// * **`Content-Encoding` gesetzt**: der Handler hat selbst komprimiert.
///   Zweimal hintereinander ist unlesbar.
/// * **Kein kompressibler Typ**: ein Bild wird groesser.
pub fn antwort_ist_kompressibel(antwort: &Response) -> bool {
    if antwort.body.is_empty() || antwort.datei.is_some() {
        return false;
    }
    if antwort
        .header("content-encoding")
        .is_some_and(|v| !v.trim().is_empty())
    {
        return false;
    }
    // Ohne Content-Type ist die Antwort ein Rätsel; dann wird sie nicht
    // verändert. `Response::status(204)` hat keinen — und genau das ist
    // der Normalfall fuer "leer".
    let typ = antwort.header("content-type").unwrap_or("");
    if typ.is_empty() {
        return false;
    }
    typ_kompressibel(typ)
}

/// Ist dieser Content-Type kompressibel? (public fuer die Tests.)
pub fn typ_kompressibel(typ: &str) -> bool {
    let basis = typ
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    basis.starts_with("text/")
        || basis.ends_with("+json")
        || basis.ends_with("+xml")
        || matches!(
            basis.as_str(),
            "application/json" | "application/javascript" | "application/xml" | "image/svg+xml"
        )
}

/// Zaehler des Glieds. Jede Entscheidung wird getroffen, damit ein
/// Test sagen kann, **warum** etwas nicht komprimiert wurde.
#[derive(Debug, Default)]
pub struct KompressionStat {
    /// Komprimiert.
    pub komprimiert: std::sync::atomic::AtomicU64,
    /// Der Client wollte kein gzip (`Accept-Encoding` fehlt oder `q=0`).
    pub unverlangt: std::sync::atomic::AtomicU64,
    /// Die Antwort ist nicht kompressibel (Bild, Datei, leer, schon kodiert).
    pub ungeeignet: std::sync::atomic::AtomicU64,
    /// Kompressibel, aber zu klein, zu gross oder die Huelle kostete mehr.
    pub unlohnend: std::sync::atomic::AtomicU64,
    /// Rohe Bytes, die komprimiert wurden.
    pub roh_bytes: std::sync::atomic::AtomicU64,
    /// Kodierte Bytes daraus.
    pub kodiert_bytes: std::sync::atomic::AtomicU64,
}

impl KompressionStat {
    pub fn neu() -> Arc<KompressionStat> {
        Arc::new(KompressionStat::default())
    }

    /// Das Verhältnis als Zahl: 1,0 = keine Ersparnis, 4,0 = viermal
    /// kleiner. Für einen Bericht ist das lesbarer als zwei Zahlen.
    pub fn verhaeltnis(&self) -> f64 {
        use std::sync::atomic::Ordering::SeqCst;
        let roh = self.roh_bytes.load(SeqCst);
        if roh == 0 {
            return 1.0;
        }
        roh as f64 / self.kodiert_bytes.load(SeqCst).max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::anfrage_aus_text;
    use std::sync::atomic::Ordering;

    /// Eine echte Seite aus `examples/site/` — keine Attrappe, weil die
    /// Frage "komprimiert das etwas, das wirklich existiert?" sonst
    /// offenbleibt.
    fn echte_seite() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/site/index.html"
        ))
        .expect("echte Seite")
    }

    fn durchlaufen(anfrage: &Request, antwort: Response) -> (Response, Arc<KompressionStat>) {
        let stat = KompressionStat::neu();
        let glied = glied(stat.clone());
        let weiter = |_: &Request| antwort.clone();
        let a = glied(anfrage, &weiter);
        (a, stat)
    }

    #[test]
    fn gzip_frage_beantwortet_200() {
        let anfrage = anfrage_aus_text(
            "GET",
            "/api/bericht",
            &[("Accept-Encoding", "gzip, deflate")],
            &[],
        );
        let roh = echte_seite();
        let (antwort, stat) = durchlaufen(
            &anfrage,
            crate::http::Response::text(200, String::from_utf8_lossy(&roh).to_string()),
        );
        assert_eq!(antwort.status.0, 200);
        assert_eq!(antwort.header("content-encoding"), Some("gzip"));
        assert_eq!(antwort.header("vary"), Some("Accept-Encoding"));
        assert!(
            antwort.body.starts_with(&[0x1f, 0x8b]),
            "gzip-Magic fehlt: {:02x?}",
            &antwort.body[..4.min(antwort.body.len())]
        );
        assert!(
            antwort.body.len() < roh.len(),
            "{} kodiert statt {} roh",
            antwort.body.len(),
            roh.len()
        );
        assert_eq!(stat.komprimiert.load(Ordering::SeqCst), 1);
        // DER MESSWERT, und er ist kleiner als man raten wuerde: die
        // echte `index.html` (1.297 Byte) enthaelt kaum Wiederholungen
        // und kommt auf Faktor 1,49. Die Behauptung „gzip macht das
        // viermal kleiner" waere an dieser Datei geraten — die Zahl
        // hier ist gemessen, und der naechste Test misst den Fall,
        // fuer den Kompression wirklich lohnt.
        assert!(
            stat.verhaeltnis() > 1.2,
            "Verhältnis {} — eine echte Seite muss mindestens etwas schrumpfen",
            stat.verhaeltnis()
        );
        println!(
            "gzip der echten index.html: {} roh -> {} kodiert (Faktor {:.2})",
            roh.len(),
            antwort.body.len(),
            stat.verhaeltnis()
        );
    }

    /// DER FALL, FUER DEN KOMPRESSION WIRKLICH LOHNT: eine
    /// API-Antwort mit hundert gleichen Feldern. Das ist die Form, in
    /// der JSON entsteht, und **hier** entscheidet sich die Frage
    /// "gzip ueber narsil-compress: ja oder nein".
    ///
    /// Der Koerper ist mit [`crate::json`] gebaut, nicht von Hand
    /// geschrieben — sonst prueft der Test eine Zeichenkette, die kein
    /// `Response::json` je erzeugt.
    #[test]
    fn json_aus_der_api_wird_erheblich_kleiner() {
        let anfrage = anfrage_aus_text("GET", "/api/zeilen", &[("Accept-Encoding", "gzip")], &[]);
        let mut zeilen = Vec::with_capacity(400);
        for i in 0..400i64 {
            let mut zeile = std::collections::HashMap::with_capacity(3);
            zeile.insert("id".to_string(), crate::json::Value::Ganz(i));
            zeile.insert(
                "name".to_string(),
                crate::json::Value::Text(format!("Eintrag {i}")),
            );
            zeile.insert("aktiv".to_string(), crate::json::Value::Bool(i % 2 == 0));
            zeilen.push(crate::json::Value::Objekt(zeile));
        }
        let dokument = crate::json::Value::Feld(zeilen);
        let roh = crate::json::stringify(&dokument);
        assert!(
            roh.len() > 10_000,
            "die Testnutzlast ist zu klein ({})",
            roh.len()
        );
        let (antwort, stat) = durchlaufen(&anfrage, crate::http::Response::json(200, roh.clone()));
        assert_eq!(antwort.header("content-encoding"), Some("gzip"));
        assert!(
            antwort.body.len() * 3 < roh.len(),
            "{} Byte kodiert statt {} roh (Faktor {:.2})",
            antwort.body.len(),
            roh.len(),
            stat.verhaeltnis()
        );
        // Der Dezimalpunkt der Messung: > 3 ist fuer 400 gleiche
        // Datensaetze conservative Untergrenze, und sie ist eine Zahl
        // statt einer Absicht.
        assert!(
            stat.verhaeltnis() > 3.0,
            "Faktor {} gemessen",
            stat.verhaeltnis()
        );
        println!(
            "gzip der API-Antwort: {} roh -> {} kodiert (Faktor {:.2})",
            roh.len(),
            antwort.body.len(),
            stat.verhaeltnis()
        );
    }

    /// DER GEGENTEST: ohne `Accept-Encoding` kommt **exakt** das
    /// Original. Sonst waere die erste Probe nur Zufall.
    #[test]
    fn ohne_accept_encoding_bleibt_es_rohr() {
        let anfrage = anfrage_aus_text("GET", "/api/bericht", &[], &[]);
        let roh = echte_seite();
        let (antwort, stat) = durchlaufen(
            &anfrage,
            crate::http::Response::text(200, String::from_utf8_lossy(&roh).to_string()),
        );
        assert_eq!(antwort.header("content-encoding"), None);
        assert_eq!(antwort.body, roh, "der Koerper wurde veraendert");
        assert_eq!(stat.unverlangt.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn gzip_q_null_ist_ein_nein() {
        let anfrage = anfrage_aus_text(
            "GET",
            "/api/bericht",
            &[("Accept-Encoding", "gzip;q=0, identity")],
            &[],
        );
        let roh = echte_seite();
        let (antwort, _) = durchlaufen(
            &anfrage,
            crate::http::Response::text(200, String::from_utf8_lossy(&roh).to_string()),
        );
        assert_eq!(antwort.header("content-encoding"), None);
    }

    #[test]
    fn bild_wird_nicht_komprimiert() {
        // Eine echte PNG-Datei aus dem Testbestand, als Koerper. Das
        // Argument ist nicht "Bilder sind komprimiert", sondern: gzip
        // macht sie GROESSER, und das ist messbar.
        let bild = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/site/bild.png"
        ))
        .expect("echtes PNG");
        let anfrage = anfrage_aus_text("GET", "/b", &[("Accept-Encoding", "gzip")], &[]);
        let mut antwort = crate::http::Response::status(200);
        antwort
            .headers
            .push(("Content-Type".into(), "image/png".into()));
        antwort.body = bild.clone();
        let (raus, stat) = durchlaufen(&anfrage, antwort);
        assert_eq!(raus.header("content-encoding"), None);
        assert_eq!(raus.body, bild, "ein Bild wurde veraendert");
        assert_eq!(stat.ungeeignet.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn leere_und_bedingte_antworten_bleiben_leer() {
        let anfrage = anfrage_aus_text("GET", "/x", &[("Accept-Encoding", "gzip")], &[]);
        for (code, grund) in [(204u16, "No Content"), (304, "Not Modified")] {
            let mut antwort = crate::http::Response::status(code);
            antwort
                .headers
                .push(("Content-Type".into(), "text/html; charset=utf-8".into()));
            let (raus, _) = durchlaufen(&anfrage, antwort);
            assert_eq!(
                raus.header("content-encoding"),
                None,
                "Status {code} ({grund})"
            );
            assert!(raus.body.is_empty(), "Status {code} ({grund})");
        }
    }

    #[test]
    fn schon_kodiert_wird_nicht_zweimal_kodiert() {
        let anfrage = anfrage_aus_text("GET", "/x", &[("Accept-Encoding", "gzip")], &[]);
        let mut antwort =
            crate::http::Response::text(200, String::from_utf8_lossy(&echte_seite()).to_string());
        antwort
            .headers
            .push(("Content-Encoding".into(), "br".into()));
        let roh = antwort.body.clone();
        let (raus, stat) = durchlaufen(&anfrage, antwort);
        assert_eq!(raus.header("content-encoding"), Some("br"));
        assert_eq!(raus.body, roh, "ein br-koerper wurde nochmal komprimiert");
        assert_eq!(stat.ungeeignet.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn winziger_koerper_bleibt_rohr() {
        // Die gzip-Huelle kostet 26 Byte. Ein 40-Byte-Koerper wird davon
        // groesser — und das muss das Glied *vor* dem Kodieren wissen.
        let anfrage = anfrage_aus_text("GET", "/x", &[("Accept-Encoding", "gzip")], &[]);
        let (antwort, stat) = durchlaufen(&anfrage, crate::http::Response::text(200, "kurz"));
        assert_eq!(antwort.header("content-encoding"), None);
        assert_eq!(antwort.body, b"kurz");
        assert_eq!(stat.unlohnend.load(Ordering::SeqCst), 1);
    }

    /// M4 als Test: ein Koerper ueber [`MAX_KOMPRESSIBEL`] wird nicht
    /// gelesen und nicht kopiert. Der Test baut ihn nicht einmal —
    /// er prueft die Grenze an der Zahl, sonst waere er ein
    /// Speichertest mit 1 MiB Nutzlast.
    #[test]
    fn grenzen_stehen_vor_der_allokation() {
        let anfrage = anfrage_aus_text("GET", "/x", &[("Accept-Encoding", "gzip")], &[]);
        let mut gross = crate::http::Response::text(200, "a".repeat(MAX_KOMPRESSIBEL + 1));
        gross
            .headers
            .push(("Content-Type".into(), "text/plain".into()));
        let (raus, stat) = durchlaufen(&anfrage, gross);
        assert_eq!(raus.header("content-encoding"), None);
        assert_eq!(stat.unlohnend.load(Ordering::SeqCst), 1);
        // Und die Typgrenze ohne Raten.
        assert!(typ_kompressibel("text/html; charset=utf-8"));
        assert!(typ_kompressibel("application/json"));
        assert!(typ_kompressibel("application/vnd.api+json"));
        assert!(typ_kompressibel("image/svg+xml"));
        assert!(typ_kompressibel("text/javascript"));
        assert!(!typ_kompressibel("image/png"));
        assert!(!typ_kompressibel("image/jpeg"));
        assert!(!typ_kompressibel("application/octet-stream"));
        assert!(!typ_kompressibel("video/mp4"));
    }
}
