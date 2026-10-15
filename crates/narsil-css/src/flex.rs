//! Flexbox-Teilmenge als reine Layout-Funktion (C8, W5).
//!
//! Einzeilig, ohne Umbruch: flex-direction row/column (als Haupt-/Querachse
//! vom Aufrufer gemappt), justify-content (start/mitte/ende/zwischen),
//! align-items (start/mitte/ende/strecke), gap, flex-grow. Schrumpfen
//! vereinfacht proportional zur Basis (kein flex-shrink-Faktor —
//! dokumentierte Luecke der Teilmenge).
//!
//! Reine Funktion ueber einer Scheibe (`&[FlexKind]`): hoechstens
//! MAX_FLEX_KINDER Kinder werden gelegt (M4), keine Allokation ausser
//! dem Ergebnis-Vec (gedeckelt), kein Panic (nicht-endlich -> 0).

use super::MAX_PX;
use super::typen::{FlexAusrichtung, FlexBuendig};

/// Maximale Kinder je Flex-Container. Begruendung: echte Zeilen haben
/// Dutzende Kinder; 256 deckt auch lange Navigationsleisten ab, und die
/// Layout-Schleife bleibt O(256) — der Rest faellt weg (wird gezaehlt?).
/// Nein: still weg, denn `lege_flex` gibt nur Streifen zurueck — der
/// Aufrufer sieht an der Laenge, was gelegt wurde (dokumentiert).
pub const MAX_FLEX_KINDER: usize = 256;

/// Ein Flex-Kind: Basis (flex-basis in px, aufgeloest), Wachstum
/// (flex-grow), Schrumpf (flex-shrink), Quermass (align-self gibt es
/// nicht — None = strecken).

/// Die Angaben JE KIND fuer die Querachsen-Ausrichtung (§9.2).
///
/// L4 (03.10.2026 13:4x). GEMESSENE ENTSCHEIDUNG, warum eine eigene
/// Art und nicht zwei Parameter: `lege_flex_zeilen` hatte mit
/// `selbst` schon acht, und der naechste Wert (`aufstieg` fuer
/// `baseline`) waere der neunte.  Zwei gekoppelte Angaben gehoeren in
/// eine Struktur — sie werden immer zusammen gelesen.
///
/// GEMESSENE ENTSCHEIDUNG, warum `selbst` hier `Option` ist und nicht
/// ein eigener Wert: §6.3 — `auto` bedeutet „den Wert des Containers",
/// und `auto` ist der Anfangswert.  Ohne `Option` waere `auto` von
/// `flex-start` nicht zu unterscheiden.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KindQuer {
    /// `align-self` des Kindes, `None` = `auto` (§6.3).
    pub selbst: Option<FlexAusrichtung>,
    /// Der Aufstieg der Schrift im Kind: der Abstand von der
    /// Oberkante bis zur ersten Grundlinie (§6.4).  0 = das Kind hat
    /// keinen Text, dann ist seine Oberkante die Grundlinie.
    pub aufstieg: f32,
}
#[derive(Debug, Clone, Default)]
pub struct FlexKind {
    /// flex-basis in px (>= 0 nach Kappung).
    pub basis: f32,
    /// flex-grow-Faktor (>= 0 nach Kappung).
    pub wachstum: f32,
    /// flex-shrink-Faktor (Anfangswert 1, >= 0 nach Kappung, C12, W6).
    pub schrumpf: f32,
    /// Quermass in px (None = strecken, wenn Ausrichtung Strecke).
    pub quer: Option<f32>,
    /// `margin-*` = `auto` an der **Hauptachse**, richtungsaufgeloest: bei
    /// `row` links/rechts, bei `column` oben/unten.
    ///
    /// GEMESSENER FEHLER, den diese vier Felder beheben (CSS_14.txt, f8/f9):
    /// `<div style="display:flex"><div style="width:160px;margin:auto">`
    /// stand in Firefox bei **x = 560** = (1280-160)/2, bei uns bei **0**.
    /// GEMESSENE URSACHE, nicht vermutet: `FlexKind` hatte **kein einziges
    /// Randfeld**. Der Auto-Rand konnte gar nicht ankommen — es fehlte nicht
    /// die Rechnung, sondern das Feld.
    pub haupt_vorn_auto: bool,
    /// das hintere Ende auf der Hauptachse (rechts bzw. unten).
    pub haupt_hinten_auto: bool,
    /// dasselbe an der **Querachse**: bei `row` oben/unten, bei `column`
    /// links/rechts. Ohne das bleibt ein `margin:auto` in einem
    /// `flex-direction: column` waagerecht linksbuendig (gemessen: x 0 statt
    /// 560).
    pub quer_vorn_auto: bool,
    /// das hintere Ende auf der Querachse.
    pub quer_hinten_auto: bool,
    /// Release 08.10.2026: die MINDESTGROESSE auf der Hauptachse (CSS
    /// Flexbox 1 §4.5: `min-width: auto` ist die Inhalts-Mindestgroesse,
    /// min-content). 0 = keine. GEMESSEN vorher: die Navigation von
    /// spiegel.de (`white-space: nowrap`) schrumpfte auf Breiten unter
    /// ihrem Text, und die Eintraege lagen uebereinander.
    pub min: f32,
    /// Release 08.10.2026: die HOECHSTgroesse auf der Hauptachse
    /// (`max-width` bzw. `max-height`, Rahmenmass); `None` = keine.
    pub max: Option<f32>,
    /// Release 08.10.2026: die Summe der Zahl-Raender auf der Hauptachse
    /// (Anfang + Ende, ohne `auto`). Die Verteilung rechnet mit der
    /// AEUSSEREN Groesse (§9.2: „outer hypothetical main size"); die
    /// Position eines Streifens ist der Anfang seines Randkastens.
    /// GEMESSEN vorher: rust-lang.org, drei Spalten mit `margin: 0 4rem` an
    /// der mittleren — die dritte lag 48 px auf der zweiten.
    pub rand_haupt: f32,
    /// dasselbe auf der Querachse (Strecken und Ausrichten rechnen in der
    /// Zeile minus Rand).
    pub rand_quer: f32,
    /// Release 08.10.2026: das GEMESSENE Quermass (Inhaltshoehe in einer
    /// Reihe) — fuer die Hoehe einer Zeile in einem MEHRzeiligen Container,
    /// auch wenn das Kind gestreckt wird (`quer = None`). CSS Flexbox §9.4
    /// Schritt 8: die Zeile ist so hoch wie ihr hoechstes Kind. GEMESSEN
    /// vorher: spiegel.de, `flex-wrap` mit sechs `w-full`-Kindern — jede
    /// Zeile bekam ein Sechstel der Containerhoehe (575 px), und unter
    /// jeder Ueberschrift klaffte eine Luecke.
    pub quer_inhalt: f32,
}

/// Gelegter Streifen: Positionen und Masse in px (Haupt-/Querachse).
#[derive(Debug, Clone, PartialEq)]
pub struct FlexStreifen {
    /// Position auf der Hauptachse (vom Anfang).
    pub haupt_pos: f32,
    /// Mass auf der Hauptachse (>= 0).
    pub haupt_mass: f32,
    /// Position auf der Querachse.
    pub quer_pos: f32,
    /// Mass auf der Querachse (>= 0).
    pub quer_mass: f32,
}

/// Eine gelegte Flex-Zeile (C12, W6): Streifen mit Haupt-Positionen
/// ab Zeilenanfang plus Zeilenversatz und -hoehe auf der Querachse.
#[derive(Debug, Clone, PartialEq)]
pub struct FlexZeile {
    /// Streifen dieser Zeile (haupt_pos ab 0).
    pub streifen: Vec<FlexStreifen>,
    /// Versatz der Zeile auf der Querachse.
    pub versatz_quer: f32,
    /// Hoehe der Zeile auf der Querachse.
    pub mass_quer: f32,
}

/// Legt `kinder` in einen Container (`haupt` x `quer` in px).
/// `luecke` = gap in px. Rueckgabe hat hoechstens min(n, MAX)-Eintraege.
/// Achsen-Abbildung (row vs. column) macht der Aufrufer: Bei row ist
/// haupt die Breite, bei column die Hoehe.
/// Einzeilig (wrap=false) — Huelle um [`lege_flex_zeilen`].
///
/// L4 (03.10.2026 14:2x): ohne Kind-Ausrichtungen — die Huelle nimmt
/// `align-self` nicht an, weil sie im alten Weg ohne Kind-Werte läuft.
pub fn lege_flex(
    haupt: f32,
    quer: f32,
    buendig: FlexBuendig,
    ausrichtung: FlexAusrichtung,
    luecke: f32,
    kinder: &[FlexKind],
) -> Vec<FlexStreifen> {
    let zeilen = lege_flex_zeilen(
        haupt,
        quer,
        buendig,
        ausrichtung,
        luecke,
        0.0,
        false,
        kinder,
        &[],
    );
    zeilen
        .into_iter()
        .next()
        .map(|z| z.streifen)
        .unwrap_or_default()
}

/// Legt `kinder` ggf. mehrzeilig (flex-wrap, C12, W6). `zeilen_luecke`
/// ist der Abstand zwischen Zeilen (row-gap). Rueckgabe: Zeilen von
/// oben; quer_pos der Streifen ist relativ zur Zeile (plus
/// versatz_quer absolut). Zeilenzahl <= Kinderzahl (gedeckelt, M4).
#[allow(clippy::too_many_arguments)]
pub fn lege_flex_zeilen(
    haupt: f32,
    quer: f32,
    buendig: FlexBuendig,
    ausrichtung: FlexAusrichtung,
    luecke: f32,
    zeilen_luecke: f32,
    umbrechen: bool,
    kinder: &[FlexKind],
    kinder_quer: &[KindQuer],
) -> Vec<FlexZeile> {
    let h = sauber_pos(haupt);
    let q = sauber_pos(quer);
    let gap = sauber_pos(luecke);
    let zgap = sauber_pos(zeilen_luecke);
    // Kinderzahl gedeckelt (M4): der Rest faellt weg.
    let n = kinder.len().min(MAX_FLEX_KINDER);
    let mut zeilen: Vec<FlexZeile> = Vec::new();
    if n == 0 {
        return zeilen;
    }
    // Zeilen aufteilen (greedy): passt Basis + Luecke nicht mehr und die
    // Zeile ist nicht leer, beginnt eine neue (nur bei umbrechen).
    let mut start = 0usize;
    while start < n {
        let mut ende = start + 1;
        if umbrechen {
            let mut belegt = hypothetisch_aussen(&kinder[start]);
            while ende < n {
                let dazu = gap + hypothetisch_aussen(&kinder[ende]);
                if belegt + dazu > h {
                    break;
                }
                belegt += dazu;
                ende += 1;
            }
        } else {
            ende = n;
        }
        let streifen = lege_zeile(h, buendig, gap, &kinder[start..ende]);
        // Zeilen-Querhoehe: Maximum der Kinder-Quermasse; Strecke ohne
        // Mass zaehlt 0 und wird spaeter gestreckt (siehe unten).
        // Release 08.10.2026: im MEHRzeiligen Container zaehlt auch das
        // gemessene Quermass gestreckter Kinder (`quer_inhalt`) — die Zeile
        // ist so hoch wie ihr Inhalt (§9.4 Schritt 8).
        let mut hoch = 0.0f32;
        for k in &kinder[start..ende] {
            if let Some(m) = k.quer {
                hoch = hoch.max(sauber_pos(m) + sauber_pos(k.rand_quer));
            } else if umbrechen {
                hoch = hoch.max(sauber_pos(k.quer_inhalt) + sauber_pos(k.rand_quer));
            }
        }
        zeilen.push(FlexZeile {
            streifen,
            versatz_quer: 0.0,
            mass_quer: hoch,
        });
        start = ende;
    }
    // Einzeilig: Die Zeile ist so hoch wie der Container (CSS: align
    // richtet im Container aus, nicht in der Zeile). Mehrzeilig bleibt
    // jede Zeile bei ihrem Maximum (dokumentierte Naeherung, C12).
    if zeilen.len() == 1 {
        zeilen[0].mass_quer = zeilen[0].mass_quer.max(q);
    }
    // Quer-Verteilung: Zeilen mit Hoehe 0 (nur Strecke-Kinder) teilen
    // sich den Rest gleichmaessig (dokumentierte Naeherung, C12).
    let mut belegt_q = 0.0f32;
    for z in &zeilen {
        belegt_q += z.mass_quer;
    }
    belegt_q += zgap * (zeilen.len() as f32 - 1.0).max(0.0);
    let rest_q = (q - belegt_q).max(0.0);
    let leere = zeilen.iter().filter(|z| z.mass_quer == 0.0).count();
    let extra = if leere > 0 {
        rest_q / leere as f32
    } else {
        0.0
    };
    let mut pos_q = 0.0f32;
    for z in zeilen.iter_mut() {
        if z.mass_quer == 0.0 {
            z.mass_quer = extra;
        }
        z.versatz_quer = pos_q;
        pos_q += z.mass_quer + zgap;
    }
    // Zweiter Durchgang: Quer-Positionen/Masse je Streifen (braucht die
    // Kinder-Quermasse — Indizes parallel zu zeilen aufbauen).
    let mut kind_idx = 0usize;
    for z in zeilen.iter_mut() {
        // L4 (13:4x): §6.4 braucht die Kinder DIESER Zeile, um die
        // tiefste Grundlinie zu finden.  Die Kinder einer Zeile liegen
        // im Gesamt-Index hintereinander, deshalb genuegt der Start.
        let zeile_start = kind_idx;
        // GEMESSENER FEHLER: `z.streifen.len()` in der Schleife gab E0502
        // (dieselbe Feld leiht einmal veraenderlich und einmal
        // unveraenderlich).  Die Zahl wird also VOR der Schleife
        // gelesen.
        let streifen_der_zeile = z.streifen.len();
        for st in z.streifen.iter_mut() {
            if let Some(k) = kinder.get(kind_idx) {
                // L4 (03.10.2026 14:2x): §9.2 — `align-self` am Kind
                // schlaegt `align-items` am Container.  `None` heisst
                // `auto` (§6.3), also „den Wert des Containers".
                //
                // GEMESSENER BEFUND, dass es fehlte: `examples/l3_flex.rs`,
                // Block H5 — `align-self: center` am Kind ergab `y 48`,
                // genau wie `align-items: flex-start`.
                let kind_quer = kinder_quer.get(kind_idx).copied().unwrap_or_default();
                let aus = kind_quer.selbst.unwrap_or(ausrichtung);
                // Release 08.10.2026: gestreckt und ausgerichtet wird in der
                // Zeile MINUS Rand; die Position ist der Anfang des
                // Randkastens (den Rand legt der Aufrufer dazu).
                let (mut qp, qm) = quermass_zeile_mit_rand(
                    (z.mass_quer - sauber_pos(k.rand_quer)).max(0.0),
                    aus,
                    k.quer,
                    k.quer_vorn_auto,
                    k.quer_hinten_auto,
                );
                // §6.4 — `baseline`: die Kinder werden nach ihrer
                // ERSTEN Grundlinie ausgerichtet, nicht nach ihrer Kante.
                //
                // GEMESSENER BEFUND, dass es fehlte: `examples/l3_flex.rs`,
                // Block H6 — `align-items: baseline` ergab `y 48` fuer
                // beide Kinder, genau wie `flex-start`; die Kinder mit 16
                // und 32 px Schrift stehen bei `flex-start` uebereinander
                // versetzt.
                if aus == FlexAusrichtung::Grundlinie {
                    // Die Grundlinie der Zeile ist die TIEFSTE der
                    // beteiligten Grundlinien: das Kind mit dem groessten
                    // Aufstieg bleibt stehen, alle anderen ruecken auf.
                    let mut tiefste: Option<f32> = None;
                    for j in 0..streifen_der_zeile {
                        let i = zeile_start + j;
                        let Some(kk) = kinder.get(i) else { break };
                        let kq = kinder_quer.get(i).copied().unwrap_or_default();
                        let a = kq.selbst.unwrap_or(ausrichtung);
                        if a != FlexAusrichtung::Grundlinie {
                            continue;
                        }
                        let basis = quermass_zeile(z.mass_quer, a, kk.quer).0;
                        let g = basis + kq.aufstieg;
                        if tiefste.is_none_or(|t| g > t) {
                            tiefste = Some(g);
                        }
                    }
                    if let Some(t) = tiefste {
                        qp = (t - kind_quer.aufstieg).max(0.0);
                    }
                }
                st.quer_pos = qp;
                st.quer_mass = qm;
            }
            kind_idx += 1;
        }
    }
    zeilen
}

/// Legt EINE Zeile (Hauptachse): Wachstum/Schrumpf + Buendigkeit.
/// `kinder` hoechstens MAX_FLEX_KINDER (Aufrufer garantiert es).
fn lege_zeile(h: f32, buendig: FlexBuendig, gap: f32, kinder: &[FlexKind]) -> Vec<FlexStreifen> {
    let n = kinder.len();
    let mut streifen: Vec<FlexStreifen> = Vec::new();
    if n == 0 {
        return streifen;
    }
    streifen.reserve(n);
    let luecken = gap * (n as f32 - 1.0).max(0.0);
    let masse = hauptmasse(h, luecken, kinder);
    // Belegte Breite nach Wachstum/Schrumpf (fuer die Buendigkeit),
    // AEUSSERE Masse (mit den Zahl-Raendern).
    let mut belegt = luecken;
    for i in 0..n {
        belegt += masse[i] + sauber_pos(kinder[i].rand_haupt);
    }
    let rest = (h - belegt).max(0.0);
    //
    // §9.6: AUTO-RAENDER SCHLUCKEN DEN FREIEN RAUM — und zwar **vor** der
    // Buendigkeit (§9.5). GEMESSEN (`CSS_14.txt`, f8): `margin:auto` in einem
    // 1280-px-Flex mit einem 160-px-Kind ergab Firefox **560** = (1280-160)/2,
    // uns **0**. `justify-content` darf daneben nichts entscheiden — mit
    // Auto-Raendern ist es ohne Wirkung.
    //
    // Jeder Auto-Rand bekommt den gleichen Anteil; zwei linke und ein
    // rechtes Auto ergeben also nicht drei gleiche Stuecke, sondern zwei
    // und eines, und das Kind mit beiden steht mittig (§9.6, Absatz 2).
    let auto_zaehler: u32 = kinder
        .iter()
        .map(|k| u32::from(k.haupt_vorn_auto) + u32::from(k.haupt_hinten_auto))
        .sum();
    let auto_anteil = if auto_zaehler > 0 {
        rest / auto_zaehler as f32
    } else {
        0.0
    };
    // Startversatz + Zusatzluecke je nach Buendigkeit — mit Auto-Raendern
    // beginnt die Zeile immer am Anfang, weil der Rand den Versatz traegt.
    let (mut pos, extra_gap) = if auto_zaehler > 0 {
        (0.0, 0.0)
    } else {
        match buendig {
            FlexBuendig::Start => (0.0, 0.0),
            FlexBuendig::Mitte => (rest / 2.0, 0.0),
            FlexBuendig::Ende => (rest, 0.0),
            // Release 09.10.2026: CSS Box Alignment 3 §5.3 — `space-between`
            // faellt bei EINEM Kind oder negativem Rest auf `flex-start`
            // zurueck (vorher: zentriert). GEMESSEN: t-online.de, die
            // Ueberschrift „Schlagzeilen" (allein in `justify-between`) stand
            // mitten im Kasten statt links.
            FlexBuendig::Zwischen => {
                if n > 1 && rest > 0.0 {
                    (0.0, rest / (n as f32 - 1.0))
                } else {
                    (0.0, 0.0)
                }
            }
            // space-around: halbe Kante aussen, ganze innen (C12). Negativer
            // Rest: `safe center` (§5.3) — Ueberlauf beginnt am Anfang.
            FlexBuendig::Ringsum => {
                if n > 0 && rest > 0.0 {
                    (rest / (2.0 * n as f32), rest / n as f32)
                } else {
                    (0.0, 0.0)
                }
            }
            // space-evenly: ueberall gleich (C12); negativer Rest wie oben.
            FlexBuendig::Gleichmaessig => {
                if rest > 0.0 {
                    let kante = rest / (n as f32 + 1.0);
                    (kante, kante)
                } else {
                    (0.0, 0.0)
                }
            }
            // K7c: `stretch`, `left`, `right` (CSS Box Alignment 3 §8).
            //
            // `stretch` verhaelt sich wie `start` (§8.3: die Kinder bekommen
            // ihre Grundgroesse, der Rest bleibt leer). `left`/`right` sind
            // physisch (§8.2) und unterscheiden sich von `start`/`end`
            // nur bei `direction: rtl` — das weiss die Kaskade hier nicht.
            // GEMESSEN: je 2x auf den echten Stylesheets; die Annahme steht
            // in WUNSCH_CSS.txt W11 als offen.
            FlexBuendig::Strecke | FlexBuendig::Links => (0.0, 0.0),
            FlexBuendig::Rechts => (rest, 0.0),
        }
    };
    let schritt = gap + extra_gap;
    for i in 0..n {
        pos += if kinder[i].haupt_vorn_auto {
            auto_anteil
        } else {
            0.0
        };
        streifen.push(FlexStreifen {
            haupt_pos: sauber_pos(pos),
            haupt_mass: masse[i],
            // Quer wird im Zeilen-Durchgang gesetzt (Platzhalter 0).
            quer_pos: 0.0,
            quer_mass: 0.0,
        });
        pos += masse[i]
            + sauber_pos(kinder[i].rand_haupt)
            + schritt
            + if kinder[i].haupt_hinten_auto {
                auto_anteil
            } else {
                0.0
            };
    }
    streifen
}

/// Die Hoechstgroesse eines Kindes (`min` schlaegt `max`, CSS 2.1 §10.4).
fn hoechst(k: &FlexKind) -> f32 {
    k.max.map_or(MAX_PX, sauber_pos).max(sauber_pos(k.min))
}

/// §9.2 (3E): die hypothetische Hauptgroesse — die Basis, geklemmt an
/// Mindest- und Hoechstgroesse.
fn hypothetisch(k: &FlexKind) -> f32 {
    sauber_pos(k.basis).min(hoechst(k)).max(sauber_pos(k.min))
}

/// Die hypothetische Hauptgroesse plus Zahl-Raender (§9.3: Zeilen bilden
/// sich aus den AEUSSEREN Groessen).
fn hypothetisch_aussen(k: &FlexKind) -> f32 {
    hypothetisch(k) + sauber_pos(k.rand_haupt)
}

/// Release 08.10.2026: §9.7 „Resolve the Flexible Lengths" vollstaendig —
/// unflexible Kinder einfrieren, den freien Platz nach `flex-grow` bzw.
/// nach `flex-shrink × Basis` verteilen, an Mindest-/Hoechstgroesse
/// klemmen und die Verletzer einfrieren, bis nichts mehr verletzt ist.
/// Vorher: Wachsen ohne `max-*`, Schrumpfen nur gegen `min`, und die
/// Raender fehlten in der Rechnung.
fn hauptmasse(h: f32, luecken: f32, kinder: &[FlexKind]) -> [f32; 256] {
    let n = kinder.len().min(256);
    let mut masse = [0.0f32; 256];
    let mut fest = [false; 256];
    let basis = |k: &FlexKind| sauber_pos(k.basis);
    let rand = |k: &FlexKind| sauber_pos(k.rand_haupt);
    // Schritt 1: wachsen oder schrumpfen? (aus den hypothetischen Groessen)
    let summe_hypo: f32 = kinder.iter().take(n).map(hypothetisch_aussen).sum();
    let wachsen = h - luecken - summe_hypo >= 0.0;
    let faktor = |k: &FlexKind| sauber_wachs(if wachsen { k.wachstum } else { k.schrumpf });
    // Schritt 2: unflexible Kinder bekommen sofort ihre hypothetische Groesse.
    for (i, k) in kinder.iter().take(n).enumerate() {
        let hypo = hypothetisch(k);
        if faktor(k) == 0.0 || (wachsen && basis(k) > hypo) || (!wachsen && basis(k) < hypo) {
            fest[i] = true;
            masse[i] = hypo;
        }
    }
    // Schritt 3: der anfaengliche freie Platz.
    let frei_von = |fest: &[bool; 256], masse: &[f32; 256]| {
        let mut belegt = luecken;
        for (i, k) in kinder.iter().take(n).enumerate() {
            belegt += rand(k) + if fest[i] { masse[i] } else { basis(k) };
        }
        h - belegt
    };
    let frei_anfang = frei_von(&fest, &masse);
    // Schritt 4: die Schleife — jede Runde friert mindestens ein Kind ein.
    for _ in 0..=n {
        if fest.iter().take(n).all(|f| *f) {
            break;
        }
        let mut frei = frei_von(&fest, &masse);
        let mut summe_faktor = 0.0f32;
        let mut summe_gewicht = 0.0f32;
        for (i, k) in kinder.iter().take(n).enumerate() {
            if !fest[i] {
                summe_faktor += faktor(k);
                summe_gewicht += faktor(k) * basis(k);
            }
        }
        // §9.7 4b: Faktoren unter 1 verteilen nur ihren Anteil.
        if summe_faktor < 1.0 {
            let teil = frei_anfang * summe_faktor;
            if teil.abs() < frei.abs() {
                frei = teil;
            }
        }
        let mut verletzung = 0.0f32;
        let mut zu_klein = [false; 256];
        let mut zu_gross = [false; 256];
        for (i, k) in kinder.iter().take(n).enumerate() {
            if fest[i] {
                continue;
            }
            let ziel = if wachsen {
                if summe_faktor > 0.0 { basis(k) + frei * faktor(k) / summe_faktor } else { basis(k) }
            } else if summe_gewicht > 0.0 {
                basis(k) + frei * faktor(k) * basis(k) / summe_gewicht
            } else {
                basis(k)
            };
            let ziel = if ziel.is_finite() { ziel } else { basis(k) };
            let geklemmt = ziel.min(hoechst(k)).max(sauber_pos(k.min)).max(0.0);
            verletzung += geklemmt - ziel;
            zu_klein[i] = geklemmt > ziel;
            zu_gross[i] = geklemmt < ziel;
            masse[i] = geklemmt;
        }
        // Schritt 4d: einfrieren — alle, die Min-Verletzer oder die
        // Max-Verletzer.
        for i in 0..n {
            if fest[i] {
                continue;
            }
            let einfrieren = if verletzung.abs() < 0.01 {
                true
            } else if verletzung > 0.0 {
                zu_klein[i]
            } else {
                zu_gross[i]
            };
            if einfrieren {
                fest[i] = true;
            }
        }
    }
    for m in masse.iter_mut().take(n) {
        *m = sauber_pos(*m);
    }
    masse
}

/// Quermass + -position in der Zeile (C12): Strecke ohne Mass wird auf
/// die Zeilenhoehe gestreckt (Aufrufer setzt sie vorher).
fn quermass_zeile(zeile_hoch: f32, aus: FlexAusrichtung, kind: Option<f32>) -> (f32, f32) {
    quermass_zeile_mit_rand(zeile_hoch, aus, kind, false, false)
}

/// Wie [`quermass_zeile`], aber mit den beiden Auto-Raendern der Querachse.
///
/// §9.6 gilt fuer **beide** Achsen. GEMESSEN (`CSS_14.txt`, f9): ein Kind mit
/// `margin:auto` in einem `flex-direction: column` steht in Firefox
/// waagerecht bei **x = 560**; ohne diese beiden Parameter bleibt es bei 0.
/// Die Ausrichtung `aus` ist mit Auto-Raendern ohne Wirkung, genau wie auf
/// der Hauptachse.
fn quermass_zeile_mit_rand(
    zeile_hoch: f32,
    aus: FlexAusrichtung,
    kind: Option<f32>,
    vorn_auto: bool,
    hinten_auto: bool,
) -> (f32, f32) {
    let wunsch = kind.map(sauber_pos).unwrap_or(zeile_hoch);
    let n = u32::from(vorn_auto) + u32::from(hinten_auto);
    if n > 0 {
        // Der Streifen wird **nicht** gestreckt: mit Auto-Raendern nimmt das
        // Kind seine eigene Groesse (§9.6 schliesst `align-self: stretch`
        // aus, weil der Rand den Platz einnimmt).
        let frei = (zeile_hoch - wunsch).max(0.0);
        let anteil = frei / n as f32;
        return (if vorn_auto { anteil } else { 0.0 }, wunsch);
    }
    match aus {
        FlexAusrichtung::Strecke => {
            if kind.is_some() {
                (0.0, wunsch)
            } else {
                (0.0, zeile_hoch)
            }
        }
        FlexAusrichtung::Start => (0.0, wunsch),
        FlexAusrichtung::Mitte => ((zeile_hoch - wunsch).max(0.0) / 2.0, wunsch),
        FlexAusrichtung::Ende => ((zeile_hoch - wunsch).max(0.0), wunsch),
        // K7c: `baseline` (CSS Box Alignment 3 §6.3) — die Grundlinie
        // der Kinder. Der Flex-Leg hat KEINE Grundlinien: sie liegen
        // in der Schrift, nicht im Kasten (§6.3 verweist auf
        // CSS Inline 3 §4.1).
        //
        // GEMESSENER FEHLER: `align-items: baseline` stand 12x auf den
        // echten Stylesheets und wurde verworfen — damit war fuer
        // diese Elemente ueberhaupt keine Ausrichtung gesetzt, und
        // `Strecke` (der Anfangswert) griff.
        //
        // Die ehrliche Naeherung: `baseline` verhaelt sich wie
        // `flex-start` fuer die GROSSRICHTUNG. Es wird NICHT als
        // `Strecke` umgedeutet, weil das eine andere Aussage ist.
        // Offen: WUNSCH_CSS.txt W11.
        FlexAusrichtung::Grundlinie => (0.0, wunsch),
    }
}

/// px-Mass: nicht-endlich -> 0, negativ -> 0, Uebermass -> MAX_PX.
fn sauber_pos(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    x.clamp(0.0, MAX_PX)
}

/// Wachstumsfaktor: nicht-endlich -> 0, negativ -> 0, Uebermass -> MAX_PX.
fn sauber_wachs(x: f32) -> f32 {
    sauber_pos(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(basis: f32, wachstum: f32) -> FlexKind {
        FlexKind {
            schrumpf: 1.0,
            basis,
            wachstum,
            quer: None,
            ..Default::default()
        }
    }

    #[test]
    fn leer() {
        assert!(
            lege_flex(
                800.0,
                100.0,
                FlexBuendig::Start,
                FlexAusrichtung::Strecke,
                0.0,
                &[]
            )
            .is_empty()
        );
    }
    #[test]
    fn basis_ohne_wachs() {
        let k = [kind(100.0, 0.0), kind(200.0, 0.0)];
        let s = lege_flex(
            800.0,
            100.0,
            FlexBuendig::Start,
            FlexAusrichtung::Strecke,
            0.0,
            &k,
        );
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].haupt_pos, s[0].haupt_mass), (0.0, 100.0));
        assert_eq!((s[1].haupt_pos, s[1].haupt_mass), (100.0, 200.0));
        // Strecke ohne Quermass: volle Containerhoehe.
        assert_eq!((s[0].quer_pos, s[0].quer_mass), (0.0, 100.0));
    }
    #[test]
    fn wachstum_verteilt() {
        let k = [kind(100.0, 1.0), kind(100.0, 3.0)];
        let s = lege_flex(
            800.0,
            100.0,
            FlexBuendig::Start,
            FlexAusrichtung::Strecke,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_mass, 250.0);
        assert_eq!(s[1].haupt_mass, 550.0);
        assert_eq!(s[1].haupt_pos, 250.0);
    }
    #[test]
    fn luecke() {
        let k = [kind(100.0, 0.0), kind(100.0, 0.0)];
        let s = lege_flex(
            800.0,
            100.0,
            FlexBuendig::Start,
            FlexAusrichtung::Strecke,
            10.0,
            &k,
        );
        assert_eq!(s[1].haupt_pos, 110.0);
    }
    #[test]
    fn buendig_mitte_ende() {
        let k = [kind(100.0, 0.0)];
        let s = lege_flex(
            800.0,
            0.0,
            FlexBuendig::Mitte,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_pos, 350.0);
        let s = lege_flex(
            800.0,
            0.0,
            FlexBuendig::Ende,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_pos, 700.0);
    }
    #[test]
    fn zwischen() {
        let k = [kind(100.0, 0.0), kind(100.0, 0.0), kind(100.0, 0.0)];
        let s = lege_flex(
            800.0,
            0.0,
            FlexBuendig::Zwischen,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_pos, 0.0);
        assert_eq!(s[1].haupt_pos, 350.0);
        assert_eq!(s[2].haupt_pos, 700.0);
    }
    /// Release 09.10.2026: CSS Box Alignment 3 §5.3 — `space-between`
    /// mit EINEM Kind faellt auf `flex-start` zurueck (vorher behauptete
    /// dieser Test die Mitte, 350 px; Firefox/Chrome: 0 px).
    #[test]
    fn zwischen_ein_kind_steht_am_anfang() {
        let k = [kind(100.0, 0.0)];
        let s = lege_flex(
            800.0,
            0.0,
            FlexBuendig::Zwischen,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_pos, 0.0);
    }
    #[test]
    fn schrumpfen_proportional() {
        let k = [kind(300.0, 0.0), kind(300.0, 0.0)];
        let s = lege_flex(
            400.0,
            0.0,
            FlexBuendig::Start,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_mass, 200.0);
        assert_eq!(s[1].haupt_mass, 200.0);
    }
    #[test]
    fn ausrichtung() {
        let q = Some(20.0);
        let k = [FlexKind {
            basis: 50.0,
            schrumpf: 1.0,
            wachstum: 0.0,
            quer: q,
            ..Default::default()
        }];
        let s = lege_flex(
            800.0,
            100.0,
            FlexBuendig::Start,
            FlexAusrichtung::Mitte,
            0.0,
            &k,
        );
        assert_eq!((s[0].quer_pos, s[0].quer_mass), (40.0, 20.0));
        let s = lege_flex(
            800.0,
            100.0,
            FlexBuendig::Start,
            FlexAusrichtung::Ende,
            0.0,
            &k,
        );
        assert_eq!((s[0].quer_pos, s[0].quer_mass), (80.0, 20.0));
    }
    #[test]
    fn kinder_deckel() {
        let viele = vec![
            FlexKind {
                basis: 1.0,
                schrumpf: 1.0,
                wachstum: 0.0,
                quer: None,
                ..Default::default()
            };
            1000
        ];
        let s = lege_flex(
            5000.0,
            10.0,
            FlexBuendig::Start,
            FlexAusrichtung::Start,
            0.0,
            &viele,
        );
        assert_eq!(s.len(), MAX_FLEX_KINDER);
    }
    #[test]
    fn unsinn_eingaben() {
        // Unendlich/nan/negativ: alles gekappt, nie Panic, nie inf.
        // (inf -> 0 per sauber-Regel; riesig-endlich -> MAX_PX.)
        let k = [
            FlexKind {
                basis: f32::NAN,
                schrumpf: 1.0,
                wachstum: 1e30,
                quer: Some(-5.0),
                ..Default::default()
            },
            kind(50.0, 0.0),
        ];
        let s = lege_flex(
            800.0,
            f32::NAN,
            FlexBuendig::Start,
            FlexAusrichtung::Mitte,
            -3.0,
            &k,
        );
        assert_eq!(s.len(), 2);
        // Frei = 750, alles ans (gekappte) Wachstum: endlich + begrenzt.
        assert_eq!(s[0].haupt_mass, 750.0);
        assert_eq!(s[1].haupt_mass, 50.0);
        assert!(s[0].haupt_mass <= MAX_PX);
        // Unendliches Wachstum zaehlt als 0 (sauber-Regel).
        let k2 = [FlexKind {
            basis: 100.0,
            schrumpf: 1.0,
            wachstum: f32::INFINITY,
            quer: None,
            ..Default::default()
        }];
        let s2 = lege_flex(
            800.0,
            0.0,
            FlexBuendig::Start,
            FlexAusrichtung::Start,
            0.0,
            &k2,
        );
        assert_eq!(s2[0].haupt_mass, 100.0);
    }
    #[test]
    fn container_nan_schrumpft_null() {
        // Container NaN -> 0: alle schrumpfen auf 0 (korrekt, kein Panic).
        let k = [kind(50.0, 0.0)];
        let s = lege_flex(
            f32::NAN,
            0.0,
            FlexBuendig::Start,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_mass, 0.0);
    }
}

#[cfg(test)]
mod tests_extra {
    use super::*;

    fn kind(basis: f32, wachstum: f32) -> FlexKind {
        FlexKind {
            schrumpf: 1.0,
            basis,
            wachstum,
            quer: None,
            ..Default::default()
        }
    }

    #[test]
    fn spalte_als_achsen() {
        // Spalte: Aufrufer mappt Hoehe->haupt, Breite->quer (Dokumentation).
        let k = [kind(100.0, 0.0), kind(100.0, 1.0)];
        let s = lege_flex(
            600.0,
            200.0,
            FlexBuendig::Start,
            FlexAusrichtung::Strecke,
            0.0,
            &k,
        );
        assert_eq!(s[0].haupt_mass, 100.0);
        assert_eq!(s[1].haupt_mass, 500.0);
        assert_eq!(s[1].quer_mass, 200.0);
    }
    #[test]
    fn grow_teilweise() {
        let k = [kind(100.0, 0.0), kind(100.0, 1.0)];
        let s = lege_flex(
            500.0,
            0.0,
            FlexBuendig::Start,
            FlexAusrichtung::Start,
            0.0,
            &k,
        );
        assert_eq!((s[0].haupt_mass, s[1].haupt_mass), (100.0, 400.0));
        assert_eq!(s[1].haupt_pos, 100.0);
    }
    #[test]
    fn luecke_mit_zwischen() {
        let k = [kind(100.0, 0.0), kind(100.0, 0.0)];
        let s = lege_flex(
            500.0,
            0.0,
            FlexBuendig::Zwischen,
            FlexAusrichtung::Start,
            10.0,
            &k,
        );
        // Belegt 210, Rest 290 als Zusatzluecke: Positionen 0 und 400.
        assert_eq!(s[0].haupt_pos, 0.0);
        assert_eq!(s[1].haupt_pos, 400.0);
    }
    #[test]
    fn quer_strecke_mit_mass() {
        // Strecke mit Quermass: Mass bleibt, kein Strecken.
        let k = [FlexKind {
            basis: 50.0,
            schrumpf: 1.0,
            wachstum: 0.0,
            quer: Some(30.0),
            ..Default::default()
        }];
        let s = lege_flex(
            800.0,
            100.0,
            FlexBuendig::Start,
            FlexAusrichtung::Strecke,
            0.0,
            &k,
        );
        assert_eq!((s[0].quer_pos, s[0].quer_mass), (0.0, 30.0));
    }
}
