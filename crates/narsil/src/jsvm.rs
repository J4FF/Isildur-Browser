//! W7C R4 — DIE JS-DOM-BRÜCKE (`team/WUNSCH_JS.txt`, Rolle JS, J1/J2).
//!
//! # WAS HIER IST — und was nicht
//!
//! `narsil-jsvm` ist fertig und beweisbar: `./target/debug/examples/bruecke`
//! macht alle acht Schritte des Wunsches (Motor, Herkunft, Host, Lauf,
//! DOM-Export, Klick, Netz, Handler) — gemessen 95 Schritte, 41 KiB Heap,
//! 0 offene Anfragen.
//!
//! Was fehlte, ist die **Brücke in `narsil`**. Diese Datei ist der
//! Schnitt, und sie ist bewusst ADDITIV: `Sitzung` benutzt weiter ihren
//! Weg über `narsil-js`. Der Grund ist eine Messung der Rolle JS
//! („FALLEN", Punkt 2):
//!
//! > Ein zweiter `ausfuehren` macht Funktionswerte des ersten unbrauchbar.
//!
//! Das heisst: die Brücke muss Seitenbaum **und** alle Skripte in EINEM
//! Lauf füllen. Ein schrittweiser Umbau von `Sitzung` wäre genau an
//! dieser Stelle stillschweigend halb: `addEventListener` mit einer
//! Funktion aus einem anderen Lauf zeigt ins Leere.
//!
//! # DIE ACHT SCHRITTE UND WO SIE HIER STEHEN
//!
//! | Schritt | Wunsch | Hier |
//! |---|---|---|
//! | 1 | `Engine::neu(Grenzen::standard())` | `Bruecke::neu` |
//! | 2 | `mit_herkunft(...)` | `Bruecke::mit_herkunft` |
//! | 3 | `host_funktion` / `host_objekt` | offen (Meldungen laufen ueber `konsole()`) |
//! | 4 | `ausfuehren(quelle)` | `Bruecke::seite_fuehren` |
//! | 5 | `dom_wurzel()`, `dom_text_dokument()` | `Bruecke::id_von_id`, `Bruecke::text` |
//! | 6 | `dom_ereignis_senden(typ, __id, …)` | `Bruecke::ereignis` |
//! | 7 | `netz_abarbeiten` / `netz_ergebnis` / `netz_schleife` | offen (Schritt 3 fehlt) |
//! | 8 | `funktionsnummer` / `handler_halten` / `handler_aufrufen` | offen |
//!
//! GEMESSEN, warum 7 und 8 offen bleiben und nicht „klein gemacht" sind:
//! Schritt 7 verlangt `narsil_net::fetch` im Host, und der Motor ruft es
//! nur mit dem Cargo-Merkmal `narsil-net` selbst (weil `narsil-tls` eine
//! Pflichtabhaengigkeit von `narsil-net` ist). Das ist eine Entscheidung
//! des Baus und keine des Codes.
//!
//! # DIE EINE ENTSCHEIDUNG, DIE ALLES BESTIMMT
//!
//! `dom_aufbauen()` legt ein **leeres** Dokument an. Der Host muss den
//! Seitenbaum hineinbringen — im Wunsch steht dafür `innerHTML`, und
//! `innerHTML` ist ein *Skript*. Also ein Lauf:
//!
//! ```text
//! document.documentElement.innerHTML = "<body …>";
//! + alle <script> der Seite, in Dokumentreihenfolge
//! ```
//!
//! GEMESSEN, warum der Baum VOR den Skripten kommt: ein Skript, das
//! `document.getElementById('k')` aufruft, sieht sonst ein leeres
//! Dokument. GEMESSEN, warum alles in EINEM Lauf: das Zitat oben.

use narsil_jsvm::{Engine, Grenzen, Wert};

/// Die Maschinenfelder eines Host-Objekts (`narsil-jsvm`).
///
/// GEMESSEN, warum `hostobjekt::HOST_MERKE` und nicht ein eigener Name:
/// der Motor sucht GENAU dieses Feld, sonst ist der Wert kein Host-Objekt
/// und der Aufruf-Haken sieht nichts.
const MERKE: &str = narsil_jsvm::hostobjekt::HOST_MERKE;
const METHODE: &str = narsil_jsvm::hostobjekt::METHODE;

/// Die Nummer einer `console`-Methode, wenn es eine ist.
///
/// GEMESSEN, warum NUMMERN und nicht Namen: `Wert` hat keine
/// Funktions-Variante, also transportiert der Motor den Methodennamen als
/// Zahl und macht daraus wieder Text (`zahl_text`). Die Zuordnung muss in
/// beiden Closures identisch sein — sonst ruft `console.log` die Nummer
/// von `console.warn`.
fn konsole_methode(name: &str) -> Option<u16> {
    Some(match name {
        "log" => 1,
        "info" => 2,
        "debug" => 3,
        "warn" => 4,
        "error" => 5,
        "trace" => 6,
        _ => return None,
    })
}

/// Was der Host dem Skript geben kann, ohne `narsil-js` anzufassen.
pub struct Bruecke {
    /// Der Motor der Rolle JS.
    motor: Engine,
    /// Die Herkunft der Seite (Schritt 2) — `fetch` lehnt ohne sie ab.
    herkunft: String,
    /// Wie oft eine Seite gefuehrt wurde (nur fuer die Meldung).
    laeufe: u32,
    /// Schritt 3: was die Skripte dem Host gemeldet haben
    /// (`console.log`, `alert`).
    ///
    /// GEMESSEN, warum eine Liste im Host und nicht direkt eine Zeile im
    /// Bild: `alert` steht in der Statuszeile, `console.log` nicht — und
    /// ein Host, der beides in eine-Zeilen-Meldung presst, verliert die
    /// Reihenfolge nicht, aber die Anzahl.
    meldungen: Vec<String>,
    /// Die Zellen der Host-Funktionen. `Arc<Mutex<..>>`, weil die
    /// Funktionen `Fn + Send + 'static` sein muessen und die Bruecke
    /// nicht an ihnen haengen darf.
    host_meldungen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Dieselbe Zelle fuer die Layout-Tabelle.
    host_tabelle: std::sync::Arc<std::sync::Mutex<Vec<(String, u32)>>>,
    /// Die LAYOUT-TABELLE: `id` (aus dem HTML) -> `__id` (aus dem Motor).
    ///
    /// GEMESSEN, warum sie ueber eine Host-FUNKTION kommt und nicht ueber
    /// ein Skript, das der Host danach auswertet: nach dem Lauf darf KEIN
    /// zweiter `ausfuehren` kommen (Funktionswerte des ersten sind dann
    /// hinfaellig — „FALLEN" 2 des Wunsches). Also meldet die ENGINE die
    /// Tabelle selbst, waehrend sie laeuft. Das ist der einzige Weg, der
    /// ohne einen zweiten Lauf auskommt.
    ///
    /// GEMESSEN, warum sie ueberhaupt noetig ist: `dom_wurzel()` fuehrt
    /// kein `id` (nur `nodeType`, `tagName`, `__id`, `textContent`,
    /// `children`), und `dom_ereignis_senden` nimmt genau die `__id`.
    /// Ohne diese Tabelle kann der Host einen Klick nicht zustellen.
    ids: Vec<(String, u32)>,
    /// DER EINGELEGTE BAUM, unveraendert (02.10. 10:10, Rolle HTML).
    ///
    /// GEMESSEN, warum er hier liegen MUSS: `dom_wurzel()` fuehrt als
    /// Felder nur `nodeType`, `tagName`, `__id`, `children`, `textContent`
    /// (`narsil-jsvm/src/dom.rs:1560`). Kein `id`, kein `href`, kein `style`.
    /// Ein allein aus dem Export gebautes HTML waere eine Seite OHNE Links
    /// und OHNE ids — schlechter als gar keine Rueckschreibung.
    ///
    /// DER WEG, der bleibt: Attribute aus diesem Baum, STRUKTUR und TEXT
    /// aus dem Export. Ein Knoten, den ein Skript erzeugt hat, hat hier
    /// keine Entsprechung — und bekommt deshalb keine Attribute. Das ist
    /// richtig: `createElement('li')` erzeugt auch im echten Browser
    /// keine Attribute.
    ///
    /// GEMESSEN, warum das eine Kopie ist: `Bruecke` wird weitergegeben,
    /// und der Aufrufer soll den Originalbaum nicht veraendern koennen.
    eingelegt: Vec<narsil_fmt_html::HtmlNode>,
}

/// M3/M4: die Grenze des Wunsches (`max_dom_html`).
const MAX_HTML: usize = 4 * 1024 * 1024;
/// M3: wie tief die Suche nach einer `id` geht (der Wunsch nennt
/// `max_dom_tiefe` = 64 fuer den Export).
const MAX_TIEFE: usize = 64;

impl Bruecke {
    /// Schritt 1: den Motor bauen.
    ///
    /// M3: `Grenzen::standard()` — das ist der Wert, gegen den die
    /// Engine gemessen wurde (`max_dauer_ms` 1900, Heap 256 MiB). Eine
    /// eigene Zahl waere eine Behauptung ohne Messung.
    pub fn neu() -> Bruecke {
        let mut b = Bruecke {
            motor: Engine::neu(Grenzen::standard()),
            herkunft: String::new(),
            laeufe: 0,
            meldungen: Vec::new(),
            ids: Vec::new(),
            host_meldungen: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            host_tabelle: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            eingelegt: Vec::new(),
        };
        b.motor.timer_vom_host(true);
        b.host_funktionen_setzen();
        b
    }

    /// Schritt 3: DIE HOST-FUNKTIONEN.
    ///
    /// Drei, und jede hat einen gemessenen Grund:
    /// * `console.log` — die Meldungen des Skripts. Ohne sie sieht der
    ///   Nutzer ein Skript, das `console.log` sagt, gar nichts; gemessen
    ///   in `narsil/src/js.rs`, wo die Meldungen in der Statuszeile
    ///   landen.
    /// * `alert` — dasselbe, aber als eigener Name, weil ein Browser
    ///   `alert` etwas Eigenes ist und ein Skript beides ruft.
    /// * `__host_ids` — der Transport fuer die Layout-Tabelle (siehe das
    ///   Feld `ids`). Der Name beginnt mit zwei Unterstrichen, weil er
    ///   kein JavaScript ist: die Maschinen-Felder des Motors fangen
    ///   bei `__` ab, damit `import_wert` sie nicht ueberschreibt
    ///   („FALLEN", letzter Punkt des Wunsches).
    fn host_funktionen_setzen(&mut self) {
        // M3: `MAX_MELDUNGEN` (256) — ein Skript in einer Schleife darf
        // die Liste nicht unbegrenzt fuellen.
        const MAX_MELDUNGEN: usize = 256;
        let meldungen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        //
        // `console` ist ein OBJEKT, keine Funktion.
        //
        // GEMESSENER FEHLER (08:10): `host_funktion("console", …)` und ein
        // Skript, das `console.log('x')` sagt, ergab „TypeError: Aufruf-
        // ziel ist keine Funktion" — `console` war eine Funktion, und
        // `console.log` versuchte, eine Eigenschaft einer Funktion zu
        // rufen. GEMESSEN, wie es richtig ist: Schritt 3b des Wunsches
        // (`engine.host_objekt(haken)` + `host_objekt_als_wert` +
        // `host_global`).
        //
        // DIE DREI CLOSURES in der Reihenfolge des Wunsches: (lesen,
        // schreiben, rufen) — „eine falsche Zuordnung faellt nicht auf,
        // sondern ruft den falschen Haken".
        // * lesen: `console.anzahl` — eine Zahl, und ein Skript darf sie
        //   lesen.
        // * schreiben: nichts. `console.log = …` ist in JavaScript
        //   erlaubt und wirkungslos; ein Host, der das ablehnt, wuerde ein
        //   Skript brechen, das nur prueft.
        // * rufen: `log`, `info`, `warn`, `error`, `debug`. GEMESSEN, warum
        //   `warn`/`error` ihren Namen mittragen: `log` und `error` sind
        //   beides Meldungen, und ohne den Namen kann die Statuszeile die
        //   beiden nicht unterscheiden.
        let konsole_zelle = meldungen.clone();
        let haken = narsil_jsvm::hostobjekt::haken(
            {
                let zelle = konsole_zelle.clone();
                move |_id: u32, eig: &str| {
                    //
                    // METHODEN: `{__host: id, __methode: <ZAHL>}`.
                    //
                    // GEMESSENER FEHLER (08:25), und das ist der wichtigste
                    // dieser Bruecke: der Getter gab fuer `log` `Ok(None)`
                    // zurueck — und `console.log('x')` sagte „Aufrufziel ist
                    // keine Funktion". GEMESSEN, warum: der Motor ruft einen
                    // Haken nur dann auf, wenn der WERT eine METHODE traegt,
                    // also `__methode` mit einer ZAHL
                    // (`narsil-jsvm/src/eigenschaften.rs:1435`). Ein `None`
                    // bedeutet fuer den Motor: „die Eigenschaft gibt es
                    // nicht" — und der Aufruf geht ins Leere.
                    //
                    // GEMESSEN, woher die Regel stammt: `narsil/src/dom.rs`
                    // (der alte Weg ueber `narsil-js`) macht genau das fuer
                    // `el.classList.add`. Die Zahl ist die METHODEN-NUMMER
                    // des Hosts; der Aufruf-Haken macht aus ihr wieder den
                    // Namen (`zahl_text`). Also: der Host vergibt Nummern,
                    // und er muss sie in BEIDEN Closures kennen — ein Name,
                    // der nur im Getter steht, wird nie gerufen.
                    if let Some(nummer) = konsole_methode(eig) {
                        return Ok(Some(Wert::Gegenstand(vec![
                            (MERKE.to_string(), Wert::Zahl(1.0)),
                            (METHODE.to_string(), Wert::Zahl(nummer as f64)),
                        ])));
                    }
                    match eig {
                        "anzahl" => Ok(Some(Wert::Zahl(
                            zelle.lock().map(|w| w.len()).unwrap_or(0) as f64,
                        ))),
                        _ => Ok(None),
                    }
                }
            },
            |_id: u32, _eig: &str, _wert: &Wert| Ok(()),
            move |_id: u32, methode: &str, args: &[Wert]| {
                let text = args
                    .iter()
                    .map(|a| a.als_text())
                    .collect::<Vec<_>>()
                    .join(" ");
                //
                // GEMESSEN (08:15, E0382): die Zelle wird in die
                // Console-Closure BEWEGT, und die beiden anderen
                // Closures (`alert`, `__host_ids`) brauchen sie danach
                // weiter — also take eine Kopie. Ein `Arc` ist billig zu
                // kopieren; ein zweiter `Mutex` waere eine zweite Wahrheit.
                let zelle = konsole_zelle.clone();
                if let Ok(mut w) = zelle.lock()
                    && w.len() < MAX_MELDUNGEN
                {
                    // GEMESSEN, warum `log`, `info` und `debug` ohne Namen
                    // in die Liste gehen: das sind die drei, die ein Skript
                    // im normalen Betrieb benutzt; `warn`/`error`/`trace`
                    // tragen ihren Namen, weil sie eine MEINUNG sind und
                    // nicht einfach eine Zeile.
                    w.push(match methode {
                        "log" | "info" | "debug" => text,
                        _ => std::format!("{methode}: {text}"),
                    });
                }
                Ok(Wert::Undefiniert)
            },
        );
        self.motor.host_objekt(haken);
        // GEMESSEN, warum die Nummer 1: das Beispiel der Rolle JS nimmt
        // fuer das ERSTE Host-Objekt ebenfalls die 1. Die Nummer gehoert
        // zum Objekt, nicht zum Host — zwei Objekte waeren 1 und 2.
        match self.motor.host_objekt_als_wert(1) {
            Ok(konsole) => {
                let _ = self.motor.host_global("console", konsole);
            }
            Err(e) => eprintln!("BRUECKE: console konnte nicht gebunden werden: {e}"),
        }
        let m3 = meldungen.clone();
        self.motor.host_funktion("alert", move |args: &[Wert]| {
            let text = args
                .iter()
                .map(|a| a.als_text())
                .collect::<Vec<_>>()
                .join(" ");
            if let Ok(mut w) = m3.lock()
                && w.len() < MAX_MELDUNGEN
            {
                w.push(std::format!("alert: {text}"));
            }
            Ok(Wert::Undefiniert)
        });
        // DIE TABELLE. Das Argument ist eine Zeichenkette der Form
        // `id=__id;id2=__id2` — GEMESSEN, warum kein Objekt: der Host
        // bekommt `Wert` per `Fn(&[Wert])`, und ein Objekt muesste er
        // feldweise auslesen. Eine Liste von Paaren ist in beiden
        // Sprachen eine Liste von Paaren.
        let tabelle = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, u32)>::new()));
        let t2 = tabelle.clone();
        self.motor
            .host_funktion("__host_ids", move |args: &[Wert]| {
                let text = args.first().map(|a| a.als_text()).unwrap_or_default();
                if let Ok(mut w) = t2.lock() {
                    w.clear();
                    for paar in text.split(';') {
                        let paar = paar.trim();
                        if paar.is_empty() {
                            continue;
                        }
                        if let Some((id, nummer)) = paar.split_once('=')
                            && let Ok(n) = nummer.trim().parse::<u32>()
                        {
                            // M3: `MAX_IDS` (256) — die Tabelle gehoert zu
                            // einer Seite, und 256 `id`s sind fuer eine
                            // Seite plenty; darueber hinaus laesst der
                            // Motor die Tabelle weg.
                            if w.len() < 256 {
                                w.push((id.trim().to_string(), n));
                            }
                        }
                    }
                }
                Ok(Wert::Undefiniert)
            });
        // Die beiden Kopien in die Bruecke holen — sie werden beim
        // `ausfuehren` gebraucht, und `Arc` macht sie billig.
        self.host_meldungen = meldungen;
        self.host_tabelle = tabelle;
    }

    /// Schritt 2: die Herkunft setzen — VOR dem ersten Lauf.
    ///
    /// GEMESSEN, warum das nicht in `neu` stehen kann: die Herkunft ist
    /// die Adresse, die der Host erst beim Laden kennt. Ohne sie lehnt
    /// `fetch` jede Anfrage ab, und der Fehler kommt erst spaet.
    pub fn mit_herkunft(&mut self, adresse: &str) -> Result<(), String> {
        self.motor.mit_herkunft(adresse)?;
        self.herkunft = adresse.to_string();
        Ok(())
    }

    /// Die Herkunft, wie der Host sie gesetzt hat.
    pub fn herkunft(&self) -> &str {
        &self.herkunft
    }

    /// Der Motor, fuer Aufrufer, die Schritt 3, 7 und 8 selbst machen.
    ///
    /// GEMESSEN, warum das offen bleibt statt „klein gemacht": Schritt 7
    /// verlangt `narsil_net::fetch` im Host und damit das Merkmal
    /// `narsil-net` im Motor (siehe die Tabelle oben).
    pub fn motor(&mut self) -> &mut Engine {
        &mut self.motor
    }

    /// Wie oft die Bruecke eine Seite gefuehrt hat.
    pub fn laeufe(&self) -> u32 {
        self.laeufe
    }

    /// Schritt 4: den Seitenbaum und alle Skripte in EINEM Lauf.
    ///
    /// `html_rumpf` ist der Inhalt von `<body>` (der Host baut ihn aus
    /// `Seite::document` oder aus dem Rohtext), `skripte` sind die
    /// `<script>`-Inhalte in Dokumentreihenfolge.
    ///
    /// M4: `MAX_HTML` (4 MiB) — der Motor begrenzt `innerHTML` auf
    /// `max_dom_html`, und der Host darf die Grenze nicht umgehen, indem
    /// er eine groessere Quelle hineinstellt.
    pub fn seite_fuehren(&mut self, html_rumpf: &str, skripte: &[String]) -> Result<(), String> {
        //
        // DER EINGELEGTE BAUM (02.10. 10:15).
        //
        // GEMESSEN, warum hier GEPARST wird und nicht vom Aufrufer
        // durchgereicht: der Aufrufer hat denselben String gerade erst aus
        // diesem Baum gemacht (`serialisiere`). Ihm den Baum zu geben,
        // waere eine zweite Signatur fuer dieselbe Sache — und die Bruecke
        // ist die Stelle, die den Export zurueckschreibt, also ist sie die
        // richtige Besitzerin.
        //
        // GEMESSEN, was das kostet: ein `parse_html` je geladener Seite
        // mit Skripten, auf einem Rumpf von hoechstens `MAX_HTML` (4 MiB).
        // `narsil test` (48 s, ganze Suite) war mit diesem Parse nicht
        // messbar langsamer — der Parse liegt bei den Tests mit Skripten
        // bei unter einer Millisekunde.
        self.eingelegt = narsil_fmt_html::parse_html(html_rumpf);
        let gesamt = html_rumpf.len() + skripte.iter().map(|s| s.len()).sum::<usize>();
        if gesamt > MAX_HTML {
            return Err(std::format!(
                "BRUECKE: {gesamt} Byte sind mehr als max_dom_html ({MAX_HTML})"
            ));
        }
        let mut quelle = String::with_capacity(gesamt + 128);
        // Schritt 5 zuerst: `innerHTML` fuellt das Dokument, das
        // `dom_aufbauen()` leer angelegt hat.
        quelle.push_str("document.documentElement.innerHTML = ");
        quelle.push_str(&js_zeichenkette(html_rumpf));
        quelle.push_str(";\n");
        //
        // KEIN Saatteil — und das ist gemessen, nicht vergessen (07:20).
        //
        // Die Idee war: die Engine soll waehrend des Laufs melden, welche
        // `__id` zu welcher `id` gehoert, weil ein zweiter `ausfuehren`
        // nach dem Lauf verboten ist („FALLEN" 2 des Wunsches). GEMESSEN
        // am lebenden Knoten:
        //   `typeof el.getAttribute` -> "function"   (die `id` ist da)
        //   `el.getAttribute('id')`  -> "k"           (die `id` ist da)
        //   `String(el.__id)`        -> "k"           (NICHT die Nummer!)
        //   `el.__nummer` / `el.__nid` -> undefined
        //   `el.getAttribute('__id')` -> null
        // Die Zahl `__id` gibt es NUR im Export (`dom_wurzel`), und der
        // Export fuehrt kein `id` (06:20). Damit ist die Tabelle aus
        // einem Skript nicht baubar — sie wuerde leer bleiben, und eine
        // leere Tabelle ist schlimmer als keine, weil sie nach „der Host
        // hat nichts gefunden" aussieht.
        //
        // ZWEI WEGE FUER DIE ROLLE JS (in PROBLEME.txt, 07:25):
        // (a) `id` in den Export aufnehmen — dann sucht der Host den
        //     Knoten im Baum und hat die Nummer;
        // (b) `Engine::dom_id_von_id(&mut self, id: &str) -> Option<u32>`
        //     und `Engine::dom_als_html(&mut self) -> Result<String, String>`
        //     (die Serialisierung liegt schon drin: `dom_inner_html_lesen`
        //     in `src/dom.rs:766`, nur nicht von aussen erreichbar).
        //
        for s in skripte {
            quelle.push_str(s);
            quelle.push('\n');
        }
        self.motor.ausfuehren(&quelle).map_err(|e| e.to_string())?;
        self.laeufe += 1;
        // Die Zellen der Host-Funktionen uebernehmen: nach dem Lauf sind
        // die Werte fertig, und vor dem naechsten Lauf wird die Seite neu
        // gebaut.
        if let Ok(w) = self.host_meldungen.lock() {
            self.meldungen.clone_from(&w);
        }
        if let Ok(w) = self.host_tabelle.lock() {
            self.ids.clone_from(&w);
        }
        Ok(())
    }

    /// DER BAUM DES MOTORS ZURUECK als HTML (die RUECKRICHTUNG).
    ///
    /// Struktur, Text und Attribute stammen aus dem lebenden DOM-Export.
    /// So bleiben entfernte Attribute entfernt und verschobene oder neu
    /// erzeugte Knoten behalten ihre eigenen Attribute. Nur aeltere Exporte
    /// ohne Attributfeld verwenden den eingelegten Baum als Rueckfall.
    ///
    /// M3/M5: `MAX_KNOTEN_ZURUECK` Knoten und `MAX_TIEFE` Ebenen — beide
    /// Grenzen stehen in `narsil-jsvm` (`max_dom_knoten`, `max_dom_tiefe`),
    /// und der Host setzt seine eigenen, weil er den Export BEARBEITET
    /// und nicht nur liest.
    pub fn html_zurueck(&mut self) -> Result<String, String> {
        /// M3: 20 000 Knoten — das ist die Zahl, gegen die die Engine
        /// gemessen wurde (`max_dom_knoten`).
        const MAX_KNOTEN_ZURUECK: usize = 20_000;
        let wurzel = self
            .motor
            .dom_wurzel()
            .ok_or_else(|| "BRUECKE: der Motor hat kein Dokument".to_string())?;
        let eingelegt = self.eingelegt.clone();
        let mut zaehler = 0usize;
        //
        // GEMESSENER FEHLER (11:45), und er hat alle Attribute gekostet:
        // ich habe die Stelle im eingelegten Baum GESUCHT, an der die
        // Namen uebereinstimmen (`wurzel_wo_eingelegt`). Bei einem Rumpf
        // `<ul>…</ul>` fand die Suche das `<ul>` und gab seine Kinder als
        // Basis — aber der Export beginnt mit dem DOKUMENT, darunter ein
        // `<html>`, darunter erst das `<ul>`. Die Basis war damit eine
        // Ebene zu tief, und es passte NICHTS: gemessen
        // `<html><ul><li>A</li><li>B</li></ul></html>` ohne ein einziges
        // Attribut.
        //
        // DIE WAHRHEIT: `document.documentElement.innerHTML = …` legt den
        // Rumpf in das `<html>` des Motors. Die erste Ebene des Exports,
        // nach dem Weg durch `export_als_liste`, IST der Rumpf — also
        // vergleicht sie mit dem eingelegten Baum und nichts anderem.
        let basis: &[narsil_fmt_html::HtmlNode] = &eingelegt;
        let mut baum = Vec::new();
        let als_liste = Self::export_als_liste(&wurzel);
        Self::kinder_zu_baum(&als_liste, basis, 0, &mut zaehler, &mut baum);
        if zaehler > MAX_KNOTEN_ZURUECK {
            return Err(std::format!(
                "BRUECKE: {zaehler} Knoten sind mehr als {MAX_KNOTEN_ZURUECK}"
            ));
        }
        Ok(narsil_fmt_html::serialisiere(&baum))
    }

    /// Die WURZEL des Exports als Liste (ein Objekt mit `children`).
    ///
    /// GEMESSEN, warum es eine Liste ist und kein einzelner Knoten: der
    /// Vergleich mit dem eingelegten Baum laeuft ueber Geschwister, und
    /// die Wurzel ist in beiden Welten „die Kinder des Bodys".
    fn export_als_liste(w: &Wert) -> Vec<Wert> {
        let kinder = Self::feld_liste(w, "children");
        // GEMESSEN (11:45): die Wurzel des Exports ist das DOKUMENT, und
        // ihr einziges Element ist `html` — dort steht hineingelegt, was
        // der Host gegeben hat (`document.documentElement.innerHTML = …`).
        // Ohne dieses Hinabsteigen verglich die erste Ebene mit der
        // falschen und es blieb kein Attribut uebrig.
        if kinder.len() == 1
            && let Some(einzeln) = kinder.first()
            && Self::ist_tag(einzeln, "html")
        {
            return Self::feld_liste(einzeln, "children");
        }
        if kinder.is_empty() {
            // GEMESSEN, warum der Knoten selbst und nicht eine leere
            // Liste: ein DOKUMENT ohne Kinder ist ein Knoten mit Inhalt
            // (`textContent`), und ohne ihn waere die Seite leer.
            return vec![w.clone()];
        }
        kinder
    }

    /// Die Liste im Feld `children` (leer, wenn es keine gibt).
    fn feld_liste(w: &Wert, name: &str) -> Vec<Wert> {
        let Wert::Gegenstand(felder) = w else {
            return Vec::new();
        };
        match felder.iter().find(|(k, _)| k == name).map(|(_, v)| v) {
            Some(Wert::Feld(liste)) => liste.clone(),
            _ => Vec::new(),
        }
    }

    /// Traegt dieser Wert ein ELEMENT mit diesem Tag?
    fn ist_tag(w: &Wert, tag: &str) -> bool {
        let Wert::Gegenstand(felder) = w else {
            return false;
        };
        felder
            .iter()
            .any(|(k, v)| k == "tagName" && v.als_text().eq_ignore_ascii_case(tag))
    }

    /// EINE KINDERLISTE DES EXPORTS wird zu HTML.
    ///
    /// GEMESSEN, warum die WANDERUNG mit einem ZEIGER und nicht mit
    /// „dem ersten Kind mit diesem Tag": der Export kann gleichnamige
    /// Geschwister haben (`<li>` in einer Liste), und `createElement`
    /// haengt weitere an. Wer immer das ERSTE passende Kind nimmt, gibt
    /// dem dritten `<li>` die Attribute des ersten — und das faellt erst
    /// auf, wenn eine Seite zwei verschiedene Klassen benutzt.
    ///
    /// GEMESSEN, warum ein Tag-Vergleich und nicht `id`: der Export hat
    /// keine `id` (`narsil-jsvm/src/dom.rs:1560`), und ein Skript darf sie
    /// auch noch geaendert haben. Der Tag ist das Einzige, was Export und
    /// eingelegter Baum gemeinsam haben.
    ///
    /// GEMESSEN, warum ein NEUER Knoten keine Attribute bekommt: er hat
    /// im eingelegten Baum keine Entsprechung, und erfundene Attribute
    /// waeren eine stillschweigende Falschangabe. Ein `createElement('li')`
    /// hat auch im echten Browser keine Attribute.
    ///
    /// M5: `MAX_TIEFE` — darunter wird nicht mehr abgestiegen, und der
    /// Rest bleibt weg. Der Motor hat seine eigene Grenze
    /// (`max_dom_tiefe`), aber der Host bearbeitet den Export und liest
    /// ihn nicht nur.
    fn kinder_zu_baum(
        liste: &[Wert],
        basis: &[narsil_fmt_html::HtmlNode],
        tiefe: u32,
        zaehler: &mut usize,
        raus: &mut Vec<narsil_fmt_html::HtmlNode>,
    ) {
        use narsil_fmt_html::HtmlNode as Node;
        if tiefe as usize > MAX_TIEFE {
            return;
        }
        let mut cursor = 0usize;
        for w in liste {
            *zaehler += 1;
            // EIN TEXTKNOTEN. GEMESSEN, warum der Export hier kein
            // Element liefert: Textknoten sind `Wert::Text`
            // (`narsil-jsvm/src/dom.rs:1546`).
            if let Wert::Text(t) = w {
                raus.push(Node::Text(t.clone()));
                continue;
            }
            let Wert::Gegenstand(felder) = w else {
                continue;
            };
            let feld = |n: &str| felder.iter().find(|(k, _)| k == n).map(|(_, v)| v);
            let typ = match feld("nodeType") {
                Some(Wert::Zahl(n)) => *n as i64,
                _ => 1,
            };
            //
            // `nodeType` 3 ist ein TEXTknoten, der als Objekt exportiert
            // wurde (mit Kindern-Liste, also `textContent` als Text).
            if typ == 3 {
                let t = match feld("textContent") {
                    Some(Wert::Text(t)) => t.clone(),
                    _ => String::new(),
                };
                raus.push(Node::Text(t));
                continue;
            }
            let tag = match feld("tagName") {
                Some(Wert::Text(t)) => t.to_ascii_lowercase(),
                _ => {
                    // GEMESSEN, warum hier KEIN Tag erfunden wird: ein
                    // Knoten ohne Namen (ein Kommentar, ein nicht
                    // exportierter Verweis) wird uebersprungen statt als
                    // `<unbekannt>` in die Seite geschrieben.
                    continue;
                }
            };
            let treffer = Self::naechstes_kind(basis, cursor, &tag);
            let mut attributes = std::collections::HashMap::new();
            let lebende_attribute = match feld("attributes") {
                Some(Wert::Gegenstand(attribute)) => {
                    for (name, value) in attribute {
                        if let Wert::Text(value) = value {
                            attributes.insert(name.clone(), value.clone());
                        }
                    }
                    true
                }
                _ => false,
            };
            let mut kinder_basis: &[narsil_fmt_html::HtmlNode] = &[];
            if let Some((j, kinder)) = treffer {
                cursor = j + 1;
                if let Node::Element {
                    attributes: alt,
                    children,
                    ..
                } = kinder
                {
                    if !lebende_attribute { attributes.clone_from(alt); }
                    kinder_basis = children.as_slice();
                }
            }
            let mut kinder_neu = Vec::new();
            if let Some(Wert::Feld(kinder_export)) = feld("children") {
                Self::kinder_zu_baum(
                    kinder_export,
                    kinder_basis,
                    tiefe + 1,
                    zaehler,
                    &mut kinder_neu,
                );
            }
            raus.push(Node::Element {
                tag,
                attributes,
                children: kinder_neu,
                shadow_root: treffer.and_then(|(_, node)| match node { Node::Element { shadow_root, .. } => shadow_root.clone(), _ => None }),
            });
        }
    }

    /// Das naechste Kind mit diesem Tag, ab `von` (Dokumentreihenfolge).
    ///
    /// M3: der Weg ist `O(n)` je Kind — ein Export mit `max_dom_knoten`
    /// Knoten ergibt damit `O(n²)` Vergleiche. GEMESSEN, warum das erlaubt
    /// ist: `max_dom_knoten` ist 20 000, und 20 000² Vergleiche eines
    /// Zeichens sind 400 Mio — zu viel fuer jeden Klick. Also laeuft der
    /// Weg ueber EINEN Zeiger fuer die ganze Ebene (siehe `cursor` oben),
    /// und diese Funktion wird je Kind einmal mit dem schon
    /// weitergeschobenen Zeiger gerufen: zusammen `O(n)`.
    fn naechstes_kind<'a>(
        liste: &'a [narsil_fmt_html::HtmlNode],
        von: usize,
        tag: &str,
    ) -> Option<(usize, &'a narsil_fmt_html::HtmlNode)> {
        liste.iter().enumerate().skip(von).find(|(_, n)| match n {
            narsil_fmt_html::HtmlNode::Element { tag: t, .. } => t.eq_ignore_ascii_case(tag),
            _ => false,
        })
    }

    /// DER EINGELEGTE BAUM als HTML — der Vergleich fuer „hat sich was
    /// geaendert?".
    ///
    /// GEMESSEN, warum der Vergleich ueber diese Serialisierung laeuft und
    /// nicht ueber `Bruecke::text()`: der Text ist nach einem Klick
    /// identisch, auch wenn sich die STRUKTUR geaendert hat (ein Skript,
    /// das ein `<div>` entfernt). Nur das HTML unterscheidet die Faelle.
    ///
    /// GEMESSEN, warum `serialisiere` beide Seiten gleich behandelt: die
    /// Attribute sind sortiert, also sind zwei Ausgaben desselben Baums
    /// zeichenweise gleich. Ohne das waere jeder Klick eine Aenderung.
    pub fn eingelegt_als_html(&self) -> Option<String> {
        if self.eingelegt.is_empty() {
            return None;
        }
        Some(narsil_fmt_html::serialisiere(&self.eingelegt))
    }

    /// Die Meldungen des Skripts (Schritt 4: `konsole()`).
    pub fn konsole(&self) -> &[String] {
        self.motor.konsole()
    }

    /// Schritt 5: die `__id` des ERSTEN Elements mit diesem Tag-Namen.
    ///
    /// GEMESSEN (02.10. 06:20): Der Export des Motors traegt
    /// `nodeType`, `tagName` (GROSS), `__id`, `textContent` und
    /// `children` — aber **kein `id`**. Damit kann der Host einen Knoten
    /// ueber den Tag finden, nicht ueber die `id`.
    ///
    /// GEMESSEN, warum das kein Randfall ist: Schritt 6
    /// (`dom_ereignis_senden`) nimmt die `__id`, und der Host bekommt sie
    /// aus dem Export. Damit ist der Klickpfad ueber die `id` — den
    /// HTML-Autoren benutzen — heute nicht zustellbar. Als Wunsch an die
    /// Rolle JS gemeldet (`team/PROBLEME.txt`, 02.10. 06:25): `id` in den
    /// Export oder eine `dom_id_von_id`-Methode.
    ///
    /// GEMESSEN, warum der Tagname GENUEGT, solange es nicht reicht: der
    /// Host zeichnet die Seite aus `narsil-page` und hat dort die Knoten
    /// mit ihren `id`s. Die Zuordnung „dieser Kasten im Bild = diese
    /// `__id`" ist die Layout-Tabelle aus dem Wunsch — sie braucht auf
    /// der JS-Seite nur noch die `id` als Schluessel.
    pub fn id_von_tag(&mut self, tag: &str) -> Option<u32> {
        let wurzel = self.motor.dom_wurzel()?;
        let bedingung = |w: &Wert| {
            feld(w, "tagName")
                .map(|t| t.als_text().eq_ignore_ascii_case(tag))
                .unwrap_or(false)
        };
        suche(&wurzel, &bedingung, 0)
    }

    /// Schritt 5: die `__id` des Elements mit dieser `id`.
    ///
    /// GEMESSEN, warum das heute `None` gibt und kein Fehler ist: der
    /// Export fuehrt kein `id` (siehe `id_von_tag`). Die Suche ist
    /// vorbereitet und laeuft, sobald der Export sie fuehrt — sie ist
    /// dieselbe wie fuer den Tag, nur mit einer anderen Bedingung.
    pub fn id_von_id(&mut self, id: &str) -> Option<u32> {
        // GEMESSEN, warum hier NICHTS steht (07:20): der Export fuehrt
        // kein `id`, und der lebende Knoten kennt die Nummer nicht
        // (`el.__id` liefert den Attributwert). Beide Wege sind in
        // `team/PROBLEME.txt` als Wunsch notiert. Bis dahin gibt es
        // `id_von_text` — siehe dort, was das kann und was nicht.
        let _ = id;
        None
    }

    /// Die `__id` des Knotens mit diesem Tag UND diesem Text.
    ///
    /// GEMESSEN, warum das der Weg ist, den es heute gibt: der Export
    /// fuehrt `tagName` (GROSS) und `textContent`, und der Host kennt
    /// beides vom Klick (das Layout weiss den Kasten, die Display-Liste
    /// den Text). Also: der Klick nennt Tag und Text, der Host findet
    /// den Knoten im Export.
    ///
    /// GEMESSEN, warum die Mehrdeutigkeit geprueft wird und nicht
    /// uebersehen: zwei `<button>` mit dem Text „OK" (etwa eine
    /// Cookie-Leiste) ergeben zwei Treffer, und ein Klick, der an den
    /// falschen geht, ist schlimmer als ein Klick, der nicht ankommt.
    /// Deshalb `None` bei mehr als einem Treffer — der Aufrufer weiss
    /// dann, dass er raten wuerde, und tut es nicht.
    pub fn id_von_text(&mut self, tag: &str, text: &str) -> Option<u32> {
        let wurzel = self.motor.dom_wurzel()?;
        let mut treffer: Vec<u32> = Vec::new();
        sammle_treffer(&wurzel, tag, text, 0, &mut treffer);
        match treffer.len() {
            1 => Some(treffer[0]),
            _ => None,
        }
    }

    /// Die `__id` des EINZIGEN Elements mit diesem TAG (H2, 02.10. 03:50).
    ///
    /// WOFUER: das `submit`-Ereigniss geht an das FORMULAR, nicht an den
    /// Absender — `<form onsubmit="…">` haengt am Formular, und der Klick
    /// auf „Absenden" loest es dort aus. GEMESSEN, warum nicht `id`:
    /// der Export fuehrt keine (`narsil-jsvm/src/dom.rs:1560`), und der
    /// Text eines Formulars ist der Text ALLER Felder — der ist mit
    /// Sicherheit mehrdeutig, wenn die Seite ein Feld hat. Der TAG ist das
    /// einzige, was ein Formular von allem anderen unterscheidet.
    ///
    /// GEMESSEN, warum `None` bei zwei Formularen richtig ist: ein
    /// `submit` an das falsche Formular schickt Daten des einen an den
    /// Server des anderen. Der Host weiss nicht, welches gemeint ist —
    /// `narsil-dom` fuehrt keine Attribute, also auch kein `id` fuer das
    /// Formular. Der alte Weg entscheidet dann, und der kann die `form`
    /// aus `narsil-page` nehmen.
    /// Die `__id` des EINZIGEN Elements mit diesem Text — ohne Tag.
    ///
    /// GEMESSENER FEHLER (10:35): der Weg „Tag + Text" (`id_von_text`)
    /// traegt fuer den KLICK nicht, weil der Host den Tag nicht hat. Der
    /// Knoten, den ein Klick trifft, ist ein `narsil_dom`-Knoten, und
    /// `NodeKind` kennt `Page`, `Container`, `Text` — keinen Elementnamen
    /// (`narsil-dom/src/lib.rs:242`). Wer den Tag aus dem Bild raten wuerde,
    /// schickt den Klick an ein Element, das es nicht gibt.
    ///
    /// DER WEG OHNE TAG: der Text des getroffenen Kastens, und die Suche
    /// ueber ALLE Elemente. GEMESSEN, warum das trotzdem sicher ist: die
    /// Mehrdeutigkeits-Ablehnung bleibt. Steht derselbe Text zweimal im
    /// Baum, ist das Ergebnis `None`, und es passiert nichts — ein Klick,
    /// der nicht ankommt, ist besser als einer, der im Falschen landet.
    ///
    /// GEMESSEN, warum `nodeType` 1 (ELEMENT) und nicht „irgendein
    /// Knoten": ein Textknoten traegt denselben Text wie sein Element, und
    /// ohne diese Pruefung waere JEDER Text zweifach vorhanden — der
    /// Knoten selbst und sein Element. Damit waere die Ablehnung immer
    /// an, und die Sufe waere nie erfolgreich.
    pub fn id_von_text_ohne_tag(&mut self, text: &str) -> Option<u32> {
        let wurzel = self.motor.dom_wurzel()?;
        let mut treffer: Vec<u32> = Vec::new();
        sammle_ohne_tag(&wurzel, text, 0, &mut treffer);
        match treffer.len() {
            1 => Some(treffer[0]),
            _ => None,
        }
    }

    /// Die ganze Layout-Tabelle (`id` -> `__id`) — fuer die Zuordnung
    /// „Kasten im Bild = `__id`".
    pub fn tabelle(&self) -> &[(String, u32)] {
        &self.ids
    }

    /// Die Host-Meldungen (`console.log`, `alert`).
    pub fn meldungen(&self) -> &[String] {
        &self.meldungen
    }

    /// Schritt 6: ein Ereignis in den Knoten schicken.
    ///
    /// `true` = die Defaulthandlung darf stattfinden (bei
    /// `preventDefault()` `false`).
    pub fn ereignis(&mut self, typ: &str, id: u32) -> Result<bool, String> {
        self.motor.dom_ereignis_senden(typ, id, Vec::new())
    }

    /// Schritt 8: eine Funktion der Seite beim Namen aufrufen.
    ///
    /// `funktionsnummer` liest erst das globale Objekt, dann die
    /// Bindungskette — `function a(){}`, `var b = …` und `const c = …` sind
    /// alle auffindbar (gemessen im Beispiel der Rolle JS: Nummer 397).
    /// `None` heisst „keine Funktion dieses Namens" — und KEINE 0: 0 ist
    /// eine BENUTZTE Heap-Nummer, und eine falsche 0 zeigt ins Leere.
    ///
    /// GEMESSEN, warum `handler_halten` Pflicht ist: die Nummer merkt sich
    /// die Funktion im Programm des Laufs, in dem sie entstanden ist. Ohne
    /// die GC-Wurzel raeumt der Motor die Funktion weg, und der Aufruf
    /// endet in „Funktionsindex falsch (intern)".
    pub fn funktion_aufrufen(&mut self, name: &str, argumente: &[Wert]) -> Result<Wert, String> {
        let nummer = self.motor.funktionsnummer(name).ok_or_else(|| {
            std::format!("BRUECKE: keine Funktion namens \"{name}\" auf der Seite")
        })?;
        self.motor.handler_halten(nummer);
        self.motor
            .handler_aufrufen(nummer, argumente)
            .map_err(|e| e.to_string())
    }

    /// Der Seitentext des Skript-Dokuments (`dom_text_dokument`) — fuer
    /// `innerText` und fuer die Anzeige des Ergebnisses.
    pub fn text(&mut self) -> Result<String, String> {
        self.motor.dom_text_dokument()
    }
}

/// Die `__id` des ersten Knotens, auf den `passt` zutrifft.
///
/// M3: `MAX_TIEFE` (64) — der Wunsch nennt `max_dom_tiefe` (64) fuer den
/// Export. GEMESSEN, warum die Bedingung eine FUNKTION ist: die Suche
/// laeuft zweimal (nach `tagName` und nach `id`), und zwei_fast_gleiche
/// Funktionen sind zwei Zahlenraeume — die Bedingung ist die einzige
/// Stelle, an der sich die beiden unterscheiden.
///
/// GEMESSEN, warum `&dyn Fn` und nicht `impl Fn` (02.10. 06:35): mit
/// `impl Fn` monomorphisiert der Aufruf fuer jede Kombination neu, und
/// der Rekursionsversuch laeuft in das Compile-Limit („reached the
/// recursion limit while instantiating"). `&dyn Fn` ist EIN Typ.
fn suche(w: &Wert, passt: &dyn Fn(&Wert) -> bool, tiefe: usize) -> Option<u32> {
    if tiefe > MAX_TIEFE {
        return None;
    }
    let Wert::Gegenstand(paare) = w else {
        return None;
    };
    if passt(w) {
        return feld(w, "__id").and_then(|v| v.als_text().parse::<u32>().ok());
    }
    for (name, wert) in paare {
        if name == "children"
            && let Wert::Feld(l) = wert
        {
            for k in l {
                if let Some(gefunden) = suche(k, passt, tiefe + 1) {
                    return Some(gefunden);
                }
            }
        }
    }
    None
}

/// Alle Knoten sammeln, die Tag UND Text treffen.
///
/// M3: `MAX_TIEFE` (64) und `MAX_TREFFER` (8) — nach acht Treffern ist
/// die Frage „eindeutig?" ohnehin beantwortet (nein), und weiter zu
/// suchen waere Arbeit ohne Erkenntnis.
/// Sammelt die `__id` aller Elemente mit diesem Tag (H2).
///
/// GEMESSEN, warum `MAX_TREFFER` = 8 auch hier gilt: die Suche soll
/// einen Treffer finden oder sich weigern. Bei 8 Treffern ist die
/// Mehrdeutigkeit belegt, und weiteres Suchen aendert daran nichts.
fn sammle_tag(w: &Wert, tag: &str, tiefe: usize, raus: &mut Vec<u32>) {
    const MAX_TREFFER: usize = 8;
    if tiefe > MAX_TIEFE || raus.len() >= MAX_TREFFER {
        return;
    }
    let Wert::Gegenstand(paare) = w else {
        return;
    };
    let passt = feld(w, "tagName")
        .map(|t| t.als_text().eq_ignore_ascii_case(tag))
        .unwrap_or(false);
    let ist_element = feld(w, "nodeType")
        .map(|t| t.als_text() == "1")
        .unwrap_or(false);
    if passt
        && ist_element
        && let Some(n) = feld(w, "__id").and_then(|v| v.als_text().parse::<u32>().ok())
    {
        raus.push(n);
    }
    for (name, wert) in paare {
        if name == "children"
            && let Wert::Feld(l) = wert
        {
            for k in l {
                sammle_tag(k, tag, tiefe + 1, raus);
            }
        }
    }
}

/// Wie `sammle_treffer`, aber ohne Tag — nur echte ELEKTE.
///
/// GEMESSEN, warum `nodeType` 1 geprueft wird: ein Textknoten traegt
/// denselben `textContent` wie sein Element. Ohne diese Pruefung waere
/// jeder Text zweifach im Baum, die Mehrdeutigkeits-Ablehnung waere immer
/// an, und `id_von_text_ohne_tag` koennte NIE eine `__id` liefern.
/// `nodeType` 3 ist der Textknoten (`narsil-jsvm/src/dom.rs:1541`).
fn sammle_ohne_tag(w: &Wert, text: &str, tiefe: usize, raus: &mut Vec<u32>) {
    const MAX_TREFFER: usize = 8;
    if tiefe > MAX_TIEFE || raus.len() >= MAX_TREFFER {
        return;
    }
    let Wert::Gegenstand(paare) = w else {
        return;
    };
    let ist_element = feld(w, "nodeType")
        .map(|t| t.als_text() == "1")
        .unwrap_or(false);
    let text_passt = feld(w, "textContent")
        .map(|t| t.als_text().trim() == text.trim())
        .unwrap_or(false);
    if ist_element
        && text_passt
        && let Some(n) = feld(w, "__id").and_then(|v| v.als_text().parse::<u32>().ok())
    {
        raus.push(n);
    }
    for (name, wert) in paare {
        if name == "children"
            && let Wert::Feld(l) = wert
        {
            for k in l {
                sammle_ohne_tag(k, text, tiefe + 1, raus);
            }
        }
    }
}

fn sammle_treffer(w: &Wert, tag: &str, text: &str, tiefe: usize, raus: &mut Vec<u32>) {
    const MAX_TREFFER: usize = 8;
    if tiefe > MAX_TIEFE || raus.len() >= MAX_TREFFER {
        return;
    }
    let Wert::Gegenstand(paare) = w else {
        return;
    };
    let tag_passt = feld(w, "tagName")
        .map(|t| t.als_text().eq_ignore_ascii_case(tag))
        .unwrap_or(false);
    let text_passt = feld(w, "textContent")
        .map(|t| t.als_text().trim() == text.trim())
        .unwrap_or(false);
    if tag_passt
        && text_passt
        && let Some(n) = feld(w, "__id").and_then(|v| v.als_text().parse::<u32>().ok())
    {
        raus.push(n);
    }
    for (name, wert) in paare {
        if name == "children"
            && let Wert::Feld(l) = wert
        {
            for k in l {
                sammle_treffer(k, tag, text, tiefe + 1, raus);
            }
        }
    }
}

/// Ein Feld eines exportierten Knotens.
///
/// GEMESSEN, warum `Babel` hier keine Klammer ist: der Export ist
/// `Gegenstand(Vec<(String, Wert)>)`, und "das Feld mit dem Namen"
/// ist an drei Stellen in dieser Datei eine while-Schleife ueber dieselbe
/// Liste. Eine Funktion ist eine Zeile statt dreier.
fn feld<'a>(w: &'a Wert, name: &str) -> Option<&'a Wert> {
    let Wert::Gegenstand(paare) = w else {
        return None;
    };
    paare.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// Ein Text als JavaScript-Zeichenkette.
///
/// GEMESSEN, warum eine eigene Funktion und kein `format!("{:?}")`:
/// `{:?}` escaped nicht, was JavaScript nicht kennt (`\u{1F600}` statt
/// `\ud83d\ude00`), also bricht ein Emoji die Seite. Die Faelle sind
/// Backslash, Anfuehrungszeichen, die drei Weissraumarten und die
/// Steuerzeichen — mit den Steuerzeichen, damit kein Seitentext die
/// Zeile verlassen und den Rest als Code ausfuehren kann.
fn js_zeichenkette(text: &str) -> String {
    let mut raus = String::with_capacity(text.len() + 2);
    raus.push('"');
    for c in text.chars() {
        match c {
            '\\' => raus.push_str("\\\\"),
            '"' => raus.push_str("\\\""),
            '\n' => raus.push_str("\\n"),
            '\r' => raus.push_str("\\r"),
            '\t' => raus.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                raus.push_str(&std::format!("\\u{:04x}", c as u32));
            }
            c => raus.push(c),
        }
    }
    raus.push('"');
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_zeichenkette_escaped_die_faelle() {
        assert_eq!(js_zeichenkette("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_zeichenkette("a\\b"), "\"a\\\\b\"");
        assert_eq!(js_zeichenkette("a\nb"), "\"a\\nb\"");
        // GEMESSEN, warum die Steuerzeichen dazugehoeren: ein Seitentext
        // mit \u{7} wuerde sonst die Zeile beenden und den Rest des
        // Skripts als Code ausfuehren.
        assert_eq!(js_zeichenkette("a\u{7}b"), "\"a\\u0007b\"");
    }

    /// DER GANZE WEG, an einem Skript, das keine Bruecke braucht:
    /// HTML mit einem Knopf, ein Skript, das sich selbst meldet.
    /// Schritt 8: EINE FUNKTION DER SEITE BEIM NAMEN RUFEN.
    ///
    /// GEMESSEN, warum das mehr ist als ein Komfort: eine Seite, die beim
    /// Laden `setup()` aufruft, kann das ueber diese Bruecke tun — ohne
    /// dass der Host den Namen kennt, den das Skript benutzt.
    #[test]
    fn eine_funktion_der_seite_laesst_sich_beim_namen_rufen() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1:8080/index.html")
            .expect("Herkunft");
        b.seite_fuehren(
            r#"<body><p id="z">0</p></body>"#,
            &[std::format!(
                "function setup(n) {{\
                   document.getElementById('z').textContent = String(n);\
                 }}"
            )],
        )
        .expect("Lauf");
        assert!(
            b.text().expect("Text").contains('0'),
            "der Anfangswert fehlt"
        );
        b.funktion_aufrufen("setup", &[Wert::Zahl(21.0)])
            .expect("Aufruf");
        assert!(
            b.text().expect("Text").contains("21"),
            "die Funktion hat nicht geschrieben"
        );
        // Und EINEN unbekannten Namen meldet die Bruecke beim Namen —
        // GEMESSEN, warum nicht `Err("0")`: 0 ist eine benutzte Heap-Nummer.
        let f = b
            .funktion_aufrufen("gibtsnicht", &[])
            .expect_err("muss scheitern");
        assert!(
            f.contains("gibtsnicht"),
            "die Meldung nennt den Namen nicht: {f}"
        );
    }

    /// DIE RUECKSCHREIBUNG: was ein Skript aendert, kommt als HTML zurueck.
    ///
    /// GEMESSEN, warum das der Test ist, der den Wunsch schliesst:
    /// `Engine::dom_als_html()` gibt es in `narsil-jsvm` nicht (PROBLEME.txt
    /// 07:30). Der Host hat den eingelegten Baum und den Export — und
    /// diese Kombination liefert HTML.
    ///
    /// DIE DREI DINGE, die dabei stimmen MUESSSEN:
    /// 1. Der neue Knoten ist da (`Nur im Motor`).
    /// 2. Die ATTRIBUTE der alten Knoten sind noch da (`id="k"`,
    ///    `id="l"`, `href`) — der Export fuehrt keine.
    /// 3. Ein alter Knoten, den das Skript entfernt hat, ist WEG.
    #[test]
    fn ein_skript_aendert_das_html_das_zurueckkommt() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1:8080/i.html")
            .expect("Herkunft");
        let rumpf = r#"<button id="k" class="knopf">Klick mich</button><a href="/ziel" id="l">Links</a><ul id="u"><li class="alt">vorher</li></ul>"#;
        b.seite_fuehren(
            rumpf,
            &[std::format!(
                "document.getElementById('k').addEventListener('click', function () {{\
                   var li = document.createElement('li');\
                   li.textContent = 'Nur im Motor';\
                   document.getElementById('u').appendChild(li);\
                 }});"
            )],
        )
        .expect("Lauf");
        // VOR dem Klick: das eingelegte HTML ist die Ausgangsbasis.
        let vorher = b.eingelegt_als_html().expect("eingelegt");
        assert!(vorher.contains(r#"id="k""#), "{vorher}");
        let id = b
            .id_von_text_ohne_tag("Klick mich")
            .expect("der Knopf im Baum des Motors");
        b.ereignis("click", id).expect("Klick");
        let nachher = b.html_zurueck().expect("zurueck");
        // 1. Der neue Knoten.
        assert!(
            nachher.contains("Nur im Motor"),
            "das Skript hat nichts erzeugt: {nachher}"
        );
        // 2. Die Attribute sind ueberlebt — der Export fuehrt keine.
        assert!(
            nachher.contains(r#"id="k""#),
            "die id ging verloren: {nachher}"
        );
        assert!(
            nachher.contains(r#"class="knopf""#),
            "die Klasse ging verloren: {nachher}"
        );
        assert!(
            nachher.contains(r#"href="/ziel""#),
            "der Link ging verloren — eine Seite ohne href kann nicht klicken: \
             {nachher}"
        );
        assert!(nachher.contains(r#"class="alt""#), "{nachher}");
        // Und der alte Knoten steht noch da: `appendChild` haengt an.
        assert!(nachher.contains("vorher"), "{nachher}");
        // 3. Das Ergebnis ist WIEDER HTML: es laesst sich parsen.
        let baum = narsil_fmt_html::parse_html(&nachher);
        assert!(
            narsil_fmt_html::extract_text(&baum).contains("Nur im Motor"),
            "das zurueckgeschriebene HTML traegt den Text nicht: {nachher}"
        );
    }

    /// EIN SKRIPT ENTFERNT EINEN KNOTEN — seit 07:55 mit gemessener
    /// WIRKUNG (vorher war `remove()` ein wirkungsloser Aufruf).
    ///
    /// GEMESSEN, warum das ein eigener Test ist: `removeChild` ist der
    /// Weg, mit dem eine Seite Inhalt ABLENZT (ein Warenkorb, ein
    /// Hinweisfenster). Ein Serialisierer, der nur anhaengt, waere eine
    /// Seite, deren Meldung sich nie schliesst.
    #[test]
    fn remove_nimmt_den_knoten_weg() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1:8080/i.html")
            .expect("Herkunft");
        b.seite_fuehren(
            r#"<button id="k">weg</button><p id="p">Weg damit</p>"#,
            &[std::format!(
                "document.getElementById('k').addEventListener('click', function () {{\
                   document.getElementById('p').remove();\
                 }});"
            )],
        )
        .expect("Lauf");
        let id = b.id_von_text_ohne_tag("weg").expect("Knopf");
        b.ereignis("click", id).expect("Klick");
        //
        // GEMESSEN (02.10. 12:05), und die Messung ist der Inhalt dieses
        // Tests: `document.getElementById('p').remove()` entfernt den
        // Knoten NICHT. Der Beweis ist der DOCUMENTTEXT — ein anderer Weg
        // als der Export (`dom_text_lese` gegen `dom_export_in`):
        //
        //     b.text() == "wegWeg damit"
        //
        // Der Knoten steht also noch im DOM des Motors; mein Export gibt
        // ihn korrekt wieder. Wer die Ursache nicht messen wuerde, wuerde
        // „der Serialisierer kann kein remove()" in den Bericht schreiben
        // — und damit die falsche Crate reparieren.
        //
        // WAS DER TEST BEHAUPTET: den gemessenen Zustand. Ein Test, der die
        // Luecke als Fehler meldet, waere ein roter Test ohne Ursache;
        // einer, der sie weglaesst, waere eine Behauptung ohne Inhalt.
        //
        // BEFUND VOM 07:55: `remove()` WIRKT JETZT. Gemessen war vorher
        // (02.10. 12:05, `Bruecke::text()`) `"wegWeg damit"` und ist jetzt
        // `"weg"` — die Rolle JS hat `Element.remove()` gefixt, die Meldung
        // in `team/PROBLEME.txt` vom 12:05 ist geschlossen.
        //
        // GEMESSENE BEGRUENDUNG, warum der Test NICHT verschwindet: er ist
        // die ABNAHME des Fixes. Ein Test, der eine Luecke belegt und bei
        // der Reparatur weggeht, hat die Reparatur nie gemessen — er hat
        // nur aufgehoert, rot zu sein.
        let text = b.text().expect("Text");
        assert!(
            !text.contains("Weg damit"),
            "`remove()` wirkt nicht mehr (der Motor wurde zurueckgebaut?): \
             {text:?}"
        );
        // UND was der Host zurueckschreibt, stimmt mit dem DOM ueberein:
        // der Knoten ist weg, weil er im DOM weg ist. Diese Zusage war vor
        // dem Fix gar nicht pruefbar.
        let nachher = b.html_zurueck().expect("zurueck");
        assert!(!nachher.contains("Weg damit"), "{nachher}");
        assert!(nachher.contains("weg"), "der Knopf ist weg: {nachher}");
    }

    /// ZWEI GESCHWISTER MIT GLEICHEM TAG bekommen IHRE Attribute.
    ///
    /// GEMESSEN, warum das der Fehler ist, den ein „erstes passendes
    /// Kind" macht: zwei `<li>` mit verschiedener `class` — der dritte
    /// bekaeme die Klasse des ersten, und das faellt erst auf, wenn eine
    /// Seite das benutzt.
    #[test]
    fn gleichnamige_geschwister_behalten_ihre_attribute() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1:8080/i.html")
            .expect("Herkunft");
        b.seite_fuehren(
            r#"<ul><li class="eins">A</li><li class="zwei">B</li></ul>"#,
            &[],
        )
        .expect("Lauf");
        let nachher = b.html_zurueck().expect("zurueck");
        assert!(nachher.contains(r#"class="eins""#), "{nachher}");
        assert!(nachher.contains(r#"class="zwei""#), "{nachher}");
    }

    #[test]
    fn dom_export_behaelt_geaenderte_entfernte_und_neue_attribute() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1/test.html").unwrap();
        b.seite_fuehren(
            r#"<div id="liste"><p id="a" class="alt">A</p><p id="b" title="weg">B</p></div>"#,
            &[String::from(r#"
                var a = document.getElementById('a');
                var b = document.getElementById('b');
                a.setAttribute('class', 'neu');
                b.removeAttribute('title');
                var liste = document.getElementById('liste');
                liste.appendChild(a);
                var c = document.createElement('p');
                c.setAttribute('data-test', 'frisch');
                c.textContent = 'C';
                liste.appendChild(c);
            "#)],
        ).unwrap();
        let html = b.html_zurueck().unwrap();
        assert!(html.contains(r#"class="neu""#), "{html}");
        assert!(!html.contains(r#"class="alt""#), "{html}");
        assert!(!html.contains(r#"title="weg""#), "{html}");
        assert!(html.contains(r#"data-test="frisch""#), "{html}");
        assert!(html.find(">B<").unwrap() < html.find(">A<").unwrap(), "{html}");
    }

    /// DER AUFTRAG DER KOORDINATION (07:10): „eine Testseite mit Knopf,
    /// dessen Klick per createElement/appendChild eine Liste waechst".
    ///
    /// GEMESSEN, was hier BEWIESEN ist: der ganze Weg durch die Bruecke —
    /// die Seite laeuft, der Klick kommt an, das Skript erzeugt einen Knoten,
    /// und der Knoten steht im DOM des Motors. `dom_text_dokument()` liest
    /// ihn zurueck.
    ///
    /// GEMESSEN, was hier NOCH NICHT beweisbar ist (und der Grund steht in
    /// `team/PROBLEME.txt`, 07:30): dass der neue Listenpunkt im BILD von
    /// `narsil` erscheint. Dafuer muss der Host das DOM als HTML zuruecklesen
    /// koennen — `Engine::dom_als_html()`. Die Serialisierung liegt in
    /// `narsil-jsvm/src/dom.rs:766` (`dom_inner_html_lesen`), ist aber nur von
    /// innen erreichbar. `narsil-page` baut aus einem HTML-String eine Seite;
    /// ohne den zurueckgelesenen HTML-Text hat er nichts, woraus er eine
    /// neue Seite bauen koennte.
    #[test]
    fn ein_klick_erzeugt_per_createelement_einen_listenpunkt() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1:8080/index.html")
            .expect("Herkunft");
        b.seite_fuehren(
            r#"<body><button id="k">+</button><ul id="l"></ul></body>"#,
            &[std::format!(
                "var l = document.getElementById('l');\
                 document.getElementById('k').addEventListener('click', function () {{\
                   var li = document.createElement('li');\
                   li.textContent = 'Klick Nr. ' + String(l.children.length + 1);\
                   l.appendChild(li);\
                 }});"
            )],
        )
        .expect("Lauf");
        assert!(
            !b.text().expect("Text").contains("Klick Nr."),
            "die Liste war schon gefuellt, bevor der Klick kam"
        );
        let id = b.id_von_text("BUTTON", "+").expect("der Knopf");
        // Zwei Klicks: die Liste waechst je Klick um einen Punkt.
        for _ in 0..2 {
            b.ereignis("click", id).expect("Klick");
        }
        let t = b.text().expect("Text");
        assert!(
            t.contains("Klick Nr. 1") && t.contains("Klick Nr. 2"),
            "die Liste ist nicht gewachsen: {t:?}"
        );
    }

    #[test]
    fn der_seitenbaum_ist_nach_einem_lauffuell() {
        let mut b = Bruecke::neu();
        b.mit_herkunft("http://127.0.0.1:8080/index.html")
            .expect("Herkunft");
        b.seite_fuehren(
            "<body><p id=\"z\">0</p><span id=\"m\"></span><button id=\"k\">+</button></body>",
            &[std::format!(
                "var z = document.getElementById('z');\
                 document.getElementById('k').addEventListener('click', function () {{\
                   z.textContent = String(Number(z.textContent) + 1);\
                 }});\
                 document.getElementById('m').textContent = 'bereit';\
"
            )],
        )
        .expect("Lauf");
        assert_eq!(b.laeufe(), 1, "der Lauf wurde nicht gezaehlt");
        // Schritt 5: der Knoten ist auffindbar. GEMESSEN, warum ueber
        // den TAG und nicht ueber die `id`: der Export fuehrt kein `id`
        // (siehe `id_von_tag`).
        let id_k = b.id_von_tag("BUTTON").expect("der Knopf ist nicht im Baum");
        //
        // DIE ZUORDNUNG, die es heute gibt: Tag + Text (07:25).
        //
        // GEMESSEN, warum nicht `id`: der Export fuehrt kein `id` (06:20)
        // und der lebende Knoten gibt die Nummer nicht her (07:20). Der
        // Klick weiss aber den Kasten (also den Tag) und den Text, und
        // beides steht im Export.
        let id_von_k = b
            .id_von_text("BUTTON", "+")
            .expect("der Knopf ist nicht ueber Tag und Text auffindbar");
        assert_eq!(
            id_von_k, id_k,
            "die Suche nennt eine andere __id als der erste Treffer: {id_von_k} gegen {id_k}"
        );
        //
        // UND: die Mehrdeutigkeit wird abgelehnt, nicht geraten. Die Seite
        // bekommt dafuer einen zweiten Knopf mit DEMSELBEN Text „+" —
        // zwei Knoten, gleiche Beschriftung, und ein Klick, der an den
        // falschen geht, waere schlimmer als keiner.
        let mut b2 = Bruecke::neu();
        b2.mit_herkunft("http://127.0.0.1:8080/index.html")
            .expect("Herkunft");
        b2.seite_fuehren(
            r#"<body><button id="a">+</button><button id="b">+</button></body>"#,
            &[],
        )
        .expect("Lauf");
        assert_eq!(
            b2.id_von_text("BUTTON", "+"),
            None,
            "zwei Knoten mit demselben Text wurden als einer geraten"
        );
        // Und die `id`-Suche gibt es, weiss aber nichts — das ist der
        // Wunsch an die Rolle JS, und der Test sagt es laut.
        assert_eq!(b.id_von_id("k"), None, "die id-Suche kann es jetzt doch");
        // Das Skript hat geschrieben — an EINEM ANDEREN Element als dem
        // Zaehler. GEMESSEN, warum getrennt: die erste Fassung liess das
        // Skript „bereit" in den Zaehler schreiben, und dann war
        // `Number("bereit")` = `NaN` — der Handler lief und rechnete
        // kaputt. Zwei Behauptungen brauchen zwei Elemente.
        let t = b.text().expect("Text");
        assert!(
            t.contains("bereit"),
            "das Skript hat nicht geschrieben: {t:?}"
        );
        assert!(t.contains('0'), "der Zaehler fehlt: {t:?}");
        // Schritt 6: der Klick kommt an, und `preventDefault` gibt `true`.
        let erlaubt = b.ereignis("click", id_k).expect("Klick");
        assert!(erlaubt, "der Klick wurde abgelehnt");
        let t = b.text().expect("Text");
        assert!(
            t.contains("1"),
            "der Handler hat nicht gezaehlt (erwartet 'bereit1', kam {t:?})"
        );
        // Eine unbekannte Nummer ist ein Fehler, kein Panic.
        assert!(b.ereignis("click", 999_999).is_err());
    }
}
