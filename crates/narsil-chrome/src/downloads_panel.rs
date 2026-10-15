//! Release 09.10.2026 (Nutzer: „Glas billig"): DAS DOWNLOAD-PANEL als
//! schwebendes Glasfeld unter dem Download-Knopf — wie die Download-Blase in
//! Chrome bzw. das Download-Panel in Firefox.
//!
//! GEMESSEN vorher: der Knopf legte die ganze Seite `about:downloads`
//! (dunkler Vollflaechen-Grund, flache graue Knoepfe ueber die Fensterbreite)
//! ueber den Tab — die Seite darunter war weg, und das Bild wirkte wie ein
//! Fehlerzustand. Jetzt bleibt die Seite sichtbar; das Panel traegt Titel,
//! Suche, die Eintraege (Name, Stand, Fortschritt) und zwei Knoepfe.
//!
//! Geometrie und Treffer stehen hier an EINER Stelle; `narsil` zeichnet mit
//! [`display_list`] und klickt mit [`treffer`] — dieselben Rechtecke.

use super::glas;
use super::seiten::{DownloadSeite, SeitenTreffer};
use super::{Chrome, Rechteck};
use crate::downloads::{Download, DownloadZustand};
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, trf};

/// Die Breite des Panels (logische Pixel).
pub const BREITE: u32 = 380;
/// Die Hoehe einer Eintragszeile (zwei Textzeilen).
pub const ZEILE: u32 = 50;
/// M3: so viele Eintraege zeigt das Panel hoechstens.
pub const MAX_ZEILEN: usize = 6;
/// Innenabstand des Glasfelds.
const INNEN: u32 = 10;
/// Hoehe der Kopfzeile („Downloads").
const KOPF: u32 = 30;
/// Hoehe des Suchfelds.
const SUCHE: u32 = 30;
/// Hoehe der Knopfzeile.
const FUSS: u32 = 44;
/// Hoehe der Zeile „Noch nichts geladen".
const LEER: u32 = 56;

/// Die Rechtecke des Panels (Fensterpixel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometrie {
    /// Das ganze Glasfeld.
    pub rahmen: Rechteck,
    /// Das Suchfeld.
    pub suchfeld: Rechteck,
    /// Der Bereich der Eintraege.
    pub liste: Rechteck,
    /// Hoehe einer Eintragszeile.
    pub zeile: u32,
    /// Wie viele Eintragszeilen Platz haben.
    pub zeilen: usize,
    /// Der Knopf „Liste leeren".
    pub alles_loeschen: Rechteck,
    /// Der Knopf „Eintrag entfernen".
    pub eintrag_loeschen: Rechteck,
}

/// Die Geometrie fuer `anzahl` sichtbare Eintraege — `None`, wenn das Fenster
/// zu klein ist (M3/M4). Das Panel haengt rechtsbuendig unter dem
/// Download-Knopf (`rechts_symbol(3)`), wie die beiden Menues unter ihren
/// Knoepfen.
pub fn geometrie(chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32, anzahl: usize) -> Option<Geometrie> {
    let px = |v: u32| chrome.px(v);
    let knopf = chrome.rechts_symbol(3);
    let breite = px(BREITE).min(fensterbreite.saturating_sub(px(16)));
    if breite < px(200) {
        return None;
    }
    let rechts = (knopf.rechts() + px(6)).min(fensterbreite.saturating_sub(px(8))).max(breite);
    let x = rechts - breite;
    let y = chrome.seiten_oberkante().max(knopf.unten()) + px(4);
    let unten_frei = fensterhoehe.saturating_sub(super::STATUSLEISTE_HOEHE + px(8));
    let fest = px(INNEN) * 2 + px(KOPF) + px(SUCHE) + px(8) + px(FUSS);
    let zeile = px(ZEILE);
    let mut zeilen = anzahl.min(MAX_ZEILEN);
    let liste_hoehe = |n: usize| if n == 0 { px(LEER) } else { zeile * n as u32 };
    while zeilen > 0 && y + fest + liste_hoehe(zeilen) > unten_frei {
        zeilen -= 1;
    }
    if y + fest + liste_hoehe(zeilen) > unten_frei {
        return None;
    }
    let hoehe = fest + liste_hoehe(zeilen);
    let innen = px(INNEN);
    let suchfeld = Rechteck { x: x + innen, y: y + innen + px(KOPF), breite: breite - 2 * innen, hoehe: px(SUCHE) };
    let liste = Rechteck { x: x + px(4), y: suchfeld.unten() + px(8), breite: breite - px(8), hoehe: liste_hoehe(zeilen) };
    let knopf_y = liste.unten() + px(8);
    let knopf_b = (breite - 2 * innen - px(8)) / 2;
    let knopf_h = px(FUSS) - px(16);
    Some(Geometrie {
        rahmen: Rechteck { x, y, breite, hoehe },
        suchfeld,
        liste,
        zeile,
        zeilen,
        alles_loeschen: Rechteck { x: x + innen, y: knopf_y, breite: knopf_b, hoehe: knopf_h },
        eintrag_loeschen: Rechteck { x: x + innen + knopf_b + px(8), y: knopf_y, breite: knopf_b, hoehe: knopf_h },
    })
}

fn liegt_in(p: &Point, r: &Rechteck) -> bool {
    p.x >= r.x as f32 && p.x < r.rechts() as f32 && p.y >= r.y as f32 && p.y < r.unten() as f32
}

/// Liegt der Punkt im Panel? (Ein Klick daneben schliesst es.)
pub fn im_panel(g: &Geometrie, p: &Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && liegt_in(p, &g.rahmen)
}

/// Was liegt unter dem Punkt? `anzahl` = Zahl der sichtbaren Eintraege.
pub fn treffer(g: &Geometrie, anzahl: usize, p: &Point) -> Option<SeitenTreffer> {
    if !im_panel(g, p) {
        return None;
    }
    if liegt_in(p, &g.suchfeld) {
        return Some(SeitenTreffer::Suche);
    }
    if liegt_in(p, &g.liste) && g.zeile > 0 {
        let nr = ((p.y as u32) - g.liste.y) / g.zeile;
        return ((nr as usize) < anzahl.min(g.zeilen)).then_some(SeitenTreffer::Zeile(nr as usize));
    }
    if liegt_in(p, &g.alles_loeschen) {
        return Some(SeitenTreffer::AllesLoeschen);
    }
    if liegt_in(p, &g.eintrag_loeschen) {
        return Some(SeitenTreffer::EintragLoeschen);
    }
    None
}

fn text(x: f32, y: f32, t: String, farbe: Color, grad: f32, gewicht: u16) -> DisplayItem {
    DisplayItem::Text {
        schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
        deko: 0,
        origin: Point::new(x, y),
        text: t,
        color: farbe,
        font_size: grad,
        gewicht,
        kursiv: false,
    }
}

fn flaeche(r: Rect, farbe: Color, radius: f32, deckkraft: f32) -> DisplayItem {
    DisplayItem::Rect {
        rect: r,
        color: farbe,
        radius: [radius; 4],
        deckkraft,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    }
}

fn rect(r: &Rechteck) -> Rect {
    Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32)
}

/// Text auf eine Breite kuerzen (Schaetzung wie im Tab „Features"), mit „…".
fn kuerzen(t: &str, breite: u32, grad: f32) -> String {
    if super::features::textbreite(t, grad) <= breite {
        return t.to_string();
    }
    let mut s = String::new();
    for c in t.chars() {
        let probe = format!("{s}{c}…");
        if super::features::textbreite(&probe, grad) > breite {
            break;
        }
        s.push(c);
    }
    s.push('…');
    s
}

/// Die Endung des Dateinamens fuer das Symbol (hoechstens vier Zeichen).
fn endung(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() && e.len() <= 4 && e.chars().all(char::is_alphanumeric) => e.to_uppercase(),
        _ => tr("FILE").chars().take(4).collect(),
    }
}

/// Die zweite Zeile eines Eintrags: Stand in Worten.
pub fn stand_text(d: &Download) -> String {
    match d.zustand {
        DownloadZustand::Laeuft => match d.fortschritt.anteil() {
            Some(a) if d.fortschritt.bytes_gesamt > 0 => trf!(
                "{} % · {} of {} KiB",
                format!("{:.0}", a * 100.0),
                d.fortschritt.kib(),
                d.fortschritt.bytes_gesamt / 1024
            ),
            Some(a) => format!("{:.0} %", a * 100.0),
            None => tr("Loading …").to_string(),
        },
        DownloadZustand::Fertig => {
            if d.pfad.is_empty() {
                tr("Done").to_string()
            } else {
                // Nur der Name des Ordners („Downloads"), nicht der ganze
                // Pfad — der laeuft in 380 px nach drei Ordnern aus dem Bild.
                let ordner = std::path::Path::new(&d.pfad.replace('\\', "/"))
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if ordner.is_empty() { tr("Done").to_string() } else { trf!("Done · {}", ordner) }
            }
        }
        DownloadZustand::Abgebrochen => tr("Cancelled").to_string(),
        DownloadZustand::Fehlgeschlagen => {
            if d.meldung.is_empty() { tr("Failed").to_string() } else { trf!("Failed · {}", d.meldung) }
        }
    }
}

/// DAS PANEL ZEICHNEN. `suche_fokus`: im Suchfeld wird getippt.
pub fn display_list(seite: &DownloadSeite, chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32, suche_fokus: bool) -> DisplayList {
    let mut liste = DisplayList::new();
    let zeilen = seite.sichtbare_zeilen();
    let Some(g) = geometrie(chrome, fensterbreite, fensterhoehe, zeilen.len()) else {
        return liste;
    };
    let prozent = glas::prozent_von(chrome);
    let s = chrome.sk();
    let px = |v: u32| chrome.px(v);
    glas::feld(&mut liste, &g.rahmen, prozent);
    // DIE KOPFZEILE: Titel links, Anzahl rechts.
    let innen = px(INNEN);
    let titel_grad = 14.5 * s;
    liste.push(text(
        (g.rahmen.x + innen + px(2)) as f32,
        (g.rahmen.y + innen) as f32 + (px(KOPF) as f32 - titel_grad * 1.25) / 2.0,
        tr("Downloads").to_string(),
        glas::GLAS_TEXT,
        titel_grad,
        600,
    ));
    let anzahl = seite.eintraege.len();
    if anzahl > 0 {
        let zahl = if anzahl == 1 { tr("1 file").to_string() } else { trf!("{} files", anzahl) };
        let grad = 11.5 * s;
        let b = super::features::textbreite(&zahl, grad);
        liste.push(text(
            g.rahmen.rechts().saturating_sub(innen + px(2) + b) as f32,
            (g.rahmen.y + innen) as f32 + (px(KOPF) as f32 - grad * 1.25) / 2.0,
            zahl,
            glas::GLAS_GRAU,
            grad,
            400,
        ));
    }
    // DAS SUCHFELD: leicht aufgehellte Pille, im Fokus mit Akzentkante.
    let sf = rect(&g.suchfeld);
    let rund = g.suchfeld.hoehe as f32 / 2.0;
    liste.push(flaeche(sf, Color::WHITE, rund, 0.07));
    liste.push(DisplayItem::Rand {
        rect: sf,
        rahmen: narsil_core::style::Rahmen {
            breite: [s.round().max(1.0); 4],
            stil: [narsil_core::style::RahmenStil::Durchgezogen; 4],
            farbe: [if suche_fokus { Color::rgb(0x5b, 0x93, 0xe6) } else { Color::new(255, 255, 255, 34) }; 4],
        },
        radius: [rund; 4],
    });
    let such_grad = 12.5 * s;
    let leer = seite.suchtext.is_empty();
    let such_text = if leer { tr("Search downloads").to_string() } else if suche_fokus { format!("{}|", seite.suchtext) } else { seite.suchtext.clone() };
    liste.push(text(
        (g.suchfeld.x + px(14)) as f32,
        g.suchfeld.y as f32 + (g.suchfeld.hoehe as f32 - such_grad * 1.25) / 2.0,
        kuerzen(&such_text, g.suchfeld.breite.saturating_sub(px(28)), such_grad),
        if leer { glas::GLAS_GRAU } else { glas::GLAS_TEXT },
        such_grad,
        400,
    ));
    // DIE EINTRAEGE.
    if zeilen.is_empty() || g.zeilen == 0 {
        let grad = 13.0 * s;
        let t = if seite.eintraege.is_empty() { tr("Nothing downloaded yet") } else { tr("No download matches the search") };
        let b = super::features::textbreite(t, grad);
        liste.push(text(
            (g.liste.x + g.liste.breite.saturating_sub(b) / 2) as f32,
            g.liste.y as f32 + (g.liste.hoehe as f32 - grad * 1.25) / 2.0,
            t.to_string(),
            glas::GLAS_GRAU,
            grad,
            400,
        ));
    }
    for (nr, d) in zeilen.iter().take(g.zeilen).enumerate() {
        let y = g.liste.y + g.zeile * nr as u32;
        let zeile_r = Rect::new(g.liste.x as f32, y as f32, g.liste.breite as f32, g.zeile as f32);
        let gewaehlt = seite.ausgewaehlt.is_some_and(|i| seite.eintraege.get(i).is_some_and(|e| std::ptr::eq(e, *d)));
        if gewaehlt {
            liste.push(glas::hell(zeile_r, [8.0 * s; 4], 0.13));
        }
        // Das Symbol: ein runder Kasten mit der Endung.
        let kante = px(32);
        let sx = g.liste.x + px(8);
        let sy = y + (g.zeile.saturating_sub(kante)) / 2;
        let farbe = match d.zustand {
            DownloadZustand::Fehlgeschlagen | DownloadZustand::Abgebrochen => Color::rgb(0xc8, 0x55, 0x5a),
            DownloadZustand::Laeuft => Color::rgb(0x3f, 0x7b, 0xd0),
            DownloadZustand::Fertig => Color::rgb(0x4a, 0x50, 0x62),
        };
        liste.push(flaeche(Rect::new(sx as f32, sy as f32, kante as f32, kante as f32), farbe, 8.0 * s, 1.0));
        let e = endung(&d.name);
        let e_grad = if e.chars().count() > 3 { 8.5 * s } else { 9.5 * s };
        let eb = super::features::textbreite(&e, e_grad);
        liste.push(text(
            (sx + kante.saturating_sub(eb) / 2) as f32,
            sy as f32 + (kante as f32 - e_grad * 1.25) / 2.0,
            e,
            Color::WHITE,
            e_grad,
            700,
        ));
        // Name und Stand.
        let tx = sx + kante + px(10);
        let platz = g.liste.rechts().saturating_sub(tx + px(10));
        let name_grad = 13.0 * s;
        liste.push(text(
            tx as f32,
            y as f32 + px(7) as f32,
            kuerzen(&d.name, platz, name_grad),
            glas::GLAS_TEXT,
            name_grad,
            500,
        ));
        let stand_grad = 11.0 * s;
        let stand_farbe = match d.zustand {
            DownloadZustand::Fehlgeschlagen => Color::rgb(0xff, 0x8a, 0x8a),
            _ => glas::GLAS_GRAU,
        };
        liste.push(text(
            tx as f32,
            y as f32 + px(27) as f32,
            kuerzen(&stand_text(d), platz, stand_grad),
            stand_farbe,
            stand_grad,
            400,
        ));
        // Der Fortschritt eines laufenden Downloads: ein duenner Balken.
        if d.zustand == DownloadZustand::Laeuft {
            let anteil = d.fortschritt.anteil().unwrap_or(0.0).clamp(0.0, 1.0);
            let by = y + g.zeile - px(7);
            let bb = platz as f32;
            liste.push(flaeche(Rect::new(tx as f32, by as f32, bb, 3.0 * s), Color::WHITE, 1.5 * s, 0.12));
            liste.push(flaeche(Rect::new(tx as f32, by as f32, (bb * anteil).max(3.0 * s), 3.0 * s), Color::rgb(0x5b, 0x93, 0xe6), 1.5 * s, 1.0));
        }
    }
    // DIE TRENNLINIE und die zwei Knoepfe.
    let linie_y = g.liste.unten() + px(3);
    liste.push(flaeche(
        Rect::new((g.rahmen.x + innen) as f32, linie_y as f32, g.rahmen.breite.saturating_sub(2 * innen) as f32, s.round().max(1.0)),
        Color::WHITE,
        0.0,
        glas::GLAS_LINIE,
    ));
    let knopf_grad = 12.5 * s;
    for (r, beschriftung, aktiv) in [
        (&g.alles_loeschen, tr("Clear list"), !seite.eintraege.is_empty()),
        (&g.eintrag_loeschen, tr("Remove entry"), seite.ausgewaehlt.is_some()),
    ] {
        let kr = rect(r);
        liste.push(flaeche(kr, Color::WHITE, r.hoehe as f32 / 2.0, if aktiv { 0.09 } else { 0.04 }));
        let b = super::features::textbreite(beschriftung, knopf_grad);
        liste.push(text(
            (r.x + r.breite.saturating_sub(b) / 2) as f32,
            r.y as f32 + (r.hoehe as f32 - knopf_grad * 1.25) / 2.0,
            beschriftung.to_string(),
            if aktiv { glas::GLAS_TEXT } else { glas::GLAS_AUS },
            knopf_grad,
            500,
        ));
    }
    liste
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eintrag(name: &str, zustand: DownloadZustand) -> Download {
        let mut d = Download::aus_adresse(&format!("https://beispiel.de/{name}"));
        d.zustand = zustand;
        d
    }

    /// Das Panel haengt unter dem Download-Knopf, ist schmal, und Bild und
    /// Treffer nutzen dieselben Rechtecke.
    #[test]
    fn das_panel_haengt_unter_dem_knopf_und_trifft_was_es_zeigt() {
        let chrome = Chrome::neu(1280);
        let mut seite = DownloadSeite::default();
        seite.eintraege.push(eintrag("bericht.pdf", DownloadZustand::Fertig));
        seite.eintraege.push(eintrag("film.mp4", DownloadZustand::Laeuft));
        let g = geometrie(&chrome, 1280, 800, seite.sichtbare_zeilen().len()).expect("Geometrie");
        let knopf = chrome.rechts_symbol(3);
        assert!(g.rahmen.y >= knopf.unten(), "unter dem Knopf: {g:?}");
        assert!(g.rahmen.rechts() >= knopf.rechts() && g.rahmen.rechts() <= 1280, "rechtsbuendig: {g:?}");
        assert!(g.rahmen.breite <= chrome.px(BREITE), "schmal, keine Vollflaeche");
        assert_eq!(g.zeilen, 2);
        // Treffer: Suche, Zeilen, Knoepfe, daneben nichts.
        let mitte = |r: &Rechteck| Point::new((r.x + r.breite / 2) as f32, (r.y + r.hoehe / 2) as f32);
        assert_eq!(treffer(&g, 2, &mitte(&g.suchfeld)), Some(SeitenTreffer::Suche));
        let z1 = Point::new((g.liste.x + 40) as f32, (g.liste.y + g.zeile + 5) as f32);
        assert_eq!(treffer(&g, 2, &z1), Some(SeitenTreffer::Zeile(1)));
        assert_eq!(treffer(&g, 2, &mitte(&g.alles_loeschen)), Some(SeitenTreffer::AllesLoeschen));
        assert_eq!(treffer(&g, 2, &mitte(&g.eintrag_loeschen)), Some(SeitenTreffer::EintragLoeschen));
        assert!(!im_panel(&g, &Point::new(100.0, 400.0)), "links daneben liegt die Seite");
        // Das Bild: ein Glasfeld (Effekt), Titel, Namen, Stand, Knoepfe.
        let l = display_list(&seite, &chrome, 1280, 800, false);
        assert!(l.effekte().iter().any(|(_, _, e)| matches!(e, narsil_display::Effekt::Glas(_))), "Glasfeld");
        let texte: Vec<String> = l
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for t in ["Downloads", "2 files", "bericht.pdf", "film.mp4", "PDF", "Clear list", "Remove entry"] {
            assert!(texte.iter().any(|x| x == t), "{t} fehlt: {texte:?}");
        }
        // Keine Flaeche ueber die ganze Fensterbreite (der alte Vollgrund).
        assert!(
            !l.iter().any(|i| matches!(i, DisplayItem::Rect { rect, .. } if rect.width >= 1000.0)),
            "kein Vollflaechen-Grund"
        );
    }

    /// Der Stand nennt den Ordner, nicht den ganzen Pfad.
    #[test]
    fn der_stand_nennt_den_ordner() {
        let mut d = eintrag("a.pdf", DownloadZustand::Fertig);
        d.pfad = r"C:\Users\Jeff\Downloads\a.pdf".into();
        assert_eq!(stand_text(&d), "Done · Downloads");
        d.pfad = "/home/jaff/Downloads/a.pdf".into();
        assert_eq!(stand_text(&d), "Done · Downloads");
        d.zustand = DownloadZustand::Fehlgeschlagen;
        d.meldung = "Netz weg".into();
        assert_eq!(stand_text(&d), "Failed · Netz weg");
    }

    /// Ohne Eintraege: ein ruhiger Satz statt einer leeren Liste.
    #[test]
    fn leer_sagt_es_einen_satz() {
        let chrome = Chrome::neu(1000);
        let seite = DownloadSeite::default();
        let l = display_list(&seite, &chrome, 1000, 700, false);
        assert!(l.iter().any(|i| matches!(i, DisplayItem::Text { text, .. } if text == "Nothing downloaded yet")));
        assert!(geometrie(&chrome, 150, 120, 0).is_none(), "zu kleines Fenster");
    }
}
