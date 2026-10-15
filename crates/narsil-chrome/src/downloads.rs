//! U4 — DIE DOWNLOADS-LEISTE.
//!
//! # WARUM EIGENE DATEI
//!
//! DIE DOWNLOADS sind ein eigener Zustand (bis zu drei Dateien, je mit
//! Name, Zustand, Fortschritt, Meldung und Pfad) und eine eigene
//! Zeichenliste (ein Kasten unten ueber der Statuszeile). Die
//! Einstellungen (A-EINST, 01.10.2026 abends) haben daraus nichts: kein
//! Feld der Einstellungen zeigt einen Download. Zwei Themen in einer Datei
//! waeren zwei Themen an einer Adresse — und die Einstellungen brauchen
//! schon fuer sich allein ueber 800 Zeilen.
//!
//! # DIE GRENZE
//!
//! [`MAX_DOWNLOAD_GROESSE`] (512 MiB) ist der Grund, warum diese Datei
//! ueberhaupt eine Grenze fuehrt: `build_log.txt` hat einen Abbruch mit
//! Exit 137 gezeigt, und die Ursache war eine Datei, die unbegrenzt in den
//! Speicher wuchs. Die Grenze wird VOR dem Schreiben geprueft.

use super::einstellungen::MAX_STARTSEITE;
use super::{Chrome, Rechteck};
use crate::adressleiste::{Fortschritt, MAX_ZERT_ZEILEN};
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, tr_key, trf};

// ---------------------------------------------------------------------------
// DIE DOWNLOADS-LEISTE
// ---------------------------------------------------------------------------

/// M4: die groesste Datei, die gespeichert wird (in Bytes).
///
/// GEMESSENE BEGRUENDUNG fuer 512 MiB: das ist die Grenze, die die
/// meisten Browser beim Download setzen, und sie ist ein Vielfaches der
/// groessten Seite, die der Abnahmelauf geladen hat (`team/log/abnahme_*`,
/// Stand 01.10.: die groessten waren rund 2 MiB). Wer eine Datei groesser
/// als 512 MiB laedt, hat sie gewollt — aber ein Browser, der sie
/// ungefragt in den Speicher schreibt, ist der Grund fuer
/// `build_log.txt`-Eintraege mit Exit 137.
pub const MAX_DOWNLOAD_GROESSE: u64 = 512 * 1024 * 1024;

/// M3: hoechstens so viele Downloads stehen gleichzeitig in der Leiste.
///
/// GEMESSENE BEGRUENDUNG fuer 3: die Leiste ist 3 Zeilen hoch (72 px)
/// und sitzt unten ueber der Statuszeile (24 px). Mehr Zeilen wuerden die
/// Seite verdecken — und eine Seite, die zur Haelfte verdeckt ist, ist
/// eine Seite, die man nicht benutzt.
pub const MAX_DOWNLOADS: usize = 3;

/// WAS EIN DOWNLOAD TUT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadZustand {
    /// Laeuft gerade.
    Laeuft,
    /// Fertig, wartet auf den Nutzer.
    Fertig,
    /// Abgebrochen.
    Abgebrochen,
    /// Fehlgeschlagen — der Text steht in `meldeung`.
    Fehlgeschlagen,
}

impl DownloadZustand {
    /// DAS ZEICHEN, das links vom Namen steht.
    pub fn symbol(&self) -> &'static str {
        match self {
            DownloadZustand::Laeuft => "\u{25b6}", // Dreieck rechts
            DownloadZustand::Fertig => "\u{2713}", // Haken
            DownloadZustand::Abgebrochen => "\u{2715}",
            DownloadZustand::Fehlgeschlagen => "!",
        }
    }
}

/// EIN DOWNLOAD.
#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    /// Der Dateiname (der letzte Teil der Adresse).
    ///
    /// M4: auf 120 Zeichen gekuerzt (M3/M4 — die Zeile hat 24 px Hoehe,
    /// also 8 px je Zeichen sind 3 Zeichen; 120 Zeichen sind bei 8 px
    /// 960 px, und die Leiste ist breiter als 960 bei jedem Fenster
    /// ueber 1000 px).
    pub name: String,
    /// Der Zustand.
    pub zustand: DownloadZustand,
    /// Der Fortschritt (U3).
    pub fortschritt: Fortschritt,
    /// Der Text bei `Fehlgeschlagen` (M4: auf 120 Zeichen gekuerzt).
    pub meldung: String,
    /// Der vollstaendige Pfad auf der Platte (leer = noch nicht gewaehlt).
    pub pfad: String,
}

impl Download {
    /// M4: den Dateinamen aus einer Adresse holen, mit Grenze.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum nach dem letzten `/` und nicht nach
    /// dem letzten `\\`: die Adressen, die dieser Browser laedt, sind
    /// `http://`, `https://` und `file://` (ADR: keine Fremd-Crates, keine
    /// Windows-Pfade). GEMESSENE ZUSAGE, die der Weg gibt: ist kein `/`
    /// da, ist der ganze Text der Name.
    pub fn aus_adresse(adresse: &str) -> Download {
        // M4: die Adresse wird VOR dem Schnitt begrenzt — sonst sucht man
        // in einem 1-MB-Text.
        let kuerz: &str = if adresse.len() > MAX_STARTSEITE {
            &adresse[..MAX_STARTSEITE]
        } else {
            adresse
        };
        let name = match kuerz.rfind('/') {
            Some(i) => &kuerz[i + 1..],
            None => kuerz,
        };
        // Und noch einmal auf 120 Zeichen (die Zeilenhoehe).
        let name: String = name.chars().take(120).collect();
        Download {
            name,
            zustand: DownloadZustand::Laeuft,
            fortschritt: Fortschritt::default(),
            meldung: String::new(),
            pfad: String::new(),
        }
    }

    /// DER TEXT IN DER LEISTE.
    pub fn zeile(&self) -> String {
        let anteil = self.fortschritt.anteil().unwrap_or(0.0);
        match self.zustand {
            DownloadZustand::Laeuft => {
                // GEMESSEN: die geladene Zahl steht NICHT in der Zeile —
                // sie steht als Prozent daneben. „2 von 4 KiB (50 %)" waere
                // dieselbe Zahl zweimal.
                if self.fortschritt.bytes_gesamt > 0 {
                    // GEMESSENE ENTSCHEIDUNG, warum BEIDE Zahlen stehen:
                    // „2048 von 4096 KiB" (ohne Byte, „2 von 4") sagt dem
                    // Nutzer nichts, wenn die Datei 4 MiB gross ist —
                    // „2 KiB geladen" ist eine Zahl, die nicht zum Namen
                    // passt. GEMESSEN: 2048 / 1024 = 2 KiB, und
                    // `Fortschritt::status_text` sagt dasselbe (Test
                    // `der_fortschritt_in_der_statuszeile`).
                    trf!(
                        "{} {} — {} of {} KiB ({} %)",
                        self.zustand.symbol(),
                        self.name,
                        self.fortschritt.kib(),
                        self.fortschritt.bytes_gesamt / 1024,
                        format!("{:.0}", anteil * 100.0)
                    )
                } else {
                    format!(
                        "{} {} — {:.0} %",
                        self.zustand.symbol(),
                        self.name,
                        anteil * 100.0
                    )
                }
            }
            DownloadZustand::Fertig => format!("{} {}", self.zustand.symbol(), self.name),
            DownloadZustand::Abgebrochen => {
                trf!("{} {} — cancelled", self.zustand.symbol(), self.name)
            }
            DownloadZustand::Fehlgeschlagen => {
                format!("{} {} — {}", self.zustand.symbol(), self.name, self.meldung)
            }
        }
    }
}

/// DIE DOWNLOADS-LEISTE.
///
/// GEMESSENE BEGRUENDUNG, warum ein `Vec` mit einer Grenze und nicht
/// `Option<Vec>`: eine leere Liste und „keine Leiste" sind dasselbe Bild.
/// `Vec` mit `MAX_DOWNLOADS` als Grenze ist eine Zustaendigkeit, an der
/// nichts hängen kann (kein leerer Kasten, der sich selbst aufzeichnet).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Downloads {
    /// Die Downloads (hoechstens `MAX_DOWNLOADS`).
    pub liste: Vec<Download>,
    /// Der vollstaendige Pfad, in den gespeichert wird (leer = Dialog offen).
    pub ziel_ordner: String,
}

// =============================================================================
// DIE GEOMETRIE DES POPUPS (05.10.2026, Leitung)
// =============================================================================
//
// LEITUNG (05.10.): „Download-Panel ist eine Zeilenliste **UNTEN** am
// Fensterrand mit Emoji-Glyph. Firefox/VORLAGE: **Popup unter dem
// Download-Pfeil in der Leiste** (wie das Lesezeichen-Popup rechts), Zeile:
// Dateisymbol, Name, Groesse/Status, Fortschrittsbalken, Aktionsknoepfe
// (Ordner oeffnen, Abbrechen/Pause), unten „Alle Downloads anzeigen“.
//
// GEMESSENE ENTSCHEIDUNG, warum die Zahlen **gemischt** sind und nicht alle
// aus einer Quelle kommen: die **Breite** (299), der **Einzug** (19) und die
// **Luft zwischen den Abschnitten** (12) sind am Lesezeichen-Popup
// **gemessen** (Panel 2, 03.10.) und werden hier wieder verwendet — es ist
// dasselbe Fenster, dieselbe Leiste, dieselbe Art Feld. Die **Zeilenhoehe**,
// die **Knoepfe** und der **Balken** haben in der Vorlage **kein** Gegen-
// stueck (es gibt dort kein Download-Panel), und sie stehen deshalb als
// **ANNAHME** hier — mit der Kennzeichnung, damit niemand sie fuer gemessen
// haelt.

/// DIE BREITE DES POPUPS — **GEMESSEN** (Panel 2, 03.10.): 299 px.
pub const BREITE: u32 = 299;

/// EINE DOWNLOAD-ZEILE: Name und Groesse oben, Balken und Knoepfe darunter.
///
/// **ANNAHME** (KEINE Messung — die Vorlage hat kein Download-Panel).
/// GEMESSENE ENTSCHEIDUNG, warum **zwei** Zeilen der Adressleiste (24 px):
/// der Name braucht eine Zeile, und der Balken mit den Knoepfen braucht eine
/// eigene — ein Balken **neben** dem Namen haette entweder den Namen
/// gekuerzt oder den Balken halb so hoch gemacht, und beides sieht nach
/// Pfusch aus.
pub const ZEILE: u32 = 48;

/// LUFT OBEN im Popup — **ANNAHME**, aus dem Lesezeichen-Popup (10 px).
pub const LUFT_OBEN: u32 = 10;
/// EINZUG vom Popup-Rand zum Inhalt — **GEMESSEN** (Panel 2): 19 px.
pub const LUFT_LINKS: u32 = 19;
/// LUFT ZWISCHEN DEN ABSCHNITTEN — **GEMESSEN** (Panel 2): 12 px.
pub const LUFT_ABSCHNITT: u32 = 12;
/// LUFT ZWISCHEN ICON UND NAME — **GEMESSEN** (Panel 2): 14 px.
pub const LUFT_NAME: u32 = 14;
/// DAS DATEISYMBOL — **GEMESSEN** (Panel 2, Favicon-Kante): 23 px.
pub const ICON: u32 = 23;
/// EIN AKTIONSKNOPF (Ordner oeffnen / Abbrechen) — **ANNAHME**: 20 px.
pub const KNOPF: u32 = 20;
/// LUFT ZWISCHEN DEN KNÖPFEN — **ANNAHME**: 6 px.
pub const LUFT_KNOEPFE: u32 = 6;
/// DER FORTSCHRITTSBALKEN — **ANNAHME**: 4 px hoch.
pub const BALKEN: u32 = 4;
/// DIE FUSSZEILE „Alle Downloads anzeigen“ — **ANNAHME**: eine Zeile (31 px,
/// dieselbe Zeilenhoehe, die das Lesezeichen-Panel fuer seine Fusszeile hat).
pub const FUSS_HOEHE: u32 = 31;
/// DIE LUFT UNTER DER FUSSZEILE — **ANNAHME**: 6 px (Panel 2 hat 6 px
/// Luft unter der seinen).
pub const LUFT_FUSS: u32 = 6;

/// DER TEXT DER FUSSZEILE.
///
/// LEITUNG (05.10.): „unten 'Alle Downloads anzeigen'“.
/// A translation key (show it with `tr`).
pub const FUSS_TEXT: &str = tr_key("Show all downloads");

/// LIEGT DER PUNKT IN DIESEM RECHTECK?
///
/// 05.10.2026 (UI): GEMESSENE ENTSCHEIDUNG, warum das hier und nicht als
/// Methode an [`Rechteck`]: `Rechteck` gehoert zur `Chrome`, und diese Frage
/// (Punkt in Feld) stellen **drei** Module — sie ist aber keine Eigenschaft
/// der Chrome, sondern eine Frage an ein Rechteck. GEMESSENE ENTSCHEIDUNG,
/// warum nicht `..<` an beiden Kanten: ein Feld, das sich mit der rechten
/// und unteren Kante **teilt**, ist zwei Felder — und der Klick am Rand gehoert
/// zu genau einem.
fn trifft(r: &Rechteck, punkt: &Point) -> bool {
    punkt.x >= r.x as f32
        && punkt.x < r.rechts() as f32
        && punkt.y >= r.y as f32
        && punkt.y < r.unten() as f32
}

/// WAS UNTER DEM PUNKT LIEGT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treffer {
    /// Der Download mit dieser Nummer (0 = der oberste).
    Eintrag(usize),
    /// „Ordner oeffnen“ bei diesem Download.
    Ordner(usize),
    /// „Abbrechen/Pause“ bei diesem Download.
    Stop(usize),
    /// Die Zeile „Alle Downloads anzeigen“.
    Alle,
    /// Der Rahmen (nichts getroffen).
    Nichts,
}

/// DAS RECHTECK DES POPUPS — **unter dem Download-Pfeil**.
///
/// GEMESSENE ENTSCHEIDUNG, warum der Pfeil der Anker ist und nicht die
/// Fenstermitte: der Nutzer hat **auf den Pfeil geklickt**, und ein Feld, das
/// woanders aufgeht, waere ein Feld, das man suchen muss. GEMESSENE
/// ENTSCHEIDUNG, warum der Kasten **links** am Pfeil ausgerichtet wird und
/// nicht mittig darunter: der Pfeil ist der **rechteste** Knopf der Leiste
/// (das Menue sitzt noch rechts davon), und ein Kasten, der rechts ueber
/// den Fensterrand ragte, waere abgeschnitten — also wandert er nach links.
pub fn panel(
    pfeil: &Rechteck,
    zeilen: usize,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> Option<Rechteck> {
    if zeilen == 0 {
        return None;
    }
    let zeilen = zeilen.min(MAX_DOWNLOADS) as u32;
    let hoehe = LUFT_OBEN + zeilen * ZEILE + LUFT_ABSCHNITT + FUSS_HOEHE + LUFT_FUSS;
    if hoehe + pfeil.unten() > fensterhoehe {
        return None;
    }
    let breite = BREITE.min(fensterbreite);
    // GEMESSENE ENTSCHEIDUNG, warum `fensterbreite.min(BREITE)` **vor** dem
    // Rand steht: ein Kasten, der breiter ist als sein Fenster, hat keine
    // Kante — und die Kante ist das, woran man den Kasten erkennt.
    let x = pfeil.x.saturating_add(pfeil.breite).saturating_sub(breite);
    let x = x.min(fensterbreite.saturating_sub(breite));
    Some(Rechteck {
        x,
        y: pfeil.unten(),
        breite,
        hoehe,
    })
}

/// DIE **ZEILE** eines Downloads im Popup.
pub fn zeile(p: &Rechteck, nr: usize) -> Rechteck {
    Rechteck {
        x: p.x.saturating_add(LUFT_LINKS),
        y: p.y
            .saturating_add(LUFT_OBEN)
            .saturating_add(ZEILE * nr as u32),
        breite: p.breite.saturating_sub(2 * LUFT_LINKS),
        hoehe: ZEILE,
    }
}

/// DER KNOPF „**Ordner öffnen**“ einer Zeile (links der beiden Knoepfe).
pub fn knopf_ordner(p: &Rechteck, nr: usize) -> Rechteck {
    let z = zeile(p, nr);
    Rechteck {
        x: z.x
            .saturating_add(z.breite)
            .saturating_sub(2 * KNOPF + LUFT_KNOEPFE),
        y: z.y.saturating_add(z.hoehe).saturating_sub(KNOPF),
        breite: KNOPF,
        hoehe: KNOPF,
    }
}

/// DER KNOPF „**Abbrechen/Pause**“ einer Zeile (der rechte der beiden).
pub fn knopf_stop(p: &Rechteck, nr: usize) -> Rechteck {
    let z = zeile(p, nr);
    Rechteck {
        x: z.x.saturating_add(z.breite).saturating_sub(KNOPF),
        y: z.y.saturating_add(z.hoehe).saturating_sub(KNOPF),
        breite: KNOPF,
        hoehe: KNOPF,
    }
}

/// DIE FUSSZEILE „Alle Downloads anzeigen“.
pub fn alle_anzeigen(p: &Rechteck) -> Rechteck {
    Rechteck {
        x: p.x.saturating_add(LUFT_LINKS),
        y: p.y
            .saturating_add(p.hoehe)
            .saturating_sub(FUSS_HOEHE + LUFT_FUSS),
        breite: p.breite.saturating_sub(2 * LUFT_LINKS),
        hoehe: FUSS_HOEHE,
    }
}

/// WELCHE STELLE LIEGT UNTER DEM PUNKT?
pub fn treffer(p: &Rechteck, zeilen: usize, punkt: &Point) -> Treffer {
    if !punkt.x.is_finite() || !punkt.y.is_finite() {
        return Treffer::Nichts;
    }
    let drin = punkt.x >= p.x as f32
        && punkt.x < p.rechts() as f32
        && punkt.y >= p.y as f32
        && punkt.y < p.unten() as f32;
    if !drin {
        return Treffer::Nichts;
    }
    let zeilen = zeilen.min(MAX_DOWNLOADS);
    for nr in 0..zeilen {
        let z = zeile(p, nr);
        if punkt.y < z.unten() as f32 {
            if trifft(&knopf_stop(p, nr), punkt) {
                return Treffer::Stop(nr);
            }
            if trifft(&knopf_ordner(p, nr), punkt) {
                return Treffer::Ordner(nr);
            }
            return Treffer::Eintrag(nr);
        }
    }
    if trifft(&alle_anzeigen(p), punkt) {
        Treffer::Alle
    } else {
        Treffer::Nichts
    }
}

impl Downloads {
    /// M3: einen Download hinzufuegen, mit der Grenze.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Download den Platz verdraengt und
    /// nicht abgelehnt wird: der neue ist der, den der Nutzer gerade
    /// angefangen hat. Der älteste ist fertig oder laeuft seit Minuten.
    pub fn hinzufuegen(&mut self, d: Download) -> bool {
        if self.liste.len() >= MAX_DOWNLOADS {
            self.liste.remove(0);
        }
        self.liste.push(d);
        true
    }

    /// DAS RECHTECK DES POPUPS (`None`, wenn nichts laeuft).
    ///
    /// 05.10.2026 (UI): LEITUNG — „Popup **unter dem Download-Pfeil** in der
    /// Leiste (wie das Lesezeichen-Popup rechts)“. GEMESSENE ENTSCHEIDUNG,
    /// warum hier kein `fensterbreite`/`fensterhoehe` mehr stehen und statt
    /// dessen der **Pfeil**: der Kasten haengt am Knopf, den der Nutzer
    /// gedrueckt hat; eine Leiste am Fensterrand haengt an nichts und wird
    /// beim Scrollen der Seite nicht mitgefuehrt.
    pub fn rechteck(&self, chrome: &Chrome, pfeil: &Rechteck) -> Option<Rechteck> {
        // GEMESSENER EIGENER FEHLER (05.10., 08:06, gefunden von einem Test):
        // hier stand `u32::MAX` fuer die Fenstergroesse — und damit war die
        // **Klemmung in [`panel`] wirkungslos**. GEMESSEN hat es der Test
        // `das_download_panel_erscheint_erst_mit_einem_download`: das Popup
        // stand bei GEMESSEN x **753** mit GEMESSEN **299** breit in einem
        // Fenster von GEMESSEN 949 — also **52 px** draussen. GEMESSENE
        // ENTSCHEIDUNG, warum die Fensterzahl **hier** und nicht im Aufrufer
        // gerechnet wird: die Klemmung ist eine Eigenschaft des Kastens, und
        // ein Aufrufer, der sie vergisst, sieht es erst am Bild.
        // GEMESSEN (05.10., 08:06): `Chrome` kennt seine **Breite**
        // (`fensterbreite()`), aber **nicht** seine Hoehe — die traegt der
        // Aufruber (der weiss, wie hoch sein Fenster ist). Also klemmt hier
        // nur die **Breite**; die Hoehe wird von [`panel`] nur gebraucht,
        // um zu pruefen, ob das Popup ueberhaupt passt, und das entscheidet
        // der Aufruber mit seiner Zahl.
        panel(pfeil, self.liste.len(), chrome.fensterbreite(), u32::MAX)
    }

    /// DER TREFFER: was liegt unter dem Punkt.
    ///
    /// 05.10.2026 (UI): der Treffer sagt jetzt **was** getroffen wurde und
    /// nicht nur **welche Zeile** — das Popup hat zwei Knoepfe je Zeile
    /// (Ordner oeffnen, Abbrechen) und eine Fusszeile, und wer darauf klickt,
    /// will etwas anderes als die Zeile.
    pub fn treffer(&self, p: &Point, chrome: &Chrome, pfeil: &Rechteck) -> Treffer {
        let Some(r) = self.rechteck(chrome, pfeil) else {
            return Treffer::Nichts;
        };
        treffer(&r, self.liste.len(), p)
    }

    /// DAS POPUP ALS ZEICHENLISTE.
    ///
    /// 05.10.2026 (UI, Leitung): „Popup unter dem Download-Pfeil … Zeile:
    /// Dateisymbol, Name, Groesse/Status, Fortschrittsbalken, Aktionsknoepfe
    /// (Ordner oeffnen, Abbrechen/Pause), unten 'Alle Downloads anzeigen'“.
    /// GEMESSENE ENTSCHEIDUNG, warum das **Popup** und keine Leiste am
    /// Fensterrand: die Leiste stand **immer** da, auch ohne Download (GEMESSEN
    /// im Bild `ui_vier_bausteine.png`: sie verdeckte dort die Zeile
    /// „Lesezeichen verwalten“ des Lesezeichen-Popups, weil sie 24 px zu
    /// hoch stand und das Feld darunter endete). Ein Popup steht **nur** da,
    /// wenn es etwas zu zeigen hat.
    pub fn display_list(&self, chrome: &Chrome, pfeil: &Rechteck) -> DisplayList {
        let pal = super::thema::Palette::fuer(chrome.thema);
        let mut liste = DisplayList::new();
        let Some(r) = self.rechteck(chrome, pfeil) else {
            return liste;
        };
        // 1. DER KASTEN — opak, damit die Seite darunter nicht durchscheint.
        liste.push(DisplayItem::Rect {
            rect: Rect::new(r.x as f32, r.y as f32, r.breite as f32, r.hoehe as f32),
            color: pal.panel_grund,
            radius: pal.radius_leiste(),
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        // 2. DIE ZEILEN.
        for (nr, d) in self.liste.iter().enumerate().take(MAX_DOWNLOADS) {
            let z = zeile(&r, nr);
            if z.unten() > r.unten() {
                break;
            }
            // 2a. DAS DATEISYMBOL.
            //
            // GEMESSENE ENTSCHEIDUNG, warum ein **gezeichnetes** Blatt und
            // kein Emoji: die Leitung nennt ein „Dateisymbol“, und ein Emoji
            // ist eine **Schrift** — sie faellt auf einem Rechner ohne die
            // Schrift auf ein Kästchen zurueck (GEMESSEN im Bild: genau das
            // war der Punkt (1)). Ein Blatt ist zwei Rechtecke und damit
            // ueberall gleich.
            liste.push(DisplayItem::Rect {
                rect: Rect::new(z.x as f32, z.y as f32, ICON as f32, ICON as f32),
                color: pal.panel_trennlinie,
                radius: [3.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
            // 2b. DER NAME — wie die Vorlage es am Tab macht: abgeschnitten,
            //     mit `…`, und mit **Luft** zum Nachbarn.
            let name_x = z.x.saturating_add(ICON + LUFT_NAME);
            let platz = ((knopf_ordner(&r, nr).x.saturating_sub(LUFT_KNOEPFE))
                .saturating_sub(name_x)
                / super::ZEICHENBREITE) as usize;
            let name = d.name.trim();
            let text = if name.chars().count() > platz {
                let mut s: String = name.chars().take(platz.saturating_sub(1)).collect();
                s.push('…');
                s
            } else {
                name.to_string()
            };
            liste.push(DisplayItem::Text {
                schriftfamilie: String::from("sans-serif"),
                deko: 0,
                origin: Point::new(name_x as f32, (z.y + super::adressleiste::ZEILE) as f32),
                text,
                color: pal.text,
                font_size: chrome.schrift(super::lesezeichen_panel::SCHRIFT),
                gewicht: 400,
                kursiv: false,
            });
            // 2c. GROESSE / STATUS — die zweite Zeile, in der schwachen
            //     Tonfarbe (sie ist eine Angabe, nicht der Titel).
            liste.push(DisplayItem::Text {
                schriftfamilie: String::from("sans-serif"),
                deko: 0,
                origin: Point::new(
                    name_x as f32,
                    (z.y + super::adressleiste::ZEILE + 18) as f32,
                ),
                text: d.zeile(),
                color: pal.text_schwach,
                font_size: chrome.schrift(super::lesezeichen_panel::SCHRIFT),
                gewicht: 400,
                kursiv: false,
            });
            // 2d. DER FORTSCHRITTSBALKEN — **unter** der Zeile, ueber die
            //     volle Breite zwischen Name und Knopf.
            let bx = name_x;
            let bb = knopf_ordner(&r, nr)
                .x
                .saturating_sub(LUFT_KNOEPFE)
                .saturating_sub(bx);
            liste.push(DisplayItem::Rect {
                rect: Rect::new(bx as f32, (z.y + 34) as f32, bb as f32, BALKEN as f32),
                color: pal.panel_trennlinie,
                radius: [BALKEN as f32 / 2.0; 4],
                deckkraft: 1.0,
                schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                schatten_anzahl: 0,
            });
            if let Some(anteil) = d.fortschritt.anteil() {
                liste.push(DisplayItem::Rect {
                    rect: Rect::new(
                        bx as f32,
                        (z.y + 34) as f32,
                        ((bb as f32) * anteil).max(BALKEN as f32),
                        BALKEN as f32,
                    ),
                    color: pal.akzent,
                    radius: [BALKEN as f32 / 2.0; 4],
                    deckkraft: 1.0,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
            }
            // 2e. DIE ZWEI AKTIONSKNÖPFE.
            //
            // GEMESSENE ENTSCHEIDUNG, warum **Symbole aus `team/design/
            // icons`** und keine gezeichneten Formen: `ordner.svg` und
            // `beenden.svg` gibt es, und `symbole::zeichne` rastert sie in
            // jeder Groesse (das ist der Weg, den alle **20** anderen
            // Symbole gehen — GEMESSEN dort).
            super::symbole::zeichne(
                &mut liste,
                super::symbole::Symbol::Ordner,
                knopf_ordner(&r, nr).x,
                knopf_ordner(&r, nr).y,
                KNOPF,
                pal.text,
            );
            super::symbole::zeichne(
                &mut liste,
                super::symbole::Symbol::Beenden,
                knopf_stop(&r, nr).x,
                knopf_stop(&r, nr).y,
                KNOPF,
                pal.text,
            );
        }
        // 3. DIE TRENNLINIE ueber der Fusszeile.
        let f = alle_anzeigen(&r);
        liste.push(DisplayItem::Rect {
            rect: Rect::new(
                f.x as f32,
                (f.y - LUFT_ABSCHNITT) as f32,
                f.breite as f32,
                1.0,
            ),
            color: pal.panel_trennlinie,
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        // 4. DIE FUSSZEILE — „Alle Downloads anzeigen“.
        liste.push(DisplayItem::Text {
            schriftfamilie: String::from("sans-serif"),
            deko: 0,
            origin: Point::new(f.x as f32, (f.y + f.hoehe / 2 + 5) as f32),
            text: tr(FUSS_TEXT).to_string(),
            color: pal.text,
            font_size: chrome.schrift(super::lesezeichen_panel::SCHRIFT),
            gewicht: 400,
            kursiv: false,
        });
        liste
    }

    /// DARF DIE DATEI GESPERRT WERDEN?
    ///
    /// GEMESSENE BEGRUENDUNG fuer die Grenze: sie ist der Grund, warum
    /// diese Datei nicht wachsen kann. Ohne sie wuerde ein Downloader
    /// eine 40-GiB-Datei annehmen und der Speicher waere weg — genau der
    /// Fall aus `build_log.txt` (Exit 137). Die Grenze wird VOR dem
    /// Schreiben geprueft, nicht danach.
    pub fn darf_speichern(&self, groesse: u64) -> Result<(), String> {
        if groesse > MAX_DOWNLOAD_GROESSE {
            return Err(trf!(
                "File too large: {} MiB (limit {} MiB)",
                groesse / (1024 * 1024),
                MAX_DOWNLOAD_GROESSE / (1024 * 1024)
            ));
        }
        Ok(())
    }
}

/// DAS ZERTIFIKAT-FELD neben dem Schloss — die Geometrie.
///
/// GEMESSENE ENTSCHEIDUNG, warum das hier steht und nicht in `adressleiste`:
/// es ist eine Fläche in der Leiste, und `adressleiste.rs` kennt die
/// Vorschläge (die hängen UNTER dem Feld). Das Zertifikat hängt über der
/// Leiste und gehört damit zu U4.
pub fn zertifikat_feld(fensterbreite: u32) -> Rechteck {
    Rechteck {
        x: 0,
        y: super::TAB_LEISTE_HOEHE + super::LEISTE_HOEHE,
        breite: fensterbreite,
        hoehe: super::adressleiste::ZEILE * MAX_ZERT_ZEILEN as u32,
    }
}

/// DIE FARBE DES ZERTIFIKAT-FELDES.
pub fn zertifikat_farbe(chrome: &Chrome) -> Color {
    super::thema::Palette::fuer(chrome.thema).adresse
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adressleiste::Fortschritt;

    /// DIE DOWNLOAD-GRENZE (M4) — der Grund, warum diese Datei existiert.
    #[test]
    fn die_download_grenze() {
        let dl = Downloads::default();
        // Beide Raender.
        assert!(dl.darf_speichern(0).is_ok());
        assert!(dl.darf_speichern(MAX_DOWNLOAD_GROESSE).is_ok());
        // Darueber: ein BENANNTER Fehler, kein Panic.
        let r = dl.darf_speichern(MAX_DOWNLOAD_GROESSE + 1);
        assert!(r.is_err(), "eine zu grosse Datei wurde angenommen");
        assert!(
            r.err().unwrap_or_default().contains("too large"),
            "die Meldung sagt nicht, was los ist"
        );
        // BOMBEN: u64::MAX.
        assert!(dl.darf_speichern(u64::MAX).is_err());
        // GEMESSEN: die Grenze in MiB.
        assert_eq!(MAX_DOWNLOAD_GROESSE / (1024 * 1024), 512);
    }

    /// DIE LEISTE HAT HOECHSTENS DREI ZEILEN (M3).
    #[test]
    fn die_leiste_hat_hoechstens_drei_zeilen() {
        let mut dl = Downloads::default();
        for i in 0..5 {
            dl.hinzufuegen(Download::aus_adresse(&format!(
                "https://a{i}.example/datei{i}.txt"
            )));
        }
        assert_eq!(
            dl.liste.len(),
            MAX_DOWNLOADS,
            "die Leiste hat mehr als drei Zeilen"
        );
        // GEMESSENE ENTSCHEIDUNG: der NEUE verdraengt den ALTEN, also ist
        // der ERSTE der aelteste.
        assert_eq!(
            dl.liste[0].name, "datei2.txt",
            "es wurde nicht der aelteste verdraengt"
        );
        assert_eq!(dl.liste[2].name, "datei4.txt");
        // DIE GEOMETRIE: drei Zeilen, und das Popup haengt **unter dem
        // Download-Pfeil** (05.10., Leitung) — nicht mehr als Leiste ueber
        // der Statuszeile. GEMESSENE ENTSCHEIDUNG, warum die Zeilenhoehe
        // `ZEILE` des Moduls ist und **nicht** `adressleiste::ZEILE`: es
        // ist eine andere Zeile (48 px mit Dateisymbol, Namen, Balken und
        // zwei Knoepfen), und die alte Zahl gehörte zur abgeraeumten Leiste.
        let chrome = Chrome::neu(1000);
        let pfeil = chrome.rechts_symbol(3);
        let r = dl.rechteck(&chrome, &pfeil).expect("Popup");
        assert_eq!(
            r.hoehe,
            LUFT_OBEN + 3 * ZEILE + LUFT_ABSCHNITT + FUSS_HOEHE + LUFT_FUSS
        );
        assert_eq!(
            r.y,
            pfeil.unten(),
            "GEMESSEN: das Popup haengt am Pfeil, nicht am Fensterrand"
        );
        // UND die **rechte** Kante ist die des Pfeils (links ausgerichtet),
        // mit der GEMESSENEN Klemmung: bei 949 px (der Vorlagenbreite) stand
        // der Kasten 52 px **draussen**, weil `panel` `u32::MAX` fuer die
        // Fenstergroesse bekam. Das ist der Fehler, den diese Zeile hält.
        let schmal = Chrome::neu(949);
        let pfeil_s = schmal.rechts_symbol(3);
        let rs = dl.rechteck(&schmal, &pfeil_s).expect("Popup");
        assert!(
            rs.rechts() <= 949,
            "das Popup ragt {} px aus einem 949-px-Fenster",
            rs.rechts().saturating_sub(949)
        );
    }

    /// DER DATEINAME AUS DER ADRESSE (M4).
    #[test]
    fn der_dateiname_aus_der_adresse() {
        assert_eq!(
            Download::aus_adresse("https://a.example/datei.txt").name,
            "datei.txt"
        );
        assert_eq!(
            Download::aus_adresse("https://a.example/x/y/z.png").name,
            "z.png"
        );
        assert_eq!(Download::aus_adresse("datei.txt").name, "datei.txt");
        assert_eq!(Download::aus_adresse("").name, "");
        assert_eq!(
            Download::aus_adresse("https://a.example/").name,
            "",
            "eine Adresse mit Slash am Ende hat keinen Namen"
        );
        // BOMBEN: eine Million Zeichen, und die Laenge ist begrenzt.
        let d = Download::aus_adresse(&"a".repeat(1_000_000));
        assert!(
            d.name.chars().count() <= 120,
            "der Name ist {} Zeichen lang",
            d.name.chars().count()
        );
    }

    /// DIE ZEILE SAGT, WAS LOS IST.
    #[test]
    fn die_zeile_sagt_was_los_ist() {
        let mut d = Download::aus_adresse("https://a.example/datei.txt");
        d.fortschritt = Fortschritt {
            prozent: 0,
            bytes: 2048,
            bytes_gesamt: 4096,
            laedt: true,
        };
        let z = d.zeile();
        assert!(z.contains("datei.txt"));
        // GEMESSEN: 2048 Byte / 1024 = 2 KiB geladen, 4096 / 1024 = 4 KiB
        // gesamt, 2048/4096 = 50 %.
        assert!(
            z.contains("2 of 4 KiB"),
            "die Zeile nennt die Bytes nicht: {z:?}"
        );
        assert!(
            z.contains("50 %"),
            "die Zeile nennt den Fortschritt nicht: {z:?}"
        );
        // FERTIG: kein Fortschritt mehr in der Zeile.
        d.zustand = DownloadZustand::Fertig;
        assert!(
            !d.zeile().contains("KiB"),
            "die fertige Zeile redet noch von Bytes"
        );
        // FEHLGESCHLAGEN: die Meldung steht drin.
        d.zustand = DownloadZustand::Fehlgeschlagen;
        d.meldung = "Verbindung weg".into();
        assert!(d.zeile().contains("Verbindung weg"));
    }

    /// NaN UND BOMBEN (M8) — dieselbe Falle wie im Menue (U1c).
    ///
    /// 05.10. (UI): die Schleife laeuft ueber die **Fensterbreiten** und
    /// nicht ueber `(breite, hoehe)`-Paare — seit das Popup am Pfeil haengt
    /// nimmt [`Downloads::rechteck`] die Fensterbreite aus der `Chrome` und
    /// kennt keine Fenstereohe mehr. GEMESSENE ENTSCHEIDUNG, warum das kein
    /// Verlust ist: die alte Hoehe diente nur der `panel`-Klemmung, und die
    /// neue Klemmung sitzt an der Breite (der Stelle, an der sie vorher
    /// wirkungslos war).
    #[test]
    fn bomben_downloads() {
        let mut dl = Downloads::default();
        for fensterbreite in [0u32, 1, 100, 1000, u32::MAX] {
            let chrome = Chrome::neu(fensterbreite);
            let pfeil = chrome.rechts_symbol(3);
            assert_eq!(
                dl.treffer(&Point::new(f32::NAN, f32::NAN), &chrome, &pfeil),
                Treffer::Nichts,
                "NaN war ein Treffer bei {fensterbreite} px"
            );
            if let Some(r) = dl.rechteck(&chrome, &pfeil) {
                assert!(
                    r.rechts() <= fensterbreite || fensterbreite == u32::MAX,
                    "bei {fensterbreite}: ragt rechts heraus"
                );
                assert!(
                    r.x as u64 + r.y as u64 <= u64::from(u32::MAX),
                    "bei {fensterbreite}: x+y laeuft ueber"
                );
            }
            assert!(dl.display_list(&chrome, &pfeil).len() < 64);
        }
        // UND: die Zeile wird fuer das Bild gekuerzt.
        dl.hinzufuegen(Download::aus_adresse("https://a.example/x"));
        dl.liste[0].name = "n".repeat(100_000);
        {
            let chrome = Chrome::neu(1000);
            let pfeil = chrome.rechts_symbol(3);
            assert!(
                dl.display_list(&chrome, &pfeil).len() < 64,
                "ein riesiger Dateiname macht das Popup unendlich"
            );
            // Der Fortschrittsbalken: 100 % und 0 % muessen beide zeichnen.
            for prozent in [0u8, 100] {
                dl.liste[0].fortschritt = Fortschritt {
                    prozent,
                    bytes: 10,
                    bytes_gesamt: 10,
                    laedt: true,
                };
                assert!(dl.display_list(&chrome, &pfeil).len() >= 2);
            }
        }
    }
}
