//! Der DOM-Baum fuer den spec-konformen HTML-Parser (Aufgabe H2).
//!
//! Arena statt Zeiger: jeder Knoten hat einen Index, Eltern und Kinder
//! sind Indizes. Das macht Foster Parenting und die Adoption Agency
//! einfach (Knoten umhaengen statt Lebensdauerakrobatik) und ist
//! cachefreundlich.
//!
//! Der Baum kennt vier Knotenarten: Dokument, DOCTYPE, Element (mit
//! Namensraum, Attributen und optionalem Template-Inhalt), Text und
//! Kommentar. Fuer `template` haelt ein eigenes Fragment die Kinder.
//!
//! [`dump`] erzeugt die Ausgabe im Format der html5lib-Tests
//! (tree-construction/.dat), woertlich wie `testSerializer` der
//! Referenzimplementierung: Zeilen mit `|`, zwei Leerzeichen je Ebene,
//! Attribute und Text zwei Stellen tiefer, Attribute nach Namen
//! sortiert, DOCTYPE ohne Balken.

/// Namensraum eines Elements oder Attributs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namensraum {
    Html,
    Svg,
    MathMl,
    Xlink,
    Xml,
    Xmlns,
    /// Release 11.10.2026: ein Element in einem ANDEREN (oder keinem)
    /// Namensraum (`createElementNS('urn:x', …)`, XML-Dokumente). Die
    /// Adresse steht in [`Dokument::fremd_ns`].
    Fremd,
}

impl Namensraum {
    /// Das Praefix im Testformat (`svg`, `math`, `xlink`, ...).
    pub fn praefix(&self) -> Option<&'static str> {
        match self {
            Namensraum::Html => None,
            Namensraum::Svg => Some("svg"),
            Namensraum::MathMl => Some("math"),
            Namensraum::Xlink => Some("xlink"),
            Namensraum::Xml => Some("xml"),
            Namensraum::Xmlns => Some("xmlns"),
            Namensraum::Fremd => None,
        }
    }
}

/// Ein Attribut mit optionalem Namensraum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribut {
    pub raum: Namensraum,
    pub name: String,
    pub wert: String,
}

impl Attribut {
    /// Der Anzeigename im Testformat.
    pub fn anzeige(&self) -> String {
        match self.raum.praefix() {
            Some(p) => format!("{p} {}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Der Modus eines Shadow Roots (declarative Shadow DOM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowRootMode {
    Open,
    Closed,
}

/// Ein Shadow Root (für declarative Shadow DOM).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowRoot {
    pub mode: ShadowRootMode,
    /// Die Kinder des Shadow Trees (Indices in der Arena).
    pub children: Vec<usize>,
    /// Release 12.10.2026: die Merkmale (DOM §4.8): [`SCHATTEN_KLONBAR`],
    /// [`SCHATTEN_SERIALISIERBAR`], [`SCHATTEN_DELEGIERT`],
    /// [`SCHATTEN_DEKLARATIV`] (aus dem Parser, bis `attachShadow` sie
    /// uebernimmt).
    pub merkmale: u8,
}

/// `clonable` (`shadowrootclonable`).
pub const SCHATTEN_KLONBAR: u8 = 1;
/// `serializable` (`shadowrootserializable`).
pub const SCHATTEN_SERIALISIERBAR: u8 = 2;
/// `delegatesFocus` (`shadowrootdelegatesfocus`).
pub const SCHATTEN_DELEGIERT: u8 = 4;
/// Die Wurzel kam aus einem `<template shadowrootmode>` (DOM „declarative").
pub const SCHATTEN_DEKLARATIV: u8 = 8;
/// `slotAssignment: "manual"` (`shadowrootslotassignment="manual"`).
pub const SCHATTEN_MANUELL: u8 = 16;

/// Die Art eines Knotens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Art {
    Dokument,
    /// Ein Dokumentfragment (Wurzel einer Fragmentanalyse oder der
    /// Inhalt eines `template`).
    Fragment,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    Element {
        raum: Namensraum,
        name: String,
        attribute: Vec<Attribut>,
        /// Inhalt eines `<template>`: eigener Fragmentknoten.
        template_inhalt: Option<usize>,
        /// Shadow Root Info: falls dieses Element ein Shadow Host ist.
        shadow_root: Option<ShadowRoot>,
    },
    /// Ein Shadow Root (declarative Shadow DOM).
    /// Wird an ein Host-Element angehängt, nicht als eigenes Kind.
    ShadowRoot {
        mode: ShadowRootMode,
        children: Vec<usize>,
    },
    Text(String),
    Kommentar(String),
    /// Eine Processing Instruction (13.2.5.72 bis .76 als Token,
    /// 13.2.6.1 als Knoten).
    ///
    /// GEMESSEN, warum es diese Variante vorher nicht gab (03.10.2026):
    /// der Spec sagt in elf Eintraegen woertlich "Insert a processing
    /// instruction", mein Treebuilder hat PI-Tokens mit `=> {}`
    /// verworfen. Das ergab drei verschiedene Erwartungen an ein
    /// `<!ENTITY x>`-Aehnliches `<?foo bar?>`:
    ///   * Live-Spec 13.2.6.4.1: ein PI-Knoten
    ///   * html5lib-Korpus (8 Faelle): ein Kommentar `<!-- ?foo bar? -->`
    ///   * mein Parser: gar nichts
    /// Die Variante stellt die erste her. Die zweite kann der Korpus
    /// nicht ausdruecken — sein Testformat kennt keinen PI-Knoten —
    /// und die dritte war schlicht falsch. Siehe PROBLEME.txt.
    Pi {
        target: String,
        daten: String,
    },
}

/// Ein Knoten in der Arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knoten {
    pub art: Art,
    pub eltern: Option<usize>,
    pub kinder: Vec<usize>,
}

/// Release 2026-10-15: a mutation version of a document. Every change takes
/// a fresh number from one process-wide counter, so a cache keyed by it
/// (the live collections in `narsil::lebend`) is stale as soon as anything
/// changed, and two documents never share a number by accident. It is
/// bookkeeping, not content: two versions always compare equal.
#[derive(Debug, Clone, Copy)]
pub struct Version(pub u64);

impl PartialEq for Version {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Version {}

/// Each thread hands out numbers from its own block (the block number in the
/// high bits), so a fresh version costs a thread-local increment, not a
/// shared atomic one — the parser takes one per inserted node.
static NEXT_VERSION_BLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const VERSION_BLOCK_BITS: u32 = 40;

thread_local! {
    static VERSION_COUNTER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

impl Version {
    /// A number no document had before.
    #[must_use]
    pub fn fresh() -> Self {
        VERSION_COUNTER.with(|c| {
            let mut v = c.get();
            let mask = (1u64 << VERSION_BLOCK_BITS) - 1;
            if v == 0 || v & mask == mask {
                v = NEXT_VERSION_BLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << VERSION_BLOCK_BITS;
            }
            v += 1;
            c.set(v);
            Version(v)
        })
    }
}

/// Das Dokument als Arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dokument {
    pub knoten: Vec<Knoten>,
    /// Release 09.10.2026: das AENDERUNGSPROTOKOLL fuer `MutationObserver`
    /// (DOM §4.3). `None` = aus (der Normalfall, kostet nichts); der Host
    /// schaltet es ein, sobald ein Beobachter angemeldet ist, und holt die
    /// Eintraege nach jedem DOM-Aufruf ab.
    pub journal: Option<Vec<Aenderung>>,
    /// Release 11.10.2026: Zustaende, die nur Selektoren sehen (Bit 1
    /// `:popover-open`, Bit 2 `:modal`, Bit 4 `:focus` aus einem Skript) —
    /// gesetzt vom Host (Popover-, Dialog- und Fokus-API), NICHT als Attribut
    /// fuer Skripte sichtbar.
    pub css_zustand: Vec<(usize, u8)>,
    /// Release 2026-10-15: the dirty values of form controls (node ->
    /// value) for selector matching and the page tree (`:placeholder-shown`,
    /// `:valid`, `:in-range`; see `css_wert_von`).
    pub css_werte: std::collections::HashMap<usize, String>,
    /// Release 11.10.2026: Namensraum-Adresse ("" = kein Namensraum) und
    /// Praefix eines Elements, wo das Element sie nicht selbst traegt
    /// (`Namensraum::Fremd`, oder ein Praefix wie `svg:rect`).
    pub fremd_ns: std::collections::HashMap<usize, (String, Option<String>)>,
    /// Release 11.10.2026: Textknoten, die CDATA-Abschnitte sind (DOM §4.11,
    /// `nodeType` 4).
    pub cdata: std::collections::HashSet<usize>,
    /// Release 11.10.2026: XML-Dokumente (DOM §4.5 „XML document") mit
    /// ihrem `contentType`.
    pub xml_typ: std::collections::HashMap<usize, String>,
    /// Release 12.10.2026: die lebenden Bereiche (DOM §5 „live ranges"),
    /// die die Mutationsprimitive mitfuehren (siehe [`crate::bereich`]).
    pub bereiche: crate::bereich::Bereiche,
    /// Release 12.10.2026: ein Datensatz, der gerade gesammelt wird (DOM
    /// §4.3.3 „queue a tree mutation record" einmal je Einfuegen/Ersetzen,
    /// siehe [`Dokument::sammeln_an`]).
    pub sammeln: Option<Sammler>,
    /// Release 2026-10-15: changes of the child lists (insert, remove).
    pub tree_version: Version,
    /// Release 2026-10-15: changes of the attributes collections filter on
    /// (`class`, `id`, `name`, `href`, `form`, `type`).
    pub attr_version: Version,
}

/// Release 12.10.2026: ein gesammelter Kinder-Datensatz (siehe
/// [`Dokument::sammeln_an`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sammler {
    pub eltern: usize,
    pub hinzu: Vec<usize>,
    pub weg: Vec<usize>,
    pub vorher: Option<usize>,
    pub nachher: Option<usize>,
    /// Entfernte Kinder dieses Eltern gehen in den Satz (Ersetzen) oder
    /// bekommen eigene Saetze (das Umhaengen beim Einfuegen).
    pub weg_sammeln: bool,
}

/// Release 09.10.2026: eine Aenderung am Baum (fuer `MutationObserver`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Aenderung {
    /// `knoten` kam zu `eltern` (`hinzu`) oder ging weg; `vorher`/`nachher`
    /// sind die Geschwister an der Stelle.
    Kinder { eltern: usize, knoten: usize, hinzu: bool, vorher: Option<usize>, nachher: Option<usize> },
    /// Release 12.10.2026: EIN Datensatz fuer ein Einfuegen mehrerer Knoten
    /// oder ein Ersetzen (DOM §4.2.3 insert/replace/replace all).
    KinderSatz { eltern: usize, hinzu: Vec<usize>, weg: Vec<usize>, vorher: Option<usize>, nachher: Option<usize> },
    /// Ein Attribut wurde gesetzt oder entfernt (`alt` = alter Wert; `ns`
    /// der Namensraum, Release 12.10.2026 fuer `attributeNamespace`).
    Attribut { knoten: usize, name: String, alt: Option<String>, ns: Option<String> },
    /// Der Inhalt eines Text-/Kommentarknotens (`alt` = alter Text).
    Daten { knoten: usize, alt: String },
}

/// Release 09.10.2026: hoechstens so viele Eintraege bis zur Abholung.
pub const MAX_JOURNAL: usize = 50_000;

/// Release 11.10.2026: die internen Attribute fuer `:popover-open` und
/// `:modal` (siehe `Dokument::css_zustand`).
pub const ATTR_POPOVER_OFFEN: &str = " narsil-popover-open";
pub const ATTR_MODAL: &str = " narsil-modal";
/// Release 11.10.2026: `:focus` nach `el.focus()` aus einem Skript (Bit 4).
pub const ATTR_FOKUS: &str = " narsil-focus";
/// Release 11.10.2026: der Haken-Zustand eines `<input>` (HTML §4.10.5
/// „checkedness" mit „dirty checkedness"): Bit 8 = vom Skript/Nutzer
/// angehakt, Bit 16 = ausdruecklich NICHT angehakt (das Attribut `checked`
/// gilt dann nicht mehr), Bit 32 = `indeterminate`.
pub const ATTR_ANGEHAKT: &str = " narsil-checked";
pub const ATTR_NICHT_ANGEHAKT: &str = " narsil-unchecked";
pub const ATTR_UNBESTIMMT: &str = " narsil-indeterminate";
/// Die Bits des Haken-Zustands (wandern beim Klonen mit).
pub const HAKEN_BITS: u8 = 8 | 16 | 32;
/// Release 2026-10-15: the dirty value of a `<textarea>` in the page tree
/// (an `<input>` carries its dirty value as `value` there).
pub const ATTR_WERT: &str = " narsil-value";

/// Groesste Knotenzahl (M3): dieselbe Groessenordnung wie der alte Parser.
pub const MAX_KNOTEN: usize = 400_000;

impl Dokument {
    /// Ein leeres Dokument mit Wurzelknoten.
    pub fn neu() -> (Self, usize) {
        let mut dok = Dokument {
            knoten: Vec::with_capacity(64),
            journal: None,
            css_zustand: Vec::new(),
            css_werte: std::collections::HashMap::new(),
            fremd_ns: std::collections::HashMap::new(),
            cdata: std::collections::HashSet::new(),
            xml_typ: std::collections::HashMap::new(),
            bereiche: crate::bereich::Bereiche::default(),
            sammeln: None,
            tree_version: Version::fresh(),
            attr_version: Version::fresh(),
        };
        let wurzel = dok.neuer_knoten(Art::Dokument);
        (dok, wurzel)
    }

    /// Release 11.10.2026: die Selektor-Zustaende eines Knotens (siehe
    /// `css_zustand`).
    #[must_use]
    pub fn css_zustand_von(&self, i: usize) -> u8 {
        if self.css_zustand.is_empty() {
            return 0;
        }
        self.css_zustand.iter().find(|(k, _)| *k == i).map_or(0, |(_, b)| *b)
    }

    /// Release 2026-10-15: the dirty value of a form control, if any.
    #[must_use]
    pub fn css_wert_von(&self, i: usize) -> Option<&str> {
        if self.css_werte.is_empty() {
            return None;
        }
        self.css_werte.get(&i).map(String::as_str)
    }

    /// Release 2026-10-15: sets (`Some`) or clears (`None`) the dirty value
    /// of a form control (at most 4096 controls).
    pub fn css_wert_setzen(&mut self, i: usize, wert: Option<&str>) {
        match wert {
            Some(v) if self.css_werte.len() < 4096 || self.css_werte.contains_key(&i) => {
                self.css_werte.insert(i, v.to_string());
            }
            Some(_) => {}
            None => {
                self.css_werte.remove(&i);
            }
        }
    }

    /// Setzt die Selektor-Zustaende eines Knotens (0 = keiner).
    pub fn css_zustand_setzen(&mut self, i: usize, bits: u8) {
        self.css_zustand.retain(|(k, _)| *k != i);
        if bits != 0 && self.css_zustand.len() < 4096 {
            self.css_zustand.push((i, bits));
        }
    }

    /// Legt einen neuen Knoten an (ohne Eltern).
    pub fn neuer_knoten(&mut self, art: Art) -> usize {
        let i = self.knoten.len();
        self.knoten.push(Knoten {
            art,
            eltern: None,
            kinder: Vec::new(),
        });
        i
    }

    /// Release 2026-10-15: a child list changed (see [`Version`]).
    pub fn tree_changed(&mut self) {
        self.tree_version = Version::fresh();
    }

    /// Release 2026-10-15: an attribute changed; only the names collections
    /// filter on count (see [`Version`]; `form`/`type` decide form
    /// ownership and listed-ness).
    pub fn attribute_changed(&mut self, name: &str) {
        if matches!(name, "class" | "id" | "name" | "href" | "form" | "type") {
            self.attr_version = Version::fresh();
        }
    }

    /// Haengt `kind` als letztes Kind an `eltern`.
    pub fn anhaengen(&mut self, eltern: usize, kind: usize) {
        self.abtrennen(kind);
        self.tree_changed();
        let vorher = self.knoten[eltern].kinder.last().copied();
        self.knoten[kind].eltern = Some(eltern);
        self.knoten[eltern].kinder.push(kind);
        if self.journal.is_some() {
            self.protokoll(Aenderung::Kinder { eltern, knoten: kind, hinzu: true, vorher, nachher: None });
        }
    }

    /// Release 09.10.2026: ein Eintrag ins Aenderungsprotokoll (wenn an).
    pub fn protokoll(&mut self, a: Aenderung) {
        if self.journal.is_none() {
            return;
        }
        // Release 12.10.2026: Kinder dieses Eltern gehen in den gesammelten Satz.
        if let Some(s) = self.sammeln.as_mut()
            && let Aenderung::Kinder { eltern, knoten, hinzu, .. } = &a
            && *eltern == s.eltern
            && (*hinzu || s.weg_sammeln)
            && s.hinzu.len() + s.weg.len() < MAX_JOURNAL
        {
            if *hinzu { s.hinzu.push(*knoten) } else { s.weg.push(*knoten) }
            return;
        }
        if let Some(j) = self.journal.as_mut()
            && j.len() < MAX_JOURNAL
        {
            j.push(a);
        }
    }

    /// Release 12.10.2026: ab jetzt einen Satz fuer `eltern` sammeln (mit
    /// den Geschwistern des Satzes); `false`, wenn schon gesammelt wird.
    pub fn sammeln_an(&mut self, eltern: usize, vorher: Option<usize>, nachher: Option<usize>, weg_sammeln: bool) -> bool {
        if self.journal.is_none() || self.sammeln.is_some() {
            return false;
        }
        self.sammeln = Some(Sammler { eltern, hinzu: Vec::new(), weg: Vec::new(), vorher, nachher, weg_sammeln });
        true
    }

    /// Release 12.10.2026: entfernte Kinder ab jetzt in den Satz oder nicht.
    pub fn sammeln_weg(&mut self, an: bool) {
        if let Some(s) = self.sammeln.as_mut() {
            s.weg_sammeln = an;
        }
    }

    /// Release 12.10.2026: den gesammelten Satz ins Protokoll (wenn er etwas
    /// enthaelt).
    pub fn sammeln_aus(&mut self) {
        if let Some(s) = self.sammeln.take()
            && (!s.hinzu.is_empty() || !s.weg.is_empty())
        {
            self.protokoll(Aenderung::KinderSatz { eltern: s.eltern, hinzu: s.hinzu, weg: s.weg, vorher: s.vorher, nachher: s.nachher });
        }
    }

    /// Fuegt `kind` vor `vor` ein.
    ///
    /// Release 12.10.2026: erst abtrennen, dann die Stelle suchen. GEMESSEN
    /// vorher: `p.insertBefore(p.firstChild, p.lastChild)` bei `a b c` ergab
    /// `b c a` (die Stelle von `vor` galt noch mit `kind` davor) statt `b a c`.
    pub fn einfuegen_vor(&mut self, eltern: usize, kind: usize, vor: usize) {
        self.abtrennen(kind);
        let stelle = self.stelle_vor(eltern, vor);
        self.einfuegen_an(eltern, kind, stelle);
    }

    /// Die Stelle in der Kinderliste von `eltern`, an der `vor` steht
    /// (oder das Ende, wenn `vor` kein Kind ist).
    ///
    /// GEMESSENER FEHLER (03.10.2026): `einfuegen_vor` hat beides
    /// linear gemacht — erst die Stelle suchen, dann verschieben. Bei
    /// vielen foster-parenteten Knoten ist das quadratisch: an
    /// `<table>` mit N Stuecken `<b>x</b>` gemessen
    ///   2.000 0,014 s | 8.000 0,123 s | 16.000 0,413 s |
    ///   32.000 1,473 s | 64.000 5,670 s
    /// (M8 verlangt unter 1 s je Eingabe). Der Aufrufer merkt sich
    /// deshalb die Stelle (`foster_stelle` im Baumaufbau) und nutzt
    /// `einfuegen_an` — diese Suche bleibt fuer die anderen Aufrufer.
    pub fn stelle_vor(&self, eltern: usize, vor: usize) -> usize {
        self.knoten[eltern]
            .kinder
            .iter()
            .position(|&k| k == vor)
            .unwrap_or(self.knoten[eltern].kinder.len())
    }

    /// Fuegt `kind` an der Stelle `stelle` in die Kinderliste von
    /// `eltern` ein. Der Aufrufer prueft selbst, dass die Stelle stimmt
    /// (siehe `foster_stelle` im Baumaufbau) — deshalb ist hier keine
    /// Suche, sondern ein Verschieben ab Position `stelle`.
    pub fn einfuegen_an(&mut self, eltern: usize, kind: usize, stelle: usize) {
        self.abtrennen(kind);
        self.tree_changed();
        self.knoten[kind].eltern = Some(eltern);
        let stelle = stelle.min(self.knoten[eltern].kinder.len());
        self.knoten[eltern].kinder.insert(stelle, kind);
        if self.journal.is_some() {
            let k = &self.knoten[eltern].kinder;
            let vorher = stelle.checked_sub(1).and_then(|i| k.get(i)).copied();
            let nachher = k.get(stelle + 1).copied();
            self.protokoll(Aenderung::Kinder { eltern, knoten: kind, hinzu: true, vorher, nachher });
        }
    }

    /// Trennt einen Knoten von seinem Elternteil.
    pub fn abtrennen(&mut self, kind: usize) {
        if let Some(alt) = self.knoten[kind].eltern {
            self.tree_changed();
            // Release 12.10.2026: die lebenden Bereiche (DOM §4.2.3 „remove").
            if !self.bereiche.ist_leer()
                && let Some(i) = self.knoten[alt].kinder.iter().position(|&x| x == kind)
            {
                self.bereiche_vor_entfernen(kind, alt, i as u32);
            }
            if self.journal.is_some() {
                let k = &self.knoten[alt].kinder;
                let i = k.iter().position(|&x| x == kind);
                let vorher = i.and_then(|i| i.checked_sub(1)).and_then(|i| k.get(i)).copied();
                let nachher = i.and_then(|i| k.get(i + 1)).copied();
                self.protokoll(Aenderung::Kinder { eltern: alt, knoten: kind, hinzu: false, vorher, nachher });
            }
            self.knoten[alt].kinder.retain(|&k| k != kind);
        }
        self.knoten[kind].eltern = None;
    }

    /// Die Kinder eines Knotens.
    pub fn kinder(&self, i: usize) -> &[usize] {
        &self.knoten[i].kinder
    }

    /// Der Elternknoten.
    pub fn eltern(&self, i: usize) -> Option<usize> {
        self.knoten[i].eltern
    }

    /// Ist `i` ein Element mit diesem Namen und Namensraum?
    pub fn ist_element(&self, i: usize, raum: Namensraum, name: &str) -> bool {
        matches!(
            &self.knoten[i].art,
            Art::Element { raum: r, name: n, .. } if *r == raum && n == name
        )
    }

    /// Ein Attribut lesen.
    pub fn attribut(&self, i: usize, name: &str) -> Option<&str> {
        match &self.knoten[i].art {
            Art::Element { attribute, .. } => attribute
                .iter()
                .find(|a| a.name == name && a.raum == Namensraum::Html)
                .map(|a| a.wert.as_str()),
            _ => None,
        }
    }

    /// Ein Attribut setzen (ersetzt oder haengt an).
    pub fn setze_attribut(&mut self, i: usize, raum: Namensraum, name: &str, wert: &str) {
        if self.journal.is_some() && matches!(self.knoten[i].art, Art::Element { .. }) {
            let alt = match &self.knoten[i].art {
                Art::Element { attribute, .. } => attribute.iter().find(|a| a.name == name && a.raum == raum).map(|a| a.wert.clone()),
                _ => None,
            };
            let ns = match raum {
                Namensraum::Xlink => Some("http://www.w3.org/1999/xlink".to_string()),
                Namensraum::Xml => Some("http://www.w3.org/XML/1998/namespace".to_string()),
                Namensraum::Xmlns => Some("http://www.w3.org/2000/xmlns/".to_string()),
                _ => None,
            };
            self.protokoll(Aenderung::Attribut { knoten: i, name: name.to_string(), alt, ns });
        }
        self.attribute_changed(name);
        if let Art::Element { attribute, .. } = &mut self.knoten[i].art {
            if let Some(a) = attribute
                .iter_mut()
                .find(|a| a.name == name && a.raum == raum)
            {
                a.wert = wert.to_string();
            } else {
                attribute.push(Attribut {
                    raum,
                    name: name.to_string(),
                    wert: wert.to_string(),
                });
            }
        }
    }

    /// Die Tiefe eines Knotens (Wurzel = 0).
    pub fn tiefe(&self, i: usize) -> usize {
        let mut t = 0;
        let mut k = i;
        while let Some(e) = self.knoten[k].eltern {
            t += 1;
            k = e;
        }
        t
    }
}

/// Erzeugt die Testausgabe (Format der html5lib-tests).
///
/// `wurzel` ist das Dokument oder ein Fragment. Der erste Balken wird
/// wie in der Referenz gesetzt: `#document` fuer das Dokument,
/// `#document-fragment` fuer ein Fragment.
pub fn dump(dok: &Dokument, wurzel: usize) -> String {
    let mut raus = String::new();
    match &dok.knoten[wurzel].art {
        Art::Dokument => raus.push_str("#document"),
        _ => raus.push_str("#document-fragment"),
    }
    let kinder: Vec<usize> = dok.knoten[wurzel].kinder.clone();
    for k in kinder {
        dump_knoten(dok, k, 1, &mut raus);
    }
    raus
}

fn dump_knoten(dok: &Dokument, i: usize, indent: usize, raus: &mut String) {
    let leer = " ".repeat(indent);
    match &dok.knoten[i].art {
        Art::Dokument | Art::Fragment => {}
        Art::Doctype {
            name,
            public_id,
            system_id,
        } => {
            if public_id.is_empty() && system_id.is_empty() {
                raus.push_str(&format!("\n|{leer}<!DOCTYPE {name}>"));
            } else {
                raus.push_str(&format!(
                    "\n|{leer}<!DOCTYPE {name} \"{public_id}\" \"{system_id}\">"
                ));
            }
            return;
        }
        Art::Kommentar(text) => {
            raus.push_str(&format!("\n|{leer}<!-- {text} -->"));
        }
        // GEMESSEN (03.10.2026): das Testformat des html5lib-Korpus
        // kennt keinen PI-Knoten. Ich schreibe ihn in der Markup-Form,
        // die der Spec beim Serialisieren verlangt (13.3: "?", target,
        // " ", Daten, " ?>"), damit ein PI-Knoten im Dump sichtbar ist
        // und nicht still verschwindet. Die acht PI-Faelle des Korpus
        // erwarten `<!-- ?foo -->` und bleiben damit ausdruecklich rot —
        // das ist die Sichtbarkeit, die vorher fehlte.
        Art::Pi { target, daten } => {
            if daten.is_empty() {
                raus.push_str(&format!("\n|{leer}<?{target}>"));
            } else {
                raus.push_str(&format!("\n|{leer}<?{target} {daten}>"));
            }
        }
        Art::Text(text) => {
            if !text.is_empty() {
                raus.push_str(&format!("\n|{leer}\"{text}\""));
            }
        }
        Art::Element {
            raum,
            name,
            attribute,
            template_inhalt,
            shadow_root,
        } => {
            let anzeige = match raum.praefix() {
                Some(p) => format!("{p} {name}"),
                None => name.clone(),
            };
            raus.push_str(&format!("\n|{leer}<{anzeige}>"));
            let mut attrs: Vec<(String, String)> = attribute
                .iter()
                .map(|a| (a.anzeige(), a.wert.clone()))
                .collect();
            attrs.sort();
            for (n, w) in attrs {
                raus.push_str(&format!("\n|{leer}  {n}=\"{w}\""));
            }
            if let Some(inhalt) = template_inhalt {
                raus.push_str(&format!("\n|{leer}  content"));
                let kinder: Vec<usize> = dok.knoten[*inhalt].kinder.clone();
                for k in kinder {
                    dump_knoten(dok, k, indent + 4, raus);
                }
            }
            if let Some(sr) = shadow_root {
                let mode_str = match sr.mode {
                    ShadowRootMode::Open => "open",
                    ShadowRootMode::Closed => "closed",
                };
                raus.push_str(&format!("\n|{leer}  #shadow-root ({mode_str})"));
                for &k in &sr.children {
                    dump_knoten(dok, k, indent + 4, raus);
                }
            }
        }
        Art::ShadowRoot { mode, children } => {
            let mode_str = match mode {
                ShadowRootMode::Open => "open",
                ShadowRootMode::Closed => "closed",
            };
            raus.push_str(&format!("\n|{leer}#shadow-root ({mode_str})"));
            for &k in children {
                dump_knoten(dok, k, indent + 2, raus);
            }
        }
    }
    // Kinder eines Elements/Kommentars (Kommentar hat keine).
    if !matches!(dok.knoten[i].art, Art::Kommentar(_)) {
        let kinder: Vec<usize> = dok.knoten[i].kinder.clone();
        for k in kinder {
            dump_knoten(dok, k, indent + 2, raus);
        }
    }
}
