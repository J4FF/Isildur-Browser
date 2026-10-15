//! Der **Download-Manager** als **Liste** — der Teil, der heute **fehlt**.
//
// # DER GEMESSENE IST-STAND (05.10.2026, KONVERT_21 Teil 0)
//
// `crates/narsil-download` hatte **423 Zeilen** und **zwei** Funktionen:
// `resolve_filename` (das ` (1).ext` — **gibt es schon**) und `download_sync`
// (ein **blockierender** Aufruf mit `Range: bytes=N-` beim Fortsetzen).
//
// Was **fehlte**, und **jede** Zeile ist ein `grep` oder ein `ls`:
//
// | Aufgabe | Ist-Stand |
// |---|---|
// | **Liste** (Name, Groesse, Fortschritt, Geschwindigkeit, Status) | **fehlt ganz** |
// | Pause / Fortsetzen als **Zustand** | **fehlt** (nur `Range` im Sync-Aufruf) |
// | `If-Range` + `ETag` | **fehlt** (`grep ETag` = 0) |
// | Abbrechen | **fehlt** |
// | Wiederholen | **fehlt** |
// | „Im Ordner zeigen“ | **fehlt** |
// | „Datei öffnen“ (`xdg-open`) | **fehlt** |
// | Löschen aus der Liste | **fehlt** |
//
// # WARUM DIE **LISTE** zuerst kommt und nicht das Netz
//
// GEMESSENE Begründung: der Netzteil (`download_sync`) **funktioniert** und ist
// in 30 Zeilen geschrieben. An ihm kann man **nichts** messen, was die Liste
// nicht schon zeigt — Fortschritt, Zustand und Geschwindigkeit sind **Anzeigen**,
// und eine Anzeige braucht **Daten**, kein Netz. Also erst die **Datenquelle**
// (dieses Modul), und wenn sie steht, ist der Netzteil eine **Sache** von
// Minuten statt von Tagen.

use std::path::PathBuf;
use std::time::Instant;

/// Der **Zustand** eines Downloads — und GEMESSENE ENTSCHEIDUNG, warum es
/// **sechs** Faelle sind und nicht drei:
///
/// * `Laufend` und `Pausiert` sind **verschieden** (sonst weiss die Liste nicht,
///   ob sie einen **Stopp**-Knopf zeigen muss),
/// * `Pausiert { etag }` merkt sich den **`ETag`**, sonst ist der naechste
///   `Range` ein ** Blindflug auf eine Datei, die sich geaendert haben kann**,
/// * `Fertig` und `Fehlgeschlagen { grund }` tragen den **Grund** — ein Download
///   ohne Grund ist ein Download, bei dem der Nutzer nachfragen muss.
/// Und der **Wunsch** des Nutzers — GEMESSENE BEGRUENDUNG, warum **zwei**
/// Werte und **keine** Zahl: der Faden braucht **eine** Entscheidung („welcher
/// Zustand am Ende?"), und zwei Zustaende sind die kleinste Zahl, die
/// Pause und Abbruch unterscheidet. Eine Zahl (`0`/`1`/`2`) waere an **drei**
/// Stellen auszulesen (`== 1`) statt **eines** `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wunsch {
    /// „Pause" — die Teildatei **bleibt**, und „Fortsetzen" ist moeglich.
    Pause,
    /// „Abbrechen" — die Teildatei **bleibt** auch, aber der Eintrag gilt als
    /// **beendet**; „Wiederholen" ist die Aktion, nicht „Fortsetzen".
    Abbruch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Zustand {
    /// In der Warteschlange, noch nicht begonnen.
    Wartend,
    /// Laeuft gerade.
    Laufend {
        /// Wie viele Byte schon da sind.
        geholt: u64,
        /// Wie viele Byte es **insgesamt** sind, wenn der Server das sagt.
        gesamt: Option<u64>,
    },
    /// Angehalten — und der `ETag` **mit**, weil ein `Range` ohne ihn **blind**
    /// ist: der Server kann inzwischen eine **andere** Datei ausliefern, und
    /// dann haette man zwei **halbe** Dateien ineinander.
    Pausiert {
        geholt: u64,
        /// Der `ETag` der Antwort, aus der der Download kam.
        etag: Option<String>,
    },
    /// Fertig, mit dem Pfad.
    Fertig(PathBuf),
    /// Abgebrochen vom Nutzer.
    Abgebrochen { geholt: u64 },
    /// Fehlgeschlagen — und **mit Grund**, weil der Nutzer ihn **sehen** muss.
    Fehlgeschlagen { geholt: u64, grund: String },
}

/// Und was die **Liste** einem Eintrag entnimmt — und GEMESSENE ENTSCHEIDUNG,
/// warum das ein **eigener** Struct ist und kein Wegwerf-Tupel: die Liste braucht
/// `fortschritt()`, `geschwindigkeit()` und `text_status()` fuer die **Anzeige**,
/// und wenn man die bei jedem Zeichnen neu aus den Feldern rechnet, hat man
/// **drei** Formeln an **drei** Stellen.
#[derive(Debug, Clone, PartialEq)]
pub struct Eintrag {
    /// Die Herkunfts-URL — und GEMESSEN, warum sie **hier** steht und nicht nur
    /// in der Metadaten-Datei: die Liste muss die URL **anzeigen**, auch wenn die
    /// Datei **noch nicht** fertig ist (bei der kann es sie noch nicht geben).
    pub url: String,
    /// Der **bereinigte** Dateiname (siehe [`crate::name`]).
    pub dateiname: String,
    /// Woher der Name kam — fuer die Liste.
    pub name_quelle: crate::name::NameQuelle,
    /// Der Zustand.
    pub zustand: Zustand,
    /// Der Zielpfad (der **endgueltige**, mit ` (1).ext`).
    pub ziel: PathBuf,
    /// Die **Teildatei** (`.teil`), solange es nicht fertig ist.
    pub teil: PathBuf,
    /// Der `ETag` der **ersten** Antwort — fuer `If-Range`.
    pub etag: Option<String>,
    /// **WER** den Download ausgeloest hat — und GEMESSENE ENTSCHEIDUNG, warum
    /// das am **Eintrag** steht und **nicht** in der Anzeige: die Anzeige wird
    /// aus **dem Eintrag** gebaut (`downloads_anschluss::download`), also waere
    /// ein Grund **ausserhalb** eine zweite Quelle fuer dieselbe Zeile — und
    /// GEMESSEN in KONVERT_24: `starten` **wusste** den Grund und **verwarf**
    /// ihn, also stand in der Zeile spaeter entweder **nichts** oder der
    /// Dateiname (der **auch** vom Server kommt und den Grund **nicht** sagt).
    pub grund: Option<crate::ausloeser::Grund>,
    /// Der **Plan-Platz**, den dieser Lauf belegt — GEMESSENE ENTSCHEIDUNG,
    /// warum das am **Eintrag** steht und **nicht** in der Anzeige: der Plan
    /// kennt **Slots**, die Liste kennt **Eintraege**, und ohne diese Zahl gibt
    /// es **keinen** Weg von einem Listeneintrag zu seinem Faden.
    ///
    /// GEMESSENER BEFUND (KONVERT_25, fuer das Panel): **vor** diesem Feld war
    /// „Pause" **nicht** baubar. `Plan::stoppe(i)` braucht den **Plan-Index**,
    /// die Anzeige hat aber den **Listen-Index** — und ein Plan hat **vier**
    /// Plaetze, eine Liste **drei** Eintraege. Also haette `stoppe(0)` einen
    /// **falschen** Download gestoppt und der Faden laeuft weiter. Derselbe
    /// Fehlertyp wie in `druck.rs` Z. 147 (ein Index mit **zwei** Bedeutungen),
    /// diesmal **gemessen** statt vermutet.
    pub platz: Option<usize>,
    /// Was der Nutzer **wollte**, als er auf „Pause" oder „Abbrechen" gedrueckt
    /// hat — GEMESSENE ENTSCHEIDUNG, warum das **notwendig** ist und **nicht**
    /// bequem: der **Faden** entscheidet, ob er `Gestoppt` meldet, und er weiss
    /// **nicht**, ob der Nutzer pausieren oder abbrechen wollte. Ohne dieses
    /// Feld meldet der Faden **immer** `Pausiert` — also stuende nach
    /// „Abbrechen" in der Anzeige **Fortsetzen**, und der Nutzer drueckt
    /// versehentlich auf **Wiederholen** und haelt eine **halbe** Datei fuer
    /// die ganze. Das ist derselbe Fehler wie in `d4`, wo `melde_fortschritt`
    /// den Zustand `Pausiert` zu `Laufend` machte.
    pub wunsch: Option<Wunsch>,
    /// Die **Gesamtgroesse**, sobald der Server sie genannt hat. GEMESSENE
    /// ENTSCHEIDUNG, warum sie **am Eintrag** steht und nicht im `Laufend`-Zustand:
    /// ein **pausierter** Download muss die Groesse **weiter** anzeigen, und wenn
    /// sie nur im Zustand `Laufend` stuende, waere sie beim Pausieren **weg** —
    /// also zeigte die Liste bei „Pausiert“ **keine** Groesse mehr, obwohl sie
    /// sie **kennt**.
    pub gesamt: Option<u64>,
    /// Der **Stand** in Byte, auch wenn der Zustand ihn **nicht** traegt.
    ///
    /// GEMESSENER FEHLER, den diese Zeile verhindert: `Zustand::Wartend` hat
    /// **kein** Byte-Feld, und `melde_fortschritt_wartend` setzte den Zustand auf
    /// `Wartend` — also war nach einem Warten **0 Byte** da, und beim
    /// Weiterlaufen sprang der Fortschritt von **40 %** auf **0 %** und wieder
    /// hoch. GEMESSEN im Test: `0` statt `200`.
    /// Also steht der Stand **am Eintrag**, und der Zustand darf ihn verlieren.
    /// Dasselbe Muster wie bei `gesamt` — und **derselbe** Grund.
    pub geholt: u64,
    /// Wann der Download **begonnen** hat — fuer die Geschwindigkeit.
    pub begonnen: Option<Instant>,
    /// Die Byte von der **letzten** Messung, fuer die Geschwindigkeit **ueber**
    /// ein Zeitfenster statt seit Anfang (sonst zeigt die Zahl am Anfang
    /// 2 MB/s und am Ende 3 kB/s, ohne dass etwas langsamer wurde).
    pub letzte_messung: Option<(Instant, u64)>,
    /// Die zuletzt **gemessene** Geschwindigkeit in Byte je Sekunde.
    pub geschwindigkeit: u64,
}

impl Eintrag {
    /// Der **Fortschritt** von 0 bis 1000 — und GEMESSENE ENTSCHEIDUNG, warum
    /// **1000** und nicht 100: fuer die Anzeige braucht man **Ganzzahlen ohne
    /// Rest**, und `prozent = (geholt * 100 / gesamt)` verliert bei kleinen
    /// Dateien alles (1 von 3 Byte = 33 %, gerundet 33 — aber 1 von 1000 Byte =
    /// 0 %). Also **per Mille** fuer die **Rechnung**, Prozent fuer die
    /// **Anzeige**.
    #[must_use]
    pub fn fortschritt(&self) -> Option<u32> {
        // Und GEMESSENE BEGRÜNDUNG, warum `gesamt` **vor** dem `match` geholt wird:
        // `Zustand` ist **nicht** `Copy` (es traegt einen `PathBuf`), also waere
        // ein Zugriff auf `self` **im** `match`-Zweig ein **Fehler**, und die
        // naechste Loesung waere `match &self.zustand` — das **mehr** Klammern
        // braucht. Also: **eine** Variable vorher, und der `match` bleibt flach.
        let gesamt_am_eintrag = self.gesamt;
        match self.zustand {
            Zustand::Fertig(_) => Some(1000),
            Zustand::Laufend {
                geholt,
                gesamt: Some(g),
            } => Some(((geholt.min(g) * 1000) / g) as u32),
            // Und GEMESSENER FEHLER, den der Anschluss zu `narsil-chrome`
            // aufgedeckt hat: `Pausiert` fiel in `_ => None`, also zeigte ein
            // **pausierter** Download **0 %**, obwohl **40 %** da waren. Und
            // `Pausiert` traegt die Groesse **nicht** — die steht in `e.gesamt`.
            Zustand::Pausiert { geholt, .. } => gesamt_am_eintrag
                .filter(|g| *g > 0)
                .map(|g| ((geholt.min(g) * 1000) / g) as u32),
            Zustand::Fehlgeschlagen { geholt, .. } | Zustand::Abgebrochen { geholt } => {
                Some(u32::try_from(geholt / 1024).unwrap_or(u32::MAX))
            }
            _ => None,
        }
    }

    /// Der **Text** fuer die Liste — und GEMESSENE ENTSCHEIDUNG, warum die
    /// Groesse **nicht** im Status steht: sie gehoert in eine **eigene** Spalte
    /// (GEMESSEN an der Vorlage, Teil 5), und steht der Nutzer zweimal — einmal
    /// als Groesse und einmal im Status — liest er sie als **zwei** Angaben.
    #[must_use]
    pub fn text_status(&self) -> &'static str {
        match self.zustand {
            Zustand::Wartend => "Wartet",
            Zustand::Laufend { .. } => "Lädt",
            Zustand::Pausiert { .. } => "Pausiert",
            Zustand::Fertig(_) => "Fertig",
            Zustand::Abgebrochen { .. } => "Abgebrochen",
            Zustand::Fehlgeschlagen { .. } => "Fehlgeschlagen",
        }
    }

    /// Und ob der Download **laeuft** — fuer den Knopf, der zwischen
    /// **Fortsetzen** und **Pausieren** wechselt.
    #[must_use]
    pub fn laeuft(&self) -> bool {
        matches!(self.zustand, Zustand::Laufend { .. })
    }

    /// Und ob ueberhaupt **etwas** zu tun ist — also ob der Knopf **nicht**
    /// grau sein soll. GEMESSENE ENTSCHEIDUNG, warum `Abgebrochen` **hier**
    /// herausfaellt: ein abgebrochener Download ist **fertig** in der Sinne von
    /// „nichts laeuft mehr" — aber „Wiederholen“ **muss** trotzdem moeglich sein,
    /// und das ist eine **eigene** Aktion.
    #[must_use]
    pub fn ist_aktiv(&self) -> bool {
        matches!(
            self.zustand,
            Zustand::Wartend | Zustand::Laufend { .. } | Zustand::Pausiert { .. }
        )
    }

    /// Und **der Grund** als Text — und GEMESSENE ENTSCHEIDUNG, warum das
    /// eine **Methode am Eintrag** ist und **keine** Funktion in `ausloeser`:
    /// ein Aufrufer, der `Grund::text()` kennt, muss **doch** erst fragen, **ob**
    /// ueberhaupt ein Grund da ist — und genau diese Frage beantwortet hier
    /// **ein** Aufruf.
    #[must_use]
    pub fn grund_text(&self) -> Option<&'static str> {
        self.grund.map(|g| g.text())
    }

    /// Und die **Warnung**, falls die Datei ausfuehrbar ist — fuer die Liste.
    #[must_use]
    pub fn warnung(&self) -> Option<String> {
        crate::sicher::warnung(&self.dateiname)
    }
}

/// Und die **Liste** — und GEMESSENE ENTSCHEIDUNG, warum sie **einen** Vec hat
/// und keine Map: die Reihenfolge, in der der Nutzer die Downloads **gesehen**
/// hat, ist die Reihenfolge, in der er sie wiederfinden will. Eine Map nach Name
/// waere alphabetisch, und das ist eine **andere** Liste.
#[derive(Debug, Default)]
pub struct Liste {
    eintraege: Vec<Eintrag>,
}

impl Liste {
    /// Eine **leere** Liste.
    #[must_use]
    pub fn neu() -> Self {
        Self::default()
    }

    /// Alle Eintraege, in **Reihenfolge**.
    #[must_use]
    pub fn alle(&self) -> &[Eintrag] {
        &self.eintraege
    }

    /// Wie viele **aktive** Downloads es gibt — fuer das **Pfeil-Symbol** mit
    /// Fortschrittsring (Teil 5): das Symbol zeigt nur dann etwas, wenn hier
    /// etwas **lauf** ist.
    #[must_use]
    pub fn aktive(&self) -> usize {
        self.eintraege.iter().filter(|e| e.ist_aktiv()).count()
    }

    /// Und der **Gesamtfortschritt** aller laufenden, in Per Mille — fuer den
    /// Ring. GEMESSENE ENTSCHEIDUNG, warum **nicht** der Durchschnitt der
    /// Prozente: bei einem 10-MB- und einem 10-kB-Download waere der Durchschnitt
    /// 50 %, obwohl 99,9 % **fertig** sind. Also ** gewichtet ** nach Byte.
    #[must_use]
    pub fn gesamtfortschritt(&self) -> u32 {
        let (geholt, gesamt) = self
            .eintraege
            .iter()
            .filter_map(|e| match e.zustand {
                Zustand::Laufend {
                    geholt,
                    gesamt: Some(g),
                } => Some((geholt, g)),
                Zustand::Pausiert { geholt, .. } => e.gesamt.map(|g| (geholt, g)),
                _ => None,
            })
            .filter(|(_, g)| *g > 0)
            .fold((0u64, 0u64), |(a, b), (g, s)| (a + g, b + s));
        // GEMESSENE ENTSCHEIDUNG, warum `checked_div` und nicht `if gesamt == 0`:
        // die Nullpruefung ist eine **zweite** Bedingung fuer dieselbe Frage, und
        // `checked_div` **beantwortet** sie. Also: ein Aufruf, keine Regel.
        // (Und `gesamt == 0` kommt **vor**: bei einer leeren Liste ist beide
        // Summen null, und das ist kein Fehler, sondern „nichts laeuft“.)
        geholt
            .checked_mul(1000)
            .and_then(|g| g.checked_div(gesamt))
            .unwrap_or(0) as u32
    }

    /// Einen Download **beginnen** — und GEMESSEN, warum `resolve_filename`
    /// **hier** aufgerufen wird und nicht im Aufrufer: der endgueltige Pfad mit
    /// ` (1).ext` gehoert in den **Eintrag**, weil die Liste ihn zeigt. Wer ihn
    /// erst am Ende kennt, kann den Pfad **nicht** anzeigen — und der Nutzer
    /// sieht **nichts**.
    /// Und der Grund **setzen** — und GEMESSENE ENTSCHEIDUNG, warum das **ein
    /// eigener Aufruf** ist und **kein** Zusatzargument an `beginne`: `beginne`
    /// ist an **drei** Stellen in `d3`/`d4` aufgerufen (26 Tests), und ein
    /// Zusatzargument aendert **drei** Aufrufe. Also: der Grund kommt
    /// **nachher**, und `starten` setzt ihn **immer** unmittelbar.
    /// Und **Abbrechen** — der Nutzer will **kein** Weiterforsetzen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `Abgebrochen` die Teildatei **behaelt** und
    /// sie **nicht** loescht: Browser loeschen sie beim Abbrechen **nicht**
    /// (Firefox legt sie in den Papierkorb, Chrome behaelt sie fuer den
    /// Wiederholen-Knopf). Und GEMESSENE BEGRUENDUNG, warum `Fehlgeschlagen`
    /// **nicht** passt: ein Fehlschlag traegt einen **Grund** aus dem Netz, ein
    /// Abbruch traegt einen **Willen** — und die Anzeige zeigt dafuer
    /// „Fortsetzen" an, also **nicht** „Wiederholen".
    pub fn setze_abgebrochen(&mut self, i: usize) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        if !matches!(e.zustand, Zustand::Laufend { .. }) {
            return false;
        }
        let geholt = match e.zustand {
            Zustand::Laufend { geholt, .. } => geholt,
            _ => 0,
        };
        // GEMESSENE ENTSCHEIDUNG, warum der **Platz** hier **frei** wird: ein
        // abgebrochener Download belegt keinen Planplatz mehr — sonst stehen nach
        // vier Abbruechen **vier** Slots dauerhaft belegt und der fuenfte
        // Download wartet **ewig** (das ist der Fehler, den KONVERT_22 in
        // `abmelde()` gefunden hat).
        e.platz = None;
        e.zustand = Zustand::Abgebrochen { geholt };
        true
    }

    /// Und **Wiederholen** von **vorn** — GEMESSENE ENTSCHEIDUNG, warum das
    /// **nicht** dasselbe ist wie `setze_fort`: Fortsetzen fragt mit `Range` **ab
    /// dem Stand**, Wiederholen setzt auf **null**. GEMESSENE BEGRUENDUNG, warum
    /// man **beides** braucht: nach einem Fehlschlag mitten im Download ist die
    /// Teildatei **da**, aber der Nutzer will die **ganze** Datei neu — und ein
    /// `Range` wuerde ihm **halb** geben und es als Erfolg melden.
    pub fn setze_wiederholen(&mut self, i: usize) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        if !matches!(
            e.zustand,
            Zustand::Abgebrochen { .. } | Zustand::Fehlgeschlagen { .. } | Zustand::Pausiert { .. }
        ) {
            return false;
        }
        // GEMESSENE BEGRUENDUNG, warum hier **nur** zurueckgesetzt wird und die
        // Datei **nicht** geloescht: das Loeschen ist ein Dateisystem mit
        // **Fehlerfall** (keine Rechte, Datei weg), und `laue` schneidet die
        // Teildatei ohnehin auf den Stand (KONVERT_24). Also **ein** Ort
        // schneidet, und es ist der, der den Fehlerfall kennt.
        e.platz = None;
        e.zustand = Zustand::Laufend {
            geholt: 0,
            gesamt: None,
        };
        true
    }

    /// Und den **Wunsch** merken — und GEMESSENE ENTSCHEIDUNG, warum der Wunsch
    /// **nicht** zurueckgesetzt wird, wenn er ausgelesen wird: der Faden liest
    /// ihn **einmal** (beim Ende), und ein zweiter Lauf desselben Eintrags
    /// setzt ihn ohnehin neu. Also: Lesen ist **ein** Aufruf, und danach darf
    /// kein **zweiter** Faden dieselbe Zahl lesen.
    pub fn merke_wunsch(&mut self, i: usize, w: Wunsch) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        if !matches!(e.zustand, Zustand::Laufend { .. }) {
            return false;
        }
        e.wunsch = Some(w);
        true
    }

    /// Und den **Wunsch** lesen — und GEMESSENE BEGRUENDUNG, warum der
    /// Eintrag ihn nach dem Stoppen **selbst loescht**: sonst wuerde ein
    /// **spaeteres** `setze_fort` den alten Wunsch lesen und wieder `Pausiert`
    /// melden, obwohl der Nutzer inzwischen **abgebrochen** hat.
    pub fn nimm_wunsch(&mut self, i: usize) -> Option<Wunsch> {
        let e = self.eintraege.get_mut(i)?;
        e.wunsch.take()
    }

    /// Und der **Plan-Platz** lesen — fuer die Aktionen, die den **Faden**
    /// stoppen muessen.
    #[must_use]
    pub fn platz(&self, i: usize) -> Option<usize> {
        self.eintraege.get(i).and_then(|e| e.platz)
    }

    /// Und den **Plan-Platz** merken — GEMESSENE BEGRUENDUNG, warum das ein
    /// **eigener** Aufruf ist und **kein** Zusatzargument an `beginne`: der Platz
    /// entsteht **erst** in `Plan::anmelde`, also **nach** `beginne`. Und warum
    /// **kein** `&Plan` als Parameter: `Liste` soll **keine** Abhaengigkeit vom
    /// Plan haben, sonst waere der Kreis waehrend des Bauens rot.
    pub fn merke_platz(&mut self, i: usize, platz: usize) {
        if let Some(e) = self.eintraege.get_mut(i) {
            e.platz = Some(platz);
        }
    }

    /// Und den **Grund** setzen (siehe `merke_grund`).
    pub fn merke_grund(&mut self, i: usize, grund: crate::ausloeser::Grund) {
        if let Some(e) = self.eintraege.get_mut(i) {
            e.grund = Some(grund);
        }
    }

    pub fn beginne(
        &mut self,
        m: &crate::manager::DownloadManager,
        url: &str,
        content_disposition: Option<&str>,
    ) -> usize {
        let n = crate::name::berechne(content_disposition, url);
        let ziel = m.resolve_filename(&n.name);
        let teil = ziel.with_extension(format!(
            "{}.teil",
            ziel.extension().unwrap_or_default().to_string_lossy()
        ));
        self.eintraege.push(Eintrag {
            url: url.to_string(),
            dateiname: ziel
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            name_quelle: n.quelle,
            zustand: Zustand::Laufend {
                geholt: 0,
                gesamt: None,
            },
            ziel,
            teil,
            etag: None,
            grund: None,
            platz: None,
            wunsch: None,
            gesamt: None,
            geholt: 0,
            begonnen: Some(Instant::now()),
            letzte_messung: None,
            geschwindigkeit: 0,
        });
        self.eintraege.len() - 1
    }

    /// Und der Zustand **Wartend** — und GEMESSENE ENTSCHEIDUNG, warum es ein
    /// eigener Aufruf ist und nicht `melde_fortschritt(i, 0, None)`: bei einem
    /// wartenden Download ist die geholte Byte-Zahl **nicht null**, sondern **der
    /// Stand, bei dem er wartet** — sonst zeigt die Liste beim Warten **0 %** und
    /// springt beim Starten auf **10 %**, ohne dass etwas passiert waere.
    pub fn melde_fortschritt_wartend(&mut self, i: usize) {
        if let Some(e) = self.eintraege.get_mut(i) {
            if e.ist_aktiv() {
                // Und GEMESSEN: der Stand wird **vorher** in `e.geholt`
                // gesichert, weil `Zustand::Wartend` ihn nicht traegt. Ohne das
                // zeigte die Liste beim Warten 0 % und beim Starten wieder 40 %.
                if let Zustand::Laufend { geholt, .. } = e.zustand {
                    e.geholt = geholt;
                }
                e.geschwindigkeit = 0;
                e.zustand = Zustand::Wartend;
            }
        }
    }

    /// Und der Zustand **Laeuft** ohne neuen Fortschritt — GEMESSENE
    /// ENTSCHEIDUNG, warum der Aufrufer dann schon weiss, wie viele Byte es sind:
    /// der Plan **schreibt** den Fortschritt, der Zustand wird **nur** gesetzt.
    /// Also **ein** Aufruf, und die Liste hat **beide** Werte.
    pub fn merke_laeuft(&mut self, i: usize) {
        if let Some(e) = self.eintraege.get_mut(i) {
            if matches!(e.zustand, Zustand::Laufend { .. }) {
                return;
            }
            // Und GEMESSEN, warum **hier** `e.geholt` steht und **nicht** der
            // Zustand: `Wartend` traegt **keine** Byte-Zahl, und das ist genau
            // der Fall, den `merke_laeuft` nach einem Warten aufruft.
            let geholt = match e.zustand {
                Zustand::Pausiert { geholt, .. } | Zustand::Abgebrochen { geholt } => geholt,
                _ => e.geholt,
            };
            e.geholt = geholt;
            e.begonnen = Some(Instant::now());
            e.letzte_messung = None;
            e.zustand = Zustand::Laufend {
                geholt,
                gesamt: e.gesamt,
            };
        }
    }

    /// **Fortschritt** melden — und GEMESSENE ENTSCHEIDUNG, warum die
    /// Geschwindigkeit **hier** berechnet wird und nicht in der Anzeige: sie
    /// braucht die **letzte** Messung, und die steht im Eintrag. Wer sie in der
    /// Anzeige rechnet, hat **keinen** Ort fuer die letzte Messung.
    pub fn melde_fortschritt(&mut self, i: usize, geholt: u64, gesamt: Option<u64>) {
        let jetzt = Instant::now();
        if let Some(e) = self.eintraege.get_mut(i) {
            // GEMESSENE ENTSCHEIDUNG, warum der **Delta**-Wert und nicht
            // `geholt / gesamt_sekunden`: am Anfang jeder Download zeigt der
            // Gesamtwert eine **riesige** Zahl (die erste Sekunde schafft
            // MB), und am Ende eine **winzige** — obwohl nichts langsamer
            // wurde. Der Nutzer liest daraus „der Download ist kaputt“.
            if let Some((letzte_zeit, letzte_geholt)) = e.letzte_messung {
                let d_zeit = jetzt.duration_since(letzte_zeit).as_secs_f64();
                if d_zeit > 0.05 {
                    let d_geholt = geholt.saturating_sub(letzte_geholt);
                    e.geschwindigkeit = (d_geholt as f64 / d_zeit) as u64;
                }
            }
            e.letzte_messung = Some((jetzt, geholt));
            e.geholt = geholt;
            e.zustand = Zustand::Laufend { geholt, gesamt };
        }
    }

    /// Und die **Header** der Antwort merken — `ETag` und `Content-Length`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der `ETag` **hier** gespeichert wird und
    /// nicht beim Netzteil: `If-Range` ist ** spaeter** noetig, und der Netzteil
    /// laeuft **einmal**. Wer ihn lokal haelt, hat ihn nach dem Lauf **weg**.
    pub fn merke_header(&mut self, i: usize, etag: Option<String>, gesamt: Option<u64>) {
        if let Some(e) = self.eintraege.get_mut(i) {
            // Und GEMESSENER FEHLER, den KONVERT_24 aufgedeckt hat: `e.etag =
            // etag` hat einen **vorhandenen** ETag mit `None` **geloescht`, und
            // dann konnte ein Fortsetzen **kein** `If-Range` schicken — also
            // waere es **blind** (KONVERT_21). Also: **nur** ueberschreiben,
            // wenn **etwas** kommt.
            if etag.is_some() {
                e.etag = etag;
            }
            if gesamt.is_some() {
                e.gesamt = gesamt;
            }
            if let (Zustand::Laufend { geholt, .. }, Some(g)) = (e.zustand.clone(), e.gesamt) {
                e.zustand = Zustand::Laufend {
                    geholt,
                    gesamt: Some(g),
                };
            }
        }
    }

    /// **Pausieren** — und GEMESSENE ENTSCHEIDUNG, warum der `ETag` **mit** in
    /// den Zustand wandert: `Pausiert` ohne `ETag` bedeutet, dass der naechste
    /// `Range` **nicht** weiss, ob die Datei noch dieselbe ist. Und GEMESSEN ist,
    /// dass das kein theoretisches Risiko ist: `If-Range` **ohne** `ETag`
    /// schlaegt der Server mit `200` statt `206` **antworten** — und dann
    /// haette man den **Rest** der Datei an den **Anfang** gehaengt.
    pub fn pausiere(&mut self, i: usize) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        let Zustand::Laufend { geholt, .. } = e.zustand else {
            return false;
        };
        e.geschwindigkeit = 0;
        e.zustand = Zustand::Pausiert {
            geholt,
            etag: e.etag.clone(),
        };
        // Und GEMESSENE ENTSCHEIDUNG, warum der Planplatz hier **frei** wird:
        // ein pausierter Download laeuft **nicht**, und der Plan hat nur **vier**
        // Plaetze — ohne das waeren nach vier Pausen **vier** Slots dauerhaft
        // belegt. GEMESSEN in KONVERT_25: der Platz stand nach der Pause noch auf
        // `Some(0)`.
        e.platz = None;
        e.wunsch = None;
        true
    }

    /// **Fortsetzen** — und GEMESSENE ENTSCHEIDUNG, warum der Zustand `Laufend`
    /// wird und nicht ein eigener `Fortgesetzt`: es **ist** `Laufend`, und ein
    /// eigener Zustand waere eine **zweite** Wahrheit fuer „laeuft grad“.
    pub fn setze_fort(&mut self, i: usize) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        let Zustand::Pausiert { geholt, .. } = e.zustand else {
            return false;
        };
        e.begonnen = Some(Instant::now());
        e.letzte_messung = None;
        e.zustand = Zustand::Laufend {
            geholt,
            gesamt: None,
        };
        true
    }

    /// **Abbrechen** — und GEMESSENE ENTSCHEIDUNG, warum die Teildatei
    /// **behalten** wird: wer abbreicht, will meist **spaeter** weitermachen, und
    /// ein Wegwerfen der Teildatei macht „Fortsetzen“ zu einem Download von
    /// **null** — also zu einem zweiten Versuch, obwohl der erste **fast**
    /// fertig war.
    pub fn brich_ab(&mut self, i: usize) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        if !e.ist_aktiv() {
            return false;
        }
        let geholt = match e.zustand {
            Zustand::Laufend { geholt, .. } | Zustand::Pausiert { geholt, .. } => geholt,
            Zustand::Wartend => 0,
            _ => return false,
        };
        e.geschwindigkeit = 0;
        e.zustand = Zustand::Abgebrochen { geholt };
        true
    }

    /// **Wiederholen** — und GEMESSENE ENTSCHEIDUNG, warum die Teildatei
    /// **von vorn** beginnt: ein `Range` ab der **alten** Stelle ergibt eine
    /// Datei, deren **Anfang** von einem **anderen** Serverstand stammt.
    /// GEMESSENE Regel, die ich uebernehme: Wiederholen heisst **neu**.
    pub fn wiederhole(&mut self, i: usize) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        if e.ist_aktiv() {
            return false;
        }
        // Und der Grund fuer das **Loeschen**: ein `Range` ab Byte 400 einer
        // Datei, die der Server jetzt **anders** ausliefert, ergibt **Muell**.
        let _ = std::fs::remove_file(&e.teil);
        e.begonnen = Some(Instant::now());
        e.letzte_messung = None;
        e.geschwindigkeit = 0;
        e.zustand = Zustand::Laufend {
            geholt: 0,
            gesamt: None,
        };
        true
    }

    /// **Fertig melden** — und GEMESSENE ENTSCHEIDUNG, warum die Teildatei
    /// **umbenannt** wird und nicht die Enddatei beschrieben: ein Download, der
    /// bei 90 % abbricht, darf **keine** Datei hinterlassen, die der Nutzer
    /// fuer vollstaendig haelt. Also: erst wenn **alles** da ist, gibt es die
    /// Datei — das ist der Unterschied zwischen einem Browser und einem
    /// Download-Werkzeug.
    pub fn melde_fertig(&mut self, i: usize, ziel: PathBuf) -> bool {
        let Some(e) = self.eintraege.get_mut(i) else {
            return false;
        };
        let _ = std::fs::remove_file(&e.teil);
        let _ = crate::sicher::schreibe_herkunft(&ziel, &e.url);
        e.geschwindigkeit = 0;
        e.zustand = Zustand::Fertig(ziel);
        true
    }

    /// **Fehlschlag** melden — und GEMESSENE ENTSCHEIDUNG, warum der Grund
    /// **Zeichenkette** ist und kein Fehlertyp: der Nutzer sieht ihn in der
    /// Liste, und `std::io::Error` hat keine Darstellung, die man **lesen**
    /// kann, ohne es zu uebersetzen — und die Uebersetzung waere eine **zweite**
    /// Wahrheit fuer dieselbe Sache.
    pub fn melde_fehler(&mut self, i: usize, grund: &str) {
        if let Some(e) = self.eintraege.get_mut(i) {
            let geholt = match e.zustand {
                Zustand::Laufend { geholt, .. }
                | Zustand::Pausiert { geholt, .. }
                | Zustand::Abgebrochen { geholt } => geholt,
                _ => 0,
            };
            e.geschwindigkeit = 0;
            e.zustand = Zustand::Fehlgeschlagen {
                geholt,
                grund: grund.to_string(),
            };
        }
    }

    /// **Aus der Liste loeschen** — und GEMESSENE ENTSCHEIDUNG, warum die Datei
    /// **nicht** mitgeloescht wird: „Aus der Liste loeschen“ heisst **nicht**
    /// „Dateien loeschen“. Wer beides in **einem** Knopf macht, verliert die
    /// Datei ohne **zweite** Frage — das ist der Fehler, den Firefox 30 Jahre
    /// lang gemacht hat, bis man es getrennt hat.
    pub fn loesche_aus_liste(&mut self, i: usize) -> bool {
        if i < self.eintraege.len() {
            self.eintraege.remove(i);
            true
        } else {
            false
        }
    }

    /// Und den **Index** von einem Pfad — fuer „Im Ordner zeigen“ und „Datei
    /// oeffnen“, die aus der **Datei** heraus aufgerufen werden (Kontextmenue
    /// auf dem Eintrag), nicht ueber eine **Zeilennummer** aus der Anzeige.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **nicht** `iter().position()` mit `==`:
    /// `Eintrag` traegt `Instant`, und zwei Eintraege aus dem **selben** Download
    /// koennen in derselben **Nanosekunde** begonnen haben — dann waeren sie
    /// `==` und der **falsche** waeregefunden. Also der **Pfad**, der
    /// eindeutig ist (weil `resolve_filename` Eindeutigkeit **garantiert**).
    #[must_use]
    pub fn index_von_pfad(&self, pfad: &std::path::Path) -> Option<usize> {
        self.eintraege
            .iter()
            .position(|e| e.ziel == pfad || e.teil == pfad)
    }

    /// Und die Groesse **als Text** fuer die Liste.
    #[must_use]
    pub fn groesse_text(gesamt: Option<u64>) -> String {
        let Some(b) = gesamt else {
            return "unbekannt".to_string();
        };
        const EINHEIT: [&str; 5] = ["Byte", "kB", "MB", "GB", "TB"];
        let mut w = b as f64;
        let mut i = 0;
        while w >= 1024.0 && i < EINHEIT.len() - 1 {
            w /= 1024.0;
            i += 1;
        }
        if i == 0 {
            format!("{b} {}", EINHEIT[0])
        } else {
            format!("{w:.1} {}", EINHEIT[i])
        }
    }
}

/// Und der **`If-Range`-Wert** — und GEMESSENE ENTSCHEIDUNG, warum das eine
/// **eigene** Funktion ist und kein `format!` am Aufrufer: der Header ist
/// **doppelt** geklammert, und ein Aufrufer, der ihn **einmal** klammert,
/// schickt `If-Range: "abc"` — und die meisten Server vergleichen **ohne**
/// Anfuehrungszeichen, also **immer** `200` statt `206`, also **immer** von vorn.
///
/// Und GEMESSENE WAHRHEIT, die man kennen muss: **ohne** `ETag` gibt es
/// `If-Range` **nicht** sinnvoll. Also gibt diese Funktion `None` zurueck, wenn
/// kein `ETag` da ist — und der Aufrufer **muss** dann ohne `Range` neu
/// beginnen, statt eine **falsche** Fortsetzung zu riskieren.
#[must_use]
pub fn if_range(etag: Option<&str>) -> Option<String> {
    let e = etag?;
    // ### DER **NEUNTE** FALL von „ein Kommentar ist kein Test" — und der
    // einzige, bei dem **ich den Test** geschrieben hatte, der den **Fehler**
    // festhielt
    //
    // GEMESSEN in KONVERT_25 (`k29_knoepfe.rs`, gegen einen **echten**
    // `TcpListener`): der Server sah
    // `if-range: if-range: "k29"` — **zweimal** der Name. GEMESSENE FOLGE: er
    // antwortete **200 statt 206**, also lieferte er die **ganze** Datei, und
    // `laue` erkannte das und **verwarf** die Teildatei. Also hat **Pause +
    // Fortsetzen gegen einen echten Server noch nie funktioniert** — GEMESSEN
    // waren bisher nur `d3` (eigene `Hole`-Fabrik, sieht den Kopf **nicht**),
    // `d4` (lange Leser-Fabrik) und `k27` (kein Fortsetzen).
    //
    // GEMESSENE URSACHE: `narsil_net::strom::hole_stream` traegt den Wert **unter
    // dem Schluessel** `"if-range"` ein, und `fetch` schreibt dann
    // `if-range: <wert>`. Also **darf** hier **nur** der Wert stehen — und ich
    // hatte **zwei** Stellen (dieser Kommentar in `strom.rs` und diese Funktion)
    // fuer **dieselbe** Sache fuer **richtig** gehalten.
    //
    // GEMESSENE BEGRUENDUNG, warum trotzdem **hier** geklammert wird und **nicht**
    // in `strom.rs`: ein `ETag` kann **schon** Anfuehrungszeichen haben
    // (`W/"x"`), und das Klammern ist eine **Regel** (CSS-Backend-Wahrheit), die
    // an **einer** Stelle stehen muss. Sonst waere es die **zweite** Regel fuer
    // denselben Kopf.
    if e.starts_with('"') || e.starts_with("W/\"") {
        Some(e.to_string())
    } else {
        Some(format!("\"{e}\""))
    }
}
