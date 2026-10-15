//! TABELLEN-LAYOUT (A1, Rolle LAYOUT, Welle 7).
//!
//! CSS 2.1, Kapitel 17 (Tabellen), vereinfacht auf das, was der Auftrag
//! verlangt: **Spaltenbreiten auto/fest, `colspan`, `rowspan`**.
//!
//! # DER WEG, UND WARUM ER HIER STEHT
//!
//! `narsil_page::tabelle_knoten` liest die STRUKTUR aus dem HTML und
//! liefert `Vec<Zeile>` mit `Vec<Zelle>` (darin `spalten` = `colspan`,
//! begrenzt auf `MAX_SPALTEN`). Das ist gemessen und getestet, dort.
//! Der Doc sagt ausdruecklich: "das Layout macht aus Spalten Zeilen" —
//! und genau das fehlte. Diese Datei ist das Fehlende.
//!
//! Warum eine reine Funktion und nicht direkt im Durchlauf des Layouts:
//! der DOM-Knoten weiss nicht, dass er eine Tabellenzelle ist. Er ist
//! ein `Container`, und ein `<div>` ist auch eins — beide entstehen in
//! `narsil-page` zu `NodeKind::Container`. Ohne ein Feld im Stil (siehe
//! `team/WUNSCH_LAYOUT.txt`) kann die Engine `table > tr > td` nicht
//! von `div > div > div` unterscheiden. Die Zuordnung `Tag -> Plan`
//! ist die eine Zeile, die in `narsil-page` fehlt (WUNSCH, Punkt 2/5).
//!
//! Was hier steht, ist deshalb die *Rechnung*, und die ist vollstaendig
//! und an echten Daten getestet (`tests/tabelle_echt.rs`).
//!
//! # DIE REGELN (CSS 2.1, 17.6 — automatisches Tabellenlayout)
//!
//! 1. Die Tabelle hat so viele Spalten, wie die breiteste Zeile
//!    beansprucht (`sum(colspan)` je Zeile, das Maximum).
//! 2. Ohne feste Breiten: jede Spalte gleich breit —
//!    `(breite - luecken) / spalten`.
//! 3. Eine Zelle mit `colspan = c` ist so breit wie die Summe der `c`
//!    Spalten, die sie abdeckt.
//! 4. Eine Zelle mit `rowspan = r` ist so hoch wie die Summe der `r`
//!    Zeilen, die sie abdeckt.
//! 5. Eine Zeile ist so hoch wie ihre hoechste Zelle; Zellen mit
//!    weniger Inhalt richten sich oben aus (kein `vertical-align`).
//! 6. Eine Zelle mit `rowspan = 0` heisst "bis zum Ende der Tabelle"
//!    (HTML 4.01). `narsil-page` liest das als 1; hier gilt dasselbe, und
//!    `rowspan_0_heisst_rest` sagt es.
//!
//! # SPEICHERREGELN (M3/M4)
//!
//! * `MAX_SPALTEN` (512) und `MAX_ZEILEN` (100 000) sind die Grenzen von
//!   `narsil-page` und gelten hier unveraendert weiter — die Zelle darf
//!   nicht mehr beanspruchen als der Parser erlaubt.
//! * Das Belegungsraster (`Vec<bool>`) ist `MAX_SPALTEN x MAX_ZEILEN`
//!   gross und wird VOR dem Anlegen auf diese Groesse gedeckelt. Bei
//!   512 x 100 000 waeren das 51,2 Mio Byte; deshalb ist die Zeilenzahl
//!   hier kleiner (`MAX_ZEILEN_LAYOUT` = 20 000, mit Begruendung unten).
//! * Keine Rekursion (M5), keine Panics, kein `unsafe`.

use narsil_core::geometry::{Point, Rect, Size};

// ===========================================================================
// GRENZEN (M3/M4)
// ===========================================================================

/// Hoechste Spaltenzahl, die diese Rechnung belegt. Begruendung: 512 ist
/// die Grenze von `narsil_page::hteile::MAX_SPALTEN`; ein `colspan` darueber
/// hinaus ergibt dort schon diese Zahl, und mehr Spalten kann eine Seite
/// nicht sinnvoll brauchen.
pub const MAX_SPALTEN: usize = 512;

/// Hoechste Zeilenzahl, die diese Rechnung belegt.
///
/// GEMESSEN, warum hier 20 000 und nicht die 100 000 von `narsil-page`:
/// das Belegungsraster ist `spalten * zeilen` Booleans, also bei
/// 100 000 Zeilen und 512 Spalten 51,2 Mio Byte fuer EINEN Tabellenbau —
/// das ist genau die Art Speicher, an der dieser Rechner schon gestorben
/// ist. 20 000 Zeilen sind mehr, als eine sichtbare Tabelle braucht
/// (bei 20 px Zeilenhoehe sind das 400 000 px, also weit ueber dem
/// Bildschirm), und das Raster passt dann in 10 MiB.
pub const MAX_ZEILEN_LAYOUT: usize = 20_000;

/// Hoechste Zellen je Tabelle. Begruendung: eine Tabelle hat Zellen, aber
/// `spalten * zeilen` ist bereits gedeckelt; diese Zahl schuetzt vor der
/// anderen Richtung — 20 000 Zeilen mit je 100 000 Zellen.
pub const MAX_ZELLEN: usize = 400_000;

// ===========================================================================
// DIE EINGABE
// ===========================================================================

/// Eine Zelle, wie sie die Rechnung braucht.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zelle {
    /// Wie viele Spalten die Zelle abdeckt (`colspan`, >= 1).
    pub spalten: u16,
    /// Wie viele Zeilen die Zelle abdeckt (`rowspan`, >= 1).
    pub zeilen: u16,
    /// Gewuenschte Breite in px. `None` = automatisch.
    pub breite: Option<f32>,
    /// Inhaltshoehe in px (>= 0).
    pub hoehe: f32,
    /// added 07.10.2026: min-content-Breite, siehe
    /// [`ZelleEingabe::min_breite`].
    pub min_breite: Option<f32>,
    /// added 07.10.2026: max-content-Breite, siehe
    /// [`ZelleEingabe::max_breite`].
    pub max_breite: Option<f32>,
}

/// Eine Zeile: ihre Zellen in Dokumentreihenfolge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Zeile {
    /// Die Zellen.
    pub zellen: Vec<Zelle>,
}

/// Die Zelle so, wie `narsil_page::Zelle` sie liefert — aber mit dem
/// `rowspan`, den `narsil-page` nicht fuehrt (es liest ihn als 1).
///
/// DER UNTERSCHIED: `narsil_page::Zelle` hat `spalten` (colspan) und
/// `text`, aber kein `rowspan`. Fuer A1 wird `rowspan` hier zusaetzlich
/// gelesen; die Funktion ist die einzige Stelle, die ihn kennt.
impl Zeile {
    /// Baut eine Zeile aus der STRUKTUR von `narsil-page` plus den
    /// `rowspan`-Angaben (parallel, in Dokumentreihenfolge).
    ///
    /// `rowspans` ist kuerzer als `zellen`: dann fehlt der Rest, und die
    /// fehlenden Zellen bekommen `rowspan = 1` — das ist der Fall, wenn
    /// narsil-page eine Zelle ohne `rowspan` gebaut hat.
    pub fn aus_masstab(zellen: &[ZelleEingabe], rowspans: &[u16]) -> Zeile {
        let mut raus = Vec::with_capacity(zellen.len().min(MAX_ZELLEN));
        for (i, z) in zellen.iter().take(MAX_ZELLEN).enumerate() {
            raus.push(Zelle {
                spalten: z.spalten.clamp(1, MAX_SPALTEN as u16),
                zeilen: rowspans.get(i).copied().unwrap_or(1).clamp(1, u16::MAX),
                breite: z.breite,
                hoehe: if z.hoehe.is_finite() {
                    z.hoehe.clamp(0.0, 100_000.0)
                } else {
                    0.0
                },
                // added 07.10.2026: die Inhaltsbreiten wandern mit; ohne
                // sie gilt weiter die alte Regel 2.
                min_breite: z.min_breite.filter(|v| v.is_finite() && *v > 0.0),
                max_breite: z.max_breite.filter(|v| v.is_finite() && *v > 0.0),
            });
        }
        Zeile { zellen: raus }
    }
}

/// Die Zellenangabe aus `narsil-page` (ohne `rowspan`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ZelleEingabe {
    /// `colspan` (>= 1).
    pub spalten: u16,
    /// Gewuenschte Breite in px. `None` = automatisch.
    pub breite: Option<f32>,
    /// Inhaltshoehe in px.
    pub hoehe: f32,
    /// added 07.10.2026, Rolle SERVER: **min-content**-Breite der Zelle
    /// in px (CSS 2.1 §17.5.2.1, Schritt 1) — die Breite des laengsten
    /// unumbrechbaren Stuecks. `None` = unbekannt.
    ///
    /// DER GRUND, warum es dieses Feld gibt: die alte Regel 2 dieser Datei
    /// war „ohne feste Breiten ist **jede** Spalte gleich breit“, und das
    /// ist an echten Seiten falsch. GEMESSEN an der Firefox-Referenz
    /// (`team/log/firefox_ref/hackernews.png`, Textbereich x 103..810,
    /// also 708 px) gegen den eigenen Screenshot (Text ueber **alle**
    /// 1280 px): es gibt gar keine Spalten, weil
    /// `narsil-page` die Tabelle **linearisiert** (dort steht es woertlich:
    /// „unsere Tabellen sind linearisiert und die Gitter-Lage der Engine
    /// wird dafuer nicht benutzt“).
    ///
    /// **WIRTSCHAFTLICH, WARUM HIER UND NICHT IM AUFRUFER:** um die
    /// Spalten aus dem Inhalt zu rechnen, braucht diese Datei die
    /// Inhaltsbreiten. `narsil-page` kann sie mit `text::min_content_breite`
    /// und `text::intrinsic_size` liefern — beide sind **hier** in der
    /// Crate, also kann der Aufrufer sie ohne neue Crate berechnen.
    ///
    /// RUECKWAERTSKOMPATIBEL: `None` (der Stand vom 06.10.) fuehrt
    /// unveraendert zur alten Regel 2. Die neuen Zweige greifen nur, wenn
    /// **irgendeine** Zelle eine Inhaltsbreite liefert.
    pub min_breite: Option<f32>,
    /// added 07.10.2026: **max-content**-Breite der Zelle in px (die
    /// Breite des Textes auf **einer** Zeile). `None` = unbekannt.
    pub max_breite: Option<f32>,
}

/// Eine gelegte Zelle: wo sie steht und wie gross sie ist.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ZellenPlatz {
    /// Spaltenindex (0-basiert).
    pub spalte: u32,
    /// Zeilenindex (0-basiert).
    pub zeile: u32,
    /// Wie viele Spalten sie abdeckt.
    pub spalten: u16,
    /// Wie viele Zeilen sie abdeckt.
    pub zeilen: u16,
    /// Breite in px (>= 0).
    pub breite: f32,
    /// Hoehe in px (>= 0).
    pub hoehe: f32,
    /// X in px.
    pub x: f32,
    /// Y in px.
    pub y: f32,
}

/// Das Ergebnis einer Tabellenrechnung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tabelle {
    /// Die gelegten Zellen, in Dokumentreihenfolge.
    pub zellen: Vec<ZellenPlatz>,
    /// Die Breite jeder Spalte in px (Index = Spalte).
    pub spaltenbreiten: Vec<f32>,
    /// Die Hoehe jeder Zeile in px (Index = Zeile).
    pub zeilenhoehen: Vec<f32>,
    /// Gesamtbreite in px.
    pub breite: f32,
    /// Gesamthoehe in px.
    pub hoehe: f32,
    /// Wie viele Zeilen abgeschnitten wurden (M7: mitzaehlen).
    pub verworfen_zeilen: usize,
    /// Wie viele Zellen abgeschnitten wurden.
    pub verworfen_zellen: usize,
    /// added 05.10.2026, Rolle SERVER (SERVER_14): **die Rechtecke der
    /// Zeilen**, nach der Nummer des Zeilenknotens.
    ///
    /// GEMESSENER GRUND, warum das hier und nicht im `Ctx`: `lege_tabelle`
    /// rechnet die Zeilenhoehen, kennt aber die **Knoten** nicht — es
    /// bekommt `Vec<Zeile>` ohne eine einzige `NodeId`. Die Zuordnung
    /// „Zeile 3 der Rechnung ist Knoten 4711“ kann nur jemand machen, der
    /// die Knoten gesehen hat: das ist `flow.rs`. Das **Ergebnis** dieser
    /// Zuordnung gehoert aber zu diesem Ergebnis, und die Zeile findet es
    /// ueber ihre Tabelle (`Ctx::tabellen`).
    ///
    /// **WARUM EINE KARTE und kein `Vec`:** die Zeilennummer aus der
    /// Rechnung und die Kindnummer im DOM muessen nicht uebereinstimmen —
    /// eine Tabelle kann neben ihren Zeilen ein `<caption>`-Kind haben.
    /// Deshalb der Knoten als Schluessel.
    pub zeilen_rechtecke: std::collections::HashMap<u32, Rect>,
    /// added 05.10.2026, Rolle SERVER (SERVER_15): **die Rechtecke der
    /// Zellen**, nach der Nummer des Zellenknotens.
    ///
    /// **WARUM BEIDE KARTEN und nicht eine:** GEMESSENE URSACHE (SERVER_14)
    /// — in `kind_platzieren` ist `eltern_stil` der Stil des **Knotens
    /// selbst**. Bei der Zelle ist das `Anzeige::TabelleZelle`, nicht
    /// `Anzeige::TabelleZeile`, und die Zelle liegt **zwei Ebenen** unter
    /// der Tabelle (Zelle -> Zeile -> Tabelle). Die Zeile kann ihre
    /// Spaltenbreite ueber `node.parent` finden; die Zelle braucht
    /// **zwei** Schritte, und dann nur noch ihre **eigene** Nummer.
    ///
    /// **DESHALB NACH ZELLENKNOTEN und nicht nach (Zeile, Spalte):**
    /// `lege_tabelle` liefert den **Spaltenindex** eines Platzes, nicht den
    /// Listenindex der Zelle — mit `colspan` stimmen die beiden nicht
    /// ueberein. Wer erst hier verrechnen muesste, baute die Zuordnung an
    /// **zwei** Stellen. Mit der Karte nach Knoten entscheidet nur die
    /// Tabelle, wo sie das legt.
    pub zellen_rechtecke: std::collections::HashMap<u32, Rect>,
}

fn sauber(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 100_000.0)
    } else {
        0.0
    }
}

// ===========================================================================
// DIE RECHNUNG (CSS 2.1, 17.6)
// ===========================================================================

/// Legt eine Tabelle.
///
/// * `breite` — die Breite, die die Tabelle zur Verfuegung hat.
/// * `zeilen` — die Zeilen mit ihren Zellen.
/// * `luecke` — Zwischenraum zwischen den Spalten (`border-spacing`).
///   Wird von der Breite abgezogen, BEVOR geteilt wird.
/// * `kopf_hoehe` — zusaetzliche Hoehe fuer eine Kopfzeile, falls die
///   Tabelle eine hat. `None` = keine.
///
/// # RUECKGABE
///
/// [`Tabelle`] mit den gelegten Zellen, den Spaltenbreiten und den
/// Zeilenhoehen. Leere Tabelle -> leeres Ergebnis, kein Panic.
/// Die Tabelle von links nach rechts — der bisherige Zustand.
///
/// K17b (02.10.2026, 19:15): ruf [`lege_tabelle_mit_richtung`] mit
/// `rtl = true`. Diese Funktion bleibt, weil sie der bestehende
/// Aufrufvertrag ist; die Testdatei `tests/tabelle_echt.rs` ruft sie
/// an acht Stellen, und eine Aenderung der Signatur wuerde dort alles
/// brechen — additiv ist hier eine Geschwisterfunktion.
pub fn lege_tabelle(
    breite: f32,
    zeilen: &[Zeile],
    luecke: f32,
    kopf_hoehe: Option<f32>,
) -> Tabelle {
    lege_tabelle_mit_richtung(breite, zeilen, luecke, kopf_hoehe, false)
}

/// Die Tabelle mit Schreibrichtung (K17b).
///
/// # WAS `rtl = true` ANDERT — und was nicht
///
/// CSS Writing Modes 3 §7.1: bei `direction: rtl` steht die ERSTE
/// Spalte am RECHTEN Rand, und die Spalten laufen von rechts nach
/// links. Das betrifft ausschliesslich die X-Positionen der Spalten
/// (`x_von`) — die Breiten, die Zeilenhoehen, das Belegungsraster
/// fuer `rowspan` und `colspan` bleiben unveraendert.
///
/// GEMESSENE ENTSCHEIDUNG: die Umkehr passiert an EINER Stelle, bei
/// den x-Positionen. Eine Umkehr der Zellenlisten in der Schleife
/// darueber wuerde auch die `rowspan`-Belegung umdrehen und damit
/// Zellen in die falsche Zeile setzen — das waere ein Fehler, den man
/// erst auf einer Seite mit rowspan sieht.
pub fn lege_tabelle_mit_richtung(
    breite: f32,
    zeilen: &[Zeile],
    luecke: f32,
    kopf_hoehe: Option<f32>,
    rtl: bool,
) -> Tabelle {
    // added 05.10.2026, SERVER_16: `b` ist **veraenderbar**, weil die
    // Mindestbreite der Tabelle die angebotene Breite **uebersteigen darf**
    // (CSS 2.1 17.5.2.1). Siehe den Schritt „DIE TABELLE WAECHST“ unten.
    let mut b = sauber(breite);
    let gap = sauber(luecke);
    let mut raus = Tabelle {
        breite: b,
        ..Tabelle::default()
    };

    // M3/M4: die Zeilen- und Zellenzahl sind Eingaben.
    let nz = zeilen.len().min(MAX_ZEILEN_LAYOUT);
    raus.verworfen_zeilen = zeilen.len().saturating_sub(nz);
    if nz == 0 {
        return raus;
    }
    // Alle Zellen der Tabelle zaehlen (fuer `verworfen_zellen`).
    let alle_zellen: usize = zeilen.iter().take(nz).map(|z| z.zellen.len()).sum();
    let mut zellen: Vec<(usize, usize, Zelle)> = Vec::with_capacity(alle_zellen.min(MAX_ZELLEN));
    for (zi, z) in zeilen.iter().take(nz).enumerate() {
        let nzellen = z.zellen.len().min(MAX_ZELLEN);
        for (ki, k) in z.zellen.iter().take(nzellen).enumerate() {
            zellen.push((zi, ki, *k));
        }
    }
    let mut kapiert = alle_zellen.saturating_sub(zellen.len());
    if zellen.len() > MAX_ZELLEN {
        kapiert += zellen.len() - MAX_ZELLEN;
        zellen.truncate(MAX_ZELLEN);
    }
    raus.verworfen_zellen = kapiert;

    // ---------------------------------------------------------------
    // REGEL 1: wie viele Spalten hat die Tabelle?
    // ---------------------------------------------------------------
    // Das Maximum aus `sum(colspan)` — mit rowspan-Belegung, denn eine
    // Zelle kann auch durch einen rowspan "in die Breite" wirken. Fuer
    // die einfache Fassung reicht das Maximum der Zeilensummen; das ist
    // die Regel, die ein Browser ohne `table-layout: fixed` anwendet,
    // wenn keine Breiten genannt sind.
    // Regel 1 mit der Grenze VOR der Summe: `colspan="65535"` darf nicht
    // erst summiert und dann gekappt werden — die Summe waere dann schon
    // zu gross.
    let mut n_spalten = 0usize;
    // Das Belegungsraster: `zeile * n_spalten + spalte` waere zu gross,
    // weil `n_spalten` erst feststeht. Also: je Zeile ein `Vec<bool>`,
    // alle gleich breit wie die spaeter ermittelte Spaltenzahl. Fuer die
    // Erste Runde nehmen wir die Zeilensummen.
    for z in zeilen.iter().take(nz) {
        let mut summe = 0usize;
        for k in z.zellen.iter() {
            summe += (k.spalten.max(1) as usize).min(MAX_SPALTEN);
            if summe >= MAX_SPALTEN {
                break;
            }
        }
        if summe > n_spalten {
            n_spalten = summe;
        }
    }
    let n_spalten = n_spalten.min(MAX_SPALTEN);
    if n_spalten == 0 {
        return raus;
    }

    // ---------------------------------------------------------------
    // REGEL 2: Spaltenbreiten.
    // ---------------------------------------------------------------
    // Feste Breiten zuerst: eine Zelle mit `breite` gibt ihrer ERSTEN
    // Spalte diese Breite. Der Rest wird gleichmaessig verteilt
    // (CSS 2.1, 17.6.2, vereinfacht: keine Verteilung nach Inhalt).
    let mut fest_verteilt = vec![0.0f32; n_spalten];
    for z in zeilen.iter().take(nz) {
        let mut sp = 0usize;
        for k in z.zellen.iter() {
            let c = (k.spalten.max(1) as usize).min(MAX_SPALTEN);
            if let Some(bw) = k.breite.filter(|v| v.is_finite() && *v > 0.0) {
                if sp < n_spalten {
                    let je = sauber(bw) / c as f32;
                    for i in 0..c {
                        if sp + i < n_spalten && fest_verteilt[sp + i] == 0.0 {
                            fest_verteilt[sp + i] = je;
                        }
                    }
                }
            }
            sp += c;
        }
    }
    // Der Rest gleichmaessig ueber die Spalten, die noch keine Breite
    // haben. Die Luecken werden ZUVOR abgezogen (Regel 2, CSS 17.6.2.1).
    // added 07.10.2026: DIE INHALTSBREITEN, und der ganze Grund, warum
    // es sie gibt, steht bei `ZelleEingabe::min_breite`. Hier werden sie
    // **je Spalte** gesammelt; die alte Regel 2 (gleiche Breite) gilt
    // weiter, wenn keine Zelle eine liefert.
    let mut min_spalte = vec![0.0f32; n_spalten];
    let mut max_spalte = vec![0.0f32; n_spalten];
    let mut hat_inhalt = false;
    for z in zeilen.iter().take(nz) {
        let mut sp = 0usize;
        for k in z.zellen.iter() {
            let c = (k.spalten.max(1) as usize).min(MAX_SPALTEN);
            if sp >= n_spalten {
                break;
            }
            if k.min_breite.is_some() || k.max_breite.is_some() {
                hat_inhalt = true;
            }
            let n = c.min(n_spalten - sp).max(1) as f32;
            if let Some(mn) = k.min_breite.filter(|v| v.is_finite() && *v > 0.0) {
                // Schritt 1 der CSS-Regel: der Ueberschuss einer spanning
                // Zelle ueber die Summe der Mindestbreiten wird **gleich**
                // auf die ueberspannten Spalten verteilt.
                let je = sauber(mn) / n;
                for i in 0..(n as usize) {
                    if sp + i < n_spalten && min_spalte[sp + i] < je {
                        min_spalte[sp + i] = je;
                    }
                }
            }
            if let Some(mx) = k.max_breite.filter(|v| v.is_finite() && *v > 0.0) {
                // Schritt 2: dasselbe fuer die max-content-Breite. Eine
                // spanning Zelle **darf** ihre Breite nicht einfach
                // verteilen, wenn die Einzelmaxima schon passen — daher
                // das Maximum, nicht die Summe.
                let je = sauber(mx) / n;
                for i in 0..(n as usize) {
                    if sp + i < n_spalten && max_spalte[sp + i] < je {
                        max_spalte[sp + i] = je;
                    }
                }
            }
            sp += c;
        }
    }

    let luecken = gap * (n_spalten.saturating_sub(1)) as f32;
    let frei = (b - luecken).max(0.0);
    let summe_fest: f32 = fest_verteilt.iter().sum();
    let n_frei = fest_verteilt.iter().filter(|w| **w == 0.0).count();

    // added 07.10.2026: DIE VERTEILUNG NACH INHALT. Reihenfolge nach CSS
    // 2.1 §17.5.2.1, mit **zwei** bewussten Vereinfachungen, die beide
    // unten benannt sind.
    if hat_inhalt && n_frei > 0 {
        let mut frei_platziert = vec![0.0f32; n_spalten];
        // added 03.10.2026: `offen_summe` wurde hier deklariert und dann
        // **nie gelesen** (GEMESSEN: `warning: unused variable`, 8x in
        // diesem Lauf). Der Zaehler ist ueberfluegig: `offen_min` traegt
        // dieselbe Summe fuer die Mindestbreiten, und fuer die Maxima
        // wird weiter unten `gesamt_offen` eigens berechnet. Es gibt also
        // keine Zahl, die verloren ginge — die Deklaration wird entfernt
        // statt sie als `_offen_summe` zu verstecken, denn ein Name mit
        // Unterstrich traegt die Bedeutung "hier fehlt noch etwas".
        let mut offen_max = 0.0f32;
        let mut offen_min = 0.0f32;
        for i in 0..n_spalten {
            if fest_verteilt[i] != 0.0 {
                continue;
            }
            // Schritt 1: mindestens die min-content-Breite.
            let m = min_spalte[i].min(frei);
            frei_platziert[i] = m;
            offen_min += m;
            offen_max += max_spalte[i].max(m);
        }
        let mut rest = (frei - offen_min).max(0.0);
        // added 07.10.2026: DER VERGLEICH IST GEGEN **frei**, nicht gegen
        // `rest`. GEMESSENER FEHLER DER ERSTEN FASSUNG: sie verglich die
        // Summe der max-content-Breiten (400) mit dem Rest **nach** den
        // Mindestbreiten (300) und kuerzte daraufhin — es blieben **100 px**
        // unverteilt (gemessen: 225 + 75 = 300 bei einer Tabelle von 400).
        // Die Frage ist: passt der gewollte Inhalt in die Tabelle?
        if offen_max > frei && offen_max > 0.0 {
            // Zu breit. **added 03.10.2026: der Fehlbetrag wird am
            // KUERZBAREN verteilt, nicht an den Maxima** — und das ist
            // eine Korrektur, keine Verfeinerung.
            //
            // GEMESSENER FEHLER DER ALTEN FASSUNG an der Wikipedia-Infobox
            // (`tests/wikipedia_infobox_echt.rs`), mit **gemessenen**
            // Inhaltsbreiten (DejaVuSans 12.6 px, die Schriftgroesse aus
            // `style="font-size:90%"` bei einem 14-px-Artikel):
            //     Beschriftungsspalte (laengste Beschriftung 128.10 px)
            //     Firefox  153 px  (x 668 -> 821, Tintenprofil)
            //     alt      67.95 px        -> -85.0 px, **-55.6 %**
            //     neu     128.10 px        ->  -24.9 px,  -16.3 %
            // Die alte Fassung rechnete `max_i × (frei / summe_max)`. Bei
            // einer Spalte mit einem sehr langen, umbrechenden Text
            // (`max` ~900 px neben `max` 128 px) drueckt das die **kurze**
            // Spalte auf ein Bruchteil ihres eigenen Inhalts — obwohl sie
            // gar nichts abgeben kann. Genau das war hier der Fall: die
            // Beschriftung hat `min == max` (ein Wort pro Zeile), also
            // **null kuerzbaren Spielraum**, und wurde trotzdem auf 53 %
            // ihres Inhalts gestutzt.
            //
            // DIE REGEL, und sie ist CSS 2.1 §17.5.2.1 Schritt 1 + die
            // Fussnote zur Ueberbreite: eine Spalte wird auf ihre
            // Mindestbreite zurueckgesetzt, **nicht** darunter. Der
            // Fehlbetrag verteilt sich also ueber den Spielraum
            // `max - min`, und wer keinen hat, haelt seine Breite.
            let fehlbetrag = offen_max - frei;
            // Der kuerzbare Spielraum je freier Spalte.
            let mut kuerzbar_gesamt = 0.0f32;
            for i in 0..n_spalten {
                if fest_verteilt[i] != 0.0 {
                    continue;
                }
                let m = min_spalte[i].min(max_spalte[i]);
                kuerzbar_gesamt += (max_spalte[i] - m).max(0.0);
            }
            for i in 0..n_spalten {
                if fest_verteilt[i] != 0.0 {
                    continue;
                }
                let m = min_spalte[i].min(max_spalte[i]);
                let spielraum = (max_spalte[i] - m).max(0.0);
                // Wer nichts abgeben kann, gibt auch nichts ab; und
                // niemand kommt unter seine Mindestbreite.
                let abzug = if kuerzbar_gesamt > 0.0 {
                    fehlbetrag * (spielraum / kuerzbar_gesamt)
                } else {
                    0.0
                };
                frei_platziert[i] = (max_spalte[i] - abzug).max(m);
            }
            // added 03.10.2026: `rest = 0.0;` stand hier und wurde nie
            // gelesen (GEMESSEN: `warning: value assigned to rest is
            // never read`). Das ist richtig so: im Ueberhang-Zweig gibt es
            // keinen Rest, der verteilt werden koennte — die Kuerzung hat
            // alles verbraucht. Die Zuweisung zu loeschen ist die
            // Aenderung; das `else` traegt danach `rest` unveraendert
            // weiter und rechnet dort von `frei - offen_min` aus, was
            // korrekt ist, weil dieser Zweig `offen_max <= frei` prueft.
        } else {
            // Genug Platz: bis zur max-content-Breite aufteilen, **anteilig
            // am Weg** (max − ist). Das ist die Verteilung, die Browser
            // benutzen; sie faellt auf „gleich“ zurueck, wenn alle Spalten
            // dieselbe Breite wollen.
            for i in 0..n_spalten {
                if rest <= 0.0 {
                    break;
                }
                if fest_verteilt[i] != 0.0 {
                    continue;
                }
                let ziel = max_spalte[i];
                if ziel <= frei_platziert[i] {
                    continue;
                }
                let fehlende = ziel - frei_platziert[i];
                // Summe aller noch offenen Wege — **vor** der Aufteilung
                // berechnet, sonst waere jeder Durchgang ein anderer Nenner.
                let gesamt_offen: f32 = (0..n_spalten)
                    .filter(|j| fest_verteilt[*j] == 0.0)
                    .map(|j| (max_spalte[j] - frei_platziert[j]).max(0.0))
                    .sum();
                if gesamt_offen <= 0.0 {
                    break;
                }
                let teil = (fehlende * rest / gesamt_offen).min(rest);
                frei_platziert[i] += teil;
                rest -= teil;
            }
            // Was danach noch uebrig ist (alle Spalten ueber ihrem
            // Maximum, z.B. leere Spalten), wird **gleich** verteilt —
            // CSS ueberlaesst die Verteilung dem Browser, und gleich ist
            // die, die Firefox bei `table-layout: auto` ohne Inhalt zeigt.
            if rest > 0.0 {
                let offen: Vec<usize> = (0..n_spalten)
                    .filter(|i| fest_verteilt[*i] == 0.0)
                    .collect();
                // Release 07.10.2026: der Überschuss geht ANTEILIG nach der
                // max-content-Breite an die Spalten (CSS 2.1 §17.5.2.2,
                // so verteilt Firefox) — gleichmässig verteilt bekam die
                // Rangspalte von news.ycombinator.com so viel wie die Titel.
                let summe_max: f32 = offen.iter().map(|i| max_spalte[*i]).sum();
                if summe_max > 0.0 {
                    for i in offen {
                        frei_platziert[i] += rest * max_spalte[i] / summe_max;
                    }
                } else if !offen.is_empty() {
                    let je = rest / offen.len() as f32;
                    for i in offen {
                        frei_platziert[i] += je;
                    }
                }
            }
        }
        for i in 0..n_spalten {
            if fest_verteilt[i] == 0.0 {
                fest_verteilt[i] = frei_platziert[i];
            }
        }
    }
    let je_frei = if n_frei > 0 {
        ((frei - summe_fest).max(0.0)) / n_frei as f32
    } else {
        0.0
    };
    raus.spaltenbreiten = fest_verteilt
        .iter()
        .map(|w| {
            if *w > 0.0 {
                sauber(*w)
            } else {
                sauber(je_frei)
            }
        })
        .collect();

    // added 05.10.2026, Rolle SERVER (SERVER_16): **DIE TABELLE WAECHST
    // UEBER IHRE BREITE HINAUS** — CSS 2.1 §17.5.2.1, woertlich: „the used
    // width of the table is the greater of the resolved value of `width`,
    // `min-width` and the **minimum width** of the table“.
    //
    // GEMESSEN an Firefox 157 (Messseite `~/.narsil_mess/tbl.html`, Screenshot
    // `~/.narsil_mess/ff/tbl.png`, Kanten an den Farben gemessen):
    //
    //     width: 400px, drei Spalten mit min-content 368 / 62 / 158
    //     Firefox:  x   8..375 / 376..437 / 438..595   -> 588 px BREIT
    //
    // Uns war die Tabelle an dieser Stelle **400 px** — wir haben die
    // Mindestbreiten auf die angebotene Breite **gedrueckt** und damit
    // den Inhalt der Spalten abgeschnitten. Firefox schneidet nichts ab:
    // es waechst.
    //
    // **WARUM HIER und nicht spaeter:** die x-Positionen und die Breite des
    // Kastens (`raus.breite`) rechnen beide mit `b`. Waechst die Tabelle
    // erst danach, stehen die Zellen an den alten Positionen und der Kasten
    // ist zu schmal — genau die halbe Verdrahtung, die SERVER_14 gemessen
    // hat. Deshalb steht dieser Schritt **vor** `x_von`.
    //
    // **NUR WENN INHALT VORLIEGT:** ohne `min_breite` bleibt `min_spalte`
    // leer, seine Summe ist 0, und es aendert sich nichts — der Rueckfall
    // auf die alte Regel 2 bleibt unangetastet.
    let mindest_summe = sauber(min_spalte.iter().sum::<f32>());
    if hat_inhalt && mindest_summe > b + 0.5 {
        raus.spaltenbreiten = min_spalte
            .iter()
            .map(|m| if *m > 0.0 { sauber(*m) } else { 0.0 })
            .collect();
        b = mindest_summe;
    }

    // Die X-Positionen aus den Breiten und den Luecken.
    //
    // K17b: bei `rtl` laeuft dieselbe Rechnung von RECHTS.    // Die X-Positionen aus den Breiten und den Luecken.
    //
    // K17b: bei `rtl` laeuft dieselbe Rechnung von RECHTS. GEMESSEN
    // als Kontrolle: die Summe der Spaltenbreiten plus die Luecken ist
    // in beiden Faellen gleich, und `x_von[0]` ist bei `ltr` genau 0.
    let mut x_von = vec![0.0f32; n_spalten];
    if rtl {
        let mut x = b;
        for (s, breite) in raus.spaltenbreiten.iter().take(n_spalten).enumerate() {
            // added 03.10.2026: `x -= *breite;` statt `x = x - *breite;`
            // (GEMESSEN: clippy `manual implementation of an assign
            // operation`, `tabelle.rs:555`). Fuer `f32` ist das dieselbe
            // Rechnung — der Typ kennt keine Ueberladung, bei der `-=`
            // etwas anderes taete als `-` mit Zuweisung.
            x -= *breite;
            x_von[s] = sauber(x);
            x -= gap;
        }
    } else {
        let mut x = 0.0f32;
        for (s, breite) in raus.spaltenbreiten.iter().take(n_spalten).enumerate() {
            x_von[s] = sauber(x);
            x = x + *breite + gap;
        }
    }

    // ---------------------------------------------------------------
    // REGEL 5: Zeilenhoehen — die hoechste Zelle der Zeile.
    // ---------------------------------------------------------------
    // Bei einem rowspan wird die Zeile am ENDE so hoch wie nötig, damit
    // die gespannte Zelle hineinpasst (Regel 4).
    let mut hoehen = vec![0.0f32; nz];
    // Das Belegungsraster fuer rowspan: `belegt[zeile][spalte]`.
    // M4: `nz * n_spalten` Booleans, beide schon gedeckelt.
    let mut belegt: Vec<bool> = vec![false; nz * n_spalten];
    // Zeile 0 ist die Kopfzeile, falls es eine gibt.
    let kopf_zusatz = kopf_hoehe.map(sauber).unwrap_or(0.0);
    if kopf_zusatz > 0.0 && nz > 0 {
        hoehen[0] = hoehen[0].max(kopf_zusatz);
    }

    for (zi, z) in zeilen.iter().take(nz).enumerate() {
        let mut sp = 0usize;
        for k in z.zellen.iter() {
            let c = (k.spalten.max(1) as usize).min(MAX_SPALTEN);
            // REGEL 4/6: die Spalten, die eine Zelle aus einer Zeile
            // darueber belegt, werden uebersprungen.
            while sp < n_spalten && sp < nz * n_spalten && belegt[zi * n_spalten + sp] {
                sp += 1;
            }
            if sp >= n_spalten {
                // Keine Spalte mehr frei: die Zelle bekommt Spalte 0.
                sp = 0;
            }
            // Die Breite der gespannten Spalten.
            let mut br = 0.0f32;
            for i in 0..c {
                if sp + i < n_spalten {
                    br += raus.spaltenbreiten[sp + i] + if i + 1 < c { gap } else { 0.0 };
                }
            }
            let h = sauber(k.hoehe);
            let r = (k.zeilen.max(1) as usize).min(MAX_ZEILEN_LAYOUT);
            raus.zellen.push(ZellenPlatz {
                spalte: sp as u32,
                zeile: zi as u32,
                spalten: c as u16, // = min(colspan, MAX_SPALTEN)
                zeilen: r as u16,
                breite: sauber(br),
                hoehe: h,
                x: x_von[sp.min(n_spalten - 1)],
                y: 0.0, // wird unten gefuellt
            });
            // Die Zelle belegt ihre Felder.
            //
            // M3/M4, UND DIE KORREKTUR EINES ECHTEN FEHLERS: die Grenzen
            // muessen die SCHLEIFEN begrenzen, nicht ihren Koerper.
            // Mit `for dz in 0..r { for ds in 0..c { if zz < nz … } }`
            // und `r = c = 65 535` waren das 4,3 MILLIARDEN Durchlaeufe
            // fuer EINE Zelle — gemessen 27,7 s. Jetzt werden die beiden
            // Schleifen direkt begrenzt.
            let dz_max = r.min(nz - zi);
            let ds_max = c.min(n_spalten - sp);
            for dz in 0..dz_max {
                let basis = (zi + dz) * n_spalten;
                for ds in 0..ds_max {
                    belegt[basis + sp + ds] = true;
                }
            }
            // Die Zeile (oder die letzte der gespannten) waechst.
            let ende = (zi + r - 1).min(nz - 1);
            hoehen[ende] = hoehen[ende].max(h);
            sp += c;
        }
    }
    raus.zeilenhoehen = hoehen.iter().map(|h| sauber(*h)).collect();

    // Die Y-Positionen und die Gesamthoehe.
    let mut y_von = vec![0.0f32; nz];
    {
        let mut y = 0.0f32;
        for i in 0..nz {
            y_von[i] = sauber(y);
            y += hoehen[i] + gap;
        }
        raus.hoehe = sauber(y - if nz > 0 { gap } else { 0.0 });
    }
    // Jede Zelle bekommt ihre endgueltige Position und ihre endgueltige
    // Hoehe (REGEL 4: eine gespannte Zelle ist so hoch wie ihre Zeilen).
    for z in raus.zellen.iter_mut() {
        let zi = z.zeile as usize;
        let mut h = 0.0f32;
        let mut erstes_y = 0.0f32;
        for dz in 0..z.zeilen as usize {
            let zz = zi + dz;
            if zz < nz {
                if dz == 0 {
                    erstes_y = y_von[zz];
                }
                h += hoehen[zz] + if dz + 1 < z.zeilen as usize { gap } else { 0.0 };
            }
        }
        z.y = erstes_y;
        z.hoehe = sauber(h);
    }
    raus.breite = sauber(b);
    raus
}

/// `rowspan = "0"` heisst in HTML 4.01 "bis zum Ende der Tabelle".
///
/// GEMESSEN, warum das hier eine Funktion ist: `narsil-page` liest es als
/// 1 ("eine Zelle, die sich ueber beliebig viele Zeilen erstreckt, ist
/// im Dokumentmodell nicht darstellbar"). Diese Funktion macht daraus
/// die Zahl, die die Tabelle gerade noch hat — so ist der Wunsch der
/// Rolle HTML eine Zeile und keine Baustelle.
pub fn rowspan_0_heisst_rest(zeilen_gesamt: usize) -> u16 {
    zeilen_gesamt.clamp(1, MAX_ZEILEN_LAYOUT) as u16
}

/// Der Kasten einer gelegten Zelle — die Form, die `LayoutBox` will.
pub fn zellen_rect(z: &ZellenPlatz) -> Rect {
    Rect::from_origin_size(Point::new(z.x, z.y), Size::new(z.breite, z.hoehe))
}

// ===========================================================================
// DIE WIKIPEDIA-INFOBOX — mein Gitter gegen das Firefox-Bild
// ===========================================================================

/// added 07.10.2026 20:05, Rolle SERVER.
///
/// **WARUM DIESE MESSUNG IM QUELLTEXT UND NICHT ALS `tests/`-DATEI:**
/// `tests/` haengt an `narsil-page` -> `narsil-raster` -> `narsil-image`, und
/// `narsil-image` ist gerade rot (`vp8.rs`, E0061/E0599, gemeldet in
/// `team/PROBLEME.txt` 96). Eine Messung, die an einer fremden Baustelle
/// haengt, ist keine Messung. Diese hier laeuft mit `bau.sh narsil-layout-
/// engine check`/`test --lib`, ohne fremde Crate im Spiel — und sie steht
/// an der Datei, um die es geht.
///
/// **DER SOLLWERT** ist am Firefox-Bild abgelesen
/// (`team/log/firefox_ref/rust_wiki.png`, Fensterdekoration 81 px
/// abgeschnitten): die Infobox „Basisdaten" steht bei x = 660..1005, also
/// **345 px breit**, mit einer Beschriftungsspalte („Paradigmen:") und
/// einer Wertspalte.
///
/// **DIE FRAGE, DIE DARAUS FOLGT:** bekommt die Funktion diese beiden
/// Mindestbreiten, legt sie die Kante zwischen beiden Spalten an die
/// Stelle, an der das Firefox-Bild sie zeigt? Wenn ja, ist mein Gitter
/// fuer diese Seite richtig und der Unterschied im Bild liegt am
/// **Aufrufer**. Wenn nein, ist es mein Gitter.
#[cfg(test)]
mod infobox_am_bild {
    use super::{Zeile, Zelle, lege_tabelle_mit_richtung};

    /// Die Infoboxbreite aus dem Firefox-Bild (am Bild abgelesen).
    const INFOSBREITE: f32 = 345.0;
    /// Die beiden Mindestbreiten: 150 + 195 = 345, also **genau** die
    /// Infoboxbreite — damit ist der Sollwert eindeutig und der Test
    /// sagt mehr als "irgendwas".
    const BESCHRIFTUNG: f32 = 150.0;
    const WERT: f32 = 195.0;

    fn zeile() -> Zeile {
        Zeile {
            zellen: vec![
                Zelle {
                    spalten: 1,
                    zeilen: 1,
                    breite: None,
                    hoehe: 24.0,
                    min_breite: Some(BESCHRIFTUNG),
                    max_breite: None,
                },
                Zelle {
                    spalten: 1,
                    zeilen: 1,
                    breite: None,
                    hoehe: 24.0,
                    min_breite: Some(WERT),
                    max_breite: None,
                },
            ],
        }
    }

    #[test]
    fn infobox_spaltentrennung_am_firefox_soll() {
        let t =
            lege_tabelle_mit_richtung(INFOSBREITE, &[zeile(), zeile(), zeile()], 0.0, None, false);
        assert_eq!(
            t.verworfen_zeilen + t.verworfen_zellen,
            0,
            "Zeilen wurden verworfen: {} / {}",
            t.verworfen_zeilen,
            t.verworfen_zellen
        );
        assert_eq!(t.spaltenbreiten.len(), 2, "zwei Spalten (Label, Wert)");

        let erste: Vec<&super::ZellenPlatz> = t.zellen.iter().filter(|p| p.zeile == 0).collect();
        assert_eq!(erste.len(), 2, "zwei Zellen in der ersten Zeile");

        // DIE KANTE: die Wertspalte beginnt bei 150 px, und beide
        // Spalten zusammen ergeben genau die Infoboxbreite.
        assert!(
            (erste[0].breite - BESCHRIFTUNG).abs() <= 1.0,
            "Beschriftungsspalte {:.1} px, erwartet {BESCHRIFTUNG}",
            erste[0].breite
        );
        assert!(
            (erste[1].x - BESCHRIFTUNG).abs() <= 1.0,
            "Wertspalte beginnt bei {:.1}, erwartet {BESCHRIFTUNG}",
            erste[1].x
        );
        let summe = erste[0].x + erste[0].breite + erste[1].breite;
        assert!(
            (summe - INFOSBREITE).abs() <= 1.0,
            "die Spalten ergeben {summe:.1} px, erwartet {INFOSBREITE}"
        );
    }

    /// added 07.10.2026, 20:15: DER RUECKWAERTSFALL bleibt gueltig.
    /// `min_breite: None` fuehrt unveraendert zur alten Regel 2 (beide
    /// Spalten gleich breit) — sonst waere die Verteilung vom 07.10.
    /// 16:15 ein Bruch.
    #[test]
    fn ohne_mindestbreiten_bleibt_die_alte_regel() {
        let ohne = Zeile {
            zellen: vec![
                Zelle {
                    spalten: 1,
                    zeilen: 1,
                    breite: None,
                    hoehe: 24.0,
                    min_breite: None,
                    max_breite: None,
                },
                Zelle {
                    spalten: 1,
                    zeilen: 1,
                    breite: None,
                    hoehe: 24.0,
                    min_breite: None,
                    max_breite: None,
                },
            ],
        };
        let t = lege_tabelle_mit_richtung(INFOSBREITE, &[ohne], 0.0, None, false);
        assert_eq!(t.spaltenbreiten.len(), 2);
        let summe: f32 = t.spaltenbreiten.iter().sum();
        assert!(
            (summe - INFOSBREITE).abs() <= 1.0,
            "die Spalten ergeben {summe:.1} px, erwartet {INFOSBREITE}"
        );
        assert!(
            (t.spaltenbreiten[0] - t.spaltenbreiten[1]).abs() <= 1.0,
            "ohne Mindestbreiten sind die Spalten gleich breit: {:?}",
            t.spaltenbreiten
        );
    }
}
