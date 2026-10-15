//! Das statische Brotli-Woerterbuch aus **RFC 7932 Anhang A**.
//!
//! added 30.09.2026 bei der Uebernahme des Brotli-Auftrags von der
//! Rolle KOMPRESS.
//!
//! ERZEUGT, NICHT ABGETIPPT: `mutation_md/tls/brotli_dict_generator.py`
//! liest die 122.784 Byte aus `rfc/rfc7932.txt` und schreibt diese
//! Datei. Die drei Pruefungen, alle mit Sollwerten **aus dem RFC**:
//!
//!   1. Laenge der DICT: der RFC nennt 122.784 Byte (Anhang A).
//!   2. CRC-32 der DICT: der RFC nennt 0x5136cb04 (Anhang A).
//!   3. `DICTSIZE` aus der Rekursion in Abschnitt 8 muss dieselben
//!      122.784 ergeben.
//!
//! Die Wortgrenzen stehen **nicht** hier, sondern werden in
//! `dictionary.rs` aus `NDBITS` berechnet. Grund: Abschnitt 8 gibt die
//! Rekursion an, und 25 so berechnete Zahlen koennen keinen Tippfehler
//! enthalten — 122.785 abgedruckte Offsets schon.
//!
//! WARUM EINE EIGENE DATEI: 122.784 Byte als `const`-Array waeren ein
//! Eintrag, der jeden `grep` und jeden Bildschirm fuellt. Hier steht
//! nur der Text; die Rust-Seite liest ihn ueber `include_str!` und
//! dekodiert ihn einmal.

/// DICT als Hex-Text, Zeilen zu 64 Zeichen.
///
/// DER WEG ZUM BYTE: `dictionary.rs` liest diese Zeichen ueber
/// `include_str!` und wandelt sie mit `u8::from_str_radix` in Paare
/// um. Das kostet beim Programmstart einmal 122.784 Umwandlungen —
/// bei einem Browser, der sechshundert Seiten am Tag laedt, ist das
/// eine Rechnung pro Seite und keine pro Prozess.
pub const DICT_HEX: &str = include_str!("dict.hex");

/// NDBITS aus RFC 7932 (die Bit-Tiefe je Wortlaenge 0 bis 24).
pub const NDBITS: [u8; 25] = [
    0, 0, 0, 0, 10, 10, 11, 11, 10, 10, 10, 10, 10, 9, 9, 8, 7, 7, 8, 7, 7, 6, 6, 5, 5,
];

/// DOFFSET[laenge] nach der Rekursion aus RFC 7932 Abschnitt 8,
/// **berechnet** und nicht abgedruckt. Siehe den Kopf dieser Datei.
pub const DOFFSET: [u32; 26] = [
    0, 0, 0, 0, 0, 4096, 9216, 21504, 35840, 44032, 53248, 63488, 74752, 87040, 93696, 100864,
    104704, 106752, 108928, 113536, 115968, 118528, 119872, 121280, 122016, 122784,
];

/// Die Gesamtlaenge, die der RFC als 122.784 nennt.
///
/// DER WERT STEHT HIER UND NICHT NUR IM GENERATOR: ein Test
/// vergleicht ihn mit der Laenge von `DICT_HEX`, und dieser
/// Test schlaegt an, wenn jemand den Generator mit einem
/// gekuerzten RFC laufen laesst.
pub const DICTSIZE: u32 = 122784;
