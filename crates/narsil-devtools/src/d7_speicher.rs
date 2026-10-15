//! D7 — SPEICHER: Cookies, localStorage, Cache ansehen und löschen.
//!
//! # WAS D7 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D7 Speicher: Cookies, localStorage, Cache ansehen/loeschen.
//!
//! # WAS HEUTE GEHT — gemessen am 01.10.2026
//!
//! | D7 braucht | gibt es? | wo |
//! |---|---|---|
//! | Cookies ansehen | **ja** | `narsil_browser::CookieSpeicher::cookies_fuer_anfrage` |
//! | Cookies löschen | **ja** | `CookieSpeicher::entfernen` / `leeren` |
//! | localStorage | **nein** | gibt es im Workspace nicht (0 Treffer) |
//! | Cache ansehen/leeren | **teilweise** | `crates/narsil-cache` (Rolle CACHE) |
//!
//! # DIE ZWEI ENTSCHEIDUNGEN, DIE DIESE CRATE TRÄGT
//!
//! **1. Cookies kommen aus dem echten Speicher des Browsers.**
//! `CookieSpeicher` hat keine Methode „alle Cookies", sondern
//! `cookies_fuer_anfrage(domain, pfad, sicher, jetzt) -> Vec<&Cookie>`. Das
//! ist genau die Liste, die ein DevTools für die **aktuelle Seite** zeigt —
//! Chrome zeigt die Cookies der Herkunft, nicht die der ganzen Maschine. Also
//! wird diese Methode benutzt, und der Aufrufer liefert die Seite.
//!
//! **2. localStorage ist ein Modell, keine erfundene Wahrheit.**
//! Es gibt keine Speicherung im Workspace. Also definiert diese Crate die
//! **Schlüssel-Wert-Liste**, in die der Host (Rolle JS, mit dem
//! `localStorage`-Binding) einfuegt. Der Reiter zeigt, was reingekommen ist;
//! er erfindet nichts. Der Wunsch steht in `team/WUNSCH_JS.txt`.

use crate::grenzen::Grenzen;
use narsil_browser::{Cookie, CookieFehler, CookieSpeicher};

/// Der Zustand des Speicher-Reiters.
#[derive(Debug, Clone, PartialEq)]
pub struct Speicher {
    /// Die Cookies der aktuellen Seite.
    pub cookies: Vec<CookieZeile>,
    /// D18: DIE sessionStorage-Eintraege.
    ///
    /// GEMESSENE ENTSCHEIDUNG (Auftrag der Leitung 04.10.: „Cookies/
    /// localStorage/sessionStorage lesen+bearbeiten"): sessionStorage ist
    /// ein ZWILLING von localStorage — gleiche API, gleiche Zeilenform,
    /// anderes Leben (bis zum Schliessen des Tabs). Ein eigener Bereich
    /// kostet eine Variante und eine Liste; ein Vermischen in
    /// `localStorage` wuerde zwei Sachen an einer Stelle zeigen, die der
    /// Nutzer nicht trennen kann — und localStorage ist der, der bleibt.
    pub session: Vec<LokalZeile>,
    /// Die localStorage-Eintraege.
    pub lokal: Vec<LokalZeile>,
    /// Die Cache-Eintraege (vom Host gefuellt).
    pub cache: Vec<CacheZeile>,
    /// Welcher Bereich angezeigt wird.
    pub reiter: Bereich,
    /// Die gewaehlte Cookie-Zeile (fuer die Detailansicht).
    pub gewaehlt: Option<String>,
    /// Ein Text oben im Reiter.
    pub hinweis: String,
    /// Die Zahl der Dinge, die NICHT gelöscht werden konnten.
    pub fehlgeschlagen: u32,
    /// Woher die Zahlen kommen (der Reiter behauptet das nicht selbst).
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x), und sie ist die wichtigste
    /// in diesem Modul: der Browser hat KEINEN Cookie-Speicher. Gemessen:
    /// `narsil_browser::CookieSpeicher` gibt es als Typ, aber `Browser` (in
    /// `Sitzung::browser`) besitzt keinen, und `Sitzung` erzeugt keinen. Die
    /// Cookies, die der Reiter zeigt, kommen deshalb aus den `Set-Cookie`-
    /// KOPFZEILEN der Antworten — das ist, was diese Sitzung BEKOMMEN hat,
    /// nicht, was der Browser gespeichert haelt (er haelt nichts).
    ///
    /// Wer beides gleich nennt, verkauft dem Nutzer eine Zahl, die er nicht
    /// hat. Deshalb steht der Satz oben im Reiter.
    pub herkunft: String,
    /// Die kurze Form fuer die Kopfzeile (der lange Satz steht im Hinweis).
    ///
    /// GEMESSEN, warum es zwei Formen gibt: `kopf_zeile` hat 24 px und keine
    /// Zeilenumbrueche. Ein Satz, der dort laenger ist als die Zeile, laeuft
    /// unter den naechsten Knopf (gemessen im Bild: „...(Wunsch|Alle Cookies
    /// loeschen").
    pub herkunft_kurz: String,
}

impl Speicher {
    /// Leer — vor der ersten Seite.
    pub fn neu() -> Self {
        Self {
            cookies: Vec::new(),
            session: Vec::new(),
            lokal: Vec::new(),
            cache: Vec::new(),
            reiter: Bereich::Cookies,
            gewaehlt: None,
            hinweis: String::new(),
            fehlgeschlagen: 0,
            herkunft: String::new(),
            herkunft_kurz: String::new(),
        }
    }

    /// Den Speicher-Reiter setzen — der Weg aus der Werkzeugleiste (D12).
    ///
    /// GEMESSENER BEFUND (03.10. 18:0x): die drei Reiter-Knoepfe der Leiste
    /// wurden gezeichnet, der Hover Ging, `an()` sagte „an" — und ein Klick
    /// tat nichts, weil `werkzeug::wirke` keinen Arm fuer `SpeicherBereich`
    /// hatte. Im Bild ist ein toter Umschalter von einem lebenden nicht zu
    /// unterscheiden; im Test ist er es sofort. Genau dieser Arm fehlte.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `bool` (wie `spalte_setzen`): ein
    /// Klick auf den Reiter, der schon steht, aendert nichts — und das soll
    /// der Aufrufer wissen duerfen, ohne den Zustand zu vergleichen.
    pub fn reiter_setzen(&mut self, b: Bereich) -> bool {
        if self.reiter == b {
            return false;
        }
        self.reiter = b;
        true
    }

    /// Alles zuruecksetzen (Seitenwechsel).
    pub fn zuruecksetzen(&mut self) {
        self.cookies.clear();
        self.lokal.clear();
        self.cache.clear();
        self.gewaehlt = None;
        self.hinweis.clear();
        self.fehlgeschlagen = 0;
    }

    /// Die Cookies einer Seite aus dem echten Speicher lesen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `jetzt` von aussen kommt: die
    /// Ablauf-Zeit der Cookies ist Unix-Sekunden, und „jetzt" im Reiter ist
    /// die Zeit des Nutzers (er sieht abgelaufene Cookies grau). Der
    /// Aufrufer rechnet sie aus seiner Uhr, diese Crate nicht — sie hat
    /// keine.
    pub fn cookies_lesen(
        &mut self,
        speicher: &CookieSpeicher,
        domain: &str,
        pfad: &str,
        sicher: bool,
        jetzt: u64,
    ) -> usize {
        let g = Grenzen::STANDARDAUSGABE;
        self.cookies.clear();
        let liste = speicher.cookies_fuer_anfrage(domain, pfad, sicher, jetzt);
        for c in liste.iter() {
            if self.cookies.len() >= g.max_cookies {
                break;
            }
            self.cookies.push(CookieZeile::aus(c, jetzt));
        }
        let n = self.cookies.len();
        if n == 0 {
            self.hinweis = String::from(
                "Keine Cookies fuer diese Seite. (Ein Cookie mit `Secure` wird ueber http nicht geliefert — \
                 das ist die Regel, keine Ausnahme.)",
            );
        } else {
            self.hinweis.clear();
        }
        n
    }

    /// Die Cookies aus `Set-Cookie`-Kopfzeilen uebernehmen (D7: „ansehen").
    ///
    /// Das ist der Weg, den der Browser heute hat: GEMESSEN, dass er keinen
    /// Cookie-Speicher besitzt (siehe [`Speicher::herkunft`]). Jede Zeile
    /// wird mit `narsil_browser::set_cookie_lesen` GELESEN — also mit dem
    /// Parser, den auch der Browser benutzen wuerde. Eine kaputte Zeile wird
    /// gezaehlt, nicht geraten und nicht weggeworfen.
    ///
    /// Liefert `(gelesen, fehler)`.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Fehler ZAHLEN und nicht nur
    /// zaehlen: `set_cookie_lesen` lehnt eine Zeile ab, die kein `=` hat
    /// oder zu lang ist. Ohne diese Zahl glaubt der Nutzer, es gaebe
    /// weniger Cookies als es gab — und die Zahl im Reiter waere falsch
    /// (M7).
    pub fn cookies_aus_set_cookie(&mut self, kopf_zeilen: &[String], url: &str) -> (usize, usize) {
        let g = Grenzen::STANDARDAUSGABE;
        self.cookies.clear();
        let jetzt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut fehler = 0usize;
        //
        // GEMESSENER FEHLER in der ersten Fassung: der Rest wurde als
        // `kopf_zeilen.len() - cookies.len() - fehler` gezaehlt, und die
        // schon gezaehlten Fehler standen darin schon — also waren die
        // Zahlen groesser als die Zahl der Zeilen. GEMESSEN: 3 Kopfzeilen,
        // 1 davon kaputt, Grenze 2 Cookies -> „4 unlesbar" (es waren 3).
        // Deshalb der Index: ab ihm wurde nichts mehr gelesen.
        for (i, z) in kopf_zeilen.iter().enumerate() {
            // M3/M4: die Grenze gilt fuer die EINGABE, nicht nur fuer die
            // Ausgabe — eine kaputte Antwort darf den Reiter nicht
            // sprengen.
            if z.len() > g.max_zeile * 16 {
                fehler += 1;
                continue;
            }
            if self.cookies.len() >= g.max_cookies {
                // Der Rest wird gezaehlt, damit die Zahl stimmt (M7).
                fehler += kopf_zeilen.len().saturating_sub(i);
                break;
            }
            match narsil_browser::set_cookie_lesen(z, url, jetzt) {
                Ok(c) => self.cookies.push(CookieZeile::aus(&c, jetzt)),
                Err(_) => fehler += 1,
            }
        }
        let n = self.cookies.len();
        self.herkunft = format!(
            "Quelle: Set-Cookie dieser Sitzung ({} Zeilen, {} unlesbar) — der \
             Browser hat noch keinen Cookie-Speicher (Wunsch an die UI)",
            kopf_zeilen.len(),
            fehler
        );
        //
        // GEMESSENER FEHLER (02.10.2026, 3:5x, im Bild gesehen): der lange
        // Satz stand in der KOPFZEILE und lief unter den Loeschknopf —
        // „...(Wunsch|Alle Cookies loeschen". Die Kopfzeile hat 24 px, und
        // eine Zeile hat keinen Umbruch. Also: die KURZE Form in die
        // Kopfzeile (`herkunft_kurz`), der lange Satz in den Hinweis
        // unten links — dort steht er ganz und wird gelesen.
        self.herkunft_kurz = format!(
            "Set-Cookie: {} ({})",
            kopf_zeilen.len(),
            if fehler == 1 {
                "1 unlesbar".to_string()
            } else {
                format!("{fehler} unlesbar")
            }
        );
        if n == 0 {
            self.hinweis = if kopf_zeilen.is_empty() {
                String::from(
                    "Keine Set-Cookie-Kopfzeile in dieser Sitzung. (Das ist eine \
                     Tatsache ueber die Antworten, keine ueber den Speicher — \
                     den gibt es noch nicht.)",
                )
            } else {
                format!(
                    "{} Set-Cookie-Zeilen, aber keine war lesbar.",
                    kopf_zeilen.len()
                )
            };
        } else {
            // Der lange Satz wandert in den Hinweis (und wird von einer
            // Loeschmeldung ueberschrieben — die ist juenger und wichtiger).
            self.hinweis = self.herkunft.clone();
        }
        (n, fehler)
    }

    /// localStorage-Eintraege eintragen (vom Host).
    ///
    /// M3/M4: die Liste kommt aus dem Speicher des Browsers (also aus der
    /// Eingabe). Sie wird auf [`Grenzen::max_lokalschluessel`] gekuerzt und
    /// der Rest wird **gezaehlt** (M7), damit die Zahl im Reiter stimmt.
    pub fn lokal_eintragen(&mut self, paare: &[(String, String)]) -> usize {
        let g = Grenzen::STANDARDAUSGABE;
        self.lokal.clear();
        let mut raus = 0usize;
        for (k, v) in paare.iter() {
            if self.lokal.len() >= g.max_lokalschluessel {
                raus += 1;
                continue;
            }
            self.lokal.push(LokalZeile {
                schluessel: g.kuerze_zeile(k),
                wert: g.kuerze(v, g.max_cookiewert),
                lang: v.len() > g.max_cookiewert,
            });
        }
        self.lokal.sort_by(|a, b| a.schluessel.cmp(&b.schluessel));
        self.lokal.len() + raus
    }

    /// D18: sessionStorage-Eintraege eintragen (vom Host).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das derselbe Weg wie `lokal_eintragen`
    /// ist: beide speichern Schluessel/Wert-Paare, beide sortieren, beide
    /// haben dieselbe Grenze. Ein eigener Sortier- und Kuerzungsweg waere
    /// eine zweite Wahrheit ueber dieselbe Form.
    pub fn session_eintragen(&mut self, paare: &[(String, String)]) -> usize {
        let g = Grenzen::STANDARDAUSGABE;
        self.session.clear();
        let mut raus = 0usize;
        for (k, v) in paare.iter() {
            if self.session.len() >= g.max_lokalschluessel {
                raus += 1;
                continue;
            }
            self.session.push(LokalZeile {
                schluessel: g.kuerze_zeile(k),
                wert: g.kuerze(v, g.max_cookiewert),
                lang: v.len() > g.max_cookiewert,
            });
        }
        self.session.sort_by(|a, b| a.schluessel.cmp(&b.schluessel));
        self.session.len() + raus
    }

    /// D18: sessionStorage leeren.
    pub fn session_leeren(&mut self) -> self::LoeschErgebnis {
        let n = self.session.len();
        self.session.clear();
        LoeschErgebnis::Geloescht {
            name: format!("alle {n} Eintraege (sessionStorage)"),
        }
    }

    /// D18: Einen sessionStorage-Schluessel loeschen.
    pub fn session_loeschen(&mut self, schluessel: &str) -> self::LoeschErgebnis {
        match self.session.iter().position(|k| k.schluessel == schluessel) {
            Some(p) => {
                self.session.remove(p);
                LoeschErgebnis::Geloescht {
                    name: schluessel.to_string(),
                }
            }
            None => {
                self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                LoeschErgebnis::NichtGefunden {
                    name: schluessel.to_string(),
                }
            }
        }
    }

    /// Cache-Eintraege eintragen (vom Host).
    pub fn cache_eintragen(&mut self, eintraege: &[CacheAngabe]) -> usize {
        let g = Grenzen::STANDARDAUSGABE;
        self.cache.clear();
        for e in eintraege.iter() {
            if self.cache.len() >= g.max_cacheneintraege {
                break;
            }
            self.cache.push(CacheZeile {
                url: g.kuerze_zeile(&e.url),
                groesse: e.groesse,
                alter_ms: e.alter_ms,
                frisch: e.frisch,
            });
        }
        self.cache.len()
    }

    /// Ein Cookie loeschen (D7: „ansehen/loeschen").
    ///
    /// Liefert das Ergebnis als Wert: `true` = geloescht. Der Aufrufer
    /// entfernt es im echten Speicher und sagt hier Bescheid — diese Crate
    /// darf das nicht selbst, weil sie den Speicher nicht besitzt.
    pub fn cookie_loeschen(&mut self, name: &str) -> self::LoeschErgebnis {
        match self.cookies.iter().position(|c| c.name == name) {
            Some(p) => {
                self.cookies.remove(p);
                if self.gewaehlt.as_deref() == Some(name) {
                    self.gewaehlt = None;
                }
                LoeschErgebnis::Geloescht {
                    name: name.to_string(),
                }
            }
            None => {
                self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                LoeschErgebnis::NichtGefunden {
                    name: name.to_string(),
                }
            }
        }
    }

    /// Alle Cookies loeschen.
    pub fn alle_cookies_loeschen(&mut self) -> self::LoeschErgebnis {
        let n = self.cookies.len();
        self.cookies.clear();
        self.gewaehlt = None;
        LoeschErgebnis::Geloescht {
            name: format!("alle {n} Cookies"),
        }
    }

    /// Einen localStorage-Schluessel loeschen.
    pub fn lokal_loeschen(&mut self, schluessel: &str) -> self::LoeschErgebnis {
        match self.lokal.iter().position(|k| k.schluessel == schluessel) {
            Some(p) => {
                self.lokal.remove(p);
                LoeschErgebnis::Geloescht {
                    name: schluessel.to_string(),
                }
            }
            None => {
                self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                LoeschErgebnis::NichtGefunden {
                    name: schluessel.to_string(),
                }
            }
        }
    }

    /// localStorage leeren.
    pub fn lokal_leeren(&mut self) -> self::LoeschErgebnis {
        let n = self.lokal.len();
        self.lokal.clear();
        LoeschErgebnis::Geloescht {
            name: format!("alle {n} Eintraege"),
        }
    }

    /// Einen Cache-Eintrag loeschen.
    pub fn cache_loeschen(&mut self, url: &str) -> self::LoeschErgebnis {
        match self.cache.iter().position(|c| c.url == url) {
            Some(p) => {
                self.cache.remove(p);
                LoeschErgebnis::Geloescht {
                    name: url.to_string(),
                }
            }
            None => {
                self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                LoeschErgebnis::NichtGefunden {
                    name: url.to_string(),
                }
            }
        }
    }

    /// Den Cache leeren.
    pub fn cache_leeren(&mut self) -> self::LoeschErgebnis {
        let n = self.cache.len();
        self.cache.clear();
        LoeschErgebnis::Geloescht {
            name: format!("alle {n} Cache-Eintraege"),
        }
    }

    /// D18: EINEN WERT BEARBEITEN (Auftrag der Leitung: „lesen+bearbeiten").
    ///
    /// GEMESSENER BEFUND VORHER: der Reiter konnte Cookies und
    /// localStorage-Eintraege **ansehen und loeschen** — nicht aendern. Ein
    /// Werkzeug, in dem man einen falschen localStorage-Wert nur loeschen
    /// kann, ist die Haelfte von Firexz-Funktion.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der neue Wert hier EINGETRAGEN wird und
    /// der Aufrufer ihn zurueckschreibt: diese Crate besitzt den Speicher
    /// nicht (siehe [`Speicher::herkunft`]). Sie weiss, was der neue Wert
    /// sein soll; der Host weiss, wo er hingehört. Also: hier wird der
    /// Zustand geaendert, und der Aufrufer liest ihn zurueck — genau wie
    /// beim Loeschen.
    ///
    /// Liefert `true`, wenn sich etwas geaendert hat. Leerer neuer Wert ist
    /// erlaubt (Firefox laesst `""` zu), ein leerer Schluessel nicht — ohne
    /// Namen gibt es keine Zeile, die man wiederfinden wuerde.
    pub fn wert_setzen(&mut self, schluessel: &str, wert: &str) -> bool {
        let g = Grenzen::STANDARDAUSGABE;
        if schluessel.trim().is_empty() {
            self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
            self.hinweis = String::from("Nichts gesetzt: der Name war leer");
            return false;
        }
        let schluessel = g.kuerze_zeile(schluessel);
        // Der Name steht am Ende noch in der Meldung — und er wandert in
        // einen der beiden Aeste hinein. Also eine Kopie fuer die Meldung,
        // statt den Namen an beiden zu verschieben.
        let anzeige = schluessel.clone();
        let wert = g.kuerze(wert, g.max_cookiewert);
        let lang = wert.chars().count() > g.max_cookiewert;
        match self.reiter {
            Bereich::Lokalspeicher => {
                match self.lokal.iter_mut().find(|k| k.schluessel == schluessel) {
                    Some(k) => {
                        if k.wert == wert {
                            return false;
                        }
                        k.wert = wert;
                        k.lang = lang;
                    }
                    None => {
                        if self.lokal.len() >= g.max_lokalschluessel {
                            self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                            self.hinweis = String::from("localStorage ist voll");
                            return false;
                        }
                        self.lokal.push(LokalZeile {
                            schluessel,
                            wert,
                            lang,
                        });
                        self.lokal.sort_by(|a, b| a.schluessel.cmp(&b.schluessel));
                    }
                }
            }
            Bereich::Sitzung => {
                match self.session.iter_mut().find(|k| k.schluessel == schluessel) {
                    Some(k) => {
                        if k.wert == wert {
                            return false;
                        }
                        k.wert = wert;
                        k.lang = lang;
                    }
                    None => {
                        if self.session.len() >= g.max_lokalschluessel {
                            self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                            self.hinweis = String::from("sessionStorage ist voll");
                            return false;
                        }
                        self.session.push(LokalZeile {
                            schluessel,
                            wert,
                            lang,
                        });
                        self.session.sort_by(|a, b| a.schluessel.cmp(&b.schluessel));
                    }
                }
            }
            // GEMESSENE ENTSCHEIDUNG: bei Cookies wird der Wert am
            // Cookie selbst geaendert (siehe `cookie_wert_setzen`), und bei
            // Cache gibt es keinen Wert — dort ist die Zelle eine URL.
            Bereich::Cookies | Bereich::Cache => {
                self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
                self.hinweis = format!("{} laesst sich nicht so bearbeiten", self.reiter.name());
                return false;
            }
        }
        self.hinweis = format!("{} = {}", self.reiter.name(), anzeige);
        true
    }

    /// D18: Den Wert eines Cookies aendern.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es einen eigenen Weg gibt und nicht
    /// `wert_setzen`: ein Cookie hat Name, Domain und Pfad; ein localStorage-
    /// Eintrag nur einen Schluessel. Ein gemeinsamer Weg muesste Felder
    /// erfinden, die einer der beiden nicht hat.
    pub fn cookie_wert_setzen(&mut self, name: &str, wert: &str) -> bool {
        let g = Grenzen::STANDARDAUSGABE;
        let Some(c) = self.cookies.iter_mut().find(|c| c.name == name) else {
            self.fehlgeschlagen = self.fehlgeschlagen.saturating_add(1);
            self.hinweis = format!("Nichts gesetzt: {name} gibt es nicht mehr");
            return false;
        };
        let kurz = g.kuerze(wert, g.max_cookiewert);
        if c.wert == kurz {
            return false;
        }
        c.wert = kurz.clone();
        c.wert_voll = wert.to_string();
        c.wert_lang = wert.chars().count() > g.max_cookiewert;
        self.hinweis = format!("Cookie {name} = {kurz}");
        true
    }

    /// DER LOESCHKNOPF: die Auswahl loeschen, sonst den ganzen Bereich.
    ///
    /// # DER EINE WEG FUER ALLE DREI BEREICHE
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x): sechs Loeschfunktionen
    /// (`cookie_loeschen`, `alle_cookies_loeschen`, `lokal_loeschen`,
    /// `lokal_leeren`, `cache_loeschen`, `cache_leeren`) waren ALLE ohne
    /// Aufrufer — der Reiter konnte nichts loeschen. Jetzt gibt es EINEN
    /// Aufruf, der den Bereich nimmt, und die Bruecke ruft ihn fuer alle
    /// drei. Wer fuer jeden Bereich eine eigene Bruecke-Schreibweise
    /// braucht, hat drei Stellen, an denen die Semantik auseinanderlaufen
    /// kann („leert" gegen „loescht einen").
    ///
    /// Liefert das Ergebnis als Text — der Reiter zeigt, WAS passiert
    /// ist, und der Aufrufer sagt, ob es im echten Speicher auch passiert
    /// ist (der Browser hat den noch nicht, siehe [`Speicher::herkunft`]).
    pub fn loeschen(&mut self) -> String {
        let gewaehlt = self.gewaehlt.clone();
        let r = match (self.reiter, gewaehlt.as_deref()) {
            (Bereich::Cookies, Some(n)) => self.cookie_loeschen(n),
            (Bereich::Cookies, None) => self.alle_cookies_loeschen(),
            (Bereich::Lokalspeicher, Some(k)) => self.lokal_loeschen(k),
            (Bereich::Lokalspeicher, None) => self.lokal_leeren(),
            (Bereich::Sitzung, Some(k)) => self.session_loeschen(k),
            (Bereich::Sitzung, None) => self.session_leeren(),
            (Bereich::Cache, Some(u)) => self.cache_loeschen(u),
            (Bereich::Cache, None) => self.cache_leeren(),
        };
        match r {
            LoeschErgebnis::Geloescht { name } => {
                let t = format!("Geloescht: {name}");
                self.hinweis = t.clone();
                t
            }
            LoeschErgebnis::NichtGefunden { name } => {
                // GEMESSENE ENTSCHEIDUNG: ein Fehler, der stillschweigend
                // nichts tut, ist der Fehler „Zahl stimmt, Bild stimmt
                // nicht". Der Reiter sagt es.
                let t = format!("Nichts geloescht: {name} gibt es nicht mehr");
                self.hinweis = t.clone();
                t
            }
        }
    }

    /// Die Kopfzeile des Reiters mit den Zahlen.
    pub fn kopf_zeile(&self) -> String {
        let z = format!(
            "Cookies {} | localStorage {} | sessionStorage {} | Cache {} | {}",
            self.cookies.len(),
            self.lokal.len(),
            self.session.len(),
            self.cache.len(),
            self.reiter.name()
        );
        if self.herkunft_kurz.is_empty() {
            return z;
        }
        format!("{z} | {}", self.herkunft_kurz)
    }

    /// Die Zeilen des gewaehlten Bereichs — so breit wie die Felder sind.
    ///
    /// Das ist die Fassung fuer Logs und Tests. Der REITER benutzt
    /// [`Speicher::zeilen_fuer`], weil ein Fenster beliebig breit ist.
    pub fn zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        match self.reiter {
            Bereich::Cookies => self
                .cookies
                .iter()
                .map(|c| {
                    let marke = if self.gewaehlt.as_deref() == Some(c.name.as_str()) {
                        "▸"
                    } else {
                        " "
                    };
                    //
                    // GEMESSENER FEHLER (02.10.2026, 3:5x, im Bild gesehen):
                    // die Spalten waren 24 + 28 + 6 + 10 Zeichen breit, also
                    // breiter als die linke Spalte — das Zeilenende lag
                    // unter der Detailansicht und wurde VON DEREN
                    // Hintergrund zugedeckt. Eine Zeile, die zur Haelfte
                    // existiert, ist schlimmer als eine kurze: sie sieht aus
                    // wie ein Fehler in den Daten.
                    // Die Breiten sind jetzt so gewaehlt, dass der laengste
                    // Wert in jede Spalte passt (Name 20, Domain 20, Pfad 8,
                    // secure 6, Ablauf der Rest).
                    g.kuerze_zeile(&format!(
                        "{} {:<20} {:<20} {:<8} {:<6} {}",
                        marke,
                        c.name,
                        c.domain,
                        c.pfad,
                        if c.secure { "secure" } else { "-" },
                        c.ablauf_text
                    ))
                })
                .collect(),
            Bereich::Lokalspeicher | Bereich::Sitzung => self
                .lokal_zeilen()
                .iter()
                .map(|k| g.kuerze_zeile(&format!("{:<30} {}", k.schluessel, k.wert)))
                .collect(),
            Bereich::Cache => self
                .cache
                .iter()
                .map(|c| {
                    g.kuerze_zeile(&format!(
                        "{:<8} {:<10} {}",
                        if c.frisch { "frisch" } else { "alt" },
                        c.groesse,
                        c.url
                    ))
                })
                .collect(),
        }
    }

    /// Die Zeilen fuer eine bestimmte BREITE (der Weg des Reiters).
    ///
    /// # WARUM DIE BREITE EIN PARAMETER IST (gemessen 02.10.2026, 3:5x)
    ///
    /// GEMESSENER FEHLER: die Cookie-Zeile war ein festes Spaltenformat
    /// (24 + 28 + 6 + 10 Zeichen). `zeichnen::spalten` gibt der linken
    /// Spalte `Breite/5`, also bei 1388 px genau 277 px — die Zeile brauchte
    /// aber ~420 px. Im Bild standen deshalb nur Name, Domain, Pfad und
    /// `secure`, und der Rest (die Ablaufzeit) lag UNTER der Detailansicht,
    /// die spaeter gezeichnet wird und ihn zudeckt. Eine halb sichtbare
    /// Zeile sieht aus wie kaputte Daten.
    ///
    /// Also: die Felder in ihrer WICHTIGKEIT (Name, Domain, secure, Pfad,
    /// Ablauf) und so viele, wie in die Breite passen. Was nicht passt,
    /// steht in der Detailansicht — dort steht es immer.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum 7 px je Zeichen: das ist die Breite
    /// der Standardschrift im Raster (5x7-Bitmap, 6/7 em). Wer die
    /// Schriftgroesse aendert, muss die Zahl anpassen — sie steht deshalb
    /// als EINE Konstante hier und nirgends sonst.
    pub fn zeilen_fuer(&self, breite_px: f32) -> Vec<String> {
        const ZEICHEN_BREITE_PX: f32 = 7.0;
        let g = Grenzen::STANDARDAUSGABE;
        // Der Rand der Spalte (6 px links + 6 px Luft) geht ab.
        let platz = ((breite_px - 14.0) / ZEICHEN_BREITE_PX).floor().max(8.0) as usize;
        match self.reiter {
            Bereich::Cookies => self
                .cookies
                .iter()
                .map(|c| {
                    let marke = if self.gewaehlt.as_deref() == Some(c.name.as_str()) {
                        "▸"
                    } else {
                        " "
                    };
                    let felder: [(&str, &str); 5] = [
                        ("Name", &c.name),
                        ("Domain", &c.domain),
                        ("secure", if c.secure { "secure" } else { "-" }),
                        ("Pfad", &c.pfad),
                        ("Ablauf", &c.ablauf_text),
                    ];
                    g.kuerze_zeile(&setze_zeile(marke, &felder, platz))
                })
                .collect(),
            Bereich::Lokalspeicher | Bereich::Sitzung => self
                .lokal_zeilen()
                .iter()
                .map(|k| {
                    let felder = [("Name", k.schluessel.as_str()), ("Wert", k.wert.as_str())];
                    g.kuerze_zeile(&setze_zeile(" ", &felder, platz))
                })
                .collect(),
            Bereich::Cache => self
                .cache
                .iter()
                .map(|c| {
                    let felder = [
                        ("Zustand", if c.frisch { "frisch" } else { "alt" }),
                        ("Bytes", &c.groesse.to_string()),
                        ("URL", c.url.as_str()),
                    ];
                    g.kuerze_zeile(&setze_zeile(" ", &felder, platz))
                })
                .collect(),
        }
    }

    /// D18: Die Zeilen des gerade gewaehlten Speicher-Bereichs, der
    /// Schluessel/Wert traegt (localStorage ODER sessionStorage).
    ///
    /// GEMESSENE ENTSCHEIDUNG: EIN Weg fuer beide Bereiche. Sie haben
    /// dieselbe Zeilenform; zwei Wege wuerden bedeuten, dass eines Tages der
    /// eine Weg gekuerzt wird und der andere nicht — und die Zahl der Zeilen
    /// im Bild waere dann die eine und die Zahl im Protokoll die andere.
    fn lokal_zeilen(&self) -> &[LokalZeile] {
        match self.reiter {
            Bereich::Sitzung => &self.session,
            _ => &self.lokal,
        }
    }

    /// Die Zeilen der Detailansicht (Bezeichnung + Pfad).
    pub fn detail_zeilen(&self) -> Vec<String> {
        let Some(name) = self.gewaehlt.as_deref() else {
            return vec![String::from("Nichts gewaehlt.")];
        };
        let g = Grenzen::STANDARDAUSGABE;
        let Some(c) = self.cookies.iter().find(|c| c.name == name) else {
            return vec![String::from("Nichts gewaehlt.")];
        };
        let mut r = Vec::new();
        r.push(format!("Name: {}", c.name));
        r.push(format!("Wert: {}", c.wert));
        r.push(format!("Domain: {}", c.domain));
        r.push(format!("Pfad: {}", c.pfad));
        r.push(format!(
            "Secure: {} | Host-only: {} | Ablauf: {}",
            if c.secure { "ja" } else { "nein" },
            if c.host_only { "ja" } else { "nein" },
            c.ablauf_text
        ));
        if c.wert_lang {
            r.push(format!(
                "(gekuerzt: der Wert hat mehr als {} Zeichen)",
                g.max_cookiewert
            ));
        }
        r
    }
}

/// Das Ergebnis eines Löschvorgangs.
///
/// # WARUM EIN EIGENER TYP UND NICHT `bool`
///
/// M7: ein Test, der nur `assert!(loeschen("x"))` schreibt, kann nicht
/// unterscheiden zwischen „geloescht" und „gab es nicht". Der Typ
/// nennt den Namen, der geloescht wurde (oder gesucht wurde) — und
/// genau das steht in der Meldung, die der Nutzer sieht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoeschErgebnis {
    /// Es ist weg; der Name steht drin.
    Geloescht {
        /// Was geloescht wurde.
        name: String,
    },
    /// Es gab es nicht.
    NichtGefunden {
        /// Was gesucht wurde.
        name: String,
    },
}

/// Die vier Bereiche des Speicher-Reiters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bereich {
    /// Cookies.
    Cookies,
    /// localStorage.
    Lokalspeicher,
    /// D18: sessionStorage (der Zwilling, der beim Schliessen des Tabs
    /// verschwindet).
    Sitzung,
    /// Der HTTP-Cache.
    Cache,
}

/// DIE DREI BEREICHSKNOPFE IN FESTER REIHENFOLGE.
///
/// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x): die Liste stand als Array im
/// Zeichencode, und der Treffer (der Bridge) brauchte dieselbe Liste. Zwei
/// Listen sind zwei Reihenfolgen — und die zweite stimmt irgendwann nicht.
/// Das Array ist jetzt EINE Konstante, und beide benutzen sie in dieser
/// Reihenfolge.
pub const BEREICHE: [Bereich; 4] = [
    Bereich::Cookies,
    Bereich::Lokalspeicher,
    Bereich::Sitzung,
    Bereich::Cache,
];

impl Bereich {
    /// Der Text des LOESCHKNOPFS in diesem Zustand.
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x), warum der Knopf seine
    /// ABTATUNG nennt und nicht nur „loeschen": zwei Knoepfe („Auswahl"
    /// und „alle") brauchen eine Entscheidung, die der Reiter ohne den
    /// Nutzer trifft — und der Nutzer soll sie sehen. „Alle Cookies
    /// loeschen" ist ein Satz, den man bereuen kann; „Auswahl loeschen"
    /// nicht. Also steht die Absicht am Knopf.
    pub const fn loesch_text(self, gewaehlt: bool) -> &'static str {
        match (self, gewaehlt) {
            (Bereich::Cookies, true) => "Cookie loeschen",
            (Bereich::Cookies, false) => "Alle Cookies loeschen",
            (Bereich::Lokalspeicher, true) => "Schluessel loeschen",
            (Bereich::Lokalspeicher, false) => "localStorage leeren",
            (Bereich::Sitzung, true) => "Schluessel loeschen",
            (Bereich::Sitzung, false) => "sessionStorage leeren",
            (Bereich::Cache, true) => "Cache-Eintrag loeschen",
            (Bereich::Cache, false) => "Cache leeren",
        }
    }

    /// Der Name im Reiter.
    pub const fn name(self) -> &'static str {
        match self {
            Bereich::Cookies => "Cookies",
            Bereich::Lokalspeicher => "localStorage",
            Bereich::Sitzung => "sessionStorage",
            Bereich::Cache => "Cache",
        }
    }
}

/// So viele Felder in `platz` Zeichen, wie hineinpassen — die restlichen
/// Felder weggelassen (ihr Inhalt steht in der Detailansicht).
///
/// GEMESSENE ENTSCHEIDUNG, warum die SPALTEN nicht gestreckt werden: ein
/// Feld, das nur einen Zeichen Inhalt hat, wuerde eine halbe Bildschirm-
/// breite fuer ein „-" reservieren. Die Felder stehen stattdessen direkt
/// hintereinander, getrennt durch zwei Leerzeichen.
fn setze_zeile(marke: &str, felder: &[(&str, &str)], platz: usize) -> String {
    let mut raus = String::from(marke);
    for (_, w) in felder {
        // + 2 Leerzeichen als Trenner.
        if raus.chars().count() + 2 + w.chars().count() > platz {
            break;
        }
        if raus.chars().count() > 1 {
            raus.push_str("  ");
        }
        raus.push_str(w);
    }
    raus
}

/// Eine Cookie-Zeile im Reiter.
#[derive(Debug, Clone, PartialEq)]
pub struct CookieZeile {
    /// Der Name.
    pub name: String,
    /// Der Wert (gekuerzt).
    pub wert: String,
    /// Der volle Wert (bis 4096 Zeichen, die Fremdgrenze).
    pub wert_voll: String,
    /// `true`, wenn der Wert gekuerzt wurde.
    pub wert_lang: bool,
    /// Die Domain.
    pub domain: String,
    /// Der Pfad.
    pub pfad: String,
    /// `Secure`.
    pub secure: bool,
    /// Host-only (kein `Domain`-Attribut).
    pub host_only: bool,
    /// Der Ablauf (Unix-Sekunden), `None` = Sitzung.
    pub ablauf: Option<u64>,
    /// Der Ablauf als Text.
    pub ablauf_text: String,
}

impl CookieZeile {
    /// Aus einem `narsil_browser::Cookie`.
    pub fn aus(c: &Cookie, jetzt: u64) -> Self {
        let g = Grenzen::STANDARDAUSGABE;
        CookieZeile {
            name: g.kuerze_zeile(&c.name),
            wert: g.kuerze(&c.wert, g.max_cookiewert),
            wert_voll: g.kuerze(&c.wert, 4_096),
            wert_lang: c.wert.chars().count() > g.max_cookiewert,
            domain: g.kuerze_zeile(c.domain.as_deref().unwrap_or("")),
            pfad: g.kuerze_zeile(c.path.as_deref().unwrap_or("/")),
            secure: c.secure,
            host_only: c.host_only,
            ablauf: c.ablauf,
            ablauf_text: ablauf_text(c.ablauf, jetzt),
        }
    }
}

/// Der Ablauf als Text: Sekunden, Tagen oder „sitzend".
fn ablauf_text(ablauf: Option<u64>, jetzt: u64) -> String {
    match ablauf {
        None => String::from("Sitzung"),
        Some(0) => String::from("sitzend"),
        Some(a) if a <= jetzt => String::from("abgelaufen"),
        Some(a) => {
            let rest = a.saturating_sub(jetzt);
            if rest < 3_600 {
                format!("in {} min", rest / 60)
            } else if rest < 86_400 {
                format!("in {} h", rest / 3_600)
            } else {
                format!("in {} Tagen", rest / 86_400)
            }
        }
    }
}

/// Eine localStorage-Zeile.
#[derive(Debug, Clone, PartialEq)]
pub struct LokalZeile {
    /// Der Schlüssel.
    pub schluessel: String,
    /// Der Wert (gekuerzt).
    pub wert: String,
    /// `true`, wenn der Wert gekuerzt wurde.
    pub lang: bool,
}

/// Eine Cache-Zeile.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheZeile {
    /// Die URL.
    pub url: String,
    /// Die Größe in Byte.
    pub groesse: u64,
    /// Wie alt der Eintrag ist (ms).
    pub alter_ms: u64,
    /// `true`, wenn er noch frisch ist (Cache-Control).
    pub frisch: bool,
}

/// Was der Host über einen Cache-Eintrag weiss.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheAngabe {
    /// Die URL.
    pub url: String,
    /// Die Größe.
    pub groesse: u64,
    /// Das Alter in ms.
    pub alter_ms: u64,
    /// Frisch oder nicht.
    pub frisch: bool,
}

impl CacheAngabe {
    /// Eine Angabe aus den vier Zahlen.
    pub fn neu(url: &str, groesse: u64, alter_ms: u64, frisch: bool) -> Self {
        Self {
            url: url.to_string(),
            groesse,
            alter_ms,
            frisch,
        }
    }
}

/// Der Cookie-Fehler als Text für die Meldung im Reiter.
///
/// GEMESSENE BEGRÜNDUNG: `CookieFehler` hat **sieben** Varianten, jede mit
/// Zahlen drin (`ZuVieleGesamt { aktuell, maximum }`). Ein `Display` auf
/// den Typ würde die Zahlen weglassen; hier stehen sie im Text, weil der
/// Nutzer daraus sieht, ob er sein Cookie loeschen kann (bei
/// „ZuVieleProDomain" ist es eine Entscheidung, kein Fehler).
pub fn cookie_fehler_text(f: &CookieFehler) -> String {
    match f {
        CookieFehler::ZuVieleGesamt { aktuell, maximum } => {
            format!("zu viele Cookies: {aktuell} von {maximum}")
        }
        CookieFehler::ZuVieleProDomain {
            domain,
            aktuell,
            maximum,
        } => {
            format!("zu viele Cookies fuer {domain}: {aktuell} von {maximum}")
        }
        CookieFehler::ZuGroesser { groesse, maximum } => {
            format!("zu gross: {groesse} Byte (Grenze {maximum})")
        }
        CookieFehler::UngueltigeDomain(d) => format!("Domain passt nicht: {d}"),
        CookieFehler::UngueltigerName(n) => format!("Cookie-Name unbrauchbar: {n}"),
        CookieFehler::UngueltigerHeader(h) => format!("Set-Cookie unlesbar: {h}"),
    }
}

#[cfg(test)]
mod speicher_eigene {
    use super::*;

    /// DIE COOKIES KOMMEN AUS DEM ECHTEN SPEICHER DES BROWSERS.
    ///
    /// GEMESSENE ENTSCHEIDUNG: der Test benutzt `CookieSpeicher::ablegen`
    /// (die echte Methode) und liest über `cookies_laschen`. Damit ist
    /// gemessen, dass die Brücke zu `narsil-browser` trägt — nicht nur, dass
    /// diese Crate eine Liste hat.
    #[test]
    fn die_cookies_kommen_aus_dem_browser_speicher() {
        let mut sp = CookieSpeicher::neu();
        let jetzt = 1_700_000_000u64;
        // GEMESSENE ENTSCHEIDUNG: die Cookies werden aus `Set-Cookie`
        // gebaut — dem Weg, den der Browser nimmt. Ein handgebautes
        // `Cookie` waere ein Cookie, das kein Server geschickt hat.
        let mk = |name: &str, wert: &str, zusatz: &str| {
            narsil_browser::set_cookie_lesen(
                &format!("{name}={wert}; Domain=beispiel.test; Path=/{zusatz}"),
                "https://beispiel.test/",
                jetzt,
            )
            .expect("Set-Cookie lesen")
        };
        assert_eq!(
            sp.ablegen(mk("a", "1", "; Max-Age=600"), jetzt),
            Ok(()),
            "der Cookie wurde nicht angenommen"
        );
        assert_eq!(sp.ablegen(mk("b", "2", "; Max-Age=600"), jetzt), Ok(()));
        // Ein Cookie einer anderen Domain: wird NICHT geliefert.
        let fremd = narsil_browser::set_cookie_lesen(
            "c=3; Domain=anders.test; Path=/; Max-Age=600",
            "https://anders.test/",
            jetzt,
        )
        .expect("Set-Cookie lesen");
        assert_eq!(sp.ablegen(fremd, jetzt), Ok(()));
        assert_eq!(sp.anzahl(), 3, "der Speicher hat nicht drei Cookies");

        let mut s = Speicher::neu();
        let n = s.cookies_lesen(&sp, "beispiel.test", "/", false, jetzt);
        assert_eq!(n, 2, "es kamen nicht zwei Cookies an: {n}");
        assert!(s.cookies.iter().any(|c| c.name == "a"), "Cookie a fehlt");
        assert!(
            !s.cookies.iter().any(|c| c.name == "c"),
            "ein fremdes Cookie kam an"
        );
        assert_eq!(s.cookies[0].domain, "beispiel.test");
        assert_eq!(s.cookies[0].pfad, "/");
        assert_eq!(s.cookies[0].wert, "1");
        // UND der Ablauf steht als Text da (M7).
        assert!(
            s.cookies[0].ablauf_text.contains("min"),
            "der Ablauf: {}",
            s.cookies[0].ablauf_text
        );
        assert!(s.cookies.iter().all(|c| c.ablauf_text != "abgelaufen"));
    }

    /// EIN SICHERES COOKIE KOMMT NUR UEBER HTTPS.
    ///
    /// GEMESSENE BEGRÜENDUNG: `cookies_fuer_anfrage` hat den Parameter
    /// `sicher`, und ein `Secure`-Cookie wird über http nicht geliefert
    /// (RFC 6265 4.1.2.5). Der Reiter darf daraus nichts machen — der
    /// Nutzer sucht ein Cookie, das er nicht findet, und haelt die Seite
    /// fuer kaputt. Also: es wird gemessen, dass es fehlt, UND der
    /// Reiter sagt warum.
    #[test]
    fn ein_sicheres_cookie_fehlt_ueber_http_und_der_reiter_sagt_es() {
        let mut sp = CookieSpeicher::neu();
        let jetzt = 1_700_000_000u64;
        let sicher = narsil_browser::set_cookie_lesen(
            "s=1; Domain=beispiel.test; Path=/; Secure",
            "https://beispiel.test/",
            jetzt,
        )
        .expect("Set-Cookie lesen");
        assert!(
            sicher.secure,
            "der sichere Cookie ist nicht als Secure erkannt"
        );
        assert_eq!(sp.ablegen(sicher, jetzt), Ok(()));
        let mut s = Speicher::neu();
        assert_eq!(s.cookies_lesen(&sp, "beispiel.test", "/", false, jetzt), 0);
        assert!(
            s.hinweis.contains("Secure"),
            "der Reiter sagt nicht, warum das Cookie fehlt: {}",
            s.hinweis
        );
        // Ueber https ist es da, und der Hinweis verschwindet.
        assert_eq!(s.cookies_lesen(&sp, "beispiel.test", "/", true, jetzt), 1);
        assert!(s.hinweis.is_empty(), "der Hinweis blieb stehen");
    }

    /// LOESCHEN GIBT EINEN WERT ZURUECK, DER DEN NAMEN NENNT.
    ///
    /// GEMESSENE BEGRÜNDUNG: M7 — „ein Test, der nur `ja` sagt, kann nicht
    /// unterscheiden zwischen geloescht und nicht vorhanden". Der Test
    /// prüft deshalb BEIDE Faelle am **Namen**.
    #[test]
    fn loeschen_nennt_den_namen_und_meldet_nicht_vorhandenes() {
        let mut sp = CookieSpeicher::neu();
        let jetzt = 1_700_000_000u64;
        sp.ablegen(
            narsil_browser::set_cookie_lesen("a=1; Domain=b.test; Path=/", "https://b.test/", jetzt)
                .expect("Set-Cookie lesen"),
            jetzt,
        )
        .expect("ablegen");
        let mut s = Speicher::neu();
        s.cookies_lesen(&sp, "b.test", "/", false, jetzt);
        // Ein Cookie, das es nicht gibt.
        assert_eq!(
            s.cookie_loeschen("gibtsnicht"),
            LoeschErgebnis::NichtGefunden {
                name: "gibtsnicht".to_string()
            },
            "das Loeschen eines fremden Cookies meldet Erfolg"
        );
        assert_eq!(s.fehlgeschlagen, 1, "der Fehlerzaehler zaehlt nicht");
        // Das echte.
        assert_eq!(
            s.cookie_loeschen("a"),
            LoeschErgebnis::Geloescht {
                name: "a".to_string()
            }
        );
        assert!(s.cookies.is_empty());
        // Und im Browser-Speicher.
        assert!(
            sp.entfernen("a"),
            "der echte Speicher hat den Cookie nicht entfernt"
        );
        // Alle.
        assert!(matches!(
            s.alle_cookies_loeschen(),
            LoeschErgebnis::Geloescht { .. }
        ));
    }

    /// BOMBEN (M8): 5000 SCHLUESSEL, 4-KB-WERTE, u32::MAX-SEITEN.
    ///
    /// GEMESSENE BEGRÜNDUNG: localStorage ist nach der HTML-Spezifikation
    /// auf 5 MiB je Herkunft begrenzt; 5000 Einträge à 4 KiB sind genau
    /// diese Grenze. Der Reiter baut daraus eine Liste, die er zeigen kann.
    #[test]
    fn bomben_lokalspeicher_kommen_mit_einem_benannten_ergebnis() {
        let g = Grenzen::STANDARDAUSGABE;
        let mut s = Speicher::neu();
        // 5000 Einträge mit 4-KiB-Werten.
        let paare: Vec<(String, String)> = (0..5_000)
            .map(|i| (format!("k{i}"), "v".repeat(4_096)))
            .collect();
        let n = s.lokal_eintragen(&paare);
        assert_eq!(
            s.lokal.len(),
            g.max_lokalschluessel,
            "die Liste ist nicht begrenzt"
        );
        assert_eq!(n, 5_000, "die Gesamtzahl wird nicht gemeldet");
        for z in s.lokal.iter() {
            assert!(
                z.wert.chars().count() <= g.max_cookiewert,
                "ein Wert ist {} Zeichen lang",
                z.wert.chars().count()
            );
            assert!(z.lang, "der Wert meldet nicht, dass er gekuerzt wurde");
        }
        // Die Zeilen sind lesbar — im RICHTIGEN Bereich (der Reiter zeigt
        // nur den aktiven Bereich, sonst wuerde der Test die Cookie-Liste
        // messen und sich wundern, dass sie leer ist).
        s.reiter = Bereich::Lokalspeicher;
        assert_eq!(
            s.zeilen().len(),
            g.max_lokalschluessel,
            "die localStorage-Zeilen fehlen"
        );
        // 2. Eine leere Liste.
        let mut leer = Speicher::neu();
        assert_eq!(leer.lokal_eintragen(&[]), 0);
        // 3. Cache mit u32::MAX-Groessen.
        leer.cache_eintragen(&[CacheAngabe::neu("https://a/", u64::MAX, u64::MAX, false)]);
        assert_eq!(
            leer.cache[0].groesse,
            u64::MAX,
            "die Groesse wurde abgeschnitten"
        );
    }

    /// DIE DREI BEREICHE SIND DREI, UND JEDER HAT SEINE ZEILEN.
    ///
    /// M7: der Test nennt die Zeilen aller drei Bereiche, nicht nur die
    /// Zahl — ein Bereich, der Zeilen zaehlt aber keine hat, waere die
    /// Fehlerklasse „Zahl berechnet, nicht geprueft".
    #[test]
    fn die_drei_bereiche_haben_zeilen() {
        let mut s = Speicher::neu();
        s.cookies.push(CookieZeile {
            name: "a".into(),
            wert: "1".into(),
            wert_voll: "1".into(),
            wert_lang: false,
            domain: "b.test".into(),
            pfad: "/".into(),
            secure: false,
            host_only: true,
            ablauf: None,
            ablauf_text: "Sitzung".into(),
        });
        s.lokal_eintragen(&[("k".to_string(), "v".to_string())]);
        s.cache_eintragen(&[CacheAngabe::neu("https://a/x", 100, 5, true)]);
        s.gewaehlt = Some("a".into());
        assert_eq!(s.reiter, Bereich::Cookies);
        assert_eq!(s.zeilen().len(), 1);
        assert!(s.zeilen()[0].contains('▸'), "die Auswahl steht nicht da");
        // Name, Wert, Domain, Pfad, Secure/Host-only/Ablauf = fuenf Zeilen.
        assert_eq!(
            s.detail_zeilen().len(),
            5,
            "die Detailansicht ist unvollstaendig"
        );
        s.reiter = Bereich::Lokalspeicher;
        assert!(
            s.zeilen()[0].contains('k'),
            "die localStorage-Zeile: {:?}",
            s.zeilen()
        );
        s.reiter = Bereich::Cache;
        assert!(
            s.zeilen()[0].contains("frisch"),
            "die Cache-Zeile: {:?}",
            s.zeilen()
        );
        let k = s.kopf_zeile();
        assert!(
            k.contains("Cookies 1") && k.contains("Cache 1"),
            "die Kopfzeile: {k}"
        );
    }
}
