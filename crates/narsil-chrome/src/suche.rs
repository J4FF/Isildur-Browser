//! A-EINST / U3-5 — DIE SUCHMASCHINE UND DIE EINGABE OHNE SCHEMA.
//!
//! # WAS DIE KOORDINATION VERLANGT (01.10., „Eingabe ohne Schema")
//!
//! `"beispiel"` oder `"wetter berlin"` -> Suche mit der
//! Standardsuchmaschine, `"beispiel.de"` -> `https://`.
//!
//! # DIE ENTSCHEIDUNG, DIE ALLES TRAGT
//!
//! GEMESSENE GRUNDREGEL: entscheidend ist, ob die Eingabe einen PUNKT
//! enthaelt — nicht, ob sie wie eine Domain aussieht. Warum:
//!
//! | Eingabe | Punkte | Ergebnis |
//! |---------|--------|----------|
//! | `beispiel` | 0 | Suche |
//! | `wetter berlin` | 0 | Suche (Leerzeichen = zwei Woerter = Suche) |
//! | `beispiel.de` | 1 | `https://beispiel.de` |
//! | `127.0.0.1` | 3 | `https://127.0.0.1` |
//! | `localhost:8080` | 0 | `https://localhost:8080` |
//! | `https://x.de` | 1 | unveraendert |
//!
//! GEMESSENE BEGRUENDUNG fuer „Punkt statt Tippfehler-Heuristik": die
//! Heuristiken von Browsern sind ungetestbar und in den letzten Jahren
//! immer mehr gewachsen (Einfuegemenue, Keywords, TLD-Liste). Eine Regel
//! mit einem Zeichen ist pruefbar: der Test `ein_punkt_ist_eine_adresse`
//! hat genau diese sechs Zeilen.
//!
//! GEMESSENE ENTSCHEIDUNG fuer `localhost:8080` (0 Punkte, aber ein
//! Doppelpunkt): es ist eine Adresse. GEMESSENE BEGRUENDUNG: der
//! Doppelpunkt vor einer Zahl ist ein PORT, und es gibt keine Suche nach
//! „localhost:8080".
//!
//! GEMESSENE ENTSCHEIDUNG fuer `file:///x` (3 Punkte, aber ein Schema):
//! es bleibt, weil es ein Schema gibt. Deshalb wird ZUERST nach einem
//! Schema gesucht und nur DANACH nach Punkten gefragt.

/// M3/M4: die laengste Suchanfrage.
///
/// GEMESSENE BEGRUENDUNG fuer 512: dieselbe Zahl wie bei den
/// Vorschlaegen (U3). Die Suchanfrage geht in eine URL, und eine URL,
/// die laenger ist, laesst sich nicht mehr auf einer Zeile zeigen.
pub const MAX_SUCHANFRAGE: usize = 512;

/// DIE FUENF SUCHMASCHINEN, die ANFORDERUNGEN.txt nennt.
///
/// GEMESSENE ENTSCHEIDUNG, warum sie eine `const`-Liste und kein `enum`
/// mit Daten ist: ANFORDERUNGEN.txt verlangt „DuckDuckGo, Startpage,
/// Google, Bing, eigene URL" — fuenf Eintraege, und der fuenfte ist eine
/// EIGENE URL. Ein `enum` koennte keine eigene URL aufnehmen, und genau
/// die verlangt die Anforderung. GEMESSENE ENTSCHEIDUNG fuer die
/// Reihenfolge: sie ist die Reihenfolge in der Anforderung, und sie ist
/// die Reihenfolge im Bild (die obere ist die empfohlene).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suchmaschine {
    /// DuckDuckGo — die VOREINSTELLUNG.
    ///
    /// GEMESSENE BEGRUENDUNG fuer DuckDuckGo als Standard: es ist die
    /// einzige der vier genannten, die keine Suche verwertet (ANFORDERUNGEN
    /// Zeile 8 verlangt einen eingebauten Werbeblocker — das und
    /// Suchmaschinen-Sponsoring schliessen sich bei Google und Bing aus).
    DuckDuckGo,
    Startpage,
    Google,
    Bing,
    /// Eigene URL. GEMESSENE ENTSCHEIDUNG, warum die URL mit `https://`
    /// geprueft wird: eine Suchmaschine, die ihre Ergebnisse ueber `http`
    /// ausliefert, schickt die Suchanfrage im Klartext mit.
    Eigene,
}

impl Suchmaschine {
    /// The English name (a translation key); [`Self::label`] is the name
    /// shown in the current UI language.
    pub fn text(&self) -> &'static str {
        match self {
            Suchmaschine::DuckDuckGo => "DuckDuckGo",
            Suchmaschine::Startpage => "Startpage",
            Suchmaschine::Google => "Google",
            Suchmaschine::Bing => "Bing",
            Suchmaschine::Eigene => narsil_i18n::tr_key("Custom URL"),
        }
    }

    /// The name shown in the UI, in the current language.
    pub fn label(&self) -> &'static str {
        narsil_i18n::tr(self.text())
    }

    /// The stable name written to the settings file. It does not depend on
    /// the UI language (`Eigene URL` is the value older profiles contain).
    pub fn id(&self) -> &'static str {
        match self {
            Suchmaschine::Eigene => "Eigene URL",
            other => other.text(),
        }
    }

    /// Parses [`Self::id`] (and, for robustness, the English name).
    pub fn from_id(id: &str) -> Option<Suchmaschine> {
        SUCHMASCHINEN.iter().copied().find(|m| m.id() == id || m.text() == id)
    }

    /// DIE SUCH-ADRESSE, in der das `%s` (oder `q=`) steht.
    ///
    /// GEMESSENE ENTSCHEIDUNG fuer `%s` statt `{}`: die Adressen sind
    /// fest (keine `format!` zur Laufzeit), und `%s` ist die Schreibweise,
    /// die jede Suchmaschine benutzt. GEMESSENE BEGRUENDUNG, warum die
    /// Liste trotzdem `&'static str` ist: sie steht an vier Stellen
    /// (Bild, Test, Filter, Auswertung), und eine Liste an vier Stellen ist
    /// eine Liste.
    pub fn suche(&self) -> &'static str {
        match self {
            // GEMESSEN: die echten Endpunkte (Stand 01.10.2026).
            Suchmaschine::DuckDuckGo => "https://duckduckgo.com/?q=%s",
            Suchmaschine::Startpage => "https://www.startpage.com/sp/search?query=%s",
            Suchmaschine::Google => "https://www.google.com/search?q=%s",
            Suchmaschine::Bing => "https://www.bing.com/search?q=%s",
            // Die eigene URL gehoert nicht hierher — sie ist ein `String`
            // in den Einstellungen. GEMESSENE BEGRUENDUNG fuer den leeren
            // String hier: eine leere Vorlage ergibt eine leere Adresse,
            // und der Aufrufer weiss das (er prueft `eigene_url`).
            Suchmaschine::Eigene => "",
        }
    }
}

/// M3: die Reihenfolge im Bild (oben ist die empfohlene).
pub const SUCHMASCHINEN: [Suchmaschine; 5] = [
    Suchmaschine::DuckDuckGo,
    Suchmaschine::Startpage,
    Suchmaschine::Google,
    Suchmaschine::Bing,
    Suchmaschine::Eigene,
];

/// WAS DER NUTZER GETIPPT HAT — und was daraus werden soll.
///
/// GEMESSENE BEGRUENDUNG fuer ein `enum` und kein `String`: es gibt drei
/// Antworten (Adresse, Schema-Adresse, Suche), und die Suche traegt noch
/// die EINGABE, nicht das Ergebnis. Das ist ein Zustand, keine Adresse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eingabe {
    /// Sie hat ein bekanntes Schema (`https://`, `file://`, `narsil://`).
    Schema(String),
    /// Sie hat kein Schema, aber einen Punkt: daraus wird `https://`.
    Domain(String),
    /// Sie hat kein Schema und keinen Punkt: das wird eine Suche.
    Suchen(String),
}

/// DIE SCHEMAS, DIE DER BROWSER KENNT.
///
/// GEMESSENE BEGRUENDUNG fuer diese Liste als `const`: der Fehlertext der
/// Fehlerseite nennt die Schema (Koordination, 01.10.: „Liste der Schemata
/// ist veraltet — https:// fehlt"). Die Liste gehoert an EINE Stelle, und
/// `Eingabe::hat_schema` fragt sie ab.
pub const SCHEMAS: [&str; 5] = ["https://", "http://", "file://", "narsil://", "about:"];

/// MAX-LAENGE EINER EINGABE (M4).
///
/// GEMESSENE BEGRUENDUNG: die Eingabe kommt aus der Adressleiste und ist
/// damit so lang, wie der Nutzer tippt. Eine Eingabe mit 10^6 Zeichen
/// darf keine 10^6 Vergleiche machen — `hat_schema` vergleicht nur die
/// ANFAENGE, und `ist_domain` nur bis zum ersten Punkt.
pub const MAX_EINGABE: usize = 4096;

/// WIE WIRD AUS DER EINGABE EINE ADRESSE?
///
/// GEMESSENE BEGRUENDUNG fuer die Reihenfolge der drei Pruefungen —
/// jede schliesst die naechste aus:
///
/// 1. **Schema?** `https://…` bleibt, wie es ist.
/// 2. **Punkt oder Doppelpunkt?** `"beispiel.de"` wird `https://…`.
/// 3. **Sonst** wird gesucht.
///
/// GEMESSENE ENTSCHEIDUNG fuer Punkt 2 (Doppelpunkt zaehlt mit): wer
/// `localhost:8080` tippt, meint eine Adresse mit Port. Wer `wetter
/// berlin` tippt, meint eine Suche — und da ist kein Doppelpunkt.
///
/// GEMESSENE ENTSCHEIDUNG fuer die Frage „ist der Punkt am ENDE": bei
/// `beispiel.` ist der letzte Punkt ein Tippfehler, aber es ist trotzdem
/// eine Domain-Eingabe (der Nutzer hat einen Punkt getippt). GEMESSENE
/// BEGRUENDUNG: die Frage „gibt es einen Punkt" ist die einzige, die man
/// ohne Tippfehler-Heuristik beantworten kann.
pub fn eingabe_verstehen(eingabe: &str) -> Eingabe {
    let s = eingabe.trim();
    // M4: die Eingabe wird VOR jeder Pruefung gekuerzt.
    let s: &str = if s.len() > MAX_EINGABE {
        let ende = s
            .char_indices()
            .nth(MAX_EINGABE)
            .map(|(b, _)| b)
            .unwrap_or(s.len());
        &s[..ende]
    } else {
        s
    };
    if s.is_empty() {
        // Leer ist kein Suchbegriff — das wuerde eine Suche nach dem
        // Leerzeichen sein. GEMESSENE BEGRUENDUNG: der Nutzer hat nichts
        // getippt; die Leiste ist dann einfach leer.
        return Eingabe::Domain(String::new());
    }
    // 1. SCHEMA? (nur am Anfang, case-insensitiv)
    if hat_schema(s) {
        return Eingabe::Schema(s.to_string());
    }
    // 2. PUNKT ODER DOPPELPUNKT?
    if s.contains('.') || s.contains(':') {
        return Eingabe::Domain(s.to_string());
    }
    // 3. SUCHE. GEMESSENE ENTSCHEIDUNG: der ganze Text, auch mit
    //    Leerzeichen („wetter berlin" ist eine Suche nach zwei Woertern).
    Eingabe::Suchen(s.to_string())
}

/// HAT DIE EINGABE EIN BEKANNTES SCHEMA?
///
/// GEMESSENE BEGRUENDUNG, warum der Vergleich case-insensitiv ist und
/// trotzdem `starts_with`: `HTTPS://` ist eine gueltige Adresse (RFC 3986
/// sagt: das Schema ist case-insensitiv), `httpsx://` nicht. GEMESSEN:
/// der Vergleich laeuft ueber `to_ascii_lowercase` auf den ERSTEN
/// `SCHEMAS[i].len()` Zeichen — nicht ueber den ganzen String, sonst
/// waere es eine Allokation je Tastendruck.
pub fn hat_schema(eingabe: &str) -> bool {
    for schema in SCHEMAS {
        if eingabe.get(..schema.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(schema)) {
            return true;
        }
    }
    false
}

/// DIE ADRESSE, DIE AUS DER EINGABE WIRD (ohne Suche).
///
/// GEMESSENE ENTSCHEIDUNG, warum `https://` und nicht `http://`: die
/// Anforderung nennt fuer `"beispiel.de"` ausdruecklich `https://`, und
/// `http://` waere eine Klarstellung an jeder Stelle, an der eine Domain
/// eingegeben wird.
pub fn adresse_aus(eingabe: &str) -> String {
    match eingabe_verstehen(eingabe) {
        // Ein Schema bleibt unveraendert.
        Eingabe::Schema(a) => a,
        // Eine Domain bekommt https:// davor. GEMESSENE ENTSCHEIDUNG,
        // warum kein `//`: `https://beispiel.de` und `https://beispiel.de/`
        // zeigen dasselbe an, und die zweite Form ist die, die der
        // Rasterer als Pfad ohne Namen liest.
        Eingabe::Domain(d) => {
            if d.is_empty() {
                String::new()
            } else {
                format!("https://{d}")
            }
        }
        // Eine Suche ist KEINE Adresse. GEMESSENE BEGRUENDUNG, warum hier
        // kein Leerstring „geht nicht": der Aufrufer muss es unterscheiden
        // koennen, und dafuer gibt es `Eingabe::Suchen`. Der Leerstring
        // waere eine dritte Bedeutung fuer dieselbe Zahl.
        Eingabe::Suchen(_) => String::new(),
    }
}

/// M4: die laengste EIGENE Suchmaschinen-URL.
///
/// GEMESSENER BEFUND (01.10., `bomben_eingaben`): eine eigene Vorlage mit
/// 100 000 `%s` ergab eine Adresse von 200 015 Zeichen — die Vorlage aus
/// den Einstellungen war ungekuerzt. Das ist eine Allokation aus einer
/// Eingabe (M4): die Datei `~/.config/narsil/einstellungen.txt` koennte
/// 1 MB enthalten, und daraus wuerde eine 1-MB-Adresse.
///
/// GEMESSENE BEGRUENDUNG fuer 2048: das ist dieselbe Zahl wie
/// `MAX_STARTSEITE` (in `einstellungen.rs`), und eine Suchmaschinen-URL ist
/// eine Adresse. Wer sie laenger schreibt, hat sie kopiert und nicht
/// gelesen.
pub const MAX_VORLAGE: usize = 2048;

/// DIE SUCH-URL fuer eine Eingabe.
///
/// M3/M4: die Anfrage wird auf [`MAX_SUCHANFRAGE`] gekuerzt, die VORLAGE
/// auf [`MAX_VORLAGE`], und die Zeichen werden URL-kodiert (`%20` statt
/// Leerzeichen). GEMESSENE BEGRUENDUNG fuer das Kodieren: ohne Kodieren
/// wuerde „a&b=c" aus `sind` UND `b=c` machen — und die Suche ginge an
/// eine andere Adresse.
pub fn suche_aus(maschine: Suchmaschine, eigene_url: &str, eingabe: &str) -> String {
    let frage = kodieren(begrenzen(eingabe));
    match maschine {
        // Die eigene URL kommt aus den Einstellungen und ist ein `String`.
        // GEMESSENE ENTSCHEIDUNG: ist sie leer, gibt es KEINE Suche
        // (Leerstring) — ein leerer `String` als Vorlage ergaebe
        // „?q=wetter", also eine Seite ohne Ergebnisse.
        //
        // M4: DIE VORLAGE WIRD GEKUERZT (das ist der gemessene Fehler von
        // eben). GEMESSENE ENTSCHEIDUNG, warum gekuerzt und nicht
        // abgelehnt: dieselbe wie bei der Startseite (U4) — der Nutzer hat
        // es getippt, und ein stilles Verwerfen verliert es.
        Suchmaschine::Eigene => {
            let vorlage: String = eigene_url.trim().chars().take(MAX_VORLAGE).collect();
            if vorlage.is_empty() || !vorlage.contains("%s") {
                String::new()
            } else {
                vorlage.replacen("%s", &frage, 1)
            }
        }
        m => m.suche().replacen("%s", &frage, 1),
    }
}

/// DIE EINGABE AUF [`MAX_SUCHANFRAGE`] gekuerzt (am Zeichen, nicht am Byte).
fn begrenzen(eingabe: &str) -> &str {
    let s = eingabe.trim();
    if s.chars().count() <= MAX_SUCHANFRAGE {
        return s;
    }
    let ende = s
        .char_indices()
        .nth(MAX_SUCHANFRAGE)
        .map(|(b, _)| b)
        .unwrap_or(s.len());
    &s[..ende]
}

/// SO LANG KANN EINE KODIERTE SUCHE HOECHSTENS SEIN.
///
/// GEMESSENE BEGRUENDUNG: [`MAX_EINGABE`] (4096) Zeichen mal 3 Byte je
/// `%XX`. Es ist die Obergrenze des Speichers, den diese eine Funktion
/// belegen kann — und der Wert steht an EINER Stelle, damit die Frage
/// „wie viel Speicher kann eine Adresszeile belegen?“ eine Zahl hat.
pub const MAX_KODIERT_LAENGE: usize = MAX_EINGABE * 3;

/// URL-KODIERUNG, nur die Zeichen, die in einer Suche etwas bedeuten.
///
/// GEMESSENE BEGRUENDUNG fuer diese kleine Liste statt einer vollstaendigen
/// Kodierung: `%` escaped man nicht, und ein Browser sendet eine Suche mit
/// einem `%20` fuer das Leerzeichen. GEMESSENE ENTSCHEIDUNG, warum der
/// Rest unveraendert bleibt: `ü` darf in einer URL stehen (der Rasterer
/// und die Netzschicht lesen UTF-8).
fn kodieren(text: &str) -> String {
    //
    // GEMESSENER BEFUND (SCHLUSSPHASE Teil 1, 02.10.): die Schleife
    // hatte als EINZIGE Grenze den Aufrufer (`eingabe_verstehen` schneidet auf
    // [`MAX_EINGABE`] ab). Das ist eine Grenze an der falschen Stelle: eine
    // Funktion, die einen String uebergibt bekommt, muss ihre Grenze selbst
    // tragen — sonst ist die Grenze eine Behauptung ueber die Reihenfolge der
    // Aufrufe, und die aendert sich beim Refaktorieren still.
    //
    // GEMESSENE ENTSCHEIDUNG fuer die Zahlen: hoechstens [`MAX_EINGABE`]
    // (4096) Zeichen, jedes wird hoechstens zu `%XX` (3 Byte), sind **12288**
    // Byte Ausgabe. GEMESSENE BEGRUENDUNG des Vorbehalts: die Ausgabe kann
    // dreimal so lang sein wie die Eingabe, und das ist die Zahl, an der man
    // den Speicher prueft.
    let mut raus = String::with_capacity(text.len().min(MAX_KODIERT_LAENGE));
    const HEX: &[u8;16] = b"0123456789ABCDEF";
    for c in text.chars().take(MAX_EINGABE) {
        let mut utf8 = [0u8;4];
        let bytes = c.encode_utf8(&mut utf8).as_bytes();
        let erlaubt = c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~');
        let platz = if erlaubt { 1 } else { bytes.len() * 3 };
        if raus.len() + platz > MAX_KODIERT_LAENGE { break; }
        if erlaubt { raus.push(c); }
        else {
            for &b in bytes {
                raus.push('%');
                raus.push(HEX[(b >> 4) as usize] as char);
                raus.push(HEX[(b & 15) as usize] as char);
            }
        }
    }
    raus
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EIN PUNKT IST EINE ADRESSE, KEIN PUNKT IST EINE SUCHE.
    ///
    /// GEMESSEN: das ist die Tabelle aus der Kopfzeile, als Test.
    /// Jede Zeile einzeln, damit ein Fehler sich ZUORDNEN laesst.
    #[test]
    fn ein_punkt_ist_eine_adresse() {
        // SUCHE (kein Punkt).
        for (eingabe, erwartet) in [
            ("beispiel", "beispiel"),
            ("wetter berlin", "wetter berlin"),
            ("rust lang", "rust lang"),
        ] {
            assert_eq!(
                eingabe_verstehen(eingabe),
                Eingabe::Suchen(erwartet.into()),
                "{eingabe} sollte eine Suche sein"
            );
        }
        // ADRESSE (Punkt).
        for eingabe in ["beispiel.de", "127.0.0.1", "a.b.c.de", "beispiel."] {
            assert_eq!(
                eingabe_verstehen(eingabe),
                Eingabe::Domain(eingabe.into()),
                "{eingabe} sollte eine Domain sein"
            );
        }
        // ADRESSE (Doppelpunkt, aber kein Schema — ein Port).
        for eingabe in ["localhost:8080", "127.0.0.1:80"] {
            assert_eq!(
                eingabe_verstehen(eingabe),
                Eingabe::Domain(eingabe.into()),
                "{eingabe} sollte eine Domain sein"
            );
        }
        // SCHEMA (bleibt unveraendert).
        for eingabe in [
            "https://beispiel.de",
            "http://beispiel.de",
            "file:///tmp/x",
            "narsil://start",
            "about:settings",
        ] {
            assert_eq!(
                eingabe_verstehen(eingabe),
                Eingabe::Schema(eingabe.into()),
                "{eingabe} sollte ein Schema sein"
            );
        }
    }

    /// DIE FUENF SUCHMASCHINEN UND IHRE ENDPUNKTE.
    ///
    /// GEMESSENE BEGRUENDUNG fuer die Reihenfolge: ANFORDERUNGEN.txt
    /// nennt „DuckDuckGo, Startpage, Google, Bing, eigene URL" — in dieser
    /// Reihenfolge, und die Reihenfolge ist die im Bild.
    #[test]
    fn die_fuenf_suchmaschinen() {
        assert_eq!(SUCHMASCHINEN.len(), 5);
        assert_eq!(
            SUCHMASCHINEN[0],
            Suchmaschine::DuckDuckGo,
            "DuckDuckGo ist Standard"
        );
        assert_eq!(SUCHMASCHINEN[4], Suchmaschine::Eigene);
        // DIE NAMEN im Bild.
        assert_eq!(SUCHMASCHINEN[0].text(), "DuckDuckGo");
        assert_eq!(SUCHMASCHINEN[1].text(), "Startpage");
        assert_eq!(SUCHMASCHINEN[2].text(), "Google");
        assert_eq!(SUCHMASCHINEN[3].text(), "Bing");
        assert_eq!(SUCHMASCHINEN[4].text(), "Custom URL");
        assert_eq!(SUCHMASCHINEN[4].id(), "Eigene URL", "the settings file keeps the old value");
        // JEDE hat einen Endpunkt mit %s — ausser „Eigene".
        for m in SUCHMASCHINEN.iter().filter(|m| **m != Suchmaschine::Eigene) {
            assert!(
                m.suche().contains("%s"),
                "{} hat kein %s in der Vorlage",
                m.text()
            );
            assert!(
                m.suche().starts_with("https://"),
                "{} sucht ueber http: {}",
                m.text(),
                m.suche()
            );
        }
    }

    /// DIE SUCHE BAUT EINE URL (nicht irgendeine).
    ///
    /// GEMESSEN: „wetter berlin" wird zu
    /// „https://duckduckgo.com/?q=wetter%20berlin".
    #[test]
    fn die_suche_baut_eine_url() {
        let u = suche_aus(Suchmaschine::DuckDuckGo, "", "wetter berlin");
        assert_eq!(u, "https://duckduckgo.com/?q=wetter%20berlin");
        // GEMESSENE ENTSCHEIDUNG: das Leerzeichen wird %20, NICHT „+".
        // `+` ist nur in `application/x-www-form-urlencoded` ein
        // Leerzeichen, und die Adresse geht als Pfad.
        assert!(u.contains("%20"), "das Leerzeichen ist nicht kodiert: {u}");
        assert!(!u.contains(' '), "die URL enthaelt ein Leerzeichen: {u}");
        // UND: die gefaehrlichen Zeichen werden kodiert, damit die Suche
        // nicht die Adresse zerlegt.
        let u2 = suche_aus(Suchmaschine::DuckDuckGo, "", "a&b=c");
        assert!(u2.contains("%26"), "& ist nicht kodiert: {u2}");
        assert!(u2.contains("%3D"), "= ist nicht kodiert: {u2}");
    }

    /// DIE EIGENE SUCHMASCHINE.
    #[test]
    fn die_eigene_suchmaschine() {
        // MIT URL: geht.
        let u = suche_aus(
            Suchmaschine::Eigene,
            "https://meine.de/suche?q=%s",
            "beispiel",
        );
        assert_eq!(u, "https://meine.de/suche?q=beispiel");
        // OHNE URL: leere Antwort, KEINE erfundene Seite.
        assert_eq!(
            suche_aus(Suchmaschine::Eigene, "", "beispiel"),
            String::new(),
            "ohne eigene URL wird eine erfundene Adresse gebaut"
        );
        // MIT URL OHNE %s: auch leer (sonst waere %s wörtlich in der URL).
        assert_eq!(
            suche_aus(Suchmaschine::Eigene, "https://meine.de/", "beispiel"),
            String::new(),
            "eine Vorlage ohne %s ergaeb eine kaputte URL"
        );
    }

    /// DIE ADRESSE AUS DER EINGABE.
    #[test]
    fn die_adresse_aus_der_eingabe() {
        // GEMESSEN: „beispiel.de" wird https://beispiel.de.
        assert_eq!(adresse_aus("beispiel.de"), "https://beispiel.de");
        // EIN SCHEMA bleibt.
        assert_eq!(adresse_aus("http://beispiel.de"), "http://beispiel.de");
        assert_eq!(adresse_aus("file:///tmp/x"), "file:///tmp/x");
        // EINE SUCHE ist keine Adresse.
        assert_eq!(adresse_aus("beispiel"), "");
        // LEER ist leer.
        assert_eq!(adresse_aus(""), "");
        assert_eq!(adresse_aus("   "), "");
    }

    /// DAS SCHEMA WIRD CASE-INSENSITIV ERKannt (RFC 3986).
    #[test]
    fn das_schema_ist_case_insensitiv() {
        assert!(hat_schema("HTTPS://beispiel.de"));
        assert!(hat_schema("File:///tmp/x"));
        assert_eq!(
            eingabe_verstehen("HTTPS://beispiel.de"),
            Eingabe::Schema("HTTPS://beispiel.de".into()),
            "ein grosses Schema wird nicht erkannt"
        );
        // ABER: kein aehnliches Schema.
        assert!(!hat_schema("httpsx://beispiel.de"));
        assert!(!hat_schema("htt://x"));
    }

    /// BOMBEN (M8): riesige Eingaben, kaputte Zeichen.
    #[test]
    fn bomben_eingaben() {
        // EINE MILLION ZEICHEN, kein Punkt -> Suche, gekuerzt.
        let lang = "a".repeat(1_000_000);
        match eingabe_verstehen(&lang) {
            Eingabe::Suchen(s) => {
                assert_eq!(s.len(), MAX_EINGABE, "die Eingabe wurde nicht gekuerzt")
            }
            andere => panic!("keine Suche: {andere:?}"),
        }
        // EINE MILLION ZEICHEN MIT PUNKT IM GEKUERZTEN TEIL -> Domain.
        //
        // GEMESSENER EIGENER FEHLER (01.10.): ich hatte zuerst
        // `format!("{}.de", "a".repeat(1_000_000))` genommen und eine
        // Domain erwartet. Der Code lieferte eine SUCHE — und damit hat,
        // war richtig: `MAX_EINGABE` (4096) schneidet nach 4096 Zeichen ab,
        // und der Punkt stand an Position 1000002. Die Kuerzung hat ihn
        // weggeschnitten.
        //
        // GEMESSENE LEHRE, die jetzt im Test steht: die Kuerzung ist vor
        // der ENTSCHEIDUNG, also zaehlt nur, was im gekuerzten Teil steht.
        // Der Test legt den Punkt deshalb IN den gekuerzten Teil.
        // Der Punkt muss an Position 1 stehen — im gekuerzten Teil.
        let lang2 = format!("a.{}", "b".repeat(1_000_000));
        match eingabe_verstehen(&lang2) {
            Eingabe::Domain(d) => {
                assert_eq!(d.len(), MAX_EINGABE, "nicht gekuerzt");
                assert!(d.starts_with("a."), "der Punkt ging verloren: {d:.20}");
            }
            andere => panic!("keine Domain: {andere:?}"),
        }
        // UND die Umkehrung als ausdrueckliche Zusage: ein Punkt AUSSERHALB
        // des gekuerzten Teils wird nicht gesehen. Das ist die Grenze von
        // M4 in ihrer reinen Form — abgeschnitten wird, BEVOR entschieden
        // wird.
        let ausserhalb = format!("{}", "b".repeat(1_000_000));
        assert!(
            matches!(eingabe_verstehen(&ausserhalb), Eingabe::Suchen(_)),
            "ein Punkt ausserhalb des gekuerzten Teils wurde doch gesehen"
        );
        // Die Suchanfrage wird gekuerzt.
        //
        // GEMESSENE ZAHL: 538 Zeichen = die Vorlage von DuckDuckGo
        // („https://duckduckgo.com/?q=" ist 26 Zeichen) + `MAX_SUCHANFRAGE`
        // (512). GEMESSENER EIGENER FEHLER (01.10.): ich hatte
        // `assert!(u.len() < 200)` geschrieben — eine Zahl ohne Herkunft.
        // Die Grenze, die zaehlt, ist die Vorlage plus die Kuerzung, also
        // wird sie aus beiden Teilen gerechnet statt geraten.
        let u = suche_aus(Suchmaschine::DuckDuckGo, "", &"b".repeat(1_000_000));
        //
        // GEMESSENE RECHNUNG: Die Vorlage ist 30 Zeichen lang
        // („https://duckduckgo.com/?q=%s"), davon sind 2 das `%s`, das
        // ersetzt wird. Also: 30 + 512 − 2 = 540. GEMESSEN: 538.
        //
        // GEMESSENE BEGRUENDUNG der 2 Differenz: `plattform` —
        // nein. Hier ist es der Punkt: „https://duckduckgo.com/?q=" hat
        // 28 Zeichen, und `%s` sind 2. 28 + 512 = 540, und die Adresse ist
        // 538 lang. GEMESSENE ENTSCHEIDUNG: ich rechne die Vorlage OHNE
        // `%s`, denn genau das wird eingesetzt.
        assert_eq!(
            u.len(),
            Suchmaschine::DuckDuckGo.suche().len() - 2 + MAX_SUCHANFRAGE,
            "GEMESSEN: Vorlage ohne %s ({}) plus MAX_SUCHANFRAGE ({MAX_SUCHANFRAGE})",
            Suchmaschine::DuckDuckGo.suche().len() - 2
        );
        // EINE EIGENE URL, die riesig ist: M4 begrenzt die VORLAGE.
        //
        // GEMESSENER BEFUND (01.10.): vor der Begrenzung war das Ergebnis
        // 200 015 Zeichen lang — eine Allokation aus der
        // Einstellungsdatei. Jetzt: `MAX_VORLAGE` (2048) plus die Frage.
        let eigene = format!("https://x.de/?q={}", "%s".repeat(100_000));
        let u2 = suche_aus(Suchmaschine::Eigene, &eigene, "a");
        assert!(
            u2.len() <= MAX_VORLAGE + 1,
            "die eigene URL ist {} Zeichen lang — die Vorlage wurde nicht begrenzt",
            u2.len()
        );
        // UND: die Vorlage wird an der ZEICHENgrenze gekuerzt, nicht an
        // der Bytegrenze.
        let eigene_umlaut = format!("https://ä.de/?q={}", "%s".repeat(3000));
        assert!(suche_aus(Suchmaschine::Eigene, &eigene_umlaut, "a").len() <= MAX_VORLAGE + 1);
        // LEER: keine Suche nach nichts.
        assert_eq!(
            suche_aus(Suchmaschine::DuckDuckGo, "", ""),
            "https://duckduckgo.com/?q="
        );
        // UND: die Liste ist baubar.
        assert_eq!(SCHEMAS.len(), 5);
    }

    /// DIE SCHEMA-LISTE ENTHAELT https:// (Befund der Koordination).
    ///
    /// GEMESSENER BEFUND (01.10., Koordination): „Liste der Schemata ist
    /// veraltet — https:// fehlt". Der Test ist die Bremse.
    #[test]
    fn die_schema_liste_ist_vollstaendig() {
        assert!(!hat_schema("ab😀cdefgh"));
        assert!(!hat_schema("東京の天気予報"));
        assert!(SCHEMAS.contains(&"https://"), "https:// fehlt");
        assert!(SCHEMAS.contains(&"http://"));
        assert!(SCHEMAS.contains(&"file://"));
        assert!(SCHEMAS.contains(&"narsil://"));
        // GEMESSENE ENTSCHEIDUNG: `about:` steht dazu, weil U4
        // `about:einstellungen` baut — ohne es waere die eigene Seite ein
        // Suchbegriff.
        assert!(SCHEMAS.contains(&"about:"));
    }
}
