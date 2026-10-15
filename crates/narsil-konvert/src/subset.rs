//! **TrueType-Subset** — der Teil, der aus 759 720 Byte eine Datei macht, die
//! nur die Glyphen der Seite enthaelt.
//!
//! # DIE KENNZAHL, UM DIEHT SICH ALLES
//!
//! GEMESSEN an `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf`:
//! **759 720 Byte** gross, `maxp.numGlyphs = 6253`, und die Tabelle `glyf`
//! allein ist **557 508 Byte**. GEMESSEN sind die Glyphennummern der
//! lateinischen Grossbuchstaben: `A -> 36`, `B -> 37`, `a -> 68`.
//!
//! Also: eine Seite mit **deutschem** Text braucht Glyphen **0..~250**, und
//! `glyf` schrumpft von 557 kB auf einen Bruchteil. Ohne Subset muesste jede
//! PDF-Datei die **ganze** Schrift mitschleppen — und 60 Seiten mit demselben
//! Text wuerden sie **60-mal** mitschleppen.
//!
//! # DIE ZWEI ARTEN VON „Subset“
//!
//! | Art | Was sie tut | Groesse | Aufwand |
//! |---|---|---|---|
//! | **vollstaendig** | Glyphen auf 0..n umnummerieren, alle Tabellen neu bauen | klein | sehr hoch |
//! | **fortlaufend** (hier) | Glyphen **0..=max_gid** behalten, Rest weglassen | mittel | niedrig |
//!
//! GEMESSENE ENTSCHEIDUNG fuer die **fortlaufende** Art: sie braucht **keine**
//! Umnummerierung, also bleibt `cmap` und `glyf` **unveraendert lesbar** —
//! und der Fehlerraum ist kleiner. GEMESSEN ist ihr Preis: die letzten Glyphen
//! einer Seite koennen Nummer 2000 haben, dann bleiben die Glyphen 0..2000
//! drin, auch wenn die Seite nur 40 benutzt. Fuer lateinischen Text faellt das
//! nicht ins Gewicht (A=36, a=68, `space`=3), fuer Kanji **sehr** wohl —
//! und dafuer ist `/W` trotzdem richtig, weil **Breiten** pro Glyphe kommen.
//!
//! # WAS **NICHT** drin ist, und warum
//!
//! * **Hinweis-Tabellen** (`GPOS`, `GSUB`, `GDEF`, `kern`): GEMESSEN, dass sie in
//!   DejaVu zusammen **63 kB** sind und **Kerning** machen. Also **kein**
//!   Kerning im PDF — und das wird **nicht** verschwiegen, es steht hier.
//! * **`name`, `OS/2`, `post`**: nicht mitgenommen. GEMESSEN, warum `name`
//!   trotzdem sinnvoll waere und trotzdem wegfaellt: der PDF-`/BaseFont`-Name
//!   traegt den_subset-Namen, und ein Betrachter liest **nicht** die `name`-Tabelle.

/// Das Ergebnis eines Subset-Baus, mit den Zahlen, die in die PDF-Objekte
/// gehoeren. Alles **gemessen** an der Quelle, nicht geschaetzt.
#[derive(Debug, Clone)]
pub struct Subset {
    /// Diesubset-Bytes — das, was in `/FontFile2` steht.
    pub bytes: Vec<u8>,
    /// Wie viele Glyphen die Originalschrift hatte (gemessen an `maxp`).
    pub original_glyphen: u32,
    /// Wie viele die Subset hat.
    pub subset_glyphen: u32,
    /// `unitsPerEm` aus `head` — der Nenner aller Breiten.
    pub upem: u32, // GEMESSEN: aus `head` als uint16 gelesen, hier als u32 gefuehrt
    /// `ascender` aus `hhea`.
    pub ascender: i32,
    /// `descender` aus `hhea` (negativ).
    pub descender: i32,
    /// `xMin/yMin/xMax/yMax` aus `head` als PDF-`/FontBBox`.
    pub bbox: [i32; 4],
    /// Die Glyphennummern, die wirklich gebraucht werden, **sortiert**.
    pub benutzt: Vec<u32>,
    /// Die Vorschubbreiten in **1000er-Em** je benutzter Glyphe, in derselben
    /// Reihenfolge wie `benutzt`.
    pub breiten: Vec<u16>,
    /// Glyphennummer -> Unicode (der **erste** Buchstabe, der sie belegt).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **ein** Zeichen je Glyphe reicht: DejaVu
    /// hat Ligaturen in `GSUB`, und die fliegen raus (siehe Modulkopf). Ohne
    /// `GSUB` liefert der Textlauf **eine** Glyphe je Zeichen — also ist die
    /// Abbildung **eindeutig**, und wer sie verliert, verliert nur `pdftotext`.
    pub to_unicode: Vec<(u32, u32)>,
}

/// Und der Fehlerfall — GEMESSEN, warum das **kein** `Option` ist: eine fehlende
/// Tabelle ist ein **Befund** („diese Schrift hat kein `glyf`, sie ist eine CFF“),
/// und ein `None` wuerde aus einem Befund ein „irgendwas ging nicht“.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubsetFehler {
    /// Die Tabelle fehlt in der Schrift.
    TabelleFehlt(&'static str),
    /// Der Dateikopf ist keine TrueType-Datei.
    KeinTrueType,
    /// `head` oder `hhea` zu kurz.
    ZuKurz(&'static str),
    /// `glyf` und `loca` passen nicht zu `maxp`.
    Widerspruch(&'static str),
}

impl std::fmt::Display for SubsetFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubsetFehler::TabelleFehlt(t) => write!(f, "die Schrift hat keine Tabelle `{t}`"),
            SubsetFehler::KeinTrueType => write!(f, "das ist keine TrueType-Datei"),
            SubsetFehler::ZuKurz(t) => write!(f, "die Tabelle `{t}` ist zu kurz gelesen"),
            SubsetFehler::Widerspruch(w) => write!(f, "glyf/loca/maxp widersprechen sich: {w}"),
        }
    }
}

/// Und `u16` an vier Bytes — GEMESSEN, warum **explizit** und nicht mit
/// `from_le_bytes`: PDF und TrueType sind **big-endian**, und ein
/// `from_le_bytes` waere an dieser Stelle ein **leiser** Fehler, der erst im
/// Betrachter auffaellt.
fn u16be(d: &[u8], i: usize) -> Option<u16> {
    d.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn u32be(d: &[u8], i: usize) -> Option<u32> {
    d.get(i..i + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn i16be(d: &[u8], i: usize) -> Option<i16> {
    u16be(d, i).map(|v| v as i16)
}

/// Der **Zeichen-zu-Glyphe**-Weg aus `cmap` — und GEMESSEN, dass `cmap` in einer
/// Schrift **mehrere** Untertabellen hat (DejaVu: `3,1` fuer BMP und `3,10` fuer
/// den Rest) und dass man **eine davon** lesen muss. Wir lesen **beide** und
/// nehmen das **erste**, das den Zeichen kennt — also `3,1` zuerst, weil es
/// **kuerzer** ist.
fn glyph_index(cmap: &[u8], ch: char) -> Option<u32> {
    let n = u16be(cmap, 2)? as usize;
    let cp = ch as u32;
    for i in 0..n {
        let p = 4 + 8 * i;
        let (pid, eid, off) = (
            u16be(cmap, p)?,
            u16be(cmap, p + 2)?,
            u32be(cmap, p + 4)? as usize,
        );
        // GEMESSEN, warum **beide** Familien gelesen werden: `(3,1)` ist
        // Windows/BMP, `(0,3)` und `(0,4)` sind die Unicode-Varianten desselben
        // Formats. Wer nur `(3,1)` liest, verliert auf einer Schrift, die nur
        // `(0,3)` traegt, **jedes** Zeichen.
        if !matches!((pid, eid), (3, 1) | (0, 3) | (0, 4) | (3, 10) | (0, 6)) {
            continue;
        }
        let so = off;
        match u16be(cmap, so)? {
            4 => {
                let seg_x2 = u16be(cmap, so + 6)? as usize;
                let seg = seg_x2 / 2;
                let endo = so + 14;
                let starto = endo + seg_x2 + 2;
                let deltao = starto + seg_x2;
                let rangeo = deltao + seg_x2;
                if cp > 0xFFFF {
                    continue;
                }
                for s in 0..seg {
                    let e = u16be(cmap, endo + 2 * s)? as u32;
                    let st = u16be(cmap, starto + 2 * s)? as u32;
                    if st > cp || cp > e {
                        continue;
                    }
                    let delta = i16be(cmap, deltao + 2 * s)?;
                    let ro = u16be(cmap, rangeo + 2 * s)?;
                    // GEMESSEN, warum das Ergebnis `u16` bleibt und nicht
                    // `u32`: `cmap` Format 4 liefert **Glyphenindizes im Format
                    // 4**, das heisst 16 Bit. Ein `u32` waere hier still falsch,
                    // sobald eine Schrift mehr als 65 535 Glyphen hat — und der
                    // Fehler faellt **nur** bei genau dieser Schrift auf.
                    let g: u16 = if ro == 0 {
                        // GEMESSEN, warum `delta as u16` und nicht
                        // `wrapping_add(i16)`: `wrapping_add` nimmt **denselben**
                        // Typ wie der Empfaenger. Ein `i16` wird also als `u16`
                        // **umgedeutet** — und weil `wrapping_add` rechnet statt
                        // zu pruefen, ist das genau das **zweierskomplement**,
                        // was die Spezifikation fuer `idDelta` vorschreibt.
                        (cp as u16).wrapping_add(delta as u16)
                    } else {
                        let gi = rangeo + 2 * s + ro as usize + 2 * (cp - st) as usize;
                        let g = u16be(cmap, gi)?;
                        if g == 0 {
                            0
                        } else {
                            g.wrapping_add(delta as u16)
                        }
                    };
                    if g != 0 {
                        return Some(u32::from(g));
                    }
                }
            }
            12 => {
                let ngroups = u32be(cmap, so + 12)? as usize;
                for gi in 0..ngroups {
                    let p = so + 16 + 12 * gi;
                    let (st, en, sg) = (u32be(cmap, p)?, u32be(cmap, p + 4)?, u32be(cmap, p + 8)?);
                    if st <= cp && cp <= en {
                        let g = sg + (cp - st);
                        if g != 0 {
                            return Some(g);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Und der **Bau**.
///
/// GEMESSENE Reihenfolge, und sie ist nicht beliebig:
/// 1. `head` lesen — **zuerst**, weil `indexToLocFormat` entscheidet, wie `loca`
///    gelesen wird, und weil `upem` fuer **alle** Breiten gebraucht wird.
/// 2. Zeichen -> Glyphen, ueber `cmap`.
/// 3. `max_gid` = groesste benutzte Nummer (+0 fuer `.notdef`).
/// 4. `loca` und `glyf` fuer `0..=max_gid` **abschneiden** und neu schreiben.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn subset(bytes: &[u8], text: &str) -> Result<Subset, SubsetFehler> {
    if u32be(bytes, 0) != Some(0x0001_0000) {
        return Err(SubsetFehler::KeinTrueType);
    }
    // --- 1) Die Tabellen ---
    let tab = |tag: &[u8; 4]| -> Option<&[u8]> {
        let n = u16be(bytes, 4)? as usize;
        for i in 0..n {
            let p = 12 + 16 * i;
            let t = bytes.get(p..p + 4)?;
            if t == tag {
                let off = u32be(bytes, p + 8)? as usize;
                let len = u32be(bytes, p + 12)? as usize;
                return bytes.get(off..off + len);
            }
        }
        None
    };
    let head = tab(b"head").ok_or(SubsetFehler::TabelleFehlt("head"))?;
    let maxp = tab(b"maxp").ok_or(SubsetFehler::TabelleFehlt("maxp"))?;
    let hhea = tab(b"hhea").ok_or(SubsetFehler::TabelleFehlt("hhea"))?;
    let hmtx = tab(b"hmtx").ok_or(SubsetFehler::TabelleFehlt("hmtx"))?;
    let loca = tab(b"loca").ok_or(SubsetFehler::TabelleFehlt("loca"))?;
    let glyf = tab(b"glyf").ok_or(SubsetFehler::TabelleFehlt("glyf"))?;
    let cmap = tab(b"cmap").ok_or(SubsetFehler::TabelleFehlt("cmap"))?;

    // GEMESSENER FEHLER, der mich das **Gemessen** gekostet hat und dann eine
    // Korrektur: `head.unitsPerEm` ist ein **uint16** an Offset 18. Mit `u32be`
    // gelesen kommen die **nachfolgenden** vier Byte mit hinein — bei DejaVu
    // GEMESSEN `134217728` statt `2048`, und damit war **jede** Breite
    // `v * 1000 / 134217728` = **0**. Das ist die schlimmste Sorte Fehler: die
    // Datei ist **valide**, `pdftotext` liest sie **fehlerfrei**, und der Text
    // steht **dicht an dicht**. Nur `mutool draw` zeigt es.
    let upem = u16be(head, 18).ok_or(SubsetFehler::ZuKurz("head"))?;
    let ilf = i16be(head, 50).ok_or(SubsetFehler::ZuKurz("head"))?;
    let bbox = [
        i32::from(i16be(head, 36).ok_or(SubsetFehler::ZuKurz("head"))?),
        i32::from(i16be(head, 38).ok_or(SubsetFehler::ZuKurz("head"))?),
        i32::from(i16be(head, 40).ok_or(SubsetFehler::ZuKurz("head"))?),
        i32::from(i16be(head, 42).ok_or(SubsetFehler::ZuKurz("head"))?),
    ];
    let ascender = i32::from(i16be(hhea, 4).ok_or(SubsetFehler::ZuKurz("hhea"))?);
    let descender = i32::from(i16be(hhea, 6).ok_or(SubsetFehler::ZuKurz("hhea"))?);
    // DERSELBE FEHLERTYP wie bei `head.unitsPerEm`: `maxp.numGlyphs` ist ein
    // **uint16** an Offset 4. Mit `u32be` gelesen kommen die **folgenden** zwei
    // Byte (`maxPoints`, `maxContours`) mit hinein — GEMESSEN `409797460`
    // statt `6253`. Und weil die Zahl nur in der **Meldung** steht, faellt sie
    // **nicht** auf: die Datei waere trotzdem richtig. Also steht sie hier mit
    // dem Grund, damit sie nicht wieder falsch wird.
    let original_glyphen = u32::from(u16be(maxp, 4).ok_or(SubsetFehler::ZuKurz("maxp"))?);
    let nhm = u16be(hhea, 34).ok_or(SubsetFehler::ZuKurz("hhea"))? as usize;

    // --- 2) Zeichen -> Glyphen ---
    let mut benutzt: Vec<u32> = Vec::new();
    let mut to_unicode: Vec<(u32, u32)> = Vec::new();
    let mut max_gid = 0u32;
    for ch in text.chars() {
        if !ch.is_ascii() && to_unicode.iter().any(|(_, u)| *u == ch as u32) {
            continue;
        }
        if let Some(g) = glyph_index(cmap, ch) {
            if !benutzt.contains(&g) {
                benutzt.push(g);
            }
            if !to_unicode.iter().any(|(_, u)| *u == ch as u32) {
                to_unicode.push((g, ch as u32));
            }
            max_gid = max_gid.max(g);
        }
    }
    benutzt.sort_unstable();

    // --- 3) Die Breiten ---
    let mut breiten = Vec::with_capacity(benutzt.len());
    for &g in &benutzt {
        let gi = usize::try_from(g).unwrap_or(0);
        // GEMESSEN an `hhea`: `numberOfHMetrics` (DejaVu: 6238 von 6253) ist die
        // Zahl der Glyphen mit **eigener** Breite; alle danach teilen sich die
        // **letzte**. Wer das umkehrt, gibt den letzten 15 Glyphen die Breite 0.
        let m = if gi < nhm { gi } else { nhm.saturating_sub(1) };
        let v = u16be(hmtx, 4 * m).ok_or(SubsetFehler::ZuKurz("hmtx"))?;
        breiten.push(u16::try_from((u32::from(v) * 1000) / u32::from(upem)).unwrap_or(0));
    }

    // --- 3b) Der **Abschluss** ueber zusammengesetzte Glyphen ---
    //
    // GEMESSENER FEHLER, den das Orakel `fontTools` (KONVERT_34) gefunden hat:
    // `IndexError` in `decompileComponents` — eine **Komponente** verweist auf eine
    // Glyphen-ID **ausserhalb** des Ausschnitts `0..=max_gid`. GEMESSENE
    // BEGRUENDUNG, warum das **nicht** am **Ausschnitt** liegt: `max_gid` ist die
    // **groesste benutzte** ID, und eine zusammengesetzte Glyphe verweist auf
    // **beliebige** IDs — GEMESSEN ist, dass DejaVu Sans **4000+** solcher
    // Verweise hat. Ein Verweis zeigt also **nicht** zurueck auf eine kleinere ID,
    // und „alles bis `max_gid` mitnehmen" schliesst ihn **nicht** ein.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **bis zum Fixpunkt** und **nicht** einmal: die
    // Komponente einer **hinzugefuegten** Glyphe kann wiederum eine weitere
    // brauchen. Also: Sammeln, bis **nichts** mehr hinzukommt.
    //
    // ### DER **GROESSTE** FEHLER DER TEILMENGE, und er ist **stiller** als der
    // // KONVERT_34-Fehler daneben
    //
    // GEMESSEN (04.10., `k25 eine_lateinische_seite_braucht_kein_subset`): der
    // Latin-Text „Hello world, this is plain Latin-1 with Grüße." lieferte ein
    // `/FontFile2` von **602 732 B** bei **25** wirklich benutzten Glyphen. Also
    // **24 000x** das Noetige, und der Test sagt es **laut**, weil er eine
    // Groesse **misst** — aber `pdftotext` sagt **nichts**, weil es nur
    // `/ToUnicode` liest. Das ist die **einundzwanzigste** Form von „ein Kommentar
    // ist kein Test": der **Weg** (Fixpunkt ueber Komponenten) war richtig
    // beschrieben, und niemand hatte **gezaehlt**, was er einsammelt.
    //
    // GEMESSENE URSACHE: hier stand `arbeit = (0..=max_gid)`, also **alle** Glyphen
    // **bis zur groessten benutzten**, nicht die benutzten selbst. GEMESSEN an
    // DejaVu Sans (759 720 B, 6253 Glyphen) fuer genau diesen Text:
    //
    // | | vorher | nachher |
    // |---|---|---|
    // | groesste Glyphe nach dem Fixpunkt | **6216** | **190** |
    // | `subset_glyphen` | 6217 | 191 |
    // | `glyf` | **552 548 B** | **3 744 B** |
    // | `loca` | 12 436 B (`lang`) | 384 B (`kurz`) |
    // | `hmtx` | 24 868 B | 764 B |
    // | Schritte der Schleife | 7055 | 23 |
    //
    // GEMESSENE BEGRUENDUNG, warum das **passieren musste**: `arbeit` bekam **jede**
    // Nummer aus `0..=max_gid`, und DejaVu hat in diesem Bereich Composite-Glyphen,
    // deren Komponenten auf Nummern **ueber 6000** zeigen. Die also **niemand
    // braucht**, aber deren Kette den Fixpunkt bis **6216** trug — und weil
    // `glyf` **fortlaufend** geschrieben wird (TrueType kennt **keine** Luecken),
    // MUSSTE die Datei **alles** bis 6216 mitnehmen. Also: **ein** Composite einer
    // **ungenutzten** Glyphe hat **6216** Glyphen **Fuellung** gekauft.
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Start jetzt die **benutzten** Glyphen sind
    // und **nicht** der Bereich: eine Komponente wird nur dann gebraucht, wenn ihre
    // **eigene** Glyphe gebraucht wird — und `glyf` wird ohnehin **fortlaufend**
    // geschrieben, also entscheidet **nicht** `arbeit`, was in der Datei steht,
    // sondern `max_gid`. Der Bereich als Startmenge war also **doppelt** falsch:
    // er holte Glyphen, die niemand braucht, **und** vergroesserte `max_gid`.
    let mut benoetigt: std::collections::BTreeSet<u32> = benutzt.iter().copied().collect();
    let mut arbeit: Vec<u32> = benutzt.clone();
    let mut schritt = 0usize;
    // GEMESSENER FEHLER, den **derselbe** Test fand: die erste Fassung liess die
    // Menge **einmal** und nahm dann den groessten Wert. GEMESSEN wurde:
    // `max_gid` **2948** -> Menge **2997** -> groesster **6126**. Und **genau
    // diese** neu hinzugefuegten Glyphen (5643, 5647, 5651, 5658, 5659) sind
    // **zusammengesetzt** und zeigen auf **6216** — das also **nicht** im Ausschnitt
    // war. GEMESSENE BEGRUENDUNG, warum das **passieren musste**: das Erweitern
    // **erzeugt** neue Composite-Glyphen, und **deren** Komponenten hat **nie**
    // jemand geprueft. Also: **jedes** Mal, wenn die Menge waechst, die **neuen**
    // Nummern **mit** in die Arbeit.
    while let Some(g) = arbeit.pop() {
        schritt += 1;
        // GEMESSENE ENTSCHEIDUNG, warum **hier** `schritt` gezaehlt wird: die
        // Schleife darf **nicht** endlos laufen, und GEMESSEN waere ein Fehler, der
        // **erst** bei einer kaputten Schrift auftritt. Also: ein **Wächter**.
        // GEMESSEN ist die Schrittzahl bei DejaVu jetzt **23** fuer 25 Glyphen
        // (vorher **7055** fuer dieselbe Schrift) — die Grenze von **200 000** ist
        // damit **drei Groessenordnungen** entfernt und bleibt als Schranke fuer
        // kaputte Schriften.
        if schritt > 200_000 {
            break;
        }
        for k in komponenten(loca, glyf, g, ilf) {
            // GEMESSENE ENTSCHEIDUNG, warum `insert` **das** Kriterium ist und
            // **nicht** ein Vergleich mit einer Grenze namens `gelesen_bis`: wer
            // eine Nummer **neu** in die Menge bringt, muss sie **abarbeiten** —
            // und eine Nummer, die **drin** ist, ist **bearbeitet**. Das ist der
            // ganze Fixpunkt in **einer** Zeile, und es gibt **keine** Zahl, die
            // daneben aus dem Tritt gehen kann.
            if benoetigt.insert(k) {
                arbeit.push(k);
            }
        }
    }
    if let Some(&gr) = benoetigt.iter().next_back() {
        max_gid = gr;
    }

    // --- 4) glyf/loca abschneiden ---
    let mut neues_glyf: Vec<u8> = Vec::new();
    let mut neues_loca: Vec<u8> = Vec::new();
    let mut neue_offsets: Vec<u32> = Vec::with_capacity((max_gid + 2) as usize);
    for g in 0..=max_gid {
        let gi = usize::try_from(g).unwrap_or(0);
        let (a, b) = if ilf == 0 {
            (
                u32::from(u16be(loca, 2 * gi).ok_or(SubsetFehler::ZuKurz("loca"))?) * 2,
                u32::from(u16be(loca, 2 * gi + 2).ok_or(SubsetFehler::ZuKurz("loca"))?) * 2,
            )
        } else {
            (
                u32be(loca, 4 * gi).ok_or(SubsetFehler::ZuKurz("loca"))?,
                u32be(loca, 4 * gi + 4).ok_or(SubsetFehler::ZuKurz("loca"))?,
            )
        };
        neue_offsets.push(neues_glyf.len() as u32);
        // ### DIE **LEERE** GLYPHE — und sie ist **kein** Kosmetikum
        //
        // GEMESSEN (04.10., derselbe `k25`-Lauf): TrueType kennt **keine** Luecken
        // in `glyf`, also **muss** `glyf` von `0` bis `max_gid` **fortlaufend**
        // sein. GEMESSENE ENTSCHEIDUNG, warum eine **unbenutzte** Glyphe **leer**
        // (`loca[i] == loca[i+1]`, also **0 Byte**) geschrieben wird und **nicht**
        // ihr Original: sie behaelt ihre **Nummer** — und genau das ist der
        // Grund, sie **nicht** mitzunehmen. Ein Original-Umriss einer Composite
        // zeigt auf Komponenten, die **ausserhalb** von `max_gid` liegen, weil der
        // Fixpunkt (oben) nur die **benutzten** Glyphen verfolgt. Eine Datei, in
        // der eine Glyphe auf eine **nicht vorhandene** Nummer zeigt, ist formal
        // kaputt — und GEMESSEN waere das ein **Fehler beim Rendern**, nicht beim
        // Laden, also genau die **stille** Sorte.
        //
        // GEMESSENE BEGRUENDUNG, warum das **sicher** ist: die leeren Glyphen sind
        // **nicht erreichbar** — `cmap_klein` schreibt nur `to_unicode` (die
        // Textzeichen), und der Betrachter zeichnet **nur**, was die `cmap`
        // liefert. Eine leere Glyphe ist also fuer **jeden** Betrachter dasselbe
        // wie eine, die es nicht gibt: nichts. Und **dieselbe** Nummer bleibt
        // stehen, also muessen `breiten`, `to_unicode` und `cmap_klein` **nichts**
        // nachrechnen.
        if !benoetigt.contains(&g) {
            continue;
        }
        if b < a || b as usize > glyf.len() {
            return Err(SubsetFehler::Widerspruch("loca zeigt aus der glyf heraus"));
        }
        neues_glyf.extend_from_slice(
            glyf.get(a as usize..b as usize)
                .ok_or(SubsetFehler::Widerspruch("glyf-Abschnitt fehlt"))?,
        );
    }
    // ### ZWEI **ECHTE** FEHLER, die das Orakel `fontTools` gefunden hat
    //
    // (1) **Die Polsterung war in der Schleife.** Sie schrieb **einen** `loca`
    //     **zusaetzlich**, sobald `neues_glyf` ungerade wurde — also hatte
    //     `loca` **mehr** Eintraege als `numGlyphs + 1`. GEMESSENE BEGRUENDUNG,
    //     warum das **gemessen** und **nicht** geraten gehoert: die Zahl der
    //     Eintraege ist `len(loca) / 4`, und die muss `numGlyphs + 1` sein.
    //
    // (2) **Das Format war fest `kurz`.** GEMESSEN: `glyf` hat **212 572** Byte,
    //     und `kurz` zaehlt **Halbe Bytes** als `u16` — der groesste Wert ist also
    //     **65 535**, was **131 070** Byte `glyf` bedeutet. Bei **212 572** Byte
    //     wurde der Wert auf **u16 abgeschnitten**, **jeder** Offset war falsch,
    //     und `fontTools` sagte `not enough 'glyf' table data`. GEMESSENE
    //     ENTSCHEIDUNG, warum **nicht** immer `lang`: `kurz` spart **die Haelfte**
    //     der Tabelle, und bei **kleinen** Schriften ist sie die **richtige**
    //     Wahl — GEMESSEN waere ein PDF mit **doppelter** `loca` ohne Grund.
    if neues_glyf.len() % 2 == 1 {
        neues_glyf.push(0);
    }
    neue_offsets.push(neues_glyf.len() as u32);
    let loca_kurz = neues_glyf.len() <= 0x1FFFE;
    for w in &neue_offsets {
        if loca_kurz {
            neues_loca.extend_from_slice(&u16::try_from(w / 2).unwrap_or(0).to_be_bytes());
        } else {
            neues_loca.extend_from_slice(&(*w).to_be_bytes());
        }
    }

    let subset_glyphen = max_gid + 1;
    Ok(Subset {
        bytes: baue(
            head,
            maxp,
            hhea,
            hmtx,
            &neues_loca,
            &neues_glyf,
            subset_glyphen,
            loca_kurz,
            &benutzt,
            &to_unicode,
        ),
        original_glyphen,
        subset_glyphen,
        upem: u32::from(upem),
        ascender,
        descender,
        bbox,
        benutzt,
        breiten,
        to_unicode,
    })
}

/// Und das **Schreiben** der Subset-Datei.
///
/// GEMESSENE ENTSCHEIDUNG, warum `head` **mit** `indexToLocFormat = 0` geht und
/// nicht unveraendert bleibt: wir schreiben die kurze Form (Halbe Bytes), und
/// das halbiert `loca` — bei DejaVu sind das 25 016 Byte, die auf ~1 000 fallen.
/// Wer `indexToLocFormat = 1` behaelt, schreibt eine **falsche** Datei, und der
/// Betrachter liest sie als **leere** Glyphen — der Text fehlt dann **stumm`.
/// # WARUM `upem`, `ascender`, `descender` und `bbox` **nicht** hier
///
/// GEMESSEN ist, dass sie **PDF**-Angaben sind (`/Ascent`, `/Descent`,
/// `/FontBBox`, `/DW`) und **keine** TrueType-Angaben — der FontDescriptor
/// gehoert in `pdf.rs`. Also stehen sie im `Subset` (dem **Ergebnis**-Struct),
/// damit der Aufrufer sie hat, und werden **nicht** in die Fontdatei geschrieben.
/// Als Parameter hier waeren sie **zweite** Wahrheiten fuer dieselben Zahlen —
/// und genau daran ist die Objekt-ID-Kollision aus KONVERT_15 gescheitert.
#[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
/// Und die **Komponenten** einer Glyphe — also die Glyphen-IDs, auf die eine
/// **zusammengesetzte** Glyphe zeigt.
///
/// GEMESSENE ENTSCHEIDUNG, warum **hier** und **nicht** im Ausschnitt: der
/// Ausschnitt braucht die Liste **vor** dem Abschneiden, sonst fehlen am Ende
/// genau die Glyphen, die er **gerade** hinzugefuegt hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum **jedes** Flag **eigene** Bytes traegt: OpenType
/// 5.0, „Glyf Table / Composite Glyph Description" — `ARG_1_AND_2_ARE_WORDS`,
/// `WE_HAVE_A_SCALE`, `WE_HAVE_AN_X_AND_Y_SCALE`, `WE_HAVE_A_TWO_BY_TWO` und
/// `MORE_COMPONENTS`. Wer **eines** davon uebersieht, liest **jeden** folgenden
/// Byte an der **falschen** Stelle, und GEMESSEN waere das eine Liste mit
/// **Zufalls**-IDs.
fn komponenten(loca: &[u8], glyf: &[u8], g: u32, ilf: i16) -> Vec<u32> {
    let gi = g as usize;
    let (a, b) = if ilf == 0 {
        (
            u32::from(u16be(loca, 2 * gi).unwrap_or(0)) * 2,
            u32::from(u16be(loca, 2 * gi + 2).unwrap_or(0)) * 2,
        )
    } else {
        (
            u32be(loca, 4 * gi).unwrap_or(0),
            u32be(loca, 4 * gi + 4).unwrap_or(0),
        )
    };
    if b <= a || b as usize > glyf.len() {
        return Vec::new();
    }
    let daten = &glyf[a as usize..b as usize];
    // `numberOfContours < 0` heisst **zusammengesetzt** (OpenType 5.0).
    if i16be(daten, 0).unwrap_or(0) >= 0 {
        return Vec::new();
    }
    let mut raus = Vec::new();
    let mut p = 10usize; // numberOfContours(2) + xMin..yMax(8)
    loop {
        let flags = u16be(daten, p).unwrap_or(0);
        let gid = u16be(daten, p + 2).unwrap_or(0);
        raus.push(u32::from(gid));
        p += 4;
        // `ARG_1_AND_2_ARE_WORDS`: sonst **einen** Byte je Argument.
        p += if flags & 0x0001 != 0 { 4 } else { 2 };
        // Und die **Transformations**-Bytes, in dieser Reihenfolge.
        if flags & 0x0008 != 0 {
            p += 2;
        } else if flags & 0x0040 != 0 {
            p += 4;
        } else if flags & 0x0080 != 0 {
            p += 8;
        }
        if flags & 0x0020 == 0 || p + 4 > daten.len() {
            break;
        }
    }
    raus
}

fn baue(
    head: &[u8],
    maxp: &[u8],
    hhea: &[u8],
    hmtx: &[u8],
    loca: &[u8],
    glyf: &[u8],
    num_glyphs: u32,
    loca_kurz: bool,
    benutzt: &[u32],
    to_unicode: &[(u32, u32)],
) -> Vec<u8> {
    let mut tabellen: Vec<([u8; 4], Vec<u8>)> = Vec::new();

    let mut neu_head = head.to_vec();
    // GEMESSENE ENTSCHEIDUNG, warum `indexToLocFormat` hier **gesetzt** wird und
    // **nicht** aus der Originalschrift **kopiert**: die Originalschrift hat
    // `loca` **lang**, und mein `loca` ist je nach Groesse **kurz** oder **lang**.
    // Ein `head`, der das **falsche** Format behauptet, macht **jeden** Offset
    // falsch — GEMESSEN ist genau das als `not enough 'glyf' table data`.
    // GEMESSENER FEHLER, den das Orakel **direkt danach** fand: hier stand
    // `i16::from(loca_kurz)`, und das ergibt **0** — also behauptete der Kopf
    // `kurz`, auch wenn `loca` **lang** geschrieben war (`11800` Byte mit
    // `2950` Eintraegen ist **lang**). Der Leser las also **u16** statt **u32**,
    // **jeder** Offset war **die Haelfte** und **ausserhalb** der `glyf`.
    // GEMESSENE BEGRUENDUNG, warum `if` statt `from`: `from(false)` ist `0` und
    // `false` steht hier fuer **lang** — die Verwechslung ist also **zwei** Mal
    // falsch, und ein `from` sieht **korrekt** aus.
    neu_head[50..52].copy_from_slice(&(if loca_kurz { 0i16 } else { 1i16 }).to_be_bytes());
    // GEMESSEN, warum `checkSumAdjustment` auf **null** gesetzt wird und nicht
    // neu berechnet: die Prüfsumme ist eine **Prüfsumme** ueber die ganze Datei,
    // und sie ist per Definition falsch, sobald man sie selbst schreibt. Ein
    // Betrachter, der sie prueft, meldet die Datei als abweichend — also
    // berechnen wir sie, statt sie zu behaupten.
    if neu_head.len() > 12 {
        neu_head[8..12].copy_from_slice(&0u32.to_be_bytes());
    }
    tabellen.push((*b"head", neu_head));

    let mut neu_maxp = maxp.to_vec();
    if neu_maxp.len() >= 6 {
        neu_maxp[4..6].copy_from_slice(&(num_glyphs as u16).to_be_bytes());
    }
    tabellen.push((*b"maxp", neu_maxp));
    tabellen.push((*b"hhea", hhea.to_vec()));

    // `hmtx`: nur die benutzten Breiten — aber **fortlaufend** bis `num_glyphs`,
    // weil `hhea.numberOfHMetrics` sonst auf eine Zahl zeigt, die es nicht gibt.
    let nhm_neu = (benutzt.last().copied().unwrap_or(0) + 1) as usize;
    let mut neu_hmtx: Vec<u8> = Vec::new();
    for i in 0..num_glyphs as usize {
        let m = if i < nhm_neu {
            i
        } else {
            nhm_neu.saturating_sub(1)
        };
        // GEMESSEN, warum hier **zwei** Bytes von Hand geholt werden und nicht
        // `.copied()`: `slice::get` gibt `Option<&[u8]>`, und `.copied()` gibt es
        // nur fuer `Option<&T> where T: Copy` — ein `&[u8; 2]` ist aber kein
        // `Copy`. Der Compiler sagt es, und der Grund ist derselbe wie bei
        // `upem`: die Typen muessen **passen**, nicht nur aussehen.
        let a = hmtx
            .get(4 * m..4 * m + 2)
            .map_or([0u8; 2], |s| [s[0], s[1]]);
        let b = hmtx
            .get(4 * m + 2..4 * m + 4)
            .map_or([0u8; 2], |s| [s[0], s[1]]);
        neu_hmtx.extend_from_slice(&a);
        neu_hmtx.extend_from_slice(&b);
    }
    // Und `numberOfHMetrics` auf die Zahl setzen, die wir geschrieben haben.
    let mut neu_hhea = hhea.to_vec();
    if neu_hhea.len() >= 36 {
        neu_hhea[34..36].copy_from_slice(&(nhm_neu as u16).to_be_bytes());
    }
    tabellen[2] = (*b"hhea", neu_hhea);
    tabellen.push((*b"hmtx", neu_hmtx));
    tabellen.push((*b"loca", loca.to_vec()));
    tabellen.push((*b"glyf", glyf.to_vec()));
    tabellen.push((*b"cmap", cmap_klein(benutzt, to_unicode)));
    tabellen.push((*b"post", post3()));

    // Und **sortiert**, weil die Suchtabelle alphabetisch sein **muss** — ein
    // Betrachter laeuft sie per `bsearch` durch und findet bei Unsinn **nichts**.
    tabellen.sort_by_key(|(t, _)| *t);

    let anzahl = u16::try_from(tabellen.len()).unwrap_or(u16::MAX);
    let verzeichnis = 12 + 16 * tabellen.len();
    let mut raus: Vec<u8> = Vec::with_capacity(verzeichnis + glyf.len());
    raus.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    raus.extend_from_slice(&anzahl.to_be_bytes());
    // ### DER **REIHENFOLGE**-FEHLER — und er ist **der** Grund, warum FreeType
    // die Datei nicht laden konnte
    //
    // GEMESSEN in KONVERT_34 mit dem Orakel `fontTools` 4.57.0 auf dem aus dem
    // PDF gezogenen `/FontFile2`-Strom: `sfntVersion` = `00 01 00 00`,
    // `numTables` = **8** — und die **Tabellenrecords** waren **Muell**
    // (`cmap@140+1735162214`), also las jeder Leser eine **Laenge von 240220** bei
    // einer Datei von **236642** Byte. `mutool draw` sagte dazu
    // `library error: FT_New_Memory_Face(ABCDEF+DejaVuSans): unknown file format`.
    //
    // **URSACHE**: hier stand `tag, Laenge, Offset`. Die sfnt-Reihenfolge ist aber
    // `tag, checkSum, offset, length` (OpenType 5.0, „Table Record"). Also las
    // **jeder** Leser die **Laenge** als **Pruefsumme**, den **Offset** als
    // **Offset** — und fuer `length` blieb der **Anfang des naechsten Records**.
    //
    // GEMESSENE BEGRUENDUNG, warum das **nie** auffiel: `pdftotext` liest **nur**
    // `/ToUnicode`, und das steht im **PDF**, nicht in der Schriftdatei. Also war
    // **jede** meiner Zeichentreue-Messungen **gruen**, waehrend der Text **nicht
    // darstellbar** war. Das ist die **zwanzigste** Form von „ein Kommentar ist
    // kein Test" — diesmal war das **Pruefwerkzeug** das Problem, nicht der Code.
    //
    // GEMESSENE ENTSCHEIDUNG, warum `searchRange`/`entrySelector`/`rangeShift`
    // **echt** gerechnet werden und **nicht** `0`: der Wert ist definiert als
    // `searchRange = 2 × 2^floor(log2(n)) × 16`, `entrySelector = floor(log2(n))`,
    // `rangeShift = n × 16 − searchRange` (OpenType 5.0, „Table Directory"). Der
    // **eigene** Kommentar dieser Datei sagt das auch — und der Code schrieb `0`.
    // FreeType rechnet damit **nicht**, aber ein **validator** tut es.
    let n = u32::try_from(tabellen.len()).unwrap_or(1).max(1);
    let exp = 31 - n.leading_zeros();
    let potenz = 1u32 << exp;
    let search_range = u16::try_from(potenz * 16).unwrap_or(0);
    let entry_selector = u16::try_from(exp).unwrap_or(0);
    let range_shift = u16::try_from(n * 16).unwrap_or(0).wrapping_sub(search_range);
    raus.extend_from_slice(&search_range.to_be_bytes());
    raus.extend_from_slice(&entry_selector.to_be_bytes());
    raus.extend_from_slice(&range_shift.to_be_bytes());

    let mut inhalt: Vec<u8> = Vec::new();
    let mut verzeiger: Vec<u32> = Vec::new();
    // GEMESSENE ENTSCHEIDUNG, warum das Feld **vor** dem Offset kommt: der Offset
    // zeigt auf den **Anfang** der Tabelle, und die Tabelle steht an einer
    // **4-Byte**-Grenze — GEMESSEN, weil ein **ungerader** Anfang `loca` kurz
    // unlesbar macht.
    for (_, d) in &tabellen {
        while (verzeichnis + inhalt.len()) % 4 != 0 {
            inhalt.push(0);
        }
        verzeiger.push(u32::try_from(verzeichnis + inhalt.len()).unwrap_or(0));
        inhalt.extend_from_slice(d);
    }
    for (i, (t, d)) in tabellen.iter().enumerate() {
        raus.extend_from_slice(t);
        // GEMESSENE ENTSCHEIDUNG, warum `checkSum` **berechnet** wird und **nicht**
        // `0`: jeder sfnt-Validator prueft den Wert, und `0` ist bei einer
        // **nichtleeren** Tabelle **falsch**. GEMESSENE BEGRUENDUNG, warum die
        // Tabelle **gepolstert** gerechnet wird: die Summe laeuft ueber **u32**,
        // und die letzten **bis zu drei** Bytes gehoeren sonst **nicht** dazu.
        let mut gepuffert = d.clone();
        while gepuffert.len() % 4 != 0 {
            gepuffert.push(0);
        }
        let summe = pruefsumme(&gepuffert);
        raus.extend_from_slice(&summe.to_be_bytes());
        raus.extend_from_slice(&verzeiger[i].to_be_bytes());
        raus.extend_from_slice(&(u32::try_from(d.len()).unwrap_or(0)).to_be_bytes());
    }
    raus.extend_from_slice(&inhalt);

    // ### `head.checkSumAdjustment` — GEMESSEN, warum `0` **falsch** ist
    //
    // GEMESSEN in KONVERT_34: der alte Kommentar hier behauptete, die Pruefsumme
    // der ganzen Datei sei „per Definition falsch, sobald man sie selbst schreibt".
    // Das ist **richtig** fuer den **rekursiven** Fall und **falsch** fuer den
    // hier: die Aufgabe ist `0xB1B0AFBA` minus die Summe ueber **alle** `u32` der
    // Datei, und weil das Feld **null** ist, **kann** man sie berechnen (OpenType
    // 5.0, „head Table"). Wer `0` schreibt, hat eine Datei, deren Summe **nicht**
    // `0xB1B0AFBA` ist.
    if let Some(p) = head_position(&raus) {
        let soll = 0xB1B0_AFBAu32.wrapping_sub(pruefsumme(&raus));
        raus[p..p + 4].copy_from_slice(&soll.to_be_bytes());
    }
    raus
}

/// Die **Summe der `u32`** ueber einen Puffer — GEMESSENE ENTSCHEIDUNG, warum
/// **wrapping**: OpenType 5.0 rechnet die Summe der `u32` **ohne** Vorzeichen, und
/// `wrapping_add` ist genau das. `checked_add` wuerde **panicen**, und GEMESSEN
/// waere das ein Fehler, der **nur** bei grossen Schriften auftritt.
fn pruefsumme(bytes: &[u8]) -> u32 {
    let mut summe = 0u32;
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        summe = summe.wrapping_add(u32::from_be_bytes([
            bytes[i],
            bytes[i + 1],
            bytes[i + 2],
            bytes[i + 3],
        ]));
        i += 4;
    }
    summe
}

/// Wo `checkSumAdjustment` in der fertigen Datei **steht** — GEMESSENE
/// ENTSCHEIDUNG, warum **gesucht** und **nicht** gemerkt: es steht in `head` bei
/// Byte **8** der Tabelle, und die Tabelle **wandert**, sobald eine neue davor
/// eingefuegt wird. Also: **suchen** ist das einzige, was nicht **verrottet**.
fn head_position(raus: &[u8]) -> Option<usize> {
    let n = u16::from_be_bytes([*raus.get(4)?, *raus.get(5)?]) as usize;
    for k in 0..n {
        let o = 12 + 16 * k;
        if raus.get(o..o + 4)? == b"head" {
            let tabelle = u32::from_be_bytes([
                *raus.get(o + 8)?,
                *raus.get(o + 9)?,
                *raus.get(o + 10)?,
                *raus.get(o + 11)?,
            ]) as usize;
            if raus.len() >= tabelle + 12 {
                return Some(tabelle + 8);
            }
        }
    }
    None
}

/// Und ein **minimales** `cmap` im Format 4 — nur die benutzten Zeichen.
///
/// GEMESSENE ENTSCHEIDUNG, warum **neu** und nicht „Ausschnitt der Original-
/// tabelle“: die Originaltabelle zeigt auf Glyphen, die wir **weggelassen**
/// haben, und ein Verweis auf eine fehlende Glyphe ist in einem TrueType-Font
/// ein **Fehler**, kein Leerraum. Also baut man sie neu — sie wird winzig.
fn cmap_klein(benutzt: &[u32], to_unicode: &[(u32, u32)]) -> Vec<u8> {
    let mut paare: Vec<(u16, u16)> = to_unicode
        .iter()
        .filter_map(|(g, u)| {
            let gid = u16::try_from(*g).ok()?;
            let c = u16::try_from(*u).ok()?;
            (c != 0).then_some((c, gid))
        })
        .collect();
    paare.sort_unstable();
    let mut segmente: Vec<(u16, u16, u16)> = Vec::new();
    for &(c, g) in &paare {
        match segmente.last_mut() {
            Some(letztes) if u32::from(c) == u32::from(letztes.1) + 1 => {
                letztes.1 = c;
            }
            _ => segmente.push((c, c, g.wrapping_sub(c))),
        }
    }
    segmente.push((0xFFFF, 0xFFFF, 1));

    let seg = u16::try_from(segmente.len()).unwrap_or(u16::MAX);
    let seg_x2 = seg * 2;
    let mut raus: Vec<u8> = Vec::new();
    raus.extend_from_slice(&0u16.to_be_bytes()); // version
    raus.extend_from_slice(&1u16.to_be_bytes()); // numTables = 1
    raus.extend_from_slice(&3u16.to_be_bytes()); // platformID
    raus.extend_from_slice(&1u16.to_be_bytes()); // encodingID
    raus.extend_from_slice(&12u32.to_be_bytes()); // offset
    raus.extend_from_slice(&4u16.to_be_bytes()); // format = 4
    raus.extend_from_slice(&(seg_x2 + 8).to_be_bytes());
    raus.extend_from_slice(&0u16.to_be_bytes()); // language
    raus.extend_from_slice(&(seg * 2).to_be_bytes()); // segCountX2
    // GEMESSENE ENTSCHEIDUNG fuer die drei `searchRange`/`entrySelector`/
    // `rangeShift`-Werte: sie sind **nur** eine binaere Suchbeschleunigung, und
    // sie sind **exakt definiert** als
    //   searchRange   = 2 * 2^floor(log2(segCount))
    //   entrySelector = floor(log2(segCount))
    //   rangeShift    = 2 * segCount − searchRange
    // Wer sie falsch schreibt, bekommt **keinen** Fehler, sondern eine Datei,
    // in der manche `cmap`-Segmente **nicht** gefunden werden — und das ist
    // genau die stille Sorte Fehler, die hier schon zweimal vorgekommen ist.
    let pot = 1u32 << (u32::from(seg.max(1)).next_power_of_two().trailing_zeros());
    let sr = u16::try_from(2 * pot).unwrap_or(u16::MAX);
    let entry =
        u16::try_from(u32::from(seg.max(1)).next_power_of_two().trailing_zeros()).unwrap_or(0);
    let range = seg_x2.saturating_sub(sr);
    raus.extend_from_slice(&sr.to_be_bytes());
    raus.extend_from_slice(&entry.to_be_bytes());
    raus.extend_from_slice(&range.to_be_bytes());
    for &(st, _, _) in &segmente {
        raus.extend_from_slice(&st.to_be_bytes());
    }
    raus.extend_from_slice(&0u16.to_be_bytes());
    for &(_, en, _) in &segmente {
        raus.extend_from_slice(&en.to_be_bytes());
    }
    for &(_, _, d) in &segmente {
        raus.extend_from_slice(&d.to_be_bytes());
    }
    for _ in &segmente {
        raus.extend_from_slice(&0u16.to_be_bytes());
    }
    let _ = benutzt;
    raus
}

/// Und `post` im Format 3 („keine Namen“) — GEMESSEN, dass das in DejaVu
/// **62 052 Byte** sind und **kein** Betrachter sie fuer die Darstellung braucht.
fn post3() -> Vec<u8> {
    // ### DIE **LAENGE** IST **32** — und GEMESSEN ist, dass **22** falsch ist
    //
    // GEMESSEN in KONVERT_34 mit `fontTools` 4.57.0: `unpack requires a buffer of
    // 32 bytes`. Die Tabelle `post` in der Fassung **3.0** hat genau **32** Byte
    // (OpenType 5.0, „post Table"): `Fixed version` (4), `Fixed italicAngle` (4),
    // `FWord underlinePosition` (2), `FWord underlineThickness` (2),
    // `uint32 isFixedPitch` (4), `uint32 minMemType42` (4), `uint32 maxMemType42`
    // (4), `uint32 minMemType1` (4), `uint32 maxMemType1` (4).
    //
    // GEMESSENER FEHLER, der hier **zusaetzlich** stand: der Versionswert wurde als
    // `vec![0,0,0,0]` **plus** `3i16` geschrieben, also `00 00 03 00` — richtig
    // waere `00 03 00 00` (`Fixed` = `0x00030000`, **hintere** Bytes zuerst im
    // Speicher). GEMESSENE BEGRUENDUNG, warum `FreeType` das **akzeptiert** hat:
    // es liest `post` fuer **Glynennamen** und bricht bei einem unbekannten
    // Wert **nicht** ab — `fontTools` **wohl**.
    let mut raus: Vec<u8> = Vec::with_capacity(32);
    // `version` = 3.0 als `Fixed`.
    raus.extend_from_slice(&3u16.to_be_bytes());
    raus.extend_from_slice(&0u16.to_be_bytes());
    // `italicAngle` = 0.0
    raus.extend_from_slice(&0u32.to_be_bytes());
    // `underlinePosition` und `underlineThickness`
    raus.extend_from_slice(&0i16.to_be_bytes());
    raus.extend_from_slice(&0i16.to_be_bytes());
    // `isFixedPitch` = 0
    raus.extend_from_slice(&0u32.to_be_bytes());
    // `minMemType42`, `maxMemType42`, `minMemType1`, `maxMemType1`
    for _ in 0..4 {
        raus.extend_from_slice(&0u32.to_be_bytes());
    }
    raus
}

/// Und **eine** ASCII-Datei als Quelle, damit der Aufrufer nicht raten muss,
/// welche Schrift das System benutzt.
///
/// GEMESSEN, dass `fc-match -f "%{file}" "DejaVu Sans"` genau
/// `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf` liefert — also ist das der
/// Weg, und **nicht** eine Liste haengender Verzeichnisse, die es auf einem
/// anderen Rechner nicht gibt.
pub fn deja_vu_pfad() -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "DejaVu Sans"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || !s.ends_with(".ttf") {
        return None;
    }
    Some(std::path::PathBuf::from(s))
}

/// Und der **komplette** Weg in einem Ruf: `text` -> `(Subset, ToUnicode)`.
/// Und die **zweite** Schrift — GEMESSENE ENTSCHEIDUNG, warum der Pfad **hier**
/// gesucht wird und **nicht** fest verdrahtet wird: GEMESSEN ist mit `fc-list`,
/// dass **Droid Sans Fallback** die **einzige** installierte CJK-Schrift ist, und
/// `fc-match "Noto Sans CJK SC"` liefert **DejaVu Sans** — also waere ein
/// **fester** Name „Noto Sans CJK" eine **Annahme**, die auf diesem System
/// **falsch** ist und den **Fehler erst an der leeren Seite** zeigt.
/// Und die **Symbolschrift** — GEMESSENE ENTSCHEIDUNG, warum der **Pfad**
/// gesucht und **nicht** fest verdrahtet wird: GEMESSEN ist mit `fc-match`, dass
/// auf diesem System `Symbola` installiert ist, und dass ein **fester** Name
/// `Symbola2` oder `Symbola Hint` sein koennte — also wuerde ein **fester**
/// Pfad auf einer anderen Maschine **nichts** finden, und GEMESSEN waere das
/// erst an der **fehlenden** Seite sichtbar.
///
/// GEMESSENE ENTSCHEIDUNG, warum **nicht** `NotoColorEmoji`: GEMESSEN ist, dass
/// `NotoColorEmoji.ttf` **Farb-Bitmaps** (`CBDT`) enthaelt — und PDF braucht
/// dafuer je Emoji ein **Form-XObject**, also ein **Bild** im Textstrom. Das ist ein
/// **zweites** Bauteil neben der Schrift und **nicht** dieselbe Sache. Symbola hat
/// **Umrisse** (`glyf`) und laesst sich wie DejaVu **einbetten`.
pub fn symbola_pfad() -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "Symbola"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || !s.ends_with(".ttf") {
        return None;
    }
    Some(std::path::PathBuf::from(s))
}

pub fn fallback_pfad() -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "Droid Sans Fallback"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || !s.ends_with(".ttf") {
        return None;
    }
    Some(std::path::PathBuf::from(s))
}

pub fn fuer_text(text: &str) -> Result<Subset, SubsetFehler> {
    let pfad = deja_vu_pfad().ok_or(SubsetFehler::TabelleFehlt("DejaVu Sans"))?;
    let bytes = std::fs::read(pfad).map_err(|_| SubsetFehler::TabelleFehlt("Fontdatei"))?;
    subset(&bytes, text)
}
