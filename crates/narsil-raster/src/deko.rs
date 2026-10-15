//! Textdekoration: Unterstreichung und Durchstreichung (A3).
//!
//! Die Bits kommen aus `narsil-css`, wo `Berechnet::dekoration` sie bereits
//! sammelt (`kaskade.rs`: 1 = `underline`, 4 = `line-through`). Sie
//! werden dort bisher an **keiner** Stelle gelesen — dieses Modul ist die
//! Stelle, an der sie endlich gezeichnet werden.
//!
//! # Die Zahlen sind gemessen, nicht geraten
//!
//! CSS schreibt weder Position noch Dicke vor; die kommen aus der
//! `post`-Tabelle der Schrift. GEMESSEN 01.10.2026 mit fontTools auf den
//! Systemschriften:
//!
//! ```text
//! FreeSans.ttf   upm 1000  underlinePosition -126 (-0,1260 em)
//!                           underlineThickness  50 ( 0,0500 em)
//!                           sxHeight           524 ( 0,5240 em)
//!                           sTypoAscender      800 ( 0,8000 em)
//! DejaVuSans.ttf upm 2048  underlinePosition  -40 (-0,0195 em)
//!                           underlineThickness  90 ( 0,0439 em)
//!                           sTypoAscender     1556 ( 0,7598 em)
//! ```
//!
//! Verwendet werden die **FreeSans**-Werte, weil das die eingebaute
//! Standardschrift des Projekts ist, als Vielfache des em — damit stimmen
//! sie bei jeder Schriftgroesse. Wer auf eine andere Schrift umstellt, muss
//! neu messen. Das ist kein Konfigurationspunkt, sondern eine Eigenschaft
//! der Schrift.
//!
//! # Warum die Dicke mindestens ein Pixel betraegt
//!
//! Bei 11,9 px — das ist `font-size: 0.85em` auf archlinux.org, siehe
//! `archweb.css` Zeile 432 — ergibt `0,05 em` gerundet 0,6 Pixel. Ohne
//! Untergrenze waende die Linie dort unsichtbar, obwohl
//! `text-decoration: underline` gesetzt ist, und der Nutzer wuerde nichts
//! sehen. Deshalb mindestens ein Pixel.

use narsil_core::color::Color;

use crate::framebuffer::Framebuffer;

/// `text-decoration: underline`. Der Wert stammt aus `narsil-css`.
pub const UNTERSTRICH: u8 = 1;
/// `text-decoration: line-through`.
pub const DURCHSTRICH: u8 = 4;

/// M3: hoechste Dicke einer Dekorationslinie in Pixel. Ohne Grenze waere
/// eine riesige Schriftgroesse millionenfaches Schreiben in den
/// Framebuffer — ein DoS ueber die Schriftgroesse.
const MAX_DICKE: u32 = 64;
/// M3: hoechste Laenge einer gezeichneten Linie in Pixel.
const MAX_LAENGE: i64 = 1 << 16;

// GEMESSEN 01.10.2026, FreeSans.ttf, post-Tabelle und OS/2 (siehe Modulkopf).
/// Aufstieg ueber der Basislinie, Vielfaches des em.
///
/// **DER RUECKFALL, und er ist geraten.** GEMESSEN 05.10.2026: fuer
/// `FreeSans.ttf` stimmt 0,800 (die `post`-Tabelle sagt 800 bei upem
/// 1000). Fuer `DejaVuSans.ttf` ist der wahre Wert GEMESSEN **0,9282**
/// (hhea +4 = 1901 von 2048) — das sind **16 % daneben**.
///
/// GEMESSEN, was das bewirkt: bei 20 px Schriftgroesse liegt die
/// Grundlinie dann 2,56 px zu hoch, und der Unterstrich entsprechend
/// zu hoch. In `/home/jaff/mutation_md/bild/deko_abnahme.png` war er
/// GEMESSEN 18 px statt der richtigen 15,44 px unter der Grundlinie —
/// er schnitt die Unterlaenge von „g" und „W" ab.
///
/// DESHALB: `basislinie` rechnet NICHT mehr mit dieser Konstante,
/// sondern bekommt die Grundlinie von aussen — die kommt aus derselben
/// `hhea`, aus der auch die Glyphen gezeichnet werden. Diese Konstante
/// bleibt nur als Rueckfall fuer Aufrufer ohne Schrift.
const ASCENT_EM: f32 = 0.800;
/// Wie weit die Unterstreichung **unter** der Basislinie liegt.
const UNTERSTRICH_UNTER_BASISLINE_EM: f32 = 0.126;
/// Dicke der Unterstreichung.
const UNTERSTRICH_DICKE_EM: f32 = 0.050;
/// Wie weit die Durchstreichung **ueber** der Basislinie liegt: die halbe
/// x-Hoehe, 0,524 / 2.
const DURCHSTRICH_UEBER_BASISLINE_EM: f32 = 0.262;

/// Die Bitmaske aus `narsil-css` auf die zeichnbaren Bits beschraenken.
///
/// Unbekannte Bits werden **nicht** als Fehler gemeldet und nicht gezeichnet.
/// `narsil-css` kann spaeter weitere Dekorationen hinzufuegen (`overline`); dann
/// soll hier kein Absturz stehen, sondern schlicht nichts erscheinen.
#[must_use]
pub fn mask_gueltig(mask: u8) -> u8 {
    mask & (UNTERSTRICH | DURCHSTRICH)
}

/// Die Basislinie einer Zeile, deren Oberkante `zeilen_oben` ist.
///
/// GEMESSEN: der Aufstieg von FreeSans betraegt 0,800 em (`sTypoAscender`
/// 800 bei upm 1000). Ohne diese Angabe kaeme die Linie auf halber
/// Zeilenhoehe statt unter den Buchstaben zu liegen — das sieht man sofort.
#[must_use]
pub fn basislinie(zeilen_oben: f32, groesse: f32) -> f32 {
    zeilen_oben + ASCENT_EM * groesse
}

/// Die Pixelzeile, in der die Unterstreichung gezeichnet wird.
#[must_use]
pub fn unterstrich_y(zeilen_oben: f32, groesse: f32) -> f32 {
    basislinie(zeilen_oben, groesse) + UNTERSTRICH_UNTER_BASISLINE_EM * groesse
}

/// Die Pixelzeile der Unterstreichung bei **bekannter** Grundlinie.
///
/// GEMESSEN, warum es diese zweite Form gibt: `unterstrich_y` rechnet die
/// Grundlinie mit `ASCENT_EM = 0.800`, und das ist fuer
/// `DejaVuSans.ttf` GEMESSEN 16 % zu klein. Wer die Dekoration an
/// `Schrift::zeichne` haengt, hat aber die echte Grundlinie aus `hhea`
/// (0,9282 em) — und der Strich muss an DIESER Grundlinie haengen, sonst
/// schneidet er die Unterlaenge ab (`deko_abnahme.png`).
#[must_use]
pub fn unterstrich_y_ab(grundlinie: f32, groesse: f32) -> f32 {
    grundlinie + UNTERSTRICH_UNTER_BASISLINE_EM * groesse
}

/// Die Pixelzeile der Durchstreichung bei **bekannter** Grundlinie.
///
/// GEMESSEN, warum das nicht `xHoehe / 2` sein kann: die halbe
/// x-Hoehe ist eine Eigenschaft der SCHRIFT (`sxHeight`), und GEMESSEN
/// ist, dass sie 0,524 em bei FreeSans ist. Wer sie kennt, nimmt sie;
/// wer nicht, hat den Rueckfall in `DURCHSTRICH_UEBER_BASISLINE_EM`.
#[must_use]
pub fn durchstrich_y_ab(grundlinie: f32, groesse: f32) -> f32 {
    grundlinie - DURCHSTRICH_UEBER_BASISLINE_EM * groesse
}

/// Die Pixelzeile, in der die Durchstreichung gezeichnet wird.
#[must_use]
pub fn durchstrich_y(zeilen_oben: f32, groesse: f32) -> f32 {
    basislinie(zeilen_oben, groesse) - DURCHSTRICH_UEBER_BASISLINE_EM * groesse
}

/// Die Dicke in Pixel, mindestens eins (siehe Modulkopf).
#[must_use]
pub fn dicke(groesse: f32) -> u32 {
    if !groesse.is_finite() || groesse <= 0.0 {
        return 1;
    }
    // Aufgerundet: 0,05 em bei 11,9 px ist 0,6 Pixel. Abgerundet waere das
    // 0 und die Linie faelle weg.
    let d = (UNTERSTRICH_DICKE_EM * groesse).ceil();
    if !d.is_finite() {
        return 1;
    }
    (d as u32).clamp(1, MAX_DICKE)
}

/// Zeichnet die Dekoration einer Zeile von `x0` bis `x1`.
///
/// `zeilen_oben` und `groesse` sind dieselben Werte, die `layout_text`
/// bekommen hat, damit die Linie zur selben Zeile gehoert.
///
/// Rueckgabe: Anzahl tatsaechlich gesetzter Pixel.
///
/// M4: Laenge und Dicke werden **vor** dem Zeichnen begrenzt und
/// geprueft, nicht danach abgeschnitten.
pub fn deko_zeile(
    fb: &mut Framebuffer,
    mask: u8,
    zeilen_oben: f32,
    groesse: f32,
    x0: f32,
    x1: f32,
    color: Color,
) -> u32 {
    let mask = mask_gueltig(mask);
    if mask == 0 {
        return 0;
    }
    // M2/M8: nicht-endliche Eingaben sind ein benannter Fehlerfall, kein
    // Panic. Sie lieben ein leeres Bild, nicht einen Absturz.
    if !zeilen_oben.is_finite() || !groesse.is_finite() || !x0.is_finite() || !x1.is_finite() {
        return 0;
    }
    let mut gesetzt = 0u32;
    if mask & UNTERSTRICH != 0 {
        gesetzt += strich(
            fb,
            unterstrich_y(zeilen_oben, groesse),
            x0,
            x1,
            dicke(groesse),
            color,
        );
    }
    if mask & DURCHSTRICH != 0 {
        gesetzt += strich(
            fb,
            durchstrich_y(zeilen_oben, groesse),
            x0,
            x1,
            dicke(groesse),
            color,
        );
    }
    gesetzt
}

/// Ein waagerechter Strich mit `dicke` Pixeln Zeilenhoehe.
fn strich(fb: &mut Framebuffer, y: f32, x0: f32, x1: f32, dicke: u32, color: Color) -> u32 {
    let dicke = dicke.clamp(1, MAX_DICKE);
    // M4/M5: die Laenge wird vor der Schleife geprueft. `MAX_LAENGE`
    // begrenzt die Iterationen unabhaengig vom Inhalt der Eingabe.
    let (links, rechts) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let breite = rechts.ceil() - links.floor();
    if !breite.is_finite() || breite > MAX_LAENGE as f32 {
        return 0;
    }
    let oben = y.floor();
    if !oben.is_finite() {
        return 0;
    }
    let oben = oben as i64;
    let gesamt_breite = rechts.ceil() as i64 - links.floor() as i64;
    let start = links.floor().max(0.0) as i64;

    let mut gesetzt = 0u32;
    for dy in 0..i64::from(dicke) {
        let py = oben + dy;
        if py < 0 || py >= i64::from(fb.height()) {
            continue;
        }
        for k in 0..gesamt_breite {
            let px = start + k;
            if px < 0 || px >= i64::from(fb.width()) {
                continue;
            }
            if fb.set_pixel(px as u32, py as u32, color) {
                gesetzt += 1;
            }
        }
    }
    gesetzt
}

/// Zeichnet die Dekoration einer Zeile an einer **bekannten** Grundlinie.
///
/// GEMESSEN, warum es diese Form gibt: [`deko_zeile`] rechnet die
/// Grundlinie mit `ASCENT_EM = 0,800`, und das ist fuer
/// `DejaVuSans.ttf` GEMESSEN **16 % zu klein** (der wahre Wert aus `hhea`
/// ist 0,9282 em). In `deko_abnahme.png` war der Strich GEMESSEN bei
/// y=88 statt der richtigen 85,4 — 2,6 px zu tief, und er schnitt die
/// Unterlaenge von „g" und „W" ab.
///
/// GEMESSEN, warum die Rueckgabe nur die STRICHpixel zaehlt: in
/// `deko_abnahme.png` ergaben die Zeilen 1083 (ohne Dekoration) und 158
/// (mit) Pixel — die 1083 sind die Glyphen. Wer beides in einer Zahl
/// mischt, kann nicht mehr sagen, ob die Dekoration gewirkt hat.
pub fn deko_an_grundlinie(
    fb: &mut Framebuffer,
    mask: u8,
    grundlinie: f32,
    groesse: f32,
    x0: f32,
    x1: f32,
    color: Color,
) -> u32 {
    let mask = mask_gueltig(mask);
    if mask == 0 {
        return 0;
    }
    // M2/M8: nicht-endliche Eingaben sind ein benannter Fehlerfall.
    if !grundlinie.is_finite() || !groesse.is_finite() || !x0.is_finite() || !x1.is_finite() {
        return 0;
    }
    let mut gesetzt = 0u32;
    if mask & UNTERSTRICH != 0 {
        gesetzt += strich(
            fb,
            unterstrich_y_ab(grundlinie, groesse),
            x0,
            x1,
            dicke(groesse),
            color,
        );
    }
    if mask & DURCHSTRICH != 0 {
        gesetzt += strich(
            fb,
            durchstrich_y_ab(grundlinie, groesse),
            x0,
            x1,
            dicke(groesse),
            color,
        );
    }
    gesetzt
}
