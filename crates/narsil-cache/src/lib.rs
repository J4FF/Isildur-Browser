//! narsil-cache — HTTP-Cache fuer den Browser nach RFC 9111.
//!
//! Ein Cache beantwortet eine Frage, bevor das Netz gefragt wird: **ist das
//! schon da, und wenn ja, darf ich es benutzen?** Alles andere ist
//! Detailarbeit.
//!
//! ```text
//! nachschlagen(anfrage) -> Frisch(antwort)      -> zeigen, fertig
//!                      -> Veraltet(validatoren) -> bedingte Anfrage, dann 304 oder 200
//!                      -> Fehlt                  -> normale Anfrage
//! ablegen(anfrage, antwort)                   -> speichern, wenn erlaubt
//! ```
//!
//! **Eigene Minimal-Typen.** Der Cache arbeitet mit [`Anfrage`] und
//! [`Antwort`] statt mit den Typen aus `narsil-net`. Das hat zwei Gruende:
//! (1) ein Cache ist eine Policy-Engine ueber (Anfrage, Antwort)-Paare und
//! braucht davon nur sechs Felder; (2) eine Abhaengigkeit von `narsil-net`
//! wuerde den Cache mit dem Netz-Layer verkoppeln — und `narsil-net` haengt
//! selbst von `narsil-compress`, das in Welle 6 gerade umgebaut wird.
//! `narsil-net`/`narsil` koennen die Typen direkt nutzen: die Umwandlung ist
//! eine Funktion mit sechs Zeilen (siehe [`Anfrage::aus_narsil_net`]).
//!
//! **Zwei Speicher, eine Ordnung.** Der Speicher-Cache (RAM) ist schnell und
//! fluechtig, der Platte-Cache (`~/.cache/narsil`) ist langsam und ueberlebt
//! einen Neustart. Beide nutzen dieselbe LRU-Reihenfolge und dieselben
//! Regeln — zwei Implementationen davon waeren zwei Stellen, an denen sie
//! sich unterscheiden koennen.
//!
//! **Grenzen (M4/M6):** der Speicher ist auf 64 MiB begrenzt, die Platte
//! ebenso, und beide zaehlen zusaetzlich die Eintraege (10.000). Ein
//! einzelner Eintrag, der die ganze Grenze sprengt, wird nicht gespeichert.
//!
//! **Was nicht gespeichert wird** (RFC 9111, Abschnitt 3): POST-Antworten,
//! Statuscodes, die nicht standardmaessig cachebar sind, `no-store`,
//! `private` in einem geteilten Cache, und `Authorization` ohne
//! ausdrueckliche Erlaubnis.
//!
//! **Vary** (RFC 9111, Abschnitt 4.1): die Request-Header, die `Vary`
//! nennt, werden gespeichert und bei jeder Anfrage verglichen. `Vary: *`
//! speichert alle Header — die Antwort passt dann nur zu einer identischen
//! Anfrage.
//!
//! **Bewusst nicht ausgewertet:** `stale-while-revalidate` und
//! `stale-if-error`. Dieser Cache bedient eine veraltete Antwort nie ohne
//! erneute Pruefung — strenger als das RFC verlangt, aber nie falsch.

use std::collections::HashMap;
use std::path::Path;

use crate::speicher::Speicher;

mod platte;
mod regeln;
mod sha256;
mod speicher;

pub use platte::{Platte, Zaehler};
pub use regeln::{grund, schreibe_datum};
// `Eintrag` stand vorher nur in `Platte::lesen` als Rueckgabetyp und
// war dabei privat — das Beispiel `examples/cache_abnahme.rs` konnte
// es deshalb nicht benutzen, und `cargo test` (das Beispiele baut)
// wurde rot. Der Typ gehoert zur oeffentlichen Flaeche, also exportiert.
pub use speicher::Eintrag;

/// Die HTTP-Methode einer Anfrage.
///
/// Nur GET wird gecacht. HEAD hat keinen Koerper, POST ist keine
/// Cache-Methode (RFC 9111, Abschnitt 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Methode {
    Get,
    Head,
    Post,
}

/// Eine Anfrage, so wie der Cache sie braucht.
///
/// Sechs Felder, nichts mehr. Der Schluessel des Caches ist
/// `http://{host}:{port}{pfad}` — der Port steht mit, weil `example.com:8080`
/// und `example.com` verschiedene Ressourcen sind.
#[derive(Debug, Clone, PartialEq)]
pub struct Anfrage {
    pub methode: Methode,
    pub host: String,
    pub port: u16,
    /// Pfad inklusive Query (`/seite?a=1`).
    pub pfad: String,
    /// Header, Namen kleingeschrieben.
    pub headers: HashMap<String, String>,
}

impl Anfrage {
    /// Eine GET-Anfrage.
    pub fn get(host: &str, port: u16, pfad: &str) -> Self {
        Anfrage {
            methode: Methode::Get,
            host: host.to_string(),
            port,
            pfad: pfad.to_string(),
            headers: HashMap::new(),
        }
    }

    /// Header setzen (Kette).
    pub fn mit_header(mut self, name: &str, wert: &str) -> Self {
        self.headers
            .insert(name.to_ascii_lowercase(), wert.to_string());
        self
    }
}

/// Eine Antwort, so wie der Cache sie braucht.
///
/// Der Koerper liegt SO vor, wie er kam — `Content-Encoding` steht noch
/// drin. Ein Cache, der den Koerper veraendert, muss dem Client eine
/// Antwort geben, die er nicht entpacken kann.
#[derive(Debug, Clone, PartialEq)]
pub struct Antwort {
    /// Statuscode (200, 301, 404, ...).
    pub status: u16,
    /// Alle Antwort-Header, unveraendert.
    pub headers: HashMap<String, String>,
    /// Der Koerper, so wie er auf der Leitung war.
    pub koerper: Vec<u8>,
}

impl Antwort {
    /// Eine Antwort mit Status und Koerper.
    pub fn neu(status: u16, koerper: Vec<u8>) -> Self {
        Antwort {
            status,
            headers: HashMap::new(),
            koerper,
        }
    }

    /// Header setzen (Kette).
    pub fn mit_header(mut self, name: &str, wert: &str) -> Self {
        self.headers
            .insert(name.to_ascii_lowercase(), wert.to_string());
        self
    }
}

/// Das Ergebnis einer Cache-Anfrage.
#[derive(Debug, Clone, PartialEq)]
pub enum Fund {
    /// Frisch: darf ohne Netz benutzt werden.
    Frisch(Antwort),
    /// Veraltet: eine bedingte Anfrage mit diesen Validatoren ist noetig.
    ///
    /// `etag` wird zu `If-None-Match`, `last_modified` zu
    /// `If-Modified-Since`. Eine 304-Antwort aktualisiert den Eintrag
    /// (RFC 9111, Abschnitt 4.3.4), eine 200 ersetzt ihn.
    Veraltet {
        etag: Option<String>,
        last_modified: Option<String>,
    },
    /// Nichts Passendes im Cache.
    Fehlt,
}

/// Der HTTP-Cache.
pub struct Cache {
    speicher: Speicher,
    platte: Option<Platte>,
    /// `true` = geteilter Cache (Proxy). `false` = Nutzer-Cache (Browser).
    ///
    /// Der Unterschied ist `private` und `s-maxage`: ein geteilter Cache
    /// darf `private` nicht speichern und folgt `s-maxage` statt `max-age`.
    geteilt: bool,
    /// Zaehler fuer die Abnahme (C-PLATTE: "zweiter Besuch laedt aus dem
    /// Cache - Zaehler und Ladezeit vorher/nachher"). `u64` statt `usize`,
    /// damit ein langer Lauf nicht ueberlaeuft; nichts davon wird
    /// zurueckgesetzt.
    treffer: u64,
    misser: u64,
    /// Woher kam der letzte Treffer: 0 = Speicher, 1 = Platte.
    zuletzt_aus_platte: bool,
    /// added 04.10.2026, Rolle NETZTEMPO: **DIE `Vary`-NAMENSSAETZE JE URL.**
    ///
    /// **WARUM ES SIE GEBEN MUSS:** beim **Ablegen** weiss der Cache, welche
    /// Header der Server genannt hat (aus der Antwort). Beim **Nachschlagen**
    /// weiss er es **nicht** — die Anfrage enthaelt keine `Vary`. Ohne diese
    /// Liste koennte der Lookup den Varianten-Schluessel nicht bilden, und
    /// `Vary` waere wieder eine Pruefung ohne Wirkung.
    ///
    /// **SIE WIRD BEGRENZT**, weil sie sonst wuerde wie der Cache selbst: ein
    /// Eintrag je **Namen** ist winzig (die Namen stehen schon in der
    /// Antwort), aber eine URL ohne jeden `Vary` darf **nichts** eintragen.
    vary_namen: HashMap<String, Vec<Vec<String>>>,
}

impl Cache {
    /// Neu mit den Standardwerten: 64 MiB Speicher, Platte unter
    /// `~/.cache/narsil` (oder `$XDG_CACHE_HOME/narsil`).
    pub fn neu() -> Self {
        let speicher = Speicher::neu(speicher::MAX_SPEICHER_BYTES);
        let platte = Platte::neu(&standard_pfad(), speicher::MAX_SPEICHER_BYTES).ok();
        Cache {
            speicher,
            platte,
            geteilt: false,
            treffer: 0,
            misser: 0,
            zuletzt_aus_platte: false,
            vary_namen: HashMap::new(),
        }
    }

    /// Nur Speicher, keine Platte — mit eigener Byte-Obergrenze.
    ///
    /// Fuer Tests und fuer den Fall, dass der Cache nichts auf die Platte
    /// schreiben darf (M9: keine Testdaten unter `/tmp`).
    pub fn nur_speicher(grenze: u64) -> Self {
        Cache {
            speicher: Speicher::neu(grenze),
            platte: None,
            geteilt: false,
            treffer: 0,
            misser: 0,
            zuletzt_aus_platte: false,
            vary_namen: HashMap::new(),
        }
    }

    /// Speicher und Platte unter einem eigenen Verzeichnis — mit eigener
    /// Byte-Obergrenze.
    ///
    /// Fuer Tests: das Verzeichnis liegt wo der Aufrufer will, nicht unter
    /// `/tmp` (tmpfs = RAM, und ein Test, der RAM frisst, ist kein Test
    /// auf Begrenzung).
    pub fn mit_pfad(pfad: &Path, grenze: u64) -> std::io::Result<Self> {
        Ok(Cache {
            speicher: Speicher::neu(grenze),
            platte: Some(Platte::neu(pfad, grenze)?),
            geteilt: false,
            treffer: 0,
            misser: 0,
            zuletzt_aus_platte: false,
            vary_namen: HashMap::new(),
        })
    }

    /// Als geteilen Cache markieren (Proxy statt Browser).
    ///
    /// Aendert `private` (nicht speichern) und `s-maxage` (gilt statt
    /// `max-age`).
    pub fn als_geteilt(mut self) -> Self {
        self.geteilt = true;
        self
    }

    /// Eine Anfrage nachschlagen.
    ///
    /// Reihenfolge: Speicher zuerst (schnell), dann Platte (langsam, aber
    /// ueberlebt einen Neustart). Ein Fund von der Platte wandert in den
    /// Speicher, damit der naechste Treffer schnell ist.
    ///
    /// `&mut self`, weil ein Treffer die LRU-Reihenfolge aendert — ein
    /// Cache, der die Ordnung nicht aendert, verdraengt irgendwann genau
    /// das, was gerade benutzt wurde.
    pub fn nachschlagen(&mut self, anfrage: &Anfrage) -> Fund {
        let schluessel = schluessel(anfrage);
        //
        // added 04.10.2026, Rolle NETZTEMPO. **DIE KANDIDATEN.** Erst der
        // einfache Schluessel (Eintraege **ohne** `Vary`), dann je gespeichertem
        // `Vary`-Namensatz einer aus den Werten **dieser** Anfrage. GEMESSEN, warum
        // die Reihenfolge so ist: `Vary: *` speichert **alle** Header, und ohne
        // `Vary` ist der einfache Schluessel der einzige richtige.
        //
        // **UND DER GRUND, DASS DAS EIN LOOP IST und nicht ein Key-Sprung:**
        // `bewerte` entscheidet am Ende ueber `Vary` (`vary_trifft`). Ein
        // Kandidat kann also **daliegen** und trotzdem **nicht** passen — dann
        // muss der **naechste** versucht werden. Wer beim ersten Fehlschlag
        // aufgibt, verliert genau die Fassung, die er sucht.
        let namenssaetze = self
            .vary_namen
            .get(&schluessel)
            .cloned()
            .unwrap_or_default();
        let kandidaten = kandidaten(&schluessel, &namenssaetze, anfrage);

        // 1. Speicher
        for kandidat in &kandidaten {
            if let Some(eintrag) = self.speicher.lesen(kandidat) {
                if let Some(fund) = self.bewerte(&eintrag, anfrage) {
                    self.treffer += 1;
                    self.zuletzt_aus_platte = false;
                    return fund;
                }
            }
        }

        // 2. Platte
        //
        // added 04.10.2026, Rolle NETZTEMPO: **DER FUND WIRD VOR GESPEICHERT,
        // DER SPEICHER-SHORT NACH DEM PLATTEN-BLOCK.** Der Uebersetzer meldete
        // `cannot borrow *self as immutable because it is also borrowed as
        // mutable` — berechtigt: `platte` haelt `&mut self.platte`, und
        // `self.speicher.schreiben` will `&self.speicher`. Der Ausweg ist
        // **kein** `unsafe` und **kein** `drop(platte)` im Block, sondern das
        // Zwischenspeichern des Treffers: es gibt nur **einen** Ort, an dem in
        // den Speicher geschrieben wird.
        //
        // added 04.10.2026, Rolle NETZTEMPO: **LESEN, DANN PRUEFEN.** Der
        // Uebersetzer meldete `cannot borrow *self as immutable because it is
        // also borrowed as mutable` — berechtigt, und die **Ursache ist eine
        // echte Falle fuer jeden**: `if let Some(platte) = &mut self.platte`
        // haelt die Leihabe, **solange** `platte` im Block benutzt wird. Im
        // alten code war sie nach `platte.lesen(...)` **tot** (NLL), also
        // erlaubte `self.bewerte(...)`. In der Variante-Schleife ist `platte`
        // aber **noch fuer den naechsten Kandidaten** da — und damit kollidiert
        // es.
        //
        // **DER WEG DARUM:** erst **alle** Kandidaten aus der Platte **lesen**
        // (deren Leihabe endet mit dem Block), **dann** bewerten. Das kostet
        // einen kleinen Vektor und ist die einzige Form ohne `unsafe`.
        let von_platte: Vec<(String, Eintrag)> = match &mut self.platte {
            Some(p) => kandidaten
                .iter()
                .filter_map(|k| p.lesen(k).map(|e| (k.clone(), e)))
                .collect(),
            None => Vec::new(),
        };
        for (kandidat, mut eintrag) in von_platte {
            if let Some(fund) = self.bewerte(&eintrag, anfrage) {
                // **UNTER DEMSELBEN NAMEN zurueck**, unter dem er gefunden
                // wurde — der **Varianten**-Schluessel. Mit dem einfachen
                // Schluessel waere die zweite Fassung unter dem falschen Namen
                // gelandet und der naechste Aufruf haette sie nicht gefunden.
                eintrag.schluessel = kandidat.clone();
                self.speicher.schreiben(kandidat, eintrag);
                self.treffer += 1;
                self.zuletzt_aus_platte = true;
                return fund;
            }
        }

        self.misser += 1;
        self.zuletzt_aus_platte = false;
        Fund::Fehlt
    }

    /// Zaehlerstand fuer die Abnahme: (Treffer, Misser).
    ///
    /// Ein Treffer ist ein `Frisch` oder `Veraltet`; ein Misser ist
    /// `Fehlt` — auch dann, wenn ein Eintrag da war, aber `Vary` nicht
    /// passt.
    pub fn statistik(&self) -> (u64, u64) {
        (self.treffer, self.misser)
    }

    /// Kam der zuletzt gelieferte Treffer aus dem Speicher (false)
    /// oder von der Platte (true)?
    ///
    /// Nach einem Neustart ist der erste Zugriff immer `true` — das ist
    /// der Nachweis, dass die Platte wirklich etwas liefert.
    pub fn letzter_aus_platte(&self) -> bool {
        self.zuletzt_aus_platte
    }

    /// Zaehler zuruecksetzen (nur fuer Tests und Auswertungen).
    pub fn statistik_zuruecksetzen(&mut self) {
        self.treffer = 0;
        self.misser = 0;
    }

    /// Eine Antwort ablegen.
    ///
    /// Eine 304 aktualisiert den vorhandenen Eintrag (RFC 9111, 4.3.4),
    /// alles andere wird gespeichert, wenn `ist_cachebar` es erlaubt.
    pub fn ablegen(&mut self, anfrage: &Anfrage, antwort: &Antwort) {
        let schluessel = schluessel(anfrage);

        // 304: der Server hat gesagt "nimm deine Kopie". Die Header der 304
        // aktualisieren den gespeicherten Eintrag, der Koerper bleibt.
        if antwort.status == 304 {
            // added 04.10.2026, Rolle NETZTEMPO: **AUCH HIER DIE KANDIDATEN.**
            // Sonst wuerde ein `304` auf eine `br`-Anfrage den `identity`-
            // Eintrag aktualisieren — und das ist die **zweite** Hälfte desselben
            // Fehlers: nicht nur das Speichern, auch das **Aktualisieren** muss
            // die Fassung finden, zu der die bedingte Anfrage gehoert.
            let namenssaetze = self
                .vary_namen
                .get(&schluessel)
                .cloned()
                .unwrap_or_default();
            let kandidaten = kandidaten(&schluessel, &namenssaetze, anfrage);
            self.aktualisiere_304(&kandidaten, &antwort.headers);
            return;
        }

        if !regeln::ist_cachebar(anfrage, &antwort.headers, antwort.status, self.geteilt) {
            return;
        }

        let eintrag = Eintrag::neu(anfrage, antwort, schluessel.clone());
        //
        // added 04.10.2026, Rolle NETZTEMPO: **DER VARIANTEN-SCHLUESSEL.** Ohne
        // das hat der Cache **einen** Slot je URL und die zweite Fassung
        // (`br` nach `identity`) **ueberschreibt** die erste — GEMESSEN als
        // 4 Server-Anfragen fuer 2 Kodierungen, mit der Chance auf die
        // falsche Antwort in der Zwischenzeit.
        let varianten = varianten_schluessel(&schluessel, &eintrag.vary);
        // **UND DIE NAMEN MERKEN**, damit der naechste Lookup den Schluessel
        // bilden kann. Ohne `Vary` wird **nichts** eingetragen: dann ist der
        // einfache Schluessel der richtige, und die Liste waere Ballast.
        if !eintrag.vary.is_empty() {
            let namen: Vec<String> = {
                let mut n: Vec<String> = eintrag.vary.iter().map(|(k, _)| k.clone()).collect();
                n.sort();
                n.dedup();
                n
            };
            let saetze = self.vary_namen.entry(schluessel.clone()).or_default();
            if !saetze.contains(&namen) {
                saetze.push(namen);
            }
        }
        self.speicher.schreiben(varianten.clone(), eintrag.clone());
        if let Some(platte) = &mut self.platte {
            platte.schreiben(&varianten, &eintrag);
        }
    }

    /// Eintrag bewerten: Vary, dann Frischheit.
    fn bewerte(&self, eintrag: &Eintrag, anfrage: &Anfrage) -> Option<Fund> {
        // Vary muss passen — sonst gehoert die Antwort zu einer anderen
        // Anfrage, und sie zu benutzen waere eine Luege.
        if !regeln::vary_trifft(&eintrag.vary, anfrage) {
            return None;
        }
        let jetzt = std::time::SystemTime::now();
        let alter = regeln::aktuelle_alter(&eintrag.headers, eintrag.empfangen, jetzt);
        let lebensdauer = regeln::frischheit(
            &eintrag.headers,
            eintrag.status,
            eintrag.empfangen,
            jetzt,
            self.geteilt,
        );
        let frisch = match lebensdauer {
            Some(dauer) => alter < dauer,
            // Keine Frische gesagt und keine Heuristik: veraltet. Ein
            // Cache, der hier "frisch" sagt, erfindet eine Erlaubnis, die
            // der Server nicht gegeben hat.
            None => false,
        };
        if frisch {
            Some(Fund::Frisch(antwort_aus_eintrag(eintrag)))
        } else {
            Some(Fund::Veraltet {
                etag: eintrag.headers.get("etag").cloned(),
                last_modified: eintrag.headers.get("last-modified").cloned(),
            })
        }
    }

    /// 304 in Speicher und Platte einarbeiten.
    fn aktualisiere_304(&mut self, schluessel: &[String], headers: &HashMap<String, String>) {
        // added 04.10.2026, Rolle NETZTEMPO: **UEBER DIE KANDIDATEN**, und in
        // derselben Reihenfolge wie der Lookup — sonst wuerde der 304 einen
        // Eintrag erneuern, den der Aufrufer nie gefragt hat.
        for kandidat in schluessel {
            if self.speicher.aktualisieren(kandidat, headers) {
                return;
            }
        }
        //
        // added 04.10.2026, Rolle NETZTEMPO: **DIE PLatte, und dieselbe
        // Kandidatenschleife.** GEMESSEN, warum das nicht „nur die Platte"
        // ist: ein `304`, der nach einem Neustart aus dem **Speicher** nichts
        // findet, muss den Platteneintrag **derselben** Fassung erneuern —
        // sonst wird der 304 einem Eintrag einer **anderen** `Vary`-Fassung
        // gutgeschrieben, und deren Alter wird zur Frische der falschen.
        //
        // added 04.10.2026, Rolle NETZTEMPO: **WIEDER: LESEN, DANN SCHREIBEN**
        // (Grund und Fehlerbild siehe `nachschlagen`). Und der Speicher-Short
        // kommt **nach** dem Platten-Block, weil `platte` `&mut self.platte`
        // haelt.
        let mut zu_erneuern: Option<(String, Eintrag)> = None;
        if let Some(platte) = &mut self.platte {
            for kandidat in schluessel {
                if let Some(mut eintrag) = platte.lesen(kandidat) {
                    for (k, v) in headers {
                        eintrag.headers.insert(k.clone(), v.clone());
                    }
                    eintrag.empfangen = std::time::SystemTime::now();
                    eintrag.schluessel = kandidat.clone();
                    platte.schreiben(kandidat, &eintrag);
                    zu_erneuern = Some((kandidat.clone(), eintrag));
                    break;
                }
            }
        }
        if let Some((kandidat, eintrag)) = zu_erneuern {
            self.speicher.schreiben(kandidat, eintrag);
        }
    }

    /// Die gespeicherte Antwort, **ohne** auf Frische zu achten.
    ///
    /// added 30.09.2026, Welle 7 / N5. WOFUER, und es ist genau ein
    /// Fall: eine bedingte Anfrage hat `304 Not Modified` bekommen.
    /// RFC 9111 Abschnitt 4.3.4 sagt dazu woertlich, dass der Client
    /// in diesem Fall den gespeicherten **Inhalt** benutzt — und
    /// `nachschlagen` liefert fuer einen gerade aktualisierten, aber
    /// ohne Frische-Angabe gespeicherten Eintrag `Veraltet`. Ohne
    /// diese Methode muesste `narsil-net` den 304 als Antwort melden und
    /// dem Benutzer eine leere Seite zeigen.
    ///
    /// DER WEG: Speicher, dann Platte, ohne `bewerte` — deshalb
    /// `Vary` hier NICHT geprueft wird. Das ist Absicht: ein 304 vom
    /// Server heisst „du hast die richtige Variante", und der
    /// `Vary`-Abgleich ist beim Abruf erfolgt. Wer den doch pruefen
    /// will, nimmt `nachschlagen`.
    pub fn gespeichert(&mut self, anfrage: &Anfrage) -> Option<Antwort> {
        let schluessel = schluessel(anfrage);
        if let Some(eintrag) = self.speicher.lesen(&schluessel) {
            return Some(antwort_aus_eintrag(&eintrag));
        }
        // M3: `as_mut` statt `as_ref`, weil `Platte::lesen` den Index
        // in einer `HashMap` **veraendert** — der Lesevorgang zaehlt
        // den Zugriff und schreibt den Eintrag ins Speicher-Background.
        // Mit `as_ref` waere das ein Compilerfehler; mit `unwrap_or` auf
        // einem `Option<&Platte>` waere es zur Laufzeit ein
        // `BorrowMutError`, und der Fehlertext waere unbrauchbar.
        let platte = self.platte.as_mut()?;
        let eintrag = platte.lesen(&schluessel)?;
        Some(antwort_aus_eintrag(&eintrag))
    }

    /// Alles loeschen — Speicher und Platte.
    pub fn leeren(&mut self) {
        self.speicher.leeren();
        if let Some(platte) = &mut self.platte {
            platte.leeren();
        }
    }

    /// Belegte Byte im Speicher.
    pub fn bytes(&self) -> u64 {
        self.speicher.bytes()
    }

    /// Zahl der Eintraege im Speicher.
    pub fn eintraege(&self) -> usize {
        self.speicher.eintraege()
    }

    /// Belegte Byte auf der Platte (0 ohne Platte).
    pub fn datei_bytes(&self) -> u64 {
        self.platte.as_ref().map(|p| p.bytes()).unwrap_or(0)
    }

    /// Zahl der Eintraege auf der Platte (0 ohne Platte).
    pub fn datei_eintraege(&self) -> usize {
        self.platte.as_ref().map(|p| p.eintraege()).unwrap_or(0)
    }

    /// Die Zaehler der PLATTE: Treffer, Fehl, Kaputt, Geschrieben,
    /// Abgelehnt, Verdraengt.
    ///
    /// Das ist die zweite Sicht neben `statistik()`: dort stehen die
    /// Treffer des GESAMTEN Caches (Speicher und Platte zusammen), hier
    /// nur das, was auf der Platte passiert ist. Fuer die Abnahme
    /// ("zweiter Besuch laedt aus dem Cache - Zaehler") sind beide
    /// noetig: `statistik()` sagt, ob ueberhaupt etwas kam,
    /// `zaehler()` sagt, ob es von der Platte kam.
    ///
    /// Ohne Platte sind alle Felder null.
    pub fn zaehler(&self) -> Zaehler {
        self.platte
            .as_ref()
            .map(|p| p.zaehler())
            .unwrap_or_default()
    }
}

/// Den Schluessel (die URL) aus der Anfrage bauen.
fn schluessel(anfrage: &Anfrage) -> String {
    format!("http://{}:{}{}", anfrage.host, anfrage.port, anfrage.pfad)
}

/// added 04.10.2026, Rolle NETZTEMPO (auf Weisung der Leitung). **DER
/// VARIANTEN-SCHLUESSEL**, und er ist die **ganze** Reparatur von `Vary`.
///
/// **DER BEFUND, und er ist nicht der, den ich zuerst gemeldet habe:** ich
/// hatte geschrieben, `vary_speichern` werde „nie aufgerufen". Das ist
/// **falsch** — `speicher.rs:66` ruft es auf, und `vary_trifft` wird in
/// `bewerte` benutzt. **Meine Diagnose war ein Lesefehler** (ich hatte nur in
/// `lib.rs` und `regeln.rs` gesucht). Die **echte** Ursache ist eine andere:
///
/// GEMESSEN (`narsil-net/tests/cache_nachweis.rs`, `Vary: Accept-Encoding`):
/// **4** Server-Anfragen fuer **zwei** Kodierungen. Der Cache hat **einen
/// Slot je URL** (`schluessel` = `http://host:portpfad`), also **überschreibt**
/// die zweite Variante die erste — und nach jedem UeberSchreiben passt der
/// Vary-Vergleich fuer die *andere* Kodierung wieder **nicht**. Jeder Abruf
/// geht zum Server; und das Schlimmere ist der **Zwischenschritt**: solange
/// beide Kodierungen dasselbe Feld nutzen, **kann** die falsche Antwort
/// ausgeliefert werden.
///
/// **DIE REGEL, und sie ist die des Auftrags:** der Schluessel traegt die
/// **Werte der `Vary`-Kopfzeilen der Anfrage**. Damit bekommen
/// `br` und `identity` **zwei** Eintraege und keiner ueberschreibt den
/// anderen. `Vary: *` bedeutet „alle Header der Anfrage" — das macht
/// `regeln::vary_speichern` bereits so, und damit passt die Regel fuer `*`
/// automatisch.
fn varianten_schluessel(schluessel: &str, vary: &[(String, String)]) -> String {
    if vary.is_empty() {
        return schluessel.to_string();
    }
    // **SORTIERT**, und das ist nicht Kosmetik: die Reihenfolge, in der
    // `vary_speichern` die Paare liefert, folgt der **Antwort**; dieselbe
    // Anfrage kann sie in anderer Reihenfolge liefern, und dann waeren zwei
    // Schluessel fuer **dieselbe** Variante. Das waere ein Cache, der sich
    // selbst leert.
    let mut paare: Vec<String> = vary
        .iter()
        .map(|(n, w)| format!("{}={}", n.to_ascii_lowercase(), w))
        .collect();
    paare.sort();
    paare.dedup();
    format!("{schluessel}|{}", paare.join("&"))
}

/// **Die Kandidaten fuer einen Lookup**: erst der einfache Schluessel (Eintraege
/// **ohne** `Vary`), dann je gespeichertem Namenssatz ein Schluessel aus den
/// Werten **dieser** Anfrage. Ohne die Namenssaetze koennte der Lookup den
/// Schluessel nicht bilden — er weiss ja nicht, **welche** Header der Server
/// beim Speichern wichtig fand.
fn kandidaten(
    schluessel: &str,
    namenssaetze: &[Vec<String>],
    anfrage: &Anfrage,
) -> Vec<String> {
    let mut raus = vec![schluessel.to_string()];
    for namen in namenssaetze {
        let paare: Vec<(String, String)> = namen
            .iter()
            .filter_map(|n| {
                anfrage
                    .headers
                    .get(n)
                    .map(|w| (n.to_ascii_lowercase(), w.clone()))
            })
            .collect();
        if paare.len() == namen.len() {
            raus.push(varianten_schluessel(schluessel, &paare));
        }
    }
    raus
}

/// Den Standard-Pfad des Platten-Caches bestimmen.
///
/// `$XDG_CACHE_HOME/narsil`, sonst `~/.cache/narsil`. Ein Fehler (kein
/// Home-Verzeichnis) wird nicht behandelt — `neu()` greift dann auf den
/// Speicher-Cache zurueck.
/// Der Standardpfad des Platten-Caches.
///
/// added 30.09.2026, Welle 7 / N5 (Verdrahtung in `narsil-net`). Die
/// Funktion war vorher `privat`, und `narsil-net` brauchte sie fuer zwei
/// Dinge: fuer die Meldung im Fehlerfall („der Cache liegt hier“) und
/// fuer Tests, die in ein **eigenes** Verzeichnis schreiben wollen,
/// ohne den Cache des Benutzers anzufassen.
///
/// DER WEG: `$XDG_CACHE_HOME/narsil`, sonst `$HOME/.cache/narsil`. Siehe
/// die Funktion fuer die Herleitung.
pub fn standard_pfad() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let basis = std::env::var_os("LOCALAPPDATA").filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").filter(|p| !p.is_empty())
                .map(|p| std::path::PathBuf::from(p).join("AppData/Local")))
            .unwrap_or_else(std::env::temp_dir);
        return basis.join("narsil/cache");
    }
    #[cfg(not(windows))]
    {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return std::path::PathBuf::from(xdg).join("narsil");
        }
    }
    std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".cache").join("narsil"))
        .unwrap_or_else(|| std::path::PathBuf::from(".cache/narsil"))
    }
}

/// Aus einem gespeicherten Eintrag eine [`Antwort`] bauen.
fn antwort_aus_eintrag(eintrag: &Eintrag) -> Antwort {
    Antwort {
        status: eintrag.status,
        headers: eintrag.headers.clone(),
        koerper: eintrag.body.clone(),
    }
}

/// added 04.10.2026, Rolle NETZTEMPO. **DIE NACHTESTS ZU `Vary`** — und sie
/// stehen hier, weil der Fehler **in dieser Crate** war, auch wenn er erst am
/// Client sichtbar wurde.
#[cfg(test)]
mod vary_tests {
    use super::*;
    use std::collections::HashMap;

    fn anfrage(pfad: &str, enc: &str) -> Anfrage {
        let mut headers = HashMap::new();
        headers.insert("accept-encoding".to_string(), enc.to_string());
        Anfrage {
            methode: Methode::Get,
            host: "beispiel.invalid".to_string(),
            port: 80,
            pfad: pfad.to_string(),
            headers,
        }
    }

    fn antwort(kopfzeilen: &[(&str, &str)], koerper: &str) -> Antwort {
        let mut headers = HashMap::new();
        for (k, v) in kopfzeilen {
            headers.insert((*k).to_string(), (*v).to_string());
        }
        Antwort {
            status: 200,
            headers,
            koerper: koerper.as_bytes().to_vec(),
        }
    }

    /// **DIE BEHAUPTUNG:** zwei Fassungen derselben Ressource (**nach
    /// `Accept-Encoding` unterschieden**) liegen **beide** im Cache, und jede
    /// kommt **fuer ihre** Anfrage zurueck.
    ///
    /// **GEMESSEN, woran es vorher lag** (`narsil-net/tests/cache_nachweis.rs`):
    /// **4** Server-Anfragen fuer **2** Kodierungen. Der Cache hatte **einen
    /// Slot je URL**, also ueberschrieb die zweite Fassung die erste.
    #[test]
    fn zwei_fassungen_einer_url_beiben_beide_im_cache() {
        let mut c = Cache::nur_speicher(1024 * 1024);
        let a = anfrage("/seite", "br");
        let b = anfrage("/seite", "identity");
        c.ablegen(
            &a,
            &antwort(
                &[("cache-control", "max-age=600"), ("vary", "Accept-Encoding")],
                "KOERPER-BR",
            ),
        );
        c.ablegen(
            &b,
            &antwort(
                &[("cache-control", "max-age=600"), ("vary", "Accept-Encoding")],
                "KOERPER-IDENTITY",
            ),
        );
        // **UND JEDE KOMMT ZURUECK** — das ist der ganze Nutzen.
        match c.nachschlagen(&a) {
            Fund::Frisch(t) => assert_eq!(
                String::from_utf8_lossy(&t.koerper),
                "KOERPER-BR",
                "die br-Fassung fehlt im Cache"
            ),
            anderes => panic!("br-Fassung nicht gefunden: {anderes:?}"),
        }
        match c.nachschlagen(&b) {
            Fund::Frisch(t) => assert_eq!(
                String::from_utf8_lossy(&t.koerper),
                "KOERPER-IDENTITY",
                "die identity-Fassung fehlt im Cache — die br hat sie ueberschrieben"
            ),
            anderes => panic!("identity-Fassung nicht gefunden: {anderes:?}"),
        }
    }

    /// **OHNE `Vary` bleibt es EIN Eintrag** — und der muss **derselbe**
    /// Schluessel sein wie vorher, sonst waere die Aenderung eine
    /// Speicherluecke.
    #[test]
    fn ohne_vary_bleibt_es_ein_eintrag() {
        let mut c = Cache::nur_speicher(1024 * 1024);
        let a = anfrage("/schlicht", "br");
        c.ablegen(
            &a,
            &antwort(&[("cache-control", "max-age=600")], "NUR-EINS"),
        );
        assert_eq!(c.speicher.eintraege(), 1, "es muss **ein** Eintrag sein");
        assert!(
            matches!(c.nachschlagen(&a), Fund::Frisch(_)),
            "der Eintrag muss **frisch** sein"
        );
    }

    /// **`Vary: *` HEISST: NIE WIEDERVERWENDEN** — und das ist der Auftrag der
    /// Leitung im Wortlaut. `regeln::vary_speichern` speichert dann **alle**
    /// Anfrage-Kopfzeilen; ein zweiter Abruf mit **einem** anderen Kopf
    /// passt nicht, und `bewerte` sagt `Fehlt`.
    #[test]
    fn vary_stern_wird_nie_wiederverwendet() {
        let mut c = Cache::nur_speicher(1024 * 1024);
        let mut erste = anfrage("/stern", "br");
        erste.headers.insert("cookie".to_string(), "a=1".to_string());
        let mut zweite = anfrage("/stern", "br");
        zweite.headers.insert("cookie".to_string(), "a=2".to_string());
        c.ablegen(
            &erste,
            &antwort(
                &[("cache-control", "max-age=600"), ("vary", "*")],
                "NUR-FUER-A1",
            ),
        );
        assert!(
            matches!(c.nachschlagen(&erste), Fund::Frisch(_)),
            "die **identische** Anfrage muss treffen"
        );
        assert!(
            matches!(c.nachschlagen(&zweite), Fund::Fehlt),
            "eine andere Anfrage wurde wiederverwendet — bei `Vary: *` ist das \\
             verboten"
        );
    }
}
