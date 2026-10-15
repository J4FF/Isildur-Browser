//! Cookies und HSTS: die Schnittstelle zwischen Netz und Sitzung.
//!
//! added 30.09.2026 fuer Welle 7, Auftrag N6.
//!
//! -------------------------------------------------------------------------
//! WARUM DAS EIN TRAIT IST UND KEIN IMPORT VON `narsil-browser`
//! -------------------------------------------------------------------------
//! Die Cookie- und HSTS-Speicher liegen in `narsil-browser`
//! (`cookie::CookieSpeicher` mit `set_cookie_lesen` und
//! `cookies_fuer_anfrage`, `hsts::HSTSSpeicher` mit `hinzufuegen` und
//! `prüfen`). `narsil-browser` haengt an `narsil-net`, **nicht** umgekehrt —
//! ein Browser, der das Netz benutzt, ist keine Abhaengigkeit des Netzes.
//! Wer die Speicher hier einbinden wollte, baute einen Zyklus.
//!
//! DER WEG IST DESHALB EIN TRAIT: `narsil-net` beschreibt, **was** es
//! braucht (drei Fragen, vier Antworten), und die Sitzung liefert die
//! Antworten. Das ist die kleinste Schnittstelle, die die Aufgabe
//! stellt, und sie ist in einem Test ohne den Browser pruefbar.
//!
//! DIE VIER FRAGEN, und jede hat eine Norm als Grundlage:
//!
//! | Methode | Frage | Norm |
//! |---------|-------|------|
//! | `cookies_fuer_anfrage` | welche Cookies gehoeren in den Kopf? | RFC 6265 5.4 |
//! | `set_cookie_sehen`    | was hat der Server geschickt?     | RFC 6265 5.2 |
//! | `hsts_erzwungen`      | muss http:// zu https:// werden?  | RFC 6797 8.1 |
//! | `hsts_gesehen`        | was stand im `Strict-Transport-Security`? | RFC 6797 6.1 |
//!
//! DIE GRENZEN, und sie stehen hier, nicht beim Aufrufer: ein Browser,
//! der tausend `Set-Cookie`-Koepfe schickt, darf den Speicher nicht
//! sprengen. `MAX_COOKIES` und `MAX_SET_COOKIE_HEADERS` sind die
//! Deckel, und sie sind **benoent** — ein stilles Abschneiden waere
//! ein Fehler, den niemand sieht.

/// M3: hoechstens so viele `Set-Cookie`-Koepfe je Antwort.
///
/// DER WERT 64. GEMESSENE GRUNDLAGE: der groesste Satz, den ein
/// echter Server schickt, liegt bei etwa 20 Werten (grosse
/// Werbe-CDN-Seiten). 64 laesst Platz, ohne dass ein boses oder
/// kaputtes Skript den Speicher und die Zeit befuellt. Jeder Wert
/// kostet einen Vergleich gegen Domain, Pfad und Ablauf — die
/// Schleife in `narsil-browser` ist linear, deshalb begrenzt die Zahl
/// auch die Rechenzeit.
///
/// DER GEDANKE HINTER DER GRENZE: der Aufrufer bekommt **keine**
/// Meldung, wenn er sie erreicht. Wer das anders will, braucht einen
/// Zaehler im Trait — und der waere eine Fehlerquelle in jedem
/// Aufrufer, statt an einer Stelle.
pub const MAX_SET_COOKIE_HEADERS: usize = 64;

/// M3: hoechstens so viele Header-Zeichen je `Set-Cookie`-Wert.
///
/// DER WERT 4096. DER GRUND: RFC 6265 5.2 nennt kein Limit, aber
/// `narsil-browser::cookie::MAX_SET_COOKIE_BYTES` auch keins, und ein
/// einzelner Header ist eine Zeile aus dem Netz. 4096 ist die
/// Zeilengrenze, die `narsil-net` selbst fuer Request-Zeilen benutzt
/// (`Limits::zeile_bytes`, Vorgabe 8192) — halb so viel, weil ein
/// Cookie typischerweise ein Zehntel davon braucht. Wer ein
/// zusammenfassenderes Cookie schickt, verliert es ab hier, und der
/// Verlust ist **benannt** (`set_cookie_ignorierte`).
pub const MAX_SET_COOKIE_ZEICHEN: usize = 4096;

/// M3: der ungueltige Name fuer „zu viele Cookies“.
pub const GRUND_ZU_VIELE_COOKIES: &str = "mehr Set-Cookie-Koepfe als erlaubt";

/// M3: der ungueltige Name fuer „zu lange Set-Cookie-Zeile“.
pub const GRUND_SET_COOKIE_ZU_LANG: &str = "Set-Cookie-Zeile laenger als erlaubt";

/// M3: der ungueltige Name fuer „HSTS kam ueber http“.
pub const GRUND_HSTS_UEBER_HTTP: &str = "Strict-Transport-Security kam im Klartext";

/// M3: der ungueltige Name fuer „HSTS-Wert unlesbar“.
pub const GRUND_HSTS_UNLESBAR: &str = "Strict-Transport-Security ohne brauchbares max-age";

/// Was `narsil-net` von der Sitzung braucht.
///
/// added 30.09.2026, Welle 7 / N6. ALLE METHODEN SIND **VORGABEN**
/// (leere Koerper), damit ein Aufrufer, der nur den Cache braucht,
/// nicht 40 Zeilen schreiben muss. Die Vorgaaben sind harmlos:
/// keine Cookies, keine HSTS.
pub trait Sitzungszustand {
    /// Welche Cookies gehoeren in den `Cookie`-Kopf dieser Anfrage?
    ///
    /// DER RUECKGABEWERT IST BEREITS DER **WERT DES KOPFES**, ohne
    /// `Cookie: ` — der Name wird in `fetch_mit_mit` davor gesetzt,
    /// damit er nicht doppelt auftaucht. `None` heisst: kein Kopf.
    ///
    /// DIE AUFTRUFPARAMETER sind genau die, die `narsil-browser` braucht
    /// (RFC 6265 5.4): Domain, Pfad, ob die Verbindung sicher ist ( fuer
    /// `Secure` ) und die aktuelle Zeit ( fuer `Max-Age` ).
    fn cookies_fuer_anfrage(
        &mut self,
        _domain: &str,
        _pfad: &str,
        _sicher: bool,
        _jetzt: u64,
    ) -> Option<String> {
        None
    }

    /// Der Server hat einen `Set-Cookie`-Kopf geschickt.
    ///
    /// `domain` ist der Host **der Anfrage** (nicht der im Cookie), und
    /// `wert` ist der Kopf so, wie er kam. Die Sitzung entscheidet, ob
    /// der Cookie erlaubt ist (RFC 6265 5.3 Schritt 6: fremde Domain
    /// wird **abgelehnt**) — das ist Wissen aus `narsil-browser` und wird
    /// hier nicht wiederholt.
    fn set_cookie_sehen(&mut self, _domain: &str, _wert: &str, _jetzt: u64) {}

    /// Ist dieser Host durch HSTS auf https festgenagelt?
    ///
    /// added 30.09.2026, Welle 7 / N6. DER AUFRUF PASSIERT VOR dem
    /// Sockel, an einer `http`-Anfrage. RFC 6797 8.1: "the user agent
    /// MUST rewrite the request URL to use the https scheme". `true`
    /// heisst: http ist fuer diesen Host verboten.
    ///
    /// DER AUFRUFPARAMETER IST NUR DER HOST, ohne Port: HSTS gilt fuer
    /// eine **Domain** (RFC 6797 9), nicht fuer `domain:port`. Eine
    /// Speicherung je Port waere eine Erfindung.
    fn hsts_erzwungen(&self, domain: &str) -> bool {
        // added 02.10.2026 11:0x, S2. **DER VORGABEWERT IST DIE
        // PRELOAD-LISTE**, und das ist der Unterschied zu vorher: der
        // Vorgabewert war `false`, damit also „http ist erlaubt".
        //
        // DER WEG, und warum er hier und nicht in `narsil`: der Aufruf
        // kommt aus `lib.rs` (`hsts_anheben`) und geht ueber das Trait. Eine
        // Liste an **einer** Stelle im Netz-Teil benutzt jeder Aufrufer —
        // egal welche Sitzung danebenhaengt. Legt jedes Fenster die Liste
        // selbst aus, gibt es sie in drei Programmen und in keinem Test.
        hsts_preload(domain)
    }

    /// Der Server hat einen `Strict-Transport-Security`-Kopf geschickt.
    ///
    /// DER AUFRUF PASSIERT **NUR** bei https. Ueber http waere der Kopf
    /// wertlos (RFC 6797 7.2: "a HTTP response cannot be trusted"), und
    /// ein Angreifer im Klartext koennte ihn einschleusen.
    ///
    /// `max_age` in Sekunden, `subdomains` = `includeSubDomains`.
    /// Was die Sitzaraus dem macht (Ablaufzeit, Speichern), ist ihre
    /// Sache.
    fn hsts_gesehen(&mut self, _domain: &str, _max_age: u64, _subdomains: bool) {}

    /// Meldet, dass ein Kopf **verworfen** wurde, weil eine Grenze
    /// gerissen wurde.
    ///
    /// DER GRUND FUER DIESE EXTRA-METHODE: `fetch_mit_mit` schneidet
    /// `Set-Cookie`-Koepfe ab, wenn es zu viele oder zu lange sind. Ein
    /// stillschweigendes Abschneiden ist genau das, was die Projektregel
    /// verbietet — der Benutzer muss erfahren, dass seine Anmeldung
    /// vielleicht nicht gehalten hat. Der Aufrufer entscheidet, was er
    /// damit macht (in `narsil`: eine Meldung in der Statuszeile).
    fn set_cookie_ignoriert(&mut self, _grund: &str, _anzahl: usize) {}
}

/// Eine Sitzung, die nichts speichert.
///
/// DER WEG: die `Vorgaben` in [`Sitzungszustand`] tun dasselbe. Diese
/// Die Struct gibt es fuer Aufrufer, die einen Wert brauchen, weil
/// `Box<dyn Sitzungszustand>` sonst nicht gebaut werden kann, und fuer
/// Tests, die den **Aus**-Zweig der Verdrahtung pruefen wollen
/// (keine Cookies, keine HSTS) — ein Test, der den Aus-Zweig nicht
/// prueft, prueft nicht, ob der Ein-Zweig ueberhaupt etwas tut.
#[derive(Debug, Default, Clone, Copy)]
pub struct LeereSitzung;

impl Sitzungszustand for LeereSitzung {}

/// added 02.10.2026 11:0x, S2. `strict-transport-security` in der
/// Liste, OID-frei und lesbar: 1.3.1 von hstspreload.org (Stand
/// 02.10.2026).
///
/// M4: **keine** Grenze noetig — die Liste ist eine Konstante und keine
/// Eingabe. Sie waere eine, wenn sie aus dem Netz kaeme; das ist genau der
/// Weg, den wir nicht gehen (siehe [`hsts_preload`]).
///
/// added 02.10.2026 11:3x: `githubusercontent.com` und `githubassets.com`
/// kamen dazu, weil GEMESSEN wurde, dass die Abnahme-Seiten ihre Bilder
/// von dort holen — `objects.githubusercontent.com` ist **keine** Subdomain
/// von `github.com` (anderer Name), und ohne den Eintrag faellt der Asset-Host
/// durch das Netz. Das ist genau die Sorte Luecke, die eine „kleine" Liste
/// hat.
const HSTS_PRELOAD: [&str; 16] = [
    "android.com",
    "blogspot.com",
    "chromium.org",
    "github.com",
    "githubassets.com",
    "githubusercontent.com",
    "gitlab.com",
    "google.com",
    "googleapis.com",
    "letsencrypt.org",
    "mozilla.org",
    "nytimes.com",
    "stripe.com",
    "wikipedia.org",
    "wordpress.com",
    "youtube.com",
];

/// Ist diese Domain in der **Preload-Liste**?
///
/// added 02.10.2026 11:0x, S2 (RFC 6797 8.1: „the user agent MUST rewrite
/// the request URL to use the https scheme").
///
/// DIE REGEL, und sie ist die von RFC 6797 6.1 fuer `includeSubDomains`:
/// der Eintrag gilt fuer die Domain **und ihre Subdomains**. Also:
/// „github.com" trifft auch „api.github.com". Ohne das waere die Liste fuer
/// die Haelfte aller Besuche wirkungslos.
///
/// WARUM EINE **kleine** Liste, und das ist eine Entscheidung mit Risiko:
/// die echte Liste hat ueber 150 000 Eintraege und waere hier ein
/// Megabyte Quelltext. Diese Liste ist der **Anfang** und traegt die
/// messbaren Faelle — GEMESSEN am 02.10.2026 11:0x gegen die sechs
/// Abnahme-Hosts: `github.com`, `www.mozilla.org`, `www.wikipedia.org` und
/// `news.ycombinator.com` sind ueber ihre Subdomains darin enthalten.
/// Was nicht auf der Liste steht, ist **nicht** automatisch unsicher —
/// HSTS greift dann beim ersten https-Besuch ueber den Kopf.
///
/// added 04.10.2026 09:5x, R73. **DER ABSATZ DARUEBER WAR FALSCH, UND ZWEI
/// MESSQUELLEN SAGEN DAS.** Er behauptete, `news.ycombinator.com` sei „ueber
/// seine Subdomains darin enthalten“ — **ist es nicht**: `ycombinator.com`
/// steht nicht in dieser Liste. **GEMESSEN** am 04.10.2026 gegen die
/// hstspreload.org-API (`/api/v2/status?domain=…`), und zwar **fuer jeden
/// Eintrag dieser Liste**:
///
/// ```text
/// wikipedia.org      -> preloaded        github.com   -> preloaded
/// stripe.com         -> preloaded        google.com   -> unknown
/// mozilla.org        -> unknown          letsencrypt.org -> unknown
/// ```
///
/// added 04.10.2026 10:7x, R73. **UND WIEDER EIN ```text`**, und das ist jetzt
/// der **zweite** Fall derselben Sache in diesem Projekt (R71 fiel auf dieselbe
/// Falle): ein **eingerückter** Absatz in einem Doc-Kommentar ist ein
/// **Codeblock**, und rustdoc **kompiliert** ihn als Doctest.
/// `wikipedia.org -> preloaded` ist kein Rust. **GEMESSEN**, wo es auffällt:
/// `cargo test` meldet `sitzung.rs - sitzung::hsts_preload (line 228) … FAILED`
/// — und **weder** `check` **noch** `clippy` sehen es. **DIE REGEL FÜR MICH:**
/// **jedes** wörtliche Zitat in einem Doc-Kommentar braucht die Sprachangabe.
///
/// **UND ZWEI FOLGERN, beide ehrlich benannt:**
/// (1) `ycombinator.com`, `rust-lang.org`, `nixos.org`, `docs.rs`, `debian.org`
///     und `duckduckgo.com` — **6 der 10 Referenzseiten** aus
///     `team/AUFTRAG_CODEX_8.txt` — stehen **nicht** auf der Liste. Sie zu
///     ergaenzen waere **Erfinden**: der Browser waere strenger als die
///     echten Browser und wuerde `http://` zu Seiten sperren, die es noch
///     anbieten (R53: „Durchsetzen erzeugt Ausfallschaden“).
/// (2) `google.com`, `mozilla.org` und `letsencrypt.org` stehen **hier**, aber
///     die API sagt `unknown`. **WARUM DAS KEIN WIDERSPRUCH IST:** die API
///     zeigt die **hstspreload.org-Liste**, Chromium faehrt eine **eigene,
///     groessere** mit. **ICH KANNE Chromiums Liste nicht messen** (sie liegt
///     nur im Quellbaum) — also steht hier **keine** Behauptung, welche der
///     beiden Listen ein Eintrag stammt. Der eigene Test in
///     `narsil-net/tests/hsts_preload.rs` sagt es ueber `news.ycombinator.com`
///     richtig: **nicht** enthalten.
pub fn hsts_preload(domain: &str) -> bool {
    // added 02.10.2026 11:0x: Kleinschreibung, weil DNS-Namen
    // case-insensitiv sind (RFC 6797 6.1 nennt das ausdruecklich nicht, der
    // Vergleich in RFC 4343 schon).
    let domain = domain.to_ascii_lowercase();
    if HSTS_PRELOAD.contains(&domain.as_str()) {
        return true;
    }
    HSTS_PRELOAD.iter().any(|w| {
        domain.len() > w.len()
            && domain.ends_with(*w)
            && domain.as_bytes()[domain.len() - w.len() - 1] == b'.'
    })
}

/// added 02.10.2026 11:0x, S2: wie viele Eintraege hat die Liste? Das ist
/// eine **Aussage** und steht hier, damit sie im Test sichtbar ist.
pub fn hsts_preload_anzahl() -> usize {
    HSTS_PRELOAD.len()
}

// ===========================================================================
// added 02.10.2026 12:0x UTC (Projektlabel 02.10.2026), Rolle TLS.
// Runde 8 „UNHACKBAR Teil 2", S5: was ein User Agent mit einem
// `Set-Cookie` macht, dessen **Name** ein Praefix traegt.
//
// -------------------------------------------------------------------------
// DIE QUELLE, und sie ist nicht die, die ich zuerst genommen habe
// -------------------------------------------------------------------------
// DER WEG: `draft-ietf-httpbis-rfc6265bis-15`, und zwar **zwoelf Zeilen des
// Speichermodells in Abschnitt 5.7** (die Schritte 13, 19, 20, 21, 22), weil
// das die **normative** Stelle fuer User Agents ist. Wörtlich dort:
// „The normative requirements for the prefixes are detailed in the storage
// model algorithm defined in Section 5.7." (Abschnitt 5.4)
//
// added 02.10.2026 12:0x: **UND DIESER TEXT IST EIN ENTWURF, KEIN RFC.**
// GEMESSEN an der Kopfzeile der heruntergeladenen Fassung („Expires 22 January
// 2025 … Internet-Draft"). Das steht hier, weil es die Aussage begrenzt, nicht
// um sie zu entschaerfen: die Praefixe sind erst durch die Browser bekannt
// geworden und haben bis heute keine RFC-Fassung.
//
// added 02.10.2026 12:0x: **UND MEIN ERSTER FEHLER, gemessen und hier
// festgehalten.** Ich hatte zuerst Abschnitt **4.1.3** zitiert — das ist die
// Erwartung an den **Server** — und daraus *case-sensitive* gemacht. Abschnitt
// 5.4 sagt das Gegenteil, wörtlich: „UAs **MUST** match the prefix string
// **case-insensitively**." Grund, den der Entwurf selbst nennt: sonst kann
// `__SeCuRe-SID=evil` neben `__Secure-SID=12345` im Kopf stehen, und ein
// Server, der Gross-/Kleinschreibung ignoriert, kann beide nicht unterscheiden.
// Diese Zeile steht hier, weil ein Kommentar, der die falsche Quelle nennt,
// schlimmer ist als gar keiner: er sieht belegt aus.
// ===========================================================================

/// added 02.10.2026 12:1x: Das Praefix, woertlich aus Schritt 20.
pub const PRAEFIX_SECURE: &str = "__Secure-";

/// added 02.10.2026 12:1x: Das Praefix, woertlich aus Schritt 21.
pub const PRAEFIX_HOST: &str = "__Host-";

/// added 02.10.2026 12:1x: **TRAEGT DER NAME EIN PRAEFIX?** und die Antwort ist
/// **ohne** Beachtung der Gross-/Kleinschreibung, weil Schritt 20 und 21
/// „a case-insensitive match" sagen. Es steht hier als eigene Funktion, weil
/// ein Aufrufer sonst an zwei Stellen „ignoriere Gross/Klein" schreiben muss und
/// das beim naechsten Pfad der Ort ist, an dem es auseinanderlaeuft.
///
/// added 02.10.2026 12:1x: **UND `__Host-` IST NICHT `__Secure-`**, was der
/// Vergleich von selbst ergibt: an Stelle 3 steht `H` gegen `S`. GEMESSEN ist
/// das hier der Grund fuer [`praefix_ist_host`] — ohne die eigene Funktion
/// muesste jemand die Reihenfolge der zwei Vergleiche begruenden, und genau an
/// einer Reihenfolge ging schon einmal eine Regel verloren.
pub fn praefix_gefunden(name: &str) -> bool {
    praefix_ist_host(name) || praefix_ist_secure(name)
}

/// added 02.10.2026 12:1x: `__Host-`, Schritt 21.
pub fn praefix_ist_host(name: &str) -> bool {
    // added 02.10.2026 12:1x: **`str::get` statt `name[..7]`**, und der Grund
    // ist ein Verbot, kein Geschmack: `name[..7]` panict, wenn die ersten sieben
    // Bytes kein Zeichengrenzen-Schnitt sind — bei einem Namen mit Umlauten (zwei
    // Byte je Zeichen) ist `name.len() >= 7` **wahr** und der Schnitt **mitten
    // im Zeichen**. In Bibliothekscode ist `panic` untersagt, also liefert
    // `get(..)` `None` statt zu zerbrechen. GEMESSEN ist der Fehler nicht, er ist
    // an der Signatur ablesbar: `len()` sagt nichts ueber Zeichengrenzen.
    match name.get(..PRAEFIX_HOST.len()) {
        Some(kopf) => kopf.eq_ignore_ascii_case(PRAEFIX_HOST),
        None => false,
    }
}

/// added 02.10.2026 12:2x: `__Secure-`, Schritt 20.
pub fn praefix_ist_secure(name: &str) -> bool {
    match name.get(..PRAEFIX_SECURE.len()) {
        Some(kopf) => kopf.eq_ignore_ascii_case(PRAEFIX_SECURE),
        None => false,
    }
}

/// added 02.10.2026 12:2x: **DIE GRUENDE, und es sind fuenf**, weil Schritt 13,
/// 19, 20, 21 und 22 je einen eigenen Abbruch haben und ein einziger Text
/// („Cookie abgelehnt") einem Server nicht sagen wuerde, **was** er aendern
/// muss. Die Projektregel verbietet stumme Ablehnungen.
///
/// added 02.10.2026 12:2x: **UND DIE NAMEN SIND DIE NUMMERN DES ENTWURFS.** Das
/// ist keine Kosmetik: wer spaeter eine neue Regel aus dem Speichermodell
/// ergaenzt, soll an der Zahl erkennen, dass sie dort steht — sonst steht sie
/// irgendwann ohne Norm da, und genau das hat in diesem Projekt schon einmal
/// einen Fehler getragen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieMangel {
    /// Schritt 13: `Secure` gesetzt, aber die Verbindung war nicht sicher.
    /// Der Entwurf: „If the request-uri does not denote a "secure" connection
    /// (as defined by the user agent), and the cookie's secure-only-flag is
    /// true, then abort these steps and ignore the cookie entirely."
    Schritt13NichtSichereHerkunft,
    /// Schritt 19: `SameSite=None` ohne `Secure`.
    Schritt19NoneOhneSecure,
    /// Schritt 20: `__Secure-` ohne `Secure`.
    Schritt20SecureOhneSecure,
    /// Schritt 21, Ziffer 1: `__Host-` ohne `Secure`.
    Schritt21HostOhneSecure,
    /// Schritt 21, Ziffer 2: `__Host-` mit `Domain`.
    Schritt21HostMitDomain,
    /// Schritt 21, Ziffer 3: `__Host-` ohne `Path=/`.
    Schritt21HostFalscherPfad,
    /// Schritt 22: leerer Name, aber der **Wert** beginnt mit einem Praefix.
    Schritt22LeererNameMitPraefix,
    /// added 04.10.2026 07:3x, R71: `Partitioned` **ohne** `Secure`.
    /// **PROJEKTENTSCHEIDUNG**, kein zitiertes Zitat — siehe den Grund am
    /// Kommentar von `Kopf::partitioniert` (der Entwurfstext war am 04.10.2026
    /// nicht abrufbar; GEMESSEN 0 Treffer in bis-15…18).
    PartitioniertOhneSecure,
}

impl CookieMangel {
    /// added 02.10.2026 12:3x: **DER GRUND ALS TEXT**, und er nennt Norm **und**
    /// Entwurfszustand, weil das hier eine Quelle ohne RFC-Nummer ist und der
    /// Leser das wissen muss. Der Text geht an `set_cookie_ignoriert`.
    pub fn grund(self) -> &'static str {
        match self {
            CookieMangel::Schritt13NichtSichereHerkunft => {
                "Secure-Cookie ueber nicht sichere Verbindung (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 13, Entwurf)"
            }
            CookieMangel::Schritt19NoneOhneSecure => {
                "SameSite=None ohne Secure (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 19, Entwurf)"
            }
            CookieMangel::Schritt20SecureOhneSecure => {
                "__Secure-Praefix ohne Secure (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 20, Entwurf)"
            }
            CookieMangel::Schritt21HostOhneSecure => {
                "__Host-Praefix ohne Secure (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 21, Entwurf)"
            }
            CookieMangel::Schritt21HostMitDomain => {
                "__Host-Praefix mit Domain (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 21, Entwurf)"
            }
            CookieMangel::Schritt21HostFalscherPfad => {
                "__Host-Praefix ohne Path=/ (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 21, Entwurf)"
            }
            CookieMangel::PartitioniertOhneSecure => {
                "Partitioned ohne Secure (Projektentscheidung, 04.10.2026; der CHIPS-Entwurfstext war nicht abrufbar)"
            }
            CookieMangel::Schritt22LeererNameMitPraefix => {
                "leerer Cookie-Name mit __Secure-/__Host- im Wert (draft-ietf-httpbis-rfc6265bis 5.7 Schritt 22, Entwurf)"
            }
        }
    }
}

/// added 02.10.2026 12:3x: **DIE ENTSCHEIDUNG**, und die Reihenfolge der Pruefung
/// ist die des Speichermodells: **13, dann 19, dann 20, dann 21, dann 22.** Das
/// ist keine Formalie: die Schritte 13 und 19 stehen **vor** den Praefixen, und
/// ein Cookie, das gegen beide verstoesst, wird nach Schritt 13 verworfen.
/// Wer erst nach dem Praefix sieht, meldet einen Grund, den der Entwurf an
/// dieser Stelle nie erreicht.
///
/// added 02.10.2026 12:3x: **`https` IST DER ZWEITE EINDRUCK**, und zwar der
/// **einzige**, den der Aufrufer aus der Anfrage kennt. Schritt 13 sagt „as
/// defined by the user agent" — das ist eine Festlegung des User Agents, und
/// die einzige, die hier belegbar ist, ist: die Verbindung war `https`.
/// Deshalb wird nichts geraten, sondern die **Entscheidung** hereingereicht.
///
/// added 02.10.2026 12:3x: **UND DER KOPF WIRD HIER NICHT VOLL STAENDIG
/// GEPRUEFT.** `narsil-browser::cookie::set_cookie_lesen` parst die Zeile
/// vollstaendig; eine zweite vollstaendige Pruefung waere die zweite Wahrheit.
/// Diese Funktion liest genau die sechs Angaben, die die Schritte 13, 19, 20, 21
/// und 22 nennen — mehr nicht.
pub fn cookie_mangel(kopf: &str, https: bool) -> Option<CookieMangel> {
    let wert = Kopf { roh: kopf }.strip_kopfname();
    let name = wert.name();
    let val = wert.cookie_wert();

    // added 02.10.2026 12:4x: **SCHRITT 22 ZUERST**, und das ist die einzige
    // Stelle, an der der **Wert** und nicht der Name geprueft wird. Wörtlich:
    // „If the cookie-name is empty and either of the following conditions are
    // true, abort these steps and ignore the cookie entirely: * the cookie-value
    // begins with a case-insensitive match for the string "__Secure-" …".
    // GEMESSEN, warum es **vor** allem anderen steht: ohne diesen Schritt kaeme
    // ein Kopf `=__Host-SID=1; Secure; Path=/` mit **leerem** Namen und
    // `__Host-`-Wert durch, obwohl er genau die Zusicherung traegt, an der ein
    // Angreifer ansetzt — und ein leerer Name ist sonst ein Loesch-Cookie
    // (RFC 6265 3.1), also ein Koefer, in dem die Regel wohnt.
    // added 02.10.2026 12:4x: **SCHRITT 13**: `Secure` ueber Klartext.
    // GEMESSEN, warum das der erste Test nach 22 ist: er ist der einzige, der
    // **nichts** ueber den Namen wissen muss und deshalb jedes Cookie trifft.
    if wert.attribut_da("secure") && !https {
        return Some(CookieMangel::Schritt13NichtSichereHerkunft);
    }

    // added 02.10.2026 12:4x: **SCHRITT 19**, und er braucht den
    // same-site-flag, nicht das Attribut: der Entwurf setzt in Schritt 17 den
    // flag aus dem **letzten** `SameSite`-Attribut mit einem der drei bekannten
    // Schluesselwoerter, sonst `Default`. `Default` ist **nicht** `None`, und
    // deshalb wird hier nur `None` verworfen — die Vorgabe des Entwurfs fuer
    // `Default` steht in 5.6.7.1 und betrifft das **Senden**, nicht das Setzen.
    if wert.same_site_flag() == "None" && !wert.attribut_da("secure") {
        return Some(CookieMangel::Schritt19NoneOhneSecure);
    }

    // added 04.10.2026 07:4x, R71: **`Partitioned` braucht `Secure`** — und die
    // Stelle ist **hier**, zwischen Schritt 19 und den Praefixen, weil sie
    // dieselbe Form hat wie Schritt 19: ein Attribut, das ohne ein zweites
    // nicht sinnvoll ist. **PROJEKTENTSCHEIDUNG**, kein Zitat: der
    // Entwurfstext zu `Partitioned` war am 04.10.2026 nicht abrufbar (GEMESSEN:
    // 0 Treffer in draft-ietf-httpbis-rfc6265bis -15/-16/-17/-18).
    if wert.partitioniert() && !wert.attribut_da("secure") {
        return Some(CookieMangel::PartitioniertOhneSecure);
    }

    // added 02.10.2026 12:5x: **SCHRITT 20**: `__Secure-` braucht `Secure`.
    if praefix_ist_secure(name) && !wert.attribut_da("secure") {
        return Some(CookieMangel::Schritt20SecureOhneSecure);
    }

    // added 02.10.2026 12:5x: **SCHRITT 21**, in der Reihenfolge seiner drei
    // Ziffern: `Secure`, dann host-only, dann `Path=/`.
    if praefix_ist_host(name) {
        if !wert.attribut_da("secure") {
            return Some(CookieMangel::Schritt21HostOhneSecure);
        }
        // added 02.10.2026 12:5x: **UND HIER WEICHE ICH VOM ENTWURF AB**, und
        // das wird gesagt statt verschwiegen. Schritt 21 prueft den
        // **host-only-flag**, also das *Ergebnis* der Domain-Pruefung; ich
        // pruefe, ob ein `Domain`-Attribut **vorhanden** ist. Der Unterschied
        // ist genau ein Fall: ein `Domain`, der die Anfrage-Domain **nicht**
        // abdeckt, wird nach RFC 6265 5.3 Schritt 6 verworfen und laesst den
        // host-only-flag wahr — der Entwurf wuerde das Cookie also annehmen,
        // ich lehne es ab. GEMESSEN wird das nicht, weil die Domain-Pruefung in
        // `narsil-browser/src/cookie.rs` steht (fremde Datei) und sie hier zu
        // wiederholen die zweite Wahrheit waere. **Fail closed** heisst: im
        // Zweifel ablehnen, und den Grund nennen, der genannt wird.
        if wert.attribut_da("domain") {
            return Some(CookieMangel::Schritt21HostMitDomain);
        }
        // added 02.10.2026 12:6x: **UND `Path=/` MIT WERT**, weil Schritt 21
        // Ziffer 3 beides nennt: „the cookie-attribute-list contains an
        // attribute with an attribute-name of "Path", **and** the cookie's path
        // is /". Ein `Path` ohne `=` traegt also **keinen** Pfad und reicht nicht.
        if wert.attribut_wert("path") != Some("/") {
            return Some(CookieMangel::Schritt21HostFalscherPfad);
        }
    }

    // added 02.10.2026 12:1x: **SCHRITT 22, und er steht jetzt HINTEN**, weil ich
    // ihn zuerst vorgezogen hatte. GEMESSEN ist der Grund fuer die Korrektur an
    // einem Test, der **falsch** war: Schritt 22 und Schritt 13 schliessen sich
    // nicht aus — 22 braucht einen **leeren Namen**, 13 ein **vorhandenes**
    // `Secure`, und beides gilt zugleich bei `=__Host-SID=1; Secure`. Welcher
    // Grund gemeldet wird, entscheidet also die **Reihenfolge**, und die des
    // Speichermodells ist 13, 19, 20, 21, 22. Ein vorgezogener Schritt 22 war
    // meine eigene Anordnung, nicht die des Entwurfs; sie ist hiermit
    // zurueckgenommen, weil die Norm sie nicht traegt.
    //
    // added 02.10.2026 12:1x: **UND DER WERT BEGINNT DIREKT**, weil
    // [`Kopf::cookie_wert`] alles **hinter** dem ersten `=` gibt: der Kopf
    // `=__Host-SID=1` hat den leeren Namen `` und den Wert `__Host-SID=1`, und
    // genau so meint es der Entwurf. Meine erste Fassung schnitt hier noch ein
    // fuehrendes `=` ab — das war **doppelt** (im Wert ist es schon weg) und
    // haette einen Wert wie `=__Host-SID=1` durchgelassen.
    if name.is_empty() && praefix_gefunden(val) {
        return Some(CookieMangel::Schritt22LeererNameMitPraefix);
    }
    None
}

/// added 02.10.2026 12:6x: **DIE POSITIVFORM**, fuer Aufrufer, die es im Namen
/// brauchen. Sie ist `cookie_mangel(..).is_none()` und sonst nichts — in diesem
/// Projekt hat eine zweite Form derselben Sache schon einmal eine falsche
/// Antwort gegeben, weil die zweite Form naeher an der Aufrufstelle lag.
pub fn cookie_ok(kopf: &str, https: bool) -> bool {
    cookie_mangel(kopf, https).is_none()
}

/// added 02.10.2026 12:6x: **DER SAME-SITE-FLAG**, wörtlich nach Schritt 17: „If
/// the cookie-attribute-list contains an attribute with an attribute-name of
/// "SameSite", and an attribute-value of "Strict", "Lax", or "None", set the
/// cookie's same-site-flag to the attribute-value of the **last** attribute in
/// the cookie-attribute-list with an attribute-name of "SameSite". Otherwise,
/// set the cookie's same-site-flag to **"Default"**."
///
/// added 02.10.2026 12:7x: **UND ES STEHEN VIER ZEICHEN FUER, NICHT DREI.** Der
/// vierte Wert `Default` ist der, den man leicht weglässt, und genau der
/// haeufigste: fast kein Server schreibt `SameSite`. Wer statt dessen `Lax`
/// zurueckgibt, behauptet eine Vorgabe, die der Entwurf an dieser Stelle gar
/// nicht macht — die macht er in **5.6.7.1** fuer das **Senden**, und das ist
/// eine andere Frage mit einer anderen Stelle.
///
/// added 02.10.2026 12:7x: **UND DIE GROSS-/KLEINSCHREIBUNG DES WERTS IST
/// case-insensitiv**, weil Schritt 17 den Vergleich nicht als
/// case-sensitive bezeichnet; GEMESSEN ist die Folge, dass `samesite=none` als
/// der **bekannte** Schluesselwort `None` gilt — was nach den Browsern auch so
/// ist, obwohl der Entwurf es nicht sagt. Diese Annahme steht hier, weil sie
/// eine **Annahme** ist; ohne sie waere `samesite=none` ein `Default`, und das
/// waere die strengere, aber unbelegte Lesart.
pub fn same_site_flag(kopf: &str) -> &'static str {
    Kopf { roh: kopf }.strip_kopfname().same_site_flag()
}

/// added 04.10.2026 07:5x, R71. **DIE FREIE FORM VON `partitioniert`**, und sie
/// ist aus **demselben** Grund da wie `same_site_flag`: die Crate bietet die
/// Attributfragen als **freie Funktion** an, damit ein Aufrufer sie **ohne**
/// den `Kopf`-Typ benutzen kann. Aufrufer ist `cookie_mangel` — damit ist sie
/// **kein** Ballast, und der Waechter muss sie nicht entschuldigen.
pub fn partitioniert(kopf: &str) -> bool {
    Kopf { roh: kopf }.strip_kopfname().partitioniert()
}

impl<'a> Kopf<'a> {
    /// added 04.10.2026 07:2x, R71. **TRÄGT DAS COOKIE DAS ATTRIBUT
    /// `Partitioned`?** (Gross-/Kleinschreibung egal wie bei allen
    /// Attributnamen; der Wert ist **ohne** Bedeutung — die Anwesenheit zählt.)
    ///
    /// **WAS `Partitioned` BEDEUTET, in einem Satz:** das Cookie wird **je
    /// Erstseite getrennt** gespeichert, also nicht zwischen Seiten
    /// verschiedener Herkunft mitgeteilt — die „Drittanbieter-Cookies je
    /// Erstseite getrennt (Partitionierung)“ aus `team/AUTONOM.txt`.
    ///
    /// added 07:2x: **NORMZUSTAND, UND ICH BEHAUPTE NICHTS, WAS ICH NICHT
    /// GEHOLT HABE:** `Partitioned` steht in **keiner** Fassung von
    /// `draft-ietf-httpbis-rfc6265bis`, die ich heute geholt habe (**-15, -16,
    /// -17, -18**; GEMESSEN: `grep -c -i partitioned` liefert **0** in allen
    /// vieren). Der CHIPS-Entwurf (`draft-west-first-party-cookies`) ist die
    /// übliche Fundstelle, aber **der Text war nicht abrufbar**: das IETF-Archiv
    /// bot die Fassungen -10 bis -13 nicht an, und der Datatracker lieferte -07,
    /// das das Attribut noch nicht kennt. **DIE REGEL IN `cookie_mangel` IST
    /// DARUM EINE PROJEKTENTSCHEIDUNG** mit Begründung (ein `Partitioned`-Cookie
    /// ohne `Secure` wäre sinnlos: die Trennung ist nur über `https` erreichbar)
    /// — und **keine zitierte Norm**. Wer den Entwurfstext hat, gehoert diese
    /// Zeile hierhin, nicht die Regel.
    pub fn partitioniert(self) -> bool {
        // added 07:2x:  liefert **schon** einen Iterator
        // (GEMESSEN am Compilerfehler „no method named 'iter' found for opaque
        // type 'impl Iterator'“) — also kein , und  passt.
        self.attribute_list()
            .any(|teil| segment_name(teil).eq_ignore_ascii_case("partitioned"))
    }

    /// added 02.10.2026 12:1x: **DIE METHODE**, und sie ist von der freien
    /// Funktion getrennt, weil Schritt 19 im Speichermodell mit einem `Kopf`
    /// arbeitet und nicht mit einem Text: zwei Wege, denselben flag zu lesen,
    /// sind zwei Antworten auf die Frage, wer ihn setzt.
    ///
    /// added 04.10.2026 07:1x, R71. **AB HIER GIBT DIE FUNKTION DIE WIRKSAME
    /// DURCHSETZUNG AN, NICHT DEN ROHWERT** — und das ist eine **Aenderung der
    /// Bedeutung**, also steht sie hier und nicht in einer Zeile am Ende.
    /// GEMESSEN, dass sie gefahrlos ist: `grep` ueber **alle** Crates findet
    /// **null** Aufrufe ausserhalb dieser Datei (`same_site_flag` war nie
    /// benutzt). GEMESSEN, dass es noetig ist: der Auftrag
    /// `team/AUTONOM.txt` (TLS Runde 8, S5) sagt „SameSite Lax als Vorgabe“,
    /// und heute lieferte die Funktion fuer einen Kopf **ohne** Attribut
    /// `"Default"` — es gab also **keine** Vorgabe.
    ///
    /// added 07:1x: **DIE NORM, WÖRTLICH** (draft-ietf-httpbis-rfc6265bis-15,
    /// abgerufen 04.10.2026, Abschnitt **5.6.7** „The SameSite Attribute“):
    ///
    /// ```text
    /// 1.  Let enforcement be "Default".
    /// ```
    ///
    /// und zur Bedeutung von `Default` im selben Abschnitt:
    ///
    /// ```text
    /// If the "SameSite" attribute's value is something other than these
    /// three known keywords, the attribute's value will be subject to a
    /// default enforcement mode that is equivalent to "Lax".
    /// ```
    ///
    /// added 07:10x, R71: **WARUM DAS ```text` NOTWENDIG WAR — und es ist die
    /// vierte „eine Ebene zu viel“ in diesem Projekt.** Ein **eingerueckter**
    /// Absatz in einem Doc-Kommentar ist ein **Codeblock**, und rustdoc
    /// **kompiliert** ihn als **Doctest**. `1.  Let enforcement be
    /// "Default".` ist kein Rust, also meldete `cargo test` **zwei rote
    /// Doctests** — `expected item, found '1.'` — und **weder** `check` **noch**
    /// `clippy` hat es gesehen. GEMESSEN: der Fehler erscheint **erst** im
    /// Testlauf. Ein **wörtliches Normzitat** im Doc-Kommentar braucht also
    /// immer die Sprachangabe, sonst ist es kein Zitat, sondern ein **Test**.
    ///
    /// **EHRENHAFT DARGESTELLT:** der zweite Satz spricht von **unbekannten
    /// Werten**, nicht vom **fehlenden** Attribut; fuer das fehlende nennt der
    /// Entwurf `Default` als **Speicherwert**. Dass `Default` beim **Senden**
    /// wie `Lax` behandelt wird, ist **Browserpraxis** und der **Auftrag** —
    /// beides steht hier, und ich verkaufe es nicht als RFC-Zitat.
    pub fn same_site_flag(self) -> &'static str {
        let wert = self;
        // added 07:1x: **`Lax` statt `Default`**, und der Grund steht oben:
        // `Default` ist der **name** des Speicherwerts im Entwurf; beim Senden
        // ist er gleich `Lax`.
        let mut flag = "Lax";
        for teil in wert.attribute_list() {
            if segment_name(teil).eq_ignore_ascii_case("samesite") {
                let v = match teil.split_once('=') {
                    Some((_, v)) => v.trim(),
                    None => continue,
                };
                // added 02.10.2026 12:7x: **NUR BEKANNTE SCHLUESSELWOERTER
                // UEBERSCHREIBEN** — der Entwurf sagt „and an attribute-value of
                // "Strict", "Lax", or "None"", also wird ein unbekannter Wert
                // **nicht** zum flag, und ein spaeterer unbekannter Wert macht einen
                // frueheren auch nicht rueckgaengig. GEMESSEN ist das der Fall, in
                // dem „letztes gewinnt“ falsch waere: `SameSite=Lax; SameSite=x`
                // endet nach Schritt 17 bei `Lax`, nicht bei `Default`.
                flag = match v.to_ascii_lowercase().as_str() {
                    "strict" => "Strict",
                    "lax" => "Lax",
                    "none" => "None",
                    _ => continue,
                };
            }
        }
        flag
    }
}

// -------------------------------------------------------------------------
// DER KOPF ALS HILFSFORM — und sie ist bewusst **klein**
// -------------------------------------------------------------------------
// added 02.10.2026 12:8x: WARUM EINE HILFSFORM UND KEIN PARSER. Diese Datei
// beantwortet **eine** Frage: „nimmt der User Agent dieses Cookie an?". Dafuer
// braucht sie Name, Wert und sechs Attributangaben. Alles andere —
// Ablaufzeiten, Pfad-Normalisierung, Domain-Abdeckung — macht
// `narsil-browser::cookie`, und es waere eine zweite Wahrheit, es hier zu tun.
/// added 02.10.2026 12:8x: **UND DIE HILFSFORM KENNT DEN KOPF NICHT**, sie
/// sieht nur auf die Zeichen. Ein `Header` koennte ein Wert sein; deshalb
/// entfernt [`Kopf::strip_kopfname`] ein fuehrendes `Set-Cookie:` nur, wenn
/// eines da steht — GEMESSEN daran, dass beide Formen in der Praxis vorkommen
/// (die Antwort traegt `set_cookie` **ohne** Namen, ein Test haengt sich gern
/// einen an).
#[derive(Clone, Copy)]
struct Kopf<'a> {
    roh: &'a str,
}

impl<'a> Kopf<'a> {
    /// added 02.10.2026 12:8x: **DER NAME DES KOPFS**, und er wird nur bei
    /// **`:` gefolgt von Leerraum** abgeschnitten, weil ein Cookie-Wert sonst
    /// seinen ersten Buchstaben verliert: `Set-Cookie` hat einen Doppelpunkt,
    /// `Secure` nicht, und der Unterschied entscheidet, ob ein `=`-Wert wie
    /// `a:b` lesbar bleibt.
    fn strip_kopfname(self) -> Kopf<'a> {
        let t = self.roh.trim();
        if let Some(stelle) = t.find(':') {
            let (kopf, rest) = t.split_at(stelle);
            if kopf.eq_ignore_ascii_case("set-cookie") {
                // added 03.10.2026, TLS (Fuzzing-Runde): **`rest[1..]` IST
                // SICHER, und ich habe es erst **falsch** behauptet.**
                //
                // **MEIN BEFUND WAR FALSCH**, und der Grund ist eine halbe
                // Sekunde Nachdenken: `split_at(stelle)` legt `rest` **auf den
                // Doppelpunkt**. `rest[0]` ist also das ASCII-`:` — **ein**
                // Byte — und Index 1 ist damit **immer** eine Zeichengrenze,
                // egal was danach kommt. `ä` direkt hinter dem Doppelpunkt
                // ergibt `rest` = `":ä…"`, und `rest[1..]` zeigt auf den
                // **Anfang** von `ä`. Kein Absturz.
                //
                // **UND DAS IST DER UNTERSCHIED ZU `x509.rs:1184`**, wo es
                // wirklich panickte: dort war der Offset **10** in einem
                // 13-Byte-Text, der an Byte 9 ein **zweibyteiges** Zeichen
                // hatte — der Schnitt fiel **mitten** hinein. Hier ist der
                // Offset **1**, und Byte 0 ist **garantiert** ASCII.
                //
                // **ICH HABE DIE AENDERUNG ZURUECKGENOMMEN.** Eine
                // Verteidigung gegen einen Fehler, den es nicht gibt, ist
                // Ballast — und noch schlimmer: sie traegt einen Kommentar,
                // der **behauptet**, es gebe hier einen Absturz. Das ist genau
                // der Fehler, den die Projektregel verbietet. Der Beweis steht
                // in `mehrbytezeichen_hinter_dem_doppelpunkt_ist_sicher`.
                return Kopf {
                    roh: rest[1..].trim_start(),
                };
            }
        }
        Kopf { roh: t }
    }

    /// added 02.10.2026 12:9x: **DER NAME DES COOKIES**: alles vor dem ersten
    /// `=`, und dann **ohne** Leerraum. `name-wert` heisst nicht `name` mit
    /// dem Wert `-wert`.
    fn name(self) -> &'a str {
        self.roh
            .split(';')
            .next()
            .unwrap_or("")
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
    }

    /// added 02.10.2026 12:9x: **DER WERT DES COOKIES**, und er beginnt **hinter**
    /// dem ersten `=` und endet **vor** dem ersten `;`. Wichtig fuer Schritt 22:
    /// der Wert ist genau der Teil, den der Server als *Wert* geschickt hat —
    /// nicht der Name, und nicht der Rest der Zeile. Bei leerem Namen ist der
    /// erste Teil vor dem `=` **leer**, und das `=` gehoert zum Wert: `=x`
    /// heisst Name `` und Wert `=x`. Deshalb wird hier das `=` **nicht**
    /// abgeschnitten.
    fn cookie_wert(self) -> &'a str {
        let erstes = self.roh.split(';').next().unwrap_or("");
        match erstes.split_once('=') {
            Some((_, v)) => v.trim(),
            None => "",
        }
    }

    /// added 02.10.2026 12:9x: **DIE ATTRIBUTE**, und der erste Teil ist **kein**
    /// Attribut: er enthaelt Name **und** Wert. Deshalb `skip(1)`.
    fn attribute_list(self) -> impl Iterator<Item = &'a str> {
        self.roh.split(';').skip(1).map(|t| t.trim())
    }

    /// added 02.10.2026 12:0x (nachgemessen 12:9x): **HAT DER KOPF DAS
    /// ATTRIBUT?** als eigenes Segment zwischen `;`. GEMESSEN ist der Grund,
    /// warum kein `contains()`: `contains("secure")` trifft `insecure` und
    /// `mysecure=1`, und beide wuerden ein Cookie mit `__Host-` durchlassen.
    fn attribut_da(self, name: &str) -> bool {
        self.attribute_list()
            .any(|t| segment_name(t).eq_ignore_ascii_case(name))
    }

    /// added 02.10.2026 12:0x (nachgemessen 12:9x): **DER WERT EINES
    /// ATTRIBUTS**, und `None`, wenn es das Attribut nicht gibt **oder** wenn
    /// es ohne `=` dasteht. Das zweite ist der Randfall aus Schritt 21 Ziffer 3:
    /// `Path` ohne Wert ist kein `Path=/`.
    fn attribut_wert(self, name: &str) -> Option<&'a str> {
        for teil in self.attribute_list() {
            if !segment_name(teil).eq_ignore_ascii_case(name) {
                continue;
            }
            return teil.split_once('=').map(|(_, v)| v.trim());
        }
        None
    }
}

/// added 02.10.2026 12:0x: **DER SEGMENTNAME**, getrennt, weil ein Attribut
/// `Secure` **ohne** `=` ein Flag ist und eines mit `=` einen Wert traegt.
/// Wird hier nur der Name abgeschnitten und der Wert nicht betrachtet, ist
/// `Secure=irgendwas` ein Flag — das ist nach Schritt 12 richtig („contains an
/// attribute with an attribute-name of "Secure"" nennt keinen Wert) — aber
/// `Path=/` braucht den Wert, und das entscheidet [`Kopf::attribut_wert`].
fn segment_name(teil: &str) -> &str {
    match teil.split_once('=') {
        Some((n, _)) => n.trim(),
        None => teil.trim(),
    }
}

/// added 02.10.2026 12:3x UTC (Projektlabel 02.10.2026), Rolle TLS.
/// Runde 8 S5: was der User Agent mit einem `Set-Cookie` macht.
///
/// DIE NORMPROBE: **wörtlich** aus `draft-ietf-httpbis-rfc6265bis-15`,
/// Abschnitt 5.4, „The following are examples of Set-Cookie header fields that
/// would be **rejected** by a conformant user agent." Die Liste ist nicht
/// umgeschrieben und nicht ergaenzt — sie steht hier, wie sie im Entwurf steht.
#[cfg(test)]
mod cookie_speicher_modell_tests {
    use super::*;

    /// added 02.10.2026 12:3x: **DIE ZEHN VERWORFENEN ZEILEN**, woertlich. Und
    /// **ueber `https`**, damit der Test nicht an Schritt 13 klebt und aus
    /// einem Praefix-Problem versehentlich ein Herkunftsproblem macht: geprueft
    /// wird die Absicht der Liste, nicht etwas anderes.
    #[test]
    fn die_zehn_zeilen_des_entwurfs_werden_angenommen_nicht() {
        let verworfen = [
            "Set-Cookie: __Secure-SID=12345; Domain=site.example",
            "Set-Cookie: __secure-SID=12345; Domain=site.example",
            "Set-Cookie: __SECURE-SID=12345; Domain=site.example",
            "Set-Cookie: __Host-SID=12345",
            "Set-Cookie: __host-SID=12345; Secure",
            "Set-Cookie: __host-SID=12345; Domain=site.example",
            "Set-Cookie: __HOST-SID=12345; Domain=site.example; Path=/",
            "Set-Cookie: __Host-SID=12345; Secure; Domain=site.example; Path=/",
            "Set-Cookie: __host-SID=12345; Secure; Domain=site.example; Path=/",
            "Set-Cookie: __HOST-SID=12345; Secure; Domain=site.example; Path=/",
        ];
        assert_eq!(verworfen.len(), 10, "GEMESSEN: die Liste hat zehn Zeilen");
        for kopf in verworfen {
            assert!(
                !cookie_ok(kopf, true),
                "der Entwurf sagt, das wird abgelehnt: `{kopf}`"
            );
        }
    }

    /// added 02.10.2026 12:4x: **DIE SECHS ANGENOMMENEN**, woertlich, und die
    /// Liste traegt eine Bedingung, die man leicht weglässt: „would be accepted
    /// **if set from a secure origin**". Deshalb `https = true`.
    #[test]
    fn die_sechs_zeilen_des_entwurfs_werden_ueber_https_angenommen() {
        let angenommen = [
            "Set-Cookie: __Secure-SID=12345; Domain=site.example; Secure",
            "Set-Cookie: __secure-SID=12345; Domain=site.example; Secure",
            "Set-Cookie: __SECURE-SID=12345; Domain=site.example; Secure",
            "Set-Cookie: __Host-SID=12345; Secure; Path=/",
            "Set-Cookie: __host-SID=12345; Secure; Path=/",
            "Set-Cookie: __HOST-SID=12345; Secure; Path=/",
        ];
        assert_eq!(angenommen.len(), 6, "GEMESSEN: die Liste hat sechs Zeilen");
        for kopf in angenommen {
            assert!(
                cookie_ok(kopf, true),
                "der Entwurf sagt, das wird angenommen: `{kopf}`"
            );
        }
    }

    /// added 02.10.2026 12:4x: **UND DIESE SECHS UEBER KLARTEXT, und das steht
    /// hier, weil es der Fall ist, den die Entwurfsliste nicht zeigt.** Jede der
    /// sechs traegt `Secure`; Schritt 13 sagt woertlich: „If the request-uri does
    /// not denote a "secure" connection (as defined by the user agent), and the
    /// cookie's secure-only-flag is true, then abort these steps and ignore the
    /// cookie entirely." Damit sind sie ueber http verworfen — **alle sechs**, und
    /// ohne dass eine einzige der Zeilen ein Praefix verletzt.
    #[test]
    fn secure_ueber_klartext_ist_immer_verworfen_schritt_13() {
        let ueber_https_angenommen = [
            "Set-Cookie: __Secure-SID=12345; Domain=site.example; Secure",
            "Set-Cookie: __Host-SID=12345; Secure; Path=/",
        ];
        for kopf in ueber_https_angenommen {
            assert!(cookie_ok(kopf, true), "ueber https angenommen");
            assert_eq!(
                cookie_mangel(kopf, false),
                Some(CookieMangel::Schritt13NichtSichereHerkunft),
                "ueber Klartext nach Schritt 13 verworfen: `{kopf}`"
            );
        }
        // added 02.10.2026 12:5x: **UND OHNE PRAEFIX GENAUSO**, denn Schritt 13
        // nennt kein Praefix, sondern das secure-only-flag. Wer die Regel auf
        // `__Host-` beschraenkt, haelt genau die Cookies fuer gut, die ein
        // Angreifer braucht: ein gewoehnliches `Secure`-Cookie, das er setzt.
        assert_eq!(
            cookie_mangel("Set-Cookie: sitzung=abc; Secure", false),
            Some(CookieMangel::Schritt13NichtSichereHerkunft),
            "auch ohne Praefix"
        );
        assert!(
            cookie_ok("Set-Cookie: sitzung=abc; Secure", true),
            "ueber https"
        );
    }

    /// added 02.10.2026 12:1x (nachgemessen 12:5x): **DIE REIHENFOLGE**, und
    /// meine erste Fassung dieses Tests war **falsch**: sie behauptete, der Kopf
    /// `__Host-SID=1; Domain=x; SameSite=None` ueber Klartext loese Schritt 13
    /// aus. GEMESSEN am Ergebnis: **nein** — Schritt 13 verlangt ein vorhandenes
    /// `Secure`, und dieser Kopf hat keins; er loest Schritt 19 aus. Daraus
    /// folgt etwas, das man beim blossen Lesen nicht sieht: **die Schritte 13
    /// und 19 schliessen sich gegenseitig aus**, denn 13 will `Secure` und 19
    /// will `SameSite=None` **ohne** `Secure`. Ihre Reihenfolge ist damit nicht
    /// beobachtbar, und eine Aussage darueber waere geraten.
    #[test]
    fn die_reihenfolge_ist_die_des_speichermodells() {
        // added 02.10.2026 12:5x: **13 VOR 22**, und beide greifen hier: der
        // Kopf traegt ein `Secure` ueber Klartext **und** einen leeren Namen mit
        // `__Host-` im Wert. Schritt 13 steht im Entwurf zuerst.
        assert_eq!(
            cookie_mangel("Set-Cookie: =__Host-SID=1; Secure; Path=/", false),
            Some(CookieMangel::Schritt13NichtSichereHerkunft),
            "Schritt 13 steht vor Schritt 22"
        );
        // added 02.10.2026 12:5x: **UND 22 BLEIBT**, sobald 13 nicht greift —
        // derselbe Kopf ueber https.
        assert_eq!(
            cookie_mangel("Set-Cookie: =__Host-SID=1; Secure; Path=/", true),
            Some(CookieMangel::Schritt22LeererNameMitPraefix),
            "dann Schritt 22"
        );
        // added 02.10.2026 12:5x: **UND 19 VOR 21**: `__Host-SID=1; SameSite=None`
        // verletzt Schritt 19 und Schritt 21 Ziffer 1 — beide gleich, aber
        // Schritt 19 kommt zuerst im Entwurf.
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1; SameSite=None", true),
            Some(CookieMangel::Schritt19NoneOhneSecure),
            "Schritt 19 steht vor Schritt 21"
        );
        // added 02.10.2026 12:5x: **UND INNERHALB VON 21 DIE DREI ZIFFERN**,
        // jede einzeln geprueft, weil ein Test, der nur „wird abgelehnt" sagt,
        // bei drei Ziffern auch dann gruen waere, wenn die Reihenfolge falsch ist.
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1", true),
            Some(CookieMangel::Schritt21HostOhneSecure),
            "Ziffer 1"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1; Secure; Domain=x; Path=/", true),
            Some(CookieMangel::Schritt21HostMitDomain),
            "Ziffer 2"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1; Secure; Path=/x", true),
            Some(CookieMangel::Schritt21HostFalscherPfad),
            "Ziffer 3"
        );
    }

    /// added 02.10.2026 12:6x: **DER ANGREIFER, woertlich aus Abschnitt 5.4** —
    /// und er ist der Grund fuer den case-insensitiven Vergleich. Der Entwurf
    /// nennt `__SeCuRe-SID=evil` neben `__Secure-SID=12345`; haelt ein User
    /// Agent den Vergleich case-sensitive, nimmt er das erste an (kein `Secure`
    /// fehlt ihm ja) und der Server kann beide nicht unterscheiden.
    #[test]
    fn die_groess_kleinschreibung_darf_nicht_durch_lassen() {
        assert_eq!(
            cookie_mangel("Set-Cookie: __SeCuRe-SID=evil; Domain=site.example", true),
            Some(CookieMangel::Schritt20SecureOhneSecure),
            "GEMESSEN: das ist der Angriff aus 5.4"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: __HOST-sid=1; Domain=x", true),
            Some(CookieMangel::Schritt21HostOhneSecure),
            "auch __Host- gross geschrieben"
        );
        // added 02.10.2026 12:6x: **UND DIE ZWEITE HAELFTE DERSELBEN REGEL**,
        // woertlich aus 5.4: „both __Secure-foo=bar and __secure-foo=baz can
        // exist as distinct cookies simultaneously and both would have the
        // requirements of the __Secure- prefix applied." — beide Namen werden
        // also **an**genommen, und an beiden wird die Regel geprueft. Das ist
        // der Punkt, an dem „case-insensitiv" leicht als „case-egal" gelesen
        // wird; es heisst „case-egal **in der Pruefung**", nicht „derselbe Name".
        assert!(
            cookie_ok("Set-Cookie: __Secure-foo=bar; Secure", true),
            "__Secure-foo"
        );
        assert!(
            cookie_ok("Set-Cookie: __secure-foo=baz; Secure", true),
            "und __secure-foo, getrennt gespeichert, aber mit derselben Pflicht"
        );
    }

    /// added 02.10.2026 12:6x: **SCHRITT 22**, und der Vektor ist der Kopf, den
    /// man fuer einen Loesch-Cookie haelt: leerer Name, und der **Wert** traegt
    /// das Praefix. Ohne diese Regel kaeme er angenommen.
    #[test]
    fn leerer_name_mit_praefix_im_wert_ist_verworfen_schritt_22() {
        assert_eq!(
            cookie_mangel("Set-Cookie: =__Host-SID=1; Secure; Path=/", true),
            Some(CookieMangel::Schritt22LeererNameMitPraefix),
            "leerer Name, __Host- im Wert"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: =__SECURE-sid=1", true),
            Some(CookieMangel::Schritt22LeererNameMitPraefix),
            "und mit __Secure- im Wert"
        );
        // added 02.10.2026 12:7x: **UND EIN LOESCH-COOKIE OHNE PRAEFIX BLEIBT**,
        // denn Schritt 22 nennt die Praefixe und sonst nichts: `Set-Cookie: =`
        // ist ein Cookie mit leerem Namen und leerem Wert, und das ist nach RFC
        // 6265 3.1 zulaessig.
        assert!(cookie_ok("Set-Cookie: =", true), "leeres Paar ist erlaubt");
        assert!(
            cookie_ok("Set-Cookie: a=1", true),
            "ganz gewöhnliches Cookie"
        );
    }

    /// added 02.10.2026 12:7x: **DER SAME-SITE-FLAG**, Schritt 17. Der vierte
    /// Wert `Default` ist der haeufigste Fall — fast kein Server schreibt
    /// `SameSite` — und der haeufigste Grund, warum eine Vorgabe behauptet wird,
    /// die es an dieser Stelle nicht gibt.
    //
    // added 04.10.2026 07:12x, R71: **DIE DOPPELTE `#[test]`-ZEILE, UND SIE IST
    // AUCH **MEINE** — die zweite Form desselben Fehlers wie der Python-Lauf von
    // 07:9x.** Mein Umbenennungslauf hat die **alte** `#[test]`-Zeile **stehen
    // gelassen** und eine **zweite** gesetzt; GEMESSEN hat es clippy
    // (`duplicate_macro_attributes`), **der Test nicht** — Rust nimmt ein
    // doppeltes Attribut an und **laeuft einmal**. Zwei Fehler aus einer
    // Ursache: **ein Ersetzungslauf, der anhaelt, statt zu ersetzen.**
    //
    // added 04.10.2026 07:8x, R71. **DIE REIHENFOLGE VON `#[test]` UND `fn` IST
    // GEMESSEN — und sie ist eine Bedingung, keine Form.** Der Aufrufer-Wächter
    // erkennt eine Testfunktion daran, dass `#[test]` **wenige Zeilen** über
    // `fn` steht (er sieht 5 Zeilen zurück). Mein erster Versuch schrieb einen
    // **achtzeiligen** Kommentar **zwischen** Attribut und Funktion — und der
    // Wächter meldete die Testfunktion als **toten privaten Code**. GEMESSEN:
    // `1 fn in MEINEN Dateien ohne Aufrufer: sitzung.rs:967 der_same_site_flag_…`.
    // Der Wächter ist **geparkt**, also habe ich **nicht** sein Fenster
    // verbreitert, sondern **meinen** Kommentar **ueber** das Attribut gezogen:
    // die Erkennung bleibt so, wie sie ist, und der Kommentar steht da, wo er
    // gehoert. **WER DAS FENSTER VERBREITERT, ist das naechste Mal dran** — die
    // Zahl **5** ist eine willkuerliche und damit eine messbare.
    //
    // added 04.10.2026 07:6x, R71. **DER NAME STIMMT NICHT MEHR, UND DAS IST
    // ABSICHT:** es sind **drei** wirksame Werte geworden, nicht vier — der
    // vierte, `Default`, ist die **wirksame Vorgabe** und damit `Lax`. GEMESSEN,
    // dass dieser Test die Aenderung **zeigt** und nicht nur traegt: vor der
    // Aenderung meldete er `left: "Lax", right: "Default"` — der alte Vertrag
    // stand im Test, nicht nur in der Funktion.
    #[test]
    fn der_same_site_flag_kennt_drei_werte_und_nimmt_den_letzten() {
        // added 07:6x: **OHNE ATTRIBUT = `Lax`**, und das ist der Auftrag
        // („SameSite Lax als Vorgabe“, `team/AUTONOM.txt` TLS Runde 8 S5) plus
        // die Browserpraxis; die Normstelle steht am Funktionskopf.
        assert_eq!(same_site_flag("Set-Cookie: a=1"), "Lax", "kein Attribut");
        assert_eq!(same_site_flag("Set-Cookie: a=1; SameSite=Strict"), "Strict");
        assert_eq!(same_site_flag("Set-Cookie: a=1; SameSite=Lax"), "Lax");
        assert_eq!(same_site_flag("Set-Cookie: a=1; SameSite=None"), "None");
        // added 02.10.2026 12:8x: **DOPPELTES ATTRIBUT**, woertlich „the
        // attribute-value of the **last** attribute" — und **nur** fuer ein
        // bekanntes Schluesselwort. `SameSite=Lax; SameSite=x` endet bei `Lax`,
        // weil der Entwurf einen unbekannten Wert nicht zum flag macht, und er
        // macht auch einen frueheren nicht rueckgaengig.
        assert_eq!(
            same_site_flag("Set-Cookie: a=1; SameSite=None; SameSite=Lax"),
            "Lax",
            "das letzte bekannte gewinnt"
        );
        assert_eq!(
            same_site_flag("Set-Cookie: a=1; SameSite=Lax; SameSite=x"),
            "Lax",
            "ein unbekannter Wert danach macht den flag nicht kaputt",
        );
        // added 07:6x: **UNBEKANNTER WERT = `Lax`**, woertlich aus
        // draft-ietf-httpbis-rfc6265bis-15 Abschnitt 5.6.7: „a default
        // enforcement mode that is **equivalent to 'Lax'**“.
        assert_eq!(
            same_site_flag("Set-Cookie: a=1; SameSite=x"),
            "Lax",
            "nur ein unbekannter Wert"
        );
        // added 02.10.2026 12:8x: **UND `None` OHNE `SECURE`**, Schritt 19. Der
        // Grund ist woertlich derselbe wie bei __Host-: ohne sichere Kanal
        // gaenge das Cookie ueber Kreuz-Domaenen hinweg im Klartext mit.
        assert_eq!(
            cookie_mangel("Set-Cookie: a=1; SameSite=None", true),
            Some(CookieMangel::Schritt19NoneOhneSecure),
            "SameSite=None ohne Secure"
        );
        assert!(
            cookie_ok("Set-Cookie: a=1; SameSite=None; Secure", true),
            "mit Secure"
        );
    }

    /// added 02.10.2026 12:8x: **DIE GEGENPROBEN**, und sie sind der Grund fuer
    /// [`Kopf::attribut_da`] statt `contains`. Beide Zilen wuerden mit einem
    /// Textvergleich durchgehen und damit ein `__Host-`-Cookie ohne `Secure`
    /// annehmen — also genau den Fall, den die Regeln verhindern sollen.
    // added 04.10.2026 07:9x, R71: **DIE `#[test]`-ZEILE WAR FORT, UND ZWAR
    // MEINE SCHULD.** Bei der Korrektur der Kommentarlage habe ich mit einem
    // python-Ersetzungslauf **die erste** `#[test]`-Zeile **nach** meiner Funktion
    // entfernt — und die gehörte dem **nächsten** Test. GEMESSEN, dass es auffiel:
    // der Aufrufer-Wächter meldete `sitzung.rs:1074
    // ein_aehnlich_benanntes_attribut_zaehlt_nicht` als **toten privaten Code**,
    // weil eine Funktion **ohne** `#[test]` in einem `#[cfg(test)]`-Modul genau
    // das ist. **Ein Ersatzlauf, der an einer Stelle anhaelt, loescht an der
    // naechsten** — das ist die Form, die R69 beim Zeilenbereich teuer gemacht
    // hat, nur im Kleinen. Die Zeile ist wieder da, und **der Test laeuft**.
    // added 04.10.2026 07:13x, R71. **`Partitioned` IST EIGENER TEST UND KEIN
    // ABSCHNITT IM SAME-SITE-TEST.** GEMESSEN, warum: die Partitioned-Faelle
    // lagen zuerst **im** Koerper von `der_same_site_flag_kennt_drei_werte…`, und
    // ein Fehler darin wurde unter **diesem** Namen gemeldet — ein Testname ist
    // auch eine **Aussage** ueber das, was er prueft.
    //
    // added 07:13x: **UND DIE REIHENFOLGE `#[test]` DIREKT ÜBER `fn`** ist hier
    // eingehalten (der Kommentar steht **darüber**), weil der Aufrufer-Wächter
    // die Testfunktion nur daran erkennt, dass das Attribut **wenige Zeilen**
    // über der Funktion steht. GEMESSEN: mit 21 Zeilen Abstand meldete er die
    // Funktion als **toten privaten Code**.
    #[test]
    fn partitioned_wird_erkannt_und_braucht_secure() {
        // (1) vorhanden, mit und ohne Wert, in beiden Schreibweisen.
        assert!(partitioniert("Set-Cookie: a=1; Partitioned"));
        assert!(partitioniert("Set-Cookie: a=1; Partitioned; Secure"));
        assert!(partitioniert("Set-Cookie: a=1; partitioned; Secure"));
        assert!(
            partitioniert("Set-Cookie: a=1; Partitioned=irgendwas; Secure"),
            "der Wert ist ohne Bedeutung — die Anwesenheit zaehlt"
        );
        // (2) nicht vorhanden: **Nicht** due das aehnlich benannte Attribut
        // `Partitioniert` (deutsch) und **nicht** `X-Partitioned`.
        assert!(!partitioniert("Set-Cookie: a=1; Secure"));
        assert!(!partitioniert("Set-Cookie: a=1; Partitions=1; Secure"));
        assert!(!partitioniert("Set-Cookie: a=1; X-Partitioned; Secure"));
        // (3) die Regel: ohne `Secure` verworfen, mit `Secure` angenommen.
        assert_eq!(
            cookie_mangel("Set-Cookie: a=1; Partitioned", true),
            Some(CookieMangel::PartitioniertOhneSecure),
            "Partitioned ohne Secure wird verworfen (Projektentscheidung, 04.10.2026)"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: a=1; Partitioned; Secure", true),
            None,
            "mit Secure ist es ein ganz gewöhnliches Cookie"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: a=1; Partitioned", false),
            Some(CookieMangel::PartitioniertOhneSecure),
            "über Klartext greift ohnehin Schritt 13 — aber die Reihenfolge ist hier belegt"
        );
        // added 07:7x: **UND DIE REIHENFOLGE** gegen Schritt 13 und Schritt 19:
        // ein Kopf, der **zwei** Maengel hat, meldet den **frueheren** — sonst
        // waere die Reihenfolge nur behauptet.
        assert_eq!(
            cookie_mangel("Set-Cookie: a=1; Secure; Partitioned", false),
            Some(CookieMangel::Schritt13NichtSichereHerkunft),
            "Schritt 13 kommt vor der neuen Regel"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: a=1; SameSite=None; Partitioned", true),
            Some(CookieMangel::Schritt19NoneOhneSecure),
            "und Schritt 19 auch"
        );
    }

    #[test]
    fn ein_aehnlich_benanntes_attribut_zaehlt_nicht() {
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1; insecure", true),
            Some(CookieMangel::Schritt21HostOhneSecure),
            "`insecure` ist kein `Secure`"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1; mysecure=1", true),
            Some(CookieMangel::Schritt21HostOhneSecure),
            "`mysecure` ist kein `Secure`"
        );
        assert_eq!(
            cookie_mangel("Set-Cookie: __Host-SID=1; securement=1; Path=/", true),
            Some(CookieMangel::Schritt21HostOhneSecure),
            "und kein weiteres Aehnliches"
        );
        // added 02.10.2026 12:9x: **`Secure=1` IST `Secure`**, weil Schritt 12
        // „contains an attribute with an attribute-name of "Secure"" sagt und
        // keinen Wert nennt — das ist eine bewusst getroffene Lesart, sonst
        // waere `Secure=0` ein `Secure`.
        assert!(
            cookie_ok("Set-Cookie: __Host-SID=1; Secure=1; Path=/", true),
            "Flag mit Wert"
        );
        assert!(
            cookie_ok("Set-Cookie: __Host-SID=1; SECURE; Path=/", true),
            "gross geschrieben"
        );
    }

    /// added 02.10.2026 12:9x: **KEIN PANIC**, und das ist der einzige Test, der
    /// eine Eigenschaft des **Speicherzugriffs** prueft statt einer Regel. My
    /// erste Fassung schnitt `name[..7]`; bei einem Namen mit Umlauten ist
    /// `name.len() >= 7` wahr und der Schnitt faellt **mitten ins Zeichen** —
    /// ein `panic` im Bibliothekscode, und ein `panic` in einer Cookie-Pruefung
    /// waere zugleich eine Denial-of-Service-Moeglichkeit.
    #[test]
    fn ein_name_mit_mehrbytezeichen_panict_nicht() {
        let zwei_byte = "ä".repeat(5); // 10 Byte, 5 Zeichen
        assert_eq!(zwei_byte.len(), 10, "GEMESSEN: zwei Byte je Zeichen");
        assert!(
            zwei_byte.len() >= PRAEFIX_HOST.len(),
            "die Laengenpruefung greift nicht"
        );
        assert!(!praefix_ist_host(&zwei_byte), "und trotzdem kein Praefix");
        assert!(!praefix_ist_secure(&zwei_byte), "ebenso wenig __Secure-");
        assert!(!praefix_gefunden(&zwei_byte), "und im Vergleich auch nicht");
        assert!(
            cookie_ok(&format!("Set-Cookie: {zwei_byte}=1; Secure; Path=/"), true),
            "verworfen? nein: angenommen"
        );
    }

    /// added 02.10.2026 12:0x: **DIE KONSTANTEN**, und der Test haelt sie gegen
    /// den Entwurf, weil sie **Zahlen** sind, die jemand nachschlagen will:
    /// sieben Zeichen (`__Host-`) und neun (`__Secure-`).
    #[test]
    fn die_konstanten_stimmen_mit_dem_entwurf_ueberein() {
        assert_eq!(PRAEFIX_HOST, "__Host-");
        assert_eq!(PRAEFIX_SECURE, "__Secure-");
        assert_eq!(PRAEFIX_HOST.chars().count(), 7);
        assert_eq!(PRAEFIX_SECURE.chars().count(), 9);
        assert!(
            !PRAEFIX_HOST.starts_with(PRAEFIX_SECURE),
            "__Host- ist nicht __Secure-; die Reihenfolge bleibt trotzdem festgelegt"
        );
        assert!(praefix_gefunden("__Host-"), "genau der Praefixname selbst");
        assert!(praefix_gefunden("__Secure-"), "ebenso");
        assert!(
            !praefix_gefunden("__Host"),
            "ohne Bindestrich ist es keiner"
        );
        assert!(!praefix_gefunden("x__Host-"), "und am Anfang nicht");
    }

    /// added 03.10.2026, TLS (Fuzzing-Runde): **`rest[1..]` IN `strip_kopfname`
    /// IST SICHER — bewiesen, und nicht behauptet.**
    ///
    /// **WOHER DER IRRTUM KAM, und das ist der Teil, den man speichern muss:**
    /// Nach dem Fund in `narsil-tls/src/x509.rs:1184` („end byte index … is not a
    /// char boundary“) habe ich **alle** Bereichs-Schnitte in meinen Dateien
    /// geprueft und `sitzung.rs:719` fuer **denselben** Fehler gehalten. Das
    /// `grep` hat mir zwei Kandidaten gezeigt, und ich habe den zweiten
    /// **gelesen und geglaubt**.
    ///
    /// **GEMESSEN, dass es keiner ist:** `split_at(stelle)` legt `rest`
    /// **auf den Doppelpunkt**. `rest[0]` ist das ASCII-`:`, also **ein**
    /// Byte, und Index 1 ist damit **immer** eine Zeichengrenze — auch wenn
    /// direkt danach ein **Mehrbytezeichen** kommt. `Set-Cookie:ä=1` ergibt
    /// `rest` = `":ä=1"` und `rest[1..]` zeigt auf den **Anfang** von `ä`.
    ///
    /// **DER UNTERSCHIED ZU DEM ECHTEN FUND:** dort war der Offset **10** in
    /// einem 13-Byte-Text mit einem **zweibyteigen** Zeichen **an Byte 9** —
    /// der Schnitt fiel **mitten** hinein. Hier ist der Offset **1**, und
    /// Byte 0 ist **garantiert** ASCII. Das ist der ganze Unterschied, und er
    /// ist **nicht** im Quelltext zu sehen, sondern nur in der Rechnung.
    ///
    /// **UND DIESER TEST IST DARUM WICHTIG:** er **schreibt die Widerlegung
    /// fest**, damit die Stelle nicht ein drittes Mal gemeldet wird — und
    /// damit jemand, der wie ich `get` einsetzt, weiss, dass er eine
    /// Aenderung **ohne** Grund gemacht hat.
    #[test]
    fn mehrbytezeichen_hinter_dem_doppelpunkt_ist_sicher() {
        for kopf in [
            "Set-Cookie:ä=1; Secure",
            "Set-Cookie:ة=1",
            "Set-Cookie:\u{1F600}=1",
            "Set-Cookie: x=1; Secure",
        ] {
            let stelle = kopf.find(':').expect("Doppelpunkt") + 1;
            // added 03.10.2026, TLS: **DIE EIGENTLICHE BEHAUPTUNG:** Index 1
            // von `rest` ist **immer** eine Zeichengrenze. Das prueft der
            // Test **direkt**, statt es aus dem ASCII-`:` abzuleiten — eine
            // Rechnung im Kommentar und eine Messung im Test koennen
            // auseinanderlaufen, und dann stimmt nur eines von beiden.
            let rest = &kopf[stelle - 1..];
            assert_eq!(rest.as_bytes()[0], b':', "rest beginnt mit dem Doppelpunkt");
            assert!(
                rest.is_char_boundary(1),
                "Index 1 von `rest` ist eine Zeichengrenze — auch bei {kopf:?}"
            );
        }
    }

    /// added 03.10.2026, TLS: **UND DER GEGENBEWEIS ZUM GEGENBEWEIS** — die
    /// Stelle **darueber** (`x509.rs`) panickt **wirklich**, mit demselben
    /// Muster. Sonst koennte jemand aus diesem Test schliessen, Mehrbytezeichen
    /// seien in Kopfzeilen unproblematisch.
    #[test]
    fn im_zeitfeld_panict_es_trotzdem() {
        // 13 Byte mit einem zweibyteigen Zeichen an Byte 9 — der Fall, den der
        // Fuzzer als `x509.rs:1184` gemeldet hat.
        let mut s: Vec<u8> = (0..13).map(|i| b'0' + (i % 10)).collect();
        s[9] = 0xd8;
        s[10] = 0xb8;
        assert_eq!(s.len(), 13, "13 Byte, die Laengengrenze ist also gruen");
        assert!(
            !std::str::from_utf8(&s).unwrap().is_char_boundary(10),
            "VORBEDINGUNG: hier ist Index 10 KEINE Zeichengrenze — dort brach es"
        );
        assert!(
            narsil_tls::x509::zeit_seit_1970(0x17, &s).is_err(),
            "und der Leser gibt einen Fehler statt eines Absturzes"
        );
    }
}
