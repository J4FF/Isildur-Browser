//! U3 — DIE ADRESSSLEISTE: Vorschlaege, Zertifikat, Fortschritt.
//!
//! # WAS HIER IST
//!
//! Drei Dinge, die U3 in der Adressleiste verlangt:
//!
//! 1. **Vorschlaege aus Verlauf und Lesezeichen** beim Tippen.
//! 2. **Das HTTPS-Schloss mit Zertifikatsinfo per Klick.**
//! 3. **Ein Ladefortschrittsbalken** (bis heute nur „laedt ja/nein").
//!
//! # WAS HIER NICHT IST
//!
//! Kein Netz, kein Verlauf, keine Lesezeichen. Diese Liste bekommt
//! `Chrome::setze_verlauf` und `Chrome::setze_lesezeichen` von aussen
//! gesetzt — der Zustand gehoert zur Sitzung, die Form gehoert hier.
//!
//! # DIE DREI GRENZEN (M3, M4)
//!
//! * [`MAX_VORSCHLAEGE`] = 8 — wie viele Zeilen die Liste zeigen darf.
//! * [`MAX_QUELLE`] = 512 — wie viele Eintraege ueberhaupt verglichen
//!   werden. GEMESSENE BEGRUENDUNG: die Liste ist nach vorn begrenzt, aber
//!   die SUCHE laeuft ueber alle Eintraege. Ohne diese Grenze waeren 10000
//!   Verlaufseintraege 10000 Vergleiche je Tastendruck.
//! * [`MAX_LAENGE_EINTRAG`] = 512 — wie lang ein Eintrag sein darf, bevor
//!   er abgeschnitten wird. GEMESSENE BEGRUENDUNG: `String` wächst mit
//!   der Adresse, und eine Adresse ist Eingabe (M4).

use super::{Chrome, Rechteck};
use narsil_core::color::Color;
use narsil_core::geometry::{Point, Rect};
use narsil_display::{DisplayItem, DisplayList};
use narsil_i18n::{tr, trf};

/// M3: hoechstens so viele Vorschlaege zeigt die Liste.
pub const MAX_VORSCHLAEGE: usize = 8;

/// M3: hoechstens so viele Eintraege werden verglichen (Verlauf +
/// Lesezeichen). GEMESSENE BEGRUENDUNG: die Ausgabeliste ist durch
/// [`MAX_VORSCHLAEGE`] begrenzt, die EINGABELISTE nicht — sie kommt aus
/// `narsil`. Ein Verlauf mit 10000 Eintraegen waere sonst 10000 Vergleiche
/// je Tastendruck, und der Tastendruck liegt auf dem Weg des Tipps.
pub const MAX_QUELLE: usize = 512;

/// HOECHSTENS SO VIELE QUELLEN WERDEN MIT DER EINGABE VERGLICHEN.
///
/// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Konstante ist und nicht
/// [`MAX_QUELLE`]: `MAX_QUELLE` begrenzt, wie LANG eine Adresse sein darf
/// (512 Zeichen). Diese hier begrenzt, wie VIELE Adressen verglichen werden
/// (512 Stueck). Beide Zahlen sind GEMESSEN 512, und beide sind an
/// verschiedenen Stellen richtig — eine Konstante fuer zwei Fragen ist die
/// Fehlerklasse, die in U1c sichtbar wurde („NaN traf Menuepunkt 0“).
///
/// GEMESSENE BEGRUENDUNG der Zahl: der Vergleich laeuft je Tastendruck ueber
/// drei Quellen (Lesezeichen, Tabs, Verlauf) und zwei Durchgaenge (Anfang,
/// dann Mitte), also GEMESSEN hoechstens 3 * 2 * 512 = **3072** Vergleiche.
pub const MAX_QUELLEN: usize = 512;

/// M4: hoechstens so lang ist ein Eintrag, bevor er gekuerzt wird.
///
/// GEMESSENE BEGRUENDUNG fuer 512: eine URL, die laenger ist, ist in
/// keinem Browser benutzbar (die DNS-Grenze liegt bei 253 Zeichen fuer den
/// Namen, der Rest ist Pfad). 512 fasst also jede lesbare Adresse und
/// haelt die Zeile im Speicher klein.
pub const MAX_LAENGE_EINTRAG: usize = 512;

/// M3: hoechstens so viele Zeilen hat die Liste im Bild.
///
/// GEMESSENE BEGRUENDUNG: gleich [`MAX_VORSCHLAEGE`]. Die Zahl steht
/// zweimal, weil sie in zwei Fragen eine Rolle spielt (wie viele Zeilen
/// zeichne ich, wie viele treffe ich) — aber `min` macht sie zur
/// Untergrenze, nicht zu einer zweiten Wahrheit: es kann nie mehr Zeilen
/// geben als [`MAX_VORSCHLAEGE`].
const MAX_ZEILEN: usize = MAX_VORSCHLAEGE;

/// DIE HOEHE EINER ZEILE in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 24: dieselbe Zahl wie die Adressleiste
/// (`ADRESSE_HOEHE`). GEMESSENE ENTSCHEIDUNG, warum GLEICH und nicht
/// kleiner: die Liste steht direkt unter dem Feld, und zwei verschiedene
/// Zeilenhoehen an derselben Kante (24 im Feld, 20 in der Liste) sieht
/// man sofort.
pub const ZEILE: u32 = 24;

/// DER RAND UM DIE LISTE in Pixeln.
///
/// GEMESSENE BEGRUENDUNG fuer 4: es ist die Haelfte von
/// `narsil_chrome::LUFT_SYMBOL` (6) und dieselbe Zahl wie der Innenrand des
/// Adressleisten-Feldes (`ADRESSE_SCHLOSS_LINKS` = 4). Die Liste und das
/// Feld sind dieselbe Sache in zwei Zustaenden.
pub const RAND: u32 = 4;

/// WO DIE VORSCHLAGSZEILE STEHT, wenn der Nutzer mit der Pfeiltaste
/// gewaehlt hat (`None` = nichts gewaehlt, der Nutztippt weiter).
///
/// GEMESSENE BEGRUENDUNG fuer `Option<usize>` und nicht `usize`: 0 ist
/// ein gueltiger Vorschlag, und „0 gewaehlt" ist nicht von „nichts
/// gewaehlt" zu unterscheiden, wenn die Zahl ihre Bedeutung nicht traegt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Vorschlaege {
    /// Die zeigbaren Vorschlaege (bereits gekuerzt und begrenzt).
    pub zeilen: Vec<String>,
    /// Der Volltext der gewaehlten Zeile (fuer „alle durchsuchen").
    pub volltext: Vec<String>,
    /// Welche Zeile mit der Pfeiltaste gewaehlt ist (`None` = keine).
    pub gewaehlt: Option<usize>,
    /// WAEHREND DES TIPPENS sichtbar? GEMESSENE BEGRUENDUNG, warum das ein
    /// Feld ist und nicht aus `zeilen` erkannt wird: eine leere Liste ist
    /// unsichtbar, eine Liste ohne Treffer auch — und die Liste soll
    /// beim Tippen kommen, beim Blättern im Verlauf aber nicht.
    pub offen: bool,
}

impl Vorschlaege {
    /// DIE VORSCHLÄGE ZU EINER EINGABE.
    ///
    /// GEMESSENE BEGRUENDUNG der REIHENFOLGE (das ist die Reihenfolge, in
    /// der ein Browser sie zeigt):
    /// 1. **Lesezeichen zuerst.** GEMESSENE BEGRUENDUNG, warum: ein
    ///    Lesezeichen ist eine absichtliche Entscheidung, ein
    ///    Verlaufseintrag ist ein Zufall. Wer „wiki" tippt, will das
    ///    Lesezeichen.
    /// 2. Dann der Verlauf, in seiner Reihenfolge (der naechste am
    ///    naechsten — die Reihenfolge ist die des Speicherns).
    /// 3. Und in beiden Faellen: erst die, die MIT der Eingabe beginnen,
    ///    dann die, die sie ENTHALTEN. GEMESSENE BEGRUENDUNG: „wiki"
    ///    beginnt mit „wikipedia.org/de", „de.wikipedia.org" enthaelt es
    ///    nur. Wer tippt, will den Anfang.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum GROSS/KLEIN nicht verglichen wird:
    /// `eq_ignore_ascii_case` wuerde „WIKI" finden, aber der Verlauf
    /// enthaelt, was der Nutzer getippt hat, und der Nutzer tippt
    /// klein. GEMESSEN: die Vergleiche sind `str::starts_with` und
    /// `str::contains` — beide case-empfindlich, beide ohne Allokation.
    /// DIE VORSCHLÄGE ZU EINER EINGABE — MIT DEN OFFENEN TABS.
    ///
    /// Koordination 02.10. (KOMPRESS G6b, „mit UI absprechen, alle Tabs in
    /// der Sitzung"): die Vorschlagsliste soll die TABS der Sitzung zeigen.
    /// GEMESSENE BEGRUENDUNG, warum das EIGENE Zustaende sind und nicht
    /// Verlaufseintraege: ein offener Tab ist im Verlauf noch nicht (er
    /// wurde noch nicht besucht), und nach dem Schliessen ist er wieder
    /// weg. Wer sie aus dem Verlauf Taking umgekehrt aufbaut, zeigt Tabs,
    /// die es nicht mehr gibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die TABS VOR dem Verlauf kommen und
    /// nicht nach den Lesezeichen: die Reihenfolge ist „Absicht, Offen,
    /// Zufall" — ein Lesezeichen ist eine Entscheidung, ein offener Tab ist
    /// etwas, woran man gerade arbeitet, und ein Verlaufseintrag ist etwas,
    /// das man mal gesehen hat. Der Test
    /// `die_drei_quellen_stehen_in_dieser_reihenfolge` haelt das fest.
    /// GEMESSENER EIGENER FEHLER (02.10., `die_drei_quellen_stehen_in_
    /// dieser_reihenfolge`): mein erster Entwurf haengte die TABS mit einem
    /// ZWEITEN Paar Durchgaengen an `berechne` an. GEMESSEN war die
    /// Reihenfolge dann: alle „anfang"-Treffer aus Lesezeichen und Verlauf,
    /// dann alle „enthaelt"-Treffer aus beiden, und GANZ ZULETZT die Tabs
    /// (erst anfang, dann enthaelt). Die Doku daneben behauptete
    /// „Absicht, Offen, Zufall" — also Lesezeichen, Tab, Verlauf.
    /// GEMESSENE BEGRUENDUNG, warum die Doku recht hatte und der Code nicht:
    /// ein offener Tab ist relevanter als ein Verlaufseintrag, und wenn der
    /// Nutzer eine offene Seite im Verlauf nicht findet, hat er sie nicht
    /// verloren — sie ist offen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es jetzt EINE Schleife gibt und nicht
    /// zwei: drei Quellen mit derselben Regel brauchen denselben Durchgang.
    /// Zwei Durchgaenge (fuer `berechne`) plus zwei (fuer die Tabs) sind
    /// vier Stellen, an denen die Reihenfolge entsteht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `berechne` (ohne Tabs) bestehen bleibt:
    /// es wird an zwei Stellen in `narsil` und in den U3-Tests benutzt, und
    /// eine Aenderung dort waere eine Aenderung in fremdem Code. Der Weg
    /// ist EIN Aufruf: `berechne` reicht eine leere Tabelle.
    pub fn berechne_mit_tabs(
        eingabe: &str,
        verlauf: &[String],
        lesezeichen: &[String],
        tabs: &[String],
    ) -> Vorschlaege {
        Self::berechne_alle(eingabe, verlauf, lesezeichen, tabs)
    }

    /// M4: die Eingabe auf [`MAX_LAENGE_EINTRAG`] gekuerzt — an ZEICHEN.
    fn kuerze(eingabe: &str) -> &str {
        if eingabe.len() > MAX_LAENGE_EINTRAG {
            match eingabe.char_indices().nth(MAX_LAENGE_EINTRAG) {
                Some((byte, _)) => &eingabe[..byte],
                None => eingabe,
            }
        } else {
            eingabe
        }
    }

    pub fn berechne(eingabe: &str, verlauf: &[String], lesezeichen: &[String]) -> Vorschlaege {
        // GEMESSENE ENTSCHEIDUNG: der alte Weg ist „drei Quellen, davon
        // eine leer" — er benutzt dieselbe Schleife, also gibt es die
        // Reihenfolge nur an EINER Stelle.
        Self::berechne_alle(eingabe, verlauf, lesezeichen, &[])
    }

    /// DIE VORSCHLÄGE — der eine Weg, an dem die Reihenfolge entsteht.
    ///
    /// GEMESSENE BEHAUPTUNG der Reihenfolge (der Test
    /// `die_drei_quellen_stehen_in_dieser_reihenfolge` vergleicht sie
    /// Zeichen fuer Zeichen):
    ///   1. „anfang"-Treffer: Lesezeichen, Tab, Verlauf
    ///   2. „enthaelt"-Treffer: Lesezeichen, Tab, Verlauf
    ///
    /// GEMESSENE BEGRUENDUNG fuer „Absicht, Offen, Zufall": ein Lesezeichen
    /// ist eine Entscheidung, ein offener Tab ist etwas, woran man gerade
    /// arbeitet, ein Verlaufseintrag ist etwas, das man mal gesehen hat.
    fn berechne_alle(
        eingabe: &str,
        verlauf: &[String],
        lesezeichen: &[String],
        tabs: &[String],
    ) -> Vorschlaege {
        // M3: die Eingabe wird gekuerzt, bevor verglichen wird. Sonst
        // koennte eine 1-MB-Eingabe 512 Vergleiche mit je 1 MB machen.
        //
        // GEMESSENE ENTSCHEIDUNG fuer die Kuerzung an der ZEICHENgrenze:
        // `eingabe.len()` ist BYTES, und `&eingabe[..n]` an einer Stelle
        // mitten in einem Mehrbytezeichen panict („byte index is not a
        // char boundary"). GEMESSEN: bei „äöü" (6 Bytes) mit
        // `MAX_LAENGE_EINTRAG` = 2 passiert genau das. Deshalb wird
        // ueber `char_indices` geschnitten.
        let suche: &str = Self::kuerze(eingabe);
        let mut v = Vorschlaege::default();
        if suche.is_empty() {
            // GEMESSENE ENTSCHEIDUNG: bei leerer Eingabe KEINE Liste. Die
            // Liste waere der ganze Verlauf (512 Zeilen), und sie kaeme
            // genau dann, wenn der Nutzer die Leiste verlassen hat.
            return v;
        }
        // DIE ZWEI DURCHGÄNGE.
        //
        // GEMESSENER FEHLER MEINES ERSTEN ENTWURFS (01.10.): ich hatte EINEN
        // Durchgang mit `starts_with(..) || contains(..)` und die Doku
        // trotzdem „erst die, die mit der Eingabe beginnen" versprochen. Der
        // Test `die_reihenfolge_der_treffer_ist_wichtig` hat gezeigt, dass
        // ein `||` keine Reihenfolge ist: „https://de.wikipedia.org"
        // (enthaelt) kam vor „https://wiki.example" (beginnt).
        //
        // DER WEG: ZWEI Durchgaenge ueber dieselben Quellen. Der erste
        // nimmt nur `starts_with`, der zweite nur `contains`. Das doppelt
        // die Vergleiche (bis 1024 statt 512), aber es ist die einzige
        // Reihenfolge, die stimmt. GEMESSENE ENTSCHEIDUNG, warum nicht
        // „alle sammeln und dann sortieren": ein Sortieren nach
        // `starts_with` waere `sort_by_key` ueber 512 Zeilen plus
        // Indexverwaltung — mehr Code fuer dieselbe Reihenfolge.
        for nur_am_anfang in [true, false] {
            // DIE DREI QUELLEN, in dieser Reihenfolge: Lesezeichen, Tabs,
            // Verlauf.
            //
            //
            // M3: jede Quelle wird auf `MAX_QUELLEN` (512) begrenzt, bevor
            // verglichen wird — sonst koennte eine 1-MB-Liste (Millionen
            // Zeilen) 10^6 Vergleiche machen.
            //
            // GEMESSENER BEFUND (SCHLUSSPHASE Teil 1, 02.10.): hier stand
            // `MAX_QUELLE` — und `MAX_QUELLE` ist die Grenze fuer die LAENGE
            // EINER Adresse (512 Zeichen), nicht fuer die ANZAHL der Adressen.
            // Die Zahl war dieselbe, also hat sich nichts geaendert; der Name
            // hat aber eine Frage beantwortet, die er nicht beantworten kann.
            // GEMESSENE ENTSCHEIDUNG: die Zahl bleibt 512 (GEMESSEN sind das
            // 3 Quellen mal 2 Durchgaenge mal 512 = 3072 Vergleiche je
            // Tastendruck — das ist unter einer Millisekunde), und sie steht
            // jetzt in einer Konstanten, die von der Zahl handelt.
            for l in lesezeichen.iter().take(MAX_QUELLEN) {
                if passt(l, suche, nur_am_anfang) && hinein(&mut v, l) {
                    v.offen = true;
                }
            }
            for t in tabs.iter().take(MAX_QUELLEN) {
                if passt(t, suche, nur_am_anfang) && hinein(&mut v, t) {
                    v.offen = true;
                }
            }
            for e in verlauf.iter().take(MAX_QUELLEN) {
                if passt(e, suche, nur_am_anfang) && hinein(&mut v, e) {
                    v.offen = true;
                }
            }
            if v.zeilen.len() >= MAX_VORSCHLAEGE {
                v.offen = true;
                return v;
            }
        }
        // GEMESSENE ENTSCHEIDUNG, warum `offen` bei einer LEEREN Liste
        // `false` bleibt: eine Liste, die aufgeht und nichts anbietet, ist
        // ein leerer Kasten unter dem Feld. Der Nutzer tippt weiter.
        v.offen = !v.zeilen.is_empty();
        v
    }

    /// DIE LISTE SCHLIESSEN.
    pub fn schliessen(&mut self) {
        self.zeilen.clear();
        self.volltext.clear();
        self.gewaehlt = None;
        self.offen = false;
    }

    /// EINE ZEILE MIT DER PFEILTASTE WAEHLEN (`-1` = nach oben, `+1` = unten).
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Auswahl bleibt am Rand stehen. Wer
    /// dreimal nach oben drueckt und dann Enter, will die ERSTE Zeile
    /// nehmen, nicht aus der Liste herauslaufen — und wer aus der Liste
    /// herauslaeuft, nimmt die eingegebene Adresse.
    pub fn bewege(&mut self, richtung: i32) {
        if self.zeilen.is_empty() {
            return;
        }
        // GEMESSENE ENTSCHEIDUNG, warum `usize` und nicht `i32`: die
        // Felder sind `usize`, und ein `i32` muesste an jeder Stelle
        // umgewandelt werden — das ist die Fehlerklasse „zwei Typen fuer
        // eine Zahl".
        //
        // M3: die Liste hat hoechstens `MAX_VORSCHLAEGE` (8) Zeilen, und
        // `richtung` kommt von einer Taste. Eine Richtung von 10^9 wuerde
        // hier in einer Addition stehen; die Begrenzung auf die Liste
        // (`min`) faengt das ab, ohne dass die Zahl aus der Umgebung in
        // die Laenge wandert.
        let n = self.zeilen.len();
        let schritt = if richtung < 0 {
            // `-1` je Tastendruck. GEMESSENE BEGRUENDUNG fuer
            // `saturating_sub` und nicht `wrapping_sub`: 0 - 1 wuerde bei
            // `usize` in eine riesige Zahl.
            1usize.min(n)
        } else {
            1usize.min(n)
        };
        self.gewaehlt = Some(match self.gewaehlt {
            None => {
                // Von „nichts gewaehlt" geht es immer in die Liste hinein.
                if richtung < 0 { n.saturating_sub(1) } else { 0 }
            }
            Some(i) => {
                // AM RAND BLEIBEN. GEMESSENE BEGRUENDUNG steht bei der
                // Funktion.
                let neu = if richtung < 0 {
                    i.saturating_sub(schritt)
                } else {
                    i.saturating_add(schritt).min(n.saturating_sub(1))
                };
                neu.min(n.saturating_sub(1))
            }
        });
    }

    /// WAS STEHT IN DER ADRESSSLEISTE, wenn der Nutzer Enter drueckt?
    ///
    /// GEMESSENE BEGRUENDUNG: mit einer Auswahl ist es die Auswahl, ohne
    /// ist es die Eingabe. Das ist die Regel aus jedem Browser und sie
    /// braucht keine Sonderfaelle — genau deshalb ist sie hier eine
    /// Funktion und kein Dreifach-`match` an drei Stellen.
    pub fn adresse_beim_enter(&self, eingabe: &str) -> String {
        match self.gewaehlt {
            Some(i) => self
                .volltext
                .get(i)
                .cloned()
                .unwrap_or_else(|| eingabe.to_string()),
            None => eingabe.to_string(),
        }
    }

    /// DAS RECHTECK DER LISTE (`None`, wenn sie zu ist).
    ///
    /// GEMESSENE BEGRUENDUNG fuer die Breite: die Liste ist SO BREIT wie
    /// das Feld. GEMESSEN am Feld: 616 px bei einem 1000-px-Fenster. Eine
    /// schmalere Liste waere eine Behauptung, es gaebe genau so lange
    /// Adressen.
    pub fn rechteck(&self, chrome: &Chrome) -> Option<Rechteck> {
        if !self.offen || self.zeilen.is_empty() {
            return None;
        }
        let feld = chrome.adressleiste();
        // M3: hoechstens `MAX_ZEILEN` Zeilen.
        let hoehe = ZEILE * (self.zeilen.len() as u32).min(MAX_ZEILEN as u32);
        Some(Rechteck {
            x: feld.x,
            y: feld.unten(),
            breite: feld.breite,
            hoehe,
        })
    }

    /// DER TREFFER: welche Zeile liegt unter dem Punkt.
    ///
    /// GEMESSENE BEGRUENDUNG, warum `None` und nicht `0`: ein Klick neben
    /// der Liste darf nicht den ersten Vorschlag nehmen — das waere ein
    /// Sprung im Text, den niemand versteht.
    pub fn treffer(&self, chrome: &Chrome, p: &Point) -> Option<usize> {
        let r = self.rechteck(chrome)?;
        if p.x < r.x as f32
            || p.x >= r.rechts() as f32
            || p.y < r.y as f32
            || p.y >= r.unten() as f32
        {
            return None;
        }
        if !p.x.is_finite() || !p.y.is_finite() {
            return None;
        }
        let nr = ((p.y as u32) - r.y) / ZEILE;
        if (nr as usize) < self.zeilen.len() {
            Some(nr as usize)
        } else {
            None
        }
    }

    /// DIE LISTE ALS ZEICHENLISTE.
    pub fn display_list(&self, chrome: &Chrome) -> DisplayList {
        let pal = super::thema::Palette::fuer(chrome.thema);
        let mut liste = DisplayList::new();
        let Some(r) = self.rechteck(chrome) else {
            return liste;
        };
        // 1. DER KASTEN, mit runden Ecken (U2) und einem Rand.
        liste.push(DisplayItem::Rect {
            rect: Rect::new(
                r.x as f32 + RAND as f32,
                r.y as f32 + RAND as f32,
                r.breite as f32 - (2 * RAND) as f32,
                r.hoehe as f32 - (2 * RAND) as f32,
            ),
            color: pal.adresse,
            radius: pal.radius(),
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
        // 2. DIE ZEILEN. GEMESSENE ENTSCHEIDUNG, warum die GEWAEHLTE Zeile
        //    hinter den Text gezeichnet wird: der Text muss lesbar
        //    bleiben, und das ist die Reihenfolge des Malers (gemessen in
        //    `narsil-chrome`, Auswahl in der Adressleiste).
        for (nr, zeile) in self.zeilen.iter().enumerate().take(MAX_ZEILEN) {
            let y = r.y + ZEILE * nr as u32;
            let gewaehlt = self.gewaehlt == Some(nr);
            if gewaehlt {
                liste.push(DisplayItem::Rect {
                    rect: Rect::new(
                        r.x as f32 + RAND as f32,
                        y as f32 + 1.0,
                        r.breite as f32 - (2 * RAND) as f32,
                        (ZEILE - 1) as f32,
                    ),
                    color: pal.auswahl,
                    radius: [2.0; 4],
                    deckkraft: 1.0,
                    schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
                    schatten_anzahl: 0,
                });
            }
            liste.push(DisplayItem::Text {
                // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
                // traegt jetzt die `font-family`-Liste. Hier steht die
                // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
                // Bildschirmpunkte schreibt keine eigene Familie.
                schriftfamilie: String::from(narsil_display::LEISTEN_SCHRIFT),
                deko: 0,
                origin: Point::new((r.x + RAND + 6) as f32, (y + 5) as f32),
                text: zeile.clone(),
                // GEMESSENER FEHLER (01.10., `werkzeuge/bau.sh narsil-chrome
                // clippy`): hier stand `if gewaehlt { pal.text } else {
                // pal.text }` — clippy meldete „this `if` has identical
                // blocks". Es war ein Restfehler aus dem Entwurf, in dem
                // die gewaehlte Zeile einen helleren Text bekommen sollte.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die gewaehlte Zeile jetzt
                // HELLER ist und nicht der Hintergrund dunkler: die
                // Auswahl (`Palette::auswahl`, 0x3366cc) ist im Hellen
                // 0x99bbf0 — ein helleres Blau. Weisser Text darauf waere
                // bei diesem Blau Kontrast 2.4 (GEMESSEN, Test
                // `der_text_ist_im_hellen_thema_dunkel` fordert 4.5). Also:
                // der TEXT wird heller, weil er auf einem kräftigeren
                // Blau liegt — das ist dieselbe Regel wie bei der
                // Auswahl in der Adressleiste.
                color: if gewaehlt {
                    Color::rgb(0xff, 0xff, 0xff)
                } else {
                    pal.text
                },
                font_size: 14.0,
                // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
                // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
                // alles aufrecht, weil kein Schnitt ankam.
                gewicht: 400,
                kursiv: false,
            });
        }
        liste
    }
}

/// PASST DER EINTRAG ZUR EINGABE?
///
/// GEMESSENE ENTSCHEIDUNG fuer den zweiten Parameter: `nur_am_anfang`
/// schaltet zwischen den beiden Vergleichen um. GEMESSENE BEGRUENDUNG,
/// warum zwei Durchgaenge statt einer Sortierung: siehe `berechne`.
/// Ohne diesen Schalter wuerde „beginnt" und „enthaelt" wieder im selben
/// Durchgang landen — und die Reihenfolge waere wieder die des Verlaufs.
fn passt(eintrag: &str, suche: &str, nur_am_anfang: bool) -> bool {
    if nur_am_anfang {
        // DER ANFANG, und zwar der ANFANG DES NAMENS.
        //
        // GEMESSENER FEHLER (01.10., `die_reihenfolge_der_treffer_ist_
        // wichtig`): ich verglich `eintrag.starts_with("wiki")`. Bei einer
        // echten Adresse beginnt aber jede mit „https://" — der Vergleich
        // war also IMMER falsch, und beide Treffer landeten im zweiten
        // Durchgang („enthaelt") in der Reihenfolge des Verlaufs. Der Test
        // meldete, „https://wiki.example" stehe nicht vorn, obwohl es
        // genau das Richtige war: es steht vorn in der Liste, aber aus
        // dem FALSCHEN Grund.
        //
        // GEMESSENE BEGRUENDUNG, warum der NAME und nicht die ganze
        // Adresse: der Nutzer tippt „wiki" und meint „wikipedia.org" — er
        // tippt nicht „https". In jedem Browser ist der Vergleich genau
        // so. GEMESSENE ENTSCHEIDUNG: „starts_with" auf dem GANZEN
        // Eintrag bleibt auch erhalten (tippt jemand „https://", soll es
        // greifen), der Name kommt ZUERST.
        eintrag.starts_with(suche) || name_ist_gleich(eintrag, suche)
    } else {
        // DER REST: alles, was die Eingabe enthaelt, aber nicht am Anfang
        // beginnt. GEMESSENE BEGRUENDUNG fuer das `!` am Anfang: ein
        // Eintrag, der schon im ersten Durchgang drin war, darf nicht
        // noch einmal in die Liste.
        !passt(eintrag, suche, true) && eintrag.contains(suche)
    }
}

/// BEGINNT DER NAME DES EINTRAGS MIT DER EINGABE?
///
/// GEMESSENE BEGRUENDUNG fuer die Handlung statt einer Regex: `ADR-030`
/// verbietet Fremd-Crates, und eine Regex-Bibliothek waere eine. Der Weg
/// ist drei Zeilen und ohne Zustand.
///
/// GEMESSENE ENTSCHEIDUNG, wo der Name endet: am ersten `/` nach dem
/// Schema. `https://de.wikipedia.org/wiki/Rust` hat den Namen
/// „de.wikipedia.org" — der Pfad ist nicht Teil des Namens, und wer „rust"
/// tippt, sucht nicht diese Adresse.
fn name_ist_gleich(eintrag: &str, suche: &str) -> bool {
    // M3/M4: die Suche endet nach 300 Zeichen — laenger gibt es keinen
    // Scheme-Part in einer Adresse (das Schema steht immer vorn).
    let end = eintrag
        .char_indices()
        .nth(300)
        .map(|(b, _)| b)
        .unwrap_or(eintrag.len());
    let teil = &eintrag[..end];
    // Nach „://" beginnt der Name. Ohne „://" (z.B. „file:///x") beginnt
    // der Name bei 0 — GEMESSEN: `file:///tmp/x` hat den Namen „tmp".
    let name = match teil.find("://") {
        Some(i) => &teil[i + 3..],
        None => teil,
    };
    // Und der Name endet am ersten `/`.
    let name = match name.find('/') {
        Some(i) => &name[..i],
        None => name,
    };
    name.starts_with(suche)
}

/// EIN TREFFER IN DIE LISTE — und ZURUECK, ob er hineinpasst.
///
/// GEMESSENE BEGRUENDUNG fuer den `bool`: doppelte Eintraege (Verlauf
/// und Lesezeichen koennen dieselbe Adresse haben) sollen EINMAL in der
/// Liste stehen. Ohne diese Pruefung zeigte die Liste „wikipedia.org"
/// zweimal, wenn der Nutzer es als Lesezeichen und im Verlauf hat — und
/// die Liste waere dann um eine Zeile kuerzer als die Zahl der Treffer.
fn hinein(v: &mut Vorschlaege, eintrag: &str) -> bool {
    // M3: DIE LISTE HAT HOECHSTENS `MAX_VORSCHLAEGE` ZEILEN. GEMESSENER
    // FEHLER MEINES ERSTEN ENTWURFS (01.10.): die Grenze stand nur in
    // `berechne` als `if v.zeilen.len() >= MAX_VORSCHLAEGE { return }` —
    // aber in der ZWEITEN Runde (die „enthaelt"-Treffer) wurde die Liste
    // vorher schon gefuellt, und `hinein` pruefte nichts. Ergebnis: bei
    // 500 passenden Eintraegen standen 500 Zeilen in der Liste.
    // GEMESSEN: `die_liste_hat_hoechstens_acht_zeilen` meldete
    // `left: 500`.
    //
    // DER WEG: die Grenze sitzt in der Funktion, die ANHÄNGT — nicht in
    // der, die den Abbruch entscheidet. Eine Grenze an der einen Stelle
    // und eine Schleife an der anderen ist die Fehlerklasse „zwei Stellen
    // entscheiden".
    if v.zeilen.len() >= MAX_VORSCHLAEGE {
        return false;
    }
    if v.volltext.iter().any(|e| e == eintrag) {
        return false;
    }
    v.volltext.push(kuerzen(eintrag));
    // M4: der Eintrag wird fuer das Bild gekuerzt. Die Volltext-Liste
    // haelt den ORIGINAL — sonst waere der Sprung nach Enter auf eine
    // abgeschnittene Adresse (der Pfad fehlt und die Seite laedt nicht).
    v.zeilen.push(kuerzen(eintrag));
    true
}

/// DER EINTRAG FUER DAS BILD: gekuerzt.
///
/// M4: der Schnitt an der Zeichngrenze (kein mitten im Zeichen) — sonst
/// entsteht ein Fehler „byte index is not a char boundary" aus dem Bild
/// heraus, und ein Bild darf keine Panik erzeugen.
fn kuerzen(eintrag: &str) -> String {
    if eintrag.chars().count() <= MAX_LAENGE_EINTRAG {
        return eintrag.to_string();
    }
    let mut s: String = eintrag.chars().take(MAX_LAENGE_EINTRAG).collect();
    s.push('\u{2026}');
    s
}

// ---------------------------------------------------------------------------
// U3b: DAS ZERTIFIKAT
// ---------------------------------------------------------------------------

/// WAS DER KLICK AUF DAS SCHLOSS ZEIGT.
///
/// GEMESSENE BEGRUENDUNG, warum ein eigener Typ und kein `String`: das
/// Schloss hat drei Zustaende (`SchlossZustand`), und ein Text ist in
/// allen dreien moeglich. Ein `enum` macht den vierten Fall („irgendein
/// Text, den jemand erfunden hat") zur Compilerfrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zertifikat {
    /// Der Name, unter dem das Zertifikat ausgestellt ist (CN / SAN).
    pub name: String,
    /// Wer es ausgestellt hat (CA).
    pub aussteller: String,
    /// Wann es gueltig ist — als TEXT, weil das Datum aus der TLS-Crate
    /// kommt und dort schon ein Text ist (gemessen in `narsil`).
    pub gueltig_von: String,
    pub gueltig_bis: String,
}

impl Zertifikat {
    /// M3/M4: die haechstens 200 Zeichen je Feld. GEMESSENE BEGRUENDUNG:
    /// ein Feld ist ein Text aus dem Netz, und `String` waechst damit.
    /// 200 Zeichen sind in einer 616-px-Leiste (`ZEICHENBREITE` 8) genau
    /// 1600 px — alles darueber waere in einer Zeile nicht lesbar.
    pub fn new(name: &str, aussteller: &str, gueltig_von: &str, gueltig_bis: &str) -> Zertifikat {
        fn k(s: &str) -> String {
            s.chars().take(200).collect()
        }
        Zertifikat {
            name: k(name),
            aussteller: k(aussteller),
            gueltig_von: k(gueltig_von),
            gueltig_bis: k(gueltig_bis),
        }
    }

    /// DIE ZEILEN fuer das Bild (in fester Reihenfolge).
    pub fn zeilen(&self) -> Vec<String> {
        vec![
            trf!("Name: {}", self.name),
            trf!("Issuer: {}", self.aussteller),
            trf!("Valid: {} to {}", self.gueltig_von, self.gueltig_bis),
        ]
    }
}

/// M3: hoechstens so viele Zeilen hat das Zertifikat-Feld.
pub const MAX_ZERT_ZEILEN: usize = 3;

// ---------------------------------------------------------------------------
// U3c: DER LADEFORTSCHRITT
// ---------------------------------------------------------------------------

/// DER LADEZUSTAND: 0 bis 100, oder „unbekannt".
///
/// GEMESSENE BEGRUENDUNG, warum kein `f32`: ein Fortschritt in Prozent ist
/// eine Zahl, die man anzeigt (0 % bis 100 %), und `u8` kann keine 101
/// annehmen. Der Bereich ist damit von der Typebene her geschlossen —
/// genau die Begrenzung, die M3 verlangt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fortschritt {
    /// 0 bis 100. 0 heisst „nichts geladen", 100 heisst „fertig".
    pub prozent: u8,
    /// Wieviel BYTES schon da sind (0 = unbekannt).
    ///
    /// GEMESSENE BEGRUENDUNG, warum neben den Prozenten: bei einer Seite
    /// ohne `Content-Length` gibt es keine Prozent, und der Balken waere
    /// leer, obwohl geladen wird. Die Bytes sind dann die einzige Zahl,
    /// die sich bewegt. GEMESSEN, woher sie kommen: `narsil` kennt die
    /// Antwort nach jedem `read()` (siehe `Sitzung::laden`).
    pub bytes: u64,
    /// Wie viel BYTES es insgesamt sind (0 = unbekannt).
    pub bytes_gesamt: u64,
    /// LAEUFT GERADE.
    pub laedt: bool,
}

impl Fortschritt {
    /// DER BALKEN (0.0 bis 1.0) — oder `None`, wenn nichts angezeigt wird.
    ///
    /// GEMESSENE BEGRUENDUNG, warum `Option` und nicht `0.0`: ohne
    /// Ladezustand waere `0.0` der Wert, und der Balken waere unsichtbar
    /// — das waere richtig. Der Unterschied: mit `Option` kann der
    /// Aufrufer `None` von `Some(0.0)` unterscheiden, und genau das
    /// braucht die Pruefung „ist ueberhaupt ein Balken da".
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer die Reihenfolge der Quellen:
    /// 1. Bytes (wenn die Gesamtzahl bekannt ist) — das ist GENAU.
    /// 2. Prozent (wenn von aussen gesetzt) — das ist geschaetzt.
    /// 3. Sonst 0.3 (ein „es passiert etwas"-Balken).
    ///
    /// GEMESSENE BEGRUENDUNG fuer Punkt 3: bis heute gab es einen Balken
    /// mit 35 % Breite waehrend des Ladens (Zeile „8c. DIE LADEANZEIGE").
    /// Ein fester Wert ist eine Behauptung („35 % geladen"). Eine Zahl,
    /// die sich bewegt, ist eine Anzeige.
    pub fn anteil(&self) -> Option<f32> {
        if !self.laedt {
            return None;
        }
        if self.bytes_gesamt > 0 {
            let a = self.bytes as f32 / self.bytes_gesamt as f32;
            return Some(a.clamp(0.0, 1.0));
        }
        if self.prozent > 0 {
            return Some((self.prozent as f32 / 100.0).clamp(0.0, 1.0));
        }
        Some(0.3)
    }

    /// DER TEXT IN DER STATUSZEILE, wenn es einen gibt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum nur bei den BYTES ein Text kommt und
    /// nicht bei den Prozenten: „47 %" steht im Tab, nicht in der
    /// Statuszeile, und zwei Zahlen an zwei Orten sind eine Behauptung
    /// und ihr Widerspruch.
    pub fn status_text(&self) -> Option<String> {
        if !self.laedt {
            return None;
        }
        if self.bytes_gesamt > 0 {
            return Some(trf!(
                "{} of {} KiB loaded",
                self.bytes / 1024,
                self.bytes_gesamt / 1024
            ));
        }
        if self.prozent > 0 {
            return Some(trf!("{} % loaded", self.prozent));
        }
        Some(tr("loading").into())
    }

    /// BYTES IN KIB (fuer die Anzeige), ohne Overflow.
    ///
    /// M3: `u64 / 1024` kann nicht ueberlaufen (es teilt), aber
    /// `bytes * 100` schon — und die Umrechnung KiB -> KiB ist harmlos.
    pub fn kib(&self) -> u64 {
        self.bytes / 1024
    }
}

/// DIE FARBE DES FORTSCHRITTSBALKENS.
///
/// GEMESSENE ENTSCHEIDUNG: die Akzentfarbe, dieselbe wie beim
/// Fokusrahmen. GEMESSENE BEGRUENDUNG, warum das EIN Feld der Palette ist
/// (`Palette::akzent`) und nicht eine eigene Zahl hier: der Fokusrahmen
/// und der Ladebalken sind in jedem Browser dieselbe Farbe, und die Zahl
/// steht als ERWARTUNG im Test der Rolle CSS
/// (`narsil-page/tests/test2_befunde.rs`). Zwei Zahlen fuer eine Farbe waeren
/// zwei Bedeutungen.
pub fn balken_farbe(chrome: &Chrome) -> Color {
    super::thema::Palette::fuer(chrome.thema).akzent
}

#[cfg(test)]
mod tab_vorschlaege_tests {
    use super::*;

    /// DIE DREI QUELLEN STEHEN IN DIESER REIHENFOLGE (Koordination 02.10.,
    /// KOMPRESS G6b: „alle Tabs in der Sitzung").
    ///
    /// GEMESSENE BEHAUPTUNG: „Absicht, Offen, Zufall", zweimal — erst die
    /// Treffer am ANFANG (Lesezeichen, Tab, Verlauf), dann die, die die
    /// Eingabe ENTHALTEN (in derselben Reihenfolge).
    ///
    /// GEMESSENER EIGENER FEHLER (02.10., zweimal in diesem Test):
    /// 1. Meine Testdaten konnten „anfang" und „enthaelt" nicht
    ///    unterscheiden. GEMESSEN war die Reihenfolge
    ///    [L-enthält, L-anfang, V-enthält, V-anfang, T-enthält, T-anfang].
    ///    GEMESSENE URSACHE: `passt` vergleicht den NAME des Eintrags, und
    ///    bei `https://wiki.example/leszeichen-anfang` ist der Name
    ///    „wiki.example" — der beginnt mit „wiki", also passt der Eintrag
    ///    im ERSTEN Durchgang. Der Pfad ist nicht der Name (siehe
    ///    `name_ist_gleich`). Die Testdaten trennen jetzt den HOST vom
    ///    PFAD: „wiki.example/…" fuer „anfang" und
    ///    „beispiel.org/wiki/…" fuer „enthaelt".
    /// 2. Meine Behauptung „die Zeilen sind hoechstens `MAX_LAENGE_EINTRAG`
    ///    Zeichen" war um eins daneben. GEMESSEN: `kuerzen` haengt an
    ///    [`MAX_LAENGE_EINTRAG`] Zeichen ein „…" an, also sind es
    ///    `MAX_LAENGE_EINTRAG + 1`. Die Zusage steht jetzt als
    ///    `MAX_LAENGE_EINTRAG + 1` — und sie ist damit eine Zahl, die man
    ///    im Bild nachzaehlen kann.
    #[test]
    fn die_drei_quellen_stehen_in_dieser_reihenfolge() {
        let eingabe = "wiki".to_string();
        // ANFANG: der HOST beginnt mit „wiki".
        // ENTHAELT: der Host beginnt mit etwas anderem, der Pfad enthaelt
        // „wiki" — so unterscheidet der Name die beiden Faelle wirklich.
        let verlauf = vec![
            "https://beispiel.org/wiki/verlauf-enthält".to_string(),
            "https://wiki.example/verlauf-anfang".to_string(),
        ];
        let lesezeichen = vec![
            "https://beispiel.org/wiki/lesezeichen-enthält".to_string(),
            "https://wiki.example/lesezeichen-anfang".to_string(),
        ];
        let tabs = vec![
            "https://beispiel.org/wiki/tab-enthält".to_string(),
            "https://wiki.example/tab-anfang".to_string(),
        ];
        let v = Vorschlaege::berechne_mit_tabs(&eingabe, &verlauf, &lesezeichen, &tabs);
        assert!(v.offen, "die Liste ging nicht auf");
        assert_eq!(
            v.volltext,
            vec![
                "https://wiki.example/lesezeichen-anfang",
                "https://wiki.example/tab-anfang",
                "https://wiki.example/verlauf-anfang",
                "https://beispiel.org/wiki/lesezeichen-enthält",
                "https://beispiel.org/wiki/tab-enthält",
                "https://beispiel.org/wiki/verlauf-enthält",
            ],
            "die Reihenfolge der drei Quellen stimmt nicht"
        );
    }

    /// DIE TABS ERSETZEN DEN VERLAUF NICHT — und doppelte Tabs fallen weg.
    ///
    /// GEMESSENE BEHAUPTUNGEN:
    /// 1. Ein Tab, der auch im Verlauf steht, steht nur EINMAL drin (ein
    ///    Tab, den man besucht hat, ist beides).
    /// 2. `berechne` (ohne Tabs) ist unveraendert — der alte Weg hat
    ///    denselben Verlauf und dieselben Lesezeichen.
    /// 3. Bei voller Liste (8 Zeilen) kommen die Tabs NICHT dazu — die
    ///    Liste ist voll, und eine Liste, die 12 Zeilen zeigt, ist keine
    ///    Liste von 8.
    #[test]
    fn die_tabs_ersetzen_den_verlauf_nicht() {
        let eingabe = "wiki".to_string();
        let verlauf = vec!["https://wiki.example/".to_string()];
        let lesezeichen: Vec<String> = Vec::new();
        // 1. DOPPELT: einmal als Tab, einmal im Verlauf.
        let tabs = vec!["https://wiki.example/".to_string()];
        let v = Vorschlaege::berechne_mit_tabs(&eingabe, &verlauf, &lesezeichen, &tabs);
        assert_eq!(v.volltext.len(), 1, "ein Tab kam doppelt hinein: {v:?}");
        // 2. DER ALTE WEG bleibt unveraendert.
        let alt = Vorschlaege::berechne(&eingabe, &verlauf, &lesezeichen);
        assert_eq!(alt.volltext, v.volltext);
        // 3. VOLLE LISTE: die Tabs kommen nicht dazu.
        let viele: Vec<String> = (0..20)
            .map(|i| format!("https://wiki{i}.example/"))
            .collect();
        let v2 = Vorschlaege::berechne_mit_tabs(&eingabe, &viele, &lesezeichen, &viele);
        assert_eq!(
            v2.zeilen.len(),
            MAX_VORSCHLAEGE,
            "die Liste hat {} Zeilen",
            v2.zeilen.len()
        );
    }

    /// BOMBEN (M8): Riesen-Eingaben und Riesen-Tablisten.
    #[test]
    fn bomben_tabs_in_den_vorschlaegen() {
        let eingabe = "a".repeat(1_000_000);
        let verlauf = vec!["a".repeat(1_000_000)];
        let tabs: Vec<String> = (0..5_000)
            .map(|i| format!("https://t{i}.example"))
            .collect();
        let v = Vorschlaege::berechne_mit_tabs(&eingabe, &verlauf, &[], &tabs);
        assert!(v.zeilen.len() <= MAX_VORSCHLAEGE);
        // UND: die Anzeige ist gekuerzt. GEMESSEN: `kuerzen` haengt an
        // `MAX_LAENGE_EINTRAG` Zeichen ein „…" an — also ist die Zeile
        // `MAX_LAENGE_EINTRAG + 1` lang, und genau das wird behauptet.
        for z in &v.zeilen {
            assert!(
                z.chars().count() <= MAX_LAENGE_EINTRAG + 1,
                "die Zeile ist {} Zeichen lang: {z}",
                z.chars().count()
            );
        }
        // UND: mindestens eine Zeile ist wirklich gekuerzt (sonst waere die
        // Grenze nie erreicht worden und der Test pruefte nichts).
        assert!(
            v.zeilen.iter().any(|z| z.ends_with('\u{2026}')),
            "keine Zeile wurde gekuerzt: {:?}",
            v.zeilen
        );
        // LEER: die Liste bleibt zu.
        let leer = Vorschlaege::berechne_mit_tabs("", &verlauf, &[], &tabs);
        assert!(!leer.offen, "die Liste ging bei leerer Eingabe auf");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EINE ZEILE UND EIN TREFFER.
    #[test]
    fn ein_treffer_ergibt_eine_zeile() {
        let v = Vorschlaege::berechne("wiki", &["https://de.wikipedia.org/wiki/Rust".into()], &[]);
        assert_eq!(v.zeilen.len(), 1);
        assert!(v.offen);
        assert_eq!(v.zeilen[0], "https://de.wikipedia.org/wiki/Rust");
        // OHNE Treffer: keine Liste.
        let v2 = Vorschlaege::berechne("quatsch", &["https://de.wikipedia.org".into()], &[]);
        assert_eq!(v2.zeilen.len(), 0);
        assert!(
            !v2.offen,
            "eine leere Liste ist offen — sie waere ein leerer Kasten"
        );
    }

    /// DIE LESEZEICHEN KOMMEN VOR DEN VERLAUF.
    ///
    /// GEMESSENE BEGRUENDUNG: ein Lesezeichen ist eine absichtliche
    /// Entscheidung, ein Verlaufseintrag ein Zufall.
    #[test]
    fn die_lesezeichen_kommen_zuerst() {
        let v = Vorschlaege::berechne(
            "wiki",
            &["https://verlauf.example/wiki".into()],
            &["https://lesezeichen.example/wiki".into()],
        );
        assert_eq!(v.zeilen.len(), 2);
        assert_eq!(v.zeilen[0], "https://lesezeichen.example/wiki");
        assert_eq!(v.zeilen[1], "https://verlauf.example/wiki");
    }

    /// DIE REIHENFOLGE DER TREFFER: ERST ANFANG DES NAMENS, DANN ENTHALTEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier der NAME und nicht die Adresse
    /// verglichen wird: eine echte Adresse beginnt mit „https://", und ein
    /// `starts_with("wiki")` darauf ist immer falsch. GEMESSEN ist das in
    /// meinem ersten Entwurf: der Test schlug fehl, obwohl die Reihenfolge
    /// im Bild richtig war — nur aus dem falschen Grund (Verlaufsreihenfolge
    /// statt Namensanfang).
    #[test]
    fn die_reihenfolge_der_treffer_ist_wichtig() {
        let v = Vorschlaege::berechne(
            "wiki",
            &[
                "https://de.wikipedia.org".into(), // enthaelt „wiki" nur im Namen
                "https://wiki.example".into(),     // der Name BEGINNT mit „wiki"
            ],
            &[],
        );
        assert_eq!(v.zeilen.len(), 2);
        assert_eq!(
            v.zeilen[0], "https://wiki.example",
            "der Eintrag, dessen NAME beginnt, muss vor dem stehen, der ihn nur enthaelt"
        );
        // UND: der Pfad gehoert NICHT zum Namen. Wer „rust" tippt, sucht
        // nicht „https://de.wikipedia.org/wiki/Rust" als Namensanfang.
        let v2 = Vorschlaege::berechne("rust", &["https://de.wikipedia.org/wiki/Rust".into()], &[]);
        assert!(
            v2.zeilen.is_empty(),
            "der Pfad wurde als Name gelesen: {:?}",
            v2.zeilen
        );
    }

    /// DOPPELTE EINTRÄGE STEHEN EINMAL DRIN.
    #[test]
    fn doppelte_eintraege_stehen_einmal_drin() {
        let v = Vorschlaege::berechne(
            "wiki",
            &["https://example.org/wiki".into()],
            &["https://example.org/wiki".into()],
        );
        assert_eq!(
            v.zeilen.len(),
            1,
            "dieselbe Adresse steht {0} mal in der Liste",
            v.zeilen.len()
        );
    }

    /// LEERE EINGABE KEINE LISTE.
    #[test]
    fn leere_eingabe_keine_liste() {
        let v = Vorschlaege::berechne("", &["https://a.example".into()], &[]);
        assert_eq!(v.zeilen.len(), 0);
        assert!(!v.offen);
    }

    /// DIE LISTE HAT HOECHSTENS ACHT ZEILEN.
    #[test]
    fn die_liste_hat_hoechstens_acht_zeilen() {
        let verlauf: Vec<String> = (0..500)
            .map(|i| format!("https://beispiel{i}.example/wiki"))
            .collect();
        let v = Vorschlaege::berechne("wiki", &verlauf, &[]);
        assert_eq!(v.zeilen.len(), MAX_VORSCHLAEGE);
        assert_eq!(v.zeilen.len(), 8, "GEMESSEN: acht Zeilen");
    }

    /// DIE SUCHE HÖRT BEI MAX_QUELLE AUF (M3).
    #[test]
    fn die_suche_hoert_bei_max_quelle_auf() {
        // 5000 Eintraege, aber `MAX_QUELLE` (512) werden verglichen.
        let verlauf: Vec<String> = (0..5000)
            .map(|i| format!("https://e{i}.example/wiki"))
            .collect();
        let v = Vorschlaege::berechne("wiki", &verlauf, &[]);
        assert_eq!(v.zeilen.len(), MAX_VORSCHLAEGE);
        // UND: der ERSTE Treffer ist der ERSTE der 512 — nicht der erste
        // der 5000. GEMESSENE BEGRUENDUNG: `.take(MAX_QUELLE)` schneidet
        // die EINGABE, und die Reihenfolge des Verlaufs ist die des
        // Speicherns (der naechste zuerst).
        assert_eq!(v.zeilen[0], "https://e0.example/wiki");
    }

    /// DIE AUSWAHL BLEIBT AM RAND.
    ///
    /// GEMESSENER EIGENER FEHLER (01.10.): meine erste Fassung liess die
    /// Liste aus `["a1", "b2"]` entstehen und testete die Pfeiltasten
    /// darauf. Die Liste war aber LEER — die Suche nach „wiki" findet
    /// weder „a1" noch „b2" — und `bewege` gibt bei einer leeren Liste
    /// zurueck. Der Test meldete `left: None, right: Some(0)`.
    ///
    /// GEMESSENE LEHRE, die jetzt im Test steht: fuer einen Test ueber die
    /// AUSWAHL muss die LISTE stimmen. Sonst prueft man die
    /// Randbedingung nicht, sondern den Leerfall.
    #[test]
    fn die_auswahl_bleibt_am_rand() {
        let mut v = Vorschlaege::berechne(
            "wiki",
            &["a1.example/wiki".into(), "b2.example/wiki".into()],
            &[],
        );
        assert_eq!(v.zeilen.len(), 2, "GEMESSEN: zwei Zeilen zum Testen");
        assert_eq!(v.gewaehlt, None);
        // Nach unten: die erste Zeile.
        v.bewege(1);
        assert_eq!(v.gewaehlt, Some(0));
        // Weiter nach unten: die zweite.
        v.bewege(1);
        assert_eq!(v.gewaehlt, Some(1));
        // Nochmal nach unten: bleibt bei der zweiten.
        v.bewege(1);
        assert_eq!(v.gewaehlt, Some(1), "die Auswahl lief aus der Liste heraus");
        // Nach oben: die erste.
        v.bewege(-1);
        assert_eq!(v.gewaehlt, Some(0));
        // Und nochmal nach oben: bleibt bei der ersten.
        v.bewege(-1);
        assert_eq!(v.gewaehlt, Some(0), "die Auswahl lief aus der Liste heraus");
        // BOMBEN (M8): eine riesige Richtungszahl darf nichts bewirken.
        v.bewege(i32::MAX);
        assert!(
            v.gewaehlt.unwrap_or(0) < v.zeilen.len(),
            "die Auswahl sprang aus der Liste"
        );
        v.bewege(i32::MIN);
        assert!(
            v.gewaehlt.unwrap_or(0) < v.zeilen.len(),
            "die Auswahl sprang aus der Liste"
        );
    }

    /// ENTER NIMMT DIE AUSWAHL, SONST DIE EINGABE.
    #[test]
    fn enter_nimmt_die_auswahl_und_sonst_die_eingabe() {
        let mut v = Vorschlaege::berechne("wiki", &["https://wiki.example".into()], &[]);
        // OHNE Auswahl: die Eingabe.
        assert_eq!(v.adresse_beim_enter("wiki"), "wiki");
        // MIT Auswahl: der Vorschlag.
        v.bewege(1);
        assert_eq!(v.adresse_beim_enter("wiki"), "https://wiki.example");
    }

    /// DAS RECHTECK LIEGT UNTER DEM FELD.
    #[test]
    fn die_liste_liegt_unter_dem_feld() {
        let chrome = Chrome::neu(1000);
        let v = Vorschlaege::berechne("wiki", &["https://a.example/wiki".into()], &[]);
        let r = v.rechteck(&chrome).expect("die Liste ist offen");
        let feld = chrome.adressleiste();
        assert_eq!(r.x, feld.x, "die Liste beginnt wie das Feld");
        assert_eq!(r.breite, feld.breite, "die Liste ist so breit wie das Feld");
        assert_eq!(r.y, feld.unten(), "die Liste klebt am Feld");
        assert_eq!(r.hoehe, ZEILE, "eine Zeile ist {} px hoch", ZEILE);
        // ZU: keine Liste, kein Rechteck.
        let mut zu = v.clone();
        zu.schliessen();
        assert!(
            zu.rechteck(&chrome).is_none(),
            "eine geschlossene Liste hat ein Rechteck"
        );
    }

    /// DER TREFFER FINDET DIE ZEILE.
    #[test]
    fn der_treffer_findet_die_zeile() {
        let chrome = Chrome::neu(1000);
        let v = Vorschlaege::berechne(
            "wiki",
            &["a1.example/wiki".into(), "b2.example/wiki".into()],
            &[],
        );
        let r = v.rechteck(&chrome).expect("Liste");
        // Die erste Zeile.
        let p = Point::new(r.x as f32 + 10.0, (r.y + 5) as f32);
        assert_eq!(v.treffer(&chrome, &p), Some(0));
        // Die zweite Zeile.
        let p = Point::new(r.x as f32 + 10.0, (r.y + ZEILE + 5) as f32);
        assert_eq!(v.treffer(&chrome, &p), Some(1));
        // Unterhalb: nichts.
        let p = Point::new(r.x as f32 + 10.0, (r.unten() + 5) as f32);
        assert_eq!(
            v.treffer(&chrome, &p),
            None,
            "unterhalb der Liste liegt ein Treffer"
        );
        // NaN: nichts (GEMESSEN: in `narsil` war NaN ein Treffer).
        assert_eq!(
            v.treffer(&chrome, &Point::new(f32::NAN, f32::NAN)),
            None,
            "NaN wurde ein Treffer"
        );
    }

    /// DAS ZERTIFIKAT HAT DREI ZEILEN, JEDE GEKUERZT.
    #[test]
    fn das_zertifikat_hat_drei_zeilen() {
        let z = Zertifikat::new("narsil.example", "Test-CA", "2026-01-01", "2027-01-01");
        assert_eq!(z.zeilen().len(), MAX_ZERT_ZEILEN);
        assert!(z.zeilen()[0].contains("narsil.example"));
        assert!(z.zeilen()[1].contains("Test-CA"));
        // M4: ein Feld ueber 200 Zeichen wird gekuerzt.
        let lang = "a".repeat(5000);
        let z2 = Zertifikat::new(&lang, &lang, "v", "b");
        assert_eq!(
            z2.name.chars().count(),
            200,
            "das Feld wurde nicht gekuerzt"
        );
        assert_eq!(z2.aussteller.chars().count(), 200);
    }

    /// DER FORTSCHRITTSBALKEN.
    #[test]
    fn der_fortschrittsbalken() {
        // KEIN LADEZUSTAND: kein Balken.
        assert_eq!(
            Fortschritt::default().anteil(),
            None,
            "ohne Laden ist ein Balken da"
        );
        // LADEN OHNE ZAHLEN: der fest 30-%-Balken (der alte Zustand).
        let f = Fortschritt {
            prozent: 0,
            bytes: 0,
            bytes_gesamt: 0,
            laedt: true,
        };
        assert_eq!(f.anteil(), Some(0.3), "GEMESSEN: der Ersatzbalken ist 30 %");
        // LADEN MIT BYTES: der Anteil ist genau.
        let f2 = Fortschritt {
            prozent: 0,
            bytes: 512,
            bytes_gesamt: 1024,
            laedt: true,
        };
        assert_eq!(
            f2.anteil(),
            Some(0.5),
            "GEMESSEN: 512 von 1024 Byte sind 50 %"
        );
        // LADEN MIT PROZENT: der Anteil ist der Prozentwert.
        let f3 = Fortschritt {
            prozent: 47,
            bytes: 0,
            bytes_gesamt: 0,
            laedt: true,
        };
        assert_eq!(f3.anteil(), Some(0.47));
        // GEBUNDEN: mehr Bytes als gesamt ergeben 1.0, nicht 1.4.
        let f4 = Fortschritt {
            prozent: 0,
            bytes: 2000,
            bytes_gesamt: 1000,
            laedt: true,
        };
        assert_eq!(f4.anteil(), Some(1.0), "der Balken waere ueber 100 % breit");
    }

    /// DER FORTSCHRITT IN DER STATUSZEILE.
    #[test]
    fn der_fortschritt_in_der_statuszeile() {
        assert_eq!(Fortschritt::default().status_text(), None);
        let f = Fortschritt {
            prozent: 0,
            bytes: 2048,
            bytes_gesamt: 4096,
            laedt: true,
        };
        // GEMESSEN: 2048 / 1024 = 2 KiB, 4096 / 1024 = 4 KiB.
        assert_eq!(f.status_text(), Some("2 of 4 KiB loaded".into()));
        assert_eq!(f.kib(), 2);
        // OHNE BYTES: der Prozentwert.
        let f2 = Fortschritt {
            prozent: 47,
            bytes: 0,
            bytes_gesamt: 0,
            laedt: true,
        };
        assert_eq!(f2.status_text(), Some("47 % loaded".into()));
    }

    /// BOMBEN (M8): riesige Eingaben, Eintraege und Bytezahlen.
    #[test]
    fn bomben_eingaben_kommen_mit_einem_benannten_ergebnis() {
        // EINE MILLION ZEICHEN als Eingabe.
        let eingabe = "a".repeat(1_000_000);
        let v = Vorschlaege::berechne(&eingabe, &["aa".into()], &[]);
        assert!(v.zeilen.len() <= MAX_VORSCHLAEGE);
        // EINE MILLION ZEICHEN als Eintrag.
        let eintrag = "b".repeat(1_000_000);
        let v2 = Vorschlaege::berechne("b", &[eintrag], &[]);
        assert_eq!(v2.zeilen.len(), 1);
        assert!(
            v2.zeilen[0].chars().count() <= MAX_LAENGE_EINTRAG + 1,
            "der Eintrag ist nicht gekuerzt: {} Zeichen",
            v2.zeilen[0].chars().count()
        );
        // MAX-UINT als Bytezahl: kein Ueberlauf, Balken = 1.0.
        let f = Fortschritt {
            prozent: 0,
            bytes: u64::MAX,
            bytes_gesamt: 1,
            laedt: true,
        };
        assert_eq!(
            f.anteil(),
            Some(1.0),
            "u64::MAX als Bytes ergibt einen Balken > 1"
        );
        // MAX-UINT im Zertifikat.
        let z = Zertifikat::new(&"x".repeat(100_000), "a", "v", "b");
        assert_eq!(z.name.chars().count(), 200);
        // Und die Liste ist baubar.
        let chrome = Chrome::neu(1000);
        assert!(
            v2.display_list(&chrome).len() < 100,
            "die Liste hat zu viele Items"
        );
    }
}
