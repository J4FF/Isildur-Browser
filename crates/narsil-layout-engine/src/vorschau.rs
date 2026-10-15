// P9, 07:50 — die ZEILENVORSCHAU.
//
// WARUM EIGENTLICH: der Inline-Lauf richtet die Teile einer Zeile sofort
// aus, und die Grundlinie der Zeile ist das GROSSTE Grundlinien-Mass der
// Teile (CSS Inline 3, §2.3: erst die Grundlinie der Zeile bestimmen,
// dann ausrichten). Wer nur die BISHER gesetzten Teile sieht, kann einen
// bereits gesetzten nicht mehr heben.
//
// GEMESSENER MANGEL (07:20): `<img>` (10 px, inline-block) VOR
// `<span>` (16 px Text) — beide standen bei y = 0. Mit der umgekehrten
// Reihenfolge stand der Kasten bei 1,4285717 px. Die Zeile war 16 px
// hoch in beiden Faellen; nur der Kasten stand anders.
//
// DER WEG, DEN ICH HEUTE GEHE, IST DER KURZE: die Zeile wird EINMAL
// VORAUSGESEHEN, wenn sie beginnt. Die Vorschau zaehlt die Teile der
// Zeile mit derselben Umbruchregel, mit der der Lauf sie danach setzt —
// damit die Vorschau und der Lauf sich nicht widersprechen koennen.
//
// WARUM NICHT SO, WIE IM VERSUCH UM 07:25 (zurueckgenommen): dort sollte
// die Zeile die Teile SAMMELN und beim Schluss ausrichten. Das hiess,
// Anweisungen in `let p: Platzierung = if ... else if ... else { ... }`
// unterzubringen — ein Ausdruck nimmt keine Anweisung auf, und die
// Bedingung "ist das der lineare Weg?" stand danach an zwei Stellen.
// Die Vorschau braucht keine dieser Aenderungen: sie liest nur.
//
// WAS SIE ZUSAETZLICH LOEST: die Breite einer Folge kollabibler
// Leerzeichen am ZeilenENDE. Die Vorschau weiss, dass das letzte Teil
// der Zeile ein Leerzeichen ist, und die Breite kann dort auf 0
// gesetzt werden — CSS 2.1, 16.6.1. Vorher war das unmoeglich, weil
// beim Setzen noch nicht bekannt war, ob ein Teil folgt.

use crate::Ctx;
use crate::box_sizing::{Sizes, get_node, resolve_spacing};
use crate::flow::{grundlinie_von, inline_kind, kollabierer_leerraum, sauber_pos, vertikal_von};
use narsil_dom::NodeKind;

/// Das Ergebnis einer Vorschau fuer EINEN Zeilenanfang.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Vorschau {
    /// Wie viele Teile die Zeile hat (mindestens 1: das aufrufende).
    pub anzahl: usize,
    /// Grundlinie der Zeile, als Abstand von ihrer Oberkante.
    ///
    /// Es zaehlen nur Teile mit `vertical-align: baseline` (oder ohne
    /// Angabe) — `top`, `middle` und `bottom` richten sich NACH der
    /// Zeile, nicht die Zeile nach ihnen.
    pub grundlinie: f32,
    /// Hoehe der Zeile: das groesste Mass ihrer Teile. Ein
    /// kollabierbares Leerzeichen zaehlt nicht mit.
    pub hoehe: f32,
    /// Ist das LETZTE Teil der Zeile ein kollabierbares Leerzeichen?
    pub letzter_leer: bool,
}

// NOTE: das ERSTE braucht kein eigenes Feld. Ob ein Teil der ERSTE der
// Zeile ist, weiss der Lauf ohne Vorschau: `!zeile.gleich`, es ist noch
// kein Teil gesetzt worden. Genau so heisst es in beiden Aufrufern.

/// P9: sieht die Zeile voraus, die bei `ab` beginnt.
///
/// # DIE UMBRUCHREGEL STEHT HIER UND IN `inline_teil`
///
/// Das ist Absicht: die Vorschau zaehlt die Teile einer Zeile, und der
/// Lauf setzt genau diese Teile. Zwei Kopien derselben Regel sind ein
/// Fehler, den niemand sieht — bis eine Seite anders aussieht als die
/// Tests. `tests/inline_lauf.rs` haelt beide aneinander: die Zeilen der
/// Tests entstehen aus dieser Vorschau.
///
/// # WAS NICHT VORKOMMEN KANN
///
/// * Ein Block: die Zeile endet vorher (CSS Inline 3, §2.1).
/// * Ein Kind, dessen Groesse nicht bekannt ist: `sizes.get` liefert
///   `None` fuer einen Knoten, den die Messphase nicht gesehen hat.
///   Dann endet die Vorschau, und der Lauf behandelt das Kind als
///   Block. Beide Wege nehmen denselben Knoten also gleich — die
///   Vorschau ist eine Vorhersage, keine Wahrheit ueber den Knoten.
/// * Ein Rand, der sich nicht aufloesen laesst: `resolve_spacing`
///   liefert dann einen Fehler. Die Vorschau bricht ab, statt zu raten.
pub(crate) fn zeilen_vorschau(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    ab: usize,
    sizes: &Sizes,
    line_height: f32,
    content_breite: f32,
    n: usize,
) -> Vorschau {
    // Release 11.10.2026 (Agent CSS2): die Zeile beginnt mit dem Strut des
    // Blocks (siehe `flow::strut`), und ihre Hoehe ist Grundlinie plus der
    // groesste Rest darunter — nicht nur das groesste Teil.
    let (strut_oben, strut_unten) = crate::flow::strut(ctx, node);
    let mut unten = strut_unten;
    let mut v = Vorschau {
        anzahl: 0,
        grundlinie: strut_oben,
        hoehe: 0.0,
        letzter_leer: false,
    };
    let mut braucht = 0.0f32;
    let mut j = ab;
    // Release 15.10.2026 (Agent CSS2): the same glue test as the placement
    // (`flow::glued_to_previous`).
    let mut prev = if ab > 0 { node.children.get(ab - 1).copied() } else { None };
    while j < n {
        let cid = node.children[j];
        let Ok(kind) = get_node(ctx, cid) else { break };
        let cst = ctx.stil_oder_standard(cid);
        if !inline_kind(ctx, cid, &cst, &kind.kind) {
            break;
        }
        let Some(size) = sizes.get(cid) else { break };
        let Ok(rand) = resolve_spacing(kind.style.margin, content_breite, cid, "margin") else {
            break;
        };
        let hoehe = if kind.kind == NodeKind::Text {
            size.height.max(line_height)
        } else {
            size.height
        };
        let kollabiert = kollabierer_leerraum(ctx, cid);
        // CSS 2.1, 16.6.1: ein fuehrendes kollabierbares Leerzeichen
        // belegt keinen Platz.
        let breite = if kollabiert && v.anzahl == 0 {
            0.0
        } else {
            sauber_pos(size.width)
        };
        let mit_rand = rand.left + breite + rand.right;
        if v.anzahl > 0 && braucht + mit_rand > content_breite && !crate::flow::glued_to_previous(ctx, prev, cid) {
            break;
        }
        prev = Some(cid);
        braucht += mit_rand;
        if !kollabiert {
            // Release 11.10.2026 (Agent CSS2): mit den senkrechten Raendern
            // atomarer Teile (`flow::teil_mit_rand`).
            let (h, g) = crate::flow::teil_mit_rand(ctx, cid, &kind.kind, hoehe, rand.top, rand.bottom);
            if vertikal_von(ctx, cid) == narsil_layout::Vertikal::Grundlinie {
                v.grundlinie = v.grundlinie.max(g);
                unten = unten.max(h - g);
            }
            v.hoehe = v.hoehe.max(h);
        }
        v.letzter_leer = kollabiert;
        v.anzahl += 1;
        j += 1;
    }
    v.hoehe = v.hoehe.max(v.grundlinie + unten);
    if v.anzahl == 0 {
        // Sollte nicht vorkommen (der aufrufende Knoten ist Inline), aber
        // eine Vorschau ohne Teil waere eine Division durch nichts an
        // anderer Stelle. Sie zaehlt als ein leeres Teil.
        v.anzahl = 1;
    }
    v
}
