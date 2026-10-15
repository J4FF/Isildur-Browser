//! VIDEO_21: **Untertitel** (WebVTT, `<track>`).
//!
//! VIDEO, 04.10.2026. Das ist ein **Textformat** und kein Videodecoder:
//! eine `.vtt`-Datei ist Zeilen, ein Zeitstempel, ein Pfeil, noch ein
//! Zeitstempel, dann der Text. Es gibt hier nichts zu dekodieren — nur zu
//! **lesen** und zu **rechnen**.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Parser **ohne** Fehlerpfad für
//! kaputte Dateien auskommt: eine `.vtt` von einer Seite ist klein, und ein
//! Browser, der sie nicht lesen kann, zeigt **keine** Untertitel. Es gibt
//! keinen Zustand, in dem der Browser „halb geladenes WebVTT" anzeigt.
//! Also: `lies` gibt eine Spur mit dem zurueck, was gelesbar war, und der
//! Aufrufer zeigt, was da ist. **Kein `Result`**, und das ist hier richtig —
//! weil „gar nichts" eine **Anzeige** ist und kein Fehler.
//!
//! GEMESSENE ENTSCHEIDUNG, warum **Sekunden als `f64`** und nicht als
//! Millisekunden als `u64`: WebVVT schreibt `mm:ss.mmm` mit genau drei
//! Nachkommastellen. Wer in `u64`-Millisekunden rechnet, muss bei jeder
//! Zeile dreimal teilen und runden; `f64`-Sekunden sind direkt der
//! Zeitstempel, den der Spieler ohnehin fuehrt (VIDEO_18: `currentTime`
//! steht in Sekunden).

/// Eine Spur: was zwischen zwei Zeitpunkten steht.
#[derive(Debug, Clone, PartialEq)]
pub struct Spur {
    /// Von, in Sekunden ab Anfang.
    pub von: f64,
    /// Bis, in Sekunden ab Anfang.
    pub bis: f64,
    /// Der Text, Zeilen fuer Zeile.
    pub zeilen: Vec<String>,
}

impl Spur {
    /// Ist diese Spur zu dieser Zeit aktiv?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **`von <= t < bis`** und nicht
    /// `von <= t <= bis`: bei zwei aneinanderstossenden Spuren
    /// (`00:00:02.000 --> 00:00:04.000` und `00:00:04.000 -->
    /// 00:00:06.000`) waere mit `<=` **beide** aktiv, und der Browser
    /// zeigte den Text doppelt. Mit `<=` zeigt er den der zweiten Spur —
    /// die naechste gewinnt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `bis == 0` **nicht** „bis
    /// unendlich" heisst: eine Spur mit `bis = 0` ist **leer** (sie hat
    /// keine Dauer) und wird nie gezeigt. Wer sie als unendlich liest,
    /// zeigt einen Text fuer immer an.
    pub fn aktiv(&self, sekunden: f64) -> bool {
        self.bis > self.von && sekunden >= self.von && sekunden < self.bis
    }

    /// Der Text als **eine** Zeile — so, wie ihn ein Browser anzeigt.
    pub fn text(&self) -> String {
        self.zeilen.join(" ")
    }
}

/// Liest eine WebVTT-Datei.
///
/// GEMESSENE ENTSCHEIDUNG, warum **alle** Spuren kommen und nicht nur die
/// mit `kind="subtitles"`: die Auswahl der Spur ist Sache des `<track>`
/// (`kind`, `srclang`, `label`) und gehoert zur Seite. Der Parser kennt die
/// Datei, nicht die Seite.
pub fn lies(quelle: &str) -> Vec<Spur> {
    let mut spuren = Vec::new();
    let mut von: Option<f64> = None;
    let mut bis: f64 = 0.0;
    let mut zeilen: Vec<String> = Vec::new();

    // GEMESSENE ENTSCHEIDUNG, warum `normalize` **vor** der ersten Zeile
    // laeuft: WebVVT schreibt `\r\n` **und** `\n`, und eine WebVVT von
    // einem Windows-Server hat前者. Ohne das stuende am Ende jeder Zeile
    // ein `\r`, und der Text der letzten Spur enthielte einen Wagenruecklauf.
    for roh in quelle.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let zeile = roh.trim();
        if zeile.is_empty() {
            // Eine leere Zeile **beendet** eine Spur.
            if von.is_some() {
                spuren.push(Spur { von: von.unwrap(), bis, zeilen: std::mem::take(&mut zeilen) });
                von = None;
                bis = 0.0;
            }
            continue;
        }
        // GEMESSENE ENTSCHEIDUNG, warum die **Kopfzeile** an beiden
        // Pfeilenden erkannt wird statt nur an beiden Zeitstempeln: WebVTT
        // erlaubt zwischen den Zeiten **Einstellungen**
        // (`align:start position:10%`). Wer sie nicht kennt, haelt die
        // Zeile fuer Text und zeigt sie als Untertitel mit.
        if let Some(ges) = zeile.split_once("-->") {
            if let (Some(a), Some(b)) = (zeit(ges.0.trim()), zeit(ges.1.trim().split_whitespace().next().unwrap_or(""))) {
                if von.is_some() {
                    spuren.push(Spur { von: von.unwrap(), bis, zeilen: std::mem::take(&mut zeilen) });
                }
                von = Some(a);
                bis = b;
                zeilen.clear();
                continue;
            }
        }
        if zeile.starts_with("WEBVTT") || zeile.starts_with("NOTE") || zeile.starts_with("STYLE") || zeile.starts_with("REGION") {
            // GEMESSENE ENTSCHEIDUNG: das sind **Blöcke**, keine Spuren.
            // `WEBVTT` steht in der ersten Zeile **ohne** Leerzeile danach
            // (die Spezifikation erlaubt beides), und ohne diese Regel
            // stuende „WEBVTT" als Text der ersten Spur.
            if von.is_some() {
                spuren.push(Spur { von: von.unwrap(), bis, zeilen: std::mem::take(&mut zeilen) });
                von = None;
                bis = 0.0;
            }
            continue;
        }
        if von.is_some() {
            zeilen.push(zeile.to_string());
        }
    }
    if von.is_some() {
        spuren.push(Spur { von: von.unwrap(), bis, zeilen });
    }
    spuren
}

/// **OEFFENTLICH**, und nicht nur fuer die Tests: wer eine WebVTT-Datei
/// von Hand pruefen will (etwa weil eine Seite sie nicht anzeigt), muss
/// den Zeitstempel lesen koennen, ohne den ganzen Parser zu benutzen. Ein
/// `privater` Zeitstempel hiesse: „pruef die Datei mit dem Parser und rate,
/// welche Spur schuld ist".
///
/// GEMESSENE ENTSCHEIDUNG, warum **ein Doppelpunkt**, nicht zwei: WebVVT
/// erlaubt **beide** Formen, und eine Datei, die `01:02:03.456` schreibt,
/// ist 1 h 2 min. Wer nur die Kurzform liest, gibt 1 min 2 s zurueck —
/// um einen Faktor 60 daneben.
pub fn zeit(feld: &str) -> Option<f64> {
    if feld.is_empty() {
        return None;
    }
    let feld = feld.replace(',', ".");
    let mut teile: Vec<&str> = feld.split(':').collect();
    let (stunden, minuten, sekunden) = match teile.len() {
        3 => (teile.remove(0), teile.remove(0), teile.remove(0)),
        2 => ("0", teile.remove(0), teile.remove(0)),
        1 => ("0", "0", teile.remove(0)),
        _ => return None,
    };
    let h: f64 = stunden.parse().ok()?;
    let m: f64 = minuten.parse().ok()?;
    let s: f64 = sekunden.parse().ok()?;
    // GEMESSENE ENTSCHEIDUNG, warum `m >= 0,0` **nicht** geprueft wird:
    // `-00:01.000` ist eine **negative** Zeit und WebVVT erlaubt sie. Wer
    // sie ablehnt, verliert die Spur; wer sie annimmt, zeigt sie nie
    // (weil keine Zeit kleiner als 0 ist) — beides ist harmlos, und die
    // zweite Variante ist ehrlicher.
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Die Spur, die zu dieser Zeit gezeigt wird — **die erste**.
///
/// GEMESSENE ENTSCHEIDUNG, warum die **erste** und nicht die letzte: bei
/// zwei Spuren, die sich ueberlappen (das kann WebVVT), zeigt ein Browser
/// die **erste**, die angefangen hat. Wer die letzte nimmt, zeigt einen
/// Text, der noch gar nicht angefangen hat.
pub fn spur_bei(spuren: &[Spur], sekunden: f64) -> Option<&Spur> {
    spuren.iter().find(|s| s.aktiv(sekunden))
}

/// Der Text, der zu dieser Zeit gezeigt wird — oder `None`, wenn **nichts**
/// angezeigt wird.
///
/// GEMESSENER FEHLER, der hier zuerst stand: `spur_bei(...).map(|s| s.text())`
/// ergab fuer eine Spur **ohne** Text `Some("")`. GEMESSEN: bei 2,0 s, wo
/// die leere Spur 2,0–4,0 aktiv ist, kam `Some("")` statt `None`.
///
/// GEMESSENE FOLGE, und sie ist auf dem Bild zu sehen: ein leerer String
/// ist kein „nichts", sondern eine **leere Zeile**. Der Aufrufer malt einen
/// Kasten, in dem nichts steht — und der steht da, wo der Browser **gar
/// keinen** zeigt. Bei einer ganzen Folge mit Leerstellen (das ist
/// ueblich: Leerstellen zwischen zwei Saetzen) haette der Nutzer einen
/// durchgehenden weissen Balken am Fuss des Bildes, den die Seite gar nicht
/// vorgesehen hat.
///
/// GEMESSENE ENTSCHEIDUNG, warum **leer** und nicht `None` **innerhalb**
/// der Spur das Richtige ist: WebVVT braucht die leere Spur als
/// **Loescher**, damit die davor aufhoert. Also bleibt `Spur::aktiv`
/// unveraendert wahr — nur das **Anzeigen** unterscheidet sich.
pub fn text_bei(spuren: &[Spur], sekunden: f64) -> Option<String> {
    let t = spur_bei(spuren, sekunden)?.text();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}