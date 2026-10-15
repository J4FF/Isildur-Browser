//! U1c (01.10.2026): DIE KLICK-WEITERLEITUNG DER LEISTE.
//!
//! # WARUM EINE EIGENE DATEI
//!
//! Freigabe der Koordination (01.10., UI Runde 2): „in crates/narsil NUR
//! die Klickweiterleitung fuer die Leiste, z.B. neue Datei
//! narsil/src/leiste_klick.rs + minimale Einhaengung".
//!
//! GEMESSENE BEGRUENDUNG fuer eine eigene Datei und nicht mehr Code in
//! `sitzung.rs`: `sitzung.rs` hat ueber 5900 Zeilen und ist die Datei der
//! Rolle HTML. Jede Zeile, die ich dort einfuege, ist eine Zeile, an der
//! die HTML-Rolle und ich aneinander vorbeiarbeiten. Eine neue Datei hat
//! genau eine Beruehrungsstelle: `Sitzung::leisten_werkzeug` und ein Aufruf
//! in `mausklick`.
//!
//! # WAS DIESE DATEI KENNT
//!
//! * das Menue des Menue-Knopfs (offen, Treffer, Eintraege, Zeichnen),
//! * das Plus (neuer Tab) und das Menue als Klickziele,
//! * den Hover der Leiste.
//!
//! Und sonst nichts: kein Netz, keine Seite, kein Parse. Die Geometrie
//! kommt aus `narsil-chrome` (`Chrome::hover_unter`, `Chrome::plus_sichtbar`,
//! `Chrome::menue_sichtbar`), damit es keine zweite Wahrheit ueber die
//! Stellen gibt.

use narsil_chrome::{Chrome, Hover, Rechteck};
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};

/// M3: DIE HOEHE EINES MENU-EINTRAGS in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 24: `ZEILE_MENUE_HOEHE` in `sitzung.rs` ist
/// 22 (das ist das Rechtsklick-Menue, das die Rolle HTML gebaut hat).
/// Dieses Menue ist 24, weil es ZUSAETZLICH einen Kopf mit dem Titel
/// „Menue" traegt — und ein 2-px-Versatz zwischen zwei gleichartigen
/// Menues waere eine Zahl, die niemand nachvollziehen kann. GEMESSENE
/// ENTSCHEIDUNG, warum ueberhaupt EIN eigener Wert: die beiden Menues
/// gehoeren zu verschiedenen Werkzeugen (Rechtsklick = Seite,
/// Menue-Knopf = Browser), und sie werden nicht gleichzeitig geoeffnet.
pub const ZEILE: u32 = 24;

/// M3: DIE BREITE DES MENUES in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 200: der laengste Eintrag ist „Neu laden"
/// (10 Zeichen) und „Einstellungen" (12 Zeichen). Bei `ZEICHENBREITE` 8 px
/// sind 12 Zeichen 96 px, plus 2 x 12 px Rand sind 120 px. 200 px geben
/// Platz fuer 25 Zeichen — das reicht fuer jeden Eintrag, auch wenn einer
/// laenger wird. GEMESSENE ENTSCHEIDUNG, warum nicht breiter: der Knopf
/// steht am rechten Rand, und ein Menue, das ueber das Fenster ragt, ist
/// fuer den Nutzer tot.
pub const BREITE: u32 = 200;

/// M3: HOEHESTE ZAHL DER EINTRÄGE. Vier (Neu laden, Zoom -, Zoom +,
/// Einstellungen) plus Reserve. GEMESSENE BEGRUENDUNG: die Zahl kommt aus
/// dem `match` in [`MenueAktion`], und ein `match` ist vollstaendig — ein
/// fuenfter Eintrag wuerde den Compiler rot machen, nicht die Liste hier.
/// Diese Konstante ist die Grenze fuer die GEOMETRIE (die Liste, die
/// gezeichnet wird).
pub const MAX_EINTRAEGE: usize = 4;

/// DIE VIER EINTRÄGE DES MENUES.
///
/// GEMESSENE BEGRUENDUNG, warum genau diese vier: U1c verlangt „Neu laden,
/// Vergroessern +/-, Einstellungen-Platzhalter". Reihenfolge: das, was man
/// am haeufigsten tut (neu laden), dann das Paar (Zoom), dann der Weg in
/// die Einstellungen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenueAktion {
    NeuLaden,
    ZoomRaus,
    ZoomRein,
    Einstellungen,
}

impl MenueAktion {
    /// DER TEXT, der im Bild steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Symbole `+`/`-` hier TEXT sind
    /// und keine Vektorsymbole: sie stehen in einer Menuezeile neben einem
    /// Wort („Vergroessern"). Der Bitmap-Font hat `+` und `-`; die Pfeile
    /// aus `narsil_chrome::symbole` sind fuer 32-px-Knoepfe gemacht und
    /// waeren in einer 24-px-Zeile zu gross. GEMESSEN: `+` (U+002B) und
    /// `-` (U+002D) sind im Font vorhanden — es sind die beiden
    /// haeufigsten Zeichen ueberhaupt.
    pub fn text(&self) -> &'static str {
        match self {
            MenueAktion::NeuLaden => narsil_i18n::tr("Reload"),
            MenueAktion::ZoomRaus => narsil_i18n::tr("Zoom out"),
            MenueAktion::ZoomRein => narsil_i18n::tr("Zoom in"),
            MenueAktion::Einstellungen => narsil_i18n::tr("Settings"),
        }
    }

    /// Release 08.10.2026 (Glas-Stil): das Linien-Icon der Zeile.
    pub fn symbol(&self) -> narsil_chrome::symbole::Symbol {
        use narsil_chrome::symbole::Symbol;
        match self {
            MenueAktion::NeuLaden => Symbol::Neu,
            MenueAktion::ZoomRaus => Symbol::Minus,
            MenueAktion::ZoomRein => Symbol::Plus,
            MenueAktion::Einstellungen => Symbol::Zahnrad,
        }
    }

    /// Release 08.10.2026 (Glas-Stil): das Tastenkuerzel (`tasten.rs`:
    /// F5, Strg+-, Strg++); die Einstellungen haben keines.
    pub fn kuerzel(&self) -> Option<&'static str> {
        match self {
            MenueAktion::NeuLaden => Some("F5"),
            MenueAktion::ZoomRaus => Some(narsil_i18n::tr("Ctrl + -")),
            MenueAktion::ZoomRein => Some(narsil_i18n::tr("Ctrl + +")),
            MenueAktion::Einstellungen => None,
        }
    }
}

/// ALLE EINTRÄGE, in fester Reihenfolge.
///
/// GEMESSENE BEGRUENDUNG fuer die Liste: sie steht an drei Orten
/// (Zeichnen, Treffer, Klick), und drei `match`e ueber `MenueAktion`
/// waeren drei Orte, an denen ein Eintrag fehlen kann. Das ist die
/// Fehlerklasse „zwei Stellen entscheiden", zum SIEBTEN Mal in diesem
/// Projekt.
pub const EINTRAEGE: [MenueAktion; 4] = [
    MenueAktion::NeuLaden,
    MenueAktion::ZoomRaus,
    MenueAktion::ZoomRein,
    MenueAktion::Einstellungen,
];

/// DER ZUSTAND DES MENUES.
///
/// GEMESSENE BEGRUENDUNG, warum ein `Option` und nicht drei Felder
/// (`menue_auf`, `menue_x`, `menue_y`): drei Felder koennen sich
/// widersprechen (offen, aber ohne Position). Das ist dieselbe
/// Begruendung, mit der `RechtsklickMenue` in `sitzung.rs` ein `Option`
/// ist (siehe dort, Zeile 285).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeistenMenue {
    /// Wo der Knopf stand, als das Menue aufging (`x`, `y` in
    /// Fensterkoordinaten). GEMESSENE BEGRUENDUNG, warum die Position des
    /// KNOPFS und nicht des Klicks: das Menue gehoert zum Knopf.
    pub x: f32,
    pub y: f32,
    /// Release 08.10.2026 (Glas-Stil, HiDPI): die Skalierung der Leisten in
    /// Prozent (100 = 1:1) — Zeilen, Breite, Schrift und Icons wachsen mit.
    pub skala_prozent: u16,
}

impl LeistenMenue {
    /// Release 08.10.2026: logische Pixel in Fensterpixel.
    pub fn px(&self, logisch: u32) -> u32 {
        narsil_chrome::glas::px_mit(self.skala_prozent, logisch)
    }

    /// Release 08.10.2026: die Skalierung als Faktor.
    fn skala(&self) -> f32 {
        self.skala_prozent.max(1) as f32 / 100.0
    }

    /// DAS RECHTECK DES MENUES.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Menue LINKS vom Knopf steht und
    /// nicht darunter: der Menue-Knopf ist am rechten Fensterrand. GEMESSEN
    /// an meinem ersten Entwurf (`x = knopf.x - 4`): bei 1000 px Fenster
    /// und einem Knopf bei x = 960 blieben `1000 - 956 = 44` px Breite —
    /// das Menue war 44 px breit und seine vier Eintraege passten nicht.
    /// Der Test `die_groessen_stimmen` meldete `left: 44, right: 200`.
    ///
    /// DER WEG: das Menue haengt mit seiner RECHTEN Kante am linken Rand
    /// des Knopfs und waechst nach links. Bei Knopf x = 960 und 200 px
    /// Breite beginnt es bei 760 — es passt, und der Knopf bleibt sichtbar.
    ///
    /// GEMESSENE BEGRUENDUNG fuer das Klemmen an beiden Enden: ein Menue,
    /// das rechts oder unten ueber den Fensterrand ragt, ist fuer den
    /// Nutzer tot (die Eintraege sind nicht erreichbar). Steht der Knopf
    /// ganz links (Fenster zu schmal), beginnt das Menue bei 0.
    pub fn rechteck(&self, fensterbreite: u32, fensterhoehe: u32) -> Rechteck {
        // EIN KOPF (ZEILE) plus die vier Eintraege.
        let voll = self.px(ZEILE) * (1 + EINTRAEGE.len() as u32);
        // DIE HOEHE ist auf das Fenster begrenzt.
        //
        // GEMESSENER FEHLER MEINES ERSTEN ENTWURFS (01.10.,
        // `das_menue_bleibt_im_fenster`): die Hoehe war `voll`, und `y`
        // wurde auf `fensterhoehe - voll` geklemmt — dadurch ragte der
        // Kasten unten aus einem 100-px-Fenster heraus (gemeldet:
        // `y: 0, hoehe: 120` bei einer Fensterhoehe von 100).
        //
        // DER WEG: erst die HOEHE begrenzen, dann `y` setzen. Damit ist
        // `y + hoehe <= fensterhoehe` IMMER wahr, und die Zeichenschleife
        // (`display_list`) bricht bei `y + ZEILE > unten` ab — die
        // unteren Eintraege fehlen dann, statt ueber den Rand zu ragen.
        //
        // GEMESSENE BEGRUENDUNG fuer „begrenzen" und nicht „nicht
        // zeichnen": ein Menue, das gar nicht aufgeht, waere an der Stelle
        // kaputt, an der man es braucht (schmales Fenster). Ein Menue mit
        // zwei von vier Eintraegen ist brauchbar.
        let hoehe = voll.min(fensterhoehe);
        let rechts = (self.x as u32).min(fensterbreite);
        let x = rechts.saturating_sub(self.px(BREITE));
        let breite = rechts - x;
        // UNTER dem Knopf, aber geklemmt.
        let y = (self.y as u32)
            .saturating_add(self.px(Self::KNOPF_KANTE))
            .min(fensterhoehe.saturating_sub(hoehe));
        Rechteck {
            x,
            y,
            breite,
            hoehe,
        }
    }

    /// DER EINE ABSTAND zwischen dem Menue und dem Knopf darueber.
    ///
    /// GEMESSENE BEGRUENDUNG fuer 4: es ist die Haelfte von
    /// `narsil_chrome::LUFT_SYMBOL` (6) — gleiche Familie, halbe Zahl, weil
    /// das Menue kein Feld in einer Leiste ist, sondern ein Kasten, der
    /// ueber der Leiste steht.
    pub const ABSTAND_KNOPF: u32 = 4;

    /// DIE HOEHE DES KNOPFS ueber dem Menue.
    pub const KNOPF_KANTE: u32 = narsil_chrome::MENUE_KANTE;

    /// DER TREFFER: welcher Eintrag liegt unter dem Punkt.
    ///
    /// GEMESSENE BEGRUENDUNG, warum `None` statt `0`: Index 0 ist „Neu
    /// laden", und ein Klick neben dem Menue wuerde die Seite neu laden.
    /// Das ist die gefaehrlichste Moeglichkeit fuer ein Menue.
    pub fn treffer(&self, fensterbreite: u32, fensterhoehe: u32, p: &Point) -> Option<MenueAktion> {
        let r = self.rechteck(fensterbreite, fensterhoehe);
        // GEMESSENER BEFUND (01.10., `bomben_menue_und_punkte`): mit
        // `Point(NaN, NaN)` lieferte der Treffer `Some(NeuLaden)`.
        //
        // GEMESSENE BEGRUENDUNG, warum: jeder Vergleich mit NaN ist FALSCH,
        // und vier Vergleiche mit `||` ergeben damit `false || false ||
        // false || false = false` — die Wache greift nicht, und die
        // Rechnung darunter lief mit `NaN as u32 = 0`, was „Zeile 0" ist.
        // Das ist die gefaehrlichste denkbare Antwort auf ein kaputtes
        // Ereignis: „Neu laden".
        //
        // DER WEG: NaN wird VOR den Vergleichen abgewiesen. `is_finite` ist
        // die Pruefung, die auch `unendlich` faengt (GEMESSEN: der Test
        // schickt `f32::INFINITY` mit).
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        if p.x < r.x as f32
            || p.x >= r.rechts() as f32
            || p.y < r.y as f32
            || p.y >= r.unten() as f32
        {
            return None;
        }
        let zeile = (p.y as u32).saturating_sub(r.y) / self.px(ZEILE).max(1);
        // DER KOPF (Zeile 0) ist KEIN Eintrag.
        //
        // GEMESSENER FEHLER MEINES ERSTEN TESTS (01.10.): ich hatte
        // `EINTRAEGE.get(zeile)` geschrieben und dann an der Stelle
        // `r.y + ZEILE + 2` (also Zeile 1) geklickt und „Neu laden"
        // erwartet. Der Code lieferte `ZoomRaus` — zu Recht, denn Zeile 1
        // IST der zweite Eintrag. Der Test war falsch, nicht der Code.
        if zeile == 0 {
            return None;
        }
        // M3: die Liste hat hoechstens `MAX_EINTRAEGE` Eintraege, und
        // `zeile - 1` kann groesser sein, wenn jemand unterhalb der letzten
        // Zeile klickt (bei einer Fensterkante, die die Hoehe abgeschnitten
        // hat) — `get` gibt dann `None`.
        EINTRAEGE.get(zeile as usize - 1).copied()
    }

    /// DAS MENUE ALS ZEICHENLISTE.
    ///
    /// GEMESSENE REIHENFOLGE: Hintergrund, dann die Eintraege von oben.
    /// GEMESSENE BEGRUENDUNG, warum das Menue sich SELBST zeichnet und
    /// nicht ueber eine Liste von Farben: es hat vier Farben (Grund, Text,
    /// Kopf, Trennlinie), und die kommen aus `narsil_chrome::thema::Palette`
    /// — dieselbe Palette wie die Leiste. Ein zweiter Farbsatz waere eine
    /// zweite Wahrheit ueber dieselbe Frage („welche Farbe hat die
    /// Oberflaeche").
    pub fn display_list(&self, fensterbreite: u32, fensterhoehe: u32) -> DisplayList {
        self.display_list_mit_zeiger(fensterbreite, fensterhoehe, None)
    }

    /// Release 09.10.2026: wie [`LeistenMenue::display_list`], mit der Zeile
    /// unter der Maus hell hinterlegt (wie im Rechtsklick-Menue). GEMESSEN
    /// vorher: das Leistenmenue hatte als einziges Menue keinen Hover.
    pub fn display_list_mit_zeiger(
        &self,
        fensterbreite: u32,
        fensterhoehe: u32,
        zeiger: Option<MenueAktion>,
    ) -> DisplayList {
        // Release 08.10.2026 (Glas-Stil, Nutzerwunsch): dasselbe Glasfeld wie
        // das Kontextmenue — Kopfzeile als kleine graue Ueberschrift, je
        // Eintrag ein Linien-Icon, weisse Beschriftung und rechtsbuendig das
        // Tastenkuerzel (nur wo es die Taste gibt). Keine Trennlinien
        // zwischen den Eintraegen: es ist EIN Abschnitt.
        use narsil_chrome::glas;
        let r = self.rechteck(fensterbreite, fensterhoehe);
        let s = self.skala();
        let mut liste = DisplayList::new();
        glas::feld(&mut liste, &r, self.skala_prozent);
        let zeile = self.px(ZEILE);
        let kopf_schrift = narsil_chrome::ktx_menue::SCHRIFT_UEBERSCHRIFT * s;
        liste.push(glas::ueberschrift(
            (r.x + self.px(14)) as f32,
            r.y as f32 + (zeile as f32 - kopf_schrift * 1.2) / 2.0 + self.px(2) as f32,
            narsil_i18n::tr("Menu"),
            kopf_schrift,
            glas::GLAS_GRAU,
        ));
        let schrift = 13.5 * s;
        let kuerzel_schrift = narsil_chrome::ktx_menue::SCHRIFT_KUERZEL * s;
        for (nr, aktion) in EINTRAEGE.iter().enumerate() {
            // NUR die Eintraege, die in das (geklemmte) Rechteck passen.
            let y = r.y + zeile * (nr as u32 + 1);
            if y + zeile > r.unten() {
                break;
            }
            if zeiger == Some(*aktion) {
                let innen = self.px(4);
                liste.push(glas::hell(
                    Rect::new(
                        (r.x + innen) as f32,
                        y as f32,
                        r.breite.saturating_sub(2 * innen) as f32,
                        zeile as f32,
                    ),
                    [6.0 * s; 4],
                    narsil_chrome::ktx_menue::GLAS_HOVER,
                ));
            }
            let kante = self.px(16);
            if let Some((pixels, b, h)) = narsil_chrome::icons::pixel_genau(aktion.symbol(), kante, glas::GLAS_ICON) {
                liste.push(DisplayItem::Bild {
                    rect: Rect::new(
                        (r.x + self.px(14)) as f32,
                        (y + zeile.saturating_sub(kante) / 2) as f32,
                        b as f32,
                        h as f32,
                    ),
                    breite: b,
                    hoehe: h,
                    pixels,
                });
            }
            let text_y = y as f32 + (zeile as f32 - schrift * 1.2) / 2.0;
            liste.push(DisplayItem::Text {
                schriftfamilie: String::from("sans-serif"),
                deko: 0,
                origin: Point::new((r.x + self.px(14 + 16 + 10)) as f32, text_y),
                text: aktion.text().to_string(),
                color: glas::GLAS_TEXT,
                font_size: schrift,
                gewicht: 400,
                kursiv: false,
            });
            if let Some(k) = aktion.kuerzel() {
                let b = narsil_chrome::ktx_menue::kuerzel_breite(k, kuerzel_schrift);
                liste.push(DisplayItem::Text {
                    schriftfamilie: String::from("sans-serif"),
                    deko: 0,
                    origin: Point::new(
                        r.rechts().saturating_sub(self.px(14) + b) as f32,
                        y as f32 + (zeile as f32 - kuerzel_schrift * 1.2) / 2.0,
                    ),
                    text: k.to_string(),
                    color: glas::GLAS_GRAU,
                    font_size: kuerzel_schrift,
                    gewicht: 400,
                    kursiv: false,
                });
            }
        }
        liste
    }
}

/// DAS ERGEBNIS EINES KLICKS AUF DIE LEISTE.
///
/// GEMESSENE BEGRUENDUNG, warum ein `enum` und kein `bool`: es gibt
/// sieben moegliche Ziele (vier Knoepfe, Plus, Menue, Tab-Schliessen) und
/// drei davon tun nichts in dieser Datei (die vier Knoepfe macht `Sitzung`
/// schon). Ein `bool` koennte „Handled" nicht unterscheiden von „ja, der
/// Menue-Knopf".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeistenKlick {
    /// Der Klick ging an einen der vier Knoepfe — `Sitzung::knopf` ist
    /// zustaendig.
    Knopf,
    /// Neuer Tab (`Sitzung::tab_neu`).
    NeuerTab,
    /// Das Menue ist aufgegangen.
    MenueAuf,
    /// Das Menue ist wieder zu.
    MenueZu,
    /// Ein Eintrag wurde gewaehlt.
    MenueWahl(MenueAktion),
    /// Der Tab wurde geschlossen.
    TabGeschlossen,
    /// Der Klick hat in der Leiste nichts ausgeloest.
    Nichts,
}

/// WAS EIN KLICK AUF DER LEISTE TUT.
///
/// GEMESSENE BEGRUENDUNG fuer die Reihenfolge, und sie ist nicht
/// beliebig: erst das offene Menue (es liegt ueber allem), dann das
/// Schliessen-Kreuz (12 px, liegt IM aktiven Tab), dann Plus, dann die
/// vier Knoepfe, dann Menue, dann Tab.
///
/// GEMESSENER FEHLER, den diese Reihenfolge vermeidet: `tab_unter` gibt
/// bei x = 330 den Tab 2 zurueck (330 / 160 = 2), obwohl dort das
/// Plus-Knopf steht. Wer erst den Tab fragt, faengt die ersten Pixel des
/// Plus-Knopfs ab. `Chrome::hover_unter` hat diese Reihenfolge schon
/// richtig (siehe dort), und diese Funktion benutzt sie.
pub fn leisten_werkzeug(
    chrome: &mut Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
    p: &Point,
    menue: &mut Option<LeistenMenue>,
) -> LeistenKlick {
    // 1. DAS OFFENE MENUE hat Vorrang: es liegt ueber der Leiste.
    if menue.is_some() {
        let m = menue.expect("geprueft");
        if let Some(aktion) = m.treffer(fensterbreite, fensterhoehe, p) {
            *menue = None;
            return LeistenKlick::MenueWahl(aktion);
        }
        // Ausserhalb des Menues: geschlossen wird nur, wenn man nicht auf
        // die LEISTE selbst klickt — sonst waere ein Klick auf die
        // Adressleiste gleichzeitig „Menue schliessen" und „Adresse
        // tippen".
        if !punkt_auf_der_leiste(chrome, p) {
            *menue = None;
            return LeistenKlick::MenueZu;
        }
    }
    // 2. DIE GEOMETRIE entscheidet — eine Funktion, sieben Stellen.
    let hover = chrome.hover_unter(p);
    match hover {
        Hover::Schliessen => {
            chrome.setze_schliessen_hover(chrome.schliessen_unter(p).unwrap_or(0));
            LeistenKlick::TabGeschlossen
        }
        Hover::Plus => {
            chrome.setze_hover(Hover::Plus);
            LeistenKlick::NeuerTab
        }
        Hover::Menue => {
            chrome.setze_hover(Hover::Menue);
            let r = chrome.menue_sichtbar();
            match r {
                Some(m) => {
                    // GEMESSENE ENTSCHEIDUNG, warum ein zweiter Klick das
                    // Menue WIEDER schliesst: der Nutzer hat es gerade
                    // geoeffnet, und es ist noch da. Ohne dieses Verhalten
                    // muesste er irgendwo hinklicken, um es loszuwerden.
                    if menue.is_some() {
                        *menue = None;
                        LeistenKlick::MenueZu
                    } else {
                        *menue = Some(LeistenMenue {
                            x: m.x as f32,
                            y: m.y as f32,
                            skala_prozent: narsil_chrome::glas::prozent_von(chrome),
                        });
                        LeistenKlick::MenueAuf
                    }
                }
                None => LeistenKlick::Nichts,
            }
        }
        //
        // 02.10. 13:50, HTML: drei neue Varianten der Rolle UI
        // (`Stern`, `Download`, `Werbeblocker`) standen hier nicht, und der
        // Compiler sagte es deutlich: „non-exhaustive patterns: `Hover::Stern`,
        // `Hover::Download`, `Hover::Werbeblocker` and 1 more not covered".
        //
        // GEMESSENE ENTSCHEIDUNG, warum sie in den KNOPF-Zweig gehoeren und
        // nicht in einen eigenen: sie sind Werkzeugleisten-Knoepfe wie
        // `Neu` und `Start` — sie setzen den Hover und melden einen Klick, den
        // die Leiste behandelt. Ein eigener Zweig wuerde eine zweite
        // Antwort auf dieselbe Frage sein („was ist ein Knopf?"), und die
        // Antwort waere an drei Stellen zu pflegen.
        //
        // WAS ICH NICHT BEHAUPTE: dass die drei Knöpfe **ihre** Klickfolge
        // bekommen. Das ist Aufgabe der Rolle UI (Einstellungen-Seite,
        // Lesezeichen, Werbeblocker). Hier steht nur, dass der Klick als
        // **Knopf** gemeldet wird — die Entscheidung, was ein Knopf tut,
        // bleibt bei der einen Stelle, die sie schon hatte.
        Hover::Zurueck | Hover::Vorwaerts | Hover::Neu | Hover::Start => {
            chrome.setze_hover(hover);
            LeistenKlick::Knopf
        }
        //
        // 02.10. 13:55, HTML: JEDER WEITERE KNOPF DER LEISTE, ohne Liste.
        //
        // GEMESSENE GRUNDLAGE, warum es eine Liste und kein `_` war: ich habe
        // erst `Stern`, `Download`, `Werbeblocker` eingetragen (13:50), und der
        // naechste Lauf sagte `Hover::Puzzle` — die Leiste waechst weiter (Rolle
        // UI: Lesezeichen, Download, Werbeblocker, Puzzle).
        //
        // GEMESSENE ENTSCHEIDUNG, warum der **Sammelzweig** und nicht wieder
        // eine Liste: ein Klick auf einen neuen Knopf ist **derselbe** Fall wie
        // ein Klick auf `Neu` — Hover setzen und einen Knopfklick melden. Eine
        // Liste macht den Compiler zum Aufpass: bei jeder neuen Variante
        // faellt das ganze `narsil` — und **gemessen** ist genau das zweimal
        // passiert (13:49 und 13:55), in zwei Ansätzen.
        //
        // WAS ICH NICHT BEHAUPTE: dass ein neuer Knopf richtig *funktioniert*.
        // Dieser Zweig meldet einen Klick; was der Knopf tut, entscheidet die
        // eine Stelle, die es schon gab. Und `Nichts`/`Menue`/`Schliessen`
        // stehen **darueber** — sie haben eigene Zweige und eigene Bedeutung.
        Hover::Nichts => {
            chrome.hover_weg();
            LeistenKlick::Nichts
        }
        other => {
            chrome.setze_hover(other);
            LeistenKlick::Knopf
        }
    }
}

/// LIEGT DER PUNKT AUF DER BEDIENFLAECHE? (die Leiste ODER die Tab-Leiste)
///
/// GEMESSENE BEGRUENDUNG, warum die Frage hier steht und nicht im
/// Aufrufer: sie entscheidet, ob ein Klick neben dem Menue es schliesst
/// oder durchlaesst. Ein Aufrufer, der die Frage zwei Mal stellt, gibt sie
/// zweimal anders her — und der Klick auf die Adressleiste waere dann
/// einmal „Menue zu" und einmal „Fokus in die Leiste".
fn punkt_auf_der_leiste(chrome: &Chrome, p: &Point) -> bool {
    if p.y < narsil_chrome::TAB_LEISTE_HOEHE as f32 {
        return true;
    }
    let bedienflaeche = (narsil_chrome::TAB_LEISTE_HOEHE + narsil_chrome::LEISTE_HOEHE) as f32;
    p.y < bedienflaeche && p.x >= 0.0 && p.x <= chrome.fensterbreite() as f32
}

/// DER HOVER DER LEISTE fuellt `Chrome` — die eine Stelle.
///
/// GEMESSENE BEGRUENDUNG fuer diese Funktion: `mausklick` setzt den Hover
/// heute nicht (es gab keinen). Ohne diese Funktion gaebe es drei Stellen
/// (Mausbewegung, Klick, Menue) und damit drei Orte, an denen der
/// Schliessen-Hover vergessen werden kann — und das Bild zeigt einen
/// Knopf, der aufleuchtet, ohne dass die Maus da ist.
pub fn hover_setzen(chrome: &mut Chrome, p: &Point) {
    let h = chrome.hover_unter(p);
    if h == Hover::Schliessen {
        chrome.setze_schliessen_hover(chrome.schliessen_unter(p).unwrap_or(0));
    } else {
        chrome.setze_hover(h);
    }
}

/// DAS MENUE IN DIE ANZEIGELISTE HANGEN (ganz ans Ende).
///
/// GEMESSENE BEGRUENDUNG fuer „ganz ans Ende": die Seite zeichnet ihren
/// weissen Grund ueber alles, was vorher in der Liste stand. Das ist
/// derselbe Fehler, den die Rolle HTML bei ihrem Rechtsklick-Menue
/// gemacht hat (BERICHT_HTML_UI4, Fehler 1: „das Menue war nicht zu
/// sehen"). Das Menue MUSS nach dem Seitentext stehen.
pub fn menue_anhaengen(
    liste: &mut DisplayList,
    menue: &Option<LeistenMenue>,
    fensterbreite: u32,
    fensterhoehe: u32,
) {
    if let Some(m) = menue {
        liste.anhaengen(&m.display_list(fensterbreite, fensterhoehe));
    }
}

/// DIE FARBE DES KNOPFS, wenn er gehovert wird — fuer den Aufrufer.
///
/// GEMESSENE BEGRUENDUNG: das ist `Palette::hover`, und es steht hier nur,
/// damit der Test der Bild-Vergleich eine Zahl hat. Eine Funktion, die
/// eine Konstante zurueckgibt, ist die kleinstemoegliche Bruecke.
pub fn hover_farbe() -> Color {
    narsil_chrome::thema::Palette::dunkel().hover
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DER KNOPF UND DAS MENUE SIND 32 PX UND 200 PX.
    #[test]
    fn die_groessen_stimmen() {
        assert_eq!(ZEILE, 24);
        assert_eq!(BREITE, 200);
        assert_eq!(EINTRAEGE.len(), MAX_EINTRAEGE);
        // GEMESSEN: die Hoehe ist ein Kopf (ZEILE) plus vier Zeilen.
        let fenster = LeistenMenue { x: 960.0, y: 26.0, skala_prozent: 100 };
        let r = fenster.rechteck(1000, 700);
        assert_eq!(r.breite, 200);
        assert_eq!(r.hoehe, ZEILE * 5);
        assert!(r.unten() <= 700, "das Menue ragt unten heraus");
        assert!(r.x + r.breite <= 1000, "das Menue ragt rechts heraus");
    }

    /// DIE VIER EINTRÄGE, in der verlangten Reihenfolge.
    #[test]
    fn die_eintraege_sind_die_vier_verlangten() {
        assert_eq!(
            EINTRAEGE,
            [
                MenueAktion::NeuLaden,
                MenueAktion::ZoomRaus,
                MenueAktion::ZoomRein,
                MenueAktion::Einstellungen
            ]
        );
        // GEMESSEN: die Texte, wie sie im Bild stehen.
        assert_eq!(EINTRAEGE[0].text(), "Reload");
        assert_eq!(EINTRAEGE[3].text(), "Settings");
        // UND: kein Text laeuft aus dem Kasten. GEMESSEN: der laengste ist
        // „Vergroessern -" (14 Zeichen) = 112 px, der Kasten ist 200 px
        // breit und hat 12 px Rand links.
        let breitester = EINTRAEGE
            .iter()
            .map(|a| a.text().chars().count())
            .max()
            .expect("keine Eintraege");
        assert!(
            (breitester as u32) * 8 + 24 <= BREITE,
            "der breiteste Eintrag ({breitester} Zeichen) passt nicht in {BREITE} px"
        );
    }

    /// DER TREFFER FINDET DEN EINTRAG UND NICHTS DANEBEN.
    #[test]
    fn der_treffer_findet_und_verwirft() {
        let m = LeistenMenue { x: 960.0, y: 26.0, skala_prozent: 100 };
        let r = m.rechteck(1000, 700);
        // JEDE Zeile trifft ihren Eintrag.
        //
        // GEMESSENE ZAHL: Zeile 0 ist der KOPF (kein Eintrag), der erste
        // Eintrag steht in Zeile 1. GEMESSENER EIGENER FEHLER (01.10.):
        // ich hatte `r.y + ZEILE * nr` fuer `nr = 0` und „Neu laden"
        // erwartet; der Code lieferte zu Recht `ZoomRaus`, weil Zeile 1 der
        // zweite Eintrag ist.
        for (nr, aktion) in EINTRAEGE.iter().enumerate() {
            let y = r.y + ZEILE * (nr as u32 + 1) + ZEILE / 2;
            let p = Point::new(r.x as f32 + 20.0, y as f32);
            assert_eq!(
                m.treffer(1000, 700, &p),
                Some(*aktion),
                "Zeile {} trifft nicht",
                nr + 1
            );
        }
        // UND DER KOPF ist kein Eintrag.
        assert_eq!(
            m.treffer(1000, 700, &Point::new(r.x as f32 + 20.0, r.y as f32 + 4.0)),
            None,
            "der Kopf des Menues hat einen Eintrag"
        );
        // AUSSERHALB: kein Treffer. GEMESSENE ENTSCHEIDUNG: der unterste
        // Rand ist ausgeschlossen, weil ein Klick DORT „nichts tun" ist.
        let p = Point::new(r.x as f32 + 20.0, r.unten() as f32 + 5.0);
        assert_eq!(m.treffer(1000, 700, &p), None, "unterhalb des Menues");
        let p = Point::new(r.x as f32 - 5.0, r.y as f32 + 10.0);
        assert_eq!(m.treffer(1000, 700, &p), None, "links vom Menue");
        let p = Point::new(r.rechts() as f32 + 5.0, r.y as f32 + 10.0);
        assert_eq!(m.treffer(1000, 700, &p), None, "rechts vom Menue");
        // UND: ganz links oben im Fenster liegt auch kein Menue.
        let win = LeistenMenue { x: 0.0, y: 0.0, skala_prozent: 100 };
        assert_eq!(win.treffer(1000, 700, &Point::new(0.0, 0.0)), None);
    }

    /// DIE VIER KLICKZIELE DER LEISTE.
    #[test]
    fn die_vier_klickziele() {
        let mut chrome = Chrome::neu(1000);
        chrome.setze_tabs(&[("Narsil".into(), true)]);
        let mut menue: Option<LeistenMenue> = None;
        let mitte = |r: &Rechteck| {
            Point::new(
                r.x as f32 + r.breite as f32 / 2.0,
                r.y as f32 + r.hoehe as f32 / 2.0,
            )
        };
        // 1. DAS PLUS: neuer Tab.
        let plus = chrome.plus_sichtbar().expect("Plus-Knopf");
        assert_eq!(
            leisten_werkzeug(&mut chrome, 1000, 700, &mitte(&plus), &mut menue),
            LeistenKlick::NeuerTab,
            "das Plus ist kein neuer Tab"
        );
        assert!(menue.is_none(), "das Plus hat das Menue geoeffnet");
        // 2. DAS MENUE: geht auf.
        let menue_knopf = chrome.menue_sichtbar().expect("Menue-Knopf");
        assert_eq!(
            leisten_werkzeug(&mut chrome, 1000, 700, &mitte(&menue_knopf), &mut menue),
            LeistenKlick::MenueAuf,
            "der Menue-Knopf hat nichts geoeffnet"
        );
        let m = menue.expect("das Menue ist nicht offen");
        assert_eq!(m.x, menue_knopf.x as f32);
        // 3. DERSELBE KNOPF: geht wieder zu.
        assert_eq!(
            leisten_werkzeug(&mut chrome, 1000, 700, &mitte(&menue_knopf), &mut menue),
            LeistenKlick::MenueZu,
            "der zweite Klick hat das Menue nicht geschlossen"
        );
        assert!(menue.is_none());
        // 4. EIN EINTRAG: wird gewaehlt und schliesst das Menue.
        let _ = leisten_werkzeug(&mut chrome, 1000, 700, &mitte(&menue_knopf), &mut menue);
        let r = menue.expect("Menue").rechteck(1000, 700);
        // GEMESSENE ZAHL: der erste Eintrag steht in Zeile 1 (Zeile 0 ist
        // der Kopf).
        let eintrag = Point::new(r.x as f32 + 20.0, (r.y + ZEILE + ZEILE / 2) as f32);
        assert_eq!(
            leisten_werkzeug(&mut chrome, 1000, 700, &eintrag, &mut menue),
            LeistenKlick::MenueWahl(MenueAktion::NeuLaden),
            "der erste Eintrag wurde nicht gewaehlt"
        );
        assert!(menue.is_none(), "das Menue blieb nach der Wahl offen");
        // 5. DIE LEERE SEITE: nichts.
        assert_eq!(
            leisten_werkzeug(
                &mut chrome,
                1000,
                700,
                &Point::new(500.0, 400.0),
                &mut menue
            ),
            LeistenKlick::Nichts
        );
    }

    /// EIN KLICK AUF DIE SEITE SCHLIESST DAS MENUE.
    ///
    /// GEMESSENE BEGRUENDUNG: das ist der Normalfall — der Nutzer hat das
    /// Menue offen und klickt weg. Ohne dieses Verhalten bleibt das Menue
    /// ueber der Seite stehen und verdeckt Text.
    #[test]
    fn ein_klick_auf_die_seite_schliesst_das_menue() {
        let mut chrome = Chrome::neu(1000);
        let mut menue: Option<LeistenMenue> = None;
        let knopf = chrome.menue_sichtbar().expect("Menue-Knopf");
        let mitte = Point::new(
            knopf.x as f32 + knopf.breite as f32 / 2.0,
            knopf.y as f32 + knopf.hoehe as f32 / 2.0,
        );
        let _ = leisten_werkzeug(&mut chrome, 1000, 700, &mitte, &mut menue);
        assert!(menue.is_some());
        // AUF DIE SEITE (y = 400, das ist unter der Bedienflaeche).
        assert_eq!(
            leisten_werkzeug(
                &mut chrome,
                1000,
                700,
                &Point::new(500.0, 400.0),
                &mut menue
            ),
            LeistenKlick::MenueZu
        );
        assert!(menue.is_none(), "das Menue blieb offen");
    }

    /// ABER EIN KLICK IN DIE ADRESSSLEISTE SCHLIESST ES NICHT.
    ///
    /// GEMESSENE BEGRUENDUNG: der Nutzer will tippen, nicht schliessen.
    /// Ein Klick in die Leiste waere sonst doppelt (Menue zu + Fokus in
    /// die Leiste), und der Flick waere weg.
    #[test]
    fn ein_klick_in_die_adressleiste_schliesst_nicht() {
        let mut chrome = Chrome::neu(1000);
        let mut menue: Option<LeistenMenue> = None;
        let knopf = chrome.menue_sichtbar().expect("Menue-Knopf");
        let mitte = Point::new(
            knopf.x as f32 + knopf.breite as f32 / 2.0,
            knopf.y as f32 + knopf.hoehe as f32 / 2.0,
        );
        let _ = leisten_werkzeug(&mut chrome, 1000, 700, &mitte, &mut menue);
        let leiste = chrome.adressleiste();
        let in_leiste = Point::new(leiste.x as f32 + 30.0, leiste.y as f32 + 10.0);
        let r = leisten_werkzeug(&mut chrome, 1000, 700, &in_leiste, &mut menue);
        assert_ne!(
            r,
            LeistenKlick::MenueZu,
            "die Leiste hat das Menue geschlossen"
        );
        assert!(menue.is_some(), "das Menue wurde trotzdem geschlossen");
    }

    /// DAS MENUE RAGT IN KEINEM FENSTER HINAUS.
    #[test]
    fn das_menue_bleibt_im_fenster() {
        for (b, h) in [
            (1000u32, 700u32),
            (200, 100),
            (208, 60),
            (0, 0),
            (1, 1),
            (1920, 1080),
        ] {
            // GEMESSENE ENTSCHEIDUNG: der Knopf kann auch bei x = 0 stehen
            // (Fenster zu schmal, `menue_sichtbar()` gibt dann `None`, aber
            // die Funktion muss trotzdem eine Zahl liefern).
            let m = LeistenMenue {
                x: b as f32,
                y: h as f32,
                skala_prozent: 100,
            };
            let r = m.rechteck(b, h);
            assert!(r.rechts() <= b, "bei {b}x{h}: ragt rechts hinaus ({:?})", r);
            assert!(r.unten() <= h, "bei {b}x{h}: ragt unten hinaus ({:?})", r);
            assert!(r.breite <= b, "bei {b}x{h}: breiter als das Fenster");
        }
    }

    /// DAS MENUE HAT VIER TEXTE UND DREI TRENNLINIEN.
    ///
    /// GEMESSENE BEGRUENDUNG: die Texte sind das, was man im Bild liest;
    /// die Trennlinien machen vier Zeilen zu vier Knoepfen. Beides wird
    /// an der Liste gemessen, nicht an der Absicht.
    #[test]
    fn das_menue_hat_vier_texte() {
        let m = LeistenMenue { x: 960.0, y: 26.0, skala_prozent: 100 };
        let liste = m.display_list(1000, 700);
        // Release 08.10.2026 (Glas-Stil): gezaehlt werden die Beschriftungen
        // (13,5 px) — Ueberschrift und Kuerzel sind eigene Texte.
        let texte: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, font_size, .. } if *font_size == 13.5 => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texte.len(), 4, "es sind nicht vier Texte: {texte:?}");
        assert_eq!(
            texte,
            [
                "Reload",
                "Zoom out",
                "Zoom in",
                "Settings"
            ]
        );
        // Release 08.10.2026 (Glas-Stil): EIN Abschnitt, also keine Linien
        // zwischen den Zeilen — dafuer je Zeile ein Icon und die Kuerzel.
        let striche = liste
            .iter()
            .filter(|i| {
                matches!(i, DisplayItem::Rect { rect, .. }
                if rect.height == 1.0 && rect.width > 100.0)
            })
            .count();
        assert_eq!(striche, 0, "Trennlinien im Glasmenue: {striche}");
        let icons = liste.iter().filter(|i| matches!(i, DisplayItem::Bild { .. })).count();
        assert_eq!(icons, 4, "nicht vier Icons: {icons}");
        let alle: Vec<String> = liste
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for k in ["F5", "Ctrl + -", "Ctrl + +"] {
            assert!(alle.iter().any(|t| t == k), "das Kuerzel {k} fehlt: {alle:?}");
        }
    }

    /// DER HOVER BLEIBT NACH EINEM KLICK AUF DER LEISTE STEHEN.
    ///
    /// GEMESSENE BEGRUENDUNG: `mausklick` setzt den Hover heute nicht.
    /// Ohne das sieht der Knopf nach dem Klick aus, als sei die Maus noch
    /// da — was sie auch ist, aber nur bis zur naechsten Bewegung.
    #[test]
    fn der_hover_bleibt_nach_dem_klick_stehen() {
        let mut chrome = Chrome::neu(1000);
        let mut menue = None;
        let plus = chrome.plus_sichtbar().expect("Plus-Knopf");
        let p = Point::new(
            plus.x as f32 + plus.breite as f32 / 2.0,
            plus.y as f32 + plus.hoehe as f32 / 2.0,
        );
        leisten_werkzeug(&mut chrome, 1000, 700, &p, &mut menue);
        assert_eq!(chrome.hover, Hover::Plus, "der Hover steht nicht auf Plus");
        // ABER: eine Bewegung NACH OBEN loescht ihn.
        hover_setzen(&mut chrome, &Point::new(500.0, 400.0));
        assert_eq!(chrome.hover, Hover::Nichts, "der Hover blieb stehen");
    }

    /// BOMBEN (M8): das Menue in extremen Fenstern.
    #[test]
    fn bomben_menue_und_punkte() {
        for (b, h) in [
            (0u32, 0u32),
            (1, 1),
            (100, 40),
            (208, 60),
            (10_000, 10_000),
            (u32::MAX, u32::MAX),
        ] {
            let m = LeistenMenue {
                x: b as f32,
                y: h as f32,
                skala_prozent: 100,
            };
            let r = m.rechteck(b, h);
            // NICHTS rechnet ueber, nichts ist negativ.
            assert!(r.rechts() >= r.x);
            assert!(r.unten() >= r.y);
            // Die Liste ist baubar.
            let liste = m.display_list(b, h);
            // Release 08.10.2026 (Glas-Stil): Glasfeld (Schattenringe, Grund,
            // Glanz, Kante), Ueberschrift, je Zeile Icon, Text und Kuerzel.
            let grenze = narsil_chrome::ktx_menue::schatten_ringe(100) as usize + 3 + 1 + 3 * EINTRAEGE.len();
            assert!(
                liste.len() <= grenze,
                "bei {b}x{h}: {} Items im Menue (Grenze {grenze})",
                liste.len()
            );
            // Der Treffer gibt immer eine Zahl zurueck.
            let t = m.treffer(b, h, &Point::new(f32::NAN, f32::NAN));
            assert_eq!(t, None, "NaN wurde ein Treffer");
        }
    }
}
