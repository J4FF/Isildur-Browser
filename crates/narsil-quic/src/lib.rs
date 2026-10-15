//! narsil-quic — QUIC und HTTP/3 in reinem Rust.
//!
//! Auftrag: team/AUFTRAG_DEEPSEEK.txt. Vision: "HTTPS/HTTP3" fuer den
//! Browser.
//!
//! Nur `std`. Kein `unsafe`. Keine Fremd-Crates (ADR-030). Die Kryptografie
//! (AEAD, HKDF, AES, ChaCha20) kommt aus `narsil-tls` (nur lesen).
//!
//! Aufbau in der Reihenfolge des Auftrags:
//!   Q1 `schutz`  — Paketschutz nach RFC 9001 (Initial-Schluessel, Header-
//!                  Schutz, AEAD). Geprueft gegen RFC 9001 Anhang A.
//!   Q2 `varint`/`kopf`/`rahmen` — QUIC-Varint, Paketkoepfe und alle Frame-
//!                  Typen nach RFC 9000.
//!   Q3 `tls`     — TLS 1.3 ueber QUIC (CRYPTO-Frames, Transportparameter).
//!   Q4 `verlust` — Verlusterkennung und Staukontrolle nach RFC 9002.
//!   Q5 `h3`      — HTTP/3 nach RFC 9114 und QPACK nach RFC 9204.
//!
//! Grundsatz wie in narsil-tls: handgeschriebener Protokollcode ist nur so gut
//! wie seine Tests. Jede Stufe wird gegen die offiziellen Vektoren aus dem
//! jeweiligen RFC geprueft, woertlich im Test mit Quellenangabe.

pub mod alt_svc;
pub mod alt_svc_speicher;
pub mod client;
pub mod h3;
pub mod huffman;
pub mod kopf;
pub mod parameter;
pub mod qpack;
pub mod qpack_dyn;
pub mod rahmen;
pub mod schutz;
pub mod spur;
pub mod varint;
pub mod verbindung;
pub mod verlust;

pub use alt_svc::{AltSvc, AltSvcFehler, alt_svc_lesen};
pub use alt_svc_speicher::{AltSvcSpeicher, H3_FRIST, MAX_URSPRUENGE, SpeicherFehler};
pub use client::retry_zerlegen_fuer_test;
pub use client::{
    Antwort, ClientFehler, FENSTER_ABSTAND, FENSTER_SCHRITT, FENSTER_START, FehlerArt, FehlerText,
    MAX_ANTWORT_BYTE, MAX_ANTWORT_DATAGRAMME, MAX_DATAGRAMM, MAX_GEPUFFERTE_PAKETE,
    MAX_GEPUFFERTES_PAKET, MAX_HANDSHAKE_DATAGRAMME, MAX_HANDSHAKE_PUFFER, MAX_OFFENE_STROEME,
    MAX_PTO, MIN_STATELESS_RESET, Optionen, ZEITBLOCK, ack_bereiche, fehler_text,
    fenster_ergaenzen, hole, hole_mit, hole_mit_roh, quic_suite, speicher_aus_pem,
    token_steht_in_daten, token_steht_in_daten_mehrere, wurzelzertifikate,
};
pub use h3::{H3Fehler, H3Rahmen, h3_lesen, h3_reserviert, h3_schreiben};
pub use kopf::{
    KopfFehler, KurzerKopf, LangerKopf, kurze_lesen, kurze_schreiben, lange_lesen, lange_schreiben,
    paketnummer_rekonstruieren,
};
pub use parameter::{
    BevorzugteAdresse, EXT_QUIC_TRANSPORT_PARAMETERS, Parameter, ParameterFehler, parameter_lesen,
    parameter_schreiben,
};
pub use qpack::{
    FeldZeile, MAX_FELD_LAENGE, MAX_FELDER, QpackFehler, STATISCHE_TABELLE,
    feldabschnitt_dekodieren, feldabschnitt_kodieren, huffman_dekodiere, huffman_kodiere,
    statisch_index, statisch_name_index,
};
pub use qpack_dyn::{
    DecoderAnweisung, DynamischeTabelle, EncoderAnweisung, QpackDekoder, decoder_anweisung_lesen,
    encoder_anweisung_lesen,
};
pub use rahmen::{
    AckBereich, MAX_ACK_BEREICHE, MAX_GRUND_LAENGE, Rahmen, RahmenFehler, rahmen_alle,
    rahmen_lesen, rahmen_schreiben,
};
pub use schutz::{
    Entschuetzt, INITIAL_SALZ, PaketSchluessel, RETRY_GEHEIMNIS, SchutzFehler, Suite, entschuetzen,
    initial_geheimnis, initial_schuetzen, kurzes_schuetzen, maske_berechnen, nonce_berechnen,
    retry_tag, richtungs_geheimnis, schluessel_ableiten,
};
pub use varint::{MAX_WERT, VarintFehler, lese as varint_lese, schreibe as varint_schreibe};
pub use verbindung::{Ebene, Tls, Verbindung, VerbindungsFehler, Zustand, datagramm_zerlegen};
pub use verlust::{Gesendet, INITIALE_RTT, PAKET_SCHWELLE, RttSchaetzer, Stau, verlorene_pakete};

pub use spur::{MAX_SPURZEILEN, spur_anzahl, spur_leeren, spur_letzte, spur_text, spur_verworfen};
