//! Oeffentliche Typen: Stylesheet, Selektoren, Werte, Berechnet, Element.
//!
//! Alles hier ist `pub`, weil die Rolle HTML dagegen programmiert
//! (Vertrag). Die Konstruktion passiert in `parse` und `kaskade`; die
//! Felder sind offen, damit Tests Zusicherungen schreiben koennen (M7).

use super::{
    BASIS_SCHRIFT_PX, MAX_AHNEN, MAX_GESCHWISTER_SCAN, MAX_HAS_BESUCHE, MAX_HAS_KANDIDATEN,
};

// ===========================================================================
// STYLESHEET
// ===========================================================================

/// Ein gelesenes Stylesheet. `regeln` ist nie laenger als
/// [`crate::MAX_REGELN`]; `verworfen` zaehlt, was wegen kaputter Syntax
/// oder Ueberschreitung der Grenzen wegfiel, plus 1 Pauschale, wenn die
/// Eingabe oder die Tokenliste abgeschnitten wurde (ehrlich gezaehlt,
/// nicht geschaetzt — siehe `parse`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stylesheet {
    /// Die gueltigen Regeln in Quellreihenfolge.
    pub regeln: Vec<Regel>,
    /// Anzahl verworfener Regeln (+ 1 bei abgeschnittenem Rest).
    pub verworfen: usize,
}

/// Eine Regel: Stilregel oder @media-Block mit Stilregeln darin.
/// Andere @-Regeln (@import, @font-face, @keyframes, ...) werden beim
/// Lesen uebersprungen — sie gehoeren nicht zur Teilmenge (C5).
#[derive(Debug, Clone, PartialEq)]
pub enum Regel {
    Stil(StilRegel),
    Media {
        abfrage: MediaAbfrage,
        regeln: Vec<StilRegel>,
    },
    Supports {
        bedingung: String,
        regeln: Vec<StilRegel>,
    },
    Layer {
        name: Option<String>,
        regeln: Vec<StilRegel>,
    },
    Import {
        url: String,
        abfrage: Option<MediaAbfrage>,
    },
    /// `@font-face { ... }` (CSS Fonts 4, §5; K9 01.10.2026).
    ///
    /// Eigene Variante und kein `Stil` mit Sonderwerten: die
    /// Deklarationen drinnen sind KEINE Eigenschaften eines Elements,
    /// sondern eine BESCHREIBUNG einer Schriftdatei. Sie duerfen nicht
    /// in die Kaskade.
    FontFace(SchriftQuelle),
}

/// Eine `@font-face`-Beschreibung (CSS Fonts 4, §5).
///
/// # WAS DRIN IST UND WARUM
///
/// `src`, `weight`, `style`, `stretch`, `unicode-range`, `display`.
///
/// `family` steht hier MIT drin: der Name gehoert zur Beschreibung,
/// und ohne ihn kann der Ladende nicht entscheiden, welche
/// `@font-face`-Regeln dieselbe Familie betreffen. Zwei `@font-face`
/// mit gleichem `family` und verschiedenem `weight` sind ZWEI
/// Schnitte DERSELBEN Schrift (CSS Fonts 4 §5.1) — die Zuordnung
/// „welcher Schnitt passt zu welcher Anfrage" ist eine Frage des
/// Ladens, nicht der Kaskade.
///
/// # GRENZEN
///
/// Alles ist auf eine LENGE gedeckelt: `MAX_SCHRIFT_QUELLEN` (4) fuer
/// die `src`-Liste, `MAX_UNICODE_BEREICHE` (64) fuer
/// `unicode-range`. Die Grenzen stehen in lib.rs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SchriftQuelle {
    /// `font-family` (CSS Fonts 4 §5.2). Leer = unbekannt.
    pub familie: String,
    /// `src`: die URL aus `url(...)`, in der angegebenen Reihenfolge.
    /// Die Liste ist nach FORM sortiert (CSS Fonts 4 §5.5: der
    /// Benutzer-Agent nimmt die erste, die er lesen kann) — hier
    /// steht sie so, wie sie dasteht.
    pub quellen: Vec<String>,
    /// Das Format je Quelle (CSS Fonts 4 §5.5), z. B. `"woff2"`.
    /// Leer heisst: die Quelle nennt kein Format.
    pub formate: Vec<String>,
    /// `font-weight` (CSS Fonts 4 §5.3): 100..900, `normal` = 400,
    /// `bold` = 700. `None` = nicht angegeben = `normal`.
    pub gewicht: Option<u16>,
    /// `font-style` (CSS Fonts 4 §5.2). Leer = `normal`.
    pub stil: String,
    /// `font-stretch` (CSS Fonts 4 §5.2). Leer = `normal`.
    pub streckung: String,
    /// `font-display` (CSS Fonts 4 §5.4): `auto`, `block`, `swap`,
    /// `fallback`, `optional`. Leer = `auto`.
    pub anzeige: String,
    /// `unicode-range` (CSS Fonts 4 §5.4): die Bereiche als
    /// Zeichenketten, z. B. `"U+0-7F"`. Leer = „alle".
    pub zeichen_bereiche: Vec<String>,
    /// Wie viele Deklarationen in dieser `@font-face` NICHT
    /// verstanden wurden. Wer sie liest, weiss, wie viel von der
    /// Beschreibung fehlt — ohne nachzaehlen zu muessen.
    pub unerleslich: usize,
}

/// `a, div.foo { color: red !important }`.
#[derive(Debug, Clone, PartialEq)]
pub struct StilRegel {
    /// Die Selektorgruppe (hoechstens MAX_SELEKTOREN_JE_REGEL).
    pub selektoren: Vec<Selektor>,
    /// Die gueltigen Deklarationen (Kurzformen expandiert).
    pub deklarationen: Vec<Deklaration>,
    /// Variablendefinitionen (`--name`, C10, W6).
    pub variablen: Vec<VariablenDekl>,
    /// Release 08.10.2026: Bedingungen verschachtelter Gruppen, die
    /// zusaetzlich zur Gruppe der Regel gelten muessen
    /// (`@layer a { @media (x) { ... } }`, `.k { @media (x) { ... } }`).
    /// Leer bei jeder Regel ohne Verschachtelung — kein Speicher.
    pub zusatz_medien: Vec<MediaAbfrage>,
}

// ===========================================================================
// MEDIA (C5)
// ===========================================================================

/// Geparste @media-Abfrage: Oder-Liste von Und-Bedingungen.
/// Komma trennt Oder, `and` trennt Und. `not` und Unbekanntes machen die
/// jeweilige Und-Zeile falsch (konservativ: Block wird ignoriert).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaAbfrage {
    /// Leere Liste = `@media all` = immer wahr.
    pub oder: Vec<MediaUnd>,
}

/// Eine Und-Zeile: Medientyp plus optionale Breiten-Schranken in px.
/// Nur px wird verstanden; andere Einheiten machen die Zeile falsch
/// (die Kaskade kennt nur die Breite, keine Aufloesung oder Hoehe).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ausrichtung {
    Hoch,
    Quer,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaUnd {
    pub typ_ok: bool,
    pub min_breite: Option<f32>,
    pub max_breite: Option<f32>,
    pub min_hoehe: Option<f32>,
    pub max_hoehe: Option<f32>,
    /// Offene Grenzen aus `<` / `>`; klassische min-/max-Angaben sind geschlossen.
    pub min_breite_offen: bool,
    pub max_breite_offen: bool,
    pub min_hoehe_offen: bool,
    pub max_hoehe_offen: bool,
    pub ausrichtung: Option<Ausrichtung>,
    /// `prefers-color-scheme: dark` (R0 d, 30.09.2026).
    ///
    /// `None` heisst "nicht gefragt". GEFRAGT wird nach der
    /// Systemeinstellung nicht — dieser Browser hat keinen Anschluss
    /// daran. Deshalb gilt: `dunkel` trifft nie, `hell` trifft immer,
    /// `no-preference` trifft immer. Das ist die ehrliche Antwort und
    /// keine, bei der eine Seite ihre Dunkelmodus-Regeln still verliert.
    pub farbschema: Option<Farbschema>,
    /// Die ganze Zeile ist mit `not` verneint.
    ///
    /// GEMESSENER FEHLER 30.09.2026 (R0 d): `not` hat `typ_ok` auf
    /// `false` gesetzt und damit die Zeile GETOETET. Bei der
    /// Kommaliste `"not screen, print"` war die Antwort "nichts" statt
    /// "print".
    pub negativ: bool,
    /// Unterzeilen aus `A or B` (Media Queries Level 4, 2.3).
    /// Leer = keine `or`-Verknuepfung, dann zaehlen die Felder oben.
    pub oder_zeilen: Vec<MediaUnd>,
}

/// Welches Farbschema gilt (`prefers-color-scheme`, K13).
///
/// ZWEI RICHTUNGEN, und die sind nicht dasselbe:
///
/// 1. Als `MediaZeile::farbschema` sagt das, was die SEITE
///    verlangt (`@media (prefers-color-scheme: dark)`).
/// 2. Als `Zustand::farbschema` sagt das, was die ANZEIGE ist
///    (das UI-Thema aus `narsil`).
///
/// Vor K13 gab es nur (1), und (1) wurde nie ausgewertet: „dunkel
/// gilt nie, hell gilt immer". Die Verbindung zwischen beiden ist
/// `MediaZeile::merkmale_trifft_mit_schema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Farbschema {
    /// `light` — hell. Der Vorgabe-Wert, weil ein Terminal im
    /// Zweifel hell ist; `Zustand::KEINER` traegt ihn.
    #[default]
    Hell,
    /// `dark` — dunkel.
    Dunkel,
    /// `no-preference` — der Nutzer hat nichts gesagt. Das ist NICHT
    /// dasselbe wie `Hell`: `prefers-color-scheme: no-preference`
    /// matcht nur bei `Keine` (CSS Media Queries 5 §2.2).
    Keine,
}

impl Default for MediaUnd {
    /// Eine leere Zeile: kein Merkmal, also WAHR.
    fn default() -> Self {
        MediaUnd {
            typ_ok: true,
            min_breite: None,
            max_breite: None,
            min_hoehe: None,
            max_hoehe: None,
            min_breite_offen: false,
            max_breite_offen: false,
            min_hoehe_offen: false,
            max_hoehe_offen: false,
            ausrichtung: None,
            farbschema: None,
            negativ: false,
            oder_zeilen: Vec::new(),
        }
    }
}

impl MediaUnd {
    /// Setzt die `or`-Unterzeilen. Builder, damit `parse_media_und`
    /// einen Ausdruck schreiben kann.
    pub fn mit_or(mut self, zeilen: Vec<MediaUnd>) -> Self {
        self.oder_zeilen = zeilen;
        self
    }
    /// Trifft die EIGENEN Felder (ohne `negativ` und ohne `or`).
    pub fn merkmale_trifft(&self, breite: f32, hoehe: f32) -> bool {
        self.merkmale_trifft_mit_schema(breite, hoehe, Farbschema::Hell)
    }

    /// Wie [`MediaZeile::merkmale_trifft`], aber mit dem Schema der
    /// Anzeige.
    ///
    /// # K13: DER BISHER HARTE WERT
    ///
    /// Vorher stand hier:
    ///
    /// ```text
    ///     let schema = match self.farbschema {
    ///         None | Some(Farbschema::Hell) | Some(Farbschema::Keine) => true,
    ///         Some(Farbschema::Dunkel) => false,
    ///     };
    /// ```
    ///
    /// Das ist kein Vergleich, das ist eine Festlegung: „dunkel gilt
    /// nie, hell gilt immer", unabhaengig von der Anzeige. GEMESSEN:
    ///
    /// ```text
    ///     @media (prefers-color-scheme: dark){p{color:blue}}
    ///     -> trifft(800, 600) = false
    /// ```
    ///
    /// und auf den 35 echten Stylesheets stehen 61
    /// `prefers-color-scheme`-Bloecke. Die Haelfte davon war dauerhaft
    /// falsch.
    ///
    /// # DIE REGEL JETZT (CSS Media Queries 5 §2.2)
    ///
    /// Die drei Werte sind drei FORDERUNGEN, keine Vorhersagen:
    ///
    /// ```text
    ///     prefers-color-scheme: light          matcht bei Hell
    ///     prefers-color-scheme: dark           matcht bei Dunkel
    ///     prefers-color-scheme: no-preference  matcht bei Keine
    /// ```
    ///
    /// # KORREKTUR AN MIR SELBST (01.10.2026, 10:10)
    ///
    /// In meiner ersten Fassung stand hier:
    ///
    /// ```text
    ///     None | Some(Farbschema::Hell) => true,
    /// ```
    ///
    /// mit der Begruendung „die Seite sagt ‚ich unterstuetze beide'".
    /// Das ist falsch. `prefers-color-scheme: light` in einer
    /// `@media`-Abfrage ist KEINE Auskunft ueber die Seite, sondern
    /// eine Frage an die Anzeige: „trifft zu, wenn es hell ist?".
    ///
    /// GEMESSENE FOLGE, und der Test hat sie gefangen
    /// (`der_hell_block_gilt_nur_wenn_die_anzeige_hell_ist`): bei
    /// dunkler Anzeige traf `… : light` trotzdem, also zeichnete die
    /// hell-Variante im dunklen Schema. Die Kopplung waere dann nur auf
    /// einer Seite funktioniert.
    pub fn merkmale_trifft_mit_schema(&self, breite: f32, hoehe: f32, anzeige: Farbschema) -> bool {
        let quer = breite >= hoehe;
        let aus = self.ausrichtung.map_or(true, |o| match o {
            Ausrichtung::Quer => quer,
            Ausrichtung::Hoch => !quer,
        });
        let schema = match self.farbschema {
            // Die Seite sagt nichts — dann wird nicht gefragt.
            None => true,
            // Drei Forderungen, drei Vergleiche.
            Some(gefragt) => anzeige == gefragt,
        };
        self.typ_ok
            && schema
            && self.min_breite.map_or(true, |m| {
                if self.min_breite_offen { breite > m } else { breite >= m }
            })
            && self.max_breite.map_or(true, |m| {
                if self.max_breite_offen { breite < m } else { breite <= m }
            })
            && self.min_hoehe.map_or(true, |m| {
                if self.min_hoehe_offen { hoehe > m } else { hoehe >= m }
            })
            && self.max_hoehe.map_or(true, |m| {
                if self.max_hoehe_offen { hoehe < m } else { hoehe <= m }
            })
            && aus
    }

    /// Die ganze Zeile: `or`-Unterzeilen ODER eigene Merkmale, dann `not`.
    ///
    /// OHNE Schema: es gilt `Farbschema::Hell`. GEMESSEN, warum das bis
    /// K13 ein HARTER WERT war und nicht eine Vorgabe:
    ///
    /// ```text
    ///     p{color:red}
    ///     @media (prefers-color-scheme: dark){p{color:blue}}
    ///     -> trifft(800, 600) = false
    /// ```
    ///
    /// Der Dunkel-Block GILT NIE und der Hell-Block IMMER — auf jeder
    /// Seite, in jeder Anzeige. Bei 61 `prefers-color-scheme`-Bloecken
    /// auf den 35 echten Stylesheets ist die Haelfte davon dauerhaft
    /// falsch.
    ///
    /// Siehe [`MediaZeile::trifft_mit_schema`] und
    /// [`Zustand::farbschema`].
    pub fn trifft(&self, breite: f32, hoehe: f32) -> bool {
        self.trifft_mit_schema(breite, hoehe, Farbschema::Hell)
    }

    /// Wie [`MediaZeile::trifft`], aber mit dem Schema der Anzeige
    /// (K13, CSS Media Queries 5 §2.2: `prefers-color-scheme` vergleicht
    /// die Benutzereinstellung mit dem angefragten Wert).
    pub fn trifft_mit_schema(&self, breite: f32, hoehe: f32, schema: Farbschema) -> bool {
        let mut treffer = if self.oder_zeilen.is_empty() {
            self.merkmale_trifft_mit_schema(breite, hoehe, schema)
        } else {
            self.oder_zeilen
                .iter()
                .any(|z| z.trifft_mit_schema(breite, hoehe, schema))
        };
        if self.negativ {
            treffer = !treffer;
        }
        treffer
    }
}

impl MediaAbfrage {
    /// True, wenn mindestens eine Und-Zeile bei dieser Breite zutrifft.
    /// Leere Abfrage (`all`) ist immer wahr.
    ///
    /// OHNE Schema: `Farbschema::Hell`. Das ist die alte, harte
    /// Annahme; fuer `prefers-color-scheme` braucht es
    /// [`MediaAbfrage::trifft_mit_schema`].
    pub fn trifft(&self, ansicht_breite: u32, ansicht_hoehe: u32) -> bool {
        self.trifft_mit_schema(ansicht_breite, ansicht_hoehe, Farbschema::Hell)
    }

    /// Wie [`MediaAbfrage::trifft`], aber mit dem Schema der Anzeige.
    ///
    /// GEMESSENER ANLASS (K13): `MediaZeile::farbschema` wurde
    /// gespeichert und nie ausgewertet. Der Block
    /// `@media (prefers-color-scheme: dark)` lieferte `false` und der
    /// Block `… : light` lieferte `true` — unabhaengig von allem.
    pub fn trifft_mit_schema(
        &self,
        ansicht_breite: u32,
        ansicht_hoehe: u32,
        schema: Farbschema,
    ) -> bool {
        if self.oder.is_empty() {
            return true;
        }
        let breite = ansicht_breite as f32;
        let hoehe = ansicht_hoehe as f32;
        for u in &self.oder {
            if u.trifft_mit_schema(breite, hoehe, schema) {
                return true;
            }
        }
        false
    }
}

// ===========================================================================
// SELEKTOREN (C2)
// ===========================================================================

/// Ein Einzelselektor: Kette von Gliedern (links nach rechts) plus die
/// Spezifitaet nach Selectors Level 3 als (ids, klassen, typen).
/// Bei Gleichstand gewinnt die spaetere Regel (Quellreihenfolge).
#[derive(Debug, Clone, PartialEq)]
pub struct Selektor {
    /// Kette links nach rechts; das erste Glied hat Kombinator Keiner.
    /// Hoechstens MAX_SELEKTOR_TEILE Glieder, sonst ist der Selektor
    /// ungueltig (er wird verworfen, nicht abgeschnitten).
    pub kette: Vec<KettenGlied>,
    /// (a, b, c): IDs / Klassen+Attribute+Pseudoklassen / Typen.
    /// `*`, Kombinatoren und `:not()` selbst zaehlen nicht; das
    /// `:not()`-Argument zaehlt mit.
    pub spezifitaet: (u32, u32, u32),
}

/// Verbindung eines Kettenglieds zum LINKEN Nachbarn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kombinator {
    /// Erstes Glied (kein linker Nachbar).
    Keiner,
    /// `A B` — B ist irgendwo unter A.
    Nachfahre,
    /// `A > B` — B ist direktes Kind von A.
    Kind,
    /// `A + B` — B ist das naechste Geschwister nach A.
    Nachbar,
    /// `A ~ B` — B ist ein spaeteres Geschwister von A.
    Geschwister,
}

/// Ein Kettenglied: Kombinator zum linken Nachbarn plus die einfache
/// Auswahl (Tag, ID, Klassen, Attribute, Pseudos) als UND-Bedingung.
#[derive(Debug, Clone, PartialEq)]
pub struct KettenGlied {
    /// Verbindung zum linken Nachbarn (erstes Glied: Keiner).
    pub komb: Kombinator,
    /// Die UND-Bedingung an diesem Knoten.
    pub einfach: Einfach,
}

/// UND-Bedingung an einem Knoten: alles Gesetzte muss zutreffen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Einfach {
    /// Typselektor in Kleinbuchstaben ("p", "div"). None = beliebig.
    /// `*` ist ebenfalls None (universal, zaehlt nicht zur Spezifitaet).
    pub tag: Option<String>,
    /// `#id` ohne "#". None = keine ID-Bedingung.
    pub id: Option<String>,
    /// `.klasse`-Bedingungen ohne "." (alle muessen zutreffen).
    pub klassen: Vec<String>,
    /// `[attr]`- / `[attr=wert]`-Bedingungen (alle muessen zutreffen).
    pub attribute: Vec<AttrBed>,
    /// Pseudoklassen (alle muessen zutreffen).
    pub pseudos: Vec<Pseudo>,
}

/// Attribut-Vergleichsoperator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    Gleich,  // =
    Praefix, // ^=
    Suffix,  // $=
    Teil,    // *=
    Wort,    // ~=
    Sprache, // |=
}

/// Attributbedingung: `[name]`, `[name=wert]`, `[name^=wert]` usw., ggf. mit `i` oder `s` Flag.
#[derive(Debug, Clone, PartialEq)]
pub struct AttrBed {
    pub name: String,
    /// (Operator, Wert, GrossKleinschreibung-Ignorieren). Release
    /// 2026-10-15: `Some(true)` = the `i` flag, `Some(false)` = the `s`
    /// flag, `None` = no flag (HTML §4.16.2: the legacy attribute list of
    /// HTML elements in HTML documents compares ASCII case-insensitively).
    pub bedingung: Option<(AttrOp, String, Option<bool>)>,
}

/// Unterstuetzte Pseudoklassen (C2/C9). `:visited` ist immer falsch
/// (Privatsphaere: der Stil darf nicht verraten, was besucht wurde),
/// `:hover`/`:focus` treffen nur mit passendem [`Zustand`] (W5).
#[derive(Debug, Clone, PartialEq)]
pub enum Pseudo {
    ErstKind,
    LetztKind,
    EinzigKind,
    Link,
    Besucht,
    Schweber,
    Fokus,
    /// `:active` (CSS Selectors 4 §6.1.2). GEMESSEN: 205 Vorkommen.
    Aktiv,
    /// `:focus-visible` (Selectors 4 §6.1.4). GEMESSEN: 148 Vorkommen.
    FocusSichtbar,
    /// Release 11.10.2026: `:popover-open` (HTML §6.12) — das Element traegt
    /// das interne Attribut `" narsil-popover-open"` (vom Host gesetzt).
    PopoverOffen,
    /// Release 11.10.2026: `:modal` (Selectors 4 §13.4) — ein mit
    /// `showModal()` geoeffneter Dialog (internes Attribut `" narsil-modal"`).
    Modal,
    /// `:target` (Selectors 4 §6.2.2). GEMESSEN: 11 Vorkommen.
    Ziel,
    /// `:checked` (Selectors 4 §6.3.2). GEMESSEN: 49 Vorkommen.
    Ausgewaehlt,
    /// `:disabled` (Selectors 4 §6.3.4). GEMESSEN: 83 Vorkommen.
    Gesperrt,
    /// `:enabled` (Selectors 4 §6.3.4). GEMESSEN: 159 Vorkommen.
    Aktiviert,
    /// `:indeterminate` (Selectors 4 §6.3.2). GEMESSEN: 8 Vorkommen.
    Unbestimmt,
    /// `:read-only` (Selectors 4 §6.3.5). GEMESSEN: 1 Vorkommen.
    NurLesen,
    /// `:read-write` (Selectors 4 §6.3.5).
    Lesbar,
    /// Release 10.10.2026: `:placeholder-shown` (CSS Selectors 4, Eingabe-Pseudoklassen) — ein
    /// Feld, das gerade seinen Platzhalter zeigt (schwebende Labels:
    /// `:not(:placeholder-shown) + label`).
    PlatzhalterSichtbar,
    /// `:first-of-type` (Selectors 4 §6.3.7). GEMESSEN: 51 Vorkommen.
    ErstVonTyp,
    /// `:last-of-type` (Selectors 4 §6.3.7). GEMESSEN: 41 Vorkommen.
    LetztVonTyp,
    /// `:only-of-type` (Selectors 4 §6.3.7). GEMESSEN: 0 Vorkommen auf
    /// den 35 Dateien — gebaut, weil 0 nicht „wird nicht gebraucht"
    /// heisst, sondern nur „stand nicht auf diesen Dateien".
    EinzigVonTyp,
    /// Release 2026-10-15: the form pseudo-classes of HTML §4.16.3 (see
    /// `crate::form_state`).
    Required,
    Optional,
    Valid,
    Invalid,
    InRange,
    OutOfRange,
    Default,
    /// `:open` (`<details>`/`<dialog>` with `open`).
    Open,
    /// Release 2026-10-15: `:heading` / `:heading(n, …)` (empty = any
    /// level; see `crate::form_state::heading_level`).
    Heading(Vec<i32>),
    Leer,
    Wurzel,
    /// Release 13.10.2026 (Agent C): the subject is in an `@scope`, tested per
    /// root (`crate::scope_match`): root list, limit list. Only produced by
    /// `scope.rs` (`:-narsil-scope-in(…):-narsil-scope-limit(…)`),
    /// specificity 0.
    ScopeIn(Vec<Selektor>, Vec<Selektor>),
    /// `:scope` inside an `@scope` limit (`:-narsil-scope-anchor`): the root
    /// currently being tested.
    ScopeAnchor,
    /// Release 14.10.2026: the class prefix selector `.foo-*` (Selectors 5,
    /// tentative): a class that starts with the prefix. Specificity of a class.
    ClassPrefix(String),
    /// `:lang(de)`, `:lang(de, de-AT)` — die Funktionsform von
    /// `[lang|=de]`.
    ///
    /// GEMESSENER FEHLER, den diese Variante behebt (01.10.2026 21:10):
    /// `:lang()` stand nicht im `Pseudo`-Enum, und der Parser gab bei
    /// einem unbekannten Namen `return None` — die GANZE Regel fiel.
    /// GEMESSEN an `ol:lang(ar) > li { … }` aus
    /// `wikipedia_vector-2022.css`: 0 Deklarationen, `verworfen = 1`.
    ///
    /// GEMESSENE REICHWEITE: **2** Regelbloecke in dieser einen Datei,
    /// zusammen **14** `:lang()`-Varianten, groesster Selektor einer
    /// Liste: 7. Es ist die einzige Datei des Korpus mit `:lang()`,
    /// und sie traegt Wikipedias Sprachausgaben-Regeln.
    ///
    /// GEMESSENE KORREKTUR (21:35): Ich hatte zuerst „27 Bloecke und
    /// 252 Varianten" gemessen. Das war ein **Artefakt**: der Zaehler
    /// suchte Bloecke ueber `[^{}]*\{[^{}]*\}`, und die Datei hat
    /// `@counter-style`-Bloecke mit einem `}` INNERHALB eines Strings
    /// (`suffix:') '}`). Der Regex hat zerschnitten und Rulemente
    /// erfunden. Mit string-bewusstem Zaehlen: 2 Bloecke, 14 Varianten
    /// — und der Parser holt alle 14. Einzelheiten in `lib.rs` bei
    /// `MAX_SELEKTOREN_JE_REGEL`.
    ///
    /// Warum das eine eigene Variante ist und nicht der
    /// Attribut-Operator: `[lang|=de]` ist eine Attributabfrage und
    /// laeuft ueber `AttrOp::Sprache`. `:lang()` ist eine
    /// Pseudoklasse — sie hat eine eigene Position in der
    /// Spezifikationsordnung und eigene Regeln (Selectors Level 4,
    /// 6.5.17) fuer die Liste.
    /// `:focus-within` (CSS Selectors 4 §6.1.4).
    ///
    /// GEMESSENE REICHWEITE (C1, WPT, 02.10.2026): **27 Faelle** in
    /// `css/selectors` benutzen die Pseudoklasse, und der Parser kannte
    /// den Namen nicht — der gab `None` zurueck, und damit fiel die
    /// GANZE Regel. Das ist der groesste Posten, der sich ohne
    /// Interaktionsmodell auswerten laesst.
    ///
    /// Die Variante traegt kein Feld: die Antwort steht in
    /// [`Zustand::focus_within`], weil sie eine Eigenschaft des Zustands
    /// ist und nicht des Selektors.
    FocusWithin,
    Sprache(Vec<String>),
    /// Release 15.10.2026 (Agent CSS2): `:dir(ltr|rtl)` (Selectors 4 §7.1),
    /// matched against the HTML directionality of the element (see
    /// [`directionality`]). `None` = another identifier: valid, matches
    /// nothing.
    Richtung(Option<SchreibRichtung>),
    Nicht(Vec<Selektor>),
    /// Release 12.10.2026: `:host` / `:host(<compound>)` (CSS Scoping §3.2.1).
    /// Trifft nur in einem Stylesheet einer Shadow Root — `narsil-page`
    /// schreibt es dort in eine Bedingung auf das interne Wirt-Attribut um
    /// (siehe `narsil_page::schatten`). Im Dokument trifft es nie.
    Wirt(Vec<Selektor>),
    /// Release 12.10.2026: `::slotted(<compound>)` (CSS Scoping §3.2.3),
    /// umgeschrieben wie [`Pseudo::Wirt`]; im Dokument trifft es nie.
    Eingesetzt(Vec<Selektor>),
    Ist(Vec<Selektor>),
    Wo(Vec<Selektor>),
    Hat(Vec<Selektor>),
    /// an+b (a, b)
    NtesKind(i32, i32, Vec<Selektor>),
    NtesLetztesKind(i32, i32, Vec<Selektor>),
    NtesTyp(i32, i32),
    /// `:nth-last-of-type(an+b)` — die Umkehrung von `NtesTyp`.
    ///
    /// C1, 02.10.2026: der WPT-Fall
    /// `selectors/child-indexed-during-parse.html` braucht
    /// `:nth-last-of-type(1)`, und der Parser kannte die Variante
    /// nicht. Ein unbekannter Funktionsname gibt `None` zurueck — und
    /// damit fiel die **ganze Regel** (dieselbe Klasse wie `:lang()`
    /// und `:focus-within()`, beide am selben Tag gefunden).
    ///
    /// GEMESSENE REICHWEITE: 1 Vorkommen in den 35 echten
    /// Stylesheets, 1 im WPT-Korpus. Wenig — aber eine verlorene Regel
    /// ist eine verlorene Regel, unabhaengig von der Haeufigkeit.
    ///
    /// GEMESSENE ENTSCHEIDUNG ueber die Form: ZWEI Felder wie
    /// `NtesTyp`, nicht drei wie `NtesKind`. Der `of`-Filter
    /// (`:nth-child(2 of .foo)`) wird von `NtesTyp` heute schon
    /// verworfen; diese Variante macht es nicht besser und nicht
    /// schlechter. Eine dritte Zahl waere eine Auslegung an einer
    /// Stelle, an der `NtesTyp` sie auch nicht hat.
    NtesLetztesTyp(i32, i32),
    /// `::before` — ein **Pseudoelement**, keine Pseudoklasse.
    ///
    /// 06.10.2026: Der Parser las `::` bisher gar nicht (`return None`),
    /// und damit fiel die GANZE Regel. GEMESSEN an de.wikipedia.org:
    /// `.skin-vector-2022 .mw-logo::before` traegt die Kugel,
    /// `::after` die Wortmarke. Die Marke steht hier nur als
    /// PARSE-Ergebnis: [`crate::typen::einfach_passt`] erfuellt sie
    /// **nie** (die Regel haengt sonst am Element), und `narsil-page`
    /// baut aus ihr eine eigene Box.
    Vorher,
    /// `::after` — siehe [`Pseudo::Vorher`].
    Nachher,
    /// Release 10.10.2026: `::placeholder` (CSS Pseudo-Elements 4 §4.3) —
    /// eine MARKE wie [`Pseudo::Vorher`]: `pseudo_passt` erfuellt sie nie,
    /// `narsil-page` rechnet daraus Farbe und Deckkraft des Platzhaltertextes.
    Platzhalter,
    /// Release 10.10.2026: `::first-letter` (CSS Pseudo-Elements 4 §2.3) —
    /// eine MARKE wie [`Pseudo::Vorher`]; `narsil-page` setzt den ersten
    /// Buchstaben (mit Satzzeichen davor und danach) als eigenen Abschnitt.
    ErsterBuchstabe,
    /// Release 15.10.2026 (Agent CSS2): `::first-line` (CSS Pseudo-Elements 4
    /// §2.2) — a marker like [`Pseudo::ErsterBuchstabe`]; `narsil-page` gives
    /// the first formatted line of the block its colour.
    ErsteZeile,
}

// ===========================================================================
// DEKLARATIONEN UND WERTE (C3/C4)
// ===========================================================================

/// Eine einzelne gueltige Deklaration: Langform-Eigenschaft plus Wert.
/// Kurzformen (margin, font, border, background, ...) stehen hier nie —
/// der Parser expandiert sie in Langformen (C3).
#[derive(Debug, Clone, PartialEq)]
pub struct Deklaration {
    /// Die Eigenschaft (nur Langformen, siehe [`Eigenschaft`]).
    pub eigenschaft: Eigenschaft,
    /// Der gepruefte Wert. Platzhalter (Wort), wenn der Rohtext erst in
    /// der Kaskade gilt (var()/calc() — C10/C11, W6): dann entscheidet
    /// `roh`, nicht dieser Wert.
    pub wert: Wert,
    /// `!important` — schlaegt in der Kaskade alles Normale.
    pub wichtig: bool,
    /// Rohtext des Werts (gedeckelt, wie geschrieben). Die Kaskade loest
    /// daraus var()/calc() mit den Variablen des Elements auf.
    pub roh: String,
}

/// Variablendefinition (`--name: wert`, C10, W6). Eigene Liste neben den
/// Deklarationen, weil der Name kein [`Eigenschaft`]-Fall ist und die
/// Aufloesung (Vererbung, Zyklen) eigenen Regeln folgt.
#[derive(Debug, Clone, PartialEq)]
pub struct VariablenDekl {
    /// Name MIT `--`, original Gross/Klein (case-sensitiv, CSS-Regel).
    pub name: String,
    /// Rohtext des Werts (gedeckelt). Kann selbst var() enthalten.
    pub roh: String,
    /// `!important` (wie bei Deklarationen).
    pub wichtig: bool,
}

/// Aufgeloeste Variablen EINES Elements (vererbt + kaskadiert).
///
/// Release 08.10.2026: eine KETTE geteilter Ebenen statt einer Kopie je
/// Element. GEMESSEN vorher: jedes Element klonte die Liste seiner
/// Eltern, und die Liste war auf 64 Eintraege gedeckelt — github.com
/// setzt rund 2000 Variablen auf `:root`, Tailwind 4 einige Hundert. Fast
/// alle gingen verloren, und mit ihnen Farben, Abstaende und Schriften.
/// Jetzt teilt ein Element die Ebene seiner Eltern (`Arc`), und nur ein
/// Element, das selbst Variablen setzt, bekommt eine neue Ebene.
///
/// M3: die Kette ist hoechstens [`MAX_VAR_KETTE`] Ebenen lang; darueber
/// wird sie zu einer Ebene zusammengelegt (Suche bleibt begrenzt).
#[derive(Debug, Clone, Default)]
pub struct Variablen {
    ebene: Option<std::sync::Arc<VarEbene>>,
}

/// Hoechste Kettenlaenge der Variablen-Ebenen (M3).
pub const MAX_VAR_KETTE: u16 = 32;

#[derive(Debug)]
struct VarEbene {
    eigene: std::collections::HashMap<String, String>,
    eltern: Option<std::sync::Arc<VarEbene>>,
    laenge: u16,
}

impl Variablen {
    /// Die Variablen eines Elements: geerbte (`eltern`) plus eigene
    /// Gewinner. Ohne eigene teilt das Element die Ebene der Eltern.
    pub fn mit_eigenen(eltern: Option<&Variablen>, eigene: Vec<(String, String)>) -> Variablen {
        let basis = eltern.and_then(|e| e.ebene.clone());
        if eigene.is_empty() {
            return Variablen { ebene: basis };
        }
        let mut karte: std::collections::HashMap<String, String> =
            std::collections::HashMap::with_capacity(eigene.len());
        for (n, v) in eigene {
            karte.insert(n, v);
        }
        let laenge = basis.as_ref().map_or(1, |b| b.laenge + 1);
        if laenge > MAX_VAR_KETTE {
            // Zusammenlegen: alle Ebenen in eine (die naeheren gewinnen).
            let mut alles = std::collections::HashMap::new();
            let mut stapel = Vec::new();
            let mut e = basis.as_deref();
            while let Some(eb) = e {
                stapel.push(eb);
                e = eb.eltern.as_deref();
            }
            for eb in stapel.iter().rev() {
                for (n, v) in &eb.eigene {
                    alles.insert(n.clone(), v.clone());
                }
            }
            alles.extend(karte);
            return Variablen {
                ebene: Some(std::sync::Arc::new(VarEbene {
                    eigene: alles,
                    eltern: None,
                    laenge: 1,
                })),
            };
        }
        Variablen {
            ebene: Some(std::sync::Arc::new(VarEbene {
                eigene: karte,
                eltern: basis,
                laenge,
            })),
        }
    }

    /// Wert einer Variablen (Name mit `--`, case-sensitiv).
    pub fn holen(&self, name: &str) -> Option<&str> {
        let mut e = self.ebene.as_deref();
        while let Some(eb) = e {
            if let Some(v) = eb.eigene.get(name) {
                return Some(v.as_str());
            }
            e = eb.eltern.as_deref();
        }
        None
    }

    /// Alle sichtbaren Variablen (fuer Tests und Werkzeuge), nach Namen sortiert.
    pub fn eintraege(&self) -> Vec<(String, String)> {
        let mut alles: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
        let mut e = self.ebene.as_deref();
        while let Some(eb) = e {
            for (n, v) in &eb.eigene {
                alles.entry(n.as_str()).or_insert(v.as_str());
            }
            e = eb.eltern.as_deref();
        }
        alles
            .into_iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    /// Keine einzige Variable sichtbar.
    pub fn ist_leer(&self) -> bool {
        self.ebene.is_none()
    }
}

impl PartialEq for Variablen {
    fn eq(&self, other: &Self) -> bool {
        match (&self.ebene, &other.ebene) {
            (None, None) => true,
            (Some(a), Some(b)) if std::sync::Arc::ptr_eq(a, b) => true,
            _ => self.eintraege() == other.eintraege(),
        }
    }
}

/// Unterstuetzte Langform-Eigenschaften (C4). Alles andere wird beim
/// Lesen still ignoriert (die Deklaration faellt weg, kein Fehler).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Eigenschaft {
    /// display: block/inline/inline-block/list-item/none.
    Anzeige,
    /// color.
    Farbe,
    /// background-color.
    Hintergrund,
    /// font-size.
    Schriftgroesse,
    /// font-weight.
    Schriftgewicht,
    /// font-style.
    Schriftstil,
    /// font-family (erste Familie der Liste).
    Schriftfamilie,
    /// line-height.
    Zeilenhoehe,
    /// text-align.
    Textausrichtung,
    /// text-decoration (als Bitmaske in [`Berechnet::dekoration`]).
    Textdekoration,
    /// white-space.
    Weissraum,
    /// margin-top/right/bottom/left.
    MargeOben,
    MargeRechts,
    MargeUnten,
    MargeLinks,
    /// `margin-inline-start` — die Marge an der ANFANGSEITE der Zeile.
    ///
    /// GEMESSENER FEHLER, den diese Variante behebt (K17b): Der Parser
    /// hat `margin-inline-start` fest auf [`Eigenschaft::MargeLinks`]
    /// gelegt. Das stimmt nur fuer `ltr`. In `rtl` ist die
    /// Anfangsseite die RECHTE, und `margin-inline-start: 1.6em`
    /// landete links. Im Korpus (417 KB, 8 Blaetter) stehen 22
    /// logische Margen — davon 4 in der Zwei-Wert-Form
    /// `margin-inline: 0 0.25em`, die man nur spiegeln kann, wenn man
    /// weiss, welche Seite die Anfangsseite ist.
    ///
    /// WARUM EIGENE VARIANTE und nicht der Schluessel-Eimer wie bei
    /// `inset-inline-*`: Hier geht es um Prozent, `auto` und die
    /// geerbten Vorgaaben — also um den vollen Aufloesungsweg. Der
    /// Eimer loest nichts auf; er legt nur einen Text ab.
    MargeInnenStart,
    /// `margin-inline-end` — die Marge an der ENDSEITE der Zeile.
    MargeInnenEnde,
    /// `padding-inline-start`, gespiegelt wie [`Eigenschaft::MargeInnenStart`].
    ///
    /// GEMESSENE REDLICHKEIT: Im Korpus kommt `padding-inline` NULL
    /// Mal vor. Gebaut wird es trotzdem, weil es derselbe Dreizeiler
    /// ist und `margin-inline` nicht allein stehen sollte. Diese Zahl
    /// steht hier, damit niemand sie als gemessen ausgibt.
    FuellungInnenStart,
    /// `padding-inline-end`, gespiegelt wie [`Eigenschaft::MargeInnenEnde`].
    FuellungInnenEnde,
    /// L4 (03.10.2026 09:5x): `align-self` (CSS Box Alignment 3 §6.3).
    ///
    /// GEMESSENER BEFUND, dass die Eigenschaft nicht ankam: `align-self`
    /// und `justify-self` wurden als `ESchluessel` gelesen, und den liest
    /// der Rasterizer nicht. GEMESSEN (Release, `examples/l4_raster.rs`,
    /// Block AA): alle fuenf Werte ergaben dieselbe Zelle.
    AusrichtungSelbst,
    /// `justify-items` (§6.2) — der Vorgabewert fuer `justify-self`.
    ///
    /// L4 (03.10.2026 11:5x). GEMESSENER BEFUND, dass die Eigenschaft
    /// nicht ankam: sie zeigte im Parser auf `ESchluessel`.
    JustifyAusrichtung,
    /// `justify-self` (§6.2) — dieselbe Wertemenge, andere Achse.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Variante KEINE Nutzlast traegt:
    /// `align-items` (`Eigenschaft::FlexAusrichtung`) traegt auch keine —
    /// der Wert steht in `Berechnet`, nicht im Enum. Und der Import
    /// `use Eigenschaft::{ … }` holt VARIANTEN; ein Tuple wuerde dort als
    /// Muster `EAusrichtungSelbst(..)` verlangen.
    AusrichtungSelbstWaagerecht,
    /// padding-top/right/bottom/left.
    FuellungOben,
    FuellungRechts,
    FuellungUnten,
    FuellungLinks,
    /// border-top/right/bottom/left-width.
    RahmenObenBreite,
    RahmenRechtsBreite,
    RahmenUntenBreite,
    RahmenLinksBreite,
    /// border-top/right/bottom/left-style.
    RahmenObenStil,
    RahmenRechtsStil,
    RahmenUntenStil,
    RahmenLinksStil,
    /// border-top/right/bottom/left-color.
    RahmenObenFarbe,
    RahmenRechtsFarbe,
    RahmenUntenFarbe,
    RahmenLinksFarbe,
    /// width / max-width / min-width / height (Laenge oder auto).
    Breite,
    MaxBreite,
    MinBreite,
    Hoehe,
    /// min-height in px (K1, 01.10.2026).
    ///
    /// GEMESSENE LUECKE: `min-height: 100vh` steht auf praktisch jeder
    /// Seite mit klebendem Kopf, und die Kaskade kannte nur `height`.
    /// `LayoutStil` in `narsil-layout` verlangt das Feld inzwischen —
    /// siehe `layout_plan.rs`.
    MinHoehe,
    /// max-height in px. `None` = nicht gesetzt.
    MaxHoehe,
    /// list-style-type.
    Listenstil,
    /// visibility.
    Sichtbarkeit,
    /// overflow (nur gemerkt, C4).
    Ueberlauf,
    /// flex-direction (C8, W5): row/column (Teilmenge, Rest faellt weg).
    FlexRichtung,
    /// justify-content (C8, W5).
    FlexBuendig,
    /// align-items (C8, W5).
    FlexAusrichtung,
    /// gap in px (C8, W5).
    Luecke,
    /// flex-grow als Faktor (C8, W5).
    FlexWachstum,
    /// flex-shrink als Faktor (Anfangswert 1, NICHT vererbt, C12, W6).
    FlexSchrumpf,
    /// flex-basis (Anfangswert auto, NICHT vererbt, C12, W6).
    FlexBasis,
    /// flex-wrap (Anfangswert nowrap, NICHT vererbt, C12, W6).
    FlexUmbruch,
    /// position (Anfangswert statisch, NICHT vererbt, C13, W6).
    Position,
    /// top/right/bottom/left in px (None = auto, NICHT vererbt, C13, W6).
    Oben,
    Rechts,
    Unten,
    Links,
    /// z-index (None = auto, NICHT vererbt, C13, W6).
    ZIndex,
    /// float (Anfangswert keins, NICHT vererbt, C14, W6).
    Schweben,
    /// clear (Anfangswert keins, NICHT vererbt, C14, W6).
    Raeumung,
    /// grid-template-columns (leer = keine, NICHT vererbt, C15, W6).
    RasterSpalten,
    /// `grid-template-rows` (CSS Grid 1 §8.3).
    ///
    /// L4 (02.10.2026 18:2x). GEMESSENER BEFUND, warum es diese
    /// eigene Eigenschaft gibt: `ERaster` war ein Alias fuer
    /// `RasterSpalten`, und `grid-template-rows` ging darauf — die
    /// Zeilen landeten in den SPALTENbreiten (GEMESSEN:
    /// `grid-template-rows: 200px 200px` ergab
    /// `[Fest(200), Fest(200)]` als Spalten).
    RasterZeilen,
    /// `grid-auto-rows` (CSS Grid 1 §7.5).
    ///
    /// L4 (02.10.2026 21:5x). GEMESSENER BEFUND, warum es diese eigene
    /// Eigenschaft gibt: `grid-auto-flow`, `grid-auto-columns` UND
    /// `grid-auto-rows` gingen alle auf `EGitterAutomatik`, also auf EIN
    /// Feld `gitter_automatik: String`. GEMESSEN in
    /// `tests/position.rs`, `l4_grid_auto_teilt_sich_ein_feld_mit_dem_
    /// fluss`: `grid-auto-flow:column; grid-auto-columns:200px` laesst
    /// genau einen der beiden Werte im Feld stehen, und
    /// `grid-auto-rows:100px` steht als String da, den niemand liest —
    /// die Zeile behalt den Inhalt (GEMESSEN 20 px statt 100 px).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `grid-auto-flow` BEHALTEN bleibt,
    /// obwohl es auch hier herausfaellt: es ist als Boolean ausgewertet
    /// (`gitter_fluss_spalten`), also ueberlebt die Vermischung. Es
    /// braucht nur noch kein eigenes Feld, weil es eine FRAGE
    /// beantwortet („welche Achse?") statt einer Zahl.
    RasterAutoZeilen,
    /// `grid-auto-columns` (CSS Grid 1 §7.3). Siehe [`RasterAutoZeilen`].
    RasterAutoSpalten,
    /// background-image: nur `none` und Verlaeufe (CSS Images 3, §3;
    /// K1, 01.10.2026). `url(...)` bleibt weg — Bilder holt eine
    /// andere Rolle. NICHT vererbt (CSS Backgrounds 3, §2.1).
    HintergrundBild,
    /// Die vier Ecken von `border-radius` einzeln (CSS Backgrounds 3
    /// §5.1; K6, 01.10.2026).
    ///
    /// Ohne sie fallen auf github die haeufigsten Deklarationen weg
    /// (GEMESSEN: 265x `border-radius` + 261x fuer die vier Ecken).
    ///
    /// `border-radius` selbst hat KEINE eigene Variante: es ist eine
    /// Kurzform, die auf diese vier expandiert (§5.4). Grund ist
    /// GEMESSEN: mit einer eigenen Variante gewannen
    /// `border-radius: 5px` UND `border-top-left-radius: 9px` beide,
    /// weil sie in verschiedenen Feldern stehen — und die Kurzform
    /// setzte sich nicht durch (Ergebnis (1, 5, 5, 5) statt
    /// (9, 5, 5, 5)).
    RadiusObenLinks,
    RadiusObenRechts,
    RadiusUntenLinks,
    RadiusUntenRechts,
    /// opacity (CSS Color 4, §3.2) als Faktor 0-1 (K6).
    Deckkraft,
    /// box-shadow (CSS Backgrounds 3, §9; K6).
    Schatten,
    /// outline-width / `outline` (CSS UI 4, §6; K6).
    OutlineBreite,
    OutlineFarbe,
    /// outline-offset (CSS UI 4, §6.2; K6) — Abstand des Rings vom
    /// Rand. Fuer das Layout ohne Bedeutung, fuer den Rasterizer nicht.
    OutlineOffset,
    /// vertical-align (CSS Inline 3, §6; K6) — wirkt auf den
    /// Zeilenfluss, also nicht nur auf das Bild.
    VertikalAlign,
    /// box-sizing (CSS Box Sizing 3, §2; K7).
    BoxArt,
    /// `width`/`height` als Schluesselwort (`max-content`,
    /// `min-content`, `fit-content`, CSS Sizing 3 §2).
    ///
    /// GEMESSEN 01.10.2026 (K7): `width: max-content` steht dreimal
    /// auf wikipedia_vector und fiel jedes Mal weg.
    BreiteArt,
    /// `mask-image` + die drei Langformen (CSS Masking 1, §4; K7).
    MaskenBild,
    MaskenGroesse,
    MaskenPosition,
    MaskenWiederholen,
    // ---- K7c (01.10.2026): 27 neue, typisierte Eigenschaften --------
    //
    // Eine je Feld. Sie stehen HINTEN, damit die vorhandenen Nummern
    // unveraendert bleiben, und `index_tabelle_ist_eindeutig` prueft
    // die Tabelle mit.
    LetterSpacing,
    Wortabstand,
    TextEinzug,
    Tabulator,
    TextTransform,
    TextUeberlauf,
    WortUmbruch,
    WortTrennung,
    ZeilenTrennung,
    GitterSpalteStart,
    GitterSpalteEnde,
    GitterZeileStart,
    GitterZeileEnde,
    GitterGebiete,
    /// `grid-area` (CSS Grid 1 §8.4).
    ///
    /// L4 (03.10.2026 02:9x). GEMESSENER BEFUND, warum es diese eigene
    /// Eigenschaft gibt: `grid-area` ging auf `ESchluessel`, also in den
    /// Sammelbehaelter der Rohwerte, den niemand liest. GEMESSEN im
    /// Korpus: 184 Vorkommen, davon 169 ein NAME.
    GitterGebiet,
    GitterVorlagen,
    GitterAutomatik,
    Ordnung,
    LueckeSpalte,
    LueckeZeile,
    OverflowX,
    OverflowY,
    ObjektPassung,
    Seitenverhaeltnis,
    TabellenKanten,
    TabellenLage,
    /// Der Sammelbehaelter fuer alles, was gelesen wird und kein
    /// eigenes Feld verdient (siehe `Berechnet::schluessel`).
    Schluessel,
    /// `background-size` / `-position` / `-repeat` (CSS Backgrounds 3
    /// §3.3-§3.5; K7).
    ///
    /// `background-image: url(...)` hat KEINE eigene Variante: es
    /// zeigt auf [`HintergrundBild`], und die Kaskade legt Verlauf
    /// ODER URL in das EINZIGE Feld `Berechnet::bild`. Zwei Felder
    /// wuerden den Zustand „ein Verlauf und ein Bild" erlauben.
    BildGroesse,
    BildPosition,
    BildWiederholen,
    /// `background-blend-mode` (CSS Compositing and Blending Level 1, §3.1).
    ///
    /// GEMESSENER ANLASS (07.10.2026, K7c): die Eigenschaft stand im
    /// Parser als `ESchluessel` — der Wert ging verloren, und die
    /// Ebenen wurden immer mit `normal` gemischt. Der Korpus enthaelt
    /// 1x `background-blend-mode` (35 echte Stylesheets).
    ///
    /// Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
    /// Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
    /// Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
    /// ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
    HintergrundMischung,
    /// `border-spacing` waagerecht und senkrecht (CSS 2.1 §17.6).
    ///
    /// GEMESSENE LUECKE (03.10.2026, Rolle CSS): `border-spacing` stand im
    /// Parser als `ESchluessel`. Es hatte damit **keine** eigene Variante und
    /// **kein** Feld in `Berechnet` — das hiess: `cellspacing="0"` (4x auf den
    /// zehn Referenzseiten, alle `"0"`) hatte nirgends eine Stelle, wo es
    /// ankommen konnte. Zwei Felder, weil die Kurzform zwei Werte annimmt
    /// (waagerecht | senkrecht, §17.6) und `cellspacing` beide Achsen setzt.
    RandAbstandWaagerecht,
    RandAbstandSenkrecht,
}

/// True, wenn die Eigenschaft vererbt wird (C3): color, font-*,
/// line-height, text-align, white-space, visibility. Alles andere
/// startet beim Anfangswert.
pub fn wird_vererbt(eigenschaft: Eigenschaft) -> bool {
    use Eigenschaft::{
        Farbe, Schriftfamilie, Schriftgewicht, Schriftgroesse, Schriftstil, Sichtbarkeit,
        Textausrichtung, Weissraum, Zeilenhoehe,
    };
    matches!(
        eigenschaft,
        Farbe
            | Schriftgroesse
            | Schriftgewicht
            | Schriftstil
            | Schriftfamilie
            | Zeilenhoehe
            | Textausrichtung
            | Weissraum
            | Sichtbarkeit
    )
}

/// Gepruefter Eigenschaftswert. `Wort` traegt ein gueltiges
/// Schluesselwort (kleingeschrieben); die Kaskade loest es zur Zahl auf.
/// `Erben`/`Anfang`/`Unbestimmt` sind inherit/initial/unset.
#[derive(Debug, Clone, PartialEq)]
pub enum Wert {
    /// Farbe (Namen, #hex, rgb(), hsl() — alles aufgeloest).
    Farbe(Farbe),
    /// Laenge mit Einheit (Zahl = einheitenlos, z.B. line-height-Faktor).
    Laenge(Laenge),
    /// Geprueftes Schluesselwort, z.B. "fett", "zentriert", "auto"-frei.
    Wort(String),
    /// `auto` (nur Breiten/Hoehe).
    Auto,
    /// `currentcolor` — wird zur eigenen Farbe des Elements.
    Aktuell,
    /// `inherit` — Wert des Elternelements (oder Anfangswert ohne Eltern).
    Erben,
    /// `initial` — Anfangswert der Eigenschaft.
    Anfang,
    /// `unset` — vererbt ? inherit : initial.
    Unbestimmt,
    /// `revert` — faellt auf UA zurueck.
    Zurueck,
    /// Ein Verlauf (`linear-gradient`, `radial-gradient`, …) als Wert
    /// fuer `background-image` (CSS Images 3, §3).
    Verlauf(Verlauf),
    /// Die Schattenliste aus `box-shadow` (CSS Backgrounds 3, §9;
    /// K6). `none` ist ein leeres Wort, keine Liste.
    Schatten(Vec<Schatten>),
    /// Die Maskenliste aus `mask-image` (CSS Masking 1, §4.1; K7).
    /// `none` ist eine Liste mit einem leeren Eintrag.
    Maske(Vec<Maske>),
    /// Eine URL aus `background-image: url(...)` (CSS Backgrounds 3
    /// §3.1; K7). Das BILD holt eine andere Rolle; hier bleibt die
    /// Zeichenkette, damit die Quelle wenigstens bekannt ist.
    Url(String),
    /// Die Geometrie des Hintergrundbildes (CSS Backgrounds 3
    /// §3.3-§3.5; K7) — `background-size`/`-position`/`-repeat` in
    /// EINEM Wert, weil sie alle dasselbe Bild beschreiben.
    BildGeo(BildGeo),
    /// `box-sizing` (CSS Box Sizing 3, §2; K7).
    BoxArt(BoxArt),
    /// EIN Schluessel-Wert mit dem NAMEN der Eigenschaft (K7c).
    ///
    /// Der Name ist Teil des Wertes, weil ein Behaelter fuer ~190
    /// Eigenschaften sonst nicht unterscheiden koennte, welcher Wert
    /// welcher Angabe gehoert. Bei typisierten Feldern ist das
    /// unnoetig (das Feld ist der Name), bei diesem Behaelter nicht.
    Schluessel(String, String),
    /// `width: max-content` und Geschwister (CSS Sizing 3, §2; K7).
    BreiteArt(String),
    /// Release 15.10.2026 (Agent F): an elliptical corner radius
    /// (`horizontal vertical`, CSS Backgrounds 3 §5.1); equal radii stay a
    /// `Laenge`.
    Radius(Laenge, Laenge),
}

/// Verlaufsart (CSS Images 3, §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerlaufArt {
    /// `linear-gradient` — Farbverlauf entlang einer geraden Linie.
    Linear,
    /// `radial-gradient` — vom Mittelpunkt nach aussen.
    Radial,
    /// `conic-gradient` — um einen Punkt herum (CSS Images 4, §12).
    Kegel,
}

/// Ein Farbstopp: eine Farbe und ihre OPTIONALE Position.
///
/// `anteil` ist `None` bei `red` ohne Angabe (CSS Images 3, §3.5: die
/// Position wird dann zwischen den Nachbarn gleichmaessig verteilt) und
/// `Some(0.0..=1.0)` bei `red 40%` oder `red 20px`. Die Zahl ist hier
/// bewusst noch NICHT in Pixeln — wer sie braucht, rechnet sie gegen
/// die Kastengroesse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Farbstopp {
    /// Die Farbe an dieser Stelle.
    pub farbe: Farbe,
    /// Position entlang der Verlaufslinie, `None` = automatisch.
    pub anteil: Option<f32>,
}

/// Die Groesse einer Maske (`mask-size`, CSS Masking 1, §4.2).
///
/// `cover` und `contain` sind KEINE Laengen, sondern Ratgeber an den
/// Rasterizer („passend fuellen", „vollstaendig hinein") — sie stehen
/// deshalb hier und nicht als `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaskGroesse {
    /// `auto` (Anfangswert): die Bildgroesse.
    Auto,
    /// `cover`: beides proportional skalieren, bis es ueberdeckt.
    Cover,
    /// `contain`: beides proportional skalieren, bis es hineinpasst.
    Contain,
    /// Eine Laenge in px (die zweite Achse folgt der ersten, §4.2).
    Wert(f32),
}

/// EINE Maske (`mask-image` plus ihre Langformen).
///
/// # WARUM EINE EIGENE STRUKTUR
///
/// `mask-image` ist auf wikipedia mit 110x die haeufigste verworfene
/// Angabe (GEMESSEN, examples/abdeckung wikipedia_vector), zusammen mit
/// der `-webkit-`-Variante 220x. Als `Wert::Wort(String)` waere die Zahl
/// gerettet und die Information weg — ein Rasterizer kann mit dem Wort
/// `url("data:image/svg+xml,...")` nichts anfangen.
///
/// Der MASKENBEHELF gehoert zu `Berechnet::masken`, nicht in die
/// Kaskade: er ist keine Eigenschaft, sondern das Ergebnis der drei
/// Langformen aneinander.
#[derive(Debug, Clone, PartialEq)]
pub struct Maske {
    /// Die URL aus `url(...)`, OHNE die Klammern. Leer = `none`.
    pub url: String,
    /// `mask-size` (CSS Masking 1 §4.2).
    pub groesse: MaskGroesse,
    /// `mask-position`: 0 = links/oben, 0,5 = Mitte, 1 = rechts/unten
    /// (CSS Masking 1 §4.1, in Längen umgerechnet).
    pub position_x: f32,
    /// Siehe `position_x`.
    pub position_y: f32,
    /// Der VERLAUF aus `linear-gradient(...)` (CSS Masking 1 §3.2).
    ///
    /// `mask-image` nimmt dieselben Bildquellen wie `background-image`
    /// (CSS Masking 1 §3.2 verweist auf CSS Images 3 §2). Vorher
    /// konnte eine Maske nur eine URL oder `none` tragen, und die
    /// Verlaufsformen wurden verworfen.
    ///
    /// GEMESSENER FEHLER 01.10.2026 (K7c): 4 Verwerfe
    /// „Wert ungeprueft: mask-image" auf den 35 echten Stylesheets,
    /// darunter `linear-gradient(90deg,#0000,#000 20% 80%,#0000)`.
    pub verlauf: Option<Verlauf>,
    /// `mask-repeat` in x (CSS Masking 1 §4.4).
    pub wiederholen_x: bool,
    /// Siehe `wiederholen_x`.
    pub wiederholen_y: bool,
}

/// DER ART DER BILDQUELLE (`background-image`, CSS Backgrounds 3 §3.1).
///
/// `Verlauf` und `Url` schliessen einander aus; `Keins` ist `none` und
/// zugleich der Anfangswert (deshalb steht es nicht in `erbe`).
#[derive(Debug, Clone, PartialEq)]
pub enum BildQuelle {
    /// Kein Bild (`none`).
    Keins,
    /// Ein Verlauf (`linear-gradient` usw., K1).
    Verlauf(Verlauf),
    /// Eine URL. Das BILD holt eine andere Rolle — hier bleibt die
    /// Zeichenkette, damit wenigstens die Quelle bekannt ist.
    Url(String),
}

/// Die Geometrie des Hintergrundbildes (CSS Backgrounds 3 §3.3-§3.5).
///
/// # WARUM EINE EIGENE STRUKTUR
///
/// `background-size`, `background-position` und `background-repeat`
/// beschreiben ALLE dasselbe Bild. In drei getrennten Feldern
/// entstuende der Zustand „Groesse aus der einen, Position aus der
/// anderen, Repeat aus der dritten Regel" — und der Rasterizer
/// muesste raten, welche Seite gewinnt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BildGeo {
    /// `background-size`: `cover`, `contain` oder eine Laenge in px.
    pub groesse: f32,
    /// `cover` heisst: skaliere, bis es ueberdeckt. Gehört zu
    /// `groesse_pfad` weiter unten.
    pub cover: bool,
    /// `contain` heisst: skaliere, bis es hineinpasst.
    pub contain: bool,
    /// `background-position` in x, 0..1.
    pub position_x: f32,
    /// `background-position` in y, 0..1.
    pub position_y: f32,
    /// `true`, wenn `position_x`/`position_y` PIXEL sind und nicht
    /// Anteile (CSS Backgrounds 3 §3.3).
    ///
    /// CSS Backgrounds 3 §3.3 erlaubt beides: `<length>` und
    /// `<percentage>`. Ein Anteil ist direkt 0..1, eine Laenge braucht
    /// die Groesse des Bildbereichs — die hat der PARSER nicht, aber
    /// der Rasterizer gleich. Deshalb wird sie hier als px
    /// durchgereicht statt als 0 gelesen und verworfen.
    ///
    /// GEMESSENE GRUND, 01.10.2026 (K7c): 13 Verwerfe
    /// „Wert ungeprueft: background-position" auf den 35 echten
    /// Stylesheets, davon 3x `right 8px center` (EIN px-Wert neben
    /// Schluesselwortern — die haeufigste Mischform).
    pub position_px: bool,
    /// Release 11.10.2026 (Agent CSS2): Pixel JE ACHSE (x, y) — `8px bottom`
    /// ist x in px, y ein Anteil. `None`: `position_px` gilt fuer beide
    /// (die alte Form). GEMESSEN vorher (WPT `bottom-applies-to-*`-Referenz,
    /// `background: url(...) no-repeat 8px bottom`): x wurde als Anteil 8.0
    /// (800 %) gelesen, das Bild lag ausserhalb.
    pub position_px_achsen: Option<(bool, bool)>,
    /// Release 11.10.2026 (Agent CSS2): die Pixelzahl je Achse ist in `em`
    /// angegeben — die Kaskade rechnet sie mit der Schriftgroesse um
    /// (GEMESSEN vorher: WPT `min-height-111`, `0 1em` wurde 1 px).
    pub position_em: (bool, bool),
    /// Release 11.10.2026 (Agent CSS2): dasselbe in `ex` (WPT
    /// `background-position-001`: `6.25ex` bei Ahem).
    pub position_ex: (bool, bool),
    /// `background-repeat` in x.
    pub wiederholen_x: bool,
    /// `background-repeat` in y.
    pub wiederholen_y: bool,
    /// Release 13.10.2026 (Agent CSS2): `background-size` mit BEIDEN Werten
    /// (x, y); `None` ist `auto`. Nach der Kaskade nur noch `px` oder `%`.
    /// `groesse` bleibt fuer den Bildweg, der nur die Breite kennt.
    pub groesse_xy: [Option<Laenge>; 2],
    /// Release 13.10.2026 (Agent CSS2): die Rahmenbreiten (oben, rechts,
    /// unten, links) des Elements in px — die Lageflaeche eines
    /// Hintergrundbildes ist die Innenkante des Rahmens (`background-origin:
    /// padding-box`, CSS Backgrounds 3 §3.7), gemalt wird bis zur
    /// Aussenkante. Traegt der Seitenbau ein.
    pub rand_px: [f32; 4],
    /// Release 13.10.2026 (Agent CSS2): `background-repeat: space` (1) oder
    /// `round` (2) je Achse (x, y); 0 sonst (CSS Backgrounds 3 §3.4).
    pub wiederhol_art: [u8; 2],
    /// Release 13.10.2026 (Agent CSS2): `background-origin` — 0 `padding-box`
    /// (Anfangswert), 1 `border-box`, 2 `content-box` (§3.7).
    pub ursprung: u8,
    /// Release 13.10.2026 (Agent CSS2): `background-clip` fuer das Bild — 0
    /// `border-box` (Anfangswert), 1 `padding-box`, 2 `content-box` (§3.5).
    pub bild_clip: u8,
    /// Release 13.10.2026 (Agent CSS2): die Innenabstaende (oben, rechts,
    /// unten, links) in px, fuer `content-box`. Traegt der Seitenbau ein.
    pub innen_px: [f32; 4],
    /// Release 13.10.2026 (Agent CSS2): die Pixelposition der Achse zaehlt
    /// von der ENDkante (`right 10px`, `bottom 2em`; CSS Backgrounds 3 §3.6).
    pub vom_ende: (bool, bool),
    /// Release 14.10.2026 (Agent CSS2): the image is a `border-image`
    /// (CSS Backgrounds 3 §6), not a background. Filled in by the page builder.
    pub border_image: Option<BorderImage>,
    /// Release 14.10.2026 (Agent CSS2): the viewport (width, height) for
    /// `background-attachment: fixed` (`ursprung == 3`), filled in by the page
    /// builder; `None` if unknown.
    pub viewport: Option<(f32, f32)>,
    /// Release 14.10.2026 (Agent CSS2): the image of the root element paints
    /// the whole canvas (CSS Backgrounds 3 §2.11.1); positioned against the
    /// root box, painted over the viewport `ansicht`.
    pub paints_canvas: bool,
    /// Release 14.10.2026 (Agent CSS2): the body's image propagated to the
    /// canvas is positioned against the root element (CSS Backgrounds 3
    /// §2.11.2), approximated as the viewport width and the document height.
    pub from_body: bool,
}

/// Release 14.10.2026 (Agent CSS2): one width from `border-image-width`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderImageWidth {
    /// `auto`: the slice size.
    Auto,
    /// A length in px.
    Px(f32),
    /// A number: multiple of the border width.
    Factor(f32),
    /// Percentage of the border image area.
    Percent(f32),
}

/// Release 14.10.2026 (Agent CSS2): `border-image-slice`, `-width`, `-outset`
/// and `-repeat` (CSS Backgrounds 3 §6.2-§6.5), per edge top, right, bottom,
/// left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderImage {
    /// (value, is percentage?) — a plain number means image pixels.
    pub slice: [(f32, bool); 4],
    /// `fill`: the middle part is drawn.
    pub fill: bool,
    pub width: [BorderImageWidth; 4],
    /// (value, is px?) — otherwise a multiple of the border width.
    pub outset: [(f32, bool); 4],
    /// 0 `stretch`, 1 `repeat`, 2 `round`, 3 `space` per axis (x, y).
    pub repeat: [u8; 2],
}

impl BildGeo {
    /// Release 11.10.2026 (Agent CSS2): sind x bzw. y Pixel (und keine
    /// Anteile)? Siehe `position_px_achsen`.
    pub fn px_achsen(&self) -> (bool, bool) {
        self.position_px_achsen.unwrap_or((self.position_px, self.position_px))
    }
}

impl Default for BildGeo {
    fn default() -> Self {
        BildGeo {
            groesse: 0.0,
            cover: false,
            contain: false,
            position_x: 0.0,
            position_y: 0.0,
            position_px: false,
            position_px_achsen: None,
            position_em: (false, false),
            position_ex: (false, false),
            wiederholen_x: true,
            wiederholen_y: true,
            groesse_xy: [None, None],
            rand_px: [0.0; 4],
            wiederhol_art: [0, 0],
            ursprung: 0,
            bild_clip: 0,
            innen_px: [0.0; 4],
            vom_ende: (false, false),
            border_image: None,
            viewport: None,
            paints_canvas: false,
            from_body: false,
        }
    }
}

/// Wie `width`/`height` zu rechnen sind (CSS Box Sizing 3, §2).
///
/// # WARUM DAS EINE EIGENE EIGENSCHAFT IST
///
/// Bei `content-box` (der Anfangswert) ist die angegebene Breite der
/// INHALT, und Rahmen und Fuellung kommen DARAUF. Bei `border-box` ist
/// sie die AUSSENkante. Der Unterschied ist bei einem Kasten mit
/// `padding: 10px` und `width: 100px` genau 20 Pixel — und der
/// Wikipedia-Baum enthaelt vier Stellen mit `box-sizing: border-box`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxArt {
    /// `content-box` (Anfangswert, CSS Box Sizing 3 §2.1).
    Inhalt,
    /// `border-box` (§2.2).
    Rand,
}

/// DER MISCHMODUS fuer Hintergrund-Ebenen (CSS Compositing and Blending
/// Level 1, §3.1).
///
/// GEMESSENER ANLASS (07.10.2026, K7c): `background-blend-mode` stand
/// im Parser als `ESchluessel` — der Wert ging verloren, und die
/// Ebenen wurden immer mit `normal` gemischt. Der Korpus enthaelt 1x
/// `background-blend-mode` (35 echte Stylesheets).
///
/// Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
/// Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
/// Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
/// ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MischModus {
    /// `normal` (Anfangswert, CSS Compositing 1 §3.1).
    Normal,
    /// `multiply` — dunkler, wie Farbe auf Farbe.
    Multiplizieren,
    /// `screen` — heller, wie Licht ueber Licht.
    Aufhellen,
    /// `overlay` — multiply fuer dunkle, screen fuer helle.
    Ueberlagern,
    /// `darken` — der dunklere Kanal gewinnt.
    Dunkeln,
    /// `lighten` — der hellere Kanal gewinnt.
    Erhellen,
    /// `color-dodge` — das Bild wird aufgehellt.
    FarbeAusweichen,
    /// `color-burn` — das Bild wird verdunkelt.
    FarbeBrennen,
    /// `hard-light` — wie overlay, aber mit vertauschten Rollen.
    HartesLicht,
    /// `soft-light` — sanfteres overlay.
    WeichesLicht,
    /// `difference` — absolute Differenz.
    Differenz,
    /// `exclusion` — wie difference, aber weicher.
    Ausschliessen,
    /// `hue` — Farbton des Vordergrunds, Sättigung/Helligkeit des Hintergrunds.
    Farbton,
    /// `saturation` — Sättigung des Vordergrunds.
    Saettigung,
    /// `color` — Farbe des Vordergrunds, Helligkeit des Hintergrunds.
    Farbe,
    /// `luminosity` — Helligkeit des Vordergrunds.
    Helligkeit,
}

impl MischModus {
    /// Parst den Wert aus `background-blend-mode` (CSS Compositing 1 §3.1).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum unbekannte Werte `None` geben und
    /// nicht `Normal`: `normal` ist der Anfangswert, und ein Tippfehler
    /// wie `multiplyy` soll NICHT stillschweigend zu `normal` fuehren —
    /// das waere eine falsche Aussage ueber den geparsten Zustand.
    pub fn aus_wort(w: &str) -> Option<MischModus> {
        Some(match w {
            "normal" => MischModus::Normal,
            "multiply" => MischModus::Multiplizieren,
            "screen" => MischModus::Aufhellen,
            "overlay" => MischModus::Ueberlagern,
            "darken" => MischModus::Dunkeln,
            "lighten" => MischModus::Erhellen,
            "color-dodge" => MischModus::FarbeAusweichen,
            "color-burn" => MischModus::FarbeBrennen,
            "hard-light" => MischModus::HartesLicht,
            "soft-light" => MischModus::WeichesLicht,
            "difference" => MischModus::Differenz,
            "exclusion" => MischModus::Ausschliessen,
            "hue" => MischModus::Farbton,
            "saturation" => MischModus::Saettigung,
            "color" => MischModus::Farbe,
            "luminosity" => MischModus::Helligkeit,
            _ => return None,
        })
    }
}

/// EIN Schatten (`box-shadow`, CSS Backgrounds 3, §9).
///
/// # WARUM EINE EIGENE STRUKTUR
///
/// `box-shadow` ist auf github 152x die haeufigste verworfene
/// Angabe (GEMESSEN, examples/abdeckung). Sie als
/// `Wert::Wort(String)` aufzubewahren wuerde die Zahl behalten, aber
/// die Farbe nicht — und die Farbe ist das Einzige, was der
/// Zeichner braucht.
///
/// Die Laenge bleibt `None` bei `none` (kein Schatten).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Schatten {
    /// Die Farbe des Schattens.
    pub farbe: Farbe,
    /// Versatz in x, in px (CSS Backgrounds 3 §9.1: zwei Laengen).
    pub x: f32,
    /// Versatz in y, in px.
    pub y: f32,
    /// Weichzeichnen (`blur`), in px. 0 = kein.
    pub weich: f32,
    /// Ausbreitung (`spread`), in px. 0 = keiner.
    pub ausbreitung: f32,
    /// `inset` (Schatten nach innen) — CSS Backgrounds 3 §9.2.
    pub innen: bool,
    /// Release 14.10.2026 (Agent CSS2): the four lengths with their units as
    /// read (x, y, blur, spread); the cascade turns them into px against the
    /// element's font (`em`, `rem`, viewport units).
    pub raw_lengths: [Laenge; 4],
    /// Release 14.10.2026 (Agent CSS2): no colour given or `currentColor`:
    /// the element's `color` (CSS Backgrounds 3 §7.1).
    pub current_color: bool,
}

/// Ein Verlauf als WERT (CSS Images 3, §3).
///
/// # WARUM ALS WERT UND NICHT ALS STRING
///
/// Vorher fiel `background-image: linear-gradient(...)` ersatzlos weg —
/// die ganze Deklaration wurde beim Lesen verworfen, und eine Seite,
/// deren Hintergrund ein Verlauf ist, hatte an dieser Stelle einfach
/// nichts. Als String hiesse der Weg weiter "irgendwo im Rohtext".
///
/// Was dieser Browser NICHT kann: den Verlauf MALEN. Dafuer braucht es
/// die Rasterung der Rolle SCHRIFT/LAYOUT. Was er kann: die Stopps
/// mit ihren Positionen und Farben zu BENENNEN, damit der Aufrufer sie
/// bekommt und entscheiden kann. Eine falsche Umrechnung waere hier
/// schlimmer als eine ehrliche Grenze.
#[derive(Debug, Clone, PartialEq)]
pub struct Verlauf {
    /// Linear, radial oder Kegel.
    pub art: VerlaufArt,
    /// Richtungswinkel in Grad. CSS Images 3, §3.1: `0deg` zeigt nach
    /// OBEN, positive Werte im Uhrzeigersinn, `linear-gradient` ohne
    /// Angabe ist `180deg` (nach unten).
    pub winkel: f32,
    /// Die Farbstopps in Schreibrichtung, mindestens zwei.
    pub stopps: Vec<Farbstopp>,
    /// `repeating-linear-gradient` & Co.
    pub wiederholt: bool,
}

impl Verlauf {
    /// `linear-gradient(180deg, …)` — die Voreinstellung der Kurzform
    /// (CSS Images 3, §3.1: ohne Richtungsangabe geht der Verlauf von
    /// oben nach unten).
    pub fn linear(stopps: Vec<Farbstopp>) -> Verlauf {
        Verlauf {
            art: VerlaufArt::Linear,
            winkel: 180.0,
            stopps,
            wiederholt: false,
        }
    }
}

/// RGBA-Farbe, je Kanal 0-255.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Farbe {
    /// Rot 0-255.
    pub r: u8,
    /// Gruen 0-255.
    pub g: u8,
    /// Blau 0-255.
    pub b: u8,
    /// Deckkraft 0 (durchsichtig) bis 255 (deckend).
    pub a: u8,
}

impl Farbe {
    /// Schwarz, deckend (Anfangswert von color).
    pub const SCHWARZ: Farbe = Farbe {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Weiss, deckend.
    pub const WEISS: Farbe = Farbe {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    /// Durchsichtig (Anfangswert von background-color).
    pub const DURCHSICHTIG: Farbe = Farbe {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    /// Linkblau des UA-Stylesheets.
    pub const BLAU: Farbe = Farbe {
        r: 0,
        g: 0,
        b: 238,
        a: 255,
    };
    /// Rot (fuer Tests).
    pub const ROT: Farbe = Farbe {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
}

/// Laenge mit Einheit (C3): px, em, rem, %, pt, vw/vh.
/// `Zahl` ist ein einheitenloser Faktor (line-height, font-weight).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Laenge {
    /// Der Zahlenwert (endlich, sonst waere der Wert verworfen).
    pub wert: f32,
    /// Die Einheit.
    pub einheit: Einheit,
}

/// Laengeneinheit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Einheit {
    /// Absolute Pixel.
    Px,
    /// Relativ zur (Eltern-)Schriftgroesse.
    Em,
    /// Relativ zu 16 px (Wurzel).
    Rem,
    /// Prozent (Basis je Eigenschaft, siehe Kaskade).
    Prozent,
    /// Punkt (1 pt = 4/3 px).
    Pt,
    /// 1 % der Ansichtsbreite.
    Vw,
    /// `dvh`, `dvw`, `dvmin`, `dvmax` (CSS Values 4, §6.1.2): der
    /// Viewport, in dem die dynamischen Browserleisten eingerechnet sind.
    /// Ohne deren Groesse sind das fuer diesen Browser die GROSSEN
    /// (dokumentierte Naeherung, siehe Kaskade).
    Dvh,
    Dvw,
    Dvmin,
    Dvmax,
    /// `lh` (CSS Values 4 §6.1.2): die Zeilenhoehe der Box.
    ///
    /// Die Kaskade kennt die Zeile nicht (das ist der Textfluss),
    /// also wird sie als `em` gerechnet — eine DOKUMENTIERTE
    /// Naeherung, siehe der Arm in `laenge_px`.
    Lh,
    /// `rlh` (CSS Values 4 §6.1.2): die Zeilenhoehe der Wurzel.
    /// Ohne den Textfluss ist sie nicht zu unterscheiden — auch hier
    /// `em`.
    Rlh,
    /// `lvh`, `lvw`, `lvmin`, `lvmax`: der Viewport mit den KLEINEN
    /// Leisten. Ohne deren Groesse das kleine Mass.
    Lvh,
    Lvw,
    Lvmin,
    Lvmax,
    /// Wie Vw (die Kaskade kennt keine Hoehe — dokumentierte Naeherung).
    Vh,
    /// Kleinere Ansichtsachse.
    Vmin,
    /// Groessere Ansichtsachse.
    Vmax,
    /// Zeichenbreite 0 (Naeherung 0.5 em).
    Ch,
    /// Einheitenlose Zahl (Faktor oder Gewicht).
    Zahl,
    /// Release 11.10.2026 (Agent CSS2): `ex`, die x-Hoehe der ersten
    /// Schrift (CSS 2.1 §4.3.2). Die Kaskade rechnet sie mit dem Anteil der
    /// Schrift um (Ahem 0.8 em, sonst die Naeherung 0.5 em).
    Ex,
}

// ===========================================================================
// BERECHNET — das Ergebnis je Element
// ===========================================================================

/// Anzeigetyp (display). `Keins` = nicht darstellen (und Kinder auch
/// nicht — die Kaskade liefert trotzdem Werte, das Ausblenden
/// entscheidet der Aufrufer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anzeige {
    /// Block (eigene Zeile(n)).
    Block,
    /// Inline (im Textfluss).
    Inline,
    /// Inline-Block (fuer Formulare/Bilder spaeter).
    InlineBlock,
    /// Listeneintrag (ul/ol-Punkt).
    ListenEintrag,
    /// Flex-Container (C8, W5): Kinder legt `lege_flex` (src/flex.rs).
    Flex,
    /// `inline-flex` (CSS Display 3, §2.5) — Flex-Container, der
    /// selbst im Textfluss steht (CSS Flexbox 1 §2: er ist wie
    /// `inline-block` aufgebaut, seine Kinder sind Flex-Items).
    ///
    /// GEMESSEN 01.10.2026 (K7): wikipedia_vector-2022.css hat vier
    /// `display: inline-flex`. Vor diesem Feld wurden sie gelesen und
    /// dann VERWORFEN — und ohne sie stellt sich die Frage „Block
    /// oder Flex?" nicht, sondern die Frage gar nicht.
    InlineFlex,
    /// `table` (CSS Tables 3, §2.1).
    ///
    /// added 05.10.2026, Rolle SERVER (SERVER_13): Der Tabellen-Leg ist
    /// **da** — er wurde am selben Tag gebaut, an dem diese Zeile als
    /// „fehlt noch“ galt. Sie trug die Umkehrung: `layout_plan.rs` hat
    /// daraus `Block` gemacht, weil **die Plan-Seite** keine Variante
    /// hatte; die Kaskade hat den Wert also richtig gelesen und richtig
    /// weitergegeben, und er ging an der naechsten Stelle verloren.
    ///
    /// Die Rolle steht jetzt auch in `narsil_layout::plan::Anzeige`, und
    /// `flow.rs` ruft fuer diese Variante `lege_tabelle`.
    Tabelle,
    /// `table-row`, `table-row-group`, `table-header-group`,
    /// `table-footer-group` (CSS 2.1, §17.2).
    ///
    /// added 05.10.2026: **eine Rolle, nicht drei.** `thead` und `tfoot`
    /// erzeugen dieselben Boxen wie `tbody`; sie stehen nur oben bzw.
    /// unten. Wer daraus spaeter etwas macht, muss es messen.
    TabelleZeile,
    /// `table-cell` (CSS 2.1, §17.2.1).
    TabelleZelle,
    /// `table-caption`, `table-column`, `table-column-group`.
    ///
    /// added 05.10.2026: **bewusst zusammengefasst.** Die Caption ist ein
    /// Block ueber oder unter der Tabelle, und die Spaltenbeschreibung
    /// erzeugt nach CSS 2.1, §17.2 **gar keine** Boxen. Der Raster-Leg
    /// braucht sie also nicht.
    TabelleRand,
    /// Release 11.10.2026: `table-column`, `table-column-group` — eigene
    /// Rolle, weil sie (anders als die Caption) KEINEN Inhalt zeigen (CSS 2.1
    /// §17.2); der Seitenbau baut sie und ihre Nachkommen nicht. GEMESSEN
    /// vorher (WPT `modal-dialog-in-table-column`): ein Dialog darin erschien.
    TabelleSpalte,
    /// Raster-Container (C15, W6): Kinder legt `lege_raster` (src/raster.rs).
    Raster,
    /// Nicht darstellen.
    Keins,
}

/// Schriftstil (font-style). oblique faellt auf Kursiv zurueck
/// (die Bitmapschrift spaeter kennt nur zwei Stile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schriftstil {
    /// Normal (Anfangswert).
    Normal,
    /// Kursiv (italic/oblique).
    Kursiv,
}

/// Schreibrichtung (CSS Writing Modes 3 §2.4, `direction`).
///
/// K17 (02.10.2026). `direction` ist eine **geerbte** Eigenschaft, und
/// sie entscheidet, welche physische Seite das „Ende" ist: bei `rtl`
/// ist `inline-start` rechts.
///
/// GEMESSENER ANLASS: `direction: rtl` kommt auf den 35 echten
/// Stylesheets NULL Mal vor. Die Nutzlast sind die logischen
/// Eigenschaften — 328 Deklarationen, alle im `Schluessel`-Eimer und
/// alle unaufgeloest (Details in `team/BERICHT_CSS_K17.txt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchreibRichtung {
    /// `ltr` — links nach rechts. Anfangswert.
    Ltr,
    /// `rtl` — rechts nach links.
    Rtl,
}

/// Computed line-height retains unitless values for inheritance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZeilenhoeheArt {
    Normal,
    Faktor(f32),
    Absolut,
}

/// Textausrichtung.
///
/// K17 (02.10.2026): `start` und `end` sind **keine** festen Werte
/// mehr, sie haengen an der Schreibrichtung. Vorher fielen sie
/// stur auf links zurueck — das ist der Grund, warum
/// `text-align: start` auf einer arabischen Seite linksbuendig stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Textausrichtung {
    /// Links (Anfangswert, und `start` bei `ltr`).
    Links,
    /// Rechts (`end` bei `ltr`, `start` bei `rtl`).
    Rechts,
    /// Zentriert.
    Zentriert,
    /// Blocksatz.
    Blocksatz,
}

/// `unicode-bidi` (CSS Writing Modes 3 §6.3, K17).
///
/// `embed=4, isolate=2, -webkit-isolate=1, -moz-isolate=1` — GEMESSEN so
/// auf den 35 Korpus-Dateien. Die beiden Praefix-Werte sind laut
/// Spezifikation Legacy-Aliase fuer `isolate`, keinproprietaer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bidi {
    /// `normal` — Anfangswert.
    Normal,
    /// `embed`.
    Embed,
    /// `isolate`.
    Isolate,
    /// `isolate-override`.
    IsolateOverride,
    /// `plaintext`.
    Plaintext,
    /// `bidi-override`.
    BidiOverride,
}

impl SchreibRichtung {
    /// CSS Writing Modes 3 §2.4: `inline-start` ist die Seite, an der
    /// der Text BEGINNT — bei `rtl` also RECHTS.
    ///
    /// Diese Eine Funktion ist der ganze Unterschied: alle logischen
    /// Eigenschaften fragen nur sie.
    #[inline]
    pub fn start_ist_rechts(self) -> bool {
        matches!(self, SchreibRichtung::Rtl)
    }
}

/// Weissraum-Behandlung (white-space).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weissraum {
    /// Umbruch nach Bedarf, Folgen schrumpfen (Anfangswert).
    Normal,
    /// Wie gesetzt lassen (pre).
    Pre,
    /// Nie umbrechen.
    KeinUmbruch,
    /// Wie gesetzt lassen, zusaetzlich umbrechen.
    PreUmbruch,
    /// Folgen schrumpfen, umbrchig bei Zeilenende.
    PreZeile,
    /// `break-spaces` (CSS Text 3, §3.1) — wie `pre-wrap`, aber jede
    /// Folge erhaelt einen Platz fuer Umbruch.
    UmbruchLeer,
}

/// Rahmenstil (border-*-style). Nur gemerkt (C4) — gezeichnet wird
/// spaeter; `Kein` und `Versteckt` bedeuten "kein Rahmen".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rahmenstil {
    /// Kein Rahmen (Anfangswert).
    Kein,
    /// Durchgehend.
    Durchgehend,
    /// Gestrichelt / gepunktet / doppelt (nur gemerkt).
    ///
    /// **BILD_37: ABGELOEST.** Bis hierher hat **ein einziger** Wert alle sieben
    /// Musterstile gefangen (siehe `parse.rs::rahmenstil_wert`). Die
    /// GEMESSENE Begruendung damals war: „der Rasterizer hat keine gestrichelte
    /// Kante". **Seit BILD_34 hat er eine** (`narsil-raster/src/rahmen.rs`), und
    /// seit BILD_37 liegen **Messwerte** vor. **Dieser Wert bleibt erhalten**,
    /// damit fremde Treffer nicht brechen, aber der Parser liefert ab jetzt
    /// **je Stil seinen eigenen Wert**.
    Gemustert,
    /// `dashed`. **GEMESSEN** an `team/testseiten/deko_ff/24.png`.
    Gestrichelt,
    /// `dotted`. **GEMESSEN** an `deko_ff/25.png`.
    Gepunktet,
    /// `double`. **GEMESSEN** an `deko_ff/26.png`.
    Doppelt,
    /// `groove`. **GEMESSEN** an `deko_ff/27.png`.
    Furche,
    /// `ridge`. GEMESSEN als `furche` mit vertauschten Haelften.
    Wulst,
    /// `inset`.
    Innen,
    /// `outset`.
    Aussen,
    /// Versteckt (wie Kein, aber dominiert in Tabellen).
    Versteckt,
}

/// Aufzaehlungszeichen (list-style-type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liste {
    /// Scheibe (Anfangswert, ul).
    Scheibe,
    /// Kreis / Quadrat.
    Kreis,
    Quadrat,
    /// Nummeriert (ol).
    Dezimal,
    /// Roemisch / alphabetisch (klein: `lower-roman`, `lower-alpha`).
    Roemisch,
    Alpha,
    /// `upper-roman` / `upper-alpha` (Release 07.10.2026: vorher mit der
    /// kleinen Form zusammengelegt, die Marke braucht den Unterschied).
    RoemischGross,
    AlphaGross,
    /// Kein Zeichen.
    Keins,
    /// Ein Skript-Ziffernsystem (`persian`, `arabic-indic`, …).
    ///
    /// GEMESSENE ENTSCHEIDUNG (K19, 01.10.2026 21:35): Diese Stile
    /// sind in CSS Counter Styles 3 §5 definiert und kommen auf
    /// `wikipedia_vector-2022.css` **8mal** vor — alle in Regeln mit
    /// `:lang()`. Vorher wurden die Deklarationen verworfen.
    ///
    /// Warum eine EIGENE Variante und nicht `Dezimal`: `Dezimal` heisst
    /// lateinische Ziffern, und die falschen Ziffern hinzusetzen waere
    /// geraten. `Skript` sagt „Ziffern in einem anderen System" und
    /// laesst die Entscheidung dort, wo die Ziffern gezeichnet werden —
    /// `narsil-text`. Das ist eine fremde Crate, und diese Zeile ist die
    /// Uebergabestelle: der Wert kommt an, die Form fehlt noch.
    Skript,
}

/// Flex-Hauptachse (flex-direction, Teilmenge C8: kein reverse, kein wrap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexRichtung {
    /// Reihe, links nach rechts (Anfangswert).
    Reihe,
    /// Spalte, oben nach unten.
    Spalte,
    /// `row-reverse` (CSS Flexbox 1 §5.1).
    ///
    /// GEMESSEN 01.10.2026 (K7c): 11x auf den echten Stylesheets —
    /// die haeufigste `flex-direction` nach `row`.
    ReiheUmgekehrt,
    /// `column-reverse` (§5.1). 7x.
    SpalteUmgekehrt,
}

/// Hauptachsen-Buendigkeit (justify-content, Teilmenge C8+C12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlexBuendig {
    /// Am Anfang (Anfangswert).
    Start,
    /// Zentriert.
    Mitte,
    /// Am Ende.
    Ende,
    /// Gleichmaessig verteilt (space-between).
    Zwischen,
    /// Raum ringsum (space-around, C12, W6).
    Ringsum,
    /// Gleicher Raum ueberall (space-evenly, C12, W6).
    Gleichmaessig,
    /// `stretch` (CSS Box Alignment 3 §8.3): Fuellung des Raums.
    Strecke,
    /// `left`/`right` (CSS Box Alignment 3 §8.2): physisch, im
    /// Gegensatz zu `start`/`end`.
    ///
    /// GEMESSEN 01.10.2026 (K7c): je 2x auf den echten Stylesheets.
    Links,
    Rechts,
}

/// Querachsen-Ausrichtung (align-items, align-self, justify-self; §6.6).
///
/// L4 (03.10.2026 09:5x): `Default` ist `Strecke` — §6.6 nennt `stretch`
/// als Anfangswert, und `RasterKind` leitet `Default` ab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlexAusrichtung {
    /// Am Anfang.
    Start,
    /// Zentriert.
    Mitte,
    /// Am Ende.
    Ende,
    /// Auf Containerhoehe gestreckt (Anfangswert).
    ///
    /// L4 (03.10.2026 09:5x): auch der `Default` — `RasterKind` leitet
    /// `Default` ab, und §6.6 sagt, der Anfangswert sei `stretch`. Damit
    /// heisst `RasterKind::default()` „gestreckt", also das Verhalten von
    /// heute, und nicht „unbekannt".
    #[default]
    Strecke,
    /// `baseline` (CSS Box Alignment 3 §6.3): die Grundlinie der
    /// Kinder. Der Anfangswert fuer `align-items`, nicht fuer
    /// `align-content`.
    ///
    /// GEMESSEN 01.10.2026 (K7c): 12x auf den 35 echten Stylesheets.
    Grundlinie,
}

/// Ueberlauf (overflow). Nur gemerkt (C4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ueberlauf {
    /// Sichtbar (Anfangswert).
    Sichtbar,
    /// Versteckt.
    Versteckt,
    /// Mit Rollbalken (spaeter).
    Scroll,
    /// Automatisch (spaeter).
    Auto,
}

/// Positionierung (position, C13, W6). Klebend (sticky) verhaelt sich
/// wie Relativ (dokumentierte Naeherung — der Aufrufer kennt kein Scrollen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// Normalfluss (Anfangswert).
    Statisch,
    /// Verschoben gegen die Normalposition (top/left gewinnen).
    Relativ,
    /// Gegen den enthaltenden Block (top/right/bottom/left).
    Absolut,
    /// Gegen die Ansicht (top/right/bottom/left).
    Fest,
    /// Wie Relativ (siehe oben).
    Klebend,
}

/// Float-Seite (float, C14, W6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Float {
    /// Kein Float (Anfangswert).
    Keins,
    /// Links schwimmend.
    Links,
    /// Rechts schwimmend.
    Rechts,
}

/// Raeumung (clear, C14, W6): unter welke Floats das Element faellt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raeumung {
    /// Keine (Anfangswert).
    Keins,
    /// Unter linke Floats.
    Links,
    /// Unter rechte Floats.
    Rechts,
    /// Unter beide.
    Beide,
}

/// Raster-Spaltenmass (grid-template-columns, C15, W6).
/// Ein Gebietsname aus `grid-template-areas` (§8.1) — das Spiegelbild von
/// `narsil_layout::plan::Gebiet`.
///
/// L4 (03.10.2026 02:6x). GEMESSENE ENTSCHEIDUNG, warum es zwei Typen
/// gibt und nicht einen: narsil-layout darf nicht von narsil-css abhaengen
/// (der Weg laeuft andersrum), und die Bruecke kostet sechs Zeilen. Das
/// ist dasselbe Muster wie `SpaltenMass`/`Spalte`.
///
/// GEMESSENE ENTSCHEIDUNG, warum ein fester Bytering und kein `String`:
/// `RasterKind` ist `Copy` und traegt den Namen des KINDES; ein `String`
/// darin waere eine Allokation je Kind. GEMESSEN im Korpus: 41
/// verschiedene Namen, der laengste `trailingComponent` (17 Zeichen).
///
/// GEMESSENE ENTSCHEIDUNG, warum ein zu langer Name LEER bleibt und nicht
/// abgeschnitten wird: `footer` und `footerBar` sind verschiedene Namen,
/// und ein Abschneiten macht beide gleich — dann legt ein Kind an der
/// Stelle des anderen. Und warum der Rest des Rings beim Vergleichen
/// geprueft wird: `footerBar` beginnt wie `footer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gebiet(pub [u8; 32]);

impl Gebiet {
    /// Der leere Name — `auto`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `LEER` und `Default` dasselbe sind:
    /// ein Null-Array ist ein leerer Name. Der `Default`-Ableitung wird es
    /// gebraucht, weil `RasterKind` sie ableitet (alle ihre Felder sollen
    /// beim `..RasterKind::default()` 0 sein) — GEMESSEN am 03.10.
    /// 02:6x: „the trait bound `Gebiet: Default` is not satisfied".
    pub const LEER: Gebiet = Gebiet([0; 32]);

    /// Setzt den Namen aus `s`; passt er nicht, bleibt der Name leer.
    ///
    /// §8.1: „ident-strings" — Buchstaben, Ziffern und `-`/`_`
    /// (GEMESSEN im Korpus: `trailing-action`, `leading-action`).
    pub fn set(&mut self, s: &str) {
        *self = Self::LEER;
        let bytes = s.as_bytes();
        if bytes.is_empty() || bytes.len() > 32 {
            return;
        }
        if !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            return;
        }
        for (i, b) in bytes.iter().enumerate() {
            self.0[i] = b.to_ascii_lowercase();
        }
    }

    /// Ist der Name leer (`auto`)?
    pub fn ist_leer(&self) -> bool {
        *self == Self::LEER
    }

    /// Vergleicht mit einem Namen aus dem Quelltext (Kleinschreibung).
    pub fn gleich(&self, s: &str) -> bool {
        if s.is_empty() || s.len() > 32 {
            return false;
        }
        let bytes = s.as_bytes();
        if !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
        {
            return false;
        }
        for (i, b) in bytes.iter().enumerate() {
            if self.0[i] != b.to_ascii_lowercase() {
                return false;
            }
        }
        self.0[bytes.len()..].iter().all(|b| *b == 0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpaltenMass {
    /// Feste Breite in px (% gegen Ansicht aufgeloest, dokumentiert).
    Fest(f32),
    /// fr-Anteil am Rest (nach festen Spalten + Luecken).
    Anteil(f32),
    /// L4 (03.10.2026 05:4x): die `auto`-SPUR (CSS Grid 1 §7.2).
    ///
    /// GEMESSENER BEFUND, dass die Form nicht ankam: `spalten_mass` gab bei
    /// `auto` kein Mass, und das `?` im Aufrufer hat die **ganze Liste**
    /// verworfen — GEMESSEN, `tests/position.rs`
    /// `l4_auto_ist_eine_spur_und_keine_weglassung`: `raster_spalten.len()`
    /// war **0** statt 2, also wurde aus `auto auto` eine implizite
    /// Vollspalte. Das ist ein Fehler mitten in der Spurliste, nicht ein
    /// fehlender Sonderfall.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Inhalt` und nicht `Auto`: `Auto`
    /// ist die WIEDERHOLUNG (`repeat(auto-fill, spur)`, §7.2.3) und
    /// traegt die Spur in sich. Hier ist die Spur selbst gemeint, und ihr
    /// Mass ist der Inhalt. §7.2 nennt `auto` als `<track-breadth>`, und
    /// §12.8 nennt die Spur danach: „tracks that have an auto max track
    /// sizing function".
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Variante OHNE Zahl ist: die Spur
    /// ist so breit wie ihr INHALT, und der Inhalt steht am Kind
    /// (`RasterKind::breit`) — dieselbe Form wie `hoch` bei den Zeilen.
    /// Eine Zahl hier waere gegen die falsche Breite gerechnet (§7.2.3,
    /// siehe `Auto`).
    ///
    /// GEMESSENE KORREKTUR AN MIR SELBST, 05:3x: der Kommentar im
    /// `BERICHT_CSS_L4_BENANNTE_BEREICHE.txt` behauptete, `auto` sei
    /// `max-content`, also die Inhaltsbreite — und daraus folgte
    /// „300 px sind falsch". Die Spec sagt in §12.8: bei `normal` oder
    /// `stretch` (der Anfangswert ist `normal`) werden Spalten mit
    /// `auto`-Obergrenze um den **Rest gleichmaessig** gedehnt. Also sind
    /// 300 px in einem 600-px-Kasten RICHTIG. Die Messung von 18:2x war
    /// richtig und ihre Deutung war es nicht.
    Inhalt,
    /// L4 (03.10.2026 09:3x): die PROZENT-SPUR (§10.1).
    ///
    /// GEMESSENER BEFUND, dass die Form falsch gerechnet wurde: `Prozent`
    /// wurde in `spalten_mass` zu `wert / 100.0 * ansicht` — und `ansicht`
    /// ist die ANSICHT. GEMESSEN (Release, `examples/l4_raster.rs`,
    /// Block Y2): ein **600-px**-Kasten mit
    /// `grid-template-columns: max(100px,50%)` ergab **640 px** statt 300.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zahl als Faktor durchwandert und
    /// nicht als px: der Prozentsatz gehoert dem CONTAINER, und den kennt
    /// die Kaskade nicht. Es ist dieselbe Frage wie bei `width: 50 %`, und
    /// dieselbe Musterloesung steht dort bereits: `Berechnet::
    /// breite_prozent` (GEMESSEN dort: „gesetzt genau dann, wenn die Angabe
    /// ein Prozent war ... der Aufrufer laesst das Layout rechnen").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `f32` in 0..1 und nicht Prozent als
    /// Zahl: `0.5` ist der Faktor, mit dem der Aufrufer multipliziert —
    /// eine zweite Umrechnung (÷ 100) an der Benutzungsstelle wäre eine
    /// zweite Wahrheit ueber dieselbe Zahl.
    Prozent(f32),
    /// L4 (02.10.2026 08:4x): `repeat(auto-fill | auto-fit, spur)`.
    ///
    /// GEMESSENER GRUND, warum das eine Variante ist und keine Zahl:
    /// `auto-fill` ZAEHLT, wie oft `spur` in die **Containerbreite**
    /// passt (CSS Grid 1 §7.2.3) — und die kennt die Kaskade nicht.
    /// `parse.rs` bekommt nur die Ansicht. Eine Zahl hier waere gegen
    /// die FALSCHE Breite gerechnet, das ist genau der Fehler, an dem
    /// der `width: 100%`-Rueckbau in `narsil-page` haengt
    /// (`BERICHT_CSS_L3_BREITE.txt`).
    ///
    /// GEMESSENER BEFUND, warum das nicht einfach ein `Vec` aus
    /// N-fachem `Fest` sein kann: N ist unbekannt, solange der
    /// Container nicht steht. GEMESSEN, Release, 600 px Kasten:
    /// `repeat(auto-fill, 100px)` ergibt 6 Spalten,
    /// `repeat(auto-fill, 250px)` ergibt 2, `700px` ergibt 1.
    Auto {
        /// Die Spur, die wiederholt wird.
        spur: Box<SpaltenMass>,
        /// `true` = `auto-fit`, `false` = `auto-fill`.
        ///
        /// GEMESSENE EINSCHRAENNKUNG, und sie ist eine echte: Beide
        /// ZAEHLEN heute gleich (siehe `lege_raster_bei`). Der
        /// Unterschied ist das Zusammenschrumpfen der LEEREN Spuren
        /// bei `auto-fit`, und das ist nicht gebaut. `fit` steht hier,
        /// damit der Unterschied spaeter eine Zeile ist und nicht
        /// wieder eine zweite Rechnung.
        fit: bool,
    },
    /// `minmax(min, max)` (CSS Grid 1 §7.2.2).
    ///
    /// GEMESSENER GRUND, warum das ein eigener Typ ist und kein
    /// `Fest`: `minmax` traegt drei Dinge gleichzeitig — eine
    /// UNTERgrenze, eine OBERgrenze und ein Mass. `Fest(f32)` kann
    /// genau eines davon, und welches, steht nicht mehr da. Das ist
    /// dieselbe Lücke wie bei `flex-basis: auto/content` und bei der
    /// fehlenden Kinderquerhöhe von `verteile_flex_alle` (88 px statt
    /// 20, `BERICHT_CSS_L3_LUECKEN.txt` 7d) — nur zeigt sie sich hier
    /// zuerst.
    ///
    /// GEMESSENER BELEG, dass es auf echten Seiten die haeufigste
    /// Rasterzeile ist: `repeat(auto-fit, minmax(270px, 1fr))` steht
    /// auf dem Korpus **13-mal**, und davon waren vorher 13 Zeilen
    /// einspaltig.
    MinMax {
        /// Die Untergrenze in px. 0 = `auto` als Minimum.
        min: f32,
        /// Die Obergrenze — wieder eine Spur, weil sie `fr` sein kann.
        max: Box<SpaltenMass>,
    },
}

/// Die berechneten Werte eines Elements — konkrete Zahlen und Farben,
/// keine Schluesselwoerter mehr (ausser der Schriftfamilie als Name).
#[derive(Debug, Clone, PartialEq)]
pub struct Berechnet {
    /// display (Anfangswert Inline; Bloecke kommen aus dem UA-Blatt).
    pub anzeige: Anzeige,
    /// Release 08.10.2026: `display: contents` (CSS Display 3 §2.8) — das
    /// Element erzeugt KEINE Box, seine Kinder gehoeren dem Elternkasten.
    /// `anzeige` bleibt dabei `Block` (fuer jeden, der das Feld nicht kennt).
    pub nur_inhalt: bool,
    /// Release 08.10.2026: `inline-flex`/`inline-grid` — innen Flex/Raster,
    /// AUSSEN inline (CSS Display 3 §2.1): der Kasten steht in der Zeile wie
    /// ein Inline-Block. `anzeige` traegt die innere Art.
    pub inline_aussen: bool,
    /// Release 11.10.2026 (Agent CSS2): `display: flow-root` (CSS Display 3
    /// §2.3) — ein Block mit eigenem Formatierungskontext.
    pub flow_root: bool,
    /// Release 11.10.2026: `display: grid-lanes` (CSS Grid 3, Masonry) —
    /// `anzeige` ist `Raster`, die Elemente liegen aber in Spuren
    /// („Lanes"): jedes in der jeweils kuerzesten.
    pub raster_lanes: bool,
    /// color in px-unabhaengig (Anfangswert schwarz, vererbt).
    pub farbe: Farbe,
    /// background-color (Anfangswert durchsichtig, NICHT vererbt).
    pub hintergrund: Farbe,
    /// font-size in px, aufgeloest (Anfangswert 16, vererbt).
    pub schriftgroesse_px: f32,
    /// Die Schriftgroesse des **WURZEL**elements in px — die Basis fuer
    /// `rem` (CSS Values 3 §5.2).
    ///
    /// 03.10.2026 15:0x. GEMESSENER BEFUND: `rem` war fest auf 16 px
    /// verdrahtet (`kaskade.rs::laenge_px`, „rem: 16"), und
    /// `html{font-size:75%}` — der uebliche Reset — ergibt im Browser
    /// 12 px.  `body{font-size:1.5rem}` ist dort **18 px** und kam bei
    /// uns als **24 px** an (Test `l4_rem_nimmt_die_schrift_der_wurzel`
    /// in `tests/position.rs`, rot mit „bekommen 24").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein Feld in `Berechnet` und nicht
    /// ein Parameter durch 19 Aufrufe von `laenge_px`: das Feld
    /// wandert mit der Elternkette, und `rem` braucht genau eine Zahl,
    /// die auf allen Ebenen gleich ist.  Ein Aufruf, der sie braucht,
    /// nimmt sie aus dem Eltern-`Berechnet` (siehe `kaskade.rs`).
    ///
    /// GEMESSENE FAELLE, die das Feld beantworten muss: `html` selbst
    /// (setzt es auf die eigene Schrift), jedes Kind (erbt es), und ein
    /// Knoten ohne Eltern (dann gilt die UA-Vorgabe 16 px).
    pub wurzel_schrift: f32,
    /// font-weight 100-900 (Anfangswert 400, vererbt).
    pub schriftgewicht: u16,
    /// font-style (Anfangswert normal, vererbt).
    pub schriftstil: Schriftstil,
    /// font-family, erste Familie (Anfangswert "serif", vererbt).
    pub schriftfamilie: String,
    /// line-height in px (Anfangswert 1,2 x Schriftgroesse, vererbt).
    pub zeilenhoehe_px: f32,
    pub zeilenhoehe_art: ZeilenhoeheArt,
    /// text-align (Anfangswert links, vererbt).
    pub textausrichtung: Textausrichtung,
    /// Release 13.10.2026 (Agent CSS2): `text-align-last` (CSS Text 3 §7.2),
    /// `start`/`end` schon nach der Schreibrichtung aufgeloest; `None` ist
    /// `auto`. Vererbt.
    pub ausrichtung_letzte: Option<Textausrichtung>,
    /// Release 13.10.2026 (Agent CSS2): `text-align: justify-all` — Blocksatz
    /// auch in der letzten Zeile (CSS Text 3 §7.1). Vererbt.
    pub blocksatz_alle: bool,
    /// Release 13.10.2026 (Agent CSS2): `text-align: start` (`Some(true)`,
    /// der Anfangswert) oder `end` (`Some(false)`) — der berechnete Wert ist
    /// das Schluesselwort und wird je Element nach SEINER Schreibrichtung zu
    /// `textausrichtung` (CSS Text 3 §7.1). `None`: eine physische Angabe.
    /// Vererbt.
    pub ausrichtung_logisch: Option<bool>,
    /// Release 13.10.2026 (Agent CSS2): `text-justify: none` — kein Blocksatz
    /// (CSS Text 3 §7.4). Vererbt.
    pub blocksatz_aus: bool,
    /// text-decoration als Bitmaske: 1 unter-, 2 ueber-, 4 durchgestrichen.
    pub dekoration: u8,
    /// white-space (Anfangswert normal, vererbt).
    pub weissraum: Weissraum,
    /// margin-oben/rechts/unten/links in px (Anfangswert 0).
    pub marge_oben: f32,
    pub marge_rechts: f32,
    pub marge_unten: f32,
    pub marge_links: f32,
    /// `margin-left: auto` / `margin-right: auto` — **die Information, die
    /// in `marge_links`/`marge_rechts` verloren ging.**
    ///
    /// GEMESSENER FEHLER (03.10.2026, Rolle CSS; BRETT 19:4x und 19:40 von
    /// CODEX): `rand_wert` gibt fuer `Wert::Auto` **0** zurueck, und die vier
    /// Randfelder sind `f32`. Damit kam `margin: 0 auto` als `0 / 0` an — der
    /// Motor kann nicht unterscheiden zwischen "Rand 0" und "Rand automatisch
    /// verteilen", und `auto_margin_versatz_x` in der Engine bekam nie zu
    /// sehen, was er zum Zentrieren braucht.
    ///
    /// CSS 2.1 §10.3.7 / §10.6.3: **`margin-left`/`margin-right: auto` auf
    /// einem Block mit fester Breite verteilt den Restplatz zu gleichen Teilen
    /// auf die beiden automatischen Raender** — das ist der Zentrierungsweg,
    /// und `margin: 0 auto` ist die gaengigste Schreibweise. Senkrecht ist
    /// `auto` dagegen **bedeutungslos** und ergibt 0 (§10.6.3) — deshalb gibt
    /// es hier nur zwei Felder und nicht vier.
    ///
    /// `false` heisst: entweder stand eine Laenge da, oder gar nichts. Beides
    /// ergibt 0 px, und beides darf den Motor nicht zentrieren.
    pub marge_links_auto: bool,
    pub marge_rechts_auto: bool,
    /// Release 11.10.2026: `margin-top`/`margin-bottom: auto`. Im Blockfluss
    /// ergibt das 0 (§10.6.3), in Flex und Raster nimmt ein solcher Rand den
    /// freien Platz (Flexbox 1 §8.1, Grid 1 §10.2) — GEMESSEN vorher:
    /// `margin-top: auto` in einer Flex-Spalte und `margin: auto` im Raster
    /// schoben nichts.
    pub marge_oben_auto: bool,
    pub marge_unten_auto: bool,
    /// padding-oben/rechts/unten/links in px (Anfangswert 0).
    pub fuellung_oben: f32,
    pub fuellung_rechts: f32,
    pub fuellung_unten: f32,
    pub fuellung_links: f32,
    /// Release 08.10.2026: Polster in PROZENT (oben, rechts, unten, links).
    /// CSS 2.1 §8.4: Prozent beziehen sich auf die BREITE des umgebenden
    /// Blocks — die kennt erst das Layout. GEMESSEN vorher: `padding-top:
    /// 56.25%` (Seitenverhaeltnis-Trick, YouTube-Geruest, Video-Einbettungen)
    /// wurde gegen die Fensterbreite gerechnet: 1062 statt 339 px.
    pub fuellung_prozent: [Option<f32>; 4],
    /// Dasselbe fuer `margin` (§8.3).
    pub marge_prozent: [Option<f32>; 4],
    /// `border-spacing` waagerecht/senkrecht in px (Anfangswert 0).
    ///
    /// CSS 2.1 §17.6: **nicht erblich**, Anfangswert `0`. HTML 5 §5.9 bildet
    /// `cellspacing` auf `border-spacing` ab, beide Achsen. Vorher gab es
    /// dafuer kein Feld, siehe `Eigenschaft::RandAbstandWaagerecht`.
    pub randabstand_waagerecht: f32,
    pub randabstand_senkrecht: f32,
    /// border-Breiten in px (Anfangswert 0).
    pub rahmen_oben_breite: f32,
    pub rahmen_rechts_breite: f32,
    pub rahmen_unten_breite: f32,
    pub rahmen_links_breite: f32,
    /// border-Stile (Anfangswert kein Rahmen).
    pub rahmen_oben_stil: Rahmenstil,
    pub rahmen_rechts_stil: Rahmenstil,
    pub rahmen_unten_stil: Rahmenstil,
    pub rahmen_links_stil: Rahmenstil,
    /// border-Farben (Anfangswert schwarz = currentcolor-Naeherung).
    pub rahmen_oben_farbe: Farbe,
    pub rahmen_rechts_farbe: Farbe,
    pub rahmen_unten_farbe: Farbe,
    pub rahmen_links_farbe: Farbe,
    /// width/max-width/min-width/height in px, None = auto.
    pub breite: Option<f32>,
    pub max_breite: Option<f32>,
    pub min_breite: Option<f32>,
    pub hoehe: Option<f32>,
    /// min-height in px (K1, 01.10.2026). `None` = nicht gesetzt.
    pub min_hoehe: Option<f32>,
    /// max-height in px (K1, 01.10.2026). `None` = nicht gesetzt.
    pub max_hoehe: Option<f32>,
    //
    // P19 (05.10.2026, 19:45): DIE EINHEIT, SEPARAT.
    //
    // GEMESSENER FEHLER, und er ist der Grund fuer diese sechs Felder:
    // `width: 100%` kommt als `breite = Some(900.0)` heraus, wenn die
    // Ansicht 900 px breit ist — **ohne** die Moglichkeit zu erkennen,
    // dass es 100 % waren. `kaskade.rs::breiten_wert` rechnet
    //
    //     laenge_px(l, em, ansicht, ansicht, ansicht_hoehe)
    //
    // und `laenge_px` macht `Prozent => l.wert / 100.0 * prozent` mit
    // `prozent = ansicht`. Die Prozentbasis ist damit die ANSICHT und
    // nicht der umgebende Block — eine DOKUMENTIERTE Naeherung
    // (kaskade.rs, Kommentar bei `MinHoehe`), aber eine, die die
    // Angabe unbrauchbar macht: wer `Some(900.0)` sieht, kann nicht
    // unterscheiden, was der Autor geschrieben hat.
    //
    // ZWEIMAL GEMESSEN, mit Folgen:
    //
    //  * 18:31 auf github.com: `min-width: 100%` an ZWEI Kaesten
    //    ergibt 900,00 px bei 900 px Ansicht. Der Eltern war 780 px.
    //  * 20:45, von der Rolle HTML gemeldet: `div{width:300px}` ergibt
    //    728 px (die Seitenbreite), `div{height:40px}` ergibt 19,2 px
    //    (eine Textzeile). Nicht wegen der Prozentbasis — hier wurde
    //    die Angabe gar nicht uebertragen, weil `Style::size` eine
    //    eigene Groesse ist (fremder Vertragstyp, narsil-core).
    //
    // WAS DIESE FELDER KENNZEICHNEN: gesetzt genau dann, wenn die
    // Angabe ein **Prozent** war. `Some(50.0)` heisst also
    // "`width: 50%`", und der Aufrufer schreibt dann KEINE Zahl, sondern
    // laesst das Layout rechnen. `breite` bleibt daneben unveraendert
    // stehen (die Zahl, die die Kaskade heute liefert), damit kein
    // Aufrufer etwas anderes bemerkt.
    //
    // DER AUFRUFER, fuer den es gedacht ist: `narsil-page` (Bruecke).
    // Es schreibt `Style::size.width` und kann ab jetzt absolute
    // Laengen von relativen unterscheiden. Ohne diese Felder gab es
    // dort nur die Zahl — und die Zahl ist die falsche (Antwort der
    // Rolle HTML vom 20.10., `team/PROBLEME.txt`).
    pub breite_prozent: Option<f32>,
    pub hoehe_prozent: Option<f32>,
    pub min_breite_prozent: Option<f32>,
    pub max_breite_prozent: Option<f32>,
    pub min_hoehe_prozent: Option<f32>,
    pub max_hoehe_prozent: Option<f32>,
    /// list-style-type (Anfangswert Scheibe).
    pub listenstil: Liste,
    /// visibility (Anfangswert sichtbar, vererbt).
    pub sichtbar: bool,
    /// overflow (Anfangswert sichtbar, nur gemerkt).
    pub ueberlauf: Ueberlauf,
    /// flex-direction (Anfangswert Reihe, NICHT vererbt).
    pub flex_richtung: FlexRichtung,
    /// justify-content (Anfangswert Start, NICHT vererbt).
    pub flex_buendig: FlexBuendig,
    /// align-items (Anfangswert Strecke, NICHT vererbt).
    pub flex_ausrichtung: FlexAusrichtung,
    /// gap in px (Anfangswert 0, NICHT vererbt).
    pub luecke_px: f32,
    /// flex-grow als Faktor (Anfangswert 0, NICHT vererbt).
    pub flex_wachstum: f32,
    /// flex-shrink als Faktor (Anfangswert 1, NICHT vererbt, C12, W6).
    pub flex_schrumpf: f32,
    /// flex-basis in px (None = auto, NICHT vererbt, C12, W6).
    pub flex_basis: Option<f32>,
    /// flex-wrap (Anfangswert false = nowrap, NICHT vererbt, C12, W6).
    pub flex_umbruch: bool,
    /// Aufgeloeste Variablen des Elements (vererbt, C10, W6).
    pub variablen: Variablen,
    /// position (Anfangswert statisch, NICHT vererbt).
    pub position: Position,
    /// top/right/bottom/left in px (None = auto, % gegen Ansicht).
    pub oben: Option<f32>,
    pub rechts: Option<f32>,
    pub unten: Option<f32>,
    pub links: Option<f32>,
    /// Prozentwerte bleiben bis zur Platzierung gegen den enthaltenden Block erhalten.
    pub oben_prozent: Option<f32>,
    pub rechts_prozent: Option<f32>,
    pub unten_prozent: Option<f32>,
    pub links_prozent: Option<f32>,
    /// z-index (None = auto).
    pub z_index: Option<i32>,
    /// float (Anfangswert keins).
    pub schweben: Float,
    /// clear (Anfangswert keins).
    pub raeumung: Raeumung,
    /// `align-self` (CSS Box Alignment 3 §6.3, §10.4) — die Ausrichtung
    /// des Kindes in seiner Rasterzeile.
    ///
    /// L4 (03.10.2026 09:5x). GEMESSENER BEFUND, dass die Form nicht
    /// ankam: `align-self` wurde als `Ziel::Lang(ESchluessel)` gelesen und
    /// landete im Schluessel-Behaelter. GEMESSEN (Release,
    /// `examples/l4_raster.rs`, Block AA, 300x100-Zelle, Kind 40x20):
    /// `stretch`, `center`, `flex-start`, `flex-end` und
    /// `justify-self: center` ergaben ALLE `b 300 h 100`.
    ///
    /// GEMESSENE HAEUFIGKEIT (Korpus-CSS + Korpusseiten): `align-self`
    /// 86 + 37, davon `center` 33x — die haeufigste Grid-Eigenschaft, die
    /// wir nicht anwenden.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum derselbe Typ wie `align-items`
    /// (`FlexAusrichtung`): §6.6 nennt fuer beide dieselbe Wertemenge, und
    /// ein zweiter Typ waere eine zweite Liste, die auseinanderlaufen kann.
    //
    // L4 (03.10.2026 11:5x): `None` heisst **`auto`** — §6.3: „auto
    // bedeutet, dass der Wert des Containers (`align-items`) benutzt
    // wird".  Das ist der Grund, warum es kein `FlexAusrichtung` mehr ist:
    // `stretch` ist sowohl der Anfangswert als auch ein ausdruecklicher
    // Wert, und mit einer blossen Zahl kann die Kaskade nicht
    // unterscheiden, ob das Kind etwas gesagt hat.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `Option` und keine neue Variante
    // `Auto` in `FlexAusrichtung`: `auto` ist ein Schluesselwort genau
    // dieser beiden Eigenschaften — `align-items` kennt es nicht. Eine
    // Variante in dem Enum wuerde einen Wert anbieten, den die
    // Container-Eigenschaft nicht annehmen darf.
    pub ausrichtung_selbst: Option<FlexAusrichtung>,
    /// `justify-self` (§6.2, §10.3) — dieselbe Form an der Querachse.
    pub ausrichtung_selbst_waagerecht: Option<FlexAusrichtung>,
    /// `justify-items` (§6.2) — der Vorgabewert fuer `justify-self`.
    ///
    /// GEMESSENER BEFUND, dass die Eigenschaft nicht ankam: sie zeigte im
    /// Parser auf `ESchluessel` (wie `justify-self` vor dem 09:5x-Punkt).
    /// GEMESSENE HAEUFIGKEIT: 5 (CSS) + 5 (HTML) — selten, aber es ist
    /// die Haelfte der Regel, und `align-items` steht ohne sie auf der
    /// Blockachse.
    pub justify_ausrichtung: FlexAusrichtung,
    /// grid-template-columns (leer = keine Angabe).
    pub raster_spalten: Vec<SpaltenMass>,
    /// Die LINIENNAMEN der Spalten (§7.2): `namen[i]` sind die Namen der
    /// Linie `i + 1`.
    ///
    /// L4 (03.10.2026 06:1x). GEMESSENER BEFUND, dass die Form nicht ankam:
    /// `raster_spalten` lehnte eine Spur ab, die mit `[` beginnt, und das
    /// `?` im Aufrufer warf die **ganze** Liste weg — aus
    /// `[content]1fr[visual]1fr` wurde nichts.
    ///
    /// GEMESSENE EINORDNUNG, warum das neben `raster_spalten` steht und
    /// nicht darin: die Namen gehoeren zu den LINIEN, nicht zu den Spuren.
    /// Eine Spur hat zwei Linien (vorher und nachher), also kaeme ein
    /// Name in der Spaltenliste doppelt vor oder gar nicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein eigenes Feld in `Berechnet`
    /// ist und kein Vektor neben `RasterKind`: die Namen stehen am
    /// CONTAINER, die Platzierung am KIND (§8.3 — „First attempt to match
    /// the grid area's edge to a named grid area" loest am Kind gegen die
    /// Vorlage des Containers auf).
    pub raster_spalten_namen: Vec<Vec<Gebiet>>,
    /// Die LINIENNAMEN der Zeilen (§7.2), dieselbe Form wie
    /// [`Berechnet::raster_spalten_namen`].
    pub raster_zeilen_namen: Vec<Vec<Gebiet>>,
    /// `grid-template-columns: subgrid` (CSS Grid 2 §7.2).
    ///
    /// L4 (03.10.2026 07:0x). GEMESSENER BEFUND, dass die Form nicht
    /// ankam: `spalten_mass("subgrid")` gibt kein Mass, und das `?` im
    /// Aufrufer hat die **ganze** Liste verworfen. GEMESSEN im Korpus
    /// (`crates/narsil-css/tests/daten/`): **16** Zeilen mit `subgrid`, 2
    /// davon in `@supports (grid-template-columns: subgrid)`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein Flag ist und keine Variante
    /// in `SpaltenMass`: ein Subgrid hat keine EIGENEN Spuren (§7.2), es
    /// benutzt die des Containers. Eine Spur in der Liste wuerde eine
    /// Zahl erfinden — und die Breite ist genau das, was wir noch nicht
    /// haben (siehe `raster_spalten`).
    pub raster_subgrid_spalten: bool,
    /// `grid-template-rows: subgrid`, dieselbe Form.
    pub raster_subgrid_zeilen: bool,
    /// Die SPUREN des Containers, uebernommen von einem Subgrid (§7.2).
    ///
    /// L4 (03.10.2026 07:2x). GEMESSENER BEFUND, warum es gebraucht
    /// wird: `subgrid` liest sich als leere Spurliste, und eine leere
    /// Liste ist eine implizite Vollspalte — ein Kind mit
    /// `grid-area: visual` (Linie 2) bekam **Breite 0** und war
    /// unsichtbar (GEMESSEN, Release, `l4_raster.rs` Block W2).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Massen und nicht die ANZAHL: §7.2
    /// sagt, das Subgrid BENUTZT die Linien des Containers — nicht nur
    /// deren Zahl, sondern deren Spuren.
    ///
    /// GEMESSENE EINSCHRAENKUNG: das sind die Massen aus dem CSS, nicht
    /// die im Layout aufgeloesten Breiten. Bei `200px 1fr` im Eltern
    /// weicht die `1fr`-Spalte ab.
    pub raster_eltern_spalten: Vec<SpaltenMass>,
    /// Die Zeilen des Containers, uebernommen von einem Subgrid (§7.2).
    pub raster_eltern_zeilen: Vec<SpaltenMass>,
    /// `grid-template-rows` (§8.3).
    ///
    /// L4 (02.10.2026 18:2x). **GECAPTURT, NOCH NICHT GERECHNET.**
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Werte hier landen und nicht
    /// verworfen werden: die Korrektur des Punktes ist die
    /// **Trennung** — vorher teilten sich Spalten und Zeilen ein Feld,
    /// und die Zeilen zerstoerten die Spalten. Ein eigenes Feld macht
    /// die Trennung messbar und laesst die Zahlen fuer den
    /// Zeilenpunkt (Grid 1 §8.3) bereit liegen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das KEIN toter Weg ist: §8.3 gibt
    /// fuer eine Zeile dieselbe Formel wie §8.2 fuer eine Spalte
    /// (`minmax(min, max)` und `fr`), und `SpaltenMass` kann beides
    /// darstellen. Das Feld ist also der ZIEL-TYP des naechsten
    /// Punktes, nicht ein Ablagefach. **Und es ist heute benutzt** —
    /// der Test `l4_grid_template_rows_landet_nicht_in_den_spalten`
    /// liest es.
    ///
    /// Bis der Zeilenpunkt gebaut ist, hat dieses Feld KEINE Wirkung
    /// auf das Layout. Das steht hier so, weil ein Feld ohne Wirkung
    /// und ohne Kommentar das schlimmste von beidem ist.
    pub raster_zeilen: Vec<SpaltenMass>,
    /// `grid-auto-rows` (§7.5): die Groesse der impliziten Zeilen.
    ///
    /// L4 (02.10.2026 21:5x). Der Anfangswert ist die LEERE Liste, und
    /// eine leere Liste heisst fuer eine implizite Zeile `auto` (§7.5)
    /// — das ist die Inhaltshoehe. Dieselbe Bedeutung wie ein fehlender
    /// Eintrag in `raster_zeilen`, und aus demselben Grund.
    pub raster_auto_zeilen: Vec<SpaltenMass>,
    /// `grid-auto-columns` (§7.3): die Groesse der impliziten Spalten.
    ///
    /// L4 (02.10.2026 21:5x). **Diese Liste ersetzt eine erfundene
    /// Zahl.** Bis 21:4x bekam eine implizite Spalte die breiteste
    /// explizite Spur als untere Schranke, weil `auto` als Hoechstwert
    /// `max-content` ist und `RasterKind` nur die Hoehe traegt. Mit
    /// dieser Liste ist der Fall „der Autor hat es angegeben" gedeckt;
    /// der ersetzte Wert bleibt nur dort, wo es die Liste nicht gibt.
    pub raster_auto_spalten: Vec<SpaltenMass>,
    /// border-radius der vier Ecken in px (K6, 01.10.2026).
    ///
    /// Reihenfolge: oben-links, oben-rechts, unten-rechts,
    /// unten-links — das ist die Reihenfolge von `border-radius`
    /// eins nach zwei (CSS Backgrounds 3, §5.4).
    ///
    /// Es ist die LAENGE, nicht die Form: die elliptische Angabe
    /// (`10px / 20px`) nimmt der Rasterizer, nicht die Kaskade.
    pub radius: [f32; 4],
    /// Release 15.10.2026 (Agent F): the corner radii as the box needs them
    /// (CSS Backgrounds 3 §5.1): per corner `[horizontal, vertical]` in px,
    /// or in percent of the border box width (horizontal) or height
    /// (vertical) where `radius_percent` has the bit (bit i: horizontal of
    /// corner i, bit 4 + i: vertical). `radius` keeps the circular
    /// approximation in px for the callers that know no ellipses.
    pub radius_xy: [[f32; 2]; 4],
    pub radius_percent: u8,
    /// opacity als Faktor 0-1 (Anfangswert 1 = deckend, CSS Color 4 §3.2).
    ///
    /// NICHT vererbt — `opacity` wirkt auf das GANZE Element
    /// einschliesslich der Kinder, also ist der geerbte Wert falsch
    /// (CSS Color 4 §3.2 nennt es ausdruecklich).
    pub deckkraft: f32,
    /// box-shadow: bis [`crate::MAX_SCHATTEN`] Eintraege (K6).
    pub schatten: Vec<Schatten>,
    /// outline-breite in px (K6). 0 = kein Outline.
    pub outline_breite: f32,
    /// outline-color (K6). Schwarz = Anfangswert.
    pub outline_farbe: Farbe,
    /// vertical-align als Schluesselwort (K6). Leer = `baseline`.
    pub vertikal: String,
    /// box-sizing (CSS Box Sizing 3, §2; K7). Anfangswert
    /// [`BoxArt::Inhalt`].
    pub box_art: BoxArt,

    // ---- K7c (01.10.2026): 26 neue, TYPISIERTE Felder --------------
    //
    // # WARUM NICHT ALLES IM SCHLUESSEL-BEHAELTER (siehe dort)
    //
    // Alles hier ist eine Zahl oder ein Schluesselwort, das ein
    // spaeterer Schritt AUSWERTEN muss — eine Zeile Abstand, eine
    // Rasterplatzierung, eine Achse fuer `overflow`. Wer nur den
    // Text speichert, muss es spaeter zerlegen, und der Zerleger
    // weiss dann nicht mehr, was ein Wert war.
    /// `letter-spacing` (CSS Text 4, §4.1) in px. 0 = normal.
    pub buchstaben_abstand: f32,
    /// `word-spacing` (CSS Text 4, §4.2) in px. 0 = normal.
    pub wort_abstand: f32,
    /// `text-indent` (CSS Text 4, §3.1) in px. 0 = keiner.
    pub text_einzug: f32,
    /// Release 11.10.2026 (Agent CSS2): `text-indent` in Prozent (0..), relativ
    /// zur Breite des enthaltenden Blocks (CSS 2.1 §16.1) — die rechnet erst
    /// der Seitenbau. Vererbt wird der Prozentwert, nicht die Zahl.
    pub text_einzug_prozent: Option<f32>,
    /// Release 12.10.2026 (Agent CSS2): `letter-spacing`/`word-spacing` in
    /// Prozent der EIGENEN Schriftgroesse (CSS Text 4 §4.1/§4.2); vererbt wird
    /// der Prozentwert, die Pixel rechnet der Seitenbau je Element.
    pub buchstaben_prozent: Option<f32>,
    pub wort_prozent: Option<f32>,
    /// `tab-size` (CSS Text 4, §8.2). 8 = der Anfangswert.
    pub tabulator_groesse: f32,
    /// Release 12.10.2026 (Agent CSS2): `tabulator_groesse` ist eine Laenge in
    /// px (sonst eine Zahl von Leerzeichen).
    pub tabulator_px: bool,
    /// `text-transform` (CSS Text 4, §5): leer = `none`.
    pub text_transform: String,
    /// `text-overflow` (CSS Text 4, §4.3): leer = `clip`.
    pub text_ueberlauf: String,
    /// `overflow-wrap` / `word-wrap` (CSS Text 4, §4.4). Die beiden
    /// sind laut Spec (Anmerkung dort) GLEICHWERTIG und landen
    /// darum in einem Feld.
    pub wort_umbruch: String,
    /// `word-break` (CSS Text 4, §4.5). Leer = `normal`.
    pub wort_trennung: String,
    /// `line-break` / `hyphens` (CSS Text 4, §6). Leer = `auto`.
    pub zeilen_trennung: String,

    /// `grid-column-start` (CSS Grid 1, §8.1) als 1-basiertes
    /// Liniennummer. 0 = `auto`.
    pub gitter_spalte_start: i32,
    /// `grid-column-end` (§8.3). 0 = `auto`.
    pub gitter_spalte_ende: i32,
    /// `grid-row-start` (§8.2). 0 = `auto`.
    pub gitter_zeile_start: i32,
    /// Die Spannweite aus `grid-row: span N` (CSS Grid 1 §8.3).
    ///
    /// L4 (02.10.2026 23:4x). GEMESSENE ENTSCHEIDUNG, warum das ein
    /// eigenes Feld ist und kein Teil von `gitter_zeile_start`:
    /// `grid-row-start: 2` (Linie 2) und `grid-row-start: span 2`
    /// (Spannweite 2) ergeben dieselbe Zahl, und aus der Zahl allein ist
    /// die Form nicht erkennbar. GEMESSEN: die Mischform gab
    /// `ende - start` = `0 - 2` und damit die Spannweite 1 — der Span
    /// wirkte nicht (Beispiel-Programm Block O: O2 gleich O1).
    ///
    /// `0` heisst „keine Angabe" und wird wie `1` gelesen.
    pub gitter_zeile_spanne: usize,
    /// Die Spannweite aus `grid-column: span N` (§8.3). Siehe
    /// `gitter_zeile_spanne`.
    pub gitter_spalte_spanne: usize,
    /// `grid-row-end` (§8.4). 0 = `auto`.
    pub gitter_zeile_ende: i32,
    /// `grid-template-areas` (CSS Grid 1, §7.3) als Rohliste, weil
    /// eine Liste von Listen ein Kasten im Kasten waere.
    /// `grid-template-areas` (§8.1): Zeilen von Gebietsnamen.
    ///
    /// L4 (03.10.2026 02:6x). **ERSETZT `gitter_gebiete: String`.**
    ///
    /// GEMESSENER BEFUND, der zum Ersetzen fuehrt: das alte Feld wurde in
    /// `kaskade.rs:3659` geschrieben und von niemandem gelesen — ein
    /// Rohfeld ohne Verwendung, also ein Feld, das nur Platz kostet.
    /// GEMESSENE ENTSCHEIDUNG, warum nicht BEIDE stehen: ein Rohfeld neben
    /// seinem geparsten Zwilling ist eine zweite Buchung derselben Sache,
    /// und die beiden koennen auseinanderlaufen — genau das ist bei
    /// `gitter_automatik` passiert (Bericht 21:9x).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Vec<Vec<Gebiet>>` und nicht eine
    /// Liste von Rechtecken: §8.1 nennt „a list of strings", und die
    /// Rechtecke sind eine ABGELEITETE Form. Wer die Liste hat, kann sie
    /// beliebig oft umrechnen; wer nur die Rechtecke hat, kann die
    /// Vorlage nicht mehr anzeigen.
    pub raster_gebiete: Vec<Vec<Gebiet>>,
    /// `grid-area` (§8.4) als Rohwert.
    ///
    /// L4 (03.10.2026 02:8x). GEMESSENE ENTSCHEIDUNG, warum das ein
    /// RohWERT bleibt und kein `Gebiet`: die Zahlform `r/c/r/c` ist noch
    /// nicht gebaut (GEMESSEN: 15 von 184 Vorkommen im Korpus), und
    /// `Gebiet::set` verwirft alles ausser Buchstaben, Ziffern und
    /// `-`/`_` — also wuerde die Zahlform als leerer Name ankommen und
    /// stillschweigend `auto` bedeuten. Ein Rohwert daneben macht den
    /// Zustand sichtbar und die Zahlform spaeter moeglich.
    pub grid_area_name: String,
    /// `grid-template` / `-columns` / `-rows` als Rohliste (§7.1-§7.2).
    pub gitter_vorlagen: String,
    /// `grid-auto-flow` / `-columns` / `-rows` (§7.4-§7.5).
    pub gitter_automatik: String,
    /// `grid-auto-flow: column` (ohne `dense`).
    ///
    /// L4 (02.10.2026 11:2x). GEMESSENER GRUND, warum neben dem
    /// Rohfeld ein aufgeloestes steht: `gitter_automatik` ist EIN Feld
    /// fuer DREI Eigenschaften (`-flow`, `-columns`, `-rows`) — die
    /// Rohform ist damit nicht unterscheidbar. Der Rohwert bleibt als
    /// Beleg stehen, und die Entscheidung „Spaltenrichtung" kommt hier
    /// hinein.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `dense` NICHT hier steckt:
    /// `dense` ist eine eigene Regel (§8.5: fruehere Luecken mit der
    /// Reihenfolge des Algorithmus fuellen) und wuerde in diesem
    /// Boolean versteckt. Sie ist nicht gebaut.
    pub gitter_fluss_spalten: bool,
    /// `grid-auto-flow: … dense` (§8.5) — die dichte Packung.
    ///
    /// L4 (03.10.2026 04:2x). GEMESSENER BEFUND, dass `dense` heute
    /// wirkungslos ist: es kam als zweites Wort in `gitter_automatik` an,
    /// und gelesen wurde nur das erste (11:3x). Also `row dense`
    /// verhielt sich wie `row`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das KEINE eigene Eigenschaft ist
    /// (wie `RasterAutoZeilen` vom 21:9x): §8.5 nennt `dense` ein
    /// ZUSATZ auf den Fluss, nicht eine eigene Angabe — „unless
    /// grid-auto-flow has the dense keyword". Es ist ein zweites Wort
    /// DERSELBEN Eigenschaft, und eine eigene Eigenschaft wuerde eine
    /// eigene Prioritaet und eigene Slots kosten (GEMESSEN am 21:9x: 6
    /// Rangaenge mal 32 Byte je Knoten).
    ///
    /// `false` heisst „sparse", und das ist der Anfangswert (§8.5: „The
    /// auto-placement algorithm will always place items in the 'sparse'
    /// manner").
    pub gitter_fluss_dicht: bool,

    /// `order` (CSS Flexbox 1, §5.1). 0 = der Anfangswert.
    pub ordnung: i32,
    /// `column-gap` / `grid-column-gap` (CSS Box Sizing 4 §2, CSS
    /// Grid 1 §10.1) in px. 0 = `normal`.
    pub luecke_spalte: f32,
    /// `row-gap` / `grid-row-gap` (CSS Grid 1 §10.1) in px.
    pub luecke_zeile: f32,

    /// `overflow-x` (CSS Overflow 3, §3.1). `None` = `visible`.
    pub overflow_x: Option<Ueberlauf>,
    /// `overflow-y` (§3.1).
    pub overflow_y: Option<Ueberlauf>,

    /// `object-fit` (CSS Images 1, §5.2). Leer = `fill`.
    pub objekt_passung: String,
    /// `aspect-ratio` (CSS Sizing 4, §4.1) als Zahl. 0 = `auto`.
    pub seitenverhaeltnis: f32,
    /// Release 11.10.2026: `auto && <ratio>` (das natuerliche Verhaeltnis
    /// eines ersetzten Elements geht vor).
    pub seitenverhaeltnis_auto: bool,
    /// `border-collapse` (CSS Tables 3, §3.3). Leer = `separate`.
    pub tabellen_kanten: String,
    /// `table-layout` (CSS Tables 3, §3.4). Leer = `auto`.
    pub tabellen_lage: String,
    /// Release 11.10.2026 (Agent CSS2): die genaue Tabellenrolle aus
    /// `display` (CSS 2.1 §17.2) — `anzeige` fasst Zeile und Zeilengruppen
    /// zusammen. Nicht geerbt.
    pub tabellen_rolle: narsil_layout::plan::TabellenRolle,
    /// Release 11.10.2026 (Agent CSS2): `caption-side: bottom` (§17.4.1).
    /// Geerbt.
    pub beschriftung_unten: bool,
    /// Release 11.10.2026 (Agent CSS2): `empty-cells: hide` (§17.6.1.1).
    /// Geerbt.
    pub leere_zellen_verbergen: bool,

    /// ALLES, was reingelesen wird und keinen eigenen Platz bekommt —
    /// der Schluesselbehaelter (K7c).
    ///
    /// # WARUM DAS EINE VERTRETBARE FORM VON „BERECHNET" IST
    ///
    /// CSS Values 3, §3 definiert den BERECHNETEN Wert einer
    /// Eigenschaft so: fuer alles, was keine Zahl und kein Farbwert
    /// ist, ist er der ANGEGEBENE Wert. Der berechnete Wert von
    /// `cursor: pointer` ist danach `pointer`, und nicht `14px` und
    /// nicht `unsichtbar`.
    ///
    /// `schluessel` haelt genau das: Eigenschaft, angegebener Wert.
    /// Es ist damit kein Muell, sondern die berechnete Form dieser
    /// Eigenschaften. Der Unterschied zu einem Wegwerfen ist, dass die
    /// Information BEI IHREN BLEIBT und in `Berechnet` steht.
    ///
    /// # GRENZE
    ///
    /// `MAX_SCHLUESSEL` (48) je Element. Eine Seite mit mehr als 48
    /// solchen Angaben auf EINEM Element hat sie nicht — die Kaskade
    /// nimmt die ersten 48 und zaehlt den Rest (M4).
    pub schluessel: Vec<(String, String)>,
    /// `direction` — die Schreibrichtung (K17). Geerbt, Anfangswert
    /// [`SchreibRichtung::Ltr`].
    ///
    /// GEMESSEN: `direction: rtl` steht auf keiner der 35 echten
    /// Dateien. Die Richtung kommt praktisch aus dem `dir`-Attribut,
    /// und das liefert `Element::richtung`.
    pub richtung: SchreibRichtung,
    /// `unicode-bidi` (K17). Geerbt, wie `direction`.
    ///
    /// GEMESSEN: 8 Deklarationen im Korpus, davon 6 in der
    /// Spezifikation und 2 als Praefix-Aliase. Ohne dieses Feld lagen
    /// alle 8 unaufgeloest im generischen Eimer.
    pub bidi: Bidi,
    /// `width`/`height` als Schluesselwort (CSS Sizing 3 §2; K7).
    /// Leer = eine Laenge (der Normalfall).
    pub breite_art: String,
    /// Release 11.10.2026 (Agent CSS2): `height: min-content | max-content |
    /// fit-content` (CSS Sizing 3 §2) — die Inhaltshoehe, auch zwischen
    /// `top` und `bottom` eines absoluten Kastens (keine Dehnung).
    pub hoehe_inhalt: bool,
    /// Die Masken in der Reihenfolge ihrer Ebenen (CSS Masking 1
    /// §4.1: die ERSTE ist oben). Hoechstens [`crate::MAX_MASKEN`].
    ///
    /// Leeres Feld = `mask-image: none` (der Anfangswert). Deshalb
    /// steht es nicht in `erbe`: Masken erben sich nicht (CSS Masking
    /// 1 §2.1 nennt sie nicht in der Vererbungsliste — anders als
    /// `background-image`, das es ebenfalls nicht tut).
    pub masken: Vec<Maske>,
    /// Die Bildquelle (K1 fuer Verlaeufe, K7 fuer `url(...)`).
    pub bild: BildQuelle,
    /// Die Geometrie dazu (K7).
    pub bild_geo: BildGeo,
    /// background-image: der gewinnende Verlauf (K1, 01.10.2026).
    ///
    /// `None` heisst "kein Verlauf" — das ist auch der Anfangswert
    /// (`background-image: none`, CSS Backgrounds 3, §2.1).
    ///
    /// WARUM EIGENES FELD und nicht im `Farbe`: `background` ist die
    /// Kurzform fuer Farbe UND Bild, und ein Verlauf ist keine Farbe.
    /// Wer in die Farbe einen Verlauf stopft, kann ihn nicht mehr von
    /// einer Farbe unterscheiden.
    pub verlauf: Option<Verlauf>,
    /// `background-blend-mode` (CSS Compositing 1 §3.1; K7c).
    ///
    /// Der Rasterizer unterstuetzt die Mischmodi NICHT (dokumentierte
    /// Naeherung): die Ebenen werden weiterhin mit `normal` gemischt.
    /// Der Wert wird trotzdem geparst und abgelegt, damit die Kaskade
    /// ihn kennt und ein spaeterer Rasterizer ihn nutzen kann.
    pub mischmodus: MischModus,
    /// Release 11.10.2026: die Rastereigenschaften (CSS Grid 1/2, Box
    /// Alignment 3) in der Form des neuen Rasteralgorithmus. `None` = alle
    /// Anfangswerte (keine Allokation fuer Elemente ohne Rasterangaben).
    pub gitter: Option<Box<narsil_layout::gitter::GitterWerte>>,
    /// Release 11.10.2026: die Groessen-Schluesselwoerter (CSS Sizing 3 §2.1)
    /// fuer `width`, `min-width`, `max-width`, `height`: 0 keines,
    /// 1 `min-content`, 2 `max-content`, 3 `fit-content`.
    pub groessen_art: [u8; 4],
    /// Release 14.10.2026: `stretch` (also `-webkit-fill-available`,
    /// `-moz-available`; CSS Sizing 4 §3.1) as a bit mask: 1 `width`,
    /// 2 `min-width`, 4 `max-width`, 8 `height`, 16 `min-height`,
    /// 32 `max-height`.
    pub stretch_bits: u8,
    /// Release 14.10.2026: `min-height` (bit 1) / `max-height` (bit 2) as
    /// `min-content`/`max-content`/`fit-content`: for a block the content
    /// height (CSS Sizing 3 §2.1, §5.1).
    pub height_keyword_limits: u8,
    /// Release 14.10.2026: the argument of `fit-content(<length-percentage>)`
    /// for `width`, `min-width`, `max-width` (CSS Sizing 3 §2.1; the keyword
    /// itself is `groessen_art` 3).
    pub fit_content_arg: [Option<narsil_layout::gitter::LaengeProzent>; 3],
    /// Release 11.10.2026: `flex-basis` mit Prozent und Schluesselwoertern
    /// (CSS Flexbox 1 §7.3.3) fuer den neuen Flex-Algorithmus.
    pub flex_basis_art: narsil_layout::flexstil::FlexBasis,
    /// Release 11.10.2026: `flex-wrap: wrap-reverse`.
    pub flex_umbruch_umgekehrt: bool,
    /// Release 11.10.2026: `flex-wrap: balance` (CSS Flexbox 2).
    pub flex_umbruch_ausgleich: bool,
    /// Release 11.10.2026: `flex-line-count` (CSS Flexbox 2), Anfangswert 1.
    pub flex_zeilen_min: u32,
    /// Release 11.10.2026: `overflow-x`/`overflow-y: clip` (kein Rollkasten).
    pub ueberlauf_clip: [bool; 2],
    /// Release 11.10.2026: `calc()` mit Prozentanteil als (px, Prozent) je
    /// Eigenschaft — die px-Felder daneben rechnen den Anteil weiter gegen
    /// die Ansicht (fuer Aufrufer ohne Bezug). `None` = keine Mischung.
    pub mischung: Option<Box<Mischung>>,
}

/// Release 11.10.2026: die `calc()`-Mischungen (px, Prozent) eines Elements
/// (CSS Values 4 §10.9). GEMESSEN vorher: `width: calc(10% + 50px)` und
/// `padding-right: calc(100% - 25px)` rechneten den Prozentanteil gegen die
/// Ansicht (WPT css-flexbox `dynamic-isize-change-003`, `gap-010-ltr`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mischung {
    pub breite: Option<(f32, f32)>,
    pub hoehe: Option<(f32, f32)>,
    /// `margin` oben, rechts, unten, links.
    pub marge: [Option<(f32, f32)>; 4],
    /// `padding` oben, rechts, unten, links.
    pub fuellung: [Option<(f32, f32)>; 4],
}

impl Berechnet {
    /// Anfangswerte ohne Eltern (CSS `initial` je Eigenschaft).
    pub fn standard() -> Berechnet {
        Berechnet {
            anzeige: Anzeige::Inline,
            nur_inhalt: false,
            inline_aussen: false,
            flow_root: false,
            raster_lanes: false,
            farbe: Farbe::SCHWARZ,
            hintergrund: Farbe::DURCHSICHTIG,
            schriftgroesse_px: BASIS_SCHRIFT_PX,
            // 03.10.2026 15:0x: ohne Eltern gilt die UA-Vorgabe (16 px).
            wurzel_schrift: BASIS_SCHRIFT_PX,
            schriftgewicht: 400,
            schriftstil: Schriftstil::Normal,
            schriftfamilie: String::from("serif"),
            zeilenhoehe_px: BASIS_SCHRIFT_PX * 1.2,
            zeilenhoehe_art: ZeilenhoeheArt::Normal,
            textausrichtung: Textausrichtung::Links,
            ausrichtung_letzte: None,
            blocksatz_alle: false,
            ausrichtung_logisch: Some(true),
            blocksatz_aus: false,
            dekoration: 0,
            weissraum: Weissraum::Normal,
            marge_oben: 0.0,
            marge_rechts: 0.0,
            marge_unten: 0.0,
            marge_links: 0.0,
            marge_links_auto: false,
            marge_rechts_auto: false,
            marge_oben_auto: false,
            marge_unten_auto: false,
            fuellung_oben: 0.0,
            fuellung_rechts: 0.0,
            fuellung_unten: 0.0,
            fuellung_links: 0.0,
            fuellung_prozent: [None; 4],
            marge_prozent: [None; 4],
            randabstand_waagerecht: 0.0,
            randabstand_senkrecht: 0.0,
            rahmen_oben_breite: 0.0,
            rahmen_rechts_breite: 0.0,
            rahmen_unten_breite: 0.0,
            rahmen_links_breite: 0.0,
            rahmen_oben_stil: Rahmenstil::Kein,
            rahmen_rechts_stil: Rahmenstil::Kein,
            rahmen_unten_stil: Rahmenstil::Kein,
            rahmen_links_stil: Rahmenstil::Kein,
            rahmen_oben_farbe: Farbe::SCHWARZ,
            rahmen_rechts_farbe: Farbe::SCHWARZ,
            rahmen_unten_farbe: Farbe::SCHWARZ,
            rahmen_links_farbe: Farbe::SCHWARZ,
            breite: None,
            max_breite: None,
            min_breite: None,
            hoehe: None,
            min_hoehe: None,
            max_hoehe: None,
            breite_prozent: None,
            hoehe_prozent: None,
            min_breite_prozent: None,
            max_breite_prozent: None,
            min_hoehe_prozent: None,
            max_hoehe_prozent: None,
            listenstil: Liste::Scheibe,
            sichtbar: true,
            ueberlauf: Ueberlauf::Sichtbar,
            flex_richtung: FlexRichtung::Reihe,
            flex_buendig: FlexBuendig::Start,
            flex_ausrichtung: FlexAusrichtung::Strecke,
            luecke_px: 0.0,
            flex_wachstum: 0.0,
            flex_schrumpf: 1.0,
            flex_basis: None,
            flex_umbruch: false,
            variablen: Variablen::default(),
            position: Position::Statisch,
            oben: None,
            rechts: None,
            unten: None,
            links: None,
            oben_prozent: None,
            rechts_prozent: None,
            unten_prozent: None,
            links_prozent: None,
            z_index: None,
            schweben: Float::Keins,
            raeumung: Raeumung::Keins,
            // L4 (09:5x): §6.6 — der Anfangswert ist `stretch`, und
            // `stretch` ist genau das, was die Platzierung ohnehin rechnet.
            // L4 (11:5x): `auto` heisst „den Wert des Containers" (§6.3),
            // also `None` — und der Anfangswert von `justify-items` ist
            // `stretch` (§6.2).
            ausrichtung_selbst: None,
            ausrichtung_selbst_waagerecht: None,
            justify_ausrichtung: FlexAusrichtung::Strecke,
            raster_spalten: Vec::new(),
            // L4 (03.10.2026 06:1x): keine Linien im Anfangswert — ein
            // Container ohne `grid-template-*` hat keine Linien, und §8.3
            // sagt dann „alle impliziten Linien tragen den Namen".
            raster_spalten_namen: Vec::new(),
            raster_zeilen_namen: Vec::new(),
            // L4 (07:0x): kein Subgrid im Anfangswert — `auto` ist kein
            // Subgrid, und §7.2 nennt es auch nicht als Anfangswert.
            raster_subgrid_spalten: false,
            raster_subgrid_zeilen: false,
            // L4 (07:2x): ohne Subgrid wird nichts uebernommen.
            raster_eltern_spalten: Vec::new(),
            raster_eltern_zeilen: Vec::new(),
            raster_zeilen: Vec::new(),
            raster_auto_zeilen: Vec::new(),
            raster_auto_spalten: Vec::new(),
            radius: [0.0; 4],
            radius_xy: [[0.0; 2]; 4],
            radius_percent: 0,
            deckkraft: 1.0,
            schatten: Vec::new(),
            outline_breite: 0.0,
            outline_farbe: Farbe::SCHWARZ,
            vertikal: String::new(),
            box_art: BoxArt::Inhalt,
            buchstaben_abstand: 0.0,
            wort_abstand: 0.0,
            text_einzug: 0.0,
            text_einzug_prozent: None,
            buchstaben_prozent: None,
            wort_prozent: None,
            tabulator_groesse: 8.0,
            tabulator_px: false,
            text_transform: String::new(),
            text_ueberlauf: String::new(),
            wort_umbruch: String::new(),
            wort_trennung: String::new(),
            zeilen_trennung: String::new(),
            gitter_spalte_start: 0,
            gitter_spalte_ende: 0,
            gitter_zeile_start: 0,
            gitter_zeile_spanne: 0,
            gitter_spalte_spanne: 0,
            gitter_zeile_ende: 0,
            raster_gebiete: Vec::new(),
            grid_area_name: String::new(),
            gitter_vorlagen: String::new(),
            gitter_automatik: String::new(),
            gitter_fluss_spalten: false,
            gitter_fluss_dicht: false,
            ordnung: 0,
            luecke_spalte: 0.0,
            luecke_zeile: 0.0,
            overflow_x: None,
            overflow_y: None,
            objekt_passung: String::new(),
            seitenverhaeltnis: 0.0,
            seitenverhaeltnis_auto: false,
            tabellen_kanten: String::new(),
            tabellen_lage: String::new(),
            tabellen_rolle: narsil_layout::plan::TabellenRolle::Keine,
            beschriftung_unten: false,
            leere_zellen_verbergen: false,
            schluessel: Vec::new(),
            richtung: SchreibRichtung::Ltr,
            bidi: Bidi::Normal,
            breite_art: String::new(),
            hoehe_inhalt: false,
            // `mask-image: none` ist EINE Ebene mit leerer URL
            // (CSS Masking 1 §4.1), nicht eine leere Liste. Im Bild ist
            // beides gleich; in der Anzahl der Ebenen nicht — und
            // `erbe` prueft `!p.masken.is_empty()`.
            //
            // GEMESSENER FEHLER 01.10.2026 (K7): hier stand
            // `Vec::new()`, und `mask-image: inherit` holte dann nichts,
            // weil „leer" als „unbekannt" gelesen wird.
            masken: vec![Maske {
                url: String::new(),
                groesse: MaskGroesse::Auto,
                position_x: 0.5,
                position_y: 0.5,
                verlauf: None,
                wiederholen_x: true,
                wiederholen_y: true,
            }],
            bild: BildQuelle::Keins,
            bild_geo: BildGeo::default(),
            verlauf: None,
            mischmodus: MischModus::Normal,
            gitter: None,
            groessen_art: [0; 4],
            stretch_bits: 0,
            height_keyword_limits: 0,
            fit_content_arg: [None; 3],
            flex_basis_art: narsil_layout::flexstil::FlexBasis::Auto,
            flex_umbruch_umgekehrt: false,
            flex_umbruch_ausgleich: false,
            flex_zeilen_min: 1,
            ueberlauf_clip: [false; 2],
            mischung: None,
        }
    }

    /// Release 11.10.2026: die Rasterwerte zum Schreiben (legt sie an).
    pub fn gitter_mut(&mut self) -> &mut narsil_layout::gitter::GitterWerte {
        // Release 11.10.2026: der Anfangswert von `justify-self`/`align-self`
        // ist `auto` (nicht `normal`, CSS Box Alignment 3 §6.1).
        self.gitter.get_or_insert_with(|| {
            Box::new(narsil_layout::gitter::GitterWerte {
                justify_self: narsil_layout::gitter::Ausr::AUTO,
                align_self: narsil_layout::gitter::Ausr::AUTO,
                ..Default::default()
            })
        })
    }
}

// ===========================================================================
// ELEMENT-TRAIT (Vertrag Abschnitt 2)
// ===========================================================================

/// Ein Element, gegen das Selektoren geprueft werden. Die Rolle HTML
/// implementiert das fuer ihre Knoten (Tag kleingeschrieben erwartet —
/// verglichen wird ASCII-fallunabhaengig, Klassen/IDs exakt).
pub trait Element {
    /// Tag in Kleinbuchstaben ("p", "div"). Leerer String = unbekannt.
    fn tag(&self) -> &str;
    /// id-Attribut ohne "#". None = kein id.
    fn id(&self) -> Option<&str>;
    /// True, wenn das class-Attribut `klasse` (ohne ".") enthaelt.
    fn hat_klasse(&self, klasse: &str) -> bool;
    /// Rohes Attribut ("href", "type", ...). None = nicht vorhanden.
    /// Namen werden kleingeschrieben verglichen.
    fn attribut(&self, name: &str) -> Option<&str>;
    /// Elternelement. None = Wurzel.
    fn eltern(&self) -> Option<Self>
    where
        Self: Sized;
    /// Geschwister direkt davor (fuer `+`, `~`, `:first-child`).
    fn voriges_geschwister(&self) -> Option<Self>
    where
        Self: Sized;
    /// Geschwister direkt danach (fuer `:last-child`).
    /// Abweichung zum WELLE4-Vorschlag: ohne diese Methode ist
    /// `:last-child` nicht pruefbar (siehe VERTRAG_CSS.txt).
    fn naechstes_geschwister(&self) -> Option<Self>
    where
        Self: Sized;
    /// Erstes Kind.
    fn erstes_kind(&self) -> Option<Self>
    where
        Self: Sized,
    {
        None
    }
    /// True wenn das Element keine Kinder (weder Knoten noch Text) hat.
    fn ist_leer(&self) -> bool {
        false
    }
    /// Die Klassen dieses Knotens, aufzaehlbar.
    ///
    /// # WARUM ES DAS GIBT (K11, 02.10.2026)
    ///
    /// Der Vormerker muss wissen, welche Regeln zu einem Knoten
    /// gehoeren. `hat_klasse` beantwortet nur die Frage "hast du
    /// diese EINE Klasse", nicht "welche hast du". Also fragt
    /// `kandidaten` ratweise durch ALLE abgelegten Klassennamen.
    ///
    /// GEMESSEN auf wikipedia_vector-2022.css: 567 Klassennamen, und
    /// bei 5000 Knoten 2 835 000 Stringvergleiche — mehr, als die 612
    /// eingesparten Matcher-Aufrufe einsparen. Der Index war deshalb
    /// nicht zu langsam, weil die Ecke schlecht war, sondern weil die
    /// MASKENERZEUGUNG teurer war als die Arbeit, die sie spart.
    ///
    /// Mit dieser Methode fragt der Index stattdessen DEN KNOTEN nach
    /// seinen wenigen Klassen. Das ist der Unterschied zwischen
    /// O(alle Klassennamen) und O(Klassen des Knotens).
    ///
    /// # DER DEFAULT IST ABSICHTLICH LEER
    ///
    /// Ein Knoten, der das nicht kann, meldet nichts, und der Index
    /// fragt weiter ratweise. Das ist langsam, aber nie falsch — und
    /// genau das ist der Grund, warum diese Methode einen Default hat
    /// und keine Pflichtmethode: `narsil-page`, `narsil` und `narsil-probe`
    /// bauen ihre `Element`-Typen ohne Aenderung weiter, bis sie sie
    /// implementieren.
    fn mit_klassen(&self, f: &mut dyn FnMut(&str)) {
        let _ = f;
    }
    /// Release 15.10.2026: `true`, wenn [`Element::mit_klassen`] ALLE Klassen
    /// des Knotens nennt — auch wenn es keine gibt. Ein Knoten ohne Klasse ist
    /// dann nicht „unbekannt": der Vormerker muss nicht ratweise durch alle
    /// Klassennamen der Blaetter gehen. GEMESSEN (spiegel.de, die Kaskade
    /// ueber das lebende DOM): jeder Knoten ohne Klassenliste nahm den
    /// langsamen Weg B ueber tausende Namen, der Animationsscan brauchte
    /// 7,7 s. Der Default `false` laesst jeden Knoten, der das nicht
    /// einfuehrt, wie bisher rechnen.
    fn klassen_aufzaehlbar(&self) -> bool {
        false
    }
    /// Die Schreibrichtung aus `dir="ltr|rtl|auto"` (K17).
    ///
    /// `None` = der Knoten sagt nichts, also erbt (CSS Writing Modes
    /// 3 §2.4: `direction` ist geerbt). Die Wurzel erbt von nichts und
    /// ist damit `ltr`.
    ///
    /// WARUM EINE METHODE MIT DEFAULT: HTML 5 §5.7.2 macht `dir` zu
    /// einer Praesentationsangabe, also gehoert der Wert aus dem DOM.
    /// `narsil-page` und `narsil` bauen ihre `Element`-Typen ohne
    /// Aenderung weiter, bis sie das implementieren — `None` ist
    /// „unbekannt", nicht „linksbündig".
    ///
    /// `auto` (bei `dir="auto"`) bestimmt die Richtung aus dem ersten
    /// starken Buchstaben. Das ist eine Frage an die Schrift, nicht an
    /// die Kaskade; der Aufrufer loest es auf und gibt `Ltr` oder `Rtl`.
    fn richtung(&self) -> Option<SchreibRichtung> {
        None
    }
    /// Release 2026-10-15: the current value of a form control (its dirty
    /// value) when the host keeps one; `None` = the content attribute (for
    /// `<textarea>` its text) decides. See `crate::form_state`.
    fn form_value(&self) -> Option<std::borrow::Cow<'_, str>> {
        None
    }
    /// Release 2026-10-15: whether `value` matches the `pattern` attribute
    /// `pattern` (a JavaScript regular expression with the `v` flag,
    /// anchored); `None` = the host cannot tell, and the pattern counts as
    /// satisfied.
    fn pattern_matches(&self, pattern: &str, value: &str) -> Option<bool> {
        let _ = (pattern, value);
        None
    }
    /// Release 2026-10-15: an HTML element in an HTML document (HTML
    /// §4.16.2 case-sensitivity of attribute selector values); `false` =
    /// unknown, values compare case-sensitively.
    fn is_html_in_html_document(&self) -> bool {
        false
    }
}

/// Release 2026-10-15: the attributes whose values attribute selectors
/// compare ASCII case-insensitively on HTML elements in HTML documents
/// (HTML §4.16.2), sorted.
const LEGACY_CASE_INSENSITIVE: [&str; 46] = [
    "accept", "accept-charset", "align", "alink", "axis", "bgcolor", "charset", "checked", "clear", "codetype", "color",
    "compact", "declare", "defer", "dir", "direction", "disabled", "enctype", "face", "frame", "hreflang", "http-equiv",
    "lang", "language", "link", "media", "method", "multiple", "nohref", "noresize", "noshade", "nowrap", "readonly", "rel",
    "rev", "rules", "scope", "scrolling", "selected", "shape", "target", "text", "type", "valign", "valuetype", "vlink",
];

fn legacy_case_insensitive(name: &str) -> bool {
    name.len() <= 14 && LEGACY_CASE_INSENSITIVE.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// An attribute selector comparison, ASCII case-insensitive without
/// allocating.
fn attr_op_ci(op: &AttrOp, a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    match op {
        AttrOp::Gleich => x.eq_ignore_ascii_case(y),
        AttrOp::Praefix => !y.is_empty() && x.len() >= y.len() && x[..y.len()].eq_ignore_ascii_case(y),
        AttrOp::Suffix => !y.is_empty() && x.len() >= y.len() && x[x.len() - y.len()..].eq_ignore_ascii_case(y),
        AttrOp::Teil => !y.is_empty() && x.windows(y.len()).any(|w| w.eq_ignore_ascii_case(y)),
        AttrOp::Wort => !y.is_empty() && a.split_whitespace().any(|w| w.as_bytes().eq_ignore_ascii_case(y)),
        AttrOp::Sprache => {
            x.eq_ignore_ascii_case(y) || (x.len() > y.len() && x[y.len()] == b'-' && x[..y.len()].eq_ignore_ascii_case(y))
        }
    }
}

// ===========================================================================
// ZUSTAND (C9, W5) — :hover/:focus als Parameter der Kaskade
// ===========================================================================

/// Interaktionszustand EINES Elements, vom Browser gesetzt und je
/// `berechnen_mit`-Aufruf uebergeben. Copy, keine Allokation.
/// Vereinfachung: Der Zustand gilt fuer die ganze gepruefte
/// Ahnenkette (Vorfahren-Pseudos sehen denselben Zustand).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Zustand {
    /// Zeiger steht ueber dem Element (`:hover` trifft).
    pub hover: bool,
    /// Element hat den Fokus (`:focus` trifft).
    pub focus: bool,
    /// Das Element selbst HAT den Fokus oder hat einen Nachfahren, der
    /// ihn hat (`:focus-within`, CSS Selectors 4 §6.1.4).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum ein eigenes Feld und nicht eine
    /// Ableitung aus `focus`: `Zustand` wird JE ELEMENT gebaut, und ein
    /// Vorfahre kann aus dem Zustand seines Nachfahren nicht ablesen,
    /// dass dieser den Fokus hat. Die Information muss beim Bau des
    /// Zustands entstehen — und der Host kennt sie, weil er den Baum
    /// von der Wurzel nach unten geht.
    ///
    /// GEMESSENE REICHWEITE (C1, WPT, 02.10.2026): 27 Faelle in
    /// `css/selectors` benutzen `:focus-within`; der Parser kannte den
    /// Namen nicht, also fiel jede dieser Regeln.
    pub focus_within: bool,
    /// Der Zeiger wird gedrueckt (`:active`, CSS Selectors 4 §6.1.2).
    ///
    /// GEMESSENE NUTZLAST 01.10.2026 (K12): 205 Vorkommen auf den 35
    /// echten Stylesheets, darunter `#news a:active` auf
    /// archlinux_archweb.css.
    pub aktiv: bool,
    /// Der Fokus kam mit der TASTATUR, nicht mit dem Zeiger
    /// (`:focus-visible`, §6.1.4).
    ///
    /// WARUM EIGENES FELD und nicht `focus`: §6.1.4 sagt ausdruecklich,
    /// dass ein per Zeiger gesetzter Fokus `:focus` traegt, aber
    /// NICHT `:focus-visible`. Beides in ein Feld zu legen hiesse: die
    /// eine Art von Fokus, die es gibt, faellt weg.
    ///
    /// GEMESSENE NUTZLAST: 148, darunter
    /// `a:where(:not([role='button'])):focus-visible` auf wikipedia.
    pub focus_sichtbar: bool,
    /// Das Fragment der Adresse zeigt auf dieses Element
    /// (`:target`, §6.2.2). 11 Vorkommen (`ol.references li:target`).
    pub ziel: bool,
    // ---- Die ELEMENT-ZUSTAENDE (aus HTML-Attributen) ----
    //
    // Die stehen hier und nicht in der `Element`-Schnittstelle, weil
    // `Zustand` der einzige Kanal ist, den diese Crate vom Aufrufer
    // kennt. GEMESSEN: im ganzen Baum baut NIEMAND ein
    // `Zustand`-Struct-Literal, alle nehmen `Zustand::KEINER` — deshalb
    // war das Ergaenzen der Felder ohne Bruch in einer fremden Crate
    // moeglich.
    /// HTML-Attribut `checked` (`:checked`, §6.3.2). 49 Vorkommen.
    pub ausgewaehlt: bool,
    /// HTML-Attribut `disabled` (`:disabled`, §6.3.4). 83 Vorkommen.
    pub gesperrt: bool,
    /// HTML-Attribut `indeterminate` (`:indeterminate`, §6.3.2).
    /// 8 Vorkommen.
    pub unbestimmt: bool,
    /// HTML-Attribut `readonly` (`:read-only`, §6.3.5). 1 Vorkommen.
    pub nur_lesen: bool,
    /// Das FARBSCHEMA der Anzeige (CSS Color 5 §3), gesetzt von
    /// `narsil` aus dem UI-Thema.
    ///
    /// WARUM ES HIER UND NICHT ALS PARAMETER: `berechnen` bekaeme
    /// sonst einen siebten Parameter, und GEMESSEN ist: im ganzen Baum
    /// schreibt niemand ein `Zustand`-Struct-Literal ausser in
    /// `narsil-css` selbst. `Zustand` IST der Kanal vom Aufrufer
    /// herueber — auch fuer einen Umgebungswert wie die
    /// Ansichtsbreite, die dort ueber die Parameter geht.
    ///
    /// ES IST KEIN ELEMENTZUSTAND, sondern eine Eigenschaft der
    /// Anzeige. Der Unterschied steht im Namen: `hover`, `aktiv` und
    /// `farbschema` sind drei verschiedene Dinge, und `hover` ist
    /// genauso wenig ein Umgebungswert wie `farbschema` ein
    /// Interaktionszustand.
    ///
    /// DER ANSCHLUSS, den K12/K13 brauchen (GEMESSEN, siehe
    /// `MediaAbfrage::trifft_mit_schema`): `prefers-color-scheme` in
    /// einer `@media`-Abfrage und `light-dark()` in einer Farbe.
    pub farbschema: Farbschema,
    /// Ausdruecklich aktiviert. NUR setzen, wenn das Element
    /// grundsaetzlich aktivierbar ist (also kein `disabled` und kein
    /// Formularfeld, das gar nicht aktiviert werden kann).
    ///
    /// WARUM EIGENES FELD: `:enabled` steht als `:enabled` im CSS
    /// (159 Vorkommen — mehr als `:disabled` mit 83) und wird als
    /// `:enabled` geparst. Wer es im Selektor zu `:not(:disabled)`
    /// umschreibt, bekommt eine andere Spezifitaet.
    pub aktiviert: bool,
}

// `Default` ist ABGELEITET und damit genauso `KEINER`: alle Felder
// sind `bool`, und `bool::default()` ist `false`. Ich habe erst eine
// eigene `impl Default` geschrieben und sie dann wieder entfernt —
// sie waere eine zweite Wahrheit ueber denselben Wert. Der Test
// `kein_und_default_sind_der_nullwert` in crates/narsil-css/tests/
// k12_zustaende.rs sichert die Gleichheit ab.

impl Zustand {
    /// Kein Zustand (Anfangswert; `berechnen` ohne Zustand nutzt diesen).
    pub const KEINER: Zustand = Zustand {
        hover: false,
        focus: false,
        focus_within: false,
        aktiv: false,
        focus_sichtbar: false,
        ziel: false,
        ausgewaehlt: false,
        gesperrt: false,
        unbestimmt: false,
        nur_lesen: false,
        aktiviert: false,
        // HELL als Vorgabe: ein Terminal im Zweifel ist hell. Und es ist
        // die Vorgabe, die der Code bis K13 hart verdrahtet hatte
        // (siehe `MediaZeile::merkmale_trifft_mit_schema`).
        farbschema: Farbschema::Hell,
    };

    /// Zeiger ueber dem Element (`Zustand` als Builder, damit der
    /// Aufrufer keine Struct-Literale schreiben muss — GEMESSEN: es
    /// gab keine, aber die Absicherung kostet nichts).
    pub fn mit_hover(mut self, an: bool) -> Self {
        self.hover = an;
        self
    }
    /// Der Zeiger wird gedrueckt.
    pub fn mit_aktiv(mut self, an: bool) -> Self {
        self.aktiv = an;
        self
    }
    /// Der Fokus liegt auf dem Element (`:focus`).
    ///
    /// GEMACHMALT, weil es das einzige Feld war, fuer das es keinen
    /// Baustein gab: `hover` und `focus` kamen vor K12 aus
    /// Struct-Literalen, und als ich die Felder ergaenzte, musste ich
    /// sie aufloesen. Ein Bausstein fuer ALLE Felder ist die
    /// einheitliche Form.
    pub fn mit_fokus(mut self, an: bool) -> Self {
        self.focus = an;
        self
    }
    /// Fokus im Element oder in einem Nachfahren (`:focus-within`).
    pub fn mit_fokus_within(mut self, an: bool) -> Self {
        self.focus_within = an;
        self
    }
    /// Tastatur-Fokus (`:focus-visible`).
    pub fn mit_fokus_sichtbar(mut self, an: bool) -> Self {
        self.focus_sichtbar = an;
        self
    }
    /// Fragment der Adresse zeigt hierher (`:target`).
    pub fn mit_ziel(mut self, an: bool) -> Self {
        self.ziel = an;
        self
    }
    /// HTML `disabled` (`:disabled`).
    pub fn mit_gesperrt(mut self, an: bool) -> Self {
        self.gesperrt = an;
        self
    }
    /// HTML `checked` (`:checked`).
    pub fn mit_ausgewaehlt(mut self, an: bool) -> Self {
        self.ausgewaehlt = an;
        self
    }
    /// HTML `indeterminate` (`:indeterminate`).
    pub fn mit_unbestimmt(mut self, an: bool) -> Self {
        self.unbestimmt = an;
        self
    }
    /// HTML `readonly` (`:read-only`).
    pub fn mit_nur_lesen(mut self, an: bool) -> Self {
        self.nur_lesen = an;
        self
    }
    /// Ausdruecklich aktivierbar (`:enabled`).
    pub fn mit_aktiviert(mut self, an: bool) -> Self {
        self.aktiviert = an;
        self
    }
    /// Das Farbschema der Anzeige (aus dem UI-Thema von `narsil`).
    pub fn mit_farbschema(mut self, s: Farbschema) -> Self {
        self.farbschema = s;
        self
    }
}

// ===========================================================================
// SELEKTORPRUEFUNG (C2) — generisch ueber Element, ohne Allokation
// ===========================================================================

/// True, wenn der Selektor auf das Element zutrifft (ohne Zustand:
/// `:hover`/`:focus` sind falsch). Huelle um [`selektor_passt_mit`].
/// Steht vor `el` ein Geschwister mit demselben Tag?
///
/// fuer `:first-of-type` (CSS Selectors 4 §6.3.7). Der Weg nach unten
/// ist `voriges_geschwister()` — vorhanden.
///
/// M3: Die Schleife endet bei `MAX_GESCHWISTER_SCAN` (512) Vergleichen. Eine
/// Seite hat keine 64 Geschwister DESSELBEN Typs vor einem Element, und
/// eine Klammer- oder Endlos-Bombe im Knoten-Zugriff kann die Frage
/// nicht unbegrenzt stellen. Nach der Grenze gilt „kein solcher
/// Geschwister" — das ist die wahre Antwort fuer jede reale Seite.
fn hat_geschwister_davor_mit_tag<E: Element>(el: &E, tag: &str) -> bool {
    let mut n = 0usize;
    let mut kuerzer = el.voriges_geschwister();
    while let Some(v) = kuerzer {
        if n >= crate::MAX_GESCHWISTER_SCAN {
            return false;
        }
        if v.tag().eq_ignore_ascii_case(tag) {
            return true;
        }
        kuerzer = v.voriges_geschwister();
        n += 1;
    }
    false
}

/// Steht nach `el` ein Geschwister mit demselben Tag? Siehe
/// [`hat_geschwister_davor_mit_tag`] zur Grenze.
fn hat_geschwister_danach_mit_tag<E: Element>(el: &E, tag: &str) -> bool {
    let mut n = 0usize;
    let mut kuerzer = el.naechstes_geschwister();
    while let Some(v) = kuerzer {
        if n >= crate::MAX_GESCHWISTER_SCAN {
            return false;
        }
        if v.tag().eq_ignore_ascii_case(tag) {
            return true;
        }
        kuerzer = v.naechstes_geschwister();
        n += 1;
    }
    false
}

/// Trifft die Position `n` auf `an+b` (CSS Selectors 4, §6.3)?
///
/// # DER ZWEITE FEHLER AN DERSELBEN STELLE
///
/// In beiden Zweigen stand am Ende:
///
/// ```text
/// let diff = n - *b;
/// diff % *a == 0 && diff / *a >= 0
/// ```
///
/// 1. `n - *b` ist ein **Unterlauf**, sobald `b > n` — also bei
///    `:nth-child(999)` an einem Knoten mit drei Geschwistern. Im Debug
///    ein Panic (`attempt to subtract with overflow`), im Release ein
///    Riesenzahl, die dann fast nie passt. Beides ist ein Fehler bei
///    fremder Eingabe; REGELN.txt verbietet beides.
/// 2. `diff / *a >= 0` ist fuer vorzeichenlose Werte IMMER wahr und
///    damit toter Code, der so aussieht als pruefe er etwas.
///
/// Jetzt wird die Rechnung in `i64` gemacht und der Unterlauf
/// ausdruecklich als "trifft nicht" beantwortet.
fn ntes_trifft(an: i32, b: i32, n: i64) -> bool {
    let an = an as i64;
    let b = b as i64;
    if an == 0 {
        return n == b;
    }
    // `n` kann nie groesser sein als MAX_GESCHWISTER_SCAN + 1, und `b`
    // kann jeder i32 sein — die Differenz passt beides in i64.
    let diff = n - b;
    diff % an == 0 && diff / an >= 0
}

/// Wie [`selektor_passt`], ohne Zustand (`:hover`/`:focus` treffen
/// nicht).
pub fn selektor_passt<E: Element>(selektor: &Selektor, el: &E) -> bool {
    selektor_passt_mit(selektor, el, &Zustand::KEINER)
}

/// True, wenn der Selektor auf das Element mit `zustand` zutrifft.
/// Laeuft die Kette von rechts nach links; Ahnen- und Geschwisterwege
/// sind gedeckelt (MAX_AHNEN, MAX_GESCHWISTER_SCAN): danach gilt "kein
/// Treffer". Braucht kein Clone: der Treffer wandert als owned-Wert von
/// rechts nach links, der erste Schritt startet an der Referenz `el`.
/// Release 15.10.2026 (Agent CSS2): the directionality of `el` (HTML §3.2.6.4,
/// what `:dir()` matches): the nearest element up the tree that states one
/// decides — [`Element::richtung`] (the host resolves `dir=auto`, `bdi`,
/// shadow roots), else a plain `dir=ltr|rtl` attribute; without any, `ltr`.
/// MEASURED before (WPT html `dom/elements/global-attributes/dir-shadow-01`,
/// `element.matches(":dir(ltr)")`): `:dir()` was unknown and never matched.
pub fn directionality<E: Element>(el: &E) -> SchreibRichtung {
    let mut owned: Option<E> = None;
    for _ in 0..1024 {
        let e = owned.as_ref().unwrap_or(el);
        if let Some(r) = e.richtung() {
            return r;
        }
        match e.attribut("dir").map(str::trim) {
            Some(v) if v.eq_ignore_ascii_case("rtl") => return SchreibRichtung::Rtl,
            Some(v) if v.eq_ignore_ascii_case("ltr") => return SchreibRichtung::Ltr,
            _ => {}
        }
        let next = e.eltern();
        match next {
            Some(p) => owned = Some(p),
            None => break,
        }
    }
    SchreibRichtung::Ltr
}

pub fn selektor_passt_mit<E: Element>(selektor: &Selektor, el: &E, zustand: &Zustand) -> bool {
    let kette = &selektor.kette;
    if kette.is_empty() {
        return false;
    }
    // Das rechteste Glied muss am Element selbst zutreffen.
    let mut i = kette.len() - 1;
    if !einfach_passt(&kette[i].einfach, el, zustand) {
        return false;
    }
    // `treffer`: das zuletzt getroffene Element als owned-Wert.
    // None = "der Treffer ist `el` selbst" (erster Schritt).
    // Die Schleife ist durch die Kettenlaenge begrenzt (der Parser
    // garantiert hoechstens MAX_SELEKTOR_TEILE Glieder).
    let mut treffer: Option<E> = None;
    while i > 0 {
        let komb = kette[i].komb;
        let links = &kette[i - 1].einfach;
        let gefunden: Option<E> = match komb {
            Kombinator::Keiner => {
                // Darf nur am ersten Glied stehen; sonst ist die Kette
                // kaputt (kann der Parser nicht bauen) — kein Treffer.
                return false;
            }
            Kombinator::Kind => {
                // Genau ein Schritt zum Elternelement.
                eltern_von(&treffer, el).filter(|e| einfach_passt(links, e, zustand))
            }
            Kombinator::Nachfahre => {
                // Irgendein Ahne (gedeckelt, M3).
                let mut kandidat = eltern_von(&treffer, el);
                let mut gefunden: Option<E> = None;
                for _ in 0..MAX_AHNEN {
                    match kandidat {
                        None => break,
                        Some(e) => {
                            if einfach_passt(links, &e, zustand) {
                                gefunden = Some(e);
                                break;
                            }
                            kandidat = e.eltern();
                        }
                    }
                }
                gefunden
            }
            Kombinator::Nachbar => {
                // Genau ein Schritt zum Vorgaengergeschwister.
                davor_von(&treffer, el).filter(|e| einfach_passt(links, e, zustand))
            }
            Kombinator::Geschwister => {
                // Irgendein Vorgaengergeschwister (gedeckelt, M3).
                let mut kandidat = davor_von(&treffer, el);
                let mut gefunden: Option<E> = None;
                for _ in 0..MAX_GESCHWISTER_SCAN {
                    match kandidat {
                        None => break,
                        Some(e) => {
                            if einfach_passt(links, &e, zustand) {
                                gefunden = Some(e);
                                break;
                            }
                            kandidat = e.voriges_geschwister();
                        }
                    }
                }
                gefunden
            }
        };
        match gefunden {
            None => return false,
            Some(e) => {
                treffer = Some(e);
            }
        }
        i -= 1;
    }
    true
}

/// Passt ein RELATIVER Selektor auf `el` — das Innere von `:has()`.
///
/// # WAS EIN RELATIVER SELEKTOR IST
///
/// `:has(> a)`, `:has(+ p)`, `:has(~ li)`, `:has(form input)`. Anders
/// als ein normaler Selektor hat er keinen "Startpunkt im Dokument",
/// sondern einen **Anker**: das Element, in dessen `:has()` er steht.
/// Gelesen wird die Kette von LINKS nach RECHTS, vom Anker weg:
///
/// ```text
/// :has(> form > input)   ->  Anker > form > input
/// ```
///
/// # WARUM EINE EIGENE FUNKTION
///
/// `selektor_passt_mit` laeuft von rechts nach links und benutzt dabei
/// die Kombinatoren als "ein Schritt zum Eltern/Geschwister". Fuer
/// `:has(> form > input)` ist aber `:has` an `[Kind, Kind]` haengt und
/// das ERSTE Glied sitzt dem Anker am naechsten. Mit der vorhandenen
/// Funktion wuerde `input` am Anker geprueft — also immer falsch.
///
/// Deshalb wird die Menge der Kandidaten GLIED FUER GLIED aufgebaut.
/// Das ist zugleich die Antwort auf die alte Rekursion: hier gibt es
/// eine Bremsengrenze statt einer Tiefengrenze.
///
/// # BREMSEN (M3/M4)
///
/// `MAX_HAS_KANDIDATEN` begrenzt die Menge, `MAX_HAS_BESUCHE` die
/// Gesamtzahl besuchter Knoten. Ueberschritten heisst "kein Treffer",
/// nicht "weiterlaufen".
pub fn relativ_passt_mit<E: Element>(sel: &Selektor, el: &E, zustand: &Zustand) -> bool {
    let kette = &sel.kette;
    if kette.is_empty() {
        return false;
    }
    // Restarbeitskonto: von `MAX_HAS_BESUCHE` heruntergezaehlt.
    let mut besuche = MAX_HAS_BESUCHE;

    // Glied 0: die Kandidaten direkt am Anker.
    //
    // `None` bedeutet "der Anker selbst" — dasselbe Muster, das
    // `selektor_passt_mit` fuer seinen ersten Schritt benutzt. Es ist
    // noetig, weil `Element` KEIN `Clone` und KEIN `Copy` verlangt: ein
    // Kandidat muss sich als besessener Wert bewegen, und fuer den
    // Anker gibt es keinen besessenen Wert, nur die Referenz.
    let mut menge: Vec<Option<E>> = Vec::new();
    // `Keiner` heisst hier "Nachfahre": `:has(li)` sucht ein `li` in
    // beliebiger Tiefe unter dem Anker, nicht das Element selbst.
    // (GEMESSEN an Chrome/Firefox: `div:has(span)` ist wahr, auch wenn
    // das `span` fuenf Ebenen tiefer haengt. Nach Selectors Level 4,
    // 4.3 wird ein Relativselektor ohne fuehrenden Kombinator wie
    // `:has(> ...)` gelesen, also absteigend.)
    let komb0 = match kette[0].komb {
        Kombinator::Keiner => Kombinator::Nachfahre,
        k => k,
    };
    sammle_relativ(&mut menge, &None, el, komb0, &mut besuche);

    // Glieder 1..n: von jedem Kandidaten aus eine Ebene weiter.
    for gi in 1..kette.len() {
        if menge.is_empty() {
            return false;
        }
        let mut naechste: Vec<Option<E>> = Vec::new();
        let alt = std::mem::take(&mut menge);
        for c in alt.iter() {
            // Release 10.10.2026: nur Kandidaten, die ihr EIGENES Glied
            // erfuellen, gehen weiter. GEMESSEN vorher: geprueft wurde nur
            // das letzte Glied — `:has(> section > u)` passte auf jedes
            // Element mit irgendeinem Enkel `u`.
            if !einfach_von(c, el, &kette[gi - 1].einfach, zustand) {
                continue;
            }
            sammle_relativ(&mut naechste, c, el, kette[gi].komb, &mut besuche);
            if naechste.len() >= MAX_HAS_KANDIDATEN {
                // Abgeschnitten (M3): der Rest kann den Anker nicht mehr
                // erreichen. "Kein Treffer" ist dann die ehrliche
                // Antwort, und die Seite wird nicht auf ewig gesucht.
                return false;
            }
        }
        menge = naechste;
    }
    let letzte = &kette[kette.len() - 1].einfach;
    menge.iter().any(|c| einfach_von(c, el, letzte, zustand))
}

/// `einfach_passt` fuer einen Kandidaten, der `None` sein darf
/// ("der Anker selbst").
fn einfach_von<E: Element>(
    kandidat: &Option<E>,
    anker: &E,
    einfach: &Einfach,
    zustand: &Zustand,
) -> bool {
    match kandidat {
        Some(e) => einfach_passt(einfach, e, zustand),
        None => einfach_passt(einfach, anker, zustand),
    }
}

/// Legt alle Knoten aus `komb` um `von` nach `heraus`.
///
/// M3: `heraus` wird auf `MAX_HAS_KANDIDATEN` begrenzt und `besuche`
/// heruntergezaehlt — BEIDES wird VOR dem Anhaengen geprueft, damit ein
/// Dokument mit 10^9 Kindern hier keinen Speicher belegt.
fn sammle_relativ<E: Element>(
    heraus: &mut Vec<Option<E>>,
    von: &Option<E>,
    anker: &E,
    komb: Kombinator,
    besuche: &mut usize,
) {
    // Das erste Kind von `von` (oder vom Anker, wenn `von` der Anker ist).
    let mut kind = match von {
        Some(e) => e.erstes_kind(),
        None => anker.erstes_kind(),
    };
    // `> Platz, ob noch einer hineinpasst.
    macro_rules! frei {
        () => {
            *besuche > 0 && heraus.len() < MAX_HAS_KANDIDATEN
        };
    }
    match komb {
        Kombinator::Keiner => {
            // "Das Element selbst." `Keiner` steht nur am ersten Glied
            // (der Parser legt es nirgends sonst an), also ist der
            // Anker der einzige moegliche Kandidat. `None` ist hier
            // richtig UND vermeidet ein `clone()` — `Element` hat keine
            // `Clone`-Schranke, und der Anker liegt nur als Referenz vor.
            if !frei!() {
                return;
            }
            *besuche -= 1;
            heraus.push(None);
        }
        Kombinator::Kind => {
            while let Some(k) = kind {
                if !frei!() {
                    return;
                }
                // DER GESCHWISTER WIRD ZUERST GESUCHT: `k` wandert gleich
                // in `heraus`, und `Element` hat kein `clone()`.
                let nxt = k.naechstes_geschwister();
                *besuche -= 1;
                heraus.push(Some(k));
                kind = nxt;
            }
        }
        Kombinator::Nachfahre => {
            // Breitensuche mit einem Stapel im Heap (M5). Die alte
            // Fassung war eine REKURSION mit Tiefengrenze 10, und eine
            // Seite mit 50 000 Knoten unter dem Wurzelknoten las damit
            // 50 000 Knoten fuer JEDEN Knoten der Kaskade.
            //
            // `stapel` haelt `Option<E>`, damit die Elemente beim
            // Abarbeiten WEGGEBEN werden koennen (kein `clone()`).
            let mut stapel: Vec<Option<E>> = Vec::new();
            while let Some(k) = kind {
                if stapel.len() >= MAX_HAS_BESUCHE {
                    break;
                }
                stapel.push(Some(k));
                kind = match stapel.last() {
                    Some(Some(l)) => l.naechstes_geschwister(),
                    _ => None,
                };
            }
            let mut i = 0;
            while i < stapel.len() {
                if !frei!() {
                    return;
                }
                let Some(k) = stapel[i].take() else {
                    i += 1;
                    continue;
                };
                let mut w = k.erstes_kind();
                while let Some(c) = w {
                    if stapel.len() >= MAX_HAS_BESUCHE {
                        break;
                    }
                    let nxt = c.naechstes_geschwister();
                    stapel.push(Some(c));
                    w = nxt;
                }
                *besuche -= 1;
                heraus.push(Some(k));
                i += 1;
            }
        }
        Kombinator::Nachbar | Kombinator::Geschwister => {
            let mut w = match von {
                Some(e) => e.naechstes_geschwister(),
                None => anker.naechstes_geschwister(),
            };
            let mut n = 0usize;
            while let Some(c) = w {
                if komb == Kombinator::Nachbar && n > 0 {
                    break; // `+` heisst: nur der ERSTE
                }
                if !frei!() {
                    return;
                }
                let nxt = c.naechstes_geschwister();
                *besuche -= 1;
                heraus.push(Some(c));
                n += 1;
                if n >= MAX_GESCHWISTER_SCAN {
                    break;
                }
                w = nxt;
            }
        }
    }
}

/// Elternelement des Treffers: `treffer` ist der owned-Wert aus dem
/// vorherigen Schritt oder None fuer "der Treffer ist `el` selbst".
fn eltern_von<E: Element>(treffer: &Option<E>, el: &E) -> Option<E> {
    match treffer {
        Some(e) => e.eltern(),
        None => el.eltern(),
    }
}

/// Vorgaengergeschwister des Treffers (wie `eltern_von`).
fn davor_von<E: Element>(treffer: &Option<E>, el: &E) -> Option<E> {
    match treffer {
        Some(e) => e.voriges_geschwister(),
        None => el.voriges_geschwister(),
    }
}

/// True, wenn die UND-Bedingung am Element selbst zutrifft.
fn einfach_passt<E: Element>(einfach: &Einfach, el: &E, zustand: &Zustand) -> bool {
    if let Some(tag) = &einfach.tag {
        if !el.tag().eq_ignore_ascii_case(tag) {
            return false;
        }
    }
    if let Some(id) = &einfach.id {
        match el.id() {
            None => return false,
            Some(haben) => {
                if haben != id {
                    return false;
                }
            }
        }
    }
    for klasse in &einfach.klassen {
        // K11: die Gegenseite zu `HAT_KLASSE_VORPRUEFUNG`. Zusammen
        // ergeben die beiden Zaehler die 897,7 `hat_klasse()` je Knoten,
        // die sich seit Tagen nicht einordnen liessen.
        crate::kaskade::profil(crate::kaskade::pz::HAT_KLASSE_MATCHER);
        if !el.hat_klasse(klasse) {
            return false;
        }
    }
    for bed in &einfach.attribute {
        // K11: `attribut()` ALLOKIERT in `narsil-page::El` (es macht
        // `to_ascii_lowercase()` fuer den HashMap-Schluessel). Der
        // Zaehler macht sichtbar, wie oft das passiert.
        crate::kaskade::profil(crate::kaskade::pz::ATTRIBUT);
        match el.attribut(&bed.name) {
            None => return false,
            Some(haben) => {
                if let Some((op, wert, flag)) = &bed.bedingung {
                    let mut a = haben;
                    let mut b = wert.as_str();
                    let a_lower;
                    let b_lower;
                    if flag.is_none() && legacy_case_insensitive(&bed.name) && el.is_html_in_html_document() {
                        if !attr_op_ci(op, a, b) {
                            return false;
                        }
                        continue;
                    }
                    if *flag == Some(true) {
                        a_lower = a.to_ascii_lowercase();
                        b_lower = b.to_ascii_lowercase();
                        a = &a_lower;
                        b = &b_lower;
                    }
                    let passt = match op {
                        AttrOp::Gleich => a == b,
                        AttrOp::Praefix => a.starts_with(b) && !b.is_empty(),
                        AttrOp::Suffix => a.ends_with(b) && !b.is_empty(),
                        AttrOp::Teil => a.contains(b) && !b.is_empty(),
                        AttrOp::Wort => a.split_whitespace().any(|w| w == b) && !b.is_empty(),
                        AttrOp::Sprache => a == b || a.starts_with(&format!("{}-", b)),
                    };
                    if !passt {
                        return false;
                    }
                }
            }
        }
    }
    for pseudo in &einfach.pseudos {
        let ok = match pseudo {
            // GEMESSENER FEHLER 30.09.2026 (R0 c): hier stand
            // `el.voriges_geschwister().is_none()`. An einem Knoten OHNE
            // Eltern gibt `voriges_geschwister()` `None` zurueck —
            // damit war `html:first-child`, `html:last-child` und
            // `html:only-child` WAHR. Nach Selectors Level 4, 4.3 ist
            // aber nur ein Element ein Kind, wenn es ein ElTERN-Element
            // hat: "an element that has no parent is not a child".
            // Die drei Bedingungen pruefen den Eltern jetzt mit.
            Pseudo::ErstKind => el.eltern().is_some() && el.voriges_geschwister().is_none(),
            Pseudo::LetztKind => el.eltern().is_some() && el.naechstes_geschwister().is_none(),
            Pseudo::EinzigKind => {
                el.eltern().is_some()
                    && el.voriges_geschwister().is_none()
                    && el.naechstes_geschwister().is_none()
            }
            Pseudo::Leer => el.ist_leer(),
            Pseudo::Link => {
                crate::kaskade::profil(crate::kaskade::pz::TAG);
                crate::kaskade::profil(crate::kaskade::pz::ATTRIBUT);
                // Release 2026-10-15: `<area href>` too (HTML §4.16.3).
                crate::form_state::is_link(el)
            }
            Pseudo::Besucht => false,
            Pseudo::Schweber => zustand.hover,
            // Release 11.10.2026: auch der Fokus aus `el.focus()` (internes
            // Attribut, siehe `Dokument::css_zustand` in narsil-fmt-html).
            Pseudo::Fokus => zustand.focus || el.attribut(" narsil-focus").is_some(),
            // K12 (CSS Selectors 4 §6). Die Zahlen stehen an den
            // Varianten in `typen.rs` und in
            // crates/narsil-css/tests/k12_zustaende.rs.
            Pseudo::Aktiv => zustand.aktiv,
            Pseudo::FocusSichtbar => zustand.focus_sichtbar,
            Pseudo::PopoverOffen => el.attribut(" narsil-popover-open").is_some(),
            Pseudo::Modal => el.attribut(" narsil-modal").is_some(),
            Pseudo::Ziel => zustand.ziel,
            // Release 11.10.2026: die „checkedness" aus dem Host (interne
            // Attribute) oder das Attribut selbst. GEMESSEN vorher: `:checked`
            // traf nie (der Zustand wurde nirgends gesetzt).
            // Release 2026-10-15: the form pseudo-classes come from the
            // element tree (`crate::form_state`); the host's `Zustand`
            // flags still force them.
            Pseudo::Ausgewaehlt => zustand.ausgewaehlt || crate::form_state::is_checked(el),
            Pseudo::Gesperrt => zustand.gesperrt || crate::form_state::is_disabled(el),
            Pseudo::Aktiviert => !zustand.gesperrt && (zustand.aktiviert || crate::form_state::is_enabled(el)),
            Pseudo::Unbestimmt => zustand.unbestimmt || crate::form_state::is_indeterminate(el),
            Pseudo::NurLesen => zustand.nur_lesen || !crate::form_state::is_read_write(el),
            // §6.3.5: `:read-write` ist „nicht read-only".
            Pseudo::Lesbar => !zustand.nur_lesen && crate::form_state::is_read_write(el),
            Pseudo::PlatzhalterSichtbar => crate::form_state::is_placeholder_shown(el),
            Pseudo::Required => crate::form_state::is_required(el),
            Pseudo::Optional => crate::form_state::is_optional(el),
            Pseudo::Valid => crate::form_state::matches_validity(el, true),
            Pseudo::Invalid => crate::form_state::matches_validity(el, false),
            Pseudo::InRange => crate::form_state::matches_range(el, true),
            Pseudo::OutOfRange => crate::form_state::matches_range(el, false),
            Pseudo::Default => crate::form_state::is_default(el),
            Pseudo::Open => crate::form_state::is_open(el),
            Pseudo::Heading(stufen) => crate::form_state::heading_level(el).is_some_and(|l| stufen.is_empty() || stufen.contains(&l)),
            // §6.3.7: erstes/letztes Geschwister DESSELBEN Typs. Die
            // Pruefung braucht den Geschwisterweg aus `Element`
            // (`voriges_geschwister`) — nach unten gibt es keinen Weg,
            // aber der wird auch nicht gebraucht.
            Pseudo::ErstVonTyp => !hat_geschwister_davor_mit_tag(el, el.tag()),
            Pseudo::LetztVonTyp => !hat_geschwister_danach_mit_tag(el, el.tag()),
            // §6.3.7: einziger seines Typs. Beide Richtungen muessen
            // leer sein.
            Pseudo::Sprache(tags) => {
                // `:lang(*)` ist als LEERE Liste gespeichert: „passt auf
                // jede gesetzte Sprache". Der Unterschied zu „gar kein
                // `:lang()`" ist genau der `lang`-Attribut-Check
                // darunter — ein Element ohne `lang` passt auch auf
                // `:lang(*)` nicht.
                // Selectors Level 4 §6.5.17 / RFC 4647 §3.3.1
                // („basic filtering"): `:lang(de)` passt auf `de` UND
                // auf `de-AT`, `de-CH`, `de-DE` — aber nicht auf `del`
                // und nicht auf `de` in `xxde`.
                //
                // GEMESSENE ENTSCHEIDUNG: der Vergleich ist hier
                // handgeschrieben, obwohl `AttrOp::Sprache` (also
                // `[lang|=de]`) dieselbe Frage stellt. Wiederverenden
                // ging nicht, weil dort der Attributname fest ist und
                // hier nicht — und weil der Attributpfad pro Element
                // einen anderen Namen traegt.
                //
                // Ohne `lang`-Attribut passt NICHTS: ein Element ohne
                // Sprache hat keine Sprache, und `:lang(ar)` auf ein
                // sprachloses Element wahr zu melden waere geraten.
                // Release 11.10.2026: die Sprache ERBT (HTML §3.2.6.2: das
                // naechste `lang`/`xml:lang` am Element oder einem
                // Vorfahren). GEMESSEN vorher: `<html lang=en>` und
                // `div:lang(en)` passten nicht (WPT css3-selectors-lang-*).
                let mut lang_quelle: Option<String> =
                    el.attribut("xml:lang").or_else(|| el.attribut("lang")).map(|v| v.to_string());
                let mut x = if lang_quelle.is_none() { el.eltern() } else { None };
                let mut schritte = 0;
                while let Some(e) = x {
                    if let Some(v) = e.attribut("xml:lang").or_else(|| e.attribut("lang")) {
                        lang_quelle = Some(v.to_string());
                        break;
                    }
                    schritte += 1;
                    if schritte > 512 {
                        break;
                    }
                    x = e.eltern();
                }
                let Some(lang) = lang_quelle else {
                    return false;
                };
                let lang = lang.trim();
                tags.is_empty()
                    || tags.iter().any(|t| {
                        let t = t.as_str();
                        !t.is_empty()
                            && (lang.eq_ignore_ascii_case(t)
                                || (lang.len() > t.len()
                                    && lang.as_bytes()[t.len()] == b'-'
                                    && lang[..t.len()].eq_ignore_ascii_case(t)))
                    })
            }
            Pseudo::Richtung(want) => want.is_some_and(|w| directionality(el) == w),
            Pseudo::EinzigVonTyp => {
                !hat_geschwister_davor_mit_tag(el, el.tag())
                    && !hat_geschwister_danach_mit_tag(el, el.tag())
            }
            Pseudo::Wurzel => el.eltern().is_none(),
            Pseudo::ScopeIn(roots, limits) => crate::scope_match::in_scope(el, roots, limits, zustand),
            Pseudo::ScopeAnchor => crate::scope_match::is_anchor(el),
            Pseudo::ClassPrefix(prefix) => el.attribut("class").is_some_and(|c| c.split_ascii_whitespace().any(|k| k.starts_with(prefix.as_str()))),
            Pseudo::Nicht(innen) => !innen.iter().any(|s| selektor_passt_mit(s, el, zustand)),
            // Release 12.10.2026: nur in einem Shadow-Root-Blatt sinnvoll (dort
            // umgeschrieben, siehe `Pseudo::Wirt`); sonst trifft es nie.
            Pseudo::Wirt(_) | Pseudo::Eingesetzt(_) => false,
            Pseudo::Ist(innen) | Pseudo::Wo(innen) => {
                innen.iter().any(|s| selektor_passt_mit(s, el, zustand))
            }
            Pseudo::FocusWithin => {
                // Selectors 4 §6.1.4. Die Entscheidung liegt im
                // Zustand, weil der Host sie beim Bau des Baums trifft:
                // er geht von der Wurzel nach unten und weiss, ob unter
                // diesem Knoten ein fokussiertes Element haengt.
                //
                // GEMESSENE ENTSCHEIDUNG, warum hier KEIN Lauf durch die
                // Nachfahren steht: `Zustand` traegt nur ein `focus:
                // bool` fuer DIESES Element. Ein Vorfahre kann daraus
                // nicht ablesen, dass ein Nachfahre den Fokus hat — die
                // Information fehlt, und ein Lauf durch den Teilbaum
                // je Element und je Kaskadendurchlauf waere der
                // quadratische Weg (und genau den hat `narsil` bei
                // `hat_klasse` gemessen: +53 - 59 us je Knoten).
                zustand.focus_within
            }
            Pseudo::Hat(innen) => {
                // GEMESSENE FEHLER 30.09.2026 (R0 c), zwei Stueck:
                //
                // (1) Die Kombinatoren IM Innern wurden ignoriert. Der
                //     Parser hat `:has(> a)` sogar VERWORFEN (eine
                //     fuehrende `>` gilt im normalen Selektor als
                //     "Kombinator ohne linken Partner"). Damit war die
                //     haeufigste Form von `:has()` auf der Welt
                //     (`a:has(> img)`, `p:has(> .fehler)`) nicht
                //     benutzbar — sie wurde verworfen, und die Seite
                //     bekam die Regel nicht.
                //
                // (2) Die Rekursion hatte eine Tiefengrenze von 10
                //     statt einer Mengengrenze. Bei 50 000 Knoten unter
                //     dem Wurzelknoten las `:has(div)` 50 000 Knoten —
                //     fuer jeden Knoten, fuer den die Kaskade laeuft.
                //
                // Beides ist jetzt `relativ_passt_mit` weiter unten:
                // eine Ebenen-fuer-Ebene-Menge statt einer Rekursion,
                // mit `MAX_HAS_KANDIDATEN` und `MAX_HAS_BESUCHE`.
                innen.iter().any(|s| relativ_passt_mit(s, el, zustand))
            }
            Pseudo::NtesKind(a, b, of_liste) | Pseudo::NtesLetztesKind(a, b, of_liste) => {
                let is_last = matches!(pseudo, Pseudo::NtesLetztesKind(..));
                let mut match_count = 0i64;

                // Das Element selbst zaehlt mit, wenn es den `of`-Filter
                // besteht (CSS Selectors 4, §6.3: der Filter entscheidet,
                // ob ueberhaupt gezaehlt wird).
                // Release 13.10.2026 (Agent C): if it fails the filter, the
                // pseudo-class does not match (Selectors 4 §14.4: "elements
                // that match S"). MEASURED before (WPT css-cascade
                // scope-invalidation ":nth-child() in scope root"): the
                // second element without `.a` matched `:nth-child(2n of .a)`.
                if of_liste.is_empty()
                    || of_liste.iter().any(|s| selektor_passt_mit(s, el, zustand))
                {
                    match_count += 1;
                } else {
                    return false;
                }

                let mut current = if is_last {
                    el.naechstes_geschwister()
                } else {
                    el.voriges_geschwister()
                };
                // Der allererste Aufruf zaehlt mit — sonst waere die
                // Grenze um eins zu gross (GEMESSEN: 513 statt 512).
                let mut gescannt = 1usize;
                // M3 — DIE GRENZE, DIE GEFEHLT HAT.
                //
                // GEMESSENER FEHLER 01.10.2026 (Koordination, P3,
                // crates/narsil-probe/tests/diag_geschwister.rs): diese
                // Schleife war `while let Some(c) = current` OHNE
                // Abbruchbedingung. Sie endet nur, wenn
                // `voriges_geschwister()` zurueckgibt — bei einem Baum
                // mit zyklischem Geschwister-Zug (was `narsil-page`
                // erzeugen kann) endet sie NIE, und `match_count += 1`
                // laeuft in einen `add overflow`-Panic (Debug) bzw. in
                // eine endlose Schleife bis zur 30-s-Grenze (Release).
                // Gemessen auf github.com und nodejs.org.
                //
                // Dieselbe Zahl wie bei `~` (MAX_GESCHWISTER_SCAN): eine
                // Liste mit mehr als 512 Eintraegen davor ist
                // pathologisch, danach gilt "kein Treffer".
                while let Some(c) = current {
                    if gescannt >= MAX_GESCHWISTER_SCAN {
                        break;
                    }
                    gescannt += 1;
                    if of_liste.is_empty()
                        || of_liste.iter().any(|s| selektor_passt_mit(s, &c, zustand))
                    {
                        match_count += 1;
                    }
                    current = if is_last {
                        c.naechstes_geschwister()
                    } else {
                        c.voriges_geschwister()
                    };
                }
                ntes_trifft(*a, *b, match_count)
            }
            Pseudo::NtesTyp(a, b) => {
                let tag = el.tag();
                let mut match_count = 1i64;
                let mut current = el.voriges_geschwister();
                // M3: dieselbe Grenze und derselbe Grund wie in
                // `NtesKind` oben — der Zaehler lief auf github.com und
                // nodejs.org endlos. Der erste Aufruf zaehlt mit.
                let mut gescannt = 1usize;
                while let Some(c) = current {
                    if gescannt >= MAX_GESCHWISTER_SCAN {
                        break;
                    }
                    gescannt += 1;
                    if c.tag() == tag {
                        match_count += 1;
                    }
                    current = c.voriges_geschwister();
                }
                ntes_trifft(*a, *b, match_count)
            }
            Pseudo::NtesLetztesTyp(a, b) => {
                // Die Umkehrung: `NtesTyp` zaehlt von vorn, dieser
                // zaehlt von hinten. Beides ist eine Frage nach
                // `an+b` — nur mit anderem `n`.
                //
                // GEMESSENE ENTSCHEIDUNG, warum ZWEI Wege gelaufen
                // werden und nicht einer: die Gesamtzahl der
                // gleichartigen Geschwister steht nirgends. Sie zu
                // bekommen heisst, nach VORNE zu zaehlen. Der Rueckweg
                // allein liefert nur "die Position von hinten", und
                // `:nth-last-of-type(1)` ist nicht "Position von
                // hinten = 1" ohne die Gesamtzahl.
                let tag = el.tag();
                // GEMESSENER FEHLER MEINER ERSTEN FASSUNG, gefunden von
                // `tests/c1_nth_last_of_type.rs`: ich habe die
                // VORGAENGER gezaehlt und das „von hinten" genannt.
                // Das ist die Position von VORN — das letzte `div` in
                // `p, div, p, div` hat einen `div`-Vorgaenger, kam also
                // auf 2 statt auf 1, und `:nth-last-of-type(1)` traf es
                // nicht.
                //
                // GEMESSENE ENTSCHEIDUNG, warum NUR der Vorwaerzslauf
                // noetig ist: die Position von hinten ist
                // `(Gesamtzahl - Position von vorn + 1)`, und
                // `Gesamtzahl = vorgaenger + nachfolger`. Der Ausdruck
                // kuerzt sich zu `nachfolger + 1` — die Vorgaenger
                // fallen heraus. Das ist nicht nur kuerzer, es ist auch
                // die Haelfte des Speicherzugriffs.
                let mut von_hinten = 1i64;
                let mut nach = el.naechstes_geschwister();
                // M3: dieselbe Grenze und derselbe Grund wie in
                // `NtesTyp` — der Zaehler lief auf github.com und
                // nodejs.org endlos. Der erste Aufruf zaehlt mit.
                let mut gescannt = 1usize;
                while let Some(c) = nach {
                    if gescannt >= MAX_GESCHWISTER_SCAN {
                        break;
                    }
                    gescannt += 1;
                    if c.tag() == tag {
                        von_hinten += 1;
                    }
                    nach = c.naechstes_geschwister();
                }
                ntes_trifft(*a, *b, von_hinten)
            }
            // 06.10.2026: PSEUDOELEMENTE werden hier NIE erfuellt — die
            // Regel gehoert einer eigenen Box (siehe `Pseudo::Vorher`).
            // Ohne diese Arme waere der Match unvollstaendig (E0004).
            Pseudo::Vorher | Pseudo::Nachher | Pseudo::Platzhalter | Pseudo::ErsterBuchstabe | Pseudo::ErsteZeile => {
                false
            }
        };
        if !ok {
            return false;
        }
    }
    true
}
