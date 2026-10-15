//! Release 09.10.2026: das LEBENDE DOM fuer Seitenskripte.
//!
//! # WARUM
//!
//! Bisher sahen Skripte ein Proxy-DOM aus flachen Tabellen ([`crate::dom`]),
//! und jede Aenderung wurde ein Befehl, der als `Patch` auf den
//! urspruenglichen `HtmlNode`-Baum gespielt wurde. GEMESSEN (09.10.):
//! Textknoten waren fuer JS unsichtbar, `nextSibling`, `cloneNode`,
//! `DocumentFragment`, Kommentare, `template.content`, `attachShadow`
//! fehlten, `innerHTML =` wurde NICHT angewendet. Web-Apps (YouTube mit
//! Polymer, React, Vue) bauen ihre Seite aber genau so.
//!
//! # WIE
//!
//! Neben dem Proxy-Stand haelt jede Seite den Arena-Baum
//! ([`narsil_fmt_html::dom::Dokument`], Operationen in
//! [`narsil_fmt_html::lebend`]). Jeder JS-Knoten (`{__host: id}`) zeigt auf
//! einen Arena-Knoten. Struktur, Text, `innerHTML` und Attribute gehen in
//! die Arena; aendert ein Lauf etwas, wird der Seitenbaum aus der Arena neu
//! erzeugt ([`Lebend::html_neu`]) — derselbe Weg wie beim ersten Laden.
//!
//! Klassen und Inline-Stil fuehrt der Proxy-Stand weiter
//! (`Stand::klassen`/`Stand::stil`); sie werden vor dem Neubau als
//! `class`/`style` in die Arena geschrieben ([`Lebend::stand_uebernehmen`]).
//!
//! # GRENZEN (M3/M4)
//!
//! Arena-Knoten: [`narsil_fmt_html::dom::MAX_KNOTEN`]. Host-Ids fuer neue
//! Knoten: [`ERSTE_NEUE_HOST`] bis [`LETZTE_NEUE_HOST`]. Treffer einer
//! Selektorsuche: [`MAX_TREFFER`]. HTML-Text fuer `innerHTML`:
//! [`MAX_HTML`]. Custom-Element-Reaktionen je Abholung: [`MAX_CE_SCHLANGE`].
//!
//! # CUSTOM ELEMENTS
//!
//! Die Registry selbst lebt im Skript (`vorspann.js`). Das Skript meldet
//! die definierten Namen und ihre beobachteten Attribute
//! (`document.__ceDefinieren`); das lebende DOM merkt sich bei jeder
//! Aenderung, welche definierten Elemente verbunden, getrennt, geklont oder
//! umattribuiert wurden ([`CeReaktion`]). Der Motor holt die Liste nach
//! jedem DOM-Aufruf ab und ruft die Callbacks (HTML §4.13.6 CEReactions).

use narsil_fmt_html::HtmlNode;
use narsil_fmt_html::dom::{Art, Dokument, Namensraum};
use narsil_jsvm::Wert;
use std::collections::HashMap;

/// Erste Host-Id fuer Knoten, die erst das lebende DOM kennt (oberhalb von
/// Dokument und Fenster, siehe `dom::MAX_HOST_IDS`).
pub const ERSTE_NEUE_HOST: u32 = 1 << 20;
/// Letzte vergebbare Host-Id.
pub const LETZTE_NEUE_HOST: u32 = (1 << 30) - 1;
/// Release 2026-10-15: the first host id of the live collections
/// (HTMLCollection, NodeList; DOM §4.2.10) — above every node id.
pub const FIRST_COLLECTION_HOST: u32 = 1 << 30;
/// At most this many live collections per arena; beyond it the old
/// snapshot arrays are handed out.
pub const MAX_COLLECTIONS: usize = 65_536;
/// The cached result lists are dropped (and rebuilt lazily) once they hold
/// this many entries in total.
const MAX_CACHED_ITEMS: usize = 4_000_000;
/// Hoechstens so viele Treffer liefert `querySelectorAll`.
pub const MAX_TREFFER: usize = 100_000;
/// Hoechstens so viel HTML-Text nimmt `innerHTML =` an.
pub const MAX_HTML: usize = 16 * 1024 * 1024;
/// Hoechstens so viele Custom-Element-Reaktionen warten auf die Abholung.
pub const MAX_CE_SCHLANGE: usize = 100_000;
/// Hoechstens so viele definierte Custom-Element-Namen.
pub const MAX_CE_NAMEN: usize = 10_000;

/// Release 2026-10-15: what a call changes for the image model, taken
/// before the call (see `Lebend::bild_vorher`).
enum BildVorher {
    Nichts,
    /// An attribute (lowercased name) and its old value.
    Attribut(String, Option<String>),
    /// The images a node removal mutates.
    Bilder(Vec<usize>),
}

/// Eine Custom-Element-Reaktion (HTML §4.13.6), die der Motor abholt.
#[derive(Debug, Clone, PartialEq)]
pub enum CeReaktion {
    /// Ein definiertes Element wurde mit dem Dokument verbunden.
    Verbunden(u32),
    /// Ein definiertes Element wurde vom Dokument getrennt.
    Getrennt(u32),
    /// Ein definiertes Element entstand (Klon/Import) und wartet auf das Upgrade.
    Upgrade(u32),
    /// Ein beobachtetes Attribut hat sich geaendert.
    Attribut { host: u32, name: String, alt: Option<String>, neu: Option<String>, raum: Option<String> },
    /// Release 10.10.2026: `el.click()` — die Seite loest `click` aus und
    /// fuehrt danach das Standardverhalten aus (siehe `vorspann.js`).
    Klick(u32),
    /// Formularzuordnung oder effektiver Disabled-Zustand kann sich aendern.
    Formular(u32),
    /// ownerDocument wechselt bei expliziter oder impliziter Adoption.
    Adoptiert { host: u32, alt: u32, neu: u32 },
    /// Release 11.10.2026: ein eingefuegtes Inline-Skript (klassisch, ohne
    /// `src`) laeuft SOFORT beim Einfuegen (HTML §4.12.1.1 „prepare the
    /// script element", Schritt „immediately execute"); der Vorspann fuehrt
    /// es aus (`__narsilSkript`).
    Skript(u32),
    /// Release 11.10.2026: ein Medienelement muss laden (HTML §4.8.11.5):
    /// `art` 1 = `src` gesetzt, 2 = `<source>`/`<track>` eingefuegt (der
    /// Host ist das Kind), 3 = `audio`/`video` eingefuegt. Das Medienmodell
    /// im Vorspann antwortet (`__narsilMedien`).
    Medien { host: u32, art: u8 },
    /// Release 13.10.2026: der erste `slotchange`-Lauscher — der Vorspann
    /// beobachtet erst ab dann die Slots (DOM §4.2.2.5).
    SlotLauscher(u32),
}

/// Release 2026-10-15: what a live collection holds (DOM §4.2.10, §4.4
/// "list of elements with qualified name" …, HTML §3.1.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CollectionKind {
    Children,
    ChildNodes,
    /// The qualified name, and whether the root's document was an HTML
    /// document when the list was made (it keeps that, DOM §4.4).
    TagName(String, bool),
    TagNameNs(Option<String>, String),
    ClassNames(String),
    /// `document.scripts`, `images`, `forms`, `links`, `anchors`, `embeds`.
    Document(&'static str),
    /// `document.getElementsByName` (a NodeList).
    Name(String),
    /// `form.elements` (HTMLFormControlsCollection, HTML §4.10.3).
    FormElements,
    /// The RadioNodeList of `form.elements` for a name.
    FormNamed(String),
    /// `fieldset.elements`: the listed descendants.
    FieldsetElements,
    /// `table.rows` (HTML §4.9.1: thead rows, then table/tbody rows, then
    /// tfoot rows), `table.tBodies`, `thead/tbody/tfoot.rows`, `tr.cells`.
    TableRows,
    TableBodies,
    SectionRows,
    Cells,
    /// `map.areas`, `datalist.options`.
    MapAreas,
    DatalistOptions,
    /// Release 2026-10-15: the document's named elements for a name (HTML
    /// §3.1.3 "dom-document-nameditem" with more than one element).
    DocumentNamed(String),
}

impl CollectionKind {
    /// Whether attribute changes can change the members (see
    /// `Dokument::attr_version`).
    fn reads_attributes(&self) -> bool {
        matches!(
            self,
            CollectionKind::ClassNames(_)
                | CollectionKind::Document("links" | "anchors")
                | CollectionKind::Name(_)
                | CollectionKind::FormElements
                | CollectionKind::FormNamed(_)
                | CollectionKind::DocumentNamed(_)
        )
    }

    /// The host kind of its objects: 5 HTMLCollection, 6 NodeList, 7
    /// HTMLFormControlsCollection, 8 RadioNodeList.
    fn art(&self) -> u8 {
        match self {
            CollectionKind::ChildNodes | CollectionKind::Name(_) => 6,
            CollectionKind::FormElements => 7,
            CollectionKind::FormNamed(_) => 8,
            _ => 5,
        }
    }
}

/// Release 2026-10-15: one live collection: its root, what it holds, and the
/// members as of the document versions it was last evaluated at.
#[derive(Debug, Clone)]
pub(crate) struct LiveCollection {
    root: usize,
    kind: CollectionKind,
    evaluated: Option<(u64, u64)>,
    items: Vec<usize>,
}

/// Das lebende DOM einer Seite.
#[derive(Debug, Clone)]
pub struct Lebend {
    pub dok: Dokument,
    pub wurzel: usize,
    host_zu_knoten: HashMap<u32, usize>,
    knoten_zu_host: HashMap<usize, u32>,
    naechste_host: u32,
    /// Hat ein Lauf die Arena veraendert (Struktur, Text, Attribute)?
    pub geaendert: bool,
    document_host: u32,
    /// Definierte Custom-Element-Namen -> beobachtete Attribute.
    pub ce_namen: HashMap<String, Vec<String>>,
    /// Name der Definition -> lokaler Tag (auch customized built-ins).
    ce_lokal: HashMap<String, Vec<String>>,
    /// Unveraenderlicher is-Wert; das spaetere is-Attribut aendert ihn nicht.
    ce_is: HashMap<usize, Option<String>>,
    /// Fremde Attribut-Namensraeume, die das Parser-Modell nicht benennt.
    attribut_ns: HashMap<(usize, String), String>,
    fremde_attribute: HashMap<usize, Vec<(String, String, String)>>,
    eigner: HashMap<usize, usize>,
    ce_states: HashMap<usize, Vec<String>>,
    /// Wartende Reaktionen (siehe [`CeReaktion`]).
    pub ce_schlange: Vec<CeReaktion>,
    /// Release 10.10.2026: per JS eingefuegte `<script>`, die noch laufen
    /// muessen (HTML §8.4.3; der Host holt sie ab, `js::dyn_skripte_pumpen`).
    pub dyn_skripte: Vec<usize>,
    /// Release 10.10.2026: „already started" (HTML §4.12.1.1) — ein Skript
    /// laeuft hoechstens einmal, auch wenn es spaeter verschoben wird.
    skripte_gestartet: std::collections::HashSet<usize>,
    /// Release 10.10.2026: `<img>`-Knoten, deren `src` ein Skript gesetzt hat
    /// — der Host meldet daraus die Bilder OHNE Platz im Dokument
    /// (`new Image()`, Vorlader; siehe `Sitzung::bild_ereignisse`).
    pub bild_quellen: Vec<usize>,
    /// Release 2026-10-15: `<img>` nodes with a relevant mutation (HTML
    /// §4.8.4.3.3): the image data is updated again, so `load`/`error` fire
    /// again even for an unchanged source (see `Sitzung::bild_ereignisse`).
    pub bild_erneut: Vec<usize>,
    /// Release 12.10.2026: der Deklarationsblock je Element mit dem Text, den
    /// er zuletzt ins `style`-Attribut schrieb (CSSOM §6.7: der Block ist die
    /// Wahrheit, das Attribut seine Serialisierung). Solange das Attribut
    /// dieser Text ist, gilt der Block — mit der Reihenfolge seiner
    /// Langformen, die ein neues Lesen der Kurzform-Schreibung verloere.
    stil_bloecke: HashMap<usize, (String, narsil_css::cssom::block::Block)>,
    /// Release 12.10.2026: Merkmale einer Shadow Root aus `attachShadow`
    /// (Bit 1 `clonable`, 2 `serializable`, 4 `delegatesFocus`).
    schatten_merkmale: HashMap<usize, u8>,
    /// Release 13.10.2026: die Einfuegestelle von `document.write` waehrend
    /// eines Parser-Skripts (die Eltern des Skripts; der Rest dahinter ist
    /// ausgeblendet, siehe `schwanz_ausblenden`).
    pub schreib_eltern: Option<usize>,
    /// Release 13.10.2026: `<style>` elements whose sheet a script disabled
    /// (`sheet.disabled = true`, CSSOM §6.1); the page tree switches them off.
    pub disabled_sheets: std::collections::HashSet<usize>,
    /// Release 13.10.2026: the CSSOM text of a `<style>` sheet that scripts
    /// changed (`insertRule` …), with the element state it belongs to
    /// (`sheet_key`). The
    /// element's own text stays as written (CSSOM: rule changes do not touch
    /// `textContent`); when that text changes, the entry is stale.
    pub sheet_texts: HashMap<usize, (String, String)>,
    /// Release 14.10.2026: `<link>` elements a script enabled explicitly
    /// (`link.disabled = false` while the attribute was present; HTML §4.6.7).
    pub enabled_links: std::collections::HashSet<usize>,
    /// Release 15.10.2026: the animated values per element (CSS Animations,
    /// CSS Transitions, Web Animations; set by `animationen.js` through
    /// `__narsilAnimSetzen`). They form their own layer above the normal
    /// declarations: the cascade reads them after the `style` attribute,
    /// `getComputedStyle` before it, the page tree appends them to `style`.
    pub animated: HashMap<usize, Vec<(String, String)>>,
    /// Release 15.10.2026: the animated values of pseudo-elements, by element
    /// and pseudo-element name (`before`, `after`, `marker` …).
    pub animated_pseudo: HashMap<(usize, String), Vec<(String, String)>>,
    /// Release 15.10.2026: elements whose animated values changed only in
    /// paint-only properties since the last build (`anim_schnell`): they are
    /// painted into the shown page without a rebuild.
    pub anim_dirty: std::collections::HashSet<usize>,
    /// Release 14.10.2026: the `<base>` elements of the arena (how many arena
    /// nodes were scanned, the HTML `<base>` nodes among them). The arena only
    /// grows, so every new node is looked at once; the document base URL of
    /// a page without `<base>` then costs nothing per URL read.
    base_elements: std::cell::RefCell<(usize, Vec<usize>)>,
    /// Release 14.10.2026: roots of the subtrees `schwanz_einblenden` showed
    /// again after a parser script; their defined custom elements count as
    /// parsed now and are upgraded before the next script (see
    /// [`Lebend::upgrade_revealed`]).
    revealed_tail: Vec<usize>,
    /// Release 2026-10-15: the live collections by host id, the id of each
    /// (root, kind) — the same arguments give the same object — and how
    /// many members their caches hold.
    collections: HashMap<u32, LiveCollection>,
    collection_ids: HashMap<(usize, CollectionKind), u32>,
    cached_items: usize,
}

impl Lebend {
    /// Baut die Arena aus dem Seitenbaum. `ids` ist die Zuordnung
    /// Seiten-`ord` -> Host-Id des Proxy-Stands (fuer alle Elemente).
    pub fn neu(html: &[HtmlNode], ids: &HashMap<usize, u32>, document_host: u32) -> Self {
        let (mut dok, wurzel, indizes) = narsil_fmt_html::lebend::aus_htmlnode(html);
        // Release 12.10.2026: die Spuren von `crate::scope_implizit` gehoeren nur
        // dem Seitenbaum.
        crate::scope_implizit::aus_dom_entfernen(&mut dok);
        // Release 2026-10-15: the attributes in source order (the page tree
        // keeps them in a map; see `narsil_fmt_html::lebend::AttributeOrder`).
        if let Some(mut order) = narsil_fmt_html::lebend::take_page_attribute_order() {
            for knoten in &mut dok.knoten {
                if let Art::Element { name, attribute, .. } = &mut knoten.art
                    && attribute.len() >= 2
                {
                    order.apply(name, attribute);
                }
            }
        }
        let mut l = Lebend {
            dok,
            wurzel,
            host_zu_knoten: HashMap::new(),
            knoten_zu_host: HashMap::new(),
            naechste_host: ERSTE_NEUE_HOST,
            geaendert: false,
            document_host,
            ce_namen: HashMap::new(),
            ce_lokal: HashMap::new(),
            ce_is: HashMap::new(),
            attribut_ns: HashMap::new(),
            fremde_attribute: HashMap::new(),
            eigner: HashMap::new(),
            ce_states: HashMap::new(),
            ce_schlange: Vec::new(),
            dyn_skripte: Vec::new(),
            skripte_gestartet: std::collections::HashSet::new(),
            bild_quellen: Vec::new(),
            bild_erneut: Vec::new(),
            stil_bloecke: HashMap::new(),
            schatten_merkmale: HashMap::new(),
            schreib_eltern: None,
            disabled_sheets: std::collections::HashSet::new(),
            sheet_texts: HashMap::new(),
            enabled_links: std::collections::HashSet::new(),
            animated: HashMap::new(),
            animated_pseudo: HashMap::new(),
            anim_dirty: std::collections::HashSet::new(),
            base_elements: std::cell::RefCell::new((0, Vec::new())),
            revealed_tail: Vec::new(),
            collections: HashMap::new(),
            collection_ids: HashMap::new(),
            cached_items: 0,
        };
        // Die Skripte des Dokuments sind schon gelaufen (der Ladeweg).
        // Release 14.10.2026: only those the parser really prepared (HTML
        // §4.12.1.1 „prepare the script element": a script without source
        // and without text, or with an unsupported type, returns BEFORE
        // „already started" is set). MEASURED before (WPT execution-timing
        // 056, 127, 144, 146-href): text or `src` added later, or a fixed
        // `type` plus re-insertion, never ran the script.
        for k in 0..l.dok.knoten.len() {
            if l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("script"))
                && (l.script_source(k).is_some() || !l.child_text(k).is_empty())
                && l.script_type_supported(k)
                // Template contents are never prepared by the parser (not
                // connected); a clone inserted into the document runs.
                && !matches!(l.dok.knoten.get(l.dok.wurzel_von(k)).map(|n| &n.art), Some(Art::Fragment))
            {
                l.skripte_gestartet.insert(k);
            }
        }
        for k in 0..l.dok.knoten.len() {
            l.eigner.insert(k, wurzel);
            if l.dok.ist_element_knoten(k) {
                l.ce_is.insert(k, l.dok.attribut(k, "is").map(str::to_string));
            }
        }
        l.schatten_merkmale_lesen(0);
        l.verbinden(document_host, wurzel);
        ord_gehen(html, &indizes, &mut |o, k| {
            if let Some(h) = ids.get(&o) {
                l.verbinden(*h, k);
            }
        });
        l
    }

    fn verbinden(&mut self, host: u32, knoten: usize) {
        self.host_zu_knoten.insert(host, knoten);
        self.knoten_zu_host.insert(knoten, host);
    }

    /// Der Arena-Knoten zu einer Host-Id.
    #[must_use]
    pub fn knoten(&self, host: u32) -> Option<usize> {
        self.host_zu_knoten.get(&host).copied()
    }

    /// Release 09.10.2026: die Host-Ids der Vorfahren von `host` (naechster
    /// zuerst), nur bestehende — fuer die geschaetzte Geometrie eines noch
    /// nicht gelayouteten Elements (`__narsilMass`).
    pub fn vorfahren_hosts(&self, host: u32) -> Vec<u32> {
        let mut raus = Vec::new();
        let Some(mut k) = self.knoten(host) else { return raus };
        for _ in 0..256 {
            let Some(e) = self.dok.eltern(k) else { break };
            if let Some(h) = self.knoten_zu_host.get(&e) {
                raus.push(*h);
            }
            k = e;
        }
        raus
    }

    /// Die Host-Id eines Arena-Knotens (legt eine an, wenn noetig).
    pub fn host(&mut self, knoten: usize) -> Option<u32> {
        if let Some(h) = self.knoten_zu_host.get(&knoten) {
            return Some(*h);
        }
        if self.naechste_host >= LETZTE_NEUE_HOST || !self.dok.gibt_es(knoten) {
            return None;
        }
        let h = self.naechste_host;
        self.naechste_host += 1;
        self.verbinden(h, knoten);
        Some(h)
    }

    /// Der JS-Wert eines Arena-Knotens (`null`, wenn es ihn nicht gibt).
    pub fn wert(&mut self, knoten: Option<usize>) -> Wert {
        match knoten.and_then(|k| self.host(k)) {
            Some(h) => crate::dom::host_wert(h),
            None => Wert::Null,
        }
    }

    /// Erzeugt den Seitenbaum neu und liefert die neue Zuordnung
    /// `ord -> Host-Id` (jedes Element bekommt eine Host-Id).
    pub fn html_neu(&mut self) -> (Vec<HtmlNode>, HashMap<usize, u32>) {
        let w = self.wurzel;
        self.html_neu_von(w)
    }

    /// Release 11.10.2026: wie [`Self::html_neu`], fuer ein beliebiges
    /// Dokument der Arena (die Dokumente der Iframes).
    pub fn html_neu_von(&mut self, wurzel: usize) -> (Vec<HtmlNode>, HashMap<usize, u32>) {
        let (mut html, mut indizes) = narsil_fmt_html::lebend::in_htmlnode_mit_indizes(&self.dok, wurzel);
        // Release 12.10.2026: implizites `@scope` (siehe `crate::scope_implizit`).
        crate::scope_implizit::umschreiben(&mut html);
        // Das zuerst angelegte `<style title>` im Dokument (Knotennummern
        // wachsen mit dem Anlegen).
        let bevorzugt = (0..self.dok.knoten.len()).find_map(|k| {
            (self.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style")) && self.ist_verbunden(k))
                .then(|| self.dok.attribut(k, "title").filter(|t| !t.is_empty()).map(str::to_string))
                .flatten()
        });
        crate::scope_implizit::titel_auswahl(&mut html, bevorzugt.as_deref());
        if crate::scope_implizit::has_disabled_link(&html, 0) {
            let enabled = |pos: usize| indizes.get(pos).is_some_and(|k| self.enabled_links.contains(k));
            crate::scope_implizit::switch_off_disabled_links(&mut html, &enabled);
        }
        self.sheet_texts.retain(|&k, (key, _)| k < self.dok.knoten.len() && sheet_key(&self.dok, k) == *key);
        if !self.disabled_sheets.is_empty() || !self.sheet_texts.is_empty() || !self.animated.is_empty() {
            apply_sheet_state(&mut html, &mut indizes, &self.dok, &self.disabled_sheets, &self.sheet_texts, &self.animated);
        }
        if !self.animated_pseudo.is_empty() {
            apply_pseudo_animations(&mut html, &mut indizes, &self.animated_pseudo);
        }
        let mut ids: HashMap<usize, u32> = HashMap::new();
        let mut paare: Vec<(usize, usize)> = Vec::new();
        ord_gehen(&html, &indizes, &mut |o, k| paare.push((o, k)));
        for (o, k) in paare {
            if self.dok.ist_element_knoten(k)
                && let Some(h) = self.host(k)
            {
                ids.insert(o, h);
            }
        }
        (html, ids)
    }

    /// Schreibt Klassen und Inline-Stil des Proxy-Stands in die Arena
    /// (vor [`Self::html_neu`]).
    pub fn stand_uebernehmen(
        &mut self,
        klassen: &HashMap<u32, Vec<String>>,
        stil: &HashMap<u32, Vec<(String, String)>>,
    ) {
        for (host, liste) in klassen {
            if let Some(k) = self.knoten(*host) {
                let wert = liste.join(" ");
                if self.dok.attribut(k, "class") != Some(wert.as_str()) {
                    self.dok.setze_attribut(k, Namensraum::Html, "class", &wert);
                    self.geaendert = true;
                }
            }
        }
        for (host, paare) in stil {
            if let Some(k) = self.knoten(*host) {
                // Release 10.10.2026: derselbe Zerleger wie `el.style` (siehe
                // `stil_zerlegen`); `cssText` ersetzt den ganzen Block.
                // GEMESSEN vorher (youtube.com): `style.cssText = …` wurde
                // die Angabe `css-text: …`, und jede Zuweisung haengte den
                // ganzen Block noch einmal an (116 KB je Element nach 1500
                // Bildern, 200 MB JS-Heap).
                // Release 12.10.2026: der CSSOM-Block (siehe `stil_block`).
                let alt = stil_block(self, k);
                let mut block = alt.clone();
                for (n, v) in paare {
                    if n == "cssText" {
                        block = narsil_css::cssom::block::Block::lesen(v);
                    } else {
                        block.setzen(&stil_name(n), v, "");
                    }
                }
                if block != alt {
                    stil_block_schreiben(self, k, block);
                }
            }
        }
    }

    /// Gibt es ueberhaupt definierte Custom Elements? (Der schnelle Weg
    /// fuer alle Seiten ohne.)
    #[must_use]
    pub fn ce_aktiv(&self) -> bool {
        !self.ce_namen.is_empty()
    }

    fn ce_schieben(&mut self, r: CeReaktion) {
        if self.ce_schlange.len() < MAX_CE_SCHLANGE {
            self.ce_schlange.push(r);
        }
    }

    /// Definierte Elemente im Teilbaum von `k` in Baumordnung, Shadow Roots
    /// eingeschlossen (HTML: "shadow-including"), Template-Inhalte nicht.
    fn ce_teilbaum(&self, k: usize, raus: &mut Vec<usize>) {
        let mut stapel = vec![k];
        while let Some(x) = stapel.pop() {
            if raus.len() >= MAX_CE_SCHLANGE || !self.dok.gibt_es(x) {
                return;
            }
            if self.ce_name(x).is_some() {
                raus.push(x);
            }
            for &c in self.dok.kinder(x).iter().rev() {
                stapel.push(c);
            }
            if let Some(sr) = self.dok.shadow_wurzel_intern(x) {
                stapel.push(sr);
            }
        }
    }

    fn ce_name(&self, k: usize) -> Option<&str> {
        if !matches!(self.dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. })) { return None; }
        let tag = self.dok.element_name(k)?;
        if self.ce_lokal.get(tag).is_some_and(|lokal| lokal.iter().any(|s| s == tag)) { return Some(tag); }
        let is = match self.ce_is.get(&k) {
            Some(v) => v.as_deref(),
            None => self.dok.attribut(k, "is"),
        }?;
        self.ce_lokal.get(is).filter(|lokal| lokal.iter().any(|s| s == tag)).map(|_| is)
    }

    /// Die verbundenen definierten Elemente in den Teilbaeumen von `knoten`.
    fn ce_verbundene(&self, knoten: &[usize]) -> Vec<usize> {
        let mut raus = Vec::new();
        for &k in knoten {
            // Release 12.10.2026: „connected" heisst in IRGENDEINEM Dokument
            // (DOM §4.2), auch einem erzeugten. GEMESSEN vorher (WPT
            // custom-elements/reactions/Node, ChildNode, ParentNode, Range):
            // beim Umzug in `createHTMLDocument()` fehlte `connected`.
            if self.dok.gibt_es(k) && self.in_einem_dokument(k) {
                self.ce_teilbaum(k, &mut raus);
            }
        }
        raus
    }

    /// Nach einer Baumaenderung: `vorher` waren verbunden (sie wurden
    /// entfernt oder verschoben), die in `knoten` sind es jetzt.
    fn ce_nach(&mut self, vorher: Vec<usize>, knoten: &[usize]) {
        let nachher = self.ce_verbundene(knoten);
        for k in vorher {
            if let Some(h) = self.host(k) {
                self.ce_schieben(CeReaktion::Getrennt(h));
            }
        }
        for &k in knoten {
            let doc = self.dokument_von(k);
            self.dokument_setzen(k, doc, true);
        }
        for k in nachher {
            if let Some(h) = self.host(k) {
                self.ce_schieben(CeReaktion::Verbunden(h));
            }
        }
        for &k in knoten { self.ce_formular(k); }
        for &k in knoten { self.medien_eingefuegt(k); }
    }

    /// Release 12.10.2026: DOM §4.5 „clone a node", Schritt 7 — ein Wirt,
    /// dessen Shadow Root `clonable` ist, bekommt im Klon eine eigene Shadow
    /// Root mit denselben Merkmalen und tief geklonten Kindern (auch bei
    /// `cloneNode(false)`). GEMESSEN vorher (WPT
    /// Range-cloneContents-around-shadow.tentative): der Klon hatte keine.
    /// Release 12.10.2026: die Merkmale deklarativer Shadow Roots (aus dem
    /// Parser, am Wirt) in `schatten_merkmale` uebernehmen — fuer alle
    /// Knoten ab `ab` (neue Knoten stehen am Ende der Arena).
    fn schatten_merkmale_lesen(&mut self, ab: usize) {
        for k in ab..self.dok.knoten.len() {
            if matches!(self.dok.knoten[k].art, Art::ShadowRoot { .. })
                && let Some(w) = self.dok.knoten[k].eltern
            {
                let m = self.dok.schatten_merkmale(w);
                if m != 0 {
                    self.schatten_merkmale.insert(k, m);
                }
            }
        }
    }

    fn schatten_mitklonen(&mut self, alt: usize, neu: usize) {
        if self.schatten_merkmale.is_empty() {
            return;
        }
        let mut paare = vec![(alt, neu)];
        let mut schritte = 0usize;
        while let Some((a, n)) = paare.pop() {
            schritte += 1;
            if schritte > narsil_fmt_html::dom::MAX_KNOTEN {
                break;
            }
            if let Some(sr) = self.dok.shadow_wurzel_intern(a) {
                let bits = self.schatten_merkmale.get(&sr).copied().unwrap_or(0);
                if bits & 1 != 0 && self.dok.shadow_wurzel_intern(n).is_none() {
                    let offen = matches!(self.dok.knoten[sr].art, Art::ShadowRoot { mode: narsil_fmt_html::dom::ShadowRootMode::Open, .. });
                    if let Some(neue) = self.dok.shadow_anhaengen(n, offen) {
                        self.schatten_merkmale.insert(neue, bits);
                        self.dok.schatten_merkmale_setzen(n, bits);
                        for c in self.dok.kinder(sr).to_vec() {
                            if let Some(k) = self.dok.klonen(c, true) {
                                self.dok.anhaengen(neue, k);
                                paare.push((c, k));
                            }
                        }
                    }
                }
            }
            paare.extend(self.dok.kinder(a).iter().copied().zip(self.dok.kinder(n).iter().copied()));
        }
    }

    /// Release 14.10.2026: the first `<base>` element with an `href` that is
    /// connected to the document node `doc` (not in a shadow tree, a template
    /// or a detached subtree), in tree order (HTML §2.4.1).
    pub(crate) fn first_base_with_href(&self, doc: usize) -> Option<usize> {
        let mut cache = self.base_elements.borrow_mut();
        let (scanned, found) = &mut *cache;
        let len = self.dok.knoten.len();
        for k in *scanned..len {
            if matches!(&self.dok.knoten[k].art, Art::Element { raum: Namensraum::Html, name, .. } if name == "base") {
                found.push(k);
            }
        }
        *scanned = len;
        let mut best: Option<(usize, Vec<usize>)> = None;
        for &k in found.iter() {
            if self.dok.attribut(k, "href").is_none() || self.dok.wurzel_von(k) != doc {
                continue;
            }
            let key = self.tree_position(k);
            if best.as_ref().is_none_or(|(_, b)| key < *b) {
                best = Some((k, key));
            }
        }
        best.map(|(k, _)| k)
    }

    /// The child indices from the root down to `k` (compares tree order).
    fn tree_position(&self, k: usize) -> Vec<usize> {
        let mut path = Vec::new();
        let mut x = k;
        while let Some(p) = self.dok.eltern(x) {
            path.push(self.dok.kinder(p).iter().position(|&c| c == x).unwrap_or(0));
            x = p;
            if path.len() > 4096 {
                break;
            }
        }
        path.reverse();
        path
    }

    pub(crate) fn dokument_von(&self, k: usize) -> usize {
        let mut x = k;
        for _ in 0..512 {
            let w = self.dok.wurzel_von(x);
            match self.dok.knoten.get(w).map(|n| &n.art) {
                Some(Art::Dokument) => return w,
                Some(Art::ShadowRoot { .. }) => {
                    if let Some(host) = self.dok.eltern(w) { x = host; continue; }
                }
                _ => {},
            }
            break;
        }
        self.eigner.get(&k).copied().unwrap_or(self.wurzel)
    }

    fn dokument_setzen(&mut self, root: usize, doc: usize, reagieren: bool) {
        let mut s = vec![root];
        while let Some(k) = s.pop() {
            let alt = self.eigner.insert(k, doc).unwrap_or(self.wurzel);
            if reagieren && alt != doc && self.ce_name(k).is_some() {
                if let (Some(host), Some(alt), Some(neu)) = (self.host(k), self.host(alt), self.host(doc)) {
                    self.ce_schieben(CeReaktion::Adoptiert { host, alt, neu });
                }
            }
            s.extend_from_slice(self.dok.kinder(k));
            if let Some(sr) = self.dok.shadow_wurzel_intern(k) { s.push(sr); }
            if let Some(tc) = self.dok.template_inhalt(k) { s.push(tc); }
        }
    }

    fn ce_formular(&mut self, k: usize) {
        let mut alle = Vec::new(); self.ce_teilbaum(k, &mut alle);
        for k in alle {
            if let Some(h) = self.host(k) { self.ce_schieben(CeReaktion::Formular(h)); }
        }
    }

    /// Ein Klon/Import: definierte Elemente darin warten auf das Upgrade.
    fn ce_klon(&mut self, k: usize) {
        let mut liste = Vec::new();
        self.ce_teilbaum(k, &mut liste);
        for e in liste {
            if let Some(h) = self.host(e) {
                self.ce_schieben(CeReaktion::Upgrade(h));
            }
        }
    }

    fn attr_ns(&self, k: usize, q: &str) -> Option<String> {
        if let Some(ns) = self.attribut_ns.get(&(k, q.to_string())) { return Some(ns.clone()); }
        match self.dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::Element { attribute, .. }) => attribute.iter().find(|a| attr_qname(a) == q).and_then(|a| match a.raum {
                Namensraum::Xlink => Some(NS_XLINK.to_string()),
                Namensraum::Xml => Some(NS_XML.to_string()),
                Namensraum::Xmlns => Some(NS_XMLNS.to_string()),
                _ => None,
            }),
            _ => None,
        }
    }

    fn attribute_von(&self, k: usize) -> Vec<(String, String, Option<String>)> {
        let mut out: Vec<_> = self.dok.attribute_von(k).into_iter()
            .map(|(q, v)| { let ns = self.attr_ns(k, &q); (q, v, ns) }).collect();
        if let Some(attrs) = self.fremde_attribute.get(&k) {
            out.extend(attrs.iter().map(|(q, ns, v)| (q.clone(), v.clone(), Some(ns.clone()))));
        }
        out
    }

    fn attr_ns_finden(&self, k: usize, ns: Option<&str>, lokal: &str) -> Option<(String, String)> {
        self.attribute_von(k).into_iter().find(|(q, _, uri)| {
            uri.as_deref() == ns && if ns.is_some() { q.rsplit(':').next() == Some(lokal) } else { q == lokal }
        }).map(|(q, v, _)| (q, v))
    }

    /// Release 11.10.2026: `src` eines `audio`/`video` geaendert — der
    /// Ladeablauf beginnt (HTML §4.8.11.5). GEMESSEN vorher: nur das
    /// Attribut stand, kein `loadstart` folgte.
    fn medien_attribut(&mut self, k: usize, name: &str) {
        // Release 12.10.2026: jede Aenderung von `src`/`srcset` an einem
        // `<img>` stoesst die Bildwahl an (auch ueber `setAttribute` aus dem
        // Vorspann). GEMESSEN vorher: `img.srcset = …` an einem losen Bild
        // lud nie.
        if matches!(name, "src" | "srcset") && self.dok.element_name(k) == Some("img") {
            self.bild_quelle_vermerken(k);
        }
        // Release 2026-10-15: a changed `name` of an open `<details>`.
        if name == "name" && self.html_name(k) == Some("details") {
            self.details_exklusiv(&[k]);
            return;
        }
        // Release 11.10.2026: `open` an `<details>` (Art 7, `toggle`).
        if name == "open" {
            if self.html_name(k) == Some("details")
                && let Some(h) = self.host(k)
            {
                self.ce_schieben(CeReaktion::Medien { host: h, art: 7 });
            }
            return;
        }
        // Release 12.10.2026: `href`/`rel` eines `<link>` (Art 9, siehe
        // `medien_eingefuegt`).
        if matches!(name, "href" | "rel") {
            if self.html_name(k) == Some("link")
                && let Some(h) = self.host(k)
            {
                self.ce_schieben(CeReaktion::Medien { host: h, art: 9 });
            }
            return;
        }
        if name != "src" {
            return;
        }
        // Release 11.10.2026: ein verbundenes, noch nicht gestartetes
        // `<script>` bekommt `src` — jetzt wird es vorbereitet (§4.12.1.1).
        if self.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("script")) {
            if self.dok.attribut(k, "src").is_some() && !self.skripte_gestartet.contains(&k) {
                self.skripte_vormerken(&[k]);
            }
            return;
        }
        let art = match self.html_name(k) {
            Some("audio" | "video") => 1,
            Some("track") => 4,
            // Release 11.10.2026: `<img src>` (Art 8) — der Vorspann meldet
            // ein leeres oder ungueltiges `src` mit `error` (HTML §4.8.4.3.5).
            Some("img") => 8,
            _ => return,
        };
        if let Some(h) = self.host(k) {
            self.ce_schieben(CeReaktion::Medien { host: h, art });
        }
    }

    /// Release 11.10.2026: der lokale Name eines Elements NUR im
    /// HTML-Namensraum. GEMESSEN vorher (WPT resource-selection-*-in-
    /// namespace): ein `<source>` aus `createElementNS('bogus', …)` startete
    /// die Ressourcenauswahl.
    fn html_name(&self, k: usize) -> Option<&str> {
        let n = self.dok.element_name(k)?;
        self.dok.ist_element(k, Namensraum::Html, n).then_some(n)
    }

    /// Release 14.10.2026: the children changed steps of a script parent
    /// run before the post-connection steps of the inserted nodes (HTML
    /// §4.12.1.1, DOM „insert" step 7 vs. 10). MEASURED before (WPT
    /// execution-timing 128): an inserted inner script ran before the outer
    /// one that received the text.
    fn script_children_changed(&mut self, k: usize, name: &str) {
        if matches!(name, "appendChild" | "insertBefore" | "append" | "prepend" | "replaceChild" | "replaceChildren")
            && self.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("script"))
            && !self.skripte_gestartet.contains(&k)
        {
            self.skripte_vormerken(&[k]);
        }
    }

    /// Release 14.10.2026: the external source of a script element: `src`,
    /// and for an SVG `<script>` `href` (SVG 2) or `xlink:href`.
    pub(crate) fn script_source(&self, k: usize) -> Option<String> {
        // An SVG script ignores `src` (WPT execution-timing 146).
        if !self.dok.ist_element(k, Namensraum::Svg, "script") {
            return self.dok.attribut(k, "src").map(str::to_string);
        }
        self.attribute_von(k)
            .into_iter()
            .find(|(q, _, _)| q == "href" || q == "xlink:href")
            .map(|(_, v, _)| v)
    }

    /// Release 14.10.2026: the child text content of a script (only its
    /// own Text children, DOM „child text content"; HTML §4.12.1.1).
    pub(crate) fn child_text(&self, k: usize) -> String {
        let mut out = String::new();
        for &c in self.dok.kinder(k) {
            if let Some(Art::Text(t)) = self.dok.knoten.get(c).map(|n| &n.art) {
                out.push_str(t);
            }
        }
        out
    }

    /// Release 14.10.2026: does the script's `type` name a script the
    /// engine prepares (classic JavaScript MIME types, `module`,
    /// `importmap`; HTML §4.12.1.1 step „determine the type")?
    pub(crate) fn script_type_supported(&self, k: usize) -> bool {
        let t = self.dok.attribut(k, "type").unwrap_or("");
        crate::js::klassischer_skript_typ(t) || matches!(t.trim().to_ascii_lowercase().as_str(), "module" | "importmap")
    }

    /// Release 11.10.2026: ein Kind eines Medienelements ging hinaus (der
    /// Vorspann gleicht die Spurliste ab).
    fn medien_kind_weg(&mut self, eltern: usize) {
        if matches!(self.dok.element_name(eltern), Some("audio" | "video"))
            && let Some(h) = self.host(eltern)
        {
            self.ce_schieben(CeReaktion::Medien { host: h, art: 5 });
        }
    }

    /// Release 11.10.2026: der schnelle Weg ohne Custom Elements (`rufen`)
    /// meldet Medien ebenso — `setAttribute('src', …)` und das Einfuegen.
    fn medien_nach_ruf(&mut self, k: usize, name: &str, args: &[Wert]) {
        if matches!(name, "appendChild" | "insertBefore" | "append" | "prepend" | "replaceChild")
            && self.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("script"))
            && !self.skripte_gestartet.contains(&k)
        {
            // §4.12.1.1: „children changed steps" eines verbundenen Skripts.
            self.skripte_vormerken(&[k]);
            return;
        }
        match name {
            "setAttribute" | "toggleAttribute" | "removeAttribute" => {
                if let Some(n) = args.first().map(Wert::als_text) {
                    if (n.eq_ignore_ascii_case("href") || n.eq_ignore_ascii_case("xlink:href"))
                        && name != "removeAttribute"
                        && self.dok.ist_element(k, Namensraum::Svg, "script")
                        && !self.skripte_gestartet.contains(&k)
                    {
                        // SVG 2 §15.2: setting `href` prepares the script.
                        self.skripte_vormerken(&[k]);
                    } else if n.eq_ignore_ascii_case("src") && name != "removeAttribute" {
                        self.medien_attribut(k, "src");
                    } else if n.eq_ignore_ascii_case("open") {
                        self.medien_attribut(k, "open");
                    }
                }
            }
            "removeChild" => {
                self.medien_kind_weg(k);
                if let Some(c) = args.first().and_then(|a| knoten_aus(self, Some(a))) {
                    self.medien_entfernt(c);
                }
            }
            "appendChild" | "insertBefore" | "replaceChild" | "insertAdjacentElement" | "append" | "prepend" | "before"
            | "after" | "replaceWith" => {
                if name == "replaceChild" {
                    self.medien_kind_weg(k);
                }
                let welche: &[Wert] = match name {
                    "insertAdjacentElement" => args.get(1..2).unwrap_or(&[]),
                    "appendChild" | "insertBefore" | "replaceChild" => args.get(0..1).unwrap_or(&[]),
                    _ => args,
                };
                let mut eingefuegt = Vec::new();
                for a in welche.iter().take(64) {
                    if let Some(c) = knoten_aus(self, Some(a)) {
                        self.medien_eingefuegt(c);
                        eingefuegt.push(c);
                    }
                }
                self.details_exklusiv(&eingefuegt);
            }
            _ => {}
        }
    }

    /// Release 11.10.2026: ein Eigenschafts-Setter auf dem schnellen Weg —
    /// `track.src = …` und neue Kinder aus `innerHTML`.
    fn medien_nach_schreiben(&mut self, k: usize, name: &str) {
        if matches!(name, "textContent" | "innerText" | "innerHTML" | "text")
            && self.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("script"))
        {
            if !self.skripte_gestartet.contains(&k) {
                self.skripte_vormerken(&[k]);
            }
            return;
        }
        // Release 2026-10-15: relevant mutations of an `<img>` or of a
        // `<picture>`'s `<source>` (see `bild_attribut`).
        if matches!(name, "src" | "srcset" | "sizes" | "media" | "type" | "width" | "height") {
            self.bild_attribut(k, name, None);
        }
        match name {
            "src" => self.medien_attribut(k, "src"),
            // Release 12.10.2026: `img.srcset = …` (siehe `medien_attribut`).
            "srcset" => self.medien_attribut(k, "srcset"),
            // Release 2026-10-15: `details.open = …` (the host writes the
            // boolean itself): `toggle` and name-group exclusivity. MEASURED
            // before (WPT the-details-element/toggleEvent, 9 failures): only
            // `setAttribute('open', '')` fired `toggle`.
            "open" => self.medien_attribut(k, "open"),
            "innerHTML" | "outerHTML" | "textContent" | "innerText" => {
                if self.html_name(k) == Some("style")
                    && let Some(h) = self.host(k)
                {
                    self.ce_schieben(CeReaktion::Medien { host: h, art: 9 });
                }
                self.medien_kind_weg(k);
                let kinder: Vec<usize> = self.dok.kinder(k).iter().copied().take(256).collect();
                for &c in &kinder {
                    self.medien_eingefuegt(c);
                }
                self.details_exklusiv(&kinder);
            }
            _ => {}
        }
    }

    /// Release 11.10.2026: ein Medium ging aus dem Dokument (HTML §4.8.11.5:
    /// danach laufen die „internal pause steps").
    fn medien_entfernt(&mut self, k: usize) {
        let mut stapel = vec![k];
        let mut n = 0;
        while let Some(x) = stapel.pop() {
            n += 1;
            if n > 4096 {
                return;
            }
            if matches!(self.dok.element_name(x), Some("audio" | "video")) {
                if let Some(h) = self.host(x) {
                    self.ce_schieben(CeReaktion::Medien { host: h, art: 6 });
                }
                continue;
            }
            stapel.extend(self.dok.kinder(x).iter().rev().copied());
        }
    }

    /// Release 11.10.2026: eingefuegte Medien und ihre `<source>`/`<track>`.
    fn medien_eingefuegt(&mut self, k: usize) {
        // Release 2026-10-15: an `<img>` inserted into a `<picture>`, or a
        // `<source>` inserted before one, is a relevant mutation.
        if let Some(e) = self.dok.eltern(k) {
            for img in self.bilder_bei_kind(e, k) {
                self.bild_erneut_vermerken(img);
            }
        }
        // A node inserted into a <style> changes its sheet: the preamble
        // reports `load` again (art 9, HTML §4.2.6 „update a style block").
        if let Some(e) = self.dok.eltern(k)
            && self.html_name(e) == Some("style")
            && let Some(h) = self.host(e)
        {
            self.ce_schieben(CeReaktion::Medien { host: h, art: 9 });
        }
        let mut stapel = vec![k];
        let mut n = 0;
        while let Some(x) = stapel.pop() {
            n += 1;
            if n > 4096 {
                return;
            }
            let art = match self.html_name(x) {
                Some("audio" | "video") => 3,
                // Release 12.10.2026: ein eingefuegtes `<link>` (Art 9: der
                // Vorspann prueft Blattstand und meldet `load`/`error`).
                // Since 14.10.2026 also `<style>` (load once per content).
                Some("link" | "style") => 9,
                Some("source" | "track")
                    if x == k
                        && self.dok.eltern(x).is_some_and(|e| matches!(self.html_name(e), Some("audio" | "video"))) =>
                {
                    2
                }
                _ => 0,
            };
            if art > 0
                && let Some(h) = self.host(x)
            {
                self.ce_schieben(CeReaktion::Medien { host: h, art });
            }
            if art != 3 {
                stapel.extend(self.dok.kinder(x).iter().rev().copied());
            }
        }
    }

    fn ce_attribut_ns(&mut self, k: usize, name: &str, alt: Option<String>, neu: Option<String>, raum: Option<String>) {
        // Nur das `src` OHNE Namensraum ist das Inhaltsattribut (WPT
        // resource-selection-invoke-set-src-in-namespace).
        if raum.as_deref().is_none_or(str::is_empty) {
            self.medien_attribut(k, name);
        }
        let Some(tag) = self.ce_name(k) else { return };
        if !self.ce_namen.get(tag).is_some_and(|b| b.iter().any(|n| n == name)) { return; }
        if let Some(host) = self.host(k) {
            self.ce_schieben(CeReaktion::Attribut { host, name: name.to_string(), alt, neu, raum });
        }
    }

    /// Ein Attribut hat sich geaendert: beobachtet es ein definiertes Element?
    fn ce_attribut(&mut self, k: usize, name: &str, alt: Option<String>) {
        self.medien_attribut(k, name);
        self.bild_attribut(k, name, alt.as_deref());
        let Some(tag) = self.ce_name(k) else { return };
        let Some(beobachtet) = self.ce_namen.get(tag) else { return };
        if !beobachtet.iter().any(|b| b == name) {
            return;
        }
        let neu = self.dok.attribut(k, name).map(str::to_string);
        if let Some(h) = self.host(k) {
            self.ce_schieben(CeReaktion::Attribut { host: h, name: name.to_string(), alt, neu, raum: None });
        }
    }

    /// Kinder eines Fragments (es gibt sie beim Einfuegen ab) oder der Knoten selbst.
    fn eingefuegte(&self, k: usize) -> Vec<usize> {
        match self.dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::Fragment | Art::ShadowRoot { .. }) => self.dok.kinder(k).to_vec(),
            _ => vec![k],
        }
    }

    /// Release 10.10.2026: die Knoten, die ein Einfuege-Aufruf ins Dokument
    /// bringt (vor dem Aufruf gerechnet — ein Fragment gibt seine Kinder ab).
    fn skript_kandidaten(&self, name: &str, args: &[Wert]) -> Vec<usize> {
        let quellen: &[Wert] = match name {
            "appendChild" | "append" | "prepend" | "before" | "after" | "replaceWith" => args,
            "insertBefore" | "replaceChild" => args.get(..1).unwrap_or(&[]),
            "insertAdjacentElement" => args.get(1..2).unwrap_or(&[]),
            _ => return Vec::new(),
        };
        let mut raus = Vec::new();
        for a in quellen.iter().take(4096) {
            if let Some(n) = knoten_aus(self, Some(a)) {
                raus.extend(self.eingefuegte(n));
            }
        }
        raus
    }

    /// Release 10.10.2026: die noch nicht gestarteten `<script>` in den
    /// eingefuegten Teilbaeumen, die jetzt im Dokument stehen, vormerken
    /// (HTML §8.4.3 „prepare the script element" beim Einfuegen).
    /// Template-Inhalte gehoeren nicht dazu (eigene Arena-Wurzel).
    fn skripte_vormerken(&mut self, kandidaten: &[usize]) {
        const MAX_DYN_SKRIPTE: usize = 4096;
        for &r in kandidaten {
            if !self.ist_verbunden(r) {
                continue;
            }
            let mut stapel = vec![r];
            while let Some(x) = stapel.pop() {
                if self.dok.element_name(x).is_some_and(|n| n.eq_ignore_ascii_case("script"))
                    // Release 14.10.2026: an unsupported type returns before
                    // „already started" (a later type change plus
                    // re-insertion still runs it).
                    && self.script_type_supported(x)
                    && self.skripte_gestartet.len() < MAX_DYN_SKRIPTE * 4
                    && self.skripte_gestartet.insert(x)
                    && self.dyn_skripte.len() < MAX_DYN_SKRIPTE
                {
                    // Release 11.10.2026: ein Inline-Skript laeuft sofort
                    // (GEMESSEN vorher: erst nach dem einfuegenden Skript —
                    // WPT execution-timing 025, 076, 129 u. v. a.).
                    let ohne_src = self.script_source(x).is_none();
                    // §4.12.1.1 Schritt „no src and empty source text":
                    // zurueck, OHNE „already started" — ein spaeteres `src`
                    // oder neuer Text bereitet es noch vor.
                    if ohne_src && self.child_text(x).is_empty() {
                        self.skripte_gestartet.remove(&x);
                    } else {
                        let inline = ohne_src
                            && self.dok.attribut(x, "nomodule").is_none()
                            && crate::js::klassischer_skript_typ(self.dok.attribut(x, "type").unwrap_or(""));
                        match self.host(x) {
                            Some(h) if inline => self.ce_schieben(CeReaktion::Skript(h)),
                            _ => self.dyn_skripte.push(x),
                        }
                    }
                }
                for &c in self.dok.kinder(x).iter().rev() {
                    stapel.push(c);
                }
            }
        }
    }

    /// Release 13.10.2026: alles, was in Baumreihenfolge HINTER `skript`
    /// steht, aus den Kinderlisten nehmen (HTML §13.2: ein Parser-Skript sieht
    /// nur das bis dahin geparste Dokument). Roh — ohne Protokoll, ohne
    /// Bereiche, ohne `geaendert`; `schwanz_einblenden` stellt es wieder her.
    /// GEMESSEN vorher (WPT scripting-1/execution-timing): Skripte sahen
    /// Elemente, die erst nach ihnen kommen („found #foo element: YES").
    pub(crate) fn schwanz_ausblenden(&mut self, skript: usize) -> Vec<(usize, Vec<usize>)> {
        let mut raus = Vec::new();
        let mut n = skript;
        let mut tiefe = 0usize;
        while let Some(p) = self.dok.knoten.get(n).and_then(|k| k.eltern) {
            tiefe += 1;
            if tiefe > 4096 {
                break;
            }
            let Some(pos) = self.dok.knoten[p].kinder.iter().position(|&c| c == n) else { break };
            let rest = self.dok.knoten[p].kinder.split_off(pos + 1);
            if !rest.is_empty() {
                self.dok.tree_changed();
                raus.push((p, rest));
            }
            n = p;
        }
        self.upgrade_revealed();
        raus
    }

    /// Gegenstueck zu `schwanz_ausblenden`: die ausgeblendeten Knoten hinter
    /// alles haengen, was das Skript inzwischen eingefuegt hat.
    pub(crate) fn schwanz_einblenden(&mut self, liste: Vec<(usize, Vec<usize>)>) {
        const MAX_REVEALED: usize = 100_000;
        for (p, rest) in liste {
            if self.revealed_tail.len() < MAX_REVEALED {
                self.revealed_tail.extend(rest.iter().take(MAX_REVEALED - self.revealed_tail.len()));
            }
            if let Some(k) = self.dok.knoten.get_mut(p) {
                k.kinder.extend(rest);
                self.dok.tree_changed();
            }
        }
    }

    /// Release 14.10.2026: the parser creates a defined custom element
    /// synchronously (HTML §13.2.6.1 „create an element for the token",
    /// synchronous custom elements flag). The tail behind a parser script is
    /// hidden while it runs, so `customElements.define` cannot see it; once
    /// the next script starts (or parsing ends), the elements shown again in
    /// between are queued for their upgrade. The preamble upgrades each one
    /// at most once (`aufwerten`). MEASURED before (WPT custom-elements
    /// ElementInternals-accessibility, 50 subtests): a `<test-element>`
    /// parsed after `define` had no `_internals` in the next script.
    pub(crate) fn upgrade_revealed(&mut self) {
        let roots = std::mem::take(&mut self.revealed_tail);
        if self.ce_lokal.is_empty() {
            return;
        }
        let mut found = Vec::new();
        for r in roots {
            if self.dok.wurzel_von(r) == self.wurzel {
                self.ce_teilbaum(r, &mut found);
            }
        }
        for e in found {
            if let Some(h) = self.host(e) {
                self.ce_schieben(CeReaktion::Upgrade(h));
            }
        }
    }

    /// Release 11.10.2026: Skripte aus dem Fragment-Parser (`innerHTML`,
    /// `outerHTML`, `insertAdjacentHTML`) sind „already started" und laufen
    /// nie (HTML §13.4 „parsing HTML fragments"). GEMESSEN vorher: sie liefen,
    /// sobald ihr Teilbaum eingefuegt wurde.
    fn skripte_markieren(&mut self, k: usize) {
        const MAX_DYN_SKRIPTE: usize = 4096;
        let mut stapel = vec![k];
        while let Some(x) = stapel.pop() {
            if self.dok.element_name(x).is_some_and(|n| n.eq_ignore_ascii_case("script")) {
                if self.skripte_gestartet.len() < MAX_DYN_SKRIPTE * 4 {
                    self.skripte_gestartet.insert(x);
                }
                continue;
            }
            stapel.extend(self.dok.kinder(x).iter().copied());
        }
    }

    /// Release 15.10.2026: notes every `<img>` of the parsed document with a
    /// source (not `loading=lazy`) so images that get no box (`display:
    /// none`) load too; the page path keeps reporting the rendered ones.
    pub fn note_document_images(&mut self) {
        let mut stack = vec![self.wurzel];
        let mut seen = 0usize;
        while let Some(k) = stack.pop() {
            seen += 1;
            if seen > 200_000 {
                break;
            }
            if self.dok.element_name(k) == Some("img") {
                let has_source = self.dok.attribut(k, "src").is_some_and(|s| !s.trim().is_empty())
                    || self.dok.attribut(k, "srcset").is_some_and(|s| !s.trim().is_empty());
                let lazy = self.dok.attribut(k, "loading").is_some_and(|s| s.trim().eq_ignore_ascii_case("lazy"));
                if has_source && !lazy {
                    self.bild_quelle_vermerken(k);
                }
            }
            stack.extend(self.dok.kinder(k).iter().copied());
        }
    }

    /// Release 10.10.2026: merkt ein `<img>`, dessen `src` ein Skript setzt.
    fn bild_quelle_vermerken(&mut self, k: usize) {
        const MAX_BILD_QUELLEN: usize = 4096;
        if self.dok.element_name(k) == Some("img")
            && self.bild_quellen.len() < MAX_BILD_QUELLEN
            && !self.bild_quellen.contains(&k)
        {
            self.bild_quellen.push(k);
        }
    }

    /// Release 2026-10-15: an `<img>` had a relevant mutation (HTML
    /// §4.8.4.3.3); its image data is updated again.
    fn bild_erneut_vermerken(&mut self, img: usize) {
        const MAX_BILD_ERNEUT: usize = 4096;
        if self.html_name(img) == Some("img") && self.bild_erneut.len() < MAX_BILD_ERNEUT && !self.bild_erneut.contains(&img) {
            self.bild_erneut.push(img);
            self.bild_quelle_vermerken(img);
        }
    }

    /// Release 2026-10-15: HTML §4.11.1 "ensure details exclusivity by
    /// closing the given element if needed" for inserted `<details>` (in
    /// tree order, as the DOM inserts them one at a time) and for a changed
    /// `name`: an open, named details closes when another member of its
    /// name group (same tree, same name) is open. The closing itself is the
    /// prelude's (`__narsilMedien` art 10: remove `open`). MEASURED before
    /// (WPT the-details-element/name-attribute, 16 failures): several
    /// details of one name stayed open.
    fn details_exklusiv(&mut self, wurzeln: &[usize]) {
        const MAX_DETAILS: usize = 256;
        let mut neu: Vec<usize> = Vec::new();
        for &r in wurzeln.iter().take(64) {
            let mut stapel = vec![r];
            let mut n = 0;
            while let Some(x) = stapel.pop() {
                n += 1;
                if n > 4096 || neu.len() >= MAX_DETAILS {
                    break;
                }
                if matches!(self.dok.knoten.get(x).map(|k| &k.art), Some(Art::ShadowRoot { .. })) {
                    continue;
                }
                if self.html_name(x) == Some("details")
                    && self.dok.attribut(x, "open").is_some()
                    && self.dok.attribut(x, "name").is_some_and(|v| !v.is_empty())
                {
                    neu.push(x);
                }
                stapel.extend(self.dok.kinder(x).iter().rev().copied());
            }
        }
        let mut zu: Vec<usize> = Vec::new();
        for (i, &d) in neu.iter().enumerate() {
            let Some(name) = self.dok.attribut(d, "name").map(str::to_string) else { continue };
            let spaeter = &neu[i + 1..];
            let wurzel = self.dok.wurzel_von(d);
            let mut stapel = vec![wurzel];
            let mut n = 0;
            let mut offen = false;
            while let Some(x) = stapel.pop() {
                n += 1;
                if n > 100_000 {
                    break;
                }
                if x != wurzel && matches!(self.dok.knoten.get(x).map(|k| &k.art), Some(Art::ShadowRoot { .. })) {
                    continue;
                }
                if x != d
                    && !spaeter.contains(&x)
                    && !zu.contains(&x)
                    && self.html_name(x) == Some("details")
                    && self.dok.attribut(x, "open").is_some()
                    && self.dok.attribut(x, "name") == Some(name.as_str())
                {
                    offen = true;
                    break;
                }
                stapel.extend(self.dok.kinder(x).iter().copied());
            }
            if offen {
                zu.push(d);
            }
        }
        for d in zu {
            if let Some(h) = self.host(d) {
                self.ce_schieben(CeReaktion::Medien { host: h, art: 10 });
            }
        }
    }

    /// The `<img>` children of a `<picture>` that follow `nach`.
    fn bilder_nach(&self, picture: usize, nach: usize) -> Vec<usize> {
        let kinder = self.dok.kinder(picture);
        let Some(pos) = kinder.iter().position(|&c| c == nach) else { return Vec::new() };
        kinder[pos + 1..].iter().copied().filter(|&c| self.html_name(c) == Some("img")).take(64).collect()
    }

    /// The images a node insertion or removal at `kind` under `eltern`
    /// mutates: the `<img>` itself, or the images after a `<source>`, when
    /// the parent is a `<picture>`.
    fn bilder_bei_kind(&self, eltern: usize, kind: usize) -> Vec<usize> {
        if self.html_name(eltern) != Some("picture") {
            return Vec::new();
        }
        match self.html_name(kind) {
            Some("img") => vec![kind],
            Some("source") => self.bilder_nach(eltern, kind),
            _ => Vec::new(),
        }
    }

    /// Release 2026-10-15: an attribute of an `<img>` or of a `<picture>`'s
    /// `<source>` changed (`alt` is the old value): `src`/`srcset`/`sizes`
    /// always count, `crossorigin`/`referrerpolicy` when their state
    /// changes; a source's `srcset`/`sizes`/`media`/`type`/`width`/`height`
    /// mutate the images after it. MEASURED before (WPT the-img-element/
    /// relevant-mutations, 44 failures): no second `load` for an unchanged
    /// source.
    fn bild_attribut(&mut self, k: usize, name: &str, alt: Option<&str>) {
        fn cors(v: Option<&str>) -> u8 {
            match v {
                None => 0,
                Some(v) if v.eq_ignore_ascii_case("use-credentials") => 2,
                Some(_) => 1,
            }
        }
        fn verweis(v: Option<&str>) -> String {
            const ZUSTAENDE: [&str; 8] = [
                "no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin",
                "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url",
            ];
            let v = v.unwrap_or("").to_ascii_lowercase();
            if ZUSTAENDE.contains(&v.as_str()) { v } else { String::new() }
        }
        match self.html_name(k) {
            Some("img") => {
                let relevant = match name {
                    "src" | "srcset" | "sizes" => true,
                    "crossorigin" => cors(alt) != cors(self.dok.attribut(k, name)),
                    "referrerpolicy" => verweis(alt) != verweis(self.dok.attribut(k, name)),
                    _ => false,
                };
                if relevant {
                    self.bild_erneut_vermerken(k);
                }
            }
            Some("source") if matches!(name, "srcset" | "sizes" | "media" | "type" | "width" | "height") => {
                if let Some(p) = self.dok.eltern(k)
                    && self.html_name(p) == Some("picture")
                {
                    for img in self.bilder_nach(p, k) {
                        self.bild_erneut_vermerken(img);
                    }
                }
            }
            _ => {}
        }
    }

    /// Release 2026-10-15: what a fast-path call is about to change for the
    /// image model (taken before the call): an attribute's old value, or the
    /// images a removal mutates.
    fn bild_vorher(&self, k: usize, name: &str, args: &[Wert]) -> BildVorher {
        match name {
            "setAttribute" | "removeAttribute" | "toggleAttribute" => {
                if !matches!(self.html_name(k), Some("img" | "source")) {
                    return BildVorher::Nichts;
                }
                let Some(n) = args.first().map(|a| a.als_text().to_ascii_lowercase()) else { return BildVorher::Nichts };
                let alt = self.dok.attribut(k, &n).map(str::to_string);
                BildVorher::Attribut(n, alt)
            }
            "removeChild" => match args.first().and_then(|a| knoten_aus(self, Some(a))) {
                Some(c) if self.dok.eltern(c) == Some(k) => BildVorher::Bilder(self.bilder_bei_kind(k, c)),
                _ => BildVorher::Nichts,
            },
            "remove" => match self.dok.eltern(k) {
                Some(e) => BildVorher::Bilder(self.bilder_bei_kind(e, k)),
                None => BildVorher::Nichts,
            },
            _ => BildVorher::Nichts,
        }
    }

    fn bild_nachher(&mut self, k: usize, vorher: BildVorher) {
        match vorher {
            BildVorher::Nichts => {}
            BildVorher::Attribut(n, alt) => self.bild_attribut(k, &n, alt.as_deref()),
            BildVorher::Bilder(l) => {
                for img in l {
                    self.bild_erneut_vermerken(img);
                }
            }
        }
    }

    /// Release 11.10.2026: DOM §4.4 „connected" — die schatten-einschliessende
    /// Wurzel ist IRGENDEIN Dokument (auch ein Iframe- oder erzeugtes
    /// Dokument). GEMESSEN vorher: `isConnected` war nur im Hauptdokument
    /// wahr; `showModal()` in einem Iframe warf InvalidStateError.
    /// (`ist_verbunden` bleibt beim Hauptdokument: Skripte, Medien.)
    pub(crate) fn in_einem_dokument(&self, k: usize) -> bool {
        let mut x = k;
        for _ in 0..64 {
            let w = self.dok.wurzel_von(x);
            match self.dok.knoten.get(w).map(|n| (&n.art, n.eltern)) {
                Some((Art::Dokument, _)) => return true,
                Some((Art::ShadowRoot { .. }, Some(wirt))) => x = wirt,
                _ => return false,
            }
        }
        false
    }

    pub(crate) fn ist_verbunden(&self, k: usize) -> bool {
        let mut x = k;
        for _ in 0..64 {
            let w = self.dok.wurzel_von(x);
            if w == self.wurzel {
                return true;
            }
            match self.dok.knoten.get(w).map(|n| (&n.art, n.eltern)) {
                Some((Art::ShadowRoot { .. }, Some(wirt))) => x = wirt,
                _ => return false,
            }
        }
        false
    }
}

/// `fontSize` -> `font-size`.
fn css_name(n: &str) -> String {
    let mut raus = String::with_capacity(n.len() + 4);
    for c in n.chars() {
        if c.is_ascii_uppercase() {
            raus.push('-');
            raus.push(c.to_ascii_lowercase());
        } else {
            raus.push(c);
        }
    }
    raus
}

/// Release 12.10.2026: der Deklarationsblock eines `style`-Attributs.
type StilBlock = narsil_css::cssom::block::Block;

/// Release 12.10.2026: hoechstens so viele Bloecke im Cache (M3).
const MAX_STIL_BLOECKE: usize = 4096;

/// Release 12.10.2026: der Block des Elements (CSSOM §6.7 „parse a CSS
/// declaration block") — aus dem Cache, solange das Attribut der zuletzt
/// geschriebene Text ist. GEMESSEN vorher (WPT css/*/parsing): der Inline-Stil
/// war eine Liste aus Rohtexten; Kurzformen setzten keine Langformen, `0`
/// blieb `0`, ungueltige Werte wurden gesetzt (~1 400 Teiltests).
fn stil_block(l: &Lebend, k: usize) -> StilBlock {
    let text = l.dok.attribut(k, "style").unwrap_or("");
    if let Some((t, b)) = l.stil_bloecke.get(&k)
        && t == text
    {
        return b.clone();
    }
    StilBlock::lesen(text)
}

/// Release 12.10.2026: den Block als `style`-Attribut schreiben (CSSOM
/// „update style attribute for the CSS declaration block").
fn stil_block_schreiben(l: &mut Lebend, k: usize, neu: StilBlock) {
    let vorher = l.dok.attribut(k, "style").map(str::to_string);
    let text = neu.text();
    l.dok.setze_attribut(k, Namensraum::Html, "style", &text);
    if l.stil_bloecke.len() >= MAX_STIL_BLOECKE && !l.stil_bloecke.contains_key(&k) {
        l.stil_bloecke.clear();
    }
    l.stil_bloecke.insert(k, (text, neu));
    l.geaendert = true;
    if l.ce_aktiv() {
        l.ce_attribut(k, "style", vorher);
    }
}

/// Release 10.10.2026: der CSS-Name einer `style`-Eigenschaft (CSSOM §6.7.1:
/// `fontSize` -> `font-size`, `webkitTransform`/`WebkitTransform` ->
/// `-webkit-transform`, `cssFloat` -> `float`; `style['font-size']` bleibt).
fn stil_name(n: &str) -> String {
    if matches!(n, "webkitFilter" | "WebkitFilter") || n.eq_ignore_ascii_case("-webkit-filter") { return "filter".into(); }
    if n == "cssFloat" {
        return "float".to_string();
    }
    if n.starts_with("--") {
        return n.to_string();
    }
    if n.contains('-') {
        return n.to_ascii_lowercase();
    }
    let mut s = css_name(n);
    for p in ["webkit-", "moz-", "ms-"] {
        if s.starts_with(p) {
            s.insert(0, '-');
            break;
        }
    }
    s
}

/// Release 10.10.2026: `el.style.x` lesen (CSSStyleDeclaration, CSSOM §6.7).
/// Release 12.10.2026: aus dem Deklarationsblock (`getPropertyValue`, auch
/// fuer Kurzformen).
pub fn stil_lesen(l: &mut Lebend, id: u32, name: &str) -> Option<Wert> {
    let k = l.knoten(id)?;
    if !l.dok.ist_element_knoten(k) {
        return None;
    }
    let b = stil_block(l, k);
    Some(match name {
        "cssText" => Wert::Text(b.text()),
        "length" => Wert::Zahl(b.laenge() as f64),
        "parentRule" => Wert::Null,
        // Release 12.10.2026: nur Indizes und CSS-Eigenschaften beantwortet
        // der Host; alles andere (`hasOwnProperty`, `Symbol.iterator`,
        // `--x`, Expandos) kommt aus der Prototypkette (CSSOM §6.7.1,
        // WebIDL). GEMESSEN vorher (WPT cssom CSSStyleDeclaration-iterator,
        // cssstyledeclaration-properties): `style.hasOwnProperty` war "".
        _ => match name.parse::<usize>() {
            Ok(i) => Wert::Text(b.eintrag(i)?.to_string()),
            Err(_) => {
                let n = stil_name(name);
                if n.starts_with("--")
                    || !(narsil_css::cssom::block::name_kanonisch(&n).is_some() || narsil_css::cssom::block::ist_kurz(&n))
                {
                    return None;
                }
                Wert::Text(b.wert(&n))
            }
        },
    })
}

/// Release 10.10.2026: `el.style.x = …` und `el.style.cssText = …` (ersetzt
/// den ganzen Block). `null` ist der leere Text (`[LegacyNullToEmptyString]`).
/// Release 12.10.2026: ein ungueltiger Wert aendert nichts (und schreibt das
/// Attribut nicht — kein Mutation Record, WPT cssom mutationrecord-002).
pub fn stil_schreiben(l: &mut Lebend, id: u32, name: &str, wert: &Wert) -> Option<Result<(), String>> {
    let k = l.knoten(id)?;
    if !l.dok.ist_element_knoten(k) {
        return None;
    }
    if matches!(name, "length" | "parentRule") {
        return Some(Ok(()));
    }
    let text = match wert {
        Wert::Null => String::new(),
        w => w.als_text(),
    };
    if name == "cssText" {
        stil_block_schreiben(l, k, StilBlock::lesen(&text));
        return Some(Ok(()));
    }
    let mut b = stil_block(l, k);
    if b.setzen(&stil_name(name), &text, "") {
        stil_block_schreiben(l, k, b);
    }
    Some(Ok(()))
}

/// Release 10.10.2026: `style.setProperty/removeProperty/getPropertyValue/
/// getPropertyPriority/item` (CSSOM §6.7). Die Namen kommen hier schon in
/// CSS-Schreibweise (`font-size`, `--x`).
fn stil_rufen(l: &mut Lebend, k: usize, name: &str, args: &[Wert]) -> Option<Result<Wert, String>> {
    let eig = || args.first().map(|a| a.als_text()).unwrap_or_default();
    let mut b = stil_block(l, k);
    Some(Ok(match name {
        "getPropertyValue" => Wert::Text(b.wert(&eig())),
        "getPropertyPriority" => Wert::Text(b.prioritaet(&eig()).to_string()),
        "item" => {
            let i = args.first().map_or(0.0, |a| match a {
                Wert::Zahl(n) => *n,
                w => w.als_text().parse().unwrap_or(0.0),
            });
            Wert::Text(if i >= 0.0 { b.eintrag(i as usize).unwrap_or_default().to_string() } else { String::new() })
        }
        "setProperty" => {
            let n = eig();
            // Release 12.10.2026: der Wert ist `[LegacyNullToEmptyString]
            // CSSOMString` — `null` wird "", `undefined` aber "undefined"
            // (ungueltig, aendert nichts; WPT setproperty-null-undefined).
            let wert = match args.get(1) {
                None | Some(Wert::Null) => String::new(),
                Some(Wert::Undefiniert) => String::from("undefined"),
                Some(w) => w.als_text(),
            };
            // Release 10.10.2026: `undefined` ist der Vorgabewert `""`,
            // `null` wird `""` (WebIDL: `optional [LegacyNullToEmptyString]
            // CSSOMString priority = ""`). GEMESSEN vorher: Alpine.js ruft
            // `style.setProperty('display', 'none', void 0)` fuer `x-show` —
            // „undefined" galt als unbekannte Prioritaet, nichts wurde
            // gesetzt, und auf spiegel.de standen Dialog und Seitenleisten
            // offen.
            let prio = match args.get(2) {
                None | Some(Wert::Null) | Some(Wert::Undefiniert) => String::new(),
                Some(a) => a.als_text(),
            };
            if b.setzen(&n, &wert, &prio) {
                stil_block_schreiben(l, k, b);
            }
            Wert::Undefiniert
        }
        "removeProperty" => {
            let (alt, geaendert) = b.entfernen(&eig());
            if geaendert {
                stil_block_schreiben(l, k, b);
            }
            Wert::Text(alt)
        }
        _ => return None,
    }))
}

/// Geht den Seitenbaum in Vorordnung und meldet `(ord, Arena-Index)` nach
/// der Zaehlregel des Seitenbaus (`narsil_page::zaehle`: Texte und
/// `option`/`optgroup` in Steuerelementen zaehlen nicht, ab
/// `MAX_BAU_TIEFE` nichts mehr). `pos` laeuft ueber ALLE Knoten, denn die
/// Indexliste enthaelt alle.
/// Ord -> Arena-Index: die Zaehlregel des Seitenbaus (Licht in Vorordnung,
/// danach die Shadow Trees in Breitenfolge; Release 12.10.2026). Die Regel
/// steht an EINER Stelle (`narsil_page::schatten::ords_gehen`), damit
/// Seitenbau und Bruecke nie auseinanderlaufen.
/// Release 13.10.2026: the CSSOM state of `<style>` sheets in the page tree:
/// a disabled sheet gets the type of a switched-off sheet
/// (`scope_implizit::TYP_AUS`), a sheet changed by scripts gets its CSSOM text
/// as content. The light tree is walked in preorder like `indizes`; a position
/// is only used when the arena node there really is a `<style>`. MEASURED
/// before (WPT cssom CSSStyleSheet-constructable "Constructed style sheets can
/// be applied on document", css-style-reparse): a disabled sheet still
/// applied, and `insertRule` rewrote the element's `textContent`.
fn apply_sheet_state(
    html: &mut [HtmlNode],
    indizes: &mut Vec<usize>,
    dok: &Dokument,
    disabled: &std::collections::HashSet<usize>,
    texts: &HashMap<usize, (String, String)>,
    animated: &HashMap<usize, Vec<(String, String)>>,
) {
    struct State<'a> {
        dok: &'a Dokument,
        disabled: &'a std::collections::HashSet<usize>,
        texts: &'a HashMap<usize, (String, String)>,
        animated: &'a HashMap<usize, Vec<(String, String)>>,
    }
    fn walk(list: &mut [HtmlNode], indizes: &mut Vec<usize>, s: &State, pos: &mut usize, depth: usize) {
        for node in list.iter_mut() {
            let mine = *pos;
            *pos += 1;
            let HtmlNode::Element { tag, attributes, children, .. } = node else { continue };
            // Release 15.10.2026: animated values after the element's own
            // declarations (`Lebend::animated`).
            if !s.animated.is_empty()
                && let Some(&k) = indizes.get(mine)
                && let Some(werte) = s.animated.get(&k)
                && !werte.is_empty()
            {
                let mut stil = attributes.get("style").cloned().unwrap_or_default();
                stil.push_str(&animated_declarations(werte));
                attributes.insert(String::from("style"), stil);
            }
            if tag.eq_ignore_ascii_case("style")
                && let Some(&k) = indizes.get(mine)
                && s.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style"))
            {
                if s.disabled.contains(&k) {
                    attributes.insert(String::from("type"), String::from(crate::scope_implizit::TYP_AUS));
                }
                if let Some((_, sheet)) = s.texts.get(&k) {
                    if children.is_empty() {
                        // An empty element gets a text node; its position
                        // has no arena node (`usize::MAX`), so all other
                        // positions still match.
                        children.push(HtmlNode::Text(sheet.clone()));
                        if mine < indizes.len() {
                            indizes.insert(mine + 1, usize::MAX);
                        }
                        *pos += 1;
                        continue;
                    }
                    // The element's text nodes (and so all positions) stay;
                    // the first one carries the sheet text.
                    let mut first = true;
                    for child in children.iter_mut() {
                        if let HtmlNode::Text(text) = child {
                            *text = if first { sheet.clone() } else { String::new() };
                            first = false;
                        }
                    }
                }
            }
            if depth < 512 {
                walk(children, indizes, s, pos, depth + 1);
            }
        }
    }
    let state = State { dok, disabled, texts, animated };
    let mut pos = 0;
    walk(html, indizes, &state, &mut pos, 0);
}

/// Release 15.10.2026: the animated values of pseudo-elements in the page
/// tree. The element gets `data-narsil-pseudo-anim`, and a style element at
/// the end of the document carries `[data-narsil-pseudo-anim="k"]::before
/// { … !important }` (an animated value beats the normal declarations; the
/// position of the new nodes has no arena node, `usize::MAX`).
fn apply_pseudo_animations(html: &mut [HtmlNode], indizes: &mut Vec<usize>, animated: &HashMap<(usize, String), Vec<(String, String)>>) {
    let mut by_node: HashMap<usize, Vec<(&str, &Vec<(String, String)>)>> = HashMap::new();
    for ((k, art), werte) in animated {
        if !werte.is_empty() && art.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            by_node.entry(*k).or_default().push((art.as_str(), werte));
        }
    }
    if by_node.is_empty() {
        return;
    }
    fn walk(list: &mut [HtmlNode], indizes: &[usize], by_node: &HashMap<usize, Vec<(&str, &Vec<(String, String)>)>>, pos: &mut usize, rules: &mut String, depth: usize) {
        for node in list.iter_mut() {
            let mine = *pos;
            *pos += 1;
            let HtmlNode::Element { attributes, children, .. } = node else { continue };
            if let Some(&k) = indizes.get(mine)
                && let Some(arten) = by_node.get(&k)
            {
                attributes.insert(String::from("data-narsil-pseudo-anim"), k.to_string());
                for (art, werte) in arten {
                    rules.push_str(&format!("[data-narsil-pseudo-anim=\"{k}\"]::{art} {{"));
                    for (name, wert) in werte.iter() {
                        let wert = wert.strip_suffix(" !important").unwrap_or(wert);
                        rules.push_str(&format!(" {name}: {wert} !important;"));
                    }
                    rules.push_str(" }\n");
                }
            }
            if depth < 512 {
                walk(children, indizes, by_node, pos, rules, depth + 1);
            }
        }
    }
    let mut rules = String::new();
    let mut pos = 0;
    walk(html, indizes, &by_node, &mut pos, &mut rules, 0);
    if rules.is_empty() {
        return;
    }
    // The new style element is the last node in tree order: the last child
    // of the last top-level element.
    let Some(HtmlNode::Element { children, .. }) = html.last_mut() else { return };
    children.push(HtmlNode::Element {
        tag: String::from("style"),
        attributes: HashMap::new(),
        children: vec![HtmlNode::Text(rules)],
        shadow_root: None,
    });
    while indizes.len() < pos {
        indizes.push(usize::MAX);
    }
    indizes.truncate(pos);
    indizes.push(usize::MAX);
    indizes.push(usize::MAX);
}

/// Release 15.10.2026: animated values as declarations to append to a
/// `style` attribute (`; a: x; b: y`).
pub(crate) fn animated_declarations(werte: &[(String, String)]) -> String {
    let mut s = String::new();
    for (name, wert) in werte {
        s.push_str("; ");
        s.push_str(name);
        s.push_str(": ");
        s.push_str(wert);
    }
    s
}

impl Lebend {
    /// Release 13.10.2026: the current sheet text of a `<style>` element: the
    /// CSSOM text when scripts changed the sheet, else the element text.
    pub(crate) fn style_text(&self, k: usize) -> String {
        match self.sheet_texts.get(&k) {
            Some((key, sheet)) if *key == sheet_key(&self.dok, k) => sheet.clone(),
            _ => self.dok.text_inhalt(k),
        }
    }
}

/// Release 13.10.2026: what a CSSOM sheet text belongs to — the element text
/// and its child nodes (setting `textContent` to the same text still makes a
/// new sheet).
pub(crate) fn sheet_key(dok: &Dokument, k: usize) -> String {
    format!("{}\u{0}{:?}", dok.text_inhalt(k), dok.kinder(k))
}

fn ord_gehen(liste: &[HtmlNode], indizes: &[usize], melde: &mut dyn FnMut(usize, usize)) {
    narsil_page::schatten::ords_gehen(liste, &mut |_, ord, pos| {
        if let Some(&arena) = indizes.get(pos) {
            melde(ord, arena);
        }
    });
}

// ===========================================================================
// DIE HAKEN: lesen / schreiben / rufen
// ===========================================================================

/// Eigenschaften, die das lebende DOM beantwortet (Schreiben: siehe
/// [`ist_schreibbar`]).
pub const EIGENSCHAFTEN: &[&str] = &[
    "nodeType", "nodeName", "tagName", "localName", "parentNode", "parentElement",
    "childNodes", "children", "firstChild", "lastChild", "nextSibling", "previousSibling",
    "firstElementChild", "lastElementChild", "nextElementSibling", "previousElementSibling",
    "childElementCount", "textContent", "innerText", "nodeValue", "data", "innerHTML",
    "outerHTML", "isConnected", "ownerDocument", "content", "shadowRoot", "host", "length",
    "__ceIs", "attributes", "documentElement", "body", "head", "implementation", "id", "className",
    "hidden", "title", "lang", "slot",
];

/// Release 09.10.2026: Eigenschaften, die ein Element-Attribut spiegeln
/// (HTML §2.6.1 "reflect"), als Text.
const SPIEGEL_TEXT: &[(&str, &str)] = &[
    // Release 11.10.2026: Aufzaehlungen und Adressen (`dir`, `crossOrigin`,
    // `referrerPolicy`, `loading`, `decoding`, `preload`, `formEnctype`,
    // `formMethod`, `enctype`, `inputMode`, `formAction`, `cite`) spiegelt
    // jetzt `vorspann.js` nach HTML §2.6.1 (Schluesselwoerter, Vorgaben,
    // aufgeloeste Adresse). GEMESSEN vorher: der Rohwert kam zurueck.
    ("id", "id"), ("className", "class"), ("title", "title"), ("lang", "lang"),
    ("slot", "slot"),
    // Release 10.10.2026: weitere gespiegelte Texte (HTML §2.6.1 „reflect").
    // GEMESSEN vorher: `input.name`, `a.target`, `img.alt` … waren
    // `undefined` — `if (el.name === …)` in Formular-Skripten griff nie.
    ("name", "name"), ("placeholder", "placeholder"), ("alt", "alt"), ("rel", "rel"),
    ("target", "target"), ("accept", "accept"),
    ("download", "download"), ("htmlFor", "for"),
    ("pattern", "pattern"), ("integrity", "integrity"), ("charset", "charset"),
    ("media", "media"), ("sizes", "sizes"), ("srcset", "srcset"), ("hreflang", "hreflang"),
    ("accessKey", "accesskey"), ("contentEditable", "contenteditable"),
    ("coords", "coords"), ("shape", "shape"), ("ping", "ping"),
    ("useMap", "usemap"), ("content", "content"),
    ("httpEquiv", "http-equiv"), ("dateTime", "datetime"), ("label", "label"),
    ("formTarget", "formtarget"),
];

/// Release 2026-10-15: whether the reflected IDL attribute `name` exists on
/// the interface of element `k`. `htmlFor` is a string on label/script
/// (output's is a DOMTokenList, made by the prelude), `sizes` a string on
/// img/source (link's is a DOMTokenList); everything else in SPIEGEL_TEXT
/// still applies to every element. MEASURED before (WPT dom/lists/
/// DOMTokenList-coverage-for-attributes): `td.htmlFor` and `a.sizes` read ""
/// instead of undefined, `output.htmlFor` was a string.
pub(crate) fn reflection_applies(dok: &Dokument, k: usize, name: &str) -> bool {
    let html_one_of = |names: &[&str]| {
        matches!(dok.knoten.get(k).map(|n| &n.art),
            Some(Art::Element { raum: Namensraum::Html, name, .. }) if names.contains(&name.as_str()))
    };
    match name {
        "htmlFor" => html_one_of(&["label", "script"]),
        "sizes" => html_one_of(&["img", "source"]),
        // Release 2026-10-15: HTML §3.2.6 — only these reflect `name`.
        // MEASURED before (WPT name-content-attribute-and-property 17/141):
        // `div.name = 'x'` changed the attribute.
        "name" => html_one_of(&[
            "a", "button", "embed", "fieldset", "form", "frame", "iframe", "img", "input", "map", "meta", "object",
            "output", "param", "select", "slot", "textarea",
        ]),
        _ => true,
    }
}

/// Release 10.10.2026: gespiegelte WAHRHEITSWERTE (vorhanden = wahr).
const SPIEGEL_BOOL: &[(&str, &str)] = &[
    ("disabled", "disabled"), ("multiple", "multiple"), ("required", "required"), ("readOnly", "readonly"),
    ("autofocus", "autofocus"), ("defer", "defer"), ("async", "async"),
    ("noValidate", "novalidate"), ("open", "open"), ("selected", "selected"),
    ("controls", "controls"), ("autoplay", "autoplay"), ("loop", "loop"),
    ("muted", "muted"), ("playsInline", "playsinline"), ("reversed", "reversed"),
    ("noModule", "nomodule"), ("allowFullscreen", "allowfullscreen"), ("inert", "inert"),
    ("isMap", "ismap"), ("defaultMuted", "muted"), ("formNoValidate", "formnovalidate"),
];

/// Release 10.10.2026: `type` mit den Vorgaben der Spec — `input` ohne (oder
/// mit unbekanntem) `type` ist `"text"`, `button` ist `"submit"`; der Wert
/// ist klein geschrieben (HTML §4.10.5 „limited to only known values").
pub(crate) fn typ_lesen(tag: &str, roh: Option<&str>) -> String {
    let klein = roh.map(|t| t.trim().to_ascii_lowercase());
    match tag {
        "input" => {
            const BEKANNT: &[&str] = &[
                "hidden", "text", "search", "tel", "url", "email", "password", "date", "month", "week",
                "time", "datetime-local", "number", "range", "color", "checkbox", "radio", "file",
                "submit", "image", "reset", "button",
            ];
            match klein {
                Some(t) if BEKANNT.contains(&t.as_str()) => t,
                _ => "text".to_string(),
            }
        }
        "button" => match klein.as_deref() {
            Some("reset") => "reset".to_string(),
            Some("button") => "button".to_string(),
            _ => "submit".to_string(),
        },
        "select" => if roh.is_some() { klein.unwrap_or_default() } else { String::new() },
        _ => roh.unwrap_or("").to_string(),
    }
}

/// Eigenschaften, die das lebende DOM schreibt.
#[must_use]
pub fn ist_schreibbar(name: &str) -> bool {
    ist_schreibbar_basis(name)
        || name == "type"
        || SPIEGEL_TEXT.iter().any(|(e, _)| *e == name)
        || SPIEGEL_BOOL.iter().any(|(e, _)| *e == name)
}

/// Release 10.10.2026: die Eigenschaften, die auch der Proxy-Weg OHNE
/// lebendes DOM selbst behandelt (der Stand vor den gespiegelten Listen).
/// Die uebrigen (`alt`, `name`, `required` …) beansprucht der Proxy nur mit
/// lebendem DOM — sonst schreibt der Prototyp-Setter das Attribut
/// (`new Image(); img.alt = 'x'` -> `getAttribute('alt')`).
#[must_use]
pub fn ist_schreibbar_basis(name: &str) -> bool {
    matches!(
        name,
        "textContent" | "innerText" | "nodeValue" | "data" | "innerHTML" | "outerHTML" | "id"
            | "className" | "hidden" | "title" | "lang" | "slot" | "checked" | "indeterminate"
            | "src" | "href" | "action" | "poster"
    )
}

/// `document.title` (HTML §3.1.4): der Text des ersten `title`, Leerraum
/// zusammengezogen.
fn dokument_titel(dok: &Dokument, wurzel: usize) -> Option<usize> {
    let mut stapel = vec![wurzel];
    while let Some(k) = stapel.pop() {
        if dok.element_name(k) == Some("title") {
            return Some(k);
        }
        for &c in dok.kinder(k).iter().rev() {
            stapel.push(c);
        }
    }
    None
}

impl Lebend {
    /// Release 2026-10-15: a document, its document element or its body
    /// (the targets with the default passive value, DOM §2.7).
    pub(crate) fn default_passive_node(&self, k: usize) -> bool {
        let dok = &self.dok;
        if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument)) {
            return true;
        }
        let Some(p) = dok.eltern(k) else { return false };
        if matches!(dok.knoten.get(p).map(|n| &n.art), Some(Art::Dokument)) {
            return dok.ist_element_knoten(k) && element_kinder(dok, p).first() == Some(&k);
        }
        ist_html(dok, k)
            && dok.element_name(k) == Some("body")
            && dok.eltern(p).is_some_and(|d| matches!(dok.knoten.get(d).map(|n| &n.art), Some(Art::Dokument)))
            && ist_html(dok, p)
            && dok.element_name(p) == Some("html")
            && element_kinder(dok, p).iter().copied().find(|&c| ist_html(dok, c) && matches!(dok.element_name(c), Some("body" | "frameset"))) == Some(k)
    }

    /// Release 2026-10-15: the host object of the live collection (root,
    /// kind) — art 5 HTMLCollection, art 6 NodeList — or `None` when the
    /// registry is full (the caller hands out a snapshot then).
    pub(crate) fn collection(&mut self, root: usize, kind: CollectionKind) -> Option<Wert> {
        let art = f64::from(kind.art());
        let id = self.collection_id(root, kind)?;
        let mut felder = match crate::dom::host_wert(id) {
            Wert::Gegenstand(f) => f,
            _ => Vec::new(),
        };
        felder.push(("__art".to_string(), Wert::Zahl(art)));
        Some(Wert::Gegenstand(felder))
    }

    /// The host id of the collection (root, kind), registered on first use.
    fn collection_id(&mut self, root: usize, kind: CollectionKind) -> Option<u32> {
        let key = (root, kind);
        let id = match self.collection_ids.get(&key) {
            Some(&id) => id,
            None => {
                if self.collections.len() >= MAX_COLLECTIONS {
                    return None;
                }
                let id = FIRST_COLLECTION_HOST + self.collections.len() as u32;
                self.collections.insert(id, LiveCollection { root: key.0, kind: key.1.clone(), evaluated: None, items: Vec::new() });
                self.collection_ids.insert(key, id);
                id
            }
        };
        Some(id)
    }

    /// The current members of the collection (root, kind).
    pub(crate) fn collection_members(&mut self, root: usize, kind: CollectionKind) -> Option<Vec<usize>> {
        let id = self.collection_id(root, kind)?;
        if !self.collection_refresh(id) {
            return None;
        }
        self.collections.get(&id).map(|c| c.items.clone())
    }

    /// The members of a collection, evaluated now.
    fn collection_items(&self, root: usize, kind: &CollectionKind) -> Vec<usize> {
        let dok = &self.dok;
        match kind {
            CollectionKind::Children => element_kinder(dok, root),
            CollectionKind::ChildNodes => dok.kinder(root).to_vec(),
            CollectionKind::TagName(q, html_dok) => {
                let html_dok = *html_dok;
                let klein = q.to_ascii_lowercase();
                element_nachfahren(dok, root)
                    .into_iter()
                    .filter(|&e| {
                        if q == "*" {
                            return true;
                        }
                        let qn = element_qname(dok, e);
                        let ist_html = matches!(dok.knoten.get(e).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. }));
                        if html_dok && ist_html { qn == klein } else { qn == *q }
                    })
                    .collect()
            }
            CollectionKind::TagNameNs(ns, lokal) => element_nachfahren(dok, root)
                .into_iter()
                .filter(|&e| {
                    let ns_ok = ns.as_deref() == Some("*") || element_ns(dok, e) == *ns;
                    ns_ok && (lokal == "*" || dok.element_name(e) == Some(lokal.as_str()))
                })
                .collect(),
            CollectionKind::ClassNames(roh) => {
                // DOM §4.4 "list of elements with class names": every
                // element whose classes contain all of them (no quirks
                // folding, as before).
                let mut gesucht: Vec<&str> = Vec::new();
                for c in roh.split_ascii_whitespace() {
                    if !gesucht.contains(&c) {
                        gesucht.push(c);
                    }
                }
                if gesucht.is_empty() {
                    return Vec::new();
                }
                element_nachfahren(dok, root)
                    .into_iter()
                    .filter(|&e| {
                        let Some(klassen) = dok.attribut(e, "class") else { return false };
                        gesucht.iter().all(|g| klassen.split_ascii_whitespace().any(|c| c == *g))
                    })
                    .collect()
            }
            // Release 2026-10-15: only HTML elements (HTML §3.1.3, e.g.
            // "all img elements"); `applets` is always empty.
            CollectionKind::Document("applets") => Vec::new(),
            CollectionKind::Document(welche) => {
                let sel = match *welche {
                    "scripts" => "script",
                    "images" => "img",
                    "forms" => "form",
                    "links" => "a[href], area[href]",
                    "anchors" => "a[name]",
                    _ => "embed",
                };
                suchen(None, dok, root, sel, false, &narsil_css::Zustand::default(), KEIN_SCOPE)
                    .into_iter()
                    .filter(|&e| ist_html(dok, e))
                    .collect()
            }
            CollectionKind::Name(v) => element_nachfahren(dok, root)
                .into_iter()
                .filter(|&e| ist_html(dok, e) && dok.attribut(e, "name") == Some(v.as_str()))
                .collect(),
            CollectionKind::FormElements => form_felder(self, root, false),
            CollectionKind::FormNamed(n) => form_felder(self, root, false)
                .into_iter()
                .filter(|&e| dok.attribut(e, "id") == Some(n.as_str()) || dok.attribut(e, "name") == Some(n.as_str()))
                .collect(),
            CollectionKind::FieldsetElements => element_nachfahren(dok, root)
                .into_iter()
                .filter(|&e| {
                    ist_html(dok, e)
                        && matches!(dok.element_name(e), Some("button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea"))
                })
                .collect(),
            CollectionKind::TableRows => {
                let html_kinder = |p: usize, tag: &str| -> Vec<usize> {
                    dok.kinder(p).iter().copied().filter(|&c| ist_html(dok, c) && dok.element_name(c) == Some(tag)).collect()
                };
                let mut kopf = Vec::new();
                let mut mitte = Vec::new();
                let mut fuss = Vec::new();
                for &c in dok.kinder(root) {
                    if !ist_html(dok, c) {
                        continue;
                    }
                    match dok.element_name(c) {
                        Some("thead") => kopf.extend(html_kinder(c, "tr")),
                        Some("tbody") => mitte.extend(html_kinder(c, "tr")),
                        Some("tr") => mitte.push(c),
                        Some("tfoot") => fuss.extend(html_kinder(c, "tr")),
                        _ => {}
                    }
                }
                kopf.extend(mitte);
                kopf.extend(fuss);
                kopf
            }
            CollectionKind::TableBodies => dok.kinder(root).iter().copied().filter(|&c| ist_html(dok, c) && dok.element_name(c) == Some("tbody")).collect(),
            CollectionKind::SectionRows => dok.kinder(root).iter().copied().filter(|&c| ist_html(dok, c) && dok.element_name(c) == Some("tr")).collect(),
            CollectionKind::Cells => dok
                .kinder(root)
                .iter()
                .copied()
                .filter(|&c| ist_html(dok, c) && matches!(dok.element_name(c), Some("td" | "th")))
                .collect(),
            CollectionKind::MapAreas => element_nachfahren(dok, root).into_iter().filter(|&e| ist_html(dok, e) && dok.element_name(e) == Some("area")).collect(),
            CollectionKind::DatalistOptions => element_nachfahren(dok, root).into_iter().filter(|&e| ist_html(dok, e) && dok.element_name(e) == Some("option")).collect(),
            CollectionKind::DocumentNamed(n) => element_nachfahren(dok, root).into_iter().filter(|&e| benannt_passt(dok, e, n)).collect(),
        }
    }

    /// Re-evaluates collection `id` if the document changed since; `false`
    /// if there is no such collection.
    fn collection_refresh(&mut self, id: u32) -> bool {
        let tree = self.dok.tree_version.0;
        let attr = self.dok.attr_version.0;
        let Some(c) = self.collections.get(&id) else { return false };
        let stand = (tree, if c.kind.reads_attributes() { attr } else { 0 });
        if c.evaluated == Some(stand) {
            return true;
        }
        let (root, kind, alt) = (c.root, c.kind.clone(), c.items.len());
        let items = self.collection_items(root, &kind);
        self.cached_items = self.cached_items.saturating_sub(alt);
        if self.cached_items + items.len() > MAX_CACHED_ITEMS {
            for c in self.collections.values_mut() {
                c.items = Vec::new();
                c.evaluated = None;
            }
            self.cached_items = 0;
        }
        self.cached_items += items.len();
        if let Some(c) = self.collections.get_mut(&id) {
            c.items = items;
            c.evaluated = Some(stand);
        }
        true
    }

    /// Release 2026-10-15: whether `k`'s node document is an XML document
    /// (DOM "is an HTML document" is false for it). Detached nodes ask their
    /// owner document. MEASURED before (WPT dom/nodes/Document-createElement):
    /// `xhtmlDoc.createElement("foo").tagName` was "FOO" — the detached
    /// element fell back to the main document.
    pub(crate) fn in_xml_document(&self, k: usize) -> bool {
        !self.dok.xml_typ.is_empty() && self.dok.xml_typ.contains_key(&self.dokument_von(k))
    }
}

/// Release 11.10.2026: die „checkedness" eines `<input>` — gesetzt (Bit 8),
/// ausdruecklich aus (Bit 16) oder nach dem Attribut `checked`.
pub(crate) fn ist_angehakt(dok: &Dokument, k: usize) -> bool {
    let z = dok.css_zustand_von(k);
    z & 8 != 0 || (z & 16 == 0 && dok.attribut(k, "checked").is_some())
}

/// Release 11.10.2026: den Haken eines (angehakten) Felds neu setzen — fuer
/// den Typwechsel zum Radio (die Gruppe wird abgehakt).
pub(crate) fn haken_neu_setzen(l: &mut Lebend, k: usize) {
    haken_setzen(l, k, true);
}

/// Setzt die „checkedness" (dirty); ein angehaktes Radio hakt seine Gruppe ab
/// (HTML §4.10.5.1.18: gleicher Name, gleicher Formular-Eigentuemer, gleicher Baum).
fn haken_setzen(l: &mut Lebend, k: usize, an: bool) {
    let z = l.dok.css_zustand_von(k) & !(8 | 16);
    l.dok.css_zustand_setzen(k, z | if an { 8 } else { 16 });
    l.geaendert = true;
    let ist_radio = l.dok.attribut(k, "type").is_some_and(|t| t.trim().eq_ignore_ascii_case("radio"));
    let name = l.dok.attribut(k, "name").unwrap_or("").to_string();
    if !an || !ist_radio || name.is_empty() {
        return;
    }
    let eigner = |dok: &Dokument, e: usize| -> Option<String> {
        if let Some(f) = dok.attribut(e, "form") {
            return Some(format!("#{f}"));
        }
        let mut p = dok.eltern(e);
        while let Some(x) = p {
            if dok.element_name(x) == Some("form") {
                return Some(format!("@{x}"));
            }
            p = dok.eltern(x);
        }
        None
    };
    let mein = eigner(&l.dok, k);
    let wurzel = l.dok.wurzel_von(k);
    let mut stapel = vec![wurzel];
    let mut andere = Vec::new();
    while let Some(x) = stapel.pop() {
        if andere.len() > 10_000 {
            break;
        }
        if x != k
            && l.dok.element_name(x) == Some("input")
            && l.dok.attribut(x, "type").is_some_and(|t| t.trim().eq_ignore_ascii_case("radio"))
            && l.dok.attribut(x, "name") == Some(name.as_str())
            && eigner(&l.dok, x) == mein
        {
            andere.push(x);
        }
        stapel.extend(l.dok.kinder(x).iter().copied());
    }
    for o in andere {
        let z = l.dok.css_zustand_von(o) & !(8 | 16);
        l.dok.css_zustand_setzen(o, z | 16);
    }
}

fn knoten_name(dok: &Dokument, k: usize, html_document: bool) -> String {
    // Release 11.10.2026: der qualifizierte Name (`praefix:lokal`, DOM §4.9);
    // CDATA-Abschnitte heissen `#cdata-section`.
    let qualifiziert = |name: &String| match dok.fremd_ns.get(&k).and_then(|(_, p)| p.as_ref()) {
        Some(p) => format!("{p}:{name}"),
        None => name.clone(),
    };
    match dok.knoten.get(k).map(|n| &n.art) {
        Some(Art::Element { raum: Namensraum::Html, name, .. }) if html_document => qualifiziert(name).to_ascii_uppercase(),
        Some(Art::Element { name, .. }) => qualifiziert(name),
        Some(Art::Text(_)) if dok.cdata.contains(&k) => "#cdata-section".into(),
        Some(Art::Text(_)) => "#text".into(),
        Some(Art::Kommentar(_)) => "#comment".into(),
        Some(Art::Dokument) => "#document".into(),
        Some(Art::Doctype { name, .. }) => name.clone(),
        Some(Art::Fragment) | Some(Art::ShadowRoot { .. }) => "#document-fragment".into(),
        Some(Art::Pi { target, .. }) => target.clone(),
        None => String::new(),
    }
}

/// Die Eltern im DOM-Sinn: die Shadow Root hat KEINE Eltern (ihr Wirt ist
/// `host`), ihre Kinder haben die Shadow Root als Eltern.
fn eltern_dom(dok: &Dokument, k: usize) -> Option<usize> {
    if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::ShadowRoot { .. })) {
        return None;
    }
    dok.eltern(k)
}

fn element_kinder(dok: &Dokument, k: usize) -> Vec<usize> {
    dok.kinder(k).iter().copied().filter(|&c| dok.ist_element_knoten(c)).collect()
}

fn naechstes_element(dok: &Dokument, k: usize, vor: bool) -> Option<usize> {
    let mut x = k;
    loop {
        x = if vor { dok.voriges_geschwister(x)? } else { dok.naechstes_geschwister(x)? };
        if dok.ist_element_knoten(x) {
            return Some(x);
        }
    }
}

/// Ein Getter. `None` = das lebende DOM kennt diese Eigenschaft nicht (der
/// Proxy-Stand antwortet weiter).
pub fn lesen(l: &mut Lebend, id: u32, name: &str) -> Option<Wert> {
    let k = l.knoten(id)?;
    // Release 13.10.2026: `form[i]` und `form.name` (HTML §4.10.3,
    // [LegacyOverrideBuiltIns]: der Name eines Felds verdeckt sogar
    // `form.submit`). GEMESSEN vorher (WPT form-nameditem, form-indexed-
    // element): `form.button`/`form[0]` waren undefined.
    if !name.is_empty() && !name.starts_with("__") && l.dok.element_name(k) == Some("form")
        && matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. }))
        && let Some(w) = form_benannt(l, k, name)
    {
        return Some(w);
    }
    let ist_dokument = matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument));
    let dok = &l.dok;
    let wert = match name {
        "nodeType" => Wert::Zahl(dok.knoten_typ(k) as f64),
        "nodeName" => Wert::Text(knoten_name(dok, k, !l.in_xml_document(k))),
        "tagName" if dok.ist_element_knoten(k) => Wert::Text(knoten_name(dok, k, !l.in_xml_document(k))),
        "localName" if dok.ist_element_knoten(k) => Wert::Text(dok.element_name(k).unwrap_or("").to_string()),
        // Release 11.10.2026: Elemente ausserhalb von HTML/SVG/MathML
        // (DOM §4.9). GEMESSEN vorher: `createElementNS('urn:x', 'p:Foo')`
        // war ein HTML-Element `FOO` ohne Praefix.
        "namespaceURI" if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Fremd, .. })) => {
            match dok.fremd_ns.get(&k) {
                Some((uri, _)) if !uri.is_empty() => Wert::Text(uri.clone()),
                _ => Wert::Null,
            }
        }
        "prefix" if dok.ist_element_knoten(k) => dok.fremd_ns.get(&k).and_then(|(_, p)| p.clone()).map_or(Wert::Null, Wert::Text),
        "__narsilXml" if ist_dokument => dok.xml_typ.get(&k).cloned().map_or(Wert::Null, Wert::Text),
        // Release 09.10.2026: SVG/MathML aus dem PARSER tragen ihren
        // Namensraum (vorher nur `createElementNS`, `dom.rs` `namespaces`).
        // GEMESSEN: `<svg id=s>` -> `s.namespaceURI` war die XHTML-Adresse,
        // `s instanceof SVGSVGElement` damit `false` (Chrome: SVG, `true`).
        "namespaceURI" if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Svg | Namensraum::MathMl, .. })) => {
            match dok.knoten.get(k).map(|n| &n.art) {
                Some(Art::Element { raum: Namensraum::Svg, .. }) => Wert::Text("http://www.w3.org/2000/svg".to_string()),
                _ => Wert::Text("http://www.w3.org/1998/Math/MathML".to_string()),
            }
        }
        // Release 10.10.2026: die Sammlungen des Dokuments (HTML §3.1.3:
        // `scripts`, `images`, `forms`, `links`, `anchors`, `embeds`/`plugins`)
        // in Baumordnung. GEMESSEN vorher: `undefined` — bbc.com
        // `Array.prototype.slice.call(document.scripts)` warf TypeError.
        "scripts" | "images" | "forms" | "links" | "anchors" | "embeds" | "plugins" | "applets" if ist_dokument => {
            let sel = match name {
                "scripts" => "script",
                "images" => "img",
                "forms" => "form",
                "links" => "a[href], area[href]",
                "anchors" => "a[name]",
                _ => "embed",
            };
            // Release 2026-10-15: live (HTML §3.1.3, HTMLCollection).
            let welche: &'static str = match name {
                "scripts" => "scripts",
                "images" => "images",
                "forms" => "forms",
                "links" => "links",
                "anchors" => "anchors",
                "applets" => "applets",
                _ => "embeds",
            };
            if let Some(w) = l.collection(k, CollectionKind::Document(welche)) {
                return Some(w);
            }
            if name == "applets" {
                return Some(Wert::Feld(Vec::new()));
            }
            let treffer = suchen(Some(l), &l.dok, k, sel, false, &narsil_css::Zustand::default(), KEIN_SCOPE);
            Wert::Feld(treffer.into_iter().map(|t| l.wert(Some(t))).collect())
        }
        "parentNode" => {
            let e = eltern_dom(dok, k);
            l.wert(e)
        }
        "parentElement" => {
            let e = eltern_dom(dok, k).filter(|&e| l.dok.ist_element_knoten(e));
            l.wert(e)
        }
        // Release 2026-10-15: live collections (DOM §4.4, §4.2.6). MEASURED
        // before (WPT dom/collections): `el.children !== el.children`, and a
        // removed child stayed in a collection read before.
        // Release 2026-10-15: the live element collections the prelude's
        // getters hand out (`table.rows`, `form.elements`, …).
        _ if name.starts_with("__narsilLive:") => {
            let kind = element_collection_kind(dok, k, &name["__narsilLive:".len()..])?;
            return l.collection(k, kind);
        }
        "childNodes" => {
            if let Some(w) = l.collection(k, CollectionKind::ChildNodes) {
                return Some(w);
            }
            let kinder = l.dok.kinder(k).to_vec();
            Wert::Feld(kinder.into_iter().map(|c| l.wert(Some(c))).collect())
        }
        "children" => {
            if let Some(w) = l.collection(k, CollectionKind::Children) {
                return Some(w);
            }
            let kinder = element_kinder(&l.dok, k);
            Wert::Feld(kinder.into_iter().map(|c| l.wert(Some(c))).collect())
        }
        "firstChild" => {
            let c = dok.kinder(k).first().copied();
            l.wert(c)
        }
        "lastChild" => {
            let c = dok.kinder(k).last().copied();
            l.wert(c)
        }
        "nextSibling" => {
            let c = if eltern_dom(dok, k).is_some() { dok.naechstes_geschwister(k) } else { None };
            l.wert(c)
        }
        "previousSibling" => {
            let c = if eltern_dom(dok, k).is_some() { dok.voriges_geschwister(k) } else { None };
            l.wert(c)
        }
        "firstElementChild" => {
            let c = element_kinder(dok, k).first().copied();
            l.wert(c)
        }
        "lastElementChild" => {
            let c = element_kinder(dok, k).last().copied();
            l.wert(c)
        }
        "nextElementSibling" => {
            let c = naechstes_element(dok, k, false);
            l.wert(c)
        }
        "previousElementSibling" => {
            let c = naechstes_element(dok, k, true);
            l.wert(c)
        }
        "childElementCount" => Wert::Zahl(element_kinder(dok, k).len() as f64),
        "textContent" | "innerText" if ist_dokument => Wert::Null,
        // Release 12.10.2026: in einem `<style>` mit eingesetzten `@import`-
        // Blaettern (`blaetter::import_ersetzen`) ist nur das letzte Stueck der
        // Text der Seite.
        "textContent" | "innerText" => {
            let t = dok.text_inhalt(k);
            Wert::Text(match t.rfind(narsil_css::BLATT_GRENZE) {
                Some(i) => t[i + narsil_css::BLATT_GRENZE.len()..].to_string(),
                None => t,
            })
        }
        "nodeValue" | "data" => match dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::Text(t)) | Some(Art::Kommentar(t)) => Wert::Text(t.clone()),
            Some(Art::Pi { daten, .. }) => Wert::Text(daten.clone()),
            _ if name == "data" => return None,
            _ => Wert::Null,
        },
        "length" => match dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::Text(t)) | Some(Art::Kommentar(t)) => Wert::Zahl(t.encode_utf16().count() as f64),
            Some(Art::Pi { daten, .. }) => Wert::Zahl(daten.encode_utf16().count() as f64),
            _ => return None,
        },
        // Release 11.10.2026: DOM §4.6 `DocumentType`, §4.13 `ProcessingInstruction`.
        "target" if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Pi { .. })) => match dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::Pi { target, .. }) => Wert::Text(target.clone()),
            _ => Wert::Null,
        },
        "name" | "publicId" | "systemId" if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Doctype { .. })) => {
            match dok.knoten.get(k).map(|n| &n.art) {
                Some(Art::Doctype { name: n, public_id, system_id }) => Wert::Text(match name {
                    "name" => n.clone(),
                    "publicId" => public_id.clone(),
                    _ => system_id.clone(),
                }),
                _ => Wert::Null,
            }
        }
        "innerHTML" if !ist_dokument => Wert::Text(dok.inner_html(k)),
        "outerHTML" if dok.ist_element_knoten(k) => Wert::Text(dok.outer_html(k)),
        "isConnected" => Wert::Wahr(l.in_einem_dokument(k)),
        // Release 11.10.2026: HTML §4.10.5 „checkedness" (mit dirty-Flag in
        // den Zustandsbits) und `indeterminate`. GEMESSEN vorher (WPT
        // the-input-element/cloning-steps 0/68): `indeterminate` fehlte,
        // `checked` stand in einer Nebenablage, die `cloneNode` nicht kannte.
        "checked" if dok.element_name(k) == Some("input") => Wert::Wahr(ist_angehakt(dok, k)),
        "indeterminate" if dok.element_name(k) == Some("input") => Wert::Wahr(dok.css_zustand_von(k) & 32 != 0),
        "ownerDocument" if ist_dokument => Wert::Null,
        "ownerDocument" => l.wert(Some(l.dokument_von(k))),
        "content" => match dok.template_inhalt(k) {
            Some(c) => l.wert(Some(c)),
            None => return None,
        },
        "shadowRoot" if dok.ist_element_knoten(k) => {
            let s = dok.shadow_wurzel(k);
            l.wert(s)
        }
        "host" => match dok.knoten.get(k).map(|n| (&n.art, n.eltern)) {
            Some((Art::ShadowRoot { .. }, Some(wirt))) => l.wert(Some(wirt)),
            _ => return None,
        },
        // Release 12.10.2026: `ShadowRoot.mode` aus der Arena (auch fuer
        // deklarative Wurzeln aus dem Parser). GEMESSEN vorher: `undefined`.
        "clonable" | "serializable" | "delegatesFocus" if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::ShadowRoot { .. })) => {
            let bit = match name {
                "clonable" => 1,
                "serializable" => 2,
                _ => 4,
            };
            Wert::Wahr(l.schatten_merkmale.get(&k).is_some_and(|b| b & bit != 0))
        }
        // Release 13.10.2026: `ShadowRoot.slotAssignment` ("named"/"manual").
        "slotAssignment" if matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::ShadowRoot { .. })) => {
            let manuell = l.schatten_merkmale.get(&k).is_some_and(|b| b & narsil_fmt_html::dom::SCHATTEN_MANUELL != 0);
            Wert::Text(if manuell { "manual" } else { "named" }.to_string())
        }
        // Release 12.10.2026: die Shadow Root auch im Modus „closed" (nur fuer
        // `vorspann.js`: `getHTML`, HTML §13.3 „serializes as void").
        "__narsilSchattenIntern" if dok.ist_element_knoten(k) => {
            let s = dok.shadow_wurzel_intern(k);
            l.wert(s)
        }
        "mode" => match dok.knoten.get(k).map(|n| &n.art) {
            Some(Art::ShadowRoot { mode, .. }) => Wert::Text(
                if matches!(mode, narsil_fmt_html::dom::ShadowRootMode::Closed) { "closed" } else { "open" }.to_string(),
            ),
            _ => return None,
        },
        "__ceIs" if dok.ist_element_knoten(k) => l.ce_is.get(&k).and_then(|v| v.clone())
            .or_else(|| (!l.ce_is.contains_key(&k)).then(|| dok.attribut(k, "is").map(str::to_string)).flatten())
            .map_or(Wert::Null, Wert::Text),
        // Release 12.10.2026: die Rohliste; `el.attributes` baut der Vorspann
        // daraus (Attr-Knoten, NamedNodeMap). GEMESSEN vorher: `attributes[i]`
        // war ein loses Objekt, `attributes[i].value = …` wirkte nicht.
        "__narsilAttributeRoh" if dok.ist_element_knoten(k) => Wert::Feld(
            l.attribute_von(k)
                .into_iter()
                .map(|(n, v, ns)| {
                    Wert::Gegenstand(vec![
                        ("name".into(), Wert::Text(n.clone())),
                        ("localName".into(), Wert::Text(if ns.is_some() { n.split_once(':').map_or(n.as_str(), |(_, l)| l).to_string() } else { n.clone() })),
                        ("namespaceURI".into(), ns.map_or(Wert::Null, Wert::Text)),
                        ("value".into(), Wert::Text(v)),
                    ])
                })
                .collect(),
        ),
        // `document.implementation` IST hier das Dokument selbst: seine
        // Methoden (`createHTMLDocument`, `hasFeature`) stehen am Dokument.
        "implementation" if ist_dokument => crate::dom::host_wert(id),
        "title" if ist_dokument => {
            let t = dokument_titel(dok, k).map(|t| dok.text_inhalt(t)).unwrap_or_default();
            Wert::Text(t.split_ascii_whitespace().collect::<Vec<_>>().join(" "))
        }
        "hidden" if dok.ist_element_knoten(k) => Wert::Wahr(dok.attribut(k, "hidden").is_some()),
        // Release 10.10.2026: `type` (siehe `typ_lesen`); `select.type` ist
        // `select-one`/`select-multiple`.
        "type" if dok.ist_element_knoten(k) => {
            let tag = dok.element_name(k).unwrap_or("");
            if tag == "select" {
                Wert::Text(if dok.attribut(k, "multiple").is_some() { "select-multiple" } else { "select-one" }.to_string())
            } else if tag == "button"
                && (dok.attribut(k, "commandfor").is_some() || dok.attribut(k, "command").is_some())
                && !dok.attribut(k, "type").is_some_and(|t| ["submit", "reset", "button"].iter().any(|x| t.eq_ignore_ascii_case(x)))
            {
                // Release 2026-10-15: the Auto state of a button with
                // `commandfor`/`command` is the Button state (HTML §4.10.6).
                Wert::Text("button".to_string())
            } else {
                Wert::Text(typ_lesen(tag, dok.attribut(k, "type")))
            }
        }
        // Release 12.10.2026: `style.disabled` spiegelt kein Attribut, sondern
        // das Blatt (HTML §4.2.6) — das macht der Vorspann.
        "disabled" if dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style")) => return None,
        _ if dok.ist_element_knoten(k) && SPIEGEL_BOOL.iter().any(|(e, _)| *e == name) => {
            let attr = SPIEGEL_BOOL.iter().find(|(e, _)| *e == name).map_or(name, |(_, a)| *a);
            Wert::Wahr(dok.attribut(k, attr).is_some())
        }
        _ if dok.ist_element_knoten(k) && SPIEGEL_TEXT.iter().any(|(e, _)| *e == name) => {
            if !reflection_applies(dok, k, name) {
                return None;
            }
            let attr = SPIEGEL_TEXT.iter().find(|(e, _)| *e == name).map_or(name, |(_, a)| *a);
            Wert::Text(dok.attribut(k, attr).unwrap_or("").to_string())
        }
        // Release 11.10.2026: `document.doctype` (DOM §4.5): der erste
        // Doctype-Knoten unter dem Dokument, sonst `null`.
        "doctype" if ist_dokument => {
            let d = dok.kinder(k).iter().copied().find(|&c| matches!(dok.knoten.get(c).map(|n| &n.art), Some(Art::Doctype { .. })));
            l.wert(d)
        }
        "documentElement" if ist_dokument => {
            let e = element_kinder(dok, k).first().copied();
            l.wert(e)
        }
        // Release 2026-10-15: HTML §3.1.3: the html element is the document
        // element when it is an HTML `html`; the body is its first HTML
        // `body` or `frameset` child, the head its first HTML `head`.
        // MEASURED before (WPT Document.body): a frameset before the body
        // was skipped, non-HTML `html`/`body` elements counted.
        // Release 2026-10-15: the document's own property names for
        // `Object.getOwnPropertyNames(document)` (named elements).
        "__narsilOwnKeys" if ist_dokument => Wert::Feld(dokument_namen(dok, k).into_iter().map(Wert::Text).collect()),
        "body" | "head" if ist_dokument => {
            let html = element_kinder(dok, k)
                .first()
                .copied()
                .filter(|&e| ist_html(dok, e) && l.dok.element_name(e) == Some("html"));
            let ziel = html.and_then(|h| {
                element_kinder(&l.dok, h).into_iter().find(|&e| {
                    ist_html(&l.dok, e)
                        && match name {
                            "body" => matches!(l.dok.element_name(e), Some("body" | "frameset")),
                            _ => l.dok.element_name(e) == Some("head"),
                        }
                })
            });
            l.wert(ziel)
        }
        _ => return None,
    };
    Some(wert)
}

/// Ein Setter. `None` = nicht zustaendig.
pub fn schreiben(l: &mut Lebend, id: u32, name: &str, wert: &Wert) -> Option<Result<(), String>> {
    let k = l.knoten(id)?;
    if matches!(name, "checked" | "indeterminate") && l.dok.element_name(k) == Some("input") {
        let an = match wert {
            Wert::Wahr(b) => *b,
            Wert::Zahl(z) => *z != 0.0 && !z.is_nan(),
            Wert::Text(t) => !t.is_empty(),
            Wert::Null | Wert::Undefiniert => false,
            _ => true,
        };
        if name == "checked" {
            haken_setzen(l, k, an);
        } else {
            let z = l.dok.css_zustand_von(k) & !32;
            l.dok.css_zustand_setzen(k, z | if an { 32 } else { 0 });
            l.geaendert = true;
        }
        return Some(Ok(()));
    }
    if !l.ce_aktiv() {
        // Release 2026-10-15: `details.open = …` reports only a change (no
        // `toggle` for open = false on a closed details).
        let offen_vorher = (name == "open").then(|| l.dok.attribut(k, "open").is_some());
        let r = schreiben_intern(l, k, name, wert);
        if matches!(r, Some(Ok(()))) && offen_vorher.is_none_or(|v| v != l.dok.attribut(k, "open").is_some()) {
            l.medien_nach_schreiben(k, name);
        }
        return r;
    }
    // Custom Elements: wer vorher verbunden war und wer es jetzt ist.
    let (vorher_knoten, eltern) = match name {
        "textContent" | "innerText" | "innerHTML" => (l.dok.kinder(k).to_vec(), None),
        "outerHTML" => (vec![k], l.dok.eltern(k)),
        _ => (Vec::new(), None),
    };
    let alte_kinder: Vec<usize> = eltern.map(|e| l.dok.kinder(e).to_vec()).unwrap_or_default();
    let attr = SPIEGEL_TEXT.iter().chain(SPIEGEL_BOOL.iter()).find(|(e, _)| *e == name).map(|(_, a)| *a)
        .or((name == "hidden").then_some("hidden"))
        .or(matches!(name, "src" | "href" | "action" | "poster" | "type").then_some(name));
    let alt_attr = attr.and_then(|a| l.dok.attribut(k, a).map(str::to_string));
    let vorher = l.ce_verbundene(&vorher_knoten);
    let r = schreiben_intern(l, k, name, wert);
    let mut nachher_knoten = match name {
        "textContent" | "innerText" | "innerHTML" => l.dok.kinder(k).to_vec(),
        _ => Vec::new(),
    };
    if let Some(e) = eltern {
        nachher_knoten.extend(l.dok.kinder(e).iter().copied().filter(|c| !alte_kinder.contains(c)));
    }
    if !vorher.is_empty() || !nachher_knoten.is_empty() {
        l.ce_nach(vorher, &nachher_knoten);
    }
    if matches!(&r, Some(Ok(()))) {
        if let Some(a) = attr {
            let neu = l.dok.attribut(k, a).map(str::to_string);
            if !SPIEGEL_BOOL.iter().any(|(e, _)| *e == name) && name != "hidden" || alt_attr != neu {
                l.ce_attribut(k, a, alt_attr);
            }
            if matches!(a, "disabled" | "form" | "id") { l.ce_formular(k); }
            // Release 12.10.2026: `img.srcset = …` stoesst die Bildwahl an.
            if a == "srcset" { l.bild_quelle_vermerken(k); }
        }
    }
    r
}

fn schreiben_intern(l: &mut Lebend, k: usize, name: &str, wert: &Wert) -> Option<Result<(), String>> {
    let ist_dokument = matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument));
    match name {
        "textContent" | "innerText" => {
            if k != l.wurzel {
                let t = match wert {
                    Wert::Null | Wert::Undefiniert => String::new(),
                    w => w.als_text(),
                };
                l.dok.text_inhalt_setzen(k, &t);
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        "nodeValue" | "data" => {
            if matches!(l.dok.knoten[k].art, Art::Text(_) | Art::Kommentar(_) | Art::Pi { .. }) {
                // Release 2026-10-15: `nodeValue` is `DOMString?` and `data`
                // [LegacyNullToEmptyString] — null sets "" (DOM §4.4,
                // §4.10). MEASURED before (WPT Node-nodeValue): "null".
                let text = if matches!(wert, Wert::Null) { String::new() } else { wert.als_text() };
                l.dok.text_inhalt_setzen(k, &text);
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        "innerHTML" => {
            // Release 13.10.2026: `[LegacyNullToEmptyString]` (DOM Parsing
            // §2.2). GEMESSEN vorher (WPT innerhtml-07): `null` ergab "null".
            let html = if matches!(wert, Wert::Null) { String::new() } else { wert.als_text() };
            if html.len() > MAX_HTML {
                return Some(Err(format!("innerHTML: {} Byte (erlaubt {MAX_HTML})", html.len())));
            }
            if l.dok.inner_html_setzen(k, &html) {
                l.geaendert = true;
                if html.contains("script") || html.contains("SCRIPT") {
                    l.skripte_markieren(k);
                }
            }
            Some(Ok(()))
        }
        "outerHTML" => {
            let Some(e) = l.dok.eltern(k) else { return Some(Ok(())) };
            // Release 13.10.2026: DOM Parsing §2.3 — unter dem Dokument
            // NoModificationAllowedError, `null` ist "", unter einem
            // Fragment ist `body` der Kontext. GEMESSEN vorher (WPT
            // outerhtml-01/-02): kein Fehler, `null` ergab "null".
            if matches!(l.dok.knoten.get(e).map(|n| &n.art), Some(Art::Dokument)) {
                return Some(Err("NoModificationAllowedError: outerHTML eines Kindes des Dokuments".into()));
            }
            let html = if matches!(wert, Wert::Null) { String::new() } else { wert.als_text() };
            if html.len() > MAX_HTML {
                return Some(Err("outerHTML: HTML zu lang".into()));
            }
            // Hilfsfragment im Kontext des Elternelements.
            let kontext = l.dok.element_name(e).unwrap_or("body").to_string();
            let Some(hilf) = l.dok.neues_element(&kontext) else {
                return Some(Ok(()));
            };
            if l.dok.inner_html_setzen(hilf, &html) {
                l.skripte_markieren(hilf);
                let neue = l.dok.kinder(hilf).to_vec();
                // Release 12.10.2026: EIN Datensatz (DOM Parsing: „replace").
                let davor = l.dok.voriges_geschwister(k);
                let danach = l.dok.naechstes_geschwister(k);
                let gesammelt = l.dok.sammeln_an(e, davor, danach, false);
                for n in neue {
                    l.dok.kind_einfuegen(e, n, Some(k));
                }
                l.dok.sammeln_weg(true);
                l.dok.abtrennen(k);
                if gesammelt {
                    l.dok.sammeln_aus();
                }
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        "title" if ist_dokument => {
            let text = wert.als_text();
            let titel = match dokument_titel(&l.dok, k) {
                Some(t) => Some(t),
                None => {
                    let kopf = rufen_intern(l, k, "querySelector", &[Wert::Text("head".into())], &narsil_css::Zustand::KEINER)
                        .and_then(|r| r.ok())
                        .and_then(|w| knoten_aus(l, Some(&w)));
                    match (kopf, l.dok.neues_element("title")) {
                        (Some(kopf), Some(t)) => {
                            l.dok.kind_anhaengen(kopf, t);
                            Some(t)
                        }
                        _ => None,
                    }
                }
            };
            if let Some(t) = titel {
                l.dok.text_inhalt_setzen(t, &text);
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        "hidden" if l.dok.ist_element_knoten(k) => {
            let an = match wert {
                Wert::Wahr(b) => *b,
                Wert::Null | Wert::Undefiniert => false,
                Wert::Zahl(n) => *n != 0.0 && !n.is_nan(),
                Wert::Text(t) => !t.is_empty(),
                _ => true,
            };
            let hat = l.dok.attribut(k, "hidden").is_some();
            if an && !hat {
                l.dok.setze_attribut(k, Namensraum::Html, "hidden", "");
                l.geaendert = true;
            } else if !an && hat {
                l.dok.attribut_entfernen(k, "hidden");
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        // Release 09.10.2026: URL-Attribute (Gegenstueck zum Lesen in
        // `dom.rs`: dort absolut, hier wird der Text so gesetzt, wie er kam).
        "src" | "href" | "action" | "poster"
            if l.dok.element_name(k).is_some_and(|t| match name {
                "src" => matches!(
                    t,
                    "img" | "script" | "iframe" | "audio" | "video" | "source" | "embed" | "input" | "track" | "frame"
                ),
                "href" => matches!(t, "a" | "area" | "link" | "base"),
                "action" => t == "form",
                _ => t == "video",
            }) =>
        {
            // Release 13.10.2026: auch ein GLEICHER Wert setzt das Attribut
            // (WebIDL [Reflect]: „set the content attribute") — ein Rahmen
            // laedt dann neu (HTML §4.8.5). GEMESSEN vorher (WPT webstorage
            // event_*: `iframe.src = gleicheAdresse`): kein Mutationssatz,
            // kein Neuladen, der zweite Testteil lief in die Frist.
            let text = wert.als_text();
            l.dok.setze_attribut(k, Namensraum::Html, name, &text);
            l.geaendert = true;
            if name == "src" {
                l.bild_quelle_vermerken(k);
            }
            Some(Ok(()))
        }
        // Release 10.10.2026: gespiegelte Wahrheitswerte setzen/entfernen
        // ihr Attribut, `type` setzt das Attribut.
        "disabled" if l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("style")) => return None,
        _ if l.dok.ist_element_knoten(k) && SPIEGEL_BOOL.iter().any(|(e, _)| *e == name) => {
            let attr = SPIEGEL_BOOL.iter().find(|(e, _)| *e == name).map_or(name, |(_, a)| *a);
            let an = match wert {
                Wert::Wahr(b) => *b,
                Wert::Null | Wert::Undefiniert => false,
                Wert::Zahl(n) => *n != 0.0 && !n.is_nan(),
                Wert::Text(t) => !t.is_empty(),
                _ => true,
            };
            let hat = l.dok.attribut(k, attr).is_some();
            // Release 14.10.2026: `link.disabled = false` while the attribute
            // is present enables the sheet explicitly, also an alternate one
            // (HTML §4.6.7; WPT cssom HTMLLinkElement-disabled-002…007).
            if name == "disabled" && l.dok.element_name(k).is_some_and(|n| n.eq_ignore_ascii_case("link")) {
                if an {
                    l.enabled_links.remove(&k);
                } else if hat {
                    l.enabled_links.insert(k);
                }
            }
            if an && !hat {
                l.dok.setze_attribut(k, Namensraum::Html, attr, "");
                l.geaendert = true;
            } else if !an && hat {
                l.dok.attribut_entfernen(k, attr);
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        "type" if l.dok.ist_element_knoten(k) => {
            let text = wert.als_text();
            if l.dok.attribut(k, "type") != Some(text.as_str()) {
                l.dok.setze_attribut(k, Namensraum::Html, "type", &text);
                l.geaendert = true;
            }
            Some(Ok(()))
        }
        _ if l.dok.ist_element_knoten(k) && SPIEGEL_TEXT.iter().any(|(e, _)| *e == name) => {
            if !reflection_applies(&l.dok, k, name) {
                return None;
            }
            let attr = SPIEGEL_TEXT.iter().find(|(e, _)| *e == name).map_or(name, |(_, a)| *a);
            let text = match wert {
                Wert::Null if name == "id" || name == "className" => "null".to_string(),
                w => w.als_text(),
            };
            if l.dok.attribut(k, attr) != Some(text.as_str()) {
                l.dok.setze_attribut(k, Namensraum::Html, attr, &text);
                l.geaendert = true;
            } else {
                // Release 12.10.2026: Setzen auf denselben Wert ist trotzdem
                // „change an attribute" (DOM §4.9) — ein Datensatz.
                l.dok.protokoll(narsil_fmt_html::dom::Aenderung::Attribut { knoten: k, name: attr.to_string(), alt: Some(text.clone()), ns: None });
            }
            Some(Ok(()))
        }
        _ => None,
    }
}

/// Release 09.10.2026: `classList` (DOMTokenList, DOM §7.1) und `dataset`
/// (HTML §3.2.6.6) lesen — direkt aus dem `class`- bzw. `data-*`-Attribut.
pub fn unterobjekt_lesen(l: &mut Lebend, id: u32, art: u8, name: &str) -> Option<Wert> {
    let k = l.knoten(id)?;
    if !l.dok.ist_element_knoten(k) {
        return None;
    }
    match art {
        2 => {
            let klassen = klassen_von(&l.dok, k);
            match name {
                "length" => Some(Wert::Zahl(klassen.len() as f64)),
                "value" => Some(Wert::Text(l.dok.attribut(k, "class").unwrap_or("").to_string())),
                // Release 2026-10-15: the supported property indices (for
                // Object.getOwnPropertyNames and friends in the engine).
                "__narsilOwnKeys" => Some(Wert::Feld((0..klassen.len()).map(|i| Wert::Text(i.to_string())).collect())),
                // Release 2026-10-15: an index past the end is no property
                // (`5 in el.classList` is false).
                _ => name.parse::<usize>().ok().and_then(|i| klassen.get(i)).map(|c| Wert::Text(c.clone())),
            }
        }
        3 => l.dok.attribut(k, &datensatz_attribut(name)?).map(|v| Wert::Text(v.to_string())),
        _ => None,
    }
}

/// Release 2026-10-15: whether `id` is the host id of a live collection.
#[must_use]
pub fn is_collection_host(id: u32) -> bool {
    (FIRST_COLLECTION_HOST..FIRST_COLLECTION_HOST + MAX_COLLECTIONS as u32).contains(&id)
}

/// Release 2026-10-15: a property of a live collection: `length`, an index,
/// a named property (HTMLCollection: the first element whose id, or for
/// HTML elements whose `name`, is the name), `__narsilOwnKeys` (the
/// supported indices and names, WebIDL §3.9 [[OwnPropertyKeys]]) and
/// `__narsilNamedItem:<name>` (namedItem without the prototype's shadowing).
/// `None`: no such property.
pub fn collection_lesen(l: &mut Lebend, id: u32, name: &str) -> Option<Wert> {
    if !l.collection_refresh(id) {
        return None;
    }
    let c = l.collections.get(&id)?;
    // Named properties: HTMLCollection (5) and HTMLFormControlsCollection (7).
    let html_collection = matches!(c.kind.art(), 5 | 7);
    let form_controls = c.kind == CollectionKind::FormElements;
    let root = c.root;
    if name == "length" {
        return Some(Wert::Zahl(c.items.len() as f64));
    }
    if let Some(i) = array_index(name) {
        let k = c.items.get(i).copied()?;
        return Some(l.wert(Some(k)));
    }
    if name == "__narsilOwnKeys" {
        let mut keys: Vec<String> = (0..c.items.len()).map(|i| i.to_string()).collect();
        if html_collection {
            let mut names: Vec<String> = Vec::new();
            for &e in &c.items {
                if let Some(v) = l.dok.attribut(e, "id").filter(|v| !v.is_empty())
                    && !names.iter().any(|n| n == v)
                {
                    names.push(v.to_string());
                }
                let ist_html = matches!(l.dok.knoten.get(e).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. }));
                if ist_html
                    && let Some(v) = l.dok.attribut(e, "name").filter(|v| !v.is_empty())
                    && !names.iter().any(|n| n == v)
                {
                    names.push(v.to_string());
                }
            }
            keys.extend(names);
        }
        return Some(Wert::Feld(keys.into_iter().map(Wert::Text).collect()));
    }
    if !html_collection {
        return None;
    }
    let (gesucht, nur_benannt) = match name.strip_prefix("__narsilNamedItem:") {
        Some(n) => (n, true),
        None => (name, false),
    };
    if gesucht.is_empty() || (!nur_benannt && (name.starts_with("__") || name.starts_with("Symbol("))) {
        return if nur_benannt { Some(Wert::Null) } else { None };
    }
    let passt = |e: usize| {
        l.dok.attribut(e, "id") == Some(gesucht)
            || (matches!(l.dok.knoten.get(e).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. }))
                && l.dok.attribut(e, "name") == Some(gesucht))
    };
    // HTMLFormControlsCollection: several matches are a RadioNodeList
    // (HTML §2.6.2.2 namedItem).
    if form_controls && c.items.iter().filter(|&&e| passt(e)).take(2).count() > 1 {
        return l.collection(root, CollectionKind::FormNamed(gesucht.to_string()));
    }
    let treffer = c.items.iter().copied().find(|&e| passt(e));
    match treffer {
        Some(k) => Some(l.wert(Some(k))),
        None if nur_benannt => Some(Wert::Null),
        None => None,
    }
}

/// A canonical array index ("0", "17"; not "01", "-1", or 2^32 - 1 and up).
fn array_index(name: &str) -> Option<usize> {
    if name.is_empty() || name.len() > 10 || !name.bytes().all(|b| b.is_ascii_digit()) || (name.len() > 1 && name.starts_with('0')) {
        return None;
    }
    let n: u64 = name.parse().ok()?;
    (n < u64::from(u32::MAX)).then_some(n as usize)
}

/// `dataset.x = …`; `None` = nicht zustaendig.
pub fn unterobjekt_schreiben(l: &mut Lebend, id: u32, art: u8, name: &str, wert: &Wert) -> Option<Result<(), String>> {
    let k = l.knoten(id)?;
    if !l.dok.ist_element_knoten(k) {
        return None;
    }
    match art {
        2 if name == "value" => {
            let alt = l.dok.attribut(k, "class").map(str::to_string);
            l.dok.setze_attribut(k, Namensraum::Html, "class", &wert.als_text());
            l.geaendert = true;
            if l.ce_aktiv() {
                l.ce_attribut(k, "class", alt);
            }
            Some(Ok(()))
        }
        3 => {
            let attr = datensatz_attribut(name)?;
            let alt = l.dok.attribut(k, &attr).map(str::to_string);
            l.dok.setze_attribut(k, Namensraum::Html, &attr, &wert.als_text());
            l.geaendert = true;
            if l.ce_aktiv() {
                l.ce_attribut(k, &attr, alt);
            }
            Some(Ok(()))
        }
        _ => None,
    }
}

/// `fooBar` -> `data-foo-bar`; `None`, wenn der Name kein gueltiger Schluessel ist.
fn datensatz_attribut(name: &str) -> Option<String> {
    if name.is_empty() || name.starts_with("__") || name.contains(|c: char| c == '-' || c.is_whitespace()) {
        return None;
    }
    let mut attribut = String::from("data-");
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            attribut.push('-');
            attribut.push(c.to_ascii_lowercase());
        } else {
            attribut.push(c);
        }
    }
    Some(attribut)
}

fn klassen_von(dok: &Dokument, k: usize) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for c in dok.attribut(k, "class").unwrap_or("").split_ascii_whitespace() {
        if !raus.iter().any(|x| x == c) {
            raus.push(c.to_string());
        }
    }
    raus
}

/// `classList.add/remove/toggle/contains/replace/item` (DOM §7.1).
fn klassen_rufen(l: &mut Lebend, k: usize, name: &str, args: &[Wert]) -> Option<Result<Wert, String>> {
    let mut klassen = klassen_von(&l.dok, k);
    let token = |w: Option<&Wert>| -> Result<String, String> {
        let t = w.map(|a| a.als_text()).unwrap_or_default();
        if t.is_empty() {
            return Err("SyntaxError: das Token ist leer".into());
        }
        if t.contains(|c: char| c.is_ascii_whitespace()) {
            return Err("InvalidCharacterError: das Token enthaelt Leerraum".into());
        }
        Ok(t)
    };
    let ergebnis = match name {
        "contains" => return Some(Ok(Wert::Wahr(klassen.iter().any(|c| Some(c.as_str()) == args.first().map(|a| a.als_text()).as_deref())))),
        "item" => {
            let i = args.first().map_or(0.0, |a| match a { Wert::Zahl(n) => *n, w => w.als_text().parse().unwrap_or(0.0) });
            return Some(Ok(if i >= 0.0 { klassen.get(i as usize).map_or(Wert::Null, |c| Wert::Text(c.clone())) } else { Wert::Null }));
        }
        "toString" => return Some(Ok(Wert::Text(l.dok.attribut(k, "class").unwrap_or("").to_string()))),
        // Release 13.10.2026: `class` hat keine „supported tokens" (DOM
        // §7.1) — TypeError. GEMESSEN vorher (WPT Element-classlist): wahr.
        "supports" => return Some(Err("TypeError: DOMTokenList has no supported tokens.".into())),
        "add" | "remove" => {
            for a in args.iter().take(4096) {
                let t = match token(Some(a)) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                if name == "add" {
                    if !klassen.contains(&t) {
                        klassen.push(t);
                    }
                } else {
                    klassen.retain(|c| *c != t);
                }
            }
            Wert::Undefiniert
        }
        "toggle" => {
            let t = match token(args.first()) {
                Ok(t) => t,
                Err(e) => return Some(Err(e)),
            };
            let hat = klassen.contains(&t);
            let soll = match args.get(1) {
                Some(Wert::Undefiniert) | None => !hat,
                Some(Wert::Wahr(b)) => *b,
                Some(Wert::Null) => false,
                Some(Wert::Zahl(n)) => *n != 0.0 && !n.is_nan(),
                Some(Wert::Text(s)) => !s.is_empty(),
                Some(_) => true,
            };
            // Release 2026-10-15: a forced toggle that changes nothing runs
            // no update steps (DOM §7.1). MEASURED before (WPT
            // Element-classlist): toggle("a", true) on "a a a  b" rewrote
            // the attribute to "a b".
            if soll == hat {
                return Some(Ok(Wert::Wahr(soll)));
            }
            if soll {
                klassen.push(t);
            } else {
                klassen.retain(|c| *c != t);
            }
            Wert::Wahr(soll)
        }
        "replace" => {
            // Release 2026-10-15: both tokens are checked for emptiness
            // first (SyntaxError), then for whitespace; the set "replace"
            // puts the new token where the first of the two stood and drops
            // the rest. MEASURED before: replace("c", "a") on "c b a" gave
            // "b a", replace("a", "a") on "a" gave "".
            let roh = |w: Option<&Wert>| w.map(|a| a.als_text()).unwrap_or_default();
            let (alt, neu) = (roh(args.first()), roh(args.get(1)));
            if alt.is_empty() || neu.is_empty() {
                return Some(Err("SyntaxError: The token provided must not be empty.".into()));
            }
            if alt.contains(|c: char| c.is_ascii_whitespace()) || neu.contains(|c: char| c.is_ascii_whitespace()) {
                return Some(Err("InvalidCharacterError: The token provided contains HTML space characters, which are not valid in tokens.".into()));
            }
            if !klassen.contains(&alt) {
                return Some(Ok(Wert::Wahr(false)));
            }
            let mut neu_liste: Vec<String> = Vec::with_capacity(klassen.len());
            let mut gesetzt = false;
            for c in klassen.drain(..) {
                if c == alt || c == neu {
                    if !gesetzt {
                        neu_liste.push(neu.clone());
                        gesetzt = true;
                    }
                } else {
                    neu_liste.push(c);
                }
            }
            klassen = neu_liste;
            Wert::Wahr(true)
        }
        _ => return None,
    };
    // DOM §7.1 "update steps": ohne `class` und ohne Token kein Attribut anlegen.
    let alt = l.dok.attribut(k, "class").map(str::to_string);
    if alt.is_none() && klassen.is_empty() {
        return Some(Ok(ergebnis));
    }
    let wert = klassen.join(" ");
    if alt.as_deref() != Some(wert.as_str()) {
        l.dok.setze_attribut(k, Namensraum::Html, "class", &wert);
        l.geaendert = true;
        if l.ce_aktiv() {
            l.ce_attribut(k, "class", alt);
        }
    } else if matches!(name, "add" | "remove" | "toggle" | "replace") {
        // Release 12.10.2026: `add`/`remove` laufen die „update steps"
        // immer (DOM §7.1) — derselbe Wert ist trotzdem ein Datensatz.
        // Release 2026-10-15: so do a toggle/replace that got this far.
        l.dok.protokoll(narsil_fmt_html::dom::Aenderung::Attribut { knoten: k, name: "class".into(), alt, ns: None });
    }
    Some(Ok(ergebnis))
}

/// Methoden des lebenden DOM (Name, Nummer fuer die Methodentabelle in
/// `dom::methode_nummer`).
pub const METHODEN: &[(&str, f64)] = &[
    ("insertBefore", 100.0),
    ("removeChild", 101.0),
    ("replaceChild", 102.0),
    ("cloneNode", 103.0),
    ("hasChildNodes", 104.0),
    ("getRootNode", 105.0),
    ("normalize", 106.0),
    ("replaceWith", 107.0),
    ("replaceChildren", 108.0),
    ("hasAttribute", 109.0),
    ("toggleAttribute", 110.0),
    ("getAttributeNames", 111.0),
    ("attachShadow", 112.0),
    ("matches", 113.0),
    ("closest", 114.0),
    ("getElementsByClassName", 115.0),
    ("createTextNode", 116.0),
    ("createComment", 117.0),
    ("createDocumentFragment", 118.0),
    ("importNode", 119.0),
    ("adoptNode", 120.0),
    ("isEqualNode", 121.0),
    ("compareDocumentPosition", 122.0),
    ("isSameNode", 123.0),
    ("webkitMatchesSelector", 124.0),
    ("hasAttributes", 125.0),
    ("createHTMLDocument", 126.0),
    ("hasFeature", 127.0),
    ("createDocument", 128.0),
    // Release 09.10.2026: Custom Elements (nur fuer `vorspann.js`).
    ("__ceDefinieren", 129.0),
    ("__ceKandidaten", 130.0),
    ("__ceStates", 190.0),
    ("replace", 131.0),
    ("item", 132.0),
    ("supports", 133.0),
    // Release 10.10.2026: `style` (CSSOM §6.7).
    ("getPropertyValue", 134.0),
    ("getPropertyPriority", 135.0),
    // Release 11.10.2026: DOM §4.2.4 `moveBefore`, §4.5 die fehlenden
    // Fabriken (GEMESSEN in WPT Lauf g: 36/19/19/32 Dateien brachen mit
    // „… is not a function" ab).
    ("moveBefore", 136.0),
    ("createProcessingInstruction", 137.0),
    ("createDocumentType", 138.0),
    ("createCDATASection", 139.0),
    // Release 11.10.2026: DOM §4.9 die Namensraum-Fassungen (GEMESSEN in WPT
    // Lauf g: 28 Dateien „setAttributeNS is not a function"; echte Seiten
    // setzen so `xlink:href` an SVG-`<use>`).
    ("setAttributeNS", 140.0),
    ("getAttributeNS", 141.0),
    ("hasAttributeNS", 142.0),
    ("removeAttributeNS", 143.0),
    // Release 11.10.2026: Popover/Dialog (nur fuer `vorspann.js`).
    ("__cssZustand", 144.0),
    // Release 11.10.2026: DOM §4.4 „list of elements with namespace".
    ("getElementsByTagNameNS", 145.0),
    // Release 11.10.2026: XML lesen (nur fuer `vorspann.js`, DOMParser).
    ("__narsilXmlParsen", 146.0),
    // Release 12.10.2026: „replace data" und `splitText` mit lebenden
    // Bereichen (nur fuer `vorspann.js`, DOM §4.10/§4.11).
    ("__narsilDaten", 147.0),
    ("__narsilTeilen", 148.0),
    // Release 13.10.2026: ein ganzes HTML-Dokument lesen (DOMParser).
    ("__narsilHtmlParsen", 170.0),
    // Release 12.10.2026: `setHTMLUnsafe` mit deklarativem Shadow DOM.
    ("__narsilHtmlUnsicher", 160.0),
    ("__narsilGetHtml", 161.0),
    ("writeln", 162.0),
    // Release 2026-10-15: HTML §3.1.5 (a live NodeList).
    ("getElementsByName", 191.0),
];

/// Release 11.10.2026: die Element-Nachfahren von `k` in Baumordnung (ohne
/// `k`, ohne Vorlagen- und Schatteninhalte).
fn element_nachfahren(dok: &Dokument, k: usize) -> Vec<usize> {
    let mut raus = Vec::new();
    let mut stapel: Vec<usize> = dok.kinder(k).iter().rev().copied().collect();
    while let Some(x) = stapel.pop() {
        if raus.len() >= 200_000 {
            break;
        }
        if dok.ist_element_knoten(x) {
            raus.push(x);
            stapel.extend(dok.kinder(x).iter().rev().copied());
        }
    }
    raus
}

/// Die Namensraum-Adresse eines Elements (`None` = kein Namensraum).
fn element_ns(dok: &Dokument, k: usize) -> Option<String> {
    match dok.knoten.get(k).map(|n| &n.art) {
        Some(Art::Element { raum: Namensraum::Html, .. }) => Some("http://www.w3.org/1999/xhtml".into()),
        Some(Art::Element { raum: Namensraum::Svg, .. }) => Some("http://www.w3.org/2000/svg".into()),
        Some(Art::Element { raum: Namensraum::MathMl, .. }) => Some("http://www.w3.org/1998/Math/MathML".into()),
        Some(Art::Element { .. }) => dok.fremd_ns.get(&k).map(|(u, _)| u.clone()).filter(|u| !u.is_empty()),
        _ => None,
    }
}

/// Der qualifizierte Name eines Elements (`praefix:lokal`).
fn element_qname(dok: &Dokument, k: usize) -> String {
    let lokal = dok.element_name(k).unwrap_or("");
    match dok.fremd_ns.get(&k).and_then(|(_, p)| p.as_deref()) {
        Some(p) => format!("{p}:{lokal}"),
        None => lokal.to_string(),
    }
}

fn knoten_aus(l: &Lebend, w: Option<&Wert>) -> Option<usize> {
    w.and_then(narsil_jsvm::host_id).and_then(|h| l.knoten(h))
}

/// Erzeugt aus einem `append`-Argument (Knoten oder Text) einen Arena-Knoten.
fn knoten_oder_text(l: &mut Lebend, w: &Wert) -> Option<usize> {
    match knoten_aus(l, Some(w)) {
        Some(k) => Some(k),
        None => l.dok.neuer_text(&w.als_text()),
    }
}

/// Ein Methodenaufruf. `None` = nicht zustaendig (der Proxy-Stand
/// antwortet). `art` ist die Objektart des Aufrufs (2 = `classList`).
pub fn rufen(l: &mut Lebend, id: u32, art: u8, name: &str, args: &[Wert], zustand: &narsil_css::Zustand) -> Option<Result<Wert, String>> {
    if art == 2 {
        let k = l.knoten(id)?;
        if !l.dok.ist_element_knoten(k) {
            return None;
        }
        return klassen_rufen(l, k, name, args);
    }
    if art == 1 {
        let k = l.knoten(id)?;
        if !l.dok.ist_element_knoten(k) {
            return None;
        }
        return stil_rufen(l, k, name, args);
    }
    if art != 0 {
        return None;
    }
    let k = l.knoten(id)?;
    if name == "__ceStates" {
        let states = match args.first() {
            Some(Wert::Feld(values)) => values.iter().take(1024).map(Wert::als_text).collect(),
            _ => Vec::new(),
        };
        l.ce_states.insert(k, states);
        l.geaendert = true;
        return Some(Ok(Wert::Undefiniert));
    }
    if matches!(name, "matches" | "closest" | "querySelector" | "querySelectorAll" | "webkitMatchesSelector") {
        // Release 11.10.2026: WebIDL — der Selektor ist Pflicht. GEMESSEN
        // vorher (WPT Element-matches): `el.matches()` war `false`.
        if args.is_empty() {
            return Some(Err(format!("TypeError: Failed to execute '{name}': 1 argument required, but only 0 present.")));
        }
        // `matches`/`closest` gibt es nur an Elementen (DOM §4.9).
        if matches!(name, "matches" | "closest" | "webkitMatchesSelector") && !l.dok.ist_element_knoten(k) {
            return Some(Err(format!("TypeError: {name} is not a function")));
        }
        let sel = args.first().map(Wert::als_text).unwrap_or_default();
        // Release 12.10.2026: ein ungueltiger Selektor wirft SyntaxError
        // (DOM §4.2.6 „scope-match a selectors string", Selectors 4 §16).
        // GEMESSEN vorher (WPT css/selectors/parsing): `querySelector(':not()')`,
        // `'::has-slotted(foo)'`, `'#000'` warfen nicht, sondern trafen nichts.
        let ns = narsil_css::cssom::selektor::Namensraeume::default();
        let Some(kanonisch) = narsil_css::cssom::selektor::liste(&sel, &ns) else {
            return Some(Err(format!("SyntaxError: Failed to execute '{name}': '{sel}' is not a valid selector.")));
        };
        // Release 12.10.2026: ungueltige Teile vergebender Listen (`:is()`)
        // treffen nichts — der Motor bekommt die Liste ohne sie (WPT
        // parse-has-disallow-nesting-has-inside-has).
        if sel.contains('(')
            && let Some(ab) = narsil_css::cssom::selektor::liste_abgleich(&sel, &ns)
            && ab != kanonisch
        {
            let mut neu = args.to_vec();
            neu[0] = Wert::Text(ab);
            return rufen(l, id, art, name, &neu, zustand);
        }
        if sel.contains(":state") {
            if let Err(e) = ce_selektor(&sel) { return Some(Err(e)); }
        }
    }
    if name == "__ceDefinieren" {
        let n = args.first().map(|a| a.als_text()).unwrap_or_default();
        let beobachtet: Vec<String> = match args.get(1) {
            Some(Wert::Feld(v)) => v.iter().take(1024).map(Wert::als_text).collect(),
            _ => Vec::new(),
        };
        if l.ce_namen.len() < MAX_CE_NAMEN {
            let lokal = args.get(2).map(Wert::als_text).unwrap_or_else(|| n.clone());
            let liste = l.ce_namen.entry(n.clone()).or_default();
            for a in beobachtet { if !liste.contains(&a) { liste.push(a); } }
            let namen = l.ce_lokal.entry(n).or_default();
            if !namen.contains(&lokal) { namen.push(lokal); }
        }
        return Some(Ok(Wert::Undefiniert));
    }
    if name == "__ceKandidaten" {
        // HTML §4.13.4 define, Schritt 18: alle Elemente des Dokuments
        // (shadow-including) mit diesem Namen, in Baumordnung.
        let n = args.first().map(|a| a.als_text()).unwrap_or_default();
        let mut raus = Vec::new();
        if l.ce_namen.contains_key(&n) {
            let mut alle = Vec::new();
            l.ce_teilbaum(l.wurzel, &mut alle);
            for e in alle {
                if l.ce_name(e) == Some(n.as_str()) {
                    raus.push(l.wert(Some(e)));
                }
            }
        }
        return Some(Ok(Wert::Feld(raus)));
    }
    // Release 10.10.2026: eingefuegte `<script>` vormerken (beide Wege).
    let skript_kandidaten = l.skript_kandidaten(name, args);
    if !l.ce_aktiv() {
        let medien_eltern = if name == "remove" { l.dok.eltern(k) } else { None };
        let ersetzt = if name == "replaceChild" { args.get(1).and_then(|a| knoten_aus(l, Some(a))) } else { None };
        let bild_vorher = l.bild_vorher(k, name, args);
        // Release 2026-10-15: a details `open` attribute set or removed
        // without a change reports nothing (no `toggle`).
        let offen_vorher = (matches!(name, "setAttribute" | "removeAttribute" | "toggleAttribute")
            && args.first().is_some_and(|a| a.als_text().eq_ignore_ascii_case("open")))
        .then(|| l.dok.attribut(k, "open").is_some());
        let r = rufen_intern(l, k, name, args, zustand);
        if matches!(r, Some(Ok(_))) {
            l.bild_nachher(k, bild_vorher);
        }
        let offen_gleich = offen_vorher.is_some_and(|v| v == l.dok.attribut(k, "open").is_some());
        if name == "adoptNode" && matches!(r, Some(Ok(_))) {
            if let Some(c) = knoten_aus(l, args.first()) { l.dokument_setzen(c, k, false); }
        }
        if matches!(r, Some(Ok(_))) {
            l.script_children_changed(k, name);
        }
        if !skript_kandidaten.is_empty() {
            l.skripte_vormerken(&skript_kandidaten);
        }
        if matches!(r, Some(Ok(_))) && !offen_gleich {
            l.medien_nach_ruf(k, name, args);
            if let Some(c) = ersetzt {
                l.medien_entfernt(c);
            }
            if let Some(e) = medien_eltern {
                l.medien_kind_weg(e);
                l.medien_entfernt(k);
            }
        }
        return r;
    }
    // Custom Elements: welche Knoten gehen hinein, welche hinaus?
    let arg_knoten = |l: &Lebend, w: Option<&Wert>| -> Vec<usize> {
        knoten_aus(l, w).map(|c| l.eingefuegte(c)).unwrap_or_default()
    };
    let mut bewegt: Vec<usize> = match name {
        "appendChild" | "insertBefore" => arg_knoten(l, args.first()),
        "removeChild" | "adoptNode" => knoten_aus(l, args.first()).into_iter().collect(),
        "remove" => vec![k],
        "replaceChild" => {
            let mut v = arg_knoten(l, args.first());
            v.extend(knoten_aus(l, args.get(1)));
            v
        }
        "append" | "prepend" | "replaceChildren" | "before" | "after" | "replaceWith" => {
            let mut v: Vec<usize> = Vec::new();
            for a in args.iter().take(4096) {
                v.extend(arg_knoten(l, Some(a)));
            }
            if name == "replaceChildren" {
                v.extend(l.dok.kinder(k).iter().copied());
            }
            if name == "replaceWith" {
                v.push(k);
            }
            v
        }
        "insertAdjacentElement" => arg_knoten(l, args.get(1)),
        _ => Vec::new(),
    };
    // insertAdjacentHTML/Text: die neuen Kinder des Ziels.
    let adjazent = matches!(name, "insertAdjacentHTML" | "insertAdjacentText").then(|| {
        let wo = args.first().map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
        let ziel = if matches!(wo.as_str(), "beforebegin" | "afterend") { l.dok.eltern(k) } else { Some(k) };
        ziel.map(|z| (z, l.dok.kinder(z).to_vec()))
    }).flatten();
    let attr = match name {
        "setAttributeNS" | "removeAttributeNS" => {
            let ns = namensraum_arg(args.first());
            let q = args.get(1).map(Wert::als_text).unwrap_or_default();
            let lokal = q.rsplit(':').next().unwrap_or(&q).to_string();
            let alt = l.attr_ns_finden(k, ns.as_deref(), &lokal).map(|(_, v)| v);
            Some((lokal, alt, ns, q))
        }
        "setAttribute" | "removeAttribute" | "toggleAttribute" => {
            let n = args.first().map(|a| a.als_text()).unwrap_or_default();
            let n = if matches!(l.dok.knoten.get(k).map(|x| &x.art), Some(Art::Element { raum: Namensraum::Html, .. })) {
                n.to_ascii_lowercase()
            } else {
                n
            };
            let old = l.attribute_von(k).into_iter().find(|(q, _, _)| *q == n);
            let ns = old.as_ref().and_then(|(_, _, ns)| ns.clone());
            let alt = old.map(|(_, v, _)| v);
            let lokal = if ns.is_some() { n.rsplit(':').next().unwrap_or(&n).to_string() } else { n.clone() };
            Some((lokal, alt, ns, n))
        }
        _ => None,
    };
    let vorher = l.ce_verbundene(&bewegt);
    let ergebnis = rufen_intern(l, k, name, args, zustand);
    if !matches!(&ergebnis, Some(Ok(_))) { return ergebnis; }
    l.script_children_changed(k, name);
    if !skript_kandidaten.is_empty() {
        l.skripte_vormerken(&skript_kandidaten);
    }
    if let Some((z, alt)) = adjazent {
        bewegt.extend(l.dok.kinder(z).iter().copied().filter(|c| !alt.contains(c)));
    }
    if !vorher.is_empty() || !bewegt.is_empty() {
        if name == "adoptNode" {
            // Erst disconnected, danach adopted (das Element bleibt abgetrennt).
            l.ce_nach(vorher, &[]);
            for &c in &bewegt { l.dokument_setzen(c, k, true); l.ce_formular(c); }
        } else { l.ce_nach(vorher, &bewegt); }
    }
    if let Some((n, alt, ns, q)) = attr {
        let _ = q;
        let neu = l.attr_ns_finden(k, ns.as_deref(), &n).map(|(_, v)| v);
        if matches!(name, "setAttribute" | "setAttributeNS") || alt != neu {
            l.ce_attribut_ns(k, &n, alt, neu, ns);
            if matches!(n.as_str(), "disabled" | "form" | "id") { l.ce_formular(k); }
        }
    }
    if matches!(name, "cloneNode" | "importNode")
        && let Some(Ok(w)) = &ergebnis
        && let Some(c) = knoten_aus(l, Some(w))
    {
        // HTML §4.13.3 "create an element": ein Klon im Dokument wartet auf
        // sein Upgrade. Template-Inhalte gehoeren zu einem inaktiven
        // Dokument — ein Klon DARIN (cloneNode) wird kein Upgrade.
        let quelle = if name == "cloneNode" { Some(k) } else { None };
        let im_template = quelle.is_some_and(|q| {
            let w = l.dok.wurzel_von(q);
            matches!(l.dok.knoten.get(w).map(|n| &n.art), Some(Art::Fragment))
                && (0..l.dok.knoten.len()).any(|t| l.dok.template_inhalt(t) == Some(w))
        });
        if !im_template && l.dokument_von(c) == l.wurzel {
            l.ce_klon(c);
        }
    }
    ergebnis
}

fn rufen_intern(l: &mut Lebend, k: usize, name: &str, args: &[Wert], zustand: &narsil_css::Zustand) -> Option<Result<Wert, String>> {
    let ist_dokument = matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument));
    let arg = |i: usize| args.get(i);
    let ergebnis: Result<Wert, String> = match name {
        "appendChild" => match knoten_aus(l, arg(0)) {
            Some(c) => {
                if let Err(e) = einfuegen_pruefen(&l.dok, k, c, None, None) {
                    return Some(Err(e));
                }
                if l.dok.kind_anhaengen(k, c) {
                    l.geaendert = true;
                    Ok(arg(0).cloned().unwrap_or(Wert::Undefiniert))
                } else {
                    Err("HierarchyRequestError: appendChild wuerde einen Zyklus bilden".into())
                }
            }
            None => Err("TypeError: appendChild braucht einen Knoten".into()),
        },
        "insertBefore" | "moveBefore" => match knoten_aus(l, arg(0)) {
            Some(c) => {
                let vor = knoten_aus(l, arg(1));
                if !matches!(arg(1), None | Some(Wert::Null) | Some(Wert::Undefiniert)) && vor.is_none() {
                    return Some(Err(format!("TypeError: {name}: das zweite Argument ist kein Knoten")));
                }
                let pruefung = if name == "moveBefore" {
                    verschieben_pruefen(&l.dok, k, c, vor)
                } else {
                    einfuegen_pruefen(&l.dok, k, c, vor, None)
                };
                if let Err(e) = pruefung {
                    return Some(Err(e));
                }
                // `insertBefore(x, x)`: der Bezug ist der Knoten selbst —
                // dann gilt sein naechstes Geschwister (DOM §4.2.3 Schritt 3).
                let vor = if vor == Some(c) { l.dok.naechstes_geschwister(c) } else { vor };
                if vor == Some(c) || l.dok.kind_einfuegen(k, c, vor) {
                    l.geaendert = true;
                    Ok(if name == "moveBefore" { Wert::Undefiniert } else { arg(0).cloned().unwrap_or(Wert::Undefiniert) })
                } else {
                    Err(format!("HierarchyRequestError: {name}"))
                }
            }
            None => Err(format!("TypeError: {name} braucht einen Knoten")),
        },
        "removeChild" => match knoten_aus(l, arg(0)) {
            Some(c) if l.dok.eltern(c) == Some(k) => {
                l.dok.abtrennen(c);
                l.geaendert = true;
                Ok(arg(0).cloned().unwrap_or(Wert::Undefiniert))
            }
            _ => Err("NotFoundError: removeChild: kein Kind dieses Knotens".into()),
        },
        // Release 13.10.2026: Nicht-Knoten sind ein TypeError (WebIDL), und
        // DOM §4.2.3 „replace" prueft Elternart und Vorfahr VOR „child ist
        // kein Kind" (NotFoundError). GEMESSEN vorher (WPT Node-replaceChild
        // 23/29): immer zuerst NotFoundError.
        "replaceChild" if knoten_aus(l, arg(0)).is_none() || knoten_aus(l, arg(1)).is_none() => {
            Err("TypeError: Failed to execute 'replaceChild' on 'Node': parameter is not of type 'Node'.".into())
        }
        "replaceChild" => match (knoten_aus(l, arg(0)), knoten_aus(l, arg(1))) {
            (Some(neu), Some(_)) if eltern_pruefen(&l.dok, k, neu).is_err() => eltern_pruefen(&l.dok, k, neu).map(|()| Wert::Undefiniert),
            (Some(neu), Some(alt)) if l.dok.eltern(alt) == Some(k) => {
                if let Err(e) = einfuegen_pruefen(&l.dok, k, neu, None, Some(alt)) {
                    return Some(Err(e));
                }
                // Release 12.10.2026: DOM §4.2.3 „replace" — erst `alt`
                // entfernen, dann `neu` vor das naechste Geschwister von
                // `alt` (ohne `neu`) einfuegen; auch `replaceChild(x, x)`.
                // GEMESSEN vorher: erst eingefuegt, dann entfernt — lebende
                // Bereiche landeten eine Stelle zu weit (WPT
                // Range-mutations-replaceChild, 21 von 60 rot).
                let mut vor = l.dok.naechstes_geschwister(alt);
                if vor == Some(neu) {
                    vor = l.dok.naechstes_geschwister(neu);
                }
                // Release 12.10.2026: EIN Datensatz (hinzu: neu, weg: alt), das
                // Umhaengen von `neu` hat einen eigenen (DOM §4.2.3 replace 15).
                // Wie Chromium (und WPT MutationObserver-childList): `neu`
                // verlaesst seinen alten Platz ZUERST, mit eigenem Satz
                // (Adoption vor dem Ersetzen); `replaceChild(x, x)` ergibt so
                // zwei Saetze.
                if !matches!(l.dok.knoten[neu].art, Art::Fragment | Art::ShadowRoot { .. }) && l.dok.eltern(neu).is_some() {
                    l.dok.abtrennen(neu);
                }
                let davor = if l.dok.eltern(alt) == Some(k) {
                    l.dok.voriges_geschwister(alt)
                } else {
                    match vor {
                        Some(v) => l.dok.voriges_geschwister(v),
                        None => l.dok.kinder(k).last().copied(),
                    }
                };
                let gesammelt = l.dok.sammeln_an(k, davor, vor, true);
                if l.dok.eltern(alt) == Some(k) {
                    l.dok.abtrennen(alt);
                }
                l.dok.sammeln_weg(false);
                l.dok.kind_einfuegen(k, neu, vor);
                if gesammelt {
                    l.dok.sammeln_aus();
                }
                l.geaendert = true;
                Ok(arg(1).cloned().unwrap_or(Wert::Undefiniert))
            }
            _ => Err("NotFoundError: replaceChild".into()),
        },
        "remove" => {
            if l.dok.eltern(k).is_some() {
                l.dok.abtrennen(k);
                l.geaendert = true;
            }
            Ok(Wert::Undefiniert)
        }
        "cloneNode" => {
            let tief = arg(0).is_some_and(|w| matches!(w, Wert::Wahr(true)));
            match l.dok.klonen(k, tief) {
                Some(c) => {
                    let mut paare = vec![(k, c)];
                    while let Some((alt, neu)) = paare.pop() {
                        let is = l.ce_is.get(&alt).cloned().unwrap_or_else(|| l.dok.attribut(alt, "is").map(str::to_string));
                        l.ce_is.insert(neu, is);
                        let ns: Vec<_> = l.attribut_ns.iter().filter(|((node, _), _)| *node == alt)
                            .map(|((_, q), uri)| ((neu, q.clone()), uri.clone())).collect();
                        l.attribut_ns.extend(ns);
                        if let Some(attrs) = l.fremde_attribute.get(&alt).cloned() { l.fremde_attribute.insert(neu, attrs); }
                        // Release 11.10.2026: ein Klon erbt „already started"
                        // (HTML §4.12.1 „cloning steps" von `script`).
                        if l.skripte_gestartet.contains(&alt) {
                            l.skripte_gestartet.insert(neu);
                        }
                        paare.extend(l.dok.kinder(alt).iter().copied().zip(l.dok.kinder(neu).iter().copied()));
                    }
                    l.schatten_mitklonen(k, c);
                    let doc = l.dokument_von(k);
                    l.dokument_setzen(c, doc, false);
                    Ok(l.wert(Some(c)))
                }
                None => Err("cloneNode: Knotengrenze erreicht".into()),
            }
        }
        "hasChildNodes" => Ok(Wert::Wahr(!l.dok.kinder(k).is_empty())),
        "hasAttributes" => Ok(Wert::Wahr(!l.attribute_von(k).is_empty())),
        "contains" => {
            let ja = knoten_aus(l, arg(0)).is_some_and(|c| l.dok.enthaelt(k, c));
            Ok(Wert::Wahr(ja))
        }
        "getRootNode" => {
            let w = l.dok.wurzel_von(k);
            Ok(l.wert(Some(w)))
        }
        "isSameNode" => Ok(Wert::Wahr(knoten_aus(l, arg(0)) == Some(k))),
        // Release 2026-10-15: DOM §4.4 "equals" (doctype ids, namespace,
        // prefix, attribute namespaces; recursively over the children).
        // MEASURED before (WPT Node-isEqualNode 6/9): the serializations
        // were compared, which ignore namespaces and doctype ids.
        "isEqualNode" => Ok(Wert::Wahr(knoten_aus(l, arg(0)).is_some_and(|c| knoten_gleich(l, k, c, 0)))),
        "compareDocumentPosition" => {
            let Some(c) = knoten_aus(l, arg(0)) else { return Some(Ok(Wert::Zahl(1.0))) };
            let wert = if c == k {
                0.0
            } else if l.dok.enthaelt(k, c) {
                20.0 // CONTAINED_BY | FOLLOWING
            } else if l.dok.enthaelt(c, k) {
                10.0 // CONTAINS | PRECEDING
            } else if l.dok.wurzel_von(c) != l.dok.wurzel_von(k) {
                // DISCONNECTED | IMPLEMENTATION_SPECIFIC und eine feste
                // Richtung (DOM §4.4 Schritt 6: „consistent"), hier nach der
                // Arena-Stelle der Wurzeln. Release 12.10.2026, vorher 33.
                if l.dok.wurzel_von(c) < l.dok.wurzel_von(k) { 35.0 } else { 37.0 }
            } else if vorordnung_vor(&l.dok, c, k) {
                2.0
            } else {
                4.0
            };
            Ok(Wert::Zahl(wert))
        }
        "normalize" => {
            // Release 12.10.2026: DOM §4.4 mit lebenden Bereichen und
            // `characterData`-Datensaetzen (`Dokument::normalisieren`).
            l.dok.normalisieren(k);
            l.geaendert = true;
            Ok(Wert::Undefiniert)
        }
        // Release 12.10.2026: DOM §4.10 „replace data" (nur fuer den Vorspann:
        // `replaceData`/`insertData`/`deleteData`/`appendData`), mit den
        // lebenden Bereichen. GEMESSEN vorher: die Methoden setzten `data`
        // neu, jeder Grenzpunkt im Knoten sprang auf 0.
        // Release 12.10.2026: `setHTMLUnsafe` (HTML §8.5): wie `innerHTML =`,
        // aber mit deklarativen Shadow Roots. GEMESSEN vorher (WPT
        // shadow-dom/declarative/gethtml): `setHTMLUnsafe` war `innerHTML` —
        // `<template shadowrootmode>` blieb ein Template.
        // Release 12.10.2026: `getHTML` (HTML §8.5), siehe
        // `Dokument::get_html`. GEMESSEN vorher (WPT shadow-dom/declarative/
        // gethtml, 6.908 Teiltests): „getHTML is not a function".
        "__narsilGetHtml" => {
            let serialisierbar = matches!(arg(0), Some(Wert::Wahr(true)));
            let wurzeln: Vec<usize> = match arg(1) {
                Some(Wert::Feld(liste)) => liste.iter().take(10_000).filter_map(|w| knoten_aus(l, Some(w))).collect(),
                _ => Vec::new(),
            };
            Ok(Wert::Text(l.dok.get_html(k, serialisierbar, &wurzeln)))
        }
        "__narsilHtmlUnsicher" => {
            let html = arg(0).map(Wert::als_text).unwrap_or_default();
            if html.len() > MAX_HTML {
                return Some(Err(format!("setHTMLUnsafe: {} Byte (erlaubt {MAX_HTML})", html.len())));
            }
            let vorher = l.dok.knoten.len();
            if l.dok.inner_html_setzen_mit(k, &html, true) {
                l.geaendert = true;
                l.schatten_merkmale_lesen(vorher);
                if html.contains("script") || html.contains("SCRIPT") {
                    l.skripte_markieren(k);
                }
            }
            Ok(Wert::Undefiniert)
        }
        // Release 13.10.2026: `document.write`/`writeln` (HTML §8.4.3) ins
        // LEBENDE DOM: waehrend eines Parser-Skripts an die Einfuegestelle
        // hinter dem Skript, sonst ans Ende von `<body>`; geschriebene
        // Skripte laufen. GEMESSEN vorher: der Text landete nur in einem
        // alten Patch-Weg — spaetere Skripte fanden ihn nie
        // (getElementById), WPT execution-timing 072 u. a.
        "write" | "writeln" if ist_dokument => {
            let mut html: String = args.iter().take(1024).map(Wert::als_text).collect();
            if name == "writeln" {
                html.push('\n');
            }
            if html.len() > MAX_HTML {
                return Some(Err(format!("document.write: {} Byte (erlaubt {MAX_HTML})", html.len())));
            }
            let body = || {
                let h = element_kinder(&l.dok, k).into_iter().find(|&e| l.dok.element_name(e) == Some("html"))?;
                element_kinder(&l.dok, h).into_iter().find(|&e| l.dok.element_name(e) == Some("body"))
            };
            let ziel = match l.schreib_eltern.filter(|&p| l.dok.ist_element_knoten(p)) {
                Some(p) if l.dok.element_name(p) == Some("head") => body().or(Some(p)),
                Some(p) => Some(p),
                None => body(),
            };
            let Some(ziel) = ziel else { return Some(Ok(Wert::Undefiniert)) };
            let neu = l.dok.html_anhaengen(ziel, &html);
            if !neu.is_empty() {
                l.geaendert = true;
                l.skripte_vormerken(&neu);
            }
            Ok(Wert::Undefiniert)
        }
        "__narsilDaten" => {
            let zahl = |w: Option<&Wert>| match w {
                Some(Wert::Zahl(z)) if z.is_finite() && *z >= 0.0 => (*z).min(f64::from(u32::MAX)) as u32,
                _ => 0,
            };
            let daten = arg(2).map(Wert::als_text).unwrap_or_default();
            match l.dok.daten_ersetzen(k, zahl(arg(0)), zahl(arg(1)), &daten) {
                Ok(()) => {
                    l.geaendert = true;
                    Ok(Wert::Undefiniert)
                }
                Err(laenge) => Err(format!("IndexSizeError: The offset {} is greater than the node's length ({laenge}).", zahl(arg(0)))),
            }
        }
        // Release 12.10.2026: DOM §4.11 `splitText` (nur fuer den Vorspann).
        "__narsilTeilen" => {
            let versatz = match arg(0) {
                Some(Wert::Zahl(z)) if z.is_finite() && *z >= 0.0 => (*z).min(f64::from(u32::MAX)) as u32,
                _ => 0,
            };
            let doc = l.dokument_von(k);
            match l.dok.text_teilen(k, versatz) {
                Ok(n) => {
                    if let Some(n) = n {
                        l.eigner.insert(n, doc);
                    }
                    l.geaendert = true;
                    Ok(l.wert(n))
                }
                Err(laenge) => Err(format!("IndexSizeError: The offset {versatz} is greater than the node's length ({laenge}).")),
            }
        }
        "append" | "prepend" | "replaceChildren" => {
            // Release 11.10.2026: erst ALLE pruefen, dann einfuegen (DOM
            // §4.2.6 „convert nodes into a node" + pre-insert).
            for a in args.iter().take(4096) {
                if let Some(c) = knoten_aus(l, Some(a)) {
                    let ersetzt = if name == "replaceChildren" { l.dok.kinder(k).iter().copied().find(|&x| l.dok.ist_element_knoten(x)) } else { None };
                    let vor = if name == "prepend" { l.dok.kinder(k).first().copied() } else { None };
                    if let Err(e) = einfuegen_pruefen(&l.dok, k, c, vor, ersetzt) {
                        return Some(Err(e));
                    }
                } else if matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument)) {
                    return Some(Err(format!("HierarchyRequestError: {name}: Text in einem Dokument")));
                }
            }
            // Release 12.10.2026: EIN Datensatz fuer alle Knoten (DOM §4.2.6:
            // erst in ein Fragment, dann einfuegen bzw. „replace all").
            let gesammelt = if name == "replaceChildren" {
                let g = l.dok.sammeln_an(k, None, None, true);
                l.dok.kinder_leeren(k);
                l.dok.sammeln_weg(false);
                g
            } else if name == "prepend" {
                let erstes = l.dok.kinder(k).first().copied();
                l.dok.sammeln_an(k, None, erstes, false)
            } else {
                let letztes = l.dok.kinder(k).last().copied();
                l.dok.sammeln_an(k, letztes, None, false)
            };
            let erstes = l.dok.kinder(k).first().copied();
            for a in args.iter().take(4096) {
                if let Some(c) = knoten_oder_text(l, a) {
                    let vor = if name == "prepend" { erstes } else { None };
                    l.dok.kind_einfuegen(k, c, vor);
                }
            }
            if gesammelt {
                l.dok.sammeln_aus();
            }
            l.geaendert = true;
            Ok(Wert::Undefiniert)
        }
        "before" | "after" | "replaceWith" => {
            let Some(e) = l.dok.eltern(k) else { return Some(Ok(Wert::Undefiniert)) };
            for a in args.iter().take(4096) {
                if let Some(c) = knoten_aus(l, Some(a))
                    && c != k
                {
                    let ersetzt = (name == "replaceWith").then_some(k);
                    if let Err(fehler) = einfuegen_pruefen(&l.dok, e, c, None, ersetzt) {
                        return Some(Err(fehler));
                    }
                }
            }
            // Release 12.10.2026: `replaceWith` nach DOM §4.2.8 — das erste
            // folgende Geschwister, das kein Argument ist; `k` geht ZUERST
            // weg (wie „replace"), dann kommen die Knoten davor.
            let argumente: Vec<usize> = args.iter().take(4096).filter_map(|a| knoten_aus(l, Some(a))).collect();
            // Release 13.10.2026: `before`/`after` ebenso (DOM §4.2.8 „viable
            // previous/next sibling"): Geschwister, die selbst Argumente sind,
            // zaehlen nicht; der Anker ist der Platz NACH dem Herausloesen
            // der Argumente. GEMESSEN vorher (WPT ChildNode-after/-before):
            // `c.after(x, y, z)` mit Geschwistern ergab die falsche
            // Reihenfolge, `c.after(c)` blieb stehen.
            let nach_rechts = |l: &Lebend, mut n: Option<usize>| {
                while let Some(x) = n.filter(|x| argumente.contains(x)) {
                    n = l.dok.naechstes_geschwister(x);
                }
                n
            };
            let vor = match name {
                "before" => {
                    let mut p = l.dok.voriges_geschwister(k);
                    while let Some(x) = p.filter(|x| argumente.contains(x)) {
                        p = l.dok.voriges_geschwister(x);
                    }
                    let start = match p {
                        Some(p) => l.dok.naechstes_geschwister(p),
                        None => l.dok.kinder(e).first().copied(),
                    };
                    nach_rechts(l, start)
                }
                _ => nach_rechts(l, l.dok.naechstes_geschwister(k)),
            };
            // Release 12.10.2026: EIN Datensatz (bei `replaceWith` mit `k`).
            let davor = {
                let mut p = match vor {
                    Some(v) => l.dok.voriges_geschwister(v),
                    None => l.dok.kinder(e).last().copied(),
                };
                while let Some(x) = p.filter(|x| argumente.contains(x) || (name == "replaceWith" && *x == k)) {
                    p = l.dok.voriges_geschwister(x);
                }
                p
            };
            let gesammelt = l.dok.sammeln_an(e, davor, vor, name == "replaceWith");
            if name == "replaceWith" {
                l.dok.abtrennen(k);
                l.dok.sammeln_weg(false);
            }
            for a in args.iter().take(4096) {
                if let Some(c) = knoten_oder_text(l, a) {
                    l.dok.kind_einfuegen(e, c, vor);
                }
            }
            if gesammelt {
                l.dok.sammeln_aus();
            }
            l.geaendert = true;
            Ok(Wert::Undefiniert)
        }
        "insertAdjacentHTML" | "insertAdjacentText" | "insertAdjacentElement" => {
            let wo = arg(0).map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
            let (eltern, vor) = match wo.as_str() {
                "beforebegin" => (l.dok.eltern(k), Some(k)),
                "afterbegin" => (Some(k), l.dok.kinder(k).first().copied()),
                "beforeend" => (Some(k), None),
                "afterend" => (l.dok.eltern(k), l.dok.naechstes_geschwister(k)),
                _ => return Some(Err(format!("SyntaxError: {name}: unbekannte Lage {wo:?}"))),
            };
            // Release 13.10.2026: DOM Parsing §2.4 — `insertAdjacentHTML`
            // davor/danach ohne Elternelement (null oder das Dokument) wirft
            // NoModificationAllowedError. GEMESSEN vorher (WPT
            // insert-adjacent-parent-errors, insert_adjacent_html): kein Fehler.
            if name == "insertAdjacentHTML"
                && matches!(wo.as_str(), "beforebegin" | "afterend")
                && eltern.is_none_or(|e| matches!(l.dok.knoten.get(e).map(|n| &n.art), Some(Art::Dokument)))
            {
                return Some(Err("NoModificationAllowedError: insertAdjacentHTML ohne Elternelement".into()));
            }
            let Some(eltern) = eltern else { return Some(Ok(Wert::Null)) };
            let neue: Vec<usize> = match name {
                "insertAdjacentHTML" => {
                    let html = arg(1).map(|a| a.als_text()).unwrap_or_default();
                    if html.len() > MAX_HTML {
                        return Some(Err("insertAdjacentHTML: HTML zu lang".into()));
                    }
                    // Kontext `<html>` (HTML-Dokument) wird `<body>` (DOM
                    // Parsing §2.4 Schritt 2) — sonst entstanden weitere
                    // `<head>`/`<body>`.
                    let kontext = match l.dok.element_name(eltern) {
                        Some(n) if !n.eq_ignore_ascii_case("html") => n.to_string(),
                        _ => "body".to_string(),
                    };
                    let Some(hilf) = l.dok.neues_element(&kontext) else { return Some(Ok(Wert::Undefiniert)) };
                    l.dok.inner_html_setzen(hilf, &html);
                    l.skripte_markieren(hilf);
                    l.dok.kinder(hilf).to_vec()
                }
                "insertAdjacentText" => l.dok.neuer_text(&arg(1).map(|a| a.als_text()).unwrap_or_default()).into_iter().collect(),
                _ => knoten_aus(l, arg(1)).into_iter().collect(),
            };
            let davor = match vor {
                Some(v) => l.dok.voriges_geschwister(v),
                None => l.dok.kinder(eltern).last().copied(),
            };
            let gesammelt = l.dok.sammeln_an(eltern, davor, vor, false);
            for n in neue {
                l.dok.kind_einfuegen(eltern, n, vor);
            }
            if gesammelt {
                l.dok.sammeln_aus();
            }
            l.geaendert = true;
            Ok(if name == "insertAdjacentElement" { arg(1).cloned().unwrap_or(Wert::Null) } else { Wert::Undefiniert })
        }
        "getAttribute" => {
            let n = arg(0).map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
            Ok(l.attribute_von(k).into_iter().find(|(q, _, _)| *q == n)
                .map_or(Wert::Null, |(_, v, _)| Wert::Text(v)))
        }
        "hasAttribute" => {
            let n = arg(0).map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
            Ok(Wert::Wahr(l.attribute_von(k).iter().any(|(a, _, _)| *a == n)))
        }
        "getAttributeNames" => Ok(Wert::Feld(l.attribute_von(k).into_iter().map(|(n, _, _)| Wert::Text(n)).collect())),
        "__cssZustand" if l.dok.ist_element_knoten(k) => {
            let bits = arg(0).map_or(0.0, |w| match w {
                Wert::Zahl(z) => *z,
                _ => 0.0,
            });
            let bits = if bits.is_finite() { (bits as i64).clamp(0, 255) as u8 } else { 0 };
            // Release 11.10.2026: der Vorspann setzt nur die Bits 1/2/4; der
            // Haken-Zustand (8/16/32) gehoert dem Host.
            let bits = (l.dok.css_zustand_von(k) & narsil_fmt_html::dom::HAKEN_BITS) | (bits & 7);
            if l.dok.css_zustand_von(k) != bits {
                l.dok.css_zustand_setzen(k, bits);
                l.geaendert = true;
            }
            Ok(Wert::Undefiniert)
        }
        "setAttributeNS" => {
            if !l.dok.ist_element_knoten(k) {
                return None;
            }
            let ns = namensraum_arg(arg(0));
            let q = arg(1).map(|a| a.als_text()).unwrap_or_default();
            let v = arg(2).map(|a| a.als_text()).unwrap_or_default();
            let (raum, name) = match namensraum_pruefen(ns.as_deref(), &q) {
                Ok(x) => x,
                Err(e) => return Some(Err(e)),
            };
            // Fremde Namensraeume duerfen keine HTML-Attribut-Sonderfaelle ausloesen.
            if let Some(uri) = ns.as_deref().filter(|uri| !matches!(*uri, NS_XLINK | NS_XML | NS_XMLNS)) {
                let local = q.rsplit(':').next().unwrap_or(&q).to_string();
                let attrs = l.fremde_attribute.entry(k).or_default();
                let mut vorher = None;
                if let Some(a) = attrs.iter_mut().find(|(old, old_ns, _)| old_ns == uri && old.rsplit(':').next() == Some(local.as_str())) {
                    vorher = Some(std::mem::replace(a, (q, uri.to_string(), v)).2);
                } else if attrs.len() < 8192 { attrs.push((q, uri.to_string(), v)); }
                // Release 12.10.2026: auch ein fremder Namensraum gibt einen
                // Datensatz (DOM §4.9 „change/append an attribute").
                l.dok.protokoll(narsil_fmt_html::dom::Aenderung::Attribut { knoten: k, name: local, alt: vorher, ns: Some(uri.to_string()) });
                l.geaendert = true;
                return Some(Ok(Wert::Undefiniert));
            }
            // Der Namensraum gehoert zum Attribut, unabhaengig vom Praefix.
            l.dok.setze_attribut(k, raum, &name, &v);
            if let Some(ns) = ns {
                l.attribut_ns.insert((k, q), ns);
            }
            l.geaendert = true;
            if raum == Namensraum::Html && (name == "src" || name == "srcset") {
                l.bild_quelle_vermerken(k);
            }
            Ok(Wert::Undefiniert)
        }
        "getAttributeNS" | "hasAttributeNS" | "removeAttributeNS" => {
            if !l.dok.ist_element_knoten(k) {
                return None;
            }
            let ns = namensraum_arg(arg(0));
            let lokal = arg(1).map(|a| a.als_text()).unwrap_or_default();
            let treffer = l.attr_ns_finden(k, ns.as_deref(), &lokal);
            match name {
                "getAttributeNS" => Ok(treffer.map_or(Wert::Null, |t| Wert::Text(t.1))),
                "hasAttributeNS" => Ok(Wert::Wahr(treffer.is_some())),
                _ => {
                    if let Some((q, alt_wert)) = treffer {
                        if let Some(uri) = ns.as_deref().filter(|uri| !matches!(*uri, NS_XLINK | NS_XML | NS_XMLNS)) {
                            if let Some(attrs) = l.fremde_attribute.get_mut(&k) {
                                attrs.retain(|(old, old_ns, _)| old != &q || old_ns != uri);
                                l.geaendert = true;
                            }
                            let lokal_name = q.rsplit(':').next().unwrap_or(&q).to_string();
                            l.dok.protokoll(narsil_fmt_html::dom::Aenderung::Attribut { knoten: k, name: lokal_name, alt: Some(alt_wert), ns: Some(uri.to_string()) });
                        } else {
                            if l.dok.attribut_entfernen(k, &q) { l.geaendert = true; }
                            l.attribut_ns.remove(&(k, q));
                        }
                    }
                    Ok(Wert::Undefiniert)
                }
            }
        }
        "setAttribute" => {
            let n = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let v = arg(1).map(|a| a.als_text()).unwrap_or_default();
            // Release 13.10.2026: „valid attribute local name" (DOM §1.4).
            if narsil_jsvm::attribute_name(&n).is_err() {
                return Some(Err("InvalidCharacterError: setAttribute".into()));
            }
            let raum = if l.dok.knoten.get(k).is_some_and(|x| matches!(x.art, Art::Element { raum: Namensraum::Html, .. })) {
                n.to_ascii_lowercase()
            } else {
                n
            };
            if !l.dok.attribute_von(k).iter().any(|(q, _)| q == &raum)
                && let Some(attr) = l.fremde_attribute.get_mut(&k).and_then(|attrs| attrs.iter_mut().find(|(q, _, _)| q == &raum)) {
                attr.2 = v;
                l.geaendert = true;
                return Some(Ok(Wert::Undefiniert));
            }
            l.dok.setze_attribut(k, Namensraum::Html, &raum, &v);
            l.geaendert = true;
            if raum == "src" || raum == "srcset" {
                l.bild_quelle_vermerken(k);
            }
            Ok(Wert::Undefiniert)
        }
        "removeAttribute" => {
            let n = arg(0).map(|a| a.als_text().to_ascii_lowercase()).unwrap_or_default();
            if l.dok.attribut_entfernen(k, &n) {
                l.geaendert = true;
            } else if let Some(attrs) = l.fremde_attribute.get_mut(&k)
                && let Some(i) = attrs.iter().position(|(q, _, _)| q == &n) {
                attrs.remove(i); l.geaendert = true;
            }
            Ok(Wert::Undefiniert)
        }
        // Release 13.10.2026: DOM §4.9 `toggleAttribute` — Namenspruefung,
        // Kleinschreibung nur fuer HTML-Elemente, das ERSTE Attribut mit dem
        // qualifizierten Namen (auch in fremden Namensraeumen), `force` nach
        // Wahrheitswert. GEMESSEN vorher (WPT dom/nodes/attributes):
        // `toggleAttribute("")` warf nicht, `foo:bar` (Namensraum foo) blieb.
        "toggleAttribute" => {
            let roh = arg(0).map(|a| a.als_text()).unwrap_or_default();
            if narsil_jsvm::attribute_name(&roh).is_err() {
                return Some(Err("InvalidCharacterError: toggleAttribute".into()));
            }
            let html = l.dok.knoten.get(k).is_some_and(|x| matches!(x.art, Art::Element { raum: Namensraum::Html, .. }));
            let n = if html { roh.to_ascii_lowercase() } else { roh };
            let erstes = l.attribute_von(k).into_iter().find(|(q, _, _)| *q == n);
            let kraft = match arg(1) {
                None | Some(Wert::Undefiniert) => None,
                Some(Wert::Null) => Some(false),
                Some(Wert::Wahr(b)) => Some(*b),
                Some(Wert::Zahl(z)) => Some(*z != 0.0 && !z.is_nan()),
                Some(Wert::Text(t)) => Some(!t.is_empty()),
                Some(_) => Some(true),
            };
            match (erstes, kraft) {
                (None, Some(false)) => Ok(Wert::Wahr(false)),
                (None, _) => {
                    l.dok.setze_attribut(k, Namensraum::Html, &n, "");
                    l.geaendert = true;
                    Ok(Wert::Wahr(true))
                }
                (Some(_), Some(true)) => Ok(Wert::Wahr(true)),
                (Some((q, _, ns)), _) => {
                    let fremd = ns.as_deref().filter(|uri| !matches!(*uri, NS_XLINK | NS_XML | NS_XMLNS));
                    if let Some(uri) = fremd {
                        let uri = uri.to_string();
                        let mut alt = None;
                        if let Some(attrs) = l.fremde_attribute.get_mut(&k)
                            && let Some(i) = attrs.iter().position(|(a, b, _)| *a == q && *b == uri)
                        {
                            alt = Some(attrs.remove(i).2);
                        }
                        let lokal = q.split_once(':').map_or(q.as_str(), |(_, b)| b).to_string();
                        l.dok.protokoll(narsil_fmt_html::dom::Aenderung::Attribut { knoten: k, name: lokal, alt, ns: Some(uri) });
                    } else {
                        l.dok.attribut_entfernen(k, &q);
                        l.attribut_ns.remove(&(k, q));
                    }
                    l.geaendert = true;
                    Ok(Wert::Wahr(false))
                }
            }
        }
        "attachShadow" => {
            let offen = match arg(0) {
                Some(Wert::Gegenstand(felder)) => !felder.iter().any(|(n, v)| n == "mode" && v.als_text() == "closed"),
                _ => true,
            };
            // Release 12.10.2026: `clonable`/`serializable`/`delegatesFocus`
            // (DOM §4.2.14 „attach a shadow root").
            let merkmal = |name: &str| match arg(0) {
                Some(Wert::Gegenstand(felder)) => felder.iter().any(|(n, v)| n == name && matches!(v, Wert::Wahr(true))),
                _ => false,
            };
            let mut bits = u8::from(merkmal("clonable")) | (u8::from(merkmal("serializable")) << 1) | (u8::from(merkmal("delegatesFocus")) << 2);
            // Release 13.10.2026: `slotAssignment: "manual"` (DOM §4.2.2.3).
            if let Some(Wert::Gegenstand(felder)) = arg(0)
                && felder.iter().any(|(n, v)| n == "slotAssignment" && v.als_text() == "manual")
            {
                bits |= narsil_fmt_html::dom::SCHATTEN_MANUELL;
            }
            // Release 12.10.2026: DOM §4.2.14 „attach a shadow root" Schritte
            // 1-3: nur HTML-Elemente aus der Liste oder gueltige Custom-
            // Element-Namen. GEMESSEN vorher (WPT attach-shadow-non-html-
            // namespace, 266 Teiltests): jedes Element bekam eine Wurzel.
            if !schatten_wirt_gueltig(&l.dok, k) {
                return Some(Err("NotSupportedError: Failed to execute 'attachShadow' on 'Element': This element does not support attachShadow".into()));
            }
            // Schritt 4: ein Wirt mit DEKLARATIVER Wurzel im selben Modus
            // bekommt sie geleert zurueck (`declarative` faellt weg).
            if let Some(sr) = l.dok.shadow_wurzel_intern(k) {
                let alt = l.schatten_merkmale.get(&sr).copied().unwrap_or(0);
                let sr_offen = matches!(l.dok.knoten[sr].art, Art::ShadowRoot { mode: narsil_fmt_html::dom::ShadowRootMode::Open, .. });
                if alt & narsil_fmt_html::dom::SCHATTEN_DEKLARATIV != 0 && sr_offen == offen {
                    for c in l.dok.kinder(sr).to_vec() {
                        l.dok.abtrennen(c);
                    }
                    let neu = alt & !narsil_fmt_html::dom::SCHATTEN_DEKLARATIV;
                    l.schatten_merkmale.insert(sr, neu);
                    l.dok.schatten_merkmale_setzen(k, neu);
                    l.geaendert = true;
                    return Some(Ok(l.wert(Some(sr))));
                }
                return Some(Err("NotSupportedError: Failed to execute 'attachShadow' on 'Element': Shadow root cannot be created on a host which already hosts a shadow tree.".into()));
            }
            match l.dok.shadow_anhaengen(k, offen) {
                Some(s) => {
                    if bits != 0 {
                        l.schatten_merkmale.insert(s, bits);
                        l.dok.schatten_merkmale_setzen(k, bits);
                    }
                    l.geaendert = true;
                    Ok(l.wert(Some(s)))
                }
                None => Err("NotSupportedError: attachShadow".into()),
            }
        }
        "matches" | "webkitMatchesSelector" => {
            let sel = arg(0).map(|a| a.als_text()).unwrap_or_default();
            Ok(Wert::Wahr(passt_ce(l, k, &sel, zustand, k)))
        }
        "closest" => {
            let sel = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let mut x = Some(k);
            let mut treffer = None;
            while let Some(e) = x {
                if l.dok.ist_element_knoten(e) && passt_ce(l, e, &sel, zustand, k) {
                    treffer = Some(e);
                    break;
                }
                x = eltern_dom(&l.dok, e);
            }
            Ok(l.wert(treffer))
        }
        // Release 11.10.2026: DOM §4.4 „list of elements with qualified
        // name" — im HTML-Dokument passt ein HTML-Element auf den klein
        // geschriebenen Namen, jedes andere nur genau; kein Selektor (ein
        // Name mit `:` ist kein Typselektor). GEMESSEN vorher (WPT
        // dom/nodes/case.html): `a:abc` fand `<abc>`, `Abc` fand `<ABC>`.
        "getElementsByTagName" => {
            let q = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let html_dok = !l.in_xml_document(k);
            if let Some(w) = l.collection(k, CollectionKind::TagName(q.clone(), html_dok)) {
                return Some(Ok(w));
            }
            let klein = q.to_ascii_lowercase();
            let treffer: Vec<usize> = element_nachfahren(&l.dok, k)
                .into_iter()
                .filter(|&e| {
                    if q == "*" {
                        return true;
                    }
                    let qn = element_qname(&l.dok, e);
                    let ist_html = matches!(l.dok.knoten.get(e).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. }));
                    if html_dok && ist_html { qn == klein } else { qn == q }
                })
                .collect();
            Ok(Wert::Feld(treffer.into_iter().map(|t| l.wert(Some(t))).collect()))
        }
        // Release 2026-10-15: a live NodeList (HTML §3.1.5). MEASURED before:
        // `document.getElementsByName('q')` was an empty array.
        "getElementsByName" if matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument)) => {
            let n = arg(0).map(|a| a.als_text()).unwrap_or_default();
            if let Some(w) = l.collection(k, CollectionKind::Name(n.clone())) {
                return Some(Ok(w));
            }
            let treffer: Vec<usize> = element_nachfahren(&l.dok, k)
                .into_iter()
                .filter(|&e| ist_html(&l.dok, e) && l.dok.attribut(e, "name") == Some(n.as_str()))
                .collect();
            Ok(Wert::Feld(treffer.into_iter().map(|t| l.wert(Some(t))).collect()))
        }
        "getElementsByTagNameNS" => {
            let ns = match arg(0) {
                None | Some(Wert::Null) | Some(Wert::Undefiniert) => None,
                Some(w) => Some(w.als_text()).filter(|t| !t.is_empty()),
            };
            let lokal = arg(1).map(|a| a.als_text()).unwrap_or_default();
            if let Some(w) = l.collection(k, CollectionKind::TagNameNs(ns.clone(), lokal.clone())) {
                return Some(Ok(w));
            }
            let treffer: Vec<usize> = element_nachfahren(&l.dok, k)
                .into_iter()
                .filter(|&e| {
                    let ns_ok = ns.as_deref() == Some("*") || element_ns(&l.dok, e) == ns;
                    ns_ok && (lokal == "*" || l.dok.element_name(e) == Some(lokal.as_str()))
                })
                .collect();
            Ok(Wert::Feld(treffer.into_iter().map(|t| l.wert(Some(t))).collect()))
        }
        "querySelector" | "querySelectorAll" | "getElementsByClassName" => {
            let roh = arg(0).map(|a| a.als_text()).unwrap_or_default();
            if name == "getElementsByClassName"
                && let Some(w) = l.collection(k, CollectionKind::ClassNames(roh.clone()))
            {
                return Some(Ok(w));
            }
            let sel = match name {
                "getElementsByTagName" if roh.trim() == "*" => "*".to_string(),
                "getElementsByTagName" => roh.trim().to_string(),
                // Release 13.10.2026: nur ASCII-Leerraum trennt (DOM §4.2.6.2
                // „ordered set parser"), und jede Klasse wird als CSS-Bezeichner
                // maskiert. GEMESSEN vorher (WPT getElementsByClassName-
                // whitespace-class-names 7/26): NBSP, U+000B usw. trennten
                // oder ergaben einen kaputten Selektor.
                "getElementsByClassName" => roh.split_ascii_whitespace().map(|c| format!(".{}", css_bezeichner(c))).collect::<String>(),
                _ => roh,
            };
            if sel.trim().is_empty() {
                return Some(if name == "querySelector" { Ok(Wert::Null) } else { Ok(Wert::Feld(Vec::new())) });
            }
            let erste = name == "querySelector";
            let treffer = suchen_ce(l, k, &sel, erste, zustand);
            if erste {
                Ok(l.wert(treffer.first().copied()))
            } else {
                Ok(Wert::Feld(treffer.into_iter().map(|t| l.wert(Some(t))).collect()))
            }
        }
        "getElementById" if matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Dokument | Art::Fragment | Art::ShadowRoot { .. })) => {
            let gesucht = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let treffer = (!gesucht.is_empty()).then(|| id_suchen(&l.dok, k, &gesucht)).flatten();
            Ok(l.wert(treffer))
        }
        "createElement" | "createElementNS" if ist_dokument => {
            // Release 11.10.2026: ein XML-Dokument erzeugt mit
            // `createElement` Elemente OHNE Namensraum (ausser XHTML), ohne
            // Kleinschreibung (DOM §4.5 Schritt 2/3).
            let xml = l.dok.xml_typ.get(&k).cloned();
            let ns_uri = if name == "createElementNS" { namensraum_arg(arg(0)).unwrap_or_default() } else if xml.as_deref().is_some_and(|t| t != "application/xhtml+xml") { String::new() } else { "http://www.w3.org/1999/xhtml".to_string() };
            let (raum, roh) = if name == "createElementNS" {
                let raum = match ns_uri.as_str() {
                    "http://www.w3.org/2000/svg" => Namensraum::Svg,
                    "http://www.w3.org/1998/Math/MathML" => Namensraum::MathMl,
                    "http://www.w3.org/1999/xhtml" => Namensraum::Html,
                    _ => Namensraum::Fremd,
                };
                (raum, arg(1).map(|a| a.als_text()).unwrap_or_default())
            } else if ns_uri.is_empty() {
                (Namensraum::Fremd, arg(0).map(|a| a.als_text()).unwrap_or_default())
            } else {
                (Namensraum::Html, arg(0).map(|a| a.als_text()).unwrap_or_default())
            };
            if roh.is_empty() || roh.len() > crate::js::MAX_TAG_LAENGE || roh.contains(|c: char| matches!(c, '\0' | '\t' | '\n' | '\x0c' | '\r' | ' ' | '/' | '>')) {
                return Some(Err("InvalidCharacterError: createElement".into()));
            }
            let valid = if name == "createElementNS" {
                let ns = namensraum_arg(arg(0));
                narsil_jsvm::namespace_name(ns.as_deref(), &roh).map(|_| ())
            } else { narsil_jsvm::element_name(&roh) };
            if let Err(e) = valid { return Some(Err(format!("{e}: {name}"))); }
            // Release 2026-10-15: the local name is what follows the FIRST
            // colon (DOM "validate and extract"), and createElement lower-cases
            // only in an HTML document (DOM §4.5 step 2). MEASURED before (WPT
            // Document-createElementNS, Document-createElement): "f:o:o" gave
            // localName "o", an XHTML document's createElement("foo") "FOO".
            let lokal = if name == "createElementNS" { roh.split_once(':').map_or(roh.as_str(), |(_, l)| l).to_string() } else { roh.clone() };
            let praefix = (name == "createElementNS").then(|| roh.split_once(':').map(|(p, _)| p.to_string())).flatten();
            let neu = if raum == Namensraum::Html && name == "createElement" && xml.is_none() { l.dok.neues_element(&lokal) } else { l.dok.neues_element_ns(raum, &lokal) };
            match neu {
                Some(e) => {
                    if raum == Namensraum::Fremd || praefix.is_some() {
                        l.dok.fremd_ns.insert(e, (if raum == Namensraum::Fremd { ns_uri.clone() } else { String::new() }, praefix));
                    }
                    let optionen = arg(if name == "createElementNS" { 2 } else { 1 });
                    let is = match optionen {
                        Some(Wert::Gegenstand(felder)) => felder.iter().find(|(n, _)| n == "is").map(|(_, v)| v.als_text()),
                        _ => None,
                    };
                    l.ce_is.insert(e, is);
                    l.eigner.insert(e, k);
                    Ok(l.wert(Some(e)))
                }
                None => Err("createElement: Knotengrenze erreicht".into()),
            }
        }
        // Release 12.10.2026: der neue Knoten gehoert DIESEM Dokument
        // (`ownerDocument`, DOM §4.5). GEMESSEN vorher:
        // `fremd.createTextNode('x').ownerDocument === document`.
        "createTextNode" if ist_dokument => {
            let t = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let n = l.dok.neuer_text(&t);
            if let Some(n) = n { l.eigner.insert(n, k); }
            Ok(l.wert(n))
        }
        "createComment" if ist_dokument => {
            let t = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let n = l.dok.neuer_kommentar(&t);
            if let Some(n) = n { l.eigner.insert(n, k); }
            Ok(l.wert(n))
        }
        "createDocumentFragment" if ist_dokument => {
            let n = l.dok.neues_fragment();
            if let Some(n) = n { l.eigner.insert(n, k); }
            Ok(l.wert(n))
        }
        // Release 11.10.2026: DOM §4.5 `createProcessingInstruction(target,
        // data)` — `target` ist ein XML-Name, `data` enthaelt kein `?>`.
        "createProcessingInstruction" if ist_dokument => {
            let ziel = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let daten = arg(1).map(|a| a.als_text()).unwrap_or_default();
            if !ist_xml_name(&ziel) || daten.contains("?>") {
                return Some(Err("InvalidCharacterError: createProcessingInstruction".into()));
            }
            if l.dok.knoten.len() >= narsil_fmt_html::dom::MAX_KNOTEN {
                return Some(Err("createProcessingInstruction: Knotengrenze erreicht".into()));
            }
            let n = l.dok.neuer_knoten(Art::Pi { target: ziel, daten });
            l.eigner.insert(n, k);
            Ok(l.wert(Some(n)))
        }
        // DOM §4.5.1 `DOMImplementation.createDocumentType` (`implementation`
        // ist hier das Dokument selbst).
        "createDocumentType" if ist_dokument => {
            let n = arg(0).map(|a| a.als_text()).unwrap_or_default();
            // Release 13.10.2026: „valid doctype name" (DOM §4.5.1): nur
            // ASCII-Leerraum, NUL und `>` sind verboten, kein QName mehr.
            // GEMESSEN vorher (WPT DOMImplementation-createDocumentType):
            // `""`, `"1foo"`, `":foo"` warfen InvalidCharacterError.
            if n.chars().any(|c| matches!(c, '\t' | '\n' | '\u{c}' | '\r' | ' ' | '\0' | '>')) {
                return Some(Err("InvalidCharacterError: createDocumentType".into()));
            }
            if l.dok.knoten.len() >= narsil_fmt_html::dom::MAX_KNOTEN {
                return Some(Err("createDocumentType: Knotengrenze erreicht".into()));
            }
            let public_id = arg(1).map(|a| a.als_text()).unwrap_or_default();
            let system_id = arg(2).map(|a| a.als_text()).unwrap_or_default();
            let d = l.dok.neuer_knoten(Art::Doctype { name: n, public_id, system_id });
            l.eigner.insert(d, k);
            Ok(l.wert(Some(d)))
        }
        // DOM §4.5: in einem HTML-Dokument gibt es keine CDATA-Abschnitte.
        "createCDATASection" if ist_dokument => {
            let t = arg(0).map(|a| a.als_text()).unwrap_or_default();
            if !l.dok.xml_typ.contains_key(&k) {
                return Some(Err("NotSupportedError: createCDATASection in einem HTML-Dokument".into()));
            }
            if t.contains("]]>") {
                return Some(Err("InvalidCharacterError: createCDATASection".into()));
            }
            let n = l.dok.neuer_text(&t);
            if let Some(n) = n {
                l.dok.cdata.insert(n);
                l.eigner.insert(n, k);
            }
            Ok(l.wert(n))
        }
        "importNode" if ist_dokument => {
            let tief = arg(1).is_some_and(|w| matches!(w, Wert::Wahr(true)));
            match knoten_aus(l, arg(0)) {
                Some(c) => {
                    let n = l.dok.klonen(c, tief);
                    if let Some(n) = n {
                        l.schatten_mitklonen(c, n);
                        l.dokument_setzen(n, k, false);
                        let mut paare = vec![(c, n)];
                        while let Some((alt, neu)) = paare.pop() {
                            let is = l.ce_is.get(&alt).cloned().unwrap_or_else(|| l.dok.attribut(alt, "is").map(str::to_string));
                            l.ce_is.insert(neu, is);
                            let ns: Vec<_> = l.attribut_ns.iter().filter(|((node, _), _)| *node == alt)
                                .map(|((_, q), uri)| ((neu, q.clone()), uri.clone())).collect();
                            l.attribut_ns.extend(ns);
                        if let Some(attrs) = l.fremde_attribute.get(&alt).cloned() { l.fremde_attribute.insert(neu, attrs); }
                            // Release 13.10.2026: „already started" geht mit
                            // (HTML §4.12.1 „cloning steps"). GEMESSEN vorher
                            // (WPT DOMParser-…-scripting-support-disabled):
                            // ein importiertes DOMParser-Skript lief.
                            if l.skripte_gestartet.contains(&alt) {
                                l.skripte_gestartet.insert(neu);
                            }
                            paare.extend(l.dok.kinder(alt).iter().copied().zip(l.dok.kinder(neu).iter().copied()));
                        }
                    }
                    Ok(l.wert(n))
                }
                None => Err("NotSupportedError: importNode braucht einen Knoten".into()),
            }
        }
        // Release 11.10.2026: `__narsilXmlParsen(text, typ)` — ein neues
        // XML-Dokument aus `text` (`narsil_fmt_html::xml`); bei einem
        // Wohlgeformtheitsfehler `Err("SyntaxError: …")`.
        "__narsilXmlParsen" if ist_dokument => {
            let text = arg(0).map(|a| a.als_text()).unwrap_or_default();
            let typ = arg(1).map(|a| a.als_text()).unwrap_or_else(|| "application/xml".into());
            if text.len() > 16 * 1024 * 1024 {
                return Some(Err("SyntaxError: XML zu gross".into()));
            }
            let Some(d) = neues_dokument(&mut l.dok, false, None) else {
                return Some(Err("__narsilXmlParsen: Knotengrenze erreicht".into()));
            };
            l.dok.xml_typ.insert(d, typ);
            match narsil_fmt_html::xml::xml_parsen(&mut l.dok, d, &text) {
                Ok(()) => {
                    // Release 13.10.2026: Skripte eines DOMParser-Dokuments
                    // laufen nie, auch nicht nach `importNode`/`adoptNode`.
                    l.skripte_markieren(d);
                    l.dokument_setzen(d, d, false);
                    l.geaendert = true;
                    Ok(l.wert(Some(d)))
                }
                Err(e) => Err(format!("SyntaxError: {e}")),
            }
        }
        // Release 13.10.2026: `__narsilHtmlParsen(text)` — ein neues
        // HTML-Dokument aus `text` mit dem vollen Baumbauer, Skripte aus
        // (HTML §8.5.1 DOMParser: `<noscript>` ist Markup). GEMESSEN vorher
        // (WPT DOMParser-parseFromString-html): `innerHTML` am Wurzelelement
        // verlor dessen Attribute, den Doctype (compatMode) und `<noscript>`.
        "__narsilHtmlParsen" if ist_dokument => {
            let text = arg(0).map(|a| a.als_text()).unwrap_or_default();
            if text.len() > 16 * 1024 * 1024 {
                return Some(Err("__narsilHtmlParsen: Text zu gross".into()));
            }
            // Zweites Argument `true`: deklaratives Shadow DOM erlaubt (ein
            // navigiertes Rahmendokument); DOMParser laesst es aus.
            let dsd = matches!(arg(1), Some(Wert::Wahr(true)));
            let (fremd, wurzel) = if dsd {
                narsil_fmt_html::lebend::lebendes_dokument(&text, false)
            } else {
                narsil_fmt_html::treebuilder::parse_dokument_ohne_dsd(&text, false).0
            };
            if l.dok.knoten.len() + fremd.knoten.len() + 8 >= narsil_fmt_html::dom::MAX_KNOTEN {
                return Some(Err("__narsilHtmlParsen: Knotengrenze erreicht".into()));
            }
            let Some(d) = neues_dokument(&mut l.dok, false, None) else {
                return Some(Err("__narsilHtmlParsen: Knotengrenze erreicht".into()));
            };
            for &kind in fremd.kinder(wurzel) {
                if let Some(c) = l.dok.importieren(&fremd, kind) {
                    l.dok.anhaengen(d, c);
                }
            }
            // Skripte darin sind „already started" (laufen nie, auch nicht
            // nach `importNode`/`adoptNode` ins Hauptdokument).
            l.skripte_markieren(d);
            l.dokument_setzen(d, d, false);
            l.geaendert = true;
            Ok(l.wert(Some(d)))
        }
        "createHTMLDocument" if ist_dokument => {
            // Ein neues (inaktives) Dokument in derselben Arena (DOM §4.5.1).
            let titel = arg(0).map(|a| a.als_text());
            match neues_dokument(&mut l.dok, true, titel.as_deref()) {
                Some(d) => {
                    // Release 13.10.2026: die Knoten gehoeren diesem Dokument,
                    // auch nach dem Abtrennen (`ownerDocument`). GEMESSEN
                    // vorher (WPT Node-replaceChild): ein herausgenommener
                    // Doctype nannte das Hauptdokument.
                    l.dokument_setzen(d, d, false);
                    Ok(l.wert(Some(d)))
                }
                None => Err("createHTMLDocument: Knotengrenze erreicht".into()),
            }
        }
        // Release 11.10.2026: `createDocument(namespace, qualifiedName,
        // doctype)` (DOM §4.5.1): ein XML-Dokument mit Doctype und
        // Wurzelelement; `contentType` nach dem Namensraum. GEMESSEN vorher:
        // die Argumente fielen weg, `documentElement` war null.
        "createDocument" if ist_dokument => {
            let ns = namensraum_arg(arg(0)).unwrap_or_default();
            let q = match arg(1) {
                None | Some(Wert::Null) | Some(Wert::Undefiniert) => String::new(),
                Some(w) => w.als_text(),
            };
            if !q.is_empty() {
                let nsr = (!ns.is_empty()).then_some(ns.as_str());
                if let Err(e) = narsil_jsvm::namespace_name(nsr, &q) {
                    return Some(Err(format!("{e}: createDocument")));
                }
            }
            let Some(d) = neues_dokument(&mut l.dok, false, None) else {
                return Some(Err("createDocument: Knotengrenze erreicht".into()));
            };
            let typ = match ns.as_str() {
                "http://www.w3.org/1999/xhtml" => "application/xhtml+xml",
                "http://www.w3.org/2000/svg" => "image/svg+xml",
                _ => "application/xml",
            };
            l.dok.xml_typ.insert(d, typ.to_string());
            if let Some(dt) = knoten_aus(l, arg(2))
                && matches!(l.dok.knoten.get(dt).map(|n| &n.art), Some(Art::Doctype { .. }))
            {
                l.dok.abtrennen(dt);
                l.dok.anhaengen(d, dt);
            }
            if !q.is_empty() {
                let lokal = q.split_once(':').map_or(q.as_str(), |(_, b)| b).to_string();
                let praefix = q.split_once(':').map(|(p, _)| p.to_string());
                let raum = match ns.as_str() {
                    "http://www.w3.org/1999/xhtml" => Namensraum::Html,
                    "http://www.w3.org/2000/svg" => Namensraum::Svg,
                    "http://www.w3.org/1998/Math/MathML" => Namensraum::MathMl,
                    _ => Namensraum::Fremd,
                };
                if let Some(e) = l.dok.neues_element_ns(raum, &lokal) {
                    if raum == Namensraum::Fremd || praefix.is_some() {
                        l.dok.fremd_ns.insert(e, (if raum == Namensraum::Fremd { ns.clone() } else { String::new() }, praefix));
                    }
                    l.dok.anhaengen(d, e);
                }
            }
            l.dokument_setzen(d, d, false);
            Ok(l.wert(Some(d)))
        }
        "hasFeature" if ist_dokument => Ok(Wert::Wahr(true)),
        "adoptNode" if ist_dokument => {
            // Release 2026-10-15: a document cannot be adopted (DOM §4.5
            // adoptNode step 1). MEASURED before (WPT Document-adoptNode): no
            // exception.
            if let Some(c) = knoten_aus(l, arg(0))
                && matches!(l.dok.knoten.get(c).map(|n| &n.art), Some(Art::Dokument))
            {
                return Some(Err("NotSupportedError: Failed to execute 'adoptNode' on 'Document': The node provided is a document, which may not be adopted.".into()));
            }
            if let Some(c) = knoten_aus(l, arg(0)) {
                l.dok.abtrennen(c);
            }
            Ok(arg(0).cloned().unwrap_or(Wert::Null))
        }
        _ => return None,
    };
    Some(ergebnis)
}

/// Release 11.10.2026: DOM §4.2.3 „ensure pre-insertion validity" (mit
/// `ersetzt` die Fassung von „replace"). GEMESSEN vorher (WPT dom/nodes):
/// ein Text im Dokument, ein zweites `<html>`, ein Doctype in einem Element
/// und ein Element in sich selbst wurden still eingefuegt.
/// Release 13.10.2026: ein beliebiger Text als CSS-Bezeichner (CSSOM
/// „serialize an identifier"): Steuerzeichen, ASCII-Sonderzeichen und eine
/// fuehrende Ziffer als `\hex `, NUL als U+FFFD.
fn css_bezeichner(t: &str) -> String {
    let mut s = String::with_capacity(t.len() + 4);
    let zeichen: Vec<char> = t.chars().collect();
    for (i, &c) in zeichen.iter().enumerate() {
        let ziffer_am_anfang = c.is_ascii_digit() && (i == 0 || (i == 1 && zeichen[0] == '-'));
        if c == '\0' {
            s.push('\u{FFFD}');
        } else if (c as u32) < 0x20 || c == '\u{7f}' || ziffer_am_anfang {
            s.push_str(&format!("\\{:x} ", c as u32));
        } else if i == 0 && c == '-' && zeichen.len() == 1 {
            s.push_str("\\-");
        } else if c.is_ascii_alphanumeric() || c == '_' || c == '-' || (c as u32) >= 0x80 {
            s.push(c);
        } else {
            s.push('\\');
            s.push(c);
        }
    }
    s
}

/// Release 13.10.2026: die ersten beiden Pruefungen von „ensure pre-insert
/// validity"/„replace" (DOM §4.2.3): kann `eltern` Kinder haben, und ist
/// `knoten` kein (inklusiver) Vorfahr davon?
fn eltern_pruefen(dok: &Dokument, eltern: usize, knoten: usize) -> Result<(), String> {
    let art = |i: usize| dok.knoten.get(i).map(|n| &n.art);
    let hre = |was: &str| Err(format!("HierarchyRequestError: {was}"));
    if !matches!(art(eltern), Some(Art::Dokument | Art::Fragment | Art::ShadowRoot { .. } | Art::Element { .. })) {
        return hre("der Eltern kann keine Kinder haben");
    }
    // Ein Vorfahr (ueber Shadow-Wirte hinweg) kann nicht sein eigenes Kind werden.
    let mut x = Some(eltern);
    let mut schritte = 0usize;
    while let Some(v) = x {
        if v == knoten {
            return hre("der Knoten ist ein Vorfahr des Eltern");
        }
        schritte += 1;
        if schritte > 100_000 {
            break;
        }
        x = dok.eltern(v);
    }
    Ok(())
}

pub(crate) fn einfuegen_pruefen(dok: &Dokument, eltern: usize, knoten: usize, vor: Option<usize>, ersetzt: Option<usize>) -> Result<(), String> {
    let art = |i: usize| dok.knoten.get(i).map(|n| &n.art);
    let hre = |was: &str| Err(format!("HierarchyRequestError: {was}"));
    eltern_pruefen(dok, eltern, knoten)?;
    if let Some(v) = vor
        && dok.eltern(v) != Some(eltern)
    {
        return Err("NotFoundError: der Bezugsknoten ist kein Kind".into());
    }
    match art(knoten) {
        // Release 12.10.2026: eine Shadow Root ist ein DocumentFragment (DOM
        // §4.8) — beim Einfuegen gibt sie ihre Kinder ab. GEMESSEN vorher
        // (WPT shadow-dom/Range-prototype-insertNode): HierarchyRequestError.
        Some(Art::Fragment | Art::ShadowRoot { .. } | Art::Doctype { .. } | Art::Element { .. } | Art::Text(_) | Art::Kommentar(_) | Art::Pi { .. }) => {}
        _ => return hre("dieser Knoten kann nicht eingefuegt werden"),
    }
    let im_dokument = matches!(art(eltern), Some(Art::Dokument));
    if matches!(art(knoten), Some(Art::Text(_))) && im_dokument {
        return hre("Text in einem Dokument");
    }
    if matches!(art(knoten), Some(Art::Doctype { .. })) && !im_dokument {
        return hre("ein Doctype ausserhalb eines Dokuments");
    }
    if !im_dokument {
        return Ok(());
    }
    let kinder = dok.kinder(eltern);
    let ist_element = |i: usize| matches!(art(i), Some(Art::Element { .. }));
    let ist_doctype = |i: usize| matches!(art(i), Some(Art::Doctype { .. }));
    let hat_element = kinder.iter().any(|&c| ist_element(c) && Some(c) != ersetzt);
    // Der Bezug fuer „folgt/geht voran": beim Ersetzen das ersetzte Kind.
    let bezug = ersetzt.or(vor);
    let pos = bezug.and_then(|b| kinder.iter().position(|&c| c == b));
    let doctype_folgt = pos.is_some_and(|p| kinder[p + 1..].iter().any(|&c| ist_doctype(c)))
        || (ersetzt.is_none() && vor.is_some_and(ist_doctype));
    match art(knoten) {
        Some(Art::Fragment | Art::ShadowRoot { .. }) => {
            let fk = dok.kinder(knoten);
            let elemente = fk.iter().filter(|&&c| ist_element(c)).count();
            if elemente > 1 || fk.iter().any(|&c| matches!(art(c), Some(Art::Text(_)))) {
                return hre("das Fragment passt nicht in ein Dokument");
            }
            if elemente == 1 && (hat_element || doctype_folgt) {
                return hre("das Dokument hat schon ein Element");
            }
        }
        Some(Art::Element { .. }) => {
            if hat_element || doctype_folgt {
                return hre("das Dokument hat schon ein Element");
            }
        }
        Some(Art::Doctype { .. }) => {
            let hat_doctype = kinder.iter().any(|&c| ist_doctype(c) && Some(c) != ersetzt);
            let element_davor = match pos {
                Some(p) => kinder[..p].iter().any(|&c| ist_element(c)),
                None => ersetzt.is_none() && hat_element,
            };
            if hat_doctype || element_davor {
                return hre("das Dokument hat schon einen Doctype oder ein Element davor");
            }
        }
        _ => {}
    }
    Ok(())
}

/// Release 11.10.2026: DOM §4.2.4 „ensure pre-move validity" (`moveBefore`):
/// derselbe Baum, nur Elemente und Zeichendaten.
fn verschieben_pruefen(dok: &Dokument, eltern: usize, knoten: usize, vor: Option<usize>) -> Result<(), String> {
    let wurzel = |mut i: usize| {
        let mut schritte = 0usize;
        while let Some(e) = dok.eltern(i) {
            i = e;
            schritte += 1;
            if schritte > 100_000 {
                break;
            }
        }
        i
    };
    if wurzel(eltern) != wurzel(knoten) {
        return Err("HierarchyRequestError: moveBefore zwischen zwei Baeumen".into());
    }
    if !matches!(
        dok.knoten.get(knoten).map(|n| &n.art),
        Some(Art::Element { .. } | Art::Text(_) | Art::Kommentar(_) | Art::Pi { .. })
    ) {
        return Err("HierarchyRequestError: moveBefore verschiebt nur Elemente und Zeichendaten".into());
    }
    einfuegen_pruefen(dok, eltern, knoten, vor, None)
}

const NS_XLINK: &str = "http://www.w3.org/1999/xlink";
const NS_XML: &str = "http://www.w3.org/XML/1998/namespace";
const NS_XMLNS: &str = "http://www.w3.org/2000/xmlns/";

/// Der Namensraum-Parameter: `null`/`undefined`/`""` heisst „keiner".
fn namensraum_arg(w: Option<&Wert>) -> Option<String> {
    match w {
        None | Some(Wert::Null) | Some(Wert::Undefiniert) => None,
        Some(x) => Some(x.als_text()).filter(|t| !t.is_empty()),
    }
}

/// Release 11.10.2026: DOM §1.4 „validate and extract" — und wohin das
/// Attribut in unserem Modell gehoert. Fremde Namensraeume behalten den
/// qualifizierten Namen (das Modell kennt nur xlink/xml/xmlns).
fn namensraum_pruefen(ns: Option<&str>, q: &str) -> Result<(Namensraum, String), String> {
    // Release 13.10.2026: die gelockerten Namensregeln (DOM §1.4,
    // Attribut-Kontext); Praefix bis zum ERSTEN `:`.
    if narsil_jsvm::attribute_namespace_name(Some("x"), q) == Err("InvalidCharacterError") {
        return Err("InvalidCharacterError: setAttributeNS".into());
    }
    let (praefix, lokal) = match q.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, q),
    };
    let ne = || Err("NamespaceError: setAttributeNS".into());
    if praefix.is_some() && ns.is_none() {
        return ne();
    }
    if praefix == Some("xml") && ns != Some(NS_XML) {
        return ne();
    }
    let xmlns_name = q == "xmlns" || praefix == Some("xmlns");
    if xmlns_name != (ns == Some(NS_XMLNS)) {
        return ne();
    }
    Ok(match ns {
        None => (Namensraum::Html, lokal.to_string()),
        Some(NS_XLINK) => (Namensraum::Xlink, lokal.to_string()),
        Some(NS_XML) => (Namensraum::Xml, lokal.to_string()),
        Some(NS_XMLNS) => (Namensraum::Xmlns, lokal.to_string()),
        Some(_) => (Namensraum::Html, q.to_string()),
    })
}

fn attr_qname(a: &narsil_fmt_html::dom::Attribut) -> String {
    match a.raum {
        Namensraum::Xlink => format!("xlink:{}", a.name),
        Namensraum::Xml => format!("xml:{}", a.name),
        Namensraum::Xmlns if a.name != "xmlns" => format!("xmlns:{}", a.name),
        _ => a.name.clone(),
    }
}

/// Passt ein Attribut zu `(namensraum, lokaler Name)`?
fn attribut_im_raum(a: &narsil_fmt_html::dom::Attribut, ns: Option<&str>, lokal: &str) -> bool {
    match ns {
        None => matches!(a.raum, Namensraum::Html | Namensraum::Svg | Namensraum::MathMl) && a.name == lokal,
        Some(NS_XLINK) => a.raum == Namensraum::Xlink && a.name == lokal,
        Some(NS_XML) => a.raum == Namensraum::Xml && a.name == lokal,
        Some(NS_XMLNS) => a.raum == Namensraum::Xmlns && a.name == lokal,
        Some(_) => a.raum == Namensraum::Html && a.name.split_once(':').is_some_and(|(_, l)| l == lokal),
    }
}

/// XML 1.0 `Name` (vereinfacht auf die Zeichenklassen von XML 1.0 5. Ausgabe).
fn ist_xml_name(n: &str) -> bool {
    let anfang = |c: char| {
        c == ':' || c == '_' || c.is_ascii_alphabetic()
            || matches!(c as u32, 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D
                | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
    };
    let weiter = |c: char| {
        anfang(c) || c == '-' || c == '.' || c.is_ascii_digit() || c == '\u{B7}'
            || matches!(c as u32, 0x300..=0x36F | 0x203F..=0x2040)
    };
    let mut z = n.chars();
    z.next().is_some_and(anfang) && z.all(weiter)
}

fn neues_dokument(dok: &mut Dokument, html: bool, titel: Option<&str>) -> Option<usize> {
    if dok.knoten.len() + 8 >= narsil_fmt_html::dom::MAX_KNOTEN {
        return None;
    }
    let d = dok.neuer_knoten(Art::Dokument);
    if html {
        // Release 11.10.2026: DOM §4.5.1 Schritt 2 — ein Doctype `html`.
        let typ = dok.neuer_knoten(Art::Doctype { name: "html".into(), public_id: String::new(), system_id: String::new() });
        dok.anhaengen(d, typ);
        let wurzel = dok.neues_element("html")?;
        let kopf = dok.neues_element("head")?;
        let rumpf = dok.neues_element("body")?;
        dok.anhaengen(d, wurzel);
        dok.anhaengen(wurzel, kopf);
        if let Some(t) = titel {
            let te = dok.neues_element("title")?;
            let tt = dok.neuer_text(t)?;
            dok.anhaengen(te, tt);
            dok.anhaengen(kopf, te);
        }
        dok.anhaengen(wurzel, rumpf);
    }
    Some(d)
}

fn vorordnung_vor(dok: &Dokument, a: usize, b: usize) -> bool {
    // Liegt `a` in Vorordnung vor `b`? Ueber die Ahnenketten.
    let kette = |mut x: usize| {
        let mut v = vec![x];
        while let Some(e) = dok.eltern(x) {
            v.push(e);
            x = e;
            if v.len() > 4096 {
                break;
            }
        }
        v.reverse();
        v
    };
    let (ka, kb) = (kette(a), kette(b));
    let gemeinsam = ka.iter().zip(kb.iter()).take_while(|(x, y)| x == y).count();
    if gemeinsam == 0 || gemeinsam >= ka.len() || gemeinsam >= kb.len() {
        return gemeinsam >= ka.len();
    }
    let eltern = ka[gemeinsam - 1];
    let pa = dok.kinder(eltern).iter().position(|&k| k == ka[gemeinsam]);
    let pb = dok.kinder(eltern).iter().position(|&k| k == kb[gemeinsam]);
    pa < pb
}

/// Release 10.10.2026: HTML §7.2.2.3 „named access on the Window object" —
/// das erste Element mit dieser `id`, sonst das erste `embed`/`form`/`img`/
/// `object` mit diesem `name` (in Baumreihenfolge). GEMESSEN vorher (Web-
/// Platform-Tests): `evaluateStyleChange(item, …)` mit `<div id="item">`
/// warf „'item' ist nicht definiert".
pub fn benannt(l: &mut Lebend, name: &str) -> Option<u32> {
    if name.is_empty() {
        return None;
    }
    let mut nach_name = None;
    let mut stapel = vec![l.wurzel];
    let mut treffer = None;
    while let Some(k) = stapel.pop() {
        if l.dok.ist_element_knoten(k) {
            if l.dok.attribut(k, "id") == Some(name) {
                treffer = Some(k);
                break;
            }
            if nach_name.is_none()
                && matches!(l.dok.element_name(k), Some("embed" | "form" | "img" | "object"))
                && l.dok.attribut(k, "name") == Some(name)
            {
                nach_name = Some(k);
            }
        }
        for &c in l.dok.kinder(k).iter().rev() {
            stapel.push(c);
        }
    }
    l.host(treffer.or(nach_name)?)
}

/// Release 11.10.2026: die benannten Eigenschaften des Dokuments (HTML
/// §3.1.6 „named properties"): `name` von `embed`/`form`/`iframe`/`img`/
/// `object`, `id` von `object` und von `img` mit nicht leerem `name`. Ein
/// Treffer ist das Element, mehrere sind eine Liste in Baumordnung.
/// GEMESSEN vorher (WPT `dom-tree-accessors/nameditem-*`): `document.x`
/// war `undefined`, `"x" in document` falsch.
/// Release 2026-10-15: element `k` is a named element of its document for
/// `name` (HTML §3.1.3: embed/form/iframe/img/object by name, object by
/// id, img by id when it also has a name; HTML elements only).
fn benannt_passt(dok: &Dokument, k: usize, name: &str) -> bool {
    if name.is_empty() || !ist_html(dok, k) {
        return false;
    }
    let name_passt = dok.attribut(k, "name") == Some(name);
    let id_passt = dok.attribut(k, "id") == Some(name);
    match dok.element_name(k) {
        Some("embed" | "form" | "iframe") => name_passt,
        Some("object") => name_passt || id_passt,
        Some("img") => name_passt || (id_passt && dok.attribut(k, "name").is_some_and(|n| !n.is_empty())),
        _ => false,
    }
}

/// Release 2026-10-15: the document's supported property names in tree
/// order, without duplicates (HTML §3.1.3).
fn dokument_namen(dok: &Dokument, wurzel: usize) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for k in element_nachfahren(dok, wurzel) {
        if !ist_html(dok, k) {
            continue;
        }
        let name = dok.attribut(k, "name").filter(|n| !n.is_empty());
        let id = dok.attribut(k, "id").filter(|n| !n.is_empty());
        let mut neu: Vec<&str> = Vec::new();
        match dok.element_name(k) {
            Some("embed" | "form" | "iframe") => neu.extend(name),
            // An element's id comes before its name (HTML §3.1.3).
            Some("object") => {
                neu.extend(id);
                neu.extend(name);
            }
            Some("img") => {
                if name.is_some() {
                    neu.extend(id);
                }
                neu.extend(name);
            }
            _ => {}
        }
        for n in neu {
            if raus.len() < 4096 && !raus.iter().any(|r| r == n) {
                raus.push(n.to_string());
            }
        }
    }
    raus
}

pub fn dokument_benannt(l: &mut Lebend, name: &str) -> Option<Wert> {
    if name.is_empty() {
        return None;
    }
    let mut treffer = Vec::new();
    let mut stapel = vec![l.wurzel];
    while let Some(k) = stapel.pop() {
        if l.dok.ist_element_knoten(k) && benannt_passt(&l.dok, k, name) && treffer.len() < 256 {
            treffer.push(k);
        }
        for &c in l.dok.kinder(k).iter().rev() {
            stapel.push(c);
        }
    }
    match treffer.len() {
        0 => None,
        1 => Some(l.wert(Some(treffer[0]))),
        // Release 2026-10-15: more than one is a live HTMLCollection.
        _ => {
            let wurzel = l.wurzel;
            l.collection(wurzel, CollectionKind::DocumentNamed(name.to_string()))
                .or_else(|| Some(Wert::Feld(treffer.into_iter().map(|k| l.wert(Some(k))).collect())))
        }
    }
}

fn id_suchen(dok: &Dokument, wurzel: usize, gesucht: &str) -> Option<usize> {
    let mut stapel = vec![wurzel];
    while let Some(k) = stapel.pop() {
        if dok.ist_element_knoten(k) && dok.attribut(k, "id") == Some(gesucht) {
            return Some(k);
        }
        for &c in dok.kinder(k).iter().rev() {
            stapel.push(c);
        }
        if let Some(t) = dok.template_inhalt(k) {
            let _ = t; // Template-Inhalt gehoert nicht zum Dokument (DOM §4.2.6).
        }
    }
    None
}

/// Release 10.10.2026: die berechneten Werte eines Elements fuer
/// `getComputedStyle` — die Kaskade entlang der Element-Vorfahren mit dem
/// Blattsatz der Seite. `cache` haelt die Vorfahren fest (der Host leert ihn
/// bei jeder DOM-Aenderung).
/// Release 12.10.2026: der angegebene Wert einer Eigenschaft im
/// `style`-Attribut des Elements (`None` = nicht gesetzt).
/// Release 12.10.2026: steht die Langform als ausstehende Kurzform (`var()`) im Inline-Stil?
pub fn stil_block_ausstehend(l: &Lebend, k: usize, name: &str) -> bool {
    stil_block(l, k).ausstehend(name).is_some()
}

pub fn stil_spez(l: &Lebend, k: usize, name: &str) -> Option<String> {
    let block = stil_block(l, k);
    // Release 15.10.2026: an animated value wins over a normal declaration
    // (`Lebend::animated`; an `!important` one stays) — a transition's value
    // (marked `!important`) over every declaration (CSS Cascade 5 §6.1).
    if let Some(werte) = l.animated.get(&k)
        && let Some((_, w)) = werte.iter().rev().find(|(n, _)| n == name)
    {
        let (wert, transition) = match w.strip_suffix(" !important") {
            Some(v) => (v, true),
            None => (w.as_str(), false),
        };
        if transition || block.prioritaet(name) != "important" {
            return Some(wert.to_string());
        }
    }
    let w = block.wert(name);
    (!w.is_empty()).then_some(w)
}

/// Release 12.10.2026: der angegebene Wert einer Langform, die die Kaskade
/// nur woertlich traegt (`Berechnet::schluessel`) — inline, sonst die
/// gewonnene Angabe aus den Stylesheets, auch ueber eine Kurzform
/// (`text-wrap: balance` -> `text-wrap-style`). GEMESSEN vorher (WPT
/// css-text white-space-shorthand-text-wrap): `text-wrap` aus einem
/// Stylesheet kam als `wrap` (Anfangswert der Langformen).
/// Release 12.10.2026: die Basisadresse des Dokuments (HTML §2.4.1: das erste
/// `<base href>`, sonst die Dokumentadresse) — fuer `url()` im berechneten Wert.
/// Release 14.10.2026: only a `<base>` connected to the document counts, and
/// its URL is the „frozen base URL" (HTML §4.2.3, see [`frozen_base_url`]).
/// The host now also resolves `a.href`, `img.src` … against it. MEASURED
/// before (WPT the-base-element base-javascript, base-data): every URL
/// attribute ignored `<base>`, so a second `<base>` never took over.
pub(crate) fn dokument_basis(l: &Lebend, dok_adresse: &str) -> String {
    match l.first_base_with_href(l.wurzel) {
        Some(b) => frozen_base_url(dok_adresse, l.dok.attribut(b, "href").unwrap_or("")),
        None => dok_adresse.to_string(),
    }
}

/// The frozen base URL of a `<base href>` (HTML §4.2.3): `href` resolved
/// against the fallback base URL; a failure, a `data:` or a `javascript:`
/// URL yields the fallback.
pub(crate) fn frozen_base_url(fallback: &str, href: &str) -> String {
    match narsil_url::loese_auf(fallback, href.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C'))) {
        Ok(u) => {
            let scheme = u.split(':').next().unwrap_or("").to_ascii_lowercase();
            if scheme == "data" || scheme == "javascript" { fallback.to_string() } else { u }
        }
        Err(_) => fallback.to_string(),
    }
}

/// Release 12.10.2026: bleibt `min-width`/`min-height: auto` im aufgeloesten
/// Wert stehen? Nur mit Kasten und bei einem Flex-/Raster-Element oder einem
/// gesetzten `aspect-ratio` (CSSOM §9, csswg-drafts#11716); sonst `0px`.
/// GEMESSEN vorher (WPT cssom getComputedStyle-resolved-min-size-auto):
/// jeder Block meldete `auto`.
pub(crate) fn min_auto_bleibt(l: &Lebend, k: usize, satz: &narsil_page::StilSatz, cache: &mut HashMap<usize, narsil_css::Berechnet>) -> bool {
    let mut x = Some(k);
    let mut schritte = 0;
    while let Some(e) = x {
        schritte += 1;
        if schritte > 512 {
            break;
        }
        if l.dok.ist_element_knoten(e) && berechnet(l, e, satz, cache).is_some_and(|b| b.anzeige == narsil_css::Anzeige::Keins) {
            return false;
        }
        x = l.dok.eltern(e);
    }
    let seite = stil_spez(l, k, "aspect-ratio").is_some_and(|s| !s.trim().eq_ignore_ascii_case("auto"))
        || berechnet(l, k, satz, cache).is_some_and(|b| b.seitenverhaeltnis != 0.0);
    let mut e = l.dok.eltern(k);
    while let Some(x) = e {
        if l.dok.ist_element_knoten(x) {
            break;
        }
        e = l.dok.eltern(x);
    }
    let behaelter = e.and_then(|x| berechnet(l, x, satz, cache)).is_some_and(|b| {
        matches!(b.anzeige, narsil_css::Anzeige::Flex | narsil_css::Anzeige::InlineFlex | narsil_css::Anzeige::Raster)
    });
    seite || behaelter
}

/// Release 12.10.2026: `var()` und `attr()` abwechselnd einsetzen, bis keine
/// mehr uebrig sind (ein Attributwert darf selbst `var()`/`attr()` tragen; CSS
/// Values 5 §7.7). Bleibt nach acht Runden etwas uebrig: ein Zyklus,
/// ungueltig zur Berechnungszeit. GEMESSEN vorher (WPT css-values
/// attr-all-types 74/79): `data-foo="var(--x, 3)"` blieb woertlich stehen.
pub(crate) fn ersetzen_kette(
    text: &str,
    vars: &narsil_css::Variablen,
    attr: &dyn Fn(&str) -> Option<String>,
    adressen: bool,
) -> Option<String> {
    ersetzen_kette_mit(text, vars, attr, adressen, None)
}

/// Wie [`ersetzen_kette`], mit `if()` (`wenn`: die Bedingungen des Elements).
pub(crate) fn ersetzen_kette_mit(
    text: &str,
    vars: &narsil_css::Variablen,
    attr: &dyn Fn(&str) -> Option<String>,
    adressen: bool,
    wenn: Option<&dyn narsil_css::cssom::wenn::Bedingungen>,
) -> Option<String> {
    let mut t = text.to_string();
    for _ in 0..8 {
        if let Some(b) = wenn
            && t.to_ascii_lowercase().contains("if(")
        {
            t = narsil_css::cssom::wenn::ersetzen(&t, b)?;
        }
        // Ein ungueltiges var() kann im unbenutzten Rueckfall eines attr()
        // stehen: dann attr() zuerst (WPT css-values if-cycle
        // `attr(data-foo, var(--y))` mit `--y: var(--y)`).
        let a = match narsil_css::ersetze_var(&t, vars, 0) {
            Some(v) => narsil_css::cssom::ersetzen::alles_ersetzen_mit(&v, attr, adressen)?,
            None if t.to_ascii_lowercase().contains("attr(") => {
                let a = narsil_css::cssom::ersetzen::alles_ersetzen_mit(&t, attr, adressen)?;
                narsil_css::ersetze_var(&a, vars, 0)?
            }
            None => return None,
        };
        let kv = narsil_css::cssom::syntax::komponenten(&a);
        let offen_wenn = wenn.is_some() && narsil_css::cssom::syntax::enthaelt_funk(&kv, "if");
        if !narsil_css::cssom::syntax::enthaelt_funk(&kv, "var") && !narsil_css::cssom::syntax::enthaelt_funk(&kv, "attr") && !offen_wenn {
            return Some(a);
        }
        t = a;
    }
    None
}

thread_local! {
    /// Release 13.10.2026: die Custom Properties, deren Wert gerade (mit
    /// `if()`) berechnet wird, und die in einem Zyklus gefundenen.
    static WENN_STAPEL: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static WENN_ZYKLUS: std::cell::RefCell<std::collections::HashSet<String>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Release 13.10.2026: die Custom Property `name` unter Zykluswache berechnen
/// (CSS Values 5 §if(): fuehrt eine benutzte Bedingung ueber `style()` auf die
/// Eigenschaft selbst zurueck, sind alle im Zyklus ungueltig zur
/// Berechnungszeit). GEMESSEN vorher (WPT css-values if-cycle):
/// Stapelueberlauf im JS-Faden.
pub(crate) fn wenn_wache(name: &str, f: impl FnOnce() -> Option<String>) -> Option<String> {
    let tiefe = WENN_STAPEL.with(|s| {
        let mut s = s.borrow_mut();
        s.push(name.to_string());
        s.len()
    });
    let w = if tiefe > 64 { None } else { f() };
    let leer = WENN_STAPEL.with(|s| {
        let mut s = s.borrow_mut();
        s.pop();
        s.is_empty()
    });
    let im_zyklus = WENN_ZYKLUS.with(|z| {
        let mut z = z.borrow_mut();
        let drin = z.contains(name);
        if leer {
            z.clear();
        }
        drin
    });
    if im_zyklus { None } else { w }
}

/// Steht einer der `bezuege` schon auf dem Stapel, ist das ein Zyklus: alles
/// ab dort (und `extra`) wird markiert.
fn zyklus_pruefen(bezuege: &std::collections::HashSet<String>, extra: Option<&str>) -> bool {
    WENN_STAPEL.with(|s| {
        let s = s.borrow();
        let Some(p) = s.iter().position(|n| bezuege.contains(n)) else {
            return false;
        };
        WENN_ZYKLUS.with(|z| {
            let mut z = z.borrow_mut();
            z.extend(s[p..].iter().cloned());
            if let Some(e) = extra {
                z.insert(e.to_string());
            }
        });
        true
    })
}

fn ident_zeichen(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// Die Custom Properties, auf die `text` ueber `var()` verweist — auch ueber
/// deren Werte und ueber die Attributwerte aus `attr()`.
fn bezuege(
    text: &str,
    vars: &narsil_css::Variablen,
    attr: &dyn Fn(&str) -> Option<String>,
    raus: &mut std::collections::HashSet<String>,
    tiefe: usize,
) {
    if tiefe > 16 {
        return;
    }
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let vor_ok = i == 0 || !ident_zeichen(b[i - 1]);
        let ist = |pat: &[u8]| vor_ok && b.len() >= i + pat.len() && b[i..i + pat.len()].eq_ignore_ascii_case(pat);
        let (laenge, ist_var) = if ist(b"var(") {
            (4, true)
        } else if ist(b"attr(") {
            (5, false)
        } else {
            i += 1;
            continue;
        };
        let mut j = i + laenge;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let anfang = j;
        while j < b.len() && ident_zeichen(b[j]) {
            j += 1;
        }
        let name = &text[anfang..j];
        if ist_var {
            if name.starts_with("--") && raus.insert(name.to_string()) && let Some(w) = vars.holen(name) {
                let w = w.to_string();
                bezuege(&w, vars, attr, raus, tiefe + 1);
            }
        } else if !name.is_empty() && let Some(w) = attr(name) {
            bezuege(&w, vars, attr, raus, tiefe + 1);
        }
        i = j.max(i + 1);
    }
}

/// Release 12.10.2026: die Bedingungen von `if()` an einem Element des
/// lebenden DOM (`narsil_css::cssom::wenn`).
pub(crate) struct LebendWenn<'a> {
    pub l: &'a Lebend,
    pub k: usize,
    pub satz: &'a narsil_page::StilSatz,
    pub vars: narsil_css::Variablen,
    pub eltern_vars: Option<narsil_css::Variablen>,
    /// Release 12.10.2026: die registrierten Custom Properties (nur wenn es
    /// `@property` gibt) und die Schriftgroesse fuer ihre Werte.
    pub registriert: Option<HashMap<String, (String, bool, Option<String>)>>,
    pub em: f64,
}

impl LebendWenn<'_> {
    pub(crate) fn neu<'a>(l: &'a Lebend, k: usize, satz: &'a narsil_page::StilSatz, cache: &mut HashMap<usize, narsil_css::Berechnet>) -> Option<LebendWenn<'a>> {
        let b = berechnet(l, k, satz, cache)?;
        let vars = b.variablen.clone();
        let em = f64::from(b.schriftgroesse_px);
        let mut e = l.dok.eltern(k);
        while let Some(x) = e {
            if l.dok.ist_element_knoten(x) {
                break;
            }
            e = l.dok.eltern(x);
        }
        let eltern_vars = e.and_then(|x| berechnet(l, x, satz, cache)).map(|b| b.variablen.clone());
        let reg = registrierungen(l);
        let registriert = (!reg.is_empty()).then_some(reg);
        Some(LebendWenn { l, k, satz, vars, eltern_vars, registriert, em })
    }

    /// Release 13.10.2026: a CSS-wide keyword produced by `attr()` in the
    /// custom property `name` (CSS Values 5 §7.7: it acts as if specified).
    /// `inherit` takes the parent's value, `initial` the registered initial
    /// value (else the guaranteed-invalid value), `unset` either of them by
    /// `inherits`. `None`: not such a keyword. MEASURED before (WPT css-values
    /// attr-css-wide-keywords 10 subtests): `--x` read `inherit` literally.
    pub(crate) fn css_wide_keyword(&self, name: &str, value: &str) -> Option<String> {
        let keyword = value.trim().to_ascii_lowercase();
        let registered = self.registriert.as_ref().and_then(|r| r.get(name));
        let inherits = registered.is_none_or(|(_, inherits, _)| *inherits);
        let initial = || registered.and_then(|(_, _, initial)| initial.clone()).unwrap_or_default();
        let parent = || narsil_css::cssom::wenn::Bedingungen::eltern_var(self, name).unwrap_or_else(initial);
        match keyword.as_str() {
            "inherit" => Some(parent()),
            "initial" => Some(initial()),
            "unset" => Some(if inherits { parent() } else { initial() }),
            _ => None,
        }
    }

    fn wert_in(&self, vars: &narsil_css::Variablen, name: &str, k: usize) -> Option<String> {
        let attr = |n: &str| self.l.dok.attribut(k, n).map(str::to_string);
        // Der Rohwert zuerst: ein `if()` darin loest seine Bedingungen selbst.
        let roh = vars.holen(name)?.to_string();
        // Fuehrt der Wert (ueber var()/attr()) auf eine Eigenschaft zurueck,
        // die gerade berechnet wird: ein Zyklus.
        let mut bz = std::collections::HashSet::new();
        bz.insert(name.to_string());
        bezuege(&roh, vars, &attr, &mut bz, 0);
        if zyklus_pruefen(&bz, Some(name)) {
            return None;
        }
        let w = wenn_wache(name, || ersetzen_kette_mit(&roh, vars, &attr, false, Some(self)))?;
        let w = w.trim().to_string();
        (!w.eq_ignore_ascii_case("initial")).then_some(w)
    }
}

/// Release 12.10.2026: die `@property`-Regeln der Blaetter: Name -> (Syntax,
/// erbt, Anfangswert).
pub(crate) fn registrierungen(l: &Lebend) -> HashMap<String, (String, bool, Option<String>)> {
    let mut raus = HashMap::new();
    let mut stapel = vec![l.wurzel];
    let mut schritte = 0usize;
    while let Some(n) = stapel.pop() {
        schritte += 1;
        if schritte > 100_000 {
            break;
        }
        if l.dok.element_name(n).is_some_and(|e| e.eq_ignore_ascii_case("style")) {
            let text = l.style_text(n);
            if text.contains("@property") {
                let ns = narsil_css::cssom::selektor::Namensraeume::default();
                for r in narsil_css::cssom::regeln::blatt_lesen(&text, &ns) {
                    if r.art != "property" {
                        continue;
                    }
                    let Some(b) = r.block else { continue };
                    let syntax = b.wert("syntax");
                    let erbt = b.wert("inherits").trim().eq_ignore_ascii_case("true");
                    let anfang = Some(b.wert("initial-value")).filter(|w| !w.is_empty());
                    if !syntax.is_empty() {
                        raus.insert(r.kopf.trim().to_string(), (syntax, erbt, anfang));
                    }
                }
            }
            continue;
        }
        for &c in l.dok.kinder(n).iter().rev() {
            stapel.push(c);
        }
    }
    raus
}

impl narsil_css::cssom::wenn::Bedingungen for LebendWenn<'_> {
    fn registriert_berechnet(&self, name: &str, wert: &str) -> Option<Option<String>> {
        let reg = self.registriert.as_ref()?;
        let (syntax, erbt, anfang) = reg.get(name)?;
        if wert == "\u{1}erbt" {
            return Some(erbt.then(String::new));
        }
        let u = narsil_css::cssom::mathe::Umgebung { em: self.em, geschwister: geschwister_von(self.l, self.k), ..Default::default() };
        if wert.trim().eq_ignore_ascii_case("initial") {
            return Some(anfang.as_deref().and_then(|a| narsil_css::cssom::wenn::nach_syntax_berechnet(syntax, a, u)));
        }
        Some(narsil_css::cssom::wenn::nach_syntax_berechnet(syntax, wert, u))
    }
    fn var_wert(&self, name: &str) -> Option<String> {
        self.wert_in(&self.vars, name, self.k)
    }
    fn eltern_var(&self, name: &str) -> Option<String> {
        let v = self.eltern_vars.as_ref()?;
        let attr = |_: &str| None;
        let w = ersetzen_kette(&format!("var({name})"), v, &attr, false)?;
        let w = w.trim().to_string();
        (!w.eq_ignore_ascii_case("initial")).then_some(w)
    }
    fn einsetzen(&self, text: &str) -> Option<String> {
        let attr = |n: &str| self.l.dok.attribut(self.k, n).map(str::to_string);
        let mut bz = std::collections::HashSet::new();
        bezuege(text, &self.vars, &attr, &mut bz, 0);
        if zyklus_pruefen(&bz, None) {
            return None;
        }
        // Release 13.10.2026: `inherit()` takes the parent's value.
        let text = narsil_css::ersetze_inherit(text, self.eltern_vars.as_ref())?;
        ersetzen_kette(&text, &self.vars, &attr, false)
    }
    fn medien(&self, abfrage: &str) -> bool {
        narsil_css::media_trifft(abfrage, self.satz.ansicht.0 as u32, self.satz.ansicht.1 as u32, self.satz.zustand.farbschema)
    }
    fn supports(&self, bedingung: &str) -> bool {
        narsil_css::supports_trifft(&format!("({bedingung})")) || narsil_css::supports_trifft(bedingung)
    }
    fn umgebung(&self) -> narsil_css::cssom::mathe::Umgebung {
        narsil_css::cssom::mathe::Umgebung { geschwister: geschwister_von(self.l, self.k), ..Default::default() }
    }
}

/// Release 14.10.2026: the selectors of a selector list that end in the
/// pseudo-element `::art` (whitespace-insensitive), without it — `li::marker`
/// gives `li`, `ul ::marker` gives `ul *`.
fn pseudo_selector_parts(list: &str, art: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in list.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&list[start..]);
    let suffix = format!("::{art}");
    parts
        .into_iter()
        .filter_map(|p| {
            let p = p.trim();
            let compact: String = p.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
            if !compact.ends_with(&suffix) {
                return None;
            }
            // The position of the `::` that starts the suffix in `p`.
            let cut = p.rfind("::")?;
            let base = &p[..cut];
            Some(if base.trim().is_empty() {
                String::from("*")
            } else if base.ends_with(char::is_whitespace) {
                format!("{} *", base.trim_end())
            } else {
                base.to_string()
            })
        })
        .collect()
}

/// Release 12.10.2026: (`sibling-index()`, `sibling-count()`) eines Elements
/// (CSS Values 5 §11) — fuer Mathe und `ident()`.
pub(crate) fn geschwister_von(l: &Lebend, k: usize) -> Option<(f64, f64)> {
    let e = eltern_dom(&l.dok, k)?;
    let kinder: Vec<usize> = l.dok.kinder(e).iter().copied().filter(|&x| l.dok.ist_element_knoten(x)).collect();
    let i = kinder.iter().position(|&x| x == k).map_or(1, |p| p + 1);
    Some((i as f64, kinder.len().max(1) as f64))
}

fn spez_kaskade(
    l: &Lebend,
    k: usize,
    name: &str,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
) -> Option<String> {
    if let Some(s) = stil_spez(l, k, name) {
        return Some(s);
    }
    let b = berechnet(l, k, satz, cache)?;
    // Eine Langform aus einer Inline-Kurzform mit `var()`: die Kurzform mit
    // den Variablen des Elements lesen (WPT css-variables
    // css-variable-change-style-001: `white-space: var(--x)`).
    if let Some((kurz, roh)) = stil_block(l, k).ausstehend(name) {
        let ersetzt = narsil_css::ersetze_var(&roh, &b.variablen, 0)?;
        let attr = |n: &str| l.dok.attribut(k, n).map(str::to_string);
        narsil_css::cssom::ersetzen::geschwister_setzen(geschwister_von(l, k));
        let ersetzt = narsil_css::cssom::ersetzen::alles_ersetzen(&ersetzt, &attr)?;
        let w = narsil_css::cssom::block::Block::lesen(&format!("{kurz}: {ersetzt}")).wert(name);
        return (!w.is_empty()).then_some(w);
    }
    if !b.schluessel.iter().any(|(n, _)| n == name)
        && !b.schluessel.iter().any(|(n, _)| narsil_css::cssom::kurz::langformen(n).is_some_and(|ls| ls.contains(&name)))
        && let Some(w) = spez_blaetter(l, k, name, &satz.zustand)
    {
        return Some(w);
    }
    // Release 12.10.2026: Langformen auch von `white-space` (das die Kaskade
    // nicht woertlich traegt): die Blaetter in Kaskadenordnung entscheiden.
    // GEMESSEN vorher (WPT css-text white-space-shorthand-text-wrap): `text-wrap:
    // nowrap balance; white-space: normal` ergab `nowrap balance`.
    if matches!(name, "text-wrap-mode" | "white-space-collapse")
        && let Some(w) = spez_blaetter(l, k, name, &satz.zustand)
    {
        return Some(w);
    }
    for (n, v) in b.schluessel.iter().rev() {
        if n == name {
            let v = v.trim();
            return (!v.is_empty()).then(|| v.to_string());
        }
        if narsil_css::cssom::kurz::langformen(n).is_some_and(|ls| ls.contains(&name)) {
            let w = narsil_css::cssom::block::Block::lesen(&format!("{n}: {v}")).wert(name);
            if !w.is_empty() {
                return Some(w);
            }
        }
    }
    None
}

thread_local! {
    /// Release 12.10.2026: die Stilregeln eines `<style>`-Texts (Selektoren,
    /// Block) — fuer [`spez_blaetter`], je Text einmal gelesen.
    static BLATT_REGELN: std::cell::RefCell<HashMap<u64, std::rc::Rc<Vec<(Vec<narsil_css::Selektor>, narsil_css::cssom::block::Block)>>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Release 12.10.2026: der gewonnene angegebene Wert einer Eigenschaft, die
/// die Kaskade gar nicht kennt (weder als Feld noch woertlich), aus den
/// obersten Stilregeln der `<style>`-Elemente (Wichtigkeit, Spezifitaet,
/// Reihenfolge; Regeln in `@media`/`@layer`/Verschachtelung zaehlen nicht).
/// GEMESSEN vorher (WPT css-values various-values-important,
/// tree-counting): `initial-letter: 123 !important` im Stylesheet kam als
/// Anfangswert.
fn spez_blaetter(l: &Lebend, k: usize, name: &str, zustand: &narsil_css::Zustand) -> Option<String> {
    spez_blaetter_mit(l, k, name, zustand, &[])
}

/// Release 12.10.2026: der berechnete Wert einer Eigenschaft des
/// Pseudoelements `::before`/`::after` von `k` (CSSOM §9 mit `pseudoElt`):
/// die gewonnene Angabe aus den Regeln mit diesem Pseudoelement (Element ohne
/// das Pseudoelement abgeglichen), geerbt vom Element selbst. GEMESSEN vorher
/// (WPT cssom getComputedStyle-pseudo): `getComputedStyle(el, '::before')`
/// lieferte das Stilbild des Elements.
/// Release 14.10.2026: `art` is any pseudo-element (`before`, `marker`,
/// `highlight(name)`, …); besides `::before`/`::after` the rules are matched
/// by the text of their selectors (`li::marker` → `li`). MEASURED before (WPT
/// cssom getComputedStyle-pseudo "Unknown pseudo-element with a known
/// identifier"): `::marker` and the others returned the element's style.
/// Release 15.10.2026: the properties that apply to `::first-letter`
/// (`art` "first-letter") or `::first-line` (CSS Pseudo 4 §3.4, §3.5).
fn first_letter_line_applies(art: &str, name: &str) -> bool {
    if name.starts_with("--") || name.starts_with("font") || name.starts_with("background") || name.starts_with("text-decoration") || name.starts_with("text-emphasis") {
        return true;
    }
    if matches!(
        name,
        "color" | "opacity" | "word-spacing" | "letter-spacing" | "text-transform" | "line-height" | "text-shadow" | "text-wrap" | "text-wrap-mode" | "text-wrap-style" | "white-space-collapse" | "font-variant" | "ruby-position" | "-webkit-text-fill-color" | "-webkit-text-stroke-color" | "-webkit-text-stroke-width"
    ) {
        return true;
    }
    art == "first-letter"
        && (name.starts_with("margin") || name.starts_with("padding") || name.starts_with("border") || matches!(name, "vertical-align" | "float" | "box-shadow" | "initial-letter" | "initial-letter-align"))
}

pub fn pseudo_berechnet(
    l: &Lebend,
    k: usize,
    art: &str,
    name: &str,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
) -> Option<String> {
    let name = narsil_css::cssom::block::name_kanonisch(name)?;
    if !narsil_css::cssom::ist_lang(&name) {
        return None;
    }
    let mut art: String = art.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    // An unclosed argument closes at the end (`::highlight(name`).
    if art.contains('(') && !art.ends_with(')') {
        art.push(')');
    }
    // Release 15.10.2026: a keyframe value computed in the pseudo-element's
    // context (`pseudo_value_of`) instead of its rules.
    let ersatz = PSEUDO_VALUE.with(|v| v.borrow().clone()).filter(|(n, _)| *n == name).map(|(_, w)| w);
    // Release 15.10.2026: an animated value of the pseudo-element
    // (`Lebend::animated_pseudo`; already a computed value).
    if ersatz.is_none()
        && let Some(werte) = l.animated_pseudo.get(&(k, art.clone()))
        && let Some((_, w)) = werte.iter().rev().find(|(n, _)| *n == name)
    {
        return Some(w.strip_suffix(" !important").unwrap_or(w).to_string());
    }
    let nachher = art == "after";
    let generic = !matches!(art.as_str(), "before" | "after");
    let art_root = art.split('(').next().unwrap_or_default().to_string();
    let marke = if nachher { narsil_css::Pseudo::Nachher } else { narsil_css::Pseudo::Vorher };
    // Die Regeln der Blaetter (wie `spez_blaetter_mit`), nur Selektoren mit der Marke.
    let mut texte = Vec::new();
    // Release 15.10.2026: on `::first-letter`/`::first-line` only some
    // properties apply (CSS Pseudo 4 §3.4, §3.5); for the others the rules of
    // the pseudo-element do not count. MEASURED before (WPT css-flexbox
    // flexbox_first-letter): `::first-letter { order: 2 }` gave `2`.
    let anwendbar = match art.as_str() {
        "first-letter" | "first-line" => first_letter_line_applies(&art, &name),
        _ => true,
    };
    let mut stapel = if anwendbar { vec![l.wurzel] } else { Vec::new() };
    let mut schritte = 0usize;
    while let Some(n) = stapel.pop() {
        schritte += 1;
        if schritte > 100_000 {
            break;
        }
        if l.dok.element_name(n).is_some_and(|e| e.eq_ignore_ascii_case("style")) {
            if !l.disabled_sheets.contains(&n) {
                texte.push(l.style_text(n));
            }
            continue;
        }
        for &c in l.dok.kinder(n).iter().rev() {
            stapel.push(c);
        }
    }
    let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: None, scope: KEIN_SCOPE };
    let mut best: Option<((bool, (u32, u32, u32), usize), String)> = None;
    let mut ordnung = 0usize;
    if let Some(w) = &ersatz {
        best = Some(((true, (u32::MAX, 0, 0), usize::MAX), w.clone()));
        texte.clear();
    }
    for text in texte {
        let text = match text.rfind(narsil_css::BLATT_GRENZE) {
            Some(i) => text[i + narsil_css::BLATT_GRENZE.len()..].to_string(),
            None => text,
        };
        if generic {
            if !text.to_ascii_lowercase().contains(&art_root) {
                continue;
            }
        } else if !text.contains("before") && !text.contains("after") {
            continue;
        }
        let ns = narsil_css::cssom::selektor::Namensraeume::default();
        for r in narsil_css::cssom::regeln::blatt_lesen(&text, &ns).into_iter().filter(|r| r.art == "stil").take(2_000) {
            ordnung += 1;
            let Some(block) = r.block else { continue };
            let w = block.wert(&name);
            if w.is_empty() {
                continue;
            }
            let mut spez: Option<(u32, u32, u32)> = None;
            if generic {
                for part in pseudo_selector_parts(&r.kopf, &art) {
                    for s in selektoren(&part) {
                        if narsil_css::selektor_passt_mit(&s, &el, &satz.zustand) {
                            let (a, b, c) = s.spezifitaet;
                            spez = spez.max(Some((a, b, c + 1)));
                        }
                    }
                }
            }
            for s in selektoren(&r.kopf).into_iter().filter(|_| !generic) {
                let Some(letzt) = s.kette.last() else { continue };
                if !letzt.einfach.pseudos.contains(&marke) {
                    continue;
                }
                let mut ohne = s.clone();
                if let Some(g) = ohne.kette.last_mut() {
                    g.einfach.pseudos.retain(|p| *p != marke);
                }
                if narsil_css::selektor_passt_mit(&ohne, &el, &satz.zustand) {
                    spez = spez.max(Some(s.spezifitaet));
                }
            }
            let Some(spez) = spez else { continue };
            let schl = (block.prioritaet(&name) == "important", spez, ordnung);
            if best.as_ref().is_none_or(|(b, _)| schl > *b) {
                best = Some((schl, w));
            }
        }
    }
    let spez = best.map(|(_, w)| w);
    // Geerbt wird vom Element selbst.
    let b = berechnet(l, k, satz, cache)?;
    let zelle = std::cell::RefCell::new(std::mem::take(cache));
    let eltern = |n: &str| -> Option<String> {
        let kaskade = narsil_css::berechnet_css::berechneter_wert(&b, n);
        let s = stil_spez(l, k, n);
        if kaskade.is_some() && s.is_none() {
            return kaskade;
        }
        cssom_rek(l, k, n, s, satz, &zelle, 1).or(kaskade)
    };
    let andere = |_: &str| -> Option<String> { None };
    let em = f64::from(b.schriftgroesse_px);
    let u = narsil_css::cssom::mathe::Umgebung {
        em,
        rem: 16.0,
        ex: em * 0.5,
        ch: em * 0.5,
        cap: em * 0.7,
        ic: em,
        lh: em * 1.2,
        rlh: 19.2,
        vw: f64::from(satz.ansicht.0),
        vh: f64::from(satz.ansicht.1),
        prozent: None,
        cq: (None, None),
        geschwister: geschwister_von(l, k),
    };
    let kontext = narsil_css::cssom::berechnet::Kontext {
        u,
        eltern_schrift: em,
        farbe: narsil_css::berechnet_css::berechneter_wert(&b, "color"),
        eltern: &eltern,
        andere: &andere,
        adresse: None,
    };
    let r = narsil_css::cssom::berechnet::berechnen(&name, spez.as_deref(), &kontext);
    *cache = zelle.into_inner();
    r
}

/// Release 12.10.2026: Eigenschaften, deren Wert aus den Blaettern vor dem
/// (verlustbehafteten) Kaskadenwert gilt.
const BLATT_ZUERST: &[&str] = &[
    "text-wrap-mode", "white-space-collapse", "justify-content", "align-content", "justify-items", "align-items",
    "justify-self", "align-self", "row-gap", "column-gap",
];

/// Werte mit diesen Funktionen verwirft die Kaskade (CSS Values 5) — nur
/// Blaetter mit einer davon werden fuer Eigenschaften gelesen, die die
/// Kaskade kennt.
/// Release 14.10.2026: also relative colors (`rgb(from red r g b)`), whose
/// computed value the cascade cannot serialize (`color(srgb …)`; WPT
/// css-values clamp-color-computed).
const NEUE_FUNKTIONEN: &[&str] = &["sibling-index(", "sibling-count(", "attr(", "ident(", "(from "];

thread_local! {
    /// Release 12.10.2026: die Dokumentbasis fuer Link-Adressen in
    /// `spez_blaetter_mit` (gesetzt von `cssom_berechnet`).
    static LINK_BASIS: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn spez_blaetter_mit(l: &Lebend, k: usize, name: &str, zustand: &narsil_css::Zustand, muss: &[&str]) -> Option<String> {
    let texte = blatt_arcs(l);
    spez_aus_texten(l, k, name, zustand, muss, &texte)
}

/// Release 15.10.2026: the texts of the style sheets in effect (`<style>`
/// elements, fetched `<link>` sheets), in document order.
fn blatt_texte(l: &Lebend) -> Vec<String> {
    blatt_arcs(l).iter().map(|t| t.to_string()).collect()
}

/// Release 15.10.2026: like [`blatt_texte`], but the fetched sheets are shared
/// (`Arc`), not copied — a sheet of several hundred KB was copied once per
/// property lookup (the base values of the animation module ask dozens of
/// properties per element).
fn blatt_arcs(l: &Lebend) -> Vec<std::sync::Arc<str>> {
    let mut texte: Vec<std::sync::Arc<str>> = Vec::new();
    let mut stapel = vec![l.wurzel];
    let mut schritte = 0usize;
    let mut link_basis: Option<String> = None;
    while let Some(n) = stapel.pop() {
        schritte += 1;
        if schritte > 100_000 {
            break;
        }
        if l.dok.element_name(n).is_some_and(|e| e.eq_ignore_ascii_case("style")) {
            if !l.disabled_sheets.contains(&n) {
                texte.push(std::sync::Arc::from(l.style_text(n)));
            }
            continue;
        }
        // Release 12.10.2026: auch geholte Link-Blaetter (aus dem Blattschrank).
        // GEMESSEN vorher (WPT css-values urls/resolve-relative-to-stylesheet,
        // css-align parse-*): Werte, die die Kaskade nicht kennt
        // (`background-image`, `justify-content: left`), fehlten aus <link>.
        if l.dok.element_name(n).is_some_and(|e| e.eq_ignore_ascii_case("link"))
            && l.dok.attribut(n, "rel").is_some_and(|r| r.split_ascii_whitespace().any(|w| w.eq_ignore_ascii_case("stylesheet")))
            && l.dok.attribut(n, "disabled").is_none()
            && let Some(href) = l.dok.attribut(n, "href")
        {
            if link_basis.is_none() {
                let dok = LINK_BASIS.with(|b| b.borrow().clone());
                link_basis = Some(if dok.is_empty() { dok } else { dokument_basis(l, &dok) });
            }
            let basis = link_basis.as_deref().unwrap_or("");
            let abs = narsil_url::loese_auf(basis, href.trim()).unwrap_or_else(|_| href.trim().to_string());
            if let Some(tx) = narsil_page::blattschrank::inhalt(&abs) {
                texte.push(tx);
            }
            continue;
        }
        for &c in l.dok.kinder(n).iter().rev() {
            stapel.push(c);
        }
    }
    texte
}

/// Release 15.10.2026: a fast 64-bit hash of a sheet text (all bytes, eight at
/// a time) for the rule cache — the standard hasher took ~1 ms per MB of CSS
/// on every property lookup.
fn text_hash(t: &str) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let b = t.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (b.len() as u64);
    let mut stuecke = b.chunks_exact(8);
    for c in &mut stuecke {
        let mut v = [0u8; 8];
        v.copy_from_slice(c);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(v)).wrapping_mul(K);
    }
    for &x in stuecke.remainder() {
        h = (h.rotate_left(5) ^ u64::from(x)).wrapping_mul(K);
    }
    h
}

fn spez_aus_texten(
    l: &Lebend,
    k: usize,
    name: &str,
    zustand: &narsil_css::Zustand,
    muss: &[&str],
    texte: &[std::sync::Arc<str>],
) -> Option<String> {
    const MAX_REGELN: usize = 2_000;
    let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: None, scope: KEIN_SCOPE };
    let mut best: Option<((bool, (u32, u32, u32), usize), String)> = None;
    let mut ordnung = 0usize;
    // Release 15.10.2026: the canonical name and its long forms once, not once
    // per rule (`Block::wert_und_prioritaet_kanonisch`).
    let kanonisch = narsil_css::cssom::block::name_kanonisch(name.trim())?;
    let langformen = narsil_css::cssom::kurz::langformen(&kanonisch);
    for t in texte {
        let info = blatt_info(t);
        // Release 12.10.2026: auch ueber eine Kurzform (`white-space` fuer
        // `text-wrap-mode`). Release 15.10.2026: einmal je Blatt und Name
        // gemerkt (`BlattInfo::genannt`).
        if !info.genannt(name, muss) {
            ordnung += 1;
            continue;
        }
        let regeln = BLATT_REGELN.with(|c| c.borrow().get(&info.hash).cloned()).unwrap_or_else(|| {
            let ns = narsil_css::cssom::selektor::Namensraeume::default();
            let r: Vec<_> = narsil_css::cssom::regeln::blatt_lesen(&info.text[info.start..], &ns)
                .into_iter()
                .filter(|r| r.art == "stil")
                .take(MAX_REGELN)
                .filter_map(|r| Some((selektoren(&r.kopf), r.block?)))
                .collect();
            let r = std::rc::Rc::new(r);
            BLATT_REGELN.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() > 64 {
                    c.clear();
                }
                c.insert(info.hash, r.clone());
            });
            r
        });
        // Only the rules that declare `name` (found once per sheet and name),
        // numbered by their position in the sheet — the order of the cascade.
        let treffer = info.treffer(name, &regeln, &kanonisch, langformen);
        for (nr, w, wichtig) in treffer.iter() {
            let Some((sel, _)) = regeln.get(nr - 1) else { continue };
            // Release 13.10.2026: including `@scope` proximity.
            let Some(spez) = sel.iter().filter_map(|s| narsil_css::matches_with_proximity(s, &el, zustand)).max() else {
                continue;
            };
            let schl = (*wichtig, spez, ordnung + nr);
            if best.as_ref().is_none_or(|(b, _)| schl > *b) {
                best = Some((schl, w.clone()));
            }
        }
        ordnung += regeln.len();
    }
    best.map(|(_, w)| w)
}

type BlattRegeln = Vec<(Vec<narsil_css::Selektor>, narsil_css::cssom::block::Block)>;

/// Release 15.10.2026: what is worked out once per style sheet text (not per
/// property lookup): where the text proper starts, its hash, whether it
/// mentions a property name, and which of its rules declare that name.
/// MEASURED (chip.de, `__narsilBasiswerte`, 100 calls): scanning, hashing and
/// walking every rule of ~16 sheets per property took 22 ms per call.
struct BlattInfo {
    text: std::sync::Arc<str>,
    start: usize,
    hash: u64,
    genannt: std::cell::RefCell<HashMap<(String, usize), bool>>,
    treffer: std::cell::RefCell<HashMap<String, std::rc::Rc<Vec<(usize, String, bool)>>>>,
}

impl BlattInfo {
    fn genannt(&self, name: &str, muss: &[&str]) -> bool {
        let schluessel = (name.to_string(), muss.len());
        if let Some(b) = self.genannt.borrow().get(&schluessel) {
            return *b;
        }
        let text = &self.text[self.start..];
        let genannt = text.contains(name)
            || narsil_css::cssom::kurz::KURZ.iter().any(|(k, ls)| ls.contains(&name) && text.contains(k));
        let ok = genannt && (muss.is_empty() || muss.iter().any(|f| text.contains(f)));
        self.genannt.borrow_mut().insert(schluessel, ok);
        ok
    }

    fn treffer(
        &self,
        name: &str,
        regeln: &BlattRegeln,
        kanonisch: &str,
        langformen: Option<&'static [&'static str]>,
    ) -> std::rc::Rc<Vec<(usize, String, bool)>> {
        if let Some(t) = self.treffer.borrow().get(name) {
            return t.clone();
        }
        let t: Vec<(usize, String, bool)> = regeln
            .iter()
            .enumerate()
            .filter_map(|(i, (_, block))| {
                let (w, wichtig) = block.wert_und_prioritaet_kanonisch(name, kanonisch, langformen);
                (!w.is_empty()).then_some((i + 1, w, wichtig))
            })
            .collect();
        let t = std::rc::Rc::new(t);
        self.treffer.borrow_mut().insert(name.to_string(), t.clone());
        t
    }
}

thread_local! {
    /// Release 15.10.2026: `BlattInfo` per sheet text, by the address and
    /// length of the shared text (the entry keeps the `Arc` alive, so the
    /// address cannot be reused while the entry exists).
    static BLATT_INFO: std::cell::RefCell<HashMap<(usize, usize), std::rc::Rc<BlattInfo>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn blatt_info(t: &std::sync::Arc<str>) -> std::rc::Rc<BlattInfo> {
    let schluessel = (std::sync::Arc::as_ptr(t) as *const u8 as usize, t.len());
    if let Some(i) = BLATT_INFO.with(|m| m.borrow().get(&schluessel).cloned()) {
        return i;
    }
    let start = t.rfind(narsil_css::BLATT_GRENZE).map_or(0, |i| i + narsil_css::BLATT_GRENZE.len());
    let info = std::rc::Rc::new(BlattInfo {
        text: t.clone(),
        start,
        hash: text_hash(&t[start..]),
        genannt: std::cell::RefCell::new(HashMap::new()),
        treffer: std::cell::RefCell::new(HashMap::new()),
    });
    BLATT_INFO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 256 {
            m.clear();
        }
        m.insert(schluessel, info.clone());
    });
    info
}

/// Release 15.10.2026 (Agent C): the value of `name` in the starting style
/// of element `k` (CSS Transitions 2 §3.1 `@starting-style`): the winner
/// among the style rules and the rules inside `@starting-style` (also
/// nested in a style rule), when it is one of the latter; `None` otherwise.
/// Conditions of `@media`/`@supports` around the rules are not checked.
thread_local! {
    /// Release 15.10.2026: (name, value) that `pseudo_berechnet` computes in
    /// place of the rules (`pseudo_value_of`).
    static PSEUDO_VALUE: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

/// Release 15.10.2026: the computed value of `value` for `name` on the
/// pseudo-element `art` of element `k` (`em`, `%`, `inherit` against the
/// element).
pub(crate) fn pseudo_value_of(
    l: &Lebend,
    k: usize,
    art: &str,
    name: &str,
    value: &str,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
) -> Option<String> {
    PSEUDO_VALUE.with(|v| *v.borrow_mut() = Some((name.to_string(), value.to_string())));
    let r = pseudo_berechnet(l, k, art, name, satz, cache);
    PSEUDO_VALUE.with(|v| *v.borrow_mut() = None);
    r
}

/// Release 15.10.2026: the style rules that give a pseudo-element
/// (`::before`, `::after`, `::marker`) animation or transition properties:
/// the selectors of the originating element and the pseudo-element name.
pub(crate) fn pseudo_animation_rules(l: &Lebend) -> Vec<(Vec<narsil_css::Selektor>, &'static str)> {
    let texte: Vec<String> = blatt_texte(l)
        .into_iter()
        .filter(|t| (t.contains("animation") || t.contains("transition")) && (t.contains(":before") || t.contains(":after") || t.contains("::marker")))
        .collect();
    let ns = narsil_css::cssom::selektor::Namensraeume::default();
    let mut out = Vec::new();
    fn gehen(regeln: &[narsil_css::cssom::regeln::Regel], out: &mut Vec<(Vec<narsil_css::Selektor>, &'static str)>, tiefe: usize) {
        for r in regeln {
            match r.art {
                "stil" => {
                    let Some(block) = &r.block else { continue };
                    let animiert = ["animation-name", "transition-property", "transition-duration"].iter().any(|n| !block.wert(n).is_empty());
                    if !animiert {
                        continue;
                    }
                    // `:before`/`:after` (CSS 2) like `::before`/`::after`.
                    let kopf = r.kopf.replace("::before", ":before").replace("::after", ":after").replace(":before", "::before").replace(":after", "::after");
                    for art in ["before", "after", "marker"] {
                        let sels: Vec<narsil_css::Selektor> = pseudo_selector_parts(&kopf, art).iter().flat_map(|p| selektoren(p)).collect();
                        if !sels.is_empty() {
                            out.push((sels, art));
                        }
                    }
                }
                "media" | "supports" | "layer-block" | "container" | "scope" if tiefe < 8 => {
                    if let Some(kinder) = &r.kinder {
                        gehen(kinder, out, tiefe + 1);
                    }
                }
                _ => {}
            }
        }
    }
    for text in texte {
        gehen(&narsil_css::cssom::regeln::blatt_lesen(&text, &ns), &mut out, 0);
    }
    out
}

/// Release 15.10.2026: does the element match one of the selectors?
pub(crate) fn element_matches(l: &Lebend, k: usize, sels: &[narsil_css::Selektor], zustand: &narsil_css::Zustand) -> bool {
    let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: None, scope: KEIN_SCOPE };
    sels.iter().any(|s| narsil_css::selektor_passt_mit(s, &el, zustand))
}

pub(crate) fn has_starting_style(l: &Lebend) -> bool {
    blatt_texte(l).iter().any(|t| t.contains("@starting-style"))
}

pub(crate) fn starting_style_value(l: &Lebend, k: usize, name: &str, zustand: &narsil_css::Zustand) -> Option<String> {
    let texte: Vec<String> = blatt_texte(l).into_iter().filter(|t| t.contains("@starting-style")).collect();
    if texte.is_empty() {
        return None;
    }
    let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: None, scope: KEIN_SCOPE };
    let ns = narsil_css::cssom::selektor::Namensraeume::default();
    // (important, specificity, order) -> (value, from @starting-style)
    let mut best: Option<((bool, (u32, u32, u32), usize), String, bool)> = None;
    let mut ordnung = 0usize;
    fn gehen(
        regeln: &[narsil_css::cssom::regeln::Regel],
        start: bool,
        eltern_sel: Option<&str>,
        el: &ArenaEl,
        name: &str,
        zustand: &narsil_css::Zustand,
        ordnung: &mut usize,
        best: &mut Option<((bool, (u32, u32, u32), usize), String, bool)>,
        tiefe: usize,
    ) {
        if tiefe > 8 {
            return;
        }
        for r in regeln {
            *ordnung += 1;
            match r.art {
                "stil" | "nested-declarations" => {
                    let kopf = if r.art == "stil" { r.kopf.as_str() } else { eltern_sel.unwrap_or("") };
                    if let Some(block) = &r.block {
                        let w = block.wert(name);
                        if !w.is_empty()
                            && let Some(spez) = selektoren(kopf).iter().filter_map(|s| narsil_css::matches_with_proximity(s, el, zustand)).max()
                        {
                            let schl = (block.prioritaet(name) == "important", spez, *ordnung);
                            if best.as_ref().is_none_or(|(b, _, _)| schl > *b) {
                                *best = Some((schl, w, start));
                            }
                        }
                    }
                    if let Some(kinder) = &r.kinder {
                        gehen(kinder, start, Some(if r.art == "stil" { &r.kopf } else { kopf }), el, name, zustand, ordnung, best, tiefe + 1);
                    }
                }
                "starting-style" => {
                    if let Some(kinder) = &r.kinder {
                        gehen(kinder, true, eltern_sel, el, name, zustand, ordnung, best, tiefe + 1);
                    }
                }
                "media" | "supports" | "layer-block" | "container" | "scope" => {
                    if let Some(kinder) = &r.kinder {
                        gehen(kinder, start, eltern_sel, el, name, zustand, ordnung, best, tiefe + 1);
                    }
                }
                _ => {}
            }
        }
    }
    for text in texte {
        let regeln = narsil_css::cssom::regeln::blatt_lesen(&text, &ns);
        gehen(&regeln, false, None, &el, name, zustand, &mut ordnung, &mut best, 0);
    }
    best.filter(|(_, _, start)| *start).map(|(_, w, _)| w)
}

/// Release 12.10.2026: Eigenschaften, deren aufgeloester Wert der GENUTZTE
/// ist (CSSOM §9 „resolved value special case"): sie bleiben bei der Kaskade
/// bzw. dem Layout. `top`/`right`/`bottom`/`left` nur, wenn das Element
/// positioniert ist (sonst gilt der berechnete Wert).
const AUFGELOEST: &[&str] = &[
    "line-height", "width", "height", "block-size", "inline-size", "margin-top", "margin-right", "margin-bottom",
    "margin-left", "margin-block-start", "margin-block-end", "margin-inline-start", "margin-inline-end", "padding-top",
    "padding-right", "padding-bottom", "padding-left", "padding-block-start", "padding-block-end", "padding-inline-start",
    "padding-inline-end", "top", "right", "bottom", "left", "inset-block-start", "inset-block-end", "inset-inline-start",
    "inset-inline-end", "transform",
];

/// Release 12.10.2026: der Kaskadenwert der Eigenschaft, wenn NUR die
/// Inline-Angabe gaelte (ohne Stylesheets) — stimmt er mit dem echten
/// Kaskadenwert ueberein, hat die Inline-Angabe gewonnen (kein `!important`
/// im Stylesheet dagegen), und ihr CSSOM-berechneter Wert ist der genauere.
fn kaskade_isoliert(
    l: &Lebend,
    k: usize,
    name: &str,
    spez: &str,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
) -> Option<String> {
    let eltern = {
        let mut x = l.dok.eltern(k);
        while let Some(e) = x {
            if l.dok.ist_element_knoten(e) {
                break;
            }
            x = l.dok.eltern(e);
        }
        x
    };
    let eb = eltern.and_then(|e| berechnet(l, e, satz, cache));
    // Release 12.10.2026: die echte Kaskade (mit allen Blaettern), nur mit
    // JEDER Inline-Angabe als `!important` — gleicht sie der echten, hat
    // kein Stylesheet die Inline-Angabe ueberstimmt. GEMESSEN vorher (WPT
    // webkit-box-computed): der Lauf ohne Blaetter kannte die UA-Werte
    // nicht (`display` eines `div` war dort `inline`), und ein Wert, den
    // die Kaskade nicht versteht (`-webkit-flex`), galt als ueberstimmt.
    let _ = spez;
    let stil_dom = l.dok.attribut(k, "style").unwrap_or("");
    // Release 15.10.2026: with the animated values (`Lebend::animated`).
    let mit_anim;
    let stil = match l.animated.get(&k) {
        Some(werte) if !werte.is_empty() => {
            mit_anim = format!("{stil_dom}{}", animated_declarations(werte));
            mit_anim.as_str()
        }
        _ => stil_dom,
    };
    let mut inline = narsil_css::inline_lesen(stil);
    for d in &mut inline {
        d.wichtig = true;
    }
    let mut vars = narsil_css::inline_variablen(stil);
    for v in &mut vars {
        v.wichtig = true;
    }
    let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: None, scope: KEIN_SCOPE };
    let b = narsil_css::berechnen_mit_vormerker(
        &satz.vormerker,
        &satz.blaetter,
        &el,
        &inline,
        &vars,
        eb.as_ref(),
        satz.ansicht.0,
        satz.ansicht.1,
        &satz.zustand,
    );
    narsil_css::berechnet_css::berechneter_wert(&b, name)
}

/// Release 12.10.2026: der berechnete Wert ueber das CSSOM-Modul
/// (`narsil_css::cssom::berechnet`), wenn die Kaskade die Eigenschaft nicht
/// kennt (`kaskade_kennt == false`) oder sie verlustbehaftet speichert und
/// das Element sie im `style`-Attribut setzt. Vererbte Eigenschaften holen
/// den Wert der Eltern auf demselben Weg.
pub fn cssom_berechnet(
    l: &Lebend,
    k: usize,
    name: &str,
    kaskade: Option<&str>,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
) -> Option<String> {
    let name = narsil_css::cssom::block::name_kanonisch(name)?;
    if !narsil_css::cssom::ist_lang(&name) {
        return None;
    }
    if !satz.basis.is_empty() {
        LINK_BASIS.with(|x| {
            if *x.borrow() != satz.basis {
                *x.borrow_mut() = satz.basis.clone();
            }
        });
    }
    let mut spez = if kaskade.is_none() { spez_kaskade(l, k, &name, satz, cache) } else { stil_spez(l, k, &name) };
    // Release 12.10.2026: ein Stylesheet-Wert, den die Kaskade verworfen hat
    // (`z-index: sibling-index()`), zaehlt als angegebener Wert (WPT
    // css-values tree-counting).
    let mut aus_blatt = false;
    // Release 12.10.2026: die Langformen von `white-space`, die die Kaskade
    // woertlich nur aus `text-wrap` kennt — die Blaetter in Kaskadenordnung.
    // Ebenso die Ausrichtung (die Kaskade kennt `left`/`stretch`/`normal` nicht
    // getreu; WPT css-align parse-*, place-*).
    if spez.is_none()
        && kaskade.is_some()
        && BLATT_ZUERST.contains(&name.as_str())
        && let Some(w) = spez_blaetter(l, k, &name, &satz.zustand)
    {
        spez = Some(w);
        aus_blatt = true;
    }
    // Woertlich gespeicherte Kaskadenwerte (`Berechnet::schluessel`) SIND der
    // angegebene Wert — mit dem CSSOM berechnet (WPT css-values
    // calc-in-font-feature-settings: `"vert" calc(2)` -> `"vert" 2`).
    if spez.is_none()
        && let Some(kw) = kaskade
        && berechnet(l, k, satz, cache).is_some_and(|b| b.schluessel.iter().any(|(n, _)| n == &name))
    {
        spez = Some(kw.to_string());
        aus_blatt = true;
    }
    if spez.is_none()
        && kaskade.is_some()
        && let Some(w) = spez_blaetter_mit(l, k, &name, &satz.zustand, NEUE_FUNKTIONEN)
        && NEUE_FUNKTIONEN.iter().any(|f| w.to_ascii_lowercase().contains(f))
    {
        spez = Some(w);
        aus_blatt = true;
    }
    // Release 12.10.2026: Eckenradien mit Prozent bleiben im berechneten
    // (= aufgeloesten) Wert Prozent (CSS Backgrounds 3 §5.1) — die Kaskade
    // rechnet sie in Pixel. GEMESSEN vorher (WPT css-values
    // getComputedStyle-border-radius-001): `calc(10px + 25%)` kam als `0px`.
    if spez.is_none()
        && kaskade.is_some()
        && name.starts_with("border-")
        && name.ends_with("-radius")
        && let Some(w) = spez_blaetter(l, k, &name, &satz.zustand)
        && w.contains('%')
    {
        spez = Some(w);
        aus_blatt = true;
    }
    // Release 12.10.2026: `attr()` kennen Kaskade und Layout nicht — dann
    // gilt der berechnete Wert, wenn er eine feste Laenge ist (fuer die
    // Eigenschaften mit genutztem Wert gleich ihm, z. B. `width: 3em`).
    // Eine Langform aus einer Inline-Kurzform mit Ersetzungsfunktion (`margin:
    // random-item(…)`) auch dort, wo die Kaskade die Eigenschaft kennt.
    let mut aus_kurzform = false;
    if spez.is_none() && kaskade.is_some() && stil_block(l, k).ausstehend(&name).is_some() {
        spez = spez_kaskade(l, k, &name, satz, cache);
        aus_kurzform = spez.is_some();
    }
    let mit_attr = aus_kurzform
        || spez.as_deref().is_some_and(|s| {
            let s = s.to_ascii_lowercase();
            s.contains("attr(") || s.contains("random-item(")
        });
    // `var()` mit den Variablen des Elements einsetzen (sonst rechnet nur die
    // Kaskade, und die klemmt z. B. `z-index` — WPT serialize-custom-props).
    // Release 12.10.2026: auch `ident()` (WPT ident-function-computed: `ident(5px)` ist `none`).
    let mit_attr_roh = spez.as_deref().is_some_and(|s| {
        let s = s.to_ascii_lowercase();
        s.contains("attr(") || s.contains("ident(")
    });
    if let Some(s) = spez.clone().as_deref()
        && narsil_css::cssom::block::hat_ersetzung(&narsil_css::cssom::syntax::komponenten(s))
    {
        // Release 12.10.2026: `if()` zuerst (`narsil_css::cssom::wenn`).
        let s_wenn;
        let s: &str = if s.to_ascii_lowercase().contains("if(")
            && let Some(lw) = LebendWenn::neu(l, k, satz, cache)
        {
            s_wenn = narsil_css::cssom::wenn::ersetzen(s, &lw).unwrap_or_default();
            &s_wenn
        } else {
            s
        };
        // Release 12.10.2026: `inherit()` mit den Variablen der Eltern
        // (CSS Values 5 §7.4; WPT inherit-function-basic).
        let mit_erben = if narsil_css::cssom::syntax::enthaelt_funk(&narsil_css::cssom::syntax::komponenten(s), "inherit") {
            let mut e = l.dok.eltern(k);
            while let Some(x) = e {
                if l.dok.ist_element_knoten(x) {
                    break;
                }
                e = l.dok.eltern(x);
            }
            let ev = e.and_then(|x| berechnet(l, x, satz, cache)).map(|pb| pb.variablen.clone());
            narsil_css::ersetze_inherit(s, ev.as_ref())
        } else {
            Some(s.to_string())
        };
        spez = mit_erben
            .and_then(|s| berechnet(l, k, satz, cache).map(|b| (s, b)))
            .and_then(|(s, b)| narsil_css::ersetze_var(&s, &b.variablen, 0))
            .map(|t| t.trim().to_string())
            // Release 12.10.2026: `attr()` mit den Attributen des Elements
            // (`narsil_css::cssom::ersetzen`).
            .and_then(|t| {
                let attr = |n: &str| l.dok.attribut(k, n).map(str::to_string);
                narsil_css::cssom::ersetzen::geschwister_setzen(geschwister_von(l, k));
                narsil_css::cssom::ersetzen::alles_ersetzen(&t, &attr)
            })
            .filter(|t| !narsil_css::cssom::block::hat_ersetzung(&narsil_css::cssom::syntax::komponenten(t)));
        // Release 12.10.2026: ungueltig zur Berechnungszeit (IACVT) heisst
        // `unset` (CSS Variables 1 §3.1) — nicht der Kaskadenwert, wenn die
        // Ersetzung an attr() scheiterte (WPT attr-universal-selector).
        if spez.is_none() && mit_attr_roh {
            // Fuer genutzte Werte rechnet das Layout (dort gilt ebenfalls `unset`).
            if AUFGELOEST.contains(&name.as_str()) {
                return None;
            }
            let zelle = std::cell::RefCell::new(std::mem::take(cache));
            let r = cssom_rek(l, k, &name, Some(String::from("unset")), satz, &zelle, 0);
            *cache = zelle.into_inner();
            return r;
        }
        spez.as_ref()?;
    }
    // Release 12.10.2026: der aufgeloeste Wert von `transform` ist die Matrix
    // (CSSOM §9) — aus dem Kaskadenwert, auch aus einem Stylesheet. Mit
    // Prozenten (Bezug Rahmenbox) bleibt es beim Kaskadenwert.
    if name == "transform"
        && let Some(kw) = kaskade
    {
        if narsil_css::cssom::block::hat_ersetzung(&narsil_css::cssom::syntax::komponenten(kw)) {
            return None;
        }
        let zelle = std::cell::RefCell::new(std::mem::take(cache));
        let r = cssom_rek(l, k, &name, Some(kw.to_string()), satz, &zelle, 0);
        *cache = zelle.into_inner();
        return r.filter(|t| t == "none" || t.starts_with("matrix"));
    }
    if let Some(kw) = kaskade.filter(|_| !aus_blatt) {
        // Die Kaskade kennt die Eigenschaft: nur wenn die Inline-Angabe
        // gewonnen hat und kein genutzter Wert gefragt ist.
        let s = spez.as_deref()?;
        if AUFGELOEST.contains(&name.as_str()) && mit_attr && name != "transform" {
            let zelle = std::cell::RefCell::new(std::mem::take(cache));
            let r = cssom_rek(l, k, &name, spez.clone(), satz, &zelle, 0);
            *cache = zelle.into_inner();
            return r.filter(|t| t.ends_with("px") && !t.contains('('));
        }
        // Release 12.10.2026: ohne Kasten (`display: none`, auch an einem
        // Vorfahren) ist der aufgeloeste Wert der berechnete (CSSOM §9; WPT
        // getComputedStyle-math-function-percentage).
        let ohne_kasten = {
            let mut x = Some(k);
            let mut aus = false;
            while let Some(e) = x {
                if l.dok.ist_element_knoten(e) && berechnet(l, e, satz, cache).is_some_and(|b| b.anzeige == narsil_css::Anzeige::Keins) {
                    aus = true;
                    break;
                }
                x = l.dok.eltern(e);
            }
            aus
        };
        if AUFGELOEST.contains(&name.as_str()) && !ohne_kasten {
            let statisch = berechnet(l, k, satz, cache).is_some_and(|b| b.position == narsil_css::Position::Statisch);
            if !(statisch
                && matches!(
                    name.as_str(),
                    "top" | "right" | "bottom" | "left" | "inset-block-start" | "inset-block-end" | "inset-inline-start" | "inset-inline-end"
                ))
            {
                return None;
            }
        }
        if narsil_css::cssom::block::hat_ersetzung(&narsil_css::cssom::syntax::komponenten(s)) {
            return None;
        }
        if kaskade_isoliert(l, k, &name, s, satz, cache).as_deref() != Some(kw) {
            return None;
        }
    }
    let zelle = std::cell::RefCell::new(std::mem::take(cache));
    let mut r = cssom_rek(l, k, &name, spez, satz, &zelle, 0);
    // Ein mit attr() eingesetzter Wert, der fuer die Eigenschaft nicht taugt,
    // ist ungueltig zur Berechnungszeit: `unset` (WPT attr-universal-selector).
    if r.is_none() && mit_attr_roh {
        r = cssom_rek(l, k, &name, Some(String::from("unset")), satz, &zelle, 0);
    }
    *cache = zelle.into_inner();
    // Release 12.10.2026: `display: math` gilt nur fuer MathML-Elemente,
    // sonst ist es `flow` (CSS Display 3 §2.1; WPT
    // display-math-on-non-mathml-elements).
    if name == "display"
        && !matches!(l.dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::MathMl, .. }))
    {
        return r.map(|t| match t.as_str() {
            "math" | "inline math" => String::from("inline"),
            "block math" => String::from("block"),
            _ => t,
        });
    }
    r
}

thread_local! {
    /// Release 12.10.2026: der Bezug fuer `%` des gefragten Werts (die
    /// Inhaltsbreite des umgebenden Blocks, nur fuer Rand/Polster; siehe
    /// [`cssom_genutzt`]).
    static PROZENT_BASIS: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
    /// Release 12.10.2026: die Masse des naechsten Groessen-Containers fuer
    /// `cqw`/`cqh` (setzt `dom::stil_berechnet` aus dem Layout).
    static BEHAELTER: std::cell::Cell<(Option<f64>, Option<f64>)> = const { std::cell::Cell::new((None, None)) };
}

/// Release 12.10.2026: die Containermasse fuer die naechste Rechnung setzen.
pub fn setze_behaelter(cq: (Option<f64>, Option<f64>)) {
    BEHAELTER.with(|b| b.set(cq));
}

/// Release 12.10.2026: `margin-*`/`padding-*` mit Prozenten — der
/// aufgeloeste Wert ist der genutzte (CSSOM §9), Prozente gegen die
/// Inhaltsbreite `basis` des umgebenden Blocks (CSS 2.1 §8.3, §8.4).
/// GEMESSEN vorher (WPT cssom getComputedStyle-calc, css-values
/// minmax-length-percent-computed): `calc(100% - 40px)` rechnete gegen die
/// Ansicht (760px statt 160px).
pub fn cssom_genutzt(
    l: &Lebend,
    k: usize,
    name: &str,
    kaskade: Option<&str>,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
    basis: f64,
) -> Option<String> {
    let spez = stil_spez(l, k, name)?;
    if !spez.contains('%') || narsil_css::cssom::block::hat_ersetzung(&narsil_css::cssom::syntax::komponenten(&spez)) {
        return None;
    }
    if let Some(kw) = kaskade
        && kaskade_isoliert(l, k, name, &spez, satz, cache).as_deref() != Some(kw)
    {
        return None;
    }
    PROZENT_BASIS.with(|p| p.set(Some(basis)));
    let zelle = std::cell::RefCell::new(std::mem::take(cache));
    let r = cssom_rek(l, k, name, Some(spez), satz, &zelle, 0);
    *cache = zelle.into_inner();
    PROZENT_BASIS.with(|p| p.set(None));
    r.filter(|t| t.ends_with("px") && !t.contains('('))
}

/// Release 12.10.2026: ein angegebener Laengenwert als px mit `basis` als
/// Bezug fuer `%` (fuer die Versaetze `top`/`left` …). `None` = keine feste
/// Laenge (`auto`, unbekannt).
pub fn laenge_mit_basis(
    l: &Lebend,
    k: usize,
    name: &str,
    spez: &str,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
    basis: f64,
) -> Option<f64> {
    if spez.trim().eq_ignore_ascii_case("auto") || narsil_css::cssom::block::hat_ersetzung(&narsil_css::cssom::syntax::komponenten(spez)) {
        return None;
    }
    PROZENT_BASIS.with(|p| p.set(Some(basis)));
    let zelle = std::cell::RefCell::new(std::mem::take(cache));
    let r = cssom_rek(l, k, name, Some(spez.to_string()), satz, &zelle, 0);
    *cache = zelle.into_inner();
    PROZENT_BASIS.with(|p| p.set(None));
    let r = r?;
    if r.contains('(') {
        return None;
    }
    r.strip_suffix("px")?.parse::<f64>().ok()
}

fn cssom_rek(
    l: &Lebend,
    k: usize,
    name: &str,
    spez: Option<String>,
    satz: &narsil_page::StilSatz,
    cache: &std::cell::RefCell<HashMap<usize, narsil_css::Berechnet>>,
    tiefe: usize,
) -> Option<String> {
    if tiefe > 128 {
        return None;
    }
    let b = berechnet(l, k, satz, &mut cache.borrow_mut())?;
    let eltern_k = {
        let mut x = l.dok.eltern(k);
        while let Some(e) = x {
            if l.dok.ist_element_knoten(e) {
                break;
            }
            x = l.dok.eltern(e);
        }
        x
    };
    let wurzel_el = l.dok.kinder(l.wurzel).iter().copied().find(|&e| l.dok.ist_element_knoten(e));
    // Release 12.10.2026: `font-size` der Wurzel selbst rechnet `rem` mit dem
    // Anfangswert (CSS Values 4 §6.1.1; WPT css-values rem-unit-root-element).
    let wurzel_schrift = if wurzel_el == Some(k) && name == "font-size" {
        16.0
    } else {
        wurzel_el.and_then(|e| berechnet(l, e, satz, &mut cache.borrow_mut())).map_or(16.0, |r| f64::from(r.schriftgroesse_px))
    };
    let eltern_schrift = eltern_k
        .and_then(|e| berechnet(l, e, satz, &mut cache.borrow_mut()))
        .map_or(16.0, |r| f64::from(r.schriftgroesse_px));
    // Release 14.10.2026: font-relative units in the font longhands refer to
    // the parent's font (CSS Values 4 §6.1.1; `font-size` handles that
    // itself). MEASURED before (WPT css-values
    // using-font-relative-units-in-font-properties): `font-weight: calc(1em /
    // 1px)` used the own 100px.
    let em = if name.starts_with("font-") && name != "font-size" { eltern_schrift } else { f64::from(b.schriftgroesse_px) };
    // Release 14.10.2026: `lh`/`rlh` in `font-size` and `line-height` are the
    // parent's / the initial line height; elsewhere the own / the root's
    // (CSS Values 4 §6.1.1). MEASURED before (WPT css-values
    // lh-rlh-on-root-001): `font-size: 1lh; line-height: 142px` gave 142px.
    let initial_line = 16.0 * 1.2;
    let line_of = |e: Option<usize>| -> f64 {
        e.and_then(|e| berechnet(l, e, satz, &mut cache.borrow_mut()))
            .map_or(initial_line, |r| if r.zeilenhoehe_px > 0.0 { f64::from(r.zeilenhoehe_px) } else { initial_line })
    };
    let font_props = matches!(name, "font-size" | "line-height");
    let lh = if font_props {
        line_of(eltern_k)
    } else if b.zeilenhoehe_px > 0.0 {
        f64::from(b.zeilenhoehe_px)
    } else {
        em * 1.2
    };
    let rlh = if font_props && wurzel_el == Some(k) { initial_line } else { line_of(wurzel_el) };
    let u = narsil_css::cssom::mathe::Umgebung {
        em,
        rem: wurzel_schrift,
        ex: em * 0.5,
        ch: em * 0.5,
        cap: em * 0.7,
        ic: em,
        lh,
        rlh,
        vw: f64::from(satz.ansicht.0),
        vh: f64::from(satz.ansicht.1),
        prozent: if tiefe == 0 { PROZENT_BASIS.with(std::cell::Cell::get) } else { None },
        cq: if tiefe == 0 { BEHAELTER.with(std::cell::Cell::get) } else { (None, None) },
        geschwister: geschwister_von(l, k),
    };
    let eltern = |n: &str| -> Option<String> {
        let e = eltern_k?;
        let kaskade = berechnet(l, e, satz, &mut cache.borrow_mut()).and_then(|be| narsil_css::berechnet_css::berechneter_wert(&be, n));
        let mut s = if kaskade.is_none() { spez_kaskade(l, e, n, satz, &mut cache.borrow_mut()) } else { stil_spez(l, e, n) };
        // Release 12.10.2026: wie in `cssom_berechnet` — Ausrichtung und die
        // white-space-Langformen der Eltern aus den Blaettern (WPT css-align
        // place-*-shorthand-005: `inherit` sah `flex-start` statt `start`).
        if s.is_none() && kaskade.is_some() && BLATT_ZUERST.contains(&n) {
            s = spez_blaetter(l, e, n, &satz.zustand);
        }
        if kaskade.is_some() && s.is_none() {
            return kaskade;
        }
        cssom_rek(l, e, n, s, satz, cache, tiefe + 1).or(kaskade)
    };
    let andere = |n: &str| -> Option<String> {
        stil_spez(l, k, n).or_else(|| berechnet(l, k, satz, &mut cache.borrow_mut()).and_then(|bb| narsil_css::berechnet_css::berechneter_wert(&bb, n)))
    };
    // Release 12.10.2026: `url()` im berechneten Wert absolut (CSS Values 4
    // §4.5.1); Nur-Fragment und leere Adressen bleiben. GEMESSEN vorher (WPT
    // css-backgrounds border-image-source-computed): `url("a.b#c")` blieb relativ.
    let basis = if satz.basis.is_empty() { String::new() } else { dokument_basis(l, &satz.basis) };
    let adresse = |u: &str| -> String {
        if u.is_empty() || u.starts_with('#') || basis.is_empty() {
            return u.to_string();
        }
        narsil_url::loese_auf(&basis, u).unwrap_or_else(|_| u.to_string())
    };
    let kontext = narsil_css::cssom::berechnet::Kontext {
        u,
        eltern_schrift,
        farbe: narsil_css::berechnet_css::berechneter_wert(&b, "color"),
        eltern: &eltern,
        andere: &andere,
        adresse: Some(&adresse),
    };
    narsil_css::cssom::berechnet::berechnen(name, spez.as_deref(), &kontext)
}

pub fn berechnet(
    l: &Lebend,
    k: usize,
    satz: &narsil_page::StilSatz,
    cache: &mut HashMap<usize, narsil_css::Berechnet>,
) -> Option<narsil_css::Berechnet> {
    const MAX_CACHE: usize = 4096;
    if !l.dok.ist_element_knoten(k) {
        return None;
    }
    let mut kette = Vec::new();
    let mut x = Some(k);
    while let Some(n) = x {
        if l.dok.ist_element_knoten(n) {
            kette.push(n);
        }
        if kette.len() > 512 {
            break;
        }
        x = l.dok.eltern(n);
    }
    let mut eltern: Option<narsil_css::Berechnet> = None;
    for n in kette.into_iter().rev() {
        if let Some(b) = cache.get(&n) {
            eltern = Some(b.clone());
            continue;
        }
        let stil_dom = l.dok.attribut(n, "style").unwrap_or("");
        // Release 15.10.2026: the animated values come after the element's
        // own declarations (`Lebend::animated`).
        let mit_anim;
        let stil = match l.animated.get(&n) {
            Some(werte) if !werte.is_empty() => {
                mit_anim = format!("{stil_dom}{}", animated_declarations(werte));
                mit_anim.as_str()
            }
            _ => stil_dom,
        };
        let inline = narsil_css::inline_lesen(stil);
        let vars = narsil_css::inline_variablen(stil);
        let el = ArenaEl { lebend: Some(l), dok: &l.dok, k: n, ce: None, scope: KEIN_SCOPE };
        let mut b = narsil_css::berechnen_mit_vormerker(
            &satz.vormerker,
            &satz.blaetter,
            &el,
            &inline,
            &vars,
            eltern.as_ref(),
            satz.ansicht.0,
            satz.ansicht.1,
            &satz.zustand,
        );
        // Release 11.10.2026: im Top Layer (modaler Dialog, offenes
        // Popover) wird `static`/`relative` zu `absolute` (CSS Position 4
        // §2.1) — wie im Seitenbau (`narsil-page`). GEMESSEN vorher (WPT
        // top-layer-position): `getComputedStyle` meldete `static`.
        if l.dok.css_zustand_von(n) & 3 != 0
            && matches!(b.position, narsil_css::Position::Statisch | narsil_css::Position::Relativ | narsil_css::Position::Klebend)
        {
            b.position = narsil_css::Position::Absolut;
        }
        if cache.len() < MAX_CACHE {
            cache.insert(n, b.clone());
        }
        eltern = Some(b);
    }
    eltern
}

/// Ein Arena-Element fuer den Selektor-Abgleich von `narsil-css`.
#[derive(Clone, Copy)]
struct ArenaEl<'a> {
    /// Release 2026-10-15: the live DOM of the query (dirty values of
    /// form controls, XML-ness of the node document), when it has one.
    lebend: Option<&'a Lebend>,
    dok: &'a Dokument,
    k: usize,
    ce: Option<(&'a HashMap<usize, Vec<String>>, &'a [String])>,
    /// Release 12.10.2026: das `:scope`-Element einer DOM-Abfrage
    /// (`KEIN_SCOPE` = keins). Siehe `scope_umschreiben`.
    scope: usize,
}

/// Release 12.10.2026: kein `:scope`-Element (Kaskade, Stilberechnung).
const KEIN_SCOPE: usize = usize::MAX;

/// Release 12.10.2026: das interne Attribut, auf das `:scope` umgeschrieben wird.
const SCOPE_ATTR: &str = "__narsil_scope__";

/// Release 12.10.2026: `:scope` (Selectors 4 §14.2) — in `querySelector(All)`,
/// `matches` und `closest` das Element, an dem die Methode gerufen wurde
/// (am Dokument die Wurzel, wie `:root`). Der Selektor wird vor dem Lesen
/// auf das interne Attribut `[__narsil_scope__]` umgeschrieben (wie
/// `:state()`), `ArenaEl::attribut` beantwortet es nur am Scope-Element.
/// GEMESSEN vorher (Hauptsitzung, WPT dom/nodes ParentNode-querySelector-
/// scope*, -All-scope, scope-in-not): `el.querySelector(':scope > x')` war
/// `null` — der Parser kannte `:scope` nicht, die ganze Liste fiel weg.
fn scope_umschreiben(sel: &str) -> std::borrow::Cow<'_, str> {
    if !sel.as_bytes().windows(6).any(|w| w.eq_ignore_ascii_case(b":scope")) {
        return std::borrow::Cow::Borrowed(sel);
    }
    let z: Vec<char> = sel.chars().collect();
    let mut out = String::with_capacity(sel.len() + 16);
    let (mut i, mut zitat, mut klammer) = (0usize, None::<char>, 0usize);
    while i < z.len() {
        let c = z[i];
        if c == '\\' {
            out.push(c);
            if let Some(&n) = z.get(i + 1) {
                out.push(n);
            }
            i += 2;
            continue;
        }
        if let Some(q) = zitat {
            out.push(c);
            if c == q {
                zitat = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => zitat = Some(c),
            '[' => klammer += 1,
            ']' => klammer = klammer.saturating_sub(1),
            _ => {}
        }
        let ist_scope = c == ':'
            && klammer == 0
            && i.checked_sub(1).is_none_or(|v| z[v] != ':')
            && z.len() >= i + 6
            && z[i + 1..i + 6].iter().collect::<String>().eq_ignore_ascii_case("scope")
            && !z.get(i + 6).is_some_and(|n| n.is_alphanumeric() || matches!(n, '-' | '_' | '\\' | '('));
        if ist_scope {
            out.push('[');
            out.push_str(SCOPE_ATTR);
            out.push(']');
            i += 6;
            continue;
        }
        out.push(c);
        i += 1;
    }
    std::borrow::Cow::Owned(out)
}

/// Release 12.10.2026: das `:scope`-Element einer Abfrage an `k` — das
/// Element selbst, an Dokument/Fragment/Schattenwurzel deren erstes Element.
fn scope_von(dok: &Dokument, k: usize) -> usize {
    if dok.ist_element_knoten(k) {
        return k;
    }
    element_kinder(dok, k).first().copied().unwrap_or(KEIN_SCOPE)
}

impl<'a> narsil_css::Element for ArenaEl<'a> {
    fn tag(&self) -> &str {
        self.dok.element_name(self.k).unwrap_or("")
    }
    fn id(&self) -> Option<&str> {
        self.dok.attribut(self.k, "id")
    }
    fn hat_klasse(&self, klasse: &str) -> bool {
        self.dok.attribut(self.k, "class").is_some_and(|c| c.split_ascii_whitespace().any(|x| x == klasse))
    }
    /// Release 15.10.2026: the classes of the element, so that the cascade
    /// over the live DOM takes the fast path of the rule index instead of
    /// asking every class name of the sheets (spiegel.de: the animation
    /// scan took 7.7 s in that slow path).
    fn mit_klassen(&self, f: &mut dyn FnMut(&str)) {
        if let Some(c) = self.dok.attribut(self.k, "class") {
            for name in c.split_ascii_whitespace() {
                f(name);
            }
        }
    }
    fn klassen_aufzaehlbar(&self) -> bool {
        true
    }
    fn attribut(&self, name: &str) -> Option<&str> {
        if self.scope != KEIN_SCOPE && name.eq_ignore_ascii_case(SCOPE_ATTR) {
            return (self.k == self.scope).then_some("");
        }
        // Release 12.10.2026: implizites `@scope` (`crate::scope_implizit`).
        if name == crate::scope_implizit::ATTRIBUT {
            return crate::scope_implizit::kennungen_der_kinder(self.dok, self.k);
        }
        if let Some((states, names)) = self.ce {
            if let Some(i) = name.strip_prefix("__narsil_ce_state_").and_then(|s| s.parse::<usize>().ok()) {
                let state = names.get(i)?;
                return states.get(&self.k).is_some_and(|list| list.contains(state)).then_some("");
            }
        }
        // Release 11.10.2026: die internen Zustandsattribute (`:popover-open`,
        // `:modal`) — siehe `Dokument::css_zustand`.
        if name.starts_with(' ') {
            let bits = self.dok.css_zustand_von(self.k);
            let an = (name == narsil_fmt_html::dom::ATTR_POPOVER_OFFEN && bits & 1 != 0)
                || (name == narsil_fmt_html::dom::ATTR_MODAL && bits & 2 != 0)
                || (name == narsil_fmt_html::dom::ATTR_FOKUS && bits & 4 != 0)
                // Release 2026-10-15: the checkedness bits too (`:checked`,
                // `:indeterminate` in DOM queries; see `ist_angehakt`).
                || (name == narsil_fmt_html::dom::ATTR_ANGEHAKT && bits & 8 != 0)
                || (name == narsil_fmt_html::dom::ATTR_NICHT_ANGEHAKT && bits & 16 != 0)
                || (name == narsil_fmt_html::dom::ATTR_UNBESTIMMT && bits & 32 != 0);
            return an.then_some("");
        }
        match &self.dok.knoten.get(self.k)?.art {
            Art::Element { attribute, .. } => attribute.iter().find(|a| a.name.eq_ignore_ascii_case(name)).map(|a| a.wert.as_str()),
            _ => None,
        }
    }
    fn eltern(&self) -> Option<Self> {
        let e = eltern_dom(self.dok, self.k)?;
        self.dok.ist_element_knoten(e).then_some(ArenaEl { k: e, ..*self })
    }
    fn voriges_geschwister(&self) -> Option<Self> {
        naechstes_element(self.dok, self.k, true).map(|k| ArenaEl { k, ..*self })
    }
    fn naechstes_geschwister(&self) -> Option<Self> {
        naechstes_element(self.dok, self.k, false).map(|k| ArenaEl { k, ..*self })
    }
    fn erstes_kind(&self) -> Option<Self> {
        element_kinder(self.dok, self.k).first().map(|&k| ArenaEl { k, ..*self })
    }
    fn ist_leer(&self) -> bool {
        self.dok.kinder(self.k).iter().all(|&c| match &self.dok.knoten[c].art {
            Art::Text(t) => t.is_empty(),
            Art::Kommentar(_) | Art::Pi { .. } => true,
            _ => false,
        })
    }
    // Release 15.10.2026 (Agent CSS2): see `dom_direction`.
    fn richtung(&self) -> Option<SchreibRichtung> {
        // Release 2026-10-15: an `<input dir=auto>` with a dirty value
        // takes its direction from that value (HTML §3.2.6.4). MEASURED
        // before (WPT dir-auto-form-associated): `input.value = "\u{5d0}"`
        // stayed ltr.
        if let Some(v) = self.dok.css_wert_von(self.k)
            && self.dok.element_name(self.k).is_some_and(|n| n.eq_ignore_ascii_case("input"))
            && self.dok.attribut(self.k, "dir").is_some_and(|d| d.trim().eq_ignore_ascii_case("auto"))
        {
            let mut read = 0usize;
            return Some(if first_strong(v, &mut read) == Some(true) { SchreibRichtung::Rtl } else { SchreibRichtung::Ltr });
        }
        dom_direction(self.dok, self.k)
    }
    // Release 2026-10-15: the dirty value and `pattern` for the form
    // pseudo-classes (`narsil_css::form_state`).
    fn form_value(&self) -> Option<std::borrow::Cow<'_, str>> {
        self.dok.css_wert_von(self.k).map(std::borrow::Cow::Borrowed)
    }
    fn pattern_matches(&self, pattern: &str, value: &str) -> Option<bool> {
        narsil_jsvm::pattern_matches(pattern, value)
    }
    fn is_html_in_html_document(&self) -> bool {
        ist_html(self.dok, self.k)
            && (self.dok.xml_typ.is_empty() || self.lebend.is_some_and(|l| !l.in_xml_document(self.k)))
    }
}

// Release 15.10.2026 (Agent CSS2): HTML directionality on the DOM side
// (HTML §3.2.6.4), for `:dir()` and the `direction` presentational hint of
// `getComputedStyle`. MEASURED before (WPT html `dom/elements/global-
// attributes/dir-shadow-*`): `dir` was ignored on the DOM side, every
// element was `ltr` and matched neither `:dir(ltr)` nor `:dir(rtl)`.

use narsil_css::SchreibRichtung;

/// What element `k` itself says about its direction (`None` = it inherits):
/// a valid `dir`, `bdi` (auto), `input type=tel` (ltr), and a child of a
/// shadow root, which takes the directionality of the host (the shadow root
/// has no parent element of its own).
fn dom_direction(dok: &Dokument, k: usize) -> Option<SchreibRichtung> {
    let name = dok.element_name(k).unwrap_or("");
    match dok.attribut(k, "dir").map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("ltr") => return Some(SchreibRichtung::Ltr),
        Some(v) if v.eq_ignore_ascii_case("rtl") => return Some(SchreibRichtung::Rtl),
        Some(v) if v.eq_ignore_ascii_case("auto") => return Some(auto_direction(dok, k)),
        _ => {}
    }
    if name.eq_ignore_ascii_case("bdi") {
        return Some(auto_direction(dok, k));
    }
    if name.eq_ignore_ascii_case("input") && dok.attribut(k, "type").is_some_and(|t| t.trim().eq_ignore_ascii_case("tel")) {
        return Some(SchreibRichtung::Ltr);
    }
    let parent = dok.knoten.get(k).and_then(|n| n.eltern)?;
    if matches!(dok.knoten.get(parent).map(|n| &n.art), Some(Art::ShadowRoot { .. })) {
        let host = dok.knoten.get(parent).and_then(|n| n.eltern)?;
        return Some(dom_directionality(dok, host));
    }
    None
}

/// The directionality of element `k`: the nearest element up the tree that
/// states one, else `ltr`.
fn dom_directionality(dok: &Dokument, k: usize) -> SchreibRichtung {
    let mut x = Some(k);
    for _ in 0..1024 {
        let Some(e) = x else { break };
        if dok.ist_element_knoten(e)
            && let Some(r) = dom_direction(dok, e)
        {
            return r;
        }
        x = dok.knoten.get(e).and_then(|n| n.eltern);
    }
    SchreibRichtung::Ltr
}

/// `dir=auto`: the first strong character of the value (`textarea`, `input`)
/// or of the contained text, which skips `bdi`, `script`, `style`, `textarea`
/// and descendants with a valid `dir` of their own; none = `ltr`.
fn auto_direction(dok: &Dokument, k: usize) -> SchreibRichtung {
    let name = dok.element_name(k).unwrap_or("");
    let mut read = 0usize;
    let strong = if name.eq_ignore_ascii_case("input") {
        first_strong(dok.attribut(k, "value").unwrap_or(""), &mut read)
    } else if name.eq_ignore_ascii_case("textarea") {
        dok.kinder(k).iter().find_map(|&c| match &dok.knoten.get(c)?.art {
            Art::Text(t) => first_strong(t, &mut read),
            _ => None,
        })
    } else {
        contained_strong(dok, k, 0, &mut read)
    };
    if strong == Some(true) { SchreibRichtung::Rtl } else { SchreibRichtung::Ltr }
}

/// `Some(true)` = the first strong character is right-to-left (UAX #9 R, AL),
/// `Some(false)` = left-to-right (L), `None` = no strong character.
fn first_strong(text: &str, read: &mut usize) -> Option<bool> {
    use narsil_text::unicode_daten::bidi_klassen::{BidiKlasse, bidi_klasse};
    for c in text.chars() {
        *read += 1;
        if *read > 4096 {
            return None;
        }
        match bidi_klasse(c) {
            BidiKlasse::L => return Some(false),
            BidiKlasse::R | BidiKlasse::Al => return Some(true),
            _ => {}
        }
    }
    None
}

fn contained_strong(dok: &Dokument, k: usize, depth: usize, read: &mut usize) -> Option<bool> {
    if depth > 64 {
        return None;
    }
    for &c in dok.kinder(k) {
        match &dok.knoten.get(c)?.art {
            Art::Text(t) => {
                if let Some(r) = first_strong(t, read) {
                    return Some(r);
                }
            }
            Art::Element { .. } => {
                let name = dok.element_name(c).unwrap_or("");
                let own_dir = dok.attribut(c, "dir").is_some_and(|v| {
                    ["ltr", "rtl", "auto"].iter().any(|w| v.trim().eq_ignore_ascii_case(w))
                });
                if own_dir || ["bdi", "script", "style", "textarea"].iter().any(|t| name.eq_ignore_ascii_case(t)) {
                    continue;
                }
                if let Some(r) = contained_strong(dok, c, depth + 1, read) {
                    return Some(r);
                }
            }
            _ => {}
        }
        if *read > 4096 {
            return None;
        }
    }
    None
}

fn selektoren(sel: &str) -> Vec<narsil_css::Selektor> {
    if sel.len() > crate::js::MAX_SELECTOR {
        return Vec::new();
    }
    narsil_css::selektorliste_lesen(sel)
}

/// DOM-Abfragen koennen :state() an denselben Selektor-Abgleich anbinden,
/// ohne ein DOM-Attribut anzulegen oder CSS-/Layout-Crates zu veraendern.
fn ce_selektor(sel: &str) -> Result<(String, Vec<String>), String> {
    let z: Vec<char> = sel.chars().collect();
    let mut out = String::new();
    let mut states = Vec::new();
    let mut i = 0;
    let mut quote = None;
    let mut bracket = 0usize;
    let syntax = || "SyntaxError: ungueltiger :state()-Selektor".to_string();
    while i < z.len() {
        let c = z[i];
        if c == '\\' {
            out.push(c); i += 1;
            if i < z.len() { out.push(z[i]); i += 1; }
            continue;
        }
        if let Some(q) = quote {
            out.push(c); i += 1;
            if c == q { quote = None; }
            continue;
        }
        if c == '\'' || c == '"' { quote = Some(c); }
        if c == '[' { bracket += 1; }
        if c == ']' { bracket = bracket.saturating_sub(1); }
        if c != ':' || bracket > 0 || !z[i+1..].starts_with(&['s','t','a','t','e']) {
            out.push(c); i += 1; continue;
        }
        let next = z.get(i + 6).copied();
        if next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            out.push(c); i += 1; continue;
        }
        if next != Some('(') { return Err(syntax()); }
        i += 7;
        while z.get(i).is_some_and(|c| c.is_ascii_whitespace()) { i += 1; }
        let mut value = String::new();
        let mut first = true;
        while i < z.len() && z[i] != ')' && !z[i].is_ascii_whitespace() {
            let c = z[i];
            if c == '\\' {
                i += 1;
                if i >= z.len() || z[i] == '\n' || z[i] == '\r' { return Err(syntax()); }
                let mut hex = String::new();
                while i < z.len() && z[i].is_ascii_hexdigit() && hex.len() < 6 { hex.push(z[i]); i += 1; }
                if hex.is_empty() { value.push(z[i]); i += 1; }
                else {
                    let num = u32::from_str_radix(&hex,16).unwrap_or(0);
                    value.push(char::from_u32(num).filter(|c| *c != '\0').unwrap_or('\u{fffd}'));
                    if z.get(i).is_some_and(|c| c.is_ascii_whitespace()) { i += 1; }
                }
            } else {
                if !(c.is_ascii_alphabetic() || c == '_' || c == '-' || c as u32 >= 128 || !first && c.is_ascii_digit()) { return Err(syntax()); }
                if first && c == '-' && !z.get(i+1).is_some_and(|c| c.is_ascii_alphabetic() || *c == '_' || *c == '-' || *c == '\\' || *c as u32 >= 128) { return Err(syntax()); }
                value.push(c); i += 1;
            }
            first = false;
        }
        if value.is_empty() { return Err(syntax()); }
        while z.get(i).is_some_and(|c| c.is_ascii_whitespace()) { i += 1; }
        if z.get(i) != Some(&')') { return Err(syntax()); }
        i += 1;
        out.push_str(&format!("[__narsil_ce_state_{}]", states.len()));
        states.push(value);
    }
    Ok((out, states))
}

fn passt_ce(l: &Lebend, k: usize, sel: &str, zustand: &narsil_css::Zustand, scope: usize) -> bool {
    let sel = &*scope_umschreiben(sel);
    if !sel.contains(":state") { return passt(Some(l), &l.dok, k, sel, zustand, scope); }
    let Ok((sel, states)) = ce_selektor(sel) else { return false };
    let liste = selektoren(&sel);
    let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: Some((&l.ce_states, &states)), scope };
    l.dok.ist_element_knoten(k) && liste.iter().any(|s| narsil_css::selektor_passt_mit(s, &el, zustand))
}

fn suchen_ce(l: &Lebend, root: usize, sel: &str, erste: bool, zustand: &narsil_css::Zustand) -> Vec<usize> {
    let sel = &*scope_umschreiben(sel);
    let scope = scope_von(&l.dok, root);
    if !sel.contains(":state") { return suchen(Some(l), &l.dok, root, sel, erste, zustand, scope); }
    let Ok((sel, states)) = ce_selektor(sel) else { return Vec::new() };
    let liste = selektoren(&sel);
    let mut raus = Vec::new();
    let mut stack: Vec<_> = l.dok.kinder(root).iter().rev().copied().collect();
    while let Some(k) = stack.pop() {
        let el = ArenaEl { lebend: Some(l), dok: &l.dok, k, ce: Some((&l.ce_states, &states)), scope };
        if l.dok.ist_element_knoten(k) && liste.iter().any(|s| narsil_css::selektor_passt_mit(s, &el, zustand)) {
            raus.push(k); if erste || raus.len() >= MAX_TREFFER { break; }
        }
        stack.extend(l.dok.kinder(k).iter().rev().copied());
    }
    raus
}

/// Release 2026-10-15: `lebend` (when the caller has it) gives the matcher
/// the dirty values of form controls and the node document's XML-ness.
fn passt(lebend: Option<&Lebend>, dok: &Dokument, k: usize, sel: &str, zustand: &narsil_css::Zustand, scope: usize) -> bool {
    let liste = selektoren(sel);
    dok.ist_element_knoten(k)
        && liste.iter().any(|s| narsil_css::selektor_passt_mit(s, &ArenaEl { lebend, dok, k, ce: None, scope }, zustand))
}

fn suchen(
    lebend: Option<&Lebend>,
    dok: &Dokument,
    wurzel: usize,
    sel: &str,
    nur_erste: bool,
    zustand: &narsil_css::Zustand,
    scope: usize,
) -> Vec<usize> {
    let liste = selektoren(sel);
    let mut raus = Vec::new();
    if liste.is_empty() {
        return raus;
    }
    let mut stapel: Vec<usize> = dok.kinder(wurzel).iter().rev().copied().collect();
    while let Some(k) = stapel.pop() {
        if dok.ist_element_knoten(k) && liste.iter().any(|s| narsil_css::selektor_passt_mit(s, &ArenaEl { lebend, dok, k, ce: None, scope }, zustand)) {
            raus.push(k);
            if nur_erste || raus.len() >= MAX_TREFFER {
                break;
            }
        }
        for &c in dok.kinder(k).iter().rev() {
            stapel.push(c);
        }
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lebend(html: &str) -> Lebend {
        let baum = narsil_fmt_html::parse_html(html);
        // Wie der Proxy-Stand: Host-Id je Element in ord-Reihenfolge.
        let mut ids = HashMap::new();
        let mut ord = 0usize;
        let mut naechste = 1u32;
        fn gehen(l: &[HtmlNode], ord: &mut usize, ids: &mut HashMap<usize, u32>, n: &mut u32) {
            for k in l {
                let me = *ord;
                *ord += 1;
                if let HtmlNode::Element { children, .. } = k {
                    ids.insert(me, *n);
                    *n += 1;
                    gehen(children, ord, ids, n);
                }
            }
        }
        gehen(&baum, &mut ord, &mut ids, &mut naechste);
        Lebend::neu(&baum, &ids, 65_536)
    }

    #[test]
    fn struktur_lesen_und_schreiben() {
        let mut l = lebend("<div id=a><span>x</span><b>y</b></div>");
        let z = narsil_css::Zustand::KEINER;
        let doc = 65_536;
        let a = rufen(&mut l, doc, 0, "getElementById", &[Wert::Text("a".into())], &z).unwrap().unwrap();
        let a_id = narsil_jsvm::host_id(&a).unwrap();
        assert_eq!(lesen(&mut l, a_id, "textContent"), Some(Wert::Text("xy".into())));
        let span = lesen(&mut l, a_id, "firstChild").unwrap();
        let b = lesen(&mut l, narsil_jsvm::host_id(&span).unwrap(), "nextSibling").unwrap();
        assert_eq!(lesen(&mut l, narsil_jsvm::host_id(&b).unwrap(), "nodeName"), Some(Wert::Text("B".into())));
        schreiben(&mut l, a_id, "innerHTML", &Wert::Text("<i>neu</i> text".into())).unwrap().unwrap();
        assert!(l.geaendert);
        let (html, ids) = l.html_neu();
        assert!(narsil_fmt_html::serialisiere(&html).contains("<i>neu</i> text"));
        assert!(ids.values().any(|h| *h == a_id), "das alte Element behaelt seine Host-Id");
    }

    /// Release 12.10.2026: `:scope` in DOM-Abfragen (Selectors 4 §14.2).
    #[test]
    fn scope_in_abfragen() {
        let mut l = lebend("<div id=a><p id=p1><span>x</span></p><p id=p2></p></div>");
        let z = narsil_css::Zustand::KEINER;
        let doc = 65_536;
        let a = rufen(&mut l, doc, 0, "getElementById", &[Wert::Text("a".into())], &z).unwrap().unwrap();
        let a_id = narsil_jsvm::host_id(&a).unwrap();
        let alle = rufen(&mut l, a_id, 0, "querySelectorAll", &[Wert::Text(":scope > p".into())], &z).unwrap().unwrap();
        let Wert::Feld(v) = alle else { panic!() };
        assert_eq!(v.len(), 2);
        let keins = rufen(&mut l, a_id, 0, "querySelector", &[Wert::Text(":SCOPE > span".into())], &z).unwrap().unwrap();
        assert_eq!(keins, Wert::Null);
        let nicht = rufen(&mut l, a_id, 0, "querySelectorAll", &[Wert::Text("p:not(:scope > p)".into())], &z).unwrap().unwrap();
        let Wert::Feld(v) = nicht else { panic!() };
        assert!(v.is_empty());
        assert_eq!(rufen(&mut l, a_id, 0, "matches", &[Wert::Text(":scope".into())], &z).unwrap().unwrap(), Wert::Wahr(true));
        assert_eq!(scope_umschreiben("[a=':scope']:scope"), "[a=':scope'][__narsil_scope__]");
        assert_eq!(scope_umschreiben("::scope-x"), "::scope-x");
    }

    #[test]
    fn neue_knoten_und_fragmente() {
        let mut l = lebend("<ul id=u></ul>");
        let z = narsil_css::Zustand::KEINER;
        let doc = 65_536;
        let u = rufen(&mut l, doc, 0, "querySelector", &[Wert::Text("#u".into())], &z).unwrap().unwrap();
        let f = rufen(&mut l, doc, 0, "createDocumentFragment", &[], &z).unwrap().unwrap();
        for i in 0..3 {
            let li = rufen(&mut l, doc, 0, "createElement", &[Wert::Text("li".into())], &z).unwrap().unwrap();
            schreiben(&mut l, narsil_jsvm::host_id(&li).unwrap(), "textContent", &Wert::Text(format!("p{i}"))).unwrap().unwrap();
            rufen(&mut l, narsil_jsvm::host_id(&f).unwrap(), 0, "appendChild", &[li], &z).unwrap().unwrap();
        }
        rufen(&mut l, narsil_jsvm::host_id(&u).unwrap(), 0, "appendChild", &[f], &z).unwrap().unwrap();
        let alle = rufen(&mut l, doc, 0, "querySelectorAll", &[Wert::Text("li".into())], &z).unwrap().unwrap();
        let Wert::Feld(v) = alle else { panic!() };
        assert_eq!(v.len(), 3);
        let (html, _) = l.html_neu();
        assert!(narsil_fmt_html::serialisiere(&html).contains("<li>p0</li><li>p1</li><li>p2</li>"));
    }
}

/// Release 12.10.2026: DOM §4.2.14 „attach a shadow root" Schritte 1-3 —
/// ein HTML-Element aus der Liste oder mit gueltigem Custom-Element-Namen.
fn schatten_wirt_gueltig(dok: &narsil_fmt_html::dom::Dokument, k: usize) -> bool {
    let Some(Art::Element { raum: Namensraum::Html, name, .. }) = dok.knoten.get(k).map(|n| &n.art) else {
        return false;
    };
    // Der LOKALE Name (`<xhtml:div>` in XHTML, WPT host-with-namespace).
    let name = name.rsplit(':').next().unwrap_or(name);
    if matches!(
        name,
        "article" | "aside" | "blockquote" | "body" | "div" | "footer" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            | "header" | "main" | "nav" | "p" | "section" | "span"
    ) {
        return true;
    }
    // HTML §4.13.2 „valid custom element name".
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.contains('-')
        && !name.chars().any(|c| c.is_ascii_uppercase())
        && !matches!(
            name,
            "annotation-xml" | "color-profile" | "font-face" | "font-face-src" | "font-face-uri" | "font-face-format"
                | "font-face-name" | "missing-glyph"
        )
}

/// Release 13.10.2026: die „listed elements" eines Formulars (HTML §4.10.2)
/// in Baumreihenfolge — Nachfahren ohne fremdes `form`-Attribut, dazu
/// Felder mit `form="id"` im selben Baum. Mit `bilder` auch `<img>` (fuer
/// die benannten Eigenschaften).
fn form_felder(l: &Lebend, form: usize, bilder: bool) -> Vec<usize> {
    let dok = &l.dok;
    let id = dok.attribut(form, "id").map(str::to_string);
    let mut wurzel = form;
    while let Some(e) = eltern_dom(dok, wurzel) {
        wurzel = e;
    }
    let mut raus = Vec::new();
    let mut stapel = vec![wurzel];
    let mut schritte = 0usize;
    while let Some(n) = stapel.pop() {
        schritte += 1;
        if schritte > 200_000 {
            break;
        }
        if let Some(name) = dok.element_name(n)
            && matches!(dok.knoten.get(n).map(|x| &x.art), Some(Art::Element { raum: Namensraum::Html, .. }))
        {
            let gelistet = matches!(name, "button" | "fieldset" | "object" | "output" | "select" | "textarea")
                || (name == "input" && !dok.attribut(n, "type").is_some_and(|t| t.trim().eq_ignore_ascii_case("image")))
                || (bilder && name == "img");
            if gelistet && n != form {
                let gehoert = match dok.attribut(n, "form") {
                    Some(f) if name != "img" => id.as_deref() == Some(f),
                    _ => dok_enthaelt(dok, form, n) && naechstes_form(dok, n) == Some(form),
                };
                if gehoert {
                    raus.push(n);
                }
            }
        }
        // Template-Inhalt gehoert nicht dazu (eigenes Fragment).
        for &c in dok.kinder(n).iter().rev() {
            stapel.push(c);
        }
    }
    raus
}

fn dok_enthaelt(dok: &Dokument, a: usize, b: usize) -> bool {
    let mut x = Some(b);
    let mut rest = 100_000;
    while let Some(n) = x {
        if n == a {
            return true;
        }
        rest -= 1;
        if rest == 0 {
            return false;
        }
        x = eltern_dom(dok, n);
    }
    false
}

fn naechstes_form(dok: &Dokument, n: usize) -> Option<usize> {
    let mut x = eltern_dom(dok, n);
    let mut rest = 100_000;
    while let Some(e) = x {
        if dok.element_name(e) == Some("form") {
            return Some(e);
        }
        rest -= 1;
        if rest == 0 {
            return None;
        }
        x = eltern_dom(dok, e);
    }
    None
}

fn form_benannt(l: &mut Lebend, form: usize, name: &str) -> Option<Wert> {
    if name.bytes().all(|b| b.is_ascii_digit()) {
        let i: usize = name.parse().ok()?;
        // Release 2026-10-15: from the live `form.elements` cache (a loop
        // over `form[i]` walked the whole tree per step before).
        let felder = l.collection_members(form, CollectionKind::FormElements).unwrap_or_else(|| form_felder(l, form, false));
        return felder.get(i).map(|&n| l.wert(Some(n)));
    }
    // Schneller Ausschluss: kommt der Name im Formularbaum gar nicht vor?
    let felder = form_felder(l, form, true);
    let treffer: Vec<usize> = felder
        .iter()
        .copied()
        .filter(|&n| l.dok.attribut(n, "id") == Some(name) || l.dok.attribut(n, "name") == Some(name))
        .collect();
    // Bilder nur, wenn kein Feld den Namen traegt (HTML §4.10.3 Schritt 2).
    let felder_treffer: Vec<usize> = treffer.iter().copied().filter(|&n| l.dok.element_name(n) != Some("img")).collect();
    let liste = if felder_treffer.is_empty() { treffer } else { felder_treffer };
    match liste.len() {
        0 => None,
        1 => Some(l.wert(Some(liste[0]))),
        // Release 2026-10-15: several controls are a live RadioNodeList
        // (HTML §4.10.3 named getter).
        _ if liste.iter().all(|&n| l.dok.element_name(n) != Some("img")) => l
            .collection(form, CollectionKind::FormNamed(name.to_string()))
            .or_else(|| Some(Wert::Feld(liste.iter().map(|&n| l.wert(Some(n))).collect()))),
        _ => Some(Wert::Feld(liste.into_iter().map(|n| l.wert(Some(n))).collect())),
    }
}

/// Release 2026-10-15: DOM §4.4 "node A equals node B".
fn knoten_gleich(l: &Lebend, a: usize, b: usize, tiefe: usize) -> bool {
    if tiefe > 512 {
        return false;
    }
    let dok = &l.dok;
    if dok.knoten_typ(a) != dok.knoten_typ(b) {
        return false;
    }
    let gleich_hier = match (dok.knoten.get(a).map(|n| &n.art), dok.knoten.get(b).map(|n| &n.art)) {
        (Some(Art::Doctype { name: n1, public_id: p1, system_id: s1 }), Some(Art::Doctype { name: n2, public_id: p2, system_id: s2 })) => {
            n1 == n2 && p1 == p2 && s1 == s2
        }
        (Some(Art::Element { .. }), Some(Art::Element { .. })) => {
            let praefix = |k: usize| dok.fremd_ns.get(&k).and_then(|(_, p)| p.clone());
            if element_ns(dok, a) != element_ns(dok, b) || praefix(a) != praefix(b) || dok.element_name(a) != dok.element_name(b) {
                return false;
            }
            let lokal = |q: &str, ns: &Option<String>| -> String {
                if ns.is_some() { q.split_once(':').map_or(q, |(_, l)| l).to_string() } else { q.to_string() }
            };
            let aa = l.attribute_von(a);
            let bb = l.attribute_von(b);
            aa.len() == bb.len()
                && aa.iter().all(|(q, v, ns)| bb.iter().any(|(q2, v2, ns2)| ns == ns2 && lokal(q, ns) == lokal(q2, ns2) && v == v2))
        }
        (Some(Art::Pi { target: t1, daten: d1 }), Some(Art::Pi { target: t2, daten: d2 })) => t1 == t2 && d1 == d2,
        (Some(Art::Text(t1)), Some(Art::Text(t2))) | (Some(Art::Kommentar(t1)), Some(Art::Kommentar(t2))) => t1 == t2,
        _ => true,
    };
    if !gleich_hier {
        return false;
    }
    let (ka, kb) = (dok.kinder(a), dok.kinder(b));
    ka.len() == kb.len() && ka.iter().zip(kb.iter()).all(|(&x, &y)| knoten_gleich(l, x, y, tiefe + 1))
}

/// Release 2026-10-15: an HTML element (any local name)?
fn ist_html(dok: &Dokument, k: usize) -> bool {
    matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Element { raum: Namensraum::Html, .. }))
}

/// Release 2026-10-15: the live collection behind `__narsilLive:<what>` of
/// element `k` (the prelude's getters ask for it), if `k` has one.
fn element_collection_kind(dok: &Dokument, k: usize, what: &str) -> Option<CollectionKind> {
    if !ist_html(dok, k) {
        return None;
    }
    let tag = dok.element_name(k)?;
    Some(match (what, tag) {
        ("rows", "table") => CollectionKind::TableRows,
        ("tBodies", "table") => CollectionKind::TableBodies,
        ("rows", "thead" | "tbody" | "tfoot") => CollectionKind::SectionRows,
        ("cells", "tr") => CollectionKind::Cells,
        ("areas", "map") => CollectionKind::MapAreas,
        ("options", "datalist") => CollectionKind::DatalistOptions,
        ("elements", "form") => CollectionKind::FormElements,
        ("elements", "fieldset") => CollectionKind::FieldsetElements,
        _ => return None,
    })
}
