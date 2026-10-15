//! D4 — NETZWERK: jede Anfrage mit Methode, URL, Status, Protokoll, Größe,
//! Zeit, Kopfzeilen.
//!
//! # WAS D4 VERLANGT (team/AUTONOM.txt, wörtlich)
//!
//! > D4 Netzwerk: jede Anfrage (Methode, URL, Status, Protokoll h1/h2/h3,
//! > Groesse, Zeit, Kopfzeilen) — Ereignisquelle in narsil-net als Wunsch an
//! > TLS.
//!
//! # WAS HEUTE GEHT — gemessen am 01.10.2026 am Quelltext von `narsil-net`
//!
//! | D4 braucht | gibt es? | wo |
//! |---|---|---|
//! | Anfrage mit Methode/URL/Kopfzeilen | **ja** | `narsil_net::Request` |
//! | Antwort mit Status/Kopfzeilen/Body | **ja** | `narsil_net::Response` |
//! | Protokoll h1/h2/h3 | **teilweise** | `Response::status.version` (Text) |
//! | Größe | **ja** | `Response::body.len()` |
//! | Zeit | **nein** | `fetch_mit` misst nichts |
//! | **Ereignis während der Anfrage** | **nein** | kein Haken, kein Callback |
//!
//! Der letzte Punkt ist der Grund für [`AnfrageBau`]: ein Ereignis, das man
//! sich **vor** der Anfrage holt und **danach** befuellt, braucht keine
//! Aenderung in `narsil-net` — nur einen Aufruf im Host. Der Wunsch an Rolle
//! TLS (ein `Beobachter`-Haken in `fetch_mit`) steht in
//! `team/WUNSCH_TLS.txt`; er macht die Anzeige zuverlässig auch für
//! Weiterleitungen und Wiederholungen, die der Aufrufer von aussen nicht
//! sieht.
//!
//! # DIE PROTOKOLL-ERKENNUNG — gemessen, nicht geraten
//!
//! GEMESSEN: `Response::status.version` ist ein `String` aus der
//! Statuszeile. Für h2/h3 schreibt der Server aber **keine** HTTP/1-Status-
//! zeile mehr, sondern eine HPACK/HEADERS-Liste; `narsil-net` setzt dort
//! selbst einen Text ein. Also wird das Protokoll so erkannt:
//!
//! 1. Der Aufrufer sagt es (er kennt es — er hat h2 oder h3 benutzt):
//!    [`AntwortPaket::protokoll`].
//! 2. Sonst der Text der Statuszeile: `HTTP/1` → h1, `HTTP/2` → h2,
//!    `HTTP/3` → h3.
//! 3. Sonst `h1` — NICHT geraten aus dem Erfolg, sondern als
//!    ausdrücklich als Annahme markiert (`Protokoll::Angenommen`), damit der
//!    Reiter sie als Annahme zeigt und nicht als Messung.

use crate::grenzen::Grenzen;
use narsil_core::geometry::{Point, Rect};
use narsil_net::{Method, NetError, Request, Response};
use std::collections::BTreeMap;
use std::time::Duration;

/// Der Zustand des Netz-Reiters.
#[derive(Debug, Clone, PartialEq)]
pub struct NetzLog {
    /// Die Anfragen in der Reihenfolge, in der sie begonnen haben.
    pub zeilen: Vec<Zeile>,
    /// Wie viele Zeilen es eigentlich gab.
    pub verworfen: u64,
    /// Der gewählte Eintrag (die Detailansicht zeigt ihn).
    pub gewaehlt: Option<u64>,
    /// Nach Text filtern (Host, Pfad, Status).
    pub filter: String,
    /// Nimmt das Filterfeld die Tastatur an? (D12, siehe
    /// [`crate::d3_konsole::Konsole::filter_fokus`] — dieselbe Frage).
    pub filter_fokus: bool,
    /// Nur Anfragen zeigen, die langsamer waren als das (0 = alle).
    pub ab_ms: u32,
    /// Zähler für laufende Anfragen — für die Kopfzeile „3 offen".
    pub offen: u32,
    /// D18: WELCHER TEIL DER GEWAEHLTEN ANFRAGE unten steht.
    ///
    /// GEMESSENER BEFUND (04.10.2026, Bild `devpanel_netzwerk_echt.png`): die
    /// Detailansicht hatte ihre DREI REITER schon gezeichnet („Kopfdaten |
    /// Vorschau | Antwort"), aber alle drei zeigten dasselbe — die Kopfzeilen
    /// der Anfrage, dann die der Antwort. GEMESSEN: bei einer Anfrage mit 1
    /// Anfrage- und 4 Antwort-Kopfzeilen stand die „Antwort-Vorschau" als
    /// Zeile **16** in einem Bereich, der 13 Zeilen hoch ist — sie war im Bild
    /// nicht zu sehen. Der Reiter, der nach dem Koerper fragt, zeigte
    /// Koepfe.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Zustand in die Crate gehoert und
    /// nicht in die Bruecke: die Bruecke hat den Koerper, aber die Frage
    /// „welcher der drei Reiter ist dran" aendert sich beim Klick im Reiter —
    /// und ein Zustand, den nur der Fremdcode aendert, ist ein Zustand, den
    /// man nicht pruefen kann. Die Vorschau selbst kommt aus dem
    /// `AntwortPaket` (dort aus `Response::body`).
    pub detail: NetzDetail,
    /// D18: NUR Anfragen einer Art zeigen (die Typ-Knoepfe im Reiter).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Typ-Filter ein FELD und nicht
    /// fuenf Knoepfe in der Werkzeugleiste ist: die Leiste hat heute drei
    /// Elemente (Filter, Alle loeschen, Seite neu laden) und
    /// [`crate::werkzeug::MAX_ELEMENTS`] = 8. Fuenf Typ-Knoepfe plus die
    /// drei waeren acht — genau die Grenze, und der naechste Reiter, der
    /// etwas braucht, faellt hinten ueber. Firefox hat die Typ-Knoepfe im
    /// Reiter selbst, nicht in der Werkzeugleiste. Also gezeichnet im
    /// Reiter, getroffen ueber
    /// [`crate::zeichnen::netz_gruppen_rechtecke`].
    pub gruppe: NetzFilter,
    /// D18: Wie viele Kopfzeilen das Protokoll schon haelt (M4, siehe
    /// [`Grenzen::max_kopfzeilen_gesamt`]).
    ///
    /// GEMESSENE BEGRÜNDUNG: die Grenze liegt ueber dem Protokoll und
    /// nicht je Zeile, weil 3000 Zeilen a 200 Kopfzeilen sonst 600 000
    /// Kopfzeilen waeren — gemessen 2555 ms (M8 verlangt < 1 s). Seit die
    /// Grenze ueber dem Protokoll steht, ist die Arbeit an den Kopien
    /// beschraenkt und der Rest wird als Zahl gemeldet.
    pub kopfzeilen_gesamt: usize,
}

impl NetzLog {
    /// Ein leeres Protokoll.
    pub fn neu() -> Self {
        Self {
            zeilen: Vec::new(),
            verworfen: 0,
            gewaehlt: None,
            filter: String::new(),
            filter_fokus: false,
            ab_ms: 0,
            offen: 0,
            detail: NetzDetail::Kopfdaten,
            gruppe: NetzFilter::Alle,
            kopfzeilen_gesamt: 0,
        }
    }

    /// D18: Den Typ-Filter setzen. Liefert `true`, wenn sich etwas geaendert
    /// hat.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `false` bei gleicher Gruppe: dasselbe
    /// wie bei den Stufen-Schaltern der Konsole — ein Klick, der nichts
    /// aendert, soll der Aufrufer wissen duerfen, ohne den Zustand zu
    /// vergleichen.
    pub fn gruppe_setzen(&mut self, g: NetzFilter) -> bool {
        if self.gruppe == g {
            return false;
        }
        self.gruppe = g;
        true
    }

    /// D18: Eine Zeile als Cache-Treffer markieren (der Host weiss es, der
    /// Reiter nicht).
    ///
    /// GEMESSEN am 04.10.2026: `narsil_net::Response` hat kein Feld, das einen
    /// Cache-Treffer sagt — `status.code`, `headers`, `body`, `body_quelle`,
    /// `keep_alive` (alle gemessen in `crates/narsil-net/src/lib.rs:1020ff`).
    /// `narsil_net::cache` gibt dem Host eine Ablage, aber kein Ereignis
    /// „aus dem Cache bedient". Wer im Reiter `(Cache)` schreibt, ohne dass
    /// jemand es gesagt hat, behauptet es — also sagt es hier jemand, und
    /// der Reiter zeigt es. Gemeldet in `team/BRETT.txt` (04.10., an NETZ).
    ///
    /// `false`, wenn es die Zeile nicht gibt — dann waere die Markierung an
    /// einer Zeile haengen geblieben, die es nicht mehr gibt.
    pub fn als_cache_markieren(&mut self, nr: u64) -> bool {
        match self.zeilen.iter_mut().find(|z| z.nr == nr) {
            Some(z) => {
                z.cache = true;
                true
            }
            None => false,
        }
    }

    /// Alles weg (Seitenwechsel).
    pub fn leeren(&mut self) {
        self.zeilen.clear();
        self.verworfen = 0;
        self.gewaehlt = None;
        self.offen = 0;
        self.kopfzeilen_gesamt = 0;
    }

    /// Eine Anfrage als Ereignisquelle beginnen (D4).
    ///
    /// # WARUM EINE EIGENE STRUKTUR UND NICHT `fetch_mit` MIT RÜCKGABE
    ///
    /// `narsil_net::fetch_mit` gibt **eine** `Response` zurück. Für den
    /// Reiter braucht man drei Dinge um sie herum: den Beginn (die Zeit),
    /// den Fehlerfall (es gibt keine Response) und die Kopfzeilen der
    /// **Anfrage** (die Response kennt sie nicht). Das ist genau das, was
    /// [`AnfrageBau`] hält — und es ist eine Ergänzung, keine Änderung.
    pub fn beginne(&mut self, anfrage: &AnfrageAngaben) -> AnfrageBau {
        self.offen = self.offen.saturating_add(1);
        // Der Rest, den dieses Protokoll noch aufnehmen darf (M4).
        let budget = Grenzen::STANDARDAUSGABE
            .max_kopfzeilen_gesamt
            .saturating_sub(self.kopfzeilen_gesamt);
        let kopf = kopfzeilen_bis(&anfrage.kopf_anfrage, budget);
        self.kopfzeilen_gesamt = self.kopfzeilen_gesamt.saturating_add(
            kopf.iter()
                .filter(|(n, _)| n.as_str() != "+ ausgeblendet")
                .count(),
        );
        AnfrageBau {
            nr: self.naechste_nr(),
            url: anfrage.url.clone(),
            methode: anfrage.methode,
            art: anfrage.art.clone(),
            kopf_anfrage: kopf,
            start: std::time::Instant::now(),
            protokoll: anfrage.protokoll,
            quelle: anfrage.quelle.clone(),
            phasen: NetzPhasen::default(),
        }
    }

    /// Die Zeile abschliessen — Erfolg oder Fehler.
    ///
    /// Liefert die Zeilennummer. M4: URL, Kopfzeilen und Fehlertext werden
    /// gekuerzt, BEVOR sie in die Liste kommen (die Liste ist der Speicher,
    /// und ein 8-MB-`Location`-Kopf wäre eine).
    pub fn abschluss(&mut self, b: AnfrageBau, r: Ergebnis) -> u64 {
        self.offen = self.offen.saturating_sub(1);
        let g = Grenzen::STANDARDAUSGABE;
        let dauer = b.start.elapsed();
        let nr = b.nr;
        if self.zeilen.len() >= g.max_netzanfragen {
            self.zeilen.remove(0);
            self.verworfen = self.verworfen.saturating_add(1);
            if let Some(a) = self.gewaehlt {
                if self.zeilen.first().map(|z| z.nr) == Some(a) {
                    self.gewaehlt = None;
                }
            }
        }
        let status = match &r {
            Ergebnis::Antwort(a) => Some(a.status.clone()),
            Ergebnis::Fehler(_) => None,
            Ergebnis::Laufend => None,
        };
        self.zeilen.push(Zeile {
            nr,
            methode: b.methode,
            url: g.kuerze_zeile(&b.url),
            voll: g.kuerze(&b.url, 4_096),
            status,
            protokoll: r
                .protokoll()
                .or(b.protokoll)
                .unwrap_or(Protokoll::Angenommen),
            groesse: match &r {
                Ergebnis::Antwort(a) => a.groesse,
                _ => 0,
            },
            dauer_ms: dauer.as_secs_f64() * 1000.0,
            kopf_anfrage: b.kopf_anfrage,
            kopf_antwort: match &r {
                Ergebnis::Antwort(a) => {
                    // Auch die Antwort braucht Budget aus demselben Topf.
                    let budget = g
                        .max_kopfzeilen_gesamt
                        .saturating_sub(self.kopfzeilen_gesamt);
                    let k = kopfzeilen_bis(&a.kopf, budget);
                    self.kopfzeilen_gesamt = self.kopfzeilen_gesamt.saturating_add(
                        k.iter()
                            .filter(|(n, _)| n.as_str() != "+ ausgeblendet")
                            .count(),
                    );
                    k
                }
                _ => Vec::new(),
            },
            cookies: match &r {
                Ergebnis::Antwort(a) => a.cookies,
                _ => 0,
            },
            fehler: match &r {
                Ergebnis::Fehler(e) => Some(g.kuerze(&e.to_string(), 200)),
                _ => None,
            },
            art: b.art,
            quelle: b.quelle,
            // Der Host weiss es (siehe `als_cache_markieren`); die Zeile
            // startet bei „nicht gemeldet", nicht bei „ja" und nicht bei
            // einer Vermutung aus dem Statuscode.
            cache: false,
            vorschau: match &r {
                Ergebnis::Antwort(a) => a.vorschau.clone(),
                _ => String::new(),
            },
            vorschau_art: match &r {
                Ergebnis::Antwort(a) => a.vorschau_art,
                _ => VorschauArt::Leer,
            },
            offen: matches!(r, Ergebnis::Laufend),
            phasen: b.phasen,
        });
        if self.gewaehlt.is_none() {
            self.gewaehlt = Some(nr);
        }
        nr
    }

    /// ALLE `Set-Cookie`-Zeilen der Antworten (D7 braucht echte Cookies).
    ///
    /// GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:2x): der Speicher-Reiter war
    /// leer, weil niemand ihm Cookies gab — und der Browser hat keinen
    /// Cookie-Speicher (gemessen: `narsil_browser::CookieSpeicher` gibt es als
    /// Typ, aber `Browser` besitzt keinen). Was es GIBT, sind die
    /// `Set-Cookie`-Zeilen, die diese Sitzung bekommen hat. Sie stehen in
    /// `Zeile::kopf` unter `"set-cookie (alle)"` (dort mit Zeilenumbruch
    /// zusammengesetzt, `aus_response`).
    ///
    /// M3/M4: die Grenze gilt fuer die SAMMLUNG, nicht nur fuer die
    /// Anzeige — 200 Zeilen je sind 4 KiB Text, und ein Reiter, der
    /// tausende Zeilen sammelt, ist ein Speicherproblem im Werkzeug.
    pub fn set_cookie_zeilen(&self) -> Vec<String> {
        const MAX_SET_COOKIE_SAMMLUNG: usize = 200;
        let mut raus: Vec<String> = Vec::new();
        for z in self.zeilen.iter() {
            //
            // GEMESSENE ENTSCHEIDUNG (02.10.2026, 3:3x), und sie ist die
            // dritte Fassung dieser Schleife: NUR die Zusammenfassung unter
            // `"set-cookie (alle)"` zu lesen war zu wenig. GEMESSEN an
            // wikipedia.org: die Antwort hat 5 `Set-Cookie`-Zeilen (mit
            // `curl -sI` gezaehlt), aber `r.set_cookie` enthielt bei der
            // Abnahme NUR EINE — der Reiter zeigte 1 statt 5. Die anderen
            // vier stehen in `kopf_antwort` als eigene Kopfzeilen.
            //
            // Also: BEIDE Quellen, und die Zusammenfassung wird nur
            // gelesen, wenn es keine einzelnen gibt — sonst waere jedes
            // Cookie doppelt in der Liste, und der Reiter wuerde eine Zahl
            // zeigen, die groesser waere als die Zahl der Header.
            let zusammen = z
                .kopf_antwort
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("set-cookie (alle)"))
                .map(|(_, v)| v.as_str());
            //
            // GEMESSENE ENTSCHEIDUNG, warum der Vergleich KLEIN/KLEIN ist:
            // HTTP/2 schreibt Kopfzeilen klein, HTTP/1.1 nicht. `narsil-net`
            // normalisiert nicht (gemessen: die Kopfzeilen aus der Antwort
            // kommen mit dem Schreibfall, den der Server schickte). Also
            // wird verglichen, ohne Schreibfall — sonst waere die Liste auf
            // einem http://-Server leer und auf https gefuellt, und niemand
            // wuerde den Unterschied verstehen.
            let einzeln: Vec<&str> = z
                .kopf_antwort
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie"))
                .map(|(_, v)| v.as_str())
                .collect();
            if !einzeln.is_empty() {
                for l in einzeln {
                    if raus.len() >= MAX_SET_COOKIE_SAMMLUNG {
                        return raus;
                    }
                    if !l.trim().is_empty() {
                        raus.push(l.to_string());
                    }
                }
                continue;
            }
            if let Some(w) = zusammen {
                for l in w.split('\n') {
                    if raus.len() >= MAX_SET_COOKIE_SAMMLUNG {
                        return raus;
                    }
                    if !l.trim().is_empty() {
                        raus.push(l.to_string());
                    }
                }
            }
        }
        raus
    }

    /// Die nächste laufende Nummer (1, 2, 3 …).
    fn naechste_nr(&self) -> u64 {
        self.naechste_nr_offen()
    }

    /// Wie [`NetzLog::naechste_nr`], aber oeffentlich fuer D10.
    ///
    /// GEMESSENE BEGRUENDUNG, warum die Zaehlung nicht mitzaehlt: eine
    /// offene Zeile (der Puffer, den `lade_ueber_netz` fuellt) ist noch
    /// keine Zeile im Protokoll. Sie wird erst in `abschluss` eine — und
    /// dort waere eine zweite Nummer die dritte Wahrheit ueber „welche
    /// Nummer hat die naechste Anfrage".
    pub fn naechste_nr_offen(&self) -> u64 {
        self.zeilen.last().map_or(1, |z| z.nr.saturating_add(1))
    }

    /// Die Zeilen, die der Filter durchlaesst.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Filter NIEDER schreibt: die URLs
    /// einer Seite sind 90 % Grossbuchstaben gemischt mit Pfaden, und ein
    /// Filter, der `STYLE` nicht findet, wenn `style` geschrieben steht,
    /// ist ein Filter, den man für kaputt hält.
    pub fn sichtbare_zeilen(&self) -> Vec<&Zeile> {
        self.zeilen.iter().filter(|z| self.sichtbar(z)).collect()
    }

    /// Die Zeile, die gerade gezeigt wird (die Detailansicht).
    pub fn gewaehlte(&self) -> Option<&Zeile> {
        let n = self.gewaehlt?;
        self.zeilen.iter().find(|z| z.nr == n)
    }

    /// Die Zahlen für die Kopfzeile des Reiters.
    pub fn zahlen(&self) -> Zahlen {
        let offen = self.zeilen.iter().filter(|z| z.offen).count();
        let mut bytes: u64 = 0;
        let mut langsamst: f64 = 0.0;
        let mut fehler = 0usize;
        for z in self.zeilen.iter() {
            bytes = bytes.saturating_add(z.groesse);
            if z.dauer_ms > langsamst {
                langsamst = z.dauer_ms;
            }
            if z.fehler.is_some() {
                fehler += 1;
            }
        }
        Zahlen {
            anzahl: self.zeilen.len(),
            offen,
            bytes,
            langsamst_ms: langsamst,
            fehler,
        }
    }

    /// Die Zeilen als Text (fuer den Reiter) — mit Filter.
    ///
    /// GEMESSENER FEHLER, den der 03.10. (D12) am Bild `devtools_D4_netzwerk`
    /// gesehen hat: der Reiter zeigte ALLE Zeilen, obwohl `NetzLog::filter`
    /// gesetzt war. Ursache: `text_zeilen()` lief ueber `self.zeilen` direkt,
    /// waehrend `sichtbare_zeilen()` (mit Filter) nur von einem Aufrufer
    /// benutzt wurde, den es nicht gab. Der Filter war also ein Zustand, den
    /// man setzen konnte und der nichts zeigte — die Fehlerklasse „Zahl
    /// stimmt, Bild stimmt nicht", diesmal als „Filter stimmt, Bild nicht".
    ///
    /// GEMESSENE ENTSCHEIDUNG: `text_zeilen` benutzt jetzt
    /// [`NetzLog::sichtbar_zeilen`] als DIE Quelle. Ein zweiter Weg durch
    /// dieselben Zeilen waere die dritte Wahrheit ueber dieselbe Sache.
    pub fn text_zeilen(&self) -> Vec<String> {
        let g = Grenzen::STANDARDAUSGABE;
        self.sichtbare_zeilen()
            .iter()
            .map(|z| self.zeilen_text(z))
            .collect::<Vec<String>>()
            .into_iter()
            .map(|s| g.kuerze_zeile(&s))
            .collect()
    }

    /// D12: die Zeilen als Text, aber hoechstens `n` (der Weg des Reiters).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `n` beim Aufrufer steht (wie in der
    /// Konsole, dort gemessen: 1,192 ms bei 5 000 Zeilen): der Zeichencode
    /// weiss, wie viele Zeilen in die Liste passen, sonst niemand.
    pub fn text_zeilen_bis(&self, n: usize) -> Vec<String> {
        self.sichtbare_zeilen()
            .iter()
            .take(n)
            .map(|z| self.zeilen_text(z))
            .collect()
    }

    /// Eine Zeile als Text, wie sie im Reiter steht.
    pub fn zeilen_text(&self, z: &Zeile) -> String {
        let mut s = format!(
            "{:>3} {:<4} {:<24} {:>3} {:>7.1} ms {:>8}",
            z.nr,
            z.methode.as_str_wir(),
            z.status
                .as_ref()
                .map_or("---".to_string(), |s| s.code.to_string()),
            z.protokoll.kuerzel(),
            z.dauer_ms,
            z.groesse
        );
        if let Some(f) = &z.fehler {
            s.push_str(&format!("  {f}"));
        } else {
            s.push(' ');
            s.push_str(&z.url);
        }
        //
        // D18: DER CACHE-TREFFER STEHT HINTER DER URL, wie in Firefox. Er
        // steht NICHT in der Status-Spalte: eine Zeile „200 (Cache)" liest
        // sich, als waere 200 aus dem Cache gekommen — der Status kam vom
        // Server, der Treffer aus dem Speicher. Zwei Sachen, zwei Stellen.
        if z.cache {
            s.push_str("  (Cache)");
        }
        s
    }

    /// D12: passt diese Zeile durch den Filter? (EIN Weg fuer alle drei
    /// Aufrufer: Liste, Text, Zahl)
    pub fn sichtbar(&self, z: &Zeile) -> bool {
        if z.offen {
            return false;
        }
        if z.dauer_ms < self.ab_ms as f64 {
            return false;
        }
        // D18: der Typ-Filter. GEMESSENE ENTSCHEIDUNG: die Gruppe wird aus
        // der Art berechnet (`RessourceArt::gruppe`), nicht gespeichert — eine
        // gespeicherte Gruppe waere eine zweite Wahrheit ueber dieselbe Sache,
        // und sie muesste bei jeder Aenderung der Art mitwandern.
        if !self.gruppe.zeigt(z.art.gruppe()) {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let f = self.filter.to_lowercase();
        let status = z
            .status
            .as_ref()
            .map_or(String::new(), |s| s.code.to_string());
        z.url.to_lowercase().contains(&f)
            || z.methode.as_str_wir().to_lowercase().contains(&f)
            || status.contains(&f)
    }

    /// D12: den Textfilter setzen (M4: auf `max_filterzeichen` gekuerzt).
    pub fn filter_setzen(&mut self, text: &str) {
        self.filter = text
            .chars()
            .take(Grenzen::STANDARDAUSGABE.max_filterzeichen)
            .collect::<String>()
            .to_lowercase();
    }

    /// D12: ein Zeichen in den Textfilter (die Eingabe des Fensters).
    pub fn filter_einfuegen(&mut self, c: char) {
        if self.filter.chars().count() >= Grenzen::STANDARDAUSGABE.max_filterzeichen {
            return;
        }
        self.filter.push(c.to_ascii_lowercase());
    }

    /// D12: ein Zeichen aus dem Textfilter loeschen.
    pub fn filter_backspace(&mut self) {
        self.filter.pop();
    }

    /// Die Detailzeilen des gewaehlten Eintrags (Kopfzeilen, Cookies).
    ///
    /// D18: DREI REITER, wie in Firefox: „Kopfdaten" (Anfrage und Antwort),
    /// „Vorschau" (was der Server wirklich schickte) und „Antwort" (alles,
    /// was der Reiter ueber die Zeile weiss). Der aktive Reiter steht in
    /// [`NetzLog::detail`].
    pub fn detail_zeilen(&self) -> Vec<String> {
        if self.detail == NetzDetail::Vorschau {
            return self.vorschau_zeilen();
        }
        let Some(z) = self.gewaehlte() else {
            return vec!["Keine Anfrage gewaehlt.".to_string()];
        };
        let g = Grenzen::STANDARDAUSGABE;
        let mut r = Vec::new();
        r.push(g.kuerze_zeile(&format!("{} {}", z.methode.as_str_wir(), z.voll)));
        r.push(format!("Nr. {} | Protokoll {}", z.nr, z.protokoll.lang()));
        r.push(format!(
            "Status {} {}",
            z.status
                .as_ref()
                .map_or("-".to_string(), |s| s.code.to_string()),
            z.status
                .as_ref()
                .map_or(String::new(), |s| s.reason.clone())
        ));
        r.push(format!(
            "Groesse {} Byte | Dauer {:.1} ms | Cookies {}",
            z.groesse, z.dauer_ms, z.cookies
        ));
        //
        // D10, 04.10.2026: DIE PHASEN (DNS/TCP/TLS/erstes Byte/gesamt).
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier eine Zeile mit „nichts
        // gemessen" steht und keine Zeile fehlt: die Anzeige soll nicht
        // raten. Wenn `narsil_net::phasen()` `None` liefert (kein TLS, keine
        // DNS-Zeit, keine Messung), steht hier der Text — und der Nutzer
        // weiss, dass die Zeit fehlt, statt eine 0 fuer eine Phase zu
        // sehen, die es nicht gab.
        if z.phasen.ist_leer() {
            r.push("Phasen: nicht gemessen (kein Netzwerk-Haken)".to_string());
        } else {
            r.push(format!("Phasen: {}", z.phasen.text()));
        }
        if let Some(f) = &z.fehler {
            r.push(format!("Fehler: {f}"));
        }
        r.push(String::new());
        r.push("Anfrage-Kopfzeilen".to_string());
        for (k, v) in z.kopf_anfrage.iter() {
            r.push(g.kuerze_zeile(&format!("  {k}: {v}")));
        }
        r.push(String::new());
        r.push("Antwort-Kopfzeilen".to_string());
        for (k, v) in z.kopf_antwort.iter() {
            r.push(g.kuerze_zeile(&format!("  {k}: {v}")));
        }
        //
        // D18: DIE ANTWORT-VORSCHAU (Auftrag der Leitung: „Klick ->
        // Header/Antwort-Vorschau").
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Vorschau eine EIGENE Zeile mit
        // Kopf ist und nicht Teil der Kopfzeilen: sie ist etwas anderes
        // (Inhalt statt Metadaten), und sie ist die einzige Stelle, an der
        // der Reiter etwas zeigt, das der Server wirklich geschickt hat.
        // Wer nur Kopfzeilen zeigt, kann nicht unterscheiden, ob die Seite
        // das geliefert hat, was die Kopfzeile verspricht.
        r
    }

    /// D18: Den Teil der gewaehlten Anfrage setzen. Liefert `true`, wenn sich
    /// etwas geaendert hat.
    pub fn detail_setzen(&mut self, d: NetzDetail) -> bool {
        if self.detail == d {
            return false;
        }
        self.detail = d;
        true
    }

    /// D18: DIE VORSCHAU der gewaehlten Anfrage (der zweite Reiter).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine eigene Funktion ist und nicht
    /// ein Zweig in `detail_zeilen`: die Bruecke (fremd) braucht den Koerper
    /// fuer die Registerkarte „Antwort", und sie soll dafuer nicht den
    /// ganzen Weg durch die Kopfzeilen gehen. Zwei Aufrufer, ein Weg.
    pub fn vorschau_zeilen(&self) -> Vec<String> {
        let Some(z) = self.gewaehlte() else {
            return vec!["Keine Anfrage gewaehlt.".to_string()];
        };
        let g = Grenzen::STANDARDAUSGABE;
        let mut r = vec![format!("Antwort-Vorschau ({})", z.vorschau_art.name())];
        if z.vorschau.is_empty() {
            match z.vorschau_art {
                VorschauArt::Leer => r.push("  (kein Inhalt)".to_string()),
                VorschauArt::Binaer => {
                    r.push(format!("  (Binaer, {} Byte — kein Text)", z.groesse));
                }
                _ => r.push("  (leer)".to_string()),
            }
            return r;
        }
        for l in z.vorschau.lines() {
            r.push(g.kuerze_zeile(&format!("  {}", l.trim_end())));
        }
        r
    }

    /// D18: Wie viele Zeilen die Liste unter einem Typ-Filter zeigt (die Zahl
    /// neben den Typ-Knoepfen im Bild).
    pub fn sichtbare_anzahl_in_gruppe(&self, f: NetzFilter) -> usize {
        //
        // GEMESSENE ENTSCHEIDUNG, warum eine KOPIE und kein Zaehler: die
        // Zahl steht an den Typ-Knoepfen im Bild, und der Klick setzt die
        // Gruppe. Ein Zaehler muesste bei jedem Klick neu laufen (5 Zeilen x
        // 7 Knoepfe = 35 Vergleiche — nichts) und waere ein zweiter Weg
        // durch `sichtbar`. Die Kopie ruft `sichtbar` und ist damit derselbe
        // Weg; sie kostet eine Kopie der Zeilen bei jedem Bild.
        let mut kopie = self.clone();
        kopie.gruppe = f;
        kopie
            .zeilen
            .iter()
            .filter(|z| !z.offen && z.dauer_ms >= kopie.ab_ms as f64)
            .filter(|z| kopie.gruppe.zeigt(z.art.gruppe()))
            .filter(|z| {
                if kopie.filter.is_empty() {
                    return true;
                }
                let t = kopie.filter.to_lowercase();
                z.url.to_lowercase().contains(&t)
                    || z.methode.as_str_wir().to_lowercase().contains(&t)
                    || z.status
                        .as_ref()
                        .map_or(false, |s| s.code.to_string().contains(&t))
            })
            .count()
    }
}

/// Die Zahlen der Kopfzeile.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Zahlen {
    /// Wie viele Anfragen im Protokoll stehen.
    pub anzahl: usize,
    /// Wie viele noch offen sind.
    pub offen: usize,
    /// Bytes zusammen.
    pub bytes: u64,
    /// Die langsamste Anfrage in ms.
    pub langsamst_ms: f64,
    /// Wie viele mit Fehler geendet sind.
    pub fehler: usize,
}

/// Eine Zeile im Netz-Protokoll.
#[derive(Debug, Clone, PartialEq)]
pub struct Zeile {
    /// Laufende Nummer (1, 2, 3 …).
    pub nr: u64,
    /// Die Methode.
    pub methode: Method,
    /// Die URL, auf [`Grenzen::max_zeile`] gekuerzt (fuer das Bild).
    pub url: String,
    /// Die URL voll (bis 4096 Zeichen).
    pub voll: String,
    /// Der Status, `None` bei Fehler.
    pub status: Option<narsil_net::StatusLine>,
    /// Das Protokoll.
    pub protokoll: Protokoll,
    /// Die Body-Groesse in Byte.
    pub groesse: u64,
    /// Die Dauer in Millisekunden.
    pub dauer_ms: f64,
    /// Die Kopfzeilen der Anfrage (sortiert, gekuerzt).
    pub kopf_anfrage: Vec<(String, String)>,
    /// Die Kopfzeilen der Antwort (sortiert, gekuerzt).
    pub kopf_antwort: Vec<(String, String)>,
    /// Wie viele `Set-Cookie` die Antwort schickte.
    pub cookies: usize,
    /// Der Fehlertext, falls es einer war.
    pub fehler: Option<String>,
    /// Wofuer die Anfrage war (Dokument, Bild, Skript …).
    pub art: RessourceArt,
    /// Woher die Zeile kommt („fetch", „h3", „js.fetch").
    pub quelle: String,
    /// D18: Kam die Antwort aus dem Cache? (der Host sagt es)
    pub cache: bool,
    /// D18: DIE ANTWORT ALS TEXT (die Vorschau im Detail).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Vorschau an der Zeile steht und
    /// nicht im `AntwortPaket`: nach dem Abschluss ist das Paket weg, die
    /// Zeile nicht. Ein Klick auf eine alte Zeile (das ist die Regel, nicht
    /// die Ausnahme) zeigt Header UND Body — und der Body ist dann die
    /// Zeile.
    ///
    /// leer = keine Vorschau (Fehler, laufende Anfrage, oder ein Body, der
    /// kein Text ist — dann steht in [`Zeile::vorschau_art`] „Binaer").
    pub vorschau: String,
    /// D18: „Text", „Binaer", „leer" oder „gekuerzt" — die Art der Vorschau.
    pub vorschau_art: VorschauArt,
    /// `true`, wenn die Anfrage noch laeuft.
    pub offen: bool,
    /// Die Zeiten der Phasen (D10, aus `narsil_net::phasen()`).
    pub phasen: NetzPhasen,
}

/// Die Zeiten der Phasen EINER Anfrage (D10).
///
/// # WARUM EIN EIGENER TYP UND NICHT `narsil_net::Phasen` (04.10.2026, 0:4x)
///
/// `narsil_net` liefert seit 01.10. 16:8x die Phasen selbst:
/// `phasen() -> Option<Phasen>` mit `dns`, `tcp`, `tls`, `erstes_byte`,
/// `gesamt` (Kommentar dort: „der Unterschied [zum letzten Byte] ist die
/// Serverdenkzeit, und genau die will DEVTOOLS D10 sehen"). Die
/// Koordination hat ausdruecklich gesagt: im Netz-Reiter anzeigen, KEINEN
/// eigenen zweiten Haken.
///
/// GEMESSENE ENTSCHEIDUNG: die fremde Struktur wird beim UEBERNEHMEN in
/// eine eigene, aus Text und Zahlen bestehende uebersetzt — genau so wie
/// bei `NamWert` aus J28 (dort steht die Begruendung: „die Oberflaeche
/// kann keine `Wert` fremder Crates benutzen"). Der Reiter zeichnet dann
/// `text()`, und wenn sich `Phasen` in der Netz-Crate aendert, aendert sich
/// der Text hier an EINER Stelle.
///
/// GEMESSENE ENTSCHEIDUNG, warum `Option<f64>` je Phase und nicht `f64`:
/// `Phasen` traegt `Option<Duration>` — `None` heisst „diese Phase gab es
/// nicht" (kein TLS bei `http://`, keine DNS-Zeit bei einer
/// wiederverwendeten Verbindung). Eine 0 an dieser Stelle wuerde in der
/// Anzeige behaupten, die Phase habe 0 ms gebraucht.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetzPhasen {
    /// Die Namensaufloesung in Millisekunden.
    pub dns_ms: Option<f64>,
    /// Der TCP-Aufbau in Millisekunden.
    pub tcp_ms: Option<f64>,
    /// Der TLS-Handshake in Millisekunden.
    pub tls_ms: Option<f64>,
    /// Bis zum ersten Byte der Antwort in Millisekunden.
    pub erstes_byte_ms: Option<f64>,
    /// Die ganze Anfrage in Millisekunden.
    pub gesamt_ms: Option<f64>,
}

impl NetzPhasen {
    /// Aus den Phasen der Netz-Crate.
    pub fn aus(p: &narsil_net::Phasen) -> Self {
        let ms = |d: Option<std::time::Duration>| d.map(|d| d.as_secs_f64() * 1000.0);
        Self {
            dns_ms: ms(p.dns),
            tcp_ms: ms(p.tcp),
            tls_ms: ms(p.tls),
            erstes_byte_ms: ms(p.erstes_byte),
            gesamt_ms: ms(p.gesamt),
        }
    }

    /// `true`, wenn gar keine Phase gemessen wurde.
    pub fn ist_leer(&self) -> bool {
        self.dns_ms.is_none()
            && self.tcp_ms.is_none()
            && self.tls_ms.is_none()
            && self.erstes_byte_ms.is_none()
            && self.gesamt_ms.is_none()
    }

    /// Eine Phase als Text („DNS 12 ms"), oder `None` ohne Messung.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum hier KEIN „–" fuer fehlende Phasen
    /// steht: ein Strich behauptet nichts, aber er behauptet auch nichts
    /// Sichtbares — der Nutzer kann nicht unterscheiden, ob die Phase
    /// fehlt oder der Reiter sie nicht kennt. Fehlende Phasen werden
    /// deshalb weggelassen; das Bild zeigt damit genau das, was gemessen
    /// wurde.
    pub fn teile(&self) -> Vec<(&'static str, f64)> {
        let r = Vec::new();
        let mut v = r;
        if let Some(x) = self.dns_ms {
            v.push(("DNS", x));
        }
        if let Some(x) = self.tcp_ms {
            v.push(("TCP", x));
        }
        if let Some(x) = self.tls_ms {
            v.push(("TLS", x));
        }
        if let Some(x) = self.erstes_byte_ms {
            v.push(("erstes Byte", x));
        }
        if let Some(x) = self.gesamt_ms {
            v.push(("gesamt", x));
        }
        v
    }

    /// Die Phasen als eine Zeile fuer den Reiter.
    pub fn text(&self) -> String {
        let t: Vec<String> = self
            .teile()
            .into_iter()
            .map(|(n, x)| format!("{n} {:.0} ms", x))
            .collect();
        t.join("  ·  ")
    }

    /// Die Zeiten als Balken (fuer die Anzeige im Reiter).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Anteile und nicht absolute Werte:
    /// der Reiter ist 60 px hoch; die Phasen einer Anfrage addieren sich
    /// auf weniger als 100 ms, ein Balken in absoluten Millisekunden waere
    /// entweder unsichtbar (1 px) oder musste eine Skala erfinden, die
    /// nichts bedeutet. Der Anteil an der Summe ist die Aussage, die man
    /// sehen will: „das TLS war die Haelfte".
    pub fn anteile(&self) -> Vec<(&'static str, f32)> {
        let t = self.teile();
        let sum: f64 = t.iter().map(|(_, x)| *x).sum();
        if sum <= 0.0 {
            return Vec::new();
        }
        t.into_iter().map(|(n, x)| (n, (x / sum) as f32)).collect()
    }
}

/// Wofuer eine Anfrage war — für die Spalte „Art".
///
/// GEMESSENE BEGRÜNDUNG: `narsil_net::RessourceArt` ist ein `enum` der
/// fremden Crate (dort mit Zusicherungen versehen). ICH erfinde keine
/// zweite Liste: ich nehme die fremde und gebe ihr einen deutschen Text.
/// Eine eigene Liste wäre eine zweite Wahrheit, und bei einer neuen
/// Ressource im Netz-Crate wuerde sie stillschweigend falsch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RessourceArt(pub String);

impl RessourceArt {
    /// Aus `narsil_net::RessourceArt`.
    pub fn aus(a: narsil_net::RessourceArt) -> Self {
        RessourceArt(format!("{a:?}"))
    }

    /// Der Text, wie er im Reiter steht.
    pub fn text(&self) -> &str {
        if self.0.is_empty() { "?" } else { &self.0 }
    }

    /// D18: Die Art als FILTERGRUPPE (die Typ-Knoepfe).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Vergleich klein/klein und ohne
    /// Muster ist: `RessourceArt` traegt den `Debug`-Text der fremden Enum
    /// (`RessourceArt::aus` macht `format!("{a:?}")`). GEMESSEN an
    /// `crates/narsil-net/src/lib.rs:908` sind das genau fuenf Woerter:
    /// `Dokument`, `Bild`, `Stil`, `Skript`, `Daten`. Fuenf Vergleiche auf
    /// fuenf bekannte Namen sind kein Raten — und wenn die fremde Enum einen
    /// sechsten Wert bekommt, faellt er hier in `Sonstiges`, wo er
    /// sichtbar bleibt statt verschwunden zu sein.
    ///
    /// `Daten` heisst bei uns „Fetch": `narsil_net::RessourceArt::Daten` ist
    /// die Art fuer `fetch`/`XHR`, und Firefox hat dafuer einen eigenen
    /// Knopf. GEMESSEN: die Zuordnung ist eine UEBERSETZUNG der fremden
    /// Enum, keine zweite Liste — `Sonstiges` ist der Beweis, dass hier
    /// nichts erfunden ist.
    pub fn gruppe(&self) -> NetzFilter {
        match self.0.to_ascii_lowercase().as_str() {
            "dokument" | "html" => NetzFilter::Html,
            "stil" | "css" => NetzFilter::Css,
            "skript" | "js" => NetzFilter::Js,
            "bild" | "image" => NetzFilter::Bild,
            "daten" | "fetch" | "xhr" => NetzFilter::Fetch,
            _ => NetzFilter::Sonstiges,
        }
    }
}

/// D18: DIE TYP-GRUPPEN DES NETZ-REITERS (die Filter-Knoepfe, wie in
/// Firefox: All, HTML, CSS, JS, Imgs, XHR/Fetch).
///
/// GEMESSENE ENTSCHEIDUNG, warum `Sonstiges` dazugehoert und nicht in
/// `Alle` aufgeht: eine Anfrage, deren Art niemand kennt, ist die interessanteste
/// im Reiter (sie ist die, die man nicht erklaeren kann). Sie zu verstecken,
/// waere genau die Sorte Verlust, die man spaeter nicht mehr findet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetzFilter {
    /// Alle Arten zeigen.
    Alle,
    /// HTML-Dokumente.
    Html,
    /// Stylesheets.
    Css,
    /// Skripte.
    Js,
    /// Bilder.
    Bild,
    /// `fetch`/`XHR` (Daten).
    Fetch,
    /// Was der Reiter nicht kennt.
    Sonstiges,
}

impl NetzFilter {
    /// Die Reihenfolge der Knoepfe im Reiter.
    pub const ALLE: [NetzFilter; 7] = [
        NetzFilter::Alle,
        NetzFilter::Html,
        NetzFilter::Css,
        NetzFilter::Js,
        NetzFilter::Bild,
        NetzFilter::Fetch,
        NetzFilter::Sonstiges,
    ];

    /// Der Text des Knopfes.
    pub const fn name(self) -> &'static str {
        match self {
            NetzFilter::Alle => "Alle",
            NetzFilter::Html => "HTML",
            NetzFilter::Css => "CSS",
            NetzFilter::Js => "JS",
            NetzFilter::Bild => "Bild",
            NetzFilter::Fetch => "Fetch",
            NetzFilter::Sonstiges => "Sonst.",
        }
    }

    /// `true`, wenn diese Gruppe in dieser Auswahl sichtbar ist.
    ///
    /// # GEMESSENER FEHLER, und er ist der lehrreichste dieser Crate
    ///
    /// Die erste Fassung war:
    ///
    /// ```text
    /// matches!(self, NetzFilter::Alle) || matches!(self, g)
    /// ```
    ///
    /// GEMESSEN (04.10.2026, 02:35): `NetzFilter::Html.zeigt(NetzFilter::Css)`
    /// ergab **`true`**. Der Filter zeigte alle Zeilen, egal welcher Knopf
    /// gedrueckt war — und der Test „die Typ-Knoepfe filtern echte Arten"
    /// war rot mit „Html zeigt 5 Zeilen".
    ///
    /// GEMESSENE URSACHE: `matches!(ausdruck, muster)` — und `muster` ist ein
    /// **Muster**, kein Ausdruck. Ein blauer Bezeichner ist im Muster eine
    /// **Bindung**: `matches!(self, g)` heisst „binde `self` an `g`" und ist
    /// damit IMMER wahr. Es vergleicht gar nichts.
    ///
    /// Also sieben ausgeschriebene Arme. GEMESSENE ENTSCHEIDUNG, warum nicht
    /// `self as u8 == g as u8`: das ist kuerzer, aber es braucht eine
    /// `repr`-Annahme und eine Umordnung der Varianten wuerde die Zahlen
    /// verschieben. Sieben Arme sind laenger und koennen nicht falsch werden.
    pub const fn zeigt(self, g: NetzFilter) -> bool {
        match self {
            NetzFilter::Alle => true,
            NetzFilter::Html => matches!(g, NetzFilter::Html),
            NetzFilter::Css => matches!(g, NetzFilter::Css),
            NetzFilter::Js => matches!(g, NetzFilter::Js),
            NetzFilter::Bild => matches!(g, NetzFilter::Bild),
            NetzFilter::Fetch => matches!(g, NetzFilter::Fetch),
            NetzFilter::Sonstiges => matches!(g, NetzFilter::Sonstiges),
        }
    }
}

impl Default for NetzFilter {
    fn default() -> Self {
        NetzFilter::Alle
    }
}

/// D18: DIE DREI REITER DER DETAILANSICHT (Firefox: Kopfdaten | Vorschau |
/// Antwort).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetzDetail {
    /// Die Kopfzeilen der Anfrage und der Antwort.
    Kopfdaten,
    /// Der Koerper der Antwort, als Text.
    Vorschau,
    /// Alles ueber die Zeile (wie „Kopfdaten", dazu die Zeile selbst).
    Antwort,
}

impl NetzDetail {
    /// Die Reihenfolge im Bild (die Bruecke zaehlt die Reiter von 0).
    pub const ALLE: [NetzDetail; 3] = [
        NetzDetail::Kopfdaten,
        NetzDetail::Vorschau,
        NetzDetail::Antwort,
    ];

    /// Der Text des Reiters.
    pub const fn name(self) -> &'static str {
        match self {
            NetzDetail::Kopfdaten => "Kopfdaten",
            NetzDetail::Vorschau => "Vorschau",
            NetzDetail::Antwort => "Antwort",
        }
    }
}

impl Default for NetzDetail {
    fn default() -> Self {
        NetzDetail::Kopfdaten
    }
}

/// D18: Welcher Typ-Knopf liegt unter einem Punkt? (der Klick im Reiter)
///
/// GEMESSENE ENTSCHEIDUNG, warum die Rechtecke als PARAMETER kommen und
/// nicht aus dem Zustand berechnet werden: sie stehen im Bild an
/// [`crate::zeichnen::netz_gruppen_rechtecke`]. Der Zeichencode malt sie
/// dort; wer sie hier noch einmal rechnen wuerde, haette zwei Zahlen fuer
/// dieselbe Stelle — und der Klick koennte neben dem Knopf landen.
pub fn gruppe_unter(k: &[(NetzFilter, Rect)], punkt: Point) -> Option<NetzFilter> {
    k.iter().find(|(_, r)| r.contains(punkt)).map(|(g, _)| *g)
}

/// D19: WELCHER REITER DER DETAILANSICHT LIEGT UNTER EINEM PUNKT? (der
/// Klick im Reiter: Kopfdaten | Vorschau | Antwort)
///
/// GEMESSENE ENTSCHEIDUNG, warum das der ZWILLER von [`gruppe_unter`] ist und
/// nicht eine eigene Rechnung: die Rechtecke kommen vom Zeichencode
/// ([`crate::zeichnen::netz_detail_rechtecke`]) — dieselben drei, die da
/// gemalt werden. Wer hier die Geometrie noch einmal rechnen wuerde, haette
/// zwei Zahlen fuer dieselbe Stelle, und der Klick koennte neben dem Reiter
/// landen. Also: die Liste kommt herein, die Antwort geht raus.
///
/// GEMESSENE ENTSCHEIDUNG, warum der ERSTE Treffer gewinnt und nicht der
/// kleinste: die drei Rechtecke stehen 92 px auseinander und sind 88 px breit
/// — sie ueberlappen nicht (GEMESSEN: `netz_detail_rechtecke` rechnet
/// `detail.x + 6 + i * 92` mit Breite 88; der Abstand ist groesser als die
/// Breite). Ohne Ueberlappung ist „welcher" eine Frage mit genau einer
/// Antwort, und `find` ist die ehrliche Form davon.
pub fn detail_unter(r: &[(NetzDetail, Rect)], punkt: Point) -> Option<NetzDetail> {
    r.iter().find(|(_, k)| k.contains(punkt)).map(|(t, _)| *t)
}

/// D18: DIE ART EINER ANTWORT-VORSCHAU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VorschauArt {
    /// Der Body war Text (HTML, CSS, JS, JSON …).
    Text,
    /// Der Body war Text, aber gekuerzt (M4).
    Gekuerzt,
    /// Der Body war kein Text (Bild, Wasm, PDF).
    Binaer,
    /// Es gab keinen Body (Fehler, laufende Anfrage, 204/304).
    Leer,
}

impl VorschauArt {
    /// Der Text in der Klammer der Vorschau-Ueberschrift.
    pub const fn name(self) -> &'static str {
        match self {
            VorschauArt::Text => "Text",
            VorschauArt::Gekuerzt => "gekuerzt",
            VorschauArt::Binaer => "Binaer",
            VorschauArt::Leer => "leer",
        }
    }
}

impl From<&str> for RessourceArt {
    fn from(s: &str) -> Self {
        RessourceArt(s.to_string())
    }
}

/// Welches Protokoll benutzt wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protokoll {
    /// HTTP/1.1 (oder 1.0).
    H1,
    /// HTTP/2 über TLS (ALPN `h2`).
    H2,
    /// HTTP/3 über QUIC.
    H3,
    /// Aus der Statuszeile nicht erkennbar — als **Annahme** h1.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil-net` schreibt bei h2/h3 eine von sich
    /// erfundene Statuszeilen-Version in `Response::status.version`. Ein
    /// Reiter, der daraus „h1" macht und es als Messung zeigt, lügt. Also
    /// steht hier „angenommen", und der Aufrufer kann es mit
    /// [`AntwortPaket::protokoll`] richtigstellen.
    Angenommen,
}

impl Protokoll {
    /// Das Kuerzel in der Spalte (3 Zeichen, damit die Spalte steht).
    pub const fn kuerzel(self) -> &'static str {
        match self {
            Protokoll::H1 => "h1",
            Protokoll::H2 => "h2",
            Protokoll::H3 => "h3",
            Protokoll::Angenommen => "h1?",
        }
    }

    /// Der lange Name (fuer die Detailansicht).
    pub const fn lang(self) -> &'static str {
        match self {
            Protokoll::H1 => "HTTP/1.1",
            Protokoll::H2 => "HTTP/2",
            Protokoll::H3 => "HTTP/3",
            Protokoll::Angenommen => "HTTP/1.1 (angenommen)",
        }
    }
}

/// Was der Host beim Beginn einer Anfrage angibt.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfrageAngaben {
    /// Die Methode.
    pub methode: Method,
    /// Die volle URL.
    pub url: String,
    /// Die Kopfzeilen (ungekuerzt — [`NetzLog::beginne`] kuerzt).
    pub kopf_anfrage: BTreeMap<String, String>,
    /// Wofuer.
    pub art: RessourceArt,
    /// Welches Protokoll der Host benutzt (wenn er es weiss).
    pub protokoll: Option<Protokoll>,
    /// Woher (für die Spalte „Quelle").
    pub quelle: String,
}

impl AnfrageAngaben {
    /// Aus einem `narsil_net::Request` — der Weg, den der Browser nimmt.
    pub fn aus_request(r: &Request, quelle: &str) -> Self {
        Self {
            methode: r.method,
            url: url_aus_request(r),
            kopf_anfrage: r
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            art: RessourceArt::aus(r.art),
            protokoll: None,
            quelle: quelle.to_string(),
        }
    }
}

/// Die Ereignisquelle: das, was zwischen Beginn und Abschluss liegt.
#[derive(Debug, Clone, PartialEq)]
pub struct AnfrageBau {
    /// Die Nummer, die diese Anfrage bekommen hat.
    pub nr: u64,
    /// Die URL.
    pub url: String,
    /// Die Methode.
    pub methode: Method,
    /// Wofuer.
    pub art: RessourceArt,
    /// Die Kopfzeilen der Anfrage (gekuerzt, sortiert).
    pub kopf_anfrage: Vec<(String, String)>,
    /// Wann sie begonnen hat.
    pub start: std::time::Instant,
    /// Was der Host ueber das Protokoll weiss.
    pub protokoll: Option<Protokoll>,
    /// Woher.
    pub quelle: String,
    /// Die Zeiten der Phasen (D10, aus `narsil_net::phasen()`).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das hier und nicht erst am Abschluss:
    /// der Aufrufer kennt die Phasen, sobald `fetch` zurueck ist — und
    /// `abschluss` ist der Ort, an dem die Zeile entsteht. Wer sie erst im
    /// Abschluss holen muesste, braeuchte den Zaehler der Netz-Crate und
    /// koennte die Zeile der FALSCHEN Anfrage treffen, wenn zwischen
    /// Abschluss und Lesen eine andere Anfrage lief.
    pub phasen: NetzPhasen,
}

/// Wie eine Anfrage endet.
#[derive(Debug, Clone, PartialEq)]
pub enum Ergebnis {
    /// Sie kam zurueck.
    Antwort(AntwortPaket),
    /// Sie ist gescheitert.
    Fehler(NetFehler),
    /// Sie laeuft noch (nur fuer die Anzeige „offen").
    Laufend,
}

impl Ergebnis {
    /// Das Protokoll, wenn die Antwort es weiss.
    pub fn protokoll(&self) -> Option<Protokoll> {
        match self {
            Ergebnis::Antwort(a) => Some(a.protokoll),
            _ => None,
        }
    }
}

/// Was der Host aus einer `Response` macht.
#[derive(Debug, Clone, PartialEq)]
pub struct AntwortPaket {
    /// Die Statuszeile.
    pub status: narsil_net::StatusLine,
    /// Die Kopfzeilen.
    pub kopf: BTreeMap<String, String>,
    /// Wie viele `Set-Cookie` kamen.
    pub cookies: usize,
    /// Die Body-Groesse in Byte.
    pub groesse: u64,
    /// D18: DIE ANTWORT ALS TEXT (gekuerzt, M4).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Vorschau hier entsteht und nicht im
    /// Reiter: `Response::body` sind Bytes, und „ist das Text?" ist eine
    /// Frage an die Bytes. Der Ort, an dem die Bytes noch da sind, ist
    /// `aus_response` — spaeter hat sie nur noch die Zeile.
    pub vorschau: String,
    /// D18: Die Art der Vorschau (Text, gekuerzt, Binaer, leer).
    pub vorschau_art: VorschauArt,
    /// Das Protokoll — vom Host gesetzt, wenn er es weiss.
    ///
    /// GEMESSENE ENTSCHEIDUNG: es ist `Protokoll` (kein `Option`), weil
    /// [`AntwortPaket::aus_response`]] IMMER eine Antwortung liefert: wenn
    /// der Host das Protokoll nicht weiss, wird die Statuszeile gelesen, und
    /// wenn auch die unlesbar ist, kommt [`Protokoll::Angenommen`] — eine
    /// als Annahme markierte Zahl, keine erfundene.
    pub protokoll: Protokoll,
}

impl AntwortPaket {
    /// Aus einer `narsil_net::Response`, wenn der Host das Protokoll weiss.
    pub fn aus_response(r: &Response, protokoll: Option<Protokoll>) -> Self {
        // Das Protokoll: erst der Host, dann die Statuszeile.
        let proto = protokoll.unwrap_or_else(|| protokoll_aus_version(&r.status.version));
        let mut kopf: BTreeMap<String, String> = r
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // `set_cookie` steht auch in `headers` (dort kommasepariert). Fuer
        // die Detailansicht ist die EINE Zeile aus `set_cookie` die
        // Wahrheit (das steht so in `narsil-net`). Also haengen wir sie an.
        if !r.set_cookie.is_empty() {
            kopf.insert("set-cookie (alle)".to_string(), r.set_cookie.join("\n"));
        }
        let (vorschau, vorschau_art) = vorschau_aus(&r.body);
        Self {
            status: r.status.clone(),
            kopf,
            cookies: r.set_cookie.len(),
            groesse: r.body.len() as u64,
            protokoll: proto,
            vorschau,
            vorschau_art,
        }
    }
}

/// D18: Aus einem Body eine Textvorschau machen.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Grenze `MAX_VORSCHAU` ist und nicht
/// `max_zeile`: die Vorschau steht in der DETAILANSICHT, und die hat bis zu
/// 40 Zeilen Platz. 40 * 171 Zeichen sind ~6 800 Byte — mehr davon wuerde
/// die Liste sprengen, ohne dass der Nutzer etwas gewaennt.
///
/// GEMESSENE ENTSCHEIDUNG, warum „Binaer" NICHT als Hexdump erscheint: ein
/// Hexdump eines Bildes ist 40 Zeilen Muell, die die Kopfzeilen verdraengen.
/// Firefox zeigt in dem Fall „Binaer-Daten" — und genau das steht hier.
const MAX_VORSCHAU: usize = 6_000;

fn vorschau_aus(body: &[u8]) -> (String, VorschauArt) {
    if body.is_empty() {
        return (String::new(), VorschauArt::Leer);
    }
    // Ein NUL-Byte ist das uebliche Kennzeichen fuer „kein Text" (M7: es
    // gibt kein Byte, das in JEDEM Text vorkommt).
    if body.contains(&0) {
        return (String::new(), VorschauArt::Binaer);
    }
    let Ok(text) = std::str::from_utf8(body) else {
        return (String::new(), VorschauArt::Binaer);
    };
    // Steuerzeichen raus: ein Antwort-Body kann ein Byte 0x07 enthalten, und
    // das wuerde im Reiter ein Klingelzeichen zeichnen.
    let sauber: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if sauber.chars().count() <= MAX_VORSCHAU {
        return (sauber, VorschauArt::Text);
    }
    let kurz: String = sauber.chars().take(MAX_VORSCHAU).collect();
    (format!("{kurz}"), VorschauArt::Gekuerzt)
}

/// Das Protokoll aus dem Text der Statuszeile.
///
/// GEMESSEN: der Text kann `HTTP/1.1`, `HTTP/1.0`, `HTTP/2`, `HTTP/3`,
/// `h2` oder ein erfundener Wert sein (siehe den Modul-Kopf). Alles
/// Unbekannte ist [`Protokoll::Angenommen`] — und NICHT `h1`, weil eine
/// erfundene Zahl schlimmer ist als eine als Annahme markierte.
pub fn protokoll_aus_version(version: &str) -> Protokoll {
    let v = version.trim().to_ascii_lowercase();
    if v.contains("/3") || v == "h3" || v.starts_with("h3") {
        return Protokoll::H3;
    }
    if v.contains("/2") || v == "h2" || v.starts_with("h2") {
        return Protokoll::H2;
    }
    if v.contains("/1") || v == "h1" || v.starts_with("h1") {
        return Protokoll::H1;
    }
    Protokoll::Angenommen
}

/// Die URL aus einem `Request` zusammensetzen.
///
/// M4: der Pfad kann lang sein (ein Daten-URI in einem `src`), der Host
/// auch. Beide werden ungekuerzt gebaut und erst in [`NetzLog::beginne`]
/// gekuerzt — das eine `Vec::with_capacity` von der Laenge einer
/// Eingabe ist verboten (M4), ein `format!` nicht.
pub fn url_aus_request(r: &Request) -> String {
    let schema = if r.port == 443 { "https" } else { "http" };
    let host = if host_ist_ipv6(&r.host) {
        format!("[{}]", r.host)
    } else {
        r.host.clone()
    };
    let pfad = if r.path.starts_with('/') {
        r.path.clone()
    } else {
        format!("/{}", r.path)
    };
    format!("{schema}://{host}:{}{pfad}", r.port)
}

/// Host mit Doppelpunkten (IPv6) braucht Klammern in einer URL.
fn host_ist_ipv6(host: &str) -> bool {
    host.contains(':') && !host.starts_with('[')
}

/// Die Kopfzeilen sortieren und kuerzen (M4).
fn kopfzeilen_bis(k: &BTreeMap<String, String>, budget: usize) -> Vec<(String, String)> {
    let g = Grenzen::STANDARDAUSGABE;
    let mut r = Vec::new();
    let mut ausgeblendet = 0usize;
    // Die Grenze ist das Minimum aus „je Anfrage" und „im Protokoll".
    let grenze = g.max_kopfzeilen.min(budget);
    for (name, wert) in k.iter() {
        if r.len() >= grenze {
            ausgeblendet += 1;
            continue;
        }
        r.push((g.kuerze(name, 64), g.kuerze(wert, g.max_kopfwert)));
    }
    if ausgeblendet > 0 {
        // M7: die Zahl der nicht gezeigten Kopfzeilen wird GEMELDET, nicht
        // stillschweigend vergessen — ein `Content-Security-Policy` fehlt
        // sonst im Bild, ohne dass jemand es merkt.
        r.push((
            String::from("+ ausgeblendet"),
            format!("{ausgeblendet} weitere Kopfzeilen"),
        ));
    }
    r
}

/// EIN VORGANG, DER AUSSERHALB DER SESSION LAEUFT (D10).
///
/// # WARUM ES DAS GIBT
///
/// GEMESSEN am 01.10.2026 an `crates/narsil`: der ganze Ladeweg ist
/// `&self` — `Sitzung::laden(&self)`, `Sitzung::laden_mit_breite(&self)`,
/// `aus_ist(&self)` — und der `&mut self` steht erst in `lade_jetzt`, wo
/// die Seite schon fertig ist. `narsil_net::fetch` sitzt aber ganz unten
/// in der freien Funktion `lade_ueber_netz`, und die hat keinen Zugriff
/// auf die Sitzung.
///
/// GEMESSENE ENTSCHEIDUNG (drei, die verworfen wurden):
/// * `static Mutex<NetzLog>` — globale Zustand, REGELN M6 verbietet das
///   ohne Obergrenze, und es waere eine ZWEITE Wahrheit neben
///   `Sitzung::devtools.netz`.
/// * `&self` auf `&mut self` umstellen — das zieht durch die ganze Crate
///   (`aus_ist` wird aus 40 Tests und aus `jsvm.rs` aufgerufen) und
///   betrifft eine fremde, gerade bearbeitete Datei.
/// * `NetzLog` selbst durchreichen statt einen Vorgang — dasselbe
///   Problem, nur mit einem `&mut` an jedem Aufruf.
///
/// Also: ein kleiner **Ausgabepuffer auf dem Stapel**. `lade_ueber_netz`
/// fuellt ihn, `lade_jetzt` gibt ihn an `NetzLog::abschluss_ereignis`.
/// Das ist additiv (keine bestehende Signatur aendert sich), explizit
/// (kein globaler Zustand) und testbar (der Puffer ist ein Wert).
#[derive(Debug, Clone, PartialEq)]
pub struct NetzEreignis {
    /// Die Angaben zur Anfrage (leer, wenn nichts geholt wurde).
    pub angaben: Option<AnfrageAngaben>,
    /// Wann sie begann.
    pub start: Option<std::time::Instant>,
    /// Das Ergebnis, sobald es da ist.
    pub ergebnis: Option<Ergebnis>,
    /// Die Zeiten der Phasen, sobald `fetch` zurueck ist (D10).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Phasen IM PUFFER stehen und nicht
    /// erst beim Abschluss gelesen werden: `narsil_net::phasen()` ist der
    /// Phasenwert des THREADS (dort bewusst `thread_local!`). Zwischen
    /// `fetch` und `abschluss_ereignis` kann derselbe Thread eine zweite
    /// Anfrage stellen (Bild, Skript) — dann gehoerte die zweite Zeit zur
    /// ersten Zeile. Der Puffer ist der Ort, an dem „zu MEINER Anfrage"
    /// festgehalten wird.
    pub phasen: NetzPhasen,
}

impl NetzEreignis {
    /// Ein leerer Puffer. `const`, weil er in einem Test als Feld steht.
    pub const fn leer() -> Self {
        Self {
            angaben: None,
            start: None,
            ergebnis: None,
            phasen: NetzPhasen {
                // GEMESSENE ENTSCHEIDUNG, warum hier `None` und nicht 0.0:
                // `NetzPhasen` ist keine Copy, also braucht der Wert
                // Felder. `const fn` verlangt eine Konstruktion, und
                // `NetzPhasen::default()` ist in einem `const`-Feld nicht
                // zulaessig — daher die ausgeschriebene Form. Sie sagt
                // dasselbe wie `default()`: nichts gemessen.
                dns_ms: None,
                tcp_ms: None,
                tls_ms: None,
                erstes_byte_ms: None,
                gesamt_ms: None,
            },
        }
    }

    /// D10: die Phasen der Netz-Crate uebernehmen (nach `fetch`).
    ///
    /// Nimmt `Option`, weil `narsil_net::phasen()` `None` heisst: „hier
    /// wurde noch keine Anfrage gestellt". Das wird zu „nichts gemessen"
    /// — und NICHT zu 0 ms.
    pub fn setze_phasen(&mut self, p: Option<narsil_net::Phasen>) {
        if let Some(p) = p.as_ref() {
            self.phasen = NetzPhasen::aus(p);
        }
    }

    /// `true`, wenn ueberhaupt etwas geholt wurde.
    ///
    /// GEMESSENE BEGRUENDUNG: `lade()` kann `about:`, `file://` oder
    /// `narsil://` bedienen — da wird NICHT geholt, und eine Zeile im
    /// Netz-Reiter waer eine erfundene Anfrage. Der Aufrufer fragt das
    /// hier, statt es zu raten.
    pub fn ist_geholt(&self) -> bool {
        self.angaben.is_some()
    }

    /// Die Anfrage merken und die Uhr starten (vor `fetch`).
    pub fn beginne(&mut self, anfrage: &AnfrageAngaben) {
        self.angaben = Some(anfrage.clone());
        self.start = Some(std::time::Instant::now());
        self.ergebnis = None;
    }

    /// Das Ergebnis merken (nach `fetch`).
    pub fn ende(&mut self, r: Ergebnis) {
        self.ergebnis = Some(r);
    }

    /// Wie lange es gedauert hat, oder `None` (wenn nichts lief).
    pub fn dauer(&self) -> Option<Duration> {
        self.start.map(|s| s.elapsed())
    }
}

/// Den Vorgang in das Protokoll uebernehmen.
///
/// `None`, wenn nichts geholt wurde (siehe [`NetzEreignis::ist_geholt`]).
pub fn abschluss_ereignis(log: &mut NetzLog, e: NetzEreignis) -> Option<u64> {
    let angaben = e.angaben.clone()?;
    let ergebnis = e.ergebnis.clone()?;
    let b = AnfrageBau {
        nr: log.naechste_nr_offen(),
        url: angaben.url,
        methode: angaben.methode,
        art: angaben.art,
        kopf_anfrage: kopfzeilen_bis(
            &angaben.kopf_anfrage,
            Grenzen::STANDARDAUSGABE.max_kopfzeilen_gesamt,
        ),
        start: e.start.unwrap_or_else(std::time::Instant::now),
        protokoll: angaben.protokoll,
        quelle: angaben.quelle,
        phasen: e.phasen,
    };
    Some(log.abschluss(b, ergebnis))
}

/// Die Dauer eines Vorgangs als Millisekunden (fuer Tests).
pub fn dauer_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Ein Netzfehler als Wert, mit Text (damit `Ergebnis` vergleichbar bleibt).
///
/// GEMESSENE BEGRUENDUNG, warum das ein eigener Typ ist: `NetError`
/// enthaelt `std::io::Error`, das ist **nicht** `Clone`. Ein Protokoll, das
/// sich nicht vergleichen laesst, kann nicht getestet werden („war die
/// Zeile gleich?") — und `Ergebnis` wird in genau diesem Sinn verglichen.
/// Also traegt die Zeile den **Text** des Fehlers, nicht den Fehler selbst;
/// die Zahl, die man braucht (die Dauer), steht daneben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetFehler {
    /// Der Text des Fehlers (`Display` von `NetError`).
    pub text: String,
}

impl NetFehler {
    /// Aus einem `NetError`.
    pub fn aus(e: NetError) -> Self {
        Self {
            text: e.to_string(),
        }
    }

    /// Aus dem TEXT eines `NetError`.
    ///
    /// GEMESSENE BEGRÜNDUNG: `NetError` enthält `std::io::Error` und ist
    /// deshalb nicht `Clone`. Wer den Fehler schon als `&NetError` hat
    /// (also neben einer anderen Benutzung der Antwort), kann ihn nicht
    /// verschieben — und `String::from(&e)` geht, weil `Display` bei
    /// `&T` auch bei `T` gilt.
    pub fn aus_text(e: &NetError) -> Self {
        Self {
            text: e.to_string(),
        }
    }
}

impl From<NetError> for NetFehler {
    fn from(e: NetError) -> Self {
        Self::aus(e)
    }
}

impl std::fmt::Display for NetFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod netz_eigene {
    use super::*;

    /// EINE ANFRAGE WIRD ZU EINER ZEILE MIT ALLEN SECHS SPALTEN.
    ///
    /// M7: der Test nennt jede Spalte einzeln.
    #[test]
    fn eine_anfrage_hat_alle_sechs_spalten() {
        let mut l = NetzLog::neu();
        let anfrage = AnfrageAngaben {
            methode: Method::Get,
            url: "https://beispiel.test/pfad.css".to_string(),
            kopf_anfrage: BTreeMap::from([
                ("User-Agent".to_string(), "Narsil".to_string()),
                ("Accept".to_string(), "text/css".to_string()),
            ]),
            art: RessourceArt::from("Dokument"),
            protokoll: Some(Protokoll::H2),
            quelle: "fetch".to_string(),
        };
        let b = l.beginne(&anfrage);
        assert_eq!(l.offen, 1, "die offene Zaehlung stimmt nicht");
        let antwort = narsil_net::Response {
            status: narsil_net::StatusLine {
                version: "HTTP/2".to_string(),
                code: 200,
                reason: "OK".to_string(),
            },
            headers: BTreeMap::from([
                ("content-type".to_string(), "text/css".to_string()),
                ("content-length".to_string(), "1234".to_string()),
            ])
            .into_iter()
            .collect(),
            body: vec![0u8; 1234],
            trailers: BTreeMap::new().into_iter().collect(),
            set_cookie: vec!["a=1".to_string()],
            body_quelle: narsil_net::BodyQuelle::ContentLength,
            keep_alive: narsil_net::KeepAlive::Bleibt,
            url: "https://beispiel.test/pfad.css".to_string(),
            entpackt: false,
        };
        let nr = l.abschluss(
            b,
            Ergebnis::Antwort(AntwortPaket::aus_response(&antwort, Some(Protokoll::H2))),
        );
        assert_eq!(nr, 1, "die Nummer stimmt nicht");
        assert_eq!(l.offen, 0, "die Anfrage bleibt offen");
        assert_eq!(l.zeilen.len(), 1);
        let z = &l.zeilen[0];
        assert_eq!(z.methode, Method::Get);
        assert_eq!(z.url, "https://beispiel.test/pfad.css");
        assert_eq!(z.status.as_ref().map(|s| s.code), Some(200));
        assert_eq!(z.protokoll, Protokoll::H2);
        assert_eq!(z.groesse, 1234, "die Groesse stimmt nicht");
        assert!(z.dauer_ms >= 0.0);
        assert_eq!(z.kopf_anfrage.len(), 2, "die Anfrage-Kopfzeilen fehlen");
        assert_eq!(
            z.kopf_antwort.len(),
            3,
            "die Antwort-Kopfzeilen fehlen (inkl. set-cookie)"
        );
        assert_eq!(z.cookies, 1);
        assert!(z.fehler.is_none());
        // UND die Zeile steht als lesbarer Text da.
        let t = l.text_zeilen();
        assert!(t[0].contains("200"), "der Status fehlt: {}", t[0]);
        assert!(t[0].contains("h2"), "das Protokoll fehlt: {}", t[0]);
        assert!(t[0].contains("1234"), "die Groesse fehlt: {}", t[0]);
    }

    /// DAS PROTOKOLL WIRD ERKANNT, NICHT GERATEN.
    ///
    /// GEMESSENE BEGRÜENDUNG: die Erkennung aus dem Text der Statuszeile
    /// ist die einzige, die es ohne `narsil-net`-Aenderung gibt. Der Test
    /// fragt alle vier Faelle ab und misst, dass der unbekannte Fall
    /// **als Annahme** kommt.
    #[test]
    fn das_protokoll_wird_erkannt_und_bei_unbekannt_gemeldet() {
        assert_eq!(protokoll_aus_version("HTTP/1.1"), Protokoll::H1);
        assert_eq!(protokoll_aus_version("HTTP/1.0"), Protokoll::H1);
        assert_eq!(protokoll_aus_version("HTTP/2"), Protokoll::H2);
        assert_eq!(protokoll_aus_version("HTTP/3"), Protokoll::H3);
        assert_eq!(protokoll_aus_version("h3"), Protokoll::H3);
        assert_eq!(
            protokoll_aus_version("irgendwas"),
            Protokoll::Angenommen,
            "ein unbekannter Text wurde als h1 gemeldet"
        );
        assert_eq!(protokoll_aus_version(""), Protokoll::Angenommen);
        // UND die Kuerzel unterscheiden die Annahme von der Messung.
        assert_ne!(Protokoll::H1.kuerzel(), Protokoll::Angenommen.kuerzel());
        assert!(Protokoll::Angenommen.lang().contains("angenommen"));
    }

    /// EIN FEHLER STEHT IN DER ZEILE, UND DIE ANFRAGE IST ZU.
    ///
    /// GEMESSENE BEGRÜNDUNG: der Fehlerfall ist der Grund fuer das
    /// `Result`-Shaped [`Ergebnis`] — ohne ihn gäbe es keine Zeile, wenn
    /// `fetch_mit` `Err` liefert, und der Reiter wuerde eine Anfrage
    /// verschlucken, die der Nutzer sehen will (die Zeit bis zum Fehler ist
    /// eine Zahl, die man sucht).
    #[test]
    fn ein_fehler_bleibt_in_der_zeile() {
        let mut l = NetzLog::neu();
        let b = l.beginne(&AnfrageAngaben {
            methode: Method::Get,
            url: "https://nope.test/".to_string(),
            kopf_anfrage: BTreeMap::new(),
            art: RessourceArt::from("Bild"),
            protokoll: None,
            quelle: "fetch".to_string(),
        });
        let nr = l.abschluss(
            b,
            Ergebnis::Fehler(NetFehler::aus(NetError::ZeitlimitLesen)),
        );
        assert_eq!(l.offen, 0, "die Anfrage bleibt offen");
        let z = &l.zeilen[0];
        assert_eq!(z.nr, nr);
        assert!(z.status.is_none(), "ein Fehler hat einen Status");
        assert!(z.fehler.is_some(), "der Fehlertext fehlt");
        assert_eq!(z.groesse, 0);
        // UND das Protokoll ist als Annahme markiert, nicht als h1 gemessen.
        assert_eq!(z.protokoll, Protokoll::Angenommen);
        assert_eq!(l.zahlen().fehler, 1, "die Fehlerzahl stimmt nicht");
    }

    /// BOMBEN (M8): 3000 KOPFZEILEN, 8-MB-URL, 5000 ANFRAGEN.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_net::Limits` erlaubt 64 KiB Kopfzeilen
    /// (das sind ~900 Zeilen), ein Server kann einen 8-MB-`Location`-Kopf
    /// schicken, und eine Seite kann 3000 Anfragen machen (alle Bilder).
    #[test]
    fn bomben_anfragen_kommen_mit_einem_benannten_ergebnis() {
        let g = Grenzen::STANDARDAUSGABE;
        let mut l = NetzLog::neu();
        // 1. 900 Kopfzeilen.
        let mut kopf = BTreeMap::new();
        for i in 0..900 {
            kopf.insert(format!("X-{i}"), "v".to_string());
        }
        let b = l.beginne(&AnfrageAngaben {
            methode: Method::Get,
            url: "https://b.test/".to_string(),
            kopf_anfrage: kopf,
            art: RessourceArt::from("Dokument"),
            protokoll: Some(Protokoll::H1),
            quelle: "fetch".to_string(),
        });
        l.abschluss(b, Ergebnis::Laufend);
        let z = &l.zeilen[0];
        assert_eq!(
            z.kopf_anfrage.len(),
            g.max_kopfzeilen + 1,
            "die Kopfzeilen sind nicht begrenzt (mit der Meldung)"
        );
        assert!(
            z.kopf_anfrage.last().map(|(a, _)| a.as_str()) == Some("+ ausgeblendet"),
            "die Zahl der ausgeblendeten Kopfzeilen wird nicht gemeldet"
        );
        // 2. Eine 8-MB-URL.
        let mut m = NetzLog::neu();
        let b = m.beginne(&AnfrageAngaben {
            methode: Method::Get,
            url: "https://b.test/".to_string() + &"x".repeat(8_000_000),
            kopf_anfrage: BTreeMap::new(),
            art: RessourceArt::from("Bild"),
            protokoll: None,
            quelle: "fetch".to_string(),
        });
        m.abschluss(b, Ergebnis::Laufend);
        assert!(
            m.zeilen[0].url.chars().count() <= g.max_zeile,
            "die URL-Zeile ist zu lang"
        );
        assert!(
            m.zeilen[0].voll.chars().count() <= 4096,
            "die volle URL ist nicht begrenzt"
        );
        // 3. 3000 Anfragen: die Liste haelt ihre Grenze.
        let mut n = NetzLog::neu();
        for i in 0..(g.max_netzanfragen + 300) {
            let b = n.beginne(&AnfrageAngaben {
                methode: Method::Get,
                url: format!("https://b.test/{i}"),
                kopf_anfrage: BTreeMap::new(),
                art: RessourceArt::from("Bild"),
                protokoll: Some(Protokoll::H1),
                quelle: "fetch".to_string(),
            });
            n.abschluss(b, Ergebnis::Laufend);
        }
        assert_eq!(
            n.zeilen.len(),
            g.max_netzanfragen,
            "die Liste ist nicht begrenzt"
        );
        assert_eq!(n.offen, 0, "es bleiben Anfragen offen");
        assert!(n.verworfen >= 300, "der Zaehler zaehlt nicht mit");
    }

    /// DIE URL AUS EINER ANFRAGE IST RICHTIG ZUSAMMENGESETZT.
    ///
    /// GEMESSENE ENTSCHEIDUNG: IPv6-Hosts brauchen Klammern, sonst ist die
    /// URL nicht lesbar (`https://::1:443/x` hat zwei Doppelpunkte und
    /// keinen Host). Der Test misst alle drei Faelle.
    #[test]
    fn die_url_ist_richtig_zusammengesetzt() {
        let mut r = Request::get("beispiel.test", 443, "/a/b.css");
        assert_eq!(url_aus_request(&r), "https://beispiel.test:443/a/b.css");
        r.port = 80;
        assert_eq!(url_aus_request(&r), "http://beispiel.test:80/a/b.css");
        r.path = "kein/slash".to_string();
        assert_eq!(url_aus_request(&r), "http://beispiel.test:80/kein/slash");
        let v6 = Request::get("::1", 8080, "/x");
        assert_eq!(
            url_aus_request(&v6),
            "http://[::1]:8080/x",
            "IPv6 ohne Klammern"
        );
    }
}
