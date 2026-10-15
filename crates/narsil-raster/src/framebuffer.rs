//! RGBA8-Framebuffer mit Software-Rasterisierung.
//!
//! Der Framebuffer speichert Pixel zeilenweise von oben nach unten,
//! 4 Byte pro Pixel: R, G, B, A.

use narsil_core::color::Color;
use narsil_core::geometry::Rect;

/// RGBA8-Framebuffer, zeilenweise von oben nach unten,
/// 4 Byte pro Pixel: R, G, B, A.
pub struct Framebuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    /// Clipping-Rechteck-Stack. Effektiver Clipp = Schnittmenge aller
    /// offenen Rechtecke. Wird bei push_clip hinzugefügt, bei pop_clip
    /// entfernt. Leerer Stack bedeutet keinen Clipp (alles darf).
    clip_stack: Vec<Rect>,
}

impl Framebuffer {
    /// Neuer Framebuffer mit Fuellfarbe.
    ///
    /// Bei width=0 oder height=0 wird ein leerer Framebuffer erzeugt.
    /// Fuellt zeilenweise (nicht pixelweise) für bessere Performance.
    pub fn new(width: u32, height: u32, fill: Color) -> Framebuffer {
        let len = (width as usize) * (height as usize) * 4;
        // Release 15.10.2026 (Agent F): a transparent buffer is zeroed
        // memory, which the allocator hands out without writing it (an
        // isolated `filter` group takes one per group and touches only its
        // area).
        if [fill.r, fill.g, fill.b, fill.a] == [0, 0, 0, 0] {
            return Framebuffer { width, height, pixels: vec![0u8; len], clip_stack: Vec::new() };
        }
        let mut pixels = Vec::with_capacity(len);

        // Fuellfarbe in Bytes umwandeln
        let fill_bytes = [fill.r, fill.g, fill.b, fill.a];

        // Eine Zeile vorbereiten (width Pixel)
        let row_len = width as usize * 4;
        let mut row = Vec::with_capacity(row_len);
        for _ in 0..(width as usize) {
            row.extend_from_slice(&fill_bytes);
        }

        // Zeilenweise fuellen
        for _ in 0..(height as usize) {
            pixels.extend_from_slice(&row);
        }

        Framebuffer {
            width,
            height,
            pixels,
            clip_stack: Vec::new(),
        }
    }

    /// Schiebt ein Clipping-Rechteck auf den Stack.
    /// Der effektive Clipp ist die Schnittmenge aller offenen Rechtecke.
    pub fn push_clip(&mut self, rect: Rect) {
        if self.clip_stack.is_empty() {
            self.clip_stack.push(rect);
        } else {
            // Schnittmenge mit bestehenden Clipp hinzufügen
            let last = self.clip_stack.last().unwrap();
            let x1 = last.x.max(rect.x);
            let y1 = last.y.max(rect.y);
            let x2 = last.right().min(rect.x + rect.width).min(self.width as f32);
            let y2 = last.bottom().min(rect.y + rect.height).min(self.height as f32);
            if x1 < x2 && y1 < y2 {
                self.clip_stack.push(Rect::new(x1, y1, x2 - x1, y2 - y1));
            } else {
                // Kein Überschneidungsbereich — kompletter Clipp
                self.clip_stack.push(Rect::new(0.0, 0.0, 0.0, 0.0));
            }
        }
    }

    /// Der EFFEKTIVE Clipp: die Schnittmenge aller offenen Rechtecke,
    /// zusaetzlich auf den Framebuffer beschraenkt.
    ///
    /// # WARUM DAS OEFFENTLICH IST
    ///
    /// GEMESSEN 03.10.2026: `kasten_mit_radius` und `schatten_zeichnen`
    /// setzen Pixel mit `set_pixel`, und `set_pixel` prueft nur die
    /// Framebuffer-Grenzen — NICHT den Clip-Stack. Ein Rechteck, das ganz
    /// ausserhalb des angerasterten Bereichs liegt, wurde darum doch
    /// gezeichnet: vier Tests in `src/rasterizer.rs` schlugen fehl
    /// (`rasterize_bereich_ausserhalb_gibt_null` und drei weitere,
    /// GEMESSEN 1 statt 0).
    ///
    /// Zwei Wege sind offen: hier den Clipp abfragen, oder in
    /// `kasten_mit_radius` das Rechteck vorher schneiden. Der erste Weg ist
    /// der richtige, weil er an EINER Stelle liegt — jede neue Zeichen-
    /// funktion, die direkt in den Framebuffer schreibt, muss sich dann
    /// nicht selbst erinnern.
    pub fn effektiver_clip(&self) -> Rect {
        let fb = Rect::new(0.0, 0.0, self.width as f32, self.height as f32);
        let Some(&erste) = self.clip_stack.first() else {
            return fb;
        };
        let mut clip = erste;
        for &c in self.clip_stack.iter().skip(1) {
            let x0 = clip.x.max(c.x).max(0.0);
            let y0 = clip.y.max(c.y).max(0.0);
            let x1 = clip.right().min(c.right()).min(self.width as f32);
            let y1 = clip.bottom().min(c.bottom()).min(self.height as f32);
            clip = Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0));
        }
        Rect::new(
            clip.x.max(fb.x),
            clip.y.max(fb.y),
            clip.right().min(fb.right()) - clip.x.max(fb.x),
            clip.bottom().min(fb.bottom()) - clip.y.max(fb.y),
        )
    }

    /// Entfernt das letzte Clipping-Rechteck vom Stack.
    pub fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    /// Gibt die aktuelle Anzahl der Clips auf dem Stack zurueck.
    /// Fuer `rasterize_bereich`, um den Stack vor/nachher zu vergleichen.
    pub(crate) fn clip_stack_len(&self) -> usize {
        self.clip_stack.len()
    }

    /// Entfernt Clips vom Stack bis zur gegebenen Laenge.
    /// Fuer `rasterize_bereich`, um den vorherigen Stand wiederherzustellen.
    pub(crate) fn truncate_clip_stack(&mut self, len: usize) {
        self.clip_stack.truncate(len);
    }

    /// Fuellt ein Rechteck mit einer OPAKEN Farbe, ohne Blending.
    ///
    /// Fuer den haeufigsten Fall im ganzen Browser: Hintergrund,
    /// Leiste, Knopfflaeche. Zeilenweise, nicht pixelweise.
    ///
    /// Rueckgabe: Anzahl tatsaechlich geaenderter Pixel.
    pub fn clear_rect(&mut self, rect: Rect, color: Color) -> usize {
        debug_assert!(color.is_opaque(), "clear_rect erwartet opake Farbe (a=255)");

        // Leeres Rechteck: nichts zu tun
        if rect.is_empty() {
            return 0;
        }

        // Effektiven Clipp berechnen (wie bei fill_rect)
        let effective_clip = if self.clip_stack.is_empty() {
            Rect::new(0.0, 0.0, self.width as f32, self.height as f32)
        } else {
            let mut clip = self.clip_stack[0];
            for i in 1..self.clip_stack.len() {
                let c = &self.clip_stack[i];
                let x0 = clip.x.max(c.x);
                let y0 = clip.y.max(c.y);
                let x1 = clip.right().min(c.right());
                let y1 = clip.bottom().min(c.bottom());
                clip = Rect::new(x0, y0, x1 - x0, y1 - y0);
            }
            let fb = Rect::new(0.0, 0.0, self.width as f32, self.height as f32);
            let x0 = clip.x.max(fb.x);
            let y0 = clip.y.max(fb.y);
            let x1 = clip.right().min(fb.right());
            let y1 = clip.bottom().min(fb.bottom());
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        };

        if effective_clip.width <= 0.0 || effective_clip.height <= 0.0 {
            return 0;
        }

        // Clipping: Schnitt mit effektivem Clipp
        let clipped = match rect.intersect(effective_clip) {
            Some(r) => r,
            None => return 0,
        };

        // Halb-offene Rechtecke: x in [x, x+width)
        let x_start = clipped.x.max(0.0) as u32;
        let y_start = clipped.y.max(0.0) as u32;
        let x_end = (clipped.x + clipped.width).min(self.width as f32).max(0.0) as u32;
        let y_end = (clipped.y + clipped.height)
            .min(self.height as f32)
            .max(0.0) as u32;

        if x_start >= x_end || y_start >= y_end {
            return 0;
        }

        let width = x_end - x_start;
        let stride = self.width as usize * 4;
        let color_bytes = [color.r, color.g, color.b, color.a];

        // Zeilenweise fuellen
        let mut changed = 0usize;
        for y in y_start..y_end {
            let row_start = (y as usize) * stride + (x_start as usize) * 4;
            let row_end = row_start + (width as usize) * 4;
            for idx in (row_start..row_end).step_by(4) {
                self.pixels[idx] = color_bytes[0];
                self.pixels[idx + 1] = color_bytes[1];
                self.pixels[idx + 2] = color_bytes[2];
                self.pixels[idx + 3] = color_bytes[3];
            }
            changed += width as usize;
        }

        changed
    }

    /// Wie fill_rect, aber der Aufrufer WEISS, dass die Farbe deckend
    /// ist. Muss dieselben Pixel schreiben wie fill_rect mit a=255.
    ///
    /// Rueckgabe: Anzahl tatsaechlich geaenderter Pixel.
    pub fn fill_rect_opak(&mut self, rect: Rect, color: Color) -> usize {
        // Delegiert an clear_rect, da beide den opaken Pfad nehmen
        self.clear_rect(rect, color)
    }

    /// Breite des Framebuffers in Pixeln.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Hoehe des Framebuffers in Pixeln.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Rohbytes, Laenge = width * height * 4.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Pixel lesen, None wenn ausserhalb.
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some(Color::new(
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ))
    }

    /// Release 08.10.2026: eine ZEILENSPANNE `[x0, x1)` in Zeile `y` fuellen —
    /// deckend als Kopie, sonst je Pixel `blend_source_over` (dieselbe
    /// Rechnung wie `set_pixel` nach `pixel`). Die Spanne wird an Puffer und
    /// Clip geklemmt. Rueckgabe: Anzahl geschriebener Pixel.
    ///
    /// GEMESSEN vorher: `kasten_mit_radius` setzte jedes Pixel einzeln
    /// (Ecken-Test, Clip-Test, Grenzen-Test je Pixel) — der Raster brauchte
    /// 20–33 ms je Bild fuer ~200 Eintraege bei 1888 x 954.
    /// Release 10.10.2026: der wirksame Zeichenbereich `[x0, x1) x [y0, y1)` —
    /// Puffer geschnitten mit dem obersten Clip (fuer Zeilen-Schleifen, die
    /// nicht je Pixel pruefen).
    pub(crate) fn zeichenbereich(&self) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (0u32, 0u32, self.width, self.height);
        if let Some(clip) = self.clip_stack.last() {
            if clip.width <= 0.0 || clip.height <= 0.0 {
                return (0, 0, 0, 0);
            }
            x0 = x0.max(clip.x.floor().max(0.0) as u32);
            y0 = y0.max(clip.y.floor().max(0.0) as u32);
            x1 = x1.min(clip.right().ceil().max(0.0) as u32);
            y1 = y1.min(clip.bottom().ceil().max(0.0) as u32);
        }
        (x0, y0, x1, y1)
    }

    /// Release 10.10.2026: die Bytes einer Zeile (RGBA, `breite * 4`).
    pub(crate) fn zeile_mut(&mut self, y: u32) -> &mut [u8] {
        let w = self.width as usize * 4;
        let a = y as usize * w;
        &mut self.pixels[a..a + w]
    }

    pub(crate) fn spanne_fuellen(&mut self, y: u32, x0: u32, x1: u32, c: Color, mischen: bool) -> u32 {
        if y >= self.height {
            return 0;
        }
        let (mut a, mut b) = (x0, x1.min(self.width));
        if let Some(clip) = self.clip_stack.last() {
            if clip.width <= 0.0
                || clip.height <= 0.0
                || (y as f32) < clip.y.floor()
                || (y as f32) >= clip.bottom().ceil()
            {
                return 0;
            }
            let cx0 = clip.x.floor().max(0.0) as u32;
            let cx1 = clip.right().ceil().max(0.0) as u32;
            a = a.max(cx0);
            b = b.min(cx1);
        }
        if b <= a {
            return 0;
        }
        let zeile = (y as usize) * (self.width as usize) * 4;
        let teil = &mut self.pixels[zeile + a as usize * 4..zeile + b as usize * 4];
        if mischen {
            for px in teil.chunks_exact_mut(4) {
                let alt = Color::new(px[0], px[1], px[2], px[3]);
                let neu = blend_source_over(c, alt);
                px.copy_from_slice(&[neu.r, neu.g, neu.b, neu.a]);
            }
        } else {
            let muster = [c.r, c.g, c.b, c.a];
            for px in teil.chunks_exact_mut(4) {
                px.copy_from_slice(&muster);
            }
        }
        b - a
    }

    /// Pixel setzen, false wenn ausserhalb (kein Panic).
    pub fn set_pixel(&mut self, x: u32, y: u32, c: Color) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }

        // push_clip stores the intersection, so checking the top is sufficient.
        if let Some(clip) = self.clip_stack.last() {
            if clip.width <= 0.0 || clip.height <= 0.0
                || (x as f32) < clip.x.floor() || (x as f32) >= clip.right().ceil()
                || (y as f32) < clip.y.floor() || (y as f32) >= clip.bottom().ceil()
            {
                return false;
            }
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.pixels[idx] = c.r;
        self.pixels[idx + 1] = c.g;
        self.pixels[idx + 2] = c.b;
        self.pixels[idx + 3] = c.a;
        true
    }

    /// Fuellt ein Rechteck mit source-over Alpha-Blending,
    /// mit Clipping auf die Framebuffer-Grenzen.
    ///
    /// Rueckgabe: Anzahl tatsaechlich geaenderter Pixel.
    ///
    /// Alpha-Blending (source-over):
    /// ```text
    /// a_out = a_src + a_dst * (1 - a_src)
    /// c_out = (c_src * a_src + c_dst * a_dst * (1 - a_src)) / a_out
    /// ```
    ///
    /// Bei a_src = 0 wird nichts geaendert.
    /// Bei a_src = 255 wird der opake Schnellweg genommen (kein Blending).
    pub fn fill_rect(&mut self, rect: Rect, color: Color) -> usize {
        // Leeres Rechteck oder transparente Farbe: nichts zu tun
        if rect.is_empty() || color.is_transparent() {
            return 0;
        }

        // Opaker Schnellweg: a=255 -> direkt ueberschreiben ohne Blending
        if color.is_opaque() {
            return self.clear_rect(rect, color);
        }

        // Effektiven Clipp berechnen: Schnittmenge aller Clipp-Rechtecke
        // und Schnitt mit Framebuffer-Grenzen
        let effective_clip = if self.clip_stack.is_empty() {
            Rect::new(0.0, 0.0, self.width as f32, self.height as f32)
        } else {
            // Schnittmenge aller Clipp-Rechtecke
            let mut clip = self.clip_stack[0];
            for i in 1..self.clip_stack.len() {
                let c = &self.clip_stack[i];
                let x0 = clip.x.max(c.x);
                let y0 = clip.y.max(c.y);
                let x1 = clip.right().min(c.right());
                let y1 = clip.bottom().min(c.bottom());
                clip = Rect::new(x0, y0, x1 - x0, y1 - y0);
            }
            // Schnitt mit Framebuffer-Grenzen
            let fb = Rect::new(0.0, 0.0, self.width as f32, self.height as f32);
            let x0 = clip.x.max(fb.x);
            let y0 = clip.y.max(fb.y);
            let x1 = clip.right().min(fb.right());
            let y1 = clip.bottom().min(fb.bottom());
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        };

        // Wenn der effektive Clipp leer ist, nichts zeichnen
        if effective_clip.width <= 0.0 || effective_clip.height <= 0.0 {
            return 0;
        }

        // Clipping: Schnitt mit effektivem Clipp
        let clipped = match rect.intersect(effective_clip) {
            Some(r) => r,
            None => return 0, // Komplett ausserhalb des Clipp
        };

        // Halb-offene Rechtecke: x in [x, x+width)
        // Wir runden auf, um die Grenzen korrekt zu behandeln
        let x_start = clipped.x.max(0.0) as u32;
        let y_start = clipped.y.max(0.0) as u32;
        let x_end = (clipped.x + clipped.width).min(self.width as f32).max(0.0) as u32;
        let y_end = (clipped.y + clipped.height)
            .min(self.height as f32)
            .max(0.0) as u32;

        let mut changed = 0usize;

        for y in y_start..y_end {
            for x in x_start..x_end {
                let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;

                // Aktuelle Ziel-Farbe lesen
                let dst = Color::new(
                    self.pixels[idx],
                    self.pixels[idx + 1],
                    self.pixels[idx + 2],
                    self.pixels[idx + 3],
                );

                // Alpha-Blending anwenden
                let blended = blend_source_over(color, dst);

                // Nur schreiben, wenn sich etwas aendert
                if blended != dst {
                    self.pixels[idx] = blended.r;
                    self.pixels[idx + 1] = blended.g;
                    self.pixels[idx + 2] = blended.b;
                    self.pixels[idx + 3] = blended.a;
                }
                changed += 1;
            }
        }

        changed
    }

    /// Kopiert einen Bildausschnitt aus einem ANDEREN Framebuffer an die
    /// Stelle `ziel`.
    ///
    /// Das ist die Grundlage fuer **teilweise neu zeichnen**
    /// (ADR-033/034): nur das Rechteck, das sich geaendert hat, wird neu
    /// gerastert und dann hier hereinkopiert, statt den ganzen Framebuffer
    /// neu zu fuellen.
    ///
    /// # Regeln
    ///
    /// * **Kein Clipping ueber den Clipp-Stack.** Der Aufrufer hat
    ///   entschieden, dass dieses Rechteck sichtbar ist. Wer hier noch
    ///   einmal clippt, verwechselt "sichtbar" mit "gerade gezeichnet".
    /// * **Alles ausserhalb** des Ziels wird geschnitten, nicht gerundet:
    ///   ein Blit auf x=5 mit Breite 6 in einen 8-Pixel-Puffer kopiert
    ///   die 3 Pixel, die passen, und meldet `3`.
    /// * **Quelle und Ziel duerfen sich nicht ueberlappen** — das ist per
    ///   Rust-Borrow nicht einmal ausdrueckbar (`&mut self` und `&self`
    ///   zugleich). Fuer den Ueberlappungsfall, also fuer Scrollen, gibt
    ///   es [`blit_selbst`](Self::blit_selbst).
    ///
    /// Rueckgabe: Anzahl tatsaechlich kopierter Pixel. Das ist eine Zahl
    /// und kein Fehler: ein Rechteck vollstaendig ausserhalb ergibt 0.
    pub fn blit(&mut self, quelle: &Framebuffer, quelle_rect: Rect, ziel: (u32, u32)) -> usize {
        let qw = laenge_als_u32(quelle_rect.width);
        let qh = laenge_als_u32(quelle_rect.height);
        if qw == 0 || qh == 0 {
            return 0;
        }
        // Negative oder nicht-endliche QUELLPOSITION: nichts. GEMESSEN
        // 29.09.2026: `laenge_als_u32(-5.0)` ist 0, und ohne diese Zeile
        // las der Code ab Spalte 0 weiter und kopierte 3 Pixel aus dem
        // FALSCHEN Bereich. Die Groesse war abgesichert, die Position
        // nicht — und die Behauptung "negativ ergibt 0 Pixel" galt fuer
        // die eine und nicht fuer die andere.
        if !ist_gueltige_position(quelle_rect.x) || !ist_gueltige_position(quelle_rect.y) {
            return 0;
        }
        let (zx, zy) = ziel;
        if zx >= self.width || zy >= self.height {
            return 0;
        }

        let mut kopiert = 0usize;
        for dy in 0..qh {
            let dy_dst = zy + dy;
            if dy_dst >= self.height {
                break;
            }
            let sy = laenge_als_u32(quelle_rect.y) + dy;
            if sy >= quelle.height {
                break;
            }
            // Nur den Ueberlappungsbereich dieser Zeile kopieren.
            let x_start = zx;
            let x_end = zx.saturating_add(qw).min(self.width);
            if x_start >= x_end {
                break;
            }
            // Quell-x: der Offset in der Zeile, der zum Ziel-x_start passt.
            let quell_x = laenge_als_u32(quelle_rect.x) + (x_start - zx);
            if quell_x >= quelle.width {
                break;
            }
            let anzahl = x_end - x_start;
            if anzahl == 0 {
                break;
            }

            for k in 0..anzahl {
                let qx = quell_x + k;
                if qx >= quelle.width {
                    break;
                }
                let src = ((sy as usize) * (quelle.width as usize) + (qx as usize)) * 4;
                let dst =
                    ((dy_dst as usize) * (self.width as usize) + ((x_start + k) as usize)) * 4;
                self.pixels[dst] = quelle.pixels[src];
                self.pixels[dst + 1] = quelle.pixels[src + 1];
                self.pixels[dst + 2] = quelle.pixels[src + 2];
                self.pixels[dst + 3] = quelle.pixels[src + 3];
            }
            kopiert += anzahl as usize;
        }
        kopiert
    }

    /// Kopiert einen Ausschnitt IN DIESEN PUFFER, in sich selbst hinein.
    ///
    /// Das ist der Scrollweg: der Ausschnitt wandert nach oben (oder links),
    /// und Quelle und Ziel sind derselbe Puffer. Genau hier scheitert ein
    /// Zeilen-fuer-Zeile-Kopierer, das von unten nach oben laeuft: Zeile 0
    /// ueberschreibt die Quellzeile, BEVOR sie gelesen wurde.
    ///
    /// # WARUM EINE TEMPORAERE KOPIE und nicht bloss die richtige Reihenfolge
    ///
    /// GEMESSEN 29.09.2026, am 29.09. gebaut: die erste Fassung dieses
    /// Codes kopierte direkt und "von oben nach unten" — was fuer
    /// vertikales Scrollen nach oben stimmt und fuer JEDE andere
    /// Ueberschneidung falsch ist. Bei horizontalem Scrollen nach links
    /// ueberschneiden sich Quelle und Ziel INNERHALB einer Zeile, und
    /// "von oben nach unten" sagt dazu nichts.
    ///
    /// Also nicht Disziplin, sondern Struktur: der Quellbereich wird
    /// einmal kopiert, und danach wird aus dieser Kopie gelesen. Damit ist
    /// das Ergebnis unabhaengig von der Kopierreihenfolge — und die
    /// Reihenfolge ist damit nicht mehr die Eigenschaft, die tragen muss.
    ///
    /// Kostet eine Pufferkopie. Bei 800x600 sind das 1,9 MiB, bei
    /// 1920x1080 rund 8 MiB — gegen einen Vorteil, der sich nicht
    /// nachweisen laesst, weil der Fehler nur in der Reihenfolge steckt.
    pub fn blit_selbst(&mut self, quelle_rect: Rect, ziel: (u32, u32)) -> usize {
        let qw = laenge_als_u32(quelle_rect.width);
        let qh = laenge_als_u32(quelle_rect.height);
        if qw == 0 || qh == 0 {
            return 0;
        }
        let (zx, zy) = ziel;
        if zx >= self.width || zy >= self.height {
            return 0;
        }

        // In eine temporaere Kopie, danach erst schreiben.
        if !ist_gueltige_position(quelle_rect.x) || !ist_gueltige_position(quelle_rect.y) {
            return 0;
        }
        let temp = self.crop(quelle_rect);
        match temp {
            Some(t) => self.blit(&t, Rect::new(0.0, 0.0, qw as f32, qh as f32), ziel),
            None => 0,
        }
    }

    /// Schneidet einen Bereich als neuen Framebuffer aus.
    ///
    /// `None`, wenn der Bereich nicht (auch nicht teilweise) im Puffer
    /// liegt. Es wird NICHTS gerundet: ein Rechteck, das halb draussen
    /// ist, ergibt `None` statt eines halben Bildes — wer schneiden
    /// will, gibt ein Rechteck an, das passt.
    pub fn crop(&self, rect: Rect) -> Option<Framebuffer> {
        let w = laenge_als_u32(rect.width);
        let h = laenge_als_u32(rect.height);
        let x = laenge_als_u32(rect.x);
        let y = laenge_als_u32(rect.y);
        if w == 0 || h == 0 {
            return None;
        }
        if x + w > self.width || y + h > self.height {
            return None;
        }
        let mut out = Framebuffer::new(w, h, Color::new(0, 0, 0, 255));
        for dy in 0..h {
            for dx in 0..w {
                let src = (((y + dy) as usize) * (self.width as usize) + ((x + dx) as usize)) * 4;
                let dst = ((dy as usize) * (w as usize) + (dx as usize)) * 4;
                out.pixels[dst] = self.pixels[src];
                out.pixels[dst + 1] = self.pixels[src + 1];
                out.pixels[dst + 2] = self.pixels[src + 2];
                out.pixels[dst + 3] = self.pixels[src + 3];
            }
        }
        Some(out)
    }
}

/// Laenge als `u32`, mit `0` statt Wrap-around bei ungueltigen Werten.
///
/// GEMESSEN, warum das nicht `as u32` sein kann: `-1.0_f32 as u32` ist in
/// Rust **4294967295**, nicht 0. Ein negatives Rechteck wuerde dann
/// 4 Milliarden Pixel beanspruchen und der naechste Indexzugriff laege
/// ausserhalb des Puffers. Diese Funktion ist die einzige Stelle, an der
/// aus einer Laenge eine Pixelzahl wird.
fn laenge_als_u32(w: f32) -> u32 {
    if w.is_finite() && w >= 0.0 && w < u32::MAX as f32 {
        w as u32
    } else {
        0
    }
}

/// `true`, wenn eine POSITION (nicht eine Laenge) verwendbar ist.
///
/// Getrennt von [`laenge_als_u32`], weil die beiden etwas anderes
/// beantworten und der Unterschied schon einen Fehler verursacht hat:
/// `laenge_als_u32(-5.0)` ist `0` — fuer eine Laenge richtig (0 Pixel sind
/// 0 Pixel), fuer eine Position falsch (Spalte -5 ist nicht Spalte 0).
/// Wer beides ueber eine Funktion abhandelt, bekommt bei der Position
/// stillschweigend ein anderes Rechteck.
fn ist_gueltige_position(p: f32) -> bool {
    p.is_finite() && p >= 0.0 && p < u32::MAX as f32
}

/// Source-over Alpha-Blending (Ganzzahl-Implementierung, kein f32).
///
/// Formel in ganzzahliger Arithmetik:
/// ```text
/// a_out_num = a_src * 255 + a_dst * (255 - a_src)        // Zähler vor Division
/// a_out = (a_out_num + 127) / 255                        // gerundetes Alpha
/// c_out = (c_src * a_src * 255 + c_dst * a_dst * (255 - a_src) + a_out_num/2) / a_out_num
/// ```
/// WICHTIG: Für die Farbkanäle wird durch den EXAKTEN Zähler a_out_num geteilt,
/// nicht durch das gerundete a_out * 255. Das ist der Schlüssel zur exakten
/// Entsprechung zur f32-Formel.
///
/// Bei a_out = 0 wird TRANSPARENT zurueckgegeben.
///
/// Diese Funktion ist die produktive Implementierung.
pub fn blend_source_over(src: Color, dst: Color) -> Color {
    // Sonderfaelle
    if src.is_transparent() {
        return dst;
    }
    if src.is_opaque() {
        return src;
    }
    if dst.is_transparent() {
        return src;
    }

    let a_src = src.a as u16;
    let a_dst = dst.a as u16;

    // Exakter Zähler für a_out (vor Division durch 255)
    // a_out = a_src + a_dst * (1 - a_src/255) = (a_src * 255 + a_dst * (255 - a_src)) / 255
    let a_out_num = (a_src as u32) * 255 + (a_dst as u32) * (255 - (a_src as u32));

    // Gerundetes Alpha für das Ergebnis
    let a_out = ((a_out_num + 127) / 255) as u8;

    if a_out == 0 {
        return Color::TRANSPARENT;
    }

    // Farbkanäle berechnen mit Division durch den EXAKTEN Zähler a_out_num
    // c_out = (c_src * a_src + c_dst * a_dst * (1 - a_src/255)) / a_out
    //     = (c_src * a_src * 255 + c_dst * a_dst * (255 - a_src)) / a_out_num
    //
    // GEMESSEN am 30.09.2026: hier stand `let half_a_out_num =
    // a_out_num / 2;` mit dem Kommentar "Mit Rundung: + a_out_num / 2".
    // Die Rundung macht aber `blend_channel_int`, und zwar
    // half-away-from-zero ueber `2 * rest >= a_out_num` — dieselbe
    // Entscheidung, nur ohne zweite Wahrheit daneben. Die Variable wurde
    // nie gelesen. Zwei Namen fuer eine Rundung sind zwei Orte, an
    // denen sie auseinanderlaufen koennen.
    let r = blend_channel_int(src.r as u16, dst.r as u16, a_src, a_dst, a_out_num);
    let g = blend_channel_int(src.g as u16, dst.g as u16, a_src, a_dst, a_out_num);
    let b = blend_channel_int(src.b as u16, dst.b as u16, a_src, a_dst, a_out_num);

    Color::new(r, g, b, a_out)
}

/// Hilfsfunktion fuer einen Farbkanal in ganzzahliger Arithmetik
/// mit Round-Half-Away-From-Zero wie f32::round().
#[inline]
fn blend_channel_int(c_src: u16, c_dst: u16, a_src: u16, a_dst: u16, a_out_num: u32) -> u8 {
    // Numerator: c_src * a_src * 255 + c_dst * a_dst * (255 - a_src)
    let term1 = (c_src as u32) * (a_src as u32) * 255;
    let term2 = (c_dst as u32) * (a_dst as u32) * (255 - (a_src as u32));
    let num = term1 + term2;

    // Exakte Division mit Round-Half-Away-From-Zero
    // f32::round() rundet 0.5 weg von 0 (nicht Banker's rounding!)
    // Da alle Werte >= 0 sind, bedeutet das: bei 2*r >= a_out_num aufrunden
    let q = num / a_out_num;
    let r = num % a_out_num;

    if r * 2 >= a_out_num {
        (q + 1) as u8
    } else {
        q as u8
    }
}

/// Source-over Alpha-Blending (alte f32-Fassung, nur fuer Tests).
///
/// Diese Fassung bleibt als Referenz erhalten, um die Ganzzahl-Implementierung
/// gegenueber der Originalformel zu validieren.
/// NICHT in Produktionscode verwenden.
#[cfg(test)]
pub fn blend_source_over_f32(src: Color, dst: Color) -> Color {
    // Sonderfaelle
    if src.is_transparent() {
        return dst;
    }
    if src.is_opaque() {
        return src;
    }
    if dst.is_transparent() {
        return src;
    }

    // Normalisierte Alpha-Werte (0.0 - 1.0)
    let a_src = src.a as f32 / 255.0;
    let a_dst = dst.a as f32 / 255.0;

    // Ergebnis-Alpha
    let a_out = a_src + a_dst * (1.0 - a_src);

    if a_out <= 0.0 {
        return Color::TRANSPARENT;
    }

    // Farbkanäle berechnen mit .round()
    let r = ((src.r as f32 * a_src + dst.r as f32 * a_dst * (1.0 - a_src)) / a_out).round() as u8;
    let g = ((src.g as f32 * a_src + dst.g as f32 * a_dst * (1.0 - a_src)) / a_out).round() as u8;
    let b = ((src.b as f32 * a_src + dst.b as f32 * a_dst * (1.0 - a_src)) / a_out).round() as u8;
    let a = (a_out * 255.0).round() as u8;

    Color::new(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framebuffer_new_with_zero_width() {
        let fb = Framebuffer::new(0, 10, Color::WHITE);
        assert_eq!(fb.width(), 0);
        assert_eq!(fb.height(), 10);
        assert_eq!(fb.pixels().len(), 0);
    }

    #[test]
    fn framebuffer_new_with_zero_height() {
        let fb = Framebuffer::new(10, 0, Color::WHITE);
        assert_eq!(fb.width(), 10);
        assert_eq!(fb.height(), 0);
        assert_eq!(fb.pixels().len(), 0);
    }

    #[test]
    fn framebuffer_new_with_zero_dimensions() {
        let fb = Framebuffer::new(0, 0, Color::WHITE);
        assert_eq!(fb.width(), 0);
        assert_eq!(fb.height(), 0);
        assert_eq!(fb.pixels().len(), 0);
    }

    #[test]
    fn framebuffer_fill_color() {
        let fb = Framebuffer::new(2, 2, Color::rgb(10, 20, 30));
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(10, 20, 30)));
        assert_eq!(fb.pixel(1, 1), Some(Color::rgb(10, 20, 30)));
    }

    #[test]
    fn framebuffer_set_pixel() {
        let mut fb = Framebuffer::new(2, 2, Color::TRANSPARENT);
        assert!(fb.set_pixel(0, 0, Color::rgb(100, 150, 200)));
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(100, 150, 200)));
    }

    #[test]
    fn framebuffer_set_pixel_out_of_bounds() {
        let mut fb = Framebuffer::new(2, 2, Color::TRANSPARENT);
        assert!(!fb.set_pixel(2, 0, Color::WHITE));
        assert!(!fb.set_pixel(0, 2, Color::WHITE));
        assert!(!fb.set_pixel(100, 100, Color::WHITE));
    }

    #[test]
    fn framebuffer_pixel_out_of_bounds() {
        let fb = Framebuffer::new(2, 2, Color::WHITE);
        assert_eq!(fb.pixel(2, 0), None);
        assert_eq!(fb.pixel(0, 2), None);
        assert_eq!(fb.pixel(100, 100), None);
    }

    #[test]
    fn fill_rect_basic() {
        let mut fb = Framebuffer::new(4, 4, Color::TRANSPARENT);
        let rect = Rect::new(1.0, 1.0, 2.0, 2.0);
        let changed = fb.fill_rect(rect, Color::rgb(255, 0, 0));
        assert_eq!(changed, 4);
        assert_eq!(fb.pixel(1, 1), Some(Color::rgb(255, 0, 0)));
        assert_eq!(fb.pixel(2, 2), Some(Color::rgb(255, 0, 0)));
        assert_eq!(fb.pixel(0, 0), Some(Color::TRANSPARENT));
    }

    #[test]
    fn fill_rect_clipping() {
        let mut fb = Framebuffer::new(4, 4, Color::TRANSPARENT);
        // Rechteck ragt rechts und unten hinaus
        let rect = Rect::new(2.0, 2.0, 10.0, 10.0);
        let changed = fb.fill_rect(rect, Color::rgb(0, 255, 0));
        assert_eq!(changed, 4); // Nur 2x2 Pixel sichtbar
        assert_eq!(fb.pixel(2, 2), Some(Color::rgb(0, 255, 0)));
        assert_eq!(fb.pixel(3, 3), Some(Color::rgb(0, 255, 0)));
    }

    #[test]
    fn fill_rect_completely_outside() {
        let mut fb = Framebuffer::new(4, 4, Color::TRANSPARENT);
        let rect = Rect::new(10.0, 10.0, 5.0, 5.0);
        let changed = fb.fill_rect(rect, Color::rgb(0, 0, 255));
        assert_eq!(changed, 0);
    }

    #[test]
    fn fill_rect_transparent_color() {
        let mut fb = Framebuffer::new(4, 4, Color::rgb(255, 255, 255));
        let rect = Rect::new(0.0, 0.0, 2.0, 2.0);
        let changed = fb.fill_rect(rect, Color::TRANSPARENT);
        assert_eq!(changed, 0);
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(255, 255, 255)));
    }

    #[test]
    fn fill_rect_opaque_overwrite() {
        let mut fb = Framebuffer::new(2, 2, Color::rgb(255, 0, 0));
        let rect = Rect::new(0.0, 0.0, 2.0, 2.0);
        fb.fill_rect(rect, Color::rgb(0, 0, 255));
        assert_eq!(fb.pixel(0, 0), Some(Color::rgb(0, 0, 255)));
        assert_eq!(fb.pixel(1, 1), Some(Color::rgb(0, 0, 255)));
    }

    #[test]
    fn fill_rect_alpha_blending_half() {
        let mut fb = Framebuffer::new(2, 2, Color::rgb(255, 255, 255));
        let rect = Rect::new(0.0, 0.0, 2.0, 2.0);
        // Alpha=128 auf Weiss ergibt ~Mittelwert
        fb.fill_rect(rect, Color::new(0, 0, 0, 128));
        let pixel = fb.pixel(0, 0).unwrap();
        // Erwartet: ca. 128 fuer alle Kanaele
        assert!(pixel.r >= 126 && pixel.r <= 130, "r = {}", pixel.r);
        assert!(pixel.g >= 126 && pixel.g <= 130, "g = {}", pixel.g);
        assert!(pixel.b >= 126 && pixel.b <= 130, "b = {}", pixel.b);
    }

    #[test]
    fn fill_rect_empty_rect() {
        let mut fb = Framebuffer::new(4, 4, Color::WHITE);
        let rect = Rect::new(0.0, 0.0, 0.0, 0.0);
        let changed = fb.fill_rect(rect, Color::rgb(255, 0, 0));
        assert_eq!(changed, 0);
    }

    #[test]
    fn blend_source_over_transparent_src() {
        let dst = Color::rgb(100, 150, 200);
        let result = blend_source_over(Color::TRANSPARENT, dst);
        assert_eq!(result, dst);
    }

    #[test]
    fn blend_source_over_opaque_src() {
        let dst = Color::rgb(100, 150, 200);
        let result = blend_source_over(Color::rgb(50, 60, 70), dst);
        assert_eq!(result, Color::rgb(50, 60, 70));
    }

    #[test]
    fn blend_source_over_half_alpha() {
        let src = Color::new(0, 0, 0, 128);
        let dst = Color::rgb(255, 255, 255);
        let result = blend_source_over(src, dst);
        // Sollte ca. 128 ergeben
        assert!(result.r >= 126 && result.r <= 130);
    }

    #[test]
    fn blend_ganzzahl_gleich_f32_alle_256x256() {
        // Beweis: Die Ganzzahl-Implementierung liefert fuer ALLE 256x256
        // Alpha-Kombinationen dasselbe Ergebnis wie die alte f32-Fassung
        // (bis auf maximal 1 LSB Abweichung durch f32-Praezisionsverluste).
        // Die f32-Formel hat nur ~7 signifikante Stellen; die Ganzzahlarithmetik
        // ist exakt. Daher gibt es wenige Faelle, in denen f32 durch Rundung
        // im Zwischenrechenergebnis leicht abweicht (wie in
        // `result_matches_f64_reference_within_one` zugelassen: 1 LSB).
        let src_base = (60u8, 120u8, 200u8);
        let dst_base = (200u8, 100u8, 50u8);

        let mut mismatches = 0usize;
        for a_src in 0..=255u8 {
            for a_dst in 0..=255u8 {
                let src = Color::new(src_base.0, src_base.1, src_base.2, a_src);
                let dst = Color::new(dst_base.0, dst_base.1, dst_base.2, a_dst);

                let int_result = blend_source_over(src, dst);
                let f32_result = blend_source_over_f32(src, dst);

                if int_result != f32_result {
                    // Pruefen: darf nur 1 LSB pro Kanal abweichen
                    let dr = int_result.r.abs_diff(f32_result.r);
                    let dg = int_result.g.abs_diff(f32_result.g);
                    let db = int_result.b.abs_diff(f32_result.b);
                    let da = int_result.a.abs_diff(f32_result.a);
                    if dr <= 1 && dg <= 1 && db <= 1 && da <= 1 {
                        mismatches += 1;
                    } else {
                        panic!(
                            "Abweichung >1 LSB bei a_src={}, a_dst={}: int={:?} f32={:?}",
                            a_src, a_dst, int_result, f32_result
                        );
                    }
                }
            }
        }
        println!(
            "blend_ganzzahl_gleich_f32: {} von 65536 Faellen mit 1 LSB Abweichung (erwartet durch f32-Praezision)",
            mismatches
        );
        // Erlaubt: Abweichungen durch f32-Praezision (1 LSB)
        // Verboten: groessere Abweichungen (wuerden auf Formelfehler hinweisen)
    }
}
