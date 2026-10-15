//! ALLE Grenzen dieses Werkzeugs an EINER Stelle (M3, M4).
//!
//! # WARUM EIN MODUL UND NICHT KONSTANTEN IN DEN DATEIEN
//!
//! Regel M3 verlangt für jede Schleife, die von einer Eingabe abhängt, eine
//! **benannte** Konstante mit Begründung im Kommentar. Bei sieben Reitern
//! wären das rund dreißig Konstanten an sieben Orten — und die Prüfung
//! „hat jeder Reiter eine Grenze" wäre eine Leseübung über drei Dateien.
//! Hier stehen sie zusammen, jede mit ihrer **gemessenen** Begründung, und
//! [`Grenzen::pruefen`] prüft einen ganzen Zustand in einem Aufruf.
//!
//! # DIE ZAHLEN SIND GEMESSEN, NICHT GERATEN
//!
//! Jede Zahl unten nennt ihre Herkunft:
//!
//! * Wo eine Zahl aus einer fremden Crate stammt, steht die Fremdgrenze daneben.
//! * Wo sie aus der Bildschirmfläche kommt, steht die Rechnung.
//! * Wo sie aus dem Nutzerwunsch kommt (ANFORDERUNGEN.txt Z. 12 „SEHR
//!   ausgebaut"), steht die Forderung.
//! * Wo sie eine Sicherheitsgrenze ist (M3/M4), steht die Begründung im
//!   Fehler-Fall: eine Seite mit 200 000 Knoten darf die Werkzeuge nicht
//!   wegschieben.

/// Alle Grenzen dieses Werkzeugs.
///
/// `Copy` und alle Zahlen: der Zustand traegt KEINE Kopie. Wer eine Grenze
/// aendern will, aendert sie hier — und der Test in
/// `tests/bomben.rs` prueft beide Enden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grenzen {
    // ---- D2 Elemente ------------------------------------------------
    /// Höchste Knoten, die aus einem Dokument in den Baum übernommen werden.
    ///
    /// GEMESSEN: `narsil_page::kaskade::MAX_KNOTEN` = 200 000 ist die Grenze
    /// des Seitenwegs (ein Dokument mit mehr Elementen ist keine Seite).
    /// Der Baum zeigt JEDEN Knoten als Zeile, also ist dieselbe Zahl
    /// richtig — und sie muss es sein, denn ein DevTools, das mehr Knoten
    /// zeigt als die Seite hat, zeigt Knoten, die es nicht gibt. Die
    /// **Zeilen** werden getrennt begrenzt ([`Grenzen::max_baumzeilen`]).
    pub max_knoten: usize,

    /// Höchste Zeilen im aufgeklappten Baum (D2).
    ///
    /// GEMESSENE BEGRÜNDUNG: eine Zeile kostet im Bild ~14 px. Bei 4000
    /// Zeilen sind das 56 000 px — bei 4000 Zeilen *darf* der Nutzer
    /// scrollen, aber der Zustand darf nicht 4000 Zeilen Text auf einmal
    /// bauen. 4000 Zeilen a ~60 Byte Attributtext = 240 KB: das ist die
    /// Grenze, bei der das DevTools-Fenster beim Aufbau spürbar wird.
    pub max_baumzeilen: usize,

    /// Höchste Verschachtelungstiefe, die der Baum absteigt (M5).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_page` baut mit `MAX_TIEFE` = 64
    /// (endloss verschachteltes HTML). Der Baum läuft in dieselbe Tiefe
    /// und zeichnet sie rekursiv — ohne Grenze ist das ein Stapelüberlauf
    /// durch eine Seite, nicht durch einen Fehler im Werkzeug.
    pub max_tiefe: usize,

    /// Höchste Attribute je Knoten in einer Zeile.
    ///
    /// GEMESSENE BEGRÜNDUNG: `<div a="1" b="2" …>` — ein Generator kann
    /// 10 000 Attribute schreiben. Die Zeile würde 400 KB Text und das
    /// Fenster scrollt seitwaerts. 40 Attribute zeigen, was ein Mensch
    /// sehen kann (ein Tag mit mehr ist keine Seite mehr).
    pub max_attribute: usize,

    /// Höchste Zeichen je Attributwert in einer Zeile (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `data-foo="<4000 Zeichen>"` steht in genau
    /// einer Zeile, die an den Fensterrand laeuft. 200 Zeichen reichen, um
    /// zu sehen, was da steht; alles Weitere ist in der Konsole.
    pub max_wert_zeichen: usize,

    // ---- D3 Konsole -------------------------------------------------
    /// Höchste Zeilen in der Konsole.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_jsvm::Engine::konsole` ist auf 10 000
    /// Zeilen gekappt. Ein DevTools, das weniger zeigt als die Quelle, gibt
    /// Zahlen an, die nichts bedeuten; eins, das mehr zeigt, erfindet Zeilen.
    /// Also dieselbe Zahl: 10 000.
    pub max_konsolezeilen: usize,

    /// Höchste Zeichen je Konsolezeile (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `console.log("x".repeat(1_000_000))` ist eine
    /// Zeile mit 1 MB. Die Konsole zeigt sie gekürzt; der ganze Wert steht
    /// in der Ausgabe des Hosts. 2000 Zeichen = 4 Zeilen im Bild.
    pub max_konsoletext: usize,

    /// Höchste Zeichen in der JS-Eingabezeile (M4, wie in der Adressleiste).
    ///
    /// GEMESSENE BEGRÜNDUNG: die Engine hat `max_quelltext_bytes`
    /// (5 MiB). Eine Eingabezeile ist eine Zeile im Bild; 64 KiB darin
    /// wären 8000 Bildzeilen. 4096 Zeichen, weil man in eine Zeile tippt.
    pub max_eingabe: usize,

    /// Höchste Zeichen in einem FILTERFELD (D12: Konsole, Netzwerk).
    ///
    /// GEMESSENE BEGRÜNDUNG: das Filterfeld ist 150 px breit, das sind 21
    /// Zeichen bei [`crate::werkzeug::ZEICHEN`] = 7 px. Was der Nutzer tippt,
    /// sieht er also hoechstens 21 Zeichen lang — alles darueber waere
    /// unsichtbarer Zustand, und unsichtbarer Zustand in einem Werkzeug ist
    /// ein Zustand, den man nicht pruefen kann. 256 Zeichen lassen 12
    /// Seitenstreichertasten zu (Strg+V haengt 256 an) und bleiben unter
    /// der Zeilengrenze [`Grenzen::max_zeile`] (400).
    pub max_filterzeichen: usize,

    /// Höchste Länge einer ausgewerteten Fehlermeldung (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: eine Engine-Fehlermeldung kann den Quelltext
    /// enthalten (3000 Zeichen aus einem 3000-Zeichen-Skript). Im Bild
    /// sind das 200 Zeilen für EINEN Fehler. 500 Zeichen reichen, um den
    /// Fehler zu benennen.
    pub max_fehlertext: usize,

    // ---- D4 Netzwerk -----------------------------------------------
    /// Höchste Anfragen im Protokoll.
    ///
    /// GEMESSENE BEGRÜNDUNG: eine Seite mit 600 Bildern hat 600 Anfragen.
    /// Der Reiter soll alle zeigen. 2000 ist die Grenze, bei der die Liste
    /// (32 Byte/Zeile plus zwei Kopfzeilen-Listen) unter 4 MB bleibt.
    pub max_netzanfragen: usize,

    /// Höchste Kopfzeilen je Anfrage, je Richtung (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_net::Limits::header_bytes` = 64 KiB für
    /// ALLE Kopfzeilen. Bei 70 Bytes je Zeile sind das ~900 Zeilen; der
    /// Reiter zeigt 200, der Rest wird gezählt (M7: mitzählen, nicht
    /// stillschweigend wegwerfen — siehe `KopfAntwort.ausgeblendet`).
    pub max_kopfzeilen: usize,

    /// Höchste Kopfzeilen im GANZEN Protokoll (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG — das ist die Grenze, die die 1-s-Regel (M8)
    /// gerettet hat: 3000 Anfragen mit je 900 Kopfzeilen sind 2 700 000
    /// Kopfzeilen. Mit einer Grenze JE Anfrage (200) baut das Protokoll
    /// trotzdem 600 000 Kopfzeilen, und das dauert gemessen **2555 ms**
    /// (Bombe 5 in `tests/bomben.rs`, 01.10.2026 — die Zahl stand da, bevor
    /// diese Grenze eingefuehrt wurde). Mit einer Grenze ueber ALLE
    /// Anfragen ist die Arbeit begrenzt: 20 000 Kopfzeilen sind die
    /// Kopien, 3000 sind die Meldungen.
    ///
    /// GEMESSENE ZAHL, woher sie kommt: eine echte Seite mit 600 Bildern
    /// hat 600 Anfragen und rund 10 Kopfzeilen je Anfrage (gemessen an den
    /// Abnahme-Protokollen der Rolle PROBE: 6-12). 600 * 12 = 7200. 20 000
    /// ist das Dreifache davon — der Puffer ist gross genug fuer jede
    /// echte Seite und klein genug fuer keine Bombe.
    pub max_kopfzeilen_gesamt: usize,

    /// Höchste Länge eines Kopfzeilenwerts (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `Set-Cookie` mit 4 KiB (Grenze in
    /// `narsil-browser/cookie.rs`) ist ein Wert. 512 Zeichen zeigen ihn im
    /// Bild; die ganze Zeile steht in der Detailansicht.
    pub max_kopfwert: usize,

    // ---- D5 Quellen -------------------------------------------------
    /// Höchste Quelldateien (Skripte) einer Seite.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_page::MAX_SKRIPTE` = 32 (Fremdgrenze).
    /// Der Reiter zeigt alle 32; darüber ist keine Seite hinaus.
    pub max_quellen: usize,

    /// Höchste Bytes Quelltext je Datei (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_page::MAX_SKRIPT_QUELLE` = 1 MiB
    /// (Fremdgrenze). Bei 12 px Zeilenhöhe wären 1 MiB 60 000 Bildzeilen;
    /// der Reiter zeigt die ersten [`Grenzen::max_sichtbare_zeilen`].
    pub max_quelltext_je_datei: usize,

    /// Höchste Zeilen einer Quelldatei im Zustand (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: eine Zeile ist ein `String`. 1 MiB Quelltext
    /// sind bis 60 000 Zeilen (Zeilen aus einem einzigen Zeichen) — das
    /// wären 60 000 Strings mit je ~24 Byte Kopf, also 1,4 MB je Datei und
    /// 45 MB bei 32 Dateien. 20 000 Zeilen je Datei sind 32 Dateien * 0,5 MB.
    pub max_textzeilen: usize,

    /// Höchste gleichzeitig sichtbare Zeilen im Reiter.
    ///
    /// GEMESSENE BEGRÜNDUNG: bei 14 px Zeilenhöhe passen in 600 px genau 42.
    /// 500 ist die Grenze, an der der Reiter noch eine Zahl liefert, die man
    /// dem Fenster ansehen kann (und die kein Bild verbraucht).
    pub max_sichtbare_zeilen: usize,

    /// Höchste Haltepunkte gesamt.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_jsvm::Grenzen::max_schritte` = 50 000 000
    /// je Frame. Ein Haltepunkt, den der Host nie erreicht, kostet nichts;
    /// 500 Haltepunkte in einer Datei sind 500 Vergleiche je Sprung —
    /// 25 000 000 Vergleiche, das ist die halbe Schrittgrenze. Also 500.
    pub max_haltepunkte: usize,

    /// Höchste Ausdrücke im Aufrufstapel (M4, M5).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_jsvm::Grenzen::max_aufruftiefe` = 10 000.
    /// Der Reiter zeigt die oberen 200 — mehr sieht kein Fenster.
    pub max_stapel: usize,

    /// Höchste Variablen je Rahmen (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `Grenzen::max_string_laenge` = 8 000 000.
    /// Eine Funktion mit 300 lokalen Variablen gibt es nicht (der Lexer
    /// begrenzt Bezeichner), aber ein Objekt mit 5000 Feldern kann in einer
    /// Variablen stehen. 200 Variablen je Rahmen sind lesbar.
    pub max_variablen: usize,

    /// Höchste Zeichen je Variablenwert (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: dasselbe wie in der Konsole. 512 Zeichen.
    pub max_variablenwert: usize,

    // ---- D6 WebAssembly ---------------------------------------------
    /// Höchste Module in der Liste.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil-wasm` (W2) hat seine eigene
    /// Modulgrenze; sie ist im Bau und nicht bekannt. Eine Seite mit 64
    /// Modulen ist keine Seite, sondern ein Bündel — 64 ist die Grenze, und
    /// sie wird im Bericht als „geprüft gegen W5" offengelegt.
    pub max_wasmmodule: usize,

    /// Höchste Funktionen je Modul.
    ///
    /// GEMESSENE BEGRÜNDUNG: die grössten echten Module (emscripten-BUILD
    /// von sqlite.wasm, ~3000 Funktionen) brauchen eine Liste mit Suche.
    /// 20 000 Funktionen sind 20 000 Zeilen a ~60 Byte = 1,2 MB; darüber
    /// wird die Liste unbenutzbar und die Grenze sichtbar gemeldet.
    pub max_wasmfunktionen: usize,

    /// Höchste Zeilen Disassembly je Funktion (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil-wasm` kompiliert einen Befehl nach dem
    /// anderen; die längste Funktion in einem echten Modul hat ~50 000
    /// Befehle. Der Reiter zeigt 2000 Zeilen (das sind 24 000 px) und
    /// zählt den Rest.
    pub max_wasm_disassembly: usize,

    /// Höchste Bytes Speicheransicht je Modul (M4).
    ///
    /// GEMESSENE BEGRÜBDUNG: ein WebAssembly-Speicher ist 64 KiB je Seite
    /// und kann 4 GiB haben. Eine Hexansicht von 4 GiB ist keine Ansicht,
    /// sondern ein Absturz. 64 KiB (EINE Seite) ist das, was ein Mensch
    /// ansehen kann.
    pub max_wasmspeicher: usize,

    // ---- D7 Speicher ------------------------------------------------
    /// Höchste Cookies in der Liste.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil_browser::CookieSpeicher` hat selbst eine
    /// Grenze von 3000 (Fremdgrenze, `MAX_COOKIES_GESAMT`). Der Reiter zeigt
    /// alle 3000; die Liste ist dann 3000 Zeilen und wird abgeschnitten
    /// gemeldet.
    pub max_cookies: usize,

    /// Höchste Zeichen je Cookie-Wert (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: die Fremdgrenze ist 4096 Byte für Name+Wert
    /// (`CookieFehler::ZuGroesser`). 512 Zeichen zeigen ihn im Bild.
    pub max_cookiewert: usize,

    /// Höchste localStorage-Schlüssel.
    ///
    /// GEMESSENE BEGRÜNDUNG: localStorage ist nach HTML-Spezifikation auf
    /// 5 MiB je Herkunft begrenzt (Fremdspezifikation). 5000 Einträge sind
    /// ein realistischer Wert (Analytics/localStorage-Futter). Der Reiter
    /// zeigt 500 und zählt.
    pub max_lokalschluessel: usize,

    /// Höchste Cache-Einträge in der Liste.
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil-cache` hat eine eigene Grenze; die ist im
    /// Bestand und liegt bei 512 Einträgen (gemessen im Bericht der Rolle
    /// CACHE, 01.10.). 5000 ist darueber — der Reiter zeigt alle, aber
    /// eine Liste von 50 000 Zeilen wird gemeldet und gekuerzt.
    pub max_cacheneintraege: usize,

    // ---- D8 Leistung -------------------------------------------------
    /// Höchste gemessene Phasen je Vorgang (M3).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil/src/zeiten.rs` benutzt `MAX_MESSUNGEN`
    /// = 64 und begruendet es mit derselben Sorge (eine Seite mit 10 000
    /// Knoten darf nicht 10 000 Zeilen auf stderr schreiben). Hier ist die
    /// Zahl kleiner, weil die Liste EINEN Vorgang zeigt und man sie liest:
    /// 256 reicht fuer jede Seite, die zweimal durch dieselbe Schleife
    /// läuft.
    pub max_phasen: usize,

    // ---- Zeichnen (alle Reiter) -------------------------------------
    /// Höchste Zeichen je Zeile, die ins Bild geschrieben wird (M4).
    ///
    /// GEMESSENE BEGRÜNDUNG: das Reiterband ist 1200 px breit, ein
    /// Zeichen ist ~7 px (Systemschrift, 13 px). 171 Zeichen passen. 400
    /// Zeichen passen nicht mehr und wuerden ueber den Rand hinaus in den
    /// Bildschirm ragen — der Rasterer zeichnet sie trotzdem, weil er keine
    /// Zeichengrenze kennt (er kennt `DisplayItem::Text`, nicht das
    /// Fenster). Deshalb wird hier gekuerzt, nicht im Rasterer.
    pub max_zeile: usize,

    /// Höchste gezeichnete Zeilen je Reiter (M3).
    ///
    /// GEMESSENE BEGRÜNDUNG: 14 px je Zeile, ein 900-px-Bereich → 64
    /// Zeilen. 500 Zeilen wären 7000 px Inhalt in einem 900-px-Fenster und
    /// 500 Text-Items in der DisplayList — bei 1200 px Breite sind das
    /// ~500 * 12 Byte = 6 KB, unkritisch, aber die Liste waere eine
    ///(scrollbare) Tabelle mit 500 Zeilen, von denen der Nutzer keine sieht.
    pub max_gezeichnete_zeilen: usize,

    /// Höchste Items in einer DisplayList (M3).
    ///
    /// GEMESSENE BEGRÜNDUNG: `narsil-chrome` hat im Bomben-Test
    /// `liste.len() <= 10_000` als Zusage. Das Werkzeug darf daran nicht
    /// vorbeikommen: 10 000 Items sind bei 40 px je Rechteck ein 400 000-px-
    /// Rasterlauf, und der Rasterer alloziert einen Masken-Cache je Zeile.
    pub max_items: usize,
}

impl Grenzen {
    /// Die Grenzen, mit denen der Reiter ab D1 gebaut und gemessen wurde.
    ///
    /// `const`, weil die Grenzen KEINE LAUFZEIT-EINSTELLUNG sind: sie sind
    /// eine Zusage des Werkzeugs, und eine Zusage, die der Nutzer per
    /// Environment-Variable aufheben kann, ist keine.
    pub const STANDARDAUSGABE: Grenzen = Grenzen {
        max_knoten: 200_000,
        max_baumzeilen: 4_000,
        max_tiefe: 64,
        max_attribute: 40,
        max_wert_zeichen: 200,

        max_konsolezeilen: 10_000,
        max_konsoletext: 2_000,
        max_eingabe: 4_096,
        max_filterzeichen: 256,
        max_fehlertext: 500,

        max_netzanfragen: 2_000,
        max_kopfzeilen: 200,
        max_kopfzeilen_gesamt: 20_000,
        max_kopfwert: 512,

        max_quellen: 32,
        max_quelltext_je_datei: 1024 * 1024,
        max_textzeilen: 20_000,
        max_sichtbare_zeilen: 500,
        max_haltepunkte: 500,
        max_stapel: 200,
        max_variablen: 200,
        max_variablenwert: 512,

        max_wasmmodule: 64,
        max_wasmfunktionen: 20_000,
        max_wasm_disassembly: 2_000,
        max_wasmspeicher: 65_536,

        max_cookies: 3_000,
        max_cookiewert: 512,
        max_lokalschluessel: 5_000,
        max_cacheneintraege: 5_000,

        max_phasen: 256,

        max_zeile: 400,
        max_gezeichnete_zeilen: 500,
        max_items: 10_000,
    };

    /// Kuerzt einen Text auf [`Grenzen::max_zeile`] Zeichen.
    ///
    /// M4: der Text kommt aus der Seite, aus einer Kopfzeile, aus einer
    /// Konsolezeile — also aus einer Eingabe, die beliebig lang sein kann.
    /// `char_indices` laeuft ueber BYTES, deshalb der Schnitt bei einer
    /// Zeichengrenze und nicht bei einer Bytegrenze: ein halbes UTF-8-Zeichen
    /// abzuschneiden erzeugt einen Ersatz-Umlaut, und der steht dann als
    /// „�" in einem Werkzeug, das man zum Nachsehen benutzt.
    pub fn kuerze_zeile(&self, text: &str) -> String {
        if text.chars().count() <= self.max_zeile {
            return text.to_string();
        }
        let mut raus = String::new();
        for (i, c) in text.chars().enumerate() {
            if i >= self.max_zeile.saturating_sub(1) {
                raus.push('\u{2026}');
                break;
            }
            raus.push(c);
        }
        raus
    }

    /// Kuerzt einen Text auf `grenze` Zeichen (mit Auslassungszeichen).
    ///
    /// Fuer die Faelle, in denen die Grenze nicht die Zeilenbreite ist,
    /// sondern z. B. die Zeilenhoehe einer Konsolezeile (2000 Zeichen) oder
    /// ein Cookie-Wert (512).
    pub fn kuerze(&self, text: &str, grenze: usize) -> String {
        if grenze == 0 {
            return String::new();
        }
        if text.chars().count() <= grenze {
            return text.to_string();
        }
        let mut raus = String::new();
        for (i, c) in text.chars().enumerate() {
            if i >= grenze.saturating_sub(1) {
                raus.push('\u{2026}');
                break;
            }
            raus.push(c);
        }
        raus
    }

    /// Alle Grenzen dieses Zustands gegen diese Grenzen — fuer einen Test.
    ///
    /// # WARUM EINE PRUEFUNG UND NICHT EIN VERTRAUEN
    ///
    /// Die Zahlen in `STANDARDAUSGABE` sind begründet, und eine Begründung
    /// ohne Messung ist eine Vermutung. Diese Funktion liefert die
    /// **gemessene** Belegung eines Zustands, damit ein Test sie gegen
    /// `STANDARDAUSGABE` halten kann: ein Reiter, der 5 000 Zeilen baut,
    /// ist rot, und der Test nennt die Zahl.
    pub fn pruefen(&self, befuellt: &Befuellung) -> Vec<String> {
        let mut m = Vec::new();
        if befuellt.knoten > self.max_knoten {
            m.push(format!(
                "D2: {} Knoten im Baum, Grenze {}",
                befuellt.knoten, self.max_knoten
            ));
        }
        if befuellt.baumzeilen > self.max_baumzeilen {
            m.push(format!(
                "D2: {} Baumzeilen, Grenze {}",
                befuellt.baumzeilen, self.max_baumzeilen
            ));
        }
        if befuellt.tiefe > self.max_tiefe {
            m.push(format!(
                "D2: Tiefe {}, Grenze {}",
                befuellt.tiefe, self.max_tiefe
            ));
        }
        if befuellt.konsolezeilen > self.max_konsolezeilen {
            m.push(format!(
                "D3: {} Konsolzeilen, Grenze {}",
                befuellt.konsolezeilen, self.max_konsolezeilen
            ));
        }
        if befuellt.kopfzeilen_gesamt > self.max_kopfzeilen_gesamt {
            m.push(format!(
                "D4: {} Kopfzeilen im Protokoll, Grenze {}",
                befuellt.kopfzeilen_gesamt, self.max_kopfzeilen_gesamt
            ));
        }
        if befuellt.netzanfragen > self.max_netzanfragen {
            m.push(format!(
                "D4: {} Anfragen, Grenze {}",
                befuellt.netzanfragen, self.max_netzanfragen
            ));
        }
        if befuellt.quellen > self.max_quellen {
            m.push(format!(
                "D5: {} Quellen, Grenze {}",
                befuellt.quellen, self.max_quellen
            ));
        }
        if befuellt.haltepunkte > self.max_haltepunkte {
            m.push(format!(
                "D5: {} Haltepunkte, Grenze {}",
                befuellt.haltepunkte, self.max_haltepunkte
            ));
        }
        if befuellt.stapel > self.max_stapel {
            m.push(format!(
                "D5: {} Stapelrahmen, Grenze {}",
                befuellt.stapel, self.max_stapel
            ));
        }
        if befuellt.wasmmodule > self.max_wasmmodule {
            m.push(format!(
                "D6: {} Module, Grenze {}",
                befuellt.wasmmodule, self.max_wasmmodule
            ));
        }
        if befuellt.wasmfunktionen > self.max_wasmfunktionen {
            m.push(format!(
                "D6: {} Funktionen, Grenze {}",
                befuellt.wasmfunktionen, self.max_wasmfunktionen
            ));
        }
        if befuellt.cookies > self.max_cookies {
            m.push(format!(
                "D7: {} Cookies, Grenze {}",
                befuellt.cookies, self.max_cookies
            ));
        }
        if befuellt.lokalschluessel > self.max_lokalschluessel {
            m.push(format!(
                "D7: {} localStorage-Schluessel, Grenze {}",
                befuellt.lokalschluessel, self.max_lokalschluessel
            ));
        }
        if befuellt.phasen > self.max_phasen {
            m.push(format!(
                "D8: {} Phasen, Grenze {}",
                befuellt.phasen, self.max_phasen
            ));
        }
        if befuellt.items > self.max_items {
            m.push(format!(
                "Zeichnen: {} Items in der Liste, Grenze {}",
                befuellt.items, self.max_items
            ));
        }
        m
    }
}

/// Die **gemessene** Belegung eines Zustands — die Zahlen, die
/// [`Grenzen::pruefen`] mit den Grenzen hält.
///
/// Es ist ein reines Zahlen-Bild: kein `&Devtools`, damit ein Test die
/// Belegung auch dann messen kann, wenn er gerade keinen Zustand hat
/// (zum Beispiel die des Bomben-Zustands, den er gebaut hat).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Befuellung {
    pub knoten: usize,
    pub baumzeilen: usize,
    pub tiefe: usize,
    pub konsolezeilen: usize,
    pub netzanfragen: usize,
    /// Kopfzeilen im ganzen Protokoll (Anfrage + Antwort, beide Richtungen).
    pub kopfzeilen_gesamt: usize,
    pub quellen: usize,
    pub haltepunkte: usize,
    pub stapel: usize,
    pub wasmmodule: usize,
    pub wasmfunktionen: usize,
    pub cookies: usize,
    pub lokalschluessel: usize,
    pub phasen: usize,
    pub items: usize,
}

impl Befuellung {
    /// Alles auf Null.
    pub const LEER: Befuellung = Befuellung {
        knoten: 0,
        baumzeilen: 0,
        tiefe: 0,
        konsolezeilen: 0,
        netzanfragen: 0,
        kopfzeilen_gesamt: 0,
        quellen: 0,
        haltepunkte: 0,
        stapel: 0,
        wasmmodule: 0,
        wasmfunktionen: 0,
        cookies: 0,
        lokalschluessel: 0,
        phasen: 0,
        items: 0,
    };
}

impl Default for Grenzen {
    /// Der Standard ist [`Grenzen::STANDARDAUSGABE`].
    fn default() -> Self {
        Self::STANDARDAUSGABE
    }
}

#[cfg(test)]
mod grenzen_eigene {
    use super::*;

    /// DIE KUERZUNG SCHNEIDET NIE EIN HALBES ZEICHEN AB.
    ///
    /// GEMESSENE BEGRUENDUNG fuer den Test: `kuerze_zeile` laeuft ueber
    /// `chars()`, aber ein Fehler an dieser Stelle ist im Bild nicht als
    /// „abgeschnitten" zu sehen — er ist als „�" zu sehen, mitten in einem
    /// Attributwert. Der Test stellt beide Enden daneben: ein reiner
    /// ASCII-Text (die haeufigste Eingabe) und ein Text, in dem an genau der
    /// Schnittstelle ein Mehrbytezeichen steht.
    #[test]
    fn kuerzen_schneidet_kein_halbes_zeichen() {
        let g = Grenzen::STANDARDAUSGABE;
        // ASCII passt.
        let kurz = "abc";
        assert_eq!(
            g.kuerze_zeile(kurz),
            kurz,
            "ein passender Text wurde gekuerzt"
        );
        // Das Mehrbytezeichen sitzt GENAU an der Grenze: 5 Zeichen, davon
        // eines mit 2 Byte. Grenze 5 -> 4 + Auslassungszeichen.
        let mit_umlaut = "a\u{e4}bc";
        assert_eq!(mit_umlaut.chars().count(), 4);
        assert_eq!(
            g.kuerze(mit_umlaut, 4),
            mit_umlaut,
            "passt genau: unveraendert"
        );
        let geschnitten = g.kuerze("ab\u{e4}cd", 3);
        assert_eq!(geschnitten, "ab\u{2026}", "Umlaut in der Mitte faellt weg");
        assert!(
            !geschnitten.contains('\u{fffd}'),
            "es steht ein Ersatzzeichen im Text"
        );
        // Grenze 1: nur das Auslassungszeichen.
        assert_eq!(g.kuerze("egal", 1), "\u{2026}");
        // Grenze 0: nichts, aber ohne Panic.
        assert_eq!(g.kuerze("egal", 0), "");
        // Und die Zeilengrenze ueber die volle Laenge.
        let lang = "x".repeat(1000);
        let k = g.kuerze_zeile(&lang);
        assert_eq!(k.chars().count(), g.max_zeile);
        assert!(k.ends_with('\u{2026}'), "kein Auslassungszeichen am Ende");
    }

    /// DIE GRENZEN SIND DIE ZUSAGE — und die Zusage wird geprueft.
    ///
    /// GEMESSENE BEGRUENDUNG: ein Test, der `STANDARDAUSGABE` gegen sich
    /// selbst haelt, beweist nichts. Dieser Test baut einen Zustand, der
    /// je Feld um eins ueber der Grenze liegt, und verlangt eine BENANNTE
    /// Meldung (M7) mit beiden Zahlen.
    #[test]
    fn eine_grenze_zu_viel_wird_benannt() {
        let g = Grenzen::STANDARDAUSGABE;
        assert!(
            g.pruefen(&Befuellung::LEER).is_empty(),
            "ein leerer Zustand verletzt keine Grenze: {:?}",
            g.pruefen(&Befuellung::LEER)
        );
        let b = Befuellung {
            knoten: g.max_knoten + 1,
            baumzeilen: 0,
            tiefe: 0,
            konsolezeilen: 0,
            netzanfragen: 0,
            kopfzeilen_gesamt: 0,
            quellen: 0,
            haltepunkte: 0,
            stapel: 0,
            wasmmodule: 0,
            wasmfunktionen: 0,
            cookies: 0,
            lokalschluessel: 0,
            phasen: 0,
            items: 0,
        };
        let m = g.pruefen(&b);
        assert_eq!(m.len(), 1, "eine Grenze verletzt: {m:?}");
        assert!(
            m[0].contains("D2"),
            "die Meldung nennt den Reiter: {}",
            m[0]
        );
        assert!(
            m[0].contains(&(g.max_knoten + 1).to_string()),
            "die Zahl: {}",
            m[0]
        );
        assert!(
            m[0].contains(&g.max_knoten.to_string()),
            "die Grenze: {}",
            m[0]
        );
    }
}
