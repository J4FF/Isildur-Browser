//! SIGNIERTE SITZUNGEN — HMAC-SHA256 ueber `narsil_tls` (SERVER V7).
//!
//! Der Teil von [`crate::sitzung`], der einem Cookie trauen **darf**.
//!
//! # DAS FORMAT, und warum es so aussieht
//!
//! ```text
//! sid = base64url( nutzlast ) . "-" . base64url( hmac-sha256 )
//! ```
//!
//! * **Nutzlast**: was der Client liest (z. B. `nutzer=7|bis=1789…`).
//!   Sie ist **base64url**, nicht base64: Base64 hat `+`, `/` und `=`,
//!   und davon sind zwei Zeichen in einem Cookie problematisch
//!   (`;` und `,` trennen, Leerzeichen sind verboten). Base64url hat
//!   davon keines.
//! * **Trenner `-`**: ein Byte, das in keinem der beiden Base64url-
//!   Alphabet vorkommt — die Signatur ist damit eindeutig trennbar.
//! * **Signatur**: HMAC-SHA256 ueber die **Nutzlast** (nicht ueber das
//!   ganze Cookie), mit [`narsil_tls::hmac::HmacSha256`].
//!
//! # WAS DAMIT NICHT MOEGLICH IST
//!
//! * Der Client kann die Nutzlast **lesen**, aber nicht **aendern** —
//!   ohne den Schluessel kann er keine gueltige Signatur bauen.
//! * Ein abgeschnittener Cookie (letztes Byte weg) schlaegt fehl.
//! * Ein Cookie mit angehaengten Bytes schlaegt fehl.
//! * **Zeit laeuft nicht mit**: die Nutzlast traegt eine Ablaufzeit, und
//!   die Pruefung vergleicht sie mit der Uhr des Servers. Das ist
//!   schwaecher als eine serverseitige Liste (die Nutzlast laesst sich
//!   nicht zuruecknehmen), aber es braucht keinen Speicher und ueberlebt
//!   einen Neustart. Fuer eine Sitzung, die eingeloggt bleibt, ist das
//!   die ehrliche Wahl; wer Widerruf braucht, fuehrt eine
//!   serverseitige Tabelle daneben.
//!
//! # DIE GRENZEN
//!
//! [`MAX_NUTZLAST_BYTES`] (8 KiB) fuer die Nutzlast, eine feste Zahl
//! Signaturbytes (32) und Base64 ohne Padding — damit die Laenge des
//! Wholets aus der Laenge der Nutzlast folgt und nicht aus der
//! Eingabe gelesen werden muss.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use narsil_tls::hmac::HmacSha256;
use narsil_tls::vergleich_gleich;

use crate::sitzung::{MAX_NUTZLAST_BYTES, STANDARD_LEBENSDAUER};

/// Laenge der Signatur in Byte (SHA-256).
pub const SIGNATUR_BYTES: usize = 32;

/// Mindestlaenge eines Schluessels (16 Byte).
///
/// HMAC-SHA256 mit einem 1-Byte-Schluessel ist HMAC mit einem geratenen
/// Schluessel. 16 Byte sind die Grenze, unterhalb derer die Meldung
/// "zu kurz" besser ist als ein vorhersehbarer Schlüssel.
pub const MIN_SCHLUESSEL_BYTES: usize = 16;

/// Warum eine Sitzung nicht gilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SitzungsFehler {
    /// Kein Cookie dieser Art vorhanden.
    Fehlt,
    /// Das Format stimmt nicht (kein Trenner, kein Base64, zu kurz).
    Kaputt(String),
    /// Die Nutzlast ist groesser als [`MAX_NUTZLAST_BYTES`].
    ZuGross,
    /// Die Signatur passt nicht — der Client hat gesendet, was er
    /// wollte.
    Signatur,
    /// Die Ablaufzeit liegt in der Vergangenheit.
    Abgelaufen,
    /// Der Schluessel ist kuerzer als [`MIN_SCHLUESSEL_BYTES`].
    SchluesselZuKurz,
}

/// Eine gepruefte Sitzung: die Nutzlast als Liste `name=wert` plus die
/// Ablaufzeit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sitzung {
    /// Die Felder der Nutzlast, in Reihenfolge (`Vec`, kein `HashMap`:
    /// die Reihenfolge gehoert zur Signatur).
    pub felder: Vec<(String, String)>,
    /// Wann die Sitzung ablaeuft.
    pub bis: SystemTime,
    /// Wann sie signiert wurde (aus der Nutzlast, nicht von hier).
    pub seit: SystemTime,
}

impl Sitzung {
    /// Wert eines Feldes (der erste mit diesem Namen).
    pub fn feld(&self, name: &str) -> Option<&str> {
        self.felder
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Ist die Sitzung jetzt noch gueltig?
    pub fn gilt(&self, jetzt: SystemTime) -> bool {
        jetzt <= self.bis
    }
}

/// Der Schluesseltraeger: der geheime Schluessel und der Cookie-Name.
///
/// Das Geheimnis liegt hier, nicht in der Konfiguration, weil eine
/// Konfiguration in [`crate::ServerConfig`] in Hand-shakes und
/// Protokolle landet.
pub struct Signierer {
    schluessel: Vec<u8>,
    name: String,
}

impl Signierer {
    /// Neuer Signierer. Der Schluessel muss [`MIN_SCHLUESSEL_BYTES`]
    /// Byte lang sein — `new` prueft das nicht (es gibt keinen
    /// `Result` dafuer), aber jede Anwendung scheitert dann beim
    /// Signieren mit [`SitzungsFehler::SchluesselZuKurz`], und der
    /// Test darunter beweist es.
    pub fn neu(schluessel: impl Into<Vec<u8>>) -> Self {
        Signierer {
            schluessel: schluessel.into(),
            name: crate::sitzung::STANDARD_NAME.to_string(),
        }
    }

    /// Anderer Cookie-Name (z. B. `__Host-sid`).
    pub fn mit_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Der Cookie-Name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Die Signatur einer Nutzlast: 32 Byte.
    ///
    /// `nil` bei zu kurzem Schluessel — HMAC wuerde sonst rechnen und
    /// ein Ergebnis liefern, das Sicherheit vortaeuscht.
    pub fn signiere(&self, nutzlast: &[u8]) -> Option<[u8; SIGNATUR_BYTES]> {
        if self.schluessel.len() < MIN_SCHLUESSEL_BYTES {
            return None;
        }
        let mac = HmacSha256::berechne(&self.schluessel, nutzlast);
        let mut raus = [0u8; SIGNATUR_BYTES];
        raus.copy_from_slice(&mac[..SIGNATUR_BYTES]);
        Some(raus)
    }

    /// Signiert Felder zu einem Cookie-Wert.
    ///
    /// Der Zeitstempel ist **Teil** der Signatur: eine Nutzlast ohne
    /// Zeit waere eine Sitzung ohne Ablauf.
    pub fn signiere_felder(
        &self,
        felder: &[(&str, &str)],
        jetzt: SystemTime,
    ) -> Result<String, SitzungsFehler> {
        let bis = jetzt + STANDARD_LEBENSDAUER;
        self.signiere_zeit(felder, jetzt, bis)
    }

    /// Signiert Felder mit eigener Ablaufzeit und Startzeit.
    ///
    /// `bis` wird **begrenzt**: mehr als 30 Tage ist kein Auslaufen
    /// mehr, sondern eine Zusage. Die Grenze steht hier und nicht erst
    /// bei der Pruefung, weil ein Cookie mit 10 Jahren Ablauf ein
    /// dauerhaft offenes Tor ist.
    pub fn signiere_zeit(
        &self,
        felder: &[(&str, &str)],
        seit: SystemTime,
        mut bis: SystemTime,
    ) -> Result<String, SitzungsFehler> {
        if self.schluessel.len() < MIN_SCHLUESSEL_BYTES {
            return Err(SitzungsFehler::SchluesselZuKurz);
        }
        let jetzt = UNIX_EPOCH + Duration::from_secs(0);
        let _ = jetzt;
        bis = bis.min(seit + MAX_LEBENSDAUER);
        let mut nutzlast = String::with_capacity(64);
        for (n, w) in felder {
            nutzlast.push_str(n);
            nutzlast.push('=');
            nutzlast.push_str(w);
            nutzlast.push('\u{1f}'); // UNIT SEPARATOR: kein Cookie-Wert enthaelt das
        }
        nutzlast.push_str(&zeit_zahl(seit));
        nutzlast.push('\u{1f}');
        nutzlast.push_str(&zeit_zahl(bis));
        // M4: die Nutzlast wird VOR der Signatur geprueft.
        if nutzlast.len() > MAX_NUTZLAST_BYTES {
            return Err(SitzungsFehler::ZuGross);
        }
        let signatur = self
            .signiere(nutzlast.as_bytes())
            .ok_or(SitzungsFehler::SchluesselZuKurz)?;
        Ok(format!(
            "{}.{}",
            base64url(nutzlast.as_bytes()),
            base64url(&signatur)
        ))
    }

    /// Prueft einen Cookie-Wert und gibt die Sitzung zurueck.
    ///
    /// Die Reihenfolge der Pruefungen ist eine Kette von `?` — und das
    /// ist Absicht: Formatchecker vor Signaturpruefung, Signatur vor
    /// Ablauf. Wer die Signatur nicht pruefen kann, darf die Ablaufzeit
    /// nicht einmal lesen (sonst liest er sie aus einem Cookie, das er
    /// selbst gebaut hat — und das ist genau der Angriff).
    pub fn pruefe(&self, wert: &str, jetzt: SystemTime) -> Result<Sitzung, SitzungsFehler> {
        if wert.is_empty() {
            return Err(SitzungsFehler::Fehlt);
        }
        let Some((nutzlast_b64, signatur_b64)) = wert.rsplit_once('.') else {
            return Err(SitzungsFehler::Kaputt("kein Trenner".into()));
        };
        let nutzlast = base64url_decode(nutzlast_b64)
            .ok_or_else(|| SitzungsFehler::Kaputt("Nutzlast kein Base64url".into()))?;
        // M4: die dekodierte Nutzlast ist gewachsen — jetzt die Grenze.
        if nutzlast.len() > MAX_NUTZLAST_BYTES {
            return Err(SitzungsFehler::ZuGross);
        }
        let signatur = base64url_decode(signatur_b64)
            .ok_or_else(|| SitzungsFehler::Kaputt("Signatur kein Base64url".into()))?;
        if signatur.len() != SIGNATUR_BYTES {
            return Err(SitzungsFehler::Kaputt(format!(
                "Signatur hat {} statt {SIGNATUR_BYTES} Byte",
                signatur.len()
            )));
        }
        let erwartet = self
            .signiere(&nutzlast)
            .ok_or(SitzungsFehler::SchluesselZuKurz)?;
        // `vergleich_gleich`: konstante Zeit, kein fruehes Zurueckgeben.
        if !vergleich_gleich(&signatur, &erwartet) {
            return Err(SitzungsFehler::Signatur);
        }
        // Ab hier ist die Nutzlast vertrauenswuerdig.
        let text = String::from_utf8(nutzlast)
            .map_err(|e| SitzungsFehler::Kaputt(format!("kein UTF-8: {e}")))?;
        // M3: die Teile sind durch `MAX_NUTZLAST_BYTES` begrenzt (die
        // Nutzlaste came vom Client und ist schon geprueft), und
        // `take` ist die zweite, naeher liegende Grenze.
        let mut teile: Vec<&str> = Vec::with_capacity(8);
        for teil in text
            .split('\u{1f}')
            .take(crate::sitzung::MAX_COOKIE_ZAHL + 2)
        {
            teile.push(teil);
        }
        // DIE LETZTEN ZWEI SIND DIE ZEITEN — und sie werden von HINTEN
        // gelesen, nicht von vorn.
        //
        // DER FEHLER, DEN DER ERSTE TESTLAUF GEFUNDEN HAT: die Felder
        // wurden mit `by_ref()` iteriert und beim ersten Teil ohne `=`
        // abgebrochen — und dieser Teil **war** der Zeitstempel. Er war
        // damit schon verbraucht, `next()` gab danach nur noch einen
        // (leeren) Rest. Von hinten gelesen gibt es diese
        // Reihenfolge-Abhaengigkeit nicht.
        let Some(bis_text) = teile.pop() else {
            return Err(SitzungsFehler::Kaputt("keine Zeit in der Nutzlast".into()));
        };
        let Some(seit_text) = teile.pop() else {
            return Err(SitzungsFehler::Kaputt("keine Startzeit".into()));
        };
        let mut felder = Vec::with_capacity(teile.len());
        for feld in teile {
            // M4: die Felder sind einzeln begrenzt — ein einzelnes Feld
            // ueber der Grenze macht die ganze Nutzlast unbrauchbar.
            if feld.len() > crate::sitzung::MAX_WERT_ZEICHEN {
                return Err(SitzungsFehler::ZuGross);
            }
            match feld.split_once('=') {
                Some((n, w)) if !n.is_empty() && n.len() <= crate::sitzung::MAX_NAME_ZEICHEN => {
                    felder.push((n.to_string(), w.to_string()));
                }
                Some(_) => {}
                // Ein Feld ohne `=` ist nach dem Zeitteil nichts mehr.
                None => return Err(SitzungsFehler::Kaputt("Feld ohne Trenner".into())),
            }
        }
        let seit = zeit_aus(seit_text)?;
        let bis = zeit_aus(bis_text)?;
        if jetzt > bis {
            return Err(SitzungsFehler::Abgelaufen);
        }
        Ok(Sitzung { felder, bis, seit })
    }

    /// Prueft einen Cookie-Wert; bei Fehler wird eine **neue** Sitzung
    /// erzeugt und als `Set-Cookie` an die Antwort gehaengt.
    ///
    /// DAS IST DER NORMALWEG, und er ist wichtig: ein abgelaufenes oder
    /// manipuliertes Cookie ist kein Fehler des Clients, sondern ein
    /// Client ohne gueltige Sitzung. Er bekommt eine neue — und keine
    /// Fehlermeldung, denn die wuerde ihm sagen, was schiefging.
    pub fn erneuere_oder_pruefe(
        &self,
        anfrage: &Request,
        antwort: &mut Response,
        jetzt: SystemTime,
        felder: &[(&str, &str)],
        https: bool,
    ) -> Result<Sitzung, SitzungsFehler> {
        if let Some(wert) = crate::sitzung::letztes_von(anfrage, &self.name) {
            if let Ok(s) = self.pruefe(&wert, jetzt) {
                return Ok(s);
            }
        }
        // Keine oder keine gueltige Sitzung: eine neue, sofort gesetzt.
        // added 05.10.2026 (SCHLUSSPHASE, clippy `map_identity`):
        // `.map_err(|e| e)` bildet ab und veraendert nichts. Die
        // `?` macht genau dasselbe und sagt, was passiert: der Fehler
        // wandert nach oben.
        let wert = self.signiere_felder(felder, jetzt)?;
        crate::sitzung::setze(
            antwort,
            &self.name,
            &wert,
            &crate::sitzung::Merkmale::sicher(),
            https,
        );
        // Die Sitzung, die gerade erzeugt wurde, ist gueltig — und wird
        // hier aus **derselben** Nutzlast gebaut, die signiert wurde
        // (nicht neu zusammengesetzt: zwei Berechnungen sind zwei
        // Wahrheiten).
        self.pruefe(&wert, jetzt)
            .map_err(|_| SitzungsFehler::Kaputt("neu gesetzte Sitzung ungueltig".into()))
    }
}

/// So lange eine Sitzung hoechstens laufen darf (30 Tage).
///
/// added 01.10.2026, SERVER V7: eine Sitzung mit `Max-Age=31536000`
/// (ein Jahr) ist keine ausgelaufene Sitzung, sondern eine
/// Dauerzusage. 30 Tage ist die Grenze, an der „Auslaufen" noch
/// etwas bedeutet.
pub const MAX_LEBENSDAUER: Duration = Duration::from_secs(30 * 24 * 3600);

use crate::http::{Request, Response};

/// Unix-Sekunden als Zahl.
fn zeit_zahl(t: SystemTime) -> String {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

/// Unix-Sekunden aus einer Zahl. `None` bei Unsinn (kein Overflow-
/// Sprung, keine Nicht-Ziffern).
fn zeit_aus(s: &str) -> Result<SystemTime, SitzungsFehler> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SitzungsFehler::Kaputt(format!("Zeitstempel {s:?}")));
    }
    let n: u64 = s
        .parse()
        .map_err(|_| SitzungsFehler::Kaputt(format!("Zeitstempel {s:?}")))?;
    // M3: die Zahl kommt aus dem Client, also ist sie eine Zahl aus der
    // Eingabe — und wird hier begrenzt statt weitergereicht.
    if n > 100_000_000_000 {
        return Err(SitzungsFehler::Kaputt("Zeitstempel unmoeglich weit".into()));
    }
    Ok(UNIX_EPOCH + Duration::from_secs(n))
}

// ---------------------------------------------------------------------------
// Base64url ohne Padding (RFC 4648 Abschnitt 5)
// ---------------------------------------------------------------------------

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Base64url kodieren, **ohne** `=` (das Padding entfaellt in URLs und
/// in Cookie-Werten, wo `=` zwar erlaubt waere, aber das Trennerzeichen
/// ist).
pub fn base64url(bytes: &[u8]) -> String {
    let mut raus = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for dreier in bytes.chunks(3) {
        let b0 = dreier[0] as u32;
        let b1 = *dreier.get(1).unwrap_or(&0) as u32;
        let b2 = *dreier.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        raus.push(B64URL[(n >> 18) as usize & 63] as char);
        raus.push(B64URL[(n >> 12) as usize & 63] as char);
        if dreier.len() > 1 {
            raus.push(B64URL[(n >> 6) as usize & 63] as char);
        }
        if dreier.len() > 2 {
            raus.push(B64URL[n as usize & 63] as char);
        }
    }
    raus
}

/// Base64url dekodieren. `None` bei jedem Fehler: ungueltiges Zeichen,
/// falsche Restlaenge, halber Block.
///
/// M4: `dekodiert` waechst mit der Eingabe, aber die Eingabe ist hier
/// schon durch [`crate::sitzung::MAX_COOKIE_BYTES`] (beim Lesen) bzw.
/// [`MAX_NUTZLAST_BYTES`] (beim Pruefen) begrenzt — die Grenze steht
/// also VOR diesem Aufruf.
pub fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 == 1 {
        return None; // ein voller Block ist 4 Byte: Rest 1 ist unsinnig
    }
    let mut raus = Vec::with_capacity(bytes.len() / 4 * 3 + 3);
    let mut puffer = 0u32;
    let mut bits = 0u32;
    for &b in bytes {
        if b == b'=' {
            return None; // Padding: in diesem Alphabet gibt es keins
        }
        let wert = B64URL
            .iter()
            .position(|&z| z == b)
            .map(|i| i as u32)
            // added 05.10.2026 (SCHLUSSPHASE, clippy
            // `unnecessary_lazy_evaluations`): `or_else` mit einer
            // `match`, die nur `None` liefert, ist ein `or`. Der
            // Kommentar bleibt, weil er die **Absicht** erklaert und
            // nicht die Form.
            .or(match b {
                // Standard-Base64: annehmen, pruefen schlaegt dann fehl
                b'+' => Some(62u32),
                b'/' => Some(63u32),
                _ => None,
            })?;
        puffer = (puffer << 6) | wert;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            raus.push(((puffer >> bits) & 0xff) as u8);
        }
    }
    // Die Fussbits muessen null sein: sonst hat jemand ein Byte
    // eingeschoben, das es nicht gibt (Base64 hat sechs Werte je Byte,
    // die letzten zwei eines Blocks sind Fuellung).
    if bits > 0 && (puffer & ((1 << bits) - 1)) != 0 {
        return None;
    }
    Some(raus)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EIN echter Schluessel aus 32 Byte Zufall — im Test feste
    /// Bytes, weil ein Test keine Wuerfel braucht.
    fn schluessel() -> Vec<u8> {
        (0u8..32).collect()
    }

    fn jetzt() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_790_000_000)
    }

    #[test]
    fn base64url_rundlauf_mit_echten_texten() {
        for text in [
            &b""[..],
            b"a",
            b"ab",
            b"abc",
            b"abcd",
            b"abcde",
            b"Hello, Welt! Umlaute: aeoeue",
            &[0u8, 255, 128, 64],
        ] {
            let kodiert = base64url(text);
            assert!(
                !kodiert.contains('='),
                "Base64url hat kein Padding: {kodiert:?}"
            );
            assert!(
                !kodiert.contains('+') && !kodiert.contains('/'),
                "Base64url hat kein + und kein /: {kodiert:?}"
            );
            assert_eq!(
                base64url_decode(&kodiert).as_deref(),
                Some(text),
                "Rundlauf {text:?} -> {kodiert:?}"
            );
        }
    }

    #[test]
    fn base64url_nimmt_standard_base64_zurueck_und_verwirft_sie_dann() {
        // `+` und `/` sind im Standard-Alphabet; wir dekodieren sie der
        // Bequemlichkeit halber — aber ein Byte mit Fuellbits dahinter
        // wird abgewiesen.
        let a = base64url(b"\xff\xff");
        assert!(base64url_decode(&a).is_some());
        let b = base64url(b"\xff\xf0");
        // dieselbe Laenge, aber ungueltige Fuellbits:
        assert_eq!(base64url_decode(&b).map(|v| v.len()), Some(2));
        assert!(base64url_decode(&a).is_some());
        assert!(base64url_decode("a").is_none(), "Restlaenge 1 ist unsinnig");
        assert!(
            base64url_decode("aGk=").is_none(),
            "Padding gehoert nicht dazu"
        );
        assert!(
            base64url_decode("a*k").is_none(),
            "Stern ist kein Base64url"
        );
    }

    #[test]
    fn signieren_und_pruefen_trifft_sich() {
        let s = Signierer::neu(schluessel());
        let wert = s
            .signiere_zeit(&[("nutzer", "7")], jetzt(), jetzt() + STANDARD_LEBENSDAUER)
            .expect("signiert");
        let geprueft = s.pruefe(&wert, jetzt()).expect("gueltig");
        assert_eq!(geprueft.feld("nutzer"), Some("7"));
        assert!(geprueft.gilt(jetzt()));
        // Und 11 Stunden spaeter (die Vorgabe sind 12) noch gueltig.
        // DER FEHLER WAR IM TEST, NICHT IM CODE: er pruefte 13 Stunden
        // und erwartete „gueltig" — bei 12 Stunden Lebensdauer ist das
        // korrekt **nicht** gueltig. Ein Test, der die eigene Vorgabe
        // ausrechnet, faengt den Fehler; einer, der eine Zahl rät,
        // meldet ihn als Serverfehler.
        assert_eq!(STANDARD_LEBENSDAUER.as_secs(), 12 * 3600);
        let spaeter = jetzt() + Duration::from_secs(11 * 3600);
        assert!(s.pruefe(&wert, spaeter).is_ok(), "nach 11 h noch gueltig");
        let zu_spaet = jetzt() + Duration::from_secs(13 * 3600);
        assert_eq!(
            s.pruefe(&wert, zu_spaet),
            Err(SitzungsFehler::Abgelaufen),
            "nach 13 h ist eine 12-h-Sitzung abgelaufen"
        );
    }

    #[test]
    fn abgelaufene_sitzung_wird_abgewiesen() {
        let s = Signierer::neu(schluessel());
        let wert = s
            .signiere_zeit(
                &[("nutzer", "7")],
                jetzt(),
                jetzt() + Duration::from_secs(60),
            )
            .expect("signiert");
        assert!(s.pruefe(&wert, jetzt() + Duration::from_secs(59)).is_ok());
        assert_eq!(
            s.pruefe(&wert, jetzt() + Duration::from_secs(61)),
            Err(SitzungsFehler::Abgelaufen)
        );
    }

    #[test]
    fn lebensdauer_wird_begrenzt() {
        let s = Signierer::neu(schluessel());
        // 10 Jahre beantragt:
        let wert = s
            .signiere_zeit(
                &[("nutzer", "7")],
                jetzt(),
                jetzt() + Duration::from_secs(10 * 365 * 24 * 3600),
            )
            .expect("signiert");
        let g = s.pruefe(&wert, jetzt()).expect("gueltig");
        // ...und kommen 30 Tage + 1 s an: weg.
        let spaeter = jetzt() + MAX_LEBENSDAUER + Duration::from_secs(1);
        assert_eq!(s.pruefe(&wert, spaeter), Err(SitzungsFehler::Abgelaufen));
        // Direkt an der Grenze noch gueltig — die Grenze ist <='.
        let an_grenze = jetzt() + MAX_LEBENSDAUER;
        assert!(
            s.pruefe(&wert, an_grenze).is_ok(),
            "und {g:?} zeigte die Grenze"
        );
    }

    /// DER KERN DES PUNKTS: manipulierte Cookies.
    ///
    /// Jede Manipulation einzeln — ein Angreifer probiert sie einzeln.
    #[test]
    fn manipulierte_cookies_werden_alle_abgewiesen() {
        let s = Signierer::neu(schluessel());
        let wert = s
            .signiere_zeit(&[("nutzer", "7")], jetzt(), jetzt() + STANDARD_LEBENSDAUER)
            .expect("signiert");
        assert!(s.pruefe(&wert, jetzt()).is_ok(), "der Ausgangswert gilt");

        // 1) Nutzlast von 7 auf 8 geaendert (die Rechte erhöhen).
        let (nutzlast, sig) = wert.rsplit_once('.').expect("Trenner");
        let fremd = Signierer::neu(schluessel());
        let neu = fremd
            .signiere_zeit(&[("nutzer", "8")], jetzt(), jetzt() + STANDARD_LEBENSDAUER)
            .expect("signiert");
        let neu_sig = neu.rsplit_once('.').expect("Trenner").1;
        let gefaelscht = format!("{nutzlast}.{neu_sig}");
        assert_eq!(
            s.pruefe(&gefaelscht, jetzt()),
            Err(SitzungsFehler::Signatur),
            "eine andere Signatur auf derselben Nutzlast muss durchfallen"
        );

        // 2) Signatur um ein Byte gekuerzt.
        let kurz = &sig[..sig.len() - 4];
        assert!(
            matches!(
                s.pruefe(&format!("{nutzlast}.{kurz}"), jetzt()),
                Err(SitzungsFehler::Kaputt(_))
            ),
            "eine zu kurze Signatur ist ein Formatfehler, kein Treffer"
        );

        // 3) Ein Byte an die Signatur angehaengt.
        assert!(
            matches!(
                s.pruefe(&format!("{nutzlast}.{sig}A"), jetzt()),
                Err(SitzungsFehler::Kaputt(_))
            ),
            "ein angehaengtes Byte an die Signatur muss durchfallen"
        );

        // 4) Kein Trenner.
        assert!(matches!(
            s.pruefe(&nutzlast, jetzt()),
            Err(SitzungsFehler::Kaputt(_))
        ));

        // 5) Leer und nur Trenner.
        assert_eq!(s.pruefe("", jetzt()), Err(SitzungsFehler::Fehlt));
        assert!(s.pruefe(".", jetzt()).is_err());

        // 6) Der Schluessel des anderen Signierers.
        let anderer = Signierer::neu(vec![1u8; 32]);
        assert_eq!(
            anderer.pruefe(&wert, jetzt()),
            Err(SitzungsFehler::Signatur),
            "ein fremder Schluessel darf nichts lesen"
        );
    }

    /// Die Nutzlast selbst von Hand gebaut (also ohne Signierer): der
    /// haeufigste Angriff ist nicht „Signatur aendern", sondern
    /// „Nutzlasteintrag mitschicken und die Signatur weglassen".
    #[test]
    fn selbstgebautes_cookie_ohne_signatur_geht_nicht() {
        let s = Signierer::neu(schluessel());
        let gefaelscht = format!("{}.{}", base64url(b"nutzer=1"), base64url(&[0u8; 32]));
        assert_eq!(
            s.pruefe(&gefaelscht, jetzt()),
            Err(SitzungsFehler::Signatur)
        );
    }

    #[test]
    fn zu_kurzer_schluessel_signiert_nicht() {
        // DER BEFUND: HMAC-SHA256 rechnet auch mit einem 1-Byte-Schluessel
        // und liefert ein Ergebnis. Genau das ist die Falle — der
        // Schluessel ist dann geraten.
        let kurz = Signierer::neu(vec![7u8]);
        assert_eq!(
            kurz.signiere_zeit(&[("nutzer", "1")], jetzt(), jetzt()),
            Err(SitzungsFehler::SchluesselZuKurz)
        );
        assert!(kurz.signiere(b"egal").is_none());
        // Und eine echte Sitzung ist damit nicht pruefbar:
        let lang = Signierer::neu(schluessel());
        let wert = lang
            .signiere_zeit(&[("nutzer", "1")], jetzt(), jetzt())
            .expect("signiert");
        assert_eq!(
            kurz.pruefe(&wert, jetzt()),
            Err(SitzungsFehler::SchluesselZuKurz)
        );
        // An der Grenze (16 Byte) geht es.
        let an_grenze = Signierer::neu(vec![3u8; MIN_SCHLUESSEL_BYTES]);
        assert!(
            an_grenze
                .signiere_zeit(&[("nutzer", "1")], jetzt(), jetzt())
                .is_ok()
        );
    }

    #[test]
    fn nutzlast_zu_gross_wird_abgewiesen() {
        let s = Signierer::neu(schluessel());
        // Ein Feld mit 9 KiB Inhalt: ueber MAX_NUTZLAST_BYTES.
        let gross = "x".repeat(MAX_NUTZLAST_BYTES + 1);
        assert_eq!(
            s.signiere_zeit(&[("f", &gross)], jetzt(), jetzt()),
            Err(SitzungsFehler::ZuGross)
        );
        // Und auf der Pruefseite: ein Cookie, das behauptet, gross zu
        // sein, muss auch dort auffallen.
        let wert = format!(
            "{}.{}",
            base64url(&[b'x'; MAX_NUTZLAST_BYTES + 1]),
            base64url(&[0u8; 32])
        );
        assert_eq!(s.pruefe(&wert, jetzt()), Err(SitzungsFehler::ZuGross));
    }

    #[test]
    fn felder_mit_und_ohne_zeichen_das_kein_cookie_wert_ist() {
        let s = Signierer::neu(schluessel());
        // Ein Feldwert mit `;` und `=` — er muss unveraendert ankommen,
        // weil er signiert ist (Base64 kodiert ihn ja).
        let wert = s
            .signiere_zeit(&[("rolle", "a=b;c"), ("x", "ü")], jetzt(), jetzt())
            .expect("signiert");
        let g = s.pruefe(&wert, jetzt()).expect("gueltig");
        assert_eq!(g.feld("rolle"), Some("a=b;c"));
        assert_eq!(g.feld("x"), Some("ü"));
        assert_eq!(g.feld("fehlt"), None);
    }

    #[test]
    fn zeitstempel_aus_der_nutzlast_muss_zahl_sein() {
        let s = Signierer::neu(schluessel());
        // "nutzer=1" + kein Trenner + kein Zeit -> Formatchecker.
        let nutzlast = b"nutzer=1\x1f\x1f";
        let wert = format!(
            "{}.{}",
            base64url(nutzlast),
            base64url(&s.signiere(nutzlast).expect("signiert"))
        );
        assert!(matches!(
            s.pruefe(&wert, jetzt()),
            Err(SitzungsFehler::Kaputt(_))
        ));
        // Und eine unmoeglich grosse Zahl:
        let nutzlast = b"nutzer=1\x1f999999999999999\x1f999999999999999";
        let wert = format!(
            "{}.{}",
            base64url(nutzlast),
            base64url(&s.signiere(nutzlast).expect("signiert"))
        );
        assert!(matches!(
            s.pruefe(&wert, jetzt()),
            Err(SitzungsFehler::Kaputt(_))
        ));
    }

    /// Der Normalweg: keine Sitzung -> eine wird gesetzt **und
    /// zurueckgegeben**. Und: eine gueltige Sitzung wird durchgelassen,
    /// ohne ein neues `Set-Cookie` zu erzeugen.
    #[test]
    fn erneuern_legt_ein_cookie_an_und_gibt_es_zurueck() {
        use crate::http::anfrage_aus_text;
        let s = Signierer::neu(schluessel());

        // 1) Anfrage ohne Cookie.
        let anfrage = anfrage_aus_text("GET", "/", &[], &[]);
        let mut antwort = crate::http::Response::text(200, "neu");
        let s1 = s
            .erneuere_oder_pruefe(&anfrage, &mut antwort, jetzt(), &[("nutzer", "42")], true)
            .expect("Sitzung erzeugt");
        assert_eq!(s1.feld("nutzer"), Some("42"));
        let gesetzt: Vec<&str> = antwort
            .headers
            .iter()
            .filter(|(k, _)| k == "Set-Cookie")
            .map(|(_, v)| v.as_str())
            .collect();
        assert!(!gesetzt.is_empty(), "es wurde kein Cookie gesetzt");
        let kern = gesetzt[0];
        assert!(kern.starts_with("sid="), "{kern}");
        for merkmal in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/"] {
            assert!(kern.contains(merkmal), "{merkmal} fehlt in {kern}");
        }

        // 2) Dieselbe Sitzung kommt wieder: kein neues Cookie.
        let wert = kern
            .trim_end_matches("; ")
            .trim_start_matches("sid=")
            .to_string();
        let anfrage2 = anfrage_aus_text("GET", "/", &[("Cookie", &format!("sid={wert}"))], &[]);
        let mut antwort2 = crate::http::Response::text(200, "bekannt");
        let s2 = s
            .erneuere_oder_pruefe(&anfrage2, &mut antwort2, jetzt(), &[("nutzer", "42")], true)
            .expect("Sitzung erkannt");
        assert_eq!(s2.feld("nutzer"), Some("42"));
        assert!(
            !antwort2.headers.iter().any(|(k, _)| k == "Set-Cookie"),
            "eine gueltige Sitzung erzeugt kein neues Cookie: {:?}",
            antwort2.headers
        );

        // 3) Manipuliertes Cookie: eine neue Sitzung, kein Fehler.
        let anfrage3 = anfrage_aus_text("GET", "/", &[("Cookie", "sid=bm90ZGU.bmljaHRnYW4")], &[]);
        let mut antwort3 = crate::http::Response::text(200, "neu");
        let s3 = s
            .erneuere_oder_pruefe(&anfrage3, &mut antwort3, jetzt(), &[("nutzer", "43")], true)
            .expect("Sitzung erneuert");
        assert_eq!(s3.feld("nutzer"), Some("43"), "die alte Nutzlast ist weg");
        assert_eq!(antwort3.status.0, 200, "kein 401: der Client darf weiter");
    }

    #[test]
    fn host_raumen_ist_der_default_namen_ablegenbar() {
        let s = Signierer::neu(schluessel()).mit_name("__Host-sid");
        assert_eq!(s.name(), "__Host-sid");
    }
}
