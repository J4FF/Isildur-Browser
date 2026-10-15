//! Alt-Svc-Header parsen (RFC 7838).
//!
//! Der Browser braucht aus einem `Alt-Svc`-Antwortkopf den HTTP/3-Eintrag
//! (`h3=":443"; ma=86400`), um die Verbindung auf QUIC zu heben (Q6). Diese
//! Funktion liefert genau das: Host, Port und `ma` (max-age). Die Anbindung
//! im Netz macht spaeter der TLS-Agent.
//!
//! M3/M4: Kopf-, Eintrags- und Hostlaenge sind begrenzt, jede Zahl wird
//! geprueft, bevor sie in den Typ geht.

/// Der Port, den `h3` ohne Angabe bedeutet (U3, 01.10.2026).
///
/// RFC 7838 Abschnitt 2.2: "If the port is omitted, the default port for the
/// protocol is used." Fuer `h3` ist das 443.
///
/// DER GRUND FUER EINE KONSTANTE: `alt_svc_lesen` gibt **port = 0** zurueck,
/// wenn der Header keinen Port nennt, und setzt **nicht** 443. Das ist
/// getrennt: der Parser sagt, was **in** dem Header stand, und der Aufrufer
/// entscheidet, was er daraus macht. Wer beides in eine Zahl presst, kann
/// nicht mehr unterscheiden zwischen "der Server sagte Port 0" (Unsinn) und
/// "der Server sagte keinen Port" (Regelfall).
pub const H3_STD_PORT: u16 = 443;

/// Groesste Laenge des ganzen Header-Werts in Byte (M4).
pub const MAX_ALT_SVC_LAENGE: usize = 8192;
/// Hoechste Zahl von Alternative-Service-Eintraegen (M3).
pub const MAX_ALT_SVC_EINTRAEGE: usize = 16;
/// Groesste Host-Laenge (M4, wie SNI in narsil-tls).
pub const MAX_ALT_SVC_HOST: usize = 253;

/// Ein geparster `h3`-Eintrag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AltSvc {
    /// Der Host; leer = derselbe wie die angefragte URL.
    pub host: String,
    pub port: u16,
    /// `ma` (max-age) in Sekunden; 0, wenn nicht angegeben.
    pub max_alter: u64,
}

/// Warum der Header nicht gelesen werden kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AltSvcFehler {
    ZuLang { ist: usize, maximum: usize },
    ZuVieleEintraege,
    HostZuLang { ist: usize, maximum: usize },
    PortFalsch,
    Unlesbar,
    KeinH3,
}

impl std::fmt::Display for AltSvcFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AltSvcFehler::ZuLang { ist, maximum } => {
                write!(f, "Alt-Svc ist {ist} Byte, erlaubt sind {maximum}")
            }
            AltSvcFehler::ZuVieleEintraege => write!(f, "zu viele Alt-Svc-Eintraege"),
            AltSvcFehler::HostZuLang { ist, maximum } => {
                write!(f, "Alt-Svc-Host ist {ist} Byte, erlaubt sind {maximum}")
            }
            AltSvcFehler::PortFalsch => write!(f, "Alt-Svc-Port ist ungueltig"),
            AltSvcFehler::Unlesbar => write!(f, "Alt-Svc ist unlesbar"),
            AltSvcFehler::KeinH3 => write!(f, "kein h3-Eintrag im Alt-Svc"),
        }
    }
}

impl std::error::Error for AltSvcFehler {}

/// Parst den Wert eines `Alt-Svc`-Headers und liefert den ersten `h3`-Eintrag.
pub fn alt_svc_lesen(wert: &str) -> Result<AltSvc, AltSvcFehler> {
    if wert.len() > MAX_ALT_SVC_LAENGE {
        return Err(AltSvcFehler::ZuLang {
            ist: wert.len(),
            maximum: MAX_ALT_SVC_LAENGE,
        });
    }
    let mut anzahl = 0usize;
    for eintrag in wert.split(',') {
        anzahl += 1;
        if anzahl > MAX_ALT_SVC_EINTRAEGE {
            return Err(AltSvcFehler::ZuVieleEintraege);
        }
        let eintrag = eintrag.trim();
        if eintrag.is_empty() {
            continue;
        }
        let (protokoll, rest) = eintrag.split_once('=').ok_or(AltSvcFehler::Unlesbar)?;
        // Nur h3 (auch h3-29 usw.) interessiert.
        if !protokoll.trim().starts_with("h3") {
            continue;
        }
        return h3_eintrag_lesen(rest);
    }
    Err(AltSvcFehler::KeinH3)
}

/// Liest den Rest eines `h3`-Eintrags: `"host:port" *( ";" param )`.
fn h3_eintrag_lesen(rest: &str) -> Result<AltSvc, AltSvcFehler> {
    let rest = rest.trim();
    // Der alt-value ist ein Quoted-String.
    let (autoritaet, parameter) = match rest.find(';') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let autoritaet = autoritaet.trim();
    let innen = autoritaet
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or(AltSvcFehler::Unlesbar)?;

    // Die Hostlaenge wird **in** `zerlege_autoritaet` geprueft, denn dort
    // entsteht der Host aus fremden Daten. Ein zweiter Aufrufer-Check waere
    // eine Zusage an zwei Stellen.
    let (host, port) = zerlege_autoritaet(innen)?;

    let mut max_alter = 0u64;
    for param in parameter.split(';') {
        let param = param.trim();
        if let Some((name, wert)) = param.split_once('=') {
            if name.trim() == "ma" {
                max_alter = wert.trim().parse().unwrap_or(0);
            }
        }
    }

    Ok(AltSvc {
        host: host.to_string(),
        port,
        max_alter,
    })
}

/// Zerlegt `host:port` in (host, port). Leerer Host bei `:port`; IPv6 in
/// `[::1]:port`.
fn zerlege_autoritaet(innen: &str) -> Result<(String, u16), AltSvcFehler> {
    let (host, port_teil) = if let Some(rest) = innen.strip_prefix('[') {
        // [IPv6]:port
        let schluss = rest.find(']').ok_or(AltSvcFehler::Unlesbar)?;
        let host = &rest[..schluss];
        let port_teil = rest[schluss + 1..]
            .strip_prefix(':')
            .ok_or(AltSvcFehler::PortFalsch)?;
        (host, port_teil)
    } else {
        // host:port oder :port
        match innen.rfind(':') {
            Some(i) => (&innen[..i], &innen[i + 1..]),
            // `h3="beispiel.de"` — **ohne** Port. RFC 7838 2.2 erlaubt das,
            // und es bedeutet "derselbe Host, Standardport". Vorher stand hier
            // `return Err(AltSvcFehler::PortFalsch)`, und damit wurde der
            // gaengigste Alt-Svc-Form-Weg als kaputt gemeldet. Gefangen von
            // `der_angefragte_host_ist_der_fallback` in tests/u3_alt_svc.rs.
            //
            // Port **0** als "nicht angegeben"; `H3_STD_PORT` setzt der
            // Aufrufer. Siehe die Konstante oben.
            None => (innen, ""),
        }
    };
    // `h3=""` (leerer Host, kein Port) und `h3="beispiel.de"` ergeben beide
    // **port = 0** = "nicht angegeben". Nur ein **geschriebener** Port 0 wie
    // in `h3="beispiel.de:0"` ist ein Fehler, denn das ist keine gueltige
    // Zieladresse.
    let port = if port_teil.is_empty() {
        0
    } else {
        let p = port_teil
            .parse::<u16>()
            .map_err(|_| AltSvcFehler::PortFalsch)?;
        if p == 0 {
            return Err(AltSvcFehler::PortFalsch);
        }
        p
    };
    if host.len() > MAX_ALT_SVC_HOST {
        return Err(AltSvcFehler::HostZuLang {
            ist: host.len(),
            maximum: MAX_ALT_SVC_HOST,
        });
    }
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Standardfall aus dem Auftrag: `h3=":443"; ma=86400`.
    #[test]
    fn h3_mit_port_und_ma() {
        let a = alt_svc_lesen("h3=\":443\"; ma=86400").expect("lesen");
        assert_eq!(
            a,
            AltSvc {
                host: String::new(),
                port: 443,
                max_alter: 86400,
            }
        );
    }

    /// Mehrere Eintraege: der h3-Eintrag wird gefunden.
    #[test]
    fn h3_zwischen_anderen() {
        let a = alt_svc_lesen("h2=\":443\"; ma=0, h3=\":443\"; ma=3600").expect("lesen");
        assert_eq!(a.port, 443);
        assert_eq!(a.max_alter, 3600);
    }

    /// Ein Hostname und ein anderer Port.
    #[test]
    fn h3_mit_host() {
        let a = alt_svc_lesen("h3=\"alt.example.com:8443\"; ma=60").expect("lesen");
        assert_eq!(
            a,
            AltSvc {
                host: "alt.example.com".into(),
                port: 8443,
                max_alter: 60,
            }
        );
    }

    /// Ein IPv6-Literal in Klammern.
    #[test]
    fn h3_mit_ipv6() {
        let a = alt_svc_lesen("h3=\"[::1]:443\"; ma=0").expect("lesen");
        assert_eq!(a.host, "::1");
        assert_eq!(a.port, 443);
    }

    /// Kein h3: benannter Fehler.
    #[test]
    fn kein_h3_ist_ein_fehler() {
        assert_eq!(alt_svc_lesen("h2=\":443\""), Err(AltSvcFehler::KeinH3));
    }

    /// Port 0 und Unsinn sind benannte Fehler.
    #[test]
    fn boese_eingaben() {
        assert_eq!(alt_svc_lesen("h3=\":0\""), Err(AltSvcFehler::PortFalsch));
        assert_eq!(alt_svc_lesen("h3=kaputt"), Err(AltSvcFehler::Unlesbar));
        assert_eq!(
            alt_svc_lesen(&"x".repeat(MAX_ALT_SVC_LAENGE + 1)),
            Err(AltSvcFehler::ZuLang {
                ist: MAX_ALT_SVC_LAENGE + 1,
                maximum: MAX_ALT_SVC_LAENGE
            })
        );
    }
}
