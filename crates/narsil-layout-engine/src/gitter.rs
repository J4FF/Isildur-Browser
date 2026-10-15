//! Release 11.10.2026: CSS Grid Layout 1/2 als eigener Algorithmus —
//! explizites Raster mit Liniennamen und `auto-fill`/`auto-fit` (§7), die
//! Platzierung mit Namen, Spannen, negativen impliziten Spuren und dem
//! Auto-Platzierungs-Cursor (§8), der Spurgroessen-Algorithmus (§11/§12:
//! intrinsische Groessen mit Spannen, Maximieren, `fr`, `auto`-Dehnen) und
//! die Ausrichtung (§10, CSS Box Alignment 3).
//!
//! GEMESSEN vorher (WPT css-grid, Lauf a00): 564 von 2460 gruen. Die alte
//! Rechnung (`narsil_css::raster`) kannte keine `min-content`/`max-content`/
//! `fit-content()`-Spuren, keine Spannen in der Spurgroesse, keine Namen an
//! den Kindern, nur 32 Spalten und keine `grid-auto-columns` ohne Vorlage
//! (`explicit-grid-size-001`: 784 px statt 10 px).
//!
//! Die alte Rechnung bleibt fuer Masonry (`grid-lanes`) und die alten Tests.

use narsil_layout::gitter::{
    Ausr, AusrLage, GitterLinie, GitterPlatz, GitterStil, LaengeProzent, Spur, SpurGrenze, SpurListe, SpurTeil,
};

/// Hoechste Liniennummer (CSS Grid 1 §8.6 erlaubt eine Grenze; WPT
/// `grid-limits-001` verlangt ±10 000).
pub(crate) const MAX_LINIE: i32 = 10_000;

/// Hoechstens so viele Spuren je Achse insgesamt.
const MAX_SPUREN: usize = 2 * MAX_LINIE as usize + 2;

const EPS: f32 = 0.001;

/// Der Platz einer Achse fuer die Spurgroessen (§12.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Platz {
    /// Definit (Inhaltsbox).
    Definit(f32),
    /// Unter einer min-content-Bedingung.
    MinInhalt,
    /// Unter einer max-content-Bedingung oder unbestimmt (die Hoehe eines
    /// Rasters mit `height: auto`).
    MaxInhalt,
}

impl Platz {
    fn definit(self) -> Option<f32> {
        match self {
            Platz::Definit(v) => Some(v),
            _ => None,
        }
    }
}

/// Die Beitraege eines Elements auf einer Achse, als Randkasten (margin box).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Beitrag {
    /// min-content-Beitrag.
    pub min: f32,
    /// max-content-Beitrag.
    pub max: f32,
    /// Mindestbeitrag (§12.5 „minimum contribution", fuer `auto`-Untergrenzen).
    pub minimum: f32,
    /// Ist der Mindestbeitrag die inhaltsbasierte Mindestgroesse (CSS Grid 1
    /// §6.6)? Dann wird er an festen Obergrenzen der Spuren gekappt.
    pub minimum_inhalt: bool,
}

/// Eine aufgeloeste Lage auf einer Achse (Linien in expliziter Zaehlung,
/// Linie 1 = 0) oder `auto` mit Spannweite.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Lage {
    Fest(i32, i32),
    Auto(i32),
}

/// Das explizite Raster einer Achse: Spuren, Namen der Linien (eine mehr als
/// Spuren) und die `auto-fit`-Marken.
#[derive(Debug, Clone, Default)]
pub(crate) struct Explizit {
    pub spuren: Vec<Spur>,
    pub namen: Vec<Vec<String>>,
    pub fit: Vec<bool>,
}

/// Eine Achse des fertig platzierten Rasters.
#[derive(Debug, Clone, Default)]
pub(crate) struct Achse {
    /// Alle Spuren (implizite davor, explizite, implizite danach).
    pub spuren: Vec<Spur>,
    /// Der Spurindex der expliziten Linie 1.
    pub versatz: i32,
    /// Zahl der expliziten Spuren.
    pub explizit: usize,
    /// Namen der expliziten Linien (fuer abspos-Elemente).
    pub namen: Vec<Vec<String>>,
    /// Kollabierte (leere `auto-fit`-) Spuren.
    pub kollabiert: Vec<bool>,
    /// Die Spurgroessen nach §12.
    pub groesse: Vec<f32>,
    /// Die Luecke (`gap`) in px.
    pub luecke: f32,
}

impl Achse {
    /// Summe der Spuren und Luecken.
    pub(crate) fn gesamt(&self) -> f32 {
        let sichtbar = self.kollabiert.iter().filter(|k| !**k).count();
        self.groesse.iter().sum::<f32>() + self.luecke * sichtbar.saturating_sub(1) as f32
    }

    /// Die Anfangslagen der Spuren (ohne Ausrichtung), dazu das Ende.
    pub(crate) fn lagen(&self, extra_vor: f32, extra_zwischen: f32) -> Vec<f32> {
        let mut raus = Vec::with_capacity(self.groesse.len() + 1);
        let mut x = extra_vor;
        let mut erste = true;
        for (i, g) in self.groesse.iter().enumerate() {
            if !self.kollabiert[i] {
                if !erste {
                    x += self.luecke + extra_zwischen;
                }
                erste = false;
            }
            raus.push(x);
            x += g;
        }
        raus.push(x);
        raus
    }
}

/// Ein platziertes Element: Spurindizes (Ende exklusiv).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Bereich {
    pub z0: usize,
    pub z1: usize,
    pub s0: usize,
    pub s1: usize,
}

/// Das platzierte Raster.
#[derive(Debug, Clone, Default)]
pub(crate) struct Raster {
    pub spalten: Achse,
    pub zeilen: Achse,
    pub bereiche: Vec<Bereich>,
}

// ===========================================================================
// §7: DAS EXPLIZITE RASTER
// ===========================================================================

/// Die Groesse, mit der eine Spur fuer die Zahl der Wiederholungen zaehlt
/// (§7.2.3.2: die Obergrenze, wenn definit, sonst die Untergrenze).
fn wiederhol_mass(s: &Spur, basis: Option<f32>, schaetzung: Option<(f32, f32)>) -> f32 {
    // CSS Grid 3 (vorlaeufig, „intrinsic auto-repeat"): eine intrinsische
    // Spur zaehlt mit dem groessten Beitrag der Elemente (min-content bzw.
    // max-content, `fit-content()` gekappt). GEMESSEN vorher: WPT
    // `grid-lanes/.../intrinsic-auto-repeat/*` (eine Wiederholung statt zehn).
    let kappe = s.passend.and_then(|p| p.aufloesen(basis));
    let fest = |g: &SpurGrenze| match g {
        SpurGrenze::Laenge(l) => l.aufloesen(basis),
        SpurGrenze::MinInhalt => schaetzung.map(|s| s.0),
        SpurGrenze::MaxInhalt | SpurGrenze::Auto => schaetzung.map(|s| match kappe {
            Some(k) => s.1.min(k).max(s.0.min(k)),
            None => s.1,
        }),
        SpurGrenze::Anteil(_) => None,
    };
    let laenge = |g: &SpurGrenze| match g {
        SpurGrenze::Laenge(l) => l.aufloesen(basis),
        _ => None,
    };
    // §7.2.3.2: die Obergrenze, wenn sie definit ist, sonst die Untergrenze
    // — erst ohne beide feste Grenze die Schaetzung. Eine Obergrenze unter
    // der Untergrenze zaehlt als die Untergrenze (`minmax(50px, 25px)` ist
    // 50 px). GEMESSEN vorher: WPT `grid-auto-repeat-minmax` bekam vier
    // statt zwei Wiederholungen; `minmax(auto, 100px)` zaehlte mit der
    // Schaetzung statt mit 100 px (WPT grid-lanes `row-auto-repeat-026`).
    match (laenge(&s.min), laenge(&s.max)) {
        (Some(a), Some(b)) => a.max(b),
        (_, Some(b)) => b,
        (Some(a), None) => a,
        (None, None) => match (fest(&s.min), fest(&s.max)) {
            (Some(a), Some(b)) => a.max(b),
            (a, b) => b.or(a).unwrap_or(0.0),
        },
    }
    .max(0.0)
}

/// Release 11.10.2026: die Spannweite eines Elements (Spalten, Zeilen) fuer
/// die Schaetzung intrinsischer Auto-Wiederholungen — ein Element ueber zwei
/// Spuren traegt je Spur die Haelfte bei. GEMESSEN vorher: WPT grid-lanes
/// `column-auto-repeat-auto-001` (`span 2` mit 200 px: zwei Spalten zu
/// 150 statt drei zu 100).
fn spannen_schaetzung(ctx: &Ctx<'_>, id: NodeId) -> (f32, f32) {
    let Some(k) = kind_stil(ctx, id) else { return (1.0, 1.0) };
    fn spanne(a: &GitterLinie, b: &GitterLinie) -> f32 {
        match (a, b) {
            (GitterLinie::Spanne(k, _), _) | (_, GitterLinie::Spanne(k, _)) => (*k).max(1) as f32,
            (GitterLinie::Zahl(x, None), GitterLinie::Zahl(y, None)) if x != y => (x - y).unsigned_abs().max(1) as f32,
            _ => 1.0,
        }
    }
    (spanne(&k.platz.spalte_start, &k.platz.spalte_ende), spanne(&k.platz.zeile_start, &k.platz.zeile_ende))
}

/// Baut die Spuren und Liniennamen einer Vorlage. `platz` ist die definite
/// Groesse (oder Hoechstgroesse) der Achse, `min_platz` die Mindestgroesse.
pub(crate) fn explizit_bauen(
    liste: &SpurListe,
    platz: Option<f32>,
    min_platz: Option<f32>,
    luecke: f32,
    basis: Option<f32>,
    schaetzung: Option<(f32, f32)>,
) -> Explizit {
    // Die Zahl der Wiederholungen (§7.2.3.2).
    let mut wdh = 1usize;
    if let Some(SpurTeil::Auto { teile, .. }) = liste.teile.iter().find(|t| matches!(t, SpurTeil::Auto { .. })) {
        let gruppe: Vec<&Spur> = teile
            .iter()
            .filter_map(|t| match t {
                SpurTeil::Spur(s) => Some(s),
                _ => None,
            })
            .collect();
        let fest: Vec<&Spur> = liste
            .teile
            .iter()
            .filter_map(|t| match t {
                SpurTeil::Spur(s) => Some(s),
                _ => None,
            })
            .collect();
        let g_summe: f32 = gruppe.iter().map(|s| wiederhol_mass(s, basis, schaetzung)).sum();
        let f_summe: f32 = fest.iter().map(|s| wiederhol_mass(s, basis, schaetzung)).sum();
        let gesamt = |n: usize| -> f32 {
            let spuren = fest.len() + n * gruppe.len();
            f_summe + n as f32 * g_summe + luecke * spuren.saturating_sub(1) as f32
        };
        let je = (g_summe + luecke * gruppe.len() as f32).max(0.0);
        // Intrinsische Spuren in der Wiederholung (CSS Grid 3, vorlaeufig):
        // ohne definite Groesse genau eine Wiederholung.
        let unbestimmt = schaetzung.is_none()
            && gruppe.iter().any(|s| !matches!(s.min, SpurGrenze::Laenge(_)) && !matches!(s.max, SpurGrenze::Laenge(_)));
        let je = if unbestimmt { 0.0 } else { je };
        let grenze_n = (MAX_LINIE as usize / gruppe.len().max(1)).max(1);
        if let Some(p) = platz {
            if je > EPS {
                let mut n = (((p - gesamt(1)) / je).floor() as i64 + 1).max(1) as usize;
                n = n.min(grenze_n);
                // Rundung absichern.
                while n > 1 && gesamt(n) > p + 0.01 {
                    n -= 1;
                }
                while n < grenze_n && gesamt(n + 1) <= p + 0.01 {
                    n += 1;
                }
                wdh = n;
            }
        } else if let Some(m) = min_platz {
            if je > EPS {
                let mut n = 1usize;
                if gesamt(1) < m {
                    n = (((m - gesamt(1)) / je).ceil() as i64 + 1).max(1) as usize;
                }
                n = n.min(grenze_n);
                while n > 1 && gesamt(n - 1) >= m - 0.01 {
                    n -= 1;
                }
                wdh = n;
            }
        }
    }
    let mut raus = Explizit::default();
    let mut namen_jetzt: Vec<String> = Vec::new();
    fn teil(t: &SpurTeil, raus: &mut Explizit, namen_jetzt: &mut Vec<String>, fit: bool) {
        match t {
            SpurTeil::Namen(n) => namen_jetzt.extend(n.iter().cloned()),
            SpurTeil::Spur(s) => {
                if raus.spuren.len() < MAX_LINIE as usize {
                    raus.namen.push(std::mem::take(namen_jetzt));
                    raus.spuren.push(*s);
                    raus.fit.push(fit);
                }
            }
            SpurTeil::Auto { .. } => {}
        }
    }
    for t in &liste.teile {
        match t {
            SpurTeil::Auto { fit, teile } => {
                for _ in 0..wdh {
                    for u in teile {
                        teil(u, &mut raus, &mut namen_jetzt, *fit);
                    }
                }
            }
            t => teil(t, &mut raus, &mut namen_jetzt, false),
        }
    }
    raus.namen.push(namen_jetzt);
    raus
}

/// Die Rechtecke der benannten Gebiete: (Name, Zeile0, Zeile1, Spalte0, Spalte1).
pub(crate) fn gebiets_rechtecke(gebiete: &[Vec<String>]) -> Vec<(String, usize, usize, usize, usize)> {
    let mut raus: Vec<(String, usize, usize, usize, usize)> = Vec::new();
    for (z, zeile) in gebiete.iter().enumerate() {
        for (s, n) in zeile.iter().enumerate() {
            if n.is_empty() {
                continue;
            }
            match raus.iter_mut().find(|r| r.0 == *n) {
                Some(r) => {
                    r.1 = r.1.min(z);
                    r.2 = r.2.max(z + 1);
                    r.3 = r.3.min(s);
                    r.4 = r.4.max(s + 1);
                }
                None => raus.push((n.clone(), z, z + 1, s, s + 1)),
            }
        }
    }
    raus
}

/// Ergaenzt die Spuren/Namen um die von `grid-template-areas` verlangten
/// (§7.3: die zusaetzlichen Spuren sind `grid-auto-*`) und die impliziten
/// Liniennamen `foo-start`/`foo-end`.
fn gebiete_einfuegen(e: &mut Explizit, anzahl: usize, auto: &[Spur], rechtecke: &[(String, usize, usize)]) {
    let alt = e.spuren.len();
    while e.spuren.len() < anzahl {
        let k = e.spuren.len() - alt;
        let s = if auto.is_empty() { Spur::AUTO } else { auto[k % auto.len()] };
        e.spuren.push(s);
        e.fit.push(false);
        e.namen.push(Vec::new());
    }
    for (n, a, b) in rechtecke {
        if let Some(l) = e.namen.get_mut(*a) {
            l.push(format!("{n}-start"));
        }
        if let Some(l) = e.namen.get_mut(*b) {
            l.push(format!("{n}-end"));
        }
    }
}

// ===========================================================================
// §8: PLATZIERUNG
// ===========================================================================

/// Die Linien einer Achse, wie die Platzierung sie braucht.
struct AchsInfo<'a> {
    explizit: i32,
    namen: &'a [Vec<String>],
}

impl AchsInfo<'_> {
    fn hat(&self, l: i32, name: &str) -> bool {
        l >= 0 && (l as usize) < self.namen.len() && self.namen[l as usize].iter().any(|n| n == name)
    }

    /// Die n-te Linie namens `name` (n > 0 vom Anfang, n < 0 vom Ende);
    /// fehlen Linien, tragen alle impliziten den Namen (§8.3).
    fn nte(&self, name: &str, n: i32) -> i32 {
        if n > 0 {
            let mut z = 0;
            for l in 0..=self.explizit {
                if self.hat(l, name) {
                    z += 1;
                    if z == n {
                        return l;
                    }
                }
            }
            self.explizit + (n - z)
        } else {
            let n = -n;
            let mut z = 0;
            for l in (0..=self.explizit).rev() {
                if self.hat(l, name) {
                    z += 1;
                    if z == n {
                        return l;
                    }
                }
            }
            -(n - z)
        }
    }

    /// Eine definite Linie (`None` fuer `auto`/`span`).
    fn definit(&self, l: &GitterLinie, ist_start: bool) -> Option<i32> {
        let v = match l {
            GitterLinie::Zahl(n, None) => {
                if *n > 0 {
                    n - 1
                } else {
                    self.explizit + 1 + n
                }
            }
            GitterLinie::Zahl(n, Some(name)) => self.nte(name, *n),
            GitterLinie::Name(name) => {
                let mit = format!("{name}{}", if ist_start { "-start" } else { "-end" });
                match (0..=self.explizit).find(|&k| self.hat(k, &mit)) {
                    Some(k) => k,
                    None => self.nte(name, 1),
                }
            }
            _ => return None,
        };
        Some(v.clamp(-MAX_LINIE, MAX_LINIE))
    }

    /// `span N name` vorwaerts ab `von` (exklusiv).
    fn spanne_vor(&self, von: i32, name: &str, n: u32) -> i32 {
        let mut z = 0u32;
        let mut l = von + 1;
        while l <= self.explizit {
            if self.hat(l, name) {
                z += 1;
                if z == n {
                    return l;
                }
            }
            l += 1;
        }
        von.max(self.explizit) + (n - z) as i32
    }

    /// `span N name` rueckwaerts ab `von` (exklusiv).
    fn spanne_zurueck(&self, von: i32, name: &str, n: u32) -> i32 {
        let mut z = 0u32;
        let mut l = von - 1;
        while l >= 0 {
            if self.hat(l, name) {
                z += 1;
                if z == n {
                    return l;
                }
            }
            l -= 1;
        }
        von.min(0) - (n - z) as i32
    }

    /// Die Lage eines Elements auf dieser Achse (§8.3, Konflikte nach §8.3.1).
    fn lage(&self, start: &GitterLinie, ende: &GitterLinie) -> Lage {
        let s = self.definit(start, true);
        let e = self.definit(ende, false);
        let span_von = |l: &GitterLinie| match l {
            GitterLinie::Spanne(n, None) => Some(*n as i32),
            GitterLinie::Spanne(_, Some(_)) => Some(1),
            _ => None,
        };
        match (s, e) {
            (Some(a), Some(b)) => {
                if a == b {
                    Lage::Fest(a, (a + 1).min(MAX_LINIE + 1))
                } else {
                    Lage::Fest(a.min(b), a.max(b))
                }
            }
            (Some(a), None) => {
                let b = match ende {
                    GitterLinie::Spanne(n, None) => a + *n as i32,
                    GitterLinie::Spanne(n, Some(name)) => self.spanne_vor(a, name, *n),
                    _ => a + 1,
                };
                Lage::Fest(a, b.min(MAX_LINIE + 1))
            }
            (None, Some(b)) => {
                let a = match start {
                    GitterLinie::Spanne(n, None) => b - *n as i32,
                    GitterLinie::Spanne(n, Some(name)) => self.spanne_zurueck(b, name, *n),
                    _ => b - 1,
                };
                Lage::Fest(a.max(-MAX_LINIE - 1), b)
            }
            (None, None) => Lage::Auto(span_von(start).or_else(|| span_von(ende)).unwrap_or(1).clamp(1, MAX_LINIE)),
        }
    }
}

/// Belegte Zellen: je Hauptlinie die belegten Intervalle der Nebenachse.
struct Belegung {
    haupt_min: i32,
    zeilen: Vec<Vec<(i32, i32)>>,
}

impl Belegung {
    fn frei(&self, h0: i32, h1: i32, n0: i32, n1: i32) -> bool {
        for h in h0..h1 {
            let k = h - self.haupt_min;
            if k < 0 {
                continue;
            }
            if let Some(iv) = self.zeilen.get(k as usize)
                && iv.iter().any(|&(a, b)| a < n1 && n0 < b)
            {
                return false;
            }
        }
        true
    }

    fn belegen(&mut self, h0: i32, h1: i32, n0: i32, n1: i32) {
        if h0 < self.haupt_min {
            let mehr = (self.haupt_min - h0) as usize;
            let mut neu = vec![Vec::new(); mehr];
            neu.append(&mut self.zeilen);
            self.zeilen = neu;
            self.haupt_min = h0;
        }
        for h in h0..h1 {
            let k = (h - self.haupt_min) as usize;
            if self.zeilen.len() <= k {
                self.zeilen.resize(k + 1, Vec::new());
            }
            self.zeilen[k].push((n0, n1));
        }
    }
}

/// Die Spuren einer Subgrid-Achse, vom Eltern uebernommen (CSS Grid 2 §9).
#[derive(Debug, Clone, Default)]
pub(crate) struct Vorgabe {
    /// Die Spurgroessen (schon um Rand/Rahmen/Innenabstand des Subgrids an
    /// den Kanten und um den Luecken-Unterschied bereinigt).
    pub groessen: Vec<f32>,
    /// Die Liniennamen (eine mehr als Spuren).
    pub namen: Vec<Vec<String>>,
    /// Die Luecke des Subgrids.
    pub luecke: f32,
}

/// Vorgaben fuer beide Achsen (`None` = eigene Spuren).
#[derive(Debug, Clone, Default)]
pub(crate) struct Vorgaben {
    pub spalten: Option<Vorgabe>,
    pub zeilen: Option<Vorgabe>,
    /// Groesster (min-content, max-content)-Beitrag der Elemente je Achse,
    /// fuer intrinsische `auto-fill`/`auto-fit`-Spuren (CSS Grid 3).
    pub schaetzung_spalten: Option<(f32, f32)>,
    pub schaetzung_zeilen: Option<(f32, f32)>,
}

/// Die Eingabe der Platzierung je Element.
pub(crate) struct PlatzEingabe<'a> {
    pub platz: &'a GitterPlatz,
}

/// Die Platzierung (§8.5) und der Aufbau der Achsen. `spalten_platz`/
/// `zeilen_platz` sind die definiten Groessen (oder Hoechstgroessen) fuer
/// `auto-fill`, `*_min` die Mindestgroessen; die Luecken sind schon aufgeloest.
#[allow(clippy::too_many_arguments)]
pub(crate) fn platzieren(
    stil: &GitterStil,
    elemente: &[PlatzEingabe<'_>],
    spalten_platz: Option<f32>,
    spalten_min: Option<f32>,
    zeilen_platz: Option<f32>,
    zeilen_min: Option<f32>,
    luecke_spalte: f32,
    luecke_zeile: f32,
    prozent_breite: Option<f32>,
    prozent_hoehe: Option<f32>,
    vorgaben: &Vorgaben,
) -> Raster {
    // Ein Subgrid (CSS Grid 2 §9) hat auf seiner Subgrid-Achse genau die
    // Spuren seines Bereichs im Eltern, mit dessen Liniennamen.
    let von_vorgabe = |v: &Vorgabe| Explizit {
        spuren: vec![Spur::AUTO; v.groessen.len()],
        namen: v.namen.clone(),
        fit: vec![false; v.groessen.len()],
    };
    let mut sp = match &vorgaben.spalten {
        Some(v) => von_vorgabe(v),
        None => explizit_bauen(&stil.spalten, spalten_platz, spalten_min, luecke_spalte, prozent_breite, vorgaben.schaetzung_spalten),
    };
    let mut ze = match &vorgaben.zeilen {
        Some(v) => von_vorgabe(v),
        None => explizit_bauen(&stil.zeilen, zeilen_platz, zeilen_min, luecke_zeile, prozent_hoehe, vorgaben.schaetzung_zeilen),
    };
    let rechtecke = gebiets_rechtecke(&stil.gebiete);
    let g_zeilen = stil.gebiete.len();
    let g_spalten = stil.gebiete.first().map_or(0, Vec::len);
    let sp_rechtecke: Vec<(String, usize, usize)> = rechtecke.iter().map(|r| (r.0.clone(), r.3, r.4)).collect();
    let ze_rechtecke: Vec<(String, usize, usize)> = rechtecke.iter().map(|r| (r.0.clone(), r.1, r.2)).collect();
    let sp_n = if vorgaben.spalten.is_some() { sp.spuren.len() } else { sp.spuren.len().max(g_spalten) };
    let ze_n = if vorgaben.zeilen.is_some() { ze.spuren.len() } else { ze.spuren.len().max(g_zeilen) };
    gebiete_einfuegen(&mut sp, sp_n, &stil.auto_spalten, &sp_rechtecke);
    gebiete_einfuegen(&mut ze, ze_n, &stil.auto_zeilen, &ze_rechtecke);
    let si = AchsInfo { explizit: sp.spuren.len() as i32, namen: &sp.namen };
    let zi = AchsInfo { explizit: ze.spuren.len() as i32, namen: &ze.namen };
    // Die Lagen auf beiden Achsen (Haupt = Zeilen im Zeilenfluss).
    let spalten_fluss = stil.fluss_spalte;
    let mut lagen: Vec<(Lage, Lage)> = elemente
        .iter()
        .map(|e| {
            let z = zi.lage(&e.platz.zeile_start, &e.platz.zeile_ende);
            let s = si.lage(&e.platz.spalte_start, &e.platz.spalte_ende);
            if spalten_fluss { (s, z) } else { (z, s) }
        })
        .collect();
    // Auf einer Subgrid-Achse gibt es keine impliziten Spuren: die Linien
    // werden in das Subgrid geklemmt (CSS Grid 2 §9).
    let klemm = |l: Lage, n: i32| -> Lage {
        match l {
            Lage::Fest(a, b) => {
                let a = a.clamp(0, (n - 1).max(0));
                Lage::Fest(a, b.clamp(a + 1, n.max(a + 1)))
            }
            Lage::Auto(s) => Lage::Auto(s.min(n.max(1))),
        }
    };
    if vorgaben.spalten.is_some() || vorgaben.zeilen.is_some() {
        for l in &mut lagen {
            let (z, s) = if spalten_fluss { (&mut l.1, &mut l.0) } else { (&mut l.0, &mut l.1) };
            if vorgaben.spalten.is_some() {
                *s = klemm(*s, si.explizit);
            }
            if vorgaben.zeilen.is_some() {
                *z = klemm(*z, zi.explizit);
            }
        }
    }
    let (haupt_explizit, neben_explizit) =
        if spalten_fluss { (si.explizit, zi.explizit) } else { (zi.explizit, si.explizit) };
    let mut bel = Belegung { haupt_min: 0, zeilen: Vec::new() };
    // Schritt 1: beide Achsen fest.
    for (h, n) in &lagen {
        if let (Lage::Fest(h0, h1), Lage::Fest(n0, n1)) = (h, n) {
            bel.belegen(*h0, *h1, *n0, *n1);
        }
    }
    // Schritt 2: fest auf der Hauptachse, `auto` auf der Nebenachse.
    let mut neben_min = 0i32;
    for (h, n) in &lagen {
        if let Lage::Fest(n0, _) = n {
            neben_min = neben_min.min(*n0);
        }
        let _ = h;
    }
    let mut zeilen_cursor: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    for i in 0..lagen.len() {
        if let (Lage::Fest(h0, h1), Lage::Auto(span)) = lagen[i] {
            let mut n = if stil.dicht { neben_min } else { *zeilen_cursor.get(&h0).unwrap_or(&neben_min) };
            let mut schritte = 0;
            while !bel.frei(h0, h1, n, n + span) && schritte < 4 * MAX_LINIE {
                n += 1;
                schritte += 1;
            }
            bel.belegen(h0, h1, n, n + span);
            zeilen_cursor.insert(h0, n + span);
            lagen[i].1 = Lage::Fest(n, n + span);
        }
    }
    // Schritt 3: die Breite der Nebenachse.
    let mut neben_max = neben_explizit;
    let mut groesste_spanne = 1;
    let mut haupt_start = 0i32;
    for (h, n) in &lagen {
        match n {
            Lage::Fest(n0, n1) => {
                neben_min = neben_min.min(*n0);
                neben_max = neben_max.max(*n1);
            }
            Lage::Auto(s) => groesste_spanne = groesste_spanne.max(*s),
        }
        if let Lage::Fest(h0, _) = h {
            haupt_start = haupt_start.min(*h0);
        }
    }
    if neben_max - neben_min < groesste_spanne {
        neben_max = neben_min + groesste_spanne;
    }
    // Schritt 4: der Rest mit dem Cursor.
    let mut cur_h = haupt_start;
    let mut cur_n = neben_min;
    let grenze_h = haupt_start + 4 * MAX_LINIE;
    for i in 0..lagen.len() {
        let (h, n) = lagen[i];
        let Lage::Auto(h_span) = h else { continue };
        match n {
            Lage::Fest(n0, n1) => {
                if stil.dicht {
                    cur_h = haupt_start;
                } else if n0 < cur_n {
                    cur_h += 1;
                }
                cur_n = n0;
                while !bel.frei(cur_h, cur_h + h_span, n0, n1) && cur_h < grenze_h {
                    cur_h += 1;
                }
                bel.belegen(cur_h, cur_h + h_span, n0, n1);
                lagen[i].0 = Lage::Fest(cur_h, cur_h + h_span);
            }
            Lage::Auto(n_span) => {
                if stil.dicht {
                    cur_h = haupt_start;
                    cur_n = neben_min;
                }
                loop {
                    let mut gefunden = false;
                    while cur_n + n_span <= neben_max {
                        if bel.frei(cur_h, cur_h + h_span, cur_n, cur_n + n_span) {
                            gefunden = true;
                            break;
                        }
                        cur_n += 1;
                    }
                    if gefunden || cur_h >= grenze_h {
                        break;
                    }
                    cur_h += 1;
                    cur_n = neben_min;
                }
                bel.belegen(cur_h, cur_h + h_span, cur_n, cur_n + n_span);
                lagen[i] = (Lage::Fest(cur_h, cur_h + h_span), Lage::Fest(cur_n, cur_n + n_span));
                if !stil.dicht {
                    cur_n += n_span;
                }
            }
        }
    }
    if vorgaben.spalten.is_some() || vorgaben.zeilen.is_some() {
        for l in &mut lagen {
            let (z, s) = if spalten_fluss { (&mut l.1, &mut l.0) } else { (&mut l.0, &mut l.1) };
            if vorgaben.spalten.is_some() {
                *s = klemm(*s, si.explizit);
            }
            if vorgaben.zeilen.is_some() {
                *z = klemm(*z, zi.explizit);
            }
        }
    }
    // Die Ausdehnung beider Achsen.
    let (mut h_lo, mut h_hi, mut n_lo, mut n_hi) = (0i32, haupt_explizit, 0i32, neben_explizit);
    for (h, n) in &lagen {
        if let (Lage::Fest(h0, h1), Lage::Fest(n0, n1)) = (h, n) {
            h_lo = h_lo.min(*h0);
            h_hi = h_hi.max(*h1);
            n_lo = n_lo.min(*n0);
            n_hi = n_hi.max(*n1);
        }
    }
    let (z_lo, z_hi, s_lo, s_hi) = if spalten_fluss { (n_lo, n_hi, h_lo, h_hi) } else { (h_lo, h_hi, n_lo, n_hi) };
    let mut bereiche = Vec::with_capacity(lagen.len());
    for (h, n) in &lagen {
        let (Lage::Fest(h0, h1), Lage::Fest(n0, n1)) = (*h, *n) else {
            bereiche.push(Bereich::default());
            continue;
        };
        let ((z0, z1), (s0, s1)) = if spalten_fluss { ((n0, n1), (h0, h1)) } else { ((h0, h1), (n0, n1)) };
        bereiche.push(Bereich {
            z0: (z0 - z_lo) as usize,
            z1: (z1 - z_lo) as usize,
            s0: (s0 - s_lo) as usize,
            s1: (s1 - s_lo) as usize,
        });
    }
    let spalten = achse_bauen(&sp, &stil.auto_spalten, s_lo, s_hi, luecke_spalte, &bereiche, true);
    let zeilen = achse_bauen(&ze, &stil.auto_zeilen, z_lo, z_hi, luecke_zeile, &bereiche, false);
    Raster { spalten, zeilen, bereiche }
}

/// Baut eine Achse aus dem expliziten Raster und den impliziten Spuren
/// (`grid-auto-*` zyklisch; vor dem Raster rueckwaerts, §7.6).
fn achse_bauen(e: &Explizit, auto: &[Spur], lo: i32, hi: i32, luecke: f32, bereiche: &[Bereich], spalten: bool) -> Achse {
    let n = ((hi - lo).max(0) as usize).min(MAX_SPUREN);
    let ex = e.spuren.len() as i32;
    let mut spuren = Vec::with_capacity(n);
    let mut fit = Vec::with_capacity(n);
    for k in 0..n as i32 {
        let l = lo + k;
        let auto_spur = |i: i32| -> Spur {
            if auto.is_empty() {
                Spur::AUTO
            } else {
                let m = auto.len() as i32;
                auto[i.rem_euclid(m) as usize]
            }
        };
        if l >= 0 && l < ex {
            spuren.push(e.spuren[l as usize]);
            fit.push(e.fit[l as usize]);
        } else if l >= ex {
            spuren.push(auto_spur(l - ex));
            fit.push(false);
        } else {
            spuren.push(auto_spur(l));
            fit.push(false);
        }
    }
    // Leere `auto-fit`-Spuren kollabieren (§7.2.3.2).
    let mut belegt = vec![false; n];
    for b in bereiche {
        let (a, z) = if spalten { (b.s0, b.s1) } else { (b.z0, b.z1) };
        for x in belegt.iter_mut().take(z.min(n)).skip(a) {
            *x = true;
        }
    }
    let kollabiert: Vec<bool> = (0..n).map(|i| fit[i] && !belegt[i]).collect();
    Achse {
        spuren,
        versatz: -lo,
        explizit: e.spuren.len(),
        namen: e.namen.clone(),
        kollabiert,
        groesse: vec![0.0; n],
        luecke,
    }
}

// ===========================================================================
// §11/§12: DIE SPURGROESSEN
// ===========================================================================

/// Eine aufgeloeste Spurgrenze.
#[derive(Debug, Clone, Copy, PartialEq)]
enum G {
    Fest(f32),
    MinI,
    MaxI,
    Auto,
    Fr(f32),
}

impl G {
    fn intrinsisch(self) -> bool {
        matches!(self, G::MinI | G::MaxI | G::Auto)
    }
}

#[derive(Debug, Clone, Copy)]
struct Def {
    min: G,
    max: G,
    /// `fit-content()`-Argument (aufgeloest).
    passend: Option<f32>,
}

impl Def {
    fn flexibel(&self) -> bool {
        matches!(self.max, G::Fr(_))
    }
    fn fr(&self) -> f32 {
        match self.max {
            G::Fr(f) => f,
            _ => 0.0,
        }
    }
    /// Intrinsische Obergrenze (auch `fit-content`).
    fn max_intrinsisch(&self) -> bool {
        self.max.intrinsisch() || self.passend.is_some()
    }
    /// max-content-artige Obergrenze (`max-content`, `auto`, `fit-content`).
    fn max_maxinhalt(&self) -> bool {
        matches!(self.max, G::MaxI | G::Auto) || self.passend.is_some()
    }
    /// Die feste Obergrenze (fuer die „limited contribution").
    fn feste_grenze(&self) -> Option<f32> {
        match self.max {
            G::Fest(v) => Some(v),
            _ => self.passend,
        }
    }
}

fn aufloesen(g: SpurGrenze, basis: Option<f32>) -> G {
    match g {
        SpurGrenze::Laenge(l) => match l.aufloesen(basis) {
            Some(v) => G::Fest(v.max(0.0)),
            None => G::Auto,
        },
        SpurGrenze::Anteil(f) => G::Fr(f.max(0.0)),
        SpurGrenze::MinInhalt => G::MinI,
        SpurGrenze::MaxInhalt => G::MaxI,
        SpurGrenze::Auto => G::Auto,
    }
}

#[derive(Debug, Clone, Copy)]
struct Z {
    basis: f32,
    grenze: f32,
    unbegrenzt: bool,
}

/// Ein Element in der Spurrechnung dieser Achse.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AchsElement {
    pub a: usize,
    pub b: usize,
    pub beitrag: Beitrag,
}

/// Was ein Unterschritt verteilt.
#[derive(Clone, Copy, PartialEq)]
enum Ziel {
    Basis,
    Grenze,
}

/// Verteilt `platz` auf `ids` mit Obergrenzen `raum` (gleichmaessig oder
/// nach `gewicht`), friert erreichte Grenzen ein. Gibt den Rest zurueck.
fn gleich_verteilen(mut platz: f32, ids: &[usize], raum: &mut [f32], gewicht: &[f32], zuwachs: &mut [f32]) -> f32 {
    let mut aktiv: Vec<usize> = ids.iter().copied().filter(|&i| raum[i] > EPS).collect();
    let mut runden = 0;
    while platz > EPS && !aktiv.is_empty() && runden < 4 * ids.len() + 4 {
        runden += 1;
        let wsum: f32 = aktiv.iter().map(|&i| gewicht[i]).sum();
        if wsum <= 0.0 {
            break;
        }
        let je = platz / wsum;
        let mut gefroren = false;
        for &i in &aktiv {
            if raum[i] <= je * gewicht[i] + EPS {
                zuwachs[i] += raum[i];
                platz -= raum[i];
                raum[i] = 0.0;
                gefroren = true;
            }
        }
        if !gefroren {
            for &i in &aktiv {
                let d = je * gewicht[i];
                zuwachs[i] += d;
                raum[i] -= d;
            }
            platz = 0.0;
        }
        aktiv.retain(|&i| raum[i] > EPS);
    }
    platz.max(0.0)
}

struct Rechnung<'a> {
    defs: Vec<Def>,
    z: Vec<Z>,
    luecke: f32,
    elemente: &'a [AchsElement],
    unter_bedingung: bool,
    max_bedingung: bool,
}

impl Rechnung<'_> {
    /// Die Luecken innerhalb einer Spanne.
    fn luecken(&self, a: usize, b: usize) -> f32 {
        self.luecke * (b - a).saturating_sub(1) as f32
    }

    /// Der begrenzte min-/max-content-Beitrag (§12.5: an festen Obergrenzen
    /// gekappt, wenn alle Spuren eine haben; nie unter dem Mindestbeitrag).
    fn begrenzt(&self, e: &AchsElement, wert: f32) -> f32 {
        let mut summe = 0.0;
        for t in e.a..e.b {
            match self.defs[t].feste_grenze() {
                Some(v) => summe += v,
                None => return wert.max(self.minimum(e)),
            }
        }
        wert.min(summe + self.luecken(e.a, e.b)).max(self.minimum(e))
    }

    /// Der Mindestbeitrag (die inhaltsbasierte Mindestgroesse an festen
    /// Obergrenzen gekappt, CSS Grid 1 §6.6).
    fn minimum(&self, e: &AchsElement) -> f32 {
        let m = e.beitrag.minimum;
        if !e.beitrag.minimum_inhalt {
            return m;
        }
        let mut summe = 0.0;
        for t in e.a..e.b {
            match self.defs[t].feste_grenze() {
                Some(v) => summe += v,
                None => return m,
            }
        }
        m.min(summe + self.luecken(e.a, e.b))
    }

    /// Ein Unterschritt von §12.5 Schritt 3/4 fuer eine Gruppe.
    #[allow(clippy::too_many_arguments)]
    fn schritt(
        &mut self,
        gruppe: &[usize],
        betroffen: &dyn Fn(&Def) -> bool,
        beitrag: &dyn Fn(&Self, &AchsElement) -> f32,
        ziel: Ziel,
        jenseits: &dyn Fn(&Def) -> bool,
        nur_flex: bool,
        markieren: bool,
    ) {
        let n = self.z.len();
        let mut geplant = vec![0.0f32; n];
        let mut beruehrt = vec![false; n];
        for &ei in gruppe {
            let e = self.elemente[ei];
            let ids: Vec<usize> = (e.a..e.b)
                .filter(|&t| betroffen(&self.defs[t]) && (!nur_flex || self.defs[t].flexibel()))
                .collect();
            if ids.is_empty() {
                continue;
            }
            let c = beitrag(self, &e);
            let belegt: f32 = (e.a..e.b)
                .map(|t| match ziel {
                    Ziel::Basis => self.z[t].basis,
                    Ziel::Grenze => {
                        if self.z[t].grenze.is_finite() {
                            self.z[t].grenze
                        } else {
                            self.z[t].basis
                        }
                    }
                })
                .sum::<f32>()
                + self.luecken(e.a, e.b);
            let platz = c - belegt;
            if platz <= EPS {
                continue;
            }
            for &t in &ids {
                beruehrt[t] = true;
            }
            let mut zuwachs = vec![0.0f32; n];
            let mut raum = vec![0.0f32; n];
            let mut gewicht = vec![1.0f32; n];
            if nur_flex {
                let summe: f32 = ids.iter().map(|&t| self.defs[t].fr()).sum();
                if summe > EPS {
                    for &t in &ids {
                        gewicht[t] = self.defs[t].fr();
                    }
                }
            }
            // Schritt 2: bis zu den Grenzen.
            for &t in &ids {
                let groesse = match ziel {
                    Ziel::Basis => self.z[t].basis,
                    Ziel::Grenze => {
                        if self.z[t].grenze.is_finite() {
                            self.z[t].grenze
                        } else {
                            self.z[t].basis
                        }
                    }
                };
                let grenze = match ziel {
                    Ziel::Basis => {
                        let g = self.z[t].grenze;
                        match self.defs[t].passend {
                            Some(p) => g.min(p.max(self.z[t].basis)),
                            None => g,
                        }
                    }
                    Ziel::Grenze => {
                        if self.z[t].unbegrenzt || !self.z[t].grenze.is_finite() {
                            f32::INFINITY
                        } else {
                            self.z[t].grenze
                        }
                    }
                };
                raum[t] = (grenze - groesse).max(0.0);
            }
            let mut rest = gleich_verteilen(platz, &ids, &mut raum, &gewicht, &mut zuwachs);
            // Schritt 3: ueber die Grenzen hinaus.
            if rest > EPS {
                let mut ziele: Vec<usize> = ids.iter().copied().filter(|&t| jenseits(&self.defs[t])).collect();
                if ziele.is_empty() {
                    ziele = ids.clone();
                }
                for &t in &ziele {
                    // `fit-content()` waechst nur bis zu seinem Argument.
                    raum[t] = match (self.defs[t].passend, ziel) {
                        (Some(p), _) if matches!(ziel, Ziel::Grenze) || jenseits(&self.defs[t]) => {
                            let groesse = match ziel {
                                Ziel::Basis => self.z[t].basis,
                                Ziel::Grenze => {
                                    if self.z[t].grenze.is_finite() {
                                        self.z[t].grenze
                                    } else {
                                        self.z[t].basis
                                    }
                                }
                            };
                            (p - groesse - zuwachs[t]).max(0.0)
                        }
                        _ => f32::INFINITY,
                    };
                }
                rest = gleich_verteilen(rest, &ziele, &mut raum, &gewicht, &mut zuwachs);
                if rest > EPS && ziel == Ziel::Basis {
                    for &t in &ziele {
                        raum[t] = f32::INFINITY;
                    }
                    gleich_verteilen(rest, &ziele, &mut raum, &gewicht, &mut zuwachs);
                }
            }
            for &t in &ids {
                geplant[t] = geplant[t].max(zuwachs[t]);
            }
        }
        for t in 0..n {
            if !beruehrt[t] {
                continue;
            }
            match ziel {
                Ziel::Basis => self.z[t].basis += geplant[t],
                Ziel::Grenze => {
                    if self.z[t].grenze.is_finite() {
                        self.z[t].grenze += geplant[t];
                    } else {
                        self.z[t].grenze = self.z[t].basis + geplant[t];
                        if markieren {
                            self.z[t].unbegrenzt = true;
                        }
                    }
                }
            }
        }
    }

    /// §12.5 Schritt 3 (bzw. 4 mit `nur_flex`) fuer eine Gruppe.
    fn spannen_schritte(&mut self, gruppe: &[usize], nur_flex: bool) {
        let unter = self.unter_bedingung;
        let maxb = self.max_bedingung;
        // 1. intrinsische Untergrenzen.
        self.schritt(
            gruppe,
            &|d| d.min.intrinsisch(),
            &|r, e| if unter { r.begrenzt(e, e.beitrag.min) } else { r.minimum(e) },
            Ziel::Basis,
            &|d| d.max_intrinsisch(),
            nur_flex,
            false,
        );
        // 2. inhaltsbasierte Untergrenzen.
        self.schritt(
            gruppe,
            &|d| matches!(d.min, G::MinI | G::MaxI),
            &|_, e| e.beitrag.min,
            Ziel::Basis,
            &|d| d.max_intrinsisch(),
            nur_flex,
            false,
        );
        // 3. max-content-Untergrenzen.
        if maxb {
            self.schritt(
                gruppe,
                &|d| matches!(d.min, G::Auto | G::MaxI),
                &|r, e| r.begrenzt(e, e.beitrag.max),
                Ziel::Basis,
                &|d| d.max_maxinhalt(),
                nur_flex,
                false,
            );
        } else {
            self.schritt(
                gruppe,
                &|d| matches!(d.min, G::MaxI),
                &|_, e| e.beitrag.max,
                Ziel::Basis,
                &|d| d.max_maxinhalt(),
                nur_flex,
                false,
            );
        }
        // 4. Grenze nie unter der Basis.
        for z in &mut self.z {
            if z.grenze < z.basis {
                z.grenze = z.basis;
            }
        }
        if nur_flex {
            return;
        }
        // 5. intrinsische Obergrenzen.
        for z in &mut self.z {
            z.unbegrenzt = false;
        }
        self.schritt(gruppe, &|d| d.max_intrinsisch(), &|_, e| e.beitrag.min, Ziel::Grenze, &|_| true, false, true);
        // 6. max-content-Obergrenzen.
        self.schritt(gruppe, &|d| d.max_maxinhalt(), &|_, e| e.beitrag.max, Ziel::Grenze, &|_| true, false, false);
        for z in &mut self.z {
            z.unbegrenzt = false;
        }
    }
}

/// Die Eingabe der Spurrechnung einer Achse.
pub(crate) struct AchsRechnung<'a> {
    pub platz: Platz,
    /// Die Basis fuer Prozentspuren (definite Groesse der Achse).
    pub prozent_basis: Option<f32>,
    /// Mindest-/Hoechstgroesse der Inhaltsbox (fuer §12.6/§12.7/§12.8).
    pub min_platz: Option<f32>,
    pub max_platz: Option<f32>,
    /// `*-content: normal | stretch` (§12.8 dehnt dann die `auto`-Spuren).
    pub strecken: bool,
    pub elemente: &'a [AchsElement],
}

/// §12.3–§12.8 fuer eine Achse; schreibt `achse.groesse`.
pub(crate) fn spuren_groessen(achse: &mut Achse, r: &AchsRechnung<'_>) {
    let n = achse.spuren.len();
    let mut defs: Vec<Def> = Vec::with_capacity(n);
    for (i, s) in achse.spuren.iter().enumerate() {
        if achse.kollabiert[i] {
            defs.push(Def { min: G::Fest(0.0), max: G::Fest(0.0), passend: None });
            continue;
        }
        let mut min = aufloesen(s.min, r.prozent_basis);
        let max = aufloesen(s.max, r.prozent_basis);
        // `fr` als Untergrenze gibt es nicht (der Leser verwirft es); sicher ist sicher.
        if matches!(min, G::Fr(_)) {
            min = G::Auto;
        }
        let passend = s.passend.and_then(|p| p.aufloesen(r.prozent_basis)).map(|v| v.max(0.0));
        defs.push(Def { min, max, passend });
    }
    let mut z: Vec<Z> = defs
        .iter()
        .map(|d| {
            let basis = match d.min {
                G::Fest(v) => v,
                _ => 0.0,
            };
            let grenze = match d.max {
                G::Fest(v) => v.max(basis),
                _ => f32::INFINITY,
            };
            Z { basis, grenze, unbegrenzt: false }
        })
        .collect();
    let luecke = achse.luecke;
    let unter = !matches!(r.platz, Platz::Definit(_));
    let maxb = matches!(r.platz, Platz::MaxInhalt);
    // §12.5 Schritt 2: Elemente mit Spanne 1 ausserhalb flexibler Spuren.
    let mut spannen: Vec<(usize, usize)> = Vec::new();
    let mut flex: Vec<usize> = Vec::new();
    for (i, e) in r.elemente.iter().enumerate() {
        if e.b <= e.a || e.b > n {
            continue;
        }
        let kreuzt_flex = (e.a..e.b).any(|t| defs[t].flexibel());
        if kreuzt_flex {
            flex.push(i);
        } else if e.b - e.a == 1 {
            let t = e.a;
            let d = defs[t];
            if !(d.min.intrinsisch() || d.max_intrinsisch()) {
                continue;
            }
            spannen.push((1, i));
        } else {
            spannen.push((e.b - e.a, i));
        }
    }
    let mut rech = Rechnung { defs, z: std::mem::take(&mut z), luecke, elemente: r.elemente, unter_bedingung: unter, max_bedingung: maxb };
    // Spanne 1 (Schritt 2).
    {
        let mut basis_neu: Vec<Option<f32>> = vec![None; n];
        let mut grenze_neu: Vec<Option<f32>> = vec![None; n];
        for &(s, i) in &spannen {
            if s != 1 {
                continue;
            }
            let e = r.elemente[i];
            let t = e.a;
            let d = rech.defs[t];
            let b = match d.min {
                G::MinI => Some(e.beitrag.min),
                G::MaxI => Some(e.beitrag.max),
                G::Auto => Some(if unter { rech.begrenzt(&e, e.beitrag.min) } else { rech.minimum(&e) }),
                _ => None,
            };
            if let Some(b) = b {
                basis_neu[t] = Some(basis_neu[t].unwrap_or(0.0).max(b.max(0.0)));
            }
            let g = match d.max {
                G::MinI => Some(e.beitrag.min),
                G::MaxI | G::Auto => Some(e.beitrag.max),
                _ => None,
            };
            let g = match (g, d.passend) {
                (Some(g), Some(p)) => Some(g.min(p)),
                (None, Some(p)) => Some(e.beitrag.max.min(p)),
                (g, None) => g,
            };
            if let Some(g) = g {
                grenze_neu[t] = Some(grenze_neu[t].unwrap_or(0.0).max(g));
            }
        }
        for t in 0..n {
            if let Some(b) = basis_neu[t] {
                rech.z[t].basis = rech.z[t].basis.max(b);
            }
            if let Some(g) = grenze_neu[t] {
                rech.z[t].grenze = g;
            }
            if rech.z[t].grenze < rech.z[t].basis {
                rech.z[t].grenze = rech.z[t].basis;
            }
        }
    }
    // Schritt 3: Spannen > 1, nach Spannweite.
    let mut weiten: Vec<usize> = spannen.iter().map(|(s, _)| *s).filter(|s| *s > 1).collect();
    weiten.sort_unstable();
    weiten.dedup();
    for w in weiten {
        let gruppe: Vec<usize> = spannen.iter().filter(|(s, _)| *s == w).map(|(_, i)| *i).collect();
        rech.spannen_schritte(&gruppe, false);
    }
    // Schritt 4: Elemente ueber flexible Spuren.
    if !flex.is_empty() {
        rech.spannen_schritte(&flex, true);
    }
    // Schritt 5: unendliche Grenzen = Basis.
    for zz in &mut rech.z {
        if !zz.grenze.is_finite() {
            zz.grenze = zz.basis;
        }
    }
    let defs = rech.defs;
    let mut z = rech.z;
    let sichtbar = achse.kollabiert.iter().filter(|k| !**k).count();
    let luecken_gesamt = luecke * sichtbar.saturating_sub(1) as f32;
    let summe = |z: &[Z]| z.iter().map(|x| x.basis).sum::<f32>() + luecken_gesamt;
    // §12.6 Maximieren.
    let vorher: Vec<Z> = z.clone();
    let maximieren = |z: &mut Vec<Z>, frei: f32| {
        let ids: Vec<usize> = (0..n).collect();
        let mut raum: Vec<f32> = z.iter().map(|x| (x.grenze - x.basis).max(0.0)).collect();
        let gewicht = vec![1.0f32; n];
        let mut zuwachs = vec![0.0f32; n];
        if frei.is_infinite() {
            for x in z.iter_mut() {
                x.basis = x.grenze;
            }
            return;
        }
        gleich_verteilen(frei, &ids, &mut raum, &gewicht, &mut zuwachs);
        for t in 0..n {
            z[t].basis += zuwachs[t];
        }
    };
    let frei = match r.platz {
        Platz::Definit(v) => v - summe(&z),
        Platz::MaxInhalt => f32::INFINITY,
        Platz::MinInhalt => 0.0,
    };
    if frei > EPS {
        maximieren(&mut z, frei);
        if let (Platz::MaxInhalt, Some(m)) = (r.platz, r.max_platz)
            && summe(&z) > m + EPS
        {
            z = vorher.clone();
            let f = m - summe(&z);
            if f > EPS {
                maximieren(&mut z, f);
            }
        }
    }
    // §12.7 `fr`.
    if defs.iter().any(Def::flexibel) {
        let fr_groesse = |z: &[Z], ids: &[usize], fuellen: f32| -> f32 {
            let mut unflex = vec![false; n];
            for _ in 0..ids.len() + 1 {
                let mut rest = fuellen - luecke * ids.iter().filter(|&&t| !achse.kollabiert[t]).count().saturating_sub(1) as f32;
                let mut fsum = 0.0f32;
                for &t in ids {
                    if defs[t].flexibel() && !unflex[t] {
                        fsum += defs[t].fr();
                    } else {
                        rest -= z[t].basis;
                    }
                }
                let hyp = rest.max(0.0) / fsum.max(1.0);
                let mut neu = false;
                for &t in ids {
                    if defs[t].flexibel() && !unflex[t] && hyp * defs[t].fr() < z[t].basis - EPS {
                        unflex[t] = true;
                        neu = true;
                    }
                }
                if !neu {
                    return hyp;
                }
            }
            0.0
        };
        let alle: Vec<usize> = (0..n).collect();
        let mut anteil = match r.platz {
            Platz::MinInhalt => 0.0,
            Platz::Definit(v) => fr_groesse(&z, &alle, v),
            Platz::MaxInhalt => {
                let mut m = 0.0f32;
                for t in 0..n {
                    if defs[t].flexibel() {
                        let f = defs[t].fr();
                        m = m.max(if f > 1.0 { z[t].basis / f } else { z[t].basis });
                    }
                }
                for e in r.elemente {
                    if e.b <= n && (e.a..e.b).any(|t| defs[t].flexibel()) {
                        let ids: Vec<usize> = (e.a..e.b).collect();
                        m = m.max(fr_groesse(&z, &ids, e.beitrag.max));
                    }
                }
                m
            }
        };
        let probe = |z: &[Z], a: f32| -> f32 {
            (0..n).map(|t| if defs[t].flexibel() { z[t].basis.max(a * defs[t].fr()) } else { z[t].basis }).sum::<f32>()
                + luecken_gesamt
        };
        if !matches!(r.platz, Platz::Definit(_)) {
            if let Some(mn) = r.min_platz
                && probe(&z, anteil) < mn - EPS
            {
                anteil = fr_groesse(&z, &alle, mn);
            }
            if let Some(mx) = r.max_platz
                && probe(&z, anteil) > mx + EPS
            {
                anteil = fr_groesse(&z, &alle, mx);
            }
        }
        for t in 0..n {
            if defs[t].flexibel() {
                let v = anteil * defs[t].fr();
                if v > z[t].basis {
                    z[t].basis = v;
                }
            }
        }
    }
    // §12.8 `auto`-Spuren dehnen.
    if r.strecken {
        let platz = r.platz.definit().or(r.min_platz);
        if let Some(p) = platz {
            let frei = p - summe(&z);
            let ids: Vec<usize> = (0..n).filter(|&t| !achse.kollabiert[t] && matches!(defs[t].max, G::Auto) && defs[t].passend.is_none()).collect();
            if frei > EPS && !ids.is_empty() {
                let je = frei / ids.len() as f32;
                for t in ids {
                    z[t].basis += je;
                }
            }
        }
    }
    achse.groesse = z.iter().map(|x| if x.basis.is_finite() { x.basis.max(0.0) } else { 0.0 }).collect();
}

// ===========================================================================
// §10: AUSRICHTUNG
// ===========================================================================

/// Die Verteilung des freien Platzes auf einer Achse (§10.5): Versatz vor
/// der ersten Spur und Zusatz je Luecke. `umgekehrt` = RTL (physisch links
/// ist dann das Ende).
pub(crate) fn inhalt_verteilen(a: Ausr, frei: f32, sichtbar: usize, umgekehrt: bool) -> (f32, f32) {
    use AusrLage as L;
    let lage = match a.lage {
        L::Links => {
            if umgekehrt {
                L::Ende
            } else {
                L::Anfang
            }
        }
        L::Rechts => {
            if umgekehrt {
                L::Anfang
            } else {
                L::Ende
            }
        }
        l => l,
    };
    // Die Ausweichlage der Verteilungen ist `safe` (CSS Box Alignment 3
    // §5.3: `space-around`/`space-evenly` -> `safe center`), ausser `unsafe`
    // steht da. GEMESSEN vorher: WPT `grid-content-distribution-026/027`.
    let verteilung = matches!(a.lage, L::Zwischen | L::Ringsum | L::Gleichmaessig);
    let sicher = a.sicher == Some(true) || (verteilung && a.sicher.is_none());
    let positionell = |l: L| -> f32 {
        let v = match l {
            L::Mitte => frei / 2.0,
            L::Ende => frei,
            _ => 0.0,
        };
        if sicher && frei < 0.0 { 0.0 } else { v }
    };
    match lage {
        L::Zwischen => {
            if frei > 0.0 && sichtbar > 1 {
                (0.0, frei / (sichtbar - 1) as f32)
            } else {
                (positionell(a.ausweich.unwrap_or(L::Anfang)), 0.0)
            }
        }
        L::Ringsum => {
            if frei > 0.0 && sichtbar > 0 {
                let je = frei / sichtbar as f32;
                (je / 2.0, je)
            } else {
                (positionell(a.ausweich.unwrap_or(L::Mitte)), 0.0)
            }
        }
        L::Gleichmaessig => {
            if frei > 0.0 && sichtbar > 0 {
                let je = frei / (sichtbar + 1) as f32;
                (je, je)
            } else {
                (positionell(a.ausweich.unwrap_or(L::Mitte)), 0.0)
            }
        }
        l => (positionell(l), 0.0),
    }
}

/// Die Selbstausrichtung eines Elements in seinem Bereich auf einer Achse:
/// die (Rand-)Lage relativ zum Bereichsanfang. `groesse` ist der Randkasten.
pub(crate) fn selbst_lage(lage: AusrLage, sicher: Option<bool>, bereich: f32, groesse: f32) -> f32 {
    let frei = bereich - groesse;
    let v = match lage {
        AusrLage::Mitte => frei / 2.0,
        AusrLage::Ende => frei,
        _ => 0.0,
    };
    if sicher == Some(true) && frei < 0.0 { 0.0 } else { v }
}

/// Loest `*-self` gegen `*-items` des Containers auf (`auto`/`normal` ->
/// der Wert des Containers).
pub(crate) fn selbst_aufloesen(selbst: Ausr, elemente: Ausr) -> Ausr {
    // Release 11.10.2026: nur `auto` nimmt den Wert des Containers; ein
    // angegebenes `normal` bleibt `normal` (CSS Box Alignment 3 §6.1).
    // GEMESSEN vorher: WPT `flex-abspos-staticpos-align-self-001`
    // (`align-self: normal` wurde zu `align-items: center`).
    match selbst.lage {
        AusrLage::Auto => elemente,
        _ => selbst,
    }
}

/// Die Luecke einer Achse aufloesen (Prozent gegen die definite Groesse,
/// sonst 0).
pub(crate) fn luecke_px(l: Option<LaengeProzent>, basis: Option<f32>, alt: f32) -> f32 {
    match l {
        Some(l) => {
            if l.hat_prozent {
                l.aufloesen(basis).unwrap_or(l.px).max(0.0)
            } else {
                l.px.max(0.0)
            }
        }
        None => alt.max(0.0),
    }
}

// ===========================================================================
// DIE ANBINDUNG AN DIE ENGINE (Messphase `box.rs`, Platzierung `flow.rs`)
// ===========================================================================

use crate::box_sizing::{Sizes, get_node, resolve_spacing};
use crate::plan::Platzierung;
use crate::{Ctx, LayoutError};
use narsil_core::geometry::{Edges, Rect, Size};
use narsil_core::length::Length;
use narsil_dom::{NodeId, NodeKind};
use narsil_layout::plan::PositionArt;

/// Das Ergebnis eines Rasters, zwischen Messphase und Platzierung geteilt
/// (`Ctx::gitter`): beide Phasen legen mit DENSELBEN Spuren.
#[derive(Debug, Clone)]
pub(crate) struct Ergebnis {
    /// Die Inhaltsbreite, mit der gerechnet wurde.
    pub breite: f32,
    /// Die definite Inhaltshoehe (`None` = unbestimmt).
    pub hoehe: Option<f32>,
    pub raster: Raster,
    /// Die Rasterelemente: (Index im DOM, Knoten), parallel zu `raster.bereiche`.
    pub kinder: Vec<(usize, NodeId)>,
    /// Kamen Spuren eines Subgrids vom Eltern?
    pub vom_eltern: bool,
    /// Sind die Zeilen gerechnet (sonst nur die Spalten, Messphase Teil 1)?
    pub zeilen_fertig: bool,
    /// Masonry: je Element, ob es automatisch platziert ist — ein solches
    /// Subgrid erbt keine Liniennamen (seine Lage ist beim Platzieren seiner
    /// Elemente noch offen). Leer = alle fest.
    pub auto_platziert: Vec<bool>,
}

/// Was die Messphase tun soll.
pub(crate) enum Messung {
    /// Diese Kinder mit dieser verfuegbaren Groesse neu messen, dann noch einmal.
    Nachmessen(Vec<(NodeId, Size)>),
    /// Fertig: die Inhaltshoehe des Rasters.
    Fertig(f32),
}

/// Die Masse eines Rasterelements, die die Rechnung braucht.
#[derive(Debug, Clone, Default)]
pub(crate) struct Masse {
    pub(crate) rand: Edges,
    /// `margin: auto` links, oben, rechts, unten.
    pub(crate) rand_auto: [bool; 4],
    pub(crate) min_b: Option<f32>,
    pub(crate) max_b: Option<f32>,
    pub(crate) min_h: Option<f32>,
    pub(crate) max_h: Option<f32>,
    pub(crate) breite_auto: bool,
    pub(crate) breite_prozent: bool,
    pub(crate) hoehe_auto: bool,
    pub(crate) scroll: bool,
    pub(crate) ersetzt: bool,
    pub(crate) text: bool,
    /// Die Glieder eines anonymen Elements (mehr als eins).
    pub(crate) glieder: Option<std::rc::Rc<Vec<NodeId>>>,
    /// Innenabstand + Rahmen waagerecht/senkrecht.
    pub(crate) fuell: (f32, f32),
    /// `box-sizing: border-box`.
    pub(crate) border_box: bool,
    /// `width: <p>%`.
    pub(crate) prozent_b: Option<f32>,
    /// `width` als Schluesselwort (1 min-, 2 max-, 3 fit-content).
    pub(crate) breite_wort: u8,
    /// `min-width`/`max-width` als Schluesselwort.
    pub(crate) grenz_worte: (u8, u8),
    /// Die Raender fuer die Beitraege: Prozentraender zaehlen dort als 0
    /// (CSS Sizing 3 §5.2.1, zyklisch zum Rasterbereich).
    pub(crate) rand_beitrag: Edges,
    /// `min-width`/`max-width` in Prozent (zaehlen fuer die Beitraege als
    /// `auto`/`none`, CSS Sizing 3 §5.2.1).
    pub(crate) grenzen_prozent_b: (bool, bool),
}

impl Masse {
    /// Die Rahmenbreite einer Prozentbreite gegen `basis`.
    fn prozent_breite(&self, basis: f32) -> Option<f32> {
        let p = self.prozent_b?;
        let inhalt = basis * p / 100.0;
        Some(klemmen(if self.border_box { inhalt } else { inhalt + self.fuell.0 }, self.min_b, self.max_b))
    }
}

pub(crate) fn masse(ctx: &Ctx<'_>, id: NodeId, basis: f32, basis_h: f32) -> Result<Masse, LayoutError> {
    let node = get_node(ctx, id)?;
    if node.kind == NodeKind::Text || ist_anonym(ctx, id) {
        let g = gruppe(ctx, id);
        let glieder = (g.len() > 1).then(|| std::rc::Rc::new(g));
        return Ok(Masse { breite_auto: true, hoehe_auto: true, text: true, glieder, ..Masse::default() });
    }
    let st = ctx.stil(id);
    let rand = resolve_spacing(node.style.margin, basis, id, "margin")?;
    let fuell = resolve_spacing(node.style.padding, basis, id, "padding")?;
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    let fh = fuell.left + fuell.right + rahmen.left + rahmen.right;
    let fv = fuell.top + fuell.bottom + rahmen.top + rahmen.bottom;
    let (min_b, max_b, min_h, max_h) = crate::r#box::grenzen_rahmen(ctx, id, Size::new(basis, basis_h), (fh, fv));
    let auto = |l: Length| l == Length::Auto;
    let m = node.style.margin;
    let rand_auto = [
        auto(m.left) || st.is_some_and(|s| s.marge_links_auto),
        auto(m.top) || st.is_some_and(|s| s.marge_oben_auto),
        auto(m.right) || st.is_some_and(|s| s.marge_rechts_auto),
        auto(m.bottom) || st.is_some_and(|s| s.marge_unten_auto),
    ];
    Ok(Masse {
        rand,
        rand_auto,
        min_b,
        max_b,
        min_h,
        max_h,
        breite_auto: auto(node.style.size.width) && st.is_none_or(|s| s.groessen_art[0] == 0),
        breite_prozent: node.style.size.width.hat_prozent(),
        hoehe_auto: !matches!(node.style.size.height, Length::Px(_)) && st.is_none_or(|s| s.groessen_art[3] == 0),
        breite_wort: st.map_or(0, |s| s.groessen_art[0]),
        grenz_worte: st.map_or((0, 0), |s| (s.groessen_art[1], s.groessen_art[2])),
        grenzen_prozent_b: ctx.plan.map_or((false, false), |p| {
            let g = p.grenzen_prozent(id);
            (g.min_breite.is_some(), g.max_breite.is_some())
        }),
        rand_beitrag: {
            // Release 14.10.2026: a `calc()` margin keeps its fixed part (only
            // the percentage is cyclic). MEASURED before: WPT css-sizing
            // `calc-margins-flex` (`margin-left: calc(10% + 100px)` counted 0).
            let fest = |l: Length, v: f32| match l {
                Length::Gemischt { px, .. } if px.is_finite() => px,
                l if l.hat_prozent() => 0.0,
                _ => v,
            };
            Edges::new(fest(m.left, rand.left), fest(m.top, rand.top), fest(m.right, rand.right), fest(m.bottom, rand.bottom))
        },
        // Release 11.10.2026: auch `overflow-y` allein (CSS Overflow 3 §3).
        // GEMESSEN vorher: WPT `flexbox-min-height-auto-003` (84 statt 30 px).
        scroll: st.is_some_and(|s| s.rollkasten),
        ersetzt: node.ersetzt.is_some() || st.is_some_and(|s| s.seitenverhaeltnis.is_finite() && s.seitenverhaeltnis != 0.0),
        text: false,
        glieder: None,
        fuell: (fh, fv),
        border_box: st.is_some_and(|s| s.border_box),
        prozent_b: match node.style.size.width {
            Length::Percent(p) if p.is_finite() => Some(p),
            _ => None,
        },
    })
}

pub(crate) fn klemmen(v: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let mut v = v;
    if let Some(m) = max {
        v = v.min(m);
    }
    if let Some(m) = min {
        v = v.max(m);
    }
    v.max(0.0)
}

/// min-/max-content-Breite (Rahmenkasten, geklemmt) eines Elements.
fn inhalt_breiten(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, m: &Masse, tiefe: usize) -> (f32, f32) {
    let gemessen = sizes.get(id).map_or(0.0, |s| s.width);
    // `width: min-content | max-content | fit-content` (CSS Sizing 3 §5.1).
    if m.breite_wort != 0 {
        let mi = sizes.min_inhalt_breite(ctx, id, tiefe);
        let ma = sizes.max_content_breite(ctx, id, tiefe).max(mi);
        let (a, b) = match m.breite_wort {
            1 => (mi, mi),
            2 => (ma, ma),
            _ => (mi, ma),
        };
        return (klemmen(a, m.min_b, m.max_b), klemmen(b, m.min_b, m.max_b));
    }
    let (mi, ma) = if m.text || m.breite_auto || m.breite_prozent {
        let mi = sizes.min_inhalt_breite(ctx, id, tiefe);
        let ma = sizes.max_content_breite(ctx, id, tiefe);
        (mi, ma.max(mi))
    } else {
        // Eine feste Breite; vom Flex-Algorithmus gesetzt zaehlt die Angabe
        // (`box.rs`, `flex_feste_breite`).
        let g = ctx.document.get(id).and_then(|n| crate::r#box::flex_feste_breite(ctx, id, n)).unwrap_or(gemessen);
        (g, g)
    };
    (klemmen(mi, m.min_b, m.max_b), klemmen(ma, m.min_b, m.max_b))
}

/// `min-width`/`max-width: min-content | max-content | fit-content` als
/// Grenzen (Rahmenkasten) eintragen (CSS Sizing 3 §2.1).
pub(crate) fn mit_grenz_worten(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, m: &Masse, tiefe: usize) -> Masse {
    let mut m = m.clone();
    if m.grenz_worte == (0, 0) {
        return m;
    }
    // Release 14.10.2026: the box's OWN sizes (see `Sizes::eigene_breite`).
    let _ = tiefe;
    let mi = sizes.eigene_breite(ctx, id, false);
    let ma = sizes.eigene_breite(ctx, id, true).max(mi);
    let wert = |a: u8| match a {
        1 => Some(mi),
        2 | 3 => Some(ma),
        _ => None,
    };
    if let Some(v) = wert(m.grenz_worte.0) {
        m.min_b = Some(v);
    }
    if let Some(v) = wert(m.grenz_worte.1) {
        m.max_b = Some(v);
    }
    m
}

pub(crate) fn spalten_beitrag(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, m: &Masse, tiefe: usize) -> Beitrag {
    let mut m = mit_grenz_worten(ctx, sizes, id, m, tiefe);
    // Prozentgrenzen sind zum Rasterbereich zyklisch: fuer den Beitrag
    // `auto`/`none` (GEMESSEN: WPT `grid-item-dynamic-min-contribution-001`).
    if m.grenzen_prozent_b.0 {
        m.min_b = None;
    }
    if m.grenzen_prozent_b.1 {
        m.max_b = None;
    }
    let m = &m;
    // Ein anonymes Element: die Zeilen eines Absatzes (min-content = das
    // breiteste Glied, max-content = die Zeilen hintereinander).
    if m.text && ist_anonym(ctx, id) {
        let mut mi = 0.0f32;
        let mut ma = 0.0f32;
        for g in gruppe(ctx, id) {
            mi = mi.max(sizes.min_inhalt_breite(ctx, g, tiefe));
            ma += sizes.max_content_breite(ctx, g, tiefe);
        }
        let ma = ma.max(mi);
        return Beitrag { min: mi, max: ma, minimum: mi, minimum_inhalt: true };
    }
    let (mi, ma) = inhalt_breiten(ctx, sizes, id, m, tiefe);
    let mr = m.rand_beitrag.left + m.rand_beitrag.right;
    let (minimum, inhalt) = if m.text {
        (mi, true)
    } else if m.breite_auto || m.breite_prozent {
        match m.min_b {
            Some(mb) => (mb, false),
            None if m.scroll => (0.0, false),
            None => (mi, true),
        }
    } else {
        (mi, false)
    };
    Beitrag { min: mi + mr, max: ma + mr, minimum: minimum + mr, minimum_inhalt: inhalt }
}

fn zeilen_beitrag(sizes: &Sizes, id: NodeId, m: &Masse) -> Beitrag {
    let h = sizes.get(id).map_or(0.0, |s| s.height);
    let h = match &m.glieder {
        Some(g) => g.iter().map(|x| sizes.get(*x).map_or(0.0, |s| s.height)).sum(),
        None => h,
    };
    let mv = m.rand_beitrag.top + m.rand_beitrag.bottom;
    let (minimum, inhalt) = if m.hoehe_auto && !m.text {
        match m.min_h {
            Some(mh) => (mh, false),
            None if m.scroll => (0.0, false),
            None => (h, true),
        }
    } else {
        (h, m.text)
    };
    Beitrag { min: h + mv, max: h + mv, minimum: minimum + mv, minimum_inhalt: inhalt }
}

/// Die Rasterelemente in `order`-Reihenfolge (CSS Display 3) und die absolut
/// positionierten Kinder.
pub(crate) fn rasterelemente(ctx: &Ctx<'_>, node: &narsil_dom::Node) -> (Vec<(usize, NodeId)>, Vec<NodeId>) {
    let n = node.children.len().min(crate::plan::MAX_KINDER);
    // Release 11.10.2026: eine zusammenhaengende Folge von Text (Textknoten
    // und Zeilenkaesten des Seitenbauers, ohne Plan) ist EIN anonymes
    // Rasterelement (CSS Grid 1 §6.1); reiner Leerraum keines. GEMESSEN
    // vorher: jede Zeile eines Absatzes war ein eigenes Element (WPT
    // `subgrid/auto-track-sizing-001`: vier Zeilen in EINER Zeile uebereinander).
    let mut folge: Vec<(usize, NodeId)> = Vec::with_capacity(n);
    let mut im_lauf = false;
    for (i, &id) in node.children.iter().take(n).enumerate() {
        if ist_anonym(ctx, id) {
            if nur_leerraum(ctx, id, 0) {
                continue;
            }
            if !im_lauf {
                folge.push((i, id));
            }
            im_lauf = true;
        } else {
            im_lauf = false;
            folge.push((i, id));
        }
    }
    let ordnung = |id: NodeId| ctx.stil(id).map_or(0, |s| s.ordnung);
    if folge.iter().any(|&(_, id)| ordnung(id) != 0) {
        folge.sort_by_key(|&(i, id)| (ordnung(id), i));
    }
    let mut kinder = Vec::with_capacity(folge.len());
    let mut abspos = Vec::new();
    for (i, id) in folge {
        let (gezeichnet, pos) = match ctx.stil(id) {
            Some(s) => (s.wird_gezeichnet(), s.position),
            None => (true, PositionArt::Statisch),
        };
        if !gezeichnet {
            continue;
        }
        match pos {
            PositionArt::Absolut => abspos.push(id),
            PositionArt::Fest => {}
            _ => kinder.push((i, id)),
        }
    }
    (kinder, abspos)
}

/// Ein Kind ohne Plan: Text oder ein Zeilenkasten des Seitenbauers.
pub(crate) fn ist_anonym(ctx: &Ctx<'_>, id: NodeId) -> bool {
    ctx.stil(id).is_none()
}

/// Enthaelt der Knoten nur Leerraum?
fn nur_leerraum(ctx: &Ctx<'_>, id: NodeId, tiefe: usize) -> bool {
    let Some(n) = ctx.document.get(id) else { return true };
    if n.kind == NodeKind::Text {
        return n.text.as_deref().is_none_or(|t| t.trim().is_empty());
    }
    if tiefe > 8 || n.ersetzt.is_some() {
        return false;
    }
    n.children.iter().all(|&c| nur_leerraum(ctx, c, tiefe + 1))
}

/// Die Glieder eines anonymen Rasterelements, das bei `id` beginnt.
fn gruppe(ctx: &Ctx<'_>, id: NodeId) -> Vec<NodeId> {
    let Some(eltern) = ctx.document.get(id).and_then(|n| n.parent).and_then(|p| ctx.document.get(p)) else {
        return vec![id];
    };
    let Some(start) = eltern.children.iter().position(|c| *c == id) else { return vec![id] };
    let mut raus = Vec::new();
    for &c in eltern.children.iter().skip(start) {
        if !ist_anonym(ctx, c) {
            break;
        }
        if nur_leerraum(ctx, c, 0) && c != id {
            continue;
        }
        raus.push(c);
    }
    raus
}

fn kind_stil<'a>(ctx: &'a Ctx<'_>, id: NodeId) -> Option<&'a narsil_layout::gitter::GitterKindStil> {
    ctx.stil(id).and_then(|s| s.gitter_kind.as_deref())
}

/// Ist `id` ein Subgrid (CSS Grid 2 §9) auf der Spalten- bzw. Zeilenachse?
/// Nur ein Rasterelement eines Rasters (oder auf der Rasterachse eines
/// Masonry-Containers) kann eines sein; sonst gilt `subgrid` als `none`.
pub(crate) fn subgrid_achsen(ctx: &Ctx<'_>, id: NodeId) -> (bool, bool) {
    let Some(st) = ctx.stil(id) else { return (false, false) };
    let Some(gs) = st.gitter.as_deref() else { return (false, false) };
    if !(gs.spalten.subgrid || gs.zeilen.subgrid) || matches!(st.position, PositionArt::Absolut | PositionArt::Fest) {
        return (false, false);
    }
    let Some(eltern) = ctx.document.get(id).and_then(|n| n.parent) else { return (false, false) };
    let Some(est) = ctx.stil(eltern) else { return (false, false) };
    if est.gitter.is_none() || est.anzeige != narsil_layout::plan::Anzeige::Raster {
        return (false, false);
    }
    // Masonry: nur die Rasterachse kann geteilt werden.
    let (sp_ok, ze_ok) = match est.lanes {
        0 => (true, true),
        1 => (true, false),
        _ => (false, true),
    };
    (gs.spalten.subgrid && sp_ok, gs.zeilen.subgrid && ze_ok)
}

/// Rand + Rahmen + Innenabstand eines Kastens an Anfang und Ende einer Achse.
fn rand_aussen(ctx: &Ctx<'_>, id: NodeId, basis: f32, spalten: bool) -> (f32, f32) {
    let Ok(node) = get_node(ctx, id) else { return (0.0, 0.0) };
    let rand = resolve_spacing(node.style.margin, basis, id, "margin").unwrap_or(Edges::ZERO);
    let fuell = resolve_spacing(node.style.padding, basis, id, "padding").unwrap_or(Edges::ZERO);
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    if spalten {
        (rand.left + fuell.left + rahmen.left, rand.right + fuell.right + rahmen.right)
    } else {
        (rand.top + fuell.top + rahmen.top, rand.bottom + fuell.bottom + rahmen.bottom)
    }
}

/// Die eigenen Liniennamen eines Subgrids (`subgrid [a] [b]`), je Linie.
fn subgrid_namen(liste: &SpurListe, n: usize) -> Vec<Vec<String>> {
    // Release 11.10.2026: `repeat(auto-fill, <line-names>+)` wiederholt die
    // Namen so oft, wie neben den uebrigen Namen Linien frei sind (CSS Grid 2
    // §9.3); ueberzaehlige Namen fallen weg. GEMESSEN vorher: WPT
    // `subgrid/repeat-auto-fill-001..008` (die Wiederholung fehlte).
    let linien = n + 1;
    let mut vor: Vec<Vec<String>> = Vec::new();
    let mut nach: Vec<Vec<String>> = Vec::new();
    let mut wiederholt: Option<Vec<Vec<String>>> = None;
    for t in &liste.teile {
        match t {
            SpurTeil::Namen(nn) if wiederholt.is_some() => nach.push(nn.clone()),
            SpurTeil::Namen(nn) => vor.push(nn.clone()),
            SpurTeil::Auto { teile, .. } if wiederholt.is_none() => {
                wiederholt = Some(
                    teile
                        .iter()
                        .filter_map(|x| if let SpurTeil::Namen(nn) = x { Some(nn.clone()) } else { None })
                        .collect(),
                );
            }
            _ => {}
        }
    }
    let mut folge = vor;
    if let Some(gruppe) = wiederholt.filter(|g| !g.is_empty()) {
        let frei = linien.saturating_sub(folge.len() + nach.len());
        for _ in 0..(frei / gruppe.len()).min(10_000) {
            folge.extend(gruppe.iter().cloned());
        }
    }
    folge.extend(nach);
    let mut raus = vec![Vec::new(); linien];
    for (k, nn) in folge.into_iter().enumerate().take(linien) {
        raus[k].extend(nn);
    }
    raus
}

/// Die Spuren einer Subgrid-Achse aus dem Ergebnis des Elterns (`None`, wenn
/// der Eltern noch nicht gerechnet ist).
fn subgrid_vorgabe(ctx: &Ctx<'_>, node: &narsil_dom::Node, gs: &GitterStil, spalten: bool) -> Option<Vorgabe> {
    let eltern = node.parent?;
    let cache = ctx.gitter.borrow();
    let e = cache.get(&eltern)?;
    if !spalten && !e.zeilen_fertig {
        return None;
    }
    let i = e.kinder.iter().position(|(_, id)| *id == node.id)?;
    let b = *e.raster.bereiche.get(i)?;
    let achse = if spalten { &e.raster.spalten } else { &e.raster.zeilen };
    let (a0, a1) = if spalten { (b.s0, b.s1) } else { (b.z0, b.z1) };
    if a1 <= a0 || a1 > achse.groesse.len() {
        return None;
    }
    let n = a1 - a0;
    let g = achse.luecke;
    let eigen = if spalten { gs.luecke_spalte } else { gs.luecke_zeile };
    let basis = if spalten { Some(e.breite) } else { e.hoehe };
    let g2 = match eigen {
        Some(l) => luecke_px(Some(l), basis, g),
        None => g,
    };
    let (rand0, rand1) = rand_aussen(ctx, node.id, e.breite, spalten);
    let mut groessen: Vec<f32> = achse.groesse[a0..a1].to_vec();
    for (k, x) in groessen.iter_mut().enumerate() {
        if k > 0 {
            *x += (g - g2) / 2.0;
        } else {
            *x -= rand0;
        }
        if k + 1 < n {
            *x += (g - g2) / 2.0;
        } else {
            *x -= rand1;
        }
        // Release 11.10.2026: eine Spur darf durch den Lueckenunterschied
        // negativ werden — die Linien bleiben dann an der Lage des Elterns
        // (CSS Grid 2 §9.5: die Luecke liegt mittig ueber der des Elterns).
        // GEMESSEN vorher: WPT `subgrid/grid-gap-010` (leere `auto`-Spur
        // mit `gap: 100px`: das Element stand bei 100 statt 50 px).
        if !(x.is_finite()) {
            *x = 0.0;
        }
    }
    // Die Namen: die des Elterns an diesen Linien und die eigenen (ein
    // automatisch platziertes Subgrid in Masonry erbt keine, GEMESSEN an WPT
    // `grid-lanes/subgrid/.../column-line-names-016`).
    let mut namen = subgrid_namen(if spalten { &gs.spalten } else { &gs.zeilen }, n);
    let erbt = !e.auto_platziert.get(i).copied().unwrap_or(false);
    for (k, l) in namen.iter_mut().enumerate().filter(|_| erbt) {
        let li = (a0 + k) as i32 - achse.versatz;
        if li >= 0
            && let Some(nn) = achse.namen.get(li as usize)
        {
            l.extend(nn.iter().cloned());
        }
    }
    Some(Vorgabe { groessen, namen, luecke: g2 })
}

/// Die Beitraege der Elemente eines Subgrids zur Spurrechnung des Elterns
/// (CSS Grid 2 §9: ihre Raender wachsen an den Kanten um Rand, Rahmen und
/// Innenabstand des Subgrids und an inneren Linien um den halben
/// Luecken-Unterschied). `a0..a1` ist der Bereich des Subgrids im Eltern.
#[allow(clippy::too_many_arguments)]
fn subgrid_elemente(
    ctx: &Ctx<'_>,
    sizes: &Sizes,
    id: NodeId,
    spalten: bool,
    a0: usize,
    a1: usize,
    luecke_eltern: f32,
    namen_eltern: &[Vec<String>],
    basis: f32,
    tiefe: usize,
    raus: &mut Vec<AchsElement>,
) {
    subgrid_elemente_mit(ctx, sizes, id, spalten, a0, a1, luecke_eltern, namen_eltern, basis, tiefe, (0.0, 0.0), raus);
}

/// Release 11.10.2026: wie [`subgrid_elemente`]; `aussen` sind die Raender
/// umgebender Subgrids an der ersten/letzten Spur dieses Subgrids (sie
/// gehoeren zu den Elementen an diesen Kanten, CSS Grid 2 §9.5).
#[allow(clippy::too_many_arguments)]
fn subgrid_elemente_mit(
    ctx: &Ctx<'_>,
    sizes: &Sizes,
    id: NodeId,
    spalten: bool,
    a0: usize,
    a1: usize,
    luecke_eltern: f32,
    namen_eltern: &[Vec<String>],
    basis: f32,
    tiefe: usize,
    aussen: (f32, f32),
    raus: &mut Vec<AchsElement>,
) {
    let Some(node) = ctx.document.get(id) else { return };
    let Some(gs) = ctx.stil(id).and_then(|s| s.gitter.as_deref()) else { return };
    if tiefe > 32 || a1 <= a0 {
        return;
    }
    let n = a1 - a0;
    let (sub_sp, sub_ze) = subgrid_achsen(ctx, id);
    let eigen = if spalten { gs.luecke_spalte } else { gs.luecke_zeile };
    let g2 = match eigen {
        Some(l) => luecke_px(Some(l), Some(basis), luecke_eltern),
        None => luecke_eltern,
    };
    let mut namen = subgrid_namen(if spalten { &gs.spalten } else { &gs.zeilen }, n);
    for (k, l) in namen.iter_mut().enumerate() {
        if let Some(nn) = namen_eltern.get(k) {
            l.extend(nn.iter().cloned());
        }
    }
    let vorgabe = Vorgabe { groessen: vec![0.0; n], namen, luecke: g2 };
    // Die andere Achse: bei einem zweiachsigen Subgrid ist ihre Zahl hier
    // unbekannt; dann nehmen wir eine Spur je Bereich nicht vorweg.
    let mut vorgaben = Vorgaben::default();
    if spalten {
        vorgaben.spalten = Some(vorgabe);
        let _ = sub_ze;
    } else {
        vorgaben.zeilen = Some(vorgabe);
        let _ = sub_sp;
    }
    let (kinder, _) = rasterelemente(ctx, node);
    let leer = GitterPlatz::default();
    let eingabe: Vec<PlatzEingabe<'_>> = kinder
        .iter()
        .map(|(_, kid)| PlatzEingabe { platz: kind_stil(ctx, *kid).map_or(&leer, |k| &k.platz) })
        .collect();
    let r = platzieren(gs, &eingabe, None, None, None, None, g2, g2, None, None, &vorgaben);
    let (rand0, rand1) = rand_aussen(ctx, id, basis, spalten);
    let (rand0, rand1) = (rand0 + aussen.0, rand1 + aussen.1);
    let halb = (g2 - luecke_eltern) / 2.0;
    let (mut am_anfang, mut am_ende) = (false, false);
    for (b, (_, kid)) in r.bereiche.iter().zip(&kinder) {
        let (s0, s1) = if spalten { (b.s0, b.s1) } else { (b.z0, b.z1) };
        let (s0, s1) = (s0.min(n - 1), s1.clamp(s0.min(n - 1) + 1, n));
        am_anfang |= s0 == 0;
        am_ende |= s1 == n;
        let extra = if s0 == 0 { rand0 } else { halb } + if s1 == n { rand1 } else { halb };
        // Ein Subgrid im Subgrid: dessen Elemente (mit unseren Raendern an
        // den gemeinsamen Kanten).
        let (k_sp, k_ze) = subgrid_achsen(ctx, *kid);
        if (spalten && k_sp) || (!spalten && k_ze) {
            let sub_namen: Vec<Vec<String>> = (s0..=s1).map(|k| r_namen(&r, spalten, k)).collect();
            let innen = (if s0 == 0 { rand0 } else { halb }, if s1 == n { rand1 } else { halb });
            subgrid_elemente_mit(ctx, sizes, *kid, spalten, a0 + s0, a0 + s1, g2, &sub_namen, basis, tiefe + 1, innen, raus);
            continue;
        }
        let Ok(m) = masse(ctx, *kid, basis, 0.0) else { continue };
        let mut beitrag = if spalten { spalten_beitrag(ctx, sizes, *kid, &m, tiefe) } else { zeilen_beitrag(sizes, *kid, &m) };
        beitrag.min = (beitrag.min + extra).max(0.0);
        beitrag.max = (beitrag.max + extra).max(0.0);
        beitrag.minimum = (beitrag.minimum + extra).max(0.0);
        raus.push(AchsElement { a: a0 + s0, b: a0 + s1, beitrag });
    }
    // Release 11.10.2026: ohne Element in der ersten bzw. letzten Spur steht
    // dort ein gedachtes leeres Element mit den Raendern des Subgrids (CSS
    // Grid 2 §9.5). GEMESSEN vorher: WPT `subgrid/subgrid-no-items-on-edges-001`
    // (leere Subgrids: 0 x 0 statt 100 x 100 px).
    let leer = |v: f32| Beitrag { min: v.max(0.0), max: v.max(0.0), minimum: v.max(0.0), minimum_inhalt: false };
    if n == 1 && !am_anfang && !am_ende {
        raus.push(AchsElement { a: a0, b: a0 + 1, beitrag: leer(rand0 + rand1) });
    } else {
        if !am_anfang {
            raus.push(AchsElement { a: a0, b: a0 + 1, beitrag: leer(rand0 + if n == 1 { rand1 } else { 0.0 }) });
        }
        if !am_ende && n > 1 {
            raus.push(AchsElement { a: a0 + n - 1, b: a0 + n, beitrag: leer(rand1) });
        }
    }
}

/// Die Namen einer Linie (Spurindex `k`) eines platzierten Rasters.
fn r_namen(r: &Raster, spalten: bool, k: usize) -> Vec<String> {
    let a = if spalten { &r.spalten } else { &r.zeilen };
    let li = k as i32 - a.versatz;
    if li < 0 {
        return Vec::new();
    }
    a.namen.get(li as usize).cloned().unwrap_or_default()
}

/// Platzierung und Spurgroessen. `breite` ist der Platz der Spalten,
/// `hoehe` die definite Inhaltshoehe; `mit_zeilen = false` rechnet nur die
/// Spalten (Messphase, Teil 1; intrinsische Breiten).
#[allow(clippy::too_many_arguments)]
pub(crate) fn rechnen(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    gs: &GitterStil,
    sizes: &Sizes,
    breite: Platz,
    hoehe: Option<f32>,
    min_h: Option<f32>,
    max_h: Option<f32>,
    mit_zeilen: bool,
    tiefe: usize,
) -> Result<Ergebnis, LayoutError> {
    let (kinder, _) = rasterelemente(ctx, node);
    let leer = GitterPlatz::default();
    let eingabe: Vec<PlatzEingabe<'_>> = kinder
        .iter()
        .map(|(_, id)| PlatzEingabe { platz: kind_stil(ctx, *id).map_or(&leer, |k| &k.platz) })
        .collect();
    let breite_def = breite.definit();
    // Subgrid-Achsen: die Spuren kommen vom Eltern (CSS Grid 2 §9).
    let (sub_sp, sub_ze) = subgrid_achsen(ctx, node.id);
    let mut vorgaben = Vorgaben {
        spalten: if sub_sp { subgrid_vorgabe(ctx, node, gs, true) } else { None },
        zeilen: if sub_ze { subgrid_vorgabe(ctx, node, gs, false) } else { None },
        ..Vorgaben::default()
    };
    if gs.spalten.hat_auto_wiederholung() || gs.zeilen.hat_auto_wiederholung() {
        let basis0 = breite_def.unwrap_or(0.0);
        let mut sp = (0.0f32, 0.0f32);
        let mut ze = (0.0f32, 0.0f32);
        for (_, id) in &kinder {
            if let Ok(m) = masse(ctx, *id, basis0, hoehe.unwrap_or(0.0)) {
                let (ks, kz) = spannen_schaetzung(ctx, *id);
                let b = spalten_beitrag(ctx, sizes, *id, &m, tiefe);
                sp = (sp.0.max(b.min / ks), sp.1.max(b.max / ks));
                let z = zeilen_beitrag(sizes, *id, &m);
                ze = (ze.0.max(z.min / kz), ze.1.max(z.max / kz));
            }
        }
        vorgaben.schaetzung_spalten = Some(sp);
        vorgaben.schaetzung_zeilen = Some(ze);
    }
    let ls = vorgaben.spalten.as_ref().map_or_else(|| luecke_px(gs.luecke_spalte, breite_def, 0.0), |v| v.luecke);
    let lz = vorgaben.zeilen.as_ref().map_or_else(|| luecke_px(gs.luecke_zeile, hoehe, 0.0), |v| v.luecke);
    // Release 11.10.2026: ohne definite Breite bestimmt `min-width` die Zahl
    // der Auto-Wiederholungen (CSS Grid 1 §7.2.3.2: die kleinste, die das
    // Minimum erfuellt). GEMESSEN vorher: WPT
    // `grid-definition/grid-auto-repeat-dynamic-001` (`min-width: 60%` in
    // 100 px: eine statt zwei Spalten, 60 statt 100 px breit).
    let spalten_min = if breite_def.is_none() { lanes_grenzen(ctx, sizes, node, false).0.filter(|m| *m > 0.0) } else { None };
    let mut raster = platzieren(gs, &eingabe, breite_def, spalten_min, hoehe.or(max_h), min_h, ls, lz, breite_def, hoehe, &vorgaben);
    let basis = breite_def.unwrap_or(0.0);
    let mut masse_alle = Vec::with_capacity(kinder.len());
    for (_, id) in &kinder {
        masse_alle.push(masse(ctx, *id, basis, hoehe.unwrap_or(0.0))?);
    }
    let kind_sub: Vec<(bool, bool)> = kinder.iter().map(|(_, id)| subgrid_achsen(ctx, *id)).collect();
    let normal = |a: Ausr| matches!(a.lage, AusrLage::Normal | AusrLage::Strecken);
    if let Some(v) = &vorgaben.spalten {
        raster.spalten.groesse = v.groessen.clone();
    } else {
        let mut spalten_el: Vec<AchsElement> = Vec::with_capacity(kinder.len());
        for (i, ((b, (_, id)), m)) in raster.bereiche.iter().zip(&kinder).zip(&masse_alle).enumerate() {
            if kind_sub[i].0 {
                let namen: Vec<Vec<String>> = (b.s0..=b.s1).map(|k| r_namen(&raster, true, k)).collect();
                subgrid_elemente(ctx, sizes, *id, true, b.s0, b.s1, ls, &namen, basis, 0, &mut spalten_el);
            } else {
                spalten_el.push(AchsElement { a: b.s0, b: b.s1, beitrag: spalten_beitrag(ctx, sizes, *id, m, tiefe) });
            }
        }
        spuren_groessen(
            &mut raster.spalten,
            &AchsRechnung {
                platz: breite,
                prozent_basis: breite_def,
                min_platz: None,
                max_platz: None,
                strecken: normal(gs.justify_content),
                elemente: &spalten_el,
            },
        );
    }
    if let Some(v) = &vorgaben.zeilen {
        raster.zeilen.groesse = v.groessen.clone();
    } else if mit_zeilen {
        // Grundlinien-Ausrichtung: der Versatz zaehlt zum Beitrag (§12.5
        // Schritt 1, „shim baseline-aligned items").
        let probe = Ergebnis {
            breite: basis,
            hoehe,
            raster: raster.clone(),
            kinder: kinder.clone(),
            vom_eltern: false,
            zeilen_fertig: false,
            auto_platziert: Vec::new(),
        };
        let versatz = grundlinien_versaetze(ctx, gs, sizes, &probe, basis, node);
        let mut zeilen_el: Vec<AchsElement> = Vec::with_capacity(kinder.len());
        for (i, ((b, (_, id)), m)) in raster.bereiche.iter().zip(&kinder).zip(&masse_alle).enumerate() {
            if kind_sub[i].1 {
                let namen: Vec<Vec<String>> = (b.z0..=b.z1).map(|k| r_namen(&raster, false, k)).collect();
                subgrid_elemente(ctx, sizes, *id, false, b.z0, b.z1, lz, &namen, basis, 0, &mut zeilen_el);
            } else {
                let mut beitrag = zeilen_beitrag(sizes, *id, m);
                if let Some(&(a, v)) = versatz.get(i)
                    && a != 0
                {
                    beitrag.min += v;
                    beitrag.max += v;
                    beitrag.minimum += v;
                }
                zeilen_el.push(AchsElement { a: b.z0, b: b.z1, beitrag });
            }
        }
        spuren_groessen(
            &mut raster.zeilen,
            &AchsRechnung {
                platz: hoehe.map_or(Platz::MaxInhalt, Platz::Definit),
                prozent_basis: hoehe,
                min_platz: min_h,
                max_platz: max_h,
                strecken: normal(gs.align_content),
                elemente: &zeilen_el,
            },
        );
    }
    Ok(Ergebnis {
        breite: basis,
        hoehe,
        raster,
        kinder,
        vom_eltern: vorgaben.spalten.is_some() || vorgaben.zeilen.is_some(),
        zeilen_fertig: mit_zeilen || vorgaben.zeilen.is_some(),
        auto_platziert: Vec::new(),
    })
}

/// Die Lagen der Spurenden nach der Inhaltsverteilung: (Lagen, Ende je Spur).
fn spur_lagen(a: &Achse, ausr: Ausr, platz: f32, umgekehrt: bool) -> Vec<f32> {
    let sichtbar = a.kollabiert.iter().filter(|k| !**k).count();
    let frei = platz - a.gesamt();
    let (vor, zw) = match ausr.lage {
        AusrLage::Normal | AusrLage::Strecken => (0.0, 0.0),
        _ => inhalt_verteilen(ausr, frei, sichtbar, umgekehrt),
    };
    a.lagen(vor, zw)
}

/// Release 11.10.2026: sind die Spuren `[a, b)` fest (Unter- und Obergrenze
/// eine Laenge)? Dann haengt ein Bereich darin nicht von seinen Elementen ab.
fn zeilen_fest(achse: &Achse, a: usize, b: usize) -> bool {
    b > a
        && b <= achse.spuren.len()
        && achse.spuren[a..b].iter().all(|s| matches!(s.min, SpurGrenze::Laenge(_)) && matches!(s.max, SpurGrenze::Laenge(_)))
}

/// Anfang und Ende (logisch) eines Bereichs auf einer Achse.
fn bereich_spanne(a: &Achse, lagen: &[f32], s0: usize, s1: usize) -> (f32, f32) {
    let n = a.groesse.len();
    if n == 0 || s1 == 0 || s0 >= n {
        return (0.0, 0.0);
    }
    let s1 = s1.min(n);
    (lagen[s0], lagen[s1 - 1] + a.groesse[s1 - 1])
}

/// Die aufgeloeste Selbstausrichtung eines Elements auf einer Achse:
/// (Lage, sicher, strecken).
fn selbst_wahl(selbst: Ausr, elemente: Ausr, m: &Masse, waagerecht: bool) -> (AusrLage, Option<bool>, bool) {
    let a = selbst_aufloesen(selbst, elemente);
    let groesse_auto = if waagerecht { m.breite_auto } else { m.hoehe_auto && !m.text };
    let rand_auto = if waagerecht { m.rand_auto[0] || m.rand_auto[2] } else { m.rand_auto[1] || m.rand_auto[3] };
    match a.lage {
        AusrLage::Normal => {
            if m.ersetzt || !groesse_auto || rand_auto {
                (AusrLage::Anfang, None, false)
            } else {
                (AusrLage::Anfang, None, true)
            }
        }
        AusrLage::Strecken => (AusrLage::Anfang, None, groesse_auto && !rand_auto),
        AusrLage::Grundlinie | AusrLage::Auto => (AusrLage::Anfang, a.sicher, false),
        AusrLage::LetzteGrundlinie => (AusrLage::Ende, a.sicher, false),
        l => (l, a.sicher, false),
    }
}

/// Die erste (bzw. mit `letzte` die letzte) Grundlinie eines Kastens als
/// Abstand von der Oberkante seines Rahmenkastens (CSS Box Alignment 3
/// §9.1); `None` = keine (dann wird sie an der Unterkante gebildet).
pub(crate) fn grundlinie(ctx: &Ctx<'_>, sizes: &Sizes, id: NodeId, letzte: bool, tiefe: usize) -> Option<f32> {
    if tiefe > 32 {
        return None;
    }
    let node = ctx.document.get(id)?;
    let groesse = sizes.get(id)?;
    if node.kind == NodeKind::Text {
        if node.text.as_deref().is_none_or(|t| t.trim().is_empty()) {
            return None;
        }
        let (a, _) = ctx.measurer.zeilen_metrik(node.style.font_size, node.style.schriftgewicht, node.style.kursiv, &node.style.schriftfamilie);
        // Release 12.10.2026: auf ganze Pixel wie die Grundlinie eines
        // Textlaufs in der Zeile (`flow.rs`, `ganz_mit_plan`). GEMESSEN
        // vorher: WPT `alignment/self-baseline/grid-self-baseline-horiz-001`
        // (91,6 statt 91 px seit der ganzzahligen Inline-Grundlinie).
        let ganz = node.parent.is_some_and(|p| ctx.stil(p).is_some());
        return Some(if ganz { a.round() } else { a });
    }
    if node.ersetzt.is_some() {
        return None;
    }
    let fuell = resolve_spacing(node.style.padding, groesse.width, id, "padding").ok()?;
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    let oben = fuell.top + rahmen.top;
    // Release 11.10.2026: ein Flex-Container hat die Grundlinie seines
    // ersten (bzw. letzten) Elements an dessen Lage (CSS Flexbox 1 §8.5).
    let flex = ctx.flex.borrow().get(&id).map(|e| (e.kinder.clone(), e.kaesten.clone()));
    if let Some((kinder, kaesten)) = flex {
        let mut paare: Vec<(NodeId, Rect)> = kinder.iter().map(|(_, k)| *k).zip(kaesten).collect();
        if letzte {
            paare.reverse();
        }
        return paare.into_iter().find_map(|(k, r)| grundlinie(ctx, sizes, k, letzte, tiefe + 1).map(|b| oben + r.y + b));
    }
    // Eine Zeilenbox des Seitenbauers: die gemeinsame Grundlinie ihrer Laeufe
    // (dieselbe Rechnung wie der halbe Durchschuss in `flow.rs`).
    if node.style.direction == narsil_core::style::Direction::Row && crate::r#box::ist_zeilenbox(ctx, id, node) {
        let (mut a_max, mut d_max, mut fs_max) = (0.0f32, 0.0f32, 0.0f32);
        for &k in &node.children {
            if let Some(n) = ctx.document.get(k)
                && n.kind == NodeKind::Text
                && n.text.as_deref().is_some_and(|t| !t.trim().is_empty())
            {
                let (a, d) = ctx.measurer.zeilen_metrik(n.style.font_size, n.style.schriftgewicht, n.style.kursiv, &n.style.schriftfamilie);
                a_max = a_max.max(a);
                d_max = d_max.max(d);
                fs_max = fs_max.max(n.style.font_size);
            }
        }
        if a_max + d_max <= 0.0 {
            return None;
        }
        let innen = (groesse.height - oben - fuell.bottom - rahmen.bottom).max(0.0);
        let normal = (innen - crate::text::zeilenhoehe_normal(fs_max)).abs() < 0.5;
        let halb = if normal { 0.0 } else { (innen - (a_max + d_max)) / 2.0 };
        // Ganze Pixel wie im Textlauf (siehe oben).
        let a_ganz = if ctx.stil(id).is_some() || node.parent.is_some_and(|p| ctx.stil(p).is_some()) { a_max.round() } else { a_max };
        return Some(oben + halb.round() + a_ganz);
    }
    // Die Kinder im Fluss untereinander (Block) oder nebeneinander (Zeile).
    let reihe = node.style.direction == narsil_core::style::Direction::Row;
    let mut lagen: Vec<(f32, NodeId)> = Vec::new();
    let mut y = oben;
    for &k in &node.children {
        let Some(ks) = ctx.document.get(k) else { continue };
        if ctx.stil(k).is_some_and(|s| !s.wird_gezeichnet() || matches!(s.position, PositionArt::Absolut | PositionArt::Fest)) {
            continue;
        }
        let rand = resolve_spacing(ks.style.margin, groesse.width, k, "margin").unwrap_or(Edges::ZERO);
        let h = sizes.get(k).map_or(0.0, |g| g.height);
        if reihe {
            lagen.push((oben + rand.top, k));
        } else {
            lagen.push((y + rand.top, k));
            y += rand.top + h + rand.bottom;
        }
    }
    if letzte {
        lagen.iter().rev().find_map(|&(y, k)| grundlinie(ctx, sizes, k, true, tiefe + 1).map(|b| y + b))
    } else {
        lagen.iter().find_map(|&(y, k)| grundlinie(ctx, sizes, k, false, tiefe + 1).map(|b| y + b))
    }
}

/// Die Grundlinienwerte der Elemente eines Rasters fuer `align-self:
/// baseline` (CSS Grid 1 §10.6, Box Alignment 3 §9.3): je Eintrag die Art
/// (1 erste, 2 letzte Grundlinie), die Zeile, der Wert (bei der ersten vom
/// Zeilenanfang, bei der letzten vom Zeilenende) und der Index des Elements
/// (`None` fuer Elemente eines Subgrids — sie nehmen an der Ausrichtung des
/// Elterns teil, CSS Grid 2 §9).
fn grundlinien_werte(ctx: &Ctx<'_>, gs: &GitterStil, sizes: &Sizes, e: &Ergebnis, basis: f32, tiefe: usize) -> Vec<(u8, usize, f32, Option<usize>)> {
    let mut raus = Vec::new();
    if tiefe > 8 {
        return raus;
    }
    for (i, (b, (_, id))) in e.raster.bereiche.iter().zip(&e.kinder).enumerate() {
        if subgrid_achsen(ctx, *id).1 {
            // Die Elemente des Subgrids, in die Zeilen dieses Rasters versetzt.
            let Some(sub_gs) = ctx.stil(*id).and_then(|s| s.gitter.as_deref()) else { continue };
            let Some(sub_e) = ctx.gitter.borrow().get(id).cloned() else { continue };
            let n = b.z1.saturating_sub(b.z0);
            let (rand0, rand1) = rand_aussen(ctx, *id, basis, false);
            let g = e.raster.zeilen.luecke;
            let g2 = sub_e.raster.zeilen.luecke;
            for (a, k, v, _) in grundlinien_werte(ctx, sub_gs, sizes, &sub_e, sub_e.breite, tiefe + 1) {
                if k >= n {
                    continue;
                }
                let kante = if a == 1 { k == 0 } else { k + 1 == n };
                let schub = if kante { if a == 1 { rand0 } else { rand1 } } else { -(g - g2) / 2.0 };
                raus.push((a, b.z0 + k, v + schub, None));
            }
            continue;
        }
        let ks = kind_stil(ctx, *id);
        let lage = selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.align_self), gs.align_items).lage;
        let a = match lage {
            AusrLage::Grundlinie => 1u8,
            AusrLage::LetzteGrundlinie => 2,
            _ => continue,
        };
        let Ok(m) = masse(ctx, *id, basis, 0.0) else { continue };
        let h = sizes.get(*id).map_or(0.0, |g| g.height);
        let wert = if a == 1 {
            m.rand.top + grundlinie(ctx, sizes, *id, false, 0).unwrap_or(h)
        } else {
            m.rand.bottom + grundlinie(ctx, sizes, *id, true, 0).map_or(0.0, |lb| (h - lb).max(0.0))
        };
        let zeile = if a == 1 { b.z0 } else { b.z1.saturating_sub(1).max(b.z0) };
        raus.push((a, zeile, wert, Some(i)));
    }
    raus
}

/// Die gemeinsamen Grundlinien des Elterns fuer die Zeilen eines Subgrids
/// (in dessen Zeilen und von dessen Zeilenkanten gemessen), leer ohne
/// Subgrid-Zeilen.
fn eltern_grundlinien(ctx: &Ctx<'_>, sizes: &Sizes, node: &narsil_dom::Node, tiefe: usize) -> std::collections::HashMap<(u8, usize), f32> {
    let mut raus = std::collections::HashMap::new();
    if tiefe > 8 || !subgrid_achsen(ctx, node.id).1 {
        return raus;
    }
    let Some(eltern) = node.parent.and_then(|p| ctx.document.get(p)) else { return raus };
    let Some(egs) = ctx.stil(eltern.id).and_then(|s| s.gitter.as_deref()) else { return raus };
    let Some(e) = ctx.gitter.borrow().get(&eltern.id).cloned() else { return raus };
    let Some(i) = e.kinder.iter().position(|(_, id)| *id == node.id) else { return raus };
    let b = e.raster.bereiche[i];
    let mut gemeinsam = eltern_grundlinien(ctx, sizes, eltern, tiefe + 1);
    for (a, z, v, _) in grundlinien_werte(ctx, egs, sizes, &e, e.breite, 0) {
        let g = gemeinsam.entry((a, z)).or_insert(f32::MIN);
        *g = g.max(v);
    }
    let n = b.z1.saturating_sub(b.z0);
    let (rand0, rand1) = rand_aussen(ctx, node.id, e.breite, false);
    let g = e.raster.zeilen.luecke;
    let g2 = ctx.gitter.borrow().get(&node.id).map_or(g, |s| s.raster.zeilen.luecke);
    for ((a, z), v) in gemeinsam {
        if z < b.z0 || z >= b.z1 {
            continue;
        }
        let k = z - b.z0;
        let kante = if a == 1 { k == 0 } else { k + 1 == n };
        let schub = if kante { if a == 1 { rand0 } else { rand1 } } else { -(g - g2) / 2.0 };
        raus.insert((a, k), v - schub);
    }
    raus
}

/// Die Grundlinien-Ausrichtung auf der Spaltenachse (`align-self: baseline`,
/// CSS Grid 1 §10.6, Box Alignment 3 §9.3): je Element 0 (keine), 1 (erste)
/// oder 2 (letzte Grundlinie) und der Versatz zur gemeinsamen Grundlinie
/// seiner Zeile (bei der letzten: vom Ende).
///
/// Release 11.10.2026: mit den Elementen der Subgrids und — fuer ein
/// Subgrid — den Grundlinien des Elterns (CSS Grid 2 §9). GEMESSEN vorher:
/// WPT `css-grid/subgrid/subgrid-baseline-003` (das Element im Subgrid
/// blieb oben, die Referenz richtet es an der Grundlinie des Nachbarn aus).
fn grundlinien_versaetze(ctx: &Ctx<'_>, gs: &GitterStil, sizes: &Sizes, e: &Ergebnis, basis: f32, node: &narsil_dom::Node) -> Vec<(u8, f32)> {
    let n = e.kinder.len();
    let werte = grundlinien_werte(ctx, gs, sizes, e, basis, 0);
    let mut gemeinsam = eltern_grundlinien(ctx, sizes, node, 0);
    for &(a, z, v, _) in &werte {
        let g = gemeinsam.entry((a, z)).or_insert(f32::MIN);
        *g = g.max(v);
    }
    let mut raus = vec![(0u8, 0.0f32); n];
    for &(a, z, v, i) in &werte {
        if let Some(i) = i.filter(|i| *i < n) {
            raus[i] = (a, (gemeinsam[&(a, z)] - v).max(0.0));
        }
    }
    raus
}

/// Die verfuegbare Breite, mit der ein Element gemessen werden muss (seine
/// Bereichsbreite; ohne Dehnung die passende Breite).
fn zielbreiten(ctx: &Ctx<'_>, gs: &GitterStil, e: &Ergebnis, sizes: &Sizes) -> Result<Vec<(NodeId, f32, f32, f32, bool)>, LayoutError> {
    let lagen = spur_lagen(&e.raster.spalten, gs.justify_content, e.breite, gs.rtl);
    let mut raus = Vec::with_capacity(e.kinder.len());
    for (b, (_, id)) in e.raster.bereiche.iter().zip(&e.kinder) {
        let (x0, x1) = bereich_spanne(&e.raster.spalten, &lagen, b.s0, b.s1);
        let bereich = (x1 - x0).max(0.0);
        // Release 11.10.2026: Raender und Innenabstaende in Prozent gegen den
        // Rasterbereich (CSS Grid 1 §6.2). GEMESSEN vorher: WPT
        // `grid-items-percentage-paddings-001` (`padding-left: 50%` in einer
        // 100-px-Spalte: 26 statt 60 px breit).
        let m = masse(ctx, *id, bereich, e.hoehe.unwrap_or(0.0))?;
        if m.text {
            continue;
        }
        let ks = kind_stil(ctx, *id);
        let (_, _, strecken) = selbst_wahl(ks.map_or(Ausr::AUTO, |k| k.justify_self), gs.justify_items, &m, true);
        let strecken = strecken || subgrid_achsen(ctx, *id).0;
        let ziel = if !strecken && m.breite_auto && !m.ersetzt {
            let mr = m.rand.left + m.rand.right;
            let (mi, ma) = inhalt_breiten(ctx, sizes, *id, &m, 0);
            // Die Inhaltsbreiten rechnen Prozent-Innenabstaende gegen die
            // gemessene Breite: auf den Bereich umrechnen.
            let korr = ctx.document.get(*id).map_or(0.0, |n| {
                let pz = |l: Length| matches!(l, Length::Percent(_) | Length::Gemischt { .. });
                if !(pz(n.style.padding.left) || pz(n.style.padding.right)) {
                    return 0.0;
                }
                let g = sizes.get(*id).map_or(0.0, |s| s.width);
                let alt = resolve_spacing(n.style.padding, g, *id, "padding").unwrap_or(Edges::ZERO);
                let neu = resolve_spacing(n.style.padding, bereich, *id, "padding").unwrap_or(Edges::ZERO);
                (neu.left + neu.right) - (alt.left + alt.right)
            });
            let (mi, ma) = ((mi + korr).max(0.0), (ma + korr).max(0.0));
            ma.min((bereich - mr).max(mi)) + mr
        } else {
            bereich
        };
        raus.push((*id, ziel, bereich, m.rand.left + m.rand.right, strecken));
    }
    Ok(raus)
}

/// Die Messphase eines Rasters (`box.rs`, `SizeTask::Format`). Teil 1 legt
/// die Spalten fest und verlangt die Nachmessung der Elemente mit ihrer
/// Bereichsbreite; Teil 2 (`nachmessung`) rechnet die Zeilen aus den
/// gemessenen Hoehen und legt das Ergebnis fuer die Platzierung ab.
#[allow(clippy::too_many_arguments)]
pub(crate) fn messen(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    gs: &GitterStil,
    sizes: &Sizes,
    breite: f32,
    hoehe: Option<f32>,
    min_h: Option<f32>,
    max_h: Option<f32>,
    nachmessung: bool,
) -> Result<Messung, LayoutError> {
    if !nachmessung {
        let e = rechnen(ctx, node, gs, sizes, Platz::Definit(breite), hoehe, min_h, max_h, false, 0)?;
        let mut nach = Vec::new();
        for (id, w, bereich, rand_b, gestreckt) in zielbreiten(ctx, gs, &e, sizes)? {
            let alt = sizes.verfuegbar_von(id);
            // Ein Subgrid wird immer nachgemessen: erst jetzt kennt es die
            // Spalten seines Elterns (CSS Grid 2 §9).
            let sub = subgrid_achsen(ctx, id);
            // Release 11.10.2026: ein schmaleres Element mit Prozent-Rand
            // oder -Innenabstand wird gegen den BEREICH gemessen (CSS Grid 1
            // §6.2), seine Breite gesetzt (`Ctx::flex_mass`, in `enter`).
            // GEMESSEN vorher: WPT `grid-items-percentage-paddings-001`.
            let prozentig = w < bereich - 0.5
                && ctx.document.get(id).is_some_and(|n| {
                    let p = |l: Length| matches!(l, Length::Percent(_) | Length::Gemischt { .. });
                    p(n.style.padding.left) || p(n.style.padding.right) || p(n.style.padding.top) || p(n.style.padding.bottom)
                        || p(n.style.margin.left) || p(n.style.margin.right) || p(n.style.margin.top) || p(n.style.margin.bottom)
                });
            // Release 11.10.2026: eine gedehnte Tabelle fuellt ihren Bereich
            // (`stretch`, CSS Box Alignment 3 §6.1; ohne Setzen bliebe sie
            // bei ihrer passenden Breite). GEMESSEN vorher: WPT
            // `table-grid-item-005` (Zellen 162 statt 400 px breit).
            let tabelle = crate::tabellen_leg::ist_tabelle(ctx, id) && (w - bereich).abs() < 0.5;
            // Ebenso ein gedehntes Element mit `aspect-ratio` (die Hoehe folgt
            // der gesetzten Breite, CSS Sizing 4 §5). GEMESSEN vorher: WPT
            // css-sizing `aspect-ratio/grid-aspect-ratio-014` (0 statt 100 px hoch).
            // Release 11.10.2026: auch ein gedehntes ersetztes Element mit
            // Verhaeltnis (`justify-self: stretch`). GEMESSEN vorher: WPT
            // `alignment/replaced-alignment-with-aspect-ratio-008` (100 x 50).
            let verhaeltnis = ctx.document.get(id).is_some_and(|n| {
                n.style.size.width == Length::Auto
                    && if n.ersetzt.is_none() {
                        // Release 14.10.2026: with a definite height the width
                        // follows the ratio (`normal` behaves as `start` for a
                        // box with a preferred aspect ratio, CSS Box Alignment
                        // 3 §6.1); only an explicit `stretch` fills. MEASURED
                        // before: WPT css-sizing `aspect-ratio/grid-aspect-ratio-
                        // 010` (`height: 100px; aspect-ratio: 1/1` in a 200 px
                        // column was 200 px wide).
                        ctx.stil(id).is_some_and(|s| s.seitenverhaeltnis.is_finite() && s.seitenverhaeltnis != 0.0)
                            && (gestreckt || !matches!(n.style.size.height, Length::Px(_)))
                    } else {
                        // Nur eine Achse gedehnt: mit beiden gilt das
                        // Verhaeltnis nicht (WPT grid-lanes `column-item-minmax-img-002`).
                        gestreckt
                            && n.style.size.height == Length::Auto
                            && crate::r#box::wirksames_verhaeltnis(ctx, id, n).is_some()
                            && !matches!(selbst_aufloesen(kind_stil(ctx, id).map_or(Ausr::AUTO, |k| k.align_self), gs.align_items).lage, AusrLage::Strecken)
                    }
            }) && (w - bereich).abs() < 0.5;
            let setzen = prozentig || tabelle || verhaeltnis;
            let vorher = if setzen {
                ctx.flex_mass.borrow_mut().insert(id, (Some((w - rand_b).max(0.0)), None))
            } else {
                ctx.flex_mass.borrow_mut().remove(&id)
            };
            let verfuegbar = if prozentig { bereich } else { w };
            let anders_mass = vorher != if setzen { Some((Some((w - rand_b).max(0.0)), None)) } else { None };
            if (alt.width - verfuegbar).abs() > 0.5 || sub.0 || sub.1 || anders_mass {
                nach.push((id, Size::new(verfuegbar.max(0.0), alt.height)));
            }
        }
        // Die Spalten fuer die Subgrids ablegen (die Zeilen folgen in Teil 2).
        ctx.gitter.borrow_mut().insert(node.id, e);
        if !nach.is_empty() {
            return Ok(Messung::Nachmessen(nach));
        }
    }
    let e = rechnen(ctx, node, gs, sizes, Platz::Definit(breite), hoehe, min_h, max_h, true, 0)?;
    let h = e.raster.zeilen.gesamt();
    let mut ih = hoehe.unwrap_or(h);
    if let Some(m) = max_h {
        ih = ih.min(m);
    }
    if let Some(m) = min_h {
        ih = ih.max(m);
    }
    let mut nach = Vec::new();
    // Release 11.10.2026: eine Tabelle mit `align-self: normal/stretch`
    // bekommt die Hoehe ihres Bereichs gesetzt (`Ctx::flex_mass`) und wird
    // damit nachgemessen — ihre Zeilen verteilen den Ueberschuss (CSS 2.1
    // §17.5.3), sonst bliebe das Raster der Tabelle bei seiner Inhaltshoehe.
    // GEMESSEN vorher: WPT `table-grid-item-005` (19 statt 400 px hoch).
    {
        let ys = spur_lagen(&e.raster.zeilen, gs.align_content, ih, false);
        let xs = spur_lagen(&e.raster.spalten, gs.justify_content, e.breite, gs.rtl);
        for (b, &(_, id)) in e.raster.bereiche.iter().zip(&e.kinder) {
            // Ebenso ein Element mit `height: P%`-Kindern (`hoehe_bedingt`):
            // gedehnt ist seine Hoehe definit. GEMESSEN vorher: WPT
            // `grid-lanes/alignment/column-align-items-005` (Rasterteil).
            let bedingt = ctx.document.get(id).is_some_and(|n| {
                n.children.iter().take(crate::plan::MAX_KINDER).any(|c| ctx.stil(*c).is_some_and(|s| s.hoehe_bedingt.is_some()))
            });
            // Release 11.10.2026: ein Element mit eigener `height: P%` (die
            // Kaskade hat sie als `hoehe_bedingt` aufgehoben, weil das Raster
            // keine definite Hoehe hat) gilt gegen seinen Bereich (CSS Grid 1
            // §6.1). GEMESSEN vorher: WPT css-sizing
            // `aspect-ratio/grid-aspect-ratio-036/-037` (`height: 100%` fiel weg).
            // Nur bei festen Zeilen: haengt der Bereich vom Element ab
            // (`auto`-Zeilen), waere die Prozenthoehe zyklisch (WPT css-sizing
            // `aspect-ratio/grid-aspect-ratio-040` lief sonst endlos).
            let eigen = ctx
                .stil(id)
                .and_then(|s| s.hoehe_bedingt)
                .filter(|p| p.is_finite() && *p >= 0.0 && zeilen_fest(&e.raster.zeilen, b.z0, b.z1));
            // Release 11.10.2026: ein ersetztes Element mit Verhaeltnis und
            // ausdruecklichem `align-self: stretch` bekommt die Bereichshoehe,
            // die Breite folgt dem Verhaeltnis. GEMESSEN vorher: WPT
            // `alignment/replaced-alignment-with-aspect-ratio-009` (50 x 100).
            let bild = ctx.document.get(id).is_some_and(|n| {
                n.ersetzt.is_some()
                    && n.style.size.height == Length::Auto
                    && crate::r#box::wirksames_verhaeltnis(ctx, id, n).is_some()
                    && matches!(selbst_aufloesen(kind_stil(ctx, id).map_or(Ausr::AUTO, |k| k.align_self), gs.align_items).lage, AusrLage::Strecken)
            });
            if !crate::tabellen_leg::ist_tabelle(ctx, id) && !bedingt && eigen.is_none() && !bild {
                continue;
            }
            let (x0, x1) = bereich_spanne(&e.raster.spalten, &xs, b.s0, b.s1);
            let (y0, y1) = bereich_spanne(&e.raster.zeilen, &ys, b.z0, b.z1);
            let m = masse(ctx, id, (x1 - x0).max(0.0), (y1 - y0).max(0.0))?;
            let ks = kind_stil(ctx, id);
            let (_, _, strecken) = selbst_wahl(ks.map_or(Ausr::AUTO, |k| k.align_self), gs.align_items, &m, false);
            let alt = ctx.flex_mass.borrow().get(&id).copied();
            // Eine Bereichshoehe jenseits jeder Seite ist ein Zyklus (Prozente,
            // die mit den Zeilen wachsen): dann nicht setzen.
            let soll = match eigen {
                Some(p) => {
                    let innen = ((y1 - y0).max(0.0) * p / 100.0).max(0.0);
                    let rahmen = if m.border_box { innen.max(m.fuell.1) } else { innen + m.fuell.1 };
                    Some(klemmen(rahmen, m.min_h, m.max_h).max(0.0))
                }
                None => strecken.then(|| klemmen((y1 - y0) - m.rand.top - m.rand.bottom, m.min_h, m.max_h).max(0.0)),
            }
            .filter(|h| h.is_finite() && *h < 1.0e6);
            if soll.is_some() {
                ctx.hoehe_definit.borrow_mut().insert(id);
            }
            let neu = (alt.and_then(|a| a.0), soll);
            if alt.unwrap_or((None, None)) != neu {
                if neu == (None, None) {
                    ctx.flex_mass.borrow_mut().remove(&id);
                } else {
                    ctx.flex_mass.borrow_mut().insert(id, neu);
                }
                let v = sizes.verfuegbar_von(id);
                nach.push((id, Size::new(v.width, soll.unwrap_or(v.height))));
            }
        }
    }
    // Absolut positionierte Kinder eines positionierten Rasters werden mit
    // ihrem Rasterbereich gemessen (CSS Grid 1 §9.1: er ist ihr enthaltender
    // Block). GEMESSEN vorher: `left:0;right:0` dehnte auf das ganze Raster.
    let positioniert = ctx.stil(node.id).is_some_and(|s| s.position != PositionArt::Statisch);
    let (_, mut abspos) = rasterelemente(ctx, node);
    if positioniert {
        abspos.extend(abspos_nachfahren(ctx, node));
    }
    if positioniert && !abspos.is_empty() {
        let inhalt = Rect::new(0.0, 0.0, breite, ih);
        for (id, r) in abspos_rechtecke(ctx, node, gs, &e, &inhalt, &abspos)? {
            ctx.gitter_abs.borrow_mut().insert(id, Size::new(r.width, r.height));
            let alt = sizes.verfuegbar_von(id);
            if (alt.width - r.width).abs() > 0.5 || (alt.height - r.height).abs() > 0.5 {
                nach.push((id, Size::new(r.width, r.height)));
            }
        }
    }
    ctx.gitter.borrow_mut().insert(node.id, e);
    if !nach.is_empty() {
        return Ok(Messung::Nachmessen(nach));
    }
    Ok(Messung::Fertig(h))
}

/// Die intrinsische Breite (min-/max-content, Rahmenkasten) eines Rasters
/// mit `width: auto` — fuer `inline-grid`, Floats, absolute Kaesten und
/// Flex-Elemente (CSS Grid 1 §12.1 unter einer min-/max-content-Bedingung).
pub(crate) fn intrinsisch(sizes: &Sizes, ctx: &Ctx<'_>, id: NodeId, max: bool, tiefe: usize) -> Option<f32> {
    if ctx.document.get(id).is_some_and(|n| matches!(n.style.size.width, Length::Px(_))) {
        return None;
    }
    intrinsisch_inhalt(sizes, ctx, id, max, tiefe)
}

/// Release 11.10.2026: wie [`intrinsisch`], aber auch bei fester eigener
/// Breite (fuer `min-width: min-content` usw., CSS Sizing 3 §5.1).
pub(crate) fn intrinsisch_inhalt(sizes: &Sizes, ctx: &Ctx<'_>, id: NodeId, max: bool, tiefe: usize) -> Option<f32> {
    let st = ctx.stil(id)?;
    let gs = st.gitter.as_deref()?;
    let node = ctx.document.get(id)?;
    if tiefe > 48 {
        return None;
    }
    let platz = if max { Platz::MaxInhalt } else { Platz::MinInhalt };
    // Prozent-Innenabstaende wie in `Sizes::max_content_breite` gegen die
    // gemessene Breite (GEMESSEN: WPT `css-flexbox/grid-flex-item-006`).
    let gemessen = sizes.get(id).map_or(0.0, |s| s.width);
    let fuell = resolve_spacing(node.style.padding, gemessen, id, "padding").unwrap_or(Edges::ZERO);
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    let rand = fuell.left + fuell.right + rahmen.left + rahmen.right;
    match st.lanes {
        0 => {
            let e = rechnen(ctx, node, gs, sizes, platz, None, None, None, false, tiefe + 1).ok()?;
            Some(e.raster.spalten.gesamt() + rand)
        }
        // Masonry mit Spalten-Spuren: die Summe der Spuren.
        1 => {
            let l = lanes_spuren(ctx, node, gs, sizes, false, platz, 0.0, tiefe + 1).ok()?;
            let ende = l.spuren.iter().map(|(a, g)| a + g).fold(0.0f32, f32::max);
            Some(ende + rand)
        }
        _ => None,
    }
}

/// Masonry (`display: grid-lanes`, CSS Grid 3): die Spuren der Rasterachse
/// und je Element seine Zelle und seine ausgerichtete Lage auf dieser Achse.
/// Alle Angaben sind Lagen ENTLANG der Rasterachse (bei Zeilen-Spuren also
/// senkrecht), absolut.
#[derive(Clone)]
pub(crate) struct Lanes {
    /// (Anfang, Groesse) je Spur.
    pub spuren: Vec<(f32, f32)>,
    /// Je Element: (Zellanfang, Zellgroesse, Randkasten-Anfang ausgerichtet,
    /// Groesse ausgerichtet, feste Lage?, Spannweite).
    pub elemente: Vec<(f32, f32, f32, f32, bool, usize)>,
    /// Die Rasterachse (fuer Subgrids in den Spuren).
    pub achse: Achse,
    /// Je Element die Ausrichtung auf der Masonry-Achse: 0 `normal`,
    /// 1 `stretch`, 2 Anfang, 3 Mitte, 4 Ende.
    pub stapel: Vec<u8>,
    /// Die Elemente (Index im DOM, Knoten).
    pub kinder: Vec<(usize, NodeId)>,
    /// Je Element die Grundlinien-Ausrichtung auf der Rasterachse (nur
    /// Zeilen-Spuren): Art (0 keine, 1 erste, 2 letzte), Wert (vom
    /// Randkasten-Anfang bzw. -Ende) und die Summe der Raender.
    pub grund: Vec<(u8, f32, f32)>,
}

/// Masonry: legt das Ergebnis fuer Subgrids ab, sobald die Spuren der
/// Elemente feststehen (`starts` = Anfangslage je Element entlang der
/// Rasterachse, absolut wie `Lanes::spuren`).
pub(crate) fn lanes_ablegen(ctx: &Ctx<'_>, node: &narsil_dom::Node, l: &Lanes, zeilen: bool, starts: &[f32], breite: f32, hoehe: Option<f32>) {
    let n = l.spuren.len();
    if n == 0 {
        return;
    }
    let mut bereiche = Vec::with_capacity(l.kinder.len());
    for (i, e) in l.elemente.iter().enumerate() {
        let x = starts.get(i).copied().unwrap_or(e.0);
        let mut beste = 0usize;
        let mut abstand = f32::INFINITY;
        for (j, (a, _)) in l.spuren.iter().enumerate() {
            let d = (a - x).abs();
            if d < abstand - 0.01 {
                abstand = d;
                beste = j;
            }
        }
        let a = beste.min(n - 1);
        let b = (a + e.5.max(1)).min(n);
        bereiche.push(if zeilen { Bereich { z0: a, z1: b, s0: 0, s1: 1 } } else { Bereich { z0: 0, z1: 1, s0: a, s1: b } });
    }
    // Die Masonry-Achse hat keine Spuren: Linien darauf sind `auto`.
    let leer = Achse::default();
    let raster = if zeilen {
        Raster { spalten: leer, zeilen: l.achse.clone(), bereiche }
    } else {
        Raster { spalten: l.achse.clone(), zeilen: leer, bereiche }
    };
    ctx.gitter.borrow_mut().insert(
        node.id,
        Ergebnis {
            breite,
            hoehe,
            raster,
            kinder: l.kinder.clone(),
            vom_eltern: false,
            zeilen_fertig: true,
            auto_platziert: l.elemente.iter().map(|e| !e.4).collect(),
        },
    );
}

/// Die Spuren der Rasterachse eines Masonry-Containers (CSS Grid 3 §„Grid
/// Lanes": wie im Raster, aber ein automatisch platziertes Element traegt zu
/// JEDER Lage bei, in die es kommen koennte). `zeilen` = Zeilen-Spuren.
/// `platz` = definiter Platz der Rasterachse (Inhaltsbox) oder eine
/// Bedingung; `anfang` = die Lage der Inhaltsbox auf dieser Achse.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lanes_spuren(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    gs: &GitterStil,
    sizes: &Sizes,
    zeilen: bool,
    platz: Platz,
    anfang: f32,
    tiefe: usize,
) -> Result<Lanes, LayoutError> {
    lanes_spuren_mit(ctx, node, gs, sizes, zeilen, platz, anfang, tiefe, None)
}

/// Release 11.10.2026: die Groesse der Spuren einer intrinsischen
/// Auto-Wiederholung, wie sie mit EINER Wiederholung ausfaellt (CSS Grid 3,
/// vorlaeufig: die Spuren werden mit den Beitraegen aller Elemente in allen
/// Lagen gemessen — ein Element ueber eine feste und eine wiederholte Spur
/// traegt zur wiederholten nur den Rest bei). GEMESSEN vorher: WPT
/// grid-lanes `column-auto-repeat-auto-029/030/031` (die Schaetzung aus den
/// geteilten Beitraegen ergab drei statt zwei Wiederholungen).
fn wiederhol_probe(liste: &SpurListe, l: &Lanes) -> Option<(f32, f32)> {
    let mut start = 0usize;
    let mut gruppe: Vec<Spur> = Vec::new();
    for t in &liste.teile {
        match t {
            SpurTeil::Spur(_) => start += 1,
            SpurTeil::Auto { teile, .. } => {
                gruppe = teile.iter().filter_map(|t| if let SpurTeil::Spur(s) = t { Some(*s) } else { None }).collect();
                break;
            }
            SpurTeil::Namen(_) => {}
        }
    }
    if gruppe.is_empty() {
        return None;
    }
    let v = l.achse.versatz.max(0) as usize;
    let g = l.achse.groesse.get(v + start..v + start + gruppe.len())?;
    // Getrennt nach min-content- und uebrigen Spuren (`wiederhol_mass`
    // nimmt fuer `min-content` den ersten, sonst den zweiten Wert).
    // GEMESSEN: WPT `column-auto-repeat-mixed-intrinsic-002`.
    let alle = g.iter().copied().fold(0.0f32, f32::max);
    let (mut mi, mut ma) = (None::<f32>, None::<f32>);
    for (sp, &w) in gruppe.iter().zip(g) {
        if matches!(sp.max, SpurGrenze::MinInhalt) {
            mi = Some(mi.map_or(w, |x| x.max(w)));
        } else {
            ma = Some(ma.map_or(w, |x| x.max(w)));
        }
    }
    Some((mi.unwrap_or(alle), ma.unwrap_or(alle)))
}

/// Wie [`lanes_spuren`]; `schaetzung_vorgabe`: `Some(x)` = diese Schaetzung
/// fuer die Auto-Wiederholung (`Some(None)`: eine Wiederholung, ohne
/// Dehnen — die Probe).
#[allow(clippy::too_many_arguments)]
fn lanes_spuren_mit(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    gs: &GitterStil,
    sizes: &Sizes,
    zeilen: bool,
    platz: Platz,
    anfang: f32,
    tiefe: usize,
    schaetzung_vorgabe: Option<Option<(f32, f32)>>,
) -> Result<Lanes, LayoutError> {
    let probe = matches!(schaetzung_vorgabe, Some(None));
    let (kinder, _) = rasterelemente(ctx, node);
    let def = platz.definit();
    let (liste, auto, luecke_l, ausr_inhalt, ausr_elemente) = if zeilen {
        (&gs.zeilen, &gs.auto_zeilen, gs.luecke_zeile, gs.align_content, gs.align_items)
    } else {
        (&gs.spalten, &gs.auto_spalten, gs.luecke_spalte, gs.justify_content, gs.justify_items)
    };
    let luecke = luecke_px(luecke_l, def, 0.0);
    // Intrinsische Spuren in der Auto-Wiederholung: erst die Probe mit einer
    // Wiederholung (siehe `wiederhol_probe`).
    let intrinsisch_wdh = liste.teile.iter().any(|t| match t {
        SpurTeil::Auto { teile, .. } => teile.iter().any(|t| matches!(t, SpurTeil::Spur(s) if !matches!(s.min, SpurGrenze::Laenge(_)) && !matches!(s.max, SpurGrenze::Laenge(_)))),
        _ => false,
    });
    let probe_schaetzung = match schaetzung_vorgabe {
        Some(v) => Some(v),
        None if intrinsisch_wdh && tiefe < 16 => {
            let pl = lanes_spuren_mit(ctx, node, gs, sizes, zeilen, platz, anfang, tiefe + 1, Some(None))?;
            Some(wiederhol_probe(liste, &pl))
        }
        None => None,
    };
    let schaetzung = if let Some(v) = probe_schaetzung {
        v
    } else if liste.hat_auto_wiederholung() {
        let basis0 = if zeilen { content_breite(sizes, node) } else { def.unwrap_or(0.0) };
        let mut sch = (0.0f32, 0.0f32);
        for (_, id) in &kinder {
            if let Ok(m) = masse(ctx, *id, basis0, def.unwrap_or(0.0)) {
                let b = if zeilen { zeilen_beitrag(sizes, *id, &m) } else { spalten_beitrag(ctx, sizes, *id, &m, tiefe) };
                let (ks, kz) = spannen_schaetzung(ctx, *id);
                let k = if zeilen { kz } else { ks };
                sch = (sch.0.max(b.min / k), sch.1.max(b.max / k));
            }
        }
        Some(sch)
    } else {
        None
    };
    // Release 11.10.2026: ohne definite Groesse zaehlt fuer `auto-fill` die
    // Hoechst-, sonst die Mindestgroesse (CSS Grid 1 §7.2.3.2). GEMESSEN
    // vorher: WPT grid-lanes `row-auto-repeat-007/-008` (`min-height:
    // 200px`, `repeat(auto-fill, 50px)`: eine Zeile statt vier).
    let (min_p, max_p) = if def.is_none() && liste.hat_auto_wiederholung() { lanes_grenzen(ctx, sizes, node, zeilen) } else { (None, None) };
    let ex = explizit_bauen(liste, def.or(max_p), min_p, luecke, def, schaetzung);
    let info = AchsInfo { explizit: ex.spuren.len() as i32, namen: &ex.namen };
    let leer = GitterPlatz::default();
    let lagen: Vec<Lage> = kinder
        .iter()
        .map(|(_, id)| {
            let p = kind_stil(ctx, *id).map_or(&leer, |k| &k.platz);
            if zeilen { info.lage(&p.zeile_start, &p.zeile_ende) } else { info.lage(&p.spalte_start, &p.spalte_ende) }
        })
        .collect();
    let mut lo = 0i32;
    let mut hi = ex.spuren.len() as i32;
    let mut groesste = 1i32;
    for l in &lagen {
        match *l {
            Lage::Fest(a, b) => {
                lo = lo.min(a);
                hi = hi.max(b);
            }
            Lage::Auto(s) => groesste = groesste.max(s),
        }
    }
    if hi - lo < groesste {
        hi = lo + groesste;
    }
    // Belegte `auto-fit`-Spuren: die festen und so viele vorn, wie die
    // automatisch platzierten Elemente brauchen.
    let n = (hi - lo) as usize;
    let mut belegt = vec![false; n];
    let mut auto_bedarf = 0usize;
    for l in &lagen {
        match *l {
            Lage::Fest(a, b) => {
                for x in belegt.iter_mut().take((b - lo) as usize).skip((a - lo) as usize) {
                    *x = true;
                }
            }
            Lage::Auto(s) => auto_bedarf += s as usize,
        }
    }
    let mut frei_gezaehlt = 0usize;
    for x in belegt.iter_mut() {
        if frei_gezaehlt >= auto_bedarf {
            break;
        }
        if !*x {
            *x = true;
            frei_gezaehlt += 1;
        }
    }
    let mut achse = achse_bauen(&ex, auto, lo, hi, luecke, &[], true);
    for (i, k) in achse.kollabiert.iter_mut().enumerate() {
        let l = i as i32 + lo;
        let fit = l >= 0 && (l as usize) < ex.fit.len() && ex.fit[l as usize];
        *k = fit && !belegt[i];
    }
    // Beitraege: jedes automatisch platzierte Element in jeder Lage.
    let basis = def.unwrap_or(0.0);
    let breite_basis = if zeilen { content_breite(sizes, node) } else { basis };
    let mut masse_alle = Vec::with_capacity(kinder.len());
    for (_, id) in &kinder {
        masse_alle.push(masse(ctx, *id, breite_basis, basis)?);
    }
    // Release 11.10.2026: Grundlinien-Ausrichtung auf der Rasterachse von
    // Zeilen-Spuren (`align-self: baseline`, CSS Grid 3 „Grid Lanes" wie
    // CSS Grid 1 §10.6): fest platzierte Elemente mit derselben Anfangs-
    // (bzw. End-)Zeile teilen eine Grundlinie, der Versatz zaehlt zum
    // Beitrag (§12.5 Schritt 1). GEMESSEN vorher: WPT grid-lanes
    // `grid-lanes-baseline-alignment-affects-intrinsic-size-004/008` (die
    // Referenzen in `display: grid` richten aus, Masonry nicht).
    // Automatisch platzierte Elemente bekommen ihre Spur erst beim Legen;
    // ihre Ausrichtung folgt danach (`lanes_grundlinien`).
    let mut grund: Vec<(u8, f32)> = vec![(0, 0.0); kinder.len()];
    let mut grund_werte: Vec<(u8, f32, f32)> = vec![(0, 0.0, 0.0); kinder.len()];
    if zeilen {
        let mut werte: Vec<(u8, usize, f32)> = vec![(0, 0, 0.0); kinder.len()];
        let mut gemeinsam: std::collections::HashMap<(u8, usize), f32> = std::collections::HashMap::new();
        for (i, ((l, (_, id)), m)) in lagen.iter().zip(&kinder).zip(&masse_alle).enumerate() {
            if subgrid_achsen(ctx, *id).1 {
                continue;
            }
            let ks = kind_stil(ctx, *id);
            let art = match selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.align_self), ausr_elemente).lage {
                AusrLage::Grundlinie => 1u8,
                AusrLage::LetzteGrundlinie => 2,
                _ => continue,
            };
            let h = sizes.get(*id).map_or(0.0, |g| g.height);
            let wert = if art == 1 {
                m.rand.top + grundlinie(ctx, sizes, *id, false, 0).unwrap_or(h)
            } else {
                m.rand.bottom + grundlinie(ctx, sizes, *id, true, 0).map_or(0.0, |lb| (h - lb).max(0.0))
            };
            grund_werte[i] = (art, wert, m.rand.top + m.rand.bottom);
            let Lage::Fest(a0, b0) = *l else { continue };
            let zeile = if art == 1 { (a0 - lo) as usize } else { (b0 - lo - 1).max(a0 - lo) as usize };
            let g = gemeinsam.entry((art, zeile)).or_insert(f32::MIN);
            *g = g.max(wert);
            werte[i] = (art, zeile, wert);
        }
        for (i, &(art, zeile, wert)) in werte.iter().enumerate() {
            if art != 0 {
                grund[i] = (art, (gemeinsam[&(art, zeile)] - wert).max(0.0));
            }
        }
    }
    let mut el: Vec<AchsElement> = Vec::new();
    for (ki, ((l, (_, id)), m)) in lagen.iter().zip(&kinder).zip(&masse_alle).enumerate() {
        // Ein Subgrid in den Spuren: seine Elemente (CSS Grid 3 §„Subgrids").
        let sub = subgrid_achsen(ctx, *id);
        let ist_sub = if zeilen { sub.1 } else { sub.0 };
        let mut beitrag = if zeilen { zeilen_beitrag(sizes, *id, m) } else { spalten_beitrag(ctx, sizes, *id, m, tiefe) };
        if grund[ki].0 != 0 {
            beitrag.min += grund[ki].1;
            beitrag.max += grund[ki].1;
            beitrag.minimum += grund[ki].1;
        }
        let lege = |a: usize, b: usize, mit_namen: bool, el: &mut Vec<AchsElement>| {
            if ist_sub {
                let namen: Vec<Vec<String>> = (a..=b)
                    .map(|k| {
                        let li = k as i32 - achse.versatz;
                        if li < 0 || !mit_namen { Vec::new() } else { achse.namen.get(li as usize).cloned().unwrap_or_default() }
                    })
                    .collect();
                subgrid_elemente(ctx, sizes, *id, !zeilen, a, b, luecke, &namen, breite_basis, 0, el);
            } else {
                el.push(AchsElement { a, b, beitrag });
            }
        };
        match *l {
            Lage::Fest(a, b) => lege((a - lo) as usize, (b - lo) as usize, true, &mut el),
            Lage::Auto(s) => {
                let s = s as usize;
                for a in 0..=n.saturating_sub(s) {
                    if el.len() > 200_000 {
                        break;
                    }
                    if (a..a + s).any(|t| achse.kollabiert[t]) {
                        continue;
                    }
                    lege(a, a + s, false, &mut el);
                }
            }
        }
    }
    let normal = |a: Ausr| matches!(a.lage, AusrLage::Normal | AusrLage::Strecken) && !probe;
    // Release 11.10.2026: `flow-start`/`flow-end` (siehe narsil-css
    // `ausrichtung`) folgen der Spurrichtung: mit `track-reverse` ist der
    // Fluss-Anfang das Ende. GEMESSEN vorher: WPT
    // `grid-lanes/alignment/row-track-reverse-align-content-001`.
    let ausr_inhalt = if ausr_inhalt.flex_bezug
        && ausr_inhalt.selbst_bezug
        && ctx.stil(node.id).is_some_and(|s| s.lanes_umkehr & 2 != 0)
    {
        let lage = match ausr_inhalt.lage {
            AusrLage::Anfang => AusrLage::Ende,
            AusrLage::Ende => AusrLage::Anfang,
            l => l,
        };
        Ausr { lage, ..ausr_inhalt }
    } else {
        ausr_inhalt
    };
    spuren_groessen(
        &mut achse,
        &AchsRechnung { platz, prozent_basis: def, min_platz: None, max_platz: None, strecken: normal(ausr_inhalt), elemente: &el },
    );
    let pos = match def {
        Some(d) => spur_lagen(&achse, ausr_inhalt, d, false),
        None => achse.lagen(0.0, 0.0),
    };
    let spuren: Vec<(f32, f32)> = (0..n).map(|i| (anfang + pos[i], achse.groesse[i])).collect();
    let mut elemente = Vec::with_capacity(kinder.len());
    for (ki, ((l, (_, id)), m)) in lagen.iter().zip(&kinder).zip(&masse_alle).enumerate() {
        let (a, b, fest) = match *l {
            Lage::Fest(a, b) => ((a - lo) as usize, (b - lo) as usize, true),
            Lage::Auto(s) => (0, (s as usize).min(n), false),
        };
        let (z0, z1) = bereich_spanne(&achse, &pos, a, b.max(a + 1));
        let zelle = (z1 - z0).max(0.0);
        let ks = kind_stil(ctx, *id);
        let gemessen = sizes.get(*id).unwrap_or(Size::new(0.0, 0.0));
        let selbst = ks.map_or(Ausr::AUTO, |k| if zeilen { k.align_self } else { k.justify_self });
        let (lage, sicher, streck) = selbst_wahl(selbst, ausr_elemente, m, !zeilen);
        let (r0, r1) = if zeilen { (m.rand.top, m.rand.bottom) } else { (m.rand.left, m.rand.right) };
        let (min_g, max_g) = if zeilen { (m.min_h, m.max_h) } else { (m.min_b, m.max_b) };
        let g = if streck {
            klemmen(zelle - r0 - r1, min_g, max_g)
        } else if zeilen {
            gemessen.height
        } else {
            // Eine Prozentbreite bezieht sich auf die Spur (den Bereich).
            m.prozent_breite(zelle).unwrap_or(gemessen.width)
        };
        let lage = match lage {
            AusrLage::Links => AusrLage::Anfang,
            AusrLage::Rechts => AusrLage::Ende,
            l => l,
        };
        let off = match grund[ki] {
            (1, v) => v,
            (2, v) => zelle - (g + r0 + r1) - v,
            _ => selbst_lage(lage, sicher, zelle, g + r0 + r1),
        };
        // Ein Subgrid fuellt seine Spuren.
        let sub = subgrid_achsen(ctx, *id);
        if (zeilen && sub.1) || (!zeilen && sub.0) {
            let g = (zelle - r0 - r1).max(0.0);
            elemente.push((anfang + z0, zelle, anfang + z0, g, fest, b - a));
            continue;
        }
        elemente.push((anfang + z0, zelle, anfang + z0 + off, g, fest, b - a));
    }
    // Die Ausrichtung auf der Masonry-Achse (CSS Grid 3: das letzte Element
    // jeder Spur richtet sich im Rest bis zum Ende der laengsten Spur aus).
    let stapel: Vec<u8> = kinder
        .iter()
        .map(|(_, id)| {
            let ks = kind_stil(ctx, *id);
            let (selbst, el) = if zeilen {
                (ks.map_or(Ausr::AUTO, |k| k.justify_self), gs.justify_items)
            } else {
                (ks.map_or(Ausr::AUTO, |k| k.align_self), gs.align_items)
            };
            match selbst_aufloesen(selbst, el).lage {
                AusrLage::Normal => 0,
                AusrLage::Strecken => 1,
                AusrLage::Mitte => 3,
                AusrLage::Ende | AusrLage::LetzteGrundlinie => 4,
                AusrLage::Rechts if zeilen => 4,
                _ => 2,
            }
        })
        .collect();
    Ok(Lanes { spuren, elemente, achse, stapel, kinder, grund: grund_werte })
}

/// Masonry (Zeilen-Spuren): die Grundlinien-Ausrichtung nach dem Legen,
/// wenn die Spur jedes Elements feststeht. `gelegt` ist die gedrehte
/// Platzierung (x entlang der Rasterachse, Randkasten-Anfang). Elemente mit
/// derselben Anfangsspur (erste Grundlinie) bzw. Endspur (letzte) teilen
/// eine Grundlinie. GEMESSEN vorher: WPT grid-lanes
/// `grid-lanes-overflow-reverse-baseline-002` (automatisch platzierte
/// Elemente blieben am Spuranfang).
pub(crate) fn lanes_grundlinien(l: &Lanes, gelegt: &mut [crate::plan::Platzierung]) {
    if l.grund.iter().all(|g| g.0 == 0) || l.spuren.is_empty() || gelegt.len() != l.grund.len() {
        return;
    }
    let spur_bei = |x: f32| l.spuren.iter().rposition(|(a, _)| *a <= x + 0.5).unwrap_or(0);
    let mut gruppe: Vec<Option<(u8, usize, usize, usize)>> = vec![None; gelegt.len()];
    let mut gemeinsam: std::collections::HashMap<(u8, usize), f32> = std::collections::HashMap::new();
    for (i, &(art, wert, _)) in l.grund.iter().enumerate() {
        if art == 0 {
            continue;
        }
        let Some(&(z0, _, _, _, fest, spann)) = l.elemente.get(i) else { continue };
        let j0 = spur_bei(if fest { z0 } else { gelegt[i].x });
        let j1 = (j0 + spann.max(1) - 1).min(l.spuren.len() - 1);
        let schluessel = if art == 1 { j0 } else { j1 };
        let g = gemeinsam.entry((art, schluessel)).or_insert(f32::MIN);
        *g = g.max(wert);
        gruppe[i] = Some((art, schluessel, j0, j1));
    }
    for (i, gr) in gruppe.iter().enumerate() {
        let Some((art, schluessel, j0, j1)) = *gr else { continue };
        let (_, wert, aussen) = l.grund[i];
        let v = (gemeinsam[&(art, schluessel)] - wert).max(0.0);
        let p = &mut gelegt[i];
        if art == 1 {
            p.x = l.spuren[j0].0 + v;
        } else {
            let ende = l.spuren[j1].0 + l.spuren[j1].1;
            p.x = ende - v - (p.breite + aussen);
        }
    }
}

/// Die Mindest- und Hoechstgroesse (Inhaltsbox, px) eines Masonry-
/// Containers auf einer Achse (`zeilen`: senkrecht).
fn lanes_grenzen(ctx: &Ctx<'_>, sizes: &Sizes, node: &narsil_dom::Node, zeilen: bool) -> (Option<f32>, Option<f32>) {
    let Some(plan) = ctx.plan else { return (None, None) };
    let g = plan.grenzen(node.id);
    let (mn, mx) = if zeilen { (g.min_hoehe, g.max_hoehe) } else { (g.min_breite, g.max_breite) };
    // Release 11.10.2026: Prozentgrenzen gegen den Platz des Containers (die
    // Zahl in `grenzen` rechnet gegen die Ansicht). GEMESSEN vorher: WPT
    // `grid-lanes/track-sizing/auto-repeat/row-auto-repeat-008` (13 statt 4
    // Zeilen bei `min-height: 80%` in 250 px).
    let gp = plan.grenzen_prozent(node.id);
    let platz = sizes.verfuegbar_von(node.id);
    let basis = if zeilen { platz.height } else { platz.width };
    let pz = |p: Option<f32>| p.filter(|_| basis.is_finite()).map(|p| basis * p / 100.0);
    let (pmn, pmx) = if zeilen { (pz(gp.min_hoehe), pz(gp.max_hoehe)) } else { (pz(gp.min_breite), pz(gp.max_breite)) };
    let (mn, mx) = (pmn.or(mn), pmx.or(mx));
    let abzug = if ctx.stil(node.id).is_some_and(|s| s.border_box) {
        let w = sizes.get(node.id).map_or(0.0, |s| s.width);
        let fuell = resolve_spacing(node.style.padding, w, node.id, "padding").unwrap_or(Edges::ZERO);
        let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
        if zeilen { fuell.top + fuell.bottom + rahmen.top + rahmen.bottom } else { fuell.left + fuell.right + rahmen.left + rahmen.right }
    } else {
        0.0
    };
    let netto = |v: Option<f32>| v.filter(|v| v.is_finite()).map(|v| (v - abzug).max(0.0));
    (netto(mn), netto(mx))
}

/// Masonry: die Rasterbereiche (bzw. statischen Lagen) der absolut
/// positionierten Kinder aus dem abgelegten Ergebnis (`lanes_ablegen`).
pub(crate) fn lanes_abspos(ctx: &Ctx<'_>, node: &narsil_dom::Node, stil: &narsil_layout::LayoutStil, gs: &GitterStil, content: &Rect) -> Result<(), LayoutError> {
    let (_, abspos) = rasterelemente(ctx, node);
    if abspos.is_empty() {
        return Ok(());
    }
    let abs_ausr = |id: NodeId| -> (narsil_css::FlexAusrichtung, narsil_css::FlexAusrichtung) {
        let ks = kind_stil(ctx, id);
        let js = selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.justify_self), gs.justify_items).lage;
        let al = selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.align_self), gs.align_items).lage;
        // Release 11.10.2026: physisch wie im Raster (`verteilen`) — bei
        // `direction: rtl` ist der Anfang rechts. GEMESSEN vorher: WPT
        // `grid-lanes/alignment/row-grid-lanes-alignment-positioned-items-001`
        // (das rtl-Raster stand wie ltr).
        let jl = match (js, gs.rtl) {
            (AusrLage::Links, _) => AusrLage::Anfang,
            (AusrLage::Rechts, _) => AusrLage::Ende,
            (AusrLage::Anfang, true) => AusrLage::Ende,
            (AusrLage::Ende, true) => AusrLage::Anfang,
            (AusrLage::Normal | AusrLage::Strecken | AusrLage::Grundlinie, true) => AusrLage::Ende,
            (l, _) => l,
        };
        (flex_lage(al), flex_lage(jl))
    };
    if stil.position == PositionArt::Statisch {
        for &id in &abspos {
            let (av, ah) = abs_ausr(id);
            ctx.gitter_statisch.borrow_mut().insert(id, (*content, av, ah));
        }
        return Ok(());
    }
    let Some(e) = ctx.gitter.borrow().get(&node.id).cloned() else { return Ok(()) };
    for (id, rect) in abspos_rechtecke(ctx, node, gs, &e, content, &abspos)? {
        let (av, ah) = abs_ausr(id);
        ctx.raster_bereiche.borrow_mut().insert(id, (rect, av, ah));
    }
    Ok(())
}

/// Die Inhaltsbreite eines Knotens aus der Messung (fuer Prozente).
fn content_breite(sizes: &Sizes, node: &narsil_dom::Node) -> f32 {
    let w = sizes.get(node.id).map_or(0.0, |s| s.width);
    let fuell = resolve_spacing(node.style.padding, w, node.id, "padding").unwrap_or(Edges::ZERO);
    let rahmen = crate::r#box::rahmen_edges(&node.style.rahmen);
    (w - fuell.left - fuell.right - rahmen.left - rahmen.right).max(0.0)
}

/// Release 11.10.2026: der aufgeloeste Wert von `grid-template-columns`/
/// `-rows` je Raster (CSS Grid 1 §7.2.6): jede Spur einzeln in px, die Namen
/// der expliziten Linien in Klammern; ohne Spuren `none`.
pub(crate) fn aufgeloeste_vorlagen(ctx: &Ctx<'_>) -> Vec<(NodeId, String, String)> {
    fn zahl(v: f32) -> String {
        let r = (v * 1000.0).round() / 1000.0;
        if r == r.trunc() { format!("{}px", r as i64) } else { format!("{r}px") }
    }
    fn achse(a: &Achse) -> String {
        let n = a.groesse.len();
        if n == 0 {
            return String::from("none");
        }
        let mut teile: Vec<String> = Vec::with_capacity(2 * n + 1);
        for k in 0..=n {
            let li = k as i32 - a.versatz;
            if li >= 0
                && let Some(namen) = a.namen.get(li as usize)
                && !namen.is_empty()
            {
                teile.push(format!("[{}]", namen.join(" ")));
            }
            if k < n {
                teile.push(zahl(a.groesse[k]));
            }
        }
        teile.join(" ")
    }
    let mut raus: Vec<(NodeId, String, String)> =
        ctx.gitter.borrow().iter().map(|(id, e)| (*id, achse(&e.raster.spalten), achse(&e.raster.zeilen))).collect();
    raus.sort_by_key(|(id, _, _)| id.0);
    raus
}

/// `NARSIL_GITTER_DEBUG=1`: die Raster auf stderr (einmal gelesen).
fn debug_an() -> bool {
    static AN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AN.get_or_init(|| std::env::var_os("NARSIL_GITTER_DEBUG").is_some())
}

fn flex_lage(l: AusrLage) -> narsil_css::FlexAusrichtung {
    match l {
        AusrLage::Mitte => narsil_css::FlexAusrichtung::Mitte,
        AusrLage::Ende => narsil_css::FlexAusrichtung::Ende,
        _ => narsil_css::FlexAusrichtung::Start,
    }
}

/// Die Platzierung (`flow.rs`): die Kinder eines Rasters, Index = Kindindex.
pub(crate) fn verteilen(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    content: &Rect,
    stil: &narsil_layout::LayoutStil,
    gs: &GitterStil,
    sizes: &Sizes,
) -> Result<Vec<Platzierung>, LayoutError> {
    let n = node.children.len().min(crate::plan::MAX_KINDER);
    let (sub_sp, sub_ze) = subgrid_achsen(ctx, node.id);
    let gemerkt = ctx
        .gitter
        .borrow()
        .get(&node.id)
        .filter(|e| (e.breite - content.width).abs() < 0.5 && e.zeilen_fertig && !sub_sp && !sub_ze)
        .cloned();
    let e = match gemerkt {
        Some(e) => e,
        None => {
            let definit = stil.hoehe_angabe.is_some() || stil.hoehe_anteil.is_some() || sub_ze;
            let h = (definit && content.height.is_finite()).then_some(content.height.max(0.0));
            let e = rechnen(ctx, node, gs, sizes, Platz::Definit(content.width), h, None, None, true, 0)?;
            // Fuer die Subgrids darin: dieses Ergebnis gilt jetzt.
            ctx.gitter.borrow_mut().insert(node.id, e.clone());
            e
        }
    };
    let sp = &e.raster.spalten;
    let ze = &e.raster.zeilen;
    if debug_an() {
        eprintln!(
            "GITTER #{} inhalt={:?} spalten={:?} zeilen={:?} kinder={:?} bereiche={:?}",
            node.id.0, content, sp.groesse, ze.groesse, e.kinder, e.raster.bereiche
        );
        for &c in &node.children {
            eprintln!("  kind #{} stil={:?} masse={:?}", c.0, ctx.stil(c).map(|s| (s.position, s.anzeige)), sizes.get(c));
        }
    }
    let xs = spur_lagen(sp, gs.justify_content, content.width, gs.rtl);
    let ys = spur_lagen(ze, gs.align_content, content.height, false);
    let phys_x = |x0: f32, x1: f32| -> (f32, f32) {
        if gs.rtl {
            (content.x + content.width - x1, content.x + content.width - x0)
        } else {
            (content.x + x0, content.x + x1)
        }
    };
    let mut alle = vec![Platzierung::WEG; n];
    let versatz = grundlinien_versaetze(ctx, gs, sizes, &e, content.width, node);
    for (ki, (b, &(index, id))) in e.raster.bereiche.iter().zip(&e.kinder).enumerate() {
        let (lx0, lx1) = bereich_spanne(sp, &xs, b.s0, b.s1);
        let (x0, x1) = phys_x(lx0, lx1);
        let (ly0, ly1) = bereich_spanne(ze, &ys, b.z0, b.z1);
        let (y0, y1) = (content.y + ly0, content.y + ly1);
        // Prozentraender und -innenabstaende beziehen sich auf den
        // Rasterbereich (CSS Grid 1 §6.2); `flow.rs` legt das Kind mit
        // dieser Flaeche. GEMESSEN vorher: WPT `grid-items-percentage-*`.
        ctx.gitter_flaeche.borrow_mut().insert(id, Size::new((x1 - x0).max(0.0), (y1 - y0).max(0.0)));
        let m = masse(ctx, id, (x1 - x0).max(0.0), (y1 - y0).max(0.0))?;
        let gemessen = sizes.get(id).unwrap_or(Size::new(0.0, 0.0));
        let ks = kind_stil(ctx, id);
        let (jl, js, jstreck) = selbst_wahl(ks.map_or(Ausr::AUTO, |k| k.justify_self), gs.justify_items, &m, true);
        let (al, asi, astreck) = selbst_wahl(ks.map_or(Ausr::AUTO, |k| k.align_self), gs.align_items, &m, false);
        // Release 11.10.2026: eine eigene Prozenthoehe (`hoehe_bedingt`) ist
        // keine `auto`-Hoehe: nicht dehnen (`normal` = Anfang).
        let eigen_prozent = ctx.stil(id).is_some_and(|s| s.hoehe_bedingt.is_some()) && zeilen_fest(ze, b.z0, b.z1);
        let (al, astreck) = if eigen_prozent && astreck {
            (if matches!(al, AusrLage::Normal | AusrLage::Strecken) { AusrLage::Anfang } else { al }, false)
        } else {
            (al, astreck)
        };
        // Ein Subgrid fuellt seinen Bereich auf den Subgrid-Achsen (§9).
        let (k_sp, k_ze) = subgrid_achsen(ctx, id);
        let (jl, jstreck) = if k_sp { (AusrLage::Anfang, true) } else { (jl, jstreck) };
        let (al, astreck) = if k_ze { (AusrLage::Anfang, true) } else { (al, astreck) };
        let r = m.rand;
        // Waagerecht.
        let bw = x1 - x0;
        let w = if jstreck { klemmen(bw - r.left - r.right, m.min_b, m.max_b) } else { gemessen.width };
        let bx = if m.rand_auto[0] || m.rand_auto[2] {
            let frei = (bw - w - r.left - r.right).max(0.0);
            let links = match (m.rand_auto[0], m.rand_auto[2]) {
                (true, true) => frei / 2.0,
                (true, false) => frei,
                _ => 0.0,
            };
            x0 + r.left + links
        } else {
            // Logisch: `Anfang` ist bei RTL rechts.
            let lage = match (jl, gs.rtl) {
                (AusrLage::Links, false) | (AusrLage::Rechts, true) => AusrLage::Anfang,
                (AusrLage::Links, true) | (AusrLage::Rechts, false) => AusrLage::Ende,
                (l, _) => l,
            };
            let off = selbst_lage(lage, js, bw, w + r.left + r.right);
            if gs.rtl { x1 - off - r.right - w } else { x0 + off + r.left }
        };
        // Senkrecht.
        let bh = y1 - y0;
        let h = if astreck { klemmen(bh - r.top - r.bottom, m.min_h, m.max_h) } else { gemessen.height };
        let by = if m.rand_auto[1] || m.rand_auto[3] {
            let frei = (bh - h - r.top - r.bottom).max(0.0);
            let oben = match (m.rand_auto[1], m.rand_auto[3]) {
                (true, true) => frei / 2.0,
                (true, false) => frei,
                _ => 0.0,
            };
            y0 + r.top + oben
        } else if let Some(&(a, v)) = versatz.get(ki).filter(|(a, _)| *a != 0) {
            // Grundlinie: an die gemeinsame Grundlinie der Zeile.
            if a == 1 { y0 + r.top + v } else { y1 - r.bottom - h - v }
        } else {
            let lage = match al {
                AusrLage::Links | AusrLage::Rechts => AusrLage::Anfang,
                l => l,
            };
            y0 + selbst_lage(lage, asi, bh, h + r.top + r.bottom) + r.top
        };
        // Ein anonymes Element: seine Glieder (Zeilen) untereinander, so
        // breit wie der Bereich (Zeilenkaesten) bzw. ihr Text.
        if let Some(glieder) = &m.glieder {
            let mut y = y0;
            for &g in glieder.iter() {
                let Some(gi) = node.children.iter().position(|c| *c == g) else { continue };
                let gm = sizes.get(g).unwrap_or(Size::new(0.0, 0.0));
                let text = ctx.document.get(g).is_some_and(|x| x.kind == NodeKind::Text);
                let gb = if text { gm.width } else { (x1 - x0).max(0.0) };
                let gx = if gs.rtl { x1 - gb } else { x0 };
                if gi < n {
                    alle[gi] = Platzierung { x: gx, y, breite: gb, hoehe: gm.height };
                }
                y += gm.height;
            }
            continue;
        }
        if index < n {
            alle[index] = Platzierung { x: bx - r.left, y: by - r.top, breite: w.max(0.0), hoehe: h.max(0.0) };
        }
    }
    // Absolut positionierte Kinder eines positionierten Rasters: ihr
    // Rasterbereich ist der enthaltende Block (CSS Grid 1 §9.1).
    let (_, abspos) = rasterelemente(ctx, node);
    // Die Ausrichtung eines abspos-Kindes (physisch: links/oben = Start).
    let abs_ausr = |id: NodeId| -> (narsil_css::FlexAusrichtung, narsil_css::FlexAusrichtung) {
        let ks = kind_stil(ctx, id);
        let js = selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.justify_self), gs.justify_items).lage;
        let al = selbst_aufloesen(ks.map_or(Ausr::AUTO, |k| k.align_self), gs.align_items).lage;
        let jl = match (js, gs.rtl) {
            (AusrLage::Links, _) => AusrLage::Anfang,
            (AusrLage::Rechts, _) => AusrLage::Ende,
            (AusrLage::Anfang, true) => AusrLage::Ende,
            (AusrLage::Ende, true) => AusrLage::Anfang,
            (AusrLage::Normal | AusrLage::Strecken | AusrLage::Grundlinie, true) => AusrLage::Ende,
            (l, _) => l,
        };
        (flex_lage(al), flex_lage(jl))
    };
    if stil.position == PositionArt::Statisch {
        for &id in &abspos {
            let (av, ah) = abs_ausr(id);
            ctx.gitter_statisch.borrow_mut().insert(id, (*content, av, ah));
        }
    }
    if stil.position != PositionArt::Statisch {
        let nachfahren = abspos_nachfahren(ctx, node);
        let mut alle_abs = abspos;
        alle_abs.extend(nachfahren.iter().copied());
        if !alle_abs.is_empty() {
            for (id, rect) in abspos_rechtecke(ctx, node, gs, &e, content, &alle_abs)? {
                let (av, ah) = abs_ausr(id);
                ctx.raster_bereiche.borrow_mut().insert(id, (rect, av, ah));
            }
        }
        ctx.gitter_nachfahren.borrow_mut().extend(nachfahren);
    }
    Ok(alle)
}

/// Release 11.10.2026: die absolut positionierten Nachfahren eines
/// positionierten Rasters, deren enthaltender Block das Raster ist (kein
/// positionierter Kasten dazwischen; die Kinder selbst nicht). CSS Grid 1
/// §9.1: auch fuer sie ist der Rasterbereich ihrer Platzierungsangaben der
/// enthaltende Block. GEMESSEN vorher: WPT css-grid/abspos
/// `positioned-grid-descendant-containing-block-015` und
/// `positioned-grid-descendants-*` (Block = ganzes Raster).
fn abspos_nachfahren(ctx: &Ctx<'_>, node: &narsil_dom::Node) -> Vec<NodeId> {
    let mut raus = Vec::new();
    let mut stapel: Vec<NodeId> = Vec::new();
    for &k in node.children.iter().take(crate::plan::MAX_KINDER) {
        let pos = ctx.stil(k).map_or(PositionArt::Statisch, |s| s.position);
        if pos == PositionArt::Statisch {
            stapel.push(k);
        }
    }
    let mut besucht = 0usize;
    while let Some(id) = stapel.pop() {
        besucht += 1;
        if besucht > 4096 {
            break;
        }
        let Some(n) = ctx.document.get(id) else { continue };
        for &k in n.children.iter().take(crate::plan::MAX_KINDER) {
            let Some(s) = ctx.stil(k) else {
                stapel.push(k);
                continue;
            };
            if !s.wird_gezeichnet() {
                continue;
            }
            match s.position {
                PositionArt::Absolut => raus.push(k),
                PositionArt::Statisch => stapel.push(k),
                _ => {}
            }
        }
    }
    raus
}

/// Die Rasterbereiche absolut positionierter Kinder eines positionierten
/// Rasters (CSS Grid 1 §9.1): ihre Linien gegen die fertigen Spuren; eine
/// `auto`-Linie (oder eine, die es nicht gibt) ist die Innenabstandskante.
fn abspos_rechtecke(
    ctx: &Ctx<'_>,
    node: &narsil_dom::Node,
    gs: &GitterStil,
    e: &Ergebnis,
    content: &Rect,
    abspos: &[NodeId],
) -> Result<Vec<(NodeId, Rect)>, LayoutError> {
    let sp = &e.raster.spalten;
    let ze = &e.raster.zeilen;
    let xs = spur_lagen(sp, gs.justify_content, content.width, gs.rtl);
    let ys = spur_lagen(ze, gs.align_content, content.height, false);
    let phys_x = |x0: f32, x1: f32| -> (f32, f32) {
        if gs.rtl {
            (content.x + content.width - x1, content.x + content.width - x0)
        } else {
            (content.x + x0, content.x + x1)
        }
    };
    let fuell = resolve_spacing(node.style.padding, content.width, node.id, "padding")?;
    let si = AchsInfo { explizit: sp.explizit as i32, namen: &sp.namen };
    let zi = AchsInfo { explizit: ze.explizit as i32, namen: &ze.namen };
    // Die Lage einer Linie (Spurindex k, 0..=n), logisch; `None` = auto.
    let linie_lage = |a: &Achse, lagen: &[f32], l: Option<i32>| -> Option<f32> {
        let k = l? + a.versatz;
        let n = a.groesse.len() as i32;
        if k < 0 || k > n || n == 0 {
            None
        } else if k == n {
            Some(lagen[(n - 1) as usize] + a.groesse[(n - 1) as usize])
        } else {
            Some(lagen[k as usize])
        }
    };
    let mut raus = Vec::with_capacity(abspos.len());
    for &id in abspos {
        let leer = GitterPlatz::default();
        let ks = kind_stil(ctx, id);
        let p = ks.map_or(&leer, |k| &k.platz);
        let paar = |info: &AchsInfo<'_>, s: &GitterLinie, en: &GitterLinie| -> (Option<i32>, Option<i32>) {
            let a = info.definit(s, true);
            let b = info.definit(en, false);
            match (a, b, s, en) {
                (Some(a), None, _, GitterLinie::Spanne(k, None)) => (Some(a), Some(a + *k as i32)),
                (Some(a), None, _, GitterLinie::Spanne(k, Some(nm))) => (Some(a), Some(info.spanne_vor(a, nm, *k))),
                (None, Some(b), GitterLinie::Spanne(k, None), _) => (Some(b - *k as i32), Some(b)),
                (None, Some(b), GitterLinie::Spanne(k, Some(nm)), _) => (Some(info.spanne_zurueck(b, nm, *k)), Some(b)),
                (Some(a), Some(b), _, _) if a > b => (Some(b), Some(a)),
                (Some(a), Some(b), _, _) if a == b => (Some(a), None),
                (a, b, _, _) => (a, b),
            }
        };
        let (s0, s1) = paar(&si, &p.spalte_start, &p.spalte_ende);
        let (z0, z1) = paar(&zi, &p.zeile_start, &p.zeile_ende);
        // Logische Lagen; `auto` = Innenabstandskante.
        let lx0 = linie_lage(sp, &xs, s0).unwrap_or(-if gs.rtl { fuell.right } else { fuell.left });
        let lx1 = linie_lage(sp, &xs, s1).unwrap_or(content.width + if gs.rtl { fuell.left } else { fuell.right });
        let ly0 = linie_lage(ze, &ys, z0).unwrap_or(-fuell.top);
        let ly1 = linie_lage(ze, &ys, z1).unwrap_or(content.height + fuell.bottom);
        let (x0, x1) = phys_x(lx0, lx1.max(lx0));
        raus.push((id, Rect::new(x0, content.y + ly0, (x1 - x0).max(0.0), (ly1 - ly0).max(0.0))));
    }
    Ok(raus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use narsil_layout::gitter::SpurListe;

    fn liste(spuren: &[Spur]) -> SpurListe {
        SpurListe { teile: spuren.iter().map(|s| SpurTeil::Spur(*s)).collect(), subgrid: false }
    }

    fn fr(f: f32) -> Spur {
        Spur { min: SpurGrenze::Auto, max: SpurGrenze::Anteil(f), passend: None }
    }

    fn platz_auto() -> GitterPlatz {
        GitterPlatz::default()
    }

    #[test]
    fn implizite_spalten_aus_grid_auto_columns() {
        let stil = GitterStil { auto_spalten: vec![Spur::fest(10.0)], auto_zeilen: vec![Spur::fest(10.0)], ..Default::default() };
        let p = platz_auto();
        let r = platzieren(&stil, &[PlatzEingabe { platz: &p }], Some(800.0), None, None, None, 0.0, 0.0, Some(800.0), None, &Vorgaben::default());
        assert_eq!(r.spalten.spuren.len(), 1);
        assert_eq!(r.spalten.spuren[0], Spur::fest(10.0));
    }

    #[test]
    fn auto_platzierung_wickelt_um() {
        let stil = GitterStil { spalten: liste(&[Spur::fest(10.0), Spur::fest(10.0)]), ..Default::default() };
        let p = platz_auto();
        let e: Vec<PlatzEingabe> = (0..3).map(|_| PlatzEingabe { platz: &p }).collect();
        let r = platzieren(&stil, &e, Some(100.0), None, None, None, 0.0, 0.0, Some(100.0), None, &Vorgaben::default());
        assert_eq!(r.bereiche[2], Bereich { z0: 1, z1: 2, s0: 0, s1: 1 });
    }

    #[test]
    fn negative_linien_erzeugen_spuren_davor() {
        let stil = GitterStil { spalten: liste(&[Spur::fest(10.0)]), ..Default::default() };
        let p = GitterPlatz { spalte_start: GitterLinie::Zahl(-3, None), ..Default::default() };
        let r = platzieren(&stil, &[PlatzEingabe { platz: &p }], Some(100.0), None, None, None, 0.0, 0.0, Some(100.0), None, &Vorgaben::default());
        assert_eq!(r.spalten.versatz, 1);
        assert_eq!(r.bereiche[0].s0, 0);
    }

    #[test]
    fn fr_verteilt_den_rest() {
        let stil = GitterStil { spalten: liste(&[Spur::fest(100.0), fr(1.0), fr(3.0)]), ..Default::default() };
        let r = platzieren(&stil, &[], Some(500.0), None, None, None, 0.0, 0.0, Some(500.0), None, &Vorgaben::default());
        let mut a = r.spalten;
        spuren_groessen(
            &mut a,
            &AchsRechnung { platz: Platz::Definit(500.0), prozent_basis: Some(500.0), min_platz: None, max_platz: None, strecken: true, elemente: &[] },
        );
        assert_eq!(a.groesse, vec![100.0, 100.0, 300.0]);
    }

    #[test]
    fn spanne_verteilt_auf_auto_spuren() {
        let mut a = Achse {
            spuren: vec![Spur::AUTO, Spur::AUTO],
            kollabiert: vec![false, false],
            groesse: vec![0.0, 0.0],
            ..Default::default()
        };
        let el = [AchsElement { a: 0, b: 2, beitrag: Beitrag { min: 60.0, max: 100.0, minimum: 60.0, minimum_inhalt: true } }];
        spuren_groessen(&mut a, &AchsRechnung { platz: Platz::MaxInhalt, prozent_basis: None, min_platz: None, max_platz: None, strecken: true, elemente: &el });
        assert_eq!(a.groesse, vec![50.0, 50.0]);
    }

    #[test]
    fn fit_content_kappt() {
        let mut a = Achse {
            spuren: vec![Spur { min: SpurGrenze::Auto, max: SpurGrenze::MaxInhalt, passend: Some(LaengeProzent::px(40.0)) }],
            kollabiert: vec![false],
            groesse: vec![0.0],
            ..Default::default()
        };
        let el = [AchsElement { a: 0, b: 1, beitrag: Beitrag { min: 20.0, max: 100.0, minimum: 20.0, minimum_inhalt: true } }];
        spuren_groessen(&mut a, &AchsRechnung { platz: Platz::Definit(500.0), prozent_basis: Some(500.0), min_platz: None, max_platz: None, strecken: true, elemente: &el });
        assert_eq!(a.groesse, vec![40.0]);
    }
}
