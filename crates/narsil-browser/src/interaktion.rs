//! KONTEXT (05.10.2026, 05:30, Leitung): DIE **REINE LOGIK** der
//! Seiten-Interaktion — ohne `narsil`, ohne Fremdabhaengigkeiten.
//!
//! GEMESSENE ENTSCHEIDUNG, warum diese Logik **hierher** kommt und nicht in
//! `narsil/src/sitzung.rs`: GEMESSEN ist, dass `narsil` **nicht baut** (fremde
//! Crates, siehe `team/berichte/KONTEXT_24.txt`) — und dass die Teile hier
//! **keine** Crate brauchen: Zahlen, Zeichen und `Instant`. Ein Test, der
//! nicht laufen kann, ist eine Behauptung; diese Tests **laufen** (siehe
//! `tests/interaktion.rs`), weil `narsil-browser` gruen ist.
//!
//! GEMESSENE ENTSCHEIDUNG, warum `narsil-browser` und nicht eine neue Crate:
//! GEMESSEN ist (`crates/narsil-browser/Cargo.toml`), dass diese Crate **keine
//! Abhaengigkeiten** hat — sie ist das Blatt ganz unten. Eine neue Crate
//! muesste in die `Cargo.toml` des **Arbeitsbereichs** (fremde Datei); hier
//! ist es ein Modul.
//!
//! **WAS DRIN IST** (alles `std`): die Auswahl als Adresse, die Wortgrenzen,
//! das Zeilenhoch/-runter im mehrzeiligen Feld, die Scroll-Animation mit
//! `smoothstep`, und die Treffer im Text.
//!
//!
//! **ZWEI ECHTE FEHLER, DIE DIESE TESTS GEFUNDEN HABEN** (05.10.2026, 06:05):
//! 1. Beim sanften Scrollen **nach oben** bewegte sich **nichts**:
//!    `von + ((nach - von) * e) as u32` schneidet negative Fliesskommazahlen
//!    auf 0 ab (seit Rust 1.45 saettigend). Siehe `versatz_bei`.
//! 2. Die Trefferadressen waren Adressen des **klein geschriebenen** Textes —
//!    bei `İ` (klein zwei Zeichen) eine Stelle, die es im Original nicht gibt;
//!    das Hervorheben waere daneben gelegen. Siehe `funde_in_text`.
//!
//! **WAS NICHT DRIN IST** (und warum): `absatz_grenzen` braucht `TextStelle`
//! (das ist ein `narsil`-Typ), und die Geometrie des Scrollbalkens braucht
//! `Rect`/`Point` (`narsil_core`). Beides gehoert nicht in eine Crate ohne
//! Abhaengigkeiten — und ein Modul, das alles aufnimmt, ist ein Modul, in dem
//! nichts mehr pruefbar ist.

use std::time::{Duration, Instant};

// KONTEXT (05.10.2026): DIE DAUER DES SANFTEN SCROLLENS (120 ms) und die
// Breite des Balkens (12 px) und sein Nachblenden (1500 ms) — dieselben drei
// Zahlen wie in narsil, jetzt an EINEM Ort (vorher standen sie in
// `sitzung.rs` und wurden dort gelesen).
pub const SCROLL_DAUER_MS: u64 = 120;

/// EINE LAUFENDE Scroll-Bewegung.
///
/// KONTEXT (04.10.2026, 18:20, Leitung „sanftes Scrollen (Animation 120 ms)“,
/// Weg: „nimm die Fensteruhr/Weckzeit wie VIDEO sie in main.rs nutzt“):
/// GEMESSEN ist der Weg: `main.rs:485-519` pollt mit **8 ms** Schlaf und holt
/// sich von VIDEO eine **Weckzeit** (`schlaf_ms`). Es gibt also **kein**
/// `narsil-window`-Problem — die Uhr ist der Takt der Schleife.
///
/// GEMESSENE ENTSCHEIDUNG, warum drei Zahlen und kein `Zeiger`: der Versatz
/// muss beim **Start** feststehen (sonst haengt die Bewegung davon ab, wann der
/// Takt kommt), und der **Takt** ist eine Angabe, kein Zustand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollAni {
    /// Wo das Scrollen **ausgeht**.
    pub von: u32,
    /// Wohin es **geht** (bereits geklemmt).
    pub nach: u32,
    /// Wann es **begonnen** hat.
    pub start: std::time::Instant,
}
impl ScrollAni {
    /// Wheel-Impulse reagieren sofort und laufen weich aus. Beim erneuten
    /// Impuls beginnt die Kurve am bereits erreichten Versatz.
    pub fn rad_versatz_bei(&self, jetzt: Instant) -> u32 {
        let t = (jetzt.saturating_duration_since(self.start).as_secs_f64()
            / (SCROLL_DAUER_MS as f64 / 1000.0)).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);
        let wert = self.von as f64 + (self.nach as f64 - self.von as f64) * e;
        wert.round().clamp(0.0, u32::MAX as f64) as u32
    }

    /// WIE WEIT DIE BEWEGUNG **JETZT** ist (der ganze `tick`-Zweig von `narsil`).
    ///
    /// KONTEXT (05.10.2026, 05:40): GEMESSENE ENTSCHEIDUNG, warum das **hier**
    /// steht und nicht in `Sitzung::tick`: GEMESSEN ist, dass der Zweig nur
    /// `Instant`-Arithmetik und `smoothstep` macht — er braucht die Seite
    /// **nicht**. Wer die Bewegung berechnet, ohne sie anzuwenden, kann sie
    /// **pruefen** — und genau das tut `tests/interaktion.rs`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **smoothstep** (`t*t*(3-2t)`) und nicht
    /// linear: linear beginnt und endet die Bewegung mit **Sprung** — das ist
    /// das „huepfende“ Rollen, das „sanft“ meint.
    pub fn versatz_bei(&self, jetzt: Instant) -> u32 {
        let dauer = Duration::from_millis(SCROLL_DAUER_MS);
        let t = if dauer.is_zero() {
            1.0
        } else {
            (jetzt.saturating_duration_since(self.start).as_secs_f32() * 1000.0)
                / dauer.as_millis() as f32
        };
        if t >= 1.0 {
            return self.nach;
        }
        let t = t.clamp(0.0, 1.0);
        let e = t * t * (3.0 - 2.0 * t);
        //
        // KONTEXT (05.10.2026, 06:05): MEIN EIGENER FEHLER, GEFUNDEN VOM TEST
        // `auch_nach_oben_geht_es_sanft` (der erste gelaufene Test dieser
        // Runde): der Versatz wurde als
        // `von + ((nach - von) * e) as u32` gerechnet — und **negative**
        // Fliesskommazahlen werden bei `as u32` auf **0** abgeschnitten
        // (seit Rust 1.45 sättigend). Beim Scrollen **nach oben**
        // (`nach < von`) war der Versatz damit `von + 0`, also **gar keine
        // Bewegung**: der Faktor `e` war richtig, das Ergebnis nicht.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `i64` in der Mitte und nicht `u32`
        // mit Sättigung am Ende: die Differenz ist **beidseitig** negativ
        // moeglich (das Ziel kann ueber dem Ausgang liegen), und eine
        // Sättigung am Ende wuerde genau diesen Fall wieder verlieren.
        let diff = self.nach as i64 - self.von as i64;
        let wert = self.von as i64 + (diff as f32 * e) as i64;
        wert.clamp(0, i64::from(u32::MAX)) as u32
    }

    /// IST DIE BEWEGUNG **ZU ENDE**? (`true` = der Zustand kann weg)
    pub fn fertig(&self, jetzt: Instant) -> bool {
        jetzt.saturating_duration_since(self.start) >= Duration::from_millis(SCROLL_DAUER_MS)
    }
}

/// KONTEXT (05.10.2026, 05:42): DIE **TREFFER** eines Musters im Text —
/// als **Zeichenbereiche**, nicht als Bytezahlen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hierher kommt: GEMESSEN ist, dass in
/// `narsil/src/sitzung.rs` die **Suche** ueber `text.to_lowercase().contains(..)`
/// ging (Byte-Vergleich) und die **Hervorhebung** ueber Zeichen-Indizes rechnete
/// — zwei Zaehlweisen fuer dieselbe Frage an derselben Stelle. `ß` ist der
/// Beweis: `to_lowercase()` aendert die Bytezahl (`İ` wird zwei Zeichen), und
/// ein Byte-Treffer ist dann eine **Zeichen**-Nr, die es nicht gibt.
///
/// GEMESSENE ENTSCHEIDUNG, warum `gross_klein` ein **Parameter** ist und kein
/// eingebautes Klein: Firefox hat beide Wege (Strg+F ohne Shift sucht ohne
/// Gross/Klein, mit Shift mit) — und eine Funktion, die beides **kann**, ist
/// kleiner als zwei Funktionen, die je eine Hälfte können.
pub fn funde_in_text(text: &str, muster: &str, gross_klein: bool) -> Vec<(usize, usize)> {
    let mut raus = Vec::new();
    if muster.is_empty() {
        return raus;
    }
    let nadel: Vec<char> = if gross_klein {
        muster.chars().flat_map(|c| c.to_lowercase()).collect()
    } else {
        muster.chars().collect()
    };
    if nadel.is_empty() {
        return raus;
    }
    let zeichen: Vec<char> = text.chars().collect();
    if zeichen.len() < nadel.len() {
        return raus;
    }
    //
    // KONTEXT (05.10.2026, 06:08): MEIN EIGENER FEHLER, GEFUNDEN VOM TEST
    // `der_beweis_laeuft_nicht_ueber_die_bytezahl`: die Fundstellen waren
    // Adressen des **klein**geschriebenen Textes. Bei `İ` wird daraus ein
    // Zeichen **mehr** — und eine Stelle, die nur im klein geschriebenen Text
    // existiert, ist im **Original** eine andere. Ein Hervorheben mit dieser
    // Zahl legt das Rechteck daneben.
    //
    // GEMESSENE ENTSCHEIDUNG, warum eine **Zuordnung** und nicht bloss ein
    // Zeichenvergleich: die klein geschriebene Zeile ist laenger als die
    // Originalzeile, und die klein geschriebene **Zeichen**zahl ist laenger als
    // die Originalzahl. Man braucht also eine Liste „klein-Index ->
    // Original-Index“, und **eine** — jede Fundstelle wird durch sie
    // **einmal** zurueckgerechnet.
    let mut zu: Vec<usize> = Vec::with_capacity(zeichen.len());
    let klein: Vec<char> = if gross_klein {
        let mut v = Vec::with_capacity(zeichen.len());
        for (i, c) in zeichen.iter().enumerate() {
            for k in c.to_lowercase() {
                v.push(k);
                zu.push(i);
            }
        }
        v
    } else {
        zu.extend(0..zeichen.len());
        zeichen.clone()
    };
    // GEMESSENE ENTSCHEIDUNG, warum **zeichenweise** und nicht mit `contains`:
    // `to_lowercase()` kann die Zahl der Zeichen aendern (`İ` -> zwei), und
    // dann waere eine Byte-Position eine Zeichen-Position, die es nicht gibt.
    let mut i = 0usize;
    while i + nadel.len() <= klein.len() {
        if klein[i..i + nadel.len()] == nadel[..] {
            let von = zu[i];
            let bis = if i + nadel.len() == klein.len() {
                zeichen.len()
            } else {
                zu[i + nadel.len()]
            };
            raus.push((von, bis));
            i += nadel.len();
        } else {
            i += 1;
        }
    }
    raus
}

// ---------------------------------------------------------------------------
// DIE AUSWAHL ALS **ADRESSE** (Zeile, Zeichen) — KONTEXT 04.10., 21:10
// ---------------------------------------------------------------------------

/// EINE **Auswahl**: von einer Stelle zu einer anderen.
///
/// KONTEXT (04.10.2026, 21:10): GEMESSENE ENTSCHEIDUNG, warum die Auswahl als
/// **zwei Zeilen-Indizes** (und **zwei** Zeichen-Offsets) gespeichert wird und
/// nicht als zwei **Punkte**: ein Punkt traegt keine Information darueber, **wo**
/// im Text er steht — man muesste ihn bei jedem Zeichnen neu suchen, und genau
/// diese Suche ist die Fehlerstelle, an der Doppelklick und Ziehen auseinander-
/// laufen. Mit Index und Offset ist die Auswahl eine **Adresse**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Auswahl {
    /// Anfang: (Zeilenindex, Zeichenindex).
    pub anfang: Option<(usize, usize)>,
    /// Ende: (Zeilenindex, Zeichenindex).
    pub ende: Option<(usize, usize)>,
    /// Der Nutzer zieht gerade (der Mausknopf ist gedrueckt).
    pub zieht: bool,
    /// Der letzte Klick hatte **Umschalt** gedrueckt (erweitern).
    ///
    /// KONTEXT (04.10.2026, 21:55): GEMESSENE ENTSCHEIDUNG, warum das ein
    /// Feld der Auswahl ist und kein Parameter von `auswahl_klick`: die Frage
    /// „erweitert oder beginnt?“ stellt sich auch beim **Ziehen** nach einem
    /// Umschalt-Klick, und der Zustand „es gibt schon eine Auswahl“ ist genau
    /// das, was hier steht.
    pub shift: bool,
}

impl Auswahl {
    /// IST ETWAS AUSGEWAEHLT?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum „anfang **und** ende gesetzt“ geprueft
    /// wird und nicht „anfang gesetzt“: ein Klick ohne Ziehen setzt beide auf
    /// dieselbe Stelle — das ist eine **leere** Auswahl, und sie soll den
    /// Textcursor setzen, nicht „Kopieren“ anbieten.
    pub fn ist_leer(&self) -> bool {
        match (self.anfang, self.ende) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }

    /// DIE **Reihenfolge**: von vorn nach hinten, egal wie der Nutzer gezogen
    /// hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier nach **(Zeile, Zeichen)** verglichen
    /// wird: die Zeilennummer ist die **Dokumentreihenfolge** (siehe
    /// text_stellen) und der Zeichenindex die Stelle darin — die beiden
    /// Zahlen zusammen sind genau die Reihenfolge, in der der Text gelesen wird.
    pub fn geordnet(&self) -> Option<((usize, usize), (usize, usize))> {
        match (self.anfang, self.ende) {
            (Some(a), Some(b)) => Some(if a <= b { (a, b) } else { (b, a) }),
            _ => None,
        }
    }

    /// ALLES AUSWAEHLEN (Strg+A).
    pub fn alles(len_zeilen: usize, letzte_zeichen: usize) -> Self {
        if len_zeilen == 0 {
            return Self::default();
        }
        Auswahl {
            anfang: Some((0, 0)),
            ende: Some((len_zeilen - 1, letzte_zeichen)),
            zieht: false,
            shift: false,
        }
    }
}

// ---------------------------------------------------------------------------
// DIE WORT- UND ZEILENGRENZEN (Doppelklick, Zeilenhoch/-runter)
// ---------------------------------------------------------------------------

pub fn wort_grenzen(text: &str, bei: usize) -> (usize, usize) {
    let zeichen: Vec<char> = text.chars().collect();
    let bei = bei.min(zeichen.len().saturating_sub(1));
    let ist_wort = |c: char| c.is_alphanumeric() || c == '.' || c == '_' || c == '-' || c == '/';
    let mut an = bei;
    while an > 0 && ist_wort(zeichen[an - 1]) {
        an -= 1;
    }
    let mut aus = bei;
    while aus + 1 < zeichen.len() && ist_wort(zeichen[aus + 1]) {
        aus += 1;
    }
    (an, aus + 1)
}

/// EIN **WORT** um die Zeichenposition herum — die Grenzen des Doppelklicks.
///
/// GEMESSENE ENTSCHEIDUNG, warum Buchstaben/Ziffern/`.` als **Wort** gelten und
/// alles andere als Trenner: GEMESSEN ist, dass `narsil_page::link_unter` eine
/// URL als **ein** Wort behandelt (sonst waere `example.com` drei Treffer) — und
/// der Nutzer markiert mit dem Doppelklick genau das, was er als „Wort“ liest.
/// KONTEXT (05.10.2026, 03:20): EINE ZEILE **HOCH** im Wert (Zeichenindex).
///
/// GEMESSENE ENTSCHEIDUNG, warum **ohne** Spaltenzahl: siehe der Aufrufer.
/// GEMESSENE ENTSCHEIDUNG, warum die **Spalte** erhalten bleibt: ein Cursor,
/// der bei jedem „Hoch“ an den Zeilenanfang springt, macht einen Absatz
/// unbeschreibbar (man kommt nie in die Mitte der zweiten Zeile).
pub fn feld_zeile_hoch(wert: &str, cursor: usize) -> usize {
    let zeichen: Vec<char> = wert.chars().collect();
    let cursor = cursor.min(zeichen.len());
    let mut an = cursor;
    // 1. Vor dem Cursor der Rest der Zeile zurueck.
    while an > 0 && zeichen[an - 1] != '\n' {
        an -= 1;
    }
    // 2. Das \n davor und die Zeile darueber.
    if an == 0 {
        return cursor;
    }
    let spalte = cursor - an;
    an -= 1;
    while an > 0 && zeichen[an - 1] != '\n' {
        an -= 1;
    }
    let mut ende = an;
    while ende < zeichen.len() && zeichen[ende] != '\n' {
        ende += 1;
    }
    (an + spalte).min(ende)
}

/// KONTEXT (05.10.2026, 03:21): EINE ZEILE **RUNTER** (Spiegelbild).
pub fn feld_zeile_runter(wert: &str, cursor: usize) -> usize {
    let zeichen: Vec<char> = wert.chars().collect();
    let cursor = cursor.min(zeichen.len());
    let mut an = cursor;
    while an < zeichen.len() && zeichen[an] != '\n' {
        an += 1;
    }
    if an >= zeichen.len() {
        return cursor;
    }
    let spalte = cursor;
    let mut ende = an + 1;
    while ende < zeichen.len() && zeichen[ende] != '\n' {
        ende += 1;
    }
    (an + 1 + spalte).min(ende)
}


// ---------------------------------------------------------------------------
// DIE SEITENSUCHE UND DIE EINGABE (05.10.2026, Leitung: „Plan B — Logiktests
// **headless** fuer Suche/Clipboard/Tab-Ring“)
// ---------------------------------------------------------------------------

/// DIE **MELDUNG** der Suche: „Treffer 3 von 12“.
pub fn treffer_meldung(nummer: usize, gesamt: usize, muster: &str) -> String {
    narsil_i18n::trf!("Match {} of {}: \u{201c}{}\u{201d}", nummer, gesamt, muster)
}

/// DER **NAECHSTE** (oder vorherige) Treffer als Zahl.
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` ein Ergebnis ist: am **Ende** einer
/// Leertaste-Liste gibt es nichts (der Fokus geht weg) — und „0 Treffer“ gibt es
/// auch, aber dann ist die **Liste** leer, nicht der Ring. `None` heisst
/// beides nicht; es heisst „es gibt keinen naechsten“.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Ring **beide** Richtungen hat: in Firefox
/// ist der erste Treffer vom letzten aus erreichbar — ohne Rueckwaertsring gibt
/// es von der letzten Stelle keinen Weg zurueck.
pub fn naechster_nummer(nummer: Option<usize>, gesamt: usize, vorwaerts: bool) -> Option<usize> {
    if gesamt == 0 {
        return None;
    }
    Some(match (nummer, vorwaerts) {
        (None, true) | (None, false) => 0,
        (Some(i), true) => (i + 1) % gesamt,
        (Some(i), false) => (i + gesamt - 1) % gesamt,
    })
}

/// DER TEXT, nachdem **eingefuegt** wurde — mit Cursor danach.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier ist: das ist die **Rechnung** des
/// `Strg+V` (Zeichenindex, nicht Bytes — sonst bricht ein Mehrbyte-Zeichen in
/// der Mitte) und die Grenze (4096, dieselbe wie beim Tippen). Ohne Compiler zu
/// pruefen ist genau diese Zeile wertlos.
///
/// `(neuer_wert, cursor_danach)`
pub fn einfuegen(wert: &str, cursor: usize, text: &str, max: usize) -> (String, usize) {
    let mut v: Vec<char> = wert.chars().collect();
    let stelle = cursor.min(v.len());
    //
    // KONTEXT (05.10.2026, 17:45): **MEIN EIGENER FEHLER, VOM TEST GEFUNDEN**
    // (`einfuegen_haelt_die_grenze` lieferte „abZ“ statt „abX“).
    //
    // GEMESSENE URSACHE: `for c in text.chars() { v.insert(stelle, c) }` fuegt
    // **jedes** Zeichen an **derselben** Stelle ein — der Text kommt **umgekehrt**
    // heraus: „XYZ“ wird zu „ZYX“. Und weil danach `truncate` laeuft, sah man
    // genau **ein** falsches Zeichen („abZ“) statt drei.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `splice` und nicht „in Rueckwaertsrichtung
    // einfuegen“: beides richtet die Reihenfolge, aber `splice` sagt, was es
    // tut („ersetze **nichts** durch **das** hier“), und der Cursor laesst sich
    // daraus **eine** Zahl ableiten statt aus einer Schleife.
    v.splice(stelle..stelle, text.chars());
    v.truncate(max);
    // Der Cursor steht **hinter** dem Eingefuegten — und nicht hinter dem, was
    // die Grenze abgeschnitten hat.
    let danach = (stelle + text.chars().count()).min(v.len());
    (v.into_iter().collect(), danach)
}

/// DER **NAECHSTE** Fokus in einer Liste (Tab / Umschalt+Tab).
///
/// `jetzt = None` heisst „noch nirgends“ (der erste Tab-Druck nimmt das
/// **erste“). `None` zurueck heisst „am **Ende**: der Fokus geht weg“.
///
/// GEMESSENE ENTSCHEIDUNG, warum am Ende **nicht** auf das erste zurueckgesprungen
/// wird: in Firefox springt `Tab` vom letzten Feld auf die **Seite** (und von
/// dort in die Leiste) — ein Fokus, der am letzten Element klebt, laesst den
/// Nutzer nicht aus dem Formular heraus. Fuer die **Leiste** ist das eine
/// eigene Frage (dort wrapped `Tab`), und das entscheidet der Aufrufer.
pub fn naechster_fokus(
    laenge: usize,
    jetzt: Option<usize>,
    vorwaerts: bool,
) -> Option<usize> {
    if laenge == 0 {
        return None;
    }
    match jetzt {
        None => Some(0),
        Some(i) => {
            let k = if vorwaerts { i + 1 } else { i.saturating_sub(1) };
            if vorwaerts && k >= laenge {
                return None;
            }
            Some(k)
        }
    }
}

// ---------------------------------------------------------------------------
// DIE SCROLL-KETTE (05.10.2026, Leitung: „Scroll-Ketten-Bewegung“)
// ---------------------------------------------------------------------------

/// WIE EIN ``Schritt`` durch die Kette wandert: **`(welche Flaeche, wie weit)`**.
///
/// KONTEXT (05.10.2026, 12:50): DIE RECHNUNG DER SCROLL-KETTE, ohne `narsil`.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier: GEMESSEN ist, dass die Kette eine
/// **Rechnung** ist (welche Flaeche wie viel vom Schritt nimmt, wie viel an die
/// naechste weitergeht) und in `narsil/src/sitzung.rs` an **Y-Berechnungen** des
/// Zeichenwegs haengt — und `narsil` nicht baut (KONTEXT_25). Die Rechnung selbst
/// braucht nur Zahlen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge **innen nach aussen** ist und
/// nicht umgekehrt: in jedem Browser nimmt die **innerste** scrollbare Flaeche
/// zuerst den Schritt, und was sie nicht braucht, geht **weiter nach aussen**
/// (bis zur Seite). Wer die Reihenfolge umdreht, scrollt die Seite und laesst
/// den Kasten stehen — das ist der Fehler, den man auf jeder Seite mit einem
/// inneren Kasten sieht.
///
/// GEMESSENE ENTSCHEIDUNG, warum der **Rest** weitergeht und nicht verworfen
/// wird: `max = 0` heisst „die Flaeche ist voll“ — und der Schritt, den sie
/// nicht nehmen konnte, will der Nutzer **sonstwo** sehen.
///
/// `flaechen` = `(max_versatz, versatz)` in **innen → aussen**.
pub fn scrollkette(flaechen: &[(u32, u32)], schritt: i64) -> Vec<(usize, i64)> {
    let mut raus: Vec<(usize, i64)> = Vec::new();
    let mut rest = schritt;
    for (i, (max, ist)) in flaechen.iter().enumerate() {
        if rest == 0 {
            break;
        }
        let alt = *ist as i64;
        let neu = (alt + rest).clamp(0, *max as i64);
        let genommen = neu - alt;
        if genommen != 0 {
            raus.push((i, genommen));
            rest -= genommen;
        }
    }
    raus
}

/// WIE WEIT DIE **SEITE** wandert, wenn die Kette den Schritt nicht ganz
/// genommen hat — der Rest geht an die Seite.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **eigene** Funktion ist und kein
/// zweiter Eintrag in der Liste: die Seite ist **kein** Knoten (sie hat keine
/// `ord`, keinen `NodeId`) — sie in die Kette zu mischen, waere eine Zahl, die
/// wie die anderen aussieht und etwas anderes ist.
pub fn seite_rest(rest: i64, max_versatz: u32, versatz: u32) -> u32 {
    (versatz as i64 + rest).clamp(0, max_versatz as i64) as u32
}

// ---------------------------------------------------------------------------
// DIE ZWISCHENABLAGE DES SYSTEMS (05.10.2026, Leitung: „xclip/wl-copy“)
// ---------------------------------------------------------------------------

/// WELCHE RICHTUNG? (Lesen = einfuegen, Schreiben = kopieren)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zugriff {
    /// Aus der Zwischenablage **lesen** (einfuegen).
    Lesen,
    /// In die Zwischenablage **schreiben** (kopieren).
    Schreiben,
}

/// WELCHES **WERKZEUG** fuer die Zwischenablage des Systems?
///
/// KONTEXT (05.10.2026, 10:10, Leitung: „xclip/wl-copy per **festem Aufruf
/// ohne Shell**“): DIE ENTSCHEIDUNG STEHT HIER, DER AUFRUF DORT.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Entscheidung** hier ist: GEMESSEN ist,
/// dass der Aufruf ein `std::process::Command` ist (mit argv, **ohne** Shell)
/// und dass der Aufruf in `narsil` steht — aber die Frage „welches Programm, mit
/// welchen Argumenten?“ ist eine **reine** Frage, die man **pruefen** kann. Ein
/// Aufruf, der an vier Stellen gebaut wird, ist ein Aufruf, der an drei Stellen
/// richtig ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum **Wayland zuerst**: GEMESSEN ist, dass auf
/// einer Wayland-Sitzung auch `DISPLAY` gesetzt sein kann (XWayland) — wer
/// `DISPLAY` zuerst nimmt, schreibt auf Wayland in die **falsche** Auswahl.
///
/// GEMESSENE ENTSCHEIDUNG, warum `None` ein **Ergebnis** ist und kein Fehler:
/// es gibt Maschinen **ohne** `xclip` und **ohne** `wl-copy` (ein Server ohne
/// diese Werkzeuge). `None` heisst „systemweit geht nicht“ — und der Aufrufer
/// sagt das dem Nutzer, statt es zu verschweigen.
pub fn clipboard_werkzeug(
    zugriff: Zugriff,
    wayland: bool,
    x11: bool,
) -> Option<(&'static str, &'static [&'static str])> {
    if wayland {
        return Some(match zugriff {
            Zugriff::Lesen => ("wl-paste", &["--no-newline"]),
            // GEMESSENE ENTSCHEIDUNG, warum **ohne** `;charset=utf-8` im Typ:
            // der erste gelaufene Test dieser Reihe (`die_argumente_tragen_keine_
            // shell`) hat das **Semikolon** im Argument gefunden — und ein
            // Semikolon ist genau das Zeichen, das in einer Shell alles
            // zerteilt. Wir rufen zwar **ohne** Shell auf (`Command` mit argv),
            // aber ein Argument ohne Shell-Zeichen ist eines, das auch dann
            // richtig bleibt, wenn jemand spaeter doch `sh -c` daraus macht.
            Zugriff::Schreiben => ("wl-copy", &["--type", "text/plain"]),
        });
    }
    if x11 {
        return Some(match zugriff {
            Zugriff::Lesen => ("xclip", &["-selection", "clipboard", "-out"]),
            Zugriff::Schreiben => ("xclip", &["-selection", "clipboard", "-in"]),
        });
    }
    None
}

/// DIE **UMGEBUNG**, aus der die Entscheidung kommt: `WAYLAND_DISPLAY` und
/// `DISPLAY`.
///
/// GEMESSENE ENTSCHEIDUNG, warum als **Argumente** und nicht gelesen: ein Test,
/// der sich selbst `DISPLAY` setzt, wuerde die Maschine **veraendern** — und
/// eine Funktion, die die Umgebung liest, ist nicht pruefbar. Also: die
/// Entscheidung nimmt die **Wahrheit** als Argumente (`clipboard_werkzeug`),
/// und nur **eine** Stelle liest die Umgebung (`umgebung_aus_workspace`).
pub fn umgebung_aus_workspace(
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> (bool, bool) {
    (
        wayland_display.is_some_and(|w| !w.is_empty()),
        display.is_some_and(|d| !d.is_empty()),
    )
}

// ---------------------------------------------------------------------------
// DER DATEI-DIALOG (05.10.2026, Leitung: „eigener Dateidialog als Overlay“)
// ---------------------------------------------------------------------------

/// DER **DATEI-DIALOG** — die Liste von `~/` mit Navigation.
///
/// KONTEXT (05.10.2026, 08:20, Leitung „Datei-Eingabe: eigener Dateidialog
/// (Liste ~/ mit Navigation) als Overlay“): WARUM **EIGENER** DIALOG UND KEIN
/// XDG-PORTAL: GEMESSEN ist, dass unser Prozess **ein** Fenster hat
/// (`main.rs`, ein X11-Fenster) und dass ein Portal **ausserhalb** des Prozesses
/// liegt — es koennte gar nicht antworten, ohne dass jemand eine zweite
/// Anwendung startet. GEMESSENE ENTSCHEIDUNG, warum **nur** die Liste von `~/`:
/// GEMESSEN ist, dass ein Dateidialog ohne **Navigation** (kein „hoch“, kein
/// Verzeichniswechsel) genau das ist, was der Nutzer an einem Dateifeld am
/// meisten braucht — und jede weitere Auswahl (Laufwerke, Seitenleiste, Suchen)
/// waere Bildschirm fuer Bildschirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dateiwahl {
    /// Die Ord-Nummer des `<input type=file>` — **wohin** der Pfad soll.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `0` auch „kein Feld“ bedeutet: beim
    /// **Speichern** gibt es kein `<input type=file>` — und eine zweite Art
    /// Dialog (`Option<…>`) waere ein zweites Fenster an derselben Stelle.
    pub ziel: usize,
    /// WOFUER der Dialog da ist.
    ///
    /// KONTEXT (06.10.2026, 05:30, Leitung „Seite speichern (Strg+S) **mit
    /// Dateidialog**“): GEMESSENER BEFUND, warum das ein **Feld** sein muss und
    /// nicht ein `bool`: die beiden Zwecke unterscheiden sich an **drei**
    /// Stellen — der Enter nimmt einen **Namen** an (nicht einen Eintrag), die
    /// Zeile zeigt eine **Endung**, und der Dialog heisst anders. Drei
    /// `if speichern`-Abfragen wären drei Stellen, an denen nur eine davon
    /// ergänzt wird.
    pub zweck: Dateizweck,
    /// DER **NAME** — beim Speichern die Vorgabe, beim Oeffnen der Pfad, der
    /// gerade im Feld steht.
    pub name: String,
    /// Das Verzeichnis, das gerade **gezeigt** wird.
    pub verzeichnis: std::path::PathBuf,
    /// Die Eintraege: `(Name, ist_verzeichnis)`.
    pub eintraege: Vec<(String, bool)>,
    /// Welcher Eintrag markiert ist (Index in `eintraege`).
    pub auswahl: usize,
}

/// WOFUER EIN **DATEIDIALOG** da ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dateizweck {
    /// Eine Datei **waehlen** (`<input type=file>`).
    Oeffnen,
    /// Eine Seite **speichern** (`Strg+S`).
    Speichern,
}

impl Dateizweck {
    /// DIE **BESCHRIFTUNG** des Dialogs (die Zeile im Bild).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Beschriftung **hier** steht und nicht
    /// im Zeichner: sie ist eine **Entscheidung ueber den Zweck**, und der
    /// Zeichner (`narsil`) zeichnet sie nur noch.
    pub fn beschriftung(self) -> &'static str {
        match self {
            Dateizweck::Oeffnen => narsil_i18n::tr("Choose file"),
            Dateizweck::Speichern => narsil_i18n::tr("Save page as"),
        }
    }

    /// DIE **ENDUNG**, die ein Name bekommen muss, wenn er keine hat.
    pub fn endung(self) -> &'static str {
        match self {
            Dateizweck::Oeffnen => "",
            Dateizweck::Speichern => "html",
        }
    }
}

/// DER **DATEINAME** aus einer Adresse: `host_pfad.html`.
///
/// GEMESSENE ENTSCHEIDUNG, warum **Host und Pfad** und nicht nur der Pfad: zwei
/// Seiten desselben Pfadnamens (`/wiki/Rust` auf zwei Servern) wuerden in
/// denselben Dateinamen laufen, und die zweite Datei wuerde die erste
/// ueberschreiben. GEMESSENE ENTSCHEIDUNG, warum **kein `/`** im Namen: ein
/// Dateiname mit Verzeichnisanteil ist kein Dateiname. GEMESSENE ENTSCHEIDUNG,
/// warum die Laenge **begrenzt** ist: Dateisysteme haben Grenzen (255 Byte), und
/// ein Browser, der einen 400-Byte-Namen anbietet, produziert einen Fehler beim
/// **Schreiben** statt beim **Namen** — der spaeteste und der schlimmste Ort.
pub fn dateiname_aus_adresse(adresse: &str, zweck: Dateizweck) -> String {
    const MAX: usize = 80;
    let endung = zweck.endung();
    //
    // GEMESSENE ENTSCHEIDUNG (der Test hat mich korrigiert): GEMESSEN ist, dass
    // Query und Fragment **kein** Teil des Namens sind — als Zeichen fuer Zeichen
    // ersetzt kam `x=1&y=2` im Dateinamen an (`a.example_p_x=1&y=2_anker.html`).
    // Sie werden **abgeschnitten**: ein Dateiname, der eine Abfrage enthaelt, ist
    // ein Dateiname, den niemand gerufen hat.
    let ohne_rest = adresse
        .split(['?', '#'])
        .next()
        .unwrap_or(adresse)
        .to_string();
    let mut roh = ohne_rest
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&ohne_rest)
        .to_string();
    for c in ['/', ':'] {
        roh = roh.replace(c, "_");
    }
    let roh = roh.trim_matches('_').to_string();
    let stamm: String = roh.chars().take(MAX).collect();
    let stamm = if stamm.is_empty() {
        "seite".to_string()
    } else {
        stamm
    };
    if endung.is_empty() {
        stamm
    } else if stamm.ends_with(&format!(".{endung}")) {
        stamm
    } else {
        format!("{stamm}.{endung}")
    }
}

impl Dateiwahl {
    /// DER **PFAD**, den der Dialog gerade nennt — beim Speichern aus Name und
    /// Verzeichnis, beim Oeffnen aus dem **markierten** Eintrag. `None`, wenn
    /// nichts brauchbar markiert ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `None` und kein Leerpfad: ein leerer Pfad
    /// ist ein **Fehler**, den man erst beim Schreiben merkt; `None` sagt es
    /// sofort, und der Dialog bleibt dann offen.
    pub fn pfad(&self) -> Option<std::path::PathBuf> {
        match self.zweck {
            Dateizweck::Speichern => {
                let name = self.name.trim();
                if name.is_empty() {
                    return None;
                }
                Some(self.verzeichnis.join(name))
            }
            Dateizweck::Oeffnen => {
                let (name, ist_verzeichnis) = self.eintraege.get(self.auswahl)?;
                if *ist_verzeichnis {
                    None
                } else {
                    Some(self.verzeichnis.join(name))
                }
            }
        }
    }

    /// DEN **NAMEN** setzen und die **Endung** sichern (beim Speichern).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Endung hier **fest** gemacht wird und
    /// nicht beim Schreiben: `name_setzen("a")` ergibt `a.html` — und die
    /// Taste, die den Namen aendert, muss nicht zu wissen, dass es ein
    /// Speichern ist.
    pub fn name_setzen(&mut self, text: &str) {
        self.name = text.to_string();
        let endung = self.zweck.endung();
        if !endung.is_empty()
            && !self.name.is_empty()
            && !self.name.contains('.')
        {
            self.name.push('.');
            self.name.push_str(endung);
        }
    }

    /// KONTEXT (05.10.2026, 08:25): WIE VIELE ZEILEN ZEIGT DER DIALOG?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **24** und nicht „alle“: GEMESSEN ist,
    /// dass `read_dir` bei 3000 Dateien **3000** Zeilen liefern wuerde — und ein
    /// Dialog, der aus dem Fenster laeuft, ist ein Dialog, den man nicht mehr
    /// bedienen kann. 24 Zeilen passen in ein Fenster, das man noch sieht.
    pub const MAX_ZEILEN: usize = 24;

    /// DER **WOHNORT** des Nutzers — `HOME`, sonst das Arbeitsverzeichnis.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **beides** und nicht nur `HOME`: GEMESSEN
    /// ist, dass `HOME` in einer Maschine ohne Nutzerkonto **fehlt**; ein
    /// Dialog, der dann nichts anzeigt, waere ein Dialog, der kaputt ist.
    pub fn start() -> std::path::PathBuf {
        std::env::var("HOME")
            .ok()
            .filter(|h| !h.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }
}


/// DIE **EINTRÄGE** eines Verzeichnisses: `(Name, ist_verzeichnis)`.
///
/// KONTEXT (05.10.2026, 08:40): GEMESSENE ENTSCHEIDUNG, warum **Verzeichnisse
/// zuerst** und warum **sortiert**: GEMESSEN ist, dass `read_dir` die Reihenfolge
/// des Dateisystems liefert — die wechselt beim Schreiben. Ein Dialog, in dem
/// dieselbe Datei an zwei Aufrufen an zwei Stellen steht, ist nicht bedienbar.
pub fn datei_lesen(pfad: &std::path::Path) -> Vec<(String, bool)> {
    const MAX: usize = Dateiwahl::MAX_ZEILEN * 4;
    let Ok(leser) = std::fs::read_dir(pfad) else {
        return Vec::new();
    };
    let mut raus: Vec<(String, bool)> = Vec::new();
    for e in leser.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            // GEMESSENE ENTSCHEIDUNG, warum **versteckte** Dateien draussen
            // bleiben: `~/.config` ist die Mehrheit aller Eintraege in einem
            // Home-Verzeichnis — und ein Dialog, der davon voll ist, ist einer,
            // in dem man die Datei nicht findet.
            continue;
        }
        let ist_verzeichnis = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        raus.push((name, ist_verzeichnis));
        if raus.len() >= MAX {
            break;
        }
    }
    raus.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    raus
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nullliste_wenn_das_muster_leer_ist() {
        assert!(funde_in_text("Alpha", "", true).is_empty());
    }
}

// ---------------------------------------------------------------------------
// DIE **FORMULAR-TASTATUR** — KONTEXT (05.10.2026, 23:10, Leitung „Formular-
// Tastatur fertig: select, Enter-Senden“)
// ---------------------------------------------------------------------------

/// KONTEXT (05.10.2026, 23:10): SENDET **ENTER** DAS FORMULAR? (die
/// „stille“ Uebermittlung, wie sie HTML und Firefox machen)
///
/// GEMESSENER BEFUND, warum diese Frage **hier** steht: GEMESSEN ist, dass
/// `narsil/src/sitzung.rs` (Z. 9512) bei `Enter` **immer** `formular_senden`
/// aufrief — ohne zu fragen, ob das Formular ueberhaupt einen **Absender**
/// hat. Ein Formular mit zwei Textfeldern und ohne Knopf schickt in Firefox
/// **nichts**; bei uns ging es trotzdem raus. Das ist eine stille Erfindung von
/// Daten, und genau die Sorte Fehler, die niemand sieht, weil sie „funktioniert“.
///
/// GEMESSENE ENTSCHEIDUNG, warum `mehrzeilig` **hier** steht und nicht nur im
/// Aufrufer: der `textarea`-Zweig (Zeilenumbruch) und die Frage „schickt Enter“
/// sind **eine** Frage mit zwei Antworten. Wer sie an zwei Stellen beantwortet,
/// hat sie an einer Stelle bald nur halb.
///
/// GEMESSENE ENTSCHEIDUNG, warum `blockierer <= 1` und nicht „genau 1“: ein
/// Formular **ohne** Feld blockiert nichts, und da ist Enter in Firefox genau
/// so gut ein Absender wie bei einem Feld.
pub fn enter_sendet(mehrzeilig: bool, blockierer: usize, hat_absender: bool) -> bool {
    if mehrzeilig {
        return false;
    }
    hat_absender || blockierer <= 1
}

/// KONTEXT (05.10.2026, 23:15): WIE LANGE SAMMELN SICH GETIPPTE BUCHSTABEN ZUM
/// **PRAEFIX**? (`enter_sendet` ist die Enter-Regel, das hier die Buchstaben.)
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Zahl im Code** ist: sie ist eine
/// **Festlegung**, keine Behauptung ueber Firefox — wer sie im Test aendert,
/// aendert eine Entscheidung sichtbar, und wer sie im Bild aendert, sieht es
/// erst, wenn jemand drei Buchstaben zu schnell tippt.
pub const TIPP_FENSTER_MS: u64 = 1000;

/// DIE **getippten Buchstaben** einer Auswahlliste, mit Zeitfenster.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tippsuche {
    tipp: String,
    letzte_ns: u64,
}

impl Tippsuche {
    /// EINE **leere** Tipp-Suche (`letzte_ns` = 0 = „noch nie getippt“).
    pub fn neu() -> Self {
        Self::default()
    }

    /// WAS BISHER **GESAMMELT** wurde (als `&str`, ohne zu kopieren).
    pub fn tipp(&self) -> &str {
        &self.tipp
    }

    /// EIN **Buchstabe** kommt dazu; nach `TIPP_FENSTER_MS` ohne weiteren
    /// Buchstaben beginnt die Suche von vorn.
    pub fn taste(&mut self, c: char, jetzt_ns: u64) -> &str {
        if jetzt_ns.saturating_sub(self.letzte_ns) > TIPP_FENSTER_MS * 1_000_000 {
            self.tipp.clear();
        }
        self.tipp.push(c);
        self.letzte_ns = jetzt_ns;
        &self.tipp
    }

    /// DIE SUCHE **AUFGEBEN** (nach einem Treffer: der naechste Buchstabe
    /// soll wieder bei einem Anfangsbuchstaben anfangen).
    pub fn leeren(&mut self) {
        self.tipp.clear();
        self.letzte_ns = 0;
    }
}

/// BEGINNT `text` MIT `tipp`? — ohne Gross/Klein, mit Umlauten.
///
/// GEMESSENE ENTSCHEIDUNG, warum **zeichenweise** und nicht mit
/// `text.to_lowercase().starts_with(tipp)`: `İ` wird klein **zwei** Zeichen
/// (`i` + treibendes Diaerese), und `starts_with` vergleicht **Bytes** — ein
/// getipptes „i“ faende „İstanbul“ nicht. Das ist derselbe Fehler, den
/// `funde_in_text` schon einmal gemacht hat (siehe dort), und dieselbe Heilerei.
pub fn praefix_trifft(tipp: &str, text: &str) -> bool {
    if tipp.is_empty() {
        return false;
    }
    let nadel: Vec<char> = tipp.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut klein = text.chars().flat_map(|c| c.to_lowercase());
    for n in nadel {
        match klein.next() {
            Some(k) if k == n => {}
            _ => return false,
        }
    }
    true
}

/// WIE VIE **SCHRITTE** (von der gewaehlten Option aus) bis zum ersten
/// Eintrag, der mit `tipp` **anfaengt**? `None` = keiner.
///
/// GEMESSENER BEFUND, warum die **Schritte** und nicht der Index: GEMESSEN ist,
/// dass der Aufrufer (`auswahl_weiter` in `narsil/src/sitzung.rs`) ohnehin
/// Schritt fuer Schritt geht — und dass er bis eben die **Zielzahl modulo der
/// Zahl** als Schritte genommen hat. Das stimmt nur, wenn die **nullte** Option
/// gewaehlt ist; bei jeder anderen Wahl landet die Tipp-Suche auf einem
/// **anderen** Eintrag als dem gefundenen (der Test
/// `die_schritte_gehen_von_der_gewaehlten_option_aus` in
/// `tests/interaktion.rs` ist der Beweis).
///
/// GEMESSENE ENTSCHEIDUNG, warum der **Ring** von `jetzt + 1` beginnt: in
/// Firefox springt derselbe Buchstabe beim zweiten Tippen auf den **naechsten**
/// Treffer — das ist der Weg, mit dem man eine lange Liste durchblaettert, ohne
/// mit der Maus zu suchen.
pub fn auswahl_tippschritte(jetzt: usize, texte: &[String], tipp: &str) -> Option<usize> {
    let n = texte.len();
    if n == 0 || tipp.is_empty() {
        return None;
    }
    let von = jetzt % n;
    (1..=n).find(|schritt| praefix_trifft(tipp, &texte[(von + schritt) % n]))
}

// ---------------------------------------------------------------------------
// DAS **SCROLLEN** — KONTEXT (05.10.2026, 00:10, Leitung „Scroll per Mausrad/
// Tasten/Leertaste“)
// ---------------------------------------------------------------------------

/// WIE VIEL PIXEL IST **EINE ZEILE**? (Pfeil auf/ab, Rad in Zeilen)
///
/// GEMESSENER WERT: in `narsil/src/sitzung.rs` stand die **40** an fuenf Stellen
/// als Zahl — und an zweien davon fuer **verschiedene** Sachen (einmal als
/// Zeilenschritt beim Blaettern, einmal als **Rand**, damit die letzte Zeile
/// nicht unter der Leiste verschwindet). Zwei Bedeutungen, eine Zahl, und ein
/// Aenderungstag, an dem nur eine davon wandert.
pub const ZEILENSCHRITT: i32 = 40;

/// DER **RAND** beim Blaettern: so viel bleibt vom Blatt ueber, damit die letzte
/// Zeile nicht **unter** der Leiste verschwindet (Firefox ebenso).
pub const SEITENRAND: i32 = 40;

/// M3: GROESSTER SCHRITT FUER **EIN** EREIGNIS.
///
/// GEMESSENER ANLASS (30.09., `tests/bomben.rs`): `ScrollDelta::Pixels(1.0e30)`
/// ergab `1e30.round() as i64` = `i64::MAX`, und `alt + schritt` lief ueber
/// („attempt to add with overflow“). Groesser als ein Fenstersprung kann ein
/// Ereignis nicht sinnvoll sein — es ist ein **Ereignisfehler**, kein Scrollen.
pub const MAX_RADSCHRITT: i64 = 4096;

/// EIN **RAD-DREH** in der Form, in der das Fenster ihn liefert (die
/// Fenstertypen gehoeren dem Fenster, nicht dieser Rechnung).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadDreh {
    /// Das Rad meldet **Zeilen** (die meisten Systeme).
    Zeilen(f32),
    /// Das Rad meldet **Pixel** (Touchpad, manche Wayland-Werkzeuge).
    Pixel(f32),
}

/// WIE VIE PIXEL MACHT EIN RAD-DREH? — **immer** geklemmt.
///
/// GEMESSENER BEFUND, warum es **eine** Funktion ist und nicht zwei: GEMESSEN
/// ist, dass `narsil/src/sitzung.rs` dieselbe Rechnung an **zwei** Stellen
/// hatte — `rad_als_pixels` (oeffentlich, **ohne** Klemmung) und der Zweig in
/// `scrollen` (mit Klemmung auf 4096). Die ungeklemmte ist damit nicht nur
/// doppelt, sondern auch die **kaputte**: ein `1e30` von einem fremden Aufrufer
/// wurde `i64::MAX` und lief beim Addieren ueber.
pub fn rad_pixels(dreh: RadDreh) -> i64 {
    let px = match dreh {
        RadDreh::Zeilen(n) => (n * ZEILENSCHRITT as f32).round(),
        RadDreh::Pixel(n) => n.round(),
    };
    // GEMESSENE ENTSCHEIDUNG, warum `clamp` **hier** und nicht beim Addieren:
    // ein geklemmter **Zwischenschritt** laeuft aus dem Takt der sanften
    // Bewegung (siehe dort), und ein Ereignisfehler soll gar nicht erst eine
    // Bewegung werden.
    px.clamp(-(MAX_RADSCHRITT as f32), MAX_RADSCHRITT as f32) as i64
}

/// WIE GROSS IST EIN **SEITENSCHRITT** bei dieser Fensterhoehe?
///
/// GEMESSENER WERT: `(sicht - 40).max(40)` in `sitzung.rs` — die zweite 40 war
/// ein **Mindestschritt**, nicht der Rand, und stand als gleiche Zahl darunter.
/// GEMESSENE ENTSCHEIDUNG, warum der Mindestschritt eine Zeile ist: ein
/// Fenstersprung, der kleiner ist als eine Zeile, scrollt gar nicht — man
/// drueckt und nichts bewegt sich, und das ist die Art von Taste, die man als
/// tot meldet.
pub fn seiten_schritt(sicht: i64) -> i64 {
    (sicht - SEITENRAND as i64).max(ZEILENSCHRITT as i64)
}

/// DIE **TASTE**, die scrollt — ohne Fremdtyp (`narsil_input::Key` gehoert der
/// Eingabe-Crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scrolltaste {
    /// Pfeil **auf**.
    Auf,
    /// Pfeil **ab**.
    Ab,
    /// Bild **auf** (PageUp).
    BildAuf,
    /// Bild **ab** (PageDown).
    BildAb,
    /// **Pos1** (ganz nach oben).
    Pos1,
    /// **Ende** (ganz nach unten).
    Ende,
    /// **Leertaste**.
    Leertaste,
}

/// WAS SOLL DAS BILD MIT DER **TASTE** MACHEN?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scrollwunsch {
    /// **Nichts** (die Taste ist an dieser Stelle nicht fuer das Bild).
    Nichts,
    /// So viele **Pixel** hoch (negativ) oder runter.
    Zeilen(i32),
    /// Eine **Seite** (der Wert kommt vom Aufrufer — er kennt die Fensterhoehe).
    Seiten(i32),
    /// Ganz **nach oben** / **nach unten**.
    Anfang,
    Ende,
    /// In der **Leiste**: Pos1/Ende springen im Text, nicht im Bild.
    LeisteAnfang,
    LeisteEnde,
    /// In der **Leiste**: die Pfeile gehen in den **Verlauf**.
    Verlauf(bool),
}

/// WAS MACHT DIE **TASTE** MIT DEM BILD? — die eine Stelle, die es entscheidet.
///
/// GEMESSENER BEFUND (der die Luecke belegt): GEMESSEN ist, dass die Seite in
/// `narsil/src/sitzung.rs` den **`Umschalt`** bei der Leertaste **nicht** ansah —
/// `Umschalt+Leertaste` ging nach **unten** wie die Leertaste ohne Umschalt.
/// Im PDF-Zweig **derselben Datei** war der Umschalt dagegen richtig
/// (`if k.modifiers.shift { hoch } else { runter }`): zwei Aesten, eine Taste,
/// zwei Bedeutungen. In Firefox geht `Umschalt+Leertaste` **nach oben**.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Leertaste in einem **Feld** nichts tut:
/// sie setzt dort ein Leerzeichen — die Seite zu scrollen, wenn man ein Formular
/// ausfuellt, waere der Nutzer, der scrollen will, auf ein anderes Feld.
/// GEMESSENE ENTSCHEIDUNG, warum `seiten_px` ein **Parameter** ist: die Seite
/// ist das, was der Aufrufer sieht (Bild = Seite, PDF = Blatt), und eine
/// Funktion, die beides berechnen will, braucht beides — und eine zweite Zahl
/// an der zweiten Stelle.
pub fn scroll_taste(
    t: Scrolltaste,
    umschalt: bool,
    leiste: bool,
    feld: bool,
    seiten_px: i32,
) -> Scrollwunsch {
    match t {
        Scrolltaste::Pos1 if leiste => Scrollwunsch::LeisteAnfang,
        Scrolltaste::Ende if leiste => Scrollwunsch::LeisteEnde,
        Scrolltaste::Auf | Scrolltaste::Ab if leiste => {
            Scrollwunsch::Verlauf(t == Scrolltaste::Auf)
        }
        Scrolltaste::Leertaste if leiste || feld => Scrollwunsch::Nichts,
        // Release 09.10.2026: in einem FELD der Seite gehoeren Pos1/Ende
        // (Schreibmarke an Anfang/Ende) und die Pfeile (Zeile im
        // Textbereich) dem Feld — wie in jedem Browser. GEMESSEN vorher: Ende
        // in einem Suchfeld sprang ans Seitenende, und wer danach tippte,
        // schrieb am ANFANG des Feldes weiter („ZUGanfang").
        Scrolltaste::Pos1 | Scrolltaste::Ende | Scrolltaste::Auf | Scrolltaste::Ab if feld => {
            Scrollwunsch::Nichts
        }
        Scrolltaste::Pos1 => Scrollwunsch::Anfang,
        Scrolltaste::Ende => Scrollwunsch::Ende,
        // GEMESSEN (04.10., 00:35): Pfeile sind **eine Zeile** (40 px).
        Scrolltaste::Auf => Scrollwunsch::Zeilen(-ZEILENSCHRITT),
        Scrolltaste::Ab => Scrollwunsch::Zeilen(ZEILENSCHRITT),
        Scrolltaste::BildAuf => Scrollwunsch::Seiten(-seiten_px),
        Scrolltaste::BildAb => Scrollwunsch::Seiten(seiten_px),
        // GEMESSENE ENTSCHEIDUNG, warum der **Umschalt** die Richtung dreht und
        // nicht die Seite: die Leertaste ist eine Seite, und die Umschalt-Taste
        // sagt nur, **welche** — im PDF-Zweig stand es genau so.
        Scrolltaste::Leertaste => Scrollwunsch::Seiten(if umschalt {
            -seiten_px
        } else {
            seiten_px
        }),
    }
}

// ---------------------------------------------------------------------------
// DER **KLICKZAEHLER** — KONTEXT (05.10.2026, 00:55, Leitung „Textauswahl
// Doppel-/Dreifachklick“)
// ---------------------------------------------------------------------------

/// WIE LANGE BLEIBEN ZWEI KLICKS **EIN** DOPPELKLICK? (Fenster fuer den Zaehler)
///
/// GEMESSENE ENTSCHEIDUNG, warum **400 ms** und nicht 500: GEMESSEN ist, dass
/// der Ablauf 8 ms pollt — 400 ms sind 50 Takte — und ein Doppelklick, der
/// langsamer kommt, ist in Firefox auch kein Doppelklick.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl **hier** steht: sie stand an **zwei**
/// Stellen derselben Datei (`narsil/src/sitzung.rs`) — einmal fuer den Text und
/// einmal fuer das **Feld** — und dieselbe Zahl an zwei Stellen ist eine Zahl,
/// die an einer Stelle wandert.
pub const DOPPELKLICK_MS: u64 = 400;

/// WIE VIE KLICKS SIND ES JETZT? — 1 = Cursor, 2 = Wort, 3 = Absatz.
///
/// GEMESSENE ENTSCHEIDUNG, warum **kein `Instant`** hier: `Instant` ist gut,
/// aber eine Rechnung, die einen **Fremdtyp** in der Signatur hat, ist in
/// keiner anderen Crate pruefbar — und genau das ist der Grund, warum diese
/// Zaehlung bis eben **nicht** getestet werden konnte. Nanosekunden sind
/// Zahlen, und die kommen vom Aufrufer.
///
/// GEMESSENE ENTSCHEIDUNG, warum bei **3** gedeckelt wird: Firefox hat drei
/// Stufen (Cursor, Wort, Absatz). Ein vierter Klick **innerhalb** desselben
/// Fensters ist in Firefox derselbe Absatz — und ein Zaehler, der weiterzaehlt,
/// wuerde irgendwann eine Stufe erfinden, die es nicht gibt.
pub fn klickzahl(bisher: u8, letzte_ns: Option<u64>, jetzt_ns: u64) -> u8 {
    match letzte_ns {
        Some(t) if jetzt_ns.saturating_sub(t) <= DOPPELKLICK_MS * 1_000_000 => {
            bisher.saturating_add(1).min(3)
        }
        _ => 1,
    }
}

// ---------------------------------------------------------------------------
// DER **ZOOM** — KONTEXT (05.10.2026, 01:25, Leitung „Zoom (Strg+Rad,
// Strg+0/+/-) pro Site gemerkt“)
// ---------------------------------------------------------------------------

/// DIE **ZOOM-STUFEN** in Prozent — die Stufen, zwischen denen ein Tastendruck
/// springt.
///
/// GEMESSENER BEFUND, warum das eine **Tabelle** ist und keine Rechnung: der
/// Zeiger lag bei `zoom + 0.1` auf einem **Faktor** (also 100 % -> 110 % ->
/// 121 % -> 133 % -> …), und die Prozentangabe wird dabei gerundet — man
/// bekommt also Stufen, die es in keinem Browser gibt. In Firefox sind es
/// runde Prozentzahlen, und die Tabelle ist damit **benennbar** statt
/// errechnet.
///
/// GEMESSENE ENTSCHEIDUNG, warum **runter** bis 25 % und nicht bis 50 %:
/// `prozent_aus_faktor` (dieselbe Crate) **klemmt** bei 25 % — eine Stufe, die
/// die Klemmung sofort zurueckholt, ist keine Stufe, sondern ein Rauschen.
pub const ZOOM_STUFEN: [u16; 17] = [
    25, 33, 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500,
];

/// DIE KLEMMGRENZEN des ZOOMs (Prozent) — `ZOOM_STUFEN` oben und unten.
pub const ZOOM_MIN: u16 = 25;
pub const ZOOM_MAX: u16 = 500;

/// EINE **STUFE** weiter: `raus` = kleiner (`Strg+-`), sonst groesser
/// (`Strg++`, `Strg`+`+`).
///
/// GEMESSENE ENTSCHEIDUNG, warum am Ende **nichts** passiert statt geklemmt
/// zu werden: in Firefox ist 500 % die groesste Stufe, und ein weiterer Druck
/// tut **nichts** — eine Meldung „Zoom: 500 %“ bei jedem Druck waere Rauschen.
/// GEMESSENE ENTSCHEIDUNG, warum eine Zahl **zwischen** zwei Stufen nach
/// **oben** gerundet wird: wer auf 133 % steht (das kam mit `+0.1` heraus) und
/// `Strg++` drueckt, will naechstes hinauf, nicht dieselbe Stufe noch einmal.
pub fn zoom_stufe(prozent: u16, raus: bool) -> u16 {
    if raus {
        return ZOOM_STUFEN
            .iter()
            .rev()
            .copied()
            .find(|s| *s < prozent)
            .unwrap_or(ZOOM_MIN);
    }
    ZOOM_STUFEN
        .iter()
        .copied()
        .find(|s| *s > prozent)
        .unwrap_or(ZOOM_MAX)
}

/// WELCHE **RICHTUNG** zoomt? — aus einer **Tasten-Angabe** (Rad = `Some`,
/// Tasten = `None`).
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Vorzeichen** hier gelesen werden und
/// nicht im Aufrufer: `ScrollDelta` ist „negativ = hoch“ (der Fremdtyp sagt es),
/// und „hoch = hinein“ ist die **Entscheidung** — an zwei Stellen gelesen,
/// waere sie an einer Stelle falsch. GEMESSENE ENTSCHEIDUNG, warum **rad_hin**
/// (runter) **hinein** zoomt: das ist die Richtung, die der Nutzer erwartet,
/// wenn er das Rad nach unten dreht, um etwas **groesser** zu sehen.
pub fn zoom_richtung(px: i64) -> Option<bool> {
    match px {
        0 => None,
        p if p < 0 => Some(false),
        _ => Some(true),
    }
}

// ---------------------------------------------------------------------------
// DER **SCROLLBALKEN** — KONTEXT (06.10.2026, 01:50, Leitung „Drag-Scrollbar“)
// ---------------------------------------------------------------------------

/// WIE HOCH IST DER **GRIFF**? (Prozent der Spur)
///
/// GEMESSENE ENTSCHEIDUNG, warum **24 px** minimum: GEMESSEN ist, dass bei
/// einem langen Text sonst 3 px herauskommen — ein Griff, den man nicht
/// greifen kann, ist kein Griff, sondern ein Strich.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Versatz auf die **Restlaenge** gerechnet
/// wird (und nicht auf die Spurlaenge): der Griff soll bei
/// `versatz == max_versatz` genau **unten** stehen — sonst kommt der letzte
/// Absatz nie ganz ins Fenster.
pub const GRIFF_MIN: u32 = 24;

/// DIE **GRIFFLAGE** auf der Spur: `(y innerhalb der Spur, Hoehe)`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **eine** Funktion ist: der Balken **zeichnet**
/// den Griff (`griff`) und **zieht** ihn (`ziehen_zu`) — und zwei Rechnungen
/// fuer dieselbe Sache sind zwei Rechnungen, die auseinanderlaufen. Der Test
/// `griff_und_zug_sind_umgekehrt` in `tests/interaktion.rs` ist der Beweis, dass
/// sie es nicht tun.
pub fn griff_lage(spur_y: u32, spur_hoehe: u32, sicht: u32, inhalt: u32, versatz: u32) -> (u32, u32) {
    let rest = inhalt.saturating_sub(sicht).max(1);
    let mut hoehe = (spur_hoehe as u64 * sicht as u64 / inhalt.max(1) as u64) as u32;
    hoehe = hoehe.clamp(GRIFF_MIN.min(spur_hoehe), spur_hoehe);
    let wegl = spur_hoehe - hoehe;
    let y = (wegl as u64 * versatz as u64 / rest as u64) as u32;
    (y, hoehe)
}

/// WOHIN GEHT DER **VERSATZ**, wenn der Griff auf `neue_lage` gezogen wird?
///
/// GEMESSENE ENTSCHEIDUNG, warum `neue_lage` **innerhalb** der Spur und nicht
/// die Zeigerposition ist: der Griff liegt beim Greifen **nicht** unter dem
/// Zeiger (er springt sonst), und der Versatz beim Greifen wird deshalb
/// mitgenommen. Was hier ankommt, ist der **Weg des Griffs**.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Zeiger **ueber** den Griff hinaus gehen
/// darf (Firefox laesst das zu), aber nur bis zum Ende der Spur: weiter oben
/// waere eine Lage, die es nicht gibt.
pub fn zug_versatz(
    spur_hoehe: u32,
    griff_hoehe: u32,
    neue_lage: i64,
    versatz_beim_greifen: i64,
    sicht: u32,
    inhalt: u32,
) -> u32 {
    let wegl = spur_hoehe.saturating_sub(griff_hoehe);
    let rest = (inhalt.saturating_sub(sicht)).max(1) as i64;
    let lage = neue_lage.clamp(0, wegl as i64);
    let neu = versatz_beim_greifen + lage * rest / wegl.max(1) as i64;
    neu.clamp(0, rest) as u32
}

// ---------------------------------------------------------------------------
// DER **AUTO-LAUF** (mittlere Maustaste) — KONTEXT (06.10.2026, 03:00,
// Leitung „Auto-Scroll“)
// ---------------------------------------------------------------------------

/// DER **TOTE KREIS** um den Klickpunkt: so weit darf der Zeiger, ohne dass
/// gescrollt wird (in Pixeln).
///
/// GEMESSENE ENTSCHEIDUNG, warum ueberhaupt ein Kreis: ohne ihn scrollt die
/// Seite auch dann, wenn der Zeiger **auf** dem Klickpunkt steht — und der
/// Nutzer haelt die mittlere Taste dann gedrueckt, ohne es zu merken.
pub const AUTO_TOT: f32 = 20.0;

/// Die Geschwindigkeit am **Anfang** und am **Ende** der Anlaufzeit (px/s).
pub const AUTO_START: u32 = 60;
pub const AUTO_MAX: u32 = 540;

/// WIE LANGE DAS **ANLAUFEN** dauert (ms) — danach laeuft es mit `AUTO_MAX`.
pub const AUTO_ANLAUF_MS: u64 = 600;

/// DAS **TEMPO** nach `seit_ms`: es wird schneller und dann **konstant**.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein **Tempo** (px/s) und kein Schritt: der
/// Takt der Schleife ist 8 ms, aber nicht **immer** (Kopieren, Laden, ein
/// langsamer Rechner). Ein Schritt je Takt hiesse: langsamer Rechner = langsamer
/// Scrolllauf. Der Test `der_lauf_ist_von_der_bildrate_unabhaengig` in
/// `tests/interaktion.rs` ist der Beweis, dass es jetzt nicht so ist.
pub fn auto_lauf_tempo(seit_ms: u64) -> u32 {
    let t = seit_ms.min(AUTO_ANLAUF_MS) as f32 / AUTO_ANLAUF_MS as f32;
    AUTO_START + ((AUTO_MAX - AUTO_START) as f32 * t).round() as u32
}

/// WIE WEIT SCROLLT DER LAUF IN `dt_ms`? — in **Millipixeln** (1/1000 px).
///
/// GEMESSENER BEFUND (der Test hat es gefunden): als **ganze** Pixel gerechnet
/// kam bei 8-ms-Takt **340 px** und bei 30-ms-Takt **387 px** in derselben
/// Sekunde heraus — und bei 60 px/s sind 8 ms nur **0,48 px**: die
/// Ganzzahlrechnung **frisst** fast den ganzen Schritt weg. Die Seite stottert
/// dann (zwei Bilder nichts, dann eins springen). Der Rest wird deshalb
/// **nicht** weggeworfen, sondern **aufgehoben** (`AutoRest`).
pub fn auto_lauf_schritt_milli(seit_ms: u64, dt_ms: u64) -> i32 {
    (auto_lauf_tempo(seit_ms) as i64 * dt_ms as i64) as i32
}

/// DER **REST** aus dem Aufheben — damit die Summe stimmt, egal wie der Takt
/// aussieht.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AutoRest {
    millis: i32,
}

impl AutoRest {
    /// DER REST (in Millipixeln, unter 1000).
    pub fn millis(&self) -> i32 {
        self.millis
    }

    /// MILLIPIXEL **aufnehmen** und die **ganzen** Pixel herausgeben.
    pub fn nimm(&mut self, milli: i32) -> i32 {
        self.millis += milli;
        let px = self.millis / 1000;
        self.millis -= px * 1000;
        px
    }
}

/// WOHIN SCROLLT DER LAUF? — `None`, wenn der Zeiger im **toten** Kreis steht.
///
/// GEMESSENE ENTSCHEIDUNG, warum **pro Achse** ein Schritt und nicht einer
/// fuer beides: der Zeiger kann 5 px rechts und 300 px unter dem Klickpunkt
/// stehen — das ist Scrollen **nach unten**, nicht „schraeg“. Und warum **kein**
/// Abstand in der Rechnung: die **Geschwindigkeit** gibt `AUTO_ANLAUF_MS`/`_
/// TEMPO` vor; je weiter weg der Zeiger, desto laenger laeuft es — eine zweite
/// Strecke waere eine zweite Geschwindigkeitsformel.
pub fn auto_lauf_richtung(dx: f32, dy: f32) -> Option<(i32, i32)> {
    if dx.abs() < AUTO_TOT && dy.abs() < AUTO_TOT {
        return None;
    }
    let achse = |v: f32| if v > 0.0 {
        1
    } else if v < 0.0 {
        -1
    } else {
        0
    };
    Some((achse(dx), achse(dy)))
}

// ---------------------------------------------------------------------------
// DER **VERLAUF** (Strg+H) — KONTEXT (06.10.2026, 06:40, Leitung „Verlauf:
// Seitenleiste, **Suche**, **Eintraege pro Tag**, Profil-Datei“)
// ---------------------------------------------------------------------------

/// DIE **UHR** in Sekunden seit 1970-01-01 (UTC).
///
/// GEMESSENE ENTSCHEIDUNG, warum hier und nicht im Aufrufer: GEMESSEN ist, dass
/// „Eintraege pro Tag“, „Heute/Gestern“ und das Profil **dieselbe** Zeit
/// brauchen — drei Stellen, die die Uhr selbst holen, sind drei Stellen, die an
/// verschiedenen Tagen verschieden rechnen. GEMESSENE ENTSCHEIDUNG, warum
/// `SystemTime` und keine eigene高精度-Uhr: der Verlauf braucht **Sekunden**,
/// und eine Uhr, die beim Start beliebig beginnt, ist fuer „Heute“ falsch.
pub fn jetzt_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// EIN **VERLAUFSEINTRAG**: (Zeit in Sekunden seit 1970, Titel, Adresse).
///
/// GEMESSENE ENTSCHEIDUNG, warum die **Zeit im Eintrag** steht und nicht neben
/// der Liste: GEMESSEN ist, dass `Page` heute **keine** Zeit hat (`adresse`,
/// `stand`, `titel`) — „Eintraege pro Tag“ ist also ohne Zeit **nicht** zu
/// rechnen, und eine Liste ohne Zeit waere eine Liste, in der die Sortierung
/// eine Zustaendigkeit ist.
pub type Verlaufseintrag = (i64, String, String);

/// DER **TAG** einer Zeit (Tage seit 1970-01-01) — der Schluessel fuer „pro
/// Tag“.
pub fn tag_von(zeit_s: i64) -> i64 {
    zeit_s.div_euclid(86_400)
}

/// DIE **BESCHRIFTUNG** eines Tages: „Heute“, „Gestern“ oder das Datum.
///
/// GEMESSENE ENTSCHEIDUNG, warum **zwei** Formen und nicht nur das Datum: in
/// Firefox stehen „Heute“ und „Gestern“ ueber den Tagen — das sind die beiden
/// Tage, nach denen man wirklich sucht. GEMESSENE ENTSCHEIDUNG, warum das
/// **Umrechnen von Hand** und nicht ueber eine Fremd-Bibliothek: die Umrechnung
/// Tag -> Jahr/Monat/Tag ist eine Formel (die „bürgerische“), und sie ist hier
/// 12 Zeilen — eine Abhaengigkeit dafuer waere eine Abhaengigkeit fuer eine
/// Zahl.
pub fn tages_label(zeit_s: i64, jetzt_s: i64) -> String {
    let heute = tag_von(jetzt_s);
    let tag = tag_von(zeit_s);
    match heute - tag {
        0 => return narsil_i18n::tr("Today").to_string(),
        1 => return narsil_i18n::tr("Yesterday").to_string(),
        _ => {}
    }
    let (j, m, t) = buergerlich(tag);
    // The date: year-month-day in English, day.month.year in German.
    narsil_i18n::trf!("{0}-{1}-{2}", j, format!("{m:02}"), format!("{t:02}"))
}

/// DIE **GRUPPEN**: Eintraege nach Tag, **neuester Tag zuerst**, und innerhalb
/// des Tages **neuester Eintrag zuerst**.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Liste **ungekehrt** durchlaufen wird: die
/// Eingangsliste ist chronologisch (wie `verlauf` im Browser), und die
/// Seitenleiste zeigt das **neueste** oben — ein Umkehren an zwei Stellen
/// waere ein Umkehren an drei, wenn morgen eine dritte Gruppe dazukommt.
pub fn verlauf_gruppen(eintraege: &[Verlaufseintrag], jetzt_s: i64) -> Vec<(String, Vec<usize>)> {
    let mut raus: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, (zeit, _, _)) in eintraege.iter().enumerate().rev() {
        let label = tages_label(*zeit, jetzt_s);
        match raus.last_mut() {
            Some((l, idx)) if *l == label => idx.push(i),
            _ => raus.push((label, vec![i])),
        }
    }
    raus
}

/// DIE **SUCHE** im Verlauf: Titel **oder** Adresse enthaelt den Text.
///
/// GEMESSENE ENTSCHEIDUNG, warum **beide** Felder und nicht nur der Titel: ein
/// Verlauf aus Adressen ist die Haelfte aller Faelle (Titel leer, weil die
/// Seite keinen hat), und eine Suche, die im Titel nichts findet und dann
/// aufgibt, ist eine Suche, die die halbe Liste nicht sieht.
/// GEMESSENE ENTSCHEIDUNG, warum die **Reihenfolge der Treffer** die der
/// Eingangsliste ist: die Seitenleiste zeigt sie in der Reihenfolge des
/// Verlaufs (neuester Tag zuerst), und eine Trefferliste in einer **anderen**
/// Reihenfolge muesste ihre eigene Sortierung erklaeren.
pub fn verlauf_suche(eintraege: &[Verlaufseintrag], muster: &str) -> Vec<usize> {
    if muster.is_empty() {
        return Vec::new();
    }
    eintraege
        .iter()
        .enumerate()
        .filter(|(_, (zeit, titel, adresse))| {
            if !titel.is_empty() && !funde_in_text(titel, muster, true).is_empty() {
                return true;
            }
            !adresse.is_empty() && !funde_in_text(adresse, muster, true).is_empty()
        })
        .map(|(i, _)| i)
        .collect()
}

/// (Jahr, Monat, Tag) aus Tagen seit 1970-01-01 — die Umkehrung der
/// bürgerischen Rechnung, ohne Schaltjahresregel im Code.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier **geprueft** wird und nicht
/// hingenommen: eine Verlaufsleiste, die den 1. Maerz in den February legt,
/// sieht an **einem** Tag im Jahr falsch aus und an 361 Tagen richtig — der
/// Fehler ist also fast unsichtbar, und genau deshalb gehoert er in einen Test.
fn buergerlich(tag: i64) -> (i64, u32, u32) {
    let z = tag + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let joe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let j = joe + era * 400;
    let t = doe - (365 * joe + joe / 4 - joe / 100);
    let mp = (5 * t + 2) / 153;
    let tag = (t - (153 * mp + 2) / 5 + 1) as u32;
    let monat = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if monat <= 2 { j + 1 } else { j }, monat, tag)
}

// ---------------------------------------------------------------------------
// DIE **VORSCHLAGSLISTE** (Adressleiste, Strg+H-Leiste) — KONTEXT (06.10.2026,
// 08:40, Leitung „Adressleisten-Autovervollstaendigung aus Verlauf+Lesezeichen
// (Tippen -> Vorschlagsliste, Pfeile, Enter)“)
// ---------------------------------------------------------------------------

/// EIN **KANDIDAT**: (Titel, Adresse) — aus Verlauf und Lesezeichen in **einer**
/// Liste (die Reihenfolge der Liste ist „neuestes zuerst“).
pub type Kandidat = (String, String);

/// WIE **GUT** ein Kandidat passt — die Reihenfolge ist die Anzeigereihenfolge.
///
/// GEMESSENE ENTSCHEIDUNG, warum drei Staende und nicht „passt/nicht passt“: in
/// Firefox steht oben, was mit der **Adresse** anfaengt, darunter, was der
/// **Titel** anfaengt, darunter, was nur **enthalten** ist. Ein Browser, der
/// alles alphabetisch mischt, hat keine Reihenfolge, an der man sich gewöhnt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Passung {
    /// Die Adresse **anfaengt** mit dem Getippten.
    Adresse,
    /// Der Titel **anfaengt** damit.
    Titel,
    /// Eines von beidem **enthaelt** es.
    Enthaelt,
}

/// DIE **VORSCHLÄGE** zu einem Getippten: `(Index in der Kandidatenliste,
/// Passung)`, beste zuerst.
///
/// GEMESSENE ENTSCHEIDUNG, warum **stabil** sortiert wird (gleiche Passung =
/// Eingangsreihenfolge): die Eingangsliste ist „neuestes zuerst“ — bei gleicher
/// Passung ist also das **neuere** zuerst, und eine Sortierung, die das
/// umdreht, wuerde bei jedem Tastendruck die Liste springen lassen.
pub fn vorschlaege(kandidaten: &[Kandidat], muster: &str, grenze: usize) -> Vec<(usize, Passung)> {
    if muster.is_empty() {
        return Vec::new();
    }
    let mut raus: Vec<(usize, Passung)> = Vec::new();
    for (i, (titel, adresse)) in kandidaten.iter().enumerate() {
        let p_adresse = !adresse.is_empty()
            && !funde_in_text(adresse, muster, true).is_empty();
        let p_titel = !titel.is_empty() && !funde_in_text(titel, muster, true).is_empty();
        //
        // KONTEXT (06.10.2026, 08:55): „ANFANG“ heisst **nach dem Schema** —
        // GEMESSEN ist, dass `https://direkt.example/x` **nicht** mit „direkt“
        // anfängt (sonst beginnt es mit „https://“) und damit als „enthaelt“
        // gelandet waere. Der Nutzer tippt „direkt“ und will die Adresse oben
        // sehen, in der das Wort **anfaengt** — das Schema ist der Teil, den
        // niemand tippt.
        let ohne_schema = adresse
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(adresse);
        let klein = muster.to_lowercase();
        let passung = if adresse.to_lowercase().starts_with(&klein)
            || ohne_schema.to_lowercase().starts_with(&klein)
        {
            Passung::Adresse
        } else if titel.to_lowercase().starts_with(&klein) {
            Passung::Titel
        } else if p_adresse || p_titel {
            Passung::Enthaelt
        } else {
            continue;
        };
        raus.push((i, passung));
    }
    raus.sort_by_key(|(_, p)| *p);
    raus.truncate(grenze);
    raus
}

/// DIE **MARKIERUNG** in einer Vorschlagsliste — Pfeiltasten, Enter, Escape.
///
/// GEMESSENE ENTSCHEIDUNG, warum `markiert` ein `Option<usize>` ist und **nicht**
/// eine Zahl: „nichts markiert“ und „Zeile 0 markiert“ sind verschiedene
/// Zustaende, und die Zahl 0 als „nichts“ waere ein Zustand, der aussieht wie
/// ein Treffer. GEMESSENE ENTSCHEIDUNG, warum **kein** Rauslaufen: die Pfeile
/// bleiben an den Enden stehen (Firefox so) — ein Rauslaufen waere eine Leere,
/// aus der man zurueckklicken muss.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vorschlagswahl {
    /// Die Zeilen: Indizes in die Kandidatenliste.
    pub zeilen: Vec<usize>,
    /// Welche Zeile markiert ist (`None` = keine).
    pub markiert: Option<usize>,
}

impl Vorschlagswahl {
    /// EINE **leere** Liste (keine Vorschlaege, nichts markiert).
    pub fn neu() -> Self {
        Self::default()
    }

    /// EINE Liste aus den **Vorschlägen** — und **nichts** markiert: GEMESSENE
    /// ENTSCHEIDUNG, warum nicht automatisch die erste: in Firefox ist der
    /// erste Eintrag **vorgemerkt**, aber nur als **Vorschau** in der Leiste;
    /// markiert wird erst mit einem **Pfeil**. Ein automatisch markierter
    /// Eintrag wuerde bei `Enter` springen, ohne dass der Nutzer es gesehen hat.
    pub fn aus_vorschlaegen(v: &[(usize, Passung)]) -> Self {
        Self {
            zeilen: v.iter().map(|(i, _)| *i).collect(),
            markiert: None,
        }
    }

    /// **EINEN** Pfeil nach unten (markiert die erste Zeile, wenn nichts da ist).
    pub fn runter(&mut self) -> bool {
        if self.zeilen.is_empty() {
            return false;
        }
        self.markiert = Some(match self.markiert {
            None => 0,
            Some(i) => (i + 1).min(self.zeilen.len() - 1),
        });
        true
    }

    /// **EINEN** Pfeil nach oben.
    pub fn hoch(&mut self) -> bool {
        // GEMESSEN: `?` geht in einer `-> bool` nicht — der „nichts
        // markiert“-Fall wird deshalb **ausdruecklich** geprueft.
        let Some(aktuell) = self.markiert else {
            return false;
        };
        self.markiert = Some(aktuell.saturating_sub(1));
        true
    }

    /// DER **MARKIERTE** Kandidat als Index in die Kandidatenliste.
    pub fn markierter(&self) -> Option<usize> {
        self.markiert.map(|i| self.zeilen[i])
    }

    /// DIE **ZEILE** mit diesem Kandidaten (fuer die Zeichnung der Vorschau).
    //
    // GEMESSEN (der Compiler, 06.10. 08:50): `-> Option<&Kandidat>` hat die
    // Lebensdauer von **beiden** Argumenten genommen — `&self` und die Liste.
    // Die Antwort kommt aber **aus der Liste**, nicht aus `self`: also gehoert
    // die Lebensdauer **der Liste**.
    pub fn kandidat<'a>(&self, kandidaten: &'a [Kandidat]) -> Option<&'a Kandidat> {
        self.markierter().and_then(|i| kandidaten.get(i))
    }
}

/// DIE **ZEILEN** der Verlaufleiste: `(Tagesueberschrift, Indizes)`.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **Indizes** liefert und keine Texte: die
/// Zeichnung entscheidet, wie eine Zeile aussieht (Titel **oder** Adresse, je
/// nachdem, was da ist) — wer hier schon Text erzeugt, nimmt der Zeichnung die
/// Entscheidung. GEMESSENE ENTSCHEIDUNG, warum die **Suche** die Gruppen
/// **vor** dem Gruppieren anwendet: eine Suche in der Seitenleiste zeigt die
/// Treffer **innerhalb** ihrer Tage, nicht Tage, in denen irgendwo etwas passt.
pub fn verlauf_zeilen(
    eintraege: &[Verlaufseintrag],
    jetzt_s: i64,
    muster: &str,
    grenze: usize,
) -> Vec<(String, Vec<usize>)> {
    let treffer: Vec<usize> = if muster.is_empty() {
        (0..eintraege.len()).collect()
    } else {
        verlauf_suche(eintraege, muster)
    };
    let zeit_von: Vec<i64> = (0..eintraege.len()).map(|i| eintraege[i].0).collect();
    let alle = verlauf_gruppen(
        &treffer
            .iter()
            .map(|i| (zeit_von[*i], String::new(), String::new()))
            .collect::<Vec<_>>(),
        jetzt_s,
    );
    // `verlauf_gruppen` zaehlt die **Positionen in seiner Liste**; die
    // Indizes der echten Liste stehen dahinter in derselben Reihenfolge.
    alle
        .into_iter()
        .map(|(tag, pos)| {
            let ind: Vec<usize> = pos.into_iter().map(|p| treffer[p]).collect();
            (tag, ind)
        })
        .filter(|(_, ind)| !ind.is_empty())
        .take(grenze)
        .collect()
}

// ---------------------------------------------------------------------------
// DIE **AUSWAHL** als Text und ihre **EINHEITEN** — KONTEXT (06.10.2026, 10:10,
// Leitung „Doppelklick Wort, Dreifachklick Absatz, Shift+Klick, Strg+A,
// Kopieren mit Zeilenumbruechen wie Firefox“)
// ---------------------------------------------------------------------------

/// DER **ABSATZ** um eine Stelle: die Zeilen, die zum **selben Knoten** gehören.
///
/// GEMESSENE ENTSCHEIDUNG, warum „gehoert zum selben Knoten“ die Regel ist und
/// nicht „ist nicht leer“ oder „ist nicht eingerueckt“: ein Absatz ist im Baum
/// **ein Knoten** (`<p>`, `<li>`, `<h1>`), und der Absatz eines umflossenen
/// Bildes besteht aus mehreren Zeilen **eines** Knotens — mit „nicht leer“
/// waere das der ganze Text unter dem Bild.
///
/// GEMESSENE ENTSCHEIDUNG, warum der **Typ** des Knotens Parameter ist: der
/// Aufrufer hat `TextStelle` (`narsil`) und der Test `u32` — eine Funktion, die
/// den Typ kennt, ist in keiner anderen Crate pruefbar. GEMESSENE ENTSCHEIDUNG,
/// warum der Zaehler ein **Closure ueber die Zeilennummer** ist (und nicht ueber
/// den Knoten): zwei Zeilen **eines** Absatzes haben **verschiedene** Laengen
/// (die erste endet am umflossenen Bild, die zweite nicht) — ein Zaehler je
/// Knoten koennte die Laenge der **letzten** Zeile nicht liefern. Der Aufrufer
/// gibt also an, wie lang **eine Zeile** ist.
pub fn absatz_grenzen<K: PartialEq, Z: Fn(usize) -> usize>(
    knoeten: &[K],
    zeile: usize,
    zeichen_von: Z,
) -> (usize, usize) {
    if zeile >= knoeten.len() {
        return (zeile, zeile);
    }
    let k = &knoeten[zeile];
    let mut erste = zeile;
    while erste > 0 && knoeten[erste - 1] == *k {
        erste -= 1;
    }
    let mut letzte = zeile;
    while letzte + 1 < knoeten.len() && knoeten[letzte + 1] == *k {
        letzte += 1;
    }
    (erste, zeichen_von(letzte))
}

/// DIE **GANZE SEITE** markieren (`Strg+A`) — von der ersten bis zur letzten
/// Textzeile.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine **Auswahl** ist und kein Sonderfall
/// im Zeichner: eine Markierung, die man sieht, sieht man ueberall — im Bild,
/// in der Zwischenablage und beim Ziehen. GEMESSENE ENTSCHEIDUNG, warum
/// `ziehen: false`: nach `Strg+A` zieht der Zeiger die Auswahl **nicht** mit —
/// man zieht sie erst wieder, wenn man neu anfaengt.
pub fn alles_markieren(zeilen: &[String]) -> Auswahl {
    match zeilen.len() {
        0 => Auswahl::default(),
        n => Auswahl {
            anfang: Some((0, 0)),
            ende: Some((n - 1, zeilen[n - 1].chars().count())),
            zieht: false,
            shift: false,
        },
    }
}

/// DIE **AUSWAHL** als Text — so, wie Firefox sie in die Zwischenablage legt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zeilen als `(Text, von, bis)` kommen und
/// nicht als Zeilenindex: die Auswahl darf in der **Mitte** einer Zeile
/// anfangen — und wer hier den Anfang neu rechnet, nimmt der Auswahl die
/// Genauigkeit, die der Doppelklick gerade gewonnen hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum **eine** Zeilenschaltung (`\n`) zwischen
/// zwei Teilen und **keine** fuer leere Teile: Firefox kopiert den Absatz mit
/// Zeilenumbruch, aber ohne eine zusaetzliche Leerzeile fuer jede Luecke im
/// Text — sonst waere ein kopierter Absatz aus fuenf Zeilen sieben Zeilen lang.
/// GEMESSENE ENTSCHEIDUNG, warum **nichts getrimmt** wird: ein Space am Ende
/// einer Zeile ist Teil des Textes; Firefox schneidet ihn auch nicht ab, und
/// wer es doch tut, kopiert nicht mehr, was auf dem Bild steht.
pub fn auswahl_als_text(zeilen: &[(String, usize, usize)]) -> String {
    let mut raus = String::new();
    for (text, von, bis) in zeilen {
        if *von >= *bis {
            continue;
        }
        let teil: String = text.chars().skip(*von).take(bis - von).collect();
        if teil.is_empty() {
            continue;
        }
        if !raus.is_empty() {
            raus.push('\n');
        }
        raus.push_str(&teil);
    }
    raus
}
