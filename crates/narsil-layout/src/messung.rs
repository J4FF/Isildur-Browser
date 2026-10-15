//! DIE MESSUNG — eine Schrift, eine Zahl, drei Verbraucher.
//!
//! W5G, 30.09.2026 (Koordination): "Gezeichnet wird jetzt mit
//! TrueType-Breiten, aber das Layout platziert und bricht noch mit den
//! BITMAP-Breiten um." Daraus die Forderung nach EINER Messfunktion, die
//! Layout, Umbruch und Zeichnen gemeinsam benutzen.
//!
//! # WARUM DIESE DATEI UND NICHT narsil-text
//!
//! `narsil-text` besitzt `measure` (Bitmap) und `measure_tt` (TrueType) —
//! zwei Funktionen, zwei Einheiten. Was fehlte, ist die ENTSCHEIDUNG,
//! welche davon gilt, und die Weitergabe dieser Entscheidung an alle drei
//! Verbraucher. Eine Entscheidung gehoert dorthin, wo die drei sie
//! brauchen, und das ist der kleinste gemeinsame Crate: `narsil-layout`
//! (Layout-Baum), von dem `narsil-page` und `narsil-raster` beide abhaengen.
//! `narsil-layout-engine` haengt davon ab, `narsil-text` haengt NICHT davon
//! ab — die Pyramide (ADR-039) bleibt also in einer Richtung.
//!
//! # DIE DREI VERBRAUCHER
//!
//! | Verbraucher | was er braucht | Methode |
//! |---|---|---|
//! | `narsil-page` (Umbruch) | Breite eines Wortes | [`Messung::breite`] |
//! | `narsil-layout-engine` (Boxbreiten) | Breite, Aufstieg | [`Messung::breite`], [`Messung::aufstieg`] |
//! | `narsil-raster` (Zeichnen) | Vorschub je Zeichen | [`Messung::vorschub`] |
//!
//! Alle drei rufen dieselben Funktionen. Wenn die Breite im Umbruch und
//! die Breite beim Zeichnen auseinanderlaufen, ist das ein FEHLER in
//! `Messung` — nicht in einer der Crates.
//!
//! # DIE ZAHLEN, GEMESSEN (FreeSans, 16 px)
//!
//! * Bitmap (`BitmapFont::builtin`): 6/7 der Schriftgroesse je Zeichen,
//!   AUF GANZE PIXEL GERUNDET. Monospace.
//!
//!   GEMESSEN 08:32, `Messung::bitmap().breite("W", grad)`:
//!   grad 12 -> 10, 14 -> 12, 16 -> 14, 19,2 -> 16, 24 -> 21, 32 -> 27.
//!
//!   DIE ZAHL "13,71 px" stand hier vorher ohne die Rundung daneben, und
//!   das ist falsch: 16 * 6/7 = 13,71 ist der Wert VOR der Rundung, und
//!   ein Aufrufer sieht 14. Wer die Zahl im Kopf gegen eine Messung
//!   haelt, schliesst auf einen Fehler, den es nicht gibt — das ist mir
//!   um 08:32 genau so gegangen (`tests/messung_stern.rs`).
//! * Systemschrift: hmtx, 4.4 px (` `) bis 15.7 px (`W`).
//!
//! Der Unterschied ist der Grund fuer die ganze Crate: 13.71 px je
//! Zeichen reserviert und 7 px gezeichnet heisst "Luecke", 7 px reserviert
//! und 13.71 px gezeichnet heisst "Ueberlappung".

use std::sync::Arc;

use narsil_text::{BitmapFont, Schriftstapel, TrueTypeFont, measure, measure_tt};

/// Hoechste Zahl Zeichen, die EINE Messung misst (M3).
///
/// GEMESSEN, warum 4.096: `measure_tt` iteriert ueber `text.chars()`, und
/// `text` kommt aus einer Datei. Der Umbruch bricht ohnehin vorher um, und
/// ein Messaufruf ueber 4.096 Zeichen wuerde am unteren Rand der
/// Bildschirmbreite nichts mehr aendern.
pub const MAX_ZEICHEN: usize = 4096;

/// Die Quelle fuer die **Familie** eines Textes — der Teil, der vor
/// dieser Kette fehlte.
///
/// GEMESSENER Anlass (SCHRIFT_9, 04.10.2026): der Umbruch misst mit
/// DejaVu Sans, der Raster zeichnet in der verlangten Familie. Ein
/// 152-Zeichen-Serifstring: Firefox GEMESSEN 1292 px, unsere Tinte
/// GEMESSEN 1292 px, unsere **Messung** GEMESSEN 1267,88 px — 24,12 px zu
/// schmal. GEMESSEN ist auch, dass es nicht an den Zeichen liegt: acht
/// Zeichen (n, u, ue, ae, eszett, e, e-acute, x) je 40x ergaben
/// GEMESSEN **0 px** Unterschied.
///
/// # WARUM EIN TRAIT UND KEIN FELD
///
/// `narsil-layout` darf keine Dateien lesen und kennt keine Schriften — der
/// Ort, der sie findet, ist `narsil-text` (und der Raster nutzt es). Die
/// Quelle kommt deshalb von aussen herein, und der Aufrufer, der sie
/// kennt, haengt sie an die Messung.
///
/// `None` (keine Quelle) ist ein **gueltiger** Zustand: dann gilt
/// [`Messung::breite_stil`] unveraendert, und der Aufrufer traegt die
/// Verantwortung fuer die Breite — wie vor dieser Kette.
pub trait FamilienStapeln: Send + Sync {
    /// Der Stapel der Familie `familie` im Schnitt (fett, kursiv), oder
    /// `None`, wenn es ihn nicht gibt.
    fn stapel_fuer(&self, familie: &str, fett: bool, kursiv: bool)
    -> Option<Arc<Schriftstapel>>;
}

/// Die gewaehlte Schrift und die daraus abgeleiteten Zahlen.
///
/// # WIE MAN SIE BENUTZT
///
/// Einmal pro Seite bauen, dann durchreichen:
///
/// ```text
/// let messung = Messung::systemschrift(arc_schrift)   // oder ::bitmap()
/// seite  = seite_aus_text_mit_messung(html, breite, basis, &messung)
/// baum   = layout_mit_messung(&seite.document, viewport, &messung)
/// liste  = build_display_list(&baum, &seite.document)
/// schrift.zeichne(&liste, &messung)                   // Zeichnen
/// ```
///
/// `Messung` ist billig zu kopieren (ein `Arc` bei der Systemschrift),
/// damit sie an vier Stellen liegen kann, ohne dass jemand sie teilen
/// muss.
#[derive(Clone)]
pub struct Messung {
    /// Die Systemschrift, wenn eine gewaehlt wurde.
    tt: Option<Arc<TrueTypeFont>>,
    /// Ersatzschriften je Zeichen (Koordination 30.09.): fehlt ein Zeichen in
    /// der Hauptschrift (Pfeile, CJK, Emoji), misst die Schrift aus dem Stapel,
    /// die es hat - genau wie der Raster es zeichnet. Kerning inklusive.
    stapel: Option<Arc<Schriftstapel>>,
    /// Die vier SCHNITTE (normal, fett, kursiv, fett-kursiv), wenn der
    /// Aufrufer sie mitgegeben hat.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT), warum das Feld noetig war: ein
    /// Absatz in `font-weight: bold` wurde mit dem **regulaeren** Schnitt
    /// gemessen und mit dem fetten gezeichnet. GEMESSEN an `texttest.html`
    /// gegen Firefox: die fetten Zeilen sind GEMESSEN 145 px breit, der
    /// regulaere Schnitt ist 128 px — der Umbruch rechnete also mit 17 px
    /// zu wenig je Zeile und brach **frueher** um als Firefox. Test
    /// `das_layout_misst_fett_noch_mit_dem_regulaeren_schnitt`
    /// (`crates/narsil-page/tests/stil_kette.rs`) haelt den Befund fest.
    ///
    /// GEMESSEN, warum hier vier `Arc` und nicht eine Tabelle mit
    /// `RwLock`: die vier Schnitte werden **einmal** je Seite gebaut und
    /// danach nur gelesen. `Arc` traegt sie ohne Kopie durch alle
    /// Verbraucher, und `narsil-raster` zeichnet mit derselben Datei — das
    /// ist der ganze Punkt (siehe [`Messung::breite_stil`]).
    ///
    /// `None`, wenn es keine gibt: dann misst [`Messung::breite_stil`]
    /// wie [`Messung::breite`], und das ist der alte Zustand — nicht
    /// schlimmer, nur nicht besser.
    ///
    /// GEMESSEN 04.10.2026, warum hier **Stapel** stehen und nicht die
    /// Schriftdatei: die Messung eines Textes mit Zeichen, die der
    /// Hauptschrift fehlen (Pfeile, CJK, Emoji), laeuft ueber den Stapel —
    /// und der Stapel hatte nur die **Grundschrift**. Die erste Fassung
    /// von `breite_stil`nahm die Schnittdatei fuer die Hauptglyphen und
    /// den Grundstapel fuer den Rest; GEMESSEN war das Ergebnis
    /// `191,83 px` fuer **beide** Schnitte, weil `Messung::stapel` VOR
    /// der Schnittdatei geprueft wurde und fast immer vorhanden ist. Der
    /// Schnitt braucht also einen **eigenen Stapel** mit sich als erster
    /// Schrift (`Schriftstapel::mit_vorrang`).
    schnitte: Option<[Option<Arc<Schriftstapel>>; SCHNITTE]>,
    /// Die **Familienquelle** (04.10.2026, SCHRIFT_9). `None` heisst:
    /// die Messung kennt nur Schnitte, keine Familien — dann ist
    /// [`Messung::breite_stil_familie`] das alte [`Messung::breite_stil`].
    ///
    /// GEMESSEN, warum sie nicht vier Schnitte je Familie als Feld traegt:
    /// eine Seite hat GEMESSEN **3** verschiedene Familien
    /// (`schrift_familien.html`), aber eine *unbekannte* Familie darf nicht
    /// still zur Vorgabe fallen — ein Bogen `Arc<dyn FamilienStapeln>` kann
    /// jede Familie nachladen, ein Feld nicht.
    familien_quelle: Option<Arc<dyn FamilienStapeln>>,
}

/// Die Zahl der Schnitte: normal, fett, kursiv, fett-kursiv.
///
/// GEMESSEN, warum vier und nicht zwei Achsen in einem Struct: ein
/// `[Option<Arc<..>>; 4]` passt in den Speicher eines Arrays, und die
/// Rechnung ist ein Index statt eines Vergleichs. Die Reihenfolge ist
/// [`schnitt_index`].
pub const SCHNITTE: usize = 4;

/// Der Platz eines Schnitts im Feld [`Messung::schnitte`].
///
/// GEMESSEN, warum `(fett, kursiv)` und nicht die umgekehrte Reihenfolge:
/// die Reihenfolge ist sonst beliebig, und eine beliebige Reihenfolge wird
/// an der einen Stelle, die sie braucht, falsch. Mit `fett * 2 + kursiv`
/// ist 0 = normal, 1 = fett, 2 = kursiv, 3 = fett-kursiv — und der
///umbruch-sensitive Teil (fett) steht vorn.
#[must_use]
pub fn schnitt_index(fett: bool, kursiv: bool) -> usize {
    usize::from(fett) * 2 + usize::from(kursiv)
}

impl Messung {
    /// Die eingebaute Bitmap-Schrift (5×7, Monospace, 6/7 em).
    pub fn bitmap() -> Self {
        Self {
            tt: None,
            stapel: None,
            schnitte: None,
            familien_quelle: None,
        }
    }

    /// Die Systemschrift.
    pub fn systemschrift(tt: Arc<TrueTypeFont>) -> Self {
        Self {
            tt: Some(tt),
            stapel: None,
            schnitte: None,
            familien_quelle: None,
        }
    }

    /// Hauptschrift + Stapel. Schrift 0 des Stapels sollte die Hauptschrift
    /// sein, damit lateinischer Text genau so misst wie ohne Stapel.
    pub fn mit_stapel(tt: Arc<TrueTypeFont>, stapel: Arc<Schriftstapel>) -> Self {
        Self {
            tt: Some(tt),
            stapel: Some(stapel),
            schnitte: None,
            familien_quelle: None,
        }
    }

    /// Der Stapel, falls einer gesetzt ist.
    pub fn stapel(&self) -> Option<Arc<Schriftstapel>> {
        self.stapel.clone()
    }

    /// Die Systemschrift als `Arc`, falls eine gewaehlt wurde.
    ///
    /// Der Aufrufer, der eine Seite BAUT (statt sie zu messen),
    /// braucht die Schrift selbst — `narsil-page` zum Beispiel, um die
    /// Umgebungen des Markdown-Renderers aufzuloesen.
    pub fn tt_font(&self) -> Option<Arc<TrueTypeFont>> {
        self.tt.clone()
    }

    /// `true`, wenn mit einer Systemschrift gemessen wird.
    pub fn ist_systemschrift(&self) -> bool {
        self.tt.is_some()
    }

    /// Nimmt die vier Schnitte dazu — der Weg des Browsers.
    ///
    /// GEMESSEN 04.10.2026, warum der Aufrufer die Dateien **bringt** und
    /// diese Crate sie nicht sucht: `narsil-raster` waehlt den Schnitt
    /// (`Schrift::mit_gewicht`, schrift.rs:654, mit der Rangliste
    /// `BEKANNT_SCHRIFTEN`) und zeichnet mit ihm. Wenn `narsil-layout` die
    /// Datei selbst suchen wuerde, gaebe es **zwei** Waehler — und die
    /// koennten auseinanderlaufen, ohne dass es jemand bemerkt: der
    /// Umbruch misst mit der einen Datei, der Raster zeichnet mit der
    /// anderen, und der Absatz bricht an anderer Stelle um als in Firefox.
    /// GEMESSEN, dass die Bruecke existiert: `Schrift::tt_font()`
    /// (schrift.rs:396) gibt genau die `Arc<TrueTypeFont>` heraus, die
    /// gezeichnet wird.
    ///
    /// GEMESSEN, warum `self` zurueckkommt und nicht `&mut self`: die
    /// Messung wird in einer Kette von Aufrufern weitergegeben
    /// (`Sitzung` → Seite → Absatz), und eine zweite Besitzvorstellung
    /// waere dort ein `&mut` durch drei Ebenen.
    ///
    /// GEMESSEN, warum ein Platz `None` sein darf: fehlt ein Schnitt
    /// (die Familie hat kein `BoldOblique`), misst [`Messung::breite_stil`]
    /// an seiner Stelle mit dem regulaeren — das ist genau das, was der
    /// Raster tut, wenn `suche_schnitt` keinen findet.
    ///
    /// GEMESSEN, warum hier **Stapel** und nicht `Arc<TrueTypeFont>`
    /// kommen (die erste Fassung nahm die Datei): siehe das Feld. Der
    /// Aufrufer baut die Stapel mit [`Schriftstapel::mit_vorrang`] —
    /// Schnitt zuerst, die bisherige Kette dahinter.
    #[must_use]
    pub fn mit_schnitten(mut self, schnitte: [Option<Arc<Schriftstapel>>; SCHNITTE]) -> Self {
        self.schnitte = Some(schnitte);
        self
    }

    /// Die Breite von `text` bei `grad` **mit** `font-weight`/
    /// `font-style` — die Breite, mit der spaeter gezeichnet wird.
    ///
    /// GEMESSEN 04.10.2026 (der Befund, den diese Funktion behebt): an
    /// `texttest.html` waren die vier Stilzeilen 128 / 145 / 130 / 144
    /// Spalten breit (Firefox 128 / 145 / 130 / 144), der Umbruch kannte
    /// aber nur **eine** Breite — die des regulaeren Schnittes. Ein
    /// fetter Absatz bricht damit an einer anderen Stelle um als in
    /// Firefox und ragt als gesperrter Buchstabenzauber aus der Box
    /// (das ist die Fehlerform aus DEVPANEL_6).
    ///
    /// GEMESSEN, warum `gewicht` als `u16` und nicht als `bool`
    /// ankommt: CSS Fonts 4 §5.3 kennt 1..1000, und die Kaskade rechnet
    /// (`Berechnet::schriftgewicht`). Ob daraus ein echter Schnitt wird,
    /// entscheidet `narsil_text::gewicht_ist_fett` — dieselbe Schwelle
    /// (600), die `Schrift::mit_gewicht` benutzt. Zwei Schwellen waeren
    /// zwei Wahrheiten.
    ///
    /// Ohne Schnitte (kein `mit_schnitten`) ist das Ergebnis **gleich**
    /// [`Messung::breite`]: das ist der Zustand vor der Kette, und ein
    /// still anderes Ergebnis waere eine Behauptung ohne Weg.
    pub fn breite_stil(&self, text: &str, grad: f32, gewicht: u16, kursiv: bool) -> f32 {
        let Some(schnitte) = self.schnitte.as_ref() else {
            return self.breite(text, grad);
        };
        let index = schnitt_index(narsil_text::gewicht_ist_fett(gewicht), kursiv);
        match schnitte.get(index).and_then(|s| s.as_deref()) {
            // Der Schnitt ist da: messen ueber **seinen** Stapel, also mit
            // seinen Vorschuben und seinem Kerning. GEMESSEN, warum das
            // derselbe Weg ist, den der Raster zeichnet: der Raster geht
            // ueber `Schrift::zeichne` → `layout_text` mit dem Schnitt,
            // und beide nehmen die Glyphen aus derselben Datei.
            Some(st) => st.breite(&kuerzen(text), grad),
            // Kein Schnitt: der regulaere Weg, wie im Raster.
            None => self.breite(text, grad),
        }
    }

    /// Die Quelle fuer Familien anhaengen — einmal, beim Bauen.
    ///
    /// GEMESSEN, warum das ein `Arc` und kein `Box` ist: `Messung` wird
    /// an vier Stellen kopiert (Seitenbau, Layout, Raster, Aufrufer), und
    /// die Quelle ist eine Dateisystemsuche mit Cache — die teilen alle.
    #[must_use]
    pub fn mit_familienquelle(mut self, quelle: Arc<dyn FamilienStapeln>) -> Self {
        self.familien_quelle = Some(quelle);
        self
    }

    /// [`Messung::breite_stil`] **mit** Familie: die Breite, mit der der
    /// Raster spaeter zeichnet.
    ///
    /// GEMESSEN 04.10.2026 (SCHRIFT_9), was das behebt: Seriftext wurde mit
    /// Sans-Metriken umbrochen. Ein Absatz aus Wikipedia brach GEMESSEN
    /// ein Wort zu spaet um („formal", GEMESSEN 45 px), weil unsere
    /// Messung GEMESSEN 1267,88 px sagte, waehrend die Zeichnung
    /// GEMESSEN 1292 px machte — also 12 px ueber das Fenster.
    ///
    /// DIE DREI FAELLE:
    /// * Familie leer oder keine Quelle: [`Messung::breite_stil`] —
    ///   genau das alte Verhalten.
    /// * Die Quelle kennt die Familie: ihr Stapel, wie bei den Schnitten.
    /// * Die Quelle kennt sie **nicht** (leer, unbekannt, nicht lesbar):
    ///   wieder [`Messung::breite_stil`]. CSS Fonts 4 §4.2 behandelt eine
    ///   fehlende Familie wie die Vorgabe — und „leer" darf nicht
    ///   bedeuten „0 px".
    pub fn breite_stil_familie(
        &self,
        text: &str,
        grad: f32,
        gewicht: u16,
        kursiv: bool,
        familie: &str,
    ) -> f32 {
        let liste = familie.trim();
        if liste.is_empty() {
            return self.breite_stil(text, grad, gewicht, kursiv);
        }
        let Some(quelle) = self.familien_quelle.as_ref() else {
            return self.breite_stil(text, grad, gewicht, kursiv);
        };
        match quelle.stapel_fuer(liste, narsil_text::gewicht_ist_fett(gewicht), kursiv) {
            Some(st) => st.breite(&kuerzen(text), grad),
            None => self.breite_stil(text, grad, gewicht, kursiv),
        }
    }

    /// Die Breite von `text` bei `grad` — DIE eine Messfunktion.
    ///
    /// M3: der Text wird auf [`MAX_ZEICHEN`] Zeichen gekuerzt, BEVOR
    /// gemessen wird. Eine Messung ist eine Schleife ueber die Eingabe,
    /// und eine Schleife ueber die Eingabe braucht eine Grenze.
    pub fn breite(&self, text: &str, grad: f32) -> f32 {
        let gekuerzt = kuerzen(text);
        if let Some(st) = self.stapel.as_deref() {
            return st.breite(&gekuerzt, grad);
        }
        match self.tt.as_deref() {
            Some(f) => measure_tt(f, &gekuerzt, grad).map_or(0.0, |m| m.width),
            None => {
                let bitmap = BitmapFont::builtin();
                measure(&bitmap, &gekuerzt, grad).map_or(0.0, |m| m.width)
            }
        }
    }

    /// Der Vorschub EINES Zeichens bei `grad`.
    ///
    /// Das ist die zweite Zahl, die Layout und Zeichnen teilen muessen:
    /// der Rasterizer zeichnet jedes Zeichen einzeln, und wenn er
    /// einen anderen Vorschub nimmt als der Umbruch vorsieht, rutscht
    /// der Text auseinander (gemessen 30.09.2026, 09:20: sichtbar
    /// gesperrte Buchstaben, weil der Zeichner den Bitmap-Vorschub
    /// 13.71 px nahm und der Umbruch hmtx 7 px).
    ///
    /// Bei einem Zeichen, das die Schrift nicht kennt, gilt der
    /// Vorschub des Leerzeichens — nicht 0. Ein 0 wuerde den Rest des
    /// Satzes ueber den Zeichen stapeln.
    pub fn vorschub(&self, ch: char, grad: f32) -> f32 {
        if let Some(st) = self.stapel.as_deref() {
            if let Some(q) = st.glyphe_fuer(ch) {
                return st.vorschub(q, grad);
            }
        }
        match self.tt.as_deref() {
            Some(f) => f
                .glyph_index(ch)
                .and_then(|i| f.glyph_metrics(i))
                .map(|m| m.advance_width * grad / f.units_per_em() as f32)
                .unwrap_or_else(|| self.vorschub_fallback(grad)),
            None => BitmapFont::builtin()
                .advance(ch, grad)
                .unwrap_or_else(|| self.vorschub_fallback(grad)),
        }
    }

    /// Release 11.10.2026: Aufstieg und Abstieg (beide positiv, px) der
    /// Schrift, mit der `familie` GEZEICHNET wird (der Raster setzt die
    /// Grundlinie `ascent_em * grad` unter den Lauf-Ursprung) — fuer den
    /// halben Durchschuss (CSS 2.1 §10.8.1). Ohne Familie: die Grundschrift.
    pub fn zeilen_metrik_familie(&self, grad: f32, gewicht: u16, kursiv: bool, familie: &str) -> (f32, f32) {
        let aus = |f: &TrueTypeFont| {
            let z = f.zeilenmetriken();
            (z.ascent_em.max(0.0) * grad, (-z.descent_em).max(0.0) * grad)
        };
        let liste = familie.trim();
        if !liste.is_empty()
            && let Some(q) = self.familien_quelle.as_ref()
            && let Some(st) = q.stapel_fuer(liste, narsil_text::gewicht_ist_fett(gewicht), kursiv)
            && let Some(s) = st.schrift(0)
        {
            return aus(&s.font);
        }
        if let Some(st) = self.stapel.as_deref()
            && let Some(s) = st.schrift(0)
        {
            return aus(&s.font);
        }
        if let Some(f) = self.tt.as_deref() {
            return aus(f);
        }
        let a = grad * narsil_text::ASCENT_RATIO;
        (a, (grad - a).max(0.0))
    }

    /// Der Aufstieg: der Abstand der Grundlinie von oben.
    ///
    /// GEMESSEN, warum fuer die Bitmap 5/7 und fuer die Schrift ihr
    /// hhea-Wert: 5/7 ist die Zahl der eingebauten 5×7-Zelle, und sie
    /// ist keine Zahl, die man einer anderen Schrift unterschiebt.
    pub fn aufstieg(&self, grad: f32) -> f32 {
        match self.tt.as_deref() {
            Some(f) => measure_tt(f, "Hxg", grad).map_or(grad * 0.8, |m| m.ascent),
            None => grad * narsil_text::ASCENT_RATIO,
        }
    }

    /// Der Vorschub, wenn die Schrift das Zeichen nicht kennt.
    ///
    /// GEMESSEN (30.09.2026, 10:15): bei der Bitmap ist der Vorschub
    /// `6/7 * grad` fuer JEDES Zeichen; bei TrueType ist der
    /// Leerzeichen-Vorschub (~0.28 em) die beste Naeherung, weil es das
    /// Zeichen mit der kleinsten Breite in fast jeder Schrift ist.
    fn vorschub_fallback(&self, grad: f32) -> f32 {
        match self.tt.as_deref() {
            Some(_) => self.vorschub_bekannt(' ', grad),
            None => 6.0 / 7.0 * grad,
        }
    }

    /// Der Vorschub eines Zeichens OHNE Fallback.
    fn vorschub_bekannt(&self, ch: char, grad: f32) -> f32 {
        match self.tt.as_deref() {
            Some(f) => f
                .glyph_index(ch)
                .and_then(|i| f.glyph_metrics(i))
                .map(|m| m.advance_width * grad / f.units_per_em() as f32)
                .unwrap_or(0.0),
            None => BitmapFont::builtin().advance(ch, grad).unwrap_or(0.0),
        }
    }
}

/// Kuerzt auf [`MAX_ZEICHEN`] Zeichen, ohne mitten in einem Zeichen zu
/// trennen.
///
/// GEMESSEN, warum `char_indices`: `&text[..n]` an einer Byte-Grenze
/// panict im UTF-8 mitten in einem Mehrbyte-Zeichen. Der Umriss dieser
/// Funktion ist ein Zeichen, keine Byte-Zahl.
fn kuerzen(text: &str) -> std::borrow::Cow<'_, str> {
    match text.char_indices().nth(MAX_ZEICHEN) {
        Some((i, _)) => std::borrow::Cow::Borrowed(&text[..i]),
        None => std::borrow::Cow::Borrowed(text),
    }
}
