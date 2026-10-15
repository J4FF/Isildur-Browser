//! Geometrische Grundtypen.
//!
//! # VORAUSSETZUNG: alle Felder sind endlich
//!
//! `Point` und `Rect` sind `f32` mit oeffentlichen Feldern. Nichts in
//! diesem Crate prueft auf `NaN` oder `inf`, und das ist Absicht: die
//! Funktionen hier liegen in der Layout-Schleife, ein Test je Vergleich
//! kostet dort mehr, als die Fehlerklasse es wert ist.
//!
//! Die Zusage kommt von einer Stelle: [`length::Length::is_valid`] weist
//! `NaN`, `inf`, negative Pixel und Prozentwerte ueber 100 ab, und
//! [`length::Length::resolve`] gibt bei ungueltiger Laenge `None` zurueck.
//! Damit kann kein `NaN` in die Geometrie gelangen.
//!
//! Wer die Zusage braucht, ohne sie zu lesen: Agent4 hat sie in
//! TASK_0013A gemessen. `Rect::intersect` mit einem `NaN`-Rechteck
//! liefert `Some(Rect { x: NaN, .. })` statt `None`, weil mit `NaN`
//! jeder Vergleich `false` ist und die Leerbedingung `x1 <= x0` nicht
//! greift. Kein Panic — der Aufrufer rechnet mit `NaN` weiter. In
//! `narsil_raster::Framebuffer::fill_rect` endet das in
//! `NaN as u32 == 0`, also in einem still leer gezeichneten Rechteck.
//!
//! Sollte doch einmal ein `NaN` auftauchen, ist die richtige Antwort
//! NICHT, `intersect` zu verdrahten, sondern die Quelle zu finden, die
//! die Voraussetzung verletzt. Eine geaenderte Signatur an sechs
//! Aufrufstellen zu reparieren, ohne die Ursache zu kennen, macht das
//! Symptom unsichtbar und nicht unsichtbarer.

/// Punkt in logischen Pixeln.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const fn translate(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

/// Rechteckige Flaeche.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// `true`, wenn die Flaeche keine positive Ausdehnung hat.
    pub const fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Kleinste `Size`, die beide umschliesst.
    pub const fn union(self, other: Size) -> Size {
        Size::new(
            if self.width > other.width {
                self.width
            } else {
                other.width
            },
            if self.height > other.height {
                self.height
            } else {
                other.height
            },
        )
    }
}

/// Achsenausgerichtete Randabstaende in Pixeln.
///
/// Reihenfolge der Felder: links, oben, rechts, unten.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Edges {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Edges {
    pub const ZERO: Edges = Edges {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn all(v: f32) -> Self {
        Self::new(v, v, v, v)
    }
}

/// Rechteck, halb-offen: x in `[x, x + width)`, y entsprechend.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// **KOMPRESS 04.10.2026: RECHTE KANTE — additiv, `narsil_core::Rect`
    /// stand sie bisher nicht.**
    ///
    /// **GEMESSENER ANLASS, und er ist ein Build-Blocker:** die Fremd-Datei
    /// `crates/narsil-chrome/src/audio_leiste.rs` (die **andere** Rolle, 14:52)
    /// schreibt `spur.rechts()` und `r.unten()` auf einen `narsil_core::Rect`,
    /// und GEMESSEN waren **drei** Fehler `no method named 'rechts'` /
    /// `'unten'` (Zeilen **292**, **301**, **410**) — **wobei** Zeile **410**
    /// die Fehlermeldung wörtlich nennt: `no method named 'rechts' found for
    /// struct 'narsil_core::Rect'`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum HIER und nicht in `audio_leiste.rs`:**
    /// `narsil_chrome::Rechteck` (u32) **hat** `rechts()`/`unten()` bereits
    /// (`narsil-chrome/src/lib.rs:662`/`:667`) — **GEMESSEN**. Aber
    /// `audio_leiste.rs` rechnet in **f32** (`Rect::new(0 as f32, y as f32,
    /// ...)`) und braucht `as f32`-Umrechnungen, wenn es `u32` bekommt. **Der
    /// kleinere Eingriff ist also der in `f32`**, und `f32` ist hier der
    /// **natuerliche** Typ: GEMESSEN sind `Rect::new(x, y, width, height)`
    /// **vier** `f32`.
    ///
    /// **UND** es ist **rein additiv**: zwei neue `const fn`, **keine**
    /// bestehende Zeile geaendert. **GEMESSENE REGEL: eine fehlende
    /// Hilfsmethode gehoert an den Typ, der sie berechnen kann — nicht an
    /// die Datei, die sie braucht.** Sonst schreibt die naechste Datei
    /// dieselbe Rechnung ein drittes Mal.
    ///
    /// **UND** es gibt **schon** ein Gegenstueck: GEMESSEN in
    /// `audio_leiste.rs` die Funktion `enthaelt(r: &Rect, p: &Point) -> bool`,
    /// die **von Hand** „liegt der Punkt im Rechteck" baut, mit dem
    /// ausdruecklichen Kommentar, das Projekt pruefe Punkte **ueberall** so.
    /// **Also** gehoert auch `enthaelt` hierhin — und das ist der **zweite**
    /// Fund: eine **fremde** Datei umgeht eine **fehlende** Methode.
    #[must_use]
    pub const fn rechts(&self) -> f32 {
        self.x + self.width
    }

    /// **KOMPRESS 04.10.2026: UNTERE KANTE — siehe `rechts`.**
    #[must_use]
    pub const fn unten(&self) -> f32 {
        self.y + self.height
    }

    /// **KOMPRESS 04.10.2026: liegt der Punkt drin?**
    ///
    /// **GEMESSENER Grund fuer `fn` und nicht `const fn`:** der Vergleich
    /// braucht keinen Rechenschritt, aber `f32`-Vergleiche sind in `const fn`
    /// erlaubt — **also `const fn`.** GEMESSEN ist aber, dass `audio_leiste.rs`
    /// **das von Hand** baut (Zeile 300, Kommentar „das Projekt prueft Punkte
    /// ueberall so"), und **das** ist eine **zweite** Wahrheit ueber dieselbe
    /// Frage. **REGEL: wer „von Hand" schreibt, dass es bequemer ist, hat
    /// meistens die Hilfsmethode **nicht gesucht**.**
    #[must_use]
    pub const fn enthaelt(&self, p: &Point) -> bool {
        p.x >= self.x && p.x < self.rechts() && p.y >= self.y && p.y < self.unten()
    }

    /// Rechteck ab (0, 0) mit der given Groesse.
    pub const fn from_size(size: Size) -> Self {
        Self::new(0.0, 0.0, size.width, size.height)
    }

    /// Rechteck am given Ursprung mit der given Groesse.
    pub const fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.width, size.height)
    }

    pub const fn left(&self) -> f32 {
        self.x
    }

    pub const fn top(&self) -> f32 {
        self.y
    }

    /// Rechte Kante, `x + width`. Halb-offen: der Rand selbst gehoert
    /// NICHT mehr zum Rechteck, ist aber Teil dieser Zahl.
    ///
    /// NICHT begrenzt. Die Felder sind `f32`, nicht `i32`, also gibt es
    /// hier keinen Ueberlauf, sondern stille Ungenauigkeit:
    /// `Rect::new(3e9, 0.0, 4e9, 1.0).right()` ist `7e9`, und jeder
    /// Cast nach `i32` schneidet danach kommentarlos ab. Gemessen in
    /// TASK_0013A, Agent4, Befund 4. Wer die Kante in ganzzahligen
    /// Pixeln braucht, muss selbst runden und selbst pruefen.
    pub const fn right(&self) -> f32 {
        self.x + self.width
    }

    /// Untere Kante, `y + height`. Siehe [`Rect::right`] zur Grenze.
    pub const fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub const fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub const fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// `true`, wenn Breite ODER Hoehe null oder negativ ist.
    ///
    /// Ein Rechteck der Breite 0 gilt als LEER, nicht als "schmaler
    /// Streifen". Das ist eine Entscheidung, keine Nebensache: `area()`
    /// gibt 0 zurueck, `union()` gibt `None`, `intersect()` gibt `None`.
    /// Wer eine 0-Breite als sichtbar annimmt, bekommt an drei Stellen
    /// das Gegenteil. Gefunden in TASK_0013A, Agent4, Befund 1; vorher
    /// stand hier nur "keine positive Ausdehnung", was beides zulässt.
    pub const fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Flaeche in Quadratpixeln; 0 fuer leere Rechtecke.
    ///
    /// KANN `inf` WERDEN. `Rect::new(0.0, 0.0, f32::MAX, f32::MAX).area()`
    /// ist `inf`, kein Fehler, kein Panic. Bounding-Boxen, die per
    /// `union` entstehen, erben das. Gemessen in TASK_0013A, Agent4,
    /// Befund 3. Wer `area()` als Pruefgroesse benutzt, um "passt das
    /// noch in den Viewport" zu entscheiden, bekommt bei `inf` die
    /// Antwort "ja".
    pub const fn area(&self) -> f32 {
        if self.is_empty() {
            0.0
        } else {
            self.width * self.height
        }
    }

    /// Halb-offene Enthaltenheit: Rand rechts/unten gehoert nicht mehr dazu.
    pub const fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    /// Schnittmenge zweier Rechtecke, `None` wenn disjunkt.
    pub const fn intersect(&self, other: Rect) -> Option<Rect> {
        let x0 = if self.x > other.x { self.x } else { other.x };
        let y0 = if self.y > other.y { self.y } else { other.y };
        let x1 = if self.right() < other.right() {
            self.right()
        } else {
            other.right()
        };
        let y1 = if self.bottom() < other.bottom() {
            self.bottom()
        } else {
            other.bottom()
        };
        if x1 <= x0 || y1 <= y0 {
            None
        } else {
            Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
        }
    }

    /// Verschiebung.
    pub const fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Verkleinert das Rechteck an allen vier Seiten.
    /// Ergebnis wird auf >= 0 begrenzt.
    pub fn inset(&self, edges: Edges) -> Rect {
        let width = self.width - edges.left - edges.right;
        let height = self.height - edges.top - edges.bottom;
        Rect::new(
            self.x + edges.left,
            self.y + edges.top,
            if width < 0.0 { 0.0 } else { width },
            if height < 0.0 { 0.0 } else { height },
        )
    }

    /// Verschmilzt zwei Rechtecke zu ihrer Bounding-Box.
    /// `None`, wenn beide leer sind.
    pub fn union(&self, other: Rect) -> Option<Rect> {
        if self.is_empty() && other.is_empty() {
            return None;
        }
        if self.is_empty() {
            return Some(other);
        }
        if other.is_empty() {
            return Some(*self);
        }

        let x0 = if self.x < other.x { self.x } else { other.x };
        let y0 = if self.y < other.y { self.y } else { other.y };
        let x1 = if self.right() > other.right() {
            self.right()
        } else {
            other.right()
        };
        let y1 = if self.bottom() > other.bottom() {
            self.bottom()
        } else {
            other.bottom()
        };
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_edges_are_half_open() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.left(), 10.0);
        assert_eq!(r.right(), 40.0);
        assert_eq!(r.top(), 20.0);
        assert_eq!(r.bottom(), 60.0);
        assert!(r.contains(Point::new(10.0, 20.0)));
        assert!(!r.contains(Point::new(40.0, 60.0)));
    }

    #[test]
    fn rect_area_and_emptiness() {
        assert_eq!(Rect::new(0.0, 0.0, 4.0, 5.0).area(), 20.0);
        assert_eq!(Rect::new(0.0, 0.0, 0.0, 5.0).area(), 0.0);
        assert!(Rect::new(0.0, 0.0, -1.0, 5.0).is_empty());
    }

    #[test]
    fn intersect_overlapping() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(b), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
    }

    #[test]
    fn intersect_disjoint_and_touching() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(a.intersect(Rect::new(20.0, 20.0, 5.0, 5.0)), None);
        // Halb-offen: Beruehrung am Rand ist keine Ueberlappung.
        assert_eq!(a.intersect(Rect::new(10.0, 0.0, 5.0, 5.0)), None);
    }

    #[test]
    fn inset_clamps_to_zero() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(r.inset(Edges::all(2.0)), Rect::new(2.0, 2.0, 6.0, 6.0));
        assert_eq!(r.inset(Edges::all(8.0)), Rect::new(8.0, 8.0, 0.0, 0.0));
    }

    #[test]
    fn union_of_rects() {
        let a = Rect::new(0.0, 0.0, 2.0, 2.0);
        let b = Rect::new(4.0, 4.0, 2.0, 2.0);
        assert_eq!(a.union(b), Some(Rect::new(0.0, 0.0, 6.0, 6.0)));
        assert_eq!(Rect::ZERO.union(a), Some(a));
        assert_eq!(Rect::ZERO.union(Rect::ZERO), None);
    }

    #[test]
    fn size_helpers() {
        assert!(Size::new(0.0, 10.0).is_empty());
        assert_eq!(
            Size::new(1.0, 2.0).union(Size::new(3.0, 0.5)),
            Size::new(3.0, 2.0)
        );
    }

    #[test]
    fn translate_moves_origin_only() {
        let r = Rect::new(1.0, 2.0, 3.0, 4.0).translate(10.0, -1.0);
        assert_eq!(r, Rect::new(11.0, 1.0, 3.0, 4.0));
    }
}
