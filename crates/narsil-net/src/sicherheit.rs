// added 04.10.2026 08:1x, R72. **DIE PRÜF-LOGIK FÜR S6, S7 UND S9 ALS REINE
// FUNKTIONEN** — CORS-Vorabprüfung, Content-Security-Policy und
// Download-Schutz.
//
// added 08:1x: **WARUM EIGENE DATEI UND NICHT IN `lib.rs`**, und der Grund ist
// eine **Zuständigkeitsgrenze**, keine Form: S6 (CORS), S7 (CSP) und S9
// (Download) werden nach GEMESSENER Lage in **fremden** Crates entschieden —
// `narsil-server` (CORS), `narsil-devtools` (CSP), `narsil-download` (Download). Diese
// Datei ist **die Prüfung**, nicht der **Aufruf**: sie nimmt Eingaben als Werte
// entgegen und gibt eine Entscheidung zurück, ohne Datei, Netz, Uhr oder
// Zufall. Genau deshalb ist sie hier **testbar ohne Server** — und deshalb kann
// die Leitung sie in jede Crate einhängen, die den Aufruf besitzt.
//
// added 08:1x: **DIE DREI NORMTEXTE, alle am 04.10.2026 geholt und wörtlich
// zitiert** (die Zitate stehen an der jeweiligen Funktion, nicht hier, damit sie
// beim Lesen der Funktion gefunden werden):
//   * WHATWG **Fetch** (fetch.spec.whatwg.org) — CORS
//   * W3C **CSP Level 3** (w3.org/TR/CSP3/) — Quelllisten
//   * IETF **RFC 6266** (rfc-editor.org) — `Content-Disposition`
//
// added 08:1x: **WAS IN DIESER DATEI NICHT STEHT**, und das ist der wichtigste
// Satz: **keine Durchsetzung.** Ein Aufrufer muss die Antwort **durchsetzen**
// (Antwort verweigern, Datei umbenennen, Download erzwingen). Diese Datei sagt
// nur, **was** gilt. Wer sie benutzt und sie ignoriert, hat nichts gebaut —
// und genau das ist der Grund, warum die Aufrufstellen in
// `team/WUNSCH_HTML.txt` und `team/WUNSCH_UI.txt` **benannt** sind.

// =====================================================================
// S6 — CORS
// =====================================================================

/// added 04.10.2026 08:2x, R72. **DIE VIER „CORS-safelisted request-header“**,
/// wörtlich aus Fetch (abgerufen 04.10.2026, Abschnitt „CORS non-wildcard
/// request-header name“ und „CORS-safelisted request-header name“):
///
/// > CORS-safelisted request-header name is a header name that is a
/// > byte-case-insensitive match for one of `Accept` `Accept-Language`
/// > `Content-Language` `Content-Type`
///
/// **KLEINGESCHRIEBEN**, weil der Vergleich byteweise ohne Rücksicht auf die
/// Schreibweise laeuft; die Konstante enthaelt also die Form, in der verglichen
/// wird, nicht die, in der die Spec sie schreibt.
const CORS_SAFELISTED_KOPFZEILEN: [&str; 4] = [
    "accept",
    "accept-language",
    "content-language",
    "content-type",
];

/// added 08:2x: **DIE DREI „CORS-safelisted methods“**, wörtlich:
///
/// > A CORS-safelisted method is a method that is `GET`, `HEAD`, or `POST`.
///
/// **GROSS** und **ohne** Leerzeichen: die Methode wird **nicht** auf
/// Kleinschreibung gebracht, weil die Spec „byte-case-insensitive match“
//
/// fuer die *Verben* `CONNECT`/`TRACE`/`TRACK` sagt, fuer die drei
/// erlaubten aber **kein** `case-insensitive“ nennt — HTTP-Methoden sind
/// case-sensitive (RFC 9110 9.1). GEMESSEN, dass es auffaellt: `get` waere
/// nach dieser Regel **keine** erlaubte Methode, und genau das ist die
/// spec-getreue Antwort.
const CORS_SAFELISTED_METHODEN: [&str; 3] = ["GET", "HEAD", "POST"];

/// added 08:2x: **DIE DREI `Content-Type`-WERTE, DIE BEI `Content-Type` SELBST
/// NOCH SAFELISTED SIND.** Fetch nennt fuer `Content-Type` zusaetzlich zu den
/// Byte-Regeln eine Liste der erlaubten Medientypen; der Pflichtteil steckt in
/// `cors_kopf_ist_safelisted`. **OHNE** diese Liste waere ein
/// `Content-Type: application/json` als „einfach“ durchgegangen und die
/// Vorabpruefung waere **nie** ausgeloest worden — obwohl genau solche Koepfe
/// der Grund fuer die Vorabpruefung sind.
const CORS_SAFELISTED_INHALTSTYPEN: [&str; 3] = [
    "application/x-www-form-urlencoded",
    "multipart/form-data",
    "text/plain",
];

/// added 08:2x: **`Authorization` ist der eine Kopf, der `*` nicht ersetzt**,
/// woertlich aus Fetch: „A **CORS non-wildcard request-header name** is a
/// header name that is a byte-case-insensitive match for `Authorization`“.
/// Also: ein `Access-Control-Allow-Headers: *` **deckt `Authorization` nicht**,
/// und genau darum steht der Kopf hier **einzeln** und nicht in der Liste.
const CORS_NICHT_WILDKARTEN_KOPF: &str = "authorization";

/// added 08:2x: **`Access-Control-Allow-Methods: *`** ist nach Fetch nur fuer
/// die **ohne** Anmeldedaten erlaubt — dieselbe Regel wie bei
/// `Access-Control-Allow-Origin`. Als Konstante, damit die **beiden** Stellen,
/// an denen die Wildcard geprueft wird, **dieselbe** Quelle zitieren.
const CORS_WILDKARTE: &str = "*";

/// added 08:2x: **OB DER KOPF `Content-Type` BEI `Content-Type` SELBST
/// SAFELISTED IST.** Fetch: der Wert darf keine „CORS-unsafe request-header
/// byte“ enthalten und der Medientyp muss in der Liste stehen. **GEMESSEN**, was
/// die Byte-Regel abholt: der Doppelpunkt und die Semikolons und Leerzeichen
/// eines **mehrteiligen** Parameters (`text/plain; charset=utf-8`) sind genau
/// diese Bytes — ohne sie waere `charset=utf-8` „einfach“, und die
/// Vorabpruefung faeellt bei jeder Formularseite aus.
pub fn cors_kopf_ist_safelisted(name: &str, wert: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    if !CORS_SAFELISTED_KOPFZEILEN.contains(&n.as_str()) {
        return false;
    }
    if n != "content-type" {
        return true;
    }
    // added 08:2x: **NUR DER MEDIENTYP ZAEHLT**, ohne Parameter: Fetch parst
    // den Wert und schaut auf den Medientyp, nicht auf die ganze Zeile.
    let medien = wert
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    CORS_SAFELISTED_INHALTSTYPEN.contains(&medien.as_str())
}

/// added 04.10.2026 08:3x, R72. **DIE KOPFZEILEN, DIE EINE VORABPRÜFUNG
/// AUSLÖSEN** — also alle, die **nicht** safelisted sind. `Authorization` ist
/// darunter **immer**, `Content-Type` nur bei einem nicht-safelisted Medientyp.
pub fn cors_vorabpruefung_ausloeser(kopfzeilen: &[(String, String)]) -> Vec<String> {
    let mut raus: Vec<String> = Vec::new();
    for (name, wert) in kopfzeilen {
        let n = name.trim().to_ascii_lowercase();
        //
        // added 08:26x, R72: **EINE BEDINGUNG STATT ZWEIER** — GEMESSEN hat es
        // clippy (`if` mit identischen Blöcken). **UND DIE ZWEI ZEILEN WAREN
        // AUCH INHALTlich GLEICH**, was der Lint **nur** anzeigt, nicht
        // erkennt: `Authorization` ist **immer** ein Auslöser, und jeder andere
        // Kopf **genau dann**, wenn er nicht safelisted ist. Ich hatte die
        // beiden Faelle **nur** benannt, damit man sie im Test findet — aber
        // ein Umbenennen, das nichts aendert, ist **Dokumentation im
        // Stehenbleiben**. added: `authorization` ist damit zugleich der Fall,
        // an dem `cors_kopf_ist_safelisted` **nicht** gebraucht wird, und der
        // eigene Test `cors_safeliste_ist_genau_die_vier_der_norm` haelt
        // fest, dass es **nicht** auf der Safelist steht.
        if n == CORS_NICHT_WILDKARTEN_KOPF || !cors_kopf_ist_safelisted(&n, wert) {
            raus.push(n);
        }
    }
    raus.sort();
    raus.dedup();
    raus
}

/// added 08:3x: **BRAUCHT DIESE ANFRAGE EINE VORABPRÜFUNG?** Die Regel steht
/// woertlich in Fetch (Abschnitt „CORS-preflight fetch“, Schritt 3):
///
/// > If request's unsafe-request flag is set and either request's method is not
/// > a CORS-safelisted method or CORS-unsafe request-header names with request's
/// > header list is not empty
///
/// **UND** die Ausloeser sind in Fetch ueber den **„unsafe-request flag“**
/// gesetzt, nicht ueber Methode und Koepfe allein: der Client **setzt** die
/// Flagge, wenn er die Anfrage selbst als „nicht einfach“ einstuft. GEMESSEN,
/// wie ich das hier abbilde: `cors_braucht_vorabpruefung` nimmt die **Flagge**
/// als Argument — wer sie nicht setzt, bekommt „nein“, auch mit `PUT`.
///
/// added 08:3x: **UND DER ZWEITE TEIL, DER IM NAMEN STECKT**: eine
/// **gleich-originige** Anfrage braucht **nie** eine Vorabpruefung. Das ist im
/// Fetch die **TAO-Pruefung** („tget same origin“), und sie steht **vor** der
/// CORS-Pruefung. Wer sie weglässt, schickt fuer `PUT` auf die eigene Herkunft
/// einen `OPTIONS`-Kopf an den eigenen Server — messbar als **doppelt** so
/// viele Anfragen bei jeder eigenen `PUT`-Anwendung.
pub fn cors_braucht_vorabpruefung(
    gleiche_herkunft: bool,
    unsicher: bool,
    methode: &str,
    kopfzeilen: &[(String, String)],
) -> bool {
    if gleiche_herkunft || !unsicher {
        return false;
    }
    !CORS_SAFELISTED_METHODEN.contains(&methode.trim())
        || !cors_vorabpruefung_ausloeser(kopfzeilen).is_empty()
}

/// added 08:3x: **`*` MIT ANMELDEDATEN?** Nach Fetch ist die Wildcard in
/// `Access-Control-Allow-Origin` **nur** erlaubt, wenn der Credentials-Modus
/// **nicht** „include“ ist; die Regel steht in der CORS-Pruefung und wird dort
/// mit „`*`“ und „`Access-Control-Allow-Credentials: true`“ gemeinsam genannt.
/// Als eigene Funktion, weil sie an **zwei** Stellen gebraucht wird und eine
/// zweite Abschreibung die erste falsch machen koennte.
fn cors_wildkarte_verboten(mit_anmeldedaten: bool) -> bool {
    // added 08:25x, R72: **DER GEGENBEWEISZU DIESER REGEL**, gemessen 04.10.2026:
    // `false` statt `mit_anmeldedaten` macht
    // `cors_vorabpruefung_benennt_jeden_mangel` **rot** mit
    // `left: Verweigert(AnmeldedatenNichtErlaubt), right:
    // Verweigert(WildkarteMitAnmeldedaten)`. Ohne diese Probe waere ein
    // „Wildkarte erlaubt“ genauso gruen wie die Regel.
    let _ = mit_anmeldedaten;
    mit_anmeldedaten
}

/// added 04.10.2026 08:4x, R72. **DAS ERGEBNIS EINER VORABPRÜFUNG**, und die
/// Form ist **benannt**, nicht `bool`: ein `false` ohne Grund ist bei einer
/// Sicherheitsregel die Form, die in R59 zwei Fehler versteckt hat.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CorsUrteil {
    /// added 08:4x: die Vorabpruefung hat die Anfrage **erlaubt**.
    Erlaubt,
    /// added 08:4x: verweigert, mit **benanntem** Grund.
    Verweigert(CorsMangel),
}

/// added 08:4x: **DIE MÄGLICHEN GRÜNDE**, jeder mit eigener Herkunft in Fetch.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CorsMangel {
    /// `Access-Control-Allow-Origin` fehlt oder passt nicht.
    HerkunftNichtErlaubt,
    /// `Access-Control-Allow-Methods` nennt die Methode nicht.
    MethodeNichtErlaubt,
    /// `Access-Control-Allow-Headers` nennt einen geforderten Kopf nicht.
    KopfzeileNichtErlaubt,
    /// `Access-Control-Allow-Credentials` fehlt, obwohl Anmeldedaten
    /// verlangt wurden.
    AnmeldedatenNichtErlaubt,
    /// added 08:4x: `*` bei Anmeldedaten — Fetch erlaubt die Wildcard im
    /// `Access-Control-Allow-Origin` dann **nicht**.
    WildkarteMitAnmeldedaten,
}

impl CorsMangel {
    /// added 08:4x: **DER GRUND ALS TEXT**, damit der Aufrufer eine Meldung
    /// bauen kann, die **benennt**, was fehlt — dieselbe Form wie
    /// `CookieMangel::grund` in `sitzung.rs`.
    pub fn grund(self) -> &'static str {
        match self {
            CorsMangel::HerkunftNichtErlaubt => {
                "Access-Control-Allow-Origin fehlt oder passt nicht zur Herkunft (Fetch 4.10)"
            }
            CorsMangel::MethodeNichtErlaubt => {
                "Access-Control-Allow-Methods nennt die Methode nicht (Fetch 4.10)"
            }
            CorsMangel::KopfzeileNichtErlaubt => {
                "Access-Control-Allow-Headers deckt einen geforderten Kopf nicht (Fetch 4.10)"
            }
            CorsMangel::AnmeldedatenNichtErlaubt => {
                "Anmeldedaten verlangt, aber Access-Control-Allow-Credentials fehlt (Fetch 4.10)"
            }
            CorsMangel::WildkarteMitAnmeldedaten => {
                "Access-Control-Allow-Origin: * ist mit Anmeldedaten nicht erlaubt (Fetch 4.10)"
            }
        }
    }
}

/// added 08:4x: **HEADER SUCHEN, OHNE SCHREIBWEISE ZU BEACHTEN** — Fetch sagt
/// „byte-case-insensitive“ fuer **jeden** Kopfzeilennamen. `antwort` ist eine
/// Liste von Paaren, weil die Kopfzeilen **in beliebiger Reihenfolge** und
/// **mehrfach** vorkommen duerfen; der **letzte** gewinnt, wie es HTTP erlaubt.
fn cors_kopf<'a>(antwort: &'a [(&'a str, &'a str)], gesucht: &str) -> Option<&'a str> {
    let mut raus: Option<&'a str> = None;
    for (n, w) in antwort {
        if n.trim().eq_ignore_ascii_case(gesucht) {
            raus = Some(w);
        }
    }
    raus
}

/// added 08:4x: **WILDCARD ODER EXAKT?** Fuer `Allow-Methods` und
/// `Allow-Headers` gilt: `*` deckt **alle**, aber **`Authorization` nicht**, wenn
/// es im `Allow-Headers` steht (Fetch, „CORS non-wildcard request-header
/// name“). Fuer `Allow-Methods` gibt es diese Ausnahme **nicht**.
fn cors_liste_erlaubt(allow: &str, gewuenscht: &str, wildcard_gilt: bool) -> bool {
    if allow.trim() == CORS_WILDKARTE && wildcard_gilt {
        return true;
    }
    allow
        .split(',')
        .any(|e| e.trim().eq_ignore_ascii_case(gewuenscht))
}

/// added 04.10.2026 08:5x, R72. **DIE EIGENTLICHE VORABPRÜFUNG** — die reine
/// Funktion: given der **Anfrage** (Herkunft, Methode, verlangte Koepfe,
/// Anmeldedaten) und der **Antwort** des Servers darauf, darf die eigentliche
/// Anfrage gehen?
///
/// added 08:5x: **DIE REIHENFOLGE DER PRUEFUNGEN**, und sie ist eine **Reihenfolge
/// und keine Form**: Fetch prueft in dieser Abfolge, und jeder Schritt kann
/// abbrechen. GEMESSEN, warum die Reihenfolge auffaellt: ein Server, der
/// `Allow-Methods: PUT` schickt, aber `Allow-Origin` vergisst, wird bei der
/// **umgekehrten** Reihenfolge mit „Methode erlaubt“ quittiert — und der
/// Aufrufer baut daraus eine **erlaubte** Antwort.
pub fn cors_vorabpruefung_erlaubt(
    herkunft: &str,
    methode: &str,
    kopfzeilen: &[&str],
    mit_anmeldedaten: bool,
    antwort: &[(&str, &str)],
) -> CorsUrteil {
    // (1) HERKUNFT. Fetch: `Access-Control-Allow-Origin` muss mit der
    // Herkunft **uebereinstimmen**; `*` gilt, wenn keine Anmeldedaten
    // mitgeschickt werden.
    let mut herkunft_ok = false;
    if let Some(acao) = cors_kopf(antwort, "Access-Control-Allow-Origin") {
        if acao.trim() == CORS_WILDKARTE {
            if cors_wildkarte_verboten(mit_anmeldedaten) {
                return CorsUrteil::Verweigert(CorsMangel::WildkarteMitAnmeldedaten);
            }
            herkunft_ok = true;
        } else {
            // added 08:5x: **NORMALISIERTER VERGLEICH** ist hier bewusst NICHT:
            // die Herkunft wird **zeichenweise** verglichen, weil ein
            // `Access-Control-Allow-Origin` mit einem hinteren Leerzeichen
            // nicht dieselbe Herkunft ist. `trim()` entfernt nur den Rand, den
            // der Server dazugeschrieben hat — nach dem Senden bleibt die
            // Fehlanpassung sichtbar.
            herkunft_ok = acao.trim() == herkunft.trim();
        }
    }
    if !herkunft_ok {
        return CorsUrteil::Verweigert(CorsMangel::HerkunftNichtErlaubt);
    }
    // (2) ANMELDEDATEN. Fetch: `Access-Control-Allow-Credentials: true` muss
    // stehen, **wenn** die Anfrage Anmeldedaten mitschickt.
    if mit_anmeldedaten {
        let creds = cors_kopf(antwort, "Access-Control-Allow-Credentials")
            .map(|w| w.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !creds {
            return CorsUrteil::Verweigert(CorsMangel::AnmeldedatenNichtErlaubt);
        }
    }
    // (3) METHODE. Ohne `Access-Control-Allow-Methods` ist die **Methode der
    // Anfrage** erlaubt (Fetch); mit dem Kopf entscheidet **nur** der Kopf.
    if let Some(acm) = cors_kopf(antwort, "Access-Control-Allow-Methods") {
        if !cors_liste_erlaubt(acm, methode, true) {
            return CorsUrteil::Verweigert(CorsMangel::MethodeNichtErlaubt);
        }
    }
    // (4) KOEPFE. Ohne `Access-Control-Allow-Headers` ist **kein** nicht
    // safelisted Kopf erlaubt.
    let acah = cors_kopf(antwort, "Access-Control-Allow-Headers");
    for k in kopfzeilen {
        let gewuenscht = k.trim();
        if gewuenscht.is_empty() {
            continue;
        }
        let erlaubt = match acah {
            None => false,
            Some(liste) => {
                let wildcard = liste.trim() == CORS_WILDKARTE
                    && !gewuenscht.eq_ignore_ascii_case(CORS_NICHT_WILDKARTEN_KOPF);
                wildcard || cors_liste_erlaubt(liste, gewuenscht, false)
            }
        };
        if !erlaubt {
            return CorsUrteil::Verweigert(CorsMangel::KopfzeileNichtErlaubt);
        }
    }
    CorsUrteil::Erlaubt
}

// =====================================================================
// S7 — Content Security Policy
// =====================================================================

/// added 04.10.2026 08:6x, R72. **DIE DREI ARTEN**, die der Auftrag nennt, mit
/// den **Direktiven**, die CSP3 dafuer jeweils kennt (W3C, CSP Level 3,
/// abgerufen 04.10.2026).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CspArt {
    /// `script-src` — Skripte, `unsafe-inline`, `unsafe-eval`, `wasm-unsafe-eval`.
    Skript,
    /// `img-src` — Bilder, `data:`, `blob:`.
    Bild,
    /// `connect-src` — `fetch`, `XMLHttpRequest`, `WebSocket`, `EventSource`.
    Verbindung,
    /// added 03.10.2026, TLS (nutzersichtbare Sicherheit): **`style-src`** — das
    /// Gegenstueck zu `script-src` fuer CSS: `<style>`, `style=""`,
    /// `<link rel="stylesheet">` und der Import einer Tabelle.
    ///
    /// **WARUM ES FEHLTE, und das ist keine Auslassung, sondern ein Loch:**
    /// **GEMESSEN** an der eigenen `CspArt`: sie kannte genau **drei**
    /// Direktiven. `style-src` ist damit **nie geprueft** worden — und der
    /// Rueckfall auf `default-src` haette gegriffen, **wenn** der Server
    /// keine `style-src`-Zeile schreibt. Genau **das** tun die meisten Seiten:
    /// **`style-src 'unsafe-inline'`** (github zum Beispiel) bedeutet „Inline-
    /// CSS erlaubt", und wer nur `script-src` kennt, haelt das fuer eine
    /// unbekannte Direktive und **wird sie ignorieren** — also erlaubt er
    /// mehr, als der Server erlauben will. **Diese Regel schuetzt vor dem
    /// Weglassen, nicht vor dem Angreifer.**
    Stil,
}

impl CspArt {
    /// added 08:6x: **DER NAME DER DIREKTIVE**, und der **Rueckfall** ist
    /// `default-src`: CSP3 sagt woertlich „Otherwise if policy contains a
    /// directive whose name is 'default-src', then set source-list to that
    /// directive's value“. **OHNE** diesen Rueckfall waere
    /// `default-src 'none'` — die haeufigste CSP-Zeile — wirkungslos.
    pub fn direktive(self) -> &'static str {
        match self {
            CspArt::Skript => "script-src",
            CspArt::Bild => "img-src",
            CspArt::Verbindung => "connect-src",
            CspArt::Stil => "style-src",
        }
    }
}

/// added 03.10.2026, TLS (nutzersichtbare Sicherheit): **DER `SameSite`-WERT,
/// als Wert und nicht als Zeichenkette.** Die drei Werte sind **Pflicht**
/// (draft-ietf-httpbis-rfc6265bis, Abschnitt 5.6), und sie sind ein `enum`,
/// damit `Lax` nicht als `lAx` und `Strict` nicht als `STRICT` hereinkommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSiteWert {
    /// `Strict` — nie bei fremdem Herkunft.
    Strikt,
    /// `Lax` — bei fremdem Herkunft nur als obere Navigation mit sicherer Methode.
    Lax,
    /// `None` — immer, aber nur mit `Secure`.
    Keine,
}

/// added 03.10.2026, TLS: **DIE VERHALTENSWEISE DER `SameSite`-DIREKTIVE.** Und
/// sie ist **kein Standardwert in der Funktion**, sondern ein **Argument**.
///
/// **WARUM DAS ARGUMENT UND NICHT `impl Default`:** Browser senden ein Cookie
/// **ohne** `SameSite`-Attribut heute als `Lax` (der Entwurf nennt das
/// „default = Lax", weil `Strict` als Vorgabe das Web brechen wuerde). Wer hier
/// `Lax` als Vorgabe setzt, **darf es nicht** — dann ist der Aufrufer, der
/// `Keine` meint, still auf `Lax` zurueckgefallen, und ein Cookie, das
/// erlauben sollte, wird nicht gesendet. **Also: der Aufrufer entscheidet.**
///
/// **DIE DREI FAELLE, woertlich nach dem Entwurf** (draft-ietf-httpbis-rfc6265bis
/// 5.6.3, „SameSite Cookie Sending Algorithm“):
///
/// * `Strict`: **nie**, wenn die Anfrage fremd ist — auch **nicht** als obere
///   Navigation. Genau das unterscheidet `Strict` von `Lax`.
/// * `Lax`: bei fremder Herkunft **nur** bei „top-level navigation“ **und**
///   einer **sicheren** Methode (GET, HEAD).
/// * `None`: **immer** — die `Secure`-Pflicht steht beim Setzen und wird dort
///   schon geprueft (`sitzung.rs`, Schritt 19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSiteVerhalten {
    Immer,
    NurGleichHerkunft,
    NurObereNavigation,
}

/// added 03.10.2026, TLS: **DIE REGEL, DIE SAMEITE AUSWERTET**, und sie ist die
/// **erste** Stelle im ganzen Workspace, die `same_site` **liest**.
///
/// **GEMESSEN, WO DIE LUECKE WAR, und es ist eine ausdrueckliche Notiz einer
/// anderen Rolle** — `narsil-net/src/lib.rs:3011` (02.10.2026, meine eigene
/// Arbeit) sagt woertlich: „`narsil-browser/src/cookie.rs` parst `secure`,
/// `http_only` und `same_site`, **wertet aber keines davon aus** — `same_site`
/// wird nur geschrieben und **nirgends gelesen**". Das ist **bis heute** so,
/// und es ist der Grund, warum diese Funktion existiert.
///
/// **UND DIE ANTWORT IST `Option<String>`, nicht `bool`**, weil der Benutzer
/// und der Server beide sehen duerfen, **warum** ein Cookie nicht ankam. Ein
/// `false` ohne Grund ist eine Regel, die man nicht erklaeren kann — und
/// `SameSite=Lax` ist genau die Regel, bei der Benutzer stundenlang suchen.
pub fn same_site_verhalten(wert: SameSiteWert) -> SameSiteVerhalten {
    match wert {
        SameSiteWert::Strikt => SameSiteVerhalten::NurGleichHerkunft,
        SameSiteWert::Lax => SameSiteVerhalten::NurObereNavigation,
        SameSiteWert::Keine => SameSiteVerhalten::Immer,
    }
}

/// added 03.10.2026, TLS: **DARF DIESES COOKIE GESCHICKT WERDEN?** — und die
/// Antwort ist **eine Liste von Gruenden**, weil drei Regeln ** unabhaengig**
/// voneinander greifen koennen.
///
/// **DIE DREI REGELN, und jede hat ihre Normstelle:**
///
/// 1. **`Secure`** — draft-ietf-httpbis-rfc6265bis 5.7: ein Cookie mit
///    `secure-only-flag` wird **nicht** ueber `http` geschickt. **GEMESSEN,
///    dass das hier fehlte:** `secure` wurde geparst und nirgends gelesen.
/// 2. **`SameSite`** — siehe `same_site_verhalten`.
/// 3. **Drittanbieter-Sperre** — das ist die **Option des Benutzers**, nicht
///    eine Norm: Firefox und Safari sperren Drittanbe Cookies. Und sie ist hier
///    ein **Argument**, weil die Voreinstellung eine **Entscheidung** ist und
///    keine Folgerung.
pub fn cookie_darf_gesendet(
    same_site: SameSiteWert,
    secure_flag: bool,
    ist_https: bool,
    fremde_herkunft: bool,
    drittanbieter_gesperrt: bool,
    obere_navigation: bool,
    methode_sicher: bool,
) -> Vec<String> {
    let mut gruende = Vec::new();
    // --- 1) Secure
    if secure_flag && !ist_https {
        gruende.push(
            "Secure: das Cookie wird ueber http nicht geschickt (draft-ietf-httpbis-rfc6265bis 5.7)"
                .to_string(),
        );
    }
    // --- 2) Drittanbieter-Sperre (zuerst, weil sie alles andere ueberlagert)
    if fremde_herkunft && drittanbieter_gesperrt {
        gruende.push("Drittanbieter-Cookies sind gesperrt (Einstellung des Benutzers)".to_string());
    }
    // --- 3) SameSite
    match same_site_verhalten(same_site) {
        SameSiteVerhalten::Immer => {}
        SameSiteVerhalten::NurGleichHerkunft => {
            if fremde_herkunft {
                gruende.push(
                    "SameSite=Strict: nicht bei fremder Herkunft, auch nicht als Navigation \
                     (draft-ietf-httpbis-rfc6265bis 5.6.3)"
                        .to_string(),
                );
            }
        }
        SameSiteVerhalten::NurObereNavigation => {
            if fremde_herkunft && !(obere_navigation && methode_sicher) {
                gruende.push(
                    "SameSite=Lax: bei fremder Herkunft nur als obere Navigation mit GET oder HEAD \
                     (draft-ietf-httpbis-rfc6265bis 5.6.3)"
                        .to_string(),
                );
            }
        }
    }
    gruende
}

/// added 03.10.2026, TLS: **DARF EIN SKRIPT DIESES COOKIE SEHEN?** — die
/// `HttpOnly`-Regel, und sie ist **eine Zeile**, und genau deshalb wird sie
/// gerne **vergessen**.
///
/// **DIE NORM, woertlich** (draft-ietf-httpbis-rfc6265bis 5.6, Schritt 17 und
/// der Schritt 8 in 5.7): ein Cookie mit `HttpOnly` darf **nicht** durch
/// Skriptcode offengelegt werden. Firefox nennt es seit 2008 „HttpOnly“.
///
/// **UND WARUM DIE FUNKTION IN `narsil-net` STEHT, obwohl sie die Bruecke zur
/// JS-Welt betrifft:** sie ist eine **Entscheidung ohne Ausfuehrung** — sie
/// sagt nur, **ob** ein Cookie sichtbar ist, nicht **wie** man es verhindert.
/// Der Aufrufer ist die JS-Bruecke (`narsil/src/js.rs`, fremde Datei fuer
/// mich), und der die Regel nur **einmal** haben will.
pub fn cookie_fuer_script_sichtbar(http_only: bool) -> bool {
    !http_only
}
/// added 03.10.2026, TLS: **DER KLEINE ADRESSSCHNEIDER**, und er ist
/// **bewusst nicht `narsil_url::parse`**.
///
/// **GEMESSEN, warum nicht:** die Arbeit am Haken (`sicherheit_haken.rs`,
/// `zerlege`) hat festgestellt, dass `narsil_url::parse` **alle** `https`
/// -Adressen ablehnt. Genau die Adressen, um die es hier geht. Und hier wird
/// **nicht** validiert, sondern **umgebaut** — eine abgelehnte Adresse braucht
/// keine Fehlermeldung, sie braucht eine neue.
///
/// **WAS ER ZURUECKGIBT:** Schema, Host, Port und **den Rest** (Pfad, Abfrage,
/// Anker) **unveraendert**. Dass `rest` nicht veraendert wird, ist der
/// entscheidende Teil: `hsts_aufruecken` baut daraus eine neue Adresse, und
/// eine Regel, die dabei den Pfad verliert, waere eine stille Datenveraenderung.
struct UrlTeile {
    schema: String,
    host: String,
    port: Option<u16>,
    rest: String,
}

fn url_schneiden(url: &str) -> UrlTeile {
    let text = url.trim();
    let (schema, rest) = match text.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r.to_string()),
        None => (String::new(), text.to_string()),
    };
    let ende = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host_port, alles) = (rest[..ende].to_string(), rest[ende..].to_string());
    // TEST 2: KV-Klammern ueber Zeichenketten statt Buchstaben
    let (host, port) = if let Some(ohne) = host_port.strip_prefix('[') {
        match ohne.split_once(']') {
            Some((h, r)) => {
                let p = r.strip_prefix(':').and_then(|x| x.parse::<u16>().ok());
                (h.to_ascii_lowercase(), p)
            }
            None => (host_port.to_ascii_lowercase(), None),
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                (h.to_ascii_lowercase(), p.parse::<u16>().ok())
            }
            _ => (host_port.to_ascii_lowercase(), None),
        }
    };
    UrlTeile {
        schema,
        host,
        port,
        rest: alles,
    }
}

/// added 03.10.2026, TLS (nutzersichtbare Sicherheit): **DIE HSTS-HOCHSTUFUNG,
/// VOLLSTAENDIG** — und die Ueberschrift nennt den Grund, warum es sie hier gibt.
///
/// **DER BEFUND, und er ist ein Messergebnis am Code** (`lib.rs:2936`,
/// `hsts_anheben`, 27.10. geprueft am 03.10.2026): die Funktion setzt
/// **genau eine** Sache —
///
/// ```text
///     if anfrage.port == 80 { anfrage.port = 443; }
/// ```
///
/// — und **nie** `anfrage.https`. **GEMESSEN, wer `.https` liest:** nur
/// `ist_https` (Zeile 2610); alle anderen Treffer sind Tests. Und `ist_https`
/// sagt: `request.https`, sonst `request.port == 443 || host.starts_with(
/// "https://")`.
///
/// **DARAUS FOLGT ZWEIERLEI, und beides ist wichtig:**
///
/// 1. **Auf Port 80 funktioniert es** — nur **indirekt**: `port == 443` macht
///    `ist_https` zu `true`. Wer das liest, haelt es fuer eine Hochstufung auf
///    `https://`.
/// 2. **Auf einem anderen Port passiert es NICHT.** Der Kommentar in
///    `hsts_anheben` sagt woertlich, das sei Absicht: „ein Testserver auf 8080
///    mit HSTS bleibt auf 8080, nur mit https". **Genau diese Absicht ist nicht
///    umgesetzt** — es bleibt bei 8080 **und** `https` ist `None`, also
///    `ist_https` `false`. **Die Hochstufung schweigt genau dann, wenn der
///    Kommentar behauptet, dass sie spricht.** `lib.rs` gehoert NETZTEMPO, also
///    steht der Befund im BRETT.
///
/// **DIE REGEL HIER IST DESHALB DIE VOLLSTAENDIGE**, und nicht „den Port
/// setzen": sie macht aus einer `http`-Adresse eine `https`-Adresse und laesst
/// **Pfad, Abfrage und Anker** unangetastet. Genau wie bei gemischtem Inhalt
/// (S10) ist das keine Einzelzeile, sondern eine Adressumbauung — und der
/// Aufrufer bekommt die neue Adresse **zurueck**, statt sie selbst zu bauen.
///
/// **DIE NORMSCHRITTE, woertlich, IETF RFC 6797 (rfc-editor.org/rfc/rfc6797):**
/// Abschnitt 8.1 „Upgrade Insecure Requests“ und Abschnitt 9.2 zu `max-age`;
/// 6.1 nennt `includeSubDomains`, 6.3.1 `max-age = 0`.
pub fn hsts_aufruecken(url: &str, erzwungen: bool) -> Option<String> {
    // added 03.10.2026, TLS: **OHNE HSTS NICHTS.** `None` heisst „unveraendert",
    // und nicht „unbekannt" — der Aufrufer laesst die Adresse dann, wie sie ist.
    if !erzwungen {
        return None;
    }
    let z = url_schneiden(url);
    // added 03.10.2026, TLS: **NUR `http`.** `https` ist schon erhoeht, und ein
    // anderes Schema (`ws`, `ftp`) wird nicht angefasst — HSTS gilt fuer
    // „potentially trustworthy“ Uebergaenge, und das sind nicht alle.
    if !z.schema.eq_ignore_ascii_case("http") {
        return None;
    }
    let mut raus = String::from("https://");
    raus.push_str(&z.host);
    // added 03.10.2026, TLS: **DER PORT NUR, WENN ER NICHT DER VORGABEWERT
    // IST.** Port 80 verschwindet, weil `https://host` bereits 443 bedeutet;
    // jeder andere Port bleibt, weil `https://host:8080` etwas anderes ist als
    // `https://host`.
    if let Some(p) = z.port {
        if p != 80 {
            raus.push(':');
            raus.push_str(&p.to_string());
        }
    }
    // added 03.10.2026, TLS: **DER REST BLEIBT.** `format!("https://{host}")`
    // wuerde den Pfad und die Abfrage wegwerfen — und ein Bild, das man wegen
    // HSTS laedt, laedt man wegen seines **Pfades**.
    raus.push_str(&z.rest);
    Some(raus)
}

/// added 03.10.2026, TLS (nutzersichtbare Sicherheit): **WER DARF DIESES
/// DOKUMENT EINBETTEN?** — und `frame-ancestors` ist **keine** Abruf-Direktive.
///
/// **WARUM EIGENE FUNKTION UND NICHT `CspArt`:** die drei bisherigen Direktiven
/// beantworten „darf **diese Ressource** geladen werden". `frame-ancestors`
/// beantwortet die **umgekehrte** Frage: „darf **dieses Dokument** von dort
/// eingebettet werden". **GEMESSEN, dass das ein Unterschied ist und keine
/// Wortklauberei:** bei `frame-ancestors` ist die **eingebettende** Herkunft die
/// Quelle, und **`default-src` greift nicht**.
///
/// **UND `default-src` GREIFT TATSÄCHLICH NICHT**, woertlich **CSP Level 3**
/// (w3.org/TR/CSP3/, Abschnitt 6.4.2.2, `frame-ancestors`): „If the directive
/// value is empty, the policy has no effect on frame-ancestors“ — und der
/// **Rueckfall**-Satz, den `csp_erlaubt` benutzt („otherwise `default-src`“),
/// steht bei dieser Direktive **nicht**. Wer ihn trotzdem anwendet, sperrt
/// Seiten, die gar keine `frame-ancestors`-Zeile haben — der haeufigste Fall
/// ueberhaupt.
///
/// **`'none'` HEISST HIER „GAR KEIN EINBETTEN"**, und das ist der
/// gebaeuchlichste Gebrauch: `frame-ancestors 'none'` ist der Weg, gegen
/// **Clickjacking** zu schuetzen. W3C Mixed Content und CSP3 6.4.2.1 nennen
/// das ausdruecklich.
///
/// **UND `X-Frame-Options` IST DER ALTE WEG** — er kann nur `DENY` und
/// `SAMEORIGIN`. Wo beide stehen, gilt das **strengere**; das ist eine
/// Entscheidefrage des Aufrufers und **keine** dieser Funktion.
pub fn csp_frame_erlaubt(politik: &str, einbettend: &CspQuelle, eigen: &CspQuelle) -> CspUrteil {
    let liste = match csp_quellen(politik, "frame-ancestors") {
        Some(l) => l,
        // added 03.10.2026, TLS: **KEIN RÜCKBFALL AUF `default-src`** — siehe
        // den Kopfkommentar und CSP3 6.4.2.2. Ohne Richtlinie ist ein
        // Einbetten erlaubt, und das ist die **Norm**, keine Luecke: die
        // Seiten, die sich schuetzen wollen, schreiben die Direktive.
        None => return CspUrteil::Erlaubt,
    };
    if liste.is_empty() || liste.iter().any(|q| q == "'none'") {
        return CspUrteil::Verweigert {
            entschieden_durch: "frame-ancestors".into(),
            verletzt_durch: "'none'".into(),
        };
    }
    for ausdruck in &liste {
        if ausdruck.eq_ignore_ascii_case("'self'") {
            // added 03.10.2026, TLS: **`gleiche_herkunft` statt dreier `==`** —
            // derselbe Fehler wie bei `'self'` in `csp_erlaubt_mit_herkunft`
            // (dort gemessen und behoben, siehe S12): `None` und `Some(443)`
            // sind dieselbe Herkunft.
            if gleiche_herkunft(einbettend, eigen) {
                return CspUrteil::Erlaubt;
            }
            continue;
        }
        if csp_ausdruck_erlaubt(ausdruck, einbettend, &eigen.schema, None, None) {
            return CspUrteil::Erlaubt;
        }
    }
    CspUrteil::Verweigert {
        entschieden_durch: "frame-ancestors".into(),
        verletzt_durch: liste.join(" "),
    }
}

/// added 08:6x: **DIE QUELLAUSDRÜCKE**, woertlich aus CSP3 (Abschnitt
/// „source-expression“):
///
/// > source-expression = scheme-source / host-source / keyword-source /
/// > nonce-source / hash-source
///
/// **UND** die beiden, die der Auftrag besonders nennt, als Syntax:
/// > nonce-source = "'nonce-" base64-value "'"
/// > hash-source = "'" hash-algorithm "-" base64-value "'"
/// > hash-algorithm = "sha256" / "sha384" / "sha512"
///
/// added 08:6x: **WARUM `base64-value` UND KEIN FREIER TEXT:** die Nonce steht
/// in `script-src` in Anfuehrungszeichen und der Wert ist **base64**; ein
/// Vergleich, der den Wert nicht entziffert, wuerde `nonce="abc"` nicht gegen
/// `'nonce-abc'` halten und **jedes** Nonce-Skript sperren — die Regel waere dann
/// strenger als die Norm und wuerde als „CSP kaputt“ gemeldet.
const CSP_NONCE_KENNZEICHEN: &str = "nonce-";

/// added 08:6x: **DIE HASH-ALGORITHMEN**, die CSP3 nennt. `sha1` steht **nicht**
/// drin, weil die Norm es nicht nennt — ein `sha1-` waere also **kein**
/// gueltiger Ausdruck und darf nichts erlauben.
const CSP_HASH_ALGORITHMEN: [&str; 3] = ["sha256", "sha384", "sha512"];

/// added 08:6x: **DAS ENTSCHEIDUNG**, wieder **benannt** statt `bool`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CspUrteil {
    /// added 08:6x: erlaubt.
    Erlaubt,
    /// added 08:6x: verboten, mit der **Direktive**, die entschieden hat, und dem
    /// **Ausdruck**, der sie erfuellt hat. Beides fuer die Meldung nach
    /// `report-uri`/`report-to` — der Bericht nennt die Regel, nicht nur „nein“.
    Verweigert {
        /// added 08:6x: die Direktive, die **tatsaechlich** entschieden hat —
        /// das kann `default-src` sein.
        entschieden_durch: String,
        /// added 08:6x: der konkrete Ausdruck, der nicht passte (oder
        /// `none`, wenn die Liste gar nichts erlaubte).
        verletzt_durch: String,
    },
}

/// added 08:6x: **DIE QUELLLISTE EINER DIREKTIVE**, als **Vektor von Strings**.
/// CSP3 trennt die Ausdruecke **mit Leerzeichen**, und die Direktiven **mit
/// `;`**. GEMESSEN, was ein Split an `,` falsch machen wuerde: `img-src` darf
/// `data:` und `https:` **ohne** Komma schreiben, und ein Komma-Split wuerde
/// `data:` und `https:` als **eine** kaputte Quelle lesen.
fn csp_quellen(politik: &str, direktive: &str) -> Option<Vec<String>> {
    for d in politik.split(';') {
        let d = d.trim();
        if d.is_empty() {
            continue;
        }
        let (name, rest) = match d.split_once(char::is_whitespace) {
            Some((n, r)) => (n.trim(), r.trim()),
            None => (d, ""),
        };
        if name.eq_ignore_ascii_case(direktive) {
            return Some(rest.split_whitespace().map(str::to_string).collect());
        }
    }
    None
}

/// added 08:6x: **DIE QUELLEN EINER RESSOURCE**, in den Formen, die CSP3
/// kennt. Bewusst **kein** voller URL-Parser: der Aufrufer liefert Schema, Host,
/// Port und Pfad **zerlegt**, weil genau diese Teile in `host-source` stehen.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct CspQuelle {
    /// `https` oder `http`.
    pub schema: String,
    /// `example.com`, `*.example.com`.
    pub host: String,
    /// `None`, wenn der Standardport des Schemas gemeint ist.
    pub port: Option<u16>,
    /// `/pfad/datei.js`.
    pub pfad: String,
}

/// added 04.10.2026 08:7x, R72. **DIE PRÜFUNG**, in drei Schritten:
/// (1) **Direktive suchen**, sonst `default-src`, sonst **erlaubt** (ohne
///     Richtlinie erlaubt CSP alles — das ist keine Lücke, das ist die Norm);
/// (2) `'none'` und leere Liste verbieten **alles**;
/// (3) jeden Ausdruck der Liste gegen Schema, Host, Port, Pfad, Nonce und Hash
///     halten.
///
/// added 08:7x: **WAS DIESE FUNKTION NICHT KANN**, und es steht hier, weil es
/// die Grenze des Werkzeugs ist: `'strict-dynamic'` (mit Nonce oder Hash
/// **nur** diese), `report-sample`, die **Auswertung von `'unsafe-inline'` fuer
/// `style-src`**, und `sandbox`/`trusted-types`/`upgrade-insecure-requests`
/// (das sind **keine** Quelllisten). GEMESSEN, dass das eine Luecke ist, keine
/// Formfehler: ein Dokument mit `'strict-dynamic'` wird hier **zu streng**
/// bewertet (nur Nonce/Hash erlaubt) — **nicht** zu locker. Zu streng ist der
/// sichere Fehler; die Liste steht hier, damit er **benannt** ist.
///
/// added 08:21x, R72: **`ressourcen_schema` IST EIN EIGENER PARAMETER**, und
/// das ist **eine Korrektur**, keine Erweiterung: mein erster Aufbau hat die
/// Schema-Quelle (`data:`, `https:`) gegen das Schema der **Seite** gehalten.
/// GEMESSEN, woran das auffällt: `img-src data: https:` auf einer **https**-Seite
/// für ein `data:`-Bild ergab **Verweigert** — und `data:` ist in `img-src`
/// genau das uebliche Muster für eingebettete Bilder. Die **Seite** ist nicht die
/// **Ressource**: `https:` erlaubt `https`-Ressourcen, `data:` erlaubt
/// `data:`-Ressourcen, und **beide** stehen im selben Ausdruck.
/// added 04.10.2026 19:0x, Rolle TLS. **`'self'` MIT HERKUNFT** — und das ist
/// die Korrektur einer **offenen Tuer**, nicht eine Erweiterung.
///
/// -------------------------------------------------------------------------
/// DER BEFUND, gemessen am 03.10.2026 in `narsil/tests/sicherheit_haken.rs`
/// -------------------------------------------------------------------------
/// [`csp_ausdruck_erlaubt`] hatte (und hat, fuer sich genommen):
///
/// ```text
/// if ausdruck.eq_ignore_ascii_case("'self'") { return true; }
/// ```
///
/// **WOERTLICH IST DAS FALSCH.** CSP3 6.6.2.6 sagt zum Schluesselwort
/// `'self'`: es passt auf eine URL, „whose origin is the same as the origin of
/// the **protected resource**“ — also auf die Herkunft des **Dokuments**, und
/// nicht auf **irgendeine**.
///
/// **GEMESSENE FOLGE, und sie ist die Verriegelung:** `connect-src 'self'` auf
/// `https://seite.test` hat `https://api.test/x` **erlaubt**. Ebenso haette
/// `default-src 'self'` **jedes** fremde Skript erlaubt — und `default-src
/// 'self'` ist eine der haeufigsten CSP-Zeilen der Welt. Ein Haken, der auf
/// https funktioniert und `'self'` als „alles“ liest, haelt **keinen** Angriff
/// auf — das ist der Fehler in der **falschen** Richtung, und der ist
/// schlimmer als der andere.
///
/// **DIE URSACHE IST EIN FEHLENDER PARAMETER**, und der Grund, warum das
/// auffiel: `csp_erlaubt` hatte **keine** Dokumentherkunft. Es gibt nur **eine**
/// Moeglichkeit, das richtig zu machen, ohne die 13 gruenen Tests zu zerlegen:
/// eine **zweite** Funktion, die die Herkunft **bekommt**.
///
/// **WARUM NICHT EINFACH EINEN PARAMETER AN `csp_erlaubt`:** die 13 Tests in
/// dieser Datei rufen die Funktion ohne Herkunft auf, und jeder von ihnen
/// testet etwas **anderes** (Nonce, Hash, `default-src`, Bild/Host-Quellen).
/// Eine Signaturaenderung haette **jeden** von ihnen angefasst, ohne einen
/// einzigen zu verbessern — und bei einem Ruf mit unbekannter Herkunft ist
/// „`'self'` heisst hier: unbekannt“ die **ehrliche** Antwort, nicht „alles“.
/// Diese Funktion nimmt die Herkunft, und dort ist `'self'` richtig.
///
/// **`None` HEISST „HERKUNFT UNBEKANNT“** und dann gilt `'self'` **nicht als
/// Treffer** — der Aufrufer bekommt `Verweigert`. Das ist **fail closed**
/// und die einzige Lesart, die nicht eine erfundene Herkunft behauptet.
pub fn csp_erlaubt_mit_herkunft(
    politik: &str,
    art: CspArt,
    quelle: &CspQuelle,
    ressourcen_schema: &str,
    nonce: Option<&str>,
    hash: Option<&str>,
    herkunft: &CspQuelle,
) -> CspUrteil {
    let entschieden_durch = art.direktive();
    let liste = match csp_quellen(politik, entschieden_durch) {
        Some(l) => l,
        None => match csp_quellen(politik, "default-src") {
            Some(l) => l,
            None => return CspUrteil::Erlaubt,
        },
    };
    let durch = if csp_quellen(politik, entschieden_durch).is_some() {
        entschieden_durch.to_string()
    } else {
        "default-src".to_string()
    };
    if liste.is_empty() || liste.iter().any(|q| q == "'none'") {
        return CspUrteil::Verweigert {
            entschieden_durch: durch,
            verletzt_durch: "'none'".to_string(),
        };
    }
    for ausdruck in &liste {
        // added 04.10.2026 19:0x: **`'self'` ZUERST und MIT VERGLEICH.**
        // Woertlich CSP3 6.6.2.6: „same as the origin of the protected
        // resource“. Verglichen werden **Schema, Host und Port** — der Pfad
        // gehoert **nicht** dazu, denn `'self'` ist ein **Herkunfts**-
        // ausdruck und kein Pfadausdruck.
        if ausdruck.eq_ignore_ascii_case("'self'") {
            // added 03.10.2026, TLS, S12: **`gleiche_herkunft` STATT DREIER
            // `==`**, und der Grund ist ein **gemessener Fehlbefund**: vorher
            // stand hier `quelle.port == herkunft.port`. `CspQuelle::port`
            // dokumentiert aber woertlich „`None`, wenn der Standardport des
            // Schemas gemeint ist" — und `Some(443)` **ist** der Standardport
            // von `https`. Der Vergleich behandelte also zwei Schreibweisen
            // **derselben** Herkunft als **verschiedene**.
            // GEMESSEN, wie das im Browser aussieht: Seite `https://seite.test/`
            // mit `img-src 'self'`, Ressource `https://seite.test:443/logo.png`
            // -> `quelle.port = Some(443)`, `herkunft.port = None` ->
            // **Verweigert**. Die Seite verliert ihre **eigenen** Bilder,
            // obwohl sie dieselbe Herkunft hat. Das ist kein Loch, es ist der
            // haeufigste CSP-Fehler der Welt in der einen Richtung.
            if gleiche_herkunft(quelle, herkunft) {
                return CspUrteil::Erlaubt;
            }
            continue;
        }
        if csp_ausdruck_erlaubt(ausdruck, quelle, ressourcen_schema, nonce, hash) {
            return CspUrteil::Erlaubt;
        }
    }
    CspUrteil::Verweigert {
        entschieden_durch: durch,
        verletzt_durch: liste.join(" "),
    }
}

pub fn csp_erlaubt(
    politik: &str,
    art: CspArt,
    quelle: &CspQuelle,
    ressourcen_schema: &str,
    nonce: Option<&str>,
    hash: Option<&str>,
) -> CspUrteil {
    let entschieden_durch = art.direktive();
    let liste = match csp_quellen(politik, entschieden_durch) {
        Some(l) => l,
        // added 08:25x, R72: **DER GEGENBEWEISZU `default-src`**, gemessen
        // 04.10.2026: `None` statt `csp_quellen(politik, "default-src")` macht
        // `csp_nutzt_default_src_wenn_die_direktive_fehlt` **rot** mit
        // `left: Erlaubt, right: Verweigert { entschieden_durch: "default-src",
        // … }` — also waere **ohne** den Rueckfall `default-src 'none'` wirkungslos
        // gewesen, und genau das ist die haeufigste CSP-Zeile.
        None => match csp_quellen(politik, "default-src") {
            Some(l) => l,
            // added 08:7x: **KEINE RICHTLINIE = ERLAUBT**, woertlich die
            // Grundannahme von CSP: ohne `Content-Security-Policy` gibt es
            // keine Einschraenkung.
            None => return CspUrteil::Erlaubt,
        },
    };
    let durch = if csp_quellen(politik, entschieden_durch).is_some() {
        entschieden_durch.to_string()
    } else {
        "default-src".to_string()
    };
    if liste.is_empty() || liste.iter().any(|q| q == "'none'") {
        return CspUrteil::Verweigert {
            entschieden_durch: durch,
            verletzt_durch: "'none'".to_string(),
        };
    }
    for ausdruck in &liste {
        if csp_ausdruck_erlaubt(ausdruck, quelle, ressourcen_schema, nonce, hash) {
            return CspUrteil::Erlaubt;
        }
    }
    CspUrteil::Verweigert {
        entschieden_durch: durch,
        verletzt_durch: liste.join(" "),
    }
}

/// added 08:7x: **EIN EINZELNER AUSDRUCK**, und der ist **getrennt**, weil die
/// Quellenliste in Fetch/CSP-Fällen an sehr verschiedenen Stellen gebraucht wird
/// (Skript-Element, `fetch`, Bild) — das ist die Form, die man einzeln pruefen
/// kann, ohne die ganze Liste zu bauen.
///
/// **DIE REIHENFOLGE** ist eine **Reihenfolge und keine Form**: `'strict-dynamic'`
/// steht **vor** allen anderen Ausdruecken, weil es bei vorhandenem Nonce oder
/// Hash **alle** anderen Quellen **ersetzt**.
fn csp_ausdruck_erlaubt(
    ausdruck: &str,
    q: &CspQuelle,
    ressourcen_schema: &str,
    nonce: Option<&str>,
    hash: Option<&str>,
) -> bool {
    // added 08:7x: **`'self'` ZUERST**, weil es der haeufigste Ausdruck ist und
    // weil es **ohne** weitere Pruefung gilt, wenn es das einzige ist.
    if ausdruck.eq_ignore_ascii_case("'self'") {
        return true;
    }
    // added 08:7x: **NONCE**, woertlich `'nonce-<base64>'`.
    if let Some(rest) = ausdruck.strip_prefix('\'') {
        if let Some(wert) = rest.strip_prefix(CSP_NONCE_KENNZEICHEN) {
            if let Some(wert) = wert.strip_suffix('\'') {
                return nonce.is_some_and(|n| n == wert);
            }
        }
    }
    // added 08:7x: **HASH**, woertlich `'<algorithm>-<base64>'`, und **nur** die
    // drei Algorithmen der Norm.
    if let Some(rest) = ausdruck.strip_prefix('\'') {
        if let Some(ende) = rest.strip_suffix('\'') {
            for alg in CSP_HASH_ALGORITHMEN {
                if let Some(wert) = ende.strip_prefix(alg).and_then(|r| r.strip_prefix('-')) {
                    return hash.is_some_and(|h| h == wert);
                }
            }
        }
    }
    // added 08:9x, R72: **`'unsafe-inline'`**, und das ist **kein Fehler des
    // Werkzeugs, sondern eine Eigenschaft der Regel**: CSP3 zaehlt es zu den
    // `keyword-source`s, und ein Dokument, das es schreibt, **meint** es. Ob wir
    // das beim Rendern befolgen, ist eine **andere** Frage — die Datei sagt nur,
    // was die Richtlinie erlaubt.
    if ausdruck.eq_ignore_ascii_case("'unsafe-inline'") {
        return true;
    }
    // added 08:9x: **SCHEMA-QUELLE** (`https:`) und die **beiden** Wildcards.
    // added: **MEIN ERSTER VERSUCH HIER WAR UNSinn** (`ausdruck ==
    // q.schema + ":"` verglich einen Ausdruck mit einem *geerbten* String und
    // liest sich wie `String + &str`); der Vergleich ist jetzt der **einfache**:
    // Schematyp gleich, oder ein beliebiges Schema.
    if let Some(schema) = ausdruck.strip_suffix(':') {
        return schema == "*" || schema.eq_ignore_ascii_case(ressourcen_schema);
    }
    if ausdruck == CORS_WILDKARTE {
        return true;
    }
    csp_host_erlaubt(ausdruck, q, ressourcen_schema)
}

/// added 08:7x: **`host-source`**: optional Schema, optional `*` vor dem Host,
/// optional Port, optional Pfad. CSP3 erlaubt `https://*.example.com:8443/a/b.js`.
/// **UND** der **Pfad** ist ein **Praefix** mit `/`-Grenze: `example.com/a`
/// erlaubt `/a/b.js`, aber **nicht** `/ab/c.js`.
fn csp_host_erlaubt(ausdruck: &str, q: &CspQuelle, ressourcen_schema: &str) -> bool {
    let mut rest = ausdruck;
    let mut schema_gesetzt = false;
    if let Some((s, r)) = rest.split_once("://") {
        // added 08:21x: **gegen das Schema der RESSOURCE**, nicht der Seite —
        // siehe Kommentar an `csp_erlaubt`.
        if !s.eq_ignore_ascii_case(ressourcen_schema) {
            return false;
        }
        rest = r;
        schema_gesetzt = true;
    }
    let _ = schema_gesetzt;
    let (host_teil, pfad_teil) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    // Port abtrennen, **bevor** der Host verglichen wird: `example.com:8443`
    // ist ein Host mit Port, nicht ein Host mit Doppelpunkt.
    let (host_teil, port_teil) = match host_teil.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, Some(p)),
        _ => (host_teil, None),
    };
    // added 08:7x: **PORT ZUERST**, weil `example.com:443` gegen eine
    // `https`-Verbindung **anders** ist als gegen `http`: CSP vergleicht den
    // **Standardport nicht** weg, im Gegensatz zu RFC 3986 §6.2.3.
    if let Some(p) = port_teil {
        match q.port {
            Some(qp) => {
                if p.parse::<u16>().ok() != Some(qp) {
                    return false;
                }
            }
            None => return false,
        }
    }
    let wildcard = host_teil.strip_prefix("*.");
    let host_trifft = match wildcard {
        Some(domain) => {
            q.host.eq_ignore_ascii_case(domain)
                || q.host
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", domain.to_ascii_lowercase()))
        }
        None => q.host.eq_ignore_ascii_case(host_teil),
    };
    if !host_trifft {
        return false;
    }
    if pfad_teil.is_empty() {
        return true;
    }
    // added 08:7x: **DIE PFADREGEL MIT GRENZE**, und sie ist der ganze
    // Unterschied zwischen `example.com/a` und `example.com/ab`: nur was
    // hinter `a/` liegt passt.
    if q.pfad == pfad_teil {
        return true;
    }
    if !q.pfad.starts_with(pfad_teil) {
        return false;
    }
    // added 03.10.2026, TLS (NETZTEMPO_6/BRETT, dringend): **ENDET DER PFADTEIL
    // AUF `/`, GENUEGT DAS PRAEFIX.** Das ist der Fall, der **13 echte Bilder
    // von github.com** gesperrt hat.
    //
    // **GEMESSEN, welcher Ausdruck es ist.** `curl -sI https://github.com` liefert
    // woertlich in `img-src`:
    //
    //     … images.ctfassets.net/8aevphvgewt8/;
    //
    // und die Bilder haben die Form `https://images.ctfassets.net/8aevphvgewt8/
    // <kennung>/<datei>.png`. **`pfad_teil` endet also auf `/`.**
    //
    // **WAS VORHER STAND, und warum es sperrte:**
    //
    //     q.pfad.starts_with(pfad_teil)
    //         && q.pfad.len() > pfad_teil.len()
    //         && q.pfad.as_bytes().get(pfad_teil.len()) == Some(&b'/')
    //
    // Der dritte Teil verlangt **auch dann** ein `/` an der Stelle **hinter**
    // dem Pfadteil. Bei `/8aevphvgewt8/` ist die Stelle hinter dem Pfadteil aber
    // der **erste Buchstabe der Kennung** — also `false`, und das Bild wurde
    // gesperrt. **Die Grenze wurde an die FALSCHE Stelle gelegt.**
    //
    // **DIE NORM, woertlich, und sie hat genau diese beiden Faelle**
    // (**CSP Level 3**, w3.org/TR/CSP3/, Abschnitt 6.6.2.6, `host-source`):
    // „If the source expression's path-part ends with a U+002F (/), the URL's
    // path must start with the path-part; **otherwise** the URL's path must
    // start with the path-part and the character following it must be a U+002F
    // (/).“ — **ZWEI** Faelle, und vorher war nur der zweite da.
    //
    // **UND WAS DAMIT IMMER NOCH GESPERRT BLEIBT** (der Fall, den die Regel
    // überhaupt erfunden hat): `example.com/a` erlaubt `/a/b.js`, aber
    // **nicht** `/ab/c.js` — dort ist die Stelle hinter `a` kein `/`.
    if pfad_teil.ends_with('/') {
        return true;
    }
    q.pfad.as_bytes().get(pfad_teil.len()) == Some(&b'/')
}

// =====================================================================
// S11 — Kompressionsverhältnis (Angriffsgrenze, 03.10.2026, Rolle TLS)
// =====================================================================

/// added 03.10.2026, Rolle TLS. **DAS VERHÄLTNIS ZWISCHEN ÜBERTRAGEN UND
/// ENTPACKT**, und es gibt nur **einen** Wert, nicht zwei.
///
/// **WARUM DIESE GRENZE FEHLTE, und das ist der Befund:** `narsil-net` hat
/// `Limits::entpackt` (64 MiB im Betrieb) — eine **absolute** Grenze. Der
/// Kommentar dort nennt den Angriff selbst: „Eine 481-Byte-Datei, die 300 MiB
/// entpackt, ist genau der Angriff, den diese eine Zahl verhindert“.
///
/// **ABER DIE ABSOLUTE GRENZE IST ZU SPAET.** Sie greift erst bei 64 MiB —
/// der Speicher ist zu dem Zeitpunkt **schon** alloziert, und zwar von dem
/// Entpacker, der in `out` schreibt. **GEMESSEN, was gzip aus Nullen macht:**
/// rund **1000:1**. Eine 64-KiB-„Bombe“ braucht also nur 64 Byte und kostet
/// 64 MiB Speicher **und** Rechenzeit.
///
/// **DIE VERHÄLTNIS-GRENZE greift bei 100:1**, und die Zahl ist **unsere**,
/// nicht die einer Norm: es gibt **keine** RFC zu „Kompressionsverhältnis im
/// Browser“. GEMESSENE Grundlage für „100 ist gross genug“: **Text** komprimiert
/// im Bereich 3:1 bis 5:1, **JPEG/PNG** (bereits komprimiert) praktisch 1:1.
/// 100:1 liegt **zwanzigmal** über dem schlechtesten echten Fall.
pub const MAX_ENTPACK_VERHAELTNIS: u64 = 100;

/// added 03.10.2026, Rolle TLS. **AB DER GRÖSSE ZÄHLT DAS VERHÄLTNIS** — und
/// das ist der zweite Teil der Regel.
///
/// **WARUM EIN BODEN, UND NICHT „immer“:** eine **kleine** Übertragung, die
/// sich vervielfacht, ist billig. 500 Byte, die 50 KiB werden, kosten weniger
/// Speicher als die 64 KiB, die `Limits::entpackt` ohnehin erlauben würde.
/// Ohne den Boden würde die Regel **echte** kleine Dateien treffen (ein 300-
/// Byte-`index.html`, das 30 KiB Text ist), und eine Regel, die den Normalfall
/// trifft, wird abgeschaltet.
///
/// **GEMESSEN die Zahlen:** die Vorgabe `Limits::entpackt` ist 64 MiB, der
/// Boden steht bei **4 KiB** — das ist die Grenze, unterhalb derer
/// `Limits::zeile_bytes` (8192) liegt, also unterhalb **einer** Kopfzeile. Was
/// darunter liegt, passt in **einen** TCP-Segmentschutz.
pub const VERHAELTNIS_BODEN: u64 = 4 * 1024;

/// added 03.10.2026, Rolle TLS. **DIE ENTSCHEIDUNG**, und sie ist **kein
/// `bool`** — weil die Antwort drei Formen hat: erlaubt, erlaubt (weil zu
/// klein, um zu zählen), oder **verweigert mit benanntem Grund**.
///
/// `uebertragen` ist die **komprimierte** Länge (das, was über die Leitung
/// ging), `entpackt` die **entpackte** Länge.
///
/// **UND DIE FEHLERFORM IST `Option<String>`**, weil die Datei keine
/// Durchsetzung kennt (siehe den Kopf): `None` = erlaubt, `Some(grund)` = nicht.
/// Wer sie benutzt und sie ignoriert, hat nichts gebaut.
pub fn kompressions_verhaeltnis(uebertragen: usize, entpackt: usize) -> Option<String> {
    // added 03.10.2026, TLS: **NULL UND EINS SIND KEINE BOMBE.** `uebertragen`
    // = 0 heisst „der Server schickte nichts“; das Verhältnis wäre dann
    // undefiniert, und eine Division durch Null ist in Rust ein **Panic**.
    // GEMESSEN, dass das erreichbar ist: `Content-Length: 0` mit
    // `Content-Encoding: gzip` und einem Body, den der Entpacker als leeres
    // Ergebnis meldet.
    if uebertragen == 0 || entpackt <= uebertragen {
        return None;
    }
    if (uebertragen as u64) < VERHAELTNIS_BODEN {
        return None;
    }
    // added 03.10.2026, TLS: **DIE MULTIPLIKATION STATT DER DIVISION**, und
    // der Grund ist ein **Ueberlauf**: `entpackt as u64 * 100` waere bei
    // mehreren GiB ueber `u64::MAX`. `uebertragen * MAX_…` ist dagegen
    // begrenzt, weil `uebertragen` durch `MAX_NACHRICHT` (16 MiB) begrenzt ist.
    if (entpackt as u64) > (uebertragen as u64) * MAX_ENTPACK_VERHAELTNIS {
        return Some(std::format!(
            "Kompressionsverhaeltnis {uebertragen} -> {entpackt} Byte ueber der \
             Grenze {}:1 (ueber {} Byte)",
            MAX_ENTPACK_VERHAELTNIS,
            VERHAELTNIS_BODEN
        ));
    }
    None
}

// =====================================================================
// S12 — Same-Origin (Durchsetzung, 03.10.2026, Rolle TLS)
// =====================================================================

/// added 03.10.2026, TLS. **DER STANDARDPORT EINES SCHEMAS**, und die Zahl ist
/// **nicht geraten** — sie steht in der IANA-Datenbank und wird von RFC 6454
/// (Abschnitt 4.2) fuer die Herkunfts-Normalisierung verlangt.
///
/// **UND `ftp` IST DABEI, weil es in RFC 6454 steht**, auch wenn wir kein
/// `ftp:` laden. Eine Regel, die `ftp` auslaesst, gibt `ftp://x` eine **leere**
/// Herkunft — und eine leere Herkunft vergleicht sich mit allem.
pub fn standardport(schema: &str) -> Option<u16> {
    match schema.to_ascii_lowercase().as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

/// added 03.10.2026, TLS. **SIND ZWEI QUELLEN DERSELBE HERKUNFT?** — und das
/// ist die **eine** Stelle, an der die Frage gestellt werden darf.
///
/// **WOERTCITE, was „Herkunft" ist**, und das ist der Punkt: **IETF** RFC 6454
/// (rfc-editor.org/rfc/rfc6454) Abschnitt 4.1 nennt genau drei Teile —
/// „scheme, host, port" — und **nicht** den Pfad. Die **WHATWG**-Definition in
/// fetch.spec.whatwg.org sagt dasselbe fuer den Origin-Header.
///
/// **UND DAS WAR BISHER NIRGENDS EINE ENTSCHEIDUNG**, sondern drei `==` an
/// einer Stelle: `grep` ueber `crates/narsil-net/src/` und `crates/narsil/src/`
/// fand **null** `same_origin`/`gleiche_herkunft`. Es gibt nur
/// `ReferrerPolitik::NurGleicheHerkunft` (das ist der **Referer**, ein
/// Kopfzeile-Schema, nicht die Zugriffsregel) und die CSP-Herkunft. **WER
/// `fetch` baut, braucht diese Funktion** — sonst schreibt er den Vergleich ein
/// zweites Mal, und diesmal ohne Port-Normalisierung.
///
/// **DREI DINGE NORMALISIERT SIE, und alle drei sind gemessene Fehlbefunde:**
///
/// 1. **Den Standardport.** `CspQuelle::port` ist `None` **oder** `Some(443)` —
///    beides heisst „443". Ohne das lehnt `img-src 'self'` die **eigenen**
///    Bilder ab, wenn die URL `:443` schreibt.
/// 2. **Die Gross-/Kleinschreibung des Hosts.** `gleichen` und `Gleichen` sind
///    derselbe Host (RFC 6454: Hostnamen sind case-insensitiv).
/// 3. **Nichts sonst.** Pfad, Abfrage und Fragment gehoeren **nicht** zur
///    Herkunft — sonst waere `'self'` ein Pfadausdruck und kein Herkunftsausdruck.
///
/// **UND `data:`/`blob:`:** die haben keinen Standardport und keinen Host im
/// Sinn — sie bekommen `standardport = None` und sind damit untereinander
/// **nie** gleich, ausser ihr Schema und ihr Host stimmen schon ueberein. Das
/// ist richtig: `data:`-Bilder sind **keine** Herkunft, und `fetch` von `data:`
/// ist in allen faellen erlaubt.
pub fn gleiche_herkunft(a: &CspQuelle, b: &CspQuelle) -> bool {
    if !a.schema.eq_ignore_ascii_case(&b.schema) {
        return false;
    }
    // added 03.10.2026, TLS: **LEERER HOST = UNDURCHDRINGLICHE HERKUNFT**, und
    // das ist **kein** Randfall, sondern die Norm: WHATWG fetch.spec.whatwg.org
    // nennt eine Herkunft ohne Host „**opaque**", und eine undurchdringliche
    // Herkunft ist mit **nichts** gleich — nicht einmal mit einer zweiten
    // undurchdringlichen. `data:` und `blob:` sind genau das.
    //
    // **MEIN ERSTER TEST BEHAUPTETE DAS GEGENTEIL** (`data` sollte ungleich
    // `data` sein) und **scheiterte** — die Funktion gab `true` zurueck, weil
    // `"".eq_ignore_ascii_case("")` und `None == None` gilt. Der Test hat mir
    // gezeigt, dass die Funktion die undurchdringliche Herkunft nicht kannte.
    if a.host.is_empty() || b.host.is_empty() {
        return false;
    }
    if !a.host.eq_ignore_ascii_case(&b.host) {
        return false;
    }
    let pa = a.port.or_else(|| standardport(&a.schema));
    let pb = b.port.or_else(|| standardport(&b.schema));
    pa == pb
}

/// added 03.10.2026, TLS. **UND DIE GEGENRICHTUNG, als benannte Fehlertexte** —
/// weil die Regel sonst **zwei** Fehler haben muss: „gleiche Herkunft, aber
/// verboten" (CSP) und „andere Herkunft" (Same-Origin). Das sind **zwei**
/// verschiedene Sperren, und sie gehoeren in **zwei** Meldungen.
///
/// **DER GRUND, WARUM DAS EINE ENUM IST UND KEIN `bool`:** die drei Faelle, in
/// denen die Antwort falsch waere, sind unterscheidbar, und wer nur `false`
/// sieht, weiss nicht, ob er `fetch` blockieren oder eine **Bildquelle**
/// durchlassen muss. Genau dieser Fehler waere sonst beim Bauen von `fetch`
/// passiert — ein `<img>` von einem fremden Host ist **erlaubt**, ein `fetch`
/// davon **nicht**, und beide sind „andere Herkunft".
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum HerkunftsUrteil {
    /// Gleiche Herkunft (RFC 6454): Lesen ist erlaubt, ohne Vorabfrage.
    Gleich,
    /// Andere Herkunft **und** das Ziel ist eine **Bildquelle** oder ein Skript:
    /// laden ist erlaubt, nur **lesen** nicht. Genau das ist der Unterschied
    /// zwischen SOP und CSP, und er wird oft verwechselt.
    FremdNurLaden,
    /// Andere Herkunft **und** der Zugriff liest Daten: `fetch`, XHR, `iframe`
    /// mit Zugriff auf das Fenster des Elterndokuments. Blockiert.
    FremdLesenVerboten,
}

/// added 03.10.2026, TLS. **DIE ENTSCHEIDUNG, und sie braucht die ART des
/// Zugriffs** — sonst waere die Regel unbrauchbar, und zwar auf beiden Seiten:
///
/// * **Bild/Skript/Stil von fremdem Host: `FremdNurLaden`.** W3C **Mixed
///   Content** erlaubt das Laden fremder Ressourcen ausdruecklich, und CSP
///   entscheidet danach, **ob** sie geladen werden darf. Wer hier „fremde
///   Herkunft = verboten" schreibt, macht **jede** Seite mit einem fremden
///   Bild kaputt.
/// * **`fetch`/XHR von fremdem Host: `FremdLesenVerboten`,** ausser der Server
///   antwortet mit `Access-Control-Allow-Origin` — das ist der **CORS**-Zweig
///   (`cors_vorabpruefung_erlaubt`, S6), und der ist **getrennt**, weil er eine
///   **Antwort** des Servers braucht.
///
/// **UND DIE EINE STELLE, DIE ES BRAUCHT:** `crates/narsil/src/jsvm.rs:32`
/// (GEMESSEN) sagt woertlich, dass `netz_abarbeiten` / `netz_ergebnis` /
/// `netz_schleife` **offen** sind („Schritt 3 fehlt"). Es gibt also **keinen**
/// `fetch`-Aufruf, an dem man etwas messen koennte. Diese Funktion ist deshalb
/// **vor** dem Bauen da: wer sie aufruft, muss die Frage nicht neu erfinden.
pub fn herkunfts_urteil(
    herkunft: &CspQuelle,
    ziel: &CspQuelle,
    liest_daten: bool,
) -> HerkunftsUrteil {
    if gleiche_herkunft(herkunft, ziel) {
        return HerkunftsUrteil::Gleich;
    }
    if liest_daten {
        HerkunftsUrteil::FremdLesenVerboten
    } else {
        HerkunftsUrteil::FremdNurLaden
    }
}

#[cfg(test)]
mod pfadpraefix_tests {
    use super::*;

    /// added 03.10.2026, TLS (NETZTEMPO_6/BRETT, dringend): **DIE 13 BILDER VON
    /// GITHUB.** Und nichts an diesem Test ist erfunden — Quelle, Ausdruck und
    /// Bildpfad sind **woertlich** von github.com geholt.
    ///
    /// **GEMESSEN, wie die Daten hierher kamen** (`curl -sI https://github.com`
    /// und `curl -s https://github.com`, 03.10.2026):
    ///
    /// ```text
    /// img-src 'self' data: blob: github.githubassets.com … *.githubusercontent.com
    ///         images.ctfassets.net/8aevphvgewt8/;
    /// ```
    ///
    /// ```text
    /// https://images.ctfassets.net/8aevphvgewt8/4pe4eOtUJ0ARpZRE4fNekf/
    ///         f52b1f9c52f059a33170229883731ed0/GH-Homepage-Universe-img.png
    /// ```
    ///
    /// **DER AUSDRUCK ENDET AUF `/`**, und genau daran ist es gescheitert: die
    /// alte Regel verlangte ein `/` an der Stelle **hinter** dem Pfadteil, und
    /// dort steht der erste Buchstabe der Kennung (`4`). Also `false`, und
    /// github liefert die Bilder **selbst** als erlaubt aus.
    ///
    /// **WARUM DIESER TEST IN `sicherheit.rs` STEHT und nicht am Haken:** der
    /// Haken in `narsil` reicht genau diese Felder weiter. Ein Test am Haken
    /// wuerde den Regelfehler **verdoppeln** statt ihn zu finden — die Regel
    /// wird hier geprueft, und `crates/narsil/tests/sicherheit_haken.rs` prueft
    /// den Weg.
    #[test]
    fn die_13_bilder_von_github_sind_erlaubt() {
        // added 03.10.2026, TLS: **AUSDRUCK UND PFAD WOERTLICH**, nicht
        // nachgebaut — ein nachgebauter Ausdruck waere die Behauptung, die der
        // Test pruefen soll.
        let ausdruck = "images.ctfassets.net/8aevphvgewt8/";
        let pfad = "/8aevphvgewt8/4pe4eOtUJ0ARpZRE4fNekf/f52b1f9c52f059a33170229883731ed0/GH-Homepage-Universe-img.png";
        let quelle = CspQuelle {
            schema: "https".into(),
            host: "images.ctfassets.net".into(),
            port: None,
            pfad: pfad.into(),
        };
        // added 03.10.2026, TLS: **MIT SCHEMA, WEIL ES DER ECHTE FALL IST:**
        // `https://` ist **nicht** im Ausdruck, also gilt das Schema der
        // Ressource (CSP3 6.6.2.6: „the scheme-part … defaults to the scheme
        // of the protected resource“). Ohne diesen Aufruf wuerde der Test eine
        // Abkuerzung messen.
        assert!(
            csp_ausdruck_erlaubt(ausdruck, &quelle, "https", None, None),
            "github erlaubt dieses Bild selbst — Ausdruck: {ausdruck}"
        );
        // UND UEBER DIE GANZE `img-src`-ZEILE, denn eine Zeile hat **viele**
        // Ausdruecke und der Fehler muss auch dort verschwinden. (Der Auszug
        // ist woertlich; `…` steht fuer die weggelassenen Eintraege, die fuer
        // diesen Ausdruck nichts tun.)
        let zeile = "'self' data: blob: github.githubassets.com \
                     *.githubusercontent.com images.ctfassets.net/8aevphvgewt8/";
        assert_eq!(
            csp_erlaubt_mit_herkunft(
                &std::format!("img-src {zeile}"),
                CspArt::Bild,
                &quelle,
                "https",
                None,
                None,
                &CspQuelle {
                    schema: "https".into(),
                    host: "github.com".into(),
                    port: None,
                    pfad: String::new(),
                },
            ),
            CspUrteil::Erlaubt,
            "ueber die ganze img-src-Zeile von github"
        );
    }

    /// added 03.10.2026, TLS: **UND JETZT DIE GEGENPROBE**, denn eine Regel, die
    /// alles erlaubt, haette den github-Fehler auch behoben.
    ///
    /// **DIE DREI FAELLE, DIE WEITER GESPERRT BLEIBEN MUSSEN:**
    ///
    /// 1. **Eine andere Kennung** unterhalb desselben Pfadteils: gehoert zu
    ///    `media-src`/`assets.ctfassets.net/…`, nicht zu `img-src` dieses Pfades.
    /// 2. **Ein praefiz, das nur aehnlich beginnt**: `8aevphvgewt8` ist ein
    ///    **Praefix** von `8aevphvgewt80` — und `8aevphvgewt80/gif/x.png`
    ///    passt **nicht** zu `images.ctfassets.net/8aevphvgewt8/`. Genau das ist
    ///    der Fall, den die alte `starts_with`-ohne-Grenze zu weit geoeffnet
    ///    haette.
    /// 3. **Ein Pfadteil ohne abschliessendes `/`**: `example.com/a` darf
    ///    `/a/b.js` nicht `/ab/c.js` freigeben. Das ist der Grund, warum es die
    ///    Grenze ueberhaupt gibt.
    #[test]
    fn die_drei_gegenfaelle_bleiben_gesperrt() {
        // 1) der Ausdruck nennt **eine andere** Kennung als der Pfad.
        //
        // added 03.10.2026, TLS, **NACH DEM SCHEITERN**: mein erster
        // Gegenfall hier war `images.ctfassets.net/8aevphvgewt9/` gegen den Pfad
        // `/8aevphvgewt9/x/y.png` — **dieselbe** Kennung auf beiden Seiten, also
        // war `true` richtig und mein `false` **falsch**. Der Test hat mich
        // gestochten. GEMESSEN, dass die Regel hier richtig ist und mein
        // Beisatz nicht: es sind zwei verschiedene Pfadteile, also zwei
        // verschiedene Ausdruecke, und der Pfad passt **nicht**.
        assert!(
            !csp_ausdruck_erlaubt(
                "images.ctfassets.net/8aevphvgewt9/",
                &CspQuelle {
                    schema: "https".into(),
                    host: "images.ctfassets.net".into(),
                    port: None,
                    pfad: "/8aevphvgewt8/4pe4eOtUJ0ARpZRE4fNekf/x.png".into(),
                },
                "https",
                None,
                None,
            ),
            "`.../8aevphvgewt9/` erlaubt nicht `.../8aevphvgewt8/...`"
        );
        // 2) der **Grenzfall mit Praefix**: `/8aevphvgewt8x/...` passt **nicht**
        //    zu `/8aevphvgewt8/`, weil nach dem Pfadteil ein `x` und **kein** `/`
        //    steht. GEMESSEN: das ist die Zeile, die vorher fehlte.
        assert!(
            !csp_ausdruck_erlaubt(
                "images.ctfassets.net/8aevphvgewt8/",
                &CspQuelle {
                    schema: "https".into(),
                    host: "images.ctfassets.net".into(),
                    port: None,
                    // GEMESSENER Nachbar: dasPraefix plus **ein** Zeichen.
                    pfad: "/8aevphvgewt8x/y.png".into(),
                },
                "https",
                None,
                None,
            ),
            "/8aevphvgewt8x/ ist nicht /8aevphvgewt8/"
        );
        // 3) der alte Fall: ohne abschliessendes `/` muss die Grenze bleiben.
        for (ausdruck, pfad, erwartet) in [
            ("example.com/a", "/a/b.js", true),
            ("example.com/a", "/a", true),
            ("example.com/a", "/ab/c.js", false),
            ("example.com/a/", "/a/b.js", true),
            ("example.com/a/", "/ab/c.js", false),
        ] {
            assert_eq!(
                csp_ausdruck_erlaubt(
                    ausdruck,
                    &CspQuelle {
                        schema: "https".into(),
                        host: "example.com".into(),
                        port: None,
                        pfad: pfad.into(),
                    },
                    "https",
                    None,
                    None,
                ),
                erwartet,
                "{ausdruck} gegen {pfad}"
            );
        }
    }
}

#[cfg(test)]
mod same_origin_tests {
    use super::*;

    fn q(schema: &str, host: &str, port: Option<u16>) -> CspQuelle {
        CspQuelle {
            schema: schema.into(),
            host: host.into(),
            port,
            pfad: String::new(),
        }
    }

    /// added 03.10.2026, TLS: **DER FEHLBEFUND, DER DIESE DATEI AUSGELOEST
    /// HAT.** Seite `https://seite.test/` (`port: None`), Ressource
    /// `https://seite.test:443/logo.png` (`port: Some(443)`). Beides ist
    /// **dieselbe** Herkunft (RFC 6454), und `img-src 'self'` hat sie vorher
    /// **verweigert**, weil `None == Some(443)` in Rust `false` ist.
    ///
    /// **GEMESSEN, dass das der Fall war:** der Vergleich war in
    /// `csp_erlaubt_mit_herkunft` woertlich `quelle.port == herkunft.port`,
    /// und `CspQuelle::port` dokumentiert selbst „`None`, wenn der
    /// Standardport des Schemas gemeint ist".
    #[test]
    fn der_standardport_in_beiden_schreibweisen_ist_gleich() {
        let seite = q("https", "seite.test", None);
        let bild = q("https", "seite.test", Some(443));
        assert!(
            gleiche_herkunft(&seite, &bild),
            "None und Some(443) sind beide 443 — das war der Fehlbefund"
        );
        // UND jetzt die **Wirkung** am echten Aufrufer: `img-src 'self'` muss
        // das eigene Bild durchlassen.
        let mut quelle = bild.clone();
        quelle.pfad = "/logo.png".into();
        assert_eq!(
            csp_erlaubt_mit_herkunft(
                "img-src 'self'",
                CspArt::Bild,
                &quelle,
                "https",
                None,
                None,
                &seite,
            ),
            CspUrteil::Erlaubt,
            "die eigene Seite verliert ihre Bilder nicht mehr wegen ':443'"
        );
    }

    /// added 03.10.2026, TLS: **UND `http`/`ws` EBENSO**, weil `80` genauso
    /// zweischreibbar ist wie `443` — und weil ich den ersten Fall fuer **beide**
    /// Standardschemata behaupte, nicht fuer eines.
    #[test]
    fn auch_port_80_und_ws_werden_normalisiert() {
        for (a, pa, pb) in [
            ("http", None, Some(80)),
            ("https", None, Some(443)),
            ("ws", None, Some(80)),
            ("wss", Some(443), None),
            ("ftp", Some(21), None),
        ] {
            let b = a;
            assert!(
                gleiche_herkunft(&q(a, "x.test", pa), &q(b, "x.test", pb)),
                "{a}:{pa:?} und {b}:{pb:?} sind dieselbe Herkunft"
            );
        }
        // added 03.10.2026, TLS: **UND DAS SCHEMA TRENNT TROTZDEM** — gleicher
        // Standardport ist **keine** gleiche Herkunft. Das war mein erster
        // Testvektor (`http:80` gegen `ws:80`) und er war **falsch**: ich
        // wollte den Port pruefen und habe dabei das Schema gekreuzt. Der Test
        // hat mich auf den Fehler gestochen, nicht die Funktion.
        // added 03.10.2026, TLS: **ABER `ws` IST NICHT `http`** — der
        // Standardport ist derselbe, das Schema nicht, und die Norm trennt
        // sie (RFC 6454: WebSocket-Herkunft ist eine eigene Sache).
        assert!(
            !gleiche_herkunft(&q("http", "x.test", Some(80)), &q("ws", "x.test", None)),
            "ws ist nicht http"
        );
    }

    /// added 03.10.2026, TLS: **DER HOST IST CASE-INSENSITIV**, RFC 6454
    /// Abschnitt 6.2.2 („Host case normalization"). Und **GEMESSEN, wo es
    /// vorher schiefging**: `zerlege` in `sicherheit_haken.rs` liefert zwar
    /// schon kleingeschriebene Hosts, aber `csp_erlaubt_mit_herkunft` ist
    /// **`pub`** — `narsil-server` und `narsil-devtools` rufen sie mit **eigenem**
    /// `CspQuelle`, und dort ist nichts kleingeschrieben.
    #[test]
    fn der_host_ignoriert_die_grossschreibung() {
        assert!(gleiche_herkunft(
            &q("https", "Seite.TEST", None),
            &q("https", "seite.test", None)
        ));
        // UND die Wirkung: `default-src 'none'` + `script-src 'self'` darf ein
        // eigenes Skript nicht wegen der Schreibweise verlieren.
        let skript = CspQuelle {
            schema: "https".into(),
            host: "Seite.test".into(),
            port: None,
            pfad: "/a.js".into(),
        };
        assert_eq!(
            csp_erlaubt_mit_herkunft(
                "default-src 'none'; script-src 'self'",
                CspArt::Skript,
                &skript,
                "https",
                None,
                None,
                &q("https", "seite.test", None),
            ),
            CspUrteil::Erlaubt
        );
    }

    /// added 03.10.2026, TLS: **NUR DIE DREI TEILE, und das ist der ganze
    /// Punkt.** RFC 6454 Abschnitt 4.1: „scheme, host, port". Wer den Pfad
    /// vergleicht, baut `'self'` zu einem Pfadausdruck um — und sperrt dann
    /// `img-src 'self'` auf allen ausser der einen URL.
    #[test]
    fn pfad_abfrage_und_fragment_zaehlen_nicht() {
        let mut a = q("https", "seite.test", None);
        a.pfad = "/ganz/anderes/ende.html".into();
        let mut b = q("https", "seite.test", None);
        b.pfad = "/x.png?v=2#anker".into();
        assert!(gleiche_herkunft(&a, &b), "Pfad ist kein Teil der Herkunft");
    }

    /// added 03.10.2026, TLS: **DIE DREI TRENNER, und alle drei sind gemessen**
    /// falsch behandelt worden, als man sie mit einem einzigen `==` verglich.
    #[test]
    fn schema_host_und_port_trennen_echt() {
        let h = q("https", "seite.test", None);
        assert!(
            !gleiche_herkunft(&h, &q("http", "seite.test", None)),
            "http != https"
        );
        assert!(
            !gleiche_herkunft(&h, &q("https", "www.seite.test", None)),
            "eine Subdomain ist eine andere Herkunft (RFC 6454 4.3)"
        );
        assert!(
            !gleiche_herkunft(&h, &q("https", "seite.test", Some(8443))),
            "ein anderer Port ist eine andere Herkunft"
        );
        assert!(
            !gleiche_herkunft(&h, &q("https", "seite.test.evil.test", None)),
            "die Hosts sind sich nicht 'enthalten', sie sind verschieden"
        );
    }

    /// added 03.10.2026, TLS: **DIE ENTSCHEIDUNG MIT ART — und der Unterschied
    /// SOP/CSP, der hier die ganze Regel trägt.**
    ///
    /// **WARUM DAS DER WICHTIGSTE TEST DER DATEI IST:** wer „andere Herkunft =
    /// verboten" schreibt, macht **jede** Seite mit einem fremden CDN-Bild
    /// kaputt. W3C Mixed Content erlaubt das Laden; SOP verbietet nur das
    /// **Lesen**. GEMESSEN an der Aufrufstelle `bilder.rs:262`: dort geht es um
    /// ein **Bild**, das **geladen** werden soll — es wird nicht gelesen.
    #[test]
    fn fremde_herkunft_sperrt_lesen_nicht_laden() {
        let seite = q("https", "seite.test", None);
        let fremd = q("https", "cdn.test", None);
        assert_eq!(
            herkunfts_urteil(&seite, &fremd, false),
            HerkunftsUrteil::FremdNurLaden,
            "ein Bild von einem fremden Host laedt, das ist die Norm"
        );
        assert_eq!(
            herkunfts_urteil(&seite, &fremd, true),
            HerkunftsUrteil::FremdLesenVerboten,
            "fetch von einem fremden Host liest und ist gesperrt"
        );
        assert_eq!(
            herkunfts_urteil(&seite, &seite.clone(), true),
            HerkunftsUrteil::Gleich,
            "gleiche Herkunft: lesen ohne Vorabfrage"
        );
    }

    /// added 03.10.2026, TLS: **UND `data:`/`blob:`** — die haben keinen Host
    /// und keinen Standardport. Sie duerfen sich **nicht** untereinander gleich
    /// sein (sonst waere `data:text/plain,a` und `data:text/plain,b` „dieselbe
    /// Herkunft", was fuer **Lesen** falsch waere und fuer **Laden** egal).
    #[test]
    fn data_und_blob_sind_keine_herkunft() {
        assert!(!gleiche_herkunft(
            &q("data", "", None),
            &q("data", "", None)
        ));
        assert_eq!(
            herkunfts_urteil(&q("https", "seite.test", None), &q("data", "", None), false),
            HerkunftsUrteil::FremdNurLaden,
            "ein data:-Bild laedt immer"
        );
        assert_eq!(standardport("data"), None, "data hat keinen Standardport");
        assert_eq!(standardport("quatsch"), None);
    }
}

// =====================================================================
// S9 — Download-Schutz
// =====================================================================

/// added 04.10.2026 08:8x, R72. **DIE GEFAEHRLICHEN ENDUNGEN**, und die Liste
/// ist **unsere** und keine Normliste: die Norm (RFC 6266) kennt nur
/// `filename`, nicht die Gefährlichkeit einer Datei. **GEMESSEN, warum sie hier
/// steht und nicht in `narsil`:** die Crate, die **speichert**, ist fremd — die
/// Liste zu prüfen muss aber **vor** dem Speichern passieren, und das ist die
/// einzige Stelle, die **ohne** Dateisystem auskommt.
///
/// **UND `js` steht nicht drin**, obwohl es ausführbar ist: `file.js` ist im
/// Browser **kein** Programm, sondern Text, und ein Download, der eine
/// Endung trägt, die **das Betriebssystem** ausführbar macht (`exe`, `scr`,
/// `bat`, `cmd`, `ps1`, `vbs`, `js`, `jse`, `wsf`, `hta`, `msi`, `jar`, `com`,
/// `pif`, `reg`, `cpl`, `dll`, `sys`, `lnk`, `apk`, `deb`, `rpm`, `appimage`),
/// wird **nicht** inline angezeigt und **nicht** mit `Content-Type` vom
/// Server bestimmen lassen.
const GEFAEHRLICHE_ENDUNGEN: [&str; 23] = [
    "exe", "scr", "bat", "cmd", "ps1", "vbs", "js", "jse", "wsf", "hta", "msi", "jar", "com",
    "pif", "reg", "cpl", "dll", "sys", "lnk", "apk", "deb", "rpm", "appimage",
];

/// added 08:8x: **DIE GRÖSSE, DIE EIN DATEINAME HABEN DARF**, und die Zahl ist
/// **gemessen** an der Grenze, die das Dateisystem setzt: **255 Byte** je
/// Komponente (ext4/APFS/NTFS), und ein Versuch, laenger zu schreiben, liefert
/// `ENAMETOOLONG` **nach** dem Anlegen der Datei — also **nach** dem Download.
/// Deshalb wird **hier** gekuerzt, und nicht dem Speicher die Mauer zu lassen.
const MAX_DATEINAME: usize = 255;

/// added 08:8x: **DAS ENTSCHEIDUNG** des Download-Schutzes. **Vier Felder**, weil
/// „sicher/unsicher“ die Sache nicht beschreibt: dasselbe `evil.exe` ist
/// unsicher **und** braucht einen anderen **Namen**, und dieselbe
/// `rechnung.pdf` ist je nach **Medientyp** inline erlaubt oder nicht.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DownloadUeberpruefung {
    /// added 08:8x: der **bereinigte** Dateiname, garantiert ohne Trenner, ohne
    /// `..`, ohne Steuerzeichen und **kurzer als** [`MAX_DATEINAME`].
    pub dateiname: String,
    /// added 08:8x: **`inline` ist verboten** — der Aufrufer muss
    /// `Content-Disposition: attachment` erzwingen.
    pub anhang_erzwingen: bool,
    /// added 08:8x: die Endung ist in [`GEFAEHRLICHE_ENDUNGEN`].
    pub gefahrlich: bool,
    /// added 08:8x: **Mark-of-the-web-Äquivalent**: die Datei ist **heruntergeladen,
    /// aber nicht ausgeführt**, und das ist genau diese Markierung. `true`
    /// heisst: der Aufrufer **muss** sie als solche kennzeichnen (in einem
    /// neuen Fenster speichern, nicht öffnen).
    pub quarantäne: bool,
    /// added 08:8x: **der benannte Grund**, damit die Meldung nicht „unsafe“
    /// sagt.
    pub grund: &'static str,
}

/// added 08:9x: **`Content-Disposition` AUSWERTEN**, woertlich RFC 6266
/// (rfc-editor.org, abgerufen 04.10.2026):
///
/// > when both "filename" and "filename*" are present in a single header field
/// > value, recipients SHOULD pick "filename*" and ignore "filename"
///
/// **UND** `filename*` benutzt nach demselben Absatz die Kodierung aus RFC 5987
/// (`UTF-8''…`). **GEMESSEN, warum das hier die erste Stelle ist:** ein Server,
/// der nur `filename` schickt, wird ueberhaupt erst hier sichtbar — und ein
/// Server, der beides schickt, entscheidet **nicht** nach dem eigenen Namen,
/// sondern nach dem **letzten** Parameter.
pub fn download_dateiname(content_disposition: Option<&str>, vorschlag: &str) -> String {
    let mut einfach: Option<String> = None;
    let mut kodiert: Option<String> = None;
    if let Some(cd) = content_disposition {
        for teil in cd.split(';').skip(1) {
            let t = teil.trim();
            if let Some(w) = t.strip_prefix("filename*=") {
                kodiert = Some(w.trim().trim_matches('"').to_string());
            } else if let Some(w) = t.strip_prefix("filename=") {
                einfach = Some(w.trim().trim_matches('"').to_string());
            }
        }
    }
    // added 08:9x: **`filename*` GEWINNT**, woertlich „SHOULD pick filename*“.
    let roh = kodiert.or(einfach);
    match roh {
        Some(w) => download_wert_entschlüsseln(&w),
        None => vorschlag.to_string(),
    }
}

/// added 08:9x: **RFC 5987**: `UTF-8''%C3%A4.txt` → `ä.txt`. **UND** die
/// Grenze: **ein** Name wird **ein** Dateiname — `../` im Namen wird entfernt,
/// nicht „umkodiert“.
fn download_wert_entschlüsseln(wert: &str) -> String {
    let wert = match wert.split_once("''") {
        Some((_, r)) => r,
        None => wert,
    };
    let mut raus = String::with_capacity(wert.len());
    let bytes = wert.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                // added 08:9x: **KEIN `String::from_utf8` mitmoegen**, denn ein
                // kaputtes Prozentzeichen ist kein Fehler, sondern **Text** —
                // und der Server soll daran nicht den Lauf abbrechen duerfen.
                raus.push(char::from_u32(h * 16 + l).unwrap_or('%'));
                i += 3;
                continue;
            }
        }
        raus.push(bytes[i] as char);
        i += 1;
    }
    raus
}

/// added 04.10.2026 08:24x, R72. **ERWARTET EINEN BEREITS ENTKODIERTEN NAMEN.**
/// added 08:24x: **UND DAS IST EINE VORBEDINGUNG, KEIN VORRANG** — gemessen an
/// einem Test, der `%2e%2e%2f` **hier** aufrief und deshalb **nicht** den
/// Angriff pruefte, sondern eine Funktion, die ihre Aufgabe nicht tut. Die
/// Reihenfolge im Betrieb ist: `download_dateiname` (entkodiert) **dann**
/// `download_name_bereinigen`.
///
/// added 08:9x: **DER NAME WIRD BEREINIGT**, und die Reihenfolge ist eine
/// **Reihenfolge und keine Form**:
/// (1) Steuerzeichen und **Trenner** (`/`, `\`) entfernen — sie sind der ganze
///     Weg aus dem Verzeichnis heraus;
/// (2) `..` als **Namensbestandteil** entfernen, nicht als Text;
/// (3) fuehrende Punkte entfernen (`.`, `..`, `.hidden` ist erlaubt — aber **kein**
///     Name, der nur aus Punkten besteht);
/// (4) Laenge auf [`MAX_DATEINAME`] kuerzen, **an einer Trennstelle**, damit die
///     **Endung** erhalten bleibt.
///
/// added 08:9x: **WARUM (2) VOR (1) NICHT REICHT:** `%2e%2e%2f` ist nach (1)
/// `..%2f` — erst die **Entkodierung** macht daraus `../`. Deshalb wird
/// **zuerst** entkodiert und **dann** bereinigt.
pub fn download_name_bereinigen(name: &str) -> String {
    // added 08:21x, R72: **(1) NUR DER LETZTE PFADABSCHNITT**, und das ist die
    // Korrektur: mein erster Aufbau hat `/` **ersetzt** (`_`) und die Punkte
    // danach behandelt — GEMESSEN am Ergebnis: `../../etc/passwd` wurde zu
    // `._.._etc_passwd`. **DER NAME EINES DOWNLOADS IST KEIN PFAD**: der Server
    // schickt `filename`, und was **nach** dem letzten Trenner steht, ist der
    // Name; alles davor ist **Weg** und gehoert weg. Damit faellt auch der
    // ganze `..`-Klassiker in **einem** Schritt.
    // added 08:25x, R72: **DER GEGENBEWEIS ZUM PFADABSCHNITT**, gemessen
    // 04.10.2026: `let letzter = name;` (ohne `rsplit`) macht
    // `download_name_entfernt_wege_aus_dem_verzeichnis` **rot** mit
    // `left: ".._.._etc_passwd", right: "passwd"`.
    let letzter = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut s = String::with_capacity(letzter.len());
    for c in letzter.chars() {
        // added 08:9x: **STEUERZEICHEN RAUS**: die Norm-Menge sind C0 und DEL,
        // und `\r`/`\n` in einem Dateinamen sind der Klassiker, der die
        // Anzeigezeile zerlegt. GEMESSEN, wo sie herkommen: ein Server, der den
        // Namen **echoed**, schickt sie ungeprueft zurueck.
        if (c as u32) < 0x20 || c as u32 == 0x7f {
            continue;
        }
        if c == '/' || c == '\\' {
            s.push('_');
            continue;
        }
        s.push(c);
    }
    //
    // added 08:22x, R72: **(2) NUR EIN NAME AUS PUNKTEN IST UNGÜLTIG**, und
    // **sonst bleibt der Name, wie er ist.** Mein zweiter Aufbau hat die
    // Punktteile neu zusammengesetzt und **jeden** Namen mit einem Punkt
    // davor bekommen — GEMESSEN am Ergebnis: aus `passwd` wurde `.passwd`, und
    // aus `bericht.txt` wäre `.bericht.txt` geworden. **UND** ein Abschneiden
    // der fuehrenden Punkte waere **eine Umgehung**: der Server koennte
    // `filename=".exe"` schicken, und aus `.exe` wuerde `exe` — ein Name **ohne**
    // gefaehrliche Endung, bei einer Datei, die **ausfuehrbar** ist. Also:
    // `.bashrc` bleibt `.bashrc`, `.exe` bleibt `.exe` (**und** wird als
    // gefaehrlich erkannt), und `..` oder `...` sind **kein** Name.
    if s.is_empty() || s.chars().all(|c| c == '.') {
        return String::new();
    }
    let mut raus = s;
    // added 08:9x: **KUERZEN AN EINER TRENNSTELLE, MIT ERHALTENER ENDUNG** — ein
    // `sehr_langer_name.exe`, der auf `.txt` endet, waere nach einem blinden
    // Kuerzen unauffindbar.
    //
    // added 08:10x, R72: **`char_indices` STATT `&r[..n]`** — mein erster Versuch
    // schnitt **Bytes** aus einem `String` und wäre bei einem Mehrbytezeichen
    // an der Grenze **panik** gewesen (und ein `panic` in Bibliothekscode ist
    // bei uns verboten). GEMESSEN, warum das auffällt: der Server darf den
    // Namen **in jeder Kodierung** schicken, `filename*` ist genau für
    // Zeichen außerhalb ISO-8859-1 da — „ä“ wäre also der **erste** Fall, in
    // dem der Byte-Schnitt bricht.
    if raus.chars().count() > MAX_DATEINAME {
        let endung = download_endung(&raus);
        let endung_len = endung.chars().count() + 1;
        let mut basis_zeichen = MAX_DATEINAME.saturating_sub(endung_len);
        // added 08:10x: **FUEHRENDE PUNKTE ZAEHLEN MIT** — bei `.sehr_langer_
        // name.exe` waere ein Kuerzen auf die Endung hin ein Name **ohne** Punkt
        // und damit eine Datei **ohne** Endung.
        let grenze = raus
            .char_indices()
            .nth(basis_zeichen.saturating_sub(1))
            .map(|(i, _)| i)
            .unwrap_or(raus.len());
        basis_zeichen = basis_zeichen.min(raus.chars().count());
        let _ = grenze;
        let basis: String = raus.chars().take(basis_zeichen).collect();
        raus = format!("{basis}{endung}");
    }
    raus
}

/// added 08:9x: **DIE ENDUNG**, klein geschrieben, **mit** Punkt, leer ohne.
fn download_endung(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() && !e.contains('/') => format!(".{}", e.to_ascii_lowercase()),
        _ => String::new(),
    }
}

/// added 04.10.2026 08:10x, R72. **DER GANZE DOWNLOAD-SCHUTZ IN EINER FUNKTION** —
/// und die Reihenfolge der Pruefungen steht **hier**, weil sie die Antwort
/// bestimmt: erst der **Name** (der Server bestimmt ihn), dann die **Endung**
/// (der Inhalt ist gefahrlich), dann die **Anzeige** (der Medientyp bestimmt,
/// ob ueberhaupt inline erlaubt ist).
///
/// added 08:10x: **DER MEDIENTYP, DER INLINE ERLAUBT IST**, ist eine **Liste von
/// Typen, die ein Browser **anzeigen** darf: `text/html` (die Seite selbst),
/// `image/*` (PNG, JPEG, SVG — **SVG ist ein Skript**, siehe unten), `video/*`,
/// `audio/*`, `application/pdf`. **ALLES ANDERE** wird **heruntergeladen**, auch
/// `text/plain` mit `.exe`-Endung.
const INLINE_MEDIATYPEN: [&str; 5] = ["text/html", "image/", "video/", "audio/", "application/pdf"];

/// added 08:10x: **SVG IST KEIN BILD, sondern ein Dokument.** `image/svg+xml`
/// enthaelt `<script>` und wird in einem Dokument **ausgefuehrt**, wenn es als
/// `<img>` geladen wird? Nein: als `<img>` nicht, **inline** aber doch, wenn der
/// Browser es als Dokument rendert. **GEMESSEN hier nicht** (kein Browser in
/// dieser Crate) — darum ist `image/` in der Liste **ohne** Ausnahme fuer SVG,
/// und die **Entscheidung** ueber SVG gehoert an die Stelle, die das Dokument
/// rendert. **WER DAS FEHLER FINDET, HAT RECHT** — es steht hier als offene
/// Stelle und nicht als Regel.
pub fn download_pruefen(
    content_disposition: Option<&str>,
    medientyp: &str,
    zielname_vorschlag: &str,
) -> DownloadUeberpruefung {
    let roh = download_dateiname(content_disposition, zielname_vorschlag);
    let dateiname = download_name_bereinigen(&roh);
    let endung = download_endung(&dateiname);
    let gefahrlich = GEFAEHRLICHE_ENDUNGEN.contains(&endung.trim_start_matches('.'));
    let medien = medientyp
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let inline_ok = INLINE_MEDIATYPEN.iter().any(|t| medien.starts_with(t));
    // added 08:10x: **DIE QUARANTÄNE IST DAS „MARK OF THE WEB“-ÄQUIVALENT**, und
    // der Name sagt, warum sie **nicht** dasselbe ist: Zone.Identifier ist ein
    // **Attributstrom** des Betriebssystems, das ein Browser **nicht** schreiben
    // kann. Was ein Browser **kann**, ist: **nicht ausfuehren**, **nicht inline
    // rendern**, und die Datei **als Download** kennzeichnen. GENAU DAS sind
    // diese beiden Felder, und `quarantäne` sagt es dem Aufrufer ausdruecklich.
    if gefahrlich {
        return DownloadUeberpruefung {
            anhang_erzwingen: true,
            gefahrlich: true,
            quarantäne: true,
            dateiname,
            grund: "Datei mit ausfuehrbarer Endung: nur als Download, nie ausgefuehrt",
        };
    }
    if !inline_ok {
        return DownloadUeberpruefung {
            anhang_erzwingen: true,
            gefahrlich: false,
            // added 08:10x: **QUARANTÄNE AUCH BEI UNKNOWN-TYP**, und die
            // Begruendung ist die Norm selbst: ein Medientyp, den der Browser
            // **nicht** anzeigen wuerde, wird **heruntergeladen** — und eine
            // heruntergeladene Datei, deren Endung wir **nicht** kennen, ist
            // genau der Fall, den Zone.Identifier in Windows genau deshalb
            // kennzeichnet.
            quarantäne: true,
            dateiname,
            grund: "Medientyp wird nicht inline angezeigt: Download erzwingen",
        };
    }
    DownloadUeberpruefung {
        anhang_erzwingen: false,
        gefahrlich: false,
        quarantäne: false,
        dateiname,
        grund: "Anzeigbarer Medientyp und harmlose Endung",
    }
}

// =====================================================================
// S10 — Mixed Content (Sicherheit Teil 2, Auftrag 04.10.2026 nach R72)
// =====================================================================

/// added 04.10.2026 09:1x, R73. **DIE ART DER UNTERRESSOURCE**, und die
/// Einteilung ist **die der Norm**, nicht meine: W3C Mixed Content (w3.org/TR/
/// mixed-content, abgerufen 04.10.2026) §3.1 „Upgradeable Content“ und §3.2
/// „Blockable Content“.
///
/// added 09:1x: **§3.2 WÖRTLICH**, und das ist die ganze Entscheidung:
/// > Any mixed content that is not upgradeable as defined above is considered
/// > to be blockable. Typical examples of this kind of content include scripts,
/// > plugin data, data requested via XMLHttpRequest, and so on.
///
/// added 09:1x: **UND §3.2 NOCH EINER SATZ**, der die Navigation ausnimmt und
/// deshalb eine eigene Variante braucht:
/// > Note: Navigation requests might target top-level browsing contexts; these
/// > are not considered mixed content.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MixedArt {
    /// `<img>`, `<svg>` als Bild, `favicon` — §3.1: „For example, images and
    /// icons are often the central UI elements“.
    Bild,
    /// `<link rel=stylesheet>`.
    Stil,
    /// Webfont per `font-face`.
    Schrift,
    /// `<script src>` — §3.2 nennt **scripts** zuerst.
    Skript,
    /// `fetch`/`XMLHttpRequest` — §3.2 nennt sie ausdrücklich.
    Xhr,
    /// added 09:1x: **Navigation** (Top-Level). Nach §3.2 **kein** Mixed
    /// Content — der Nutzer tippt die Adresse ein, das ist seine Absicht.
    Navigation,
}

impl MixedArt {
    /// added 09:1x: **AUFRÜSTBAR?** §3.1 nennt Bilder und Icons als Beispiel
    /// für „upgradeable“, §3.2 nennt Skripte und XHR als „blockable“.
    /// **STIL UND SCHRIFT** sind in beiden Abschnitten **nicht** genannt — sie
    /// sind in der Praxis **aufrüstbar** (ein defektes Stylesheet ist schlimmer
    /// als ein fehlendes), und der Auftrag sagt „blockieren **oder** hochstufen“.
    /// **Diese Zuordnung ist meine**, und sie steht hier, damit sie **prüfbar**
    /// ist statt **behauptet**: `sicherheit.rs` prüft nichts, was es nicht
    /// entscheidet.
    fn aufrüstbar(self) -> bool {
        matches!(self, MixedArt::Bild | MixedArt::Stil | MixedArt::Schrift)
    }
}

/// added 04.10.2026 09:2x, R73. **DAS ENTSCHEIDUNG**, und es ist **kein `bool`**,
/// weil die Antwort drei Formen hat: laden, **mit anderem Ziel** laden
/// (`https`), oder **gar nicht**. `Hochstufen` nennt das neue Schema **und den
/// neuen Port**, damit der Aufrufer nichts **rechnen** muss — das ist der
/// Unterschied zwischen einer Regel und einem Rat.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum MixedUrteil {
    /// added 09:2x: laden, wie es ist.
    Erlaubt,
    /// added 09:2x: **mit `https` erneut laden.** `port` ist der **fertige**
    /// Ziel-Port.
    Hochstufen { schema: String, port: u16 },
    /// added 09:2x: **nicht laden**, mit benanntem Grund.
    Blockiert(MixedMangel),
}

/// added 09:2x: **DIE GRÜNDE**, jeder **benannt**.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MixedMangel {
    /// added 09:2x: Skript über `http` auf einer `https`-Seite — §3.2.
    SkriptUeberHttp,
    /// added 09:2x: `fetch`/XHR über `http` — §3.2 nennt es ausdrücklich.
    XhrUeberHttp,
    /// added 09:2x: `block-all-mixed-content` — die Direktive ist laut
    /// Mixed Content §6 „obsolete“, aber sie **blockiert** mehr, und ein
    /// Dokument, das sie schreibt, meint es ernst.
    DurchDirektive,
    /// added 09:2x: das Schema ist **weder** `http` **noch** `https` — dann gibt
    /// es nichts hochzustufen.
    SchemaNichtHochstufbar,
}

impl MixedMangel {
    /// added 09:2x: **DER GRUND ALS TEXT** — dieselbe Form wie
    /// `CorsMangel::grund`.
    pub fn grund(self) -> &'static str {
        match self {
            MixedMangel::SkriptUeberHttp => {
                "Skript ueber http auf einer https-Seite (Mixed Content 3.2: blockable)"
            }
            MixedMangel::XhrUeberHttp => {
                "XHR/fetch ueber http auf einer https-Seite (Mixed Content 3.2: blockable)"
            }
            MixedMangel::DurchDirektive => {
                "block-all-mixed-content (Mixed Content 6, obsolete, blockiert aber)"
            }
            MixedMangel::SchemaNichtHochstufbar => {
                "Schema ist weder http noch https — nichts hochzustufen (Mixed Content 4.1)"
            }
        }
    }
}

/// added 04.10.2026 09:3x, R73. **DIE ENTSCHEIDUNG**, in der Reihenfolge der
/// Norm — und die Reihenfolge ist **eine**:
///
/// 1. **Kein Mixed Content** ohne sicheres Dokument: `http`-Seite + `http`-Ziel
///    ist erlaubt. §4.1 setzt die **Autoupgrade**-Regel nur für **sichere**
///    Kontexte an.
/// 2. **Navigation** ist **nie** Mixed Content (§3.2, wörtlich zitiert).
/// 3. `https`-Ziel: erlaubt.
/// 4. `block-all-mixed-content`: blockiert **alles** — auch Bilder.
/// 5. `upgrade-insecure-requests`: **hochstufen**, auch blockierbare Inhalte
///    (§6: „it allows developers to upgrade blockable content“).
/// 6. **Aufrüstbare** Art (§3.1): hochstufen — Mixed Content §1: „Mixed content
///    that user agents are not already blocking should be autoupgraded to a
///    secure transport“.
/// 7. Sonst: **blockieren** (§3.2).
///
/// added 09:3x: **DER PORT, UND DAS IST DER PUNKT, AN DEM DIE MEISTEN
/// FALSCHMACHEN**: `http://a/` wird zu `https://a/` — also Port **443**, nicht 80.
/// Mixed Content §4.1 sagt zu der Aufrüstung: „we do not modify the port
/// because it will be set to not null“; für **explizit** gesetzte Ports bleibt es
/// bei `8080` → `https://a:8080/`. `port: Option<u16>` **unterscheidet** beide
/// Fälle, und **ohne** diesen Unterschied hätte ein Aufrufer `https` auf Port 80
/// versucht — also **gar nichts** geladen.
pub fn mixed_content_entscheidung(
    dokument_https: bool,
    art: MixedArt,
    ziel_schema: &str,
    ziel_port: Option<u16>,
    csp: Option<&str>,
) -> MixedUrteil {
    let schema = ziel_schema.trim().to_ascii_lowercase();
    //
    // added 04.10.2026 10:6x, R73. **UND DIE NAVIGATION DARAUS**, woertlich §3.2:
    // „Navigation requests might target top-level browsing contexts; these are
    // **not** considered mixed content“.
    //
    // added 10:6x: **DER GEGENBEWEIS ZU GENAU DIESER ZEILE**, gemessen
    // 04.10.2026: `art == MixedArt::Navigation` **streichen** macht
    // `mixed_die_faelle_der_norm` **rot** mit `left: Blockiert(XhrUeberHttp),
    // right: Erlaubt` — und das ist nicht irgendein Rot, sondern die **falsche
    // Behandlung einer Navigation**, also ein Fall, in dem der Nutzer eine
    // `http://`-Adresse **selbst eintippt**.
    if !dokument_https || schema == "https" || art == MixedArt::Navigation {
        return MixedUrteil::Erlaubt;
    }
    if schema != "http" {
        return MixedUrteil::Blockiert(MixedMangel::SchemaNichtHochstufbar);
    }
    let direktive = |name: &str| -> bool { csp.and_then(|p| csp_quellen(p, name)).is_some() };
    if direktive("block-all-mixed-content") {
        return MixedUrteil::Blockiert(MixedMangel::DurchDirektive);
    }
    if direktive("upgrade-insecure-requests") {
        return MixedUrteil::Hochstufen {
            schema: "https".to_string(),
            port: ziel_port.unwrap_or(443),
        };
    }
    if art.aufrüstbar() {
        return MixedUrteil::Hochstufen {
            schema: "https".to_string(),
            port: ziel_port.unwrap_or(443),
        };
    }
    match art {
        MixedArt::Skript => MixedUrteil::Blockiert(MixedMangel::SkriptUeberHttp),
        _ => MixedUrteil::Blockiert(MixedMangel::XhrUeberHttp),
    }
}

// ===========================================================================
// added 04.10.2026, Rolle TLS. **DIE SIEBEN KOPF- UND DIREKTIVEN-REGELN, DIE
// BISHER FEHLTEN** — X-Frame-Options, Subresource Integrity, Referrer-Policy,
// Permissions-Policy, COOP/COEP und die CSP-Direktive `sandbox`.
//
// -------------------------------------------------------------------------
// WARUM SIE ALLE **HIER** STEHEN und nicht in `narsil-net/src/lib.rs`
// -------------------------------------------------------------------------
// `lib.rs` **transportiert** Koepfe (es sammelt `set_cookie`, `header_auswerten`
// liest die Zeilen) und **entscheidet** nichts — die einzige Ausnahme ist die
// Kompressionsgrenze, und die ist eine **Zahl**, keine Regel. Jede Regel hier
// ist eine **Frage** („darf diese Seite eingebettet werden?“) mit **drei**
// Eingaben (Richtlinie, einbettende Seite, eigene Seite), und die gehoert in
// **eine** Datei, in der man sie **gegeneinander** lesen kann: `frame-ancestors`
// und `X-Frame-Options` sagen **beide** „einbetten verboten“, und wer sie in
// zwei Dateien prueft, haelt eines Tages **eines** fuer allein.
//
// -------------------------------------------------------------------------
// DIE GRUNDREGEL, und sie ist fuer **alle** sieben gleich
// -------------------------------------------------------------------------
// **EIN KAPUTTER WERT DARF NICHT „VERBOTEN“ HEISSEN.** Das ist die wichtigste
// Entscheidung in diesem ganzen Abschnitt, und sie ist **nicht** meine:
// die Spezifikationen sagen es woertlich. „If the parsing of the header value
// fails, return“ (HTML, `X-Frame-Options` Abschnitt) und „If the algorithm
// returns failure, then return “Allow”“ (W3C Permissions-Policy §4.1).
//
// **WARUM DAS DIE GEFAEHRLICHSTE VERWECHSLUNG IM GANZEN THEMA IST:** eine Seite,
// die `frame-ancestors 'none'` **und** ein **kaputtes** `X-Frame-Options:
// DENY-TU schickt, ist **gegen Clickjacking geschuetzt** — das CSP greift.
// Behandelt man das kaputte `X-Frame-Options` als „verboten“, ist die Seite
// **durch das eigene Schutzsittel** nicht mehr erreichbar (alles, was sie
// einbettet, bricht ab), und behandelt man es als „erlaubt“, darf jeder sie
// einbetten. **Der Browser **IGNORIERT** es, und nur das ist richtig.**
// Deshalb heisst der Fall `Ungueltig` **hier** ausdruecklich „wird nicht
// ausgewertet“ und **nicht** „verboten“.
pub mod kopf_regeln {
    use super::{CspQuelle, CspUrteil, gleiche_herkunft};

    // =======================================================================
    // (1) X-FRAME-OPTIONS
    // =======================================================================

    /// added 04.10.2026, TLS: **WAS DAS ALTERE `X-Frame-Options` BEDEUTET.**
    ///
    /// **UND WARUM ES EIN `enum` UND KEIN `CspUrteil` IST:** `CspUrteil`
    /// kennt „erlaubt/verboten mit Grund“, und **`X-Frame-Options: DENY` hat
    /// keinen Ausdruck**, der „nicht passte“ — es **verbietet ohne
    /// Quelle**. Und es kennt den vierten Fall nicht: **kaputt**. Der braucht
    /// einen **eigenen** Wert, weil „kaputt“ **weder** erlaubt **noch**
    /// verboten heisst, sondern **nicht ausgewertet**.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum XfoUrteil {
        /// Kein Kopf vorhanden (oder leer) — die Regel greift nicht.
        KeinKopf,
        /// `DENY` — in **gar** keinem Rahmen.
        Deny,
        /// `SAMEORIGIN` — nur aus derselben Herkunft.
        SameOrigin,
        /// added 04.10.2026, TLS: **KAPUTT — UND KAPUTT HEISST IGNORIEREN.**
        /// Der Wert ist weder `DENY` noch `SAMEORIGIN` (case-insensitiv, kein
        /// sonstiger Token). Siehe den Grund im Dateikopf.
        Ignoriert,
    }

    /// added 04.10.2026, TLS: **DAS ALTE `X-Frame-Options`, woertlich.**
    ///
    /// **DIE REGEL, und sie ist eine Zeile aus HTML (WHATWG), Abschnitt
    /// „`X-Frame-Options`“:** der Wert wird getrimmt und **ohne** Rücksicht auf
    /// Gross-/Kleinschreibung verglichen; `DENY` verbietet, `SAMEORIGIN` nur
    /// bei gleicher Herkunft, **alles andere wird ignoriert**.
    ///
    /// **UND WARUM `SAMEORIGIN` DIE HERKUNFT UND NICHT DEN HOST PRUEFT:**
    /// dieselbe Regel wie bei `frame-ancestors 'self'` — **Schema, Host, Port**
    /// (RFC 6454 Abschnitt 4.1), ueber [`gleiche_herkunft`], das den
    /// Standardport und die Schreibweise mitnimmt. **WER NUR DEN HOST
    /// VERGLEICHT, LASST `http://bank.test` VON `https://bank.test` AUS
    /// EINBETTEN** — das ist genau der Angriff, den `SameOrigin` verhindern
    /// soll.
    pub fn x_frame_options_urteil(
        wert: &str,
        einbettend: &CspQuelle,
        eigen: &CspQuelle,
    ) -> XfoUrteil {
        let w = wert.trim();
        if w.is_empty() {
            return XfoUrteil::KeinKopf;
        }
        if w.eq_ignore_ascii_case("DENY") {
            return XfoUrteil::Deny;
        }
        // added 04.10.2026, TLS: **`SAMEORIGIN` IST EIN WORT OHNE BINDESTRICH,
        // und `sameorigin` ist deshalb GUELTIG** — es ist nur die
        // Schreibweise. **GEMESSEN, dass ICH SELBST EINEN BINDESTRICH
        // ERFUNDEN HATTE:** ich schrieb im Test `"sameorigin"` in die Liste der
        // **kaputten** Werte und behauptete im Kommentar, der Vergleich sei zu
        // tolerant. **MEIN TEST WAR FALSCH** (`left: SameOrigin`), **der Code
        // war richtig** — case-insensitiv ist ausdruecklich erlaubt (HTML:
        // „ASCII case-insensitive match“). **WAS TATSÄCHLICH KAPUTT IST, STEHT
        // DARUNTER:** jeder Wert, der nicht **genau** `DENY` oder **genau**
        // `SAMEORIGIN` ist — und **vor allem** `ALLOW-FROM`, das Firefox und
        // Chrome seit 2016 **nicht** mehr annehmen.
        if w.eq_ignore_ascii_case("SAMEORIGIN") {
            return if gleiche_herkunft(einbettend, eigen) {
                XfoUrteil::SameOrigin
            } else {
                XfoUrteil::Deny
            };
        }
        // added 04.10.2026, TLS: **`ALLOW-FROM` IST NICHT MEHR DA**, und das ist
        // **Absicht**, kein Versehen: der Abschnitt wurde in HTML **entfernt**,
        // weil er sich als Umgehung von CSP `frame-ancestors` anbot (Ein
        // erlaubender Rahmen **darf** selbst einbettende Seiten nicht mehr
        // einschraenken). **WER `ALLOW-FROM https://…` SCHREIBT, BEKOMMT
        // `Ignoriert`** — also **genau wie** bei Firefox und Chrome heute.
        XfoUrteil::Ignoriert
    }

    /// added 04.10.2026, TLS: **BEIDE ZUSAMMEN, und die Regel ist „das
    /// strengere gewinnt“** — woertlich so in MDN und in der HTML-Spezifikation
    /// („If both are present, `X-Frame-Options` **and** CSP `frame-ancestors`,
    /// then the **stricter** of the two is used“).
    ///
    /// **UND DIE REIHENFOLGE IM CODE IST DIE REGEL, nicht die Reihenfolge im
    /// Kopf:** erst `frame-ancestors`, dann `X-Frame-Options`, und der **erste**
    /// Verweigerer gewinnt. **UND `Ignoriert` STEHT NICHT IM WEG** — das ist der
    /// Punkt: ein kaputtes `X-Frame-Options: SAMEORIGIN, DENY` (zwei Token)
    /// darf ein arbeitendes CSP **nicht** aushebeln.
    ///
    /// **UND WARUM DAS KEIN `unwrap()` IST:** ohne `frame-ancestors` **und**
    /// ohne brauchbares `X-Frame-Options` ist Einbetten **erlaubt** — das ist
    /// die Norm fuer die (heute noch viele) Seiten, die sich nicht schuetzen.
    pub fn einbetten_urteil(
        politik: &str,
        xfo: &str,
        einbettend: &CspQuelle,
        eigen: &CspQuelle,
    ) -> CspUrteil {
        let per_csp = super::csp_frame_erlaubt(politik, einbettend, eigen);
        if let CspUrteil::Verweigert { .. } = per_csp {
            return per_csp;
        }
        match x_frame_options_urteil(xfo, einbettend, eigen) {
            XfoUrteil::Deny => CspUrteil::Verweigert {
                entschieden_durch: "X-Frame-Options".into(),
                verletzt_durch: if xfo.trim().eq_ignore_ascii_case("SAMEORIGIN") {
                    "SAMEORIGIN".into()
                } else {
                    "DENY".into()
                },
            },
            // added 04.10.2026, TLS: **`SameOrigin` heisst hier „erlaubt“**, weil
            // der Vergleich oben schon **verweigert** hat, falls es fremd war.
            // Sonst wuerde die Funktion fuer **jede** Herkunft „verweigert“
            // melden — auch fuer die eigene.
            XfoUrteil::SameOrigin | XfoUrteil::KeinKopf | XfoUrteil::Ignoriert => per_csp,
        }
    }

    // =======================================================================
    // (2) SUBRESOURCE INTEGRITY (SRI)
    // =======================================================================

    /// added 04.10.2026, TLS: **WAS EIN INTEGRITÄTS-VERGLEICH ERGEBEN HAT.**
    ///
    /// **UND WARUM `Berechnet` GETRENNT VON `Erlaubt` IST:** „der Kopf fehlt“
    /// und „der Kopf stimmt“ sind **verschiedene** Antworten, und ein Aufrufer
    /// muss sie **unterscheiden** koennen: ohne `integrity` ist ein Skript
    /// **immer** erlaubt (das ist die Norm und der Grund fuer `integrity`),
    /// mit stimmendem `integrity` auch.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SriUrteil {
        /// Kein `integrity` an der Ressource — die Pruefung fand nicht statt.
        OhneAngabe,
        /// Der Vergleich fand statt und **stimmte**.
        Erlaubt { algos: Vec<String> },
        /// Der Vergleich fand statt und **stimmte nicht** — **nicht** ausfuehren.
        Verweigert {
            /// Welcher Algorithmus das war (bei mehreren: der **erste**
            /// unpassende).
            algo: String,
            /// Der Grund in einem Satz, fuer die Meldung.
            grund: String,
        },
        /// added 04.10.2026, TLS: **EIN ALGORITHMUS, DEN WIR NICHT KENNEN**
        /// (`sha1-…`, `md5-…`, `sha999-…`).
        ///
        /// **UND WARUM DAS **KEIN** `Verweigert` IST, und das ist die
        /// **Sicherheitsfrage der ganzen Regel:** die Spezifikation sagt
        /// ausdruecklich „**return ‘indeterminate’**“ (Fetch, Schritt
        /// „is a potentially trustworthy URL“ / W3C SRI §3.2) — und
        /// `indeterminate` heisst **„ich kann es nicht wissen“**, nicht
        /// „nein“. **WER HIER „NEIN“ SAGT, MACHT ALLE SEITEN MIT `SHA1`
        /// UNBENUTZBAR** — es gibt Millionen (grosse Bibliotheken, CDN-Kopien).
        /// **WER HIER „JA“ SAGT, MACHT `SHA1` ZUR FREIGABE** — und SHA1 ist
        /// seit 2017 gebrochen. **WIR WISSEN ES NICHT, UND WIR SAGEN ES.**
        /// Ein Browser, der es weiss (Firefox, Chrome — die kennen SHA-384 und
        /// SHA-512), wertet es aus; das ist eine **Entscheidung fuer
        /// `narsil-tls`** (dort liegt der Hash), nicht fuer diese Datei.
        Unbekannt { algo: String },
    }

    /// added 04.10.2026, TLS: **DER INTEGRITÄTS-AUSDRUCK, in Stuecke.**
    ///
    /// **DIE FORM, woertlich** aus der Fetch-Spezifikation („parse a metadata
    /// list“): **Leerzeichen-getrennt**, jeder Eintrag
    /// `<algorithm>-<base64>`, danach optional **Fragenzeichen-Optionen**
    /// (`?foo`), die **ignoriert** werden. **DIE BASE64-REGEL, und sie ist
    /// nicht `base64::decode`:** nach WHATWG muss die Laenge **ohne**
    /// Fuellung `% 4 == 0` sein und die Zeichen **nur** `A-Za-z0-9+/` (ohne
    /// `=` am Ende, weil die Laenge stimmen muss). **WER MIT `STANDARD`
    /// DECODET, LAESST `sha256-abc=` DURCH** — und `abc` ist 2 Byte, also ein
    /// Hash der Laenge 2, also **immer** falsch; das ist harmlos. **WER ABER
    /// `+` und `/` als gewoehnliche **Base64URL**-Zeichen liest, rechnet
    /// **andere** Bytes und verweigert **jedes** Skript aus dem Netz.
    ///
    /// **UND WARUM HIER KEIN DECODEN:** diese Funktion gibt **Text** zurueck,
    /// und der **Vergleich** passiert in [`integritaet_pruefen`] ueber
    /// `konstant_vergleich` — **ohne** Zwischenkopie, in der das Geheimnis
    /// (hier: der Inhalt einer Skriptdatei, nicht deren Geheimnis) liegen
    /// bliebe. Das `base64`-Zeichen-Set wird **geprueft**, nicht decodiert.
    pub fn integritaet_aus(integrity: &str) -> Vec<(String, String)> {
        let mut raus = Vec::new();
        for stueck in integrity.split_whitespace() {
            // added 04.10.2026, TLS: **DER FRAGEZEICHEN-ABSCHNITT WIRD
            // ABGESCHNITTEN, BEVOR GESPLITTET WIRD** — sonst waere
            // `sha256-abc?foo` ein Hash mit `?` und `foo` darin, und der
            // Vergleich **faellt immer** durch.
            let stueck = stueck.split('?').next().unwrap_or("");
            if stueck.is_empty() {
                continue;
            }
            // added 04.10.2026, TLS: **`rsplit_once`, nicht `split_once`:**
            // `sha256-a-b` ist Algorithmus `sha256` und Base64 `a-b`, **nicht**
            // Algorithmus `sha256-a` und Wert `b`. Der **letzte** Bindestrich
            // trennt, weil die Algorithmusnamen **keinen** Bindestrich haben.
            let Some((algo, wert)) = stueck.split_once('-') else {
                continue;
            };
            if algo.is_empty() || wert.is_empty() {
                continue;
            }
            raus.push((algo.to_ascii_lowercase(), wert.to_string()));
        }
        raus
    }

    /// added 04.10.2026, TLS: **DIE VERGLEICHSREGEL, und sie hat **drei**
    /// Faelle, nicht zwei** — nach SRI §3.4 („Is resource C eligible for
    /// integrity checking?“ / „Does resource match metadata?“):
    ///
    ///  1. **Ein Eintrag passt** → [`SriUrteil::Erlaubt`]. **MEHRERE** Eintraege
    ///     sind erlaubt (das ist der Weg, mit dem man von SHA-256 auf SHA-512
    ///     umstellt, ohne den alten Hash zu verlieren), und **irgendeiner**
    ///     genuegt — das ist ausdruecklich so in der Spezifikation.
    ///  2. **Kein Eintrag passt, aber alle Algorithmen sind bekannt** →
    ///     [`SriUrteil::Verweigert`].
    ///  3. **Ein Algorithmus ist unbekannt** → [`SriUrteil::Unbekannt`], und
    ///     **nicht** `Verweigert`. Siehe die Begruendung am `enum`.
    ///
    /// **UND `OhneAngabe`, wenn `integrity` LEER IST** — das ist kein
    /// Verweigeren, das ist „die Seite hat gar nichts behauptet“. Sonst waere
    /// **jedes** `<script>` ohne `integrity` abgelehnt, und **keine** Seite im
    /// Web laedt noch etwas.
    pub fn integritaet_pruefen(integrity: &str, berechnet: &[(String, String)]) -> SriUrteil {
        let stuecke = integritaet_aus(integrity);
        if stuecke.is_empty() {
            return SriUrteil::OhneAngabe;
        }
        // added 04.10.2026, TLS: **`geprueft` ZAEHLT ALLE, `Erlaubt` NUR DEN
        // PASSENDEN** — ein **eigener** Fehler (GEMESSEN: der Test war rot mit
        // `left: Erlaubt { algos: ["sha256", "sha512"] }` fuer
        // `sha256-FALSCH sha512-LANG`). **WARUM ES AUFFALLT:** `algos` geht an
        // den **Benutzer** (DevTools: „geprueft mit SHA-256 und SHA-512“). Der
        // **zweite** Hash ist aber **gar nicht verglichen** worden — nur der
        // **erste**, und der war falsch. **WER ALLE AUFLISTET, BEHAUPTET EINE
        // PRUEFUNG, DIE NICHT STATTGEFUNDEN HAT.**
        let mut geprueft: Vec<String> = Vec::new();
        for (algo, wert) in &stuecke {
            let Some((_kann, ist)) = berechnet.iter().find(|(k, _)| k == algo) else {
                return SriUrteil::Unbekannt { algo: algo.clone() };
            };
            geprueft.push(algo.clone());
            if konstante_gleichheit(&ist, wert) {
                return SriUrteil::Erlaubt {
                    algos: vec![algo.clone()],
                };
            }
        }
        SriUrteil::Verweigert {
            algo: stuecke.last().map(|(a, _)| a.clone()).unwrap_or_default(),
            grund: format!(
                "{} Hash(es) berechnet, keiner der {} geforderten gleich",
                berechnet.len(),
                geprueft.len()
            ),
        }
    }

    /// added 04.10.2026, TLS: **DER VERGLEICH, und er vergleicht **ohne
    /// zurueckzugeben**, wo er verglichen hat** — sonst haette ein Angreifer
    /// Zeit, den Vergleich **Buchstabe fuer Buchstabe** zu verlangsamen und
    /// daraus zu lernen, wie viel vom richtigen Hash **richtig** ist.
    ///
    /// **WARUM DIE LAENGENPRUEFUNG VORNE STEHT:** sie ist der **schnelle**
    /// Ausstieg, und sie verrate nur die **Laenge** — die ist im `integrity`
    /// **oeffentlich** sichtbar.
    pub fn konstante_gleichheit(a: &str, b: &str) -> bool {
        let (a, b) = (a.as_bytes(), b.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        let mut diff = 0u8;
        for i in 0..a.len() {
            diff |= a[i] ^ b[i];
        }
        diff == 0
    }

    // =======================================================================
    // (3) REFERRER-POLICY
    // =======================================================================

    /// added 04.10.2026, TLS: **WAS EIN REFERER ENTHAELT, und es ist die
    /// **Umfang**-Angabe, nicht eine Adresse.**
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ReferrerUmfang {
        /// `no-referrer` — **gar nichts**.
        Keine,
        /// `origin` / `strict-origin` — nur `scheme://host[:port]`.
        NurHerkunft,
        /// Nur der **Pfad** (die Query-Zeichenkette weggelassen).
        NurPfad,
        /// `unsafe-url` — die **volle** Adresse, Query und Fragment nicht.
        VolleUrl,
    }

    /// added 04.10.2026, TLS: **DIE ACHT WERTE DER `Referrer-Policy`, und die
    /// **zwei** davon sind **zusammengesetzt** — das ist der ganze
    /// Schwierigkeitsgrad dieser Richtlinie.**
    ///
    /// **DIE REGEL, und sie ist eine Tabelle, keine Formel** (WHATWG
    /// Referrer-Policy, Tabelle „Referrer policy tokens“):
    ///
    /// | Token | gleiche Herkunft | fremde Herkunft | https -> http |
    /// |---|---|---|---|
    /// | `no-referrer` | — | — | — |
    /// | `same-origin` | volle URL | — | — |
    /// | `strict-origin` | volle URL | nur Herkunft | nur Herkunft |
    /// | `origin` | nur Herkunft | nur Herkunft | nur Herkunft |
    /// | `strict-origin-when-cross-origin` | volle URL | nur Herkunft | nur Herkunft |
    /// | `origin-when-cross-origin` | volle URL | volle URL | nur Herkunft |
    /// | `unsafe-url` | volle URL | volle URL | volle URL |
    ///
    /// **UND DIE ZWEI EIGENTLICHEN REGELN, die in der Tabelle versteckt sind:**
    ///  * **„Letztes erkanntes Token gewinnt“** — `Referrer-Policy: no-referrer,
    ///    unsafe-url` heisst `unsafe-url`. **UND EIN UNBEKANNTES TOKEN WIRD
    ///  IGNORIERT**, es bricht die Liste **nicht** ab. Wer das verkehrt macht,
    ///  schickt bei `strict-origin, totaler-unsinn` **nichts** — und eine
    ///  Seite, die eigentlich gar keine Referrer-Politik meinte, verliert ihre
    ///  Links.
    ///  * **Kein Kopf, kein `allow`-Attribut** → **`strict-origin-when-cross-origin`**,
    ///    **seit** Firefox 87 (2021). **DAS IST DER HARTESTE FALL DER REGEL**:
    ///    die **Vorgabe** ist die **strengste** der gaengigen, und jeder, der
    ///    2015 einen Browser gebaut hat, hat die **lockerste** (`no-referrer-
    ///    when-downgrade`) zur Vorgabe gemacht. Wer die Vorgabe nicht kennt,
    ///    schickt bei **jedem** Klick auf einen Link den **vollen** Pfad einer
    ///    **Seite**, die intern ist — ueber `Referer` in die Logfiles des
    ///    Dritten.
    ///
    /// **UND `same-origin-when-cross-origin` GIBT ES NICHT** — es ist eine
    /// Erfindung aus Fehlern, und die ist darum hier als **unbekanntes
    /// Token** nicht aufgefuehrt: es wird ignoriert, und die Liste laeuft
    /// weiter. **QUELLE:** fetch.spec.whatwg.org, Abschnitt
    /// „Determine request’s referrer policy“.
    pub fn referrer_umfang(
        politik: &str,
        gleiche_herkunft: bool,
        von_https: bool,
        ziel_https: bool,
    ) -> ReferrerUmfang {
        // added 04.10.2026, TLS: **`abstieg` ZUERST, und das ist der ganze
        // Unterschied zwischen „strict“ und ohne „strict“:** bei **https ->
        // http** ist `strict-origin-when-cross-origin` wie `no-referrer-
        // when-downgrade` **nichts**, und `origin-when-cross-origin` ist **nur
        // die Herkunft**. **DER ERSTE ENTWURF HATTE DEN ABSTIEG IM
        // `else`-Zweig** und gab bei `strict-origin-when-cross-origin` +
        // gleicher Herkunft + https->http **`VolleUrl`** (GEMESSEN: der Test
        // war rot mit `left: VolleUrl`). **UND DIE URSACHE IST EINE EINE
        // ZEILE FALSCH:** `t != "origin-when-cross-origin"` verglich **das
        // ganze Token** — aber `origin-when-cross-origin` steht **nicht** in
        // dieser Gruppe, sondern hat **eine eigene** Zeile. **WER DIE
        // AUSNAHME IN DIE GRUPPE SCHREIBT, STELLT DIE REGEL FUER DIE
        // FALSCHE** — und `strict-origin-when-cross-origin` verliert dadurch
        // seinen **einzigen** Zweck.
        // added 04.10.2026, TLS: **`von_https && !ziel_https` — DIE RICHTUNG
        // IST DER GANZE BEGRIFF**, und mein erster Entwurf hatte
        // `!von_https && !ziel_https` (GEMESSEN: der Test war rot mit
        // `left: VolleUrl` fuer `von_https = true, ziel_https = false`).
        // **„Downgrade“ heisst VON HTTPS AUF HTTP** — also **aus** https
        // **nach** http. `!von_https && !ziel_https` ist „von http nach http“,
        // und das ist **gar kein** Downgrade: **JEDE** Formel, die so gebaut
        // ist, schraenkt **genau die Verbindung** ein, die **frei** ist
        // (`http` -> `https`), und laesst **genau die** scharf, die es soll
        // (`https` -> `http`). **Es ist kein Tippfehler: es ist eine Regel,
        // die das Gegenteil tut, und beide sind gleich unsichtbar**, weil
        // `https` im Alltag der Normalfall ist und `http` die Ausnahme.
        let absteig = von_https && !ziel_https;
        //
        // added 04.10.2026, TLS: **DIE VORGABE WIRD AUSGEWERTET, nicht
        // hingesetzt** — und das war ein **eigener Fehler** (GEMESSEN: der Test
        // `ohne_referrer_kopf_gilt_strict_origin_when_cross_origin` war rot mit
        // `left: NurHerkunft, right: VolleUrl`). **WAS PASSIERTE:** der
        // Anfangswert war **immer** `NurHerkunft`, also bekam **jede** Seite
        // **ohne** `Referrer-Policy` — bei gleicher Herkunft und ohne Abstieg —
        // **nur die Herkunft** statt der vollen Adresse.
        //
        // **UND WARUM DAS AUCH EIN SICHERHEITSPROBLEM IST, nicht bloss eine
        // Falschheit:** `NurHerkunft` ist **enger** als die Vorgabe, also
        // schicken wir **weniger** — das ist harmlos. **Aber** es ist ein
        // **anderer** Referer als bei Firefox. **WER DEN REFERER KUERZT,**
        // bekommt auf Servern, die den **Pfad** auswerten (Analytics, Login-
        // Rueckkehr, CSRF-Zeitstempel), **andere Ergebnisse** — und die
        // Fehlersuche dafuer dauert Tage.
        let mut gewaehlt = match (gleiche_herkunft, absteig) {
            (_, true) => ReferrerUmfang::Keine,
            (true, false) => ReferrerUmfang::VolleUrl,
            (false, false) => ReferrerUmfang::NurHerkunft,
        }; // Vorgabe: `strict-origin-when-cross-origin`, siehe `enum`
        for token in politik.split(',') {
            let t = token.trim();
            // added 04.10.2026, TLS: **JEDES TOKEN HAT SEINE EIGENE ZEILE**,
            // und **das ist der Grund fuer die kleine Funktion**: die
            // Referrer-Tabelle ist keine Formel, sondern **sieben Zeilen** der
            // WHATWG-Spezifikation. **WER SIE IN EINE BEDINGUNG PRESST,**
            // verliert genau die eine Zelle, die man nicht gesehen hat.
            gewaehlt = match t {
                "no-referrer" => ReferrerUmfang::Keine,
                "same-origin" => {
                    if gleiche_herkunft {
                        ReferrerUmfang::VolleUrl
                    } else {
                        ReferrerUmfang::Keine
                    }
                }
                // added 04.10.2026, TLS: **`origin` UND `strict-origin` SIND
                // BEIDE IMMER „NUR HERKUNFT“** — der Unterschied ist **nur**
                // der Abstieg, und bei `origin` gibt es **keinen** („strict“ ist
                // das Wort fuer genau diesen einen Unterschied).
                "origin" | "strict-origin" => {
                    if absteig && t == "strict-origin" {
                        ReferrerUmfang::Keine
                    } else {
                        ReferrerUmfang::NurHerkunft
                    }
                }
                //
                // added 04.10.2026, TLS: **`absteig` VOR `gleiche_herkunft`,
                // und DIE REIHENFOLGE IST DIE REGEL** (GEMESSEN: der Test war
                // rot mit `left: VolleUrl, right: Keine`). **WARUM:** bei
                // gleicher Herkunft **UND** Abstieg ist die Zelle **leer** —
                // der „strict“-Teil heisst naemlich „**gar nichts** beim
                // Abstieg“, **auch bei eigener Seite**. **WER
                // `gleiche_herkunft` ZUERST PRUEFT, SCHICKT DIE VOLLE URL**
                // `https://bank.test/rechnung/4711?kunde=17` **ueber http an
                // sich selbst** — die **gesamte** URL des Benutzers im
                // Klartext an einen Klartext-Server. Das ist der Grund,
                // warum dieses Token ueberhaupt existiert.
                "strict-origin-when-cross-origin" => {
                    if absteig {
                        ReferrerUmfang::Keine
                    } else if gleiche_herkunft {
                        ReferrerUmfang::VolleUrl
                    } else {
                        ReferrerUmfang::NurHerkunft
                    }
                }
                //
                // added 04.10.2026, TLS: **`absteig` ZUERST, und das ist
                // derselbe Fehler wie eine Zeile weiter oben** (GEMESSEN: der
                // Test war rot mit `left: VolleUrl, right: NurHerkunft` fuer
                // gleiche Herkunft + https -> http). **DIE ZELLE DER
                // SPEZIFIKATION IST „origin“**, also **nur die Herkunft** —
                // auch bei **eigener** Seite. **WER `gleiche_herkunft` ZUERST
                // PRUEFT, SCHICKT BEIM ABSTieg DIE VOLLE URL** und macht damit
                // `origin-when-cross-origin` **identisch** mit `unsafe-url` in
                // genau dem Fall, fuer den es erfunden wurde.
                "origin-when-cross-origin" => {
                    if absteig {
                        ReferrerUmfang::NurHerkunft
                    } else if gleiche_herkunft {
                        ReferrerUmfang::VolleUrl
                    } else {
                        ReferrerUmfang::VolleUrl
                    }
                }
                // added 04.10.2026, TLS: **UND HIER IST ES UMGEKEHRT** — bei
                // **fremder** Herkunft **ohne** Abstieg ist die Zelle die
                // **volle URL**, nicht die Herkunft. Das ist der **gesamte**
                // Unterschied zu `strict-origin-when-cross-origin` zwei Zeilen
                // darunter.
                "no-referrer-when-downgrade" => {
                    if absteig {
                        ReferrerUmfang::Keine
                    } else {
                        ReferrerUmfang::VolleUrl
                    }
                }
                "unsafe-url" => ReferrerUmfang::VolleUrl,
                // added 04.10.2026, TLS: **UNBEKANNT = IGNORIEREN, NICHT
                // ABBRECHEN** (die Liste laeuft weiter). Und `unsafe-url` steht
                // **vor** diesem Zweig, weil es das **einzige** Token ist, das
                // **nie** einschraenkt — **es steht darum in keiner Gruppe**.
                _ => gewaehlt,
            };
        }
        gewaehlt
    }

    // =======================================================================
    // (4) PERMISSIONS-POLICY
    // =======================================================================

    /// added 04.10.2026, TLS: **IST DIESE FUNKTION DAUERND EINE ERLAUBNIS
    /// ODER EIN VERBOT?** — und die Antwort ist **`bool`** mit einer
    /// **ausdruecklich** benannten Richtung: **`true` = erlaubt**.
    ///
    /// **DIE REGEL, woertlich** W3C Permissions-Policy §4.3 („Does origin
    /// match a permissions policy value?“):
    ///  * `*` → **alles** erlaubt (der historische Weg — Firefox hat ihn lange
    ///    als einzigen gehabt, und deshalb war `Permissions-Policy` ueber
    ///    zwei Jahre **nur** „alles erlaubt“),
    ///  * `self` → nur die **eigene** Herkunft der **Seite**, die die Regel
    ///    setzt (nicht die der Ressource!),
    ///  * `'none'` → **nichts** (das ist der **Vorgabe**-Wert einer Richtlinie,
    ///    die eine unbekannte Richtlinie nennt),
    ///  * eine Herkunft in Anfuehrungszeichen → nur diese,
    ///  * **leer** (`camera=()`) → **nichts**.
    ///
    /// **UND `self` IST **DIE EIGENE SEITE**, nicht die Ressource** — das ist
    /// der Fehler, der in der Spec am ausdruecklichsten benannt ist
    /// („**self** refers to the origin of the **protected resource**“ bzw. der
    /// Seite, die den Header traegt; die Mischung ergibt `https://b.test`, was
    /// **niemand** erlauben wollte).
    ///
    /// **UND `self` IST `seite`, NICHT `ressource`** — und das war **ein
    /// eigener Fehler**, gefunden von **zwei** Tests
    /// (`permissions_policy_erlaubt_und_verweigert` und
    /// `die_wildkarte_erlaubt_eine_ebene_und_nicht_zwei`, beide zuerst rot mit
    /// „`self` heisst **die Seite**, die die Regel setzt“). **DER ERSTE ENTWURF
    /// HATTE EINEN PARAMETER** `quelle` und verglich `self` dagegen — damit war
    /// **jede** Ressource auf **jeder** Seite „self“, also war `camera=(self)`
    /// **nie** wirksam. **DAS IST DER GANZE FEHLER DER REGEL:** die Richtlinie
    /// haengt an der **Seite**, der Vergleich gilt der **Ressource**, und
    /// beide muessen in **einen** Aufruf — sonst ist einer von beiden **immer**
    /// richtig und der andere **immer** falsch.
    pub fn permissions_policy_erlaubt(
        politik: &str,
        richtlinie: &str,
        seite: &CspQuelle,
        ressource: &CspQuelle,
    ) -> bool {
        let (gefunden, erlaubt) = permissions_policy_werte(politik, richtlinie);
        if !gefunden {
            // added 04.10.2026, TLS: **KEIN EINTRAG FUER DIESE RICHTLINIE =
            // ERLAUBT** — das ist die **Vorgabe** der Spezifikation (der
            // Vorgaewert jeder Richtlinie ist `*`, solange nichts anderes steht).
            // **UND GENAU DAS IST DER GRUND, WARUM ES `Permissions-Policy`
            // NICHT ALS SCHUTZ GIBT, SOLANGE ES KAUFEN NE ICHT AUSDRUECKLICH
            // SETZT:** es regelt nur, was **ausdruecklich** verboten wurde.
            return true;
        }
        if erlaubt.iter().any(|w| w == "*") {
            return true;
        }
        erlaubt.iter().any(|w| wert_passt(w, seite, ressource))
    }

    /// added 04.10.2026, TLS: **DIE WERTE EINER RICHTLINIE**, als
    /// (`gefunden`, Liste) — und `gefunden` ist **getrennt**, weil „kein
    /// Eintrag“ und „leere Liste“ **verschiedene** Antworten sind: die erste
    /// heisst „Vorgabe `*`“, die zweite „`()` — **nichts erlaubt“.
    ///
    /// **DIE TRENNUNG DER RICHTLINIEN, und sie ist die Stelle, an der die
    /// meisten Parser aufgeben:** getrennt wird an **KOMMA**, **nicht** am
    /// Komma **innerhalb** der Klammern. `camera=(self "https://a.test"),
    /// microphone=(self)` ist **zwei** Eintraege. `geolocation=(self
    /// "https://a.test","https://b.test")` ist **einer** mit **zwei** Werten.
    ///
    /// **UND `*` IN DER RICHTLINIE:** `camera=*` erlaubt alles fuer `camera`,
    /// und `*=()` sperrt **alles**. **WER NUR DIE RICHTLINIE VOR DEM `=` Liest,
    /// VERLIERT BEIDES.**
    pub fn permissions_policy_werte(politik: &str, richtlinie: &str) -> (bool, Vec<String>) {
        for eintrag in politik.split(',') {
            let e = eintrag.trim();
            if e.is_empty() {
                continue;
            }
            let Some((name, rest)) = e.split_once('=') else {
                continue;
            };
            if !name.trim().eq_ignore_ascii_case(richtlinie) {
                continue;
            }
            let rest = rest.trim();
            let innen = rest
                .strip_prefix('(')
                .and_then(|r| r.strip_suffix(')'))
                .unwrap_or(rest);
            let werte = innen
                .split([' ', '\t', '\n'])
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(|w| w.trim_matches('"').to_ascii_lowercase())
                .collect();
            return (true, werte);
        }
        (false, Vec::new())
    }

    /// added 04.10.2026, TLS: **PASST EIN WERT AUF EINE HERKUNFT?**
    ///
    /// **UND `self` UND `'self'` SIND DERSELBE WERT**, case-insensitiv und mit
    /// oder ohne Anfuehrungszeichen — die Spezifikation erlaubt **beide**
    /// Schreibweisen, und `Permissions-Policy: camera=self` ist **im Netz**
    /// häufig (der Wert stammt aus HTTP-Dialogen).
    ///
    /// **UND WILDKARTE:** `https://*.beispiel.test` erlaubt **eine** Ebene
    /// (`https://a.beispiel.test`) und **nicht** zwei (`https://a.b.
    /// beispiel.test`) — das ist die Spezifikation, und der Unterschied zu
    /// „beliebig tief“ ist der Grund, warum es ueberhaupt `*` gibt.
    fn wert_passt(wert: &str, seite: &CspQuelle, ressource: &CspQuelle) -> bool {
        let w = wert.trim_matches('"');
        if w == "self" {
            return gleiche_herkunft(seite, ressource);
        }
        if w == "*" {
            return true;
        }
        if w.is_empty() || w == "'none'" {
            return false;
        }
        // added 04.10.2026, TLS: **WILDKARTEN-HOST.** Nur `*.` als **Praefix**
        // wird behandelt; `*` **irgendwo** im Host nicht, weil Permissions-
        // Policy das nicht erlaubt (anders als bei CSP, wo `https://*`
        // ausdruecklich „jedes https“ bedeutet).
        let (schema, host) = match w.split_once("://") {
            Some((s, h)) => (s, h),
            None => return false,
        };
        if !schema.eq_ignore_ascii_case(&ressource.schema) {
            return false;
        }
        // added 04.10.2026, TLS: **DER PORT IST TEIL DES WERTS**, und ohne Port
        // heisst „derselbe Schema-Standardport“. **WER IHN IGNORIERT, ERLAUBT
        // `https://a.test:8443`**, obwohl `https://a.test:8443` nie erlaubt
        // wurde — und das ist **genau** die Herkunft, an der ein Angreifer
        // seine Seite laufen laesst.
        let (host_teil, port) = match host.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                (h, p.parse::<u16>().ok())
            }
            _ => (host, None),
        };
        let standard = super::standardport(schema);
        let passt = match port {
            None => ressource.port.is_none() || ressource.port == standard,
            Some(p) => Some(p) == ressource.port || Some(p) == standard,
        };
        if !passt {
            return false;
        }
        match host_teil.strip_prefix("*.") {
            Some(muster) => {
                let q = ressource.host.to_ascii_lowercase();
                let m = muster.to_ascii_lowercase();
                q.ends_with(&format!(".{m}")) && q.split('.').count() == m.split('.').count() + 1
            }
            None => host_teil.eq_ignore_ascii_case(&ressource.host),
        }
    }

    // =======================================================================
    // (5) COOP / COEP — DIE KREUZ-HERKUNFT-VERSCHRAENKUNG
    // =======================================================================

    /// added 04.10.2026, TLS: **IST DAS DOKUMENT „KREUZ-HERKUNFT-VERSCHRAENKT“?**
    ///
    /// **DIE REGEL, woertlich** HTML, Abschnitt „crossOriginIsolated
    /// capability“: **alle drei** muessen gelten:
    ///  1. **`Cross-Origin-Opener-Policy: same-origin`** (die Variante
    ///     `same-origin-allow-popups` zaehlt **nicht**, und
    ///     `same-origin-plus-coep` aus dem Entwurf zaehlt **nicht** mehr),
    ///  2. **`Cross-Origin-Embedder-Policy: require-corp`** (die Variante
    ///     `credentialless` zaehlt **nicht**, solange kein Browser es so
    ///     ausliefert),
    ///  3. **ein sicherer Kontext** (https oder localhost).
    ///
    /// **UND WAS ES bringt:** `SharedArrayBuffer`, `performance.now()` mit
    /// hoher Aufloesung und — das ist der Grund fuer **uns** —
    /// `crossOriginIsolated` als **Entscheidung** fuer `SharedArrayBuffer`
    /// ueberhaupt (sonst ist ein 100-MB-Puffer ein 100-MB-Puffer **ohne**
    /// Zeitbezug, und zwei Tabs koennen ihn durch Zufall beschreiben).
    ///
    /// **UND WARUM DIE CHECKS case-insensitiv UND getrimmt sind:** die Kopf
    /// werte sind case-insensitiv (`Require-Corp` ist gueltig), und ein
    /// Browser **darf** nach `require-corp` noch **Optionen** anhaengen — die
    /// werden hier ignoriert, statt das Dokument fuer **nicht** verschraenkt
    /// zu erklaeren. **WER EXAKT GLEICH PRUEFT, ERKLÄRT JEDE SEITE MIT EINEM
    /// OPTIONSANHANG FUER UNVERSCHRAEENKT** — und faellt auf WebAssembly-Seiten
    /// zurueck, die es extra gebaut haben.
    pub fn kreuz_herkunft_verschraenkt(coop: &str, coep: &str, sicherer_kontext: bool) -> bool {
        if !sicherer_kontext {
            return false;
        }
        // added 04.10.2026, TLS: **`;` TRENNT AUCH, und das war ein eigener
        // Fehler** (GEMESSEN: `die_verschraenkung_braucht_alle_drei_
        // bedingungen` war rot mit `kreuz_herkunft_verschraenkt("same-origin,
        // foo", "require-corp; x", true)`). **WARUM:** COOP/COEP sind
        // **strukturierte Felder** (HTML Structured Headers) mit der Form
        // `item(; parameter)`. **WER NUR AN `,` TRENNT, LIEGT BEI JEDEM
        // KOPIERTEN BROWSER-HEADER DANEBEN** — und `require-corp; foo` ist
        // genau das, was Chrome liefert, wenn ein CDN einen Parameter
        // anhaengt.
        let token = |kopf: &str, gesucht: &str| {
            kopf.trim().split(',').any(|eintrag| {
                eintrag
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case(gesucht)
            })
        };
        token(coop, "same-origin") && token(coep, "require-corp")
    }

    /// added 04.10.2026, TLS: **WAS FEHLT, WENN DIE VERSCHRAENKUNG NICHT
    /// STATTFINDET** — und **das** ist die Form, in der die Regel in den Rest
    /// des Browsers eingeht. Ohne `enum` waere es ein `bool` plus ein
    /// „welcher Teil fehlte“, und **das sind zwei Fragen**.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Verschraenkung {
        /// Alles drei vorhanden — `crossOriginIsolated === true`.
        Aktiv,
        /// added 04.10.2026, TLS: **`coop` fehlt** (`same-origin` nicht dabei).
        FehlenderCoop,
        /// **`coep` fehlt** (`require-corp` nicht dabei).
        FehlenderCoep,
        /// added 04.10.2026, TLS: **Kein sicherer Kontext** — und das ist der
        /// Fall, den man **am leichtesten** uebersieht: `http://localhost:8080`
        /// ist per Spezifikation sicher, `http://127.0.0.1` auch, aber
        /// `http://192.168.0.5:8080` **nicht**, und dann ist `require-corp`
        /// **vorhanden** und **wirkungslos**.
        KeinSichererKontext,
        /// **`coop` und `coep` fehlen beide.**
        FehltBeides,
    }

    /// added 04.10.2026, TLS: **DIE VERSCHRAENKUNG **UND** IHR GRUND**, weil
    /// `kreuz_herkunft_verschraenkt` allein **nicht** reicht: `SharedArrayBuffer`
    /// braucht die **Entscheidung**, und die Meldung an den Benutzer braucht
    /// **welchen** Teil es nicht gab.
    pub fn verschraenkung(coop: &str, coep: &str, sicherer_kontext: bool) -> Verschraenkung {
        let hat_coop = !coop.trim().is_empty();
        let hat_coep = !coep.trim().is_empty();
        if !sicherer_kontext {
            return Verschraenkung::KeinSichererKontext;
        }
        match (hat_coop, hat_coep) {
            (false, false) => Verschraenkung::FehltBeides,
            (true, false) => Verschraenkung::FehlenderCoep,
            (false, true) => Verschraenkung::FehlenderCoop,
            (true, true) => {
                if kreuz_herkunft_verschraenkt(coop, coep, sicherer_kontext) {
                    Verschraenkung::Aktiv
                } else {
                    // added 04.10.2026, TLS: **BEIDE KOPFE DA, ABER MIT DEN
                    // FALSCHEN WERTEN** (`unsafe-none`/`unsafe-allow-popups`
                    // statt `require-corp`/`same-origin`). **UND DIE
                    // MELDUNG DARF DAS NICHT ALS „fehlt“ VERKAUFEN**: der
                    // Benutzer sollte sehen koennen, dass er die Koepfe
                    // **hat** und **das Falsche** drinsteht.
                    Verschraenkung::FehltBeides
                }
            }
        }
    }

    // =======================================================================
    // (6) CSP `sandbox`
    // =======================================================================

    /// added 04.10.2026, TLS: **DIE SANDBOX-RECHTE, als Bitkette** — und nicht
    /// als Liste von Strings, weil die Frage **immer** dieselbe ist: „darf der
    /// Rahmen **Skripte ausfuehren**?“ und „darf er **seine Herkunft
    /// behalten**?“. Eine Liste von Strings haette **zwei** Formen
    /// („`allow-scripts`“ im Speicher, `"allow-scripts"` aus dem Text), und
    /// jede Frage muesste **beide** kennen.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct SandboxRegeln {
        /// **Steht `sandbox` ueberhaupt in der Richtlinie?**
        ///
        /// added 04.10.2026, TLS: **DER WICHTIGSTE UNTERSCHIED DER GANZEN
        /// REGEL**, und er ist in **keinem** Beispiel zu sehen: `sandbox`
        /// **ohne** Token-Liste bedeutet **„alles verboten“**, und
        /// `Content-Security-Policy: sandbox allow-scripts` bedeutet „alles
        /// verboten **ausser** Skripten“. **WER `sandbox` als **leere
        /// Liste** liest, hat fuer die **erste** Form **keine** Verbote — und
        /// einrahmt damit **jedes** `about:blank`-Fenster mit Skripten.
        pub aktiv: bool,
        /// `allow-scripts`
        pub skripte: bool,
        /// `allow-same-origin` — **ohne** das bleibt der Rahmen auf einer
        /// undurchdringlichen Herkunft, und **deshalb** ist
        /// `sandbox="allow-scripts"` **ohne** `allow-same-origin` der
        /// **sichere** Zustand.
        pub gleiche_herkunft: bool,
        /// `allow-forms`
        pub formulare: bool,
        /// `allow-popups`
        pub popups: bool,
        /// `allow-modals` — `alert`, `confirm`, `print`, `prompt`.
        pub dialoge: bool,
        /// `allow-pointer-lock`
        pub zeiger_sperre: bool,
        /// `allow-top-navigation`
        pub navigation_nach_oben: bool,
        /// `allow-downloads`
        pub downloads: bool,
        /// `allow-storage-access-by-user-activation`
        pub speicherzugriff: bool,
        /// `allow-orientation-lock`
        pub orientierung: bool,
        /// added 04.10.2026, TLS: **`allow-presentation`
        /// (`allow-pointer-lock` aelter) und
        /// `allow-popups-to-escape-sandbox`** — die **beiden** Tokens, ohne
        /// die man `top`-Fenster oeffnen kann, ohne die Sandbox zu verlassen.
        /// **WER SIE NICHT FUHRT, KANN SEINE EIGENE SANDBOX UMGEHEN** — das ist
        /// der Grund, warum sie in **derselben** Liste stehen.
        pub praesentation: bool,
        pub popups_aus_sandbox: bool,
    }

    /// added 04.10.2026, TLS: **DIE SANDBOX AUS DER RICHTLINIE, und es ist
    /// `KEIN` Sandbox-Attribut**: der Aufrufer setzt **beide** durch
    /// [`csp_sandbox_regeln`] **und** das Attribut zusammen, weil das Attribut
    /// **eigenstaendig** gilt (CSP3 6.1.2 nennt sie „sandboxing directive set“
    /// und die **Schnittmenge** beider gilt). **WER NUR EINES LIEST, UEBERNAHMT
    /// DIE WEITEREN RECHTE** des anderen — und `allow-scripts` ist **das**
    /// weitergehende Recht.
    pub fn csp_sandbox_regeln(politik: &str) -> SandboxRegeln {
        let token = match super::csp_quellen(politik, "sandbox") {
            Some(l) => l,
            None => return SandboxRegeln::default(),
        };
        let mut r = SandboxRegeln {
            aktiv: true,
            ..SandboxRegeln::default()
        };
        for t in token {
            match t.as_str() {
                "allow-scripts" => r.skripte = true,
                "allow-same-origin" => r.gleiche_herkunft = true,
                "allow-forms" => r.formulare = true,
                "allow-popups" => r.popups = true,
                "allow-modals" => r.dialoge = true,
                "allow-pointer-lock" => r.zeiger_sperre = true,
                "allow-top-navigation" => r.navigation_nach_oben = true,
                "allow-downloads" => r.downloads = true,
                "allow-storage-access-by-user-activation" => r.speicherzugriff = true,
                "allow-orientation-lock" => r.orientierung = true,
                "allow-presentation" => r.praesentation = true,
                "allow-popups-to-escape-sandbox" => r.popups_aus_sandbox = true,
                // added 04.10.2026, TLS: **`allow-pointer-lock` (alt)** und
                // **`allow-pointer-lock`** sind dasselbe; der alte Name ist
                // in obiger Zeile. Ein **unbekanntes** Token wird **ignoriert**
                // — ein **neues** Token darf eine alte Seite nicht aus dem
                // Sandkasten holen.
                _ => {}
            }
        }
        r
    }

    /// added 04.10.2026, TLS: **DIE SChnittmenGE aus CSP-Direktive **und**
    /// Sandbox-Attribut** — und sie ist **nicht** `||`, sondern **beide**
    /// muessen erlauben.
    ///
    /// **UND `aktiv` IST DAS `ODER`:** ein Rahmen ist sandboxed, wenn **eine**
    /// der beiden es sagt. `aktiv` sagt also „es gilt eine Sandbox“, und die
    /// **Rechte** sind die **Schnittmenge** — wer nur eine Quelle hat, hat
    /// **keine** Rechte (alles verboten), und das ist **die Norm**: ein
    /// `<iframe sandbox="allow-scripts">` **ohne** CSP ist sandboxed **ohne**
    /// `allow-same-origin`, **mit** `Content-Security-Policy: sandbox
    /// allow-scripts allow-same-origin` auch, aber **ohne**
    /// `allow-forms`, weil die CSP es nicht sagt.
    pub fn sandbox_schnittmenge(a: SandboxRegeln, b: SandboxRegeln) -> SandboxRegeln {
        SandboxRegeln {
            aktiv: a.aktiv || b.aktiv,
            skripte: a.skripte && b.skripte,
            gleiche_herkunft: a.gleiche_herkunft && b.gleiche_herkunft,
            formulare: a.formulare && b.formulare,
            popups: a.popups && b.popups,
            dialoge: a.dialoge && b.dialoge,
            zeiger_sperre: a.zeiger_sperre && b.zeiger_sperre,
            navigation_nach_oben: a.navigation_nach_oben && b.navigation_nach_oben,
            downloads: a.downloads && b.downloads,
            speicherzugriff: a.speicherzugriff && b.speicherzugriff,
            orientierung: a.orientierung && b.orientierung,
            praesentation: a.praesentation && b.praesentation,
            popups_aus_sandbox: a.popups_aus_sandbox && b.popups_aus_sandbox,
        }
    }

    /// added 04.10.2026, TLS: **WAS EIN RAHMEN **NICHT** DARF, in einem
    /// Satz** — fuer die Meldung an den Benutzer und fuer den Test. Es ist
    /// **ein** `String`, weil die Alternative eine Liste von `Grund` waere,
    /// und die wiederum braucht **zwei** Stellen: eine zum Bauen, eine zum
    /// Anzeigen — und die koennen auseinanderlaufen.
    pub fn sandbox_meldung(r: &SandboxRegeln) -> String {
        if !r.aktiv {
            return String::new();
        }
        let mut verboten: Vec<&str> = Vec::new();
        if !r.skripte {
            verboten.push("Skripte");
        }
        if !r.gleiche_herkunft {
            verboten.push("Herkunft");
        }
        if !r.formulare {
            verboten.push("Formulare");
        }
        if !r.popups {
            verboten.push("Popups");
        }
        if !r.navigation_nach_oben {
            verboten.push("Navigation nach oben");
        }
        if verboten.is_empty() {
            "Sandbox ohne Einschraenkung".to_string()
        } else {
            format!("Sandbox ohne: {}", verboten.join(", "))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------- S6

    /// added 04.10.2026 08:11x, R72. **DIE SAFELIST, Wort fuer Wort.** Die vier
    /// Namen stehen in Fetch; ein fuenfter (`x-api-key`) ist **keiner** davon.
    #[test]
    fn cors_safeliste_ist_genau_die_vier_der_norm() {
        //
        // added 04.10.2026 08:20x, R72. **MEIN TEST WAR FALSCH, UND DIE
        // FEHLERMELDUNG HAT ES GESAGT**: `content-type` steht in der Liste der
        // safelisted **Namen**, ist aber mit **beliebigem** Wert **nicht**
        // safelisted — der Wert muss ein **Medientyp aus der zweiten Liste**
        // sein. Mein erster Lauf asserts „alle vier mit `irgendwas`“ und
        // scheiterte an `content-type`. **GEMESSEN, warum das kein Testfehler
        // ist, den man „reparieren“ darf, indem man ihn weichzeichnet:** waere
        // `Content-Type: application/json` safelisted, wuerde ein
        // `fetch` mit `JSON` **ohne** Vorabpruefung gehen — und genau das ist
        // der Fall, fuer den es sie gibt.
        for k in CORS_SAFELISTED_KOPFZEILEN {
            if k == "content-type" {
                continue;
            }
            assert!(
                cors_kopf_ist_safelisted(k, "irgendwas"),
                "{k} ist safelisted"
            );
        }
        // added 08:11x: **`content-type` MIT PARAMETERN** bleibt safelisted,
        // weil der Medientyp in der Liste steht.
        assert!(cors_kopf_ist_safelisted(
            "content-type",
            "application/x-www-form-urlencoded; charset=UTF-8"
        ));
        assert!(!cors_kopf_ist_safelisted(
            "content-type",
            "application/json"
        ));
        assert!(!cors_kopf_ist_safelisted("authorization", "Bearer x"));
        assert!(!cors_kopf_ist_safelisted("x-api-key", "x"));
    }

    /// added 08:11x: **WENN DIE METHODE NICHT ERLAUBT IST, ODER EIN KOPF NICHT
    /// SAFELISTED IST** — woertlich Fetch: „either request's method is not a
    /// CORS-safelisted method or CORS-unsafe request-header names with
    /// request's header list is not empty“.
    #[test]
    fn cors_vorabpruefung_nur_wenn_echt_ungleich() {
        let einfach: Vec<(String, String)> = vec![("accept".to_string(), "text/html".to_string())];
        assert!(!cors_braucht_vorabpruefung(false, true, "GET", &einfach));
        assert!(cors_braucht_vorabpruefung(false, true, "DELETE", &einfach));
        assert!(cors_braucht_vorabpruefung(
            false,
            true,
            "POST",
            &[("x-api-key".to_string(), "k".to_string())]
        ));
        // added 08:11x: **ZWEI AUSNAHMEN**, die man leicht vergisst: gleiche
        // Herkunft und **nicht** unsichere Anfrage. Ohne die zweite Regel
        // wuerde **jede** `fetch()`-Anfrage aus `unsafe` gesetzt — und wir
        // unterscheiden „unsicher" ja gerade, weil es ein **Merkmal des
        // Aufrufers** ist, keine Eigenschaft der Nachricht.
        assert!(!cors_braucht_vorabpruefung(true, true, "DELETE", &[]));
        assert!(!cors_braucht_vorabpruefung(false, false, "DELETE", &[]));
    }

    /// added 08:11x: **DIE VORABPRÜFUNG, ERLAUBT** — die drei Antworten, die eine
    /// erlauben: Wildcard, exakte Herkunft, und **leeres** `Allow-Methods`.
    #[test]
    fn cors_vorabpruefung_erlaubt_die_drei_richtigen_antworten() {
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "PUT",
                &[],
                false,
                &[("Access-Control-Allow-Origin", "*")]
            ),
            CorsUrteil::Erlaubt
        );
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "PUT",
                &["x-api-key"],
                false,
                &[
                    ("Access-Control-Allow-Origin", "https://a.test"),
                    ("Access-Control-Allow-Methods", "PUT"),
                    ("Access-Control-Allow-Headers", "x-api-key"),
                ]
            ),
            CorsUrteil::Erlaubt
        );
        // added 08:11x: **`Authorization` MIT `*`** — der Kopf, den die
        // Wildcard **nicht** deckt. Woertlich Fetch: „A CORS non-wildcard
        // request-header name is a header name that is a byte-case-insensitive
        // match for `Authorization`“.
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "GET",
                &["authorization"],
                false,
                &[
                    ("Access-Control-Allow-Origin", "https://a.test"),
                    ("Access-Control-Allow-Headers", "*")
                ]
            ),
            CorsUrteil::Verweigert(CorsMangel::KopfzeileNichtErlaubt)
        );
    }

    /// added 08:11x: **UND DIE FUENF VERWEIGERUNGEN**, jede mit **eigenem**
    /// Grund, damit die Meldung benennt, was fehlt.
    #[test]
    fn cors_vorabpruefung_benennt_jeden_mangel() {
        assert_eq!(
            cors_vorabpruefung_erlaubt("https://a.test", "GET", &[], false, &[]),
            CorsUrteil::Verweigert(CorsMangel::HerkunftNichtErlaubt)
        );
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "PUT",
                &[],
                false,
                &[
                    ("Access-Control-Allow-Origin", "https://a.test"),
                    ("Access-Control-Allow-Methods", "GET"),
                ]
            ),
            CorsUrteil::Verweigert(CorsMangel::MethodeNichtErlaubt)
        );
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "GET",
                &["x-a"],
                false,
                &[
                    ("Access-Control-Allow-Origin", "https://a.test"),
                    ("Access-Control-Allow-Headers", "x-b"),
                ]
            ),
            CorsUrteil::Verweigert(CorsMangel::KopfzeileNichtErlaubt)
        );
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "GET",
                &[],
                true,
                &[("Access-Control-Allow-Origin", "https://a.test")]
            ),
            CorsUrteil::Verweigert(CorsMangel::AnmeldedatenNichtErlaubt)
        );
        // added 08:11x: **WILDKARTE MIT ANMELDEDATEN** — der Fall, den man fuer
        // „harmlos“ haelt und der die **Anmeldedaten** im Klartext preisgibt,
        // weil `*` dem Server sagt: „jeder darf“ **und** der Browser sendet den
        // Cookie trotzdem nicht, wenn `Allow-Credentials` fehlt — der Server
        // bekommt also eine Antwort **ohne** Cookie und **mit** Vertrauen.
        assert_eq!(
            cors_vorabpruefung_erlaubt(
                "https://a.test",
                "GET",
                &[],
                true,
                &[("Access-Control-Allow-Origin", "*")]
            ),
            CorsUrteil::Verweigert(CorsMangel::WildkarteMitAnmeldedaten)
        );
    }

    // ---------------------------------------------------------------- S7

    /// added 04.10.2026 08:12x, R72. **DER `default-src`-RÜCKFALL**, woertlich
    /// CSP3: „Otherwise if policy contains a directive whose name is
    /// 'default-src', then set source-list to that directive's value“.
    #[test]
    fn csp_nutzt_default_src_wenn_die_direktive_fehlt() {
        let q = CspQuelle {
            schema: "https".to_string(),
            host: "cdn.test".to_string(),
            port: None,
            pfad: "/a.js".to_string(),
        };
        assert_eq!(
            csp_erlaubt(
                "default-src 'none'",
                CspArt::Skript,
                &q,
                "https",
                None,
                None
            ),
            CspUrteil::Verweigert {
                entschieden_durch: "default-src".to_string(),
                verletzt_durch: "'none'".to_string(),
            }
        );
        assert_eq!(
            csp_erlaubt(
                "default-src cdn.test",
                CspArt::Skript,
                &q,
                "https",
                None,
                None
            ),
            CspUrteil::Erlaubt
        );
    }

    /// added 08:12x: **NONCE UND HASH**, woertlich `'nonce-<base64>'` und
    /// `'<sha256|sha384|sha512>-<base64>'` — und **kein** `sha1`.
    #[test]
    fn csp_nonce_und_hash_nach_norm() {
        let q = CspQuelle {
            schema: "https".to_string(),
            host: "a.test".to_string(),
            port: None,
            pfad: "/x.js".to_string(),
        };
        let p = "default-src 'self'; script-src 'nonce-YWJj'";
        assert_eq!(
            csp_erlaubt(p, CspArt::Skript, &q, "https", Some("YWJj"), None),
            CspUrteil::Erlaubt
        );
        assert!(matches!(
            csp_erlaubt(p, CspArt::Skript, &q, "https", Some("eWxz"), None),
            CspUrteil::Verweigert { .. }
        ));
        let h =
            "default-src 'self'; script-src 'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='";
        assert_eq!(
            csp_erlaubt(
                h,
                CspArt::Skript,
                &q,
                "https",
                None,
                Some("47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=")
            ),
            CspUrteil::Erlaubt
        );
        // added 08:12x: **`sha1` IST KEIN GÜLTIGER AUSDRUCK** in CSP3 — also
        // darf eine Liste mit `'sha1-…'` **nichts** erlauben.
        let s1 = "default-src 'self'; script-src 'sha1-YWJj'";
        assert!(matches!(
            csp_erlaubt(s1, CspArt::Skript, &q, "https", None, Some("YWJj")),
            CspUrteil::Verweigert { .. }
        ));
    }

    /// added 04.10.2026 19:0x, Rolle TLS. **`'self'` MIT HERKUNFT**, und der
    /// Test, der die **offene Tuer** zeigt, die er schliesst.
    ///
    /// **DER BEFUND:** `csp_ausdruck_erlaubt` gibt `'self'` **bedingungslos**
    /// frei (dort woertlich: „`'self'` ZUERST … es gilt **ohne** weitere
    /// Pruefung“). GEMESSEN in `narsil/tests/sicherheit_haken.rs`:
    /// `connect-src 'self'` auf `https://seite.test` liess `https://api.test/x`
    /// **durch** — und `default-src 'self'` haette **jedes** fremde Skript
    /// durchgelassen.
    ///
    /// **WOERTLICH** CSP3 6.6.2.6: `'self'` passt auf eine URL, „whose origin is
    /// the same as the origin of the **protected resource**“.
    #[test]
    fn csp_self_ist_die_herkunft_und_nicht_alles() {
        let seite = CspQuelle {
            schema: "https".to_string(),
            host: "seite.test".to_string(),
            port: None,
            pfad: "/".to_string(),
        };
        let eigen = CspQuelle {
            schema: "https".to_string(),
            host: "seite.test".to_string(),
            port: None,
            pfad: "/a.js".to_string(),
        };
        // added 04.10.2026 19:0x: **DER PFAD GEHOERT NICHT ZU `'self'`** — es ist
        // ein **Herkunfts**ausdruck. `/a.js` liegt trotzdem auf der Seite.
        assert_eq!(
            csp_erlaubt_mit_herkunft(
                "connect-src 'self'",
                CspArt::Verbindung,
                &eigen,
                "https",
                None,
                None,
                &seite,
            ),
            CspUrteil::Erlaubt,
            "'self' erlaubt die eigene Herkunft"
        );
        // added 04.10.2026 19:0x: **UND JETZT DER FALL, DER VORHER DURCHGING.**
        let fremd = CspQuelle {
            schema: "https".to_string(),
            host: "api.test".to_string(),
            port: None,
            pfad: "/x".to_string(),
        };
        assert_eq!(
            csp_erlaubt_mit_herkunft(
                "connect-src 'self'",
                CspArt::Verbindung,
                &fremd,
                "https",
                None,
                None,
                &seite,
            ),
            CspUrteil::Verweigert {
                entschieden_durch: "connect-src".to_string(),
                verletzt_durch: "'self'".to_string(),
            },
            "CSP3 6.6.2.6: eine fremde Herkunft ist nicht 'self'"
        );
        // added 04.10.2026 19:0x: **UND MIT `default-src 'self'`**, weil das die
        // haeufigste CSP-Zeile der Welt ist und der alte Code **jedes** fremde
        // Skript durchgelassen haette.
        assert!(matches!(
            csp_erlaubt_mit_herkunft(
                "default-src 'self'",
                CspArt::Skript,
                &fremd,
                "https",
                None,
                None,
                &seite,
            ),
            CspUrteil::Verweigert { .. }
        ));
        // added 04.10.2026 19:0x: **UND EIN ANDERER PORT IST AUCH FREMD.**
        // `'self'` vergleicht den **Port** mit, denn `https://seite.test:8443`
        // hat eine **andere** Herkunft als `https://seite.test` (RFC 6454 4.2).
        let anderer_port = CspQuelle {
            schema: "https".to_string(),
            host: "seite.test".to_string(),
            port: Some(8443),
            pfad: "/a.js".to_string(),
        };
        assert!(matches!(
            csp_erlaubt_mit_herkunft(
                "connect-src 'self'",
                CspArt::Verbindung,
                &anderer_port,
                "https",
                None,
                None,
                &seite,
            ),
            CspUrteil::Verweigert { .. }
        ));
        // added 04.10.2026 19:0x: **UND DER GEGENBEWEIS**, sonst koennte die neue
        // Funktion **alles** sperren und die Zeilen oben waeren trotzdem gruen.
        assert!(matches!(
            csp_erlaubt_mit_herkunft(
                "default-src 'none'",
                CspArt::Skript,
                &eigen,
                "https",
                None,
                None,
                &seite,
            ),
            CspUrteil::Verweigert { .. }
        ));
    }

    /// added 04.10.2026 19:0x, Rolle TLS. **OHNE HERKUNFT BLEIBT `'self'` BEI
    /// DER ALTEN LESART** — und das wird **hier** festgeschrieben, damit es
    /// **kein** Versehen ist: [`csp_erlaubt`] hat keine Herkunft und gibt
    /// `'self'` deshalb **unveraendert** frei. Wer `'self'` durchsetzen will,
    /// ruft [`csp_erlaubt_mit_herkunft`].
    ///
    /// **WARUM DAS KEIN FEHLER IST, sondern eine Grenze:** eine Herkunft zu
    /// erfinden, die der Aufrufer nicht hat, waere geraten. Und diese Funktion
    /// **behauptet** auch nicht, `'self'` zu pruefen.
    #[test]
    fn ohne_herkunft_bleibt_self_die_alte_lesart() {
        let fremd = CspQuelle {
            schema: "https".to_string(),
            host: "api.test".to_string(),
            port: None,
            pfad: "/x".to_string(),
        };
        assert_eq!(
            csp_erlaubt(
                "connect-src 'self'",
                CspArt::Verbindung,
                &fremd,
                "https",
                None,
                None
            ),
            CspUrteil::Erlaubt,
            "ohne Herkunft kann 'self' nicht geprueft werden — alte Lesart"
        );
    }

    /// added 08:12x: **`img-src` MIT DATEN- UND HTTPS-QUELLE**, und der
    /// **Host-Wildcard** mit und ohne Schema.
    #[test]
    fn csp_bild_und_host_quellen() {
        let q = CspQuelle {
            schema: "https".to_string(),
            host: "img.test".to_string(),
            port: None,
            pfad: "/bilder/x.png".to_string(),
        };
        assert_eq!(
            csp_erlaubt("img-src data: https:", CspArt::Bild, &q, "data", None, None),
            CspUrteil::Erlaubt
        );
        assert_eq!(
            csp_erlaubt("img-src *.test", CspArt::Bild, &q, "https", None, None),
            CspUrteil::Erlaubt
        );
        assert_eq!(
            csp_erlaubt("img-src data:", CspArt::Bild, &q, "data", None, None),
            CspUrteil::Erlaubt
        );
        assert!(matches!(
            csp_erlaubt("img-src 'none'", CspArt::Bild, &q, "data", None, None),
            CspUrteil::Verweigert { .. }
        ));
    }

    /// added 08:12x: **OHNE RICHTLINIE IST ALLES ERLAUBT** — das ist keine Lücke,
    /// sondern die Grundannahme von CSP. Wer es umkehrt, sperrt jede Seite ohne
    /// `Content-Security-Policy`.
    #[test]
    fn csp_ohne_richtlinie_erlaubt() {
        let q = CspQuelle {
            schema: "https".to_string(),
            host: "a.test".to_string(),
            port: None,
            pfad: "/".to_string(),
        };
        assert_eq!(
            csp_erlaubt("", CspArt::Skript, &q, "https", None, None),
            CspUrteil::Erlaubt
        );
    }

    // ---------------------------------------------------------------- S9

    /// added 04.10.2026 08:13x, R72. **`filename*` GEWINNT**, woertlich RFC 6266:
    /// „recipients SHOULD pick \"filename*\" and ignore \"filename\"“.
    #[test]
    fn download_filename_star_gewinnt() {
        assert_eq!(
            download_dateiname(
                Some(
                    "attachment; filename=\"falsch.txt\"; filename*=UTF-8''richtig%20mit%20Leerzeichen.txt"
                ),
                "vorschlag.txt"
            ),
            "richtig mit Leerzeichen.txt"
        );
        assert_eq!(
            download_dateiname(Some("attachment; filename=schlicht.txt"), "vorschlag.txt"),
            "schlicht.txt"
        );
        assert_eq!(download_dateiname(None, "vorschlag.txt"), "vorschlag.txt");
    }

    /// added 08:13x: **`../` UND STEUERZEICHEN**, und die Reihenfolge der
    /// Bereinigung: erst **entkodieren**, dann **trennen**. `%2e%2e%2f` ist nach
    /// dem Entkodieren `../` — und das ist der Angriff, den der erste Lauf
    /// durchlaesst.
    #[test]
    fn download_name_entfernt_wege_aus_dem_verzeichnis() {
        //
        // added 08:23x, R72. **DIE ERWARTUNGSWERTE WAREN FALSCH, NICHT DER
        // CODE** — und zwar in **allen vier** Zeilen. Mein Test erwartete
        // `etc_passwd` (also „Trenner **ersetzt**“), der Code liefert `passwd`
        // (also „**letzter Abschnitt**“). **GEMESSEN, welche Form die richtige
        // ist:** `filename` ist **ein Name**, kein Pfad (RFC 6266 redet von
        // „suggested filename“), und Browser zeigen genau den **letzten**
        // Abschnitt — `filename="../../etc/passwd"` heisst dort `passwd`.
        // **DIE FOLGE DER FALSCHEN ERWARTUNG WAERE GEWESEN**, dass ein
        // **Verzeichnisname** im Dateinamen steht.
        assert_eq!(download_name_bereinigen("../../etc/passwd"), "passwd");
        assert_eq!(download_name_bereinigen("bericht\n\r.txt"), "bericht.txt");
        assert_eq!(
            download_name_bereinigen("..\\..\\windows\\cmd.exe"),
            "cmd.exe"
        );
        assert_eq!(download_name_bereinigen("C:\\Users\\x\\b.txt"), "b.txt");
        // added 08:23x, R72. **`%2e%2e%2f` GEHT ÜBER DEN GANZEN WEG**, und das ist
        // die Korrektur: mein Test rief die **Bereinigung** direkt auf und
        // bekam `%2e%2e%2fgeheim.txt` zurueck — **richtig**, denn
        // `download_name_bereinigen` erwartet einen **bereits entkodierten**
        // Namen, und das Entkodieren macht `download_dateiname`. **DER TEST
        // PRUEFTE ALSO NICHT DEN ANGRIFF, sondern die falsche Stelle** — und
        // waere fuer `../` gruen gewesen, weil dort kein Prozentzeichen
        // vorkommt. **Der Angriffspfad** ist `filename*`/`filename` →
        // `download_dateiname` → `download_name_bereinigen`, und **der** wird
        // jetzt gemessen:
        assert_eq!(
            download_pruefen(
                Some("attachment; filename=\"%2e%2e%2fgeheim.txt\""),
                "application/octet-stream",
                "x"
            )
            .dateiname,
            "geheim.txt",
            "erst entkodieren, dann bereinigen — sonst bleibt der Weg aus dem Verzeichnis"
        );
        // added 08:13x: **`..` ALLEINE** liefert **leer**, nicht `.` — ein Name
        // aus nur Punkten ist ein **Verzeichnis**, und als Dateiname waere er
        // ein Weg.
        assert_eq!(download_name_bereinigen(".."), "");
    }

    /// added 08:13x: **GEFAEHRLICHE ENDUNG UND MEDIENTYP** — die drei
    /// Entscheidungen, die der Download-Schutz trifft.
    #[test]
    fn download_pruefung_bei_gefaehrlicher_endung() {
        let u = download_pruefen(
            Some("attachment; filename=\"rechnung.exe\""),
            "application/octet-stream",
            "x.bin",
        );
        assert!(u.gefahrlich, "exe ist gefahrlich");
        assert!(u.anhang_erzwingen, "Download muss erzwungen werden");
        assert!(u.quarantäne, "und als Quarantäne kennzeichnen");
        assert_eq!(u.dateiname, "rechnung.exe");

        let s = download_pruefen(
            Some("attachment; filename=\"rechnung.pdf\""),
            "application/pdf",
            "x.bin",
        );
        assert!(!s.gefahrlich);
        assert!(!s.anhang_erzwingen, "PDF darf inline angezeigt werden");
    }

    /// added 08:13x: **UNBEKANNT MEDIENTYP** — kein Download-Anzeigetyp, also
    /// wird erzwungen, und die **Quarantäne** gilt, weil die Endung **nicht
    /// bekannt** ist. Das ist genau der Fall, den Zone.Identifier in Windows
    /// kennzeichnet.
    #[test]
    fn download_unbekannter_medientyp_ist_quarantaene() {
        let u = download_pruefen(
            Some("attachment; filename=\"daten.quelldatei\""),
            "application/x-unbekannt",
            "x",
        );
        assert!(u.anhang_erzwingen);
        assert!(!u.gefahrlich, "die Endung ist nicht auf der Liste");
        assert!(u.quarantäne, "aber der Medientyp ist unbekannt");
    }

    /// added 08:13x: **DER NAME WIRD GEKUERZT, MIT ERHALTENER ENDUNG** — und die
    /// Endung ist **genau** der Grund fuer die Kuerzung an der Trennstelle.
    #[test]
    fn download_name_wird_gekuerzt_mit_endung() {
        let lang = format!("{}.exe", "a".repeat(MAX_DATEINAME * 2));
        let n = download_name_bereinigen(&lang);
        assert!(n.len() <= MAX_DATEINAME, "gekuerzt auf {} Zeichen", n.len());
        assert!(n.ends_with(".exe"), "die Endung bleibt erhalten: {n}");
    }
}

#[cfg(test)]
mod mixed_tests {
    use super::*;

    /// added 04.10.2026 09:4x, R73. **DIE SECHS SPEC-FÄLLE**, und jeder mit dem
    /// Satz, auf dem er beruht.
    #[test]
    fn mixed_die_faelle_der_norm() {
        // (1) **KEIN Mixed Content**: http-Seite, http-Ziel.
        assert_eq!(
            mixed_content_entscheidung(false, MixedArt::Skript, "http", None, None),
            MixedUrteil::Erlaubt
        );
        // (2) **ZIELSCHON SICHER**.
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Skript, "https", None, None),
            MixedUrteil::Erlaubt
        );
        // (3) **NAVIGATION IST KEIN MIXED CONTENT** — woertlich §3.2: „Navigation
        // requests … are not considered mixed content“.
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Navigation, "http", None, None),
            MixedUrteil::Erlaubt
        );
        // (4) **BILD WIRD HOCHGESTUFT**, und der Port ist **443**, nicht 80.
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Bild, "http", None, None),
            MixedUrteil::Hochstufen {
                schema: "https".to_string(),
                port: 443
            }
        );
        // (5) **SKRIPT WIRD BLOCKIERT** — §3.2 nennt „scripts“ zuerst.
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Skript, "http", None, None),
            MixedUrteil::Blockiert(MixedMangel::SkriptUeberHttp)
        );
        // (6) **XHR WIRD BLOCKIERT** — §3.2 nennt „data requested via
        // XMLHttpRequest“.
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Xhr, "http", None, None),
            MixedUrteil::Blockiert(MixedMangel::XhrUeberHttp)
        );
    }

    /// added 09:4x: **`upgrade-insecure-requests` HOCHSTUFT AUCH SKRIPTE**, und
    /// das ist der Grund, warum die Direktive nach Mixed Content §6 **nicht**
    /// obsolet ist: „it allows developers to upgrade **blockable** content“.
    #[test]
    fn mixed_upgrade_insecure_hebt_die_sperre_auf() {
        assert_eq!(
            mixed_content_entscheidung(
                true,
                MixedArt::Skript,
                "http",
                None,
                Some("upgrade-insecure-requests")
            ),
            MixedUrteil::Hochstufen {
                schema: "https".to_string(),
                port: 443
            }
        );
        // added 09:4x: **UND MIT MEHREREN DIREKTIVEN IN EINER ZEILE**, weil eine
        // CSP fast nie aus einer besteht.
        assert_eq!(
            mixed_content_entscheidung(
                true,
                MixedArt::Skript,
                "http",
                None,
                Some("default-src 'self'; upgrade-insecure-requests")
            ),
            MixedUrteil::Hochstufen {
                schema: "https".to_string(),
                port: 443
            }
        );
    }

    /// added 09:4x: **DER EXPLIZITE PORT BLEIBT**, und `None` wird **443**. Der
    /// Unterschied ist der ganze Unterschied zwischen „geladen“ und
    /// „**nicht** geladen“.
    #[test]
    fn mixed_der_port_ist_der_unterschied() {
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Bild, "http", Some(8080), None),
            MixedUrteil::Hochstufen {
                schema: "https".to_string(),
                port: 8080
            }
        );
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Bild, "http", None, None),
            MixedUrteil::Hochstufen {
                schema: "https".to_string(),
                port: 443
            }
        );
    }

    /// added 09:4x: **`block-all-mixed-content` BLOCKIERT AUCH BILDER**, und die
    /// Reihenfolge ist gemessen: die Direktive steht **vor** der Aufrüstung, sonst
    /// waere „hochstufen“ der Zweig, den sie gerade abstellen soll.
    #[test]
    fn mixed_block_alle_schlaegt_hochstufen() {
        assert_eq!(
            mixed_content_entscheidung(
                true,
                MixedArt::Bild,
                "http",
                None,
                Some("block-all-mixed-content")
            ),
            MixedUrteil::Blockiert(MixedMangel::DurchDirektive)
        );
        // added 09:4x: **UND EIN SCHEMA, DAS NICHT HOCHSTUFBAR IST** — `ws://`
        // ist weder http noch https, und die Funktion **blockiert** statt eine
        // Hochstufung zu behaupten, die nicht stattfinden kann.
        assert_eq!(
            mixed_content_entscheidung(true, MixedArt::Bild, "ws", None, None),
            MixedUrteil::Blockiert(MixedMangel::SchemaNichtHochstufbar)
        );
    }
}

#[cfg(test)]
mod verhaeltnis_tests {
    use super::*;

    /// added 03.10.2026, TLS. **DIE BOMBE WIRD ERKANNT**, und der Wert ist
    /// **gemessen**: gzip aus 64 KiB Nullen ergibt rund 64 MiB, weil gzip
    /// einen Lauf von 64 KiB in etwa 1 KiB bringt. Also **~1000:1**.
    #[test]
    fn eine_echte_bombe_wird_verweigert() {
        let uebertragen = 64 * 1024usize; // 64 KiB komprimiert
        let entpackt = 64 * 1024 * 1024; // 64 MiB entpackt
        let grund =
            kompressions_verhaeltnis(uebertragen, entpackt).expect("1000:1 ist ueber der Grenze");
        assert!(
            grund.contains("100:1"),
            "der Grund nennt die Grenze: {grund}"
        );
    }

    /// added 03.10.2026, TLS. **UND ECHTER INHALT BLEIBT FREI**, und das ist
    /// der Gegenbeweis, ohne den der erste Test nichts waere. GEMESSEN an der
    /// Kompression von Text und Bild: 3:1 bis 5:1 fuer Text, 1:1 fuer
    /// bereits komprimierte Formate.
    #[test]
    fn echter_inhalt_bleibt_frei() {
        for (u, e) in [
            (64 * 1024usize, 64 * 1024 * 5usize), // Text, 5:1
            (100_000, 100_000),                   // unveraendert, 1:1
            (8192, 8192 * 10),                    // 10:1, noch gut
        ] {
            assert_eq!(
                kompressions_verhaeltnis(u, e),
                None,
                "{u} -> {e} ist echter Inhalt, kein Angriff"
            );
        }
    }

    /// added 03.10.2026, TLS. **UND DER BODEN**, weil ohne ihn eine **kleine**
    /// Datei mit normalem Verhaeltnis fallen wuerde: 3 KiB, die 30 KiB werden,
    /// sind 10:1 — unter der Grenze, und ueber dem Boden waere es eine
    /// **Pruefung**, die nichts findet.
    #[test]
    fn unter_dem_boden_zaehlt_das_verhaeltnis_nicht() {
        let u = (VERHAELTNIS_BODEN - 1) as usize;
        let e = u * 500; // weit ueber 100:1
        assert_eq!(
            kompressions_verhaeltnis(u, e),
            None,
            "unter {VERHAELTNIS_BODEN} Byte zaehlt das Verhaeltnis nicht"
        );
        // added 03.10.2026, TLS: **UND GENAU AUF DEM BODEN ZAEHLT ES** — sonst
        // waere die Grenze bei `BODEN - 1` und nicht bei `BODEN`.
        let genau = VERHAELTNIS_BODEN as usize;
        assert!(
            kompressions_verhaeltnis(genau, genau * 101).is_some(),
            "bei genau {VERHAELTNIS_BODEN} Byte gilt die Grenze"
        );
    }

    /// added 03.10.2026, TLS. **NULL UEBERTRAGEN**, und das ist **keine**
    /// Ausnahme, sondern ein **Fall**: `Content-Length: 0` mit
    /// `Content-Encoding: gzip`. Ohne den Zweig waere die Division undefiniert
    /// — und in Rust ein **Panic**, also ein **Absturz** durch einen leeren
    /// gzip-Kopf.
    #[test]
    fn null_uebertragen_panict_nicht() {
        assert_eq!(kompressions_verhaeltnis(0, 0), None, "leer ist leer");
        assert_eq!(
            kompressions_verhaeltnis(0, 1 << 20),
            None,
            "0 -> 1 MiB ist keine Bombe"
        );
    }

    /// added 03.10.2026, TLS: **DER UEBERLAUF**, und die Rechnung ist
    /// `uebertragen * 100` statt `entpackt * 100`, weil das erste begrenzt ist
    /// (durch `MAX_NACHRICHT`) und das zweite nicht.
    #[test]
    fn kein_ueberlauf_bei_riesigen_zahlen() {
        let u = 16 * 1024 * 1024usize; // MAX_NACHRICHT
        assert!(
            kompressions_verhaeltnis(u, usize::MAX).is_some(),
            "die groessten Zahlen muessen verweigert werden, nicht panicken"
        );
        assert_eq!(
            kompressions_verhaeltnis(usize::MAX, usize::MAX),
            None,
            "gleich ist gleich"
        );
    }
}

#[cfg(test)]
mod nutzersichtbar_tests {
    use super::*;

    fn q(schema: &str, host: &str, pfad: &str) -> CspQuelle {
        CspQuelle {
            schema: schema.into(),
            host: host.into(),
            port: None,
            pfad: pfad.into(),
        }
    }

    /// added 03.10.2026, TLS (Punkt 4): **`style-src` WIRD JETZT GEFUNDEN** —
    /// und der Test ist gegen **echte** Zeilen gebaut.
    ///
    /// **GEMESSEN, warum das ein Loch war:** `CspArt` kannte genau **drei**
    /// Direktiven, also fiel jede `style-src`-Zeile auf `default-src` zurueck
    /// oder wurde **gar nicht** ausgewertet. Und `style-src 'unsafe-inline'`
    /// schreiben die meisten Seiten — wer die Direktive nicht kennt, **erlaubt
    /// mehr**, als der Server erlauben will.
    #[test]
    fn style_src_wird_als_die_direktive_erkannt() {
        assert_eq!(CspArt::Stil.direktive(), "style-src");
        // added 03.10.2026, TLS: **DIE POSITIVKONTROLLE**, sonst waere der
        // Test ein Erfolg der Auswertung von `default-src` statt von `style-src`.
        let richtlinie = "style-src 'self'";
        assert!(
            csp_quellen(richtlinie, "style-src").is_some(),
            "die Richtlinie traegt style-src, nicht etwas anderes"
        );
        // 1) `style-src 'none'` sperrt das fremde Stylesheet.
        let fremd = q("https", "cdn.test", "/a.css");
        assert_eq!(
            // added 03.10.2026, TLS: **`&` vor dem `replace`** — `csp_erlaubt`
            // nimmt `&str`, und `String::replace` gibt eine `String`. GEMESSEN am
            // Compilerfehler `expected &str, found String`.
            csp_erlaubt(
                &richtlinie.replace("'self'", "'none'"),
                CspArt::Stil,
                &fremd,
                "https",
                None,
                None
            ),
            CspUrteil::Verweigert {
                entschieden_durch: "style-src".into(),
                verletzt_durch: "'none'".into()
            },
            "style-src 'none' sperrt ein fremdes Stylesheet"
        );
        // 2) Und die **Umkehrung**, sonst haette der Test nur die Sperre bewiesen:
        //    `script-src 'none'` darf ein Stylesheet **nicht** sperren.
        assert_eq!(
            csp_erlaubt(
                "script-src 'none'",
                CspArt::Stil,
                &fremd,
                "https",
                None,
                None
            ),
            CspUrteil::Erlaubt,
            "script-src 'none' hat mit style-src nichts zu tun"
        );
        // 3) Und der **Rueckfall** greift, wenn `style-src` fehlt — das ist der
        //    Normalfall und der Grund fuer `direktive()`.
        assert_eq!(
            csp_erlaubt(
                "default-src 'none'",
                CspArt::Stil,
                &fremd,
                "https",
                None,
                None
            ),
            CspUrteil::Verweigert {
                entschieden_durch: "default-src".into(),
                verletzt_durch: "'none'".into()
            },
            "ohne style-src entscheidet default-src"
        );
    }

    /// added 03.10.2026, TLS (Punkt 4): **`frame-ancestors`** — Clickjacking.
    ///
    /// **DER ENTSCHEIDENDE UND DER LEICHT VERwechselbare Fall:** `frame-ancestors`
    /// hat **keinen** Rueckfall auf `default-src`. Woertlich **CSP Level 3**,
    /// 6.4.2.2: „If the directive value is empty, the policy has no effect on
    /// frame-ancestors". Der Satz, den `csp_erlaubt` benutzt, steht dort
    /// **nicht**. Wer ihn trotzdem anwendet, sperrt jede Seite, die keine
    /// `frame-ancestors`-Zeile hat — also fast jede.
    #[test]
    fn frame_ancestors_hat_keinen_rueckfall_auf_default_src() {
        let einbettend = q("https", "evil.test", "/rahmen.html");
        let eigen = q("https", "bank.test", "/");
        // 1) 'none' sperrt jeden Einbett.
        assert_eq!(
            csp_frame_erlaubt("frame-ancestors 'none'", &einbettend, &eigen),
            CspUrteil::Verweigert {
                entschieden_durch: "frame-ancestors".into(),
                verletzt_durch: "'none'".into()
            }
        );
        // 2) 'self' erlaubt die eigene Seite.
        assert_eq!(
            csp_frame_erlaubt("frame-ancestors 'self'", &einbettend, &eigen),
            CspUrteil::Verweigert {
                entschieden_durch: "frame-ancestors".into(),
                verletzt_durch: "'self'".into()
            },
            "'self' heisst die eigene Herkunft, nicht jede"
        );
        assert_eq!(
            csp_frame_erlaubt("frame-ancestors 'self'", &eigen, &eigen),
            CspUrteil::Erlaubt,
            "die eigene Seite darf sich selbst einbetten"
        );
        // 3) **OHNE** `frame-ancestors` ist Einbetten erlaubt — auch wenn
        //    `default-src 'none'` dasteht. Das ist der Unterschied zu allen
        //    anderen Direktiven und der Grund fuer die eigene Funktion.
        assert_eq!(
            csp_frame_erlaubt("default-src 'none'", &einbettend, &eigen),
            CspUrteil::Erlaubt,
            "frame-ancestors hat keinen Rueckfall — das ist die Norm"
        );
        // 4) Und eine erlaubte Herkunft wird erlaubt.
        assert_eq!(
            csp_frame_erlaubt(
                "frame-ancestors https://partner.test",
                &q("https", "partner.test", "/"),
                &eigen
            ),
            CspUrteil::Erlaubt
        );
    }

    /// added 03.10.2026, TLS (Punkt 2): **DIE HSTS-HOCHSTUFUNG**, und der Test
    /// ist gegen die **drei** Faelle, an denen eine Aufrueckfunktion
    /// typischerweise kaputtgeht.
    #[test]
    fn hsts_rueckt_klartext_auf_https_hoch() {
        assert_eq!(
            hsts_aufruecken("http://seite.test/a/b.png", true).as_deref(),
            Some("https://seite.test/a/b.png"),
            "der Pfad muss **unveraendert** bleiben"
        );
        // added 03.10.2026, TLS: **ABFRAGE UND ANKER**, weil ein Bild wegen
        // seines Pfades geladen wird und ein Formular wegen seiner Abfrage.
        assert_eq!(
            hsts_aufruecken("http://seite.test/suche?q=a%20b&x=1#oben", true).as_deref(),
            Some("https://seite.test/suche?q=a%20b&x=1#oben")
        );
        // added 03.10.2026, TLS: **PORT 80 VERSCHWINDET, JEDER ANDERE BLEIBT.**
        // `https://host:80` waere etwas anderes als `https://host`, und der
        // Aufrufer von `hsts_anheben` (fremde Datei) will genau das nicht.
        assert_eq!(
            hsts_aufruecken("http://seite.test:80/x", true).as_deref(),
            Some("https://seite.test/x")
        );
        assert_eq!(
            hsts_aufruecken("http://seite.test:8080/x", true).as_deref(),
            Some("https://seite.test:8080/x"),
            "ein anderer Port bleibt erhalten — sonst waere es ein anderer Server"
        );
    }

    /// added 03.10.2026, TLS (Punkt 2): **DIE GEGENPROBE ZUR HSTS-HOCHSTUFUNG** —
    /// denn eine Regel, die immer hochrueckt, waere eine Erweiterung.
    #[test]
    fn hsts_aendert_nichts_ohne_hsts() {
        assert_eq!(
            hsts_aufruecken("http://seite.test/a.png", false),
            None,
            "ohne erzwungenes HSTS bleibt die Adresse unveraendert"
        );
        assert_eq!(
            hsts_aufruecken("https://seite.test/a.png", true),
            None,
            "https ist bereits erhoeht"
        );
        assert_eq!(
            hsts_aufruecken("ws://seite.test/socket", true),
            None,
            "HSTS gilt nicht fuer andere Schemata"
        );
        assert_eq!(
            hsts_aufruecken("https://seite.test:8443/a", true),
            None,
            "ein fremder Port an https wird nicht angefasst"
        );
    }

    /// added 03.10.2026, TLS (Punkt 3): **`SameSite`** — und der Test stellt
    /// **alle drei** Werte nebeneinander, weil der Unterschied zwischen
    /// `Strict` und `Lax` **genau eine Bedingung** ist und sonst nie auffaellt.
    #[test]
    fn samesite_steunt_lax_und_strict_unterscheiden_sich() {
        // Gleiche Herkunft: alle drei kommen durch.
        for w in [SameSiteWert::Strikt, SameSiteWert::Lax, SameSiteWert::Keine] {
            assert!(
                cookie_darf_gesendet(w, false, true, false, false, false, true).is_empty(),
                "{w:?} bei gleicher Herkunft: keine Sperre"
            );
        }
        // Fremde Herkunft, normale Unterressource: Strict **und** Lax sperren,
        // `None` laesst durch.
        for (w, gesperrt) in [
            (SameSiteWert::Strikt, true),
            (SameSiteWert::Lax, true),
            (SameSiteWert::Keine, false),
        ] {
            let g = cookie_darf_gesendet(w, false, true, true, false, false, false);
            assert_eq!(
                !g.is_empty(),
                gesperrt,
                "{w:?} bei fremder Herkunft: {}",
                g.first().map(String::as_str).unwrap_or("durchgelassen")
            );
        }
        // **UND DER UNTERSCHIED:** obere Navigation mit GET.
        let g = cookie_darf_gesendet(SameSiteWert::Lax, false, true, true, false, true, true);
        assert!(
            g.is_empty(),
            "SameSite=Lax laesst die obere Navigation durch"
        );
        let g = cookie_darf_gesendet(SameSiteWert::Strikt, false, true, true, false, true, true);
        assert!(
            !g.is_empty(),
            "SameSite=Strict sperrt **auch** die Navigation — das ist der Unterschied"
        );
        // Und POST als obere Navigation: das ist kein „sicherer" Versuch nach Lax.
        let g = cookie_darf_gesendet(SameSiteWert::Lax, false, true, true, false, true, false);
        assert!(!g.is_empty(), "Lax erlaubt nur GET oder HEAD, kein POST");
    }

    /// added 03.10.2026, TLS (Punkt 3): **`Secure`, `HttpOnly` und die
    /// Drittanbieter-Sperre** — die drei, die laut `lib.rs:3011` **nie
    /// ausgewertet** wurden.
    #[test]
    fn secure_httponly_und_drittanbieter_werden_endlich_geprueft() {
        // Secure ueber http: gesperrt, mit Normstelle im Grund.
        let g = cookie_darf_gesendet(SameSiteWert::Keine, true, false, false, false, false, false);
        assert_eq!(g.len(), 1, "genau ein Grund: {g:?}");
        assert!(g[0].contains("Secure"), "{}", g[0]);
        // Und ueber https: durch.
        assert!(
            cookie_darf_gesendet(SameSiteWert::Keine, true, true, false, false, false, false)
                .is_empty()
        );
        // HttpOnly: die Sichtbarkeit fuer Skript.
        assert!(
            !cookie_fuer_script_sichtbar(true),
            "HttpOnly ist fuer Skript unsichtbar"
        );
        assert!(
            cookie_fuer_script_sichtbar(false),
            "ohne HttpOnly ist es sichtbar"
        );
        // Drittanbieter-Sperre als **Option**, also als Argument und nicht als
        // Vorgabe: aus heilig gesperrt wird sie durch das Aufraeumen der Seite.
        assert!(
            !cookie_darf_gesendet(SameSiteWert::Keine, false, true, true, true, false, false)
                .is_empty(),
            "mit gesperrten Drittanbeier-Cookies wird auch SameSite=None gesperrt"
        );
        assert!(
            cookie_darf_gesendet(SameSiteWert::Keine, false, true, true, false, false, false)
                .is_empty(),
            "ohne die Sperre kommt ein Drittanbieter-Cookie durch"
        );
        // UND MEHRERE GRUENDE GLEICHZEITIG, weil das der Grund fuer `Vec` ist.
        let g = cookie_darf_gesendet(SameSiteWert::Strikt, true, false, true, true, false, false);
        assert_eq!(g.len(), 3, "drei unabhaengige Regeln: {g:?}");
    }
}

#[cfg(test)]
mod kopf_regel_tests {
    use super::*;
    use kopf_regeln::*;

    fn quelle(schema: &str, host: &str, port: Option<u16>, pfad: &str) -> CspQuelle {
        CspQuelle {
            schema: schema.into(),
            host: host.into(),
            port,
            pfad: pfad.into(),
        }
    }

    fn eigene() -> CspQuelle {
        quelle("https", "bank.test", None, "/")
    }

    // =======================================================================
    // X-FRAME-OPTIONS
    // =======================================================================

    /// **DER GANZE KAPUTTE-FALL**, und er ist der Grund fuer die Regel im
    /// Dateikopf: `X-Frame-Options: ALLOW-FROM https://bank.test` sieht wie
    /// eine Erlaubnis aus und ist **keine**.
    #[test]
    fn xfo_kennt_deny_sameorigin_und_ignoriert_den_rest() {
        let fremd = quelle("https", "boese.test", None, "/");
        assert_eq!(
            x_frame_options_urteil("DENY", &fremd, &eigene()),
            XfoUrteil::Deny
        );
        assert_eq!(
            x_frame_options_urteil("deny", &fremd, &eigene()),
            XfoUrteil::Deny,
            "case-insensitiv, woertlich so in der Spec"
        );
        assert_eq!(
            x_frame_options_urteil(
                " SAMEORIGIN ",
                &quelle("https", "bank.test", None, "/"),
                &eigene()
            ),
            XfoUrteil::SameOrigin,
            "getrimmt und gleiche Herkunft"
        );
        assert_eq!(
            x_frame_options_urteil("SAMEORIGIN", &fremd, &eigene()),
            XfoUrteil::Deny,
            "gleiche Herkunft fehlt -> verweigert"
        );
        // **UND DIE DREI FORMEN, DIE NICHT `ALLOW-FROM` SIND**
        for kaputt in [
            // added 04.10.2026, TLS: **`ALLOW-FROM` IST DER WICHTIGSTE
            // KAPUTTE WERT**, weil er **wie eine Erlaubnis aussieht**: die
            // Seite will erlauben, dass `https://bank.test` sie einbetten darf,
            // und bekommt **gar nichts**. **UND `DENY, SAMEORIGIN` IST EBENSO
            // KAPUTT** — die Spezifikation vergleicht den **gesamten**
            // getrimmten Wert mit **einem** Token, und ein Komma **beendet**
            // den Vergleich. **WER SPLITTET UND DAS ERSTE STUECK NIMMT, LIEGT
            // BEI `DENY, SAMEORIGIN` GENAU RICHTIG** — der Fehler geht in die
            // Richtung „strenger“, also ist er **harmlos**, aber er ist
            // trotzdem ein Fehler: `SAMEORIGIN, DENY` waere bei dieser Lesart
            // **erlaubt**.
            "ALLOW-FROM https://bank.test",
            "DENY, SAMEORIGIN",
            "SAMEORIGIN, DENY",
            "DENY SAMEORIGIN",
            "-",
            "1",
        ] {
            // **UND `sameorigin`/`sameOrigin`/`SameOrigin` SIND GUELTIG** —
            // case-insensitiv ist ausdruecklich erlaubt. **MEIN TEST HATTE
            // SIE ZUERST ZU DEN KAPUTTEN GEZAELT** (GEMESSEN: rot mit
            // `left: SameOrigin`), weil ich einen Bindestrich **erfunden**
            // hatte, den der Wert nicht hat.
        }
        for gueltig in ["SAMEORIGIN", "sameorigin", "SameOrigin"] {
            assert_eq!(
                x_frame_options_urteil(
                    gueltig,
                    &quelle("https", "bank.test", None, "/"),
                    &eigene()
                ),
                XfoUrteil::SameOrigin,
                "{gueltig:?} ist gueltig (nur die Schreibweise)"
            );
        }
        for kaputt in [
            "ALLOW-FROM https://bank.test",
            "DENY, SAMEORIGIN",
            "SAMEORIGIN, DENY",
            "DENY SAMEORIGIN",
            "-",
            "1",
        ] {
            assert_eq!(
                x_frame_options_urteil(kaputt, &quelle("https", "bank.test", None, "/"), &eigene()),
                XfoUrteil::Ignoriert,
                "{kaputt:?} wird ignoriert, nicht befolgt"
            );
        }
        assert_eq!(
            x_frame_options_urteil("", &fremd, &eigene()),
            XfoUrteil::KeinKopf
        );
    }

    /// **DAS SCHEMA GEHOERT DAZU**, und das ist der Angriff: `http://bank.test`
    /// in `https://bank.test` einbetten zu lassen.
    #[test]
    fn xfo_sameorigin_prueft_das_schema_und_den_port() {
        let http = quelle("http", "bank.test", None, "/");
        assert_eq!(
            x_frame_options_urteil("SAMEORIGIN", &http, &eigene()),
            XfoUrteil::Deny,
            "http in https: nicht dieselbe Herkunft"
        );
        let mit_port = quelle("https", "bank.test", Some(8443), "/");
        assert_eq!(
            x_frame_options_urteil("SAMEORIGIN", &mit_port, &eigene()),
            XfoUrteil::Deny,
            "Port 8443 ist nicht 443"
        );
        // **UND `:443` AUSDRUECKLICH IST `https` OHNE PORT**
        assert_eq!(
            x_frame_options_urteil(
                "SAMEORIGIN",
                &quelle("https", "bank.test", Some(443), "/"),
                &eigene()
            ),
            XfoUrteil::SameOrigin,
            "der Standardport zaehlt mit"
        );
    }

    /// **BEIDE ZUSAMMEN**, und die drei Faelle, die es gibt: strenger CSP,
    /// strenger XFO, und **keiner**.
    #[test]
    fn beim_einbetten_gewinnt_das_strengere() {
        let fremd = quelle("https", "boese.test", None, "/");
        // 1) CSP verbietet, XFO erlaubt -> **verboten**.
        let u = einbetten_urteil("frame-ancestors 'none'", "SAMEORIGIN", &fremd, &eigene());
        assert!(matches!(u, CspUrteil::Verweigert { .. }), "{u:?}");
        // 2) CSP fehlt, XFO verbietet -> **verboten**.
        let u = einbetten_urteil("", "DENY", &fremd, &eigene());
        assert_eq!(
            u,
            CspUrteil::Verweigert {
                entschieden_durch: "X-Frame-Options".into(),
                verletzt_durch: "DENY".into()
            },
            "{u:?}"
        );
        // 3) keiner von beiden -> **erlaubt** (die Norm).
        assert_eq!(
            einbetten_urteil("", "", &fremd, &eigene()),
            CspUrteil::Erlaubt
        );
        // 4) **UND WICHTIG: ein KAPUTTES XFO darf ein arbeitendes CSP nicht
        //    aushebeln** — das ist der Fehler, den man in drei Zeilen macht.
        assert_eq!(
            einbetten_urteil(
                "frame-ancestors 'none'",
                "ALLOW-FROM https://bank.test",
                &quelle("https", "bank.test", None, "/"),
                &eigene()
            ),
            CspUrteil::Verweigert {
                entschieden_durch: "frame-ancestors".into(),
                verletzt_durch: "'none'".into()
            },
            "das CSP entscheidet, das kaputte XFO schweigt"
        );
    }

    // =======================================================================
    // SUBRESOURCE INTEGRITY
    // =======================================================================

    /// **DER AUSDRUCK, und alle fuenf Formen**, die im Netz vorkommen.
    #[test]
    fn der_integritaets_ausdruck_wird_zerlegt() {
        assert_eq!(integritaet_aus("").len(), 0);
        assert_eq!(
            integritaet_aus("sha256-abc"),
            vec![("sha256".to_string(), "abc".to_string())]
        );
        // **MEHRERE, LEERZEUMLIG GETRENNT** — das ist der Weg, mit dem man von
        // SHA-256 auf SHA-512 umstellt, ohne den alten Hash zu verlieren.
        assert_eq!(
            integritaet_aus("sha256-abc sha512-def").len(),
            2,
            "zwei Hashes"
        );
        // **UND MIT OPTIONEN** — die werden **abgeschnitten**, nicht geparst.
        assert_eq!(
            integritaet_aus("sha256-abc?foo=bar"),
            vec![("sha256".to_string(), "abc".to_string())],
            "die Option gehoert nicht in den Hash"
        );
        // **UND `rsplit_once`:** `sha256-a-b` ist Algorithmus `sha256`, Wert
        // `a-b`. Mit `split_once` waere der Algorithmus `sha256-a`.
        assert_eq!(
            integritaet_aus("sha256-a-b"),
            vec![("sha256".to_string(), "a-b".to_string())],
            "der LETZTE Bindestrich trennt"
        );
        // **UND OHNE `-` ODER OHNE WERT** ist der Ausdruck **kaputt** und wird
        // **stillschweigend verworfen** — NICHT als „alles erlaubt“
        // gewertet, denn das waere eine Einladung.
        assert_eq!(integritaet_aus("sha256").len(), 0, "ohne Bindestrich");
        assert_eq!(integritaet_aus("-abc").len(), 0, "ohne Algorithmus");
        assert_eq!(integritaet_aus("sha256-").len(), 0, "ohne Wert");
    }

    /// **DIE VIER AUSGNAGEN**, und es sind **vier**, nicht zwei — die beiden
    /// mittleren sind genau die, die man vergisst.
    #[test]
    fn der_integritaets_vergleich_hat_vier_ausgaenge() {
        let berechnet = vec![
            ("sha256".to_string(), "RICHTIG".to_string()),
            ("sha512".to_string(), "LANG".to_string()),
        ];
        assert_eq!(
            integritaet_pruefen("", &berechnet),
            SriUrteil::OhneAngabe,
            "ohne `integrity` wird **nicht** geprueft"
        );
        assert_eq!(
            integritaet_pruefen("sha256-RICHTIG", &berechnet),
            SriUrteil::Erlaubt {
                algos: vec!["sha256".into()]
            }
        );
        // **IRGENDEINER** genuegt, auch wenn der erste falsch ist — das ist
        // ausdruecklich so in SRI §3.4 („If **any** of the metadata matches“).
        assert_eq!(
            integritaet_pruefen("sha256-FALSCH sha512-LANG", &berechnet),
            SriUrteil::Erlaubt {
                algos: vec!["sha512".into()]
            }
        );
        assert!(
            matches!(
                integritaet_pruefen("sha256-FALSCH", &berechnet),
                SriUrteil::Verweigert { .. }
            ),
            "beide bekannt, keiner passt -> verweigert"
        );
        // **UND DER UNBEKANNTE ALGORITHMUS** — der Fall, der **kein**
        // Verweigeren ist.
        assert_eq!(
            integritaet_pruefen("sha1-abc", &berechnet),
            SriUrteil::Unbekannt {
                algo: "sha1".into()
            },
            "SHA1 ist **unbekannt**, nicht falsch"
        );
    }

    /// **WARUM `Unbekannt` KEIN `Verweigert` IST**, an einem Beispiel mit
    /// **Folge**: verweigert man, ist die Seite **kaputt**; erlaubt man, ist
    /// SHA1 **Freigabe**. Beides ist ein Fehler, und die ehrliche Antwort ist
    /// die dritte.
    #[test]
    fn ein_unbekannter_algorithmus_ist_kein_verweigeren() {
        let berechnet = vec![("sha256".to_string(), "RICHTIG".to_string())];
        let u = integritaet_pruefen("sha1-abc", &berechnet);
        assert_ne!(
            u,
            SriUrteil::Verweigert {
                algo: "sha1".into(),
                grund: String::new()
            }
        );
        // **UND EIN BEKANNTER ALGORITHMUS, DER GAR NICHT BERECHNET WURDE**,
        // ist ebenfalls **Unbekannt** fuer uns — nicht „verweigert“.
        assert_eq!(
            integritaet_pruefen("sha384-abc", &berechnet),
            SriUrteil::Unbekannt {
                algo: "sha384".into()
            }
        );
    }

    /// **DER VERGLEICH SELBST**, und die drei Eigenschaften, die er haben muss.
    #[test]
    fn der_vergleich_ist_konstant_gleich_und_laengensicher() {
        assert!(konstante_gleichheit("abc", "abc"));
        assert!(!konstante_gleichheit("abc", "abd"), "ein Zeichen");
        assert!(!konstante_gleichheit("abc", "abcd"), "anders lang");
        assert!(konstante_gleichheit("", ""), "leer ist leer");
        assert!(!konstante_gleichheit("", "a"));
    }

    // =======================================================================
    // REFERRER-POLICY
    // =======================================================================

    /// **DIE ACHT TOKEN als Tabelle**, und jede Zelle wird geprueft — eine
    /// Tabelle, die nur **eine** Zeile prueft, ist eine Behauptung.
    #[test]
    fn die_referrer_tabelle_stimmt_in_Allen_drei_spalten() {
        let gleiche = true;
        let fremde = false;
        // gleiche Herkunft
        assert_eq!(
            referrer_umfang("no-referrer", gleiche, true, true),
            ReferrerUmfang::Keine
        );
        assert_eq!(
            referrer_umfang("same-origin", gleiche, true, true),
            ReferrerUmfang::VolleUrl
        );
        // added 04.10.2026, TLS: **`strict-origin` NIE die volle URL** — es
        // ist **immer** „nur die Herkunft“, auch bei gleicher Herkunft. **UND
        // HIER WAREN BEIDE FALSCH:** die erste Implementierung **und** der
        // erste Test (GEMESSEN: zuerst rot mit
        // `left: NurHerkunft, right: VolleUrl` — der Test beschrieb die
        // Implementierung, nicht die Spezifikation).
        assert_eq!(
            referrer_umfang("strict-origin", gleiche, true, true),
            ReferrerUmfang::NurHerkunft
        );
        assert_eq!(
            referrer_umfang("origin", gleiche, true, true),
            ReferrerUmfang::NurHerkunft
        );
        assert_eq!(
            referrer_umfang("strict-origin-when-cross-origin", gleiche, true, true),
            ReferrerUmfang::VolleUrl
        );
        assert_eq!(
            referrer_umfang("origin-when-cross-origin", gleiche, true, true),
            ReferrerUmfang::VolleUrl,
            "bei **gleicher** Herkunft: die volle URL"
        );
        assert_eq!(
            referrer_umfang("unsafe-url", gleiche, true, true),
            ReferrerUmfang::VolleUrl
        );
        // fremde Herkunft
        assert_eq!(
            referrer_umfang("no-referrer", fremde, true, true),
            ReferrerUmfang::Keine
        );
        assert_eq!(
            referrer_umfang("same-origin", fremde, true, true),
            ReferrerUmfang::Keine
        );
        assert_eq!(
            referrer_umfang("strict-origin", fremde, true, true),
            ReferrerUmfang::NurHerkunft
        );
        assert_eq!(
            referrer_umfang("origin", fremde, true, true),
            ReferrerUmfang::NurHerkunft
        );
        assert_eq!(
            referrer_umfang("strict-origin-when-cross-origin", fremde, true, true),
            ReferrerUmfang::NurHerkunft
        );
        assert_eq!(
            referrer_umfang("origin-when-cross-origin", fremde, true, true),
            ReferrerUmfang::VolleUrl,
            "der Name sagt es: nur die Herkunft wird zurueckgestuft"
        );
        assert_eq!(
            referrer_umfang("unsafe-url", fremde, true, true),
            ReferrerUmfang::VolleUrl
        );
    }

    /// **DER ABSTIEG https -> http — die dritte Spalte, und die Spalte, in der
    /// sich die beiden `when-cross-origin`-Zwillinge unterscheiden.**
    ///
    /// **UND DIESER TEST HAT MEINE ERSTE TABELLE KORRIGIERT, nicht die
    /// umgekehrt:** `strict-origin-when-cross-origin` und
    /// `no-referrer-when-downgrade` geben beim Abstieg **KEINEN** Referer
    /// (`Keine`), **nicht** „nur die Herkunft“. **WARUM DAS DER TEUERSTE
    /// EINZELNE FEHLER DER REGEL IST:** „nur die Herkunft“ **schickt immer
    /// noch** `https://bank.test` — also den **Namen der Seite**, auf die
    /// verlinkt wurde. Eine Seite mit `strict-origin-when-cross-origin`
    /// behauptet aber, sie wolle beim Abstieg **gar nichts** verraten, und das
    /// stimmt **nicht**, wenn man „nur die Herkunft“ schickt. Der Unterschied
    /// zwischen `Keine` und `NurHerkunft` **am Abstieg** ist genau der
    /// Unterschied zwischen „wir schweigen“ und „wir nennen nur den Host“.
    #[test]
    fn der_abstieg_https_nach_http_ist_die_dritte_spalte() {
        let gleiche = true;
        assert_eq!(
            referrer_umfang("strict-origin-when-cross-origin", gleiche, true, false),
            ReferrerUmfang::Keine,
            "strict: beim Abstieg **gar nichts**"
        );
        assert_eq!(
            referrer_umfang("origin-when-cross-origin", gleiche, true, false),
            ReferrerUmfang::NurHerkunft,
            "ohne strict: **nur die Herkunft** — das ist der ganze Unterschied"
        );
        assert_eq!(
            referrer_umfang("no-referrer-when-downgrade", gleiche, true, false),
            ReferrerUmfang::Keine,
            "`no-referrer-when-downgrade` ist dasselbe wie
             `strict-origin-when-cross-origin` **ohne** die Fremdregel"
        );
        assert_eq!(
            referrer_umfang("strict-origin", gleiche, true, false),
            ReferrerUmfang::Keine
        );
        assert_eq!(
            referrer_umfang("origin", gleiche, true, false),
            ReferrerUmfang::NurHerkunft,
            "`origin` schraenkt **nie** ein — auch beim Abstieg nicht"
        );
        assert_eq!(
            referrer_umfang("unsafe-url", gleiche, true, false),
            ReferrerUmfang::VolleUrl,
            "unsafe heisst: nie"
        );
        assert_eq!(
            referrer_umfang("same-origin", gleiche, true, false),
            ReferrerUmfang::VolleUrl,
            "`same-origin` schaut **nicht** auf das Schema"
        );
        // **UND DERSELBE ABSTIG MIT FREMDER HERKUNFT** — dort ist
        // `strict-origin-when-cross-origin` **ohnehin** nicht die volle URL.
        assert_eq!(
            referrer_umfang("strict-origin-when-cross-origin", false, true, false),
            ReferrerUmfang::Keine
        );
        assert_eq!(
            referrer_umfang("origin-when-cross-origin", false, true, false),
            ReferrerUmfang::NurHerkunft
        );
        assert_eq!(
            referrer_umfang("no-referrer-when-downgrade", false, true, false),
            ReferrerUmfang::Keine
        );
    }

    /// **DIE ZWEI REGELN DER LISTE**, und beide sind je **ein** Fehler wert.
    #[test]
    fn das_letzte_erkannte_token_gewinnt_und_unbekannte_werden_ignoriert() {
        assert_eq!(
            referrer_umfang("no-referrer, unsafe-url", true, true, true),
            ReferrerUmfang::VolleUrl,
            "das LETZTE gewinnt"
        );
        assert_eq!(
            referrer_umfang("unsafe-url, no-referrer", true, true, true),
            ReferrerUmfang::Keine
        );
        assert_eq!(
            referrer_umfang("origin, totaler-unsinn", true, true, true),
            ReferrerUmfang::NurHerkunft,
            "ein unbekanntes Token **bricht die Liste nicht ab**"
        );
        assert_eq!(
            referrer_umfang("totaler-unsinn, no-referrer", true, true, true),
            ReferrerUmfang::Keine,
            "und es steht auch **hinter** einem unbekannten noch"
        );
        // **UND WAS DIE VORGABE DANN IST, HAENGT AN DEN EINGABEN** — der
        // Test hatte hier `NurHerkunft` behauptet, weil er die **erste**
        // Implementierung beschrieb (die schrieb die Vorgabe **fest** statt
        // sie auszuwerten). **MIT KORRIGIERTER VORGABE** ist es bei gleicher
        // Herkunft und ohne Abstieg `VolleUrl`.
        assert_eq!(
            referrer_umfang("same-origin-when-cross-origin", true, true, true),
            ReferrerUmfang::VolleUrl,
            "`same-origin-when-cross-origin` gibt es **nicht** — die Erfindung
             wird ignoriert, und es bleibt die Vorgabe"
        );
        assert_eq!(
            referrer_umfang("same-origin-when-cross-origin", false, true, true),
            ReferrerUmfang::NurHerkunft,
            "dieselbe Erfindung bei fremder Herkunft: die Vorgabe greift"
        );
    }

    /// **DIE VORGABE, und sie ist der wichtigste einzelne Messwert dieser
    /// Regel**: ohne Kopf ist der Browser **nicht** locker, sondern
    /// `strict-origin-when-cross-origin`.
    #[test]
    fn ohne_referrer_kopf_gilt_strict_origin_when_cross_origin() {
        for leer in ["", "   ", "\t"] {
            assert_eq!(
                referrer_umfang(leer, true, true, true),
                ReferrerUmfang::VolleUrl,
                "gleiche Herkunft: die volle Adresse"
            );
            assert_eq!(
                referrer_umfang(leer, false, true, true),
                ReferrerUmfang::NurHerkunft,
                "fremde Herkunft: nur die Herkunft — **nicht** der volle Pfad"
            );
        }
    }

    // =======================================================================
    // PERMISSIONS-POLICY
    // =======================================================================

    /// **DIE VIER WERTFORMEN**, und `self` heisst **die Seite**, die die Regel
    /// setzt — nicht die Ressource.
    #[test]
    fn permissions_policy_erlaubt_und_verweigert() {
        let seite = quelle("https", "bank.test", None, "/");
        // 1) kein Eintrag -> **Vorgabe: erlaubt**.
        assert!(
            permissions_policy_erlaubt("", "camera", &seite, &seite),
            "Vorgabe `*`"
        );
        // 2) `self` -> nur die **Seite**, die die Regel setzt.
        assert!(permissions_policy_erlaubt(
            "camera=(self)",
            "camera",
            &seite,
            &seite
        ));
        assert!(
            permissions_policy_erlaubt("camera=self", "camera", &seite, &seite),
            "ohne Klammern, ohne Anfuehrung"
        );
        assert!(
            !permissions_policy_erlaubt(
                "camera=(self)",
                "camera",
                &seite,
                &quelle("https", "boese.test", None, "/")
            ),
            "`self` heisst **die Seite**, die die Regel setzt"
        );
        // 3) `'none'` und `()` -> nichts.
        assert!(
            !permissions_policy_erlaubt("camera=()", "camera", &seite, &seite),
            "leer = nichts"
        );
        assert!(!permissions_policy_erlaubt(
            "camera=('none')",
            "camera",
            &seite,
            &seite
        ));
        // 4) `*` -> alles.
        assert!(permissions_policy_erlaubt(
            "camera=*", "camera", &seite, &seite
        ));
        // 5) eine fremde Herkunft.
        assert!(permissions_policy_erlaubt(
            "camera=(\"https://andere.test\")",
            "camera",
            &seite,
            &quelle("https", "andere.test", None, "/")
        ));
        assert!(!permissions_policy_erlaubt(
            "camera=(\"https://andere.test\")",
            "camera",
            &seite,
            &seite
        ));
        // **UND DER PORT, der sonst durchginge**: `https://andere.test:8443` ist
        // **nicht** `https://andere.test` — und das ist genau die Herkunft, an
        // der ein Angreifer seine Seite laufen laesst.
        assert!(!permissions_policy_erlaubt(
            "camera=(\"https://andere.test\")",
            "camera",
            &seite,
            &quelle("https", "andere.test", Some(8443), "/")
        ));
        assert!(
            permissions_policy_erlaubt(
                "camera=(\"https://andere.test:443\")",
                "camera",
                &seite,
                &quelle("https", "andere.test", None, "/")
            ),
            "`:443` **ist** `https` ohne Port"
        );
    }

    /// **DIE TRENNUNG DER RICHTLINIEN AN KOMMA, NICHT INNERHALB DER
    /// KLAMMERN** — und **das** ist die Stelle, an der die meisten Parser
    /// aufgeben.
    #[test]
    fn die_richtlinien_trennen_sich_am_komma_ausserhalb_der_klammern() {
        // **ZWEI** Richtlinien, **EIN** Komma zwischen ihnen.
        let (gefunden, werte) = permissions_policy_werte(
            "camera=(self \"https://a.test\"), microphone=(self)",
            "microphone",
        );
        assert!(gefunden);
        assert_eq!(werte, vec!["self".to_string()]);
        // **EINER** Eintrag, **zwei** Werte, Komma **innerhalb**.
        let (gefunden, werte) = permissions_policy_werte(
            "geolocation=(self \"https://a.test\",\"https://b.test\")",
            "geolocation",
        );
        assert!(gefunden);
        assert_eq!(werte.len(), 2, "zwei Werte in einer Richtlinie");
        // **UND EIN FREMDER NAME STOERT NICHT** — `microphone` darf nicht
        // antworten, wenn nach `camera` gefragt wurde.
        assert_eq!(permissions_policy_werte("camera=*", "microphone").0, false);
    }

    /// **DIE WILDKARTE, und sie ist „eine Ebene“, nicht „beliebig tief“.**
    #[test]
    fn die_wildkarte_erlaubt_eine_ebene_und_nicht_zwei() {
        let politik = "camera=(\"https://*.beispiel.test\")";
        let seite = eigene();
        assert!(
            permissions_policy_erlaubt(
                politik,
                "camera",
                &seite,
                &quelle("https", "a.beispiel.test", None, "/")
            ),
            "eine Ebene passt"
        );
        assert!(
            !permissions_policy_erlaubt(
                politik,
                "camera",
                &seite,
                &quelle("https", "a.b.beispiel.test", None, "/")
            ),
            "zwei Ebenen passen **nicht** — dafuer gibt es `*`"
        );
        assert!(
            !permissions_policy_erlaubt(
                politik,
                "camera",
                &seite,
                &quelle("http", "a.beispiel.test", None, "/")
            ),
            "das Schema muss dazu passen"
        );
    }

    /// **SIRI/SPEECH UND KAMERA, und das ist der Grund fuer die ganze Regel**:
    /// ein Kopf, der **nur** `camera` verbietet, laesst **Mikrofon** und
    /// **Standort** zu — und das ist der haeufigste Denkfehler in der
    /// Werbung („wir blockieren Kamerazugriff“).
    #[test]
    fn eine_richtlinie_blockiert_nur_ihre_own() {
        let politik = "camera=()";
        let seite = quelle("https", "bank.test", None, "/");
        assert!(!permissions_policy_erlaubt(
            politik, "camera", &seite, &seite
        ));
        assert!(
            permissions_policy_erlaubt(politik, "microphone", &seite, &seite),
            "Mikrofon ist eine **andere** Richtlinie"
        );
        assert!(permissions_policy_erlaubt(
            politik,
            "geolocation",
            &seite,
            &seite
        ));
    }

    // =======================================================================
    // COOP / COEP
    // =======================================================================

    /// **DIE DRINGINGLICHKEIT DER DREI BEDINGUNGEN**, und jede einzelne fehlt
    /// lassen heisst „nicht verschraenkt“.
    #[test]
    fn die_verschraenkung_braucht_alle_drei_bedingungen() {
        assert!(kreuz_herkunft_verschraenkt(
            "same-origin",
            "require-corp",
            true
        ));
        assert!(
            !kreuz_herkunft_verschraenkt("same-origin-allow-popups", "require-corp", true),
            "die Popups-Variante zaehlt **nicht**"
        );
        assert!(
            !kreuz_herkunft_verschraenkt("same-origin", "credentialless", true),
            "`credentialless` zaehlt (noch) **nicht**"
        );
        assert!(
            !kreuz_herkunft_verschraenkt("same-origin", "unsafe-none", true),
            "`unsafe-none` ist das Gegenteil"
        );
        assert!(
            !kreuz_herkunft_verschraenkt("same-origin", "require-corp", false),
            "ohne sicheren Kontext ist alles **wirkungslos**"
        );
        // **UND case-insensitiv + mit Optionsanhang**, weil Browser
        // Optionsanhaenge liefern duerfen.
        assert!(kreuz_herkunft_verschraenkt(
            "Same-Origin",
            "Require-Corp",
            true
        ));
        assert!(kreuz_herkunft_verschraenkt(
            "same-origin, foo",
            "require-corp; x",
            true
        ));
    }

    /// **DER GRUND, und nicht nur das Ja/Nein** — `http://192.168.x.x` ist der
    /// Fall, den man am leichtesten uebersieht.
    #[test]
    fn die_verschraenkung_sagt_welcher_teil_fehlt() {
        assert_eq!(
            verschraenkung("same-origin", "require-corp", true),
            Verschraenkung::Aktiv
        );
        assert_eq!(
            verschraenkung("", "require-corp", true),
            Verschraenkung::FehlenderCoop
        );
        assert_eq!(
            verschraenkung("same-origin", "", true),
            Verschraenkung::FehlenderCoep
        );
        assert_eq!(verschraenkung("", "", true), Verschraenkung::FehltBeides);
        assert_eq!(
            verschraenkung("same-origin", "require-corp", false),
            Verschraenkung::KeinSichererKontext,
            "der Fall, den man am leichtesten uebersieht"
        );
    }

    // =======================================================================
    // CSP `sandbox`
    // =======================================================================

    /// **DER FALL, DER IN KEINEM BEISPIEL STEHT**: `sandbox` **ohne**
    /// Token-Liste bedeutet **alles verboten**.
    #[test]
    fn sandbox_ohne_token_heisst_alles_verboten() {
        let r = csp_sandbox_regeln("sandbox");
        assert!(r.aktiv, "die Direktive steht");
        assert!(!r.skripte, "keine Skripte");
        assert!(!r.gleiche_herkunft, "keine Herkunft");
        assert!(!r.formulare);
        // **UND DIE MELDUNG, weil „Sandbox ohne: …“ das ist, was der Benutzer
        // sehen muss, und es muss **alle** fehlenden Rechte nennen.**
        let m = sandbox_meldung(&r);
        assert!(m.contains("Skripte"), "{m}");
        assert!(m.contains("Herkunft"), "{m}");
        assert!(m.contains("Formulare"), "{m}");
    }

    /// **KEINE SANDBOX, wenn die Direktive fehlt** — und das ist der
    /// **Normalfall** jeder Seite im Web.
    #[test]
    fn ohne_direktive_ist_keine_sandbox() {
        let r = csp_sandbox_regeln("default-src 'self'");
        assert!(!r.aktiv);
        assert_eq!(sandbox_meldung(&r), "", "keine Sandbox, keine Meldung");
    }

    /// **DIE TOKEN, und es sind **elf** — jeder mit dem Recht, das er gibt.
    #[test]
    fn die_sandbox_token_geben_ihre_rechte() {
        let r = csp_sandbox_regeln(
            "sandbox allow-scripts allow-same-origin allow-forms allow-popups allow-modals \
             allow-pointer-lock allow-top-navigation allow-downloads \
             allow-storage-access-by-user-activation allow-orientation-lock \
             allow-presentation allow-popups-to-escape-sandbox",
        );
        assert!(r.aktiv);
        assert!(r.skripte && r.gleiche_herkunft && r.formulare && r.popups && r.dialoge);
        assert!(r.zeiger_sperre && r.navigation_nach_oben && r.downloads);
        assert!(r.speicherzugriff && r.orientierung);
        assert!(r.praesentation && r.popups_aus_sandbox);
        assert_eq!(sandbox_meldung(&r), "Sandbox ohne Einschraenkung");
        // **UND EINEN TEIL WEG** → genau **dieser** fehlt in der Meldung.
        let r2 = csp_sandbox_regeln("sandbox allow-scripts allow-forms");
        let m = sandbox_meldung(&r2);
        assert!(m.contains("Herkunft"), "{m}");
        assert!(!m.contains("Skripte"), "{m}");
        assert!(!m.contains("Formulare"), "{m}");
    }

    /// **DIE SChnittMENGE**, und **das `&&` ist der ganze Punkt**: ein `iframe
    /// sandbox="allow-scripts allow-forms"` neben einer CSP
    /// `sandbox allow-scripts` hat am Ende **beide** Rechte.
    #[test]
    fn die_schnittmenge_aus_attribut_und_direktive_ist_nicht_die_vereinigung() {
        let attribut = SandboxRegeln {
            aktiv: true,
            skripte: true,
            formulare: true,
            ..SandboxRegeln::default()
        };
        let direktive = csp_sandbox_regeln("sandbox allow-scripts allow-same-origin");
        let s = sandbox_schnittmenge(attribut, direktive);
        assert!(s.aktiv);
        assert!(s.skripte, "beide sagen es");
        assert!(!s.formulare, "**nur das Attribut** sagt es -> weg");
        assert!(!s.gleiche_herkunft, "**nur die CSP** sagt es -> weg");
        let m = sandbox_meldung(&s);
        assert!(m.contains("Formulare"), "{m}");
        assert!(m.contains("Herkunft"), "{m}");
    }

    /// **UND `aktiv` IST DAS `ODER`** — ein Rahmen ist sandboxed, wenn **eine**
    /// der beiden es sagt, auch wenn die andere **gar nichts** erlaubt.
    #[test]
    fn ein_aktives_attribut_allein_macht_den_rahmen_sandboxed() {
        let s = sandbox_schnittmenge(
            SandboxRegeln {
                aktiv: true,
                skripte: true,
                ..SandboxRegeln::default()
            },
            SandboxRegeln::default(),
        );
        assert!(s.aktiv);
        assert!(!s.skripte, "die CSP sagt nichts -> **kein** Recht");
    }
}
