//! Release 11.10.2026: die Rastereigenschaften (CSS Grid 1/2) als Werte des
//! neuen Rasteralgorithmus (`narsil_layout::gitter`).
//!
//! GEMESSEN vorher (WPT css-grid, 564 von 2460): `parse::raster_spalten`
//! kannte keine `min-content`/`max-content`/`fit-content()`-Spuren, kein
//! `minmax(auto, …)`, hoechstens 32 Spuren, und die Linienangaben der Kinder
//! waren Zahlen ohne Namen. Diese Datei liest dieselben Texte in die reichere
//! Form; die alte bleibt fuer Masonry und die alten Tests.

use narsil_layout::gitter::{
    Ausr, AusrLage, GitterLinie, GitterPlatz, LaengeProzent, Spur, SpurGrenze, SpurListe, SpurTeil,
};

use crate::rechnen::{RechenCtx, rechne};

/// Hoechstens so viele Spuren je Achse aus einer Vorlage (CSS Grid 1 §7.2.3:
/// eine Implementierung darf begrenzen; WPT `grid-limits-001` will 9999).
pub const MAX_SPUREN: usize = 10_000;

/// Die Gebietsmatrix (`grid-template-areas`): Zeilen von Namen, leer = `.`.
pub type Gebiete = Vec<Vec<String>>;

/// Teilt einen Wert an Leerraum auf der obersten Ebene; `[…]` ist immer ein
/// eigenes Stueck (auch ohne Leerraum davor/danach), Klammern bleiben ganz.
fn stuecke(s: &str) -> Option<Vec<String>> {
    let mut raus: Vec<String> = Vec::new();
    let mut akt = String::new();
    let mut tiefe = 0i32;
    let mut in_namen = false;
    for c in s.chars() {
        if in_namen {
            akt.push(c);
            if c == ']' {
                raus.push(std::mem::take(&mut akt));
                in_namen = false;
            }
            continue;
        }
        match c {
            '(' => {
                tiefe += 1;
                akt.push(c);
            }
            ')' => {
                tiefe -= 1;
                if tiefe < 0 {
                    return None;
                }
                akt.push(c);
            }
            '[' if tiefe == 0 => {
                if !akt.trim().is_empty() {
                    raus.push(std::mem::take(&mut akt));
                }
                akt.clear();
                akt.push('[');
                in_namen = true;
            }
            c if c.is_whitespace() && tiefe == 0 => {
                if !akt.is_empty() {
                    raus.push(std::mem::take(&mut akt));
                }
            }
            c => akt.push(c),
        }
        if raus.len() > 4 * MAX_SPUREN {
            return None;
        }
    }
    if in_namen || tiefe != 0 {
        return None;
    }
    if !akt.is_empty() {
        raus.push(akt);
    }
    Some(raus)
}

/// Teilt die Argumente einer Funktion an Kommas der obersten Ebene.
fn argumente(innen: &str) -> Vec<String> {
    let mut raus = Vec::new();
    let mut akt = String::new();
    let mut tiefe = 0i32;
    for c in innen.chars() {
        match c {
            '(' | '[' => {
                tiefe += 1;
                akt.push(c);
            }
            ')' | ']' => {
                tiefe -= 1;
                akt.push(c);
            }
            ',' if tiefe == 0 => raus.push(std::mem::take(&mut akt).trim().to_string()),
            c => akt.push(c),
        }
    }
    raus.push(akt.trim().to_string());
    raus
}

/// `name(innen)` -> `innen`, wenn `s` genau diese Funktion ist.
fn funktion<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    let n = name.len();
    if s.len() > n + 1 && s[..n].eq_ignore_ascii_case(name) && s.as_bytes()[n] == b'(' && s.ends_with(')') {
        Some(&s[n + 1..s.len() - 1])
    } else {
        None
    }
}

/// Ist `s` ein `<custom-ident>` (ohne die CSS-weiten Schluesselwoerter)?
fn ist_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 64 {
        return false;
    }
    let erst = b[0];
    let gut_start = erst.is_ascii_alphabetic() || erst == b'_' || erst == b'-' || erst >= 0x80;
    if !gut_start || (erst == b'-' && b.len() > 1 && b[1].is_ascii_digit()) {
        return false;
    }
    if !b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-' || *c >= 0x80) {
        return false;
    }
    !matches!(
        s.to_ascii_lowercase().as_str(),
        "initial" | "inherit" | "unset" | "default" | "revert" | "revert-layer" | "span" | "auto"
    )
}

/// Eine `<length-percentage>` (mit `calc()`), nicht negativ, wenn verlangt.
///
/// Der Prozentanteil wird durch zweimaliges Rechnen (Basis 0 und 100)
/// abgespalten — exakt fuer `calc()` aus Summen und Faktoren.
pub fn laenge_prozent(t: &str, ctx: &RechenCtx, nicht_negativ: bool) -> Option<LaengeProzent> {
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    let k = t.to_ascii_lowercase();
    let ist_mathe = ["calc(", "min(", "max(", "clamp("].iter().any(|f| k.starts_with(f));
    // Eine reine Zahl ist keine Laenge (ausser 0).
    if !ist_mathe {
        // Release 11.10.2026: das Zahlende nach CSS Syntax 3 §4.3.12 — ein
        // `e` gehoert nur mit folgender Ziffer zur Zahl. GEMESSEN vorher:
        // `25em` las als Zahl „25e" und war ungueltig (WPT
        // css-grid/grid-definition/grid-support-repeat-002).
        let zahl_ende = zahl_laenge(&k);
        let einheit = &k[zahl_ende..];
        if einheit.is_empty() {
            let v: f32 = k.parse().ok()?;
            return (v == 0.0).then_some(LaengeProzent::px(0.0));
        }
        let (zahl, rest) = k.split_at(zahl_ende);
        let z: f32 = zahl.parse().ok()?;
        // Einheiten, die `rechne` nicht kennt (CSS Values 4 §6.2).
        let px = match rest {
            "in" => Some(z * 96.0),
            "cm" => Some(z * 96.0 / 2.54),
            "mm" => Some(z * 96.0 / 25.4),
            "q" => Some(z * 96.0 / 101.6),
            "pc" => Some(z * 16.0),
            "ex" => Some(z * ctx.em * 0.5),
            _ => None,
        };
        if let Some(px) = px {
            if !px.is_finite() || (nicht_negativ && px < 0.0) {
                return None;
            }
            return Some(LaengeProzent::px(px));
        }
    }
    let hat_prozent = k.contains('%');
    let mut c0 = ctx.clone();
    c0.prozent = 0.0;
    let a = rechne(&k, &c0, 0)?;
    let (px, prozent) = if hat_prozent {
        let mut c1 = ctx.clone();
        c1.prozent = 100.0;
        let b = rechne(&k, &c1, 0)?;
        (a, b - a)
    } else {
        (a, 0.0)
    };
    if !px.is_finite() || !prozent.is_finite() {
        return None;
    }
    // Negativ ist nur `calc()` erlaubt (wird spaeter auf 0 geklemmt).
    if nicht_negativ && !ist_mathe && (px < 0.0 || prozent < 0.0) {
        return None;
    }
    Some(LaengeProzent { px, prozent, hat_prozent })
}

/// Die Laenge der fuehrenden Zahl in `k` (Vorzeichen, Ziffern, Punkt,
/// Exponent nur mit Ziffer dahinter).
fn zahl_laenge(k: &str) -> usize {
    let b = k.as_bytes();
    let mut i = 0usize;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    i
}

/// Eine `<flex>`-Angabe (`2fr`), nicht negativ.
fn anteil(t: &str) -> Option<f32> {
    let k = t.trim().to_ascii_lowercase();
    let z = k.strip_suffix("fr")?;
    let v: f32 = z.parse().ok()?;
    (v.is_finite() && v >= 0.0).then_some(v)
}

/// Eine Spurgrenze (`<track-breadth>`, mit `fr`, wenn erlaubt).
fn grenze(t: &str, ctx: &RechenCtx, fr_erlaubt: bool) -> Option<SpurGrenze> {
    let k = t.trim().to_ascii_lowercase();
    match k.as_str() {
        "auto" => return Some(SpurGrenze::Auto),
        "min-content" => return Some(SpurGrenze::MinInhalt),
        "max-content" => return Some(SpurGrenze::MaxInhalt),
        _ => {}
    }
    if let Some(f) = anteil(&k) {
        return fr_erlaubt.then_some(SpurGrenze::Anteil(f));
    }
    laenge_prozent(&k, ctx, true).map(SpurGrenze::Laenge)
}

/// Eine Spurgroesse (`<track-size>`, CSS Grid 1 §7.2.1).
pub fn spur(t: &str, ctx: &RechenCtx) -> Option<Spur> {
    let t = t.trim();
    if let Some(innen) = funktion(t, "minmax") {
        let a = argumente(innen);
        if a.len() != 2 {
            return None;
        }
        let min = grenze(&a[0], ctx, false)?;
        let max = grenze(&a[1], ctx, true)?;
        return Some(Spur { min, max, passend: None });
    }
    if let Some(innen) = funktion(t, "fit-content") {
        let l = laenge_prozent(innen, ctx, true)?;
        return Some(Spur { min: SpurGrenze::Auto, max: SpurGrenze::MaxInhalt, passend: Some(l) });
    }
    match grenze(t, ctx, true)? {
        SpurGrenze::Anteil(f) => Some(Spur { min: SpurGrenze::Auto, max: SpurGrenze::Anteil(f), passend: None }),
        g => Some(Spur { min: g, max: g, passend: None }),
    }
}

/// `[a b]` -> die Namen.
fn namen(t: &str) -> Option<Vec<String>> {
    let innen = t.strip_prefix('[')?.strip_suffix(']')?;
    let mut raus = Vec::new();
    for n in innen.split_whitespace() {
        if !ist_name(n) {
            return None;
        }
        raus.push(n.to_ascii_lowercase());
    }
    Some(raus)
}

/// Haengt Namen an (und verschmilzt sie mit einem direkt davor stehenden
/// Namensteil, `[a] [b]` ist `[a b]`).
fn namen_anhaengen(teile: &mut Vec<SpurTeil>, n: Vec<String>) {
    if let Some(SpurTeil::Namen(alt)) = teile.last_mut() {
        alt.extend(n);
    } else {
        teile.push(SpurTeil::Namen(n));
    }
}

/// Liest den Inhalt eines `repeat()`: Namen und Spuren.
fn wiederhol_inhalt(innen: &str, ctx: &RechenCtx) -> Option<Vec<SpurTeil>> {
    let mut teile = Vec::new();
    let mut spuren = 0usize;
    for st in stuecke(innen)? {
        if st.starts_with('[') {
            namen_anhaengen(&mut teile, namen(&st)?);
        } else {
            teile.push(SpurTeil::Spur(spur(&st, ctx)?));
            spuren += 1;
        }
    }
    (spuren > 0).then_some(teile)
}

/// Ist die Spur fest (`<fixed-size>`, CSS Grid 1 §7.2.3: mindestens eine
/// Grenze ist eine Laenge, die andere kein `fr` als Untergrenze)?
fn ist_fest(s: &Spur) -> bool {
    matches!(s.min, SpurGrenze::Laenge(_)) || (matches!(s.max, SpurGrenze::Laenge(_)) && s.passend.is_none())
}

/// `grid-template-columns`/`-rows` (`none`, `<track-list>`,
/// `<auto-track-list>`, `subgrid <line-name-list>?`).
pub fn spurliste(x: &str, ctx: &RechenCtx) -> Option<SpurListe> {
    let k = x.trim();
    if k.eq_ignore_ascii_case("none") || k.is_empty() {
        return Some(SpurListe::default());
    }
    let st = stuecke(k)?;
    let mut liste = SpurListe::default();
    let mut rest: &[String] = &st;
    if st.first().is_some_and(|s| s.eq_ignore_ascii_case("subgrid")) {
        liste.subgrid = true;
        rest = &st[1..];
    }
    let mut anzahl = 0usize;
    let mut autos = 0usize;
    // Release 11.10.2026: zwei Namensgruppen hintereinander sind ausserhalb
    // von `subgrid` ungueltig (§7.2: `<line-names>? <track-size>`); mit einer
    // Auto-Wiederholung muessen die uebrigen Spuren fest sein (§7.2.3.2
    // `<auto-track-list>`). GEMESSEN vorher: WPT
    // `parsing/grid-template-columns-invalid` (`[one] 10px [two] [three]`,
    // `auto repeat(auto-fill, auto) auto`).
    let mut zuletzt_namen = false;
    let mut nicht_fest = false;
    for s in rest {
        if s.starts_with('[') {
            if zuletzt_namen && !liste.subgrid {
                return None;
            }
            zuletzt_namen = true;
            namen_anhaengen(&mut liste.teile, namen(s)?);
            continue;
        }
        zuletzt_namen = false;
        if let Some(innen) = funktion(s, "repeat") {
            let a = argumente(innen);
            if a.len() != 2 {
                return None;
            }
            let zahl = a[0].trim().to_ascii_lowercase();
            if liste.subgrid {
                // `repeat(N | auto-fill, [a] [b])` — nur Namen.
                let mut gruppen: Vec<Vec<String>> = Vec::new();
                for g in stuecke(&a[1])? {
                    gruppen.push(namen(&g)?);
                }
                if gruppen.is_empty() {
                    return None;
                }
                if zahl == "auto-fill" {
                    autos += 1;
                    liste.teile.push(SpurTeil::Auto {
                        fit: false,
                        teile: gruppen.into_iter().map(SpurTeil::Namen).collect(),
                    });
                } else {
                    let n: usize = zahl.parse().ok().filter(|n| *n >= 1)?;
                    for _ in 0..n.min(MAX_SPUREN) {
                        for g in &gruppen {
                            liste.teile.push(SpurTeil::Namen(g.clone()));
                        }
                    }
                }
                continue;
            }
            let inhalt = wiederhol_inhalt(&a[1], ctx)?;
            match zahl.as_str() {
                "auto-fill" | "auto-fit" => {
                    // CSS Grid 3 (vorlaeufig): auch intrinsische Spuren in der
                    // Wiederholung; nur `fr` bleibt der Wiederholung verwehrt.
                    if inhalt.iter().any(|t| matches!(t, SpurTeil::Spur(s) if matches!(s.min, SpurGrenze::Anteil(_)) || (matches!(s.max, SpurGrenze::Anteil(_)) && !ist_fest(s)))) {
                        return None;
                    }
                    autos += 1;
                    liste.teile.push(SpurTeil::Auto { fit: zahl == "auto-fit", teile: inhalt });
                }
                _ => {
                    let n: usize = zahl.parse().ok().filter(|n| *n >= 1)?;
                    if inhalt.iter().any(|t| matches!(t, SpurTeil::Spur(s) if !ist_fest(s))) {
                        nicht_fest = true;
                    }
                    let je = inhalt.iter().filter(|t| matches!(t, SpurTeil::Spur(_))).count().max(1);
                    let n = n.min((MAX_SPUREN.saturating_sub(anzahl) / je).max(1));
                    for _ in 0..n {
                        for t in &inhalt {
                            match t {
                                SpurTeil::Namen(nn) => namen_anhaengen(&mut liste.teile, nn.clone()),
                                t => {
                                    liste.teile.push(t.clone());
                                    anzahl += 1;
                                }
                            }
                        }
                    }
                }
            }
            continue;
        }
        if liste.subgrid {
            return None;
        }
        let sp = spur(s, ctx)?;
        if !ist_fest(&sp) {
            nicht_fest = true;
        }
        liste.teile.push(SpurTeil::Spur(sp));
        anzahl += 1;
    }
    if autos > 1 || (autos == 1 && nicht_fest) {
        return None;
    }
    if !liste.subgrid && anzahl == 0 && autos == 0 {
        return None;
    }
    // Mit `auto-fill`/`auto-fit` duerfen daneben keine `fr`-Spuren ohne
    // feste Untergrenze stehen (§7.2.3; intrinsische erlaubt Grid 3).
    if autos == 1
        && !liste.subgrid
        && liste.teile.iter().any(|t| matches!(t, SpurTeil::Spur(s) if matches!(s.max, SpurGrenze::Anteil(_)) && !ist_fest(s)))
    {
        return None;
    }
    Some(liste)
}

/// `grid-auto-columns`/`-rows`: eine Liste von Spurgroessen.
pub fn spuren_einfach(x: &str, ctx: &RechenCtx) -> Option<Vec<Spur>> {
    let mut raus = Vec::new();
    for s in stuecke(x.trim())? {
        if s.starts_with('[') {
            return None;
        }
        raus.push(spur(&s, ctx)?);
    }
    (!raus.is_empty()).then_some(raus)
}

/// Eine Linienangabe (`<grid-line>`, CSS Grid 1 §8.3).
pub fn linie(x: &str) -> Option<GitterLinie> {
    let w: Vec<&str> = x.split_whitespace().collect();
    if w.is_empty() || w.len() > 3 {
        return None;
    }
    if w.len() == 1 && w[0].eq_ignore_ascii_case("auto") {
        return Some(GitterLinie::Auto);
    }
    let mut span = false;
    let mut zahl: Option<i32> = None;
    let mut name: Option<String> = None;
    for t in &w {
        if t.eq_ignore_ascii_case("span") {
            if span {
                return None;
            }
            span = true;
        } else if let Ok(z) = t.parse::<i32>() {
            if zahl.is_some() {
                return None;
            }
            zahl = Some(z);
        } else if ist_name(t) {
            if name.is_some() {
                return None;
            }
            name = Some(t.to_ascii_lowercase());
        } else {
            return None;
        }
    }
    // `span` muss am Anfang oder am Ende stehen.
    if span && !(w[0].eq_ignore_ascii_case("span") || w[w.len() - 1].eq_ignore_ascii_case("span")) {
        return None;
    }
    if span {
        // Release 11.10.2026: `span` allein ist ungueltig (CSS Grid 1 §8.3:
        // `span && [ <integer> || <custom-ident> ]`). GEMESSEN vorher: WPT
        // `parsing/grid-column-invalid` (`span` wurde gesetzt).
        if zahl.is_none() && name.is_none() {
            return None;
        }
        let n = zahl.unwrap_or(1);
        if n < 1 {
            return None;
        }
        return Some(GitterLinie::Spanne(n.min(10_000) as u32, name));
    }
    match (zahl, name) {
        (Some(0), _) => None,
        (Some(z), n) => Some(GitterLinie::Zahl(z.clamp(-10_000, 10_000), n)),
        (None, Some(n)) => Some(GitterLinie::Name(n)),
        (None, None) => None,
    }
}

/// Die Teile einer Kurzform an `/`.
fn schraeg(x: &str) -> Vec<&str> {
    x.split('/').map(str::trim).collect()
}

/// `grid-row`/`grid-column`: `<start> [ / <end> ]?`. Fehlt das Ende, ist es
/// der Name des Anfangs (wenn er ein Name ist), sonst `auto` (§8.4).
pub fn linien_paar(x: &str) -> Option<(GitterLinie, GitterLinie)> {
    let t = schraeg(x);
    if t.is_empty() || t.len() > 2 {
        return None;
    }
    let a = linie(t[0])?;
    let b = match t.get(1) {
        Some(e) => linie(e)?,
        None => match &a {
            GitterLinie::Name(n) => GitterLinie::Name(n.clone()),
            _ => GitterLinie::Auto,
        },
    };
    Some((a, b))
}

/// `grid-area`: bis zu vier Linien (§8.4). Fehlende Teile: der Name des
/// Gegenstuecks, sonst `auto`.
pub fn gebiet_kurz(x: &str) -> Option<GitterPlatz> {
    let t = schraeg(x);
    if t.is_empty() || t.len() > 4 {
        return None;
    }
    let mut l: Vec<GitterLinie> = Vec::new();
    for s in &t {
        l.push(linie(s)?);
    }
    let name_von = |g: &GitterLinie| match g {
        GitterLinie::Name(n) => GitterLinie::Name(n.clone()),
        _ => GitterLinie::Auto,
    };
    let zs = l[0].clone();
    let ss = l.get(1).cloned().unwrap_or_else(|| name_von(&zs));
    let ze = l.get(2).cloned().unwrap_or_else(|| name_von(&zs));
    let se = l.get(3).cloned().unwrap_or_else(|| name_von(&ss));
    Some(GitterPlatz { zeile_start: zs, spalte_start: ss, zeile_ende: ze, spalte_ende: se })
}

/// `grid-template-areas`: die Zeilen als Namen (leer = `.`). `None` =
/// ungueltig (ungleiche Zeilenlaengen oder nicht rechteckige Gebiete).
pub fn gebiete(x: &str) -> Option<Gebiete> {
    let k = x.trim();
    if k.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let mut zeilen: Gebiete = Vec::new();
    let mut rest = k;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let q = rest.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let nach = &rest[1..];
        let ende = nach.find(q)?;
        let innen = &nach[..ende];
        rest = &nach[ende + 1..];
        let mut zeile: Vec<String> = Vec::new();
        // Zellen: Namen und Punktfolgen (`...` ist EINE leere Zelle).
        let mut akt = String::new();
        let mut punkte = false;
        let schliessen = |akt: &mut String, zeile: &mut Vec<String>, punkte: bool| {
            if punkte {
                zeile.push(String::new());
            } else if !akt.is_empty() {
                zeile.push(akt.to_ascii_lowercase());
            }
            akt.clear();
        };
        for c in innen.chars() {
            if c.is_whitespace() {
                schliessen(&mut akt, &mut zeile, punkte);
                punkte = false;
            } else if c == '.' {
                if !punkte && !akt.is_empty() {
                    schliessen(&mut akt, &mut zeile, false);
                }
                punkte = true;
            } else {
                if punkte {
                    schliessen(&mut akt, &mut zeile, true);
                    punkte = false;
                }
                if !(c.is_alphanumeric() || c == '-' || c == '_' || (c as u32) >= 0x80) {
                    return None;
                }
                akt.push(c);
            }
        }
        schliessen(&mut akt, &mut zeile, punkte);
        if zeile.is_empty() {
            return None;
        }
        zeilen.push(zeile);
        if zeilen.len() > MAX_SPUREN {
            return None;
        }
    }
    if zeilen.is_empty() {
        return None;
    }
    let breite = zeilen[0].len();
    if zeilen.iter().any(|z| z.len() != breite) {
        return None;
    }
    // Jedes Gebiet muss ein Rechteck sein (§7.3).
    let mut gesehen: Vec<&str> = Vec::new();
    for (zi, z) in zeilen.iter().enumerate() {
        for (si, n) in z.iter().enumerate() {
            if n.is_empty() || gesehen.contains(&n.as_str()) {
                continue;
            }
            gesehen.push(n);
            let mut s1 = si;
            while s1 < breite && zeilen[zi][s1] == *n {
                s1 += 1;
            }
            let mut z1 = zi;
            while z1 < zeilen.len() && zeilen[z1][si] == *n {
                z1 += 1;
            }
            for (zz, zeile) in zeilen.iter().enumerate() {
                for (ss, m) in zeile.iter().enumerate() {
                    let innen = zz >= zi && zz < z1 && ss >= si && ss < s1;
                    if (m == n) != innen {
                        return None;
                    }
                }
            }
        }
    }
    Some(zeilen)
}

/// Welche Eigenschaft eine Ausrichtung liest (die Wertemengen sind
/// verschieden, CSS Box Alignment 3 §5/§6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AusrArt {
    /// `justify-content`/`align-content`.
    Inhalt,
    /// `justify-self`/`align-self`.
    Selbst,
    /// `justify-items`/`align-items`.
    Elemente,
}

/// Liest einen Ausrichtungswert. `None` = ungueltig.
pub fn ausrichtung(x: &str, art: AusrArt) -> Option<Ausr> {
    let k = x.trim().to_ascii_lowercase();
    let w: Vec<&str> = k.split_whitespace().collect();
    if w.is_empty() || w.len() > 3 {
        return None;
    }
    let lage_von = |s: &str| -> Option<AusrLage> {
        Some(match s {
            "normal" => AusrLage::Normal,
            "stretch" => AusrLage::Strecken,
            // Release 11.10.2026: `flow-start`/`flow-end` (CSS Box Alignment 4,
            // im Flex-Container wie `flex-start`/`flex-end`). GEMESSEN vorher:
            // WPT `css-flexbox/alignment/flow-start-flow-end-*`.
            "start" | "self-start" | "flex-start" | "flow-start" => AusrLage::Anfang,
            "end" | "self-end" | "flex-end" | "flow-end" => AusrLage::Ende,
            "center" => AusrLage::Mitte,
            "left" => AusrLage::Links,
            "right" => AusrLage::Rechts,
            "space-between" => AusrLage::Zwischen,
            "space-around" => AusrLage::Ringsum,
            "space-evenly" => AusrLage::Gleichmaessig,
            "auto" => AusrLage::Auto,
            "baseline" => AusrLage::Grundlinie,
            "legacy" => AusrLage::Normal,
            _ => return None,
        })
    };
    let mut a = Ausr::NORMAL;
    match w.as_slice() {
        [ein] => {
            a.lage = lage_von(ein)?;
            if a.lage == AusrLage::Auto && art != AusrArt::Selbst {
                return None;
            }
        }
        ["first", "baseline"] | ["baseline", "first"] => a.lage = AusrLage::Grundlinie,
        ["last", "baseline"] | ["baseline", "last"] => a.lage = AusrLage::LetzteGrundlinie,
        ["legacy", r] | [r, "legacy"] => {
            a.lage = lage_von(r)?;
        }
        [s, p] if *s == "safe" || *s == "unsafe" => {
            a.lage = lage_von(p)?;
            if matches!(a.lage, AusrLage::Normal | AusrLage::Strecken | AusrLage::Auto | AusrLage::Grundlinie) {
                return None;
            }
            a.sicher = Some(*s == "safe");
        }
        [d, s, p] if (*s == "safe" || *s == "unsafe") && art == AusrArt::Inhalt => {
            // `space-between safe center` gibt es nicht; dies ist die
            // Ausweichform `<content-distribution> || <overflow-position>
            // <content-position>`, selten — lesen wir grosszuegig.
            a.lage = lage_von(d)?;
            a.ausweich = lage_von(p);
            a.sicher = Some(*s == "safe");
        }
        _ => return None,
    }
    if art != AusrArt::Inhalt && matches!(a.lage, AusrLage::Zwischen | AusrLage::Ringsum | AusrLage::Gleichmaessig) {
        return None;
    }
    a.flex_bezug = w.iter().any(|x| matches!(*x, "flex-start" | "flex-end" | "flow-start" | "flow-end"));
    a.selbst_bezug = w.iter().any(|x| matches!(*x, "self-start" | "self-end"));
    // Release 11.10.2026: bei `*-content` markiert `flex_bezug` UND
    // `selbst_bezug` die Werte `flow-start`/`flow-end` (bezogen auf die
    // Flussrichtung; im Masonry mit `track-reverse` vertauscht, CSS Grid 3).
    if art == AusrArt::Inhalt && w.iter().any(|x| matches!(*x, "flow-start" | "flow-end")) {
        a.selbst_bezug = true;
    }
    Some(a)
}

/// Release 11.10.2026: setzt die Rasterwerte (`Berechnet::gitter`) fuer eine
/// gewonnene Deklaration. Laeuft VOR dem grossen `match` der Kaskade und
/// liest den Rohtext (die alte Form wirft dort `safe`, `left`, `normal`,
/// Namen und Prozentluecken weg).
pub(crate) fn kaskade_setzen(
    e: crate::typen::Eigenschaft,
    roh: &str,
    w: &crate::typen::Wert,
    ctx: &RechenCtx,
    erg: &mut crate::typen::Berechnet,
) {
    use crate::typen::Eigenschaft as E;
    let roh = roh.trim();
    let text = match w {
        crate::typen::Wert::Wort(x) => x.as_str(),
        _ => roh,
    };
    // Die Groessen-Schluesselwoerter (CSS Sizing 3 §2.1).
    let art = |t: &str| -> u8 {
        match t.trim().to_ascii_lowercase().as_str() {
            "min-content" | "-moz-min-content" | "-webkit-min-content" => 1,
            "max-content" | "-moz-max-content" | "-webkit-max-content" => 2,
            "fit-content" | "-moz-fit-content" | "-webkit-fit-content" => 3,
            // Release 14.10.2026: `fit-content(<length-percentage>)` as well.
            x if x.starts_with("fit-content(") => 3,
            _ => 0,
        }
    };
    match e {
        E::Breite => erg.groessen_art[0] = art(roh),
        E::MinBreite => erg.groessen_art[1] = art(roh),
        E::MaxBreite => erg.groessen_art[2] = art(roh),
        E::Hoehe => erg.groessen_art[3] = art(roh),
        _ => {}
    }
    // Release 14.10.2026: `stretch` and its legacy aliases (CSS Sizing 4
    // §3.1) as a bit mask (`Berechnet::stretch_bits`). MEASURED before: WPT
    // css-sizing `stretch/stretch-block-size-001` (`block-size: stretch`
    // stayed `auto`).
    if let Some(bit) = stretch_bit(e) {
        if matches!(roh.trim().to_ascii_lowercase().as_str(), "stretch" | "-webkit-fill-available" | "-moz-available") {
            erg.stretch_bits |= bit;
        } else {
            erg.stretch_bits &= !bit;
        }
    }
    // Release 14.10.2026: `min-height`/`max-height` as a content keyword.
    // MEASURED before: WPT css-sizing `block-size-with-min-or-max-content-2`
    // (`height: 0; min-height: max-content` around 100 px stayed 0).
    if let Some(bit) = match e {
        E::MinHoehe => Some(1u8),
        E::MaxHoehe => Some(2u8),
        _ => None,
    } {
        if art(roh) != 0 {
            erg.height_keyword_limits |= bit;
        } else {
            erg.height_keyword_limits &= !bit;
        }
    }
    // Release 14.10.2026: the argument of `fit-content(<length-percentage>)`
    // (CSS Sizing 3 §2.1). MEASURED before: WPT css-sizing
    // `fit-content-length-percentage-001` (`width: fit-content(100px)` around
    // 120 px of content was 120 px wide).
    if let Some(k) = match e {
        E::Breite => Some(0),
        E::MinBreite => Some(1),
        E::MaxBreite => Some(2),
        _ => None,
    } {
        let klein = roh.trim().to_ascii_lowercase();
        erg.fit_content_arg[k] = klein
            .strip_prefix("fit-content(")
            .and_then(|r| r.strip_suffix(')'))
            .and_then(|innen| laenge_prozent(innen, ctx, true));
    }
    match e {
        E::RasterSpalten => {
            if let Some(l) = spurliste(text, ctx) {
                erg.gitter_mut().spalten = l;
            }
        }
        E::RasterZeilen => {
            if let Some(l) = spurliste(text, ctx) {
                erg.gitter_mut().zeilen = l;
            }
        }
        E::RasterAutoSpalten => {
            if let Some(l) = spuren_einfach(text, ctx) {
                erg.gitter_mut().auto_spalten = l;
            }
        }
        E::RasterAutoZeilen => {
            if let Some(l) = spuren_einfach(text, ctx) {
                erg.gitter_mut().auto_zeilen = l;
            }
        }
        E::GitterGebiete => {
            erg.gitter_mut().gebiete = gebiete(text).unwrap_or_default();
        }
        E::GitterSpalteStart => {
            if roh.contains('/') {
                if let Some((a, b)) = linien_paar(roh) {
                    let g = erg.gitter_mut();
                    g.platz.spalte_start = a;
                    g.platz.spalte_ende = b;
                }
            } else if let Some(a) = linie(roh) {
                erg.gitter_mut().platz.spalte_start = a;
            }
        }
        E::GitterSpalteEnde => {
            if let Some(a) = linie(roh) {
                erg.gitter_mut().platz.spalte_ende = a;
            }
        }
        E::GitterZeileStart => {
            if roh.contains('/') {
                if let Some((a, b)) = linien_paar(roh) {
                    let g = erg.gitter_mut();
                    g.platz.zeile_start = a;
                    g.platz.zeile_ende = b;
                }
            } else if let Some(a) = linie(roh) {
                erg.gitter_mut().platz.zeile_start = a;
            }
        }
        E::GitterZeileEnde => {
            if let Some(a) = linie(roh) {
                erg.gitter_mut().platz.zeile_ende = a;
            }
        }
        E::GitterGebiet => {
            if let Some(p) = gebiet_kurz(roh) {
                erg.gitter_mut().platz = p;
            }
        }
        E::FlexBuendig => {
            if let Some(a) = ausrichtung(roh, AusrArt::Inhalt) {
                erg.gitter_mut().justify_content = a;
            }
        }
        E::FlexAusrichtung => {
            if let Some(a) = ausrichtung(roh, AusrArt::Elemente) {
                erg.gitter_mut().align_items = a;
            }
        }
        E::JustifyAusrichtung => {
            if let Some(a) = ausrichtung(roh, AusrArt::Elemente) {
                erg.gitter_mut().justify_items = a;
            }
        }
        E::AusrichtungSelbst => {
            if let Some(a) = ausrichtung(roh, AusrArt::Selbst) {
                erg.gitter_mut().align_self = a;
            }
        }
        E::AusrichtungSelbstWaagerecht => {
            if let Some(a) = ausrichtung(roh, AusrArt::Selbst) {
                erg.gitter_mut().justify_self = a;
            }
        }
        E::Luecke => {
            let l = luecke(roh, ctx);
            let g = erg.gitter_mut();
            g.luecke_spalte = l;
            g.luecke_zeile = l;
        }
        E::LueckeSpalte => erg.gitter_mut().luecke_spalte = luecke(roh, ctx),
        E::LueckeZeile => erg.gitter_mut().luecke_zeile = luecke(roh, ctx),
        E::Schluessel => {
            if let crate::typen::Wert::Schluessel(name, x) = w {
                schluessel_setzen(name, x, erg);
            }
        }
        _ => {}
    }
}

/// Release 11.10.2026: die Rasterwerte aus dem Schluessel-Eimer
/// (`align-content`, `place-content`); gerufen von `schluessel_eintragen`.
/// Release 11.10.2026: `flex-line-count` — eine ganze Zahl ab 1 (auch aus
/// `calc()`/`round()`, ein NaN gilt als 1).
pub(crate) fn zeilenzahl(x: &str) -> Option<u32> {
    let k = x.trim().to_ascii_lowercase();
    if let Ok(n) = k.parse::<i64>() {
        return (n >= 1).then(|| n.min(10_000) as u32);
    }
    if k.contains('(') {
        let v = rechne(&k, &RechenCtx::neu(16.0, 0.0, 800.0), 0)?;
        if v.is_nan() {
            return Some(1);
        }
        return Some(v.round().clamp(1.0, 10_000.0) as u32);
    }
    None
}

pub(crate) fn schluessel_setzen(name: &str, x: &str, erg: &mut crate::typen::Berechnet) {
    match name {
        "flex-line-count" => {
            if let Some(n) = zeilenzahl(x) {
                erg.flex_zeilen_min = n;
            }
        }
        "align-content" => {
            if let Some(a) = ausrichtung(x, AusrArt::Inhalt) {
                erg.gitter_mut().align_content = a;
            }
        }
        "place-content" => {
            // `<align-content> <justify-content>?` (CSS Box Alignment 3
            // §5.3); ohne zweiten Wert gilt der erste fuer beide.
            let w: Vec<&str> = x.split_whitespace().collect();
            let ganz = ausrichtung(x, AusrArt::Inhalt);
            let (a, j) = match (w.len(), ganz) {
                (_, Some(g)) => (Some(g), Some(g)),
                (2, None) => (ausrichtung(w[0], AusrArt::Inhalt), ausrichtung(w[1], AusrArt::Inhalt)),
                _ => (None, None),
            };
            if let (Some(a), Some(j)) = (a, j) {
                let g = erg.gitter_mut();
                g.align_content = a;
                g.justify_content = j;
            }
        }
        _ => {}
    }
}

/// Release 11.10.2026: ist `wert` fuer die Rastereigenschaft `e` gueltig?
/// (Andere Eigenschaften: immer `true`.) `var()` wird spaeter geprueft.
pub(crate) fn gueltig_fuer(e: crate::typen::Eigenschaft, wert: &str) -> bool {
    use crate::typen::Eigenschaft as E;
    let w = wert.trim();
    if w.contains("var(") || matches!(w, "inherit" | "initial" | "unset" | "revert" | "revert-layer") {
        return true;
    }
    let ctx = RechenCtx::neu(16.0, 0.0, 800.0);
    match e {
        E::RasterSpalten | E::RasterZeilen => {
            // `masonry` (alte Masonry-Syntax) wird als Vorlage geduldet.
            w == "masonry" || spurliste(w, &ctx).is_some()
        }
        E::RasterAutoSpalten | E::RasterAutoZeilen => spuren_einfach(w, &ctx).is_some(),
        E::GitterSpalteStart | E::GitterZeileStart => {
            if w.contains('/') { linien_paar(w).is_some() } else { linie(w).is_some() }
        }
        E::GitterSpalteEnde | E::GitterZeileEnde => linie(w).is_some(),
        E::GitterGebiet => gebiet_kurz(w).is_some(),
        E::GitterGebiete => gebiete(w).is_some(),
        E::GitterAutomatik => {
            let t: Vec<&str> = w.split_whitespace().collect();
            let achsen = t.iter().filter(|x| matches!(**x, "row" | "column")).count();
            let dicht = t.iter().filter(|x| **x == "dense").count();
            !t.is_empty() && achsen <= 1 && dicht <= 1 && achsen + dicht == t.len()
        }
        _ => true,
    }
}

/// Eine Luecke (`normal` = `None`).
fn luecke(roh: &str, ctx: &RechenCtx) -> Option<LaengeProzent> {
    if roh.trim().eq_ignore_ascii_case("normal") {
        return None;
    }
    laenge_prozent(roh, ctx, true)
}

/// Release 14.10.2026: the bit of a sizing property in
/// `Berechnet::stretch_bits`.
fn stretch_bit(e: crate::typen::Eigenschaft) -> Option<u8> {
    use crate::typen::Eigenschaft as E;
    match e {
        E::Breite => Some(1),
        E::MinBreite => Some(2),
        E::MaxBreite => Some(4),
        E::Hoehe => Some(8),
        E::MinHoehe => Some(16),
        E::MaxHoehe => Some(32),
        _ => None,
    }
}

/// Release 11.10.2026: `inherit` fuer die Rasterwerte.
pub(crate) fn kaskade_erben(
    e: crate::typen::Eigenschaft,
    eltern: Option<&crate::typen::Berechnet>,
    erg: &mut crate::typen::Berechnet,
) {
    use crate::typen::Eigenschaft as E;
    let k = match e {
        E::Breite => Some(0),
        E::MinBreite => Some(1),
        E::MaxBreite => Some(2),
        E::Hoehe => Some(3),
        _ => None,
    };
    if let Some(k) = k {
        erg.groessen_art[k] = eltern.map_or(0, |p| p.groessen_art[k]);
    }
    if let Some(bit) = stretch_bit(e) {
        erg.stretch_bits = (erg.stretch_bits & !bit) | eltern.map_or(0, |p| p.stretch_bits & bit);
    }
    if let Some(k) = k.filter(|k| *k < 3) {
        erg.fit_content_arg[k] = eltern.and_then(|p| p.fit_content_arg[k]);
    }
    if let Some(bit) = match e {
        E::MinHoehe => Some(1u8),
        E::MaxHoehe => Some(2u8),
        _ => None,
    } {
        erg.height_keyword_limits = (erg.height_keyword_limits & !bit) | eltern.map_or(0, |p| p.height_keyword_limits & bit);
    }
    if !gehoert_dazu(e) {
        return;
    }
    let p = eltern.and_then(|p| p.gitter.as_deref()).cloned().unwrap_or_default();
    let g = erg.gitter_mut();
    match e {
        E::RasterSpalten => g.spalten = p.spalten,
        E::RasterZeilen => g.zeilen = p.zeilen,
        E::RasterAutoSpalten => g.auto_spalten = p.auto_spalten,
        E::RasterAutoZeilen => g.auto_zeilen = p.auto_zeilen,
        E::GitterGebiete => g.gebiete = p.gebiete,
        E::GitterSpalteStart => g.platz.spalte_start = p.platz.spalte_start,
        E::GitterSpalteEnde => g.platz.spalte_ende = p.platz.spalte_ende,
        E::GitterZeileStart => g.platz.zeile_start = p.platz.zeile_start,
        E::GitterZeileEnde => g.platz.zeile_ende = p.platz.zeile_ende,
        E::GitterGebiet => g.platz = p.platz,
        E::FlexBuendig => g.justify_content = p.justify_content,
        E::FlexAusrichtung => g.align_items = p.align_items,
        E::JustifyAusrichtung => g.justify_items = p.justify_items,
        E::AusrichtungSelbst => g.align_self = p.align_self,
        E::AusrichtungSelbstWaagerecht => g.justify_self = p.justify_self,
        E::Luecke => {
            g.luecke_spalte = p.luecke_spalte;
            g.luecke_zeile = p.luecke_zeile;
        }
        E::LueckeSpalte => g.luecke_spalte = p.luecke_spalte,
        E::LueckeZeile => g.luecke_zeile = p.luecke_zeile,
        _ => {}
    }
}

/// Release 11.10.2026: `initial` fuer die Rasterwerte.
pub(crate) fn kaskade_anfang(e: crate::typen::Eigenschaft, erg: &mut crate::typen::Berechnet) {
    use crate::typen::Eigenschaft as E;
    match e {
        E::Breite => erg.groessen_art[0] = 0,
        E::MinBreite => erg.groessen_art[1] = 0,
        E::MaxBreite => erg.groessen_art[2] = 0,
        E::Hoehe => erg.groessen_art[3] = 0,
        _ => {}
    }
    if let Some(bit) = stretch_bit(e) {
        erg.stretch_bits &= !bit;
    }
    match e {
        E::Breite => erg.fit_content_arg[0] = None,
        E::MinBreite => erg.fit_content_arg[1] = None,
        E::MaxBreite => erg.fit_content_arg[2] = None,
        E::MinHoehe => erg.height_keyword_limits &= !1,
        E::MaxHoehe => erg.height_keyword_limits &= !2,
        _ => {}
    }
    if !gehoert_dazu(e) || erg.gitter.is_none() {
        return;
    }
    let g = erg.gitter_mut();
    let n = Ausr::NORMAL;
    match e {
        E::RasterSpalten => g.spalten = SpurListe::default(),
        E::RasterZeilen => g.zeilen = SpurListe::default(),
        E::RasterAutoSpalten => g.auto_spalten.clear(),
        E::RasterAutoZeilen => g.auto_zeilen.clear(),
        E::GitterGebiete => g.gebiete.clear(),
        E::GitterSpalteStart => g.platz.spalte_start = GitterLinie::Auto,
        E::GitterSpalteEnde => g.platz.spalte_ende = GitterLinie::Auto,
        E::GitterZeileStart => g.platz.zeile_start = GitterLinie::Auto,
        E::GitterZeileEnde => g.platz.zeile_ende = GitterLinie::Auto,
        E::GitterGebiet => g.platz = GitterPlatz::default(),
        E::FlexBuendig => g.justify_content = n,
        E::FlexAusrichtung => g.align_items = n,
        E::JustifyAusrichtung => g.justify_items = n,
        E::AusrichtungSelbst => g.align_self = Ausr::AUTO,
        E::AusrichtungSelbstWaagerecht => g.justify_self = Ausr::AUTO,
        E::Luecke => {
            g.luecke_spalte = None;
            g.luecke_zeile = None;
        }
        E::LueckeSpalte => g.luecke_spalte = None,
        E::LueckeZeile => g.luecke_zeile = None,
        _ => {}
    }
}

/// Release 11.10.2026: zerlegt einen Wert in Stuecke wie [`stuecke`], dazu
/// Zeichenketten (`"a b"`) als eigene Stuecke.
fn stuecke_mit_text(s: &str) -> Option<Vec<String>> {
    let mut raus = Vec::new();
    let mut akt = String::new();
    let mut zeichen = s.chars().peekable();
    while let Some(c) = zeichen.next() {
        if c == '"' || c == '\'' {
            if !akt.trim().is_empty() {
                raus.extend(stuecke(&akt)?);
            }
            akt.clear();
            let mut t = String::from(c);
            let mut zu = false;
            for d in zeichen.by_ref() {
                t.push(d);
                if d == c {
                    zu = true;
                    break;
                }
            }
            if !zu {
                return None;
            }
            raus.push(t);
        } else {
            akt.push(c);
        }
    }
    if !akt.trim().is_empty() {
        raus.extend(stuecke(&akt)?);
    }
    Some(raus)
}

/// Die Stelle eines `/` auf oberster Ebene (nicht in Klammern/Texten).
fn schraeg_oben(s: &str) -> Option<usize> {
    let mut tiefe = 0i32;
    let mut text: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (text, c) {
            (Some(q), c) if c == q => text = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => text = Some(c),
            (None, '(' | '[') => tiefe += 1,
            (None, ')' | ']') => tiefe -= 1,
            (None, '/') if tiefe == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Release 11.10.2026: ist der Wert der Kurzform `grid-template` gueltig
/// (CSS Grid 1 §7.4: `none | <rows> / <columns> | [ <line-names>? <string>
/// <track-size>? <line-names>? ]+ [ / <explicit-track-list> ]?`)?
/// GEMESSEN vorher: WPT `parsing/grid-template-shorthand-invalid` (55 von 66
/// ungueltigen Werten wurden gesetzt).
pub(crate) fn vorlage_kurz_gueltig(wert: &str) -> bool {
    let k = wert.trim();
    if k.contains("var(") || k.eq_ignore_ascii_case("none") {
        return true;
    }
    let ctx = RechenCtx::neu(16.0, 0.0, 800.0);
    let (links, rechts) = match schraeg_oben(k) {
        Some(i) => (&k[..i], Some(&k[i + 1..])),
        None => (k, None),
    };
    let Some(teile) = stuecke_mit_text(links) else { return false };
    let mit_text = teile.iter().any(|t| t.starts_with('"') || t.starts_with('\''));
    if !mit_text {
        return match rechts {
            Some(r) => spurliste(links, &ctx).is_some() && spurliste(r, &ctx).is_some(),
            None => false,
        };
    }
    // Die Gebiets-Form: je Zeile Namen? Text Spur? Namen?
    let mut i = 0usize;
    let mut texte: Vec<String> = Vec::new();
    while i < teile.len() {
        if teile[i].starts_with('[') {
            if namen(&teile[i]).is_none() {
                return false;
            }
            i += 1;
        }
        let Some(t) = teile.get(i) else { return false };
        if !(t.starts_with('"') || t.starts_with('\'')) {
            return false;
        }
        texte.push(t.clone());
        i += 1;
        if let Some(t) = teile.get(i)
            && !t.starts_with('[')
            && !t.starts_with('"')
            && !t.starts_with('\'')
        {
            if funktion(t, "repeat").is_some() || spur(t, &ctx).is_none() {
                return false;
            }
            i += 1;
        }
        if let Some(t) = teile.get(i)
            && t.starts_with('[')
        {
            if namen(t).is_none() {
                return false;
            }
            i += 1;
        }
    }
    if gebiete(&texte.join(" ")).is_none() {
        return false;
    }
    match rechts {
        None => true,
        Some(r) => {
            let r = r.trim();
            match spurliste(r, &ctx) {
                Some(l) => !l.subgrid && !l.hat_auto_wiederholung() && !r.eq_ignore_ascii_case("none"),
                None => false,
            }
        }
    }
}

/// Release 11.10.2026: ist der Wert der Kurzform `grid` gueltig (CSS Grid 1
/// §7.8: `<'grid-template'> | <rows> / [ auto-flow && dense? ] <auto-columns>?
/// | [ auto-flow && dense? ] <auto-rows>? / <columns>`)? GEMESSEN vorher: WPT
/// `parsing/grid-shorthand-invalid` (34 ungueltige Werte gesetzt).
pub(crate) fn grid_kurz_gueltig(wert: &str) -> bool {
    let k = wert.trim();
    if k.contains("var(") {
        return true;
    }
    let klein = k.to_ascii_lowercase();
    if !klein.split_whitespace().any(|w| w == "auto-flow") {
        return vorlage_kurz_gueltig(k);
    }
    let ctx = RechenCtx::neu(16.0, 0.0, 800.0);
    let Some(i) = schraeg_oben(&klein) else { return false };
    let (links, rechts) = (&klein[..i], &klein[i + 1..]);
    let fluss = |t: &str| t.split_whitespace().any(|w| w == "auto-flow");
    // Die Fluss-Seite: `auto-flow` (einmal), `dense` (hoechstens einmal),
    // dann hoechstens die impliziten Spuren.
    let fluss_seite = |t: &str| -> bool {
        let mut fl = 0;
        let mut di = 0;
        let mut rest: Vec<&str> = Vec::new();
        for w in t.split_whitespace() {
            match w {
                "auto-flow" if rest.is_empty() => fl += 1,
                "dense" if rest.is_empty() => di += 1,
                _ => rest.push(w),
            }
        }
        fl == 1 && di <= 1 && (rest.is_empty() || spuren_einfach(&rest.join(" "), &ctx).is_some())
    };
    match (fluss(links), fluss(rechts)) {
        (true, false) => fluss_seite(links) && spurliste(rechts, &ctx).is_some(),
        (false, true) => fluss_seite(rechts) && spurliste(links, &ctx).is_some(),
        _ => false,
    }
}

/// Prueft `gueltig_fuer` die Eigenschaft vollstaendig (auch mit `calc()`)?
pub(crate) fn prueft_selbst(e: crate::typen::Eigenschaft) -> bool {
    use crate::typen::Eigenschaft as E;
    matches!(
        e,
        E::RasterSpalten
            | E::RasterZeilen
            | E::RasterAutoSpalten
            | E::RasterAutoZeilen
            | E::GitterSpalteStart
            | E::GitterSpalteEnde
            | E::GitterZeileStart
            | E::GitterZeileEnde
            | E::GitterGebiet
            | E::GitterGebiete
            | E::GitterAutomatik
    )
}

/// Betrifft die Eigenschaft die Rasterwerte?
fn gehoert_dazu(e: crate::typen::Eigenschaft) -> bool {
    use crate::typen::Eigenschaft as E;
    matches!(
        e,
        E::RasterSpalten
            | E::RasterZeilen
            | E::RasterAutoSpalten
            | E::RasterAutoZeilen
            | E::GitterGebiete
            | E::GitterSpalteStart
            | E::GitterSpalteEnde
            | E::GitterZeileStart
            | E::GitterZeileEnde
            | E::GitterGebiet
            | E::FlexBuendig
            | E::FlexAusrichtung
            | E::JustifyAusrichtung
            | E::AusrichtungSelbst
            | E::AusrichtungSelbstWaagerecht
            | E::Luecke
            | E::LueckeSpalte
            | E::LueckeZeile
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RechenCtx {
        RechenCtx::neu(16.0, 0.0, 800.0)
    }

    #[test]
    fn spurliste_mit_namen_und_wiederholung() {
        let l = spurliste("[a] 10px repeat(2, [b] 1fr) [c d] minmax(min-content, 20%)", &ctx()).unwrap();
        let spuren = l.teile.iter().filter(|t| matches!(t, SpurTeil::Spur(_))).count();
        assert_eq!(spuren, 4);
        assert!(matches!(&l.teile[0], SpurTeil::Namen(n) if n == &["a"]));
    }

    #[test]
    fn auto_fill_braucht_feste_spuren() {
        assert!(spurliste("repeat(auto-fill, 1fr)", &ctx()).is_none());
        // CSS Grid 3 (vorlaeufig): intrinsische Spuren in der Wiederholung.
        assert!(spurliste("repeat(auto-fill, auto)", &ctx()).is_some());
        assert!(spurliste("repeat(auto-fill, minmax(10px, 1fr))", &ctx()).is_some());
        assert!(spurliste("repeat(auto-fill, 10px) repeat(auto-fit, 10px)", &ctx()).is_none());
    }

    #[test]
    fn linien() {
        assert_eq!(linie("span 2"), Some(GitterLinie::Spanne(2, None)));
        assert_eq!(linie("foo 3"), Some(GitterLinie::Zahl(3, Some("foo".into()))));
        assert_eq!(linie("0"), None);
        assert_eq!(linie("span 0"), None);
        assert_eq!(linie("foo"), Some(GitterLinie::Name("foo".into())));
    }

    #[test]
    fn gebiete_muessen_rechteckig_sein() {
        assert!(gebiete("\"a a\" \"b a\"").is_none());
        assert_eq!(gebiete("\"a . b\"").unwrap()[0], vec!["a".to_string(), String::new(), "b".to_string()]);
        assert_eq!(gebiete("'a...b'").unwrap()[0].len(), 3);
    }

    #[test]
    fn prozentanteil_aus_calc() {
        let l = laenge_prozent("calc(10px + 20%)", &ctx(), true).unwrap();
        assert!((l.px - 10.0).abs() < 1e-3 && (l.prozent - 20.0).abs() < 1e-3 && l.hat_prozent);
    }
}
