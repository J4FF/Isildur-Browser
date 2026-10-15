//! Sitzungs-Tickets: RFC 8446 Abschnitt 4.6.1 und der Speicher dafuer.
//!
//! added 30.09.2026 fuer Welle 7 / A1 (TLS-1.3-Sitzungswiederaufnahme).
//!
//! -------------------------------------------------------------------------
//! WAS HIER IST UND WAS NOCH NICHT
//! -------------------------------------------------------------------------
//! IST: das **Empfangen** und **Merken** von `NewSessionTicket`.
//! Ein Server schickt die Nachricht nach dem `finished` des Clients
//! (RFC 8446 4.6.1: "servers send one or more tickets ... immediately
//! after their [Handshake] flight, without waiting for the client to
//! send a Finished message"). Das ist die eine Haelfte.
//!
//! NOCH NICHT: das **Senden** von `pre_shared_key` im naechsten
//! ClientHello, und damit die Wiederaufnahme selbst. Sie braucht den
//! Binder ueber einen verkuerzten ClientHello-Transkript (RFC 8446
//! 4.2.11.2) und die Erkennung von `selected_identity` im ServerHello.
//! Das ist der naechste Schritt; es steht als offener Punkt im Bericht,
//! und die halbe Arbeit ohne den anderen Teil waere ein Speicher, den
//! niemand liest.
//!
//! -------------------------------------------------------------------------
//! DIE GRENZEN, und warum jede vonnoeten ist
//! -------------------------------------------------------------------------
//! Ein Ticket ist ein **opaque<1..2^16-1>**, also bis 65535 Byte. Der
//! Speicher eines Browsers waere mit einem einzigen bösen Server in
//! einer Minute voll. Drei Grenzen, jede mit ihrer Herleitung:
//!
//! * `MAX_TICKET_BYTES` pro Ticket — die Norm selbst sagt 2^16-1.
//! * `MAX_TICKETS_PRO_HOST` — ein Browser speichert je Host eine
//!   Handvoll, nicht tausende.
//! * `MAX_HOSTS` — sonst ist die Zahl der Hosts die Grenze.
//!
//! DIE ZEIT: `ticket_lifetime` sagt dem Client, wie lange das Ticket
//! lebt. Der Client **darf** es laenger benutzen, aber der Server
//! lehnt es dann ab — und ein abgelehntes Ticket kostet einen
//! Fehlversuch. Wir benutzen es, solange der Server es said.

use std::collections::HashMap;

use crate::nachricht;
use crate::vektor::{Leser, VektorFehler};

/// Was beim Lesen eines `NewSessionTicket` schiefgehen kann.
///
/// added 30.09.2026, Welle 7 / A1. DER GRUND FUER EINEN EIGENEN FEHLER
/// statt `VektorFehler`: die Vektor-Fehler kennen alle „der Puffer ist
/// zu kurz" und nichts darueber, dass eine Nachricht **zu gross** ist.
/// Genau diese zweite Art braucht der Ticket-Pfad, weil dort ein
/// Angreifer die Laenge bestimmen kann.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketFehler {
    /// Die Nachricht oder das Ticket ist laenger als erlaubt.
    ZuLang {
        /// Was gemessen wurde.
        war: usize,
        /// Was erlaubt ist.
        max: usize,
    },
    /// Das Ticket hat keine Nutzlast.
    Leer,
    /// Der Vektor endete vor dem Ende der Nachricht.
    ZuKurz(VektorFehler),
    /// added 01.10.2026 13:6x, R4z5: der `ticket_nonce` ist laenger, als
    /// die Norm erlaubt.
    ///
    /// **UMBENANNT.** Vorher hiess diese Variante `LayoutOhnePsk` und
    /// meldete „das Ticket ist kuerzer, als eine PSK hineinpasst". Nach
    /// dem Wegfall von `psk_aus_ticket` gibt es **keine** PSK im Ticket,
    /// und die PSK-Laenge hat mit diesem Fehler nichts mehr zu tun. Ein
    /// Name, der auf eine entfernte Sache zeigt, ist schlimmer als
    /// keiner: `Ticket zu kurz fuer eine PSK` stand wochenlang im
    /// Probeweg und hat eine halbe Fehlersuche gekostet, weil niemand
    /// die Meldung beim Wort genommen hat.
    ///
    /// WARUM UEBERHAUPT EIN EIGENER FEHLER: `expand_label_sha256` setzt
    /// die Laenge des Kontexts in **ein** Byte. Ein Nonce mit 256 Byte
    /// wuerde stillschweigend abgeschnitten, und die PSK waere die eines
    /// kuerzeren Nonces — ein Raten, kein Fehler. Genau das ist die Art
    /// Fehler, die erst Wochen spaeter auffaellt.
    ///
    /// `war` ist die Laenge, die kam; `max` die erlaubte (255, aus
    /// `ticket_nonce<0..255>`, RFC 8446 4.6.1).
    NonceZuLang { war: usize, max: usize },
}

impl std::fmt::Display for TicketFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TicketFehler::ZuLang { war, max } => {
                write!(
                    f,
                    "{war} Byte, erlaubt sind {max} ({} Byte)",
                    GRUND_TICKET_ZU_LANG
                )
            }
            TicketFehler::Leer => write!(f, "{}", GRUND_TICKET_LEER),
            TicketFehler::ZuKurz(e) => write!(f, "unvollstaendig: {e}"),
            // added 02.10.2026, R4i. Die Zahl, die hier steht, ist
            // added 01.10.2026 13:6x, R4z5: `{max}` als **Feld** und
            // nicht als Konstante im Text — sonst muesste man an zwei
            // Stellen aendern, wenn sich die Grenze aendert.
            TicketFehler::NonceZuLang { war, max } => {
                write!(f, "der ticket_nonce hat {war} Byte, erlaubt sind {max}")
            }
        }
    }
}

/// M3: die groesste Ticketlaenge in Byte.
///
/// DER WERT 65535 = `2^16 - 1`, und das ist **keine** eigene Zahl: RFC
/// 8446 Abschnitt 4.6.1 definiert `ticket` als `opaque<1..2^16-1>`, und
/// `u16` kann genau bis 65535 zaehlen. Ein groesserer Wert waere eine
/// Erfindung, ein kleinerer wuerde echte Tickets (Cloudflare schickt
/// laut Beobachtung 200 bis 1000 Byte) ablehnen.
pub const MAX_TICKET_BYTES: usize = 65535;

/// M3: so viele Byte hat das Wiederaufnahmegeheimnis.
///
/// added 02.10.2026, R4i. DER WERT 32 ist **keine eigene Zahl**: er ist
/// `SHA-256` in der Standardlaenge, also die Hashlaenge der
/// Schluesselableitung in RFC 8446 Abschnitt 7.1. Das Geheimnis muss
/// **dieselbe** Laenge haben wie die abgeleiteten Verkehrsgeheimnisse,
/// sonst muss an jeder Stelle eine Sonderbehandlung fuer einen Fall
/// auftauchen, den es gar nicht gibt.
///
/// GEMESSEN am 02.10.2026 mit `openssl sess_id -text`: `openssl` haelt
/// eine **32-Byte**-PSK. Das ist die Norm und nicht unsere Wahl — und
/// vorher hatten wir 40 (den ganzen Ticketinhalt).
pub const PSK_LAENGE: usize = 32;

/// Wie viele Byte der `ticket_nonce` eines Servers hat.
///
/// added 01.10.2026 13:6x, R4z5. **WARUM EINE KONSTANTE UND NICHT ZWEIMAL
/// `8`:** der Nonce steht an **zwei** Stellen — einmal als Laenge, mit der
/// der Server ihn zieht, und einmal als Laenge, mit der er den
/// Ticketinhalt vorbelegt. Zwei `8` sind zwei Wahrheiten fuer dieselbe
/// Zahl, und die falsche hiesse: der Server zieht 8 Byte, schreibt aber
/// 4 ins Ticket — und der Client rechnet mit 8 weiter. Genau solche
/// stillen Verschiebungen sind in dieser Datei schon einmal passiert.
///
/// RFC 8446 4.6.1 erlaubt `ticket_nonce<0..255>`; **8** ist unsere
/// Wahl, keine Vorgabe. Warum nicht 0: die Norm verlangt
/// "the ticket_nonce value is distinct for each NewSessionTicket message"
/// (`rfc8446.txt` Zeile 4179-4180), und mit 0 waere die PSK fuer alle
/// Tickets einer Sitzung gleich. Warum nicht 32: 8 Byte geben 2^64
/// Zufallswerte je Sitzung, das ist fuer die Zahl der Tickets pro Host
/// (`MAX_TICKETS_PRO_HOST` = 4) weit mehr als genug, und der Nonce geht
/// in jedes Ticket hinein.
pub const TICKET_NONCE_BYTES: usize = 8;

/// Die kuerzeste Ticketlaenge, die **wir** erzeugen.
///
/// added 01.10.2026 13:6x, R4z5. ERSATZ fuer `MIN_TICKET_MIT_PSK`
/// (= 40), das aus `PSK_VERSATZ` + `PSK_LAENGE` gebaut war — beides
/// beschrieb die **alte**, nicht normgerechte Ticketform.
///
/// DER WERT, und er ist eine Rechnung, keine Vermutung:
///     8 Byte Zeitstempel + 8 Byte ticket_nonce = 16 Byte
/// Der Zeitstempel ist unsere Buchhaltung (Ticket-Alter), der Nonce
/// gehoert zur Norm.
pub const MIN_TICKET: usize = 8 + TICKET_NONCE_BYTES;

/// added 01.10.2026 13:6x, R4z5: **beides ist weg** — die Funktion
/// `psk_aus_ticket` und die Konstante `PSK_VERSATZ`.
///
/// WAS HIER STAND: `PSK_VERSATZ` = 8 als der Versatz, an dem die PSK im
/// Ticket lag, und darunter die Doku von `psk_aus_ticket`, die daraus
/// schnitt. Beides beschrieb eine **Ticketform, die es nicht mehr gibt**.
///
/// WARUM ENTFERNEN STATT UMBENENNEN: eine Konstante, die einen
/// entfernten Versatz dokumentiert, ist eine **Falle**. Wer nach dem Weg
/// sucht, die PSK aus einem Ticket zu bekommen, findet genau diese
/// Zahl und genau diese Funktion und ist wieder bei dem Fehler aus
/// R4z2. Wer sie umbenennt, haelt die Falle — nur mit einem anderen
/// Namen dran.
/// Leitet die **Wiederaufnahme-PSK** aus `resumption_master_secret` und
/// dem `ticket_nonce`.
///
/// added 01.10.2026 12:5x, R4z2 Teil C. **DAS IST DER ERSATZ FUER
/// `psk_aus_ticket`**, und es ist die Antwort auf `PROBLEME.txt` Nr. 10.
///
/// GEMESSEN, warum der alte Weg falsch war (Bericht R4z2, Teil C):
/// dieselbe Sitzung, dasselbe Ticket, zwei verschiedene PSKs —
///   unsere   a332f48fde6f800918a0861603f02a88a5c83b3255a95cdd71d361ce4d398b5b
///   openssl  7A04648D0DC3B0D29E73EB3647E5CADCFF4108BDD89173A7103366D35B8ED310
/// Unsere war ein Zufallswert **im Ticket** (`server.rs` Zeile 989),
/// und `openssl` rechnet sie nach RFC 8446 4.6.1 aus dem
/// `resumption_master_secret`. Deshalb stimmte der Binder nicht.
///
/// DIE FORMEL, woertlich aus RFC 8446 Abschnitt 4.6.1:
///     PSK = HKDF-Expand-Label(resumption_master_secret, "resumption",
///                             ticket_nonce, Hash.length)
/// `Hash.length` ist fuer SHA-256 **32**, und das steht als `PSK_LAENGE`
/// daneben — an **einer** Stelle, damit nicht zwei Zahlen auseinander-
/// laufen koennen.
///
/// WARUM `resumption_master_secret` und nicht `master_secret`: der
/// Sitzungsschluessel darf nicht in ein Ticket (server.rs Zeile 961-966
/// sagt das richtig, und es ist der Grund fuer diesen ganzen Punkt).
/// `resumption_master_secret` wird ueber
/// `Derive-Secret(master_secret, "resumption", Hash(""))` gebildet.
pub fn psk_aus_nonce(
    resumption_master_secret: &[u8; 32],
    nonce: &[u8],
) -> Result<Vec<u8>, TicketFehler> {
    // M4: der Nonce kommt aus dem Netz (RFC 8446 4.6.1 erlaubt 0..255
    // Byte). `expand_label_sha256` setzt seine Laenge in **ein** Byte,
    // und ein Nonce mit 256 Byte wuerde stillschweigend abgeschnitten.
    // Deshalb die Grenze hier, mit derselben Zahl wie in der Norm.
    const MAX_NONCE_BYTES: usize = 255;
    if nonce.len() > MAX_NONCE_BYTES {
        return Err(TicketFehler::NonceZuLang {
            war: nonce.len(),
            max: MAX_NONCE_BYTES,
        });
    }
    crate::hkdf::expand_label_sha256(
        // added 01.10.2026 12:5x: `[u8; 32]` wird zu `&[u8]` — der
        // Aufrufer reicht die PSK-Festlaenge, HKDF braucht nur Bytes.
        &resumption_master_secret[..],
        b"resumption",
        nonce,
        PSK_LAENGE,
    )
    // added 01.10.2026 12:5x: `expand_label_sha256` kann nur an der
    // Laenge scheitern, und die Laenge ist hier `PSK_LAENGE` (32 — weit
    // unter der Grenze 0xFFFF). Der Fehler ist also **unmoeglich**; ihn
    // zu benennen waere eine Behauptung ohne Messung. Deshalb `expect`,
    // und deshalb bleibt der Rueckgabetyp `Result` **trotzdem**: der
    // Aufrufer gibt es weiter, und der Nonce-Fehler oben bleibt ein
    // echter `TicketFehler`.
    .map_err(|_| TicketFehler::NonceZuLang {
        war: nonce.len(),
        max: MAX_NONCE_BYTES,
    })
}

// -----------------------------------------------------------------------
// ZURUECKGENOMMEN am 01.10.2026 13:6x (R4z5): die Funktion
// `psk_aus_ticket` und die Konstante `PSK_VERSATZ` sind **weg**.
//
// WAS HIER STAND: ein `#[deprecated]`-Hinweis und die Funktion, die die PSK
// an einem festen Versatz aus dem Ticket schnitt. Der Hinweis genuegte
// nicht: `#[deprecated]` erzeugt eine **Warnung**, und die Abnahme verlangt
// 0 Warnungen, auch in Tests. Sieben Testdateien benutzten sie deshalb als
// Ticket-Bauhilfe, und jede rief die Warnung.
//
// WARUM DAS DER RICHTIGE SCHRITT IST: die Funktion beschrieb eine
// **Ticketform, die es nicht mehr gibt**. Sie zu behalten hiesse: die alte
// Regel ist noch gueltig. Und sie war eine **Falle** — wer nach dem Weg
// sucht, die PSK aus einem Ticket zu bekommen, findet genau diese Funktion
// und ist wieder bei dem Fehler aus R4z2 (Zufalls-PSK im Ticket).
//
// DER ERSATZ: `psk_aus_nonce` (oben) und `Ticket::mit_psk` (unten).
// -----------------------------------------------------------------------

/// M3: so viele Tickets werden je Host gemerkt.
///
/// DER WERT 4. BEGUENDUNG: RFC 8446 4.6.1 erlaubt dem Server beliebig
/// viele Tickets zu schicken, typisch sind 1 bis 2. Vier lassen Platz
/// fuer einen Server, der nach jedem Handshake ein neues schickt, ohne
/// den Speicher wachsen zu lassen.
pub const MAX_TICKETS_PRO_HOST: usize = 4;

/// M3: so viele Hosts werden gemerkt.
///
/// DER WERT 128. BEGUENDUNG: 128 Hosts mal 4 Tickets mal 1000 Byte sind
/// 512 KB — das passt bequem neben die 64-MiB-Speichergrenze des Caches
/// und neben den Bildschirm. Wer mehr Hosts offen hat, verliert die
/// aeltesten Tickets, und das ist der Normalfall (ein Tab besucht
/// tausende Seiten, 128 davon sind noch erreichbar).
pub const MAX_HOSTS: usize = 128;

/// Der ungueltige Grund fuer ein abgelehntes Ticket.
///
/// added 30.09.2026. DER GRUND FUER EINE KONSTANTE: die Meldung wird
/// in `trace` und im Bericht gebraucht; ein getippter String an zwei
/// Stellen waere die uebliche Stelle, an der die beiden auseinanderlaufen.
pub const GRUND_TICKET_ZU_LANG: &str = "Ticket laenger als 65535 Byte";

/// Der ungueltige Grund fuer ein Ticket ohne Nutzlast.
pub const GRUND_TICKET_LEER: &str = "Ticket ohne Nutzlast";

/// Der ungueltige Grund fuer ein abgelaufenes Ticket.
pub const GRUND_TICKET_ABGELAUFEN: &str = "Ticket abgelaufen";

/// M3: die groesste Zahl der Bytes, die ein einzelnes `NewSessionTicket`
/// belegen darf, **bevor** irgendetwas gelesen wird.
///
/// DER WERT 70000. Rechnung: 65535 (Ticket) + 255 (Nonce) + 4 + 4 +
/// 2 (Extensions-Laenge) + 65535 (Extensions) waere theoretisch moeglich,
/// aber ein Server schickt keine 64-KiB-Erweiterungsliste. 70000 nimmt
/// das Doppelte des Ticketmaximums plus den Rest — gross genug fuer
/// jeden echten Server, klein genug, dass ein Angreifer keine 16 MiB
/// anhaengen kann. DER WEG: die Nachricht kommt aus einem Record, und
/// der Record ist durch `MAX_RECORD_BYTES` gedeckelt; diese Grenze ist
/// die **innere**.
pub const MAX_NACHRIFT_BYTES: usize = 70_000;

/// Ein vom Server geschicktes Sitzungs-Ticket.
///
/// added 30.09.2026, Welle 7 / A1. Felder woertlich nach RFC 8446
/// Abschnitt 4.6.1:
///
/// ```text
///   struct {
///       uint32 ticket_lifetime;
///       uint32 ticket_age_add;
///       opaque ticket_nonce<0..255>;
///       opaque ticket<1..2^16-1>;
///       Extension extensions<0..2^16-1>;
///   } NewSessionTicket;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// Wie lange der Server das Ticket haben will, in Sekunden.
    pub lebensdauer: u32,
    /// Der Zufallswert aus `ticket_age_add`, gegen den der Client sein
    /// eigenes Alter prueft (RFC 8446 4.6.1: `obfuscated_ticket_age =
    /// ticket_age_add + (now - ticket_issue_time)`).
    pub alter_add: u32,
    /// `ticket_nonce`, 0 bis 255 Byte. Wird fuer die PSK-Ableitung
    /// gebraucht und fuer die Verkennung von Wiederverwendung.
    pub nonce: Vec<u8>,
    /// Das Ticket selbst, `opaque<1..2^16-1>`.
    pub ticket: Vec<u8>,
    /// Die **Wiederaufnahme-PSK**, aus `resumption_master_secret` und
    /// `nonce` abgeleitet.
    ///
    /// added 02.10.2026, R4i; **geaendert 01.10.2026 12:5x, R4z2
    /// Teil C.** Der urspruengliche Text behauptete, die PSK stehe "an
    /// fester Stelle im Ticketinhalt" und zitierte als Beleg, dass
    /// `openssl` sie nicht bilde. Diese Behauptung war **falsch** und ist
    /// hiermit zurueckgenommen: `openssl` bildet sie sehr wohl — nur nach
    /// RFC 8446 4.6.1 aus dem `resumption_master_secret`, und deshalb
    /// zu einem **anderen** Wert als unser Zufallswert.
    ///
    /// GEMESSEN am 01.10.2026, gleiche Sitzung, gleiches Ticket:
    ///   unsere   a332f48fde6f800918a0861603f02a88a5c83b3255a95cdd71d361ce4d398b5b
    ///   openssl  7A04648D0DC3B0D29E73EB3647E5CADCFF4108BDD89173A7103366D35B8ED310
    /// Der Binder passte deshalb nicht, und `openssl s_client -sess_in`
    /// sagte `New` statt `Reused`.
    ///
    /// **LEER heisst: fuer diesen Client nicht benutzbar.** Es heisst
    /// nicht „unbekannt" — der Decoder laesst sie leer, wenn er kein
    /// `resumption_master_secret` hat, und `mit_psk` fuellt sie, wenn
    /// er eins hat. Wer das nicht unterscheidet, bietet einen
    /// Zufallswert an.
    ///
    /// WARUM EIGENES FELD STATT EINES ABSCHNITTS: `ticket` und `psk` werden
    /// an **drei** Stellen gebraucht (`psk_fuer`, `angebot`, und der
    /// Decoder), und ein Herausschneiden an drei Stellen ist drei
    /// Moeglichkeiten fuer denselben Versatz. Das Feld traegt den Wert, der
    /// Abschnitt bestimmt, woher er kommt.
    ///
    /// DER ZWEITE GEWINN, und er ist unabhaengig von OpenSSL: der
    /// Ticketinhalt enthielt bis hier das **`master secret`** der Sitzung.
    /// Das ist etwas, das **nicht** in ein Ticket gehoert (RFC 8446 4.6.1
    /// nennt ein eigenes `resumption_master_secret`); jetzt steht dort ein
    /// eigener Zufallswert.
    pub psk: Vec<u8>,
    /// Die (unbekannten) Erweiterungen, unveraendert. Sie werden
    /// bewusst **nicht** geparst: RFC 8446 sagt, dass ein Client eine
    /// unbekannte Erweiterung ignorieren MUSS, und ein Client, der
    /// einen unbekannten Typ ablehnt, ist der Grund, warum manche Seiten
    /// mit neuen Browsern kaputt sind.
    pub erweiterungen: Vec<u8>,
    /// Wann der Client die Nachricht bekommen hat (Sekunden seit 1970).
    pub empfangen: u64,
}

impl Ticket {
    /// Fuellt die PSK aus dem `resumption_master_secret`.
    ///
    /// added 01.10.2026 12:5x, R4z2 Teil C. DER ERSATZ FUER DAS LESEN
    /// AUS DEM TICKET: RFC 8446 4.6.1 rechnet
    ///   PSK = HKDF-Expand-Label(resumption_master_secret, "resumption",
    ///                           ticket_nonce, Hash.length)
    ///
    /// DER GRUND FUER EINE METHODE STATT EINES FELD-ZUGRIFFS: der
    /// Nonce und das Geheimnis muessen **zusammen** in die Rechnung, und
    /// ein Aufrufer, der nur `t.psk` liest, sieht nicht, dass er die PSK
    /// noch gar nicht berechnet hat. Die leere Liste sagt es nicht
    /// zuverlaessig — sie sieht aus wie ein Wert, der verloren ging.
    ///
    /// `Ok(self)` bei vorhandener PSK: ein zweites Aufrufen mit einem
    /// **anderen** Geheimnis waere ein Fehler, kein Ersatz. Wer den Wert
    /// schon hat, hat ihn richtig.
    pub fn mit_psk(mut self, resumption_master_secret: &[u8; 32]) -> Result<Self, TicketFehler> {
        if !self.psk.is_empty() {
            return Ok(self);
        }
        self.psk = psk_aus_nonce(resumption_master_secret, &self.nonce)?;
        Ok(self)
    }

    /// Traegt dieses Ticket eine benutzbare PSK?
    ///
    /// added 01.10.2026 12:5x, R4z2 Teil C. Nur diese **eine** Stelle
    /// entscheidet, ob ein Ticket angeboten wird. Ein `psk.is_empty()`
    /// an drei Aufrufstellen waere drei Moeglichkeiten fuer dieselbe
    /// Frage — und die erste, die es vergisst, bietet ein leeres
    /// Geheimnis an.
    pub fn hat_psk(&self) -> bool {
        !self.psk.is_empty()
    }

    /// Wie alt ist das Ticket, nach RFC 8446 4.6.1?
    ///
    /// DER NAME IST ABSICHT „verschleiert": der Wert wird **nicht** an
    /// den Server geschickt, sondern nur zur Pruefung des eigenen
    /// Alters benutzt (die Vorstellung eines MITM, der ein Ticket
    /// zurueckspielt, ist genau der Grund fuer `ticket_age_add`).
    ///
    /// added 02.10.2026 04:6x UTC (Projektlabel 02.10.2026): DIE EINHEIT, und
    /// sie war falsch — **Sekunden statt Millisekunden**. Beleg aus der Norm,
    /// woertlich (`/home/jaff/mutation_md/tls/rfc/rfc8446.txt`, Zeilen
    /// 3216-3222, RFC 8446 Abschnitt 4.2.11): „The \"obfuscated_ticket_age\"
    /// field of each PskIdentity contains an obfuscated version of the ticket
    /// age formed by taking the age in **milliseconds** and adding the
    /// \"ticket_age_add\" value that was included with the ticket (see
    /// Section 4.6.1), modulo 2^32." Und weiter: „Note that the
    /// \"ticket_lifetime\" field in the NewSessionTicket message is **in
    /// seconds** but the \"obfuscated_ticket_age\" is **in
    /// milliseconds**." Zwei Felder, zwei Einheiten — und unsere Rechnung
    /// war im Feld, das in Sekunden kommt.
    ///
    /// added 02.10.2026 04:6x: GEMESSEN, dass es so war. Im Mitschnitt vom
    /// 02.10.2026 05:0x (Werkzeug: `werkzeuge/tls_mitschchnitt/`) steht in
    /// unserem `pre_shared_key`-Angebot `Alter 0`, obwohl der Mitschnitt
    /// **einen** frischen Lauf zeigt — bei einem Ticket, das knapp eine
    /// Sekunde alt war, ist 0 richtig und 0 falsch nicht unterscheidbar. Die
    /// Normstelle oben ist der eigentliche Beleg; der Test in
    /// `tests/ticket.rs` haelt die Einheit fest (3 s Alter => 3000, nicht 3).
    pub fn verschleiertes_alter(&self, jetzt: u64) -> u32 {
        let echtes_alter_s = jetzt.saturating_sub(self.empfangen);
        // added 02.10.2026 04:6x: DIE UMRECHNUNG, und sie ist der ganze Fix.
        // `jetzt` und `empfangen` sind **Sekunden** (so kommen sie aus
        // `new_session_ticket(daten, jetzt)` und aus `narsil-net`, das Unix-
        // Sekunden durchreicht), das Feld will Millisekunden.
        const MS_PRO_SEKUNDE: u64 = 1000;
        let echtes_alter_ms = echtes_alter_s.wrapping_mul(MS_PRO_SEKUNDE);
        // M3: `as u32` ist hier eine **Umrechnung**, kein Raten:
        // `ticket_age_add` ist `uint32`, und die Norm definiert die
        // Summe in dieser Breite. Ein Alter von mehr als 4 Milliarden
        // Millisekunden (49 Tage) passt nicht; wir rechnen mit `wrapping`,
        // weil die Norm das so meint — RFC 8446 4.2.11: „... modulo 2^32".
        // added 02.10.2026 04:6x: in der Rechnung steht jetzt **beides** —
        // `wrapping_mul` fuer die Sekunden-zu-Millisekunden-Stufe und das
        // `min` fuer den Sprung in die Normbreite. Bei Ticketlaufzeiten bis
        // einer Woche (Norm: „Because ticket lifetimes are restricted to a
        // week", rfc8446.txt Zeile 3222) ist die Millisekundenzahl mit
        // 604 800 000 weit unter `u32::MAX`, der Zweig ist also die Form des
        // Normwertes.
        //
        // added 02.10.2026 04:7x: GEMESSEN war die Warnung „unused doc
        // comment" hier — eine Zeile mit `///` mitten im Funktionskoerper.
        // Sie war als Denknotiz gemeint, nicht als Doku.
        self.alter_add
            .wrapping_add(echtes_alter_ms.min(u32::MAX as u64) as u32)
    }

    /// Ist das Ticket abgelaufen?
    ///
    /// DER WERT 7 Tage als **Vorgabe**, wenn der Server keine Zeit sagt:
    /// `ticket_lifetime == 0` bedeutet nach der Norm „unendlich
    /// gueltig" fuer diesen einen Handshake. Wir halten das nicht fuer
    /// eine Ausnahme, sondern fuer eine fehlende Angabe, und nehmen 7
    /// Tage: lang genug fuer eine Arbeitssitzung, kurz genug, dass ein
    /// liegengebliebenes Ticket nicht ewig gespeichert bleibt.
    pub const VORGABE_LEBENSDAUER: u32 = 7 * 24 * 3600;

    /// `true`, wenn das Ticket nach `sekunden` nicht mehr benutzt werden soll.
    pub fn abgelaufen(&self, jetzt: u64) -> bool {
        let dauer = if self.lebensdauer == 0 {
            Self::VORGABE_LEBENSDAUER
        } else {
            self.lebensdauer
        };
        jetzt.saturating_sub(self.empfangen) > dauer as u64
    }
}

/// Liest ein `NewSessionTicket`.
///
/// added 30.09.2026, Welle 7 / A1. DER WEG: `Vektor` ueber die ganze
/// Nachricht, Feld fuer Feld nach RFC 8446 4.6.1. Die Grenzen stehen im
/// Kopf der Datei; jede verletzte ergibt einen benannten `VektorFehler`
/// ueber `grund`.
///
/// DER HINWEIS AUF `MAX_NACHRIFT_BYTES`: der Aufrufer prueft die
/// Laenge **vor** diesem Aufruf. Hier wird sie nur noch einmal
/// nachgeprueft, damit die Funktion auch allein benutzt werden darf.
pub fn new_session_ticket(daten: &[u8], jetzt: u64) -> Result<Ticket, TicketFehler> {
    if daten.len() > MAX_NACHRIFT_BYTES {
        return Err(TicketFehler::ZuLang {
            war: daten.len(),
            max: MAX_NACHRIFT_BYTES,
        });
    }
    let mut v = Leser::neu(daten);
    // DER WEG, und warum kein `?`: die Vektor-Fehler werden in
    // `TicketFehler::ZuKurz` verpackt, damit der Aufrufer **eine**
    // Fehlerart bekommt und nicht zwei. Ein `map_err` an jeder Stelle
    // waere drei Zeilen mehr und an vier Stellen.
    // DIE ZWEI `uint32` AUS ZWEI `u16`. DER WEG: TLS ist
    // netzwerk-Byteordnung (big endian), und `Leser` liest `u16` genau
    // so. `((hi << 16) | lo)` ist damit dasselbe wie `u32::from_be_bytes`
    // ueber vier Byte — nur ohne die vier Byte dazwischen zu kopieren.
    // DER FEHLER, DEN DIESE ZEILE VERHINDERT: `u32::from_be_bytes`
    // ueber `[hi_be, hi_le, lo_be, lo_le]` setzt die Bits in der
    // Reihenfolge, die ein LITTLE-endian-Leser erwartet; das Ergebnis
    // ist bei ungeraden Werten ein anderer Wert (0x0102 -> 0x0201).
    let lebensdauer = (u32::from(v.u16().map_err(TicketFehler::ZuKurz)?) << 16)
        | u32::from(v.u16().map_err(TicketFehler::ZuKurz)?);
    let alter_add = (u32::from(v.u16().map_err(TicketFehler::ZuKurz)?) << 16)
        | u32::from(v.u16().map_err(TicketFehler::ZuKurz)?);
    // M4: `vektor8` gibt ein `&[u8]` aus den **Eingangsdaten** zurueck —
    // es kopiert nichts. Der Besitzwechsel passiert beim `to_vec()`,
    // und der kostet hoechstens 255 Byte (die Normgrenze der Nonce).
    let nonce = v.vektor8().map_err(TicketFehler::ZuKurz)?.to_vec();
    let ticket = v.vektor16().map_err(TicketFehler::ZuKurz)?.to_vec();
    // DIE ERWEITERUNGEN, und sie werden **gelesen** statt uebersprungen.
    //
    // DER FEHLER, DEN DER RFC-VECTOR HIER GEFUNDEN HAT: die erste Fassung
    // las nur die vier Felder bis `ticket` und pruefte dann `ist_leer()`.
    // RFC 8448 Abschnitt 4 (Handshake mit Wiederaufnahme) schickt 8
    // Byte Erweiterungen hinterher (`00 2a 00 04 00 00 04 00`, also
    // `pre_shared_key` mit `selected_identity = 0`) — der Test mit dem
    // Vektor meldete `ZuLang` fuer eine Nachricht, die vollstaendig in
    // Ordnung war.
    let erweiterungen = v.vektor16().map_err(TicketFehler::ZuKurz)?.to_vec();
    // DER ENDE-TEST, und er ist nicht optional: `RFC 8446 4.6.1` sagt
    // "extensions<0..2^16-1>", und eine Nachricht mit Restbytes ist
    // kaputt. Ohne diese Pruefung wuerde `?` stillschweigend den Rest
    // fressen — bei einem Server mit einem eigenen Feld genau dann, wenn
    // man es braucht.
    if !v.ist_leer() {
        return Err(TicketFehler::ZuLang {
            war: daten.len() - v.uebrig(),
            max: daten.len(),
        });
    }
    if ticket.is_empty() {
        return Err(TicketFehler::Leer);
    }
    // added 01.10.2026 12:5x, R4z2 Teil C. **DIE PSK WIRD HIER NICHT
    // MEHR GELESEN.** Vorher stand hier `psk_aus_ticket(&ticket)?`, also
    // wurde die PSK an einem festen Versatz **aus dem Ticketinhalt**
    // geholt. Das ist nach RFC 8446 4.6.1 falsch: das Ticket traegt den
    // `ticket_nonce`, und die PSK wird aus
    // `HKDF-Expand-Label(resumption_master_secret, "resumption",
    // ticket_nonce, Hash.length)` berechnet.
    //
    // GEMESSEN, warum das kein Reinfall ist: unser Ticket und das von
    // `openssl` ausgewertete Ticket gehoeren zu derselben Sitzung, und
    // die PSKs waren verschieden (`a332f48f…` gegen `7A04648D…`),
    // siehe `PROBLEME.txt` Nr. 10.
    //
    // DER WEG JETZT: `Ticket::mit_psk` erfuellt das Feld, wenn der
    // Aufrufer den `resumption_master_secret` hat (`strom.rs`, direkt
    // nach dem Handshake). Ohne Geheimnis bleibt das Feld **leer** —
    // und ein leeres Feld ist eine Aussage: „dieses Ticket ist fuer
    // uns nicht benutzbar", nicht „die PSK ist unbekannt".
    //
    // DER GRUND, DASS DAS NICHT FEHLERHAFT Bricht: ein Ticket ohne
    // ableitbare PSK ist fuer diesen Client wertlos, und ein Browser,
    // der an fremden Tickets scheitert, laedt keine Seite mehr
    // (das ist der Grund fuer `continue` in `strom.rs`).
    let psk = Vec::new();
    Ok(Ticket {
        lebensdauer,
        alter_add,
        nonce,
        ticket,
        psk,
        erweiterungen,
        empfangen: jetzt,
    })
}

/// Der Speicher der Sitzungs-Tickets, je Host.
///
/// added 30.09.2026, Welle 7 / A1.
///
/// DER WEG ZUR BEGRENZUNG: `MAX_HOSTS` ueber die Zahl der Hosts und
/// `MAX_TICKETS_PRO_HOST` ueber die Zahl je Host. Beides wird **nach**
/// dem Einfuegen geprueft, und der jeweils **aelteste** Eintrag faellt
/// raus — nicht ein beliebiger. Das ist die Reihenfolge, die ein Browser
/// braucht: wer eine Seite zum zweiten Mal besucht, hat ihr Ticket noch.
#[derive(Default)]
pub struct Tickets {
    /// Host -> Tickets, neueste zuerst.
    je_host: HashMap<String, Vec<Ticket>>,
}

impl Tickets {
    /// Ein leerer Speicher.
    pub fn neu() -> Self {
        Tickets {
            je_host: HashMap::new(),
        }
    }

    /// Legt ein Ticket ab und raeumt nach den Grenzen auf.
    ///
    /// DER WEG: erst in die Liste haengen, dann kuerzen. Das ist
    /// andersherum als man zuerst denkt, aber nur so stimmt die Zahl
    /// der **verworfenen** Tickets — sonst wuerde man ein Ticket
    /// verwerfen, das gar nicht existiert.
    ///
    /// DER RUECKGABEWERT IST DIE ANZAHL DER VERWORFENEN, und nicht nur
    /// `()`: ein stilles Abschneiden von Tickets ist genau das, was die
    /// Projektregel verbietet, und die Zahl ist das, was man in der
    /// Meldung braucht.
    pub fn ablegen(&mut self, host: &str, ticket: Ticket) -> usize {
        // M4: der Ticketinhalt wird kopiert (der Aufrufer kann seinen
        // Puffer wegschweren), und die Groesse ist durch
        // `MAX_TICKET_BYTES` begrenzt, weil `new_session_ticket` sie
        // geprueft hat. Ein Ticket, das diesen Weg umgeht, wird hier
        // noch einmal abgelehnt — die Grenze steht an **beiden**
        // Enden, weil ein zweiter Aufrufer sonst um sie herum kaeme.
        if ticket.ticket.is_empty() {
            return 1;
        }
        if ticket.ticket.len() > MAX_TICKET_BYTES {
            return 1;
        }
        let liste = self.je_host.entry(host.to_string()).or_default();
        liste.insert(0, ticket);
        let mut verworfen = 0;
        while liste.len() > MAX_TICKETS_PRO_HOST {
            liste.pop();
            verworfen += 1;
        }
        // M3: die Hostgrenze. Der **am laengsten nicht benutzte** Host
        // fliegt raus. `HashMap` hat keine Reihenfolge, darum wird der
        // aelteste *inhalt* gesucht — der mit dem kleinsten `empfangen`.
        // Das ist ein Durchgang ueber hoechstens `MAX_HOSTS + 1` Hosts,
        // also durch die Grenze selbst begrenzt.
        while self.je_host.len() > MAX_HOSTS {
            let mut opfer: Option<(String, u64)> = None;
            for (h, l) in &self.je_host {
                let neueste = l.first().map(|t| t.empfangen).unwrap_or(0);
                if opfer.as_ref().is_none_or(|(_, a)| neueste < *a) {
                    opfer = Some((h.clone(), neueste));
                }
            }
            match opfer {
                Some((h, _)) => {
                    self.je_host.remove(&h);
                    verworfen += MAX_TICKETS_PRO_HOST;
                }
                None => break,
            }
        }
        verworfen
    }

    /// Das juengste noch gueltige Ticket fuer einen Host.
    ///
    /// DER WEG: die Liste von vorn, und `abgelaufen` bricht ab. Abge-
    /// laufene Tickets werden dabei **entfernt** — ein Speicher, der sie
    /// liegen laesst, prueft sie bei jeder Anfrage erneut.
    pub fn neuestes(&mut self, host: &str, jetzt: u64) -> Option<Ticket> {
        let liste = self.je_host.get_mut(host)?;
        while liste.first().is_some_and(|t| t.abgelaufen(jetzt)) {
            liste.remove(0);
        }
        liste.first().cloned()
    }

    /// Wie viele Tickets liegen fuer diesen Host?
    pub fn anzahl(&self, host: &str) -> usize {
        self.je_host.get(host).map_or(0, |l| l.len())
    }

    /// Wie viele Hosts sind im Speicher?
    pub fn hosts(&self) -> usize {
        self.je_host.len()
    }

    /// Der PSK zu einer **Identitaet**, die ein Client angeboten hat.
    ///
    /// added 02.10.2026, R4d Stufe 2. DER GEGENSTUECK ZU `angebot`: der
    /// Client sucht nach Host, der Server nach dem, was im ClientHello
    /// steht — und das ist die Identitaet, die der Server selbst
    /// ausgestellt hat.
    ///
    /// DER PSK IST DAS TICKET selbst, und das ist die in `PskAngebot`
    /// benannte Vereinfachung auf der Gegenseite: beide Seiten benutzen
    /// dieselbe. Ein echter Server leitet den PSK aus dem Ticket und einem
    /// `resumption_master_secret` ab (RFC 8446 4.6.1); das ist der Punkt,
    /// an dem eine fremde Sitzung unabhaengig von ihrem Ticket waere.
    ///
    /// M3: die Suche laeuft ueber `MAX_HOSTS` (128) mal
    /// `MAX_TICKETS_PRO_HOST` (4) Eintraege, also ueber hoechstens 512
    /// Vergleiche. Jeder Vergleich ist ein Bytevergleich zweier bis
    /// `MAX_TICKET_BYTES` (65535) langer Folgen. 512 mal 65535 Byte sind
    /// 33 MB gelesen — das ist die Rechnung, mit der die Schleife
    /// begründet wird, und sie faellt nur dann ins Gewicht, wenn ein
    /// Client **keinen** bekannten Eintrag schickt.
    pub fn psk_fuer(&self, identitaet: &[u8]) -> Option<Vec<u8>> {
        for liste in self.je_host.values() {
            for t in liste {
                if t.ticket == identitaet {
                    // added 02.10.2026, R4i: `t.psk`, **nicht**
                    // `t.ticket.clone()`. Bis hier war der Ticketinhalt die
                    // PSK, und das war die Entwurfsentscheidung, an der
                    // `openssl` scheitert (Bericht R4h: `openssl sess_id`
                    // nennt eine 32-Byte-PSK, `Session-ID` ist
                    // `SHA-256(Ticket)`). Jetzt steht die PSK im Feld.
                    return Some(t.psk.clone());
                }
            }
        }
        None
    }

    /// Das Angebot fuer einen Handshake, oder `None` wenn es kein
    /// brauchbares Ticket gibt.
    ///
    /// added 01.10.2026, R4 Teil 2. DER GRUND FUER EIGENE METHODE STATT
    /// `neuestes`: `neuestes(&mut self, …)` **raeumt** abgelaufene Tickets
    /// weg, und das ist am falschen Ort — der Client liest nur, ein Ticket
    /// zu entfernen gehoert in die Verwaltung. Ausserdem braucht
    /// `TlsStrom` den Speicher mit `&` (er haelt ihn nicht), und
    /// `neuestes` verlangt `&mut`. Diese Methode hat deshalb `&self`.
    ///
    /// `jetzt` sind Sekunden **seit 1970** (wie in `neuestes`), nicht die
    /// Differenz: ein Aufruf mit einer `Instant`-Restzeit ergaebe ein
    /// `verschleiertes_alter`, das um 1970 danebenliegt.
    pub fn angebot(&self, host: &str, jetzt: u64) -> Option<PskAngebot> {
        let liste = self.je_host.get(host)?;
        // Das **neueste** Ticket: das mit der groessten `empfangen`-Zeit.
        // M3: die Schleife laeuft ueber eine Liste mit `MAX_TICKETS_PRO_HOST`
        // (4) Eintraegen — die Grenze steht am Feld, nicht hier.
        let mut bestes: Option<&Ticket> = None;
        for t in liste {
            if t.abgelaufen(jetzt) {
                continue;
            }
            if bestes.is_none_or(|b| t.empfangen > b.empfangen) {
                bestes = Some(t);
            }
        }
        let t = bestes?;
        Some(PskAngebot {
            vorschlaege: vec![PskVorschlag {
                identitaet: t.ticket.clone(),
                verschleiertes_alter: t.verschleiertes_alter(jetzt),
            }],
            // added 02.10.2026, R4i: `t.psk` statt `t.ticket.clone()`.
            // Die **Identitaet** bleibt der Ticketinhalt (so schickt man
            // ein Ticket zurueck), aber **gehasht** wird ueber das
            // Geheimnis aus dem Ticketinhalt — und das ist nicht dasselbe
            // Bytefeld. Bis hier war es beides gleich, und die Runde lief
            // nur, weil beide Seiten es gleich machten.
            psk: t.psk.clone(),
        })
    }

    /// added 02.10.2026 02:9x UTC (Projektlabel 02.10.2026), Rolle TLS.
    /// Wirft die Tickets **eines** Hosts weg und meldet, ob etwas da war.
    ///
    /// DER GRUND, gemessen: `team/AUTONOM.txt` Zeile 691 und die Messung vom
    /// 02.10.2026 02:7x–02:8x in `src/lib.rs::tests::haenger_messung`.
    /// Fehlschlaege gegen `www.rust-lang.org` und `www.wikipedia.org` gab es
    /// **nur**, wenn die TLS-Wiederaufnahme benutzt wurde (am Zaehler
    /// `wiederaufnahme_versuche()` ablesbar: `wdn 1` bei jedem Fehlschlag,
    /// `wdn 0` bei jedem Erfolg; ohne Angebot 9 von 9 Laeufen gruen). Nach
    /// dem Handshake (28 ms statt 290 ms — kein Zertifikat) schickte der
    /// Server **kein** Byte mehr. Damit ist ein Ticket fuer diesen Host
    /// unbrauchbar, und der naechste Versuch muss ohne ihn laufen — sonst
    /// scheitert er genauso.
    ///
    /// added 02.10.2026 02:9x: **nicht** `leeren()`. Das leert alle Hosts und
    /// damit die Wiederaufnahme aller anderen Verbindungen im Prozess; hier
    /// geht es um **einen** Host, und der Unterschied ist der Grund, warum es
    /// eine eigene Funktion ist.
    pub fn host_verwerfen(&mut self, host: &str) -> bool {
        self.je_host.remove(host).is_some()
    }

    /// Leert den Speicher.
    pub fn leeren(&mut self) {
        self.je_host.clear();
    }
}

/// Der Nachrichtentyp, den wir in der Nachschrift erwarten.
///
/// added 30.09.2026, Welle 7 / A1. RFC 8446 Abschnitt 4.6.1 legt
/// `NewSessionTicket` auf Typ **4**. `nachricht::typen` traegt die
/// Nummer; die Konstante hier macht die Absicht sichtbar, ohne dass
/// man die Datei `nachricht.rs` aufmachen muss.
pub const TYP_NEW_SESSION_TICKET: u8 = nachricht::typen::NEW_SESSION_TICKET;

// ---------------------------------------------------------------------------
// DER BINDER (RFC 8446 4.2.11.2), added 30.09.2026
// ---------------------------------------------------------------------------

/// Der Nachrichtentyp `pre_shared_key`.
///
/// added 30.09.2026, Welle 7 / A1. RFC 8446 4.2.11.1 nennt die
/// Erweiterung `pre_shared_key (41)`.
pub const TYP_PRE_SHARED_KEY: u16 = 41;

/// M3: die Laenge des Binders in Byte.
///
/// DER WERT 32 = SHA-256. DER GRUND, DASS ER NICHT AUS DER SUITE
/// FOLGT: RFC 8446 4.2.11.2 schreibt `PskBinderEntry<32..255>`, also
/// **mindestens** 32 Byte, und die Größe hängt am Hash der ausgehandelten
/// Cipher-Suite. Wir fahren heute nur `TLS_AES_128_GCM_SHA256`; bei einer
/// Suite mit SHA-384 waeren es 48. Steht hier als Konstante, damit die
/// Stelle, an der das spaeter anders wird, auffindbar ist — und nicht
/// als `32` verstreut im Code.
pub const BINDER_LAENGE: usize = 32;

/// M3: hoechstens so viele Bytes ein PSK-Vorschlag haben darf.
///
/// DER WERT 65535 + 4 + 2 + 2 + BINDER_LAENGE, aufgerundet auf 70000.
/// Rechnung: Ticket (`opaque<1..2^16-1>`, also bis 65535) + Altersfeld
/// (4) + Laengen der Identitaetenliste (2) + Laengen der Binderliste
/// (2) + 32 Byte Binder.
///
/// Der Wert deckt damit den groessten **zulaessigen** Vorschlag ab. Ein
/// Client, der mehr schickt, verletzt die Norm; ein Client, der weniger
/// schickt, kann kein Ticket anbieten, das der Server ihm gegeben hat.
pub const MAX_PSK_VORSCHLAG: usize = 70000;

/// M3: die Obergrenze fuer die **Identitaetenliste** (16-Bit-Feld).
///
/// added 01.10.2026, R4. `PskIdentityEntry identities<7..2^16-1>`
/// (RFC 8446 4.2.11): 65535 Byte fuer die ganze Liste, also fuer
/// **alle** Eintraege zusammen. Das ist eine andere Grenze als
/// `MAX_TICKET_BYTES` (das laesst 65535 fuer ein **einzelnes** Ticket
/// zu) — und ohne diese Grenze schreibt `Schreiber::vektor16` ein
/// Laengenfeld, das **stillschweigend** rundet: ein Ticket mit 65535
/// Byte ergibt eine Liste von 65535 + 2 + 4 = 65541 Byte, `as u16` macht
/// daraus 5, und der Server laese ab Byte 5 in die Identitaeten. Der
/// Fehler ist im Betrieb still und in einem Test nur mit einem Ticket
/// an der Grenze zu finden.
pub const MAX_IDENTITAETEN_LISTE: usize = 65535;

/// M3: hoechstens so viele Identitaeten kann man anbieten.
///
/// added 01.10.2026, R4. `PskBinderEntry binders<33..2^16-1>`: je
/// Eintrag 1 Byte Laenge plus 32 Byte Binder, also 33 Byte. Die
/// Binderliste traegt dasselbe 16-Bit-Feld, also
/// `65535 / 33 = 1985` Eintraege gehen, der 1986. nicht.
pub const MAX_ANZAHL_IDENTITAETEN: usize = 1985;

/// Der Vorschlag, den der Browser einem Server macht.
///
/// added 30.09.2026, Welle 7 / A1. Genau die Felder aus RFC 8446
/// 4.2.11.1:
/// ```text
///   struct {
///       PskIdentity identities<7..2^16-1>;
///       PskBinderEntry binders<33..2^16-1>;
///   } PSKExtension;
///   struct {
///       opaque identity<1..2^16-1>;
///       uint32 obfuscated_ticket_age;
///   } PskIdentity;
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PskVorschlag {
    /// Das Ticket, unveraendert.
    pub identitaet: Vec<u8>,
    /// `obfuscated_ticket_age` = `ticket_age_add + (jetzt - ausgegeben)`
    /// (RFC 8446 4.2.11.1).
    pub verschleiertes_alter: u32,
}

/// Was in einem **empfangenen** `pre_shared_key`-Block stand.
///
/// added 02.10.2026, R4d Stufe 2. DER GEGENSTUECK ZU `PskAngebot`: der
/// Client baut ein Angebot, der Server liest eines. Beide brauchen
/// `identitaet` + `verschleiertes_alter`, aber nur der Server braucht
/// zusaetzlich den **empfangenen** Binder — und der ist per Definition
/// das, was geprueft werden soll, nicht das, was man selbst hineingelegt
/// hat. Ein Feld dafuer fehlte, also ist es hier nachgetragen.
///
/// `lebensdauer` steht **nicht** drin: der Server hat es beim Ausstellen
/// gewusst, und `Ticket` traegt es. Wer es hier fuehren wuerde, haette es
/// an zwei Stellen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PskEmpfang {
    /// Die angebotenen Identitaeten mit ihrem `obfuscated_ticket_age`.
    pub eintraege: Vec<(Vec<u8>, u32)>,
    /// Die Binder, **wie sie ankamen** — der Server hat keine Erwartung
    /// daran, er rechnet sie gleich.
    pub binder: Vec<u8>,
}

/// Liest einen empfangenen `pre_shared_key`-Block.
///
/// added 02.10.2026, R4d Stufe 2. DER WEG, und es ist derselbe wie beim
/// Bauen: **eine** Lage-Funktion fuer beide Richtungen. `psk_lage`
/// bestimmt, wo der Block beginnt und wie lang er ist; diese Funktion
/// liest die beiden Listen darin. Ein zweiter Decoder waere eine zweite
/// Layout-Wahrheit — genau die Kopie, die ich am 01.10. bei `psk_lage`
/// bekämpft habe (siehe dort).
///
/// DIE NORM (RFC 8446 4.2.11.1) in der Reihenfolge, in der sie hier
/// gelesen wird:
///   Identitaet: 16-Bit-Laenge, Ticket-Byte, 32-Bit-Alter
///   Binder:     16-Bit-Laenge, je Eintrag 1-Byte-Laenge + Binder-Byte
///
/// M3: die Schleife laeuft ueber die Laengenangabe der Liste. Der
/// Listeninhalt ist durch das 16-Bit-Feld auf 65535 Byte begrenzt und
/// jeder Eintrag braucht mindestens 6 Byte (2 Laenge, 1 Byte Ticket,
/// 4 Alter), also sind es hoechstens 10922 Runden. Der Binder-Teil wird
/// **nicht** iteriert, sondern als Block uebernommen — der Server
/// vergleicht ihn als Block, und ein Eintrag-weise Zerlegen wuerde nur
/// eine Stelle sein, an der die 33-Byte-Regel noch einmal falsch
/// ausgelegt werden kann.
pub fn psk_lese(clienthello: &[u8]) -> Result<PskEmpfang, BinderFehler> {
    let (inhalt_start, laenge) = psk_lage(clienthello)?;
    let inhalt = &clienthello[inhalt_start..inhalt_start + laenge];
    if inhalt.len() < 2 {
        return Err(BinderFehler::ZuKurz {
            war: inhalt.len(),
            braucht: 2,
        });
    }
    // --- Die Identitaetenliste -----------------------------------------
    let id_laenge = u16::from_be_bytes([inhalt[0], inhalt[1]]) as usize;
    if inhalt.len() < 2 + id_laenge + 2 {
        return Err(BinderFehler::LayoutFalsch {
            stelle: inhalt_start + 2,
            was: "Laenge der Identitaetenliste",
        });
    }
    let mut eintraege: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut pos = 2usize;
    // `pos < 2 + id_laenge`: der Bereich der Identitaetenliste.
    while pos < 2 + id_laenge {
        if pos + 6 > 2 + id_laenge {
            return Err(BinderFehler::LayoutFalsch {
                stelle: inhalt_start + pos,
                was: "Identitaeteneintrag passt nicht in die Liste",
            });
        }
        let laenge_ticket = u16::from_be_bytes([inhalt[pos], inhalt[pos + 1]]) as usize;
        if laenge_ticket == 0 {
            return Err(BinderFehler::LayoutFalsch {
                stelle: inhalt_start + pos,
                was: "opaque identity<1..2^16-1> (0 Byte sind nicht erlaubt)",
            });
        }
        let eintrag_ende = pos + 2 + laenge_ticket + 4;
        if eintrag_ende > 2 + id_laenge {
            return Err(BinderFehler::LayoutFalsch {
                stelle: inhalt_start + pos,
                was: "Ticket laenger als die Identitaetenliste",
            });
        }
        let alter = u32::from_be_bytes([
            inhalt[eintrag_ende - 4],
            inhalt[eintrag_ende - 3],
            inhalt[eintrag_ende - 2],
            inhalt[eintrag_ende - 1],
        ]);
        eintraege.push((inhalt[pos + 2..pos + 2 + laenge_ticket].to_vec(), alter));
        pos = eintrag_ende;
    }
    // M4: die Liste ist auf 65535/6 Eintraege begrenzt, aber das ist eine
    // Rechnung aus dem Feld, keine Zusage. `Vec::with_capacity(1)` und
    // `push` verdoppeln bei Bedarf — die Grenze steht am 16-Bit-Feld.
    if eintraege.is_empty() {
        return Err(BinderFehler::LayoutFalsch {
            stelle: inhalt_start + 2,
            was: "leere Identitaetenliste (identities<7..2^16-1>)",
        });
    }
    // --- Die Binderliste, als Block ------------------------------------
    let binder_start = 2 + id_laenge;
    let binder_laenge =
        u16::from_be_bytes([inhalt[binder_start], inhalt[binder_start + 1]]) as usize;
    if binder_start + 2 + binder_laenge != inhalt.len() {
        return Err(BinderFehler::LayoutFalsch {
            stelle: inhalt_start + binder_start,
            was: "Laenge der Binderliste (sie muss bis zum Ende reichen)",
        });
    }
    // DER BINDER, und hier war die erste Fassung um **ein** Byte zu lang.
    //
    // GEMESSEN am 02.10.2026, 22:0x: der Server lehnte ein **korrekt**
    // aufgebautes Angebot ab (`selected_identity: None`, obwohl
    // `konstant_gleich` zu Unrecht `false` liefert), weil `binder` die ganze
    // Binderliste war — also `[1-Byte-Laenge][32 Byte]`, **33** Byte. Der
    // Vergleich mit dem erwarteten 32-Byte-Binder scheitert an der
    // Laenge, und `konstant_gleich` sagt bei verschiedener Laenge zu
    // Recht `false`. Der Fehler war also nicht im Vergleich, sondern
    // eine Byte zu viel im Wert daneben — und ein Vergleich, der wegen
    // eines Bytes im **Namen** der Sache fehlschlaegt, sieht aus wie ein
    // Kryptographie-Fehler.
    //
    // DER WEG, und er ist RFC 8446 4.2.11.2:
    //     PskBinderEntry binders<33..2^16-1>;
    //     struct { opaque binder_entry<32..255>; } PskBinderEntry;
    // Der erste (und bei uns einzige) Eintrag hat also ein 1-Byte-Laengen-
    // feld und danach den Binder. Wir verlangen **genau** `BINDER_LAENGE`
    // (32): laenger waere ein Hash, den wir nicht implementieren, und
    // kuerzer waere nach der Grammatik unmoeglich.
    if binder_laenge < 33 {
        return Err(BinderFehler::LayoutFalsch {
            stelle: inhalt_start + binder_start,
            was: "binders<33..2^16-1> (ein Eintrag braucht 1 + 32 Byte)",
        });
    }
    const EINTRAG: usize = 1 + BINDER_LAENGE;
    let laenge_eintrag = inhalt[binder_start + 2] as usize;
    if laenge_eintrag != BINDER_LAENGE {
        return Err(BinderFehler::LayoutFalsch {
            stelle: inhalt_start + binder_start + 2,
            was: "binder_entry (bei SHA-256 genau 32 Byte)",
        });
    }
    Ok(PskEmpfang {
        eintraege,
        binder: inhalt[binder_start + 2 + 1..binder_start + 2 + EINTRAG].to_vec(),
    })
}

/// Vergleicht zwei gleich lange Bytefolgen in **konstanter Zeit**.
///
/// added 02.10.2026, R4d Stufe 2. DER GRUND, und er ist nicht
/// theoretisch: der Binder ist ein MAC, und `a == b` bricht bei der
/// ersten Abweichung ab. Die Dauer haengt damit von der Position der
/// ersten falschen Byte ab, und ein Angreifer, der den Binder erraten
/// will, braucht dadurch `n` Versuche **pro Byte** statt 256^n aller
/// Bytes. Bei 32 Byte Binder ist das der Unterschied zwischen 2^256 und
/// 2^8 Versuchen fuer die falsche Stelle.
///
/// DIE ZWEI EIGENTLICHEN ANFORDERUNGEN, und beide sind hier erfuellt:
///  * **gleiche Laenge**: unterschiedliche Laengen werden sofort als
///    verschieden gemeldet — die Laenge steht im Block und ist keine
///    Geheimnis, sie zu verbergen hiesse nur, die Fehlerart zu
///    verschleiern.
///  * **keine fruehe Rueckkehr**: die Schleife laeuft immer ueber **alle**
///    Bytes; die Differenz wird am Ende ausgewertet.
///
/// M3: die Schleife laeuft ueber `a.len()`, und beide Laengen sind vorher
/// verglichen — es gibt also keine eingabeabhaengige Endlichkeit, die man
/// nicht begruenden koennte.
pub fn konstant_gleich(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut differenz = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        differenz |= x ^ y;
    }
    differenz == 0
}

/// Was der Client einem Handshake anbietet: ein Ticket und den dazugehoerigen
/// PSK.
///
/// added 01.10.2026, R4 Teil 2. WARUM EIGENE STRUKTUR UND NICHT
/// `Option<(Vec<PskVorschlag>, Vec<u8>)>`: `TlsStrom::handshake` bekommt
/// sie als **Parameter**, damit `TlsStrom` selbst keine Lebensdauer
/// bekommt. Das Struct hat keine, und `Tickets` muesste darin fuer die
/// ganze Lebenszeit des Sockets liegen — ein `&Tickets` im `TlsStrom`
/// waere ein Lebensdauerkonflikt mit jedem Aufrufer, der die Sitzung
/// danach noch braucht (und genau der ist der Browser: die Sitzung lebt
/// ueber mehrere Verbindungen). Zwei Felder in einer Struktur statt zwei
/// Parameter neben einem `Option`, weil die Frage "gehoeren die beiden
/// zusammen?" sonst jeder Aufrufer neu beantworten muss.
///
/// DER PSK IST DAS TICKET selbst, und das ist eine bewusste
/// Vereinfachung, die im Bericht benannt ist: ein echter Server leitet
/// aus dem Ticket ueber `HKDF-Expand-Label(resumption_master_secret,
/// "resumption", ticket_nonce)` einen PSK ab. Fuer einen Handshake
/// **gegen unseren eigenen** Server ist der Ticket-Byteinhalt genauso
/// brauchbar, solange beide Seiten dieselbe Vereinfachung benutzen — und
/// beide Seiten stehen in diesem Crate. Wer das gegen einen fremden Server
/// fahren will, muss die Ableitung einbauen; der Ort dafuer ist genau
/// diese Konstruktion, und dort steht auch, dass es eine
/// Vereinfachung ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PskAngebot {
    /// Die Identitaeten fuer das `pre_shared_key`-Feld.
    pub vorschlaege: Vec<PskVorschlag>,
    /// Der PSK, aus dem der Binder gerechnet wird.
    pub psk: Vec<u8>,
}

impl PskAngebot {
    /// Wie viele Identitaeten angeboten werden.
    pub fn anzahl(&self) -> usize {
        self.vorschlaege.len()
    }
}

/// Bietet einen Wiederaufnahme-PSK in einem ClientHello an und traegt den
/// Binder ein.
///
/// added 01.10.2026, R4 Teil 2. DIE SIEBEN SCHRITTE, und ihre Reihenfolge
/// ist erzwungen:
///
/// ```text
/// 1  ps_k_erweiterung_ohne_binder(vorschlaege)  -> Block mit 32 Nullbyte
/// 2  hello.psk = Some(block)                     -> kodieren
/// 3  transkript_fuer_binder(kodiert, anzahl)     -> laesst die Nullen drin
/// 4  binder(psk, transkript)
/// 5  binder_setzen(kodiert, anzahl, 0, wert)     -> nur 32 Byte aendern
/// 6  zurueckgeben
/// ```
///
/// WARUM ALS EINE FUNKTION UND NICHT ALS SIEBEN AUFRUFE: diese Kette
/// stand in diesem Projekt schon dreimal an drei Stellen falsch (einmal
/// 2 Byte zu lang im Schnitt, einmal ohne die zweite Expansionsstufe,
/// einmal mit dem leeren statt des gehashten Kontext). Sie hat hier
/// **eine** Stelle.
///
/// WARUM SIE DIE BYTES ZURUECKGIBT: der Aufrufer braucht genau diese
/// Bytes fuer den Transkript (RFC 8446 4.4.1 — in den Transkript geht
/// die *gesendete* Nachricht). Ein `Result<(), _>` mit einem zweiten
/// `hello.kodiere()` im Aufrufer wuerde eine **andere** Nachricht
/// hashen, sobald `kodiere` sich aendert, und der Fehler faelle erst in
/// der Schluesselableitung auf.
///
/// DER FEHLER WIRD ZURUECKGEGEBEN, nicht verschluckt: `psk == None`
/// heisst "es gibt kein Ticket" und ist **kein** Fehler — das ist der
/// Weg, den ein Client ohne Sitzung geht (kein `pre_shared_key` im
/// ClientHello, ein vollstaendiger Handshake). `psk == Some(..)` mit
/// einem leeren Slice **ist** ein Fehler (`BinderFehler::Leer`), weil ein
/// PSK der Laenge 0 nach RFC 8446 4.2.11.2 gar nicht angeboten werden
/// darf (`opaque identity<1..2^16-1>` heisst: mindestens 1 Byte).
///
/// DER FEHLERTYP, und die Begruendung fuer `BinderFehler` statt
/// `NachrichtFehler`: `nachricht::Ergebnis` ist ein **privater** Typ
/// (`type Ergebnis<T>`, kein `pub`), und `ClientHello::kodiere` ist
/// fehlerfrei. Die drei Fehlerquellen dieser Funktion sind alle
/// Binder-Fehler (`ps_k_erweiterung_ohne_binder`,
/// `transkript_fuer_binder`, `binder_setzen`), also gehoeren sie in
/// dieselbe Aufzaehlung; eine zweite Fehlerart nur fuer "du hast mir
/// null Vorschlaege gegeben" waere eine Aufzaehlung mit zwei Namen
/// fuer dasselbe Problem.
pub fn psk_anbieten(
    clienthello: &mut crate::nachricht::ClientHello,
    vorschlaege: &[PskVorschlag],
    psk: &[u8],
) -> Result<Vec<u8>, BinderFehler> {
    // M3: `vorschlaege.len()` kommt vom Aufrufer; die Grenze steht in
    // `MAX_ANZAHL_IDENTITAETEN` und wird in `ps_k_erweiterung_ohne_binder`
    // geprueft. Die Zaehlung selbst hat hier keine Grenze, weil sie an
    // einem Slice haengt, dessen Laenge die Maschine begrenzt.
    let anzahl = vorschlaege.len();
    if anzahl == 0 {
        return Err(BinderFehler::AnzahlNull);
    }
    if psk.is_empty() {
        // DER KURZE WEG, und er ist nicht nur bequem: ein leerer PSK
        // waere eine Identitaet der Laenge 0, die die Norm ausschliesst.
        return Err(BinderFehler::Leer);
    }
    clienthello.psk = Some(ps_k_erweiterung_ohne_binder(vorschlaege)?);
    let mut kodiert = clienthello.kodiere();
    let transkript = transkript_fuer_binder(&kodiert, anzahl)?;
    let wert = binder(psk, &transkript)?;
    binder_setzen(&mut kodiert, anzahl, 0, &wert)?;
    Ok(kodiert)
}

/// Der Fehler der Binder-Berechnung.
///
/// added 30.09.2026. DER GRUND FUER EIGENE NAMEN statt `VektorFehler`:
/// der Binder scheitert nicht am Parsen, sondern an der Kryptographie
/// (HKDF) und an einer Grenze. Beides braucht eigene Namen, sonst
/// meldet ein Fehler „unvollstaendig", obwohl die Eingabe vollstaendig
/// war.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinderFehler {
    /// Der Index liegt ausserhalb der angebotenen Identitaeten.
    ///
    /// added 01.10.2026, R4. EIGENE VARIANTE, weil „Layout stimmt nicht"
    /// und „Index zu gross" zwei verschiedene Fehler sind: der erste
    /// kommt von fremden Bytes, der zweite vom Aufrufer.
    FalscherIndex {
        /// Der Index, der verlangt wurde.
        war: usize,
        /// Wie viele Identitaeten es gibt.
        anzahl: usize,
    },
    /// Der Puffer ist kuerzer, als die Binderliste darin Platz braucht.
    ZuKurz {
        /// So lang ist der Puffer.
        war: usize,
        /// So lang muesste er sein.
        braucht: usize,
    },
    /// An der erwarteten Stelle steht nicht das erwartete Feld.
    ///
    /// added 01.10.2026, R4. DER WEG, und er ist der wichtige Teil:
    /// `binder_setzen` rechnet die Stelle **von hinten** (die
    /// `pre_shared_key`-Erweiterung ist die letzte, 4.2.11). Stimmt
    /// die Rechnung nicht — etwa weil der Aufrufer einen Puffer ohne
    /// diese Erweiterung gibt —, wuerde ein 32-Byte-Wert an einer
    /// **falschen** Stelle landen. Deshalb wird jede der drei
    /// Strukturmarken geprueft, und bei Abweichung **nichts**
    /// geschrieben.
    LayoutFalsch {
        /// Wo die Marke nicht stand.
        stelle: usize,
        /// Was dort erwartet wurde.
        was: &'static str,
    },
    /// Der Vorschlag ist laenger als `MAX_PSK_VORSCHLAG`.
    ZuLang {
        /// Gemessen.
        war: usize,
        /// Erlaubt.
        max: usize,
    },
    /// Der Vorschlag ist leer (kein Ticket).
    Leer,
    /// Die Laengenberechnung schlug fehl (HKDF).
    Kryptographie,
    /// Der Aufrufer hat **null** Vorschlaege gegeben.
    ///
    /// added 01.10.2026, R4 Teil 2 (`psk_anbieten`). EIGENER NAME und
    /// nicht `Leer`: `Leer` heisst "kein Ticket im Vorschlag" (bzw. ein
    /// PSK-Slice der Laenge 0), und `ps_k_erweiterung_ohne_binder(&[])`
    /// trifft es ebenfalls. Zwei Namen, weil zwei verschiedene Eingaben
    /// beteiligt sind: ein leerer **PSK** und eine leere **Liste**.
    AnzahlNull,
}

impl std::fmt::Display for BinderFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinderFehler::ZuLang { war, max } => {
                write!(f, "{war} Byte, erlaubt sind {max}")
            }
            BinderFehler::Leer => write!(f, "kein Ticket im Vorschlag"),
            BinderFehler::Kryptographie => write!(f, "die Binder-Ableitung schlug fehl"),
            BinderFehler::AnzahlNull => {
                write!(f, "kein PSK-Vorschlag angegeben (die Liste war leer)")
            }
            BinderFehler::FalscherIndex { war, anzahl } => {
                write!(f, "Index {war} bei {anzahl} Identitaeten")
            }
            BinderFehler::ZuKurz { war, braucht } => {
                write!(f, "der Puffer hat {war} Byte, gebraucht werden {braucht}")
            }
            BinderFehler::LayoutFalsch { stelle, was } => {
                write!(f, "an Byte {stelle} steht nicht das erwartete Feld ({was})")
            }
        }
    }
}

/// Berechnet den `binder` fuer einen Vorschlag.
///
/// added 30.09.2026, Welle 7 / A1; **korrigiert 01.10.2026, R4** (der
/// Schluessel war der PSK statt des Early Secret, das Label war
/// `finished` statt `res binder` — die Belege mit Zeilennummer stehen in
/// `binder` selbst). Der Weg nach RFC 8446 4.2.11.2:
///
/// ```text
///   binder_key = HKDF-Expand-Label(PSK, "finished", "", Hash.length)
///   binder     = HMAC(binder_key, Hash(Transkript))
/// ```
///
/// UND, **GLEICH MITTEN IM WEG**, der Teil, der in drei
/// Implementierungen falsch gemacht wird (der Satz steht woertlich so
/// im RFC, 4.2.11.2): "The length fields for the message (including the
/// overall length, the length of the extensions block, and the length of
/// the 'pre_shared_key' extension) are all set as if binders of the
/// correct lengths were present." Das heisst: der Transkript enthaelt den
/// ClientHello in **voller Laenge**, aber mit **genullten** Binder-Bytes.
/// Ein Transkript ueber einen ClientHello, der mit Nullen *gesendet*
/// wurde, waere ein anderer Hash — und der Server wuerde ablehnen.
///
/// DER AUFRUFER liefert das Transkript; hier wird nur der Wert
/// gerechnet. Das ist eine bewusste Trennung: wer das Transkript
/// baut, muss das Zwoimal-regel-Verfahren anwenden (Abschnitt 2), und
/// das gehoert in den ClientHello-Bau, nicht in die Krypto-Funktion.
pub fn binder(psk: &[u8], transkript: &[u8]) -> Result<[u8; BINDER_LAENGE], BinderFehler> {
    // DER WEG, und er ist in drei Schritten (RFC 8446 4.2.11.2 mit 7.1):
    //
    //   1. Early Secret = HKDF-Extract(salt = 0^Hash.length, ikm = PSK)
    //   2. binder_key  = Derive-Secret(Early Secret, "res binder", "")
    //   3. binder      = HMAC(binder_key, Hash(Truncate(ClientHello)))
    //
    // added 01.10.2026, R4. DIE ZWEI FEHLER, DIE DIESE FUNKTION HATTE,
    // und beide sind erst beim **Verdrahten** aufgefallen, nicht beim
    // Schreiben: als Schluessel stand der **PSK selbst** statt des Early
    // Secret, und das Label war `"finished"` statt `"res binder"`.
    //
    // Belege aus `/home/jaff/mutation_md/tls/rfc/rfc8446.txt`:
    // * Zeile 3239-3242 (4.2.11.2): "The PskBinderEntry is computed in
    //   the same way as the Finished message (Section 4.4.4) but with
    //   the BaseKey being the binder_key derived via the key schedule
    //   from the corresponding PSK which is being offered (see
    //   Section 7.1)." — die Formel des `finished`, aber ein anderer
    //   **Schluessel**.
    // * Zeile 5161-5165 (7.1): "PSK -> HKDF-Extract = Early Secret" und
    //   darunter `Derive-Secret(., "ext binder" | "res binder", "")`
    //   `= binder_key`.
    // * Zeile 5228-5231: "the label is 'ext binder' for external PSKs
    //   ... and 'res binder' for resumption PSKs". Zwei Labels, damit
    //   ein externer PSK nicht als Wiederaufnahme ausgegeben werden kann.
    //
    // WARUM ES UNENTDECKT BLEIBEN KONNTE, und das ist der Teil, den ich
    // notiere: der Test in diesem Modul verglich die Crate-Rechnung mit
    // einer **zweiten Rechnung im selben Testmodul** — und der Fehler
    // stand in beiden. Zwei gleiche Rechnungen sind kein Test. Ab jetzt
    // steht der Beleg in der Norm, nicht in der zweiten Kopie.
    //
    // `extract_sha256(0^32, PSK)`: der Salt ist **Null**, nicht "leer" —
    // RFC 8446 7.1 schreibt `HKDF-Extract(0, PSK)`, und HKDF-Extract
    // deutet eine Nulllaenge als Nullen-Salt, sodass beides dasselbe
    // ergibt. Der Unterschied ist in der Doku festgehalten, weil
    // `hkdf::extract_sha256` an dieser Stelle ein echter 32-Byte-Salt
    // will.
    // STUFE 1 + 2 ueber die oeffentliche Funktion: der Server braucht
    // genau dieselbe Rechnung zum **Pruefen** eines Binders, und zwei
    // Kopien waeren zwei Orte, an denen die Formel falsch sein kann.
    let binder_key = binder_key_aus_psk(psk)?;
    //
    // STUFE 3, und die hat die erste Fassung **nicht** (30.09.2026 ->
    // 01.10.2026, R4): finished_key = HKDF-Expand-Label(binder_key,
    // "finished", "", Hash.length). Beleg aus RFC 8448 Abschnitt 4, Block
    // `{client} calculate PSK binder`:
    //
    //   PRK (32 octets):    69 fe 13 1a …    <- der binder_key
    //   info (18 octets):   00 20 0e "tls13 finished" 00
    //   expanded (32):      55 88 67 3e …    <- der finished_key
    //   finished (32):      3a dd 4f b2 …    <- der Binder
    //
    // `info` nennt 0x0020 = **32** als Ausgabelaenge und
    // `"tls13 finished"` als Label — das ist ein `HKDF-Expand-Label`
    // ueber den `PRK`, also ueber den binder_key. Ohne diese Stufe war
    // der HMAC-Schluessel etwas, das weder `finished_key` noch ein
    // anderer Normwert ist.
    let finished_key = finished_key_aus_binder_key(&binder_key)?;
    // DER HASH-Schritt, und der ist nicht optional: 4.2.11.2 sagt "an HMAC
    // over a transcript hash". Die erste Fassung hat den Transkript
    // direkt in den HMAC gegeben. GEMESSEN: die Fassung ohne Hash ergab
    // `e319 49 5b be fd 60 45 …`, die mit Hash `3992 75a 3b 9f 9d bb …`.
    // Zusaetzlich geaendert: der Transkript, der hier ankommt, ist
    // **der abgeschnittene ClientHello** (bis einschliesslich der
    // Identitaeten plus der Binder-Listenlaenge, 4.2.11.2) — nicht der
    // volle. Das Abschneiden macht der Aufrufer, weil er die Erweiterung
    // kennt; hier steht nur die Formel.
    let transkript_hash = crate::sha2::sha256(transkript);
    Ok(crate::hmac::HmacSha256::berechne(
        &finished_key,
        &transkript_hash,
    ))
}

/// Der `binder_key` aus einem Wiederaufnahme-PSK.
///
/// added 01.10.2026, R4. `binder_key = Derive-Secret(Early Secret,
/// "res binder", "")` und `Early Secret = HKDF-Extract(0, PSK)`
/// (RFC 8446 7.1, Zeilen 5161-5165).
///
/// **OEFFENTLICH**, weil der Server genau diese Rechnung braucht: er
/// muss einen angebotenen Binder **pruefen**, und das geht nur ueber
/// denselben Schluessel. Wer die Kette nur im Client gehaelt haette,
/// koennte einen eingehenden Binder nicht pruefen — und ein Server, der
/// das nicht prueft, nimmt jede Wiederaufnahme an.
pub fn binder_key_aus_psk(psk: &[u8]) -> Result<[u8; BINDER_LAENGE], BinderFehler> {
    if psk.is_empty() {
        return Err(BinderFehler::Leer);
    }
    // M3: der Salt ist eine Konstante (`0` in HKDF-Extract(0, PSK)), und
    // das PSK kommt vom Server und ist bis 65535 Byte lang. Die Grenze
    // des PSK ist die Aufgabe des Aufrufers.
    let null_salt = [0u8; 32];
    let early_secret = crate::hkdf::extract_sha256(&null_salt, psk);
    // DER KONTEXT, und er ist **nicht** leer: `Derive-Secret(Secret,
    // Label, Messages)` bedeutet
    //   HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), ...)
    // und `Messages` ist hier der leere Transkript. Also ist der Kontext
    // **SHA-256 der leeren Nachricht** = e3b0c442… (32 Byte), nicht die
    // leere Zeichenkette.
    //
    // added 01.10.2026, R4. DER FEHLER, und er war derselbe wie in
    // `record.rs::handshakephase` (dort steht die ausfuehrliche
    // Begruendung): ein leerer Kontext ergibt andere Zahlen, und weil
    // `HKDF-Expand-Label` die Kontextlaenge mit in das `info`-Feld
    // schreibt, aendert sie das Ergebnis vollstaendig. Gegen den Trace
    // aus RFC 8448 geprueft: mit leerem Kontext kam
    // `3da937f7…` statt `69fe131a…`.
    let leerer_transkript = crate::sha2::sha256(&[]);
    // `expand_label_sha256` liefert `Vec<u8>`; hier wird ein `[u8; 32]`
    // gebraucht, weil `binder` mit `[u8; BINDER_LAENGE]` rechnet. Die
    // Umwandlung ist ein `try_into` und damit **fehlbar** — was hier
    // stimmt, weil die Laenge aufgefordert wurde, aber ein `unwrap()`
    // waere an dieser Stelle eine Behauptung.
    crate::hkdf::expand_label_sha256(
        &early_secret,
        LABEL_RES_BINDER,
        &leerer_transkript,
        BINDER_LAENGE,
    )
    .map_err(|_| BinderFehler::Kryptographie)?
    .try_into()
    .map_err(|_| BinderFehler::Kryptographie)
}

/// Der `finished_key` aus dem `binder_key`.
///
/// added 01.10.2026, R4. `HKDF-Expand-Label(binder_key, "finished", "",
/// Hash.length)` — die zweite Stufe, die in der ersten Fassung fehlte.
/// Beleg: RFC 8448 Abschnitt 4, Trace `{client} calculate PSK binder`,
/// `info (18 octets): 00 20 0e "tls13 finished" 00`.
///
/// DER GRUND FUER EINE EIGENE OEFFENTLICHE FUNKTION: die beiden Stufen
/// sind getrennt pruefbar, und die `info`-Zeile des Traces belegt genau
/// diese zweite. Haette man beide in einem Aufruf versteckt, koennte
/// man gegen den Trace nur das **Endergebnis** stellen — und genau das
/// Endergebnis war vorher falsch, ohne zu zeigen, welche Stufe schuld war.
pub fn finished_key_aus_binder_key(
    binder_key: &[u8; BINDER_LAENGE],
) -> Result<[u8; BINDER_LAENGE], BinderFehler> {
    crate::hkdf::expand_label_sha256(binder_key, b"finished", b"", BINDER_LAENGE)
        .map_err(|_| BinderFehler::Kryptographie)?
        .try_into()
        .map_err(|_| BinderFehler::Kryptographie)
}

/// Das Label des `binder_key` fuer einen **Wiederaufnahme**-PSK.
///
/// added 01.10.2026, R4. RFC 8446 7.1 (Zeile 5228-5231) nennt zwei
/// Labels, damit ein externer PSK nicht als Wiederaufnahme ausgegeben
/// werden kann: `"res binder"` fuer PSKs aus dem
/// `resumption_master_secret` eines frueheren Handshakes, `"ext binder"`
/// fuer ausserhalb von TLS bereitgestellte. Diese Crate macht nur
/// Wiederaufnahme, also ist es hier immer `"res binder"` — als
/// Konstante, damit ein Tippfehler auffaellt.
pub const LABEL_RES_BINDER: &[u8] = b"res binder";

/// Schreibt die `pre_shared_key`-Erweiterung **ohne** Binder und liefert
/// die Laenge, die sie am Ende haben wird.
///
/// added 30.09.2026, Welle 7 / A1. DER ZWEITE TEIL DER
/// ZWEIMAL-REGEL: RFC 8446 4.2.11.2 will, dass **alle** Laengenfelder
/// schon beim Hashen die **endgueltige** Groesse haben. Also wird die
/// Erweiterung hier mit `BINDER_LAENGE` Nullbytes gebaut — das ist der
/// Prefix — und `Laenge` sagt dem Aufrufer, wie viele Byte er danach
/// noch einfuegen muss (die echten Binder).
///
/// DER WEG, und warum `Schreiber`: die Laengen muessen **tatsaechlich**
/// stimmen, nicht geraten. Ein falsches Laengenfeld ergibt einen
/// anderen Hash, und der Server meldet „binder mismatch" ohne Hinweis,
/// woran es lag.
///
/// M3: die Eingangsgrenze wird **vor** dem Bau geprueft, weil die
/// Ticketlaenge vom Server kommt.
pub fn ps_k_erweiterung_ohne_binder(vorschlaege: &[PskVorschlag]) -> Result<Vec<u8>, BinderFehler> {
    if vorschlaege.is_empty() {
        return Err(BinderFehler::Leer);
    }
    // M4: die Groesse ist bekannt, bevor alloziert wird.
    // M4: die Identitaetenliste geht in einen **eigenen** Schreiber,
    // weil ihre Laenge vor dem Inhalt stehen muss und man die Laenge
    // nicht kennt, bevor man den Inhalt kennt.
    let mut inhalt = crate::vektor::Schreiber::neu();
    for v in vorschlaege {
        inhalt.vektor16(&v.identitaet);
        inhalt.bytes(&v.verschleiertes_alter.to_be_bytes());
    }
    // DIE GRENZEN, und sie stehen **vor** dem Bau des Blocks.
    //
    // DER FUND DIESER ZEILE (30.09.2026, Test
    // `die_psk_erweiterung_verweigert_zu_grosse_und_leere_vorschlaege`): die
    // erste Fassung pruefte nur `MAX_PSK_VORSCHLAG` = 70000 am ** fertigen
    // Block. Ein Ticket mit 65536 Byte kam hindurch, weil 65536 + 33 noch
    // unter 70000 liegt. Der Wert 70000 ist aber fuer den **Vorschlag**
    // gedacht; das einzelne Ticket hat seine eigene, strengere Normgrenze
    // `opaque<1..2^16-1>`. Zwei Grenzen fuer zwei Dinge — und beide
    // muessen an **beiden** Enden stehen, sonst faengt die eine die
    // nicht-ab.
    //
    // added 01.10.2026, R4: **zwei** Grenzen, und die zweite fehlte. Der
    // Eintrag ist
    //   struct { opaque identity<1..2^16-1>;
    //             uint32 obfuscated_ticket_age; }     (4.2.11.1)
    // und die **Liste** davor traegt ebenfalls ein 16-Bit-Laengenfeld
    // (`PskIdentityEntry identities<7..2^16-1>`). Ein Ticket mit
    // 65535 Byte ergibt eine Liste von 65535 + 2 + 4 = 65541 Byte — 6
    // zu viel. `Schreiber::vektor16` rundet das still auf 5, und die
    // Nachricht gaenge mit einer Laengenangabe raus, die nicht zum
    // Inhalt passt.
    const PRO_ENTRY: usize = 2 + 4; // Ticket-Laenge + obfuscated_ticket_age
    let mut listen_laenge = 0usize;
    for v in vorschlaege {
        if v.identitaet.is_empty() {
            return Err(BinderFehler::Leer);
        }
        if v.identitaet.len() > MAX_TICKET_BYTES {
            return Err(BinderFehler::ZuLang {
                war: v.identitaet.len(),
                max: MAX_TICKET_BYTES,
            });
        }
        listen_laenge += PRO_ENTRY + v.identitaet.len();
    }
    if listen_laenge > MAX_IDENTITAETEN_LISTE {
        return Err(BinderFehler::ZuLang {
            war: listen_laenge,
            max: MAX_IDENTITAETEN_LISTE,
        });
    }
    // Und die Binderliste: 33 Byte je Eintrag, dasselbe 16-Bit-Feld.
    if vorschlaege.len() > MAX_ANZAHL_IDENTITAETEN {
        return Err(BinderFehler::ZuLang {
            war: vorschlaege.len(),
            max: MAX_ANZAHL_IDENTITAETEN,
        });
    }
    let binder_teil = {
        let mut b = crate::vektor::Schreiber::neu();
        for _ in vorschlaege {
            b.u8(BINDER_LAENGE as u8);
            b.bytes(&[0u8; BINDER_LAENGE]);
        }
        b.fertig().to_vec()
    };
    // DIE GESAMTLAENGE, und sie ist jetzt die Summe der **drei**
    // Vektoren plus ihrer Laengenfelder: 2 (identities) + Inhalt +
    // 2 (binders) + Inhalt. Vorher fehlte die 2 fuer die
    // Identitaetenliste, und die Grenze war damit um genau diese zwei
    // Byte zu knapp angesetzt.
    // DER RUECKGABEWERT IST DER **FERTIGE** ERWEITERUNGSBLOCK, also
    // Typ, Laenge und Inhalt — nicht nur der Inhalt.
    //
    // DER FEHLER, DEN DER TEST GEFUNDEN HAT: die erste Fassung gab nur den
    // Rumpf zurueck, und der Test las an Byte 0 die Laenge der
    // Identitaetenliste (10) und erwartete 41. Der Aufrufer haette die
    // Typ- und Laengenbytes von Hand schreiben muessen — und genau das
    // ist an zwei Stellen (ClientHello und ein Test) schon schiefgegangen.
    // DER WEG: ein Baustein, der den ganzen Typ enthaelt, wird nicht
    // zweimal gebaut und nicht zweimal falsch gebaut.
    //
    // DER FEHLER, DEN DER TEST GEFUNDEN HAT (zweiter derselben Art):
    // `raus.u16(typ)` und dann `raus.vektor16(&inhalt)` schreiben **zwei**
    // Laengenfelder — eines fuer die Erweiterung und (innerhalb von
    // `vektor16`) eines fuer die Identitaetenliste. Der Test las an
    // Byte 2 die **10** (Laenge der Identitaetenliste) und verglich sie
    // mit der Erweiterungslaenge 43.
    //
    // DER RICHTIGE AUFBAU: Typ(2) || Erweiterungslaenge(2) || Rumpf,
    // und der Rumpf enthaelt **beide** Vektoren.
    // DER DRITTE VERSATZ DERSELBAU ART, und der Test hat ihn gefunden:
    // `inhalt` enthaelt die Identitaeten **ohne** ihre Laengenangabe —
    // jedes Element hat zwar seine eigene, aber die Liste als Vektor
    // braucht zusaetzlich eine. Im Rueckgabewert stand deshalb an Byte 4
    // die Ticket-Laenge (4) statt der Listen-Laenge (10).
    //
    // DER AUFBAU, und er ist jetzt in einer Zeile nachzulesen:
    //   Erweiterung = Typ || Laenge || identities-Vektor || binders-Vektor
    let mut id_vektor = crate::vektor::Schreiber::neu();
    id_vektor.vektor16(inhalt.fertig());
    let mut binder_vektor = crate::vektor::Schreiber::neu();
    binder_vektor.vektor16(&binder_teil); // `&` noetig: das ist ein Vec
    let mut rumpf = crate::vektor::Schreiber::neu();
    rumpf.bytes(id_vektor.fertig());
    rumpf.bytes(binder_vektor.fertig());
    let rumpf_bytes = rumpf.fertig().to_vec();
    //
    // added 01.10.2026, R4 Teil 2. DIE GRENZE AN DAS, WAS GESCHRIEBEN
    // WIRD, und das war vorher **zwei Byte zu knapp**.
    //
    // WAS VORHER DASTAND:
    //     let gesamt = 2 + (2 + inhalt.fertig().len()) + (2 + binder_teil.len());
    //     if gesamt > MAX_TICKET_BYTES { … }
    // Das ist `len + 45`. Geschrieben wird aber `rumpf_bytes`, und das ist
    // `(2 + inhalt) ++ (2 + binder)` = `len + 43`: die beiden
    // Laengenangaben der Teillisten stehen **jeweils direkt vor** ihrem
    // Inhalt, nicht beide am Anfang. Die Formel zaehlte also eine
    // Laengenangabe zusaetzlich und wies zwei Byte zurueck, die die Norm
    // erlaubt.
    //
    // GEMESSEN, und die Zahl stimmt nur so: bei einer Identitaet von 65490
    // Byte steht im Feld **65533** = 65490 + 43. Die alte Formel haette
    // 65535 geprueft (65490 + 45) und damit zufallig durchgelassen — die
    // Zwei fallen bei dieser Laenge nur nicht auf. Ein Byte weiter (65493)
    // waere es dann gescheitert, obwohl `65535` noch hineinpasst.
    //
    // DER WEG, und er ist der eigentliche Grund fuer diese Aenderung: die
    // Grenze misst jetzt **dieselben Bytes**, die in das 16-Bit-Feld
    // geschrieben werden. Zwei Zahlen fuer dieselbe Sache koennen nicht
    // auseinanderlaufen — vorher konnten sie es, und der Test
    // `eine_identitaet_die_das_laengenfeld_sprengt_wird_abgewiesen`
    // (01.10.2026) fand den Unterschied.
    if rumpf_bytes.len() > MAX_TICKET_BYTES {
        return Err(BinderFehler::ZuLang {
            war: rumpf_bytes.len(),
            max: MAX_TICKET_BYTES,
        });
    }
    // UND die zweite Grenze darueber, unveraendert: `MAX_PSK_VORSCHLAG`
    // (70000) begrenzt den Vorschlag. Sie steht hier **zusaetzlich**, weil
    // sie jetzt nicht mehr die einzige ist — vorher war sie das, und genau
    // deshalb konnte ein Inhalt entstehen, der nicht in sein Feld passte.
    if rumpf_bytes.len() > MAX_PSK_VORSCHLAG {
        return Err(BinderFehler::ZuLang {
            war: rumpf_bytes.len(),
            max: MAX_PSK_VORSCHLAG,
        });
    }
    let mut raus = crate::vektor::Schreiber::neu();
    raus.u16(TYP_PRE_SHARED_KEY);
    raus.vektor16(&rumpf_bytes);
    Ok(raus.fertig().to_vec())
}

/// Die Lage der `pre_shared_key`-Erweiterung in einer Nachricht.
///
/// added 01.10.2026, R4. DER WEG, und es ist ein **Vorwaerts**weg, weil
/// ein Rueckwaertsweg nicht funktioniert: von hinten sieht man
/// ```text
///   ... || Identitaeten || binders_len(2) || Eintraege
/// ```
/// und zwischen `binders_len` und der Erweiterungsnummer stehen die
/// Laenge der Identitaetenliste **und** die Identitaeten — deren Laenge
/// kennt man am Ende nicht. Vorwaerts ist es einfach: `00 29` ist die
/// Nummer der **letzten** Erweiterung (RFC 8446 4.2.11), und die
/// Laengenangabe der Erweiterung muss genau bis zum Ende der Nachricht
/// reichen. Damit sind Nummer und Lage in einem Schritt geprueft.
///
/// DIE GEGENPROBE, und sie ist der Grund fuer die Laengenpruefung: eine
/// Zufallsfolge `00 29` irgendwo im Zufallsschluessel oder in einem
/// anderen Laengenfeld wuerde ohne sie als Treffer gelten, und der
/// Binder kaeme an eine falsche Stelle.
/// Prueft, ob an `stelle` eine **vollstaendige** `pre_shared_key`-
/// Erweiterung beginnt.
///
/// added 01.10.2026, R4 Teil 2. DER GRUND, warum es diese Funktion gibt:
/// `psk_lage` suchte die Typbytes `00 29` **irgendwo** in der Nachricht und
/// nahm die letzte Fundstelle. In R4 Teil 1 war das unbemerkt moeglich,
/// weil das Ticket dort 200 mal `0x33` war und der Binder noch auf Null
/// stand. Mit echten Ticketbytes und einem echten (32 Byte zufaelligen)
/// Binder ist ein `00 29` **darin normal** — und dann zeigte der Zeiger in
/// die Mitte des Tickets.
///
/// DAS IST KEIN KOSMETISCHER FEHLER: `psk_lage` bestimmt, wo der Binder
/// steht und wo der Transkript abgeschnitten wird. Ein Zeiger, der um
/// einen beliebigen Betrag danebenliegt, laesst den Server den Binder an
/// der falschen Stelle lesen — und ein Angreifer, der das Ticket waehlt,
/// waehlt es genau so, dass `00 29` an der erwarteten Stelle steht.
///
/// DIE FUENF PRUEFUNGEN, alle aus RFC 8446 4.2.11.1.
///
/// (1) Typbytes `00 29` — geprueft an der Aufrufstelle.
///
/// (2) `4 + Laenge` reicht **genau** bis zum Nachrichtenende, weil die
/// Erweiterung die letzte ist (4.2.11 "MUST be the last extension").
///
/// (3) Die Identitaetenliste traegt mindestens 7 Byte: ein Eintrag braucht
/// `2 + 1 + 4` (Laengenfeld, ein Ticketbyte, Altersfeld), und die
/// Grammatik sagt `identities<7..2^16-1>`.
///
/// (4) Die Identitaetenliste laesst sich **exakt** abgearbeiten: jeder
/// Eintrag ist `2 + Laenge + 4` Byte, und am Ende muss die Byte-Zahl der
/// Laengenangabe entsprechen — nicht kleiner, nicht groesser.
///
/// (5) Direkt danach steht die Binderliste, und sie reicht **exakt** bis zum
/// Ende; sie besteht aus lauter 33-Byte-Eintraegen (1 Laengenbyte + 32
/// Binderbyte bei SHA-256), denn `PskBinderEntry binders<33..2^16-1>` und
/// jedes `binder_entry` ist 32 Byte lang.
///
/// OHNE MARKDOWN-LISTE, und der Grund ist clippy: `1.` am Zeilenanfang
/// macht eine Markdown-Liste, und fuer die verlangt
/// `doc_list_item_without_indent` eine Einrueckung, die ich dreimal anders
/// getroffen habe (zwei Leerzeichen mit vier eingerueckten Folgezeilen,
/// drei Leerzeichen mit drei, drei mit vier). GEMESSEN am 01.10.2026,
/// 23:4x-23:5x UTC: drei Laeufe, drei Fehlermeldungen. Als Prosa mit
/// `(1)`-Marken gibt es die Liste weiterhin, aber ohne die Regel.
///
/// M3: keine Schleife ohne Grenze. Die Identitaetenschleife laeuft ueber
/// die Laengenangabe der Liste, und **jede** Iteration verbraucht
/// mindestens 7 Byte, also sind es hoechstens `65535 / 7` = 9365 Runden —
/// und die Liste selbst ist durch das 16-Bit-Feld begrenzt. Die
/// Binderpruefung **rchnet** statt zu iterieren (`laenge % 33 == 0`).
fn psk_stelle_pruefen(nachricht: &[u8], stelle: usize) -> Option<(usize, usize)> {
    // (2) Der Kopf der Erweiterung muss vollstaendig sein.
    if stelle + 4 > nachricht.len() {
        return None;
    }
    let laenge = u16::from_be_bytes([nachricht[stelle + 2], nachricht[stelle + 3]]) as usize;
    let inhalt_start = stelle + 4;
    // (2) Sie muss bis zum Ende reichen — **und** die Nachricht darf nicht
    // laenger sein, sonst waeren die letzten Bytes eine zweite Erweiterung
    // und `pre_shared_key` waere nicht die letzte.
    if inhalt_start.checked_add(laenge)? != nachricht.len() {
        return None;
    }
    // (3) Das Laengenfeld der Identitaetenliste.
    if inhalt_start + 2 > nachricht.len() {
        return None;
    }
    let id_laenge =
        u16::from_be_bytes([nachricht[inhalt_start], nachricht[inhalt_start + 1]]) as usize;
    // `identities<7..2^16-1>`: mindestens ein Eintrag.
    if id_laenge < 7 {
        return None;
    }
    let id_ende = inhalt_start.checked_add(2)?.checked_add(id_laenge)?;
    // Nach der Identitaetenliste muss die Binderliste kommen; die braucht
    // mindestens 2 Byte fuer ihre Laenge plus 33 fuer einen Eintrag.
    if id_ende + 2 + 33 > nachricht.len() {
        return None;
    }
    // (4) Die Eintraege **exakt** abgearbeitet: `2 + Laenge + 4` je
    // Eintrag, und am Ende muss die Byte-Zahl stimmen.
    let mut pos = inhalt_start + 2;
    let ende = id_ende;
    let mut anzahl = 0usize;
    while pos < ende {
        // Mindestens 6 Byte pro Eintrag (Laengenfeld + Alter), sonst
        // kann keiner mehr hineinpassen — damit kann die Schleife nicht
        // laufen.
        if pos + 6 > ende {
            return None;
        }
        let laenge_ticket = u16::from_be_bytes([nachricht[pos], nachricht[pos + 1]]) as usize;
        // `opaque identity<1..2^16-1>`: mindestens 1 Byte.
        if laenge_ticket == 0 {
            return None;
        }
        let eintrag_ende = pos
            .checked_add(2)?
            .checked_add(laenge_ticket)?
            .checked_add(4)?;
        if eintrag_ende > ende {
            return None;
        }
        pos = eintrag_ende;
        anzahl += 1;
    }
    // `pos == ende` muss jetzt gelten; ein leeres `anzahl` waere eine
    // Liste mit 0 Eintraegen, die es nicht gibt (Mindestlaenge 7).
    if anzahl == 0 {
        return None;
    }
    // (5) Die Binderliste, direkt danach.
    let binder_ende = nachricht.len();
    let binder_laenge = u16::from_be_bytes([nachricht[id_ende], nachricht[id_ende + 1]]) as usize;
    // `binders<33..2^16-1>`: mindestens ein 33-Byte-Eintrag.
    if binder_laenge < 33 {
        return None;
    }
    // Jeder Eintrag ist `1 + 32` Byte (der Binder ist bei SHA-256 immer 32
    // Byte lang), also muss die Laenge ein Vielfaches von 33 sein. Ohne
    // diese Rechnung wuerde eine gefaelschte Laenge durchgehen.
    const BINDER_EINTRAG: usize = 1 + BINDER_LAENGE;
    if binder_laenge % BINDER_EINTRAG != 0 {
        return None;
    }
    if id_ende + 2 + binder_laenge != binder_ende {
        return None;
    }
    Some((inhalt_start, laenge))
}

fn psk_lage(nachricht: &[u8]) -> Result<(usize, usize), BinderFehler> {
    let nummer = crate::nachricht::ext::PRE_SHARED_KEY.to_be_bytes();
    // ALLE Fundstellen, jede **strukturell** geprueft, und die erste
    // genuegt (Begruendung im Fehlerzweig darunter). added 01.10.2026,
    // R4 Teil 2 — vorher stand hier "die letzte Fundstelle nehmen",
    // was an Ticketbytes scheitert.
    let mut treffer: Option<(usize, usize)> = None;
    if nummer.len() <= nachricht.len() {
        for i in 0..=(nachricht.len() - nummer.len()) {
            if nachricht[i..i + nummer.len()] != nummer {
                continue;
            }
            if let Some(lage) = psk_stelle_pruefen(nachricht, i) {
                treffer = Some(lage);
                break;
            }
        }
    }
    // DER KORRIGIERTE WEG, added 01.10.2026, R4 Teil 2: nicht die letzte
    // Fundstelle nehmen, sondern die **erste strukturell gueltige**.
    //
    // DIE ALTE FASSUNG (`let stelle = treffer…` ohne Pruefung) ist in
    // `tests/psk_angebot.rs` gescheitert: das Ticket enthielt ein `00 29`
    // und `LayoutFalsch` sagte "Erweiterungslaenge (sie muss bis zum
    // Nachrichtenende reichen)" bei Stelle 170. Siehe den Kopf von
    // `psk_stelle_pruefen` fuer die volle Begruendung — kurz gesagt: die
    // Suche machte aus einer Berechnung eine Suchanfrage, und eine
    // Suchanfrage kann man treffen.
    //
    // WARUM DIE **ERSTE** gueltige und nicht die letzte: die echte
    // Erweiterung ist die **letzte**, ihre Typbytes stehen also **vor**
    // ihrem Inhalt. Ein Kandidat tiefer in der Nachricht liegt im Ticket
    // oder im Binder und kann den Identitaeten-Weg nicht bestehen; wenn
    // doch, ist er ein Kandidat, dessen Weg **innerhalb** des Inhalts der
    // echten Erweiterung liegt — und dann waere die echte Erweiterung
    // keine, weil `inhalt_start + laenge` dann nicht bis zum Ende reichte.
    // Es kann also hoechstens **eine** gueltige Stelle geben.
    //
    // DER FEHLER, wenn nichts passt: `stelle: 0` und der Grund, dass die
    // Nachricht **nirgends** eine `pre_shared_key`-Erweiterung traegt, die
    // die fuenf Pruefungen besteht. Frueher stand hier der Hinweis auf die
    // Laengenpruefung; der stand an dieser Stelle aber **nach** der
    // Fundstellensuche, und eine falsche Fundstelle erzeugte genau ihn —
    // die Meldung behauptete also etwas ueber die Nachricht, das an der
    // Fundstelle gar nicht geprueft worden war.
    treffer.ok_or(BinderFehler::LayoutFalsch {
        stelle: 0,
        was: "Erweiterungsnummer 41 irgendwo in der Nachricht",
    })
}

/// Setzt den `index`-ten Binder in einem **fertigen** ClientHello.
///
/// added 01.10.2026, R4. DER ZWEITE TEIL DER ZWEIMAL-REGEL:
/// [`ps_k_erweiterung_ohne_binder`] baut den Block mit `BINDER_LAENGE`
/// Nullbindern, der Binder wird ueber den **abgeschnittenen** ClientHello
/// gerechnet (RFC 8446 4.2.11.2), und mit dieser Funktion kommt er an
/// seine Stelle. **Kein Laengenfeld aendert sich** — die Laengen waren
/// schon beim Hashen endgueltig ("all fields ... are all set as if
/// binders of the correct lengths were present", 4.2.11.2).
///
/// DER WEG ZUR STELLE, und er ist eine Rechnung von **hinten**, ausgehend
/// vom Ende der Nachricht (siehe [`psk_lage`] fuer den Vorwaertsweg,
/// der die Erweiterung findet):
/// ```text
///   listen_start = len(nachricht) - anzahl * (1 + BINDER_LAENGE)
///   stelle       = listen_start + index * (1 + BINDER_LAENGE) + 1
/// ```
/// Das `+ 1` ueberspringt das 1-Byte-Laengenfeld des Eintrags.
///
/// M3/M4: jede Grenze wird **vor** dem Schreiben geprueft. Es gibt kein
/// `copy_from_slice` auf einem Slice, der zu kurz sein koennte, und ohne
/// die Pruefungen wuerde ein Aufrufer mit dem falschen Puffer 32 Byte an
/// einer **falschen** Stelle ueberschreiben — der Client schickte dann
/// eine scheinbar gueltige, aber unauffindbare Nachricht.
pub fn binder_setzen(
    clienthello: &mut [u8],
    anzahl: usize,
    index: usize,
    wert: &[u8; BINDER_LAENGE],
) -> Result<usize, BinderFehler> {
    if anzahl == 0 || index >= anzahl {
        return Err(BinderFehler::FalscherIndex { war: index, anzahl });
    }
    let eintrag = 1 + BINDER_LAENGE;
    let liste = anzahl * eintrag;
    // M3: `anzahl` kann aus dem Aufrufer kommen; `anzahl * 33` muss in
    // `u16` passen, weil die Binderliste eine 16-Bit-Laenge traegt.
    if liste > MAX_TICKET_BYTES {
        return Err(BinderFehler::ZuLang {
            war: liste,
            max: MAX_TICKET_BYTES,
        });
    }
    let (inhalt_start, inhalt) = psk_lage(clienthello)?;
    // Die Binderliste ist der **letzte** Teil des Inhalts, und ihre
    // 2-Byte-Laenge steht 2 Byte vor ihr.
    let listen_start = clienthello.len() - liste;
    if listen_start < inhalt_start + 2 {
        return Err(BinderFehler::ZuKurz {
            war: clienthello.len(),
            braucht: inhalt_start + 2 + liste,
        });
    }
    if clienthello[listen_start - 2..listen_start] != (liste as u16).to_be_bytes() {
        return Err(BinderFehler::LayoutFalsch {
            stelle: listen_start - 2,
            was: "Binderlisten-Laenge",
        });
    }
    // Und sie liegt auch wirklich am Ende des Erweiterungsinhalts.
    if listen_start + liste != inhalt_start + inhalt {
        return Err(BinderFehler::LayoutFalsch {
            stelle: listen_start,
            was: "Binderliste am Ende des Erweiterungsinhalts",
        });
    }
    let stelle = listen_start + index * eintrag + 1;
    if clienthello[stelle - 1] as usize != BINDER_LAENGE {
        return Err(BinderFehler::LayoutFalsch {
            stelle: stelle - 1,
            was: "Binderlaenge",
        });
    }
    clienthello[stelle..stelle + BINDER_LAENGE].copy_from_slice(wert);
    Ok(stelle)
}

/// Schneidet den ClientHello fuer die Binder-Berechnung ab.
///
/// added 01.10.2026, R4. RFC 8446 4.2.11.2: der Binder wird ueber
/// "a transcript hash of the partial ClientHello up to and including
/// the PreSharedKeyExtension.identities field, i.e., the ClientHello
/// including the binders field with the length of the binders field,
/// but excluding the length of the binders list".
///
/// DER WEG: alles bis einschliesslich der Identitaeten, dann die
/// **2-Byte-Laenge der Binderliste**, dann Schluss. Die Liste selbst
/// (Eintraege mit Laengenbyte und 32 Byte Inhalt) fliegt raus.
///
/// DER TEST DER PRUEFUNG, und er ist der Grund fuer `psk_lage`: ohne die
/// Lagepruefung waere der abgeschnittene ClientHello ein **anderer** als
/// der gesendete, der Binder also falsch — und nichts wuerde es merken.
/// Mit der Pruefung ist ein falscher Puffer ein benannter Fehler.
pub fn transkript_fuer_binder(clienthello: &[u8], anzahl: usize) -> Result<Vec<u8>, BinderFehler> {
    let eintrag = 1 + BINDER_LAENGE;
    let liste = anzahl * eintrag;
    if anzahl == 0 {
        return Err(BinderFehler::FalscherIndex { war: 0, anzahl });
    }
    if liste > MAX_TICKET_BYTES {
        return Err(BinderFehler::ZuLang {
            war: liste,
            max: MAX_TICKET_BYTES,
        });
    }
    let (inhalt_start, inhalt) = psk_lage(clienthello)?;
    if clienthello.len() < inhalt_start + 2 + liste {
        return Err(BinderFehler::ZuKurz {
            war: clienthello.len(),
            braucht: inhalt_start + 2 + liste,
        });
    }
    let listen_start = clienthello.len() - liste;
    if clienthello[listen_start - 2..listen_start] != (liste as u16).to_be_bytes() {
        return Err(BinderFehler::LayoutFalsch {
            stelle: listen_start - 2,
            was: "Binderlisten-Laenge",
        });
    }
    if listen_start + liste != inhalt_start + inhalt {
        return Err(BinderFehler::LayoutFalsch {
            stelle: listen_start,
            was: "Binderliste am Ende des Erweiterungsinhalts",
        });
    }
    // DER ENDPUNKT, und den hat der Trace aus RFC 8448 Abschnitt 4
    // entschieden — gegen den Normtext.
    //
    // added 01.10.2026, R4. DIE ZAHLEN: der gekuerzte ClientHello hat
    // **477** Byte, die vollstaendige Nachricht **512** (`01 00 01 fc` =
    // 508 Byte Inhalt plus 4 Byte Kopf). Es fallen also **35** Byte weg:
    // 2 (Laenge der Binderliste) + 1 (Laenge des Eintrags) + 32 (Binder).
    // Der Schnitt liegt damit **vor** der Binderliste UND vor ihrer
    // Laengenangabe: `clienthello[..listen_start - 2]`.
    //
    // DER NORMTEXT LIEGT HIER ANDERS, und das ist der Grund fuer den
    // Hinweis: RFC 8446 4.2.11.2 sagt "up to and including the
    // PreSharedKeyExtension.identities field, i.e., the ClientHello
    // including the binders field with the length of the binders field,
    // but excluding the length of the binders list". Gelesen heisst das:
    // die 2-Byte-Laenge der Binderliste **mitnehmen**, denn sie ist die
    // "length of the binders field". Der Trace zeigt aber, dass alle 35
    // Byte zusammen fallen — die 2 Byte also auch.
    //
    // MEINE ZWISCHENFASSUNGEN, drei an dieser Stelle: `+ 2` (2 Byte zu
    // lang), dann `[..listen_start]` (2 Byte zu lang), jetzt
    // `[..listen_start - 2]`. Der Test
    // `der_binder_stimmt_byteweise_mit_rfc8448_abschnitt_4` in
    // `tests/ticket.rs` stellt die 477 Byte **aus dem Trace** daneben,
    // also kann diese Zahl nicht mehr falsch sein.
    Ok(clienthello[..listen_start - 2].to_vec())
}
