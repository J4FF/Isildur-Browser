//! Der Brotli-Dekoder (RFC 7932).
//!
//! added 30.09.2026 bei der Uebernahme des Brotli-Auftrags von der
//! Rolle KOMPRESS.
//!
//! DIE REIHENFOLGE DER MODULE, und sie entspricht der Reihenfolge, in
//! der ein Meta-Block gelesen wird:
//!
//! | Modul            | Inhalt                                      | RFC         |
//! |------------------|---------------------------------------------|-------------|
//! | `bit_reader`     | Bit-fuer-Bit-Lesen, Bytegrenzen              | 2           |
//! | `huffman`        | die Huffman-Codes eines Alphabets            | 3, 5        |
//! | `kontext`        | Lut0/Lut1/Lut2, die vier Kontextarten        | 7.1         |
//! | `decode`         | Meta-Block-Header, Kontextkarten, Befehle    | 9.2, 9.3, 10|
//! | `dict`           | das 122.784-Byte-Woerterbuch, NDBITS        | Anhang A    |
//! | `transforms`     | die 121 Worttransformationen                | Anhang B    |
//! | `laengen`        | Insert-, Kopie- und Distanztabellen          | 4, 5        |
//! | `woerterbuch`    | Wortsuche und Transformationsanwendung       | 8           |
//! | `ring_buffer`    | das Sliding Window                           | 2           |
//!
//! `dict`, `transforms`, `kontext` und `laengen` sind **erzeugte
//! Dateien** — `mutation_md/tls/brotli_dict_generator.py` schreibt sie
//! aus `rfc/rfc7932.txt` und prueft dabei acht Sollwerte, die alle im
//! RFC stehen (zwei Laengen, zwei CRC-32, drei Tabellen-CRC-32 und die
//! Transform-CRC).

pub mod bit_reader;
pub mod decode;
pub mod dict;
pub mod huffman;
pub mod kontext;
pub mod laengen;
pub mod ring_buffer;
pub mod transforms;
pub mod woerterbuch;
