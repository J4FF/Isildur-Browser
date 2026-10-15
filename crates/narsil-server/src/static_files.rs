//! Statische Dateien: Narsil, HTML, Klartext — mit Traversal-Schutz,
//! ETag/304, Last-Modified und Range/206.
//!
//! Der Server liefert Dateien aus einem Wurzelverzeichnis. Der Angreifer
//! liefert Pfade. Dazwischen steht genau eine Funktion ([`sicherer_pfad`]),
//! und sie ist der Grund, warum es diese Crate gibt statt eines
//! `format!("{wurzel}/{pfad}")` im Handler.
//!
//! Regeln:
//!
//! * `..` entkommt nie der Wurzel (weder als Segment noch kodiert).
//! * Segmentezahl begrenzt ([`MAX_PFADE_SEGMENTE`]) — 10.000 `"../"`
//!   kosten eine Schleife mit fester Obergrenze, keine Minute (M3).
//! * Absolute Pfade im Request werden relativ zur Wurzel gelesen.
//! * Verzeichnisse liefern `index.narsil`, dann `index.html`, sonst `404`
//!   (kein Listing — ein Listing ist ein Verzeichnis-Dump mit URL).
//! * Content-Type nach Endung, nicht nach Inhalt (schnell, vorhersagbar).
//! * Fehlende Datei ist `404`, verbotene ist `403` — nicht beides `404`.
//! * Die Datei wird NIE in einen Vec gelesen: [`Response::datei`] schreibt
//!   in 64-KiB-Bloecken (M4). Ein 5-GB-Video kostet 64 KiB Speicher.
//!
//! **Komprimierung (S7):** `Accept-Encoding` wird ausgewertet
//! (`gzip;q=0` ist eine Ablehnung, `*` eine Einladung), die Datei nur
//! dann gzip-komprimiert, wenn sie echt komprimierbar UND unter
//! [`MAX_GZIP_DATEI`] ist und die Fassung wirklich kuerzer wird, und
//! der `ETag` bekommt eine eigene Variante je Fassung — sonst wuerde
//! ein Cache die rohe und die komprimierte Antwort fuer dieselbe
//! halten. `Range` bleibt roh, `Vary: Accept-Encoding` steht auf
//! JEDER Antwort.
//!
//! Der Encoder kommt aus `narsil-compress`. Hier steht bewusst KEIN
//! zweiter: zwei Implementationen desselben Formates waeren zwei
//! Stellen, an denen dieselbe Stelle "fast korrekt" sein kann.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::http::{DateiLesen, Request, Response};

/// Maximale Anzahl von Pfadsegmenten (Regel M3).
///
/// `sicherer_pfad` laeuft einmal je Segment. Ohne diese Grenze waere
/// die Laenge der Schleife vom Angreifer bestimmt — und ein Pfad aus
/// 10.000 `"../"` ist genau der Test, der das zeigen muss. 256 Segmente
/// sind mehr, als jede tiefe Dokumentation braucht.
pub const MAX_PFADE_SEGMENTE: usize = 256;

/// Obergrenze EINER Datei, die gzip-komprimiert wird (1 MiB).
///
/// `gzip()` braucht das ganze Stueck in den Speicher (M4), also wird
/// die Laenge VOR dem Lesen geprueft und DANACH gegen die tatsaechlich
/// gelesene Laenge geprueft — ein `INODE`, das loggt, waere sonst die
/// Luecke, durch die eine 5-GB-Datei in einen 1-MiB-Block passt.
pub const MAX_GZIP_DATEI: u64 = 1024 * 1024;

/// Einen ETag um eine Variante erweitern: `"abc"` wird `"abc-gzip"`.
///
/// Das Suffix steht VOR dem schliessenden Anfuehrungszeichen (RFC 9110,
/// Abschnitt 8.8.3), sonst gehoert es nicht zum Tag, sondern zur
/// kuemmerlichen Restklammer — und der Cache sieht zwei Tags fuer
/// dieselbe Sache.
pub fn etag_variante(etag: &str, suffix: &str) -> String {
    match etag.rfind('"') {
        Some(p) => format!("{}-{}{}", &etag[..p], suffix, &etag[p..]),
        None => format!("{etag}-{suffix}"),
    }
}

/// Was sich zu lohnen hat: Text und alles, das aus Text gebaut ist.
///
/// Ein JPEG mit gzip zu schicken kostet Rechenzeit und bringt nichts
/// (bzw. macht es groesser) — deshalb die Liste und kein
/// `alles außer Bildern`, weil Bildformate dazukommen, ohne dass
/// jemand sie eintraegt.
fn kompressibel(typ: &str) -> bool {
    let basis = typ.split(';').next().unwrap_or("").trim();
    basis.starts_with("text/")
        || basis.ends_with("+json")
        || basis.ends_with("+xml")
        || matches!(
            basis,
            "application/json" | "application/javascript" | "application/xml" | "image/svg+xml"
        )
}

/// Will der Client gzip? `gzip;q=0` ist eine Ablehnung, `*` erlaubt
/// alles, was nicht ausdruecklich abgelehnt wird.
///
/// Ohne `Accept-Encoding` gibt es KEINE Komprimierung: das ist kein
/// Fehlerfall, sondern der normale Weg eines Clients, der nichts
/// versteht — und ihm etwas unterzuschieben, das er nicht entpacken
/// kann, waere eine Stille, die als Datenfehler aussieht.
pub fn accept_encoding_gzip(anfrage: &Request) -> bool {
    let Some(wert) = anfrage.header("accept-encoding") else {
        return false;
    };
    let mut stern = false;
    for teil in wert.split(',') {
        let mut teile = teil.split(';');
        let name = teile.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut q = 1.0f64;
        for p in teile {
            let p = p.trim();
            if let Some(zahl) = p.strip_prefix("q=") {
                q = zahl.trim().parse().unwrap_or(0.0);
            }
        }
        // Ausdruecklich entscheidet AUSDRUECKLICH — auch wenn vorher
        // ein `*` stand. Ein `q=0` ist ein Nein und kein "aber".
        if name == "gzip" || name == "x-gzip" {
            return q > 0.0;
        }
        if name == "*" {
            stern = q > 0.0;
        }
    }
    stern
}

/// Lohnt sich gzip fuer DIESE Anfrage? Vor den Bedingungen geprueft,
/// weil der ETag je Variante anders ist und der schon fuer
/// `If-None-Match` gebraucht wird.
fn gzip_machbar(anfrage: &Request, typ: &str, laenge: u64) -> bool {
    laenge > 0
        && laenge <= MAX_GZIP_DATEI
        // Range auf der kodierten Fassung waere ein zweites
        // Koordinatensystem. Die Anfrage bleibt roh.
        && anfrage.header("range").is_none()
        && kompressibel(typ)
        && accept_encoding_gzip(anfrage)
}

/// Content-Type nach Dateiendung. Narsil zuerst — das ist die eigene
/// Sprache, und sie hat einen eigenen Typ.
pub fn content_type_fuer(pfad: &Path) -> &'static str {
    match pfad
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("narsil") => "text/narsil; charset=utf-8",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("ppm") => "image/x-portable-pixmap",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Baut aus Wurzel + Request-Pfad einen sicheren Dateipfad.
///
/// Gibt `None`, wenn der Pfad entkommen will (`..` ueber die Wurzel,
/// absolute Pfade ausserhalb, leere Segmente mit Backslash-Tricks) oder
/// mehr als [`MAX_PFADE_SEGMENTE`] Teile hat.
/// Kodierte `..` (`%2e%2e`) werden VORHER dekodiert — sonst waere der
/// Schutz ein Kommentar statt einer Pruefung.
pub fn sicherer_pfad(wurzel: &Path, anfrage_pfad: &str) -> Option<PathBuf> {
    // Query ist schon getrennt (Router gibt nur Pfad), aber ein `?`
    // im Pfad waere ein zweiter Query-Versuch.
    let roh = anfrage_pfad.split('?').next().unwrap_or("");
    // Prozentdekodierung VOR der Pruefung (sonst ist `%2e%2e` unsichtbar).
    let dekodiert = crate::router::prozent_dekodieren(roh);
    let mut ziel = PathBuf::from(wurzel);
    // Fuehrendes `/` entfernen (sonst ersetzt `push` die Wurzel).
    let relativ = dekodiert.strip_prefix('/').unwrap_or(&dekodiert);
    for (i, teil) in relativ.split('/').enumerate() {
        // M3: Obergrenze der Schleife, bevor sie ueber die Eingabe laeuft.
        if i >= MAX_PFADE_SEGMENTE {
            return None;
        }
        // Backslash ist auf Unix ein Zeichen, auf Windows ein Trenner.
        // Wer ihn schickt, meint einen Trenner — und bekommt keinen.
        if teil.contains('\\') {
            return None;
        }
        match teil {
            "" | "." => {}
            ".." => {
                // Ein `..` darf die Wurzel nie verlassen. `pop` gibt
                // `false`, wenn nichts mehr da ist — das ist die Grenze.
                // Aber: `pop` auf der WURZEL wuerde die Wurzel selbst
                // verlassen. Also pruefen, ob wir noch innerhalb sind.
                if !ziel.pop() {
                    return None;
                }
                if !ziel.starts_with(wurzel) {
                    return None;
                }
            }
            sonst => {
                ziel.push(sonst);
            }
        }
    }
    // Endgueltig: der Pfad muss in der Wurzel liegen. Wer hier vorbei
    // kommt, hat die Schleife ueberlistet — und bekommt trotzdem keinen
    // Zugriff.
    if !ziel.starts_with(wurzel) {
        return None;
    }
    // Saeubern gegen `.`/`..`-Reste, ohne das Dateisystem zu fragen
    // (kein `canonicalize` — das folgt Symlinks und oeffnet TOCTOU).
    let mut sauber = PathBuf::new();
    for comp in ziel.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                return None;
            }
            c => sauber.push(c.as_os_str()),
        }
    }
    Some(sauber)
}

/// Liefert eine Datei als Antwort — ohne Anfrage, also ohne
/// Bedingungen (200, ganze Datei). Fuer Tests und fuer Aufrufer, die
/// keinen Request haben.
pub fn datei_antwort(wurzel: &Path, anfrage_pfad: &str) -> Response {
    datei_intern(wurzel, anfrage_pfad, None)
}

/// Liefert eine Datei als Antwort MIT Anfrage: ETag/If-None-Match (304),
/// Last-Modified/If-Modified-Since (304), Range (206/416).
///
/// Das ist die Funktion, die ein statischer Route benutzt.
pub fn datei_antwort_fuer(wurzel: &Path, anfrage_pfad: &str, anfrage: &Request) -> Response {
    datei_intern(wurzel, anfrage_pfad, Some(anfrage))
}

fn datei_intern(wurzel: &Path, anfrage_pfad: &str, anfrage: Option<&Request>) -> Response {
    let Some(pfad) = sicherer_pfad(wurzel, anfrage_pfad) else {
        return Response::fehler(403, "Pfad nicht erlaubt");
    };
    if !pfad.exists() {
        return Response::fehler(404, format!("{anfrage_pfad}: nicht gefunden"));
    }
    // Verzeichnis: Index suchen.
    let Some(kanonisch) = kanonisch_in_wurzel(wurzel, &pfad) else {
        return Response::fehler(403, "Datei ausserhalb der Wurzel");
    };
    if kanonisch.is_dir() {
        for index in ["index.narsil", "index.html"] {
            let kandidat = kanonisch.join(index);
            if kandidat.is_file() {
                return datei_sicher(wurzel, &kandidat, anfrage);
            }
        }
        return Response::fehler(404, format!("{anfrage_pfad}: kein Index"));
    }
    if !kanonisch.is_file() {
        return Response::fehler(404, format!("{anfrage_pfad}: nicht gefunden"));
    }
    datei_sicher(wurzel, &kanonisch, anfrage)
}

/// Auch ein lexikalisch sicherer Pfad kann ueber einen Symlink aus der
/// Wurzel zeigen. Beide bestehenden Pfade werden deshalb vor dem Lesen
/// kanonisiert; ein Fehler gibt niemals Zugriff frei.
fn kanonisch_in_wurzel(wurzel: &Path, pfad: &Path) -> Option<PathBuf> {
    match (std::fs::canonicalize(wurzel), std::fs::canonicalize(pfad)) {
        (Ok(w), Ok(p)) if p.starts_with(&w) => Some(p),
        _ => None,
    }
}

fn datei_sicher(wurzel: &Path, pfad: &Path, anfrage: Option<&Request>) -> Response {
    let Some(kanonisch) = kanonisch_in_wurzel(wurzel, pfad) else {
        return Response::fehler(403, "Datei ausserhalb der Wurzel");
    };
    let datei = match std::fs::File::open(&kanonisch) {
        Ok(d) => d,
        Err(e) => return Response::fehler(403, format!("nicht lesbar: {e}")),
    };
    // Auf Linux kontrollieren wir den TATSAECHLICH geoeffneten Handle.
    // Ein Symlink-Tausch zwischen canonicalize und open kann so nicht
    // heimlich eine Datei ausserhalb der Wurzel einschleusen.
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let fd_pfad = format!("/proc/self/fd/{}", datei.as_raw_fd());
        let wirklich = std::fs::read_link(fd_pfad).ok();
        let erlaubte_wurzel = std::fs::canonicalize(wurzel).ok();
        if !matches!((wirklich, erlaubte_wurzel), (Some(p), Some(w)) if p.starts_with(&w)) {
            return Response::fehler(403, "Geoeffnete Datei ausserhalb der Wurzel");
        }
    }
    datei_mit_bedingungen(&kanonisch, Arc::new(Mutex::new(datei)), anfrage)
}

/// Die eigentliche Antwort: Bedingungen pruefen, dann STREAMEN.
fn datei_mit_bedingungen(
    pfad: &Path,
    quelle: Arc<Mutex<std::fs::File>>,
    anfrage: Option<&Request>,
) -> Response {
    let meta = match quelle.lock().unwrap_or_else(|p| p.into_inner()).metadata() {
        Ok(m) => m,
        Err(e) => return Response::fehler(403, format!("nicht lesbar: {e}")),
    };
    // Nicht die Datei lesen, sondern nur ihre Daten: Laenge und Zeit
    // kommen aus dem INODE, und die Datei selbst wird beim Schreiben
    // in Bloecken geholt (M4).
    let laenge = meta.len();
    let mtime = meta.modified().ok();
    let roh_etag = etag_fuer(laenge, mtime);
    let typ = content_type_fuer(pfad);

    let Some(anfrage) = anfrage else {
        return antwort_mit_datei(pfad, &quelle, 200, 0, laenge, typ, &roh_etag, mtime);
    };

    // --- Die Variante steht VOR den Bedingungen fest: der ETag ist je
    // Variante ein anderer, und `If-None-Match` braucht den schon. ---
    let gzip = gzip_machbar(anfrage, typ, laenge);
    let etag = if gzip {
        etag_variante(&roh_etag, "gzip")
    } else {
        roh_etag.clone()
    };

    // --- Bedingte Anfrage: ETag hat Vorrang vor der Datumsgrenze ---
    if let Some(inm) = anfrage.header("if-none-match") {
        if etag_trifft(inm, &etag) {
            // Die Laenge der AUSGEWAEHLTEN Darstellung ist die der
            // kodierten — und die kennen wir ohne Komprimieren nicht.
            // Lieber weglassen als die rohe Zahl hinzuschreiben.
            let laenge_anzeige = if gzip { None } else { Some(laenge) };
            return antwort_304(&etag, mtime, typ, laenge_anzeige);
        }
        // Kopf war da und traf nicht: dann NICHT mehr auf If-Modified-
        //-Since schauen (RFC 9110, Abschnitt 13.1.3).
        return range_entscheiden(pfad, &quelle, anfrage, typ, &roh_etag, mtime, laenge, gzip);
    }
    if let Some(ims) = anfrage.header("if-modified-since") {
        if let Some(grenze) = http_datum_sekunden(ims) {
            if let Some(m) = mtime.and_then(epoch_sekunden) {
                // Vergleich in SEKUNDEN: HTTP-Daten haben keine
                // Nanosekunden, sonst wuerde jede Datei nie treffen.
                if m <= grenze + 1 {
                    let laenge_anzeige = if gzip { None } else { Some(laenge) };
                    return antwort_304(&etag, mtime, typ, laenge_anzeige);
                }
            }
        }
    }
    range_entscheiden(pfad, &quelle, anfrage, typ, &roh_etag, mtime, laenge, gzip)
}

/// Range-Anfrage auswerten (206/416) oder ganz ausliefern (200).
// added 05.10.2026 (SCHLUSSPHASE, clippy `too_many_arguments`):
// acht Parameter, beide Funktionen sind **Dateizugriff mit Range** —
// Pfad, Quelle, Bereich, Kopf, Frist, Flags. Sie teilen sich die
// Argumente absichtlich, damit ein Range-Request und ein
// Vollabruf denselben Weg nehmen; eine Parameterstruktur wuerde
// hier eine dritte Fassung derselben Liste bedeuten.
#[allow(clippy::too_many_arguments)]
fn range_entscheiden(
    pfad: &Path,
    quelle: &Arc<Mutex<std::fs::File>>,
    anfrage: &Request,
    typ: &str,
    roh_etag: &str,
    mtime: Option<SystemTime>,
    laenge: u64,
    gzip_gewaehlt: bool,
) -> Response {
    // `Vary: Accept-Encoding` steht auf JEDEM Dokument — auf der
    // rohen wie auf der komprimierten Fassung, sonst kann ein Cache
    // zwischen ihnen nicht unterscheiden.
    let Some(range) = anfrage.header("range") else {
        if gzip_gewaehlt {
            let varianten_etag = etag_variante(roh_etag, "gzip");
            if let Some(r) = antwort_gzip(quelle, typ, &varianten_etag, mtime, laenge) {
                return r;
            }
            // Nicht lohnend (kuerzer geworden, nicht lesbar, zu gross
            // gewachsen): roh — und mit dem ROHEN ETag, weil das die
            // Fassung ist, die hier wirklich rausgeht.
        }
        return antwort_mit_datei(pfad, quelle, 200, 0, laenge, typ, roh_etag, mtime);
    };
    let Some((start, ende)) = range_auswerten(range, laenge) else {
        // Unverstaendlicher oder mehrfacher Bereich: RFC 7233 erlaubt,
        // ihn zu ignorieren. Also ganz ausliefern statt einen Fehler
        // zu erfinden.
        return antwort_mit_datei(pfad, quelle, 200, 0, laenge, typ, roh_etag, mtime);
    };
    if start >= laenge && laenge > 0 {
        // 416: der Bereich faengt hinter dem Dateiende an.
        let mut r = Response::status(416);
        r.headers
            .push(("Content-Range".into(), format!("bytes */{laenge}")));
        r.headers
            .push(("Content-Type".into(), "text/plain; charset=utf-8".into()));
        r.body = format!("Bereich {start}- ausserhalb der Datei (laenge {laenge})").into_bytes();
        return r;
    }
    let start = start.min(laenge.saturating_sub(1));
    let ende = ende
        .unwrap_or(laenge.saturating_sub(1))
        .min(laenge.saturating_sub(1));
    if ende < start {
        return antwort_mit_datei(pfad, quelle, 200, 0, laenge, typ, roh_etag, mtime);
    }
    let laenge_teil = ende - start + 1;
    let mut r = antwort_mit_datei(pfad, quelle, 206, start, laenge_teil, typ, roh_etag, mtime);
    r.headers.push((
        "Content-Range".into(),
        format!("bytes {start}-{ende}/{laenge}"),
    ));
    r
}

/// Eine Antwort mit Datei-Koerper (gestreamt, nie gelesen).
// added 05.10.2026 (SCHLUSSPHASE, clippy `too_many_arguments`):
// acht Parameter, beide Funktionen sind **Dateizugriff mit Range** —
// Pfad, Quelle, Bereich, Kopf, Frist, Flags. Sie teilen sich die
// Argumente absichtlich, damit ein Range-Request und ein
// Vollabruf denselben Weg nehmen; eine Parameterstruktur wuerde
// hier eine dritte Fassung derselben Liste bedeuten.
#[allow(clippy::too_many_arguments)]
fn antwort_mit_datei(
    pfad: &Path,
    quelle: &Arc<Mutex<std::fs::File>>,
    code: u16,
    offset: u64,
    laenge: u64,
    typ: &str,
    etag: &str,
    mtime: Option<SystemTime>,
) -> Response {
    let mut r = Response::status(code);
    r.headers.push(("Content-Type".into(), typ.into()));
    r.headers.push(("ETag".into(), etag.into()));
    if let Some(m) = mtime {
        r.headers.push(("Last-Modified".into(), http_datum(m)));
    }
    r.headers.push(("Accept-Ranges".into(), "bytes".into()));
    r.headers.push(("Vary".into(), "Accept-Encoding".into()));
    r.datei = Some(DateiLesen {
        pfad: pfad.to_path_buf(),
        offset,
        laenge,
        quelle: Some(quelle.clone()),
    });
    r
}

/// Die komprimierte Fassung — oder `None`, wenn sie sich nicht lohnt.
///
/// Drei Male `None` sind bewusste Auswege, kein Unfertiges:
///
/// * die Datei ist nicht lesbar oder mittlerweile zu gross,
/// * die Komprimierung kam nicht kuerzer heraus (die gzip-Huelle
///   kostet 18 Bytes, ein schon gepresstes Bild wird dadurch groesser),
/// * die Datei waechst waehrend des Lesens ueber die Grenze.
///
/// In allen drei Faellen liefert der Aufrufer roh weiter — mit dem
/// rohen ETag, der zu der Fassung gehoert, die dann rausgeht.
fn antwort_gzip(
    quelle: &Arc<Mutex<std::fs::File>>,
    typ: &str,
    etag: &str,
    mtime: Option<SystemTime>,
    laenge: u64,
) -> Option<Response> {
    // M4: erst die Laenge deckeln, DANN lesen. Ohne diese Zeile waere
    // die Grenze ein Vorsatz, den `fs::read` nicht liest.
    if laenge == 0 || laenge > MAX_GZIP_DATEI {
        return None;
    }
    // Die Datei kann NACH der Metadatenpruefung wachsen. `take` deckelt
    // auch diesen zweiten Lesezeitpunkt, bevor Speicher belegt wird.
    let mut datei = quelle.lock().unwrap_or_else(|p| p.into_inner());
    datei.seek(SeekFrom::Start(0)).ok()?;
    let mut roh = Vec::new();
    (&mut *datei)
        .take(MAX_GZIP_DATEI + 1)
        .read_to_end(&mut roh)
        .ok()?;
    if roh.is_empty() || roh.len() as u64 > MAX_GZIP_DATEI {
        return None;
    }
    let kodiert = narsil_compress::gzip_komprimiere(&roh);
    if kodiert.len() >= roh.len() {
        return None;
    }

    let mut r = Response::status(200);
    r.headers.push(("Content-Type".into(), typ.into()));
    r.headers.push(("Content-Encoding".into(), "gzip".into()));
    r.headers.push(("ETag".into(), etag.into()));
    if let Some(m) = mtime {
        r.headers.push(("Last-Modified".into(), http_datum(m)));
    }
    // Kein `Accept-Ranges`: die Bereiche bezogen sich immer auf die
    // rohe Datei. Wer Bereich 3..6 will, bekommt sie roh.
    r.headers.push(("Vary".into(), "Accept-Encoding".into()));
    r.body = kodiert;
    Some(r)
}

/// 304 Not Modified: Header, aber kein Koerper.
///
/// `Content-Length` nennt die Groesse der AUSGEWAEHLTEN Darstellung —
/// das darf ein 304 (RFC 9110, Abschnitt 15.4.5), und der eigene
/// Client liest bei 304 ohnehin keinen Koerper.
fn antwort_304(etag: &str, mtime: Option<SystemTime>, typ: &str, laenge: Option<u64>) -> Response {
    let mut r = Response::status(304);
    r.headers.push(("ETag".into(), etag.into()));
    if let Some(m) = mtime {
        r.headers.push(("Last-Modified".into(), http_datum(m)));
    }
    r.headers.push(("Content-Type".into(), typ.into()));
    // `None` = die Laenge der gewaehlten Darstellung ist hier nicht
    // bekannt (kompimiert, ohne zu komprimieren). Lieber gar keine
    // Zahl als eine, die zu der Fassung gehoert, die der Client
    // NICHT bekommen hat.
    if let Some(l) = laenge {
        r.headers.push(("Content-Length".into(), l.to_string()));
    }
    r.headers.push(("Vary".into(), "Accept-Encoding".into()));
    r
}

/// ETag aus Laenge + Aenderungszeit — stark (kein `W/`), weil die
/// Daten vom selben Dateisystem kommen.
pub fn etag_fuer(laenge: u64, mtime: Option<SystemTime>) -> String {
    let nanos = mtime
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("\"{laenge:x}-{nanos:x}\"")
}

/// Passt `If-None-Match` auf diesen ETag? Akzeptiert `*`, Komma-Listen
/// und schwache Angaben (`W/`).
fn etag_trifft(kopf: &str, etag: &str) -> bool {
    kopf.split(',').any(|teil| {
        let t = teil.trim();
        let t = t.strip_prefix("W/").unwrap_or(t);
        t == "*" || t == etag
    })
}

/// `Range: bytes=a-b` / `bytes=a-` / `bytes=-n` auswerten.
///
/// Gibt `None` zurueck, wenn die Angabe unverstaendlich ist oder mehr
/// als EINEN Bereich verlangt (dann wird die ganze Datei ausgeliefert
/// — RFC 7233 erlaubt das). `(start, ende)` mit `ende = None` heisst
/// "bis zum Schluss".
fn range_auswerten(kopf: &str, laenge: u64) -> Option<(u64, Option<u64>)> {
    let rest = kopf.trim().strip_prefix("bytes=")?;
    if rest.contains(',') {
        return None; // Mehrbereichs-Requests: ignorieren, ganz liefern
    }
    let (roh_a, roh_b) = rest.split_once('-')?;
    let a = roh_a.trim();
    let b = roh_b.trim();
    if a.is_empty() {
        // Suffix: die LETZTEN n Bytes.
        let n: u64 = b.parse().ok()?;
        if n == 0 {
            return Some((laenge, Some(laenge.saturating_sub(1))));
        }
        return Some((laenge.saturating_sub(n), None));
    }
    let start: u64 = a.parse().ok()?;
    if b.is_empty() {
        return Some((start, None));
    }
    let ende: u64 = b.parse().ok()?;
    if ende < start {
        return None;
    }
    Some((start, Some(ende)))
}

// ---------------------------------------------------------------------------
// HTTP-Daten (RFC 7231, IMF-fixdate) — ohne Fremdcrate, aus std.
// ---------------------------------------------------------------------------

const MONATE_KURZ: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WOCHENTAGE: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Tage seit 1970-01-01 (Howard Hinnant, `days_from_civil`).
fn tage_seit_epoch(jahr: i64, monat: u32, tag: u32) -> i64 {
    let jahr = if monat <= 2 { jahr - 1 } else { jahr };
    let aera = if jahr >= 0 { jahr } else { jahr - 399 } / 400;
    let jahr_e = jahr - aera * 400;
    let doy = (153 * (monat as i64 + if monat > 2 { -3 } else { 9 }) + 2) / 5 + tag as i64 - 1;
    let doe = jahr_e * 365 + jahr_e / 4 - jahr_e / 100 + doy;
    aera * 146_097 + doe - 719_468
}

/// Der umgekehrte Weg (`civil_from_days`) fuer [`http_datum`].
fn civil(tag: i64) -> (i64, u32, u32) {
    let z = tag + 719_468;
    let aera = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - aera * 146_097;
    let jahr_e = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let jahr = jahr_e + aera * 400;
    let doy = doe - (365 * jahr_e + jahr_e / 4 - jahr_e / 100);
    let mp = (5 * doy + 2) / 153;
    let tag = doy - (153 * mp + 2) / 5 + 1;
    let monat = if mp < 10 { mp + 3 } else { mp - 9 };
    (jahr + i64::from(monat <= 2), monat as u32, tag as u32)
}

/// `SystemTime` -> `Wed, 30 Sep 2026 04:10:00 GMT`.
pub fn http_datum(t: SystemTime) -> String {
    let sek = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let tage = sek.div_euclid(86_400);
    let rest = sek.rem_euclid(86_400);
    let (jahr, monat, tag) = civil(tage);
    let wt = WOCHENTAGE[rem7(tage)];
    format!(
        "{}, {tag:02} {} {jahr} {:02}:{:02}:{:02} GMT",
        wt,
        MONATE_KURZ[(monat - 1) as usize],
        rest / 3600,
        (rest / 60) % 60,
        rest % 60
    )
}

fn rem7(tage: i64) -> usize {
    // 1970-01-01 war ein Donnerstag (Index 4 in der Sonntags-Woche).
    tage.rem_euclid(7).wrapping_add(4).rem_euclid(7) as usize
}

/// HTTP-Datum -> Sekunden seit 1970. Akzeptiert das heutige Format
/// `Wed, 30 Sep 2026 04:10:00 GMT` und die alten Formen mit Ziffern-
/// Monat. Zeitzone ohne Offset wird als GMT gelesen (RFC-Pflicht).
pub fn http_datum_sekunden(s: &str) -> Option<i64> {
    let teile: Vec<&str> = s.split_whitespace().collect();
    // Monat als 3-Buchstaben suchen — daneben stehen Tag und Jahr.
    let mi = teile
        .iter()
        .position(|t| MONATE_KURZ.iter().any(|m| m.eq_ignore_ascii_case(t)))?;
    let tag: i64 = teile.get(mi.checked_sub(1)?)?.parse().ok()?;
    let jahr: i64 = teile.get(mi + 1)?.parse().ok()?;
    if !(1..=31).contains(&tag) || !(DATUM_JAHR_MIN..=DATUM_JAHR_MAX).contains(&jahr) {
        return None;
    }
    let mut h = 0i64;
    let mut min = 0i64;
    let mut sek = 0i64;
    if let Some(zeit) = teile.get(mi + 2) {
        if zeit.contains(':') {
            let mut z = zeit.split(':');
            h = z.next()?.parse().ok()?;
            min = z.next()?.parse().ok()?;
            sek = z.next()?.parse().ok()?;
            if !(0..24).contains(&h) || !(0..60).contains(&min) || !(0..60).contains(&sek) {
                return None;
            }
        }
    }
    let offset = teile.get(mi + 3).and_then(|z| zone_offset(z)).unwrap_or(0);
    let monat = monat_nummer(teile.get(mi)?)?;
    Some(tage_seit_epoch(jahr, monat, tag as u32) * 86_400 + h * 3600 + min * 60 + sek - offset)
}

fn monat_nummer(s: &str) -> Option<u32> {
    MONATE_KURZ
        .iter()
        .position(|m| m.eq_ignore_ascii_case(s))
        .map(|i| i as u32 + 1)
}

/// Zeitzonen-Angabe in Sekunden (nach Osten positiv).
fn zone_offset(s: &str) -> Option<i64> {
    let s = s.trim_end_matches(',');
    if ["GMT", "UTC", "Z", "gmt", "utc"].contains(&s) {
        return Some(0);
    }
    // +0200 / -0500 / +02:00
    let s = s.trim_start_matches(['"', '\'']);
    let (vorzeichen, zahl) = match s.strip_prefix('+') {
        Some(z) => (1i64, z),
        None => match s.strip_prefix('-') {
            Some(z) => (-1, z),
            None => return Some(0),
        },
    };
    let zahl: String = zahl.chars().filter(|c| c.is_ascii_digit()).collect();
    if zahl.is_empty() {
        return None;
    }
    let (h, m) = if zahl.len() <= 2 {
        (zahl.parse::<i64>().ok()?, 0)
    } else {
        (
            zahl[..zahl.len() - 2].parse::<i64>().ok()?,
            zahl[zahl.len() - 2..].parse::<i64>().ok()?,
        )
    };
    Some(vorzeichen * (h * 3600 + m * 60))
}

fn epoch_sekunden(t: SystemTime) -> Option<i64> {
    t.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Kalendergrenze fuer Datumsumrechnungen: Werte ausserhalb werden
/// NICHT umgerechnet (Ein-/Ausgabe-Datum weit ausserhalb der echten
/// Zeit sind ein Angriff, keine Eingabe). 1601 bis 9999 deckt die
/// gesamte HTTP-Datenhistorie und Zukunft ab (M3: auch hier eine
/// benannte Grenze statt eines offenen Bereichs).
pub const DATUM_JAHR_MIN: i64 = 1601;
pub const DATUM_JAHR_MAX: i64 = 9999;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn traversal_wird_blockiert() {
        let wurzel = Path::new("/srv/www");
        assert!(sicherer_pfad(wurzel, "/../etc/passwd").is_none());
        assert!(sicherer_pfad(wurzel, "/a/../../etc").is_none());
        assert!(sicherer_pfad(wurzel, "/%2e%2e/etc").is_none());
        assert!(sicherer_pfad(wurzel, "/a\\b").is_none());
    }

    #[test]
    fn normal_bleibt_drinen() {
        let wurzel = Path::new("/srv/www");
        let p = sicherer_pfad(wurzel, "/seite/index.narsil").unwrap();
        assert!(p.starts_with(wurzel));
        assert!(p.ends_with("seite/index.narsil"));
    }

    #[test]
    fn tausend_doppelpunkt_durch_doppelpunkt_sind_begrenzt() {
        let wurzel = Path::new("/srv/www");
        // 10.000 Segmente: aus der Eingabe wird die Laenge der
        // Schleife — und die hat jetzt eine Obergrenze (M3).
        let pfad = "/".to_string() + &"../".repeat(10_000);
        let start = std::time::Instant::now();
        assert!(sicherer_pfad(wurzel, &pfad).is_none());
        assert!(start.elapsed() < Duration::from_millis(100));
        // Genau an der Grenze geht es noch, darueber nicht. WICHTIG:
        // ein Pfad ohne fuehrenden Slash hat kein leeres Restsegment,
        // deshalb hier ohne haengenden `/`.
        let ok = "/".to_string() + &"a/".repeat(MAX_PFADE_SEGMENTE - 1) + "a";
        assert!(sicherer_pfad(wurzel, &ok).is_some());
        let zu_viel = "/".to_string() + &"a/".repeat(MAX_PFADE_SEGMENTE) + "a";
        assert!(sicherer_pfad(wurzel, &zu_viel).is_none());
    }

    #[test]
    fn content_type_narsil_zuerst() {
        assert_eq!(
            content_type_fuer(Path::new("a.narsil")),
            "text/narsil; charset=utf-8"
        );
        assert_eq!(
            content_type_fuer(Path::new("a.HTML")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type_fuer(Path::new("a.unbekannt")),
            "application/octet-stream"
        );
    }

    #[test]
    fn datei_antwort_404_mit_namen() {
        // KEIN /tmp: M9 verbietet Testdaten dort (tmpfs = RAM).
        let r = datei_antwort(
            Path::new("/home/jaff/mutation_md/server/gibt-es-nicht-12345"),
            "/fehlt.narsil",
        );
        assert_eq!(r.status.0, 404);
        assert!(String::from_utf8_lossy(&r.body).contains("nicht gefunden"));
    }

    #[test]
    fn http_datum_rundlauf() {
        let t = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        let s = http_datum(t);
        // RFC-Format: "Wochentag, TT Mon YYYY HH:MM:SS GMT"
        assert!(s.contains("GMT"), "{s}");
        assert!(s.len() >= 29, "{s}");
        let zurueck = http_datum_sekunden(&s).expect("datum lesbar");
        let echt = epoch_sekunden(t).unwrap();
        assert!((zurueck - echt).abs() <= 1, "{s} -> {zurueck} vs {echt}");
        // Feste Werte (nicht geraten): 1970-01-01 war ein Donnerstag.
        assert_eq!(http_datum(UNIX_EPOCH), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(
            http_datum_sekunden("Thu, 01 Jan 1970 00:00:00 GMT"),
            Some(0)
        );
        // Zeitzone: +0200 ist zwei Stunden VOR UTC.
        assert_eq!(
            http_datum_sekunden("Wed, 30 Sep 2026 04:10:00 +0200"),
            http_datum_sekunden("Wed, 30 Sep 2026 02:10:00 GMT")
        );
        // Unverstaendliches bleibt draussen (kein `0` und kein Raten).
        assert!(http_datum_sekunden("gar kein Datum").is_none());
        assert!(http_datum_sekunden("Wed, 99 Xxx 2026 04:10:00 GMT").is_none());
        assert!(http_datum_sekunden("Wed, 30 Sep 2026 25:99:99 GMT").is_none());
    }

    #[test]
    fn range_wird_ausgewertet() {
        assert_eq!(range_auswerten("bytes=0-4", 10), Some((0, Some(4))));
        assert_eq!(range_auswerten("bytes=5-", 10), Some((5, None)));
        assert_eq!(range_auswerten("bytes=-3", 10), Some((7, None)));
        assert_eq!(range_auswerten("bytes=0-999", 10), Some((0, Some(999))));
        assert_eq!(range_auswerten("items=0-1", 10), None);
        assert_eq!(range_auswerten("bytes=0-1,3-4", 10), None);
        assert_eq!(range_auswerten("bytes=9-3", 10), None);
        assert_eq!(range_auswerten("bytes=abc", 10), None);
    }

    #[test]
    fn etag_trifft_auch_mit_liste() {
        let e = "\"abc-123\"";
        assert!(etag_trifft("\"abc-123\"", e));
        assert!(etag_trifft("W/\"abc-123\"", e));
        assert!(etag_trifft("*", e));
        assert!(etag_trifft("\"anderes\", \"abc-123\"", e));
        assert!(!etag_trifft("\"anderes\"", e));
    }

    /// EINE echte Datei — geschrieben, ausgeliefert, berechnet, dann
    /// wieder 304. Bislang gab es hier nur Gegenproben (`404`,
    /// Blockaden); was NIE getestet war, ist die eine Frage, die ein
    /// statischer Server beantworten muss: kommt der Inhalt an?
    ///
    /// M9: die Datei liegt unter `/home/jaff/mutation_md/server/`, nicht
    /// unter `/tmp` (tmpfs = RAM, und ein Test, der RAM frisst, ist kein
    /// Test auf Begrenzung).
    #[test]
    fn echte_datei_wird_gestreamt_richtig_und_bedingt() {
        use crate::http::{Method, anfrage_aus_text, write_response};
        use std::fs;

        let wurzel = Path::new("/home/jaff/mutation_md/server/statisch");
        fs::create_dir_all(wurzel).expect("Wurzel anlegbar");
        fs::write(wurzel.join("probe.narsil"), b"0123456789").expect("Datei schreibbar");

        // (1) Ganze Datei: 200, Koerper GESTREAMT — also nirgends im
        //     `Vec` der Antwort, aber beim Schreiben vollstaendig da.
        let ganz = datei_antwort(wurzel, "/probe.narsil");
        assert_eq!(ganz.status.0, 200, "Datei existiert");
        let d = ganz.datei.as_ref().expect("gestreamt, nicht gelesen");
        assert_eq!((d.offset, d.laenge), (0, 10));
        assert!(
            ganz.body.is_empty(),
            "die Bytes duerfen nicht im Vec stecken"
        );
        assert_eq!(
            ganz.header("content-type"),
            Some("text/narsil; charset=utf-8")
        );
        assert_eq!(ganz.header("accept-ranges"), Some("bytes"));
        let etag = ganz.header("etag").expect("ETag fehlt").to_string();

        let mut roh = Vec::new();
        write_response(&mut roh, &ganz, Method::Get).expect("schreibbar");
        let text = String::from_utf8_lossy(&roh);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
        assert!(text.contains("Content-Length: 10"), "{text}");
        assert!(
            text.ends_with("0123456789"),
            "Koerper fehlt oder ist verkuerzt: {text}"
        );

        // (2) Bereich 3..=6: 206, offset und Laenge stimmen, und der
        //     geschriebene Koerper ist GENAU diese vier Bytes.
        let teil = datei_antwort_fuer(
            wurzel,
            "/probe.narsil",
            &anfrage_aus_text("GET", "/probe.narsil", &[("Range", "bytes=3-6")], &[]),
        );
        assert_eq!(teil.status.0, 206);
        let d = teil.datei.as_ref().expect("206 streamt auch");
        assert_eq!((d.offset, d.laenge), (3, 4));
        assert_eq!(teil.header("content-range"), Some("bytes 3-6/10"));
        let mut roh2 = Vec::new();
        write_response(&mut roh2, &teil, Method::Get).expect("schreibbar");
        let text2 = String::from_utf8_lossy(&roh2);
        assert!(text2.starts_with("HTTP/1.1 206 Partial Content"), "{text2}");
        assert!(text2.ends_with("3456"), "{text2}");

        // (3) Derselbe ETag zurueck: 304, ohne Koerper, aber mit der
        //     Groesse der Darstellung im Kopf.
        let wieder = datei_antwort_fuer(
            wurzel,
            "/probe.narsil",
            &anfrage_aus_text("GET", "/probe.narsil", &[("If-None-Match", &etag)], &[]),
        );
        assert_eq!(wieder.status.0, 304);
        assert!(wieder.datei.is_none(), "304 schickt keine Datei");
        let mut roh3 = Vec::new();
        write_response(&mut roh3, &wieder, Method::Get).expect("schreibbar");
        let text3 = String::from_utf8_lossy(&roh3);
        assert!(text3.starts_with("HTTP/1.1 304 Not Modified"), "{text3}");
        assert!(text3.contains("Content-Length: 10"), "{text3}");

        // (4) Der Ausgang des Pfads bleibt draussen, auch als Datei.
        assert_eq!(
            datei_antwort(wurzel, "/../statisch/probe.narsil").status.0,
            403
        );
        assert_eq!(datei_antwort(wurzel, "/gibt-es-nicht.narsil").status.0, 404);
    }

    /// Echte Website, echte Anfragen, echter gzip-Rundlauf (S7).
    ///
    /// Der Punkt ist nicht "komprimiert es?", sondern "kommt die
    /// DATEI wieder an?" — deshalb `gunzip` auf die Originaldaten,
    /// nicht auf die Erwartung.
    #[test]
    fn gzip_komprimiert_echte_dateien_und_der_rundlauf_stimmt() {
        use crate::http::{Method, anfrage_aus_text, write_response};

        let wurzel = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/site"));
        let roh = std::fs::read(wurzel.join("index.html")).expect("echte Datei da");

        // (1) Mit `Accept-Encoding: gzip`: kodiert, kuerzer, eigener
        //     ETag — und der Inhalt kommt EXAKT wieder.
        let anfrage = anfrage_aus_text(
            "GET",
            "/index.html",
            &[("Accept-Encoding", "gzip, deflate")],
            &[],
        );
        let komp = datei_antwort_fuer(wurzel, "/index.html", &anfrage);
        assert_eq!(komp.status.0, 200);
        assert_eq!(komp.header("content-encoding"), Some("gzip"));
        assert_eq!(komp.header("vary"), Some("Accept-Encoding"));
        assert!(
            komp.body.starts_with(&[0x1f, 0x8b]),
            "gzip-Magic-Bytes fehlen"
        );
        assert!(
            komp.body.len() < roh.len(),
            "{} Bytes kodiert statt {}",
            komp.body.len(),
            roh.len()
        );
        let etag_komp = komp.header("etag").expect("ETag fehlt").to_string();
        assert!(etag_komp.ends_with("-gzip\""), "{etag_komp}");
        assert!(
            !komp.body.is_empty() && komp.datei.is_none(),
            "gzip steckt im Vec, nicht als Datei"
        );

        // RUNDLAUF: `narsil_compress::gunzip` zerlegt zurzeit seinen
        // eigenen gzip-Strom falsch (es laesst den ZLIB-Entpacker auf
        // Roh-DEFLATE los — gemeldet in `team/WUNSCH_SERVER.txt`).
        // Der Beweis geht deshalb ueber dieselben Roh-DEFLATE-Bytes:
        // gzip-Kopf abziehen, zlib-Rahmen drum, Adler32 der
        // Originaldaten darunter — und der Entpacker MUSS exakt die
        // Datei von der Platte zurueckgeben. Schlecht komprimierte
        // Bytes wuerden hier anders herauskommen oder gar nicht.
        let payload = &komp.body[10..komp.body.len() - 8];
        let mut zlib = Vec::with_capacity(payload.len() + 6);
        zlib.push(0x78); // CM=8, CINFO=7
        zlib.push(0x01); // FCHECK passt, kein FDICT
        zlib.extend_from_slice(payload);
        zlib.extend_from_slice(&adler32_test(&roh).to_be_bytes());
        let mut zurueck = Vec::new();
        narsil_compress::zlib_entpacken(&zlib, &mut zurueck, roh.len() + 64)
            .expect("Rundlauf ueber dieselben DEFLATE-Bytes");
        assert_eq!(zurueck, roh, "Rundlauf gibt NICHT die Originaldaten");

        let mut geschrieben = Vec::new();
        write_response(&mut geschrieben, &komp, Method::Get).expect("schreibbar");
        assert!(
            String::from_utf8_lossy(&geschrieben)
                .contains(&format!("Content-Length: {}", komp.body.len())),
            "Content-Length nennt die KODIERTE Laenge"
        );

        // (2) Ohne `Accept-Encoding`: roh, gestreamt, roher ETag.
        let roh_antwort = datei_antwort_fuer(
            wurzel,
            "/index.html",
            &anfrage_aus_text("GET", "/index.html", &[], &[]),
        );
        assert_eq!(roh_antwort.header("content-encoding"), None);
        assert!(roh_antwort.datei.is_some(), "roh wird gestreamt");
        assert!(roh_antwort.body.is_empty());
        let etag_roh = roh_antwort.header("etag").expect("ETag fehlt");
        assert!(!etag_roh.ends_with("-gzip\""), "{etag_roh}");

        // (3) `gzip;q=0` ist eine Ablehnung, keine Einladung.
        let abgelehnt = datei_antwort_fuer(
            wurzel,
            "/index.html",
            &anfrage_aus_text(
                "GET",
                "/index.html",
                &[("Accept-Encoding", "gzip;q=0, identity")],
                &[],
            ),
        );
        assert_eq!(abgelehnt.header("content-encoding"), None, "q=0 lehnt ab");
        assert!(abgelehnt.datei.is_some());

        // (4) `*` erlaubt alles, was nicht ausdruecklich aus ist.
        assert!(accept_encoding_gzip(&anfrage_aus_text(
            "GET",
            "/x",
            &[("Accept-Encoding", "*")],
            &[]
        )));
        assert!(!accept_encoding_gzip(&anfrage_aus_text(
            "GET",
            "/x",
            &[("Accept-Encoding", "br, zstd")],
            &[]
        )));
        assert!(!accept_encoding_gzip(&anfrage_aus_text(
            "GET",
            "/x",
            &[],
            &[]
        )));

        // (5) Ein Bild bleibt roh — gzip wuerde es nur groesser machen.
        let bild = datei_antwort_fuer(
            wurzel,
            "/bild.png",
            &anfrage_aus_text("GET", "/bild.png", &[("Accept-Encoding", "gzip")], &[]),
        );
        assert_eq!(bild.header("content-encoding"), None, "PNG bleibt roh");
        assert!(bild.datei.is_some(), "PNG wird weiter gestreamt");

        // (6) Range bleibt roh, auch wenn gzip gewuenscht wird.
        let bereich = datei_antwort_fuer(
            wurzel,
            "/index.html",
            &anfrage_aus_text(
                "GET",
                "/index.html",
                &[("Accept-Encoding", "gzip"), ("Range", "bytes=0-9")],
                &[],
            ),
        );
        assert_eq!(bereich.status.0, 206);
        assert_eq!(bereich.header("content-encoding"), None, "Range bleibt roh");
        // 206 ohne Content-Range waere kein 206, sondern eine 200 mit
        // anderer Nummer.
        assert!(
            bereich.header("content-range").is_some(),
            "Content-Range fehlt beim 206"
        );

        // (7) Die Variante erkennt den eigenen ETag wieder (304) und
        //     den rohen NICHT — sonst haette ein Cache zwei Fassungen
        //     fuer eine gehalten.
        let wieder_komp = datei_antwort_fuer(
            wurzel,
            "/index.html",
            &anfrage_aus_text(
                "GET",
                "/index.html",
                &[("Accept-Encoding", "gzip"), ("If-None-Match", &etag_komp)],
                &[],
            ),
        );
        assert_eq!(wieder_komp.status.0, 304, "eigener ETag trifft");
        let fremd = datei_antwort_fuer(
            wurzel,
            "/index.html",
            &anfrage_aus_text(
                "GET",
                "/index.html",
                &[("Accept-Encoding", "gzip"), ("If-None-Match", etag_roh)],
                &[],
            ),
        );
        assert_eq!(
            fremd.status.0, 200,
            "roher ETag gehoert nicht zur kodierten Fassung"
        );
        assert_eq!(fremd.header("content-encoding"), Some("gzip"));
    }

    /// Die Grenzen, bevor sie greifen — ohne eine 1-MiB-Datei
    /// schreiben zu muessen (M3: aus der Konfiguration, nicht aus der
    /// Eingabe).
    #[test]
    fn gzip_grenzen_stehen_vor_der_anfrage() {
        use crate::http::anfrage_aus_text;

        let gzip_anfrage = anfrage_aus_text("GET", "/x", &[("Accept-Encoding", "gzip")], &[]);

        assert!(kompressibel("text/html; charset=utf-8"));
        assert!(kompressibel("application/json"));
        assert!(kompressibel("image/svg+xml"));
        assert!(kompressibel("application/vnd.api+json"));
        assert!(!kompressibel("image/png"));
        assert!(!kompressibel("application/octet-stream"));

        // Die Grenze entscheidet VOR dem Komprimieren.
        assert!(gzip_machbar(&gzip_anfrage, "text/html", 10));
        assert!(
            !gzip_machbar(&gzip_anfrage, "text/html", 0),
            "leer lohnt nie"
        );
        assert!(
            !gzip_machbar(&gzip_anfrage, "text/html", MAX_GZIP_DATEI + 1),
            "ueber der Grenze wird nicht mal der ETag zur Variante"
        );
        assert!(
            !gzip_machbar(&gzip_anfrage, "image/png", 10),
            "unkomprimierba bleibt roh"
        );
        assert!(
            !gzip_machbar(
                &anfrage_aus_text(
                    "GET",
                    "/x",
                    &[("Accept-Encoding", "gzip"), ("Range", "bytes=0-1")],
                    &[]
                ),
                "text/html",
                10
            ),
            "Range schaltet gzip ab"
        );

        // ETag-Varianten: Suffix VOR dem schliessenden Anfuehrungszeichen.
        assert_eq!(etag_variante("\"abc-123\"", "gzip"), "\"abc-123-gzip\"");
        assert_eq!(etag_variante("\"abc\"", "gzip"), "\"abc-gzip\"");
        assert_eq!(etag_variante("ohne-klammer", "gzip"), "ohne-klammer-gzip");
    }

    /// Adler32 nur fuer den Test-Rundlauf (RFC 1950, Abschnitt 9).
    ///
    /// `narsil-compress` hat eine eigene, aber nicht exportiert; hier
    /// nachzubauen waere es sonst, die Pruefsumme selbst zu erfinden —
    /// die Formel ist zehn Zeilen Standard und steht in jedem RFC.
    fn adler32_test(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in bytes {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
}
