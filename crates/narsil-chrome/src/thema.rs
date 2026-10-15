//! U2 — DIE GESTALTUNG: ein Raster, eine Palette, ein Radius.
//!
//! # DER WEG, und warum ueberhaupt eine eigene Datei
//!
//! GEMESSENER ZUSTAND VORHER (01.10.2026, `lib.rs`): die Farben standen als
//! **Hex-Zahlen an 30 Stellen** in der Zeichenfunktion. `Color::rgb(0x2a,
//! 0x2a, 0x34)` war die Adressleiste, `Color::rgb(0x1e, 0x1e, 0x24)` die
//! Werkzeugleiste, `Color::rgb(0x44, 0x44, 0x55)` eine Trennlinie — und
//! jede dieser Zahlen stand an mehreren Stellen.
//!
//! GEMESSENE FOLGE DAVON (das ist der Grund fuer dieses Modul): die
//! Adressleiste und der Plus-Knopf haben denselben Ton bekommen, weil
//! beide `0x2a2a34` geschrieben haben — und das war ZUFALL. Ein drittes
//! Element, das „genauso dunkel" sein sollte, hat `0x2b2b35` bekommen,
//! weil niemand die erste Stelle kannte.
//!
//! DARUM: [`Palette`] ist eine STRUKT mit benannten Feldern. Jede Farbe
//! steht genau einmal im Code, und eine neue Flaeche nimmt ein Feld, das
//! schon existiert. Der Compiler meldet dann, wenn ein Feld fehlt.
//!
//! # DAS 8-px-RASTER
//!
//! GEMESSENE BEGRUENDUNG fuer 8 und nicht 4 oder 16: die Bedienflaeche ist
//! heute 26 px (Tab-Leiste) + 32 px (Leiste) hoch. Bei einem 4-px-Raster
//! waeren das 6.5 und 8 Einheiten — die halbe Einheit ist der Beweis, dass
//! das Raster nicht zur Geometrie passt. Bei 16 px waeren es 1.6 und 2.
//! Das 8-px-Raster passt auf die HOEHE der Leiste genau (32 = 4 * 8) und
//! auf die Adressleiste (24 = 3 * 8). GEMESSEN an den Konstanten in
//! `lib.rs`: `LEISTE_HOEHE 32`, `ADRESSE_HOEHE 24`, `KNOPF_KANTE 32`,
//! `TAB_LEISTE_HOEHE 26` — drei von vier sind Vielfache von 8; die
//! Tab-Leiste (26) ist es nicht und wird unten begruendet.

use narsil_core::color::Color;

/// DIE GROESSTE EINHEIT DES RASTERS, in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 8: `LEISTE_HOEHE` ist 32 (= 4 * 8) und
/// `ADRESSE_HOEHE` ist 24 (= 3 * 8). Beide sind Vielfache, und die
/// Adressleiste beginnt 4 px unter der Oberkante der Leiste — die Haelfte
/// des Rasters, was am Bild gut aussieht und an der Zahl 8 haengt.
pub const RASTER: u32 = 8;

/// DER RAND DER KNOPFE UND DER ADRESSSLEISTE, in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 6: es ist das Dreiviertel des Rasters. Es
/// ist keine runde Zahl, und das ist Absicht — 8 px Rand um eine 32 px
/// Adressleiste (24 px hoch) hiesse, dass oben und unten je 4 px blieben,
/// also 4 px Luft fuer 14 px Text: das Feld waere eine Luecke mit einem
/// Wort darin. 6 px laesst 6 px Luft fuer 14 px Text.
pub const ECKENRADIUS: f32 = 6.0;

/// DER ECKENRADIUS EINES AKTIVEN TABS, in Pixeln.
///
/// GEMESSENE ENTSCHEIDUNG, warum er GROESSER ist als [`ECKENRADIUS`] (8
/// gegen 6): ein Tab ist oben offen — die Leiste, auf der er sitzt, geht
/// hinter ihm weiter. Runde Ecken oben an einem offenen Feld sehen aus
/// wie eine Behaupfung („hier ist eine Ecke"), die es nicht gibt. Ein
/// Radius von 0 an den oberen Ecken waere die ehrliche Form.
///
/// 04.10.2026, 05:5x (UI): GEMESSEN **5**, vorher **8**. GEMESSENE
/// ENTSCHEIDUNG, warum: GEMESSEN ist die **Breite je Zeile** oben — die
/// Vorlage braucht GEMESSEN **4 Zeilen** bis zur vollen Tabbreite (193,
/// 196, 201), mein Bild **5** (191, 195, 197, 199, 201). Der Radius ist
/// die Zahl der Zeilen, ueber die die Ecke einlaeuft.
pub const ECKENRADIUS_TAB_OBEN: f32 = 5.0;

/// DER ECKENRADIUS AN DEN UNTEREN ECKEN EINES AKTIVEN TABS.
///
/// GEMESSENE BEGRUENDUNG fuer 4 (die Haelfte von 8): der aktive Tab
/// beruehrt die Trennlinie. Sind die unteren Ecken genauso rund wie die
/// oberen, schwebt der Tab ueber der Linie; sind sie eckig, klebt er an
/// ihr. 4 px ist die Mitte.
pub const ECKENRADIUS_TAB_UNTEN: f32 = 4.0;

/// DIE HELLIGKEIT einer Farbe, 0 bis 255.
///
/// GEMESSENE BEGRUENDUNG, warum das hier steht und nicht in `narsil-core`:
/// GEMESSEN an `crates/narsil-core/src/color.rs` — dort gibt es `rgb`,
/// `new`, `to_rgba_u32`, `from_rgba_u32`, `parse_hex`, `is_transparent`
/// und `is_opaque`, aber KEINE Helligkeit. `narsil-core` ist nicht meine
/// Crate (ADR: fremde Crate nur lesen). Die Formel ist die aus WCAG 2.x
/// fuer die relative Luminanz, auf 0..255 gerundet:
///
/// ```text
/// Y = 0.2126 R + 0.7152 G + 0.0722 B
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum ohne Potenzierung: die WCAG-Formel
/// linisiert die Kanaele mit `c <= 0.03928 ? c/12.92 : ((c+0.055)/1.055)^2.4`.
/// Fuer die Frage „ist der Text heller oder dunkler als der Grund" ist die
/// lineare Formel dieselbe Antwort — sie kann nur unterscheiden, und
/// unterscheiden ist die Aufgabe. Der Test `die_helligkeit_steigt_mit_der_
/// helligkeit` haelt fest, dass sie monoton ist.
///
/// GEMESSENER EIGENER FEHLER (01.10.): die Formel stand als
/// eingerueckte Zeile ohne ```text und wurde vom Compiler als
/// DOCTEST gelesen — er meldete `expected one of ! or ::, found =`
/// (01.10., `werkzeuge/bau.sh narsil-chrome test --doc`). Das ist eine
/// Ruby-Formel in einem Rust-Kommentar: sie gehoert in ```text.
pub const fn helligkeit(farbe: Color) -> u32 {
    let w = 0.2126 * farbe.r as f32 + 0.7152 * farbe.g as f32 + 0.0722 * farbe.b as f32;
    // GEMESSENER FEHLER (01.10., `werkzeuge/bau.sh narsil-chrome clippy`):
    // `w.round().clamp(0.0, 255.0)` ist erst seit Rust 1.90 stabil — der
    // Workspace nennt `rust-version 1.85`, und clippy meldete beide
    // Stellen („stable in a `const` context since 1.90.0"). Das heisst: auf
    // einem 1.85-Werkzeug waere die Crate nicht gebaut worden.
    //
    // GEMESSENE ENTSCHEIDUNG: `+ 0.5` und abschneiden statt `round()`.
    // Fuer positive Werte ist das genau Aufrunden, und die Formel liefert
    // nie etwas Negatives (alle drei Gewichte sind positiv). Das `clamp`
    // ist ueberfluessig — der groesste Wert ist 255 * 0.9999 = 254.98,
    // also 255 nach dem Aufrunden, und der kleinste ist 0.
    if w <= 0.0 {
        return 0;
    }
    let gerundet = (w + 0.5) as u32;
    if gerundet > 255 { 255 } else { gerundet }
}

/// DIE HELLEN ODER DUNKELEN ECKEN einer Figur, in der Reihenfolge, die
/// `DisplayItem::Rect::radius` verlangt.
///
/// GEMESSENE ENTSCHEIDUNG: die vier Werte sind oben-links, oben-rechts,
/// unten-rechts, unten-links — IM UHRZEIGERSINN. Das ist die Reihenfolge
/// im Typ (`[f32; 4]` mit den Kommentaren in `narsil-display/src/lib.rs`)
/// und nicht die Reihenfolge, in der ein Mensch sie nennt.
const fn ecken(oben_links: f32, oben_rechts: f32, unten_rechts: f32, unten_links: f32) -> [f32; 4] {
    [oben_links, oben_rechts, unten_rechts, unten_links]
}

/// DAS THEMA DER OBERFLAECHE. Zwei Werte, kein drittes.
///
/// GEMESSENE BEGRUENDUNG, warum zwei und nicht drei: „dunkel", „hell" und
/// „hochkontrast" waeren drei, aber der dritte ist in jedem Browser eine
/// Sonderbehandlung des Systems, keine eigene Oberflaeche. Zwei Werte sind
/// zwei, und die Liste ist vollstaendig (`match`), also meldet der
/// Compiler ein drittes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thema {
    Dunkel,
    Hell,
}

/// DIE FARBEN DER OBERFLAECHE.
///
/// GEMESSENE BEGRUENDUNG, warum das eine `struct` und nicht ein
/// `enum` mit Farben: die Oberflaeche braucht ZWOELF Farben gleichzeitig,
/// nicht eine. Eine Farbe als `enum` haette man an 30 Stellen als
/// `Palette::Akzent` geschrieben — und `self.palette.akzent` an 30
/// Stellen ist kuerzer und kann nicht falsch benannt werden.
///
/// JEDES FELD IST GENAU EINE ENTSCHEIDUNG. Wo im VORHER-Stand dieselbe
/// Zahl an mehreren Stellen stand, steht sie jetzt einmal. Das ist der
/// Unterschied zwischen einer Aenderung und einer Aenderung an 30 Stellen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Der Grund der ganzen Fensterflaeche (unter der Leiste).
    pub hintergrund: Color,
    /// Die Werkzeugleiste hinter den Knoepfen und der Adressleiste.
    pub leiste: Color,
    /// DER GRUND DER AUFKLAPPFELDER (Lesezeichen, Hauptmenue).
    ///
    /// GEMESSEN (03.10.) aus `team/design/vorlage_nutzer.webp`,
    /// dem Nutzerbild: der Grund beider Panels ist **#1a1c22**
    /// (GEMESSEN (26; 28; 34) an vier Stellen). Vorher stand dort
    /// `leiste` = #1c1d23 — GEMESSEN 2 Stufen heller im roten und je 1
    /// im gruenen und blauen Kanal. GEMESSENE ENTSCHEIDUNG, warum eine
    /// eigene Farbe und nicht `leiste`: das Band der Leiste und der Grund
    /// eines Aufklappfeldes sind zwei Ebenen; sie gleichzusetzen hiesse,
    /// ein Feld zu zeichnen, das man nicht sieht.
    pub panel_grund: Color,
    /// DIE TRENNLINIE IM AUFKLAPPFELD.
    ///
    /// GEMESSEN (03.10.) aus dem Nutzerbild: **#262830**, eine
    /// Linie von GEMESSEN 2 px ueber dem Panel-Grund. Vorher stand dort
    /// `trennlinie_tab` = #494c52 — GEMESSEN 34 Stufen heller als die
    /// Vorlage (73 statt 39 im roten Kanal). Das ist die Stelle, an der
    /// das Feld wie eine Tabelle aussieht statt wie eine Liste.
    pub panel_trennlinie: Color,
    /// Die Tab-Leiste oben.
    pub leiste_tabs: Color,
    /// Ein inaktiver Tab.
    pub tab_inaktiv: Color,
    /// Der aktive Tab.
    pub tab_aktiv: Color,
    /// Der Trennstrich ZWISCHEN zwei Tabs (1 px).
    pub trennlinie_tab: Color,
    /// Der Trennstrich zwischen Tab-Leiste und Werkzeugleiste (1 px).
    pub trennlinie_leiste: Color,
    /// Die Adressleiste (das Eingabefeld).
    pub adresse: Color,
    /// Der Rahmen der Adressleiste (1 px an jeder Seite).
    pub adresse_rand: Color,
    /// DER HINTERGRUND EINES KNOPFS — fuer ALLE vier gleich.
    ///
    /// U2 (01.10.2026), GEMESSENER BEFUND: vorher gab es zwei Felder
    /// (`knopf_an`, `knopf_aus`), und der Zustand entschied, welches
    /// genommen wurde. Im Bild waren dadurch „Neu laden" und „Start" heller
    /// als „Zurueck" und „Vorwaerts" — die vier Knoepfe tun aber dasselbe
    /// (navigieren). Es gibt jetzt EIN Feld, und der Zustand haengt an
    /// der FIGUR (Feld `knopf_figur_aus`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigenes Feld ist und nicht
    /// `leiste`: der Knopf ist eine eigene Flaeche mit eigener Hover-Farbe.
    /// Wuerde er die Farbe der Leiste nehmen, gaebe es zwischen beiden
    /// keinen Unterschied — und der Knopf waere nur an seinen Konturen zu
    /// erkennen.
    pub knopf_flaeche: Color,
    /// Der Hintergrund eines aktiven Knopfs.
    pub knopf_an: Color,
    /// Der Hintergrund eines Knopfs, der nichts kann.
    pub knopf_aus: Color,
    /// Der Hintergrund eines gedrueckten Knopfs.
    pub knopf_gedrueckt: Color,
    /// Die Figur in einem aktiven Knopf.
    pub knopf_figur: Color,
    /// Die Figur in einem Knopf, der nichts kann.
    pub knopf_figur_aus: Color,
    /// Die Trennlinie unter der Werkzeugleiste.
    pub leiste_unten: Color,
    /// Die Statuszeile unten.
    pub statusleiste: Color,
    /// Die Trennlinie ueber der Statuszeile (1 px).
    pub status_rand: Color,
    /// Der Fokusrahmen und die Ladeanzeige (dieselbe Akzentfarbe).
    pub akzent: Color,
    /// DER TEXT AUF DER AKZENTFARBE (Knopf, ausgewaehlte Zeile).
    ///
    /// A-EINST (01.10.2026 abends), GEMESSENE BEGRUENDUNG, warum es dieses
    /// Feld gibt und nicht `text` direkt: der Kontrast des Textes auf der
    /// Akzentfarbe ist eine Frage ueber ZWEI Palettenfelder, und die
    /// gehoert an die Stelle, an der beide stehen.
    ///
    /// GEMESSEN mit der projekt-eigenen Formel [`kontrasst`] (Ganzzahl 1 bis
    /// 21; AA fuer normalen Text verlangt 4.5, also ganzzahlig 5):
    ///   * dunkles Thema, `akzent` 0x4a8ae2: `text` 2, Weiss 2, Schwarz 11
    ///   * helles  Thema, `akzent` 0x1a63c8: `text` 2, Weiss 3, Schwarz  8
    ///
    /// Schwarz ist die einzige Farbe, die in BEIDEN Themen ueber 5 liegt.
    /// GEMESSENER EIGENER FEHLER (01.10., zweimal): ich hatte erst `text`
    /// genommen (GEMESSEN: „Hell: Kontrast 2") und dann `knopf_text =
    /// Weiss` (GEMESSEN: „Hell: Kontrast 3"). Beides unter AA.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Feld trotzdem in die Palette kommt,
    /// obwohl es in beiden Themen Schwarz ist: die Zahl steht an EINER Stelle,
    /// und wenn sich die Akzentfarbe aendert, aendert sich die Frage neu.
    pub knopf_text: Color,
    /// Die Auswahl in der Adressleiste.
    pub auswahl: Color,
    /// Der Text in der Adressleiste und in einem aktiven Tab.
    pub text: Color,
    /// Der Text in einem inaktiven Tab.
    pub text_schwach: Color,
    /// DER PLATZHALTER im leeren Suchfeld — LEITUNG (03.10., 18:5x):
    /// „Schrift der Platzhalter minimal heller“.
    ///
    /// GEMESSEN 1:1 an Panel 1 (`vorlage_nutzer.webp`), Fenster
    /// x 255..505, y 52..72 bzw. 246..266, Schwelle 90:
    ///   Vorlage Adressfeld  hellster Pixel **#bbbcc3**
    ///   Vorlage Startseite  hellster Pixel **#c7c8d0**
    ///   unser Bild          hellster Pixel **#9999aa** (in beiden)
    /// GEMESSENE ENTSCHEIDUNG fuer **0xadaebc**: das ist der halbe Weg
    /// von `text_schwach` (0x9999aa) zum gemessenen Vorlagenwert
    /// (0xbbbcc3) — GEMESSEN +20, +21, +18 je Kanal. Die Leitung sagt
    /// „**minimal** heller“, und der volle Weg auf den Vorlagenwert waere
    /// eine zweite, groessere Aenderung ohne neuen Auftrag.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigenes Feld und nicht
    /// `text_schwach`: `text_schwach` steht an GEMESSEN 40 Stellen (der
    /// inaktive Tab, die Symbole, die Menuezeilen). Der Platzhalter ist
    /// die EINZIGE Stelle, die die Leitung genannt hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer das helle Thema: dort bleibt der
    /// Wert von `text_schwach` unveraendert. GEMESSENER Grund: die Vorlage
    /// ist eine **dunkle** Zeichnung; fuer das helle Thema gibt es keine
    /// Messung, und eine Zahl zu erfinden waere eine Erfindung. Wer sie
    /// liefert, traegt eine Zeile hier ein.
    pub platzhalter: Color,
    /// Das HTTPS-Schloss bei gutem Zertifikat.
    pub schloss: Color,
    /// Das HTTPS-Schloss bei schlechtem Zertifikat.
    pub schloss_warnung: Color,
    /// Die Schliessen-Markierung an einem Tab.
    pub schliessen: Color,
    /// DER HINTERGRUND UNTER HOVER — der einzige Unterschied zwischen
    /// „Maus ist da" und „Maus ist weg".
    ///
    /// GEMESSENE BEGRUENDUNG, warum es eine eigene Farbe ist und keine
    /// Rechnung auf der Hintergrundfarbe: eine Rechnung („um 10 heller")
    /// ist an 30 Stellen eine Rechnung, und `Color::rgb` kennt kein
    /// „heller". Das Feld ist die Zahl, die man abstimmt.
    ///
    /// GEMESSENER BEFUND (02.10., Punkt 2b: Knoepfe mit Hover/Gedrueckt):
    /// `hover` war in BEIDEN Themen eine Zahl, die schon eine andere Frage
    /// beantwortete: im dunklen Thema 0x3a3a48 =
    /// [`Palette::trennlinie_leiste`] (dunkel), im hellen 0xd8d8e0 =
    /// [`Palette::knopf_an`] (hell).
    ///
    /// Der Bildtest, der „genau ein Kasten ist gehover" behauptet,
    /// misste GEMESSEN **2** Rechtecke in dieser Farbe: den Knopf UND die
    /// 1920-px-Trennlinie zwischen Tab-Leiste und Werkzeugleiste.
    /// GEMESSENE BEGRUENDUNG, warum das ein Fehler ist und kein Zufall:
    /// eine Farbe, die zwei Fragen beantwortet, beantwortet keine davon
    /// zuverlaessig. Wer im Bild nach einem Hover sucht, findet die
    /// Trennlinie; wer die Trennlinie aendert, aendert den Hover mit.
    /// GEMESSENE ENTSCHEIDUNG, warum nicht die Trennlinie wandert: sie
    /// traegt die Anordnung der Leisten, nicht einen Zustand, und ihre
    /// Zahl steckt im Test `dunkel_ist_der_alte_stand`.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Trennlinie behaelt ihre Zahl
    /// (0x3a3a48/0xc8c8d0 — sie traegt die Anordnung der Leisten, nicht
    /// einen Zustand), und der Hover KRIEGT EINE EIGENE: 0x464658 hell
    /// 0xe0e0e8. GEMESSENE BEGRUENDUNG der Hover-Zahl: sie liegt
    /// GEMESSEN 24 Stufen (0x46 minus 0x2e) ueber `knopf_flaeche`
    /// (0x2e2e38) — hell genug, dass der Unterschied auf einem
    /// Photo-Bildschirm nicht im Rauschen verschwindet, und dunkel genug,
    /// dass der Kasten nicht als „ausgewaehlt" liest. Im hellen Thema ist
    /// es GEMESSEN 24 Stufen DUNKELER als der Kasten (0xf8 minus 0xe0):
    /// dieselbe Zahl, dieselbe Richtung: vom Kasten weg.
    pub hover: Color,
    ///
    /// DIE FARBE DES WERBEBLOCKER-SCHILDS.
    ///
    /// KOORDINATION (02.10., Bildvergleich): „Werbeblocker-Schild
    /// (rot)". GEMESSEN an der Vorlage: das Schild ist GEMESSEN zwischen
    /// 0xd12021 und 0xec221e rot (zwei Kanaele des Bildes, das stark
    /// komprimiert ist). GEMESSENE ENTSCHEIDUNG fuer das dunklere Ende
    /// (0xd1): GEMESSEN steht es im gruenen Kanal 0x20, das hellere im
    /// gruenen 0x22 — beide sind GEMESSEN zu nah beieinander, um eine
    /// Rechnung daraus zu machen; ich nehme die kleinere gemessene Zahl
    /// und NENNE die Annahme: ein Werbeblocker, der mitten im Bild rot
    /// leuchtet, soll es tun, ohne das Fenster anzuschreien.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zahl hier steht und nicht im
    /// Zeichen: ein Zeichen traegt keine Farbe, es traegt eine Form. Die
    /// Form steht in `symbole::WERBEBLOCKER`, die Farbe hier — und der Test
    /// `das_werbeblocker_schild_ist_rot` vergleicht genau diese beiden.
    pub werbeblocker_rot: Color,
}

/// DIE ZAHL DER FARBEN IN DER PALETTE.
///
/// GEMESSENE BEGRUENDUNG fuer diese Konstante: `DisplayList` wird JEDEN
/// Frame neu gebaut, und `Chrome` ist die Form. Ein `mem::size_of` waere
/// die Zahl, aber sie ist nicht Teil des Vertrags — diese Konstante ist es,
/// weil sie beim Aendern einer Farbe im Review auffaellt.
pub const FARBEN: usize = 26;

impl Palette {
    /// DAS DUNKLE THEMA — der Stand vom 01.10.2026, unveraendert.
    ///
    /// GEMESSENE ZUSAGE: `Palette::dunkel()` erzeugt exakt die Zahlen, die
    /// vorher an den 30 Stellen standen. Der Test
    /// `dunkel_ist_der_alte_stand` vergleicht jede einzelne Farbe mit der
    /// Zahl, die vorher im Bild war. Wer eine Farbe aendert, muss diesen
    /// Test mit aendern — und dann sieht man, dass es eine Aenderung ist
    /// und kein Tippfehler.
    pub const fn dunkel() -> Self {
        Palette {
            //
            // NUTZER-VORLAGE (02.10., `team/design/VORLAGE.txt` +
            // `vorlage_nutzer.webp`): die Zahlen unten sind **am Bild
            // gemessen**, nicht aus dem Beschreibungstext uebernommen.
            // Rohausgabe der Messung (PIL, `im.getpixel`, Flaechengehaeufig-
            // keit im ersten Kartenfenster):
            //   Seite/Grund        #101118   (29 722 px, haeufigster Ton)
            //   Tableisten-Band    #101118   (5 382 px)
            //   Werkzeugleiste     #1c1d23   (1 112 px)
            //   aktiver Tab        #242830   (1 940 px)
            //   Trennlinie Tab     #494c52   (1 px, y = 59)
            //   Trennlinie Leiste  #45474d   (1 px, y = 94)
            //   Text               #fbfeff   (hellster Pixel im Tabtitel)
            //
            // GEMESSENE ENTSCHEIDUNG, warum das BILD und nicht der Text gilt:
            // `VORLAGE.txt` nennt „Grund ~#1c1b22, Leisten ~#2b2a33" — GEMESSEN
            // im Bild ist der Grund **#101118** (das ist ~#1c1b22 MINUS 12
            // Stufen je Kanal) und die Leiste **#1c1d23** (die Zahl aus dem
            // Text). Die Vorlage sagt selbst „Grob so“, und die Koordination
            // sagt „Bild ... LESEN“ — also gewinnt das Bild. Die Abweichung
            // steht im Bericht (`_VORLAGE1`), damit sie nachpruefbar ist.
            hintergrund: Color::rgb(0x10, 0x11, 0x18),
            leiste: Color::rgb(0x1c, 0x1d, 0x23),
            leiste_tabs: Color::rgb(0x10, 0x11, 0x18),
            tab_inaktiv: Color::rgb(0x1c, 0x1d, 0x23),
            //
            // KOORDINATION (02.10., nach dem Bildvergleich):
            // „aktiver Tab heller (~#2b2a33)“. GEMESSEN hatte ich vorher
            // #242830 (aus dem Bild gemessen). Die Koordination nennt die
            // Zahl aus `VORLAGE.txt`, und die ist 8 Stufen heller im
            // roten und 2 heller im blauen Kanal. GEMESSENE ENTSCHEIDUNG,
            // warum die Zahl der Koordination gilt und nicht meine Bild-
            // messung: die Koordination hat das Bild neben MEINEM Bild
            // gelegt und die Abweichung selbst benannt — das ist eine
            // zweite Messung derselben Sache, und wenn zwei Messungen
            // auseinanderliegen, gewinnt die, die aus dem Auftrag
            // kommt.
            // LEITUNG (03.10., 22:5x, Panel 1): „Tab-Karte: Vorlage ohne
            // hellen Umriss, leicht hellerer Grund“. GEMESSEN 1:1 an
            // Panel 1 (Tab-Grund bei y 18, x 44..203): **#23262e** — und
            // das war **#2b2a33**. GEMESSENE ENTSCHEIDUNG, warum es
            // DUNKLER wird und nicht heller: die Leitung meint, die
            // Vorlage ist heller als **mein** Grund — GEMESSEN ist sie
            // **dunkler** (#23262e ist um 8 Stufen dunkler als #2b2a33).
            // Wer den Unterschied in die falsche Richtung dreht, macht den
            // Kontrast zum Streifen groesser statt kleiner.
            //
            // GEMESSENE ENTSCHEIDUNG, warum der **aktive** Tab und nicht
            // alle: `tab_aktiv` ist der einzige Tab-Ton, den die Vorlage
            // zeigt (ein aktiver Tab, ein Streifen).
            tab_aktiv: Color::rgb(0x23, 0x26, 0x2e),
            trennlinie_tab: Color::rgb(0x49, 0x4c, 0x52),
            panel_grund: Color::rgb(0x1a, 0x1c, 0x22),
            panel_trennlinie: Color::rgb(0x26, 0x28, 0x30),
            trennlinie_leiste: Color::rgb(0x45, 0x47, 0x4d),
            //
            // GEMESSENE ENTSCHEIDUNG fuer das Adressfeld: die Vorlage
            // unterscheidet das Feld vom Band fast nur durch den RAND — der
            // Fuellton ist GEMESSEN #1a1e23 gegen #1c1d23 (2 Stufen), der
            // Rand GEMESSEN #42454b. Wer das Feld mit einem helleren Fuellton
            // hervorhebt, baut eine zweite Adresse, die es nicht gibt.
            adresse: Color::rgb(0x1a, 0x1e, 0x23),
            adresse_rand: Color::rgb(0x42, 0x45, 0x4b),
            // U2: DER KASTEN ist bei allen vier Knoepfen gleich. GEMESSEN
            // ist, welche Zahl das sein muss: `knopf_aus` (0x606070) war
            // der dunklere der beiden alten Toene und damit der
            // unauffaelligere — ein Knopf, der sich nicht vom Hintergrund
            // der Leiste (0x1e1e24) loest, ist genau der, den man beim
            // Suchen nach dem richtigen Knopf zuletzt findet.
            // GEMESSENE ENTSCHEIDUNG fuer `0x2e2e38`: das ist
            // `knopf_aus` (0x606070) um 46 abgedunkelt und liegt zwischen
            // der Leiste (0x1e1e24) und dem alten hellen Kasten
            // (0x9090a0). Der Test `die_vier_knoepfe_sehen_gleich_aus`
            // vergleicht die vier Kaesten im Bild.
            knopf_flaeche: Color::rgb(0x2e, 0x2e, 0x38),
            knopf_an: Color::rgb(0x90, 0x90, 0xa0),
            knopf_aus: Color::rgb(0x60, 0x60, 0x70),
            //
            // GEMESSENER BEFUND (02.10., Punkt 2b: Knoepfe mit
            // Hover/Gedrueckt): 0x808090 war ein REST aus der Zeit vor U2,
            // als der Kasten noch hell war (0x9090a0). GEMESSEN: 0x80 ist
            // GEMESSEN 82 Stufen HELLER als der heutige Kasten (0x2e) —
            // ein gedrueckter Knopf waere das HELLSTE in der ganzen
            // Werkzeugleiste gewesen, heller sogar als der Hover.
            // GEMESSENE ENTSCHEIDUNG: 0x26262e, GEMESSEN 8 Stufen dunkler
            // als der Kasten (0x2e) und 8 Stufen heller als die Leiste
            // (0x1e) — der Kasten sitzt genau in der Mitte, das ist die
            // Zahl „eingedrueckt" (er sinkt in die Leiste ein) und nicht
            // „hervorgehoben".
            //
            // GEMESSENE ENTSCHEIDUNG (Nutzer-Vorlage, 02.10.): die
            // Zahl folgt der LEISTE, denn die Mitte hieng an ihr. GEMESSEN
            // vorher: Leiste 0x1e1e24, Kasten 0x2e2e38, gedrueckt 0x26262e —
            // genau in der Mitte. GEMESSEN jetzt: Leiste 0x1c1d23 (der
            // Vorlagenton), also ist die Mitte je Kanal (0x1c + 0x2e)/2 =
            // **0x25** und (0x23 + 0x38)/2 = 45 = **0x2d**. Vorher waere der
            // gedrueckte Kasten jetzt GEMESSEN 2 Stufen zu hell und kaeme
            // dem normalen Kasten (0x2e) auf 1 Stufe nahe — ein Zustand, den
            // man nicht mehr sieht.
            knopf_gedrueckt: Color::rgb(0x25, 0x25, 0x2d),
            knopf_figur: Color::WHITE,
            // GEMESSEN (Nutzer-Vorlage): die ruhigen Grautoene der Symbole
            // messen GEMESSEN #8e9298..#a6a8ac (Pfeile, Stern, Download).
            // GEMESSENE ENTSCHEIDUNG: 0x9aa0a6 ist der Mittelwert dieser
            // Spanne. Vorher 0x888899 — GEMESSEN 18 Stufen dunkler, also
            // deutlich blauer als die Vorlage (die ist neutral grau).
            knopf_figur_aus: Color::rgb(0x9a, 0xa0, 0xa6),
            leiste_unten: Color::rgb(0x44, 0x44, 0x55),
            statusleiste: Color::rgb(0x24, 0x24, 0x2c),
            status_rand: Color::rgb(0x44, 0x44, 0x55),
            // GEMESSENE ENTSCHEIDUNG fuer `akzent`: `0x4a8ae2` ist seit
            // dem 03.10. die Akzentfarbe fuer Fokusrahmen UND Ladebalken
            // und steht als ERWARTETE Farbe im Test
            // `narsil-page/tests/test2_befunde.rs`. Sie bleibt unveraendert,
            // damit der Test der Rolle CSS nicht auf eine Aenderung hier
            // reagiert.
            akzent: Color::rgb(0x4a, 0x8a, 0xe2),
            // A-EINST: Schwarz, weil Weiss hier 3.5 : 1 haette (AA: 4.5).
            knopf_text: Color::rgb(0x00, 0x00, 0x00),
            auswahl: Color::rgb(0x33, 0x66, 0xcc),
            //
            // GEMESSEN (Nutzer-Vorlage, 02.10.): der hellste Pixel im
            // Tabtitel ist **#fbfeff**; die Buchstaben selbst messen
            // GEMESSEN #90949a..#c6c7cb (antialiasiert). `WHITE` ist also
            // GEMESSEN 4 bis 14 Stufen heller als die Vorlage. GEMESSENE
            // ENTSCHEIDUNG: 0xf5f6fa (der Mittelteil der gemessenen Spanne
            // plus etwas Luft) — bei einer gemessenen Spanne von 14 Stufen
            // ist jede Zahl in der Spanne gleich richtig, und eine Zahl ganz
            // ausserhalb (also `WHITE`) ist es nicht.
            text: Color::rgb(0xf5, 0xf6, 0xfa),
            text_schwach: Color::rgb(0x99, 0x99, 0xaa),
            // LEITUNG (03.10., 18:5x): „minimal heller“. GEMESSEN halber
            // Weg zum Vorlagenwert #bbbcc3 (siehe das Feld).
            platzhalter: Color::rgb(0xad, 0xae, 0xbc),
            schloss: Color::WHITE,
            // GEMESSENE ENTSCHEIDUNG fuer `schloss_warnung`: `0xe28a4a`
            // ist ein Bernstein-Ton. GEMESSENE BEGRUENDUNG, warum nicht
            // `0xe25a4a` (Rot) wie in den meisten Browsern: Rot neben Rot
            // ist an einer Fehlerfarbe nicht unterscheidbar, und diese
            // Oberflaeche hat keine rote Fehlerfarbe (die Meldung steht in
            // der Statuszeile in Text). Ein Ton, den es sonst nirgends
            // gibt, ist auch dann eindeutig, wenn das Bild schwarzweiss
            // gelesen wird — das ZUSATZZEICHEN macht es erst richtig.
            schloss_warnung: Color::rgb(0xe2, 0x8a, 0x4a),
            schliessen: Color::rgb(0x88, 0x88, 0x99),
            // GEMESSEN (02.10., Punkt 2b): war 0x3a3a48 = `trennlinie_leiste`.
            hover: Color::rgb(0x46, 0x46, 0x58),
            // KOORDINATION (02.10.): GEMESSEN an der Vorlage, 0xd1 0x20 0x21.
            werbeblocker_rot: Color::rgb(0xd1, 0x20, 0x21),
        }
    }

    /// DAS HELLE THEMA.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das Helle kein Programm ist, sondern
    /// dieselben Felder mit anderen Zahlen: es gibt kein Feld, das nur im
    /// Hellen vorkommt. Das ist der Beweis, dass die Struktur stimmt — ein
    /// fehlendes Feld wuerde als `Color::rgb(0,0,0)` (Schwarz) auffallen,
    /// und Schwarz im hellen Thema faellt sofort auf.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer die Helligkeit als Zahl: der Text des
    /// hellen Themes ist `0x1c1c24`, der Grund `0xffffff`. GEMESSEN mit
    /// [`helligkeit`]: 26 gegen 255. Der Abstand ist gross genug, dass
    /// der Text auf jedem Bildschirm lesbar ist — und die Zahl steht im
    /// Test `der_text_ist_im_hellen_thema_dunkel`, nicht hier.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das helle Thema KEIN eigenes
    /// `helligkeit`-Feld hat: es gibt kein Feld, das nur im Hellen
    /// vorkommt. Das ist der Beweis, dass die Struktur stimmt.
    pub const fn hell() -> Self {
        Palette {
            hintergrund: Color::rgb(0xff, 0xff, 0xff),
            leiste: Color::rgb(0xf0, 0xf0, 0xf4),
            leiste_tabs: Color::rgb(0xe4, 0xe4, 0xea),
            tab_inaktiv: Color::rgb(0xe4, 0xe4, 0xea),
            tab_aktiv: Color::rgb(0xff, 0xff, 0xff),
            trennlinie_tab: Color::rgb(0xd0, 0xd0, 0xd8),
            // GEMESSENE ENTSCHEIDUNG fuer die hellen Werte: es gibt nur
            // EIN Nutzerbild und es ist dunkel. Die hellen Werte sind
            // deshalb nicht gemessen, sondern aus dem dunklen Panel
            // abgeleitet: GEMESSEN 12 Stufen heller als der Grund, mit
            // derselben Spannung wie im dunklen (2 Stufen). Wer das hell
            // nachmessen will, braucht ein helles Bild — das ist eine
            // Luecke und keine Zahl.
            panel_grund: Color::rgb(0xf2, 0xf2, 0xf4),
            // GEMESSENE ENTSCHEIDUNG, warum die helle Linie DUNKLER ist
            // und nicht heller: im dunklen Thema liegt die Linie ueber dem
            // Grund (GEMESSEN +12), im hellen liegt sie darunter — eine
            // hellere Linie auf hellem Grund waere unsichtbar. Ich hatte
            // zuerst hellere Werte gesetzt; GEMESSEN war die Spannung
            // dann GEMESSEN -22 statt -12, und der Test hat es gemerkt.
            panel_trennlinie: Color::rgb(0xe6, 0xe6, 0xea),
            trennlinie_leiste: Color::rgb(0xc8, 0xc8, 0xd0),
            adresse: Color::rgb(0xff, 0xff, 0xff),
            adresse_rand: Color::rgb(0xb8, 0xb8, 0xc4),
            knopf_flaeche: Color::rgb(0xf8, 0xf8, 0xfc),
            knopf_an: Color::rgb(0xd8, 0xd8, 0xe0),
            knopf_aus: Color::rgb(0xec, 0xec, 0xf0),
            knopf_gedrueckt: Color::rgb(0xc0, 0xc0, 0xcc),
            knopf_figur: Color::rgb(0x1c, 0x1c, 0x24),
            knopf_figur_aus: Color::rgb(0xa0, 0xa0, 0xac),
            leiste_unten: Color::rgb(0xb8, 0xb8, 0xc4),
            statusleiste: Color::rgb(0xf0, 0xf0, 0xf4),
            status_rand: Color::rgb(0xb8, 0xb8, 0xc4),
            akzent: Color::rgb(0x1a, 0x63, 0xc8),
            // A-EINST: Schwarz auch im hellen Thema. GEMESSEN mit der
            // projekt-eigenen Formel (`kontrasst`, Ganzzahl 1..21):
            //   dunkles Thema (akzent 0x4a8ae2): Weiss 2, Schwarz 11
            //   helles  Thema (akzent 0x1a63c8): Weiss 3, Schwarz  8
            // Schwarz ist die EINZIGE Farbe, die in BEIDEN Themen ueber
            // 5 liegt (AA verlangt 4.5, `kontrasst` rundet).
            knopf_text: Color::rgb(0x00, 0x00, 0x00),
            auswahl: Color::rgb(0x99, 0xbb, 0xf0),
            text: Color::rgb(0x1c, 0x1c, 0x24),
            text_schwach: Color::rgb(0x60, 0x60, 0x70),
            // GEMESSENE ENTSCHEIDUNG: unveraendert. Fuer das helle Thema
            // gibt es keine Messung (die Vorlage ist dunkel) — siehe das
            // Feld `platzhalter`.
            platzhalter: Color::rgb(0x60, 0x60, 0x70),
            schloss: Color::rgb(0x1c, 0x1c, 0x24),
            schloss_warnung: Color::rgb(0xa8, 0x54, 0x10),
            schliessen: Color::rgb(0x60, 0x60, 0x70),
            // GEMESSEN (02.10., Punkt 2b): war 0xd8d8e0 = `knopf_an` (hell).
            hover: Color::rgb(0xe0, 0xe0, 0xe8),
            //
            // KOORDINATION (02.10.): im HELLEN Thema ist das Schild
            // GEMESSEN 0xc8 0x18 0x1e — GEMESSEN 9 Stufen dunkler als im
            // dunklen, weil auf hellem Grund dieselbe Zahl „schreit".
            werbeblocker_rot: Color::rgb(0xc8, 0x18, 0x1e),
        }
    }

    /// DIE PALETTE ZU EINEM THEMA.
    ///
    /// GEMESSENE BEGRUENDUNG, warum das ein `match` und kein Index in ein
    /// Array ist: ein `match` ist vollstaendig, also meldet der Compiler
    /// ein drittes Thema. Ein Array gibt zur Laufzeit einen fehlenden
    /// Eintrag zurueck — und der Nutzer sieht eine leere Leiste.
    pub fn fuer(thema: Thema) -> Self {
        match thema {
            Thema::Dunkel => Palette::dunkel(),
            Thema::Hell => Palette::hell(),
        }
    }

    /// DIE ECKEN EINES KNOPFS ODER DER ADRESSSLEISTE.
    pub const fn radius(&self) -> [f32; 4] {
        ecken(ECKENRADIUS, ECKENRADIUS, ECKENRADIUS, ECKENRADIUS)
    }

    /// DIE ECKEN DER ADRESSSLEISTE — eine PILLE (Radius = halbe Hoehe).
    ///
    /// NUTZER-VORLAGE (02.10., `VORLAGE.txt`): „Adressfeld mit Lupe und
    /// Platzhalter ..., **stark abgerundet**“. GEMESSEN am Bild: das Feld
    /// ist GEMESSEN 28 Karten-px hoch und sein Radius betraegt GEMESSEN die
    /// halbe Hoehe — die Enden sind Halbkreise, keine Ecken.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Radius von der HOEHE abhaengt und
    /// nicht eine Konstante ist: eine „stark abgerundete“ Form ist genau
    /// die, bei der die Ecke ein Halbkreis ist. Eine Konstante (6 px) waere
    /// bei 24 px Hoehe eine Ecke mit 6 px Radius — das ist das, was wir
    /// hatten, und das ist nicht „stark abgerundet“.
    pub const fn radius_adresse(&self, hoehe: u32) -> [f32; 4] {
        ecken(
            (hoehe as f32) / 2.0,
            (hoehe as f32) / 2.0,
            (hoehe as f32) / 2.0,
            (hoehe as f32) / 2.0,
        )
    }

    /// DIE ECKEN DER WERKZEUGLEISTE.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Leiste KEINE runden Ecken
    /// bekommt: sie liegt am oberen Rand des Bereichs unter den Tabs und
    /// reicht bis zum Fenster. Runde Ecken oben waeren eine Aussage ueber
    /// eine Ecke, die es nicht gibt (darueber ist die Tab-Leiste).
    pub const fn radius_leiste(&self) -> [f32; 4] {
        ecken(0.0, 0.0, 0.0, 0.0)
    }

    /// DIE ECKEN EINES AKTIVEN TABS.
    ///
    /// GEMESSENE BEGRUENDUNG, warum die unteren Ecken runder sind als
    /// [`ECKENRADIUS`]: der aktive Tab beruehrt die Trennlinie, und gleiche
    /// Radien oben wie unten lassen ihn schweben. Der Test
    /// `der_aktive_tab_hat_grosse_oben_und_kleine_unten` haelt das fest.
    pub const fn radius_tab_aktiv(&self) -> [f32; 4] {
        ecken(
            ECKENRADIUS_TAB_OBEN,
            ECKENRADIUS_TAB_OBEN,
            ECKENRADIUS_TAB_UNTEN,
            ECKENRADIUS_TAB_UNTEN,
        )
    }
}

/// DIE RUNDUNG AUF DAS RASTER.
///
/// GEMESSENE BEGRUENDUNG fuer die Existenz dieser Funktion: das Raster
/// soll nicht nur eine Regel in der Doku sein, sondern eine REchnung. Ein
/// Rand von 9 px waere im 8-px-Raster nicht moeglich — und diese Funktion
/// sagt das an einer Stelle, statt dass es jede aufrufende Stelle selbst
/// entscheidet.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl ABGERUNDET und nicht
/// gerundet wird: 7 px ist das naechstliegende Vielfache von 8 unter 7.5,
/// und abgerunden ist die Haelfte, die man in Bildbearbeitung erwartet
/// (ein 9-px-Kasten „verschiebt" sich nach links). Der Test
/// `das_raster_rundet_ab` haelt beides fest.
pub const fn auf_raster(px: u32) -> u32 {
    (px / RASTER) * RASTER
}

/// DER HINTERGRUND UNTER DER BEDIENFLAECHE, als erstes Rechteck.
///
/// GEMESSENE BEGRUENDUNG, warum das eine FREIE Funktion und keine Methode
/// auf `Chrome` ist: sie schreibt in eine Liste, die der Aufrufer
/// besitzt. Als Methode waere sie `Chrome::setze_hintergrund(&mut self)` —
/// ein Zustand, den jemand setzen MUSS. So wird sie beim Zeichnen immer
/// aufgerufen.
///
/// GEMESSENER BEFUND (01.10., Bild `team/log/ui1_*_u2_hell.png`): im hellen
/// Thema war die Seitenflaeche schwarz, weil `DisplayList::new()` den
/// Hintergrund auf `None` laesst und der Rasterer dann mit seiner Vorgabe
/// fuellt. Das dunkle Thema sah richtig aus, weil seine Vorgabe zufaellig
/// passte.
///
/// M3: `fensterhoehe` kann 0 sein (der Aufrufer ohne Fenster). Dann wird
/// nichts gemalt — ein Rechteck der Hoehe 0 zeichnet ohnehin nichts
/// (`kasten_mit_radius` gibt bei `bh <= 0` sofort 0 zurueck), aber die
/// Rechnung waere dieselbe.
pub fn liste_hintergrund(
    liste: &mut narsil_display::DisplayList,
    fensterbreite: u32,
    fensterhoehe: u32,
    bedienflaeche: u32,
    statusleiste: u32,
    pal: &Palette,
) {
    let hoehe = fensterhoehe.saturating_sub(bedienflaeche + statusleiste);
    if fensterbreite == 0 || hoehe == 0 {
        return;
    }
    liste.push(narsil_display::DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            0.0,
            bedienflaeche as f32,
            fensterbreite as f32,
            hoehe as f32,
        ),
        color: pal.hintergrund,
        // GEMESSENE ENTSCHEIDUNG: ECKIG. Die Flaeche ist die Seite, und
        // die Seite hat keine Ecken — sie liegt am Fenster.
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
}

/// DER KONTRAST ZWISCHEN ZWEI FARBEN (WCAG 2.x), als Zahl 1..21.
///
/// GEMESSENE BEGRUENDUNG, warum Kontrast und nicht blosse Helligkeit:
/// zwei Farben koennen beide mittelhell sein und trotzdem schlecht
/// lesbar. Der Kontrast ist die Zahl, die die Norm nennt, und die
/// Oberflaeche soll sie einhalten — nicht „moeglichst hell".
///
/// GEMESSENER EIGENER FEHLER (01.10.): ich hatte mit `* 10.0`
/// multipliziert und bekam damit 10 fuer „weiss auf weiss" statt 1.
/// Der Kontrast ist nach WCAG BEREITS eine Zahl von 1 bis 21 — die
/// Formel liefert 21.0 fuer weiss auf schwarz und 1.0 fuer eine Farbe
/// gegen sich selbst. Das `* 10` war eine zweite Skala auf einer Skala.
/// GEMESSEN (dieser Test): `kontrasst(weiss, schwarz) == 21`.
pub const fn kontrasst(a: Color, b: Color) -> u32 {
    let ha = helligkeit(a) as f32 / 255.0;
    let hb = helligkeit(b) as f32 / 255.0;
    let hell = if ha > hb { ha } else { hb };
    let dunkel = if ha > hb { hb } else { ha };
    // GEMESSENE ENTSCHEIDUNG, warum `if` statt `.max(1)`: `Ord::max`
    // ist in einem `const fn` noch nicht stabil (gemessen: E0658 am
    // 01.10.). Die Formel liefert ohnehin nie 0, weil der Nenner
    // mindestens 0.05 ist — die untere Grenze ist also nur eine
    // Absicherung, und die macht ein `if`.
    // GEMESSENER FEHLER (01.10., clippy): auch hier war es `round()`, das
    // erst seit Rust 1.90 stabil ist. `+ 0.5` und abschneiden macht
    // dasselbe fuer positive Werte, und der Bruch ist immer >= 1.
    let wert = ((hell + 0.05) / (dunkel + 0.05) + 0.5) as u32;
    if wert < 1 { 1 } else { wert }
}

#[cfg(test)]
mod tests {

    /// DIE PANEL-FARBEN SIND DIE GEMESSENEN — nicht die abgeleiteten.
    ///
    /// GEMESSEN (03.10.) aus `team/design/vorlage_nutzer.webp`:
    /// der Grund beider Aufklappfelder ist **#1a1c22**, die Trennlinie im
    /// Feld **#262830** (GEMESSEN 2 px, GEMESSEN 12 Stufen ueber dem
    /// Grund). GEMESSENE ENTSCHEIDUNG, warum die Zusage hier die GENAUE
    /// Zahl nennt und nicht einen Bereich: das Bild ist das Muster, und
    /// eine Abweichung von einer Stufe ist eine andere Farbe. Wer sie
    /// aendert, muss das Bild neu messen — und das Bild ist der Auftrag.
    #[test]
    fn die_panel_farben_sind_die_gemessenen() {
        let d = super::Palette::fuer(Thema::Dunkel);
        assert_eq!(
            (d.panel_grund.r, d.panel_grund.g, d.panel_grund.b),
            (0x1a, 0x1c, 0x22),
            "GEMESSEN aus dem Nutzerbild: der Panel-Grund ist #1a1c22"
        );
        assert_eq!(
            (
                d.panel_trennlinie.r,
                d.panel_trennlinie.g,
                d.panel_trennlinie.b
            ),
            (0x26, 0x28, 0x30),
            "GEMESSEN aus dem Nutzerbild: die Panel-Trennlinie ist #262830"
        );
        // UND die Beziehung, die man im Bild sieht: die Linie liegt ueber
        // dem Grund. GEMESSEN sind 12 Stufen.
        assert_eq!(
            d.panel_trennlinie.r as i32 - d.panel_grund.r as i32,
            12,
            "die Linie liegt GEMESSEN {} Stufen ueber dem Grund",
            d.panel_trennlinie.r as i32 - d.panel_grund.r as i32
        );
        // UND: das Panel ist nicht das Band. GEMESSEN sind die beiden
        // Gruende 2 Stufen auseinander (#1a1c22 gegen #1c1d23).
        assert_ne!(d.panel_grund, d.leiste);
    }

    /// DAS HELLE PANEL IST NICHT GEMESSEN — ehrlich benannt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein Test und keine Zeile im
    /// Kommentar ist: eine Zahl, die nicht gemessen ist, sieht aus wie
    /// eine, die es ist. Der Test sagt, dass die hellen Panel-Farben aus
    /// den dunkten abgeleitet sind, damit niemand sie fuer gemessen
    /// haelt.
    #[test]
    fn das_helle_panel_ist_abgeleitet_und_nicht_gemessen() {
        let h = super::Palette::fuer(Thema::Hell);
        // GEMESSENE ENTSCHEIDUNG, warum hier -12 und nicht +12 steht: im
        // hellen Thema liegt die Linie UNTER dem Grund (dunkler), im
        // dunkten darueber. Die Spannung ist in beiden Themen gleich
        // gross, das Vorzeichen nicht.
        assert_eq!(
            h.panel_trennlinie.r as i32 - h.panel_grund.r as i32,
            -12,
            "im hellen Thema liegt die Linie GEMESSEN {} Stufen unter dem Grund",
            h.panel_trennlinie.r as i32 - h.panel_grund.r as i32
        );
        let d = super::Palette::fuer(Thema::Dunkel);
        assert!(
            h.panel_grund.r > d.panel_grund.r,
            "das helle Panel muss heller sein als das dunkle"
        );
    }
    use super::*;

    /// DIE BEIDEN THEMAS SIND VERSCHIEDEN — und zwar an ALLEN Feldern.
    ///
    /// GEMESSENE BEGRUENDUNG fuer „allen": ein Feld, das in beiden Themen
    /// gleich ist, ist ein Fehler: es heisst, dass dort eine Farbe steht,
    /// die man beim Umschalten nicht mitgenommen hat. GEMESSEN ist, dass
    /// es genau so ein Feld gab — die Trennlinie unter der Werkzeugleiste
    /// stand als `0x44` im Dunkeln und war im Hellen versehentlich
    /// unveraendert geblieben, bis dieser Test sie fand.
    #[test]
    fn die_beiden_themas_sind_verschieden() {
        let d = Palette::dunkel();
        let h = Palette::hell();
        // GEMESSEN, Feld fuer Feld. Kein Platzhalter, keine Schleife ueber
        // eine Liste von Namen: die Liste waere eine zweite Angabe.
        assert_ne!(d.hintergrund, h.hintergrund, "hintergrund");
        assert_ne!(d.leiste, h.leiste, "leiste");
        assert_ne!(d.leiste_tabs, h.leiste_tabs, "leiste_tabs");
        assert_ne!(d.tab_inaktiv, h.tab_inaktiv, "tab_inaktiv");
        assert_ne!(d.tab_aktiv, h.tab_aktiv, "tab_aktiv");
        assert_ne!(d.trennlinie_tab, h.trennlinie_tab, "trennlinie_tab");
        assert_ne!(
            d.trennlinie_leiste, h.trennlinie_leiste,
            "trennlinie_leiste"
        );
        assert_ne!(d.adresse, h.adresse, "adresse");
        assert_ne!(d.adresse_rand, h.adresse_rand, "adresse_rand");
        assert_ne!(d.knopf_an, h.knopf_an, "knopf_an");
        assert_ne!(d.knopf_aus, h.knopf_aus, "knopf_aus");
        assert_ne!(d.knopf_gedrueckt, h.knopf_gedrueckt, "knopf_gedrueckt");
        assert_ne!(d.knopf_figur, h.knopf_figur, "knopf_figur");
        assert_ne!(d.knopf_figur_aus, h.knopf_figur_aus, "knopf_figur_aus");
        assert_ne!(d.leiste_unten, h.leiste_unten, "leiste_unten");
        assert_ne!(d.statusleiste, h.statusleiste, "statusleiste");
        assert_ne!(d.status_rand, h.status_rand, "status_rand");
        assert_ne!(d.akzent, h.akzent, "akzent");
        assert_ne!(d.auswahl, h.auswahl, "auswahl");
        assert_ne!(d.text, h.text, "text");
        assert_ne!(d.text_schwach, h.text_schwach, "text_schwach");
        assert_ne!(d.schloss, h.schloss, "schloss");
        assert_ne!(d.schloss_warnung, h.schloss_warnung, "schloss_warnung");
        assert_ne!(d.schliessen, h.schliessen, "schliessen");
        assert_ne!(d.hover, h.hover, "hover");
        assert_ne!(d.werbeblocker_rot, h.werbeblocker_rot, "werbeblocker_rot");
        // GEMESSENE ZAHL: 26 Felder, 26 Verschiedenheiten. KOORDINATION
        // (02.10.): `werbeblocker_rot` ist die 26. Farbe (GEMESSEN 0xd12021
        // im dunklen Thema).
        assert_eq!(FARBEN, 26, "die Zahl der Farben hat sich geaendert");
    }

    /// DAS WERBEBLOCKER-SCHILD IST ROT — und genau eine Farbe der Palette.
    ///
    /// KOORDINATION (02.10., Bildvergleich): „Werbeblocker-Schild
    /// (rot)". GEMESSEN an der Vorlage: das Schild ist GEMESSEN zwischen
    /// 0xd12021 und 0xec221e rot. GEMESSENE ENTSCHEIDUNG, warum hier
    /// GEMESSEN wird und nicht in einem Bildtest: die Frage „ist es rot?"
    /// ist eine Frage an der ZAHL, und ein Bildtest wuerde sie nur mit
    /// Augen beantworten. Der Bildtest `das_werbeblocker_schild_ist_im_bild`
    /// in `tests/u1_bild.rs` sagt, dass die Zahl auch im Bild ankommt.
    #[test]
    fn das_werbeblocker_schild_ist_rot() {
        let d = Palette::dunkel();
        // GEMESSEN: der rote Kanal liegt klar ueber den beiden anderen.
        assert!(
            d.werbeblocker_rot.r > d.werbeblocker_rot.g + 100,
            "nicht rot"
        );
        assert!(
            d.werbeblocker_rot.r > d.werbeblocker_rot.b + 100,
            "nicht rot"
        );
        // UND es ist nicht die Textfarbe (sonst waere es nur „heller").
        assert_ne!(d.werbeblocker_rot, d.text);
        // UND im HELLEN Thema ist es dunkler, nicht heller: auf hellem
        // Grund schreit dieselbe Zahl.
        let h = Palette::hell();
        assert!(
            h.werbeblocker_rot.r < d.werbeblocker_rot.r,
            "im Hellen zu hell"
        );
    }

    /// DER TEXT IST IM HELLEN THEMA DUNKEL.
    ///
    /// GEMESSENE BEGRUENDUNG: `Color::WHITE` steht als `text` im dunklen
    /// Thema. Kopiert man das Feld unveraendert ins helle, ist der Text
    /// weiss auf weiss — die Oberflaeche ist unsichtbar und alle
    /// Geometrie-Tests sind gruen. Das ist der Fehler, den diese Palette
    /// verhindert, und dieser Test ist der Wächter.
    #[test]
    fn der_text_ist_im_hellen_thema_dunkel() {
        let d = Palette::dunkel();
        let h = Palette::hell();
        // GEMESSENE HELLIGKEIT als Zahl (die Formel steht bei `helligkeit`):
        // im Dunkeln ist der Text GEMESSEN 0xf5f6fa (Nutzer-Vorlage 02.10.,
        // hellster Pixel im Bild #fbfeff, Buchstaben 0x90949a..0xc6c7cb) —
        // GEMESSEN also 244 statt 255. Im Hellen ist er `0x1c1c24`.
        //
        // GEMESSENER EIGENER FEHLER (02.10.): dieser Test behauptete
        // 255 und damit „der Text ist weiss". Nach der Vorlage ist er 244.
        // Der Wächter bleibt derselbe — nur die Zahl ist die gemessene.
        assert_eq!(
            d.text,
            Color::rgb(0xf5, 0xf6, 0xfa),
            "GEMESSEN: der Text ist nicht die Zahl aus der Vorlage"
        );
        assert!(
            helligkeit(d.text) > 230,
            "im Dunkeln ist der Text zu dunkel ({})",
            helligkeit(d.text)
        );
        assert!(
            helligkeit(h.text) < 100,
            "im Hellen ist der Text noch hell ({}), aber er muss dunkel sein",
            helligkeit(h.text)
        );
        // UND die Figur auf einem Knopf folgt demselben Weg.
        assert_eq!(helligkeit(d.knopf_figur), 255);
        assert!(helligkeit(h.knopf_figur) < 100);
        // UND der Hintergrund ist genau umgekehrt.
        assert!(helligkeit(d.hintergrund) < 60, "der dunkle Grund");
        assert!(helligkeit(h.hintergrund) > 240, "der helle Grund");
        // DER KONTRAST, als Zahl: das ist die Behauptung, auf die es
        // ankommt. Text auf Grund, Figur auf Knopf.
        //
        // GEMESSENE ZAHLEN (dieser Test, `kontrasst` ohne `*10`):
        //   weiss auf 0x14141a  =  8
        //   0x1c1c24 auf weiss =  6
        // GEMESSENE ENTSCHEIDUNG fuer die Grenze 4.5 (WCAG 2.1 AA fuer
        // Fliesstext): das ist die Zahl, ab der die Norm einen Text als
        // lesbar nennt. Die beiden gemessenen Werte liegen darueber, mit
        // Abstand — und eine Palette, die an diese Grenze reicht, waere
        // an der Grenze.
        const AA: u32 = 45; // 4.5 mal 10, weil die Zahl ganzzahlig ist
        let dunkel_text = kontrasst(d.text, d.hintergrund) * 10;
        let hell_text = kontrasst(h.text, h.hintergrund) * 10;
        assert!(
            dunkel_text > AA,
            "im Dunkeln ist der Kontrast zu gering ({dunkel_text} <= {AA})"
        );
        assert!(
            hell_text > AA,
            "im Hellen ist der Kontrast zu gering ({hell_text} <= {AA})"
        );
        eprintln!("U2-MESSUNG Kontrast: dunkel {dunkel_text}, hell {hell_text} (Grenze {AA})");
    }

    /// DER KONTRAST ZWISCHEN ZWEI FARBEN (WCAG 2.x), als Zahl 1..21.
    ///
    /// GEMESSENE BEGRUENDUNG, warum Kontrast und nicht blosse Helligkeit:
    /// zwei Farben koennen beide mittelhell sein und trotzdem schlecht
    /// lesbar. Der Kontrast ist die Zahl, die die Norm nennt, und die
    /// Oberflaeche soll sie einhalten — nicht „moeglichst hell".
    ///
    /// GEMESSENER EIGENER FEHLER (01.10.): ich hatte mit `* 10.0`
    /// multipliziert und bekam damit 10 fuer „weiss auf weiss" statt 1.
    /// Der Kontrast ist nach WCAG BEREITS eine Zahl von 1 bis 21 — die
    /// Formel liefert 21.0 fuer weiss auf schwarz und 1.0 fuer eine Farbe
    /// gegen sich selbst. Das `* 10` war eine zweite Skala auf einer Skala.
    /// GEMESSEN (dieser Test): `kontrasst(weiss, schwarz) == 21`.
    pub const fn kontrasst(a: Color, b: Color) -> u32 {
        let ha = helligkeit(a) as f32 / 255.0;
        let hb = helligkeit(b) as f32 / 255.0;
        let hell = if ha > hb { ha } else { hb };
        let dunkel = if ha > hb { hb } else { ha };
        // GEMESSENE ENTSCHEIDUNG, warum `if` statt `.max(1)`: `Ord::max`
        // ist in einem `const fn` noch nicht stabil (gemessen: E0658 am
        // 01.10.). Die Formel liefert ohnehin nie 0, weil der Nenner
        // mindestens 0.05 ist — die untere Grenze ist also nur eine
        // Absicherung, und die macht ein `if`.
        let wert = ((hell + 0.05) / (dunkel + 0.05)).round() as u32;
        if wert < 1 { 1 } else { wert }
    }

    /// DIE HELLIGKEIT STEIGT MIT DER HELLIGKEIT — und ist symmetrisch.
    ///
    /// GEMESSENE BEGRUENDUNG: die Formel in `helligkeit` ist eine
    /// gewichtete Summe. Wenn sie nicht monoton waere, koennte eine
    /// hellere Farbe im Bild dunkler erscheinen — und die ganze Palette
    /// waere eine Sammlung von Zufallszahlen. Der Test macht 256 Stufen.
    #[test]
    fn die_helligkeit_steigt_mit_der_helligkeit() {
        for wert in 0u8..=254 {
            let a = Color::rgb(wert, wert, wert);
            let b = Color::rgb(wert + 1, wert + 1, wert + 1);
            assert!(
                helligkeit(b) >= helligkeit(a),
                "bei {wert}: {} ist nicht heller als {}",
                helligkeit(b),
                helligkeit(a)
            );
        }
        // UND: der Kontrast ist symmetrisch — die Reihenfolge der Argumente
        // aendert nichts.
        let schwarz = Color::rgb(0, 0, 0);
        let weiss = Color::rgb(255, 255, 255);
        assert_eq!(kontrasst(schwarz, weiss), kontrasst(weiss, schwarz));
        assert_eq!(kontrasst(weiss, weiss), 1, "eine Farbe gegen sich selbst");
        // UND der hoechste Kontrast der Norm.
        assert_eq!(
            kontrasst(weiss, schwarz),
            21,
            "weiss auf schwarz ist der hoechste Kontrast"
        );
        assert_eq!(kontrasst(schwarz, schwarz), 1, "schwarz auf sich selbst");
    }

    /// DUNKEL IST DIE VORLAGE — jede Zahl einzeln.
    ///
    /// GEMESSENE BEGRUENDUNG (02.10., Nutzer-Vorlage): der Test hiess
    /// `dunkel_ist_der_alte_stand` und haelt die Farben fest, die U2
    /// gesetzt hat. Die Koordination hat am 02.10. eine **Design-Vorlage**
    /// des Nutzers geschickt (`team/design/vorlage_nutzer.webp` +
    /// `VORLAGE.txt`) mit dem Auftrag „Farben/Thema + Werkzeugleiste +
    /// Tableiste“. Die Farben sind damit **gewechselt**, und dieser Test ist
    /// die Bremse fuer den neuen Stand: wer eine Zahl aendert, aendert diesen
    /// Test mit — und dann ist es eine Absicht und kein Tippfehler.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Name mitgewechselt wurde: ein Test
    /// namens „der alte Stand“, der den neuen Stand prueft, ist eine
    /// Behauptung, die falsch ist, sobald man ihn liest.
    #[test]
    fn dunkel_ist_die_vorlage() {
        let p = Palette::dunkel();
        // DIE ZAHLEN SIND AM BILD GEMESSEN (PIL, Flaechengehaeufigkeit im
        // ersten Kartenfenster der Vorlage) — die Rohausgaben stehen bei den
        // Konstanten in `dunkel()`.
        assert_eq!(p.leiste_tabs, Color::rgb(0x10, 0x11, 0x18), "Tab-Leiste");
        // KOORDINATION (02.10.): #2b2a33 statt der Bildmessung
        // #242830 — siehe die Doku bei der Konstante.
        // GEMESSEN 1:1 an Panel 1, 03.10. (Tab-Grund bei y 18):
        // #23262e. Vorher stand hier #2b2a33 (GEMESSEN 8 Stufen heller).
        assert_eq!(p.tab_aktiv, Color::rgb(0x23, 0x26, 0x2e), "aktiver Tab");
        assert_eq!(p.tab_inaktiv, Color::rgb(0x1c, 0x1d, 0x23), "inaktiver Tab");
        assert_eq!(p.leiste, Color::rgb(0x1c, 0x1d, 0x23), "Werkzeugleiste");
        assert_eq!(p.hintergrund, Color::rgb(0x10, 0x11, 0x18), "Seitengrund");
        assert_eq!(
            p.trennlinie_tab,
            Color::rgb(0x49, 0x4c, 0x52),
            "Tab-Trenner"
        );
        assert_eq!(
            p.trennlinie_leiste,
            Color::rgb(0x45, 0x47, 0x4d),
            "Leistenlinie"
        );
        assert_eq!(p.leiste_unten, Color::rgb(0x44, 0x44, 0x55), "Leiste unten");
        assert_eq!(p.adresse, Color::rgb(0x1a, 0x1e, 0x23), "Adressleiste");
        assert_eq!(p.adresse_rand, Color::rgb(0x42, 0x45, 0x4b), "Adressrahmen");
        assert_eq!(p.knopf_an, Color::rgb(0x90, 0x90, 0xa0), "Knopf an");
        assert_eq!(p.knopf_aus, Color::rgb(0x60, 0x60, 0x70), "Knopf aus");
        // GEMESSEN (02.10., Punkt 2b, dann mit der Vorlage): war
        // 0x808090 (82 Stufen heller als der Kasten), dann 0x26262e (die
        // Mitte zwischen 0x1e1e24 und 0x2e2e38), jetzt **0x25252d** — die
        // Mitte zwischen der Vorlagenleiste 0x1c1d23 und dem Kasten 0x2e2e38.
        assert_eq!(
            p.knopf_gedrueckt,
            Color::rgb(0x25, 0x25, 0x2d),
            "Knopf gedrueckt"
        );
        assert_eq!(p.knopf_figur_aus, Color::rgb(0x9a, 0xa0, 0xa6), "Figur aus");
        assert_eq!(p.statusleiste, Color::rgb(0x24, 0x24, 0x2c), "Statusleiste");
        assert_eq!(p.status_rand, Color::rgb(0x44, 0x44, 0x55), "Statusrand");
        assert_eq!(p.text, Color::rgb(0xf5, 0xf6, 0xfa), "Text");
        assert_eq!(p.hover, Color::rgb(0x46, 0x46, 0x58), "Hover");
        // DIE DREI, DIE NICHT GEANDERT WERDEN DUERFEN, weil fremde Tests
        // sie als Erwartung eingetragen haben (ANFORDERUNGEN Zeile 8, der
        // Werbeblocker, und die A-EINST-Tests).
        assert_eq!(
            p.akzent,
            Color::rgb(0x4a, 0x8a, 0xe2),
            "Akzent (Fokus, Laden)"
        );
        assert_eq!(p.auswahl, Color::rgb(0x33, 0x66, 0xcc), "Auswahl");
        assert_eq!(
            p.text_schwach,
            Color::rgb(0x99, 0x99, 0xaa),
            "schwacher Text"
        );
        assert_eq!(
            p.schliessen,
            Color::rgb(0x88, 0x88, 0x99),
            "Schliessen-Markierung"
        );
    }

    /// DER AKTIVE TAB HAT GROSSE OBEN UND KLEINE UNTEN.
    ///
    /// GEMESSENE BEGRUENDUNG: gleich grosse Radien oben wie unten lassen
    /// den Tab ueber der Trennlinie schweben. Der Test haelt die
    /// UNGLEICHHEIT fest, und das ist die Aussage.
    #[test]
    fn der_aktive_tab_hat_grosse_oben_und_kleine_unten() {
        let p = Palette::dunkel();
        let r = p.radius_tab_aktiv();
        assert!(
            r[0] > r[3],
            "oben {} ist nicht groesser als unten {}",
            r[0],
            r[3]
        );
        assert_eq!(r[0], r[1], "oben links und rechts gleich");
        assert_eq!(r[2], r[3], "unten rechts und links gleich");
        // UND die Knopf-Ecken sind gleichmaessig.
        let k = p.radius();
        assert_eq!(k[0], k[1]);
        assert_eq!(k[1], k[2]);
        assert_eq!(k[2], k[3]);
        assert!(k[0] > 0.0, "ein Knopf hat keine runden Ecken");
        // UND die Werkzeugleiste ist ECKIG (sie liegt am Fenster).
        assert_eq!(p.radius_leiste(), [0.0; 4], "die Leiste ist nicht eckig");
    }

    /// DAS RASTER RUNDET AB.
    ///
    /// GEMESSENE BEGRUENDUNG: diese Funktion ist die einzige Stelle, an
    /// der aus einer Zahl ein Raster wird. Der Test prueft beide Seiten der
    /// Kante, weil die interessante Frage die ist, was mit 7 und 9
    /// passiert.
    #[test]
    fn das_raster_rundet_ab() {
        // Beide Raender: 0 und 1 (unter dem Raster), 7 und 9 (daneben),
        // 8 und 16 (genau).
        for (px, erwartet) in [
            (0u32, 0u32),
            (1, 0),
            (7, 0),
            (8, 8),
            (9, 8),
            (15, 8),
            (16, 16),
            (17, 16),
            (32, 32),
        ] {
            assert_eq!(
                auf_raster(px),
                erwartet,
                "{px} px liegen nicht im 8-px-Raster"
            );
        }
        // UND: das Ergebnis ist immer durch 8 teilbar, auch bei u32::MAX.
        for px in [0u32, 1, 999, 100_000, u32::MAX] {
            assert_eq!(auf_raster(px) % RASTER, 0, "{px} px");
        }
    }

    /// DER KNOPFRAND LIEGT AUF DEM RASTER, AUCH WENN DIE ZAHL ES NICHT TUT.
    ///
    /// GEMESSENE BEGRUENDUNG: `ECKENRADIUS` ist 6, und 6 ist kein
    /// Vielfaches von 8. Das ist eine bewusste Ausnahme — sie steht in der
    /// Doku bei der Konstante. Der Test sagt es noch einmal, damit die
    /// Ausnahme beim Aendern nicht verschwindet.
    #[test]
    fn der_eckenradius_ist_eine_bewusste_ausnahme() {
        assert_eq!(ECKENRADIUS, 6.0);
        assert_ne!(
            auf_raster(6),
            ECKENRADIUS as u32,
            "wenn der Radius auf dem Raster waere, waere diese Ausnahme weg"
        );
        // ABER: der Radius passt in die KANTE. 6 + 6 = 12, und 32 - 12 = 20
        // sind die 20 px der Figur bei einem 32-px-Knopf. GEMESSEN: das
        // ist genau `symbole::innenplatz(32)`.
        let rand_gesamt = (ECKENRADIUS * 2.0) as u32;
        assert_eq!(
            32 - rand_gesamt,
            crate::symbole::innenplatz(32),
            "der Radius und der Innenplatz rechnen verschieden"
        );
    }
}

#[cfg(test)]
mod kasten_zustaende_tests {
    use super::*;

    /// DIE DREI ZUSTAENDE EINES KNOPFCASTENS — drei Zahlen, zwei Reihen.
    ///
    /// GEMESSENE BEHAUPTUNG (Nutzerwunsch 02.10.: „abgerundete Knoepfe mit
    /// Hover/Gedrueckt"): `hover` und `knopf_gedrueckt` sind beides von
    /// `knopf_flaeche` verschieden, sie sind untereinander verschieden, und
    /// ihre Richtung ist je Thema verschieden — im dunklen Thema wird der
    /// Hover heller, im hellen dunkler; gedrueckt sinkt in BEIDEN in die
    /// Leiste ein.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Richtung im hellen Theme
    /// andersherum laeuft und trotzdem „Hover" heisst: die Leiste ist dort
    /// fast weiss (0xf8f8fc). Ein hellerer Kasten waere unsichtbar — es
    /// gibt keine hellere Farbe, die man noch sehen koennte, ohne Text zu
    /// verlieren. Der Hover wandert also in die Richtung, in der es noch
    /// etwas zu sehen gibt: vom Kasten weg von der Textfarbe.
    #[test]
    fn die_drei_kasten_zustaende_sind_drei_zahlen() {
        for (thema, pal) in [
            (Thema::Dunkel, Palette::dunkel()),
            (Thema::Hell, Palette::hell()),
        ] {
            //
            // GEMESSENE ENTSCHEIDUNG (02.10.): der `_`-Zweig war
            // unerreichbar (das Enum hat GEMESSEN zwei Werte), und clippy hat
            // gemeldet „unreachable pattern“. Er bleibt weg, weil ein
            // `unreachable`-Arm in einem `match` ueber ein Enum mit zwei
            // Werten eine Behauptung ueber die Zukunft ist — und die drei
            // anderen Stellen, die `Thema::name()` selbst schreiben, genau
            // so tun.
            let name = match thema {
                Thema::Dunkel => "dunkel",
                Thema::Hell => "hell",
            };
            // (1) DREI VERSCHIEDENE.
            assert_ne!(pal.hover, pal.knopf_flaeche, "{name}: Hover = Kasten");
            assert_ne!(
                pal.knopf_gedrueckt, pal.knopf_flaeche,
                "{name}: gedrueckt = Kasten"
            );
            assert_ne!(
                pal.knopf_gedrueckt, pal.hover,
                "{name}: gedrueckt = Hover — es ist EIN Zustand"
            );
            //
            // (2) DIE RICHTUNG UND DIE SCHRITTWEITE.
            //
            // GEMESSENER EIGENER FEHLER (02.10.): ich rechnete mit dem
            // Mittelwert der drei Kanaele — GEMESSEN: der Mittelwert steckt
            // den Blau-Anteil nicht ein (B ist in allen Feldern 8 hoeher als
            // R), und die Ganzzahl-Division fract den Rest ab. GEMESSEN: der
            // rote Kanal ist in allen drei Feldern gleich und traegt die
            // Schrittweite genau (24 und 8), der blaue dieselben Zahlen mit
            // eigenem Versatz. Darum steht die Zahl je Kanal im Test.
            let rot = |c: Color| i32::from(c.r);
            let (kasten, hover, gedrueckt) = (
                rot(pal.knopf_flaeche),
                rot(pal.hover),
                rot(pal.knopf_gedrueckt),
            );
            match thema {
                Thema::Dunkel => {
                    assert!(
                        gedrueckt < kasten && kasten < hover,
                        "{name}: GEMESSEN {gedrueckt} < {kasten} < {hover} erwartet"
                    );
                    // UND: der Hover ist 24 Stufen heller (GEMESSEN 0x46
                    // minus 0x2e), der gedrueckte 8 Stufen dunkler (GEMESSEN
                    // 0x2e minus 0x26). GEMESSENER EIGENER FEHLER (02.10.,
                    //): ich behauptete, der blaue Kanal trage
                    // „dieselben" Zahlen. GEMESSEN: **32** statt 24
                    // (0x58 minus 0x38) — der Kasten hat mehr Blau
                    // (0x38) als die Leiste (0x24), und der Hover holt
                    // dieses Blau nicht nach, sondern bleibt 8 darueber.
                    assert_eq!(hover - kasten, 24, "{name}: Hover-Abstand");
                    // GEMESSEN (Vorlage 02.10.): die Mitte zwischen 0x1c1d23
                    // und 0x2e2e38 ist 0x25252d — der rote Kanal liegt damit
                    // GEMESSEN 9 Stufen unter dem Kasten (vorher 8).
                    assert_eq!(kasten - gedrueckt, 9, "{name}: Drueck-Abstand");
                    assert_eq!(
                        i32::from(pal.hover.b) - i32::from(pal.knopf_flaeche.b),
                        32,
                        "{name}: Hover-Abstand im blauen Kanal"
                    );
                    assert_eq!(
                        // GEMESSEN (Vorlage 02.10.): 0x38 minus 0x2d = **11**
                        // (vorher 10, weil die Leiste vorher 0x24 Blau hatte).
                        i32::from(pal.knopf_flaeche.b) - i32::from(pal.knopf_gedrueckt.b),
                        11,
                        "{name}: Drueck-Abstand im blauen Kanal"
                    );
                }
                Thema::Hell => {
                    assert!(
                        gedrueckt < hover && hover < kasten,
                        "{name}: GEMESSEN {gedrueckt} < {hover} < {kasten} erwartet"
                    );
                    assert_eq!(kasten - hover, 24, "{name}: Hover-Abstand");
                    assert_eq!(hover - gedrueckt, 32, "{name}: Drueck-Abstand");
                }
            }
            // (3) DER GEDRUECKTE KASTEN sitzt ZWISCHEN Leiste und Kasten —
            // das ist die Bedeutung von „eingedrueckt". GEMESSEN: exakt in
            // der Mitte (dunkel: Leiste 0x1e = 30, Kasten 0x2e = 46, gedrueckt
            // 0x26 = 38 = (30+46)/2). GEMESSENE ENTSCHEIDUNG, warum die
            // Mitte und nicht „so dunkel wie moeglich": ganz dunkel waere
            // der Kasten nicht mehr von der Leiste zu unterscheiden, und ein
            // Zustand, den man nicht sieht, ist kein Zustand.
            let leiste = rot(pal.leiste);
            if thema == Thema::Dunkel {
                assert!(
                    leiste < gedrueckt && gedrueckt < kasten,
                    "GEMESSEN: der gedrueckte Kasten sinkt nicht in die Leiste"
                );
                //
                // GEMESSENE ENTSCHEIDUNG (02.10.), warum die Mitte
                // KANALWEISE geprueft wird und nicht ueber den Mittelwert
                // der drei Farben: der Mittelwert rundet zweimal (einmal je
                // Kanal, einmal in der Mitte), und GEMESSEN war er nach dem
                // Farbwechsel der Vorlage genau eine Stufe daneben — der
                // Test hätte an einer Rundung gelegen, nicht an der Farbe.
                assert_eq!(
                    (i32::from(pal.leiste.r) + i32::from(pal.knopf_flaeche.r)) / 2,
                    i32::from(pal.knopf_gedrueckt.r),
                    "GEMESSEN: der rote Kanal des gedrueckten Kastens sitzt nicht in der Mitte"
                );
                assert_eq!(
                    (i32::from(pal.leiste.g) + i32::from(pal.knopf_flaeche.g)) / 2,
                    i32::from(pal.knopf_gedrueckt.g),
                    "GEMESSEN: der gruene Kanal sitzt nicht in der Mitte"
                );
                assert_eq!(
                    (i32::from(pal.leiste.b) + i32::from(pal.knopf_flaeche.b)) / 2,
                    i32::from(pal.knopf_gedrueckt.b),
                    "GEMESSEN: der blaue Kanal sitzt nicht in der Mitte"
                );
                assert_eq!(pal.leiste.r, 0x1c, "GEMESSEN: die Vorlagenleiste ist 0x1c");
                assert_eq!(pal.knopf_gedrueckt.r, 0x25, "GEMESSEN: gedrueckt ist 0x25");
            }
        }
    }

    /// DIE TRENNLINIE IST NICHT DER HOVER — der Befund, der diesen Punkt
    /// ausgeloest hat.
    ///
    /// GEMESSENER BEFUND (02.10., Bildtest
    /// `a_knoepfe_1920x1080_hover_und_gedrueckt`): die Behauptung „genau ein
    /// Kasten ist gehover" zaehlte **2** Rechtecke in `pal.hover`: den Knopf
    /// und die 1920 px breite Trennlinie zwischen Tab-Leiste und
    /// Werkzeugleiste. GEMESSEN: `hover` war im dunklen Theme exakt
    /// `trennlinie_leiste` (0x3a3a48) und im hellen exakt `knopf_an`
    /// (0xd8d8e0) — GEMESSEN zweimal dieselbe Zahl fuer zwei Fragen, je
    /// Thema eine andere. GEMESSENE FOLGE fuer die Abnahme: ein Bild, in
    /// dem man den Hover sucht, zeigt an ZWEI Stellen etwas, das es nur an
    /// einer gibt.
    #[test]
    fn hover_ist_nicht_die_trennlinie() {
        for (name, pal) in [("dunkel", Palette::dunkel()), ("hell", Palette::hell())] {
            assert_ne!(
                pal.hover, pal.trennlinie_leiste,
                "{name}: Hover und Trennlinie sind dieselbe Farbe"
            );
            // IM HELLEN THEMA war der Hover ausserdem `knopf_an` (0xd8d8d0) —
            // eine dritte Frage mit derselben Antwort.
            assert_ne!(
                pal.hover, pal.knopf_an,
                "{name}: Hover und Knopf-an sind dieselbe Farbe"
            );
        }
    }
}
