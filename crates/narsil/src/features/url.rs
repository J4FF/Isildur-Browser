//! Release 08.10.2026: FEATURE 7 (Link-Reiniger) und die Grundlage von
//! FEATURE 12 (Link-Ziel-Vorschau).
//!
//! * Tracking-Parameter entfernen (`utm_*`, `fbclid`, `gclid` …),
//! * Weiterleitungs-Adressen mit dem Ziel im Parameter auspacken
//!   (`google.com/url?q=`, `l.facebook.com/l.php?u=` …) — ohne Netz,
//! * Kurzlinks per `HEAD` aufloesen (Location-Kopf, `narsil_net::fetch_ein_sprung`),
//! * Umschreiben: Reddit → Redlib, YouTube → Invidious (Instanz einstellbar).

use super::{felder, lesen_text, ordner, schreiben, zeile};

/// Die Einstellungen der Umschreib-Regeln (`<profil>/features/umleitungen.tsv`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regeln {
    pub tracking: bool,
    pub kurzlinks: bool,
    pub reddit: bool,
    pub reddit_instanz: String,
    pub youtube: bool,
    pub youtube_instanz: String,
    /// Zusaetzliche Parameter, die entfernt werden (Namen, klein).
    pub eigene_parameter: Vec<String>,
}

impl Default for Regeln {
    fn default() -> Self {
        Regeln {
            tracking: true,
            kurzlinks: true,
            reddit: false,
            reddit_instanz: "https://redlib.catsarch.com".into(),
            youtube: false,
            youtube_instanz: "https://yewtu.be".into(),
            eigene_parameter: Vec::new(),
        }
    }
}

fn pfad() -> std::path::PathBuf {
    ordner().join("umleitungen.tsv")
}

impl Regeln {
    /// Aus dem Profil lesen.
    pub fn laden() -> Regeln {
        let mut r = Regeln::default();
        for z in lesen_text(&pfad()).lines().take(64) {
            let f = felder(z);
            if f.len() < 2 {
                continue;
            }
            let wert = f[1].trim();
            match f[0].as_str() {
                "tracking" => r.tracking = wert == "true",
                "kurzlinks" => r.kurzlinks = wert == "true",
                "reddit" => r.reddit = wert == "true",
                "youtube" => r.youtube = wert == "true",
                "reddit_instanz" if instanz_gueltig(wert) => r.reddit_instanz = wert.into(),
                "youtube_instanz" if instanz_gueltig(wert) => r.youtube_instanz = wert.into(),
                "parameter" => {
                    r.eigene_parameter = wert
                        .split(',')
                        .map(|p| p.trim().to_ascii_lowercase())
                        .filter(|p| !p.is_empty() && p.len() <= 64)
                        .take(64)
                        .collect();
                }
                _ => {}
            }
        }
        r
    }

    /// Ins Profil schreiben.
    pub fn speichern(&self) -> Result<(), String> {
        let ja = |b: bool| if b { "true" } else { "false" };
        let text = [
            zeile(&["tracking", ja(self.tracking)]),
            zeile(&["kurzlinks", ja(self.kurzlinks)]),
            zeile(&["reddit", ja(self.reddit)]),
            zeile(&["reddit_instanz", &self.reddit_instanz]),
            zeile(&["youtube", ja(self.youtube)]),
            zeile(&["youtube_instanz", &self.youtube_instanz]),
            zeile(&["parameter", &self.eigene_parameter.join(",")]),
        ]
        .join("\n");
        schreiben(&pfad(), text.as_bytes())
    }
}

/// Eine Instanz ist eine `https://`-Adresse ohne Pfad-Tricks.
pub fn instanz_gueltig(s: &str) -> bool {
    let s = s.trim();
    s.len() <= 200
        && (s.starts_with("https://") || s.starts_with("http://"))
        && !s.contains(['"', '<', '>', ' ', '?', '#'])
        && super::host_von(s).contains('.')
}

/// Die Tracking-Parameter (Namen klein; `*` am Ende = Praefix).
pub const TRACKING: &[&str] = &[
    "utm_*", "fbclid", "gclid", "gclsrc", "dclid", "gbraid", "wbraid", "msclkid", "mc_cid",
    "mc_eid", "igshid", "igsh", "yclid", "_hsenc", "_hsmi", "__hssc", "__hstc", "__hsfp",
    "hsctatracking", "mkt_tok", "oly_anon_id", "oly_enc_id", "vero_id", "vero_conv",
    "ref_src", "ref_url", "s_cid", "twclid", "ttclid", "epik", "wickedid", "srsltid",
    "pk_*", "piwik_*", "matomo_*", "mtm_*", "si", "spm", "scm", "_ga", "_gl", "trk",
    "trackingid", "campaign_id", "ad_id", "adset_id", "guccounter", "guce_referrer",
    "guce_referrer_sig",
];

/// Ist der Parametername ein Tracking-Parameter?
pub fn ist_tracking(name: &str, eigene: &[String]) -> bool {
    let n = name.to_ascii_lowercase();
    let passt = |muster: &str| match muster.strip_suffix('*') {
        Some(p) => n.starts_with(p),
        None => n == muster,
    };
    // `si` nur bei YouTube/Spotify-Freigaben — als Name ist es zu kurz, um es
    // ueberall zu entfernen; die Pruefung uebernimmt `reinigen` mit dem Host.
    TRACKING.iter().filter(|m| **m != "si").any(|m| passt(m)) || eigene.iter().any(|m| passt(m))
}

/// Die Tracking-Parameter aus einer Adresse entfernen. Der Anker bleibt.
pub fn tracking_entfernen(adresse: &str, eigene: &[String]) -> String {
    let (vorn, anker) = match adresse.split_once('#') {
        Some((v, a)) => (v, Some(a)),
        None => (adresse, None),
    };
    let Some((basis, abfrage)) = vorn.split_once('?') else {
        return adresse.to_string();
    };
    let host = super::host_von(adresse);
    let si_weg = host.ends_with("youtube.com") || host == "youtu.be" || host.ends_with("spotify.com");
    let behalten: Vec<&str> = abfrage
        .split('&')
        .filter(|t| !t.is_empty())
        .filter(|t| {
            let name = super::dekodiere(t.split('=').next().unwrap_or(""));
            !(ist_tracking(&name, eigene) || (si_weg && name == "si"))
        })
        .collect();
    let mut r = basis.to_string();
    if !behalten.is_empty() {
        r.push('?');
        r.push_str(&behalten.join("&"));
    }
    if let Some(a) = anker {
        r.push('#');
        r.push_str(a);
    }
    r
}

/// Bekannte Weiterleiter mit dem Ziel als Parameter: (Host-Ende, Pfad-Anfang, Parameter).
const WEITERLEITER: &[(&str, &str, &str)] = &[
    ("google.com", "/url", "q"),
    ("google.com", "/url", "url"),
    ("google.de", "/url", "q"),
    ("google.de", "/url", "url"),
    ("l.facebook.com", "/l.php", "u"),
    ("lm.facebook.com", "/l.php", "u"),
    ("l.instagram.com", "/", "u"),
    ("out.reddit.com", "/", "url"),
    ("youtube.com", "/redirect", "q"),
    ("t.umblr.com", "/redirect", "z"),
    ("steamcommunity.com", "/linkfilter", "url"),
    ("disq.us", "/url", "url"),
    ("click.linksynergy.com", "/", "murl"),
    ("deref-gmx.net", "/mail/client", "redirectUrl"),
    ("deref-web.de", "/mail/client", "redirectUrl"),
    ("safelinks.protection.outlook.com", "/", "url"),
];

/// Das Ziel einer Weiterleitungs-Adresse, wenn es im Parameter steht.
pub fn ziel_aus_parameter(adresse: &str) -> Option<String> {
    let host = super::host_von(adresse);
    let rest = adresse.split_once("://")?.1;
    let pfad = rest.find('/').map_or("/", |i| &rest[i..]);
    for (h, p, name) in WEITERLEITER {
        if (host == *h || host.ends_with(&format!(".{h}"))) && pfad.starts_with(p) {
            for (k, v) in super::abfrage(adresse) {
                if k == *name && (v.starts_with("http://") || v.starts_with("https://")) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Bekannte Kurzlink-Dienste.
pub const KURZLINKS: &[&str] = &[
    "bit.ly", "bitly.com", "t.co", "tinyurl.com", "goo.gl", "ow.ly", "buff.ly", "is.gd", "v.gd",
    "rebrand.ly", "lnkd.in", "shorturl.at", "cutt.ly", "tiny.cc", "rb.gy", "s.id", "t.ly",
    "trib.al", "dlvr.it", "amzn.to", "amzn.eu", "youtu.be", "fb.me", "ift.tt", "bl.ink",
    "short.io", "tiny.one", "shorte.st", "adf.ly", "t1p.de", "kurzelinks.de", "lmy.de",
    "spoti.fi", "apple.co", "wp.me", "mcaf.ee", "qr.ae", "x.co", "su.pr", "zpr.io",
];

/// Ist die Adresse ein Kurzlink?
pub fn ist_kurzlink(adresse: &str) -> bool {
    if !(adresse.starts_with("http://") || adresse.starts_with("https://")) {
        return false;
    }
    let host = super::host_von(adresse);
    let host = host.strip_prefix("www.").unwrap_or(&host);
    // youtu.be ist ein Kurzlink, wird aber lokal umgeschrieben (kein Netz noetig).
    host != "youtu.be" && KURZLINKS.contains(&host)
}

/// Ist die Adresse ein Tracking- oder Kurzlink (fuer die Vorschau, Feature 12)?
pub fn ist_verdaechtig(adresse: &str) -> bool {
    ist_kurzlink(adresse)
        || ziel_aus_parameter(adresse).is_some()
        || tracking_entfernen(adresse, &[]) != adresse
}

/// Umschreiben nach Redlib/Invidious, wenn eingeschaltet.
pub fn umschreiben(adresse: &str, r: &Regeln) -> String {
    let host = super::host_von(adresse);
    let rest = adresse.split_once("://").map_or("", |(_, x)| x);
    let pfad_ab = rest.find('/').map_or("/", |i| &rest[i..]);
    if r.reddit
        && (host == "reddit.com" || host.ends_with(".reddit.com"))
        && host != "out.reddit.com"
    {
        return format!("{}{}", r.reddit_instanz.trim_end_matches('/'), pfad_ab);
    }
    if r.youtube {
        let inst = r.youtube_instanz.trim_end_matches('/');
        if host == "youtu.be" {
            let id_und_rest = pfad_ab.trim_start_matches('/');
            let (id, abfrage) = id_und_rest.split_once('?').unwrap_or((id_und_rest, ""));
            if !id.is_empty() {
                let mut ziel = format!("{inst}/watch?v={id}");
                if !abfrage.is_empty() {
                    ziel.push('&');
                    ziel.push_str(abfrage);
                }
                return ziel;
            }
        }
        if host == "youtube.com" || host == "www.youtube.com" || host == "m.youtube.com"
            || host == "music.youtube.com"
        {
            if pfad_ab.starts_with("/watch") || pfad_ab.starts_with("/shorts/")
                || pfad_ab.starts_with("/channel/") || pfad_ab.starts_with("/playlist")
                || pfad_ab.starts_with("/@") || pfad_ab.starts_with("/results")
            {
                return format!("{inst}{pfad_ab}");
            }
        }
    }
    adresse.to_string()
}

/// Ohne Netz bereinigen: Weiterleiter auspacken, Tracking weg, umschreiben.
pub fn reinigen(adresse: &str, r: &Regeln) -> String {
    if !(adresse.starts_with("http://") || adresse.starts_with("https://")) {
        return adresse.to_string();
    }
    let mut a = adresse.to_string();
    for _ in 0..3 {
        match ziel_aus_parameter(&a) {
            Some(z) => a = z,
            None => break,
        }
    }
    if r.tracking {
        a = tracking_entfernen(&a, &r.eigene_parameter);
    }
    umschreiben(&a, r)
}

/// M3: hoechstens so viele Spruenge beim Aufloesen.
pub const MAX_SPRUENGE: u32 = 5;

/// Eine Adresse per `HEAD` (Location-Kopf) aufloesen — bis zu
/// [`MAX_SPRUENGE`] Spruenge, ohne einen Seitenkoerper zu laden.
pub fn per_head_aufloesen(adresse: &str) -> Result<String, String> {
    let mut aktuell = adresse.to_string();
    for _ in 0..MAX_SPRUENGE {
        if let Some(z) = ziel_aus_parameter(&aktuell) {
            aktuell = z;
            continue;
        }
        let anfrage = anfrage_fuer(&aktuell, true)?;
        let limits = narsil_net::Limits {
            gesamt: std::time::Duration::from_secs(8),
            verbinden: std::time::Duration::from_secs(5),
            body_bytes: 64 * 1024,
            ..narsil_net::Limits::default()
        };
        let mut antwort = narsil_net::fetch_ein_sprung(&anfrage, limits).map_err(|e| e.to_string())?;
        // Manche Kurzlink-Dienste beantworten HEAD mit 405: dann ein GET mit
        // demselben einen Sprung (der Koerper der Umleitung ist klein).
        if antwort.status.code == 405 || antwort.status.code == 501 {
            let get = anfrage_fuer(&aktuell, false)?;
            antwort = narsil_net::fetch_ein_sprung(&get, limits).map_err(|e| e.to_string())?;
        }
        if !antwort.is_redirect() {
            return Ok(aktuell);
        }
        let Some(ort) = antwort.redirect_location() else {
            return Ok(aktuell);
        };
        aktuell = narsil_net::loese_auf(&aktuell, ort).map_err(|e| e.to_string())?;
        if !(aktuell.starts_with("http://") || aktuell.starts_with("https://")) {
            return Err(narsil_i18n::trf!("The target is not a web address: {}", aktuell));
        }
    }
    Ok(aktuell)
}

fn anfrage_fuer(adresse: &str, head: bool) -> Result<narsil_net::Request, String> {
    let https = adresse.starts_with("https://");
    let rest = adresse.split_once("://").map(|(_, r)| r).ok_or_else(|| narsil_i18n::tr("no address").to_string())?;
    let (autoritaet, pfad) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let pfad = pfad.split('#').next().unwrap_or("/");
    let pfad = if pfad.starts_with('?') { format!("/{pfad}") } else { pfad.to_string() };
    let standard = if https { 443 } else { 80 };
    let autoritaet = autoritaet.rsplit('@').next().unwrap_or(autoritaet);
    let (host, port) = if let Some(innen) = autoritaet.strip_prefix('[') {
        let (h, nach) = innen.split_once(']').ok_or("IPv6-Adresse ohne ]")?;
        let port = nach.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(standard);
        (h.to_string(), port)
    } else {
        match autoritaet.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(n) => (h.to_string(), n),
                Err(_) => (autoritaet.to_string(), standard),
            },
            None => (autoritaet.to_string(), standard),
        }
    };
    if host.is_empty() {
        return Err(narsil_i18n::tr("no host").into());
    }
    let r = if head {
        narsil_net::Request::head(&host, port, &pfad)
    } else {
        narsil_net::Request::get(&host, port, &pfad)
    };
    Ok(if https { r.https() } else { r })
}

/// Bereinigen und Kurzlinks aufloesen (mit Netz, nur wenn eingeschaltet).
pub fn aufloesen_und_reinigen(adresse: &str, r: &Regeln) -> String {
    let sauber = reinigen(adresse, r);
    if r.kurzlinks && ist_kurzlink(&sauber) {
        if let Ok(ziel) = per_head_aufloesen(&sauber) {
            return reinigen(&ziel, r);
        }
    }
    sauber
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_parameter_fliegen_raus_der_rest_bleibt() {
        let a = "https://shop.example/p?id=7&utm_source=nl&utm_medium=mail&fbclid=XY&farbe=rot#bild";
        assert_eq!(tracking_entfernen(a, &[]), "https://shop.example/p?id=7&farbe=rot#bild");
        assert_eq!(tracking_entfernen("https://a.example/?gclid=1", &[]), "https://a.example/");
        assert_eq!(tracking_entfernen("https://a.example/x", &[]), "https://a.example/x");
        assert_eq!(
            tracking_entfernen("https://a.example/x?eigen=1&b=2", &["eigen".into()]),
            "https://a.example/x?b=2"
        );
        // `si` nur bei YouTube/Spotify.
        assert_eq!(tracking_entfernen("https://youtu.be/abc?si=xyz", &[]), "https://youtu.be/abc");
        assert_eq!(tracking_entfernen("https://wiki.example/?si=1", &[]), "https://wiki.example/?si=1");
    }

    #[test]
    fn weiterleiter_werden_ohne_netz_ausgepackt() {
        assert_eq!(
            ziel_aus_parameter("https://www.google.com/url?sa=t&q=https%3A%2F%2Fziel.example%2Fa%3Fb%3D1&ved=x")
                .as_deref(),
            Some("https://ziel.example/a?b=1")
        );
        assert_eq!(
            ziel_aus_parameter("https://l.facebook.com/l.php?u=https%3A%2F%2Fz.example%2F&h=AT0").as_deref(),
            Some("https://z.example/")
        );
        assert_eq!(ziel_aus_parameter("https://www.google.com/search?q=x"), None);
        let r = Regeln::default();
        assert_eq!(
            reinigen("https://www.google.com/url?q=https://z.example/?utm_source=g%26id=3", &r),
            "https://z.example/?id=3"
        );
    }

    #[test]
    fn kurzlinks_werden_erkannt() {
        assert!(ist_kurzlink("https://bit.ly/3abc"));
        assert!(ist_kurzlink("http://t.co/xyz"));
        assert!(!ist_kurzlink("https://bitly.example/3abc"));
        assert!(!ist_kurzlink("https://youtu.be/abc"));
        assert!(ist_verdaechtig("https://bit.ly/3abc"));
        assert!(ist_verdaechtig("https://a.example/?utm_source=x"));
        assert!(!ist_verdaechtig("https://a.example/seite"));
    }

    #[test]
    fn umschreiben_nach_redlib_und_invidious() {
        let mut r = Regeln::default();
        assert_eq!(umschreiben("https://www.reddit.com/r/rust/", &r), "https://www.reddit.com/r/rust/");
        r.reddit = true;
        r.youtube = true;
        r.reddit_instanz = "https://redlib.example/".into();
        r.youtube_instanz = "https://inv.example".into();
        assert_eq!(umschreiben("https://old.reddit.com/r/rust/comments/1", &r), "https://redlib.example/r/rust/comments/1");
        assert_eq!(umschreiben("https://www.youtube.com/watch?v=abc&t=10", &r), "https://inv.example/watch?v=abc&t=10");
        assert_eq!(umschreiben("https://youtu.be/abc?t=5", &r), "https://inv.example/watch?v=abc&t=5");
        assert_eq!(umschreiben("https://www.youtube.com/about", &r), "https://www.youtube.com/about");
        assert!(instanz_gueltig("https://yewtu.be"));
        assert!(!instanz_gueltig("javascript:alert(1)"));
        assert!(!instanz_gueltig("https://x"));
    }

    #[test]
    fn regeln_ueberleben_die_datei() {
        let dir = std::env::temp_dir().join(format!("narsil-test-url-{}", std::process::id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        let mut r = Regeln::default();
        r.reddit = true;
        r.youtube_instanz = "https://inv.example".into();
        r.eigene_parameter = vec!["ref".into(), "aff_*".into()];
        r.speichern().expect("speichern");
        assert_eq!(Regeln::laden(), r);
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn head_aufloesung_gegen_einen_lokalen_server() {
        use std::io::{Read, Write};
        let lauscher = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = lauscher.local_addr().expect("addr").port();
        let faden = std::thread::spawn(move || {
            let mut methoden = Vec::new();
            for runde in 0..2 {
                let (mut s, _) = lauscher.accept().expect("accept");
                let mut puffer = [0u8; 2048];
                let n = s.read(&mut puffer).unwrap_or(0);
                let anfrage = String::from_utf8_lossy(&puffer[..n]).to_string();
                methoden.push(anfrage.split(' ').next().unwrap_or("").to_string());
                let antwort = if runde == 0 {
                    format!("HTTP/1.1 301 Moved\r\nLocation: http://127.0.0.1:{port}/ziel?utm_source=x&id=9\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                };
                let _ = s.write_all(antwort.as_bytes());
            }
            methoden
        });
        let ziel = per_head_aufloesen(&format!("http://127.0.0.1:{port}/kurz")).expect("aufloesen");
        assert_eq!(ziel, format!("http://127.0.0.1:{port}/ziel?utm_source=x&id=9"));
        let methoden = faden.join().expect("Server");
        assert_eq!(methoden, vec!["HEAD".to_string(), "HEAD".to_string()], "nur HEAD-Anfragen");
        assert_eq!(reinigen(&ziel, &Regeln::default()), format!("http://127.0.0.1:{port}/ziel?id=9"));
    }
}
