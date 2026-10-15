//! KONTEXT (05.10.2026, 22:40, Leitung): **EINGABE-EINSPEISUNG** — der Weg
//! vom Mausereignis zur Wirkung, **ohne** Fenster.
//!
//! GEMESSENE ENTSCHEIDUNG, warum es ein **Modul** ist und kein `#[cfg(test)]`:
//! GEMESSEN ist, dass die Tests in `crates/narsil/tests/` die Crate als
//! **normale** Abhaengigkeit einbinden — und dass ein `#[cfg(test)]`-Modul fuer
//! sie **nicht sichtbar** ist. Ein Testmodus, den nur die Unit-Tests sehen, ist
//! fuer genau die Tests nutzlos, die einen Klickweg beweisen sollen. Deshalb ist
//! es ein oeffentliches Modul, das **nur** Ereignisse einspeist und **nichts**
//! zeichnet.
//!
//! GEMESSENE ENTSCHEIDUNG, warum hier **Ereignisse** und keine Direktaufrufe:
//! GEMESSEN ist, dass `Sitzung::ereignis(&SurfaceEvent)` der Weg ist, den auch
//! das Fenster nimmt (`main.rs`), und dass `rechtsklick`/`mausklick` **darin**
//! liegen. Wer `rechtsklick` direkt aufruft, beweist die **eine** Funktion; wer
//! ein `ButtonPressed` einspeist, beweist den **Weg** — und der Weg ist die
//! Behauptung.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Klick **per Beschriftung** kommt und nicht
//! per Zeilennummer: eine Zahl in einem Test ist eine Behauptung ueber die
//! Reihenfolge; ein **Wort** ist eine Behauptung ueber das, was der Nutzer
//! anklickt. Wenn sich die Reihenfolge aendert, soll der Test an der Zeile
//! scheitern, die der Nutzer liest.

use narsil_core::geometry::Point;
use narsil_input::{KeyEvent, Modifiers};
use narsil_window::{MouseButton, SurfaceEvent};

use crate::sitzung::Sitzung;

/// EIN **RECHTSKLICK** an der Stelle — es geht genau der Weg, den das Fenster
/// geht (`ButtonPressed` mit der rechten Taste -> `Sitzung::rechtsklick`).
pub fn rechtsklick(s: &mut Sitzung, x: f32, y: f32) -> bool {
    s.ereignis(&SurfaceEvent::ButtonPressed {
        point: Point::new(x, y),
        button: MouseButton::Right,
    });
    true
}

/// EIN **LINKSKLICK** an der Stelle (der Weg fuer Menuezeilen und Links).
pub fn linksklick(s: &mut Sitzung, x: f32, y: f32) -> bool {
    s.ereignis(&SurfaceEvent::ButtonPressed {
        point: Point::new(x, y),
        button: MouseButton::Left,
    });
    true
}

/// EINE **TASTE** druecken (der Weg wie im Fenster).
pub fn taste(s: &mut Sitzung, k: narsil_input::Key, m: Modifiers) -> bool {
    s.ereignis(&SurfaceEvent::Key(KeyEvent {
        key: k,
        pressed: true,
        modifiers: m,
    }));
    true
}

/// DER **PUNKT** einer Menuezeile mit dieser **Beschriftung** — `None`, wenn es
/// die Zeile nicht gibt.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Mitte der Zeile und nicht die linke Kante:
/// `ZEILE` (28 px) und `LUFT_OBEN` (4 px) sind die Zahlen der Zeichnung; ein
/// Punkt am **Anfang** der Zeile kann knapp neben ihr liegen. Die Mitte liegt
/// in **jeder** Zeile.
pub fn zeilenpunkt(s: &Sitzung, beschriftung: &str) -> Option<Point> {
    // GEMESSEN: `Sitzung::menue` ist ein `RechtsklickMenue`, und der **Zustand**
    // (Zeilen, Rechteck) steckt in dessen Feld `ktx` — `s.tabmenue` ist die
    // **Tab**-Leiste und nicht das Kontextmenue.
    let m = &s.menue.as_ref()?.ktx;
    // Release 09.10.2026: mit der Fenstergroesse der Sitzung (vorher fest
    // 1000 px Hoehe) — die Zeilen, die ins Fenster passen, entscheidet sie.
    let (fb, fh) = (s.breite, s.hoehe);
    let nr = m
        .zeilen(fh)
        .iter()
        .position(|z| match z {
            narsil_chrome::ktx_menue::Zeile::Aktion(a) => a.text() == beschriftung,
            _ => false,
        })?;
    // GEMESSENE ENTSCHEIDUNG, warum `nr` **ohne** `as u32` weitergeht (E0308):
    // `Menue::zeilen_y` nimmt seit dem Trennlinien-Schritt (03.10., KONTEXT)
    // einen `usize` — der Zaehler `enumerate()` liefert genau den. Und
    // `Menue::x` ist ein `u32`, deshalb wird **er** zu `f32` (E0277); vorher
    // stand `m.x + 40.0`, was der Compiler als `u32 + {float}` las und der
    // Wegwerf-`as f32` um das `+ 40.0` herum legte.
    // Release 08.10.2026 (Glas-Stil): die Mitte kommt aus dem Menue selbst —
    // mit Ueberschriften und Skalierung gerechnet.
    //
    // Release 09.10.2026: `zeile_mitte_y` ist RELATIV zur Oberkante des
    // Menues. GEMESSEN: der Punkt fuer „Link in neuem Tab öffnen" lag bei
    // y = 207, das Menue bei y = 164 — der Klick traf „Kopieren", und der
    // Klickweg-Test meldete „kein neuer Tab". Die Oberkante kommt jetzt aus
    // dem Rechteck des Menues (das auch nach oben klappen kann).
    let r = m.rechteck(fb, fh)?;
    let y = r.y + m.zeile_mitte_y(nr, fh);
    Some(Point::new(r.x as f32 + 40.0, y as f32))
}

/// EINE Menuezeile **per Beschriftung** anklicken — `false`, wenn es sie nicht
/// gibt (das ist der Unterschied zwischen „die Zeile fehlt“ und „der Klick
/// lief daneben“).
pub fn klick_auf_zeile(s: &mut Sitzung, beschriftung: &str) -> bool {
    match zeilenpunkt(s, beschriftung) {
        Some(p) => linksklick(s, p.x, p.y),
        None => false,
    }
}

/// DER **WEG** in einem Satz: Rechtsklick, dann die Zeile mit dieser
/// Beschriftung anklicken. `true`, wenn **beides** passiert ist.
pub fn menue_weg(s: &mut Sitzung, x: f32, y: f32, beschriftung: &str) -> bool {
    rechtsklick(s, x, y);
    klick_auf_zeile(s, beschriftung)
}