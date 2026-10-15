//! Fenster- und DPR-Masse werden in eine Render-Groesse umgerechnet.
//!
//! Die [`ViewportPolicy`] legt fest, wie aus logischer Fenstergroesse und
//! Geraete-Pixeldichte die physische Render-Groesse entsteht. Das ist die
//! Logik, die beim ersten Rendern gebraucht wird, nicht erst beim
//! Fensterschritt (siehe ADR-006: headless zuerst).

use narsil_core::geometry::Size;

use crate::surface::SurfaceError;

/// Rechnet Fenstermasse in eine Render-Groesse um.
///
/// Die Policy haelt drei Grenzen ein: Mindestgroesse, Obergrenze fuer die
/// Geraete-Pixeldichte (damit 4K-Screens nicht explodieren) und mindestens
/// 1x1 Pixel. Ungueltige Eingaben werden als Fehler gemeldet, nie als Panic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportPolicy {
    /// Logische Mindestbreite.
    pub min_width: f32,
    /// Logische Mindesthoehe.
    pub min_height: f32,
    /// Obergrenze fuer die DPR, damit 4K-Screens nicht explodieren.
    pub max_device_pixel_ratio: f32,
    /// CSS-Pixel pro logischem Pixel. MVP-0-Wert: 1.0.
    pub css_scale: f32,
}

impl Default for ViewportPolicy {
    /// Standard-Policy: min 200x150, max_dpr 3.0, css_scale 1.0.
    fn default() -> Self {
        Self {
            min_width: 200.0,
            min_height: 150.0,
            max_device_pixel_ratio: 3.0,
            css_scale: 1.0,
        }
    }
}

impl ViewportPolicy {
    /// Berechnet die Render-Groesse aus Fenstermassen und DPR.
    ///
    /// Regeln:
    /// 1. `device_pixel_ratio` wird auf `max_device_pixel_ratio` begrenzt.
    /// 2. Das Ergebnis wird auf mindestens `min_width`/`min_height`
    ///    gehalten.
    /// 3. Das Ergebnis wird auf mindestens 1x1 Pixel gehalten.
    /// 4. Ungueltige Eingaben (NaN, negativ, 0, unendlich — bei Fenster,
    ///    DPR oder Policy-Feldern) geben
    ///    `Err(SurfaceError::InvalidArgument(..))` zurueck, kein Panic.
    ///    `ZeroExtent`/`SizeMismatch` sind fuer `Surface::present`
    ///    reserviert, die Geraeteverlust-Variante fuer das echte
    ///    Fenster-Backend (TASK_0009); beide werden hier nicht verwendet.
    /// 5. Das Ergebnis wird auf ganze Pixel abgerundet (`floor`), damit
    ///    der Rasterizer immer ganzzahlige Masse bekommt.
    ///
    /// Formel: `floor(max(fenster * min(dpr, max_dpr) * css_scale, min))`,
    /// danach auf mindestens 1x1 begrenzt.
    pub fn render_size(
        &self,
        window_size: Size,
        device_pixel_ratio: f32,
    ) -> Result<Size, SurfaceError> {
        if !is_positive_finite(window_size.width) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige Fensterbreite: {}",
                window_size.width
            )));
        }
        if !is_positive_finite(window_size.height) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige Fensterhoehe: {}",
                window_size.height
            )));
        }
        if !is_positive_finite(device_pixel_ratio) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige device_pixel_ratio: {device_pixel_ratio}"
            )));
        }
        if !is_positive_finite(self.min_width) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige min_width in Policy: {}",
                self.min_width
            )));
        }
        if !is_positive_finite(self.min_height) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige min_height in Policy: {}",
                self.min_height
            )));
        }
        if !is_positive_finite(self.max_device_pixel_ratio) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige max_device_pixel_ratio in Policy: {}",
                self.max_device_pixel_ratio
            )));
        }
        if !is_positive_finite(self.css_scale) {
            return Err(SurfaceError::InvalidArgument(std::format!(
                "ungueltige css_scale in Policy: {}",
                self.css_scale
            )));
        }

        // Regel 1: DPR begrenzen.
        let effective_dpr = device_pixel_ratio.min(self.max_device_pixel_ratio);
        let scaled_w = window_size.width * effective_dpr * self.css_scale;
        let scaled_h = window_size.height * effective_dpr * self.css_scale;
        if !scaled_w.is_finite() || !scaled_h.is_finite() {
            return Err(SurfaceError::InvalidArgument(
                "Render-Groesse laeuft ueber (nicht endlich)".into(),
            ));
        }

        // Regel 2: Mindestgroesse halten. Regel 3: mindestens 1x1.
        // Regel 5: auf ganze Pixel abrunden.
        let out_w = scaled_w.max(self.min_width).max(1.0).floor();
        let out_h = scaled_h.max(self.min_height).max(1.0).floor();
        if !is_positive_finite(out_w) || !is_positive_finite(out_h) {
            return Err(SurfaceError::InvalidArgument(
                "Render-Groesse ist nach Rundung ungueltig".into(),
            ));
        }
        Ok(Size::new(out_w, out_h))
    }
}

/// `true` genau dann, wenn der Wert endlich und groesser als 0 ist.
///
/// Fasst die Gueltigkeitspruefung fuer alle Viewport-Eingaben zusammen:
/// NaN, unendlich, 0 und negative Werte sind ungueltig.
fn is_positive_finite(v: f32) -> bool {
    v.is_finite() && v > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_values() {
        let p = ViewportPolicy::default();
        assert_eq!(p.min_width, 200.0);
        assert_eq!(p.min_height, 150.0);
        assert_eq!(p.max_device_pixel_ratio, 3.0);
        assert_eq!(p.css_scale, 1.0);
    }

    #[test]
    fn identity_at_dpr_one() {
        let p = ViewportPolicy::default();
        let out = p.render_size(Size::new(800.0, 600.0), 1.0);
        assert_eq!(out, Ok(Size::new(800.0, 600.0)));
    }

    #[test]
    fn helper_rejects_non_positive_non_finite() {
        assert!(is_positive_finite(1.0));
        assert!(!is_positive_finite(0.0));
        assert!(!is_positive_finite(-1.0));
        assert!(!is_positive_finite(f32::NAN));
        assert!(!is_positive_finite(f32::INFINITY));
    }
}
