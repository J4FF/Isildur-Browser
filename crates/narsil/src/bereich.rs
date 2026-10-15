//! Release 12.10.2026: die Host-Seite von `Range`/`StaticRange` (DOM §5).
//!
//! # WARUM
//!
//! GEMESSEN vorher (WPT dom/ranges, Commit 147859dd): 8 von 72 Dateien,
//! 8.829 von 43.906 Teiltests. `Range` war ein Polyfill im Vorspann:
//! `compareBoundaryPoints` gab immer 0, `isPointInRange` immer `false`,
//! `comparePoint` fehlte, kein Grenzpunkt ging bei einer DOM-Aenderung mit,
//! und `createRange` gab es nur am Hauptdokument.
//!
//! # WIE
//!
//! Die Grenzpunkte leben in der Arena ([`narsil_fmt_html::bereich`]); die
//! Mutationsprimitive halten sie aktuell. Der Vorspann (`vorspann.js`, Block
//! „Range / Selection") ruft EINE Host-Funktion `__narsilBereich(op, …)`
//! fuer alles, was den Baum befragt (Baumordnung, Wurzel, Laenge, Grenzen
//! setzen nach DOM §5.4 „set the start or end"). Was den Baum AENDERT
//! (`extractContents`, `insertNode`, `surroundContents`, …), macht der
//! Vorspann ueber die normalen DOM-Methoden — damit gehen Mutation Records,
//! Custom-Element-Reaktionen und eingefuegte Skripte den gewohnten Weg.
//!
//! Fehler kommen als Text mit dem Namen der `DOMException` zurueck
//! (`"IndexSizeError"`), der Vorspann wirft sie.

use crate::lebend::Lebend;
use narsil_fmt_html::bereich::{laenge16, Punkt};
use narsil_fmt_html::dom::{Art, Dokument};
use narsil_jsvm::Wert;
use std::cmp::Ordering;

/// Hoechstens so viele Knoten liefert eine Liste (M3).
const MAX_LISTE: usize = 1 << 20;

/// Die Operationen von `__narsilBereich` (gleich im Vorspann).
mod op {
    pub const NEU: u32 = 0;
    pub const LESEN: u32 = 1;
    pub const GRENZE: u32 = 2;
    pub const ZUSAMMEN: u32 = 3;
    pub const WAEHLEN: u32 = 4;
    pub const SETZEN: u32 = 5;
    pub const VERGLEICH: u32 = 6;
    pub const PUNKT: u32 = 7;
    pub const SCHNEIDET: u32 = 8;
    pub const GEMEINSAM: u32 = 9;
    pub const TEXT: u32 = 10;
    pub const TEILE: u32 = 11;
    pub const LAGE: u32 = 12;
    pub const LAENGE: u32 = 13;
    pub const INDEX: u32 = 14;
    pub const PRUEFEN: u32 = 15;
    pub const TEILWEISE: u32 = 16;
    pub const LOESCHLISTE: u32 = 17;
    pub const WURZEL: u32 = 18;
    pub const SCHATTENWURZEL: u32 = 19;
    pub const KLON: u32 = 20;
}

fn zahl(w: Option<&Wert>) -> u32 {
    match w {
        Some(Wert::Zahl(z)) if z.is_finite() && *z >= 0.0 => (*z).min(f64::from(u32::MAX)) as u32,
        _ => 0,
    }
}

fn wahr(w: Option<&Wert>) -> bool {
    matches!(w, Some(Wert::Wahr(true)))
}

fn fehler(name: &str) -> Wert {
    Wert::Text(name.to_string())
}

/// Der Arena-Knoten zu einem JS-Wert.
fn knoten(l: &Lebend, w: Option<&Wert>) -> Option<usize> {
    w.and_then(narsil_jsvm::host_id).and_then(|h| l.knoten(h))
}

/// DOM §4.2 „length": Doctype 0, Zeichendaten in UTF-16, sonst die Kinder.
#[must_use]
pub fn laenge(dok: &Dokument, k: usize) -> u32 {
    match dok.knoten.get(k).map(|n| &n.art) {
        Some(Art::Doctype { .. }) | None => 0,
        Some(Art::Text(t)) | Some(Art::Kommentar(t)) => laenge16(t),
        Some(Art::Pi { daten, .. }) => laenge16(daten),
        Some(_) => dok.knoten[k].kinder.len() as u32,
    }
}

/// Der Elternknoten im Sinn der DOM (eine Shadow Root hat keinen).
fn eltern(dok: &Dokument, k: usize) -> Option<usize> {
    let n = dok.knoten.get(k)?;
    if matches!(n.art, Art::ShadowRoot { .. }) {
        return None;
    }
    n.eltern
}

fn ist_doctype(dok: &Dokument, k: usize) -> bool {
    matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Doctype { .. }))
}

fn ist_zeichendaten(dok: &Dokument, k: usize) -> bool {
    matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Text(_) | Art::Kommentar(_) | Art::Pi { .. }))
}

fn ist_text(dok: &Dokument, k: usize) -> bool {
    matches!(dok.knoten.get(k).map(|n| &n.art), Some(Art::Text(_)))
}

fn index(dok: &Dokument, k: usize) -> u32 {
    if eltern(dok, k).is_none() {
        return 0;
    }
    dok.index_von(k).unwrap_or(0) as u32
}

/// Die Ahnenkette von der Wurzel bis `k` (einschliesslich).
fn kette(dok: &Dokument, k: usize) -> Vec<usize> {
    let mut v = vec![k];
    let mut x = k;
    while let Some(e) = eltern(dok, x) {
        v.push(e);
        x = e;
        if v.len() > narsil_fmt_html::bereich::MAX_KETTE {
            break;
        }
    }
    v.reverse();
    v
}

/// DOM §5.2 „position of a boundary point" — `a` relativ zu `b` (beide in
/// derselben Wurzel): `Less` = davor, `Greater` = dahinter.
#[must_use]
pub fn lage(dok: &Dokument, a: Punkt, b: Punkt) -> Ordering {
    if a.0 == b.0 {
        return a.1.cmp(&b.1);
    }
    let ka = kette(dok, a.0);
    let kb = kette(dok, b.0);
    let n = ka.iter().zip(kb.iter()).take_while(|(x, y)| x == y).count();
    if n == 0 {
        // Verschiedene Wurzeln: der Aufrufer prueft das vorher.
        return Ordering::Less;
    }
    if n == ka.len() {
        // `a` ist Vorfahr von `b`: das Kind von `a` auf dem Weg zu `b`.
        return if index(dok, kb[n]) < a.1 { Ordering::Greater } else { Ordering::Less };
    }
    if n == kb.len() {
        return if index(dok, ka[n]) < b.1 { Ordering::Less } else { Ordering::Greater };
    }
    if index(dok, ka[n]) < index(dok, kb[n]) { Ordering::Less } else { Ordering::Greater }
}

fn ordnung_wert(o: Ordering) -> Wert {
    Wert::Zahl(match o {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    })
}

fn wurzel(dok: &Dokument, k: usize) -> usize {
    dok.wurzel_von(k)
}

/// Ist `a` ein inklusiver Vorfahr von `b`?
fn inklusiv_vorfahr(dok: &Dokument, a: usize, b: usize) -> bool {
    dok.ist_inklusiver_nachfahre(b, a)
}

/// Der naechste Knoten in Baumordnung, der NICHT in `k` liegt.
fn hinter_teilbaum(dok: &Dokument, k: usize) -> Option<usize> {
    let mut x = k;
    for _ in 0..narsil_fmt_html::bereich::MAX_KETTE {
        if let Some(e) = eltern(dok, x) {
            let kinder = &dok.knoten[e].kinder;
            if let Some(i) = kinder.iter().position(|&c| c == x)
                && let Some(&n) = kinder.get(i + 1)
            {
                return Some(n);
            }
            x = e;
        } else {
            return None;
        }
    }
    None
}

/// Der naechste Knoten in Baumordnung.
fn naechster(dok: &Dokument, k: usize) -> Option<usize> {
    if let Some(&c) = dok.knoten[k].kinder.first() {
        return Some(c);
    }
    hinter_teilbaum(dok, k)
}

/// DOM §5.5 „contained": `k` liegt ganz im Bereich.
fn enthalten(dok: &Dokument, k: usize, s: Punkt, e: Punkt) -> bool {
    wurzel(dok, k) == wurzel(dok, s.0)
        && lage(dok, (k, 0), s) == Ordering::Greater
        && lage(dok, (k, laenge(dok, k)), e) == Ordering::Less
}

fn text_von(dok: &Dokument, k: usize) -> &str {
    match dok.knoten.get(k).map(|n| &n.art) {
        Some(Art::Text(t)) => t,
        _ => "",
    }
}

fn teil16(t: &str, von: u32, bis: u32) -> String {
    let a = narsil_fmt_html::bereich::byte_index16(t, von);
    let b = narsil_fmt_html::bereich::byte_index16(t, bis.max(von));
    t[a..b.max(a)].to_string()
}

/// DOM §5.5 „stringification behavior".
fn als_text(dok: &Dokument, s: Punkt, e: Punkt) -> String {
    if s.0 == e.0 && ist_text(dok, s.0) {
        return teil16(text_von(dok, s.0), s.1, e.1);
    }
    let mut raus = String::new();
    if ist_text(dok, s.0) {
        let t = text_von(dok, s.0);
        raus.push_str(&teil16(t, s.1, laenge16(t)));
    }
    let ende = hinter_teilbaum(dok, e.0);
    let mut n = naechster(dok, s.0);
    let mut schritte = 0usize;
    while let Some(k) = n {
        if Some(k) == ende || schritte > narsil_fmt_html::dom::MAX_KNOTEN {
            break;
        }
        schritte += 1;
        if k != e.0 && ist_text(dok, k) && enthalten(dok, k, s, e) {
            raus.push_str(text_von(dok, k));
        }
        n = naechster(dok, k);
    }
    if ist_text(dok, e.0) {
        raus.push_str(&teil16(text_von(dok, e.0), 0, e.1));
    }
    raus
}

/// DOM §5.5 „extract"/„clone the contents", Schritte 5–11: gemeinsamer
/// Vorfahr, erstes/letztes teilweise enthaltenes Kind, die enthaltenen Kinder
/// und der Punkt, auf den der Bereich danach faellt.
fn teile(l: &mut Lebend, s: Punkt, e: Punkt) -> Wert {
    let dok = &l.dok;
    let mut gemeinsam = s.0;
    for _ in 0..narsil_fmt_html::bereich::MAX_KETTE {
        if inklusiv_vorfahr(dok, gemeinsam, e.0) {
            break;
        }
        match eltern(dok, gemeinsam) {
            Some(p) => gemeinsam = p,
            None => break,
        }
    }
    let ks = kette(dok, s.0);
    let ke = kette(dok, e.0);
    let tiefe = kette(dok, gemeinsam).len();
    let erstes = (!inklusiv_vorfahr(dok, s.0, e.0)).then(|| ks.get(tiefe).copied()).flatten();
    let letztes = (!inklusiv_vorfahr(dok, e.0, s.0)).then(|| ke.get(tiefe).copied()).flatten();
    let kinder = &dok.knoten[gemeinsam].kinder;
    let von = match erstes {
        Some(f) => kinder.iter().position(|&c| c == f).map_or(kinder.len(), |i| i + 1),
        None => s.1 as usize,
    };
    let bis = match letztes {
        Some(f) => kinder.iter().position(|&c| c == f).unwrap_or(0),
        None => e.1 as usize,
    };
    let enthalten: Vec<usize> = kinder.get(von.min(kinder.len())..bis.min(kinder.len()).max(von.min(kinder.len()))).unwrap_or(&[]).to_vec();
    if enthalten.iter().any(|&c| ist_doctype(dok, c)) {
        return fehler("HierarchyRequestError");
    }
    let (neu_k, neu_o) = neuer_punkt(dok, s, e);
    let liste: Vec<Wert> = enthalten.iter().take(MAX_LISTE).map(|&c| l.wert(Some(c))).collect();
    Wert::Feld(vec![
        l.wert(Some(gemeinsam)),
        l.wert(erstes),
        l.wert(letztes),
        Wert::Feld(liste),
        l.wert(Some(neu_k)),
        Wert::Zahl(f64::from(neu_o)),
    ])
}

/// DOM §5.5 „extract" Schritte 10/11 (`deleteContents` 5/6): der Punkt, auf
/// den der Bereich nach dem Herausnehmen faellt.
fn neuer_punkt(dok: &Dokument, s: Punkt, e: Punkt) -> Punkt {
    if inklusiv_vorfahr(dok, s.0, e.0) {
        return s;
    }
    let mut r = s.0;
    for _ in 0..narsil_fmt_html::bereich::MAX_KETTE {
        match eltern(dok, r) {
            Some(p) if !inklusiv_vorfahr(dok, p, e.0) => r = p,
            _ => break,
        }
    }
    match eltern(dok, r) {
        Some(p) => (p, index(dok, r) + 1),
        None => (r, 0),
    }
}

/// DOM §5.5 `deleteContents`, Schritt 4: die enthaltenen Knoten, deren
/// Eltern nicht enthalten sind, in Baumordnung.
fn loeschliste(dok: &Dokument, s: Punkt, e: Punkt) -> Vec<usize> {
    let mut gemeinsam = s.0;
    while !inklusiv_vorfahr(dok, gemeinsam, e.0) {
        match eltern(dok, gemeinsam) {
            Some(p) => gemeinsam = p,
            None => return Vec::new(),
        }
    }
    let mut raus = Vec::new();
    let kinder = |k: usize| dok.knoten[k].kinder.as_slice();
    // Die Startkette von unten bis unter den gemeinsamen Vorfahren.
    let mut tief = s.0;
    let mut ab = s.1 as usize;
    while tief != gemeinsam {
        raus.extend(kinder(tief).iter().skip(ab).copied());
        let Some(p) = eltern(dok, tief) else { break };
        ab = index(dok, tief) as usize + 1;
        tief = p;
    }
    let von = ab;
    // Die Endkette von oben nach unten.
    let ke = kette(dok, e.0);
    let t = kette(dok, gemeinsam).len();
    let bis = if e.0 == gemeinsam { e.1 as usize } else { ke.get(t).map_or(0, |&c| index(dok, c) as usize) };
    let kg = kinder(gemeinsam);
    raus.extend(kg.iter().take(bis.min(kg.len())).skip(von).copied());
    for w in t..ke.len() {
        let k = ke[w];
        let bis = if k == e.0 { e.1 as usize } else { ke.get(w + 1).map_or(0, |&c| index(dok, c) as usize) };
        raus.extend(kinder(k).iter().take(bis).copied());
    }
    raus.truncate(MAX_LISTE);
    raus
}

/// DOM §5.5 `surroundContents`, Schritt 1: liegt ein Nicht-Text-Knoten
/// nur teilweise im Bereich?
fn teilweise_nicht_text(dok: &Dokument, s: Punkt, e: Punkt) -> bool {
    let ks = kette(dok, s.0);
    let ke = kette(dok, e.0);
    let n = ks.iter().zip(ke.iter()).take_while(|(x, y)| x == y).count();
    ks[n..].iter().chain(ke[n..].iter()).any(|&k| !ist_text(dok, k))
}

/// `__narsilBereich(op, …)` (siehe Modulkopf).
pub fn rufen(l: &mut Lebend, args: &[Wert]) -> Result<Wert, String> {
    let a = |i: usize| args.get(i);
    let o = zahl(a(0));
    let platz = (zahl(a(1)), zahl(a(2)));
    let lesen = |l: &Lebend| l.dok.bereiche.lesen(platz.0, platz.1);
    Ok(match o {
        op::NEU => {
            let Some(d) = knoten(l, a(1)) else { return Ok(Wert::Null) };
            let (i, g) = l.dok.bereiche.anlegen((d, 0));
            Wert::Feld(vec![Wert::Zahl(f64::from(i)), Wert::Zahl(f64::from(g))])
        }
        op::KLON => match lesen(l) {
            Some([s, e]) => {
                let (i, g) = l.dok.bereiche.anlegen(s);
                l.dok.bereiche.setzen(i, g, s, e);
                Wert::Feld(vec![Wert::Zahl(f64::from(i)), Wert::Zahl(f64::from(g))])
            }
            None => Wert::Null,
        },
        op::LESEN => match lesen(l) {
            Some([s, e]) => Wert::Feld(vec![
                l.wert(Some(s.0)),
                Wert::Zahl(f64::from(s.1)),
                l.wert(Some(e.0)),
                Wert::Zahl(f64::from(e.1)),
            ]),
            None => Wert::Null,
        },
        op::GRENZE => {
            // (platz, ende?, knoten, versatz, modus 0 Punkt/1 davor/2 dahinter)
            let Some([s, e]) = lesen(l) else { return Ok(Wert::Null) };
            let Some(k) = knoten(l, a(4)) else { return Ok(Wert::Null) };
            let dok = &l.dok;
            let bp = match zahl(a(6)) {
                0 => {
                    if ist_doctype(dok, k) {
                        return Ok(fehler("InvalidNodeTypeError"));
                    }
                    let v = zahl(a(5));
                    if v > laenge(dok, k) {
                        return Ok(fehler("IndexSizeError"));
                    }
                    (k, v)
                }
                m => {
                    let Some(p) = eltern(dok, k) else { return Ok(fehler("InvalidNodeTypeError")) };
                    (p, index(dok, k) + u32::from(m == 2))
                }
            };
            let anders = wurzel(dok, bp.0) != wurzel(dok, s.0);
            let (s2, e2) = if wahr(a(3)) {
                let s2 = if anders || lage(dok, bp, s) == Ordering::Less { bp } else { s };
                (s2, bp)
            } else {
                let e2 = if anders || lage(dok, bp, e) == Ordering::Greater { bp } else { e };
                (bp, e2)
            };
            l.dok.bereiche.setzen(platz.0, platz.1, s2, e2);
            Wert::Zahl(0.0)
        }
        op::ZUSAMMEN => {
            let Some([s, e]) = lesen(l) else { return Ok(Wert::Null) };
            if wahr(a(3)) {
                l.dok.bereiche.setzen(platz.0, platz.1, s, s);
            } else {
                l.dok.bereiche.setzen(platz.0, platz.1, e, e);
            }
            Wert::Zahl(0.0)
        }
        op::WAEHLEN => {
            let Some(k) = knoten(l, a(3)) else { return Ok(Wert::Null) };
            let dok = &l.dok;
            let (s, e) = if wahr(a(4)) {
                if ist_doctype(dok, k) {
                    return Ok(fehler("InvalidNodeTypeError"));
                }
                ((k, 0), (k, laenge(dok, k)))
            } else {
                let Some(p) = eltern(dok, k) else { return Ok(fehler("InvalidNodeTypeError")) };
                let i = index(dok, k);
                ((p, i), (p, i + 1))
            };
            l.dok.bereiche.setzen(platz.0, platz.1, s, e);
            Wert::Zahl(0.0)
        }
        op::SETZEN => {
            let (Some(sk), Some(ek)) = (knoten(l, a(3)), knoten(l, a(5))) else { return Ok(Wert::Wahr(false)) };
            Wert::Wahr(l.dok.bereiche.setzen(platz.0, platz.1, (sk, zahl(a(4))), (ek, zahl(a(6)))))
        }
        op::VERGLEICH => {
            // (platz, wie, anderer Platz)
            let (Some([s, e]), Some([qs, qe])) = (lesen(l), l.dok.bereiche.lesen(zahl(a(4)), zahl(a(5)))) else {
                return Ok(Wert::Null);
            };
            let dok = &l.dok;
            if wurzel(dok, s.0) != wurzel(dok, qs.0) {
                return Ok(fehler("WrongDocumentError"));
            }
            let (dies, das) = match zahl(a(3)) {
                0 => (s, qs),
                1 => (e, qs),
                2 => (e, qe),
                _ => (s, qe),
            };
            ordnung_wert(lage(dok, dies, das))
        }
        op::PUNKT => {
            // (platz, knoten, versatz, isPointInRange?)
            let Some([s, e]) = lesen(l) else { return Ok(Wert::Null) };
            let Some(k) = knoten(l, a(3)) else { return Ok(Wert::Null) };
            let dok = &l.dok;
            let im = wahr(a(5));
            if wurzel(dok, k) != wurzel(dok, s.0) {
                return Ok(if im { Wert::Wahr(false) } else { fehler("WrongDocumentError") });
            }
            if ist_doctype(dok, k) {
                return Ok(fehler("InvalidNodeTypeError"));
            }
            let v = zahl(a(4));
            if v > laenge(dok, k) {
                return Ok(fehler("IndexSizeError"));
            }
            let vor = lage(dok, (k, v), s) == Ordering::Less;
            let nach = lage(dok, (k, v), e) == Ordering::Greater;
            if im {
                Wert::Wahr(!vor && !nach)
            } else {
                Wert::Zahl(if vor { -1.0 } else if nach { 1.0 } else { 0.0 })
            }
        }
        op::SCHNEIDET => {
            let Some([s, e]) = lesen(l) else { return Ok(Wert::Null) };
            let Some(k) = knoten(l, a(3)) else { return Ok(Wert::Null) };
            let dok = &l.dok;
            if wurzel(dok, k) != wurzel(dok, s.0) {
                return Ok(Wert::Wahr(false));
            }
            let Some(p) = eltern(dok, k) else { return Ok(Wert::Wahr(true)) };
            let i = index(dok, k);
            Wert::Wahr(lage(dok, (p, i), e) == Ordering::Less && lage(dok, (p, i + 1), s) == Ordering::Greater)
        }
        op::GEMEINSAM => {
            let Some([s, e]) = lesen(l) else { return Ok(Wert::Null) };
            let mut c = s.0;
            for _ in 0..narsil_fmt_html::bereich::MAX_KETTE {
                if inklusiv_vorfahr(&l.dok, c, e.0) {
                    break;
                }
                match eltern(&l.dok, c) {
                    Some(p) => c = p,
                    None => break,
                }
            }
            l.wert(Some(c))
        }
        op::TEXT => {
            let Some([s, e]) = lesen(l) else { return Ok(Wert::Null) };
            Wert::Text(als_text(&l.dok, s, e))
        }
        op::TEILE | op::LOESCHLISTE | op::TEILWEISE => {
            // (knoten, versatz, knoten, versatz) ab Stelle 1
            let (Some(sk), Some(ek)) = (knoten(l, a(1)), knoten(l, a(3))) else { return Ok(Wert::Null) };
            let s = (sk, zahl(a(2)));
            let e = (ek, zahl(a(4)));
            match o {
                op::TEILE => teile(l, s, e),
                op::TEILWEISE => Wert::Wahr(teilweise_nicht_text(&l.dok, s, e)),
                _ => {
                    let (nk, no) = neuer_punkt(&l.dok, s, e);
                    let liste = loeschliste(&l.dok, s, e);
                    let liste = Wert::Feld(liste.into_iter().map(|k| l.wert(Some(k))).collect());
                    Wert::Feld(vec![l.wert(Some(nk)), Wert::Zahl(f64::from(no)), liste])
                }
            }
        }
        op::LAGE => {
            let (Some(ak), Some(bk)) = (knoten(l, a(1)), knoten(l, a(3))) else { return Ok(Wert::Null) };
            if wurzel(&l.dok, ak) != wurzel(&l.dok, bk) {
                return Ok(Wert::Zahl(2.0));
            }
            ordnung_wert(lage(&l.dok, (ak, zahl(a(2))), (bk, zahl(a(4)))))
        }
        op::LAENGE => match knoten(l, a(1)) {
            Some(k) => Wert::Zahl(f64::from(laenge(&l.dok, k))),
            None => Wert::Null,
        },
        op::INDEX => match knoten(l, a(1)) {
            Some(k) => Wert::Zahl(f64::from(index(&l.dok, k))),
            None => Wert::Null,
        },
        op::WURZEL => match knoten(l, a(1)) {
            Some(k) => {
                let w = wurzel(&l.dok, k);
                l.wert(Some(w))
            }
            None => Wert::Null,
        },
        op::SCHATTENWURZEL => match knoten(l, a(1)) {
            // DOM §4.2.2 „shadow-including root": ueber die Wirte hinauf.
            Some(k) => {
                let mut w = wurzel(&l.dok, k);
                for _ in 0..narsil_fmt_html::bereich::MAX_KETTE {
                    match l.dok.knoten.get(w) {
                        Some(n) if matches!(n.art, Art::ShadowRoot { .. }) && n.eltern.is_some() => {
                            w = wurzel(&l.dok, n.eltern.unwrap_or(w));
                        }
                        _ => break,
                    }
                }
                l.wert(Some(w))
            }
            None => Wert::Null,
        },
        op::PRUEFEN => {
            // DOM §4.2.3 „ensure pre-insertion validity" (eltern, knoten, vor)
            let (Some(p), Some(k)) = (knoten(l, a(1)), knoten(l, a(2))) else { return Ok(Wert::Null) };
            let vor = knoten(l, a(3));
            match crate::lebend::einfuegen_pruefen(&l.dok, p, k, vor, None) {
                Ok(()) => Wert::Null,
                Err(m) => Wert::Text(m.split(':').next().unwrap_or("HierarchyRequestError").trim().to_string()),
            }
        }
        _ => Wert::Undefiniert,
    })
}

/// Ist `k` ein Zeichendaten-Knoten? (fuer Tests)
#[must_use]
pub fn ist_zeichen(l: &Lebend, k: usize) -> bool {
    ist_zeichendaten(&l.dok, k)
}

// ===========================================================================
// GEOMETRIE (CSSOM View §6.4 `getClientRects`/`getBoundingClientRect`)
// ===========================================================================

/// Release 12.10.2026: ein Textlauf des letzten Layouts — Kasten in
/// Seitenkoordinaten (mit `transform`), der gezeichnete Text und die Schrift,
/// mit der das Layout ihn gemessen hat. Je Element-Host (die Laeufe eines
/// Elements tragen seine Ordnungszahl, siehe `narsil-page` Zeilenbau).
#[derive(Debug, Clone)]
pub struct TextLauf {
    pub x: f32,
    pub y: f32,
    pub b: f32,
    pub h: f32,
    /// Massstab aus `transform` (Breiten wachsen mit).
    pub s: f32,
    pub text: String,
    pub grad: f32,
    pub gewicht: u16,
    pub kursiv: bool,
    pub familie: String,
}

/// Hoechstens so viele Laeufe je Element (M3).
const MAX_LAEUFE: usize = 4096;

/// Die Laeufe eines Layoutbaums je Element-Host (gleiche Zuordnung und
/// gleiche `transform`-Abbildung wie `sitzung::masse_rechnen`).
pub(crate) fn textlaeufe_rechnen(
    baum: &narsil_layout::LayoutTree,
    seite: &narsil_page::Seite,
    ids: &std::collections::HashMap<usize, u32>,
) -> std::collections::HashMap<u32, Vec<TextLauf>> {
    fn lauf(
        bx: &narsil_layout::LayoutBox,
        a: &narsil_page::Seite,
        ids: &std::collections::HashMap<usize, u32>,
        raus: &mut std::collections::HashMap<u32, Vec<TextLauf>>,
        tiefe: usize,
        aussen: (f32, f32, f32),
        oben: Option<u32>,
    ) {
        if tiefe > 512 {
            return;
        }
        let (s, dx, dy) = bx.abbildung(aussen);
        // Ein Lauf ohne eigene Nummer gehoert dem naechsten Kasten darueber,
        // der eine hat (Zeilen eines `<textarea>`, anonyme Zeilenkaesten).
        let eigen = a.dom_zu_ord.get(&(bx.node.index() as u32)).and_then(|o| ids.get(o)).copied();
        let host_hier = eigen.or(oben);
        if let Some(t) = bx.text.as_ref()
            && let Some(host) = host_hier
        {
            let r = bx.border_box;
            let v = raus.entry(host).or_default();
            if v.len() < MAX_LAEUFE && r.width.is_finite() && r.height.is_finite() {
                v.push(TextLauf {
                    x: s * r.x + dx,
                    y: s * r.y + dy,
                    b: s * r.width,
                    h: s * r.height,
                    s,
                    text: t.text.clone(),
                    grad: t.font_size,
                    gewicht: t.gewicht,
                    kursiv: t.kursiv,
                    familie: t.schriftfamilie.clone(),
                });
            }
        }
        for k in &bx.children {
            lauf(k, a, ids, raus, tiefe + 1, (s, dx, dy), host_hier);
        }
    }
    let mut raus = std::collections::HashMap::new();
    lauf(&baum.root, seite, ids, &mut raus, 0, (1.0, 0.0, 0.0), None);
    raus
}

fn ist_leer(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r' | '\u{c}')
}

fn zeichen_gleich(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase()) || (a == '\u{a0}' && b == ' ') || (b == '\u{a0}' && a == ' ')
}

/// Ordnet jedem Zeichen eines Laufs die UTF-16-Stelle im DOM-Text zu
/// (Leerraum faellt im Layout zusammen; ein nicht passendes Zeichen — etwa
/// erzeugter Inhalt — bekommt `u32::MAX`). Je Lauf: Stellen der Zeichen und
/// die Stelle hinter dem letzten.
fn ausrichten(dom: &str, laeufe: &[TextLauf]) -> Vec<(Vec<u32>, u32)> {
    let mut zeichen: Vec<(char, u32)> = Vec::new();
    let mut o = 0u32;
    for c in dom.chars() {
        zeichen.push((c, o));
        o += c.len_utf16() as u32;
    }
    zeichen.push((' ', o));
    let ende = zeichen.len() - 1;
    let mut pos = 0usize;
    let mut raus = Vec::with_capacity(laeufe.len());
    for l in laeufe {
        let mut stellen = Vec::with_capacity(l.text.len());
        // Ein Zeichen, das `text-transform` zu mehreren macht (ß -> SS):
        // die restlichen erwarteten Zeichen und ihre DOM-Stelle.
        let mut erwartet: std::collections::VecDeque<char> = std::collections::VecDeque::new();
        let mut erwartet_o = 0u32;
        for c in l.text.chars() {
            if let Some(&e) = erwartet.front() {
                if e == c {
                    erwartet.pop_front();
                    stellen.push(erwartet_o);
                    continue;
                }
                erwartet.clear();
            }
            if ist_leer(c) {
                stellen.push(zeichen[pos].1);
                if pos < ende && ist_leer(zeichen[pos].0) {
                    while pos < ende && ist_leer(zeichen[pos].0) {
                        pos += 1;
                    }
                }
                continue;
            }
            let mut p = pos;
            while p < ende && ist_leer(zeichen[p].0) {
                p += 1;
            }
            if p < ende && zeichen_gleich(zeichen[p].0, c) {
                stellen.push(zeichen[p].1);
                pos = p + 1;
            } else if p < ende
                && let Some(mehr) = [zeichen[p].0.to_uppercase().collect::<Vec<char>>(), zeichen[p].0.to_lowercase().collect::<Vec<char>>()]
                    .into_iter()
                    .find(|v| v.len() > 1 && v[0] == c)
            {
                erwartet = mehr[1..].iter().copied().collect();
                erwartet_o = zeichen[p].1;
                stellen.push(zeichen[p].1);
                pos = p + 1;
            } else {
                stellen.push(u32::MAX);
            }
        }
        // Ein maskierter Lauf (`type=password`: gleich viele Punkte wie
        // Zeichen) gilt Stelle fuer Stelle.
        let n = stellen.len();
        if n > 0 && stellen.iter().all(|&o| o == u32::MAX) && ende - pos == n {
            for (i, st) in stellen.iter_mut().enumerate() {
                *st = zeichen[pos + i].1;
            }
            pos = ende;
        }
        raus.push((stellen, zeichen[pos].1));
    }
    raus
}

/// Die Rechtecke `[x, y, b, h]` (Seitenkoordinaten) fuer die UTF-16-Stellen
/// `von..bis` des DOM-Texts `dom`, der in den Laeufen `laeufe` steht. Ein
/// leerer Bereich ergibt ein Rechteck der Breite 0 an der Stelle.
pub(crate) fn text_rechtecke(messung: &narsil_layout::Messung, dom: &str, laeufe: &[TextLauf], von: u32, bis: u32) -> Vec<[f32; 4]> {
    let breite = |l: &TextLauf, n: usize| -> f32 {
        if n == 0 {
            return 0.0;
        }
        let teil: String = l.text.chars().take(n).collect();
        let b = l.s * messung.breite_stil_familie(&teil, l.grad, l.gewicht, l.kursiv, &l.familie);
        if b.is_finite() { b } else { 0.0 }
    };
    let mut raus = Vec::new();
    let zuordnung = ausrichten(dom, laeufe);
    for (l, (stellen, hinter)) in laeufe.iter().zip(zuordnung.iter()) {
        // Ein Lauf nur aus Leerraum (zwischen Bloecken) hat in Chromium
        // keinen Kasten (WPT range-bounding-client-rect-with-nested-text).
        if l.text.chars().all(ist_leer) {
            continue;
        }
        let mut erste_i = None;
        let mut letzte_i = None;
        // Ein Zeichen zaehlt, wenn es den Bereich schneidet — auch ein
        // Ersatzpaar, in dessen Mitte eine Grenze liegt (CSSOM View §6.4).
        let laengen: Vec<u32> = l.text.chars().map(|c| c.len_utf16() as u32).collect();
        for (i, &o) in stellen.iter().enumerate() {
            let n = laengen.get(i).copied().unwrap_or(1);
            if o != u32::MAX && o < bis && o + n > von {
                if erste_i.is_none() {
                    erste_i = Some(i);
                }
                letzte_i = Some(i);
            }
        }
        if let (Some(a), Some(z)) = (erste_i, letzte_i) {
            let x0 = breite(l, a);
            let x1 = breite(l, z + 1);
            raus.push([l.x + x0, l.y, (x1 - x0).max(0.0), l.h]);
        } else if von == bis && raus.is_empty() {
            // Die Einfuegemarke: im ersten Lauf, der die Stelle enthaelt.
            if let Some(erste) = stellen.iter().copied().find(|&o| o != u32::MAX)
                && von >= erste
                && von <= *hinter
            {
                let i = stellen.iter().position(|&o| o != u32::MAX && o >= von).unwrap_or(stellen.len());
                let x = breite(l, i);
                raus.push([l.x + x, l.y, 0.0, l.h]);
            }
        }
        if raus.len() > MAX_LAEUFE {
            break;
        }
    }
    raus
}

/// Release 12.10.2026: der GEZEICHNETE Text der UTF-16-Stellen `von..bis`
/// eines Textknotens (mit `text-transform` und zusammengefallenem
/// Leerraum) — fuer `Selection.toString()`, das in Chromium den gerenderten
/// Text liefert. `None`, wenn es keine Laeufe gibt.
pub fn gezeichnet(w: &mut crate::dom::Stand, args: &[Wert]) -> Result<Wert, String> {
    if !w.textlaeufe_an {
        w.textlaeufe_an = true;
        w.layout_veraltet = true;
    }
    crate::zwangslayout::erzwingen(w);
    let w = &mut *w;
    let Some(l) = w.lebend.as_mut() else { return Ok(Wert::Null) };
    let (Some(sk), Some(ek)) = (knoten(l, args.first()), knoten(l, args.get(2))) else { return Ok(Wert::Null) };
    // Nur Textstuecke unter EINEM Element (kein Block- oder Elementwechsel,
    // dort setzte Chromium Zeilenumbrueche): sonst der Text des Bereichs.
    let teile = geometrie_teile(&l.dok, (sk, zahl(args.get(1))), (ek, zahl(args.get(3))));
    let mut eltern_k = None;
    for &(k, art, _, _) in &teile {
        let p = eltern(&l.dok, k);
        if art != 3 || p.is_none() || (eltern_k.is_some() && eltern_k != p) {
            return Ok(Wert::Null);
        }
        eltern_k = p;
    }
    let Some(p) = eltern_k.filter(|&p| l.dok.ist_element_knoten(p)) else { return Ok(Wert::Null) };
    let Some(h) = l.host(p) else { return Ok(Wert::Null) };
    let Some(laeufe) = w.textlaeufe.get(&h) else { return Ok(Wert::Null) };
    let mut raus = String::new();
    let mut gefunden = false;
    for &(k, _, a, b) in &teile {
        let (text, basis) = textkinder(&l.dok, p, k);
        let (von, bis) = (basis + a, basis + b);
        for (lauf, (stellen, _)) in laeufe.iter().zip(ausrichten(&text, laeufe).iter()) {
            for (c, &o) in lauf.text.chars().zip(stellen.iter()) {
                if o != u32::MAX && o >= von && o < bis {
                    raus.push(c);
                    gefunden = true;
                }
            }
        }
    }
    Ok(if gefunden { Wert::Text(raus) } else { Wert::Null })
}

/// Release 12.10.2026: CSSOM View §6.1 `caretPositionFromPoint` — die Stelle
/// im Text eines Elements unter dem Punkt (`x`, `y` in Ansichtskoordinaten):
/// der Lauf unter dem Punkt (sonst der naechste), darin die naechste
/// Zeichengrenze. Mit `text` (Formularfeld) die Stelle in diesem Text,
/// sonst `[Textknoten, Stelle]` unter den direkten Textkindern. `null`,
/// wenn das Element keine Laeufe hat.
pub fn stelle_an_punkt(w: &mut crate::dom::Stand, args: &[Wert]) -> Result<Wert, String> {
    if !w.textlaeufe_an {
        w.textlaeufe_an = true;
        w.layout_veraltet = true;
    }
    crate::zwangslayout::erzwingen(w);
    let Some(messung) = w.stil_satz.as_ref().map(|s| s.messung.clone()) else { return Ok(Wert::Null) };
    let rollen = w.rollen_y;
    let w = &mut *w;
    let Some(l) = w.lebend.as_mut() else { return Ok(Wert::Null) };
    let Some(el) = knoten(l, args.first()) else { return Ok(Wert::Null) };
    let zahl_f = |v: Option<&Wert>| match v {
        Some(Wert::Zahl(z)) if z.is_finite() => *z as f32,
        _ => 0.0,
    };
    let (x, y) = (zahl_f(args.get(1)), zahl_f(args.get(2)) + rollen);
    let feld = args.get(3).filter(|v| !matches!(v, Wert::Null | Wert::Undefiniert)).map(Wert::als_text);
    // Hat das Element selbst keinen Lauf (etwa `pointer-events: none` an
    // den Kindern), gilt das Element darunter, dessen Lauf am naechsten liegt.
    let abstand = |l: &TextLauf| -> (f32, f32) {
        let dy = if y < l.y { l.y - y } else if y >= l.y + l.h { y - (l.y + l.h) } else { 0.0 };
        let dx = if x < l.x { l.x - x } else if x > l.x + l.b { x - (l.x + l.b) } else { 0.0 };
        (dy, dx)
    };
    let ist_feld = |l: &Lebend, k: usize| matches!(l.dok.element_name(k), Some("input" | "textarea" | "select"));
    let mut el = el;
    if feld.is_none() {
        // Das tiefste Element, dessen Kasten den Punkt enthaelt (die
        // Trefferpruefung des Vorspanns uebergeht `pointer-events: none`).
        let mut kandidaten = Vec::new();
        let mut stapel: Vec<usize> = l.dok.knoten[el].kinder.iter().rev().copied().collect();
        while let Some(k) = stapel.pop() {
            if kandidaten.len() > 4096 {
                break;
            }
            if l.dok.ist_element_knoten(k) {
                kandidaten.push(k);
                if !ist_feld(l, k) {
                    stapel.extend(l.dok.knoten[k].kinder.iter().rev().copied());
                }
            }
        }
        for k in kandidaten.into_iter().rev() {
            let Some(h) = l.host(k) else { continue };
            if let Some(m) = w.masse.get(&h)
                && x >= m[0]
                && x < m[0] + m[2]
                && y >= m[1]
                && y < m[1] + m[3]
            {
                el = k;
                break;
            }
        }
        if ist_feld(l, el) || w.textlaeufe.get(&l.host(el).unwrap_or(0)).is_none_or(|v| v.iter().all(|t| t.text.chars().all(ist_leer))) {
            // Ein Element ohne eigenen Text: der Vorspann entscheidet (-1).
            return Ok(Wert::Feld(vec![l.wert(Some(el)), Wert::Zahl(-1.0)]));
        }
    }
    let Some(h) = l.host(el) else { return Ok(Wert::Null) };
    let Some(laeufe) = w.textlaeufe.get(&h).filter(|v| !v.is_empty()) else { return Ok(Wert::Null) };
    let text = match &feld {
        Some(t) => t.clone(),
        None => {
            let mut s = String::new();
            for &c in &l.dok.knoten[el].kinder {
                if let Some(Art::Text(t)) = l.dok.knoten.get(c).map(|n| &n.art) {
                    s.push_str(t);
                }
            }
            s
        }
    };
    // Der Lauf: unter dem Punkt, sonst auf derselben Zeile, sonst der naechste.
    let Some((nr, lauf)) = laeufe
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.text.chars().all(ist_leer))
        .min_by(|a, b| abstand(a.1).partial_cmp(&abstand(b.1)).unwrap_or(Ordering::Equal))
    else {
        return Ok(Wert::Null);
    };
    let n = lauf.text.chars().count();
    let mut beste = (f32::MAX, 0usize);
    let mut teil = String::new();
    for (i, c) in std::iter::once('\u{0}').chain(lauf.text.chars()).enumerate() {
        if i > 0 {
            teil.push(c);
        }
        let b = if i == 0 { 0.0 } else { lauf.s * messung.breite_stil_familie(&teil, lauf.grad, lauf.gewicht, lauf.kursiv, &lauf.familie) };
        let d = (lauf.x + b - x).abs();
        if d < beste.0 {
            beste = (d, i);
        }
        if i >= n {
            break;
        }
    }
    let zuordnung = ausrichten(&text, laeufe);
    let Some((stellen, hinter)) = zuordnung.get(nr) else { return Ok(Wert::Null) };
    let stelle = stellen[beste.1.min(stellen.len())..].iter().copied().find(|&o| o != u32::MAX).unwrap_or(*hinter);
    if feld.is_some() {
        return Ok(Wert::Feld(vec![l.wert(Some(el)), Wert::Zahl(f64::from(stelle))]));
    }
    let mut basis = 0u32;
    let mut letzter = None;
    for &c in &l.dok.knoten[el].kinder.clone() {
        if let Some(Art::Text(t)) = l.dok.knoten.get(c).map(|n| &n.art) {
            let len = laenge16(t);
            if stelle < basis + len || (stelle == basis + len && len > 0) {
                return Ok(Wert::Feld(vec![l.wert(Some(c)), Wert::Zahl(f64::from(stelle - basis))]));
            }
            basis += len;
            letzter = Some((c, len));
        }
    }
    Ok(match letzter {
        Some((c, len)) => Wert::Feld(vec![l.wert(Some(c)), Wert::Zahl(f64::from(len))]),
        None => Wert::Null,
    })
}

/// Die direkten Textkinder eines Elements als ein Text, dazu die Stelle,
/// an der der Text von `t` darin beginnt.
fn textkinder(dok: &Dokument, eltern_k: usize, t: usize) -> (String, u32) {
    let mut s = String::new();
    let mut basis = 0u32;
    for &c in &dok.knoten[eltern_k].kinder {
        if let Some(Art::Text(x)) = dok.knoten.get(c).map(|n| &n.art) {
            if c == t {
                basis = laenge16(&s);
            }
            s.push_str(x);
        }
    }
    (s, basis)
}

/// DOM-Teile eines Bereichs fuer `getClientRects` (CSSOM View §6.4): die
/// Elemente, die im Bereich liegen und deren Eltern nicht (Art 1), und jeder
/// Textknoten, der ganz oder teilweise darin liegt (Art 3, mit Stellen).
pub(crate) fn geometrie_teile(dok: &Dokument, s: Punkt, e: Punkt) -> Vec<(usize, u8, u32, u32)> {
    let mut raus = Vec::new();
    if wurzel(dok, s.0) != wurzel(dok, e.0) {
        return raus;
    }
    let ende = hinter_teilbaum(dok, e.0);
    let mut n = Some(s.0);
    let mut schritte = 0usize;
    while let Some(k) = n {
        if Some(k) == ende || schritte > narsil_fmt_html::dom::MAX_KNOTEN || raus.len() > MAX_LISTE {
            break;
        }
        schritte += 1;
        if ist_text(dok, k) {
            let l = laenge(dok, k);
            let teil = match (k == s.0, k == e.0) {
                (true, true) => Some((s.1, e.1)),
                (true, false) => Some((s.1, l)),
                (false, true) => Some((0, e.1)),
                (false, false) if enthalten(dok, k, s, e) => Some((0, l)),
                _ => None,
            };
            if let Some((a, b)) = teil
                && a <= b
            {
                raus.push((k, 3, a, b));
            }
        } else if dok.ist_element_knoten(k)
            && enthalten(dok, k, s, e)
            && !eltern(dok, k).is_some_and(|p| dok.ist_element_knoten(p) && enthalten(dok, p, s, e))
        {
            raus.push((k, 1, 0, 0));
        }
        n = naechster(dok, k);
    }
    raus
}

/// `__narsilTextKaesten(knoten, text, von, bis)` (`bereich` = false) und
/// `__narsilBereichKaesten(sc, so, ec, eo)` (`bereich` = true): die
/// Rechtecke in Ansichtskoordinaten als `[[x, y, b, h], …]`.
pub fn kaesten(w: &mut crate::dom::Stand, args: &[Wert], bereich: bool) -> Result<Wert, String> {
    if !w.textlaeufe_an {
        w.textlaeufe_an = true;
        w.layout_veraltet = true;
    }
    crate::zwangslayout::erzwingen(w);
    let leer = || Ok(Wert::Feld(Vec::new()));
    let Some(messung) = w.stil_satz.as_ref().map(|s| s.messung.clone()) else { return leer() };
    let rollen = w.rollen_y;
    let mut rechtecke: Vec<[f32; 4]> = Vec::new();
    let w = &mut *w;
    let Some(l) = w.lebend.as_mut() else { return leer() };
    if bereich {
        let (Some(sk), Some(ek)) = (knoten(l, args.first()), knoten(l, args.get(2))) else { return leer() };
        let teile = geometrie_teile(&l.dok, (sk, zahl(args.get(1))), (ek, zahl(args.get(3))));
        for (k, art, a, b) in teile {
            if art == 1 {
                // Auch ein Kasten der Groesse 0 zaehlt (er ist gelayoutet).
                if let Some(h) = l.host(k)
                    && let Some(m) = w.masse.get(&h)
                {
                    rechtecke.push([m[0], m[1], m[2], m[3]]);
                }
                continue;
            }
            let Some(p) = eltern(&l.dok, k).filter(|&p| l.dok.ist_element_knoten(p)) else { continue };
            let (text, basis) = textkinder(&l.dok, p, k);
            let Some(h) = l.host(p) else { continue };
            if let Some(laeufe) = w.textlaeufe.get(&h) {
                rechtecke.extend(text_rechtecke(&messung, &text, laeufe, basis + a, basis + b));
            }
        }
    } else {
        let Some(k) = knoten(l, args.first()) else { return leer() };
        let text = args.get(1).map(Wert::als_text).unwrap_or_default();
        let Some(h) = l.host(k) else { return leer() };
        if let Some(laeufe) = w.textlaeufe.get(&h) {
            rechtecke.extend(text_rechtecke(&messung, &text, laeufe, zahl(args.get(2)), zahl(args.get(3))));
        }
    }
    let zahl_w = |v: f32| Wert::Zahl(f64::from(v));
    Ok(Wert::Feld(
        rechtecke
            .into_iter()
            .map(|r| Wert::Feld(vec![zahl_w(r[0]), zahl_w(r[1] - rollen), zahl_w(r[2]), zahl_w(r[3])]))
            .collect(),
    ))
}

#[cfg(test)]
mod geometrie_tests {
    use super::*;

    fn lauf(text: &str) -> TextLauf {
        TextLauf { x: 0.0, y: 0.0, b: 10.0 * text.chars().count() as f32, h: 19.0, s: 1.0, text: text.into(), grad: 16.0, gewicht: 400, kursiv: false, familie: String::new() }
    }

    #[test]
    fn leerraum_faellt_zusammen() {
        let z = ausrichten("Hello   big\n world", &[lauf("Hello big "), lauf("world")]);
        assert_eq!(z[0].0, vec![0, 1, 2, 3, 4, 5, 8, 9, 10, 11]);
        assert_eq!(z[1].0, vec![13, 14, 15, 16, 17]);
        assert_eq!(z[1].1, 18);
    }

    #[test]
    fn grossschreibung_und_erzeugtes() {
        let z = ausrichten("abc", &[lauf(">>"), lauf("ABC")]);
        assert_eq!(z[0].0, vec![u32::MAX, u32::MAX]);
        assert_eq!(z[1].0, vec![0, 1, 2]);
    }
}
