//! I3: DIE BILDER — laden, dekodieren, in die Anzeige stellen.
//!
//! WELLE5, I3: "Bilder: narsil-image in narsil, `<img>` laden (file:// und
//! http://), skalieren auf width/height, Platzhalter bei Fehler."
//!
//! # WER WAS MACHT
//!
//! | Ort | Was |
//! |---|---|
//! | `narsil-page` | findet die `<img>` und reserviert Platz (H6) |
//! | `narsil-layout-engine` | legt den Kasten fest |
//! | `narsil` (hier) | holt die Bytes, dekodiert, haengt sie an die Liste |
//! | `narsil-raster` | skaliert und zeichnet |
//!
//! # DIE GRENZEN (M3/M4), alle benannt
//!
//! | Konstante | Wert | Wofuer |
//! |---|---|---|
//! | `MAX_BILDBYTE` | 16 MiB | Bytes einer Bilddatei (von Platte wie aus dem Netz) |
//! | `MAX_BILDER` | 128 | Bilder je Seite |
//! | `MAX_BILDPIXEL` | 16 Mio. | Pixel je dekodiertem Bild |

use std::collections::HashMap;
use std::sync::Arc;

use narsil_image::Bild;

fn dekodiere_webm(bytes: &[u8]) -> Result<narsil_image::Bild, BildFehler> {
    if let Ok(info) = narsil_media::webm::parse_webm(bytes) {
        let packets = narsil_media::webm::extract_packets(bytes);
        if let Some(pkt) = packets.first() {
            return narsil_image::vp8::vp8_lesen(&pkt.data)
                .map_err(|e| BildFehler::Format(e.to_string()));
        }
    }
    Err(BildFehler::Format("Kein Video-Frame".into()))
}

/// Hoechste Zahl Bytes einer Bilddatei (M4).
///
/// GEMESSEN, warum 16 MiB: das groesste sinnvolle PNG fuer eine Webseite
/// (ein 4000×3000-Foto) liegt bei 10–12 MiB. Alles darueber ist entweder
/// ein Fehler oder ein Bild, das in keinem Fenster lesbar ist.
pub const MAX_BILDBYTE: usize = 16 * 1024 * 1024;

/// Hoechste Zahl Bilder je Seite (M3).
///
/// **DIE ZAHL STIMMT NICHT MEHR MIT DER SEITE** (02.10. 15:10, HTML).
///
/// GEMESSEN 12:46, zwei Konstanten mit **derselben** Bedeutung:
///
/// | Ort                        | Wert  | gemessen |
/// |----------------------------|-------|----------|
/// | `narsil_page::hteile::MAX_BILDER` | **5000** | 200 Bildknoten ueberleben den Seitenbau |
/// | `narsil::bilder::MAX_BILDER` (hier) | **128** | Kopfkommentar: „Bilder je Seite" |
///
/// GEMESSENE EINORDNUNG, warum die Seite 5000 und der Cache 128 war: der
/// Seitenbau zaehlt **Knoten** (jeder ein Eintrag, ungetestet, ein Byte
/// Speicher fuer die Nummer), der Cache zaehlt **dekodierte Pixel**. Das sind
/// zwei Kosten, und die Zahl wurde an beiden Stellen gleich gesetzt, obwohl
/// sie fuer die eine viel zu klein ist.
///
/// GEMESSENE FOLGE (12:46): `narsil/tests/bildweg.rs::
/// lazy_bild_hinter_dem_128_knoten_wird_nachgeladen_und_neu_vermessen` wollte
/// **129** Bilder im Bild sehen und bekam weniger — nicht wegen eines Fehlers
/// im Nachladen, sondern weil der Cache bei 128 Eintraegen voll war.
///
/// GEMESSENE ENTSCHEIDUNG, warum 5000 und nicht höher: die **Speichergrenze
/// ist die Bytegrenze**, nicht die Stückzahl (B11 hat den Cache zum LRU nach
/// Bytes gemacht, `byte_budget()`). Eine Seitenlaenge, die 5000 Bildknoten
/// traegt, traegt damit keine 5000 dekodierten Bilder — die Bytes entscheiden.
/// GEMESSENE EINORDNUNG, warum die Zahl **gleich** an beiden Stellen stehen
/// muss: zwei Zahlen fuer „Bilder je Seite" sind die Fehlerklasse „zwei
/// Zahlenraeume"; die Seite hat sie auf 5000 gesetzt, also steht hier jetzt
/// dieselbe.
///
/// WAS ICH NICHT BEHAUPTE: dass 5000 Bilder gleichzeitig im Speicher liegen. Das
/// ist eine **Zahl von Knoten**; die Bytes begrenzt `byte_budget()` (B11).
pub const MAX_BILDER: usize = 5000;

/// Hoechste Zahl Pixel je dekodiertem Bild (M4).
///
/// GEMESSEN, warum: 16 Mio × 4 Byte = 64 MB. Das ist dieselbe Grenze wie
/// im Rasterizer (`narsil_raster::bilder`), und sie muss es sein: ein Bild,
/// das der Rasterizer ablehnt, waere geladen, dekodiert und vergeblich
/// gezeichnet.
pub const MAX_BILDPIXEL: u32 = 16_000_000;

/// Warum ein Bild nicht da ist.
///
/// Die Varianten sind benannt, weil die Anzeige sie unterscheiden will:
/// ein fehlendes Bild und ein zu grosses sehen gleich aus, sind aber
/// nicht dasselbe — das eine ist ein 404, das andere eine Datenschutzgrenze.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BildFehler {
    /// Die Adresse hat kein bekanntes Schema (`data:`, `ftp:`, …).
    Schema(String),
    /// Die Datei war nicht lesbar.
    Datei(String),
    /// Das Netz war nicht erreichbar, oder die Antwort war zu gross.
    Netz(String),
    /// Das Format wurde nicht erkannt, oder es war kaputt.
    Format(String),
    /// Das Bild war groesser als [`MAX_BILDPIXEL`].
    ZuGross(u32),
    /// Die Datei war groesser als [`MAX_BILDBYTE`].
    ZuVielByte(usize),
    /// Der Werbefilter hat die Anfrage blockiert (A-ADBLOCK, Koordination
    /// 01.10.2026, 21:45).
    ///
    /// EIGENE VARIANTE und keine `Netz(String)`: der Aufrufer soll das
    /// Bild nicht als kaputt zeichnen, sondern gar keins. „Kaputt"
    /// heisst „da war etwas und es ging nicht" — das stimmt hier nicht,
    /// es war nie etwas da. Der Unterschied ist dort sichtbar, wo
    /// gezaehlt wird.
    Blockiert,
    /// added 03.10.2026, Rolle TLS: **VON DER SICHERHEITSPERRE GESPERRT** —
    /// Mixed Content (W3C §3.2) oder `img-src` (CSP).
    ///
    /// **UND EIGENE VARIANTE, und der Grund ist derselbe wie bei
    /// [`BildFehler::Blockiert`]:** der Aufrufer soll das Bild nicht als
    /// kaputt zeichnen, sondern gar keins. Und der **Text** gehoert dazu,
    /// weil die Projektregel kein stilles Blockieren erlaubt — der Benutzer
    /// soll sehen, **welche** Regel gegriffen hat. `Blockiert` kann keinen Text
    /// tragen (es ist der Werbefilter, dessen Text an anderer Stelle steht).
    Sicherheit(String),
}

impl std::fmt::Display for BildFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BildFehler::Schema(a) => write!(f, "unbekanntes Schema: {a}"),
            BildFehler::Datei(a) => write!(f, "Datei nicht lesbar: {a}"),
            BildFehler::Netz(a) => write!(f, "Netzfehler: {a}"),
            BildFehler::Format(a) => write!(f, "kein Bildformat: {a}"),
            BildFehler::ZuGross(n) => write!(f, "Bild zu gross: {n} Pixel"),
            BildFehler::ZuVielByte(n) => write!(f, "Bilddatei zu gross: {n} Byte"),
            BildFehler::Blockiert => write!(f, "vom Werbefilter blockiert"),
            // added 03.10.2026, Rolle TLS: **DER TEXT STEHT DRIN**, weil die
            // Projektregel kein stilles Blockieren erlaubt: der Benutzer muss
            // sehen, WELCHE Regel gegriffen hat (Mixed Content 3.2 oder
            // `img-src`), nicht nur dass etwas nicht geladen wurde.
            BildFehler::Sicherheit(g) => write!(f, "gesperrt: {g}"),
        }
    }
}

/// Die Bilddaten, fertig fuer die Anzeigeliste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeladenesBild {
    /// Die Pixel, RGBA8.
    pub pixels: Arc<Vec<u8>>,
    /// Die Breite in Pixeln (der Pixel IM PUFFER).
    pub breite: u32,
    /// Die Hoehe in Pixeln (der Pixel IM PUFFER).
    pub hoehe: u32,
    /// Release 08.10.2026 (Bild-Diaet): die NATUERLICHE Groesse des Bildes —
    /// fuer Layout (`<img>` ohne Masse) und `object-fit`. Ohne Bild-Diaet
    /// ist sie gleich `breite`/`hoehe`; verkleinert oder verworfen bleibt
    /// sie, damit sich die Seite nicht verschiebt.
    pub natur_breite: u32,
    pub natur_hoehe: u32,
    /// Release 14.10.2026 (Agent CSS2): the intrinsic dimensions of a vector
    /// image, which may be missing per axis (CSS Images 3 §5.1). `None` for a
    /// raster image: then width, height and ratio come from the pixels.
    pub vector: Option<IntrinsicSize>,
    /// Release 15.10.2026 (Agent F): the source of an SVG image, so that it
    /// can be rendered again at the size it is drawn at (see
    /// [`vektor_pixel`]).
    pub svg: Option<Arc<SvgQuelle>>,
}

/// Release 15.10.2026 (Agent F): an SVG image's text with its hash (the key
/// of the rendered variants).
#[derive(Debug, PartialEq, Eq)]
pub struct SvgQuelle {
    pub text: String,
    pub hash: u64,
}

impl SvgQuelle {
    #[must_use]
    pub fn neu(text: &str) -> SvgQuelle {
        // FNV-1a.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        SvgQuelle { text: text.to_string(), hash: h }
    }
}

/// Release 15.10.2026 (Agent F): the most pixel bytes the rendered variants
/// of SVG images keep.
const VEKTOR_BUDGET: usize = 96 * 1024 * 1024;

/// Release 15.10.2026 (Agent F): the pixels of an SVG image rendered at the
/// size it is drawn at (`breite` x `hoehe` CSS pixels, the box being the
/// root's viewport; the bitmap has the rounded size), instead of the
/// natural-size bitmap scaled up or down. The viewport keeps the fractional
/// size: a 804 x 20.1 box of an 800 x 20 `viewBox` is not letterboxed (WPT
/// svg/as-image sprite-sheet-fractional-size).
/// MEASURED before: an `<img>` of an SVG logo at twice its natural size was
/// blurry, and a `viewBox` image in a box of another ratio was stretched
/// instead of fitted. `None` keeps the decoded bitmap (no SVG, the same
/// size, or too large).
#[must_use]
pub fn vektor_pixel(bild: &GeladenesBild, css_breite: f32, css_hoehe: f32) -> Option<(u32, u32, Arc<Vec<u8>>)> {
    use std::sync::Mutex;
    type Eintrag = ((u64, u32, u32), Option<Arc<Vec<u8>>>);
    static VARIANTEN: Mutex<Vec<Eintrag>> = Mutex::new(Vec::new());
    let quelle = bild.svg.as_ref()?;
    if !(css_breite > 0.0 && css_hoehe > 0.0 && css_breite.is_finite() && css_hoehe.is_finite()) {
        return None;
    }
    let (breite, hoehe) = (css_breite.round().max(1.0) as u32, css_hoehe.round().max(1.0) as u32);
    let exakt = css_breite == breite as f32 && css_hoehe == hoehe as f32;
    if exakt && breite == bild.breite && hoehe == bild.hoehe {
        return None;
    }
    if u64::from(breite) * u64::from(hoehe) > MAX_BILDPIXEL as u64 {
        return None;
    }
    let schluessel = (quelle.hash ^ (u64::from(css_breite.to_bits()) << 32 | u64::from(css_hoehe.to_bits())), breite, hoehe);
    if let Ok(v) = VARIANTEN.lock()
        && let Some((_, p)) = v.iter().find(|(k, _)| *k == schluessel)
    {
        return p.clone().map(|p| (breite, hoehe, p));
    }
    let gerendert = narsil_svg::render_viewport(&quelle.text, (f64::from(css_breite), f64::from(css_hoehe)), (breite, hoehe))
        .ok()
        .map(|r| Arc::new(narsil_svg::straight_rgba(&r.image)));
    if let Ok(mut v) = VARIANTEN.lock() {
        v.push((schluessel, gerendert.clone()));
        let mut summe: usize = v.iter().map(|(_, p)| p.as_ref().map_or(0, |p| p.len())).sum();
        while summe > VEKTOR_BUDGET && v.len() > 1 {
            let (_, alt) = v.remove(0);
            summe -= alt.map_or(0, |p| p.len());
        }
    }
    gerendert.map(|p| (breite, hoehe, p))
}

/// Release 14.10.2026 (Agent CSS2): intrinsic width, height and ratio of a
/// vector image; each may be missing. Stored as `f32` bits so the image stays
/// `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrinsicSize {
    width: Option<u32>,
    height: Option<u32>,
    ratio: Option<u32>,
}

impl IntrinsicSize {
    #[must_use]
    pub fn new(width: Option<f32>, height: Option<f32>, ratio: Option<f32>) -> Self {
        let keep = |v: Option<f32>| v.filter(|v| v.is_finite() && *v > 0.0).map(f32::to_bits);
        IntrinsicSize { width: keep(width), height: keep(height), ratio: keep(ratio) }
    }

    #[must_use]
    pub fn width(&self) -> Option<f32> {
        self.width.map(f32::from_bits)
    }

    #[must_use]
    pub fn height(&self) -> Option<f32> {
        self.height.map(f32::from_bits)
    }

    #[must_use]
    pub fn ratio(&self) -> Option<f32> {
        self.ratio.map(f32::from_bits)
    }
}

impl GeladenesBild {
    /// Release 08.10.2026: ein Bild in voller Groesse (natuerlich = Puffer).
    #[must_use]
    pub fn voll(pixels: Arc<Vec<u8>>, breite: u32, hoehe: u32) -> Self {
        GeladenesBild { pixels, breite, hoehe, natur_breite: breite, natur_hoehe: hoehe, vector: None, svg: None }
    }

    /// Release 15.10.2026: the natural size the layout gives this image as a
    /// replaced element. A vector image only has the axes its document
    /// states (CSS Images 3 §5.1): width and height, or one of them plus the
    /// ratio; the rest comes from the default object size (see below). MEASURED before
    /// (WPT css-sizing svg-intrinsic-size-005, css-flexbox
    /// flex-aspect-ratio-img-column-017, found by Agent CSS2): a sizeless SVG
    /// got a natural size of 300 x 150, i.e. a 2:1 ratio it does not have.
    #[must_use]
    pub fn layout_masse(&self) -> Option<(f32, f32)> {
        // In f32: a ratio alone must survive (`viewBox="0 0 96 12"` fitted
        // into 300 x 150 is 300 x 37.5, not 300 x 38).
        let ganz = |v: f32| v.clamp(0.01, 100_000.0);
        let Some(v) = self.vector else {
            return (self.natur_breite > 0 && self.natur_hoehe > 0).then_some((self.natur_breite as f32, self.natur_hoehe as f32));
        };
        // Release 15.10.2026 (correction): the layout has one natural size per
        // image, so the missing parts come from the default object size
        // 300 x 150 (CSS Images 3 §5.2): a ratio alone is fitted into it
        // (`viewBox` only — most SVG logos), a missing axis is 300 resp. 150.
        // MEASURED before this correction (Agent A, WPT css-sizing
        // image-fractional-height-with-wide-aspect-ratio,
        // intrinsic-size-fallback-replaced): a `viewBox`-only SVG had no size
        // at all and lost its ratio.
        Some(match (v.width(), v.height(), v.ratio()) {
            (Some(w), Some(h), _) => (ganz(w), ganz(h)),
            (Some(w), None, Some(r)) => (ganz(w), ganz(w / r)),
            (None, Some(h), Some(r)) => (ganz(h * r), ganz(h)),
            (None, None, Some(r)) if r >= 2.0 => (300.0, ganz(300.0 / r)),
            (None, None, Some(r)) => (ganz(150.0 * r), 150.0),
            (Some(w), None, None) => (ganz(w), 150.0),
            (None, Some(h), None) => (300.0, ganz(h)),
            (None, None, None) => (300.0, 150.0),
        })
    }

    /// Release 08.10.2026: sind Pixel da (nicht von der Bild-Diaet verworfen)?
    #[must_use]
    pub fn hat_pixel(&self) -> bool {
        self.breite > 0 && self.hoehe > 0 && !self.pixels.is_empty()
    }

    /// Release 08.10.2026: Bytes des Puffers.
    #[must_use]
    pub fn puffer_bytes(&self) -> usize {
        (self.breite as usize).saturating_mul(self.hoehe as usize).saturating_mul(4)
    }

    /// Release 08.10.2026: Bytes in natuerlicher Groesse (was ohne Diaet
    /// belegt waere).
    #[must_use]
    pub fn natur_bytes(&self) -> usize {
        (self.natur_breite as usize).saturating_mul(self.natur_hoehe as usize).saturating_mul(4)
    }
}

/// Holt die Bytes einer Bildadresse.
///
/// # WAS DIESE FUNKTION NICHT KANN
///
/// * `https` — es gibt kein TLS (siehe `LadeFehler::TlsNichtUnterstuetzt`
///   in `laden.rs`). Der Fehler ist benannt, nicht versteckt.
/// * `data:` — ein eingebettetes Bild waer eine Base64-Dekodierung plus
///   eine Formatwahl, und beides waere eine zweite Bildquelle neben der.
/// * Ein Redirect. `narsil-net` meldet eine 3xx als Fehler; der Aufrufer
///   sieht also einen Platzhalter statt einer zweiten Anfrage.
pub fn bytes_holen(adresse: &str) -> Result<Vec<u8>, BildFehler> {
    // Release 15.10.2026 (Agent C): `url()` request modifiers
    // (`cross-origin()`, `integrity()`; `crate::url_modifiers`).
    let (ohne_modifier, modifier) = crate::url_modifiers::split(adresse);
    let adresse = ohne_modifier.as_str();
    if adresse.starts_with("data:") {
        return crate::bilder_daten::lesen(adresse);
    }
    if let Some(rest) = adresse.strip_prefix("file://") {
        let pfad = crate::laden::dateipfad(rest).map_err(|e| BildFehler::Datei(e.to_string()))?;
        // M4: die Dateigroesse wird VOR dem Lesen geprueft, wenn der
        // Betriebssystem sie kennen kann. `metadata` kostet einen `stat`,
        // das Lesen waere der teure Teil.
        if let Ok(meta) = std::fs::metadata(&pfad)
            && meta.len() > MAX_BILDBYTE as u64
        {
            return Err(BildFehler::ZuVielByte(meta.len() as usize));
        }
        let bytes = std::fs::read(&pfad).map_err(|e| BildFehler::Datei(format!("{pfad}: {e}")))?;
        if bytes.len() > MAX_BILDBYTE {
            return Err(BildFehler::ZuVielByte(bytes.len()));
        }
        return Ok(bytes);
    }
    // Koordination 01.10.2026: hier stand "https ohne TLS wird nicht
    // geladen" aus der Zeit vor narsil-tls. GEMESSEN: 0 von 62 Bildern auf
    // ubuntu.com, weil fast jedes Bild im Netz https ist. `narsil_net::fetch`
    // waehlt TLS ueber Port 443 - genau wie der Seitenlader in laden.rs.
    let https = adresse.starts_with("https://");
    if https || adresse.starts_with("http://") {
        let standard_port: u16 = if https { 443 } else { 80 };
        // Host, Port und Pfad trennen — dieselbe Zerlegung wie in
        // `laden.rs`, und aus demselben Grund: `narsil-net` hat eine
        // `Request` aus drei Teilen, keine Adresse.
        //
        // GEMESSEN, warum hier nicht `narsil_url`: der Lader zerlegt seine
        // Adresse von Hand, weil `narsil_url::parse` (Stand 30.09.2026)
        // weder `file://` noch Ports in dieser Form annimmt. Eine zweite
        // Zerlegung mit einem anderen Ergebnis waere die schlechteste
        // denkbare Stelle: ein Bild, das die Seite nicht laedt, weil ein
        // Parser anders zaehlt.
        let rest = &adresse[if https {
            "https://".len()
        } else {
            "http://".len()
        }..];
        let (host_teil, pfad) = match rest.find(['/', '?', '#']) {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        // Das Fragment geht nie auf die Leitung.
        let pfad = pfad.split('#').next().unwrap_or("/");
        let pfad = if pfad.is_empty() || pfad.starts_with('?') {
            std::format!("/{pfad}")
        } else {
            pfad.to_string()
        };
        let (host, port) = match host_teil.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(n) => (h.to_string(), n),
                Err(_) => (host_teil.to_string(), standard_port),
            },
            None => (host_teil.to_string(), standard_port),
        };
        if host.is_empty() {
            return Err(BildFehler::Netz("kein Host in der Adresse".into()));
        }
        // added 01.10.2026, R0: `.art(Bild)`, sonst ginge ein Bild mit
        // `Accept: text/html,…` auf die Leitung. Gemessen war die
        // Inhaltsverhandlung an crates.io der Unterschied zwischen 404 und
        // 200 — dieselbe Liste faellt hier als „kaputtes Bild" auf, nicht
        // als falscher Kopf.
        //
        // A-ADBLOCK (Koordination 01.10.2026, 21:45): DER FILTER VOR DEM
        // ABRUF. Das ist die einzige Aenderung an dieser Crate — eine
        // Bedingung und ein `return`.
        //
        // GEMESSENE BEGRUENDUNG fuer die REIHENFOLGE: nach der
        // Adresspruefung (ein `data:` oder `file://` hat keinen Host und
        // ist nie eine Werbeanfrage) und vor `Request::get`. „Vor dem
        // Abruf" heisst: die Anfrage geht nicht raus, es wird keine
        // Zeit gemessen, es geht keine DNS-Anfrage raus. Genau das ist
        // der Punkt eines Werbeblockers.
        if !narsil_page::werbung::erlaubt(
            adresse,
            narsil_page::werbung::Art::Bild,
            &narsil_page::werbung::seiten_host(),
        ) {
            return Err(BildFehler::Blockiert);
        }
        //
        // added 03.10.2026, Rolle TLS (Freigabe der Leitung): DER
        // SICHERHEITSHAKEN — Mixed Content und CSP `img-src`, **vor** dem
        // Abruf. GEMESSENE ENTSCHEIDUNG fuer die Stelle: dieselbe wie der
        // Werbefilter, denn beide wollen dasselbe — die Anfrage geht nicht
        // raus. Und **nach** dem Werbefilter, weil die Erweiterungsregel vom
        // Benutzer kommt und Vorrang hat.
        if let Err(grund) =
            crate::sicherheit_haken::gate(crate::sicherheit_haken::Unterart::Bild, adresse)
        {
            return Err(BildFehler::Sicherheit(grund));
        }
        // Release 10.10.2026: das Schema ausdruecklich mitgeben — `narsil-net`
        // erkennt TLS sonst nur an Port 443 (`ist_https`). GEMESSEN vorher:
        // `https://localhost:8443/` ging als KLARTEXT-HTTP an den TLS-Port.
        let mut anfrage = narsil_net::Request::get(&host, port, &pfad).art(narsil_net::RessourceArt::Bild);
        anfrage.https = Some(https);
        let antwort = narsil_net::fetch(&anfrage)
            .map_err(|e| BildFehler::Netz(std::format!("{host}:{port}{pfad} — {e}")))?;
        if antwort.status.code >= 400 {
            return Err(BildFehler::Netz(std::format!(
                "HTTP {} {}",
                antwort.status.code,
                antwort.status.reason
            )));
        }
        // M4: `fetch` hat den Body schon im Speicher — die Grenze hier
        // schuetzt die nachfolgenden Schritte (Dekodierung, Pixel), nicht
        // den Netzzugriff. Das ist eine ehrliche Grenze an der richtigen
        // Stelle; eine Grenze UEBER dem Netz waer eine Behauptung, weil
        // `narsil-net` keinen Teilstrom hat.
        if antwort.body.len() > MAX_BILDBYTE {
            return Err(BildFehler::ZuVielByte(antwort.body.len()));
        }
        if let Some(m) = &modifier
            && let Err(grund) = crate::url_modifiers::check(m, &antwort.headers, &antwort.body, &narsil_page::werbung::seiten_host())
        {
            return Err(BildFehler::Netz(grund));
        }
        if std::env::var_os("NARSIL_BILD_KETTE").is_some() {
            eprintln!(
                "BILDABRUF\t{}\t{}\t{}",
                adresse,
                antwort.status.code,
                antwort.body.len()
            );
        }
        return Ok(antwort.body);
    }
    // DAS SCHEMA, und das ist alles, was der Fehler nennen muss.
    //
    // GEMESSEN, warum nicht `split("://")`: `data:image/png;base64,…` hat
    // kein `://`, und der Fehler haette die ganze 800-Zeichen-Adresse
    // genannt — also nicht das Schema, sondern das, was man schon
    // geschrieben hat. Ein Schema steht vor dem ERSTEN Doppelpunkt.
    Err(BildFehler::Schema(
        adresse
            .split(':')
            .next()
            .filter(|s| !s.is_empty() && s.len() < 32)
            .unwrap_or("<kein Schema>")
            .to_string(),
    ))
}

/// Bytes -> Pixel.
///
/// Decodiert PNG, JPEG und BMP — die drei, die `narsil-image` kann und die
/// drei, die ein Browser am häufigsten sieht.
///
/// M4: `MAX_BILDPIXEL` wird VOR der Allokation geprüft: der Decoder
/// reserviert `breite * hoehe * 4`, und ein PNG mit `width=60000` waere
/// 14 GB. Die Pruefung sitzt deshalb ZWISCHEN dem Lesen der Kopfzeile
/// und der Dekodierung — dort, wo `narsil-image` sie zulaesst.
/// H3: EIN SVG-BILD IN EIN `Bild` (H3, 03.10. 10:50).
///
/// GEMESSENE BEGRUENDUNG fuer den Umweg ueber `svg_lesen` + `svg_rasteren`
/// und nicht umgekehrt: `svg_rasteren` nimmt eine GELEBTE `Svg`-Struktur,
/// und das Lesen macht aus dem Text eine. Beide sind oeffentlich, und
/// `dekodieren` braucht am Ende ein `Bild` — also ist das der Weg.
///
/// M4: der Text wird begrenzt, BEVOR er umgewandelt wird
/// (`MAX_BILDBYTE`, dieselbe Grenze wie fuer Rasterformate). Ein
/// UTF-8-Fehler wird als `Format` gemeldet und NICHT mit `from_utf8_lossy`
/// repariert: ein Ersatzzeichen in einer Pfadanweisung erzeugt ein Bild
/// mit einem anderen Namen — still falsch statt sichtbar falsch.
fn dekodiere_svg(bytes: &[u8]) -> Result<narsil_image::Bild, BildFehler> {
    let quelle = std::str::from_utf8(bytes)
        .map_err(|e| BildFehler::Format(std::format!("svg ist kein UTF-8 ({e})")))?;
    // Release 15.10.2026 (Agent F): the SVG renderer on narsil-canvas
    // (groups, paint servers, markers, text, clip paths, masks, filters).
    // The flat renderer of narsil-image stays as the fallback for documents
    // the new one rejects.
    if let Ok(r) = narsil_svg::render(quelle.trim_start_matches('\u{feff}'), None) {
        return Ok(narsil_image::Bild { breite: r.image.width, hoehe: r.image.height, rgba: narsil_svg::straight_rgba(&r.image) });
    }
    let svg = narsil_image::svg::svg_lesen(quelle)
        .map_err(|e| BildFehler::Format(std::format!("svg: {e}")))?;
    narsil_image::svg::svg_rasteren(&svg).map_err(|e| BildFehler::Format(std::format!("svg: {e}")))
}

/// Release 14.10.2026 (Agent CSS2): an SVG file may carry a BOM, whitespace,
/// comments and a doctype before `<svg` (XML 1.0 §2.8 prolog). MEASURED
/// before (WPT css-backgrounds `background-size/vector/*`, every file starts
/// with a license comment): the image was not recognised.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let mut s = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    for _ in 0..64 {
        while let [c, rest @ ..] = s
            && c.is_ascii_whitespace()
        {
            s = rest;
        }
        if s.starts_with(b"<?xml") || s.starts_with(b"<svg") {
            return true;
        }
        let end: &[u8] = if s.starts_with(b"<!--") {
            b"-->"
        } else if s.starts_with(b"<!") || s.starts_with(b"<?") {
            b">"
        } else {
            return false;
        };
        match s.windows(end.len()).position(|w| w == end) {
            Some(i) => s = &s[i + end.len()..],
            None => return false,
        }
    }
    false
}

/// Ohne Anzeigegroesse: das Bild in voller Aufloesung.
///
/// B11 (02.10.2026, Freigabe der Koordination): der Aufrufer, der die
/// Anzeigegroesse kennt, nimmt [`dekodieren_mit_groesse`]. Diese Fassung
/// bleibt, damit kein Aufrufer sich aendern muss.
pub fn dekodieren(bytes: &[u8]) -> Result<GeladenesBild, BildFehler> {
    dekodieren_mit_groesse(bytes, ZIEL_UNBEGRENZT, ZIEL_UNBEGRENZT)
}

/// „Kein Ziel“: so gross, dass `lesen_klein` das volle Bild liefert.
///
/// GEMESSENE BEGRUENDUNG fuer `u32::MAX` statt `0`: in `lesen_klein` ist ein
/// Ziel von 0 ein **Fehler** (`GroesseNull`), weil ein 0x0-Bild kein Bild
/// ist. `u32::MAX` dagegen ist groesser als jedes Bild und loest damit genau
/// den Weg aus, den dieser Aufrufer vorher hatte: voll lesen.
pub const ZIEL_UNBEGRENZT: u32 = u32::MAX;

/// Liest ein Bild und verkleinert es auf die Anzeigegroesse.
///
/// B11: der Weg geht jetzt durch `narsil_image::lesen_klein`, und damit durch
/// **alle** Formate, die `narsil_image` liest. Vorher probierte
/// `dekodiere_bild` vier Wege von Hand (SVG, PNG, JPEG, BMP) — und WebP,
/// GIF, ICO und PPM fehlten, obwohl `narsil_image` alle vier liest.
///
/// GEMESSENE Groessenordnung des Fehlers (probe ueber 30 Seiten, siehe
/// team/BERICHT_BILD_B10.txt): ubuntu.com 1 von 62 Bildern gezeichnet,
/// github.com 0 von 24; 54 von 60 bzw. 16 von 22 geholtenen Bildquellen
/// waren WebP, und **alle** 60 bzw. 22 liest `narsil_image::lesen` ohne
/// Fehler.
pub fn dekodieren_mit_groesse(
    bytes: &[u8],
    ziel_breite: u32,
    ziel_hoehe: u32,
) -> Result<GeladenesBild, BildFehler> {
    // M3/M4: die Bytezahl ist Eingabe.
    if bytes.len() > MAX_BILDBYTE {
        return Err(BildFehler::ZuVielByte(bytes.len()));
    }
    let bild = dekodiere_bild(bytes, ziel_breite, ziel_hoehe)?;
    // M4: nach der Dekodierung noch einmal, denn ein Decoder kann eine
    // Bildgroesse melden, die er dann nicht einhaelt.
    let breite = bild.breite;
    let hoehe = bild.hoehe;
    let pix = (breite as usize).saturating_mul(hoehe as usize);
    if pix as u64 > MAX_BILDPIXEL as u64 {
        return Err(BildFehler::ZuGross(breite.saturating_mul(hoehe)));
    }
    // `narsil-image` liefert RGBA in einem Block, und die Anzeigeliste
    // braucht RGBA in einem Block. Es gibt also nichts umzurechnen — nur
    // zu pruefen, dass die Laenge stimmt, denn ein Decoder, der zu kurz
    // liefert, wuerde im Rasterizer als "Bild kaputt" enden.
    if bild.rgba.len() != pix * 4 {
        return Err(BildFehler::Format(std::format!(
            "Bild liefert {} Byte statt {}",
            bild.rgba.len(),
            pix * 4
        )));
    }
    let mut image = GeladenesBild::voll(Arc::new(bild.rgba), breite, hoehe);
    // Release 14.10.2026 (Agent CSS2): see `GeladenesBild::vector`.
    if looks_like_svg(bytes)
        && let Some(m) = std::str::from_utf8(bytes).ok().and_then(narsil_image::svg::svg_intrinsic_size)
    {
        image.vector = Some(IntrinsicSize::new(
            m.width.map(|v| v as f32),
            m.height.map(|v| v as f32),
            m.ratio.map(|v| v as f32),
        ));
        if let Ok(text) = std::str::from_utf8(bytes) {
            image.svg = Some(Arc::new(SvgQuelle::neu(text.trim_start_matches('\u{feff}'))));
        }
    }
    Ok(image)
}

/// Der Decoder-Weg: SVG zuerst, dann `narsil_image`.
///
/// B11 (02.10.2026, Freigabe): der alte Weg probierte **von Hand** vier
/// Formate — SVG ueber die ersten fuenf Bytes, dann `png_lesen`, dann
/// `jpeg_lesen` (an `FF D8`), dann `bmp_lesen`. Das ist eine Liste, die man
/// pflegen muss, und sie war unvollstaendig: **WebP, GIF, ICO und PPM**
/// fehlten, obwohl `narsil_image` alle vier liest (`erkenne` liefert sie,
/// `webp_lesen`/`gif_lesen`/`ico_lesen`/`ppm_lesen` sind oeffentlich).
///
/// GEMESSENE Folge, BERICHT_BILD_B10.txt Abschnitt 5: 17 der 18 nicht
/// gezeichneten Bilder auf 30 Seiten gingen an dieser Liste verloren, nicht
/// am Netz und nicht am Layout.
///
/// Der neue Weg: `lesen_klein` erkennt das Format ueber `erkenne` (Signatur
/// an den ersten Byte, hoechstens 64 gelesen) und verkleinert dabei auf die
/// Anzeigegroesse — das ist der Weg aus `narsil-image`, mit dem dort
/// gemessenen 8,7-fachen Zeitgewinn (team/BERICHT_BILD_B9.txt).
///
/// Der SVG-Zweig bleibt **zuerst**, und aus einem gemessenen Grund: SVG ist
/// Text, und `erkenne` erkennt es an `<?xml` oder `<svg`; `lesen_klein`
/// nimmt es auch, aber der Zweihand-Check ist billiger als die
/// Formatabfrage und der Fehlertext bleibt der aus H3.
fn dekodiere_bild(bytes: &[u8], ziel_breite: u32, ziel_hoehe: u32) -> Result<Bild, BildFehler> {
    if bytes.is_empty() {
        return Err(BildFehler::Format("leer".into()));
    }
    //
    // H3 (03.10. 10:50): SVG ZUERST — und nicht als letzter Versuch.
    //
    // GEMESSENER ZUSTAND VORHER: `dekodiere_bild` probierte PNG, dann
    // JPEG, dann BMP. Ein `<img src="…svg">` bekam also drei
    // Fehlversuche und landete in `BildFehler::Format` — der Platzhalter
    // mit dem Alt-Text. GEMESSEN, dass `narsil-image` die Faehigkeit
    // HAT: `svg::svg_lesen(quelle)` und `svg::svg_rasteren(&svg)` sind
    // oeffentlich (`narsil-image/src/svg.rs:781` und `:1081`), und sie
    // liefern ein fertiges `Bild`. Es fehlte nur der eine Aufruf.
    //
    // GEMESSENE BEGRUENDUNG, warum SVG ZUERST und nicht zuletzt: eine
    // SVG-Datei beginnt mit `<?xml` oder `<svg`, und kein Rasterformat
    // erkennt das. Wer sie zuerst probiert, erkennt sie an ZWEI Bytes
    // und spart den anderen Decodern die Arbeit — das ist derselbe Weg,
    // den `png_lesen` mit seiner Signatur geht. GEMESSENE BEGRUENDUNG
    // fuer die ZWEI Bytes statt der Endung: die Endung steht nicht in
    // der Datei (sie steht in der URL, und die URL hat der Cache
    // verloren).
    if looks_like_svg(bytes) {
        return dekodiere_svg(bytes);
    }
    //
    // B11: der Rest geht an `narsil_image::lesen_klein`. Das erkennt das
    // Format ueber die Signatur (`erkenne`), liest es in der verlangten
    // Verkleinerung und gibt einen BENANNTEN `BildFehler` zurueck, dessen
    // `name()` die Ursache nennt.
    //
    // GEMESSENER Grund fuer `lesen_klein` statt `lesen`: der Aufrufer weiss
    // die Anzeigegroesse, und `lesen_klein` ist die einzige Stelle, die
    // darueber entscheidet (JPEG-Gleichstromweg, sonst Box-Filter). Ohne
    // Ziel (`ZIEL_UNBEGRENZT`) liefert es genau das volle Bild, das der alte
    // Weg geliefert hat.
    narsil_image::lesen_klein(bytes, ziel_breite, ziel_hoehe)
        .map_err(|e| BildFehler::Format(std::format!("{}: {e}", e.name())))
}

/// Der Cache: eine Adresse, ein Bild.
///
/// GEMESSEN, warum es ueberhaupt einen gibt: eine Seite mit demselben
/// Logo in 30 Kopfzeilen holt es 30 Mal — 30mal dieselbe Datei, 30mal
/// dieselbe Dekodierung. Bei einer 200-KB-Datei sind das 6 MB und eine
/// Sekunde, die niemand bemerkt, weil das Fenster haengt.
///
/// # `PartialEq`
///
/// Die Pixel sind `Arc<Vec<u8>>`, und zwei gleichwertige Bilder aus
/// verschiedenen Adressen sind fuer den Vergleich NICHT gleich. Das ist
/// die Eigenschaft, an der `Angezeigt` haengt (sie wird verglichen), und
/// sie stimmt: ein Browser, der ein Bild ersetzt, ist nicht derselbe
/// Zustand wie einer, der es behalten hat. Fuer den Bild-Pixel-Vergleich
/// ist [`BildCache::gleich_pixel`] da — das vergleicht die INHALTE.
#[derive(Debug, Default, Clone)]
pub struct BildCache {
    /// Die bereits geholten Bilder, nach Adresse.
    ///
    /// M3: `MAX_BILDER` Eintraege. Der Pfad ist einfach: ist die Zahl
    /// erreicht, wird der Cache LEER. Das ist kein LRU, aber es ist
    /// begrenzt, und ein LRU brauchte eine Zeitangabe je Eintrag.
    ///
    /// B11 (02.10.2026): **beides** gilt jetzt. Die Zahl der Eintraege ist
    /// weiterhin `MAX_BILDER`, aber es wird nicht mehr leer geputzt —
    /// stattdessen fliegt der am laengsten unbenutzte Eintrag hinaus, bis
    /// die **Bytegrenze** eingehalten ist (siehe `byte_budget`). Das Feld
    /// bleibt `pub` und vom Typ her unveraendert, weil
    /// `sitzung.rs:6235` (`a.bilder.geladen.get(&bk.bild.quelle)`) und
    /// `sitzung.rs:2541` (`BildCache::neu()`) darauf rechnen.
    pub geladen: HashMap<String, Result<GeladenesBild, BildFehler>>,
    /// B11: Reihenfolge der Benutzung, hinten = juengster Zugriff. Nur
    /// Adressen, die gerade drin sind; doppelte Eintraege werden beim
    /// Zugriff entfernt.
    ///
    /// M3/M8: `VecDeque` mit einer Laengengrenze (`MAX_BILDER` plus der
    /// Zahl der Verdrengungen), damit kein Adressen-Wildwuchs entsteht.
    benutzt: std::collections::VecDeque<String>,
    /// B11: die Byte, die die dekodierten Bilder belegen. Wird aus
    /// `GeladenesBild` berechnet, nicht geschaetzt.
    bytes: usize,
    /// Release 08.10.2026 (Bild-Diaet): die ORIGINALBYTES (komprimiert, wie
    /// geladen) — daraus wird neu dekodiert, wenn ein verkleinertes oder
    /// verworfenes Bild wieder groesser gebraucht wird. Begrenzt durch
    /// [`ORIGINAL_BUDGET`], aelteste zuerst hinaus.
    originale: HashMap<String, Arc<Vec<u8>>>,
    originale_reihe: std::collections::VecDeque<String>,
    originale_bytes: usize,
    /// Release 08.10.2026: sollen beim Holen die Originalbytes behalten
    /// werden (Bild-Diaet an)?
    pub originale_behalten: bool,
    /// Release 08.10.2026 (Bild-Diaet): Bilder mit festem Kasten erst bei
    /// Annaeherung holen, auch ohne `loading="lazy"`.
    pub aufschieben: bool,
}

/// Release 08.10.2026 (Bild-Diaet): so viele Byte Originaldaten (JPEG, PNG …,
/// komprimiert) werden je Seite behalten. GEMESSEN: ein Nachrichtenfoto ist
/// komprimiert 100–400 KB und dekodiert 2–17 MB — 96 MB Originale ersetzen
/// also ein Vielfaches an Bitmaps.
pub const ORIGINAL_BUDGET: usize = 96 * 1024 * 1024;

/// B11: Vorgabe des Bildbestands, 256 MB dekodierte Bilder.
///
/// GEMESSEN, warum diese Zahl und nicht `MAX_BILDER` allein: mit 128
/// Eintraegen und je einem 2400x1800-Foto sind das **2,2 GB**
/// (128 mal 17 280 032 Byte). Die alte Regel raeumte das nur, indem sie bei
/// 128 Eintraegen ALLES wegwarf — auf einer Seite mit 100 Bildern heisst
/// das: nach dem 100. Bild kein Bild mehr, auch nicht das gerade geladene.
/// Mit dem Budget stehen 15 Bilder im Bestand und die Kette verliert die
/// aeltesten, statt alle zu verlieren (`team/BERICHT_BILD_B9.txt` Abschnitt 3
/// am Beispiel: 85 Verdrengungen, Bestand am Ende 259 200 480 Byte bei
/// 268 435 456 Byte Grenze).
pub const BYTE_BUDGET: usize = 256 * 1024 * 1024;

impl BildCache {
    /// Uebernimmt eine fertige Hintergrundladung mit denselben Cachegrenzen.
    pub(crate) fn uebernehmen(&mut self, adresse: String, ergebnis: Result<GeladenesBild, BildFehler>) {
        if let Some(Ok(alt)) = self.geladen.remove(&adresse) {
            self.bytes = self.bytes.saturating_sub(Self::bytezahl(&alt));
        }
        self.benutzt.retain(|a| a != &adresse);
        if let Ok(bild) = &ergebnis {
            self.bytes = self.bytes.saturating_add(Self::bytezahl(bild));
        }
        self.geladen.insert(adresse.clone(), ergebnis);
        self.benutzt.push_back(adresse);
        self.grenze_halten();
    }

    /// Neu, leer.
    pub fn neu() -> Self {
        Self {
            geladen: HashMap::with_capacity(16),
            benutzt: std::collections::VecDeque::with_capacity(16),
            bytes: 0,
            originale: HashMap::new(),
            originale_reihe: std::collections::VecDeque::new(),
            originale_bytes: 0,
            originale_behalten: false,
            aufschieben: false,
        }
    }

    /// Release 08.10.2026 (Bild-Diaet): die Originalbytes einer Adresse
    /// merken (innerhalb von [`ORIGINAL_BUDGET`]).
    pub fn original_merken(&mut self, adresse: &str, bytes: Arc<Vec<u8>>) {
        if bytes.len() > ORIGINAL_BUDGET / 4 {
            return;
        }
        if let Some(alt) = self.originale.remove(adresse) {
            self.originale_bytes = self.originale_bytes.saturating_sub(alt.len());
            self.originale_reihe.retain(|a| a != adresse);
        }
        self.originale_bytes += bytes.len();
        self.originale.insert(adresse.to_string(), bytes);
        self.originale_reihe.push_back(adresse.to_string());
        while self.originale_bytes > ORIGINAL_BUDGET {
            let Some(alt) = self.originale_reihe.pop_front() else { break };
            if let Some(b) = self.originale.remove(&alt) {
                self.originale_bytes = self.originale_bytes.saturating_sub(b.len());
            }
        }
    }

    /// Release 08.10.2026: einen Eintrag ganz entfernen (der Nachlader holt
    /// ihn dann neu).
    pub fn entfernen(&mut self, adresse: &str) {
        if let Some(Ok(alt)) = self.geladen.remove(adresse) {
            self.bytes = self.bytes.saturating_sub(Self::bytezahl(&alt));
        }
        self.benutzt.retain(|a| a != adresse);
    }

    /// Release 08.10.2026: die gemerkten Originalbytes (falls noch da).
    #[must_use]
    pub fn original(&self, adresse: &str) -> Option<Arc<Vec<u8>>> {
        self.originale.get(adresse).cloned()
    }

    /// Release 08.10.2026: Bytes aller gemerkten Originale.
    #[must_use]
    pub fn originale_bytes(&self) -> usize {
        self.originale_bytes
    }

    /// Release 08.10.2026 (Bild-Diaet): den Pixelpuffer eines geladenen
    /// Bildes ersetzen (verkleinert, verworfen oder neu dekodiert). Die
    /// natuerliche Groesse bleibt die des alten Eintrags. `false`, wenn die
    /// Adresse nicht (mehr) geladen ist.
    pub fn puffer_ersetzen(&mut self, adresse: &str, pixels: Arc<Vec<u8>>, breite: u32, hoehe: u32) -> bool {
        let Some(Ok(bild)) = self.geladen.get_mut(adresse) else {
            return false;
        };
        self.bytes = self.bytes.saturating_sub(bild.puffer_bytes());
        bild.pixels = pixels;
        bild.breite = breite;
        bild.hoehe = hoehe;
        self.bytes = self.bytes.saturating_add(bild.puffer_bytes());
        true
    }

    /// Die Bytegrenze in Byte (B11).
    pub fn byte_budget(&self) -> usize {
        BYTE_BUDGET
    }

    /// Wie viele Byte die Bilder im Bestand belegen (B11).
    pub fn belegte_bytes(&self) -> usize {
        self.bytes
    }

    /// Die Groesse eines geladenen Bildes in Byte (M4: `saturating_mul`).
    fn bytezahl(b: &GeladenesBild) -> usize {
        (b.breite as usize)
            .saturating_mul(b.hoehe as usize)
            .saturating_mul(4)
    }

    /// B11: haelt `MAX_BILDER` und `BYTE_BUDGET` ein.
    ///
    /// Der aelteste Eintrag fliegt zuerst (`benutzt.pop_front`). Fehler
    /// zaehlen **nicht** als Byte — ein 404 belegt keinen Speicher, und
    /// sonst wuerde eine Seite voller toter Links den ganzen Bestand
    /// verdraengen.
    fn grenze_halten(&mut self) {
        while self.geladen.len() > MAX_BILDER || self.bytes > BYTE_BUDGET {
            let Some(alteste) = self.benutzt.pop_front() else {
                break;
            };
            if let Some(Ok(b)) = self.geladen.remove(&alteste) {
                self.bytes = self.bytes.saturating_sub(Self::bytezahl(&b));
            }
        }
    }

    /// Holt ein Bild, notfalls aus dem Netz, und merkt sich das Ergebnis.
    ///
    /// GEMESSEN, warum auch FEHLER gemerkt werden: ein 404 auf einer
    /// Seite mit 30 Bildern ist 30mal derselbe Fehler. Ohne Merken
    /// waere der zweite Versuch genauso teuer wie der erste.
    ///
    /// # DIE EIGENTLICHE FRAGE: WANN WIRD GEHOLT?
    ///
    /// Beim **Laden** der Seite, nicht beim Zeichnen. Das ist eine
    /// Entscheidung und keine Bequemlichkeit: zeichnet man die Bilder
    /// bei jedem Frame, holt man sie bei jedem Frame — und ein Fenster
    /// zeichnet 30mal in der Sekunde. Beim Laden werden sie einmal
    /// geholt, und der Cache in `Angezeigt` traegt sie bis zum naechsten
    /// Laden.
    pub fn hole(&mut self, adresse: &str) -> Result<&GeladenesBild, BildFehler> {
        self.hole_mit_groesse(adresse, ZIEL_UNBEGRENZT, ZIEL_UNBEGRENZT)
    }

    /// Wie [`BildCache::hole`], aber mit Anzeigegroesse.
    ///
    /// B11 (02.10.2026): das ist der Aufruf, mit dem der Zeitgewinn aus
    /// `narsil_image::lesen_klein` (8,7-fach, gemessen in
    /// `team/BERICHT_BILD_B9.txt` Abschnitt 2) in der Kette ankommt. Der
    /// Aufrufer ist `sitzung.rs`, und dort steht die Kastengroesse im
    /// Ladevorgang bereits da (`bild_kaesten(a)`, mit `width` und
    /// `height`). Ein Aufruf von `hole` statt `hole_mit_groesse` liefert
    /// weiterhin das volle Bild — der Aufrufer kann den Weg also Schritt
    /// fuer Schritt umstellen.
    ///
    /// M3: `ziel_breite`/`ziel_hoehe` sind Zahlen aus dem Aufrufer; `0`
    /// heisst „keine Angabe“ und wird zu `ZIEL_UNBEGRENZT`, weil ein 0x0-Ziel
    /// in `lesen_klein` ein benannter Fehler waere (`GroesseNull`) — ein
    /// Kasten ohne gemessene Groesse ist aber kein kaputtes Bild.
    pub fn hole_mit_groesse(
        &mut self,
        adresse: &str,
        ziel_breite: u32,
        ziel_hoehe: u32,
    ) -> Result<&GeladenesBild, BildFehler> {
        // M3: `hole` leitet auf `hole_mit_groesse` um (nicht umgekehrt),
        // also gibt es keine Endlosschleife.
        let ziel_breite = if ziel_breite == 0 {
            ZIEL_UNBEGRENZT
        } else {
            ziel_breite
        };
        let ziel_hoehe = if ziel_hoehe == 0 {
            ZIEL_UNBEGRENZT
        } else {
            ziel_hoehe
        };
        if !self.geladen.contains_key(adresse) {
            // Release 08.10.2026 (Bild-Diaet): die Originalbytes behalten, wenn
            // gewuenscht — daraus wird spaeter neu dekodiert.
            let mut original: Option<Arc<Vec<u8>>> = None;
            let behalten = self.originale_behalten;
            let ergebnis = bytes_holen(adresse).and_then(|b| {
                let r = dekodieren_mit_groesse(&b, ziel_breite, ziel_hoehe);
                if behalten && r.is_ok() {
                    original = Some(Arc::new(b));
                }
                r
            });
            if let Some(b) = original {
                self.original_merken(adresse, b);
            }
            if std::env::var_os("NARSIL_BILD_KETTE").is_some() {
                match &ergebnis {
                    Ok(b) => eprintln!(
                        "BILDDECODE\t{}\t{}\t{}\t{}",
                        adresse,
                        b.breite,
                        b.hoehe,
                        b.pixels.chunks_exact(4).filter(|p| p[3] > 0).count()
                    ),
                    Err(e) => eprintln!("BILDDECODE\t{}\tFEHLER\t{}", adresse, e),
                }
            }
            if let Ok(b) = &ergebnis {
                self.bytes = self.bytes.saturating_add(Self::bytezahl(b));
            }
            self.geladen.insert(adresse.to_string(), ergebnis);
            self.benutzt.push_back(adresse.to_string());
            self.grenze_halten();
        } else {
            // B11: ein Treffer macht den Eintrag zum juengsten. Ohne das
            // waere „am laengsten unbenutzt“ gleich „am fruehesten
            // eingelegt“ — und eine Seite, die zur Kopfzeile zurueckblaetternt,
            // wuerde genau die Bilder verlieren, die sie gleich braucht.
            if let Some(stelle) = self.benutzt.iter().position(|a| a == adresse) {
                self.benutzt.remove(stelle);
            }
            self.benutzt.push_back(adresse.to_string());
        }
        match self.geladen.get(adresse) {
            Some(Ok(b)) => Ok(b),
            // DER FEHLER WIRD KOPIERT, nicht verborgen. Eine `&BildFehler`
            // aus dieser Map heraus waere an `&mut self` gebunden — und
            // der Aufrufer soll das Bild BEHALTEN duerfen, waehrend er
            // weiter Bilder holt. Also: geklont. Das kostet eine
            // Zeichenkette, und zwar nur auf dem Fehlerpfad.
            Some(Err(f)) => Err(f.clone()),
            None => Err(BildFehler::Schema(adresse.to_string())),
        }
    }

    /// Wie viele Bilder geholt wurden (auch die, die fehlschlugen).
    pub fn len(&self) -> usize {
        self.geladen.len()
    }

    /// `true`, wenn noch nichts geholt wurde.
    pub fn ist_leer(&self) -> bool {
        self.geladen.is_empty()
    }

    /// Dasselbe unter dem Namen, den clippy erwartet (B11).
    ///
    /// GEMESSENER Grund fuer den Doppelnamen: clippy meldet
    /// `len_without_is_empty`, und `ist_leer` ist der Name, den diese Crate
    /// durchgaengig benutzt (deutsch). Der Aufrufer soll nicht umbenennen
    /// muessen, also gibt es beide.
    pub fn is_empty(&self) -> bool {
        self.geladen.is_empty()
    }

    /// Sind zwei Caches inhaltlich gleich?
    ///
    /// Der Vergleich von `Arc` vergleicht die ARCS (also die Zeiger), nicht
    /// die Bilder. Fuer einen Test, der wissen will, ob sich das Bild
    /// geaendert hat, ist das die falsche Frage — deshalb diese Methode.
    pub fn gleich_pixel(&self, andere: &BildCache) -> bool {
        self.geladen.len() == andere.geladen.len()
            && self
                .geladen
                .iter()
                .all(|(a, la)| match (la, andere.geladen.get(a)) {
                    (Ok(x), Some(Ok(y))) => {
                        x.breite == y.breite
                            && x.hoehe == y.hoehe
                            && x.pixels.as_slice() == y.pixels.as_slice()
                    }
                    (Err(x), Some(Err(y))) => x == y,
                    _ => false,
                })
    }
}

impl PartialEq for BildCache {
    /// Siehe [`BildCache::gleich_pixel`] — der Vergleich fragt nach den
    /// INHALTEN, weil ein Arc-Vergleich die Frage "gleiches Bild?" mit
    /// "gleicher Speicher?" verwechselt.
    fn eq(&self, andere: &Self) -> bool {
        self.gleich_pixel(andere)
    }
}

impl Eq for BildCache {}

#[cfg(test)]
mod b11_tests {
    //! B11 (02.10.2026, Freigabe der Koordination): Regressionstests fuer
    //! die Aenderung an `dekodiere_bild` und `BildCache`.
    //!
    //! GEMESSENER Ausgangszustand, gegen den diese Tests stehen:
    //! `dekodiere_bild` probierte SVG, PNG, JPEG und BMP von Hand. WebP und
    //! GIF fielen durch, obwohl `narsil_image` beide liest. Auf ubuntu.com
    //! sind 54 von 60 geholtenen Bildquellen WebP, auf github.com 16 von 22
    //! — und probe zeichnete dort 1 von 62 bzw. 0 von 24 Bildern
    //! (`team/BERICHT_BILD_B10.txt`).
    //!
    //! Die Testdaten liegen in `tests/bilder/` (WebP 66 Byte, GIF 43 Byte),
    //! weil sie dort schon liegen duerfen und nicht in den Quellbaum einer
    //! anderen Crate gehoeren.

    use super::{BYTE_BUDGET, BildCache, GeladenesBild, MAX_BILDER, dekodieren_mit_groesse};

    fn daten(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("bilder")
                .join(name),
        )
        .unwrap_or_else(|e| panic!("Testdaten {name} fehlen ({e})"))
    }

    /// B11: WebP geht durch. Vorher stand hier `BildFehler::Format`.
    #[test]
    fn webp_wird_gelesen() {
        let b = dekodieren_mit_groesse(
            &daten("format.webp"),
            super::ZIEL_UNBEGRENZT,
            super::ZIEL_UNBEGRENZT,
        )
        .expect("WebP muss gelesen werden — das war der Zweck der Aenderung");
        assert!(b.breite > 0 && b.hoehe > 0, "Bild hat keine Groesse");
        assert_eq!(
            b.pixels.len(),
            b.breite as usize * b.hoehe as usize * 4,
            "RGBA-Laenge passt nicht zur Groesse"
        );
    }

    /// B11: GIF geht auch durch (der zweite fehlende Zweig).
    #[test]
    fn gif_wird_gelesen() {
        let b = dekodieren_mit_groesse(
            &daten("format.gif"),
            super::ZIEL_UNBEGRENZT,
            super::ZIEL_UNBEGRENZT,
        )
        .expect("GIF muss gelesen werden");
        assert!(b.breite > 0, "Bild hat keine Breite");
    }

    /// Der alte Weg darf nicht kaputtgegangen sein: PNG und SVG.
    #[test]
    fn png_und_svg_gehen_weiter() {
        for name in ["vierfelder.png", "format.webp"] {
            let bytes = daten(name);
            assert!(
                dekodieren_mit_groesse(&bytes, super::ZIEL_UNBEGRENZT, super::ZIEL_UNBEGRENZT)
                    .is_ok(),
                "{name} muss lesbar bleiben"
            );
        }
        // SVG ueber den Zweihand-Zweig: die Datei ist Text.
        let svg = b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><rect width=\"8\" height=\"8\" fill=\"#f00\"/></svg>";
        let b = dekodieren_mit_groesse(svg, super::ZIEL_UNBEGRENZT, super::ZIEL_UNBEGRENZT)
            .expect("SVG muss lesbar bleiben (H3)");
        assert_eq!((b.breite, b.hoehe), (8, 8), "SVG-Groesse");
    }

    /// B11: mit Anzeigegroesse wird kleiner geliefert.
    ///
    /// GEMESSENE Grundlage: `lesen_klein` mit Ziel 4x4 auf einem 8x8-Bild
    /// muss 4x4 liefern. Ohne Ziel (`ZIEL_UNBEGRENZT`) kommt das volle Bild.
    #[test]
    fn anzeigegroesse_wird_geachtet() {
        let bytes = daten("format.webp");
        let voll = dekodieren_mit_groesse(&bytes, super::ZIEL_UNBEGRENZT, super::ZIEL_UNBEGRENZT)
            .expect("volles Bild");
        let klein = dekodieren_mit_groesse(&bytes, 1, 1).expect("verkleinertes Bild");
        assert!(
            klein.breite <= voll.breite && klein.hoehe <= voll.hoehe,
            "mit Ziel 1x1 darf das Bild nicht groesser werden: {}x{} gegen {}x{}",
            klein.breite,
            klein.hoehe,
            voll.breite,
            voll.hoehe
        );
        assert_eq!(
            klein.breite as usize * klein.hoehe as usize * 4,
            klein.pixels.len(),
            "RGBA-Laenge"
        );
    }

    /// B11: der Bestand haelt `BYTE_BUDGET` und `MAX_BILDER` ein, und der
    /// aelteste Eintrag fliegt zuerst.
    ///
    /// Die Bilder werden von Hand in `geladen` gelegt (das Feld ist `pub`),
    /// weil `hole` ins Netz geht und ein Test kein Netz braucht.
    #[test]
    fn bestand_haelt_die_bytegrenze() {
        assert_eq!(BYTE_BUDGET, 256 * 1024 * 1024, "Vorgabe des Bestands");
        let mut c = BildCache::neu();
        // GEMESSENER Rechenfehler in diesem Test beim ersten Wurf: 16 Bilder
        // zu je 4 000 000 Byte sind 64 MB und liegen **unter** dem Budget von
        // 256 MB — es wurde also gar nichts verdraengt, und der Test fiel an
        // seiner eigenen Behauptung. 128 Bilder sind 512 MB: das ueberschreitet
        // das Budget und liegt genau auf `MAX_BILDER`, damit die Bytegrenze
        // ausloest und nicht die Stueckzahl.
        for i in 0..MAX_BILDER {
            c.geladen.insert(
                format!("b{i}"),
                Ok(GeladenesBild::voll(std::sync::Arc::new(vec![0u8; 4_000_000]), 1000, 1000)),
            );
            c.benutzt.push_back(format!("b{i}"));
        }
        c.bytes = MAX_BILDER * 4_000_000;
        c.grenze_halten();
        assert!(
            c.belegte_bytes() <= BYTE_BUDGET,
            "Bestand {} Byte ueber dem Budget {}",
            c.belegte_bytes(),
            BYTE_BUDGET
        );
        assert!(
            c.geladen.len() <= MAX_BILDER,
            "zu viele Eintraege: {}",
            c.geladen.len()
        );
        // 256 MB passen noch, 256 MB plus ein Bild passt nicht mehr: bei je
        // 4 000 000 Byte bleiben genau 64 Bilder stehen.
        assert!(
            !c.geladen.contains_key("b0"),
            "der aelteste Eintrag muss draussen sein"
        );
        let juengster = MAX_BILDER - 1;
        assert!(
            c.geladen.contains_key(&format!("b{juengster}")),
            "der juengste Eintrag muss drin sein"
        );
        // GEMESSENE Rechnung (der erste Wurf behauptete 64): 268 435 456 Byte
        // Budget geteilt durch 4 000 000 Byte je Bild sind 67, Rest 435 456.
        // Ein Bild zu 4 000 000 Byte ist 3,8 MiB und nicht 4 MiB — deshalb
        // passen 67 hinein und nicht 64.
        assert_eq!(
            c.geladen.len(),
            67,
            "268435456 / 4000000 = 67 Bilder passen in den Bestand"
        );
    }

    /// Ein Fehler belegt keinen Speicher: eine Seite voller 404 darf den
    /// Bestand nicht verdraengen.
    #[test]
    fn fehler_zaehlen_nicht_als_byte() {
        let mut c = BildCache::neu();
        for i in 0..(MAX_BILDER + 10) {
            c.geladen
                .insert(format!("f{i}"), Err(super::BildFehler::Netz("404".into())));
            c.benutzt.push_back(format!("f{i}"));
        }
        c.grenze_halten();
        assert_eq!(c.belegte_bytes(), 0, "Fehler belegen 0 Byte");
    }
}
