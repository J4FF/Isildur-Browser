//! HTTP/2-Rahmen für den **Client** (RFC 9113).
//!
//! added 01.10.2026 09:0x UTC (Projektlabel 02.10.2026), Rolle TLS.
//! Vorgang: A2 aus `AUTONOM.txt` („HTTP/2 im CLIENT (narsil-net) mit ALPN h2").
//!
//! DER ANLASS, und er war eine Messung: `narsil-net` hatte nach R4u einen
//! HPACK-**Decoder** und nach R4v einen -**Encoder**, aber keine einzige
//! Zeile, die daraus einen `HEADERS`-Rahmen macht. Ohne Rahmen kann der
//! Client keine Anfrage über h2 stellen — das war die Lücke, an der A2
//! hing.
//!
//! WARUM EINE EIGENE DATEI und kein `lib.rs`: die Kodierung ist ein
//! **Bitmuster** (24 Bit Länge, 8 Bit Typ, 1 Bit Flags, 31 Bit Strom-ID),
//! und das gehört nicht in eine Datei, die HTTP/1.1-Aufbautrag trägt.
//! `narsil-server/src/h2.rs` macht es genauso — das ist der Vorbild, nicht
//! die Quelle: hier steht **nur**, was ein **Client** braucht.
//!
//! DIE KORREKTHEIT, und sie kommt nicht von mir, sondern aus RFC 9113:
//! * Abschnitt 4.1: der Rahmenkopf ist **9 Byte**, in dieser Reihenfolge.
//! * Abschnitt 4.1: die Länge ist 24 Bit und zählt **nicht** den Kopf.
//! * Abschnitt 4.3: die Strom-Identität ist **31 Bit**, Bit 31 ist
//!   reserviert und muss beim Empfang ignoriert werden.
//! * Abschnitt 6.5.2: `MAX_FRAME_SIZE` erlaubt bis 2^24-1, aber nur,
//!   wenn beide Seiten es gesetzt haben.
//! * Abschnitt 4.1: ein Stream-Byte darf **nie** in einen anderen Strom
//!   wandern — das ist der Angriff „WINDOW_UPDATE auf den falschen Strom".

/// Der Rahmenkopf ist **9 Byte** (RFC 9113 Abschnitt 4.1).
///
/// DER WERT 9, und er ist keine Rechenentscheidung: die Felder sind
/// 24 + 8 + 1 + 32 = 65 Bit, und 65 Bit passen nicht in 8 Byte. Mit dem
/// reservierten Bit bleiben 64 nutzbare Bit plus das reservierte selbst —
/// also genau 9 Byte.
pub const KOPF_BYTES: usize = 9;

/// M4: die größte Länge, die ein **einzelner** Rahmen haben darf.
///
/// DER WERT 16 384: RFC 9113 Abschnitt 6.5.2 nennt
/// `SETTINGS_MAX_FRAME_SIZE` mit einem Vorgabewert von 16 384 Byte. Wir
/// setzen ihn im eigenen SETTINGS (siehe [`EIGENE_SETTINGS`]) und
/// akzeptieren nichts Größeres.
///
/// WARUM NICHT 2^24-1, obwohl die Norm es erlaubt: ein Rahmen darf nie
/// größer sein als das Fenster, das der Sender uns zugesteht, und 16 KiB
/// ist der Wert, den `nghttp2` (in `curl`) ebenfalls nutzt. Ein Server,
/// der 16 MiB in einen Rahmen presst, würde bei uns einen Puffer dieses
/// Formats belegen — das ist der Preis einer großzügigen Grenze.
pub const MAX_RAHMEN_BYTES: usize = 16 * 1024;

/// M3/M4: die kleinste Länge, die ein Rahmen haben darf.
///
/// DER WERT 0: RFC 9113 Abschnitt 4.1 erlaubt einen leeren Rahmen, und
/// `SETTINGS` mit leerer Nutzlast ist der **erste** Rahmen einer Sitzung
/// (Abschnitt 3.5). Wer eine Untergrenze von 1 setzte, könnte keine
/// Sitzung eröffnen.
pub const MIN_RAHMEN_BYTES: usize = 0;

/// Unsere `SETTINGS`, als erster Rahmen der Sitzung.
///
/// added 01.10.2026 09:0x, Rolle TLS. Jede Zeile hat einen Grund:
/// * `ENABLE_PUSH = 0` — RFC 9113 Abschnitt 6.5.2: ein Client **darf**
///   Push abschalten, und wir wollen keinen (es gibt keine API dafür).
/// * `MAX_CONCURRENT_STREAMS = 100` — mehr Strome, als ein Browser
///   braucht; begrenzt, was eine Verbindung bindet.
/// * `INITIAL_WINDOW_SIZE = 1 MiB` — größer als der Vorgabewert
///   (65 535), damit eine Seite nicht am Fenster hängen bleibt.
/// * `MAX_FRAME_SIZE = 16 384` — der Vorgabewert, siehe
///   [`MAX_RAHMEN_BYTES`].
/// added 04.10.2026, Rolle NETZTEMPO. **EIN `Link`-HINWEIS** — RFC 8288,
/// und die Felder, die ein Browser fuer den **Zeitpunkt** braucht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkHinweis {
    /// Die `rel`, **klein** geschrieben (die Norm erlaubt Gross- und
    /// Kleinschreibung, und `Rel=Preload` ist dasselbe wie `rel=preload`).
    pub rel: String,
    /// Die Zieladresse, **unveraendert** — sie wird **nicht** aufgeloest, weil
    /// die Aufloesung eine Basis braucht, und die hat der Aufrufer.
    pub ziel: String,
    /// `as=script`/`as=style`/… (nur bei `preload`).
    pub art: Option<String>,
    /// `crossorigin` — **ohne** sie ist die Antwort fuer ein Modulskript
    /// unbrauchbar, weil CORS dann greift (RFC 8288 3.3).
    pub crossorigin: bool,
}

/// added 04.10.2026, Rolle NETZTEMPO. **`Link`-KOPF IN HINWEISE ZERLEGEN** —
/// und die drei Faelle, an denen ein `split(',')` scheitert.
///
/// **DIE DREI FAELLE, und jeder davon ist in freier Wildbahn vorgekommen:**
/// 1. **Komma in einer URL**: `</a.css?v=1,2>; rel=preload` — ein `split(',')`
///    macht daraus **zwei** kaputte Werte.
/// 2. **Komma in einem Attributwert**: `as="script,module"`.
/// 3. **`rel` mit mehreren Werten**: `rel="preload stylesheet"`.
///
/// **DER WEG, und er ist ein Zeichenzustandsautomat:** Wir laufen **einmal**
/// ueber den Text und merken uns, ob wir in `<…>`, `"…"` oder `(…)` stehen.
/// Nur **ausserhalb** davon trennt ein Komma.
pub fn link_hinweise(text: &str) -> Vec<LinkHinweis> {
    let mut raus = Vec::new();
    for stueck in link_stuecke(text) {
        let stueck = stueck.trim();
        if stueck.is_empty() {
            continue;
        }
        let (ziel_teil, parameter) = match stueck.find(';') {
            Some(i) => (&stueck[..i], &stueck[i + 1..]),
            None => (stueck, ""),
        };
        let ziel = ziel_teil
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        if ziel.is_empty() {
            continue;
        }
        let mut rel = String::new();
        let mut art = None;
        let mut crossorigin = false;
        for p in parameter.split(';') {
            let p = p.trim();
            let (name, wert) = match p.find('=') {
                Some(i) => (
                    p[..i].trim().to_ascii_lowercase(),
                    p[i + 1..].trim().trim_matches('"').to_string(),
                ),
                None => (p.to_ascii_lowercase(), String::new()),
            };
            match name.as_str() {
                // **`rel` KANN MEHRERE WERTE haben** (`preload stylesheet`),
                // und wir nehmen den **ersten** — der Hinweis gilt fuer alle.
                "rel" if rel.is_empty() && !wert.is_empty() => {
                    rel = wert
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                }
                "as" => art = Some(wert.to_ascii_lowercase()),
                "crossorigin" => crossorigin = true,
                _ => {}
            }
        }
        if rel.is_empty() {
            continue;
        }
        raus.push(LinkHinweis {
            rel,
            ziel: ziel.to_string(),
            art,
            crossorigin,
        });
    }
    raus
}

/// added 04.10.2026: **DIE STUECKE, und warum kein `split(',')`.** Siehe den
/// Kommentar an `link_hinweise` — die drei Faelle. Der Automat zaehlt **drei**
/// offene Stellen: `<…>`, `"…"` und `(…)`.
fn link_stuecke(text: &str) -> Vec<String> {
    let mut raus = Vec::new();
    let mut aktuell = String::new();
    let mut in_winkel = false;
    let mut in_anfuehrung = false;
    let mut in_klammer = false;
    for z in text.chars() {
        match z {
            '<' => {
                in_winkel = true;
                aktuell.push(z);
            }
            '>' => {
                in_winkel = false;
                aktuell.push(z);
            }
            '"' => {
                in_anfuehrung = !in_anfuehrung;
                aktuell.push(z);
            }
            '(' => {
                in_klammer = true;
                aktuell.push(z);
            }
            ')' => {
                in_klammer = false;
                aktuell.push(z);
            }
            ',' if !in_winkel && !in_anfuehrung && !in_klammer => {
                raus.push(std::mem::take(&mut aktuell));
            }
            _ => aktuell.push(z),
        }
    }
    if !aktuell.trim().is_empty() {
        raus.push(aktuell);
    }
    raus
}

pub const EIGENE_SETTINGS: [(u16, u32); 5] = [
    // added 01.10.2026 20:7x, R5c. **`HEADER_TABLE_SIZE` FEHLTE**, und
    // das war kein Schönheitsfehler.
    //
    // GEMESSEN: `www.mozilla.org` lieferte über h2
    //     HTTP/2: HPACK: DynamischeTabelleZuGross (4116)
    // 4 116 ist mehr als unsere Tabelle (4 096). Grund: ohne
    // `HEADER_TABLE_SIZE` nimmt ein Server nach RFC 9113 6.5.2 den
    // **Vorgabewert 65 536** an — unsere stille Grenze nennt ihm
    // nirgends etwas, also füllt er 4 116 Byte hinein und wir lehnen ab.
    //
    // DIE ZAHL 4096 ist der Vorgabewert nach RFC 7541 4.2, also aendern wir
    // am Ende gar nichts — wir sagen nur, was wir ohnehin fuer richtig
    // halten. Wer sie anhebt, muss `hpack::MAX_DYNAMISCHE_TABELLE` mit
    // anheben, sonst dreht man den Fehler nur auf die andere Seite.
    (0x0001, 4096),    // HEADER_TABLE_SIZE = 4 KiB (Vorgabewert)
    (0x0002, 0),       // ENABLE_PUSH = 0
    (0x0003, 100),     // MAX_CONCURRENT_STREAMS
    (0x0004, 1 << 20), // INITIAL_WINDOW_SIZE = 1 MiB
    (0x0005, 16384),   // MAX_FRAME_SIZE
];

/// added 02.10.2026 18:0x, R38. **DIE ANGEKUENDIGTE FENSTERGROESSE, AUS
/// DERSELBEN TABELLE.** Der Grund ist ein gemessener Widerspruch: `h2client`
/// startete seine Fensterbuchfuehrung mit `65_535`, obwohl `EIGENE_SETTINGS` fuer
/// `INITIAL_WINDOW_SIZE` etwas anderes ankündigt. Beide Werte zu pflegen ist die
/// Art von Fehler, die erst beim naechsten Ausfall sichtbar wird, also steht die
/// Zahl ab hier **nur noch hier**.
///
/// added 02.10.2026 18:0x: **`None`, wenn die Kennung fehlt**, und nicht
/// `65_535` als Ersatz: ein stiller Ersatzwert waere wieder eine zweite Zahl,
/// die von niemandem gepflegt wird. Der Aufrufer entscheidet dann, was ohne
/// Anzeige gilt — bei uns ist das der Protokollausgangswert aus 6.9.2.
pub fn eigenes_fenster() -> Option<u32> {
    // added 04.10.2026, Rolle NETZTEMPO: **aus `eigene_settings()`**, nicht aus
    // `EIGENE_SETTINGS` — sonst waere der Preset-Schalter eine Attrappe: die
    // Ankuendigung gaendert sich, die **geprueft**e Zahl nicht, und der Fehler
    // waere wieder der von NETZTEMPO_12 (Ankuendigung und Wirklichkeit
    // auseinander).
    eigene_settings()
        .iter()
        .find(|(kennung, _)| *kennung == 0x04)
        .map(|(_, wert)| *wert)
}
/// added 04.10.2026, Rolle NETZTEMPO. **FIREFOX' WERTE, als Tabelle.** Und sie
/// sind **gemessen**, nicht aus dem Gedächtnis: der Auftrag der Leitung war
/// „Firefox: HEADER_TABLE_SIZE 65536, INITIAL_WINDOW_SIZE 131072", und die Zahl
/// auf der Leitung wird in `h2client::schreibe_vorlauf` **ausgegeben**, damit
/// man sie mit dem echten Firefox-Antrag vergleichen kann.
///
/// **WARUM DIE TABELLE UND KEINE KONSTANTE:** die Frage der Leitung war ein
/// **A/B-Vergleich**, und ein A/B-Vergleich braucht **zwei** Werte an **einer**
/// Stelle. Mit einer Konstante und einem `if` im Aufrufer waeren es zwei
/// Stellen, und die Zahlen laufen auseinander — das ist die Form von Fehler,
/// die in `h2client` schon einmal zwei Tage gedauert hat (Fenster 65 535 gegen
/// `EIGENE_SETTINGS`).
pub const FIREFOX_SETTINGS: [(u16, u32); 5] = [
    (0x0001, 65536),  // HEADER_TABLE_SIZE
    (0x0002, 0),      // ENABLE_PUSH
    (0x0003, 100),    // MAX_CONCURRENT_STREAMS
    (0x0004, 131072), // INITIAL_WINDOW_SIZE
    (0x0005, 16384),  // MAX_FRAME_SIZE
];

/// added 04.10.2026: **DER SCHALTER, und er ist ein `AtomicU8` im Prozess** —
/// nicht etwa eine Env-Variable, weil `werkzeuge/bau.sh` ueber
/// `systemd-run --user --scope` laeuft und ein Scope eine **minimale**
/// Umgebung bekommt (GEMESSEN in `tests/phasen_wohin_live.rs`:
/// `NARSIL_H2_TRACE=1` vor `bau.sh` ergibt **null** Spurzeilen). Und `set_var`
/// ist hier verboten (`-F unsafe-code`).
///
/// **UND WARUM ER NICHT `unsafe` IST:** `AtomicU8` ist `Sync` und braucht kein
/// `unsafe`; wer hier eines braucht, ist fast immer ein Schalter, der
/// gleichzeitig eine **Verbindung** mitten im Betrieb umstellt.
static PRESET: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// added 04.10.2026: **PRESET A** — unsere Werte (siehe [`EIGENE_SETTINGS`]).
pub const PRESET_STANDARD: u8 = 0;
/// added 04.10.2026: **PRESET B** — Firefox' Werte.
pub const PRESET_FIREFOX: u8 = 1;

/// added 04.10.2026: **UMSTELLEN**, mit Preset-Nummer **und** Namen, damit ein
/// Fehlschlag in der Ausgabe sagt, **welche** Variante lief.
pub fn preset_setzen(p: u8) {
    PRESET.store(p, std::sync::atomic::Ordering::Relaxed)
}

/// added 04.10.2026: **DER NAME DES AKTUELLEN PRESETS** — und er wird **in
/// jeder Messzeile** mitgeschrieben, weil eine A/B-Zahl ohne Angabe, **welche**
/// Variante lief, eine Zahl ohne Aussage ist.
pub fn preset_name() -> &'static str {
    match PRESET.load(std::sync::atomic::Ordering::Relaxed) {
        PRESET_FIREFOX => "B/Firefox",
        _ => "A/Standard",
    }
}

/// added 04.10.2026: **DIE ANKUENDIGUNG, je Preset** — die **eine** Stelle, an
/// der beide Varianten stehen. `eigenes_fenster()` liest **hier**, damit die
/// angekuendigte Zahl und die **geprueft**e nicht auseinanderlaufen koennen.
pub fn eigene_settings() -> &'static [(u16, u32)] {
    if PRESET.load(std::sync::atomic::Ordering::Relaxed) == PRESET_FIREFOX {
        &FIREFOX_SETTINGS
    } else {
        &EIGENE_SETTINGS
    }
}

/// added 04.10.2026: **DAS VERBINDUNGSFENSTER, je Preset.** Das ist die zweite
/// Zahl des Vergleichs, und sie wird **nicht** aus `SETTINGS` gelesen, weil
/// RFC 9113 6.9 sie nur per `WINDOW_UPDATE` auf Strom 0 aendert.
///
/// GEMESSEN, welche Zahlen auf der Leitung stehen (Ausgabe der Spur in
/// `schreibe_vorlauf`): wir **8 323 073** = 8 MiB - 65 535, Firefox
/// **12 517 377** = 12 MiB - 65 535. Der Ahaupt-Durchsatz-Unterschied ist also
/// **4 MiB** Verbindungsfenster und **768 KiB** Stromfenster.
pub const STANDARD_VERBINDUNGSFENSTER: u32 = 8 * 1024 * 1024;
/// added 04.10.2026: **12 MiB** — `65_535 + 12_517_377`, die Zahl, die Firefox
/// schickt.
pub const FIREFOX_VERBINDUNGSFENSTER: u32 = 12_582_912;

/// added 04.10.2026: **DIE ZAHL, die gerade gilt.**
pub fn verbindungsfenster() -> u32 {
    if PRESET.load(std::sync::atomic::Ordering::Relaxed) == PRESET_FIREFOX {
        FIREFOX_VERBINDUNGSFENSTER
    } else {
        STANDARD_VERBINDUNGSFENSTER
    }
}

/// Die Client-Vorfahrt, die nach dem TLS-Handshake kommen muss.
///
/// RFC 9113 Abschnitt 3.4: **24 Byte**, exakt. Sie wird **vor** jedem
/// Rahmen gesendet; wer sie weglässt, redet ab da an ein anderes Protokoll
/// (HTTP/1.1 über TLS, was ein h2-Server nicht spricht).
///
/// DER BELEG, dass 24 stimmt: die Datei ist wörtlich geschrieben, und
/// `PRIORITAET.len() == 24` wird im Test geprüft — nicht behauptet.
pub const PRIORITAET: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// M3: so viele Rahmen liest eine Sitzung höchstens, ohne dass etwas
/// antwortet.
///
/// DER WERT 10 000: das ist eine **Rechenentscheidung** und steht in
/// keinem RFC. Ein Browser öffnet für 30 Seiten vielleicht 60 Strome und
/// bekommt einige hundert Rahmen; 10 000 ist das Zehnfache und lässt
/// Platz für Seiten, die viele Ressourcen nachladen. Ohne diese Grenze
/// wäre ein Server, der endlos `PING` schickt, ein Endlosleser.
pub const MAX_RAHMEN: usize = 10_000;

/// M3: so viele `SETTINGS` je Sitzung.
///
/// DER WERT 20: ein Browser schickt **eines**. RFC 9113 Abschnitt 6.5.3
/// sagt nichts zur Zahl. 20 lässt Platz für einen Server, der seine
/// Einstellungen in mehreren Blöcken schickt, und begrenzt eine Kette.
pub const MAX_SETTINGS: usize = 20;

/// M4: so viele Byte nimmt ein einzelner `SETTINGS`-Block an.
///
/// DER WERT 4096: RFC 9113 Abschnitt 6.5.3 erlaubt beliebig viele Paare,
/// aber jedes Paar ist 6 Byte, also sind 4096 Byte rund 680 Paare. Das ist
/// weit mehr, als ein Server sinnvoll schickt, und begrenzt die
/// Allokation.
pub const MAX_SETTINGS_BYTES: usize = 4096;

/// M3: so viele Bytes dürfen **ohne** Rahmenkopf im Puffer liegen.
///
/// DER WERT 16384: `PRIORITAET` hat 24 Byte, ein `HEADERS`-Block eines
/// typischen Anfragkopfes etwa 60. Der Wert fasst beides mit Reserve und
/// ist benannt, weil `lies_rahmen` sonst über einen Frame hinauslesen
/// könnte.
pub const MAX_PUFFER_BYTES: usize = 16 * 1024;

/// Die Rahmentypen (RFC 9113 Abschnitt 11).
///
/// DER WERT 5 für `PUSH_PROMISE`: RFC 9113 Abschnitt 6.6 sagt, dass ein
/// Client `ENABLE_PUSH = 0` setzt und dann **niemals** ein
/// `PUSH_PROMISE` bekommt. Der Wert steht trotzdem da, damit ein
/// unerwarteter Rahmen benannt werden kann statt als Zahl.
/// Ist der **vollstaendige** HTTP/2-Clientweg gebaut?
///
/// added 01.10.2026 12:5x, R4z-NACHRAGE. DIE EINE WAHRHEIT, und sie ist
/// hier, weil sie eine Eigenschaft des **h2-Moduls** ist und nicht des
/// Aufrufers.
///
/// GEMESSEN, warum es diese Funktion geben muss: am 01.10.2026 12:3x
/// bot `narsil-net` `[b"h2", b"http/1.1"]` an, waehrend der Anfragenteil
/// fehlte. Folge: `github.com` waehlte `h2` (GEMESSEN), und
/// `fetch` lieferte `H2NochNichtAusgebaut` — die Seite lud **gar
/// nicht**, waehrend sie vorher geladen hatte.
///
/// WARUM EINE FUNKTION UND NICHT EINE KONSTANTE IM AUFRUFER: ein `false`
/// an zwei Aufrufstellen ist zwei Wahrheiten fuer dieselbe Frage. Hier
/// steht sie einmal, und das Angebot folgt ihr.
///
/// WAS DRIN STEHEN MUSS, damit hier `true` steht (RFC 9113 3.4, 6.2):
///  1. der Client-Preface (`PRIORITAET`),
///  2. ein `SETTINGS`-Rahmen,
///  3. `HEADERS` mit `END_HEADERS` (R4y: `baue_headers`),
///  4. `END_STREAM` und der Koerper,
///  5. **und die Antwort**: `SETTINGS`-Bestaetigung, `HPACK`-Dekodierung
///     der Antwortkopfzeilen, `DATA` bis `END_STREAM`.
///
/// Der letzte Punkt ist der, der fehlt: 1 bis 4 kann man schreiben, aber
/// ohne 5 weiss der Browser die Seite nicht.
///
/// added 01.10.2026 12:5x: **`const fn`**, weil der Aufrufer sie in einer
/// `const` braucht — das Angebot ist eine Konstante, und eine Konstante,
/// die zur Laufzeit aus einer Funktion kommt, waere wieder eine zweite
/// Wahrheit (siehe Kommentar dort).
pub const fn weg_gebaut() -> bool {
    //
    // added 01.10.2026 14:3x, R4z7 stand hier `false` — und der Grund war
    // ein gemessener Befund: von sechs echten Servern lieferten zwei
    // **keine** Seite, weil die HPACK-Dynamiktabelle im Client fehlte.
    //
    // added 01.10.2026 16:2x, R4z8: **`true`**, und der Grund ist wieder
    // ein gemessener Befund, diesmal der andere.
    //
    // WAS BIS R4z8 FEHLTE, und es waren **drei** Dinge, nicht eines:
    //  1. die HPACK-Dynamiktabelle im Dekodierer (`DynamischeTabelle`
    //     wurde in `narsil-net` nirgends konstruiert),
    //  2. die **Indexbreite** der Literalformen: 6.2.1 hat **sechs** Bit
    //     (RFC 7541 6.2.1), gelesen wurden vier; 6.2.2/6.2.3 haben vier.
    //     GEMESSEN: `github.com` lieferte 20 Felder mit **richtigen Werten
    //     und falschen Namen** — 0x61 ist Index 33 (`date`), gelesen wurde
    //     1 (`:authority`),
    //  3. die Zahlen-Kodierung: die Siebenergruppen gehen von **unten**
    //     nach oben, und das erste Oktett traegt das Continuation-Bit.
    //     GEMESSEN: `133` wurde als `0x81 0x05` geschrieben statt
    //     `0xff 0x06` — und die Testsuite fand das nicht, weil Encoder und
    //     Dekodierer **denselben** Fehler in gespiegelter Richtung hatten.
    //     1 754 Rundenproben bestanden auf einem Tautologie.
    //
    // Dazu kam die **Flusskontrolle**: ohne eigenes `WINDOW_UPDATE` blieb
    // die Verbindung nach 65 535 Byte stehen. GEMESSEN: `github.com`
    // lieferte 70 697 Byte, `www.google.com` 66 458, und beide brachen ab.
    //
    // GEMESSEN am 01.10.2026 16:1x, dreimal hintereinander, ueber
    // `narsil_net::fetch` (die Funktion, die die Probe ruft):
    //     github.com             OK 200  576 123 Byte
    //     archlinux.org          OK 200   25 523 Byte
    //     news.ycombinator.com   OK 200   34 462 Byte
    //     www.wikipedia.org      OK 200   93 955 Byte
    //     www.mozilla.org        OK 200   47 774 Byte
    //     www.google.com         OK 200   84 344 Byte
    // `h2_verbindungen` steigt dabei je Lauf, also wird h2 wirklich
    // ausgehandelt und benutzt.
    //
    // WAS ICH NICHT BEHAUPTEN KANN: die Abnahme „per **probe** laden"
    // (AUTONOM.txt Zeile 538) war von mir **nicht** messbar —
    // `werkzeuge/bau.sh narsil-probe build --example probe` ist rot an
    // fremden Crates (`narsil/src/dienste.rs:112`, `narsil/src/laden.rs:138`).
    // Ich habe stattdessen `narsil_net::fetch` gemessen, also genau die
    // Funktion, die die Probe benutzt, und das ausdruecklich als **Ersatz**
    // benannt, nicht als probe-Ergebnis.
    //
    // DER EINZEILIGE SCHALTER, falls die Probe etwas sieht, das `fetch`
    // nicht sieht: dieses `true` auf `false` setzen. Der Rueckschritt
    // kostet nichts — der Weg bleibt gebaut und getestet, es wird nur
    // nicht mehr angeboten. Genau diese Bremse hat am 01.10.2026 12:5x
    // eine Regression beendet, und sie ist billig.
    true
}
/// Bieten wir `h2` ueberhaupt an?
///
/// added 01.10.2026 12:5x, R4z-NACHRAGE. Fuer Tests und Berichte: das ist
/// die Behauptung ueber den **Zustand des Angebots**, und sie ist erst
/// durch die Aenderung an `h2::weg_gebaut()` falsch zu machen.
pub const fn h2_angeboten() -> bool {
    weg_gebaut()
}

pub mod typen {
    pub const DATA: u8 = 0x0;
    pub const HEADERS: u8 = 0x1;
    pub const PRIORITY: u8 = 0x2;
    pub const RST_STREAM: u8 = 0x3;
    pub const SETTINGS: u8 = 0x4;
    pub const PUSH_PROMISE: u8 = 0x5;
    pub const PING: u8 = 0x6;
    pub const GOAWAY: u8 = 0x7;
    pub const WINDOW_UPDATE: u8 = 0x8;
    pub const CONTINUATION: u8 = 0x9;
}

/// Die Flags eines Rahmens, als Bitmuster.
///
/// RFC 9113 Abschnitt 6: die Bits sind **stabil** und dürfen nicht neu
/// belegt werden — ein Empfänger **muss** unbekannte Flags ignorieren
/// (Abschnitt 4.1). Deshalb wird hier kein `reserviert`-Feld geprüft.
pub mod flags {
    /// `HEADERS`: END_STREAM (0x1), END_HEADERS (0x4).
    pub const END_STREAM: u8 = 0x1;
    /// `HEADERS`: END_HEADERS (0x4).
    pub const END_HEADERS: u8 = 0x4;
    /// `HEADERS`: PRIORITY (0x20).
    pub const PRIORITY: u8 = 0x20;
    /// `SETTINGS`: ACK (0x1).
    pub const ACK: u8 = 0x1;
    /// `DATA`: END_STREAM (0x1).
    pub const DATA_END_STREAM: u8 = 0x1;
    /// `DATA`: PADDED (0x8).
    ///
    /// added 01.10.2026 09:0x, Rolle TLS: `PADDED` ist **einmal** hier
    /// definiert und nicht zweimal. Es stand oben schon bei `HEADERS` —
    /// und Rust meldet das als Fehler, was gut ist: zwei Namen fuer
    /// denselben Wert an zwei Stellen sind zwei Wahrheiten, und eine
    /// davon kann sich aendern, ohne dass die andere es merkt.
    pub const PADDED: u8 = 0x8;
}

/// M4: so viele Byte hat der Prioritätsblock eines `HEADERS` mit `PRIORITY`.
///
/// added 02.10.2026 02:0x, R5f. **Fuenf**, und das ist gemessen an der Norm
/// (RFC 9113 6.2: 31 Bit Stream-Dependency, 1 Bit reserviert, 8 Bit Gewicht),
/// nicht geraten. Die Zahl ist eine Grenze, weil `daten` sonst fuenf Byte
/// eines Rahmens lesen wuerde, die es nicht gibt.
pub const PRIITAET_BYTES: usize = 5;

/// Die **Daten** eines Rahmens: ohne Pad-Länge, ohne Prioritätsblock.
///
/// added 02.10.2026 02:0x, R5f. DIE EINE STELLE, an der `PADDED` und
/// `PRIORITY` abgezogen werden, weil die Regeln an **drei** Rahmenarten
/// hängen und an drei Stellen zu wiederholen die Art von Fehler ist, die
/// R4z7 („`WINDOW_UPDATE` auf Strom 0 ist verboten") gerade behoben hat.
///
/// DIE NORM woertlich (RFC 9113 6.1):
///     "A PADDED frame ... the first octet of the frame payload contains
///      the length of the padding ... the padding octets MUST be included
///      in the frame length but MUST be excluded from the application data."
/// und 6.2 für `PRIORITY` bei `HEADERS`: fünf Byte „stream dependency and
/// weight" gehören **nicht** zum HPACK-Block.
///
/// GEMESSEN, wofuer (02.10.2026 02:0x, `www.google.com` über h2, und
/// deshalb erst jetzt sichtbar): **jedes** `DATA`-Rahmens des Servers kommt
/// mit `flags 0x08`, also `PADDED`:
///
/// ```text
/// MESSUNG DATA: vorher 0     neu 14026 flags 0x08 erste_neu [9f, 1f, 8b, …]
/// MESSUNG DATA: vorher 14026 neu 179  flags 0x08 erste_neu [1f, 10, …]
/// MESSUNG DATA: vorher 14205 neu 14893 flags 0x08 erste_neu [a1, c4, …]
/// MESSUNG DATA: vorher 29098 neu 0    flags 0x01 erste_neu []
/// MESSUNG gunzip: len 29098 erste [9f, 1f, 8b, 08, …]
/// ```
///
/// Das erste Byte jedes Blocks ist die Pad-Länge, nicht Inhalt. Ohne
/// Abzug stand ein Muellbyte je Block im Body — drei auf dieser Seite.
/// Das war bis hierher **nicht sichtbar**, weil der Body unkomprimiert
/// ankam und der HTML-Zerleger ein Muellbyte vor einem `<` schluckt. Sobald
/// der Client `accept-encoding` schickt (R5f, gleiche Runde), kommt der
/// Body komprimiert und `gunzip` sagt laut: „gzip-Kopf ungueltig: Magic
/// Bytes fehlen".
pub fn daten(rahmen: &Rahmen) -> Result<&[u8], RahmenFehler> {
    let mut rest: &[u8] = &rahmen.nutzlast;
    // 6.1: das erste Byte der Nutzlast ist die Pad-Laenge.
    let pad = if rahmen.flags & flags::PADDED != 0 {
        let (erste, uebrige) = rest
            .split_first()
            .ok_or(RahmenFehler::KopfZuKurz { war: 0 })?;
        rest = uebrige;
        *erste as usize
    } else {
        0
    };
    // 6.2: bei HEADERS mit PRIORITY folgen fuenf Byte, die nicht zum
    // HPACK-Block gehoeren.
    if rahmen.typ == typen::HEADERS && rahmen.flags & flags::PRIORITY != 0 {
        if rest.len() < PRIITAET_BYTES {
            return Err(RahmenFehler::KopfZuKurz { war: rest.len() });
        }
        rest = &rest[PRIITAET_BYTES..];
    }
    // 6.1: „MUST NOT exceed" — die Pad-Bytes muessen im Rahmen stehen. Ein
    // Server, der mehr Pad angibt als Nutzlast, hat einen Rahmen geschickt,
    // den es nicht gibt; das ist nach 6.1 ein `FRAME_SIZE_ERROR`.
    if pad > rest.len() {
        return Err(RahmenFehler::PolsterZuGross {
            pad,
            daten: rest.len(),
        });
    }
    Ok(&rest[..rest.len() - pad])
}

/// Ein Fehler aus dem Netz, benannt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RahmenFehler {
    /// Der Kopf war kürzer als [`KOPF_BYTES`].
    KopfZuKurz { war: usize },
    /// Die Verbindung endete, **bevor** überhaupt ein Rahmen kam.
    ///
    /// added 01.10.2026 21:1x, R5d. **EIGENE FORM**, und sie ersetzt eine
    /// Falschmeldung: der Aufrufer gab hier `KopfZuKurz { war: 0 }` zurueck,
    /// und das Display sagte „der Rahmenkopf hat 0 Byte, er braucht 9". Der
    /// Kopf war nicht zu kurz — es kam **keiner**, weil die andere Seite
    /// die Verbindung geschlossen hat.
    ///
    /// WARUM DAS WIRKLICH WICHTIG IST: nur an dieser Stelle kann der Aufrufer
    /// erkennen, dass eine **gepoolte** Verbindung zwischen der Prüfung
    /// (`lebt_noch()`) und dem Lesen gestorben ist — und genau dann ist ein
    /// Neuaufbau richtig, während ein echter Protokollfehler kein
    /// Wiederholen verdient. Aus „Kopf zu kurz" war das nicht erkennbar.
    ///
    /// GEMESSEN, wofür: `www.mozilla.org`, `tests/h2_live.rs`:
    ///     HTTP/2: der Rahmenkopf hat 0 Byte, er braucht 9
    KeinRahmen,
    /// Der Server hat die Verbindung **ordentlich** beendet (`GOAWAY`).
    ///
    /// added 01.10.2026 23:5x, R5b. RFC 9113 Abschnitt 6.8: „The GOAWAY
    /// frame applies to the connection, not a stream." Ein `GOAWAY` ist
    /// damit die **ehrliche** Form von „die Leitung ist tot" — und die
    /// Form, an der der Aufrufer den Neuaufbau erkennt.
    ///
    /// GEMESSEN, warum das keine Feinheit ist, sondern der ganze
    /// T4-Nachweis (01.10.2026 23:4x, `tests/t4_live_tmp.rs` gegen
    /// `www.google.com`, drei Abrufe hintereinander):
    ///     Lauf 0: status 200, h2 1, pool gesamt 1
    ///     Lauf 1: FEHLER HTTP/2: der Rahmenkopf hat 0 Byte, er braucht 9
    /// Der Server hatte auf die gepoolte Verbindung ein `GOAWAY` geschickt,
    /// und `lies_antwort` gab dafuer `KopfZuKurz { war: 0 }` zurueck — weil
    /// der Puffer leer war. Die Meldung war **falsch**: es gab keinen zu
    /// kurzen Rahmenkopf, sondern eine beendete Verbindung. Und weil die
    /// Meldung kein `KeinRahmen` war, sprang der Neuaufbau aus R5d **nicht**
    /// an — der zweite Abruf schlug fehl, statt neu zu verbinden.
    ///
    /// `letzter_strom` ist die `Last-Stream-ID` aus der Nutzlast (6.8): der
    /// hoechste Strom, den der Server noch verarbeitet hat. `u32::MAX` heisst
    /// „unbekannt", weil der Server sie weglassen darf.
    ServerBeendet { letzter_strom: u32, grund: u32 },
    /// Ein `CONTINUATION` kam ohne vorheriges offenes `HEADERS`.
    ///
    /// added 01.10.2026 17:1x, R5b. RFC 9113 Abschnitt 6.8: „A CONTINUATION
    /// frame MUST be preceded by a HEADERS frame that has not yet been
    /// terminated by an END_HEADERS flag."
    ///
    /// WARUM EINE EIGENE FORM und nicht `StromVerworfen`: dort heisst die
    /// Meldung „der Server hat den Strom geschlossen", und das ist etwas
    /// voellig anderes als „der Server hat die Reihenfolge der Rahmen
    /// gebrochen". Zwei verschiedene Fehler, eine Zahl — das ist die Art von
    /// Sammelform, die man beim Debuggen zuerst falsch liest.
    KopfblockVerworfen { strom: u32 },
    /// Die Verbindung endete, **bevor** der Strom zu war.
    ///
    /// added 01.10.2026 15:7x, R4z8. DIESE FORM WAR BISHER NICHT DA, und
    /// ihr Fehlen hat eine **Falschmeldung** erzeugt: der Aufrufer gab
    /// `KopfZuKurz { war: 0 }` zurueck, und das Display sagte „der
    /// Rahmenkopf hat 0 Byte, er braucht 9". Der Kopf war aber gar nicht
    /// zu kurz — der Server hatte die Antwort **angefangen** und nicht
    /// beendet.
    ///
    /// GEMESSEN, gegen welche echten Seiten das auftritt (01.10.2026,
    /// 15:7x, `tests/h2_live.rs`, `weg_gebaut() == true`):
    ///     github.com      FEHLER Rahmenkopf hat 0 Byte   (579 865 Byte)
    ///     www.google.com  FEHLER Rahmenkopf hat 0 Byte   ( 84 293 Byte)
    ///     archlinux.org   OK 200                           ( 25 460 Byte)
    ///     news.ycombinator.com OK 200                      ( 34 587 Byte)
    ///     www.mozilla.org  OK 200                          ( 47 774 Byte)
    /// Genau die beiden groesseren als 65 535 scheitern, und 65 535 ist
    /// unser Verbindungsfenster, das wir nie erhoehen.
    ///
    /// `strom` nennt den Strom, `gelesen` die bisher empfangenen Byte
    /// Nutzlast — damit die Meldung die Zahl nennt, an der man es sieht.
    VerbindungEndetVorzeitig { strom: u32, gelesen: usize },
    /// Die Länge im Kopf ist größer als [`MAX_RAHMEN_BYTES`].
    ZuGross { laenge: usize, erlaubt: usize },
    /// Die Länge passt nicht in 24 Bit.
    LaengeUeberlauf { war: usize },
    /// Es kamen mehr Rahmen, als [`MAX_RAHMEN`] erlaubt.
    ZuVieleRahmen { waren: usize, erlaubt: usize },
    /// `SETTINGS` war größer als [`MAX_SETTINGS_BYTES`].
    SettingsZuGross { war: usize, erlaubt: usize },
    /// Mehr `SETTINGS`, als [`MAX_SETTINGS`] erlaubt.
    ZuVieleSettings { waren: usize, erlaubt: usize },
    /// Ein `SETTINGS`-Paar hatte keine 6 Byte.
    SettingsUnvollstaendig { war: usize },
    /// added 02.10.2026 16:7x UTC (Projektlabel 02.10.2026), Rolle TLS:
    /// **PROTOCOL_ERROR**, und die Form fehlte, obwohl die Norm sie an zwei
    /// Stellen verlangt: 6.5.2 nennt fuer `SETTINGS_ENABLE_PUSH` und fuer
    /// `SETTINGS_MAX_FRAME_SIZE` je „a connection error (Section 5.4.1) of type
    /// PROTOCOL_ERROR“. Ohne eine eigene Form musste ich eine bestehende
    /// umdeuten (`Unbekannt` ist fuer unbekannte **Rahmentypen**, `SettingsZuGross`
    /// fuer unsere **eigene** Grenze) — und eine umgedeutete Form ist eine
    /// falsche Aussage, die beim naechsten Fehler in die Irre fuehrt.
    /// `grund` ist `String` und kein `&'static str`, weil hier der **Ort** im
    /// Text steht (siehe Nr. 38: dieselbe Entscheidung wie bei
    /// `SchreibenFehlgeschlagen`).
    ProtokollVerletzung { grund: String },
    /// added 02.10.2026 18:0x, R38. **`FLOW_CONTROL_ERROR`**, und die Form
    /// fehlte, obwohl die Norm den Fall benennt: 6.9.1 woertlich „The sender
    /// **MUST NOT** send a flow-controlled frame with a length that exceeds
    /// the space available in either of the flow-control windows advertised by
    /// the receiver.“ Bis hierhin haben wir eine Ueberschreitung **nie bemerkt**:
    /// `h2client` zog die Nutzlast von einem `fenster` ab und verglich es mit
    /// nichts.
    ///
    /// added 02.10.2026 18:0x: **WARUM EINE EIGENE FORM** und keine umgedeutete.
    /// `ZuGross` ist unsere **eigene** Grenze (`MAX_RAHMEN_BYTES`, 16 384, eine
    /// Rahmenlaenge) — das ist eine andere Sache als ein Fenster. `Protokoll-
    /// Verletzung` ist eine **Verbindung**, kein Strom, und traegt keinen Ort.
    /// `fenster` steht **vor** der Meldung, damit der Text sagt, welches der
    /// beiden Fenster gerissen ist: `strom: 0` heisst Verbindung.
    FensterUeberschritten {
        strom: u32,
        daten: usize,
        fenster: i64,
    },
    /// Ein Kennzeichen, das die Spezifikation nicht kennt.
    Unbekannt { war: u8 },
    /// Ein Strom-Byte, das die Norm verbietet (etwa `SETTINGS` auf einem
    /// Strom ungleich 0).
    StromVerboten { typ: u8, strom: u32 },
    /// Ein Schreiben auf die Verbindung ist fehlgeschlagen.
    ///
    /// added 01.10.2026 14:1x, R4z7. **EIGENE VARIANTE, und der Grund
    /// ist eine Falschmeldung.** Bis hierher habe ich jeden
    /// `write_all`-Fehler als `ZuGross { laenge: puffer.len(), erlaubt:
    /// puffer.len() }` gemeldet — das ergab die Meldung „der Rahmen ist
    /// 33 Byte lang, erlaubt sind 33", die **nichts** erzaehlt. GEMESSEN
    /// genau so, gegen `tests/alpn_client.rs`: ein Schreibfehler auf
    /// einer geschlossenen Leitung sah aus wie eine Laengenverletzung.
    ///
    /// Wer einen Schreibfehler als Laengenfehler liest, sucht die Ursache
    /// an der falschen Stelle — und genau das hat in R4z zwei Runden
    /// gekostet.
    ///
    /// added 02.10.2026 16:3x (Projektlabel 02.10.2026), Rolle TLS: **`grund`
    /// ist von `&'static str` auf `String` geworden**, und der Grund ist eine
    /// **Messung**, keine Formfeile: `tests/alpn_client.rs` meldet
    /// „HTTP/2: Schreiben auf der Verbindung fehlgeschlagen: die Verbindung nimmt
    /// keine weiteren Daten an“ — und **zehn** Schreibstellen in `h2client.rs`
    /// erzeugen genau diesen Text. Ohne Ortsangabe ist nicht zu ersehen, **welche**
    /// Stelle gerissen ist; `PROBLEME.txt` Nr. 30 laeuft daran, dass drei
    /// Hypothesen nacheinander widerlegt werden mussten, weil alle drei denselben
    /// Text erzeugen. Der Ort steht jetzt **vor** der Ursache im Text.
    ///
    /// added 02.10.2026 16:3x: **UND DAS IST EINE OEFFENTLICHE FORM** — wer
    /// ausserhalb von `narsil-net` auf **diesen** `RahmenFehler` matcht oder das
    /// Feld baut, muss das neu bauen. GEMESSEN am 02.10.2026
    /// (`grep -rn "RahmenFehler::" crates/ --include=*.rs` ohne `narsil-net/`):
    /// **19** Fundstellen, **alle** in `narsil-quic/src/rahmen.rs` — und das ist
    /// ein **eigenes** `RahmenFehler`, dort in Zeile 155 definiert;
    /// `narsil-quic` haengt nicht von `narsil-net` (GEMESSEN: kein `narsil_net` in
    /// seiner `Cargo.toml`). Ich hatte zuerst „null Fundstellen“ notiert, das war
    /// falsch, und die Zahl steht hier, damit sie nicht noch einmal falsch wird.
    /// Als **Aenderungsforderung**: `PROBLEME.txt` Nr. 38.
    SchreibenFehlgeschlagen { grund: String },
    /// Der Server hat den Strom mit `RST_STREAM` beendet.
    ///
    /// added 01.10.2026 14:0x, R4z7. EIGENE VARIANTE, und der Grund ist
    /// eine **Information**, die in `StromVerboten` verloren geht: der
    /// Fehlercode des `RST_STREAM` (RFC 9113 7). `REFUSED_STREAM` und
    /// `INTERNAL_ERROR` sagen voellig verschiedene Dinge — der erste ist
    /// „deine Anfrage war zu gross", der zweite „ich habe einen Fehler
    /// gemacht". Beide als „Strom verboten" zu melden hiesse: der
    /// Browser zeigt einen Fehler an, der nichts erklaert.
    ///
    /// `code` ist die 32-Bit-Zahl aus der Nutzlast; 0, wenn der Server
    /// keine schickte (das ist verboten, aber ein Server darf es
    /// versuchen — und dann soll der Fehler den Namen sagen).
    StromVerworfen { strom: u32, code: u32 },
    /// `PADDED`: die Pad-Laenge ist groesser als die Nutzlast daneben.
    ///
    /// added 02.10.2026 02:0x, R5f. RFC 9113 6.1: „A receiver MUST treat
    /// the receipt of a PADDED frame ... where the length of the padding
    /// is greater than the length of the frame payload ... as a connection
    /// error of type `FRAME_SIZE_ERROR`." Eigene Form, weil „Kopf zu kurz"
    /// die Richtung falsch benennt: hier ist zu viel da, nicht zu wenig.
    PolsterZuGross { pad: usize, daten: usize },
    /// Die Nutzlast war nicht UTF-8, wo sie es sein muss.
    KeinUtf8 { wo: &'static str },
    /// Ein Fehler aus der HPACK-Dekodierung; die Ursache bleibt
    /// benannt (added 01.10.2026, R4y).
    HpackFehler(HpackFehlerWeiter),
}

impl std::fmt::Display for RahmenFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RahmenFehler::KopfblockVerworfen { strom } => {
                write!(
                    f,
                    "ein CONTINUATION auf Strom {strom} ohne vorheriges \
                     HEADERS ohne END_HEADERS (RFC 9113 6.10)"
                )
            }
            RahmenFehler::VerbindungEndetVorzeitig { strom, gelesen } => {
                write!(
                    f,
                    "die Verbindung endete, bevor Strom {strom} zu war; \
                     {gelesen} Byte waren da"
                )
            }
            RahmenFehler::KeinRahmen => {
                write!(f, "die Verbindung endete, bevor ein Rahmen kam")
            }
            // added 01.10.2026 23:5x, R5b: der Name steht im Text, weil
            // „der Server beendet die Verbindung" etwas voellig anderes
            // bedeutet als „ein Rahmen war zu kurz" — und genau diese
            // Verwechslung hat den Lauf 1 oben gekippt.
            RahmenFehler::ServerBeendet {
                letzter_strom,
                grund,
            } => {
                // added 01.10.2026 23:7x: siehe den Grund in `lib.rs` —
                // E0716, wenn der Wert direkt in `write!` erzeugt wird.
                let gemeldet = if *letzter_strom == u32::MAX {
                    "unbekannt".to_string()
                } else {
                    letzter_strom.to_string()
                };
                write!(
                    f,
                    "der Server beendet die Verbindung (GOAWAY, letzter Strom \
                     {gemeldet}, Grund {grund})"
                )
            }
            RahmenFehler::KopfZuKurz { war } => {
                write!(f, "der Rahmenkopf hat {war} Byte, er braucht {KOPF_BYTES}")
            }
            RahmenFehler::ZuGross { laenge, erlaubt } => write!(
                f,
                "der Rahmen ist {laenge} Byte lang, erlaubt sind {erlaubt}"
            ),
            RahmenFehler::LaengeUeberlauf { war } => {
                write!(f, "{war} Byte passen nicht in 24 Bit")
            }
            RahmenFehler::ZuVieleRahmen { waren, erlaubt } => write!(
                f,
                "{waren} Rahmen gelesen, erlaubt sind {erlaubt} (der Server \
                 antwortet nicht)"
            ),
            RahmenFehler::SettingsZuGross { war, erlaubt } => write!(
                f,
                "der SETTINGS-Block hat {war} Byte, erlaubt sind {erlaubt}"
            ),
            RahmenFehler::ZuVieleSettings { waren, erlaubt } => {
                write!(f, "{waren} SETTINGS-Rahmen, erlaubt sind {erlaubt}")
            }
            RahmenFehler::SettingsUnvollstaendig { war } => {
                write!(f, "der letzte SETTINGS-Eintrag hat {war} Byte statt 6")
            }
            RahmenFehler::Unbekannt { war } => {
                write!(f, "unbekannter Rahmentyp {war:#04x}")
            }
            RahmenFehler::StromVerboten { typ, strom } => {
                write!(f, "Rahmen {typ:#04x} auf verbotenem Strom {strom}")
            }
            // added 01.10.2026 14:0x, R4z7: der Name des Codes kommt aus
            // `h2strom::fehler_name`, damit die **eine** Namensliste gilt.
            // Eine zweite Liste hier wuerde zwei Wahrheiten fuer dieselbe
            // Tabelle sein — und die Namen der RFC 9113 7 sind nicht
            // zu raten.
            // added 01.10.2026 14:1x, R4z7: die Meldung nennt die
            // **Ursache des Schreibens**, nicht eine Laenge.
            RahmenFehler::FensterUeberschritten {
                strom,
                daten,
                fenster,
            } => {
                write!(
                    f,
                    "HTTP/2 FLOW_CONTROL_ERROR: {daten} Byte auf Strom {strom}, \
                     es sind nur {fenster} im Fenster"
                )
            }
            RahmenFehler::ProtokollVerletzung { grund } => {
                write!(f, "HTTP/2 PROTOCOL_ERROR: {grund}")
            }
            RahmenFehler::SchreibenFehlgeschlagen { grund } => {
                write!(f, "Schreiben auf die Verbindung fehlgeschlagen: {grund}")
            }
            RahmenFehler::StromVerworfen { strom, code } => write!(
                f,
                "der Server hat den Strom {strom} verworfen: {}",
                crate::h2strom::fehler_name(*code)
            ),
            RahmenFehler::PolsterZuGross { pad, daten } => write!(
                f,
                "PADDED: {pad} Fuellbytes, aber nur {daten} Byte Nutzlast"
            ),
            RahmenFehler::KeinUtf8 { wo } => write!(f, "{wo} ist kein UTF-8"),
            RahmenFehler::HpackFehler(h) => match h.zahl {
                Some(z) => write!(f, "HPACK: {} ({z})", h.was),
                None => write!(f, "HPACK: {}", h.was),
            },
        }
    }
}

/// Der HPACK-Fehler als **eigener** Fall im Rahmenfehler.
///
/// added 01.10.2026 10:2x UTC (Projektlabel 02.10.2026), R4y.
///
/// WARUM KEIN `from` UND KEINE TEXTUMWANDLUNG: mein erster Entwurf war
/// `KeinUtf8 { wo: Box::leak(...) }` — und `Box::leak` **leakt** für immer.
/// Ein Client, der 30 Seiten mit je drei kaputten Blöcken holt, verliert
/// 90 `String`s und kann sie nie wieder freigeben. Das ist kein
/// Spezialfall, das ist der Normalfall eines Browsers.
///
/// DIE ZWEITE FALSCHE IDEE war, jeden HPACK-Fehler zu `Unbekannt` zu
/// machen: dann sieht ein `HuffmanUnvollstaendig` aus wie „der Rahmentyp
/// gibt es nicht", und wer sucht, findet die Ursache nicht.
///
/// ALSO: eine eigene Variante, die den **Typ** behält und die Zahlen
/// mitnimmt. Der Text wird beim `Display` erzeugt, nicht gespeichert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HpackFehlerWeiter {
    /// Der Name der Variante, z. B. `HuffmanUnvollstaendig`.
    pub was: &'static str,
    /// Die Zahl, die in der Meldung stand, falls es eine gab.
    pub zahl: Option<usize>,
}

/// Ein gelesener Rahmen: der Kopf **und** die Nutzlast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rahmen {
    pub typ: u8,
    pub flags: u8,
    /// Die Strom-Identität mit 31 Bit (das reservierte Bit ist entfernt).
    pub strom: u32,
    pub nutzlast: Vec<u8>,
}

/// Baut einen Rahmenkopf (9 Byte) nach RFC 9113 Abschnitt 4.1.
///
/// DIE REIHENFOLGE, und sie ist nicht verhandelbar: 24 Bit Länge, 8 Bit
/// Typ, 8 Bit Flags, 32 Bit Strom-ID.
///
/// DIE GRENZEN, alle drei (M4):
/// * Länge > [`MAX_RAHMEN_BYTES`] → `ZuGross`.
/// * Länge passt nicht in 24 Bit → `LaengeUeberlauf`.
/// * Strom-ID > 2^31-1 → das Bit 31 ist reserviert und **muss** 0 sein
///   (RFC 9113 4.3).
pub fn baue_kopf(typ: u8, flags: u8, strom: u32, laenge: usize) -> Result<[u8; 9], RahmenFehler> {
    if laenge > MAX_RAHMEN_BYTES {
        return Err(RahmenFehler::ZuGross {
            laenge,
            erlaubt: MAX_RAHMEN_BYTES,
        });
    }
    if laenge > 0x00ff_ffff {
        return Err(RahmenFehler::LaengeUeberlauf { war: laenge });
    }
    if strom > 0x7fff_ffff {
        return Err(RahmenFehler::StromVerboten { typ, strom });
    }
    let mut kopf = [0u8; KOPF_BYTES];
    kopf[0] = ((laenge >> 16) & 0xff) as u8;
    kopf[1] = ((laenge >> 8) & 0xff) as u8;
    kopf[2] = (laenge & 0xff) as u8;
    kopf[3] = typ;
    kopf[4] = flags;
    kopf[5..9].copy_from_slice(&strom.to_be_bytes());
    Ok(kopf)
}

/// Liest einen Rahmenkopf aus 9 Byte (RFC 9113 Abschnitt 4.1).
///
/// DER RESERVIERTE-BIT: RFC 9113 4.3 sagt, Bit 31 der Strom-ID ist
/// reserviert und muss beim Empfang **ignoriert** werden. Es wird also
/// abgeschnitten, nicht geprüft — ein Server, der es setzt, ist nicht
/// verboten, aber wir dürfen uns nicht darauf verlassen.
pub fn lies_kopf(b: &[u8]) -> Result<(u8, u8, u32, usize), RahmenFehler> {
    if b.len() < KOPF_BYTES {
        return Err(RahmenFehler::KopfZuKurz { war: b.len() });
    }
    let laenge = ((b[0] as usize) << 16) | ((b[1] as usize) << 8) | (b[2] as usize);
    if laenge > MAX_RAHMEN_BYTES {
        return Err(RahmenFehler::ZuGross {
            laenge,
            erlaubt: MAX_RAHMEN_BYTES,
        });
    }
    let typ = b[3];
    let fl = b[4];
    let strom = u32::from_be_bytes([b[5], b[6], b[7], b[8]]) & 0x7fff_ffff;
    Ok((typ, fl, strom, laenge))
}

/// Baut einen vollständigen Rahmen: Kopf **plus** Nutzlast.
///
/// DER WEG, und er ist absichtlich einfach: der Kopf wird gebaut, die
/// Nutzlast angehängt. Für `HEADERS` ist die Nutzlast der HPACK-Block,
/// den `hpack::schreibe_string` und Freunde erzeugen.
pub fn baue_rahmen(
    typ: u8,
    flags: u8,
    strom: u32,
    nutzlast: &[u8],
) -> Result<Vec<u8>, RahmenFehler> {
    let kopf = baue_kopf(typ, flags, strom, nutzlast.len())?;
    let mut raus = Vec::with_capacity(KOPF_BYTES + nutzlast.len());
    raus.extend_from_slice(&kopf);
    raus.extend_from_slice(nutzlast);
    Ok(raus)
}

/// Baut unseren `SETTINGS`-Rahmen (ohne ACK).
pub fn baue_settings() -> Result<Vec<u8>, RahmenFehler> {
    // added 04.10.2026, Rolle NETZTEMPO: **`eigene_settings()` statt
    // `EIGENE_SETTINGS`** — das ist der ganze Schalter. Alles andere (die
    // Reihenfolge, die Nutzlast, der Rahmenkopf) bleibt, weil es **keine**
    // Zahl ist, die der Preset veraendert.
    let mut nutzlast = Vec::with_capacity(eigene_settings().len() * 6);
    for (kennung, wert) in eigene_settings() {
        nutzlast.extend_from_slice(&kennung.to_be_bytes());
        nutzlast.extend_from_slice(&wert.to_be_bytes());
    }
    baue_rahmen(typen::SETTINGS, 0, 0, &nutzlast)
}

/// Baut ein `SETTINGS`-Fenster für einen Strom (`WINDOW_UPDATE`).
///
/// RFC 9113 Abschnitt 6.9: die Nutzlast ist eine 31-Bit-Zahl; Bit 31 ist
/// reserviert und muss 0 sein. Die Zahl zählt **Fenster-Increments**, und
/// eine Null ist verboten (Abschnitt 6.9: "a receiver MUST treat the
/// receipt of a WINDOW_UPDATE frame with a flow-control window increment
/// of 0 as a connection error").
pub fn baue_window_update(strom: u32, inkrement: u32) -> Result<Vec<u8>, RahmenFehler> {
    if inkrement == 0 {
        return Err(RahmenFehler::StromVerboten {
            typ: typen::WINDOW_UPDATE,
            strom,
        });
    }
    if inkrement > 0x7fff_ffff {
        return Err(RahmenFehler::LaengeUeberlauf {
            war: inkrement as usize,
        });
    }
    baue_rahmen(typen::WINDOW_UPDATE, 0, strom, &inkrement.to_be_bytes())
}

/// Liest einen Rahmen aus einem Puffer, der **mindestens** einen
/// vollständigen Rahmen enthält.
///
/// DER RÜCKGABE-WEG, und er ist wichtig: `(Rahmen, verbraucht)`, weil ein
/// Puffer mehrere Rahmen enthalten kann und der Aufrufer den Rest
/// braucht. Wer stattdessen „alles konsumiert" annimmt, verliert bei
/// zwei Rahmen in einem TCP-Segment den zweiten.
pub fn lies_rahmen(puffer: &[u8]) -> Result<(Rahmen, usize), RahmenFehler> {
    let (typ, flags, strom, laenge) = lies_kopf(puffer)?;
    let gesamt = KOPF_BYTES + laenge;
    if puffer.len() < gesamt {
        // Noch nicht alles da. Das ist **kein** Fehler, sondern der
        // Normalfall: TCP zerlegt Nachrichten.
        return Err(RahmenFehler::KopfZuKurz { war: puffer.len() });
    }
    Ok((
        Rahmen {
            typ,
            flags,
            strom,
            nutzlast: puffer[KOPF_BYTES..gesamt].to_vec(),
        },
        gesamt,
    ))
}

/// Prüft einen gelesenen Rahmen gegen die Regeln, die man **vor** der
/// Verarbeitung halten muss.
///
/// added 01.10.2026 09:0x, Rolle TLS. WARUM EINE EIGENE FUNKTION und
/// nicht ein scattering im Leser: die Prüfungen gehören an **eine** Stelle,
/// weil sie sonst pro Aufrufweg vergessen werden — das ist die Drei-Pfade-
/// Falle aus R4k/R4s.
///
/// DIE DREI REGELN, alle aus RFC 9113:
/// * Abschnitt 5.1: `SETTINGS` und `GOAWAY` gehören auf Strom **0**.
///   Ein Server, der `SETTINGS` auf Strom 3 schickt, redet ein anderes
///   Protokoll.
/// * Abschnitt 6.5.3: ein `SETTINGS`-Block hat 6 Byte je Paar.
/// * Abschnitt 6.5.2: der Typ muss bekannt sein, sonst ist es nach
///   Abschnitt 4.1 „unknown frame type" — das ist **kein** Fehler, aber
///   es muss übersprungen werden. Hier wird es benannt, damit der Aufrufer
///   entscheiden kann.
///
/// M3: die Anzahl der bisher gelesenen `SETTINGS` wird übergeben, nicht
/// intern gehalten — sonst wäre die Grenze nicht von außen sichtbar und
/// ein Test könnte sie nicht pruefen.
pub fn pruefe(rahmen: &Rahmen, settings_bisher: usize) -> Result<(), RahmenFehler> {
    match rahmen.typ {
        typen::SETTINGS | typen::GOAWAY => {
            if rahmen.strom != 0 {
                return Err(RahmenFehler::StromVerboten {
                    typ: rahmen.typ,
                    strom: rahmen.strom,
                });
            }
        }
        // added 01.10.2026 09:3x, Rolle TLS: clippy sagt „das `if` laesst
        // sich in den `match` ziehen" — und das ist hier richtig: die
        // Bedingung gehoert in das Wertenmuster, nicht in den Rumpf.
        // GEMESSEN (Lauf 09:0x, `werkzeuge/bau.sh narsil-net clippy`).
        //
        // added 01.10.2026 14:1x, R4z7. **`RST_STREAM` AUF STROM 0 IST
        // VERBOTEN** — und nur das. Vorher stand hier
        // `typen::WINDOW_UPDATE | typen::RST_STREAM`, und damit war auch
        // `WINDOW_UPDATE` auf Strom 0 verboten.
        //
        // GEMESSEN, gegen `archlinux.org`, mit dem neuen Client:
        //   `HTTP/2: Rahmen 0x08 auf verbotenem Strom 0`
        // 0x08 ist `WINDOW_UPDATE`. Der Server schickt **Verbindungs**-
        // Flusskontrolle, und wir haben ihn abgewiesen.
        //
        // DIE NORM, woertlich (`rfc9113.txt` Zeile 2117-2120):
        //   "The WINDOW_UPDATE frame can be specific to a stream or to
        //    the entire connection. In the former case, the frame's
        //    stream identifier indicates the affected stream; in the
        //    latter, the value \"0\" indicates that the entire connection
        //    is the subject of the frame."
        //
        // WARUM DAS SO LANGE UNBEMERKT BLIEB: `pruefe` wurde in R4w gegen
        // **handgebaute** Rahmen geprueft, und in keinem Test kam ein
        // `WINDOW_UPDATE` vor. Ein Fehler, der nur von einem echten
        // Server ausgeloest wird, braucht einen echten Server.
        typen::RST_STREAM if rahmen.strom == 0 => {
            return Err(RahmenFehler::StromVerboten {
                typ: rahmen.typ,
                strom: rahmen.strom,
            });
        }
        _ => {}
    }
    if rahmen.typ == typen::SETTINGS {
        if rahmen.nutzlast.len() > MAX_SETTINGS_BYTES {
            return Err(RahmenFehler::SettingsZuGross {
                war: rahmen.nutzlast.len(),
                erlaubt: MAX_SETTINGS_BYTES,
            });
        }
        if rahmen.flags & flags::ACK == 0 {
            // RFC 9113 6.5.3: ohne ACK ist es eine Anweisung, und die
            // muss aus Paaren ganzer Byte bestehen.
            if rahmen.nutzlast.len() % 6 != 0 {
                return Err(RahmenFehler::SettingsUnvollstaendig {
                    war: rahmen.nutzlast.len(),
                });
            }
        } else {
            // MIT ACK ist die Nutzlast nach 6.5.3 **leer**. Ein ACK mit
            // Inhalt ist ein Verbindungsfehler — die genaue Diagnose
            // nennt `SettingsUnvollstaendig`, weil es dieselbe Ursache hat.
            if !rahmen.nutzlast.is_empty() {
                return Err(RahmenFehler::SettingsUnvollstaendig {
                    war: rahmen.nutzlast.len(),
                });
            }
        }
        if settings_bisher >= MAX_SETTINGS {
            return Err(RahmenFehler::ZuVieleSettings {
                waren: settings_bisher,
                erlaubt: MAX_SETTINGS,
            });
        }
    }
    Ok(())
}

/// Liest die Paare aus einem `SETTINGS`-Rahmen.
///
/// DER RÜCKGABE-WEG: ein `Vec` der Paare, **nicht** ein `HashMap`, weil
/// die Reihenfolge in RFC 9113 6.5.3 keine Rolle spielt, aber ein Vec
/// keine Allokation pro Paar braucht und leichter zu begrenzen ist.
pub fn settings_paare(block: &[u8]) -> Result<Vec<(u16, u32)>, RahmenFehler> {
    if block.len() > MAX_SETTINGS_BYTES {
        return Err(RahmenFehler::SettingsZuGross {
            war: block.len(),
            erlaubt: MAX_SETTINGS_BYTES,
        });
    }
    if block.len() % 6 != 0 {
        return Err(RahmenFehler::SettingsUnvollstaendig { war: block.len() });
    }
    // M4: die Zahl der Paare ist durch `MAX_SETTINGS_BYTES / 6` begrenzt
    // (also 682), und das ist aus einer **Eingabe** abgeleitet — ohne
    // eigene Konstante, weil die Grenze schon in `MAX_SETTINGS_BYTES`
    // steht. Ein `Vec::with_capacity` hier waere eine ungeprueffte
    // Allokation aus dem Netz.
    let anzahl = block.len() / 6;
    let mut raus = Vec::with_capacity(anzahl);
    let mut i = 0usize;
    while i < block.len() {
        let kennung = u16::from_be_bytes([block[i], block[i + 1]]);
        let wert = u32::from_be_bytes([block[i + 2], block[i + 3], block[i + 4], block[i + 5]]);
        raus.push((kennung, wert));
        i += 6;
    }
    Ok(raus)
}

/// Die Prüfsumme eines `PING`-Rahmens (`SETTINGS`-Art, aber 8 Byte).
///
/// RFC 9113 Abschnitt 6.7: die Nutzlast ist **8 Byte**, und eine
/// ACK-Antwort muss **denselben** Inhalt tragen. Deshalb ist es eine
/// Funktion und kein Feld: der Aufrufer soll den Wert nicht selbst
/// erzeugen müssen.
pub fn ping_nutzlast(wert: [u8; 8]) -> [u8; 8] {
    wert
}
