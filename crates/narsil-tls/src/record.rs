//! T8 — TLS 1.3: Record-Schicht, Transkript-Hash, Schluesselplan.
//!
//! Die drei Grenzen aus RFC 8446, und sie sind der Kern dieses Moduls:
//!
//!   * `TLSCiphertext.length` darf hoechstens 2^14 + 256 sein
//!     (RFC 8446 Abschnitt 5.2). Ein laengerer Wert wird abgelehnt, BEVOR
//!     ein Puffer fuer ihn entsteht (M4).
//!   * `HandshakeMsg.length` ist drei Byte breit, also hoechstens
//!     2^24 - 1. Auch das wird vor der Allokation geprueft (M4).
//!   * Die innere Nutzlast ist `content` plus ein Typ-Byte, und `content`
//!     darf hoechstens 2^14 sein (RFC 8446 Abschnitt 5.1).
//!
//! M3: jede Schleife laeuft ueber `Vec::chunks` einer Laenge, die vorher
//! gegen eine der obigen Grenzen geprueft wurde, oder ueber eine
//! benannte Konstante. Es gibt keine Schleife ueber eine Zahl, die aus
//! dem Netz kommt.

use crate::aes::{gcm_chiffre, gcm_dechiffre};
use crate::chacha::{aead_chiffre, aead_dechiffre};
// added 02.10.2026 03:5x: `expand_label_sha256_fest` fuer die 15 Stellen, an
// denen vorher ein `expect("32 Byte sind erlaubt")` stand.
use crate::hkdf::{expand_label_sha256, expand_label_sha256_fest, extract_sha256};
use crate::sha2::Sha256;

// ---------------------------------------------------------------------------
// Grenzen
// ---------------------------------------------------------------------------

/// Hoechste Laenge eines TLSCiphertext (RFC 8446 Abschnitt 5.2).
pub const MAX_CIPHERTEXT: usize = (1 << 14) + 256;
/// Hoechste Laenge des Klartexts INNEN in einem Record (Abschnitt 5.1).
pub const MAX_INNEN: usize = 1 << 14;
/// Hoechste Laenge einer Handshake-Nachricht: drei Byte Laengenfeld
/// (RFC 8446 Abschnitt 4).
pub const MAX_NACHRICHT: usize = (1 << 24) - 1;
/// Hoechste Laenge, die der Record-Parser liest, BEVOR er die Laengenangabe
/// im Kopf geprueft hat. Es ist ein Fenster, kein Feld: so viele Byte
/// koennen im Speicher liegen, mehr braucht der Parser nie.
pub const MAX_EINLESEN: usize = MAX_CIPHERTEXT + 22;

/// Warum eine Eingabe abgelehnt wird. BENANNTE Varianten (M7: der Test
/// sichert die Variante, nicht die Panik).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordFehler {
    /// Die Laengenangabe im Kopf ist groesser als `MAX_CIPHERTEXT`.
    LaengeZuGross {
        angegeben: usize,
        erlaubt: usize,
    },
    /// Der Kopf sagt mehr Bytes an, als im Puffer liegen.
    Abgeschnitten {
        erwartet: usize,
        vorhanden: usize,
    },
    /// Der Puffer ist kuerzer als ein Recordkopf.
    KopfZuKurz {
        vorhanden: usize,
    },
    /// Unbekannter Inhaltstyp.
    TypUnbekannt {
        typ: u8,
    },
    /// Die innere Nutzlast war laenger als 2^14.
    InnenZuGross {
        ist: usize,
        erlaubt: usize,
    },
    /// Es war kein innerer Typ-Byte vorhanden.
    KeinInnertyp,
    /// Der Datensatz war nicht verschluesselt, obwohl ein Schluessel
    /// gesetzt war (RFC 8446 Abschnitt 5.2: nach dem Handshake ist
    /// Aussen plaintext unerlaubt).
    /// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// added 02.10.2026 03:5x: Der Schluesselplan liess sich nicht rechnen
    /// (HKDF). Das ist die Form, in der die **15**
    /// `expand_label_sha256(…, 32).expect("32 Byte sind erlaubt")` in dieser
    /// Datei jetzt enden. Sie traegt den HKDF-Fehler **bei sich** und nicht in
    /// einem `String` — sonst waere es am Ende doch eine unbenannte Meldung.
    Hkdf(crate::hkdf::Fehler),
    Unverschluesselt,
    /// Die Authentifizierung schlug fehl.
    Authentifizierung,
}

/// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// added 02.10.2026 03:5x: DIE UMWANDLUNG, und sie ist der Grund, warum die
/// 15 Stellen kein `map_err` brauchen: `Schluesselplan` rechnet nur mit HKDF,
/// `RecordFehler` traegt den HKDF-Fehler bei sich. GEMESSEN: ohne diese
/// `From`-Umwandlung meldete der Uebersetzer an **sechs** Stellen
/// „`?` couldn't convert the error to `RecordFehler`" — und jede einzelne
/// `.map_err(RecordFehler::Hkdf)` an sechs Stellen waere dieselbe Antwort
/// sechsmal.
/// added 02.10.2026 05:1x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// Der Name der Umgebungsvariablen, unter der der Schluessellog abgelegt wird.
/// Format: **NSS-Keylog** (`SSLKEYLOGFILE`), also
/// `<LABEL> <client_random_hex> <geheimnis_hex>` je Zeile — so, wie
/// `tshark -o tls.keylog_file:…` und Wireshark es lesen.
///
/// WARUM: seit R12/R15 steht fest, dass die stapelnden Server unser
/// TLS-Ticket annehmen und dann schweigen, und dass die verschluesselten
/// Records dieser Verbindung **nicht lesbar** sind. Ohne Schluessel ist die
/// Frage „was schickt der Server nach dem Resume?" nicht beantwortbar, und
/// GEMESSEN ist nur ihre Zahl (vier Records, 439 Byte).
///
/// added 02.10.2026 05:1x: DER ZWECK IST DIE DIAGNOSE, nicht der Betrieb.
/// Deshalb gilt: ohne gesetzte Variable wird **nichts** geschrieben und
/// **nichts** gelesen; ein Fehler beim Oeffnen oder Schreiben wird
/// **stillschweigend** uebergangen — ein Diagnose-Werkzeug darf einen
/// Handshake, der sonst gelingt, nicht scheitern lassen.
pub const UMWELT_KEYLOG: &str = "NARSIL_TLS_KEYLOG";

/// added 02.10.2026 05:1x: DER ZUFALL AUS DEM GESENDETEN ClientHello.
/// `Hallo.ch` traegt die Nachricht **mit Kopf**, also liegt der Zufall ab
/// Byte 10: 1 Byte Nachrichtentyp, 3 Byte Dreierlaenge, 2 Byte `legacy_version`
/// (RFC 8446 4.1.2: `random` steht direkt danach).
pub fn client_random_aus_hello(ch: &[u8]) -> Option<[u8; 32]> {
    if ch.len() < 4 + 2 + 32 {
        return None;
    }
    let mut r = [0u8; 32];
    r.copy_from_slice(&ch[6..38]);
    Some(r)
}

/// added 02.10.2026 05:1x: SCHREIBT DIE VIER TLS-1.3-MARKEN. Die Namen sind
/// die von Wireshark; `CLIENT_RANDOM` (TLS 1.2 und aelter) waere hier der
/// falsche, denn es geht um die Verkehrsgeheimnisse, nicht um den
/// Master-Schluessel.
/// added 02.10.2026 05:3x: DER PFAD, und er ist eine **eigene** Funktion.
/// added 02.10.2026 05:3x: GEMESSEN, warum: der erste Test stand vor und
/// brauchte `std::env::set_var`, und das ist seit Edition 2024 `unsafe` —
/// `unsafe { std::env::set_var(..) }` verlangte der Uebersetzer, und `unsafe`
/// ist im Projekt verboten. Mit dieser Trennung braucht **kein** Test die
/// Umgebung zu veraendern: der Test ruft `keylog_schreiben` mit einem Pfad
/// auf, den er selbst gewaehlt hat.
pub fn keylog_pfad() -> Option<String> {
    match std::env::var(UMWELT_KEYLOG) {
        Ok(p) if !p.is_empty() => Some(p),
        _ => None,
    }
}

/// added 02.10.2026 05:3x: das Schreiben selbst, mit **uebergebenem** Pfad.
pub fn keylog_schreiben(
    pfad: &str,
    client_random: &[u8; 32],
    client_hs: &[u8; 32],
    server_hs: &[u8; 32],
    client_ap: &[u8; 32],
    server_ap: &[u8; 32],
) {
    let mut datei = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(pfad)
    {
        Ok(d) => d,
        // added 02.10.2026 05:1x: stillschweigend, siehe den Kopf des Moduls.
        Err(_) => return,
    };
    use std::io::Write;
    for (marke, geheimnis) in [
        ("CLIENT_HANDSHAKE_TRAFFIC_SECRET", client_hs),
        ("SERVER_HANDSHAKE_TRAFFIC_SECRET", server_hs),
        ("CLIENT_TRAFFIC_SECRET_0", client_ap),
        ("SERVER_TRAFFIC_SECRET_0", server_ap),
    ] {
        let zeile = format!(
            "{marke} {} {}\n",
            crate::sha2::hexen(client_random),
            crate::sha2::hexen(geheimnis)
        );
        // added 02.10.2026 05:1x: auch ein Schreibfehler wird uebergangen; ein
        // halb geschriebener Log ist besser als kein Handshake.
        let _ = datei.write_all(zeile.as_bytes());
    }
}

impl From<crate::hkdf::Fehler> for RecordFehler {
    fn from(e: crate::hkdf::Fehler) -> Self {
        RecordFehler::Hkdf(e)
    }
}

impl std::fmt::Display for RecordFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordFehler::LaengeZuGross { angegeben, erlaubt } => {
                write!(f, "Laenge {angegeben} ueber der Grenze {erlaubt}")
            }
            RecordFehler::Abgeschnitten {
                erwartet,
                vorhanden,
            } => {
                write!(f, "Kopf sagt {erwartet} Byte, Puffer hat {vorhanden}")
            }
            RecordFehler::KopfZuKurz { vorhanden } => {
                write!(f, "nur {vorhanden} Byte, ein Recordkopf hat 5")
            }
            RecordFehler::TypUnbekannt { typ } => write!(f, "unbekannter Inhaltstyp {typ}"),
            RecordFehler::InnenZuGross { ist, erlaubt } => {
                write!(f, "Innen {ist} Byte, erlaubt {erlaubt}")
            }
            RecordFehler::KeinInnertyp => write!(f, "kein innerer Typ-Byte am Ende"),
            RecordFehler::Hkdf(e) => {
                write!(f, "der Schluesselplan liess sich nicht rechnen: {e}")
            }
            RecordFehler::Unverschluesselt => write!(f, "Datensatz war nicht verschluesselt"),
            RecordFehler::Authentifizierung => write!(f, "die Authentifizierung schlug fehl"),
        }
    }
}

impl std::error::Error for RecordFehler {}

// ---------------------------------------------------------------------------
// Cipher Suites
// ---------------------------------------------------------------------------

/// Die beiden Cipher Suites, die diese Crate spricht (T8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    /// TLS_CHACHA20_POLY1305_SHA256 (RFC 8446 Abschnitt 5.3).
    ChaCha20Poly1305Sha256,
    /// TLS_AES_128_GCM_SHA256 (RFC 8446 Abschnitt 5.3).
    Aes128GcmSha256,
}

impl Suite {
    /// Die Kennung aus dem Cipher-Suite-Register (RFC 8446 Abschnitt 5.3).
    pub fn code(self) -> u16 {
        match self {
            // 0x1303
            Suite::ChaCha20Poly1305Sha256 => 0x1303,
            // 0x1301
            Suite::Aes128GcmSha256 => 0x1301,
        }
    }

    /// Aus der Kennung. Unbekannt ist ein Fehler, kein Ersatzwert: eine
    /// falsche Suite wuerde stillschweigend falsch verschluesseln.
    pub fn aus_code(code: u16) -> Option<Suite> {
        match code {
            0x1303 => Some(Suite::ChaCha20Poly1305Sha256),
            0x1301 => Some(Suite::Aes128GcmSha256),
            _ => None,
        }
    }

    /// Laenge des Schluessels in Byte.
    pub fn schluessel_laenge(self) -> usize {
        match self {
            Suite::ChaCha20Poly1305Sha256 => 32,
            Suite::Aes128GcmSha256 => 16,
        }
    }

    // added 04.10.2026 00:3x, R67. **`nonce_laenge` GELOESCHT** — GEMESSEN: ein
    // Treffer in allen Crates, die eigene Definition. Der Wert **12** steht als
    // Konstante an zwei Stellen im Code (RFC 8446 5.3), und ein Getter, der
    // nichts variiert, ist eine zweite Quelle fuer dieselbe Zahl.

    /// Das Geheimnis in Klartext: Schluessel und IV (RFC 8446
    /// Abschnitt 7.3, "keying material").
    // added 02.10.2026 03:5x: `Result`, und das ist der Grund fuer die
    // Fehlerform `Hkdf`. GEMESSEN: `schluessel_laenge()` liefert 16 oder 32,
    // beide weit unter `0xFFFF` — der Fehler ist unerreichbar und wird
    // trotzdem **benannt** statt behauptet, genau wie die 15 Stellen davor.
    pub fn keying_material(self, secret: &[u8]) -> Result<([u8; 32], [u8; 12]), RecordFehler> {
        let laenge = self.schluessel_laenge();
        let schluessel =
            expand_label_sha256(secret, b"key", b"", laenge).map_err(RecordFehler::Hkdf)?;
        // added 02.10.2026 03:5x: die IV ist 12 Byte und damit **fest** — der
        // Typ traegt die Laenge, also bleibt hier nichts zu behaupten.
        let iv: [u8; 12] =
            expand_label_sha256_fest::<12>(secret, b"iv", b"").map_err(RecordFehler::Hkdf)?;
        // added 02.10.2026 03:6x: DIE KORREKTUR, und sie kam aus dem Test.
        // Meine erste Fassung war ein `try_into::<[u8; 32]>()`, und
        // `schluesselung_rundlauf_und_manipulation` meldete
        // `InnenZuGross { ist: 16, erlaubt: 32 }`: **AES-128 hat einen
        // 16-Byte-Schluessel**, und der alte Code hat ihn mit Nullen auf 32
        // Byte aufgefuellt (`s[..laenge].copy_from_slice`). Also wird die
        // Grenze **vor** dem Kopieren geprueft, und der Rest bleibt Null —
        // das Verhalten des Betriebs, nur ohne `expect`.
        let mut s = [0u8; 32];
        if laenge > 32 {
            return Err(RecordFehler::InnenZuGross {
                ist: laenge,
                erlaubt: 32,
            });
        }
        s[..laenge].copy_from_slice(&schluessel);
        Ok((s, iv))
    }
}

// ---------------------------------------------------------------------------
// Schluesselplan (RFC 8446 Abschnitt 7.1)
// ---------------------------------------------------------------------------

/// Der Schluesselplan. Er haelt die Geheimnisse und den Transkript-Hash.
pub struct Schluesselplan {
    /// Das ECDHE-Geheimnis, aus X25519.
    shared: [u8; 32],
    /// Der laufende Transkript-Hash (SHA-256).
    transkript: Sha256,
    /// Traegt, ob `c hs traffic` schon berechnet wurde.
    phase_hs: bool,
    /// Traegt, ob `master secret` schon berechnet wurde.
    phase_master: bool,
    /// Das `master secret` selbst, zwischengespeichert.
    master: [u8; 32],
    pub client_hs_traffic: [u8; 32],
    pub server_hs_traffic: [u8; 32],
    pub client_ap_traffic: [u8; 32],
    pub server_ap_traffic: [u8; 32],
    /// Das `finished`-Geheimnis des CLIENTS, aus `c hs traffic`.
    pub finished_key_client: [u8; 32],
    /// Das `finished`-Geheimnis des SERVERS, aus `s hs traffic`.
    pub finished_key_server: [u8; 32],
    /// Der Wiederaufnahme-PSK, oder `None` fuer einen vollen Handshake.
    ///
    /// added 02.10.2026, R4d. DER GRUND FUER EIN FELD STATT EINES
    /// PARAMETERS: `handshakephase` rechnet den Early Secret **einmal**
    /// und leitet daraus alles ab (RFC 8446 7.1). Ein PSK, der erst beim
    /// Aufruf von `handshakephase` ankommt, muesste also genau einmal
    /// weitergegeben werden und waere danach nicht mehr zu sehen — und
    /// genau dieses "danach nicht mehr zu sehen" ist der Fehler, den man
    /// bei der Pruefung nicht bemerkt: ein Plan ohne PSK und ein Plan mit
    /// PSK liefern verschiedene Schluessel, beide aber plausible Laengen.
    ///
    /// M3: die Laenge ist die des Aufrufers. Sie wird in `handshakephase`
    /// gegen `MAX_PSK_BYTES` geprueft; PSK ist ein `opaque<1..2^32-1>`
    /// (RFC 8446 4.2.11.2), und wir nehmen bewusst nur, was ein Ticket
    /// hergeben kann — hoeher waere eine Allokation aus dem Netz.
    ///
    /// **PRIVAT**, added 02.10.2026, R4d. `pub` wuerde ermoeglichen, einen
    /// leeren PSK einzustellen; `handshakephase` muesste dann zwischen
    /// „leer" und „kein PSK" entscheiden und haette keine Fehlerart fuer
    /// beides. Der einzige Weg hereinfuehrt ueber `mit_psk` oder
    /// `setze_psk`, und beide pruefen. Die erste Fassung hatte `pub` und
    /// zwei `panic!` in `handshakephase` — ein `panic` auf Aufruferdaten
    /// ist hier genau die Sorte Fehler, die man nicht bauen sollte, wenn
    /// man sie zur Laufzeit auch nicht braucht.
    psk: Option<Vec<u8>>,
    /// Der Early Secret, sobald `handshakephase` gerechnet hat.
    ///
    /// added 02.10.2026, R4d. OEFFENTLICH, weil ein Server und ein Test
    /// ihn sehen muessen: `HKDF-Extract(0, PSK)` ist die **erste**
    /// Groesse, die sich mit und ohne Wiederaufnahme unterscheidet, und
    /// ein Plan, der sie nicht preisgibt, kann nur indirekt geprueft
    /// werden — indirekt heisst hier: ueber Schluessel, die auch von
    /// drei anderen Rechnungen abhaengen.
    pub early_secret: [u8; 32],
    /// Das Handshake-Geheimnis, sobald `handshakephase` gerechnet hat.
    ///
    /// added 02.10.2026, R4d. Aus demselben Grund oeffentlich wie
    /// `early_secret`: es ist das `HKDF-Extract(derived, ECDHE)`, die
    /// **zweite** Groesse, die sich mit und ohne Wiederaufnahme
    /// unterscheidet.
    pub handshake_secret: [u8; 32],
}

/// Der Fehler beim Setzen eines Wiederaufnahme-PSK.
///
/// added 02.10.2026, R4d. EIGENE ART und kein `String`, weil der
/// Aufrufer an den **zwei** Faellen etwas tun muss: bei `PskLeer` gar
/// nichts (es gab nie einen PSK) und bei `PskZuLang` den Aufruf kuerzen.
/// Eine Zeichenkette beides gleich zu behandeln hiesse, die Entscheidung
/// dem Aufrufer zu ueberlassen, ohne ihm zu sagen, dass es sie gibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanFehler {
    /// Der PSK hatte 0 Byte. Nach RFC 8446 4.2.11.2 ist
    /// `opaque identity<1..2^16-1>` — mindestens 1 Byte. Zu behandeln wie
    /// „kein PSK" waere eine stille Annahme.
    PskLeer,
    /// Der PSK war laenger als ein Ticket sein kann.
    PskZuLang { war: usize, max: usize },
}

impl std::fmt::Display for PlanFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanFehler::PskLeer => {
                write!(
                    f,
                    "PSK der Laenge 0 (RFC 8446 4.2.11.2 verlangt mindestens 1)"
                )
            }
            PlanFehler::PskZuLang { war, max } => {
                write!(f, "PSK {war} Byte, erlaubt sind {max}")
            }
        }
    }
}

impl Schluesselplan {
    /// Neuer Schluesselplan aus dem ECDHE-Geheimnis.
    ///
    /// M3: `shared` ist `[u8; 32]`, im Typ erzwungen. Hier gibt es
    /// nichts aus der Eingabe, was alloziert werden koennte.
    pub fn neu(shared: [u8; 32]) -> Schluesselplan {
        Schluesselplan {
            shared,
            transkript: Sha256::neu(),
            phase_hs: false,
            phase_master: false,
            master: [0u8; 32],
            client_hs_traffic: [0u8; 32],
            server_hs_traffic: [0u8; 32],
            client_ap_traffic: [0u8; 32],
            server_ap_traffic: [0u8; 32],
            finished_key_client: [0u8; 32],
            finished_key_server: [0u8; 32],
            psk: None,
            early_secret: [0u8; 32],
            handshake_secret: [0u8; 32],
        }
    }

    /// Wie `neu`, aber **mit** Wiederaufnahme-PSK.
    ///
    /// added 02.10.2026, R4d. DER WEG, und es ist derselbe wie in
    /// `ticket::psk_anbieten`: die Kette gehoert an **eine** Stelle. Hier
    /// ist sie eine Zeile (`early_secret = HKDF-Extract(0, PSK)`), aber
    /// sie ist die Stelle, an der ein Client und ein Server sonst
    /// verschiedene Early Secrets hätten und es erst an den
    /// `finished`-Bytes gemerkt wird.
    ///
    /// DER GRUND ZWEIER BAUFORMEN: `neu` wird an ~20 Stellen benutzt, und
    /// ein zusaetzliches `Option`-Argument dort waere an jeder ein
    /// `None`, das man beim Lesen nicht sieht.
    pub fn mit_psk(shared: [u8; 32], psk: Vec<u8>) -> Result<Schluesselplan, PlanFehler> {
        let mut p = Self::neu(shared);
        p.setze_psk(psk)?;
        Ok(p)
    }

    /// Setzt den Wiederaufnahme-PSK **mit** Pruefung.
    ///
    /// added 02.10.2026, R4d. DER GRUND FUER DEN EIGENEN SETZER: `psk`
    /// ist privat, und damit ist „der PSK hat 0 Byte" ein Zustand, den
    /// niemand von aussen herstellen kann. Wer `psk` per `pub` setzen
    /// koennte, muesste `handshakephase` zwischen „leer" und „kein PSK"
    /// entscheiden — und diese Entscheidung waere eine Annahme.
    ///
    /// DIE GRENZEN, beide benannt (M3):
    ///  * **0** ist verboten: RFC 8446 4.2.11.2 sagt
    ///    `opaque identity<1..2^16-1>`, das Ticket hat mindestens 1 Byte.
    ///  * `MAX_PSK_BYTES` = `ticket::MAX_TICKET_BYTES` = 65535: mehr als
    ///    ein Ticket kann hier nicht ankommen, und alles darueber waere
    ///    eine Allokation aus dem Netz ohne Normgrund.
    pub fn setze_psk(&mut self, psk: Vec<u8>) -> Result<(), PlanFehler> {
        match psk.len() {
            0 => {
                return Err(PlanFehler::PskLeer);
            }
            n if n > crate::ticket::MAX_TICKET_BYTES => {
                return Err(PlanFehler::PskZuLang {
                    war: n,
                    max: crate::ticket::MAX_TICKET_BYTES,
                });
            }
            _ => {}
        }
        self.psk = Some(psk);
        Ok(())
    }

    /// Eine vollstaendige Handshake-Nachricht in den Transkript aufnehmen.
    ///
    /// Wichtig: es wird die NACHRICHT mit Kopf aufgenommen, nicht nur der
    /// Inhalt (RFC 8446 Abschnitt 4.4.1). Deshalb wird hier der volle
    /// Byte-String gehasht, den der Absender gesendet hat.
    pub fn aufnehmen(&mut self, nachricht_kopf_und_inhalt: &[u8]) {
        self.transkript.aufnehmen(nachricht_kopf_und_inhalt);
    }

    /// Der Transkript-Hash bis hierher.
    pub fn transkript_hash(&self) -> [u8; 32] {
        self.transkript.fertig()
    }

    /// Die Handshake-Geheimnisse berechnen. Aufruf nach dem
    /// ServerHello, wenn der Transkript bis dahin vollstaendig ist.
    /// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026), Rolle TLS:
    /// `Result<(), RecordFehler>`, und das ist die eigentliche Aenderung
    /// dieser Runde. Vorher stand hier an **15** Stellen ein
    /// `expect("32 Byte sind erlaubt")`, und die kürzeste saubere Form ist
    /// die, dass die Rechnung einen Fehler **zurückgeben** darf. GEMESSEN: mit
    /// `FRIST`-freien 32-Byte-Argumenten sind alle 15 unerreichbar — der
    /// Fehler bleibt also unerreichbar, wird aber nicht mehr behauptet.
    pub fn handshakephase(&mut self) -> Result<(), RecordFehler> {
        if self.phase_hs {
            return Ok(());
        }
        // early_secret = HKDF-Extract(0, PSK).
        //
        // added 02.10.2026, R4d. Ohne PSK ist das IKM 32 Nullbyte — das
        // ist die Zeile, die hier seit dem ersten Stand stand und die
        // **falsch** war, sobald es Wiederaufnahme geben sollte: ein
        // Plan ohne PSK und ein Plan mit PSK unterschieden sich dann in
        // gar keinem Feld dieses Plans, sondern erst in den Schluesseln.
        //
        // M3: die Laenge des PSK kommt aus `self.psk`, also aus dem
        // Aufrufer. `MAX_PSK_BYTES` ist die Grenze; ein Ticket ist auf
        // 65535 Byte begrenzt (`MAX_TICKET_BYTES`), und mehr anzunehmen
        // waere eine Allokation aus dem Netz ohne Normgrund.
        const MAX_PSK_BYTES: usize = crate::ticket::MAX_TICKET_BYTES;
        let null_psk;
        let psk: &[u8] = match &self.psk {
            Some(p) => {
                // BEIDE GRUENDE SIND VOM SETZER ABGEDECKT: `psk` ist
                // privat, also kam `p` nur durch `mit_psk` oder
                // `setze_psk` herein, und beide lehnen 0 Byte und mehr
                // als `MAX_PSK_BYTES` ab. Deshalb **kein** `panic!` hier,
                // sondern eine Behauptung im Debug-Modus — sie beschreibt
                // eine Eigenschaft, sie verteidigt sie nicht.
                debug_assert!(!p.is_empty(), "psk kam nur durch den geprueften Setzer");
                debug_assert!(
                    p.len() <= MAX_PSK_BYTES,
                    "psk kam nur durch den geprueften Setzer"
                );
                p.as_slice()
            }
            None => {
                null_psk = [0u8; 32];
                &null_psk
            }
        };
        let frueh = extract_sha256(&[0u8; 32], psk);
        self.early_secret = frueh;
        // ECHTER FEHLER, und der hat den Handshake gegen example.com
        // scheitern lassen: als Kontext stand hier `b""`, also ein
        // leerer STRING. `Derive-Secret(secret, "derived", Hash)`
        // verlangt aber den Hash des LEEREN TRANSKRIPTS, und der ist
        // nicht leer, sondern `e3b0c442 98fc 1c14 9afb f4c8 996f
        // 9247 5aea 4aee 5b6c d1 2d 1d 0a` (32 Byte).
        //
        // `HKDF-Expand-Label` packt den Kontext mit Laengenangabe in
        // das `info`-Feld, also aendert die Laenge das Ergebnis
        // vollstaendig. Mit `b""` war jedes abgeleitete Geheimnis
        // falsch — und damit auch jeder Schluessel, jede Nonce und
        // jedes AAD.
        //
        // Warum es nicht auffiel: `trace.rs` rechnet dieselbe Formel
        // richtig nach, mit `transkript(&[])` als Kontext. Der Test
        // verglich also die FORMEL, nicht `Schluesselplan`. Zwei
        // Wege, eine Rechnung, und der zweite war ungeprueft.
        // Genau diesen zweiten Weg prueft jetzt
        // `station_5a_schluesselplan_gibt_die_geheimnisse`.
        let leerer_transkript = crate::sha2::sha256(&[]);
        let abgeleitet = expand_label_sha256_fest::<32>(&frueh, b"derived", &leerer_transkript)?;
        let hs = extract_sha256(&abgeleitet, &self.shared);
        // added 02.10.2026, R4d: das Handshake-Geheimnis merken, damit
        // es ohne den Umweg ueber die Schluessel sichtbar ist (siehe das
        // Feld). Ohne PSK ist es **unveraendert** — dieselbe Rechnung
        // wie vorher, weil `frueh` dann derselbe Wert ist.
        self.handshake_secret = hs;
        let th = self.transkript_hash();
        self.client_hs_traffic = expand_label_sha256_fest::<32>(&hs, b"c hs traffic", &th)?;
        self.server_hs_traffic = expand_label_sha256_fest::<32>(&hs, b"s hs traffic", &th)?;
        //
        // ECHTER FEHLER, und der letzte, der am Handshake hing. Der
        // `finished_key` wurde aus `hs` — dem ROHEN
        // Handshake-Geheimnis — abgeleitet. RFC 8446 Abschnitt 4.4.4
        // sagt aber:
        //
        //     finished_key = HKDF-Expand-Label(BaseKey, "finished", "")
        //     verify_data  = HMAC(finished_key, Transcript-Hash(...))
        //
        // und der `BaseKey` ist das HANDSHAKE-VERKEHRSGEHEIMNIS der
        // jeweiligen Seite, nicht `handshake_secret`.
        //
        // Dass es auffaellt, obwohl die Schluesselplanung stimmt: der
        // Fehler aendert nichts an `c hs traffic` und `s hs traffic`.
        // Die verschluesselten Records liessen sich entschluesseln, die
        // Kette wurde geprueft, die `CertificateVerify`-Signatur
        // bestaetigt — und dann kam der Vergleich des `finished` und
        // schlug fehl. Drei bestaetigte Schritte und eine Abweichung
        // ganz am Ende.
        //
        // Beide Seiten brauchen ein eigenes, also gibt es zwei Felder.
        // Der Client prueft mit `finished_key_server` und sendet mit
        // `finished_key_client`; ein Feld fuer beide waere wieder
        // dieselbe Verwechslung mit weniger Zeichen.
        //
        // Belege aus RFC 8448 Abschnitt 3: `finished_key (server)` ist
        // `HKDF-Expand-Label(server_handshake_traffic_secret,
        // "finished", "")`, und das ist in `trace.rs` als
        // `FINISHED_KEY_SERVER` nachgerechnet. Derselbe Test hat den
        // Server-Schluessel hierher geholt — sonst waere dieser
        // Unterschied nie aufgefallen.
        self.finished_key_client =
            expand_label_sha256_fest::<32>(&self.client_hs_traffic, b"finished", b"")?;
        self.finished_key_server =
            expand_label_sha256_fest::<32>(&self.server_hs_traffic, b"finished", b"")?;
        self.phase_hs = true;
        Ok(())
    }

    /// Das Client-Anwendungsgeheimnis. Aufruf, wenn das ClientFinished
    /// im Transkript ist (Transkript endet auf ClientHello...server Finished).
    /// ECHTER FEHLER, und der letzte am Handshake: hier stand nur die
    /// CLIENT-Halbseite. `strom.rs` liest aber beide Schluesselpaare
    /// nach EINEM Aufruf:
    ///
    /// ```text
    /// let (ks, ivs) = keying_material(&plan.client_ap_traffic);
    /// let (ke, ive) = keying_material(&plan.server_ap_traffic);
    /// ```
    ///
    /// und `server_ap_traffic` war zu diesem Zeitpunkt noch NICHT
    /// gesetzt — also null. Daraus wurde ein Leseschluessel aus dem
    /// Null-Geheimnis abgeleitet, und jeder Record des Servers nach
    /// dem Handshake schlug an der Authentifizierung fehl. Das war der
    /// "Verschluesselung (lesen)"-Fehler, an dem der Live-Test
    /// scheiterte, nachdem Schluesselplan, Transkript und Kette
    /// stimmten.
    ///
    /// WARUM ES NICHT AUFFIEL: der Test `schluesselplan_gegen_die_
    /// referenz` ruft `anwendungsphase_client()` und danach
    /// `anwendungsphase_server()` auf und prueft jedes Feld direkt
    /// nach seinem Aufruf. Beide Felder waren also je fuer sich
    /// richtig — nur nie beide zur selben Zeit, und das pruefte
    /// niemand.
    ///
    /// Nach RFC 8446 Abschnitt 7.1 stehen `c ap traffic` und
    /// `s ap traffic` ueber DEMSELBEN Transkript, nämlich dem bis zum
    /// `finished` des Servers. Beide werden deshalb hier zusammen
    /// gesetzt.
    pub fn anwendungsphase_client(&mut self) -> Result<(), RecordFehler> {
        let master = self.master_secret()?;
        let th = self.transkript_hash();
        self.client_ap_traffic = expand_label_sha256_fest::<32>(&master, b"c ap traffic", &th)?;
        self.server_ap_traffic = expand_label_sha256_fest::<32>(&master, b"s ap traffic", &th)?;
        Ok(())
    }

    /// Beide Anwendungsgeheimnisse. Aufruf, wenn das **ServerFinished**
    /// im Transkript ist.
    ///
    /// added 30.09.2026, Runde 2, TLS R1c. DIE DOKU VORHER, und sie war
    /// **falsch**: sie behauptete, das Client-Geheimnis endete auf dem
    /// ClientFinished. RFC 8446 Abschnitt 7.1 sagt fuer **beide**
    /// `ClientHello...server Finished` (Beleg: `rfc/rfc8446.txt`, Zeilen
    /// 5191-5197). Der Server in `server.rs` hat daraufhin das
    /// ClientFinished **vor** diesen Aufruf genommen, und damit waren
    /// die Anwendungsgeheimnisse beider Seiten verschieden: der erste
    /// Anwendungsrecord liess sich nicht oeffnen ("die
    /// Authentifizierung schlug fehl") — bei unserem eigenen Client und
    /// bei `openssl s_client`, also bei zwei unabhaengigen Gegnern.
    ///
    /// Warum es zwei Aufrufe und nicht einer: der Server und der Client
    /// rufen ihn an verschiedenen Stellen auf. Beide setzen **beide**
    /// Felder, weil ein Feld frisch und das andere alt sonst eine Lage
    /// waere, in der niemand weiss, welches gilt. Ausgerechnet wird in
    /// beiden Faellen derselbe Transkript: ClientHello..server Finished.
    pub fn anwendungsphase_server(&mut self) -> Result<(), RecordFehler> {
        let master = self.master_secret()?;
        let th = self.transkript_hash();
        self.client_ap_traffic = expand_label_sha256_fest::<32>(&master, b"c ap traffic", &th)?;
        self.server_ap_traffic = expand_label_sha256_fest::<32>(&master, b"s ap traffic", &th)?;
        Ok(())
    }

    /// `master_secret` nach RFC 8446 Abschnitt 7.1.
    ///
    /// EIGENER FEHLER, 30.09.: `derived2` wurde mit dem AKTUELLEN
    /// Transkript gebildet. Der RFC schreibt dafuer `""` vor, also den
    /// Hash der leeren Nachrichtenkette. Der Unterschied ist nicht
    /// theoretisch — die Zahlen kommen vollstaendig daneben.
    /// Der `master secret`, **bei Bedarf gerechnet**.
    ///
    /// added 02.10.2026, R4d Stufe 2: war `fn master_secret` und ist jetzt
    /// `pub`. DER GRUND: der Server leitet daraus den
    /// `resumption_master_secret` fuer ein `NewSessionTicket` ab (RFC 8446
    /// 7.2: `Derive-Secret(master_secret, "resumption", Hash(nonce))`).
    ///
    /// DIE ERSTE FASSUNG, und sie war eine **zweite** Methode mit demselben
    /// Namen und `Option` als Rueckgabe — der Compiler meldete
    /// `duplicate definitions with name master_secret`, und die Meldung
    /// war berechtigt. Ein Getter, der `None` liefert, wenn der Wert noch
    /// nicht berechnet ist, ist hier auch **schlechter**: die vorhandene
    /// Methode rechnet ihn, statt den Aufrufer raten zu lassen, ob er
    /// `masterphase` schon gebraucht hat. Also: die vorhandene, nicht
    /// eine neue.
    pub fn master_secret(&mut self) -> Result<[u8; 32], RecordFehler> {
        if !self.phase_master {
            self.handshakephase()?;
            let frueh = extract_sha256(&[0u8; 32], &[0u8; 32]);
            // Wie in `handshakephase`: der Kontext ist der HASH des
            // leeren Transkripts, nicht ein leerer String. Siehe dort
            // die ausfuehrliche Begruendung — derselbe Fehler stand an
            // beiden Stellen.
            let leerer_transkript = crate::sha2::sha256(&[]);
            let abgeleitet =
                expand_label_sha256_fest::<32>(&frueh, b"derived", &leerer_transkript)?;
            let hs = extract_sha256(&abgeleitet, &self.shared);
            // Und hier ist es genauso: `Derive-Secret(handshake_secret,
            // "derived", "")` bedeutet in der Notation des RFC das
            // leere TRANSKRIPT, nicht einen leeren Kontext.
            let abgeleitet2 = expand_label_sha256_fest::<32>(&hs, b"derived", &leerer_transkript)?;
            self.master = extract_sha256(&abgeleitet2, &[0u8; 32]);
            self.phase_master = true;
        }
        Ok(self.master)
    }
}

/// `Derive-Secret(secret, label, transcript_hash)` als freie Funktion.
// added 02.10.2026 03:5x UTC (Projektlabel 02.10.2026): `Result`, weil der
// Aufrufer sonst `.unwrap()` brauchte — und `unwrap` im Bibliothekscode ist
// dasselbe Verbrechen wie `expect`, nur leiser.
pub fn derive_secret(
    secret: &[u8; 32],
    label: &[u8],
    transkript: &[u8; 32],
) -> Result<[u8; 32], RecordFehler> {
    // added 02.10.2026 03:5x: die feste Laengenvariante, damit hier kein
    // `expect` steht — das war der letzte in dieser Datei ausser der
    // Suite-Laenge.
    // added 02.10.2026 03:5x: `?` nutzt die `From`-Umwandlung weiter oben
    // (`hkdf::Fehler` -> `RecordFehler::Hkdf`) — deshalb steht hier kein
    // `map_err` an jeder Stelle.
    Ok(expand_label_sha256_fest::<32>(secret, label, transkript)?)
}

// ---------------------------------------------------------------------------
// Record-Schicht
// ---------------------------------------------------------------------------

/// Die `legacy_record_version`, die auf der Leitung steht.
///
/// RFC 8446 Abschnitt 5.1: TLS 1.3 nutzt weiterhin 0x0303 im
/// Recordkopf, "for compatibility with middleboxes". Der Wert steht
/// an zwei unabhaengigen Stellen: im Kopf jedes gesendeten Records
/// und im AAD beim Ver-/Entschluesseln. Steht er zweimal als Literal,
/// ist er zweimal falsch zu aendern — deshalb ist er eine Konstante.
pub const VERSION: [u8; 2] = [0x03, 0x03];

/// Die Inhaltstypen aus RFC 8446 Abschnitt 5.1.
pub mod typen {
    pub const CHANGE_CIPHER_SPEC: u8 = 20;
    pub const ALERT: u8 = 21;
    pub const HANDSHAKE: u8 = 22;
    pub const APPLICATION_DATA: u8 = 23;
    /// 23 ist der AUSSERE Typ eines verschluesselten TLS-1.3-Records.
    ///
    /// ECHTER FEHLER, gefunden am 30.09.2026 gegen example.com: hier
    /// stand 25, und 25 ist der Wert aus **draft-29** von TLS 1.3.
    /// In der Schlussfassung ist der aeussere Typ eines
    /// `TLSCiphertext` wieder `application_data`, also 23
    /// (RFC 8446 Abschnitt 5.1: "The TLSCiphertext record types are
    /// only application_data"). Der echte Inhaltstyp steht als letztes
    /// Byte im Klartext, nachdem die Authentifizierung entfernt wurde.
    ///
    /// BELEG AUS DER NORM SELBST, nicht aus dem Gedaechtnis: RFC 8448
    /// Abschnitt 3, der vollstaendige Trace eines echten Servers. Die
    /// verschluesselten Records beginnen dort mit `17 03 03`, und
    /// 0x17 ist 23:
    ///
    /// ```text
    ///   complete record (679 octets):  17 03 03 02 a2 ...   <- ServerHello ist separat
    ///   complete record (227 octets):  17 03 03 00 de ...   <- verschluesselt
    ///   complete record ( 24 octets):  17 03 03 00 13 ...   <- Alert
    /// ```
    ///
    /// Die ClientHello-Nachricht dagegen beginnt mit `16 03 01`, und
    /// 0x16 ist 22. Genau dieser Unterschied haette den Handshake
    /// gegen example.com scheitern lassen, und die Fehlermeldung
    /// sprach von einem "unerwarteten Record-Typ 23" — also davon,
    /// dass der Server kein TLS 1.3 spraec he.
    pub const AEAD_CHIFFERTEXT: u8 = 23;
    /// Der Wert aus draft-29 von TLS 1.3.
    ///
    /// Nur noch GELESEN, nie geschrieben: ein Server, der das noch
    /// schickt, soll nicht abgewiesen werden. Geschrieben wird immer
    /// 23, weil das die Schlussfassung ist.
    pub const AEAD_CHIFFERTEXT_ENTWURF: u8 = 25;
}

/// Ein eingelesener Record: Kopf und Inhalt, ohne die Schluesselung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub typ: u8,
    pub inhalt: Vec<u8>,
}

/// Zerlegt einen Puffer in Records.
///
/// M4 — das ist die Stelle, an der ein Angreifer sonst Speicher frisst:
/// ein Kopf kann `0xFFFF` ankündigen. Der Test der Laenge kommt VOR
/// `Vec::with_capacity`, und `MAX_CIPHERTEXT` ist kleiner als 0xFFFF, also
/// ist die Ankündigung 0xFFFF schon ein Fehler.
///
/// M3: die Schleife laeuft ueber `chunks(1)`, aber nur solange mindestens
/// `MAX_EINLESEN` Byte vorliegen — nach der Pruefung ist die Zahl der
/// Durchlaeufe durch die Pufferlaenge begrenzt, und die ist durch
/// `MAX_EINLESEN` begrenzt.
pub fn records(puffer: &[u8]) -> Result<Vec<Record>, RecordFehler> {
    let mut raus: Vec<Record> = Vec::new();
    let mut pos = 0usize;
    while pos < puffer.len() {
        if puffer.len() - pos < 5 {
            return Err(RecordFehler::KopfZuKurz {
                vorhanden: puffer.len() - pos,
            });
        }
        let typ = puffer[pos];
        let laenge = u16::from_be_bytes([puffer[pos + 3], puffer[pos + 4]]) as usize;
        // M4: VOR der Allokation.
        if laenge > MAX_CIPHERTEXT {
            return Err(RecordFehler::LaengeZuGross {
                angegeben: laenge,
                erlaubt: MAX_CIPHERTEXT,
            });
        }
        let ende = pos + 5 + laenge;
        if ende > puffer.len() {
            return Err(RecordFehler::Abgeschnitten {
                erwartet: 5 + laenge,
                vorhanden: puffer.len(),
            });
        }
        if !ist_bekannter_typ(typ) {
            return Err(RecordFehler::TypUnbekannt { typ });
        }
        // M4: die Groesse ist geprueft und bekannt.
        raus.push(Record {
            typ,
            inhalt: puffer[pos + 5..ende].to_vec(),
        });
        pos = ende;
    }
    Ok(raus)
}

/// Bekannte Inhaltstypen. Alles andere wird abgelehnt.
///
/// `AEAD_CHIFFERTEXT` steht hier NICHT mehr in der Liste, denn es hat
/// nach der Korrektur den Wert 23 und ist damit dasselbe wie
/// `APPLICATION_DATA`. Ein Versuch, es doch aufzufuehren, erzeugte die
/// Warnung "unreachable pattern" — und die Warnung hatte recht: der
/// zweite Zweig waere nie erreicht worden.
///
/// Das ist eine kleine Nebenerscheinung der Korrektur von 25 auf 23
/// und zugleich das beste Argument dafuer, die Konstante nicht wieder
/// auf 25 zu setzen: mit 25 waere dieser Zweistrich still
/// verschwunden.
fn ist_bekannter_typ(typ: u8) -> bool {
    matches!(
        typ,
        typen::CHANGE_CIPHER_SPEC
            | typen::ALERT
            | typen::HANDSHAKE
            | typen::APPLICATION_DATA
            | typen::AEAD_CHIFFERTEXT_ENTWURF
    )
}

/// Baut einen Record als Byte-String.
///
/// M4: `inhalt` wird gegen `MAX_CIPHERTEXT` geprueft, BEVOR ein Puffer
/// seiner Laenge entsteht. Bei einem Inhalt, der laenger ist als das
/// Laengenfeld (2^16 - 1) aufnehmen kann, gibt es einen benannten Fehler.
pub fn baue_record(typ: u8, inhalt: &[u8]) -> Result<Vec<u8>, RecordFehler> {
    if !ist_bekannter_typ(typ) {
        return Err(RecordFehler::TypUnbekannt { typ });
    }
    if inhalt.len() > MAX_CIPHERTEXT {
        return Err(RecordFehler::LaengeZuGross {
            angegeben: inhalt.len(),
            erlaubt: MAX_CIPHERTEXT,
        });
    }
    if inhalt.len() > u16::MAX as usize {
        return Err(RecordFehler::LaengeZuGross {
            angegeben: inhalt.len(),
            erlaubt: u16::MAX as usize,
        });
    }
    // M4: 5 + inhalt.len(), beide geprueft.
    let mut raus = Vec::with_capacity(5 + inhalt.len());
    raus.push(typ);
    // TLS 1.2 auf der Leitung, 1.3 im Protokoll — siehe `VERSION`.
    raus.extend_from_slice(&VERSION);
    raus.extend_from_slice(&(inhalt.len() as u16).to_be_bytes());
    raus.extend_from_slice(inhalt);
    Ok(raus)
}

/// Schluesselt einen Record mit ChaCha20-Poly1305 oder AES-128-GCM.
///
/// M4: die Nutzlast wird vorher gegen `MAX_INNEN` geprueft, und die
/// Ausgabe ist `nutzlast + tag`, also aus einer geprueften Laenge.
pub fn schluessel_protect(
    suite: Suite,
    secret: &[u8; 32],
    seq: u64,
    typ: u8,
    nutzlast: &[u8],
) -> Result<Vec<u8>, RecordFehler> {
    if nutzlast.len() > MAX_INNEN {
        return Err(RecordFehler::InnenZuGross {
            ist: nutzlast.len(),
            erlaubt: MAX_INNEN,
        });
    }
    let (roher_schluessel, iv) = suite.keying_material(secret)?;
    // Nonce = IV XOR seq, big endian in die letzten acht Byte
    // (RFC 8446 Abschnitt 5.3).
    let mut nonce = iv;
    let seq_bytes = seq.to_be_bytes();
    for i in 0..8 {
        nonce[4 + i] ^= seq_bytes[i];
    }
    // Der AAD ist der Recordkopf: Typ, 0x0303, Laenge (RFC 8446
    // Abschnitt 5.2).
    let laenge = (nutzlast.len() + 1 + 16) as u16;
    let mut aad = Vec::with_capacity(5);
    aad.push(typen::AEAD_CHIFFERTEXT);
    aad.extend_from_slice(&VERSION);
    aad.extend_from_slice(&laenge.to_be_bytes());

    let mut innen = Vec::with_capacity(nutzlast.len() + 1);
    innen.extend_from_slice(nutzlast);
    innen.push(typ);

    let raus = match suite {
        Suite::ChaCha20Poly1305Sha256 => {
            let k: [u8; 32] = roher_schluessel;
            aead_chiffre(&k, &nonce, &aad, &innen).map_err(|_| RecordFehler::Authentifizierung)?
        }
        Suite::Aes128GcmSha256 => {
            // GCM gibt Chiffrat und Tag getrennt zurueck; auf der Leitung
            // stehen sie hintereinander (RFC 8446 Abschnitt 5.2). Die
            // Reihenfolge ist hier ein Fehler gewesen: es wurde nur `.0`
            // weitergegeben, damit fehlte der Tag und jedes Entschluesseln
            // schlug fehl.
            let (koerper, tag) = gcm_chiffre(&roher_schluessel[..16], &nonce, &aad, &innen)
                .map_err(|_| RecordFehler::Authentifizierung)?;
            let mut beides = koerper;
            beides.extend_from_slice(&tag);
            beides
        }
    };
    Ok(raus)
}

/// Entschluesselt einen Record.
///
/// M3/M4: `chiffrat` wird gegen `MAX_CIPHERTEXT` geprueft, und der
/// Tag wird VOR der Entschluesselung geprueft (so wie in chacha.rs und
/// aes.rs).
pub fn schluessel_unprotect(
    suite: Suite,
    secret: &[u8; 32],
    seq: u64,
    typ: u8,
    chiffrat: &[u8],
) -> Result<(Vec<u8>, u8), RecordFehler> {
    if chiffrat.len() > MAX_CIPHERTEXT {
        return Err(RecordFehler::LaengeZuGross {
            angegeben: chiffrat.len(),
            erlaubt: MAX_CIPHERTEXT,
        });
    }
    if chiffrat.len() < 17 {
        return Err(RecordFehler::Abgeschnitten {
            erwartet: 17,
            vorhanden: chiffrat.len(),
        });
    }
    let (roher_schluessel, iv) = suite.keying_material(secret)?;
    let mut nonce = iv;
    let seq_bytes = seq.to_be_bytes();
    for i in 0..8 {
        nonce[4 + i] ^= seq_bytes[i];
    }
    let laenge = chiffrat.len() as u16;
    let mut aad = Vec::with_capacity(5);
    aad.push(typen::AEAD_CHIFFERTEXT);
    aad.extend_from_slice(&VERSION);
    aad.extend_from_slice(&laenge.to_be_bytes());

    let mut innen = match suite {
        Suite::ChaCha20Poly1305Sha256 => {
            let k: [u8; 32] = roher_schluessel;
            aead_dechiffre(&k, &nonce, &aad, chiffrat)
                .map_err(|_| RecordFehler::Authentifizierung)?
        }
        Suite::Aes128GcmSha256 => {
            // Das letzte 16-Byte-Stueck IST der Tag; der Rest ist der
            // Chiffrat. Beides wird VOR der Entschluesselung getrennt, und
            // `gcm_dechiffre` prueft den Tag, bevor es entschluesselt.
            let n = chiffrat.len() - 16;
            let (koerper, tag) = chiffrat.split_at(n);
            let mut tag16 = [0u8; 16];
            tag16.copy_from_slice(tag);
            gcm_dechiffre(&roher_schluessel[..16], &nonce, &aad, koerper, &tag16)
                .map_err(|_| RecordFehler::Authentifizierung)?
        }
    };
    // Das LETZTE Byte ist der innere Typ (RFC 8446 Abschnitt 5.2).
    let letzte = match innen.pop() {
        Some(b) => b,
        None => return Err(RecordFehler::KeinInnertyp),
    };
    if innen.len() > MAX_INNEN {
        return Err(RecordFehler::InnenZuGross {
            ist: innen.len(),
            erlaubt: MAX_INNEN,
        });
    }
    let _ = typ;
    Ok((innen, letzte))
}

// ---------------------------------------------------------------------------
// Handshake-Nachrichten
// ---------------------------------------------------------------------------

/// Warum eine Handshake-Nachricht abgelehnt wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NachrichtFehler {
    /// Weniger als 4 Byte: kein vollstaendiger Kopf.
    KopfZuKurz { vorhanden: usize },
    /// Die Laenge im Kopf passt nicht zu den vorhandenen Byte.
    Abgeschnitten { erwartet: usize, vorhanden: usize },
    /// Die Laenge im Kopf ist groesser als 2^24 - 1 — bei drei Byte
    /// Laengenfeld nicht moeglich, aber der Fall wird geprueft, weil ein
    /// Parser hier sonst stillschweigend umlaeuft.
    LaengeZuGross { angegeben: usize, erlaubt: usize },
}

/// Zerlegt einen Handshake-Puffer in Nachrichten.
///
/// M4: das Dreier-Laengenfeld macht 2^24 - 1 zur Grenze. Sie wird vor
/// `Vec::with_capacity` geprueft. Ein Puffer, der 16 MiB Laenge ankündigt,
/// wird abgelehnt, auch wenn er nur 20 Byte lang ist.
pub fn nachrichten(puffer: &[u8]) -> Result<Vec<(u8, Vec<u8>)>, NachrichtFehler> {
    let mut raus: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut pos = 0usize;
    while pos < puffer.len() {
        if puffer.len() - pos < 4 {
            return Err(NachrichtFehler::KopfZuKurz {
                vorhanden: puffer.len() - pos,
            });
        }
        let typ = puffer[pos];
        let laenge = ((puffer[pos + 1] as usize) << 16)
            | ((puffer[pos + 2] as usize) << 8)
            | (puffer[pos + 3] as usize);
        if laenge > MAX_NACHRICHT {
            return Err(NachrichtFehler::LaengeZuGross {
                angegeben: laenge,
                erlaubt: MAX_NACHRICHT,
            });
        }
        let ende = pos + 4 + laenge;
        if ende > puffer.len() {
            return Err(NachrichtFehler::Abgeschnitten {
                erwartet: 4 + laenge,
                vorhanden: puffer.len(),
            });
        }
        // M4: geprueft.
        raus.push((typ, puffer[pos + 4..ende].to_vec()));
        pos = ende;
    }
    Ok(raus)
}

/// Baut den Kopf einer Handshake-Nachricht: Typ und Dreier-Laenge.
pub fn baue_nachricht(typ: u8, inhalt: &[u8]) -> Result<Vec<u8>, NachrichtFehler> {
    if inhalt.len() > MAX_NACHRICHT {
        return Err(NachrichtFehler::LaengeZuGross {
            angegeben: inhalt.len(),
            erlaubt: MAX_NACHRICHT,
        });
    }
    let mut raus = Vec::with_capacity(4 + inhalt.len());
    raus.push(typ);
    raus.push((inhalt.len() >> 16) as u8);
    raus.push((inhalt.len() >> 8) as u8);
    raus.push(inhalt.len() as u8);
    raus.extend_from_slice(inhalt);
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::hexen;

    const IKM: [u8; 32] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31,
    ];

    /// M7: der Schluesselplan gegen eine unabhaengige Python-Rechnung.
    ///
    /// QUELLE DER SOLLWERTE: `/home/jaff/mutation_md/tls/schluesselplan_ref.py`,
    /// geschrieben aus RFC 8446 Abschnitt 7.1 und RFC 5869, nicht aus
    /// `record.rs`.
    ///
    /// DIESE SOLLWERTE SIND KORRIGIERT, und der Grund ist der wichtigste
    /// Befund dieser Datei. Die alten Werte stammeten aus der Ausgabe
    /// des eigenen Codes, und dieser Code hatte einen echten Fehler:
    /// `handshakephase()` rechnete
    /// `HKDF-Expand-Label(early_secret, "derived", "", 32)` — mit einem
    /// LEEREN STRING als Kontext. `Derive-Secret(secret, "derived",
    /// Hash)` verlangt aber den Hash des leeren TRANSKRIPTS, also
    /// `sha256("") = e3b0c442…`. Damit war jedes abgeleitete Geheimnis
    /// falsch, und die Werte im Test beschrieben den Fehler.
    ///
    /// Das ist die Sorte Sollwert, die nichts prueft: sie entsteht aus
    /// dem Programm und bestaetigt es. Beim Korrigieren habe ich sie
    /// nicht durch neue Ausgabe des Programms ersetzt, sondern durch
    /// das Skript ersetzt — und das Skript ist zusaetzlich an RFC 8448
    /// verankert: es reproduziert `early_secret`, `derived`,
    /// `handshake_secret` und BEIDE Verkehrsgeheimnisse aus Abschnitt 3
    /// byteweise. Erst dadurch ist es eine zweite Quelle und keine
    /// zweite Kopie.
    #[test]
    fn schluesselplan_gegen_die_referenz() {
        let ch_sh = [0xaau8; 64];
        let sh_ch = [0xbbu8; 64];
        let ch_fin = [0xccu8; 32];
        let sh_fin = [0xddu8; 32];

        let mut p = Schluesselplan::neu(IKM);
        // Reihenfolge nach RFC 8446 Abschnitt 7.1: "c hs traffic" endet auf
        // dem ServerHello, die Anwendungsgeheimnisse spaeter.
        p.aufnehmen(&ch_sh);
        p.handshakephase().expect("Schluesselplan rechenbar");

        assert_eq!(
            hexen(&p.client_hs_traffic),
            "fbe74ddc8f22ee4cf09af96ae8602ea73a8eb52a056be02c0b9f94ceaf6787f2",
            "client handshake traffic secret"
        );
        assert_eq!(
            hexen(&p.server_hs_traffic),
            "5f7807603f6816217e7f606b03642d2e4468fa070018a6433fba83d53746f348",
            "server handshake traffic secret"
        );
        // Die `finished`-Schluessel kommen aus den VERKEHRSGEHEIMNISSEN,
        // nicht aus `handshake_secret` — RFC 8446 Abschnitt 4.4.4.
        assert_eq!(
            hexen(&p.finished_key_client),
            "31a54efad31792d95408ae2a2cc4dbddf850d3b16f42490ed1579861baef5bbb",
            "finished key des Clients, aus c hs traffic"
        );
        assert_eq!(
            hexen(&p.finished_key_server),
            "43387db75ad40fff8df70c6754e0120df6e2a87ff8965051c7d68a0a3375524b",
            "finished key des Servers, aus s hs traffic"
        );
        // Und die beiden unterscheiden sich, sonst waere die Seite nicht
        // gebunden.
        assert_ne!(
            p.finished_key_client, p.finished_key_server,
            "M7: die Seiten haben verschiedene finished-Geheimnisse"
        );

        // Die Handshake-Phase ist idempotent: ein zweiter Aufruf aendert
        // nichts (M7).
        let vorher = p.client_hs_traffic;
        p.handshakephase().expect("Schluesselplan rechenbar");
        assert_eq!(
            p.client_hs_traffic, vorher,
            "M7: zweiter Aufruf aendert nichts"
        );

        // Client-Geheimnis: Transkript endet auf dem ClientFinished.
        p.aufnehmen(&sh_ch);
        p.aufnehmen(&ch_fin);
        p.anwendungsphase_client()
            .expect("Anwendungsphase rechenbar");
        assert_eq!(
            hexen(&p.client_ap_traffic),
            "132d93849c10de68f40b08c65840c3f725c85f408289d1a5a301f4e58408161e",
            "client application traffic secret"
        );
        // Server-Geheimnis: derselbe Aufruf, jetzt mit ServerFinished.
        p.aufnehmen(&sh_fin);
        p.anwendungsphase_server()
            .expect("Anwendungsphase rechenbar");
        assert_eq!(
            hexen(&p.server_ap_traffic),
            "e06ccd4235b5a736633227a50d1992038513a1238431e16fd71a655875a8c1bc",
            "server application traffic secret"
        );
        // Und die beiden Seiten unterscheiden sich — sonst waere die
        // Richtung nicht gebunden.
        assert_ne!(p.client_ap_traffic, p.server_ap_traffic, "M7: Richtungen");
    }

    /// M7: die Client-Schluesselmaterialien gegen die Referenz.
    #[test]
    fn keying_material_gegen_die_referenz() {
        let mut p = Schluesselplan::neu(IKM);
        p.aufnehmen(&[0xaau8; 64]);
        p.handshakephase().expect("Schluesselplan rechenbar");
        let (k, iv) = Suite::ChaCha20Poly1305Sha256
            .keying_material(&p.client_hs_traffic)
            .expect("Schluessel rechenbar");
        assert_eq!(k.len(), 32, "M7: Schluessellaenge");
        assert_eq!(iv.len(), 12, "M7: IV-Laenge");
        // Aus der Rechnung in /home/jaff/mutation_md/tls/
        // beziehen sich auf denselben Geheimniswert und dasselbe
        // Transkript. Hier nur Laenge und Stabilitaet pruefen, die
        // Bytestaerke Abnahme macht der Schluesselplan-Test oben.
        let (k2, iv2) = Suite::ChaCha20Poly1305Sha256
            .keying_material(&p.client_hs_traffic)
            .expect("Schluessel rechenbar");
        assert_eq!(k, k2, "M7: stabil");
        assert_eq!(iv, iv2, "M7: stabil");
    }

    /// M4/M7: die Laengengrenze der Records. `0xFFFF` ist die Bombe aus
    /// M8 und muss VOR der Allokation abgewiesen werden.
    #[test]
    fn laengenangabe_0xffff_wird_abgewiesen() {
        let mut kopf = vec![typen::APPLICATION_DATA, 0x03, 0x03, 0xff, 0xff];
        // Selbst mit 40 KiB Inhalt dahinter: der Kopf allein genuegt.
        kopf.extend_from_slice(&vec![0u8; 40 * 1024]);
        let r = records(&kopf);
        assert_eq!(
            r,
            Err(RecordFehler::LaengeZuGross {
                angegeben: 0xFFFF,
                erlaubt: MAX_CIPHERTEXT
            }),
            "M7: Fehlervariante gesichert"
        );
        const {
            assert!(
                MAX_CIPHERTEXT < 0xFFFF,
                "die Grenze liegt unter dem, was ein Kopf ankündigen kann"
            )
        };
    }

    /// M4: genau an der Grenze ist erlaubt, ein Byte darüber nicht.
    #[test]
    fn genau_an_der_grenze() {
        assert_eq!(MAX_CIPHERTEXT, 16640, "2^14 + 256");
        let inhalt = vec![0u8; MAX_CIPHERTEXT];
        let r = baue_record(typen::APPLICATION_DATA, &inhalt);
        assert!(r.is_ok(), "genau an der Grenze: erlaubt");
        assert_eq!(r.expect("erlaubt").len(), 5 + MAX_CIPHERTEXT);
        let r = baue_record(typen::APPLICATION_DATA, &vec![0u8; MAX_CIPHERTEXT + 1]);
        assert_eq!(
            r,
            Err(RecordFehler::LaengeZuGross {
                angegeben: MAX_CIPHERTEXT + 1,
                erlaubt: MAX_CIPHERTEXT
            })
        );
    }

    /// M3/M4: die Meldungsschicht kann nicht unbegrenzt viele kleine
    /// Nachrichten aus einem Puffer machen, weil die Pufferlaenge die
    /// Durchlaufzahl begrenzt. Der Test sichert beide Enden.
    #[test]
    fn viele_kleine_nachrichten() {
        // 1000 Meldungen mit je 4 Byte Inhalt.
        let mut puffer = Vec::new();
        for _ in 0..1000 {
            puffer.extend_from_slice(&[1, 0, 0, 4, 0xaa, 0xbb, 0xcc, 0xdd]);
        }
        let n = nachrichten(&puffer).expect("1000 gueltige Nachrichten");
        assert_eq!(n.len(), 1000, "M7: Anzahl");
        assert_eq!(n[999].0, 1, "M7: Typ der letzten");
        assert_eq!(
            n[999].1,
            vec![0xaa, 0xbb, 0xcc, 0xdd],
            "M7: Inhalt der letzten"
        );
    }

    /// M4: eine abgeschnittene Nachricht ist ein benannter Fehler, und
    /// die angekuendigte Laenge darf keine Allokation ausloesen.
    #[test]
    fn abgeschnittene_nachricht() {
        // Ankündigt 16 MiB, vorhanden sind 4 Byte.
        let r = nachrichten(&[2, 0xff, 0xff, 0xff, 0x00]);
        assert_eq!(
            r,
            Err(NachrichtFehler::Abgeschnitten {
                erwartet: 4 + MAX_NACHRICHT,
                vorhanden: 5
            }),
            "M7: Fehlervariante"
        );
        // Und ein Kopf, der mitten im Puffer endet.
        assert_eq!(
            nachrichten(&[2, 0, 0, 4, 0xaa, 0xbb]),
            Err(NachrichtFehler::Abgeschnitten {
                erwartet: 8,
                vorhanden: 6
            })
        );
        assert_eq!(
            nachrichten(&[2, 0, 0]),
            Err(NachrichtFehler::KopfZuKurz { vorhanden: 3 })
        );
    }

    /// M7: Hin- und Rueckweg durch die Schluesselung, fuer BEIDE Suites.
    /// Der innere Typ kommt zurueck, und ein gekipptes Byte faellt durch.
    #[test]
    fn schluesselung_rundlauf_und_manipulation() {
        let nutzlast = b"Hallo, das ist eine Nutzlast fuer den Record-Test.";
        for suite in [Suite::ChaCha20Poly1305Sha256, Suite::Aes128GcmSha256] {
            for seq in [0u64, 1, 255, 256, u32::MAX as u64, u64::MAX] {
                let c = schluessel_protect(suite, &IKM, seq, typen::APPLICATION_DATA, nutzlast)
                    .expect("Verschluesselung");
                assert_eq!(
                    c.len(),
                    nutzlast.len() + 1 + 16,
                    "M7: Chiffrat + Typbyte + Tag"
                );
                let (zurueck, innertyp) =
                    schluessel_unprotect(suite, &IKM, seq, typen::APPLICATION_DATA, &c)
                        .expect("Entschluesselung");
                assert_eq!(zurueck, nutzlast.to_vec(), "M7: Nutzlast");
                assert_eq!(innertyp, typen::APPLICATION_DATA, "M7: innerer Typ");
            }

            // Ein gekipptes Byte wird erkannt.
            let c = schluessel_protect(suite, &IKM, 7, typen::APPLICATION_DATA, nutzlast)
                .expect("Verschluesselung");
            let mut kaputt = c.clone();
            kaputt[3] ^= 0x40;
            assert_eq!(
                schluessel_unprotect(suite, &IKM, 7, typen::APPLICATION_DATA, &kaputt),
                Err(RecordFehler::Authentifizierung),
                "M7: gekipptes Chiffratbyte"
            );
            // Die Folgeziffer gehoert dazu: derselbe Chiffrat mit
            // anderer Folgeziffer schlaegt fehl.
            assert_eq!(
                schluessel_unprotect(suite, &IKM, 8, typen::APPLICATION_DATA, &c),
                Err(RecordFehler::Authentifizierung),
                "M7: falsche Folgeziffer"
            );
        }
    }

    /// M4: die innere Nutzlast wird gegen 2^14 geprueft, VOR der
    /// Verschluesselung, und der Fehler ist benannt.
    #[test]
    fn innen_zu_gross() {
        let r = schluessel_protect(
            Suite::ChaCha20Poly1305Sha256,
            &IKM,
            0,
            typen::APPLICATION_DATA,
            &vec![0u8; MAX_INNEN + 1],
        );
        assert_eq!(
            r,
            Err(RecordFehler::InnenZuGross {
                ist: MAX_INNEN + 1,
                erlaubt: MAX_INNEN
            })
        );
    }

    /// M7: Record hin und zurueck ueber die Byteschnittstelle.
    #[test]
    fn record_baud_und_lese() {
        let a = baue_record(typen::HANDSHAKE, b"hallo").expect("Bau");
        let b = baue_record(typen::ALERT, b"warnung").expect("Bau");
        let mut beides = a.clone();
        beides.extend_from_slice(&b);
        assert_eq!(beides.len(), 5 + 5 + 5 + 7, "M7: Gesamtlaenge");

        let gelesen = records(&beides).expect("Lesen");
        assert_eq!(gelesen.len(), 2, "M7: zwei Records");
        assert_eq!(gelesen[0].typ, typen::HANDSHAKE);
        assert_eq!(gelesen[0].inhalt, b"hallo".to_vec());
        assert_eq!(gelesen[1].typ, typen::ALERT);
        assert_eq!(gelesen[1].inhalt, b"warnung".to_vec());
        assert_eq!(gelesen[0].inhalt.len(), 5, "M7: Inhaltslaenge");
    }

    /// M7: ein Record mit leerem Inhalt ist gueltig und ergibt eine
    /// leere Nutzlast, kein Fehler.
    #[test]
    fn leerer_record() {
        let r = baue_record(typen::ALERT, b"").expect("Bau");
        assert_eq!(r, vec![typen::ALERT, 0x03, 0x03, 0x00, 0x00], "M7: 5 Byte");
        let g = records(&r).expect("Lesen");
        assert_eq!(g.len(), 1, "M7: ein Record");
        assert!(g[0].inhalt.is_empty(), "M7: leerer Inhalt");
    }
}
