//! Adressen lesen und aufloesen, ohne sie aufzurufen.
//!
//! `parse` macht aus einem String eine strukturierte Adresse. Es ruft
//! NICHTS auf — kein Netz, kein Dateisystem. Der Unterschied ist der ganze
//! Auftrag: Ein Parser, der nebenbei schon verbindet, ist kein Parser.
//!
//! Regeln (BACKLOG 0060, gemessen in tests/adressen.rs):
//! * Kein Schema heisst localhost (`localhost:8080/seite` ist gueltig).
//! * `https` wird ERKANNT und ehrlich abgelehnt (ADR-046 TLS-Mauer), nicht
//!   als "unbekannt" verkleidet — der Fehler nennt TLS beim Namen.
//! * Pfad ohne `/` bekommt ihn; leerer Pfad wird `/`.
//! * Leerer String ist ein Fehler, kein "aktuelle Seite".
//! * Leerzeichen aussen weg, innen ein Fehler im Host.
//! * Host kleingeschrieben (case-insensitiv), Pfad unveraendert.
//! * IPv6, Userinfo (`user@`) und IDNA gibt es nicht (bewusste Luecken,
//!   dokumentiert statt halb gebaut).
//!
//! WELLE 2 (NETZ, 29.09.2026) — was dazukam und warum:
//! * `file://` und `narsil://` werden als Schema ERKANNT statt als
//!   "unbekanntes Schema" abgewiesen. Der Browser benutzt beide
//!   (GEMESSEN in crates/narsil/src/sitzung.rs::waehle_eingabe: es
//!   schreibt `file://` selbst in die Adresszeile). Wer sie ablehnt,
//!   verhindert dem Benutzer das Laden einer lokalen Seite.
//! * `loese_auf` macht die relative Aufloesung nach RFC 3986 §5. Das ist
//!   die Regel, die JEDER Link braucht, und `narsil-page::loese_link`
//!   macht heute etwas Eigenes — der Vergleich steht in BERICHT_NETZ.txt.
//! * `prozent_dekodieren` und `plus_als_leerzeichen`: `%C3%A4` ist ein `ä`
//!   und `+` in einer Query ist ein Leerzeichen. Ohne das zeigt ein
//!   Browser Umlaute als `%C3%A4` an.
//! * Ports: `:8080` ist ein Port, `a:b` in einem Pfad NICHT. Die Regel
//!   unterscheidet die beiden an der Position des Doppelpunkts.

/// Schema einer Adresse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// `http://` — ladbar, sobald narsil-net file/http kann.
    Http,
    /// `https://` — erkannt, aber TLS fehlt (ADR-046); parse lehnt ab.
    Https,
    /// Kein Schema — localhost (eigene Maschine zuerst).
    Localhost,
}

/// Was der Browser aus einer Adresse machen kann: `http` und `localhost`
/// gehen ins Netz, `file` und `narsil` nicht.
///
/// WARUM ein ZWEITES Schema und nicht mehr Varianten in [`Scheme`]:
/// GEMESSEN am 29.09.2026 mit
///   grep -rn "narsil_url" --include=*.rs crates | grep -v narsil-url
/// steht `narsil-app/src/main.rs` (Zeilen 514/536/553) auf einem
/// ERSCHOELFENDEN `match` ueber `Scheme` — ohne `_`-Zweig. Ein
/// weiteres `Scheme` wuerde diese fremde Crate nicht mehr bauen lassen,
/// und R7 verbietet mir, sie zu reparieren. Also: `Scheme` bleibt, wo
/// es ist, und die neuen Faelle kommen in [`Ziel`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ziel {
    /// Ueber HTTP laden. `adresse` sagt, wohin.
    Netz { adresse: Address },
    /// `file:///pfad` — lokale Datei. Der Pfad steht hier, der Host
    /// ist leer. ERKANNT und gemeldet, nicht verboten: `narsil` schreibt
    /// `file://` selbst in die Adresszeile (GEMESSEN 29.09.2026 in
    /// crates/narsil/src/sitzung.rs Zeile 506).
    Datei { pfad: String },
    /// `narsil://start` — mitgelieferte Seite. Kein Netz, kein
    /// Dateisystem: der Browser entscheidet anhand des Namens.
    Narsil { name: String },
    /// `about:blank` und Verwandte: eingebaute Seite ohne Datei.
    Ueber { schema: String, inhalt: String },
}

/// Strukturierte Adresse ohne Netz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// Welches Schema (bzw. keines).
    pub scheme: Scheme,
    /// Kleingeschriebener Host ohne Port. Bei IPv6 OHNE Klammern
    /// (`::1`) — die Klammern sind Syntax, nicht Hostname.
    pub host: String,
    /// Port, falls angegeben (`None` heisst: nicht angegeben, kein Default).
    pub port: Option<u16>,
    /// Pfad immer mit fuehrendem `/` (Query/Fragment bleiben drin).
    pub path: String,
}

impl Address {
    /// Host und Port so, wie sie in einen HTTP-Kopf gehoeren: mit
    /// Klammern um eine IPv6-Adresse, mit Port ausser er ist nicht 80.
    ///
    /// GEMESSEN am 29.09.2026: `Request::to_bytes` baute den `Host`-Kopf
    /// aus zwei `format!`-Aufrufen. Bei IPv6 kam `Host: ::1:8080` heraus —
    /// nach RFC 3986 §3.2.2 ein KAPUTTER Kopf, weil nicht erkennbar ist,
    /// wo der Host endet. Der Server antwortet dann 400, und der Benutzer
    /// sieht „Server antwortet nicht".
    pub fn host_mit_port(&self) -> String {
        let port = self.port.unwrap_or(80);
        if port == 80 {
            if self.host.contains(':') {
                format!("[{}]", self.host)
            } else {
                self.host.clone()
            }
        } else if self.host.contains(':') {
            format!("[{}]:{}", self.host, port)
        } else {
            format!("{}:{}", self.host, port)
        }
    }
}

/// Fehler beim Adresslesen, alle deutsch mit konkretem Wert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    /// Leere Eingabe (auch nur Leerzeichen).
    Empty,
    /// Schema weder http/https/noetig-los (`ftp://...`).
    UnknownScheme(String),
    /// `https` erkannt — TLS fehlt (ADR-046), ehrlich statt "unbekannt".
    TlsNotSupported,
    /// Host unbrauchbar (leer, Leerzeichen innen, `@`, `[`).
    InvalidHost(String),
    /// Port keine Zahl oder > 65535.
    InvalidPort(String),
    /// Basis und Referenz passen nicht zusammen (z. B. leere Basis bei
    /// relativer Referenz). Ohne diese Meldung liefert die Aufloesung
    /// still eine falsche Adresse.
    KeineBasis { referenz: String },
}

impl std::fmt::Display for UrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UrlError::Empty => write!(f, "leere Adresse: kein Ziel angegeben"),
            UrlError::UnknownScheme(s) => {
                write!(f, "unbekanntes Schema: {s:?} (nur http, https, oder keins)")
            }
            UrlError::TlsNotSupported => write!(
                f,
                "https erkannt, aber TLS wird noch nicht unterstuetzt (ADR-046)"
            ),
            UrlError::InvalidHost(h) => write!(f, "ungueltiger Host: {h:?}"),
            UrlError::InvalidPort(p) => write!(f, "ungueltiger Port: {p:?}"),
            UrlError::KeineBasis { referenz } => write!(
                f,
                "relative Adresse {referenz:?} ohne gueltige Basis zum Aufloesen"
            ),
        }
    }
}

impl std::error::Error for UrlError {}

/// Liest eine Adresse, ruft nichts auf.
///
/// Ablauf: trimmen -> Schema trennen -> Host/Port/Pfad trennen -> pruefen.
/// Kein Panic-Pfad: Jede ungueltige Stelle gibt `UrlError` mit Wert.
pub fn parse(input: &str) -> Result<Address, UrlError> {
    let text = input.trim_matches(|c: char| c.is_whitespace());
    if text.is_empty() {
        return Err(UrlError::Empty);
    }
    let (scheme, rest) = split_scheme(text)?;
    if scheme == Scheme::Https {
        return Err(UrlError::TlsNotSupported);
    }
    // GEMESSENE REGEL, 29.09.2026: `host:8080` hat einen Port,
    // `host/pfad:mit:doppelpunkt` hat KEINEN. Der Unterschied ist die
    // Position: alles ab dem ersten `/`, `?` oder `#` ist Pfad, nicht
    // Host. Die alte Fassung teilte NUR am `/` und machte aus
    // `h/a:b` den Host `h/a` mit Port `b` — bei `b` nicht numerisch
    // wurde still Port 80 angenommen und der Pfad war verloren.
    let ende_host = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host_port, path) = (&rest[..ende_host], &rest[ende_host..]);
    let (host, port) = split_host_port(host_port)?;
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    };
    Ok(Address {
        scheme,
        host,
        port,
        path,
    })
}

/// Trennt `schema://rest` oder meldet localhost. Schema case-insensitiv.
fn split_scheme(text: &str) -> Result<(Scheme, &str), UrlError> {
    match text.find("://") {
        None => Ok((Scheme::Localhost, text)),
        Some(i) => {
            let (raw, rest) = (&text[..i], &text[i + 3..]);
            if raw.eq_ignore_ascii_case("http") {
                Ok((Scheme::Http, rest))
            } else if raw.eq_ignore_ascii_case("https") {
                Ok((Scheme::Https, rest))
            } else {
                Err(UrlError::UnknownScheme(raw.to_string()))
            }
        }
    }
}

// -----------------------------------------------------------------------
// WAS DER BROWSER DARAUS MACHT. Neu in Welle 2 (NETZ, 29.09.2026).
// -----------------------------------------------------------------------

/// Ordnet eine Adresse einem Ziel zu: Netz, Datei, mitgeliefert oder
/// eingebaut. Ohne `file://` und `narsil://` kann der Browser keine lokale
/// Seite und keine mitgelieferte Startseite zeigen.
///
/// `https` wird hier NICHT still zu `Netz`: es bleibt der benannte Fall
/// „TLS fehlt" (ADR-046), mit derselben Meldung wie `parse`.
pub fn ziel(input: &str) -> Result<Ziel, UrlError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(UrlError::Empty);
    }
    // `file:` kann mit oder ohne `//` geschrieben werden. Beides kommt in
    // der Praxis vor, und der Unterschied darf keinen Fehler machen.
    if let Some(rest) = text.strip_prefix("file:") {
        let pfad = rest
            .strip_prefix("//")
            .unwrap_or(rest)
            .trim_start_matches('/');
        if pfad.is_empty() {
            return Err(UrlError::InvalidHost(rest.to_string()));
        }
        return Ok(Ziel::Datei {
            // `file:///pfad` und `file://pfad` ergeben beide `/pfad`: ein
            // Dateisystempfad beginnt mit einem Schraegstrich, und
            // `file://` gehoert zum Schema, nicht zum Pfad.
            pfad: format!("/{pfad}"),
        });
    }
    if let Some(rest) = text.strip_prefix("narsil:") {
        let name = rest
            .strip_prefix("//")
            .unwrap_or(rest)
            .trim_start_matches('/');
        if name.is_empty() {
            return Err(UrlError::InvalidHost(rest.to_string()));
        }
        return Ok(Ziel::Narsil {
            name: name.to_string(),
        });
    }
    // `about:`, `data:`, `chrome:` — ein Schema, ein Doppelpunkt, KEIN
    // `://`. Das ist die Form, an der man sie erkennt.
    if let Some(i) = text.find(':') {
        let schema = &text[..i];
        if !schema.is_empty()
            && schema
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-')
            && !schema.chars().next().is_some_and(|c| c.is_ascii_digit())
            && !text[i..].starts_with("://")
        {
            return Ok(Ziel::Ueber {
                schema: schema.to_ascii_lowercase(),
                inhalt: text[i + 1..].to_string(),
            });
        }
    }
    // Alles Uebrige ist eine Netz-Adresse; `parse` entscheidet, ob es
    // eine ist (https scheitert dort ehrlich).
    let adresse = parse(text)?;
    Ok(Ziel::Netz { adresse })
}

/// Zerlegt `[::1]:8080` in `::1` und 8080. Fehler sind BENANNT und nennen
/// den konkreten Fehler — eine fehlende Klammer ist ein Tippfehler in
/// der Adresszeile, und der Benutzer soll das sehen, nicht einen
/// DNS-Fehler, der ihm drei Bildschirme spaeter erscheint.
///
/// Was hier abgelehnt wird und warum:
/// * `[::1`  — keine schliessende Klammer. Ohne sie weiss niemand, wo
///   die Adresse endet; `::1:8080` ist eine ANDERE (gueltige) Adresse.
/// * `[]:80` — Klammern ohne Inhalt.
/// * `[h]:80` — kein Doppelpunkt und kein `%` in der Klammer, also kein
///   IPv6-Literal, sondern ein Hostname in Klammern. Kein Server ist so
///   erreichbar.
/// * `[::1]:` — Doppelpunkt ohne Port.
/// * `[::1]x` — Zeichen hinter der Klammer.
fn split_ipv6(text: &str) -> Result<(String, Option<u16>), UrlError> {
    let ende = match text.find(']') {
        Some(i) => i,
        None => {
            return Err(UrlError::InvalidHost(format!(
                "{text:?}: IPv6-Adresse ohne schliessende Klammer"
            )));
        }
    };
    let innen = &text[1..ende];
    if innen.is_empty() {
        return Err(UrlError::InvalidHost(format!(
            "{text:?}: IPv6-Adresse ohne Inhalt"
        )));
    }
    // Ein IPv6-Literal hat mindestens zwei Doppelpunkte (Doppelpunkt-
    // Kompression) ODER eine IPv4-Adresse am Ende (`::ffff:127.0.0.1`)
    // ODER eine Zone (`fe80::1%eth0`). `h` ist keines davon.
    let hat_ipv6_zeichen = innen.contains(':');
    let hat_zone = innen.contains('%');
    if !hat_ipv6_zeichen && !hat_zone {
        return Err(UrlError::InvalidHost(format!(
            "{text:?}: kein IPv6-Literal, sondern ein Hostname in Klammern"
        )));
    }
    if innen.chars().any(|c| c.is_whitespace()) {
        return Err(UrlError::InvalidHost(format!(
            "{text:?}: Leerzeichen in der IPv6-Adresse"
        )));
    }
    let rest = &text[ende + 1..];
    let port = if rest.is_empty() {
        None
    } else if let Some(p) = rest.strip_prefix(':') {
        if p.is_empty() {
            return Err(UrlError::InvalidPort(format!("{p} in {text}")));
        }
        match p.parse::<u32>() {
            Ok(n) if (1..=65535).contains(&n) => Some(n as u16),
            _ => return Err(UrlError::InvalidPort(p.to_string())),
        }
    } else {
        return Err(UrlError::InvalidHost(format!(
            "{text:?}: Zeichen nach der schliessenden Klammer"
        )));
    };
    // IPv6 in HEX, also case-insensitiv — `FE80::1` und `fe80::1` sind
    // dieselbe Adresse. Derselbe Hostnamen wird ebenfalls kleingeschrieben.
    Ok((innen.to_ascii_lowercase(), port))
}

/// Loest `referenz` gegen `basis` auf — die ORIGINAL-Fassung der Regel
/// aus RFC 3986 §5, in der Fassung, die ein Browser braucht.
///
/// Warum das nicht `narsil-page::loese_link` ist: dort steht eine
/// Eigenloesung. Der Vergleich beider Fassungen steht in
/// crates/narsil-net/tests/aufgeloest.rs; das Ergebnis im BERICHT_NETZ.txt.
///
/// Faelle:
/// * absolute URI (`http://x/y`) — gewinnt vollstaendig
/// * `//x/y` — Schema bleibt, Host wechselt
/// * `/pfad` — Wurzel, gleicher Host
/// * `neu`, `seite.html` — letztes Segment ersetzt
/// * `../neu`, `../../neu` — Verzeichnisse zurueck
/// * `./neu` — gleiches Verzeichnis
/// * `?query` — gleicher Pfad, neue Query
/// * `#anker` — gleiche URI, nur Fragment
/// * `""` (leer) — die Basis selbst
/// * `mailto:` / `tel:` — keine Netz-Adresse, wird als `Ziel::Ueber`
///   durchgereicht
/// Loest eine Referenz gegen eine Basis auf (WHATWG-Grundfaelle).
///
/// Abgedeckt: TAB/LF/CR entfernen, Gross-/Kleinschreibung des Schemas,
/// Standardports, Host-Kleinschreibung, Backslash als Schraegstrich bei
/// Sonderfaellen, Prozent-Kodierung in Pfad/Query/Fragment,
/// Netzwerk-Pfad-Referenzen, `?`/`#`, leere Referenz, `..`/`.` und
/// undurchsichtige Pfade fuer Nicht-Sonderfaelle.
pub fn loese_auf(basis: &str, referenz: &str) -> Result<String, UrlError> {
    let basis_teile = Teile::zerlege(basis);
    let eingabe = entferne_tab_und_neu(referenz);
    let eingabe = eingabe
        .trim_matches(|c: char| c <= ' ' || c == '\u{7F}')
        .to_string();

    // 1. Absolute Referenz (eigenes Schema)?
    let eingabe = if let Some((schema, rest)) = schema_trennen(&eingabe) {
        // Sonderfall-Schema gleich dem Basis-Schema ohne `//`: das ist
        // eine RELATIVE Referenz (`http:foo.com` gegen `http://...`).
        let relativ_zu_basis = ist_sonderfall(&schema)
            && basis_teile
                .as_ref()
                .is_some_and(|t| t.schema == schema)
            && !rest.starts_with("//");
        if relativ_zu_basis {
            rest.to_string()
        } else {
            return absolut_serialisieren(&schema, rest);
        }
    } else {
        eingabe
    };

    // 2. Ohne Basis gibt es keine Aufloesung.
    let basis_teile = match basis_teile {
        Some(t) => t,
        None => {
            return Err(UrlError::KeineBasis {
                referenz: referenz.to_string(),
            });
        }
    };
    let sonder = ist_sonderfall(&basis_teile.schema);

    // 2b. `file:`-Basis mit Laufwerksbuchstaben-Referenz (`C|/foo`).
    if basis_teile.schema == "file" {
        let z: Vec<char> = eingabe.chars().collect();
        if z.len() >= 2
            && z[0].is_ascii_alphabetic()
            && (z[1] == '|' || z[1] == ':')
            && (z.len() == 2 || matches!(z[2], '/' | '\\' | '?' | '#'))
        {
            let rest: String = z[2..].iter().collect();
            let (p, q, f) = zerlege_rest(&rest, true);
            let p = pfad_absolut(&format!("/{}:{p}", z[0]), true);
            return Ok(serialisiere(
                "file",
                Some(&basis_teile.autoritaet),
                &p,
                q,
                f,
            ));
        }
        // Gegen eine undurchsichtige Basis (Pfad ohne `/`) kann keine
        // relative Referenz aufgeloest werden.
        if !basis_teile.hat_autoritaet && !basis_teile.pfad.starts_with('/') {
            let rest_zeichen: Vec<char> = eingabe.chars().collect();
            let hat_pfad = rest_zeichen
                .first()
                .is_some_and(|c| !matches!(c, '#'));
            if hat_pfad {
                return Err(UrlError::KeineBasis {
                    referenz: referenz.to_string(),
                });
            }
        }
    } else if !basis_teile.hat_autoritaet && !basis_teile.pfad.starts_with('/') {
        let rest_zeichen: Vec<char> = eingabe.chars().collect();
        let hat_pfad = rest_zeichen
            .first()
            .is_some_and(|c| !matches!(c, '#'));
        if hat_pfad {
            return Err(UrlError::KeineBasis {
                referenz: referenz.to_string(),
            });
        }
    }

    // 3. Netzwerk-Pfad `//host/pfad` (auch `/\` und `\/` bei Sonderfaellen).
    let zeichen: Vec<char> = eingabe.chars().collect();
    let netz = eingabe.starts_with("//")
        || (sonder
            && zeichen.len() >= 2
            && matches!(zeichen[0], '/' | '\\')
            && matches!(zeichen[1], '/' | '\\'));
    if netz {
        let mut ohne = &eingabe[2..];
        // Sonderfaelle (ausser file): weitere Schraegstriche gehoeren
        // zum Autoritaetsanfang (`///test` = Host `test`).
        if sonder && basis_teile.schema != "file" {
            while let Some(r) = ohne
                .strip_prefix('/')
                .or_else(|| ohne.strip_prefix('\\'))
            {
                ohne = r;
            }
        }
        let (autoritaet, pfad, query, fragment) = teile_referenz(ohne, sonder);
        if basis_teile.schema == "file" {
            if let Some((laufwerk, rest_pfad)) = laufwerk_host(autoritaet) {
                let p = pfad_absolut(&format!("/{laufwerk}:{rest_pfad}{pfad}"), true);
                return Ok(serialisiere(
                    "file",
                    Some(&leere_autoritaet()),
                    &p,
                    query,
                    fragment,
                ));
            }
        }
        let autoritaet = autoritaet_lesen(&basis_teile.schema, autoritaet)?;
        return Ok(serialisiere(
            &basis_teile.schema,
            Some(&autoritaet),
            &pfad_absolut(&pfad, sonder),
            query,
            fragment,
        ));
    }

    // 4. Alles ab `?` oder `#` abtrennen.
    let (ref_pfad, query, fragment) = zerlege_rest(&eingabe, sonder);

    if ref_pfad.is_empty() {
        // `?query`/`#fragment`/leer: Pfad der Basis bleibt.
        let autoritaet = if basis_teile.hat_autoritaet {
            Some(&basis_teile.autoritaet)
        } else {
            None
        };
        return Ok(serialisiere(
            &basis_teile.schema,
            autoritaet,
            &basis_teile.pfad,
            query.or(basis_teile.query.clone()),
            fragment,
        ));
    }
    if ref_pfad.starts_with('/') || (sonder && ref_pfad.starts_with('\\')) {
        let autoritaet = if basis_teile.hat_autoritaet {
            Some(&basis_teile.autoritaet)
        } else {
            None
        };
        // `file:` mit Laufwerk in der Basis: der Laufwerksbuchstabe
        // bleibt bei absoluten Referenzen erhalten.
        let neuer_pfad = if basis_teile.schema == "file" {
            let ref_b: Vec<char> = ref_pfad.chars().collect();
            let ref_hat_laufwerk = ref_b.len() >= 3
                && matches!(ref_b[0], '/' | '\\')
                && ref_b[1].is_ascii_alphabetic()
                && matches!(ref_b[2], ':' | '|');
            if let Some(laufwerk) = laufwerk_aus_pfad(&basis_teile.pfad) {
                if ref_hat_laufwerk {
                    // Referenz bringt ihr eigenes Laufwerk mit.
                    pfad_absolut(&ref_pfad, sonder)
                } else {
                    format!("{laufwerk}{}", pfad_absolut(&ref_pfad, sonder))
                }
            } else {
                pfad_absolut(&ref_pfad, sonder)
            }
        } else {
            pfad_absolut(&ref_pfad, sonder)
        };
        return Ok(serialisiere(
            &basis_teile.schema,
            autoritaet,
            &neuer_pfad,
            query,
            fragment,
        ));
    }

    // 5. Relativer Pfad: mit dem Basisverzeichnis verschmelzen.
    let verzeichnis = match basis_teile.pfad.rfind('/') {
        Some(i) => &basis_teile.pfad[..=i],
        None => "/",
    };
    let zusammengesetzt = format!("{verzeichnis}{ref_pfad}");
    let autoritaet = if basis_teile.hat_autoritaet {
        Some(&basis_teile.autoritaet)
    } else {
        None
    };
    Ok(serialisiere(
        &basis_teile.schema,
        autoritaet,
        &pfad_absolut(&zusammengesetzt, sonder),
        query,
        fragment,
    ))
}

/// Zerlegte Basis-URL.
#[derive(Debug, Clone)]
struct Teile {
    schema: String,
    autoritaet: Autoritaet,
    pfad: String,
    query: Option<String>,
    /// Hat die Basis eine Autoritaet (`//`)? Opaque Basen nicht.
    hat_autoritaet: bool,
}

impl Teile {
    fn zerlege(basis: &str) -> Option<Teile> {
        let basis = entferne_tab_und_neu(basis);
        let basis = basis.trim();
        if basis.is_empty() {
            return None;
        }
        let (schema, rest) = schema_trennen(basis).unwrap_or(("http".to_string(), basis));
        let sonder = ist_sonderfall(&schema);
        if !rest.starts_with("//") && !(sonder && rest.starts_with("\\\\")) {
            // Opaque Basis (z. B. `mailto:x@x.com`, `data:,`).
            if !sonder {
                let pfad = if rest.starts_with('/') {
                    pfad_absolut(rest, false)
                } else {
                    rest.to_string()
                };
                return Some(Teile {
                    schema,
                    autoritaet: leere_autoritaet(),
                    pfad,
                    query: None,
                    hat_autoritaet: false,
                });
            }
            return None;
        }
        let rest = &rest[2..];
        let (autoritaet_roh, pfad, query, _fragment) = teile_referenz(rest, sonder);
        let autoritaet = autoritaet_lesen(&schema, autoritaet_roh).ok()?;
        let pfad = if pfad.is_empty() {
            if sonder {
                "/".to_string()
            } else {
                String::new()
            }
        } else {
            pfad_absolut(&pfad, sonder)
        };
        Some(Teile {
            schema,
            autoritaet,
            pfad,
            query,
            hat_autoritaet: true,
        })
    }
}

/// Zerlegt eine Autoritaet samt Rest in (Autoritaet, Pfad, Query, Fragment).
fn teile_referenz(
    rest: &str,
    sonder: bool,
) -> (&str, String, Option<String>, Option<String>) {
    // Autoritaet endet beim ersten `/`, `\` (nur Sonderfall), `?`, `#`.
    let mut ende = rest.len();
    for (i, c) in rest.char_indices() {
        let grenze = c == '/' || c == '?' || c == '#' || (sonder && c == '\\');
        if grenze {
            ende = i;
            break;
        }
    }
    let autoritaet = &rest[..ende];
    let (pfad, query, fragment) = zerlege_rest(&rest[ende..], sonder);
    (autoritaet, pfad, query, fragment)
}

/// Zerlegt `pfad?query#fragment` (in dieser Reihenfolge).
fn zerlege_rest(rest: &str, _sonder: bool) -> (String, Option<String>, Option<String>) {
    let (ohne_fragment, fragment) = match rest.find('#') {
        Some(i) => (&rest[..i], Some(rest[i + 1..].to_string())),
        None => (rest, None),
    };
    let (pfad, query) = match ohne_fragment.find('?') {
        Some(i) => (&ohne_fragment[..i], Some(ohne_fragment[i + 1..].to_string())),
        None => (ohne_fragment, None),
    };
    (pfad.to_string(), query, fragment)
}

/// Eine geparste Autoritaet.
#[derive(Debug, Clone)]
struct Autoritaet {
    /// Benutzerinfo inklusive `@`, leer wenn keine.
    benutzer: String,
    host: String,
    port: Option<u16>,
}

/// Liest `user:pass@host:port`.
fn autoritaet_lesen(schema: &str, roh: &str) -> Result<Autoritaet, UrlError> {
    let sonder = ist_sonderfall(schema);
    // Benutzerinfo: alles bis zum LETZTEN `@`; Benutzer und Passwort
    // werden getrennt kodiert (das `:` dazwischen ist Trenner, kein
    // Zeichen).
    let (benutzer, host_port) = match roh.rfind('@') {
        Some(i) => {
            let raw = &roh[..i];
            let (u, p) = match raw.split_once(':') {
                Some((u, p)) => (u, Some(p)),
                None => (raw, None),
            };
            let mut b = prozent_kodieren(u, Kodierart::Benutzer);
            if let Some(p) = p {
                if !p.is_empty() {
                    b.push(':');
                    b.push_str(&prozent_kodieren(p, Kodierart::Benutzer));
                }
            }
            if !b.is_empty() {
                b.push('@');
            }
            (b, &roh[i + 1..])
        }
        None => (String::new(), roh),
    };
    // IPv6 in Klammern.
    let (host, port) = if let Some(ende) = host_port.find(']') {
        let host = &host_port[..=ende];
        let rest = &host_port[ende + 1..];
        if rest.is_empty() {
            (host.to_string(), None)
        } else if let Some(p) = rest.strip_prefix(':') {
            (host.to_string(), Some(p.to_string()))
        } else {
            return Err(UrlError::InvalidHost(host_port.to_string()));
        }
    } else {
        match host_port.rfind(':') {
            Some(i) => (
                host_port[..i].to_string(),
                Some(host_port[i + 1..].to_string()),
            ),
            None => (host_port.to_string(), None),
        }
    };
    let host = host_lesen(schema, &host)?;
    let port = match port {
        None => None,
        Some(p) if p.is_empty() => None,
        Some(p) => {
            if !p.chars().all(|c| c.is_ascii_digit()) {
                return Err(UrlError::InvalidPort(p));
            }
            match p.parse::<u32>() {
                Ok(n) if n <= 65535 => {
                    let n = n as u16;
                    if sonder && n == standard_port(schema) {
                        None
                    } else {
                        Some(n)
                    }
                }
                _ => return Err(UrlError::InvalidPort(p)),
            }
        }
    };
    // `file:` kennt keinen Port.
    if schema == "file" && port.is_some() {
        return Err(UrlError::InvalidPort(port.unwrap().to_string()));
    }
    // Nicht-Sonderfall mit leerem Host: Autoritaetszeichen sind ein
    // Fehler (`sc://@/`, `sc://:/`).
    if !sonder && host.is_empty() && (roh.contains('@') || roh.contains(':')) {
        return Err(UrlError::InvalidHost(roh.to_string()));
    }
    if sonder && host.is_empty() && schema != "file" {
        return Err(UrlError::InvalidHost(roh.to_string()));
    }
    Ok(Autoritaet {
        benutzer,
        host,
        port,
    })
}

/// Prueft und normalisiert den Host.
fn host_lesen(schema: &str, host: &str) -> Result<String, UrlError> {
    let sonder = ist_sonderfall(schema);
    if host.is_empty() {
        return Ok(String::new());
    }
    // IPv6-Klammern werden NICHT prozent-dekodiert und VOR allem
    // anderen erkannt.
    if host.starts_with('[') {
        if !host.ends_with(']') {
            return Err(UrlError::InvalidHost(host.to_string()));
        }
        let innen = &host[1..host.len() - 1];
        let normal = ipv6_normalisieren(innen)
            .ok_or_else(|| UrlError::InvalidHost(host.to_string()))?;
        return Ok(format!("[{normal}]"));
    }
    // Undurchsichtiger Host (Nicht-Sonderfall): keine IDNA, keine
    // Dekodierung; C0/Leerzeichen/nicht-ASCII werden prozent-kodiert,
    // nur die verbotenen Zeichen sind ein Fehler.
    if !sonder {
        let verboten = |c: char| {
            matches!(
                c,
                '\0' | '\t' | '\n' | '\r' | ' '
                    | '#' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
            )
        };
        if host.chars().any(verboten) {
            return Err(UrlError::InvalidHost(host.to_string()));
        }
        let mut raus = String::with_capacity(host.len());
        for c in host.chars() {
            if c.is_control() || c.is_whitespace() || (c as u32) > 0x7E {
                for b in c.to_string().as_bytes() {
                    raus.push_str(&format!("%{b:02X}"));
                }
            } else {
                raus.push(c);
            }
        }
        return Ok(raus);
    }
    // Sonderfaelle: `%XX` dekodieren, dann IDNA-Vorbereitung.
    let host_roh = prozent_dekodieren_text(host);
    // IDNA-Vorbereitung: ignorierbare Zeichen weg, Breitzeichen und
    // ideographische Punkte abbilden, Nichtzeichen ablehnen.
    let mut h = String::with_capacity(host_roh.len());
    for c in host_roh.chars() {
        let cp = c as u32;
        if ist_ignorierbar(cp) {
            continue;
        }
        if matches!(cp, 0x3002 | 0xFF0E | 0xFF61) {
            h.push('.');
            continue;
        }
        if (0xFF01..=0xFF5E).contains(&cp) {
            h.push(char::from_u32(cp - 0xFEE0).unwrap_or('\u{FFFD}'));
            continue;
        }
        // Mathematische alphanumerische Zeichen (𝐀, 𝗈 ...) werden zu
        // ASCII (IDNA-Mapping).
        if (0x1D400..=0x1D6A3).contains(&cp) {
            let rel = (cp - 0x1D400) % 52;
            let ascii = if rel < 26 {
                b'A' + rel as u8
            } else {
                b'a' + (rel - 26) as u8
            };
            h.push(ascii as char);
            continue;
        }
        if (0x1D7CE..=0x1D7FF).contains(&cp) {
            let ziffer = ((cp - 0x1D7CE) % 10) as u8;
            h.push((b'0' + ziffer) as char);
            continue;
        }
        if cp == 0x3000 {
            h.push(' ');
            continue;
        }
        if cp == 0xFFFD || (0xFDD0..=0xFDEF).contains(&cp) || (cp & 0xFFFE) == 0xFFFE {
            return Err(UrlError::InvalidHost(host.to_string()));
        }
        h.push(c);
    }
    let host = h.as_str();
    if host.is_empty() {
        // `file:`: ein Host, der nur aus ignorierbaren Zeichen bestand,
        // ist ein Fehler (er WAR nicht leer).
        if schema == "file" && !host_roh.is_empty() {
            return Err(UrlError::InvalidHost(host_roh.clone()));
        }
        return Ok(String::new());
    }
    // Ziffernform? Dann IPv4 (WHATWG erlaubt dezimal, oktal, hex).
    if ist_whatwg_zahlenende(host) {
        return ipv4_parsen(host).map_err(|_| UrlError::InvalidHost(host.to_string()));
    }
    // `file://localhost` ist der lokale Rechner: leerer Host.
    if schema == "file" && host.eq_ignore_ascii_case("localhost") {
        return Ok(String::new());
    }
    let verboten = |c: char| {
        c.is_control()
            || c.is_whitespace()
            || c == '%'
            || matches!(
                c,
                '#' | '/' | '?' | '@' | '[' | ']' | '\\' | '^' | '|' | '<' | '>' | ':'
            )
    };
    if host.chars().any(verboten) {
        return Err(UrlError::InvalidHost(host.to_string()));
    }
    // Nicht-ASCII: IDNA (Punycode) fuer Sonderfaelle.
    if !host.is_ascii() {
        return domain_kodieren(host).map_err(|_| UrlError::InvalidHost(host.to_string()));
    }
    Ok(host.to_ascii_lowercase())
}

/// Dekodiert `%XX` in einem Host (ungueltige Folgen bleiben stehen).
fn prozent_dekodieren_text(text: &str) -> String {
    let b = text.as_bytes();
    let mut raus: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h1 = b[i + 1] as char;
            let h2 = b[i + 2] as char;
            if let (Some(a), Some(c)) = (h1.to_digit(16), h2.to_digit(16)) {
                raus.push((a * 16 + c) as u8);
                i += 3;
                continue;
            }
        }
        raus.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&raus).into_owned()
}

/// Ist der Pfad `/X:/...`? Liefert `/X:`.
fn laufwerk_aus_pfad(pfad: &str) -> Option<String> {    let b: Vec<char> = pfad.chars().collect();
    if b.len() >= 3 && b[0] == '/' && b[1].is_ascii_alphabetic() && b[2] == ':' {
        Some(format!("/{}:", b[1]))
    } else {
        None
    }
}

/// Eine leere Autoritaet (`file:///...`).
fn leere_autoritaet() -> Autoritaet {
    Autoritaet {
        benutzer: String::new(),
        host: String::new(),
        port: None,
    }
}

/// Ist der Host ein Windows-Laufwerksbuchstabe (`C|` oder `C:`)?
fn laufwerk_host(host: &str) -> Option<(char, String)> {
    let mut z = host.chars();
    let erstes = z.next()?;
    let zweites = z.next()?;
    if erstes.is_ascii_alphabetic() && (zweites == '|' || zweites == ':') {
        Some((erstes, z.collect()))
    } else {
        None
    }
}

/// IDNA-ignorierbare Zeichen (Cf und Verwandte): werden entfernt.
fn ist_ignorierbar(cp: u32) -> bool {
    matches!(cp,
        0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5
        | 0x180B..=0x180F | 0x200B..=0x200F | 0x202A..=0x202E
        | 0x2060..=0x2064 | 0x2066..=0x206F | 0x3164 | 0xFE00..=0xFE0F
        | 0xFEFF | 0xFFA0)
}

/// Endet der Host auf eine WHATWG-Zahl? (dezimal, oktal oder `0x`-hex)
fn ist_whatwg_zahlenende(host: &str) -> bool {
    let mut teile: Vec<&str> = host.split('.').collect();
    if teile.last() == Some(&"") {
        teile.pop();
    }
    let letzte = match teile.last() {
        Some(t) if !t.is_empty() => *t,
        _ => return false,
    };
    if letzte.starts_with("0x") || letzte.starts_with("0X") {
        let hex = &letzte[2..];
        return hex.is_empty() || hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    letzte.chars().all(|c| c.is_ascii_digit())
}

/// Eine WHATWG-Zahl (dezimal, oktal mit fuehrender 0, hex mit 0x).
fn whatwg_zahl(s: &str) -> Option<u64> {
    if s.is_empty() {
        return Some(0);
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        if hex.is_empty() {
            return Some(0);
        }
        u64::from_str_radix(hex, 16).ok()
    } else if s.len() > 1 && s.starts_with('0') {
        u64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse().ok()
    }
}

/// IPv4 nach WHATWG: bis zu vier Teile, der letzte fuellt den Rest.
fn ipv4_parsen(host: &str) -> Result<String, ()> {
    let mut teile: Vec<&str> = host.split('.').collect();
    if teile.last() == Some(&"") {
        teile.pop();
    }
    if teile.is_empty() || teile.len() > 4 {
        return Err(());
    }
    let mut werte: Vec<u64> = Vec::with_capacity(teile.len());
    for t in &teile {
        if t.is_empty() {
            return Err(());
        }
        werte.push(whatwg_zahl(t).ok_or(())?);
    }
    let mut ip = [0u8; 4];
    for i in 0..werte.len() - 1 {
        if werte[i] > 255 {
            return Err(());
        }
        ip[i] = werte[i] as u8;
    }
    let letzter = *werte.last().ok_or(())?;
    let rest = 4 - (werte.len() - 1);
    let grenze = 256u64.pow(rest as u32);
    if letzter >= grenze {
        return Err(());
    }
    let mut wert = letzter;
    for i in (werte.len() - 1..4).rev() {
        ip[i] = (wert & 0xFF) as u8;
        wert >>= 8;
    }
    Ok(format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
}

/// Ist das der Inhalt einer IPv6-Klammer? Liefert die normalisierte Form
/// (WHATWG: laengster Null-Lauf wird zu `::`, eingebettete IPv4-Adresse
/// wird hexadezimal, Kleinschreibung).
fn ipv6_normalisieren(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let (links, rechts, hat_doppel) = match s.split_once("::") {
        Some((l, r)) => {
            if r.contains("::") {
                return None;
            }
            (l, r, true)
        }
        None => (s, "", false),
    };
    let mut gruppen: Vec<u16> = Vec::new();
    if !links.is_empty() {
        let link_teile: Vec<&str> = links.split(':').collect();
        for (i, teil) in link_teile.iter().enumerate() {
            if teil.contains('.') {
                if hat_doppel || i != link_teile.len() - 1 {
                    return None;
                }
                let v4: Vec<&str> = teil.split('.').collect();
                if v4.len() != 4 {
                    return None;
                }
                let mut okt = [0u16; 4];
                for (k, t) in v4.iter().enumerate() {
                    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    if t.len() > 1 && t.starts_with('0') {
                        return None; // fuehrende Null im IPv4-Teil
                    }
                    let n: u32 = t.parse().ok()?;
                    if n > 255 {
                        return None;
                    }
                    okt[k] = n as u16;
                }
                gruppen.push(okt[0] * 256 + okt[1]);
                gruppen.push(okt[2] * 256 + okt[3]);
                continue;
            }
            if teil.is_empty() || teil.len() > 4 {
                return None;
            }
            gruppen.push(u16::from_str_radix(teil, 16).ok()?);
        }
    }
    let mut rechts_gruppen: Vec<u16> = Vec::new();
    if !rechts.is_empty() {
        let teile: Vec<&str> = rechts.split(':').collect();
        for (i, teil) in teile.iter().enumerate() {
            if teil.is_empty() {
                return None;
            }
            if teil.contains('.') {
                if i != teile.len() - 1 {
                    return None;
                }
                let v4: Vec<&str> = teil.split('.').collect();
                if v4.len() != 4 {
                    return None;
                }
                let mut okt = [0u16; 4];
                for (k, t) in v4.iter().enumerate() {
                    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    if t.len() > 1 && t.starts_with('0') {
                        return None; // fuehrende Null im IPv4-Teil
                    }
                    let n: u32 = t.parse().ok()?;
                    if n > 255 {
                        return None;
                    }
                    okt[k] = n as u16;
                }
                rechts_gruppen.push(okt[0] * 256 + okt[1]);
                rechts_gruppen.push(okt[2] * 256 + okt[3]);
            } else {
                if teil.len() > 4 {
                    return None;
                }
                rechts_gruppen.push(u16::from_str_radix(teil, 16).ok()?);
            }
        }
    }
    if hat_doppel {
        if gruppen.len() + rechts_gruppen.len() >= 8 {
            return None;
        }
        while gruppen.len() + rechts_gruppen.len() < 8 {
            gruppen.push(0);
        }
    } else if gruppen.len() + rechts_gruppen.len() != 8 {
        return None;
    }
    gruppen.extend(rechts_gruppen);
    // Laengsten Null-Lauf (>= 2) zu `::`.
    let mut best_start = 0usize;
    let mut best_len = 0usize;
    let mut i = 0usize;
    while i < 8 {
        if gruppen[i] == 0 {
            let start = i;
            while i < 8 && gruppen[i] == 0 {
                i += 1;
            }
            if i - start > best_len {
                best_start = start;
                best_len = i - start;
            }
        } else {
            i += 1;
        }
    }
    let mut raus = String::new();
    let mut k = 0usize;
    while k < 8 {
        if best_len >= 2 && k == best_start {
            raus.push_str("::");
            k += best_len;
            continue;
        }
        if k > 0 && !raus.ends_with(':') {
            raus.push(':');
        }
        raus.push_str(&format!("{:x}", gruppen[k]));
        k += 1;
    }
    if raus.is_empty() {
        raus.push_str("::");
    }
    Some(raus)
}

/// Der Standardport eines Sonderfall-Schemas.
fn standard_port(schema: &str) -> u16 {
    match schema {
        "ftp" => 21,
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        _ => 0,
    }
}

/// Ist das Schema ein WHATWG-Sonderfall?
fn ist_sonderfall(schema: &str) -> bool {
    matches!(
        schema,
        "http" | "https" | "ws" | "wss" | "ftp" | "file"
    )
}

/// Trennt ein gueltiges Schema ab (`abc:`).
fn schema_trennen(text: &str) -> Option<(String, &str)> {
    let doppel = text.find(':')?;
    let roh = &text[..doppel];
    if roh.is_empty() {
        return None;
    }
    let mut zeichen = roh.chars();
    let erstes = zeichen.next()?;
    if !erstes.is_ascii_alphabetic() {
        return None;
    }
    if !zeichen.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    Some((roh.to_ascii_lowercase(), &text[doppel + 1..]))
}

/// Entfernt TAB, LF und CR (WHATWG 4.4: vor dem Parsen).
fn entferne_tab_und_neu(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// Baut eine absolute URL und normalisiert sie.
fn absolut_serialisieren(schema: &str, rest: &str) -> Result<String, UrlError> {
    let sonder = ist_sonderfall(schema);
    if sonder {
        // `file:` hat eigene Regeln: NUR `//` beginnt eine Autoritaet,
        // ein einzelner `/` bedeutet leeren Host; `///` ebenso.
        if schema == "file" {
            let b: Vec<char> = rest.chars().collect();
            let zwei = b.len() >= 2 && matches!(b[0], '/' | '\\') && matches!(b[1], '/' | '\\');
            if zwei {
                let rest2 = &rest[2..];
                let b2: Vec<char> = rest2.chars().collect();
                if b2.first().is_some_and(|c| matches!(c, '/' | '\\')) {
                    // Leerer Host, Pfad ab dem zweiten Zeichen.
                    let (p, q, f) = zerlege_rest(rest2, true);
                    let p = pfad_absolut(&p, true);
                    return Ok(serialisiere(schema, Some(&leere_autoritaet()), &p, q, f));
                }
                let (autoritaet, pfad, query, fragment) = teile_referenz(rest2, true);
                if let Some((laufwerk, rest_pfad)) = laufwerk_host(autoritaet) {
                    let p = pfad_absolut(&format!("/{laufwerk}:{rest_pfad}{pfad}"), true);
                    return Ok(serialisiere(
                        schema,
                        Some(&leere_autoritaet()),
                        &p,
                        query,
                        fragment,
                    ));
                }
                let autoritaet = autoritaet_lesen(schema, autoritaet)?;
                let pfad = if pfad.is_empty() {
                    "/".to_string()
                } else {
                    pfad_absolut(&pfad, true)
                };
                return Ok(serialisiere(schema, Some(&autoritaet), &pfad, query, fragment));
            }
            // Ein einzelner Schraegstrich: leerer Host, Rest ist Pfad.
            let pfad = rest.trim_start_matches(['/', '\\']);
            let (pfad, query, fragment) = zerlege_rest(pfad, true);
            let pfad = pfad_absolut(&format!("/{pfad}"), true);
            return Ok(serialisiere(schema, Some(&leere_autoritaet()), &pfad, query, fragment));
        }
        // Beliebige fuehrende `/` und `\` gehoeren zum Schema.
        let mut rest2 = rest;
        while let Some(r) = rest2
            .strip_prefix('/')
            .or_else(|| rest2.strip_prefix('\\'))
        {
            rest2 = r;
        }
        let (autoritaet, pfad, query, fragment) = teile_referenz(rest2, sonder);
        if schema == "file" {
            if let Some((laufwerk, rest_pfad)) = laufwerk_host(autoritaet) {
                let p = pfad_absolut(&format!("/{laufwerk}:{rest_pfad}{pfad}"), true);
                return Ok(serialisiere(schema, Some(&leere_autoritaet()), &p, query, fragment));
            }
        }
        let autoritaet = autoritaet_lesen(schema, autoritaet)?;
        let pfad = if pfad.is_empty() {
            "/".to_string()
        } else {
            pfad_absolut(&pfad, sonder)
        };
        Ok(serialisiere(schema, Some(&autoritaet), &pfad, query, fragment))
    } else if let Some(rest2) = rest.strip_prefix("//") {
        let (autoritaet, pfad, query, fragment) = teile_referenz(rest2, false);
        let autoritaet = autoritaet_lesen(schema, autoritaet)?;
        let pfad = if pfad.is_empty() {
            String::new()
        } else {
            pfad_absolut(&pfad, false)
        };
        Ok(serialisiere(schema, Some(&autoritaet), &pfad, query, fragment))
    } else if rest.starts_with('/') {
        // Hierarchischer Pfad ohne Autoritaet (`non-spec:/pfad`).
        let (pfad, query, fragment) = zerlege_rest(rest, false);
        let pfad = pfad_absolut(&pfad, false);
        Ok(serialisiere_ohne_autoritaet(schema, &pfad, query, fragment))
    } else {
        // Undurchsichtiger Pfad (Leerzeichen bleiben, das LETZTE wird
        // vor `?`/`#`/Ende zu %20).
        let (pfad, query, fragment) = zerlege_rest(rest, false);
        let mut pfad = prozent_kodieren(&pfad, Kodierart::Undurchsichtig);
        if pfad.ends_with(' ') {
            pfad.pop();
            pfad.push_str("%20");
        }
        Ok(serialisiere_ohne_autoritaet(
            schema, &pfad, query, fragment,
        ))
    }
}

/// Pfad normalisieren: Backslash (Sonderfall) zu `/`, Punkte entfernen,
/// fuehrenden `/` erzwingen, Prozent-Kodierung. LEERE SEGMENTE BLEIBEN
/// (`//` ist kein `/`).
fn pfad_absolut(pfad: &str, sonder: bool) -> String {
    let ersetzt: String = if sonder {
        pfad.replace('\\', "/")
    } else {
        pfad.to_string()
    };
    let mut mit_slash = ersetzt;
    if !mit_slash.starts_with('/') {
        mit_slash.insert(0, '/');
    }
    let normalisiert = normalisiere_pfad(&mit_slash);
    prozent_kodieren(&normalisiert, Kodierart::Pfad)
}

/// Serialisiert eine URL; `autoritaet = Some` erzwingt `//`.
fn serialisiere(
    schema: &str,
    autoritaet: Option<&Autoritaet>,
    pfad: &str,
    query: Option<String>,
    fragment: Option<String>,
) -> String {
    let mut raus = String::new();
    raus.push_str(schema);
    raus.push(':');
    if let Some(a) = autoritaet {
        raus.push_str("//");
        raus.push_str(&a.benutzer);
        raus.push_str(&a.host);
        if let Some(p) = a.port {
            raus.push(':');
            raus.push_str(&p.to_string());
        }
    } else if !ist_sonderfall(schema) && pfad.starts_with("//") {
        // Kein Host und ein Pfad, der wie eine Autoritaet aussieht:
        // WHATWG setzt ein `/.'` davor.
        raus.push_str("/.");
    }
    // `file:`: Windows-Laufwerksbuchstabe `C|` wird zu `C:`.
    let pfad = if schema == "file" && pfad.len() >= 3 {        let b: Vec<char> = pfad.chars().collect();
        if b[0] == '/' && b[1].is_ascii_alphabetic() && b[2] == '|' && (b.len() == 3 || b[3] == '/') {
            let mut neu: String = b[..2].iter().collect();
            neu.push(':');
            neu.extend(&b[3..]);
            neu
        } else {
            pfad.to_string()
        }
    } else {
        pfad.to_string()
    };
    raus.push_str(&pfad);
    if let Some(q) = query {
        raus.push('?');
        let art = if ist_sonderfall(schema) {
            Kodierart::Query
        } else {
            Kodierart::QueryNichtSonder
        };
        raus.push_str(&prozent_kodieren(&q, art));
    }
    if let Some(f) = fragment {
        raus.push('#');
        raus.push_str(&prozent_kodieren(&f, Kodierart::Fragment));
    }
    raus
}

/// Serialisiert eine URL ohne Autoritaet (undurchsichtiger Pfad).
fn serialisiere_ohne_autoritaet(
    schema: &str,
    pfad: &str,
    query: Option<String>,
    fragment: Option<String>,
) -> String {
    let mut raus = format!("{schema}:");
    // Nicht-Sonderfall ohne Host: ein Pfad, der mit `//` beginnt, bekommt
    // ein `/.'` davor (WHATWG-Serialisierer), damit er nicht wie eine
    // Autoritaet aussieht.
    if !ist_sonderfall(schema) && pfad.starts_with("//") {
        raus.push_str("/.");
    }
    raus.push_str(pfad);
    if let Some(q) = query {
        raus.push('?');
        let art = if ist_sonderfall(schema) {
            Kodierart::Query
        } else {
            Kodierart::QueryNichtSonder
        };
        raus.push_str(&prozent_kodieren(&q, art));
    }
    if let Some(f) = fragment {
        raus.push('#');
        raus.push_str(&prozent_kodieren(&f, Kodierart::Fragment));
    }
    raus
}

/// Wo Prozent-Kodierung angewendet wird.
#[derive(Clone, Copy)]
enum Kodierart {
    Pfad,
    /// Undurchsichtiger Pfad (Nicht-Sonderfall): Leerzeichen bleiben.
    Undurchsichtig,
    Query,
    /// Query fuer Nicht-Sonderfaelle (Apostroph bleibt).
    QueryNichtSonder,
    Fragment,
    Benutzer,
}

/// Prozent-Kodiert die Zeichen der WHATWG-Mengen; vorhandene `%XX`
/// bekommen Grossbuchstaben, ungueltige `%` werden zu `%25`.
fn prozent_kodieren(text: &str, art: Kodierart) -> String {
    let mut raus = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1] as char;
            let h2 = bytes[i + 2] as char;
            if h1.is_ascii_hexdigit() && h2.is_ascii_hexdigit() {
                // Vorhandene Kodierung bleibt in ihrer Schreibweise.
                raus.push('%');
                raus.push(h1);
                raus.push(h2);
                i += 3;
                continue;
            }
        }
        let c = text[i..].chars().next().expect("Zeichen");
        let kodieren = if matches!(art, Kodierart::Undurchsichtig) {
            // Undurchsichtiger Pfad: C0-Steuerzeichen, DEL und
            // Nicht-ASCII; Leerzeichen und Satzzeichen bleiben.
            c <= '\u{1F}' || c == '\u{7F}' || (c as u32) > 0x7E
        } else {
            c <= '\u{1F}'
                || c == '\u{7F}'
                || c == ' '
                || c == '"'
                || c == '<'
                || c == '>'
                || (matches!(art, Kodierart::Pfad) && matches!(c, '{' | '}' | '`' | '^'))
                || (matches!(art, Kodierart::Fragment) && c == '`')
                || (matches!(art, Kodierart::Query) && matches!(c, '#' | '\''))
                || (matches!(art, Kodierart::Benutzer)
                    && matches!(
                        c,
                        '/' | ':' | ';' | '=' | '@' | '[' | '\\' | ']' | '^' | '|' | '`' | '{'
                            | '}'
                    ))
                || (c as u32) > 0x7E
        };
        if kodieren {
            for byte in c.to_string().as_bytes() {
                raus.push_str(&format!("%{byte:02X}"));
            }
        } else {
            raus.push(c);
        }
        i += c.len_utf8();
    }
    raus
}


/// Macht `.` und `..` zu nichts, rein im Text.
///
/// `..` ueber der Wurzel wird verworfen (`/../a` = `/a`): ein Pfad kann
/// nicht aus dem Server-Root hinauszeigen, und eine Endlosschleife aus
/// `..` waere ein Speicherproblem.
fn normalisiere_pfad(pfad: &str) -> String {
    let mit_slash = pfad.starts_with('/');
    // WHATWG: leere Segmente bleiben erhalten (`/a//b` ist nicht
    // `/a/b`); nur `.` faellt weg, `..` verkuerzt den Pfad. Ein
    // abschliessendes Punktesegment wird durch `/` ersetzt.
    let endet_auf_slash = {
        let klein = pfad.to_ascii_lowercase();
        (pfad.ends_with('/') && pfad.len() > 1)
            || klein.ends_with("/.")
            || klein.ends_with("/..")
            || klein.ends_with("/%2e")
            || klein.ends_with("/%2e.")
            || klein.ends_with("/.%2e")
            || klein.ends_with("/%2e%2e")
    };
    let mut teile: Vec<&str> = Vec::new();
    let segmente: Vec<&str> = pfad.split('/').collect();
    for (i, teil) in segmente.iter().enumerate() {
        if i == 0 && mit_slash {
            teile.push("");
            continue;
        }
        let klein = teil.to_ascii_lowercase();
        let ist_doppel = klein == ".." || klein == "%2e%2e" || klein == "%2e." || klein == ".%2e";
        let ist_einfach = klein == "." || klein == "%2e";
        if ist_doppel {
            if !(teile.is_empty() || (teile.len() == 1 && teile[0].is_empty())) {
                // Ein Laufwerksbuchstabe (`C:`) wird nicht entfernt.
                let letzter = *teile.last().expect("nicht leer");
                let ist_laufwerk = teile.len() == 2
                    && letzter.len() == 2
                    && letzter.ends_with(':')
                    && letzter.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
                if !ist_laufwerk {
                    teile.pop();
                }
            }
            continue;
        }
        if ist_einfach {
            continue;
        }
        teile.push(teil);
    }
    let mut raus = teile.join("/");
    if raus.is_empty() {
        raus.push('/');
    }
    if !raus.starts_with('/') {
        raus.insert(0, '/');
    }
    // Ein abschliessendes Punktesegment ergaenzt ein LEERES Segment
    // (`/a/.` -> `/a/`, `////../..` -> `//`), aber nicht, wenn der Pfad
    // danach die Wurzel ist (`/test/..` -> `/`).
    if endet_auf_slash && !pfad.ends_with('/') && teile.len() > 1 {
        raus.push('/');
    }
    raus
}

// -----------------------------------------------------------------------
// Prozent-Dekodierung. Ohne sie zeigt der Browser `Gr%C3%BC%C3%9Fe`
// statt `Grüße` und `a+b` statt `a b`.
// -----------------------------------------------------------------------

/// Dekodiert `%XX` zu Bytes.
///
/// Das Ergebnis kann KEIN gueltiges UTF-8 sein: `%FF` ist eine einzelne
/// Byte-Folge, die kein Zeichen ist. Deshalb wird hier nicht
/// `String::from_utf8` gerufen (das wuerde `Err` liefern und der Aufrufer
/// muesste raten), sondern der Fall liegt als [`Dekodiert::Bytes`] vor.
pub fn prozent_dekodieren(text: &str) -> Dekodiert {
    let roh = text.as_bytes();
    let mut raus: Vec<u8> = Vec::with_capacity(roh.len());
    let mut i = 0usize;
    while i < roh.len() {
        match roh[i] {
            b'%' if i + 2 < roh.len() => {
                match hex_paar(roh[i + 1], roh[i + 2]) {
                    Some(wert) => {
                        raus.push(wert);
                        i += 3;
                    }
                    // `%ZZ` ist kein Prozentzeichen: es bleibt, wie es ist.
                    // stillschweigend zu verwerfen wuerde den Text
                    // veraendern, ohne dass jemand etwas merkt.
                    None => {
                        raus.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                raus.push(b);
                i += 1;
            }
        }
    }
    match String::from_utf8(raus) {
        Ok(text) => Dekodiert::Text(text),
        // Kein Weg an die Bytes zurueck: `from_utf8` gibt bei einem
        // Fehler `FromUtf8Error` zurueck, und darin stecken sie.
        Err(fehler) => Dekodiert::Bytes(fehler.into_bytes()),
    }
}

/// Ergebnis der Dekodierung: Text (guiltiges UTF-8) oder Bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dekodiert {
    /// Gueltiges UTF-8, benutzbar als Anzeigetext.
    Text(String),
    /// Kein gueltiges UTF-8. Der Aufrufer entscheidet (Dateiname? Bild?).
    Bytes(Vec<u8>),
}

impl Dekodiert {
    /// Der Text, wenn es einer ist — fuer Anzeige.
    pub fn als_text(&self) -> Option<&str> {
        match self {
            Dekodiert::Text(t) => Some(t),
            Dekodiert::Bytes(_) => None,
        }
    }

    /// Die Bytes, wenn es keine sind.
    pub fn als_bytes(&self) -> &[u8] {
        match self {
            Dekodiert::Text(t) => t.as_bytes(),
            Dekodiert::Bytes(b) => b,
        }
    }
}

/// Zwei Hexadezimalzeichen zu einem Byte. `None` bei allem, was keine
/// Hexadezimalzahl ist — auch bei Grossbuchstaben ist sie es nicht.
fn hex_paar(oben: u8, unten: u8) -> Option<u8> {
    let o = hex_wert(oben)?;
    let u = hex_wert(unten)?;
    Some(o * 16 + u)
}

fn hex_wert(zeichen: u8) -> Option<u8> {
    match zeichen {
        b'0'..=b'9' => Some(zeichen - b'0'),
        b'a'..=b'f' => Some(zeichen - b'a' + 10),
        b'A'..=b'F' => Some(zeichen - b'A' + 10),
        _ => None,
    }
}

/// Dekodiert eine Query: Prozentzeichen UND `+` als Leerzeichen.
///
/// Warum `+`: das ist die Form aus HTML (`<form method=get>`). Der
/// Benutzer gibt `name=J%C3%BCrgen+M%C3%BCller` ein und will
/// `Jürgen Müller` sehen. Wer nur `%` dekodiert, zeigt `+` statt eines
/// Leerzeichens.
///
/// WICHTIG: `+` wird NUR in Query und Fragment ersetzt, nicht im Pfad.
/// Im Pfad ist `+` ein Pluszeichen (`/a+b` ist nicht `/a b`) — das ist
/// der Unterschied, den ein Browser an einem Dateinamen merkt.
pub fn query_dekodieren(query: &str) -> Dekodiert {
    let mit_plus = query.replace('+', " ");
    prozent_dekodieren(&mit_plus)
}

// -----------------------------------------------------------------------
// Punycode/IDNA nach RFC 3492 (Bootstring-Algorithmus).
//
// Ein Domain-Label mit Umlauten (`münchen`) wird in ACE-Form
// (`xn--mnchen-3ya`) kodiert, damit es im DNS (nur ASCII) steht.
// Rein-ASCII-Labels bleiben unveraendert.
//
// Die Konstanten unten sind die Bootstring-Parameter aus RFC 3492 §5.
// -----------------------------------------------------------------------

/// Bootstring-Basis: 36 verschiedene Ziffernzeichen (a-z, 0-9).
/// Quelle: RFC 3492, Abschnitt 5.
const PUNY_BASIS: u32 = 36;
/// Kleinste Schwelle der variablen Ganzzahl-Kodierung (RFC 3492 §5).
const PUNY_MIN: u32 = 1;
/// Groesste Schwelle der variablen Ganzzahl-Kodierung (RFC 3492 §5).
const PUNY_MAX: u32 = 26;
/// Schiefe der Neigungs-Anpassung (RFC 3492 §5).
const PUNY_SCHIEFE: u64 = 38;
/// Daempfung des allerersten Deltas (RFC 3492 §5).
const PUNY_DAEMPFUNG: u64 = 700;
/// Anfangs-Neigung (RFC 3492 §5).
const PUNY_START_NEIGUNG: u32 = 72;
/// Kleinstes nicht-basis Zeichen (RFC 3492 §5: 0x80).
const PUNY_START_N: u32 = 128;
/// Trennzeichen zwischen Basis-Anteil und kodiertem Anteil (RFC 3492 §5).
const PUNY_TRENNER: u8 = b'-';

/// Hoechste Zahl von Unicode-Punkten in EINEM Label, die kodiert werden.
///
/// BEGRUENDUNG (M3/M4): Ein DNS-Label fasst hoechstens 63 Zeichen
/// (RFC 1034 §3.1). 256 Punkte sind das Vierfache davon und damit
/// grosszuegig fuer jedes echte Label; alles darueber ist keine Domain,
/// sondern eine Bombe. Die Pruefung steht VOR jeder Allokation und vor
/// jeder Schleife, die von der Eingabe abhaengt.
const MAX_PUNYCODE_PUNKTE: usize = 256;
/// Hoechste Byte-Laenge einer Punycode-Eingabe beim Dekodieren (M4,
/// selbe Begruendung: kodierte Labels sind nie laenger als ihr
/// decoded Gegenstueck plus Trennzeichen; 256 ist grosszuegig).
const MAX_PUNYCODE_BYTES: usize = 256;
/// Hoechste Byte-Laenge einer ganzen Domain (M4: RFC 1035 §2.3.4
/// begrenzt Domain-Namen auf 255 Oktette; 256 ist die naechste runde
/// Schranke darueber).
const MAX_DOMAIN_BYTES: usize = 256;

/// Fehler der Punycode/IDNA-Kodierung, alle benannt (M7/M8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PunycodeFehler {
    /// Leere Eingabe: kein Label, keine Domain.
    Leer,
    /// Eingabe laenger als die Schranke — Bombe, kein Label.
    ZuLang { gefunden: usize, maximum: usize },
    /// Zeichen ohne Ziffernwert, abgeschnittene Eingabe, kein gueltiges
    /// Unicode-Zeichen — mit dem beanstandeten Wert.
    Ungueltig(String),
    /// Arithmetik-Ueberlauf nach RFC 3492 §6.4 (Erkennung statt
    /// Verhinderung: jede gepruefte Operation meldet statt zu wickeln).
    Ueberlauf,
}

impl std::fmt::Display for PunycodeFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PunycodeFehler::Leer => write!(f, "leeres Label: nichts zu kodieren"),
            PunycodeFehler::ZuLang { gefunden, maximum } => write!(
                f,
                "Eingabe mit {gefunden} Zeichen ist laenger als {maximum} (kein Domain-Label)"
            ),
            PunycodeFehler::Ungueltig(w) => write!(f, "ungueltige Punycode-Eingabe: {w:?}"),
            PunycodeFehler::Ueberlauf => write!(f, "Ueberlauf in der Punycode-Arithmetik (RFC 3492 §6.4)"),
        }
    }
}

impl std::error::Error for PunycodeFehler {}

/// Neigungs-Anpassung nach RFC 3492 §6.1 (`adapt`).
///
/// `differenz` ist das zuletzt kodierte Delta, `punkte` die Zahl der
/// bisher behandelten Codepunkte (immer >= 1: Division durch Null ist
/// ausgeschlossen), `erstes` sagt, ob es das allererste Delta ist.
fn anpassen(mut differenz: u64, punkte: u64, erstes: bool) -> u32 {
    differenz = if erstes {
        differenz / PUNY_DAEMPFUNG
    } else {
        differenz / 2
    };
    differenz += differenz / punkte;
    let mut k = 0u32;
    // Die Schleife endet sicher: `differenz` schrumpft pro Durchlauf
    // mindestens auf ein 35stel (M3: Eingabe-unabhaengige Schranke durch
    // Division, kein Zaehler noetig).
    while differenz > ((PUNY_BASIS - PUNY_MIN) * PUNY_MAX / 2) as u64 {
        differenz /= (PUNY_BASIS - PUNY_MIN) as u64;
        k += PUNY_BASIS;
    }
    k + (((PUNY_BASIS - PUNY_MIN + 1) as u64 * differenz) / (differenz + PUNY_SCHIEFE)) as u32
}

/// Schwelle `t` nach RFC 3492 §6.2/§6.3, geklemmt auf [`PUNY_MIN`]..=[`PUNY_MAX`].
fn schwelle(k: u32, neigung: u32) -> u32 {
    if k <= neigung {
        PUNY_MIN
    } else if k >= neigung + PUNY_MAX {
        PUNY_MAX
    } else {
        k - neigung
    }
}

/// Ziffernwert 0..36 nach RFC 3492 §5. Gross- und Kleinbuchstaben zaehlen
/// gleich (der Dekodierer MUSS beide erkennen).
fn ziffer_wert(zeichen: u8) -> Option<u64> {
    match zeichen {
        b'0'..=b'9' => Some((zeichen - b'0') as u64 + 26),
        b'a'..=b'z' => Some((zeichen - b'a') as u64),
        b'A'..=b'Z' => Some((zeichen - b'A') as u64),
        _ => None,
    }
}

/// Ziffer 0..36 zum Zeichen (Kodierer gibt nur Kleinbuchstaben aus,
/// RFC 3492 §5: "SHOULD output only lowercase").
fn ziffer_zeichen(ziffer: u32) -> char {
    debug_assert!(ziffer < PUNY_BASIS);
    if ziffer < 26 {
        (b'a' + ziffer as u8) as char
    } else {
        (b'0' + (ziffer - 26) as u8) as char
    }
}

/// Kodiert EIN Label nach RFC 3492 §6.3 — ohne `xn--`-Vorsatz.
///
/// Rein-ASCII-Labels kommen kleingeschrieben zurueck (IDNA vergleicht
/// ohne Gross/Klein). `münchen` wird zu `mnchen-3ya`.
/// Ueberlange Eingaben und Arithmetik-Ueberlaeufe sind benannte Fehler,
/// kein Panic, kein Speicherwachstum.
pub fn punycode_kodieren(label: &str) -> Result<String, PunycodeFehler> {
    if label.is_empty() {
        return Err(PunycodeFehler::Leer);
    }
    let punkte: Vec<u32> = label.chars().map(|c| c as u32).collect();
    // M4: Laenge pruefen, BEVOR weiter alloziert oder geschleift wird.
    if punkte.len() > MAX_PUNYCODE_PUNKTE {
        return Err(PunycodeFehler::ZuLang {
            gefunden: punkte.len(),
            maximum: MAX_PUNYCODE_PUNKTE,
        });
    }
    if punkte.iter().all(|&c| c < 0x80) {
        return Ok(label.to_ascii_lowercase());
    }
    // M4: Ausgabe gedeckelt aufstellen, nicht wachsen lassen.
    let mut ausgabe = String::with_capacity(punkte.len() + 8);
    let mut n = PUNY_START_N;
    let mut differenz: u64 = 0;
    let mut neigung = PUNY_START_NEIGUNG;
    let mut behandelt: usize = 0;
    // 1. Basis-Zeichen wörtlich kopieren (RFC 3492 §3.1).
    for &c in &punkte {
        if c < 0x80 {
            ausgabe.push(c as u8 as char);
            behandelt += 1;
        }
    }
    let basis_anzahl = behandelt;
    if basis_anzahl > 0 {
        ausgabe.push('-');
    }
    // 2. Nicht-Basis-Zeichen als Deltas einfuegen (§3.2, Verfahren §6.3).
    // M3: Die Schleife endet, weil `behandelt` bei jedem kodierten
    // Zeichen waechst und die Eingabe auf MAX_PUNYCODE_PUNKTE begrenzt ist.
    while behandelt < punkte.len() {
        let mut m = u32::MAX;
        for &c in &punkte {
            if c >= n && c < m {
                m = c;
            }
        }
        let schritt = ((m - n) as u64)
            .checked_mul(behandelt as u64 + 1)
            .ok_or(PunycodeFehler::Ueberlauf)?;
        differenz = differenz.checked_add(schritt).ok_or(PunycodeFehler::Ueberlauf)?;
        n = m;
        for &c in &punkte {
            if c < n {
                differenz = differenz.checked_add(1).ok_or(PunycodeFehler::Ueberlauf)?;
            } else if c == n {
                let mut rest = differenz;
                let mut k = PUNY_BASIS;
                loop {
                    let grenze = schwelle(k, neigung);
                    if rest < grenze as u64 {
                        break;
                    }
                    let rest_basis = PUNY_BASIS as u64 - grenze as u64;
                    let ziffer =
                        (grenze as u64 + (rest - grenze as u64) % rest_basis) as u32;
                    ausgabe.push(ziffer_zeichen(ziffer));
                    if ausgabe.len() > MAX_PUNYCODE_BYTES {
                        return Err(PunycodeFehler::ZuLang {
                            gefunden: ausgabe.len(),
                            maximum: MAX_PUNYCODE_BYTES,
                        });
                    }
                    rest = (rest - grenze as u64) / rest_basis;
                    k = k.saturating_add(PUNY_BASIS);
                }
                ausgabe.push(ziffer_zeichen(rest as u32));
                if ausgabe.len() > MAX_PUNYCODE_BYTES {
                    return Err(PunycodeFehler::ZuLang {
                        gefunden: ausgabe.len(),
                        maximum: MAX_PUNYCODE_BYTES,
                    });
                }
                neigung = anpassen(differenz, behandelt as u64 + 1, behandelt == basis_anzahl);
                differenz = 0;
                behandelt += 1;
            }
        }
        differenz = differenz.checked_add(1).ok_or(PunycodeFehler::Ueberlauf)?;
        n = n.checked_add(1).ok_or(PunycodeFehler::Ueberlauf)?;
    }
    Ok(ausgabe)
}

/// Dekodiert EIN Label nach RFC 3492 §6.2 — ohne `xn--`-Vorsatz.
///
/// `mnchen-3ya` wird zu `münchen`. Grossbuchstaben in der Eingabe werden
/// erkannt (RFC 3492 §5). Abgeschnittene Eingaben, Zeichen ohne
/// Ziffernwert und Ueberlaeufe sind benannte Fehler.
pub fn punycode_dekodieren(code: &str) -> Result<String, PunycodeFehler> {
    if code.is_empty() {
        return Err(PunycodeFehler::Leer);
    }
    // M4: Byte-Laenge pruefen, bevor irgendetwas angefasst wird.
    if code.len() > MAX_PUNYCODE_BYTES {
        return Err(PunycodeFehler::ZuLang {
            gefunden: code.len(),
            maximum: MAX_PUNYCODE_BYTES,
        });
    }
    let bytes = code.as_bytes();
    if !bytes.iter().all(|b| b.is_ascii()) {
        return Err(PunycodeFehler::Ungueltig(
            "nicht-ASCII in Punycode-Eingabe".to_string(),
        ));
    }
    // Basis-Anteil = alles vor dem LETZTEN Trennzeichen (§3.1).
    let mut ausgabe: Vec<u32> = Vec::new();
    let mut pos = 0usize;
    if let Some(letzte) = bytes.iter().rposition(|&b| b == PUNY_TRENNER) {
        for &b in &bytes[..letzte] {
            ausgabe.push(b as u32);
        }
        pos = letzte + 1;
    }
    if ausgabe.len() > MAX_PUNYCODE_PUNKTE {
        return Err(PunycodeFehler::ZuLang {
            gefunden: ausgabe.len(),
            maximum: MAX_PUNYCODE_PUNKTE,
        });
    }
    let mut n = PUNY_START_N;
    let mut i: u64 = 0;
    let mut neigung = PUNY_START_NEIGUNG;
    // M3: Jede Runde verbraucht mindestens ein Eingabe-Byte; die Eingabe
    // ist auf MAX_PUNYCODE_BYTES begrenzt, die Ausgabe auf
    // MAX_PUNYCODE_PUNKTE — keine Runde kann ewig laufen.
    while pos < bytes.len() {
        let alt_i = i;
        let mut gewicht: u64 = 1;
        let mut k = PUNY_BASIS;
        loop {
            if pos >= bytes.len() {
                return Err(PunycodeFehler::Ungueltig(
                    "abgeschnittene Ganzzahl am Eingabe-Ende".to_string(),
                ));
            }
            let ziffer = match ziffer_wert(bytes[pos]) {
                Some(d) => d,
                None => {
                    return Err(PunycodeFehler::Ungueltig(format!(
                        "kein Punycode-Zeichen: {:?}",
                        bytes[pos] as char
                    )));
                }
            };
            pos += 1;
            let summand = ziffer.checked_mul(gewicht).ok_or(PunycodeFehler::Ueberlauf)?;
            i = i.checked_add(summand).ok_or(PunycodeFehler::Ueberlauf)?;
            let grenze = schwelle(k, neigung);
            if ziffer < grenze as u64 {
                break;
            }
            gewicht = gewicht
                .checked_mul((PUNY_BASIS - grenze) as u64)
                .ok_or(PunycodeFehler::Ueberlauf)?;
            k = k.saturating_add(PUNY_BASIS);
        }
        let anzahl = ausgabe.len() as u64 + 1;
        // RFC 3492 §6.2, woertlich: `adapt` bekommt das Delta (i - alt_i),
        // `n` und `i` dagegen das VOLLE i. Der Dekodier-Trace zu Beispiel B
        // belegt es: zweites Delta 64 bei i=1 -> n = 4E0D + 65/2 = 4E2D,
        // Einfuegeposition 65 % 2 = 1 (hinten, nicht vorn).
        neigung = anpassen(i - alt_i, anzahl, alt_i == 0);
        let quotient = u32::try_from(i / anzahl).map_err(|_| PunycodeFehler::Ueberlauf)?;
        n = n.checked_add(quotient).ok_or(PunycodeFehler::Ueberlauf)?;
        i %= anzahl;
        if ausgabe.len() + 1 > MAX_PUNYCODE_PUNKTE {
            return Err(PunycodeFehler::ZuLang {
                gefunden: ausgabe.len() + 1,
                maximum: MAX_PUNYCODE_PUNKTE,
            });
        }
        char::from_u32(n).ok_or_else(|| {
            PunycodeFehler::Ungueltig(format!("kein Unicode-Punkt: U+{n:04X}"))
        })?;
        ausgabe.insert(i as usize, n);
        i += 1;
    }
    Ok(ausgabe
        .into_iter()
        .map(|c| char::from_u32(c).expect("oben geprueft: gueltiger Punkt"))
        .collect())
}

/// Kodiert eine ganze Domain labelweise nach IDNA: Labels mit Umlauten
/// erhalten den `xn--`-Vorsatz (ACE), ASCII-Labels bleiben.
///
/// `münchen.de` wird zu `xn--mnchen-3ya.de`, `muenchen.de` bleibt.
/// Leere Teilstuecke (fuehrender/folgender Punkt) bleiben erhalten.
///
/// ```
/// use narsil_url::domain_kodieren;
/// assert_eq!(domain_kodieren("muenchen.de").unwrap(), "muenchen.de");
/// assert_eq!(domain_kodieren("münchen.de").unwrap(), "xn--mnchen-3ya.de");
/// ```
pub fn domain_kodieren(domain: &str) -> Result<String, PunycodeFehler> {
    if domain.is_empty() {
        return Err(PunycodeFehler::Leer);
    }
    // M4: Domain-Laenge nach RFC 1035 §2.3.4 (255 Oktette) deckeln.
    if domain.len() > MAX_DOMAIN_BYTES {
        return Err(PunycodeFehler::ZuLang {
            gefunden: domain.len(),
            maximum: MAX_DOMAIN_BYTES,
        });
    }
    let mut teile: Vec<String> = Vec::new();
    for label in domain.split('.') {
        if label.is_empty() {
            teile.push(String::new());
        } else if label.bytes().all(|b| b.is_ascii()) {
            teile.push(label.to_ascii_lowercase());
        } else {
            teile.push(format!("xn--{}", punycode_kodieren(label)?));
        }
    }
    Ok(teile.join("."))
}

/// Dekodiert eine ganze Domain: `xn--`-Labels werden zurueckverwandelt.
///
/// `xn--mnchen-3ya.de` wird zu `münchen.de`. Labels ohne Vorsatz bleiben.
pub fn domain_dekodieren(domain: &str) -> Result<String, PunycodeFehler> {
    if domain.is_empty() {
        return Err(PunycodeFehler::Leer);
    }
    if domain.len() > MAX_DOMAIN_BYTES {
        return Err(PunycodeFehler::ZuLang {
            gefunden: domain.len(),
            maximum: MAX_DOMAIN_BYTES,
        });
    }
    let mut teile: Vec<String> = Vec::new();
    for label in domain.split('.') {
        if label.is_empty() {
            teile.push(String::new());
        } else if let Some(rest) = label.strip_prefix("xn--").or_else(|| {
            if label.len() > 4 && label[..4].eq_ignore_ascii_case("xn--") {
                Some(&label[4..])
            } else {
                None
            }
        }) {
            teile.push(punycode_dekodieren(rest)?);
        } else {
            teile.push(label.to_string());
        }
    }
    Ok(teile.join("."))
}

/// Kodiert Daten nach `application/x-www-form-urlencoded` (WHATWG URL §5).
///
/// Regeln: Leerzeichen wird zu `+`; ASCII-Buchstaben/Ziffern sowie
/// `* - . _` bleiben; ALLES andere wird Byte-fuer-Byte seiner UTF-8-Form
/// als `%XX` geschrieben. Umlaute werden also kodiert (`ü` -> `%C3%BC`),
/// nicht abgeschnitten.
///
/// GEFUNDEN 30.09.2026: Die alte Fassung schrieb `c as u8` (nur das
/// untere Byte) und liess nicht-ASCII-`is_alphanumeric` roh stehen —
/// `ü` kam kaputt oder roh heraus. U6 verlangt Umlaute: darum UTF-8.
pub fn form_url_encode(text: &str) -> String {
    // M4: Ausgabe ist hoechstens 3x Eingabe (%XX je Byte) — gedeckelt.
    let mut result = String::with_capacity(text.len().saturating_mul(3).min(MAX_FORMULAR_BYTES));
    for c in text.chars() {
        match c {
            ' ' => result.push('+'),
            '0'..='9' | 'a'..='z' | 'A'..='Z' | '*' | '-' | '.' | '_' => {
                result.push(c);
            }
            _ => {
                // Jedes UTF-8-Byte einzeln als %XX (gross, wie WHATWG).
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).bytes() {
                    result.push('%');
                    result.push(char::from(HEX_CHARS[(byte >> 4) as usize]));
                    result.push(char::from(HEX_CHARS[(byte & 0xF) as usize]));
                }
            }
        }
        if result.len() >= MAX_FORMULAR_BYTES {
            break;
        }
    }
    result
}

/// Dekodiert `application/x-www-form-urlencoded`-Daten.
///
/// Umgekehrt von [`form_url_encode`]: `+` wird zu Leerzeichen,
/// `%XX`-Folgen werden zu Bytes gesammelt und als UTF-8 gelesen.
/// Ungueltiges UTF-8 wird zum Ersatzzeichen (kein Panic); unvollstaendige
/// `%`-Folgen bleiben stehen statt zu verschwinden.
pub fn form_url_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut roh: Vec<u8> = Vec::with_capacity(bytes.len().min(MAX_FORMULAR_BYTES));
    let mut i = 0usize;
    // M3: Ein Durchlauf ueber die Eingabe, jede Runde frisst >= 1 Byte.
    while i < bytes.len() && roh.len() < MAX_FORMULAR_BYTES {
        match bytes[i] {
            b'+' => {
                roh.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                match (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                    (Some(oben), Some(unten)) => {
                        roh.push(oben << 4 | unten);
                        i += 3;
                    }
                    // Kein Hex: `%` bleibt, wie es ist.
                    _ => {
                        roh.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                roh.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&roh).into_owned()
}

fn from_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'A'..=b'F' => Some(c - b'A' + 10),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

const HEX_CHARS: &[char; 16] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F'];

// -----------------------------------------------------------------------
// Formular-Daten (U6): GET/POST aus Feldern, x-www-form-urlencoded.
// -----------------------------------------------------------------------

/// Hoechste Byte-Laenge EINER kodierten Formular-Nutzlast.
///
/// BEGRUENDUNG (M4): Formulare tragen Namen, Adressen, Suchwoerter —
/// keine Romane. 1 MiB fasst etwa 5000 ausgefuellte Suchfelder; alles
/// darueber bricht der Aufbau mit [`FormularFehler::ZuGross`] ab, statt
/// zu wachsen. Geprueft WAEHREND des Aufbaus, nicht erst am Ende.
pub const MAX_FORMULAR_BYTES: usize = 1_048_576;
/// Hoechste Feldzahl eines Formulars (M3: echte Formulare haben unter
/// hundert Felder; 1024 ist grosszuegig, danach ist es eine Bombe).
pub const MAX_FORMULAR_FELDER: usize = 1024;
/// Inhaltstyp eines url-kodierten POST-Rumpfs (HTML-Standard).
pub const FORMULAR_INHALTSTYP: &str = "application/x-www-form-urlencoded";

/// Fehler beim Formular-Aufbau, alle benannt (M7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormularFehler {
    /// Mehr Felder als [`MAX_FORMULAR_FELDER`].
    ZuVieleFelder { gefunden: usize, maximum: usize },
    /// Nutzlast laenger als [`MAX_FORMULAR_BYTES`].
    ZuGross { gefunden: usize, maximum: usize },
}

impl std::fmt::Display for FormularFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormularFehler::ZuVieleFelder { gefunden, maximum } => {
                write!(f, "Formular mit {gefunden} Feldern, erlaubt sind {maximum}")
            }
            FormularFehler::ZuGross { gefunden, maximum } => {
                write!(f, "Formular-Nutzlast mit {gefunden} Bytes groesser als {maximum}")
            }
        }
    }
}

impl std::error::Error for FormularFehler {}

/// EIN Feld eines HTML-Formulars: Name und Wert als getippt.
///
/// `q` + `München Wetter` ergibt kodiert `q=M%C3%BCnchen+Wetter`.
/// Leere Namen bleiben erhalten (Browser schicken `=wert` mit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormularFeld {
    /// Der `name`-Attribut des Eingabefelds.
    pub name: String,
    /// Der eingetippte/ausgewaehlte Wert.
    pub wert: String,
}

impl FormularFeld {
    /// Ein Feld mit Name und Wert.
    pub fn neu(name: impl Into<String>, wert: impl Into<String>) -> Self {
        FormularFeld {
            name: name.into(),
            wert: wert.into(),
        }
    }
}

/// Baut `name=wert&...` aus Feldern (Rumpf fuer POST, Anhang fuer GET).
///
/// Kodierung je Teil per [`form_url_encode`]: Umlaute als UTF-8-`%XX`,
/// Leerzeichen als `+`. Leere Liste ergibt leeren String (kein `?`).
pub fn formular_kodieren(felder: &[FormularFeld]) -> Result<String, FormularFehler> {
    if felder.len() > MAX_FORMULAR_FELDER {
        return Err(FormularFehler::ZuVieleFelder {
            gefunden: felder.len(),
            maximum: MAX_FORMULAR_FELDER,
        });
    }
    let mut raus = String::new();
    for (i, feld) in felder.iter().enumerate() {
        if i > 0 {
            raus.push('&');
        }
        raus.push_str(&form_url_encode(&feld.name));
        raus.push('=');
        raus.push_str(&form_url_encode(&feld.wert));
        if raus.len() > MAX_FORMULAR_BYTES {
            return Err(FormularFehler::ZuGross {
                gefunden: raus.len(),
                maximum: MAX_FORMULAR_BYTES,
            });
        }
    }
    Ok(raus)
}

/// Baut die GET-Zieladresse: `basis` + `?`/`&` + kodierte Felder.
///
/// Bestehende Query bleibt (`/s?a=1` + `q=x` -> `/s?a=1&q=x`), ein
/// `#fragment` wandert ans Ende. Leere Felder: `basis` unveraendert.
pub fn formular_get_url(basis: &str, felder: &[FormularFeld]) -> Result<String, FormularFehler> {
    let anhang = formular_kodieren(felder)?;
    if anhang.is_empty() {
        return Ok(basis.to_string());
    }
    // Fragment abspalten, Query anhaengen, Fragment wieder dran.
    let (stamm, bruch) = match basis.find('#') {
        Some(i) => (&basis[..i], &basis[i..]),
        None => (basis, ""),
    };
    let trenner = if stamm.contains('?') { '&' } else { '?' };
    let mut raus = String::with_capacity(stamm.len() + 1 + anhang.len() + bruch.len());
    raus.push_str(stamm);
    raus.push(trenner);
    raus.push_str(&anhang);
    raus.push_str(bruch);
    if raus.len() > MAX_FORMULAR_BYTES {
        return Err(FormularFehler::ZuGross {
            gefunden: raus.len(),
            maximum: MAX_FORMULAR_BYTES,
        });
    }
    Ok(raus)
}

/// Trennt `host[:port]`, prueft Host (klein, kein Leerzeichen/`@`/`[`,
/// nicht leer) und Port (Zahl, <= 65535).
fn split_host_port(host_port: &str) -> Result<(String, Option<u16>), UrlError> {
    if host_port.is_empty() {
        return Err(UrlError::InvalidHost(host_port.to_string()));
    }
    // IPv6-LITERAL, neu in Welle 2 (29.09.2026).
    //
    // GEMESSENER ZUSTAND VORHER: `http://[::1]:8080/x` scheiterte mit
    // `InvalidHost("[::1]:8080")`, weil die alte Fassung jedes `[`
    // ablehnte. Sie tat das, um zu verhindern, dass ein Hostname wie
    // `[irgendwas]` durchrutscht — aber IPv6 ist auf localhost und auf
    // Servern, die auf ::1 lauschen, der NORMALFALL, nicht der
    // Ausnahmefall. `localhost` auf einem Rechner, der auf ::1 zuerst
    // antwortet, laedt sonst gar nichts.
    //
    // Die Klammern sind SYNTAX (RFC 3986 §3.2.2), nicht Teil des
    // Hostnamens. `Address::host` traegt darum `::1` OHNE Klammern; wer
    // sie braucht, setzt sie ueber `Address::host_mit_port`.
    if host_port.starts_with('[') {
        let (host, port) = split_ipv6(host_port)?;
        return Ok((host, port));
    }
    // Eine schliessende Klammer ohne oeffnende ist genauso kaputt.
    if host_port.contains(']') {
        return Err(UrlError::InvalidHost(host_port.to_string()));
    }
    let (host_teil, port_text) = match host_port.find(':') {
        Some(i) => (&host_port[..i], Some(&host_port[i + 1..])),
        None => (host_port, None),
    };
    if host_teil.is_empty()
        || host_teil.contains('@')
        || host_teil.contains('[')
        || host_teil.chars().any(|c| c.is_whitespace())
    {
        return Err(UrlError::InvalidHost(host_teil.to_string()));
    }
    let port = match port_text {
        // Port 0 ist reserviert (kein Zielport) -> Fehler statt Schweigen.
        None => None,
        Some(p) => {
            // `host:abc` ist KEIN Port und auch kein Fehler: das ist ein
            // Hostname mit einem Doppelpunkt, den es in freier Wildbahn
            // nicht gibt, aber der Benutzer kann es tippen. GEMESSEN:
            // `narsil/src/laden.rs` macht es genauso (Zeile 128-137) und
            // nimmt dann Port 80. Ein stiller Port 80 wuerde aber
            // `localhost:9x9` an einen ganz anderen Server schicken —
            // deshalb ist es ein benannter Fehler und nicht 80.
            if p.is_empty() {
                return Err(UrlError::InvalidPort(p.to_string()));
            }
            match p.parse::<u32>() {
                Ok(n) if (1..=65535).contains(&n) => Some(n as u16),
                _ => return Err(UrlError::InvalidPort(p.to_string())),
            }
        }
    };
    Ok((host_teil.to_ascii_lowercase(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_split_is_case_insensitive() {
        assert_eq!(split_scheme("HTTP://h/x"), Ok((Scheme::Http, "h/x")));
        assert_eq!(split_scheme("h/x"), Ok((Scheme::Localhost, "h/x")));
    }

    #[test]
    fn u2_leere_segmente_und_laufwerk() {
        assert_eq!(normalisiere_pfad("////../.."), "//");
        assert_eq!(normalisiere_pfad("/a//b"), "/a//b");
        assert_eq!(normalisiere_pfad("/a/../b"), "/b");
        assert_eq!(normalisiere_pfad("/C:/a/.."), "/C:/");
    }
}
