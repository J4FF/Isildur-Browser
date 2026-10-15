//! Release 10.10.2026: das ERZWUNGENE Layout fuer Geometrie-Abfragen aus
//! Skripten — `offsetWidth`, `getBoundingClientRect`, `getComputedStyle(…)
//! .width` (CSSOM View: „flush layout").
//!
//! Chrome rechnet das Layout an der Abfrage synchron neu, wenn sich das DOM
//! seit dem letzten Layout geaendert hat. Hier lagen die Masse des LETZTEN
//! Layouts vor; GEMESSEN vorher (Web-Platform-Tests): `el.style.height =
//! '100px'; other.offsetTop` lieferte den alten Wert oder 0 (143 Dateien mit
//! `expected N but got 0`).
//!
//! Der Weg ist derselbe wie beim Neubau (`Sitzung::neubau_rechnen`): das
//! HTML aus dem lebenden DOM, Seitenbau, Layout — nur ohne die Anzeige zu
//! ersetzen. Ein ZEITBUDGET je Sekunde (250 ms) haelt Schleifen aus
//! „aendern, messen, aendern …" in Grenzen; ist es aufgebraucht, gelten die
//! alten Masse weiter.

use std::time::{Duration, Instant};

/// Diagnose-Schalter (einmal gelesen): `NARSIL_ZWANG_AUS=1` / `NARSIL_STIL_AUS=1`.
pub fn aus(name: &'static str) -> bool {
    static ZWANG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    static STIL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let zelle = if name == "NARSIL_ZWANG_AUS" { &ZWANG } else { &STIL };
    *zelle.get_or_init(|| std::env::var_os(name).is_some())
}

/// Hoechstens so viel Zeit je Sekunde fuer erzwungene Layouts.
const BUDGET: Duration = Duration::from_millis(250);

/// Rechnet das Layout neu, wenn das DOM seit dem letzten Layout geaendert
/// wurde (`Stand::layout_veraltet`), und schreibt die Masse in den Stand.
pub fn erzwingen(w: &mut crate::dom::Stand) {
    if !w.layout_veraltet || aus("NARSIL_ZWANG_AUS") {
        return;
    }
    let jetzt = Instant::now();
    let (start, summe) = match w.zwang_fenster {
        Some((s, d)) if jetzt.duration_since(s) <= Duration::from_secs(1) => (s, d),
        _ => (jetzt, Duration::ZERO),
    };
    if summe >= BUDGET {
        w.zwang_fenster = Some((start, summe));
        return;
    }
    let Some(satz) = w.stil_satz.clone() else { return };
    let Some(l) = w.lebend.as_mut() else { return };
    let (html, ids) = l.html_neu();
    let (breite, sicht) = satz.ansicht;
    // Release 12.10.2026: die Werte, die ein Skript in Formularfelder schrieb
    // (`input.value = …`), gelten auch im erzwungenen Layout. GEMESSEN vorher
    // (WPT OpaqueRange-geometry-*): direkt nach `value =` war das Feld leer
    // gelayoutet, jede Textgeometrie 0.
    let mut patch = narsil_page::Patch::default();
    if !w.feld_werte.is_empty() {
        for (&ord, host) in &ids {
            if let Some(v) = w.feld_werte.get(host) {
                patch.attribut.insert((ord, "value".to_string()), v.clone());
            }
        }
    }
    let mut seite = narsil_page::seite_aus_knoten_mit_ansicht_zustand(
        &html,
        &patch,
        breite,
        sicht,
        &satz.basis,
        &satz.messung,
        &satz.zustand,
    );
    let (fertig, vektor) = (&w.bilder_fertig, &w.bilder_vektor);
    crate::sitzung::bildmasse_uebernehmen_mit(&mut seite, |u| match vektor.get(u) {
        Some(m) => *m,
        None => fertig.get(u).copied().filter(|g| g.0 > 0 && g.1 > 0).map(|g| (g.0 as f32, g.1 as f32)),
    });
    if let Ok(baum) = crate::nachumbruch::layout(
        &mut seite,
        narsil_core::geometry::Size::new(breite as f32, sicht as f32),
        &satz.messung,
    ) {
        w.masse = crate::sitzung::masse_rechnen(&baum, &seite, &ids);
        w.raster_vorlagen = crate::sitzung::vorlagen_rechnen(&baum, &seite, &ids);
        if w.textlaeufe_an {
            w.textlaeufe = crate::bereich::textlaeufe_rechnen(&baum, &seite, &ids);
        }
    }
    rahmen_layouten(w, &satz);
    w.layout_veraltet = false;
    w.zwang_fenster = Some((start, summe + jetzt.elapsed()));
}

/// Release 11.10.2026: die Dokumente der Iframes (`Stand::rahmen`) mit der
/// Innengroesse ihres Iframes layouten; ihre Masse kommen zu den Massen der
/// Seite (die Host-Ids sind seitenweit eindeutig). Koordinaten sind die der
/// Iframe-Ansicht (CSSOM View §6: das Fenster des Iframes).
fn rahmen_layouten(w: &mut crate::dom::Stand, satz: &narsil_page::StilSatz) {
    w.rahmen_hosts.clear();
    let rahmen = w.rahmen.clone();
    for (iframe, dok) in rahmen {
        // Innengroesse: Rahmenbox minus Raender; ohne Kasten (versteckt) die
        // Vorgabe 300 x 150 (HTML §15.4.2).
        let (breite, hoehe) = match w.masse.get(&iframe) {
            // Masse: [x, y, b, h, Rand l, o, r, u, Position, rechts, unten].
            Some(m) => ((m[2] - m[4] - m[6]).max(0.0), (m[3] - m[5] - m[7]).max(0.0)),
            None => (300.0, 150.0),
        };
        let (breite, hoehe) = (breite.round().max(1.0) as u32, hoehe.round().max(1.0) as u32);
        let Some(l) = w.lebend.as_mut() else { return };
        let Some(k) = l.knoten(dok) else { continue };
        let (html, ids) = l.html_neu_von(k);
        let mut seite = narsil_page::seite_aus_knoten_mit_ansicht_zustand(
            &html,
            &narsil_page::Patch::default(),
            breite,
            hoehe,
            &satz.basis,
            &satz.messung,
            &satz.zustand,
        );
        let (fertig, vektor) = (&w.bilder_fertig, &w.bilder_vektor);
        crate::sitzung::bildmasse_uebernehmen_mit(&mut seite, |u| match vektor.get(u) {
            Some(m) => *m,
            None => fertig.get(u).copied().filter(|g| g.0 > 0 && g.1 > 0).map(|g| (g.0 as f32, g.1 as f32)),
        });
        if let Ok(baum) = crate::nachumbruch::layout(
            &mut seite,
            narsil_core::geometry::Size::new(breite as f32, hoehe as f32),
            &satz.messung,
        ) {
            let masse = crate::sitzung::masse_rechnen(&baum, &seite, &ids);
            for (h, m) in masse {
                w.rahmen_hosts.insert(h);
                w.masse.insert(h, m);
            }
        }
    }
}
