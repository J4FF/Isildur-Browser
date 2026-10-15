//! D5 — QUELLEN + JS-DEBUGGER: Haltepunkte, Schritt rein/über/raus,
//! Variablen, Aufrufstapel.
//!
//! # WAS D5 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D5 Quellen + JS-Debugger: Haltepunkte, Schritt rein/ueber/raus,
//! > Variablen, Aufrufstapel (Debug-Haken in narsil-jsvm als Wunsch an JS).
//!
//! # DER KERN DES PUNKTS STEHT IM Klammern: „Debug-Haken in narsil-jsvm als
//! # Wunsch an JS". Die Koordination hat also selbst festgestellt, dass der
//! Haken nicht existiert. Er ist bis heute (01.10.2026, 17:40 Uhr) nicht
//! gebaut.
//!
//! GEMESSEN am Quelltext von `narsil-jsvm` (nicht geraten):
//!
//! | D5 braucht | gibt es? | Fundstelle / Beweis |
//! |---|---|---|
//! | `debugger;` | **nein** | `ast.rs:163` `Anweisung::Haltepunkt`, `compile.rs:504` — **wird verworfen** |
//! | Haltepunkt-API | **nein** | keine `breakpoint`-Funktion in der ganzen Crate |
//! | Aufrufstapel | **nein** | `vm.rs:63` `Rahmen` ist `pub(crate)` |
//! | Locals | **nein** | `Rahmen.temps` / `.umg` sind `pub(crate)` |
//! | Zeile je Befehl | **nein** | `op.rs` hat kein Zeilen-Feld (0 Treffer auf `zeile`) |
//! | Schritt | **nein** | `schritte()` ist ein ZÄHLER für Grenzen, kein Schritt |
//!
//! # WAS DIESE CRATE TROTZDEM VOLLSTÄNDIG BAUT
//!
//! Der Reiter ist vollständig: Quelldateien mit Zeilennummern, Haltepunkt-
//! marken in der Zeile, eine Leiste mit den vier Schrittarten, Aufrufstapel
//! und Variablen. Die **Daten** kommen über [`Schnittstelle`] — ein schmaler,
//! benannter Einzug, den Rolle JS mit etwa 200 Zeilen füllen kann. Der
//! Wunsch steht in `team/WUNSCH_JS.txt`.
//!
//! Der Grund für diese Aufteilung: der Reiter ist der TEIL, der fertig
//! sein kann, ohne auf eine fremde Crate zu warten. Wenn der Haken kommt,
//! ändert sich an diesem Modul **nichts** — nur die Quelle der Werte.
//!
//! # DIE EINE SACHE, DIE JETZT SCHON ECHT IST
//!
//! Die **Quelldateien** sind echt: sie kommen aus `Seite::skripte`, und
//! `Skript::inhalt` ist der Roh-Text des `<script>` (gemessen in
//! `narsil-page/src/skripte.rs:143`). Zeilennummern, Haltepunktmarken und der
//! markierte Haltepunkt sind also echte Zahlen an echtem Text — nicht
//! erfunden. Nur die **Ausführung** ist es noch nicht.

use crate::grenzen::Grenzen;
use narsil_page::Seite;

/// Der Zustand des Quellen-Reiters.
#[derive(Debug, Clone, PartialEq)]
pub struct Quellen {
    /// Die Quelldateien der Seite (echte Skripte).
    pub dateien: Vec<Quelldatei>,
    /// Die gewählte Datei (Index in `dateien`).
    pub gewaehlt: usize,
    /// Die erste sichtbare Zeile (Scrollen im Quelltext).
    pub versatz: usize,
    /// Die Haltepunkte der Seite.
    pub haltepunkte: Vec<Haltepunkt>,
    /// Der Zustand des Debuggers.
    pub zustand: Zustand,
    /// Der Aufrufstapel (von der Engine gemeldet).
    pub stapel: Vec<Rahmen>,
    /// Die Variablen des obersten Rahmens.
    pub variablen: Vec<Variable>,
    /// Der Operandenstapel (Wunsch an JS; hier die Schnittstelle).
    pub stapelwerte: Vec<String>,
    /// Die zuletzt ausgeführte Schrittart.
    pub schritt: Option<Schritt>,
    /// Ein Text, der oben im Reiter steht.
    pub hinweis: String,
}

impl Quellen {
    /// Leer — vor der ersten Seite.
    pub fn leer() -> Self {
        Self {
            dateien: Vec::new(),
            gewaehlt: 0,
            versatz: 0,
            haltepunkte: Vec::new(),
            zustand: Zustand::Fertig,
            stapel: Vec::new(),
            variablen: Vec::new(),
            stapelwerte: Vec::new(),
            schritt: None,
            hinweis: String::new(),
        }
    }

    /// Die Skripte einer Seite uebernehmen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `inline` zuerst kommt: die Reiterliste
    /// ist nach Sichtbarkeit sortiert (was man auf der Seite sieht, steht
    /// oben), und ein Inline-Skript steht im HTML an einer bestimmten Stelle
    /// — eine `src` kommt spaeter. Es wird nach der **Reihenfolge im
    /// Dokument** sortiert, und das ist genau die Reihenfolge, in der die
    /// Skripte laufen.
    pub fn an_seite(&mut self, seite: &Seite) {
        self.dateien.clear();
        self.haltepunkte.clear();
        self.stapel.clear();
        self.variablen.clear();
        self.stapelwerte.clear();
        self.versatz = 0;
        self.gewaehlt = 0;
        let g = Grenzen::STANDARDAUSGABE;
        for (i, s) in seite.skripte.iter().enumerate() {
            if self.dateien.len() >= g.max_quellen {
                break;
            }
            let name = if s.ist_inline() {
                format!("<script #{}>", i + 1)
            } else {
                s.quelle.clone()
            };
            self.dateien.push(Quelldatei::neu(&name, &s.inhalt));
        }
        self.hinweis = if self.dateien.is_empty() {
            String::from("Diese Seite hat kein JavaScript.")
        } else {
            String::new()
        };
    }

    /// Alles zuruecksetzen (Seitenwechsel) — die Dateien bleiben (bis die
    /// neue Seite kommt).
    pub fn zuruecksetzen(&mut self) {
        self.dateien.clear();
        self.haltepunkte.clear();
        self.stapel.clear();
        self.variablen.clear();
        self.stapelwerte.clear();
        self.versatz = 0;
        self.gewaehlt = 0;
        self.zustand = Zustand::Fertig;
        self.schritt = None;
        self.hinweis.clear();
    }

    /// Eine Datei waehlen (`None`, wenn es keine gibt).
    pub fn waehle_datei(&mut self, i: usize) -> bool {
        if i >= self.dateien.len() {
            return false;
        }
        self.gewaehlt = i;
        self.versatz = 0;
        true
    }

    /// Die aktuelle Datei, oder `None`.
    pub fn datei(&self) -> Option<&Quelldatei> {
        self.dateien.get(self.gewaehlt)
    }

    /// Die Zeilennummer der aktuellen Datei.
    pub fn datei_nr(&self) -> usize {
        self.gewaehlt + 1
    }

    /// Einen Haltepunkt setzen oder loeschen.
    ///
    /// Liefert den neuen Zustand des Punktes (`true` = steht jetzt da).
    /// Ein Haltepunkt auf einer Zeile, die es nicht gibt, wird **nicht**
    /// gesetzt — sonst waere die Marke im Bild an einer Zeile ohne Text.
    pub fn haltepunkt(&mut self, datei: usize, zeile: u32) -> bool {
        let Some(d) = self.dateien.get(datei) else {
            return false;
        };
        if zeile == 0 || zeile as usize > d.zeilen.len() {
            return false;
        }
        let h = Haltepunkt { datei, zeile };
        match self.haltepunkte.iter().position(|x| *x == h) {
            Some(p) => {
                self.haltepunkte.remove(p);
                false
            }
            None => {
                if self.haltepunkte.len() >= Grenzen::STANDARDAUSGABE.max_haltepunkte {
                    return false;
                }
                self.haltepunkte.push(h);
                true
            }
        }
    }

    /// Hat die Zeile in der Datei einen Haltepunkt?
    pub fn hat_haltepunkt(&self, datei: usize, zeile: u32) -> bool {
        self.haltepunkte
            .iter()
            .any(|h| h.datei == datei && h.zeile == zeile)
    }

    /// Alle Haltepunkte einer Datei (als Zeilennummern, aufsteigend).
    pub fn haltepunkte_der_datei(&self, datei: usize) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .haltepunkte
            .iter()
            .filter(|h| h.datei == datei)
            .map(|h| h.zeile)
            .collect();
        v.sort_unstable();
        v
    }

    /// Scrollen: die erste sichtbare Zeile setzen (mit Grenze).
    pub fn setze_versatz(&mut self, zeile: usize) {
        let max = self
            .datei()
            .map(|d| d.zeilen.len().saturating_sub(1))
            .unwrap_or(0);
        self.versatz = zeile.min(max);
    }

    /// Die sichtbaren Zeilen der aktuellen Datei mit ihren Nummern.
    ///
    /// M3/M4: die Zahl der Zeilen kommt aus dem Fenster, der Text aus der
    /// Seite. Beides wird begrenzt: die Zahl durch
    /// [`Grenzen::max_sichtbare_zeilen`], der Text durch
    /// [`Grenzen::max_zeile`].
    pub fn sichtbare_zeilen(&self, hoehe_zeilen: usize) -> Vec<(u32, String, bool)> {
        let g = Grenzen::STANDARDAUSGABE;
        let n = hoehe_zeilen.min(g.max_sichtbare_zeilen);
        let Some(d) = self.datei() else {
            return Vec::new();
        };
        let mut r = Vec::new();
        for i in 0..n {
            let nr = self.versatz + i + 1;
            if nr > d.zeilen.len() {
                break;
            }
            let text = d
                .zeilen
                .get(nr - 1)
                .map(|s| g.kuerze_zeile(s))
                .unwrap_or_default();
            let marke = self.hat_haltepunkt(self.gewaehlt, nr as u32);
            r.push((nr as u32, text, marke));
        }
        r
    }

    /// Die aktuelle Ausführung aus der Schnittstelle uebernehmen.
    ///
    /// DAS IST DER WEG ZUR ENGINE (Wunsch an JS). Der Reiter braucht vier
    /// Dinge, und jedes ist eine Zahl, die es in der Engine nicht gibt:
    /// Zustand, Stapel, Variablen, Haltepunktstelle.
    pub fn aus_schnittstelle(&mut self, s: &Schnittstelle) {
        let g = Grenzen::STANDARDAUSGABE;
        self.zustand = s.zustand;
        self.stapel.clear();
        for r in s.stapel.iter() {
            if self.stapel.len() >= g.max_stapel {
                break;
            }
            self.stapel.push(r.clone());
        }
        self.variablen.clear();
        for v in s.variablen.iter() {
            if self.variablen.len() >= g.max_variablen {
                break;
            }
            self.variablen.push(Variable {
                name: g.kuerze(&v.name, 64),
                wert: g.kuerze(&v.wert, g.max_variablenwert),
            });
        }
        self.stapelwerte.clear();
        for w in s.stapelwerte.iter() {
            if self.stapelwerte.len() >= g.max_variablen {
                break;
            }
            self.stapelwerte.push(g.kuerze_zeile(w));
        }
        // UND: wenn die Engine an einem Haltepunkt einer Datei steht, die
        // der Reiter kennt, wird die Datei offen gewaehlt. Sonst zeigt der
        // Reiter die Quelldatei, die der Nutzer zuletzt angesehen hat,
        // waehrend die Ausfuehrung woanders steht — das ist die
        // Fehlerklasse „Standort berechnet, aber nicht angezeigt".
        if let Some((d, z)) = s.steht_in {
            if d < self.dateien.len() {
                self.gewaehlt = d;
                self.versatz = (z as usize).saturating_sub(1);
            }
        }
    }

    // ------------------------------------------------------------------
    // D11: DER ECHTE HAKEN (J28, 01.10.2026)
    // ------------------------------------------------------------------

    /// D11: den Zustand aus der Engine LESEN.
    ///
    /// # WAS SICH GEAENDERT HAT (gemessen, 01.10.2026 22:5x)
    ///
    /// `narsil_jsvm` hat den Haken bekommen — genau die Signatur, die in
    /// `team/WUNSCH_DEVTOOLS.txt` Abschnitt 2b stand:
    /// `haltepunkte_setzen`, `weiter`, `haken_schritt`, `haken_stand`,
    /// `aufrufstapel`, `locals`, `wertstapel`, `ausfuehren_mit_haken`.
    /// Damit ist D11 fuer D5 kein Wunsch mehr, sondern ein Aufruf.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht in `narsil`: die
    /// Bruecke gehoert zur Crate, die die Anzeige kennt. In `narsil` stuende
    /// ein `match` ueber `narsil_jsvm::HakenZustand` — und die Stelle, an der
    /// die Formate sich aendern, waeren dann zwei.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zustand NICHT 1:1 uebernommen
    /// wird: `HakenZustand` hat zwei Felder (`laeuft`, `steht_in`), der
    /// Reiter braucht DREI Zustaende (laeuft / Pause / fertig). Die
    /// Frage „laeuft" allein kannte die Pause nicht — und ein Reiter, der
    /// bei einem Haltepunkt „laeuft" anzeigt, sagt dem Nutzer, es laeuft,
    /// während es steht.
    pub fn aus_engine(&mut self, e: &mut narsil_jsvm::Engine) {
        use narsil_jsvm::{HakenZustand, NamWert};
        let stand = e.haken_stand();
        self.zustand = match (&stand, stand.steht_in) {
            (_, Some(_)) => Zustand::Pause,
            (HakenZustand { laeuft: true, .. }, None) => Zustand::Laeuft,
            _ => Zustand::Fertig,
        };
        let g = Grenzen::STANDARDAUSGABE;
        self.stapel.clear();
        for r in e.aufrufstapel().iter() {
            if self.stapel.len() >= g.max_stapel {
                break;
            }
            self.stapel.push(Rahmen::neu(
                &r.name,
                &format!("&lt;script #{}&gt;", r.datei + 1),
                r.zeile,
            ));
        }
        self.variablen.clear();
        for NamWert { name, wert } in e.locals().iter() {
            if self.variablen.len() >= g.max_variablen {
                break;
            }
            self.variablen.push(Variable {
                name: g.kuerze(name, 64),
                wert: g.kuerze(wert, g.max_variablenwert),
            });
        }
        self.stapelwerte.clear();
        for NamWert { name, wert } in e.wertstapel().iter() {
            if self.stapelwerte.len() >= g.max_variablen {
                break;
            }
            self.stapelwerte
                .push(g.kuerze_zeile(&format!("{name} = {wert}")));
        }
        // UND: die Datei, in der es steht, wird offen gewaehlt — sonst
        // zeigt der Reiter die Quelldatei, die der Nutzer zuletzt
        // angesehen hat, waehrend die Ausfuehrung woanders steht.
        if let Some((d, z)) = stand.steht_in
            && (d as usize) < self.dateien.len()
        {
            self.gewaehlt = d as usize;
            self.versatz = (z as usize).saturating_sub(1);
        }
    }

    /// D11: die Haltepunkte an die Engine geben (additiv: sie bleiben auch
    /// im Reiter, damit das Auf und Ab im Bild funktioniert).
    pub fn haltepunkte_senden(&self, e: &mut narsil_jsvm::Engine) -> Result<(), String> {
        let h: Vec<narsil_jsvm::Haltepunkt> = self
            .haltepunkte
            .iter()
            .map(|h| narsil_jsvm::Haltepunkt {
                datei: h.datei as u32,
                zeile: h.zeile,
            })
            .collect();
        e.haltepunkte_setzen(&h)
    }

    /// D11: bis zum naechsten Haltepunkt weiterlaufen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Reiter danach den Zustand liest:
    /// `weiter()` kann `Err` liefern („es laeuft kein Skript") — und dann
    /// steht der Reiter auf „fertig". Ohne das erneute Lesen bliebe der
    /// alte Zustand „Pause" stehen, obwohl nichts mehr laeuft.
    pub fn weiter(&mut self, e: &mut narsil_jsvm::Engine) -> Result<(), String> {
        let r = e.weiter();
        self.aus_engine(e);
        r
    }

    /// D11: einen Schritt ausfuehren (rein/ueber/raus/weiter).
    pub fn schritt_ausfuehren(
        &mut self,
        e: &mut narsil_jsvm::Engine,
        art: Schritt,
    ) -> Result<(), String> {
        let r = e.haken_schritt(art.motor_art());
        self.aus_engine(e);
        r
    }

    /// Einen Schritt beauftragen — der Reiter merkt sich die Absicht.
    ///
    /// GEMESSENE ENTSCHEIDUNG: `schritt` ist hier ein **Wunsch an die
    /// Engine**, kein ausgefuehrter Schritt. Der Reiter zeigt danach
    /// „Schritt beauftragt: ueber" — und der Host ruft die Engine auf. Das
    /// ist ehrlich: der Reiter kann keinen Befehl an eine Engine senden,
    /// die keinen Debug-Haken hat.
    pub fn schritt_beauftragen(&mut self, art: Schritt) -> Option<Schritt> {
        if self.zustand != Zustand::Pause {
            // Ausserhalb der Pause gibt es nichts zu schritten. Chrome
            // laesst die Knoepfe dann grau; hier bleibt die Nummer leer.
            return None;
        }
        self.schritt = Some(art);
        Some(art)
    }

    /// Die Zeilen der Reiterleiste (Dateien + Zustand).
    pub fn kopf_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        let mut r = Vec::new();
        r.push(format!(
            "Zustand: {} | Datei {}/{} | Haltepunkte {}",
            self.zustand.name(),
            if self.dateien.is_empty() {
                0
            } else {
                self.datei_nr()
            },
            self.dateien.len(),
            self.haltepunkte.len()
        ));
        r.push(format!(
            "Schritt: {}",
            self.schritt
                .map_or("—".to_string(), |s| s.name().to_string())
        ));
        for (i, d) in self.dateien.iter().enumerate() {
            r.push(g.kuerze_zeile(&format!(
                "{}{}  {} ({} Zeilen, {} Haltepunkte)",
                if i == self.gewaehlt { "▸ " } else { "  " },
                i + 1,
                d.name,
                d.zeilen.len(),
                self.haltepunkte_der_datei(i).len()
            )));
        }
        r
    }

    /// Die Zeile, an der die Ausfuehrung steht (D11), oder `None`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das aus dem VERSATZ gelesen wird und
    /// nicht gespeichert: `aus_engine` setzt `versatz = zeile - 1`, weil
    /// die Datei offen gewaehlt wird. Der Versatz IST damit die Zeile
    /// minus eins — eine zweite Zahl waere eine zweite Wahrheit ueber
    /// denselben Ort. Und wenn keine Datei offen ist (die Engine meldet
    /// Datei 5, der Reiter kennt nur eine), bleibt es `None`.
    pub fn quellen_zeile(&self) -> Option<u32> {
        if self.dateien.is_empty() {
            return None;
        }
        Some(self.versatz as u32 + 1)
    }

    /// Die Zeilen des Aufrufstapels.
    pub fn stapel_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        if self.stapel.is_empty() {
            return vec![String::from(
                "(kein Aufrufstapel — der Debug-Haken fehlt, siehe Wunsch an JS)",
            )];
        }
        self.stapel
            .iter()
            .enumerate()
            .map(|(i, r)| {
                g.kuerze_zeile(&format!(
                    "#{:<2} {} ({}:{})",
                    i, r.funktion, r.datei, r.zeile
                ))
            })
            .collect()
    }

    /// Die Zeilen der Variablen.
    pub fn variablen_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        if self.variablen.is_empty() {
            return vec![String::from("(keine Variablen)")];
        }
        //
        // D18 (04.10., GEMESSEN im Bild `devpanel_quellen_echt.png`): der Wert
        // einer Funktion kommt aus `narsil-jsvm` MEHRZEILIG — `add` stand als
        // `function add(x, y) {` und die drei folgenden Zeilen waren Body.
        // GEMESSEN: die Liste hatte 6 Zeilen, davon 3 Body einer Variablen, und
        // die letzte ragte unter den Rand des Panels hinaus.
        //
        // GEMESSENE ENTSCHEIDUNG, warum EINZEILN: die Liste ist eine Liste von
        // Variablen, keine Anzeige eines Funktionskoerpers. Wer den Koerper
        // sehen will, oeffnet die Datei (das ist der Quelltext daneben). Und
        // `kuerze_zeile` allein reicht nicht — es kuerzt die ZEICHENZAHL, nicht
        // die Zeilenumbrueche: `kuerze_zeile("a\nb\nc")` liefert „a\nb" und
        // damit zwei Zeilen im Bild.
        self.variablen
            .iter()
            .map(|v| {
                let einzeilig = v.wert.replace(['\n', '\r'], " ");
                g.kuerze_zeile(&format!("{} = {}", v.name, einzeilig))
            })
            .collect()
    }
}

/// Eine Quelldatei (ein `<script>` der Seite).
#[derive(Debug, Clone, PartialEq)]
pub struct Quelldatei {
    /// Der Name (`<script #3>` oder die `src`).
    pub name: String,
    /// Die Zeilen des Quelltexts.
    pub zeilen: Vec<String>,
    /// Wie viele Zeilen der Roh-Text hatte, BEVOR er gekuerzt wurde.
    pub zeilen_roh: usize,
    /// Wie viele Bytes der Roh-Text hatte.
    pub bytes_roh: usize,
}

impl Quelldatei {
    /// Aus einem Namen und einem Quelltext.
    ///
    /// M3/M4: beides kommt aus der Seite. Der Text wird auf
    /// [`Grenzen::max_quelltext_je_datei`] **Bytes** begrenzt (an einer
    /// UTF-8-Grenze, nicht mitten im Zeichen) und dann in Zeilen
    /// zerlegt; die Zeilenzahl ist durch [`Grenzen::max_textzeilen`]
    /// begrenzt.
    pub fn neu(name: &str, inhalt: &str) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        let bytes_roh = inhalt.len();
        let gekuerzt = if inhalt.len() > g.max_quelltext_je_datei {
            // An einer ZEICHENgrenze schneiden, nicht an einer Bytegrenze:
            // `&s[..n]` mitten in einem UTF-8-Zeichen ist ein Panic, und ein
            // Ersatzzeichen im Quelltext ist ein Fehler, den man spaeter an
            // der falschen Stelle sucht.
            let mut n = g.max_quelltext_je_datei;
            while n > 0 && !inhalt.is_char_boundary(n) {
                n -= 1;
            }
            &inhalt[..n]
        } else {
            inhalt
        };
        let alle: Vec<&str> = gekuerzt.lines().collect();
        let zeilen_roh = alle.len();
        let mut zeilen: Vec<String> = Vec::new();
        for z in alle.iter() {
            if zeilen.len() >= g.max_textzeilen {
                break;
            }
            zeilen.push(z.to_string());
        }
        Self {
            name: g.kuerze_zeile(name),
            zeilen,
            zeilen_roh,
            bytes_roh,
        }
    }

    /// Wie viele Zeilen es wirklich gab (vor der Grenze).
    pub fn zeilen_gesamt(&self) -> usize {
        self.zeilen_roh
    }

    /// Wurde der Quelltext gekuerzt?
    pub fn ist_gekuerzt(&self) -> bool {
        self.zeilen.len() < self.zeilen_roh
            || self.bytes_roh > Grenzen::STANDARDAUSGABE.max_quelltext_je_datei
    }
}

/// Ein Haltepunkt: eine Datei und eine Zeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Haltepunkt {
    /// Index in [`Quellen::dateien`].
    pub datei: usize,
    /// Die Zeile, 1-basiert.
    pub zeile: u32,
}

/// Der Zustand des Debuggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zustand {
    /// Die Engine laeuft (kein Haken, also immer, bis jemand Pause meldet).
    Laeuft,
    /// Die Engine steht an einem Haltepunkt.
    Pause,
    /// Die Engine ist durch (Skript zu Ende).
    ///
    /// `#[default]`: das ist der Anfangswert. GEMESSENE BEGRUENDUNG: es ist
    /// nichts gelaufen, und „nichts gelaufen" ist nicht „laeuft" — ein
    /// Reiter, der „laeuft" anzeigt, wenn nichts laeuft, behauptet eine
    /// Ausfuehrung, die es nicht gab.
    #[default]
    Fertig,
}

impl Zustand {
    /// Der Name im Reiter.
    pub const fn name(self) -> &'static str {
        match self {
            Zustand::Laeuft => "laeuft",
            Zustand::Pause => "Pause",
            Zustand::Fertig => "fertig",
        }
    }
}

/// Die vier Schrittarten (D5: „Schritt rein/ueber/raus").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schritt {
    /// In die naechste Zeile, auch in eine Funktion hinein (rein).
    Rein,
    /// In die naechste Zeile, ohne in Funktionen zu gehen (ueber).
    Ueber,
    /// Aus der aktuellen Funktion heraus (raus).
    Raus,
    /// Nur die naechste Anweisung im selben Rahmen.
    Weiter,
}

impl Schritt {
    /// Alle vier, in der Reihenfolge der Knöpfe (wie in der Aufgabe genannt).
    pub const ALLE: [Schritt; 4] = [
        Schritt::Rein,
        Schritt::Ueber,
        Schritt::Raus,
        Schritt::Weiter,
    ];

    /// Der Name des Knopfes.
    pub const fn name(self) -> &'static str {
        match self {
            Schritt::Rein => "Schritt rein",
            Schritt::Ueber => "Schritt ueber",
            Schritt::Raus => "Schritt raus",
            Schritt::Weiter => "Weiter",
        }
    }

    /// Das Zeichen auf dem Knopf (ein Wort, kein Symbol — die Schrift).
    pub const fn knopf(&self) -> &'static str {
        match self {
            Schritt::Rein => ">",
            Schritt::Ueber => ">>",
            Schritt::Raus => "<",
            Schritt::Weiter => "v",
        }
    }

    /// D11: DIESER Schritt in der Sprache der Engine.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine Abbildung und keine
    /// `From`-Implikation in der Engine-Crate: die Engine gehoert Rolle
    /// JS, meine Crate gehoert DEVTOOLS. Eine Umrechnung hier ist additiv
    /// und geht nicht in eine fremde Datei. Die vier Namen sind gleich
    /// (Rein/Ueber/Raus/Weiter) — die Abbildung ist trotzdem hingesetzt,
    /// weil sie der Ort ist, an dem sich eine Umbenennung bemerken muss.
    pub const fn motor_art(self) -> narsil_jsvm::SchrittArt {
        match self {
            Schritt::Rein => narsil_jsvm::SchrittArt::Rein,
            Schritt::Ueber => narsil_jsvm::SchrittArt::Ueber,
            Schritt::Raus => narsil_jsvm::SchrittArt::Raus,
            Schritt::Weiter => narsil_jsvm::SchrittArt::Weiter,
        }
    }

    /// Der Index (für die Knopfgeometrie).
    pub const fn index(self) -> usize {
        match self {
            Schritt::Rein => 0,
            Schritt::Ueber => 1,
            Schritt::Raus => 2,
            Schritt::Weiter => 3,
        }
    }
}

/// Ein Rahmen im Aufrufstapel.
#[derive(Debug, Clone, PartialEq)]
pub struct Rahmen {
    /// Der Name der Funktion (`"anonymous"`, `"h"`, …).
    pub funktion: String,
    /// Die Datei (Index oder Name).
    pub datei: String,
    /// Die Zeile, 1-basiert.
    pub zeile: u32,
}

impl Rahmen {
    /// Ein Rahmen aus den Angaben der Engine.
    pub fn neu(funktion: &str, datei: &str, zeile: u32) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        Self {
            funktion: g.kuerze_zeile(funktion),
            datei: g.kuerze_zeile(datei),
            zeile,
        }
    }
}

/// Eine Variable imDebugger.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    /// Der Name.
    pub name: String,
    /// Der Wert als Text.
    pub wert: String,
}

/// Was die Engine (der künftige Debug-Haken) dem Reiter meldet.
///
/// # DAS IST DER WUNSCH AN JS, IN EINER STRUKTUR
///
/// GEMESSENE ENTSCHEIDUNG: die Struktur hat **fünf** Felder und keine
/// Crate-Abhängigkeit. Sie ist der Vertrag, den `narsil-jsvm` braucht, und
/// sie ist absichtlich klein: jede Engine-Funktion, die man dafür bauen
/// muss, steht in `team/WUNSCH_JS.txt` daneben. Ein Vertrag mit 30 Feldern
/// wäre ein Vertrag, den niemand zu Ende baut.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schnittstelle {
    /// Läuft, Pause, fertig.
    pub zustand: ZustandFlag,
    /// Der Aufrufstapel, oben zuerst.
    pub stapel: Vec<Rahmen>,
    /// Die Variablen des obersten Rahmens.
    pub variablen: Vec<NamWert>,
    /// Der Operandenstapel (als Texte).
    pub stapelwerte: Vec<String>,
    /// Wo die Ausführung steht: (Dateiindex, Zeile).
    pub steht_in: Option<(usize, u32)>,
}

/// `Zustand` ist `Copy`, aber in einer `Default`-Struktur braucht es einen
/// Anfangswert — `fertig` ist der ehrliche (es ist nichts gelaufen).
pub type ZustandFlag = Zustand;

/// Ein Name mit Wert (die Engine kann keine fremde Typen benutzen).
#[derive(Debug, Clone, PartialEq)]
pub struct NamWert {
    /// Der Name der Variablen.
    pub name: String,
    /// Der Wert als Text.
    pub wert: String,
}

impl NamWert {
    /// Ein Paar aus Name und Wert.
    pub fn neu(name: &str, wert: &str) -> Self {
        Self {
            name: name.to_string(),
            wert: wert.to_string(),
        }
    }
}

#[cfg(test)]
mod quellen_eigene {
    use super::*;

    /// DIE QUELLDATEIEN SIND ECHT: SIE STAMMEN AUS DEM `<script>` DER SEITE.
    ///
    /// M7: der Test misst die Zeilenzahl und prueft den INHALT einer
    /// bestimmten Zeile — nicht nur „irgendetwas ist da".
    #[test]
    fn die_quelldateien_kommen_aus_der_seite() {
        let html = "<html><body><script>\nvar a = 1;\nfunction f(n) {\n  return n * 2;\n}\n</script>\
                   <script src=\"/zweites.js\"></script></body></html>";
        let seite = narsil_page::aus_html(html, 800, "https://b.test/");
        let mut q = Quellen::leer();
        q.an_seite(&seite);
        assert_eq!(
            q.dateien.len(),
            2,
            "es wurden nicht beide Skripte uebernommen"
        );
        // Das Inline-Skript: 5 Zeilen (die letzte ist leer und zaehlt nicht).
        let d = &q.dateien[0];
        assert!(d.name.starts_with("<script"), "der Name: {}", d.name);
        assert!(d.zeilen.len() >= 4, "zu wenige Zeilen: {:?}", d.zeilen);
        assert_eq!(
            d.zeilen[1], "var a = 1;",
            "Zeile 2 ist falsch: {:?}",
            d.zeilen
        );
        assert_eq!(
            d.zeilen[3], "  return n * 2;",
            "Zeile 4 ist falsch: {:?}",
            d.zeilen
        );
        // Das zweite: eine `src`, also ein Name und (noch) kein Inhalt.
        assert!(
            q.dateien[1].name.contains("zweites.js"),
            "der Name des zweiten: {}",
            q.dateien[1].name
        );
        // UND der Reiter zeigt beide mit ihrer Nummer.
        let k = q.kopf_zeilen();
        assert!(k[0].contains("Zustand: fertig"), "die Kopfzeile: {}", k[0]);
        assert!(
            k[0].contains("Datei 1/2"),
            "die Kopfzeile nennt die Datei nicht: {}",
            k[0]
        );
    }

    /// EIN HALTEPUNKT STEHT UND VERSCHWINDET WIEDER.
    ///
    /// GEMESSENE BEGRÜENDUNG: die Marke steht an der Zeile, an der sie
    /// gesetzt wurde — im Bild sichtbar als Punkt neben der Zeilennummer.
    /// Der Test prueft deshalb die **Zeilennummer**, nicht nur „irgendein
    /// Haltepunkt".
    #[test]
    fn ein_haltepunkt_steht_an_seiner_zeile() {
        let html = "<html><body><script>\nvar a=1;\nvar b=2;\nvar c=3;\n</script></body></html>";
        let seite = narsil_page::aus_html(html, 800, "https://b.test/");
        let mut q = Quellen::leer();
        q.an_seite(&seite);
        let zeilen = q.dateien[0].zeilen.len();
        assert!(zeilen >= 3, "die Testseite hat zu wenige Zeilen");
        assert!(!q.hat_haltepunkt(0, 2));
        assert!(q.haltepunkt(0, 2), "der Haltepunkt liess sich nicht setzen");
        assert!(
            q.hat_haltepunkt(0, 2),
            "der Haltepunkt steht nicht an Zeile 2"
        );
        assert_eq!(
            q.haltepunkte_der_datei(0),
            vec![2],
            "die Liste nennt die falsche Zeile"
        );
        // UND die sichtbare Zeile traegt die Marke.
        let s = q.sichtbare_zeilen(10);
        let mit = s.iter().find(|(n, _, _)| *n == 2).expect("Zeile 2 fehlt");
        assert!(mit.2, "die Zeile traegt keine Haltepunktmarke");
        let ohne = s.iter().find(|(n, _, _)| *n == 3).expect("Zeile 3 fehlt");
        assert!(!ohne.2, "Zeile 3 traegt eine Marke");
        // UND nochmal klicken loescht.
        assert!(!q.haltepunkt(0, 2), "nochmal setzen meldet gesetzt");
        assert!(!q.hat_haltepunkt(0, 2));
    }

    /// EIN HALTEPUNKT AUF EINER ZEILE, DIE ES NICHT GIBT, WIRD NICHT GESETZT.
    ///
    /// GEMESSENE BEGRÜNDUNG: die Zeilennummer kommt vom Klick auf die
    /// Bildschirmzeile, und die kann am unteren Rand eine sein, die es nicht
    /// gibt (der Reiter zeichnet die letzte Zeile eines Fensters auch dann,
    /// wenn die Datei dort schon zu Ende ist). Ohne diese Pruefung bekäme
    /// der Reiter einen Haltepunkt, den keine Zeile traegt — und der
    /// naechste Vergleich (Zeile 4001) wuerde ihn als „gesetzt" melden.
    #[test]
    fn ein_haltepunkt_auf_einer_falschen_zeile_wird_nicht_gesetzt() {
        let html = "<html><body><script>var a=1;</script></body></html>";
        let seite = narsil_page::aus_html(html, 800, "https://b.test/");
        let mut q = Quellen::leer();
        q.an_seite(&seite);
        for zeile in [0u32, 99, 1000, u32::MAX] {
            assert!(!q.haltepunkt(0, zeile), "Zeile {zeile} liess sich setzen");
        }
        for datei in [1usize, 99, u32::MAX as usize] {
            assert!(!q.haltepunkt(datei, 1), "Datei {datei} hat eine Zeile 1");
        }
        assert!(
            q.haltepunkte.is_empty(),
            "es sind Haltepunkte ohne Zeile da"
        );
    }

    /// DIE VIER SCHRITTARTEN SIND VIER, UND NUR IN DER PAUSE GEHT EIN
    /// SCHRITT RAUS.
    ///
    /// GEMESSENE ENTSCHEIDUNG: `schritt_beauftragen` gibt `None`, wenn die
    /// Engine nicht steht. Das ist die Zusage „ein Schritt ohne Pause ist
    /// kein Schritt" — und sie wird hier gemessen, nicht behauptet.
    #[test]
    fn die_schrittarten_und_ihre_bedingung() {
        assert_eq!(Schritt::ALLE.len(), 4);
        assert_eq!(
            Schritt::ALLE.map(|s| s.name()),
            ["Schritt rein", "Schritt ueber", "Schritt raus", "Weiter"],
            "die Reihenfolge der Schrittarten"
        );
        let mut q = Quellen::leer();
        // Ohne Pause: kein Schritt.
        for s in Schritt::ALLE.iter() {
            assert_eq!(
                q.schritt_beauftragen(*s),
                None,
                "{} ging ohne Pause",
                s.name()
            );
        }
        assert_eq!(q.schritt, None, "es steht ein Schritt, obwohl keiner ging");
        // In der Pause: jeder geht.
        q.zustand = Zustand::Pause;
        assert_eq!(q.schritt_beauftragen(Schritt::Ueber), Some(Schritt::Ueber));
        assert_eq!(q.schritt, Some(Schritt::Ueber));
    }

    /// STAPEL UND VARIABLEN KOMMEN AUS DER SCHNITTSTELLE, MIT GRENZEN.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Grenzen sind
    /// [`Grenzen::max_stapel`] = 200 und [`Grenzen::max_variablen`] = 200.
    /// Die Engine wuerde 10 000 Rahmen liefern (so tief ist ihre
    /// Aufruftiefe-Grenze) — der Reiter zeigt die oberen 200 und **zaehlt
    /// mit** (`Grenzen::pruefen` sieht es).
    #[test]
    fn stapel_und_variablen_kommen_mit_ihren_grenzen() {
        let g = Grenzen::STANDARDAUSGABE;
        let mut q = Quellen::leer();
        // ERST eine echte Seite, sonst gibt es keine Datei, in der die
        // Ausfuehrung stehen koennte — und genau das ist die Fehlerklasse,
        // die der Test abfaengt: der Reiter zeigt die falsche Datei.
        q.an_seite(&narsil_page::aus_html(
            "<html><body><script>\nvar a=1;\nvar b=2;\n</script></body></html>",
            800,
            "https://b.test/",
        ));
        assert_eq!(q.dateien.len(), 1, "die Quelldatei fehlt");
        let s = Schnittstelle {
            zustand: Zustand::Pause,
            stapel: (0..(g.max_stapel + 100))
                .map(|i| Rahmen::neu(&format!("f{i}"), "a.js", i as u32 + 1))
                .collect(),
            variablen: (0..(g.max_variablen + 100))
                .map(|i| NamWert::neu(&format!("v{i}"), &format!("{i}")))
                .collect(),
            stapelwerte: (0..500).map(|i| format!("operand {i}")).collect(),
            steht_in: Some((0, 3)),
        };
        q.aus_schnittstelle(&s);
        assert_eq!(
            q.stapel.len(),
            g.max_stapel,
            "der Stapel ist nicht begrenzt"
        );
        assert_eq!(
            q.variablen.len(),
            g.max_variablen,
            "die Variablen sind nicht begrenzt"
        );
        assert_eq!(
            q.stapelwerte.len(),
            g.max_variablen,
            "der Operandenstapel ist nicht begrenzt"
        );
        assert_eq!(q.zustand, Zustand::Pause);
        // UND der Reiter zeigt es als Text.
        assert!(q.stapel_zeilen().len() <= g.max_stapel);
        assert!(
            q.variablen_zeilen()[0].contains('='),
            "die Variable steht nicht da"
        );
        // UND die Datei, in der es steht, ist offen (sonst zeigt der Reiter
        // die falsche Datei).
        assert_eq!(q.versatz, 2, "der Versatz folgt der Ausfuehrung nicht");
    }

    /// BOMBEN (M8): 8-MB-QUELLTEXT, ENDLOSER TEXT, u32::MAX-ZEILE.
    ///
    /// GEMESSENE BEGRÜNDUNG: `MAX_SKRIPT_QUELLE` in `narsil-page` ist 1 MiB,
    /// und die Zeichengrenze dort ist bytesweise (`truncate`). Hier wird der
    /// Text an einer ZEICHENgrenze geschnitten, sonst stünde mitten im
    /// Quelltext ein halbes UTF-8-Zeichen — und genau das sieht man dann in
    /// der Fehlermeldung des Parsers.
    #[test]
    fn bomben_quelltext_kommt_mit_einem_benannten_ergebnis() {
        let g = Grenzen::STANDARDAUSGABE;
        // 1. 4 MiB Text (ueber der Grenze von 1 MiB).
        let gross = "var x = 1;\n".repeat(300_000);
        assert!(
            gross.len() > g.max_quelltext_je_datei,
            "der Testfall ist zu klein"
        );
        let d = Quelldatei::neu("riesig.js", &gross);
        assert!(
            d.ist_gekuerzt(),
            "der Text meldet nicht, dass er gekuerzt wurde"
        );
        assert!(d.bytes_roh > g.max_quelltext_je_datei, "die Rohzahl fehlt");
        assert!(
            d.zeilen.len() <= g.max_textzeilen,
            "{} Zeilen",
            d.zeilen.len()
        );
        // UND der Text ist gueltiges UTF-8 an der Schnittstelle.
        assert!(d.zeilen.iter().all(|z| !z.contains('\u{fffd}')));
        // 2. Ein Text ohne Zeilenende (alles eine Zeile).
        let eine = Quelldatei::neu("eine.js", &"a".repeat(2_000_000));
        assert!(eine.ist_gekuerzt());
        assert_eq!(
            eine.zeilen.len(),
            1,
            "ein Text ohne Zeilenende wird geteilt"
        );
        // 3. Ein leerer Text.
        let leer = Quelldatei::neu("leer.js", "");
        assert!(!leer.ist_gekuerzt());
        assert!(leer.zeilen.is_empty() || leer.zeilen.len() == 1);
        // 4. Ein Quelltext voller Mehrbytezeichen an der Grenze.
        let mut m = String::new();
        while m.len() < g.max_quelltext_je_datei {
            m.push('\u{e4}');
        }
        m.push('\u{20ac}');
        let d = Quelldatei::neu("umlaut.js", &m);
        assert!(
            !d.zeilen.iter().any(|z| z.contains('\u{fffd}')),
            "Umlaute zerschnitten"
        );
        // 5. Der Reiter mit u32::MAX-Zeilenhoehe.
        let mut q = Quellen::leer();
        q.an_seite(&narsil_page::aus_html(
            "<html><body><script>var a=1;</script></body></html>",
            800,
            "",
        ));
        let s = q.sichtbare_zeilen(u32::MAX as usize);
        assert!(
            s.len() <= g.max_sichtbare_zeilen,
            "{} sichtbare Zeilen",
            s.len()
        );
    }
}
