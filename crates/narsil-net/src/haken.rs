//! Release 08.10.2026: DIE HAKEN DER NETZSCHICHT fuer den Tab „Features".
//!
//! Drei Dinge greifen hier in **jede** Anfrage ein, bevor sie den Rechner
//! verlaesst — und zwar an der einen Stelle, durch die jeder Abruf geht
//! (`hole_und_sende_runden`: Seite, Bilder, Blaetter, `fetch`, jede
//! Umleitung):
//!
//! 1. **Die Netzregel** (Feature 3, Netz-Sandbox): frei, nur lokal, gesperrt.
//!    Es gibt einen prozessweiten Wert (der aktive Tab, gesetzt von `narsil`)
//!    und einen Wert je Faden, der ihn ueberstimmt (Hintergrundaufgaben wie
//!    der Element-Waechter, Tests). Die Arbeiter von [`crate::parallel`]
//!    erben den Fadenwert ihres Auftraggebers.
//! 2. **Die Kopfzeilen-Regeln** (Feature 6): je Domain setzen oder
//!    entfernen, fuer Anfrage und Antwort.
//! 3. **Global Privacy Control** (Feature 11): `Sec-GPC: 1` und `DNT: 1`.
//!
//! Es gibt bewusst keine eingebauten Regeln: die Liste ist leer, bis der
//! Nutzer sie fuellt.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use crate::{NetError, Request, Response};

/// Was ein Tab ins Netz darf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Netzregel {
    /// Alles (der Normalfall).
    Frei,
    /// Nur der eigene Rechner: `localhost`, `*.localhost`, `127.0.0.0/8`, `::1`.
    NurLokal,
    /// Gar nichts.
    Gesperrt,
}

impl Netzregel {
    /// Die Zahl fuer Menue und Speicher (0 frei, 1 nur lokal, 2 gesperrt).
    pub fn als_zahl(self) -> u8 {
        match self {
            Netzregel::Frei => 0,
            Netzregel::NurLokal => 1,
            Netzregel::Gesperrt => 2,
        }
    }

    /// Aus der Zahl; Unbekanntes ist `Frei`.
    pub fn aus_zahl(n: u8) -> Netzregel {
        match n {
            1 => Netzregel::NurLokal,
            2 => Netzregel::Gesperrt,
            _ => Netzregel::Frei,
        }
    }
}

static GLOBALE_REGEL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    static FADEN_REGEL: Cell<Option<Netzregel>> = const { Cell::new(None) };
}

/// Die prozessweite Regel setzen (der aktive Tab).
pub fn regel_setzen(r: Netzregel) {
    GLOBALE_REGEL.store(r.als_zahl(), Ordering::SeqCst);
}

/// Die prozessweite Regel.
pub fn globale_regel() -> Netzregel {
    Netzregel::aus_zahl(GLOBALE_REGEL.load(Ordering::SeqCst))
}

/// Die Regel dieses Fadens setzen (`None` = die prozessweite gilt).
pub fn regel_im_faden(r: Option<Netzregel>) {
    FADEN_REGEL.with(|c| c.set(r));
}

/// Die Regel dieses Fadens, falls gesetzt.
pub fn faden_regel() -> Option<Netzregel> {
    FADEN_REGEL.with(Cell::get)
}

/// Die Regel, die fuer eine Anfrage aus diesem Faden gilt.
pub fn regel() -> Netzregel {
    faden_regel().unwrap_or_else(globale_regel)
}

/// Ist der Host der eigene Rechner?
pub fn ist_lokal(host: &str) -> bool {
    let h = crate::host_ohne_schema(host)
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") || h == "::1" {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// Darf eine Anfrage an `host` unter der Regel `r` hinaus?
pub fn erlaubt(r: Netzregel, host: &str) -> Result<(), NetError> {
    match r {
        Netzregel::Frei => Ok(()),
        Netzregel::NurLokal if ist_lokal(host) => Ok(()),
        Netzregel::NurLokal => Err(NetError::Connect(format!(
            "Netz-Sandbox: dieser Tab darf nur lokal ins Netz ({host} gesperrt)"
        ))),
        Netzregel::Gesperrt => Err(NetError::Connect(format!(
            "Netz-Sandbox: dieser Tab ist vom Netz getrennt ({host} gesperrt)"
        ))),
    }
}

/// Die Richtung einer Kopfzeilen-Regel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Richtung {
    /// Kopfzeile der Anfrage.
    Anfrage,
    /// Kopfzeile der Antwort.
    Antwort,
}

/// Eine Kopfzeilen-Regel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KopfRegel {
    /// Die Domain: `beispiel.de` gilt auch fuer `www.beispiel.de`; `*` fuer alle.
    pub domain: String,
    /// Anfrage oder Antwort.
    pub richtung: Richtung,
    /// Der Name der Kopfzeile (Gross/Klein egal).
    pub name: String,
    /// Der Wert — `None` heisst „entfernen".
    pub wert: Option<String>,
}

/// M3: hoechstens so viele Regeln.
pub const MAX_REGELN: usize = 256;
/// M4: die laengste Kopfzeile (Name plus Wert).
pub const MAX_KOPF: usize = 4096;

impl KopfRegel {
    /// Gilt die Regel fuer diesen Host?
    pub fn gilt_fuer(&self, host: &str) -> bool {
        let h = crate::host_ohne_schema(host).to_ascii_lowercase();
        let d = self.domain.trim().trim_start_matches("*.").to_ascii_lowercase();
        if d == "*" || d.is_empty() {
            return true;
        }
        h == d || h.ends_with(&format!(".{d}"))
    }

    /// Ist der Name eine erlaubte Kopfzeile (Token-Zeichen, keine
    /// Verbindungs-Kopfzeile, die das Protokoll selbst fuehrt)?
    pub fn ist_gueltig(&self) -> bool {
        let n = self.name.trim();
        if n.is_empty() || n.len() + self.wert.as_ref().map_or(0, String::len) > MAX_KOPF {
            return false;
        }
        if !n.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)) {
            return false;
        }
        let klein = n.to_ascii_lowercase();
        if matches!(
            klein.as_str(),
            "host" | "connection" | "content-length" | "transfer-encoding" | "upgrade" | "keep-alive"
        ) {
            return false;
        }
        self.wert
            .as_ref()
            .is_none_or(|w| !w.contains('\r') && !w.contains('\n'))
    }
}

fn regeln_speicher() -> &'static RwLock<Arc<Vec<KopfRegel>>> {
    static R: OnceLock<RwLock<Arc<Vec<KopfRegel>>>> = OnceLock::new();
    R.get_or_init(|| RwLock::new(Arc::new(Vec::new())))
}

/// Die Kopfzeilen-Regeln setzen (ungueltige werden verworfen, M3).
pub fn kopfregeln_setzen(regeln: Vec<KopfRegel>) {
    let gefiltert: Vec<KopfRegel> = regeln
        .into_iter()
        .filter(KopfRegel::ist_gueltig)
        .take(MAX_REGELN)
        .collect();
    match regeln_speicher().write() {
        Ok(mut g) => *g = Arc::new(gefiltert),
        Err(v) => *v.into_inner() = Arc::new(gefiltert),
    }
}

/// Die geltenden Kopfzeilen-Regeln.
pub fn kopfregeln() -> Arc<Vec<KopfRegel>> {
    match regeln_speicher().read() {
        Ok(g) => g.clone(),
        Err(v) => v.into_inner().clone(),
    }
}

static GPC: AtomicBool = AtomicBool::new(false);

/// `Sec-GPC: 1` und `DNT: 1` mitsenden (Feature 11)?
pub fn gpc_setzen(an: bool) {
    GPC.store(an, Ordering::SeqCst);
}

/// Werden die Datenschutz-Signale gesendet?
pub fn gpc() -> bool {
    GPC.load(Ordering::SeqCst)
}

/// Die Anfrage-Regeln auf eine Anfrage anwenden (rein, ohne Zustand).
pub fn anfrage_regeln_anwenden(anfrage: &mut Request, regeln: &[KopfRegel], gpc: bool) {
    if gpc {
        anfrage.headers.insert("sec-gpc".into(), "1".into());
        anfrage.headers.insert("dnt".into(), "1".into());
    }
    for r in regeln.iter().filter(|r| r.richtung == Richtung::Anfrage) {
        if !r.gilt_fuer(&anfrage.host) {
            continue;
        }
        let name = r.name.trim().to_ascii_lowercase();
        // Gross/Klein: eine vorhandene Schreibweise wird ersetzt, nicht
        // verdoppelt.
        let vorhanden: Vec<String> = anfrage
            .headers
            .keys()
            .filter(|k| k.eq_ignore_ascii_case(&name))
            .cloned()
            .collect();
        for k in vorhanden {
            anfrage.headers.remove(&k);
        }
        if let Some(w) = &r.wert {
            anfrage.headers.insert(name, w.clone());
        } else if name == "user-agent" || name == "accept-language" || name == "accept" {
            // Ohne Eintrag schriebe `felder_mit_vorgaben` die Vorgabe des
            // Browsers wieder hinein — „entfernen" heisst hier „leer senden".
            anfrage.headers.insert(name, String::new());
        }
    }
}

/// Die Antwort-Regeln auf eine Antwort anwenden (nur Entfernen und Setzen).
pub fn antwort_regeln_anwenden(host: &str, antwort: &mut Response, regeln: &[KopfRegel]) {
    for r in regeln.iter().filter(|r| r.richtung == Richtung::Antwort) {
        if !r.gilt_fuer(host) {
            continue;
        }
        let name = r.name.trim().to_ascii_lowercase();
        let vorhanden: Vec<String> = antwort
            .headers
            .keys()
            .filter(|k| k.eq_ignore_ascii_case(&name))
            .cloned()
            .collect();
        for k in vorhanden {
            antwort.headers.remove(&k);
        }
        if name == "set-cookie" && r.wert.is_none() {
            antwort.set_cookie.clear();
        }
        if let Some(w) = &r.wert {
            antwort.headers.insert(name, w.clone());
        }
    }
}

/// Eine Sperre fuer einzelne Adressen (Feature 9: Chat- und Push-Skripte).
/// Die Entscheidung trifft der Aufrufer (`narsil` mit `narsil-adblock`); die
/// Netzschicht kennt keine Filterliste.
pub type Sperre = Arc<dyn Fn(&str) -> bool + Send + Sync>;

fn sperre_speicher() -> &'static RwLock<Option<Sperre>> {
    static S: OnceLock<RwLock<Option<Sperre>>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(None))
}

/// Die Adress-Sperre setzen oder (`None`) aufheben.
pub fn sperre_setzen(s: Option<Sperre>) {
    match sperre_speicher().write() {
        Ok(mut g) => *g = s,
        Err(v) => *v.into_inner() = s,
    }
}

fn sperre() -> Option<Sperre> {
    match sperre_speicher().read() {
        Ok(g) => g.clone(),
        Err(v) => v.into_inner().clone(),
    }
}

/// VOR DEM SENDEN: Regel pruefen, Kopfzeilen anpassen. `Ok(None)` heisst
/// „unveraendert senden".
pub fn vor_dem_senden(anfrage: &Request) -> Result<Option<Request>, NetError> {
    erlaubt(regel(), &anfrage.host)?;
    if let Some(f) = sperre() {
        let url = crate::anfrage_url(anfrage);
        if f(&url) {
            return Err(NetError::Connect(format!("Bitte nicht stören: {url} gesperrt")));
        }
    }
    let regeln = kopfregeln();
    let gpc = gpc();
    if regeln.is_empty() && !gpc {
        return Ok(None);
    }
    let mut neu = anfrage.clone();
    anfrage_regeln_anwenden(&mut neu, &regeln, gpc);
    Ok(Some(neu))
}

/// NACH DEM EMPFANG: die Antwort-Regeln.
pub fn nach_dem_empfang(anfrage: &Request, antwort: &mut Response) {
    let regeln = kopfregeln();
    if !regeln.is_empty() {
        antwort_regeln_anwenden(&anfrage.host, antwort, &regeln);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regel(domain: &str, name: &str, wert: Option<&str>) -> KopfRegel {
        KopfRegel {
            domain: domain.into(),
            richtung: Richtung::Anfrage,
            name: name.into(),
            wert: wert.map(str::to_string),
        }
    }

    #[test]
    fn lokal_ist_nur_der_eigene_rechner() {
        for h in ["localhost", "app.localhost", "127.0.0.1", "127.5.4.3", "[::1]", "::1", "LOCALHOST"] {
            assert!(ist_lokal(h), "{h}");
        }
        for h in ["example.com", "10.0.0.1", "192.168.1.1", "localhost.example.com", "128.0.0.1"] {
            assert!(!ist_lokal(h), "{h}");
        }
    }

    #[test]
    fn die_regeln_sperren_was_sie_sollen() {
        assert!(erlaubt(Netzregel::Frei, "example.com").is_ok());
        assert!(erlaubt(Netzregel::NurLokal, "127.0.0.1").is_ok());
        assert!(erlaubt(Netzregel::NurLokal, "example.com").is_err());
        assert!(erlaubt(Netzregel::Gesperrt, "127.0.0.1").is_err());
        for n in 0..3 {
            assert_eq!(Netzregel::aus_zahl(n).als_zahl(), n);
        }
    }

    #[test]
    fn die_fadenregel_ueberstimmt_die_globale_und_greift_im_abruf() {
        regel_im_faden(Some(Netzregel::Gesperrt));
        assert_eq!(super::regel(), Netzregel::Gesperrt);
        // Der echte Abruf scheitert, bevor ein Sockel geoeffnet wird.
        let r = crate::fetch(&Request::get("example.invalid", 80, "/"));
        match r {
            Err(NetError::Connect(t)) => assert!(t.contains("Netz-Sandbox"), "{t}"),
            anders => panic!("nicht gesperrt: {anders:?}"),
        }
        regel_im_faden(Some(Netzregel::NurLokal));
        let r = crate::fetch(&Request::get("example.invalid", 80, "/"));
        assert!(matches!(r, Err(NetError::Connect(t)) if t.contains("nur lokal")));
        regel_im_faden(None);
        assert_eq!(super::regel(), globale_regel());
    }

    #[test]
    fn kopfregeln_setzen_ersetzen_und_entfernen() {
        let mut a = Request::get("www.beispiel.de", 443, "/");
        a.headers.insert("User-Agent".into(), "alt".into());
        a.headers.insert("referer".into(), "https://quelle/".into());
        let regeln = vec![
            regel("beispiel.de", "user-agent", Some("Mobil/1.0")),
            regel("beispiel.de", "Referer", None),
            regel("anderes.de", "x-nie", Some("1")),
            regel("*", "X-Eigen", Some("ja")),
        ];
        anfrage_regeln_anwenden(&mut a, &regeln, true);
        assert_eq!(a.headers.get("user-agent").map(String::as_str), Some("Mobil/1.0"));
        assert!(!a.headers.contains_key("User-Agent"));
        assert!(!a.headers.contains_key("referer"));
        assert!(!a.headers.contains_key("x-nie"));
        assert_eq!(a.headers.get("x-eigen").map(String::as_str), Some("ja"));
        assert_eq!(a.headers.get("sec-gpc").map(String::as_str), Some("1"));
        assert_eq!(a.headers.get("dnt").map(String::as_str), Some("1"));
        // Der Draht: die Vorgabe „Narsil/0.1" wird nicht wieder eingesetzt.
        let felder = crate::felder_mit_vorgaben(&a);
        let ua: Vec<&(String, String)> = felder.iter().filter(|(k, _)| k == "user-agent").collect();
        assert_eq!(ua.len(), 1);
        assert_eq!(ua[0].1, "Mobil/1.0");
    }

    #[test]
    fn ungueltige_regeln_werden_verworfen() {
        assert!(!regel("a.de", "Host", Some("x")).ist_gueltig());
        assert!(!regel("a.de", "X Leer", Some("x")).ist_gueltig());
        assert!(!regel("a.de", "X-A", Some("x\r\nInjiziert: 1")).ist_gueltig());
        assert!(regel("a.de", "X-A", Some("x")).ist_gueltig());
        assert!(regel("a.de", "Referer", None).ist_gueltig());
        assert!(regel("beispiel.de", "a", None).gilt_fuer("sub.beispiel.de"));
        assert!(!regel("beispiel.de", "a", None).gilt_fuer("nichtbeispiel.de"));
    }

    #[test]
    fn antwortregeln_entfernen_kopfzeilen() {
        let mut antwort = Response::leer(200);
        antwort.headers.insert("x-tracking".into(), "1".into());
        antwort.headers.insert("content-type".into(), "text/html".into());
        let regeln = vec![KopfRegel {
            domain: "beispiel.de".into(),
            richtung: Richtung::Antwort,
            name: "X-Tracking".into(),
            wert: None,
        }];
        antwort_regeln_anwenden("beispiel.de", &mut antwort, &regeln);
        assert!(!antwort.headers.contains_key("x-tracking"));
        assert!(antwort.headers.contains_key("content-type"));
    }
}
