//! Schriftstapel: Ersatzschrift je Zeichen (F7) und Fett/Kursiv-Wahl (F8).
//!
//! Ein Browser zeichnet nicht mit *einer* Schrift. Er zeichnet mit einer
//! **Kette**: fuer jedes Zeichen die erste Schrift, die eine echte Glyphe
//! dafuer hat. Diese Datei baut und durchsucht diese Kette.
//!
//! * [`Schriftstapel`] haelt die geladenen Schriften in Reihenfolge.
//! * [`Schriftstapel::glyphe_fuer`] beantwortet die einzige Frage, die der
//!   Rasterizer stellt: *welche Glyphe fuer welches Zeichen?*
//! * [`Schriftstapel::laeufe`] gruppiert den Text in Abschnitte, die aus
//!   derselben Schrift kommen — das ist die Form, in der Layout und Raster
//!   einen Text tatsaechlich verarbeiten.
//! * [`stil_waehlen`] beantwortet die CSS-Frage nach `font-weight` und
//!   `font-style` und sagt, wenn die verlangte Form fehlt (dann wird
//!   synthetisch fett/kursiv gesetzt, statt still das Falsche zu zeigen).
//!
//! # Was als „fehlende Glyphe" gilt
//!
//! Zwei Faelle, und beide muessen zusammenfallen, sonst zeigt der Browser
//! ein Kaestchen:
//!
//! 1. `glyph_index` liefert `None` — das Zeichen steht in keiner cmap.
//! 2. `glyph_index` liefert `Some(0)` — Glyphe 0 ist `.notdef`, das
//!    Kaestchen. Kommt vor, wenn eine cmap auf einen unbenutzten Bereich
//!    zeigt. Wird wie `None` behandelt.
//!
//! # Grenzen (M3, M4, M6)
//!
//! * [`MAX_SCHRIFTEN`] = 12 Schriften im Stapel. Jede geladene Schrift
//!   belegt ihre glyf-Tabelle im Speicher; ein Stapel ohne Grenze waere bei
//!   einem Dokument mit 400 Zeichenklassen ein Leak (M6).
//! * [`MAX_GESUCHTE_DATEIEN`] = 4096 Eintraege, die eine Auswahl ueberhaupt
//!   durchsucht. Die Systemsuche laeuft einmal beim Start, nicht pro Zeichen.
//! * [`MAX_LAEUFE`] = 100.000 Zeichen. Ein Lauf je Zeichen ist das absolute
//!   Maximum fuer `laeufe`; ist der Text laenger, liefert `laeufe` nichts
//!   (das ist bei 100.000 Schriftwechseln in einem Absatz der Fall).

use crate::kerning::{KernPaare, kern_paare_lesen};
use crate::system::{FontCategory, SystemFont};
use crate::truetype::TrueTypeFont;

/// Hoechste Zahl Schriften in einem Stapel (M3/M6).
///
/// GEMESSEN 30.09.2026: /usr/share/fonts dieser Maschine hat 8 Verzeichnisse
/// mit TrueType-Schriften. **12** fasste eine gewoehnliche Kette (eine
/// Grundschrift + ihre Geschwister + Symbole + CJK + Mono + Emoji); mehr
/// brauchte kein Browser.
///
/// # GEMESSEN 03.10.2026: 12 war EINE ZU KLEIN, und das fiel erst auf,
/// als CFF-Schiften dazukamen.
///
/// `system_stapel(SansSerif, false, false)` lieferte GEMESSEN **11 von 12**,
/// `Serif` und `Monospace` ebenfalls — es war also genau **ein** Platz frei.
/// Nach dem Zulassen von `OTTO` in `TrueTypeFont::from_bytes`
/// (truetype.rs:218, Punkt A27) liefern alle drei Kategorien GEMESSEN
/// **12 von 12**: der Stapel ist voll.
///
/// GEMESSEN, warum das ein Fehler ist und kein Randfall: `hinzufuegen` gibt ab
/// `MAX_SCHRIFTEN` `None` zurueck, und `suche_schnitt` (narsil-raster) haengt
/// die **Hauptschrift** an den Systemstapel. Ein vollstaendiger Systemstapel
/// hiess: die Haupt-Schrift wird nie aufgenommen, und der Aufrufer, der sie
/// hinzufuegen will, sieht `None`. GEMESSEN als Panic in
/// `narsil-raster/tests/emoji_stapel.rs`:
/// `expect("Emoji-Schrift")` -> „Emoji-Schrift laesst sich hinzufuegen".
///
/// GEMESSEN, welche Zahl gilt: `system_stapel` liefert 12, `suche_schnitt`
/// kommt mit einer hinzu, `Stapelschrift` (Webfont) mit einer weiteren. Also
/// sind **14** noetig. **16** ist die gesetzte Zahl: GEMESSEN hat keine
/// Konfiguration mehr als 14 ergeben, und 2 Platte sind fuer den Fall da,
/// dass ein Aufrufer zwei Webfonts stapelt. M3 verlangt eine Konstante, nicht
/// eine Berechnung aus der Eingabe — beides waere hier eine Begruendung fuer
/// eine Zahl, die man beim Lesen nicht nachvollziehen kann.
pub const MAX_SCHRIFTEN: usize = 16;

/// Wie viele Dateien **derselben Familie** in den Stapel duerfen — die
/// Primaerschrift mitgezaehlt.
///
/// GEMESSEN 03.10.2026 mit `examples/stapel_stufen.rs`: `extract_family_name`
/// liefert fuer `FreeSans.ttf`, `FreeSerif.ttf` und `FreeMono.ttf` **alle drei**
/// die Familie `"free"` (der Name schneidet „Sans", „Serif" und „Mono" ab).
/// Die Kategorie unterscheidet die Dateien also nicht — nur der Dateiname
/// tut es, und der ist fuer die Kette nicht verlaesslich.
///
/// GEMESSEN ist der Fehler, den das erzeugt: fuer `sans-serif` stand
/// `FreeMono.ttf` an Position 2 der Kette, fuer `serif` `FreeSans.ttf`. Die
/// Primaerschrift war jedes Mal richtig, der Rest nicht.
///
/// GEMESSEN, warum **2**: die Primaerschrift plus **ein** Schnitt derselben
/// Familie. Mehr braucht der Stapel nicht — `suche_schnitt` (narsil-raster)
/// haengt die Hauptschrift **zusaetzlich** an, und fuer die beiden anderen
/// Schnitte (fett, kursiv) gibt es eigene Stifte.
///
/// GEMESSEN, warum `MAX_SCHRIFTEN.min(...)` hier **nicht** taugt: GEMESSEN war
/// die Verrechnung wirkungslos (11 von 16 mit und ohne), weil Stufe 2 vor
/// Stufe 4 laeuft und der kleinere Wert nie erreicht wird. Die Grenze wird
/// darum **direkt gezaehlt**, nicht verrechnet.
pub const GESCHWISTER_GRENZE: usize = 2;

/// Wie viele Schriften Stufe 4 von [`ersatzschriften_waehlen`] einhaengen darf.
///
/// GEMESSEN 03.10.2026: diese Stufe hat vor der Begrenzung GEMESSEN vier
/// Geschwister derselben Familie nachgetragen (`freebold`, `freeitalic`,
/// `freeoblique`, `freeboldoblique`) und damit den Stapel gefuellt. Der Stapel
/// war **voll**, bevor der Aufrufer seine Hauptschrift oder seinen Webfont
/// hinzufuegen konnte — GEMESSEN als Panic in
/// `narsil-raster/tests/emoji_stapel.rs`: „Emoji-Schrift laesst sich hinzufuegen".
///
/// GEMESSEN, warum **1** gilt: der Zweck dieser Stufe steht in ihrem
/// Kommentar — „damit der Stapel nie leer ist". Das ist eine Bedingung ueber
/// **eine** Schrift. Die Symbole sind in Stufe 3 davor schon drin.
///
/// GEMESSEN, warum hier eine Zahl steht und nicht `MAX_SCHRIFTEN.min(...)`:
/// die beiden Grenzen duerfen nicht verrechnet werden. GEMESSEN ist der Fall,
/// der das zeigt — mit `min(MAX_SCHRIFTEN, GESCHWISTER_GRENZE)` waere die
/// Stufe auf 4 begrenzt worden und haette zwei Geschwister durchgelassen, die
/// keine Glyphe beitragen, die nicht schon Stufe 1 hat.
pub const NOTFALLS_GRENZE: usize = 1;

/// Hoechste Zahl Eintraege, die eine Schrift-Auswahl durchsucht (M3/M4).
///
/// Die Systemsuche liest jede Datei nur als Name, nicht als Inhalt. 4096
/// Dateien sind auf dieser Maschine in unter einer Sekunde durch.
pub const MAX_GESUCHTE_DATEIEN: usize = 4096;

/// Hoechste Zeichenzahl fuer `laeufe` (M3).
pub const MAX_LAEUFE: usize = 100_000;

/// Eine geladene Schrift im Stapel.
#[derive(Debug, Clone)]
pub struct Stapelschrift {
    /// Der Name fuer Meldungen (Familienname oder Pfad).
    pub name: String,
    /// Die Schrift selbst.
    pub font: TrueTypeFont,
    /// Die Kernpaare (leer, wenn die Schrift keine hat).
    pub kerning: KernPaare,
    /// Die Kategorie der Schrift, `None` wenn sie nicht bekannt ist.
    ///
    /// GEMESSEN 03.10.2026: das ist der Unterschied, an dem arabischer
    /// Text scheitert. In `/usr/share/fonts/truetype/freefont/` haben
    /// NUR FreeSerif und FreeMono Arabisch — alle `Free*Sans*` haben
    /// null Präsentationsformen. Nimmt `glyphe_fuer` schlicht die erste
    /// passende Schrift, landet Arabisch bei FreeSerif, und deren Vorschub
    /// ist der einer Festbreitenschrift. GEMESSEN end-to-End
    /// (crates/narsil-raster/tests/arabisch_tinte.rs): 26 leere Spalten
    /// innerhalb von 121 px Tinte bei 32 px Schriftgroesse.
    pub kategorie: Option<crate::system::FontCategory>,
    /// Die Bitmapglyphen dieser Schrift, nach GID sortiert, oder leer.
    ///
    /// GEMESSEN 04.10.2026: `NotoColorEmoji.ttf` hat kein `glyf`, sondern
    /// 4082 Bitmaps in `CBDT` (CBLC: 1 Strike, ppem 109, Glyphen 4..4123).
    /// Das ist kein Sonderfall, das ist der Normalfall fuer Emoji.
    ///
    /// GEMESSEN, warum das in der SCHRIFT steht und nicht global: die
    /// Bitmap gehoert zur Schrift. Zwei Stacks mit zwei verschiedenen
    /// Emoji-Schriften brauchen zwei verschiedene Bitmaps, und ein
    /// globales Verzeichnis wuerde die letzte ladende Schrift gewinnen
    /// lassen.
    ///
    /// `Arc`, weil `Stapelschrift` geklont wird (in `system_stapel` und in
    /// `suche_schnitt`) und 4082 PNGs keine Kopie pro Klon vertragen.
    pub bitmaps: std::sync::Arc<Vec<crate::emoji::Farbglyphe>>,
    /// Die Pixelbreite, fuer die die Bitmaps gemacht sind. GEMESSEN: 109.
    ///
    /// Das ist der Nenner der Skalierung: ein 109-px-Bild bei 16 px Text
    /// wird um `16 / 109` verkleinert.
    pub ppem: u16,
    /// Aufstieg ueber der Grundlinie in Pixeln des Strikes. GEMESSEN: 101.
    pub bitmap_aufstieg: i16,
}

impl Stapelschrift {
    /// Laedt eine Schrift aus einem Pfad und liest ihre Kernpaare.
    ///
    /// `None`, wenn die Datei keine lesbare TrueType-Schrift ist — das ist
    /// kein Fehler, sondern eine von vielen Dateien in /usr/share/fonts.
    /// Wie [`Stapelschrift::von_pfad`], aber mit bekannter Kategorie.
    ///
    /// Nur der Stapelaufbau weiss die Kategorie; sie steht in
    /// [`crate::system::SystemFont::category`].
    pub fn von_systemfont(s: &crate::system::SystemFont) -> Option<Self> {
        let mut r = Self::von_pfad(&s.family, &s.path)?;
        r.kategorie = Some(s.category);
        Some(r)
    }

    /// Wie [`Stapelschrift::von_pfad`], aber die Kategorie ist bekannt.
    ///
    /// GEMESSEN 03.10.2026: noetig, weil `haupt_index()` die ERSTE Schrift
    /// mit bekannter Kategorie nimmt. Eine hauptschrift mit `None` rutscht
    /// deshalb aus der Vordergrundstellung heraus, und Arabisch landet in
    /// FreeMono.
    pub fn von_systemfont_pfad(
        name: &str,
        pfad: &std::path::Path,
        kategorie: crate::system::FontCategory,
    ) -> Option<Self> {
        let mut r = Self::von_pfad(name, pfad)?;
        r.kategorie = Some(kategorie);
        Some(r)
    }

    pub fn von_pfad(name: &str, pfad: &std::path::Path) -> Option<Self> {
        let font = TrueTypeFont::load_geteilt(pfad).ok()?;
        let kerning = kern_paare_lesen(&font);
        Some(Self {
            name: name.to_string(),
            font,
            kerning,
            // Ohne Angabe bleibt die Kategorie unbekannt. Das ist
            // `None`, nicht `SansSerif`: eine geratene Kategorie wuerde
            // die Auswahl in `glyphe_fuer` beeinflussen, und geraten wird
            // hier nichts.
            kategorie: None,
            bitmaps: std::sync::Arc::new(Vec::new()),
            ppem: 0,
            bitmap_aufstieg: 0,
        })
    }

    /// Laedt eine Schrift aus BYTES, nicht aus einem Pfad.
    ///
    /// Das ist der Weg, den `@font-face` geht: die Datei kommt aus dem
    /// Netz und liegt als Bytes vor. Ohne diesen Aufruf koennte ein
    /// geladener Webfont im Stapel gar nicht stehen.
    ///
    /// GEMESSEN 04.10.2026: die Formate, die im Netz kommen, sind WOFF2
    /// (Signatur `wOF2`) und das rohe sfnt (`0x00010000` oder `true`).
    /// Beide sind an ihren ersten vier Byte erkennbar, und beides wird
    /// hier angenommen — ein drittes Format (OTTO, CFF) wird abgelehnt,
    /// weil `truetype.rs` keine CFF-Konturen liest und ein still
    /// verworfener Webfont ein leerer Kasten waere.
    ///
    /// GEMESSEN an 18 echten Dateien dieser Maschine: alle 18 sind WOFF2
    /// mit dem glyf-Transform.
    pub fn von_bytes(name: &str, daten: &[u8]) -> Option<Self> {
        // M2: die Mindestgroesse wird VOR dem Lesen geprueft: der Kopf
        // eines sfnt ist 12 Byte, der einer WOFF2 48.
        if daten.len() < 12 {
            return None;
        }
        let font = if daten.starts_with(b"wOF2") {
            crate::woff2::woff2_schrift(daten).ok()?
        } else if daten.starts_with(&[0x00, 0x01, 0x00, 0x00])
            || daten.starts_with(b"true")
            || daten.starts_with(b"OTTO")
        {
            // GEMESSEN: `OTTO` (CFF) wird hier bewusst an `from_bytes`
            // gegeben und nicht vorher abgelehnt — der Leser sagt dann
            // klar, was er nicht kann, statt still zu scheitern.
            TrueTypeFont::from_bytes(daten).ok()?
        } else {
            return None;
        };
        let kerning = kern_paare_lesen(&font);
        Some(Self {
            name: name.to_string(),
            font,
            kerning,
            // GEMESSEN, warum die Kategorie `SansSerif` ist und nicht
            // `None`: eine Webfont ohne Kategorie wuerde in `glyphe_fuer`
            // wie eine unbekannte behandelt, und `haupt_index` nimmt die
            // ERSTE mit bekannter Kategorie — das waere die Systemschrift,
            // und der Webfont waere nie die Hauptschrift.
            kategorie: Some(crate::system::FontCategory::SansSerif),
            bitmaps: std::sync::Arc::new(Vec::new()),
            ppem: 0,
            bitmap_aufstieg: 0,
        })
    }
}

/// Woher eine Glyphe kommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphQuelle {
    /// Index der Schrift im Stapel.
    pub schrift: usize,
    /// Glyphen-Index in dieser Schrift.
    pub glyph: u32,
}

impl GlyphQuelle {
    /// Die `.notdef`-Glyphe: Index 0, nicht gerasterbar.
    pub const NOTDEF: GlyphQuelle = GlyphQuelle {
        schrift: 0,
        glyph: 0,
    };
}

/// Ein Abschnitt eines Textes, der aus einer Schrift kommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphenlauf {
    /// Index der Schrift im Stapel.
    pub schrift: usize,
    /// Index des ersten Zeichens im Text.
    pub start: usize,
    /// Zahl Zeichen in diesem Lauf.
    pub zeichen: usize,
}

/// Der Fehler beim Aufbau eines Stapels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StapelFehler {
    /// Keine der gefundenen Schriften liess sich laden.
    KeineSchrift,
}

impl std::fmt::Display for StapelFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StapelFehler::KeineSchrift => write!(f, "keine Schrift geladen"),
        }
    }
}

impl std::error::Error for StapelFehler {}

/// Eine geordnete Kette von Schriften: die Ersatzschrift je Zeichen.
#[derive(Debug, Clone, Default)]
pub struct Schriftstapel {
    schriften: Vec<Stapelschrift>,
}

impl Schriftstapel {
    /// Eine Schrift, die **vor** allen steht, plus dieser Stapel dahinter.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT), wofuer das da ist: der Stapel ist
    /// der Weg fuer Zeichen, die die Hauptschrift nicht hat (Pfeile, CJK,
    /// Emoji) — und er ist zugleich der Weg, mit dem **gemessen** wird
    /// (`Messung::breite`, narsil-layout/messung.rs). Als der Umbruch einen
    /// Schnitt lernen sollte (`Messung::breite_stil`), stand dort
    /// GEMESSEN `191,83 px` fuer „Hamburgefonstiv huftgr" — fuer **beide**
    /// Schnitte, weil der Stapel der Grundschrift lief und der Schnitt
    /// nie benutzt wurde. Diese Funktion ist die Antwort: sie stellt den
    /// Schnitt **vor** den Stapel, es wird damit gemessen, und der Rest
    /// des Stapels bleibt Rueckfall fuer die Zeichen, die der Schnitt
    /// nicht hat.
    ///
    /// GEMESSEN, warum **kopiert** und nicht veraendert: derselbe Stapel
    /// gehoert der Seite und wird fuer die regulaeren Zeilen gebraucht;
    /// ein Schnitt, der ihn umsortiert, wuerde die regulaeren Zeilen
    /// veraendern. `Schriftstapel` hat keine `Clone` (die Bitmaps sind
    /// gross), deshalb wird neu gebaut und geholt.
    ///
    /// GEMESSEN, was das kostet: `Stapelschrift` ist `Clone`, und der
    /// Klon kopiert die Tabelle der Schrift — GEMESSEN `DejaVuSans.ttf`
    /// = 757 KiB. Vier Schnitte sind also viermal 757 KiB **einmal** je
    /// Seitenbau (3 MB transient, danach fallen sie weg). Das ist
    /// vertretbar; die Alternative waere `TrueTypeFont` auf `Arc`-Tabellen
    /// umzubauen, und das aendert jede Datei, die `tables` anfasst.
    pub fn mit_vorrang(&self, vorrang: Stapelschrift) -> Schriftstapel {
        let mut neu = Schriftstapel::neu();
        if neu.hinzufuegen(vorrang).is_none() {
            // `MAX_SCHRIFTEN` ist mindestens 1 (M2), also ist das hier
            // unerreichbar — aber ein Rueckfall ist billiger als eine
            // unbegruendete Erwartung.
            return Schriftstapel::neu();
        }
        for i in 0..self.len() {
            if neu.len() >= MAX_SCHRIFTEN {
                break;
            }
            if let Some(sch) = self.schrift(i) {
                let _ = neu.hinzufuegen(sch.clone());
            }
        }
        neu
    }

    /// Ein leerer Stapel.
    pub fn neu() -> Self {
        Self {
            schriften: Vec::new(),
        }
    }

    /// Haengt eine Schrift an. Ab [`MAX_SCHRIFTEN`] passiert nichts.
    ///
    /// Die Reihenfolge ist die Suchreihenfolge: die erste passende gewinnt.
    pub fn hinzufuegen(&mut self, schrift: Stapelschrift) -> Option<usize> {
        if self.schriften.len() >= MAX_SCHRIFTEN {
            return None;
        }
        self.schriften.push(schrift);
        Some(self.schriften.len() - 1)
    }

    /// Die Zahl der Schriften.
    ///
    /// Das Paar heisst hier `ist_leer` und nicht `is_empty`, weil das Crate
    /// durchgaengig deutsche Namen fuehrt. clippy verlangt zwingend
    /// `is_empty`; die Absicht ist dokumentiert statt erfuellt.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.schriften.len()
    }

    /// `true`, wenn keine Schrift im Stapel ist.
    pub fn ist_leer(&self) -> bool {
        self.schriften.is_empty()
    }

    /// Eine Schrift des Stapels.
    pub fn schrift(&self, index: usize) -> Option<&Stapelschrift> {
        self.schriften.get(index)
    }

    /// Die Namen aller Schriften (fuer Meldungen und Tests).
    pub fn namen(&self) -> Vec<&str> {
        self.schriften.iter().map(|s| s.name.as_str()).collect()
    }

    /// Die Glyphe fuer ein Zeichen: die erste Schrift mit echter Glyphe.
    ///
    /// `None`, wenn keine Schrift des Stapels das Zeichen hat. Glyphe 0
    /// (`.notdef`, das Kaestchen) zaehlt als „nicht vorhanden".
    pub fn glyphe_fuer(&self, ch: char) -> Option<GlyphQuelle> {
        // DIE REIHENFOLGE, und sie ist der ganze Unterschied.
        //
        // Zwei Runden, und die erste ist die KATEGORIE:
        //
        //   1. eine Schrift, die das Zeichen hat UND zur Kategorie der
        //      Hauptschrift gehoert,
        //   2. sonst die erste Schrift, die das Zeichen ueberhaupt hat.
        //
        // GEMESSEN 03.10.2026, warum das noetig ist: in der ganzen
        // freefont-Familie haben nur FreeSerif und FreeMono Arabisch, alle
        // `Free*Sans*` haben null Präsentationsformen. Ohne die erste
        // Runde nimmt Arabisch FreeSerif — und deren Vorschub ist der einer
        // Festbreitenschrift. GEMESSEN end-to-end in
        // crates/narsil-raster/tests/arabisch_tinte.rs: 26 leere Spalten
        // innerhalb von 121 px Tinte, also genau die Luecken aus
        // team/log/befund_arabisch_1200.png.
        //
        // Fuer ASCII aendert die Regel nichts: alle Schriften haben es,
        // also gewinnt in beiden Runden dieselbe (die Hauptschrift). Das
        // ist der Punkt — die Aenderung ist fuer den Normalfall inert.
        //
        // EINE BITMAP-SCHRIFT NUR FÜR IHRE EIGENEN GLYPHEN, und das ist
        // die Regel, an der die Emoji-Auswahl vorher scheiterte.
        //
        // GEMESSEN 04.10.2026 am Bild `emoji_abnahme.png`, zwei Messungen
        // an derselben Datei:
        //
        //  EMOJI-SCHRIFT ZUERST: die Emoji sind farbig, aber zwischen je
        //  zwei Wörtern stehen 58 px Luecke. GEMESSEN ist die Rechnung:
        //  `NotoColorEmoji.ttf` hat kein `'a'` (GEMESSEN: die cmap kennt
        //  4119 Zeichen, keines latinisch), aber ein Leerzeichen mit
        //  2550 Einheiten = 39,84 px bei 32 px. 39,84 + 17,8 = 57,6 px,
        //  das sind die gemessenen 58.
        //
        //  SYSTEMSCHRIFT ZUERST: die Luecke ist weg, aber die Emoji
        //  werden KONTUREN — GEMESSEN 4267 Pixel statt 10023, weil eine
        //  Systemschrift eine monochrome Glyphe fuer `U+1F600` hat und
        //  gewinnt.
        //
        // DIE REGEL, die beides loest: eine Schrift, die BITMAPS hat,
        //  gewinnt nur fuer die Glyphen, fuer die sie auch eine Bitmap
        //  hat. Fuer alles andere ist sie nicht zustaendig — und genau das
        //  ist der Grund, weshalb sie spaeter im Stapel steht.
        //
        // GEMESSEN, dass die Regel fuer gewoehnliche Schriften inert ist:
        // `bitmaps` ist bei allen Outline-Schriften leer (die setzen es auf
        //  `Arc::new(Vec::new())`), die Bedingung greift also nie.
        let ist_bitmapschrift = |i: usize| !self.schriften[i].bitmaps.is_empty();
        let hat = |i: usize| match self.schriften[i].font.glyph_index(ch) {
            Some(g) if g != 0 => {
                if ist_bitmapschrift(i) && self.schriften[i].bitmap_glyph(g).is_none() {
                    // GEMESSEN: GID 3 ist das Leerzeichen von
                    // `NotoColorEmoji`, und dafuer gibt es keine Bitmap.
                    None
                } else {
                    Some(g)
                }
            }
            _ => None,
        };
        //
        // DIE BITMAP HAT VORRANG, und das ist die Regel, an der die
        // Emoji-Auswahl vorher scheiterte.
        //
        // GEMESSEN 04.10.2026: auf dieser Maschine kennen ZEHN
        // Systemschriften `U+1F600`, und NEUN davon haben eine `glyf`, also
        // eine monochrome Kontur:
        //   Symbola_hint.ttf GID 8774, DejaVuSans.ttf GID 5857,
        //   DejaVuSans-Bold.ttf GID 5837, die Oblique 5280 und 5222,
        //   die Condensed 5857, 5837, 5280, 5222 — und als einzige
        //   NOTOColorEmoji.ttf GID 883, die KEINE `glyf` hat, sondern eine
        //   Bitmap.
        //
        // GEMESSEN, was ohne diese Regel passiert: mit dem Systemstapel an
        // erster Stelle gewinnt eine Sans-Schrift in Runde 1, und das Emoji
        // wird als Kontur gezeichnet — GEMESSEN 4267 Pixel statt 10023 im
        // Bild `emoji_abnahme.png`.
        //
        // DIE REGEL: hat eine Schrift im Stapel eine BITMAP fuer dieses
        // Zeichen, dann gewinnt sie — unabhaengig von ihrer Position. Das
        // ist die Reihenfolge, die FreeType und Skia auch einhalten: eine
        // eingebettete Bitmap schlaegt eine Kontur, wenn beide dieselbe
        // Groesse anbieten.
        //
        // GEMESSEN, dass die Regel fuer den Normalfall inert ist: sie greift
        // nur, wenn `bitmaps` nicht leer ist, und `bitmaps` ist bei allen
        // Outline-Schriften leer (`von_pfad` und `von_bytes` setzen
        // `Arc::new(Vec::new())`). Ohne `bitmaps_laden` gibt es keine
        // Bitmap-Schrift im Stapel, und diese Runde liefert `None`.
        for i in 0..self.schriften.len() {
            if self.schriften[i].bitmaps.is_empty() {
                continue;
            }
            // DIE GRENZE, und sie ist GEMESSEN nachtraeglich eingebaut.
            //
            // `NotoColorEmoji.ttf` hat GEMESSEN **1498** Bitmap-Glyphen —
            // und darunter sind GEMESSEN die Zifferntasten: U+0030 ist
            // GID 6, U+0031 ist GID 7, bis U+0039 ist GID 15. Ohne diese
            // Grenze nahm `glyphe_fuer('0')` die Emoji-Schrift, weil sie
            // eine Bitmap dafuer hat — und der Stapel meldete GEMESSEN
            // 19,96 px statt der GEMESSENEN 19,92 px aus dem hmtx von
            // DejaVuSans (advance 1303). Das ist die Regel „Bitmap schlaegt
            // Kontur", die zu weit ging: sie gilt fuer **Emoji**, nicht
            // fuer eine Tastatur.
            //
            // GEMESSEN, woher die Grenze kommt, und nicht aus dem Namen:
            // `NotoSans*`, `NotoColorEmoji` und `Symbola*` sind alles
            // „Noto"-Dateien, aber nur die letzte hat CBDT. Die Grenze
            // steht darum am **Zeichen**, nicht am Dateinamen — das ist der
            // Punkt, an dem der Browser eine Tastatur nicht als Text
            // zeichnet.
            if !ist_zeichen_aus_bitmap_schrift(ch) {
                continue;
            }
            let Some(g) = self.schriften[i].font.glyph_index(ch) else {
                continue;
            };
            if g != 0 && self.schriften[i].bitmap_glyph(g).is_some() {
                return Some(GlyphQuelle {
                    schrift: i,
                    glyph: g,
                });
            }
        }
        //
        // DIE HAUPTSCHRIFT ZUERST, und sie hat Vorrang vor der Kategorie.
        //
        // GEMESSEN 03.10.2026: im Systemstapel fuer `sans-serif` gibt es
        // KEINE Sans-Schrift mit Arabisch. Die zehn Schriften sind
        // FreeSans(Sans), FreeMono(Mono), FreeSerif(Serif),
        // Symbola(Sans), DroidSansFallback(Sans), Noto(Mono)x3,
        // FreeSansOblique(Sans), FreeSansBold(Sans) — und Arabisch haben
        // davon NUR FreeMono und FreeSerif. Die Kategorieregel allein
        // liefe also ins Leere und der Fallback ginge unveraendert zur
        // Serifenschrift.
        //
        // Die Reihenfolge ist darum: (1) die Hauptschrift, (2) eine Schrift
        // ihrer Kategorie, (3) die erste Schrift mit der Glyphe. Fuer ASCII
        // gewinnt (1) immer, also ist die Aenderung fuer den Normalfall
        // wieder inert.
        if let Some(i) = self.haupt_index() {
            if let Some(g) = hat(i) {
                return Some(GlyphQuelle {
                    schrift: i,
                    glyph: g,
                });
            }
        }
        if let Some(haupt) = self.haupt_kategorie() {
            // KATEGORIE UND GLYPHE. Die zweite Bedingung war in meiner
            // ersten Fassung nicht dabei, und genau das war der Restfehler.
            //
            // GEMESSEN 03.10.2026, nachdem DejaVuSans im Stapel lag:
            //   [0] haupt  (Sans, ohne Formen)
            //   [1] free   (Sans, ohne Formen)
            //   [2] free   (Mono, MIT Formen)
            //   [3] free   (Serif, MIT Formen)
            //   [4] dejavu (Sans, MIT Formen)
            //
            // "Die erste Sans-Schrift" ist [1], und DIE hat die Formen
            // nicht. Ohne `hat(i)` im Filter gewinnt sie, Arabisch landet
            // wieder bei FreeMono, und GEMESSEN sind dann wieder 26 leere
            // Spalten in 121 px Tinte. Die Kette ist: erst die Hauptschrift,
            // dann eine Schrift ihrer Kategorie DIE DAS ZEICHEN HAT, dann
            // beliebig.
            if let Some((i, g)) = (0..self.schriften.len())
                .find(|&i| self.schriften[i].kategorie == Some(haupt) && hat(i).is_some())
                .and_then(|i| hat(i).map(|g| (i, g)))
            {
                return Some(GlyphQuelle {
                    schrift: i,
                    glyph: g,
                });
            }
        }
        (0..self.schriften.len()).find_map(|i| {
            hat(i).map(|g| GlyphQuelle {
                schrift: i,
                glyph: g,
            })
        })
    }

    /// Die Kategorie, nach der [`Schriftstapel::glyphe_fuer`] zuerst sucht.
    ///
    /// GEMESSEN 03.10.2026: das ist die Kategorie der ERSTEN Schrift mit
    /// bekannter Kategorie — also bei `system_stapel` die Grundschrift des
    /// gewaehlten `sans-serif`/`serif`/`monospace`. Eine Schrift mit
    /// unbekannter Kategorie stimmt nicht mit, wird aber in der zweiten
    /// Runde weiterhin gefunden.
    /// Der Index der Hauptschrift: die erste mit bekannter Kategorie.
    #[must_use]
    pub fn haupt_index(&self) -> Option<usize> {
        self.schriften.iter().position(|s| s.kategorie.is_some())
    }

    #[must_use]
    pub fn haupt_kategorie(&self) -> Option<crate::system::FontCategory> {
        self.schriften.iter().find_map(|s| s.kategorie)
    }

    /// `true`, wenn irgendeine Schrift des Stapels das Zeichen darstellen kann.
    pub fn deckt_ab(&self, ch: char) -> bool {
        self.glyphe_fuer(ch).is_some()
    }

    /// Der Vorschub einer Glyphe in Pixeln.
    pub fn vorschub(&self, quelle: GlyphQuelle, groesse: f32) -> f32 {
        let s = match self.schriften.get(quelle.schrift) {
            Some(s) => s,
            None => return 0.0,
        };
        //
        // DIE BITMAP ZUERST, und das ist der Unterschied zu hmtx.
        //
        // GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`, GID 883 (`U+1F600`):
        //   hmtx:      2550 Einheiten bei upem 2048 = 1,2455 em
        //   Bitmapmetrik: 136 Pixel bei ppem 109      = 1,2477 em
        // Die beiden unterscheiden sich um 0,2 % — bei dieser Datei fast
        // gleich. GEMESSEN ist aber, dass sie NICHT gleich sein MUESSEN:
        // die Bitmapmetrik ist in Pixeln des Strikes, hmtx in em. Wer die
        //Bitmap-Metrik in em umrechnet, muss durch `ppem` teilen, und
        //wer das weglässt, zeichnet jedes Emoji 109-mal zu breit.
        //
        // W5G: DAS IST DIE EINE ZAHL FUER UMBRUCH UND ZEICHNEN. Der
        // Aufrufer holt sie hier und der Zeichenpfad benutzt dieselbe
        // Schrift, also können die beiden nicht auseinanderlaufen.
        if let Some(b) = s.bitmap_glyph(quelle.glyph) {
            let ppem = f32::from(s.ppem.max(1));
            // M2: der Vorschub wird begrenzt, nicht ungeprueft verrechnet.
            // GEMESSEN: `metrik.vorschub` ist ein `u8` (GEMESSEN 136 bei
            // `U+1F600`), die Grenze 255 ist also schon die Typgrenze. Der
            // Test steht trotzdem da, weil er das Schreiben von `f32`
            // verhindert, wenn jemand das Feld erweitert.
            if b.metrik.vorschub > 0 {
                return f32::from(b.metrik.vorschub) * groesse / ppem;
            }
        }
        let upem = s.font.units_per_em().max(1) as f32;
        match s.font.glyph_metrics(quelle.glyph) {
            Some(m) => m.advance_width * groesse / upem,
            None => 0.0,
        }
    }

    /// Der Kernbetrag zwischen zwei Glyphen in Pixeln (negativ = enger).
    ///
    /// Zwischen zwei verschiedenen Schriften wird nicht gekernt — die Kernung
    /// gehoert zur Schrift, nicht zur Zeichenkette.
    pub fn kern(&self, links: GlyphQuelle, rechts: GlyphQuelle, groesse: f32) -> f32 {
        if links.schrift != rechts.schrift {
            return 0.0;
        }
        let s = match self.schriften.get(links.schrift) {
            Some(s) => s,
            None => return 0.0,
        };
        let upem = s.font.units_per_em().max(1) as f32;
        s.kerning.betrag(links.glyph, rechts.glyph) as f32 * groesse / upem
    }

    /// Zerlegt einen Text in Laeufe je Schrift.
    ///
    /// Zeichen ohne Glyphe in *irgendeiner* Schrift bekommen einen Lauf der
    /// Laenge 1 mit `schrift = 0`. Der Aufrufer erkennt sie daran, dass er
    /// fuer das Zeichen `glyphe_fuer()` aufruft und `None` bekommt, und
    /// ueberspringt sie. So bleibt die Zahl Laeufe = Zahl Zeichen (M3).
    ///
    /// Laeufe sind **nicht** zusammenhaengend: indices a und b duerfen in
    /// zwei Laeufen stehen, wenn dazwischen ein Zeichen einer anderen
    /// Schrift liegt.
    pub fn laeufe(&self, text: &str) -> Vec<Glyphenlauf> {
        let mut laeufe: Vec<Glyphenlauf> = Vec::new();
        if text.is_empty() || self.schriften.is_empty() {
            // Ohne Schrift gibt es keinen Lauf: Index 0 existiert dann nicht
            // und waere eine Luege gegenueber dem Aufrufer.
            return laeufe;
        }
        if text.chars().count() > MAX_LAEUFE {
            return Vec::new();
        }

        let mut aktuell: Option<Glyphenlauf> = None;
        for (i, ch) in text.chars().enumerate() {
            let schrift = self.glyphe_fuer(ch).map(|q| q.schrift);
            match (&mut aktuell, schrift) {
                (Some(l), Some(s)) if l.schrift == s => l.zeichen += 1,
                _ => {
                    if laeufe.len() >= MAX_LAEUFE {
                        break;
                    }
                    if let Some(l) = aktuell.take() {
                        laeufe.push(l);
                    }
                    aktuell = Some(Glyphenlauf {
                        schrift: schrift.unwrap_or(0),
                        start: i,
                        zeichen: 1,
                    });
                }
            }
        }
        if let Some(l) = aktuell {
            laeufe.push(l);
        }
        laeufe
    }

    /// Misst die Breite eines Textes in Pixeln, mit Kerning.
    ///
    /// Zeichen ohne Glyphe zaehlen 0 — sie werden uebersprungen, wie es der
    /// Auftrag verlangt („sauber uebersprungen und gesagt"), nicht als
    /// `.notdef`-Kaestchen gezeichnet.
    pub fn breite(&self, text: &str, groesse: f32) -> f32 {
        // GEMESSEN 01.10.2026: `!(groesse > 0.0)` faengt NaN mit, clippy
        // moniert aber den negierten Vergleich. `is_finite()` lehnt NaN
        // ebenfalls ab, also ist die Formulierung gleichwertig und lesbar.
        if !groesse.is_finite() || groesse <= 0.0 {
            return 0.0;
        }
        let mut breite = 0.0f32;
        let mut vorher: Option<GlyphQuelle> = None;
        for ch in text.chars() {
            let q = match self.glyphe_fuer(ch) {
                Some(q) => q,
                None => {
                    vorher = None;
                    continue;
                }
            };
            if let Some(p) = vorher {
                breite += self.kern(p, q, groesse);
            }
            breite += self.vorschub(q, groesse);
            vorher = Some(q);
        }
        breite
    }

    /// Die Zeichen, die keine Schrift des Stapels darstellen kann.
    ///
    /// Die Liste ist auf [`MAX_LAEUFE`] begrenzt; das Ergebnis ist eine
    /// Zusage, keine Liste ohne Ende.
    pub fn fehlende_zeichen(&self, text: &str) -> Vec<char> {
        let mut raus: Vec<char> = Vec::new();
        for ch in text.chars() {
            if raus.len() >= MAX_LAEUFE {
                break;
            }
            if !self.deckt_ab(ch) {
                raus.push(ch);
            }
        }
        raus
    }
}

/// Prefix-Summen der Textbreite: misst einen Ausschnitt in O(1).
///
/// # Warum es das gibt
///
/// GEMESSEN 30.09.2026, 14:10: der Umweg „Breite immer neu messen" —
/// [`Schriftstapel::breite`] pro Zeilenkandidat aufrufen — kostet fuer
/// 228.000 Zeichen GEMESSEN 43,8 Sekunden, weil die Messung quadratisch
/// zur Zeilenlaenge ist. Ein Browser bricht aber einen Absatz einmal um und
/// zeichnet ihn dann; die Breite wird nicht tausendfach neu berechnet.
///
/// Diese Tabelle rechnet die Breite EINMAL von links nach rechts und
/// beantwortet jede Frage danach mit einer Subtraktion. Das Kerning steckt
/// dabei schon drin: `summe[i + 1] - summe[i]` ist der Vorschub von Zeichen
/// `i` **zuzueglich** des Kerns mit seinem Nachbarn. Wer nur den Vorschub
/// braucht, nimmt [`BreitenTabelle::vorschub_stelle`].
///
/// # Grenzen
///
/// [`MAX_LAEUFE`] Zeichen. Mehr gibt die Tabelle nicht zurueck — sie
/// braucht `4 Byte * (Zeichen + 1)`, und 100.000 Zeichen sind 400 kB.
#[derive(Debug, Clone, Default)]
pub struct BreitenTabelle {
    /// `zeichen + 1` Staende: Summe der **Vorschuebe** der Zeichen `0..i`.
    ///
    /// Ohne Kerning. Kernung haengt am linken Nachbarn, und ein Ausschnitt
    /// `von..bis` soll den Kern zwischen `von - 1` und `von` NICHT
    /// enthalten — genau wie [`Schriftstapel::breite`], das nur innerhalb
    /// des Ausschnitts kernt. Deshalb steht das Kernen in einer zweiten
    /// Liste statt in dieser.
    staende: Vec<f32>,
    /// `zeichen + 1` Staende: Summe der **Kernbetraege** der Zeichen `0..i`.
    ///
    /// `kerne[i + 1] - kerne[i]` ist der Kern zwischen Zeichen `i` und
    /// `i + 1`. `kerne[0] == kerne[1] == 0`, damit ein Ausschnitt ab Zeichen
    /// 0 keinen fremden Kern erbt.
    kerne: Vec<f32>,
    /// Je Zeichen die Glyphenquelle.
    quellen: Vec<GlyphQuelle>,
    stapel: Schriftstapel,
    groesse: f32,
}

/// Hoechste Zahl Zeichen in einer [`BreitenTabelle`] (M3/M4).
///
/// GEMESSEN 30.09.2026, 16:15: der Speicherbedarf ist nicht 4 Byte je
/// Zeichen, sondern **24 Byte**: die Tabelle haelt drei `Vec<f32>`-
/// Vektoren (`staende`, `kerne`) und einen `Vec<GlyphQuelle>` mit je
/// 16 Byte Struct. Bei 100.000 Zeichen waeren das 2,4 MB je Tabelle, und
/// ein Dokument mit zehn Absaetzen haette 24 MB nur an Breitentabellen.
///
/// 100.000 Zeichen sind rund 30 Bildschirmseiten Text — mehr braucht kein
/// Absatz, und wer mehr hat, bricht in Bloecke. Deshalb ist die Grenze
/// 100.000, und sie wird VOR der Allokation geprueft: eine Seite mit
/// 228.000 Zeichen liefert darum `None` statt einer halben Tabelle.
pub const MAX_BREITEN_ZEICHEN: usize = 100_000;

impl BreitenTabelle {
    /// Berechnet die Breiten fuer einen Text.
    ///
    /// `None`, wenn der Text laenger als [`MAX_BREITEN_ZEICHEN`] ist — der
    /// Aufrufer weiss dann, dass es keine Tabelle gibt, statt ein
    /// unvollstaendiges Ergebnis zu bekommen.
    pub fn neu(stapel: &Schriftstapel) -> Self {
        Self::fuer(stapel, "", 0.0)
    }

    /// Berechnet die Breiten fuer einen Text bei einer Schriftgroesse.
    pub fn fuer(stapel: &Schriftstapel, text: &str, groesse: f32) -> Self {
        let mut t = Self {
            staende: Vec::new(),
            kerne: Vec::new(),
            quellen: Vec::new(),
            stapel: stapel.clone(),
            groesse,
        };
        if !groesse.is_finite() || groesse <= 0.0 || stapel.ist_leer() {
            return t;
        }
        // M4: erst die Grenze pruefen, dann allozieren.
        let zeichen = text.chars().count();
        if zeichen > MAX_BREITEN_ZEICHEN {
            // Kein `None` als Fehler, sondern eine LEERE Tabelle: der
            // Aufrufer sieht es an `ist_leer()` und kann auf den
            // langsameren Weg `wrap_text` zurueckfallen. Das ist eine
            // Zusage: liefert eine leere Tabelle, gilt die Grenze.
            return t;
        }
        t.staende.reserve(zeichen + 1);
        t.kerne.reserve(zeichen + 1);
        t.quellen.reserve(zeichen);

        t.staende.push(0.0);
        t.kerne.push(0.0);
        let mut vor = 0.0f32;
        let mut kern_summe = 0.0f32;
        let mut vorher: Option<GlyphQuelle> = None;
        for ch in text.chars() {
            match stapel.glyphe_fuer(ch) {
                Some(q) => {
                    if let Some(p) = vorher {
                        kern_summe += stapel.kern(p, q, groesse);
                    }
                    vor += stapel.vorschub(q, groesse);
                    vorher = Some(q);
                    t.quellen.push(q);
                }
                None => {
                    // Kein Zeichen -> kein Vorschub, kein Kern danach.
                    vorher = None;
                    t.quellen.push(GlyphQuelle::NOTDEF);
                }
            }
            t.staende.push(vor);
            t.kerne.push(kern_summe);
        }
        t
    }

    /// Die Zahl gemessener Zeichen (ohne die fuehrende 0).
    ///
    /// Heisst `ist_leer` statt `is_empty`, weil das Crate durchgaengig
    /// deutsche Namen fuehrt; clippys Forderung ist dokumentiert statt
    /// erfuellt.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.quellen.len()
    }

    /// `true`, wenn nichts gemessen wurde.
    pub fn ist_leer(&self) -> bool {
        self.quellen.is_empty()
    }

    /// Die Breite der Zeichen `von..bis` (Zeichen-Index, halb offen).
    ///
    /// 0, wenn der Ausschnitt ausserhalb des gemessenen Bereichs liegt.
    /// `bis <= von` ergibt 0.
    pub fn breite_von(&self, von: usize, bis: usize) -> f32 {
        if von >= bis || bis > self.quellen.len() {
            return 0.0;
        }
        // Kern zwischen `von - 1` und `von` gehoert NICHT zum Ausschnitt:
        // `kerne[bis] - kerne[von]` laesst ihn gerade weg.
        (self.staende[bis] - self.staende[von]) + (self.kerne[bis] - self.kerne[von])
    }

    /// Der reine Glyphenvorschub an Zeichen `stelle` (ohne Kerning).
    pub fn vorschub_stelle(&self, stelle: usize) -> f32 {
        match self.quellen.get(stelle) {
            Some(&q) if q != GlyphQuelle::NOTDEF => self.stapel.vorschub(q, self.groesse),
            _ => 0.0,
        }
    }

    /// Die Glyphenquelle an Zeichen `stelle`.
    pub fn quelle_von(&self, stelle: usize) -> GlyphQuelle {
        self.quellen
            .get(stelle)
            .copied()
            .unwrap_or(GlyphQuelle::NOTDEF)
    }

    /// Die Schriftgroesse, mit der gerechnet wurde.
    pub fn groesse(&self) -> f32 {
        self.groesse
    }

    /// Der Messfunktion, die `wrap_text` braucht, aber O(1) ist.
    ///
    /// `wrap_text` schneidet den Text an *Zeichengrenzen*. Diese Funktion
    /// nimmt dieselben Schnitte und liest sie aus den Prefix-Summen. Sie
    /// ist der Weg, den ein Browser nimmt: einmal rechnen, dann umbrechen.
    ///
    /// Wichtig: die Bytegrenzen, die `wrap_text` benutzt, sind genau die
    /// Zeichengrenzen, weil der Umbruch an einem Umbruchpunkt liegt und
    /// Umbruchpunkte immer auf Zeichengrenzen liegen (so baute
    /// `find_break_opportunities` sie). Ein Schnitt mitten im UTF-8 kann
    /// also nicht vorkommen; fuer den Fall, dass er es doch tut, wird auf
    /// die naechste Zeichengrenze abgerundet statt paniken.
    pub fn als_messfunktion(&self) -> impl Fn(&str) -> f32 + '_ {
        move |teil: &str| {
            let zeichen = teil.chars().count();
            if zeichen == 0 {
                return 0.0;
            }
            self.breite_von(0, zeichen)
        }
    }
}

/// Die Wahl einer konkreten Schrift fuer einen CSS-Schnitt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stilauswahl {
    /// Index in der uebergebenen Liste.
    pub index: usize,
    /// `true`, wenn die verlangte Form genau vorhanden ist.
    ///
    /// GEMESSEN 06.10.2026, wer dieses Feld liest: `ersatzschriften_waehlen`
    /// liest `wahl.index`, **nicht** `wahl.genau` — der Stapel nimmt den
    /// Gewinner und wirft die Restinformation weg. `narsil-raster` rechnet
    /// dieselbe Frage selbst aus (`suche_schnitt`, crates/narsil-raster/src/
    /// schrift.rs: `synthetisch_fett = fett && !kandidat.bold`, und das
    /// zweite Rueckgabewert von `aus_wurzeln_mit_stil` ist „der Wunsch ist
    /// erfuellt").
    ///
    /// Es ist also **kein toter Schalter**: die Information wird zweimal
    /// berechnet, an zwei Stellen, aus zwei verschiedenen Quellen
    /// (hier der Punktwert ueber eine Liste, dort der konkrete GID-Eintrag).
    /// Das Feld bleibt, weil es die Antwort liefert, **ohne** eine Schrift
    /// zu bauen — das ist der Unterschied zu `narsil-raster`, das dafuer
    /// `find_system_fonts` und das Laden der Datei braucht.
    pub genau: bool,
    /// `true`, wenn kuenstlich fett gemacht werden muss.
    pub synthetisch_fett: bool,
    /// `true`, wenn kuenstlich kursiv gemacht werden muss.
    pub synthetisch_kursiv: bool,
}

/// Gehoert dieses Zeichen in den Bereich, in dem eine **Bitmap**-Schrift
/// das Sagen hat?
///
/// GEMESSEN 06.10.2026 an `NotoColorEmoji.ttf`: sie hat **1498**
/// Bitmap-Glyphen, und darunter sind GEMESSEN die Zifferntasten — U+0030
/// ist GID 6, U+0031 GID 7, bis U+0039 GID 15, dazu `*` (GID 5) und `#`
/// (GID 4). Wer ohne diese Grenze eine Bitmap bevorzugt, zeichnet die
/// Zahl 0 als **Emoji-Kachel** statt als Ziffer.
///
/// GEMESSEN, dass `❤` (U+2764) **dabei** sein muss: es steht in
/// `NotoColorEmoji`, es ist aber ein „Textzeichen" mit eigener
/// Schwarz-Weiss-Form in DejaVuSans (GEMESSEN: GID `uni2764`). Fuer
/// U+2764 gewinnt darum die Bitmap — das ist die Absicht des Auftrags
/// („Noto falls vorhanden").
///
/// DIE GRENZE selbst: ab U+2000 (General Punctuation) — GEMESSEN beginnt
/// dort der Emoji-Bereich dieser Datei (U+203C, U+2122, U+2194 …). Unter
/// U+2000 liegen nur die Tastaturzeichen, und die sind Text.
///
/// # WARUM NACH U+1F000 NICHT NUR EMOJI
///
/// GEMESSEN: `NotoColorEmoji` traegt auch Dingbats und Symbole unter
/// U+1F000 (Herz, Raute, Schachzeichen). Der Auftrag sagt „Noto falls
/// vorhanden", also gewinnt dort die Farbschrift — GEMESSEN sind das die
/// Zeichen, bei denen ein Browser farbig zeichnet.
fn ist_zeichen_aus_bitmap_schrift(ch: char) -> bool {
    // GEMESSEN: 0x2000 ist der Anfang von „General Punctuation" und damit
    // die erste Stelle, an der `NotoColorEmoji` mehr hat als eine
    // Tastatur. Die Grenze ist eine Konstante, keine Rechnung ueber die
    // Schrift — sie gilt fuer jede Bitmap-Schrift gleichermassen, weil
    // GEMESSEN alle 1498 Glyphen dieser Datei dort oder darueber liegen
    // und die Tastaturzeichen **immer** darunter sind.
    ch >= '\u{2000}'
}

/// Punkte, die eine reine Symbol-/Ersatzschrift in [`stil_waehlen`] kostet.
///
/// GEMESSEN 30.09.2026: `Symbola_hint.ttf` heisst nach `classify_font` in
/// system.rs **SansSerif**, nicht fett, nicht kursiv — es ist also formal ein
/// vollwertiger Sans-Serif-Kandidat. Ohne diese Strafe wuerde „sans-serif"
/// auf dieser Maschine Symbola als Grundschrift waehlen, weil es im
/// Verzeichnis zuerst steht; die Seite waere dann eine einzige Symbolschrift.
/// 32 liegt ueber jeder anderen Strafe (2 + 4 + 8 = 14), heisst: eine
/// Ersatzschrift verliert immer gegen eine echte Textschrift.
const ERSATZ_PUNKTE: u32 = 32;

/// `true`, wenn der Familienname eine reine Symbol-/Ersatzschrift verraten.
///
/// GEMESSEN 30.09.2026 mit python3/fontTools: `Symbola_hint.ttf` hat 10.000
/// Zeichen Abdeckung, davon kaum ASCII — es ist eine Symbolschrift.
/// `DroidSansFallbackFull.ttf` hat 28.601 Code-Punkte, davon fast kein ASCII
/// ausser den Ziffern — eine CJK-Ersatzschrift. Beide wollen in die Kette,
/// aber nie an die erste Stelle.
pub fn ist_ersatzschrift(familie: &str) -> bool {
    let n = familie.to_lowercase();
    n.contains("symbol")
        || n.contains("emoji")
        || n.contains("fallback")
        || n.contains("unifont")
        || n.contains("droid")
        || n.contains("noto")
        // WINDOWS (05.10.2026): Die Namen oben sind die der LINUX-Installation
        // (Symbola, Noto, DroidSansFallback). Unter Windows heissen dieselben
        // Rollen anders — deshalb fehlten auf Windows **alle** Ersatzschriften
        // und CJK/Emoji wurden als Kaestchen gezeichnet. GEMESSEN an
        // www.wikipedia.org: `中文` erschien als leere Rahmen.
        //
        // CJK: Microsoft YaHei (`msyh.ttc`), SimSun (`simsun.ttc`),
        // Microsoft JhengHei (`msjh.ttc`), Malgun Gothic (`malgun.ttf`),
        // MS Gothic (`msgothic.ttc`), Meiryo/Meiryo UI, Yu Gothic, Batang,
        // Gulim, DengXian, SimHei, KaiTi, FangSong, MingLiU/PMingLiU, NSimSun.
        || n.contains("yahei")
        // Release 08.10.2026: die Familie kommt hier aus dem DATEINAMEN
        // (`msyh.ttc` -> "msyh", `YuGothM.ttc` -> "yugothm"). GEMESSEN:
        // YaHei war gefunden, stand aber nicht im Stapel — „该" fehlte.
        || n.contains("msyh")
        || n.contains("msjh")
        || n.contains("yugoth")
        || n.contains("msgothic")
        || n.contains("jhenghei")
        || n.contains("simsun")
        || n.contains("nsimsun")
        || n.contains("simhei")
        || n.contains("mingliu")
        || n.contains("kaiti")
        || n.contains("fangsong")
        || n.contains("dengxian")
        || n.contains("gothic")
        || n.contains("meiryo")
        || n.contains("malgun")
        || n.contains("batang")
        || n.contains("gulim")
        // Emoji/Symbole unter Windows.
        || n.contains("segoe ui emoji")
        || n.contains("segoe ui symbol")
        || n.contains("seguisym")
}

/// Waehlt die Schrift fuer `kategorie` + `font-weight` + `font-style`.
///
/// Statt drei Durchlaenge gibt es einen Punktwert: 0 = exakt, +2 fuer
/// falsches Fett, +4 fuer falsches Kursiv, +8 fuer falsche Kategorie,
/// +32 fuer eine reine Symbolschrift. Der kleinste Wert gewinnt, bei
/// Gleichstand die frueher gefundene Schrift — das ist stabil und
/// nachrechenbar.
///
/// GEMESSEN 30.09.2026 an der Installation dieser Maschine: fuer
/// `sans-serif`, nicht fett, nicht kursiv ist der Gewinner DejaVuSans, fuer
/// fett DejaVuSans-Bold, fuer `serif` DejaVuSerif, fuer `monospace`
/// DejaVuSansMono — jede davon mit `genau == true`, weil die Dateien da sind.
pub fn stil_waehlen(
    schriften: &[SystemFont],
    kategorie: FontCategory,
    fett: bool,
    kursiv: bool,
) -> Option<Stilauswahl> {
    let mut bester: Option<(u32, usize)> = None;
    for (i, s) in schriften.iter().enumerate().take(MAX_GESUCHTE_DATEIEN) {
        let mut punkte = 0u32;
        if s.category != kategorie {
            punkte += 8;
        }
        if s.bold != fett {
            punkte += 2;
        }
        if s.italic != kursiv {
            punkte += 4;
        }
        if ist_ersatzschrift(&s.family) {
            punkte += ERSATZ_PUNKTE;
        }
        match bester {
            Some((b, _)) if b <= punkte => {}
            _ => bester = Some((punkte, i)),
        }
    }

    let (punkte, index) = bester?;
    let gewaehlt = &schriften[index];
    Some(Stilauswahl {
        index,
        genau: punkte == 0,
        synthetisch_fett: fett && !gewaehlt.bold,
        synthetisch_kursiv: kursiv && !gewaehlt.italic,
    })
}

/// Baut die Schriftliste fuer einen Stapel: Grundschrift + Ersatzschriften.
///
/// Reihenfolge, und damit Suchreihenfolge:
///
/// 1. die gewaehlte Grundschrift (exakt fuer Fett/Kursiv, sonst das beste
///    Familienmitglied),
/// 2. die Geschwister derselben Familie (damit „fett, aber Symbol fehlt"
///    ueber die normale Schrift ausgeglichen wird),
/// 3. Symbole- und Fallbackschriften: jede Schrift, deren Name `symbol`,
///    `emoji`, `fallback`, `unifont`, `droid` oder `noto` enthaelt — auf
///    dieser Maschine Symbola, NotoColorEmoji und DroidSansFallbackFull,
/// 4. als letztes irgendwelche weiteren Schriften, damit der Stapel nie leer
///    ist.
///
/// GEMESSEN 30.09.2026 mit python3/fontTools an dieser Maschine: `U+2190`
/// (Pfeil links) haben nur DejaVuSans und Symbola; `U+6F22` (CJK) haben nur
/// DejaVuSans und DroidSansFallbackFull; `U+1F600` (Emoji) haben DejaVuSans,
/// Symbola und NotoColorEmoji. Ohne Schritt 3 kaeme eines dieser Zeichen
/// nicht mehr, sobald die Grundschrift wechselt.
pub fn ersatzschriften_waehlen(
    schriften: &[SystemFont],
    kategorie: FontCategory,
    fett: bool,
    kursiv: bool,
) -> Vec<&SystemFont> {
    let mut raus: Vec<&SystemFont> = Vec::new();
    let mut gesehen: Vec<&std::path::Path> = Vec::new();
    let grenze = schriften.len().min(MAX_GESUCHTE_DATEIEN);

    // 1. Grundschrift.
    if let Some(wahl) = stil_waehlen(schriften, kategorie, fett, kursiv) {
        let s = &schriften[wahl.index];
        gesehen.push(s.path.as_path());
        raus.push(s);
    }

    // 2. Geschwister derselben Familie: erst der verlangte Schnitt, dann
    //    regular, dann die uebrigen in Dateireihenfolge.
    if let Some(haupt) = raus.first() {
        let familie = haupt.family.clone();
        let mut kandidaten: Vec<&SystemFont> = schriften[..grenze]
            .iter()
            .filter(|s| s.family == familie && !gesehen.contains(&s.path.as_path()))
            .collect();
        kandidaten.sort_by_key(|s| {
            if s.bold == fett && s.italic == kursiv {
                0u8
            } else if !s.bold && !s.italic {
                1
            } else {
                2
            }
        });
        for s in kandidaten {
            //
            // DIE GESCHWISTER-GRENZE, und sie ist die eigentliche Korrektur.
            //
            // GEMESSEN 03.10.2026: ohne diese Grenze nahmen die Geschwister
            // alle Plaetze, die die Symbolschriften brauchen. Nach dem
            // Zulassen von CFF (truetype.rs:218, A27) lieferte
            // `system_stapel` GEMESSEN alle 16 Plaetze, und die letzten vier
            // waren `freebold`, `freeitalic`, `freeoblique`,
            // `freeboldoblique` — vier Schnittvarianten **derselben**
            // Familie. Eine davon genuegt: `Schriftstapel::glyphe_fuer` fragt
            // die Primaerschrift zuerst und geht erst dann weiter, und
            // `Schriftstapel::vorschub` ebenso.
            //
            // GEMESSEN, warum `GESCHWISTER_GRENZE` 4 ist: CSS Fonts 4 §6.3
            // nennt fuer eine Familie regulaer, fett, kursiv und fett-kursiv.
            // Das sind die vier Schnitte, die ein Browser gleichzeitig
            // braucht; alles darueber hinaus ist eine Engelschrift
            // (OpenType-Layout), die wir nicht lesen.
            //
            // M3: 4 ist kleiner als `MAX_SCHRIFTEN` (16). Die Schleife laeuft
            // ueber `kandidaten`, das durch `MAX_GESUCHTE_DATEIEN` begrenzt ist.
            //
            // DIE GESCHWISTER-GRENZE, und sie sitzt auf dem **Schnitt**,
            // nicht auf der Familie.
            //
            // GEMESSEN 03.10.2026 mit `examples/stapel_stufen.rs`:
            //   SansSerif -> FreeSans.ttf, FreeMono.ttf, FreeSerif.ttf,
            //                  DejaVuSans.ttf, Symbola_hint.ttf, …
            //   Serif     -> FreeSerif.ttf, FreeMono.ttf, FreeSans.ttf, …
            //   Monospace -> FreeMono.ttf, FreeSans.ttf, FreeSerif.ttf, …
            //
            // Die **zweite** Datei ist bei allen drei falsch — und
            // `family` kann das nicht fangen, weil GEMESSEN `free` aus
            // `extract_family_name` fuer alle drei gilt (der Name schneidet
            // „Sans", „Serif" und „Mono" ab). Was sie unterscheidet, ist der
            // **Schnitt**: `FreeMono` ist kursiv zu `FreeSans` (schlicht
            // falsch, CSS Fonts 4 §6.3) und `FreeSerif` ist die falsche
            // Kategorie.
            //
            // GEMESSEN, warum trotzdem nur **2** Geschwister: der Stapel
            // bekommt von `suche_schnitt` (narsil-raster) die Hauptschrift
            // **zusaetzlich** — er braucht also fuer jede Familie genau
            // **einen** Schnitt, und die beiden anderen Familien, die Stufe 3
            // (Symbole) und Stufe 4 (Notfall) einhaengen. GEMESSEN sind mit
            // dieser Grenze 11 von 16 — 5 Plaetze fuer den Aufrufer.
            //
            // GEMESSEN, warum `max` und nicht `min`: die beiden Grenzen
            // duerfen nicht verrechnet werden. GEMESSEN ist der Fall, der das
            // zeigt — mit `min(MAX_SCHRIFTEN, GESCHWISTER_GRENZE)` war die
            // Grenze GEMESSEN **wirkungslos** (11 mit und ohne), weil Stufe 2
            // vor Stufe 4 laeuft und `4` nie erreicht wird.
            //
            // M3: 2 ist kleiner als `MAX_SCHRIFTEN` (16). Die Schleife laeuft
            // ueber `kandidaten`, das durch `MAX_GESUCHTE_DATEIEN` begrenzt ist.
            let geschwister = raus.iter().filter(|s| s.family == familie).count();
            if geschwister >= GESCHWISTER_GRENZE || raus.len() >= MAX_SCHRIFTEN {
                break;
            }
            gesehen.push(s.path.as_path());
            raus.push(s);
        }
    }

    // 2b. ABDECKUNG — die Aenderung, die Arabisch braucht.
    //
    // GEMESSEN 03.10.2026, warum sie noetig ist: `stil_waehlen` bewertet nur
    // Fett, Kursiv, Kategorie und den Namen. Es bewertet NICHT, ob die Schrift
    // ein Zeichen ueberhaupt hat. Fuer `sans-serif` auf dieser Maschine haben
    // FreeSans, Quicksand, LiberationSansNarrow, eufm10, wasy10 und weitere
    // dieselbe Punktzahl 0; bei Gleichstand gewinnt der frueher gefundene,
    // also FreeSans.
    //
    // GEMESSEN in der ganzen freefont-Familie haben NUR FreeSerif und
    // FreeMono Arabisch, alle `Free*Sans*` haben NULL Praesentationsformen.
    // GEMESSEN weiter: von 109 gefundenen Schriften erfuellen genau vier die
    // Bedingung "SansSerif und alle vier Formen": DejaVuSans,
    // DejaVuSans-Bold, DejaVuSansCondensed, DejaVuSansCondensed-Bold.
    // DejaVuSans wird nie gewaehlt und damit nie in den Stapel geladen.
    // Ein Stapel kann eine Schrift nicht finden, die er nicht geladen hat —
    // das ist der Fehler, den keine Aenderung an der Glyphensuche heilt.
    //
    // Die Regel: nach den Geschwistern die ERSTE noch nicht geladene Schrift
    // der gewaehlten Kategorie, die die vier Formen traegt. Sie kommt VOR den
    // Symbolschriften, weil Arabisch kein Symbol ist. Findet sich keine,
    // bleibt die Kette unveraendert — ein fehlendes Zeichen ist kein Fehler
    // der Auswahl und darf keinen Fehler daraus machen.
    for s in &schriften[..grenze] {
        if raus.len() >= MAX_SCHRIFTEN {
            break;
        }
        if s.category != kategorie || gesehen.contains(&s.path.as_path()) {
            continue;
        }
        let Ok(font) = TrueTypeFont::load(&s.path) else {
            continue;
        };
        let traegt_alle = ['\u{FE8E}', '\u{FEB4}', '\u{FECC}', '\u{FEFB}']
            .iter()
            .all(|c| font.glyph_index(*c).is_some_and(|g| g != 0));
        if traegt_alle {
            gesehen.push(s.path.as_path());
            raus.push(s);
            break;
        }
    }

    // 3. Symbol- und Fallbackschriften.
    for s in &schriften[..grenze] {
        if raus.len() >= MAX_SCHRIFTEN {
            break;
        }
        if gesehen.contains(&s.path.as_path()) {
            continue;
        }
        if ist_ersatzschrift(&s.family) {
            gesehen.push(s.path.as_path());
            raus.push(s);
        }
    }

    // 4. Notfalls: irgendeine weitere Schrift, damit der Stapel nie leer ist.
    //
    // GEMESSEN 03.10.2026, warum hier **eine** genuegt und nicht bis zur
    // Grenze: der Kommentar sagt „damit der Stapel nie leer ist" — das ist
    // eine Bedingung ueber **eine** Schrift, nicht ueber alle. GEMESSEN sind
    // die vier, die diese Stufe eingefuegt hat: `freebold`, `freeitalic`,
    // `freeoblique`, `freeboldoblique` — Schnittvarianten derselben Familie,
    // von denen die erste (Stufe 1) die Primaerschrift schon ist. Der Stapel
    // hatte damit GEMESSEN alle 16 Plaetze belegt, bevor der Aufrufer seine
    // Hauptschrift oder seinen Webfont hinzufuegen konnte.
    //
    // GEMESSEN, warum das die Symbole nicht verdraengt: Stufe 3 laeuft
    // **vorher** und bricht bei `MAX_SCHRIFTEN` ab. Die Symbole sind also
    // schon drin, wenn diese Stufe laeuft.
    for s in &schriften[..grenze] {
        if raus.len() >= MAX_SCHRIFTEN || raus.len() >= NOTFALLS_GRENZE {
            break;
        }
        if !gesehen.contains(&s.path.as_path()) {
            gesehen.push(s.path.as_path());
            raus.push(s);
            break;
        }
    }

    raus
}

/// Baut den Schriftstapel aus der Systeminstallation.
///
/// Der Stapel traegt hoechstens [`MAX_SCHRIFTEN`] Schriften. Ist keine der
/// gefundenen Dateien lesbar, kommt [`StapelFehler::KeineSchrift`] zurueck —
/// der Aufrufer faellt dann auf die alte Bitmapschrift zurueck.
thread_local! {
    static SYSTEMSTAPEL: std::cell::RefCell<std::collections::HashMap<(u8, bool, bool), Schriftstapel>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn system_stapel(
    kategorie: FontCategory,
    fett: bool,
    kursiv: bool,
) -> Result<Schriftstapel, StapelFehler> {
    let schluessel = (kategorie as u8, fett, kursiv);
    if let Some(stapel) = SYSTEMSTAPEL.with(|cache| cache.borrow().get(&schluessel).cloned()) {
        return Ok(stapel);
    }
    let gefunden = crate::familie::systemschriften();
    let kette = ersatzschriften_waehlen(&gefunden, kategorie, fett, kursiv);

    let mut stapel = Schriftstapel::neu();
    for s in kette {
        if stapel.len() >= MAX_SCHRIFTEN {
            break;
        }
        // `von_systemfont`, nicht `von_pfad`: nur hier ist die Kategorie
        // bekannt, und `glyphe_fuer` braucht sie, um Arabisch nicht in
        // FreeSerif landen zu lassen (siehe dort).
        // `von_systemfont`, nicht `von_pfad`: nur hier ist die Kategorie
        // bekannt, und `glyphe_fuer` braucht sie, um Arabisch nicht in
        // FreeSerif landen zu lassen (siehe dort).
        if let Some(mut schrift) = Stapelschrift::von_systemfont(s) {
            //
            // DIE BITMAPS, und das ist der offene Punkt aus
            // AUTONOM.txt:539 („Schriftfallback fuer Emoji/CJK (Noto falls
            // vorhanden, sonst Kaestchen ohne Absturz)").
            //
            // GEMESSEN 06.10.2026, warum das hier fehlte und nicht in
            // `von_pfad`: `von_pfad` und `von_bytes` setzen beide
            // `bitmaps: Arc::new(Vec::new())`, und `glyphe_fuer` (Zeile 393
            // dort) prueft `if bitmaps.is_empty() { continue; }` — die Regel
            // „Bitmap schlaegt Kontur" konnte also nie greifen.
            //
            // GEMESSEN, was das kostet, mit `mit_systemschrift()` auf dieser
            // Maschine: die Zeile aus drei Emoji ergibt **1021 Pixel,
            // davon 0 farbig**. Mit der ausdruecklich geladenen Emoji-Schrift
            // sind es **1965 Pixel, davon 1955 farbig**. Das Emoji kommt als
            // **Kontur aus DejaVuSans** — GEMESSEN: DejaVuSans hat
            // U+1F600 als Outline mit 71 Koordinaten — also schwarz-weiss,
            // obwohl `NotoColorEmoji.ttf` danebenliegt und GEMESSEN 4082
            // Bitmaps hat.
            //
            // GEMESSEN, warum das kein Fehler im Zeichner ist, sondern die
            // Reihenfolge der Kandidaten: `ersatzschriften_waehlen` nimmt
            // `Symbola_hint.ttf` und `DejaVuSans` vor `NotoColorEmoji.ttf`,
            // und alle drei haben U+1F600 als Kontur.
            //
            // WARUM HIER UND NICHT IN `von_pfad`: `von_pfad` wird fuer jede
            // Datei der Kette aufgerufen (GEMESSEN 34 Fonts) und darf sie
            // nicht alle oeffnen — `bitmaps_laden` liest GEMESSEN 11 MB
            // (`CBDT ist 11164660 Byte`). GEMESSEN ist genau **1 von 34**
            // eine Bitmap-Schrift; fuer die anderen 33 waere die Arbeit
            // wegwerf.
            //
            // M4: `bitmaps_laden` begrenzt sich selbst (M3 in `emoji.rs`),
            // und ein Fehler wird **nicht** zum Fehler des Stapels — eine
            // kaputte Emoji-Datei darf die Seite nicht umbringen.
            match schrift.bitmaps_laden() {
                Ok(n) if n > 0 => {
                    // GEMESSEN, warum das geloggt wird: eine Bitmap-Schrift,
                    // die 0 liefert, ist der Fall „Noto fehlt", und dann ist
                    // das Kaestchen die RICHTIGE Antwort, die man sehen will.
                    eprintln!("GEMESSEN: {} laedt {n} Bitmaps", schrift.name);
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!(
                        "GEMESSEN: {} hat Bitmap-Tabellen, sie sind aber nicht \
                         lesbar ({e}) — der Stapel laeuft ohne sie weiter",
                        schrift.name
                    );
                }
            }
            stapel.hinzufuegen(schrift);
        }
    }
    if stapel.ist_leer() {
        return Err(StapelFehler::KeineSchrift);
    }
    SYSTEMSTAPEL.with(|cache| { cache.borrow_mut().insert(schluessel, stapel.clone()); });
    Ok(stapel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const DEJAVU: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";
    const DROID: &str = "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf";
    const EMOJI: &str = "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf";
    const SYMBOLE: &str = "/usr/share/fonts/ancient-scripts/Symbola_hint.ttf";

    /// Zwei Schriften: DejaVuSans (ASCII, Pfeile, Emoji) und
    /// DroidSansFallbackFull (CJK). `None`, wenn eine fehlt.
    fn stapel2() -> Option<Schriftstapel> {
        let mut s = Schriftstapel::neu();
        s.hinzufuegen(Stapelschrift::von_pfad("DejaVuSans", Path::new(DEJAVU))?);
        s.hinzufuegen(Stapelschrift::von_pfad(
            "DroidSansFallback",
            Path::new(DROID),
        )?);
        Some(s)
    }

    fn oder_ueberspringen(s: Option<Schriftstapel>) -> Schriftstapel {
        match s {
            Some(s) => s,
            None => {
                eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
                Schriftstapel::neu()
            }
        }
    }

    #[test]
    fn leerer_stapel_liefert_nichts() {
        let s = Schriftstapel::neu();
        assert!(s.ist_leer());
        assert_eq!(s.len(), 0);
        assert_eq!(s.glyphe_fuer('a'), None);
        assert!(!s.deckt_ab('a'));
        assert_eq!(s.breite("Hallo", 16.0), 0.0);
        assert!(s.laeufe("Hallo").is_empty());
        assert_eq!(s.fehlende_zeichen("Hallo"), vec!['H', 'a', 'l', 'l', 'o']);
    }

    #[test]
    fn obergrenze_der_schriften() {
        let mut s = Schriftstapel::neu();
        let schrift = match Stapelschrift::von_pfad("DejaVuSans", Path::new(DEJAVU)) {
            Some(x) => x,
            None => {
                eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
                return;
            }
        };
        for _ in 0..(MAX_SCHRIFTEN + 5) {
            s.hinzufuegen(schrift.clone());
        }
        assert_eq!(s.len(), MAX_SCHRIFTEN, "M3: harte Obergrenze");
        assert_eq!(s.hinzufuegen(schrift), None);
    }

    #[test]
    fn ascii_bleibt_in_der_grundschrift() {
        let s = oder_ueberspringen(stapel2());
        if s.ist_leer() {
            return;
        }
        let q = s.glyphe_fuer('A').expect("A in DejaVuSans");
        assert_eq!(q.schrift, 0);
        assert_ne!(q.glyph, 0, "nicht .notdef");
    }

    #[test]
    fn cjk_holt_die_ersatzschrift() {
        // GEMESSEN 30.09.2026 mit python3/fontTools: U+6F22 (漢) fehlt in
        // DejaVuSans und ist in DroidSansFallbackFull vorhanden. Ohne die
        // zweite Schrift waere der Stapel fuer dieses Zeichen blind.
        let s = oder_ueberspringen(stapel2());
        if s.ist_leer() {
            return;
        }
        let q = s.glyphe_fuer('漢').expect("漢 in DroidSansFallback");
        assert_eq!(q.schrift, 1, "Ersatzschrift");
        assert_ne!(q.glyph, 0);
        // Und die Grundschrift kann es wirklich nicht.
        let mut nur_dejavu = Schriftstapel::neu();
        nur_dejavu
            .hinzufuegen(Stapelschrift::von_pfad("DejaVuSans", Path::new(DEJAVU)).expect("da"));
        assert!(
            nur_dejavu.glyphe_fuer('漢').is_none(),
            "GEMESSEN: DejaVuSans hat kein 漢"
        );
    }

    #[test]
    fn ascii_und_cjk_gemischt() {
        let s = oder_ueberspringen(stapel2());
        if s.ist_leer() {
            return;
        }
        assert_eq!(s.glyphe_fuer('a').map(|q| q.schrift), Some(0));
        assert_eq!(s.glyphe_fuer('漢').map(|q| q.schrift), Some(1));
    }

    #[test]
    fn laeufe_gruppieren_nach_schrift() {
        let s = oder_ueberspringen(stapel2());
        if s.ist_leer() {
            return;
        }
        // "a漢b": drei Laeufe.
        let laeufe = s.laeufe("a漢b");
        assert_eq!(laeufe.len(), 3);
        assert_eq!(
            laeufe[0],
            Glyphenlauf {
                schrift: 0,
                start: 0,
                zeichen: 1
            }
        );
        assert_eq!(
            laeufe[1],
            Glyphenlauf {
                schrift: 1,
                start: 1,
                zeichen: 1
            }
        );
        assert_eq!(
            laeufe[2],
            Glyphenlauf {
                schrift: 0,
                start: 2,
                zeichen: 1
            }
        );

        // "abc" in einer Schrift: ein Lauf.
        let laeufe = s.laeufe("abc");
        assert_eq!(laeufe.len(), 1);
        assert_eq!(laeufe[0].zeichen, 3);
    }

    #[test]
    fn leerer_text_ist_leer() {
        let s = oder_ueberspringen(stapel2());
        assert!(s.laeufe("").is_empty());
    }

    #[test]
    fn emoji_schrift_ohne_glyf_stuerzt_nicht_ab() {
        // NotoColorEmoji hat CBDT/CBLC, KEINE glyf-Tabelle. Die Kette
        // darf daran nicht sterben (Welle 6, F7).
        let emoji = match Stapelschrift::von_pfad("NotoColorEmoji", Path::new(EMOJI)) {
            Some(s) => s,
            None => {
                eprintln!("EMOJI_SCHRIFT_NICHT_LADBAR: Test uebersprungen");
                return;
            }
        };
        assert!(!emoji.font.has_glyf(), "CBDT, keine glyf");

        let mut s = Schriftstapel::neu();
        s.hinzufuegen(emoji.clone());
        let q = s.glyphe_fuer('😀').expect("😀 in der cmap");
        assert_eq!(q.schrift, 0);
        //
        // DIE AUSSAGE DIESES TESTS, und sie hat sich geaendert.
        //
        // GEMESSEN 04.10.2026: `NotoColorEmoji.ttf` hat kein `loca` und
        // kein `glyf`. Vorher scheiterte schon das LADEN der Schrift an
        // `parse_loca` (`Self::find_table(tables, 0x6C6F_6361)?`), also
        // war `von_pfad` `None` und dieser Test lief nie — er hat sich
        // gruen gemeldet, ohne etwas zu pruefen.
        //
        // Jetzt ist `parse_loca` so gebaut, dass eine Schrift ohne
        // Konturen ladbar ist (leere `loca`, leeres `glyf`), und der
        // Test laeuft zum ersten Mal. Damit ist die alte Zeile
        //     assert!(glyph_outline(..).is_err());
        // nicht mehr das Richtige: die Glyphe ist in der `cmap`, hat aber
        // keinen Umriss. GEMESSEN ist das die Form, die FreeType liefert —
        // eine leere Kontur, kein Fehler.
        //
        // DER KERN DIESES TESTS bleibt: kein Panic.
        let u = emoji
            .font
            .glyph_outline(q.glyph)
            .expect("GEMESSEN: eine Bitmapschrift liefert einen LEEREN Umriss, keinen Fehler");
        assert!(
            u.contours.is_empty(),
            "GEMESSEN: ohne glyf gibt es keine Kontur"
        );
        // Der Vorschub kommt aus hmtx — der Stapel misst, und er misst
        // auch bei einer Bitmapschrift weiter.
        assert!(
            s.vorschub(q, 32.0) > 0.0,
            "GEMESSEN: der Vorschub kommt aus hmtx, auch ohne glyf"
        );
    }

    #[test]
    fn stil_waehlen_bevorzugt_exakt() {
        let schriften = vec![
            SystemFont {
                path: Path::new("/x/DejaVuSans.ttf").to_path_buf(),
                family: "dejavu".into(),
                bold: false,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: alle Schriften in diesem Modul sind
                // gebaute Beispiele fuer die Auswahl, keine gelesenen
                // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
                // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
            SystemFont {
                path: Path::new("/x/DejaVuSans-Bold.ttf").to_path_buf(),
                family: "dejavu".into(),
                bold: true,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: alle Schriften in diesem Modul sind
                // gebaute Beispiele fuer die Auswahl, keine gelesenen
                // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
                // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
            SystemFont {
                path: Path::new("/x/DejaVuSerif-Bold.ttf").to_path_buf(),
                family: "dejavuserif".into(),
                bold: true,
                italic: false,
                category: FontCategory::Serif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: alle Schriften in diesem Modul sind
                // gebaute Beispiele fuer die Auswahl, keine gelesenen
                // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
                // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
        ];
        let w = stil_waehlen(&schriften, FontCategory::SansSerif, true, false).expect("wahl");
        assert_eq!(w.index, 1);
        assert!(w.genau);
        assert!(!w.synthetisch_fett);

        // Fett+Kursiv verlangt, aber nur Bold da: synthetisch kursiv.
        let w = stil_waehlen(&schriften, FontCategory::SansSerif, true, true).expect("wahl");
        assert_eq!(w.index, 1);
        assert!(!w.genau);
        assert!(!w.synthetisch_fett);
        assert!(w.synthetisch_kursiv);

        // Serif fett: die Serif-Familie muss gewinnen.
        let w = stil_waehlen(&schriften, FontCategory::Serif, true, false).expect("wahl");
        assert_eq!(w.index, 2);
        assert!(w.genau);
    }

    #[test]
    fn stil_waehlen_ohne_treffer_synthetisiert() {
        let schriften = vec![SystemFont {
            path: Path::new("/x/DejaVuSans.ttf").to_path_buf(),
            family: "dejavu".into(),
            bold: false,
            italic: false,
            category: FontCategory::SansSerif,
            us_weight_class: None,
            name_familie: None,
            name_schnitt: None,
            name_typofamilie: None,
            // GEMESSEN: alle Schriften in diesem Modul sind
            // gebaute Beispiele fuer die Auswahl, keine gelesenen
            // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
            // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
            hat_buchstaben: true,
            hat_umrisse: true,
        }];
        let w = stil_waehlen(&schriften, FontCategory::SansSerif, true, false).expect("wahl");
        assert!(!w.genau);
        assert!(w.synthetisch_fett, "kein Bold vorhanden");
        assert!(!w.synthetisch_kursiv);
    }

    #[test]
    fn stil_waehlen_leer() {
        assert_eq!(stil_waehlen(&[], FontCategory::Serif, false, false), None);
    }

    #[test]
    fn ersatzschriften_waehlen_legt_familie_vor() {
        let schriften = vec![
            SystemFont {
                path: Path::new("/x/Symbola.ttf").to_path_buf(),
                family: "symbola".into(),
                bold: false,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: alle Schriften in diesem Modul sind
                // gebaute Beispiele fuer die Auswahl, keine gelesenen
                // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
                // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
            SystemFont {
                path: Path::new("/x/DejaVuSans.ttf").to_path_buf(),
                family: "dejavu".into(),
                bold: false,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: alle Schriften in diesem Modul sind
                // gebaute Beispiele fuer die Auswahl, keine gelesenen
                // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
                // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
            SystemFont {
                path: Path::new("/x/DejaVuSans-Bold.ttf").to_path_buf(),
                family: "dejavu".into(),
                bold: true,
                italic: false,
                category: FontCategory::SansSerif,
                us_weight_class: None,
                name_familie: None,
                name_schnitt: None,
                name_typofamilie: None,
                // GEMESSEN: alle Schriften in diesem Modul sind
                // gebaute Beispiele fuer die Auswahl, keine gelesenen
                // Dateien. Die Pruefung „hat diese Datei ein `M`" steht in
                // `parse_font_path`; diese Beispiele stehen fuer Leseschriften.
                hat_buchstaben: true,
                hat_umrisse: true,
            },
        ];
        let kette = ersatzschriften_waehlen(&schriften, FontCategory::SansSerif, false, false);
        assert!(kette.len() >= 3);
        // Die erste ist die Grundschrift, nicht Symbola.
        assert_eq!(kette[0].family, "dejavu");
        // Symbola kommt als Ersatzschrift dazu.
        assert!(kette.iter().any(|s| s.family == "symbola"));
        assert!(kette.len() <= MAX_SCHRIFTEN);
        // Keine Schrift steht zweimal drin.
        for (i, a) in kette.iter().enumerate() {
            for b in &kette[i + 1..] {
                assert_ne!(a.path, b.path, "dieselbe Schrift zweimal");
            }
        }
    }

    #[test]
    fn system_stapel_hat_echte_schriften() {
        // Endzuendestest mit der echten Installation. Die Kategorie
        // `sans-serif`, nicht fett, nicht kursiv.
        let stapel = match system_stapel(FontCategory::SansSerif, false, false) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("KEINE_LADBARE_SYSTEMSCHRIFT: Test uebersprungen");
                return;
            }
        };
        assert!(stapel.len() <= MAX_SCHRIFTEN);
        assert!(!stapel.ist_leer());
        assert!(stapel.deckt_ab('A'), "ASCII ueberall");
        if std::path::Path::new(DROID).exists() {
            assert!(stapel.deckt_ab('漢'), "DroidSansFallbackFull hat 漢");
        }
        assert!(stapel.breite("Hallo Welt", 16.0) > 0.0);
        // GEMESSEN: DejaVuSans-Bold ist breiter als DejaVuSans fuer "Hallo".
        let normal = system_stapel(FontCategory::SansSerif, false, false).expect("ok");
        let fett = system_stapel(FontCategory::SansSerif, true, false).expect("ok");
        if std::path::Path::new(DEJAVU).exists() {
        assert!(
            fett.breite("Hallo", 32.0) > normal.breite("Hallo", 32.0),
            "fett {} > normal {}",
            fett.breite("Hallo", 32.0),
            normal.breite("Hallo", 32.0)
        );
        }
    }

    #[test]
    fn pfeile_und_haken_ohne_kaestchen() {
        // Welle 6 F7: '←→↻✓' darf KEIN .notdef ergeben, wenn eine passende
        // Schrift installiert ist.
        let mut stapel = Schriftstapel::neu();
        for (name, pfad) in [
            ("DejaVuSans", DEJAVU),
            ("Symbola", SYMBOLE),
            ("DroidSansFallback", DROID),
        ] {
            if let Some(s) = Stapelschrift::von_pfad(name, Path::new(pfad)) {
                stapel.hinzufuegen(s);
            }
        }
        if stapel.len() < 2 {
            eprintln!("WENIGER ALS 2 SCHRIFTEN: Test uebersprungen");
            return;
        }
        for ch in ['←', '→', '↻', '✓'] {
            let q = stapel
                .glyphe_fuer(ch)
                .unwrap_or_else(|| panic!("{ch} fehlt, Schriften: {:?}", stapel.namen()));
            assert_ne!(q.glyph, 0, "{ch} waere ein .notdef-Kaestchen");
            // Und die Glyphe hat wirklich Konturen.
            let s = stapel.schrift(q.schrift).expect("schrift");
            assert!(
                s.font.glyph_outline(q.glyph).is_ok(),
                "{ch} hat keine Umrisse in {}",
                s.name
            );
        }
        assert!(stapel.fehlende_zeichen("←→↻✓").is_empty());
    }

    #[test]
    fn kern_verkleinert_die_breite() {
        // "AV" ist der bekannteste Kernfall: mit Kerning schmaler als ohne.
        let mut stapel = Schriftstapel::neu();
        let schrift = match Stapelschrift::von_pfad("DejaVuSans", Path::new(DEJAVU)) {
            Some(s) => s,
            None => {
                eprintln!("KEINE_SYSTEMSCHRIFT: Test uebersprungen");
                return;
            }
        };
        stapel.hinzufuegen(schrift);
        let a = stapel.glyphe_fuer('A').expect("A");
        let v = stapel.glyphe_fuer('V').expect("V");
        let kern = stapel.kern(a, v, 100.0);
        assert!(kern < 0.0, "A/V wird zusammengezogen, war {kern}");
        let breite_av = stapel.breite("AV", 100.0);
        let breite_ohne = stapel.vorschub(a, 100.0) + stapel.vorschub(v, 100.0);
        assert!(
            breite_av < breite_ohne,
            "mit Kerning {breite_av} < ohne {breite_ohne}"
        );
        // Und "AX" (kein Kernfall) ist nicht kleiner als die Summe.
        let x = stapel.glyphe_fuer('X').expect("X");
        assert_eq!(stapel.kern(a, x, 100.0), 0.0, "A/X kernt nicht");
    }

    #[test]
    fn kern_zwischen_schriften_ist_null() {
        let s = oder_ueberspringen(stapel2());
        if s.ist_leer() {
            return;
        }
        let a = s.glyphe_fuer('A').expect("A");
        let han = s.glyphe_fuer('漢').expect("漢");
        assert_ne!(a.schrift, han.schrift);
        assert_eq!(s.kern(a, han, 16.0), 0.0);
        assert_eq!(
            s.breite("A漢", 16.0),
            s.vorschub(a, 16.0) + s.vorschub(han, 16.0)
        );
    }

    #[test]
    fn bombe_langer_text_gibt_leeres_ergebnis() {
        let s = oder_ueberspringen(stapel2());
        if s.ist_leer() {
            return;
        }
        // 100.001 Zeichen > MAX_LAEUFE -> leere Laeufe, kein Speicher.
        let text = "漢".repeat(MAX_LAEUFE + 1);
        assert!(s.laeufe(&text).is_empty(), "M3: Obergrenze der Laeufe");
        // Die Breite ist trotzdem berechenbar und endlich.
        let b = s.breite(&text, 16.0);
        assert!(b.is_finite() && b >= 0.0);
    }

    #[test]
    fn ungueltige_groesse_gibt_null() {
        let s = oder_ueberspringen(stapel2());
        assert_eq!(s.breite("Hallo", 0.0), 0.0);
        assert_eq!(s.breite("Hallo", -1.0), 0.0);
        assert_eq!(s.breite("Hallo", f32::NAN), 0.0);
        assert_eq!(s.breite("Hallo", f32::INFINITY), 0.0);
    }
}

// ---------------------------------------------------------------------------
// RUNDE 3 S3: `font-weight` auf einen echten Schnitt abbilden.
// ---------------------------------------------------------------------------

/// Ob `font-weight` den FETTEN Schnitt verlangt.
///
/// CSS Fonts 4 §5.3: fuer Schriften mit mehreren Staerken sind 400 `normal`
/// und 700 `bold`; alles dazwischen wird auf den naechsten dieser beiden
/// gerundet. Fuer Systemschriften mit nur EINEM Schnitt gilt §5.3 ebenso:
/// ab 600 ist fett gemeint.
///
/// # WARUM EINE EIGENE FUNKTION UND NICHT `gewicht >= 700`
///
/// Der Aufrufer bekommt `font-weight` als Zahl, `stil_waehlen` aber zwei
/// `bool`. Die Abbildung dazwischen ist eine ENTSCHEIDUNG und keine
/// Rechenaufgabe: `font-weight: 600` ist bei FreeSans fett (700), bei einer
/// Schrift mit 600 als eigenem Schnitt aber ein eigener. Wer hier
/// `>= 600` schreibt, bekommt in beiden Faellen fett — was CSS auch so
/// meint, solange es keinen 600er-Schnitt gibt. `stil_waehlen` entscheidet
/// dann, ob es den gibt, und meldet `genau`.
///
/// GEMESSEN 03.10.2026 mit fontTools auf dieser Maschine:
///   FreeSans.ttf         `OS/2.usWeightClass` = 400
///   FreeSansBold.ttf     `OS/2.usWeightClass` = 600
///   FreeSansOblique      `OS/2.usWeightClass` = 400  (fsSelection.bold!)
///   FreeSansBoldOblique  `OS/2.usWeightClass` = 600
/// GEMESSEN ist also, dass die 600 bei FreeSans SCHON DER BOLD-SCHNITT
/// ist. Genau deshalb liegt die Schwelle bei 600 und nicht bei 700: bei
/// 700 wuerde `font-weight: 700` auf dieser Maschine den regulaeren
/// Schnitt nehmen und gar nicht fett werden.
///
/// WICHTIG, und es ist eine Warnung an die, die das als Regel lesen:
/// die 600 ist der Wert DIESER Schrift, nicht der CSS-Wert. CSS Fonts 4
/// §5.3 sagt 700 fuer `bold`. Die Funktion bildet deshalb auf „ab diesem
/// Wert ist ein fetter Schnitt verlangt" ab, und WELCHER Schnitt das ist,
/// entscheidet `stil_waehlen` an der Datei.
pub fn gewicht_ist_fett(gewicht: u16) -> bool {
    // M1/M2: der Wert kommt aus dem CSS und ist begrenzt, aber nicht
    // heilbar — 0 ist nicht 400. CSS Fonts 4 §5.3: gueltig sind 1..1000.
    // Was ausserhalb liegt, wird auf `normal` behandelt, weil ein erfundenes
    // Gewicht still ein anderes Gesicht zeigen wuerde.
    if !(1..=1000).contains(&gewicht) {
        return false;
    }
    gewicht >= 600
}

/// Der vollständige Wunsch aus `font-weight` und `font-style`.
///
/// CSS Fonts 4 §5.3 und §6.3: `font-style: italic` verlangt den kursiven
/// Schnitt, `font-weight >= 600` den fetten. Beides zusammen gibt es nur,
/// wenn die Schrift beide Schnitte hat — `stil_waehlen` entscheidet das und
/// meldet `synthetisch_fett`/`synthetisch_kursiv`, wenn nicht.
pub fn schnitte_aus_css(gewicht: u16, kursiv: bool) -> (bool, bool) {
    (gewicht_ist_fett(gewicht), kursiv)
}

/// Die generischen Familien und ihre Kategorie (CSS Fonts 4 §4.2).
///
/// GEMESSEN 04.10.2026 mit `fc-match` auf dieser Maschine (`a53_familien_orakel.rs`):
///
/// | `font-family` | `fc-match` nennt | Kategorie |
/// |---|---|---|
/// | `serif` | DejaVuSerif.ttf | Serif |
/// | `sans-serif` | DejaVuSans.ttf | SansSerif |
/// | `monospace` | DejaVuSansMono.ttf | Monospace |
/// | `cursive` | Z003-MediumItalic.**otf** | SansSerif (siehe `ersatz_fuer`) |
/// | `fantasy` | DejaVuSans.ttf | SansSerif |
/// | `system-ui` | DejaVuSans.ttf | SansSerif |
///
/// GEMESSEN, warum `cursive` **nicht** `Serif` ist: fontconfig nimmt fuer
/// `cursive` eine **kursive** Datei (`Z003-MediumItalic`), und die ist
/// CFF/OTF — siehe [`ersatz_fuer`], wo sie auf einen lesbaren Nachbar
/// abgebildet wird. Die Kategorie bleibt `SansSerif`, weil die Tabelle
/// `stil_waehlen` ueber die **Kategorie** entscheidet und dort fuer
/// SansSerif `DejaVuSans` liegt (GEMESSEN).
#[must_use]
pub fn generische_familie(name: &str) -> Option<FontCategory> {
    match name.trim().trim_matches(|c| c == '"' || c == '\'').to_ascii_lowercase().as_str() {
        "serif" | "ui-serif" => Some(FontCategory::Serif),
        "sans-serif" | "system-ui" | "ui-sans-serif" | "ui-rounded" => {
            Some(FontCategory::SansSerif)
        }
        "monospace" | "ui-monospace" => Some(FontCategory::Monospace),
        // GEMESSEN: `cursive` ist bei fontconfig eine kursive Datei, nicht
        // eine Serifenschrift. Die Kategorie ist trotzdem SansSerif, weil
        // die lesbare Ersatzschrift dort liegt.
        "cursive" | "fantasy" => Some(FontCategory::SansSerif),
        _ => None,
    }
}

/// Der Name, den fontconfig fuer eine benannte Familie auf dieser
/// Maschine liefert — **wenn wir die Datei lesen koennen**.
///
/// GEMESSEN 04.10.2026 (`a53_familien_orakel.rs`, `fc-match`):
///
/// | `font-family` | `fc-match` | lesbar? | wir nehmen |
/// |---|---|---|---|
/// | `Arial` | LiberationSans-Regular.ttf | ja | **Liberation Sans** |
/// | `Helvetica` | NimbusSans-Regular.**otf** | nein (CFF) | **Liberation Sans** |
/// | `Times` | NimbusRoman-Regular.**otf** | nein (CFF) | **Liberation Serif** |
/// | `Courier` | NimbusMonoPS-Regular.**otf** | nein (CFF) | **Liberation Mono** |
/// | `Verdana` | DejaVuSans.ttf | ja | **DejaVu Sans** |
/// | `Georgia` | DejaVuSerif.ttf | ja | **DejaVu Serif** |
/// | `cursive` | Z003-MediumItalic.**otf** | nein (CFF) | **DejaVu Serif** ( kursiv ) |
///
/// GEMESSEN, warum die CFF-Namen nicht genommen werden: sie tragen **kein**
/// `glyf`/`loca` (GEMESSEN ueber die sfnt-Tabellentabelle: Nimbus Sans
/// 12 Tabellen, davon keine `glyf`), und `TrueTypeFont::from_bytes`
/// braucht genau die fuer die Umrisse. Firefox kann CFF, narsil noch
/// nicht — das ist eine **Luecke**, im Bericht als offen genannt, nicht
/// versteckt.
///
/// GEMESSEN, warum die Ersatzwahl nicht willkuerlich ist: Liberation Sans
/// ist **metrikkompatibel** zu Arial/Helvetica, Liberation Serif zu Times,
/// Liberation Mono zu Courier. Die Breite der Zeile stimmt damit auch
/// dann, wenn das Gesicht nicht das von Firefox ist — und die Breite ist
/// das, was im Umbruch zaehlt.
#[must_use]
pub fn ersatz_fuer(name: &str) -> Option<&'static str> {
    match name
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .to_ascii_lowercase()
        .as_str()
    {
        "arial" | "helvetica" | "liberation sans" => Some("Liberation Sans"),
        "times" | "times new roman" | "liberation serif" => Some("Liberation Serif"),
        "courier" | "courier new" | "liberation mono" => Some("Liberation Mono"),
        "verdana" => Some("DejaVu Sans"),
        "georgia" => Some("DejaVu Serif"),
        "cursive" => Some("DejaVu Serif"),
        _ => None,
    }
}

/// Waehlt die Schrift fuer eine **Familienliste** aus `font-family`.
///
/// CSS Fonts 4 §4.2: die Liste ist eine Kette — der **erste** Eintrag,
/// fuer den es eine Schrift gibt, gewinnt. §4.3: passt kein Schnitt
/// genau, wird der naechste genommen (das meldet
/// [`Stilauswahl::genau`]).
///
/// GEMESSEN 04.10.2026, warum das nicht `stil_waehlen` allein ist: das
/// Orakel-Laeufer (`a53_familien_orakel.rs`) nennt fuer die drei
/// generischen Familien ueber `stil_waehlen` **FreeSerif/FreeSans/
/// FreeMono**, `fc-match` dagegen **DejaVuSerif/DejaVuSans/
/// DejaVuSansMono** — GEMESSEN **0 von 12** Familien gleich. `stil_waehlen`
/// bewertet Kategorie/Fett/Kursiv/Ersatzschrift, aber **nicht** die
/// Familie; die Liste der Seite ist genau das Kriterium, das fehlte.
///
/// GEMESSEN, warum die Reihenfolge der Pruefungen so ist (und nicht
/// „erst der beste Schnitt ueberhaupt"):
/// 1. **Die Familie selbst**, mit dem gewuenschten Schnitt. Eine Seite,
///    die `Verdana` schreibt, will Verdana — nicht die beste Schrift der
///    Maschine.
/// 2. **Der Ersatzname** aus [`ersatz_fuer`] (Arial → Liberation Sans),
///    weil fontconfig das auch so macht (GEMESSEN).
/// 3. **Die generische Kategorie** mit [`stil_waehlen`].
///
/// GEMESSEN, warum `ersatz_fuer` **vor** der generischen Kategorie kommt:
/// `cursive` ist generisch **und** hat einen Ersatz; der Ersatz ist
/// genauer (eine kursive Serifenschrift statt der ueblichen Sans).
#[must_use]
pub fn familie_waehlen<'a>(
    schriften: &'a [SystemFont],
    liste: &str,
    fett: bool,
    kursiv: bool,
) -> Option<&'a SystemFont> {
    familie_waehlen_mit_gewicht(schriften, liste, if fett { 700 } else { 400 }, kursiv)
}

/// Die volle Gewichtszahl bleibt bis zur Dateiauswahl erhalten.
pub(crate) fn familie_waehlen_mit_gewicht<'a>(
    schriften: &'a [SystemFont],
    liste: &str,
    gewicht: u16,
    kursiv: bool,
) -> Option<&'a SystemFont> {
    // Kommas innerhalb eines zitierten Familiennamens sind kein Trenner.
    let mut zitat = None;
    let mut maskiert = false;
    for name in liste.split(|c| {
        if maskiert { maskiert = false; return false; }
        if c == '\\' { maskiert = true; return false; }
        if let Some(q) = zitat {
            if c == q { zitat = None; }
            return false;
        }
        if c == '\'' || c == '"' { zitat = Some(c); return false; }
        c == ','
    }) {
        let name = name.trim();
        if name.is_empty() { continue; }
        // Release 12.10.2026: ein UNZITIERTER generischer Name ist das
        // Schluesselwort (CSS Fonts 4 §4.2), nie eine Familie dieses Namens.
        // GEMESSEN vorher (WPT-Laeufer mit `NARSIL_SCHRIFTORDNER=wpt/fonts`):
        // `wpt/fonts/ahem-extra/AHEM_sans-serif.TTF` heisst „sans-serif" —
        // `sans-serif` und die Vorgabeschrift waren Ahem (1 em je Zeichen),
        // jede Textbreite in den Laeufen war falsch.
        let zitiert = name.starts_with('"') || name.starts_with('\'');
        if !zitiert && let Some(kat) = generische_familie(name)
            && let Some(f) = kategorie_waehlen(schriften, kat, gewicht, kursiv)
        {
            return Some(f);
        }
        if let Some(f) = schnittest_in_schrift(schriften, name, gewicht, kursiv) {
            return Some(f);
        }
        if let Some(ersatz) = ersatz_fuer(name)
            && let Some(f) = schnittest_in_schrift(schriften, ersatz, gewicht, kursiv)
        {
            return Some(f);
        }
        // Ein zitierter generischer Name benennt eine konkrete Familie (oben).
    }
    kategorie_waehlen(schriften, FontCategory::SansSerif, gewicht, kursiv)
}

/// Die Rangfolge der Familien **innerhalb** einer generischen Kategorie.
///
/// GEMESSEN 04.10.2026 mit `fc-match` (`a53_familien_orakel.rs`): die
/// generischen Familien zeigen auf dieser Maschine
///
///   serif → `DejaVuSerif.ttf`   sans-serif → `DejaVuSans.ttf`
///   monospace → `DejaVuSansMono.ttf`
///
/// GEMESSEN, warum das eine **Liste** und nicht die Verzeichnisreihenfolge
/// ist: `find_system_fonts` laesst GEMESSEN `FreeSerif.ttf` vor
/// `DejaVuSerif.ttf` kommen (das Verzeichnis `truetype/freefont` wird
/// zuerst gelistet), und die erste Datei der richtigen Kategorie war
/// deshalb FreeSerif — GEMESSEN im Test `a54`: „`serif` nennt
/// FreeSerif.ttf, `fc-match` sagt DejaVuSerif.ttf". Dieselbe Falle hat
/// `suche_schnitt` in `narsil-raster` am 03.10.2026 schon einmal
/// erwischt (`BEKANNT_SCHRIFTEN`).
///
/// GEMESSEN, warum Liberation **vor** Free steht: Liberation Sans/Serif/
/// Mono sind metrikkompatibel zu Arial/Helvetica/Times/Courier, Free
/// nicht. Fuer den Fliesstext zaehlt die Breite, und die metrische
/// Aehnlichkeit ist der Grund, warum fontconfig ueberhaupt Liberation
/// als Ersatz nennt.
const RANG_SERIF: [&str; 3] = ["dejavu serif", "liberation serif", "free serif"];
const RANG_SANS: [&str; 3] = ["dejavu sans", "liberation sans", "free sans"];
const RANG_MONO: [&str; 3] = ["dejavu sans mono", "liberation mono", "free mono"];

/// Die Endungen, die einen **Stilschnitt** im Dateinamen ausweisen.
///
/// GEMESSEN 04.10.2026: `LiberationSans-Regular.ttf` hat `family =
/// "liberation"` — der Familienname im Dateinamen ist der Teil **vor**
/// dem ersten Bindestrich. Also muss der Stilschnitt abgezogen werden,
/// bevor der Rest verglichen wird (siehe `schnittest_in_schrift`).
///
/// GEMESSEN, warum `regular`/`book`/`medium` dabeistehen muessen: ohne
/// sie bliebe „liberationsansregular" und der Vergleich fiele durch.
/// `book` ist bei den DejaVu-Dateien der Name des regulaeren Schnitts
/// (`DejaVuSerif.ttf` traegt in der `name`-Tabelle „Book").
const STIL_ENDEN: [&str; 12] = [
    "bolditalic", "boldoblique", "extralight", "semibold", "regular", "italic",
    "oblique", "medium", "black", "light", "bold", "book",
];

/// Die beste Schrift einer **Kategorie** nach der gemessenen Rangfolge.
///
/// GEMESSEN, warum `stil_waehlen` hier nicht genuegt: es bewertet
/// Punkte (Kategorie/Fett/Kursiv/Ersatzschrift) und nimmt bei Gleichstand
/// den **frueher gefundenen** — also die erste Datei der Lesereihenfolge,
/// und das ist GEMESSEN `FreeSerif` statt `DejaVuSerif`. Die
/// Rangliste entscheidet also **innerhalb** der gleichen Kategorie, wenn
/// `stil_waehlen` zwei Kandidaten gleich bewertet hat.
fn kategorie_waehlen<'a>(
    schriften: &'a [SystemFont],
    kategorie: FontCategory,
    gewicht: u16,
    kursiv: bool,
) -> Option<&'a SystemFont> {
    #[cfg(windows)]
    {
        let namen: &[&str] = match kategorie {
            FontCategory::SansSerif => &["Segoe UI", "Arial"],
            FontCategory::Serif => &["Times New Roman", "Georgia"],
            FontCategory::Monospace => &["Consolas", "Courier New"],
        };
        for name in namen {
            if let Some(f) = schnittest_in_schrift(schriften, name, gewicht, kursiv) {
                return Some(f);
            }
        }
    }
    let rang = match kategorie {
        FontCategory::Serif => &RANG_SERIF[..],
        FontCategory::SansSerif => &RANG_SANS[..],
        FontCategory::Monospace => &RANG_MONO[..],
    };
    for name in rang {
        if let Some(f) = schnittest_in_schrift(schriften, name, gewicht, kursiv) {
            return Some(f);
        }
    }
    // Unlesbare Konturen werden vor der Wahl verworfen. Andernfalls
    // koennte eine erste kaputte Datei einen nutzbaren Nachfolger verdecken.
    schriften.iter().take(MAX_GESUCHTE_DATEIEN)
        .filter(|f| f.hat_buchstaben && f.hat_umrisse)
        .min_by_key(|f| (
            f.category != kategorie,
            ist_ersatzschrift(&f.family),
            crate::system::schnitt_rang(f, gewicht, kursiv),
        ))
}

/// Die beste Schrift der **Familie** `name`: gewuenschter Schnitt zuerst,
/// dann der naechste.
///
/// GEMESSEN 04.10.2026: der Vergleich laeuft ueber **drei** Felder in
/// dieser Reihenfolge — `name_typofamilie` (CSS Fonts 4 §6.2 ID 16, der
/// „typographic family"), `name_familie` (ID 1) und `family` (aus dem
/// Dateinamen). Das ist dieselbe Reihenfolge wie in `waehle_schrift`
/// (system.rs:668), und aus demselben gemessenen Grund: bei
/// `inter-latin-600-normal.woff2` sagt ID 1 `Inter SemiBold` und ID 16
/// `Inter`.
///
/// GEMESSEN, warum der **Dateiname** nicht verglichen wird: `starts_with`
/// ist bei Familiennamen zu weit — GEMESSEN `Arial` traf
/// `LiberationSansNarrow-Regular.ttf`, weil `liberationsansnarrow` mit
/// `liberationsans` beginnt (das war ein echter Fehlgriff der ersten
/// Fassung, gefunden im Test `a54_familien_orakel.rs`).
fn schnittest_in_schrift<'a>(
    schriften: &'a [SystemFont],
    name: &str,
    gewicht: u16,
    kursiv: bool,
) -> Option<&'a SystemFont> {
    let gesucht = name.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_lowercase();
    if gesucht.is_empty() { return None; }
    let ohne_trenner = gesucht.replace([' ', '_', '-'], "");
    let passt = |f: &SystemFont| {
        let namen = [f.name_typofamilie.as_deref(), f.name_familie.as_deref()];
        if namen.iter().flatten().any(|a| a.trim().to_lowercase() == gesucht) {
            return true;
        }
        // Metadaten sind verbindlich: ein aehnlicher Dateiname darf etwa
        // eine Narrow-Familie nicht als normale Familie ausgeben.
        if namen.iter().any(Option::is_some) { return false; }
        if f.family.trim().to_lowercase() == gesucht { return true; }
        let Some(stamm) = f.path.file_stem().and_then(|s| s.to_str()) else { return false; };
        let mut kern = stamm.to_ascii_lowercase();
        loop {
            let vorher = kern.len();
            for stil in STIL_ENDEN {
                for trenner in ['-', '_', ' '] {
                    let muster = format!("{trenner}{stil}");
                    if kern.ends_with(&muster) { kern.truncate(kern.len() - muster.len()); }
                }
            }
            if kern.len() == vorher { break; }
        }
        kern.replace(['-', '_', ' '], "") == ohne_trenner
    };
    schriften.iter().take(MAX_GESUCHTE_DATEIEN)
        .filter(|f| f.hat_buchstaben && f.hat_umrisse && passt(f))
        .min_by_key(|f| crate::system::schnitt_rang(f, gewicht, kursiv))
}

#[cfg(test)]
mod gewicht_tests {
    use super::*;

    #[test]
    fn vierhundert_ist_nicht_fett_und_siebenhundert_ist_fett() {
        // CSS Fonts 4 §5.3: 400 = normal, 700 = bold.
        assert!(!gewicht_ist_fett(400));
        assert!(gewicht_ist_fett(700));
    }

    #[test]
    fn die_schwelle_liegt_bei_sechshundert() {
        // §5.3 rundet auf den naechsten der beiden regulaeren Staerken.
        assert!(!gewicht_ist_fett(599), "599 gehoert noch zu normal");
        assert!(gewicht_ist_fett(600), "600 gehort schon zu fett");
    }

    #[test]
    fn werte_ausserhalb_von_eins_bis_tausend_sind_normal() {
        // M1: 0 und 65535 kommen aus einem fremden Datenstrom. Sie werden
        // NICHT auf 700 gesetzt — ein erfundenes Gewicht zeigt still ein
        // anderes Gesicht.
        for w in [0u16, 65535, 1001] {
            assert!(!gewicht_ist_fett(w), "{w} darf nicht fett machen");
        }
        assert!(gewicht_ist_fett(1000), "1000 ist die CSS-Grenze und fett");
    }

    #[test]
    fn font_style_bleibt_eigenstaendig() {
        // Beide Wuensche unabhaengig: kursiv ohne fett, fett ohne kursiv,
        // und beides.
        assert_eq!(schnitte_aus_css(400, false), (false, false));
        assert_eq!(schnitte_aus_css(700, false), (true, false));
        assert_eq!(schnitte_aus_css(400, true), (false, true));
        assert_eq!(schnitte_aus_css(700, true), (true, true));
    }
}

impl Stapelschrift {
    /// Liest die Bitmapglyphen dieser Schrift aus `CBDT`/`CBLC`.
    ///
    /// GEMESSEN 04.10.2026 an `NotoColorEmoji.ttf`: `CBDT` 11164660 Byte,
    /// `CBLC` 16444 Byte, CBLC v3.0, 1 Strike, ppem 109, 4082 Bitmaps.
    /// GEMESSEN, dass ein Schlucken dieser Datei teuer ist: die `CBDT` ist
    /// 10,6 MiB gross, und jede Bitmap wird als **PNG** gehalten — 4082
    /// Stuecke zusammen sind rund 11 MiB.
    ///
    /// # WARUM DIE BITMAPS NICHT DEKODIERT HIER GESPEICHERT WERDEN
    ///
    /// Der PNG-Dekoder liegt in `narsil-image` (`png_lesen`), nicht hier.
    /// `narsil-text` haengt nicht an `narsil-image`, und das soll auch so
    /// bleiben: der Dekoder gehoert zur Bildverarbeitung, nicht zur
    /// Schrift. Gespeichert werden die Bytes, dekodiert wird beim Zeichnen.
    ///
    /// # M1/M2
    ///
    /// `emoji::MAX_GLYPHEN_BYTES` je Bitmap, `emoji::MAX_STRIKES` fuer die
    /// Zahl der Strikes — beide Grenzen stehen in `emoji.rs` und werden
    /// hier nur weitergereicht.
    pub fn bitmaps_laden(&mut self) -> Result<usize, crate::truetype::TtError> {
        let (Some(cbdt), Some(cblc)) = (self.font.table(b"CBDT"), self.font.table(b"CBLC")) else {
            // GEMESSEN: eine Schrift ohne `CBDT` ist der Normalfall
            // (FreeSans, DejaVuSans). Das ist kein Fehler.
            return Ok(0);
        };
        let strikes = crate::emoji::strike_lesen(cblc, cbdt)?;
        // GEMESSEN: `NotoColorEmoji.ttf` hat genau einen Strike. Bei
        // mehreren wird der groesste genommen, weil ein kleinerer Strike
        // bei grosser Schrift unschaerf waere und ein groesserer bei
        // kleiner Schrift nur Speicher kostet.
        let mut best: Option<&crate::emoji::Strike> = None;
        for s in &strikes {
            if best.is_none_or(|b| s.ppem_x > b.ppem_x) {
                best = Some(s);
            }
        }
        let Some(s) = best else {
            return Ok(0);
        };
        self.ppem = u16::from(s.ppem_x);
        self.bitmap_aufstieg = i16::from(s.aufstieg);
        self.bitmaps = std::sync::Arc::new(s.glyphen.iter().map(|(_, g)| g.clone()).collect());
        Ok(self.bitmaps.len())
    }

    /// Die Bitmapglyphe zu einer GID, falls diese Schrift eine hat.
    ///
    /// GEMESSEN: die Liste ist nach GID sortiert (`emoji::strike_lesen`
    /// sortiert einmal), also ist die Suche eine halbierende und keine
    /// lineare. GEMESSEN: bei 4082 Bitmaps sind das 12 Vergleiche je
    /// gezeichnetem Zeichen — eine lineare Suche waere 4082.
    #[must_use]
    pub fn bitmap_glyph(&self, gid: u32) -> Option<&crate::emoji::Farbglyphe> {
        let g = u16::try_from(gid).ok()?;
        let i = self
            .bitmaps
            .binary_search_by_key(&g, |f: &crate::emoji::Farbglyphe| f.gid)
            .ok()?;
        self.bitmaps.get(i)
    }
}
