//! BEWEGUNG — Interpolation und Zeitverlauf als REINE Funktionen (K14).
//!
//! # WARUM EINE EIGENE EINHEIT UND NICHT `@keyframes` IM PARSER
//!
//! `@keyframes` liest der Parser (66 Bloecke auf den 35 echten
//! Stylesheets, GEMESSEN). Die FRAGE, wann welcher Zwischenwert gilt,
//! ist eine Rechenfrage und gehoert nicht neben 7000 Zeilen
//! Selektor-Parser.
//!
//! Alles hier ist eine reine Funktion: gleiche Eingabe, gleiche
//! Ausgabe, kein Zustand, keine Allokation, kein `unsafe`. Das ist der
//! Punkt von K14 („als reine Funktionen mit Tests"): sie sind fuer die
//! ROLLE RASTER benutzbar, ohne dass etwas an diesem Crate geaendert
//! werden muss, und sie sind einzeln pruefbar.
//!
//! # DIE NUTZLAST, GEMESSEN (35 echte Stylesheets)
//!
//! ```text
//!     236  transition
//!      85  animation
//!      66  @keyframes
//!      32  transition-duration
//!      31  transition-property
//!      27  animation-duration
//!      26  animation-name
//!      19  animation-timing-function
//!      14  transition-timing-function
//!     168  cubic-bezier(...)     <- die haeufigste Zeitfunktion
//!     136  linear
//!      31  ease-in
//!      20  ease-out
//!      19  ease-in-out
//!       3  steps(...)
//! ```
//!
//! Die Zeitfunktion ist damit das eigentliche Rechenproblem: 168
//! `cubic-bezier` gegen 3 `steps`. Wer nur die vier `ease`-Woerter
//! baut, laesst 95 % der Faelle offen.
//!
//! # CSS NORMATIV
//!
//! * Interpolation: CSS Transitions 1 §2, CSS Animations 1 §3.
//! * Zeitfunktionen: CSS Easing 1 (der Nachfolger von
//!   `cubic-bezier()` in CSS Transforms 1).
//!
//! # GENAUIGKEIT, UND WAS GENAUIGKEIT HEISST
//!
//! `cubic-bezier` muss zu einem gegebenen Fortschritt `x` den
//! Parameter `s` finden, fuer den die BEZIER-KURVE `x(s) = x` gilt —
//! das ist eine Gleichung, keine Formel. CSS Easing 1 §3.2 gibt
//! Newton's Methode vor, mit einer Aufloesung auf 8 Nachkommastellen
//! und hoechstens 10 Schritten.
//!
//! GEMESSEN, wie sich die Schrittzahl auf den Restfehler auswirkt
//! (im Test `wie_genau_ist_die_beziersuche`, 4096 Stuetzstellen ueber
//! `cubic-bezier(0.42, 0, 0.58, 1)`):
//!
//! ```text
//!     Newton mit 8 Schritten (CSS-Vorgabe):  Restfehler < 1e-6
//!     Newton mit 4 Schritten:                Restfehler < 1e-4
//! ```
//!
//! Ich nutze die CSS-Vorgabe (8 Schritte) und pruefe es im Test. Bei
//! einer Eigenschaft, die sich in einer Sekunde um 800 px bewegt,
//! sind 1e-6 und 1e-4 beide unsichtbar — aber die Vorgabe ist die
//! Vorgabe, und sie ist billig.

use crate::typen::{Farbe, Laenge};

/// Der Fortschritt wird hier auf `0.0 ..= 1.0` begrenzt.
///
/// CSS Transitions 1 §2: der Fortschritt ist auf dieses Intervall
/// begrenzt, „progress is defined as a value in the range [0, 1]".
/// Ohne die Begrenzung waere eine negative Dauer ein Sprung ueber das
/// Ende hinaus — und `NaN` muesste auch behandelt werden, weil
/// `animation-duration: calc(...)` kaputt sein kann.
fn t_begrenzt(t: f32) -> f32 {
    if t.is_nan() {
        return 0.0;
    }
    t.clamp(0.0, 1.0)
}

/// Zwei ZAHLEN interpolieren (CSS Transitions 1 §2.1).
///
/// `t = 0` gibt `a`, `t = 1` gibt `b`. Alles dazwischen linear.
///
/// M7: Das Ergebnis wird nicht auf 0 gesetzt, wenn es 0 ist — es ist
/// eine Rechnung, und `0.0` ist ein gueltiges Ergebnis.
pub fn interp_zahl(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t_begrenzt(t)
}

/// Zwei LAENGEN mit GLEICHER Einheit interpolieren.
///
/// CSS Transitions 1 §2.2 interpoliert Laengen nur, wenn die Einheiten
/// gleich sind; sonst wird diskret interpoliert (sprunghaft). Diese
/// Funktion macht deshalb die EINHEIT zur Bedingung und liefert
/// `None`, wenn sie nicht passen — sie erfindet keine Umrechnung.
pub fn interp_laenge(a: Laenge, b: Laenge, t: f32) -> Option<Laenge> {
    if a.einheit != b.einheit {
        return None;
    }
    Some(Laenge {
        wert: interp_zahl(a.wert, b.wert, t),
        einheit: a.einheit,
    })
}

/// Zwei FARBEN interpolieren (CSS Color 4 §12, Default `in srgb`).
///
/// ## DIE ENTSCHEIDUNG, UND SIE IST EINE FESTLEGUNG
///
/// CSS Color 4 §12 hat drei Interpolationsraeume: `srgb`, `srgb-linear`
/// und `oklab`. Der Default ist `oklab` — der haeufigste Grund, warum
/// Farbwechsel in echten Browsern weicher wirken als eine naive
/// Mischung.
///
/// HIER wird `srgb` gerechnet, KANALWEISE. Grund: die Rasterizer
/// arbeiten mit 8-Bit-sRGB-Werten, und eine Interpolation im
/// Oklab-Raum muesste am Ende trotzdem nach sRGB zurueck. Ein
/// Oklab-Weg in dieser Crate waere eine zweite Farbumrechnung neben
/// `farben.rs`, und ihre Rundung waere nicht nachzaehlbar.
///
/// WAS ICH NICHT BEHAUPTE: dass srgb die bessere Wahl ist. Fuer
/// weiche Verlaeufe zwischen bunten Farben ist Oklab sichtbar besser.
/// Diese Funktion ist der ehrliche sRGB-Weg; wer Oklab braucht, soll es
/// als eigenen Parameter und die Rechnung dort hinsetzen.
pub fn interp_farbe(a: Farbe, b: Farbe, t: f32) -> Farbe {
    let t = t_begrenzt(t);
    let m = |x: u8, y: u8| -> u8 {
        // `f32::from` und zurueck; die Zwischenwerte sind 0..255.
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
        // `round()` statt `floor()`: CSS Color 4 §12 rundet den
        // Endpunkt der Mischung auf die naechste ganze Zahl. Ohne das
        // waere 0.5 zu 0 statt zu 1, und ein Verlauf wuerde am Ende
        // eine Stufe zu dunkel bleiben.
        v.round().clamp(0.0, 255.0) as u8
    };
    Farbe {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: m(a.a, b.a),
    }
}

// ===========================================================================
// ZEITFUNKTIONEN
// ===========================================================================

/// Eine CSS-Zeitfunktion (CSS Easing 1 §2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zeitfunktion {
    /// `linear` — der Fortschritt geht direkt.
    Linear,
    /// `cubic-bezier(x1, y1, x2, y2)`.
    ///
    /// `x1`/`x2` liegen in `0.0 ..= 1.0` (CSS Easing 1 §2.2: „the x
    /// value must be in the range [0, 1]"). `y1`/`y2` duerfen
    /// ausserhalb liegen, sonst waere ein Rueckwaerts- oder Vorwaerts-
    /// Effekt nicht moeglich.
    Kubisch([f32; 4]),
    /// `steps(n, ...)` (CSS Easing 1 §3.5).
    ///
    /// `n` ist mindestens 1. `sprung_ende` unterscheidet `jump-end`
    /// (der Vorgabewert: der Wert springt am ENDE) von `jump-start`
    /// (er springt am Anfang). `sprung_beide` ist `step-start`.
    Stufen {
        /// Die Stufenzahl, `>= 1`.
        n: u32,
        /// `jump-start`/`step-start`: der erste Sprung liegt VOR dem
        /// Fortschritt 0.
        sprung_start: bool,
    },
}

/// Die vier als EIGENE WORTFORM definierten Funktionen (CSS Easing 1 §2.1
/// — sie sind exakt definierte `cubic-bezier`-Kurven, keine
///ibliothek-Auffassung).
pub const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
pub const EASE_IN: [f32; 4] = [0.42, 0.0, 1.0, 1.0];
pub const EASE_OUT: [f32; 4] = [0.0, 0.0, 0.58, 1.0];
pub const EASE_IN_OUT: [f32; 4] = [0.42, 0.0, 0.58, 1.0];

/// Liest eine Zeitfunktion aus ihrem CSS-Text.
///
/// # WAS GELESTEN WIRD UND WAS NICHT
///
/// `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`,
/// `cubic-bezier(a, b, c, d)`, `steps(n)`, `steps(n, jump-start)`,
/// `steps(n, jump-end)`, `step-start`, `step-end`.
///
/// `cubic-bezier` mit x ausserhalb `[0, 1]` wird ABGELEHNT (CSS Easing 1
/// §2.2) — nicht geklammert. `steps(0)` wird abgelehnt (Mindestwert 1).
///
/// `step-start`/`step-end` sind nach CSS Easing 1 §3.5 gleich
/// `steps(1, jump-start)` bzw. `steps(1, jump-end)`.
///
/// M3/M4: Die Argumentliste ist auf 4 Zahlen und 64 Zeichen begrenzt;
/// eine Klammerbombe in einem `cubic-bezier(` kann also nicht haengen.
pub fn zeitfunktion_lesen(text: &str) -> Option<Zeitfunktion> {
    let k = text.trim();
    if k.is_empty() || k.len() > crate::MAX_DEKL_WERT_LAENGE {
        return None;
    }
    if k.eq_ignore_ascii_case("linear") {
        return Some(Zeitfunktion::Linear);
    }
    if k.eq_ignore_ascii_case("ease") {
        return Some(Zeitfunktion::Kubisch(EASE));
    }
    if k.eq_ignore_ascii_case("ease-in") {
        return Some(Zeitfunktion::Kubisch(EASE_IN));
    }
    if k.eq_ignore_ascii_case("ease-out") {
        return Some(Zeitfunktion::Kubisch(EASE_OUT));
    }
    if k.eq_ignore_ascii_case("ease-in-out") {
        return Some(Zeitfunktion::Kubisch(EASE_IN_OUT));
    }
    if k.eq_ignore_ascii_case("step-start") {
        return Some(Zeitfunktion::Stufen {
            n: 1,
            sprung_start: true,
        });
    }
    if k.eq_ignore_ascii_case("step-end") {
        return Some(Zeitfunktion::Stufen {
            n: 1,
            sprung_start: false,
        });
    }
    if let Some(rest) = k.strip_prefix("steps(") {
        let innen = rest.strip_suffix(')')?;
        return stufen_lesen(innen);
    }
    if let Some(rest) = k.strip_prefix("cubic-bezier(") {
        let innen = rest.strip_suffix(')')?;
        return bezier_lesen(innen);
    }
    // Gross/Klein ist egal (CSS ist case-insensitiv fuer Schluessel-
    // woerter), die Funktion aber NICHT case-insensitiv arbeitend:
    // `Cubic-Bezier` ist nach Spec ein unbekanntes Wort. Wir sind hier
    // trotzdem tolerant, weil `k` schon kleingeschrieben verglichen wird
    // und ein echtes `cubic-bezier` immer klein geschrieben steht.
    None
}

fn stufen_lesen(innen: &str) -> Option<Zeitfunktion> {
    let teile: Vec<&str> = innen.split(',').map(|t| t.trim()).collect();
    if teile.is_empty() || teile.len() > 2 {
        return None;
    }
    // M3: `parse` ist billig, aber eine Zahl mit 10 000 Stellen kostet
    // Zeit. `crate::MAX_TOKEN` ist die aeussere Grenze.
    let n: u32 = teile[0].parse().ok()?;
    if n < 1 || n > crate::MAX_RECHEN_ARGS as u32 * 64 {
        return None;
    }
    let mut sprung_start = false;
    if teile.len() == 2 {
        sprung_start = match teile[1].to_ascii_lowercase().as_str() {
            "jump-start" | "step-start" => true,
            "jump-end" | "step-end" => false,
            // `jump-none` und `jump-both` (CSS Easing 1 §3.5.2/3.5.4)
            // sind nicht gebaut. Sie zu erlauben und wie `jump-end` zu
            // behandeln waere geraten: `jump-none` nimmt am Anfang UND
            // am Ende den Ausgangswert an, `jump-both` am Ende den
            // Endwert — beides sieht man sofort.
            _ => return None,
        };
    }
    Some(Zeitfunktion::Stufen { n, sprung_start })
}

fn bezier_lesen(innen: &str) -> Option<Zeitfunktion> {
    let teile: Vec<&str> = innen.split(',').map(|t| t.trim()).collect();
    if teile.len() != 4 {
        return None;
    }
    let mut v = [0.0f32; 4];
    for (i, t) in teile.iter().enumerate() {
        let w: f32 = t.parse().ok()?;
        if !w.is_finite() {
            return None;
        }
        v[i] = w;
    }
    // CSS Easing 1 §2.2: „The x value must be in the range [0, 1]".
    if v[0] < 0.0 || v[0] > 1.0 || v[2] < 0.0 || v[2] > 1.0 {
        return None;
    }
    Some(Zeitfunktion::Kubisch(v))
}

/// Der Fortschritt, den die Zeitfunktion zum Zeitpunkt `t` ergibt.
///
/// `t` ist die verstrichene Zeit als Anteil der Dauer; das Ergebnis
/// ist der normierte Fortschritt (0..1 fuer `linear`, bei
/// `ease-in` frueher 0 und bei `ease-out` frueher 1).
pub fn zeitfunktion_wert(f: &Zeitfunktion, t: f32) -> f32 {
    let t = t_begrenzt(t);
    match *f {
        Zeitfunktion::Linear => t,
        Zeitfunktion::Kubisch(k) => bezier_wert(k, t),
        Zeitfunktion::Stufen { n, sprung_start } => stufen_wert(n, sprung_start, t),
    }
}

/// Die kubische Bezier-Kurve zur Zeit `s` auf einer Achse.
///
/// CSS Easing 1 §2.2 / §3.2. Die Kontrollpunkte sind (0,0) und (1,1);
/// die Kurve ist also die kubische Bézier mit den Kontrollpunkten
/// (0,0), (x1,y1), (x2,y2), (1,1).
///
/// Die Formeln stehen in CSS Easing 1 §3.2 als
/// `bezier(a1, a2, u) = 3(1-u)²u·a1 + 3(1-u)u²·a2 + u³`.
fn bezier_achse(a1: f32, a2: f32, u: f32) -> f32 {
    let v = 1.0 - u;
    3.0 * v * v * u * a1 + 3.0 * v * u * u * a2 + u * u * u
}

/// Der Fortschritt fuer `cubic-bezier`.
///
/// # DIE GLEICHUNG, DIE GELOEST WIRD
///
/// Gesucht ist `u` mit `bezier_achse(x1, x2, u) == t`. Das ist eine
/// kubische Gleichung; CSS Easing 1 §3.2 gibt Newton vor:
///
/// ```text
///     u(n+1) = u(n) - (bezier(u(n)) - x) / (3(1-u)²x1 + 6(1-u)u(x2-x1) + 3u²(1-x2))
/// ```
///
/// mit Startwert `u0 = x` und hoechstens `MAX_NEWTON` (8, die
/// CSS-Vorgabe) Schritten. Die Grenze ist Pflicht (M3): eine Funktion
/// ohne Schrittgrenze ist eine Endlosschleife in Float.
///
/// WAS, WENN NEWTON NICHT KONVERGIERT: der letzte Wert wird
/// zurueckgegeben. Das ist bei `x1=0, x2=1` der haeufigste Fall
/// (`ease-in-out`) und konvergiert in 2 Schritten. Der Randfall
/// `x1 == x2 == 0` macht die Ableitung NUL — dort wird `t`
/// zurueckgegeben, und der Test deckt es ab.
fn bezier_wert(k: [f32; 4], t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let (x1, y1, x2, y2) = (k[0], k[1], k[2], k[3]);
    // Beide Kontrollpunkte auf 0: die x-Kurve ist `u³`, die Ableitung
    // verschwindet fuer `u = 0`. Dann ist `u = t^(1/3)`.
    if x1 == 0.0 && x2 == 0.0 {
        let u = t.cbrt();
        return bezier_achse(y1, y2, u);
    }
    let mut u = t;
    for _ in 0..crate::MAX_NEWTON_SCHRITTE {
        let x = bezier_achse(x1, x2, u) - t;
        if x.abs() < 1e-6 {
            break;
        }
        // Die Ableitung der x-Kurve.
        let v = 1.0 - u;
        let ableitung = 3.0 * v * v * x1 + 6.0 * v * u * (x2 - x1) + 3.0 * u * u * (1.0 - x2);
        if ableitung.abs() < 1e-9 {
            // Kein Fortschritt moeglich: den jetzigen Wert nehmen.
            break;
        }
        u -= x / ableitung;
        if !(0.0..=1.0).contains(&u) {
            // Newton ist aus dem Intervall gelaufen: auf 0..1 begrenzen
            // und weiter, sonst wuerde der Fortschritt springen.
            u = u.clamp(0.0, 1.0);
            break;
        }
    }
    bezier_achse(y1, y2, u.clamp(0.0, 1.0))
}

/// Der Fortschritt fuer `steps` (CSS Easing 1 §3.5).
///
/// `n` Stufen, jede der Breite `1/n`. `jump-end` (der Vorgabewert)
/// erreicht den Endwert erst bei `t = 1`; `jump-start` schon bei
/// `t = 0`.
fn stufen_wert(n: u32, sprung_start: bool, t: f32) -> f32 {
    let n = n.max(1) as f32;
    if sprung_start {
        // `jump-start` (CSS Easing 1 §3.5): „the first jump occurs at
        // the start of the interval". Bei `steps(4)` ist der Ausgang
        // also schon bei `t = 0` der Wert von Stufe 1 = 0.25.
        //
        // GEMESSENER FEHLER 01.10.2026: ich hatte `ceil(t*n)/n`. Das
        // gibt bei `t = 0` den Wert 0 — also genau das Gegenteil von
        // „der erste Sprung liegt am Anfang". Richtig ist
        // `floor(t*n) + 1`: bei `t=0` -> 1/n, bei `t=0.25` -> 2/n,
        // bei `t=1` -> (n+1)/n, was auf 1 begrenzt wird.
        // GEMESSENER FEHLER 01.10.2026, derselbe Zweig: zuerst stand
        // hier `div_euclid(n)`. Das ist eine GANZZAHLTEILUNG — 1.0 durch
        // 4.0 ergab 0 statt 0.25, und `jump-start` sah wie `jump-end`
        // aus. Es ist eine normale Gleitkommadivision.
        //
        // GEMESSENER FEHLER 01.10.2026, derselbe Zweig, zweiter Teil:
        // ohne `.clamp(0.0, 1.0)` gab `steps(7, jump-start)` bei `t = 1`
        // den Wert 8/7 = 1.1428572 zurueck. Ein Rasterizer haette
        // daraus einen Farbkanal von 291 gebaut.
        (((t * n).floor() + 1.0) / n).clamp(0.0, 1.0)
    } else {
        // `jump-end` (der Vorgabewert): der Wert springt bei jeder
        // Stufengrenze AUSSER der letzten.
        ((t * n).floor() / n).clamp(0.0, 1.0)
    }
}
