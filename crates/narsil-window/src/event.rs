//! Eingaben der Praesentationsschicht und ihr Entprellen.
//!
//! Bei einem Fenster-Drag kommen Dutzende Resize-Events pro Sekunde.
//! Der [`RedrawScheduler`] sammelt sie und meldet, ob neu gerendert
//! werden muss — das eigentliche Rendern passiert erst nach dem letzten
//! Ereignis.

use narsil_core::geometry::{Point, Size};
use narsil_input::KeyEvent;

/// Eingaben, die die Praesentationsschicht dem Aussenleben meldet.
///
/// Positionen sind LOGISCHE Pixel im Fenster (Fensterursprung oben links),
/// keine Bildschirmkoordinaten: Gemeint ist "an dieser Stelle im Dokument",
/// der einzige Raum, in dem die Trefferpruefung etwas damit anfangen kann.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceEvent {
    /// Neu zeichnen noetig.
    Redraw,
    /// Fenstergroesse hat sich geaendert (logische Pixel).
    Resized(Size),
    /// Fenster wurde sichtbar.
    Shown,
    /// Fenster soll geschlossen werden.
    CloseRequested,
    /// Zeiger bewegt (logische Pixel im Fenster).
    PointerMoved(Point),
    /// Maustaste gedrueckt (logische Pixel im Fenster).
    ButtonPressed {
        /// Wo im Fenster.
        point: Point,
        /// Welche Taste.
        button: MouseButton,
    },
    /// Maustaste losgelassen (logische Pixel im Fenster).
    ///
    /// Ohne Loslassen kein Ziehen, kein Markieren — und die
    /// Trefferpruefung wuesste nicht, ob ein Klick noch aktiv ist.
    ButtonReleased {
        /// Wo im Fenster.
        point: Point,
        /// Welche Taste.
        button: MouseButton,
    },
    /// Mausrad gedreht (logische Pixel im Fenster).
    ///
    /// X11 liefert das Rad als Tasten 4/5/6/7; hier steht es als eigene
    /// Art, damit niemand Magic Numbers vergleicht (`Unknown` heisst
    /// "unbekannt" — das Rad ist bekannt).
    Wheel {
        /// Wo im Fenster.
        point: Point,
        /// Wie weit, wohin.
        delta: ScrollDelta,
    },
    /// Taste gedrueckt oder losgelassen.
    ///
    /// Das [`KeyEvent`] enthaelt Taste, Modifier-Zustand und ob
    /// gedrueckt/losgelassen. Damit muss der Verbraucher keinen Zustand
    /// aufbauen — das Ereignis traegt alles, was er braucht.
    Key(KeyEvent),
}

/// Scrollweg. Vorzeichen traegt die Richtung: negativ = hoch/links,
/// positiv = runter/rechts (Offset waechst nach unten, STUFE 2).
/// Die horizontale Achse faellt derzeit auf dieselbe Skala (links -1,
/// rechts +1); STUFE 2 nutzt vertikal, eine eigene Achse ist Folgearbeit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Radklicks in Zeilen (Radtasten 4/5/6/7: je +-1.0).
    Lines(f32),
    /// Feine Wege in Pixeln (Praezision/Trackpad — spaeter, reserviert).
    Pixels(f32),
}

/// Maustaste. Rad (X11-Buttons 4/5) kommt als `Unknown` an — kein Scrollen
/// im MVP, aber auch kein stilles Verschlucken: Die Trefferpruefung
/// ignoriert alles ausser `Left` ausdruecklich statt zufaellig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Linke Taste (X11-Button 1).
    Left,
    /// Mittlere Taste (X11-Button 2).
    Middle,
    /// Rechte Taste (X11-Button 3).
    Right,
    /// Andere Taste (Nummer wie vom Server gemeldet, z. B. Rad 4/5).
    Unknown(u8),
}

/// Zustandsautomat, der Resize- und Redraw-Ereignisse entprellt.
///
/// Zweck: Bei einem Fenster-Drag kommen Dutzende Resize-Events pro
/// Sekunde. Neu rendern soll aber erst nach dem letzten erfolgen.
///
/// Verhalten:
/// * `handle(Redraw)` setzt `dirty` und merkt das Ereignis.
/// * `handle(Resized(s))` merkt sich `s` als `window_size()`, setzt
///   `dirty` und merkt das Ereignis.
/// * `handle(Shown)` merkt das Ereignis und setzt `dirty` (ein gerade
///   sichtbar gewordenes Fenster muss gezeichnet werden).
/// * `handle(CloseRequested)` setzt `closed`, loescht `dirty` und merkt
///   das Ereignis. Danach verwirft `handle` jedes weitere Ereignis
///   (inklusive `Redraw`).
/// * `handle(PointerMoved(..))` und `handle(ButtonPressed{..})` merken das
///   Ereignis, aendern aber NICHTS sonst: kein `dirty` (Position allein ist
///   kein Zeichengrund), keine Groesse. Die Trefferpruefung liest aus
///   `drain()`, nicht aus Flags.
/// * `drain()` gibt alle gemerkten Ereignisse zurueck, leert die
///   Warteschlange und setzt `needs_redraw()` auf `false` zurueck:
///   entnommene Ereignisse gelten als verarbeitet. `is_closed()` und
///   `window_size()` bleiben erhalten.
/// * `window_size()` ist die letzte per `Resized` gemeldete Groesse,
///   anfangs `Size::ZERO`.
/// * Ungueltige Groessen (NaN, negativ) werden ohne Panic gespeichert;
///   die Pruefung ist Aufgabe der `ViewportPolicy`, nicht des Schedulers.
#[derive(Debug, Clone, PartialEq)]
pub struct RedrawScheduler {
    pending: Vec<SurfaceEvent>,
    dirty: bool,
    closed: bool,
    window_size: Size,
}

impl RedrawScheduler {
    /// Neuer Scheduler: nichts schmutzig, nichts geschlossen, keine Groesse.
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            dirty: false,
            closed: false,
            window_size: Size::ZERO,
        }
    }

    /// Nimmt ein Ereignis entgegen.
    ///
    /// Nach `CloseRequested` werden alle weiteren Ereignisse verworfen.
    /// Panict nie, auch nicht bei NaN-Groessen in `Resized`.
    pub fn handle(&mut self, event: SurfaceEvent) {
        if self.closed {
            return;
        }
        match event {
            SurfaceEvent::Redraw => {
                self.pending.push(event);
                self.dirty = true;
            }
            SurfaceEvent::Resized(size) => {
                self.window_size = size;
                self.pending.push(event);
                self.dirty = true;
            }
            SurfaceEvent::Shown => {
                self.pending.push(event);
                self.dirty = true;
            }
            SurfaceEvent::CloseRequested => {
                self.pending.push(event);
                self.closed = true;
                self.dirty = false;
            }
            SurfaceEvent::PointerMoved(_)
            | SurfaceEvent::ButtonPressed { .. }
            | SurfaceEvent::ButtonReleased { .. }
            | SurfaceEvent::Wheel { .. }
            | SurfaceEvent::Key(_) => {
                self.pending.push(event);
            }
        }
    }

    /// Ob ein Neuzeichnen noetig ist.
    ///
    /// `false` nach `new()`, nach `drain()` und sobald `is_closed()`
    /// gilt — ein schliessendes Fenster wird nicht mehr gezeichnet.
    pub fn needs_redraw(&self) -> bool {
        self.dirty && !self.closed
    }

    /// Ob das Fenster geschlossen werden soll.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Letzte bekannte Fenstergroesse.
    ///
    /// Anfangs `Size::ZERO`, danach die letzte per
    /// `Resized(..)` gemeldete Groesse.
    pub fn window_size(&self) -> Size {
        self.window_size
    }

    /// Entnimmt alle Ereignisse und leert die Warteschlange.
    ///
    /// Setzt `needs_redraw()` auf `false` zurueck (entnommene Ereignisse
    /// gelten als verarbeitet). `is_closed()` und `window_size()` bleiben
    /// erhalten, damit der Aufrufer sie nach dem Drain noch lesen kann.
    pub fn drain(&mut self) -> Vec<SurfaceEvent> {
        let out = std::mem::take(&mut self.pending);
        self.dirty = false;
        out
    }
}

impl Default for RedrawScheduler {
    /// Wie `RedrawScheduler::new()`.
    fn default() -> Self {
        Self::new()
    }
}

/// Ob Druecken und Loslassen zusammengehoeren: gleiche Taste, gleiche
/// Stelle. Kein Zustand, keine Vermutung — die Trefferpruefung fragt das
/// pro Paar, statt Klicks zu raten. Verschiedene Stellen ergeben NIE ein
/// Paar (die Taste springt nicht).
pub fn is_click_pair(press: &SurfaceEvent, release: &SurfaceEvent) -> bool {
    match (press, release) {
        (
            SurfaceEvent::ButtonPressed {
                point: p1,
                button: b1,
            },
            SurfaceEvent::ButtonReleased {
                point: p2,
                button: b2,
            },
        ) => p1 == p2 && b1 == b2,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scheduler_is_clean() {
        let s = RedrawScheduler::new();
        assert!(!s.needs_redraw());
        assert!(!s.is_closed());
        assert_eq!(s.window_size(), Size::ZERO);
        assert!(RedrawScheduler::default() == s);
    }

    #[test]
    fn redraw_sets_dirty() {
        let mut s = RedrawScheduler::new();
        s.handle(SurfaceEvent::Redraw);
        assert!(s.needs_redraw());
    }

    #[test]
    fn resized_updates_size_and_dirty() {
        let mut s = RedrawScheduler::new();
        let size = Size::new(640.0, 480.0);
        s.handle(SurfaceEvent::Resized(size));
        assert_eq!(s.window_size(), size);
        assert!(s.needs_redraw());
    }

    #[test]
    fn close_discards_later_events() {
        let mut s = RedrawScheduler::new();
        s.handle(SurfaceEvent::Redraw);
        s.handle(SurfaceEvent::CloseRequested);
        assert!(s.is_closed());
        assert!(!s.needs_redraw());
        s.handle(SurfaceEvent::Redraw);
        assert!(!s.needs_redraw());
        assert!(s.is_closed());
    }

    #[test]
    fn drain_clears_queue_and_dirty_but_keeps_size_and_closed() {
        let mut s = RedrawScheduler::new();
        s.handle(SurfaceEvent::Resized(Size::new(100.0, 100.0)));
        let events = s.drain();
        assert_eq!(events.len(), 1);
        assert!(!s.needs_redraw());
        assert_eq!(s.window_size(), Size::new(100.0, 100.0));
        assert!(s.drain().is_empty());
    }

    #[test]
    fn mouse_events_queue_without_dirty() {
        let mut s = RedrawScheduler::new();
        s.handle(SurfaceEvent::PointerMoved(Point::new(10.0, 20.0)));
        s.handle(SurfaceEvent::ButtonPressed {
            point: Point::new(10.0, 20.0),
            button: MouseButton::Left,
        });
        assert!(!s.needs_redraw(), "Position allein ist kein Zeichengrund");
        assert!(!s.is_closed());
        let events = s.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            SurfaceEvent::PointerMoved(Point::new(10.0, 20.0))
        );
    }

    #[test]
    fn mouse_button_kinds_are_distinct() {
        assert_ne!(MouseButton::Left, MouseButton::Right);
        assert_eq!(MouseButton::Unknown(4), MouseButton::Unknown(4));
        assert_ne!(MouseButton::Unknown(4), MouseButton::Unknown(5));
    }

    #[test]
    fn wheel_and_release_queue_without_dirty() {
        let mut s = RedrawScheduler::new();
        s.handle(SurfaceEvent::Wheel {
            point: Point::new(1.0, 2.0),
            delta: ScrollDelta::Lines(-1.0),
        });
        s.handle(SurfaceEvent::ButtonReleased {
            point: Point::new(1.0, 2.0),
            button: MouseButton::Left,
        });
        assert!(!s.needs_redraw());
        assert_eq!(s.drain().len(), 2);
    }

    #[test]
    fn click_pair_needs_same_button_and_point() {
        let press = SurfaceEvent::ButtonPressed {
            point: Point::new(5.0, 5.0),
            button: MouseButton::Left,
        };
        let release_same = SurfaceEvent::ButtonReleased {
            point: Point::new(5.0, 5.0),
            button: MouseButton::Left,
        };
        let release_moved = SurfaceEvent::ButtonReleased {
            point: Point::new(9.0, 5.0),
            button: MouseButton::Left,
        };
        assert!(is_click_pair(&press, &release_same));
        assert!(!is_click_pair(&press, &release_moved));
        assert!(!is_click_pair(&release_same, &press));
    }
}
