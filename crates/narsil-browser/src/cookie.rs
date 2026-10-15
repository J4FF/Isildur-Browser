//! Cookie-Speicher nach RFC 6265.
//! Verwaltet Cookies pro Domain mit Grenzen (M4/M6).
//!
//! Obergrenzen:
//! - Hoechstens 50 Cookies je Domain
//! - Hoechstens 3000 Cookies gesamt
//! - Hoechstens 4096 Bytes je Cookie-Name+Wert
//! - Drittanbieter-Cookies standardmaessig AUS: Ein `Domain`-Attribut,
//!   das nicht zur Anfrage-Domain passt (fremde oder oeffentliche Domain
//!   wie `com`), wird mit [`CookieFehler::UngueltigeDomain`] abgelehnt.
//!
//! Einstieg: [`set_cookie_lesen`] liest einen echten `Set-Cookie`-Header
//! (RFC 6265 §5.2), [`CookieSpeicher::ablegen`] speichert ihn ( §5.3),
//! [`CookieSpeicher::cookies_fuer_anfrage`] liefert passende Cookies.
//! Daten (RFC 6265 §5.1.1), Domain-Abgleich (§5.1.3) und Standard-Pfad
//! (§5.1.4) sind eigene, einzeln getestete Funktionen.

// ---------------------------------------------------------------------------
// Modul-Intern
// ---------------------------------------------------------------------------

/// Der Ladezustand eines Cookies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LadezustandCookie {
    /// Der Cookie ist neu und wurde noch nicht vom Server zurueckgegeben.
    Neu,
    /// Der Cookie wurde vom Server gesetzt und ist gueltig.
    Gueltig,
    /// Der Cookie ist abgelaufen und wurde geloescht.
    Abgelaufen,
}

/// Ein Cookie nach RFC 6265, Abschnitt 5.3 (Speichermodell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// Der Name des Cookies.
    pub name: String,
    /// Der Wert des Cookies (darf leer sein: Loesch-Cookie, RFC 6265 §3.1).
    pub wert: String,
    /// Das `Expires`-Attribut im Originalwortlaut, falls vorhanden.
    pub expires: Option<String>,
    /// Das `Max-Age`-Attribut in Sekunden, falls gueltig.
    /// Hat VORRANG vor `Expires` (RFC 6265 §4.1.2.2).
    pub max_age: Option<i64>,
    /// Ablaufzeitpunkt als Unix-Sekunden: aus `Max-Age` (Vorrang) oder
    /// `Expires` berechnet, `None` heisst Sitzungs-Cookie (kein Verfall).
    pub ablauf: Option<u64>,
    /// Die Domain, fuer die der Cookie gilt. Ohne `Domain`-Attribut ist
    /// das die Anfrage-Domain und [`Cookie::host_only`] wahr (§5.3).
    pub domain: Option<String>,
    /// Nur Anfrage-Domain exakt (kein `Domain`-Attribut gesetzt, §5.3).
    pub host_only: bool,
    /// Der Path, fuer den der Cookie gueltig ist (Default: §5.1.4).
    pub path: Option<String>,
    /// Secure-Flag: nur ueber HTTPS senden (§5.2.5).
    pub secure: bool,
    /// HttpOnly-Flag: nicht fuer Skripte sichtbar (§5.2.6).
    pub http_only: bool,
    /// SameSite-Wert normiert (`Lax`/`Strict`/`None`), falls gueltig.
    pub same_site: Option<String>,
    /// Der Ladezustand.
    pub zustand: LadezustandCookie,
    /// Erstellungszeitpunkt als Unix-Sekunden (RFC 6265 §5.3, Schritt 2).
    pub alter: u64,
}

/// Der Typ eines Cookies: "First-Party" oder "Third-Party".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieTyp {
    /// Der Cookie gehoert zur aktuellen Domain (First-Party).
    FirstParty,
    /// Der Cookie gehoert einer anderen Domain (Third-Party).
    ThirdParty,
}

/// Der Zustand des Cookie-Speichers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieSpeicher {
    /// Alle gespeicherten Cookies.
    cookies: Vec<Cookie>,
    /// Zaehler fuer die Gesamtanzahl der Cookies (Grenze: 3000).
    gesamtanzahl: usize,
}

/// Fehler beim Cookie-Betrieb. Jede Variante nennt den Wert (M7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieFehler {
    /// Zu viele Cookies gesamt (Grenze: 3000).
    ZuVieleGesamt { aktuell: usize, maximum: usize },
    /// Zu viele Cookies pro Domain (Grenze: 50).
    ZuVieleProDomain {
        domain: String,
        aktuell: usize,
        maximum: usize,
    },
    /// Cookie-Name+Wert groesser als 4096 Bytes.
    ZuGroesser { groesse: usize, maximum: usize },
    /// `Domain`-Attribut passt nicht zur Anfrage (fremde Domain) oder
    /// ist eine oeffentliche Endung ohne exakten Treffer (`com`).
    UngueltigeDomain(String),
    /// Cookie-Name leer oder mit Trennzeichen.
    UngueltigerName(String),
    /// Header ohne `=` im Namensteil oder sonst unlesbar.
    UngueltigerHeader(String),
}

impl std::fmt::Display for CookieFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CookieFehler::ZuVieleGesamt { aktuell, maximum } => {
                write!(f, "zu viele Cookies gesamt: {aktuell} von {maximum}")
            }
            CookieFehler::ZuVieleProDomain {
                domain,
                aktuell,
                maximum,
            } => {
                write!(
                    f,
                    "zu viele Cookies fuer {domain:?}: {aktuell} von {maximum}"
                )
            }
            CookieFehler::ZuGroesser { groesse, maximum } => {
                write!(f, "Cookie mit {groesse} Bytes groesser als {maximum}")
            }
            CookieFehler::UngueltigeDomain(d) => {
                write!(f, "Domain-Attribut {d:?} passt nicht zur Anfrage")
            }
            CookieFehler::UngueltigerName(n) => write!(f, "ungueltiger Cookie-Name: {n:?}"),
            CookieFehler::UngueltigerHeader(h) => {
                write!(f, "unlesbarer Set-Cookie-Header: {h:?}")
            }
        }
    }
}

impl std::error::Error for CookieFehler {}

/// Hoechste Byte-Laenge eines Set-Cookie-Headers, die gelesen wird.
///
/// BEGRUENDUNG (M4): Name+Wert duerfen ohnehin nur 4096 Bytes tragen;
/// 16384 Bytes lassen zusaetzlich alle Attribute zu. Alles darueber ist
/// keine Antwort, sondern eine Bombe. Geprueft VOR jeder Allokation.
const MAX_SET_COOKIE_BYTES: usize = 16384;
/// Hoechste Zahl von Attributen (`;`-Teilen) in einem Header.
///
/// BEGRUENDUNG (M3): Echte Header haben unter zehn Teile; 64 ist
/// grosszuegig. Jede Schleife ueber Eingabe-Teile endet hier.
const MAX_COOKIE_TEILE: usize = 64;
/// Hoechste Zahl von Datums-Marken in einem `Expires`-Wert (M3: RFC 6265
/// §5.1.1 erzeugt aus jedem Begrenzer eine Marke; echte Daten haben
/// unter zehn).
const MAX_DATUM_MARKEN: usize = 32;

/// Ein leerer Cookie-Speicher mit standardmaessigen Grenzen.
impl CookieSpeicher {
    /// Einen neuen, leeren Cookie-Speicher erzeugen.
    ///
    /// Die Grenzen sind standardmaessig:
    /// - Max 3000 Cookies gesamt
    /// - Max 50 Cookies pro Domain
    /// - Max 4096 Bytes je Cookie-Name+Wert
    pub fn neu() -> Self {
        Self {
            cookies: Vec::new(),
            gesamtanzahl: 0,
        }
    }

    /// Die maximale Anzahl an Cookies gesamt.
    fn max_gesamt(&self) -> usize {
        3000
    }

    /// Die maximale Anzahl an Cookies pro Domain.
    fn max_pro_domain(&self) -> usize {
        50
    }

    /// Die maximale Groesse von Cookie-Name+Wert in Bytes.
    fn max_groesse(&self) -> usize {
        4096
    }
}

impl CookieSpeicher {
    /// Legt einen gelesenen Cookie ab (RFC 6265 §5.3, Schritte 11-12).
    ///
    /// Gleicher Name + gleiche Domain + gleicher Pfad ERSETZT den alten
    /// Eintrag (kein Duplikat). Ein abgelaufener Cookie loescht den alten
    /// Eintrag und wird nicht gespeichert (Loesch-Cookie, §3.1).
    /// Grenzen (M4/M6): 50 je Domain, 3000 gesamt.
    pub fn ablegen(&mut self, cookie: Cookie, jetzt: u64) -> Result<(), CookieFehler> {
        if cookie.name.is_empty() {
            return Err(CookieFehler::UngueltigerName(cookie.name));
        }
        let groesse = cookie.name.len() + cookie.wert.len();
        if groesse > self.max_groesse() {
            return Err(CookieFehler::ZuGroesser {
                groesse,
                maximum: self.max_groesse(),
            });
        }
        // Alten Eintrag mit gleichem Dreiklang entfernen (RFC 6265 §5.3).
        let dreiklang = (
            cookie.name.clone(),
            cookie.domain.clone(),
            cookie.path.clone(),
        );
        if let Some(pos) = self
            .cookies
            .iter()
            .position(|c| (c.name.clone(), c.domain.clone(), c.path.clone()) == dreiklang)
        {
            self.cookies.remove(pos);
            self.gesamtanzahl = self.gesamtanzahl.saturating_sub(1);
        }
        // Abgelaufen heisst geloeschst: nicht speichern, nur entfernen.
        if let Some(ablauf) = cookie.ablauf {
            if ablauf <= jetzt {
                return Ok(());
            }
        }
        let schluessel = cookie.domain.clone().unwrap_or_default();
        let pro_domain = self
            .cookies
            .iter()
            .filter(|c| c.domain.as_ref().map_or(false, |d| d == &schluessel))
            .count();
        if pro_domain >= self.max_pro_domain() {
            return Err(CookieFehler::ZuVieleProDomain {
                domain: schluessel,
                aktuell: pro_domain,
                maximum: self.max_pro_domain(),
            });
        }
        if self.gesamtanzahl >= self.max_gesamt() {
            return Err(CookieFehler::ZuVieleGesamt {
                aktuell: self.gesamtanzahl,
                maximum: self.max_gesamt(),
            });
        }
        self.cookies.push(cookie);
        self.gesamtanzahl += 1;
        Ok(())
    }

    /// Alle Cookies, die zu Anfrage-Domain und -Pfad passen und nicht
    /// abgelaufen sind (RFC 6265 §5.4 sinngemaess).
    ///
    /// Prueft Ablauf (`jetzt`), `Secure` (nur bei `sicher`), Domain
    /// (exakt bei Host-only, Suffix-Abgleich sonst, §5.1.3) und Pfad
    /// (§5.1.4). Abgelaufene Cookies werden NIE geliefert.
    pub fn cookies_fuer_anfrage(
        &self,
        anfrage_domain: &str,
        anfrage_pfad: &str,
        sicher: bool,
        jetzt: u64,
    ) -> Vec<&Cookie> {
        // M4: Anfrage-Werte deckeln, bevor verglichen wird.
        if anfrage_domain.len() > MAX_SET_COOKIE_BYTES || anfrage_pfad.len() > MAX_SET_COOKIE_BYTES
        {
            return Vec::new();
        }
        let gefragt = anfrage_domain.to_ascii_lowercase();
        self.cookies
            .iter()
            .filter(|cookie| {
                // Ablauf zuerst: Totes wird nicht serviert.
                if let Some(ablauf) = cookie.ablauf {
                    if ablauf <= jetzt {
                        return false;
                    }
                }
                // Secure-Cookies duerfen nur ueber HTTPS gesendet werden.
                if cookie.secure && !sicher {
                    return false;
                }
                // Domain-Abgleich (§5.1.3): Host-only heisst exakt.
                let domain_match = match &cookie.domain {
                    Some(cd) if cookie.host_only => gefragt == *cd,
                    Some(cd) => domain_passt(&gefragt, cd),
                    None => true,
                };
                if !domain_match {
                    return false;
                }
                // Pfad-Abgleich (§5.1.4).
                let pfad = cookie.path.as_deref().unwrap_or("/");
                if !pfad_passt(anfrage_pfad, pfad) {
                    return false;
                }
                // SameSite: Ohne Kontext-Information (Top-Level oder nicht)
                // liefert diese Schicht an passende Domains wie bei `Lax`;
                // `Strict` wird genauso behandelt und hier dokumentiert
                // statt still zu blockieren.
                true
            })
            .collect()
    }

    /// Anzahl der gespeicherten Cookies.
    pub fn anzahl(&self) -> usize {
        self.cookies.len()
    }

    /// Anzahl der Cookies pro Domain.
    pub fn pro_domain(&self, domain: &str) -> usize {
        self.cookies
            .iter()
            .filter(|c| c.domain.as_ref().map_or(false, |d| d == domain))
            .count()
    }

    /// Einen Cookie durch Namen finden.
    pub fn finde(&self, name: &str) -> Option<&Cookie> {
        self.cookies.iter().find(|c| c.name == name)
    }

    /// Einen Cookie entfernen.
    pub fn entfernen(&mut self, name: &str) -> bool {
        if let Some(pos) = self.cookies.iter().position(|c| c.name == name) {
            self.cookies.remove(pos);
            self.gesamtanzahl = self.gesamtanzahl.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Alle Cookies loeschen.
    pub fn leeren(&mut self) {
        self.cookies.clear();
        self.gesamtanzahl = 0;
    }
}

// ---------------------------------------------------------------------------
// Set-Cookie lesen (RFC 6265 §5.2)
// ---------------------------------------------------------------------------

/// Liest einen echten `Set-Cookie`-Header (RFC 6265 §5.2).
///
/// `header` ist der komplette Header-Wert mit oder ohne `Set-Cookie:`-Vorsatz,
/// `anfrage_url` die volle Anfrage-Adresse (fuer Domain-Pruefung und
/// Standard-Pfad), `jetzt` die Unix-Sekunden (keine Uhr in der Crate).
///
/// Verhalten nach RFC:
/// - `Expires` wird als Cookie-Datum gelesen (§5.1.1); unlesbar heisst:
///   Attribut ignorieren, nicht Cookie verwerfen.
/// - `Max-Age` hat VORRANG vor `Expires` (§4.1.2.2); `<= 0` heisst sofort
///   abgelaufen (Loesch-Cookie). Unlesbar heisst: Attribut ignorieren.
/// - `Domain` ohne fuehrenden Punkt, kleingeschrieben; passt die Anfrage
///   nicht dazu, wird der Cookie ABGELEHNT (fremde Domain, §5.3 Schritt 6).
///   Eine Endung ohne Punkt (`com`) gilt nur bei exaktem Treffer
///   (Naeherung an die Public-Suffix-Liste, §5.3 Schritt 5).
/// - `Path` ohne `/` am Anfang faellt auf den Standard-Pfad (§5.1.4).
/// - Unbekannte Attribute werden ignoriert, der Cookie bleibt (§5.2).
/// - Leerer Name heisst: ganzen Header verwerfen (§5.2 Schritt 5).
pub fn set_cookie_lesen(
    header: &str,
    anfrage_url: &str,
    jetzt: u64,
) -> Result<Cookie, CookieFehler> {
    // M4: Header-Laenge pruefen, BEVOR irgendetwas kopiert wird.
    if header.len() > MAX_SET_COOKIE_BYTES {
        return Err(CookieFehler::ZuGroesser {
            groesse: header.len(),
            maximum: MAX_SET_COOKIE_BYTES,
        });
    }
    // Vorsatz `Set-Cookie:` entfernen (Gross/Klein egal).
    let wert = header.trim();
    let wert = match wert.find(':') {
        Some(i) if wert[..i].eq_ignore_ascii_case("set-cookie") => wert[i + 1..].trim(),
        _ => wert,
    };
    // Name=Wert steht vor dem ersten `;` (§5.2 Schritt 1).
    let (paar, attribute) = match wert.find(';') {
        Some(i) => (&wert[..i], &wert[i + 1..]),
        None => (wert, ""),
    };
    let (roh_name, roh_wert) = match paar.find('=') {
        Some(i) => (&paar[..i], &paar[i + 1..]),
        None => {
            return Err(CookieFehler::UngueltigerHeader(
                "kein `=` im Namensteil".to_string(),
            ));
        }
    };
    // Leerraum aussen weg, innen bleibt (§5.2 Schritte 3-4).
    let name = roh_name.trim_matches(|c| c == ' ' || c == '\t');
    let cookie_wert = roh_wert.trim_matches(|c| c == ' ' || c == '\t');
    // Leerer Name: ganzen Header verwerfen (§5.2 Schritt 5).
    if name.is_empty() {
        return Err(CookieFehler::UngueltigerName(name.to_string()));
    }

    let (anfrage_host, anfrage_pfad) = anfrage_zerlegen(anfrage_url);

    let mut expires_roh: Option<String> = None;
    let mut expires_zeit: Option<u64> = None;
    let mut max_alter: Option<i64> = None;
    let mut domain: Option<String> = None;
    let mut pfad: Option<String> = None;
    let mut sicher = false;
    let mut nur_http = false;
    let mut gleiche_seite: Option<String> = None;

    // M3: Attribute zaehlen, nach MAX_COOKIE_TEILE ist Schluss.
    let mut anzahl = 0usize;
    for teil in attribute.split(';') {
        if teil.trim().is_empty() {
            continue;
        }
        anzahl += 1;
        if anzahl > MAX_COOKIE_TEILE {
            break;
        }
        let (attr_name, attr_wert) = match teil.find('=') {
            Some(i) => (teil[..i].trim(), Some(teil[i + 1..].trim())),
            None => (teil.trim(), None),
        };
        // Letztes gleichnamiges Attribut gewinnt (§5.3: "the last").
        if attr_name.eq_ignore_ascii_case("expires") {
            if let Some(v) = attr_wert {
                expires_roh = Some(v.to_string());
                expires_zeit = cookie_datum_lesen(v);
            }
        } else if attr_name.eq_ignore_ascii_case("max-age") {
            if let Some(v) = attr_wert {
                // §5.2.2: erstes Zeichen Ziffer oder `-`, Rest nur Ziffern.
                let bytes = v.as_bytes();
                let gueltig = !bytes.is_empty()
                    && (bytes[0].is_ascii_digit() || bytes[0] == b'-')
                    && bytes[1..].iter().all(|b| b.is_ascii_digit());
                if gueltig {
                    // Laenge deckeln, damit kein Riesen-String geparst wird.
                    if v.len() <= 19 {
                        if let Ok(delta) = v.parse::<i64>() {
                            max_alter = Some(delta);
                        }
                    }
                }
            }
        } else if attr_name.eq_ignore_ascii_case("domain") {
            if let Some(v) = attr_wert {
                if v.is_empty() {
                    // Leeres Domain-Attribut: ignorieren (§5.2.3).
                } else {
                    // Fuehrender Punkt faellt weg (§4.1.2.3), Rest klein.
                    domain = Some(v.trim_start_matches('.').to_ascii_lowercase());
                }
            }
        } else if attr_name.eq_ignore_ascii_case("path") {
            if let Some(v) = attr_wert {
                if !v.is_empty() && v.starts_with('/') {
                    pfad = Some(v.to_string());
                } else {
                    pfad = Some(standard_pfad(&anfrage_pfad));
                }
            }
        } else if attr_name.eq_ignore_ascii_case("secure") {
            sicher = true;
        } else if attr_name.eq_ignore_ascii_case("httponly") {
            nur_http = true;
        } else if attr_name.eq_ignore_ascii_case("samesite") {
            if let Some(v) = attr_wert {
                if v.eq_ignore_ascii_case("lax") {
                    gleiche_seite = Some("Lax".to_string());
                } else if v.eq_ignore_ascii_case("strict") {
                    gleiche_seite = Some("Strict".to_string());
                } else if v.eq_ignore_ascii_case("none") {
                    gleiche_seite = Some("None".to_string());
                }
                // Unbekannter Wert: Attribut ignorieren, Cookie bleibt.
            }
        }
        // Unbekannte Attribute: ignorieren (§5.2 Schritt 6).
    }

    // Domain festlegen und pruefen (§5.3 Schritte 5-6).
    let (domain, host_only) = match domain {
        Some(d) => {
            if d != anfrage_host && !domain_passt(&anfrage_host, &d) {
                return Err(CookieFehler::UngueltigeDomain(d));
            }
            // Naeherung an die Public-Suffix-Liste (§5.3 Schritt 5): Eine
            // Endung ohne Punkt (`com`) setzt niemand ausser sie selbst.
            // Ohne echte PSL-Datei ist "exakt oder Punkt drin" die
            // schärfste noch ehrliche Regel.
            if d != anfrage_host && !d.contains('.') {
                return Err(CookieFehler::UngueltigeDomain(d));
            }
            (Some(d), false)
        }
        None => (Some(anfrage_host.clone()), true),
    };
    let pfad = Some(pfad.unwrap_or_else(|| standard_pfad(&anfrage_pfad)));

    // Ablauf: Max-Age hat Vorrang vor Expires (§4.1.2.2, §5.3 Schritt 3).
    let ablauf = match max_alter {
        Some(delta) if delta <= 0 => Some(0),
        Some(delta) => Some(jetzt.saturating_add(delta as u64)),
        None => expires_zeit,
    };

    Ok(Cookie {
        name: name.to_string(),
        wert: cookie_wert.to_string(),
        expires: expires_roh,
        max_age: max_alter,
        ablauf,
        domain,
        host_only,
        path: pfad,
        secure: sicher,
        http_only: nur_http,
        same_site: gleiche_seite,
        zustand: LadezustandCookie::Gueltig,
        alter: jetzt,
    })
}

/// Zerlegt eine Anfrage-Adresse in (Host, Pfad), beides kleingeschrieben
/// beim Host. Reine Textarbeit, kein Netz. Unlesbares ergibt ("", "/").
fn anfrage_zerlegen(url: &str) -> (String, String) {
    let text = url.trim();
    let nach_schema = match text.find("://") {
        Some(i) => &text[i + 3..],
        None => text,
    };
    let ende = nach_schema
        .find(['/', '?', '#'])
        .unwrap_or(nach_schema.len());
    let mut autoritaet = &nach_schema[..ende];
    let mut pfad = &nach_schema[ende..];
    if let Some(i) = pfad.find(['?', '#']) {
        pfad = &pfad[..i];
    }
    if pfad.is_empty() {
        pfad = "/";
    }
    // Benutzerinfo (`user@`) gehoert nicht zum Host.
    if let Some(i) = autoritaet.rfind('@') {
        autoritaet = &autoritaet[i + 1..];
    }
    // Host vom Port trennen; IPv6 steht in Klammern.
    let host = if let Some(rest) = autoritaet.strip_prefix('[') {
        match rest.find(']') {
            Some(i) => &rest[..i],
            None => rest,
        }
    } else if autoritaet.chars().filter(|&c| c == ':').count() >= 2 {
        // Unverklammertes IPv6-Literal: Doppelpunkte gehoeren dazu.
        autoritaet
    } else {
        match autoritaet.find(':') {
            Some(i) => &autoritaet[..i],
            None => autoritaet,
        }
    };
    (host.to_ascii_lowercase(), pfad.to_string())
}

/// Domain-Abgleich nach RFC 6265 §5.1.3.
///
/// Wahr bei Identitaet oder wenn `domain` ein Suffix von `anfrage` ist
/// und davor ein Punkt steht. IP-Adressen matchen nie per Suffix
/// (sonst wuerde `1.2.3.4` Cookies von `2.3.4` annehmen).
pub fn domain_passt(anfrage: &str, domain: &str) -> bool {
    if anfrage == domain {
        return true;
    }
    if ist_ip(anfrage) {
        return false;
    }
    match anfrage.strip_suffix(domain) {
        Some(rest) => rest.ends_with('.') && !rest.is_empty(),
        None => false,
    }
}

/// Grobe IP-Erkennung fuer den Domain-Abgleich: IPv4 (vier Zahlen mit
/// Punkten) oder IPv6 (Doppelpunkt drin). Keine Validierung, nur die
/// Unterscheidung "Host-Name oder Adresse" aus RFC 6265 §5.1.3.
fn ist_ip(host: &str) -> bool {
    if host.contains(':') {
        return true;
    }
    let teile: Vec<&str> = host.split('.').collect();
    teile.len() == 4
        && teile
            .iter()
            .all(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
}

/// Pfad-Abgleich nach RFC 6265 §5.1.4.
///
/// Wahr bei Identitaet, bei Praefix mit `/` am Ende des Cookie-Pfads
/// oder wenn das naechste Anfrage-Zeichen nach dem Praefix `/` ist.
pub fn pfad_passt(anfrage_pfad: &str, cookie_pfad: &str) -> bool {
    if anfrage_pfad == cookie_pfad {
        return true;
    }
    match anfrage_pfad.strip_prefix(cookie_pfad) {
        Some(rest) => cookie_pfad.ends_with('/') || rest.starts_with('/'),
        None => false,
    }
}

/// Standard-Pfad nach RFC 6265 §5.1.4.
///
/// Leer oder ohne `/` am Anfang heisst `/`; sonst alles bis zum letzten
/// `/` (ohne ihn). `/docs/lesen` wird zu `/docs`, `/docs` zu `/`.
pub fn standard_pfad(uri_pfad: &str) -> String {
    if uri_pfad.is_empty() || !uri_pfad.starts_with('/') {
        return "/".to_string();
    }
    match uri_pfad[1..].find('/') {
        // Nur ein `/` drin (`/docs`): Wurzel.
        None => "/".to_string(),
        // Bis zum LETZTEN `/`, ohne ihn.
        Some(_) => match uri_pfad.rfind('/') {
            Some(0) => "/".to_string(),
            Some(i) => uri_pfad[..i].to_string(),
            None => "/".to_string(),
        },
    }
}

/// Monatsname (RFC 6265 §5.1.1: jan..dec, Gross/Klein egal) zur Zahl.
fn monat_zahl(name: &str) -> Option<u32> {
    match name {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

/// Schaltjahr nach gregorianischer Regel (fuer die Tages-Pruefung).
fn ist_schaltjahr(jahr: i64) -> bool {
    (jahr % 4 == 0 && jahr % 100 != 0) || jahr % 400 == 0
}

/// Tage im Monat, Februar schaltjahr-abhaengig.
fn tage_im_monat(jahr: i64, monat: u32) -> u32 {
    match monat {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if ist_schaltjahr(jahr) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Tage seit 1970-01-01 (Howard Hinnants `days_from_civil`, ganzzahlig).
/// Jahr ist auf 1601..9999 begrenzt (RFC 6265 §5.1.1 + 4 Stellen),
/// Ueberlauf ist rechnerisch ausgeschlossen.
fn tage_seit_epoche(jahr: i64, monat: u32, tag: u32) -> i64 {
    let y = if monat <= 2 { jahr - 1 } else { jahr };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (monat as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + tag as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Liest ein Cookie-Datum nach RFC 6265 §5.1.1 und liefert Unix-Sekunden.
///
/// Versteht `Wed, 21 Oct 2026 07:28:00 GMT` ebenso wie kuerzere Formen
/// (`21 Oct 2026 07:28:00`): Begrenzer trennen Marken, Reihenfolge egal,
/// Monat als Name, Jahr zwei- (70-99 -> 19xx, 00-69 -> 20xx) oder
/// vierstellig. Wochentage werden ignoriert (sie sind Deko, kein Datum).
/// Unlesbares ergibt `None` — dann gilt das Attribut als nicht gesetzt
/// (§5.2.1), nicht der Cookie als kaputt.
pub fn cookie_datum_lesen(datum: &str) -> Option<u64> {
    // M4: Datum-Laenge deckeln, bevor Marken kopiert werden.
    if datum.len() > 256 {
        return None;
    }
    // Marken an RFC-Begrenzern trennen (M3: hoechstens MAX_DATUM_MARKEN).
    let mut marken: Vec<&str> = Vec::new();
    let bytes = datum.as_bytes();
    let mut start: Option<usize> = None;
    let ist_begrenzer = |b: u8| {
        b == 0x09
            || (0x20..=0x2F).contains(&b)
            || (0x3B..=0x40).contains(&b)
            || (0x5B..=0x60).contains(&b)
            || (0x7B..=0x7E).contains(&b)
    };
    for (i, &b) in bytes.iter().enumerate() {
        if ist_begrenzer(b) {
            if let Some(s) = start {
                marken.push(&datum[s..i]);
                if marken.len() > MAX_DATUM_MARKEN {
                    return None;
                }
                start = None;
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        marken.push(&datum[s..]);
    }

    let mut zeit: Option<(u32, u32, u32)> = None;
    let mut tag: Option<u32> = None;
    let mut monat: Option<u32> = None;
    let mut jahr: Option<i64> = None;

    // Ziffern-Anfang einer Marke: fuehrende Ziffernfolge (hoechstens
    // so viele, wie die Produktion erlaubt) plus Rest.
    for marke in marken {
        let klein = marke.to_ascii_lowercase();
        // 1. Zeit `hh:mm:ss` — nur wenn noch keine gefunden (§5.1.1).
        if zeit.is_none() {
            let teile: Vec<&str> = klein.split(':').collect();
            if teile.len() == 3 {
                // Jedes Feld: 1-2 Ziffern, danach nur Nicht-Ziffern.
                let mut felder: Vec<u32> = Vec::new();
                let mut passt = true;
                for t in teile {
                    let ziffern: String = t
                        .bytes()
                        .take_while(|b| b.is_ascii_digit())
                        .map(|b| b as char)
                        .collect();
                    let rest = &t[ziffern.len()..];
                    if ziffern.is_empty()
                        || ziffern.len() > 2
                        || !rest.bytes().all(|b| !b.is_ascii_digit())
                    {
                        passt = false;
                        break;
                    }
                    felder.push(ziffern.parse::<u32>().unwrap_or(99));
                }
                if passt && felder.len() == 3 {
                    zeit = Some((felder[0], felder[1], felder[2]));
                    continue;
                }
            }
        }
        // Fuehrende Ziffernfolge der Marke bestimmen.
        let ziffern: String = klein
            .bytes()
            .take_while(|b| b.is_ascii_digit())
            .map(|b| b as char)
            .collect();
        let rest = &klein[ziffern.len()..];
        let rest_hat_ziffer = rest.bytes().any(|b| b.is_ascii_digit());
        // 2. Tag `1*2DIGIT (non-digit *OCTET)`.
        if tag.is_none() && !ziffern.is_empty() && ziffern.len() <= 2 && !rest_hat_ziffer {
            if let Ok(t) = ziffern.parse::<u32>() {
                tag = Some(t);
                continue;
            }
        }
        // 3. Monat `(jan|...|dec) *OCTET`.
        if monat.is_none() && ziffern.is_empty() {
            // Marke beginnt mit Buchstaben: ersten drei nehmen.
            let name: String = klein.bytes().take(3).map(|b| b as char).collect();
            if let Some(m) = monat_zahl(&name) {
                monat = Some(m);
                continue;
            }
        }
        // 4. Jahr `2*4DIGIT (non-digit *OCTET)`.
        if jahr.is_none() && ziffern.len() >= 2 && ziffern.len() <= 4 && !rest_hat_ziffer {
            if let Ok(mut j) = ziffern.parse::<i64>() {
                if ziffern.len() == 2 {
                    if (70..=99).contains(&j) {
                        j += 1900;
                    } else if (0..=69).contains(&j) {
                        j += 2000;
                    }
                }
                jahr = Some(j);
            }
        }
    }

    let (h, min, sek) = zeit?;
    let tag = tag?;
    let monat = monat?;
    let jahr = jahr?;
    // Gueltigkeits-Schranken aus §5.1.1 Schritt 5.
    if !(1..=31).contains(&tag) || jahr < 1601 || jahr > 9999 {
        return None;
    }
    if h > 23 || min > 59 || sek > 59 {
        return None;
    }
    // Schritt 6: Das Datum muss existieren (30. Februar gibt es nicht).
    if tag > tage_im_monat(jahr, monat) {
        return None;
    }
    let tage = tage_seit_epoche(jahr, monat, tag);
    let sekunden = tage * 86_400 + h as i64 * 3_600 + min as i64 * 60 + sek as i64;
    u64::try_from(sekunden).ok()
}
