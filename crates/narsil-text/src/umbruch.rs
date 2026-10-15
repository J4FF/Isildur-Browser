//! Zeilenumbruch nach UAX #14 (Aufgabe I2).
//!
//! Umsetzung der Regeln LB1..LB31 der Fassung Unicode 15.1
//! (<https://www.unicode.org/reports/tr14/tr14-51.html>) mit der in
//! Abschnitt 8.2, Beispiel 7 beschriebenen Zahlen-Tailoring, die auch
//! der Abnahmedatei `LineBreakTest.txt` zugrunde liegt:
//!
//! * LB13 wird zu `[^NU] x (CL | CP | IS | SY)` abgeschwaecht,
//! * LB25 wird durch die dortigen fuenf "Regex-Number"-Regeln ersetzt.
//!
//! Die Klassen kommen aus [`crate::unicode_daten::linebreak_klassen`]
//! (aus DerivedLineBreak.txt erzeugt, Binaersuche). Kombinationszeichen
//! werden nach LB9/LB10 zu einer logischen Einheit mit ihrer Basis
//! zusammengefasst; dadurch arbeiten alle Regeln auf Einheiten statt auf
//! einzelnen Zeichen.
//!
//! Eigenschaften: lineare Laufzeit in der Textlaenge, keine Panics,
//! nur gueltige UTF-8-Grenzen. Fuer Thai/Lao/Khmer (SA) gibt es keine
//! Wortliste; LB1 loest SA wie vorgeschrieben nach AL bzw. CM auf, es
//! wird also nur an den Stellen getrennt, die UAX #14 ohne
//! Sprachanalyse erlaubt.

use crate::unicode_daten::linebreak_klassen::{
    Umbruchklasse, ext_pict_cn, ist_breit, ist_pf, ist_pi, sa_ist_kombinationszeichen,
    umbruch_klasse,
};

/// Hoechstzahl der Umbruchstellen, die [`umbruch_punkte`] meldet.
pub const MAX_UMBRUECHE: usize = 1_000_000;

/// DOTTED CIRCLE (U+25CC). LB28a nennt in der Testfassung diese
/// konkrete Figur (Variable "DottedCircle") neben AK und AS.
const PUNKTKREIS: char = '\u{25CC}';

/// Eine logische Einheit: Basiszeichen plus absorbierte CM/ZWJ (LB9).
#[derive(Debug, Clone, Copy)]
struct Einheit {
    /// Wirksame Zeilenumbruchklasse (LB1-Aufloesung, LB9/LB10).
    klasse: Umbruchklasse,
    /// Erstes Zeichen der Einheit (fuer die Pi/Pf-Pruefung in LB15a/b).
    ch: char,
    /// East_Asian_Width F/W/H (LB30).
    breit: bool,
    /// [Extended_Pictographic & Cn] (LB30b).
    pict_cn: bool,
    /// Letztes Zeichen der Einheit ist ein ZWJ (LB8a).
    endet_mit_zwj: bool,
}

/// Zerlegter Text: Einheitengrenzen als Byte-Offsets plus Einheiten.
struct Zerlegt {
    /// Byte-Offset des ersten Zeichens jeder Einheit, aufsteigend.
    bytes: Vec<usize>,
    einheiten: Vec<Einheit>,
}

impl Einheit {
    fn neu(ch: char, klasse: Umbruchklasse) -> Self {
        Einheit {
            klasse,
            ch,
            breit: ist_breit(ch),
            pict_cn: ext_pict_cn(ch),
            endet_mit_zwj: ch == '\u{200D}',
        }
    }
}

/// LB1: AI/SG/XX -> AL, CJ -> NS, SA -> CM (nur Mn/Mc) sonst AL.
fn aufloesen(ch: char) -> Umbruchklasse {
    match umbruch_klasse(ch) {
        Umbruchklasse::Ai | Umbruchklasse::Sg | Umbruchklasse::Xx => Umbruchklasse::Al,
        Umbruchklasse::Cj => Umbruchklasse::Ns,
        Umbruchklasse::Sa => {
            if sa_ist_kombinationszeichen(ch) {
                Umbruchklasse::Cm
            } else {
                Umbruchklasse::Al
            }
        }
        klasse => klasse,
    }
}

/// LB9: eine Basis ist fuer nachfolgende CM/ZWJ aufnahmefaehig, wenn sie
/// nicht BK, CR, LF, NL, SP oder ZW ist.
fn absorbierbar(klasse: Umbruchklasse) -> bool {
    !matches!(
        klasse,
        Umbruchklasse::Bk
            | Umbruchklasse::Cr
            | Umbruchklasse::Lf
            | Umbruchklasse::Nl
            | Umbruchklasse::Sp
            | Umbruchklasse::Zw
    )
}

/// Zerlegt den Text in Einheiten (linear, ohne Allokation pro Zeichen
/// ausser den beiden Ergebnis-Vektoren).
fn zerlegen(text: &str) -> Zerlegt {
    let mut bytes: Vec<usize> = Vec::new();
    let mut einheiten: Vec<Einheit> = Vec::new();
    for (byte, ch) in text.char_indices() {
        let roh = aufloesen(ch);
        if matches!(roh, Umbruchklasse::Cm | Umbruchklasse::Zwj) {
            // LB9: an die vorige Einheit anhaengen, wenn deren Basis
            // aufnahmefaehig ist.
            if let Some(letzte) = einheiten.last_mut() {
                if absorbierbar(letzte.klasse) {
                    letzte.endet_mit_zwj = roh == Umbruchklasse::Zwj;
                    continue;
                }
            }
            // LB10: uebrig gebliebenes CM/ZWJ wirkt wie AL.
            bytes.push(byte);
            einheiten.push(Einheit::neu(ch, Umbruchklasse::Al));
        } else {
            bytes.push(byte);
            einheiten.push(Einheit::neu(ch, roh));
        }
    }
    Zerlegt { bytes, einheiten }
}

/// `vor_sp[j]` = Index der letzten Einheit vor der Grenze `j`, die kein
/// Leerzeichen (SP) ist; `usize::MAX`, wenn es keine gibt. Wird von
/// LB8 (ZW SP*), LB14 (OP SP*), LB15a, LB16 und LB17 gebraucht.
fn vor_sp_tabelle(e: &[Einheit]) -> Vec<usize> {
    let mut tabelle = vec![usize::MAX; e.len() + 1];
    for j in 1..=e.len() {
        tabelle[j] = if e[j - 1].klasse == Umbruchklasse::Sp {
            tabelle[j - 1]
        } else {
            j - 1
        };
    }
    tabelle
}

/// `ri_lauf[i]` = Laenge des Laufs gleicher RI-Klassen, der bei Einheit
/// `i` endet (0, wenn `i` kein RI ist). Fuer LB30a (gerade Anzahl).
fn ri_lauf_tabelle(e: &[Einheit]) -> Vec<u32> {
    let mut tabelle = vec![0u32; e.len()];
    for i in 0..e.len() {
        if e[i].klasse == Umbruchklasse::Ri {
            tabelle[i] = if i > 0 && e[i - 1].klasse == Umbruchklasse::Ri {
                tabelle[i - 1].saturating_add(1)
            } else {
                1
            };
        }
    }
    tabelle
}

/// LB25 (Beispiel 7): `zahl_basis[i]` ist true, wenn der maximale Lauf
/// aus NU/SY/IS, der bei Einheit `i` endet, ein NU enthaelt. Ersetzt das
///rueckwaertsuchen (Regex `NU (NU|SY|IS)*`) durch O(1) je Grenze.
fn zahl_basis_tabelle(e: &[Einheit]) -> Vec<bool> {
    let mut basis = vec![false; e.len()];
    for i in 0..e.len() {
        if matches!(
            e[i].klasse,
            Umbruchklasse::Nu | Umbruchklasse::Sy | Umbruchklasse::Is
        ) {
            basis[i] = e[i].klasse == Umbruchklasse::Nu
                || (i > 0
                    && matches!(
                        e[i - 1].klasse,
                        Umbruchklasse::Nu | Umbruchklasse::Sy | Umbruchklasse::Is
                    )
                    && basis[i - 1]);
        }
    }
    basis
}

/// LB25 (Beispiel 7), Regel 4: steht vor der Grenze ein Lauf
/// `NU (NU|SY|IS)*`?
fn zahl_laeuft(e: &[Einheit], basis: &[bool], grenze: usize) -> bool {
    if grenze == 0 {
        return false;
    }
    let i = grenze - 1;
    matches!(
        e[i].klasse,
        Umbruchklasse::Nu | Umbruchklasse::Sy | Umbruchklasse::Is
    ) && basis[i]
}

/// LB25 (Beispiel 7), Regel 5: derselbe Lauf, aber mit einem optionalen
/// schliessenden CL/CP direkt vor der Grenze.
fn zahl_mit_klammer_laeuft(e: &[Einheit], basis: &[bool], grenze: usize) -> bool {
    if grenze == 0 {
        return false;
    }
    let mut i = grenze - 1;
    if matches!(e[i].klasse, Umbruchklasse::Cl | Umbruchklasse::Cp) {
        if i == 0 {
            return false;
        }
        i -= 1;
    }
    matches!(
        e[i].klasse,
        Umbruchklasse::Nu | Umbruchklasse::Sy | Umbruchklasse::Is
    ) && basis[i]
}

/// Ist an der Grenze `j` (zwischen `e[j-1]` und `e[j]`) ein Umbruch
/// erlaubt? Wendet LB2..LB31 in der vorgeschriebenen Reihenfolge an.
fn entscheide(
    e: &[Einheit],
    j: usize,
    vor_sp: &[usize],
    ri_lauf: &[u32],
    zahl_basis: &[bool],
) -> bool {
    if j == 0 || j >= e.len() {
        return false;
    }
    let a = e[j - 1].klasse;
    let b = e[j].klasse;
    let p = vor_sp[j];

    // LB2/LB3 (sot/eot) werden vom Aufrufer behandelt.

    // LB4: BK !
    if a == Umbruchklasse::Bk {
        return true;
    }
    // LB5: CR x LF, CR !, LF !, NL !
    if a == Umbruchklasse::Cr && b == Umbruchklasse::Lf {
        return false;
    }
    if matches!(a, Umbruchklasse::Cr | Umbruchklasse::Lf | Umbruchklasse::Nl) {
        return true;
    }
    // LB6: x (BK | CR | LF | NL)
    if matches!(
        b,
        Umbruchklasse::Bk | Umbruchklasse::Cr | Umbruchklasse::Lf | Umbruchklasse::Nl
    ) {
        return false;
    }
    // LB7: x SP, x ZW
    if matches!(b, Umbruchklasse::Sp | Umbruchklasse::Zw) {
        return false;
    }
    // LB8: ZW SP* ÷
    if p != usize::MAX && e[p].klasse == Umbruchklasse::Zw {
        return true;
    }
    // LB8a: ZWJ x
    if e[j - 1].endet_mit_zwj {
        return false;
    }
    // LB9/LB10 sind in `zerlegen` eingebaut.

    // LB11: x WJ, WJ x
    if matches!(a, Umbruchklasse::Wj) || matches!(b, Umbruchklasse::Wj) {
        return false;
    }
    // LB12: GL x
    if a == Umbruchklasse::Gl {
        return false;
    }
    // LB12a: [^SP BA HY] x GL
    if b == Umbruchklasse::Gl
        && !matches!(a, Umbruchklasse::Sp | Umbruchklasse::Ba | Umbruchklasse::Hy)
    {
        return false;
    }
    // LB13 in der Fassung von Beispiel 7 (Zahlen-Tailoring):
    // x EX, [^NU] x (CL | CP | IS | SY)
    if b == Umbruchklasse::Ex {
        return false;
    }
    if matches!(
        b,
        Umbruchklasse::Cl | Umbruchklasse::Cp | Umbruchklasse::Is | Umbruchklasse::Sy
    ) && a != Umbruchklasse::Nu
    {
        return false;
    }
    // LB14: OP SP* x
    if p != usize::MAX && e[p].klasse == Umbruchklasse::Op {
        return false;
    }
    // LB15a: (sot | BK | CR | LF | NL | OP | QU | GL | SP | ZW) QU_Pi SP* x
    if p != usize::MAX
        && e[p].klasse == Umbruchklasse::Qu
        && ist_pi(e[p].ch)
        && (p == 0
            || matches!(
                e[p - 1].klasse,
                Umbruchklasse::Bk
                    | Umbruchklasse::Cr
                    | Umbruchklasse::Lf
                    | Umbruchklasse::Nl
                    | Umbruchklasse::Op
                    | Umbruchklasse::Qu
                    | Umbruchklasse::Gl
                    | Umbruchklasse::Sp
                    | Umbruchklasse::Zw
            ))
    {
        return false;
    }
    // LB15b: x QU_Pf (SP | GL | WJ | CL | QU | CP | EX | IS | SY | BK | CR | LF | NL | ZW | eot)
    if e[j].klasse == Umbruchklasse::Qu && ist_pf(e[j].ch) {
        if j + 1 == e.len() {
            return false;
        }
        if matches!(
            e[j + 1].klasse,
            Umbruchklasse::Sp
                | Umbruchklasse::Gl
                | Umbruchklasse::Wj
                | Umbruchklasse::Cl
                | Umbruchklasse::Qu
                | Umbruchklasse::Cp
                | Umbruchklasse::Ex
                | Umbruchklasse::Is
                | Umbruchklasse::Sy
                | Umbruchklasse::Bk
                | Umbruchklasse::Cr
                | Umbruchklasse::Lf
                | Umbruchklasse::Nl
                | Umbruchklasse::Zw
        ) {
            return false;
        }
    }
    // LB16: (CL | CP) SP* x NS
    if b == Umbruchklasse::Ns
        && p != usize::MAX
        && matches!(e[p].klasse, Umbruchklasse::Cl | Umbruchklasse::Cp)
    {
        return false;
    }
    // LB17: B2 SP* x B2
    if b == Umbruchklasse::B2 && p != usize::MAX && e[p].klasse == Umbruchklasse::B2 {
        return false;
    }
    // LB18: SP ÷
    if a == Umbruchklasse::Sp {
        return true;
    }
    // LB19: x QU, QU x
    if a == Umbruchklasse::Qu || b == Umbruchklasse::Qu {
        return false;
    }
    // LB20: ÷ CB, CB ÷
    if a == Umbruchklasse::Cb || b == Umbruchklasse::Cb {
        return true;
    }
    // LB21: x BA, x HY, x NS, BB x
    if matches!(b, Umbruchklasse::Ba | Umbruchklasse::Hy | Umbruchklasse::Ns)
        || a == Umbruchklasse::Bb
    {
        return false;
    }
    // LB21a: HL (HY | BA) x
    if matches!(a, Umbruchklasse::Hy | Umbruchklasse::Ba)
        && j >= 2
        && e[j - 2].klasse == Umbruchklasse::Hl
    {
        return false;
    }
    // LB21b: SY x HL
    if a == Umbruchklasse::Sy && b == Umbruchklasse::Hl {
        return false;
    }
    // LB22: x IN
    if b == Umbruchklasse::In_ {
        return false;
    }
    // LB23: (AL | HL) x NU, NU x (AL | HL)
    if (matches!(a, Umbruchklasse::Al | Umbruchklasse::Hl) && b == Umbruchklasse::Nu)
        || (a == Umbruchklasse::Nu && matches!(b, Umbruchklasse::Al | Umbruchklasse::Hl))
    {
        return false;
    }
    // LB23a: PR x (ID | EB | EM), (ID | EB | EM) x PO
    if (a == Umbruchklasse::Pr
        && matches!(b, Umbruchklasse::Id | Umbruchklasse::Eb | Umbruchklasse::Em))
        || (matches!(a, Umbruchklasse::Id | Umbruchklasse::Eb | Umbruchklasse::Em)
            && b == Umbruchklasse::Po)
    {
        return false;
    }
    // LB24: (PR | PO) x (AL | HL), (AL | HL) x (PR | PO)
    if (matches!(a, Umbruchklasse::Pr | Umbruchklasse::Po)
        && matches!(b, Umbruchklasse::Al | Umbruchklasse::Hl))
        || (matches!(a, Umbruchklasse::Al | Umbruchklasse::Hl)
            && matches!(b, Umbruchklasse::Pr | Umbruchklasse::Po))
    {
        return false;
    }
    // LB25 (Beispiel 7): fuenf "Regex-Number"-Regeln.
    if matches!(a, Umbruchklasse::Pr | Umbruchklasse::Po) {
        if b == Umbruchklasse::Nu {
            return false;
        }
        if matches!(b, Umbruchklasse::Op | Umbruchklasse::Hy)
            && j + 1 < e.len()
            && e[j + 1].klasse == Umbruchklasse::Nu
        {
            return false;
        }
    }
    if matches!(a, Umbruchklasse::Op | Umbruchklasse::Hy) && b == Umbruchklasse::Nu {
        return false;
    }
    if a == Umbruchklasse::Nu
        && matches!(b, Umbruchklasse::Nu | Umbruchklasse::Sy | Umbruchklasse::Is)
    {
        return false;
    }
    if matches!(
        b,
        Umbruchklasse::Nu
            | Umbruchklasse::Sy
            | Umbruchklasse::Is
            | Umbruchklasse::Cl
            | Umbruchklasse::Cp
    ) && zahl_laeuft(e, zahl_basis, j)
    {
        return false;
    }
    if matches!(b, Umbruchklasse::Po | Umbruchklasse::Pr)
        && zahl_mit_klammer_laeuft(e, zahl_basis, j)
    {
        return false;
    }
    // LB26: koreanische Silbenbloecke
    if a == Umbruchklasse::Jl
        && matches!(
            b,
            Umbruchklasse::Jl | Umbruchklasse::Jv | Umbruchklasse::H2 | Umbruchklasse::H3
        )
    {
        return false;
    }
    if matches!(a, Umbruchklasse::Jv | Umbruchklasse::H2)
        && matches!(b, Umbruchklasse::Jv | Umbruchklasse::Jt)
    {
        return false;
    }
    if matches!(a, Umbruchklasse::Jt | Umbruchklasse::H3) && b == Umbruchklasse::Jt {
        return false;
    }
    // LB27: Silbenblock wie ID
    if matches!(
        a,
        Umbruchklasse::Jl
            | Umbruchklasse::Jv
            | Umbruchklasse::Jt
            | Umbruchklasse::H2
            | Umbruchklasse::H3
    ) && b == Umbruchklasse::Po
    {
        return false;
    }
    if a == Umbruchklasse::Pr
        && matches!(
            b,
            Umbruchklasse::Jl
                | Umbruchklasse::Jv
                | Umbruchklasse::Jt
                | Umbruchklasse::H2
                | Umbruchklasse::H3
        )
    {
        return false;
    }
    // LB28: (AL | HL) x (AL | HL)
    if matches!(a, Umbruchklasse::Al | Umbruchklasse::Hl)
        && matches!(b, Umbruchklasse::Al | Umbruchklasse::Hl)
    {
        return false;
    }
    // LB28a: Orthographische Silben (AK/AS/DottedCircle, VF/VI). CM ist
    // bereits in die Basis eingezogen; U+25CC steht fuer die Figur.
    let a_silbe = matches!(a, Umbruchklasse::Ak | Umbruchklasse::As) || e[j - 1].ch == PUNKTKREIS;
    let b_silbe = matches!(b, Umbruchklasse::Ak | Umbruchklasse::As) || e[j].ch == PUNKTKREIS;
    if a == Umbruchklasse::Ap && b_silbe {
        return false;
    }
    if a_silbe && matches!(b, Umbruchklasse::Vf | Umbruchklasse::Vi) {
        return false;
    }
    if a == Umbruchklasse::Vi
        && j >= 2
        && (matches!(e[j - 2].klasse, Umbruchklasse::Ak | Umbruchklasse::As)
            || e[j - 2].ch == PUNKTKREIS)
        && b_silbe
    {
        return false;
    }
    if b_silbe && j + 1 < e.len() && e[j + 1].klasse == Umbruchklasse::Vf && a_silbe {
        return false;
    }
    // LB29: IS x (AL | HL)
    if a == Umbruchklasse::Is && matches!(b, Umbruchklasse::Al | Umbruchklasse::Hl) {
        return false;
    }
    // LB30: (AL | HL | NU) x OP, CP x (AL | HL | NU); OP/CP mit
    // East_Asian_Width F/W/H sind ausgenommen.
    if matches!(a, Umbruchklasse::Al | Umbruchklasse::Hl | Umbruchklasse::Nu)
        && b == Umbruchklasse::Op
        && !e[j].breit
    {
        return false;
    }
    if a == Umbruchklasse::Cp
        && !e[j - 1].breit
        && matches!(b, Umbruchklasse::Al | Umbruchklasse::Hl | Umbruchklasse::Nu)
    {
        return false;
    }
    // LB30a: RI RI* x RI nur bei ungerader RI-Anzahl
    if a == Umbruchklasse::Ri && b == Umbruchklasse::Ri && ri_lauf[j - 1] % 2 == 1 {
        return false;
    }
    // LB30b: EB x EM, [Extended_Pictographic & Cn] x EM
    if a == Umbruchklasse::Eb && b == Umbruchklasse::Em {
        return false;
    }
    if b == Umbruchklasse::Em && e[j - 1].pict_cn {
        return false;
    }
    // LB31: ALL ÷
    true
}

/// Liefert die Byte-Offsets erlaubter Zeilenumbrueche.
///
/// Die Offsets liegen zwischen zwei Zeichen (nie 0 oder `text.len()`),
/// sind aufsteigend und hoechstens [`MAX_UMBRUECHE`] viele.
pub fn umbruch_punkte(text: &str) -> Vec<usize> {
    let zerlegt = zerlegen(text);
    let e = &zerlegt.einheiten;
    if e.len() < 2 {
        return Vec::new();
    }
    let vor_sp = vor_sp_tabelle(e);
    let ri_lauf = ri_lauf_tabelle(e);
    let zahl_basis = zahl_basis_tabelle(e);
    let mut punkte = Vec::new();
    for j in 1..e.len() {
        if punkte.len() >= MAX_UMBRUECHE {
            break;
        }
        if entscheide(e, j, &vor_sp, &ri_lauf, &zahl_basis) {
            punkte.push(zerlegt.bytes[j]);
        }
    }
    punkte
}

/// Ist an diesem Byte-Offset ein Zeilenumbruch erlaubt?
///
/// `false` fuer 0, `text.len()`, Positionen ausserhalb und Positionen,
/// die keine Zeichengrenze sind.
pub fn umbruch_erlaubt(text: &str, byte_pos: usize) -> bool {
    if byte_pos == 0 || byte_pos >= text.len() || !text.is_char_boundary(byte_pos) {
        return false;
    }
    let zerlegt = zerlegen(text);
    let j = match zerlegt.bytes.binary_search(&byte_pos) {
        Ok(j) if j > 0 => j,
        _ => return false,
    };
    let vor_sp = vor_sp_tabelle(&zerlegt.einheiten);
    let ri_lauf = ri_lauf_tabelle(&zerlegt.einheiten);
    let zahl_basis = zahl_basis_tabelle(&zerlegt.einheiten);
    entscheide(&zerlegt.einheiten, j, &vor_sp, &ri_lauf, &zahl_basis)
}

/// Release 15.10.2026 (Agent CSS2): is there a soft wrap opportunity between
/// the adjacent characters `before` and `after` because of `word-break:
/// break-all` or `line-break: anywhere` (CSS Text 3 §5.2, §5.3)? Only the
/// opportunities these values ADD; the ordinary ones (spaces, ideographs)
/// are decided elsewhere.
///
/// - Never inside a typographic character unit: not before a combining mark,
///   a spacing mark or ZWJ, not after a prepended mark.
/// - `anywhere`: everywhere else, "disregarding any prohibition against line
///   breaks", glue (U+00A0, U+2060, U+FEFF) included.
/// - `break-all`: letters and digits (UAX #14 AL, HL, NU, SA, and the
///   ideographic and Hangul classes) break like ideographs (ID ÷ ID);
///   punctuation, glue and spaces keep their UAX #14 rules, so no opportunity
///   is added next to them.
pub fn letter_break(before: char, after: char, mode: narsil_core::style::LetterBreaks) -> bool {
    use crate::unicode_daten::graphem_klassen::{Graphemklasse as G, graphem_klasse};
    use narsil_core::style::LetterBreaks;
    if mode == LetterBreaks::None {
        return false;
    }
    if matches!(graphem_klasse(after), G::Extend | G::SpacingMark | G::Zwj)
        || graphem_klasse(before) == G::Prepend
    {
        return false;
    }
    if mode == LetterBreaks::Anywhere {
        return true;
    }
    let letter = |k: Umbruchklasse| {
        use Umbruchklasse as K;
        matches!(
            k,
            K::Al | K::Hl | K::Nu | K::Sa | K::Ai | K::Id | K::Cj | K::H2 | K::H3 | K::Jl | K::Jv | K::Jt | K::Ak
                | K::Ap | K::As
        )
    };
    letter(umbruch_klasse(before)) && letter(umbruch_klasse(after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenzen_sind_immer_gesperrt() {
        assert!(!umbruch_erlaubt("", 0));
        assert!(!umbruch_erlaubt("a", 0));
        assert!(!umbruch_erlaubt("a", 1));
        // "ab": AL x AL -> kein Umbruch (LB28).
        assert!(!umbruch_erlaubt("ab", 1));
        // Ungueltige Grenze im Mehrbyte-Zeichen (1 von 2 Byte).
        assert!(!umbruch_erlaubt("\u{00E4}", 1));
        assert!(!umbruch_erlaubt("a\u{308}", 2));
    }

    #[test]
    fn leerraum_und_bindestrich() {
        assert!(umbruch_erlaubt("a b", 2));
        assert!(!umbruch_erlaubt("a b", 1));
        // Bindestrich (HY): Umbruch danach erlaubt.
        assert!(umbruch_erlaubt("a-b", 2));
    }

    #[test]
    fn cjk_zwischen_ideogrammen() {
        assert!(umbruch_erlaubt("\u{65E5}\u{672C}", 3));
        let punkte = umbruch_punkte("\u{65E5}\u{672C}\u{8A9E}");
        assert_eq!(punkte, vec![3, 6]);
    }

    #[test]
    fn keine_panik_bei_randfaellen() {
        for text in [
            "",
            " ",
            "\n",
            "\u{0}",
            "\u{10FFFF}",
            "\u{200D}",
            "\u{308}",
            "a\u{308}",
        ] {
            let _ = umbruch_punkte(text);
            for pos in 0..=text.len() {
                let _ = umbruch_erlaubt(text, pos);
            }
        }
    }

    #[test]
    fn max_umbrueche_wird_eingehalten() {
        let text = "a ".repeat(MAX_UMBRUECHE + 10);
        let punkte = umbruch_punkte(&text);
        assert_eq!(punkte.len(), MAX_UMBRUECHE);
    }

    #[test]
    fn bombe_eine_million_zeichen() {
        let text = "wort ".repeat(200_000);
        assert_eq!(text.chars().count(), 1_000_000);
        let punkte = umbruch_punkte(&text);
        // Nach jedem Leerzeichen ausser dem letzten (dahinter folgt nichts).
        assert_eq!(punkte.len(), 199_999);
        assert!(punkte.len() <= MAX_UMBRUECHE);
    }
}
