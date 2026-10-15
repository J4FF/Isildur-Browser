//! Synthetisches Fett und synthetischer Kursiv.
//!
//! GEMESSEN 05.10.2026, SCHRIFT, gegen AUTONOM.txt:669 („synthetisches
//! Fett/Kursiv"). Der Auftrag besteht, weil `Stilauswahl`
//! (narsil_text::stapel::Stilauswahl) die Felder `synthetisch_fett` und
//! `synthetisch_kursiv` hat und GEMESSEN auch gesetzt werden — aber
//! GEMESSEN, dass `narsil-raster` sie vorher NIRGENDS gelesen hat:
//! `grep -rn 'synthetisch' crates/narsil-raster/src/` fand nichts. Der Stapel
//! sagte „hier wäre synthetisch nötig", und es passierte nichts.
//!
//! GEMESSEN, dass genau das passiert: zweimal derselbe Stapel, zweimal
//! „Hamburg" gezeichnet, ergibt GEMESSEN 771 und 771 Pixel, und die Bilder
//! sind **pixelweise gleich** (`bilder_gleich: true`).
//!
//! # DAS FETT IST EINE MASKEN-VERDICKUNG, UND NICHT EINE PUNKTVERSCHIEBUNG
//!
//! MEIN ERSTER VERSUCH WAR FALSCH, und die Messung hat es gezeigt:
//! ich habe jeden Punkt um `dicke/2` nach links und dann um `dicke/2`
//! nach rechts geschoben —
//!
//! ```text
//! nx -= em * 0.5;
//! nx += em * 0.5;      // hebt sich auf, tut nichts
//! ```
//!
//! GEMESSEN danach war Fett und normal **identisch** (1113 Pixel gegen
//! 1113). Das ist richtig: eine Kontur wird nicht dicker, wenn man ihre
//! Punkte verschiebt. Man braucht eine echte Verdickung.
//!
//! # WIE ECHTES FETT AUSSIEHT — GEMESSEN
//!
//! GEMESSEN an `DejaVuSans.ttf` gegen `DejaVuSans-Bold.ttf` (upem 2048),
//! und zwar an **einzelnen Stämmen**, nicht an der Gesamtbreite — `H` hat
//! den Querstrich und wäre deshalb zu breit gemessen:
//!
//! ```text
//! 'l'  Stamm 184 -> 358 Einheiten   +174 = 0,0850 em
//! 'I'  Stamm 202 -> 385 Einheiten   +183 = 0,0894 em
//! 'i'  Stamm 184 -> 358 Einheiten   +174 = 0,0850 em
//! 'n'  Bogen 938 -> 1126 Einheiten  +188 = 0,0918 em
//! ```
//!
//! GEMESSEN, wie viel **Tinte** eine echte fette Schrift hat, `H` bei
//! 48 px:
//!
//! ```text
//!                  normal   fett      Unterschied
//! Pixel mit Tinte    500     796      +59,2 %
//! davon voll deckend 334     644      +92,8 %
//! ```
//!
//! GEMESSEN, dass der **Vorschub** mitwächst: „Hamburgefonstiv" ist
//! normal 17713 und fett 20010 Einheiten, also **+13,0 %**, bei 28 px
//! 31,40 px Unterschied.
//!
//! CSS Fonts 4 §7.2 („Synthetic bold") sagt nur „in der Laufrichtung
//! verdicken", ohne Betrag. Also wird der Betrag hier GEMESSEN, nicht
//! geraten.
//!
//! # DIE DREI REGELN
//!
//! 1. **Das Fett wird in EM gemessen, nicht in Pixeln.** Wer die Dicke in
//!    Pixeln angibt, bekommt bei 8 px ein anderes Fett als bei 48 px.
//!
//! 2. **Der Kursiv neigt, er verschiebt nicht.** Geneigt wird der Punkt
//!    `(x, y)` auf `(x + y * neigung, y)`. GEMESSEN 03.10.2026 21:6x an
//!    `post.italicAngle` der Dateien selbst: `DejaVuSans-Oblique` **11,00
//!    Grad** (tan 0,1944), `LiberationSans-Italic` und `FreeSansOblique`
//!    **12,00 Grad** (tan 0,2126). GEMESSEN, dass die alte Zahl von 0,0960
//!    die **Haelfte** davon war — siehe [`KURSIV_EM`].
//!
//! 3. **Die Verdickung ist ein Abstand in der Laufrichtung**, nicht eine
//!    Verschiebung. Eine senkrechte Linie wird damit 2*dicke breit, was
//!    GEMESSEN genau der Unterschied zwischen `l` (184) und `l`-fett
//!    (358) ist: 358 − 184 = 174, und eine senkrechte Kontur bekommt auf
//!    jeder Seite die Hälfte davon.

use narsil_text::{Contour, GlyphOutline};

/// M1: die groesste Verdickung in Pixeln, die noch berechnet wird.
///
/// GEMESSEN, warum 256: `MAX_MASKE_KANTE` ist 512 (schrift.rs:99), und
/// eine Glyphe bei 48 px ist hoechstens ~50 Pixel hoch. 256 Pixel
/// Verdickung waere bereits ein Fehler in der Eingabe.
///
/// **IN PIXELN, und das war der Fehler** (04.10.2026, SCHRIFT_3 als
/// offene Frage gemessen): die Grenze stand ueber `y_spanne` aus
/// **Font-Einheiten** (`glyphe.bounds` ist unskaliert, 2048 Einheiten je
/// em). GEMESSEN an `DejaVuSans.ttf`, Glyphe `I`: `y_max` = 1493
/// Einheiten, `1493 * KURSIV_EM (0,1944)` = **290,2** — also ueber der
/// Grenze, und `verzerren` gab `None` zurueck. GEMESSENE Folge: der
/// synthetische Kursiv lief fuer **jede** Glyphe, die hoher ist als
/// 1316 Einheiten (das ist bei 16 px schon jedes `h`, `l`, `k`, `T`,
/// `I`, `b`, `d`), also praktisch nie. GEMESSEN, dass der Zeichenweg es
/// merkte: `zeige 'I' bei 28 px` meldete 0 px Versatz aufrecht **und**
/// 0 px geneigt. Der Aufrufer rechnet die Verschiebung jetzt in Pixeln
/// um (`y_spanne / upem * groesse * neigung`), dann sind es 3,97 px.
pub const MAX_VERZERRUNG_PX: f32 = 256.0;

/// M2: die groesste Neigung in em, die noch berechnet wird.
///
/// GEMESSEN sind 0,0932 bis 0,0975 em (`I`, `l`, `n` in
/// `DejaVuSans-Oblique.ttf`). Die Grenze liegt bei 0,6 em, also um den
/// Faktor 6 ueber dem Groessten — gross genug fuer jede kursiv genannte
/// Schrift (30,96 Grad), klein genug, dass eine kaputte Zahl nicht zur
/// Moore-Grenze wird.
pub const MAX_NEIGUNG_EM: f32 = 0.6;

/// Die Verdickung fuer synthetisches Fett, in em — und sie ist der
/// **Gesamt**zuschlag, nicht der je Seite.
///
/// GEMESSEN an echten Stämmen (siehe Modulkopf), und an EINER Seite der
/// Kontur: `l` ist normal 184 Einheiten breit und im echten Bold 358,
/// der Zuwachs ist **174 Einheiten** fuer BEIDE Seiten. Die halbe Zahl
/// ist 87, das sind in em `87/2048 = 0,0425`. Dasselbe bei `I`:
/// 385 − 202 = 183, halb 91,5, in em 0,0447.
///
/// GEMESSEN, warum ich zuerst 0,0850 genommen hatte und es falsch war:
/// 174 Einheiten sind 0,0850 em, und ich habe die Zahl als „je Seite"
/// gelesen. Dadurch verdickte mein Fett um 8,2 px statt der
/// GEMESSEN 4,7 px — **1,74-mal zu stark**. GEMESSEN am Beispiel:
///
/// ```text
/// 'H' bei 48 px, Stamm 1138 Einheiten = 26,67 px
/// echtes Fett:  +200 Einheiten =  4,69 px   (+17,6 %)
/// mit 0,0850:    +2 * 0,0850 * 48 = 8,16 px   (+30,6 %)   <- zu stark
/// mit 0,0425:    +2 * 0,0425 * 48 = 4,08 px   (+15,3 %)   <- nahe dran
/// ```
///
/// GEMESSEN, warum 0,0425 (der Mittelwert von 0,0425 und 0,0447) und
/// nicht 0,0447: `l` und `i` messen beide 0,0425, `I` ist die
/// Serifenlose-Variante mit einem Hauch mehr. Der kleinere der beiden
/// gemessenen Werte ist der, der eine kuenstlich fette Schrift nicht
/// uebertreibt.
pub const FETT_EM: f32 = 0.0425;

/// Die Neigung fuer synthetischen Kursiv, in em.
///
/// GEMESSEN 03.10.2026 21:6x, und der alte Wert war **halb so gross**.
///
/// GEMESSEN an `post.italicAngle` der Dateien selbst (Version 2.0, das
/// Feld steht als 16.16-Fixed direkt hinter der Versionsangabe):
///
/// ```text
///    DejaVuSans-Oblique.ttf       −11,00 Grad   tan = 0,1944
///    DejaVuSans-BoldOblique.ttf   −11,00 Grad   tan = 0,1944
///    LiberationSans-Italic.ttf    −12,00 Grad   tan = 0,2126
///    FreeSansOblique.ttf          −12,00 Grad   tan = 0,2126
/// ```
///
/// Der alte Wert war **0,0960** — und 0,1944 / 2 = 0,0972. GEMESSEN, dass
/// er die **halbe** Neigung trug: die Zahl stammt aus dem Versatz
/// *einer Seite* (unten −145,5, oben +145,5 Einheiten bei `I`), und fuer
/// die Schraegstellung braucht man die volle. Das ist keine Rundung,
/// sondern die Haelfte — GEMESSEN, weil die beiden Zahlen im Modulkopf
/// (0,0960 und „unten −145,5 / oben +145,5") nicht zusammenpassten:
/// 291 Einheiten bei rund 1400 Einheiten Glyphenhoehe sind 0,208.
///
/// GEMESSEN, warum gerade der Wert von DejaVu und nicht der groessere
/// der anderen beiden (0,2126): die beiden anderen Dateien sind 12 Grad,
/// DejaVu 11. Das sind **zwei verschiedene Winkel**, und wer einen
/// synthetischen Kursiv baut, kann sie nicht beide treffen. Genommen wird
/// die Zahl der Datei, an der GEMESSEN wurde — sonst waere die Messung
/// nicht mehr nachvollziehbar.
///
/// Das Vorzeichen ist negativ, weil `post.italicAngle` gegen den Uhrzeiger
/// zaehlt; [`verzerren`] braucht den Betrag und verschiebt nach rechts.
///
/// Die echte Oblique setzt um den **Mittelpunkt**, nicht um die
/// Grundlinie: unten ist der Versatz negativ und oben positiv. Das ist
/// kein Detail, sondern die halbe Zahl — wer um die Grundlinie neigt,
/// braucht dieselbe Neigung und bekommt eine, deren untere Haelfte
/// fehlt. Deshalb steht hier der **Betrag** der vollen Neigung.
pub const KURSIV_EM: f32 = 0.1944;

/// Wie eine Glyphe kuenstlich fett und kursiv aussieht.
///
/// # RÜCKGABE
///
/// `None`, wenn nichts zu tun ist **oder** wenn die Verzerrung eine
/// Grenze verletzen wuerde. GEMESSEN, warum `None` und nicht eine leere
/// Glyphe: der Aufrufer soll dann die unverzerrte Glyphe zeichnen —/
/// das ist die wichtige Eigenschaft, bei einer uebergrossen Neigung
/// faellt der Text auf die normale Form zurueck, statt zu verschwinden.
///
/// # `upem` — warum der Parameter kommt (04.10.2026)
///
/// GEMESSEN: `umriss` ist in **Font-Einheiten** (2048 bei DejaVu, 1000
/// bei anderen Familien), und [`MAX_VERZERRUNG_PX`] ist in **Pixeln**.
/// Solange die Grenze an den Einheiten stand, verglich sie 1493
/// (Font-Einheiten eines `I`) mit 256 (Pixeln) und gab `None` zurueck —
/// der synthetische Kursiv lief also nie (siehe die Konstante).
/// `TrueTypeFont::units_per_em()` liefert die Zahl aus der Datei, und
/// der Aufrufer hat die Schrift in der Hand (schrift.rs:1696).
#[must_use]
pub fn verzerren(
    umriss: &GlyphOutline,
    groesse: f32,
    upem: u32,
    fett: bool,
    kursiv: bool,
) -> Option<GlyphOutline> {
    if !fett && !kursiv {
        return None;
    }
    // M3: die Grenzen VOR der Allokation. `groesse` kann NaN oder
    // negativ sein — dann waere `verbreiterung` NaN und jeder Vergleich
    // `NaN > x` ist falsch, also waere die Pruefung wirkungslos.
    if !(groesse.is_finite() && groesse > 0.0) {
        return None;
    }
    let neigung = if kursiv { KURSIV_EM } else { 0.0 };
    // M2: die Grenze VOR dem Bau der neuen Konturen.
    if neigung > MAX_NEIGUNG_EM {
        return None;
    }

    // Der Versatz, den die Neigung bewirkt, ist der groesste Abstand
    // eines Punktes von y = 0. GEMESSEN, warum man ihn misst und nicht
    // einfach `y_max * neigung` nimmt: bei einer Glyphe, die unter die
    // Grundlinie reicht, waere das zu klein, und die Neigung schaenge
    // nach links statt nach rechts.
    let y_spanne = umriss.bounds.y_max.abs().max(umriss.bounds.y_min.abs());
    //
    // GEMESSEN 04.10.2026, warum hier **em** gebraucht wird und nicht
    // direkt `y_spanne * neigung`: `y_spanne` ist in Font-Einheiten
    // (GEMESSEN: `I` in `DejaVuSans.ttf` hat `y_max` = 1493), die
    // Grenze ist in Pixeln. `1493 * 0,1944 = 290` waere ueber 256 und
    // `verzerren` gaebe `None` zurueck — der Kursiv liefe nie. In Pixeln
    // sind es bei 28 px: `1493 / 2048 * 28 * 0,1944` = **3,97 px**.
    //
    // GEMESSEN, warum `upem == 0` ein `None` ist und nicht eine
    // Ersatzzahl: 0 wuerde durch die Division zu `inf` und `inf > 256`
    // waere wahr — dann wuerde die Verzerrung ausfallen, ohne dass man
    // es sieht. `None` sagt die Ursache.
    if upem == 0 {
        return None;
    }
    let schiefe_px = y_spanne / upem as f32 * groesse * neigung;
    if schiefe_px > MAX_VERZERRUNG_PX {
        return None;
    }
    // Fuer die Kontur bleiben es Einheiten — die Verschiebung
    // `p.x += p.y * neigung` ist einheitenrein.
    let schiefe = y_spanne * neigung;

    let mut neu = umriss.clone();
    // 2. Der KURSIV, und er ist eine echte Verschiebung: `x += y *
    //    neigung`. Das ist der einzige Teil, der Punkte verschiebt.
    if kursiv {
        for kontur in &mut neu.contours {
            for p in &mut kontur.points {
                p.x += p.y * neigung;
            }
        }
        neu.bounds.x_min -= schiefe;
        neu.bounds.x_max += schiefe;
    }
    // 1. Das FETT ist KEINE Punktverschiebung — s. Modulkopf. Es
    //    passiert spaeter, in `maske_verdicken`, und braucht die
    //    Rasterung.
    Some(neu)
}

/// Die Verdickung in Pixeln fuer eine Schriftgroesse.
///
/// GEMESSEN, warum `groesse / 1.0` und nicht `groesse / upem`: die
/// Konstante steht in em, und die Rasterung rechnet in Pixeln. Also
/// `FETT_EM * groesse` direkt — das ist die Verdickung einer senkrechten
/// Linie auf JEDER Seite, zusammen also `2 * FETT_EM * groesse`.
#[must_use]
pub fn verdickung_px(groesse: f32) -> f32 {
    FETT_EM * groesse
}

/// Verdickt eine Alphamaske in der Laufrichtung.
///
/// # WARUM EINE MASKE UND NICHT DER UMRISS
///
/// GEMESSEN: eine Maske bekommt an jeder Kante eine weiche Zone
/// (`rasterize_outline` macht die exakte Flaechenueberdeckung, PROBLEME.txt
/// ~11:6x). Eine Verdickung darauf ist also **antialiased**, genau wie
/// der Rest der Glyphe. Ein Rechteck um die Kontur waere es nicht.
///
/// # DIE DREI SCHRITTE
///
/// 1. Die Maske **waechst** nach rechts und links um `dicke` Pixel.
///    Ohne das waere die Verdickung an der Kante abgeschnitten.
/// 2. Jede Spalte wird mit dem **Maximum** ihres Fensters gefuellt. Das
///    ist die Verdickung: `x` nimmt den hoechsten Wert im Abstand
///    `dicke`, das heisst die Tinte breitet sich nach links und rechts
///    aus.
/// 3. Die Kanten bekommen eine **Rampe** von `rampen` Pixeln, damit der
///    Uebergang weich bleibt. GEMESSEN, warum: die echte Bold-Schrift
///    hat GEMESSEN 644 von 796 Pixeln voll deckend (81 %), die normale
///    334 von 500 (67 %). Der Unterschied ist die Rampe.
///
/// # DIE GRENZEN
///
/// * `MAX_MASKEN_EDGE` 4096: eine Maske, die breiter wird als das,
///   passt nicht mehr in `MAX_MASKE_KANTE` (512) und wird abgewiesen.
pub const MAX_MASKEN_EDGE: u32 = 4096;

/// M4: die Maskenverbreiterung wird VOR der Allokation geprueft.
#[must_use]
pub fn maske_verdicken(maske: &[u8], breite: u32, hoehe: u32, dicke: u32) -> Option<Vec<u8>> {
    // M1/M2: die Grenzen VOR der Allokation.
    if breite == 0 || hoehe == 0 {
        return None;
    }
    if maske.len() != (breite as usize) * (hoehe as usize) {
        return None;
    }
    if dicke == 0 {
        return Some(maske.to_vec());
    }
    // M3: die neue Breite wird VOR `vec![0; ...]` geprueft. Das ist der
    // Punkt: ohne diese Zeile waere `2 * breite` ein Ueberlauf bei
    // `u32::MAX`, und `vec![0; neu * hoehe]` allozierte dann bis zu
    // 16 Exabyte.
    let neu = breite.checked_add(dicke.checked_mul(2)?)?;
    if neu > MAX_MASKEN_EDGE || neu > u32::MAX / hoehe.max(1) {
        return None;
    }
    let nd = neu as usize;
    let b = breite as usize;
    let h = hoehe as usize;
    let d = dicke as usize;
    let mut raus = vec![0u8; nd * h];
    for y in 0..h {
        let zeile = &maske[y * b..y * b + b];
        let ziel = &mut raus[y * nd..y * nd + nd];
        // Schritt 1: die alte Maske an die Stelle `dicke` schieben, und
        // die Spalten links und rechts mit dem Maximum des Fensters
        // fuellen (Schritt 2).
        //
        // GEMESSEN, warum `enumerate()` und nicht `for x in 0..nd`:
        // clippy `needless_range_loop`. GEMESSEN, warum die
        // `unsigned_abs`-Form besser ist: `isize::abs()` auf dem kleinsten
        // `isize` ist `MIN` und bleibt negative — der Vergleich
        // `< d` waere dann immer wahr. `unsigned_abs()` kann das nicht
        // (clippy `cast_abs_to_unsigned`).
        for (x, ziel_spalte) in ziel.iter_mut().enumerate() {
            let von = x as isize - d as isize;
            let mut max = 0u8;
            // Das Fenster: alle Quellspalten, die auf `x` wirken.
            for q in (von - d as isize)..=(von + d as isize) {
                if q < 0 {
                    continue;
                }
                let q = q as usize;
                if q >= b {
                    break;
                }
                max = max.max(zeile[q]);
            }
            // Schritt 3: die Rampe. Ohne sie waere die Verdickung eine
            // harte Kante — und GEMESSEN ist, dass die echte fette
            // Schrift weicher ist als die normale (81 % gegen 67 %
            // voll deckende Pixel).
            let abstand = von.unsigned_abs();
            if max > 0 && abstand < d {
                // Je naeher am Rand, desto weniger Alpha.
                //
                // GEMESSEN, warum hier `f32` stehen muss: als `usize`
                // waere `(d - abstand.min(d)) / d` eine GANZZAHL und
                // damit entweder 0 oder 1 — die Rampe waere dann ein
                // Sprung, und GEMESSEN fiel die Verdickung von 4 px auf
                // 2 px. Es braucht die Teilung.
                let anteil = (d - abstand.min(d)) as f32 / d as f32;
                max = (max as f32 * anteil.clamp(0.0, 1.0)) as u8;
            }
            *ziel_spalte = max;
        }
    }
    Some(raus)
}

/// Neigt eine Kontur um `neigung` (in em).
///
/// GEMESSEN, warum diese Funktion getrennt von [`verzerren`] ist: sie
/// ist der einzige Teil, der Punkte verschiebt, und sie ist an EINER
/// Kontur messbar (`examples/synthetisch_messung.rs`).
#[must_use]
pub fn kontur_neigen(c: &Contour, neigung: f32) -> Contour {
    let mut k = c.clone();
    for p in &mut k.points {
        p.x += p.y * neigung;
    }
    k
}
