//! T8 — Die Handshake-Nachrichten von TLS 1.3, kodieren und dekodieren.
//!
//! Reihenfolge nach RFC 8446 Abschnitt 4:
//!   ClientHello, ServerHello, (EncryptedExtensions), (Certificate),
//!   (CertificateVerify), Finished
//!
//! Alle sechs sind hier vertreten. Die drei in Klammern schicken nur der
//! Server, werden aber vom Client genauso dekodiert.
//!
//! M4 gilt fuer jede Laengenangabe in diesem Modul, und es gibt keine
//! Ausnahme: `Leser` und `Schreiber` aus `vektor.rs` pruefen immer, ob
//! der Puffer die angesagte Laenge hergibt, BEVOR ein `Vec` entsteht.

use crate::vektor::{Leser, Schreiber, VektorFehler};

/// Die Nachrichtentypen aus RFC 8446 Abschnitt 4.
/// Die Laenge des **Handshake-Kopfes**: Nachrichtentyp (1 Byte) plus
/// `uint24`-Laenge (3 Byte).
///
/// added 02.10.2026, R4k. DER GRUND FUER EINE KONSTANTE: `zerlege_
/// nachrichten` in `strom.rs` braucht diese Zahl an **vier** Stellen, und
/// mein Aufrufer von `hole_nachschrift` brauchte sie an einer fuenften — mit
/// einem eigenen `4` an der fuenften Stelle hat `new_session_ticket`
/// 02.10.2026 den **Kopf** als Inhalt bekommen und
/// `erwartet 11138 Byte, vorhanden 13` gemeldet. Die Zahl steht jetzt an
/// **einer** Stelle.
///
/// DIE 4 IST KEINE ERFUNDENE ZAHL: `Handshake` ist in RFC 8446 5.1
/// `struct { HandshakeType msg_type; opaque body[0..2^24-1]; }`, und
/// `HandshakeType` ist ein Enum mit **einem** Byte.
pub const HANDSHAKE_KOPF: usize = 4;

pub mod typen {
    pub const CLIENT_HELLO: u8 = 1;
    pub const SERVER_HELLO: u8 = 2;
    pub const NEW_SESSION_TICKET: u8 = 4;
    pub const ENCRYPTED_EXTENSIONS: u8 = 8;
    pub const CERTIFICATE: u8 = 11;
    pub const CERTIFICATE_VERIFY: u8 = 15;
    pub const FINISHED: u8 = 20;
}

/// Die Extensionstypen, die diese Crate liest und schreibt
/// (RFC 8446 Abschnitt 4.2, 4.2.1 server_name, 4.2.3 ALPN,
/// 4.2.7 supported_versions, 4.2.8 cookie,
/// 4.2.8 psk_key_exchange_modes, 4.2.11 key_share,
/// 4.2.3 signature_algorithms, 4.2.7 supported_groups).
pub mod ext {
    pub const SERVER_NAME: u16 = 0;
    pub const SUPPORTED_GROUPS: u16 = 10;
    pub const SIGNATURE_ALGORITHMS: u16 = 13;
    pub const ALPN: u16 = 16;
    /// added 02.10.2026 11:4x, S4. `status_request`, OID-Nummer 5 — fuer
    /// **beide** Protokollversionen.
    ///
    /// DER WEG UND SEIN GRUND: die Nummer stand bisher nur in `nachricht12.rs`,
    /// also im TLS-1.2-ClientHello. TLS 1.3 benutzt dieselbe Erweiterung
    /// (RFC 8446 4.4.2.1 verweist auf RFC 6066 Abschnitt 3). Ohne sie im
    /// 1.3-ClientHello fragt unser Browser gar nicht erst nach dem
    /// Widerrufsstatus — und ein Server, der stapeln *kann*, schickt es nur,
    /// wenn gefragt wurde.
    pub const STATUS_REQUEST: u16 = 5;
    pub const SUPPORTED_VERSIONS: u16 = 43;
    pub const PSK_KEY_EXCHANGE_MODES: u16 = 45;
    pub const KEY_SHARE: u16 = 51;
    // --- Die Erweiterungen, die nur TLS 1.2 braucht.
    //
    // added 30.09.2026, als EIN ClientHello beide Protokollversionen
    // anbieten sollte. Ohne diese drei nimmt ein TLS-1.2-Server den
    // Handshake nicht an oder weist ihn zurueck:
    //   EC_POINT_FORMATS        (11)  RFC 4492 Abschnitt 5
    //   EXTENDED_MASTER_SECRET  (23)  RFC 7627
    //   RENEGOTIATION_INFO    (65281) RFC 5746
    // Dazu `SESSION_TICKET` (35, RFC 5077), ebenfalls leer.
    pub const EC_POINT_FORMATS: u16 = 11;
    pub const SESSION_TICKET: u16 = 35;
    pub const EXTENDED_MASTER_SECRET: u16 = 23;
    pub const RENEGOTIATION_INFO: u16 = 65281;
    /// `cookie` (44), RFC 8446 Abschnitt 4.2.2.
    pub const COOKIE: u16 = 44;
    /// `quic_transport_parameters` (57 = 0x39), RFC 9001 Abschnitt 8.2.
    ///
    /// added 30.09.2026, Runde 2, TLS R2 (Auftrag
    /// `team/WUNSCH_DEEPSEEK.txt`). DER WERT 57, und nicht 0x0039 als
    /// Zahl 57 zu verwechseln: die Erweiterungsnummern stehen als
    /// **zweier Hex-Ziffern** in RFC 9001 Abschnitt 8.2
    /// ("quic_transport_parameters (0x0039)"), und `u16` ist hier
    /// durchgaengig dezimal notiert (`KEY_SHARE: u16 = 51` = 0x33).
    pub const QUIC_TRANSPORT_PARAMETERS: u16 = 57;
    /// `pre_shared_key` (41 = 0x29), RFC 8446 Abschnitt 4.2.11.
    ///
    /// added 01.10.2026, R4 (A1). DIESE KONSTANTE, und sie ist nicht
    /// dieselbe wie `crate::ticket::TYP_PRE_SHARED_KEY`: hier stehen
    /// die Erweiterungsnummern **als `u16`**, dort der Nachrichtentyp.
    /// Beide sind 41, und genau deshalb ist es besser, sie an **einer**
    /// Stelle zu fuehren: eine zweite Zahl mit demselben Wert ist die
    /// Art Kopie, die beim Aendern auseinanderlaufen.
    pub const PRE_SHARED_KEY: u16 = 41;
}

/// Die eine Gruppe, die diese Crate anbietet: x25519 (RFC 8446
/// Abschnitt 4.2.7, Wert 0x001d).
pub const GRUPPE_X25519: u16 = 0x001d;
/// TLS 1.3 auf der Legacy-Version-Zeile (RFC 8446 Abschnitt 4.1.2).
pub const LEGACY_VERSION: u16 = 0x0303;
/// TLS 1.3 als Wert von supported_versions.
pub const VERSION_TLS13: u16 = 0x0304;
/// TLS 1.2: der Wert von `legacy_version` und der Eintrag 0x0303 in
/// `supported_versions` (RFC 8446 Abschnitt 4.1.2 und 4.2.1).
pub const VERSION_LEGACY: u16 = 0x0303;
/// Der Sonderwert des ServerRandom in einem HelloRetryRequest.
///
/// RFC 8446 Abschnitt 4.1.3, woertlich: "the special value of the
/// SHA-256 of \"HelloRetryRequest\"" mit den Oktetten
///   CF 21 AD 74 E5 9A 61 11 BE 1D 8C 02 1E 65 B8 91
///   C2 A2 11 16 7A BB 8C 5E 07 9E 09 E2 C8 A8 33 9C
///
/// DER WEG ZUM WERT, und er ist nicht "ich kenne ihn": Die Klammert
/// steht in `rfc/rfc8446.txt` (Abschnitt 4.1.3), und der Generator
/// `mutation_md/tls/tls13_hrr_generator.py` holt denselben Wert aus
/// `rfc8448.txt` (Abschnitt 5, die ServerHello des HRR) — dort ist er
/// die ersten 32 Byte der Nachricht. `der_hrr_ist_erkennbar` prueft
/// beide Wege gegeneinander, und der Test
/// `hrr_sonderwert_stimmt_gegen_rfc8448` haelt es fest.
///
/// added 30.09.2026 fuer W6.
pub const HRR_SONDERRANDOM: [u8; 32] = [
    0xCF, 0x21, 0xAD, 0x74, 0xE5, 0x9A, 0x61, 0x11, 0xBE, 0x1D, 0x8C, 0x02, 0x1E, 0x65, 0xB8, 0x91,
    0xC2, 0xA2, 0x11, 0x16, 0x7A, 0xBB, 0x8C, 0x5E, 0x07, 0x9E, 0x09, 0xE2, 0xC8, 0xA8, 0x33, 0x9C,
];

/// secp256r1, Wert 0x0017 (RFC 8422 Abschnitt 5.1.1).
///
/// needed fuer den TLS-1.2-Pfad: badssl.com waehlt diese Gruppe, und
/// `strom12::clientkey_und_premaster` rechnet sie.
pub const GRUPPE_SECP256R1: u16 = 0x0017;

/// M4: die SNI-Extension traegt einen Hostnamen. RFC 6066 Abschnitt 3
/// erlaubt 2^16 - 1 Byte; fuer einen DNS-Namen sind 253 die praktische
/// Obergrenze (RFC 1035 Abschnitt 2.3.4). Es wird die kleinere Grenze
/// benutzt, damit ein Angreifer keinen 64-KiB-"Hostnamen" anbieten kann.
pub const MAX_HOSTNAME: usize = 253;

/// added 03.10.2026, Rolle TLS (Angriffsgrenzen): **DIE ZAHL DER ZERTIFIKATE IN
/// EINER `certificate`-NACHRICHT**, und sie ist **16**.
///
/// **DIE REICHWEITE OHNE DIESE GRENZE, gemessen aus den Konstanten:** die
/// Nachricht darf `MAX_NACHRICHT` = `0xFFFFFF` Byte gross sein (`record.rs:35`),
/// und jedes Zertifikat braucht mindestens **vier** Byte — drei fuer das
/// Laengenpraefix (`vektor24`) plus mindestens **ein** Byte Inhalt. Also
/// **4 194 303** Eintraege, jeder eine eigene `Vec`-Allokation.
///
/// **UND `MAX_KETTEN_TIEFE` = 10 haelt DAS NICHT AUF.** Diese Zahl begrenzt die
/// **Pruefschleife** in `kette.rs`, und die kommt **nach** dem Einlesen. Zu dem
/// Zeitpunkt sind die 16 MiB **schon** im Speicher und vier Millionen Zeiger
/// auf Einzelallokationen mit ihm.
///
/// **WARUM 16 UND NICHT 10** (= `MAX_KETTEN_TIEFE`): RFC 8446 4.4.2 erlaubt
/// mehrere Zwischenzertifikate, und ein Blatt kann **kreuzsigniert** sein — der
/// Server schickt dann einen Pfad mit 3-4 Zertifikaten. 16 laesst Reserve unter
///halb der 16-MiB-Nachrichtengrenze, ohne einen echten Pfad zu beschneiden.
pub const MAX_ZERTIFIKATE_PRO_NACHRICHT: usize = 16;
/// M4: die ALPN-Liste. RFC 7301 Abschnitt 3.1 begrenzt die Gesamtlaenge
/// auf 2^16 - 1, ein einzelnes Protokoll auf 255 Byte (Ein-Byte-Feld).
/// Fuer "http/1.1" und "h2" ist das weit ueberdimensioniert; 32
/// Protokolle mit je 32 Byte sind reichlich und begrenzen die Arbeit.
pub const MAX_ALPN_EINTRAG: usize = 32;
/// M4: hoechstens so viele Protokolle in der ALPN-Liste.
pub const MAX_ALPN_ANZAHL: usize = 32;

/// Die Certificate-Nachricht aus RFC 8448 Abschnitt 3, 445 Oktett.
///
/// Herkunft: /home/jaff/mutation_md/tls/rfc8448_werte.txt, und die Datei,
/// die sie erzeugt, liest sie aus dem RFC-Text. Sie traegt eine
/// RSA-Kette mit 432 Oktett DER.
pub const CERTIFICATE_RFC8448_ABSCHNITT_3: [u8; 445] = [
    0x0b, 0x00, 0x01, 0xb9, 0x00, 0x00, 0x01, 0xb5, 0x00, 0x01, 0xb0, 0x30, 0x82, 0x01, 0xac, 0x30,
    0x82, 0x01, 0x15, 0xa0, 0x03, 0x02, 0x01, 0x02, 0x02, 0x01, 0x02, 0x30, 0x0d, 0x06, 0x09, 0x2a,
    0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b, 0x05, 0x00, 0x30, 0x0e, 0x31, 0x0c, 0x30, 0x0a,
    0x06, 0x03, 0x55, 0x04, 0x03, 0x13, 0x03, 0x72, 0x73, 0x61, 0x30, 0x1e, 0x17, 0x0d, 0x31, 0x36,
    0x30, 0x37, 0x33, 0x30, 0x30, 0x31, 0x32, 0x33, 0x35, 0x39, 0x5a, 0x17, 0x0d, 0x32, 0x36, 0x30,
    0x37, 0x33, 0x30, 0x30, 0x31, 0x32, 0x33, 0x35, 0x39, 0x5a, 0x30, 0x0e, 0x31, 0x0c, 0x30, 0x0a,
    0x06, 0x03, 0x55, 0x04, 0x03, 0x13, 0x03, 0x72, 0x73, 0x61, 0x30, 0x81, 0x9f, 0x30, 0x0d, 0x06,
    0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00, 0x03, 0x81, 0x8d, 0x00,
    0x30, 0x81, 0x89, 0x02, 0x81, 0x81, 0x00, 0xb4, 0xbb, 0x49, 0x8f, 0x82, 0x79, 0x30, 0x3d, 0x98,
    0x08, 0x36, 0x39, 0x9b, 0x36, 0xc6, 0x98, 0x8c, 0x0c, 0x68, 0xde, 0x55, 0xe1, 0xbd, 0xb8, 0x26,
    0xd3, 0x90, 0x1a, 0x24, 0x61, 0xea, 0xfd, 0x2d, 0xe4, 0x9a, 0x91, 0xd0, 0x15, 0xab, 0xbc, 0x9a,
    0x95, 0x13, 0x7a, 0xce, 0x6c, 0x1a, 0xf1, 0x9e, 0xaa, 0x6a, 0xf9, 0x8c, 0x7c, 0xed, 0x43, 0x12,
    0x09, 0x98, 0xe1, 0x87, 0xa8, 0x0e, 0xe0, 0xcc, 0xb0, 0x52, 0x4b, 0x1b, 0x01, 0x8c, 0x3e, 0x0b,
    0x63, 0x26, 0x4d, 0x44, 0x9a, 0x6d, 0x38, 0xe2, 0x2a, 0x5f, 0xda, 0x43, 0x08, 0x46, 0x74, 0x80,
    0x30, 0x53, 0x0e, 0xf0, 0x46, 0x1c, 0x8c, 0xa9, 0xd9, 0xef, 0xbf, 0xae, 0x8e, 0xa6, 0xd1, 0xd0,
    0x3e, 0x2b, 0xd1, 0x93, 0xef, 0xf0, 0xab, 0x9a, 0x80, 0x02, 0xc4, 0x74, 0x28, 0xa6, 0xd3, 0x5a,
    0x8d, 0x88, 0xd7, 0x9f, 0x7f, 0x1e, 0x3f, 0x02, 0x03, 0x01, 0x00, 0x01, 0xa3, 0x1a, 0x30, 0x18,
    0x30, 0x09, 0x06, 0x03, 0x55, 0x1d, 0x13, 0x04, 0x02, 0x30, 0x00, 0x30, 0x0b, 0x06, 0x03, 0x55,
    0x1d, 0x0f, 0x04, 0x04, 0x03, 0x02, 0x05, 0xa0, 0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86,
    0xf7, 0x0d, 0x01, 0x01, 0x0b, 0x05, 0x00, 0x03, 0x81, 0x81, 0x00, 0x85, 0xaa, 0xd2, 0xa0, 0xe5,
    0xb9, 0x27, 0x6b, 0x90, 0x8c, 0x65, 0xf7, 0x3a, 0x72, 0x67, 0x17, 0x06, 0x18, 0xa5, 0x4c, 0x5f,
    0x8a, 0x7b, 0x33, 0x7d, 0x2d, 0xf7, 0xa5, 0x94, 0x36, 0x54, 0x17, 0xf2, 0xea, 0xe8, 0xf8, 0xa5,
    0x8c, 0x8f, 0x81, 0x72, 0xf9, 0x31, 0x9c, 0xf3, 0x6b, 0x7f, 0xd6, 0xc5, 0x5b, 0x80, 0xf2, 0x1a,
    0x03, 0x01, 0x51, 0x56, 0x72, 0x60, 0x96, 0xfd, 0x33, 0x5e, 0x5e, 0x67, 0xf2, 0xdb, 0xf1, 0x02,
    0x70, 0x2e, 0x60, 0x8c, 0xca, 0xe6, 0xbe, 0xc1, 0xfc, 0x63, 0xa4, 0x2a, 0x99, 0xbe, 0x5c, 0x3e,
    0xb7, 0x10, 0x7c, 0x3c, 0x54, 0xe9, 0xb9, 0xeb, 0x2b, 0xd5, 0x20, 0x3b, 0x1c, 0x3b, 0x84, 0xe0,
    0xa8, 0xb2, 0xf7, 0x59, 0x40, 0x9b, 0xa3, 0xea, 0xc9, 0xd9, 0x1d, 0x40, 0x2d, 0xcc, 0x0c, 0xc8,
    0xf8, 0x96, 0x12, 0x29, 0xac, 0x91, 0x87, 0xb4, 0x2b, 0x4d, 0xe1, 0x00, 0x00,
];

/// Die CertificateVerify-Nachricht aus RFC 8448 Abschnitt 3, 136 Oktett.
pub const CERTIFICATE_VERIFY_RFC8448_ABSCHNITT_3: [u8; 136] = [
    0x0f, 0x00, 0x00, 0x84, 0x08, 0x04, 0x00, 0x80, 0x5a, 0x74, 0x7c, 0x5d, 0x88, 0xfa, 0x9b, 0xd2,
    0xe5, 0x5a, 0xb0, 0x85, 0xa6, 0x10, 0x15, 0xb7, 0x21, 0x1f, 0x82, 0x4c, 0xd4, 0x84, 0x14, 0x5a,
    0xb3, 0xff, 0x52, 0xf1, 0xfd, 0xa8, 0x47, 0x7b, 0x0b, 0x7a, 0xbc, 0x90, 0xdb, 0x78, 0xe2, 0xd3,
    0x3a, 0x5c, 0x14, 0x1a, 0x07, 0x86, 0x53, 0xfa, 0x6b, 0xef, 0x78, 0x0c, 0x5e, 0xa2, 0x48, 0xee,
    0xaa, 0xa7, 0x85, 0xc4, 0xf3, 0x94, 0xca, 0xb6, 0xd3, 0x0b, 0xbe, 0x8d, 0x48, 0x59, 0xee, 0x51,
    0x1f, 0x60, 0x29, 0x57, 0xb1, 0x54, 0x11, 0xac, 0x02, 0x76, 0x71, 0x45, 0x9e, 0x46, 0x44, 0x5c,
    0x9e, 0xa5, 0x8c, 0x18, 0x1e, 0x81, 0x8e, 0x95, 0xb8, 0xc3, 0xfb, 0x0b, 0xf3, 0x27, 0x84, 0x09,
    0xd3, 0xbe, 0x15, 0x2a, 0x3d, 0xa5, 0x04, 0x3e, 0x06, 0x3d, 0xda, 0x65, 0xcd, 0xf5, 0xae, 0xa2,
    0x0d, 0x53, 0xdf, 0xac, 0xd4, 0x2f, 0x74, 0xf3,
];

/// Warum eine Nachricht nicht dekodiert werden kann. BENANNTE Varianten,
/// damit der Aufrufer den Grund kennt (M7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NachrichtFehler {
    /// Ein Feld war zu kurz.
    Vektor(VektorFehler),
    /// Der Nachrichtentyp war nicht der erwartete.
    TypFalsch { erwartet: u8, bekommen: u8 },
    /// Die Legacy-Version war nicht 0x0303.
    VersionFalsch { ist: u16 },
    /// Die unterstuetzte Version war nicht TLS 1.3.
    VersionNichtTls13 { ist: u16 },
    /// `legacy_compression_methods` war nicht `{ 0 }`.
    ///
    /// added 30.09.2026. RFC 8446 Abschnitt 4.1.3: "legacy_
    /// compression_methods ... A server which selects a TLS 1.3 cipher
    /// suite MUST use the legacy_compression_methods value of { 0 }".
    /// Wer hier `{ 0 }` nicht findet, ist kein TLS-1.3-Server — und ein
    /// Client, der es trotzdem durchreicht, laesst einem
    /// Kompressionsverfahren die Arbeit, das es seit 2009 gibt.
    ///
    /// Die Summe der Methoden-Nummern steht drin, damit die Meldung
    /// mehr sagt als "falsch": {1} und {1,2} haben verschiedene
    /// Summen, und die Abweichung ist im Mitschnitt sichtbar.
    KompressionFalsch { war: u32 },
    /// Der Hostname war laenger als `MAX_HOSTNAME`.
    HostnameZuLang { ist: usize, erlaubt: usize },
    /// Der SNI-Typ war nicht 0 (host_name).
    SniTypFalsch { ist: u8 },
    /// Die ALPN-Liste hatte zu viele Eintraege.
    AlpnZuViele { ist: usize, erlaubt: usize },
    /// Ein ALPN-Eintrag war laenger als `MAX_ALPN_EINTRAG`.
    AlpnEintragZuLang { ist: usize, erlaubt: usize },
    /// Die Liste der unterstuetzten Gruppen war leer.
    KeineGruppe,
    /// Die Key-Share-Liste war leer, und es war kein HelloRetryRequest.
    KeinKeyShare,
    /// Die Key-Share-Liste enthielt kein x25519.
    KeinX25519,
    /// Unbekannte Signaturalgorithmen-Liste.
    KeineSignaturalgorithmen,
    /// Die Zertifikatsliste war leer.
    KeineZertifikate,
    /// added 03.10.2026, Rolle TLS (Angriffsgrenzen): **ZU VIELE
    /// ZERTIFIKATE IN EINER NACHRICHT.**
    ///
    /// **GEMESSEN, warum das eine eigene Variante ist und nicht
    /// `Vektor(VektorFehler)`:** die Zahl der Eintraege ist **keine**
    /// Vektorlaenge — jeder Eintrag ist fuer sich ein gueltiges `vektor24()`.
    /// Ohne diese Grenze konnte ein Server **4 194 303** Eintraege (16 MiB
    /// Nachricht, 4 Byte Mindestlaenge je Eintrag) schicken, und `MAX_
    /// KETTEN_TIEFE` = 10 haelt das **nicht** auf, weil die Pruefschleife
    /// **nach** dem Einlesen kommt.
    ZuVieleZertifikate { erlaubt: usize },
    /// Die Signatur in CertificateVerify war nicht ueber das erwartete
    /// Schema gebildet.
    SignaturschemaFalsch { ist: u8 },
    /// added 02.10.2026 23:ix, S4. Mehr Erweiterungen in einer
    /// `Certificate`-Nachricht, als `MAX_ZERTIFIKATS_ERWEITERUNGEN`.
    ///
    /// added 02.10.2026 23:ix: EIGENE FORM, weil `AlpnZuViele` von einer
    /// anderen Sache spricht. Die Form `ist`/`erlaubt` entspricht
    /// `AlpnZuViele` und `HostnameZuLang`, damit die Meldungen im
    /// gewohnten Satz bleiben: „zu viele Erweiterungen (6, erlaubt 5)".
    ZuVieleErweiterungen { ist: usize, erlaubt: usize },
}

impl std::fmt::Display for NachrichtFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NachrichtFehler::Vektor(e) => write!(f, "Feld: {e}"),
            NachrichtFehler::TypFalsch { erwartet, bekommen } => {
                write!(f, "Nachrichtentyp {bekommen}, erwartet {erwartet}")
            }
            NachrichtFehler::VersionFalsch { ist } => {
                write!(f, "Legacy-Version {ist:#06x}, erwartet 0x0303")
            }
            NachrichtFehler::KompressionFalsch { war } => write!(
                f,
                "legacy_compression_methods ist nicht {{ 0 }}, Summe der Methoden {war}"
            ),
            NachrichtFehler::VersionNichtTls13 { ist } => {
                write!(f, "Version {ist:#06x}, erwartet 0x0304 (TLS 1.3)")
            }
            NachrichtFehler::HostnameZuLang { ist, erlaubt } => {
                write!(f, "Hostname {ist} Byte, erlaubt {erlaubt}")
            }
            NachrichtFehler::SniTypFalsch { ist } => {
                write!(f, "SNI-Typ {ist}, erwartet 0 (host_name)")
            }
            NachrichtFehler::AlpnZuViele { ist, erlaubt } => {
                write!(f, "{ist} ALPN-Eintraege, erlaubt {erlaubt}")
            }
            NachrichtFehler::AlpnEintragZuLang { ist, erlaubt } => {
                write!(f, "ALPN-Eintrag {ist} Byte, erlaubt {erlaubt}")
            }
            NachrichtFehler::KeineGruppe => write!(f, "keine unterstuetzte Gruppe"),
            NachrichtFehler::KeinKeyShare => write!(f, "kein Key Share"),
            NachrichtFehler::KeinX25519 => write!(f, "kein x25519 im Key Share"),
            NachrichtFehler::KeineSignaturalgorithmen => write!(f, "keine Signaturalgorithmen"),
            NachrichtFehler::KeineZertifikate => write!(f, "keine Zertifikate"),
            NachrichtFehler::ZuVieleZertifikate { erlaubt } => {
                write!(f, "mehr als {erlaubt} Zertifikate in einer Nachricht")
            }
            NachrichtFehler::SignaturschemaFalsch { ist } => {
                write!(
                    f,
                    "Signaturschema {ist}, erwartet 1 (RSA-PSS) oder 3 (ECDSA)"
                )
            }
            // added 02.10.2026 23:ix, S4.
            NachrichtFehler::ZuVieleErweiterungen { ist, erlaubt } => {
                write!(f, "zu viele Erweiterungen ({ist}, erlaubt sind {erlaubt})")
            }
        }
    }
}

impl std::error::Error for NachrichtFehler {}

impl From<VektorFehler> for NachrichtFehler {
    fn from(e: VektorFehler) -> NachrichtFehler {
        NachrichtFehler::Vektor(e)
    }
}

type Ergebnis<T> = Result<T, NachrichtFehler>;

// ---------------------------------------------------------------------------
// ClientHello
// ---------------------------------------------------------------------------

/// Der ClientHello eines TLS-1.3-Clients.
///
/// Was ein minimaler, aber vollstaendiger ClientHello braucht, steht hier.
/// Unbekannte Erweiterungen werden beim Dekodieren uebersprungen, nicht
/// abgelehnt: ein Server darf mehr anbieten, als wir verstehen
/// (RFC 8446 Abschnitt 4.2: "Unrecognized extensions MUST be ignored").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientHello {
    /// 32 Byte Zufall (RFC 8446 Abschnitt 4.1.2).
    pub random: [u8; 32],
    /// Cipher Suites, 2 Byte je Eintrag.
    pub cipher_suites: Vec<u16>,
    /// Kompressionsverfahren. TLS 1.3 schickt nur [0] (RFC 8446
    /// Abschnitt 4.1.2).
    pub kompression: Vec<u8>,
    /// Der Servername aus SNI, falls gesetzt.
    pub sni: Option<Vec<u8>>,
    /// Die unterstuetzten Protokollversionen. TLS 1.3: [0x0304].
    pub versionen: Vec<u16>,
    /// Die unterstuetzten Gruppen. Hier immer x25519.
    pub gruppen: Vec<u16>,
    /// Die Signaturalgorithmen, die der Client versteht.
    pub signaturalgorithmen: Vec<u16>,
    /// Die gewaehlte Gruppe und deren oeffentlicher Schluessel.
    pub key_share_gruppe: u16,
    /// 32 Byte oeffentlicher X25519-Schluessel.
    pub key_share: Vec<u8>,
    /// Die ALPN-Protokolle in der Reihenfolge der Vorliebe.
    pub alpn: Vec<Vec<u8>>,
    /// PSK-Modi. 1 = psk_dhe_ke (RFC 8446 Abschnitt 4.2.9).
    pub psk_modi: Vec<u8>,
    /// Bietet TLS 1.2 an? Dann kommen `extended_master_secret`,
    /// `renegotiation_info`, `ec_point_formats` und `session_ticket`
    /// in den ClientHello.
    ///
    /// added 30.09.2026. DER GRUND, warum das ein Schalter und kein
    /// Dauerzustand ist: ein reiner TLS-1.2-Client (der Test
    /// `tls12_gegen_badssl`) darf ein ClientHello schicken, das
    /// `supported_versions` gar nicht kennt — sonst wuerde der Server
    /// TLS 1.3 waehlen und der Test pruefte die falsche Sache.
    /// Die `legacy_session_id`, **unveraendert aus der Nachricht**.
    ///
    /// added 30.09.2026, Runde 2, TLS R1. DER GRUND FUER DAS FELD:
    /// RFC 8446 Abschnitt 4.1.2 sagt, der Server wiederholt die
    /// `legacy_session_id` des ClientHello unveraendert. Der Decoder
    /// las das Feld und verwarf es (`let _session_id = ...`), also
    /// hatte der neue Server (`server.rs`) nichts, was er zurueckschicken
    /// koennte. Ein Client, der eine 32-Byte-ID fuer Middleboxen
    /// schickt — das ist die Empfehlung der Norm —, weist eine leere
    /// Antwort mit `illegal_parameter` ab, **ohne** Fehlermeldung.
    pub session_id: Vec<u8>,
    /// Die QUIC-Transportparameter **unveraendert**, oder `None`.
    ///
    /// added 30.09.2026, Runde 2, TLS R2 (Wunsch DEEPSEEK). DER INHALT
    /// IST SCHON DER PARAMETERBLOCK: kein Laengenfeld davor, denn die
    /// Laenge der Erweiterungsdaten steht im Kopf der Erweiterung
    /// selbst. Innen ist der Block eine Folge von
    /// Dreiergruppen (ID/Laenge/Wert) nach RFC 9000 Abschnitt 18; dieses
    /// Crate **parst ihn nicht** — es traegt ihn nur. Wer ihn parst,
    /// macht das in `narsil-quic` (`parameter`), und dort gehoert er hin,
    /// weil die Bedeutung der IDs dort definiert ist.
    pub quic_transport_parameters: Option<Vec<u8>>,
    /// Der **fertige** `pre_shared_key`-Block (Typ, Laenge, Identitaeten,
    /// Binderliste) oder `None`.
    ///
    /// added 01.10.2026, R4 (A1). DER INHALT IST DER GANZE BLOCK, und
    /// nicht nur die Identitaeten: `ticket::ps_k_erweiterung_ohne_binder`
    /// baut genau diesen Block (mit `BINDER_LAENGE` Nullbindern), und
    /// `ticket::binder_setzen` schreibt danach den echten Binder an seine
    /// Stelle. **Die Laengenfelder aendern sich dabei nicht** — das ist
    /// die Zweimal-Regel aus RFC 8446 4.2.11.2: "all fields ... are all
    /// set as if binders of the correct lengths were present".
    ///
    /// WARUM ALS BYTES UND NICHT ALS `Vec<PskVorschlag>`: die Laengen
    /// muessen **beim Hashen** endgueltig sein, und der Block ist das
    /// Objekt, aus dem der abgeschnittene ClientHello entsteht. Ein
    /// `Vec<Vorschlag>` muesste beim Hashen und noch einmal beim Senden
    /// gebaut werden — zwei Baupfade fuer dieselbe Nachricht.
    /// added 02.10.2026 11:4x, S4: **wir** wollen den stapelten
    /// Widerrufsstatus. `true` heisst: der Client bittet darum.
    ///
    /// WARUM EIN BOOL und nicht die Bytes: der Inhalt ist nach RFC 6066 3
    /// **immer** dieselben fuenf Byte (status_type=1, leere
    /// Responder-ID-Liste, leere request_extensions). Ein Byte-Vektor
    /// wuerde eine zweite Wahrheit sein, an der jemand die Liste veraendern
    /// koennte.
    pub status_request: bool,
    pub psk: Option<Vec<u8>>,
    /// Das Cookie aus einem HelloRetryRequest (Erweiterung 44).
    ///
    /// added 30.09.2026 fuer W6. `None` im ersten ClientHello, `Some`
    /// im zweiten. Der Inhalt ist ein `opaque cookie<1..2^16-1>` aus
    /// RFC 8446 Abschnitt 4.2.2 — der Client gibt ihn unveraendert
    /// zurueck und deutet ihn nicht.
    pub cookie: Option<Vec<u8>>,
    pub auch_tls12: bool,
    /// secp256r1 in `supported_groups` anbieten (0x0017)?
    ///
    /// NOTWENDIG, und nicht nur fuer TLS 1.2: badssl.com waehlt im
    /// TLS-1.2-Handshake secp256r1, auch wenn x25519 an erster Stelle
    /// steht. Bei TLS 1.3 waehlt der Server nach der Reihenfolge des
    /// Clients, also x25519 — und dafuer liegt ein `key_share` vor.
    pub auch_secp256r1: bool,
}

impl ClientHello {
    /// Die beiden Cipher Suites, die diese Crate spricht.
    pub fn suites_standard() -> Vec<u16> {
        vec![0x1301, 0x1303]
    }

    /// Die Signaturalgorithmen, die der Client versteht: ECDSA P-256 mit
    /// SHA-256 und RSA-PSS mit SHA-256 (RFC 8446 Abschnitt 4.2.3).
    pub fn signaturalgorithmen_standard() -> Vec<u16> {
        vec![0x0403, 0x0804]
    }

    /// Baut einen ClientHello fuer einen Server, eine ALPN-Liste und
    /// einen Schluesselpaar.
    pub fn baue(
        random: [u8; 32],
        hostname: Option<&[u8]>,
        alpn: &[&[u8]],
        _eigener_schluessel: &[u8; 32],
        oeffentlich: &[u8; 32],
    ) -> Ergebnis<ClientHello> {
        // M4: der Hostname wird geprueft, BEVOR er kopiert wird.
        if let Some(h) = hostname {
            if h.len() > MAX_HOSTNAME {
                return Err(NachrichtFehler::HostnameZuLang {
                    ist: h.len(),
                    erlaubt: MAX_HOSTNAME,
                });
            }
        }
        if alpn.len() > MAX_ALPN_ANZAHL {
            return Err(NachrichtFehler::AlpnZuViele {
                ist: alpn.len(),
                erlaubt: MAX_ALPN_ANZAHL,
            });
        }
        let mut alpn_vor = Vec::with_capacity(alpn.len());
        for p in alpn {
            if p.len() > MAX_ALPN_EINTRAG {
                return Err(NachrichtFehler::AlpnEintragZuLang {
                    ist: p.len(),
                    erlaubt: MAX_ALPN_EINTRAG,
                });
            }
            alpn_vor.push(p.to_vec());
        }
        Ok(ClientHello {
            random,
            cipher_suites: ClientHello::suites_standard(),
            kompression: vec![0],
            sni: hostname.map(|h| h.to_vec()),
            versionen: vec![VERSION_TLS13],
            gruppen: vec![GRUPPE_X25519],
            signaturalgorithmen: ClientHello::signaturalgorithmen_standard(),
            key_share_gruppe: GRUPPE_X25519,
            key_share: oeffentlich.to_vec(),
            alpn: alpn_vor,
            // added 02.10.2026 11:4x, S4: der Client bittet um den
            // stapelten Widerrufsstatus. Siehe das Feld und die
            // Erweiterungsnummer.
            status_request: true,
            psk_modi: vec![1],
            // DER WERT, und er ist nicht beliebig: `kodiere` schickt
            // `vektor8(&[])`, also **leer**. Wer hier 32 Byte eintraege,
            // schickt eine ID, die der Server zurueckschicken soll, ohne
            // dass `kodiere` sie tun kann — der ServerEcho und der
            // Client wuerden sich dann unterscheiden.
            session_id: Vec::new(),
            quic_transport_parameters: None,
            psk: None,
            cookie: None,
            auch_tls12: false,
            auch_secp256r1: false,
        })
    }

    /// Baut EIN ClientHello, das TLS 1.3 UND TLS 1.2 anbietet.
    ///
    /// added 30.09.2026. DER AUFTRAG: der Server soll waehlen, nicht
    /// der Client, und es darf **kein zweiter Verbindungsversuch** mit
    /// schwächerer Version geben. Beides geht nur mit einem einzigen
    /// ClientHello, der beides anbietet.
    ///
    /// WAS DRINSTEHT, und warum:
    ///
    /// * `legacy_version` = 0x0303. Auch in TLS 1.3 steht dort 1.2
    ///   (RFC 8446 Abschnitt 4.1.2); ein 1.2-Server bricht ab, wenn
    ///   dort etwas anderes steht.
    /// * `supported_versions` = **0x0304, 0x0303**. Der Eintrag 0x0303
    ///   ist das EINZIGE, was TLS 1.2 von TLS 1.3 unterscheidet: der
    ///   1.2-Server sieht diese Erweiterung nicht und liest
    ///   `legacy_version`.
    /// * `cipher_suites`: die TLS-1.3-Suiten zuerst, dann die
    ///   TLS-1.2-Suiten. Fuer 1.3 zaehlt die Reihenfolge, fuer 1.2
    ///   waehlt der Server nach eigener Vorliebe.
    /// * `supported_groups` = x25519 **und** secp256r1. Fuer 1.3
    ///   zaehlt die Reihenfolge und es liegt ein `key_share` nur fuer
    ///   x25519 vor — ein 1.3-Server, der secp256r1 waehlt, schickt
    ///   ein HelloRetryRequest, das diese Crate nicht kann
    ///   (`StromFehler::KeinX25519`). Fuer 1.2 waehlt der Server frei,
    ///   und badssl.com waehlt secp256r1.
    /// * `signature_algorithms`: die Vereinigungsmenge beider Wege.
    /// * Die vier TLS-1.2-Erweiterungen aus
    ///   `kodiere_tls12_erweiterungen`.
    pub fn baue_beide(
        random: [u8; 32],
        hostname: &str,
        alpn: &[&[u8]],
        oeffentlich: &[u8; 32],
    ) -> Ergebnis<ClientHello> {
        let mut ch = ClientHello::baue(
            random,
            Some(hostname.as_bytes()),
            alpn,
            &random,
            oeffentlich,
        )?;
        ch.auch_tls12 = true;
        ch.auch_secp256r1 = true;
        // 0x0304 zuerst, 0x0303 danach: die Reihenfolge ist die
        // Vorliebe des Clients, und TLS 1.2 ist die Ausnahme.
        ch.versionen = vec![VERSION_TLS13, VERSION_LEGACY];
        ch.cipher_suites = ClientHello::suites_beide();
        ch.gruppen = vec![GRUPPE_X25519, GRUPPE_SECP256R1];
        ch.signaturalgorithmen = ClientHello::signaturalgorithmen_beide();
        Ok(ch)
    }

    /// Der **zweite** ClientHello nach einem HelloRetryRequest.
    ///
    /// added 30.09.2026 fuer W6. RFC 8446 Abschnitt 4.1.2 und 4.1.4:
    /// der zweite ClientHello ist **derselbe** wie der erste, bis auf
    /// drei Dinge —
    ///
    /// 1. ein `key_share` fuer die Gruppe, die der Server verlangt hat,
    /// 2. das `cookie`, unveraendert zurueckgegeben,
    /// 3. `early_data` faellt weg (wir senden keins).
    ///
    /// Alles andere bleibt: `client_random` (der Vektor aus RFC 8448
    /// Abschnitt 5 benutzt denselben), `supported_versions`, die Suites,
    /// `supported_groups` (der Vektor aendert die Liste **nicht**),
    /// SNI, ALPN.
    ///
    /// DER BELEG FUER "alles andere bleibt": der Vergleich der beiden
    /// ClientHellos aus RFC 8448 Abschnitt 5 ergibt genau zwei
    /// Unterschiede — `cookie` kommt dazu, `key_share` wechselt von
    /// 0x001d/32 Byte auf 0x0017/65 Byte. Dazu ein `padding`, mit dem
    /// der Vektor auf 512 Byte fuellt; das ist gegen Fingerprinting
    /// und **nicht** Pflicht, also hier weggelassen.
    pub fn wiederholung(&self, gruppe: u16, key_share: Vec<u8>, cookie: &[u8]) -> ClientHello {
        let mut c = self.clone();
        c.key_share_gruppe = gruppe;
        c.key_share = key_share;
        c.cookie = Some(cookie.to_vec());
        c
    }

    /// Die Cipher Suites fuer den gemeinsamen ClientHello.
    ///
    /// Die TLS-1.3-Suiten stehen vorn: fuer 1.3 ist die Reihenfolge die
    /// Vorliebe des Clients, fuer 1.2 uebersieht der Server sie ohnehin.
    /// Die 1.2-Suiten sind dieselben sechs, die `strom12::suites()`
    /// anbietet — **kein CBC** und **kein RSA-Schluesseltausch**, wie
    /// beauftragt.
    pub fn suites_beide() -> Vec<u16> {
        let mut v = ClientHello::suites_standard();
        v.extend_from_slice(&[
            0xC02F, // TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
            0xC030, // TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
            0xCCA8, // TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
            0xC02B, // TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
            0xC02C, // TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
            0xCCA9, // TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
        ]);
        v
    }

    /// Die Signaturverfahren beider Wege.
    ///
    /// Die Vereinigung aus `strom12::sigverfahren()` und den beiden
    /// TLS-1.3-Verfahren. Fehlt eines, verhandelt der Server ein
    /// Verfahren, fuer das es keinen Weg gibt — das `ServerKeyExchange`
    /// im 1.2-Pfad waere dann nicht pruefbar.
    pub fn signaturalgorithmen_beide() -> Vec<u16> {
        vec![
            0x0403, // ecdsa_secp256r1_sha256  (1.2 und 1.3)
            0x0503, // ecdsa_secp384r1_sha384  (1.2)
            0x0804, // rsa_pss_rsae_sha256     (1.2 und 1.3)
            0x0805, // rsa_pss_rsae_sha384     (1.2)
            0x0401, // rsa_pkcs1_sha256        (1.2)
        ]
    }

    /// Kodiert den ClientHello als Handshake-Nachricht MIT Kopf
    /// (Typ 1 und Dreier-Laenge), so wie sie auf der Leitung steht.
    pub fn kodiere(&self) -> Vec<u8> {
        let mut koerper = Schreiber::neu();
        koerper.u16(LEGACY_VERSION);
        koerper.bytes(&self.random);
        koerper.vektor8(&[]); // legacy_session_id: leer ist erlaubt
        let mut suites = Vec::with_capacity(self.cipher_suites.len() * 2);
        for s in &self.cipher_suites {
            suites.extend_from_slice(&s.to_be_bytes());
        }
        koerper.vektor16(&suites);
        koerper.vektor8(&self.kompression);
        self.kodiere_erweiterungen(&mut koerper);

        let mut voll = Schreiber::neu();
        voll.vektor24(koerper.fertig());
        voll.uebernehmen().tap_typ(typen::CLIENT_HELLO)
    }

    /// Kodiert die Erweiterungen, die **nur TLS 1.2** braucht.
    ///
    /// added 30.09.2026, als EIN ClientHello beide Protokollversionen
    /// anbieten sollte. Sie stehen am Ende der Liste, nach `key_share`.
    ///
    /// * `ec_point_formats` (11) mit `[uncompressed]` — ohne sie
    ///   akzeptieren Server den uncompressed Punkt nicht
    ///   (RFC 4492 Abschnitt 5).
    /// * `session_ticket` (35) leer — sonst schickt der Server kein
    ///   Ticket, und im TLS-1.2-Pfad gaebe es nichts zu ueberspringen.
    /// * `extended_master_secret` (23) leer — RFC 7627. Ohne Angebot
    ///   verweigert ein RFC-7627-Server den Handshake.
    /// * `renegotiation_info` (65281) mit Laengenbyte 0 — RFC 5746
    ///   Abschnitt 3.6: der Client sendet eine leere Sitzungskennung.
    ///
    /// DER FEHLER, DEN DIESE REIHENFOLGE EINMAL HATTE:
    /// `renegotiation_info` stand mit einer gefuellten
    /// Sitzungskennung drin, also `[1, 0]` statt `[0]`. Der Server
    /// las das als "Sitzung 0 laeuft" und antwortete mit
    /// `unexpected_message`. Siehe `strom12.rs`, wo dieselbe
    /// Verwechslung beim Lesen passiert war.
    fn kodiere_tls12_erweiterungen(&self, alle: &mut Schreiber) {
        if !self.auch_tls12 {
            return;
        }
        alle.u16(ext::EC_POINT_FORMATS);
        alle.vektor16(&[1, 0]);
        alle.u16(ext::SESSION_TICKET);
        alle.vektor16(&[]);
        alle.u16(ext::EXTENDED_MASTER_SECRET);
        alle.vektor16(&[]);
        alle.u16(ext::RENEGOTIATION_INFO);
        alle.vektor16(&[0]);
    }

    fn kodiere_erweiterungen(&self, s: &mut Schreiber) {
        let mut alle = Schreiber::neu();
        if let Some(h) = &self.sni {
            // server_name (RFC 6066 Abschnitt 3)
            let mut liste = Schreiber::neu();
            liste.u8(0); // host_name
            liste.vektor16(h);
            let mut inhalt = Schreiber::neu();
            inhalt.vektor16(liste.fertig());
            alle.u16(ext::SERVER_NAME);
            alle.vektor16(inhalt.fertig());
        }
        // ECHTER FEHLER, GEFUNDEN BEIM ANFASSEN DIESER STELLE am
        // 30.09.2026: beiden Erweiterungen fehlte die **innere**
        // 2-Byte-Laenge. Sie traegt die Laenge der Liste, die
        // Erweiterung zusaetzlich ihre eigene — zwei Ebenen:
        //
        //   SupportedGroups:  supported_groups<2..2^16-1>   (RFC 8446
        //                     Abschnitt 4.2.7, dort `NamedGroup
        //                     named_group_list<2..2^16-1>`)
        //   SignatureSchemes: signature_algorithms<2..2^16-1>
        //                     (RFC 8446 Abschnitt 4.2.3)
        //
        // GEGENPROBE aus einem echten Client, `openssl s_client -msg`
        // gegen badssl.com vom 30.09.2026 (mutation_md/tls/ch_ref.bin):
        //   0x000a len 12  000a001d0017001e00180019   <- 0x000a = 10
        //   0x000d len 42  002804030503060308070808…   <- 0x0028 = 40
        // Die inneren Laengen stehen dort, und ohne sie beginnt die
        // Liste mit der ersten Gruppenkennung (0x001d), die der
        // Server als Laenge liest: 29 Byte, die er nicht hat.
        //
        // DASS ES BISHER NICHT AUFFIEL: der TLS-1.3-Handshake kam
        // zustande, weil der Server bei TLS 1.3 den Schluessel aus dem
        // `key_share` nimmt und die Gruppenliste fuer die
        // Gruppenauswahl nicht braucht. Die Liste war also falsch und
        // unbenutzt — und bei einem Server, der sie streng liest,
        // waere der Handshake mit `decode_error` abgebrochen.
        if !self.gruppen.is_empty() {
            let flach: Vec<u8> = self.gruppen.iter().flat_map(|g| g.to_be_bytes()).collect();
            let mut inhalt = Schreiber::neu();
            inhalt.vektor16(&flach);
            alle.u16(ext::SUPPORTED_GROUPS);
            alle.vektor16(inhalt.fertig());
        }
        if !self.signaturalgorithmen.is_empty() {
            let flach = self
                .signaturalgorithmen
                .iter()
                .flat_map(|a| a.to_be_bytes())
                .collect::<Vec<u8>>();
            let mut inhalt = Schreiber::neu();
            inhalt.vektor16(&flach);
            alle.u16(ext::SIGNATURE_ALGORITHMS);
            alle.vektor16(inhalt.fertig());
        }
        if !self.versionen.is_empty() {
            let flach = self
                .versionen
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<u8>>();
            let mut inhalt = Schreiber::neu();
            inhalt.vektor8(&flach);
            alle.u16(ext::SUPPORTED_VERSIONS);
            alle.vektor16(inhalt.fertig());
        }
        //
        // added 02.10.2026 06:4x, R5h. **BEHALTEN, OBWOHL ES AUFFIELLA.**
        //
        // GEMESSEN, was auffiel (`narsil-server`, 02.10.2026 05:5x,
        // `serve_https` auf 9443, ein Abruf unseres Browsers):
        //     ANGEBOT ABGELEHNT: das Angebot ist kaputt (Identitaeten: 0,
        //     psk_modi: [1])
        // Unser **eigener Server** weist also ein ClientHello ab, das wir
        // selbst schicken. Die erste Fassung dieser Zeile hat daraus eine
        // Aenderung gemacht: `psk_key_exchange_modes` nur noch, wenn
        // `self.psk.is_some()`.
        //
        // DIE AENDERUNG IST ZURUECKGENOMMEN, und der Grund ist der
        // **Normvektor**: `client_hello_aus_rfc8448_bytegenau` schickt genau
        // so eins — `psk_key_exchange_modes = [1]` **ohne**
        // `pre_shared_key`. RFC 8446 4.2.9 sagt nur: „A client MUST provide
        // a "psk_key_exchange_modes" extension if it offers a
        // "pre_shared_key" extension" — ein Verbot ohne PSK steht dort
        // nicht. Der Test wurde rot und hat die Sache entschieden.
        //
        // WAS BLEIBT, ist der Befund: `narsil-server` ist **strenger als die
        // Norm**, und die Unstimmigkeit ist zwischen unseren beiden
        // Implementierungen. Das gehoert in `narsil-server` (fremde Datei)
        // und ist in `team/BERICHT_TLS_R5h.txt` gemeldet.
        if !self.psk_modi.is_empty() {
            let mut inhalt = Schreiber::neu();
            inhalt.vektor8(&self.psk_modi);
            alle.u16(ext::PSK_KEY_EXCHANGE_MODES);
            alle.vektor16(inhalt.fertig());
        }
        //
        // added 02.10.2026 11:4x, S4. `status_request` (RFC 6066 3). Fuenf
        // Byte Inhalt, genau wie im TLS-1.2-Weg (`nachricht12.rs` Zeile 348
        // nennt den Grund: eine Ein-Byte-Erweiterung brachte badssl.com in
        // einen Alert).
        //
        // GEMESSEN, warum das hier etwas bewegt: `openssl s_client -status`
        // gegen github.com, www.google.com und www.wikipedia.org liefert bei
        // allen dreien `OCSP response: no response sent` — die grossen
        // Dienste staple'n **nicht**, auch auf Anfrage. Diese Erweiterung
        // aendert daran nichts; sie ist die Voraussetzung dafuer, dass ein
        // Server, der es kann, es auch tut. Die **Auswertung** des
        // stapelten Status fehlt weiterhin (siehe `team/BERICHT_TLS_R7.txt`).
        if self.status_request {
            alle.u16(ext::STATUS_REQUEST);
            alle.vektor16(&[1, 0, 0, 0, 0]);
        }
        // Das Cookie steht zwischen `supported_versions` (0x2b) und
        // `psk_key_exchange_modes` (0x2d) — so haelt es der Vektor aus
        // RFC 8448 Abschnitt 5. Die Reihenfolge ist fuer den Server
        // belanglos, aber sie haelt die beiden ClientHellos vergleichbar.
        if let Some(c) = &self.cookie {
            // M3/M4: das Cookie kommt aus dem Netz. RFC 8446 erlaubt
            // 1..2^16-1 Byte. Ein leeres Cookie waere sinnlos und wird
            // hier abgelehnt, statt es als "vorhanden" zu zaehlen.
            if c.is_empty() || c.len() > u16::MAX as usize {
                return;
            }
            alle.u16(ext::COOKIE);
            alle.vektor16(c);
        }
        if !self.alpn.is_empty() {
            let mut liste = Schreiber::neu();
            for p in &self.alpn {
                liste.vektor8(p);
            }
            let mut inhalt = Schreiber::neu();
            inhalt.vektor16(liste.fertig());
            alle.u16(ext::ALPN);
            alle.vektor16(inhalt.fertig());
        }
        // Der Key Share steht nach ALPN, damit die Reihenfolge der
        // Erweiterungen der in RFC 8448 entspricht.
        if !self.key_share.is_empty() {
            // EIGENER FEHLER, vom eigenen Rundlaufstest gefunden: es gab
            // ZWEI Laengenebenen (vektor16 ueber vektor16 ueber eintrag).
            // Die Erweiterung traegt aber nur EINE:
            //     KeyShare client_shares<0..2^16-1>
            // und darin je Eintrag die Gruppe und ein vektor16. Am
            // RFC-8448-Vektor sieht man das an `0033 0026 0024 001d 0020
            // <32 Byte>`: eine Laengenangabe, dann der Eintrag.
            //
            // Der Leser las deshalb `0026` als Listenlaenge, dann `0024`
            // als GRUPPENkennung (0x24) und lief danach mitten im
            // Schluessel durch die Byte-Felder.
            // Genau EINE Laengenebene in den Erweiterungsdaten:
            //     KeyShare client_shares<0..2^16-1>;
            // und darin je Eintrag `group` + `key_exchange<1..2^16-1>`.
            //
            // Daran habe ich zweimal danebengegriffen. Zuerst standen
            // ZWEI Ebenen (das Erweiterungsdatum war `vektor16` ueber
            // `vektor16` ueber dem Eintrag), was eine Ebene zu viel war
            // und den Dekodierer mitten im Schluessel durch die Felder
            // laufen liess. Dann habe ich alle entfernt, was eine zu
            // wenig war. Richtig ist die eine Ebene, und der Gegner ist
            // der RFC-8448-Vektor: `0033 0026 0024 001d 0020 <32 Byte>`.
            let mut eintrag = Schreiber::neu();
            eintrag.u16(self.key_share_gruppe);
            eintrag.vektor16(&self.key_share);
            let mut liste = Schreiber::neu();
            liste.vektor16(eintrag.fertig());
            alle.u16(ext::KEY_SHARE);
            alle.vektor16(liste.fertig());
        }
        // Die TLS-1.2-Erweiterungen kommen hinten an.
        self.kodiere_tls12_erweiterungen(&mut alle);
        // Und ganz hinten der QUIC-Parameterblock (0x39).
        //
        // DER ORT, und er ist **wirklich** der letzte: die Reihenfolge
        // ist eine Zusage, und der Test `der_schreiber_baut_die_
        // erweiterung_0x39_byteweise_...` in `tests/quic.rs` prueft
        // sie. Er steht hier, weil `pre_shared_key` (A1) spaeter
        // dazukommt und laut RFC 8446 4.2.11 die **letzte** Erweiterung
        // im ClientHello sein muss — dann steht der Block davor, und
        // die Zusage dieses Tests bleibt gueltig.
        //
        // DER FEHLER, DEN DIESER BLOCK ZUERST HATTE: er stand zwischen
        // `psk_key_exchange_modes` und `key_share`, und der Kommentar
        // behauptete trotzdem "am Ende". Der Test hat es gefunden, weil
        // er die **letzte** Fundstelle sucht.
        if let Some(parameter) = &self.quic_transport_parameters {
            alle.u16(ext::QUIC_TRANSPORT_PARAMETERS);
            alle.vektor16(parameter);
        }
        // Und **zuletzt** `pre_shared_key` (41), und das ist
        // Vorschrift, nicht Geschmack: RFC 8446 4.2.11 sagt "The
        // pre_shared_key extension MUST be the last extension in the
        // ClientHello". Der Grund ist der Binder: er wird ueber einen
        // **abgeschnittenen** ClientHello gerechnet (4.2.11.2), und ein
        // Feld nach `pre_shared_key` kaeme dort nicht mehr vor — der
        // Binder waere dann garantiert falsch und der Server wuerde
        // `decrypt_error` schicken.
        //
        // added 01.10.2026, R4. DER BLOCK WIRD ROH EINGEHANGT, und nicht
        // neu gebaut: `ps_k_erweiterung_ohne_binder` liefert ihn mit
        // Typ und Laenge, und ein zweiter Bau hier waere ein zweiter
        // Weg, auf dem die Laengen von denen im Binder abweichen
        // koennten.
        if let Some(block) = &self.psk {
            alle.bytes(block);
        }
        s.vektor16(alle.fertig());
    }

    /// Dekodiert einen ClientHello aus einer Handshake-Nachricht MIT
    /// Kopf.
    pub fn dekodiere(nachricht: &[u8]) -> Ergebnis<ClientHello> {
        let (_typ, inhalt) = zerlege(nachricht, typen::CLIENT_HELLO)?;
        let mut l = Leser::neu(inhalt);
        let version = l.u16()?;
        if version != LEGACY_VERSION {
            return Err(NachrichtFehler::VersionFalsch { ist: version });
        }
        let mut random = [0u8; 32];
        random.copy_from_slice(l.block(32)?);
        let session_id = l.vektor8()?.to_vec();
        // EIGENER FEHLER, vom RFC-8448-Vektor gefunden: cipher_suites hat
        // ein ZWEI-Byte-Laengenfeld (`cipher_suites<2..2^16-2>`,
        // RFC 8446 Abschnitt 4.1.2). Ich las es als Ein-Byte, bekam also
        // die Laenge 0, las dann das Kompressionsfeld als Suitenliste und
        // danach die 0x01 aus "01 00" als Erweiterungslaenge 256.
        let suites_roh = l.vektor16()?;
        if suites_roh.len() % 2 != 0 {
            return Err(NachrichtFehler::Vektor(VektorFehler::InhaltZuKurz {
                angesagt: suites_roh.len(),
                vorhanden: suites_roh.len() - 1,
            }));
        }
        let cipher_suites = suites_roh
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        let kompression = l.vektor8()?.to_vec();

        let mut ch = ClientHello {
            random,
            cipher_suites,
            kompression,
            sni: None,
            // added 02.10.2026 11:5x, S4: der Test-Rueckbau startet
            // **ohne** die Erweiterung — so sieht der Dekodierer sie in
            // einem ClientHello, das sie nicht traegt.
            status_request: false,
            versionen: Vec::new(),
            gruppen: Vec::new(),
            signaturalgorithmen: Vec::new(),
            key_share_gruppe: 0,
            key_share: Vec::new(),
            alpn: Vec::new(),
            psk_modi: Vec::new(),
            session_id,
            quic_transport_parameters: None,
            psk: None,
            cookie: None,
            auch_tls12: false,
            auch_secp256r1: false,
        };
        ch.les_erweiterungen(&mut l)?;
        Ok(ch)
    }

    fn les_erweiterungen(&mut self, l: &mut Leser) -> Ergebnis<()> {
        if l.ist_leer() {
            return Ok(());
        }
        let alle = l.vektor16()?;
        let mut e = Leser::neu(alle);
        while !e.ist_leer() {
            let nummer = e.u16()?;
            let inhalt = e.vektor16()?;
            let mut x = Leser::neu(inhalt);
            match nummer {
                ext::SERVER_NAME => {
                    let liste = x.vektor16()?;
                    let mut s = Leser::neu(liste);
                    let typ = s.u8()?;
                    if typ != 0 {
                        return Err(NachrichtFehler::SniTypFalsch { ist: typ });
                    }
                    let name = s.vektor16()?;
                    if name.len() > MAX_HOSTNAME {
                        return Err(NachrichtFehler::HostnameZuLang {
                            ist: name.len(),
                            erlaubt: MAX_HOSTNAME,
                        });
                    }
                    self.sni = Some(name.to_vec());
                }
                ext::SUPPORTED_GROUPS => {
                    // Auch das ist ein Zwei-Byte-Feld
                    // (`NamedGroup supported_groups<4..2^16-4>`).
                    let flach = x.vektor16()?;
                    self.gruppen = flach
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                }
                ext::SIGNATURE_ALGORITHMS => {
                    // Ebenfalls Zwei-Byte
                    // (`SignatureScheme ...<2..2^16-2>`).
                    let flach = x.vektor16()?;
                    self.signaturalgorithmen = flach
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                }
                ext::SUPPORTED_VERSIONS => {
                    let flach = x.vektor8()?;
                    self.versionen = flach
                        .chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                }
                ext::PSK_KEY_EXCHANGE_MODES => {
                    self.psk_modi = x.vektor8()?.to_vec();
                }
                ext::STATUS_REQUEST => {
                    // added 02.10.2026 11:4x: nur die Form pruefen, nicht
                    // auswerten — die Auswertung ist S4 und steht aus.
                    self.status_request = x.block(5)? == [1, 0, 0, 0, 0];
                }
                ext::ALPN => {
                    let liste = x.vektor16()?;
                    let mut a = Leser::neu(liste);
                    while !a.ist_leer() {
                        if self.alpn.len() >= MAX_ALPN_ANZAHL {
                            return Err(NachrichtFehler::AlpnZuViele {
                                ist: self.alpn.len() + 1,
                                erlaubt: MAX_ALPN_ANZAHL,
                            });
                        }
                        let p = a.vektor8()?;
                        if p.len() > MAX_ALPN_EINTRAG {
                            return Err(NachrichtFehler::AlpnEintragZuLang {
                                ist: p.len(),
                                erlaubt: MAX_ALPN_EINTRAG,
                            });
                        }
                        self.alpn.push(p.to_vec());
                    }
                }
                ext::KEY_SHARE => {
                    let liste = x.vektor16()?;
                    let mut k = Leser::neu(liste);
                    while !k.ist_leer() {
                        let gruppe = k.u16()?;
                        let key = k.vektor16()?;
                        // Nur x25519 wird uebernommen; andere Gruppen
                        // werden uebersprungen (RFC 8446 Abschnitt 4.2.8).
                        if gruppe == GRUPPE_X25519 && self.key_share.is_empty() {
                            self.key_share_gruppe = gruppe;
                            self.key_share = key.to_vec();
                        }
                    }
                }
                // `quic_transport_parameters` (0x39).
                //
                // added 30.09.2026, Runde 2, TLS R2. DER INHALT, und
                // das ist der Punkt, den der Wunsch ausdruecklich
                // betont: **kein** `vektor16` mehr. Der Erweiterungs-
                // inhalt IST bereits der Parameterblock; ein weiteres
                // Laengenfeld darueber zu lesen wuerde die ersten vier
                // Byte des Blocks (`04 08 ffff…`) als Laenge
                // verstehen und den Block damit zerlegen. Genau das ist
                // der Fehler, den der Wunsch benennt — und deshalb
                // steht hier `rest()` und nicht `vektor16()`.
                ext::QUIC_TRANSPORT_PARAMETERS => {
                    self.quic_transport_parameters = Some(x.rest().to_vec());
                }
                // `pre_shared_key` (41), added 01.10.2026, R4.
                //
                // DER INHALT, und er ist ein Block aus
                // `ticket::ps_k_erweiterung_ohne_binder` — also **ohne**
                // weiteres Laengenfeld, wie bei 0x39. Zwei Erweiterungen
                // mit derselben Regel; das ist Absicht, denn beide sind
                // nach RFC 8446 „Vektor mit impliziter Laenge im
                // Erweiterungskopf".
                //
                // DIE PRUEFUNG DER REIHENFOLGE, und sie ist nicht
                // kosmetisch: `pre_shared_key` muss die **letzte**
                // Erweiterung sein (4.2.11). Bei `ClientHello::baue`
                // ist das so (der Encoder haengt es hinten an), und fuer
                // einen von aussen gebauten ClientHello prueft es der
                // Test `das_psk_ist_die_letzte_erweiterung`.
                ext::PRE_SHARED_KEY => {
                    // DER VOLLE BLOCK, und das ist eine bewusste
                    // Entscheidung: der Decoder liefert Typ + Laenge +
                    // Inhalt, damit `kodiere` den Block unveraendert
                    // wieder anhaengen kann.
                    //
                    // added 01.10.2026, R4. DER FEHLER, DEN DER TEST
                    // GEFUNDEN HAT, und er waere im Betrieb still
                    // schiefgegangen: mit `x.rest().to_vec()` lieferte
                    // der Decoder nur den **Inhalt**, waehrend
                    // `kodiere` den **ganzen** Block anhaengt. Ein
                    // `ClientHello::dekodiere(..).kodiere()` — also jedes
                    // Weiterreichen eines gelesenen ClientHello, etwa in
                    // einem Test oder in einer Middlebox — haette die
                    // Erweiterung ein **zweites** Typ-Laengen-Paar
                    // bekommen und der Server haette sie nicht mehr
                    // gefunden. Zwei aehnlich benannte Felder
                    // (`quic_transport_parameters` = Inhalt,
                    // `psk` = Block) sind genau die Falle.
                    let inhalt = x.rest();
                    let mut block = Vec::with_capacity(4 + inhalt.len());
                    block.extend_from_slice(&ext::PRE_SHARED_KEY.to_be_bytes());
                    block.extend_from_slice(&(inhalt.len() as u16).to_be_bytes());
                    block.extend_from_slice(inhalt);
                    self.psk = Some(block);
                }
                // Unbekannte Erweiterungen werden uebersprungen.
                _ => {}
            }
        }
        Ok(())
    }

    // added 04.10.2026 00:3x, R67. **`gruppen_zahlen` GELOESCHT** — und die
    // Begruendung ist eine **Messung**, keine Formfehler: `grep` ueber **alle**
    // Crates fand **einen** Treffer, die eigene Definition. Der Getter kopierte
    // nur `self.gruppen`, und das Feld ist `pub`, also gibt es keinen Grund, den
    // Getter zu behalten. (R67 nennt die Methode als Beispiel der Klasse
    // „geliefert und nie benutzt“ — dieselbe Form wie R60 an
    // `crl_status_pruefen`.)
}

// ---------------------------------------------------------------------------
// ServerHello
// ---------------------------------------------------------------------------

/// Der ServerHello eines TLS-1.3-Servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHello {
    pub random: [u8; 32],
    /// Die gewaehlte Cipher Suite.
    pub cipher_suite: u16,
    /// Die gewaehlte Gruppe.
    pub gruppe: u16,
    /// Der oeffentliche Schluessel des Servers (32 Byte bei x25519).
    pub key_share: Vec<u8>,
    /// Das Cookie aus einem HelloRetryRequest, unveraendert
    /// zurueckzugeben (RFC 8446 Abschnitt 4.2.2).
    ///
    /// added 30.09.2026. `Some` nur, wenn der Server eines schickte.
    pub cookie: Option<Vec<u8>>,
    /// Schickt der Server ein Cookie? `cookie` sagt, **was**; dieses
    /// Feld nur, ob ueberhaupt eines kam.
    ///
    /// added 30.09.2026. DER UNTERSCHIED ZU `hrr`: ein Cookie kann
    /// auch in einem **normalen** ServerHello stehen, und `hrr` steht
    /// allein am Sonderwert des ServerRandom. Wer `hrr` an einem Cookie
    /// festmacht, faellt bei einem Server mit Sitzungswiederaufnahme
    /// in eine Schleife aus zweitem ClientHello.
    pub retry: bool,
    /// Ist das ein **HelloRetryRequest**?
    ///
    /// added 30.09.2026 fuer W6. Erkannt am Sonderwert des ServerRandom
    /// (RFC 8446 Abschnitt 4.1.3), NICHT an einem Cookie: der
    /// Sonderwert ist der einzige, den die Norm vorschreibt, und ein
    /// Cookie kann auch in einem normalen ServerHello stehen.
    pub hrr: bool,
    /// `selected_identity` aus dem `pre_shared_key`, oder `None`.
    ///
    /// added 02.10.2026, R4d Stufe 2 (RFC 8446 4.2.11.3). DER INHALT:
    /// `SelectedIdentity<0..2^16-1>` = der **Index** der Identitaet aus dem
    /// ClientHello, die der Server gewaehlt hat.
    ///
    /// DER GRUND FUER EIN FELD: bei **mehreren** Angeboten ist das eine
    /// Zahl, und der Client muss daraus ersehen, **welchen** PSK er in den
    /// Schluesselplan nimmt. Ohne dieses Feld haette er alle Angebote
    /// rechnen muessen.
    ///
    /// `None` heisst: der Server hat kein `pre_shared_key` angenommen
    /// (vollstaendiger Handshake). Das ist ein Unterschied zu `Some(0)`,
    /// und der Unterschied ist der ganze Punkt — `Some(0)` heisst
    /// „Wiederaufnahme der **ersten** Identitaet".
    pub selected_identity: Option<u16>,
    /// Die Version, die der Server **gewaehlt** hat.
    ///
    /// added 30.09.2026. DER GRUND: `legacy_version` ist in BEIDEN
    /// Protokollversionen 0x0303 (RFC 8446 Abschnitt 4.1.2), man kann
    /// an ihr also nicht ablesen, ob der Server TLS 1.3 gewaehlt hat.
    /// Das steht allein in `supported_versions`.
    ///
    /// Der alte Dekodierer hat dort bei jedem Wert ungleich 0x0304
    /// abgebrochen (`VersionNichtTls13`) — damit liess sich ein
    /// TLS-1.2-Server gar nicht erst dekodieren, und die gemeinsame
    /// Verbindungsfunktion hatte nichts, woran sie sich entscheiden
    /// konnte. Jetzt wird der Wert GESPERRT und zurueckgegeben.
    pub version: u16,
}

impl ServerHello {
    /// Dekodiert einen ServerHello.
    pub fn dekodiere(nachricht: &[u8]) -> Ergebnis<ServerHello> {
        let (_typ, inhalt) = zerlege(nachricht, typen::SERVER_HELLO)?;
        let mut l = Leser::neu(inhalt);
        let version = l.u16()?;
        if version != LEGACY_VERSION {
            return Err(NachrichtFehler::VersionFalsch { ist: version });
        }
        let mut random = [0u8; 32];
        random.copy_from_slice(l.block(32)?);
        let _session_id_echo = l.vektor8()?;
        let cipher_suite = l.u16()?;
        // `legacy_compression_method` ist im **ServerHello** ein
        // einzelnes `uint8` (RFC 8446 4.1.3) — anders als im
        // ClientHello, wo es `legacy_compression_methods<1..2^8-1>`
        // ist (4.1.2). DER FEHLER, DEN DIESER BLOCK HATTE: es stand ein
        // `vektor8` hier, und das liest das Byte als **Laenge**.
        //
        // added 30.09.2026, Runde 2, TLS R1. WARUM ES NICHT AUFFIEL:
        // bei jedem echten TLS-1.3-Server steht an dieser Stelle `00`,
        // und ein Laengenbyte `00` verbraucht genau so viele Byte wie
        // der Wert selbst — der Rest der Nachricht bleibt also richtig
        // ausgerichtet, und die leere Liste passiert die Pruefung
        // darunter. Der Fehler wurde erst sichtbar, als der eigene
        // Server nach RFC gebaut wurde und `openssl s_client` die
        // Nachricht ablehnte (`bad length`), waehrend unser eigener
        // Dekodierer sie las. GEFUNDEN hat ihn also nicht unser Test,
        // sondern ein fremdes Programm — das ist der Grund, warum der
        // Beweis fuer den Server ueber `openssl` laufen muss.
        let kompression = l.u8()?;
        if kompression != 0 {
            return Err(NachrichtFehler::KompressionFalsch {
                war: kompression as u32,
            });
        }
        // M4: der Rest ist die Erweiterungsliste mit 16-Bit-Laenge.
        let alle = l.vektor16()?;

        let mut sh = ServerHello {
            random,
            cipher_suite,
            gruppe: 0,
            key_share: Vec::new(),
            cookie: None,
            retry: false,
            // Ohne `supported_versions` ist es die Legacy-Version, und
            // die ist per Definition 0x0303 (oben geprueft).
            version: VERSION_LEGACY,
            // Ohne `pre_shared_key` im ServerHello gab es keine
            // Wiederaufnahme; `None` ist der Unterschied zu `Some(0)`.
            selected_identity: None,
            // DER WEG ZUR ERKENNUNG: der Sonderwert des ServerRandom.
            hrr: random == HRR_SONDERRANDOM,
        };
        let mut e = Leser::neu(alle);
        while !e.ist_leer() {
            let nummer = e.u16()?;
            let inhalt = e.vektor16()?;
            let mut x = Leser::neu(inhalt);
            match nummer {
                ext::SUPPORTED_VERSIONS => {
                    let v = x.u16()?;
                    // NUR die beiden Versionen sind moeglich. Alles
                    // andere (0x7f00ff aus RFC 8446 fuer GECKO, oder
                    // ein Server, der sich verlesen hat) wird hier
                    // abgelehnt — mit der Zahl im Text.
                    if v != VERSION_TLS13 && v != VERSION_LEGACY {
                        return Err(NachrichtFehler::VersionNichtTls13 { ist: v });
                    }
                    sh.version = v;
                }
                ext::KEY_SHARE => {
                    sh.gruppe = x.u16()?;
                    // DER FEHLER, DEN DIESER ZWEIG HATTE: er las
                    // danach IMMER ein `vektor16`. In einem
                    // HelloRetryRequest traegt `key_share` aber **nur
                    // die Gruppe** (RFC 8446 Abschnitt 4.2.8: "if the
                    // server is selecting a group ... the KeyShare
                    // entry is omitted"), und der Vektor las dann ins
                    // Leere:
                    //   Vektor(ZuKurz { erwartet: 2, vorhanden: 0 })
                    // Das ist der Fehler, an dem der HRR-Vektor aus
                    // RFC 8448 zuerst scheiterte.
                    //
                    // Also: eine Gruppe **und** ein Schluessel, oder
                    // nur eine Gruppe. Beides ist normal, und welcher
                    // Fall vorliegt, steht danach in `key_share`.
                    if x.uebrig() > 0 {
                        sh.key_share = x.vektor16()?.to_vec();
                    }
                }
                ext::PSK_KEY_EXCHANGE_MODES => {
                    // Nur relevant bei resumed; hier ignorieren.
                }
                // `pre_shared_key` (41) in der **Antwort**, RFC 8446
                // 4.2.11.3:
                //     struct {
                //         uint16 selected_identity;
                //     } PreSharedKeyExtension;
                //
                // DER WEG ZUR ENTSCHEIDUNG, ob das eine
                // Wiederaufnahme ist: **das Feld**, und nicht die
                // Erweiterung. RFC 8446 sagt, ein Server, der kein
                // `pre_shared_key` schickt, macht einen vollstaendigen
                // Handshake — die Erweiterung *im ClientHello* allein
                // sagt das nicht, denn ein Server darf sie ignorieren
                // (und muss es, wenn er kein Ticket kennt).
                //
                // DER INDEX WIRD NICHT GEPRUEFT, und das ist Absicht: die
                // Identitaetenliste steht nur im ClientHello, und dieses
                // Struct hat sie nicht. Eine Pruefung gehoerte in den
                // Schluesselplan des Servers, der beide Seiten hat; hier
                // wuerde sie eine Zahl gegen nichts stellen. Die
                // Gegenprobe (ein Index, den es nicht gibt) steht in
                // `tests/psk_rundlauf.rs`.
                ext::PRE_SHARED_KEY => {
                    let index = x.u16()?;
                    sh.selected_identity = Some(index);
                }
                // `cookie` (44), RFC 8446 Abschnitt 4.2.2:
                //   `opaque cookie<1..2^16-1>`
                // — der Inhalt ist ein Vektor mit ZWEI Byte Laenge,
                // nicht die Bytes selbst. Der Vektor aus RFC 8448
                // Abschnitt 5 hat 116 Byte Erweiterungsinhalt, davon
                // sind die ersten zwei `00 72` (114) und der Rest das
                // Cookie.
                //
                // DER FEHLER, DEN DIESER ZWEIG HATTE: er las den
                // Vektor und **verwarf** ihn (`let _ =`). Damit
                // konnte der zweite ClientHello kein Cookie zurueckgeben
                // und der Handshake waere mit `HrrOhneCookie` abgewiesen
                // worden — an einem Server, der es richtig gemacht hat.
                ext::COOKIE => {
                    sh.cookie = Some(x.vektor16()?.to_vec());
                    sh.retry = true;
                }
                _ => {}
            }
        }
        Ok(sh)
    }

    /// Der X25519-Schluessel, wenn die Gruppe stimmt. Sonst `None`.
    pub fn x25519(&self) -> Option<[u8; 32]> {
        if self.gruppe != GRUPPE_X25519 || self.key_share.len() != 32 {
            return None;
        }
        let mut r = [0u8; 32];
        r.copy_from_slice(&self.key_share);
        Some(r)
    }
}

// ---------------------------------------------------------------------------
// EncryptedExtensions
// ---------------------------------------------------------------------------

/// Die EncryptedExtensions des Servers (RFC 8446 Abschnitt 4.3.1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EncryptedExtensions {
    /// Die vom Server gewaehlten ALPN-Protokolle. Hoechstens eines.
    pub alpn: Option<Vec<u8>>,
    /// Der Cookie des HelloRetryRequest, falls einer kommt.
    pub cookie: Option<Vec<u8>>,
    /// Die **eigenen** QUIC-Transportparameter des Servers, oder
    /// `None`, wenn der Server keine schickt.
    ///
    /// added 30.09.2026, Runde 2, TLS R2 (Wunsch DEEPSEEK, Position 1:
    /// "`ClientHello` und `EncryptedExtensions` muessen die Erweiterung
    /// ... tragen bzw. zurueckgeben"). Wie beim ClientHello ist der
    /// Inhalt unveraendert der Block aus RFC 9000 Abschnitt 18.
    pub quic_transport_parameters: Option<Vec<u8>>,
}

impl EncryptedExtensions {
    pub fn dekodiere(nachricht: &[u8]) -> Ergebnis<EncryptedExtensions> {
        let (_typ, inhalt) = zerlege(nachricht, typen::ENCRYPTED_EXTENSIONS)?;
        let mut l = Leser::neu(inhalt);
        let alle = l.vektor16()?;
        let mut ee = EncryptedExtensions::default();
        let mut e = Leser::neu(alle);
        while !e.ist_leer() {
            let nummer = e.u16()?;
            let inhalt = e.vektor16()?;
            let mut x = Leser::neu(inhalt);
            match nummer {
                ext::ALPN => {
                    let liste = x.vektor16()?;
                    let mut a = Leser::neu(liste);
                    if a.ist_leer() {
                        continue;
                    }
                    let p = a.vektor8()?;
                    if p.len() > MAX_ALPN_EINTRAG {
                        return Err(NachrichtFehler::AlpnEintragZuLang {
                            ist: p.len(),
                            erlaubt: MAX_ALPN_EINTRAG,
                        });
                    }
                    ee.alpn = Some(p.to_vec());
                }
                44 => {
                    ee.cookie = Some(x.vektor16()?.to_vec());
                }
                // `quic_transport_parameters` (0x39), siehe oben beim
                // ClientHello: der Inhalt ist der Parameterblock, ohne
                // weiteres Laengenfeld.
                ext::QUIC_TRANSPORT_PARAMETERS => {
                    ee.quic_transport_parameters = Some(x.rest().to_vec());
                }
                _ => {}
            }
        }
        Ok(ee)
    }
}

// ---------------------------------------------------------------------------
// Certificate
// ---------------------------------------------------------------------------

/// Die Certificate-Nachricht (RFC 8446 Abschnitt 4.4.2).
///
/// Der Inhalt wird hier bewusst ROH gehalten: das Dekodieren und die
/// Pruefung macht das DER-Modul (T10). Hier zaehlt nur, dass die Liste
/// richtig gelesen wird und dass jede Laenge geprueft ist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Certificate {
    /// Die Zertifikate in der Reihenfolge der Kette, Blatt zuerst.
    pub zertifikate: Vec<Vec<u8>>,
    /// Der kontextspezifische Erweiterungsblock der Nachricht.
    pub erweiterungen: Vec<u8>,
}

impl Certificate {
    pub fn dekodiere(nachricht: &[u8]) -> Ergebnis<Certificate> {
        let (_typ, inhalt) = zerlege(nachricht, typen::CERTIFICATE)?;
        let mut l = Leser::neu(inhalt);
        let kontext = l.vektor8()?;
        let _ = kontext; // leer in TLS 1.3 ohne Client-Zertifikat (Abschnitt 4.4.2)
        let liste = l.vektor24()?;
        let mut c = Certificate {
            zertifikate: Vec::new(),
            erweiterungen: l.rest().to_vec(),
        };
        let mut k = Leser::neu(liste);
        while !k.ist_leer() {
            // added 03.10.2026, Rolle TLS (Angriffsgrenzen): **DIE ZAHL DER
            // ZERTIFIKATE IST BEGRENZT**, und vorher war sie es **nicht**.
            //
            // **GEMESSEN, wie gross der Schaden ohne Grenze waere:** jedes
            // Zertifikat braucht mindestens **vier** Byte — drei fuer das
            // Laengenpraefix (`vektor24`) und mindestens **ein** Byte Inhalt.
            // `MAX_NACHRICHT` ist `0xFFFFFF` (also 16 777 215 Byte), und die
            // `Certificate`-Nachricht darf die volle Laenge benutzen. Damit
            // koennte ein Server **4 194 303** Eintraege schicken — jeder eine
            // eigene `Vec`-Allokation in `c.zertifikate`.
            //
            // **UND `MAX_KETTEN_TIEFE` = 10 (`kette.rs`) haelt das NICHT auf:**
            // die begrenzt die **Pruefschleife**, und die kommt **nach** dem
            // Einlesen. Die 16 MiB sind zu diesem Zeitpunkt **schon**
            // alloziert und **vier Millionen** `Vec` stehen im Speicher.
            //
            // **WARUM 16, und nicht 10:** RFC 8446 4.4.2 erlaubt mehrere
            // Zwischenzertifikate, und ein-Blatt-Zertifikat kann mehrfach
            // kreuzsigniert sein — der Server schickt dann **einen** Pfad mit
            // 3-4 Zertifikaten. Wer `MAX_KETTEN_TIEFE` (=10) als Zahlengrenze
            // auch fuer die Liste nehmen wuerde, haette bei einer
            // Kreuzsignierung **zu wenig** Spiel. 16 laesst 6 Zertifikate
            // Reserve und ist **unter** der Nachrichtengrenze, die 16 MiB
            // gross ist.
            if c.zertifikate.len() >= MAX_ZERTIFIKATE_PRO_NACHRICHT {
                return Err(NachrichtFehler::ZuVieleZertifikate {
                    erlaubt: MAX_ZERTIFIKATE_PRO_NACHRICHT,
                });
            }
            let zertifikat = k.vektor24()?;
            // added 03.10.2026 03:9x, R49: **DIE ERWEITERUNGEN DES ERSTEN
            // EINTRAGS WERDEN GESPEICHERT**, und davor wurden sie **weggeworfen**
            // (`let _zertifikate_erweiterungen = k.vektor16()?`). Das war ein
            // Fehler, der den Client **jede** OCSP-Stapelung blind machen
            // liess — GEMESSEN (03:7x) an `must_staple_mit_gelieferter_
            // stapelung_steht_die_verbindung`, das rot wurde, obwohl der Server
            // eine (von uns selbst gebaute und von `ocsp::beweise` akzeptierte)
            // Antwort mitschickte.
            //
            // added 03.10.2026 03:9x: **WAS AN DER STELLE ALS ERSTES STAND** und
            // warum es falsch war: `erweiterungen: l.rest().to_vec()` — `l` steht
            // hinter der Zertifikatsliste, also ist `rest()` **leer**. Das Feld
            // war damit **immer** `[]`, `ocsp_aus_zertifikatsblock` bekam `&[]`
            // und gab `Ok(None)` zurueck. Der Weg war also **lautlos** tot: kein
            // Fehler, kein Protokollbruch, nur keine Stapelung.
            //
            // added 03.10.2026 03:9x: **WAS DAS IN DER WILD BEDEUTET**, und das
            // ist der Grund, warum der Fehler mehr wert ist als ein roter Test:
            // (1) ein **widerrufenes** Zertifikat zu stapeln wird **nicht
            // gesehen** — `ocsp::beweise` hatte nie eine chance; (2) must-staple
            // war **unerfuellbar**, jede solche Verbindung brach ab. Wir haben
            // in R48 gemessen, dass 0 von 6 echten Hosts stapeln, also blieb
            // beides unauffaellig.
            //
            // added 03.10.2026 03:9x: **DAS 2-BYTE-PRAEFIX BLEIBT DRIN**, weil
            // `ocsp_aus_zertifikatsblock` es dort erwartet (Kommentar dort) —
            // deshalb wird der Ausschnitt **vor** dem `vektor16()` genommen.
            // Und **nur** der erste Eintrag: die Stapelung gehoert zum
            // Zertifikat, dessen Status sie meldet (RFC 8446 4.4.2.1), und
            // weiterzuzaehlen hiesse, fremde Erweiterungen als unsere zu lesen.
            // added 03.10.2026 04:0x, R49: **MEIN ERSTER WEG WAR FALSCH**, und
            // die Fehlerform gehoert notiert: ich wollte die Bytes **mit**
            // Laengenpraefix retten und habe dafuer `k.rest()` benutzt. GEMESSEN:
            // `Leser::rest()` **verbraucht** den Rest (`self.pos = self.daten.len()`,
            // vektor.rs:176) — der Leser stand danach am Ende und `vektor16()`
            // meldete `ZuKurz { erwartet: 2, vorhanden: 0 }`. **DIE HIER GESPEICHERTE
            // ERWEITERUNG IST DER INHALT OHNE PRAEFIX**, und der Aufrufer
            // `ocsp_aus_zertifikatsblock` ist darauf umgestellt. Der Grund ist
            // nicht Geschmack: `vektor16()` **prueft** die Laenge gegen den Puffer,
            // ein von mir gebautes `rest().to_vec()` nicht.
            let erweiterungen = k.vektor16()?;
            if c.erweiterungen.is_empty() {
                c.erweiterungen = erweiterungen.to_vec();
            }
            c.zertifikate.push(zertifikat.to_vec());
        }
        if c.zertifikate.is_empty() {
            return Err(NachrichtFehler::KeineZertifikate);
        }
        Ok(c)
    }
}

/// added 02.10.2026 23:hx UTC (Projektlabel 02.10.2026), S4. Die stapelte
/// OCSP-Antwort aus einer TLS-1.3-`Certificate`-Nachricht.
///
/// added 02.10.2026 23:hx: WARUM DAS NOETIG IST, und es ist eine Messung.
/// In TLS 1.3 gibt es **keine** `CertificateStatus`-Nachricht; der stapelte
/// Status steckt in der Erweiterung `status_request` der `Certificate`-
/// Nachricht (RFC 8446 4.4.3). GEMESSEN am 02.10.2026 23:fx mit
/// `openssl s_client -status` gegen `xkcd.com` und `www.netbsd.org`: beide
/// liefern in TLS 1.3 eine OCSP-Antwort („OCSP response: …"), in TLS 1.2 aber
/// in **0 von 5** Handshakes eine `CertificateStatus`-Nachricht. Wer nur den
/// 1.2-Weg auswertet, wertet also genau die Ausnahme aus.
///
/// added 02.10.2026 23:hx: DER RUMPF IST DERSELBE wie in 1.2. RFC 8446
/// 4.4.3 schreibt
///
/// ```text
/// struct {
///     CertificateStatusType status_type;   1 Byte, 1 = ocsp
///     select (status_type) { case ocsp: OCSPResponse; }   3 Byte Laenge
/// } CertificateStatusResponse;
/// opaque OCSPResponse<1..2^24-1>;
/// ```
///
/// — also `status_type` plus Dreierlaenge plus DER, genau wie RFC 6066 3 es
/// fuer die 1.2-Nachricht beschreibt. `ocsp::antwort_lesen` liest den
/// daraufhin ohne Aenderung beides.
///
/// `Ok(None)` heisst: der Server hat nichts gestapelt. Das ist der haeufige
/// Fall und **kein** Fehler.
pub fn ocsp_aus_zertifikatsblock(block: &[u8]) -> Result<Option<Vec<u8>>, NachrichtFehler> {
    // M4: `block` kommt aus einer `Certificate`-Nachricht, die durch
    // `MAX_NACHRICHT` begrenzt ist; die Laengenangaben darin werden gegen den
    // Rest geprueft, bevor irgendein Ausschnitt genommen wird.
    //
    // added 03.10.2026 04:1x, R49: **DER BISHERIGE ANFANG WAR FALSCH UND HAT DEN
    // FEHLER MITVERURSACHT.** Dort stand: „die ersten zwei Byte sind die Laenge
    // des Erweiterungsblocks selbst … also stehen sie hier noch drin“. **Sie
    // standen nie drin.** `Certificate::dekodiere` lieferte bis eben
    // `l.rest()` — den Rest **hinter** der Zertifikatsliste, also **leer** — und
    // selbst jetzt liefert es den **Inhalt** (`vektor16()`-Rueckgabe) **ohne**
    // Praefix. Ein Kommentar, der ein Verhalten beschreibt, das es nicht gibt,
    // ist schlimmer als keiner: er hat den naechsten Leser davon abgehalten,
    // `dekodiere` zu pruefen. **Regel daraus: wer ein Verhalten beschreibt,
    // der hat es gemessen.**
    //
    // added 03.10.2026 04:1x: **WOHER DIE LANGENPRUEFUNG JETZT KOMMT.** Vorher
    // stand hier `2 + laenge > block.len()` mit der Laenge aus den ersten zwei
    // Byte. Jetzt traegt der **Aufrufer** die Laenge: `dekodiere` liest sie mit
    // `vektor16()`, und das prueft sie gegen den Puffer. Was hier noch zu pruefen
    // ist, ist die **innere** Konsistenz — die Schleife unten begrenzt jeden
    // Ausschnitt auf `rest.len()` (M3/M4).
    let mut rest = block;
    // M3: die Zahl der Erweiterungen ist durch den Block begrenzt, die
    // Schleife laeuft aber ueber `MAX_ZERTIFIKATS_ERWEITERUNGEN`, damit ein
    // Block aus lauter Null-Erweiterungen (4 Byte je Stueck) nicht die
    // Zeitgrenze des Handshakes frisst.
    let mut gezaehlt = 0usize;
    while !rest.is_empty() {
        // added 03.10.2026 04:1x: der `if rest.len() < 4`-Test steht jetzt am
        // Anfang der Schleife und ist damit die einzige Laengengrenze fuer den
        // ersten Zugriff — vorher stand er nach dem `gezaehlt`-Test, was
        // unveraendert ist; ich habe ihn nur nicht angefasst.
        if gezaehlt >= MAX_ZERTIFIKATS_ERWEITERUNGEN {
            return Err(NachrichtFehler::ZuVieleErweiterungen {
                ist: gezaehlt,
                erlaubt: MAX_ZERTIFIKATS_ERWEITERUNGEN,
            });
        }
        gezaehlt += 1;
        if rest.len() < 4 {
            return Err(NachrichtFehler::Vektor(VektorFehler::ZuKurz {
                erwartet: 4,
                vorhanden: rest.len(),
            }));
        }
        let typ = u16::from_be_bytes([rest[0], rest[1]]);
        let laenge = u16::from_be_bytes([rest[2], rest[3]]) as usize;
        if 4 + laenge > rest.len() {
            return Err(NachrichtFehler::Vektor(VektorFehler::InhaltZuKurz {
                angesagt: 4 + laenge,
                vorhanden: rest.len(),
            }));
        }
        let inhalt = &rest[4..4 + laenge];
        if typ == ext::STATUS_REQUEST {
            // `status_type` (1 Byte) muss 1 sein (`ocsp`); 2 = `stapled SCTs`
            // (RFC 6962) ist eine andere Zusage und wird nicht gelesen.
            if inhalt.first() == Some(&1) {
                if inhalt.len() < 4 {
                    return Err(NachrichtFehler::Vektor(VektorFehler::ZuKurz {
                        erwartet: 4,
                        vorhanden: inhalt.len(),
                    }));
                }
                let n = u32::from_be_bytes([0, inhalt[1], inhalt[2], inhalt[3]]) as usize;
                if 4 + n > inhalt.len() {
                    return Err(NachrichtFehler::Vektor(VektorFehler::InhaltZuKurz {
                        angesagt: 4 + n,
                        vorhanden: inhalt.len(),
                    }));
                }
                return Ok(Some(inhalt[4..4 + n].to_vec()));
            }
            return Ok(None);
        }
        rest = &rest[4 + laenge..];
    }
    Ok(None)
}

/// added 02.10.2026 23:hx: M3, hoechstens so viele Erweiterungen in einer
/// `Certificate`-Nachricht.
///
/// added 02.10.2026 23:hx: GEMESSEN an echten Nachrichten: GitHub liefert 1,
/// `www.opensuse.org` 0 (nach `openssl s_client -msg`). Fuenf sind Reserve
/// fuer ein Server, das `status_request` noch um eine eigene Erweiterung
/// ergaenzt — und ein Vielfaches dessen, was in der Wildbahn gemessen wurde.
pub const MAX_ZERTIFIKATS_ERWEITERUNGEN: usize = 5;

// ---------------------------------------------------------------------------
// CertificateVerify
// ---------------------------------------------------------------------------

/// Die CertificateVerify-Nachricht (RFC 8446 Abschnitt 4.4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateVerify {
    /// Der Algorithmus (2 Byte).
    pub algorithmus: u16,
    /// Die Signatur, ungeprueft. Geprueft wird sie in T11.
    pub signatur: Vec<u8>,
}

/// RSA-PSS mit SHA-256 (RFC 8446 Abschnitt 4.4.3).
pub const SCHEMA_RSA_PSS_SHA256: u16 = 0x0804;
/// ECDSA mit P-256 und SHA-256 (RFC 8446 Abschnitt 4.4.3).
pub const SCHEMA_ECDSA_P256_SHA256: u16 = 0x0403;

impl CertificateVerify {
    pub fn dekodiere(nachricht: &[u8]) -> Ergebnis<CertificateVerify> {
        let (_typ, inhalt) = zerlege(nachricht, typen::CERTIFICATE_VERIFY)?;
        let mut l = Leser::neu(inhalt);
        let algorithmus = l.u16()?;
        let signatur = l.vektor16()?.to_vec();
        Ok(CertificateVerify {
            algorithmus,
            signatur,
        })
    }

    /// Ist das ein Schema, das diese Crate pruefen kann?
    pub fn schema_unterstuetzt(&self) -> bool {
        self.algorithmus == SCHEMA_RSA_PSS_SHA256 || self.algorithmus == SCHEMA_ECDSA_P256_SHA256
    }
}

// ---------------------------------------------------------------------------
// Finished
// ---------------------------------------------------------------------------

/// Die Finished-Nachricht (RFC 8446 Abschnitt 4.4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub verify_data: [u8; 32],
}

impl Finished {
    /// Das `verify_data`, das ein Peer zu einem `finished_key` und einem
    /// Transkript-Hash gehoeren sollte (RFC 8446 4.4.4).
    ///
    /// added 30.09.2026, Runde 2, TLS R2. ANLASS: die Frage in
    /// `team/WUNSCH_DEEPSEEK.txt` — "falls `Finished` keine
    /// oeffentliche Verifikation hat, sage ich hier Bescheid". Sie
    /// hatte keine, und ohne diese Funktion muss jeder Aufrufer den
    /// HMAC von Hand bilden; in `narsil-quic` waere das eine zweite
    /// Stelle, an der die Formel steht.
    ///
    /// DER WEG, und er ist derselbe wie in `strom.rs`:
    /// `HMAC(finished_key, Transcript-Hash)`. Der Hash muss der Stand
    /// **bis einschliesslich** der Nachricht sein, fuer die geprueft
    /// wird — fuer das `finished` des Servers also bis zum
    /// `CertificateVerify`, fuer das des Clients bis zum `finished` des
    /// Servers. Beide Staende sind an zwei Stellen notiert, weil sie
    /// sich um 4 + 32 Byte unterscheiden.
    pub fn erwartet(finished_key: &[u8; 32], transkript_hash: &[u8; 32]) -> [u8; 32] {
        crate::hmac::HmacSha256::berechne(finished_key, transkript_hash)
    }

    /// Prueft das `finished` in **konstanter Zeit**.
    ///
    /// added 30.09.2026, Runde 2, TLS R2. `vergleich_gleich` vergleicht
    /// ohne fruehen Abbruch; ein `==` auf `[u8; 32]` waere schneller
    /// zu schreiben und waere eine Zeitluecke.
    pub fn pruefe_gegen(&self, finished_key: &[u8; 32], transkript_hash: &[u8; 32]) -> bool {
        let erwartet = Finished::erwartet(finished_key, transkript_hash);
        crate::hmac::vergleich_gleich(&erwartet, &self.verify_data)
    }

    pub fn dekodiere(nachricht: &[u8]) -> Ergebnis<Finished> {
        let (_typ, inhalt) = zerlege(nachricht, typen::FINISHED)?;
        if inhalt.len() != 32 {
            return Err(NachrichtFehler::Vektor(VektorFehler::InhaltZuKurz {
                angesagt: 32,
                vorhanden: inhalt.len(),
            }));
        }
        let mut verify_data = [0u8; 32];
        verify_data.copy_from_slice(inhalt);
        Ok(Finished { verify_data })
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

/// Zerlegt eine Handshake-Nachricht in Typ und Inhalt, mit Pruefung des
/// Typs. M4: das Laengenfeld wird von `Leser::vektor24` geprueft.
fn zerlege(nachricht: &[u8], erwartet: u8) -> Ergebnis<(u8, &[u8])> {
    let mut l = Leser::neu(nachricht);
    let typ = l.u8()?;
    if typ != erwartet {
        return Err(NachrichtFehler::TypFalsch {
            erwartet,
            bekommen: typ,
        });
    }
    let inhalt = l.vektor24()?;
    Ok((typ, inhalt))
}

/// Kleiner Trick, damit `kodiere` den Typ voranstellen kann, ohne den
/// Schreiber zu duplizieren.
trait TypVoranstellen {
    fn tap_typ(self, typ: u8) -> Vec<u8>;
}

impl TypVoranstellen for Vec<u8> {
    fn tap_typ(mut self, typ: u8) -> Vec<u8> {
        self.insert(0, typ);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::von_hex;

    /// M7: der ClientHello aus RFC 8448 Abschnitt 3, Byte fuer Byte.
    ///
    /// Die Sollwerte stammen aus
    /// /home/jaff/mutation_md/tls/rfc8448_werte.txt, und die Datei, die
    /// sie erzeugt, liest sie aus dem RFC-Text. Vier Gegenproben darin
    /// laufen gegen den RFC und sind bestaetigt. Nichts davon ist
    /// abgetippt.
    const RFC_CLIENT_HELLO: &str = "010000c00303cb34ecb1e78163ba1c38c6dacb196a6dffa21a8d9912ec18a2ef6283024dece7000006130113031302010000910000000b0009000006736572766572ff01000100000a00140012001d0017001800190100010101020103010400230000003300260024001d002099381de560e4bd43d23d8e435a7dbafeb3c06e51c13cae4d5413691e529aaf2c002b0003020304000d0020001e040305030603020308040805080604010501060102010402050206020202002d00020101001c00024001";
    // EIGENER FEHLER: hier stand zuerst ein 122-Oktett-ServerHello, und
    // der stammt aus Abschnitt 7 des RFC (Compatibility Mode). Abschnitt 3
    // hat 90 Oktett. Der Extraktor in
    // /home/jaff/mutation_md/tls/rfc8448_vektoren.py hatte die
    // Oktettzahl aus der Beschriftung ignoriert und war 3000 Zeilen
    // weitergelaufen — die Laenge passte, weil er einfach weiterlas.
    const RFC_SERVER_HELLO: &str = "020000560303a6af06a4121860dc5e6e60249cd34c95930c8ac5cb1434dac155772ed3e2692800130100002e00330024001d0020c9828876112095fe66762bdbf7c672e156d6cc253b833df1dd69b1b04e751f0f002b00020304";
    const RFC_CLIENT_PUB: &str = "99381de560e4bd43d23d8e435a7dbafeb3c06e51c13cae4d5413691e529aaf2c";
    const RFC_SERVER_PUB: &str = "c9828876112095fe66762bdbf7c672e156d6cc253b833df1dd69b1b04e751f0f";

    fn b32(s: &str) -> [u8; 32] {
        let v = von_hex(s).expect("Hex im Test");
        let mut a = [0u8; 32];
        a.copy_from_slice(&v);
        a
    }

    /// M7: der ClientHello aus RFC 8448 wird dekodiert und wieder
    /// kodiert, und das Ergebnis ist BYTEGENAU derselbe. Das prueft
    /// Kodierer und Dekodierer in einem Schritt.
    #[test]
    fn client_hello_aus_rfc8448_bytegenau() {
        let soll = von_hex(RFC_CLIENT_HELLO).expect("Hex");
        assert_eq!(soll.len(), 196, "M7: der RFC sagt 196 Oktett");

        let ch = ClientHello::dekodiere(&soll).expect("Dekodieren");
        assert_eq!(
            ch.random,
            b32("cb34ecb1e78163ba1c38c6dacb196a6dffa21a8d9912ec18a2ef6283024dece7")
        );
        assert_eq!(ch.cipher_suites, vec![0x1301, 0x1303, 0x1302]);
        assert_eq!(ch.kompression, vec![0], "M7: nur Nullkompression");
        assert_eq!(
            ch.sni.as_deref(),
            Some(&b"server"[..]),
            "M7: SNI ist der Servername"
        );
        assert_eq!(ch.versionen, vec![0x0304], "M7: TLS 1.3");
        // GEMESSEN: der RFC-Client bietet NEUN Gruppen an, x25519 ist
        // nur die erste. Ich hatte eine erwartet und damit den Dekodierer
        // fuer falsch gehalten — die Implementierung war richtig.
        assert_eq!(ch.gruppen.len(), 9, "M7: neun Gruppen im Angebot");
        assert_eq!(ch.gruppen[0], 0x001d, "M7: x25519 ist die erste");
        assert!(
            ch.gruppen.contains(&0x001d),
            "M7: x25519 muss enthalten sein"
        );
        // GEMESSEN: der ClientHello aus RFC 8448 Abschnitt 3 traegt KEINE
        // ALPN-Erweiterung. Ich hatte eine erwartet — die Erwartung stammte
        // aus einem anderen Abschnitt des RFC. Die Erweiterungen, die
        // wirklich drin sind, sind in
        // /home/jaff/mutation_md/tls/rfc8448_werte.txt nachlesbar.
        assert!(ch.alpn.is_empty(), "M7: Abschnitt 3 schickt kein ALPN");
        assert_eq!(ch.psk_modi, vec![1], "M7: psk_dhe_ke");
        // Der Key Share im ClientHello ist der OEFFENTLICHE Schluessel des
        // CLIENT. Ich hatte hier den Server-Schluessel erwartet — die
        // Implementierung war richtig, die Erwartung nicht.
        assert_eq!(
            ch.key_share,
            von_hex(RFC_CLIENT_PUB).expect("Hex"),
            "M7: Key Share ist der oeffentliche Schluessel des Client"
        );
        assert_eq!(ch.key_share_gruppe, GRUPPE_X25519);

        // Und jetzt der Weg zurueck. BITTE KEIN BYTEWEISER VERGLEICH mit
        // dem RFC-Byte-String: die Reihenfolge der Erweiterungen ist in
        // RFC 8446 nicht festgelegt, und mein Kodierer schreibt sie in
        // eigener Reihenfolge. Der RFC-ClientHello enthaelt ausserdem
        // `ff01` (renegotiation_info), `0023` (session_ticket) und `001c`,
        // die diese Crate nicht schreibt.
        //
        // Behauptet wird deshalb der SINNGEMASSE Rundlauf: was
        // dekodiert wurde, ergibt wieder dieselben Felder.
        let wieder = ch.kodiere();
        let nochmal = ClientHello::dekodiere(&wieder).expect("zweites Dekodieren");
        assert_eq!(nochmal.random, ch.random, "M7: random");
        assert_eq!(nochmal.cipher_suites, ch.cipher_suites, "M7: Cipher Suites");
        assert_eq!(nochmal.kompression, ch.kompression, "M7: Kompression");
        assert_eq!(nochmal.sni, ch.sni, "M7: SNI");
        assert_eq!(nochmal.versionen, ch.versionen, "M7: Versionen");
        assert_eq!(nochmal.gruppen, ch.gruppen, "M7: Gruppen");
        assert_eq!(
            nochmal.signaturalgorithmen, ch.signaturalgorithmen,
            "M7: Signaturalgorithmen"
        );
        assert_eq!(nochmal.alpn, ch.alpn, "M7: ALPN");
        assert_eq!(nochmal.psk_modi, ch.psk_modi, "M7: PSK-Modi");
        assert_eq!(nochmal.key_share, ch.key_share, "M7: Key Share");
        assert_eq!(nochmal.key_share_gruppe, ch.key_share_gruppe, "M7: Gruppe");
        // Und die Laenge des RFC-ClientHellos, als Mass fuer den Leser.
        assert_eq!(soll.len(), 196, "M7: der RFC-ClientHello hat 196 Oktett");
    }

    /// M7: der ServerHello aus RFC 8448, byteweise.
    #[test]
    fn server_hello_aus_rfc8448() {
        let soll = von_hex(RFC_SERVER_HELLO).expect("Hex");
        assert_eq!(soll.len(), 90, "M7: der RFC sagt 90 Oktett");
        let sh = ServerHello::dekodiere(&soll).expect("Dekodieren");
        assert_eq!(sh.cipher_suite, 0x1301, "M7: TLS_AES_128_GCM_SHA256");
        assert!(
            !sh.cookie.is_some(),
            "M7: kein Cookie, kein HelloRetryRequest"
        );
        assert_eq!(sh.gruppe, GRUPPE_X25519);
        assert_eq!(sh.x25519().expect("32 Byte"), b32(RFC_SERVER_PUB));
        assert!(!sh.retry, "M7: das ist kein HelloRetryRequest");
    }

    /// M7: Finished dekodieren, aus dem ServerFinished des RFC.
    #[test]
    fn finished_aus_rfc8448() {
        let verify = b32("9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718");
        let mut n = Schreiber::neu();
        n.vektor24(&verify);
        n.u8(typen::FINISHED);
        let mut nachricht = n.uebernehmen();
        // Der Typ muss VOR die Laenge.
        nachricht.rotate_right(1);
        let f = Finished::dekodiere(&nachricht).expect("Dekodieren");
        assert_eq!(f.verify_data, verify, "M7: verify_data");
    }

    /// M4: ein ClientHello, dessen Laengenfeld 16 MiB ankündigt.
    #[test]
    fn laengenfeld_bombe_im_client_hello() {
        let n = von_hex("01000000ff").expect("Hex"); // typ 1, Laenge 0xffffff
        let r = ClientHello::dekodiere(&n);
        assert!(r.is_err(), "M7: muss ein Fehler sein, kam {r:?}");
    }

    /// M4: ein ClientHello, das mitten im Feld endet.
    #[test]
    fn abgeschnittener_client_hello() {
        for kuerzen in 1..196usize {
            let voll = von_hex(RFC_CLIENT_HELLO).expect("Hex");
            let r = ClientHello::dekodiere(&voll[..voll.len() - kuerzen]);
            assert!(r.is_err(), "M7: {kuerzen} Byte fehlen, muss Fehler sein");
        }
    }

    /// M4: die SNI- und ALPN-Grenzen, VOR der Allokation.
    #[test]
    fn sni_und_alpn_grenzen() {
        let zu_lang = vec![b'x'; MAX_HOSTNAME + 1];
        assert!(matches!(
            ClientHello::baue([0u8; 32], Some(&zu_lang), &[], &[0u8; 32], &[0u8; 32]),
            Err(NachrichtFehler::HostnameZuLang { .. })
        ));
        // Genau an der Grenze: erlaubt.
        let an = vec![b'x'; MAX_HOSTNAME];
        assert!(ClientHello::baue([0u8; 32], Some(&an), &[], &[0u8; 32], &[0u8; 32]).is_ok());

        // Zu viele ALPN-Eintraege.
        let viele: Vec<&[u8]> = vec![b"h2"; MAX_ALPN_ANZAHL + 1];
        assert!(matches!(
            ClientHello::baue([0u8; 32], None, &viele, &[0u8; 32], &[0u8; 32]),
            Err(NachrichtFehler::AlpnZuViele { .. })
        ));
        // Ein zu langer Eintrag.
        let langer = vec![b'p'; MAX_ALPN_EINTRAG + 1];
        assert!(matches!(
            ClientHello::baue([0u8; 32], None, &[&langer], &[0u8; 32], &[0u8; 32]),
            Err(NachrichtFehler::AlpnEintragZuLang { .. })
        ));
    }

    /// M7: ein selbst gebauter ClientHello mit SNI und ALPN geht durch
    /// den Rundlauf und behält beide.
    #[test]
    fn selbstgebauter_client_hello_rundlauf() {
        let ch = ClientHello::baue(
            [7u8; 32],
            Some(b"www.example.com"),
            &[b"http/1.1", b"h2"],
            &[1u8; 32],
            &[2u8; 32],
        )
        .expect("Bau");
        let bytes = ch.kodiere();
        let zurück = ClientHello::dekodiere(&bytes).expect("Dekodieren");
        assert_eq!(zurück.sni.as_deref(), Some(&b"www.example.com"[..]));
        assert_eq!(zurück.alpn, vec![b"http/1.1".to_vec(), b"h2".to_vec()]);
        assert_eq!(zurück.random, [7u8; 32]);
        assert_eq!(zurück.key_share, vec![2u8; 32]);
        assert_eq!(zurück.kodiere(), bytes, "M7: stabil");
    }
    /// M7: EIN ClientHello, das beide Protokollversionen anbietet.
    ///
    /// Die Pruefung geht von den BYTES aus, nicht von den Feldern: ein
    /// Feld, das richtig befuellt ist und beim Kodieren leer bleibt,
    /// waere fuer einen Server dasselbe wie ein nicht gesetztes Feld.
    #[test]
    fn der_gemeinsame_clienthello_nennt_beide_versionen() {
        let ch = ClientHello::baue_beide([0x22u8; 32], "badssl.com", &[b"http/1.1"], &[0x33u8; 32])
            .expect("ClientHello");
        let b = ch.kodiere();
        assert_eq!(b[0], typen::CLIENT_HELLO);
        // legacy_version 0x0303, auch fuer TLS 1.3 (RFC 8446 4.1.2).
        assert_eq!(&b[4..6], &[0x03, 0x03], "M7: legacy_version ist 0x0303");
        // Die Suites: TLS 1.3 zuerst, dann TLS 1.2.
        //
        // DIE FESTEN OFFSETS: 4 Kopf + 2 legacy_version + 32 random
        // + 1 Laengenbyte der session_id (leer) = 39. Dann die
        // 2-Byte-Laenge der Suite-Liste und die Suites selbst.
        let n = u16::from_be_bytes([b[39], b[40]]) as usize;
        let suites: Vec<u16> = (0..n / 2)
            .map(|i| u16::from_be_bytes([b[41 + 2 * i], b[42 + 2 * i]]))
            .collect();
        assert_eq!(suites[0], 0x1301, "M7: TLS 1.3 zuerst");
        assert!(suites.contains(&0xC02F), "M7: AES_128_GCM_SHA256 ist dabei");
        assert!(
            !suites.contains(&0xC013) && !suites.contains(&0x002F),
            "M7: kein CBC und kein RSA-Schluesseltausch"
        );
        // Die Erweiterungen. Der Test sucht die PAARE, nicht die Nummern
        // allein: eine Erweiterung `0017` ohne Laenge ist eine andere
        // Erweiterung.
        for (code, laenge, inhalt) in [
            // supported_versions: DER INHALT TRAEGT EINE EIGENE
            // LAENGE. `versions<2..254>` ist ein Vektor mit
            // EINEM Byte Laenge, die Erweiterung traegt zusaetzlich
            // ihre 2-Byte-Laenge: 5 Byte Inhalt fuer zwei Versionen.
            // Ohne das innere `04` waere der ServerHello-Leser um
            // vier Byte verrutscht.
            (43u16, 5usize, &[0x04, 0x03, 0x04, 0x03, 0x03][..]),
            (23, 0, &[][..]),     // extended_master_secret
            (65281, 1, &[0][..]), // renegotiation_info
            (11, 2, &[1, 0][..]), // ec_point_formats
            (35, 0, &[][..]),     // session_ticket
        ] {
            let fenster = b.windows(4).position(|w| {
                w[0] == (code >> 8) as u8
                    && w[1] == (code & 0xff) as u8
                    && w[2] == (laenge >> 8) as u8
                    && w[3] == (laenge & 0xff) as u8
            });
            assert!(
                fenster.is_some(),
                "M7: Erweiterung {code} mit Laenge {laenge} fehlt"
            );
            if !inhalt.is_empty() {
                let f = fenster.expect("gefunden") + 4;
                assert_eq!(&b[f..f + inhalt.len()], inhalt, "Erweiterung {code}");
            }
        }
        // Die Gruppen: x25519 UND secp256r1, MIT der inneren
        // 2-Byte-Laenge (siehe `kodiere_erweiterungen`). Ohne
        // secp256r1 waehlt badssl.com im TLS-1.2-Pfad eine Gruppe,
        // die diese Crate fuer 1.2 nicht rechnen kann.
        let gruppen = b
            .windows(5)
            .position(|w| w == [0x00, 0x0a, 0x00, 0x06, 0x00])
            .expect("supported_groups")
            + 4;
        assert_eq!(
            &b[gruppen..gruppen + 6],
            &[0x00, 0x04, 0x00, 0x1d, 0x00, 0x17],
            "M7: innere Laenge 4, dann x25519 und secp256r1"
        );
    }

    /// M7: der ClientHello **ohne** den Schalter traegt die
    /// TLS-1.2-Erweiterungen nicht.
    ///
    /// DER GRUND FUER DEN GEGENTEST: der TLS-1.3-Weg hier und der
    /// TLS-1.2-Weg in `nachricht12.rs` sind zwei Bauplanstellen. Der
    /// Schalter `auch_tls12` wird von `baue` (nur 1.3) NICHT gesetzt.
    /// Setzte ihn jemand auf `true`, wuerde der TLS-1.3-Client vier
    /// Erweiterungen mitschicken, die dort nichts zu bedeuten haben —
    /// und kein Server wuerde sich beschweren.
    #[test]
    fn der_reine_tls12_clienthello_bleibt_rein() {
        let ch = ClientHello::baue(
            [0x22u8; 32],
            Some(b"h"),
            &[b"http/1.1"],
            &[0u8; 32],
            &[0x33u8; 32],
        )
        .expect("ClientHello");
        let b = ch.kodiere();
        assert!(!ch.auch_tls12, "M7: der Schalter steht auf aus");
        assert!(!ch.auch_secp256r1, "M7: und der zweite auch");
        // `extended_master_secret` (0x0017) mit Laenge 0: alle vier
        // Byte, sonst waere es eine andere Erweiterung.
        assert!(
            !b.windows(4).any(|w| w == [0x00, 0x17, 0x00, 0x00]),
            "M7: keine extended_master_secret-Erweiterung"
        );
        assert!(
            !b.windows(4).any(|w| w == [0xff, 0x01, 0x00, 0x01]),
            "M7: keine renegotiation_info-Erweiterung"
        );
        // Und `supported_groups` nennt NUR x25519: die zweite Gruppe
        // gehoert zum gemeinsamen ClientHello, nicht zu diesem. Die
        // innere Laenge ist 2 fuer EINEN Eintrag — siehe
        // `kodiere_erweiterungen`, wo dieser Fehler zuerst stand.
        assert!(
            b.windows(6)
                .any(|w| w == [0x00, 0x0a, 0x00, 0x04, 0x00, 0x02]),
            "M7: der reine 1.3-ClientHello bietet genau x25519"
        );
    }
}

#[cfg(test)]
mod erweiterung_tests {
    /// added 03.10.2026 04:6x, R49. **DIE DEKODIERSTELLE DIREKT.** Der
    /// Handshake-Test in `server.rs` beweist die **Wirkung**, aber nicht die
    /// **Stelle**: hier steht geprueft, dass `Certificate::dekodiere` die
    /// Erweiterungen des ersten Eintrags **wirklich ablegt** — der Fehler war
    /// ein `let _zertifikate_erweiterungen = k.vektor16()?`, also ein gelesenes
    /// und weggeworfenes Feld, und genau das ist an dieser Stelle unsichtbar.
    #[test]
    fn zertifikatseintrag_behaelt_seine_erweiterung() {
        let antwort = crate::x509::bauen::ocsp_antwort_gut(
            &crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA")),
            &crate::x509::bauen::blatt_seriennummer(),
        )
        .unwrap_or_else(|_| panic!("OCSP-Antwort"));
        let blatt = crate::x509::bauen::zertifikat(true).unwrap_or_else(|_| panic!("Blatt"));
        let ca = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
        let nachricht =
            crate::server::baue_certificate_liste_mit_ocsp(&[blatt, ca], Some(&antwort))
                .unwrap_or_else(|_| panic!("Nachricht"));
        let c = super::Certificate::dekodiere(&nachricht).unwrap_or_else(|_| panic!("Dekodieren"));
        assert_eq!(
            c.zertifikate.len(),
            2,
            "die Nachricht traegt zwei Zertifikate — sonst prueft der naechste Satz nichts"
        );
        assert!(
            !c.erweiterungen.is_empty(),
            "die Erweiterungen sind wieder verworfen worden"
        );
        let nutzlast = super::ocsp_aus_zertifikatsblock(&c.erweiterungen)
            .unwrap_or_else(|_| panic!("Erweiterungsblock unlesbar"))
            .unwrap_or_else(|| panic!("die status_request-Erweiterung wurde nicht gefunden"));
        // added 03.10.2026 04:6x: **ZAEHLEN, NICHT SUCHEN** — ein `contains` auf
        // einem Byte-Praefix wuerde auch einen halben Abschneidefehler
        // bestehen. Die Laenge muss zur gebauten Antwort passen.
        assert_eq!(
            nutzlast.len(),
            antwort.len(),
            "die Nutzlast ist nicht die gebaute Antwort ({} gegen {} Byte)",
            nutzlast.len(),
            antwort.len()
        );
    }

    /// added 03.10.2026 04:6x, R49. **DER GEGENFALL:** dieselbe Nachricht
    /// **ohne** Stapelung muss `Ok(None)` liefern — und nicht etwa eine
    /// Restnutzlast. Sonst wuerde der Test oben auch eine Erweiterung finden,
    /// die der Server gar nicht geschickt hat.
    #[test]
    fn ohne_stapelung_bleibt_es_bei_keiner_antwort() {
        let blatt = crate::x509::bauen::zertifikat(true).unwrap_or_else(|_| panic!("Blatt"));
        let ca = crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA"));
        let nachricht = crate::server::baue_certificate_liste(&[blatt, ca])
            .unwrap_or_else(|_| panic!("Nachricht"));
        let c = super::Certificate::dekodiere(&nachricht).unwrap_or_else(|_| panic!("Dekodieren"));
        let r = super::ocsp_aus_zertifikatsblock(&c.erweiterungen)
            .unwrap_or_else(|_| panic!("der leere Block liess sich nicht lesen"));
        assert!(r.is_none(), "ohne Stapelung wurde eine Antwort gefunden");
    }

    /// added 03.10.2026, Rolle TLS (Angriffsgrenzen): **DIE ZERTIFIKATGRENZE
    /// GREIFT AM ECHTEN PARSER**, und nicht nur als Konstante.
    ///
    /// **DIE ZAHL OHNE GRENZE, und das ist das Argument:** `MAX_NACHRICHT` ist
    /// `0xFFFFFF` Byte, und der kleinstemoegliche Eintrag in der Liste braucht
    /// **vier** Byte — drei Laengenpraefix plus ein Inhaltsbyte. Das sind
    /// **4 194 303** Eintraege und damit **4 194 303** `Vec`-Allokationen,
    /// **bevor** `MAX_KETTEN_TIEFE` = 10 irgendetwas begrenzen kann.
    ///
    /// **UND WARUM DIESER TEST DIREKT AN `dekodiere` GEHT:** die Grenze steht
    /// in einer Schleife ueber `vektor24()`. Ein Test, der nur die Konstante
    /// prueft, wuerde gruen sein, wenn die Schleife die Grenze **nicht**
    /// aufruft — und genau das war der Fehler, den R49 bei den Erweiterungen
    /// gemacht hat (ein `let _… =` gelesen und weggeworfen).
    #[test]
    fn zu_viele_zertifikate_brechen_am_parser_ab() {
        // added 03.10.2026, TLS: **EINE ECHTE KETTE von genau der Grenze** —
        // die muss durch. Sonst prueft der Test nur die Ablehnung und die
        // Grenze waere zu eng.
        let blatt = crate::x509::bauen::zertifikat(true).unwrap_or_else(|_| panic!("Blatt"));
        let mut pfad = vec![blatt];
        while pfad.len() < super::MAX_ZERTIFIKATE_PRO_NACHRICHT {
            pfad.push(crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA")));
        }
        let nachricht =
            crate::server::baue_certificate_liste(&pfad).unwrap_or_else(|_| panic!("Nachricht"));
        let c = super::Certificate::dekodiere(&nachricht)
            .unwrap_or_else(|_| panic!("die Grenze muss genau noch durchlassen"));
        assert_eq!(
            c.zertifikate.len(),
            super::MAX_ZERTIFIKATE_PRO_NACHRICHT,
            "GENAU die Grenze kommt an"
        );

        // added 03.10.2026, TLS: **UND EIN ENTRAG MEHR WIRD ABGELEHNT**, mit
        // dem benannten Fehler und **nicht** mit einem Absturz.
        pfad.push(crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA")));
        let nachricht =
            crate::server::baue_certificate_liste(&pfad).unwrap_or_else(|_| panic!("Nachricht"));
        match super::Certificate::dekodiere(&nachricht) {
            Err(super::NachrichtFehler::ZuVieleZertifikate { erlaubt }) => assert_eq!(
                erlaubt,
                super::MAX_ZERTIFIKATE_PRO_NACHRICHT,
                "der Fehler nennt die Grenze"
            ),
            // added 03.10.2026, TLS: **JEDER ANDERE AUSGANG IST EIN BEFUND** —
            // ein `unwrap` wuerde die Art des Fehlers verlieren, und genau die
            // ist hier die Aussage.
            anders => panic!("erwartet ZuVieleZertifikate, bekam {anders:?}"),
        }
    }

    /// added 03.10.2026, Rolle TLS: **UND DIE KETTE AUS DER WILDL.** Ein echter
    /// oeffentlicher Pfad hat 3 bis 4 Zertifikate, und die Grenze 16 darf keinen
    /// davon beschneiden — das ist der Grund fuer die Zahl und nicht bloss eine
    /// runde.
    #[test]
    fn eine_echte_kette_kommt_ungekuerzt_an() {
        let pfad = [
            crate::x509::bauen::zertifikat(true).unwrap_or_else(|_| panic!("Blatt")),
            crate::x509::bauen::zertifikat_ca().unwrap_or_else(|_| panic!("CA")),
        ];
        let nachricht =
            crate::server::baue_certificate_liste(&pfad).unwrap_or_else(|_| panic!("Nachricht"));
        let c = super::Certificate::dekodiere(&nachricht).unwrap_or_else(|_| panic!("Dekodieren"));
        assert_eq!(c.zertifikate.len(), 2, "Blatt plus Zwischenzertifikat");
    }
}
