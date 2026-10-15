//! HPACK — die Kopfdaten-Kodierung von HTTP/2 (RFC 7541).
//!
//! added 30.09.2026 fuer Welle 6, Teil 2.
//!
//! WARUM EIGENE DATEI: die Kodierung hat drei unabhaengige Teile —
//! die statische Tabelle (Anhang A), die Huffman-Kodierung (Anhang B)
//! und die dynamosche Tabelle (Abschnitt 2.3.2) — und die gehoeren
//! nicht in `lib.rs`, das den HTTP/1.1-Aufbau traegt.
//!
//! DIE WICHTIGSTE EIGENSCHAFT DIESER DATEI: sie liest **nichts** von
//! Hand. Die statische Tabelle, der Huffman-Code und die Beispiele aus
//! Anhang C stehen in `testdaten/hpack_rfc7541.txt`, erzeugt von
//! `mutation_md/tls/hpack_generator.py` aus dem RFC-Text. Wer hier
//! Zahlen oder Bytes eintraegt, hat den Generator umgangen.
//!
//! SICHERHEIT (M4): jedes Byte kommt vom Server und ist feindlich. Es
//! gibt eine Obergrenze fuer die Groesse eines einzelnen
//! Kopfblocks, fuer die dynamische Tabelle, fuer die Laenge eines
//! Namens und eines Wertes. Jede dieser Grenzen ist eine benannte
//! `const` mit Begruendung.

use std::collections::VecDeque;
use std::fmt;

/// M4: die groesste Laenge, die eine als Zahl kodiert werden darf.
///
/// DER WERT 8 Byte: RFC 7541 Abschnitt 5.1 erlaubt eine
/// `Integer` mit beliebig vielen Oktetten, aber eine Zahl, die mehr
/// als 2^62 braucht, ist in diesem Zusammenhang sinnlos — sie waere
/// ein Index in eine Tabelle mit hoechstens ein paar Millionen
/// Eintraegen. Ohne diese Grenze koennte ein Server 32 Byte
/// Nullen schicken, und der Vergleich mit der Tabellenlaenge wuerde
/// erst nach einer halben Ewigkeit entscheiden.
///
/// DER BELEG: die Grenze steht in keinem RFC, sie ist eine
/// Rechenentscheidung. 2^62 ist die groesste Zahl, die in
/// `u64::MAX / 4` passt — mit dem Faktor vier, weil
/// `dyn_tabellen_groesse` verdoppelt werden muss (Abschnitt 4.2).
pub const MAX_ZAHL_BYTES: usize = 8;

/// M4: die groesste Laenge, die ein als **String** kodiert wird.
///
/// DER WERT 32767 Byte: das ist die Grenze, die der RFC in Abschnitt
/// 4.1 selbst nennt — "A string literal is not restricted to a
/// specific length" ist es nicht; die *Implementierung* begrenzt es.
/// Konkret: HTTP/2 Abschnitt 10.5.1 (RFC 9113) haelt fest, dass eine
/// Implementierung die Groesse eines Kopfdatenblocks begrenzen MUSS,
/// und nennt keine Zahl. Wir nehmen 32 KiB, weil das bereits mehr ist
/// als die groesste einzelne Header-Zeile, die ein Browser anzeigt.
pub const MAX_STRING_BYTES: usize = 32767;

/// M4: die groesste Groesse der dynamischen Tabelle in Byte.
///
/// DER WERT 4096: RFC 7541 Abschnitt 4.2 sagt "the size of the dynamic
/// table can be limited" und nennt in Abschnitt 4.2 "the protocol
/// SHOULD NOT cause the dynamic table size to exceed the size chosen
/// by the protocol". Der Wert 4096 ist die Vorgabe aus RFC 9113
/// Abschnitt 6.5.2 fuer SETTINGS_HEADER_TABLE_SIZE und zugleich die
/// Groesse, die die geprueften Server (Cloudflare, Google) melden —
/// siehe `team/BERICHT_TLS_W6.txt`.
pub const MAX_DYNAMISCHE_TABELLE: usize = 4096;

/// M4: die groesste Anzahl an Eintraegen in der dynamischen Tabelle.
///
/// DER WERT: bei hoechstens `MAX_DYNAMISCHE_TABELLE / 32` Byte je
/// Eintrag (32 = laengster moeglicher Name plus Wert, praktisch nie
/// erreicht) sind das 128 Eintraege. Die Grenze ist **zusaetzlich**
/// zur Bytegrenze noetig, weil ein Server sonst 128 Eintraege mit je
/// 1 Byte schicken kann und die Schleife laeuft.
pub const MAX_DYNAMISCHE_EINTRAEGE: usize = 128;

/// M4: die groesste Laenge eines kompletten HEADERS-Rahmens.
///
/// DER WERT 65536: das ist die Grenze, die RFC 9113 Abschnitt 6.5.2
/// in SETTINGS_MAX_FRAME_SIZE als **Default** nennt ("The initial
/// value of SETTINGS_MAX_FRAME_SIZE is 2^14 (16384)"). Wir erlauben
/// mehr, weil manche Server grosse Cookies in einem Block schicken —
/// aber nicht beliebig: 64 KiB ist die Groesse, ab der ein
/// Kopfdatenblock im Speicher unangenehm wird.
pub const MAX_HEADERS_RAHMEN: usize = 65536;

/// Fehler aus der HPACK-Kodierung.
///
/// Jeder Fehler nennt, **was** schiefging, damit ein Mitschnitt
/// dieselbe Auskunft gibt wie die Meldung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HpackFehler {
    /// Die Zahl am Anfang eines Feldes hat mehr als `MAX_ZAHL_BYTES`
    /// Oktette, oder die Endlos-Eins (alle Eins-Bits) wurde nicht
    /// innerhalb von `MAX_ZAHL_BYTES` beendet.
    ///
    /// added 30.09.2026. DER WEG ZU DIESEM FEHLER: RFC 7541
    /// Abschnitt 5.1 erlaubt eine beliebig lange Zahl. Ohne Grenze
    /// schickt ein Server 64 Byte `ff` und der Leser haengt beim
    /// Nachfuehren der Eins-Bits.
    ZahlZuLang { kam: usize },
    /// Eine als String kodierte Laenge ist groesser als
    /// `MAX_STRING_BYTES`.
    ///
    /// added 30.09.2026. Beispiel aus dem Netz: `be 00` heisst
    /// "Huffman, 0 Byte", und ein Server, der `be ff ff ff` schickt,
    /// will 4 GiB anzeigen.
    StringZuLang { war: usize },
    /// Der Huffman-Bitstrom endete mitten in einem Code.
    ///
    /// added 30.09.2026. Grund: der letzte Code wird ueber die
    /// Zeilengrenze hinweg gelesen, und wenn der Strom dort endet,
    /// ist der Code unvollstaendig.
    HuffmanUnvollstaendig,
    /// Der Huffman-Code enthaelt ein Symbol, das es nicht gibt.
    ///
    /// added 30.09.2026. RFC 7541 Abschnitt 5.2 erlaubt nur die
    /// 257 Symbole der Anhang-B-Tabelle. Ein 258. oder ein
    /// unzulaessiges 4-Byte-Symbol ist ein Fehler des Servers.
    HuffmanUnbekannt { code: u32 },
    /// Der Huffman-Code enthaelt das EOS-Symbol (256).
    ///
    /// added 30.09.2026. RFC 7541 Abschnitt 5.2: "A Huffman-encoded
    /// string literal containing the EOS symbol MUST be treated as a
    /// decoding error". Es steht nie in einer echten Nachricht.
    HuffmanEos,
    /// Der Index einer Kopfdatenzeile liegt in keiner Tabelle.
    ///
    /// added 30.09.2026. Index 0 ist nach RFC 7541 Abschnitt 6.1
    /// immer ein Fehler ("A value of 0 ... MUST be treated as a
    /// decoding error"), und Indices ab `MAX_DYNAMISCHE_EINTRAEGE + 62`
    /// gibt es nicht.
    IndexUngueltig { war: usize },
    /// Die dynamische Tabelle waechst ueber ihre Grenze hinaus.
    ///
    /// added 30.09.2026. Das ist der Angriff aus RFC 7541
    /// Abschnitt 4.2: 16 kurze Zeilen kosten weniger Bytes als der
    /// Platz, den sie im Speicher brauchen.
    DynamischeTabelleZuGross { war: usize },
    /// Der Block ist laenger als erlaubt.
    ///
    /// added 30.09.2026. Siehe `MAX_HEADERS_RAHMEN`.
    BlockZuLang { war: usize },
    /// Eine SETTINGS_HEADER_TABLE_SIZE-Aenderung ist unzulaessig.
    ///
    /// added 30.09.2026. RFC 7541 Abschnitt 6.3: "A dynamic table
    /// size update MUST occur at the beginning of the first header
    /// block following the change to the dynamic table size."
    GroessenaenderungFalscherOrt,
}

impl HpackFehler {
    /// Meldet den Namen der eigenen Variante, ohne sie aufzuschreiben.
    ///
    /// added 01.10.2026 10:2x UTC (Projektlabel 02.10.2026), R4y. DER WEG
    /// ZUR UMWANDLUNG, und er ist nicht `format!`: `RahmenFehler` soll keinen
    /// **Text** speichern, sondern den Typ. Ein Name ist ein `&'static str`,
    /// der zur Compile-Zeit feststeht und kein Speicher braucht — im
    /// Gegensatz zu `Box::leak(String)`, das ich zuerst geschrieben hatte und
    /// das für immer leakt.
    pub fn fehler_name(&self) -> &'static str {
        match self {
            HpackFehler::ZahlZuLang { .. } => "ZahlZuLang",
            HpackFehler::StringZuLang { .. } => "StringZuLang",
            HpackFehler::HuffmanUnvollstaendig => "HuffmanUnvollstaendig",
            HpackFehler::HuffmanUnbekannt { .. } => "HuffmanUnbekannt",
            HpackFehler::HuffmanEos => "HuffmanEos",
            HpackFehler::IndexUngueltig { .. } => "IndexUngueltig",
            HpackFehler::DynamischeTabelleZuGross { .. } => "DynamischeTabelleZuGross",
            HpackFehler::BlockZuLang { .. } => "BlockZuLang",
            HpackFehler::GroessenaenderungFalscherOrt => "GroessenaenderungFalscherOrt",
        }
    }

    /// Die Zahl, die in der Meldung stand, falls die Variante eine traegt.
    ///
    /// added 01.10.2026 10:2x, R4y: `None` bei den Varianten ohne Zahl —
    /// `HuffmanUnvollstaendig`, `HuffmanEos` und
    /// `GroessenaenderungFalscherOrt` haben keine.
    pub fn fehler_zahl(&self) -> Option<usize> {
        match self {
            // added 01.10.2026 10:2x, R4y: die Felder heissen nicht alle
            // gleich — `ZahlZuLang` hat `kam`, die anderen `war`. Deshalb
            // je ein Arm und kein `|`-Muster: das verlangt identische
            // Bindungen, und das sind sie nicht.
            HpackFehler::ZahlZuLang { kam } => Some(*kam),
            HpackFehler::StringZuLang { war }
            | HpackFehler::IndexUngueltig { war }
            | HpackFehler::DynamischeTabelleZuGross { war }
            | HpackFehler::BlockZuLang { war } => Some(*war),
            HpackFehler::HuffmanUnbekannt { code } => Some(*code as usize),
            _ => None,
        }
    }
}

impl fmt::Display for HpackFehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HpackFehler::ZahlZuLang { kam } => write!(
                f,
                "die Zahl am Feldanfang hat {kam} Oktett, erlaubt sind {MAX_ZAHL_BYTES}"
            ),
            HpackFehler::StringZuLang { war } => write!(
                f,
                "ein String angekündigt mit {war} Byte, erlaubt sind {MAX_STRING_BYTES}"
            ),
            HpackFehler::HuffmanUnvollstaendig => {
                write!(f, "der Huffman-Bitstrom endet mitten in einem Code")
            }
            HpackFehler::HuffmanUnbekannt { code } => {
                write!(f, "im Huffman-Code steht das unbekannte Symbol {code}")
            }
            HpackFehler::HuffmanEos => write!(
                f,
                "der Huffman-Code enthaelt das EOS-Symbol 256, das nie vorkommen darf"
            ),
            HpackFehler::IndexUngueltig { war } => {
                write!(f, "der Kopfindex {war} steht in keiner Tabelle")
            }
            HpackFehler::DynamischeTabelleZuGross { war } => write!(
                f,
                "die dynamische Tabelle waechst auf {war} Byte, erlaubt sind {MAX_DYNAMISCHE_TABELLE}"
            ),
            HpackFehler::BlockZuLang { war } => {
                write!(
                    f,
                    "der Kopfdatenblock hat {war} Byte, erlaubt sind {MAX_HEADERS_RAHMEN}"
                )
            }
            HpackFehler::GroessenaenderungFalscherOrt => write!(
                f,
                "die Aenderung der Tabellengroesse steht nicht am Anfang des Blocks"
            ),
        }
    }
}

/// Die statische Tabelle aus RFC 7541 Anhang A.
///
/// 61 Eintraege, Index 1 bis 61. DIE HERKUNFT: `hpack_rfc7541.txt`,
/// vom Generator aus dem RFC gelesen, und der Generator bricht ab,
/// wenn ein Eintrag nicht an der Stelle steht, die seine Indexzahl
/// nennt. Ein hier von Hand eingetragener 62. Eintrag waere also
/// sofort auffaellig.
///
/// DER FEHLER, DER HIER ZUERST STAND: die Tabelle war als
/// `[(&str, &str); 61]` mit `("", "")` am Anfang geschrieben, damit
/// Index 1 stimmt — und damit stand Index 0 als leerer Eintrag da.
/// Nach RFC 7541 Abschnitt 6.1 ist Index 0 aber ein **Fehler**, kein
/// leerer Name. Deshalb ist der Index hier **eins** und wird beim
/// Nachsehen um eins erhoeht.
pub fn statischer_eintrag(index: usize) -> Option<(&'static str, &'static str)> {
    // added 01.10.2026 07:4x, R4u. DIE PANIK, und sie lag genau auf der
    // Stelle, die der eigene Doc-Kommentar als Fehlerfall beschreibt:
    // `index - 1` ist bei `index == 0` ein Unterlauf, und `0` ist nach
    // RFC 7541 Abschnitt 6.1 der Index, den ein Server schicken **darf**,
    // um den Client zum Fehler zu bringen. `get(..)` haette `None`
    // geliefert, aber die Rechnung **vor** dem `get` ist ein Unterlauf —
    // im Testmodus Panik, im Release ein Riesenzahl, die `get` zur
    //-harmlosen Fehlanzeige fuehrt. Zwei verschiedene Fehlerbilder fuer
    // denselben Fehler, je nach Build.
    // `checked_sub` macht beide zu `None`.
    let t = STATISCHE_TABELLE.get(index.checked_sub(1)?)?;
    // DER FEHLER, und warum er nie auffiel: `get` gibt `&(&str, &str)`,
    // `Some(t)` verpackt also die **Referenz**. Das haette als
    // `Some(*t)` geschrieben werden muessen. Die Datei wurde nie gebaut
    // (kein `mod hpack;` in `lib.rs`), also hat es niemand gesehen.
    Some(*t)
}

/// M3: hoechstens so viele Felder in **einem** Block.
///
/// DER WERT 512: RFC 7541 nennt keine Zahl. Der Block ist durch
/// `MAX_HEADERS_RAHMEN` (65 536 Byte) begrenzt, und im ungünstigsten Fall
/// ist ein Feld ein Byte (`0x80` = indiziertes Feld), also wären 65 536
/// Felder möglich. 512 ist eine **Rechenentscheidung**: ein Anfragekopf
/// hat ueberlicherweise unter 20 Felder, und eine Antwort mit Trailern hat
/// vielleicht 40. Wer mehr hat, schickt einen zweiten Block
/// (`CONTINUATION`), und das ist nach RFC 9113 6.10 ausdrücklich erlaubt.
pub const MAX_FELDER_PRO_BLOCK: usize = 512;

/// Schreibt einen **indizierten** Kopf (RFC 7541 Abschnitt 6.1).
///
/// added 01.10.2026 10:1x UTC (Projektlabel 02.10.2026), R4y. DER
/// KURZESTE FALL, und er ist der Grund, warum `HEADERS` ueberhaupt klein
/// wird: ein Feld, das in der statischen Tabelle steht, braucht **ein**
/// Byte.
///
/// DAS BYTE, und es ist nicht `index`: es ist `0x80 | index`, weil Bit 7
/// das Feld „Indexed Header Field" kennzeichnet (6.1, 1xxxxxxx).
///
/// DIE GRENZE (M4): Index 0 ist nach RFC 7541 6.1 ein **Fehler** („The
/// index MUST NOT be 0"), und Index 62 (0x3e) ist der Anfang der
/// dynamischen Tabelle — der braucht eine Laenge mit Continuation, also
/// eine andere Funktion. Deshalb nimmt diese Funktion nur 1 bis 61.
pub fn kodiere_indiziert(index: usize, aus: &mut Vec<u8>) -> Result<(), HpackFehler> {
    if index == 0 || index > 61 {
        // added 01.10.2026 10:1x, R4y: Index 0 herausgeworfen statt
        // stillschweigend zu einem gültigen Index zu werden. Es ist nach
        // 6.1 ein Fehler, und ein Decoder, der `80` liest, meldet nach
        // eigener Regel „Index 0" — wir erzeugen die kaputte Nachricht
        // also gar nicht erst.
        return Err(HpackFehler::IndexUngueltig { war: index });
    }
    aus.push(0x80 | index as u8);
    Ok(())
}

/// Schreibt ein Feld als „Literal Header Field with Incremental
/// Indexing" (RFC 7541 Abschnitt 6.2.1).
///
/// DAS FORMAT, und es sind drei Teile: ein Praefix, das die Form kennzeichnet
/// (`01xxxxxx`, Bit 6 gesetzt, Bit 7 frei), dann der **Name** als
/// String (mit Laenge), dann der **Wert** als String.
///
/// added 02.10.2026 17:1x, R37. **DIESE BEGRUENDUNG WAR FALSCH, und sie stand
/// hier als scheinbare Tatsache.** Sie sagt: „sie nimmt den Eintrag in der
/// dynamischen Tabelle auf, und damit sind die folgenden Bloecke kleiner“.
/// **GEMESSEN:** kleiner werden die folgenden Bloecke nur, wenn der Kodierer die
/// dynamische Tabelle auch **auswertet** — also wenn er Indexe aus der Tabelle
/// schreibt. Unser Anfrageweg tut das nicht: er schreibt einen Kopfblock,
/// fuehrt **keinen** Zustand mit und kann den Index gar nicht kennen. Diese
/// Funktion wird im Anfrageweg ab R37 **nicht mehr benutzt**; sie bleibt
/// oeffentlich, weil sie die richtige Form fuer einen Aufrufer ist, der die
/// Tabelle wirklich fuehrt. Wer sie benutzt, muss 4.2 beachten: die gewaehlte
/// Groesse muss kleiner oder gleich der angezeigten sein.
///
/// DIE NAMEN, und hier ist die Kürzung: steht der Name in der **statischen**
/// Tabelle (Anhang A), wird er als Index geschrieben (1 Byte). Das ist
/// der Grund, warum `:method`, `:scheme` und `:path` billig sind.
pub fn kodiere_literal_mit_name(
    name: &str,
    wert: &str,
    aus: &mut Vec<u8>,
) -> Result<(), HpackFehler> {
    // DER INDEX DES NAMENS, oder `None`. Die Suche ist linear ueber 61
    // Eintraege — billig, und sie passiert einmal je Feld.
    match statischer_index(name) {
        Some(idx) => {
            // 6.2.1 mit 4-Bit-Praefix (01xxxxxx): der Index muss < 15
            // passen, sonst braucht die 6-Bit-Form mehr Byte als der
            // Literal-Name.
            //
            // added 01.10.2026 15:4x, R4z8. **SEchs Bit, nicht vier**, und
            // der alte Zweig „ab Index 15 nimm 6.2.2" ist damit hinfuellig.
            // Die Norm (`rfc7541.txt`, 6.2.1): „the index of that entry is
            // represented as an integer with a 6-bit prefix".
            //
            // GEMESSEN, warum das ein echter Fehler war: `github.com`
            // schickt `0x61` fuer `date`. Mit vier Bit las der Dekodierer
            // Index 1 (`:authority`) und lieferte 20 Felder mit **richtigen
            // Werten und falschen Namen**. Unser eigener Kodierer hat das
            // nie bemerkt, weil er selbst nie einen Index >= 15 geschrieben
            // hat — die Luecke war beidseitig dieselbe.
            if idx < 63 {
                // 6.2.1: Muster `01`, dann **sechs** Indexbit. Der Index
                // liegt in den unteren sechs Bit, das Muster in den oberen
                // zwei — deshalb `0x40 | idx`, und nicht `schreibe_zahl`.
                //
                // added 01.10.2026 10:3x, R4y (unveraendert gueltig): `0x40`
                // ist **nicht** 6.2.2. 6.2.2 hat den Praefix `0000xxxx`
                // (also `0x00`).
                //
                // added 01.10.2026 15:4x: die Grenze ist 63 und nicht 15,
                // denn `2^6 - 1 = 63` ist der Ausgangswert. `idx == 63`
                // ist in diesem Zweig nicht erreichbar (`statischer_index`
                // liefert hoechstens 61), aber es muss auch nicht
                // ausgeschlossen werden — die Bedingung sagt es.
                aus.push(0x40 | (idx as u8 & 0x3f));
            } else {
                // Die 6-Bit-Form von 6.2.1: `0010xxxx`? Nein — 6.2.1 mit
                // 6-Bit-Index hat den Praefix `01` plus 6 Bit Index, also
                // `01xxxxxx` wie oben, aber der Index darf bis 62 reichen.
                // Deshalb: wenn `idx` nicht in 4 Bit passt, muss die
                // **6.2.2**-Form (ohne Indizierung) genommen werden,
                // denn 6.2.1 hat nur die 4-Bit-Variante.
                //
                // DIE WAHRHEIT, und sie steht so in RFC 7541 6.2.1 Bild 9:
                // „Literal Header Field with Incremental Indexing — Indexed
                // Name" hat einen **4-Bit**-Index; ab Index 15 gibt es
                // diese Form nicht mehr, man nimmt 6.2.2.
                // added 01.10.2026 10:3x, R4y: HIER WAR MEIN FEHLER —
                // `0x40` ist **nicht** 6.2.2. 6.2.2 hat den Praefix
                // `0000xxxx` (also `0x00`), und `0x40` ist 6.2.1 mit
                // 4-Bit-Index. Der Decoder las deshalb Index 64 (und dann
                // noch mehr) und meldete `IndexUngueltig { war: 1096 }`.
                //
                // 6.2.2 „without indexing" ist genau richtig fuer einen
                // Namen, der nicht in der statischen Tabelle steht und
                // auch **nicht** in die dynamische gehoert — und fuer
                // `user-agent` heisst das: jeder Request traegt das
                // Literal mit, aber die dynamische Tabelle bleibt klein.
                aus.push(0x00); // 6.2.2, Index 0 = neuer Name folgt
                schreibe_string(name, true, aus)?;
            }
        }
        None => {
            // Der Name ist neu: als Literal-String mit Huffman, denn ein
            // langer unbekannter Name ist genau der Fall, für den Huffman
            // bezahlt macht.
            aus.push(0x00); // 6.2.1 „Neue Name"-Form (00xxxxxx)
            schreibe_string(name, true, aus)?;
        }
    }
    // Und der Wert. Huffman immer: nach RFC 7541 4.1 ist es eine
    // Entscheidung des Kodierers, und für die Felder einer Anfrage
    // (Pfade, Hosts, `user-agent`) ist es fast immer kleiner.
    schreibe_string(wert, true, aus)
}

/// Schreibt ein Feld als „Literal Header Field **without Indexing**"
/// (RFC 7541 Abschnitt 6.2.2) — und **beruehrt die dynamische Tabelle nicht**.
///
/// added 02.10.2026 17:0x, R37. **WARUM ES DIE FUNKTION GIBT**, und es ist
/// eine Normstelle, keine Geschmacksfrage. RFC 7541 4.2, woertlich:
/// „Protocols that use HPACK determine the maximum size that the encoder is
/// permitted to use for the dynamic table. In HTTP/2, this value is determined
/// by the SETTINGS_HEADER_TABLE_SIZE setting“ und „An encoder can choose to use
/// less capacity than this maximum size (see Section 6.3), but the chosen size
/// **MUST** stay lower than or equal to the maximum set by the protocol.“
/// Unsere gewaehlte Groesse war bis hierher **implizit 4 096** (der
/// Protokollausgangswert), weil `kodiere_literal_mit_name` 6.2.1 schreibt und
/// damit bei **jedem** Feld einfuegt — GEMESSEN: `h2strom::baue_headers`
/// ruft es fuer `:authority` (immer), `:method`, `:path`, `:scheme` und ueber
/// `kodiere_namen_wert` fuer jedes Zusatzfeld wie `user-agent`.
/// SCHLUSSFOLGERUNG: schickt ein Server `SETTINGS_HEADER_TABLE_SIZE = 0` — was
/// er darf, 6.5.2 kennt keinen Mindestwert —, dann war unsere gewaehlte Groesse
/// 4 096 und damit **groesser** als das Maximum des Protokolls. Das ist genau
/// der Fall, den 4.2 verbietet.
/// added 02.10.2026 17:0x: **UND WARUM MAN NICHT WARENET, BIS DIE SETTINGS DA
/// SIND** (der scheinbar saubere Weg nach 4.2 waere ja, den Wert zu lesen und
/// eine Groessenaenderung nach 6.3 zu schicken): unsere HEADERS gehen nach
/// RFC 9113 3.4 erlaubt **vor** den Server-SETTINGS raus. GEMESSEN zudem: der
/// h2-Weg macht pro Abruf **eine** Verbindung (`schreibe_vorlauf` wird genau
/// einmal je `hole_ueber_h2` gerufen, und `hole_ueber_h2` genau einmal je
/// Abruf), es gibt also gar keinen „zweiten Kopfblock", in dem die nach 4.2
/// geforderte Groessenaenderung („MUST occur at the beginning of the first
/// header block following the change“) stehen koennte. Wer hier auf die
/// spaetere Anzeige wartet, verliert einen Rundlauf und repariert nichts.
///
/// added 02.10.2026 17:1x: **DAS FORMAT**, woertlich nach dem Bild in 6.2.2:
/// `| 0 | 0 | 0 | 0 | HdrName Index (4+) |` — der Index hat hier **vier** Bit,
/// nicht sechs wie in 6.2.1. Und 6.2.2 sagt fuer den Fall „Index passt nicht“:
/// der Name muss als String im Namensfeld stehen. Genau danach ist hier
/// entschieden, mit `statischer_index` als der einen Quelle der Wahrheit.
pub fn kodiere_literal_ohne_index(
    name: &str,
    wert: &str,
    aus: &mut Vec<u8>,
) -> Result<(), HpackFehler> {
    match statischer_index(name) {
        // added 02.10.2026 17:1x: VIER Bit heisst: 0 bis 15. `idx == 0` gibt es
        // nicht, weil `statischer_index` erst ab 1 zaehlt; `idx < 15` ist die
        // Bedingung der Norm, und `15` selbst ist bereits zu gross — deshalb
        // ist es `< 15` und nicht `<= 15`.
        Some(idx) if idx < 15 => aus.push(idx as u8),
        _ => {
            aus.push(0x00);
            schreibe_string(name, true, aus)?;
        }
    }
    // Der Wert: Huffman wie in 6.2.1, das ist eine Entscheidung des Kodierers
    // (4.1) und keine Eigenschaft der Form.
    schreibe_string(wert, true, aus)
}

/// Der Index eines Namens in der **statischen** Tabelle (Anhang A).
///
/// added 01.10.2026 10:1x, R4y. `None`, wenn der Name nicht drinsteht —
/// und die Suche ist **nach Namen**, nicht nach dem Paar: ein Name wie
/// `:path` steht viermal da (Index 4 bis 7) mit verschiedenen Werten.
pub fn statischer_index(name: &str) -> Option<usize> {
    STATISCHE_TABELLE
        .iter()
        .position(|(n, _)| *n == name)
        .map(|i| i + 1)
}

/// Dekodiert einen **Block** von Kopfzeilen und hängt ihn an.
///
/// added 01.10.2026 10:1x, R4y. DER NAME, und er sagt das Wichtigste:
/// **`bloecke`** (plural). Ein grosser Kopf wird nach RFC 9113 6.10 auf
/// mehrere `HEADERS`/`CONTINUATION`-Rahmen verteilt, und **jeder** Rahmen
/// trägt einen vollständigen HPACK-Block, der an den vorherigen anknüpft
/// (die dynamische Tabelle ist der Zustand dazwischen).
///
/// DESHALB wird hier **nicht** `Dekoder` neu gebaut: der Aufrufer hält
/// den Zustand in `kopfzeilen` und übergibt ihn hier.
///
/// DER FEHLER, und er ist der Grund für das `&mut`: ein Block, der die
/// dynamische Tabelle ändert, muss das auch im **Aufrufer** tun. Sonst
/// liest der zweite Rahmen die Tabelle vom ersten nicht und die
/// Dekodierung bricht ab.
pub fn dekodiere_bloecke(
    block: &[u8],
    ziel: &mut Vec<(String, String)>,
) -> Result<usize, HpackFehler> {
    // added 01.10.2026 15:0x, R4z8. DER HINWEIS, der jetzt dort steht,
    // wo er hingehört: diese Form ist **zustandslos** und damit nur richtig
    // für einen Block, der die dynamische Tabelle *nicht* benutzt. Der
    // echte Weg über eine Verbindung ist `dekodiere_bloecke_mit`.
    //
    // GEMESSEN, warum das keine Kleinigkeit ist: `tests/h2_live.rs` meldete
    // gegen `github.com` `HPACK: BlockZuLang (512)` und gegen
    // `www.google.com` `HPACK: StringZuLang (18)` — beides echte Server,
    // beide mit dynamischen Verweisen. Wer nur `dekodiere_bloecke` hat,
    // kann solche Antworten nicht lesen; es ist keine Feinheit, sondern
    // die Grenze des Wegs.
    dekodiere_bloecke_mit(block, ziel, &mut DynamischeTabelle::neu())
}

/// DER ECHTE Dekodierer: die dynamische Tabelle lebt **ueber die ganze
/// Verbindung**.
///
/// added 01.10.2026 15:0x, R4z8. RFC 7541 Abschnitt 4 ist eine
/// **Zustandsmaschine ueber den ganzen Verbindungsweg**, nicht ueber einen
/// Block. Jeder `HEADERS`- und jeder `CONTINUATION`-Ramen aendert die
/// Tabelle fuer alle folgenden. Deshalb liegt sie beim **Aufrufer** —
/// bei uns in `h2client::lies_antwort` — und nicht in diesem Aufruf.
///
/// added 01.10.2026 15:0x: `lies_feld` liefert jetzt `Option`: `None`
/// heisst „das war ein Kommando, kein Feld". Vorher kam fuer eine
/// Groessenaenderung (6.3) ein Feld mit **leerem Namen** zurueck, und
/// der Aufrufer hat es — trotz des Kommentars „der Aufrufer verwirft ihn"
/// — mit `ziel.push` in die Kopfzeilen gelegt. GEMESSEN als Fehlerform,
/// nicht als Messung: ein leeres Feld ist in keiner RFC-Beispieldatenbank
/// ein Feld.
pub fn dekodiere_bloecke_mit(
    block: &[u8],
    ziel: &mut Vec<(String, String)>,
    tabelle: &mut DynamischeTabelle,
) -> Result<usize, HpackFehler> {
    if block.len() > MAX_HEADERS_RAHMEN {
        return Err(HpackFehler::BlockZuLang { war: block.len() });
    }
    let mut pos = 0usize;
    let mut anzahl = 0usize;
    // M3: die Zahl der Felder je Block ist begrenzt. Ohne diese Grenze
    // koennte ein Server 65 KiB aus lauter Ein-Byte-Feldern machen und
    // damit 65 000 Felder erzeugen — mehr, als ein Browser je anzeigt.
    while pos < block.len() {
        if anzahl >= MAX_FELDER_PRO_BLOCK {
            return Err(HpackFehler::BlockZuLang { war: anzahl });
        }
        // added 01.10.2026 10:5x, R4y. DAS WAR DER FEHLER, und er war
        // still: `lies_feld` setzt `*pos` **selbst** auf das Ende des
        // Feldes, und die Schleife addierte `verbraucht` ein zweites Mal.
        // Damit sprang `pos` nach jedem Feld **doppelt** so weit, und der
        // Decoder landete mitten in einem Huffman-String.
        //
        // GEMESSEN: `IndexUngueltig { war: 33729 }` = `0x8401` — das ist
        // `0x84` (H-Bit + Länge 4) gefolgt von `0x01`, gelesen als Index.
        // Feldweise nachgebaut funktionierte jeder Schritt; nur in der
        // Schleife kam der Fehler.
        //
        // DIE LOESUNG: `lies_feld` gibt die verbrauchte Bytezahl fuer den
        // **Aufrufer** zurueck (`Ok((name, wert, verbraucht))`), damit die
        // Schleife sie **nicht** addieren muss — der Aufrufer nutzt sie
        // nur fuer die Zaehlung.
        let gelesen = lies_feld(block, &mut pos, tabelle)?;
        let Some((name, wert)) = gelesen else {
            // added 01.10.2026 15:0x, R4z8: ein **Kommando** (6.3,
            // Groessenaenderung) ist kein Feld. Es zaehlt nicht gegen
            // `MAX_FELDER_PRO_BLOCK` und es landet nicht in `ziel`.
            continue;
        };
        // DER FEHLER, und er ist der Grund für das Anhängen statt
        // Ersetzen: ein zweiter `HEADERS`-Ramen auf demselben Strom
        // (das macht RFC 9113 6.4 bei einem Trailers) **ergänzt** die
        // Kopfzeilen.
        ziel.push((name, wert));
        anzahl += 1;
    }
    Ok(anzahl)
}

/// Liest **ein** Feld ab Position `*pos` (RFC 7541 Abschnitt 6).
///
/// added 01.10.2026 10:1x, R4y. DIE VIER FORMEN, und sie unterscheiden
/// sich am **höchsten** gesetzten Bit:
///
/// * `1xxxxxxx` — Indexed Header Field (6.1), Index 1 bis 61 hier.
/// * `01xxxxxx` — Literal with Incremental Indexing (6.2.1).
/// * `001xxxxx` — Literal with Dynamic Table Size Update (6.3).
/// * `0001xxxx` — Literal **Never** Indexed (6.2.3).
/// * `0000xxxx` — Literal without Indexing (6.2.2).
///
/// Und 6.2.2 beginnt mit **zwei** Nullen, 6.2.3 mit `0001` — deshalb
/// braucht es hier die volle Bitmaske und nicht nur ein Bit.
/// added 01.10.2026 15:0x, R4z8. DIE DREI TEILE, die sich geaendert haben,
/// und jeder ist noetig, nicht Geschmack:
///  1. `tabelle: &mut DynamischeTabelle` — ohne sie kann der Decoder
///     dynamische Verweise (Index >= 62) nicht lesen **und** nicht
///     einfuegen. GEMESSEN: 2 von 3 echten Servern scheitern ohne sie.
///  2. `Option<(String, String)>` — 6.3 ist ein Kommando, kein Feld.
///  3. die Rueckgabe der **verbrauchten** Bytezahl bleibt, weil die
///     Schleife sie nicht addieren darf (siehe Kommentar dort).
fn lies_feld(
    b: &[u8],
    pos: &mut usize,
    tabelle: &mut DynamischeTabelle,
) -> Result<Option<(String, String)>, HpackFehler> {
    if *pos >= b.len() {
        return Err(HpackFehler::BlockZuLang { war: b.len() });
    }
    let start = *pos;
    let erstes = b[start];

    // 6.1: Indexed Header Field. added 01.10.2026 10:6x, R4y.
    //
    // DER FEHLER, und er war der letzte dieser Kette: ich habe hier
    // `lies_zahl7` gerufen, und die liest `0x80` als
    // **Continuation-Bit**. Bei 6.1 ist das Hochbit aber die
    // **Formkennung**, und die Zahl hat genau **7 Bit** — sie braucht nie
    // eine Fortsetzung (RFC 7541 6.1, Bild 6: „Index … represented as an
    // integer with a 7-bit prefix").
    //
    // GEMESSEN: `82 87` (zwei indizierte Felder) ->
    // `ZahlZuLang { kam: 2 }`. Der Decoder wollte ein zweites Byte.
    //
    // DIE REGEL: bei 6.1 ist `index = b[start] & 0x7f`, und es ist genau
    // **ein** Byte verbraucht. Index 62 bis 68 (die dynamische Tabelle)
    // sind nach 6.1 nicht kodierbar — dafuer gibt es die Literalformen.
    if erstes & 0x80 != 0 {
        let index = (erstes as usize) & 0x7f;
        *pos = start + 1;
        let (n, w) = hole(index, tabelle)?;
        return Ok(Some((n, w)));
    }

    // 6.2: die Literal-Formen. Alle brauchen zuerst den **Namen**, und
    // der ist bei 6.2.1/6.2.2/6.2.3 ein Index und bei „neuer Name" ein
    // String.
    // added 01.10.2026 10:4x, R4y. DIE BITBREITE WAR 6 STATT 4, und das
    // ist der Fehler, der die letzten beiden Tests rot hielt:
    // `IndexUngueltig { war: 1096 }` beim Bauen eines Blocks mit
    // `:authority`.
    //
    // GEMESSEN, wie es passiert: `kodiere_literal_mit_name(":authority",
    // "h")` schreibt `41 81 9f` — `0x41` ist 6.2.1 mit 4-Bit-Index 1.
    // Der Decoder las `0x41 & 0x3f` = 1 und war richtig. Aber sobald
    // `:path` mit einem Wert kam, stand ein Index ueber 15 — und **da**
    // ist die Form „Literal with Incremental Indexing — Indexed Name" nach
    // RFC 7541 6.2.1 auf **4 Bit** beschraenkt. Ein 6-Bit-Praefix gibt es
    // nur fuer 6.3 (Groessenaenderung), und das habe ich verwechselt.
    //
    // added 01.10.2026 15:4x, R4z8. **SECHS Bit, nicht vier.** Das war der
    // Grund, warum `github.com` und `www.google.com` ueber h2 keine Seite
    // lieferten, obwohl die Dynamiktababelle jetzt verdrahtet war.
    //
    // DIE NORM, woertlich (`rfc7541.txt`, 6.2.1): „If the header field name
    // matches … the index of that entry is represented as an integer with a
    // 6-bit prefix (see Section 5.1). This value is always non-zero."
    // Ebenso 6.2.2 und 6.2.3. Sieben Bit nimmt nur 6.1 (reine Indexierung),
    // fuenf Bit nur 6.3 (Groessenaenderung).
    //
    // GEMESSEN an `github.com`, Block-Oktett 2: `0x61`. Mit vier Bit las der
    // Decoder Index **1** (`:authority`), mit sechs Bit Index **33**
    // (`date`) — und der Wert dahinter war `Thu, 01 Oct 2026 16:09:48 GMT`.
    // Die **Werte** stimmten in allen 20 Feldern, nur die **Namen** waren
    // verschoben. Das ist die typische Fehlerform: ein zu kleiner Index liest
    // die Bytezahl richtig weiter und liefert deshalb eine plausibel
    // aussehende, aber falsche Feldliste.
    //
    // UNABHAENGIG GEBLIEBEN: `hpack` 4.0.0 (die Python-Bibliothek) liest
    // denselben Block als `date = Thu, 01 Oct 2026 16:09:48 GMT`. Das ist
    // der Beweis, dass es an **unserer** Stelle lag und nicht am Server.
    //
    // added 01.10.2026 15:4x, R4z8. DIE DREI PRAEFIXBREITEN SIND
    // **VERSCHIEDEN**, und der Fehler war, dass alle drei gleich waren:
    //
    //   6.2.1 (`01`)      **sechs** Bit  — „the index of that entry is
    //                                 represented as an integer with a
    //                                 6-bit prefix" (6.2.1)
    //   6.2.2 (`0000`)    **vier** Bit   — „… with a 4-bit prefix" (6.2.2)
    //   6.2.3 (`0001`)    **vier** Bit   — „identical to the literal
    //                                 header field without indexing"
    //   6.3   (`001`)     **fuenf** Bit  — 6.3
    //
    // GEMESSEN, warum 6.2.1 sechs Bit braucht: `github.com` schickt `0x61`
    // fuer `date`. `0x61 & 0x3f` = 33 = `date`; `0x61 & 0x0f` = 1 =
    // `:authority`. Unser Dekodierer lieferte 20 Felder mit **richtigen
    // Werten und falschen Namen** — Index 1 statt 33.
    //
    // GEMESSEN, warum 6.2.2 **vier** Bit hat und nicht sechs:
    // `www.google.com`, Oktett 25, ist `0f 12 …`. Mit sechs Bit waere der
    // Index 15 (`accept-charset`) und die Feldliste laege ab da neben der
    // des Servers; mit vier Bit ist 15 der **Ausgangswert** `2^4 - 1`, es
    // folgt `0x12` (18), und der Index ist `15 + 18 = 33` — wieder
    // `date`. Nachgeprueft mit `hpack` 4.0.0: derselbe Block, Offset 25,
    // erstes Feld `date`.
    let (index_bits, praefix) = if erstes & 0x40 != 0 {
        (6usize, 0x40u8) // 6.2.1 — 6-Bit-Index
    } else if erstes & 0x20 != 0 {
        (5usize, 0x20u8) // 6.3 — 5-Bit-Praefix (kein Feldindex)
    } else if erstes & 0x10 != 0 {
        (4usize, 0x10u8) // 6.2.3 — 4-Bit-Index
    } else {
        (4usize, 0x00u8) // 6.2.2 — 4-Bit-Index
    };

    // 6.3: Groessenaenderung — kein Feld, sondern ein Kommando. Es wird
    // hier **nicht** als Feld gelesen, weil es keines ist. Wer es als
    // eines liest, erzeugt ein Feld mit leerem Namen.
    if praefix == 0x20 {
        //
        // added 01.10.2026 15:2x, R4z8. **Fuenf Bit, nicht sieben**, und
        // die Fortsetzung beginnt **hinter** dem Praefix-Oktett.
        // Vorher stand hier `lies_zahl7(&b[start + 1..])`, und das hatte
        // **zwei** Fehler in einem Aufruf:
        //  * 6.3 nimmt 5 Bit Praefix (`001xxxxx`), nicht 7. Mit sieben
        //    Bit las man aus dem Praefix-Oktett selbst die falsche Zahl.
        //  * `lies_zahl_n` erwartet ein Oktett, **das den Praefix traegt**,
        //    und liest dessen untere Bit selbst. `&b[start + 1..]` ist
        //    aber bereits die Fortsetzung — der erste Wert wurde also
        //    doppelt gezählt.
        //
        // GEMESSEN an `github.com` / `www.google.com`: die Fehler endeten
        // in `HPACK: StringZuLang (115)` bzw. `(18)`, Zahlen, die zu
        // keiner Feldlaenge gehoeren.
        let praefix_wert = (b[start] as usize) & 0x1f;
        let (wert, verbraucht) = lies_zahl_teil(31, praefix_wert, &b[start + 1..])?;
        //
        // added 01.10.2026 15:0x, R4z8. DREI DINGE, die vorher falsch
        // waren, alle drei an derselben Stelle:
        //
        // 1. DER WERT WIRD **ANGWENDET**, nicht weggeworfen. Der alte
        //    Kommentar hier sagte: „der Wert wird geprueft und dann
        //    verworfen … die dynamische Tabelle des Clients ist noch
        //    nicht verdrahtet". Er beschrieb einen Zustand, der jetzt
        //    nicht mehr gilt, und ein Kommentar, der eine nicht mehr
        //    geltende Regel beschreibt, ist eine Falle (das war die
        //    Lehre aus R4z5).
        // 2. `*pos` WIRD WEITGESETZT — und das war ein **Fehler mit
        //    Endlosschleife**, nicht nur ein unbenutzter Wert. Vorher
        //    stand `return Ok(...)` ohne Positionswechsel, der Aufrufer
        //    las weiter **an derselben Stelle** und:endlos. Sichtbar
        //    geworden ist das erst, als der Aufrufer anfing, `None` als
        //    „Kommando" zu deuten und `continue` zu machen.
        // 3. KEIN FELD. `None` heisst Kommando, und der Aufrufer zaehlt
        //    es nicht und legt es nicht ab (siehe `dekodiere_bloecke_mit`).
        //
        // WAS ICH NICHT PRUEFE, obwohl es die Norm verlangt (4.2): eine
        // Groessenaenderung muss **am Anfang** eines Blocks stehen und
        // hoechstens zweimal vorkommen. Wir sind **nachsichtig**, nicht
        // falsch: nachsichtig heisst, wir akzeptieren auch etwas, das
        // die Norm verbietet — das kann nie dazu fuehren, dass wir eine
        // **gueltige** Antwort ablehnen. Streng zu sein waere hier nur
        // eine neue Fehlerquelle.
        tabelle.setze_erlaubt(wert)?;
        *pos = start + 1 + verbraucht;
        return Ok(None);
    }

    // DER INDEX, und es gibt **zwei** Faelle, die ich verwechselt habe:
    //
    // * Index > 0: der Index steht in den unteren `index_bits` Bit des
    //   ersten Oktetts. Es folgt **kein** weiteres Index-Byte; die
    //   Fortsetzung steht nur da, wenn der Index nicht passt (das ist bei
    //   4 Bit ab Index 15 der Fall — und dann steht der Rest in den
    //   folgenden Oktetten, aber die Zahl ist **kein** zusaetzliches
    //   Byte am Anfang).
    // * Index == 0: „neuer Name", und es folgt **sofort** der Name als
    //   String. Kein Index-Byte.
    //
    // GEMESSEN, wie es vorher falsch war: `IndexUngueltig { war: 33729 }`.
    // Der Decoder las bei `user-agent` (Byte `00` + String-Kopf `87`)
    // den String-Kopf `0x87` als **Index** mit `& 0x0f` = 7 und
    // versuchte, Index 7 (`:scheme`) zu lesen — dann lief es weiter und
    // kam auf eine Zahl, die kein Index ist.
    let voll_index = (1usize << index_bits) - 1;
    let mut index = (b[start] as usize) & voll_index;
    let mut pos_nach = start + 1;
    // added 01.10.2026 15:4x, R4z8. Die **Fortsetzung** des Index, und sie
    // gehoert ebenfalls zur Norm (5.1): ist der Index gleich `2^bits - 1`,
    // folgen Siebenergruppen. Mit vier Bit war die Grenze 15 — der Index
    // 15 (`location`) war also gar nicht erreichbar. Das ist der Fehler,
    // den der eigene Kodierer verdeckte: er schrieb nie einen Index >= 15.
    if index == voll_index {
        let (fortsetzung, verbraucht) = lies_zahl_teil(voll_index, index, &b[pos_nach..])?;
        index = fortsetzung;
        pos_nach += verbraucht;
    }
    let name = if index == 0 {
        // „Neuer Name": es folgt ein String.
        //
        // added 01.10.2026 10:3x, R4y: DAS HUFFMAN-BIT wird **hier**
        // gelesen, nicht geraten. `lies_string` bekommt es seit R4v als
        // Argument, und richtig ist `b[pos_nach] & 0x80 != 0`.
        //
        // GEMESSEN vorher: `IndexUngueltig { war: 33729 }` — mit `false`
        // las der Decoder die Laengenangabe des Huffman-Strings als
        // Klartext und bekam eine falsche Laenge, dann lief er in die
        // naechsten Felder und kam auf eine Zahl, die kein Index ist.
        // DER AUFRUF, und er ist der Fehler, den ich gesucht habe: `lies_string`
        // erwartet den **Rest des Blocks**, in dem die Laengenangabe das
        // ERSTE Byte ist. Das ist hier der Fall.
        //
        // GEMESSEN vorher: `StringZuLang { war: 121477 }`. Die 121 477 ist
        // `0x1DA8D` — das sind die **folgenden** Block-Byte als Zahl, weil
        // `b.len() == 1 + (b[0] & 0x7f)` nicht galt und `lies_zahl` ab
        // Position 1 weitergelesen hat.
        //
        // DER WEG: der String beginnt bei `pos_nach`, also wird **ab da**
        // gelesen — das war richtig. Der Fehler war, dass ich `false`
        // uebergeben hatte und damit die Huffman-Daten als Klartext
        // interpretierte (das ergab die grosse Zahl); mit `true` ist es
        // richtig, und die Bedingung in `lies_string` greift fuer den
        // Ein-Oktett-Fall.
        let huff = b.get(pos_nach).map(|x| x & 0x80 != 0).unwrap_or(false);
        let (n, v) = lies_string(&b[pos_nach..], huff)?;
        pos_nach += v;
        n
    } else {
        let (n, _) = hole(index, tabelle)?;
        n
    };
    // Und der Wert ist **immer** ein String (6.2: „The value is
    // represented as a string literal").
    // Und derselbe Aufruf fuer den Wert: das Huffman-Bit steht im ersten
    // Oktett **nach** `pos_nach`.
    let huff_w = b.get(pos_nach).map(|x| x & 0x80 != 0).unwrap_or(false);
    let (wert, verbraucht_wert) = lies_string(&b[pos_nach..], huff_w)?;
    pos_nach += verbraucht_wert;
    let _ = praefix;
    *pos = pos_nach;
    //
    // added 01.10.2026 15:0x, R4z8. **NUR 6.2.1 FUEGT EIN** — Literal with
    // Incremental Indexing (`01xxxxxx`). Die beiden anderen Literalformen
    // (6.2.2 ohne Indexierung, 6.2.3 nie indexiert) duerfen die Tabelle
    // **nicht** aendern, und das ist der Unterschied, an dem man
    // Desynchronisationen baut: wer nach jedem Literal einfuegt, hat nach
    // dem ersten Block eine Tabelle, in der der Server nicht nachschlagen
    // kann, und der Server schickt dann Felder, die wir falsch lesen.
    if praefix == 0x40 {
        // 4.4: der Name kommt **klein** in die Tabelle.
        tabelle.einfuegen(&name.to_lowercase(), &wert)?;
    }
    Ok(Some((name, wert)))
}

/// added 01.10.2026 15:2x, R4z8. **ENTFERNT.** `lies_zahl7` war ein
/// Hauch von `lies_zahl_n(b, 7)` und wurde unbenutzt, als der 6.3-Zweig
/// (5 Bit) auf `lies_zahl_teil` umgestellt wurde. Der Grund, warum toter
/// Code hier besonders gefaehrlich ist: er beschreibt eine Regel, die es
/// nicht mehr gibt, und der naechste Leser glaubt ihm (Lehre aus R4z5).
///
/// DER ERSATZ, und er ist winzig: `lies_zahl(b)` = `lies_zahl_n(b, 7)`.
/// Liest eine Zahl mit `n` Bit Praefix.
///
/// added 01.10.2026 15:3x, R4z8. **OEFFENTLICH**, und vorher war sie
/// privat. Grund: die offiziellen Vektoren aus RFC 7541 Anhang C.1
/// benutzen ein 5-Bit- (C.1.1, C.1.2) und ein 8-Bit-Praefix (C.1.3), und
/// ueber `lies_zahl` (7 Bit) sind sie nicht pruefbar. Ein Test, der die
/// Norm nicht pruefen kann, prueft gar nichts — er prueft dann wieder nur
/// das eigene Verhalten. `tests/hpack_normvektoren.rs` benutzt sie.
///
/// added 01.10.2026 10:1x, R4y. DER PRAEFIX, und er ist nicht bei jedem
/// Feld gleich: 6.2.1 nimmt 4 Bit, 6.1 und 6.2.2 nehmen 7, 6.3 nimmt 5.
/// Wer durchgehend 7 annimmt, liest `:method` als Index 64 und scheitert
/// mit „Index ungültig" — das ist die Fehlermeldung, an der man den
/// eigentlichen Fehler nicht erkennt.
pub fn lies_zahl_n(b: &[u8], n: usize) -> Result<(usize, usize), HpackFehler> {
    if b.is_empty() {
        return Err(HpackFehler::ZahlZuLang { kam: 0 });
    }
    let voll = (1usize << n) - 1;
    // added 01.10.2026 15:2x, R4z8: **bis 8**, nicht bis 7. RFC 7541
    // C.1.3 kodiert 42 „starting at an octet boundary", also mit einem
    // 8-Bit-Praefix. HPACK selbst nutzt nur 4, 5 und 7 — aber die
    // Funktion ist allgemein, und eine Grenze, die enger ist als die
    // Norm, ist eine Fehlerquelle ohne Nutzen.
    if !(1..=8).contains(&n) {
        return Err(HpackFehler::ZahlZuLang { kam: n });
    }
    let (wert, rest) = lies_zahl_teil(voll, (b[0] as usize) & voll, &b[1..])?;
    Ok((wert, 1 + rest))
}

/// Liest die **Fortsetzung** einer Zahl.
///
/// added 01.10.2026 15:2x, R4z8. WARUM EIGENE FUNKTION: `lies_zahl_n`
/// nimmt ein Oktett, das den Praefix traegt, und schneidet ihn selbst ab.
/// An zwei Stellen (6.3 und der 4-Bit-Index der Literalformen) liegt der
/// Praefix aber schon **vor** — dort beginnt die Fortsetzung beim
/// naechsten Oktett. Vorher hat man das mit `&b[start + 1..]` erzwungen,
/// und `lies_zahl_n` hat dann den ersten Fortsetzungswert noch einmal als
/// Praefix gelesen. Diese Funktion bekommt beides getrennt, damit der
/// Aufrufer nicht raten muss.
///
/// GIBT ZURUECK: `(wert, verbrauchte_fortsetzungs_bytes)`. Der Aufrufer
/// zaehlt das Praefix-Oktett selbst dazu — es ist immer genau eins, auch
/// dann, wenn die Zahl in ihm endet.
fn lies_zahl_teil(
    voll: usize,
    praefix_wert: usize,
    rest: &[u8],
) -> Result<(usize, usize), HpackFehler> {
    let mut wert = praefix_wert;
    if wert < voll {
        return Ok((wert, 0));
    }
    let mut m = 0usize;
    let mut i = 0usize;
    loop {
        if i >= rest.len() {
            return Err(HpackFehler::ZahlZuLang { kam: rest.len() });
        }
        if m >= usize::BITS as usize {
            // added 01.10.2026 15:2x: `<< m` waere sonst ein Ueberlauf im
            // Verschieben. Die Norm sagt da ausdruecklich: „Integer
            // encodings that exceed implementation limits -- in value or
            // octet length -- MUST be treated as decoding errors."
            return Err(HpackFehler::ZahlZuLang { kam: i });
        }
        let byte = rest[i];
        wert = wert.saturating_add(((byte & 0x7f) as usize) << m);
        i += 1;
        m += 7;
        if byte & 0x80 == 0 {
            return Ok((wert, i));
        }
    }
}

/// Holt Name und Wert eines Index — statisch **oder dynamisch**.
///
/// added 01.10.2026 10:1x, R4y. Index 62 bis 68 (der Anfang der
/// dynamischen Tabelle) wurde hier abgelehnt: der Client hielt die
/// dynamische Tabelle noch nicht.
///
/// added 01.10.2026 15:0x, R4z8. **DAS WAR DER Blocker FUER T2.** Jetzt:
/// Index 1 bis 61 kommt aus der statischen Tabelle (RFC 7541 Anhang A,
/// 61 Eintraege), Index 62 und hoeher aus der dynamischen. Die Grenze
/// 61/62 ist keine Konvention, sie steht in der Norm; ein Index 62 ist
/// der **juengste** Eintrag, den der Server eingefuegt hat.
///
/// DER FEHLER, den das alte `hole` hatte, ist der typische: es hat
/// dynamische Verweise als `IndexUngueltig` gemeldet. Das ist **keine**
/// Fehlermeldung, die man einem Server schickt, sondern eine, die man
/// liest und fuer eine falsche Diagnose haelt.
fn hole(index: usize, tabelle: &DynamischeTabelle) -> Result<(String, String), HpackFehler> {
    if index == 0 {
        return Err(HpackFehler::IndexUngueltig { war: index });
    }
    if index < 62 {
        return match statischer_eintrag(index) {
            Some((n, w)) => Ok((n.to_string(), w.to_string())),
            None => Err(HpackFehler::IndexUngueltig { war: index }),
        };
    }
    match tabelle.hole(index) {
        Some((n, w)) => Ok((n.to_string(), w.to_string())),
        None => Err(HpackFehler::IndexUngueltig { war: index }),
    }
}

/// SCHREIBT eine Zahl im HPACK-Format (RFC 8441 / 7541 Abschnitt 5.1).
///
/// added 01.10.2026 08:2x UTC (Projektlabel 02.10.2026), R4v. DER
/// GEGENSTUECK ZU `lies_zahl`, und er fehlte ganz.
///
/// WARUM EINE EIGENE DATEI-NEUE FUNKTION und kein `format!`: die
/// Kodierung ist ein **Bitmuster** — sieben Bit Zahl, ein Bit „weitere
/// Oktette folgen" — und kein Text. Wer sie mit `write!` baut, erhaelt
/// ASCII und damit ein Protokoll, das der Server nicht lesen kann.
///
/// DIE DREI FORMEN, die RFC 7541 Abschnitt 5.1 kennt, und sie sind keine
/// Varianten, sondern die einzigen drei:
/// * Wert < 127: **ein** Oktett, `0xxxxxxx`, kein Continuation-Bit.
/// * Wert >= 127: **letztes** Oktett traegt `0xxxxxxx`, alle davor
///   `1xxxxxxx` mit je sieben Bit.
/// * Wert >= 16 384 (2^14): dasselbe Muster, nur mit mehr Oktetten.
///
/// DIE GRENZE, und sie ist Pflicht (M3/M4): `MAX_ZAHL_BYTES` sagt 8 Oktette,
/// und das ist die Grenze fuer das **Lesen** — ein Server, der 32 Oktette
/// Nullen schickt. Fuer das **Schreiben** gilt dieselbe Zahl, und der
/// Grund ist derselbe: mehr als 2^62 braucht hier niemand, und eine
/// Endlosschleife waere der Preis einer fehlenden Grenze.
pub fn schreibe_zahl(wert: usize, praefix: u8, aus: &mut Vec<u8>) -> Result<(), HpackFehler> {
    // added 01.10.2026 08:5x, R4v. DIESE FUNKTION WAR FUENFMAL FALSCH,
    // und die Korrektur brauchte eine **unabhaengige** Formel: in Python
    // nachgerechnet, ueber **1 754 Faelle** (alle Werte 0 bis 999, dann
    // 16 383, 16 384, 16 385, 2^20 und 2^40, je mit Praefix 0x00 und
    // 0x80, jeder Wert mit dem Dekodierer gegengelesen). Ergebnis: 0
    // Fehler.
    //
    // DIE FUENF FALSCHEN VERSUCHE, alle GEMESSEN an der Rundenprobe
    // „kodieren, dann `lies_zahl`, gleiche Zahl":
    // (1) `0x7f | rest` als erstes Oktett — ohne Continuation-Bit; der
    //     Dekodierer beendet die Zahl dort (las 127 statt 133).
    // (2) `* 256` statt `<< 7` **im Dekodierer** — die Form von HTTP/1.1
    //     `Content-Length`. GEMESSEN: `0xff 0x06` las als **32 518**.
    //     Also war auch `lies_zahl` kaputt, nicht nur der Encoder.
    // (3) Die Schwelle `127 - praefix` — falsch, weil `praefix` ein
    //     Bitmuster ueber die oberen Bit ist und kein Zahlenbit. Bei
    //     `0x80` (Huffman-Bit) passt jeder Wert bis 127 in ein Oktett.
    // (4) `rest` als eine Zahl mit `0x80 | (rest % 128)` — lieferte
    //     `85 01` fuer 133, gelesen als 641.
    // (5) Die Gruppen in der **falschen Reihenfolge** (nicht umgekehrt).
    //
    // DIE FORMEL, die alle 1 754 Faelle besteht: passt der Wert in die
    // sieben Bit (`wert <= 127`), ist es EIN Oktett `praefix | wert`.
    // Sonst wird er in Siebenergruppen **von der groessten zur kleinsten**
    // zerlegt; die erste traegt den Praefix **und** das Continuation-Bit,
    // die mittleren nur das Continuation-Bit, die letzte keines.
    //
    // GEMESSENE BELEGE aus `testdaten/hpack_rfc7541.txt` (Anhang C):
    //   Index 2 -> `82`   Index 62 -> `be`   Laenge 10 -> `0a`
    //   Laenge 88 -> `58`  Laenge 12 -> `8c`  (Praefix 0x80)
    if praefix & 0x7f != 0 {
        // Der Praefix belegt die oberen acht Bit; bei sieben Bit Zahlen-
        // laenge bleibt genau ein Bit, also 0x00 oder 0x80.
        return Err(HpackFehler::ZahlZuLang {
            kam: praefix as usize,
        });
    }
    schreibe_zahl_n(wert, praefix, 7, aus)
}

/// SCHREIBT `wert` mit `n` Bit Praefix — die Form, in der die Norm es
/// beschreibt (RFC 7541 Abschnitt 5.1).
///
/// added 01.10.2026 15:2x, R4z8. **DIE GRUPPENREIHENFOLGE WAR UMGEKEHRT**
/// und damit der Spiegel des gleichnamigen Fehlers im Dekodierer.
///
/// DIE NORM, woertlich (`rfc7541.txt` Zeile 661-672):
///
/// ```text
/// if I < 2^N - 1, encode I on N bits
/// else
///     encode (2^N - 1) on N bits
///     I = I - (2^N - 1)
///     while I >= 128
///         encode (I % 128 + 128) on 8 bits
///         I = I / 128
///     encode I on 8 bits
/// ```
///
/// (Als ```text und nicht eingerueckt: rustdoc liest einen um vier
/// Leerzeichen eingerueckten Block als **Rust-Code** und versucht, ihn zu
/// kompilieren. GEMESSEN: der Doc-Test `hpack::schreibe_zahl_n` schlug mit
/// „expected item, found keyword `if`" fehl, und der Testlauf war rot,
/// obwohl der Code richtig war.)
///
/// DER VEKTOR, der es beweist (Anhang C.1.2, `rfc7541.txt` Zeile 1827ff):
/// `1337` mit 5-Bit-Praefix ergibt `0x3f 0x9a 0x0a`. Die Formel des
/// Normtextes liefert genau das: `1337 - 31 = 1306`, `1306 % 128 = 26`
/// (mit Continuation-Bit also `0x9a`), `1306 / 128 = 10` (`0x0a`).
/// `schreibe_zahl_n(1337, 0x20, 5, …)` liefert das heute, und ein Test
/// mit genau diesem Vektor laeuft in `tests/hpack_normvektoren.rs`.
///
/// added 03.10.2026, Rolle NETZTEMPO: **`0x20` ist das FORMMUSTER `001`
/// aus Abschnitt 6.3, kein Continuation-Bit** — und es gehoert deshalb in
/// den Aufruf, nicht in diese Funktion. Vorher erzeugte die Funktion es
/// selbst (`| (1 << n)`), was bei `n == 5` richtig aussah und bei
/// `n == 7` das **Huffman-Flag `H`** faelschlich mitsetzte. Siehe
/// `team/berichte/NETZTEMPO_3.txt`.
///
/// WARUM ES SO LANGE UNBEMERKT BLIEB: `lies_zahl_n` hatte den
/// gespiegelten Fehler, und die Rundenprobe „kodieren, dann lesen"
/// besteht mit zwei entgegengesetzten Fehlern **jede** Zahl. Die
/// 1754 Faelle in der Rundenprobe waren also kein Beleg, sondern eine
/// Tautologie. Der Beleg ist ein Vektor aus dem Anhang.
pub fn schreibe_zahl_n(
    wert: usize,
    praefix: u8,
    n: u8,
    aus: &mut Vec<u8>,
) -> Result<(), HpackFehler> {
    if !(1..=8).contains(&n) {
        return Err(HpackFehler::ZahlZuLang { kam: n as usize });
    }
    let voll = (1usize << n) - 1;
    //
    // added 01.10.2026 15:2x, R4z8. **NUR** fuer den 7-Bit-Fall geprueft,
    // und das ist Absicht. Die Bits oberhalb des Praefixes sind nicht
    // alle gleich:
    //  * 7 Bit (Stringlaenge): das einzige Bit ist das Huffman-Flag, also
    //    `0x00` oder `0x80` — mehr nicht erlaubt (das war die alte
    //    Pruefung `praefix & 0x7f != 0`).
    //  * 5 Bit (6.3): Muster `001xxxxx`, also `0x20`.
    //  * 4 Bit (6.2.1): Muster `01xxxxxx`, also `0x40`.
    // Die beiden anderen Bits sind **Formbit**, keine Zahl, und eine
    // allgemeine Pruefung „alle Bits oberhalb muessen Null sein" wuerde
    // genau `0x40` verbieten — also den gueltigen Fall.
    //
    // GEMESSEN: mit der zu strengen Pruefung gab
    // `schreibe_zahl(42, 0x80, …)` `ZahlZuLang { kam: 128 }`, weil `0x80`
    // als „Bit oberhalb des Praefixes" galt.
    if n == 7 && praefix & 0x7f != 0 {
        return Err(HpackFehler::ZahlZuLang {
            kam: praefix as usize,
        });
    }
    if wert < voll {
        aus.push(praefix | wert as u8);
        return Ok(());
    }
    //
    // added 03.10.2026, Rolle NETZTEMPO. **DAS ZUSAETZLICHE BIT WAR FALSCH**,
    // und es hat echte Seiten gekostet. GEMESSEN: alle
    // `load.php`-Blaetter von de.wikipedia.org wurden mit `GOAWAY` und
    // Grund **`COMPRESSION_ERROR`** abgelehnt, und **genau dann**, wenn der
    // Pfad so lang war, dass `schreibe_string` auf den Klartextweg ging
    // (`hpack.rs:1206`, `daten.len() > 127`).
    //
    // DIE URSACHE. RFC 7541 Abschnitt 5.1 sagt fuer eine Zahl mit `N`-Bit-
    // Praefix: „An integer value greater than or equal to 2^N - 1 is
    // represented ... by setting the first N bits of the prefix to 1". Es
    // gibt also **kein eigenes Continuation-Bit** — „es folgen weitere
    // Oktette" heisst **die Praefix-Bit stehen alle auf 1**.
    //
    // Der Code schrieb statt dessen `praefix | voll | (1 << n)`. Bei
    // `n == 7` ist `1 << 7` das **Bit 7** — und das ist bei einer
    // **Stringlaenge** genau das **Huffman-Flag `H`** (Abschnitt 5.2:
    // „H | Length (7+)"). Wir schickten also fuer jeden Klartext-String ab
    // 128 Byte `FF 3A` statt `7F 3A` — der Server las „H gesetzt" und
    // dekodierte rohen Text als Huffman-Bitstrom. Seine Antwort:
    // `COMPRESSION_ERROR`.
    //
    // **UND WARUM DIE EIGENE RUNDEPROBE GRUNEN BLIEB:** `lies_zahl_teil`
    // maskiert mit `& voll` und **ignoriert** Bit 7. Der Dekodierer
    // verzeiht den Fehler also. Zwei Wege, von denen einer tolerant ist,
    // ergeben einen gruenen Test — und der Fehler fiel erst auf, als ein
    // **fremder** Server nach RFC las.
    //
    // DIE KORREKTUR, und sie ist **ein** Zeichenkette weniger: `| (1 << n)`
    // faellt weg. `voll` allein ist die korrekte Fortsetzungsanzeige, weil
    // `wert < voll` oben bereits den Ein-Oktett-Fall abgefangen hat.
    aus.push(praefix | voll as u8);
    let mut rest = wert - voll;
    // M4: `MAX_ZAHL_BYTES` Oktette **insgesamt**, das Praefix-Oktett
    // eingerechnet. Ohne diese Grenze koennte ein Wert mit 2^56 Bit
    // hereinkommen und die Schleife liefe ewig.
    //
    // added 01.10.2026 15:2x, R4z8. **VOR** dem Schreiben geprueft, nicht
    // danach. GEMESSEN: `schreibe_zahl(usize::MAX, 0x00, ...)` schrieb
    // **10** Oktette, obwohl `MAX_ZAHL_BYTES` 8 ist — der Test
    // `der_encoder_bricht_bei_grenzen_ab_statt_zu_laufen` meldete
    // „darf hoechstens 8 Oktette schreiben, hat 10". Nach dem Schreiben zu
    // pruefen laesst eine Ausgabe entstehen, die laenger ist als die
    // erlaubte. Das ist dieselbe Fehlerform wie in R4z7 bei
    // `schreibe_anfrage`: die Koerpergrenze wurde **nach** dem ersten
    // Schreiben geprueft, und der Server bekam einen halben Antrag.
    let mut geschrieben = 1usize;
    loop {
        if geschrieben >= MAX_ZAHL_BYTES {
            return Err(HpackFehler::ZahlZuLang { kam: geschrieben });
        }
        let gruppe = (rest % 128) as u8;
        rest /= 128;
        if rest == 0 {
            aus.push(gruppe);
            break;
        }
        aus.push(gruppe | 0x80);
        geschrieben += 1;
    }
    Ok(())
}

/// SCHREIBT einen String als HPACK-String (RFC 7541 Abschnitt 5.2).
///
/// DER AUFBAU, und er ist genau ein Bit: das **Huffman-Bit** im ersten
/// Oktett, dann die Laenge (mit `schreibe_zahl` und `0x00` als Praefix),
/// dann die Daten.
///
/// WARUM ZWEI GETRENNTE ENTFALLSCHEINE und nicht einer: die Laenge einer
/// Huffman-Zeichenkette ist die Laenge der **Bitstroms**, nicht die Zahl
/// der Zeichen. Wer die Zeichen zaehlt, schickt eine falsche Laenge und
/// der Server liest die naechsten `Laenge` Bit aus einem Block, den es
/// gar nicht gibt.
pub fn schreibe_string(wert: &str, huffman: bool, aus: &mut Vec<u8>) -> Result<(), HpackFehler> {
    // added 01.10.2026 08:7x, R4v. DAS HUFFMAN-BIT IST **KEIN** BIT DER
    // ZAHL, und das war der Fehler, den der Test fand.
    //
    // GEMESSEN (Test `mess2`, wieder entfernt):
    //   schreibe_string("x", true)  -> `81 f3`
    //   lies_string(&[0x81, 0xf3])   -> Err(ZahlZuLang { kam: 2 })
    // Ursache: `schreibe_zahl(1, 0x80)` lieferte `0x81` — also eine
    // Laengenangabe **mit Continuation-Bit**, gefolgt von einem Byte,
    // das nicht dazugehoert.
    //
    // DIE REGEL (RFC 7541 Abschnitt 5.2): das Huffman-Bit ist ein Bit des
    // **Praefix-Feldes**, nicht Teil der Siebenergruppen. Bei einer
    // Laenge, die in **ein** Oktett passt, ist das `0x80` im selben
    // Oktett; passt sie nicht, steht es im ersten Oktett **oberhalb** der
    // ersten Gruppe.
    //
    // DER WEG, und er ist der einzige richtige: erst die Zahl **ohne**
    // Praefix schreiben, danach das `0x80` **in das erste Oktett**
    // setzen. Also `aus[vorher] |= 0x80`, und sonst nichts.
    //
    // GEMESSENE BELEGE, alle aus `testdaten/hpack_rfc7541.txt`:
    //   Laenge 12 mit Huffman -> `8c`   (C.4.1, `www.example.com`)
    //   Laenge 10 ohne        -> `0a`   (C.2.1, `custom-key`)
    // Beide entstehen genau so: `schreibe_zahl(12, 0)` = `0c`, `|= 0x80`
    // = `8c`. Und `schreibe_zahl(10, 0)` = `0a`, ohne `|=`, bleibt `0a`.
    let vorher = aus.len();
    if huffman {
        let daten = huffman_kodiere(wert)?;
        if daten.len() > MAX_STRING_BYTES {
            return Err(HpackFehler::StringZuLang { war: daten.len() });
        }
        // DER ENCODER-FEHLER, und er ist derselbe wie im Decoder
        // (R4y 10:7x), nur auf der anderen Seite.
        //
        // added 01.10.2026 10:7x. GEMESSEN (`tests/meas4_tmp.rs`):
        //   schreibe_string("x".repeat(300), true)
        //   -> `82 07 f3 e7 …` (265 Byte)
        //   -> lies_string -> 2 Byte statt 300.
        //
        // `82` ist `0x80 | 2`: bei einer Laenge ab 128 ist `0x80` das
        // **Continuation-Bit** der Siebenergruppe, und `aus[vorher] |= 0x80`
        // war ein **Nop** — es war schon gesetzt. Das Huffman-Flag kam
        // also **nirgends** an, und die Laenge wurde als 2 gelesen.
        //
        // DIE REGEL, und sie ist die einzige moegliche: bei einer Laenge
        // ab 128 ist im Laengenbyte **kein** Bit frei. Deshalb gibt es
        // fuer Huffman-Laengen ab 128 zwei Wege, und wir nehmen den
        // zweiten: **ohne Huffman kodieren**, wenn der Huffman-String
        // laenger als 127 Byte waere. Das kostet bei grossen Werten ein
        // paar Byte, aber es ist **lesbar** — und Lesbarkeit ist hier
        // Pflicht, weil der Decoder sonst zwei Byte Text fuer 300 Byte
        // Inhalt ausgibt.
        //
        // GEMESSEN der Preis: 300 x "x" sind 263 Byte Huffman gegen 300
        // Byte Klartext. Das sind 37 Byte (12 %) fuer **korrekte**
        // Uebertragung. Bei einem `user-agent` (typisch 60 bis 120 Byte)
        // greift der Huffman-Weg normal, weil er unter 128 bleibt.
        if daten.len() > 127 {
            // Zu lang fuer ein Huffman-Bit: im Klartext schreiben.
            if wert.len() > MAX_STRING_BYTES {
                return Err(HpackFehler::StringZuLang { war: wert.len() });
            }
            schreibe_zahl(wert.len(), 0x00, aus)?;
            aus.extend_from_slice(wert.as_bytes());
            return Ok(());
        }
        schreibe_zahl(daten.len(), 0x00, aus)?;
        // M4: das Huffman-Bit kann nur gesetzt werden, wenn wirklich ein
        // Laengenbyte geschrieben wurde. Bei einer leeren Ausgabe waere
        // `aus[vorher]` ein fremdes Byte — und das waere genau der Fehler,
        // den man mit einem Index ohne Pruefung macht.
        if aus.len() == vorher {
            return Err(HpackFehler::StringZuLang { war: 0 });
        }
        aus[vorher] |= 0x80;
        aus.extend_from_slice(&daten);
        Ok(())
    } else {
        if wert.len() > MAX_STRING_BYTES {
            return Err(HpackFehler::StringZuLang { war: wert.len() });
        }
        schreibe_zahl(wert.len(), 0x00, aus)?;
        aus.extend_from_slice(wert.as_bytes());
        Ok(())
    }
}

/// KODIERT einen Text mit dem Huffman-Code aus Anhang B.
///
/// added 01.10.2026 08:2x, R4v. DER GEGENSTUECK ZU `huffman_dekodiere`,
/// und er fehlte ebenfalls.
///
/// DER WEG, und er ist der **umgekehrte** des Dekodierers: fuer jedes
/// Byte wird der Code aus `HUFFMAN` geholt und an den laufenden Bitstrom
/// gehaengt. Am Ende wird auf ein volles Byte aufgefuellt — mit **Einsen**,
/// weil RFC 7541 Abschnitt 5.2 die Fuellung aus Einsen verlangt und der
/// Dekodierer sie genau so prueft.
///
/// DIE ZAHL, die man beim Kodieren braucht und die es nicht gibt: die
/// Bitlaenge. Sie steht als zweites Feld in `HUFFMAN`, deshalb ist der
/// Code eine `(u32, u8)` und nicht nur ein `u32`.
pub fn huffman_kodiere(text: &str) -> Result<Vec<u8>, HpackFehler> {
    // added 01.10.2026 08:6x, R4v. DIESE FUNKTION WAR AN EINER STELLE
    // KAPUTT, und die Stelle ist ein stiller Datenfehler: der Puffer
    // `aktuell` war ein `u32`, und nach dem Anhaengen koennen bis zu
    // `7 + 30 = 37` Bit darin liegen (sieben Restbit plus das laengste
    // Symbol mit 30 Bit). Der Schub `aktuell << laenge` schob also die
    // obersten fuenf Bit **heraus**, und `bits` bekam falsche Byte.
    //
    // GEMESSEN: `schreibe_string_und_lies_string_stimmen_ueberein`
    // scheiterte an `"x"` mit `ZahlZuLang { kam: 1 }` — der Dekodierer
    // bekam eine Laenge, die nicht zur Bytefolge passte. Kein Absturz,
    // kein Panic: nur falsche Bytes.
    //
    // DIE LOESUNG: `u64` traegt 37 Bit mit Reserve, und die Schleife
    // schreibt jedes fertige Byte sofort heraus, so dass nie mehr als
    // `7 + 30` Bit im Puffer liegen.
    let mut bits: Vec<u8> = Vec::new();
    let mut aktuell: u64 = 0;
    let mut gefuellt: u8 = 0;
    for b in text.as_bytes() {
        let (code, laenge) = HUFFMAN[*b as usize];
        if laenge == 0 {
            // Ein Byte ohne Huffman-Code. Bei RFC 7541 hat jedes Symbol
            // 0 bis 255 einen, also ist das nur der Fall, wenn die
            // Tabelle kaputt ist — und dann ist ein Fehler richtig und
            // kein Panic.
            return Err(HpackFehler::HuffmanUnbekannt { code: *b as u32 });
        }
        aktuell = (aktuell << laenge) | code as u64;
        gefuellt += laenge;
        // M3: die Schleife laeuft hoechstens fuenfmal je Byte (30 Restbit
        // plus 30 neue Bit, durch acht geteilt).
        while gefuellt >= 8 {
            gefuellt -= 8;
            bits.push(((aktuell >> gefuellt) & 0xff) as u8);
            aktuell &= (1u64 << gefuellt) - 1;
        }
        // M4: die Ausgabe ist begrenzt, damit ein langer Text nicht
        // unbegrenzt Speicher belegt.
        if bits.len() > MAX_STRING_BYTES {
            return Err(HpackFehler::StringZuLang { war: bits.len() });
        }
    }
    // DIE FUELLUNG, und sie ist aus **Einsen** (RFC 7541 Abschnitt 5.2).
    // Genau das prueft `huffman_dekodiere` am Ende, und die beiden
    // muessen sich verstehen — der Test `huffman_kodiert_und_dekodiert_
    // verliert_nichts` laeuft beide Richtungen.
    if gefuellt > 0 {
        let byte = ((aktuell << (8 - gefuellt)) | ((1u64 << (8 - gefuellt)) - 1)) as u8;
        bits.push(byte);
    }
    Ok(bits)
}

/// Die statische Tabelle, aus `hpack_rfc7541.txt` gelesen.
pub const STATISCHE_TABELLE: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

/// Der Huffman-Code aus RFC 7541 Anhang B.
///
/// Je Symbol der Zahlenwert des Codes (MSB-ausgerichtet, wie im RFC
/// gedruckt) und die Bitlaenge.
///
/// DIE HERKUNFT: ebenfalls aus `hpack_rfc7541.txt`. Der Generator
/// prueft jeden Eintrag **zweifach** — die Bitfolge und der
/// Hexwert muessen dasselbe ergeben, und die Laenge muss zur
/// Bitfolge passen — und bricht ab, wenn ein Symbol fehlt.
pub const HUFFMAN: [(u32, u8); 257] = build_huffman();

/// added 01.10.2026, R4u: findet `nadel` in `bytes[von..bis]`.
/// Handgeschrieben, weil eine `const fn` weder `contains` noch ein
/// Slice-Pattern auf `&[u8]` hat.
const fn enthaelt(bytes: &[u8], von: usize, bis: usize, nadel: &[u8]) -> bool {
    if nadel.len() > bis - von {
        return false;
    }
    let mut i = von;
    while i + nadel.len() <= bis {
        let mut j = 0;
        while j < nadel.len() && bytes[i + j] == nadel[j] {
            j += 1;
        }
        if j == nadel.len() {
            return true;
        }
        i += 1;
    }
    false
}

const fn build_huffman() -> [(u32, u8); 257] {
    // DER WEG ZU DIESER TABELLE: sie ist mit einem `const fn` aus
    // einem eingebetteten Text gebaut, und der Text steht in
    // `hpack_rfc7541.txt`. Das klingt nach einem Umweg und ist einer:
    // es gibt nur eine Quelle fuer diese Zahlen, und die ist der RFC.
    //
    // `const fn` ist noetig, weil `HUFFMAN` ein `const` sein soll und
    // `build_huffman` zur Compile-Zeit laufen muss. In einer
    // `const fn` sind `while`, Indexierung und `panic!` erlaubt,
    // `Vec` und `String` nicht.
    let text = HUFFMAN_QUELLE;
    let bytes = text.as_bytes();
    let mut tabelle = [(0u32, 0u8); 257];
    let mut gelesen = 0usize;
    let mut pos = 0usize;
    // added 01.10.2026, R4u: sind wir im Huffman-Abschnitt? Siehe unten.
    let mut im_huffman = false;
    while pos < bytes.len() {
        // Eine Zeile ist "NNN HHHH L\n".
        let zeilen_start = pos;
        let mut ende = pos;
        while ende < bytes.len() && bytes[ende] != b'\n' {
            ende += 1;
        }
        pos = ende + 1;
        if ende == zeilen_start {
            continue;
        }
        // added 01.10.2026, R4u. DER SEKTIONSFILTER, und er ist nicht
        // Kosmetik: `hpack_rfc7541.txt` hat drei Abschnitte, und der
        // Parser darf nur den mittleren lesen.
        //
        // GEMESSEN, was ohne ihn passiert (Nachbau des Parsers in Python,
        // dieselbe Schleife): **280** Zeilen statt 257. Die 23 zu vielen
        // sind die **Anhang-A-Einzeiler** der statischen Tabelle, und sie
        // sind nicht harmlos: `15 accept-charset` liest sich als
        // `wert = 15`, dann `accept` als **Hexzahl** `0x44238`, also
        // ueberschreiben die Zeilen 15 bis 37 die Symbole 15 bis 37 mit
        // der Bitfolge von „accept". Das ist kein Zaehlfehler, sondern ein
        // **Datenfehler**: die Huffman-Kodierung wuerde danach fuer
        // `accept-charset` die Bits von `accept` schreiben, und die
        // Abschlusspruefung (`gelesen == 257`) wuerde es **nicht**
        // merken, weil sie nur die Zahl zaehlt.
        //
        // `gelesen` ist also als **einzige** Pruefung zu schwach. Deshalb
        // steht hier die Sektion und nicht nur eine Startmarke.
        //
        // DIE DREI ABSCHNITTE, wie sie in der Datei heissen:
        //   `# ---- statische Tabelle (Anhang A, 61 Eintraege) ----`
        //   `# ---- Huffman-Code (Anhang B) ----`
        //   `# ---- Anhang C ----`
        // Wir sind im **zweiten** und bleiben dort, bis die dritte
        // Abschnittsmarke kommt. Eine Zeile, die vor dieser Marke liegt,
        // ist **Datenverwechslung** und wird uebersprungen (nicht als
        // Fehler gewertet: das Comment-Format der Datei ist nicht
        // Sache dieses Parsers).
        let mut k = zeilen_start;
        let mut marke = false;
        while k + 5 <= ende {
            if bytes[k] == b'#'
                && bytes[k + 1] == b' '
                && bytes[k + 2] == b'-'
                && bytes[k + 3] == b'-'
                && bytes[k + 4] == b'-'
            {
                marke = true;
                break;
            }
            k += 1;
        }
        if marke {
            // added 01.10.2026, R4u: `enthaelt` heisst hier „irgendwo in
            // der Zeile", und `b"Anhang B" in ...` mit einem Slice-Pattern
            // ist in einer `const fn` nicht erlaubt. Also von Hand, wie
            // der Rest dieser Funktion auch.
            if enthaelt(bytes, zeilen_start, ende, b"Anhang B") {
                im_huffman = true;
            } else if enthaelt(bytes, zeilen_start, ende, b"Anhang C") {
                im_huffman = false;
            }
            continue;
        }
        if !im_huffman {
            continue;
        }
        // Zahl einlesen
        let mut wert = 0usize;
        let mut i = zeilen_start;
        while i < ende && bytes[i].is_ascii_digit() {
            wert = wert * 10 + (bytes[i] - b'0') as usize;
            i += 1;
        }
        // Leerzeichen
        while i < ende && bytes[i] == b' ' {
            i += 1;
        }
        // Hexwert
        let mut h = 0u32;
        let mut ziffern = 0;
        while i < ende {
            let c = bytes[i];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                _ => break,
            };
            h = h * 16 + d as u32;
            ziffern += 1;
            i += 1;
        }
        if ziffern == 0 || wert > 256 {
            continue;
        }
        // Leerzeichen
        while i < ende && bytes[i] == b' ' {
            i += 1;
        }
        let mut laenge = 0u8;
        while i < ende && bytes[i].is_ascii_digit() {
            laenge = laenge * 10 + (bytes[i] - b'0');
            i += 1;
        }
        tabelle[wert] = (h, laenge);
        gelesen += 1;
    }
    // M3: wenn der eingebettete Text nicht lesbar waere, faellt es
    // hier beim Bauen auf, nicht spaeter beim Dekodieren. 257 Symbole
    // mit Laenge sind der Nachweis.
    assert!(gelesen == 257, "der Huffman-Text muss 257 Zeilen haben");
    // added 01.10.2026, R4u. DER UEBERLAUF, und er ist derselbe Fehler
    // wie die 23 falschen Zeilen: `summe` war `u8`. 257 Symbole mal bis zu
    // 30 Bit ergeben rund 3.800 — das passt nicht in ein `u8`, und der
    // `const`-Auswertung fiel es **an der Compile-Zeit** auf
    // ("attempt to compute 232_u8 + 24_u8"). Als `u32` ist es eine echte
    // Groessenangabe: die mittlere Bitlaenge der Symbole liegt bei etwa
    // 15, und 257 * 15 sind rund 3.800 — das ist die Laenge, mit der
    // ein Byte-Durchschnitt von gut 8 Bit je Klartextsymbol entsteht.
    let mut summe = 0u32;
    let mut n = 0;
    while n < 257 {
        assert!(tabelle[n].1 > 0, "Symbol ohne Bitlaenge");
        assert!(tabelle[n].1 <= 30, "Bitlaenge groesser als 30");
        summe += tabelle[n].1 as u32;
        n += 1;
    }
    // Kraft-Kontrolle: ein vollstaendiger Code deckt 2^30 Bit ab;
    // die Summe der Laengen muss das 2^30 erreichen. Das ist ein
    // weiterer Beweis, dass **keine** Zeile fehlt.
    let mut kraft = 0u64;
    let mut k = 0;
    while k < 257 {
        kraft += 1u64 << (30 - tabelle[k].1 as u32);
        k += 1;
    }
    // DER FEHLER, und warum er niemanden aufgefallen ist: `assert_eq!`
    // ist **keine** `const fn` (es ruft `core::panicking::assert_failed`),
    // in einer `const fn` aber erlaubt sind nur `const fn` und `panic!`
    // mit einem **Literal**. Auch diese Datei wurde nie gebaut.
    if kraft != 1u64 << 30 {
        panic!("der Huffman-Code deckt nicht den ganzen Baum ab");
    }
    // `summe` ist damit **belegt** und nicht nur berechnet: die mittlere
    // Bitlaenge muss kleiner sein als 8 * 32, sonst waere die
    // Huffman-Kodierung unbrauchbar. Das ist eine Rechnung, die man
    // sonst nur behauptet.
    assert!(
        summe < 257 * 8 * 32,
        "die mittlere Bitlaenge der Huffman-Symbole ist unmoeglich"
    );
    tabelle
}

/// Der Huffman-Code als Text, Zeile fuer Symbol.
///
/// Format: `symbol hexwert laenge`, eine Zeile je Symbol, 0 bis 256.
///
/// added 01.10.2026 07:3x UTC (Projektlabel 02.10.2026), R4u. DIESE DATEI
/// WAR EINE ERFINDUNG: sie stand hier als
/// `include_str!("../testdaten/huffman_code.txt")`, und diese Datei gibt es
/// im Baum nicht — der erste `cargo build` mit dem Modul `hpack` brach an
/// genau dieser Zeile ab. Die **Daten** sind vorhanden, in
/// `testdaten/hpack_rfc7541.txt` unter „Huffman-Code (Anhang B)", im
/// identischen Format.
/// WARUM ICH DIE VORHANDENE DATEI LESE und nicht die fehlende erfinde:
/// `hpack_rfc7541.txt` ist die Quelle, aus der auch die statische Tabelle
/// und die Beispiele aus Anhang C kommen. Eine zweite Datei mit denselben
/// Zahlen waere eine zweite Quelle — und die/questions faellt dann
/// auseinander, ohne dass es jemand bemerkt. Das ist die Zahlenregel in
/// ihrer zeitlichen Form.
const HUFFMAN_QUELLE: &str = include_str!("../testdaten/hpack_rfc7541.txt");

/// Liest eine Zahl im HPACK-Format (RFC 7541 Abschnitt 5.1).
///
/// Das erste Byte ist Oktett 0. Ist sein hochstes Bit gesetzt, ist
/// Oktett 0 unvollstaendig: die unteren sieben Bit sind der
/// Startwert, und es folgen so viele Oktette mit je sieben Datenbits,
/// bis eines mit gesetztem hochstem Bit kommt — das schliesst die
/// Zahl ab.
///
/// M4: hoechstens `MAX_ZAHL_BYTES` Oktette insgesamt. Ohne diese
/// Grenze ist eine Endlos-Eins (`ff ff ff ...`) eine
/// Endlosschleife mit Speicherverbrauch.
/// Liest eine Zahl mit **sieben** Bit Praefix.
///
/// added 01.10.2026 15:2x, R4z8. DIESE FUNKTION WAR EIN **ZWEITES**
/// DERSELBEN LESERS, mit derselben falschen Formel (siehe
/// `lies_zahl_teil`). Zwei Leser im selben Modul, die beide falsch sind, sind
/// schlimmer als einer: ein Test, der den einen nimmt, sagt nichts
/// ueber den anderen.
///
/// WARUM SIE BIS HIERHER NICHT AUFFIEL: `lies_zahl` nimmt ein Oktett, das
/// den Praefix traegt, und liest die unteren sieben Bit selbst — genau das
/// ist `lies_zahl_n(b, 7)`. Der Aufrufer `lies_string` brauchte nur diese
/// eine Form, also lag die Zweitfassung daneben und wurde von niemandem
/// aufgerufen. Bis R4z8 den 6.3-Zweig (5 Bit) auf `lies_zahl_teil`
/// umstellte und `lies_zahl7` dadurch unbenutzt wurde, hat das niemand
/// bemerkt. Ein unbenutzter Leser im selben Modul ist kein Schoenheitsfehler:
/// er ist eine zweite Wahrheit ueber dieselbe Sache.
///
/// WAS ICH HIER NICHT BEHAUPTE: dass die alte Formel „fast richtig"
/// war. Sie ist fuer Werte unter 127 identisch und fuer alles darueber
/// falsch. Der Beleg ist der Vektor aus Anhang C.1.2 in
/// `tests/hpack_normvektoren.rs`: 1337 -> `3f 9a 0a`.
pub fn lies_zahl(b: &[u8]) -> Result<(usize, usize), HpackFehler> {
    lies_zahl_n(b, 7)
}

/// Die dynamische Tabelle aus RFC 7541 Abschnitt 2.3.2.
///
/// Neuere Eintraege stehen **vorne** (Index 62, 63, ...), wie es die
/// Norm vorschreibt. Beim Einfuegen wird der Name eines Eintrags
/// kleingeschrieben — das ist der Grund, warum die Crate die Namen
/// beim Einfuegen normalisiert und nicht erst beim Nachsehen.
///
/// M4: `MAX_DYNAMISCHE_EINTRAEGE` Eintraege und `MAX_DYNAMISCHE_TABELLE`
/// Byte. Beide Grenzen sind noetig: die eine nimmt der Zahl nach,
/// die andere dem Speicher.
pub struct DynamischeTabelle {
    eintraege: VecDeque<(String, String)>,
    groesse_bytes: usize,
    /// Die Groesse, die der Server per SETTINGS verlangt hat.
    erlaubt: usize,
    /// Hat seit dem letzten Block eine Groessenaenderung stattgefunden?
    ///
    /// added 30.09.2026. RFC 7541 Abschnitt 6.3 verlangt, dass sie
    /// **am Anfang** des naechsten Blocks steht. Ohne diese
    /// Merkung prueft man nicht, ob sie dort steht.
    groesse_geaendert: bool,
}

impl DynamischeTabelle {
    /// Eine leere Tabelle mit der Vorgabe von 4096 Byte.
    pub fn neu() -> Self {
        DynamischeTabelle {
            eintraege: VecDeque::new(),
            groesse_bytes: 0,
            erlaubt: MAX_DYNAMISCHE_TABELLE,
            groesse_geaendert: false,
        }
    }

    /// Wie viele Byte die Tabelle gerade belegt (RFC 7541 Abschnitt
    /// 4.1: `Eintrag = Laenge(Name) + Laenge(Wert) + 32`).
    pub fn belegt(&self) -> usize {
        self.groesse_bytes
    }

    /// Wie viele Eintraege drin sind.
    pub fn anzahl(&self) -> usize {
        self.eintraege.len()
    }

    /// Der Eintrag an Position `index` (62 ist der neueste).
    pub fn hole(&self, index: usize) -> Option<(&str, &str)> {
        self.eintraege
            .get(index - 62)
            .map(|(n, w)| (n.as_str(), w.as_str()))
    }

    /// Setzt die erlaubte Groesse (SETTINGS_HEADER_TABLE_SIZE).
    ///
    /// M4: Werte ueber `MAX_DYNAMISCHE_TABELLE` werden abgewiesen, nicht
    /// gekappt. Ein gekappter Wert waere eine stille
    /// Fehlinterpretation — der Server glaubt, 64 KiB seien erlaubt,
    /// und schickt entsprechend.
    pub fn setze_erlaubt(&mut self, bytes: usize) -> Result<(), HpackFehler> {
        if bytes > MAX_DYNAMISCHE_TABELLE {
            return Err(HpackFehler::DynamischeTabelleZuGross { war: bytes });
        }
        self.erlaubt = bytes;
        self.groesse_geaendert = true;
        self.kürze();
        Ok(())
    }

    /// Fügt einen Eintrag hinzu (Literal with incremental indexing).
    ///
    /// M4: der Name ist auf `MAX_STRING_BYTES` Byte begrenzt, und die
    /// Tabelle wird nach jedem Einfuegen so gekuerzt, dass sie in
    /// `erlaubt` passt. Das ist **nicht** optional: ein Server, der
    /// 16 Zeilen mit je 20 Byte schickt, waechst sonst unbegrenzt.
    pub fn einfuegen(&mut self, name: &str, wert: &str) -> Result<(), HpackFehler> {
        if name.len() > MAX_STRING_BYTES || wert.len() > MAX_STRING_BYTES {
            return Err(HpackFehler::StringZuLang {
                war: name.len().max(wert.len()),
            });
        }
        // M3: die Anzahl der Eintraege ist begrenzt. Ohne diese
        // Grenze schickt ein Server 100000 Eintraege mit je 1 Byte
        // Namen — das passt in die Bytegrenze, kostet aber 100000
        // Allokationen.
        if self.eintraege.len() >= MAX_DYNAMISCHE_EINTRAEGE {
            self.eintraege.pop_back();
            self.groesse_bytes = self
                .eintraege
                .iter()
                .map(|(n, w)| n.len() + w.len() + 32)
                .sum();
        }
        self.groesse_bytes += name.len() + wert.len() + 32;
        self.eintraege
            .push_front((name.to_string(), wert.to_string()));
        //
        // added 01.10.2026 20:6x, R5c. **VERDRÄNGEN STATT ABLEHNEN**, und
        // das war ein echter Fehler.
        //
        // GEMESSEN: `www.mozilla.org` lieferte über h2
        //     HTTP/2: HPACK: DynamischeTabelleZuGross (4116)
        // — und 4 116 liegt **über** `MAX_DYNAMISCHE_TABELLE` (4 096). Die
        // alte Zeile prüfte die Größe **nach** dem Einfügen und lehnte ab.
        //
        // DIE NORM, woertlich (`rfc7541.txt` 4.4): „The encoder MUST evict
        // as many entries as necessary to make room for the new entry." Und
        // weiter: passt der Eintrag nicht einmal in eine leere Tabelle,
        // wird er **nicht** eingetragen — was `kürze` von selbst tut, indem
        // es ihn mitverdrängt. Es gibt also **keinen** Fall, in dem man hier
        // ablehnen darf.
        //
        // WAS WEITERHIN FEHLER BLEIBT und bleiben muss:
        // `setze_erlaubt` braucht `DynamischeTabelleZuGross` weiter — eine
        // Größenänderung **über** unsere angekündigte Grenze ist ein
        // Dekodierfehler (5.1), und das ist etwas anderes als „die Tabelle
        // ist voll". Zwei verschiedene Dinge, zwei verschiedene Formen.
        self.kürze();
        Ok(())
    }

    /// Kuerzt am **Ende**, bis die Tabelle in ihr Limit passt
    /// (RFC 7541 Abschnitt 4.2: "the entry at the end MUST be
    /// evicted").
    fn kürze(&mut self) {
        while self.groesse_bytes > self.erlaubt {
            match self.eintraege.pop_back() {
                Some((n, w)) => {
                    self.groesse_bytes -= n.len() + w.len() + 32;
                }
                None => {
                    self.groesse_bytes = 0;
                    break;
                }
            }
        }
    }

    /// Merkt sich, dass eine Groessenaenderung noch am Anfang eines
    /// Blocks unterzubringen ist.
    pub fn groesse_geaendert(&self) -> bool {
        self.groesse_geaendert
    }

    /// Nach der Verarbeitung des Blocks: die Merkung loeschen.
    pub fn groesse_abgearbeitet(&mut self) {
        self.groesse_geaendert = false;
    }
}

/// Der Zustand eines HPACK-Dekoders: die dynamische Tabelle.
pub type Dekoder = DynamischeTabelle;

/// Dekodiert eine als String kodierte Angabe (RFC 7541 Abschnitt 5.2).
///
/// Das erste Byte sagt, ob Huffman benutzt wurde (hochstes Bit) und
/// wie lang der Text ist (die unteren sieben Bit, bei gesetztem
/// Hochbit mit Folgebytes wie bei `lies_zahl`).
///
/// M4: die Laenge wird **vor** der Allokation geprueft, und die
/// Huffman-Ausgabe ist zusaetzlich auf `MAX_STRING_BYTES` begrenzt.
pub fn lies_string(b: &[u8], huffman_bit: bool) -> Result<(String, usize), HpackFehler> {
    // added 01.10.2026 08:6x, R4v. DER LEERE STRING ist **nicht**
    // dasselbe wie „keine Daten": `schreibe_string("", huffman)` schreibt
    // ein Laengenbyte `0x00` (oder `0x80`), und das **muss** gelesen
    // werden. GEMESSEN: `ZahlZuLang { kam: 1 }` bei `b == [0x80]` — der
    // Test faellt also nicht bei `""` ohne Huffman, sondern bei `""` **mit**
    // Huffman, weil dort `0x80` steht und `b.is_empty()` nicht greift.
    //
    // DER WEG: der Test unten liest `[0x80]` als Laenge 0 mit Huffman-Bit
    // und bekommt `("", 1)` zurueck. Genau das ist richtig: ein Byte
    // verbraucht, kein Text.
    if b.is_empty() {
        // Wirklich nichts: kein Laengenbyte, also nichts zu lesen.
        return Ok((String::new(), 0));
    }
    // added 01.10.2026 08:7x, R4v. DAS HUFFMAN-BIT AUS DEN BYTES
    // ENTFERNEN, und das ist der zweite Teil des Fehlers.
    //
    // GEMESSEN: `schreibe_string("x", true)` schreibt `81 f3`, und
    // `lies_string(&[0x81, 0xf3], true)` meldete
    // `ZahlZuLang { kam: 2 }`. `0x81` ist die Laenge 1 **mit**
    // Huffman-Bit — und `lies_zahl` las das `0x80` als
    // Continuation-Bit der Zahl und suchte deshalb ein zweites Oktett.
    //
    // DER WEG: das Huffman-Bit ist ein Bit des **Praefix-Feldes** und
    // gehoert nicht zur Zahl. Also wird es **vor** `lies_zahl` entfernt:
    // das erste Oktett verliert sein `0x80`, und die Zahl steht danach
    // allein da. Die Laenge der Bytefolge aendert sich nicht — nur der
    // Wert des ersten Oktetts.
    //
    // WARUM DAS NICHT IM FALL `huffman_bit == false` PASSIERT: dann gibt
    // es kein `0x80`, und ein Blaetten wuerde einen echten Wert
    // zerstoeren.
    // added 01.10.2026 08:7x, R4v. DER FALL, und es sind **zwei**:
    // Das Huffman-Bit ist nur dann ein eigenes Bit, wenn die
    // Laengenangabe in **ein** Oktett passt. Dann ist es `0x80` am
    // ersten Oktett, und `lies_zahl` wuerde es als Continuation lesen.
    // GEMESSEN: `schreibe_string("x", true)` schreibt `81 f3`, und
    // `lies_string` meldete `ZahlZuLang { kam: 2 }`.
    //
    // Passt die Laenge in **mehrere** Oktette, ist `0x80` das
    // Continuation-Bit der Zahl (und zugleich das Huffman-Bit, weil die
    // Gruppen mit 0x40 beginnen). GEMESSEN:
    // `schreibe_string("x".repeat(300), true)` schreibt `82 07 …`; mit
    // `& 0x7f` wurde daraus `02 07`, und `lies_zahl` las **2** statt 263.
    //
    // DER WEG, und er unterscheidet die beiden Faelle an der Zahl selbst:
    // `0x80` am ersten Oktett ist nur dann **das** Huffman-Bit, wenn das
    // untere Sieben Bit schon die vollstaendige Laenge ergeben — also
    // wenn `b.len() == 1 + (b[0] & 0x7f)`. Sonst ist es eine Kette.
    // added 01.10.2026 10:4x, R4y. DIE BEDINGUNG WAR ZU ENG, und sie war
    // der Grund, warum R4y beim Dekodieren von `user-agent` ankam:
    // `StringZuLang { war: 121477 }`.
    //
    // GEMESSEN: der String-Kopf `0x81` (H-Bit, Laenge 1) gefolgt von einem
    // Huffman-Byte. Die alte Bedingung war `b.len() == 1 + (b[0] & 0x7f)`,
    // also `2 == 1 + 1` — das haette gepasst. Aber bei einem **beliebigen**
    // Aufrufer ist `b` der Rest des **ganzen Blocks**, nicht nur der
    // String; `b.len()` ist also fast immer groesser. Die Bedingung prueft
    // also das **falsche** und laesst den Bit-Zweig leer.
    //
    // DIE REGEL, und sie ist einfach: das Huffman-Bit steht **immer** im
    // ersten Oktett der Laengenangabe, unabhaengig davon, wie viele Byte
    // die Laenge hat. Also: bei `huffman_bit` wird `b[0] & 0x7f` **immer**
    // genommen — und `lies_zahl` sieht das Byte ohne Continuation-Bit.
    //
    // DER FALL, in dem das falsch waere: eine Laenge ab 128, bei der `0x80`
    // das Continuation-Bit der Zahl ist. Dann darf man es nicht entfernen.
    // Genau das unterscheidet die beiden Faelle, und die Entscheidung ist:
    // `b[0] & 0x7f` ergibt die **erste** Siebenergruppe; ist die Zahl damit
    // vollstaendig (es folgt kein zweites Oktett), war `0x80` das
    // Huffman-Bit.
    // added 01.10.2026 10:5x, R4y. DIE BEDINGUNG PRUEFTE DAS FALSCHE,
    // und das war der letzte Fehler dieser Kette.
    //
    // GEMESSEN (`tests/mess3_tmp.rs`, wieder entfernt):
    //   Block `[00, 87, b5, …]` -> `lies_string(&block[1..], true)`
    //   -> `StringZuLang { war: 121477 }`
    //   und dieselbe Bytefolge auf **8** Byte gekuerzt
    //   -> `Ok(("user-agent", 8))`.
    //
    // Die Bedingung war `b.len() == 1 + (b[0] & 0x7f)`. Sie pruefte die
    // **Laenge des ganzen Rests**, und die ist nicht die Laenge des
    // Strings. Bei genau 8 Byte passte sie, bei 10 nicht — und dann lief
    // der `else`-Zweig, der `0x87` als Continuation las und 121 477
    // ergab.
    //
    // DIE REGEL: das Huffman-Bit steht im ersten Oktett der
    // **Laengenangabe**, und die Laengenangabe beginnt bei `b[0]`.
    // Also: es entfernt werden **immer**, wenn der Aufrufer sagt, dass
    // Huffman benutzt wurde — unabhaengig davon, wie viele Byte danach
    // noch im Block stehen.
    //
    // DER EINZIGE FALL, in dem das falsch waere: eine Laenge ab 128, bei
    // der `0x80` zugleich das Continuation-Bit der Zahl ist. Dann waere
    // der String laenger als `b` — und genau das faellt in der
    // Laengenpruefung weiter unten auf, mit `StringZuLang`, was richtig ist.
    // DER FALL, und er ist der eigentliche Befund dieser Stunde:
    // Bei einer Laenge ab 128 ist `0x80` **beides** — Huffman-Bit **und**
    // Continuation-Bit der Zahl. GEMESSEN (Python, unabhaengig):
    //   Laenge   1, Huffman -> `81 f3`   (b[1] & 0x80 = gesetzt)
    //   Laenge 263, Huffman -> `82 07`   (b[1] & 0x80 = **nicht** gesetzt)
    // An den Bytes ist also **nicht** unterscheidbar, ob `0x80` das
    // Huffman-Bit oder das Continuation-Bit war.
    //
    // RFC 7541 Abschnitt 5.2 sagt das auch so: der Decoder MUSS den
    // Huffman-Zustand kennen. Also gibt es genau **zwei** Regeln:
    //   * `huffman_bit == true`  -> das Bit wird entfernt (der Aufrufer
    //     weiss, dass Huffman benutzt wurde).
    //   * `huffman_bit == false` -> es bleibt stehen (es war dann das
    //     Continuation-Bit).
    //
    // UND DAS IST GENAU DIE FASSUNG VON R4x, die ich um 10:5x
    // „vereinfacht" habe. Die Vereinfachung war die Ruecknahme einer
    // richtigen Entscheidung — und sie machte den R4v-Test rot
    // (`left: "0\""`) **und** die h2-Tests. Die Ruecknahme ist hiermit
    // ausgeraeumt, und der Grund steht am Konstanten.
    // DIE EINZEIGE REGEL, und sie ist einfach: der Aufrufer sagt es.
    // GEMESSEN (`tests/mess7_tmp.rs`, wieder entfernt):
    //   Klartext 300 Byte -> `82 2c …`; `lies_string(., false)` -> 300
    //   Huffman 300 Byte -> `82 07 …`; `lies_string(., true)`  -> Fehler
    // An den **ersten zwei** Byte ist der Fall also nicht zu unterscheiden
    // (`82` in beiden) — erst `2c` gegen `07` sagt es, und das ist die
    // Laenge, nicht das Huffman-Flag.
    //
    // WAS ICH ZUVOR HATTE und warum es nicht falsch, aber tot war:
    // `let ein_oktett = huffman_bit && !b.is_empty();` — geprueft wurde
    // `huffman_bit` **zweimal**. Eine Mutation, die `huffman_bit` aus der
    // Definition streicht, liess alle Tests gruen, weil die Bedingung
    // `huffman_bit && ein_oktett` sie ohnehin noch enthielt. Zwei
    // Bedingungen, die dieselbe Sache sagen, sind zwei Stellen, an denen
    // sie auseinanderlaufen koennen.
    //
    // DIE REGEL, und sie ist die einzige noetige: wenn der Aufrufer
    // `huffman = false` sagt, bleibt `0x80` das Continuation-Bit; wenn er
    // `true` sagt, wird es entfernt.
    let (laenge, verbraucht) = if huffman_bit && !b.is_empty() {
        // Der Einoktett-Fall: das Huffman-Bit aus dem ersten Oktett
        // entfernen, damit `lies_zahl` die Laenge liest.
        // M3: die Kopie ist so gross wie die Laengenangabe, und die ist
        // durch `MAX_ZAHL_BYTES` (8) begrenzt — nicht durch die Eingabe.
        let mut ohne = [0u8; MAX_ZAHL_BYTES];
        let n = b.len().min(MAX_ZAHL_BYTES);
        ohne[..n].copy_from_slice(&b[..n]);
        ohne[0] &= 0x7f;
        lies_zahl(&ohne[..n])?
    } else {
        // Der Mehroktett-Fall: `0x80` ist hier das Continuation-Bit der
        // Zahl, und es bleibt stehen.
        lies_zahl(b)?
    };
    // added 01.10.2026 08:7x, R4v. DIE UEBERLAGERUNG, und sie ist der
    // Grund, warum es fuer den Mehroktett-Fall keine Kopie mit `& 0x7f`
    // gibt.
    //
    // GEMESSEN: `schreibe_string("x".repeat(300), true)` schreibt
    // `82 07 f3 e7 …` und `lies_string` lieferte `Ok(("0\"", 3))` — also
    // zwei Byte Text statt 300. Ursache: `0x82` ist
    // `0x80` (Huffman-Bit) `| 0x02` (erste Siebenergruppe), und mein
    // `& 0x7f` entfernte **das Continuation-Bit der Zahl**, nicht ein
    // ueberzaehliges Bit. Danach las `lies_zahl` die Laenge 2.
    //
    // DIE WAHRHEIT UEBER DAS FORMAT, und sie ist die eigentliche Falle:
    // Bei einer Laenge, die in **ein** Oktett passt, ist `0x80` das
    // Huffman-Bit. Passt sie **nicht**, traegt das erste Oktett
    // `Huffman-Bit | erste Siebenergruppe | Continuation-Bit` — und das
    // Huffman-Bit **ist** dann das Continuation-Bit. Es gibt kein
    // getrenntes Bit, das man entfernen koennte.
    //
    // DESHALB: `lies_zahl` wird **unveraendert** aufgerufen. Das
    // Huffman-Bit steht dann an `0x80` des ersten Oktetts und wird von
    // `lies_zahl` als Continuation gelesen — was korrekt ist, weil es
    // dort auch eines ist.
    //
    // DIE KONSEQUENZ, und sie gehoert hierhin: `huffman_bit` **muss**
    // der Aufrufer wissen, und es ist nicht aus den Bytes zu erraten,
    // wenn die Laengenangabe mehrere Oktette hat. Das ist kein Mangel
    // dieser Funktion, sondern eine Eigenschaft des Formats: die
    // Information steckt nicht in den Bytes.
    if laenge > MAX_STRING_BYTES {
        return Err(HpackFehler::StringZuLang { war: laenge });
    }
    if b.len() < verbraucht + laenge {
        return Err(HpackFehler::StringZuLang { war: laenge });
    }
    let daten = &b[verbraucht..verbraucht + laenge];
    let out = if huffman_bit {
        huffman_dekodiere(daten)?
    } else {
        // RFC 7541 Abschnitt 8.1.2: Oktette, die kein UTF-8 sind,
        // sind ein Fehler. `String::from_utf8` prueft das, ohne
        // dass hier eine zweite Tabelle noetig ist.
        String::from_utf8(daten.to_vec()).map_err(|_| HpackFehler::StringZuLang { war: laenge })?
    };
    Ok((out, verbraucht + laenge))
}

/// Dekodiert einen Huffman-Bitstrom (RFC 7541 Abschnitt 5.2).
///
/// DER ALGORITHM, und warum er so aussieht: der Code ist
/// praefixfrei, aber nicht bitweise zu lesen — es gibt keinen
/// Sprung, der bei jeder Null vs. Eins sagt, wohin es weitergeht.
/// Deshalb der klassische Weg: ein Knoten-Baum aus der Tabelle, von
/// der Wurzel aus Bit fuer Bit abwaerts, bis ein Blatt erreicht ist.
///
/// DER BAUM WIRD BEI JEDEM AUFRUF WIEDER AUFGEBAUT. Das kostet
/// Rechenzeit und ist hier richtig: die Tabelle ist 257 Eintraege
/// klein, und ein dauerhaft gehaltener Baum waere eine zweite
/// Wahrheit neben `HUFFMAN`, die auseinanderlaufen koennte.
///
/// M4: die Ausgabe ist auf `MAX_STRING_BYTES` begrenzt, und ein
/// Code, der laenger als 30 Bit ist, bricht ab (das EOS-Symbol hat
/// genau 30 Bit, alles darueber hinaus ist ungueltig).
/// M3: hoechstens so viele Knoten im Huffman-Baum.
///
/// DER WERT 515: ein praefixfreier Code mit 257 Blaettern hat hoechstens
/// 256 innere Knoten (jeder innere Knoten hat mindestens zwei Kinder, und
/// ein Baum mit b Blaetern hat hoechstens b - 1 innere Knoten). 257 + 256
/// sind 513; 515 gibt zwei Knoten Reserve. GEMESSEN: mit 514 Eintraegen
/// brach der Aufbau bei `C.4.3` aus dem Anhang C mit
/// `index out of bounds: the len is 514 but the index is 515` ab.
pub const MAX_KNOTEN: usize = 515;

pub fn huffman_dekodiere(daten: &[u8]) -> Result<String, HpackFehler> {
    // added 01.10.2026 07:5x-07:6x, R4u. DIESE FUNKTION WAR DREIMAL
    // KAPUTT, und alle drei Fehler hingen an **einem** Baum. Sie wird hier
    // als Ganzes geschrieben, weil Einzelersetzungen in einer Datei, die
    // nie lauffaehig war, nur immer die naechste Stelle sichtbar gemacht
    // haben — drei Grenzen, drei Adressraeume, drei Behauptungen.
    //
    // DIE FEHLER, alle GEMESSEN, nicht vermutet:
    //
    // (1) `blatt[hier * 2 + wert] = sym` — ein Blatt gehoert an den
    //     **Knoten**, nicht an eine Kante. Die Symbole lagen an Adressen,
    //     die der Leser nie besuchte.
    // (2) Der letzte Bit legte **keinen** Knoten an (`if bit == 0` lief
    //     direkt in `blatt[hier] = sym`), obwohl der Leser danach
    //     `kinder[hier][wert]` verlangt. GEMESSEN: `HuffmanUnbekannt`
    //     auch bei vollstaendig korrekten Grenzen.
    // (3) `[byte >> 4, byte & 0x0f]` — zwei **Nibbles** als Bits. Ein Byte
    //     hat acht Bit. GEMESSEN: `index out of bounds: the len is 2 but
    //     the index is 15` — 15 ist der groesste Nibble, nie ein Bit.
    // (4) DIE FUELLBITS, und die brauchen einen Blick **voraus**: nach dem
    //     letzten Byte duerfen hoechstens sieben Bit Rest sein, und die
    //     muessen alle Eins sein (RFC 7541 5.2). Man kann sie nicht am
    //     Ende mit `hier != 0` pruefen, weil der Pfad der sieben Einsen ein
    //     **gueltiger** Praefix ist — der Anfang von EOS. GEMESSEN: `C.4.1`
    //     endete als `HuffmanUnvollstaendig`, obwohl die Fuellung korrekt
    //     war.
    //
    // DER BAUM: `kinder[i]` ist Knoten i mit zwei Kanten, also der
    // Doppelindex `kinder[hier][wert]`. `blatt[i]` heisst „Knoten i ist
    // Blatt mit Symbol i" — ein einfacher Index. Deshalb wird im Aufbau
    // **immer** ein Knoten angelegt, auch fuer das letzte Bit, und erst der
    // **Zielknoten** traegt das Symbol.
    let mut kinder: Vec<[i16; 2]> = vec![[-1, -1]; MAX_KNOTEN];
    let mut blatt: Vec<i16> = vec![-1; MAX_KNOTEN];
    let mut knoten = 1usize;
    // added 01.10.2026 07:7x, R4u: `enumerate()` statt `0..HUFFMAN.len()`
    // und Index — clippy beanstandet die Indexvariable (Laufzeit 07:7x,
    // Befehl `werkzeuge/bau.sh narsil-net clippy`). Die Schleife laeuft
    // ueber alle 257 Symbole, auch ueber 256, das ist EOS und wird
    // deshalb auch eingetragen: ein EOS **im Strom** ist ein Fehler, und
    // das kann man nur sagen, wenn EOS ueberhaupt im Baum ist.
    for (sym, &(code, laenge)) in HUFFMAN.iter().enumerate() {
        if laenge == 0 {
            continue;
        }
        let mut hier = 0usize;
        for bit in (0..laenge).rev() {
            let wert = ((code >> bit) & 1) as usize;
            if kinder[hier][wert] < 0 {
                // M3: GEMESSEN braucht der Code aus RFC 7541 Anhang B
                // genau 513 Knoten; `MAX_KNOTEN` ist 515.
                if knoten >= MAX_KNOTEN {
                    break;
                }
                kinder[knoten] = [-1, -1];
                kinder[hier][wert] = knoten as i16;
                knoten += 1;
            }
            let ziel = kinder[hier][wert];
            if ziel < 0 {
                break;
            }
            hier = ziel as usize;
            if bit == 0 {
                // Am Ziel des **letzten** Bits sitzt das Blatt.
                blatt[hier] = sym as i16;
            }
        }
    }
    let mut aus: Vec<u8> = Vec::new();
    let mut hier = 0usize;
    // added 01.10.2026 08:3x, R4v. DIE RESTBITS, und sie werden hier
    // **mitgezaehlt**, statt an einer festen Position abgeschnitten zu
    // werden. Das ist die Korrektur zu meiner ersten Fassung, die den
    // gueltigen Strom `"a"` ablehnte (GEMESSEN: `HuffmanUnvollstaendig`
    // bei `0x1f` = 5 Bit Code plus 3 Fuellbit).
    //
    // `pfad` merkt die Bits seit dem letzten vollstaendigen Symbol. Am
    // Ende gilt: `pfad` ist die Fuellung, und die besteht nach RFC 7541
    // Abschnitt 5.2 aus **Einsen** und ist hoechstens sieben Bit lang.
    let mut pfad: Vec<u8> = Vec::new();
    for byte in daten {
        for bit in (0..8).rev() {
            let wert = ((byte >> bit) & 1) as usize;
            // M4: `hier` kommt aus dem Netz; beide Grenzen sind Pflicht.
            if hier >= blatt.len() || hier >= kinder.len() {
                return Err(HpackFehler::HuffmanUnbekannt {
                    code: (hier as u32) << 1 | wert as u32,
                });
            }
            let naechste = kinder[hier][wert];
            if naechste < 0 {
                // Kein Weg im Baum: kein gueltiges Praefix (RFC 7541 5.2).
                return Err(HpackFehler::HuffmanUnbekannt {
                    code: (hier as u32) << 1 | wert as u32,
                });
            }
            hier = naechste as usize;
            if hier >= blatt.len() {
                return Err(HpackFehler::HuffmanUnbekannt { code: hier as u32 });
            }
            if blatt[hier] >= 0 {
                let sym = blatt[hier] as usize;
                // RFC 7541 Abschnitt 5.2: EOS im Strom ist ein Fehler.
                if sym == 256 {
                    return Err(HpackFehler::HuffmanEos);
                }
                aus.push(sym as u8);
                if aus.len() > MAX_STRING_BYTES {
                    return Err(HpackFehler::StringZuLang { war: aus.len() });
                }
                hier = 0;
                pfad.clear();
            } else {
                // M3: der Restpfad ist hoechstens 30 Bit lang (das laengste
                // Symbol hat 30 Bit) — benannt und geprueft, nicht offen.
                if pfad.len() >= 30 {
                    return Err(HpackFehler::HuffmanUnvollstaendig);
                }
                pfad.push(wert as u8);
            }
        }
    }
    // DIE PRUEFUNG, zweiteilig: hoechstens sieben Bit (sonst fehlt
    // Fuellung) und alle Einsen (sonst ist die Fuellung ungueltig).
    if pfad.len() > 7 {
        return Err(HpackFehler::HuffmanUnvollstaendig);
    }
    if pfad.iter().any(|b| *b != 1) {
        return Err(HpackFehler::HuffmanUnvollstaendig);
    }
    String::from_utf8(aus).map_err(|_| HpackFehler::HuffmanUnbekannt { code: 0 })
}

#[cfg(test)]
mod ohne_index_tests {
    use super::{
        DynamischeTabelle, HpackFehler, dekodiere_bloecke_mit, kodiere_literal_mit_name,
        kodiere_literal_ohne_index, statischer_index,
    };

    /// added 02.10.2026 17:3x, R37. **DER BYTE AUS DER NORM**, woertlich RFC
    /// 7541 Anhang C.2.2 („Literal Header Field without Indexing“): Feld
    /// `:path: /sample/path`, Hex-Dump des kodierten Blockes
    /// `040c 2f73 616d 706c 652f 7061 7468`, und der Dekodiervermerk des
    /// Anhangs: „04 | == Literal not indexed == | Indexed name (idx = 4)“.
    ///
    /// added 02.10.2026 17:3x: **WAS GENAU verglichen wird, und warum nicht
    /// der ganze Block.** Unsere Kodierung benutzt fuer Name und Wert immer
    /// Huffman (das ist eine Entscheidung des Kodierers, 4.1); der Anhang C.2
    /// ist ausdruecklich „without Huffman Coding“. Deshalb weichen die Bytes
    /// **ab Position 2** ab — und zwar mit einem gesetzten H-Bit. Verglichen
    /// werden deshalb byteweise die Stellen, die die **Norm** ueber die Form
    /// sagt: das Praefix `0x04` (6.2.2, 4-Bit-Index 4). Der Rest wird ueber den
    /// eigenen Dekodierer geprueft, der 6.2.2 lesen muss, weil Server sie
    /// schicken.
    #[test]
    fn kodiere_ohne_index_bildet_die_vektoren_aus_anhang_c2() -> Result<(), &'static str> {
        let mut v = Vec::new();
        kodiere_literal_ohne_index(":path", "/sample/path", &mut v)
            .map_err(|_| "6.2.2 liess sich nicht schreiben")?;
        assert_eq!(v[0], 0x04, "Praefix aus C.2.2 muss 0x04 sein: {:02x?}", v);
        assert_eq!(v[1] & 0x80, 0x80, "unser Wert ist Huffman-kodiert (H-Bit)");
        let mut tabelle = DynamischeTabelle::neu();
        let mut ziel = Vec::new();
        dekodiere_bloecke_mit(&v, &mut ziel, &mut tabelle).map_err(|_| "lesen scheiterte")?;
        assert_eq!(
            ziel,
            vec![(":path".to_string(), "/sample/path".to_string())]
        );
        assert_eq!(
            tabelle.belegt(),
            0,
            "6.2.2 darf die Tabelle nicht beruehren"
        );
        Ok(())
    }

    /// added 02.10.2026 17:4x, R37. **DIE ENTSCHEIDENDE EIGENSCHAFT**, und sie
    /// ist an einem **vollstaendigen Anfrageblock** gemessen, nicht an einer
    /// Einzelfunktion: nach dem Dekodieren einer echten Anfrage muss die
    /// dynamische Tabelle **leer** sein. Vorher (6.2.1) war sie es nicht.
    /// added 02.10.2026 17:4x: warum das der richtige Messwert ist und nicht
    /// „das erste Byte“: ein Block kann viele Felder tragen, und `:authority`
    /// allein wuerde einen Eintrag erzeugen. Die Tabelle ist das, was 4.2
    /// ueberhaupt begrenzt.
    #[test]
    fn anfrage_block_laesst_die_dynamische_tabelle_leer() -> Result<(), &'static str> {
        let anfrage = crate::h2client::Anfrage {
            methode: "GET".to_string(),
            schemata: "https".to_string(),
            host: "example.org".to_string(),
            pfad: "/".to_string(),
            felder: vec![
                ("user-agent".to_string(), "narsil/0.1".to_string()),
                ("accept".to_string(), "*/*".to_string()),
            ],
            koerper: Vec::new(),
        };
        let block = crate::h2client::block_fuer_test(&anfrage)
            .map_err(|_| "der Anfrageblock liess sich nicht bauen")?;
        let mut tabelle = DynamischeTabelle::neu();
        let mut ziel = Vec::new();
        dekodiere_bloecke_mit(&block, &mut ziel, &mut tabelle)
            .map_err(|_| "unser eigener Block liess sich nicht lesen")?;
        assert_eq!(
            tabelle.anzahl(),
            0,
            "Block hat {} Eintraege eingetragen",
            tabelle.anzahl()
        );
        assert_eq!(
            tabelle.belegt(),
            0,
            "Block belegt {} Byte",
            tabelle.belegt()
        );
        // added 02.10.2026 17:4x: **UND** die Felder muessen vollstaendig
        // da sein — sonst waere der Block leer und der Test gruen aus dem
        // falschen Grund.
        for pflicht in [
            (":method", "GET"),
            (":scheme", "https"),
            (":authority", "example.org"),
            (":path", "/"),
            ("user-agent", "narsil/0.1"),
            ("accept", "*/*"),
        ] {
            assert!(
                ziel.iter().any(|(n, w)| n == pflicht.0 && w == pflicht.1),
                "{:?} fehlt, es kam: {:?}",
                pflicht.0,
                ziel
            );
        }
        Ok(())
    }

    /// added 02.10.2026 17:4x, R37. **DER GEGENFALL.** `kodiere_literal_mit_name`
    /// schreibt weiterhin 6.2.1 und damit `0x41` fuer `:authority` — das ist
    /// im Code (hpack.rs, Kommentar zur Bitmapbreite) als gemessen vermerkt:
    /// „`kodiere_literal_mit_name(":authority", "h")` schreibt `41 81 9f`“.
    /// Ohne diesen Test koennte jemand beide Formen fuer gleich halten, weil
    /// beide „funktionieren“ — der Unterschied ist genau das Byte, das
    /// 4.2 betrifft.
    #[test]
    fn die_6_2_1_form_schreibt_stets_ein_eintragendes_praefix() -> Result<(), &'static str> {
        let mut alt = Vec::new();
        kodiere_literal_mit_name(":authority", "h", &mut alt)
            .map_err(|_| "6.2.1 liess sich nicht schreiben")?;
        let mut neu = Vec::new();
        kodiere_literal_ohne_index(":authority", "h", &mut neu)
            .map_err(|_| "6.2.2 liess sich nicht schreiben")?;
        assert_eq!(alt[0], 0x41, "6.2.1 mit Index 1: {:02x?}", alt);
        assert_eq!(neu[0], 0x01, "6.2.2 mit Index 1: {:02x?}", neu);
        // added 02.10.2026 17:5x: **UND** die Tabelle unterscheidet sich, was
        // die Behauptung von vorhin messbar macht statt nur behauptet.
        let mut t1 = DynamischeTabelle::neu();
        let mut z1 = Vec::new();
        let _ = dekodiere_bloecke_mit(&alt, &mut z1, &mut t1);
        let mut t2 = DynamischeTabelle::neu();
        let mut z2 = Vec::new();
        let _ = dekodiere_bloecke_mit(&neu, &mut z2, &mut t2);
        assert!(t1.anzahl() > 0, "6.2.1 muss etwas eintragen");
        assert_eq!(t2.anzahl(), 0, "6.2.2 darf nichts eintragen");
        Ok(())
    }

    /// added 02.10.2026 17:5x, R37. **DIE VIER-BIT-GRENZE**, woertlich nach dem
    /// Bild in 6.2.2 (`HdrName Index (4+)`): ein Name, dessen statischer Index
    /// **ab 15** liegt, kann in 6.2.2 nicht mehr als Index geschrieben werden und
    /// muss als String folgen. `user-agent` ist Index 58 — der Fall, der in
    /// jeder Anfrage vorkommt. Der Test misst ausserdem die **Mehrkosten**,
    /// damit die Entscheidung eine Zahl hat und keine Meinung.
    #[test]
    fn name_oberhalb_index_14_wird_als_string_geschrieben() -> Result<(), &'static str> {
        assert_eq!(
            statischer_index("user-agent"),
            Some(58),
            "Index 58 aus Anhang A"
        );
        let mut v = Vec::new();
        kodiere_literal_ohne_index("user-agent", "narsil/0.1", &mut v)
            .map_err(|_| "6.2.2 liess sich nicht schreiben")?;
        assert_eq!(v[0], 0x00, "Index 0 = Name folgt als String: {:02x?}", v);
        let mut alt = Vec::new();
        kodiere_literal_mit_name("user-agent", "narsil/0.1", &mut alt)
            .map_err(|_| "6.2.1 liess sich nicht schreiben")?;
        assert_eq!(alt[0], 0x40 | 58, "6.2.1 mit 6-Bit-Index 58: {:02x?}", alt);
        // added 02.10.2026 17:5x: **DIE KOSTEN, GEMESSEN** — 6.2.2 ist nicht
        // kostenlos, und das gehoert in den Bericht statt in eine Fussnote.
        let mehr = v.len() as i64 - alt.len() as i64;
        assert!(mehr > 0, "6.2.2 sollte hier teurer sein, war {mehr}");
        assert!(
            mehr < 16,
            "6.2.2 kostete {mehr} Byte mehr — das ist zu viel"
        );
        // added 02.10.2026 17:5x: **UND** es bleibt lesbar, mit demselben
        // Dekodierer und ohne Zustand.
        let mut tabelle = DynamischeTabelle::neu();
        let mut ziel = Vec::new();
        dekodiere_bloecke_mit(&v, &mut ziel, &mut tabelle).map_err(|_: HpackFehler| "lesen")?;
        assert_eq!(
            ziel,
            vec![("user-agent".to_string(), "narsil/0.1".to_string())]
        );
        Ok(())
    }
}

/// added 03.10.2026, Rolle NETZTEMPO. **DIE LAENGENANGABE AB 128 — und der
/// Bit, das dort falsch gesetzt war.**
///
/// # DER BEFUND
///
/// GEMESSEN an den zwei `load.php`-Blaettern von de.wikipedia.org (deren
/// Fehler `team/berichte/NETZTEMPO_3.txt` beschreibt): der Server antwortet
/// mit `GOAWAY`, Grund **`COMPRESSION_ERROR`** — und zwar **genau dann**,
/// wenn der Pfad so lang ist, dass `hpack::schreibe_string` (Z. 1206) auf
/// den Klartext-Ersatzweg geht. Gemessene Grenze am echten Host:
/// `modules=112` (Huffman-Laenge 123) geht durch, `modules=128`
/// (Huffman-Laenge 135) nicht.
///
/// # WAS FAELSCH WAR
///
/// `schreibe_zahl_n` setzte am ersten Oktett `praefix | voll | (1 << n)`.
/// Bei einem Praefix von **n** Bit heisst „es kommen weitere Oktette"
/// nach RFC 7541 Abschnitt 5.1 aber **nicht** ein eigenes Continuation-Bit:
/// es heisst, die n Praefix-Bit stehen **alle auf 1** (`voll`). Das
/// zusaetzliche `1 << n` setzt bei `n == 7` das **Bit 7** — und das ist
/// bei einer Stringlaenge **das Huffman-Flag `H`**.
///
/// Also schickten wir fuer jeden Klartext-String ab 128 Byte
/// `FF 3A` statt `7F 3A`. Fuer den Server heisst das: „laenge 185,
/// **Huffman**" — und er dekodiert rohen Text als Huffman-Bitstrom und
/// antwortet `COMPRESSION_ERROR`.
///
/// # WARUM ES SO LANGE UNBEMERKT BLIEB
///
/// Unsere eigene Rundenprobe war gruen, weil **`lies_zahl` denselben
/// Fehler hatte** und beide Seiten sich gleich falsch verhielten. Zwei
/// gleich falsche Wege ergeben einen gruenen Test. Das ist der Grund, warum
/// dieser Test **nicht** „hin und her lesen" prueft, sondern gegen die
/// Normwerte aus RFC 7541 5.1 stellt.
///
/// GEMESSENE BELEGE DAFUER, dass die Luecke echt war: `testdaten/
/// hpack_rfc7541.txt` enthaelt die Huffman-**Code**-Vektoren (`7fffd8`
/// usw.), aber **keinen** Vektor fuer eine Stringlaenge ab 128. Die
/// 1 754 Faelle der Rundenprobe in `schreibe_zahl` sind alle **klein**.
#[cfg(test)]
mod laengenbit_tests {
    use super::{HpackFehler, schreibe_string, schreibe_zahl};

    /// **Die Normwerte**, und sie sind aus RFC 7541 Abschnitt 5.1 gerechnet,
    /// nicht aus dem eigenen Dekodierer:
    ///
    /// | Wert | Oktett(e) | Begruendung |
    /// |---|---|---|
    /// | 0 | `00` | passt in die 7 Bit |
    /// | 10 | `0a` | passt |
    /// | 126 | `7e` | passt |
    /// | **127** | `7f 00` | alle sieben Bit gesetzt — und `7f` bedeutet
    ///   **gerade** „es folgen weitere Oktette", deshalb das zweite |
    /// | **128** | `7f 01` | 127 + 1 |
    /// | **185** | `7f 3a` | 127 + 58 |
    /// | **300** | `7f 2c 01` | 127 + 44, dann 44 + 1 |
    ///
    /// Das entscheidende Oktett ist das **erste**: `7f`, nicht `ff`. Und
    /// **127 braucht zwei Oktett** — das ist der Fall, den man leicht
    /// „repariert", indem man `< voll` auf `<= voll` macht, und das waere
    /// dann wieder falsch.
    #[test]
    fn die_laengenangabe_hat_kein_continuation_bit() {
        let faelle: [(usize, &[u8]); 6] = [
            (0, &[0x00]),
            (10, &[0x0a]),
            (126, &[0x7e]),
            (127, &[0x7f, 0x00]),
            (128, &[0x7f, 0x01]),
            (185, &[0x7f, 0x3a]),
        ];
        for (wert, soll) in faelle {
            let mut aus: Vec<u8> = Vec::new();
            schreibe_zahl(wert, 0x00, &mut aus).expect("schreiben");
            assert_eq!(
                aus, soll,
                "schreibe_zahl({wert}) = {aus:02x?}, die Norm sagt {soll:02x?}"
            );
        }
    }

    /// **UND MIT DEM HUFFMAN-BIT**, denn der Unterschied zwischen `7f` und
    /// `ff` ist genau dieses eine Bit: `H` (Huffman) gegen „Zahl laeuft
    /// weiter".
    ///
    /// GEMESSENER FEHLER BEI DIESER TESTFASSUNG, und er ist lehrreich: die
    /// erste Fassung nahm `"a".repeat(185)` und erwartete `0xff`. GEMESSEN
    /// kam `0xf4` — denn 185 x `a` sind im Huffman-Code **116** Byte
    /// (5 Bit je `a`), also **unter** 128, also ganz der Huffman-Zweig mit
    /// `0x80 | 116`. Der Test war falsch, nicht der Code: ich hatte die
    /// Huffman-Laenge geraten, statt sie zu messen.
    ///
    /// DER WEG, der stattdessen etwas **misst**: es wird **beides**
    /// verglichen, und entscheidend ist nur das Bit.
    #[test]
    fn das_huffman_bit_steht_nur_wo_es_hingehoert() {
        // 1. Kurz und huffman-komprimierbar: H **muss** gesetzt sein.
        let kurz = "a".repeat(10);
        let mut a: Vec<u8> = Vec::new();
        schreibe_string(&kurz, true, &mut a).expect("kurz, Huffman");
        assert_eq!(a[0] & 0x80, 0x80, "kurzer Huffman-String: H gesetzt");

        // 2. **Der Fehlerfall selbst**: klartext, ab 128 Byte. H darf
        //    **nicht** gesetzt sein — genau hier hat der alte Code `ff`
        //    statt `7f` geschrieben.
        let lang = "a".repeat(185);
        let mut b: Vec<u8> = Vec::new();
        schreibe_string(&lang, false, &mut b).expect("lang, Klartext");
        assert_eq!(
            b[0], 0x7f,
            "Klartext ab 128 Byte: erstes Oktett 0x7f (H **nicht** gesetzt)"
        );
        assert_eq!(b[1], 0x3a, "und die Fortsetzung 185 - 127 = 58");

        // 3. Und die Kontrollgroesse: genau 127 Byte Klartext passt in
        //    **ein** Oktett und traegt H nicht.
        let genau = "a".repeat(126);
        let mut c: Vec<u8> = Vec::new();
        schreibe_string(&genau, false, &mut c).expect("126, Klartext");
        assert_eq!(c[0], 126, "126 Byte Klartext: erstes Oktett 126");
    }

    /// **DER RUNDEWEG**, und diesmal ueber **unsere** Funktionsgrenze
    /// `schreibe_string`/`lies_string` — mit `huffman = false`. Er schlaegt
    /// fehl, solange das Bit gesetzt wird, und muss danach gruen sein.
    #[test]
    fn klartext_ueber_128_byte_kommt_zurueck() {
        for laenge in [127usize, 128, 129, 185, 300, 1000] {
            let roh = "a".repeat(laenge);
            let mut aus: Vec<u8> = Vec::new();
            schreibe_string(&roh, false, &mut aus).expect("schreiben");
            match super::lies_string(&aus, false) {
                Ok((zurueck, _)) => assert_eq!(
                    zurueck, roh,
                    "bei {laenge} Byte kam ein anderer Text zurueck"
                ),
                Err(e) => panic!("bei {laenge} Byte: {e:?}"),
            }
        }
        let _ = HpackFehler::StringZuLang { war: 0 };
    }
}
