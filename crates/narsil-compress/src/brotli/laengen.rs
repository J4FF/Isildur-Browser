//! Die Laengen- und Abstandstabellen aus **RFC 7932**.
//!
//! ERZEUGT, NICHT ABGETIPPT: `mutation_md/tls/brotli_dict_generator.py`.
//!
//! DREI SACHEN, die beim Lesen des RFC leicht daneben gehen,
//! und die alle drei hier ausgelesen und geprueft sind:
//!
//! 1. Die Tabellen stehen im RFC **spaltenweise**: die erste
//!    Zeile hat die Codes 0, 8, 16, die zweite 1, 9, 17. Wer
//!    sie zeilenweise uebernimmt, hat eine Tabelle, die nicht
//!    nach Code geordnet ist. Der Generator sortiert deshalb
//!    nach Code und prueft, dass die Codes 0 bis 23 ohne
//!    Luecke dastehen.
//!
//! 2. Eine Laenge kann zwei Werte haben, `6,7` oder `18..25`.
//!    Der Generator nimmt den **Anfang** des Bereichs — das ist
//!    der Wert, plus die Extra-Bits ergibt den Rest.
//!
//! 3. Fuer die Distanzen gibt es **keine** Tabelle, sondern eine
//!    Formel (Abschnitt 4). Sie steht woertlich im Kommentar
//!    von `distanz_aus_symbol`. Der Generator prueft nur, dass
//!    die vier Zeilen der Formel noch im RFC stehen.

/// Die Insert-Laengen: (Code, Extra-Bits, Anfang) — Abschnitt 5.
///
/// DER WERT IM DRITTEN FELD ist der Anfang des Bereichs, nicht
/// sein Ende: `laenge + extra_bits` ergibt die tatsaechliche
/// Laenge (Abschnitt 5).
pub const INSERT: [(u32, u32, u32); 24] = [
    (0, 0, 0),
    (1, 0, 1),
    (2, 0, 2),
    (3, 0, 3),
    (4, 0, 4),
    (5, 0, 5),
    (6, 1, 6),
    (7, 1, 8),
    (8, 2, 10),
    (9, 2, 14),
    (10, 3, 18),
    (11, 3, 26),
    (12, 4, 34),
    (13, 4, 50),
    (14, 5, 66),
    (15, 5, 98),
    (16, 6, 130),
    (17, 7, 194),
    (18, 8, 322),
    (19, 9, 578),
    (20, 10, 1090),
    (21, 12, 2114),
    (22, 14, 6210),
    (23, 24, 22594),
];

/// Die Kopie-Laengen: (Code, Extra-Bits, Anfang) — Abschnitt 5.
pub const KOPIE: [(u32, u32, u32); 24] = [
    (0, 0, 2),
    (1, 0, 3),
    (2, 0, 4),
    (3, 0, 5),
    (4, 0, 6),
    (5, 0, 7),
    (6, 0, 8),
    (7, 0, 9),
    (8, 1, 10),
    (9, 1, 12),
    (10, 2, 14),
    (11, 2, 18),
    (12, 3, 22),
    (13, 3, 30),
    (14, 4, 38),
    (15, 4, 54),
    (16, 5, 70),
    (17, 5, 102),
    (18, 6, 134),
    (19, 7, 198),
    (20, 8, 326),
    (21, 9, 582),
    (22, 10, 1094),
    (23, 24, 2118),
];
