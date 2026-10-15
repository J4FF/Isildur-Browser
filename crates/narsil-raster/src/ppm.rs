//! PPM-P6-Schreiber.
//!
//! PPM (Portable Pixmap) ist ein einfaches, unkomprimiertes Bildformat.
//! P6 ist die binaere Variante mit RGB-Bytes.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use crate::framebuffer::Framebuffer;

/// Schreibt den Framebuffer als binaere PPM-Datei (P6).
///
/// Format:
/// ```text
/// P6\n
/// {width} {height}\n
/// 255\n
/// {RGB-Bytes}
/// ```
///
/// Alpha wird verworfen: Die RGB-Kanäle werden direkt geschrieben.
/// Der Hintergrund wird als Schwarz dargestellt, wenn Alpha < 255.
///
/// Rueckgabe: Anzahl geschriebener Bytes.
pub fn write_ppm(fb: &Framebuffer, path: &Path) -> io::Result<usize> {
    let mut file = File::create(path)?;

    // Header schreiben
    let header = format!("P6\n{} {}\n255\n", fb.width(), fb.height());
    file.write_all(header.as_bytes())?;

    let mut bytes_written = header.len();

    // Pixeldaten schreiben (RGB, Alpha wird verworfen)
    let pixels = fb.pixels();
    for i in (0..pixels.len()).step_by(4) {
        let r = pixels[i];
        let g = pixels[i + 1];
        let b = pixels[i + 2];
        // Alpha wird verworfen
        file.write_all(&[r, g, b])?;
        bytes_written += 3;
    }

    Ok(bytes_written)
}

/// Schreibt den Framebuffer als PPM mit Weiss als Hintergrund.
///
/// Transparente Pixel werden auf Weiss gemischt.
/// Dies ist nuetzlich, wenn das Bild auf weissem Hintergrund angezeigt wird.
///
/// Rueckgabe: Anzahl geschriebener Bytes.
pub fn write_ppm_with_white_background(fb: &Framebuffer, path: &Path) -> io::Result<usize> {
    let mut file = File::create(path)?;

    // Header schreiben
    let header = format!("P6\n{} {}\n255\n", fb.width(), fb.height());
    file.write_all(header.as_bytes())?;

    let mut bytes_written = header.len();

    // Pixeldaten schreiben (RGB, Alpha auf Weiss mischen)
    let pixels = fb.pixels();
    for i in (0..pixels.len()).step_by(4) {
        let r = pixels[i];
        let g = pixels[i + 1];
        let b = pixels[i + 2];
        let a = pixels[i + 3];

        // Alpha auf Weiss mischen
        let (fr, fg, fb_val) = blend_on_white(r, g, b, a);
        file.write_all(&[fr, fg, fb_val])?;
        bytes_written += 3;
    }

    Ok(bytes_written)
}

/// Mischt eine Farbe auf Weiss.
fn blend_on_white(r: u8, g: u8, b: u8, a: u8) -> (u8, u8, u8) {
    if a == 255 {
        return (r, g, b);
    }
    if a == 0 {
        return (255, 255, 255);
    }

    let a_f = a as f32 / 255.0;
    let inv_a = 1.0 - a_f;

    let fr = (r as f32 * a_f + 255.0 * inv_a) as u8;
    let fg = (g as f32 * a_f + 255.0 * inv_a) as u8;
    let fb = (b as f32 * a_f + 255.0 * inv_a) as u8;

    (fr, fg, fb)
}

/// Parst den PPM-Header und gibt die Position nach dem Header zurueck.
///
/// Format: `P6\n{width} {height}\n255\n`
pub fn parse_ppm_header(data: &[u8]) -> Option<usize> {
    // Mindestens "P6\n" + etwas + "\n255\n"
    if data.len() < 4 || &data[0..3] != b"P6\n" {
        return None;
    }

    // Erstes Newline nach "P6" finden (Position 2)
    // Zweites Newline finden
    let mut pos = 3;
    let mut newlines_found = 1; // Wir haben bereits eines nach "P6"

    while pos < data.len() && newlines_found < 3 {
        if data[pos] == b'\n' {
            newlines_found += 1;
        }
        pos += 1;
    }

    if newlines_found == 3 { Some(pos) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_core::color::Color;
    use std::fs;

    #[test]
    fn write_ppm_creates_file() {
        let fb = Framebuffer::new(2, 2, Color::rgb(255, 0, 0));
        let path = std::env::temp_dir().join("test_narsil_raster.ppm");

        // Aufräumen VOR dem unwrap
        let _ = fs::remove_file(&path);

        let bytes = write_ppm(&fb, &path).unwrap();
        assert!(bytes > 0);

        // Datei als Bytes einlesen
        let data = fs::read(&path).unwrap();

        // Header finden und pruefen
        let header_end = parse_ppm_header(&data).expect("Gültiger P6-Header");
        let header = String::from_utf8_lossy(&data[..header_end]);
        assert!(header.starts_with("P6\n2 2\n255\n"));

        // Aufräumen
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn write_ppm_pixel_data() {
        let mut fb = Framebuffer::new(2, 1, Color::TRANSPARENT);
        fb.set_pixel(0, 0, Color::rgb(255, 0, 0));
        fb.set_pixel(1, 0, Color::rgb(0, 255, 0));

        let path = std::env::temp_dir().join("test_narsil_raster_pixels.ppm");

        // Aufräumen VOR dem unwrap
        let _ = fs::remove_file(&path);

        write_ppm(&fb, &path).unwrap();

        // Datei als Bytes einlesen
        let data = fs::read(&path).unwrap();

        // Header-Länge dynamisch berechnen
        let header_end = parse_ppm_header(&data).expect("Gültiger P6-Header");

        // Gesamtlänge = Header + 2 Pixel * 3 Bytes
        assert_eq!(data.len(), header_end + 2 * 3);

        // Pixeldaten
        assert_eq!(data[header_end], 255); // R
        assert_eq!(data[header_end + 1], 0); // G
        assert_eq!(data[header_end + 2], 0); // B
        assert_eq!(data[header_end + 3], 0); // R
        assert_eq!(data[header_end + 4], 255); // G
        assert_eq!(data[header_end + 5], 0); // B

        // Aufräumen
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn write_ppm_with_white_background_transparent() {
        let fb = Framebuffer::new(1, 1, Color::TRANSPARENT);
        let path = std::env::temp_dir().join("test_narsil_raster_white.ppm");

        // Aufräumen VOR dem unwrap
        let _ = fs::remove_file(&path);

        write_ppm_with_white_background(&fb, &path).unwrap();

        // Datei als Bytes einlesen
        let data = fs::read(&path).unwrap();

        // Header-Länge dynamisch berechnen
        let header_end = parse_ppm_header(&data).expect("Gültiger P6-Header");

        // Transparent -> Weiss
        assert_eq!(data[header_end], 255); // R
        assert_eq!(data[header_end + 1], 255); // G
        assert_eq!(data[header_end + 2], 255); // B

        // Aufräumen
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn write_ppm_with_white_background_opaque() {
        let fb = Framebuffer::new(1, 1, Color::rgb(100, 150, 200));
        let path = std::env::temp_dir().join("test_narsil_raster_white_opaque.ppm");

        // Aufräumen VOR dem unwrap
        let _ = fs::remove_file(&path);

        write_ppm_with_white_background(&fb, &path).unwrap();

        // Datei als Bytes einlesen
        let data = fs::read(&path).unwrap();

        // Header-Länge dynamisch berechnen
        let header_end = parse_ppm_header(&data).expect("Gültiger P6-Header");

        // Opaque -> Originalfarbe
        assert_eq!(data[header_end], 100); // R
        assert_eq!(data[header_end + 1], 150); // G
        assert_eq!(data[header_end + 2], 200); // B

        // Aufräumen
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parse_ppm_header_various_sizes() {
        // Teste verschiedene Header-Größen
        let test_cases = vec![
            (b"P6\n1 1\n255\n".to_vec(), 11),
            (b"P6\n10 10\n255\n".to_vec(), 13),
            (b"P6\n100 100\n255\n".to_vec(), 15),
        ];

        for (input, expected_len) in test_cases {
            let result = parse_ppm_header(&input);
            assert_eq!(
                result,
                Some(expected_len),
                "Fehler bei Länge {}",
                input.len()
            );
        }
    }
}
