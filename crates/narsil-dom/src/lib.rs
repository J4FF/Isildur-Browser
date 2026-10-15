//! Dokumentbaum der Narsil-Sprache (Document Tree).
//!
//! Ergebnis des Parsens, Eingabe der Layout-Phase.
//!
//! Speicher-Modell: flache Arena. Jeder Knoten liegt in einem `Vec<Node>` und
//! wird ueber eine `NodeId` referenziert. Es gibt bewusst KEIN `Rc`/`RefCell`
//! und keine Zyklenmoeglichkeit. Das haelt die Struktur klonbar, serialisierbar
//! und im Layout-Renderer frei von Zyklen.
//!
//! Invariante: Es gibt hoechstens EINEN Knoten mit `parent == None`, und das ist
//! der Root (ein `Page`-Knoten). Das haelt die Baummorphologie garantiert.

use narsil_core::style::Style;

/// Stabiler Index in die Arena des Dokuments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Index fuer Array-Zugriff. Nur gueltig, wenn der Knoten existiert.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Position im Quelltext, fuer spaetere Fehlermeldungen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    /// Byte-Offset des Beginns.
    pub start: u32,
    /// Laenge in Bytes.
    pub len: u32,
}

impl Span {
    pub const EMPTY: Span = Span { start: 0, len: 0 };

    pub const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    /// Ende als exklusiver Offset.
    pub const fn end(self) -> u32 {
        self.start + self.len
    }
}

/// Eine Stelle im Quelltext, auf die man SPRINGEN kann.
///
/// GEMESSEN, warum es sie geben muss (ABNAHME_SPRACHE.txt, Abschnitt 2):
/// `LexError` und `ParseError` tragen nur `message` und `span`, und
/// `span` ist ein BYTE-Offset. "Byte 239" ist fuer einen Menschen, der
/// eine Datei bearbeitet, keine Position: er muss selbst zaehlen. Die
/// Byte-Offsets bleiben gueltig — es kommt etwas dazu, es faellt nichts
/// weg.
///
/// BEIDE ZAHLEN SIND 1-BASIERT, und das ist eine Entscheidung: die
/// Fehlermeldung eines Editors sagt "Zeile 1, Spalte 1" fuer das erste
/// Zeichen, nicht "Zeile 0". Ein 0-basierter Wert muesste in jeder
/// Anzeige umgerechnet werden, und genau daran vergisst man sich.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    /// 1-basiert. Zeile 1 ist die erste.
    pub zeile: u32,
    /// 1-basiert, und in ZEICHEN, nicht in Bytes.
    ///
    /// GEMESSEN, warum in Zeichen: `"ä"` ist 2 Bytes und 1 Zeichen. Bei
    /// einem Byte-Zaehler zeigt ein Fehler hinter "ä" auf Spalte 5 statt
    /// auf Spalte 4 — und der Cursor landet neben dem Zeichen statt
    /// darauf. Fuer eine Fehlermeldung ist das ein Unterschied zwischen
    /// "da ist es" und "da ist es nicht".
    pub spalte: u32,
    /// Der Byte-Offset, aus dem alles berechnet wurde.
    ///
    /// STEHT MIT DRIN, weil die Umkehrung nicht geht: aus (Zeile, Spalte)
    /// laesst sich der Byte-Offset nur mit dem Quelltext bestimmen, und
    /// der ist nicht Teil dieses Wertes.
    pub byte: u32,
}

impl Position {
    /// Die Position als Text, so wie sie in einer Meldung steht.
    ///
    /// "Zeile 12, Spalte 3 (Byte 239)" — beides, weil beides gebraucht
    /// wird: der Mensch springt nach Zeile/Spalte, das Programm rechnet
    /// mit dem Byte.
    pub fn als_text(&self) -> String {
        format!(
            "Zeile {}, Spalte {} (Byte {})",
            self.zeile, self.spalte, self.byte
        )
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Zeile {}, Spalte {} (Byte {})",
            self.zeile, self.spalte, self.byte
        )
    }
}

/// Rechnet einen Byte-Offset in eine [`Position`] um.
///
/// Das ist die EINE Stelle im ganzen Workspace, an der aus einem Offset
/// eine Zeile wird. Es gibt hier keine zweite.
///
/// # REGELN, und jede ist gemessen
///
/// 1. **Zeile** = Anzahl der `\n` VOR dem Offset, plus 1.
/// 2. **Spalte in Zeichen** = Zeichen seit dem letzten `\n`, plus 1.
/// 3. **Offset hinter dem letzten Zeichen**: es wird nicht auf das letzte
///    Zeichen GEZOGEN, sondern der Offset zeigt HINTER die Datei. Fuer
///    `"abc\n"` mit Offset 4 ist das Zeile 2, Spalte 1 — so zeigen es
///    die Editoren, und so sucht man einen fehlenden Endzucker.
///    GEMESSENER FEHLER 29.09.2026: die erste Fassung zog auf das letzte
///    Zeichen zurueck und meldete fuer `"\n"` mit Offset 1 "Zeile 1,
///    Spalte 1". Richtig ist Zeile 2: der Umbruch ist passiert.
/// 4. **Byte-Offset mitten in einem UTF-8-Zeichen**: es wird auf den
///    ZeichenANFANG zurueckgerundet. Grund: der Fehler wurde vom Lexer
///    an einer Zeichengrenze gefunden, und wenn der Aufrufer einen
///    halben Offsetuebergibt, ist Runden nach unten besser als ein
///    Spaltenwert, der auf kein Zeichen zeigt.
/// 5. **Leere Quelle**: Zeile 1, Spalte 1. Es gibt nichts anderes, und
///    (1,1) ist der Ort, an dem man anfangen wuerde.
pub fn position(quelle: &str, byte: u32) -> Position {
    if quelle.is_empty() {
        return Position {
            zeile: 1,
            spalte: 1,
            byte: 0,
        };
    }

    let bytes = quelle.as_bytes();
    let gewuenscht = byte as usize;

    // FAELL 1 (Regel 3): der Offset liegt HINTER dem letzten Zeichen.
    // Dann zeigt er auf das Ende, und es wird nichts zurueckgezogen.
    if gewuenscht >= bytes.len() {
        let zeile = 1 + bytes.iter().filter(|c| **c == b'\n').count() as u32;
        let letzte_nl = quelle.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let spalte = 1 + quelle[letzte_nl..].chars().count() as u32;
        return Position {
            zeile,
            spalte,
            byte: bytes.len() as u32,
        };
    }

    // FAELL 2: der Offset zeigt auf einen Zeichenanfang.
    let b = gewuenscht;

    // FAELL 3 (Regel 4): mitten in einem Zeichen. Gesucht wird
    // RUECKWAERTS, weil der gueltige Fall sofort passt.
    let b = if (bytes[b] & 0xC0) == 0x80 {
        let mut rueck = b;
        while rueck > 0 && (bytes[rueck] & 0xC0) == 0x80 {
            rueck -= 1;
        }
        rueck
    } else {
        b
    };

    // Vor dem Offset: Zeilen zaehlen.
    let prefix = &quelle[..b];
    let zeile = 1 + prefix.bytes().filter(|c| *c == b'\n').count() as u32;

    // Spalte: Zeichen seit dem letzten Zeilenumbruch.
    let letzte_nl = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let spalte = 1 + quelle[letzte_nl..b].chars().count() as u32;

    Position {
        zeile,
        spalte,
        byte: b as u32,
    }
}

/// Die INTRINSISCHE Groesse eines ERSETZTEN Elements — CSS 2.1, 10.3.2.
///
/// # WAS "ERSETZT" HEISST
///
/// CSS 2.1, 3.2: "replaced elements are those whose content comes from an
/// external source" — `<img>`, `<svg>`, `<video>`, `<iframe>`. Sie
/// bekommen ihre Groesse NICHT aus ihren Kindern (sie haben keine), und
/// auch nicht aus einer Prozentangabe, sondern aus dem Verhaeltnis ihrer
/// eigenen natuerlichen Masse.
///
/// # WARUM DIE MASSE HIER UND NICHT IM STIL
///
/// `Style` traegt die ENTSCHIEDENE Groesse — das, was der Autor
/// geschrieben hat. Die intrinsische ist eine EIGENSCHAFT des Knotens,
/// keine Angabe: sie ist da, ob oder ob jemand etwas geschrieben hat, und
/// sie aendert sich nicht, wenn `width` auf `auto` steht. Ein `Style` ist
/// dafuer nicht das richtige Feld.
///
/// `None` in einem der beiden Felder heisst: diese Masse ist nicht
/// bekannt (nicht geladen, kein Attribut, kein `viewBox`). CSS 2.1, 10.3.2
/// regelt den Fall dann mit dem, was bekannt ist.
///
/// # GRENZEN
///
/// `breite`/`hoehe` kommen aus den HTML-Attributen `width`/`height`, und
/// die sind laut HTML nur PRAESENTATIVE HINWEISE (CSS 2.1, 6.4.4) — was
/// CSS schreibt, gewinnt. Fuer `<img>` ist der Attributwert zugleich die
/// beste vorlaeufige natuerliche Masse, die es ohne Decoder gibt; steht
/// das Bild spaeter wirklich da, ist `narsil::bilder` die bessere Quelle.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ErsatzMasse {
    /// Natuerliche Breite in px, `None` = unbekannt.
    pub breite: Option<f32>,
    /// Natuerliche Hoehe in px, `None` = unbekannt.
    pub hoehe: Option<f32>,
}

impl ErsatzMasse {
    /// Beide Masse bekannt? Nur dann ist das VERHAELTNIS berechenbar.
    pub fn ist_vollstaendig(&self) -> bool {
        matches!((self.breite, self.hoehe), (Some(b), Some(h)) if b > 0.0 && h > 0.0)
    }

    /// Das Seitenverhaeltnis (breite / hoehe), oder `None`.
    ///
    /// CSS 2.1, 10.3.2 rechnet in diesem Verhaeltnis. Ist nur eine Seite
    /// bekannt, ergibt es `None` — die Frage ist dann nicht zu
    /// beantworten, und eine Zahl zu raten waere schlimmer als keine.
    pub fn verhaeltnis(&self) -> Option<f32> {
        if self.ist_vollstaendig() {
            Some(self.breite.unwrap_or(0.0) / self.hoehe.unwrap_or(1.0))
        } else {
            None
        }
    }
}

/// Art eines Knotens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// Wurzel des Dokuments.
    Page,
    /// Container mit Kindern.
    Container,
    /// Blatt mit Textinhalt.
    Text,
}

impl NodeKind {
    /// Name wie in der Sprache geschrieben.
    pub const fn tag_name(self) -> &'static str {
        match self {
            NodeKind::Page => "page",
            NodeKind::Container => "container",
            NodeKind::Text => "text",
        }
    }

    /// Ob dieser Knoten Textinhalt traegt.
    pub const fn is_text(self) -> bool {
        matches!(self, NodeKind::Text)
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag_name())
    }
}

/// Ein Knoten im Dokumentbaum.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Deklarierter Stil aus dem Dokument.
    pub style: Style,
    /// Textinhalt, nur bei `NodeKind::Text` gesetzt.
    pub text: Option<String>,
    /// Kind-Knoten in Dokumentreihenfolge.
    pub children: Vec<NodeId>,
    /// Eltern-Knoten, `None` nur beim Root.
    pub parent: Option<NodeId>,
    /// Quelltext-Position fuer Diagnosezwecke.
    pub span: Span,
    /// Ist das ein ERSETZTES Element, und wie gross ist es von Natur aus?
    ///
    /// `None` = kein ersetztes Element (der Fall fuer alle Knoten, die
    /// keine Bilder sind), und damit genau das alte Verhalten.
    ///
    /// GEMESSENER FEHLER, den dieses Feld behebt (02.10.2026, P6):
    /// `https://www.nixos.org` liefert
    ///     <img src="/_astro/nixos-logo-...svg" width="7129"
    ///          height="1995" class="h-12 w-auto">
    /// Die Engine hatte keine Moeglichkeit zu wissen, dass das ein Bild
    /// ist, und rechnete 7129 px. Nach CSS 2.1, 10.3.2 ergibt
    /// `width: auto` bei gesetzter Hoehe die Breite aus dem Verhaeltnis:
    /// 7129 * (48 / 1995) = 171 px.
    ///
    /// `NodeKind` konnte das nicht tragen: es kennt nur Page, Container
    /// und Text, und ein Container ist bei uns ein Block. Das Feld ist
    /// additiv — `Node::new` setzt `None`.
    pub ersetzt: Option<ErsatzMasse>,
}

impl Node {
    /// Konstruiert einen Knoten mit Default-Stil.
    pub fn new(id: NodeId, kind: NodeKind) -> Self {
        Self {
            id,
            kind,
            style: Style::default(),
            text: None,
            children: Vec::new(),
            parent: None,
            span: Span::EMPTY,
            ersetzt: None,
        }
    }

    /// Merkt sich, dass dieser Knoten ein ERSETZTES Element ist (P6).
    ///
    /// Additiv wie `set_style`: keine bestehende Signatur aendert sich,
    /// und wer es nicht benutzt, sieht `None` und das alte Verhalten.
    pub fn set_ersetzt(&mut self, masse: ErsatzMasse) -> Result<(), DomError> {
        self.ersetzt = Some(masse);
        Ok(())
    }
}

/// Fehler beim Manipulieren des Dokumentbaums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomError {
    /// Knoten-Index existiert nicht.
    UnknownNode(NodeId),
    /// Der Dokumenteinstieg ist kein `Page`.
    InvalidRootKind(NodeId, NodeKind),
    /// Ein `Text`-Knoten muss Inhalt haben, andere duerfen keinen haben.
    InvalidTextContent(NodeId, NodeKind),
    /// Ein Kind ohne Root kann nicht erzeugt werden.
    MissingRoot,
}

impl std::fmt::Display for DomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomError::UnknownNode(id) => write!(f, "unbekannter Knoten {id:?}"),
            DomError::InvalidRootKind(id, k) => {
                write!(f, "Root {id:?} muss 'page' sein, ist aber '{k}'")
            }
            DomError::InvalidTextContent(id, k) => {
                write!(f, "Knoten {id:?} ('{k}') hat ungueltigen Textinhalt")
            }
            DomError::MissingRoot => write!(f, "Dokument hat keinen 'page'-Root"),
        }
    }
}

impl std::error::Error for DomError {}

/// Der Dokumentbaum: flache Arena plus Root-Zeiger.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    nodes: Vec<Node>,
    root: NodeId,
}

impl Document {
    /// Erzeugt ein leeres Dokument mit einem `page`-Root ohne Kinder.
    pub fn new() -> Self {
        let root = Node::new(NodeId(0), NodeKind::Page);
        Self {
            nodes: vec![root],
            root: NodeId(0),
        }
    }

    /// Erzeugt ein Dokument mit `page`-Root und deklariertem Stil.
    pub fn with_root_style(style: Style) -> Self {
        let mut root = Node::new(NodeId(0), NodeKind::Page);
        root.style = style;
        Self {
            nodes: vec![root],
            root: NodeId(0),
        }
    }

    /// Der Root-Knoten (immer `page`).
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Anzahl aller Knoten inklusive Root.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Zugriff auf einen Knoten, `None` wenn unbekannt.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    /// Wie `get`, aber mit `DomError`.
    pub fn node(&self, id: NodeId) -> Result<&Node, DomError> {
        self.get(id).ok_or(DomError::UnknownNode(id))
    }

    /// Alle Knoten der Arena in Indexreihenfolge.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Erzeugt einen neuen Knoten und haengt ihn an `parent`.
    pub fn push_child(
        &mut self,
        parent: NodeId,
        kind: NodeKind,
        text: Option<String>,
        span: Span,
    ) -> Result<NodeId, DomError> {
        let index = self.nodes.len();
        if index > u32::MAX as usize {
            return Err(DomError::UnknownNode(parent));
        }
        let id = NodeId(index as u32);
        let mut node = Node::new(id, kind);
        node.text = text;
        node.span = span;
        node.parent = Some(parent);
        self.nodes.push(node);
        self.nodes[parent.index()].children.push(id);
        Ok(id)
    }

    /// Setzt die Kinderliste eines Knotens neu (Release 07.10.2026, Nachumbruch).
    ///
    /// Jedes Kind in `kinder` bekommt `eltern` als Elternteil; ein bisheriges
    /// Kind, das nicht mehr in der Liste steht, ist danach abgehängt
    /// (`parent == None`) und wird von keiner Baumwanderung mehr erreicht.
    /// Die Arena bleibt unverändert lang — `NodeId`s bleiben gültig.
    pub fn kinder_setzen(&mut self, eltern: NodeId, kinder: Vec<NodeId>) -> Result<(), DomError> {
        self.node(eltern)?;
        for k in &kinder {
            self.node(*k)?;
            if *k == self.root || *k == eltern {
                return Err(DomError::UnknownNode(*k));
            }
        }
        let alt = std::mem::take(&mut self.nodes[eltern.index()].children);
        for k in alt {
            if self.nodes[k.index()].parent == Some(eltern) {
                self.nodes[k.index()].parent = None;
            }
        }
        for k in &kinder {
            if let Some(p) = self.nodes[k.index()].parent
                && p != eltern
            {
                self.nodes[p.index()].children.retain(|c| c != k);
            }
            self.nodes[k.index()].parent = Some(eltern);
        }
        self.nodes[eltern.index()].children = kinder;
        Ok(())
    }

    /// Ersetzt den Inhalt eines Textknotens.
    pub fn set_text(&mut self, id: NodeId, text: String) -> Result<(), DomError> {
        let node = self.get_mut(id)?;
        if !node.kind.is_text() {
            return Err(DomError::InvalidTextContent(id, node.kind));
        }
        node.text = Some(text);
        Ok(())
    }

    /// Setzt den deklarierten Stil eines Knotens.
    pub fn set_style(&mut self, id: NodeId, style: Style) -> Result<(), DomError> {
        self.get_mut(id)?.style = style;
        Ok(())
    }

    /// Setzt die intrinsische Groesse eines ERSETZTEN Elements (P6).
    ///
    /// Das Gegenstueck zu [`Document::set_style`], aus demselben Grund:
    /// der Bauer in `narsil-page` soll eine Frage stellen, nicht am
    /// `Node` herumschreiben.
    pub fn set_ersetzt(&mut self, id: NodeId, masse: ErsatzMasse) -> Result<(), DomError> {
        self.get_mut(id)?.set_ersetzt(masse)
    }

    /// Die intrinsische Groesse eines Knotens, oder `None` (P6).
    pub fn ersatz(&self, id: NodeId) -> Result<Option<ErsatzMasse>, DomError> {
        Ok(self.get_mut_durch(id)?.ersetzt)
    }

    /// Wie `get_mut`, aber auf `&self` (unveraenderlich lesen).
    fn get_mut_durch(&self, id: NodeId) -> Result<&Node, DomError> {
        let idx = id.index();
        if idx >= self.nodes.len() {
            return Err(DomError::UnknownNode(id));
        }
        Ok(&self.nodes[idx])
    }

    /// Setzt die Quelltext-Position eines Knotens nachtraeglich.
    ///
    /// `push_child` nimmt den Span bereits entgegen. Diese Methode ist
    /// fuer den Sonderfall, in dem der Span erst nach dem Einfuegen
    /// feststeht — das ist der Root eines Dokuments: der Parser kennt
    /// ihn, bevor er den ersten Kindknoten anlegt.
    ///
    /// Ohne diese Methode bleibt der Span des Root-Knotens
    /// `Span::EMPTY`, und eine Fehlermeldung, die sich auf den Root
    /// bezieht, kann keine Position nennen. Agent4 hat das zweimal
    /// gemeldet, Agent5 sieht es in `build_display_list`.
    pub fn set_span(&mut self, id: NodeId, span: Span) -> Result<(), DomError> {
        self.get_mut(id)?.span = span;
        Ok(())
    }

    fn get_mut(&mut self, id: NodeId) -> Result<&mut Node, DomError> {
        let idx = id.index();
        if idx >= self.nodes.len() {
            return Err(DomError::UnknownNode(id));
        }
        Ok(&mut self.nodes[idx])
    }

    /// Validiert die STRUKTUR, nicht die Semantik.
    ///
    /// Geprueft wird: Root ist `page`, kein Knoten hat zwei Eltern, alle
    /// `children`-Verweise zeigen auf existierende Knoten, `Text`-Knoten
    /// haben Inhalt, und es gibt hoechstens EINEN Knoten ohne Eltern.
    ///
    /// Zwei Dinge werden hier bewusst NICHT geprueft, weil sie nicht
    /// strukturell sind:
    ///
    /// 1. Ein ZWEITER `page` als Kind. `validate()` zaehlt Knoten mit
    ///    `parent == None` — das ist eine Zusage ueber die Baumform. Ein
    ///    verschachteltes `page` hat Eltern und faellt durch diese Regel
    ///    nicht, ergibt aber `Ok`. Gefunden in TASK_0013A, Agent4,
    ///    Befund 8. Entscheidung: Doku statt Code, weil der PARSER ein
    ///    `page` in einem `page` ablehnt (TASK_0008, Regel E13) und die
    ///    Kette damit geschlossen ist. Einen Datentyp, der etwas darstellen
    ///    kann, das unsinnig ist, dafür abzulehnen wäre eine
    ///    Verhaltensänderung an sechs Aufrufstellen für einen Fall, den
    ///    niemand erzeugen kann.
    ///
    /// 2. Ein `text`-Knoten MIT Kindern. Ebenfalls eine semantische
    ///    Regel, ebenfalls vom Parser durchgesetzt.
    ///
    /// Merksatz: `validate()` sagt "das ist ein Baum". Es sagt nicht
    /// "das ist ein gueltiges Narsil-Dokument". Das ist die Aufgabe von
    /// `narsil_parser`, und die beiden duerfen sich nicht ins Gehege kommen.
    pub fn validate(&self) -> Result<(), DomError> {
        let root = self.node(self.root)?;
        if root.kind != NodeKind::Page {
            return Err(DomError::InvalidRootKind(self.root, root.kind));
        }
        for node in &self.nodes {
            if node.kind.is_text() && node.text.is_none() {
                return Err(DomError::InvalidTextContent(node.id, node.kind));
            }
            if !node.kind.is_text() && node.text.is_some() {
                return Err(DomError::InvalidTextContent(node.id, node.kind));
            }
            for child in &node.children {
                let c = self.node(*child)?;
                if c.parent != Some(node.id) {
                    return Err(DomError::InvalidTextContent(node.id, node.kind));
                }
            }
            if node.id == self.root && node.parent.is_some() {
                return Err(DomError::InvalidTextContent(node.id, node.kind));
            }
        }
        Ok(())
    }

    /// Zaehlt die Knoten einer Art im Dokument.
    pub fn count_of_kind(&self, kind: NodeKind) -> usize {
        self.nodes.iter().filter(|n| n.kind == kind).count()
    }

    /// Iteration ueber alle Knoten in Tiefensuche-Reihenfolge (inklusive Root).
    pub fn iter_depth_first(&self) -> DepthFirstIter<'_> {
        let stack = vec![self.root];
        DepthFirstIter {
            document: self,
            stack,
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// Tiefensuche-Iterator über den Dokumentbaum.
pub struct DepthFirstIter<'a> {
    document: &'a Document,
    stack: Vec<NodeId>,
}

impl<'a> Iterator for DepthFirstIter<'a> {
    type Item = &'a Node;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let node = self.document.get(id)?;
        // Umgekehrte Reihenfolge, damit Kinder in Dokumentreihenfolge besucht
        // werden, da wir vom Ende des Stacks poppen.
        for child in node.children.iter().rev() {
            self.stack.push(*child);
        }
        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_has_page_root() {
        let doc = Document::new();
        assert_eq!(doc.root(), NodeId(0));
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.node(doc.root()).unwrap().kind, NodeKind::Page);
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn push_child_builds_tree() {
        let mut doc = Document::new();
        let c = doc
            .push_child(doc.root(), NodeKind::Container, None, Span::new(10, 5))
            .unwrap();
        let t = doc
            .push_child(c, NodeKind::Text, Some("hi".into()), Span::new(20, 2))
            .unwrap();
        assert_eq!(doc.len(), 3);
        assert_eq!(doc.node(t).unwrap().text.as_deref(), Some("hi"));
        assert_eq!(doc.node(t).unwrap().parent, Some(c));
        assert!(doc.validate().is_ok());
        assert_eq!(doc.count_of_kind(NodeKind::Container), 1);
        assert_eq!(doc.count_of_kind(NodeKind::Text), 1);
    }

    #[test]
    fn text_node_requires_content() {
        let mut doc = Document::new();
        let t = doc
            .push_child(doc.root(), NodeKind::Text, None, Span::EMPTY)
            .unwrap();
        assert!(matches!(
            doc.validate(),
            Err(DomError::InvalidTextContent(_, _))
        ));
        assert_eq!(doc.node(t).unwrap().kind, NodeKind::Text);
    }

    #[test]
    fn set_style_and_with_root_style() {
        let mut doc = Document::with_root_style(Style {
            deckkraft: 1.0,
            radius: [0.0; 4],
            schatten_anzahl: 0,
            dekoration: 0,
            size: narsil_core::length::Dimension::fixed(800.0, 600.0),
            ..Style::default()
        });
        assert_eq!(
            doc.node(doc.root()).unwrap().style.size.width,
            narsil_core::length::Length::Px(800.0)
        );
        let c = doc
            .push_child(doc.root(), NodeKind::Container, None, Span::EMPTY)
            .unwrap();
        doc.set_style(c, Style::default()).unwrap();
        assert!(doc.validate().is_ok());
    }

    #[test]
    fn unknown_node_access_errors() {
        let doc = Document::new();
        assert!(doc.get(NodeId(99)).is_none());
        assert!(matches!(
            doc.node(NodeId(99)),
            Err(DomError::UnknownNode(_))
        ));
    }

    #[test]
    fn depth_first_visits_children_in_order() {
        let mut doc = Document::new();
        let a = doc
            .push_child(doc.root(), NodeKind::Text, Some("a".into()), Span::EMPTY)
            .unwrap();
        let b = doc
            .push_child(doc.root(), NodeKind::Text, Some("b".into()), Span::EMPTY)
            .unwrap();
        let texts: Vec<&str> = doc
            .iter_depth_first()
            .filter(|n| n.kind.is_text())
            .filter_map(|n| n.text.as_deref())
            .collect();
        assert_eq!(texts, vec!["a", "b"]);
        assert_eq!(doc.node(a).unwrap().span, Span::EMPTY);
        assert_eq!(doc.node(b).unwrap().id, b);
    }

    #[test]
    fn span_end_is_exclusive() {
        assert_eq!(Span::new(3, 4).end(), 7);
        assert_eq!(Span::EMPTY.end(), 0);
    }

    #[test]
    fn node_kind_tag_names() {
        assert_eq!(NodeKind::Page.tag_name(), "page");
        assert_eq!(NodeKind::Container.tag_name(), "container");
        assert_eq!(NodeKind::Text.tag_name(), "text");
        assert!(NodeKind::Text.is_text());
        assert!(!NodeKind::Page.is_text());
    }
}
