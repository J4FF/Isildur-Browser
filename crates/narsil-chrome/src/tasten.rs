//! U-TASTEN (Koordination 02.10.): DIE TASTENKUERZEL-LISTE (`about:tasten`).
//!
//! # WOFUER
//!
//! Der neue Eintrag der Koordination lautet: „danach Feinschliff aller
//! Seiten (about:*), **Tastenkuerzel-Liste**". Der Feinschliff ist gemacht
//! (`tests/a_schliiff.rs`), und dies ist die Liste.
//!
//! # WARUM EINE SEITE UND NICHT EIN TEXT IM MENU
//!
//! GEMESSENE ENTSCHEIDUNG: ein Menuepunkt („Tastenkürzel" im Drei-Streichen-
//! Menue) waer eine Zeile, die man suchen muss; `about:tasten` ist eine
//! Adresse, die man tippen kann, und die man in ein Lesezeichen legen kann.
//! GEMESSENE BEGRUENDUNG fuer die Seite: die Liste ist laenger als ein
//! Bildschirm, wenn man sie als Dialog baut — und ein Dialog, den man
//! schliesst, vergisst man.
//!
//! # WAS IN DER LISTE STEHT UND WAS NICHT
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Liste die Kuerzel zeigt, die die
//! OBERFLAECHE verlangt, nicht die, die `narsil` heute abhaengt: `narsil-chrome`
//! weiss nicht, welche Taste in `narsil` eine Funktion hat (Tasten sind
//! Prozesse, siehe `team/WUNSCH_UI.txt`). Eine Liste aus nur den heute
//! funktionierenden Kuerzeln waere eine Liste, die morgen kuerzer wird — und
//! wer sie als Lesezeichen gespeichert hat, sieht eine Seite, auf der etwas
//! fehlt, ohne Grund. GEMESSENE BEGRUENDUNG fuer das Umgekehrte: eine Seite
//! mit Kuerzeln, die nichts tun, ist eine Anleitung ins Leere. Deshalb steht
//! bei jedem Kuerzel, WOHER es kommt (siehe [`Herkunft`]) — `narsil` kann die
//! Liste damit abgleichen und die nicht gebauten streichen.

use super::{Chrome, Rechteck};
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, tr_key};

/// M3: so viele Kuerzel stehen hoechstens in der Liste.
///
/// GEMESSENE BEGRUENDUNG fuer 32: die Liste hat heute 24 Eintraege
/// (GEMESSEN: [`KUERZEL`].len()). 32 ist die naechste Potenz von zwei
/// darueber — sie fasst die Liste EINMAL, ohne dass ein Kuerzel verschwinden
/// muss, wenn ein neues dazukommt. GEMESSENE ENTSCHEIDUNG, warum es hier
/// steht und nicht in `narsil`: die Seite zeichnet hoechstens so viele Zeilen;
/// eine groessere Liste wird abgeschnitten, und eine abgeschnittene Liste
/// ist eine Liste, die man nicht lesen kann.
pub const MAX_KUERZEL: usize = 32;

/// M3: der linke Rand der Seite (24 px, [`super::MARGIN_SEITE`]).
pub const MARGIN: u32 = super::MARGIN_SEITE;

/// M3: die Hoehe einer Zeile (28 px, `seiten::ZEILE`).
///
/// GEMESSENE BEGRUENDUNG: es ist dieselbe Zeile wie in den anderen Listen
/// (Verlauf, Downloads). Eine Taste ist eine Zeile lang — wer sie mit dem
/// Finger trifft, soll nicht danebentreffen.
pub const ZEILE: u32 = super::seiten::ZEILE;

/// M3: der Luft-Rand zwischen den beiden Spalten (24 px).
pub const LUFT_SPALTE: u32 = super::MARGIN_SEITE;

/// M3: die Breite einer Spalte (das Fenster minus zwei Raende, halbiert).
///
/// GEMESSENE ENTSCHEIDUNG, warum die Spalten BREIT und nicht schmal sind: die
/// Wirkung ist ein Text („Alles löschen" ist 13 Zeichen), und eine Spalte,
/// in der der Text abgeschnitten wird, ist eine Liste, in der man raten muss.
/// GEMESSENE ENTSCHEIDUNG fuer ZWEI Spalten: 24 Zeilen sind in einer Spalte
/// 24 * 28 = 672 px hoch — das passt in ein 760-px-Fenster nicht mit Rand.
/// In zwei Spalten sind es 12 * 28 = 336 px, und darunter ist noch Platz fuer
/// den Hinweis am Fuss.
pub const SPALTEN: u32 = 2;

/// M3: die Breite der Tastenspalte innerhalb einer Spalte (136 px).
///
/// GEMESSENE BEGRUENDUNG: die laengste Taste der Liste ist „Alt+Pfeil rechts"
/// mit 16 Zeichen, und der Stern der offenen Kuerzel kommt obendraup —
/// GEMESSEN im Test: 16 * 8 = 128 px, mit Stern 136. GEMESSENE ENTSCHEIDUNG,
/// warum die laengste Taste den Platz bekommt: die Taste steht links und die
/// Wirkung rechts; eine abgeschnittene Taste sieht aus wie eine falsche.
/// GEMESSENER EIGENER FEHLER (02.10.): hier standen 120 px, und der Test
/// `die_liste_ist_vollstaendig_und_eindeutig` hat gemeldet, dass
/// „Alt+Pfeil rechts" 128 px braucht — der Stern der offenen Kuerzel passte
/// nicht einmal daneben.
pub const BREITE_TASTE: u32 = 136;

/// M3: der Luft-Rand zwischen Taste und Wirkung (12 px).
pub const LUFT_TEXT: u32 = 12;

/// M3: der Platz ueber der ersten Zeile fuer den ersten Gruppenamen (16 px).
///
/// GEMESSENE ENTSCHEIDUNG (02.10., Massstab 3 in `tests/a_schliiff.rs`):
/// zuerst stand der Gruppenname 8 px UEBER dem ersten Kuerzel — GEMESSEN: bei
/// 74 px, waehrend die Seite bei 82 px beginnt, also stand er auf der
/// Lesezeichenleiste. BEHoben: die Liste reserviert oben 16 px fuer den
/// Namen, und die Zeilen beginnen darunter. GEMESSENE BEGRUENDUNG fuer 16 px:
/// 12 px Schrift plus 4 px Luft, und mehr braucht ein Name nicht.
pub const LUFT_GRUPPE: u32 = 16;

/// M3: die Hoehe des Hinweises am Fuss (32 px, eine Zeile plus Rand).
pub const HOEHE_HINWEIS: u32 = 32;

/// WOHER EIN KUERZEL KOMMT — damit `narsil` die Liste abgleichen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Herkunft {
    /// `narsil-chrome` zeichnet es: der Knopf bzw. das Feld traegt es.
    Oberflaeche,
    /// `narsil` muss es bauen (steht in `team/WUNSCH_UI.txt`).
    Narsil,
    /// Aus ANFORDERUNGEN (`team/ANFORDERUNGEN.txt`).
    Anforderung,
}

impl Herkunft {
    /// DER NAME, der in der Spalte „Herkunft" steht.
    pub fn text(&self) -> &'static str {
        match self {
            Herkunft::Oberflaeche => "Oberfläche",
            Herkunft::Narsil => "narsil",
            Herkunft::Anforderung => "Anforderungen",
        }
    }

    /// WER MUSS ES BAUEN? (`None` = es ist schon da)
    pub fn offen(&self) -> bool {
        *self == Herkunft::Narsil
    }
}

/// EIN KUERZEL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kuerzel {
    /// Die Tasten, so wie man sie drueckt: „Strg+F", „F11", „Alt+Pfeil links".
    pub taste: &'static str,
    /// Was passiert.
    pub wirkung: &'static str,
    /// Woher es kommt.
    pub herkunft: Herkunft,
}

impl Kuerzel {
    /// EIN KUERZEL aus den drei Angaben (fuer Tests und eigene Zeilen).
    pub const fn neu(taste: &'static str, wirkung: &'static str, herkunft: Herkunft) -> Kuerzel {
        Kuerzel {
            taste,
            wirkung,
            herkunft,
        }
    }
}

/// DIE KURZE IN DER LISTE.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie nach GRUPPEN sortiert ist und nicht
/// alphabetisch: man sucht nicht „Strg", man sucht „Fenster" oder „Seite".
/// Die Gruppen stehen in [`GRUPPEN`] in derselben Reihenfolge.
///
/// GEMESSENE ENTSCHEIDUNG, warum „Alt+Pfeil links" statt „Alt+A": der
/// Pfeil ist die Taste, die man drueckt — „Alt+A" waere eine Erfindung, die
/// auf keiner Tastatur steht.
pub const KUERZEL: [Kuerzel; 24] = [
    Kuerzel::neu("F11", tr_key("Toggle full screen"), Herkunft::Narsil),
    Kuerzel::neu("F5", tr_key("Reload page"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+R"), tr_key("Reload page (hard)"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Alt+Left arrow"), tr_key("Back"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Alt+Right arrow"), tr_key("Forward"), Herkunft::Anforderung),
    Kuerzel::neu("Esc", tr_key("Close find bar"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+L"), tr_key("Select address bar"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+D"), tr_key("Bookmark this page"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+Shift+B"), tr_key("Show/hide bookmarks bar"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+T"), tr_key("New tab"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+W"), tr_key("Close tab"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+Shift+T"), tr_key("Reopen last tab"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl++"), tr_key("Larger"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+-"), tr_key("Smaller"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+0"), tr_key("Actual size (100 %)"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+F"), tr_key("Find in page"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+G"), tr_key("Next match"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Page Up / Down"), tr_key("Scroll by page"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl++ / Ctrl+-"), tr_key("Zoom PDF page"), Herkunft::Narsil),
    Kuerzel::neu("F12", tr_key("Open developer tools"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+Shift+I"), tr_key("Open developer tools"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+Shift+J"), tr_key("Open console"), Herkunft::Narsil),
    Kuerzel::neu(tr_key("Ctrl+P"), tr_key("Print"), Herkunft::Anforderung),
    Kuerzel::neu(tr_key("Ctrl+S"), tr_key("Save page as"), Herkunft::Anforderung),
];

/// DIE GRUPPEN DER LISTE — (Name, Anzahl der Zeilen danach).
///
/// GEMESSENE BEGRUENDUNG, warum die Gruppen hier stehen und nicht als Feld
/// in `Kuerzel`: die Liste ist nach Gruppen sortiert, und die Gruppennamen
/// stehen in [`Kuerzel::wirkung`]-Naehe — aber sie sind KEINE Kuerzel. Ein
/// sechtes Feld in jeder Zeile, das in der Spalte „Herkunft" nichts verloren
/// hat, waere eine Spalte, die man nicht liest.
/// The group names are translation keys (drawn with `tr`).
pub const GRUPPEN: [(&str, usize); 6] = [
    (tr_key("Page"), 6),
    (tr_key("Address"), 3),
    (tr_key("Tabs"), 3),
    (tr_key("Zoom and page"), 5),
    (tr_key("PDF viewer"), 2),
    (tr_key("Tools"), 5),
];

/// DER HINWEIS AM FUSS — was die Liste ist und was sie nicht ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Hinweis auf der Seite steht und nicht
/// nur im Quelltext: die Liste nennt Kuerzel, die `narsil` noch bauen muss
/// (Spalte „Herkunft"). Wer die Seite liest, soll wissen, dass „narsil" in der
/// Spalte eine BEDROEFNIS bedeutet und keine Ausrede.
pub const HINWEIS: &str = tr_key("* = shortcut that Narsil is still building.");

/// WELCHE SPALTE, WELCHE ZEILE UND WELCHE HOEHE hat der Eintrag `nr`?
///
/// GEMESSENE ENTSCHEIDUNG, warum die Liste SPALTENweise gefuellt wird und
/// nicht zeilenweise: bei zeilenweiser Fuellung stehen die ersten beiden
/// Zeilen nebeneinander und der Rest in der zweiten Spalte — man liest dann
/// quer statt ueber.
///
/// GEMESSENER EIGENER FEHLER (02.10., Fensterbild `a_schliiff_4.png`): die
/// Gruppennamen standen auf den Zeilen darunter („Adresse" auf „Strg+D",
/// „PDF-Betrachter" auf „Strg+F"). GEMESSENE URSACHE: der Name wurde an die
/// Y-Stelle der ERSTEN Zeile seiner Gruppe gesetzt, und die erste Zeile der
/// Gruppe steht dort ebenfalls. BEHoben: ueber jeder Gruppe steht ein eigener
/// Platz von [`LUFT_GRUPPE`] px, und die Zeilen rechnen ihn mit.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Gruppenzahl nicht aus [`KUERZEL`]
/// berechnet wird, sondern aus [`GRUPPEN`]: `GRUPPEN` ist die Liste, die der
/// Mensch pflegt; `KUERZEL` waere die zweite Liste mit derselben Information.
pub fn platz(nr: usize) -> Option<(u32, u32)> {
    if nr >= KUERZEL.len() || nr >= MAX_KUERZEL {
        return None;
    }
    let je_spalte = zeilen_je_spalte();
    let spalte = (nr / je_spalte) as u32;
    let position = nr % je_spalte;
    if spalte >= SPALTEN {
        return None;
    }
    let erste = spalte as usize * je_spalte;
    // DIE HOEHE: LUFT_GRUPPE ueber jeder Gruppe, die in dieser Spalte
    // beginnt — inclusive der eigenen (der Name steht ueber der ersten
    // Zeile). GEMESSENE ZAHLEN (Spalte 0, Gruppen bei 0, 6 und 9):
    // 32, 60, 88, 116, 144, 172, 216, 244, 272, 316, 344, 372.
    let mut y = LUFT_GRUPPE;
    for j in 0..=position {
        if ist_gruppen_start(erste + j) {
            y += LUFT_GRUPPE;
        }
        if j < position {
            y += ZEILE;
        }
    }
    Some((spalte, y))
}

/// DIE HOEHE DES NAMENS der Gruppe, die bei `nr` beginnt (oder `None`).
///
/// GEMESSENE ENTSCHEIDUNG, warum der Name 8 px ueber seiner ersten Zeile
/// steht: 12 px Schrift plus 8 px Luft — er soll an der Gruppe kleben, nicht
/// an der Zeile DARUEBER (die einer anderen Gruppe gehoert).
pub fn gruppen_platz(nr: usize) -> Option<(u32, u32)> {
    if !ist_gruppen_start(nr) {
        return None;
    }
    let (spalte, y) = platz(nr)?;
    Some((spalte, y.saturating_sub(8)))
}

/// WIE VIELE ZEILEN STEHEN IN EINER SPALTE? (abgerundet nach oben)
pub fn zeilen_je_spalte() -> usize {
    // GEMESSEN (02.10., clippy): `(len + n - 1) / n` ist eine von Hand
    // geschriebene `div_ceil` — clippy nennt es „manually reimplementing
    // div_ceil". Die Formel stimmte, der Name fehlte.
    KUERZEL.len().div_ceil(SPALTEN as usize)
}

/// BEGINNT BEI `nr` EINE GRUPPE?
fn ist_gruppen_start(nr: usize) -> bool {
    gruppen_start().contains(&nr)
}

/// DIE NUMMERN, bei denen eine Gruppe beginnt (aus [`GRUPPEN`]).
pub fn gruppen_start() -> Vec<usize> {
    let mut r = Vec::new();
    let mut summe = 0;
    for (_, anzahl) in GRUPPEN {
        r.push(summe);
        summe += anzahl;
    }
    r
}

/// WELCHE GRUPPE GEHOERT ZU `nr`? (Index in [`GRUPPEN`])
pub fn gruppe_von(nr: usize) -> Option<usize> {
    let start = gruppen_start();
    start.iter().rposition(|s| *s <= nr)
}

/// WIE VIELE KUERZEL SIND NOCH OFFEN (Herkunft `narsil`)?
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine Zahl und kein Flag ist: die Seite
/// sagt unten „* = Kuerzel, das narsil noch baut" — und diese Zahl ist die
/// Belegstelle fuer das Sternchen. Sie aendert sich mit jeder Zeile, die
/// `narsil` baut, und das ist dann auch im Test sichtbar.
pub fn offene_zaehler() -> usize {
    KUERZEL.iter().filter(|k| k.herkunft.offen()).count()
}

/// DIE GEOMETRIE DER SEITE — Spalten, Liste, Hinweis (`None`, wenn zu klein).
///
/// M3: die Seite braucht mindestens eine Zeile, einen Hinweis und zwei
/// Spalten, die zusammenpassen.
pub struct Geometrie {
    /// Der linke Rand.
    pub x: u32,
    /// Die Oberkante der Liste (die Oberkante des INHALTS — unter der
    /// Ueberschrift, mit Lesezeichenleiste).
    pub y: u32,
    /// Die Breite EINER Spalte.
    pub spalten_breite: u32,
}

/// DIE GEOMETRIE (`None`, wenn das Fenster zu klein ist).
///
/// GEMESSENER EIGENER FEHLER (02.10., Fensterbild `a_schliiff_4.png`): hier
/// stand `TAB_LEISTE_HOEHE + LEISTE_HOEHE + UEBERSCHRIFT_HOEHE` (82 px) ohne
/// die Chrome. GEMESSENE FOLGE: mit Lesezeichenleiste begann die Liste 24 px
/// zu hoch — der Gruppenname „Seite" stand auf der Ueberschrift
/// „Tastenkürzel". BEHoben: `chrome: &Chrome` zuerst und
/// [`Chrome::inhalt_oberkante`], wie in `seiten.rs`.
pub fn geometrie(chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> Option<Geometrie> {
    let y = chrome.inhalt_oberkante();
    // GEMESSEN (02.10.): `+ LUFT_GRUPPE` — ueber der ersten Zeile steht der
    // Name der ersten Gruppe.
    let platz =
        fensterhoehe.saturating_sub(y + super::STATUSLEISTE_HOEHE + HOEHE_HINWEIS + LUFT_GRUPPE);
    let nutzbreite = fensterbreite.saturating_sub(2 * MARGIN);
    // M3: eine Spalte muss die TASTE und mindestens 12 Zeichen Wirkung tragen
    // (der kuerzeste Wirkungstext ist „Zurück" mit 6, der laengste
    // „PDF-Seite vergrößern" mit 19 — 19 * 8 = 152 px, plus 120 px Taste und
    // 12 px Luft sind 284 px je Spalte, zwei Spalten also 592 px).
    //
    // GEMESSENER EIGENER FEHLER (02.10., Massstab 1): hier stand
    // `nutzbreite - LUFT_SPALTE`. GEMESSEN: bei einem 1-px-Fenster ist
    // `nutzbreite` 0, und `0 - 24` panickt mit „attempt to subtract with
    // overflow".
    let spalten_breite = nutzbreite.saturating_sub(LUFT_SPALTE) / SPALTEN;
    if spalten_breite < BREITE_TASTE + LUFT_TEXT + 12 * super::ZEICHENBREITE || platz < ZEILE {
        return None;
    }
    Some(Geometrie {
        x: MARGIN,
        y,
        spalten_breite,
    })
}

/// DAS RECHTECK EINER ZEILE — (`None`, wenn die Seite zu klein ist).
///
/// M3/M4: die Zeile haengt an Fensterbreite und -hoehe; beide kommen von
/// `narsil`, nicht aus einer Eingabe.
pub fn zeile(
    nr: usize,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    let g = geometrie(chrome, fensterbreite, fensterhoehe)?;
    let (spalte, y) = platz(nr)?;
    let r = Rechteck {
        x: g.x + spalte * (g.spalten_breite + LUFT_SPALTE),
        y: g.y + y,
        breite: g.spalten_breite,
        hoehe: ZEILE,
    };
    // GEMESSENER EIGENER FEHLER (02.10., Massstab 2): hier stand nur die
    // Rechnung, keine Grenze. GEMESSEN: bei 640 x 480 endete die letzte Zeile
    // bei 510 px — 30 px UNTER dem Fenster, und der Hinweis (bei 436 px)
    // stand mitten in der Liste. BEHoben: eine Zeile, die nicht bis ueber
    // den Hinweis passt, wird nicht gezeichnet (`display_list` hoert beim
    // ersten `None` auf). GEMESSENE ENTSCHEIDUNG: die Liste wird KUERZER,
    // nicht abgeschnitten — ein Kuerzel, von dem die Haelfte unter dem Rand
    // liegt, ist keins.
    if r.unten() + HOEHE_HINWEIS + super::STATUSLEISTE_HOEHE > fensterhoehe {
        return None;
    }
    Some(r)
}

/// DER TREFFER: welche Zeile liegt unter dem Punkt? (`None` = keine)
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zeilen hitbar sind, obwohl sie NICHTS
/// tun: eine Zeile, die man anklicken kann und dann passiert nichts, ist ein
/// Fehler. `narsil` soll sie koennen (Punkt 20 in `team/WUNSCH_UI.txt`:
/// Klick auf eine Zeile legt das Kuerzel in die Zwischenablage — dann ist die
/// Seite nuetzlich und der Treffer noetig). GEMESSENE ENTSCHEIDUNG fuer den
/// Anfang: `treffer` sagt, welche Zeile getroffen wurde; WAS `narsil` damit
/// tut, entscheidet `narsil`.
pub fn treffer(p: &Point, chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> Option<usize> {
    // GEMESSENE ENTSCHEIDUNG, warum die NaN-Wache VOR den Vergleichen steht:
    // jeder Vergleich mit NaN ist falsch (U1c war genau hier der Fehler).
    if !p.x.is_finite() || !p.y.is_finite() {
        return None;
    }
    for nr in 0..KUERZEL.len() {
        if let Some(r) = zeile(nr, chrome, fensterbreite, fensterhoehe) {
            if p.x >= r.x as f32
                && p.x < r.rechts() as f32
                && p.y >= r.y as f32
                && p.y < r.unten() as f32
            {
                return Some(nr);
            }
        }
    }
    None
}

/// DAS ZEICHEN DER OFFENEN KUERZEL — ein Stern hinter der Taste.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein Stern und keine dritte Spalte: die
/// Seite hat zwei Spalten aus je 568 px (Fenster 1200), und eine dritte
/// Spalte „Herkunft" mit 24 Zeilen wuerde die Wirkung auf 6 Zeichen
/// kuerzen. GEMESSENE ENTSCHEIDUNG, warum die WIRKUNG offen in
/// `text_schwach` steht: sie ist eine Angabe, die noch nicht gilt — und
/// blass ist die Zahl, die man mit einem Blick vom Rest trennt.
pub const STERN: char = '*';

/// DIE SEITE ALS ZEICHENLISTE.
pub fn display_list(chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    let seiten_oben = chrome.seiten_oberkante();
    // 1. DER GRUND.
    liste.push(DisplayItem::Rect {
        rect: Rect::new(
            0.0,
            seiten_oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(seiten_oben) as f32,
        ),
        color: pal.hintergrund,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // 2. DIE ÜBERSCHRIFT (Feinschliff 02.10.) — erst nach dem Grund.
    chrome.ueberschrift_zeichnen(super::seiten::Seite::Tasten.text(), &mut liste);
    let Some(g) = geometrie(chrome, fensterbreite, fensterhoehe) else {
        return liste;
    };
    // 3. DIE GRUPPENNAMEN — ueber der ersten Zeile ihrer Gruppe.
    for (nr, (name, _)) in GRUPPEN.iter().enumerate() {
        let start = gruppen_start()[nr];
        let Some((spalte, y)) = gruppen_platz(start) else {
            break;
        };
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(
                (g.x + spalte * (g.spalten_breite + LUFT_SPALTE)) as f32,
                (g.y + y) as f32,
            ),
            text: tr(name).to_string(),
            color: pal.text_schwach,
            font_size: 12.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    // 4. DIE KUERZEL — Taste links, Wirkung rechts.
    // GEMESSEN (02.10., clippy): `for (nr, k) in KUERZEL.iter().enumerate()`
    // statt `for nr in 0..len` mit `KUERZEL[nr]` — clippy nennt die zweite
    // Form „the loop variable is used to index".
    for (nr, k) in KUERZEL.iter().enumerate().take(MAX_KUERZEL) {
        let Some(r) = zeile(nr, chrome, fensterbreite, fensterhoehe) else {
            break;
        };
        // DIE TASTE — mit Stern, wenn `narsil` sie noch bauen muss.
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(r.x as f32, (r.y + 4) as f32),
            text: if k.herkunft.offen() {
                format!("{}{STERN}", tr(k.taste))
            } else {
                tr(k.taste).to_string()
            },
            color: pal.text,
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
        // DIE WIRKUNG — offen in `text_schwach`, gekuerzt mit „…" (sonst
        // weiss man nicht, dass Text fehlt; siehe `kuerzen`).
        liste.push(DisplayItem::Text {
            // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
            // traegt jetzt die `font-family`-Liste. Hier steht die
            // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
            // Bildschirmpunkte schreibt keine eigene Familie.
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new((r.x + BREITE_TASTE + LUFT_TEXT) as f32, (r.y + 4) as f32),
            // GEMESSEN (02.10., clippy): der Zugriff war `k.wirkung` = `KUERZEL[nr]`;
            // clippy nennt es „the loop variable is used to index". Jetzt steht
            // der Wert ueber `iter().enumerate()` in `k`.
            text: kuerzen(tr(k.wirkung), zeichen_platz(g.spalten_breite)),
            color: if k.herkunft.offen() {
                pal.text_schwach
            } else {
                pal.text
            },
            font_size: 14.0,
            // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
            // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
            // alles aufrecht, weil kein Schnitt ankam.
            gewicht: 400,
            kursiv: false,
        });
    }
    // 5. DER HINWEIS AM FUSS.
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new(
            MARGIN as f32,
            (fensterhoehe - super::STATUSLEISTE_HOEHE - 20) as f32,
        ),
        // GEMESSENE ENTSCHEIDUNG: der Hinweis steht über der ganzen Seite,
        // nicht in einer Spalte — er wird darum an der Fensterbreite
        // gemessen, nicht an der Spaltenbreite.
        text: tr(HINWEIS)
            .chars()
            .take((fensterbreite.saturating_sub(2 * MARGIN) / super::ZEICHENBREITE) as usize)
            .collect(),
        color: pal.text_schwach,
        font_size: 12.0,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });
    liste
}

/// WIE VIELE ZEICHEN DER WIRKUNG in `breite` passen?
///
/// GEMESSENE ENTSCHEIDUNG, warum von [`ZEICHENBREITE`] hier KEINE
/// Korrektur fuer die kleinere Schrift steht: 12 px und 14 px sind hier
/// dieselbe Zahl (8 px je Zeichen), weil die Zahl eine Entscheidung und keine
/// Messung ist (siehe `lib.rs::ZEICHENBREITE`).
fn zeichen_platz(breite: u32) -> usize {
    (breite as usize).saturating_sub((BREITE_TASTE + LUFT_TEXT) as usize)
        / super::ZEICHENBREITE as usize
}

/// KUERZT EINEN TEXT AUF `platz` ZEICHEN — mit „…" am Ende, wenn etwas fehlt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das „…" ist und kein stilles Abschneiden:
/// GEMESSEN bei 640 px Fensterbreite: nach der Tastenspalte bleiben 136 px,
/// das sind 17 Zeichen; „Lesezeichenleiste ein/aus" (24 Zeichen) passt nicht.
/// Ohne Zeichen fuer die Auslassung ist eine abgeschnittene Wirkung eine
/// andere Behauptung — „Seite neu lade" statt „Seite neu laden".
///
/// GEMESSENE ENTSCHEIDUNG, warum `platz - 1` Zeichen bleiben: das „…" ist ein
/// Zeichen. GEMESSENE ENTSCHEIDUNG fuer `platz = 0`: dann bleibt der Text
/// leer — ein Punkt in einer Spalte ohne Platz ist eine Lupe.
pub fn kuerzen(text: &str, platz: usize) -> String {
    if text.chars().count() <= platz {
        return text.to_string();
    }
    if platz == 0 {
        return String::new();
    }
    let mut r: String = text.chars().take(platz - 1).collect();
    r.push('…');
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DIE LISTE: 24 KUERZEL, keine leere Zeile, keine Doppelung.
    ///
    /// GEMESSENE BEHAUPTUNG: 24 Eintraege, jeder mit Taste und Wirkung, und
    /// keine Taste steht zweimal drin. GEMESSENE ENTSCHEIDUNG, warum die
    /// Zahl 24 eine Zahl ist und keine Absicht: `KUERZEL` ist die Liste, und
    /// der Test liest sie, statt sie zu behaupten — `MAX_KUERZEL` (32) ist
    /// die Grenze, das Array `[Kuerzel; 24]` die Zahl.
    #[test]
    fn die_liste_ist_vollstaendig_und_eindeutig() {
        assert_eq!(KUERZEL.len(), 24, "GEMESSEN: 24 Kuerzel");
        assert!(
            KUERZEL.len() <= MAX_KUERZEL,
            "GEMESSEN: {} Kuerzel passen nicht in {MAX_KUERZEL}",
            KUERZEL.len()
        );
        for (nr, k) in KUERZEL.iter().enumerate() {
            assert!(!k.taste.is_empty(), "Kuerzel {nr} hat keine Taste");
            assert!(!k.wirkung.is_empty(), "Kuerzel {nr} hat keine Wirkung");
            // DIE TASTE passt in die Spalte (BREITE_TASTE = 120 px, 8 px je
            // Zeichen) — sonst schneidet der Knopf sie ab.
            let zeichen = k.taste.chars().count() as u32 + if k.herkunft.offen() { 1 } else { 0 };
            assert!(
                zeichen * super::super::ZEICHENBREITE <= BREITE_TASTE,
                "Kuerzel {nr} ({:?}) ist {zeichen} Zeichen breit — mehr als {BREITE_TASTE} px",
                k.taste
            );
            // UND: die WIRKUNG passt in die breitere Restspalte (gemessen
            // unten beim Fenster 1200: 436 px = 54 Zeichen).
            for breite in [1200u32, 1000, 800] {
                let platz = (breite - 2 * MARGIN - LUFT_SPALTE) / SPALTEN;
                let rest = platz - BREITE_TASTE - LUFT_TEXT;
                assert!(
                    k.wirkung.chars().count() as u32 * super::super::ZEICHENBREITE <= rest,
                    "Kuerzel {nr} ({:?}) passt nicht bei {breite} px: {} Zeichen > {} px",
                    k.wirkung,
                    k.wirkung.chars().count(),
                    rest
                );
            }
        }
        // KEINE DOPPELUNG: dieselbe Taste zweimal waere eine Zeile, die man
        // zweimal liest.
        for (i, k) in KUERZEL.iter().enumerate() {
            assert!(
                !KUERZEL[..i].iter().any(|x| x.taste == k.taste),
                "die Taste {:?} steht zweimal drin",
                k.taste
            );
        }
    }

    /// DIE OFFENEN KUERZEL: der Zaehler passt zu den Sternchen.
    #[test]
    fn die_offenen_kuerzel_werden_gezaehlt() {
        // GEMESSEN: 12 der 24 Kuerzel kommen aus `narsil`.
        assert_eq!(offene_zaehler(), 12, "GEMESSEN: 12 offene Kuerzel");
        let mit_stern = KUERZEL.iter().filter(|k| k.herkunft.offen()).count();
        assert_eq!(mit_stern, offene_zaehler());
        // UND: kein Kuerzel traegt den Stern zweimal (der Stern wird
        // angehaengt, nicht ersetzt).
        for k in KUERZEL.iter() {
            assert!(
                !k.taste.contains(STERN),
                "{:?} traegt den Stern schon im Text",
                k.taste
            );
        }
        // UND: `Herkunft` ist benannt und nicht leer.
        assert_eq!(Herkunft::Oberflaeche.text(), "Oberfläche");
        assert_eq!(Herkunft::Narsil.text(), "narsil");
        assert_eq!(Herkunft::Anforderung.text(), "Anforderungen");
        // GEMESSENER EIGENER FEHLER (02.10., beim Schreiben des Tests): ich
        // behauptete `!Narsil.offen()` — das Gegenteil ist die Definition
        // („offen" heisst: `narsil` muss es noch bauen). Der Test hat es
        // gemeldet; die Behauptung ist jetzt richtig und benannt.
        assert!(Herkunft::Narsil.offen());
        assert!(!Herkunft::Oberflaeche.offen());
        assert!(!Herkunft::Anforderung.offen());
    }

    /// DIE GRUPPEN: sie decken die Liste ab, ohne Luecke.
    ///
    /// GEMESSENE BEHAUPTUNG: die Gruppenanfänge sind 0, 6, 9, 12, 17, 19 —
    /// GEMESSEN aus [`GRUPPEN`] (6 + 3 + 3 + 5 + 2 + 5 = 24).
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., beim Aufsetzen des Tests): hier
    /// standen 0, 6, 9, 12, 16, 18 und die Summe 23 — in der Gruppe „Zoom und
    /// Seite" standen 4 statt 5 Kuerzel (es sind Strg++, Strg+-, Strg+0,
    /// Strg+F und Strg+G). Der Test hat es gemeldet, und `GRUPPEN` nennt jetzt
    /// fuenf. GEMESSENE REGEL: die Gruppenliste wird neben dem Array
    /// gepflegt und von einem Test geprueft, das ihre Summe gleich der Zahl
    /// der Kuerzel ist — sonst zaehlt man zweimal.
    #[test]
    fn die_gruppen_decken_die_liste_ab() {
        let start = gruppen_start();
        assert_eq!(start, vec![0, 6, 9, 12, 17, 19], "GEMESSEN: 6 Gruppen");
        let summe: usize = GRUPPEN.iter().map(|(_, n)| *n).sum();
        assert_eq!(
            summe,
            KUERZEL.len(),
            "GEMESSEN: {summe} Zeilen in den Gruppen, {} Kuerzel",
            KUERZEL.len()
        );
        assert_eq!(GRUPPEN.len(), start.len());
        // JEDE Zeile gehoert zu genau einer Gruppe.
        for nr in 0..KUERZEL.len() {
            let g = gruppe_von(nr).expect("Gruppe");
            assert!(
                GRUPPEN[g].0.len() > 2,
                "Gruppe {g} hat keinen brauchbaren Namen"
            );
        }
        // UND: kein Kuerzel ist in zwei Gruppen (die Gruppen sind
        // aufeinanderfolgende Abschnitte).
        for (i, (_, n)) in GRUPPEN.iter().enumerate() {
            let von = start[i];
            let bis = start
                .get(i + 1)
                .map(|b| b - von)
                .unwrap_or(KUERZEL.len() - von);
            assert_eq!(
                *n, bis,
                "GEMESSEN: Gruppe {i} umfasst {bis} Zeilen, nicht {n}"
            );
        }
    }

    /// DIE GRUPPENNAMEN stehen 8 px ueber ihrer ersten Zeile — und NICHT
    /// auf der Zeile darunter.
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., Fensterbild `a_schliiff_4.png`):
    /// der Gruppenname stand AUF der ersten Zeile („Adresse" auf „Strg+D").
    /// GEMESSENE BEHAUPTUNG: `gruppen_platz(nr).1 + 8 == platz(nr).1` fuer
    /// jeden Gruppenanfang, und der Name liegt zwischen der Zeile DARUEBER und
    /// seiner eigenen (er endet 8 px ueber dem Zeilenanfang, die Zeile
    /// darueber endet 28 px darueber).
    #[test]
    fn der_gruppenname_steht_ueber_seiner_ersten_zeile() {
        let mut chrome = Chrome::neu(1200);
        chrome_lesezeichen(&mut chrome);
        for start in gruppen_start() {
            let (spalte, name_y) = gruppen_platz(start).expect("Gruppenplatz");
            let (_, zeilen_y) = platz(start).expect("Platz");
            assert_eq!(
                zeilen_y - name_y,
                8,
                "GEMESSEN: {LUFT_GRUPPE} px Platz, davon 8 px Luft"
            );
            // GEMESSEN: der Name steht 8 px ueber dem Zeilenanfang, sein
            // Kasten ist 12 px hoch — er reicht also 4 px IN die Zeile hinein.
            // Der Zeilentext beginnt aber erst 4 px unter dem Zeilenanfang,
            // also genau dort, wo der Name endet: sie beruehren sich, sie
            // ueberlappen sich nicht.
            assert!(
                name_y + 12 <= zeilen_y + 4,
                "GEMESSEN: der Name endet bei {}, der Zeilentext beginnt bei {}",
                name_y + 12,
                zeilen_y + 4
            );
            // UND: er steht NICHT in der Zeile darueber — das ist die Zeile
            // `start - 1`, wenn sie in derselben Spalte liegt (die Spalten
            // sind 12 Zeilen lang).
            //
            // GEMESSENER EIGENER FEHLER (02.10., beim Schreiben des Tests):
            // ich verglich mit der ERSTEN Zeile der VORHERIGEN GRUPPE, nicht
            // mit der Zeile direkt darueber. GEMESSEN: bei der Gruppe
            // „PDF-Betrachter" (Position 5 in der zweiten Spalte) behauptete
            // der Test „der Name steht auf der Zeile darueber (350 bis 308)" —
            // die 350 war das Ende der ERSTEN Zeile der vorigen Gruppe, nicht
            // das Ende der Zeile direkt darueber. BEHoben: der Vergleich
            // laeuft ueber `platz(start - 1)`, also die Nachbarzeile.
            if start > 0 {
                if let Some((p_spalte, p_y)) = platz(start - 1) {
                    if p_spalte == spalte {
                        assert!(
                            name_y >= p_y + ZEILE,
                            "GEMESSEN: der Name beginnt bei {name_y}, die Zeile darueber endet bei {}",
                            p_y + ZEILE
                        );
                    }
                }
            }
        }
    }

    /// DIE ZEILEN: kein Ueberlappen, kein herausragen, und ab und zu KEINE.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer „ab und zu keine": bei 640 x 480 passt
    /// die Liste nicht ganz (GEMESSEN: die letzte Zeile endete bei 510 px).
    /// GEMESSENE REGEL: eine Zeile, die nicht ueber den Hinweis passt, wird
    /// nicht gezeichnet — die Liste wird kuerzer, nicht abgeschnitten.
    #[test]
    fn die_zeilen_sitzen_und_passen() {
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (300, 200),
            (640, 480),
            (800, 600),
            (1200, 760),
            (1920, 1080),
            (u32::MAX, u32::MAX),
        ] {
            let mut chrome = Chrome::neu(b);
            chrome_lesezeichen(&mut chrome);
            let mut vorher = None;
            let mut gezeichnet = 0;
            for nr in 0..KUERZEL.len() {
                let Some(r) = zeile(nr, &chrome, b, h) else {
                    break;
                };
                assert!(
                    r.rechts() <= b || b == u32::MAX,
                    "bei {b}x{h}: Zeile {nr} ragt rechts heraus"
                );
                assert!(
                    r.y >= chrome.inhalt_oberkante(),
                    "bei {b}x{h}: Zeile {nr} oben drauf"
                );
                assert!(
                    r.unten() <= h || h == u32::MAX,
                    "bei {b}x{h}: Zeile {nr} unten raus"
                );
                if let Some((x, y, breite)) = vorher {
                    if x == r.x {
                        // UND: sie stapeln sich (kein Ueberlappen).
                        assert!(y <= r.y, "bei {b}x{h}: Zeile {nr} ueberlappt die vorige");
                        assert!(y + breite > 0);
                    } else {
                        // DIE SPALTE beginnt wieder oben.
                        assert!(r.y < y, "bei {b}x{h}: die zweite Spalte startet nicht oben");
                    }
                }
                vorher = Some((r.x, r.y, r.breite));
                gezeichnet += 1;
            }
            eprintln!(
                "TASTEN {b}x{h}: {gezeichnet} von {} Zeilen gezeichnet",
                KUERZEL.len()
            );
            if b == 1200 && h == 760 {
                assert_eq!(gezeichnet, KUERZEL.len(), "im Abnahmefenster fehlen Zeilen");
            }
            //
            // KOORDINATION (02.10.): die Bedienflaeche ist GEMESSEN von 72
            // auf 108 px gewachsen, deshalb passen in ein 640x480-Fenster
            // GEMESSEN **7** Zeilen statt 9. GEMESSEN: die Zeilenzahl haengt
            // an der Fenstergroesse, und diese Zahl ist eine Messung, kein
            // Versprechen.
            if b == 640 && h == 480 {
                assert_eq!(
                    gezeichnet, 8,
                    "GEMESSEN: 8 Zeilen bei 640x480 nach der 1:1-Messung — die kuerzere Chrome (81 statt 108) laesst GEMESSEN eine Zeile mehr zu"
                );
                assert!(gezeichnet < KUERZEL.len(), "GEMESSEN: es sind nicht alle");
            }
        }
    }

    /// DER TREFFER: jede Zeile trifft sich selbst, NaN nie (M8).
    #[test]
    fn der_treffer_der_liste() {
        let mut chrome = Chrome::neu(1200);
        chrome_lesezeichen(&mut chrome);
        for nr in 0..KUERZEL.len() {
            let r = zeile(nr, &chrome, 1200, 760).expect("Zeile");
            let p = Point::new((r.x + 10) as f32, (r.y + 10) as f32);
            assert_eq!(
                treffer(&p, &chrome, 1200, 760),
                Some(nr),
                "{nr} trifft sich nicht selbst"
            );
        }
        // AUSSERHALB: nichts.
        assert_eq!(
            treffer(&Point::new(5.0, 5.0), &chrome, 1200, 760),
            None,
            "oben links ist ein Treffer"
        );
        // NaN und Unendlich (M8).
        assert_eq!(
            treffer(&Point::new(f32::NAN, f32::NAN), &chrome, 1200, 760),
            None
        );
        assert_eq!(
            treffer(&Point::new(f32::INFINITY, 0.0), &chrome, 1200, 760),
            None
        );
        // UND: im zu kleinen Fenster gibt es keine Treffer (keine Zeile).
        assert_eq!(treffer(&Point::new(30.0, 130.0), &chrome, 1, 1), None);
    }

    /// DAS BILD: jede Taste steht drin, und die Sterne zaehlen.
    #[test]
    fn die_liste_im_bild() {
        let mut chrome = Chrome::neu(1200);
        chrome_lesezeichen(&mut chrome);
        let liste = display_list(&chrome, 1200, 760);
        let texte: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for (nr, k) in KUERZEL.iter().enumerate() {
            let erwartet = if k.herkunft.offen() {
                format!("{}{STERN}", k.taste)
            } else {
                k.taste.to_string()
            };
            assert!(
                texte.contains(&erwartet),
                "Kuerzel {nr} ({erwartet}) fehlt im Bild: {texte:?}"
            );
            assert!(
                texte.iter().any(|t| t == k.wirkung),
                "die Wirkung {:?} fehlt im Bild",
                k.wirkung
            );
        }
        // DIE GRUPPENNAMEN stehen drin.
        for (name, _) in GRUPPEN {
            assert!(
                texte.iter().any(|t| t == name),
                "die Gruppe {name:?} steht nicht im Bild"
            );
        }
        // DER HINWEIS steht drin, mit dem STERN.
        assert!(
            texte.iter().any(|t| t == HINWEIS),
            "der Hinweis steht nicht im Bild: {texte:?}"
        );
        // DIE ÜBERSCHRIFT steht drin (18 px — die einzige groessere Schrift).
        let ueberschriften: Vec<&String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text {
                    text, font_size, ..
                } if *font_size > 14.0 => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(ueberschriften.len(), 1, "GEMESSEN: eine Ueberschrift");
        assert_eq!(ueberschriften[0], "Keyboard shortcuts");
        // DIE LISTE IST SCHLICHT: 24 Zeilen mal 2 Texte + 6 Namen + 1 Hinweis
        // + 1 Ueberschrift + 1 Grund = 57 Items.
        assert_eq!(
            liste.len(),
            KUERZEL.len() * 2 + GRUPPEN.len() + 2 + 1,
            "GEMESSEN: {} Items",
            liste.len()
        );
    }

    /// DIE LISTE BEGINNT UNTER DER ÜBERSCHRIFT (Feinschliff 02.10.).
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., Fensterbild): die Liste begann bei
    /// `TAB_LEISTE_HOEHE + LEISTE_HOEHE + UEBERSCHRIFT_HOEHE` = 82 px, ohne
    /// die Chrome — mit Lesezeichenleiste stand der Gruppenname „Seite" auf
    /// der Ueberschrift „Tastenkürzel".
    #[test]
    fn die_liste_beginnt_unter_der_ueberschrift() {
        let mut chrome = Chrome::neu(1200);
        chrome_lesezeichen(&mut chrome);
        assert_eq!(
            chrome.seiten_oberkante(),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 24,
            "GEMESSEN: 39 + 42 + 24 = 105 (1:1-Messung, 03.10.)"
        );
        assert_eq!(
            chrome.inhalt_oberkante(),
            crate::TAB_LEISTE_HOEHE + crate::LEISTE_HOEHE + 48,
            "GEMESSEN: 81 + 48 = 129 (1:1-Messung, 03.10.)"
        );
        let r = zeile(0, &chrome, 1200, 760).expect("Zeile");
        let (spalte, name_y) = gruppen_platz(0).expect("Gruppenplatz");
        assert_eq!(spalte, 0);
        // DER NAME beginnt 24 px unter der Inhaltsoberkante (platz(0) = 32,
        // davon 8 px Luft drueber = 24) — GEMESSEN: 130.
        assert_eq!(
            chrome.inhalt_oberkante() + name_y,
            153,
            "GEMESSEN: 129 + 24 = 153 (1:1-Messung, 03.10.)"
        );
        assert_eq!(name_y, 24);
        assert!(
            r.y >= chrome.inhalt_oberkante(),
            "die erste Zeile ist ueber dem Inhalt"
        );
        assert_eq!(
            r.y, 161,
            "GEMESSEN: 129 + platz(0) = 32 (1:1-Messung, 03.10.)"
        );
    }

    /// DER HINWEIS STEHT UEBER DER LETZTEN ZEILE.
    #[test]
    fn der_hinweis_steht_unter_der_liste() {
        let mut chrome = Chrome::neu(1200);
        chrome_lesezeichen(&mut chrome);
        let letzte = zeile(KUERZEL.len() - 1, &chrome, 1200, 760).expect("letzte Zeile");
        let hinweis_y = 760 - super::super::STATUSLEISTE_HOEHE - 20;
        assert!(
            letzte.unten() <= hinweis_y,
            "GEMESSEN: die letzte Zeile endet bei {}, der Hinweis steht bei {hinweis_y}",
            letzte.unten()
        );
        // UND: es ist Luft dazwischen (nicht direkt darunter).
        assert!(
            hinweis_y - letzte.unten() >= 8,
            "kein Luft zwischen Liste und Hinweis"
        );
    }

    /// HILFSFUNKTION: die Lesezeichenleiste AN (24 px).
    fn chrome_lesezeichen(chrome: &mut Chrome) {
        chrome.setze_lesezeichen_hoehe(super::super::lesezeichen::HOEHE_LEISTE);
    }
}

/// (im Testmodul) DAS KUERZEN MIT „…" — GEMESSEN, nicht behauptet.
#[cfg(test)]
mod kuerzen_tests {
    use super::kuerzen;

    /// DAS KUERZEN: passt der Text, bleibt er; passt er nicht, kommt „…".
    ///
    /// GEMESSENE BEHAUPTUNG: 25 Zeichen in 17 Zeichen Platz ergeben
    /// „Lesezeichenleist…" (16 Zeichen Text plus „…"), und in 0 Zeichen
    /// Platz ergibt es einen leeren Text.
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., beim Schreiben des Tests): ich
    /// behauptete „Lesezeichenleiste ein/…" — das sind 18 Zeichen in 17
    /// Zeichen Platz. GEMESSEN ist „Lesezeichenleist…". Die Regel steht jetzt
    /// im Code (`platz - 1` Zeichen plus „…") und der Test nennt Laenge UND
    /// Text.
    #[test]
    fn das_kuerzen_zeigt_die_auslassung() {
        assert_eq!(kuerzen("Strg+F", 10), "Strg+F");
        assert_eq!(
            kuerzen("Lesezeichenleiste ein/aus", 17),
            "Lesezeichenleist…",
            "GEMESSEN: 16 Zeichen plus Auslassungszeichen"
        );
        // GEMESSENER EIGENER FEHLER (02.10.): hier stand 24 als Platz fuer
        // den ungekuerzten Text. GEMESSEN: „Lesezeichenleiste ein/aus" hat
        // **25** Zeichen (17 + 1 + 3 + 1 + 3), also wurde gekuerzt und der
        // Test meldete „Lesezeichenleiste ein/a…". Die Zahl stand in der
        // Behauptung und nicht am Text.
        assert_eq!(
            kuerzen("Lesezeichenleiste ein/aus", 25),
            "Lesezeichenleiste ein/aus"
        );
        assert_eq!(
            "Lesezeichenleiste ein/aus".chars().count(),
            25,
            "GEMESSEN: der Text hat 25 Zeichen"
        );
        assert_eq!(kuerzen("Lesezeichenleiste ein/aus", 0), "");
        // UND: es schneidet nie in der Mitte eines ZEICHENS (die Laenge in
        // Zeichen bleibt `platz`, das Zeichen selbst ganz).
        let lang = "Ä".repeat(10);
        assert_eq!(kuerzen(&lang, 3).chars().count(), 3);
        assert_eq!(kuerzen(&lang, 3), "ÄÄ…");
    }
}
