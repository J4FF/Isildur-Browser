//! W7B P3 — EXTERNE STYLESHEETS (`<link rel="stylesheet">`).
//!
//! # DER BEFUND (Koordination, hoechste Priorita)
//!
//! „`<link rel="stylesheet" href="...">` wird nicht geladen — nur `<style>`.
//! Fast jede echte Seite hat ihr CSS extern (rust-lang.org erscheint
//! deshalb dunkel statt hell)."
//!
//! # DER WEG: DAS BLATT WIRD ZU EINEM `<style>` AN SEINER STELLE
//!
//! Der geholte CSS-Text wird als `<style>`-Knoten an DIE STELLE gesetzt,
//! an der das `<link>` stand. Dadurch stimmt die Reihenfolge von selbst:
//! die Kaskade liest `Seite::stile` in Dokumentreihenfolge, und ein
//! externes Blatt steht damit genau dort, wo es in der Quelle stand —
//! zwischen zwei Inline-Blaettern, nicht am Ende.
//!
//! GEMESSEN, warum nicht `Seite::stile` von aussen fuellen: `Seite` ist
//! das ERGEBNIS des Baus, und die Blaetter stehen im Baum, den man
//! braucht, BEVOR man baut (`html_lesen` → CSS holen → `seite_aus_knoten…`).
//!
//! # DIE GRENZEN (alle benannt, M3/M4)
//!
//! * [`MAX_BLAETTER`] 32 Blaetter je Seite.
//! * [`MAX_BLAETTER_BYTES`] 4 MiB je Blatt — **vor** dem Lesen geprueft
//!   ueber `Content-Length`, und der Body wird nicht beendet.
//! * [`MAX_IMPORT_TIEFE`] 3 Ebenen `@import`.
//! * `media="print"` wird uebersprungen (ein Druck-Stylesheet im
//!   Bildschirmbrower ist Papier auf dem Bildschirm).

use narsil_fmt_html::HtmlNode;
use std::sync::Arc;

/// Hoechste Zahl externer Stylesheets je Seite (M3).
pub const MAX_BLAETTER: usize = 32;
/// Hoechste Groesse EINES Blattes in Byte (M4).
///
/// YouTubes Hauptstylesheet hat etwa 3,5 MiB; es muss vollstaendig
/// geladen werden. Die Gesamtgrenze von 8 MiB bleibt bestehen.
pub const MAX_BLAETTER_BYTES: usize = 4 * 1024 * 1024;
/// Hoechste Summe aller Blaetter in Byte (M4).
///
/// GEMESSEN, warum es diese Grenze gibt: 32 x 2 MiB waeren 64 MiB CSS im
/// Speicher — bei einem 4-GiB-Limit (M1) waere das der Tod einer halben
/// Maschine. Also gilt das Doppelte, nicht nur das Einfache.
pub const MAX_SUMME_BYTES: usize = 8 * 1024 * 1024;
/// Hoechste Tiefe von `@import` (M3/M5).
pub const MAX_IMPORT_TIEFE: usize = 3;

/// Das Ergebnis: der Baum mit den geholten Blaettern, plus eine Meldung.
pub struct Ergebnis {
    /// Der neue Baum (der alte ist unveraendert — er gehoert der Seite).
    pub baum: Arc<Vec<HtmlNode>>,
    /// Die Basis, gegen die die relativen Links aufgeloest wurden.
    ///
    /// GEMESSEN, warum sie mit herausgeht: `Sitzung` baut die Seite mit
    /// der Adresse, und ein Tree, dessen Links gegen eine ANDERE Basis
    /// aufgeloest wurden, waere eine Seite, deren CSS nicht passt.
    pub basis: String,
    /// Wie viele Blaetter geholt wurden.
    pub geholt: usize,
    /// Die benannten Fehler (fremde Herkunft, zu gross, 404).
    pub fehler: Vec<String>,
}

/// Holt alle `<link rel="stylesheet">` und setzt sie als `<style>` an ihre
/// Stelle.
///
/// GEMESSEN, warum die Meldungen IMMER gefuellt werden (nicht nur bei
/// Fehlern): ein Blatt, das nicht kam, sieht man der Seite nicht an — man
/// sieht nur, dass sie „falsch" aussieht. Die Zahl der geholten Blaetter
/// steht in der Statuszeile.
pub fn blaetter_holen(baum: &Arc<Vec<HtmlNode>>, basis: &str) -> Ergebnis {
    let mut r = Ergebnis {
        baum: baum.clone(),
        basis: basis.to_string(),
        geholt: 0,
        fehler: Vec::new(),
    };
    // 1. Die Links in Dokumentreihenfolge sammeln (M3: `MAX_BLAETTER`).
    let mut links: Vec<(Vec<usize>, String, String)> = Vec::new();
    sammle_links(baum, &[], basis, &mut links);
    links.truncate(MAX_BLAETTER);
    if links.is_empty() {
        stil_importe_einsetzen(&mut r);
        return r;
    }
    // 2. Holen — **parallel, nach Rang** (03.10.2026, Rolle NETZTEMPO).
    //
    // WAS SICH GEAENDERT HAT und was nicht: **nur die Reihenfolge der
    // Abrufe.** Vorher stand hier eine `for`-Schleife, die ein Blatt nach
    // dem anderen holte; jedes kostete eine eigene Rundlaufzeit, und vier
    // Blaetter von vier Hosts waren 4 x Rundlaufzeit.
    //
    // GEMESSENE GRUNDLAGE (`team/berichte/NETZTEMPO_1.txt`): ueber die zehn
    // Referenzseiten mit warmem Cache **35 566 ms -> 9 942 ms**; lokal ohne
    // Internet 6 Blaetter **904 -> 152 ms** bei 6 von 6 Antworten.
    //
    // UNVERAENDERT, und jede Zeile ist unten am Aufruf zu sehen:
    //   * die Links kommen in **Dokumentreihenfolge** (`sammle_links`),
    //   * `media=print` faellt raus,
    //   * jede URL **genau einmal**,
    //   * der Werbefilter laeuft **vor** dem Abruf (`hole_blatt` -> `vorbereite`),
    //   * `MAX_BLAETTER_BYTES` **vor** dem Lesen des Bodys,
    //   * `MAX_SUMME_BYTES` bricht weiterhin **ab**,
    //   * `@import` bis `MAX_IMPORT_TIEFE`,
    //   * die Fehlermeldungen haben denselben Wortlaut,
    //   * die Antworten werden in **Eingangsfolge** ausgewertet, nicht in
    //     Reihenfolge des Eintreffens — sonst kaeme ein spaet geholtes Blatt
    //     an die Stelle eines fruehen, und die Kaskade laege anders.
    let mut geholt_css: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut summe = 0usize;
    // 2a. Die Aufgaben bauen. Der Werbefilter und die DNS-/Adressen-
    //     Pruefungen laufen **hier**, also **vor** dem Abruf — das ist die
    //     A-ADBLOCK-Regel und sie darf nicht erst beim Auswerten greifen.
    let mut aufgaben: Vec<narsil_net::parallel::Aufgabe> = Vec::new();
    // DIE ADRESSE JE AUFGABE, und sie gehoert **hierher** und nicht
    // spaeter rekonstruiert zu werden: `setze_stile` sucht mit dem String
    // aus `links`, und dieser ist der Pfad von `Request` **nicht**
    // (der enthaelt weder Schema noch Host). Siehe der Hinweis bei 2c.
    let mut urls_der_aufgaben: Vec<String> = Vec::new();
    let mut schon_geplant: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_pfad, quelle, medium) in &links {
        if medium.split(',').any(|m| m.trim() == "print") {
            continue;
        }
        if !schon_geplant.insert(quelle.as_str()) {
            continue;
        }
        // Release 10.10.2026: lokale Blaetter einer LOKALEN Seite direkt von
        // der Platte (wie die Skripte, `js::quelle_holen`). Eine Seite aus
        // dem Netz darf keine `file://`-Blaetter einbinden (wie Chrome).
        // GEMESSEN vorher: „CSS: 0 Blätter geladen, 1 nicht" — jede lokale
        // Testseite stand ohne ihr CSS da.
        if let Some(rest) = quelle.strip_prefix("file://") {
            if !basis.starts_with("file://") {
                r.fehler.push(std::format!("CSS {quelle}: lokale Datei in einer Seite aus dem Netz"));
                narsil_page::blattschrank::fehler(quelle);
                continue;
            }
            let gelesen = crate::laden::dateipfad(rest)
                .map_err(|e| e.to_string())
                .and_then(|pfad| {
                    let meta = std::fs::metadata(&pfad).map_err(|e| std::format!("{pfad}: {e}"))?;
                    if meta.len() as usize > MAX_SUMME_BYTES {
                        return Err(std::format!("zu gross: {} Byte", meta.len()));
                    }
                    std::fs::read(&pfad).map_err(|e| std::format!("{pfad}: {e}"))
                });
            match gelesen {
                Ok(bytes) => {
                    let css = css_dekodieren(&bytes, None);
                    summe += css.len();
                    geholt_css.insert(quelle.clone(), css);
                    r.geholt += 1;
                }
                Err(e) => {
                    r.fehler.push(std::format!("CSS {quelle}: {e}"));
                    narsil_page::blattschrank::fehler(quelle);
                }
            }
            continue;
        }
        // Release 12.10.2026: `data:`-Blaetter direkt (WPT HTMLLinkElement-*:
        // `href="data:text/css,*{}"`). GEMESSEN vorher: „kein Netzziel".
        if quelle.starts_with("data:") {
            match crate::bilder_daten::lesen(quelle) {
                Ok(bytes) => {
                    let css = css_dekodieren(&bytes, None);
                    summe += css.len();
                    geholt_css.insert(quelle.clone(), css);
                    r.geholt += 1;
                }
                Err(e) => {
                    r.fehler.push(std::format!("CSS {quelle}: {e}"));
                    narsil_page::blattschrank::fehler(quelle);
                }
            }
            continue;
        }
        match vorbereite(quelle) {
            Ok(anfrage) => {
                // `Sichtbar`, weil ohne dieses Blatt die Seite **nackt** ist —
                // es gibt nichts, was der Nutzer zuerst sehen koennte.
                aufgaben.push(narsil_net::parallel::Aufgabe::neu(
                    narsil_net::parallel::Rang::Sichtbar,
                    anfrage,
                ));
                urls_der_aufgaben.push(quelle.clone());
            }
            Err(e) => {
                r.fehler.push(std::format!("CSS {quelle}: {e}"));
                narsil_page::blattschrank::fehler(quelle);
            }
        }
    }
    // 2b. Gemeinsam holen.
    //
    // **DIE MESSUHR**, und sie haengt an einer **Umgebungsvariable**, weil
    // `narsil-probe` die `ladezeit_ms` ueber die **ganze** Seite misst —
    // Dokument, Bilder, Layout, Raster. Der Blatt-Abruf ist darin nur ein
    // Anteil, und die Frage „hat die Aenderung etwas gebracht?" laesst sich
    // mit der Gesamtzeit nicht beantworten. `NARSIL_BLAETTER_MESSEN=1`
    // schreibt deshalb **eine** Zeile je Seite nach `stderr`:
    //     BLAETTER-MESSUNG modus=parallel aufgaben=3 geholt=3 fehler=0 47 ms
    // Im Betrieb ist die Variable nicht gesetzt, es wird nichts geschrieben.
    let messen = std::env::var_os("NARSIL_BLAETTER_MESSEN").is_some();
    let t_start = std::time::Instant::now();
    //
    // DER SCHALTER `NARSIL_BLAETTER_SERELL`, und der Grund fuer sein Dasein
    // ist eine **Fehlmessung**, nicht Bequemlichkeit. GEMESSEN am 03.10.:
    // der Vergleich „Probe vorher / Probe nachher" ueber die zehn
    // Referenzseiten ergab **22 079 ms -> 37 022 ms** — also **schlechter**,
    // obwohl die zweite Messung mit dem parallelen Code lief. Diese Zahl
    // ist unbrauchbar, und die zwei Gruende sind gemessen, nicht vermutet:
    // * die Last der Maschine lag bei `load average 7.4` (es bauten zur
    //   selben Zeit vier fremde Crates), und
    // * zwischen den Laeufen hat eine andere Rolle `narsil` geaendert: die
    //   Rahmenzahl sprang von 94 auf 52, die Linkzahl von 241 auf 242, und
    //   duckduckgo.com wechselte von „Laden fehlgeschlagen" auf „geladen".
    // Zwei Laeufe, die sich im **Rauschen** unterscheiden, sind kein Vorher.
    //
    // DER WEG, der daraus folgt: **beide Wege im selben Programm**, per
    // Umgebungsschalter, damit Bin und Maschinenzustand **gleich** sind.
    // `NARSIL_BLAETTER_SERELL=1` nimmt den alten Weg (ein Blatt nach dem
    // anderen, direkt `fetch` je Aufgabe) — er raeumt den alten Code nicht
    // wieder auf, er ruft denselben Weg nacheinander auf. Im Betrieb ist die
    // Variable nicht gesetzt, also laeuft der parallele Weg.
    let antworten = if std::env::var_os("NARSIL_BLAETTER_SERELL").is_some() {
        aufgaben
            .iter()
            .map(|a| narsil_net::parallel::Ergebnis {
                nummer: 0,
                rang: a.rang,
                anfrage: a.anfrage.clone(),
                antwort: narsil_net::fetch(&a.anfrage),
            })
            .collect::<Vec<_>>()
    } else {
        narsil_net::parallel::hole_alle(&aufgaben, narsil_net::Limits::default())
    };
    // 2c. Auswerten, **in Eingangsfolge**.
    //
    // DER SCHLUESSEL, und er ist die ganze Bruecke zum Einsetzen: hier
    // muss die **volle URL** stehen, denn `setze_stile` schlaegt spaeter
    // mit `narsil_url::loese_auf(basis, href)` nach — also nach genau dem
    // String, der in `links` stand. GEMESSENER FEHLER, den diese Zeile in
    // der ersten Fassung hatte: sie nahm `a.anfrage.path` (nur `/a.css`),
    // und damit fand `setze_stile` **keinen** Knoten — die Blaetter wurden
    // geholt, in `geholt_css` gespeichert und dann nie eingebaut. Die Seite
    // sah aus wie vorher, nur schneller geladen: eine Aenderung, die man
    // beim Laden nicht bemerkt und beim **Bild** sofort.
    for (i, antwort) in antworten.iter().enumerate() {
        let quelle = match urls_der_aufgaben.get(i) {
            Some(u) => u.clone(),
            None => continue,
        };
        let css = match &antwort.antwort {
            Ok(a) => match fertig(&quelle, a, 0) {
                Ok(css) => css,
                Err(e) => {
                    r.fehler.push(std::format!("CSS {quelle}: {e}"));
                    narsil_page::blattschrank::fehler(&quelle);
                    continue;
                }
            },
            Err(e) => {
                r.fehler.push(std::format!("CSS {quelle}: {e}"));
                narsil_page::blattschrank::fehler(&quelle);
                continue;
            }
        };
        summe += css.len();
        if summe > MAX_SUMME_BYTES {
            r.fehler.push(std::format!(
                "CSS: mehr als {} Byte geholt — der Rest faellt weg",
                MAX_SUMME_BYTES
            ));
            break;
        }
        geholt_css.insert(quelle, css);
        r.geholt += 1;
    }
    // 3. Einbauen: an der Stelle des `<link>` ein `<style>`.
    // Release 12.10.2026: der Link bleibt im Baum ein Link; das CSS liegt im
    // Blattschrank, aus dem JEDER Seitenbau liest (siehe
    // `narsil_page::blattschrank`). GEMESSEN vorher: jedes geholte `<link>`
    // war ein `<style>` — fuer Skripte gab es keinen Link mehr.
    for (url, css) in &geholt_css {
        narsil_page::blattschrank::ablegen(url, css);
    }
    stil_importe_einsetzen(&mut r);
    // DIE ZEILE DER MESSUHR, siehe oben. `geholt` und `fehler` stehen
    // **vor** dem Einsetzen, weil der Aufrufer an `fehler.len()` erkennt,
    // ob etwas fehlt — eine Zahl, die sich beim Einsetzen noch aendern
    // wuerde, waere eine andere Messung.
    //
    // DIE BASIS STEHT MITTEN IN DER ZEILE, und das ist eine **Korrektur
    // eines eigenen Messfehlers**: die erste Fassung schrieb nur eine Zahl
    // und keine Seite. `narsil-probe` laedt die zehn Seiten in **einander
    // eigenen Threads** (das ist sein Zweck, Zeitgrenze je Seite), also
    // ist die Reihenfolge der Zeilen **nicht** die Reihenfolge der Seiten,
    // und `aufgaben=3 geholt=1 fehler=2` liess sich **keiner** Seite
    // zuordnen. Eine Zahl ohne Ort ist bei nebenlaeufigen Laeufen keine
    // Angabe, sondern eine Behauptung mit Gramm. Darum: Seite, Zaehler,
    // und **jeder** Fehler einzeln mit seiner URL.
    if messen {
        eprintln!(
            "BLAETTER-MESSUNG basis={basis} modus={} aufgaben={} geholt={} fehler={} {} ms",
            if std::env::var_os("NARSIL_BLAETTER_SERELL").is_some() {
                "serell"
            } else {
                "parallel"
            },
            aufgaben.len(),
            r.geholt,
            r.fehler.len(),
            t_start.elapsed().as_millis()
        );
        // JEDER Fehler einzeln, mit der Adresse. Ohne die Adresse ist die
        // Zeile nicht benutzbar — das war der Fehler.
        for f in &r.fehler {
            eprintln!("BLAETTER-FEHLER basis={basis} {f}");
        }
        // MESSUNG (temporaer): sind die Blaetter als `<style>` auch
        // EINGEBAUT? `geholt=4` allein sagt darueber nichts.
        fn zaehle(liste: &[HtmlNode], style: &mut usize, link: &mut usize) {
            for k in liste {
                if let HtmlNode::Element {
                    tag,
                    attributes,
                    children, ..
                } = k
                {
                    let klein = tag.to_ascii_lowercase();
                    if klein == "style" {
                        *style += 1;
                    }
                    if klein == "link"
                        && attributes
                            .get("rel")
                            .map(|r| r.to_ascii_lowercase())
                            .is_some_and(|r| r.split_whitespace().any(|t| t == "stylesheet"))
                    {
                        *link += 1;
                    }
                    zaehle(children, style, link);
                }
            }
        }
        let (mut st, mut li) = (0usize, 0usize);
        zaehle(&r.baum, &mut st, &mut li);
        eprintln!("BLAETTER-EINBAU style={st} link_stylesheet={li}");
    }
    r
}

/// Die `<link rel=stylesheet>`-Knoten finden: Pfad (Kind-Indizes) + URL +
/// `media`.
fn sammle_links(
    liste: &[HtmlNode],
    pfad: &[usize],
    basis: &str,
    raus: &mut Vec<(Vec<usize>, String, String)>,
) {
    // M5: `MAX_BAU_TIEFE` — ein Baum, der tiefer ist, wird nicht
    // durchlaufen (sonst waere der Pfad unbegrenzt lang).
    if pfad.len() >= narsil_page::MAX_BAU_TIEFE {
        return;
    }
    for (i, k) in liste.iter().enumerate() {
        if let HtmlNode::Element {
            tag,
            attributes,
            children, ..
        } = k
        {
            let klein = tag.to_ascii_lowercase();
            if klein == "link" {
                let rel = attributes
                    .get("rel")
                    .map(|r| r.to_ascii_lowercase())
                    .unwrap_or_default();
                if rel.split_whitespace().any(|t| t == "stylesheet")
                    && let Some(href) = attributes.get("href")
                {
                    let url = narsil_url::loese_auf(basis, href).unwrap_or_else(|_| href.clone());
                    raus.push((
                        pfad.iter().copied().chain(std::iter::once(i)).collect(),
                        url,
                        attributes
                            .get("media")
                            .map(|m| m.to_ascii_lowercase())
                            .unwrap_or_default(),
                    ));
                }
            }
            // GEMESSEN, warum der Pfad als `Vec` und nicht als `&[usize]`
            // mit `collect()`: `collect()` in einen Slice braucht eine
            // bekannte Groesse. Der Pfad waere (M3) hoechstens
            // `MAX_BAU_TIEFE` (64) tief — eine `Vec` kostet dasselbe und
            // ist einfacher zu pruefen.
            let mut tiefer: Vec<usize> = pfad.to_vec();
            tiefer.push(i);
            if tiefer.len() >= narsil_page::MAX_BAU_TIEFE {
                continue;
            }
            sammle_links(children, &tiefer, basis, raus);
        }
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **DER AUFTEILUNGSPUNKT**, und er ist
/// die kleinste Aenderung, die die Parallele ueberhaupt zulaesst.
///
/// VORHER war `hole_blatt(url, tiefe)` ein einziger Weg: pruefen, **holen**,
/// auswerten, `@import`. Das Holen liegt jetzt **ausserhalb**, weil es von
/// `hole_alle` gemacht wird und dort **alle** Blaetter gemeinsam laufen.
/// Deshalb sind es zwei Funktionen:
///
/// * [`vorbereite`] — **vor** dem Abruf: Laengengrenze, Host/Port, Werbefilter,
///   Erweiterungsregeln, Sicherheitshaken. Liefert eine `Request`.
/// * [`fertig`] — **nach** dem Abruf: Statuscode, `MAX_BLAETTER_BYTES`, Text,
///   `@import`.
///
/// **WAS UNVERAENDERT BLEIBT, und das ist der ganze Punkt:** die Pruefungen
/// sind **dieselben Aufrufe in derselben Reihenfolge**. Nur dass sie jetzt auf
/// zwei Funktionen verteilt sind — die Fehlertexte sind wortgleich mit denen
/// von vorher, weil `hole_blatt` (unten) weiterhin existiert und genau diese
/// beiden aufruft.
pub(crate) fn vorbereite(url: &str) -> Result<narsil_net::Request, String> {
    // M3/M4: die URL-Länge ist eine Eingabe.
    if url.len() > 2048 {
        return Err("URL zu lang".to_string());
    }
    let https = url.starts_with("https://");
    let standard_port: u16 = if https { 443 } else { 80 };
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        None => url,
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
        return Err("kein Host".to_string());
    }
    // A-ADBLOCK (Koordination 01.10.2026, 21:45, „vor jedem
    // Unterressourcen-Abruf"): DER FILTER, vor dem Abruf.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Fehlermeldung so lautet: Der
    // Aufrufer dieser Funktion behandelt jeden Fehler gleich (das Blatt
    // fehlt) — das ist richtig fuer ein kaputtes Stylesheet und richtig
    // fuer ein blockiertes. Der Unterschied steht in `zahlen()`, wo
    // `blockiert` und `frei` getrennt gezaehlt werden.
    // GEMESSENER FEHLER, erster Fassung: Ich hatte hier `adresse`
    // geschrieben. Der Parameter heisst `url` (`fn hole_blatt(url:
    // &str, …)`) — der Compiler sagt es mit Zeilennummer. Es ist mein
    // vierter erfundener Bezeichner in dieser Sitzung.
    if !narsil_page::werbung::erlaubt(
        url,
        narsil_page::werbung::Art::Stylesheet,
        &narsil_page::werbung::seiten_host(),
    ) {
        return Err("vom Werbefilter blockiert".to_string());
    }
    //
    // G6b: dieselbe Luecke wie in `js.rs` (siehe dort) — der Werbefilter
    // lief fuer Stylesheets, die `declarativeNetRequest`-Regeln einer
    // Erweiterung nicht. Chrome nennt die Art `"stylesheet"`.
    if !crate::dienste::dnr_erlaubt(url, "stylesheet") {
        return Err("von einer Erweiterung blockiert".to_string());
    }
    //
    // added 03.10.2026, Rolle TLS (Freigabe der Leitung): DER SICHERHEITSHAKEN
    // — Mixed Content und CSP, **vor** dem Abruf.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `Stil` und nicht `Bild`: ein
    // `<link rel=stylesheet>` ist ein **Stylesheet**, und Mixed Content §3.1
    // nennt fuer „upgradeable“ nur „images and icons“. Fuer den
    // Style-Import gilt dieselbe Stufe wie fuer ein Blatt (hochstufen), und
    // das entscheidet `sicherheit::MixedArt`, nicht diese Datei.
    //
    // **UND `style-src` FEHLT, und das ist ein BEFUND:** `sicherheit::CspArt`
    // kennt `Skript`, `Bild` und `Verbindung` — **kein** `Stil`. Der Haken
    // reicht die Art durch und entscheidet hier **nichts**; der Weg ueber
    // `style-src` steht als Luecke in `team/BRETT.txt` (die Datei mit der
    // Regel gehoert der Rolle TLS, und `CspArt` steht in `sicherheit.rs`).
    if let Err(grund) = crate::sicherheit_haken::gate(crate::sicherheit_haken::Unterart::Stil, url)
    {
        return Err(std::format!("{url}: gesperrt — {grund}"));
    }
    // Release 10.10.2026: das Schema ausdruecklich mitgeben — `narsil-net`
    // erkennt TLS sonst nur an Port 443 (`ist_https`). GEMESSEN vorher:
    // `https://localhost:8443/` ging als KLARTEXT-HTTP an den TLS-Port.
    Ok(narsil_net::Request {
        method: narsil_net::Method::Get,
        host: host.clone(),
        port,
        path: pfad.to_string(),
        https: Some(https),
        ..narsil_net::Request::get(&host, port, pfad)
    })
}

/// Release 11.10.2026 (Agent CSS2): die Bytes eines Stylesheets als Text
/// (CSS Syntax 3 §3.2 „determine the fallback encoding"): ein BOM, sonst
/// das `charset` der HTTP-Antwort, sonst ein `@charset "...";` ganz am
/// Anfang (UTF-16 dort heisst UTF-8), sonst UTF-8.
pub(crate) fn css_dekodieren(roh: &[u8], content_type: Option<&str>) -> String {
    use narsil_fmt_html::kodierung::{Kodierung, dekodiere, kodierung_aus_label};
    if let Some(rest) = roh.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return dekodiere(rest, Kodierung::Utf8);
    }
    if let Some(rest) = roh.strip_prefix(&[0xFE, 0xFF]) {
        return dekodiere(rest, Kodierung::Utf16Be);
    }
    if let Some(rest) = roh.strip_prefix(&[0xFF, 0xFE]) {
        return dekodiere(rest, Kodierung::Utf16Le);
    }
    let aus_http = content_type.and_then(|ct| {
        ct.split(';').skip(1).find_map(|teil| {
            let (name, wert) = teil.split_once('=')?;
            name.trim().eq_ignore_ascii_case("charset").then(|| wert.trim().trim_matches(['"', '\'']).to_string())
        })
    });
    if let Some(k) = aus_http.as_deref().and_then(kodierung_aus_label) {
        return dekodiere(roh, k);
    }
    if let Some(rest) = roh.strip_prefix(b"@charset \"") {
        let fenster = &rest[..rest.len().min(1024)];
        if let Some(ende) = fenster.windows(2).position(|w| w == b"\";")
            && let Ok(label) = std::str::from_utf8(&fenster[..ende])
            && let Some(k) = kodierung_aus_label(label)
        {
            let k = if matches!(k, Kodierung::Utf16Le | Kodierung::Utf16Be) { Kodierung::Utf8 } else { k };
            return dekodiere(roh, k);
        }
    }
    dekodiere(roh, Kodierung::Utf8)
}

/// Release 11.10.2026 (Agent CSS2): `@import` in einem `<style>`-Element der
/// Seite (CSS Cascade 4 §2) — bisher nur in geholten Blaettern aufgeloest.
/// Der Inhalt des eingebundenen Blattes ersetzt die Zeile (wie in
/// `import_ersetzen`), die Basis ist die Seitenadresse. GEMESSEN vorher (WPT
/// `syntax/at-charset-*`, `at-rule-005`): `<style>@import "x.css";` holte
/// nichts.
/// Steht irgendwo ein `<style title>`?
fn hat_titel(liste: &[HtmlNode], tiefe: usize) -> bool {
    tiefe < 256
        && liste.iter().any(|k| match k {
            HtmlNode::Element { tag, attributes, children, .. } => {
                (tag.eq_ignore_ascii_case("style") && attributes.get("title").is_some_and(|t| !t.is_empty()))
                    || hat_titel(children, tiefe + 1)
            }
            _ => false,
        })
}

fn stil_importe_einsetzen(r: &mut Ergebnis) {
    fn hat_import(liste: &[HtmlNode], tiefe: usize) -> bool {
        tiefe < 256
            && liste.iter().any(|k| match k {
                HtmlNode::Element { tag, children, .. } => {
                    (tag.eq_ignore_ascii_case("style")
                        && children.iter().any(|c| matches!(c, HtmlNode::Text(t) if t.to_ascii_lowercase().contains("@import"))))
                        || hat_import(children, tiefe + 1)
                }
                HtmlNode::Text(_) => false,
                #[allow(unreachable_patterns)]
                _ => false,
            })
    }
    // Der Text des `<style>` bekommt die eingesetzten Blaetter (je durch
    // `BLATT_GRENZE` getrennt, also einzeln gelesen). KEINE neuen Elemente:
    // an den Ordinalzahlen der Elemente haengen Skript und Plan.
    fn umbauen(liste: &[HtmlNode], basis: &str, tiefe: usize) -> Vec<HtmlNode> {
        liste
            .iter()
            .map(|k| match k {
                HtmlNode::Element { tag, attributes, children, shadow_root } if tiefe < 256 => {
                    let kinder = if tag.eq_ignore_ascii_case("style") {
                        let text: String = children
                            .iter()
                            .filter_map(|c| match c {
                                HtmlNode::Text(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect();
                        if text.to_ascii_lowercase().contains("@import") {
                            vec![HtmlNode::Text(import_ersetzen(&text, basis, 0))]
                        } else {
                            children.clone()
                        }
                    } else {
                        umbauen(children, basis, tiefe + 1)
                    };
                    HtmlNode::Element {
                        tag: tag.clone(),
                        attributes: attributes.clone(),
                        children: kinder,
                        shadow_root: shadow_root.clone(),
                    }
                }
                _ => k.clone(),
            })
            .collect()
    }
    if hat_import(&r.baum, 0) {
        let basis = r.basis.clone();
        r.baum = Arc::new(umbauen(&r.baum, &basis, 0));
    }
    // Release 12.10.2026 (Agent C): implizites `@scope` (`crate::scope_implizit`),
    // nach den eingesetzten Blaettern (`@import … scope()`).
    if crate::scope_implizit::kommt_vor(&r.baum) {
        let mut baum = (*r.baum).clone();
        if crate::scope_implizit::umschreiben(&mut baum) {
            r.baum = std::sync::Arc::new(baum);
        }
    }
    // Release 12.10.2026: die bevorzugte Blattmenge (`scope_implizit::titel_auswahl`).
    if hat_titel(&r.baum, 0) {
        let mut baum = (*r.baum).clone();
        crate::scope_implizit::titel_auswahl(&mut baum, None);
        r.baum = std::sync::Arc::new(baum);
    }
    // Release 14.10.2026: `<link disabled>` (`scope_implizit::switch_off_disabled_links`).
    if crate::scope_implizit::has_disabled_link(&r.baum, 0) {
        let mut baum = (*r.baum).clone();
        crate::scope_implizit::switch_off_disabled_links(&mut baum, &|_| false);
        r.baum = std::sync::Arc::new(baum);
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **Nach** dem Abruf: die Antwort wird zu
/// CSS-Text. Wortgleiche Fehlertexte wie in der Zusammenfassung.
fn fertig(url: &str, antwort: &narsil_net::Response, tiefe: usize) -> Result<String, String> {
    if antwort.status.code >= 400 {
        return Err(std::format!("HTTP {}", antwort.status.code));
    }
    // M4: `Content-Length` wird VOR dem Lesen geprueft.
    if antwort.body.len() > MAX_BLAETTER_BYTES {
        return Err(std::format!(
            "{} Byte (erlaubt {})",
            antwort.body.len(),
            MAX_BLAETTER_BYTES
        ));
    }
    // Release 11.10.2026 (Agent CSS2): die Kodierung nach CSS Syntax 3 §3.2
    // (BOM, HTTP-charset, `@charset`, sonst UTF-8). GEMESSEN vorher (WPT
    // `syntax/at-charset-*`): ein Blatt, das nicht gueltiges UTF-8 war
    // (windows-1252, Shift_JIS), fiel als Fehler ganz weg.
    let mut css = css_dekodieren(&antwort.body, antwort.content_type());
    // Release 09.10.2026: relative `url()` gegen die BLATT-Adresse aufloesen.
    // Das Blatt steht danach als `<style>` im Baum, und dort gilt die Seiten-
    // adresse als Basis — Hintergrundbilder und Schriften aus `/css/a.css`
    // mit `url(../img/x.png)` zeigten sonst auf `/img/x.png` der SEITE.
    css = urls_absolut(&css, url);
    // `@import` (Tiefe begrenzt, M3/M5).
    if tiefe < MAX_IMPORT_TIEFE {
        css = import_ersetzen(&css, url, tiefe);
    }
    Ok(css)
}

/// added 03.10.2026, Rolle NETZTEMPO. **`hole_blatt` bleibt** — fuer die
/// `@import`-Verschachtelung, die einen Weg je Ebene braucht und deshalb
/// nicht in die gemeinsame Runde passt.
///
/// DER GRUND, warum es nicht auch in die Runde geht: ein `@import` steht
/// **im** CSS eines anderen Blattes, das man erst **holen** muss. Ein zweiter
/// Durchlauf durch `hole_alle` wuerde die Runde von einer zu N machen und
/// braeuchte eine eigene Warteschlange — bei `MAX_IMPORT_TIEFE` 3 und in der
/// Messung **null** `@import`-Faellen unter den zehn Referenzseiten waere das
/// Aufwand ohne Beleg. Deshalb der alte Weg, unveraendert, fuer die Tiefe.
/// Release 12.10.2026: EIN Blatt nachholen — fuer ein `<link>`, das erst
/// ein Skript eingefuegt hat (siehe `Sitzung::blaetter_nachladen_takt`).
/// Derselbe Weg wie beim Laden: `data:`, `file://` nur aus `file://`-Seiten,
/// sonst das Netz mit den Pruefungen von `fertig`.
pub(crate) fn blatt_nachholen(url: &str, basis: &str) -> Result<String, String> {
    if url.starts_with("data:") {
        let bytes = crate::bilder_daten::lesen(url).map_err(|e| e.to_string())?;
        return Ok(css_dekodieren(&bytes, None));
    }
    if let Some(rest) = url.strip_prefix("file://") {
        if !basis.starts_with("file://") {
            return Err("lokale Datei in einer Seite aus dem Netz".to_string());
        }
        let pfad = crate::laden::dateipfad(rest).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&pfad).map_err(|e| std::format!("{pfad}: {e}"))?;
        if meta.len() as usize > MAX_SUMME_BYTES {
            return Err(std::format!("zu gross: {} Byte", meta.len()));
        }
        let bytes = std::fs::read(&pfad).map_err(|e| std::format!("{pfad}: {e}"))?;
        return Ok(css_dekodieren(&bytes, None));
    }
    hole_blatt(url, 0)
}

fn hole_blatt(url: &str, tiefe: usize) -> Result<String, String> {
    if tiefe > MAX_IMPORT_TIEFE {
        return Err(format!("@import tiefer als {MAX_IMPORT_TIEFE}"));
    }
    let anfrage = vorbereite(url)?;
    let antwort = narsil_net::fetch(&anfrage).map_err(|e| e.to_string())?;
    fertig(url, &antwort, tiefe)
}

/// added 03.10.2026, Rolle NETZTEMPO. Die volle Adresse zu einer Aufgabe —
/// fuer Aufrufe, die den **Pfad** nicht brauchen.
///
/// DER WEG, und warum sie trotz des Wegs bei 2c stehen bleibt: hier wird
/// sie aus der Anfrage **zusammengesetzt**, dort kommt sie aus `links`.
/// Beide muessen dasselbe ergeben — sonst findet `setze_stile` den Knoten
/// nicht. Der Zusammenbau ist der Fall „der Aufrufer hat nur eine
/// `Request`"; fuer `setze_stile` ist der Original-String die richtige
/// Quelle, weil er genau der ist, gegen den der HTML-Knoten aufgeloest
/// wurde.
#[allow(dead_code)]
fn url_aus(aufgaben: &[narsil_net::parallel::Aufgabe], i: usize) -> String {
    aufgaben
        .get(i)
        .map(|a| {
            std::format!(
                "{}://{}:{}{}",
                if a.anfrage.https.unwrap_or(false) {
                    "https"
                } else {
                    "http"
                },
                a.anfrage.host,
                a.anfrage.port,
                a.anfrage.path
            )
        })
        .unwrap_or_default()
}

/// Die `@import`-Zeilen durch den Inhalt ersetzen.
///
/// GEMESSEN, warum `MAX_IMPORT_TIEFE` (3): ein Blatt, das sich selbst
/// einbindet, waere eine Endlosschleife. Drei Ebenen reichen fuer echte
/// Seiten (bootstrap.css → Rebuz.info → _ reboot.scss).
/// Release 09.10.2026: `url(...)` in einem geholten Blatt absolut machen
/// (gegen die Adresse des Blattes). `data:`, absolute und Fragment-Verweise
/// (`#id`, SVG-Filter) bleiben, wie sie sind. M3: die Schleife laeuft ueber
/// die Eingabe, jeder Schritt verbraucht mindestens ein Zeichen.
pub(crate) fn urls_absolut(css: &str, basis: &str) -> String {
    let bytes = css.as_bytes();
    let mut raus = String::with_capacity(css.len() + css.len() / 16);
    let mut i = 0usize;
    let mut kopiert = 0usize;
    while i + 4 <= bytes.len() {
        let ist_url = bytes[i..i + 4].eq_ignore_ascii_case(b"url(")
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-' || bytes[i - 1] == b'_'));
        if !ist_url {
            i += 1;
            continue;
        }
        let start = i + 4;
        let Some(ende_rel) = css[start..].find(')') else { break };
        let ende = start + ende_rel;
        let roh = css[start..ende].trim();
        let (inhalt, _quote) = if roh.len() >= 2
            && ((roh.starts_with('"') && roh.ends_with('"')) || (roh.starts_with('\'') && roh.ends_with('\'')))
        {
            (&roh[1..roh.len() - 1], true)
        } else {
            (roh, false)
        };
        let klein = inhalt.trim_start().to_ascii_lowercase();
        let bleibt = inhalt.is_empty()
            || klein.starts_with("data:")
            || klein.starts_with("http://")
            || klein.starts_with("https://")
            || klein.starts_with("//")
            || klein.starts_with('#')
            || klein.starts_with("about:")
            || klein.starts_with("blob:")
            || inhalt.contains('"');
        if !bleibt && let Ok(absolut) = narsil_url::loese_auf(basis, inhalt.trim()) {
            raus.push_str(&css[kopiert..i]);
            raus.push_str("url(\"");
            raus.push_str(&absolut);
            raus.push_str("\")");
            kopiert = ende + 1;
        }
        i = ende + 1;
    }
    raus.push_str(&css[kopiert..]);
    raus
}

/// `scope(<kopf>)` am Anfang des Rests einer `@import`-Anweisung: (Kopf ohne
/// die aeusseren Klammern, uebrige Medienliste).
fn import_bereich(medien: &str) -> Option<(String, String)> {
    let m = medien.trim_start();
    if !m.get(..6).is_some_and(|s| s.eq_ignore_ascii_case("scope(")) {
        return None;
    }
    let z: Vec<char> = m.chars().collect();
    let mut tiefe = 0usize;
    for (i, &c) in z.iter().enumerate().skip(5) {
        match c {
            '(' => tiefe += 1,
            ')' => {
                tiefe -= 1;
                if tiefe == 0 {
                    let kopf: String = z[6..i].iter().collect();
                    let rest: String = z[i + 1..].iter().collect();
                    let rest = rest.trim().to_string();
                    let rest = if rest.eq_ignore_ascii_case("all") { String::new() } else { rest };
                    return Some((kopf.trim().to_string(), rest));
                }
            }
            _ => {}
        }
    }
    None
}

fn import_ersetzen(css: &str, basis_url: &str, tiefe: usize) -> String {
    let (blaetter, rest) = importe_lesen(css, basis_url, tiefe);
    let mut raus = String::with_capacity(css.len());
    let eingesetzt = !blaetter.is_empty();
    for (inhalt, medien) in blaetter {
        // Release 12.10.2026 (Agent C): `@import … scope(<kopf>)` (CSS Cascade 6
        // §2.5.4): das Blatt als `@scope <kopf> { … }`, ohne Kopf implizit.
        // GEMESSEN vorher (WPT css-cascade scope-import-*): das Blatt galt
        // ueberall bzw. gar nicht (`scope(...)` als Medienliste).
        let (inhalt, medien) = match import_bereich(&medien) {
            Some((kopf, rest)) => (format!("@scope {kopf} {{\n{inhalt}\n}}\n"), rest),
            None => (inhalt, medien),
        };
        // Release 11.10.2026 (Agent CSS2): mit Medienliste gilt das Blatt
        // nur dort (CSS Cascade 4 §2) — als `@media`-Block eingesetzt.
        // GEMESSEN vorher (WPT `cascade/at-import-004`,
        // `media-dependency-008`): `print` galt auch am Bildschirm.
        if medien.is_empty() {
            raus.push_str(&inhalt);
        } else {
            raus.push_str("@media ");
            raus.push_str(&medien);
            raus.push_str(" {\n");
            raus.push_str(&inhalt);
            raus.push_str("\n}\n");
        }
        // Jedes eingesetzte Blatt wird fuer sich gelesen (WPT
        // `syntax/at-charset-029`: ein falsch dekodiertes Blatt verschluckte
        // die Regeln dahinter).
        raus.push_str(narsil_css::BLATT_GRENZE);
    }
    // Release 12.10.2026 (Agent C): als letztes Stueck der UNVERAENDERTE Text
    // (mit seinen `@import`-Anweisungen, die der CSS-Leser nicht laedt) statt
    // des zeilenweise gekuerzten Rests — das CSSOM (`cssRules`, `textContent`)
    // sieht nur dieses Stueck. GEMESSEN vorher (release_1012_cssom::
    // stylesheet_cssom): `@import url(a.css) layer(x) screen; @media …` in EINER
    // Zeile liess das ganze `<style>` leer, `cssRules` war leer.
    if !eingesetzt {
        return css.to_string();
    }
    let _ = rest;
    raus.push_str(css);
    raus
}

/// Release 11.10.2026 (Agent CSS2): die `@import`-Anweisungen eines Blattes
/// lesen und die eingebundenen Blaetter holen — (Inhalt, Medienliste ohne
/// `all`) je Blatt in Reihenfolge, und der Text OHNE die Anweisungen.
/// `@import` gilt nur VOR allen anderen Regeln (ausser `@charset` und
/// `@layer`-Anweisungen; CSS 2.1 §6.3, Cascade 4 §2: sonst ungueltig).
/// GEMESSEN vorher (WPT `syntax/import-000`, `at-rule-006`): ein `@import`
/// hinter einer Regel wurde trotzdem geladen.
/// Release 12.10.2026 (Agent C): eine `@import`-Anweisung ueber mehrere
/// Zeilen (Medienliste auf der naechsten Zeile, Kommentare darin) auf EINE
/// Zeile ohne Kommentare — `importe_lesen` liest zeilenweise. GEMESSEN vorher
/// (WPT css-cascade import-conditional-001/-002): die Medienliste der zweiten
/// Zeile fiel weg, das Blatt galt immer.
fn importe_einzeilig(css: &str) -> std::borrow::Cow<'_, str> {
    if !css.to_ascii_lowercase().contains("@import") {
        return std::borrow::Cow::Borrowed(css);
    }
    let z: Vec<char> = css.chars().collect();
    let mut raus = String::with_capacity(css.len());
    let mut i = 0usize;
    let mut tiefe = 0usize;
    let mut geaendert = false;
    while i < z.len() {
        let c = z[i];
        if c == '/' && z.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            while j + 1 < z.len() && !(z[j] == '*' && z[j + 1] == '/') {
                j += 1;
            }
            let ende = (j + 2).min(z.len());
            raus.extend(&z[i..ende]);
            i = ende;
            continue;
        }
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < z.len() && z[j] != c && z[j] != '\n' {
                if z[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let ende = (j + 1).min(z.len());
            raus.extend(&z[i..ende]);
            i = ende;
            continue;
        }
        match c {
            '{' => tiefe += 1,
            '}' => tiefe = tiefe.saturating_sub(1),
            '@' if tiefe == 0 && z[i..].iter().take(7).collect::<String>().eq_ignore_ascii_case("@import") => {
                // Bis zum `;` auf oberster Ebene; Zeilenumbrueche werden Leerzeichen.
                let mut j = i;
                let mut klammern = 0usize;
                let mut stueck = String::new();
                let mut in_text: Option<char> = None;
                while j < z.len() {
                    let d = z[j];
                    if let Some(q) = in_text {
                        stueck.push(d);
                        if d == q {
                            in_text = None;
                        }
                        j += 1;
                        continue;
                    }
                    if d == '/' && z.get(j + 1) == Some(&'*') {
                        let mut k = j + 2;
                        while k + 1 < z.len() && !(z[k] == '*' && z[k + 1] == '/') {
                            k += 1;
                        }
                        j = (k + 2).min(z.len());
                        stueck.push(' ');
                        geaendert = true;
                        continue;
                    }
                    match d {
                        '"' | '\'' => in_text = Some(d),
                        '(' => klammern += 1,
                        ')' => klammern = klammern.saturating_sub(1),
                        ';' if klammern == 0 => {
                            stueck.push(';');
                            j += 1;
                            break;
                        }
                        '{' | '}' if klammern == 0 => break,
                        '\n' | '\r' => {
                            stueck.push(' ');
                            geaendert = true;
                            j += 1;
                            continue;
                        }
                        _ => {}
                    }
                    stueck.push(d);
                    j += 1;
                }
                raus.push_str(&stueck);
                i = j;
                continue;
            }
            _ => {}
        }
        raus.push(c);
        i += 1;
    }
    if geaendert { std::borrow::Cow::Owned(raus) } else { std::borrow::Cow::Borrowed(css) }
}

fn importe_lesen(css: &str, basis_url: &str, tiefe: usize) -> (Vec<(String, String)>, String) {
    let css_einzeilig = importe_einzeilig(css);
    let css: &str = &css_einzeilig;
    let mut rest = String::with_capacity(css.len());
    let mut blaetter: Vec<(String, String)> = Vec::new();
    // M3: `MAX_IMPORT_ZEILEN` (256) `@import`-Zeilen je Blatt.
    const MAX_IMPORT_ZEILEN: usize = 256;
    let mut gesehen = 0usize;
    // Bisher gelesener Text (fuer die Pruefung, ob davor schon eine GUELTIGE
    // Regel steht — die entscheidet der CSS-Leser selbst: eine unbekannte
    // @-Regel oder ein kaputter Selektor zaehlen nicht, CSS 2.1 §4.2; WPT
    // `syntax/at-rule-008/009`).
    let mut davor = String::new();
    for zeile in css.lines() {
        let k = zeile.trim();
        if !k.starts_with("@import") {
            rest.push_str(zeile);
            rest.push('\n');
            davor.push_str(zeile);
            davor.push('\n');
            continue;
        }
        let blockiert = !davor.trim().is_empty()
            && narsil_css::stylesheet_lesen(&davor).regeln.iter().any(|r| match r {
                narsil_css::Regel::Import { .. } => false,
                narsil_css::Regel::Layer { regeln, .. } => !regeln.is_empty(),
                _ => true,
            });
        // Steht davor ein unfertiger Vorspann (Text ohne `{`, `;` oder `}` am
        // Ende), gehoert die Zeile zu IHM und ist keine @-Regel (WPT
        // `cascade/at-import-011`: `# :unknownpseudo` davor).
        let ohne_kommentare = {
            let mut t = String::with_capacity(davor.len());
            let mut r = davor.as_str();
            while let Some(i) = r.find("/*") {
                t.push_str(&r[..i]);
                r = r[i + 2..].find("*/").map_or("", |j| &r[i + 2 + j + 2..]);
            }
            t.push_str(r);
            t
        };
        let schluss = ohne_kommentare.trim_end();
        let im_vorspann = !(schluss.is_empty()
            || schluss.ends_with('}')
            || schluss.ends_with(';')
            || schluss.ends_with("<!--")
            || schluss.ends_with("-->"));
        if blockiert || im_vorspann {
            // Bleibt Text: der CSS-Leser behandelt die Zeile, wie sie steht.
            rest.push_str(zeile);
            rest.push('\n');
            davor.push_str(zeile);
            davor.push('\n');
            continue;
        }
        gesehen += 1;
        if gesehen > MAX_IMPORT_ZEILEN {
            break;
        }
        // `url("…")` oder `url(…)`.
        // Release 09.10.2026: auch die Zeichenkettenform `@import "a.css";`.
        // Release 11.10.2026 (Agent CSS2): dazu der Rest der Anweisung — die
        // Medienliste (`@import url(a.css) print;`).
        let (url, hinten) = k
            .split_once("url(")
            .and_then(|(_, r)| r.split_once(')'))
            .map(|(u, hinten)| (u.trim().trim_matches(['"', '\'']), hinten))
            .or_else(|| {
                let r = k.trim_start_matches("@import").trim_start();
                let q = r.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                r[1..].split_once(q).map(|(u, hinten)| (u.trim(), hinten))
            })
            .unwrap_or(("", ""));
        if url.is_empty() {
            rest.push_str(zeile);
            rest.push('\n');
            continue;
        }
        let medien = hinten.split(';').next().unwrap_or("").trim();
        let medien = if medien.eq_ignore_ascii_case("all") { "" } else { medien };
        // Die Basis fuer das Blatt ist sein eigener Pfad.
        let absolut = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_string()
        } else {
            narsil_url::loese_auf(basis_url, url).unwrap_or_else(|_| url.to_string())
        };
        if let Ok(inhalt) = hole_blatt(&absolut, tiefe + 1) {
            blaetter.push((inhalt, medien.to_string()));
        }
    }
    (blaetter, rest)
}


#[cfg(test)]
mod tests {
    //! W7B, ABNAHME (Regel 2d): DER EIGENE MUESER.
    //!
    //! GEMESSEN, warum das hier und nicht im Fuzzer: `narsil-fuzz` ist eine
    //! fremde Crate (nicht mein Scope), und seine Ziele kennen meinen Code
    //! nicht — `ziel_html` fuettert **Text** (`from_utf8_lossy`) in den
    //! Parser, `ziel_seite` ruft `narsil_page::aus_html`. Weder
    //! `sammle_links` noch `text_aus_antwort` (der Zeichensatz, der
    //! google.com getoetet hat) laufen dort. Ein Punkt gilt aber erst als
    //! abgenommen, wenn jemand seinen EIGENEN Code mit Zufall trifft —
    //! also laeuft der Mueller hier, deterministisch (xorshift, kein
    //! `rand`, gleiche Saat = gleiches Ergebnis auf jeder Maschine).

    use super::*;

    #[test]
    fn import_ueber_zeilen() {
        let t = importe_einzeilig("@import \"a.css\"\n  (min-width: 1px) and /* x */ (max-width: 4in), nonsense;\np { color: red }");
        assert!(t.starts_with("@import \"a.css\"   (min-width: 1px) and   (max-width: 4in), nonsense;\n"), "{t}");
        assert!(matches!(importe_einzeilig("p { color: red }"), std::borrow::Cow::Borrowed(_)));
    }

    /// Release 09.10.2026: relative `url()` im geholten Blatt zeigen auf die
    /// Adresse des BLATTES, nicht der Seite.
    #[test]
    fn urls_im_blatt_werden_gegen_das_blatt_aufgeloest() {
        let css = "a{background:url(../img/x.png)} @font-face{src:url('f.woff2') format('woff2')}                    c{m:url(data:image/png;base64,AA) url(#f) url(https://x.de/y.png)}";
        let r = urls_absolut(css, "https://s.de/css/a.css");
        assert!(r.contains("url(\"https://s.de/img/x.png\")"), "{r}");
        assert!(r.contains("url(\"https://s.de/css/f.woff2\")"), "{r}");
        assert!(r.contains("url(data:image/png;base64,AA)"), "{r}");
        assert!(r.contains("url(#f)"), "{r}");
        assert!(r.contains("url(https://x.de/y.png)"), "{r}");
    }

    /// xorshift64* — dieselbe Familie wie der Fuzzer, damit die Faelle
    /// „spannend" sind (0, grosse Zahlen, kaputte Bytes).
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
        fn unter(&mut self, n: usize) -> usize {
            (self.n() % n.max(1) as u64) as usize
        }
    }

    /// Wie viele Faelle. GEMESSEN: 400 in < 0,2 s — genug fuer jede
    /// Verzweigung der Baumwanderung, und klein genug fuer jeden
    /// Testlauf (M6: kein Test ohne Grenze).
    const FAELLE: usize = 400;

    /// Die Baender, an denen man scheitern kann: eine URL, die es nicht
    /// gibt (Verbindung verweigert, das ist der billigste Fehler), ein
    /// fremdes Schema, ein leeres `href`, ein Rucksprung, ein Fragment,
    /// und ein Weg, den `narsil_url` nicht aufloesen kann.
    const HREFS: &[&str] = &[
        "a.css",
        "http://127.0.0.1:1/x.css",
        "data:text/css,p%7Bcolor%3Ared%7D",
        "",
        "../../../../../../x.css",
        "#frag",
        "javascript:alert(1)",
        "https://[::1]:1/x.css",
        "http://%00/",
        "a.css?x=1#y",
    ];

    /// DER MUESER: 400 erfundene Seiten mit echten `<link>`-Knoten in
    /// zufaelliger Verschachtelung.
    ///
    /// Behauptet wird dreierlei, und keine davon ist „ist nicht
    /// abgestuerzt":
    /// 1. keine Panik (das ist der Punkt),
    /// 2. `geholt + fehler <= MAX_BLAETTER` — die Grenze haelt auch dann,
    ///    wenn die Seite 200 Links anbietet,
    /// 3. fetched sheets arrive: their CSS is in the sheet store
    ///    (`narsil_page::blattschrank`; links stay links since d0d76c2c),
    ///    otherwise the test would be green and the page without CSS.
    #[test]
    fn mueller_ueber_blaetter_und_vertiefungen() {
        let mut z = Zufall(0x5eed_1234);
        for f in 0..FAELLE {
            // M3: Die Tiefe ist eine Eingabe — 60 ist weit mehr als jede
            // echte Seite (der Parser bricht bei `MAX_BAU_TIEFE` ab).
            let tiefe = 1 + z.unter(60);
            let mut html = String::from("<html><head>");
            let mut offen = 0usize;
            for _ in 0..tiefe {
                match z.unter(4) {
                    0 => {
                        html.push_str("<link rel=\"stylesheet\" href=\"");
                        html.push_str(HREFS[z.unter(HREFS.len())]);
                        html.push_str("\">");
                    }
                    1 => {
                        html.push_str("<link rel=\"");
                        html.push_str(if z.unter(2) == 0 {
                            "print stylesheet"
                        } else {
                            "stylesheet"
                        });
                        html.push_str("\" media=\"print\" href=\"");
                        html.push_str(HREFS[z.unter(HREFS.len())]);
                        html.push_str("\">");
                    }
                    2 => {
                        html.push_str("<div class=\"x\" style=\"color:red\">");
                        offen += 1;
                    }
                    _ => html.push_str("<!-- Kommentar -->"),
                }
            }
            for _ in 0..offen {
                html.push_str("</div>");
            }
            html.push_str("</head><body><p>Text</p></body></html>");

            let mut baum: Arc<Vec<HtmlNode>> = Arc::new(narsil_fmt_html::parse_html(&html));
            let eingang = Arc::as_ptr(&baum);
            let e = blaetter_holen(&baum, "https://127.0.0.1:1/seite/index.html");
            assert!(
                e.geholt + e.fehler.len() <= MAX_BLAETTER,
                "Fall {f}: {} Blaetter und {} Fehler — die Grenze {MAX_BLAETTER} \
                 haelt nicht",
                e.geholt,
                e.fehler.len()
            );
            // Fall 1: 40 Links, alle auf einen geschlossenen Port. Die
            // Grenze muss greifen, und die Seite muss trotzdem dastehen.
            assert!(!e.basis.is_empty(), "Fall {f}: die Basis ging verloren");
            // Fall 2 (Release 14.10.2026, Agent C): since d0d76c2c links stay
            // links and the fetched CSS goes to the sheet store
            // (`narsil_page::blattschrank`), so the tree may be the same `Arc`.
            // Only the `data:` link can succeed here; when something was
            // fetched, its CSS must be in the store.
            let _ = eingang;
            if e.geholt > 0 {
                let geholt = e.geholt;
                assert!(
                    narsil_page::blattschrank::inhalt(HREFS[2]).is_some(),
                    "Fall {f}: {geholt} sheets fetched, but the sheet store does not \
                     hold them — the sheets do not arrive"
                );
            }
            baum = e.baum;
            assert!(!baum.is_empty(), "Fall {f}: der Baum wurde leer");
        }
    }

    /// 40 Links auf einen geschlossenen Port: die GRENZE als Behauptung,
    /// nicht als Zahl in einer Konstanten.
    #[test]
    fn zu_viele_links_werden_gebrochen() {
        let mut html = String::from("<html><head>");
        for _ in 0..40 {
            html.push_str("<link rel=\"stylesheet\" href=\"http://127.0.0.1:1/a.css\">");
        }
        html.push_str("</head><body><p>Text</p></body></html>");
        let baum: Arc<Vec<HtmlNode>> = Arc::new(narsil_fmt_html::parse_html(&html));
        let e = blaetter_holen(&baum, "https://127.0.0.1:1/");
        assert!(
            e.geholt + e.fehler.len() <= MAX_BLAETTER,
            "{} Versuche fuer 40 Links — MAX_BLAETTER={MAX_BLAETTER} greift nicht",
            e.geholt + e.fehler.len()
        );
    }

    /// `@import` darf nicht in einen Kreis laufen: `a.css` importiert
    /// sich ueber `b.css`. GEMESSEN, warum das der teuerste Fehler ist:
    /// ohne `MAX_IMPORT_TIEFE` holt der Browser dieselbe Datei endlos.
    /// Behauptet wird: es kommt zurück, in wenigen Millisekunden.
    #[test]
    fn import_kreis_bricht_bei_der_tiefe() {
        let css = "@import url(b.css);";
        let raus = import_ersetzen(css, "https://127.0.0.1:1/a.css", MAX_IMPORT_TIEFE);
        // Am Ende der erlaubten Tiefe wird der Import nicht mehr
        // geholt, sondern bleibt Text (oder faellt weg) — beides ist
        // erlaubt, ein Endloslauf nicht.
        assert!(
            raus.len() < 1 << 20,
            "der Import ist nicht begrenzt: {} Zeichen",
            raus.len()
        );
        assert!(
            import_ersetzen(css, "https://127.0.0.1:1/a.css", 99).len() < 1 << 20,
            "auch mit ueberzogener Tiefe muss es enden"
        );
    }
}
