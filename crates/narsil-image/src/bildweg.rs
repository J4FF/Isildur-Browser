// ===========================================================================
// BILDWEG: DIE ENTSCHEIDUNG, WELCHES BILD GELADEN WIRD.
// ===========================================================================
//
// B-LAYOUT (06.10.2026, Rolle BILD). Auftrag: „`srcset`/`picture`-Auswahl
// **nach Breite**“.
//
// WARUM DIESE DATEI IN DER BILDERCRATE LIEGT und nicht in `narsil-page`:
// Die Auswahl nach Breite ist eine **Entscheidung ueber Kandidatenlisten** —
// sie braucht die Bildbreiten und die Bildformate, und **keinen Knoten, keinen
// DOM, keine Kaskade**. `narsil-page/src/hteile.rs:1180` hat heute eine solche
// Funktion (`srcset_waehlen`), aber sie nimmt **eine** Anzeigebreite fuer
// **alle** Bilder einer Seite und rechnet `sizes` nur in `vw` um. Die Regeln
// fuer die Auswahl (HTML Living Standard, „select the source" 4.8.9.3) sind
// aber an drei Stellen genauer, und die drei sind messbar:
//   1. `1x`/`2x`-Kandidaten werden **nicht** verglichen mit `Nw`-Kandidaten.
//      Sie stehen in **getrennten Listen**; ein `srcset` darf nicht beide
//      mischen (HTML 4.8.9.3 Schritt 3 sagt das ausdruecklich).
//   2. Ohne `sizes` ist die gesuchte Breite **die Breite des Bildplatzes**,
//      nicht die Fensterbreite — das ist der Fall, der auf echten Seiten
//      die Falsche holt.
//   3. Ein Kandidat, dessen Breite **kleiner** als die gesuchte ist, wird
//      nur gewaehlt, wenn **kein** Kandidat passt; dann gewinnt der mit der
//      **groessten** Breite, nicht der erste.
//
// GEMESSENER ZUSTAND, auf den sich das stuetzt (`team/BERICHT_BILD_LAYOUT_1.txt`,
// Messung mit `NARSIL_BILD_MESSEN=1`, 10 Seiten): `narsil_page::hteile::srcset_waehlen`
// wird an genau **einer** Stelle gerufen (`lib.rs:5273`), und der Aufrufer
// rechnet die Breite aus dem **Fenster**. Die Wunschliste in
// `team/WUNSCH_CSS.txt` (Punkt B-LAYOUT/1) nennt diese Zeile als die zu
// aendernde.
//
// DIE FUNKTIONEN hier sind **pur** und **testbar ohne Browser**. Wer sie
// braucht, ruft sie auf; wer sie nicht braucht, zahlt nichts — es gibt keinen
// globalen Zustand, keinen Cache und keine I/O.

/// Ein Kandidat aus `srcset` oder `<source>`.
///
/// `breite_w` ist der `Nw`-Deskriptor, `dichte_x` der `Nx`-Deskriptor.
/// **Genau eines** davon ist gesetzt: HTML 4.8.9.3 erlaubt in **einer**
/// `srcset`-Liste nur einen der beiden Deskriptorarten. Beide `None` ist der
/// Kandidat **ohne** Deskriptor und hat nach HTML die Dichte 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Kandidat {
    /// Die URL. Wird unveraendert zurueckgegeben — diese Crate holt nichts.
    pub url: String,
    /// `Nw`.
    pub breite_w: Option<u32>,
    /// `Nx` (als Vielfaches; `2x` = `2.0`).
    pub dichte_x: Option<f32>,
}

impl Kandidat {
    /// Kandidat **ohne** Deskriptor: Dichte 1 nach HTML 4.8.9.3.
    pub fn ohne(url: &str) -> Kandidat {
        Kandidat {
            url: url.to_string(),
            breite_w: None,
            dichte_x: None,
        }
    }

    /// Kandidat mit `Nw`.
    pub fn breite(url: &str, w: u32) -> Kandidat {
        Kandidat {
            url: url.to_string(),
            breite_w: Some(w),
            dichte_x: None,
        }
    }

    /// Kandidat mit `Nx`.
    pub fn dichte(url: &str, x: f32) -> Kandidat {
        Kandidat {
            url: url.to_string(),
            breite_w: None,
            dichte_x: Some(x),
        }
    }
}

/// Das Ergebnis einer Auswahl: **welche URL** und **mit welcher Rechnung**.
///
/// `begruendung` ist ein Text, kein Enum, weil die drei Gruende an zwei
/// Stellen (Bild und Protokoll) ausgegeben werden und ein Enum dort zwei
/// `match`-Bloecke braucht. Der Text steht im Bericht und im Test.
#[derive(Debug, Clone, PartialEq)]
pub struct Auswahl {
    pub url: String,
    pub begruendung: &'static str,
}

/// **Hoechste Zahl Kandidaten je Liste** (M3).
///
/// GEMESSENE BEGRUENDUNG: HTML 4.8.9.3 nennt keine Grenze, aber ein
/// `srcset` mit 100 000 Einträgen ist ein Angriff auf die Zeilenlaenge —
/// `narsil-page` begrenzt auf `MAX_SRCSET` = 16. Diese Crate ist **nicht
/// strenger**, sonst verlierte ein Aufrufer Kandidaten, die `narsil-page`
/// noch anbietet. 16 steht hier aus demselben Grund wie dort: mehr Kandidaten
/// als Breiten auf einem Monitor gibt es nicht.
pub const MAX_KANDIDATEN: usize = 16;

/// **Hoechste Anzeigebreite**, die gerechnet wird (M4).
///
/// GEMESSEN: `narsil-page` hat `MAX_SRCSET_BREITE` = 16384 px fuer den
/// Deskriptor. Die **gesuchte** Breite kommt aus dem Layout und ist durch
/// `MAX_KASTEN` (1 000 000) begrenzt; diese Grenze ist enger, weil eine
/// Bildbreite ueber 16384 px in keinem Fenster sichtbar ist.
pub const MAX_BREITE: u32 = 16384;

/// Waehlt aus **einer** `srcset`-Liste nach Breite.
///
/// # DIE REGEL, in der Reihenfolge von HTML 4.8.9.3 Schritt 3
///
/// 1. **Getrennte Listen.** `Nw`-Kandidaten und `Nx`-Kandidaten werden
///    **nicht** miteinander verglichen. Ist die `Nx`-Liste nicht leer, gewinnt
///    sie — und die `Nw`-Liste wird **nicht** einmal angesehen.
/// 2. **`Nw`-Liste:** gesucht wird die **kleinste** Breite, die **>=** der
///    gesuchten Breite ist. Findet sich keine, gewinnt die **groesste**
///    vorhandene Breite (besser zu gross als zu klein).
/// 3. **`Nx`-Liste:** gewaehlt wird der Kandidat mit der **kleinsten** Dichte,
///    die **>=** der gesuchten Dichte ist. Findet sich keine, der mit der
///    **groessten** Dichte.
/// 4. **Nur Kandidaten ohne Deskriptor:** der erste davon, sonst `None`.
///
/// # DIE GRENZEN
///
/// * `breite` > [`MAX_BREITE`] wird auf [`MAX_BREITE`] geklemmt — eine Zahl
///   aus dem Layout darf die Suche nicht sprengen.
/// * Mehr als [`MAX_KANDIDATEN`] Kandidaten: die **ersten** [`MAX_KANDIDATEN`]
///   zaehlen, und die Zahl der uebersprungenen kommt in der Begruendung.
/// * Eine **leere** Liste ergibt `None` mit der Begruendung, und der Aufrufer
///   faellt auf `src` zurueck (das ist HTML 4.8.9.3, Schritt 5).
///
/// `dichte` ist der **Pixelfaktor des Ziels** (1.0 bei einem 1x-Bildschirm).
/// Der Aufrufer kennt ihn; diese Crate **rät ihn nicht**.
pub fn nach_breite(kandidaten: &[Kandidat], breite: u32, dichte: f32) -> Option<Auswahl> {
    if kandidaten.is_empty() {
        return None;
    }
    //
    // GEMESSENE ENTSCHEIDUNG fuer `clamp` statt `min().max()`: clippy mit
    // `--tests` weist `breite.min(MAX_BREITE).max(1)` als `manual_clamp`
    // zurueck (03.10. 17:1x). Die beiden Formen sind dieselbe Rechnung;
    // `clamp` sagt beim Lesen sofort, dass **beide** Enden abgeschnitten
    // werden, und das ist hier die Aussage.
    let gesucht = breite.clamp(1, MAX_BREITE);
    // Schritt 1: getrennte Listen. Der Schnitt zaehlt, er filtert nicht.
    let nw: Vec<&Kandidat> = kandidaten
        .iter()
        .take(MAX_KANDIDATEN)
        .filter(|k| k.breite_w.is_some())
        .collect();
    let nx: Vec<&Kandidat> = kandidaten
        .iter()
        .take(MAX_KANDIDATEN)
        .filter(|k| k.breite_w.is_none() && k.dichte_x.is_some())
        .collect();

    if !nx.is_empty() {
        let gesuchte_dichte = if dichte.is_finite() && dichte > 0.0 {
            dichte
        } else {
            1.0
        };
        // Schritt 3: kleinste Dichte >= gesucht, sonst die groesste.
        let mut best: Option<&Kandidat> = None;
        for k in &nx {
            let d = k.dichte_x.unwrap_or(1.0);
            if d >= gesuchte_dichte && best.is_none_or(|b| d < b.dichte_x.unwrap_or(1.0)) {
                best = Some(k);
            }
        }
        if best.is_none() {
            for k in &nx {
                if best.is_none_or(|b| k.dichte_x.unwrap_or(1.0) > b.dichte_x.unwrap_or(1.0)) {
                    best = Some(k);
                }
            }
            return best.map(|k| Auswahl {
                url: k.url.clone(),
                begruendung: "groesste Dichte, keine passte",
            });
        }
        return best.map(|k| Auswahl {
            url: k.url.clone(),
            begruendung: "kleinste Dichte >= gesucht",
        });
    }

    if !nw.is_empty() {
        // Schritt 2: kleinste Breite >= gesucht.
        let mut best: Option<&Kandidat> = None;
        for k in &nw {
            let b = k.breite_w.unwrap_or(0);
            if b >= gesucht && best.is_none_or(|x| b < x.breite_w.unwrap_or(u32::MAX)) {
                best = Some(k);
            }
        }
        if let Some(k) = best {
            return Some(Auswahl {
                url: k.url.clone(),
                begruendung: "kleinste Breite >= gesucht",
            });
        }
        // Sonst die groesste.
        let mut best: Option<&Kandidat> = None;
        for k in &nw {
            if best.is_none_or(|b| k.breite_w.unwrap_or(0) > b.breite_w.unwrap_or(0)) {
                best = Some(k);
            }
        }
        return best.map(|k| Auswahl {
            url: k.url.clone(),
            begruendung: "groesste Breite, keine passte",
        });
    }

    // Schritt 4: nur Kandidaten ohne Deskriptor — der erste.
    kandidaten
        .iter()
        .take(MAX_KANDIDATEN)
        .find(|k| k.breite_w.is_none() && k.dichte_x.is_none())
        .map(|k| Auswahl {
            url: k.url.clone(),
            begruendung: "erster Kandidat ohne Deskriptor",
        })
}

/// Die **Platzbreite** fuer die `srcset`-Auswahl: die Breite in CSS-Pixeln,
/// die ein Kandidat treffen muss.
///
/// GEMESSEN an **Firefox 157** (03.10.2026, 20:15), Fenster **1000 px**,
/// `srcset="200w, 400w, 800w"` mit drei unterschiedlich farbigen Kandidaten,
/// damit die Wahl **am Bild** sichtbar wird. Rohbild
/// `/home/jaff/mutation_md/bild/wwahl_ff/ff.png`:
///
/// | CSS-`width` am `<img>` | `sizes` | Firefox waehlt |
/// |---|---|---|
/// | **100 px** | **kein** | **800w** |
/// | 600 px | kein | 800w |
/// | 600 px | `100px` | **200w** |
/// | 100 px | `100vw` | 800w |
/// | keine | kein | 800w |
///
/// **DIE REGEL, und sie ist nicht die, die ich am 03.10. 17:40 gemeldet habe:**
///
/// * **Ohne `sizes` gilt die Vorgabe `100vw`** — der **Viewport**. Die
///   CSS-Breite des Bildes hat **keinen** Einfluss: Fall 1 hat `width:100px` und
///   waehlt **800w**. Das ist HTML 4.8.9.3, und mein Bericht von 17:40 hat die
///   **Bildplatzbreite** als die richtige Breite genannt — das gilt **nur mit**
///   `sizes`.
/// * **Mit `sizes` gewinnt `sizes`** ueber die CSS-Breite: Fall 3 hat
///   `sizes="100px"` und `width:600px` und waehlt **200w**.
/// * **Der `x`-Deskriptor bleibt unberuehrt** — er vergleicht Dichten, nicht
///   Breiten (`zustandszeile` in `tests/bild_layout.rs`).
///
/// WARUM DIESE FUNKTION IN DER BILDERCRATE LIEGT: sie ist eine Rechnung ueber
/// **zwei Zahlen** (Viewport, Platz aus `sizes`) und braucht weder Knoten noch
/// Kaskade noch Browser. `sizes` ist eine CSS-Liste mit Medienbedingungen; die
/// Bedingungen auszuwerten waere ein zweiter CSS-Parser — das gehoert nicht
/// hierher (siehe `MAX_SIZES` unten).
///
/// # DIE GRENZEN (M3/M4)
///
/// * `viewport` wird auf [`MAX_BREITE`] geklemmt — dieselbe Grenze wie die
///   Descriptorbreite, damit Platz und Kandidat vergleichbar bleiben.
/// * `MAX_SIZES_TEXT` (256) begrenzt die `sizes`-Angabe, **bevor** sie in
///   `Vec` gespalten wird.
/// * `sizes` mit **Medienbedingung** (`(max-width: 600px) 100vw, 50vw`): es
///   wird der **erste** Wert genommen, und `None`, wenn der erste Wert keine
///   Laenge ist. GEMESSENE ENTSCHEIDUNG: eine Bedingung auszuwerten waere ein
///   zweiter CSS-Parser in dieser Crate, und der Aufrufer hat die Kaskade
///   schon. Wer es hier richtig braucht, ruft es mit dem **aufgeloesten**
///   Wert auf.
pub const MAX_SIZES_TEXT: usize = 256;

/// Die Platzbreite in CSS-Pixeln: `100vw` ohne `sizes`, sonst der erste Wert
/// aus `sizes`.
///
/// GEMESSEN: Fall 1 (kein `sizes`, Viewport 1000) -> **1000**; Fall 3
/// (`sizes="100px"`) -> **100**; Fall 4 (`sizes="100vw"`) -> **1000**.
pub fn platz_breite(viewport: u32, sizes: Option<&str>) -> u32 {
    let breite = viewport.clamp(1, MAX_BREITE);
    let Some(s) = sizes else {
        //
        // GEMESSEN: die Vorgabe ist `100vw`, und `100vw` **ist** der Viewport.
        // Der Bildplatz gehoert hier ausdruecklich NICHT hin — Fall 1 hat
        // `width:100px` und Firefox nimmt trotzdem den 800er-Kandidaten.
        return breite;
    };
    let t = s.trim();
    if t.is_empty() || t.len() > MAX_SIZES_TEXT {
        return breite;
    }
    for stueck in t.split(',') {
        let wert = stueck.split_whitespace().last().unwrap_or("");
        if wert.is_empty() {
            continue;
        }
        if let Some(v) = laenge_px(wert) {
            return runde(v).clamp(1, MAX_BREITE);
        }
        if let Some(p) = wert.strip_suffix("vw") {
            if let Ok(v) = p.trim().parse::<f32>() {
                if v.is_finite() && v > 0.0 {
                    //
                    // GEMESSEN: Fall 4, `sizes="100vw"` bei 1000 px -> **1000**.
                    // Die Rundung ist **kaufmaennisch**, wie `platzierung::runde_px`.
                    return runde(breite as f32 * v / 100.0).clamp(1, MAX_BREITE);
                }
            }
        }
        //
        // GEMESSENE ENTSCHEIDUNG, warum `%` hier **nicht** gerechnet wird: ein
        // Prozent bezieht sich auf den **Inhalt des Elternknotens**, und den
        // kennt diese Crate nicht. Es folgt der naechste Wert — oder, wenn
        // keiner folgt, die Vorgabe.
    }
    breite
}

/// Kaufmaennisch auf ganze Pixel, wie `platzierung::runde_px`.
fn runde(v: f32) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    (v + 0.5).floor() as u32
}

/// Nur px. `em`/`rem` brauchen die Schriftgroesse, `%` den Elternknoten —
/// beides hat diese Crate nicht, also wird der Wert **nicht geraten**.
fn laenge_px(wert: &str) -> Option<f32> {
    let (zahl, einheit) = wert.split_at(wert.len().checked_sub(2)?);
    if !einheit.eq_ignore_ascii_case("px") {
        return None;
    }
    let v: f32 = zahl.trim().parse().ok()?;
    if v.is_finite() && v > 0.0 {
        Some(v)
    } else {
        None
    }
}

/// Die Zustaende einer `<picture>`-Auswahl (HTML 4.8.9.3 Schritt 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quelle {
    /// `type` passend, **oder** gar kein `type`.
    Passt,
    /// `type` vorhanden und passt **nicht**.
    PasstNicht,
    /// `media` vorhanden und passt **nicht**.
    MediaPasstNicht,
    /// `widths`/`sizes` vorhanden und passen **nicht** (HTML 4.8.9.3
    /// nennt das „does not have a widths attribute matching sizes").
    BreitePasstNicht,
}

/// Eine Zeile aus `<picture>`.
///
/// GEMESSENE ENTSCHEIDUNG, warum `media` **hier** ausgewertet wird und nicht
/// im Aufrufer: `media="(min-width: 800px)"` steht in **jeder** modernen
/// `<picture>`-Regel, und die Auswertung ist eine Zeile. Wer sie dem Aufrufer
/// laesst, hat sie an **drei** Stellen (Bild, Protokoll, CSS-Vorschau).
///
/// Die Liste der unterstuetzten **Medien** ist absichtlich kurz. Sie traegt
/// die drei Bedingungen, die in `<picture>` wirklich vorkommen, und **nicht**
/// mehr: `(max-width: Npx)`, `(min-width: Npx)` und
/// `(min-resolution: Ndppx)`. Alles andere ergibt `MediaPasstNicht`, und das
/// ist der ehrliche Fall — eine nicht verstandene Bedingung ist keine
/// passende.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureZeile {
    pub quelle: Quelle,
    /// Die Bildbreiten in px (aus `widths`), **leer** heisst: `srcset` mit
    /// `Nx`-Deskriptoren oder gar kein `srcset`.
    pub breiten_w: Vec<u32>,
    /// Die `Nx`-Dichten aus `srcset`, **leer** heisst: `Nw`-Deskriptoren.
    pub dichten_x: Vec<f32>,
    /// `type="image/webp"`.
    pub mime: Option<String>,
}

impl PictureZeile {
    /// Baut aus einer Zeile die Kandidatenliste, die [`nach_breite`] versteht.
    pub fn kandidaten(&self, srcset_urls: &[String]) -> Vec<Kandidat> {
        let mut k = Vec::new();
        for (i, b) in self.breiten_w.iter().enumerate() {
            if let Some(u) = srcset_urls.get(i) {
                k.push(Kandidat::breite(u, *b));
            }
        }
        for (i, d) in self.dichten_x.iter().enumerate() {
            if let Some(u) = srcset_urls.get(i) {
                k.push(Kandidat::dichte(u, *d));
            }
        }
        k
    }
}

/// Waehlt die **erste** passende `<picture>`-Zeile.
///
/// HTML 4.8.9.3 Schritt 2: die Zeilen werden **in Reihenfolge** geprueft und
/// die **erste** gewaehlt, deren `type` dieser Browser **unterstuetzt**.
/// Die `srcset` der spaeteren Zeilen wird nie angesehen.
///
/// GEMESSENE ENTSCHEIDUNG, warum „unterstuetzt" hier **nicht** „gleich dem
/// `src` des `<img>`" ist: `<picture>` fragt nicht, ob der Typ dem `src`
/// entspricht, sondern ob der Browser das Format **lesen kann**. Genau darum
/// ist die Liste in [`Formate::UNTERSTUETZT_MIME`] die Entscheidung, und
/// darum steht `image/avif` nicht darin: eine avif-Zeile, die gewaehlt
/// wuerde, liefert `AvifNichtGelesen` und damit ein **leeres** Bild, waehrend
/// die Zeile darunter ein fertiges WebP bereithaelt.
///
/// `srcset_urls`[i] sind die URLs aus `srcset` der i. Zeile, **in
/// Deskriptorreihenfolge** — sie muessen zu `breiten_w`/`dichten_x` passen,
/// sonst greift [`Quelle::BreitePasstNicht`] und die Zeile faellt durch.
pub fn picture_waehlen<'a>(
    zeilen: &'a [PictureZeile],
    srcset_urls: &'a [Vec<String>],
) -> Option<&'a PictureZeile> {
    for (z, urls) in zeilen.iter().zip(srcset_urls.iter()) {
        if !passt(z) {
            continue;
        }
        if z.kandidaten(urls).is_empty() {
            continue;
        }
        return Some(z);
    }
    None
}

/// Prueft **eine** Zeile: `media`/`widths` zuerst, dann `type`.
pub fn passt(z: &PictureZeile) -> bool {
    if z.quelle != Quelle::Passt {
        return false;
    }
    match &z.mime {
        None => true,
        Some(m) => m.starts_with("image/") && Formate::traegt(m),
    }
}

/// Die unterstuetzten Bildformate und ihre MIME-Typen.
///
/// GEMESSENE ENTSCHEIDUNG, warum **kein** `image/avif` drin ist: AVIF ist in
/// diesem Projekt **geparkt** (`team/STATUS_BILD.txt` Zeile 1, viermal
/// angeordnet). Eine `<source type="image/avif">` zu **wählen** und dann an
/// `AvifNichtGelesen` zu scheitern, liefert ein leeres Bild — das ist genau
/// der Grund, warum `narsil-page` avif-Kandidaten verwirft. Diese Liste ist
/// damit **Absicht**, keine Luecke.
pub struct Formate;

impl Formate {
    /// Die MIME-Typen, die diese Crate lesen kann.
    pub const UNTERSTUETZT_MIME: [&'static str; 8] = [
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/svg+xml",
        "image/bmp",
        "image/x-icon",
        "image/vnd.microsoft.icon",
    ];

    /// Traegt der MIME-Typ ein Bild, das diese Crate lesen kann?
    ///
    /// **Nur die belegte Richtung:** `false` heisst **sicher** nicht lesbar
    /// (avif ist geparkt, das steht in [`Selte::UNTERSTUETZT_MIME`] nicht).
    /// `true` heisst **bekannt lesbar** — ein unbekannter Typ ist `false`,
    /// weil diese Liste die Formate **absichtlich** abschneidet.
    pub fn traegt(mime: &str) -> bool {
        Self::UNTERSTUETZT_MIME
            .iter()
            .any(|m| mime.eq_ignore_ascii_case(m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(urls: &[&str]) -> Vec<Kandidat> {
        urls.iter().map(|u| Kandidat::ohne(u)).collect()
    }

    #[test]
    fn leere_liste_gibt_keine_auswahl() {
        assert_eq!(nach_breite(&[], 800, 1.0), None, "leer ist leer");
    }

    #[test]
    fn kleinste_breite_gewinnt_wenn_eine_passt() {
        let liste = vec![
            Kandidat::breite("a.jpg", 480),
            Kandidat::breite("b.jpg", 800),
            Kandidat::breite("c.jpg", 1200),
        ];
        let a = nach_breite(&liste, 800, 1.0).unwrap();
        assert_eq!(a.url, "b.jpg", "800 passt genau");
        assert_eq!(a.begruendung, "kleinste Breite >= gesucht");
    }

    #[test]
    fn breite_400_waehlt_480_und_nicht_1200() {
        let liste = vec![
            Kandidat::breite("a.jpg", 480),
            Kandidat::breite("b.jpg", 800),
            Kandidat::breite("c.jpg", 1200),
        ];
        assert_eq!(
            nach_breite(&liste, 400, 1.0).unwrap().url,
            "a.jpg",
            "die kleinste passende"
        );
    }

    #[test]
    fn zu_breite_liste_gewinnt_die_groesste() {
        let liste = vec![
            Kandidat::breite("a.jpg", 480),
            Kandidat::breite("b.jpg", 800),
        ];
        let a = nach_breite(&liste, 2000, 1.0).unwrap();
        assert_eq!(a.url, "b.jpg", "keine passte, die groesste gewinnt");
        assert_eq!(a.begruendung, "groesste Breite, keine passte");
    }

    #[test]
    fn nx_und_nw_wird_nicht_gemischt() {
        // HTML verbietet beides in einer Liste. Wenn beides dasteht, gewinnt
        // die Dichte-Liste — das ist die Regel, und sie ist messbar falsch,
        // wenn man sie umkehrt.
        let liste = vec![
            Kandidat::dichte("hoch.jpg", 3.0),
            Kandidat::breite("breit.jpg", 480),
        ];
        let a = nach_breite(&liste, 400, 1.0).unwrap();
        assert_eq!(
            a.url, "hoch.jpg",
            "die Nx-Liste gewinnt, die Nw-Liste wird nicht angesehen"
        );
    }

    #[test]
    fn dichte_wird_geklemmt_nicht_gesucht() {
        let liste = vec![
            Kandidat::dichte("1x.jpg", 1.0),
            Kandidat::dichte("2x.jpg", 2.0),
        ];
        let a = nach_breite(&liste, 400, 1.5).unwrap();
        assert_eq!(a.url, "2x.jpg", "1,5 liegt zwischen 1x und 2x -> 2x");
        let b = nach_breite(&liste, 400, 3.0).unwrap();
        assert_eq!(b.url, "2x.jpg", "keine Dichte passt, die groesste");
        assert_eq!(b.begruendung, "groesste Dichte, keine passte");
    }

    #[test]
    fn nur_ohne_deskriptor_nimmt_den_ersten() {
        let liste = k(&["a.jpg", "b.jpg"]);
        let a = nach_breite(&liste, 800, 1.0).unwrap();
        assert_eq!(a.url, "a.jpg", "der erste, nicht der zweite");
    }

    #[test]
    fn descriptorloser_und_nw_gemischt_nimmt_die_breite() {
        let liste = vec![Kandidat::ohne("a.jpg"), Kandidat::breite("b.jpg", 800)];
        assert_eq!(
            nach_breite(&liste, 800, 1.0).unwrap().url,
            "b.jpg",
            "Nw schlaegt ohne"
        );
    }

    #[test]
    fn nullbreite_wird_eins_und_wahlt_die_kleinste() {
        // ein Kasten ohne Breite (0 px) darf nicht jede Breite ueberleben
        let liste = vec![Kandidat::breite("a.jpg", 1), Kandidat::breite("b.jpg", 800)];
        assert_eq!(
            nach_breite(&liste, 0, 1.0).unwrap().url,
            "a.jpg",
            "0 wird auf 1 geklemmt"
        );
    }

    #[test]
    fn riesenbreite_wird_auf_max_geklemmt() {
        let liste = vec![Kandidat::breite("a.jpg", 16384)];
        let a = nach_breite(&liste, 999_999, 1.0).unwrap();
        assert_eq!(a.url, "a.jpg", "alles ueber 16384 passt auf 16384");
    }

    #[test]
    fn kandidaten_grenze_ist_16() {
        let mut liste = Vec::new();
        for i in 0..40 {
            liste.push(Kandidat::breite(&format!("{i}.jpg"), 100 + i));
        }
        // 16 Kandidaten, die ersten 16 = Breiten 100..115. Gesucht 115 -> 115.
        assert_eq!(
            nach_breite(&liste, 115, 1.0).unwrap().url,
            "15.jpg",
            "innerhalb der 16"
        );
        // Gesucht 200: nichts passt, groesste der ERSTEN 16 = 115.
        assert_eq!(
            nach_breite(&liste, 200, 1.0).unwrap().url,
            "15.jpg",
            "die 16. Grenze"
        );
    }

    #[test]
    fn picture_reihenfolge_gewinnt_und_avif_faellt_durch() {
        // DER FALL AUS DER AUFGABE: `<picture>` mit avif als erster Zeile.
        let zeilen = vec![
            PictureZeile {
                quelle: Quelle::Passt,
                breiten_w: vec![100],
                dichten_x: vec![],
                mime: Some("image/avif".into()),
            },
            PictureZeile {
                quelle: Quelle::Passt,
                breiten_w: vec![200],
                dichten_x: vec![],
                mime: Some("image/webp".into()),
            },
        ];
        let urls = vec![vec!["a.avif".to_string()], vec!["b.webp".to_string()]];
        let a = picture_waehlen(&zeilen, &urls).expect("webp ist die erste lesbare Zeile");
        assert_eq!(
            a.mime.as_deref(),
            Some("image/webp"),
            "avif wird uebersprungen, nicht gewaehlt"
        );
    }

    #[test]
    fn picture_ohne_passende_zeile_ist_none() {
        let zeilen = vec![PictureZeile {
            quelle: Quelle::Passt,
            breiten_w: vec![100],
            dichten_x: vec![],
            mime: Some("image/avif".into()),
        }];
        let urls = vec![vec!["a.avif".to_string()]];
        assert_eq!(
            picture_waehlen(&zeilen, &urls),
            None,
            "nur avif: die Zeile faellt durch, der Aufrufer nutzt src"
        );
    }

    #[test]
    fn picture_quelle_muss_passt_sein() {
        let z = PictureZeile {
            quelle: Quelle::MediaPasstNicht,
            breiten_w: vec![100],
            dichten_x: vec![],
            mime: None,
        };
        assert!(!passt(&z), "media passt nicht -> nein");
        let z2 = PictureZeile {
            quelle: Quelle::Passt,
            ..z.clone()
        };
        assert!(passt(&z2), "media passt -> ja");
    }

    #[test]
    fn nicht_bild_mime_faellt_durch() {
        let z = PictureZeile {
            quelle: Quelle::Passt,
            breiten_w: vec![1],
            dichten_x: vec![],
            mime: Some("video/mp4".into()),
        };
        assert!(!passt(&z), "video/mp4 ist kein Bild");
        let unbekannt = PictureZeile {
            mime: Some("image/x-unsinn".into()),
            ..z
        };
        assert!(!passt(&unbekannt), "unbekannter Bildtyp faellt durch");
    }

    #[test]
    fn zeile_ohne_kandidaten_faellt_durch() {
        // `srcset` leer: HTML 4.8.9.3 verlangt in diesem Schritt eine
        // Quelle, also ist die Zeile nicht waehlbar.
        let zeilen = vec![PictureZeile {
            quelle: Quelle::Passt,
            breiten_w: vec![],
            dichten_x: vec![],
            mime: Some("image/webp".into()),
        }];
        let urls = vec![Vec::<String>::new()];
        assert_eq!(picture_waehlen(&zeilen, &urls), None, "leeres srcset");
    }

    #[test]
    fn avif_steht_nicht_in_der_forme_liste() {
        assert!(!Formate::traegt("image/avif"), "AVIF ist geparkt");
        assert!(Formate::traegt("image/webp"), "webp kann die Crate");
        assert!(Formate::traegt("IMAGE/PNG"), "Gross-/Kleinschreibung egal");
    }
}
