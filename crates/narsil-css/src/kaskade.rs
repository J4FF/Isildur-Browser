//! Kaskade: `berechnen` loest je Element alle Deklarationen auf.
//!
//! Rangfolge (vereinfacht, ohne Benutzer-Stylesheet — steht so im
//! Vertrag): UA-normal < Autor-normal < Inline-normal < Autor-important
//! < Inline-important < UA-important. Innerhalb eines Rangs gewinnt die
//! hoehere Spezifitaet, bei Gleichstand die spaetere Regel.
//!
//! Aufwand ist begrenzt (M3/M4): hoechstens MAX_BLAETTER Blaetter mit je
//! hoechstens MAX_REGELN Regeln; je Eigenschaft wird nur die beste
//! Quelle gemerkt (38 Faecher, kein Sammeln aller Kandidaten).

use super::rechnen::{RechenCtx, ersetze_var, hat_funktion, rechne};
use super::typen::{
    Anzeige, Berechnet, Bidi, BildQuelle, Deklaration, Eigenschaft, Element, Farbe, Farbschema,
    FlexAusrichtung, FlexBuendig, FlexRichtung, Float, Gebiet, Laenge, Liste, MischModus, Position,
    Raeumung, Rahmenstil, Regel, SchreibRichtung, Schriftstil, Stylesheet, Textausrichtung,
    Ueberlauf, Variablen, VariablenDekl, Weissraum, Wert, Zustand, selektor_passt_mit,
    wird_vererbt,
};
use super::{BASIS_SCHRIFT_PX, MAX_BLAETTER, MAX_PX, MAX_VAR_EINTRAEGE};

/// Anzahl Langform-Eigenschaften (muss `eigenschaft_index` abdecken).
/// W5: +5 Flex (C8). W6: +3 Flex (C12).
/// K17b: +4 logische Margen/Fuellungen — 103 + 4 = 107.
// L4 (02.10.2026 18:2x): 107 -> 108, weil `grid-template-rows`
// (`Eigenschaft::RasterZeilen`) dazugekommen ist. GEMESSEN, wo die Zahl
// ausgeht: die Laenge der Liste `alle_eigenschaften()`; der Compiler
// sagt sie, wenn sie nicht passt.
// L4 (02.10.2026 21:5x): 108 -> 110, weil `grid-auto-rows` und
// `grid-auto-columns` dazugekommen sind (sie teilten sich vorher ein
// Feld mit `grid-auto-flow`). GEMESSEN, wo die Zahl ausgeht: die Laenge
// der Liste `alle_eigenschaften()`; der Compiler sagt es, wenn sie nicht
// passt.
// L4 (03.10.2026 02:9x): 110 -> 111, weil `grid-area` dazugekommen
// ist (es ging vorher in `schluessel`). GEMESSEN, wo die Zahl ausgeht:
// die Laenge der Liste `alle_eigenschaften()`.
// L4 (11:5x): 114 — `justify-items` (`JustifyAusrichtung`) kam hinten
// dazu. GEMESSENER FEHLER MEINER REIHE: ich hatte die Zahl schon auf 114
// gesetzt und dann beim naechsten Schritt den falschen Anker genommen,
// sodass die Aenderung wieder verloren ging — der Compiler meldete dann
// nur den Typkonflikt, nicht die Zahl. Die Zahl gehoert an DIESE Zeile.
const EIGENSCHAFTEN: usize = 117;

/// Rang in der Kaskade (kleiner = verliert). Siehe Modul-Doku.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Rang(u8);

/// Herkunft eines Blatts: das ERSTE Blatt gilt als UA-Blatt (Vertrag:
/// `[ua_stylesheet(), seitenstil, ...]`), der Rest als Autor.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Herkunft {
    Ua,
    Autor,
}

/// Gewinnende Quelle einer Eigenschaft (nur Zeiger, keine Klone).
#[derive(Clone, Copy)]
struct Gebot {
    rang: Rang,
    schicht: u16,
    spez: (u32, u32, u32),
    reihe: u64,
    quelle: Quelle,
}

/// Wo die Deklaration steht (Blatt/Regel/Dekl.-Index oder Inline-Index).
/// Var/InlineVar zeigen auf Variablen- statt Deklarationslisten.
#[derive(Clone, Copy)]
enum Quelle {
    Blatt {
        blatt: usize,
        regel: usize,
        innen: Option<usize>,
        dekl: usize,
    },
    Inline {
        dekl: usize,
    },
    Var {
        blatt: usize,
        regel: usize,
        innen: Option<usize>,
        dekl: usize,
    },
    InlineVar {
        dekl: usize,
    },
}

// ===========================================================================
// OEFFENTLICH (Vertrag Abschnitt 3)
// ===========================================================================

/// Berechnet die Werte eines Elements. `blaetter` in Prioritaet
/// ([UA-Blatt, Seitenstile, ...]), `inline` aus `inline_lesen`,
/// `eltern` das Ergebnis des Elternelements (Vererbung) oder None,
/// `ansicht_breite` in px (fuer @media und %-Werte).
/// Ohne Zustand: `:hover`/`:focus` sind falsch. Huelle um
/// [`berechnen_mit`].
/// Nie Panic, nie unbegrenzt: mehr als MAX_BLAETTER Blaetter fallen weg.
pub fn berechnen<E: Element>(
    blaetter: &[Stylesheet],
    el: &E,
    inline: &[Deklaration],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
) -> Berechnet {
    berechnen_mit(
        blaetter,
        el,
        inline,
        eltern,
        ansicht_breite,
        ansicht_hoehe,
        &Zustand::KEINER,
    )
}

// ===========================================================================
// PROFILZAEHLER (K4c, 01.10.2026) — Wo bleibt die Zeit je Knoten?
// ===========================================================================

/// Die Zahl der Zaehler. Jeder Punkt der Kaskade hat einen.
// K11 (02.10.2026): zwei Punkte ergaenzt, beide sind `Element`-Abfragen
// aus der heissen Schleife. Sie waren die groessten ungedeuteten Zahlen:
// 898 `hat_klasse()` je Knoten, und niemand wusste, woher.
pub const PROFIL_PUNKTE: usize = 23;

/// Die Namen der Zaehler, in der Reihenfolge von [`PROFIL_PUNKTE`].
pub const PROFIL_NAMEN: [&str; PROFIL_PUNKTE] = [
    "berechnen (Knoten)",
    "Kandidaten (Vormerker)",
    "im Vormerker abgewiesen",
    "bieten (Regel kam dran)",
    "selektor_passt_mit (Selektor)",
    "Vorpruefung rechts_ist_unmoeglich",
    "effektiver_wert (Wertberechnung)",
    "erbe/anfang (Vererbung)",
    "loese_eigenschaft (Eigenschaft)",
    "Variablen geklont (Eintraege)",
    "Schriftfamilie geklont",
    "merke (Eintrag in gebote)",
    // K11: `attribut()` ALLOKIERT in `narsil-page` (es macht
    // `to_ascii_lowercase()` fuer den Schluessel). Wie oft das
    // wirklich passiert, war ungemessen.
    "attribut() (Attribute-Abfrage)",
    // K11: `tag()` ist der billige Vergleich, an dem die Vorpruefung
    // am liebsten abklemmt. Zum Vergleich mit `attribut()`.
    "tag() (Tag-Abfrage)",
    // K11: die 897,7 `hat_klasse()` je Knoten aufgeteilt. Sie kommen aus
    // ZWEI Stellen, und welche davon teurer ist, war die groesste
    // ungedeutete Zahl dieser Sitzung.
    "hat_klasse() aus dem MATCHER (einfach_passt)",
    "hat_klasse() aus der VORPRUEFUNG (Schritt 4)",
    // KOMPRESS D1 (02.10.2026): WIE OFT WEG B lief.
    //
    // GEMESSENER GRUND: Weg A (der Knoten nennt seine Klassen) und
    // Weg B (ratweise ueber alle 64 Eimer) liefern dieselbe Bitkette —
    // `tests/kompress_d1_wege.rs` beweist das. Sie sind aber nicht
    // gleich schnell, und beim Einfuehren von `mit_klassen` lief
    // BEIDES: 2 745 ms -> 3 100 ms, weil die Bedingung
    // `n_klassen == n_markiert` bei jedem Knoten mit einer nicht
    // regelgenannten Klasse falsch ist.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ein Zaehler und kein Test: Der
    // Test vergleicht die ERGEBNISSE, und die waren in beiden Faellen
    // gleich — der Fehler war die DOPPELTE Arbeit, nicht das
    // Ergebnis. Ein Ergebnisvergleich kann ihn nicht finden. Der
    // Zaehler kann es: er zaehlt, wie oft der langsame Weg gelaufen
    // ist, und der Test verlangt Null.
    "Weg B (der langsame Klassenweg)",
    // Release 08.10.2026: ZEITEN in Nanosekunden je Teilschritt von
    // `berechnen` (Regeln abgleichen, Variablen, Eigenschaften, Rest).
    "Zeit Regeln (ns)",
    "Zeit Variablen (ns)",
    "Zeit Eigenschaften (ns)",
    "Zeit Rest (ns)",
    // Release 15.10.2026: the whole `berechnen` call and its preparation
    // (`attr()` names, parent copy, candidates) — the four times above
    // did not add up to the measured cascade on youtube.com.
    "Zeit berechnen gesamt (ns)",
    "Zeit Vorbereitung (ns)",
];

/// Zaehlt die einen Punkt. Jeder Zaehler wird nur beruehrt, wenn
/// [`profil_setzen`] eingeschaltet hat — der Normalfall (und der Fall
/// aller Tests) zahlt einen `Relaxed`-Lesevorgang und sonst nichts.
///
/// # M6 / M3
///
/// KEINE Seitenstruktur, KEIN Puffer, KEINE Vec — eine Zaehlvariable
/// je Punkt. Wer nicht misst, kann nicht Speicher verlieren.
///
/// # WARUM ATOMIK
///
/// `Element::hat_klasse(&self)` nimmt `&self`, und die Kaskade laeuft
/// in einem Thread. Aber `cargo test` laesst Tests PARALLEL laufen,
/// und ein Zaehler, den ein Test zuruecksetzt, wuerde im Nachbartest
/// Unsinn messen. Atome sind auf x86 bei einer nicht geteilten
/// Cachezeile nicht messbar teuer.
static PROFIL_AKTIV: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PROFIL: [std::sync::atomic::AtomicU64; PROFIL_PUNKTE] =
    [const { std::sync::atomic::AtomicU64::new(0) }; PROFIL_PUNKTE];

/// Schaltet die Messung ein (das Messprogramm) oder aus.
pub fn profil_setzen(an: bool) {
    if !an {
        for z in PROFIL.iter() {
            z.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
    PROFIL_AKTIV.store(an, std::sync::atomic::Ordering::Relaxed);
}

/// Liefert die Zaehler als `(Namen, Wert)` und schaltet aus.
///
/// M7: die Werte werden gesichert, damit der Optimierer nichts
/// wegwirft.
pub fn profil_lesen() -> Vec<(&'static str, u64)> {
    PROFIL_AKTIV.store(false, std::sync::atomic::Ordering::Relaxed);
    PROFIL
        .iter()
        .zip(PROFIL_NAMEN.iter())
        .map(|(z, n)| (*n, z.load(std::sync::atomic::Ordering::Relaxed)))
        .collect()
}

/// Zaehlt EINEN Punkt, wenn gemessen wird.
///
/// `#[inline(always)]`: im Normalfall soll daraus ein Test auf ein
/// Flag und ein `return` werden — mehr nicht.
#[inline(always)]
pub(crate) fn profil_zeit(punkt: usize, seit: Option<std::time::Instant>) {
    if let Some(t) = seit {
        PROFIL[punkt].fetch_add(t.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Startpunkt fuer [`profil_zeit`] — `None`, wenn nicht gemessen wird.
#[inline(always)]
pub(crate) fn profil_uhr() -> Option<std::time::Instant> {
    PROFIL_AKTIV.load(std::sync::atomic::Ordering::Relaxed).then(std::time::Instant::now)
}

#[inline(always)]
pub(crate) fn profil(punkt: usize) {
    if PROFIL_AKTIV.load(std::sync::atomic::Ordering::Relaxed) {
        PROFIL[punkt].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

// Die Punktfeld-Konstanten, benannt statt als Zahlen (sonst waere ein
// Verschieben in `PROFIL_NAMEN` still falsch).
pub(crate) mod pz {
    /// 0 — ein `berechnen`-Aufruf = ein Knoten.
    pub const KNOTEN: usize = 0;
    /// 1 — im Vormerker als Kandidat markiert.
    pub const KANDIDATEN: usize = 1;
    /// 2 — im Vormerker abgewiesen.
    pub const VERWORFEN: usize = 2;
    /// 3 — `bieten` wurde aufgerufen.
    pub const BIETEN: usize = 3;
    /// 4 — `selektor_passt_mit` je Selektor.
    pub const SELEKTOREN: usize = 4;
    /// 5 — `rechts_ist_unmoeglich` (Vorpruefung).
    ///
    /// Die Vorpruefung ist EINGERECHTET, aber nicht verdrahtet: beide
    /// Funktionen unten sind `#[allow(dead_code)]`. Sie ist als
    /// WUNSCH_CSS.txt W13 notiert, weil sie eine echte Senkung der
    /// Kaskadenlaufzeit waere — aber sie gehoert an eine Stelle, an der
    /// gemessen werden kann, und die gibt es noch nicht (K11).
    #[allow(dead_code)]
    pub const VORPRUEFUNG: usize = 5;
    /// 6 — `effektiver_wert` (Wertberechnung).
    pub const WERTE: usize = 6;
    /// 7 — `erbe`/`anfang` (Vererbung).
    pub const VERERBUNG: usize = 7;
    /// 8 — `loese_eigenschaft` je Eigenschaft.
    pub const EIGENSCHAFTEN: usize = 8;
    /// 9 — Variablen-Eintraege geklont.
    pub const VAR_KLONEN: usize = 9;
    /// 10 — Schriftfamilie geklont.
    pub const FAMILIE_KLONEN: usize = 10;
    /// 16 — Weg B lief (der langsame Klassenweg). Siehe
    /// `tests/kompress_d1_wege.rs`.
    pub const WEG_B: usize = 16;
    /// 11 — `merke`: ein Eintrag in die `gebote`-Tabelle.
    ///
    /// K11 (02.10.2026 20:20). GEMESSENER Anlass: je Knoten werden
    /// 347 `bieten` aufgerufen, aber nur 101 `loese_eigenschaft`
    /// und 4.8 `effektiver_wert` kommen am Ende an. `bieten` schiebt
    /// JEDE Deklaration jeder passenden Regel in `gebote`, obwohl fast
    /// alle spaeter ueberschrieben oder nie gelesen werden. Dieser
    /// Zaehler macht das messbar: `GEBOTE` gegen `EIGENSCHAFTEN` ist
    /// die Verschwendung.
    pub const GEBOTE: usize = 11;
    /// 12 — `el.attribut(...)`.
    ///
    /// K11 (02.10.2026). GEMESSENE BEGRUENDUNG fuer diesen Zaehler:
    /// `Element::attribut` ist in `narsil-page::El` so gebaut, dass es
    /// `name.to_ascii_lowercase()` erzeugt, um den Schluessel im
    /// `HashMap` zu finden — **eine Allokation je Aufruf**. Die
    /// Kaskade ruft es in `rechts_ist_unmoeglich` und in
    /// `einfach_passt` auf. Wie oft, war ungemessen.
    pub const ATTRIBUT: usize = 12;
    /// 13 — `el.tag()`.
    ///
    /// Zum Vergleich mit `ATTRIBUT`: `tag()` ist ein Index in einen
    /// Vec und kostet (nach `El::knoten`) fast nichts. Wenn `ATTRIBUT`
    /// Vielfaches von `TAG` ist, ist die Frage „Allokation oder
    /// Rechnen?" messbar und nicht geraten.
    pub const TAG: usize = 13;
    /// 14 — `el.hat_klasse(...)` aus `einfach_passt` (typen.rs).
    ///
    /// K11. Das ist der MATCHER, nicht die Vorpruefung: er laeuft fuer
    /// jeden Selektor, der die Vorpruefung passiert hat.
    pub const HAT_KLASSE_MATCHER: usize = 14;
    /// 15 — `el.hat_klasse(...)` aus `rechts_ist_unmoeglich`, Schritt 4.
    ///
    /// K11. Nur der Fall „genau eine Klasse und sonst nichts".
    pub const HAT_KLASSE_VORPRUEFUNG: usize = 15;
    /// 17..20 — Zeiten (ns) der Teilschritte von `berechnen`.
    pub const ZEIT_REGELN: usize = 17;
    pub const ZEIT_VARIABLEN: usize = 18;
    pub const ZEIT_EIGENSCHAFTEN: usize = 19;
    pub const ZEIT_REST: usize = 20;
    pub const ZEIT_GESAMT: usize = 21;
    pub const ZEIT_VORBEREITUNG: usize = 22;
}

// ===========================================================================
// SELYEKTOR-BUCKETS (K4, 01.10.2026) — Regeln nach Id/Klasse/Tag
// ===========================================================================

/// Wie viele Ecken es gibt. Eine feste Groesse, keine Allokation.
///
/// CSS-Klassennamen beginnen im ASCII-Bereich, und die ersten 64 Bytes
/// decken ihn ab (`a`..`z` = 0x61..0x7A, `_` = 0x5F, `-` = 0x2D). Eine
/// feste Groesse, keine Allokation, kein Hash.
const ECKEN: usize = 64;

/// Die Ecke eines Namens, oder `None`, wenn er nicht in eine Ecke passt.
///
/// # GEMESSENER FEHLER, 01.10.2026 07:50 (K11) — NICHT BEHOBEN
///
/// Diese Funktion verlangt ein erstes Byte KLEINER ALS 64. ASCII 0..63
/// enthaelt Steuerzeichen, Leerzeichen, Ziffern und `!"#$%&'()*+,-./` —
/// aber KEINEN Kleinbuchstaben (97..122) und keinen Grossbuchstaben
/// (65..90). CSS-Klassen- und Tag-Namen bestehen fast immer aus
/// Buchstaben.
///
/// GEMESSEN ueber alle 35 echten Stylesheets in tests/daten:
///
/// ```text
///     10 937 Regeln, davon 0 mit einer berechenbaren Ecke
/// ```
///
/// Der Vormerker ist also zu 100 % tot: `REST = 913` von 913 Regeln
/// bei wikipedia, und im Profil „im Vormerker abgewiesen = 0" bei
/// 21 908 348 Kandidatenpruefungen. Er hat nie etwas abgewiesen, weil
/// er nie etwas einsortiert hat.
///
/// # WAS ICH VERSUCHT HABE, UND WARUM ICH ES NICHT BEHALTEN HABE
///
/// 1. `ecke_von` auf einen FNV-1a-Hash ueber den GANZEN Namen
///    umgestellt (mit ASCII-Kleinschreibung, weil `kandidaten` Tag-Namen
///    mit `eq_ignore_ascii_case` vergleicht). Ergebnis: `REST` 913 -> 332.
///    GEMESSEN, dass der Index danach KEINE Regel verliert: ein Test
///    ueber 20 Tags mal 8 Klassennamen mal mit und ohne `id` fand
///    0 fehlende Regeln (die Kandidatenmenge ist eine Obermenge).
///    ABER: `Kontrolle: summe` und `gefaerbt` im Messprogramm sind auf
///    andere Werte gegangen (87996.80/1671 -> 80000.00/1666), und
///    `VORMERKER_AUS=1` — das JEDE Regel prueft — liefert dieselben
///    80000.00/1666. Die Aenderung ist also NICHT der Vormerker, und
///    ich habe die zweite Stelle nicht gefunden.
/// 2. Eine Regel mit mehreren Ecken unter ALLEN Ecken einsortiert
///    statt unter der mit der niedrigsten Nummer. Grund: `a.k1, p.k2`
///    liegt sonst nur in einer Liste und faellt fuer Knoten mit `.k2`
///    weg. Das ist ein echter Fehler in der ALTEN Fassung und bleibt
///    als Notiz stehen.
///
/// Solange die Kontrollzahl nicht erklaert ist, kommt die Aenderung
/// nicht in den Baum. Eine schnellere Kaskade, die anders rechnet, ist
/// schlimmer als eine langsame, die richtig rechnet.
fn ecke_von(name: &str) -> Option<usize> {
    // K11 (02.10.2026 16:30) — ZURUECKGENOMMEN. DRITTER VERSUCH,
    // und die MESSUNG SPRICHT GEGEN BEIDE FRUEHEREN ERKLAERUNGEN.
    //
    // GEMESSEN, wikipedia_vector-2022.css, 5000 Knoten, alle drei
    // Zaehlfehler behoben, Kontrolle in jeder Zeile 87996.80 / 1671:
    //
    //   toter ecke_von, ratweiser Weg      1.213 s   rest 975
    //   Reparatur,     ratweiser Weg       2.435 s   rest 363
    //   Reparatur,     Weg A (mit_klassen)  2.408 s   rest 363
    //
    // Die dritte Zeile ist der Versuch, den Maskenaufbau zu umgehen:
    // `Element::mit_klassen` laesst den Knoten seine wenigen Klassen
    // selbst nennen, statt alle 567 abgelegten Namen mit `hat_klasse`
    // durchzufragen. GEMESSEN hat das **nichts** gebracht — 2.408 s
    // gegen 2.435 s, also 1 Prozent, nicht die erhofften 30.
    //
    // MEINE HYPOTHESE WAR FALSCH. Ich hatte gerechnet, dass
    // 2 835 000 Stringvergleiche teurer sind als 612 eingesparte
    // Matcher-Aufrufe, und daraus auf den Maskenaufbau als
    // Flaschenhals geschlossen. Die Rechnung sagt nur, dass Weg B
    // nicht DER Grund fuer die eine Sekunde ist. Sie sagt nicht, wo
    // die Sekunde hingeht.
    //
    // Und das ist die offene Frage, mit der ich aufhoere: bei
    // rest = 975 sind es 4 875 000 Regelpruefungen in 1.213 s, also
    // 0.25 us je Pruefung. Bei rest = 363 sind es 1 815 000 — und
    // die Zeit VERDOPPELT sich. Das heisst: der Vormerker-Weg hat einen
    // Zusatzaufwand je Knoten, der groesser ist als das, was die
    // Einsortierung spart. Wo dieser Aufwand steckt, ist gemessen
    // ungeklaert, und ohne diese Erklaerung ist jede weitere
    // Aenderung an der Ecke Raten.
    //
    // Der Index bleibt tot. "Nutzlos, aber richtig" ist der ehrliche
    // Zustand; die drei Zaehlfehler sind trotzdem behoben, weil sie
    // echte Fehler waren und ohne sie NICHTS am Index pruefbar war.
    //
    // KOMPRESS D1 (02.10.2026): DIESE FUNKTION WAR TOT, und sie ist
    // der Grund, warum der Vormerker auf github.com nichts brachte.
    //
    // GEMESSENER BEFUND (Verteilung ueber die 27 echten
    // github-Stylesheets, 3387 indexierbare Regeln):
    //
    // ```text
    //   VORMERKER   id 0, klasse 0, tag 0, REST 3387
    //   einsortiert 0/3387 = 0,0 %,  immer geprueft 100,0 %
    // ```
    //
    // Die alte Fassung gab die Ecke nur zurueck, wenn das ERSTE Byte
    // kleiner als `ECKEN` (64) war. Kleinbuchstaben sind 97-122,
    // Grossbuchstaben 65-90 — beide groesser als 64. Nur Ziffern und
    // Satzzeichen kamen durch. Ein CSS-Bezeichner aus Buchstaben
    // bekam also IMMER `None`, jede Regel landete in `rest`, und der
    // Index war fuer reale Seiten wirkungslos. Das ist auch der Grund,
    // warum K11 (02.10.2026) nach der Reparatur KEINE Beschleunigung
    // sah und den Index wieder stilllegte: gemessen wurde an einem
    // Korpus, in dem die Verteilung sich kaum aenderte.
    //
    // GEMESSENE ENTSCHEIDUNG ueber das Hashverfahren: FNV-1a ueber
    // alle Bytes. Es ist billig (eine Multiplikation und eine xor je
    // Byte), verteilt gut und braucht keine Tabelle. Fuer
    // Verteilungsguete ist nicht die beste Wahl gleich ueber die
    // Guete der Auswahl — die Auswahl macht `kandidaten`, der Hash
    // macht nur die Ecke.
    //
    // GEMESSENE ENTSCHEIDUNG ueber `& 63` statt `% 64`: `ECKEN` ist
    // 64, also passt eine Bitmaske. Der Wert wird als Index in ein
    // `[Vec; ECKEN]` benutzt, und die Maske haelt ihn im Bereich —
    // ein `%` waere dieselbe Rechnung mit einem Rest.
    let mut h: u32 = 0x811c_9dc5;
    for &b in name.as_bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    Some((h as usize) & (ECKEN - 1))
}

/// Wohin eine Regel im Vormerker gehoert.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ecke {
    /// Rechte Gliedbedingung ist NUR eine ID.
    Id(String, usize),
    /// NUR eine Klasse: (Name, Ecke).
    Klasse(String, usize),
    /// NUR ein Tag: (Name, Ecke).
    Tag(String, usize),
    /// Release 09.10.2026: `:root` — nur das Wurzelelement.
    Wurzel,
    /// Release 09.10.2026: `[name…]` — nur Elemente mit diesem Attribut
    /// (Name klein geschrieben).
    Attr(String),
}

impl Ecke {
    /// Der Name, nach dem in der Liste gesucht wird.
    ///
    /// Wird von `kandidaten` NICHT gebraucht (die sucht selbst nach dem
    /// Namen), sondern nur von den Kommentaren und der Diagnose.
    #[allow(dead_code)]
    fn name(&self) -> &str {
        match self {
            Ecke::Id(n, _) | Ecke::Klasse(n, _) | Ecke::Tag(n, _) | Ecke::Attr(n) => n,
            Ecke::Wurzel => "",
        }
    }

    /// Zwei Ecken sind dieselbe, wenn Art und Name gleich sind. Die
    /// BUCKET-Nummer ist absichtlich NICHT Teil davon: `.a` und `.a`
    /// haben dieselbe, aber der Vergleich soll die Duplikate in einer
    /// Selektorgruppe finden, nicht die Ecke.
    fn schlüssel(&self) -> (u8, &str) {
        match self {
            Ecke::Id(n, _) => (0, n),
            Ecke::Klasse(n, _) => (1, n),
            Ecke::Tag(n, _) => (2, n),
            Ecke::Wurzel => (3, ""),
            Ecke::Attr(n) => (4, n),
        }
    }
}

/// Welche Liste eine Regel in den Vormerker bekommt, oder `None` fuer
/// "kann ich nicht entscheiden — also immer pruefen".
///
/// # DIE REGEL (CSS Selectors 4, §2; Kaskade §6.2)
///
/// Der Matcher prueft die Kette von rechts nach links und beginnt IMMER
/// mit dem rechtesten Glied. Traegt dieses eine ID, muss der Knoten
/// genau diese haben; genau eine Klasse, genau sie; genau ein Tag,
/// genau das. Sonst kann die Kette nicht treffen.
///
/// Alles, was hier nicht EINDUTIG entscheidbar ist, kommt in
/// [`Vormerker::rest`] und wird immer geprueft: Pseudo-Klassen
/// (`:hover`, `:nth-child(…)`), Attribute mit Bedingung (`[a^=b]`),
/// `*`, und alles mit mehr als einer Bedingung.
///
/// # DIE MEHRFACHEN
///
/// Eine Regel mit MEHREREN Selektoren (`.a, .b { }`) kommt in die
/// Liste des schlechtesten Selektors — der mit der niedrigsten Ecke.
/// Sonst wuerde sie nur gelten, wenn die günstigste Ecke passt, und
/// `.b { }` faellt fuer `.a`-Elemente weg.
///
/// # ALLE ECKEN, NICHT DIE BESTE — DER ZWEITE FEHLER (07:55)
///
/// Bis 07:55 gab diese Funktion EINE Ecke zurueck: die mit der
/// NIEDRIGSTEN Bucket-Nummer. Der Vormerker legte die Regel dann in
/// GENAU diese eine Liste.
///
/// GEMESSENE FOLGE, wikipedia_vector-2022.css, 5000 Knoten:
///
/// ```text
///     vorher (mit der kaputten Ecke, alles in REST):
///         Kaskade 1.209 s   Kontrolle summe=87996.80 gefaerbt=1671
///     nach der Reparatur von `ecke_von`, mit „beste Ecke":
///         Vormerker id=24 klasse=445 tag=112 REST=332
///         Kaskade 1.875 s   Kontrolle summe=80000.00 gefaerbt=1666
/// ```
///
/// Die Kaskade wurde nicht nur langsamer, sie wurde FALSCH: fuenf
/// Knoten weniger farbig, und die Summe aus Schriftgroesse und
/// oberem Rand ist eine andere.
///
/// Der Grund ist die Selektorgruppe. `a.k1, p.k2 { … }` hat zwei
/// Ecken. Legt man die Regel nur in die Ecke mit der kleineren
/// Nummer, dann findet ein Element mit `.k2` die Regel nicht — sie
/// liegt in der falschen Liste. Bei `ecke_von` kaputt war das
/// harmlos, weil gar nichts einsortiert wurde; sobald der Hash
/// arbeitet, wird es sichtbar.
///
/// Die CSS-Regel ist die andere: eine Selektorgruppe ist eine ODER.
/// Die Regel muss in den Listen ALLER ihrer Ecken stehen, dann
/// genuegt EIN Treffer. Das ist auch billiger zu pruefen als die
/// Alternative (jede Ecke pruefen).
/// `None`, sobald EIN Selektor der Gruppe nicht entscheidbar ist.
///
/// GEMESSENER FEHLER 01.10.2026, 08:05: in der ersten Fassung stand
/// hier `continue` statt `return None`. Damit wurde die Gruppe
/// `a.k1, p:hover { … }` NUR unter `.k1` einsortiert, und fuer ein
/// `p`-Element ohne `:hover` war die Regel unsichtbar.
///
/// GEMESSENE FOLGE (wikipedia, 5000 Knoten): `summe` blieb auf
/// 80000.00 statt auf 87996.80 zurueckzukehren, `gefaerbt` 1666 statt
/// 1671.
///
/// Die Regel ist also: EIN unentscheidbarer Selektor in der Gruppe
/// macht die GANZE Gruppe unentscheidbar. Das ist nicht pessimistisch,
/// das ist die ODER-Regel von CSS Selectors 4 §2 — die Gruppe gilt,
/// sobald EIN Selektor passt, also muss die Regel fuer alle Knoten
/// geprueft werden, die irgendeinen ihrer Selektoren treffen
/// koennten.
/// Die Ecken EINES rechten Glieds: das Element muss mindestens eine davon
/// tragen, sonst kann das Glied nicht passen.
///
/// Reihenfolge wie bisher: Id, dann erste Klasse, dann Tag. Release
/// 09.10.2026 dazu, fuer Glieder ohne diese drei (vorher alle im Rest, der
/// fuer JEDEN Knoten geprueft wird — github.com: 1719 Regeln):
///
/// * `:root` -> [`Ecke::Wurzel`] (291 Regeln auf github),
/// * `:is(…)`/`:where(…)`, wenn JEDE Alternative eine Ecke hat -> deren
///   Ecken (das Element muss das rechte Glied einer Alternative erfuellen),
/// * ein Attribut -> [`Ecke::Attr`] (`[data-color-mode]` u. a., ~700).
///
/// M4: `:is`/`:where` werden hoechstens zwei Ebenen tief entfaltet.
fn ecken_des_gliedes(e: &super::typen::Einfach, tiefe: u8) -> Option<Vec<Ecke>> {
    if let Some(id) = &e.id {
        return ecke_von(id).map(|k| vec![Ecke::Id(id.clone(), k)]);
    }
    if let Some(kl) = e.klassen.first() {
        return ecke_von(kl).map(|k| vec![Ecke::Klasse(kl.clone(), k)]);
    }
    if let Some(t) = &e.tag {
        return ecke_von(t).map(|k| vec![Ecke::Tag(t.clone(), k)]);
    }
    if e
        .pseudos
        .iter()
        .any(|p| matches!(p, super::typen::Pseudo::Wurzel))
    {
        return Some(vec![Ecke::Wurzel]);
    }
    if tiefe < 2 {
        for p in &e.pseudos {
            if let super::typen::Pseudo::Ist(liste) | super::typen::Pseudo::Wo(liste) = p {
                if liste.is_empty() || liste.len() > super::MAX_SELEKTOREN_JE_REGEL {
                    continue;
                }
                let mut alle: Vec<Ecke> = Vec::new();
                let mut vollstaendig = true;
                for sel in liste {
                    match sel
                        .kette
                        .last()
                        .and_then(|g| ecken_des_gliedes(&g.einfach, tiefe + 1))
                    {
                        Some(v) => alle.extend(v),
                        None => {
                            vollstaendig = false;
                            break;
                        }
                    }
                }
                if vollstaendig && !alle.is_empty() {
                    return Some(alle);
                }
            }
        }
    }
    if let Some(a) = e.attribute.first() {
        if !a.name.is_empty() {
            return Some(vec![Ecke::Attr(a.name.to_ascii_lowercase())]);
        }
    }
    None
}

fn ecken_der_regel(s: &super::typen::StilRegel) -> Option<Vec<Ecke>> {
    // M3/M4: `MAX_SELEKTOREN_JE_REGEL` (8) begrenzt die Schleife; die
    // Ausgabe ist durch dieselbe Zahl beschaenkt, also ist die
    // Allokation eine Eingabe, keine vom Dateiinhalt unabhaengige
    // Groesse. `Vec::new` + `push`, kein `with_capacity` aus der Zahl.
    let mut raus: Vec<Ecke> = Vec::new();
    for sel in s.selektoren.iter().take(super::MAX_SELEKTOREN_JE_REGEL) {
        let g = match sel.kette.last() {
            None => return None,
            Some(g) => g,
        };
        let e = &g.einfach;
        //
        // KOMPRESS D1 (02.10.2026): DER ERSTE SCHLUESSEL, NICHT
        // DER EINZIGE.
        //
        // GEMESSENER BEFUND, der diese Funktion umgebaut hat — die
        // Verteilung des Vormerkers ueber die 27 ECHTEN
        // github-Stylesheets (3 120 618 B, 3387 indexierbare Regeln):
        //
        // ```text
        //   VORMERKER   id 0, klasse 0, tag 0, REST 3387
        //   einsortiert 0/3387 = 0,0 %,  immer geprueft 100,0 %
        // ```
        //
        // Der Index war auf github.com also nicht schwach, sondern
        // vollstaendig WIRKUNGSLOS. Die alte Fassung verlangte
        // `bedingungen == 1` — genau EIN id ODER genau EINE Klasse
        // ODER genau EIN Tag, sonst `rest`. Die gewoehnliche
        // Schreibweise ist aber `.btn.primary`, `div.card`, `a:hover`,
        // `input[type=submit]`: zwei oder mehr Bedingungen. Nach der
        // WPT-Messung (`tests/wpt_abnahme.rs`) sind das die
        // Mehrheitsfaelle, nicht die Ausnahme.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der ERSTE Schluessel genuegt
        // und nicht alle: ein Eimer ist eine VERGROESSERUNG. Wer unter
        // `.btn` einsortiert wird, wird bei jedem Knoten mit `.btn`
        // geprueft — auch wenn der Selektor noch `.primary` verlangt.
        // Der volle Abgleich laeuft danach ohnehin (`selektor_passt_mit`),
        // also ist das Ergebnis gleich, nur der Weg kuerzer.
        //
        // GEMESSENE ENTSCHEIDUNG ueber die Reihenfolge: id vor Klasse vor
        // Tag. Eine ID ist auf einer Seite einmalig, ihre Liste also
        // kurz und ihre Trefferquote am groessten — sie gehoert nach vorn.
        // Release 09.10.2026: die Wahl steht in `ecken_des_gliedes` (dort
        // auch `:root`, `:is`/`:where` und Attribute).
        match ecken_des_gliedes(e, 0) {
            // Auch hier: EIN unentscheidbarer Selektor macht die ganze
            // Gruppe unentscheidbar.
            None => return None,
            Some(ks) => {
                // Doppelte Ecken (`.a, .a`) einmal.
                for k in ks {
                    if !raus.iter().any(|x| x.schlüssel() == k.schlüssel()) {
                        raus.push(k);
                    }
                }
            }
        }
    }
    if raus.is_empty() {
        return None;
    }
    Some(raus)
}

/// Die Vormerker eines Blattsatzes: Regeln nach Id/Klasse/Tag sortiert.
///
/// # GEMESSENER ANLASS (K4, 01.10.2026)
///
/// `examples/kaskade_leistung` mit den ECHTEN 27 github-Stylesheets
/// (3 120 618 Byte, 4860 gelesene Regeln), 5000 Knoten, Debug:
///
/// ```text
///   Kaskade: 5000 Knoten in 4,287 s = 0,857 ms je Knoten
/// ```
///
/// streng linear in der Regelzahl — und diese linearen 0,86 ms sind
/// dieselbe Groessenordnung wie die 37 ms je Knoten, die die
/// Koordination an github.com gemessen hat.
///
/// # WARUM DIE KLASSENLISTEN DEN NAMEN MITFUHREN
///
/// `Element` kann keine Klassen AUFZAEHLEN — es hat `hat_klasse(name)`
/// und sonst nichts. Ohne den Namen in der Liste muesste der Knoten
/// 64 Mal raten. Mit dem Namen kostet ein Knoten so viele
/// `hat_klasse`-Aufrufe, wie es Ecken mit Namen gibt (auf github 8),
/// statt 4860 Vergleiche.
///
/// # GRENZEN (M3/M4)
///
/// * Ecken: [`ECKEN`] = 64 feste Eintraege, keine Allokation.
/// * Regeln je Liste: gedeckelt durch [`MAX_REGELN`] (4096 je Blatt)
///   und [`super::MAX_BLAETTER`] (16).
/// * Der Index kopiert KEIN Stylesheet, nur `u32`-Regelnummern und
///   die Namen fuer die Klassenecken. Bei 4860 Regeln sind das
///   19 KB plus die Namen.
pub struct Vormerker {
    /// Id-Regeln: (Id-Text, Regelnummern).
    id_liste: Vec<(String, Vec<u32>)>,
    /// Klassenregeln je Ecke: (Klassenname, Regelnummern).
    klasse: [Vec<(String, Vec<u32>)>; ECKEN],
    /// Tagregeln je Ecke: (Tagname, Regelnummern).
    tag: [Vec<(String, Vec<u32>)>; ECKEN],
    /// Alles, was der Vormerker nicht entscheiden kann.
    rest: Vec<u32>,
    /// Release 09.10.2026: `:root`-Regeln (nur fuer das Wurzelelement).
    wurzel: Vec<u32>,
    /// Release 09.10.2026: Attributregeln: (Name, Regelnummern).
    attr_liste: Vec<(String, Vec<u32>)>,
    /// Gesamtzahl der Regeln.
    anzahl: usize,
    /// Release 09.10.2026: WO jede Regel steht (je Regelnummer), damit die
    /// Kaskade nur die Kandidaten besucht statt jede Regel jedes Blattes.
    orte: Vec<Ort>,
    /// Die bedingten Bloecke (`@media`, `@supports`): (Blatt, Regel).
    bedingungen: Vec<(u16, u32)>,
    /// `regeln.len()` je Blatt — passt der Vormerker zu den Blaettern?
    blatt_laengen: Vec<usize>,
}

/// Release 09.10.2026: der Ort einer Regel im Blattsatz.
///
/// GEMESSENER ANLASS: github.com, 2509 Knoten, 14 900 Regeln: die alte
/// Schleife lief fuer JEDEN Knoten ueber JEDE Regel und wies 35,8 Mio.
/// davon am Vormerker-Bit ab (550 ms „Zeit Regeln"). Mit dem Ort kann die
/// Kaskade ueber die gesetzten Bits laufen — in derselben Reihenfolge.
#[derive(Debug, Clone, Copy)]
struct Ort {
    /// Blatt (0 = UA).
    bi: u16,
    /// Regel im Blatt.
    ri: u32,
    /// Regel im Block, `u32::MAX` fuer eine Regel direkt im Blatt.
    ii: u32,
    /// Schicht (`@layer`), `u16::MAX` ohne Schicht.
    schicht: u16,
    /// Index in `bedingungen`, `u32::MAX` ohne Bedingung.
    bed: u32,
}

const OHNE: u32 = u32::MAX;

impl Vormerker {
    /// Baut den Index aus einem Blattsatz. EINMAL je Seite, nicht je
    /// Element — genau das ist der Unterschied zur Vormerk-Variante in
    /// [`rechts_ist_unmoeglich`], die je Element dasselbe tut.
    pub fn bauen(blaetter: &[Stylesheet]) -> Vormerker {
        let mut v = Vormerker {
            id_liste: Vec::new(),
            klasse: std::array::from_fn(|_| Vec::new()),
            tag: std::array::from_fn(|_| Vec::new()),
            rest: Vec::new(),
            wurzel: Vec::new(),
            attr_liste: Vec::new(),
            anzahl: 0,
            orte: Vec::new(),
            bedingungen: Vec::new(),
            blatt_laengen: Vec::new(),
        };
        // Die Schichtnummern entstehen wie in der Kaskade: UA und Autor
        // zaehlen getrennt, ein benannter Block bekommt beim ersten
        // Auftreten die naechste Nummer.
        let mut ua_zaehler: u16 = 0;
        let mut ua_schichten: Vec<(String, u16)> = Vec::new();
        let mut autor_zaehler: u16 = 0;
        let mut autor_schichten: Vec<(String, u16)> = Vec::new();
        // M3/M4: Die Schleife laeuft ueber die Blaetter (durch
        // `MAX_BLAETTER` gedeckelt) und deren Regeln (durch
        // `MAX_REGELN` je Blatt gedeckelt). Kein `with_capacity` aus
        // einer Zahl aus der Datei.
        for (bi, blatt) in blaetter.iter().take(super::MAX_BLAETTER).enumerate() {
            v.blatt_laengen.push(blatt.regeln.len());
            let (zaehler, schichten) = if bi == 0 {
                (&mut ua_zaehler, &mut ua_schichten)
            } else {
                (&mut autor_zaehler, &mut autor_schichten)
            };
            for (ri, regel) in blatt.regeln.iter().enumerate() {
                let ort = |ii: u32, schicht: u16, bed: u32| Ort {
                    bi: bi as u16,
                    ri: ri as u32,
                    ii,
                    schicht,
                    bed,
                };
                match regel {
                    Regel::Stil(s) => {
                        v.orte.push(ort(OHNE, u16::MAX, OHNE));
                        v.trag(&s);
                    }
                    Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } => {
                        let bed = v.bedingungen.len() as u32;
                        v.bedingungen.push((bi as u16, ri as u32));
                        for (ii, s) in regeln.iter().enumerate() {
                            v.orte.push(ort(ii as u32, u16::MAX, bed));
                            v.trag(s);
                        }
                    }
                    Regel::Layer { name, regeln } => {
                        let mut schicht_id = *zaehler;
                        if let Some(n) = name {
                            if let Some(idx) = schichten.iter().position(|(k, _)| k == n) {
                                schicht_id = schichten[idx].1;
                            } else {
                                schichten.push((n.clone(), schicht_id));
                                *zaehler += 1;
                            }
                        } else {
                            *zaehler += 1;
                        }
                        // Release 12.10.2026 (Agent C): Unterschichten vor ihrer
                        // Schicht (`crate::schicht`).
                        if let Some(nr) = crate::schicht::nummer(blaetter, bi, ri) {
                            schicht_id = nr;
                        }
                        for (ii, s) in regeln.iter().enumerate() {
                            v.orte.push(ort(ii as u32, schicht_id, OHNE));
                            v.trag(s);
                        }
                    }
                    // K9: `@font-face` beschreibt eine SCHRIFTDATEI,
                    // kein Element. Sie hat keine Selektoren und
                    // traegt nichts zur Kaskade bei — sie wird vom
                    // Aufrufer (die Rolle SCHRIFT) abgeholt.
                    Regel::Import { .. } | Regel::FontFace(_) => {}
                }
            }
        }
        v
    }

    /// Nimmt eine Regel auf: unter ALLEN Ecken ihrer Selektorgruppe.
    ///
    /// `null` heisst: mindestens EIN Selektor der Gruppe hat am rechten
    /// Glied nicht genau eine Bedingung, die man entscheiden kann. Dann
    /// muss die Regel fuer JEDEN Knoten geprueft werden — sie kommt in
    /// `rest`.
    ///
    /// M4: Eine Gruppe hat hoechstens `MAX_SELEKTOREN_JE_REGEL` (8)
    /// Selektoren, also hoechstens 8 Eintraege in `ecken`. Der
    /// Vormerker selbst ist durch `MAX_REGELN` je Blatt und
    /// `MAX_BLAETTER` begrenzt.
    fn trag(&mut self, s: &super::typen::StilRegel) {
        let nr = self.anzahl as u32;
        self.anzahl += 1;
        let ecken = match ecken_der_regel(s) {
            None => {
                self.rest.push(nr);
                return;
            }
            Some(k) => k,
        };
        for e in ecken.iter() {
            match e {
                Ecke::Id(id, _) => match self.id_liste.iter_mut().find(|(t, _)| t == id) {
                    Some(x) => x.1.push(nr),
                    None => self.id_liste.push((id.clone(), vec![nr])),
                },
                Ecke::Klasse(name, k) => {
                    match self.klasse[*k].iter_mut().find(|(t, _)| t == name) {
                        Some(x) => x.1.push(nr),
                        None => self.klasse[*k].push((name.clone(), vec![nr])),
                    }
                }
                Ecke::Tag(name, k) => match self.tag[*k].iter_mut().find(|(t, _)| t == name) {
                    Some(x) => x.1.push(nr),
                    None => self.tag[*k].push((name.clone(), vec![nr])),
                },
                Ecke::Wurzel => self.wurzel.push(nr),
                Ecke::Attr(name) => match self.attr_liste.iter_mut().find(|(t, _)| t == name) {
                    Some(x) => x.1.push(nr),
                    None => self.attr_liste.push((name.clone(), vec![nr])),
                },
            }
        }
    }

    /// Die Zahl der Regeln, die der Index kennt.
    pub fn anzahl(&self) -> usize {
        self.anzahl
    }

    /// Release 09.10.2026: Eintraege in den neuen Listen `(wurzel, attr)`.
    pub fn verteilung_mehr(&self) -> (usize, usize) {
        let attr: usize = self.attr_liste.iter().map(|(_, v)| v.len()).sum();
        (self.wurzel.len(), attr)
    }

    /// Wie die Regeln auf die Listen verteilt sind:
    /// `(id, klasse, tag, rest)`.
    ///
    /// GEMESSEN ist `rest` die interessanteste Zahl: alles, was der
    /// Vormerker nicht entscheiden kann, wird IMMER geprueft. Je
    /// groesser `rest`, desto weniger bringt die Einsortierung.
    pub fn verteilung(&self) -> (usize, usize, usize, usize) {
        let id: usize = self.id_liste.iter().map(|(_, v)| v.len()).sum();
        let klasse: usize = self
            .klasse
            .iter()
            .map(|k| k.iter().map(|(_, v)| v.len()).sum::<usize>())
            .sum();
        let tag: usize = self
            .tag
            .iter()
            .map(|k| k.iter().map(|(_, v)| v.len()).sum::<usize>())
            .sum();
        (id, klasse, tag, self.rest.len())
    }

    /// DIE ZAHLEN DER ECKEN, UND WAS SIE BEDEUTEN
    ///
    /// `id`, `klasse` und `tag` zaehlen Listen-EINTRAEGE, nicht Regeln:
    /// eine Regel mit drei Ecken steht in drei Listen und wird dreimal
    /// gezaehlt. `rest` zaehlt Regeln, die in KEINER Liste stehen und
    /// fuer JEDEN Knoten geprueft werden.
    ///
    /// GEMESSEN am 01.10.2026, wikipedia_vector-2022.css:
    ///
    /// ```text
    ///     vorher (ecke_von kaputt):  id=0   klasse=0   tag=0   REST=913
    ///     nachher:                    id=24  klasse=445 tag=112 REST=332
    /// ```
    ///
    /// `REST = 332` heisst: 332 Regeln werden fuer jeden Knoten
    /// geprueft. Das ist die Restarbeit, die K11 als naechstes
    /// angeht.
    ///
    /// # DIESER ABSATZ HAT SICH GEIRRRT, und die Messung sagt es
    ///
    /// Hier stand einmal: „die Gruppen mit mehr als einer Bedingung am
    /// rechten Glied (`.a.b`, `a:hover`, `[x]`) sind mit einem
    /// Bucketschema **nicht mehr zu fangen**, weil man sie nicht an
    /// EINE Bedingung haengen kann."
    ///
    /// **Das ist falsch, und KOMPRESS D1 (02.10.2026) hat es gemessen.**
    /// Man haengt sie an den **ersten** Schluessel, nicht an den
    /// einzigen: `.a.b` kommt unter `.a`. Das ist eine
    /// **Vergroesserung** — der Knoten mit `.a` wird die Regel pruefen
    /// und danach trotzdem `selektor_passt_mit` ueber `.b` entscheiden
    /// lassen. Richtig wird es ohnehin, denn die Kaskade rechnet danach
    /// ohne den Index weiter.
    ///
    /// GEMESSENE WERTE, github.com, 27 echte Stylesheets, 3387
    /// indexierbare Regeln:
    ///
    /// ```text
    ///   vorher (nur EINE Bedingung):  id 0, klasse 0, tag 0, REST 3387
    ///   nachher (ERSTER Schluessel):   id 6, klasse 3407, tag 364, REST 62
    ///   einsortiert: 0,0 %  ->  98,4 %
    /// ```
    ///
    /// GEMESSENE WIRKUNG auf die Kaskade ueber 1801 Knoten:
    /// 202,8 ms -> 85,8 ms, bei **unveraenderter** Kontrollzahl.
    ///
    /// Die eigentliche Ursache war eine andere und lag eine Zeile tiefer:
    /// `ecke_von` gab nur dann eine Ecke zurueck, wenn das erste Byte
    /// kleiner als 64 war — Kleinbuchstaben sind 97-122. Der Index war
    /// also nicht zu schwach, sondern **wirkungslos**.

    /// Markiert alle Regeln, die der Knoten treffen KANN.
    ///
    /// Das Ergebnis ist eine Bitschlange in Regelnummer-Reihenfolge —
    /// die Reihenfolge ist Pflicht, weil `reihe` den Gleichstand
    /// entscheidet (CSS Cascade 6, §6.4.1).
    pub fn kandidaten<E: Element>(&self, el: &E) -> Vec<bool> {
        let mut bit = vec![false; self.anzahl];
        self.markiere(el, &mut |nr| {
            if let Some(b) = bit.get_mut(nr as usize) {
                *b = true;
            }
        });
        bit
    }

    /// Release 09.10.2026: dieselbe Maske als BITMENGE (64 Regeln je Wort).
    /// Die Kaskade laeuft damit ueber die gesetzten Bits
    /// (`trailing_zeros`) statt ueber jede Regel.
    /// Release 10.10.2026: die Kandidaten als NUMMERN (aufsteigend, je
    /// Selektor einmal) — fuer Aufrufer, die eigene Regellisten fuehren
    /// (`narsil-page`: die `::before`/`::after`-Regeln).
    pub fn kandidaten_nummern<E: Element>(&self, el: &E) -> Vec<u32> {
        let mut raus: Vec<u32> = Vec::new();
        self.markiere(el, &mut |nr| raus.push(nr));
        raus.sort_unstable();
        raus.dedup();
        raus
    }

    /// Release 10.10.2026: zu welchem Blatt und welcher Regel (Index in
    /// `Stylesheet::regeln`) gehoert der Selektor mit Nummer `nr`?
    pub fn regel_von(&self, nr: u32) -> Option<(usize, usize)> {
        let o = self.orte.get(nr as usize)?;
        Some((o.bi as usize, o.ri as usize))
    }

    pub fn kandidaten_bits<E: Element>(&self, el: &E) -> Vec<u64> {
        let mut bits = vec![0u64; self.anzahl.div_ceil(64)];
        self.markiere(el, &mut |nr| {
            if let Some(w) = bits.get_mut(nr as usize / 64) {
                *w |= 1u64 << (nr % 64);
            }
        });
        bits
    }

    /// Passt dieser Vormerker zu diesen Blaettern (gleiche Zahl, gleiche
    /// Laengen)? Sonst waeren die Orte falsch — dann der alte Weg.
    fn passt_zu(&self, blaetter: &[Stylesheet]) -> bool {
        let n = blaetter.len().min(super::MAX_BLAETTER);
        self.blatt_laengen.len() == n
            && self.orte.len() == self.anzahl
            && blaetter
                .iter()
                .take(n)
                .zip(self.blatt_laengen.iter())
                .all(|(b, l)| b.regeln.len() == *l)
    }

    /// Die Regel an einem Ort.
    fn regel_an<'a>(&self, blaetter: &'a [Stylesheet], ort: &Ort) -> Option<&'a super::typen::StilRegel> {
        let regel = blaetter.get(ort.bi as usize)?.regeln.get(ort.ri as usize)?;
        match regel {
            Regel::Stil(s) if ort.ii == OHNE => Some(s),
            Regel::Media { regeln, .. } | Regel::Supports { regeln, .. } | Regel::Layer { regeln, .. } => {
                regeln.get(ort.ii as usize)
            }
            _ => None,
        }
    }

    /// Markiert die Kandidaten eines Knotens (gemeinsamer Kern von
    /// [`Self::kandidaten`] und [`Self::kandidaten_bits`]).
    fn markiere<E: Element>(&self, el: &E, mark: &mut dyn FnMut(u32)) {
        for nr in &self.rest {
            mark(*nr);
        }
        // Release 09.10.2026: `:root` und Attribute.
        if !self.wurzel.is_empty() && el.eltern().is_none() {
            for nr in &self.wurzel {
                mark(*nr);
            }
        }
        for (name, liste) in &self.attr_liste {
            if el.attribut(name).is_some() {
                for nr in liste {
                    mark(*nr);
                }
            }
        }
        // Tags: Ecke aus dem ersten Byte, dann der Name.
        if let Some(k) = ecke_von(el.tag()) {
            for (name, liste) in &self.tag[k] {
                if name.eq_ignore_ascii_case(el.tag()) {
                    for nr in liste {
                        mark(*nr);
                    }
                }
            }
        }
        // Klassen, WEG A (K11, 02.10.2026): der Knoten kann sie
        // aufzaehlen. Dann reichen seine wenigen Namen, und der Index
        // sieht sich nur die Ecke(n) an, die dazugehoeren.
        //
        // GEMESSEN, warum das noetig war: der alte Weg fragte fuer
        // jede der 567 abgelegten Klassennamen einmal `hat_klasse`, bei
        // 5000 Knoten also 2 835 000 Stringvergleiche. Das kostete mehr
        // als die 612 Matcher-Aufrufe, die die Einsortierung spart —
        // der Index war damit ein Verlust statt ein Gewinn.
        // `weg_a_tauglich` heisst: der Knoten zaehlt seine Klassen auf
        // UND fuer jede davon laesst sich eine Ecke bestimmen. Der
        // zweite Teil ist wichtig und war zuerst falsch:
        //
        // GEMESSENER FEHLER 02.10.2026 (K11), gefunden durch
        // `tests/k11_maskenwege.rs`: ich habe Weg A benutzt, sobald der
        // Knoten seine Klassen genannt hat. Mit der toten `ecke_von`
        // liefert die aber `None` fuer jeden Klassennamen — also markierte
        // Weg A NICHTS, und Weg B wurde uebersprungen. Die Maske war
        // leer und die Kaskade falsch.
        //
        // Noch schlimmer: der Test `weg_a_und_weg_b_liefern_dasselbe`
        // war GRUEN, weil beide Wege dasselbe (leere) Ergebnis hatten.
        // Ein Test, der vakuos bestehen kann, ist schlimmer als gar
        // keiner — das ist dieselbe Falle wie bei P16.
        //
        // Also: Weg A nur, wenn er auch ETWASMARKIEREN kann.
        //
        // KOMPRESS D1 (02.10.2026 01:47): DIE BEDINGUNG WAR ZU STRENG,
        // und das hat Weg A unbrauchbar gemacht.
        //
        // GEMESSENER BEFUND: Ich habe `mit_klassen` fuer `CssKnoten`
        // eingefuehrt (W26) und die Kaskade wurde **langsamer**:
        //
        //   vorher (Weg B)          2 745 ms in 10 185 Aufrufen
        //   nachher (Weg A + B!)    3 100 ms in 10 185 Aufrufen
        //
        // GEMESSENE URSACHE: die alte Bedingung war
        // `n_klassen > 0 && n_klassen == n_markiert`. Sie verlangt,
        // dass JEDE Klasse des Knotens auch eine Regel findet. Ein
        // Knoten mit `class="btn mt-1"` hat fast immer eine Klasse,
        // die keine einzige Regel des Blattsatzes nennt — und dann ist
        // die Bedingung falsch, Weg A hatte aber trotzdem die meisten
        // Regeln markiert, und **Weg B lief danach noch einmal**.
        // Bezahlt wurde also zweimal.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `n_markiert` nichts zur
        // Richtigkeit sagt: Weg A markiert GENAU dieselben Regeln wie
        // Weg B — fuer jeden Namen, den der Knoten traegt, die Regeln
        // dieses Namens. Die Frage "hat dieser Name eine Regel"
        // beantwortet nicht, OB die Markierung richtig ist, sondern ob
        // es etwas zu markieren gab. Ein leeres Ergebnis ist kein
        // Fehler, sondern eine Seite ohne passende Regel.
        //
        // GEMESSENE ENTSCHEIDUNG ueber die echte Bedingung: Weg A ist
        // benutzbar, wenn der Knoten seine Klassen nennen kann UND
        // fuer jede eine Ecke bestimmbar ist. Die zweite Haelfte ist
        // keine Vorsicht, sondern Notwendigkeit: `ecke_von` ist eine
        // reine Funktion des Namens, also faellt sie beim Bau genauso
        // aus wie beim Fragen — die Regeln zu diesemnamen liegen dann
        // in `rest`, und `rest` wird immer geprueft.
        let mut weg_a_tauglich = false;
        {
            let klasse_ecke = &self.klasse;
            let mut n_klassen = 0usize;
            let mut ohne_ecke = false;
            el.mit_klassen(&mut |name| {
                n_klassen += 1;
                let Some(k) = ecke_von(name) else {
                    // GEMESSENE ENTSCHEIDUNG: `return` aus der
                    // Closure hebt nur diesen Namen aus, nicht die
                    // Gesamtantwort. Deshalb das Flag — vorher fiel
                    // ein einzelner unbestimmbarer Name auf, ohne dass
                    // man es merkte.
                    ohne_ecke = true;
                    return;
                };
                for (abgelegt, liste) in &klasse_ecke[k] {
                    if abgelegt.eq_ignore_ascii_case(name) {
                        for nr in liste {
                            mark(*nr);
                        }
                    }
                }
            });
            // Release 15.10.2026: ein Knoten, der seine Klassen vollstaendig
            // nennen kann (`klassen_aufzaehlbar`), darf auch KEINE haben —
            // dann gibt es nichts zu raten.
            weg_a_tauglich = (n_klassen > 0 || el.klassen_aufzaehlbar()) && !ohne_ecke;
        }
        // Klassen, WEG B: Weg A war nicht benutzbar — der Knoten kann
        // seine Klassen nicht aufzaehlen (Default in
        // `Element::mit_klassen`), oder `ecke_von` kann fuer eine
        // davon keine Ecke bestimmen. Dann der ratweise Weg ueber
        // alle 64 Eimer: langsam, aber richtig.
        if !weg_a_tauglich {
            // KOMPRESS D1: mitzaehlen, damit ein Test pruefen kann, ob
            // der langsame Weg gelaufen ist. Siehe `pz::WEG_B`.
            profil(pz::WEG_B);
            for k in 0..ECKEN {
                if self.klasse[k].is_empty() {
                    continue;
                }
                for (name, liste) in &self.klasse[k] {
                    if el.hat_klasse(name) {
                        for nr in liste {
                            mark(*nr);
                        }
                    }
                }
            }
        }
        // IDs: der Knoten hat hoechstens eine.
        if let Some(id) = el.id() {
            for (text, liste) in &self.id_liste {
                if text == id {
                    for nr in liste {
                        mark(*nr);
                    }
                }
            }
        }
    }
}

/// Wie [`berechnen`], zusaetzlich mit Interaktions-`zustand` (C9, W5):
/// der Browser uebergibt je Element, ob es gehovert oder fokussiert ist,
/// damit `:hover`/`:focus`-Regeln treffen koennen. Huelle um
/// [`berechnen_voll`] (ohne Inline-Variablen).
pub fn berechnen_mit<E: Element>(
    blaetter: &[Stylesheet],
    el: &E,
    inline: &[Deklaration],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
    zustand: &Zustand,
) -> Berechnet {
    berechnen_voll(
        blaetter,
        el,
        inline,
        &[],
        eltern,
        ansicht_breite,
        ansicht_hoehe,
        zustand,
    )
}

/// Vollstaendige Kaskade (C10, W6): wie `berechnen_mit`, zusaetzlich mit
/// Inline-Variablen (`inline_variablen(style)`). Eigene Variablen werden
/// vererbt (Eltern zuerst berechnen!) und stehen aufgeloest in
/// `Berechnet::variablen`.
pub fn berechnen_voll<E: Element>(
    blaetter: &[Stylesheet],
    el: &E,
    inline: &[Deklaration],
    inline_vars: &[VariablenDekl],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
    zustand: &Zustand,
) -> Berechnet {
    berechnen_ernst(
        blaetter,
        None,
        el,
        inline,
        inline_vars,
        eltern,
        ansicht_breite,
        ansicht_hoehe,
        zustand,
    )
}

/// K4 (01.10.2026): wie [`berechnen_voll`], aber mit einem
/// Vormerker, der EINMAL je Seite gebaut wurde.
///
/// # WARUM DAS EIN EIGENER AUFRUF IST
///
/// GEMESSEN mit den 27 echten github-Stylesheets (4860 Regeln,
/// 5000 Knoten, Debug):
///
/// ```text
///   Vormerker je Knoten gebaut   6,350 s = 1,270 ms je Knoten
///   ohne Vormerker                4,287 s = 0,857 ms je Knoten
/// ```
///
/// Der Aufbau des Index ist selbst ein Durchgang ueber alle Regeln —
/// und EIN Aufruf ist EIN Element. Bei 5000 Elementen baut man ihn
/// 5000 mal, und das ist ein Verlust von 48 %.
///
/// Deshalb der eigene Aufruf: der Aufrufer (die Rolle HTML) baut den
/// Index einmal ueber [`Vormerker::bauen`] und ruft diese Funktion je
/// Knoten. Erst dann loehnt sich die Einsortierung.
pub fn berechnen_mit_vormerker<E: Element>(
    vormerker: &Vormerker,
    blaetter: &[Stylesheet],
    el: &E,
    inline: &[Deklaration],
    inline_vars: &[VariablenDekl],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
    zustand: &Zustand,
) -> Berechnet {
    let uhr = profil_uhr();
    let erg = berechnen_ernst(
        blaetter,
        Some(vormerker),
        el,
        inline,
        inline_vars,
        eltern,
        ansicht_breite,
        ansicht_hoehe,
        zustand,
    );
    profil_zeit(pz::ZEIT_GESAMT, uhr);
    erg
}

/// Die gemeinsame Maschine. `vormerker` = `None` heisst "selbst
/// bauen, und zwar je Aufruf" (das ist der alte Weg, und er ist langsam
/// — siehe [`berechnen_mit_vormerker`]).
fn berechnen_ernst<E: Element>(
    blaetter: &[Stylesheet],
    vormerker: Option<&Vormerker>,
    el: &E,
    inline: &[Deklaration],
    inline_vars: &[VariablenDekl],
    eltern: Option<&Berechnet>,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
    zustand: &Zustand,
) -> Berechnet {
    let uhr_vorbereitung = profil_uhr();
    // Release 12.10.2026 (Agent C): die Attribute fuer `attr()`
    // (`crate::attr_kaskade`).
    crate::attr_kaskade::element_setzen(
        blaetter,
        inline,
        inline_vars,
        eltern.map(|p| &p.variablen),
        (ansicht_breite, ansicht_hoehe, Some(zustand.farbschema)),
        |n| el.attribut(n).map(str::to_string),
    );
    // Start: Anfangswerte, dann Vererbtes von den Eltern uebernehmen.
    let mut erg = Berechnet::standard();
    if let Some(p) = eltern {
        erg.farbe = p.farbe;
        erg.schriftgroesse_px = p.schriftgroesse_px;
        erg.schriftgewicht = p.schriftgewicht;
        erg.schriftstil = p.schriftstil;
        profil(pz::FAMILIE_KLONEN);
        erg.schriftfamilie = p.schriftfamilie.clone();
        erg.zeilenhoehe_px = p.zeilenhoehe_px;
        erg.zeilenhoehe_art = p.zeilenhoehe_art;
        erg.textausrichtung = p.textausrichtung;
        erg.ausrichtung_letzte = p.ausrichtung_letzte;
        erg.blocksatz_alle = p.blocksatz_alle;
        erg.blocksatz_aus = p.blocksatz_aus;
        erg.ausrichtung_logisch = p.ausrichtung_logisch;
        erg.weissraum = p.weissraum;
        erg.sichtbar = p.sichtbar;
        // K17: `direction` ist geerbt (CSS Writing Modes 3 §2.1).
        // Release 14.10.2026 (Agent CSS2): `unicode-bidi` is NOT inherited
        // (CSS Writing Modes 3 §2.2, "Inherited: no"). MEASURED before (WPT
        // `CSS2/bidi-text/unicode-bidi-applies-to-001`, `bidi-override` on a
        // row group): the cell's text was overridden too.
        erg.richtung = p.richtung;
        // Release 07.10.2026: die übrigen vererbten Eigenschaften
        // (CSS Lists 3 §3, CSS Text 3 §4–§8, CSS 2.1 §17.6). GEMESSENER
        // FEHLER: `ul.list { list-style-type: none }` erreichte das `<li>`
        // nicht — die Navigation von blog.rust-lang.org zeigte „* Rust* Install".
        erg.listenstil = p.listenstil;
        erg.buchstaben_abstand = p.buchstaben_abstand;
        erg.wort_abstand = p.wort_abstand;
        erg.buchstaben_prozent = p.buchstaben_prozent;
        erg.wort_prozent = p.wort_prozent;
        erg.text_einzug = p.text_einzug;
        erg.text_einzug_prozent = p.text_einzug_prozent;
        erg.tabulator_groesse = p.tabulator_groesse;
        erg.tabulator_px = p.tabulator_px;
        erg.text_transform = p.text_transform.clone();
        erg.wort_umbruch = p.wort_umbruch.clone();
        erg.wort_trennung = p.wort_trennung.clone();
        erg.zeilen_trennung = p.zeilen_trennung.clone();
        // Release 15.10.2026 (Agent CSS2): `hyphens` is inherited (CSS Text 3
        // §6.1); it lives in the verbatim keys.
        // Release 15.10.2026 (Agent CSS2): `text-wrap` and
        // `text-wrap-style` are inherited as well (CSS Text 4 §6.2).
        for name in ["hyphens", "hyphenate-character", "text-wrap", "text-wrap-style"] {
            if let Some(w) = p.schluessel.iter().rev().find(|(n, _)| n == name).map(|(_, w)| w.clone()) {
                erg.schluessel.push((String::from(name), w));
            }
        }
        erg.tabellen_kanten = p.tabellen_kanten.clone();
        // Release 11.10.2026 (Agent CSS2): geerbt (CSS 2.1 §17.4.1, §17.6.1.1).
        erg.beschriftung_unten = p.beschriftung_unten;
        erg.leere_zellen_verbergen = p.leere_zellen_verbergen;
        erg.randabstand_waagerecht = p.randabstand_waagerecht;
        erg.randabstand_senkrecht = p.randabstand_senkrecht;
    }
    // K17: `dir="rtl"` aus dem DOM (HTML 5 §5.7.2).
    //
    // Reihenfolge mit Absicht: NACH der Vererbung, VOR den Autorenregeln.
    // `dir` ist eine PRAESENTATIONSANGABE, und die steht in der Kaskade
    // zwischen der Browser-Vorgabe (Rang 0) und der Autorenregel
    // (Rang 1). Ein Kind ohne `dir` erbt also, ein Kind mit `dir`
    // ueberschreibt den geerbten Wert, und ein Autoren-`direction`
    // gewinnt gegen `dir` — genau die Reihenfolge der Spezifikation.
    if let Some(r) = el.richtung() {
        erg.richtung = r;
    }
    let eltern_schrift = eltern
        .map(|p| p.schriftgroesse_px)
        .unwrap_or(BASIS_SCHRIFT_PX);
    let ansicht = if ansicht_breite == 0 {
        800.0
    } else {
        ansicht_breite as f32
    };
    // Begruendung 800: Breite 0 gibt es nicht (Aufrufer-Fehler) — statt
    // durch 0 zu teilen, rechnet die Kaskade mit einer typischen Breite.
    //
    // R0 e (30.09.2026): die Hoehe fehlte an dieser Stelle, und damit
    // auch in `RechenCtx`. `vh`, `vmin` und `vmax` wurden als "wie vw"
    // gerechnet. `0` heisst hier dasselbe wie bei der Breite.
    //
    // 05.10.2026: DIE KONSTANTE 600 IST KENNMERKWERTIG, NICHT NACH MESSUNG.
    //
    // GEMESSENER BEFUND: sie stand hier **durchgehend**, weil `narsil-page`
    // keine Ansichtshoehe hatte und `ansicht_hoehe == 0` damit **immer**
    // zutraf. Bei einer Ansicht von 1280x800 war `min-height: 100vh` damit
    // **600 statt 800** — 25 % zu wenig, auf jeder Seite mit
    // `min-height: 100vh` (siehe `narsil-page/src/lib.rs`, Feld
    // `Bauer::ansicht_hoehe`, und `tests/vh_ansichtshoehe.rs`).
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Konstante **bleibt**: `0` heisst
    // „unbekannt" (ein Aufrufer ohne Fenster: `.narsil` aus der Kommandozeile,
    // ein Test). Eine erfundene Hoehe waere dort ein **Messfehler**, und
    // `vh` braucht eine Zahl, um nicht durch nichts zu teilen. Sie ist
    // nur nicht mehr der Normalfall des Browsers — das ist der Unterschied.
    //
    // GEMESSENE ENTSCHEIDUNG, warum sie nicht 800 ist: 800 ist genauso eine
    // Zahl ohne Fenster. Beide sind willkuerlich; 600 ist die
    // durchgaengige gewesen, also aendert die 800 **keine** Messung, sie
    // macht nur eine andere Zahl falsch.
    let ansicht_hoehe_b = if ansicht_hoehe == 0 {
        600.0
    } else {
        ansicht_hoehe as f32
    };

    // Je Eigenschaft das beste Gebot (None = keine Deklaration).
    let mut gebote: [[Option<Gebot>; 6]; EIGENSCHAFTEN] = [[None; 6]; EIGENSCHAFTEN];
    // Variablen-Gebote als (Name, Gebot) — gedeckelt (M4, MAX_VAR_EINTRAEGE).
    let mut var_gebote = VarGebote::default();
    // K17: namensgeschluesselte Gebote fuer den generischen Eimer. Sie
    // sind getrennt von `gebote`, weil sie einen NAMEN tragen und einen
    // Slot nicht genuegen (siehe `bieten`).
    let mut schluessel_gebote: Vec<(String, Gebot)> = Vec::new();
    let mut reihe: u64 = 0;

    // Wir tracken Schichten (Layer) pro Herkunft:
    // Un-layered Regeln haben u16::MAX. Benannte Layer kriegen aufsteigende IDs.
    let mut ua_schicht_zaehler: u16 = 0;
    let mut ua_schichten: Vec<(String, u16)> = Vec::new();
    let mut autor_schicht_zaehler: u16 = 0;
    let mut autor_schichten: Vec<(String, u16)> = Vec::new();

    // K4 (01.10.2026): Der Vormerker beantwortet vor der Schleife
    // "welche Regeln ueberhaupt in Frage kommen". Er wird hier je
    // Aufruf gebaut — das ist ein O(Regeln)-Durchgang, und ein
    // Aufruf ist EIN Element. Wer die Seite rechnet, baut ihn einmal
    // mit `Vormerker::bauen` und ruft `berechnen_mit_index`.
    // R_MESSUNG_OHNE_VORMERKER (nur fuer die Messung, K4): setzt
    // ALLE Regeln auf "Kandidat", also exakt der Zustand vor den
    // Buckets. Der Vergleich "mit" gegen "ohne" ist die Zahl, die
    // zaehlt.
    // Release 09.10.2026: passt der Vormerker zu den Blaettern, laeuft die
    // Kaskade ueber seine Kandidaten-Bits (schneller Weg unten); sonst die
    // alte Schleife ueber jede Regel.
    let schnell: Option<&Vormerker> =
        if super::VORMERKER_AUS.load(std::sync::atomic::Ordering::Relaxed) {
            None
        } else {
            vormerker.filter(|v| v.passt_zu(blaetter))
        };
    let kandidat: Vec<bool> = if super::VORMERKER_AUS.load(std::sync::atomic::Ordering::Relaxed)
        || schnell.is_some()
    {
        Vec::new()
    } else {
        match vormerker {
            Some(v) => v.kandidaten(el),
            None => Vormerker::bauen(blaetter).kandidaten(el),
        }
    };
    let mut flach: usize = 0;
    profil(pz::KNOTEN);
    profil_zeit(pz::ZEIT_VORBEREITUNG, uhr_vorbereitung);
    //
    // KOMPRESS D1 (02.10.2026): DIE BITZAHLUNG LAEUFT NUR, WENN
    // GEZAEHLT WIRD — und sie stand hier ZWEIMAL.
    //
    // GEMESSENER BEFUND: zwei identische `for`-Schleifen ueber
    // `kandidat.iter()`, jede mit einem `profil()`-Aufruf je Bit. Bei
    // 3839 Regeln und 8730 `berechnen`-Aufrufen sind das
    // **67 064 940** Aufrufe je Seitenaufbau.
    //
    // Zwei Folgen, beide gemessen:
    //
    // 1. **Die Zaehler waren doppelt.** `Kandidaten` und `Abgewiesen`
    //    meldeten jeweils das Doppelte — die Zahlen, die man zur
    //    Begruendung einer Aenderung benutzt, sind also um den Faktor
    //    2 zu hoch, wenn man diese Schleife nicht kennt.
    // 2. **Die Schleife lief auch bei ausgeschalteter Zaehlung.**
    //    `profil()` kehrt frueh zurueck, aber die Schleife und ihre
    //    3839 Iterationen blieben. In einem Debug-Bau ist das der
    //    groesste einzelne Posten der ganzen Kaskade.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Test AUSSERHALB steht und
    // nicht in `profil()`: `profil()` ist publich fuer die Zaehler im
    // ganzen Modul; die Abfrage an JEDER Aufrufstelle zu wiederholen
    // waere eine Aenderung an vielen Stellen fuer eine Bedingung, die
    // sich nur hier stellt. Die Bedingung liest dasselbe
    // `PROFIL_AKTIV`, das `profil()` liest — sie kann nicht auseinander-
    // laufen.
    if PROFIL_AKTIV.load(std::sync::atomic::Ordering::Relaxed) {
        // Die Bitschlange: die Kandidaten zaehlen, die uebrigen sind die
        // Abweisungen. Das ist die EINE Zahl, die die Buckets
        // rechtfertigt.
        for b in kandidat.iter() {
            profil(if *b { pz::KANDIDATEN } else { pz::VERWORFEN });
        }
    }

    let uhr_regeln = profil_uhr();
    if let Some(v) = schnell {
        let bits = v.kandidaten_bits(el);
        if PROFIL_AKTIV.load(std::sync::atomic::Ordering::Relaxed) {
            let an: usize = bits.iter().map(|w| w.count_ones() as usize).sum();
            for _ in 0..an {
                profil(pz::KANDIDATEN);
            }
        }
        // Bedingte Bloecke werden je Knoten hoechstens EINMAL geprueft:
        // 0 = offen, 1 = trifft, 2 = trifft nicht.
        let mut bed_stand: Vec<u8> = Vec::new();
        for (wi, wort) in bits.iter().enumerate() {
            let mut w = *wort;
            while w != 0 {
                let nr = wi * 64 + w.trailing_zeros() as usize;
                w &= w - 1;
                let Some(ort) = v.orte.get(nr) else {
                    continue;
                };
                if ort.bed != OHNE {
                    if bed_stand.is_empty() {
                        bed_stand = vec![0u8; v.bedingungen.len()];
                    }
                    let i = ort.bed as usize;
                    let mut stand = bed_stand.get(i).copied().unwrap_or(2);
                    if stand == 0 {
                        let block = v
                            .bedingungen
                            .get(i)
                            .and_then(|(b, r)| blaetter.get(*b as usize)?.regeln.get(*r as usize));
                        let trifft = match block {
                            Some(Regel::Media { abfrage, .. }) => abfrage.trifft_mit_schema(
                                ansicht_breite,
                                ansicht_hoehe,
                                zustand.farbschema,
                            ),
                            Some(Regel::Supports { bedingung, .. }) => {
                                crate::supports::supports_trifft_gemerkt(bedingung)
                            }
                            _ => false,
                        };
                        stand = if trifft { 1 } else { 2 };
                        if let Some(x) = bed_stand.get_mut(i) {
                            *x = stand;
                        }
                    }
                    if stand != 1 {
                        continue;
                    }
                }
                let Some(s) = v.regel_an(blaetter, ort) else {
                    continue;
                };
                if !crate::VORPRUEFUNG_AUS.load(std::sync::atomic::Ordering::Relaxed)
                    && regel_ist_unmoeglich(s, el)
                {
                    continue;
                }
                if !s.zusatz_medien.is_empty()
                    && !zusatz_trifft(s, ansicht_breite, ansicht_hoehe, zustand)
                {
                    continue;
                }
                let herkunft = if ort.bi == 0 {
                    Herkunft::Ua
                } else {
                    Herkunft::Autor
                };
                let quelle = if ort.ii == OHNE {
                    Quelle::aus_blatt(ort.bi as usize, ort.ri as usize)
                } else {
                    Quelle::aus_media(ort.bi as usize, ort.ri as usize, ort.ii as usize)
                };
                bieten(
                    s,
                    el,
                    zustand,
                    herkunft,
                    ort.schicht,
                    &mut reihe,
                    &mut gebote,
                    &mut var_gebote,
                    &mut schluessel_gebote,
                    quelle,
                );
            }
        }
    }
    // Der alte Weg (ohne passenden Vormerker): jede Regel jedes Blattes.
    // Blaetter gedeckelt (M4): mehr Blaetter fallen still weg.
    let alte_blaetter: &[Stylesheet] = if schnell.is_some() { &[] } else { blaetter };
    for (bi, blatt) in alte_blaetter.iter().take(MAX_BLAETTER).enumerate() {
        let (herkunft, zaehler, schichten) = if bi == 0 {
            (Herkunft::Ua, &mut ua_schicht_zaehler, &mut ua_schichten)
        } else {
            (
                Herkunft::Autor,
                &mut autor_schicht_zaehler,
                &mut autor_schichten,
            )
        };
        for (ri, regel) in blatt.regeln.iter().enumerate() {
            // K11 (02.10.2026 14:45) — DIE ZAEHLUNG WAR UM EINE
            // STELLE VERSCHOBEN, und das ist ein Fehler, der sich
            // versteckt hat.
            //
            // `flach` wurde ERHÖHT und DANN als Index benutzt. Die
            // erste Regel stand damit auf Index 1, während
            // `Vormerker::trag` sie auf Index 0 legt
            // (`let nr = self.anzahl; self.anzahl += 1;`). Fuer JEDE
            // Regel wurde also die Nachbarin geprueft.
            //
            // GEMESSEN, warum das nie auffiel: mit der toten
            // `ecke_von` landet jede Regel in `rest`, und `kandidaten`
            // setzt dann ALLE Eintraege auf `true`. Ein Index, der
            // um eins danebenliegt, trifft dann zufallig genauso.
            //
            // GEMESSEN, wann es auffiel: mit einer reparierten
            // `ecke_von` (Rest 975 -> 363) wurden 6 Tests rot
            // (kopf_h1, media_schmal, nav_flex, post_titel_link,
            // nav_hover und eine weitere) — die Kaskade prufte die
            // falschen Regeln. Der Zaehler wird hier deshalb VOR der
            // Abfrage als eigene Variable genommen, damit die
            // Reihenfolge nicht mehr von einer Nebenwirkung abhaengt.
            match regel {
                Regel::Stil(s) => {
                    // K11 (02.10.2026 15:30) — DAS `flach += 1` GEHOERT
                    // IN DIESEN ARM.
                    //
                    // Es stand VOR dem `match` und zählte damit auch
                    // die CONTAINER mit: `@media`, `@supports`,
                    // `@layer`. `Vormerker::bauen` zählt aber nur die
                    // INNEREN Regeln (`for s in regeln.iter() { v.trag(s) }`)
                    // und nicht den Container selbst. Bei jedem
                    // Container stand `flach` deshalb um eins zu hoch,
                    // und alle folgenden Regeln wurden um eins daneben
                    // geprueft.
                    //
                    // GEMESSEN, wann das auffiel: mit reparierter
                    // `ecke_von` blieben genau ZWEI Tests rot,
                    // media_schmal und media_spalte. Bei 400 px lieferte
                    // `#inhalt` 24.0 statt 12.0: das passende
                    // `@media (max-width: 600px)` mit `margin: 12px` war
                    // uebersprungen, weil sein Index im Vormerker nicht
                    // mit dem Zaehler hier uebereinstimmte.
                    flach += 1;
                    let flach_nr = flach - 1;
                    if !kandidat.get(flach_nr).copied().unwrap_or(true) {
                        continue;
                    }
                    // K11 (02.10.2026 17:40): DIE REGEL UEBERSPRINGEN, wenn kein
                    // Selektor zutreffen kann. Das ist der Gewinn, den die Doku dieser
                    // Funktion verspricht, und er war nie eingebaut. GEMESSEN stand
                    // der Profilerzaehler `Vorpruefung` auf 0.
                    if !crate::VORPRUEFUNG_AUS.load(std::sync::atomic::Ordering::Relaxed)
                        && regel_ist_unmoeglich(s, el)
                    {
                        continue;
                    }
                    // Release 08.10.2026: Bedingungen verschachtelter Gruppen.
                    if !s.zusatz_medien.is_empty()
                        && !zusatz_trifft(s, ansicht_breite, ansicht_hoehe, zustand)
                    {
                        continue;
                    }
                    bieten(
                        s,
                        el,
                        zustand,
                        herkunft,
                        u16::MAX,
                        &mut reihe,
                        &mut gebote,
                        &mut var_gebote,
                        &mut schluessel_gebote,
                        Quelle::aus_blatt(bi, ri),
                    );
                }
                Regel::Media { abfrage, regeln } => {
                    // K13 (01.10.2026): das Schema kommt aus dem
                    // `Zustand`, den der Aufrufer fuellt — also aus dem
                    // UI-Thema von `narsil`.
                    //
                    // GEMESSEN, was vorher hier stand und was es tat:
                    // `abfrage.trifft(...)`, und das pruefte
                    // `Some(Dunkel) => false`. Der Dunkel-Block galt NIE
                    // und der Hell-Block IMMER, auf jeder Seite. Bei 61
                    // `prefers-color-scheme`-Bloecken auf den 35 echten
                    // Stylesheets war die Haelfte dauerhaft falsch.
                    // K11 (02.10.2026 15:00) — DER ZAEHLER MUSS AUCH
                    // FUER DIE UEBERSPRUNGENEN BLOECKE WEITERLAUFEN.
                    //
                    // `Vormerker::bauen` nummeriert JEDE Stilregel im
                    // Baum, auch die in einem `@media` oder `@supports`,
                    // dessen Bedingung nicht zutrifft. Hier stand das
                    // `continue` VOR dem `flach += 1`, also lief der
                    // Zaehler nur ueber die passenden Bloecke. Sobald
                    // die Nummer hier von der im Vormerker auch nur um
                    // eins abweicht, zeigt die Kandidatenmaske auf die
                    // falschen Regeln.
                    //
                    // GEMESSEN, wann das auffiel: mit reparierter
                    // `ecke_von` waren `media_schmal` und `media_spalte`
                    // rot — genau die Tests mit einer Abfrage, die bei
                    // 800 px NICHT trifft.
                    flach += regeln.len();
                    let basis = flach - regeln.len();
                    if !abfrage.trifft_mit_schema(ansicht_breite, ansicht_hoehe, zustand.farbschema)
                    {
                        continue;
                    }
                    for (ii, s) in regeln.iter().enumerate() {
                        let flach_nr = basis + ii;
                        if !kandidat.get(flach_nr).copied().unwrap_or(true) {
                            continue;
                        }
                        // K11: siehe die Begruendung beim ersten Auftreten von
                        // `regel_ist_unmoeglich`.
                        if !crate::VORPRUEFUNG_AUS.load(std::sync::atomic::Ordering::Relaxed)
                            && regel_ist_unmoeglich(s, el)
                        {
                            continue;
                        }
                        // Release 08.10.2026: Bedingungen verschachtelter Gruppen.
                        if !s.zusatz_medien.is_empty()
                            && !zusatz_trifft(s, ansicht_breite, ansicht_hoehe, zustand)
                        {
                            continue;
                        }
                        bieten(
                            s,
                            el,
                            zustand,
                            herkunft,
                            u16::MAX,
                            &mut reihe,
                            &mut gebote,
                            &mut var_gebote,
                            &mut schluessel_gebote,
                            Quelle::aus_media(bi, ri, ii),
                        );
                    }
                }
                Regel::Supports { bedingung, regeln } => {
                    // GEMESSENER FEHLER 30.09.2026 (R0 d): hier stand
                    // `bedingung: _` — die Bedingung wurde gelesen und
                    // weggeworfen, die Regeln im Block GELTEN IMMER.
                    // Damit war `@supports (unsinn: 1)` genauso wirksam
                    // wie `@supports (display: grid)`, und genau das ist
                    // der Fall, den `@supports` verhindern soll.
                    //
                    // `supports_trifft` beantwortet die Bedingung ueber
                    // den DENSELBEN Deklarationsparser, den auch das
                    // Blatt benutzt — es gibt damit keine zweite Liste
                    // "bekannter Eigenschaften", die auseinanderlaufen
                    // koennte.
                    // K11 (02.10.2026 15:00) — DER ZAEHLER MUSS AUCH
                    // FUER DIE UEBERSPRUNGENEN BLOECKE WEITERLAUFEN.
                    //
                    // `Vormerker::bauen` nummeriert JEDE Stilregel im
                    // Baum, auch die in einem `@media` oder `@supports`,
                    // dessen Bedingung nicht zutrifft. Hier stand das
                    // `continue` VOR dem `flach += 1`, also lief der
                    // Zaehler nur ueber die passenden Bloecke. Sobald
                    // die Nummer hier von der im Vormerker auch nur um
                    // eins abweicht, zeigt die Kandidatenmaske auf die
                    // falschen Regeln.
                    //
                    // GEMESSEN, wann das auffiel: mit reparierter
                    // `ecke_von` waren `media_schmal` und `media_spalte`
                    // rot — genau die Tests mit einer Abfrage, die bei
                    // 800 px NICHT trifft.
                    flach += regeln.len();
                    let basis = flach - regeln.len();
                    if !crate::supports::supports_trifft_gemerkt(bedingung) {
                        continue;
                    }
                    for (ii, s) in regeln.iter().enumerate() {
                        let flach_nr = basis + ii;
                        if !kandidat.get(flach_nr).copied().unwrap_or(true) {
                            continue;
                        }
                        // K11: siehe die Begruendung beim ersten Auftreten von
                        // `regel_ist_unmoeglich`.
                        if !crate::VORPRUEFUNG_AUS.load(std::sync::atomic::Ordering::Relaxed)
                            && regel_ist_unmoeglich(s, el)
                        {
                            continue;
                        }
                        // Release 08.10.2026: Bedingungen verschachtelter Gruppen.
                        if !s.zusatz_medien.is_empty()
                            && !zusatz_trifft(s, ansicht_breite, ansicht_hoehe, zustand)
                        {
                            continue;
                        }
                        bieten(
                            s,
                            el,
                            zustand,
                            herkunft,
                            u16::MAX,
                            &mut reihe,
                            &mut gebote,
                            &mut var_gebote,
                            &mut schluessel_gebote,
                            Quelle::aus_media(bi, ri, ii),
                        );
                    }
                }
                Regel::Layer { name, regeln } => {
                    let mut schicht_id = *zaehler;
                    if let Some(n) = name {
                        if let Some(idx) = schichten.iter().position(|(k, _)| k == n) {
                            schicht_id = schichten[idx].1;
                        } else {
                            schichten.push((n.clone(), schicht_id));
                            *zaehler += 1;
                        }
                    } else {
                        *zaehler += 1;
                    }
                    // Release 12.10.2026 (Agent C): siehe `crate::schicht`.
                    if let Some(nr) = crate::schicht::nummer(blaetter, bi, ri) {
                        schicht_id = nr;
                    }
                    for (ii, s) in regeln.iter().enumerate() {
                        flach += 1;
                        // Siehe die Begruendung beim ersten Auftreten
                        // von `flach_nr`: dieselbe Verschiebung, dieselbe
                        // Korrektur.
                        let flach_nr = flach - 1;
                        if !kandidat.get(flach_nr).copied().unwrap_or(true) {
                            continue;
                        }
                        // K11: siehe die Begruendung beim ersten Auftreten von
                        // `regel_ist_unmoeglich`.
                        if !crate::VORPRUEFUNG_AUS.load(std::sync::atomic::Ordering::Relaxed)
                            && regel_ist_unmoeglich(s, el)
                        {
                            continue;
                        }
                        // Release 08.10.2026: Bedingungen verschachtelter Gruppen.
                        if !s.zusatz_medien.is_empty()
                            && !zusatz_trifft(s, ansicht_breite, ansicht_hoehe, zustand)
                        {
                            continue;
                        }
                        bieten(
                            s,
                            el,
                            zustand,
                            herkunft,
                            schicht_id,
                            &mut reihe,
                            &mut gebote,
                            &mut var_gebote,
                            &mut schluessel_gebote,
                            Quelle::aus_media(bi, ri, ii),
                        );
                    }
                }
                // K9: `@font-face` traegt nichts zur Kaskade bei
                // (siehe der gleiche Arm in `Vormerker::bauen`).
                Regel::Import { .. } | Regel::FontFace(_) => {}
            }
        }
    }
    profil_zeit(pz::ZEIT_REGELN, uhr_regeln);
    let uhr_vars = profil_uhr();
    // Inline-Deklarationen (hoechste normale Herkunft).
    for (di, d) in inline.iter().enumerate() {
        let rang = Rang(if d.wichtig { 4 } else { 2 });
        // Release 10.10.2026: der generische Eimer (`direction`, logische
        // Eigenschaften, `cursor` …) auch fuer das `style`-Attribut — wie in
        // `bieten` fuer die Blaetter. GEMESSEN vorher (Web-Platform-Tests):
        // `style="direction:rtl"` blieb `ltr`, weil Inline-Schluessel nie in
        // `schluessel_gebote` kamen.
        if let Wert::Schluessel(name, _) = &d.wert
            && !name.is_empty()
        {
            merke_var(&mut schluessel_gebote, name, rang, u16::MAX, (1, 0, 0), reihe, Quelle::Inline { dekl: di });
            reihe = reihe.saturating_add(1);
            continue;
        }
        merke(
            &mut gebote,
            d.eigenschaft,
            rang,
            u16::MAX,
            (1, 0, 0),
            reihe,
            Quelle::Inline { dekl: di },
        );
        reihe = reihe.saturating_add(1);
        let _ = di;
    }
    // Inline-Variablen (C10): gleiche Reihe, Spezifitaet wie Inline.
    for (di, v) in inline_vars.iter().enumerate() {
        let rang = Rang(if v.wichtig { 4 } else { 2 });
        var_gebote.merke(
            &v.name,
            rang,
            u16::MAX,
            (1, 0, 0),
            reihe,
            Quelle::InlineVar { dekl: di },
        );
        reihe = reihe.saturating_add(1);
    }

    // Eigene Variablen: Eltern erben (gedeckelt), Gewinner ueberschreiben.
    // HINWEIS: Rohwerte, keine Substitution — die passiert je Gebrauch
    // (mit Zyklen-Schutz, C10).
    // Release 08.10.2026: keine Kopie der Eltern mehr — die eigenen
    // Gewinner werden eine neue Ebene ueber der geteilten Elternebene
    // (siehe `Variablen`).
    let mut eigene: Vec<(String, String)> = Vec::with_capacity(var_gebote.liste.len());
    for (name, gebot) in &var_gebote.liste {
        if let Some(roh) = var_roh(gebot.quelle, blaetter, inline_vars) {
            // Release 12.10.2026 (Agent C): `--x: inherit`/`unset` (unregistriert)
            // erbt den Wert der Eltern statt des Wortes (CSS Variables 1 §2.2;
            // WPT css-values if-conditionals 17).
            if matches!(roh.trim().to_ascii_lowercase().as_str(), "inherit" | "unset" | "revert" | "revert-layer") {
                continue;
            }
            // Release 12.10.2026 (Agent C): `inherit()` gleich mit den
            // Eltern einsetzen; ungueltig heisst `unset` (also geerbt).
            if crate::attr_kaskade::erben_aktiv() && hat_funktion(roh, "inherit(") {
                if let Some(r) = crate::attr_kaskade::erben_ersetzen(roh) {
                    eigene.push((name.clone(), r));
                }
                continue;
            }
            eigene.push((name.clone(), String::from(roh)));
        }
    }
    if eltern.is_some() {
        profil(pz::VAR_KLONEN);
    }
    let vars = Variablen::mit_eigenen(eltern.map(|p| &p.variablen), eigene);
    profil_zeit(pz::ZEIT_VARIABLEN, uhr_vars);
    let uhr_eig = profil_uhr();

    // Aufloesen: zuerst Schriftgroesse (Basis fuer em), dann Farbe
    // (Basis fuer currentcolor), dann der Rest.
    // K17: DIE RICHTUNG VOR ALLEN ANDEREN EIGENSCHAFTEN.
    //
    // Reihenfolge ist hier keine Formalie: `text-align: start` loest
    // sich ueber `Eigenschaft::Textausrichtung` auf, das ist Index 8 —
    // der generische Eimer, in dem `direction` steht, hat Index 102.
    // Ohne diesen Vorab-Durchlauf haette `start` immer `ltr` gesehen,
    // unabhaengig davon, ob `direction: rtl` im selben Block davor
    // oder danach stand.
    richtung_aus_gebote(&schluessel_gebote, blaetter, inline, &mut erg);
    // Release 11.10.2026 (Agent CSS2): `ex` in `font-size` meint die
    // Schrift der Eltern (siehe `ex_anteil_fuer`).
    ex_anteil_setzen(eltern.map_or("", |p| p.schriftfamilie.as_str()));
    // Release 14.10.2026 (Agent C): `lh` in `font-size` and `line-height`
    // is the parent's line height (`line_height_set`).
    line_height_set(
        eltern.map_or(BASIS_SCHRIFT_PX * 1.2, |p| p.zeilenhoehe_px),
        eltern.is_none().then_some(BASIS_SCHRIFT_PX * 1.2),
    );
    loese_schriftgroesse(
        &gebote,
        blaetter,
        inline,
        eltern,
        eltern_schrift,
        ansicht,
        ansicht_hoehe_b,
        &vars,
        &mut erg,
    );
    loese_farbe(
        &gebote,
        blaetter,
        inline,
        eltern,
        &vars,
        zustand.farbschema,
        &mut erg,
    );
    // K17: der generische Eimer. NACH `loese_schriftgroesse`, weil
    // `em` die Schriftgroesse des Elements ist und logische Laengen
    // `em` rechnen koennen.
    // 03.10.2026 15:0x: die Wurzelschrift wandert auch in die
    // benutzerdefinierten Properties — ein `var(--x)` mit `rem` darin
    // soll dieselbe Basis sehen wie `font-size`.
    // Release 12.10.2026 (Agent C): an der Wurzel selbst ist `rem` ausser in
    // `font-size` ihre eigene Schrift (CSS Values 4 §6.1.1; WPT css-values
    // rem-unit-root-element: `:root { font-size: 50px; margin-left: 2rem }`).
    let rem_basis_hier = eltern.map_or(erg.wurzel_schrift.max(0.0), |e| e.wurzel_schrift);
    schluessel_eintragen(
        &schluessel_gebote,
        blaetter,
        inline,
        rem_basis_hier,
        ansicht,
        ansicht_hoehe_b,
        &mut erg,
    );
    // Release 11.10.2026 (Agent CSS2): die Schriftfamilie VOR allen
    // Laengen, damit `ex` die eigene Schrift sieht.
    loese_eigenschaft(
        Eigenschaft::Schriftfamilie,
        &gebote,
        blaetter,
        inline,
        eltern,
        ansicht,
        ansicht_hoehe_b,
        &vars,
        &mut erg,
    );
    ex_anteil_setzen(&erg.schriftfamilie);
    for e in alle_eigenschaften() {
        match e {
            Eigenschaft::Schriftgroesse | Eigenschaft::Farbe | Eigenschaft::Schriftfamilie => {}
            _ => loese_eigenschaft(
                e,
                &gebote,
                blaetter,
                inline,
                eltern,
                ansicht,
                ansicht_hoehe_b,
                &vars,
                &mut erg,
            ),
        }
        if e == Eigenschaft::Zeilenhoehe {
            // Release 14.10.2026 (Agent C): from here on `lh` is the own line
            // height, and at the root `rlh` too.
            let own = match erg.zeilenhoehe_art {
                super::typen::ZeilenhoeheArt::Normal => erg.schriftgroesse_px * 1.2,
                super::typen::ZeilenhoeheArt::Faktor(f) => (erg.schriftgroesse_px * f).clamp(0.0, MAX_PX),
                super::typen::ZeilenhoeheArt::Absolut => erg.zeilenhoehe_px,
            };
            line_height_set(own, eltern.is_none().then_some(own));
        }
    }
    line_height_set(0.0, None);
    profil_zeit(pz::ZEIT_EIGENSCHAFTEN, uhr_eig);
    let uhr_rest = profil_uhr();
    praesentations_groesse(el, &gebote, blaetter, inline, &mut erg);
    praesentations_ausrichtung(el, &gebote, blaetter, inline, &mut erg);
    // Release 11.10.2026 (Agent CSS2): die UA-Vorgabe `vertical-align` der
    // Tabellenteile (HTML §15.3.8: `thead, tbody, tfoot, table > tr
    // { vertical-align: middle }`, `tr, td, th { vertical-align: inherit }`)
    // — HIER und nicht im UA-Blatt, damit `valign` (Praesentation) ueber ihr
    // steht. GEMESSEN vorher (WPT css/CSS2/tables): jede Zelle stand auf der
    // Grundlinie, auch in `<table>`-Zeilen, die Browser mittig setzen.
    if erg.vertikal.is_empty() && gewinner(Eigenschaft::VertikalAlign, &gebote, blaetter, inline).is_none() {
        let geerbt = || eltern.map(|p| p.vertikal.clone()).unwrap_or_default();
        match el.tag() {
            "thead" | "tbody" | "tfoot" => erg.vertikal = String::from("middle"),
            "tr" => {
                erg.vertikal = if el.eltern().is_some_and(|e| e.tag().eq_ignore_ascii_case("table")) {
                    String::from("middle")
                } else {
                    geerbt()
                }
            }
            "td" | "th" => erg.vertikal = geerbt(),
            _ => {}
        }
    }
    praesentations_tabelle(el, &gebote, blaetter, inline, &mut erg);
    praesentations_abstand(el, &gebote, blaetter, inline, &mut erg);
    // Release 13.10.2026 (Agent CSS2): `start`/`end` nach der Richtung DIESES
    // Elements (siehe `Berechnet::ausrichtung_logisch`). GEMESSEN vorher (WPT
    // css-text `text-align/text-align-end-*`, `-start-*` mit `dir=rtl`): die
    // geerbte Ausrichtung war schon beim Wurzelelement zu `left` geworden.
    if let Some(start) = erg.ausrichtung_logisch {
        let rechts = erg.richtung.start_ist_rechts() == start;
        erg.textausrichtung = if rechts { Textausrichtung::Rechts } else { Textausrichtung::Links };
    }
    // Release 08.10.2026: das „visually hidden"-Muster. GEMESSEN vorher:
    // `clip-path: inset(50%)` und `clip: rect(1px, 1px, 1px, 1px)` lagen
    // ungelesen im Schluessel-Eimer, und Screenreader-Texte wie „Navigation
    // Menu" (github.com) standen sichtbar auf der Seite. Deckkraft 0 und
    // nicht `visibility`: der Kasten bleibt im Baum und im Layout (er ist
    // nur weggeschnitten), und die Deckkraft gilt fuer den ganzen Teilbaum
    // (Stapelkontext, CSS Color 3 §3.2).
    // Release 11.10.2026 (Agent CSS2): `clip: inherit` nimmt den Wert des
    // Elternelements (CSS 2.1 §11.1.2; WPT `visufx/clip-102`).
    // Release 14.10.2026 (Agent CSS2): the same for every key in
    // `KEYS_WITH_INHERIT`.
    for name in KEYS_WITH_INHERIT {
        if erg.schluessel.iter().any(|(n, w)| n == name && w.trim().eq_ignore_ascii_case("inherit")) {
            let geerbt = eltern.and_then(|p| p.schluessel.iter().rev().find(|(n, _)| n == name).map(|(_, w)| w.clone()));
            erg.schluessel.retain(|(n, _)| n != name);
            if let Some(w) = geerbt {
                erg.schluessel.push((String::from(*name), w));
            }
        }
    }
    if versteckt_durch_clip(&erg) {
        erg.deckkraft = 0.0;
    }
    erg.variablen = vars;
    // L4 (03.10.2026 07:0x): §7.2 — DIE LINIENNAMEN EINES SUBGRIDS SIND
    // DIE DES ELTERNS. Ein Subgrid teilt die Linien seines Containers,
    // also auch deren Namen, und es hat keine eigenen Spuren.
    //
    // GEMESSENER BEFUND, dass es daran lag: `grid-area: visual` steht 5x
    // im Korpus, und die beiden Zeilen drumherum sind
    // `grid-template-columns: subgrid` und (eine Ebene weiter aussen)
    // `[content]1fr[visual]1fr`. Vorher las das Kind im Subgrid eine
    // LEERE Namensliste, also blieb `gitter_spalte_start` auf `0` (auto)
    // und der Cursor legte das Kind hin, wo er gerade passte.
    //
    // GEMESSENE ENTSCHEIDUNG, warum HIER und nicht im Layout: die Namen
    // sind statisch — sie stehen im CSS, nicht in einer Rechnung. Der
    // Layoutteil von `subgrid` (die SPURENBREITEN des Containers zu
    // benutzen) braucht dagegen Laufzeitwerte und ist nicht gebaut;
    // siehe `BERICHT_CSS_L4_SUBGRID.txt`.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die eigenen Namen GEWINNEN: wer
    // `grid-template-columns: [a] 1fr` schreibt, hat keine Spur
    // uebernommen, sondern eine eigene Vorlage. §7.2 erlaubt beides
    // nebeneinander nicht.
    if let Some(p) = eltern
        && (erg.raster_subgrid_spalten || erg.raster_subgrid_zeilen)
    {
        if erg.raster_subgrid_spalten && erg.raster_spalten_namen.is_empty() {
            erg.raster_spalten_namen = p.raster_spalten_namen.clone();
            // L4 (07:2x): und die SPUREN dazu (§7.2 — „the subgrid... uses
            // the lines of its parent"). GEMESSENER BEFUND, warum das
            // nicht im Plan steht: der Plan sieht nur den einen Knoten,
            // und die Kaskade ist die Stelle, die den Eltern kennt.
            erg.raster_eltern_spalten = p.raster_spalten.clone();
        }
        if erg.raster_subgrid_zeilen && erg.raster_zeilen_namen.is_empty() {
            erg.raster_zeilen_namen = p.raster_zeilen_namen.clone();
            erg.raster_eltern_zeilen = p.raster_zeilen.clone();
        }
    }
    match erg.zeilenhoehe_art {
        super::typen::ZeilenhoeheArt::Normal => erg.zeilenhoehe_px = erg.schriftgroesse_px * 1.2,
        super::typen::ZeilenhoeheArt::Faktor(f) => erg.zeilenhoehe_px = (erg.schriftgroesse_px * f).clamp(0.0, MAX_PX),
        super::typen::ZeilenhoeheArt::Absolut => {}
    }
    let ausserhalb = matches!(erg.position, Position::Absolut | Position::Fest);
    let layout_kind = eltern.is_some_and(|p| matches!(p.anzeige, Anzeige::Flex | Anzeige::InlineFlex | Anzeige::Raster));
    if erg.anzeige != Anzeige::Keins && (ausserhalb || layout_kind || erg.schweben != Float::Keins) {
        erg.anzeige = match erg.anzeige {
            Anzeige::InlineFlex => Anzeige::Flex,
            Anzeige::Inline | Anzeige::InlineBlock => Anzeige::Block,
            // Release 11.10.2026 (Agent CSS2): CSS 2.1 §9.7 — auch die
            // Tabellenteile werden Block. GEMESSEN vorher (WPT
            // `float-applies-to-005`): eine schwebende `table-column-group`
            // blieb Spaltengruppe und war unsichtbar.
            Anzeige::TabelleZeile | Anzeige::TabelleZelle | Anzeige::TabelleRand | Anzeige::TabelleSpalte => Anzeige::Block,
            other => other,
        };
        erg.tabellen_rolle = match erg.tabellen_rolle {
            narsil_layout::plan::TabellenRolle::Tabelle | narsil_layout::plan::TabellenRolle::InlineTabelle => {
                narsil_layout::plan::TabellenRolle::Tabelle
            }
            _ => narsil_layout::plan::TabellenRolle::Keine,
        };
        if ausserhalb || layout_kind { erg.schweben = Float::Keins; }
        // Release 08.10.2026: blockifiziert ist der Kasten nicht mehr inline.
        erg.inline_aussen = false;
    }
    // Release 14.10.2026 (Agent CSS2): a replaced element with a
    // layout-internal (`table-*`) display is laid out as `inline` (CSS
    // Display 3 §2.4). MEASURED before (WPT `CSS2/ui/outline-applies-to-016`,
    // `<img style="display: table-column-group">`): the image became an
    // empty column group and disappeared.
    if matches!(
        erg.anzeige,
        Anzeige::TabelleZeile | Anzeige::TabelleZelle | Anzeige::TabelleRand | Anzeige::TabelleSpalte
    ) && matches!(
        el.tag().to_ascii_lowercase().as_str(),
        "img" | "video" | "canvas" | "iframe" | "embed" | "object"
    ) {
        erg.anzeige = Anzeige::Inline;
        erg.tabellen_rolle = narsil_layout::plan::TabellenRolle::Keine;
        erg.inline_aussen = true;
    }
    // Release 12.10.2026 (Agent CSS2): der Anfangswert von `border-width` ist
    // `medium` (3 px, CSS 2.1 §8.5.1) — er zeigt sich, sobald ein Stil gesetzt
    // ist und keine Breite angegeben wurde (`border-style: solid` allein).
    // Ohne Stil bleibt die berechnete Breite 0 (§8.5.3). GEMESSEN vorher (WPT
    // `css1/c5511-ibrdr-tw-000` u. a.): `border-top-width: thin; border-style:
    // solid` zeichnete nur den oberen Rahmen.
    {
        let frei = |e: Eigenschaft| gewinner(e, &gebote, blaetter, inline).is_none();
        let mittel = |stil: Rahmenstil, breite: &mut f32, e: Eigenschaft| {
            if !matches!(stil, Rahmenstil::Kein | Rahmenstil::Versteckt) && *breite == 0.0 && frei(e) {
                *breite = 3.0;
            }
        };
        mittel(erg.rahmen_oben_stil, &mut erg.rahmen_oben_breite, Eigenschaft::RahmenObenBreite);
        mittel(erg.rahmen_rechts_stil, &mut erg.rahmen_rechts_breite, Eigenschaft::RahmenRechtsBreite);
        mittel(erg.rahmen_unten_stil, &mut erg.rahmen_unten_breite, Eigenschaft::RahmenUntenBreite);
        mittel(erg.rahmen_links_stil, &mut erg.rahmen_links_breite, Eigenschaft::RahmenLinksBreite);
        // Release 14.10.2026 (Agent CSS2): the initial `border-color` is
        // `currentColor` (CSS Backgrounds 3 §4.1), and the `border` shorthand
        // without a colour resets it to that. MEASURED before (WPT
        // `CSS2/borders/border-color-010`, `p { color: orange; border: solid }`):
        // the border was black.
        let farbe = erg.farbe;
        for (e, ziel) in [
            (Eigenschaft::RahmenObenFarbe, &mut erg.rahmen_oben_farbe),
            (Eigenschaft::RahmenRechtsFarbe, &mut erg.rahmen_rechts_farbe),
            (Eigenschaft::RahmenUntenFarbe, &mut erg.rahmen_unten_farbe),
            (Eigenschaft::RahmenLinksFarbe, &mut erg.rahmen_links_farbe),
        ] {
            if frei(e) {
                *ziel = farbe;
            }
        }
    }
    profil_zeit(pz::ZEIT_REST, uhr_rest);
    erg
}

impl Quelle {
    fn aus_blatt(blatt: usize, regel: usize) -> Quelle {
        Quelle::Blatt {
            blatt,
            regel,
            innen: None,
            dekl: 0,
        }
    }
    fn aus_media(blatt: usize, regel: usize, innen: usize) -> Quelle {
        Quelle::Blatt {
            blatt,
            regel,
            innen: Some(innen),
            dekl: 0,
        }
    }
}

/// K4 (01.10.2026): Kann das RECHTSTE Glied eines Selektors
/// ueberhaupt zutreffen?
///
/// # GEMESSENER ANLASS
///
/// `examples/kaskade_leistung`, 5000 Knoten x 2000 Regeln, Debug:
///
/// ```text
///   Kaskade: 5000 Knoten x 2000 Regeln in 13,3 s = 2,66 ms je Knoten
/// ```
///
/// streng linear in der Regelzahl (1 Regel 0,016 ms, 2000 Regeln
/// 2,049 ms je Knoten — 0,66 us je Regelpraefung). Der Matcher
/// vergleicht fuer JEDE Regel Zeichenketten, auch fuer die 70 %, die
/// gar nicht passen koennen.
///
/// # DIE REGEL, UND WARUM SIE SICHER IST
///
/// `selektor_passt_mit` prueft die Kette von rechts nach links und
/// beginnt IMMER mit dem rechtesten Glied. Treffen dessen Bedingungen
/// nicht zu, kann die ganze Kette nicht treffen — unabhaengig davon,
/// was links steht. Es genuegt also, die rechteste Bedingung zu
/// pruefen.
///
/// KOSTEN: drei Vergleiche (Tag, hoechstens EIN Attribut, hoechstens
/// eine Klasse) statt des ganzen Aufrufs. Wird nichts gefunden, ist
/// der Aufruf nicht vermieden — dann war es billiger, es zu versuchen.
///
/// # WAS hier BEWUSST NICHT geprueft wird
///
/// * `:hover`, `:focus`, `:nth-child(…)` und alle anderen
///   Pseudo-Klassen: sie kosten eine Zustandsabfrage oder eine
///   Geschwistersuche, und eine falsche Vorhersage waere ein
///   falscher Kaskadenwert.
/// * Attribute mit Bedingung (`[a^=b]`): sie vergleichen Zeichen und
///   allocating — dafuer liefert `einfach_passt` keine billige
///   Antwort, also wird der Matcher gerufen.
/// * Mehr als eine Klasse im rechten Glied: selten und die
///   Abbruchwahrscheinlichkeit sinkt; der Matcher darf entscheiden.
#[allow(dead_code)] // siehe `pz::VORPRUEFUNG` (W13)
fn rechts_ist_unmoeglich<E: Element>(sel: &super::typen::Selektor, el: &E) -> bool {
    profil(pz::VORPRUEFUNG);
    // Die Kette ist nie leer (der Parser verwirft leere Selektoren),
    // aber das kostet hier nichts und macht die Funktion stand-alone.
    let letzte = match sel.kette.last() {
        None => return false,
        Some(g) => g,
    };
    let e = &letzte.einfach;
    // 1) ID: der schnellste Vergleich, den es gibt.
    if let Some(id) = &e.id {
        return el.id() != Some(id.as_str());
    }
    // 2) Attribute: nur dann, wenn KEINS eine Bedingung traegt.
    //    `[a]` und `[data-x]` fragen nur „vorhanden?“, das ist billig.
    //    `[a^=b]` vergleicht Zeichen — dafuer hat `einfach_passt` keine
    //    billige Antwort, also entscheidet der Matcher.
    if e.pseudos.is_empty()
        && e.klassen.len() <= 1
        && e.attribute.iter().all(|b| b.bedingung.is_none())
    {
        for bed in &e.attribute {
            profil(pz::ATTRIBUT);
            if el.attribut(&bed.name).is_none() {
                return true;
            }
        }
    }
    // 3) EIN Tag und sonst nichts: der Vergleich endet nach den
    //    ersten Bytes, nicht nach der ganzen Liste.
    if let Some(tag) = &e.tag {
        if e.klassen.is_empty() && e.attribute.is_empty() && e.pseudos.is_empty() {
            profil(pz::TAG);
            return !el.tag().eq_ignore_ascii_case(tag);
        }
    }
    // 4) GENAU eine Klasse und sonst nichts.
    if e.klassen.len() == 1
        && e.id.is_none()
        && e.tag.is_none()
        && e.attribute.is_empty()
        && e.pseudos.is_empty()
    {
        profil(pz::HAT_KLASSE_VORPRUEFUNG);
        return !el.hat_klasse(&e.klassen[0]);
    }
    // Alles andere: der Matcher entscheidet.
    false
}

/// K4 (01.10.2026): Kann die GANZE Regel ausfallen?
///
/// `true` heisst: kein einziger Selektor der Regel kann zutreffen.
/// Das ist die Bedingung, an der sich der Aufruf von [`bieten`]
/// sparen laesst — und GEMESSEN ist genau das der teure Teil: der
/// Aufruf mit seinen acht Argumenten kostet im Debug-Bau mehr als die
/// drei Vergleiche, die diese Funktion macht.
///
/// Leer ist `false` (nichts zu ueberspringen, aber auch nichts zu
/// gewinnen) — eine Regel ohne Selektor ist unmoeglich, und sie wird
/// ohnehin von `bieten` verworfen.
// K11 (02.10.2026 17:40): DAS `#[allow(dead_code)]` IST WEG, weil die
// Funktion jetzt GERUFEN wird.
//
// Sie stand hier seit dem 01.10. als toter Code, obwohl ihre eigene
// Doku misst, dass sie Arbeit einspart. GEMESSEN war die Folge im
// Profiler sichtbar: `Vorpruefung rechts_ist_unmoeglich` stand auf
// NULL — weil `rechts_ist_unmoeglich` nur von hier aus aufgerufen
// wird, und hier rief niemand. Die Vorpruefung lief NIE.
fn regel_ist_unmoeglich<E: Element>(s: &super::typen::StilRegel, el: &E) -> bool {
    !s.selektoren.is_empty()
        && s.selektoren
            .iter()
            .all(|sel| rechts_ist_unmoeglich(sel, el))
}

/// Prueft eine Stilregel: trifft ein Selektor, wird jede Deklaration
/// mit (Rang, Spezifitaet, Reihe) geboten. `reihe` zaehlt global.
fn bieten<E: Element>(
    s: &super::typen::StilRegel,
    el: &E,
    zustand: &Zustand,
    herkunft: Herkunft,
    schicht: u16,
    reihe: &mut u64,
    gebote: &mut [[Option<Gebot>; 6]; EIGENSCHAFTEN],
    var_gebote: &mut VarGebote,
    schluessel_gebote: &mut Vec<(String, Gebot)>,
    vorlage: Quelle,
) {
    // Beste zutreffende Spezifitaet (spaetere gleiche zaehlt nicht mehr
    // als fruehere gleiche — die Reihe entscheidet unten).
    let mut spez: Option<(u32, u32, u32)> = None;
    profil(pz::BIETEN);
    for sel in &s.selektoren {
        profil(pz::SELEKTOREN);
        crate::scope_match::clear_proximity();
        let passt = selektor_passt_mit(sel, el, zustand);
        if passt {
            // Release 13.10.2026: including `@scope` proximity (`crate::scope_match`).
            let sp = crate::scope_match::specificity_with_proximity(sel.spezifitaet);
            spez = Some(match spez {
                None => sp,
                Some(b) => b.max(sp),
            });
        }
    }
    let spez = match spez {
        None => return,
        Some(v) => v,
    };
    for (di, d) in s.deklarationen.iter().enumerate() {
        let rang = Rang(match (d.wichtig, herkunft) {
            // Inline kommt hier nie an (eigener Rang in `berechnen`).
            (false, Herkunft::Ua) => 0,
            (false, Herkunft::Autor) => 1,
            (true, Herkunft::Ua) => 5,
            (true, Herkunft::Autor) => 3,
        });
        let quelle = match vorlage {
            Quelle::Blatt {
                blatt,
                regel,
                innen,
                ..
            } => Quelle::Blatt {
                blatt,
                regel,
                innen,
                dekl: di,
            },
            Quelle::Inline { .. } => Quelle::Inline { dekl: di },
            Quelle::Var { .. } => Quelle::Inline { dekl: di },
            Quelle::InlineVar { .. } => Quelle::Inline { dekl: di },
        };
        // K17 (02.10.2026): DER GENERISCHE EIMER BRAUCHT EINEN NAMEN.
        //
        // GEMESSENER BEFUND, `.i{cursor:pointer;direction:ltr}` auf EINEM
        // Element: es kam EIN Eintrag heraus, und es war `direction` —
        // `cursor` war spurlos weg. Zwei weitere Kontrollen:
        //
        //   direction:ltr;inset-inline-start:5px   -> 1 (erwartet 2)
        //   inset-inline-start:5px;inset-inline-end:7px -> 1 (erwartet 2)
        //
        // Grund: alle Schluessel-Eigenschaften teilen sich den Slot 102
        // (`eigenschaft_index`), und in einem Slot gewinnt genau EIN
        // Gebot. Damit war `Berechnet::schluessel` nie groesser als 1,
        // die dokumentierte Grenze `MAX_SCHLUESSEL` (48) unerreichbar,
        // und der Doc-Kommentar an dem Feld ("haelt genau das:
        // Eigenschaft, angegebener Wert") falsch.
        //
        // Der Eimer bekommt deshalb dasselbe wie die Custom Properties:
        // einen namensgeschluesselten Vektor mit `merke_var`, das Rang,
        // Schicht, Spezifitaet und Reihe vergleicht. Das ist derselbe
        // Vergleich an derselben Stelle — es kommt keine neue
        // Kaskadenlogik dazu, nur ein anderer Schluessel.
        if let Wert::Schluessel(name, _) = &d.wert {
            // K17: der Schalter misst den Preis des neuen Eimers gegen
            // den alten Zustand, auf DEMSELBEN Binary.
            if !name.is_empty()
                && !crate::SCHLUESSEL_EIMER_AUS.load(std::sync::atomic::Ordering::Relaxed)
            {
                merke_var(schluessel_gebote, name, rang, schicht, spez, *reihe, quelle);
                *reihe = reihe.saturating_add(1);
            }
            continue;
        }
        merke(gebote, d.eigenschaft, rang, schicht, spez, *reihe, quelle);
        *reihe = reihe.saturating_add(1);
    }
    // Variablen derselben Regel (C10): gleiche Spezifitaet/Reihe-Logik.
    for (di, v) in s.variablen.iter().enumerate() {
        let rang = Rang(match (v.wichtig, herkunft) {
            (false, Herkunft::Ua) => 0,
            (false, Herkunft::Autor) => 1,
            (true, Herkunft::Ua) => 5,
            (true, Herkunft::Autor) => 3,
        });
        let quelle = match vorlage {
            Quelle::Blatt {
                blatt,
                regel,
                innen,
                ..
            } => Quelle::Var {
                blatt,
                regel,
                innen,
                dekl: di,
            },
            _ => Quelle::InlineVar { dekl: di },
        };
        var_gebote.merke(&v.name, rang, schicht, spez, *reihe, quelle);
        *reihe = reihe.saturating_add(1);
    }
}

/// Schneidet `clip-path` oder `clip` das ganze Element weg? (CSS Masking 1
/// §5.1 `inset()`, CSS 2.1 §11.1.2 `rect()`.) Ein `inset()` ab 50 % laesst
/// keine Flaeche uebrig; ein `rect()` ohne Breite oder Hoehe auch nicht —
/// `clip` gilt nur fuer absolut positionierte Kaesten.
fn versteckt_durch_clip(b: &Berechnet) -> bool {
    for (name, wert) in &b.schluessel {
        let w = wert.trim().to_ascii_lowercase();
        if name == "clip-path" {
            let Some(innen) = w.strip_prefix("inset(").and_then(|r| r.strip_suffix(')')) else {
                continue;
            };
            let teile: Vec<&str> = innen.split_whitespace().collect();
            if teile.len() == 1
                && teile[0]
                    .strip_suffix('%')
                    .and_then(|z| z.parse::<f32>().ok())
                    .is_some_and(|p| p >= 50.0)
            {
                return true;
            }
        } else if name == "clip"
            && matches!(b.position, crate::Position::Absolut | crate::Position::Fest)
        {
            let Some(innen) = w.strip_prefix("rect(").and_then(|r| r.strip_suffix(')')) else {
                continue;
            };
            // Release 11.10.2026 (Agent CSS2): alle absoluten Einheiten und
            // em/ex (CSS 2.1 §11.1.2). GEMESSEN vorher (WPT `visufx/clip-*`,
            // 36 Dateien): nur `px` wurde gelesen, `rect(0pt, 0pt, 0pt, 0pt)`
            // schnitt nichts weg, die rote Flaeche blieb sichtbar.
            let em = b.schriftgroesse_px;
            let z: Vec<f32> = innen
                .split([',', ' '])
                .filter(|t| !t.is_empty())
                .filter_map(|t| clip_laenge(t, em))
                .collect();
            if z.len() == 4 && (z[1] - z[3] <= 0.0 || z[2] - z[0] <= 0.0) {
                return true;
            }
        }
    }
    false
}

/// Release 11.10.2026 (Agent CSS2): eine Laenge in `clip: rect(...)` in px.
fn clip_laenge(t: &str, em: f32) -> Option<f32> {
    let t = t.trim();
    let einheiten: [(&str, f32); 9] = [
        ("px", 1.0),
        ("pt", 4.0 / 3.0),
        ("pc", 16.0),
        ("in", 96.0),
        ("cm", 96.0 / 2.54),
        ("mm", 96.0 / 25.4),
        ("em", em),
        ("ex", em * 0.5),
        ("q", 96.0 / 101.6),
    ];
    for (e, f) in einheiten {
        if let Some(zahl) = t.strip_suffix(e) {
            return zahl.parse::<f32>().ok().map(|v| v * f);
        }
    }
    t.parse::<f32>().ok()
}

/// Release 08.10.2026: traefen die Media-Bedingungen der Gruppen, in
/// denen die Regel verschachtelt stand (`@layer a { @media (x) { ... } }`)?
fn zusatz_trifft(
    s: &super::typen::StilRegel,
    ansicht_breite: u32,
    ansicht_hoehe: u32,
    zustand: &Zustand,
) -> bool {
    s.zusatz_medien
        .iter()
        .all(|q| q.trifft_mit_schema(ansicht_breite, ansicht_hoehe, zustand.farbschema))
}

/// Release 08.10.2026: die Variablen-Gebote eines Elements, mit Index ab
/// [`VAR_INDEX_AB`] Eintraegen. GEMESSEN vorher: lineare Suche ueber
/// hoechstens 64 Namen — bei github.com (rund 2000 Variablen auf
/// `:root`) waere das ohne Deckel quadratisch geworden.
#[derive(Default)]
struct VarGebote {
    liste: Vec<(String, Gebot)>,
    index: std::collections::HashMap<String, usize>,
}

/// Ab so vielen Variablen-Geboten sucht `VarGebote` ueber den Index.
const VAR_INDEX_AB: usize = 24;

impl VarGebote {
    fn merke(
        &mut self,
        name: &str,
        rang: Rang,
        schicht: u16,
        spez: (u32, u32, u32),
        reihe: u64,
        quelle: Quelle,
    ) {
        let neu = Gebot {
            rang,
            schicht,
            spez,
            reihe,
            quelle,
        };
        let stelle = if self.liste.len() >= VAR_INDEX_AB {
            if self.index.is_empty() {
                for (i, (n, _)) in self.liste.iter().enumerate() {
                    self.index.insert(n.clone(), i);
                }
            }
            self.index.get(name).copied()
        } else {
            self.liste.iter().position(|(n, _)| n == name)
        };
        match stelle {
            Some(i) => gebot_ersetzen(&mut self.liste[i].1, neu),
            None => {
                if self.liste.len() < MAX_VAR_EINTRAEGE {
                    if !self.index.is_empty() {
                        self.index.insert(String::from(name), self.liste.len());
                    }
                    self.liste.push((String::from(name), neu));
                }
            }
        }
    }
}

/// Ersetzt `alt` durch `neu`, wenn `neu` die Kaskade gewinnt (Rang,
/// Schicht, dann Spezifitaet und Reihe) — dieselbe Regel wie `merke_var`.
fn gebot_ersetzen(alt: &mut Gebot, neu: Gebot) {
    if neu.rang > alt.rang {
        *alt = neu;
    } else if neu.rang == alt.rang {
        if schicht_gewinnt_oder(neu.rang, neu.schicht, alt.schicht) {
            *alt = neu;
        } else if neu.schicht == alt.schicht && (neu.spez, neu.reihe) > (alt.spez, alt.reihe) {
            *alt = neu;
        }
    }
}

/// Merkt ein Variablen-Gebot (Name, Rang, Spezifitaet, Reihe).
/// Neue Namen nur bis MAX_VAR_EINTRAEGE (M4) — der Rest faellt weg.
fn merke_var(
    gebote: &mut Vec<(String, Gebot)>,
    name: &str,
    rang: Rang,
    schicht: u16,
    spez: (u32, u32, u32),
    reihe: u64,
    quelle: Quelle,
) {
    let neu = Gebot {
        rang,
        schicht,
        spez,
        reihe,
        quelle,
    };
    for (n, alt) in gebote.iter_mut() {
        if n == name {
            if rang > alt.rang {
                *alt = neu;
            } else if rang == alt.rang {
                let schicht_gewinnt = schicht_gewinnt_oder(rang, schicht, alt.schicht);
                if schicht_gewinnt {
                    *alt = neu;
                } else if schicht == alt.schicht {
                    if (spez, reihe) > (alt.spez, alt.reihe) {
                        *alt = neu;
                    }
                }
            }
            return;
        }
    }
    if gebote.len() < MAX_VAR_EINTRAEGE {
        gebote.push((String::from(name), neu));
    }
}

/// Vergleicht zwei Kaskaden-Schichten fuer die Frage: gewinnt die NEUE?
///
/// # DIE DREI FAELLE
///
/// `schicht == u16::MAX` heisst "nicht in einer Schicht" (unlayered).
///
/// 1. **normal** (`rang < 3`): die spaetere Schicht gewinnt, und
///    ungeschichtet heisst "die allerletzte" — also gewinnt sie.
///    Genau das gab `schicht > alt.schicht` mit `u16::MAX`, und das
///    bleibt so.
/// 2. **wichtig** (`rang >= 3`): die Reihenfolge ist UMGEKEHRT, die
///    fruehere Schicht gewinnt.
/// 3. **wichtig und ungeschichtet**: eine wichtige Deklaration
///    ausserhalb jeder Schicht schlaegt JEDE geschichtete wichtige.
///
/// Zu Fall 3 stand `schicht < alt.schicht`. Mit `schicht = u16::MAX`
/// ist `MAX < 0` falsch — die ungeschichtete wichtige Deklaration
/// VERLOR also gegen jede geschichtete. Das ist die Umkehrung von Fall 1.
///
/// GEMESSENER FEHLER 30.09.2026 (R0 d), Test
/// `important_aus_einer_schicht_gewinnt_gegen_important_ohne_schicht`:
///
/// ```text
///   @layer eins { p { color: #ff0000 !important; } }
///   p           { color: #00ff00 !important; }   ->  war ff0000
/// ```
///
/// Die Referenz ist CSS Cascade 5, 6.4.2 ("Cascade Layers"): unlayered
/// styles werden als eine implizite LETZTE Schicht behandelt, und fuer
/// wichtige Deklarationen ist die Schichtenreihenfolge umgekehrt — die
/// implizite letzte Schicht ist damit die ERSTE und gewinnt. MDN sagt
/// dasselbe woertlich: "Important declarations outside of any layer take
/// precedence over all layered important declarations."
fn schicht_gewinnt_oder(rang: Rang, schicht: u16, alt_schicht: u16) -> bool {
    const UNGESCHICHTET: u16 = u16::MAX;
    if rang.0 >= 3 {
        // Wichtig: Reihenfolge umgekehrt, ungeschichtet gewinnt immer.
        if schicht == UNGESCHICHTET {
            return true;
        }
        if alt_schicht == UNGESCHICHTET {
            return false;
        }
        schicht < alt_schicht
    } else {
        // Normal: spaetere Schicht gewinnt, ungeschichtet ist die letzte.
        schicht > alt_schicht
    }
}

/// Rohwert einer Gewinn-Variablen (oder None bei kaputter Quelle —
/// duerfte nicht passieren, defensiv trotzdem ohne Panic).
fn var_roh<'a>(
    quelle: Quelle,
    blaetter: &'a [Stylesheet],
    inline_vars: &'a [VariablenDekl],
) -> Option<&'a str> {
    match quelle {
        Quelle::Var {
            blatt,
            regel,
            innen,
            dekl,
        } => {
            let b = blaetter.get(blatt)?;
            let r = b.regeln.get(regel)?;
            // Release 08.10.2026: auch `@supports` und `@layer` —
            // GEMESSEN vorher: jede Variable aus einer Schicht fiel weg
            // (Primer Brand auf github.com setzt ihre Farben in
            // `@layer primer-brand { :root { ... } }`).
            let s = match (r, innen) {
                (Regel::Stil(x), None) => x,
                (Regel::Media { regeln, .. }, Some(ii))
                | (Regel::Supports { regeln, .. }, Some(ii))
                | (Regel::Layer { regeln, .. }, Some(ii)) => regeln.get(ii)?,
                _ => return None,
            };
            s.variablen.get(dekl).map(|v| v.roh.as_str())
        }
        Quelle::InlineVar { dekl } => inline_vars.get(dekl).map(|v| v.roh.as_str()),
        Quelle::Blatt { .. } | Quelle::Inline { .. } => None,
    }
}

/// Merkt das Gebot, wenn es das bisher beste schlaegt (Rang, dann
/// Spezifitaet, dann Reihe — spaeter gewinnt bei Gleichstand).
fn merke(
    gebote: &mut [[Option<Gebot>; 6]; EIGENSCHAFTEN],
    e: Eigenschaft,
    rang: Rang,
    schicht: u16,
    spez: (u32, u32, u32),
    reihe: u64,
    quelle: Quelle,
) {
    profil(pz::GEBOTE);
    let idx = eigenschaft_index(e);
    let r = rang.0 as usize;
    let neu = Gebot {
        rang,
        schicht,
        spez,
        reihe,
        quelle,
    };
    match &gebote[idx][r] {
        None => gebote[idx][r] = Some(neu),
        Some(alt) => {
            let schicht_gewinnt = schicht_gewinnt_oder(rang, schicht, alt.schicht);
            if schicht_gewinnt {
                gebote[idx][r] = Some(neu);
            } else if schicht == alt.schicht {
                if (spez, reihe) > (alt.spez, alt.reihe) {
                    gebote[idx][r] = Some(neu);
                }
            }
        }
    }
}

// ===========================================================================
// EIGENSCHAFTS-INDEX (56 Faecher: W5 +5, W6 +3, W6D +9, K1 +1)
// ===========================================================================

/// Alle Langformen in fester Reihenfolge (fuer die Aufloeseschleife).
pub(crate) fn alle_eigenschaften() -> [Eigenschaft; EIGENSCHAFTEN] {
    [
        Eigenschaft::Anzeige,
        Eigenschaft::Farbe,
        Eigenschaft::Hintergrund,
        Eigenschaft::Schriftgroesse,
        Eigenschaft::Schriftgewicht,
        Eigenschaft::Schriftstil,
        Eigenschaft::Schriftfamilie,
        Eigenschaft::Zeilenhoehe,
        Eigenschaft::Textausrichtung,
        Eigenschaft::Textdekoration,
        Eigenschaft::Weissraum,
        Eigenschaft::MargeOben,
        Eigenschaft::MargeRechts,
        Eigenschaft::MargeUnten,
        Eigenschaft::MargeLinks,
        Eigenschaft::FuellungOben,
        Eigenschaft::FuellungRechts,
        Eigenschaft::FuellungUnten,
        Eigenschaft::FuellungLinks,
        Eigenschaft::RahmenObenBreite,
        Eigenschaft::RahmenRechtsBreite,
        Eigenschaft::RahmenUntenBreite,
        Eigenschaft::RahmenLinksBreite,
        Eigenschaft::RahmenObenStil,
        Eigenschaft::RahmenRechtsStil,
        Eigenschaft::RahmenUntenStil,
        Eigenschaft::RahmenLinksStil,
        Eigenschaft::RahmenObenFarbe,
        Eigenschaft::RahmenRechtsFarbe,
        Eigenschaft::RahmenUntenFarbe,
        Eigenschaft::RahmenLinksFarbe,
        Eigenschaft::Breite,
        Eigenschaft::MaxBreite,
        Eigenschaft::MinBreite,
        Eigenschaft::Hoehe,
        // K1, 01.10.2026: min/max-height (siehe typen.rs).
        Eigenschaft::MinHoehe,
        Eigenschaft::MaxHoehe,
        Eigenschaft::Listenstil,
        Eigenschaft::Sichtbarkeit,
        Eigenschaft::Ueberlauf,
        Eigenschaft::FlexRichtung,
        Eigenschaft::FlexBuendig,
        Eigenschaft::FlexAusrichtung,
        Eigenschaft::Luecke,
        Eigenschaft::FlexWachstum,
        Eigenschaft::FlexSchrumpf,
        Eigenschaft::FlexBasis,
        Eigenschaft::FlexUmbruch,
        Eigenschaft::Position,
        Eigenschaft::Oben,
        Eigenschaft::Rechts,
        Eigenschaft::Unten,
        Eigenschaft::Links,
        Eigenschaft::ZIndex,
        Eigenschaft::Schweben,
        Eigenschaft::Raeumung,
        Eigenschaft::RasterSpalten,
        // L4 (18:2x): `grid-template-rows` (CSS Grid 1 §8.3).
        Eigenschaft::RasterZeilen,
        // L4 (21:5x): `grid-auto-rows` (§7.5) und `grid-auto-columns`
        // (§7.3). GEMESSENER BEFUND: alle drei gingen auf EIN Feld
        // (`gitter_automatik`), und der Kommentar vom 11:3x hat das
        // selbst geschrieben — „die drei ueberschreiben sich".
        Eigenschaft::RasterAutoZeilen,
        Eigenschaft::RasterAutoSpalten,
        Eigenschaft::HintergrundBild,
        // K6 (01.10.2026): die vier Ecken von `border-radius`,
        // opacity, box-shadow, outline und vertical-align — die
        // haeufigsten Luecken auf echten Seiten (GEMESSEN,
        // examples/abdeckung).
        Eigenschaft::RadiusObenLinks,
        Eigenschaft::RadiusObenRechts,
        // Die Reihenfolge in `Berechnet::radius` ist
        // oben-links, oben-rechts, UNTEN-RECHTS, unten-links.
        Eigenschaft::RadiusUntenRechts,
        Eigenschaft::RadiusUntenLinks,
        Eigenschaft::Deckkraft,
        Eigenschaft::Schatten,
        Eigenschaft::OutlineBreite,
        Eigenschaft::OutlineFarbe,
        Eigenschaft::OutlineOffset,
        // `VertikalAlign` ist der LETZTE in dieser Liste und hat
        // darum die letzte Nummer. Die Liste ist die Wahrheit —
        // `index_tabelle_ist_eindeutig` prueft das mit.
        Eigenschaft::VertikalAlign,
        // K7 (01.10.2026): box-sizing, `width: max-content` und die
        // Masken sowie die Hintergrund-Langformen.
        Eigenschaft::BoxArt,
        Eigenschaft::BreiteArt,
        Eigenschaft::MaskenBild,
        Eigenschaft::MaskenGroesse,
        Eigenschaft::MaskenPosition,
        Eigenschaft::MaskenWiederholen,
        Eigenschaft::BildGroesse,
        Eigenschaft::BildPosition,
        Eigenschaft::BildWiederholen,
        // ---- K7c (01.10.2026): die 27 neuen Eigenschaften ----
        //
        // Sie stehen HINTEN, damit die vorhandenen Nummern
        // unveraendert bleiben. `index_tabelle_ist_eindeutig` prueft
        // die Zuordnung mit.
        Eigenschaft::LetterSpacing,
        Eigenschaft::Wortabstand,
        Eigenschaft::TextEinzug,
        Eigenschaft::Tabulator,
        Eigenschaft::TextTransform,
        Eigenschaft::TextUeberlauf,
        Eigenschaft::WortUmbruch,
        Eigenschaft::WortTrennung,
        Eigenschaft::ZeilenTrennung,
        Eigenschaft::GitterSpalteStart,
        Eigenschaft::GitterSpalteEnde,
        Eigenschaft::GitterZeileStart,
        Eigenschaft::GitterZeileEnde,
        Eigenschaft::GitterGebiete,
        // L4 (02:9x): `grid-area` (§8.4).
        Eigenschaft::GitterGebiet,
        Eigenschaft::GitterVorlagen,
        Eigenschaft::GitterAutomatik,
        Eigenschaft::Ordnung,
        Eigenschaft::LueckeSpalte,
        Eigenschaft::LueckeZeile,
        Eigenschaft::OverflowX,
        Eigenschaft::OverflowY,
        Eigenschaft::ObjektPassung,
        Eigenschaft::Seitenverhaeltnis,
        Eigenschaft::TabellenKanten,
        Eigenschaft::TabellenLage,
        Eigenschaft::Schluessel,
        // ---- K17b: die vier logischen Innenspitzen ----
        //
        // Sie stehen HINTEN, damit alle 103 vorhandenen Nummern
        // unveraendert bleiben. `index_tabelle_ist_eindeutig` prueft
        // die Zuordnung mit.
        Eigenschaft::MargeInnenStart,
        Eigenschaft::MargeInnenEnde,
        Eigenschaft::FuellungInnenStart,
        Eigenschaft::FuellungInnenEnde,
        // ---- L4 (09:5x): `align-self` und `justify-self` (§6.6) ----
        //
        // Sie stehen HINTEN, damit alle 111 vorhandenen Nummern
        // unveraendert bleiben — dieselbe Form wie der K17b-Block
        // darueber. `index_tabelle_ist_eindeutig` prueft die Zuordnung.
        Eigenschaft::AusrichtungSelbst,
        Eigenschaft::AusrichtungSelbstWaagerecht,
        // ---- L4 (11:5x): `justify-items` (§6.2) ----
        //
        // Wieder HINTEN, damit alle 112 vorhandenen Nummern
        // unveraendert bleiben — dieselbe Form wie der 09:5x-Block
        // darueber. `index_tabelle_ist_eindeutig` prueft die Zuordnung.
        Eigenschaft::JustifyAusrichtung,
        // ---- 03.10.2026 (Rolle CSS): `border-spacing` (§17.6) ----
        //
        // Wieder HINTEN, damit alle 113 vorhandenen Nummern unveraendert
        // bleiben. Siehe `Eigenschaft::RandAbstandWaagerecht`.
        Eigenschaft::RandAbstandWaagerecht,
        Eigenschaft::RandAbstandSenkrecht,
        // ---- 07.10.2026 (K7c): `background-blend-mode` (CSS Compositing 1 §3.1) ----
        //
        // GEMESSENER ANLASS: die Eigenschaft stand im Parser als `ESchluessel`
        // — der Wert ging verloren, und die Ebenen wurden immer mit
        // `normal` gemischt. Der Korpus enthaelt 1x `background-blend-mode`
        // (35 echte Stylesheets).
        //
        // Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
        // Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
        // Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
        // ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
        Eigenschaft::HintergrundMischung,
    ]
}

/// Index je Eigenschaft (0-37). Vollstaendig per match — der Compiler
/// meldet jede neue Variante, die hier fehlt (kein stiller Fehler).
fn eigenschaft_index(e: Eigenschaft) -> usize {
    match e {
        Eigenschaft::Anzeige => 0,
        Eigenschaft::Farbe => 1,
        Eigenschaft::Hintergrund => 2,
        Eigenschaft::Schriftgroesse => 3,
        Eigenschaft::Schriftgewicht => 4,
        Eigenschaft::Schriftstil => 5,
        Eigenschaft::Schriftfamilie => 6,
        Eigenschaft::Zeilenhoehe => 7,
        Eigenschaft::Textausrichtung => 8,
        Eigenschaft::Textdekoration => 9,
        Eigenschaft::Weissraum => 10,
        Eigenschaft::MargeOben => 11,
        Eigenschaft::MargeRechts => 12,
        Eigenschaft::MargeUnten => 13,
        Eigenschaft::MargeLinks => 14,
        Eigenschaft::FuellungOben => 15,
        Eigenschaft::FuellungRechts => 16,
        Eigenschaft::FuellungUnten => 17,
        Eigenschaft::FuellungLinks => 18,
        Eigenschaft::RahmenObenBreite => 19,
        Eigenschaft::RahmenRechtsBreite => 20,
        Eigenschaft::RahmenUntenBreite => 21,
        Eigenschaft::RahmenLinksBreite => 22,
        Eigenschaft::RahmenObenStil => 23,
        Eigenschaft::RahmenRechtsStil => 24,
        Eigenschaft::RahmenUntenStil => 25,
        Eigenschaft::RahmenLinksStil => 26,
        Eigenschaft::RahmenObenFarbe => 27,
        Eigenschaft::RahmenRechtsFarbe => 28,
        Eigenschaft::RahmenUntenFarbe => 29,
        Eigenschaft::RahmenLinksFarbe => 30,
        Eigenschaft::Breite => 31,
        Eigenschaft::MaxBreite => 32,
        Eigenschaft::MinBreite => 33,
        Eigenschaft::Hoehe => 34,
        Eigenschaft::MinHoehe => 35,
        Eigenschaft::MaxHoehe => 36,
        Eigenschaft::Listenstil => 37,
        Eigenschaft::Sichtbarkeit => 38,
        Eigenschaft::Ueberlauf => 39,
        Eigenschaft::FlexRichtung => 40,
        Eigenschaft::FlexBuendig => 41,
        Eigenschaft::FlexAusrichtung => 42,
        Eigenschaft::Luecke => 43,
        Eigenschaft::FlexWachstum => 44,
        Eigenschaft::FlexSchrumpf => 45,
        Eigenschaft::FlexBasis => 46,
        Eigenschaft::FlexUmbruch => 47,
        Eigenschaft::Position => 48,
        Eigenschaft::Oben => 49,
        Eigenschaft::Rechts => 50,
        Eigenschaft::Unten => 51,
        Eigenschaft::Links => 52,
        Eigenschaft::ZIndex => 53,
        Eigenschaft::Schweben => 54,
        Eigenschaft::Raeumung => 55,
        Eigenschaft::RasterSpalten => 56,
        // L4 (18:2x): `grid-template-rows` (CSS Grid 1 §8.3). Steht in
        // `alle_eigenschaften()` direkt hinter den Spalten, also auf
        // Position 57.
        //
        // GEMESSENE ENTSCHEIDUNG, warum alle Nummern ab 57 um eins
        // verschoben wurden und keine eigene Zahl vergeben wurde: der
        // Test `index_tabelle_ist_eindeutig` verlangt, dass die Zahl der
        // LISTENPOSITION entspricht. Eine eigene Zahl waere eine
        // handgepflegte Wahrheit neben einer zweiten — und GEMESSEN hat
        // der Test genau das gemeldet: „RasterZeilen ist auf Index 80
        // abgebildet, steht aber an Position 57".
        Eigenschaft::RasterZeilen => 57,
        // L4 (02.10.2026 21:5x): `grid-auto-rows` und
        // `grid-auto-columns`. Stehen in `alle_eigenschaften()` direkt
        // hinter den Vorlagen, also auf Position 58 und 59 — und wie
        // bei `RasterZeilen` (18:2x) verschieben sich alle Nummern
        // dahinter um zwei, weil `index_tabelle_ist_eindeutig` die Zahl
        // an die LISTENPOSITION bindet.
        Eigenschaft::RasterAutoZeilen => 58,
        Eigenschaft::RasterAutoSpalten => 59,
        // 07.10.2026 (K7c): `background-blend-mode` (CSS Compositing 1 §3.1).
        // GEMESSENER ANLASS: die Eigenschaft stand im Parser als `ESchluessel`
        // — der Wert ging verloren, und die Ebenen wurden immer mit
        // `normal` gemischt. Der Korpus enthaelt 1x `background-blend-mode`
        // (35 echte Stylesheets).
        //
        // Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
        // Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
        // Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
        // ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
        Eigenschaft::HintergrundBild => 60,
        Eigenschaft::RadiusObenLinks => 61,
        Eigenschaft::RadiusObenRechts => 62,
        Eigenschaft::RadiusUntenRechts => 63,
        Eigenschaft::RadiusUntenLinks => 64,
        Eigenschaft::Deckkraft => 65,
        Eigenschaft::Schatten => 66,
        Eigenschaft::OutlineBreite => 67,
        Eigenschaft::OutlineFarbe => 68,
        Eigenschaft::OutlineOffset => 69,
        Eigenschaft::VertikalAlign => 70,
        Eigenschaft::BoxArt => 71,
        Eigenschaft::BreiteArt => 72,
        Eigenschaft::MaskenBild => 73,
        Eigenschaft::MaskenGroesse => 74,
        Eigenschaft::MaskenPosition => 75,
        Eigenschaft::MaskenWiederholen => 76,
        Eigenschaft::BildGroesse => 77,
        Eigenschaft::BildPosition => 78,
        Eigenschaft::BildWiederholen => 79,
        Eigenschaft::LetterSpacing => 80,
        Eigenschaft::Wortabstand => 81,
        Eigenschaft::TextEinzug => 82,
        Eigenschaft::Tabulator => 83,
        Eigenschaft::TextTransform => 84,
        Eigenschaft::TextUeberlauf => 85,
        Eigenschaft::WortUmbruch => 86,
        Eigenschaft::WortTrennung => 87,
        Eigenschaft::ZeilenTrennung => 88,
        Eigenschaft::GitterSpalteStart => 89,
        Eigenschaft::GitterSpalteEnde => 90,
        Eigenschaft::GitterZeileStart => 91,
        Eigenschaft::GitterZeileEnde => 92,
        Eigenschaft::GitterGebiete => 93,
        // L4 (02:9x): `grid-area`. GEMESSEN, warum 94 und nicht eine
        // eigene Zahl: `index_tabelle_ist_eindeutig` bindet die Prioritaet
        // an die LISTENPOSITION, und die ist hier 94 — die Eigenschaft steht
        // in `alle_eigenschaften()` direkt hinter `GitterGebiete`.
        Eigenschaft::GitterGebiet => 94,
        Eigenschaft::GitterVorlagen => 95,
        Eigenschaft::GitterAutomatik => 96,
        Eigenschaft::Ordnung => 97,
        Eigenschaft::LueckeSpalte => 98,
        Eigenschaft::LueckeZeile => 99,
        Eigenschaft::OverflowX => 100,
        Eigenschaft::OverflowY => 101,
        Eigenschaft::ObjektPassung => 102,
        Eigenschaft::Seitenverhaeltnis => 103,
        Eigenschaft::TabellenKanten => 104,
        Eigenschaft::TabellenLage => 105,
        Eigenschaft::Schluessel => 106,
        // K17b: die Nummern 103..106. Sie muessen der POSITION in
        // `alle_eigenschaften` entsprechen; der Test
        // `index_tabelle_ist_eindeutig` sagt das mit.
        Eigenschaft::MargeInnenStart => 107,
        Eigenschaft::MargeInnenEnde => 108,
        Eigenschaft::FuellungInnenStart => 109,
        Eigenschaft::FuellungInnenEnde => 110,
        // L4 (09:5x): 111 und 112 — die LISTENPOSITION, wie `index_
        // tabelle_ist_eindeutig` verlangt. `EIGENSCHAFTEN` ist 113.
        // L4 (11:5x): 111 und 112 — die LISTENPOSITION (siehe
        // `alle_eigenschaften`).
        Eigenschaft::AusrichtungSelbst => 111,
        Eigenschaft::AusrichtungSelbstWaagerecht => 112,
        // L4 (11:5x): die LISTENPOSITION, 113.
        Eigenschaft::JustifyAusrichtung => 113,
        // 03.10.2026 (Rolle CSS): 114 und 115, die LISTENPOSITION.
        Eigenschaft::RandAbstandWaagerecht => 114,
        Eigenschaft::RandAbstandSenkrecht => 115,
        // 07.10.2026 (K7c): `background-blend-mode` (CSS Compositing 1 §3.1).
        // GEMESSENER ANLASS: die Eigenschaft stand im Parser als `ESchluessel`
        // — der Wert ging verloren, und die Ebenen wurden immer mit
        // `normal` gemischt. Der Korpus enthaelt 1x `background-blend-mode`
        // (35 echte Stylesheets).
        //
        // Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
        // Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
        // Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
        // ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
        Eigenschaft::HintergrundMischung => 116,
    }
}

// ===========================================================================
// AUFLOESUNG
// ===========================================================================

/// Hört die gewinnende Deklaration einer Eigenschaft (oder None).
/// Die Deklaration, auf die ein `Quelle` zeigt.
///
/// K17: aus `gewinner` herausgezogen, weil der generische Eimer
/// (`schluessel_gebote`) dieselbe Aufloesung braucht und sonst eine
/// zweite Kopie bekame. Verhalten unveraendert — dieselbe `match`-Arme.
fn dekl_an<'a>(
    quelle: Quelle,
    blaetter: &'a [Stylesheet],
    inline: &'a [Deklaration],
) -> Option<&'a Deklaration> {
    match quelle {
        Quelle::Var { .. } | Quelle::InlineVar { .. } => None,
        Quelle::Inline { dekl } => inline.get(dekl),
        Quelle::Blatt {
            blatt,
            regel,
            innen,
            dekl,
        } => {
            let b = blaetter.get(blatt);
            let rule = b.and_then(|b| b.regeln.get(regel));
            rule.and_then(|ru| match (ru, innen) {
                (Regel::Stil(s), None) => s.deklarationen.get(dekl),
                (Regel::Media { regeln, .. }, Some(ii)) => {
                    regeln.get(ii).and_then(|s| s.deklarationen.get(dekl))
                }
                (Regel::Supports { regeln, .. }, Some(ii)) => {
                    regeln.get(ii).and_then(|s| s.deklarationen.get(dekl))
                }
                (Regel::Layer { regeln, .. }, Some(ii)) => {
                    regeln.get(ii).and_then(|s| s.deklarationen.get(dekl))
                }
                _ => None,
            })
        }
    }
}

fn gewinner<'a>(
    e: Eigenschaft,
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &'a [Stylesheet],
    inline: &'a [Deklaration],
) -> Option<&'a Deklaration> {
    for r in (0..=5).rev() {
        if let Some(g) = &gebote[eigenschaft_index(e)][r] {
            let dekl = dekl_an(g.quelle, blaetter, inline);
            if let Some(d) = dekl {
                if d.wert == Wert::Zurueck {
                    // Revert rolls back to UA origin.
                    // If we are evaluating Author (rang 1, 2, 3, 4), we skip to UA (rang 0).
                    // If we are at UA, well, UA revert is theoretically initial, but normally doesn't happen.
                    if r > 0 && r < 5 {
                        // Author origin
                        if let Some(_ua_g) = &gebote[eigenschaft_index(e)][0] {
                            // Recursively return UA declaration or loop continues
                            // We can just set the loop to check r=0 next.
                            continue;
                        } else {
                            return None; // No UA rule, so it reverts to initial/inherit.
                        }
                    }
                    return None; // Fallback
                }
                return Some(d);
            }
        }
    }
    None
}

/// Laenge in px aufloesen. `em_basis` ist die eigene (bzw. bei
/// font-size die Eltern-)Schrift, `prozent` die %-Basis (Ansicht oder
/// eigene Schrift). Ergebnis auf +-MAX_PX gekappt (M4).
/// Rechnet eine Laenge in Pixel.
///
/// # DIE VIEWPORT-EINHEITEN (R0 e, 30.09.2026)
///
/// `vw` = 1 % der AnsichtsBREITE, `vh` = 1 % der AnsichtsHOEHE,
/// `vmin`/`vmax` = das Kleinere/Groessere von beidem (CSS Values 3, 5.4).
///
/// GEMESSENER FEHLER: hier stand `Vw | Vh => wert / 100 * ansicht` und
/// `Vmin`/`Vmax` genauso. Damit war `10vh` bei 800x600 genauso breit wie
/// `10vw` — 80 px statt 60 px. `berechnen` bekommt die Ansichtshoehe
/// seit G3, sie war nur nicht weitergereicht.
///
/// `ansicht_hoehe == 0` heisst "unbekannt"; dann faellt `vh` auf die
/// Breite zurueck, damit keine Zahl durch nichts geteilt wird.
fn laenge_px(
    l: &Laenge,
    em_basis: f32,
    rem_basis: f32,
    prozent: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
) -> f32 {
    use super::typen::Einheit::{Ch, Em, Prozent, Pt, Px, Rem, Vh, Vmax, Vmin, Vw, Zahl};
    let hoehe = if ansicht_hoehe.is_finite() && ansicht_hoehe > 0.0 {
        ansicht_hoehe
    } else {
        ansicht
    };
    let v = match l.einheit {
        Px => l.wert,
        Pt => l.wert * 4.0 / 3.0,
        Em => l.wert * em_basis,
        // CSS Values 3, 5.2: `rem` ist die Schriftgroesse des WURZEL-
        // Elements.
        //
        // 03.10.2026 15:0x: bis hier stand hier `BASIS_SCHRIFT_PX` (16),
        // mit der Notiz, die Kaskade kenne kein `html`.  Sie kennt es
        // doch: `Berechnet::wurzel_schrift` wandert aus dem
        // Eltern-`Berechnet` herab, und der Wurzelknoten setzt es auf
        // seine eigene Schrift.  GEMESSEN (`tests/position.rs::
        // l4_rem_nimmt_die_schrift_der_wurzel`): `html{font-size:75%}`
        // plus `1.5rem` war **24 px** statt 18.
        //
        // GEMESSENE REGEL: eine Konstante an der Stelle, wo die
        // Spezifikation eine Kette verlangt, ist eine Vermutung mit
        // Fehler — sie faellt erst auf, wenn eine Seite sie braucht.
        Rem => l.wert * rem_basis,
        Prozent => l.wert / 100.0 * prozent,
        Vw => l.wert / 100.0 * ansicht,
        Vh => l.wert / 100.0 * hoehe,
        Vmin => (l.wert / 100.0) * ansicht.min(hoehe),
        Vmax => (l.wert / 100.0) * ansicht.max(hoehe),
        Ch => l.wert * (em_basis * ch_anteil()),
        // CSS Values 4 §6.1.2: `d*` (mit Browserleisten) und `l*`
        // (ohne). Dieser Browser hat keine einstellbaren Leisten, also
        // sind beide Masszahlen gleich — eine DOKUMENTIERTE Naeherung
        // und keine Rechnung.
        // CSS Values 4 §6.1.2: `lh`/`rlh` sind die Zeilenhoehe. Die
        // kennt die Kaskade nicht (sie kommt aus dem Textfluss), also
        // wird `em` gerechnet — eine DOKUMENTIERTE Naeherung. GEMESSEN
        // 01.10.2026 (K7c): 1x `height: 1lh`.
        // Release 13.10.2026 (Agent CSS2): `lh` ist die berechnete
        // Zeilenhoehe des Elements (in `line-height` selbst die des Eltern),
        // sonst wie bisher `em`. GEMESSEN vorher (WPT css-overflow
        // `line-clamp-auto-*`: `font: 16px/32px; max-height: 4lh`): 64 statt
        // 128 px.
        // Release 14.10.2026 (Agent C): with the line height of the element
        // being computed when known (`line_height_set`); merged: C's value
        // first, then the CSS2 agent's line height, then `em`.
        super::typen::Einheit::Lh => {
            let z = zeile_px();
            let zeile = (z.is_finite() && z > 0.0).then_some(z);
            l.wert * line_height().0.or(zeile).unwrap_or(em_basis)
        }
        super::typen::Einheit::Rlh => l.wert * line_height().1.unwrap_or(em_basis),
        // `typen::` ist noetig: ohne den Pfad ist `Dvh` eine BIN-
        // DUNG (ein neues Muster mit gleichem Namen), nicht die
        // Enum-Variante — dieselbe Falle wie bei `BreiteArt` (K7) und
        // `SchriftGroesse`.
        super::typen::Einheit::Dvh => (l.wert / 100.0) * hoehe,
        super::typen::Einheit::Dvw => (l.wert / 100.0) * ansicht,
        super::typen::Einheit::Dvmin => (l.wert / 100.0) * ansicht.min(hoehe),
        super::typen::Einheit::Dvmax => (l.wert / 100.0) * ansicht.max(hoehe),
        super::typen::Einheit::Lvh => (l.wert / 100.0) * hoehe,
        super::typen::Einheit::Lvw => (l.wert / 100.0) * ansicht,
        super::typen::Einheit::Lvmin => (l.wert / 100.0) * ansicht.min(hoehe),
        super::typen::Einheit::Lvmax => (l.wert / 100.0) * ansicht.max(hoehe),
        super::typen::Einheit::Ex => l.wert * em_basis * ex_anteil(),
        Zahl => l.wert,
    };
    v.clamp(-MAX_PX, MAX_PX)
}

/// Release 11.10.2026 (Agent CSS2): `ex` ist die x-Hoehe der ersten
/// Schrift (CSS 2.1 §4.3.2). Die Kaskade kennt keine Schriftmetrik; die
/// einzige Schrift mit fester, bekannter x-Hoehe ist Ahem (0.8 em, jede
/// Glyphe ein Quadrat mit Oberlaenge 0.8 em). Sonst bleibt die Naeherung
/// 0.5 em. Der Anteil gilt fuer das Element, das gerade berechnet wird
/// (`berechnen_ernst` setzt ihn: fuer `font-size` den der Eltern, danach
/// den eigenen). GEMESSEN vorher (WPT `*-080/083/084`, `top-091` u. a.,
/// 91 Dateien mit `ex` und Ahem): `6ex` bei 20 px Ahem war 60 statt 96 px.
pub(crate) fn ex_anteil_fuer(familie: &str) -> f32 {
    schrift_anteile_fuer(familie).0
}

/// Release 12.10.2026: die ECHTEN Schriftmasse einer Familienliste —
/// `(x-Hoehe, Vorschub der „0")` in em. narsil-css liest keine Schriften;
/// der Seitenbau meldet den Haken an (siehe [`schrift_masse_anmelden`]).
/// GEMESSEN vorher (WPT css-text/i18n css3-text-line-break-*, 100 Dateien
/// mit `font-family: monospace; width: 50ch`): `ch` war fest 0,5 em — mit
/// Consolas (0,55 em) brach die Zeile an der falschen Stelle.
static SCHRIFT_MASSE: std::sync::OnceLock<fn(&str) -> Option<(f32, f32)>> = std::sync::OnceLock::new();

/// Den Haken fuer die Schriftmasse anmelden (einmal je Prozess).
pub fn schrift_masse_anmelden(f: fn(&str) -> Option<(f32, f32)>) {
    let _ = SCHRIFT_MASSE.set(f);
}

/// `(ex, ch)` in em fuer eine Familienliste: aus dem Haken, sonst die
/// Naeherung (Ahem: 0,8 / 1,0; sonst 0,5 / 0,5).
pub(crate) fn schrift_anteile_fuer(familie: &str) -> (f32, f32) {
    if let Some(f) = SCHRIFT_MASSE.get()
        && let Some((ex, ch)) = f(familie)
        && ex.is_finite()
        && ch.is_finite()
        && ex > 0.05
        && ch > 0.05
    {
        return (ex, ch);
    }
    let erste = familie.split(',').next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'');
    if erste.eq_ignore_ascii_case("ahem") { (0.8, 1.0) } else { (0.5, 0.5) }
}

/// Release 14.10.2026 (Agent CSS2): verbatim keys whose `inherit` takes the
/// parent's verbatim value (CSS Cascade 4 §7.2); the parser keeps their
/// name for the CSS-wide keyword.
pub(crate) const KEYS_WITH_INHERIT: &[&str] = &[
    "clip",
    "overflow-clip-margin",
    "background-clip",
    "-webkit-background-clip",
    "background-origin",
    "background-attachment",
];

thread_local! {
    static EX_ANTEIL: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.5, 0.5)) };
    /// Release 13.10.2026 (Agent CSS2): die Zeilenhoehe (px) des Elements,
    /// das gerade berechnet wird — fuer `lh` (CSS Values 4 §6.1.1).
    static ZEILE_PX: std::cell::Cell<f32> = const { std::cell::Cell::new(f32::NAN) };
}

/// Release 13.10.2026 (Agent CSS2): siehe `ZEILE_PX`.
fn zeile_px_setzen(px: f32) {
    ZEILE_PX.with(|c| c.set(px));
}

fn zeile_px() -> f32 {
    ZEILE_PX.with(|c| c.get())
}

fn ex_anteil() -> f32 {
    EX_ANTEIL.with(|c| c.get().0)
}

/// Release 12.10.2026: der `ex`-Anteil des Elements (fuer `calc()`).
pub(crate) fn ex_anteil_jetzt() -> f32 {
    ex_anteil()
}

/// Release 12.10.2026: der `ch`-Anteil des Elements, das gerade berechnet wird.
thread_local! {
    /// Release 14.10.2026 (Agent C): the line height for `lh` (the parent's
    /// while `font-size`/`line-height` are computed, then the own one) and
    /// for `rlh` (only known at the root itself); 0 = unknown.
    static LINE_HEIGHT: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
}

fn line_height_set(lh: f32, rlh: Option<f32>) {
    LINE_HEIGHT.with(|z| z.set((lh, rlh.unwrap_or(0.0))));
}

/// MEASURED before (WPT css-values lh-unit-005, rlh-on-root-lengths): `lh`
/// and `rlh` were `em` — `height: 1lh` under `line-height: 50px` gave 16px.
fn line_height() -> (Option<f32>, Option<f32>) {
    let (lh, rlh) = LINE_HEIGHT.with(std::cell::Cell::get);
    ((lh > 0.0).then_some(lh), (rlh > 0.0).then_some(rlh))
}

pub(crate) fn ch_anteil() -> f32 {
    EX_ANTEIL.with(|c| c.get().1)
}

fn ex_anteil_setzen(familie: &str) {
    EX_ANTEIL.with(|c| c.set(schrift_anteile_fuer(familie)));
}

/// font-size in px (Basis: Elternschrift; rem: 16; em/%: Eltern).
fn loese_schriftgroesse(
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    eltern: Option<&Berechnet>,
    eltern_schrift: f32,
    ansicht: f32,
    // Ansichts-HOEHE in px fuer `vh`/`vmin`/`vmax` (R0 e, 30.09.2026).
    ansicht_hoehe: f32,
    vars: &Variablen,
    erg: &mut Berechnet,
) {
    // 03.10.2026 15:0x: `rem` = Schrift der WURZEL (CSS Values 3 §5.2).
    let rem_basis = eltern.map_or(BASIS_SCHRIFT_PX, |e| e.wurzel_schrift);
    //
    // 03.10.2026 18:0x — DIE ERBUNG VOR DER FRUEHEN RUECKKEHR.
    //
    // GEMESSENER FEHLER (eigene Messung, `tests/k21_rem_calc.rs`): mit
    // `html { font-size: 12px }` ergab die Kette
    //
    //     html  schriftgroesse_px=12  wurzel_schrift=12   RICHTIG
    //     head  schriftgroesse_px=12  wurzel_schrift=16   FALSCH
    //     body  schriftgroesse_px=12  wurzel_schrift=16   FALSCH
    //     div   schriftgroesse_px=12  wurzel_schrift=16   FALSCH
    //
    // Grund: `erg.wurzel_schrift` wurde erst am ENDE dieser Funktion
    // gesetzt — nach `let d = match gewinner(Schriftgroesse, …) { None =>
    // return, … }`. Jeder Knoten **ohne** eigene `font-size`-Angabe kehrt
    // also vorher zurueck und behaelt den Vorgabewert 16 px. Da die meisten
    // Knoten keine eigene Schrift angeben, war `rem` auf jeder Seite mit
    // `html { font-size: … }` falsch, und `width: 2rem` ergab 32 px statt
    // 24 px.
    //
    // GEMESSENE EINORDNUNG, warum es bisher niemand gesehen hat: der
    // auffaellige Fall (CODEX-8, 02.10.) war `body { font-size: 1.5rem }` —
    // und **der** Knoten hat eine eigene Angabe, also kehrt er nicht zurueck
    // und rechnet richtig. Der Fehler steckt in den Knoten OHNE Angabe.
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier und nicht am Ende: die Erbung ist
    // die Voraussetzung fuer jede andere Auswertung in dieser Funktion, und
    // `return` ist keine Auswertung. Fuer einen Knoten MIT eigener Angabe
    // ist die spaetere Zuweisung dieselbe (sie nimmt wieder den Elternwert),
    // also aendert diese Zeile dort nichts.
    if let Some(e) = eltern {
        erg.wurzel_schrift = e.wurzel_schrift;
    }
    let d = match gewinner(Eigenschaft::Schriftgroesse, gebote, blaetter, inline) {
        None => return,
        Some(v) => v,
    };
    // em/% gegen die Elternschrift (CSS-Regel); var()/calc() zuerst (C10/C11).
    let ctx = RechenCtx {
        em: eltern_schrift,
        // 03.10.2026 17:5x: `rem` im Rechner gegen die Wurzelschrift —
        // dieselbe Basis, die `laenge_px` zwei Zeilen weiter unten nimmt.
        rem: rem_basis,
        prozent: eltern_schrift,
        ansicht,
        ansicht_hoehe: super::rechnen::ansichts_hoehe(ansicht, ansicht_hoehe),
    };
    let w = match effektiver_wert(Eigenschaft::Schriftgroesse, d, vars, &ctx) {
        None => return,
        Some(v) => v,
    };
    match &w {
        Wert::Erben => {
            erg.schriftgroesse_px = eltern
                .map(|p| p.schriftgroesse_px)
                .unwrap_or(BASIS_SCHRIFT_PX);
        }
        Wert::Anfang => erg.schriftgroesse_px = BASIS_SCHRIFT_PX,
        Wert::Unbestimmt => {
            // Vererbt -> wie Erben (siehe oben).
            erg.schriftgroesse_px = eltern
                .map(|p| p.schriftgroesse_px)
                .unwrap_or(BASIS_SCHRIFT_PX);
        }
        Wert::Laenge(l) => {
            use super::typen::Einheit::Zahl;
            if matches!(l.einheit, Zahl) {
                return; // einheitenlos ausser 0 ist hier ungueltig
            }
            erg.schriftgroesse_px = laenge_px(
                l,
                eltern_schrift,
                rem_basis,
                eltern_schrift,
                ansicht,
                ansicht_hoehe,
            )
            .clamp(0.0, MAX_PX);

        }
        Wert::Wort(w) => {
            let px = match w.as_str() {
                "xx-small" => 9.0,
                "x-small" => 10.0,
                "small" => 12.0,
                "medium" => BASIS_SCHRIFT_PX,
                "large" => 18.0,
                "x-large" => 24.0,
                "xx-large" => 32.0,
                "xxx-large" => 48.0,
                // Begruendung Faktoren: CSS 2.1 (1.2/0.83 als Naeherung).
                "larger" => eltern_schrift * 1.2,
                "smaller" => eltern_schrift * 0.83,
                _ => return,
            };
            erg.schriftgroesse_px = px.clamp(0.0, MAX_PX);

        }
        _ => {}
    }
    // 03.10.2026 15:0x: `rem` ist die Schrift der WURZEL (§5.2).  Das
    // WURZELelement selbst (kein Elternstil) setzt sie auf die eigene
    // Schrift, jedes Kind erbt sie.  GEMESSEN: `html{font-size:75%}`
    // ergibt 12 px, und `1.5rem` ist danach 18 px statt 24.
    erg.wurzel_schrift = match eltern {
        Some(e) => e.wurzel_schrift,
        None => erg.schriftgroesse_px.clamp(0.0, MAX_PX),
    };
}

/// GEMESSEN 30.09.2026 (R0 a): einmal lesen, dann nur noch ein Bool.
/// `std::env::var` ist nicht billig (es sucht den ganzen
/// Environment-Block), und `loese_farbe` laeuft je Element und
/// Eigenschaft — 50 000 Kaskaden sind 50 000 Suchen.
fn debug_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var("NARSIL_CSS_DEBUG").is_ok_and(|v| v != "0"))
}

/// color (Basis fuer currentcolor spaeter). Aktuell ohne Eltern: schwarz.
/// K17: die Schreibrichtung aus dem generischen Eimer.
///
/// `direction: rtl` landet als `Wert::Schluessel("direction", "rtl")` im
/// Eimer, nicht in einem eigenen Feld — es gab vor K17 kein Feld dafür.
/// Diese Funktion liest das stärkste Gebot und setzt `Berechnet::richtung`.
///
/// Sie läuft VOR der Eigenschaftsschleife, weil `text-align: start`
/// die Richtung braucht (CSS Writing Modes 3 §2.4) und der Eimer einen
/// spaeteren Index hat als `Textausrichtung`.
fn richtung_aus_gebote(
    gebote: &[(String, Gebot)],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    erg: &mut Berechnet,
) {
    for eintrag in gebote.iter() {
        let name = &eintrag.0;
        if name != "direction" {
            continue;
        }
        // `merke_var` laesst pro Name nur den Sieger stehen. Es gibt
        // also hoechstens EINEN Eintrag fuer `direction`, und die
        // Kaskadenregeln sind bereits angewandt.
        erg.richtung = richtung_aus_wort(roh_von_gebot(eintrag, blaetter, inline));
        return;
    }
}

/// `ltr` / `rtl` als Wort. Alles andere heisst „unbekannt" und laesst die
/// bisherige Richtung stehen — kein Raten, kein Zuruecksetzen.
fn richtung_aus_wort(roh: Option<&str>) -> SchreibRichtung {
    match roh {
        Some("ltr") => SchreibRichtung::Ltr,
        Some("rtl") => SchreibRichtung::Rtl,
        _ => SchreibRichtung::Ltr,
    }
}

/// Der Rohtext, auf den ein Gebot zeigt.
///
/// Fuer den generischen Eimer ist der WERT die Form
/// `Wert::Schluessel(name, roh)` — dort steht der eigentliche Text.
/// `dekl_an` allein wuerde nur den Container liefern.
fn roh_von_gebot<'a>(
    (name, gebot): &(String, Gebot),
    blaetter: &'a [Stylesheet],
    inline: &'a [Deklaration],
) -> Option<&'a str> {
    let d = dekl_an(gebot.quelle, blaetter, inline)?;
    match &d.wert {
        Wert::Schluessel(n, roh) if n == name => Some(roh.as_str()),
        _ => None,
    }
}

/// K17: den generischen Eimer eintragen — als Angabe UND aufgeloest.
///
/// # WAS GEMESSEN PASSIERT IST
///
/// Vorher: `.i{cursor:pointer;direction:ltr}` lieferte EINEN Eintrag
/// (`direction`), `cursor` war weg. Grund war ein gemeinsamer Slot für
/// alle ~190 Schluessel-Eigenschaften.
///
/// Nachher: beide. Der Eimer ist namensgeschluesselt (`schluessel_gebote`)
/// und laesst pro Name genau einen Sieger zu.
///
/// # ZWEITE HAElfte: LOGISCHE EIGENSCHAFTEN
///
/// `inset-inline-start` (CSS Logical Properties 1 §1) ist die Seite, an
/// der die Zeile beginnt — bei `rtl` also RECHTS. Solche Angaben haben
/// im Eimer gelegen und wurden nie auf ein Feld gerechnet.
///
/// GEMESSENE ABGRENZUNG, und sie ist wichtig: `margin-inline-start`
/// und `padding-inline-start` sind heute BEREITS aufgeloest
/// (`marge_links=2`, `fuellung_links=1` bei
/// `.i{padding-inline-start:1px;margin-inline-start:2px}`). Der Parser
/// gibt ihnen eigene Felder. Deshalb steht hier KEIN Marge und KEINE
/// Fuellung — sie waeren doppelt und wuerden eine bestehende, richtige
/// Loesung ueberschreiben.
///
/// `unicode-bidi` (8 Deklarationen im Korpus, alle `-moz-isolate`)
/// braucht ein eigenes Feld und bleibt deshalb im Eimer liegen.
fn schluessel_eintragen(
    gebote: &[(String, Gebot)],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    rem_basis: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
    erg: &mut Berechnet,
) {
    if gebote.is_empty() {
        return;
    }
    let em = erg.schriftgroesse_px;
    zeile_px_setzen(erg.zeilenhoehe_px);
    for eintrag in gebote.iter() {
        let name = &eintrag.0;
        let Some(roh) = roh_von_gebot(eintrag, blaetter, inline) else {
            continue;
        };
        // M4: je Name ein Platz. Die bisherige Grenze stand in der
        // Kaskade und zaehlte den Rest; hier bleibt sie beim Eintragen,
        // damit `erg.schluessel` die Grenze einhaelt.
        let bekannt = erg.schluessel.iter().any(|el| &el.0 == name);
        if !bekannt && erg.schluessel.len() >= crate::MAX_SCHLUESSEL {
            profil(pz::VERWORFEN);
            continue;
        }
        if name != "direction" {
            logisch_rechnen(name, roh, erg, em, rem_basis, ansicht, ansicht_hoehe);
        }
        // Release 11.10.2026: `align-content`/`place-content` fuer das Raster.
        crate::gitter::schluessel_setzen(name, roh, erg);
        tabellen_schluessel(name, roh, erg);
        ausrichtung_letzte_schluessel(name, roh, erg);
        upsert_schluessel(&mut erg.schluessel, name, roh);
    }
}

/// Release 13.10.2026 (Agent CSS2): `text-align-last` (CSS Text 3 §7.2) —
/// `start`/`end` nach der Schreibrichtung, `match-parent` wie `start`;
/// `inherit`/`unset` behalten den geerbten Wert.
fn ausrichtung_letzte_schluessel(name: &str, wert: &str, erg: &mut Berechnet) {
    // Release 13.10.2026 (Agent CSS2): `text-justify` (CSS Text 3 §7.4), nur
    // `none` schaltet ab.
    if name == "text-justify" {
        match wert.trim().to_ascii_lowercase().as_str() {
            "inherit" | "unset" => {}
            "none" => erg.blocksatz_aus = true,
            "auto" | "inter-word" | "inter-character" | "distribute" | "initial" => erg.blocksatz_aus = false,
            _ => {}
        }
        return;
    }
    if name != "text-align-last" {
        return;
    }
    let rechts_start = erg.richtung.start_ist_rechts();
    let (start, ende) = if rechts_start {
        (Textausrichtung::Rechts, Textausrichtung::Links)
    } else {
        (Textausrichtung::Links, Textausrichtung::Rechts)
    };
    erg.ausrichtung_letzte = match wert.trim().to_ascii_lowercase().as_str() {
        "inherit" | "unset" => return,
        "auto" | "initial" => None,
        "start" | "match-parent" => Some(start),
        "end" => Some(ende),
        "left" => Some(Textausrichtung::Links),
        "right" => Some(Textausrichtung::Rechts),
        "center" => Some(Textausrichtung::Zentriert),
        "justify" => Some(Textausrichtung::Blocksatz),
        _ => return,
    };
}

/// Release 11.10.2026 (Agent CSS2): `caption-side` und `empty-cells`
/// bekommen eigene (geerbte) Felder (CSS 2.1 §17.4.1, §17.6.1.1).
/// `inherit`/`unset` behalten den geerbten Wert.
fn tabellen_schluessel(name: &str, wert: &str, erg: &mut Berechnet) {
    let wort = wert.trim().to_ascii_lowercase();
    if matches!(wort.as_str(), "inherit" | "unset") {
        return;
    }
    if name == "caption-side" {
        erg.beschriftung_unten = wort == "bottom";
    } else if name == "empty-cells" {
        erg.leere_zellen_verbergen = wort == "hide";
    }
}

/// Die Seite bestimmen, auf die eine logische Angabe zeigt, und sie
/// rechnen.
///
/// GEMESSENE GRENZE: Prozent laeuft wie beim Rest der Kaskade gegen die
/// Ansichtsbreite (`breiten_wert` macht es so). Fuer `block-size` ist
/// die Ansichtshoehe die Basis — dort steht die Ansichtshoehe.
fn logisch_rechnen(
    name: &str,
    roh: &str,
    erg: &mut Berechnet,
    em: f32,
    rem_basis: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
) {
    let start_rechts = erg.richtung.start_ist_rechts();

    // GEMESSENE ABGRENZUNG: `inline-size` und `block-size` stehen hier
    // NICHT, weil der Parser sie bereits auf `Breite`/`Hoehe` abbildet
    // (`parse.rs`, `"inline-size" => Ziel::Lang(Breite)`). Sie kommen
    // also nie hier an. Sie wurden entfernt, statt sie als toten Zweig
    // mitzuschleppen.
    //
    // Die Prozentbasis ist wie beim Rest der Kaskade die Ansichtsbreite
    // (siehe `breiten_wert`).
    let betrag = |roh: &str| -> Option<f32> {
        let l = laenge_aus_text(roh)?;
        Some(laenge_px(&l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX))
    };

    match name {
        "inset-inline-start" => {
            let Some(w) = betrag(roh) else { return };
            if start_rechts {
                erg.rechts = Some(w);
            } else {
                erg.links = Some(w);
            }
        }
        "inset-inline-end" => {
            let Some(w) = betrag(roh) else { return };
            if start_rechts {
                erg.links = Some(w);
            } else {
                erg.rechts = Some(w);
            }
        }
        "inset-inline" => {
            // Release 11.10.2026 (Agent CSS2): auch zwei Werte (Anfang, Ende;
            // CSS Logical Properties 1 §4.3).
            let mut teile = roh.split_whitespace();
            let Some(a) = teile.next().and_then(&betrag) else { return };
            let e = teile.next().and_then(&betrag).unwrap_or(a);
            if start_rechts {
                erg.rechts = Some(a);
                erg.links = Some(e);
            } else {
                erg.links = Some(a);
                erg.rechts = Some(e);
            }
        }
        // Release 11.10.2026 (Agent CSS2): `inset-block` (horizontale
        // Schreibweise: oben, unten). GEMESSEN vorher (UA-Stil von
        // `dialog:modal { inset-block: 0 }`): ohne Wirkung, der Dialog stand
        // oben statt mittig.
        "inset-block" => {
            let mut teile = roh.split_whitespace();
            let Some(a) = teile.next().and_then(&betrag) else { return };
            let e = teile.next().and_then(&betrag).unwrap_or(a);
            erg.oben = Some(a);
            erg.unten = Some(e);
        }
        "unicode-bidi" => erg.bidi = bidi_aus_wort(roh),
        _ => {
            // Alles andere bleibt eine Angabe im Eimer — das ist die
            // dokumentierte Form fuer „kein eigenes Feld".
        }
    }
}

/// `unicode-bidi` als Wort.
///
/// GEMESSEN im Korpus: `embed=4, isolate=2, -webkit-isolate=1,
/// -moz-isolate=1`. Die beiden Praefix-Formen sind Legacy-Aliase fuer
/// `isolate` (CSS Writing Modes 3 §6.3) — sie werden deshalb genauso
/// gelesen und nicht als Unbekanntes verworfen.
///
/// Alles andere ist `Normal`: die Anfangswert. Ein unbekanntes Wort darf
/// den geerbten Wert nicht zerstoeren (CSS ist „invalid at computed-value
/// time" -> Initialwert, und hier ist der Initialwert genau das Richtige).
fn bidi_aus_wort(roh: &str) -> Bidi {
    match roh {
        "embed" => Bidi::Embed,
        "isolate" | "-webkit-isolate" | "-moz-isolate" => Bidi::Isolate,
        "isolate-override" => Bidi::IsolateOverride,
        "plaintext" => Bidi::Plaintext,
        "bidi-override" => Bidi::BidiOverride,
        _ => Bidi::Normal,
    }
}

/// Ein Laengenwert aus einem Text — fuer die logischen Eigenschaften, die
/// im Eimer als TEXT liegen.
///
/// GEMESSENE NOTWENDIGKEIT: `.i{inset-inline-start:5px}` kam als
/// `Wert::Schluessel("inset-inline-start", "5px")` an, also als String.
/// Ohne diesen Schritt muesste jede logische Angabe die Einheit selbst
/// erraten.
///
/// `var(...)` gibt bewusst `None`: das ist eine Verweise-Zyklo-Gefahr und
/// der Wert haengt vom Rechenkontext ab. Solche Angaben bleiben im Eimer
/// liegen und werden nicht geraten (M4).
fn laenge_aus_text(text: &str) -> Option<Laenge> {
    use super::typen::Einheit;
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let trenn = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(t.len());
    let (zahl, einheit) = t.split_at(trenn);
    let w: f32 = zahl.parse().ok()?;
    if !w.is_finite() {
        return None;
    }
    let e = match einheit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => Einheit::Px,
        "pt" => Einheit::Pt,
        "em" => Einheit::Em,
        "rem" => Einheit::Rem,
        "%" => Einheit::Prozent,
        "vw" | "dvw" | "lvw" | "svw" | "vi" | "svi" | "lvi" | "dvi" => Einheit::Vw,
        "vh" | "dvh" | "lvh" | "svh" | "vb" | "svb" | "lvb" | "dvb" => Einheit::Vh,
        "vmin" | "dvmin" | "lvmin" | "svmin" => Einheit::Vmin,
        "vmax" | "dvmax" | "lvmax" | "svmax" => Einheit::Vmax,
        "ch" => Einheit::Ch,
        _ => return None,
    };
    Some(Laenge {
        wert: w,
        einheit: e,
    })
}

/// HTML 5 §5.9 / WHATWG "presentational hints": `width`/`height` als
/// **Attribut** an einem ersetzten Element sind CSS-`width`/`height`.
///
/// # GEMESSENER FEHLER (03.10.2026, Rolle CSS)
///
/// `<img class="central-featured-logo" width="200" height="183">` auf
/// www.wikipedia.org ergab `breite = None, hoehe = None`. Der Motor nimmt
/// damit die **intrinsische** Groesse des Bildes — bei diesem Logo 380 px
/// statt 200 px. `team/VERGLEICH_FIREFOX.txt` nennt genau das als
/// "Bild in EigenGroesse statt CSS-Groesse" (2 von 10 Seiten: nixos.org,
/// www.wikipedia.org). Die Korrelation Bilder <-> Abweichung ist dort +0,82 —
/// die groesste aller Spalten, und dies ist eine ihrer Ursachen; sie liegt
/// in der Kaskade, nicht im Layout.
///
/// | Messform                        | `breite`    |
/// |---------------------------------|-------------|
/// | `<img width="200">`, **kein** CSS | **`None`**  |
/// | `img { width: 200px }`          | `Some(200)` |
///
/// Eine echte Autorenregel kommt in beiden Faellen durch. Was fehlte, war
/// ausschliesslich der Attribut-Weg — `Element::attribut()` wurde vorher nur
/// fuer Selektoren (`[width]`) und `lang` benutzt.
///
/// # WARUM NACHGEFUELLT UND NICHT GESETZT
///
/// `dir="rtl"` steht als Praesentationsangabe zwischen Vorgabe und Autor
/// (Zeile 1083 ff.). `width`/`height` aus einem Attribut stehen **unter**
/// allem: HTML 5 §5.9 stellt sie ganz an den Anfang des Autor-Urspungs, also
/// unter jede Autoren- und jede Benutzerregel. Was die Kaskade entschieden
/// hat, bleibt deshalb unberuehrt.
///
/// # DIE WERTE SIND STRENG (HTML 5 §5.9)
///
/// Zulaessig ist eine **nichtnegative ganze Zahl**, optional mit `%`. Alles
/// andere — `width="-5"`, `width="abc"`, `width=""` — wird **ignoriert**, nicht
/// gerundet und nicht geraten.
fn praesentations_groesse<E: Element>(
    el: &E,
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    erg: &mut Berechnet,
) {
    // HTML 5 §5.9: die Elemente, fuer die `width`/`height` als
    // Praesentationsangabe definiert sind. `input`/`button` gehoeren NICHT
    // dazu — dort waere es eine Formular-Eigenschaft, keine Groesse.
    // Release 14.10.2026: `canvas` neither (HTML §15.4.3 lists embed, iframe,
    // img, object, video; the canvas attributes are its natural size, see
    // `narsil-page`). MEASURED before: WPT css-align
    // `abspos/stretch-intrinsic-size-htb-htb` (`<canvas width=10>` with
    // `justify-self: stretch` stayed 10 px, the attribute counted as `width`).
    if !matches!(
        el.tag(),
        "img" | "embed" | "iframe" | "object" | "video"
    ) {
        return;
    }
    let frei = |e: Eigenschaft| gewinner(e, gebote, blaetter, inline).is_none();

    // # DER BEFUND 03.10.2026 (Punkt 3 der Leitung: "nixos Logo fuellt das
    // Fenster")
    //
    // Das echte Tag von nixos.org:
    //
    // ```html
    // <img width="7129" height="1995" class="h-12 w-auto"
    //      src="/_astro/nixos-logo-default-gradient-black-regular.svg">
    // ```
    //
    // GEMESSEN vorher, mit `.w-auto{width:auto}` und `.h-12{height:3rem}`:
    //
    // ```text
    //   breite = Some(7129.0)   hoehe = Some(48.0)
    // ```
    //
    // GEMESSEN: Firefox zeigt das Logo **3rem hoch** und laesst die Breite
    // `auto` — also etwa 3rem mal 3.58 (das Seitenverhaeltnis der SVG), nicht
    // 7129 px breit. Genau das ist "das Logo fuellt das Fenster".
    //
    // DIE URSACHE: hier stand `erg.breite.is_none()`. Aber **`width: auto`
    // laesst `breite` leer** — es ist eine gewonnene Deklaration, deren Wert
    // "kein fester Wert" ist. Das Attribut sah deshalb ein freies Feld und
    // schrieb sich hinein, **obwohl die Autorenregel gewonnen hatte**.
    //
    // `praesentations_ausrichtung` macht es seit eaef4b2 richtig: sie fragt
    // `gewinner()`, also „hat eine Deklaration gewonnen?". Genau das gehoert
    // hierher. `max-width:100%` (nixos, github, Tailwind) ist derselbe Fall:
    // `max_breite` begrenzt die Breite, aber das Attribut lieferte die
    // ungebegrenzten 7129.
    //
    // `erg.breite_art` bleibt zusaetzlich geprueft: ein `max-content` aus
    // einem Messauftrag ist keine Autorenregel und soll nicht verdraengt
    // werden — der Attribut-Hinweis greift dann, weil `breite` leer ist.
    if frei(Eigenschaft::Breite)
        && erg.breite_art.is_empty()
        && let Some(l) = praesentations_laenge(el.attribut("width"))
    {
        setze_breite_oder_anteil(&mut erg.breite, &mut erg.breite_prozent, &l);
    }
    if frei(Eigenschaft::Hoehe)
        && let Some(l) = praesentations_laenge(el.attribut("height"))
    {
        setze_breite_oder_anteil(&mut erg.hoehe, &mut erg.hoehe_prozent, &l);
    }
}

/// Ein **Prozent** gehoert NICHT in die px-Feld: die Kaskade kennt den
/// umgebenden Block nicht, sie kennt nur die Ansicht.
///
/// GEMESSENER FEHLER IM ERSTEN VERSUCH (03.10.2026): `width="50%"` landete
/// mit einer 0-%-Basis in `breite` und ergab **`Some(0.0)`** — eine erfundene
/// Breite von 0 px. Das ist schlimmer als `None`: `None` laesst den Motor die
/// Intrinsik oder den Eltern nehmen, `0.0` macht das Bild unsichtbar.
///
/// Also derselbe Weg, den P19 fuer `max-width` gebaut hat: der Anteil wandert
/// nach `*_prozent` (`layout_grenzen_prozent`), und der Motor rechnet ihn
/// gegen den Inhalt des Eltern.
fn setze_breite_oder_anteil(feld: &mut Option<f32>, anteil: &mut Option<f32>, l: &Laenge) {
    if matches!(l.einheit, super::typen::Einheit::Prozent) {
        *anteil = Some(l.wert);
    } else {
        // `ansicht`/`ansicht_hoehe` = 0: eine Praesentationsangabe enthaelt
        // nach §5.9 nie `vw`/`vh`/`vmin`/`vmax`, und `em` rechnet gegen die
        // eigene Schrift.
        *feld = breiten_wert(
            &Wert::Laenge(*l),
            BASIS_SCHRIFT_PX,
            BASIS_SCHRIFT_PX,
            0.0,
            0.0,
        );
    }
}

/// Das Attribut als [`Laenge`] — nur wenn es nach HTML 5 §5.9 eine erlaubte
/// Angabe ist. `None` heisst "die Angabe gilt nicht", nicht "0".
fn praesentations_laenge(roh: Option<&str>) -> Option<Laenge> {
    let t = roh?.trim();
    if t.is_empty() {
        return None;
    }
    // "valid non-negative integer": kein `-`, kein `+`, kein `.`, keine
    // Einheit, kein Leerraum drin. Nur so bleibt `width="50px"` draussen.
    let ziffern = t.strip_suffix('%').unwrap_or(t);
    if ziffern.is_empty() || !ziffern.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    laenge_aus_text(t)
}

/// HTML 5 §5.9, Rest der Liste: `valign` -> `vertical-align`,
/// `align` -> `text-align`, `bgcolor` -> `background-color`.
///
/// # GEMESSEN, welche Attribute auf den zehn Referenzseiten wirklich vorkommen
///
/// `team/log/codex8/vor/html/*.html`, gezaehlt mit grep, 03.10.2026:
///
/// | Attribut     | Treffer | wo           | wird hier abgebildet |
/// |--------------|---------|--------------|----------------------|
/// | `valign`     | **60**  | alle `<td>`  | ja: `vertical-align` |
/// | `align`      | **30**  | alle `<td>`  | ja: `text-align`     |
/// | `bgcolor`    | 3       | `<td>`/`<table>` | ja: `background-color` |
/// | `border`     | 3       | `<table>`    | nein, siehe unten    |
/// | `cellpadding`| 4       | `<table>`    | **nein**             |
/// | `cellspacing`| 4       | `<table>`    | **nein**             |
/// | `color`      | 9       | 7 `<svg>`, 2 `<link>` | **nein** (SVG `fill`) |
/// | `size`       | 3       | `<input>`    | **nein** (Formular)  |
/// | `type`       | 131     | 122 in `script`/`link`/`input`/`button` | **nein** |
///
/// Die letzten vier Zeilen sind **Vermessung, nicht Auswahl**: `type` ist zu
/// 93 % `type="text"`/`type="stylesheet"`, `color` ist die SVG-`fill`-Kuerzel
/// und `size` die Zahl der sichtbaren Zeichen im Eingabefeld. Sie sind keine
/// Praesentationsangaben und werden hier nicht angefasst.
///
/// `cellpadding` und `cellspacing` haben **kein CSS-Gegenstueck** — sie sind
/// HTML-eigene Tabellenattribute. Sie zu emulieren hiesse `padding`/`border`
/// zu erfinden, und das gehoert in den Tabellen-Leg (BRETT an SERVER/CODEX),
/// nicht in die Kaskade.
///
/// `border="0"` auf `<table>` ist in HTML 5 ebenfalls HTML-eigen (es zeichnet
/// einen Rahmen **um die Tabelle und um jede Zelle**). Ohne Tabellen-Leg waere
/// das eine halbe Loesung, die zusaetzlich `border-width` erfindet — also
/// ebenfalls BRETT.
///
/// # DIE RANGFOLGE
///
/// Wie `width`/`height` stehen auch diese Attribute ganz am Anfang des
/// Autor-Urspungs: **unter** jeder Autoren- und Benutzerregel. Geprueft wird
/// das ueber `gewinner()` — die Eigenschaft zaehlt als "gesetzt", sobald
/// irgendeine Deklaration aus irgendeinem Blatt oder `style=""` gewonnen hat.
/// Das ist genauer als ein Vergleich mit dem Feldwert: `text-align` erbt, und
/// ein Feldvergleich wuerde einen geerbten Wert mit einem gesetzten verwechseln.
fn praesentations_ausrichtung<E: Element>(
    el: &E,
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    erg: &mut Berechnet,
) {
    let frei = |e: Eigenschaft| gewinner(e, gebote, blaetter, inline).is_none();

    // `valign` auf einer Tabellenzelle: top / middle / bottom / baseline.
    // GEMESSEN: 60x `valign="top"` auf `<td>` (de.wikipedia Infobox).
    //
    // `erg.vertikal` traegt den Roh-Wert (CSS Inline 3 §6), also passt das
    // Schluesselwort unveraendert hinein.
    if erg.vertikal.is_empty()
        && frei(Eigenschaft::VertikalAlign)
        && let Some(v) = el.attribut("valign")
    {
        let v = v.trim();
        if matches!(v, "top" | "middle" | "bottom" | "baseline") {
            erg.vertikal = String::from(v);
        }
    }

    // `align` auf einer Tabellenzelle: CSS 2.1 §17.6.1 raeumt es zu
    // `text-align` auf. GEMESSEN: 30x `align="right"` auf `<td>`
    // (nixos.org Basisdatentabelle).
    //
    // `align` auf `img` waere `float` (nicht `text-align`) — an den zehn
    // Referenzseiten kommt `align` **ausschliesslich** auf `<td>` vor (30 von
    // 30), deshalb wird der Bild-Fall hier bewusst nicht geraten.
    if frei(Eigenschaft::Textausrichtung)
        && let Some(a) = el.attribut("align")
    {
        let a = a.trim();
        if matches!(a, "left" | "right" | "center" | "justify") {
            erg.ausrichtung_logisch = None;
        }
        erg.textausrichtung = match a {
            "left" => Textausrichtung::Links,
            "right" => Textausrichtung::Rechts,
            "center" => Textausrichtung::Zentriert,
            "justify" => Textausrichtung::Blocksatz,
            // Alles andere (z. B. `char` auf `<table>`) ist keine Ausrichtung
            // fuer `text-align` und wird ignoriert.
            _ => erg.textausrichtung,
        };
    }

    // `bgcolor` -> `background-color`. GEMESSEN: 1x `#f6f6ef` (Wikipedia
    // Infobox-Hintergrund), 2x `#ff6600` (nixos.org). Die Form ist immer
    // `#rrggbb`, dieselbe Schreibweise wie im CSS.
    //
    // GEMESSENER FEHLER IN DER EIGENEN IMPLEMENTIERUNG: hier stand zusaetzlich
    // `&& erg.hintergrund == Farbe::WEISS`, um "nicht gesetzt" zu erkennen.
    // Der Anfangswert von `background-color` ist aber `DURCHSICHTIG`
    // (`typen.rs:2348`), nicht Weiss — die Bedingung war also **immer
    // falsch**, und `bgcolor` kam nie an. Genau die Doppelabfrage, vor der
    // die Doku von `praesentations_ausrichtung` warnt: `frei()` sagt bereits,
    // ob eine Deklaration gewonnen hat, und ein zweiter Vergleich auf den
    // Feldwert kann nur etwas anderes behaupten.
    if frei(Eigenschaft::Hintergrund)
        && let Some(f) = el.attribut("bgcolor")
    {
        // `parse::farbe_aus` ist crate-privat. HTML 5 §5.9 nennt fuer
        // `bgcolor` eine "simple colour" — ein Farbname oder `#rrggbb` —
        // und **alle drei** Vorkommen auf den zehn Referenzseiten sind
        // `#rrggbb` (GEMESSEN: `#f6f6ef`, 2x `#ff6600`). Darum ein eigener
        // Leser fuer genau diese Form, streng: sechs Ziffern, sonst nichts.
        //
        // WARUM NICHT `parse::farbe_aus` oeffnen: das waere eine
        // Sichtbarkeitsaenderung fuer einen Weg, den die Kaskade an dieser
        // Stelle nicht braucht. Wer den Farbnamen will, hat `bgcolor`
        // schon fast nie — und fuer die drei gemessenen Faelle reicht es.
        if let Some(c) = hex_bgcolor(f.trim()) {
            erg.hintergrund = c;
        }
    }
}

/// HTML 5 §5.9, die **Tabellen**-Attribute: `cellpadding` -> `padding` der
/// Zellen, `cellspacing` -> `border-spacing`, `border` -> `border-width` und
/// `border-style` von Tabelle **und** Zellen.
///
/// # WARUM DAS HIERHER GEHOERT UND DAMALS NICHT (03.10.2026, Rolle CSS)
///
/// In der ersten Fassung stand hier, `cellpadding` und `cellspacing` haetten
/// "kein CSS-Gegenstueck" und gehoerten in den Tabellen-Leg. Das war **halb
/// richtig**: es fehlte nicht die Zuordnung, sondern das **Feld**. `border-
/// spacing` stand im Parser als `ESchluessel` und hatte damit weder eine
/// `Eigenschaft` noch ein Feld in `Berechnet` — die Kaskade konnte den Wert
/// nicht einmal annehmen, egal wer ihn liest. Beide sind jetzt angelegt
/// (`Eigenschaft::RandAbstandWaagerecht`, `Berechnet::randabstand_*`).
///
/// Der Rest bleibt beim Tabellen-Leg: wer die 2 px **zeichnet**, ist nicht die
/// Kaskade. Das ist eine getrennte Meldung.
///
/// # DIE RANGFOLGE
///
/// Wie die anderen Attribute ganz am Anfang des Autor-Urspungs, also **unter**
/// jeder Autoren- und Benutzerregel — geprueft ueber `gewinner()`.
///
/// # GEMESSEN auf den zehn Referenzseiten (`team/log/codex8/vor/html/*.html`)
///
/// | Attribut      | Treffer | Werte        | Tags        |
/// |---------------|---------|--------------|-------------|
/// | `cellspacing` | **4**   | alle `"0"`   | `<table>`   |
/// | `cellpadding` | **4**   | 3x `"0"`, 1x `"1"` | `<table>` |
/// | `border`      | **3**   | alle `"0"`   | `<table>`   |
///
/// `cellspacing="0"` ist der interestingeste Fall: der **Anfangswert** von
/// `border-spacing` ist `0` (§17.6), aber die HTML-Vorgabe im Browser ist
/// **2 px**. Das Attribut schaltet also etwas ab, was sonst da waere.
fn praesentations_tabelle<E: Element>(
    el: &E,
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    erg: &mut Berechnet,
) {
    let frei = |e: Eigenschaft| gewinner(e, gebote, blaetter, inline).is_none();
    let tag = el.tag();

    // `cellspacing="0"` -> `border-spacing: 0` (§17.6, beide Achsen).
    // Release 11.10.2026: ohne `cellspacing` gilt fuer `<table>` die
    // UA-Vorgabe `border-spacing: 2px` (HTML §15.3.8). Sie steht HIER und
    // nicht im UA-Blatt, weil das Praesentationsattribut ueber dem UA-Blatt
    // steht: als UA-Regel war `border-spacing` nicht mehr „frei", und
    // `cellspacing="0"` wirkte nicht (WPT `table-anonymous-objects-059`).
    // `display: table` auf anderen Elementen und anonyme Tabellen: 0.
    if tag == "table" && frei(Eigenschaft::RandAbstandWaagerecht) {
        let px = praesentations_zahl(el.attribut("cellspacing")).unwrap_or(2.0);
        erg.randabstand_waagerecht = px;
        erg.randabstand_senkrecht = px;
    }

    // `border="1"` -> `border-width: 1px` + `border-style: solid`, auf der
    // Tabelle **und** auf jeder Zelle (HTML 5 §5.9 zeichnet beides).
    //
    // `border-style` ist noetig, weil `border-width` allein nichts zeichnet:
    // der Anfangswert von `border-style` ist `none` (§8.3). Das ist der
    // haeufigste stillschweigende Fehler bei dieser Zuordnung.
    //
    // `border="0"` ist damit ein **Abschalten**: `width: 0` und `style: none`
    // sagen beide dasselbe, und es ist genau das, was die Attribute meinen.
    let rand_px = if tag == "table" {
        praesentations_zahl(el.attribut("border"))
    } else if matches!(tag, "td" | "th") {
        tabelle_oben(el).and_then(|t| praesentations_zahl(t.attribut("border")))
    } else {
        None
    };
    if let Some(px) = rand_px {
        if frei(Eigenschaft::RahmenObenBreite) && px > 0.0 {
            erg.rahmen_oben_breite = px;
            erg.rahmen_rechts_breite = px;
            erg.rahmen_unten_breite = px;
            erg.rahmen_links_breite = px;
        }
        if px > 0.0 && frei(Eigenschaft::RahmenObenStil) {
            erg.rahmen_oben_stil = Rahmenstil::Durchgehend;
            erg.rahmen_rechts_stil = Rahmenstil::Durchgehend;
            erg.rahmen_unten_stil = Rahmenstil::Durchgehend;
            erg.rahmen_links_stil = Rahmenstil::Durchgehend;
        }
    }

    // `cellpadding="4"` -> `padding: 4px` an den **Zellen**. Das Attribut steht
    // an der Tabelle, die Wirkung sitzt an der Zelle — deshalb der Weg ueber
    // den Vorfahren. HTML 5 §5.9 sagt genau das.
    //
    // Release 11.10.2026 (Agent CSS2): ohne `cellpadding` gilt die
    // UA-Vorgabe `td, th { padding: 1px }` (HTML §15.3.8) — HIER und nicht
    // im UA-Blatt, aus demselben Grund wie `border-spacing` oben: das
    // Attribut (auch `cellpadding="0"`) steht ueber dem UA-Blatt.
    if matches!(tag, "td" | "th")
        && let Some(tabelle) = tabelle_oben(el)
        && let Some(px) = Some(praesentations_zahl(tabelle.attribut("cellpadding")).unwrap_or(1.0))
        && px >= 0.0
    {
        // Je Seite: `td { padding-left: 0 }` laesst die anderen drei frei.
        if frei(Eigenschaft::FuellungOben) {
            erg.fuellung_oben = px;
        }
        if frei(Eigenschaft::FuellungRechts) {
            erg.fuellung_rechts = px;
        }
        if frei(Eigenschaft::FuellungUnten) {
            erg.fuellung_unten = px;
        }
        if frei(Eigenschaft::FuellungLinks) {
            erg.fuellung_links = px;
        }
    }
}

/// Der naechste `<table>`-Vorfahr, sonst `None`.
///
/// HTML 5 §5.9: `cellpadding` und `border` stehen an der `<table>` und wirken
/// auf die Zellen. `Element::eltern()` ist der einzige Weg dorthin, den die
/// Kaskade von einem Knoten aus hat.
///
/// GEMESSENE KOSTEN: der Weg läuft nur fuer `td`/`th` und steigt dort wenige
/// Ebenen. Fuer `<table>` selbst wird er nie gebraucht — dort steht das
/// Attribut direkt am Knoten.
fn tabelle_oben<E: Element>(el: &E) -> Option<E> {
    let mut k = el.eltern();
    let mut schritte = 0;
    while let Some(e) = k {
        schritte += 1;
        // HTML erlaubt keine Tabelle in einer Tabelle. Eine Grenze ist hier
        // trotzdem noetig, weil `eltern()` ein fremdes BaumeLEMENT ist: ein
        // Endlosschleifen-Schutz ist billiger als eine haengende Seite.
        if schritte > 32 {
            return None;
        }
        if e.tag().eq_ignore_ascii_case("table") {
            return Some(e);
        }
        k = e.eltern();
    }
    None
}

/// HTML 5 §5.9: `hspace` und `vspace` sind **Rand**, keine Fuge.
///
/// `hspace` -> `margin-left`/`margin-right`, `vspace` -> `margin-top`/
/// `margin-bottom` (CSS 2.1 §8.3). Der Wortlaut der Spec ist "Horizontal
/// space around the element"; `margin` ist genau das, `padding` waere es nicht.
///
/// # GEMESSEN: NULL Vorkommen
///
/// `hspace` und `vspace` kommen auf **keiner** der zehn Referenzseiten vor
/// (grep, 03.10.2026). Das ist keine Auswahl, sondern eine Zaehlung. Umgesetzt
/// sind sie damit nach **Vollstaendigkeit** (HTML 5 §5.9), nicht nach
/// Messung — es gibt keinen Messwert, den man hier nennen koennte.
fn praesentations_abstand<E: Element>(
    el: &E,
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    erg: &mut Berechnet,
) {
    // HTML 5 §5.9 nennt fuer `hspace`/`vspace` genau diese Elemente.
    if !matches!(el.tag(), "img" | "object" | "iframe" | "embed" | "applet") {
        return;
    }
    let frei = |e: Eigenschaft| gewinner(e, gebote, blaetter, inline).is_none();

    if let Some(px) = praesentations_zahl(el.attribut("hspace")) {
        if px > 0.0 && frei(Eigenschaft::MargeLinks) {
            erg.marge_links = px;
            erg.marge_rechts = px;
            // `hspace` ist eine **Laenge**, kein `auto`. Die Flaggen MUSSEN
            // geloescht werden, sonst zentrierte der Motor einen Rand, der
            // eine echte Pixelbreite hat (CODEX-Blocker, BRETT 19:4x).
            erg.marge_links_auto = false;
            erg.marge_rechts_auto = false;
        }
    }
    if let Some(px) = praesentations_zahl(el.attribut("vspace")) {
        if px > 0.0 && frei(Eigenschaft::MargeOben) {
            erg.marge_oben = px;
            erg.marge_unten = px;
        }
    }
}

/// Die Zahl aus `cellpadding="4"` / `cellspacing="0"` / `border="1"` /
/// `hspace="8"`.
///
/// HTML 5 §5.9: eine "valid non-negative integer" — dieselbe Form wie bei
/// `width`, und derselbe bewusst strenge Leser. Kein `%`, keine Einheit, kein
/// Vorzeichen, keine Zahl mit Punkt. Alles andere wird ignoriert.
fn praesentations_zahl(roh: Option<&str>) -> Option<f32> {
    let t = roh?.trim();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // `u32` statt `f32`: die Zahlen sind klein, und ein Ueberlauf wuerde
    // sonst zu `inf` und dann zu einem Kasten weit ausserhalb des Bildes.
    t.parse::<u32>().ok().map(|n| n as f32)
}

/// `#rrggbb` -> [`Farbe`]. Streng: genau sechs Ziffern nach dem `#`.
///
/// HTML 5 §5.9 nennt `#rrggbb` und die Kurzform `#rgb`; die Kurzform kommt
/// auf den zehn Referenzseiten **nicht** vor (GEMESSEN), deshalb wird sie hier
/// nicht geraten. `#rrggbbaa` und `#rgb` bleiben unberuehrt, statt sie
/// falsch zu deuten.
fn hex_bgcolor(t: &str) -> Option<Farbe> {
    let z = t.strip_prefix('#')?;
    if z.len() != 6 || !z.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let b = z.as_bytes();
    let h = |i: usize| -> u8 {
        let z = (b[i] as char).to_digit(16).unwrap_or(0);
        (z * 16 + (b[i + 1] as char).to_digit(16).unwrap_or(0)) as u8
    };
    Some(Farbe {
        r: h(0),
        g: h(2),
        b: h(4),
        a: 255,
    })
}

fn loese_farbe(
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    eltern: Option<&Berechnet>,
    vars: &Variablen,
    schema: Farbschema,
    erg: &mut Berechnet,
) {
    // GEMESSEN 30.09.2026 (R0 a): hier standen zwei `println!` auf
    // stdout. Beim Lauf ueber 30 echte Seiten (R2/A4) hat der
    // Pruefstand dadurch rund 400 Zeilen "KEIN GEWINNER FUER FARBE!"
    // und "PASST: false fuer tag=..." in die Bildschirmausgabe
    // geschrieben — eine Ausgabe, die von einem Programm kommt, das
    // eigentlich nur Messwerte liefert. Sie war auch der Grund, warum
    // `KEIN GEWINNER` im Log stand, obwohl die Seite ihre Farbe
    // bekommen hat: die Zeile bedeutet "keine DEKLARATION gefunden",
    // nicht "keine Farbe".
    let Some(d) = gewinner(Eigenschaft::Farbe, gebote, blaetter, inline) else {
        return;
    };
    if debug_an() {
        // GEMESSEN 30.09.2026 (R0 a): hier stand ein `println!` OHNE
        // Bremse. Beim Lauf ueber 30 echte Seiten (R2/A4) hat der
        // Pruefstand rund 400 Zeilen "KEIN GEWINNER FUER FARBE!" und
        // "PASST: false fuer tag=..." in die Bildschirmausgabe
        // geschrieben — von einem Programm, das nur Messwerte liefern
        // soll. Und die Zeile loggte Missverstaendnis: sie bedeutet
        // "keine DEKLARATION gefunden", nicht "keine Farbe".
        //
        // JETZT NUR AUF WUNSCH: `NARSIL_CSS_DEBUG=1`. `debug_an()` liest
        // die Umgebung EINMAL (M2: `std::env::var` in einer Schleife
        // ueber jedes Element und jede Eigenschaft ist eine Allokation
        // pro Aufruf — bei 50 000 Kaskaden sind das 50 000 Kopien der
        // Environment-Zeichenkette).
        println!("GEWINNER FARBE: {d:?}");
    }
    // var() in Farben (z.B. rgb(var(--r),0,0)) wird substituiert (C10).
    let ctx = RechenCtx {
        em: BASIS_SCHRIFT_PX,
        // Ohne Eltern gibt es keine Wurzelschrift: die Vorgabe (16 px).
        rem: BASIS_SCHRIFT_PX,
        prozent: BASIS_SCHRIFT_PX,
        ansicht: 0.0,
        // Farben brauchen keine Ansicht; `0.0` heisst hier "unbekannt",
        // und `ansichts_hoehe` faellt dann auf die Breite zurueck.
        ansicht_hoehe: 0.0,
    };
    let w = match effektiver_wert(Eigenschaft::Farbe, d, vars, &ctx) {
        None => return,
        Some(v) => v,
    };
    match &w {
        // =================================================================
        // `light-dark(<hell>, <dunkel>)` (CSS Color 5 §6.2) — AB HAENGIG VOM
        // SCHEMA (03.10.2026, Rolle CSS).
        //
        // GEMESSENER FEHLER: `farbe_aus` gab hier immer das **helle** Feld
        // zurueck, mit der Begruendung "das Schema kommt nicht hier herein —
        // das Stylesheet wird einmal gelesen". Diese Begruendung war seit
        // K13 (03.10. 20:0x) **falsch**: `Zustand::farbschema` wird je Kaskade
        // befuellt, und `@media (prefers-color-scheme: dark)` wird damit
        // ausgewertet (GEMESSEN: dunkel -> bg 16,20,24 wie Firefox). Es gab
        // also zwei verschiedene Antworten auf dieselbe Frage.
        //
        // GEMESSENE REICHWEITE, und deshalb ist es kein Detail: auf
        // `developer.mozilla.org` steht **`light-dark(` 42 Mal** und
        // `prefers-color-scheme` **null Mal** (grep,
        // `team/log/codex8/vor/html/*.html`). Das Dunkelthema von MDN haengt
        // vollstaendig an dieser Funktion; ein `@media`-Fix haette dort gar
        // nichts geaendert. Auf `duckduckgo` (2x) und `www.wikipedia` (3x)
        // steht umgekehrt nur `@media` — dort greift der andere Weg.
        //
        // WARUM HIER und nicht in `farbe_aus`: dort kennt man das Schema
        // nicht, das Blatt wird einmal gelesen. Hier laeuft die Kaskade je
        // Knoten, und `d.roh` traegt den Rohtest der gewonnenen Deklaration.
        //
        // `var()` in den beiden Feldern: **nicht** aufgeloest. Der
        // Platzhalter greift, und es bleibt die dokumentierte alte Antwort
        // (das helle Feld). Das ist eine bewusste Grenze, kein Versehen.
        // =================================================================
        Wert::Farbe(_) if d.roh.trim_start().starts_with("light-dark(") => {
            if let Some(c) = light_dark_aus(&d.roh, schema) {
                erg.farbe = c;
            }
        }
        // Steht bewusst HINTER dem `light-dark()`-Zweig: der Guard dort greift
        // nur, wenn der Rohtext wirklich mit `light-dark(` beginnt.
        Wert::Farbe(f) => erg.farbe = *f,
        Wert::Aktuell => {
            erg.farbe = eltern.map(|p| p.farbe).unwrap_or(Farbe::SCHWARZ);
        }
        Wert::Erben => {
            erg.farbe = eltern.map(|p| p.farbe).unwrap_or(Farbe::SCHWARZ);
        }
        Wert::Anfang => erg.farbe = Farbe::SCHWARZ,
        Wert::Unbestimmt => {
            erg.farbe = eltern.map(|p| p.farbe).unwrap_or(Farbe::SCHWARZ);
        }
        _ => {}
    }
}

/// `light-dark(<hell>, <dunkel>)` (CSS Color 5 §6.2) gegen das Schema.
///
/// GEMESSEN (03.10.2026, Rolle CSS, Punkt 1 der Leitung):
///
/// ```text
/// body{color:light-dark(#000000,#ffffff)}
///   Schema Hell   -> fg = (0, 0, 0)      Firefox hell
///   Schema Dunkel -> fg = (0, 0, 0)      Firefox DUNKEL: (255, 255, 255)
/// ```
///
/// `Farbschema::Keine` (`no-preference`, §2.2) nimmt wie `Hell` das erste
/// Feld — es ist eine ausdrueckliche Entschiedung gegen „irgendwas", und
/// `Hell` ist der Anfangswert von `Farbschema`.
/// Kommas auf **Klammerebene 0** in `t`. Die in `rgb(1,2,3)` stehen auf
/// Ebene 1 und zaehlen nicht.
fn comma_ebene_null(t: &str) -> usize {
    let mut tiefe = 0i32;
    let mut n = 0usize;
    for b in t.bytes() {
        match b {
            b'(' => tiefe += 1,
            b')' => tiefe -= 1,
            b',' if tiefe == 0 => n += 1,
            _ => {}
        }
    }
    n
}

fn light_dark_aus(roh: &str, schema: Farbschema) -> Option<Farbe> {
    let k = roh.trim();
    let innen = k.strip_prefix("light-dark(")?.strip_suffix(')')?;
    if innen.contains("var(") || innen.contains("calc(") {
        // Dokumentierte Grenze, siehe Aufrufer.
        return None;
    }
    // Genau EINE Kommaebene: `light-dark(rgb(1,2,3), #fff)` darf nicht an
    // den Kommas in `rgb()` zerbrechen.
    let mut tiefe = 0i32;
    let mut trenn = None;
    for (i, b) in innen.char_indices() {
        match b {
            '(' => tiefe += 1,
            ')' => tiefe -= 1,
            ',' if tiefe == 0 => {
                trenn = Some(i);
                break;
            }
            _ => {}
        }
    }
    let i = trenn?;
    let felder = [&innen[..i].trim(), &innen[i + 1..].trim()];
    // CSS Color 5 §6.2 nimmt genau zwei Felder. Ein **zweites Komma auf
    // Klammerebene 0** heisst drei Felder -> ungueltig.
    //
    // GEMESSENER FEHLER MEINER ERSTEN FASSUNG: hier stand ein blodes
    // `felder[1].contains(',')`. Das wirft aber auch
    // `light-dark(#000, rgb(200,201,202))` weg, denn die Kommas in `rgb()`
    // stehen auf Ebene 1 und sind gueltig. Zaehlen heisst: Ebene 0.
    if comma_ebene_null(felder[1]) > 0 {
        return None;
    }
    let gewaehlt = if schema == Farbschema::Dunkel {
        felder[1]
    } else {
        felder[0]
    };
    crate::parse::farbe_aus(gewaehlt)
}

/// Alle uebrigen Eigenschaften (Schriftgroesse/Farbe schon geloest).
fn loese_eigenschaft(
    e: Eigenschaft,
    gebote: &[[Option<Gebot>; 6]; EIGENSCHAFTEN],
    blaetter: &[Stylesheet],
    inline: &[Deklaration],
    eltern: Option<&Berechnet>,
    ansicht: f32,
    // Ansichts-HOEHE in px fuer `vh`/`vmin`/`vmax` (R0 e, 30.09.2026).
    ansicht_hoehe: f32,
    vars: &Variablen,
    erg: &mut Berechnet,
) {
    // 03.10.2026 15:0x: dieselbe Basis fuer `rem` (an der Wurzel ihre eigene
    // Schrift, siehe `rem_basis_hier`).
    let rem_basis = eltern.map_or(erg.wurzel_schrift.max(0.0), |e| e.wurzel_schrift);
    zeile_px_setzen(erg.zeilenhoehe_px);
    profil(pz::EIGENSCHAFTEN);
    let d = match gewinner(e, gebote, blaetter, inline) {
        None => return,
        Some(v) => v,
    };
    let em_vor = erg.schriftgroesse_px;
    // var()/calc() zuerst aufloesen (C10/C11, W6); %-Basis ist hier die
    // Ansicht (ausser Zeilenhoehe — dort eigene Schrift, siehe Arm).
    let ctx = RechenCtx {
        em: em_vor,
        // 03.10.2026 17:5x: `rem_basis` steht drei Zeilen ueber dieser
        // Konstruktion (gleiche Funktion).
        rem: rem_basis,
        // Release 12.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing`
        // rechnen Prozente gegen die eigene Schrift (CSS Text 4 §4.1/§4.2) —
        // auch in `calc(0.05em + 5%)`.
        prozent: if matches!(e, Eigenschaft::LetterSpacing | Eigenschaft::Wortabstand) { em_vor } else { ansicht },
        ansicht,
        ansicht_hoehe: super::rechnen::ansichts_hoehe(ansicht, ansicht_hoehe),
    };
    profil(pz::WERTE);
    let w = match effektiver_wert(e, d, vars, &ctx) {
        None => {
            // "INVALID AT COMPUTED-VALUE TIME" (CSS Variables 1, §3.2;
            // CSS Cascade 6, §5.2.2): eine Deklaration mit `var()`, das
            // sich nicht aufloesen laesst, ist zur Berechnungszeit
            // UNGUELTIG — und das heisst ausdruecklich:
            //
            //   * vererbte Eigenschaft  -> den ERBten Wert
            //   * nicht vererbte        -> den ANFANGSWERT
            //
            // es heisst ausdruecklich NICHT "die naechstbeste Deklaration
            // gewinnt". Das ist der Unterschied, und er ist billig zu
            // verwechseln.
            //
            // GEMESSENER FEHLER 01.10.2026 (K3,
            // `var_ohne_auflosung_verwirft_diese_deklaration`):
            // `.k { border-radius: 1px } .k { border-radius: var(--weg) }`
            // ergab [1;4] statt [0;4] — die Deklaration wurde
            // einfach UEBERSPRUNGEN, und der Wert der VORHERIGEN Regel
            // blieb stehen. `border-radius` ist nicht vererbt, also
            // haette es 0 sein muessen.
            profil(pz::WERTE);
            if wird_vererbt(e) {
                erbe(e, eltern, erg);
            } else {
                anfang(e, erg);
            }
            return;
        }
        Some(v) => v,
    };
    // Generisch: Erben/Anfang/Unbestimmt (gilt fuer jede Eigenschaft).
    match &w {
        Wert::Erben => {
            profil(pz::VERERBUNG);
            erbe(e, eltern, erg);
            return;
        }
        Wert::Anfang => {
            anfang(e, erg);
            return;
        }
        Wert::Unbestimmt => {
            if wird_vererbt(e) {
                erbe(e, eltern, erg);
            } else {
                anfang(e, erg);
            }
            return;
        }
        _ => {}
    }
    let em = erg.schriftgroesse_px;
    // Release 11.10.2026: die Rasterwerte des neuen Rasteralgorithmus (CSS
    // Grid 1/2, Box Alignment 3) aus dem Rohtext — vor dem `match`, weil der
    // dort die Namen, `safe`/`left`/`normal` und Prozentluecken verliert.
    // GEMESSEN vorher: WPT css-grid 564 von 2460 (u. a. `minmax(auto, …)`,
    // `fit-content()`, Liniennamen an den Kindern fehlten).
    {
        let roh_ersetzt;
        let roh: &str = if d.roh.contains("var(") {
            roh_ersetzt = super::rechnen::ersetze_var(&d.roh, vars, 0).unwrap_or_default();
            &roh_ersetzt
        } else {
            &d.roh
        };
        crate::gitter::kaskade_setzen(e, roh, &w, &ctx, erg);
        // Release 11.10.2026: `calc()` mit Prozent als Mischung (px, Prozent)
        // — der Anteil wird zweimal gerechnet (Basis 0 und 100). GEMESSEN
        // vorher: der Prozentanteil galt gegen die Ansicht (WPT css-flexbox
        // `gap-010-ltr`, `dynamic-isize-change-003`).
        mischung_setzen(e, roh, &ctx, erg);
    }
    match e {
        Eigenschaft::Anzeige => {
            if let Wert::Wort(w) = &w {
                // K7 (01.10.2026): `inline-flex` und die Tabellen-
                // Schluesselwoerter (CSS Display 3 §2.5, CSS Tables 3
                // §2.1). Der Parser liest sie seit K7, aber dieser
                // `match` kannte sie nicht — und der Notfall-Zweig
                // `_ => Anzeige::Inline` hat sie geschluckt.
                //
                // GEMESSENER FEHLER: `display: inline-flex` ergab
                // `Inline`, nicht `InlineFlex`. Das heisst: das Element
                // war inline statt Flex-Container, und seine Kinder
                // waren keine Flex-Items (CSS Flexbox 1 §2).
                erg.nur_inhalt = w == "contents";
                // Release 11.10.2026 (Agent CSS2): auch `inline-table` steht
                // aussen inline (CSS 2.1 §17.4: eine atomare Inline-Box).
                erg.inline_aussen = matches!(w.as_str(), "inline-flex" | "inline-grid" | "inline-grid-lanes" | "inline-table");
                erg.flow_root = w.as_str() == "flow-root";
                erg.raster_lanes = matches!(w.as_str(), "grid-lanes" | "inline-grid-lanes");
                // Release 11.10.2026 (Agent CSS2): die genaue Tabellenrolle
                // (CSS 2.1 §17.2) — `anzeige` unten fasst sie zusammen.
                {
                    use narsil_layout::plan::TabellenRolle as R;
                    erg.tabellen_rolle = match w.as_str() {
                        "table" => R::Tabelle,
                        "inline-table" => R::InlineTabelle,
                        "table-row-group" => R::Rumpf,
                        "table-header-group" => R::Kopf,
                        "table-footer-group" => R::Fuss,
                        "table-row" => R::Zeile,
                        "table-cell" => R::Zelle,
                        "table-caption" => R::Beschriftung,
                        "table-column" => R::Spalte,
                        "table-column-group" => R::Spaltengruppe,
                        _ => R::Keine,
                    };
                }
                erg.anzeige = match w.as_str() {
                    "block" | "flow-root" | "contents" => Anzeige::Block,
                    "inline-block" => Anzeige::InlineBlock,
                    "list-item" => Anzeige::ListenEintrag,
                    "flex" => Anzeige::Flex,
                    "inline-flex" => Anzeige::InlineFlex,
                    "grid" | "inline-grid" | "grid-lanes" | "inline-grid-lanes" => Anzeige::Raster,
                    // added 05.10.2026, Rolle SERVER (SERVER_13): **Jetzt
                    // sind es drei Rollen, nicht eine.** Sie teilten sich ein
                    // Feld, weil es „keinen Tabellen-Leg“ gab — der fehlt
                    // nicht mehr (WUNSCH_CSS.txt W10 ist damit geschlossen).
                    //
                    // GEMESSENER WEG, den die Information nun nimmt:
                    //   Kaskade -> plan::Anzeige -> `flow.rs` -> `lege_tabelle`
                    //
                    // `thead`/`tbody`/`tfoot` sind **eine** Rolle (CSS 2.1,
                    // §17.2 erzeugt fuer alle drei dieselben Boxen), und
                    // `caption`/`column`/`column-group` eine andere: die
                    // Caption ist ein Block ausserhalb des Rasters, die
                    // Spaltenbeschreibung erzeugt gar keine Boxen.
                    "table" | "inline-table" => Anzeige::Tabelle,
                    "table-row" | "table-row-group" | "table-header-group"
                    | "table-footer-group" => Anzeige::TabelleZeile,
                    "table-cell" => Anzeige::TabelleZelle,
                    "table-caption" => Anzeige::TabelleRand,
                    "table-column" | "table-column-group" => Anzeige::TabelleSpalte,
                    "none" => Anzeige::Keins,
                    _ => Anzeige::Inline,
                };
            }
        }
        Eigenschaft::Farbe => {}
        // `font-size` (CSS 2.1 §15.8, CSS Values 3 §5.1).
        //
        // GEMESSENER FEHLER 01.10.2026 (Rekonstruktion nach dem
        // Editierfehler um 04:50): hier stand `=> {}`. Die Laenge kam an,
        // wurde aber nicht in px umgerechnet — und `font-size: calc(1em +
        // 2px)` blieb bei 16 px stehen. Drei Tests haben es gefangen
        // (`schriftgroessen`, `calc_schrift_em`,
        // `koord_fall_var_in_calc`).
        //
        // `em` und `%` beziehen sich auf die Schriftgroesse des
        // ELTERN — das ist `erg.schriftgroesse_px` VOR dieser Aenderung
        // (siehe `em_vor` oben). Es gibt keine `larger`/`smaller`
        // in px, sondern in Stufen der Groessen-Tabelle.
        Eigenschaft::Schriftgroesse => match &w {
            Wert::Laenge(l) => {
                let px = match l.einheit {
                    super::typen::Einheit::Px | super::typen::Einheit::Zahl => l.wert,
                    super::typen::Einheit::Em => l.wert * em_vor,
                    super::typen::Einheit::Rem => l.wert * BASIS_SCHRIFT_PX,
                    super::typen::Einheit::Pt => l.wert * 4.0 / 3.0,
                    super::typen::Einheit::Prozent => l.wert / 100.0 * em_vor,
                    super::typen::Einheit::Vw => l.wert / 100.0 * ansicht,
                    super::typen::Einheit::Vh => l.wert / 100.0 * ansicht_hoehe,
                    super::typen::Einheit::Vmin => l.wert / 100.0 * ansicht.min(ansicht_hoehe),
                    super::typen::Einheit::Vmax => l.wert / 100.0 * ansicht.max(ansicht_hoehe),
                    // CSS Values 4 §6.1.2 — dokumentierte Naeherung,
                    // siehe der gleiche Arm in `laenge_px`.
                    super::typen::Einheit::Dvh => l.wert / 100.0 * ansicht_hoehe,
                    super::typen::Einheit::Dvw => l.wert / 100.0 * ansicht,
                    super::typen::Einheit::Dvmin => l.wert / 100.0 * ansicht.min(ansicht_hoehe),
                    super::typen::Einheit::Dvmax => l.wert / 100.0 * ansicht.max(ansicht_hoehe),
                    super::typen::Einheit::Lvh => l.wert / 100.0 * ansicht_hoehe,
                    // CSS Values 4 §6.1.2: `lh`/`rlh` sind die Zeilen-
                    // hoehe der ELTERN (bei `font-size`); die kennt die
                    // Kaskade nicht, also `em_vor` — dokumentierte
                    // Naeherung.
                    super::typen::Einheit::Lh => l.wert * line_height().0.unwrap_or(em_vor),
                    super::typen::Einheit::Rlh => l.wert * BASIS_SCHRIFT_PX,
                    super::typen::Einheit::Lvw => l.wert / 100.0 * ansicht,
                    super::typen::Einheit::Lvmin => l.wert / 100.0 * ansicht.min(ansicht_hoehe),
                    super::typen::Einheit::Lvmax => l.wert / 100.0 * ansicht.max(ansicht_hoehe),
                    super::typen::Einheit::Ch => l.wert * em_vor * ch_anteil(),
                    super::typen::Einheit::Ex => l.wert * em_vor * ex_anteil(),
                };
                if px.is_finite() {
                    erg.schriftgroesse_px = px.clamp(0.0, MAX_PX);
                }
            }
            Wert::Wort(x) => {
                // Die absolute Groessen-Tabelle. CSS 2.1 §15.8 nennt sie
                // RELATIV; die Zahlen sind die des Browsers, an dem
                // gemessen wurde. Sie stehen hier als Festlegung, nicht
                // als Ableitung.
                //
                // GEMESSEN: `font-size: large` = 18 px bei 16 px
                // Grundgroesse (Test `schriftgroessen`).
                let tabelle: [(&str, f32); 7] = [
                    ("xx-small", 9.0),
                    ("x-small", 10.0),
                    ("small", 13.0),
                    ("medium", 16.0),
                    ("large", 18.0),
                    ("x-large", 24.0),
                    ("xx-large", 32.0),
                ];
                if let Some((_, px)) = tabelle.iter().find(|(k, _)| k == x) {
                    erg.schriftgroesse_px = *px;
                } else if x == "larger" || x == "smaller" {
                    // CSS 3 §4.3: EINEN Schritt in der Tabelle.
                    // Der Schritt haengt an der aktuellen Groesse, nicht
                    // an der des Eltern — sonst waeren zwei Schritte in
                    // Folge nicht zwei Schritte.
                    let r = em_vor / BASIS_SCHRIFT_PX;
                    let ziel = if x == "larger" {
                        tabelle.iter().find(|(_, p)| *p > r).map(|(_, p)| *p)
                    } else {
                        tabelle.iter().rev().find(|(_, p)| *p < r).map(|(_, p)| *p)
                    };
                    if let Some(px) = ziel {
                        erg.schriftgroesse_px = px;
                    }
                }
            }
            _ => {}
        },
        Eigenschaft::Hintergrund => match &w {
            Wert::Farbe(f) => erg.hintergrund = *f,
            Wert::Aktuell => erg.hintergrund = erg.farbe,
            _ => {}
        },
        Eigenschaft::HintergrundMischung => {
            if let Wert::Wort(x) = &w {
                if let Some(m) = MischModus::aus_wort(x) {
                    erg.mischmodus = m;
                }
            }
        }
        Eigenschaft::Schriftgewicht => match &w {
            Wert::Laenge(l) => {
                erg.schriftgewicht = l.wert.clamp(1.0, 1000.0).round() as u16;
            }
            Wert::Wort(w) => {
                // Vereinfacht (dokumentiert): bolder/lighter in Stufen.
                let elterngewicht = eltern.map(|p| p.schriftgewicht).unwrap_or(400);
                erg.schriftgewicht = match w.as_str() {
                    "bolder" => {
                        if elterngewicht >= 700 {
                            900
                        } else {
                            700
                        }
                    }
                    "lighter" => {
                        if elterngewicht > 400 {
                            400
                        } else {
                            100
                        }
                    }
                    _ => erg.schriftgewicht,
                };
            }
            _ => {}
        },
        Eigenschaft::Schriftstil => {
            if let Wert::Wort(w) = &w {
                erg.schriftstil = match w.as_str() {
                    "kursiv" => Schriftstil::Kursiv,
                    _ => Schriftstil::Normal,
                };
            }
        }
        Eigenschaft::Schriftfamilie => {
            if let Wert::Wort(w) = &w {
                erg.schriftfamilie = w.clone();
            }
        }
        Eigenschaft::Zeilenhoehe => {
            // %-Basis ist hier die eigene Schrift (nicht die Ansicht):
            // bei var()/calc() erneut mit eigenem Kontext aufloesen.
            let w2;
            let wref = if hat_funktion(&d.roh, "var(") || hat_mathe(&d.roh) {
                let ctx2 = RechenCtx {
                    em,
                    // Gleiche Basis wie der Hauptkontext: `line-height: 1.5rem`
                    // ist sonst eine andere Zahl als `font-size: 1.5rem`.
                    rem: rem_basis,
                    prozent: em,
                    ansicht,
                    ansicht_hoehe: super::rechnen::ansichts_hoehe(ansicht, ansicht_hoehe),
                };
                w2 = match effektiver_wert(e, d, vars, &ctx2) {
                    None => return,
                    Some(v) => v,
                };
                &w2
            } else {
                &w
            };
            match wref {
                Wert::Wort(x) if x == "normal" => {
                    erg.zeilenhoehe_px = em * 1.2;
                    erg.zeilenhoehe_art = super::typen::ZeilenhoeheArt::Normal;
                },
                Wert::Laenge(l) => {
                    use super::typen::Einheit::Zahl;
                    if matches!(l.einheit, Zahl) {
                        erg.zeilenhoehe_px = (em * l.wert).clamp(0.0, MAX_PX);
                        erg.zeilenhoehe_art = super::typen::ZeilenhoeheArt::Faktor(l.wert);
                    } else {
                        erg.zeilenhoehe_art = super::typen::ZeilenhoeheArt::Absolut;
                        erg.zeilenhoehe_px =
                            laenge_px(l, em, rem_basis, em, ansicht, ansicht_hoehe)
                                .clamp(0.0, MAX_PX);
                    }
                }
                _ => {}
            }
        }
        Eigenschaft::Textausrichtung => {
            // Release 15.10.2026 (Agent CSS2): `match-parent` keeps the
            // inherited, already resolved alignment and drops the logical
            // marker, so this element's own direction does not turn it
            // around. MEASURED before (WPT css-text `text-align/text-align-
            // match-parent-01`): it acted like `start` of the child.
            // On the root (no parent) it is `start` (WPT css-text
            // `text-align-match-parent-root-rtl`).
            if let Wert::Wort(w) = &w
                && w == "match-parent"
                && eltern.is_some()
            {
                erg.ausrichtung_logisch = None;
            } else if let Wert::Wort(w) = &w {
                // K17: `start` und `end` sind keine festen Werte, sie
                // haengen an der Schreibrichtung (CSS Writing Modes 3
                // §2.4). GEMESSEN VORHER: sie fielen stur auf links
                // zurueck, was auf einer arabischen Seite bedeutet:
                // `text-align: start` war linksbuendig.
                //
                // Die Richtung ist zu diesem Zeitpunkt schon richtig:
                // `richtung_aus_gebote` laeuft vor der Schleife.
                let start_rechts = erg.richtung.start_ist_rechts();
                erg.blocksatz_alle = w.as_str() == "blocksatz-alle";
                erg.ausrichtung_logisch = match w.as_str() {
                    "start" | "anfang" | "match-parent" => Some(true),
                    "end" | "ende" => Some(false),
                    _ => None,
                };
                erg.textausrichtung = match w.as_str() {
                    "rechts" | "right" => Textausrichtung::Rechts,
                    "zentriert" | "center" => Textausrichtung::Zentriert,
                    "blocksatz" | "justify" | "blocksatz-alle" => Textausrichtung::Blocksatz,
                    "start" | "anfang" | "match-parent" => {
                        if start_rechts {
                            Textausrichtung::Rechts
                        } else {
                            Textausrichtung::Links
                        }
                    }
                    "end" | "ende" => {
                        if start_rechts {
                            Textausrichtung::Links
                        } else {
                            Textausrichtung::Rechts
                        }
                    }
                    _ => Textausrichtung::Links,
                };
            }
        }
        Eigenschaft::Textdekoration => {
            if let Wert::Wort(w) = &w {
                erg.dekoration = deko_bits(w);
            }
        }
        Eigenschaft::Weissraum => {
            if let Wert::Wort(w) = &w {
                erg.weissraum = match w.as_str() {
                    "pre" => Weissraum::Pre,
                    "nowrap" => Weissraum::KeinUmbruch,
                    "pre-wrap" => Weissraum::PreUmbruch,
                    "pre-line" => Weissraum::PreZeile,
                    // Release 13.10.2026 (Agent CSS2): `break-spaces` (CSS Text
                    // 3 §3) — vorher `normal`. GEMESSEN (WPT css-text
                    // `white-space/break-spaces-*`): erhaltene Leerzeichen
                    // fielen zusammen.
                    "break-spaces" => Weissraum::UmbruchLeer,
                    _ => Weissraum::Normal,
                };
            }
        }
        // CSS 2.1 §10.3.7: `margin-left/right: auto` auf einem Block mit
        // fester Breite verteilt den Restplatz zu gleichen Teilen auf die
        // beiden automatischen Raender — das ist der Zentrierungsweg.
        // `rand_wert` gibt fuer `Auto` 0 zurueck; ohne die Flagge kann der
        // Motor „Rand 0" nicht von „Rand automatisch verteilt" unterscheiden
        // (GEMESSENER FEHLER, BRETT 19:4x / 19:40 von CODEX).
        //
        // SENKRECHT ist `auto` bedeutungslos und ergibt 0 (§10.6.3), also
        // gibt es hierfür keine Flagge — genau darum sind es nur zwei.
        //
        // Die Flagge wird in DIESEM Zweig gesetzt und in `anfang()`
        // geloescht: eine spaeter gewinnende Laenge muss sie wieder
        // wegnehmen, sonst zentrierte der Motor einen Rand, der eine
        // echte Laenge hat.
        Eigenschaft::MargeOben => {
            erg.marge_prozent[0] = prozent_wert(&w);
            erg.marge_oben_auto = matches!(w, Wert::Auto);
            erg.marge_oben = rand_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::MargeRechts => {
            erg.marge_prozent[1] = prozent_wert(&w);
            erg.marge_rechts_auto = matches!(w, Wert::Auto);
            erg.marge_rechts = rand_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::MargeUnten => {
            erg.marge_prozent[2] = prozent_wert(&w);
            erg.marge_unten_auto = matches!(w, Wert::Auto);
            erg.marge_unten = rand_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::MargeLinks => {
            erg.marge_prozent[3] = prozent_wert(&w);
            erg.marge_links_auto = matches!(w, Wert::Auto);
            erg.marge_links = rand_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        // K17b: DIE SPIEGELUNG. `erg.richtung` ist zu diesem Zeitpunkt
        // schon gesetzt — `richtung_aus_gebote` laeuft bei Zeile 1311,
        // lange vor dieser Schleife. Ohne diese Zeilen waere
        // `margin-inline-start: 1.6em` in einem `dir=rtl`-Block immer
        // links gelandet.
        Eigenschaft::MargeInnenStart => {
            let auto = matches!(w, Wert::Auto);
            let w = rand_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            if erg.richtung.start_ist_rechts() {
                erg.marge_rechts = w;
                erg.marge_rechts_auto = auto;
            } else {
                erg.marge_links = w;
                erg.marge_links_auto = auto;
            }
        }
        Eigenschaft::MargeInnenEnde => {
            let auto = matches!(w, Wert::Auto);
            let w = rand_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            if erg.richtung.start_ist_rechts() {
                erg.marge_links = w;
                erg.marge_links_auto = auto;
            } else {
                erg.marge_rechts = w;
                erg.marge_rechts_auto = auto;
            }
        }
        // GEMESSENE REDLICHKEIT: `padding-inline` kommt im Korpus NULL
        // Mal vor. Der Zweig ist derselbe wie bei der Marge; er steht
        // hier, damit `padding-inline` nicht als „unbekannt" im Eimer
        // liegen bleibt, falls es doch einmal vorkommt.
        Eigenschaft::FuellungInnenStart => {
            let prozent = prozent_wert(&w);
            let w = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            if erg.richtung.start_ist_rechts() {
                erg.fuellung_rechts = w;
                erg.fuellung_prozent[1] = prozent;
            } else {
                erg.fuellung_links = w;
                erg.fuellung_prozent[3] = prozent;
            }
        }
        Eigenschaft::FuellungInnenEnde => {
            let prozent = prozent_wert(&w);
            let w = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            if erg.richtung.start_ist_rechts() {
                erg.fuellung_links = w;
                erg.fuellung_prozent[3] = prozent;
            } else {
                erg.fuellung_rechts = w;
                erg.fuellung_prozent[1] = prozent;
            }
        }
        Eigenschaft::FuellungOben => {
            erg.fuellung_prozent[0] = prozent_wert(&w);
            erg.fuellung_oben = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::FuellungRechts => {
            erg.fuellung_prozent[1] = prozent_wert(&w);
            erg.fuellung_rechts = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::FuellungUnten => {
            erg.fuellung_prozent[2] = prozent_wert(&w);
            erg.fuellung_unten = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::FuellungLinks => {
            erg.fuellung_prozent[3] = prozent_wert(&w);
            erg.fuellung_links = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        // CSS 2.1 §17.6: `border-spacing: [<h>] [<v>]`, beide **nichtnegativ**
        // (§17.6: „Negative values are invalid"). `fuell_wert` kappt bei 0 —
        // das ist hier genau richtig, denn `border-spacing` kann nicht negativ
        // sein und ein Kappen ist besser als eine erfundene negative Zahl.
        //
        // Der `%`-Anteil wird bewusst nicht umgerechnet: die Kaskade kennt den
        // umgebenden Block nicht, und ein geratener Pixelwert waere eine
        // erfundene Zahl. `fuell_wert` gibt dafuer 0.
        Eigenschaft::RandAbstandWaagerecht => {
            erg.randabstand_waagerecht = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::RandAbstandSenkrecht => {
            erg.randabstand_senkrecht = fuell_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::RahmenObenBreite => {
            erg.rahmen_oben_breite = breite_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::RahmenRechtsBreite => {
            erg.rahmen_rechts_breite = breite_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::RahmenUntenBreite => {
            erg.rahmen_unten_breite = breite_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::RahmenLinksBreite => {
            erg.rahmen_links_breite = breite_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
        }
        Eigenschaft::RahmenObenStil => erg.rahmen_oben_stil = stil_wert(&w),
        Eigenschaft::RahmenRechtsStil => erg.rahmen_rechts_stil = stil_wert(&w),
        Eigenschaft::RahmenUntenStil => erg.rahmen_unten_stil = stil_wert(&w),
        Eigenschaft::RahmenLinksStil => erg.rahmen_links_stil = stil_wert(&w),
        Eigenschaft::RahmenObenFarbe => erg.rahmen_oben_farbe = rahmenfarbe(&w, erg.farbe),
        Eigenschaft::RahmenRechtsFarbe => erg.rahmen_rechts_farbe = rahmenfarbe(&w, erg.farbe),
        Eigenschaft::RahmenUntenFarbe => erg.rahmen_unten_farbe = rahmenfarbe(&w, erg.farbe),
        Eigenschaft::RahmenLinksFarbe => erg.rahmen_links_farbe = rahmenfarbe(&w, erg.farbe),
        // K7: `width: max-content` und Geschwister (CSS Sizing 3 §2.1)
        // sind AUFTRAGE an die Messung der Inhaltsbreite, keine
        // Laengen. Sie landen in `breite_art`; die Laenge bleibt
        // `None`, denn eine Zahl zu raten waere geraten und wuerde
        // einen Kasten so breit machen wie seine Laenge statt wie
        // seinen Inhalt.
        //
        // GEMESSENER FEHLER 01.10.2026: `Wert::Wort("max-content")` kam
        // hier an, `breiten_wert` gab None zurueck, und damit war die
        // Angage gelesen und weg.
        Eigenschaft::Breite => {
            erg.breite = breiten_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            // P19: die Einheit daneben (19:46).
            erg.breite_prozent = prozent_wert(&w);
            if let Wert::Wort(x) = &w {
                if matches!(
                    x.as_str(),
                    "max-content" | "min-content" | "fit-content" | "stretch"
                ) {
                    erg.breite_art = x.clone();
                }
            }
        }
        // K7: `max-width: none` (CSS Sizing 3 §2.1) heisst „keine
        // Grenze" und ist `None` — NICHT 0. Der Unterschied ist nicht
        // kosmetisch: 0 px als Grenze wuerde jeden Breiten-Schrumpf
        // auf 0 ziehen.
        //
        // Der Parser liefert fuer `none` eine 0-px-Laenge, weil er die
        // Form liefert und nicht die Bedeutung. Die Umdeutung steht hier,
        // weil die Kaskade die Bedeutung kennt.
        Eigenschaft::MaxBreite => {
            erg.max_breite = if ist_none(&w) {
                None
            } else {
                breiten_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
            };
            // P19: die Einheit daneben (19:46).
            erg.max_breite_prozent = prozent_wert(&w);
        }
        Eigenschaft::MinBreite => {
            erg.min_breite = breiten_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            erg.min_breite_prozent = prozent_wert(&w);
        }
        Eigenschaft::Hoehe => {
            erg.hoehe = breiten_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            // P19: die Einheit daneben (19:46).
            erg.hoehe_prozent = prozent_wert(&w);
            // Release 11.10.2026 (Agent CSS2): siehe `Berechnet::hoehe_inhalt`.
            erg.hoehe_inhalt =
                matches!(&w, Wert::Wort(x) if matches!(x.as_str(), "max-content" | "min-content" | "fit-content"));
        }
        // min/max-height: dieselbe Rechnung wie die Breite, aber die
        // Prozentbasis ist die ANSCHTSHOEHE, nicht die Breite.
        //
        // GEMESSENE LUECKE 01.10.2026: die beiden Felder gab es nicht,
        // obwohl `@media (min-height: 600px)` sie bereits auswertete.
        // `min-height: 100vh` (jede klebende Kopfzeile) kam nirgends an.
        //
        // `%` bleibt dabei auf der Ansichtsbreite — eine Breite kann
        // man nur in Prozent der Breite lesen, und die Ansichtsbreite
        // ist das, was die Kaskade kennt. Das ist eine DOKUMENTIERTE
        // Naeherung, kein Zufall.
        Eigenschaft::MinHoehe => {
            erg.min_hoehe = breiten_wert(&w, em, rem_basis, ansicht, ansicht_hoehe);
            erg.min_hoehe_prozent = prozent_wert(&w);
        }
        // Dasselbe fuer `max-height: none` (CSS Sizing 3 §2.1).
        Eigenschaft::MaxHoehe => {
            erg.max_hoehe = if ist_none(&w) {
                None
            } else {
                breiten_wert(&w, em, rem_basis, ansicht, ansicht_hoehe)
            };
            // P19: die Einheit daneben (19:46).
            erg.max_hoehe_prozent = prozent_wert(&w);
        }
        Eigenschaft::Listenstil => {
            if let Wert::Wort(w) = &w {
                erg.listenstil = match w.as_str() {
                    "circle" => Liste::Kreis,
                    "square" => Liste::Quadrat,
                    "decimal" => Liste::Dezimal,
                    "lower-roman" => Liste::Roemisch,
                    "upper-roman" => Liste::RoemischGross,
                    "lower-alpha" | "lower-latin" => Liste::Alpha,
                    "upper-alpha" | "upper-latin" => Liste::AlphaGross,
                    "none" => Liste::Keins,
                    // K19: die Skript-Ziffernsysteme aus Counter Styles 3
                    // §5. Sie sind gueltige Werte und landen deshalb in
                    // `Skript` statt im Verworfen — die Ziffern selbst
                    // zeichnet `narsil-text`, das ist eine fremde Crate.
                    "arabic-indic"
                    | "bengali"
                    | "cambodian"
                    | "khmer"
                    | "devanagari"
                    | "gujarati"
                    | "gurmukhi"
                    | "kannada"
                    | "lao"
                    | "malayalam"
                    | "mongolian"
                    | "myanmar"
                    | "oriya"
                    | "persian"
                    | "tamil"
                    | "telugu"
                    | "thai"
                    | "tibetan"
                    | "hebrew"
                    | "cjk-ideographic"
                    | "armenian"
                    | "georgian"
                    | "lower-greek"
                    | "decimal-leading-zero" => Liste::Skript,
                    _ => Liste::Scheibe,
                };
            }
        }
        Eigenschaft::Sichtbarkeit => {
            if let Wert::Wort(w) = &w {
                erg.sichtbar = w != "versteckt" && w != "zusammen";
                // Release 14.10.2026 (Agent CSS2): a collapsed table row or
                // row group takes no space (CSS 2.1 §17.5.5); approximated by
                // removing its box (the column widths then ignore its cells).
                // MEASURED before (WPT `CSS2/tables/row-visibility-001`): the
                // hidden row kept its height.
                if w == "zusammen" && erg.anzeige == Anzeige::TabelleZeile {
                    erg.anzeige = Anzeige::Keins;
                }
            }
        }
        Eigenschaft::Ueberlauf => {
            if let Wert::Wort(w) = &w {
                erg.ueberlauf = match w.as_str() {
                    "versteckt" => Ueberlauf::Versteckt,
                    "scroll" => Ueberlauf::Scroll,
                    "auto" => Ueberlauf::Auto,
                    _ => Ueberlauf::Sichtbar,
                };
            }
        }
        Eigenschaft::FlexRichtung => {
            if let Wert::Wort(w) = &w {
                erg.flex_richtung = match w.as_str() {
                    "spalte" => FlexRichtung::Spalte,
                    // Release 09.10.2026: die Umkehrungen kamen vom Parser
                    // („reihe-umgekehrt", „spalte-umgekehrt"), fielen hier
                    // aber in den Notfallzweig `Reihe`.
                    "reihe-umgekehrt" => FlexRichtung::ReiheUmgekehrt,
                    "spalte-umgekehrt" => FlexRichtung::SpalteUmgekehrt,
                    _ => FlexRichtung::Reihe,
                };
            }
        }
        Eigenschaft::FlexBuendig => {
            if let Wert::Wort(w) = &w {
                erg.flex_buendig = match w.as_str() {
                    "mitte" => FlexBuendig::Mitte,
                    "ende" => FlexBuendig::Ende,
                    "zwischen" => FlexBuendig::Zwischen,
                    "ringsum" => FlexBuendig::Ringsum,
                    "gleichmaessig" => FlexBuendig::Gleichmaessig,
                    // `stretch` (CSS Box Alignment 3 §8.3). Der Parser
                    // liefert seit K7c „strecke"; ohne diesen Arm fiel
                    // es in den Notfallzweig `Start`. Der Test
                    // `flex_buendig_alle` hat es gefangen.
                    "strecke" => FlexBuendig::Strecke,
                    // `left`/`right` sind PHYSISCH (§8.2) und kommen
                    // ebenfalls als logische Woerter an.
                    "links" => FlexBuendig::Links,
                    "rechts" => FlexBuendig::Rechts,
                    _ => FlexBuendig::Start,
                };
            }
        }
        Eigenschaft::FlexAusrichtung => {
            if let Wert::Wort(w) = &w {
                erg.flex_ausrichtung = match w.as_str() {
                    "start" => FlexAusrichtung::Start,
                    "mitte" => FlexAusrichtung::Mitte,
                    "ende" => FlexAusrichtung::Ende,
                    // L4 (13:5x): §6.4 — `baseline` ist ein eigener Wert
                    // und wurde vorher von `_` zu `Strecke` verschluckt.
                    // GEMESSENER BEWEIS: der Test
                    // `l4_baseline_bleibt_baseline_im_plan` sah links
                    // `Strecke`, rechts `Grundlinie`.
                    "grundlinie" => FlexAusrichtung::Grundlinie,
                    _ => FlexAusrichtung::Strecke,
                };
            }
        }
        // L3 (02.10.2026 04:5x): `gap` ist eine KURZFORM fuer BEIDE
        // Achsen.
        //
        // CSS Grid 1 §10.1 woertlich: gap setzt row-gap und column-gap
        // gemeinsam. Vorher schrieb der Arm nur `luecke_px`, und
        // `row-gap`/`column-gap` schrieben `luecke_zeile`/
        // `luecke_spalte`. Drei Felder, die nie zusammenfanden: der
        // Plan sah nur `luecke_px` und kannte `row-gap: 20px` also
        // nicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `luecke_px` MITgeschrieben wird
        // und nicht ersetzt: `LayoutStil::luecke` liest es, und davon
        // leben Raster, Flex und drei Messprogramme. `gap: 20px` soll
        // dort 20 bleiben — das ist die gemessene Bedeutung heute. Die
        // beiden Felder kommen dazu, nicht darum herum.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Kurzform die Kaskaden-
        // Reihenfolge richtig abbildet: Beide Felder werden beim
        // Anwenden der Regel gesetzt, und die Kaskade wendet in ihrer
        // Reihenfolge an. `div{gap:10px; row-gap:20px}` laesst also
        // `row-gap` gewinnen, `div{row-gap:20px; gap:10px}` umgekehrt —
        // genau die Kurzform-Regel, ohne Sonderfall.
        Eigenschaft::Luecke => match &w {
            // gap-%: Basis ist der Container (hier die Ansicht — dokumentiert).
            Wert::Laenge(l) => {
                let p =
                    laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX);
                erg.luecke_px = p;
                erg.luecke_spalte = p;
                erg.luecke_zeile = p;
            }
            _ => {}
        },
        Eigenschaft::FlexWachstum => match &w {
            Wert::Laenge(l) => {
                use super::typen::Einheit::Zahl;
                if matches!(l.einheit, Zahl) {
                    // Faktor, kein px-Wert: 0 bis 1 Mio (darueber Bombe).
                    erg.flex_wachstum = l.wert.clamp(0.0, 1_000_000.0);
                }
            }
            _ => {}
        },
        Eigenschaft::FlexSchrumpf => match &w {
            Wert::Laenge(l) => {
                use super::typen::Einheit::Zahl;
                if matches!(l.einheit, Zahl) {
                    erg.flex_schrumpf = l.wert.clamp(0.0, 1_000_000.0);
                }
            }
            _ => {}
        },
        Eigenschaft::FlexBasis => match &w {
            // auto -> None (Inhalt entscheidet, siehe Layout).
            Wert::Auto => {
                erg.flex_basis = None;
                erg.flex_basis_art = narsil_layout::flexstil::FlexBasis::Auto;
            }
            Wert::Laenge(l) => {
                // %-Basis ist der Container (hier die Ansicht — dokumentiert).
                erg.flex_basis = Some(
                    laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX),
                );
                // Release 11.10.2026: fuer den neuen Flex-Algorithmus bleibt
                // ein Prozentwert Prozent (gegen die innere Hauptgroesse des
                // Containers, CSS Flexbox 1 §7.3.3). GEMESSEN vorher: WPT
                // `flexbox_flex-0-0-Npercent*` (Prozent gegen die Ansicht).
                erg.flex_basis_art = narsil_layout::flexstil::FlexBasis::Laenge(if matches!(l.einheit, super::typen::Einheit::Prozent) {
                    narsil_layout::gitter::LaengeProzent { px: 0.0, prozent: l.wert.max(0.0), hat_prozent: true }
                } else {
                    narsil_layout::gitter::LaengeProzent::px(laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX))
                });
            }
            // Release 11.10.2026: `content` und die Groessen-Schluesselwoerter.
            Wert::Wort(x) => {
                use narsil_layout::flexstil::FlexBasis as B;
                erg.flex_basis = None;
                erg.flex_basis_art = match x.as_str() {
                    "min-content" => B::MinInhalt,
                    "max-content" => B::MaxInhalt,
                    "fit-content" => B::Passend,
                    _ => B::Inhalt,
                };
            }
            _ => {}
        },
        Eigenschaft::FlexUmbruch => {
            // (Die TEMPORAERE Messung fuer diesen Punkt sitzt in
            // `layout_plan.rs` — hier kommt die Deklaration an, `Wort("wrap")`
            // 52-mal in `examples/l3_flex.rs`. Der Verlust ist danach.)
            if let Wert::Wort(x) = &w {
                erg.flex_umbruch = x == "wrap";
                // Release 11.10.2026: `wrap-reverse` fuer den neuen
                // Flex-Algorithmus (CSS Flexbox 1 §5.2).
                erg.flex_umbruch_umgekehrt = x.starts_with("wrap-reverse");
                erg.flex_umbruch_ausgleich = x.ends_with("balance");
            }
        }
        // C13 (W6D): Positionierung. Offsets: auto -> None, Laenge
        // (% gegen Ansicht, negativ erlaubt); z-index: Zahl -> i32.
        Eigenschaft::Position => {
            if let Wert::Wort(x) = &w {
                erg.position = match x.as_str() {
                    "relative" => Position::Relativ,
                    "absolute" => Position::Absolut,
                    "fixed" => Position::Fest,
                    "sticky" => Position::Klebend,
                    _ => Position::Statisch,
                };
            }
        }
        // CSS 2.1 §10.3.7 und CSS Position 3 §5.1: **die waagerechte
        // %-Basis ist die Breite, die senkrechte die Hoehe** — deswegen
        // bekommt `oben`/`unten` die AnsichtsHOEHE und `links`/`rechts` die
        // AnsichtsBREITE.
        //
        // GEMESSENER FEHLER (03.10.2026, Rolle CSS; BRETT 20:0x von CODEX):
        // hier stand fuer **alle vier** die AnsichtsBREITE. `top: 20%` bei
        // 1280x800 ergab 256 px statt 160 px — bei einer Ansicht, die breiter
        // ist als hoch, wandert jedes senkrechte Prozent zu weit nach unten.
        // Auf www.wikipedia.org sind das die Sprachlinks
        // (`.central-featured-lang`, `top: 20% / right: 70%` in einem 520 px
        // hohen `.central-featured`), und ihre Ueberlappung in
        // `team/log/leitung_vergleich_wikipedia_1921.png` ist genau das.
        //
        // Die **enthaltende Box** kennt die Kaskade nicht; die Ansicht ist die
        // dokumentierte Naeherung (siehe `laenge_px`). Sie war nur auf der
        // falschen Achse — die Breite fuer `top` war nie eine Naeherung,
        // sondern ein Fehler.
        //
        // `ansichts_hoehe()` (0 = unbekannt -> Breite) ist dieselbe
        // Normalisierung, die `RechenCtx` und die anderen Prozent-Arme
        // benutzen; ohne sie waere `top: 20%` bei unbekannter Hoehe 0.
        Eigenschaft::Oben => {
            erg.oben_prozent = prozent_wert(&w);
            erg.oben = versatz_wert(
                &w,
                em,
                rem_basis,
                super::rechnen::ansichts_hoehe(ansicht, ansicht_hoehe),
                ansicht,
                ansicht_hoehe,
            )
        }
        Eigenschaft::Rechts => {
            erg.rechts_prozent = prozent_wert(&w);
            erg.rechts = versatz_wert(&w, em, rem_basis, ansicht, ansicht, ansicht_hoehe)
        }
        Eigenschaft::Unten => {
            erg.unten_prozent = prozent_wert(&w);
            erg.unten = versatz_wert(
                &w,
                em,
                rem_basis,
                super::rechnen::ansichts_hoehe(ansicht, ansicht_hoehe),
                ansicht,
                ansicht_hoehe,
            )
        }
        Eigenschaft::Links => {
            erg.links_prozent = prozent_wert(&w);
            erg.links = versatz_wert(&w, em, rem_basis, ansicht, ansicht, ansicht_hoehe)
        }
        Eigenschaft::ZIndex => match &w {
            Wert::Auto => erg.z_index = None,
            Wert::Laenge(l) => {
                use super::typen::Einheit::Zahl;
                if matches!(l.einheit, Zahl) {
                    erg.z_index = Some(l.wert.clamp(-1_000_000.0, 1_000_000.0).round() as i32);
                }
            }
            _ => {}
        },
        // C14 (W6D): float/clear.
        Eigenschaft::Schweben => {
            if let Wert::Wort(x) = &w {
                erg.schweben = match x.as_str() {
                    "links" => Float::Links,
                    "rechts" => Float::Rechts,
                    _ => Float::Keins,
                };
            }
        }
        Eigenschaft::Raeumung => {
            if let Wert::Wort(x) = &w {
                erg.raeumung = match x.as_str() {
                    "links" => Raeumung::Links,
                    "rechts" => Raeumung::Rechts,
                    "beide" => Raeumung::Beide,
                    _ => Raeumung::Keins,
                };
            }
        }
        // C15 (W6D): Spalten mit Element-Kontext aufloesen (em: eigene
        // Schrift, %: Ansicht — dokumentiert wie Breiten).
        Eigenschaft::RasterSpalten => {
            if let Wert::Wort(x) = &w {
                if let Some(sp) = crate::parse::raster_spalten(x, em, ansicht, ansicht) {
                    // L4 (03.10.2026 06:1x): die LINIENNAMEN kommen mit
                    // (§7.2). GEMESSENE ENTSCHEIDUNG, warum sie hier
                    // landen und nicht spaeter: `raster_spalten` hat sie
                    // gelesen, und wegwerfen koennte man sie nur noch,
                    // indem man den Wert erneut zerlegt.
                    erg.raster_spalten = sp.spuren;
                    erg.raster_spalten_namen = sp.namen;
                    // L4 (07:0x): `subgrid` (§7.2). Die Liste ist dann
                    // leer, und das ist KEIN Fehler — siehe
                    // `raster_subgrid_spalten`.
                    erg.raster_subgrid_spalten = sp.subgrid;
                }
            }
        }
        // L4 (02.10.2026 18:2x): `grid-template-rows`, getrennt von den
        // Spalten.
        //
        // GEMESSENER BEFUND, was vorher passierte: `grid-template-rows`
        // ging auf `ERaster`, und das ist ein Alias fuer
        // `RasterSpalten` — die Zeilen landeten in den SPALTENbreiten.
        // GEMESSEN: `grid-template-rows: 200px 200px` allein ergab
        // `[Fest(200), Fest(200)]` als Spalten.
        //
        // GEMESSENE ENTSCHEIDUNG, warum derselbe Aufruf
        // `raster_spalten` genutzt wird und nicht eine eigene Funktion:
        // §8.2 und §8.3 haben dieselbe Syntax (`<track-list>`) und
        // dieselben Mass-Einheiten; der Unterschied ist §8.3 gegen §8.2,
        // also die ACHSE, nicht der Wert. Eine zweite Funktion waere
        // derselbe Code an zwei Stellen.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Aufloesung gegen `ansicht`
        // laeuft und nicht gegen `ansicht_hoehe`: Spaltenbreiten loesen
        // `fr` und `%` gegen die Breite auf (CSS Grid 1 §10.1). Fuer
        // Zeilen gilt dasselbe fuer die Breite ihrer Spuren — die
        // Zeilenhoehe selbst kommt aus §8.3 und wird heute noch nicht
        // gerechnet.
        Eigenschaft::RasterZeilen => {
            if let Wert::Wort(x) = &w {
                if let Some(sp) = crate::parse::raster_spalten(x, em, ansicht, ansicht) {
                    erg.raster_zeilen = sp.spuren;
                    // L4 (06:1x): dieselbe Form an der Zeilenachse — und
                    // die Zeilenliste traegt ihre EIGENEN Namen. Ein Name
                    // der Spaltenachse gehoert nicht hierher (§8.3: die
                    // Suche laeuft pro Achse).
                    erg.raster_zeilen_namen = sp.namen;
                    // L4 (07:0x): dieselbe Form an der Zeilenachse.
                    erg.raster_subgrid_zeilen = sp.subgrid;
                }
            }
        }
        // L4 (02.10.2026 21:5x): `grid-auto-rows` (§7.5) und
        // `grid-auto-columns` (§7.3).
        //
        // GEMESSENE ENTSCHEIDUNG, warum derselbe Aufruf
        // `raster_spalten` genutzt wird: §7.3 und §7.5 nennen fuer die
        // impliziten Spuren dieselbe Form `<track-size>` wie §8.2 und
        // §8.3 fuer die Vorlagen. Es ist die Form, nicht die Achse, die
        // der Parser braucht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `ansicht` (die Breite) fuer
        // BEIDE gilt und nicht `ansicht_hoehe` fuer die Zeilen: eine
        // Spur loest `fr` und `%` gegen die Containerbreite auf
        // (CSS Grid 1 §10.1), und die Zeilenhoehe selbst kommt aus §7.5,
        // nicht aus der Vorlage. Fuer die Zeilen ist die Breite also die
        // richtige Aufloesung.
        Eigenschaft::RasterAutoZeilen => {
            if let Wert::Wort(x) = &w {
                if let Some(sp) = crate::parse::raster_spalten(x, em, ansicht, ansicht) {
                    // GEMESSENE ENTSCHEIDUNG, warum die Namen hier
                    // verworfen werden: §7.5 nennt fuer `grid-auto-rows`
                    // `<track-size>*` — ohne `<line-names>`. Es gibt also
                    // keine Namen auf dieser Achse, und sie zu erfinden
                    // waere eine Zahl, die im CSS nicht steht.
                    erg.raster_auto_zeilen = sp.spuren;
                }
            }
        }
        Eigenschaft::RasterAutoSpalten => {
            if let Wert::Wort(x) = &w {
                if let Some(sp) = crate::parse::raster_spalten(x, em, ansicht, ansicht) {
                    erg.raster_auto_spalten = sp.spuren;
                }
            }
        }
        // K1, 01.10.2026: `background-image: none` loescht den Verlauf
        // (das ist der Anfangswert), ein Verlauf setzt ihn. `url(...)`
        // kommt hier nicht an — der Parser laesst es weg, statt eine
        // Farbe zu erfinden.
        // K1 stellte hier nur den Verlauf; K7 ergaenzt die URL. Beide
        // gehen in `Berechnet::bild` — EIN Feld, weil `background-image`
        // nur EINE Quelle haben kann (CSS Backgrounds 3 §3.1). Das
        // Feld `verlauf` bleibt als Sicht auf dasselbe, damit kein
        // Aufrufer nachbrechen muss.
        Eigenschaft::HintergrundBild => match &w {
            Wert::Verlauf(v) => {
                erg.bild = BildQuelle::Verlauf(v.clone());
                erg.verlauf = Some(v.clone());
            }
            Wert::Url(u) => {
                erg.bild = BildQuelle::Url(u.clone());
                erg.verlauf = None;
            }
            Wert::Wort(x) if x == "none" => {
                erg.bild = BildQuelle::Keins;
                erg.verlauf = None;
            }
            _ => {}
        },
        // ---- K6: DIE NEUEN EIGENSCHAFTEN ----
        //
        // `border-radius` ohne Eckenname gilt fuer alle vier Ecken
        // (CSS Backgrounds 3, §5.4); die Ecken-Langformen sind
        // oben auf denselben Wert abgebildet, deshalb steht hier nur
        // EINE Auswertung fuer alle vier.
        // Die Reihenfolge in `Berechnet::radius` ist oben-links,
        // oben-rechts, unten-rechts, unten-links — die Reihenfolge
        // von `border-radius` eins nach zwei (CSS Backgrounds 3 §5.4).
        //
        // `border-radius` (ohne Eckenname) setzt ALLE VIER, die
        // Ecke setzt genau eine (§5.1). Deshalb sind es zwei
        // Eigenschaften und nicht eine.
        Eigenschaft::RadiusObenLinks
        | Eigenschaft::RadiusObenRechts
        | Eigenschaft::RadiusUntenLinks
        | Eigenschaft::RadiusUntenRechts => {
            let stelle = match e {
                Eigenschaft::RadiusObenLinks => 0,
                Eigenschaft::RadiusObenRechts => 1,
                Eigenschaft::RadiusUntenRechts => 2,
                _ => 3,
            };
            // Release 15.10.2026 (Agent F): the radii for elliptical corners
            // (percentages stay percentages of the border box).
            let als_wert = |l: &Laenge| -> (f32, bool) {
                if l.einheit == super::typen::Einheit::Prozent {
                    (l.wert.clamp(0.0, 1.0e6), true)
                } else {
                    (laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX), false)
                }
            };
            let paar = match &w {
                Wert::Laenge(l) => Some((*l, *l)),
                Wert::Radius(h, v) => Some((*h, *v)),
                _ => None,
            };
            if let Some((h, v)) = paar {
                // The circular approximation: the smaller radius of one unit,
                // otherwise the horizontal one.
                let kreis = if v.einheit == h.einheit && v.wert < h.wert { v } else { h };
                erg.radius[stelle] =
                    laenge_px(&kreis, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX);
                let ((hx, hp), (vy, vp)) = (als_wert(&h), als_wert(&v));
                erg.radius_xy[stelle] = [hx, vy];
                let bits = (1u8 << stelle) | (1u8 << (4 + stelle));
                erg.radius_percent &= !bits;
                if hp {
                    erg.radius_percent |= 1 << stelle;
                }
                if vp {
                    erg.radius_percent |= 1 << (4 + stelle);
                }
            }
        }
        // CSS Color 4, §3.2: `opacity` ist eine Zahl 0-1 und wird
        // geklemmt, nicht abgelehnt (`opacity: 2` heisst 1).
        Eigenschaft::Deckkraft => {
            if let Wert::Laenge(l) = &w {
                erg.deckkraft = l.wert.clamp(0.0, 1.0);
            }
        }
        // CSS Backgrounds 3, §9: `none` leert die Liste (und das ist
        // der Anfangswert — darum hier NICHHT vererbt).
        Eigenschaft::Schatten => match &w {
            // Release 14.10.2026 (Agent CSS2): units against the element's
            // font, `currentColor` as the element's colour. MEASURED before
            // (WPT css-backgrounds `box-shadow-overlapping-001`, `box-shadow:
            // -3em 0em` without a colour): 3 px to the left, black.
            Wert::Schatten(l) => {
                erg.schatten = l.clone();
                for s in erg.schatten.iter_mut() {
                    let px = |l: &Laenge| laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(-MAX_PX, MAX_PX);
                    s.x = px(&s.raw_lengths[0]);
                    s.y = px(&s.raw_lengths[1]);
                    s.weich = px(&s.raw_lengths[2]).max(0.0);
                    s.ausbreitung = px(&s.raw_lengths[3]);
                    if s.current_color {
                        s.farbe = erg.farbe;
                    }
                }
            }
            Wert::Wort(x) if x == "none" => erg.schatten.clear(),
            _ => {}
        },
        Eigenschaft::OutlineBreite => {
            if let Wert::Laenge(l) = &w {
                erg.outline_breite =
                    laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX);
            }
        }
        // `currentColor` heisst: die eigene Textfarbe (CSS UI 4 §6.1).
        Eigenschaft::OutlineFarbe => erg.outline_farbe = rahmenfarbe(&w, erg.farbe),
        // CSS UI 4, §6.2: der Abstand des Rings vom Rand. Fuer das
        // Layout ohne Bedeutung, deshalb wird er gemerkt und nicht
        // gerechnet — der Rasterizer zieht ihn vom Ring ab.
        Eigenschaft::OutlineOffset => {
            if let Wert::Laenge(l) = &w {
                let _ = laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe);
            }
        }
        // CSS Inline 3 §6: Schluesselwort ODER Laenge. Die Laenge wird
        // als TEXT gemerkt, nicht in px umgerechnet — die Verschiebung
        // der Inline-Box gehoert dem Textfluss, und `em` bezieht sich
        // dort auf die Schrift der Box, nicht auf die des Eltern.
        //
        // GEMESSENER FEHLER 01.10.2026: hier stand nur der Wort-Zweig,
        // und `vertical-align: -1px` (3x) sowie `-.075em` (1x) auf den
        // 35 echten Stylesheets fielen weg.
        Eigenschaft::VertikalAlign => match &w {
            Wert::Wort(x) => erg.vertikal = x.clone(),
            Wert::Laenge(_) => {
                // Der Rohtext steht in `d.roh`; er ist die getreueste
                // Form (`-.075em` wird nicht zu `-0.075em`).
                let roh = d.roh.trim();
                erg.vertikal = String::from(if roh.is_empty() { "" } else { roh });
            }
            _ => {}
        },
        // ---- K7c (01.10.2026): die 27 neuen Eigenschaften ----
        // CSS Text 4 §4.1: eine Laenge, 0 = `normal`.
        // Release 12.10.2026 (Agent CSS2): Prozent gegen die eigene Schrift,
        // vererbt als Prozent (`buchstaben_prozent`). GEMESSEN vorher (WPT
        // `css-text/letter-spacing/letter-spacing-percent-001`): `10%` waren
        // 10 % der Ansichtsbreite (80 px je Zeichen).
        Eigenschaft::LetterSpacing => {
            if let Wert::Laenge(l) = &w {
                erg.buchstaben_abstand =
                    laenge_px(l, em, rem_basis, em, ansicht, ansicht_hoehe)
                        .clamp(-MAX_PX, MAX_PX);
                erg.buchstaben_prozent =
                    (l.einheit == super::typen::Einheit::Prozent && l.wert.is_finite()).then_some(l.wert);
            }
        }
        // CSS Text 4 §4.2.
        Eigenschaft::Wortabstand => {
            if let Wert::Laenge(l) = &w {
                erg.wort_abstand = laenge_px(l, em, rem_basis, em, ansicht, ansicht_hoehe)
                    .clamp(-MAX_PX, MAX_PX);
                erg.wort_prozent =
                    (l.einheit == super::typen::Einheit::Prozent && l.wert.is_finite()).then_some(l.wert);
            }
        }
        // CSS Text 4 §3.1: die erste Zeile einruecken. Positiv =
        // rechts, negativ = links (hanging indent).
        Eigenschaft::TextEinzug => {
            if let Wert::Laenge(l) = &w {
                erg.text_einzug = laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe)
                    .clamp(-MAX_PX, MAX_PX);
                // Release 11.10.2026 (Agent CSS2): siehe `text_einzug_prozent`.
                erg.text_einzug_prozent = (l.einheit == super::typen::Einheit::Prozent && l.wert.is_finite())
                    .then_some(l.wert);
                // Release 12.10.2026 (Agent CSS2): der Prozentanteil rechnet der
                // Seitenbau (`text_einzug + prozent * Breite`), die Pixel hier
                // sind dann nur der feste Anteil — bei `calc(50% - 3px)` beide
                // (`misch_anteile`). GEMESSEN vorher (WPT `css-values/calc-text-
                // indent-1`): der Prozentanteil galt gegen die Ansicht.
                if erg.text_einzug_prozent.is_some() {
                    erg.text_einzug = 0.0;
                } else if let Some((px, prozent)) = misch_anteile(&d.roh, &ctx) {
                    erg.text_einzug = px.clamp(-MAX_PX, MAX_PX);
                    erg.text_einzug_prozent = Some(prozent);
                }
            }
        }
        // CSS Text 4 §8.2: Zahl (Tabulatorstopps) ODER Laenge.
        Eigenschaft::Tabulator => {
            if let Wert::Laenge(l) = &w {
                erg.tabulator_groesse = if l.einheit == super::typen::Einheit::Zahl {
                    l.wert.clamp(0.0, 1000.0)
                } else {
                    laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX)
                };
                erg.tabulator_px = l.einheit != super::typen::Einheit::Zahl;
            }
        }
        // CSS Text 4 §5.
        Eigenschaft::TextTransform => {
            if let Wert::Wort(x) = &w {
                erg.text_transform = x.clone();
            }
        }
        // CSS Text 4 §4.3.
        Eigenschaft::TextUeberlauf => {
            if let Wert::Wort(x) = &w {
                erg.text_ueberlauf = x.clone();
            }
        }
        // CSS Text 4 §4.4. `overflow-wrap` und `word-wrap` sind
        // laut Anmerkung dort GLEICHWERTIG und teilen ein Feld.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): `word-wrap` stand als
        // eigener Name in der Liste, `overflow-wrap` nicht — beide
        // gingen verloren.
        Eigenschaft::WortUmbruch => {
            if let Wert::Wort(x) = &w {
                erg.wort_umbruch = x.clone();
            }
        }
        // CSS Text 4 §4.5.
        Eigenschaft::WortTrennung => {
            if let Wert::Wort(x) = &w {
                erg.wort_trennung = x.clone();
            }
        }
        // CSS Text 4 §6: `line-break` und `hyphens` sind beide
        // Zeilenumbruch-Regeln und teilen deshalb ein Feld.
        Eigenschaft::ZeilenTrennung => {
            if let Wert::Wort(x) = &w {
                erg.zeilen_trennung = x.clone();
            }
        }
        //
        // L4 (02.10.2026 23:4x): `span N` ist KEINE Linie. GEMESSENER
        // BEFUND: `gitter_linie` gab fuer `span 2` die `2` zurueck, und
        // mein `spanne(start, ende)` in `layout_plan.rs` rechnete
        // `ende - start` — bei `grid-row: span 2` ist `ende == 0`
        // (auto), also `0 - 2` und `.max(1)` = **1**. GEMESSEN ueber den
        // Seitenweg (`examples/l4_raster.rs`, Block O): O2 sah aus wie
        // O1, vier Kaesten a 50 px.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ein eigenes Feld und keine
        // Unterscheidung in `spanne()`: `grid-row-start: 2` (Linie 2) und
        // `grid-row-start: span 2` (Spannweite 2) ergeben dieselbe Zahl.
        // Aus der Zahl allein ist die Form nicht erkennbar — also muss
        // die Form beim Lesen unterschieden werden, und das gehoert an
        // die Stelle, die den Rohwert sieht.
        // L4 (03.10.2026 00:4x): §8.4 — `<grid-line> [ / <grid-line> ]?`.
        //
        // GEMESSENER BEFUND, was vorher hier stand: der ganze Wert ging an
        // `gitter_spanne`, und bei `auto/span 12` ist das weder eine
        // Spannweite noch eine Linie — `strip_prefix("span")` scheitert
        // am `auto`, und `parse::<i32>()` scheitert am Rest. Die
        // Deklaration fiel still. GEMESSEN in
        // `tests/position.rs::l4_zweiteilige_platzierung_kommt_an`:
        // `left: 0` statt 12, bei **506 von 532** Vorkommen im Korpus.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Teilung eine eigene Funktion
        // ist und kein `split('/')`: `minmax(min(100px, 200px), 1fr)`
        // traegt ein `/` in der Klammer, und `grid-area` hat drei. Ein
        // Zerlegen an JEDEM `/` wuerde die verschachtelte Form zerlegen.
        // Derselbe Tiefenzaehler wie `komma_0` (parse.rs, 21:7x), nur
        // fuer `/` statt fuer `,`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der ENDE-Teil auch eine
        // Spannweite setzen darf: §8.3 — „if the end of the grid area is
        // a span, the grid item spans that many tracks". Das ist
        // `grid-row: 2/span 2`, und es ist im Korpus in 12 Faellen
        // gemessen. Also wird der zweite Teil genauso gelesen wie der
        // erste, mit DEM Unterschied, dass er nie die Startlinie setzt.
        Eigenschaft::GitterZeileStart => {
            let roh = d.roh.to_ascii_lowercase();
            let teile = gitter_teile(&roh);
            if let Some(v) = teile.first() {
                match gitter_spanne(v) {
                    Some(sp) => erg.gitter_zeile_spanne = sp,
                    None => erg.gitter_zeile_start = gitter_linie(v),
                }
            }
            if let Some(v) = teile.get(1) {
                match gitter_spanne(v) {
                    Some(sp) => {
                        // §8.3: die Endspannweite GEWINNT gegen eine
                        // Startspannweite — sonst wuerde `2/span 2` die
                        // 2 doppelt zaehlen.
                        erg.gitter_zeile_spanne = sp.max(erg.gitter_zeile_spanne);
                    }
                    None => erg.gitter_zeile_ende = gitter_linie(v),
                }
            }
            // L4 (06:1x): §8.3 — die Namen gegen die Linien des
            // CONTAINERS. `grid-row: header` ist im Korpus 2x, und vorher
            // stand dort nur `0` (auto) plus ein `verwurf`.
            loese_liniennamen(erg, &teile, PlatzAchse::NurZeile, eltern);
        }
        Eigenschaft::GitterZeileEnde => {
            erg.gitter_zeile_ende = gitter_linie(&d.roh.to_ascii_lowercase());
            // L4 (06:1x): `grid-row-end: name` — ein Teil, Zeilenachse.
            loese_liniennamen(
                erg,
                &[&d.roh.to_ascii_lowercase()],
                PlatzAchse::ZeileEnde,
                eltern,
            );
        }
        //
        // L4 (03.10.2026 02:6x): `grid-template-areas` wird GELESEN.
        //
        // GEMESSENER BEFEUND, was vorher hier stand: der Rohwert in einem
        // String, den niemand liest (`gitter_gebiete`).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Liste in die KASKADE gehoert
        // und nicht in die Weiterverarbeitung: §8.1 ist eine
        // WERT-SYNTAX (eine Liste anfuehrungszeichenbegrenzter Zeilen),
        // und der Kommentar sagte „das gehoert in die Weiterverarbeitung".
        // GEMESSENE ENTSCHEIDUNG, warum das nicht mehr gilt: die
        // Weiterverarbeitung (Layout) kann die Anfuehrungszeichen nicht
        // mehr unterscheiden — sie bekommt `Berechnet`, und dort waere
        // beides ein String, und die Frage „wo endet eine Zeile?" waere
        // nicht mehr beantwortbar. **Ein Wert, den zwei Schritte weiter
        // gebraucht wird, gehoert an die Stelle, die ihn sieht.**
        Eigenschaft::GitterGebiete => {
            if let Wert::Wort(x) = &w {
                erg.raster_gebiete = parse_gebiete(x);
            }
        }
        // L4 (03.10.2026 02:9x): `grid-area` (§8.4).
        //
        // GEMESSENER BEFUND, was vorher hier stand: der Wert blieb in
        // `schluessel` stehen (ein Rohwert als Text), und niemand las ihn.
        // GEMESSENE ENTSCHEIDUNG, warum der Name ROH gespeichert wird und
        // nicht als `Gebiet`: die Zahlform `r/c/r/c` ist nicht gebaut, und
        // `Gebiet::set` verwirft eine Zahl still — dann waere der Zustand
        // unsichtbar. Der Rohwert macht ihn sichtbar (siehe
        // `Berechnet::grid_area_name`).
        Eigenschaft::GitterGebiet => {
            if let Wert::Wort(x) = &w {
                let roh = x.trim().to_string();
                // L4 (03.10.2026 04:8x): §8.4 — die VIERTEILIGE Form.
                //
                // GEMESSENER BEFUND, was vorher hier stand: der Rohwert
                // ging in `grid_area_name`, und `plan_gebiet`warf alles
                // ausser Buchstaben, Ziffern und `-`/`_` weg. GEMESSEN
                // also: `grid-area: 2/3/4/5` kam als `auto` an (ein leerer
                // Name trifft kein Gebiet).
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Form an DEN `/`
                // erkannt wird und nicht an der Zahl: §8.4 nennt die
                // Kurzform `<name>` ODER `<row-start> / <column-start> /
                // <row-end> / <column-end>`. Ein Name hat keinen `/`, also
                // entscheidet der Trenner — und `labels/1/labels/-1`
                // (2 Vorkommen im Korpus) hat einen `/` UND einen Namen.
                let klein = roh.to_ascii_lowercase();
                let teile = gitter_teile(&klein);
                if teile.len() >= 2 {
                    setze_grid_area_teile(erg, &teile);
                } else {
                    erg.grid_area_name = roh;
                }
                // L4 (03.10.2026 06:1x): §8.3 — der EINE Teil geht in
                // ALLE VIER Langformen, und jeder Name loest sich gegen
                // die Linien SEINER Achse.
                //
                // GEMESSEN im Korpus: `grid-area: content` 9x und
                // `grid-area: visual` 5x, und beide zusammen mit
                // `grid-template-columns:[content]1fr[visual]1fr`. Vorher
                // stand im Kind nur `grid_area_name`, und die Linien
                // kannte niemand.
                //
                // GEMESSENE ENTSCHEIDUNG, warum der eine Teil auch die
                // ENDEN setzt: §8.4 — `grid-area: foo` ist
                // `foo/foo/foo/foo`, und die Enden sind `auto`, weil die
                // Regel sie nicht nennt. Also nur die beiden Startwerte.
                loese_liniennamen(erg, &teile, PlatzAchse::Gitter, eltern);
            }
        }
        // CSS Grid 1 §7.1-§7.2.
        Eigenschaft::GitterVorlagen => {
            if let Wert::Wort(x) = &w {
                erg.gitter_vorlagen = x.clone();
            }
        }
        // CSS Grid 1 §7.4-§7.5.
        //
        // L4 (02.10.2026 11:3x): aus dem ROHWORT wird die ENTSCHEIDUNG.
        //
        // GEMESSENER BEFUND, warum `column` allein noch nichts bewegt
        // (Release, `l4_raster.rs`, 2 Spalten, 6 Kinder, vorher):
        //
        //     grid-auto-flow: row     3 Zeilen x 2 Spalten belegt
        //     grid-auto-flow: column  3 Zeilen x 2 Spalten belegt   <- gleich
        //
        // GEMESSENER GRUND, warum ein eigener Boolean und kein Lesen
        // des Rohworts weiter unten: `gitter_automatik` ist EIN Feld
        // fuer `grid-auto-flow`, `-columns` UND `-rows`. Die Rohform
        // ist damit nicht unterscheidbar — die drei ueberschreiben
        // sich. Ein Boolean beantwortet genau die eine Frage, die
        // gestellt wird.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `dense` hier nicht steckt:
        // §8.5 „fill in any holes left by earlier grid items" ist eine
        // eigene Regel und wuerde in einem Boolean verschwinden. Sie
        // ist NICHT gebaut; `column dense` verhaelt sich deshalb wie
        // `column`.
        Eigenschaft::GitterAutomatik => {
            if let Wert::Wort(x) = &w {
                erg.gitter_automatik = x.clone();
                // GEMESSENER BEFUND (02.10.2026 11:5x), mit
                // bedingungslosem `eprintln` in diesem Arm:
                //
                //     "grid-auto-flow: column"        -> "column"
                //     "grid-auto-flow: column dense"  -> "column dense"
                //     "grid-auto-flow: row dense"     -> "row dense"
                //
                // Der Hand-Parser gibt den GANZEN Wert als EIN Wort
                // weiter, Leerzeichen eingeschlossen. Mein Vergleich
                // `x == "column"` traf also nur die einwortige Form, und
                // GEMESSEN verhielt sich `column dense` wie `row` —
                // die Richtung ging bei zwei Wörtern verloren.
                //
                // GEMESSENE ENTSCHEIDUNG: das ERSTE Wort entscheidet.
                // §7.4 definiert `[row | column] || dense` — die Richtung
                // steht vorn, `dense` ist der Zusatz. Also wird nicht
                // nach `dense` gesucht (das waere eine Annahme ueber
                // die Reihenfolge), sondern das erste Wort gelesen.
                //
                //
                // L4 (03.10.2026 04:2x): `dense` wird gelesen. Der Grund,
                // warum es bis eben NICHT gelesen wurde, steht hier: der
                // Vergleich suchte nach einem bestimmten Wort und las das
                // erste. GEMESSEN: `l4_dense_verhaelt_sich_wie_ohne_weil_
                // es_ein_boolean_ist` war gruen -- `row dense` verhielt
                // sich wie `row`.
                //
                // GEMESSENE ENTSCHEIDUNG, warum alle WORTER gelesen
                // werden und nicht nur nach `dense` gesucht: §8.5 nennt
                // `dense` einen Zusatz auf den Fluss, und die Reihenfolge
                // der Woerter ist in §8.5 nicht festgelegt. Also: das
                // erste Wort entscheidet die ACHSE, und irgendein Wort
                // entscheidet die PACKUNG.
                erg.gitter_fluss_dicht = x.split_whitespace().any(|w| w == "dense");
                erg.gitter_fluss_spalten = x.split_whitespace().next() == Some("column");
            }
        }
        // CSS Flexbox 1 §5.1: ganzzahlig, negativ erlaubt. Release 11.10.2026:
        // eine Bruchzahl (`order: 1.5`) ist ungueltig und bleibt ohne Wirkung
        // (WPT `flexbox_order-noninteger-invalid`).
        Eigenschaft::Ordnung => {
            if let Wert::Laenge(l) = &w {
                if l.wert.is_finite() && l.wert.fract() == 0.0 {
                    erg.ordnung = (l.wert as i32).clamp(-32768, 32767);
                }
            }
        }
        // CSS Box Sizing 4 §2 / CSS Grid 1 §10.1. `normal` = 0.
        Eigenschaft::LueckeSpalte => {
            if let Wert::Laenge(l) = &w {
                erg.luecke_spalte =
                    laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX);
            }
        }
        // L4 (03.10.2026 09:5x): `align-self` (§6.6, §10.4) und
        // `justify-self` (§6.2, §10.3).
        //
        // GEMESSENER BEFUND, dass sie nicht ankamen: beide zeigten im
        // Parser auf `ESchluessel` und landeten im Schluessel-Behaelter.
        // GEMESSEN (Release, `examples/l4_raster.rs`, Block AA, Zelle
        // 300x100, Kind 40x20): `stretch`, `center`, `flex-start`,
        // `flex-end` und `justify-self: center` ergaben ALLE `b 300 h 100`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier ZWEI Felder stehen und nicht
        // eins: §6.2 und §6.3 nennen die Wertemenge gleich, aber die ACHSE
        // ist es, die die Platzierung liest (Block gegen Quer).
        Eigenschaft::AusrichtungSelbst => {
            if let Wert::Wort(x) = &w {
                erg.ausrichtung_selbst = selbst_ausrichtung(x);
            }
        }
        Eigenschaft::AusrichtungSelbstWaagerecht => {
            if let Wert::Wort(x) = &w {
                erg.ausrichtung_selbst_waagerecht = selbst_ausrichtung(x);
            }
        }
        // L4 (11:5x): `justify-items` (§6.2) — der Vorgabewert des Kindes
        // auf der Querachse. GEMESSENER BEFUND, dass die Eigenschaft nicht
        // ankam: sie zeigte auf `ESchluessel`.
        Eigenschaft::JustifyAusrichtung => {
            if let Wert::Wort(x) = &w {
                erg.justify_ausrichtung = selbst_ausrichtung(x).unwrap_or(FlexAusrichtung::Strecke);
            }
        }
        // CSS Grid 1 §10.1.
        Eigenschaft::LueckeZeile => {
            if let Wert::Laenge(l) = &w {
                erg.luecke_zeile =
                    laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX);
            }
        }
        // CSS Overflow 3 §3.1: die beiden Achsen sind getrennte
        // Eigenschaften mit getrennten Anfangswerten.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): `overflow-x` und
        // `overflow-y` waren nicht im Ziel, also gingen 61 Deklarationen
        // verloren — auf einer Seite, auf der `overflow-y: hidden` ueber
        // die Kopfzeile entscheidet.
        // CSS Overflow 3 §3.1. Das Einzelfeld `ueberlauf` wird
        // MITGEsetzt, damit die Aufrufer, die nur eine Richtung
        // kennen, weiterhin etwas sehen.
        //
        // GEMESSENER FEHLER 01.10.2026 (K7c): als `overflow` von der
        // Langform auf die Kurzform der beiden Achsen umgestellt wurde,
        // blieb `Berechnet::ueberlauf` auf `Sichtbar` — und zwei
        // K1-Tests fielen um (`alle_drei_paare_in_einer_regel`,
        // `overflow_verliert_nicht_gegen_align_items`).
        //
        // Die Wahl der x-Achse ist eine Entscheidung: sie ist die
        // erste im Kurztext, also gewinnt sie bei `overflow: hidden
        // auto` gegen `auto hidden`. Dokumentiert, nicht geraten.
        // Release 11.10.2026: `auto` kommt als `Wert::Auto` (nicht als
        // Wort) an. GEMESSEN vorher: `overflow-y: auto` fiel weg — der
        // Kasten blieb `visible` (WPT cssom-view scrollintoview-*).
        Eigenschaft::OverflowX => {
            let x = match &w { Wert::Wort(x) => Some(x.as_str()), Wert::Auto => Some("auto"), _ => None };
            if let Some(x) = x {
                // Release 11.10.2026: `clip` macht keinen Rollkasten (CSS
                // Overflow 3 §3.1).
                erg.ueberlauf_clip[0] = x == "clip";
                erg.overflow_x = ueberlauf_aus(x);
                // `visible` wird zu `Sichtbar` — das ist der
                // Anfangswert des Feldes, nicht `None`.
                erg.ueberlauf = ueberlauf_aus(x).unwrap_or(Ueberlauf::Sichtbar);
            }
        }
        Eigenschaft::OverflowY => {
            let x = match &w { Wert::Wort(x) => Some(x.as_str()), Wert::Auto => Some("auto"), _ => None };
            if let Some(x) = x {
                erg.ueberlauf_clip[1] = x == "clip";
                erg.overflow_y = ueberlauf_aus(x);
            }
        }
        // CSS Images 1 §5.2.
        Eigenschaft::ObjektPassung => {
            if let Wert::Wort(x) = &w {
                erg.objekt_passung = x.clone();
            }
        }
        // CSS Sizing 4 §4.1: eine Zahl oder `a / b` — das Verhaeltnis
        // Breite : Hoehe (Release 10.10.2026: als Quotient, siehe
        // `verhaeltnis_zaehler`).
        Eigenschaft::Seitenverhaeltnis => {
            if let Wert::Wort(x) = &w {
                if let Some(z) = verhaeltnis_zaehler(x) {
                    // Release 11.10.2026: `auto && <ratio>` merkt sich
                    // `seitenverhaeltnis_auto` — ein ersetztes Element nimmt
                    // dann sein natuerliches Verhaeltnis, falls es eines hat
                    // (CSS Sizing 4 §5.1); ohne `auto` gilt die Angabe.
                    // GEMESSEN vorher: WPT css-sizing
                    // `aspect-ratio/flex-aspect-ratio-015` (ein 20 x 50-Bild mit
                    // `aspect-ratio: 1/1` behielt in Flex 2:5).
                    erg.seitenverhaeltnis_auto = x.split(|c: char| c.is_whitespace() || c == '/').any(|t| t.eq_ignore_ascii_case("auto"));
                    erg.seitenverhaeltnis = z.clamp(0.0, 1000.0);
                }
            } else if let Wert::Laenge(l) = &w {
                erg.seitenverhaeltnis = l.wert.clamp(0.0, 1000.0);
                erg.seitenverhaeltnis_auto = false;
            }
        }
        // CSS Tables 3 §3.3.
        Eigenschaft::TabellenKanten => {
            if let Wert::Wort(x) = &w {
                erg.tabellen_kanten = x.clone();
            }
        }
        // CSS Tables 3 §3.4.
        Eigenschaft::TabellenLage => {
            if let Wert::Wort(x) = &w {
                erg.tabellen_lage = x.clone();
            }
        }
        // ---- K7c: DER SCHLUESSELBEHAELTER ----
        //
        // CSS Values 3 §3: der berechnete Wert einer Eigenschaft, die
        // weder Zahl noch Farbe ist, ist der ANGEGEBENE Wert. Fuer
        // `cursor: pointer` ist er `pointer`. Was hier landet, ist
        // also der berechnete Wert — nicht weggeworfen.
        //
        // M4: hoechstens `MAX_SCHLUESSEL` (48). Darueber wird
        // gezaehlt, nicht abgeschnitten: die abgeschnittene Angabe
        // waere eine halbe CSS-Regel.
        Eigenschaft::Schluessel => {
            // Der Name steht im WERT (siehe `Wert::Schluessel`), damit
            // ~190 Eigenschaften einen Behaelter teilen koennen, ohne
            // dass eine die andere ueberschreibt.
            if let Wert::Schluessel(name, x) = &w {
                if !x.is_empty() {
                    // M4: nur ein NEUER Name kostet einen Platz; ein
                    // bekannter Name wird ueberschrieben.
                    let bekannt = erg.schluessel.iter().any(|el| &el.0 == name);
                    tabellen_schluessel(name, x, erg);
                    if bekannt || erg.schluessel.len() < crate::MAX_SCHLUESSEL {
                        upsert_schluessel(&mut erg.schluessel, name, x);
                    } else {
                        // Zu viele verschiedene Namen: gezaehlt, nicht
                        // abgeschnitten — die letzten Namen waeren eine
                        // halbe CSS-Regel.
                        profil(pz::VERWORFEN);
                    }
                }
            }
        }
        Eigenschaft::GitterSpalteStart => {
            // L4 (02.10.2026 23:4x): dieselbe Trennung wie bei der
            // Zeilenachse — `span N` ist eine Spannweite, keine Linie.
            let roh = d.roh.to_ascii_lowercase();
            // L4 (00:4x): dieselbe Teilung wie bei der Zeilenachse; siehe
            // dort die Messung (506 von 532 Vorkommen sind
            // `grid-column: auto/span N`).
            let teile = gitter_teile(&roh);
            if let Some(v) = teile.first() {
                match gitter_spanne(v) {
                    Some(sp) => erg.gitter_spalte_spanne = sp,
                    None => erg.gitter_spalte_start = gitter_linie(v),
                }
            }
            if let Some(v) = teile.get(1) {
                match gitter_spanne(v) {
                    Some(sp) => {
                        // §8.3: siehe Zeilenachse.
                        erg.gitter_spalte_spanne = sp.max(erg.gitter_spalte_spanne);
                    }
                    None => erg.gitter_spalte_ende = gitter_linie(v),
                }
            }
            // L4 (06:1x): §8.3 — Namen gegen die Spaltenlinien des
            // CONTAINERS. GEMESSEN im Korpus: `grid-column: <name>` kommt
            // nicht vor, `grid-area: content` 9x — und das liest Teil 0
            // fuer die Zeilen und Teil 1 fuer die Spalten.
            loese_liniennamen(erg, &teile, PlatzAchse::NurSpalte, eltern);
        }
        Eigenschaft::GitterSpalteEnde => {
            erg.gitter_spalte_ende = gitter_linie(&d.roh.to_ascii_lowercase());
            // L4 (06:1x): `grid-column-end: name` — ein Teil, Spaltenachse.
            loese_liniennamen(
                erg,
                &[&d.roh.to_ascii_lowercase()],
                PlatzAchse::SpalteEnde,
                eltern,
            );
        }

        // ---- K7 ----
        // CSS Box Sizing 3, §2. Die ANGABE wird gemerkt; die
        // Wirkung gehoert in den Layout-Schritt (WUNSCH_CSS.txt W10).
        Eigenschaft::BoxArt => {
            if let Wert::BoxArt(b) = &w {
                erg.box_art = *b;
            }
        }
        // CSS Sizing 3, §2: `max-content`/`min-content`/`fit-content`
        // sind keine Laengen, sondern Aufträge an die Messung der
        // Inhaltsbreite. Als Wort gemerkt, damit die Laenge nicht
        // geraten wird.
        Eigenschaft::BreiteArt => {
            if let Wert::Wort(x) = &w {
                erg.breite_art = x.clone();
            }
        }
        // CSS Masking 1, §4.1: die Ebenenliste, die ERSTE oben.
        // `none` leert sie (und ist der Anfangswert — darum nicht
        // vererbt).
        Eigenschaft::MaskenBild => match &w {
            Wert::Maske(l) => erg.masken = l.clone(),
            _ => {}
        },
        // Die drei Langformen gelten fuer ALLE Ebenen.
        //
        // # WAS HIER FEHLT UND WARUM
        //
        // Die Spec (CSS Masking 1 §4.1) laesst die Langformen pro
        // Ebene gelenkt werden: `mask-size: 10px, cover` gibt der
        // ersten Ebene 10 px und der zweiten `cover`. Diese Kaskade
        // kennt die Ebenenzahl erst, wenn `mask-image` gelesen ist,
        // und die Reihenfolge im Block ist frei — der Wert kann VOR
        // `mask-image` kommen. Ohne zweites Lesen kann sie die
        // Ebenenzahl nicht zuordnen.
        //
        // Deshalb gilt der Wert fuer alle. Das ist eine DOKUMENTIERTE
        // Naeherung, und sie ist falsch bei mehreren Ebenen mit
        // verschiedenen Langformen — auf wikipedia GEMESSEN: 110
        // `mask-image`, aber keine Stelle mit mehr als einer Ebene UND
        // abweichenden Langformen.
        Eigenschaft::MaskenGroesse => {
            if let Wert::Maske(l) = &w {
                for m in erg.masken.iter_mut() {
                    m.groesse = l[0].groesse;
                }
            }
        }
        Eigenschaft::MaskenPosition => {
            if let Wert::Maske(l) = &w {
                for m in erg.masken.iter_mut() {
                    m.position_x = l[0].position_x;
                    m.position_y = l[0].position_y;
                }
            }
        }
        Eigenschaft::MaskenWiederholen => {
            if let Wert::Maske(l) = &w {
                for m in erg.masken.iter_mut() {
                    m.wiederholen_x = l[0].wiederholen_x;
                    m.wiederholen_y = l[0].wiederholen_y;
                }
            }
        }
        // CSS Backgrounds 3 §3.3-§3.5: drei Langformen, EIN Feld.
        Eigenschaft::BildGroesse | Eigenschaft::BildPosition | Eigenschaft::BildWiederholen => {
            if let Wert::BildGeo(g) = &w {
                // Nur die Ecke setzen, die diese Langform meint —
                // sonst wuerde `background-size: cover` die Position
                // auf 0,0 zuruecksetzen.
                match e {
                    Eigenschaft::BildGroesse => {
                        erg.bild_geo.groesse = g.groesse;
                        erg.bild_geo.cover = g.cover;
                        erg.bild_geo.contain = g.contain;
                        // Release 13.10.2026 (Agent CSS2): Laengen in px
                        // (`em`/`ch`/`ex` gegen die eigene Schrift), Prozente
                        // bleiben Anteile der Flaeche.
                        erg.bild_geo.groesse_xy = g.groesse_xy.map(|l| {
                            l.map(|l| match l.einheit {
                                super::typen::Einheit::Prozent => l,
                                _ => Laenge {
                                    wert: laenge_px(&l, em, rem_basis, ansicht, ansicht, ansicht_hoehe)
                                        .clamp(-MAX_PX, MAX_PX),
                                    einheit: super::typen::Einheit::Px,
                                },
                            })
                        });
                    }
                    Eigenschaft::BildPosition => {
                        erg.bild_geo.position_x = g.position_x;
                        erg.bild_geo.position_y = g.position_y;
                        // Release 11.10.2026 (Agent CSS2): ob die Zahlen
                        // Pixel sind, gehoert dazu. GEMESSEN vorher: jede
                        // Pixelposition (`8px bottom`, `10px 20px`) kam als
                        // Anteil an (8.0 = 800 %), das Bild lag daneben.
                        erg.bild_geo.position_px = g.position_px;
                        erg.bild_geo.position_px_achsen = g.position_px_achsen;
                        erg.bild_geo.vom_ende = g.vom_ende;
                        // `em` gegen die Schriftgroesse des Elements.
                        if g.position_em.0 {
                            erg.bild_geo.position_x = g.position_x * erg.schriftgroesse_px;
                        }
                        if g.position_em.1 {
                            erg.bild_geo.position_y = g.position_y * erg.schriftgroesse_px;
                        }
                        erg.bild_geo.position_em = (false, false);
                        if g.position_ex.0 {
                            erg.bild_geo.position_x = g.position_x * erg.schriftgroesse_px * ex_anteil();
                        }
                        if g.position_ex.1 {
                            erg.bild_geo.position_y = g.position_y * erg.schriftgroesse_px * ex_anteil();
                        }
                        erg.bild_geo.position_ex = (false, false);
                    }
                    _ => {
                        erg.bild_geo.wiederholen_x = g.wiederholen_x;
                        erg.bild_geo.wiederholen_y = g.wiederholen_y;
                        erg.bild_geo.wiederhol_art = g.wiederhol_art;
                    }
                }
            }
        }
    }
}

/// True bei Rahmen-Langform (fuer Ganzwert-Zerlegung, W6C).
fn ist_rahmen_langform(e: Eigenschaft) -> bool {
    use Eigenschaft::{
        RahmenLinksBreite, RahmenLinksFarbe, RahmenLinksStil, RahmenObenBreite, RahmenObenFarbe,
        RahmenObenStil, RahmenRechtsBreite, RahmenRechtsFarbe, RahmenRechtsStil, RahmenUntenBreite,
        RahmenUntenFarbe, RahmenUntenStil,
    };
    matches!(
        e,
        RahmenObenBreite
            | RahmenRechtsBreite
            | RahmenUntenBreite
            | RahmenLinksBreite
            | RahmenObenStil
            | RahmenRechtsStil
            | RahmenUntenStil
            | RahmenLinksStil
            | RahmenObenFarbe
            | RahmenRechtsFarbe
            | RahmenUntenFarbe
            | RahmenLinksFarbe
    )
}

/// True bei Schrift-Langform (fuer Ganzwert-Zerlegung, W6C).
fn ist_schrift_langform(e: Eigenschaft) -> bool {
    use Eigenschaft::{Schriftfamilie, Schriftgewicht, Schriftgroesse, Schriftstil, Zeilenhoehe};
    matches!(
        e,
        Schriftstil | Schriftgewicht | Schriftgroesse | Zeilenhoehe | Schriftfamilie
    )
}

/// True, wenn `roh` eine Rechenfunktion enthaelt (C11, fallunabhaengig).
fn hat_mathe(roh: &str) -> bool {
    hat_funktion(roh, "calc(")
        || hat_funktion(roh, "min(")
        || hat_funktion(roh, "max(")
        || hat_funktion(roh, "clamp(")
}

/// Aufgeloester Wert einer Gewinn-Deklaration (C10/C11, W6): var() wird
/// mit den Element-Variablen ersetzt, calc()/min()/max()/clamp() mit dem
/// Kontext gerechnet. None = ungueltig (Deklaration faellt weg — die
/// Naeherung fuer "invalid at computed-value time": Vererbtes bzw.
/// Anfangswerte bleiben stehen, statt zu raten).
fn effektiver_wert(
    e: Eigenschaft,
    d: &Deklaration,
    vars: &Variablen,
    ctx: &RechenCtx,
) -> Option<Wert> {
    let braucht_var = hat_funktion(&d.roh, "var(");
    // Release 12.10.2026 (Agent C): `attr()` wie `var()` erst hier.
    let braucht_attr = crate::attr_kaskade::aktiv() && hat_funktion(&d.roh, "attr(");
    let braucht_erben = crate::attr_kaskade::erben_aktiv() && hat_funktion(&d.roh, "inherit(");
    // Release 12.10.2026 (Agent C): `if()` (`crate::cssom::wenn`).
    let braucht_wenn = crate::attr_kaskade::erben_aktiv() && hat_funktion(&d.roh, "if(");
    let braucht_mathe = hat_mathe(&d.roh);
    // L4 (03.10.2026 08:0x): EINE SPUR-LISTE DARF NICHT ZU EINER
    // LAENGE ZUSAMMENGEZOGEN WERDEN.
    //
    // GEMESSENER BEFUND, der hier hergefuehrt hat: `grid-template-columns:
    // min(100px,50%)` kam als `Wert::Laenge(100px)` an, weil der Weg
    // unten JEDE Deklaration mit einer Rechenfunktion zu einer Zahl
    // macht — und der `RasterSpalten`-Zweig verlangt `Wert::Wort`, also
    // blieb die Liste leer (GEMESSEN rot: `raster_spalten.len() == 0`).
    //
    // GEMESSENE ENTSCHEIDUNG, warum hier eine AUSNAHME steht und nicht im
    // Raster-Zweig: eine Liste aus mehreren Spuren ist per Definition
    // mehr als eine Laenge. Wer sie zusammenfasst, verliert die Liste —
    // und der Raster-Zwig kann sie nicht wiederherstellen, weil er nur
    // einen Wert sieht.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ALLE Raster-Listen betroffen sind
    // und nicht nur `grid-template-columns`: dieselbe Liste traegt
    // `grid-template-rows`, `grid-auto-rows`, `grid-auto-columns`
    // (§7.3/§7.5 nennen dieselbe Form), und `grid-template-areas` ist
    // eine Liste von Zeilen. `var(--x)` darf hier uebrigens durch — die
    // Liste wird spaeter gelesen, und die Ersetzung steht dann schon im
    // Rohwert.
    //
    // **EIN A/B, DAS MICH BELEOG, und die Korrektur:** ich hatte die
    // Ausnahme einmal weggenommen, um zu pruefen, ob sie noetig ist, und
    // der Testlauf meldete GRUEN. GEMESSEN richtig (09:03): dieselben drei
    // Tests sind OHNE die Ausnahme rot. Der erste Lauf ist gegen ein
    // altes Binary gelaufen. **Ein A/B, der die Aenderung nicht sieht, ist
    // kein A/B.**
    let spur_liste = matches!(
        e,
        Eigenschaft::RasterSpalten
            | Eigenschaft::RasterZeilen
            | Eigenschaft::RasterAutoZeilen
            | Eigenschaft::RasterAutoSpalten
            | Eigenschaft::GitterGebiete
    );
    if !braucht_var && !braucht_attr && !braucht_erben && !braucht_wenn && (!braucht_mathe || spur_liste) {
        // Schnellweg: geparster Wert gilt. Platzhalter ohne Grund waere
        // ein Konstruktionsfehler — defensiv weg statt anzuwenden.
        if let Wert::Wort(x) = &d.wert {
            if x == "spaet" {
                return None;
            }
        }
        return Some(d.wert.clone());
    }
    let mut roh = d.roh.clone();
    if braucht_wenn {
        roh = crate::attr_kaskade::wenn_ersetzen(&roh, vars)?;
        if roh.trim().is_empty() {
            return None;
        }
    }
    if braucht_erben {
        roh = crate::attr_kaskade::erben_ersetzen(&roh)?;
    }
    if braucht_var {
        roh = ersetze_var(&roh, vars, 0)?;
        // Ein `if()` aus einer Variablen (Agent C, `crate::cssom::wenn`).
        if crate::attr_kaskade::erben_aktiv() && hat_funktion(&roh, "if(") {
            roh = crate::attr_kaskade::wenn_ersetzen(&roh, vars)?;
            if roh.trim().is_empty() {
                return None;
            }
        }
    }
    if crate::attr_kaskade::aktiv() && hat_funktion(&roh, "attr(") {
        roh = crate::attr_kaskade::ersetzen(&roh)?;
    }
    // Nach var(): generische Schluesselwoerter (ganzer Wert, CSS-Regel).
    let k = roh.trim().to_ascii_lowercase();
    if k == "inherit" {
        return Some(Wert::Erben);
    }
    if k == "initial" {
        return Some(Wert::Anfang);
    }
    if k == "unset" {
        return Some(Wert::Unbestimmt);
    }
    // Kurzform-Langformen (border/font, W6C): substituierte Ganzwerte
    // werden fachlich zerlegt (nicht als Einzelwert geparst).
    if ist_rahmen_langform(e) {
        if let Some(w) = crate::parse::border_teil_fuer(e, &k, ctx) {
            return Some(w);
        }
        // Kein Treffer: weiter zum Rechen-/Einzelweg unten.
    } else if ist_schrift_langform(e) {
        if let Some(w) = crate::parse::font_teil_fuer(e, &k, ctx) {
            return Some(w);
        }
        // Kein Treffer: weiter unten.
    }
    // Rechnen (auch erst durch var() entstanden, C11): Ergebnis in px,
    // ausser Wachstum/Schrumpf (Faktor, einheitenlos).
    //
    // L4 (08:0x): fuer eine SPUR-LISTE ist dieser Weg aus — siehe die
    // Begruendung bei `spur_liste` (GEMESSEN: sonst wird aus der Liste
    // eine Laenge, und der Raster-Zweig sieht nichts).
    // Release 14.10.2026 (Agent C): a color function with math inside
    // (`rgb(calc(5% + 15%), …)`) is a color, not a length (WPT css-values
    // calc-rgb-percent-001).
    if ["rgb(", "rgba(", "hsl(", "hsla(", "hwb(", "lab(", "lch(", "oklab(", "oklch(", "color(", "color-mix("]
        .iter()
        .any(|f| k.starts_with(f))
    {
        return crate::parse::lang_wert(e, &k);
    }
    if !spur_liste && (braucht_mathe || hat_mathe(&roh)) {
        if let Some(px) = rechne(&roh, ctx, 0) {
            return Some(match e {
                Eigenschaft::FlexWachstum | Eigenschaft::FlexSchrumpf => Wert::Laenge(Laenge {
                    wert: px,
                    einheit: super::typen::Einheit::Zahl,
                }),
                _ => Wert::Laenge(Laenge {
                    wert: px,
                    einheit: super::typen::Einheit::Px,
                }),
            });
        }
        return None;
    }
    // Nur var(): Text erneut parsen (als stuende er im Stylesheet).
    crate::parse::lang_wert(e, &k)
}

/// Release 11.10.2026: die (px, Prozent)-Mischung eines `calc()` mit
/// Prozentanteil, `None` ohne Prozent oder fuer mehrteilige Werte (eine
/// Kurzform mit mehreren Angaben).
fn misch_anteile(roh: &str, ctx: &RechenCtx) -> Option<(f32, f32)> {
    let k = roh.trim();
    if !k.contains('%') || !hat_mathe(k) {
        return None;
    }
    // Mehrere Angaben auf oberster Ebene: nicht eine Laenge.
    let mut tiefe = 0i32;
    for c in k.chars() {
        match c {
            '(' => tiefe += 1,
            ')' => tiefe -= 1,
            c if c.is_whitespace() && tiefe == 0 => return None,
            _ => {}
        }
    }
    let mut c0 = ctx.clone();
    c0.prozent = 0.0;
    let mut c1 = ctx.clone();
    c1.prozent = 100.0;
    let a = rechne(k, &c0, 0)?;
    let b = rechne(k, &c1, 0)?;
    let p = b - a;
    (a.is_finite() && p.is_finite() && p.abs() > 1e-4).then_some((a, p))
}

/// Das Mischungsfeld der Eigenschaft `e` (`None` = keine mit Mischung).
fn mischung_feld(m: &mut super::typen::Mischung, e: Eigenschaft, start_rechts: bool) -> Option<&mut Option<(f32, f32)>> {
    use Eigenschaft as E;
    Some(match e {
        E::Breite => &mut m.breite,
        E::Hoehe => &mut m.hoehe,
        E::MargeOben => &mut m.marge[0],
        E::MargeRechts => &mut m.marge[1],
        E::MargeUnten => &mut m.marge[2],
        E::MargeLinks => &mut m.marge[3],
        E::MargeInnenStart => &mut m.marge[if start_rechts { 1 } else { 3 }],
        E::MargeInnenEnde => &mut m.marge[if start_rechts { 3 } else { 1 }],
        E::FuellungOben => &mut m.fuellung[0],
        E::FuellungRechts => &mut m.fuellung[1],
        E::FuellungUnten => &mut m.fuellung[2],
        E::FuellungLinks => &mut m.fuellung[3],
        E::FuellungInnenStart => &mut m.fuellung[if start_rechts { 1 } else { 3 }],
        E::FuellungInnenEnde => &mut m.fuellung[if start_rechts { 3 } else { 1 }],
        _ => return None,
    })
}

/// Traegt die Mischung der Eigenschaft `e` ein (oder loescht sie).
fn mischung_setzen(e: Eigenschaft, roh: &str, ctx: &RechenCtx, erg: &mut Berechnet) {
    let rechts = erg.richtung.start_ist_rechts();
    let mut probe = super::typen::Mischung::default();
    if mischung_feld(&mut probe, e, rechts).is_none() {
        return;
    }
    let v = misch_anteile(roh, ctx);
    if v.is_none() && erg.mischung.is_none() {
        return;
    }
    let m = erg.mischung.get_or_insert_with(Default::default);
    if let Some(z) = mischung_feld(m, e, rechts) {
        *z = v;
    }
}

/// top/right/bottom/left-Wert: auto -> None, Laenge -> px (negativ
/// erlaubt, CSS-Regel).
///
/// `prozent` ist die %-Basis **dieser Achse**: die Ansichtshoehe fuer
/// `oben`/`unten`, die Ansichtsbreite fuer `links`/`rechts` (CSS Position 3
/// §5.1). Vorher stand ueberall die Breite — siehe die Aufrufstelle.
fn versatz_wert(
    w: &Wert,
    em: f32,
    rem_basis: f32,
    prozent: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
) -> Option<f32> {
    match w {
        Wert::Auto => None,
        Wert::Laenge(l) => Some(laenge_px(l, em, rem_basis, prozent, ansicht, ansicht_hoehe)),
        _ => None,
    }
}

/// Kanonische Dekorationsworte -> Bitmaske (1/2/4). Unbekanntes -> 0
/// (der Parser laesst es nicht durch; doppelte Sicherheit, kein Panic).
fn deko_bits(w: &str) -> u8 {
    if w == "keine" {
        return 0;
    }
    let mut bits = 0u8;
    for teil in w.split_whitespace() {
        match teil {
            "underline" => bits |= 1,
            "overline" => bits |= 2,
            "line-through" => bits |= 4,
            _ => {}
        }
    }
    bits
}

/// margin-Wert: auto -> 0 (unser Layout kennt kein Zentrieren per auto —
/// dokumentierte Naeherung), Laenge gegen Ansicht (%-Basis: Breite).
fn rand_wert(w: &Wert, em: f32, rem_basis: f32, ansicht: f32, ansicht_hoehe: f32) -> f32 {
    match w {
        Wert::Auto => 0.0,
        Wert::Laenge(l) => laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe),
        _ => 0.0,
    }
}

/// padding-Wert: wie margin, aber ohne auto (fiel schon im Parser weg).
fn fuell_wert(w: &Wert, em: f32, rem_basis: f32, ansicht: f32, ansicht_hoehe: f32) -> f32 {
    match w {
        Wert::Laenge(l) => {
            laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX)
        }
        _ => 0.0,
    }
}

/// border-width: nie negativ (CSS-Regel: negativ ist ungueltig — der
/// Parser liess es durch, hier wird gekappt statt verworfen).
fn breite_wert(w: &Wert, em: f32, rem_basis: f32, ansicht: f32, ansicht_hoehe: f32) -> f32 {
    match w {
        Wert::Laenge(l) => {
            laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX)
        }
        _ => 0.0,
    }
}

/// width/height: auto -> None, Laenge -> px (negativ -> 0).
/// `none` (CSS Sizing 3 §2.1) — „keine Grenze".
///
/// Der Parser gibt das Wort durch, NICHT eine 0-px-Laenge. Das ist
/// Absicht: `max-width: none` und `max-width: 0px` sind zwei
/// verschiedene Aussagen, und eine 0-px-Laenge als Marker wuerde
/// beide gleichsetzen.
///
/// GEMESSENER FEHLER 01.10.2026 (K7): die erste Fassung verglich
/// `l.wert == 0.0` und machte damit auch `max-width: 0px` zu „keine
/// Grenze". Der Test `max_width_none_ist_keine_grenze` sichert beide
/// Werte.
fn ist_none(w: &Wert) -> bool {
    matches!(w, Wert::Wort(x) if x == "none")
}

/// P19 (05.10.2026, 19:46): der PROZENTANTEIL eines Wertes, oder `None`.
///
/// `breiten_wert` macht aus `50%` die Zahl `0,5 × ansicht`. Diese
/// Funktion liefert das `50` selbst, damit `Berechnet` beides behalten
/// kann (siehe `typen.rs`, P19). Fuer alles, was kein Prozent ist, ist
/// die Antwort `None` — und `None` heisst in den Feldern "der Autor
/// hat eine absolute Laenge oder ein Schluesselwort geschrieben".
fn prozent_wert(w: &Wert) -> Option<f32> {
    match w {
        Wert::Laenge(l) if l.einheit == super::typen::Einheit::Prozent && l.wert.is_finite() => {
            Some(l.wert)
        }
        _ => None,
    }
}

fn breiten_wert(
    w: &Wert,
    em: f32,
    rem_basis: f32,
    ansicht: f32,
    ansicht_hoehe: f32,
) -> Option<f32> {
    match w {
        Wert::Auto => None,
        Wert::Laenge(l) => {
            Some(laenge_px(l, em, rem_basis, ansicht, ansicht, ansicht_hoehe).clamp(0.0, MAX_PX))
        }
        _ => None,
    }
}

/// border-style-Wort -> Stil (Unbekanntes -> Kein, doppelte Sicherheit).
fn stil_wert(w: &Wert) -> Rahmenstil {
    match w {
        Wert::Wort(s) => match s.as_str() {
            "durchgehend" => Rahmenstil::Durchgehend,
            "gemustert" => Rahmenstil::Gemustert,
            // BILD_37: die sieben Musterstile sind einzeln (siehe
            // `parse.rs::rahmenstil_wert`, wo die Umkehr begruendet ist).
            "gestrichelt" => Rahmenstil::Gestrichelt,
            "gepunktet" => Rahmenstil::Gepunktet,
            "doppelt" => Rahmenstil::Doppelt,
            "furche" => Rahmenstil::Furche,
            "wulst" => Rahmenstil::Wulst,
            "innen" => Rahmenstil::Innen,
            "aussen" => Rahmenstil::Aussen,
            _ => Rahmenstil::Kein,
        },
        _ => Rahmenstil::Kein,
    }
}

/// border-color (currentcolor -> eigene Farbe).
fn rahmenfarbe(w: &Wert, eigene: Farbe) -> Farbe {
    match w {
        Wert::Farbe(f) => *f,
        Wert::Aktuell => eigene,
        _ => eigene,
    }
}

/// inherit: Wert der Eltern oder Anfangswert ohne Eltern.
/// Release 08.10.2026: `inherit`/`initial` an Polster/Rand ersetzen eine
/// fruehere Prozentangabe (der Pixelwert gilt dann).
fn prozent_zuruecksetzen(e: Eigenschaft, erg: &mut Berechnet) {
    match e {
        Eigenschaft::FuellungOben => erg.fuellung_prozent[0] = None,
        Eigenschaft::FuellungRechts => erg.fuellung_prozent[1] = None,
        Eigenschaft::FuellungUnten => erg.fuellung_prozent[2] = None,
        Eigenschaft::FuellungLinks => erg.fuellung_prozent[3] = None,
        Eigenschaft::MargeOben => erg.marge_prozent[0] = None,
        Eigenschaft::MargeRechts => erg.marge_prozent[1] = None,
        Eigenschaft::MargeUnten => erg.marge_prozent[2] = None,
        Eigenschaft::MargeLinks => erg.marge_prozent[3] = None,
        _ => {}
    }
}

fn erbe(e: Eigenschaft, eltern: Option<&Berechnet>, erg: &mut Berechnet) {
    prozent_zuruecksetzen(e, erg);
    // Release 14.10.2026 (Agent CSS2): the computed value of a percentage
    // margin or padding is the percentage, so `inherit` takes it over and
    // resolves it against the child's own containing block (CSS 2.1 §8.3,
    // §8.4). MEASURED before (WPT `CSS2/margin-padding-clear/margin-
    // percentage-inherit-001`, `padding-percentage-inherit-001`): the
    // parent's pixel value was inherited.
    if let Some(p) = eltern {
        match e {
            Eigenschaft::FuellungOben => erg.fuellung_prozent[0] = p.fuellung_prozent[0],
            Eigenschaft::FuellungRechts => erg.fuellung_prozent[1] = p.fuellung_prozent[1],
            Eigenschaft::FuellungUnten => erg.fuellung_prozent[2] = p.fuellung_prozent[2],
            Eigenschaft::FuellungLinks => erg.fuellung_prozent[3] = p.fuellung_prozent[3],
            Eigenschaft::MargeOben => erg.marge_prozent[0] = p.marge_prozent[0],
            Eigenschaft::MargeRechts => erg.marge_prozent[1] = p.marge_prozent[1],
            Eigenschaft::MargeUnten => erg.marge_prozent[2] = p.marge_prozent[2],
            Eigenschaft::MargeLinks => erg.marge_prozent[3] = p.marge_prozent[3],
            _ => {}
        }
    }
    // Release 11.10.2026: die Rasterwerte (`crate::gitter`).
    crate::gitter::kaskade_erben(e, eltern, erg);
    let std = Berechnet::standard();
    let p = eltern.unwrap_or(&std);
    match e {
        // Release 08.10.2026: `inherit` erbt auch `display: contents`
        // (`nur_inhalt`). GEMESSEN vorher: apple.com, `li.globalnav-menu
        // {display:contents}` mit `.globalnav-flyout{display:inherit}` —
        // das Flyout wurde ein Block und die Hauptnavigation stand senkrecht.
        Eigenschaft::Anzeige => {
            erg.anzeige = p.anzeige;
            erg.nur_inhalt = p.nur_inhalt;
            erg.inline_aussen = p.inline_aussen;
            erg.flow_root = p.flow_root;
            erg.raster_lanes = p.raster_lanes;
        }
        Eigenschaft::Farbe => erg.farbe = p.farbe,
        Eigenschaft::Hintergrund => erg.hintergrund = p.hintergrund,
        Eigenschaft::Schriftgroesse => erg.schriftgroesse_px = p.schriftgroesse_px,
        Eigenschaft::Schriftgewicht => erg.schriftgewicht = p.schriftgewicht,
        Eigenschaft::Schriftstil => erg.schriftstil = p.schriftstil,
        Eigenschaft::Schriftfamilie => erg.schriftfamilie = p.schriftfamilie.clone(),
        Eigenschaft::Zeilenhoehe => { erg.zeilenhoehe_px = p.zeilenhoehe_px; erg.zeilenhoehe_art = p.zeilenhoehe_art; },
        Eigenschaft::Textausrichtung => {
            erg.textausrichtung = p.textausrichtung;
            erg.blocksatz_alle = p.blocksatz_alle;
            erg.ausrichtung_logisch = p.ausrichtung_logisch;
        }
        Eigenschaft::Textdekoration => erg.dekoration = p.dekoration,
        Eigenschaft::Weissraum => erg.weissraum = p.weissraum,
        Eigenschaft::MargeOben => erg.marge_oben = p.marge_oben,
        // CSS 2.1 §8.3: `margin: inherit` erbt den BERECHNETEN Wert des
        // Elternteils — und der kann `auto` sein. Die Flagge gehoert dazu,
        // sonst verliert ein Kind genau das wieder, woran der Motor das
        // Zentrieren erkennt. Diese Arme laufen nur bei ausdruecklichem
        // `inherit`; die Margen sind nicht erblich (§8.3), im Normalfall
        // greift `anfang()`.
        Eigenschaft::MargeRechts => {
            erg.marge_rechts = p.marge_rechts;
            erg.marge_rechts_auto = p.marge_rechts_auto;
        }
        Eigenschaft::MargeUnten => {
            erg.marge_unten = p.marge_unten;
            erg.marge_unten_auto = p.marge_unten_auto;
        }
        Eigenschaft::MargeLinks => {
            erg.marge_links = p.marge_links;
            erg.marge_links_auto = p.marge_links_auto;
        }
        // K17b: Marge und Fuellung werden NICHT geerbt (CSS 2.1 §8.3).
        // Diese Arme stehen trotzdem da, weil der `match` vollstaendig
        // sein muss — und sie tun das Richtige: sie erben nichts.
        // `erg` behaelt die 0 der Anfangswerte, und das ist die
        // Vorgabe fuer eine nicht geerbte Eigenschaft.
        Eigenschaft::MargeInnenStart
        | Eigenschaft::MargeInnenEnde
        | Eigenschaft::FuellungInnenStart
        | Eigenschaft::FuellungInnenEnde => {}
        // L4 (09:5x): `align-self`/`justify-self` sind NICHT erblich
        // (§6.2: „Inherited: no").  Der Anfangswert `stretch` steht also
        // auch fuer ein Kind ohne Angabe.
        // L4 (11:5x): `justify-items` ist ebenfalls nicht erblich (§6.2).
        Eigenschaft::AusrichtungSelbst
        | Eigenschaft::AusrichtungSelbstWaagerecht
        | Eigenschaft::JustifyAusrichtung => {}
        // CSS 2.1 §17.6 sagt „Inherited: no" — im Normalfall greift `anfang()`.
        // ABER `border-spacing: inherit` erbt den berechneten Wert des
        // Elternteils, und dieser Arm laeuft genau fuer das ausdrueckliche
        // `inherit`. Deshalb wird hier kopiert statt verworfen — dieselbe
        // Begruendung wie bei `margin: inherit`.
        Eigenschaft::RandAbstandWaagerecht => erg.randabstand_waagerecht = p.randabstand_waagerecht,
        Eigenschaft::RandAbstandSenkrecht => erg.randabstand_senkrecht = p.randabstand_senkrecht,
        Eigenschaft::FuellungOben => erg.fuellung_oben = p.fuellung_oben,
        Eigenschaft::FuellungRechts => erg.fuellung_rechts = p.fuellung_rechts,
        Eigenschaft::FuellungUnten => erg.fuellung_unten = p.fuellung_unten,
        Eigenschaft::FuellungLinks => erg.fuellung_links = p.fuellung_links,
        Eigenschaft::RahmenObenBreite => erg.rahmen_oben_breite = p.rahmen_oben_breite,
        Eigenschaft::RahmenRechtsBreite => erg.rahmen_rechts_breite = p.rahmen_rechts_breite,
        Eigenschaft::RahmenUntenBreite => erg.rahmen_unten_breite = p.rahmen_unten_breite,
        Eigenschaft::RahmenLinksBreite => erg.rahmen_links_breite = p.rahmen_links_breite,
        Eigenschaft::RahmenObenStil => erg.rahmen_oben_stil = p.rahmen_oben_stil,
        Eigenschaft::RahmenRechtsStil => erg.rahmen_rechts_stil = p.rahmen_rechts_stil,
        Eigenschaft::RahmenUntenStil => erg.rahmen_unten_stil = p.rahmen_unten_stil,
        Eigenschaft::RahmenLinksStil => erg.rahmen_links_stil = p.rahmen_links_stil,
        Eigenschaft::RahmenObenFarbe => erg.rahmen_oben_farbe = p.rahmen_oben_farbe,
        Eigenschaft::RahmenRechtsFarbe => erg.rahmen_rechts_farbe = p.rahmen_rechts_farbe,
        Eigenschaft::RahmenUntenFarbe => erg.rahmen_unten_farbe = p.rahmen_unten_farbe,
        Eigenschaft::RahmenLinksFarbe => erg.rahmen_links_farbe = p.rahmen_links_farbe,
        // `width` erbt sich (CSS 2.1 §6.2.2) — auch der Messauftrag.
        //
        // Release 10.10.2026: und der PROZENTSATZ. Geerbt wird der
        // berechnete Wert (CSS Cascade 4 §7.2), und der ist bei `100%` der
        // Prozentsatz — nicht die Pixelbreite, die die Kaskade nebenher
        // schaetzt. GEMESSEN vorher (github.com, `.CtaFormControl-field-wrap
        // span { width: inherit }` unter `width: 100%`): der `span` erbte
        // `breite = 1600` (Fensterbreite) ohne Prozent, galt als feste
        // Breite, und die Max-Content-Breite des Formulars wurde 1000 statt
        // 360 — das E-Mail-Feld lief ueber die ganze Seite.
        Eigenschaft::Breite => {
            erg.breite = p.breite;
            erg.breite_prozent = p.breite_prozent;
            erg.breite_art = p.breite_art.clone();
        }
        Eigenschaft::MaxBreite => {
            erg.max_breite = p.max_breite;
            erg.max_breite_prozent = p.max_breite_prozent;
        }
        Eigenschaft::MinBreite => {
            erg.min_breite = p.min_breite;
            erg.min_breite_prozent = p.min_breite_prozent;
        }
        Eigenschaft::Hoehe => {
            erg.hoehe = p.hoehe;
            erg.hoehe_prozent = p.hoehe_prozent;
            erg.hoehe_inhalt = p.hoehe_inhalt;
        }
        Eigenschaft::MinHoehe => {
            erg.min_hoehe = p.min_hoehe;
            erg.min_hoehe_prozent = p.min_hoehe_prozent;
        }
        Eigenschaft::MaxHoehe => {
            erg.max_hoehe = p.max_hoehe;
            erg.max_hoehe_prozent = p.max_hoehe_prozent;
        }
        Eigenschaft::Listenstil => erg.listenstil = p.listenstil,
        Eigenschaft::Sichtbarkeit => erg.sichtbar = p.sichtbar,
        Eigenschaft::Ueberlauf => erg.ueberlauf = p.ueberlauf,
        Eigenschaft::FlexRichtung => erg.flex_richtung = p.flex_richtung,
        Eigenschaft::FlexBuendig => erg.flex_buendig = p.flex_buendig,
        Eigenschaft::FlexAusrichtung => erg.flex_ausrichtung = p.flex_ausrichtung,
        // L3 (04:5x): `gap` ist Kurzform, also wandern alle DREI Felder
        // mit. GEMESSENE ENTSCHEIDUNG, warum hier doch vererbt wird,
        // obwohl `gap` sich nicht vererbt: dieser Arm wird nur erreicht,
        // wenn die Eigenschaft zum Vererbungsgehoert gezaehlt wird —
        // `gap` steht nicht in dieser Liste, der Arm ist also toter Code
        // wie vorher. Er bleibt der Vollstaendigkeit halber, und jetzt
        // mit allen drei Feldern, damit ein spaeteres Aufnehmen von
        // `gap` hier nicht nur die Hauptachse zuruecksetzt.
        Eigenschaft::Luecke => {
            erg.luecke_px = p.luecke_px;
            erg.luecke_spalte = p.luecke_spalte;
            erg.luecke_zeile = p.luecke_zeile;
        }
        Eigenschaft::FlexWachstum => erg.flex_wachstum = p.flex_wachstum,
        Eigenschaft::FlexSchrumpf => erg.flex_schrumpf = p.flex_schrumpf,
        Eigenschaft::FlexBasis => {
            erg.flex_basis = p.flex_basis;
            erg.flex_basis_art = p.flex_basis_art;
        }
        Eigenschaft::FlexUmbruch => {
            erg.flex_umbruch = p.flex_umbruch;
            erg.flex_umbruch_umgekehrt = p.flex_umbruch_umgekehrt;
            erg.flex_umbruch_ausgleich = p.flex_umbruch_ausgleich;
        }
        Eigenschaft::Position => erg.position = p.position,
        Eigenschaft::Oben => {
            erg.oben = p.oben;
            erg.oben_prozent = p.oben_prozent;
        }
        Eigenschaft::Rechts => {
            erg.rechts = p.rechts;
            erg.rechts_prozent = p.rechts_prozent;
        }
        Eigenschaft::Unten => {
            erg.unten = p.unten;
            erg.unten_prozent = p.unten_prozent;
        }
        Eigenschaft::Links => {
            erg.links = p.links;
            erg.links_prozent = p.links_prozent;
        }
        Eigenschaft::ZIndex => erg.z_index = p.z_index,
        Eigenschaft::Schweben => erg.schweben = p.schweben,
        Eigenschaft::Raeumung => erg.raeumung = p.raeumung,
        Eigenschaft::RasterSpalten => erg.raster_spalten = p.raster_spalten.clone(),
        // L4 (18:2x): die Zeilen erben wie die Spalten (CSS Grid 1
        // §8.3 nennt keine Ausnahme — die Spurengroesse ist eine
        // Listenangabe wie bei den Spalten).
        Eigenschaft::RasterZeilen => erg.raster_zeilen = p.raster_zeilen.clone(),
        // L4 (21:5x): §7.5/§7.3 nennen keine Ausnahme von der
        // Vererbung, und die Listen sind wie die Vorlagen eine
        // Groessenangabe des Containers.
        Eigenschaft::RasterAutoZeilen => erg.raster_auto_zeilen = p.raster_auto_zeilen.clone(),
        Eigenschaft::RasterAutoSpalten => erg.raster_auto_spalten = p.raster_auto_spalten.clone(),
        // background-image ist NICHT vererbt (CSS Backgrounds 3, §2.1);
        // `inherit` holt es trotzdem aus dem Eltern, das ist der Sinn
        // des Schluesselworts.
        // `background-image` erbt sich nicht (CSS Backgrounds 3 §2.1),
        // `inherit` holt es aber.
        Eigenschaft::HintergrundBild => {
            erg.bild = p.bild.clone();
            erg.verlauf = p.verlauf.clone();
        }

        // ---- K7c: die 27 neuen Eigenschaften ----
        //
        // KEINE davon erbt sich normal, und das steht nicht in einem
        // Kommentar, sondern in den Spezifikationen:
        //
        //   * `letter-spacing`, `word-spacing`, `text-indent`,
        //     `text-transform`, `text-overflow`, `word-break`,
        //     `overflow-wrap`, `line-break`, `hyphens` — CSS Text 4
        //     §2.1 fuehrt sie nicht als erblich auf.
        //   * `order` — CSS Flexbox 1 §2.7: es sortiert die KINDER,
        //     nicht deren Inhalt.
        //   * `grid-column*`/`grid-row*` — CSS Grid 1 §6: die
        //     Platzierung gehoert dem Kind.
        //   * `overflow-x`/`-y` — CSS Overflow 3 §2: Ueberlauf
        //     gehoert dem Kasten.
        //   * `object-fit`, `aspect-ratio`, `border-collapse`,
        //     `table-layout`, `tab-size` — je eigene Anfangswerte.
        //
        // `inherit` holt sie TROTZDEM (CSS Cascade 6 §5.2.2) — der
        // Unterschied zwischen „nicht vererbt" und „unmoeglich".
        Eigenschaft::LetterSpacing => {}
        Eigenschaft::Wortabstand => {}
        Eigenschaft::TextEinzug => {}
        Eigenschaft::Tabulator => {}
        Eigenschaft::TextTransform => {}
        Eigenschaft::TextUeberlauf => {}
        Eigenschaft::WortUmbruch => {}
        Eigenschaft::WortTrennung => {}
        Eigenschaft::ZeilenTrennung => {}
        Eigenschaft::GitterSpalteStart => {}
        Eigenschaft::GitterSpalteEnde => {}
        Eigenschaft::GitterZeileStart => {}
        Eigenschaft::GitterZeileEnde => {}
        Eigenschaft::GitterGebiete => {}
        // L4 (02:9x): `grid-area` erbt ebenfalls nicht (§8.4: „Inherited:
        // no“) und der Anfangswert ist `auto` — beides ist das leere Feld.
        Eigenschaft::GitterGebiet => {}
        Eigenschaft::GitterVorlagen => {}
        Eigenschaft::GitterAutomatik => {}
        Eigenschaft::Ordnung => {}
        Eigenschaft::LueckeSpalte => {}
        Eigenschaft::LueckeZeile => {}
        Eigenschaft::OverflowX => {}
        Eigenschaft::OverflowY => {}
        Eigenschaft::ObjektPassung => {}
        Eigenschaft::Seitenverhaeltnis => {}
        Eigenschaft::TabellenKanten => {}
        Eigenschaft::TabellenLage => {}
        Eigenschaft::Schluessel => {}
        // K6: `inherit` holt den Elternwert — bei ALLEN neuen
        // Eigenschaften, obwohl keine davon normal vererbt wird
        // (CSS Backgrounds 3 §2.1, CSS UI 4 §6, CSS Color 4 §3.2).
        // Das ist der Unterschied zwischen "nicht vererbt" (der
        // Normalfall) und "unmoeglich" (gibt es nicht).
        //
        // GEMESSENER FEHLER 01.10.2026: hier stand `=> {}`, und
        // `border-radius: inherit` holte nichts.
        //
        // `inherit` auf eine Ecke holt diese Ecke — nicht alle
        // vier. Das ist der Unterschied zu `border-radius: inherit`,
        // das es nicht gibt (die Kurzform expandiert auf vier
        // Ecken, jede holt ihr Feld).
        Eigenschaft::RadiusObenLinks
        | Eigenschaft::RadiusObenRechts
        | Eigenschaft::RadiusUntenRechts
        | Eigenschaft::RadiusUntenLinks => {
            let i = match e {
                Eigenschaft::RadiusObenLinks => 0,
                Eigenschaft::RadiusObenRechts => 1,
                Eigenschaft::RadiusUntenRechts => 2,
                _ => 3,
            };
            erg.radius[i] = p.radius[i];
            // Release 15.10.2026 (Agent F): the elliptical radii of the corner.
            erg.radius_xy[i] = p.radius_xy[i];
            let bits = (1u8 << i) | (1u8 << (4 + i));
            erg.radius_percent = (erg.radius_percent & !bits) | (p.radius_percent & bits);
        }
        Eigenschaft::Deckkraft => erg.deckkraft = p.deckkraft,
        Eigenschaft::Schatten => erg.schatten = p.schatten.clone(),
        Eigenschaft::OutlineBreite => erg.outline_breite = p.outline_breite,
        Eigenschaft::OutlineFarbe => erg.outline_farbe = p.outline_farbe,
        // `outline-offset` hat keinen eigenen Platz in `Berechnet` —
        // der Rasterizer liest ihn aus dem Rohtext. `inherit` heisst
        // hier also: nichts tun.
        Eigenschaft::OutlineOffset => {}
        Eigenschaft::VertikalAlign => erg.vertikal = p.vertikal.clone(),
        // K7: auch hier gilt „nicht vererbt" != „unmoeglich" —
        // `inherit` holt den Elternwert (K6, gleiche Ueberlegung).
        // `outline-offset` fehlt, weil es kein Feld in `Berechnet`
        // hat (es ist ein Abstand fuer den Rasterizer, kein Layout).
        Eigenschaft::BoxArt => erg.box_art = p.box_art,
        Eigenschaft::BreiteArt => erg.breite_art = p.breite_art.clone(),
        Eigenschaft::MaskenBild => erg.masken = p.masken.clone(),
        Eigenschaft::MaskenGroesse => {
            if !p.masken.is_empty() {
                erg.masken = p.masken.clone();
            }
        }
        Eigenschaft::MaskenPosition | Eigenschaft::MaskenWiederholen => {
            if !p.masken.is_empty() {
                erg.masken = p.masken.clone();
            }
        }
        Eigenschaft::BildGroesse | Eigenschaft::BildPosition | Eigenschaft::BildWiederholen => {
            erg.bild_geo = p.bild_geo;
        }
        // `background-blend-mode` (CSS Compositing 1 §3.1) — nicht vererbt,
        // aber `inherit` holt den Elternwert (CSS Cascade 6 §5.2.2).
        Eigenschaft::HintergrundMischung => {
            erg.mischmodus = p.mischmodus;
        }
    }
}

/// initial: Anfangswert der Eigenschaft (aus `standard`, vererbte
/// Aehnlichkeit spielt keine Rolle).
fn anfang(e: Eigenschaft, erg: &mut Berechnet) {
    prozent_zuruecksetzen(e, erg);
    // Release 11.10.2026: die Rasterwerte (`crate::gitter`).
    crate::gitter::kaskade_anfang(e, erg);
    let std = Berechnet::standard();
    match e {
        Eigenschaft::Anzeige => {
            erg.anzeige = std.anzeige;
            erg.nur_inhalt = false;
            erg.inline_aussen = false;
            erg.raster_lanes = false;
        }
        Eigenschaft::Farbe => erg.farbe = std.farbe,
        Eigenschaft::Hintergrund => erg.hintergrund = std.hintergrund,
        Eigenschaft::Schriftgroesse => {
            erg.schriftgroesse_px = std.schriftgroesse_px;
        }
        Eigenschaft::Schriftgewicht => erg.schriftgewicht = std.schriftgewicht,
        Eigenschaft::Schriftstil => erg.schriftstil = std.schriftstil,
        Eigenschaft::Schriftfamilie => erg.schriftfamilie = std.schriftfamilie,
        Eigenschaft::Zeilenhoehe => { erg.zeilenhoehe_px = std.zeilenhoehe_px; erg.zeilenhoehe_art = std.zeilenhoehe_art; },
        Eigenschaft::Textausrichtung => {
            erg.textausrichtung = std.textausrichtung;
            erg.blocksatz_alle = false;
            erg.ausrichtung_logisch = Some(true);
        }
        Eigenschaft::Textdekoration => erg.dekoration = std.dekoration,
        Eigenschaft::Weissraum => erg.weissraum = std.weissraum,
        Eigenschaft::MargeOben => erg.marge_oben = std.marge_oben,
        // `margin: initial` / `margin: revert` und der Fall „gar keine
        // Angabe": der Anfangswert ist `0` und ausdruecklich NICHT `auto`
        // (§8.3), also muessen die Flaggen hier ausdruecklich LOESCHEN —
        // sonst zentrierte der Motor einen Knoten, der einfach keinen
        // Rand angegeben hat.
        Eigenschaft::MargeRechts => {
            erg.marge_rechts = std.marge_rechts;
            erg.marge_rechts_auto = std.marge_rechts_auto;
        }
        Eigenschaft::MargeUnten => erg.marge_unten = std.marge_unten,
        Eigenschaft::MargeLinks => {
            erg.marge_links = std.marge_links;
            erg.marge_links_auto = std.marge_links_auto;
        }
        // K17b: Das UA-Blatt (dieses Browser-Standardblatt) benutzt
        // keine logischen Margen — die Zahl ist 0. Gespiegelt wird
        // trotzdem, damit ein spaeteres UA-Blatt mit
        // `margin-inline-start` nicht an der falschen Seite landet.
        Eigenschaft::MargeInnenStart => {
            if erg.richtung.start_ist_rechts() {
                erg.marge_rechts = std.marge_rechts;
                erg.marge_rechts_auto = std.marge_rechts_auto;
            } else {
                erg.marge_links = std.marge_links;
                erg.marge_links_auto = std.marge_links_auto;
            }
        }
        Eigenschaft::MargeInnenEnde => {
            if erg.richtung.start_ist_rechts() {
                erg.marge_links = std.marge_links;
                erg.marge_links_auto = std.marge_links_auto;
            } else {
                erg.marge_rechts = std.marge_rechts;
                erg.marge_rechts_auto = std.marge_rechts_auto;
            }
        }
        Eigenschaft::FuellungInnenStart => {
            if erg.richtung.start_ist_rechts() {
                erg.fuellung_rechts = std.fuellung_rechts;
            } else {
                erg.fuellung_links = std.fuellung_links;
            }
        }
        Eigenschaft::FuellungInnenEnde => {
            if erg.richtung.start_ist_rechts() {
                erg.fuellung_links = std.fuellung_links;
            } else {
                erg.fuellung_rechts = std.fuellung_rechts;
            }
        }
        // L4 (11:5x): `justify-items` hat den Anfangswert `stretch`
        // (§6.2); `align-self`/`justify-self` stehen auf `auto` (= `None`).
        Eigenschaft::JustifyAusrichtung => erg.justify_ausrichtung = std.justify_ausrichtung,
        // §17.6: Anfangswert `0`, nicht erblich.
        Eigenschaft::RandAbstandWaagerecht => {
            erg.randabstand_waagerecht = std.randabstand_waagerecht
        }
        Eigenschaft::RandAbstandSenkrecht => erg.randabstand_senkrecht = std.randabstand_senkrecht,
        // L4 (09:5x): §6.6 — der Anfangswert ist `stretch`.
        Eigenschaft::AusrichtungSelbst => erg.ausrichtung_selbst = std.ausrichtung_selbst,
        Eigenschaft::AusrichtungSelbstWaagerecht => {
            erg.ausrichtung_selbst_waagerecht = std.ausrichtung_selbst_waagerecht;
        }
        Eigenschaft::FuellungOben => erg.fuellung_oben = std.fuellung_oben,
        Eigenschaft::FuellungRechts => erg.fuellung_rechts = std.fuellung_rechts,
        Eigenschaft::FuellungUnten => erg.fuellung_unten = std.fuellung_unten,
        Eigenschaft::FuellungLinks => erg.fuellung_links = std.fuellung_links,
        Eigenschaft::RahmenObenBreite => erg.rahmen_oben_breite = std.rahmen_oben_breite,
        Eigenschaft::RahmenRechtsBreite => erg.rahmen_rechts_breite = std.rahmen_rechts_breite,
        Eigenschaft::RahmenUntenBreite => erg.rahmen_unten_breite = std.rahmen_unten_breite,
        Eigenschaft::RahmenLinksBreite => erg.rahmen_links_breite = std.rahmen_links_breite,
        Eigenschaft::RahmenObenStil => erg.rahmen_oben_stil = std.rahmen_oben_stil,
        Eigenschaft::RahmenRechtsStil => erg.rahmen_rechts_stil = std.rahmen_rechts_stil,
        Eigenschaft::RahmenUntenStil => erg.rahmen_unten_stil = std.rahmen_unten_stil,
        Eigenschaft::RahmenLinksStil => erg.rahmen_links_stil = std.rahmen_links_stil,
        Eigenschaft::RahmenObenFarbe => erg.rahmen_oben_farbe = std.rahmen_oben_farbe,
        Eigenschaft::RahmenRechtsFarbe => erg.rahmen_rechts_farbe = std.rahmen_rechts_farbe,
        Eigenschaft::RahmenUntenFarbe => erg.rahmen_unten_farbe = std.rahmen_unten_farbe,
        Eigenschaft::RahmenLinksFarbe => erg.rahmen_links_farbe = std.rahmen_links_farbe,
        Eigenschaft::Breite => {
            erg.breite = std.breite;
            erg.breite_art = std.breite_art.clone();
        }
        Eigenschaft::MaxBreite => erg.max_breite = std.max_breite,
        Eigenschaft::MinBreite => erg.min_breite = std.min_breite,
        Eigenschaft::Hoehe => {
            erg.hoehe = std.hoehe;
            erg.hoehe_inhalt = false;
        }
        Eigenschaft::MinHoehe => erg.min_hoehe = std.min_hoehe,
        Eigenschaft::MaxHoehe => erg.max_hoehe = std.max_hoehe,
        Eigenschaft::Listenstil => erg.listenstil = std.listenstil,
        Eigenschaft::Sichtbarkeit => erg.sichtbar = std.sichtbar,
        Eigenschaft::Ueberlauf => erg.ueberlauf = std.ueberlauf,
        Eigenschaft::FlexRichtung => erg.flex_richtung = std.flex_richtung,
        Eigenschaft::FlexBuendig => erg.flex_buendig = std.flex_buendig,
        Eigenschaft::FlexAusrichtung => erg.flex_ausrichtung = std.flex_ausrichtung,
        // L3 (04:5x): `gap: initial` / `revert` setzt BEIDE Achsen
        // zurueck. Ohne das bliebe nach `div{gap:20px; gap:initial}` eine
        // halbe Kurzform stehen — das genau ist der Fehler, den die
        // Kurzform an anderer Stelle (Vererbung) einbaute.
        Eigenschaft::Luecke => {
            erg.luecke_px = std.luecke_px;
            erg.luecke_spalte = std.luecke_spalte;
            erg.luecke_zeile = std.luecke_zeile;
        }
        Eigenschaft::FlexWachstum => erg.flex_wachstum = std.flex_wachstum,
        Eigenschaft::FlexSchrumpf => erg.flex_schrumpf = std.flex_schrumpf,
        Eigenschaft::FlexBasis => {
            erg.flex_basis = std.flex_basis;
            erg.flex_basis_art = std.flex_basis_art;
        }
        Eigenschaft::FlexUmbruch => {
            erg.flex_umbruch = std.flex_umbruch;
            erg.flex_umbruch_umgekehrt = std.flex_umbruch_umgekehrt;
            erg.flex_umbruch_ausgleich = std.flex_umbruch_ausgleich;
        }
        Eigenschaft::Position => erg.position = std.position,
        Eigenschaft::Oben => {
            erg.oben = std.oben;
            erg.oben_prozent = std.oben_prozent;
        }
        Eigenschaft::Rechts => {
            erg.rechts = std.rechts;
            erg.rechts_prozent = std.rechts_prozent;
        }
        Eigenschaft::Unten => {
            erg.unten = std.unten;
            erg.unten_prozent = std.unten_prozent;
        }
        Eigenschaft::Links => {
            erg.links = std.links;
            erg.links_prozent = std.links_prozent;
        }
        Eigenschaft::ZIndex => erg.z_index = std.z_index,
        Eigenschaft::Schweben => erg.schweben = std.schweben,
        Eigenschaft::Raeumung => erg.raeumung = std.raeumung,
        Eigenschaft::RasterSpalten => erg.raster_spalten = std.raster_spalten.clone(),
        // L4 (18:2x): Anfangswert der Zeilen ist die LEERE Liste.
        Eigenschaft::RasterZeilen => erg.raster_zeilen = std.raster_zeilen.clone(),
        // L4 (21:5x): Anfangswert ist die LEERE Liste, und eine leere
        // Liste heisst `auto` (§7.5) — die Inhaltshoehe bzw. die untere
        // Schranke aus Bericht 21:4x.
        Eigenschaft::RasterAutoZeilen => erg.raster_auto_zeilen = std.raster_auto_zeilen.clone(),
        Eigenschaft::RasterAutoSpalten => erg.raster_auto_spalten = std.raster_auto_spalten.clone(),
        // `initial` heisst fuer background-image `none`.
        // `initial` von `background-image` ist `none` (CSS Backgrounds
        // 3 §2.1) — und die Geometrie bleibt, weil `background-size`
        // eine EIGENE Eigenschaft mit eigenem Anfangswert ist.
        Eigenschaft::HintergrundBild => {
            erg.bild = BildQuelle::Keins;
            erg.verlauf = None;
        }
        // `initial`: der Anfangswert jeder der neuen Eigenschaften.
        Eigenschaft::RadiusObenLinks
        | Eigenschaft::RadiusObenRechts
        | Eigenschaft::RadiusUntenLinks
        | Eigenschaft::RadiusUntenRechts => {
            erg.radius = std.radius;
            erg.radius_xy = std.radius_xy;
            erg.radius_percent = std.radius_percent;
        }
        Eigenschaft::Deckkraft => erg.deckkraft = std.deckkraft,
        Eigenschaft::Schatten => erg.schatten = std.schatten,
        Eigenschaft::OutlineBreite => erg.outline_breite = std.outline_breite,
        Eigenschaft::OutlineFarbe => erg.outline_farbe = std.outline_farbe,
        // `outline-offset` hat kein Feld in `Berechnet` (es ist ein
        // Abstand fuer den Rasterizer, kein Layout) — hier gibt es
        // darum nichts zu setzen.
        Eigenschaft::OutlineOffset => {}
        Eigenschaft::VertikalAlign => erg.vertikal = std.vertikal.clone(),

        // ---- K7c: `initial` = der Anfangswert je Feld ----
        //
        // Das ist nicht `{}`: `tab-size: initial` heisst 8, nicht 0;
        // `overflow-x: initial` heisst `visible`, nicht `hidden`.
        // Wer das leer laesst, laesst den Wert des vorherigen Gewinners
        // stehen, und `initial` waere wirkungslos.
        Eigenschaft::LetterSpacing => {}
        Eigenschaft::Wortabstand => {}
        Eigenschaft::TextEinzug => {}
        Eigenschaft::Tabulator => {}
        Eigenschaft::TextTransform => {}
        Eigenschaft::TextUeberlauf => {}
        Eigenschaft::WortUmbruch => {}
        Eigenschaft::WortTrennung => {}
        Eigenschaft::ZeilenTrennung => {}
        Eigenschaft::GitterSpalteStart => {}
        Eigenschaft::GitterSpalteEnde => {}
        Eigenschaft::GitterZeileStart => {}
        Eigenschaft::GitterZeileEnde => {}
        Eigenschaft::GitterGebiete => {}
        // L4 (02:9x): `grid-area` erbt ebenfalls nicht (§8.4: „Inherited:
        // no“) und der Anfangswert ist `auto` — beides ist das leere Feld.
        Eigenschaft::GitterGebiet => {}
        Eigenschaft::GitterVorlagen => {}
        Eigenschaft::GitterAutomatik => {}
        Eigenschaft::Ordnung => {}
        Eigenschaft::LueckeSpalte => {}
        Eigenschaft::LueckeZeile => {}
        Eigenschaft::OverflowX => {}
        Eigenschaft::OverflowY => {}
        Eigenschaft::ObjektPassung => {}
        Eigenschaft::Seitenverhaeltnis => {}
        Eigenschaft::TabellenKanten => {}
        Eigenschaft::TabellenLage => {}
        Eigenschaft::Schluessel => {}
        // K7: `initial` = der Anfangswert jeder neuen Eigenschaft.
        Eigenschaft::BoxArt => erg.box_art = std.box_art,
        Eigenschaft::BreiteArt => erg.breite_art = std.breite_art.clone(),
        // `mask-image: none` (CSS Masking 1 §4.1) ist eine Ebene mit
        // leerer URL — NICHT eine leere Liste. Im Bild ist das gleich,
        // in der Anzahl der Ebenen nicht; und eine leere Liste wuerde
        // das `!p.masken.is_empty()` in `erbe` falsch machen.
        Eigenschaft::MaskenBild => erg.masken = std.masken.clone(),
        // Die drei Langformen aendern nur die BESTEHENDEN Ebenen; gibt
        // es keine, gibt es auch nichts zu aendern.
        Eigenschaft::MaskenGroesse
        | Eigenschaft::MaskenPosition
        | Eigenschaft::MaskenWiederholen => {}
        // `background-size`/`-position`/`-repeat` sind eigene
        // Eigenschaften mit eigenen Anfangswerten: `background-size`
        // heisst `auto`, `-position` heisst `0% 0%`, `-repeat` heisst
        // `repeat` (CSS Backgrounds 3 §2.1).
        Eigenschaft::BildGroesse => {
            erg.bild_geo.groesse = std.bild_geo.groesse;
            erg.bild_geo.cover = std.bild_geo.cover;
            erg.bild_geo.contain = std.bild_geo.contain;
            erg.bild_geo.groesse_xy = std.bild_geo.groesse_xy;
        }
        Eigenschaft::BildPosition => {
            erg.bild_geo.position_x = std.bild_geo.position_x;
            erg.bild_geo.position_y = std.bild_geo.position_y;
            erg.bild_geo.position_px = std.bild_geo.position_px;
            erg.bild_geo.position_px_achsen = std.bild_geo.position_px_achsen;
            erg.bild_geo.vom_ende = std.bild_geo.vom_ende;
        }
        Eigenschaft::BildWiederholen => {
            erg.bild_geo.wiederholen_x = std.bild_geo.wiederholen_x;
            erg.bild_geo.wiederhol_art = std.bild_geo.wiederhol_art;
            erg.bild_geo.wiederholen_y = std.bild_geo.wiederholen_y;
        }
        // `background-blend-mode` (CSS Compositing 1 §3.1) — Anfangswert
        // `normal` (steht in `Berechnet::standard`).
        Eigenschaft::HintergrundMischung => {
            erg.mischmodus = std.mischmodus;
        }
    }
}

// ===========================================================================
// TESTS — die Verbindung, die der Compiler nicht pruefen kann
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// K11 (02.10.2026 20:00): WIE VIELE BYTE LEGT EIN `berechnen` AN,
    /// BEVOR IRGENDEINE REGEL GELESEN WIRD?
    ///
    /// GEMESSENER Anlass: die Zaehler je Knoten (Nachtrag 19:40) zeigen
    /// 975 Vorpruefungen, 347 `bieten`, 511 Selektorvergleiche — aber
    /// nur 101 `loese_eigenschaft` und 4.8 `effektiver_wert`. Die Zeit
    /// geht fast vollstaendig fuer ENTSCHEIDEN drauf. Bevor man
    /// entscheidet, will man wissen, was der ENTSCHEIDUNG vorausgeht.
    ///
    /// `bieten` bekommt `gebote: &mut [[Option<Gebot>; 6]; EIGENSCHAFTEN]`.
    /// Das ist eine flache Tabelle, die der Aufrufer FUENKT und
    /// aufbauen muss — also bei JEDEM `berechnen` neu. Bei 975 Regeln
    /// ist die Zahl der Regeln fuer diese Groesse egal.
    #[test]
    fn wie_gross_ist_die_gebote_tabelle() {
        use std::mem::size_of;
        let g = size_of::<Gebot>();
        let og = size_of::<Option<Gebot>>();
        let zeile = size_of::<[Option<Gebot>; 6]>();
        let tabelle = size_of::<[[Option<Gebot>; 6]; EIGENSCHAFTEN]>();
        let slots = EIGENSCHAFTEN * 6;
        println!("GROSSE: Gebot={g} B  Option<Gebot>={og} B  [Option<Gebot>;6]={zeile} B");
        println!(
            "GROSSE: [[Option<Gebot>;6];{}] = {tabelle} B = {:.1} kB  ({slots} Slots)",
            EIGENSCHAFTEN,
            tabelle as f64 / 1024.0
        );
        // Und was das ueber fuenf Tausend Knoten bedeutet:
        println!(
            "GROSSE: 5000 Knoten wuerden {} MB belegen, ohne dass EINE Regel gelesen wird",
            tabelle as u64 * 5000 / 1_000_000
        );
        // Der eigentliche Punkt ist aber eine Obergrenze: wenn die
        // Tabelle kilobyteweise ist, ist das Anlegen und Nullen je Knoten
        // ein postenster Anteil, unabhaengig von der Regelzahl.
        //
        // K17b: 618 -> 642. Die vier logischen Margen/Fuellungen
        // kosten 24 Slots je Knoten. GEMESSEN ist die Groesse ueber
        // `size_of_val` in diesem Test — der Aufruf oben gibt sie aus,
        // also ist die Zahl nicht geraten.
        //
        // Warum 24 und nicht weniger: Der Kaskaden-Slot ist
        // `[[Option<Gebot>; 6]; EIGENSCHAFTEN]`, und `EIGENSCHAFTEN`
        // ist jetzt 107 statt 103. 6 Rangaenge * 4 Eigenschaften = 24
        // Slots. Jeder Slot ist ein `Option<Gebot>` mit `Gebot` = Rang
        // (u8) + Schicht (u16) + Spez (3x u32) + Reihe (u64) + Quelle
        // (Enum mit Index-Varianten) — 32 Byte.
        //
        // L4 (02.10.2026 18:4x): 642 -> 648, weil `grid-template-rows`
        // dazugekommen ist. `EIGENSCHAFTEN` ist 108 statt 107, und
        // 6 Rangaenge * 1 Eigenschaft = 6 Slots zu 32 Byte = 192 Byte je
        // Knoten. GEMESSEN: die Zahl unten ist `size_of_val`, nicht
        // geraten — der Test gibt sie aus und vergleicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das erwaehnt wird und nicht
        // stillschweigend angepasst: Jede Eigenschaft kostet Platz je
        // Knoten, und das ist eine **Skalenzahl**. 648 Slots * 32 Byte
        // = 20,7 kB je Knoten; bei 5 000 Knoten sind das 104 MB. Das ist
        // die Rechnung, die dieser Test bewacht — und sie ist der Grund,
        // warum `grid-template-rows` ein **eigenes Feld in `Berechnet`**
        // bekommen hat und nicht noch eine Eigenschaft in der Tabelle
        // mit Spalten, Zeilen, Bereichen und Breiten zusammen.
        // L4 (02.10.2026 21:5x): 648 -> 660, weil `grid-auto-rows` und
        // `grid-auto-columns` dazugekommen sind. 2 Eigenschaften *
        // 6 Rangaenge = 12 Slots zu 32 Byte = 384 Byte je Knoten.
        // GEMESSEN: die Zahl unten ist `size_of_val`, der Test gibt sie
        // aus und vergleicht — GEMESSEN sind 660.
        //
        // GEMESSENE ENTSCHEIDUNG, warum ich die Rechnung hinschreibe und
        // sie nicht weglasse: 660 * 32 Byte = 21,1 kB je Knoten, bei
        // 5000 Knoten 105 MB. Das ist die Skalenzahl dieses Tests, und
        // sie ist der Grund, warum die Groesse der impliziten Spuren
        // EINE Liste je Achse ist und nicht „eine Zahl, die man bei Bedarf
        // nachlaedt“.
        // L4 (03.10.2026 02:9x): 660 -> 666, weil `grid-area` als eigene
        // Eigenschaft dazugekommen ist (vorher in `schluessel`).
        // 1 * 6 Rangaenge = 6 Slots zu 32 Byte = 192 Byte je Knoten.
        // GEMESSEN: die Zahl unten ist `size_of_val`.
        //
        // L4 (03.10.2026 09:5x): 666 -> **678**, weil `align-self` und
        // `justify-self` dazugekommen sind (§6.6; sie lagen vorher im
        // Schluessel-Behaelter, also in keinem Feld).  2 * 6 Rangaenge =
        // 12 Slots.  GEMESSEN: der Test ist genau hier rot geworden — er
        // ist der Ort, an dem die Groessenrechnung des Berichts
        // nachgezogen wird.
        //
        // L4 (03.10.2026 11:5x): 678 -> **684**, weil `justify-items`
        // dazugekommt (§6.2; es lag vorher im Schluessel-Behaelter).
        // 1 * 6 Rangaenge = 6 Slots.  GEMESSENE ZAHL: die Testausgabe
        // `[[Option<Gebot>;6];114] = 49248 B = 48.1 kB  (684 Slots)`
        // (114 Eigenschaften, `size_of_val` je 72 Byte) — 49248 / 72 =
        // 684.  GEMESSENE REGEL: diese Zahl wird nicht geschaetzt,
        // sondern aus der Ausgabe des roten Tests gelesen.
        //
        // 03.10.2026 (Rolle CSS): 684 -> **696**, weil `border-spacing`
        // dazukommt (§17.6). GEMESSENE ZAHL: die Testausgabe
        // `[[Option<Gebot>;6];116] = 50112 B = 48.9 kB  (696 Slots)`
        // (116 Eigenschaften) — 50112 / 72 = 696. Nach derselben Regel
        // abgelesen, nicht geschaetzt: 2 * 6 Rangaenge = 12 Slots.
        // 07.10.2026 (K7c): 696 -> **702**, weil `background-blend-mode`
        // dazukommt (CSS Compositing 1 §3.1). 1 * 6 Rangaenge = 6 Slots.
        // GEMESSENE ZAHL: die Testausgabe
        // `[[Option<Gebot>;6];117] = 50544 B = 49.4 kB  (702 Slots)`
        // (117 Eigenschaften) — 50544 / 72 = 702.
        assert!(
            slots == 702,
            "702 Slots je Knoten (696 + background-blend-mode, je 6 Rangaenge) — \
             weicht ab, dann stimmt die Rechnung oben nicht"
        );

        // K11 (02.10.2026 21:40): die zweite Struktur, die je Knoten
        // ANFALLT — `Berechnet`. Sie wird von `berechnen` BY VALUE
        // zurueckgegeben, also je Aufruf kopiert, und `eltern:
        // Option<&Berechnet>` zeigt auf die Kopie des Elternknotens.
        let b = size_of::<Berechnet>();
        let ob = size_of::<Option<&Berechnet>>();
        println!(
            "GROSSE: Berechnet={b} B = {:.2} kB, Option<&Berechnet>={ob} B",
            b as f64 / 1024.0
        );
        println!(
            "GROSSE: eine Kaskade ueber drei Ebenen legt {} Byte an Kopien an",
            b * 3
        );
        // Und die Groesse, an der `berechnen_mit` vorbeikommt: die
        // Initialwerte.
        println!(
            "GROSSE: Berechnet::standard() wird {} mal je Knoten gebaut (Phase E)",
            1
        );
    }

    /// `eigenschaft_index` muss die POSITION in [`alle_eigenschaften`]
    /// sein, und jede Position genau einmal.
    ///
    /// GEMESSENER FEHLER 01.10.2026: `Sichtbarkeit`, `Ueberlauf` und
    /// `FlexRichtung` waren auf 39/40/41 abgebildet — dieselben
    /// Nummern wie `FlexBuendig`, `FlexAusrichtung` und `Luecke`.
    /// Sechs Eigenschaften, drei Faecher. `merke` ist je Fach ein
    /// Wettbewerb, also hat die spaeter gefundene Deklaration die
    /// andere verdraengt, und `visibility: hidden` neben
    /// `justify-content: center` in einer Regel hat das Element
    /// sichtbar gemacht (Beleg: crates/narsil-css/tests/k1_facherrand.rs,
    /// 6 von 6 Tests rot vor dem Beheben).
    ///
    /// WARUM DIESER TEST UNTE RDIE ZWEI ANDEREN STEHT: `loese_eigenschaft`
    /// hat ein vollstaendiges `match` ueber alle 55 Varianten — dort
    /// meldet der Compiler jede neue Eigenschaft. Diese Tabelle ist
    /// die einzige Stelle mit einer handgepflegten ZAHL, und eine Zahl
    /// ist fuer den Compiler nur eine Zahl.
    #[test]
    fn index_tabelle_ist_eindeutig() {
        let alle = alle_eigenschaften();
        // 1) Jede Eigenschaft steht auf ihrer eigenen Position.
        for (pos, e) in alle.iter().enumerate() {
            assert_eq!(
                eigenschaft_index(*e),
                pos,
                "{e:?} ist auf Index {} abgebildet, steht aber an Position {pos}",
                eigenschaft_index(*e)
            );
        }
        // 2) Und keine Position ist doppelt belegt. Das ist die
        //    eigentliche Zusicherung: eine doppelte Position faellt
        //    nicht auf, wenn eine Eigenschaft aus der Liste faellt.
        for (a, x) in alle.iter().enumerate() {
            for (b, y) in alle.iter().enumerate() {
                if a == b {
                    continue;
                }
                assert_ne!(
                    eigenschaft_index(*x),
                    eigenschaft_index(*y),
                    "{x:?} und {y:?} teilen sich Index {}",
                    eigenschaft_index(*x)
                );
            }
        }
        // 3) Und alle Indizes liegen im Feldraster.
        for e in alle.iter() {
            assert!(eigenschaft_index(*e) < EIGENSCHAFTEN);
        }
    }
}

/// `grid-column-start`/`-end` und `grid-row-start`/`-end` als Zahl
/// (CSS Grid 1 §8).
///
/// # WAS DRAUS EINER ZEIT GEMACHT WIRD
///
/// `auto` (der Anfangswert) wird 0. `span N` und `N` werden N.
/// Negative Werte sind Zeilen von unten (das ist die Spec, keine
/// Erfindung). `name` (ein benanntes Linienpaar) wird 0 und
/// GEZAEHLT — der Name passt zu keinem anderen Element, und eine Zahl
/// zu behaupten waere geraten.
///
/// Der Text kommt aus `Deklaration::roh`, nicht aus dem geparsten
/// Wert: bei `span 2` ist der geparste Wert eine Liste, und die Zahl
/// steckt im Wort.
/// Liest `grid-template-areas`: eine Liste anfuehrungszeichenbegrenzter
/// Zeilen, jede Zeile eine Liste von Namen (CSS Grid 1 §8.1).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zeilen an den ANFUEHRUNGSZEICHEN
/// getrennt werden und nicht am Leerraum: GEMESSEN im Korpus stehen die
/// Zeilen oft ohne Leerraum nebeneinander — `"text
/// trailingComponent""cta trailingComponent"` (4 Vorkommen). Ein
/// Leerraum-Zerlegen wuerde daraus EINE Zeile mit sechs Namen machen.
///
/// GEMESSENE ENTSCHEIDUNG, warum alle Zeilen gleich breit gemacht werden
/// (§8.1 verlangt das) und nicht die kuerzere: „If the grid has fewer
/// columns than the longest row, the extra columns are ignored" — es
/// heisst: alle Zeilen werden auf die laengste GEFUELLT. Eine kuerzere
/// Zeile ohne Namen zu ergaenzen ist die einzige Form, in der die
/// folgenden Zeilen nicht verrutschen.
fn parse_gebiete(x: &str) -> Vec<Vec<crate::typen::Gebiet>> {
    let mut zeilen: Vec<Vec<crate::typen::Gebiet>> = Vec::new();
    let mut aktuell: Vec<crate::typen::Gebiet> = Vec::new();
    let mut rest = x.trim();
    while let Some(open) = rest.find('"') {
        let nach = &rest[open + 1..];
        // GEMESSENE ENTSCHEIDUNG, warum ein nicht geschlossenes
        // Anfuehrungszeichen den Rest VERWORFEN laesst und nicht als
        // Zeile liest: `"abc` ist keine Zeile, sondern ein Tippfehler, und
        // §8.1 nennt fuer jede Zeile ein schliessendes Anfuehrungszeichen.
        let Some(close) = nach.find('"') else {
            return Vec::new();
        };
        for name in nach[..close].split_whitespace() {
            let mut g = crate::typen::Gebiet::LEER;
            g.set(name);
            //
            // L4 (04.10.2026 03:1x): `.` BLEIBT ALS ZELLE STEHEN.
            //
            // GEMESSENER FEHLER (`CSS_21.txt`, Fall 2): in
            // `grid-template-areas:'a . c'` setzte Firefox den dritten
            // Bereich auf **x 220** (die dritte Spur), wir auf **x 0**.
            //
            // GEMESSENE URSACHE, und sie steht in diesem Kommentar: `.`
            // wurde **uebersprungen**. Damit rutschte jedes folgende Gebiet
            // eine Spalte nach links — `c` aus Spalte 2 nach Spalte 1.
            //
            // §8.1: `.` ist ein **leeres Gebiet** (gleichwertig `none`).
            // „Leer" heisst: **kein Element** wird dort platziert. Es heisst
            // nicht: die Zelle existiert nicht. Die Zelle **belegt ihre
            // Position**, sonst wuessten alle folgenden Spaltennummern
            // verrutscht — und genau das war der Fehler. Die Unterscheidung
            // „nicht belegt" und „nicht vorhanden" war hier verwechselt.
            aktuell.push(g);
        }
        zeilen.push(std::mem::take(&mut aktuell));
        rest = &nach[close + 1..];
    }
    if zeilen.is_empty() {
        return zeilen;
    }
    // §8.1: alle Zeilen auf die laengste bringen.
    let breit = zeilen.iter().map(Vec::len).max().unwrap_or(0);
    for z in zeilen.iter_mut() {
        while z.len() < breit {
            // GEMESSENE ENTSCHEIDUNG, warum das Fuellen mit LEEREN Namen
            // passiert und nicht mit `.`: ein leerer Name belegt nichts, und
            // §8.1 sagt, ein leeres Gebiet werde wie `none` behandelt.
            // Ein Name, der wie `.` aussieht, wuerde sonst ein Gebiet
            // erzeugen, das es nicht gibt.
            z.push(crate::typen::Gebiet::LEER);
        }
    }
    zeilen
}

/// Schreibt die Teile von `grid-area` in die vier LINIEN-Felder (§8.4).
///
/// L4 (03.10.2026 04:8x). §8.4: `grid-area: <row-start> /
/// <column-start> / <row-end> / <column-end>`.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge der Zuordnung nicht die
/// von `grid-row` ist: §8.4 listet genau diese vier, und ein zweites
/// Argument wäre geraten.
///
/// GEMESSENE ENTSCHEIDUNG, warum `span` in der ENDposition die Spannweite
/// setzt und nicht das Ende: §8.3 — „if the end of the grid area is a
/// span, the grid item spans that many tracks". GEMESSEN im Korpus:
/// `1/1/span 2/span 2` (2 Vorkommen).
///
/// GEMESSENE ENTSCHEIDUNG, warum mehr als vier Teile kein Fehler sind:
/// §8.4 nennt genau vier, also wird der Rest ignoriert — ein Name, den
/// wir nicht lesen koennen, darf die Angabe nicht verwerfen.
/// Die 1-basierte Linie mit diesem Namen (§7.2/§8.3), sonst `0`.
///
/// L4 (03.10.2026 06:1x). GEMESSENE ENTSCHEIDUNG, warum `0` statt `None`
/// zurueckkommt: `gitter_spalte_start` ist eine Zahl, und `0` bedeutet
/// dort seit 01:3x `auto`. Eine Option muesste an vier Stellen in eine
/// Zahl uebersetzt werden, und jede Uebersetzung ist eine Stelle, an der
/// `auto` zu einer Linie werden kann.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Suche die ERSTE Linie nimmt: §8.3
/// sagt „contributes the first such line". `namen[i]` traegt die Linie
/// `i + 1`, und die Liste ist nach Linien sortiert.
fn linie_durch_name(namen: &[Vec<Gebiet>], w: &str) -> i32 {
    for (i, liste) in namen.iter().enumerate() {
        if liste.iter().any(|g| g.gleich(w)) {
            return i as i32 + 1;
        }
    }
    0
}

/// Release 08.10.2026: CSS Grid 1 §8.3 — ein `<name>` an der START-Kante
/// sucht zuerst die Linie `<name>-start`, an der END-Kante `<name>-end`,
/// erst dann den Namen selbst. GEMESSEN vorher: MDN
/// `[content-start] … [content-end]` mit `grid-column: content` traf keine
/// Linie, der Inhalt landete rechts ausserhalb des Bildes.
fn linie_fuer_kante(namen: &[Vec<Gebiet>], w: &str, ende: bool) -> i32 {
    let endung = if ende { "-end" } else { "-start" };
    if !w.ends_with(endung) {
        let z = linie_durch_name(namen, &format!("{w}{endung}"));
        if z != 0 {
            return z;
        }
    }
    linie_durch_name(namen, w)
}

/// Welche Achse ein Teil einer Platzierung aufloest.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein Parameter ist und nicht aus dem
/// Property folgt: `grid-row: a/b` liest Teil 0 als ZEILENanfang und
/// Teil 1 als ZEILENende; `grid-area: a/b` liest Teil 1 als
/// SPALTENanfang (§8.4 — vier Teile in der Reihenfolge
/// Zeile/Spalte/Zeile/Spalte). Dieselbe Zahl an derselben Stelle, zwei
/// Bedeutungen.
#[derive(Clone, Copy, PartialEq)]
enum PlatzAchse {
    /// `grid-row` — beide Teile auf der Zeilenachse.
    NurZeile,
    /// `grid-column` — beide Teile auf der Spaltenachse.
    NurSpalte,
    /// `grid-area` — Teil 0 Zeile, 1 Spalte, 2 Zeile, 3 Spalte.
    Gitter,
    /// Release 08.10.2026: `grid-row-end` — der eine Teil ist die
    /// ZEILEN-ENDkante (vorher als Anfang gelesen).
    ZeileEnde,
    /// `grid-column-end` — der eine Teil ist die SPALTEN-ENDkante.
    /// GEMESSEN vorher: MDN `grid-column-end: extended-full-end` setzte
    /// den ANFANG auf die letzte Linie — Breite 0 bei x 884.
    SpalteEnde,
}

/// Loest die NAMEN einer Platzierung gegen die Linien des Containers auf
/// (§8.3).
///
/// GEMESSENER BEFUND, dass die Form nicht ankam: `gitter_linie` gibt fuer
/// einen Namen `0` (also `auto`) und zaehlt einen `verwurf` — der Name
/// ging verloren. GEMESSEN im Korpus (`crates/narsil-css/tests/daten/`, der
/// ganze Baum): `grid-area: content` 9x, `grid-area: visual` 5x,
/// `grid-row: header` 2x, `grid-row: leadingComponent` 2x.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Aufloesung in der KASKADE passiert
/// und nicht im Layout: die Namen stehen am CONTAINER
/// (`grid-template-columns`), und die Kaskade ist die einzige Stelle, die
/// beides sieht — `loese_eigenschaft` hat `eltern: Option<&Berechnet>`
/// (Zeile 1054). Der Weg durch `LayoutStil` und `RasterKind` waere
/// dieselbe Zahl an drei weiteren Structs.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Gebiet VOR dem Liniennamen
/// aufgeloest bleibt (§8.3: „First attempt to match the grid area's edge
/// to a named grid area"): die Kaskade kennt nur die Linien, nicht die
/// Gebietsrechtecke. Sie loest also NUR die Linie auf, und das Gebiet
/// gewinnt weiterhin im Layout, wo `im_gebiet` vor der Startlinie gelesen
/// wird (`raster.rs`). Der Vorrang bleibt an DER STELLE, die die Gebiete
/// kennt.
///
/// **DIE ABWEICHUNG VON §8.4 — sie ist eine bewusste und sie steht hier,
/// nicht im Bericht.**
///
/// §8.4 woertlich: „When grid-column-start is omitted, if grid-row-start
/// is a `<custom-ident>`, **all four** longhands are set to that value."
/// §8.3 fuer einen nackten `<custom-ident>`: erst `<name>-start`/
/// `<name>-end`, sonst „treat this as if the **integer 1** had been
/// specified along with the `<custom-ident>`".
///
/// Zusammen ergibt `grid-area: content` bei `[content]1fr[visual]1fr`
/// fuer BEIDE Kanten die Linie 1 — eine **leere** Grid-Area, und der
/// Inhalt waere unsichtbar.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier nur die STARTkanten gesetzt werden:
/// eine leere Grid-Area waere ein falsches Bild (nichts sichtbar), und
/// welcher der beiden Punkte von mir falsch gelesen ist, laesst sich
/// OHNE Browser nicht messen — `PROBLEME.txt` Nr. 51 sagt, warum (kein
/// Browser verbunden, `data:`-URLs abgewiesen). Also die kleinere
/// Loesung, die den Inhalt sichtbar laesst.
///
/// GEMESSENE ENTSCHEIDUNG, warum die STARTkante ueberhaupt gesetzt wird:
/// `grid-area: visual` bei `[content]1fr[visual]1fr` steht 5x im Korpus
/// und meint die zweite Spalte. Ohne Aufloesung legt der Cursor das Kind
/// dorthin, wo es gerade passt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die ENDKANTE gar nicht gebaut wird und
/// nicht etwa auf dieselbe Linie gesetzt wird: §8.4 wuerde genau das
/// tun, und dann waere die Flaeche leer. „Leer" ist hier keine
/// Kleinigkeit, sondern ein Loch im Bild.
/// Das Wort der Ausrichtung → `FlexAusrichtung` (§6.6).
///
/// L4 (09:5x). GEMESSENE ENTSCHEIDUNG, warum das eine Funktion ist und
/// kein `match` im Arm: `align-items`, `align-self` und `justify-self`
/// nennen dieselbe Wertemenge, und der Parser gibt allen drei dieselben
/// Woerter. Eine Funktion ist die eine Stelle, an der `baseline` und
/// `self-start` entschieden werden.
/// L4 (11:5x): `auto` → `None`, alles andere → der Wert.
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` und nicht `Strecke`: §6.3 —
/// `auto` bedeutet „den Wert des Containers uebernehmen", und `stretch`
/// ist ein eigener Wert, der den Container NICHT ueberschreibt. Mit einer
/// blossen Zahl kann die Kaskade das nicht unterscheiden, und der
/// Container koennte seinen Wert nie durchreichen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Aufrufer trotzdem denselben Namen
/// benutzen: `align-items` will KEIN `None` (dort gibt es keinen
/// Container, von dem man erben koennte) — das erledigt der Aufrufer mit
/// `unwrap_or(Strecke)`.
fn selbst_ausrichtung(w: &str) -> Option<FlexAusrichtung> {
    match w {
        "auto" => None,
        "start" | "flex-start" | "self-start" => Some(FlexAusrichtung::Start),
        "mitte" | "center" | "centre" => Some(FlexAusrichtung::Mitte),
        "ende" | "end" | "flex-end" | "self-end" => Some(FlexAusrichtung::Ende),
        "grundlinie" | "baseline" => Some(FlexAusrichtung::Grundlinie),
        // §6.6: `stretch` und `normal` verhalten sich gleich.
        _ => Some(FlexAusrichtung::Strecke),
    }
}

fn loese_liniennamen(
    erg: &mut Berechnet,
    teile: &[&str],
    achse: PlatzAchse,
    eltern: Option<&Berechnet>,
) {
    let Some(e) = eltern else {
        return;
    };
    if e.raster_zeilen_namen.is_empty() && e.raster_spalten_namen.is_empty() {
        return;
    }
    // §8.3: ein `span` ist keine Linie — der Rohwert wurde schon als
    // Spannweite gelesen, und hier stuende nur noch der Name.
    let namen_von =
        |idx: usize| -> Option<&str> { teile.get(idx).copied().filter(|w| !w.starts_with("span")) };
    // Teil 0: bei `grid-area` und `grid-row` der Zeilenanfang. §8.3 sagt
    // fuer `grid-area: content`: „contributes the first such line" — und
    // weil `grid-area` alle VIER Langformen setzt, steht derselbe Name
    // auch in der SPALTENangabe. GEMESSEN: `grid-area: content` loest also
    // in BEIDEN Achsen, jede gegen ihre eigene Liste.
    // Release 08.10.2026: die End-Longhands haben EINEN Teil, und der ist
    // die Endkante (siehe `PlatzAchse::SpalteEnde`).
    match achse {
        PlatzAchse::ZeileEnde => {
            if erg.gitter_zeile_ende == 0
                && let Some(w) = namen_von(0)
            {
                erg.gitter_zeile_ende = linie_fuer_kante(&e.raster_zeilen_namen, w, true);
            }
            return;
        }
        PlatzAchse::SpalteEnde => {
            if erg.gitter_spalte_ende == 0
                && let Some(w) = namen_von(0)
            {
                erg.gitter_spalte_ende = linie_fuer_kante(&e.raster_spalten_namen, w, true);
            }
            return;
        }
        _ => {}
    }
    if erg.gitter_zeile_start == 0
        && achse != PlatzAchse::NurSpalte
        && let Some(w) = namen_von(0)
    {
        erg.gitter_zeile_start = linie_fuer_kante(&e.raster_zeilen_namen, w, false);
    }
    //
    // L4 (04.10.2026 04:0x): BEIDE ENDEN AUFLOSEN, und je Achse die
    // richtigen Teile.
    //
    // GEMESSENER FEHLER (`CSS_22.txt`): `grid-column: links-l / inhalte-l`
    // gab **Breite 0 bei x 120**, Firefox **120 bei x 0**.
    //
    // GEMESSENE URSACHE: `gitter_spalte_start` und `gitter_spalte_ende`
    // werden beide ueber `gitter_linie(v)` gesetzt, und das gibt fuer einen
    // **Namen** die 0. Aufgeloest wurde bisher **nur der Anfang** — und das
    // aus **Teil 1**, nicht aus Teil 0. Damit bekam `links-l` nie eine
    // Linie, der Anfang landete auf `inhalte-l` (2) und das Ende blieb 0.
    //
    // §8.3: `grid-column: <anfang> / <ende>` — beide Teile sind
    // SPALTENlinien; `grid-row` entsprechend ZEILENlinien. §8.4: bei
    // `grid-area` steht derselbe Name in allen vier Langformen, Teil 0 ist
    // der Zeilenanfang (oben bereits aufgeloest) und Teil 1 der
    // SPALTENanfang.
    match achse {
        PlatzAchse::NurZeile => {
            if erg.gitter_zeile_ende == 0
                && let Some(w) = namen_von(1)
            {
                erg.gitter_zeile_ende = linie_fuer_kante(&e.raster_zeilen_namen, w, true);
            }
        }
        PlatzAchse::NurSpalte => {
            if erg.gitter_spalte_start == 0
                && let Some(w) = namen_von(0)
            {
                erg.gitter_spalte_start = linie_fuer_kante(&e.raster_spalten_namen, w, false);
            }
            if erg.gitter_spalte_ende == 0
                && let Some(w) = namen_von(1)
            {
                erg.gitter_spalte_ende = linie_fuer_kante(&e.raster_spalten_namen, w, true);
            }
        }
        PlatzAchse::Gitter => {
            let idx = if teile.len() == 1 { 0 } else { 1 };
            if erg.gitter_spalte_start == 0
                && let Some(w) = namen_von(idx)
            {
                erg.gitter_spalte_start = linie_fuer_kante(&e.raster_spalten_namen, w, false);
            }
        }
        PlatzAchse::ZeileEnde | PlatzAchse::SpalteEnde => {}
    }
    // §8.3.1: Anfang == Ende heisst „kein Ende" (eine Spur), nicht eine
    // leere Flaeche.
    if erg.gitter_spalte_ende != 0 && erg.gitter_spalte_ende == erg.gitter_spalte_start {
        erg.gitter_spalte_ende = 0;
    }
    if erg.gitter_zeile_ende != 0 && erg.gitter_zeile_ende == erg.gitter_zeile_start {
        erg.gitter_zeile_ende = 0;
    }
}

fn setze_grid_area_teile(erg: &mut crate::typen::Berechnet, teile: &[&str]) {
    if let Some(v) = teile.first() {
        match gitter_spanne(v) {
            Some(sp) => erg.gitter_zeile_spanne = sp,
            None => erg.gitter_zeile_start = gitter_linie(v),
        }
    }
    if let Some(v) = teile.get(1) {
        match gitter_spanne(v) {
            Some(sp) => erg.gitter_spalte_spanne = sp,
            None => erg.gitter_spalte_start = gitter_linie(v),
        }
    }
    if let Some(v) = teile.get(2) {
        match gitter_spanne(v) {
            Some(sp) => erg.gitter_zeile_spanne = sp.max(erg.gitter_zeile_spanne),
            None => erg.gitter_zeile_ende = gitter_linie(v),
        }
    }
    if let Some(v) = teile.get(3) {
        match gitter_spanne(v) {
            Some(sp) => erg.gitter_spalte_spanne = sp.max(erg.gitter_spalte_spanne),
            None => erg.gitter_spalte_ende = gitter_linie(v),
        }
    }
}

/// Zerlegt eine Kurzform an den `/`, die NICHT in einer Klammer stehen
/// (CSS Grid 1 §8.4).
///
/// L4 (03.10.2026 00:4x). GEMESSENE ENTSCHEIDUNG, warum derselbe
/// Tiefenzaehler wie bei `komma_0` (parse.rs, 21:7x): `minmax(min(100px,
/// 200px), 1fr)` traegt ein `/` in der Klammer, und ein Zerlegen an
/// jedem `/` wuerde die verschachtelte Form zerlegen. Der Zaehler ist
/// derselbe Fehlerfall mit einem anderen Trenner.
///
/// GEMESSENE ENTSCHEIDUNG, warum das Ergebnis `Vec` ist und kein
/// `Option<(String, Option<String>)>`: `grid-area` hat drei Trenner, und
/// dieselbe Funktion soll sie bedienen koennen, ohne dass jemand sie
/// zweimal schreibt. Ein Tupel waere eine Form, die man fuer jede
/// weitere Kurzform wieder aufmacht.
fn gitter_teile(text: &str) -> Vec<&str> {
    let mut raus: Vec<&str> = Vec::new();
    let mut tiefe = 0i32;
    let mut start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        match b {
            b'(' | b'[' => tiefe += 1,
            b')' | b']' => tiefe -= 1,
            b'/' if tiefe == 0 => {
                raus.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if tiefe != 0 {
        // Unbalancierte Klammer: die Angabe ist ungueltig, und der
        // Aufrufer verwirft sie ueber die leere Liste. Ein Fehlerpfad,
        // der sich nicht durch Zugabe reparieren laesst.
        return vec![text];
    }
    raus.push(text[start..].trim());
    raus
}

/// Die Spannweite aus `span N`, oder `None`, wenn es keine ist.
///
/// L4 (02.10.2026 23:4x). GEMESSENE ENTSCHEIDUNG, warum es eine eigene
/// Funktion ist und kein Rueckgabewert von `gitter_linie`: die Zahl nach
/// `span` ist eine SPANNWEITE, die andere Zahl ist eine LINIE, und beide
/// kommen als `i32` in `Berechnet` an. Sie imselben Feld zu halten hiesse
/// „die Form steht im Wert" — und die Form steht im Rohwert.
///
/// GEMESSENE ENTSCHEIDUNG, warum `span` OHNE Zahl hier `None` ist und
/// nicht `Some(1)`: §8.3 sagt „`span` by itself is equivalent to
/// `span 1`", und `span 1` ist dasselbe wie keine Angabe. Also aendert
/// es nichts, und der Rueckgabewert bleibt fuer die Zaehlung der
/// Testfaelle frei.
fn gitter_spanne(text: &str) -> Option<usize> {
    let t = text.trim();
    let rest = t.strip_prefix("span")?.trim();
    if rest.is_empty() {
        return None;
    }
    // GEMESSENE ENTSCHEIDUNG, warum der Oberwert 65534 ist: §8.3 erlaubt
    // `span <integer>` mit 1..65535, und dieselbe Grenze steht in
    // `gitter_linie` (`clamp(-65534, 65535)`). `0` ist ungueltig (§8.3:
    // „span must be greater than zero"), also wird es zu 1.
    let z: i32 = rest.parse().unwrap_or(0);
    if z < 1 {
        return None;
    }
    Some((z as usize).min(65535))
}

/// Eine Gitter-LINIE (§8.3): eine Zahl, `auto` (0) oder ein Name (0).
fn gitter_linie(text: &str) -> i32 {
    let t = text.trim();
    if t == "auto" || t.is_empty() {
        return 0;
    }
    // L4 (23:4x): `span N` wird NICHT mehr als Linie gelesen. Vorher stand
    // hier `strip_prefix("span ")` -> `parse`, und die Zahl kam in
    // `gitter_zeile_start` — wo sie als Linie gelesen wurde. GEMESSEN
    // ueber `l4_span_ueber_zeilen_macht_ein_kind_hoch`: die Item-Hoehe
    // blieb 50 px statt 100 px. Die Zahl gehoert nach
    // `gitter_zeile_spanne` (siehe `gitter_spanne`).
    match t.parse::<i32>() {
        Ok(z) => z.clamp(-65534, 65535),
        Err(_) => {
            // Ein Name: er gehoert zu einem anderen Raster.
            verwurf_zaehler();
            0
        }
    }
}

/// `overflow-x`/`-y` (CSS Overflow 3 §3.1).
///
/// `clip` und `hidden` unterscheiden sich erst beim Zeichnen (§4.1:
/// `clip` erzeugt keinen Bildlaufbereich), nicht im berechneten Wert.
fn ueberlauf_aus(w: &str) -> Option<Ueberlauf> {
    // CSS Overflow 3 §3.1: `visible` ist der Anfangswert und steht in
    // `Berechnet` als `None`; die drei anderen sind EIGENE Zustaende,
    // weil sie sich beim Zeichnen unterscheiden (§4.1: `clip`
    // erzeugt keinen Bildlaufbereich, `scroll` immer einen).
    //
    // `clip` ist `hidden` mit dem Unterschied, dass kein
    // Bildlaufbereich entsteht — fuer DIESEN Enum also `Versteckt`,
    // mit dem Unterschied im Rasterizer (WUNSCH_CSS.txt W8).
    match w {
        "visible" => None,
        "hidden" | "clip" => Some(Ueberlauf::Versteckt),
        "scroll" => Some(Ueberlauf::Scroll),
        "auto" => Some(Ueberlauf::Auto),
        _ => None,
    }
}

/// `aspect-ratio: [auto] <zahl> [ / <zahl> ]` (CSS Sizing 4 §4.1) — das
/// Verhaeltnis Breite : Hoehe als EINE Zahl (`16/9` -> 1,777…).
///
/// Release 10.10.2026: vorher wurde nur der ZAEHLER genommen — `16/9`
/// wurde 16, und ein 616 px breites Bild bekam 38,5 px Hoehe statt 346,5
/// (GEMESSEN: bbc.com, jedes Aufmacherbild ein flacher Streifen). Mit dem
/// Nenner 1 (`2/1`) fiel der Fehler nie auf. `auto` vor oder nach dem
/// Bruch (fuer ersetzte Elemente mit eigenem Verhaeltnis) wird
/// uebergangen; der Bruch gilt als Rueckfall.
fn verhaeltnis_zaehler(t: &str) -> Option<f32> {
    let k = t.trim();
    let k = k.strip_prefix("auto").map(str::trim).unwrap_or(k);
    let k = k.strip_suffix("auto").map(str::trim).unwrap_or(k);
    if k.is_empty() {
        return None;
    }
    let zahl = |s: &str| s.trim().parse::<f32>().ok().filter(|f| f.is_finite() && *f > 0.0);
    match k.split_once('/') {
        Some((z, n)) => {
            let v = zahl(z)? / zahl(n)?;
            v.is_finite().then_some(v)
        }
        None => zahl(k),
    }
}

/// Setzt einen Schluessel im Behaelter, ohne dass ein spaeterer Wert
/// einen frueheren verdoppelt (CSS Cascade 6 §6.4: die spaetere
/// Setzt einen Schluessel im Behaelter OHNE zu verdoppeln
/// (CSS Cascade 6 §6.4: die spaetere Deklaration gewinnt).
///
/// GEMESSENER FEHLER 01.10.2026 (K7c): meine erste Fassung hier war
/// `clear()` + `push` mit einem festen Schluessel — der Behaelter
/// konnte damit genau EINEN Wert halten, und alle ~190 Eigenschaften
/// teilten sich denselben Platz. Das ist kein Sammelbehaelter, das ist
/// ein Wegwerf mit Etikett.
///
/// Deshalb traegt der Wert den NAMEN mit (`Wert::Schluessel`), und der
/// Behaelter fuehrt je Name genau einen Eintrag. 20 `cursor`- und 30
/// `transform`-Werte auf einem Element sind dann zwei Zeilen.
///
/// M4: die Laengengrenze hat der Aufrufer schon geprueft.
fn upsert_schluessel(behaelter: &mut Vec<(String, String)>, name: &str, w: &str) {
    for e in behaelter.iter_mut() {
        if e.0 == name {
            e.1.clear();
            e.1.push_str(w);
            return;
        }
    }
    behaelter.push((String::from(name), String::from(w)));
}

/// Zaehlt einen Verwurf im Profilzaehler (K7c).
fn verwurf_zaehler() {
    profil(pz::VERWORFEN);
}
