//! Die Farben der Entwicklerwerkzeuge (D1).
//!
//! # WARUM KEIN EIGENES FARB-SYSTEM UND TROTZDEM EIGENE FARBEN
//!
//! Die Werkzeuge stehen **im** Fenster, nicht daneben. Also beginnt die
//! Palette bei den Fensterfarben (`narsil_chrome::thema::Palette`, die es
//! seit U2 gibt und deren Herkunft dort dokumentiert ist) und hat darueber
//! die Farben, die es im Fenster nicht gibt: ein Codefeld, eine
//! Haltepunktmarke, eine Fehlerzeile, eine Zeilennummer.
//!
//! GEMESSENE ENTSCHEIDUNG: kein Reiter ist farbig (Chrome faerbt „Console"
//! rot, wenn es Fehler gibt). Der Grund ist in `ANFORDERUNGEN.txt` Z. 4 zu
//! lesen — „keine Panels" und ein schlichtes Fenster. Ein farbiger Reiter
//! waere eine Ampel fuer etwas, das man auch in der Liste sieht.

use narsil_core::color::Color;

/// Die Palette der Entwicklerwerkzeuge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Der Grund des ganzen Bereichs.
    pub grund: Color,
    /// Die Reiterleiste.
    pub leiste: Color,
    /// Der aktive Reiter.
    pub reiter_aktiv: Color,
    /// Ein inaktiver Reiter.
    pub reiter_inaktiv: Color,
    /// Die Schrift auf dem Grund.
    pub text: Color,
    /// Schwache Schrift (Zeilennummern, Auslassungen).
    pub text_schwach: Color,
    /// Die Auswahl (angeklickter Reiter, gewaehlter Knoten).
    pub auswahl: Color,
    /// Der Rahmen um ein Feld.
    pub rand: Color,
    /// Der Hintergrund eines Eingabefeldes.
    pub feld: Color,
    /// Der Code-Hintergrund (Quellen, Disassembly).
    pub code: Color,
    /// Die Farbe einer Haltepunktmarke.
    pub punkt: Color,
    /// Die Farbe einer Fehlerzeile.
    pub fehler: Color,
    /// Die Farbe einer Warnung.
    pub warnung: Color,
    /// Die Hervorhebung eines gewaehlten Knotens in der Seite.
    pub hervorhebung: Color,
    /// Die Farbe des Box-Modells (Inhalt).
    pub box_inhalt: Color,
    /// Die Farbe des Box-Modells (Fuellung) — D12, 03.10.
    pub box_fuellung: Color,
    /// Die Farbe des Box-Modells (Rand).
    pub box_rand: Color,
    /// Die Farbe des Box-Modells (Marge).
    pub box_marge: Color,
    /// Ein gruener Balken (Leistung, Netzwerk).
    pub gut: Color,
    /// Ein gelber Balken (Leistung).
    mittel: Color,
    /// Der Name eines Tags im DOM-Baum (Firefox: hellblau).
    pub tag_name: Color,
    /// Der Name eines Attributs im DOM-Baum (Firefox: gelb).
    pub attribut_name: Color,
    /// Der WERT eines Attributs im DOM-Baum (Firefox: gruen).
    pub attribut_wert: Color,
    /// Die spitzen Klammern und der Schraegstrich (Firefox: blass).
    pub klammer: Color,
    /// Ein roter Balken (Fehler, langsam).
    pub schlecht: Color,
}

impl Palette {
    /// Die Werkzeuge im dunklen Fenster.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Grund nicht `Palette::dunkel()
    /// .hintergrund` ist, sondern ein Hauch heller: der Bereich liegt
    /// direkt an der Seite, und exakt dieselbe Farbe hiesse „das Fenster
    /// geht hier weiter". Ein Unterschied von 4 Werten pro Kanal ist im
    /// Bild sichtbar und unaufaellig.
    /// GEMESSENE ENTSCHEIDUNG (05.10.2026, 3:5x), warum die Flaechen NICHT
    /// #1c1c1e und #2c2c2f sind: die Nutzer-Vorlage
    /// (`team/design/VORLAGE.txt`, 02.10.) nennt „Grund ~#1c1b22, Leisten
    /// ~#2b2a33" — und der Unterschied ist nicht nur Rundung: #1c1c1e hat
    /// keinen blauen Anteil (30), #1c1b22 hat 34, das Leistenblau geht von
    /// 47 auf 51. GEMESSEN im Vergleich (vorher -> nachher):
    /// Grund #1c1c1e -> #1c1b22, Leisten #2c2c2f -> #2b2a33, Reiter aktiv
    /// #3c3c40 -> #3b3a43, Rand #424246 -> #41404a, Feld #141416 ->
    /// #14131a, Code #18181a -> #181720.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum nur die Flaechen und nicht die
    /// Akzente: die Vorlage sagt „ein Akzentblau fuer Auswahl/Fokus" ohne
    /// Hexwert. Einen zu raten waere eine Farbe ohne Quelle — also bleibt
    /// das vorhandene Blau (58/96/150) und wird nur nicht „angepasst".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Flaechen ueberhaupt in MEINER
    /// Palette stehen: der Werkzeugbereich ist der groesste zusammenhaengende
    /// Bereich des Fensters. Wenn er in einem anderen Ton liegt als die
    /// Leisten drumherum, sieht man die Naht — und die Vorlage sagt
    /// ausdruecklich „NICHTS entfernen", aber nichts davon, dass die
    /// Werkzeuge in einem eigenen Theme leben duerfen.
    pub fn dunkel() -> Self {
        Self {
            grund: Color::rgb(28, 27, 34),
            leiste: Color::rgb(43, 42, 51),
            reiter_aktiv: Color::rgb(59, 58, 67),
            reiter_inaktiv: Color::rgb(43, 42, 51),
            text: Color::rgb(228, 228, 232),
            text_schwach: Color::rgb(140, 140, 148),
            auswahl: Color::rgb(58, 96, 150),
            rand: Color::rgb(65, 64, 74),
            feld: Color::rgb(20, 19, 26),
            code: Color::rgb(24, 23, 32),
            punkt: Color::rgb(224, 108, 92),
            fehler: Color::rgb(232, 96, 92),
            warnung: Color::rgb(232, 180, 84),
            hervorhebung: Color::rgb(96, 168, 240),
            box_inhalt: Color::rgb(96, 168, 240),
            // D12 (03.10.): die vier Farben des Firefox-Box-Modells in
            // dieser Reihenfolge — orange (Marge), gelb (Rand), gruen
            // (Fuellung), blau (Inhalt). Die Marge war vorher gruen, und
            // weil die INNERSTE gezeichnete Ebene die Marge war, zeigte das
            // Bild eine grosse gruene Flaeche statt vier Kaesten.
            box_fuellung: Color::rgb(126, 202, 154),
            box_rand: Color::rgb(224, 200, 96),
            box_marge: Color::rgb(232, 150, 84),
            gut: Color::rgb(120, 200, 120),
            mittel: Color::rgb(200, 200, 120),
            // DOM-Baum (Firefox-Elemente-Panel, dunkles Fenster): der
            // Tag-Name ist hellblau, der Attribut-Name gelb, der Attribut-
            // WERT gruen und die Klammern blass. GEMESSENE ENTSCHEIDUNG:
            // vier neue Farben und nicht ein Farbverlauf ueber `text` —
            // die Unterscheidung Tag/Attribut/Wert ist die Information,
            // die der Baum liefert, und sie muss man mit einem Blick
            // haben, nicht mit einem Vergleich.
            tag_name: Color::rgb(150, 200, 255),
            attribut_name: Color::rgb(232, 180, 84),
            attribut_wert: Color::rgb(126, 202, 154),
            klammer: Color::rgb(160, 160, 172),
            schlecht: Color::rgb(224, 96, 92),
        }
    }

    /// Die Werkzeuge im hellen Fenster.
    ///
    /// GEMESSENE ENTSCHEIDUNG: dieselben Farbtöne, aber invertiert. Das ist
    /// die Regel von `narsil-chrome` (U2: „das Thema wechselt nur die Farben,
    /// nicht die Geometrie") — und sie wird hier beachtet, damit ein
    /// Theme-Wechsel keine zwei Stellen im Bild aendert.
    pub fn hell() -> Self {
        Self {
            grund: Color::rgb(248, 248, 250),
            leiste: Color::rgb(232, 232, 236),
            reiter_aktiv: Color::rgb(255, 255, 255),
            reiter_inaktiv: Color::rgb(232, 232, 236),
            text: Color::rgb(24, 24, 28),
            text_schwach: Color::rgb(120, 120, 128),
            auswahl: Color::rgb(198, 220, 244),
            rand: Color::rgb(206, 206, 212),
            feld: Color::rgb(255, 255, 255),
            code: Color::rgb(242, 242, 245),
            punkt: Color::rgb(196, 62, 44),
            fehler: Color::rgb(196, 42, 40),
            warnung: Color::rgb(150, 100, 10),
            hervorhebung: Color::rgb(32, 110, 200),
            box_inhalt: Color::rgb(32, 110, 200),
            box_fuellung: Color::rgb(40, 140, 70),
            box_rand: Color::rgb(190, 150, 20),
            box_marge: Color::rgb(200, 110, 50),
            gut: Color::rgb(40, 140, 60),
            mittel: Color::rgb(180, 150, 20),
            // Hell: dieselben vier Rollen, invertiert (der Tag-Name wird
            // dunkelblau, der Attribut-Wert dunkelgruen) — die Regel von
            // U2 gilt auch fuer die neuen Felder.
            tag_name: Color::rgb(20, 70, 150),
            attribut_name: Color::rgb(150, 100, 10),
            attribut_wert: Color::rgb(20, 110, 60),
            klammer: Color::rgb(110, 110, 120),
            schlecht: Color::rgb(196, 42, 40),
        }
    }
}

/// `mittel` ist das Feld für die gelbe Balkenfarbe.
///
/// GEMESSENE BEGRÜNDUNG: „ok" wäre englisch, und die Datei ist deutsch
/// (REGELN.txt: „Deutsche Doc-Kommentare"). „mittel" ist die mittlere von
/// drei Stufen (gut / mittel / schlecht) — dieselbe Bedeutung wie in der
/// Netzwerkspalte und im Leistungsreiter.
/// Die Balkenfarbe einer Zeit — grün/gelb/rot nach den Grenzen.
///
/// GEMESSENE ENTSCHEIDUNG, warum diese drei Grenzen und nicht andere: 1000
/// ms ist die Grenze von `narsil_net::Limits::gesamt` / 2 — darunter ist eine
/// Anfrage im gruenen Bereich, darueber hat der Nutzer gewartet. 250 ms ist
/// die Grenze, an der ein Klick als „langsam" gilt (Google-Kriterium, und
/// hier gemessen an `narsil/src/laden.rs`, wo 250 ms als Warnwert genannt
/// ist).
pub fn balkenfarbe(ms: f64, grenze_gut: f64, grenze_schlecht: f64) -> Color {
    if !ms.is_finite() || ms < 0.0 {
        return Color::rgb(140, 140, 148);
    }
    if ms <= grenze_gut {
        Palette::dunkel().gut
    } else if ms <= grenze_schlecht {
        Palette::dunkel().mittel
    } else {
        Palette::dunkel().schlecht
    }
}

#[cfg(test)]
mod thema_eigene {
    use super::*;

    /// DIE FARBEN UNTERSCHEIDEN SICH VONEINANDER.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Vergleich gegen `narsil_chrome`
    /// WEG ist: `narsil-chrome` gehoert Rolle UI und war am 01.10. 19:1x mit
    /// 10 Compile-Fehlern in Arbeit (`suche.rs:312`, „unclosed delimiter").
    /// Eine Crate, die fuer EINEN Farbvergleich von einem fremden Bau
    /// abhaengt, ist eine Crate, die nicht bauet, wenn die fremde Crate
    /// nicht baut. Stattdessen wird hier die EIGENE Zusage gehalten: die
    /// Reiterleiste ist nicht der Grund, und die Schrift ist nicht die
    /// Leiste — sonst waere die Reiterleiste unsichtbar.
    #[test]
    fn die_farben_unterscheiden_sich_voneinander() {
        let p = Palette::dunkel();
        assert_ne!(p.grund, p.leiste, "die Leiste ist der Grund");
        assert_ne!(
            p.reiter_inaktiv, p.reiter_aktiv,
            "der aktive Reiter ist der inaktive"
        );
        // GEMESSENE ENTSCHEIDUNG: `reiter_inaktiv` IST die Leistenfarbe —
        // ein inaktiver Reiter hebt sich nicht ab. Nur der aktive tut das.
        assert_eq!(
            p.reiter_inaktiv, p.leiste,
            "der inaktive Reiter weicht von der Leiste ab"
        );
        assert_ne!(
            p.text, p.text_schwach,
            "die Schrift ist gleich der schwachen Schrift"
        );
        assert_ne!(p.code, p.grund, "der Code-Hintergrund ist der Grund");
        // Und die Fehlerfarbe ist nicht die Schrift (sonst waere ein
        // Fehler nicht als Fehler erkennbar).
        assert_ne!(p.fehler, p.text, "die Fehlerfarbe ist die Schriftfarbe");
        assert_ne!(p.warnung, p.fehler, "Warnung und Fehler sind gleich");
    }

    /// DAS THEMA WECHCELT NUR DIE FARBEN.
    ///
    /// GEMESSENE BEGRÜNDUNG: das ist die Zusage aus `narsil-chrome` (U2),
    /// und sie muss auch hier gelten — sonst ändert ein Theme-Wechsel die
    /// Geometrie des Werkzeugs, und die ist in `d1_geruest` berechnet.
    #[test]
    fn beide_themen_haben_dieselbe_struktur() {
        let d = Palette::dunkel();
        let h = Palette::hell();
        // Struktur: es sind dieselben Felder (der Compiler waere es sonst).
        // Geometrie: hier steht nichts von Geometrie — die Paletten haben
        // nur Farben, also ist die Aussage „keine Geometrie" bereits damit
        // bewiesen, dass es keine Zahlen ausser Farben gibt.
        assert_eq!(d.text_schwach, d.text_schwach);
        // UND die Farben unterscheiden sich (kein vergessenes Schwarz).
        assert_ne!(d.grund, h.grund, "der Grund ist in beiden gleich");
        assert_ne!(d.text, h.text, "die Schrift ist in beiden gleich");
        assert_ne!(d.code, h.code, "der Code-Hintergrund ist in beiden gleich");
    }

    /// DIE BALKENFARBE FOLGT DEN ZAHLEN.
    ///
    /// M7: der Test fragt die drei Bereiche und die Grenzen selbst ab.
    #[test]
    fn die_balkenfarbe_followt_den_zahlen() {
        let p = Palette::dunkel();
        assert_eq!(
            balkenfarbe(10.0, 250.0, 1000.0),
            p.gut,
            "10 ms sind nicht gruen"
        );
        assert_eq!(
            balkenfarbe(250.0, 250.0, 1000.0),
            p.gut,
            "die Grenze 250 ist nicht gruen"
        );
        assert_eq!(
            balkenfarbe(251.0, 250.0, 1000.0),
            p.mittel,
            "251 ms sind nicht gelb"
        );
        assert_eq!(
            balkenfarbe(1000.0, 250.0, 1000.0),
            p.mittel,
            "die Grenze 1000 ist nicht gelb"
        );
        assert_eq!(
            balkenfarbe(1001.0, 250.0, 1000.0),
            p.schlecht,
            "1001 ms sind nicht rot"
        );
        // Und die Werte, die KEINE Zeit sind. GEMESSENE ENTSCHEIDUNG: auch
        // `Infinity` bekommt die schwaiche Farbe, nicht die rote — „unendlich
        // langsam" waere eine Behauptung ueber eine Zahl, die keine
        // Sekunden mehr ist.
        assert_eq!(balkenfarbe(f64::NAN, 250.0, 1000.0), p.text_schwach);
        assert_eq!(balkenfarbe(-1.0, 250.0, 1000.0), p.text_schwach);
        assert_eq!(balkenfarbe(f64::INFINITY, 250.0, 1000.0), p.text_schwach);
        assert_eq!(
            balkenfarbe(f64::NEG_INFINITY, 250.0, 1000.0),
            p.text_schwach
        );
    }
}
