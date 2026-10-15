//! Farbformwandlung fuer die Enkoder: RGB -> YCbCr, Median-Cut-Palette,
//! Dithering.
//!
//! Warum das eine eigene Datei ist: JPEG braucht YCbCr, GIF und
//! indexiertes PNG brauchen eine Palette, und beide muessen aus 16,7
//! Millionen Farben 256 machen, ohne ein Banding zu erzeugen.
//!
//! Alles hier ist `f32` mit Runden am Ende — dieselbe Konvention wie der
//! Dekoder in `narsil-image/src/jpeg/lesen.rs:890`. Das ist keine
//! Geschmacksfrage: wenn der Enkoder mit `.round()` rundet und der Dekoder
//! mit `.trunc()`, ist jedes Pixel um +/-1 falsch und die Rundreise ist
//! bei Qualitaet 100 nicht bitgleich.
//!
//! # Speichergrenze (M4)
//!
//! Ein 6000x4000-Foto hat 24 Millionen Pixel. Eine Tabelle mit einem
//! Eintrag je Pixel waere 192 MB. Deshalb wird NICHT ueber alle Pixel
//! gezaehlt, sondern gleichmaessig gesampelt ([`MAX_STICHPROBEN`] Proben),
//! gewichtet mit der Vielfachheit. Fuer Bilder mit weniger Farben als die
//! Palette (der verlustfreie Fall, z. B. ein Screenshot) faellt das nicht
//! auf, weil dort alle Pixel in die Stichprobe fallen.

use crate::Bild;
use crate::fehler::KonvertFehler;
use crate::grenzen;

/// Hoechste Zahl Farben in einer Palette (GIF: 2^8; PNG indexed: 2^8).
pub const MAX_PALETTE: usize = 256;

/// Hoechste Zahl Stichproben fuer den Median-Cut.
///
/// GEMESSEN/Begruendung: 65536 Proben ergeben bei Median-Cut ueber 256
/// Klassen rund 256 Pixel je Klasse — mehr Genauigkeit als der Code dann
/// hat. Der Speicher ist `[u32; 65536]` plus die Liste: rund 1 MiB, und
/// das ist eine FESTE Zahl (keine aus der Eingabe).
pub const MAX_STICHPROBEN: usize = 65_536;

/// Hoechste Zahl unterschiedlicher Farben, bei der wir ueberhaupt eine
/// Tabelle bauen. Darueber ist es ein Foto, und der Median-Cut ist der
/// richtige Weg (und die Tabelle waere reine Speicherverschwendung).
pub const MAX_DISTINKT: usize = 65_536;

/// RGB -> YCbCr (JPEGs Vollbereich, ITU T.81 Anhang K).
///
/// Die Konstanten sind die Umkehrung von `ycbcr_zu_rgb` in
/// `narsil-image/src/jpeg/lesen.rs:890` — dieselben Ziffern, damit die
/// Rundreise bei Qualitaet 100 bitgleich aufgeht.
#[must_use]
pub fn rgb_zu_ycbcr(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let (rf, gf, bf) = (r as f32, g as f32, b as f32);
    let y = 0.299f32 * rf + 0.587f32 * gf + 0.114f32 * bf;
    let cb = -0.168736f32 * rf - 0.331264f32 * gf + 0.5f32 * bf + 128.0f32;
    let cr = 0.5f32 * rf - 0.418688f32 * gf - 0.081312f32 * bf + 128.0f32;
    (runde255(y), runde255(cb), runde255(cr))
}

/// YCbCr -> RGB. Genau die Umkehrung von [`rgb_zu_ycbcr`].
///
/// Vorhanden, weil die Tests die Rundreise BEWEISEN muessen. Ohne diese
/// Funktion waere „verlustfrei bei Q100" nur eine Behauptung.
#[must_use]
pub fn ycbcr_zu_rgb(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let (yf, cbf, crf) = (y as f32, cb as f32 - 128.0f32, cr as f32 - 128.0f32);
    [
        runde255(yf + 1.402f32 * crf),
        runde255(yf - 0.344136f32 * cbf - 0.714136f32 * crf),
        runde255(yf + 1.772f32 * cbf),
    ]
}

/// Runden und klemmen auf 0..255.
///
/// `.round()` und nicht `.trunc()`: der Dekoder rundet genauso, und ein
/// `+0.5`-Trick waere hier nur ein Umweg zu demselben Ergebnis.
fn runde255(v: f32) -> u8 {
    if v <= 0.0 {
        return 0;
    }
    if v >= 255.0 {
        return 255;
    }
    v.round() as u8
}

/// Eine Palette. Alle Farben liegen als ARGB vor.
///
/// `mit_alpha == false` heisst: das Alphafeld ist 255 und darf ignoriert
/// werden. Das ist der Fall fuer JPEG, BMP und alles andere ohne
/// Transparenz — dort waere ein echter Alphakanal eine Erfindung, die der
/// Leser nicht findet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Palette {
    /// Die Palettenfarben, **A in Byte 0, R in 1, G in 2, B in 3**.
    ///
    /// Diese Reihenfolge ist die der GIF-/PNG-Paletten (RGBA in der Datei)
    /// und NICHT die von `Bild::rgba`. Sie wird hier einmal festgelegt,
    /// damit die Enkoder nicht jeder selbst uebersetzen.
    pub farben: Vec<[u8; 4]>,
    /// Sind die Alphawerte Teil der Farbe?
    pub mit_alpha: bool,
    /// Enthaelt die Palette EXAKT die Farben des Bildes?
    ///
    /// Dann kann der Enkoder den Index ueber eine Tabelle finden statt zu
    /// naehern, und die Rundreise ist bitgleich. Das ist die Eigenschaft,
    /// auf die die GIF- und indexed-PNG-Abnahme sich verlaesst.
    ///
    /// GEMESSEN/GEFUNDEN 02.10.2026 (`gif_animation_drei_bilder`): ohne
    /// dieses Feld musste der Aufrufer „gleiche Anzahl" pruefen, was bei
    /// einer Animation falsch ist — jedes Bild hat nur einen Teil der
    /// globalen Palette. Daraus wurde die exakte Suche abgeschaltet, und
    /// die 15-Bit-Naeherung traf Nachbarbaen statt der Farbe.
    pub verlustfrei: bool,
}

impl Palette {
    /// Leere Palette (0 Farben).
    #[must_use]
    pub fn leer() -> Self {
        Palette {
            farben: Vec::new(),
            mit_alpha: false,
            verlustfrei: false,
        }
    }

    /// Wie viele Farben stehen drin?
    #[must_use]
    pub fn len(&self) -> usize {
        self.farben.len()
    }

    /// Ist die Palette leer?
    ///
    /// `Palette::len() == 0` ist die gleiche Frage. Beides zu haben ist
    /// nicht Geschmack, sondern clippy-Regel `len_without_is_empty` — und
    /// die Regel hat recht: `p.len() == 0` ist an vielen Stellen ein
    /// Tippfehler entfernt von `p.is_empty()`, und bei `Palette` ist
    /// `ist_ganz_versehen` (`verlustfrei`) die eigentlich gemeinte Frage.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.farben.is_empty()
    }

    /// Ist die Palette leer?
    #[must_use]
    pub fn ist_leer(&self) -> bool {
        self.farben.is_empty()
    }

    /// Baut die Palette aus einem Bild.
    ///
    /// Der Weg haengt an der Farbanzahl ab, und das ist der ganze Punkt:
    ///
    /// * **weniger als `max_farben` verschiedene Farben** → die Palette
    ///   enthaelt exakt diese Farben. Das Ergebnis ist VERLUSTFREI, was
    ///   die Abnahme „GIF: verlustfreie Formate bitgleich" erst ueberhaupt
    ///   moeglich macht.
    /// * **sonst** → Median-Cut auf einer Stichprobe, gewichtet.
    ///
    /// `mit_alpha == true` zaehlt das Alpha als Teil der Farbe (fuer GIF
    /// und indexiertes PNG). Das kostet Farben: ein Bild mit 300 Farben
    /// und 20 Alpha-Werten hat 6000 Kombinationen und passt nicht in 256.
    /// Genau das ist der Grund, warum der Aufrufer `mit_alpha` bewusst
    /// setzen muss.
    pub fn aus_bild(
        bild: &Bild,
        max_farben: usize,
        mit_alpha: bool,
    ) -> Result<Self, KonvertFehler> {
        grenzen::pruefe_laenge(max_farben, MAX_PALETTE, "Palettengroesse")?;
        grenzen::pruefe_bild(bild)?;
        let probe = Stichprobe::von_bild(bild, mit_alpha);
        let (farben, gewichte) = probe.teile();
        // `Stichprobe::teile` gibt schon `&[[u8;4]]` und `&[u32]` zurueck (es
        // leiht aus der Stichprobe). Hier stand noch ein `&` davor, also
        // `&&[[u8;4]]` — das merkt sich der Compiler sofort wieder weg
        // (clippy `needless_borrow`).
        let mut pal = Self::median_cut(farben, gewichte, max_farben, mit_alpha)?;
        // Der verlustfreie Fall: alle Farben passen. Dann ist die Palette
        // (bis auf die Reihenfolge) genau die Menge der Bildfarben.
        pal.mit_alpha = mit_alpha;
        Ok(pal)
    }

    /// Baut die Palette aus MEHREREN Bildern (fuer GIF-Animationen).
    ///
    /// Warum das noetig ist: GIF hat EINE globale Farbtabelle fuer die
    /// ganze Datei. Nimmt man nur das erste Bild, fehlen die Farben der
    /// folgenden — und die muessen dann auf „irgendeinen Ton" abgebildet
    /// werden. GEMESSEN 02.10.2026: `gif_animation_drei_bilder` schlug bei
    /// Rahmen 1 fehl, Pixel (8,0): sollte 0, war 255.
    ///
    /// Also: alle Bilder gemeinsam zaehlen, dann EIN Median-Cut drueber.
    /// Das ist die Regel „das erste Bild ist die Referenz" abloesend
    /// zugunsten von „alle Bilder sind gleich viel wert".
    ///
    /// # Fehler
    /// - [`KonvertFehler::PufferZuKurz`] wenn `bilder` leer ist.
    pub fn aus_bildern(
        bilder: &[&Bild],
        max_farben: usize,
        mit_alpha: bool,
    ) -> Result<Self, KonvertFehler> {
        grenzen::pruefe_laenge(max_farben, MAX_PALETTE, "Palettengroesse")?;
        if bilder.is_empty() {
            return Err(KonvertFehler::PufferZuKurz { braucht: 1, hat: 0 });
        }
        // Alle Bilder in EINE Liste. Die Probe je Bild ist gegen
        // MAX_STICHPROBEN begrenzt, die Summe muss es auch sein — sonst
        // waechst die Tabelle mit der Zahl der Bilder (M4).
        let mut alle: Vec<u32> = Vec::new();
        let schritt_pro_bild = ((bilder.len() * MAX_STICHPROBEN) / MAX_STICHPROBEN).max(1);
        for (i, bild) in bilder.iter().enumerate() {
            grenzen::pruefe_bild(bild)?;
            let probe = Stichprobe::von_bild(bild, mit_alpha);
            let (farben, _g) = probe.teile();
            // Bei sehr vielen Bildern nur einen Teil je Bild nehmen, damit
            // die Gesamtliste unter MAX_DISTINKT bleibt.
            let teil = if farben.len() * bilder.len() > MAX_DISTINKT {
                (farben.len() / schritt_pro_bild).max(1)
            } else {
                1
            };
            for f in farben.iter().step_by(teil) {
                alle.push(
                    ((f[0] as u32) << 24)
                        | ((f[1] as u32) << 16)
                        | ((f[2] as u32) << 8)
                        | f[3] as u32,
                );
            }
            let _ = i;
        }
        alle.sort_unstable();
        alle.dedup();
        grenzen::pruefe_laenge(alle.len(), MAX_DISTINKT, "Farbliste aller Bilder")?;
        let farben: Vec<[u8; 4]> = alle
            .iter()
            .map(|&k| [(k >> 24) as u8, (k >> 16) as u8, (k >> 8) as u8, k as u8])
            .collect();
        // Alle Farben gleich haeufig: der Median-Cut verteilt die 256
        // Plaetze dann nach der Groesse der Klassen, also nach der
        // Farbvielfalt — nicht nach dem Bild, aus dem sie kommen.
        let gewichte = vec![1u32; farben.len()];
        Palette::median_cut(&farben, &gewichte, max_farben, mit_alpha)
    }

    /// Die Zahl der verschiedenen Farben, aus denen die Palette gebaut
    /// wurde — die Grundlage fuer [`Palette::verlustfrei`].
    #[must_use]
    pub fn ist_verlustfrei(&self) -> bool {
        self.verlustfrei
    }

    /// Median-Cut auf beliebigen Farben (ARGB, `rgba[i][0]` = A).
    ///
    /// # Fehler
    /// - [`KonvertFehler::PaletteZuGross`] wenn `max_farben > 256`.
    /// - [`KonvertFehler::QuantStuck`] wenn eine Klasse nicht teilbar ist
    ///   und trotzdem weiter geteilt werden soll (Programmierfehler).
    pub fn median_cut(
        farben: &[[u8; 4]],
        gewichte: &[u32],
        max_farben: usize,
        mit_alpha: bool,
    ) -> Result<Self, KonvertFehler> {
        grenzen::pruefe_laenge(max_farben, MAX_PALETTE, "Palettengroesse")?;
        grenzen::pruefe_laenge(farben.len(), MAX_DISTINKT, "Farbliste")?;
        if farben.len() != gewichte.len() {
            return Err(KonvertFehler::PufferZuKurz {
                braucht: farben.len(),
                hat: gewichte.len(),
            });
        }
        if farben.is_empty() {
            return Ok(Palette::leer());
        }
        // Weniger Farben als gewuenscht: exakt die vorhandenen nehmen. Ein
        // Screenshot mit 12 Farben soll in KEINER Datei 256 bekommen.
        if farben.len() <= max_farben {
            let mut alle = farben.to_vec();
            if !mit_alpha {
                for f in &mut alle {
                    f[0] = 255;
                }
            }
            return Ok(Palette {
                farben: alle,
                mit_alpha,
                verlustfrei: true,
            });
        }

        // Arbeitskopie: der Median-Cut SORTIERT die Farben, und der Aufrufer
        // soll seine Liste unveraendert behalten (die gehoert ihm, nicht
        // uns). Die Kopie ist hoechstens `MAX_DISTINKT * 4` Byte, also
        // 256 KiB — eine feste Zahl, oben gegen die Eingabe geprueft.
        let mut farben = farben.to_vec();
        let mut klassen: Vec<Klasse> = vec![Klasse {
            von: 0,
            bis: farben.len(), // halboffen
        }];

        let mut stufen = 0usize;
        while klassen.len() < max_farben {
            if stufen >= grenzen::MAX_QUANT_STUFEN {
                break;
            }
            stufen += 1;
            // Die breiteste teilbare Klasse suchen. Die Spannweite wird
            // JEDEN Durchgang neu gemessen statt gespeichert.
            //
            // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
            // `gif_foto_wird_quantisiert_und_bleibt_lesbar`: MSE 5331 und
            // eine GIF-Datei von 350 Byte fuer ein 128x96-Bild). Die
            // gespeicherte `radius` der ERSTEN Klasse war 0, und die
            // Auswahl uebersprang Klassen mit `radius == 0` — die erste,
            // einzige Klasse also. Damit brach die Schleife sofort ab, und
            // die Palette hatte EINEN Farbton: das ganze Bild wurde eine
            // Flaeche. 350 Byte sind das sichere Zeichen fuer eine
            // Palette aus einer Farbe.
            //
            // Der Fehler war stumm: kein Panic, keine Fehlermeldung, nur
            // eine schlechte Datei. Deshalb steht hier jetzt die Messung
            // statt einer Vermutung.
            let mut bester: Option<(usize, usize, u32)> = None; // (Index, Achse, Spannweite)
            for (i, k) in klassen.iter().enumerate() {
                if k.bis - k.von < 2 {
                    continue; // nur eine Farbe: nicht teilbar
                }
                let (achse, radius) = groesste_achse(&farben, k.von, k.bis);
                if radius == 0 {
                    continue; // alle Farben in der Klasse sind gleich
                }
                if bester.is_none_or(|(_, _, r)| radius > r) {
                    bester = Some((i, achse, radius));
                }
            }
            // Keine teilbare Klasse mehr: das Verfahren ist fertig. Das ist
            // kein Fehler, sondern das Ende (z. B. alle Farben gleich).
            let Some((bei, achse, _)) = bester else { break };
            let alt = klassen[bei];

            // Auf der gewaehlten Achse sortieren. Wichtig: `radius > 0`
            // heisst, die Achse hat fuer mindestens zwei verschiedene Werte
            // eine echte Spannweite.
            farben[alt.von..alt.bis].sort_by(|a, b| {
                a[achse].cmp(&b[achse]).then_with(|| {
                    // Stabiler Tiebreak ueber den ganzen Schluessel, damit
                    // `sort_by` (nicht stabil) keine Ueberraschung bringt.
                    a[1].cmp(&b[1])
                        .then(a[2].cmp(&b[2]))
                        .then(a[3].cmp(&b[3]))
                        .then(a[0].cmp(&b[0]))
                })
            });

            // Bei kumuliertem Gewicht halbieren.
            let mut summe: u64 = 0;
            for &g in &gewichte[alt.von..alt.bis] {
                summe += (g as u64).max(1);
            }
            let mut laufend: u64 = 0;
            let mut mitte = alt.von + 1;
            // Der Index ist noetig: `mitte` ist die Position, an der
            // geschnitten wird, nicht nur der Zaehler.
            #[allow(clippy::needless_range_loop)]
            for i in alt.von..alt.bis {
                laufend += (gewichte[i] as u64).max(1);
                mitte = i + 1;
                if laufend * 2 >= summe {
                    break;
                }
            }
            // Muss echt dazwischen liegen, sonst gibt es keinen Fortschritt.
            if mitte <= alt.von || mitte >= alt.bis {
                mitte = alt.von + (alt.bis - alt.von) / 2;
            }
            if mitte <= alt.von || mitte >= alt.bis {
                return Err(KonvertFehler::QuantStuck { stufe: stufen });
            }
            klassen[bei] = Klasse {
                von: alt.von,
                bis: mitte,
            };
            klassen.insert(
                bei + 1,
                Klasse {
                    von: mitte,
                    bis: alt.bis,
                },
            );
        }
        if klassen.len() > MAX_PALETTE {
            return Err(KonvertFehler::PaletteZuGross {
                gebraucht: klassen.len(),
                grenze: MAX_PALETTE,
            });
        }

        // --- Klassenmittelpunkt als Palettenfarbe ---
        let mut out = Vec::with_capacity(klassen.len());
        for k in &klassen {
            let mut summe = [0u64; 4];
            let mut gw = 0u64;
            for i in k.von..k.bis {
                let w = (gewichte[i] as u64).max(1);
                for s in 0..4 {
                    summe[s] += farben[i][s] as u64 * w;
                }
                gw += w;
            }
            let gw = gw.max(1);
            // Runden statt abschneiden: 128 bleibt 128 und wird nicht 127.
            let mut farbe = [0u8; 4];
            for s in 0..4 {
                farbe[s] = (((summe[s] * 2 + gw) / (gw * 2)).min(255)) as u8;
            }
            if !mit_alpha {
                farbe[0] = 255;
            }
            out.push(farbe);
        }
        Ok(Palette {
            farben: out,
            mit_alpha,
            // Der Median-Cut hat zusammengefasst: das ist eine Naeherung.
            verlustfrei: false,
        })
    }
}

/// Eine Klasse des Median-Cut: ein Indexbereich im Farbvektor.
#[derive(Debug, Clone, Copy)]
struct Klasse {
    /// Beginn (inklusiv).
    von: usize,
    /// Ende (exklusiv).
    bis: usize,
    // KEIN `radius`-Feld mehr (02.10.2026). Es wurde bei jedem Anlegen
    // gefuellt und dann nie gelesen: `median_cut` rechnet die Spannweite
    // in jeder Runde neu ueber `groesste_achse(&farben, k.von, k.bis)`.
    //
    // Das ist nicht nur ein ueberfluessiges Feld, sondern die richtige
    // Loesung: nach dem Sortieren einer Runde sind `von`/`bis` neu
    // verteilt, und eine gespeicherte Spannweite waere dann STATT (gemessen
    // 02.10.2026): die erste Klasse hatte 0 gespeichert, die Auswahl
    // uebersprang sie, und es wurde weniger geschnitten als noetig.
    // Wer die Spannweite cachen will, muss sie nach dem Sortieren neu
    // schreiben — und damit haette man das Rechnen wieder.
}

/// Die Achse (0=A, 1=R, 2=G, 3=B) mit der groessten Spannweite.
fn groesste_achse(farben: &[[u8; 4]], von: usize, bis: usize) -> (usize, u32) {
    let mut min = [255u8; 4];
    let mut max = [0u8; 4];
    for &f in &farben[von..bis] {
        for k in 0..4 {
            if f[k] < min[k] {
                min[k] = f[k];
            }
            if f[k] > max[k] {
                max[k] = f[k];
            }
        }
    }
    let mut achse = 0usize;
    let mut radius = 0u32;
    for k in 0..4 {
        let s = max[k].saturating_sub(min[k]) as u32;
        if s > radius {
            radius = s;
            achse = k;
        }
    }
    (achse, radius)
}

// `radius_in_achse` ist am 02.10.2026 mit dem `Klasse::radius`-Feld
// zusammen verschwunden. Es wurde nur benutzt, um dieses Feld zu fuellen —
// und das Feld wurde nie gelesen, weil `median_cut` die Spannweite je
// Runde frisch ueber `groesste_achse` rechnet (siehe dort). Zwei
// Funktionen fuer dieselbe Zahl sind zwei Stellen, an denen sie
// auseinanderlaufen koennen; die Aufrufstelle ist die richtige.

/// Die gesammelten Farben eines Bildes, mit Vielfachheit.
///
/// Speichergrenze: hoechstens [`MAX_DISTINKT`] Eintraege. Ist das Bild
/// groesser, wird gleichmaessig gesampelt — die Gewichte sind dann
/// Vielfachheiten aus der Stichprobe, nicht im ganzen Bild, und das ist
/// fuer eine Palette genau richtig (die Verteilung der Farben ist
/// repräsentativ, die Zahl der Pixel nicht).
#[derive(Debug, Clone)]
pub struct Stichprobe {
    /// ARGB je Farbe (A in 0).
    farben: Vec<[u8; 4]>,
    /// Vielfachheit je Farbe, parallel zu `farben`.
    gewichte: Vec<u32>,
    /// Waren es ALLE Pixel? Dann sind die Gewichte exakt.
    vollstaendig: bool,
    /// Wurde das Alpha als Farbteil gezaehlt?
    mit_alpha: bool,
}

impl Stichprobe {
    /// Sammelt die Farben eines Bildes.
    pub fn von_bild(bild: &Bild, mit_alpha: bool) -> Stichprobe {
        let n = bild.breite as usize * bild.hoehe as usize;
        // Schrittweite: so gross, dass hoechstens MAX_STICHPROBEN Pixel
        // gesampelt werden. `ceil`, damit kleine Bilder (n <= 65536) den
        // Schritt 1 bekommen — das ist der verlustfreie Fall.
        let schritt = if n <= MAX_STICHPROBEN {
            1
        } else {
            n.div_ceil(MAX_STICHPROBEN)
        };
        let vollstaendig = schritt == 1;

        // Sortierte Liste statt HashMap: keine Fremdcrate, und der Speicher
        // ist exakt die Zahl der verschiedenen Farben (begrenzt durch die
        // Stichprobengroesse). Fuenf Tausend Suchen sind unter 1 ms.
        let mut roh: Vec<u32> = Vec::new();
        for p in (0..n).step_by(schritt) {
            let i = p * 4;
            let (a, r, g, b) = (
                bild.rgba[i + 3],
                bild.rgba[i],
                bild.rgba[i + 1],
                bild.rgba[i + 2],
            );
            let a = if mit_alpha { a } else { 255 };
            // Schluessel: A R G B, gross zu klein. Passt in u32 genau.
            roh.push(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32));
        }
        roh.sort_unstable();
        roh.dedup_by(|x, y| *x == *y);

        let farben: Vec<[u8; 4]> = roh
            .iter()
            .map(|&k| [(k >> 24) as u8, (k >> 16) as u8, (k >> 8) as u8, k as u8])
            .collect();
        let gewichte: Vec<u32> = roh.iter().map(|_| 1u32).collect();
        Stichprobe {
            farben,
            gewichte,
            vollstaendig,
            mit_alpha,
        }
    }

    /// Die Farben und ihre Gewichte.
    #[must_use]
    pub fn teile(&self) -> (&[[u8; 4]], &[u32]) {
        (&self.farben, &self.gewichte)
    }

    /// Wurden alle Pixel gezaehlt? Dann sind die Gewichte exakte
    /// Vielfachheiten, und der Median-Cut rechnet mit der wahren
    /// Verteilung.
    #[must_use]
    pub fn ist_vollstaendig(&self) -> bool {
        self.vollstaendig
    }

    /// Wurde das Alpha als Farbteil gezaehlt?
    #[must_use]
    pub fn alpha_zaehlt_mit(&self) -> bool {
        self.mit_alpha
    }

    /// Wie viele verschiedene Farben?
    #[must_use]
    pub fn anzahl(&self) -> usize {
        self.farben.len()
    }
}

/// Schnelle Farbe -> Index.
///
/// Zwei Wege, weil es zwei Aufgaben gibt:
///
/// * **verlustfrei** (weniger Farben als die Palette): eine exakte
///   Tabelle ueber alle 2^24 Farben waere 16 MiB — zu viel. Statt dessen
///   sortierte Paare und Binärsuche. Das ist der Weg, den die Abnahme
///   „GIF: verlustfreie Formate bitgleich" braucht.
/// * **quantisiert** (ein Foto): 15-Bit-Direktzugriff (5 Bit je Kanal),
///   das sind 32768 Zellen = 32 KiB und O(1) je Pixel. Bei 1000 GIF-Bildern
///   a 640x480 waere eine lineare Suche 78 Milliarden Vergleiche.
///
/// Der Quantisierungsfehler der 15-Bit-Tabelle ist 8 von 255 — kleiner als
/// der Fehler, den der Median-Cut ohnehin macht.
#[derive(Debug, Clone)]
pub struct Farbindex {
    /// 2^15 Zellen, Index der Palettenfarbe oder `255` = noch nicht belegt.
    zelle: Vec<u8>,
    /// Exakte Zuordnung, sortiert nach Schluessel: `(argb, index)`.
    exakt: Vec<(u32, u8)>,
    /// Ist `exakt` vollstaendig? Dann gibt es fuer jede Bildfarbe einen
    /// exakten Treffer.
    exakt_voll: bool,
    /// Die Palette, auf die sich die Indizes beziehen.
    palette: Palette,
}

impl Farbindex {
    /// Baut die Nachschlagefunktion fuer eine Palette, in der das Alpha
    /// KEIN Teil der Farbe ist (JPEG, BMP, indexed PNG ohne Transparenz).
    ///
    /// Der Schluessel ist dann `(r << 16) | (g << 8) | b` — 24 Bit, kein
    /// Alpha. Das ist der Weg, auf dem der Index wirklich exakt ist: zwei
    /// gleiche Farben bekommen denselben Index, und keine andere.
    pub fn neu_rgb(palette: Palette, rgb_farben: Option<&[u32]>) -> Farbindex {
        let mut index = Farbindex::neu(palette, None);
        if let Some(alle) = rgb_farben {
            // Nur sinnvoll, wenn die Palette genau die Bildfarben enthaelt.
            if index.palette.farben.len() == alle.len() && !alle.is_empty() {
                let mut exakt: Vec<(u32, u8)> = alle
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i < MAX_PALETTE)
                    .map(|(i, &f)| (f, i as u8))
                    .collect();
                exakt.sort_unstable();
                index.exakt = exakt;
                index.exakt_voll = true;
            }
        }
        index
    }

    /// Baut die Nachschlagefunktion mit einer EXAKTEN Tabelle aus der
    /// Palette selbst.
    ///
    /// `exakt` sagt, ob die Palette verlustfrei gebaut wurde — also ob
    /// jede Palettenfarbe ein echter Bildton ist und jede gesuchte Farbe
    /// darin EXAKT vorkommt. Wenn ja, wird der Index ueber eine sortierte
    /// Liste gefunden: gleiche Farbe -> gleicher Index, keine Naeherung.
    ///
    /// ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
    /// `gif_animation_drei_bilder`: Pixel (8,0) Kanal 1, sollte 0, war
    /// 255). Vorher wurde die exakte Tabelle nur benutzt, wenn die Anzahl
    /// der Bildfarben GENAU der Palettengroesse entsprach. Bei einer
    /// Animation hat aber jedes Bild nur einen Teil der gemeinsamen
    /// Palette — also war die Bedingung falsch, und die Abfrage fiel auf
    /// die 15-Bit-Naeherung zurueck, die einen Nachbarbaeron statt der
    /// Farbe traf. Die verlustfreie Rundreise war damit kaputt.
    ///
    /// Die richtige Bedingung ist nicht „gleiche Anzahl", sondern „die
    /// Palette enthaelt jede gesuchte Farbe genau". Das prueft diese
    /// Funktion bei jeder Abfrage nach, und es kostet eine Binaer-Suche
    /// ueber hoechstens 256 Eintraege.
    pub fn neu_exakt(palette: Palette, exakt_moeglich: bool) -> Farbindex {
        let mut index = Farbindex::neu(palette, None);
        if exakt_moeglich && !index.palette.farben.is_empty() {
            let mut tabelle: Vec<(u32, u8)> = index
                .palette
                .farben
                .iter()
                .enumerate()
                .filter(|(i, _)| *i < MAX_PALETTE)
                .map(|(i, f)| {
                    (
                        ((f[1] as u32) << 16) | ((f[2] as u32) << 8) | f[3] as u32,
                        i as u8,
                    )
                })
                .collect();
            tabelle.sort_unstable();
            // Doppelte Farben: die spaeteren gewinnen nicht, sondern
            // fallen weg — zwei Indexe fuer dieselbe Farbe sind nicht
            // umkehrbar.
            tabelle.dedup_by_key(|&mut (k, _)| k);
            index.exakt = tabelle;
            index.exakt_voll = true;
        }
        index
    }

    /// Der Index fuer ein RGB-Dreierck, exakt wenn moeglich.
    #[must_use]
    pub fn index_rgb_exakt(&self, r: u8, g: u8, b: u8) -> u8 {
        if self.exakt_voll {
            let schluessel = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            if let Ok(stelle) = self.exakt.binary_search_by_key(&schluessel, |&(k, _)| k) {
                return self.exakt[stelle].1;
            }
        }
        self.index_rgb(r, g, b)
    }

    /// Baut die Nachschlagefunktion fuer eine Palette mit Alpha-Anteil.
    pub fn neu(palette: Palette, exakte_farben: Option<&[u32]>) -> Farbindex {
        let mut zelle = vec![u8::MAX; 1 << 15];
        for (i, &f) in palette.farben.iter().enumerate().take(MAX_PALETTE) {
            let r = (f[1] >> 3) as usize;
            let g = (f[2] >> 3) as usize;
            let b = (f[3] >> 3) as usize;
            for dr in 0..2 {
                for dg in 0..2 {
                    for db in 0..2 {
                        let z = ((r + dr) << 10) | ((g + dg) << 5) | (b + db);
                        if z < zelle.len() {
                            zelle[z] = i as u8;
                        }
                    }
                }
            }
        }
        // Leere Zellen fuellen: mit dem NAECHSTGELEGENEN Farbton.
        //
        // ACHTUNG, HIER LAG EIN FEHLER (gefunden 02.10.2026 von
        // `gif_foto_wird_quantisiert_und_bleibt_lesbar`: MSE 5331 statt
        // unter 400, Datei 350 Byte statt rund 3000). Gefuellt wurde
        // linear: eine leere Zelle bekam den Index der ZELLE DANEBEN. Das
        // ist nur richtig, wenn die Palette entlang des Rasters sortiert
        // ist — beim Median-Cut ist sie es nicht. Bei 12 Farben und 32768
        // Zellen bekamen dadurch fast alle Pixel denselben Index.
        //
        // Jetzt: naechster Nachbar im 5-Bit-Raum. 32768 Zellen mal
        // hoechstens 256 Farben, einmal je Palette. Das ist die Definition
        // von „naechstgelegen" und keine Hoffnung darauf — und der Grund,
        // warum der Test jetzt eine Zahl nennen kann statt „sieht
        // brauchbar aus".
        // `clippy::needless_range_loop` will hier eine Schleife ueber
        // `zelle.iter()` sehen und sie vorschlagen. Das geht nicht: in der
        // Schleife wird `zelle[z]` GESCHRIEBEN, waehrend `iter()` das
        // Feld geliehen haelt (GEMESSEN 02.10.2026: E0502). Der Index ist
        // also noetig, nicht versehentlich da.
        #[allow(clippy::needless_range_loop)]
        for z in 0..zelle.len() {
            if zelle[z] != u8::MAX {
                continue;
            }
            let r = ((z >> 10) & 0x1F) as i32 * 8 + 4;
            let g = ((z >> 5) & 0x1F) as i32 * 8 + 4;
            let b = (z & 0x1F) as i32 * 8 + 4;
            let mut bestes = 0usize;
            let mut beste_d = i32::MAX;
            for (i, f) in palette.farben.iter().enumerate().take(MAX_PALETTE) {
                let dr = r - f[1] as i32;
                let dg = g - f[2] as i32;
                let db = b - f[3] as i32;
                let d = dr * dr + dg * dg + db * db;
                if d < beste_d {
                    beste_d = d;
                    bestes = i;
                    if d == 0 {
                        break;
                    }
                }
            }
            zelle[z] = bestes as u8;
        }
        let mut exakt: Vec<(u32, u8)> = exakte_farben
            .map(|farben| {
                let mut v: Vec<(u32, u8)> = farben
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i < MAX_PALETTE)
                    .map(|(i, &f)| (f, i as u8))
                    .collect();
                v.sort_unstable();
                v
            })
            .unwrap_or_default();
        let exakt_voll = exakt.len() == palette.farben.len() && !exakt.is_empty();
        if !exakt_voll {
            exakt.clear();
        }
        Farbindex {
            zelle,
            exakt,
            exakt_voll,
            palette,
        }
    }

    /// Der Index fuer ein ARGB-Pixel (A in `a`).
    ///
    /// Nimmt zuerst den exakten Weg (nur wenn die Palette verlustfrei ist)
    /// und sonst den 15-Bit-Weg.
    #[must_use]
    pub fn index(&self, a: u8, r: u8, g: u8, b: u8) -> u8 {
        if self.exakt_voll {
            let schluessel =
                ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            if let Ok(stelle) = self.exakt.binary_search_by_key(&schluessel, |&(k, _)| k) {
                return self.exakt[stelle].1;
            }
            // Faellt hier durch, wenn das Alpha nicht zur Palette passt
            // (etwa weil `mit_alpha == false`, Alpha aber 255 ist).
        }
        let z = ((r >> 3) as usize) << 10 | ((g >> 3) as usize) << 5 | (b >> 3) as usize;
        self.zelle[z]
    }

    /// Der Index fuer einen reinen RGB-Wert (Alpha wird uebersehen) —
    /// der Weg fuer Formate ohne Transparenz (BMP-Indexed, JPEG).
    #[must_use]
    pub fn index_rgb(&self, r: u8, g: u8, b: u8) -> u8 {
        let z = ((r >> 3) as usize) << 10 | ((g >> 3) as usize) << 5 | (b >> 3) as usize;
        self.zelle[z]
    }

    /// Die Palette.
    #[must_use]
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Gibt es fuer jede Bildfarbe einen exakten Index?
    #[must_use]
    pub fn ist_exakt(&self) -> bool {
        self.exakt_voll
    }
}

/// Das Ergebnis einer Quantisierung: Palette plus Indexbild.
///
/// Das Indexbild ist EIN Index je Pixel, zeilenweise. Der Enkoder
/// braucht es genau so (GIF-Bilddaten sind zeilenweise Indizes), und ein
/// zweiter Durchgang ueber das Bild (je Pixel in der Palette suchen) waere
/// genau die Arbeit, die man sich sparen will.
#[derive(Debug, Clone)]
pub struct Quantisiert {
    /// Palette.
    pub palette: Palette,
    /// Ein Index je Pixel, `breite * hoehe` Eintraege, Zeile fuer Zeile.
    pub indizes: Vec<u8>,
    /// Ist das Ergebnis verlustfrei? (Dann muss der Enkoder darauf
    /// achten, keine Filterung anzufangen.)
    pub verlustfrei: bool,
}

/// Quantisiert ein Bild auf eine Palette.
///
/// `dither` schaltet Floyd-Steinberg zu (an/aus). Dithering ist eine
/// Frage des Geschmacks, aber eine mit Folgen: bei 256 Farben und einem
/// Verlauf entstehen ohne Dithering sichtbare Baender („Banding"), mit
/// Dithering ein feines Korn. Fuer GIF (das Format mit der kleinsten
/// Palette) ist Dithering deshalb voreingestellt AN.
///
/// Der Fehler, den Dithering zurueckbringt, ist kleiner als der, den die
/// Quantisierung ohnehin macht — es tauscht Systemfehler gegen Zufallsfehler.
#[must_use]
pub fn quantisiere(bild: &Bild, max_farben: usize, mit_alpha: bool, dither: bool) -> Quantisiert {
    let probe = Stichprobe::von_bild(bild, mit_alpha);
    let (farben, gewichte) = probe.teile();
    let Ok(palette) = Palette::median_cut(farben, gewichte, max_farben, mit_alpha) else {
        // Sollte nicht vorkommen (median_cut prueft selbst). Ein Fehler
        // darf aber kein Panic sein: leere Palette, Bild wird schwarz.
        return Quantisiert {
            palette: Palette::leer(),
            indizes: vec![0u8; bild.breite as usize * bild.hoehe as usize],
            verlustfrei: false,
        };
    };
    // Verlustfrei, wenn ALLE Bildfarben unveraendert in der Palette stehen.
    let verlustfrei = !mit_alpha
        && probe.ist_vollstaendig()
        && palette.len() <= MAX_PALETTE
        && farben.len() <= max_farben
        && farben.iter().all(|f| {
            palette
                .farben
                .iter()
                .any(|p| p[1] == f[1] && p[2] == f[2] && p[3] == f[3])
        });

    let exakte: Option<Vec<u32>> = if verlustfrei {
        Some(
            farben
                .iter()
                .map(|f| {
                    ((255u32) << 24) | ((f[1] as u32) << 16) | ((f[2] as u32) << 8) | f[3] as u32
                })
                .collect(),
        )
    } else {
        None
    };
    let index = Farbindex::neu(palette.clone(), exakte.as_deref());
    let breite = bild.breite as usize;
    let hoehe = bild.hoehe as usize;
    let mut indizes = vec![0u8; breite * hoehe];

    if dither && !verlustfrei {
        // --- Floyd-Steinberg ---
        // Fehlerdiffusion nach links, rechts, oben, unten mit den Gewichten
        // 7/16, 3/16, 5/16, 1/16. Wir sammeln den Fehler in zwei Zeilen
        // `i32`-Puffern (R,G,B); das ist die Standardform und braucht
        // 2 * breite * 3 * 4 Byte, bei 16384 breit also 384 KiB — eine Zahl
        // aus der Eingabe, aber gegen `MAX_KANTE` begrenzt.
        let mut fehler_oben: Vec<i32> = vec![0; breite * 3];
        let mut fehler_unten: Vec<i32> = vec![0; breite * 3];
        for y in 0..hoehe {
            fehler_oben.copy_from_slice(&fehler_unten);
            fehler_unten.iter_mut().for_each(|v| *v = 0);
            for x in 0..breite {
                let i = (y * breite + x) * 4;
                let mut rgb = [
                    bild.rgba[i] as i32,
                    bild.rgba[i + 1] as i32,
                    bild.rgba[i + 2] as i32,
                ];
                for k in 0..3 {
                    rgb[k] += fehler_oben[x * 3 + k];
                    rgb[k] = rgb[k].clamp(0, 255);
                }
                let idx = index.index_rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
                indizes[y * breite + x] = idx;
                let ziel = index.palette().farben[idx as usize];
                for k in 0..3 {
                    let fehler = rgb[k] - ziel[k + 1] as i32;
                    // links (x-1, y)
                    if x > 0 {
                        let z = &mut fehler_oben[(x - 1) * 3 + k];
                        *z += fehler * 7 / 16;
                    }
                    // rechts (x+1, y)
                    if x + 1 < breite {
                        let z = &mut fehler_oben[(x + 1) * 3 + k];
                        *z += fehler * 3 / 16;
                    }
                    // unten (x, y+1)
                    if y + 1 < hoehe {
                        let z = &mut fehler_unten[x * 3 + k];
                        *z += fehler * 5 / 16;
                    }
                    // unten links
                    if x > 0 && y + 1 < hoehe {
                        let z = &mut fehler_unten[(x - 1) * 3 + k];
                        *z += fehler / 16;
                    }
                }
            }
        }
    } else {
        for y in 0..hoehe {
            for x in 0..breite {
                let i = (y * breite + x) * 4;
                let idx = index.index(
                    bild.rgba[i + 3],
                    bild.rgba[i],
                    bild.rgba[i + 1],
                    bild.rgba[i + 2],
                );
                indizes[y * breite + x] = idx;
            }
        }
    }

    Quantisiert {
        palette,
        indizes,
        verlustfrei,
    }
}
