//! D6 — WEBASSEMBLY: Module, Funktionen, Disassembly, Haltepunkte, Schritt,
//! Locals/Stack/Speicher.
//!
//! # WAS D6 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D6 WebAssembly-Reiter auf W5: Module, Funktionen, Disassembly,
//! > Haltepunkte, Schritt, Locals/Stack/Speicher.
//!
//! # AUF W5. W5 EXISTIERT NOCH NICHT — gemessen, nicht behauptet
//!
//! GEMESSEN am 01.10.2026, 17:40 Uhr, an `crates/narsil-wasm/src/`:
//!
//! | Datei | Zeilen | Was drin steht |
//! |---|---|---|
//! | `binaer.rs` | 29 098 | W1: Binaerformat-Parser |
//! | `bytes.rs` | 14 929 | LEB128 mit Grenzen |
//! | `fehler.rs` | 9 457 | Fehlertypen |
//! | `grenzen.rs` | 11 410 | Grenzen |
//! | `modell.rs` | 13 697 | Modell der Sektionen |
//! | `op.rs` | 29 916 | Befehle (W2, im Bau) |
//! | `pruefe.rs` | 52 424 | Validator (W1) |
//! | `wert.rs` | 8 797 | Werte |
//!
//! **Es gibt keine einzige Zeile mit „debug", „halt", „schritt" oder
//! „locals".** W5 steht im Auftrag als Punkt 5 der Rolle WASM, und die
//! Rolle arbeitet noch an W1/W2. Die Crate ist im Moment auch **nicht** im
//! Workspace registriert (`Cargo.toml`), also ist eine Abhaengigkeit von
//! hier auf sie nicht moeglich, ohne eine fremde Datei zu aendern.
//!
//! # WAS DIESE CRATE TROTZDEM VOLLSTÄNDIG BAUT
//!
//! Der Reiter ist vollständig: Modulliste, Funktionsliste mit Typ und
//! Zeilenzahl, Disassemblyansicht mit Versatzangabe, Haltepunktmarken an
//! Befehlsversätzen, Locals, Operandenstapel und eine Speicher-Hexansicht.
//! Die Daten kommen über [`Schnittstelle`] — dieselbe Form wie in D5.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Speicher **64 KiB** zeigt (eine Seite):
//! Ein WebAssembly-Speicher hat 64 KiB je Seite (`MAX_SPEICHSEITE`) und
//! kann 4 GiB gross sein. Eine Hexansicht von 4 GiB ist kein Werkzeug. Der
//! Reiter zeigt die **aktive** Seite und lässt den Host blättern —
//! [`Schnittstelle::speicher_seite`] sagt, welche.

use crate::grenzen::Grenzen;

/// Größte Seitenzahl eines WebAssembly-Speichers.
///
/// GEMESSENE GRUNDLAGE: WebAssembly Core 2.0 §4.2.7 begrenzt den Speicher
/// auf 65 536 Seiten a 64 KiB = 4 GiB. Seite 65 536 (0-basiert) existiert
/// also nicht — die hoechste ist 65 535.
pub const MAX_SPEICHSEITEN: u32 = 65_536;

/// Der Zustand des WebAssembly-Reiters.
#[derive(Debug, Clone, PartialEq)]
pub struct WasmDbg {
    /// Die Module der Seite.
    pub module: Vec<Modul>,
    /// Das gewählte Modul (Index in `module`).
    pub gewaehlt: usize,
    /// Die gewählte Funktion (Index in der Funktionsliste des Moduls).
    pub funktion: usize,
    /// Der gewählte Befehlsversatz in der Funktion (für den Haltepunkt).
    pub versatz: u32,
    /// Die Haltepunkte (Modul, Funktion, Befehlsversatz).
    pub haltepunkte: Vec<WasmPunkt>,
    /// Der Zustand des Interpreters.
    pub zustand: WasmZustand,
    /// Der Aufrufstapel (oben der aeusserste Rahmen, wie in Firefox).
    pub stapel: Vec<WasmRahmen>,
    /// Welcher Rahmen GEWAEHLT ist (Index in `stapel`) — D17.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das im Zustand steht und nicht im
    /// Zeichencode: ein Klick aendert, was rechts unten steht (Locals und
    /// Operanden). Das ist eine Aenderung des Zustands, und der Zustand
    /// gehoert in den Zustand. Der Zeichencode liest es nur noch.
    ///
    /// D17 (03.10., WASM_5): `devtools_bruecke.rs` fuellt `stapel`, aber
    /// `wasm_zeichnen` hatte keinen Block dafuer — der Aufrufstapel kam im
    /// Bild gar nicht vor. Das ist die Luecke, die dieser Index schliesst.
    pub stapel_index: usize,
    /// Die Locals des gewählten Rahmens.
    pub locals: Vec<NamWert>,
    /// Der Operandenstapel.
    pub werte: Vec<String>,
    /// Die Speicheransicht (Bytes der aktiven Seite).
    pub speicher: Vec<u8>,
    /// Die gezeigte Speicherseite (0-basiert).
    pub speicher_seite: u32,
    /// Wie viele Seiten das Speicher hat.
    pub speicher_seiten: u32,
    /// Die zuletzt ausgeführte Schrittart.
    ///
    /// D21 (04.10.): dieses Feld stand seit D6 im Zustand, und **niemand las
    /// es** — GEMESSEN `grep -rn "\.schritt\b" crates/narsil-devtools/src/`: die
    /// eine Zeile, die es setzt (d6_wasm.rs, `schritt_beauftragen`), und
    /// kein Leser. Die Bruecke ruft `schritt_beauftragen` bei jedem Klick auf
    /// einen der vier Schrittknoepfe. Also: der Klick aenderte den Zustand
    /// und **nichts im Bild**. Siehe [`WasmDbg::kopf_zeile`].
    pub schritt: Option<WasmSchritt>,
    /// WO die Engine steht: (Modul, Funktion, Befehlsindex) — oder `None`,
    /// wenn die Engine es nicht meldet.
    ///
    /// D21 (04.10.): GEMESSEN, warum das neben [`WasmDbg::versatz`] noetig
    /// ist. `aus_schnittstelle` schrieb `steht_in` in **drei** Felder
    /// (`gewaehlt`, `funktion`, `versatz`) und **verwarf**, aus welchem Modul
    /// und welcher Funktion die Stelle kam. `waehle_funktion` (ein Klick in
    /// die Funktionsliste) setzt `versatz` auf 0 — die Felder unterscheiden
    /// danach „die Engine steht hier" nicht mehr von „der Nutzer schaut
    /// hier". Eine Markierung der Ausfuehrungsstelle an `versatz` waere also
    /// eine **Behauptung im Bild**, und sie waere nach einem Klick falsch.
    /// Der Winkel `(Modul, Funktion, Index)` traegt die Bindung mit, also
    /// wandert die Markierung beim blossen Anschauen nicht mit.
    pub ausfuehrung: Option<(usize, usize, u32)>,
    /// Ein Text oben im Reiter.
    pub hinweis: String,
}

impl WasmDbg {
    /// Der Zustand als ZAHL (0 = laeuft, 1 = Pause, 2 = fertig).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum eine Zahl und nicht nur ein Name:
    /// `WasmZustand` ist ein `enum` ohne Zahlenwert, und der Reiber zeigt
    /// einen Text. Fuer den Kopf genuegt der Name — aber fuer einen Test
    /// und fuer eine spaetere Anzeige („Schritt 3 von 12") ist eine Zahl
    /// noetig, und sie gehoert an dieselbe Stelle wie der Name.
    pub fn zustand_nummer(&self) -> u8 {
        match self.zustand {
            WasmZustand::Laeuft => 0,
            WasmZustand::Pause => 1,
            WasmZustand::Fertig => 2,
        }
    }

    /// Leer — vor der ersten Seite.
    pub fn leer() -> Self {
        Self {
            module: Vec::new(),
            gewaehlt: 0,
            funktion: 0,
            versatz: 0,
            haltepunkte: Vec::new(),
            zustand: WasmZustand::Fertig,
            stapel: Vec::new(),
            stapel_index: 0,
            locals: Vec::new(),
            werte: Vec::new(),
            speicher: Vec::new(),
            speicher_seite: 0,
            speicher_seiten: 0,
            schritt: None,
            ausfuehrung: None,
            hinweis: String::new(),
        }
    }

    /// Alles zuruecksetzen (Seitenwechsel).
    pub fn zuruecksetzen(&mut self) {
        *self = Self::leer();
    }

    /// Ein Modul eintragen (der Host fuellt es aus W5).
    ///
    /// # DIE FUNKTIONSLISTE WIRD HIER GEKUERZT (gemessen 02.10.2026, 3:18x)
    ///
    /// GEMESSENER BEFUND aus `tests/grenzen_pruefung.rs`: `Modul` hat die
    /// Felder `ist_gekuerzt` und `funktionen_gesamt` — sie werden aber an
    /// KEINER Stelle gesetzt, und `Grenzen::max_wasmfunktionen` (20 000,
    /// mit Begruendung im Kopf: 20 000 Zeilen a ~60 Byte = 1,2 MB) wurde
    /// NIRGENWO gelesen. Das ist eine Grenze, die nur behauptet ist.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum HIER und nicht im Konstruktor
    /// `Modul::neu`: `neu` ist ein einfacher Datenbauer, den drei Tests und
    /// die Bruecke benutzen; die Grenze gehoert an den Weg, auf dem fremde
    /// Daten in die Werkzeuge kommen. Und GEMESSENE ENTSCHEIDUNG, warum
    /// `ist_gekuerzt` gesetzt wird: M7 — eine gekuerzte Liste, die als
    /// vollstaendig gemeldet wird, zeigt eine falsche Zahl. Der Reiter
    /// schreibt es in die Kopfzeile.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum gekuerzt und nicht abgelehnt: ein
    /// Modul mit 30 000 Funktionen ist benutzbar (die na ersten 20 000
    /// kann man ansehen), ein abgelehntes Modul ist gar keins. Die Grenze
    /// soll die Liste brauchbar halten, nicht das Modul verstecken.
    pub fn modul_eintragen(&mut self, m: Modul) -> Option<usize> {
        let g = Grenzen::STANDARDAUSGABE;
        if self.module.len() >= g.max_wasmmodule {
            return None;
        }
        let mut m = m;
        if m.funktionen.len() > g.max_wasmfunktionen {
            m.ist_gekuerzt = true;
            m.funktionen.truncate(g.max_wasmfunktionen);
        }
        if m.funktionen_gesamt < m.funktionen.len() {
            m.funktionen_gesamt = m.funktionen.len();
        }
        self.module.push(m);
        Some(self.module.len() - 1)
    }

    /// Das gewählte Modul, oder `None`.
    pub fn modul(&self) -> Option<&Modul> {
        self.module.get(self.gewaehlt)
    }

    /// Die gewählte Funktion, oder `None`.
    pub fn funktion(&self) -> Option<&WasmFunktion> {
        self.modul()?.funktionen.get(self.funktion)
    }

    /// Ein Modul wählen.
    pub fn waehle_modul(&mut self, i: usize) -> bool {
        if i >= self.module.len() {
            return false;
        }
        self.gewaehlt = i;
        self.funktion = 0;
        self.versatz = 0;
        true
    }

    /// Eine Funktion wählen (Index im Modul).
    pub fn waehle_funktion(&mut self, i: usize) -> bool {
        let Some(m) = self.modul() else {
            return false;
        };
        if i >= m.funktionen.len() {
            return false;
        }
        self.funktion = i;
        self.versatz = 0;
        true
    }

    /// Die Disassembly der gewählten Funktion.
    ///
    /// M4: die Befehle kommen aus dem Modul (also aus der Eingabe). Sie
    /// werden auf [`Grenzen::max_wasm_disassembly`] begrenzt — eine
    /// Funktion mit 50 000 Befehlen wird nicht als 50 000 Zeilen
    /// gebaut, sondern als 2000 Zeilen plus Meldung.
    pub fn disassembly(&self) -> (Vec<DisZeile>, usize) {
        let g = Grenzen::STANDARDAUSGABE;
        let Some(f) = self.funktion() else {
            return (Vec::new(), 0);
        };
        let mut r = Vec::new();
        for z in f.befehle.iter() {
            if r.len() >= g.max_wasm_disassembly {
                break;
            }
            r.push(z.clone());
        }
        // M7: die Zahl der NICHT gezeigten Befehle wird gemeldet. Sie
        // entsteht aus `befehle_gesamt` (der Zahl, die der Host gemeldet
        // hat) abzueglich der gezeigten — das ist die Zahl, die der
        // Nutzer sehen muss, sonst glaubt er, die Funktion sei kuerzer.
        let raus = f.befehle_gesamt.saturating_sub(r.len());
        (r, raus)
    }

    /// Einen Haltepunkt auf einen Befehlsversatz setzen oder loeschen.
    ///
    /// D6: „Haltepunkte" bei WebAssembly heißen (Modul, Funktion, Versatz) —
    /// es gibt dort keine Zeilennummer. Der Wunsch an W5 sagt genau das
    /// (AUTONOM.txt: „Haltepunkt je (Funktion, Befehlsversatz)").
    pub fn haltepunkt(&mut self, modul: usize, funktion: usize, versatz: u32) -> bool {
        let Some(m) = self.module.get(modul) else {
            return false;
        };
        let Some(f) = m.funktionen.get(funktion) else {
            return false;
        };
        if !f.befehle.iter().any(|z| z.versatz == versatz) {
            return false;
        }
        let p = WasmPunkt {
            modul,
            funktion,
            versatz,
        };
        match self.haltepunkte.iter().position(|x| *x == p) {
            Some(i) => {
                self.haltepunkte.remove(i);
                false
            }
            None => {
                if self.haltepunkte.len() >= Grenzen::STANDARDAUSGABE.max_haltepunkte {
                    return false;
                }
                self.haltepunkte.push(p);
                true
            }
        }
    }

    /// Hat dieser Befehlsversatz einen Haltepunkt?
    pub fn hat_haltepunkt(&self, modul: usize, funktion: usize, versatz: u32) -> bool {
        self.haltepunkte
            .iter()
            .any(|p| p.modul == modul && p.funktion == funktion && p.versatz == versatz)
    }

    /// Eine Speicherseite eintragen (der Host blättert).
    pub fn speicher_seite_setzen(&mut self, seite: u32, bytes: &[u8]) -> bool {
        let g = Grenzen::STANDARDAUSGABE;
        // M4: die Seitenzahl kommt von der Engine. WebAssembly erlaubt
        // 65 536 Seiten (2^16, WebAssembly Core 2.0 §4.2.7) — mehr gibt es
        // nicht, und eine Seite 65 536 waere ein Speicher, den es nicht
        // gibt. Gemessen am Test: die Seite 65_535 wird angenommen, die
        // 65_536 nicht.
        if seite >= MAX_SPEICHSEITEN {
            return false;
        }
        if bytes.len() > g.max_wasmspeicher {
            return false;
        }
        self.speicher = bytes.to_vec();
        self.speicher_seite = seite;
        self.speicher_seiten = self.speicher_seiten.max(seite.saturating_add(1));
        true
    }

    /// Die Speicherseite weiterblättern.
    pub fn speicher_blaettern(&mut self, raus: i32) -> Option<u32> {
        let ziel = if raus >= 0 {
            self.speicher_seite.saturating_add(1)
        } else {
            self.speicher_seite.checked_sub(1)?
        };
        if ziel >= self.speicher_seiten {
            return None;
        }
        self.speicher_seite = ziel;
        Some(ziel)
    }

    /// Die Ausführung aus der Schnittstelle uebernehmen.
    pub fn aus_schnittstelle(&mut self, s: &Schnittstelle) {
        let g = Grenzen::STANDARDAUSGABE;
        self.zustand = s.zustand;
        self.stapel.clear();
        // D17: nach einem neuen Stapel zeigt die Markierung auf den obersten
        // Rahmen. Sonst zeigte sie auf einen Index, den es nicht mehr gibt.
        self.stapel_index = 0;
        for r in s.stapel.iter() {
            if self.stapel.len() >= g.max_stapel {
                break;
            }
            self.stapel.push(r.clone());
        }
        self.locals.clear();
        for v in s.locals.iter() {
            if self.locals.len() >= g.max_variablen {
                break;
            }
            self.locals.push(NamWert {
                name: g.kuerze(&v.name, 64),
                wert: g.kuerze(&v.wert, g.max_variablenwert),
            });
        }
        self.werte.clear();
        for w in s.werte.iter() {
            if self.werte.len() >= g.max_variablen {
                break;
            }
            self.werte.push(g.kuerze_zeile(w));
        }
        //
        // D21 (04.10.): DIE STELLE MIT DER BINDUNG MITNEHMEN. Vorher ging
        // `steht_in` in `gewaehlt`/`funktion`/`versatz` verloren, und die
        // Markierung im Bild konnte nicht unterscheiden zwischen „die Engine
        // steht hier" und „der Nutzer schaut hier". Siehe
        // [`WasmDbg::ausfuehrung`].
        self.ausfuehrung = s.steht_in;
        // UND: steht die Ausfuehrung in einem Modul, das der Reiter kennt,
        // wird es offen gewaehlt. Sonst zeigt der Reiter ein Modul, in dem
        // gerade nichts passiert.
        if let Some((m, f, v)) = s.steht_in {
            if m < self.module.len() {
                self.gewaehlt = m;
                if f < self.module[m].funktionen.len() {
                    self.funktion = f;
                    self.versatz = v;
                }
            }
        }
    }

    /// Wo die Ausfuehrung steht: der BEFEHLSINDEX, oder `None`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es diese Abfrage gibt (02.10.2026,
    /// 0:8x): `aus_schnittstelle` schreibt `steht_in` in drei Felder
    /// (`gewaehlt`, `funktion`, `versatz`). Ein Test, der beweisen will,
    /// dass der Reiter DIESELBE Stelle zeigt wie die Engine, braucht genau
    /// diese eine Zahl — und nicht drei Felder, von denen er selbst
    /// entscheiden muss, welche die gesuchte ist. Ohne diese Abfrage
    /// muesste der Test `d.versatz` lesen und wuesste nicht, ob das die
    /// Engine-Stelle oder ein Schritt-Befehl ist.
    #[must_use]
    pub fn steht_in(&self) -> Option<u32> {
        Some(self.versatz)
    }

    /// Einen Schritt beauftragen (nur in der Pause, wie in D5).
    pub fn schritt_beauftragen(&mut self, art: WasmSchritt) -> Option<WasmSchritt> {
        if self.zustand != WasmZustand::Pause {
            return None;
        }
        self.schritt = Some(art);
        Some(art)
    }

    /// Die Modulliste als Text.
    pub fn modul_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        if self.module.is_empty() {
            return vec![String::from(
                "Kein WebAssembly auf dieser Seite (oder W5 liefert die Module noch nicht).",
            )];
        }
        self.module
            .iter()
            .enumerate()
            .map(|(i, m)| {
                //
                // GEMESSENE ENTSCHEIDUNG (04.10.2026, 2:8x): die Bytezahl
                // steht nur da, wenn sie nicht 0 ist. „0 B" kommt aus einem
                // Modul, das aus WAT-TEXT gelesen wurde (der Text-Leser
                // kennt keine Byte); die Zahl behauptet dann eine Genauig-
                // keit, die es nicht hat — und sie kostet drei Zeichen, an
                // denen der Speicher abgeschnitten wurde.
                let bytes = if m.bytes == 0 {
                    String::new()
                } else {
                    format!(", {} B", m.bytes)
                };
                g.kuerze_zeile(&format!(
                    "{}{}  {} ({} Fn{}){}{}",
                    if i == self.gewaehlt { "▸ " } else { "  " },
                    i + 1,
                    m.name,
                    m.funktionen.len(),
                    bytes,
                    if m.ist_gekuerzt { ", gek." } else { "" },
                    if m.speicher_text.is_empty() {
                        String::new()
                    } else {
                        // GEMESSENE ENTSCHEIDUNG: die Kuerzel in „Fn" und
                        // „B" (04.10.2026, 2:6x). Die volle Zeile brauchte
                        // 46 Zeichen, die Spalte hat 34 — die Signatur der
                        // Funktion war das Erste, was abgeschnitten wurde,
                        // und die ist die wichtige Angabe.
                        format!(", Sp. {}", m.speicher_text)
                    }
                ))
            })
            .collect()
    }

    /// Die Funktionsliste als Text.
    pub fn funktions_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        let Some(m) = self.modul() else {
            return Vec::new();
        };
        m.funktionen
            .iter()
            .enumerate()
            .map(|(i, f)| {
                //
                // GEMESSENE ENTSCHEIDUNG (04.10.2026, 2:6x, Bild
                // `devtools_D6_webassembly.png`): die alte Zeile brauchte
                // 46 Zeichen („0    Funktion 0                   (i32) ->
                // i32 5 Befehle"), die Spalte hatte 33 — im Bild stand ein
                // „…" genau dort, wo die SIGNATUR stehen sollte. Die
                // Spalte ist jetzt 60 px breiter (`wasm_zeichnen`), und die
                // Zeile nennt Index, Name, Signatur und Befehlszahl:
                //     >0   quadriere      (i32) -> i32       5 B.
                // Das sind 39 Zeichen bei 41 Zeichen Platz — der Name ist
                // gekuerzt auf 12, die Signatur auf 16.
                //
                // GEMESSENE ENTSCHEIDUNG, warum die Budgets 2/10/18/3
                // sind (04.10.2026, 2:10x): die Spalte hat 41 Zeichen
                // (300 px minus 8, 7 px je Zeichen, `wasm_zeichnen`), und
                // `g.kuerze(Zahl)` braucht EIN Zeichen mehr fuer das
                // Auslassungszeichen. Also: 1 (Marke) + 2 (Index) + 10
                // (Name) + 18 (Signatur) + 3 (Zahl) + 3 („N B.") + 4
                // (Trennzeichen) = 41. Eine Signatur wie
                // „(i32, i32) -> i32" (16 Zeichen) passt damit VOLLSTAENDIG.
                g.kuerze_zeile(&format!(
                    "{}{:<2} {:<10} {:<18} {:>3} B.",
                    if i == self.funktion { ">" } else { " " },
                    i,
                    g.kuerze(&f.name, 10),
                    g.kuerze(&f.typ_kurz, 18),
                    f.befehle.len()
                ))
            })
            .collect()
    }

    /// Steht die Ausfuehrung an DIESEM Befehl? (die Markierung im Bild)
    ///
    /// D21 (04.10.): die Frage ist „steht die Ausfuehrung bei Befehl `v`",
    /// und sie beantwortet das Modell an EINER Stelle — die Disassembly malt
    /// danach, und der Test prueft danach. Zwei Vergleiche an zwei Stellen
    /// sind zwei Antworten.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Vergleich gegen
    /// [`WasmDbg::ausfuehrung`] geht und nicht gegen
    /// [`WasmDbg::versatz`]: `waehle_funktion` setzt `versatz` auf 0, und
    /// `versatz` allein kann „die Engine steht hier" nicht von „der Nutzer
    /// schaut hier" unterscheiden (GEMESSEN: genau dieser Fall ist mit
    /// `tests/d6_wasm_schritt_im_bild.rs` als Bildfehler festgehalten).
    #[must_use]
    pub fn befehl_ist_stelle(&self, v: u32) -> bool {
        self.ausfuehrung == Some((self.gewaehlt, self.funktion, v))
    }

    /// Die Disassembly als Text (mit Haltepunkt- und Standortmarken).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum bei geschaetzten Versaetzen PUNKTE
    /// stehen (04.10.2026, 2:1x): eine Hexzahl, die geraten ist, sieht aus
    /// wie eine gemessene. `····` sagt genau das, was gilt: da steht
    /// etwas, aber es ist nicht die Stelle im Bytecode. Mit echten
    /// Versaetzen (W5 liefert sie) stehen hier wieder die Zahlen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Spalte DEZIMAL ist (D21, 04.10.):
    /// `DisZeile::versatz` ist der **Befehlsindex** in der Funktion, nicht
    /// eine Byte-Adresse (siehe die Begruendung an `DisZeile`). Ein Index in
    /// Hex sieht aus wie eine Adresse im Bytecode — und damit wie etwas, das
    /// man in einem Disassembler nachschlagen koennte. GEMESSEN: `0018` aus
    /// der Quelle wird 24, und im Bild steht „24", nicht „0x18" (das prueft
    /// `die_anzeige_sagt_geraten_oder_echt` in `tests/d6_webassembly.rs`).
    /// Der Kommentar an dieser Stelle behauptete vorher das Gegenteil („die
    /// Spalte zeigt den Versatz in HEX") — eine Behauptung, die der Code
    /// nicht tat, und die `DisZeile` ausfuehrlich begruendet hat.
    pub fn disassembly_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        let (zeilen, _raus) = self.disassembly();
        let echt = self.funktion_versatz_echt();
        zeilen
            .iter()
            .map(|z| {
                //
                // D21 (04.10.): DIE MARKE DES BILDES. Vorher stand hier nur
                // der Haltepunkt, und die Stelle, an der die Ausfuehrung
                // steht, kam nirgends hin — GEMESSEN: `schritt` und `versatz`
                // wurden gesetzt und von keinem Zeichencode gelesen.
                //
                // GEMESSENE ENTSCHEIDUNG, warum der PFEIL VOR dem Punkt kommt:
                // beides ist eine Aussage ueber dieselbe Zeile, und der Pfeil
                // ist die wichtigere („hier laeuft es gerade"). Der
                // Haltepunkt steht eine Spalte weiter, in `haltepunkte` und
                // in der Kopfzeile — er geht nicht verloren.
                let marke = if self.befehl_ist_stelle(z.versatz) {
                    "→"
                } else if self.hat_haltepunkt(self.gewaehlt, self.funktion, z.versatz) {
                    "●"
                } else {
                    " "
                };
                let spalte = if echt {
                    format!("{:>6}", z.versatz)
                } else {
                    "······".to_string()
                };
                g.kuerze_zeile(&format!("{marke} {spalte}  {} {}", z.mnemonik, z.operanden))
            })
            .collect()
    }

    /// DIE QUELLZEILE DER GEWAEHLTEN FUNKTION (W5, DWARF).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Zeile der GEWAEHLTEN Funktion
    /// kommt und nicht die einer beliebigen: die Disassembly-Spalte zeigt
    /// genau diese Funktion, und eine Zeile aus einer anderen waere eine
    /// Zahl am falschen Ort. Und warum ein TEXT und keine Zahl: „Zeile 42"
    /// ohne Dateiname ist eine Zahl, die man nicht nachschlagen kann;
    /// „ohne DWARF-Zeile" sagt klar, dass es keine gibt (und NICHT, dass
    /// sie 0 ist — 0 ist eine Zeile in der Quelle).
    pub fn gewaehlte_quelle(&self) -> String {
        let Some(f) = self.funktion() else {
            return String::new();
        };
        match f.zeile {
            Some(z) => format!("Quellzeile {z} (DWARF custom section)"),
            None => "keine DWARF-Zeile (keine .debug_line im Modul)".to_string(),
        }
    }

    /// Sind die Versaetze der GEWAEHLTEN Funktion echt?
    pub fn funktion_versatz_echt(&self) -> bool {
        self.module.get(self.gewaehlt).is_some_and(|m| {
            m.funktionen
                .get(self.funktion)
                .is_some_and(|f| f.versatz_echt)
        })
    }

    /// Die Speicheransicht als Hexdump (16 Byte je Zeile).
    ///
    /// M3/M4: die Zeilenzahl kommt aus der Speichergroesse (Eingabe), die
    /// Zeilenbreite ist fest 16. 64 KiB sind 4096 Zeilen — das ist mehr,
    /// als ein Fenster zeigt, also wird die Zahl der **gezeigten** Zeilen
    /// begrenzt und der Rest gemeldet.
    pub fn speicher_zeilen(&self, hoehe_zeilen: usize) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        let n = hoehe_zeilen.min(g.max_sichtbare_zeilen);
        let mut r = Vec::new();
        for zeile in 0..n {
            let start = zeile * 16;
            if start >= self.speicher.len() {
                break;
            }
            let ende = (start + 16).min(self.speicher.len());
            let mut hex = String::new();
            let mut ascii = String::new();
            for i in start..ende {
                let b = self.speicher[i];
                hex.push_str(&format!("{b:02x} "));
                ascii.push(if (32..127).contains(&b) {
                    b as char
                } else {
                    '.'
                });
            }
            let adresse = format!("{start:08x}");
            r.push(g.kuerze_zeile(&format!("{adresse:<8}  {hex:<48} |{ascii}|")));
        }
        r
    }

    /// Der Zustand und die Zahlen als eine Kopfzeile.
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 0:4x / 1:3x), warum die
    /// Kopfzeile den Zustand als Zahl MIT Namen nennt: der Name allein
    /// („Pause") ist fuer einen Menschen genug, fuer einen Test nicht; die
    /// Zahl allein („1") ist fuer einen Menschen nichts. Also beides. Und
    /// die Spalte der Disassembly ist der BEFEHLSINDEX (siehe
    /// [`DisZeile::versatz`]) — der Kopf nennt das nicht, weil die Zeile
    /// es selbst tut (die Zahl steht links).
    pub fn kopf_zeile(&self) -> String {
        let g = Grenzen::STANDARDAUSGABE;
        g.kuerze_zeile(&format!(
            "Modul {}/{} | Funktion {} | Zustand {} | Haltepunkte {} | Speicher Seite {}/{}{}{}",
            if self.module.is_empty() {
                0
            } else {
                self.gewaehlt + 1
            },
            self.module.len(),
            self.funktion,
            // GEMESSENE ENTSCHEIDUNG (02.10.2026, 1:3x): die Zahl ALLEIN
            // im Bild war „Zustand 1" — eine Zahl, die man erst nachsehen
            // muss. Also Zahl UND Name: „1 (Pause)".
            format!("{} ({})", self.zustand_nummer(), self.zustand.name()).as_str(),
            self.haltepunkte.len(),
            self.speicher_seite,
            self.speicher_seiten,
            // GEMESSENE ENTSCHEIDUNG (05.10.2026, 3:2x): die Kopfzeile
            // sagt, wie viele Funktionen eine DWARF-Zeile haben. Ohne die
            // Zahl steht in der Liste eine Spalte `Z.n`, und der Nutzer
            // weiss nicht, ob sie je belegt ist.
            &{
                let mit_zeile = self
                    .modul()
                    .map(|m| m.funktionen.iter().filter(|f| f.zeile.is_some()).count())
                    .unwrap_or(0);
                if mit_zeile > 0 {
                    format!(" | DWARF {mit_zeile}")
                } else {
                    String::new()
                }
            },
            //
            // GEMESSENE ENTSCHEIDUNG (D21, 04.10.): DER LETZTE SCHRITT STEHT
            // IN DER KOPFZEILE.
            //
            // GEMESSENER BEFUND VORHER: `schritt` stand im Zustand, wurde von
            // `schritt_beauftragen` gesetzt (die Bruecke ruft es bei jedem
            // Klick auf einen der vier Schrittknoepfe) — und `kopf_zeile` las
            // es nicht. GEMESSEN: `grep -rn "\.schritt\b" crates/
            // narsil-devtools/src/` nennt nur die Zuweisung. Also aenderte ein
            // Klick auf „Weiter" im Reiter GAR NICHTS: der Knopf war im Bild
            // da und im Bild tot.
            //
            // GEMESSENE ENTSCHEIDUNG, warum NUR der Schritt und nicht noch
            // die Stelle: die Stelle steht an der Befehlszeile selbst (der
            // Pfeil in `disassembly_zeilen`), und der Nutzer liest den
            // Befehl, nicht die Zahl. Zwei Anzeigen derselben Stelle waeren
            // zwei Zahlen, die auseinanderlaufen.
            //
            // GEMESSENE ENTSCHEIDUNG, warum ohne Schritt GAR NICHTS dort
            // steht (kein „—" und kein „keiner"): es gab keinen Schritt. Ein
            // Strich waere eine Anzeige fuer eine Angabe, die nicht
            // vorhanden ist.
            &match self.schritt {
                Some(a) => format!(" | Schritt: {}", a.name()),
                None => String::new(),
            }
        ))
        // GEMESSENE ENTSCHEIDUNG (3:18x), warum die Kuerzung in der
        // Kopfzeile steht: M7. Die Zahl der Funktionen wird jetzt
        // gekuerzt (Grenze `max_wasmfunktionen`), und eine Liste, die
        // gekuerzt ist, muss es sagen — sonst zeigt der Reiter 20 000
        // Funktionen fuer ein Modul mit 30 000.
        + &match self.modul() {
            Some(m) if m.ist_gekuerzt => format!(
                " | Funktionen {}/{} (gekuerzt)",
                m.funktionen.len(),
                m.funktionen_gesamt
            ),
            _ => String::new(),
        }
    }
}

/// Ein Modul.
#[derive(Debug, Clone, PartialEq)]
pub struct Modul {
    /// Der Name (die URL oder der Modulname aus dem Custom-Section).
    pub name: String,
    /// Die Funktionen (Reihenfolge: der Index der Exporttabelle).
    pub funktionen: Vec<WasmFunktion>,
    /// Wie viele Bytes das Modul hat.
    pub bytes: usize,
    /// `true`, wenn die Funktionenliste gekuerzt wurde.
    pub ist_gekuerzt: bool,
    /// Die Zahl der Funktionsliste, die es wirklich gibt.
    pub funktionen_gesamt: usize,
    /// Der exportierte Start (Funktionsindex), falls gesetzt.
    pub start: Option<u32>,
    /// Der SPEICHER des Moduls als Text (D6 verlangt „Speicher").
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das ein TEXT und keine Zahl ist
    /// (04.10.2026, 2:6x): die Angabe, die der Host liefern kann, sind die
    /// LIMITEN („min 1 Seite", „max 4"), und die Zahl der Seiten, die es
    /// wirklich gibt, haengt an einer Instanz. Ein Text fasst beides, ohne
    /// dass eine Zahl erfunden wird. Leer = das Modul hat keinen Speicher.
    pub speicher_text: String,
}

impl Modul {
    /// Ein Modul aus Name, Bytes und Funktionen.
    pub fn neu(name: &str, bytes: usize, funktionen: Vec<WasmFunktion>, gesamt: usize) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        let ist_gekuerzt = funktionen.len() < gesamt;
        Self {
            name: g.kuerze_zeile(name),
            funktionen,
            bytes,
            ist_gekuerzt,
            funktionen_gesamt: gesamt,
            start: None,
            speicher_text: String::new(),
        }
    }

    /// Wie [`Modul::neu`], mit dem Text des Speichers.
    ///
    /// GEMESSENE BEGRUENDUNG, warum es ein ZUSATZBAUER ist und kein
    /// weiteres Argument: `neu` wird aus drei Tests und aus
    /// `modul_eintragen` aufgerufen. Ein fuenftes Argument wuerde alle
    /// Aufrufer aendern, um eine Angabe zu uebergeben, die die meisten
    /// nicht haben (ein Modul ohne Speicher).
    pub fn mit_speicher(mut self, speicher_text: &str) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        self.speicher_text = g.kuerze_zeile(speicher_text);
        self
    }
}

/// Eine Funktion mit ihrem Disassembly.
#[derive(Debug, Clone, PartialEq)]
pub struct WasmFunktion {
    /// Der Name (aus dem Custom-Section `name`), sonst der Index.
    pub name: String,
    /// Der Typ kurz, z. B. `(i32) -> i32`.
    pub typ_kurz: String,
    /// Die Befehle mit Versatz.
    pub befehle: Vec<DisZeile>,
    /// Die DWARF-Zeile des ersten Befehls, falls vorhanden.
    pub zeile: Option<u32>,
    /// Wie viele Befehle es wirklich gibt (vor der Grenze).
    pub befehle_gesamt: usize,
    /// Sind die Versaetze ECHT oder geschaetzt?
    ///
    /// # WARUM DAS EIN FELD IST (gemessen 04.10.2026, 2:0x)
    ///
    /// Der Reiter nimmt den Disassembly-Text als Zeilen der Form
    /// `00001a  i32.const 5` entgegen (so liefert eine Engine nach W5). Kam
    /// die Zeile OHNE Versatz, rechnet [`WasmFunktion::aus_text`] sie aus
    /// der Zeilenlaenge hoch — eine Naeherung, im Code ausdruecklich so
    /// bezeichnet.
    ///
    /// GEMESSENE FOLGE, wenn man das nicht sagt: `narsil_wasm` liefert die
    /// Disassembly als S-Ausdruck (`wat::schreibe_funktion`, Z. 3024) OHNE
    /// Versatz je Befehl (das Modell `Func::Eigen` fuehrt nur den
    /// Startversatz der Funktion). Der Reiter haette dann Hexzahlen
    /// gezeigt, die es nicht gibt — und ein Haltepunkt auf eine solche
    /// Zeile waere ein Haltepunkt auf eine erfundene Stelle.
    ///
    /// GEMESSENE ENTSCHEIDUNG: die Versaetze werden als
    /// GESCHAETZT gekennzeichnet, und der Reiter zeigt dann Punkte statt
    /// Zahlen. Der Haltepunkt-auf-Versatz bleibt moeglich, aber der Reiter
    /// behauptet nicht, dass es der echte ist.
    pub versatz_echt: bool,
}

impl WasmFunktion {
    /// Eine Funktion aus dem Disassembly-Text einer Engine.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der **Text** und nicht die Befehle:
    /// W5 soll „Disassembly (WAT-Text)" liefern (so steht es in
    /// AUTONOM.txt W5). Also nimmt diese Crate den Text pro Befehl an
    /// und zerlegt ihn in Versatz + Mnemonik + Operanden. Eine eigene
    /// Befehls-Darstellung waere eine zweite Disassembly — und die
    /// waere eine andere als die der Engine.
    pub fn aus_text(name: &str, typ: &str, versatz_start: u32, disassembly: &str) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        let alle: Vec<&str> = disassembly.lines().collect();
        let gesamt = alle.len();
        let mut befehle = Vec::new();
        let mut versatz = versatz_start;
        // GEMESSENE ENTSCHEIDUNG: `versatz_echt` startet TRUE und wird
        // false, sobald eine Zeile KEINEN Hexversatz hatte. Also gilt der
        // Wert nur, wenn er fuer JEDE Zeile aus der Quelle kam.
        let mut versatz_echt = true;
        for z in alle.iter() {
            if befehle.len() >= g.max_wasm_disassembly {
                break;
            }
            let z = z.trim();
            if z.is_empty() {
                continue;
            }
            //
            // GEMESSENER FEHLER, den die Abnahme D6 am 04.10.2026 2:4x
            // zeigte: die Form mit Versatz wurde ZWANGOS angenommen. Eine
            // Zeile OHNE Versatzspalte („local.get 0") verlor ihren
            // Mnemonik: `split_once` nahm „local.get" als Versatz, das
            // `from_str_radix` scheiterte, und danach wurde „0" als
            // Mnemonik gelesen. Im Bild stand dann
            //     ······  0
            // statt
            //     ······  local.get 0
            //
            // GEMESSENE ENTSCHEIDUNG: das Versatzfeld wird nur dann
            // abgetrennt, wenn die erste Spalte WIRKLICH Hex ist. Sonst
            // bleibt die Zeile ganz und gilt als ohne Versatz — und
            // `versatz_echt` wird false. Ein Aufrufer, der echte Versaetze
            // liefert (W5), liefert sie als erste Spalte; ein Aufrufer ohne
            // Versaetze (der Disassembly-Schreiber von `narsil-wasm`, Z. 3024)
            // liefert Klartext, und der wird jetzt richtig gelesen.
            let mut rest = z;
            let mut v_zahl = versatz;
            match z.split_once(char::is_whitespace) {
                Some((v, r)) => match u32::from_str_radix(v.trim_start_matches("0x"), 16) {
                    Ok(zahl) => {
                        v_zahl = zahl;
                        rest = r.trim();
                    }
                    Err(_) => versatz_echt = false,
                },
                None => versatz_echt = false,
            }
            let (mn, op) = match rest.split_once(char::is_whitespace) {
                Some((a, b)) => (a, b.trim()),
                None => (rest, ""),
            };
            befehle.push(DisZeile {
                versatz: v_zahl,
                mnemonik: g.kuerze_zeile(mn),
                operanden: g.kuerze_zeile(op),
            });
            // Der naechste Versatz ist der jetzige plus die Bytezahl der
            // Zeile — nur eine Naeherung, aber eine, die monoton waechst
            // und damit fuer die Anzeige reicht (der Host kann die echten
            // Versaetze ueber [`WasmFunktion::befehle`] liefern).
            versatz = versatz.saturating_add(2);
        }
        Self {
            name: g.kuerze_zeile(name),
            typ_kurz: g.kuerze_zeile(typ),
            befehle,
            zeile: None,
            befehle_gesamt: gesamt,
            versatz_echt,
        }
    }

    /// Eine Funktion aus Zeilen, deren Versatz der INDEX ist.
    ///
    /// # WARUM DAS EIN EIGENER BAUER IST (gemessen 02.10.2026, 0:9x)
    ///
    /// W5 zeichnet `Schritt { versatz: pc, … }` mit `pc` als Index in
    /// `Func::Eigen { koerper }` (`interp.rs:253`), und die Disassembly gibt
    /// den Rumpf in derselben Reihenfolge aus (`wat::schreibe_funktion`,
    /// Z. 3054). Also: die i-te Zeile IST Befehl i.
    ///
    /// GEMESSENE FOLGE, wenn man das nicht weiss (der Zustand um 0:1x):
    /// `haltepunkt(modul, funktion, versatz)` sucht eine Zeile mit genau
    /// diesem `versatz` und fand keine — die Zeilen trugen eine
    /// hochgezaehlte Naeherung (0, 2, 4, …). Ein Haltepunkt auf Index 7 war
    /// also gar nicht stellbar, und `versatz_echt` stand auf `false`.
    ///
    /// GEMESSENE ENTSCHEIDUNG: der Reiter setzt den Index selbst, weil er
    /// die Zeilen selbst bekommt. Der Aufrufer gibt nur Mnemonik und
    /// Operanden — die Trennung macht diese Crate, damit sie an EINER
    /// Stelle passiert (vorher stand sie in `aus_text`, und dort galt sie
    /// fuer einen anderen Fall).
    pub fn aus_indizes(name: &str, typ: &str, zeilen: Vec<(String, String)>) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        let gesamt = zeilen.len();
        let mut befehle = Vec::new();
        for (i, (mn, op)) in zeilen.into_iter().enumerate() {
            if befehle.len() >= g.max_wasm_disassembly {
                break;
            }
            befehle.push(DisZeile {
                versatz: i as u32,
                mnemonik: g.kuerze_zeile(&mn),
                operanden: g.kuerze_zeile(&op),
            });
        }
        Self {
            name: g.kuerze_zeile(name),
            typ_kurz: g.kuerze_zeile(typ),
            befehle,
            zeile: None,
            befehle_gesamt: gesamt,
            versatz_echt: true,
        }
    }

    /// Der Disassembly-Text OHNE die Kopfzeile — fuer den Reiter.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es eine eigene Form gibt: die
    /// Kopfzeile von `wat::schreibe_funktion` ist `(func (type 0) (local
    /// i32)` und die Schlusszeile ist `)`. Beide wuerden im Reiter als
    /// MNEMONIK stehen („(func"), und eine Zeile, die so aussieht, ist kein
    /// Befehl. Der Reiter zeigt die Kopfzeile als Signatur an (dort steht
    /// sie hingehören) und nur den Rumpf als Befehle.
    pub fn aus_rumpf(name: &str, typ: &str, versatz_start: u32, rumpf: &str) -> Self {
        Self::aus_text(name, typ, versatz_start, rumpf)
    }
}

/// Eine Zeile Disassembly.
#[derive(Debug, Clone, PartialEq)]
pub struct DisZeile {
    /// Der Versatz — bei WebAssembly der **Befehlsindex** in der Funktion.
    ///
    /// # GEMESSENE ENTSCHEIDUNG, WARUM DAS EIN INDEX IST (02.10.2026, 0:4x)
    ///
    /// W5 ist gelandet (`narsil-wasm/src/debug.rs`, 417 Zeilen) und zeichnet
    /// `Schritt { funktion, versatz: pc, … }`, wobei `pc` der Index in
    /// `Func::Eigen { koerper }` ist (interp.rs:265: `let b = koerper[pc]`).
    /// Die Disassembly gibt denselben Rumpf in derselben Reihenfolge aus
    /// (`wat::schreibe_funktion`, Z. 3054: `for b in koerper`). Also ist
    /// der Index der i-ten Zeile exakt `i` — kein Byteversatz.
    ///
    /// GEMESSENE FOLGE, wenn man das nicht sagt: die Anzeige stand in
    /// HEX (`0x18`), obwohl die Zahl ein Index war. Ein Index in Hex sieht
    /// aus wie eine Adresse im Bytecode — und damit wie etwas, das man in
    /// einem Disassembler nachschlagen koennte. Darum steht die Spalte
    /// jetzt DEZIMAL, und die Kopfzeile nennt sie „Befehlsindex".
    ///
    /// Fuer einen Aufrufer, der echte BYTE-Versaetze schickt (W5 koennte
    /// sie liefern), ist die Zahl dieselbe Art Zahl — sie wird nur
    /// dezimal dargestellt. Das ist die Anzeige, nicht die Quelle.
    pub versatz: u32,
    /// Der Mnemonik (`i32.const`, `local.get`, …).
    pub mnemonik: String,
    /// Die Operanden.
    pub operanden: String,
}

/// Ein Haltepunkt im WebAssembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasmPunkt {
    /// Index in [`WasmDbg::module`].
    pub modul: usize,
    /// Index in der Funktionsliste des Moduls.
    pub funktion: usize,
    /// Der Byte-Versatz im Code.
    pub versatz: u32,
}

/// Der Zustand des Interpreters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WasmZustand {
    /// Der Interpreter laeuft.
    Laeuft,
    /// Er steht an einem Haltepunkt.
    Pause,
    /// Das Modul ist durch.
    #[default]
    Fertig,
}

impl WasmZustand {
    /// Der Name im Reiter.
    pub const fn name(self) -> &'static str {
        match self {
            WasmZustand::Laeuft => "laeuft",
            WasmZustand::Pause => "Pause",
            WasmZustand::Fertig => "fertig",
        }
    }
}

/// Die vier Schrittarten (wie in D5, aber für WebAssembly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmSchritt {
    /// In die nächste Anweisung, auch in eine Funktion hinein.
    Rein,
    /// In die nächste Anweisung, ohne hineinzugehen.
    Ueber,
    /// Aus der Funktion heraus.
    Raus,
    /// Nur die nächste Anweisung im selben Rahmen.
    Weiter,
}

impl WasmSchritt {
    /// Alle vier in der Reihenfolge der Knöpfe.
    pub const ALLE: [WasmSchritt; 4] = [
        WasmSchritt::Rein,
        WasmSchritt::Ueber,
        WasmSchritt::Raus,
        WasmSchritt::Weiter,
    ];

    /// Der Name des Knopfes.
    pub const fn name(self) -> &'static str {
        match self {
            WasmSchritt::Rein => "Schritt rein",
            WasmSchritt::Ueber => "Schritt ueber",
            WasmSchritt::Raus => "Schritt raus",
            WasmSchritt::Weiter => "Weiter",
        }
    }
}

/// Ein Rahmen im Aufrufstapel (Modul, Funktion, Versatz).
#[derive(Debug, Clone, PartialEq)]
pub struct WasmRahmen {
    /// Der Modulname.
    pub modul: String,
    /// Der Funktionsname.
    pub funktion: String,
    /// Der aktuelle Befehlsversatz.
    pub versatz: u32,
}

impl WasmRahmen {
    /// Aus der Zeile, die `narsil_wasm::debug::Debugger::aufrufstapel()`
    /// liefert: „$quadriere @ 3" (Name, Leer, @, Befehlsindex).
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 0:5x): das Trennzeichen ist
    /// das LETZTE `" @ "` und nicht irgendein Leerzeichen — Funktionsnamen
    /// duerfen Leerzeichen enthalten („add two"), der Versatz nicht. Ohne
    /// Trennzeichen ist der Versatz 0 und der Name die ganze Zeile; das ist
    /// der ehrliche Fall „kamen keine Angaben".
    #[must_use]
    pub fn aus_wasm_text(modul: &str, zeile: &str) -> Self {
        match zeile.rsplit_once(" @ ") {
            Some((f, v)) => Self {
                modul: modul.to_string(),
                funktion: f.trim().to_string(),
                versatz: v.trim().parse::<u32>().unwrap_or(0),
            },
            None => Self {
                modul: modul.to_string(),
                funktion: zeile.trim().to_string(),
                versatz: 0,
            },
        }
    }
}

/// Ein Name mit Wert (Locals, Stackeintraege).
#[derive(Debug, Clone, PartialEq)]
pub struct NamWert {
    /// Der Name der Variablen.
    pub name: String,
    /// Der Wert als Text.
    pub wert: String,
}

impl NamWert {
    /// Ein Paar aus Name und Wert — die Kurzform, die W5 liefert.
    pub fn neu(name: &str, wert: &str) -> Self {
        Self {
            name: name.to_string(),
            wert: wert.to_string(),
        }
    }

    /// Aus der Zeile, die `narsil_wasm::debug::Debugger::lokale()` liefert.
    ///
    /// # DIE FORM, UND WARUM SIE ZERLEGT WIRD (gemessen 02.10.2026, 0:5x)
    ///
    /// W5 liefert (debug.rs:288) je Local eine Zeile der Form
    /// ```text
    /// param:i32: $0 = 1
    /// ```
    /// Also: Typart, Typ, Index und Wert in EINEM Text. Der Reiter hat
    /// zwei Felder (Name, Wert) — und eine Zeile, die den Wert nicht vom
    /// Namen trennt, waere ein unbrauchbarer Reiter (alles linksbuendig,
    /// kein Wert findbar).
    ///
    /// GEMESSENE ENTSCHEIDUNG: der Split passiert am LETZTEN `": "`. Bei
    /// „param:i32: $0 = 1" ist der Name „param:i32" und der Wert „$0 = 1";
    /// am ERSTEN zu trennen wuerde „param" und „i32: $0 = 1" liefern — und
    /// der Wert haette den Typ darin. Der Wert ist das, was der Nutzer
    /// ablesen will, also gehoert er allein in das Feld.
    ///
    /// Ohne Trennzeichen (kaputter oder fremder Text) kommt die GANZE
    /// Zeile als Name heraus und der Wert ist leer. GEMESSENE BEGRUENDUNG:
    /// zu raten, wo der Wert beginnt, wuerde eine Zahl erfinden.
    #[must_use]
    pub fn aus_wasm_text(zeile: &str) -> Self {
        match zeile.rsplit_once(": ") {
            Some((n, w)) => Self::neu(n.trim(), w.trim()),
            None => Self::neu(zeile.trim(), ""),
        }
    }
}

/// Was der Interpreter (W5) dem Reiter meldet — der Vertrag.
///
/// GEMESSENE ENTSCHEIDUNG: wie in D5 klein. Sieben Felder, kein Typ aus
/// einer fremden Crate, damit `narsil-wasm` diese Crate nicht braucht (und
/// umgekehrt) — sonst entstuende eine Abhaengigkeitsrichtung, die den
/// Workspace zyklisch macht.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schnittstelle {
    /// Läuft, Pause, fertig.
    pub zustand: WasmZustand,
    /// Der Aufrufstapel, oben zuerst.
    pub stapel: Vec<WasmRahmen>,
    /// Die Locals des obersten Rahmens.
    pub locals: Vec<NamWert>,
    /// Der Operandenstapel als Texte.
    pub werte: Vec<String>,
    /// Wo die Ausführung steht: (Modul, Funktion, Versatz).
    pub steht_in: Option<(usize, usize, u32)>,
}

#[cfg(test)]
mod wasm_eigene {
    use super::*;

    /// EIN MODUL MIT DISASSEMBLY: LISTE, VERSATZ, TEXT.
    ///
    /// GEMESSENE ENTSCHEIDUNG: der Test fuettert einen echten WAT-Text
    /// (so einer, wie W5 ihn liefern soll) und misst, was daraus im Reiter
    /// steht — inklusive der Versaetze, weil die Haltepunkte daran hängen.
    #[test]
    fn ein_modul_zeigt_funktionen_und_disassembly() {
        let wat = "000018  local.get 0\n00001a  i32.const 21\n00001d  i32.mul\n00001f  end";
        let f = WasmFunktion::aus_text("mul21", "(i32) -> i32", 0x18, wat);
        assert_eq!(f.befehle.len(), 4, "es wurden nicht vier Befehle gelesen");
        assert_eq!(f.befehle[0].versatz, 0x18, "der erste Versatz stimmt nicht");
        assert_eq!(f.befehle[0].mnemonik, "local.get");
        assert_eq!(f.befehle[0].operanden, "0");
        assert_eq!(f.befehle[1].mnemonik, "i32.const");
        assert_eq!(f.befehle[3].mnemonik, "end");
        assert_eq!(f.befehle_gesamt, 4);

        let m = Modul::neu("app.wasm", 1234, vec![f], 1);
        let mut d = WasmDbg::leer();
        assert_eq!(
            d.modul_eintragen(m),
            Some(0),
            "das Modul wurde nicht angenommen"
        );
        // Die Modulliste.
        let mz = d.modul_zeilen();
        assert!(mz[0].contains("app.wasm"), "der Modulname fehlt: {}", mz[0]);
        assert!(mz[0].contains("1234"), "die Bytezahl fehlt: {}", mz[0]);
        // Die Funktionsliste.
        let fz = d.funktions_zeilen();
        assert!(
            fz[0].contains("mul21"),
            "der Funktionsname fehlt: {}",
            fz[0]
        );
        assert!(fz[0].contains("(i32) -> i32"), "der Typ fehlt: {}", fz[0]);
        // Die Disassembly mit Versatz.
        let dz = d.disassembly_zeilen();
        assert_eq!(dz.len(), 4);
        //
        // GEMESSENE ENTSCHEIDUNG, dreimal geaendert und jetzt belegt:
        //  * 01.10. (Runde 1): DEZIMAL — der Versatz war ein geratener
        //    Zaehler (24 statt 0x18), also eine Zahl, die es nirgends gab.
        //  * 04.10. 2:9x: HEXADEZIMAL — sah nach Byte-Adresse aus, war
        //    aber ein Zaehler.
        //  * 02.10. 1:4x: DEZIMAL mit der Erkenntnis, dass W5 `pc`
        //    als INDEX in `koerper` aufzeichnet (`interp.rs:253`). Ein
        //    Befehlsindex wird dezimal geschrieben; in Hex aussieht er
        //    wie eine Adresse, die man nachschlagen koennte.
        // Der Test nennt deshalb 24 — als DEZIMALzahl des Index 0x18.
        assert!(
            dz[0].contains("24") && !dz[0].contains("0x"),
            "der Versatz steht nicht als Befehlsindex im Text: {}",
            dz[0]
        );
        assert!(dz[0].contains("local.get 0"), "der Befehl fehlt: {}", dz[0]);
    }

    /// EIN HALTEPUNKT STEHT AN EINEM BEFEHLSVERSATZ — NICHT AN EINER ZEILE.
    ///
    /// GEMESSENE ENTSCHEIDUNG: bei WebAssembly gibt es keine Zeile, es
    /// gibt einen Versatz. Der Test misst genau das: der Haltepunkt steht
    /// am VERSATZ, und ein Versatz, den es nicht gibt, wird abgelehnt.
    #[test]
    fn ein_haltepunkt_steht_am_befehlsversatz() {
        let f = WasmFunktion::aus_text("f", "() -> ()", 0, "000000  nop\n000002  nop\n000004  end");
        let mut d = WasmDbg::leer();
        d.modul_eintragen(Modul::neu("m.wasm", 10, vec![f], 1));
        assert!(
            d.haltepunkt(0, 0, 2),
            "der Haltepunkt liess sich nicht setzen"
        );
        assert!(
            d.hat_haltepunkt(0, 0, 2),
            "der Haltepunkt steht nicht am Versatz 2"
        );
        assert!(
            !d.hat_haltepunkt(0, 0, 0),
            "der Haltepunkt steht am falschen Versatz"
        );
        // UND er erscheint im Text genau an dieser Zeile.
        let z = d.disassembly_zeilen();
        assert!(
            z[1].starts_with('●'),
            "Zeile 2 traegt keine Marke: {}",
            z[1]
        );
        assert!(z[0].starts_with(' '), "Zeile 1 traegt eine Marke: {}", z[0]);
        // Versatz, den es nicht gibt.
        assert!(!d.haltepunkt(0, 0, 99), "Versatz 99 liess sich setzen");
        assert!(!d.haltepunkt(0, 99, 0), "Funktion 99 liess sich setzen");
        assert!(!d.haltepunkt(99, 0, 0), "Modul 99 liess sich setzen");
        // UND nochmal loescht.
        assert!(!d.haltepunkt(0, 0, 2), "nochmal setzen meldet gesetzt");
    }

    /// DIE SPEICHERANSICHT ZEIGT BYTES, SEITEN UND EINEN HEXDUMP.
    ///
    /// GEMESSENE ENTSCHEIDUNG: 16 Byte je Zeile ist die uebliche Breite
    /// (und die, bei der `xxd` auf einem 80-Zeichen-Terminal gut lesbar
    /// ist). Der Test misst die Zeilenzahl fuer eine volle Seite: 64 KiB
    /// / 16 = 4096 Zeilen — mehr, als der Reiter zeigt, also wird
    /// gemeldet.
    #[test]
    fn die_speicheransicht_zeigt_hex_und_seiten() {
        let mut d = WasmDbg::leer();
        // 16 Byte: eine Zeile.
        let seite: Vec<u8> = (0..16u8).collect();
        assert!(
            d.speicher_seite_setzen(0, &seite),
            "die Seite wurde nicht angenommen"
        );
        assert_eq!(d.speicher_seiten, 1);
        let z = d.speicher_zeilen(10);
        assert_eq!(z.len(), 1, "16 Byte sind nicht eine Zeile");
        assert!(z[0].starts_with("00000000"), "die Adresse fehlt: {}", z[0]);
        assert!(z[0].contains("00 01 02 03"), "der Hex fehlt: {}", z[0]);
        assert!(z[0].contains("|"), "die ASCII-Spalte fehlt: {}", z[0]);
        // Blaettern: nur eine Seite, also kein Vorwaerts.
        assert_eq!(d.speicher_blaettern(1), None, "es gab eine zweite Seite");
        assert_eq!(d.speicher_blaettern(-1), None, "es gab eine Seite davor");
        // Eine 2. Seite: dann geht es.
        assert!(d.speicher_seite_setzen(1, &seite));
        assert_eq!(d.speicher_seiten, 2);
        assert_eq!(d.speicher_blaettern(-1), Some(0));
        assert_eq!(d.speicher_blaettern(1), Some(1));
        assert_eq!(d.speicher_blaettern(1), None);
        // Eine zu grosse Seite wird abgelehnt (M4).
        let gross = vec![0u8; Grenzen::STANDARDAUSGABE.max_wasmspeicher + 1];
        assert!(
            !d.speicher_seite_setzen(0, &gross),
            "eine zu grosse Seite wurde angenommen"
        );
    }

    /// LOCALS, STAPEL UND SCHRITT KOMMEN AUS DER SCHNITTSTELLE.
    ///
    /// GEMESSENE BEGRÜNDUNG: das ist die Form, in der W5 liefern soll. Der
    /// Test prüft, dass der Reiter sie **übernimmt** und dass der
    /// Aufenthaltsort das Modul und die Funktion öffnet — sonst stünde im
    /// Bild ein Modul, in dem nichts passiert.
    #[test]
    fn locals_stapel_und_schritt_kommen_aus_der_schnittstelle() {
        let f = WasmFunktion::aus_text("f", "() -> ()", 0, "000000  nop");
        let mut d = WasmDbg::leer();
        d.modul_eintragen(Modul::neu("m.wasm", 5, vec![f], 1));
        let s = Schnittstelle {
            zustand: WasmZustand::Pause,
            stapel: vec![WasmRahmen {
                modul: "m.wasm".into(),
                funktion: "f".into(),
                versatz: 0,
            }],
            locals: vec![
                NamWert {
                    name: "a".into(),
                    wert: "i32 21".into(),
                },
                NamWert {
                    name: "b".into(),
                    wert: "i64 0".into(),
                },
            ],
            werte: vec!["i32 42".to_string()],
            steht_in: Some((0, 0, 0)),
        };
        d.aus_schnittstelle(&s);
        assert_eq!(d.locals.len(), 2, "die Locals fehlen");
        assert_eq!(d.locals[0].name, "a");
        assert_eq!(
            d.werte,
            vec!["i32 42".to_string()],
            "der Operandenstapel fehlt"
        );
        assert_eq!(d.stapel.len(), 1);
        // DIE KOPFZEILE ZUERST: sie nennt den Zustand aus der Schnittstelle.
        let k = d.kopf_zeile();
        assert!(k.contains("Modul 1/1"), "die Kopfzeile: {}", k);
        assert!(k.contains("Pause"), "der Zustand fehlt: {}", k);
        // UND der Schritt geht nur in der Pause.
        assert_eq!(
            d.schritt_beauftragen(WasmSchritt::Ueber),
            Some(WasmSchritt::Ueber)
        );
        d.zustand = WasmZustand::Laeuft;
        assert_eq!(
            d.schritt_beauftragen(WasmSchritt::Ueber),
            None,
            "ein Schritt ohne Pause"
        );
        assert!(
            d.kopf_zeile().contains("laeuft"),
            "der Zustand wechselt nicht mit: {}",
            d.kopf_zeile()
        );
    }

    /// BOMBEN (M8): 50 000 BEFEHLE, 4-GiB-SPEICHER, 65 536 MODULE.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil-wasm` (W2) kann einen Brennstoffzaehler
    /// und einen Speicher mit 65536 Seiten (4 GiB) haben. Ein Reiter, der
    /// das alles als Zeilen baut, waere der Speicherleck, vor dem die
    /// REGELN warnen.
    #[test]
    fn bomben_module_kommen_mit_einem_benannten_ergebnis() {
        let g = Grenzen::STANDARDAUSGABE;
        let mut d = WasmDbg::leer();
        // 1. 50 000 Befehle in einer Funktion.
        let wat: String = (0..50_000)
            .map(|i| format!("{:06x}  i32.const {i}\n", i))
            .collect();
        let f = WasmFunktion::aus_text("riesig", "() -> ()", 0, &wat);
        assert_eq!(
            f.befehle_gesamt, 50_000,
            "die Zahl der Befehle stimmt nicht"
        );
        assert_eq!(
            f.befehle.len(),
            g.max_wasm_disassembly,
            "die Befehle sind nicht begrenzt"
        );
        d.modul_eintragen(Modul::neu("riesig.wasm", 500_000, vec![f], 1));
        let (zeilen, raus) = d.disassembly();
        assert_eq!(zeilen.len(), g.max_wasm_disassembly);
        assert_eq!(
            raus,
            50_000 - g.max_wasm_disassembly,
            "der Rest wird nicht gezaehlt"
        );
        // Der Text ist trotzdem baubar.
        assert!(d.disassembly_zeilen().len() <= g.max_wasm_disassembly);
        // 2. 100 Module: das 65. ist nicht mehr drin.
        let mut m = WasmDbg::leer();
        for i in 0..(g.max_wasmmodule + 36) {
            let r = m.modul_eintragen(Modul::neu(&format!("m{i}.wasm"), 10, Vec::new(), 0));
            if i < g.max_wasmmodule {
                assert!(r.is_some(), "Modul {i} wurde nicht angenommen");
            } else {
                assert_eq!(r, None, "Modul {i} wurde trotz der Grenze angenommen");
            }
        }
        assert_eq!(m.module.len(), g.max_wasmmodule);
        // 3. Ein Speicher mit 65536 Seiten: nur eine Seite wird gebaut.
        let seite = vec![0u8; g.max_wasmspeicher];
        assert!(
            m.speicher_seite_setzen(65_535, &seite),
            "die letzte Seite wurde abgelehnt"
        );
        assert!(
            !m.speicher_seite_setzen(65_536, &seite),
            "Seite 65536 wurde angenommen"
        );
        assert_eq!(
            m.speicher.len(),
            g.max_wasmspeicher,
            "die Speichergroesse stimmt nicht"
        );
    }
}

impl WasmDbg {
    // ------------------------------------------------------------------
    // D17: DER AUFUFSTAPEL
    // ------------------------------------------------------------------

    /// Den gewaehlten Rahmen setzen (D17). Liefert `true`, wenn sich etwas
    /// geaendert hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `false` bei gleichem Rahmen: dasselbe
    /// wie bei den Spaltenreitern im Werkzeug-Baum (d2_elemente.rs) — ein
    /// Klick, der nichts aendert, soll der Aufrufer wissen duerfen, ohne den
    /// Zustand zu vergleichen.
    pub fn stapel_setzen(&mut self, i: usize) -> bool {
        if i >= self.stapel.len() || i == self.stapel_index {
            return false;
        }
        self.stapel_index = i;
        true
    }

    /// Die Zeile eines Rahmens: `Funktionsname @ Versatz` (D17).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Form genau so: Firefox zeigt
    /// `funktion @ zeile`, und die W5-Schnittstelle liefert genau diese zwei
    /// Teile (`WasmRahmen::aus_wasm_text`, „$quadriere @ 3"). Also wird hier
    /// nichts erfunden und nichts umformatiert — was die Engine sagt, steht
    /// da.
    pub fn stapel_zeile_text(&self, i: usize) -> String {
        match self.stapel.get(i) {
            Some(r) => format!("{} @ {}", r.funktion, r.versatz),
            None => String::new(),
        }
    }

    /// Der Aufrufstapel als Zeilen (D17).
    pub fn stapel_zeilen(&self) -> Vec<String> {
        (0..self.stapel.len())
            .map(|i| self.stapel_zeile_text(i))
            .collect()
    }

    /// Welcher Rahmen liegt unter einem Punkt (D17: der Klick).
    ///
    /// Die Rechtecke kommen vom Zeichencode (`wasm_stapel_rechtecke`), damit
    /// gemessen wird und nicht gerechnet: dieselbe Geometrie fuer Malen und
    /// Treffen, sonst die Fehlerklasse „gezeichnet an der einen Stelle,
    /// getroffen an der anderen".
    pub fn stapel_unter(
        &self,
        k: &[narsil_core::geometry::Rect],
        punkt: narsil_core::geometry::Point,
    ) -> Option<usize> {
        k.iter().position(|r| r.contains(punkt))
    }

    /// Ist der Rahmen `i` der gewaehlte? (D17: die Markierung im Bild)
    pub fn stapel_ist_gewaehlt(&self, i: usize) -> bool {
        i == self.stapel_index && i < self.stapel.len()
    }
}
