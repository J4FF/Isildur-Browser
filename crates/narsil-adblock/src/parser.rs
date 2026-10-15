//! DER EASYLIST-PARSER.
//!
//! # WAS EINE ZEILE SEIN KANN
//!
//! ```text
//! ! Kommentar                            — wird gezählt, nicht benutzt
//! [Adblock Plus 2.0]                     — Listenkopf
//! @@||beispiel.de^$script,third-party    — Ausnahme (erlauben)
//! ||werbung.de/banner.gif                — Netzregel
//! /stats.php?                            — Netzregel (irgendwo)
//! beispiel.de##.werbebanner              — kosmetisch
//! beispiel.de,~intranet.de##.intern     — kosmetisch, Host-Ausnahme
//! beispiel.de#@#.banner                  — Ausnahme von kosmetisch
//! beispiel.de#$#body > .werbung         — erweiterte kosmetisch
//! ```
//!
//! # GRENZEN (M3/M4/M5)
//!
//! * [`MAX_ZEILE`] je Zeile — eine laengere Zeile wird GEKUERZT und
//!   gezaehlt, nicht verworfen. Ein gekuerztes Muster trifft nichts,
//!   ein verworfenes Blatt waere nicht.
//! * [`MAX_REGELN`] fuer die ganze Liste.
//! * [`MAX_OPTIONEN`], [`MAX_ANTEIL`], [`MAX_SELEKTOR`] fuer die Teile.
//! * Keine Rekursion (M5) — der Parser ist ein einziger Durchlauf.

use crate::regel::{
    Andocken, AnfrageArt, Kosmetisch, MAX_ANTEIL, MAX_DOMAIN_ANTEILE, MAX_OPTIONEN, MAX_REGELN,
    MAX_SELEKTOR, MAX_ZEILE, Netzregel,
};
use std::collections::BTreeMap;

/// Was der Parser aus einer Liste gemacht hat.
///
/// M7: das Ergebnis wird SICHERGESTELLT, nicht nur zurueckgegeben —
/// die Zahlen sind das, was man beim Nachmessen braucht.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Auswertung {
    /// Die angenommenen Regeln, in Dateireihenfolge.
    pub regeln: Vec<Regel>,
    /// Zeilen, die mit `!` anfangen.
    pub kommentare: usize,
    /// Listenkoepfe wie `[Adblock Plus 2.0]`.
    pub kopfzeilen: usize,
    /// Zeilen, die keine Regel ergaben.
    pub unerkannt: usize,
    /// Leere Zeilen (nur Weissraum).
    ///
    /// GEMESSENE BEGRUENDUNG fuer den Zaehler: ohne ihn gilt die
    /// Invariante `kommentare + kopfzeilen + unerkannt + regeln_gesamt
    /// == zeilen` NICHT, weil eine Filterliste sehr viele Leerzeilen
    /// hat. Und eine Invariante, die man nicht pruefen kann, ist keine.
    pub leerzeilen: usize,
    /// Zeilen, die zu lang waren und deshalb gekuerzt wurden.
    pub gekuerzt: usize,
    /// Regeln, die ab [`MAX_REGELN`] nicht mehr genommen wurden.
    pub weggelassen: usize,
    /// `$`-Angaben, die es nicht gibt (`$foo`) oder die zu lang waren.
    pub unbekannte_optionen: usize,
}

/// Eine Regel, beide Sorten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Regel {
    /// Eine Anfrage-Regel.
    Netz(Netzregel),
    /// Eine kosmetische Regel.
    Kosmetisch(Kosmetisch),
}

/// Liest eine ganze Liste.
///
/// M4: `text` ist bereits im Speicher; `MAX_REGELN` und `MAX_ZEILE`
/// begrenzen, was daraus entsteht.
pub fn liste_lesen(text: &str) -> Auswertung {
    let mut a = Auswertung::default();
    for roh in text.lines() {
        // Die Grenze EINMAL am Anfang der Zeile pruefen — sie gilt fuer
        // Kommentare und Koepfe genauso wie fuer Regeln, sonst koennte
        // eine Datei aus 10 Millionen Kommentarzeilen unbegrenzt
        // gezaehlt werden.
        let gesehen = a.regeln.len() + a.kommentare + a.kopfzeilen + a.unerkannt + a.leerzeilen;
        if gesehen >= MAX_REGELN {
            a.weggelassen += 1;
            continue;
        }
        let (t, war_gekuerzt) = if roh.len() > MAX_ZEILE {
            a.gekuerzt += 1;
            (roh[..MAX_ZEILE].trim(), true)
        } else {
            (roh.trim(), false)
        };
        let _ = war_gekuerzt;
        if t.is_empty() {
            a.leerzeilen += 1;
            continue;
        }
        if t.starts_with('!') {
            a.kommentare += 1;
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') {
            a.kopfzeilen += 1;
            continue;
        }
        if let Some(k) = kosmetisch(t, &mut a) {
            a.regeln.push(Regel::Kosmetisch(k));
            continue;
        }
        match netz(t, &mut a) {
            Some(r) => a.regeln.push(Regel::Netz(r)),
            None => a.unerkannt += 1,
        }
    }
    a
}

/// Gibt es eine kosmetische Regel in dieser Zeile?
///
/// Die Reihenfolge der Trenner ist bindend: `##` wird vor `#$#`
/// gesucht, sonst wuerde `beispiel.de##.x` als erweiterte Regel
/// gelesen. `#@#` steht vorn, weil es sonst als `##` an Position 1
/// erkannt wuerde.
fn kosmetisch(t: &str, a: &mut Auswertung) -> Option<Kosmetisch> {
    // Die REIHENFOLGE ist bindend und nicht nur Kosmetik: `##` wird vor
    // `#$#` gesucht, sonst laesst `beispiel.de##.x` sich als erweiterte
    // Regel lesen. Und `#@#` steht vorn, weil es sonst an Position 1
    // als `##` erkannt wuerde.
    let (ausnahme, hosts_teil, selektor_teil) = match t.find("#@#") {
        Some(i) => (true, &t[..i], &t[i + 3..]),
        None => match t.find("##") {
            Some(i) => (false, &t[..i], &t[i + 2..]),
            None => {
                let i = t.find("#$#")?;
                (false, &t[..i], &t[i + 3..])
            }
        },
    };
    let roh = selektor_teil.trim();
    if roh.is_empty() {
        return None;
    }
    let selektor = if roh.len() > MAX_SELEKTOR {
        a.unbekannte_optionen += 1;
        &roh[..MAX_SELEKTOR]
    } else {
        roh
    };
    let mut hosts: Vec<String> = Vec::new();
    for h in hosts_teil.split(',') {
        let h = h.trim();
        if h.is_empty() {
            continue;
        }
        if let Some(ohne) = h.strip_prefix('~') {
            hosts.push(format!("~{}", ohne.to_ascii_lowercase()));
        } else {
            hosts.push(h.to_ascii_lowercase());
        }
        if hosts.len() >= MAX_DOMAIN_ANTEILE {
            a.unerkannt += 1;
            break;
        }
    }
    Some(Kosmetisch {
        ausnahme,
        hosts,
        selektor: selektor.to_string(),
    })
}

/// Gibt es eine Netzregel in dieser Zeile?
fn netz(t: &str, a: &mut Auswertung) -> Option<Netzregel> {
    let ohne_ausnahme = t.strip_prefix("@@").unwrap_or(t);
    let ausnahme = ohne_ausnahme.len() != t.len();

    // Optionen abtrennen.
    //
    // GEMESSENE ENTSCHEIDUNG, nach einem Fehlversuch: geteilt wird nur an
    // einem `$`, HINTER DEM EIN BEKANNTES STICHWORT steht. Die erste
    // Fassung trennte am letzten `$` mit keinem `/` dahinter — damit
    // wurde `||werbung.de/banner$1.gif` als Muster `banner` mit der
    // Option `1.gif` gelesen, also als Regel, die fast nichts mehr
    // trifft. Ein Pfad darf ein `$` enthalten, und genau darum ist
    // "bekanntes Stichwort" das sichere Kriterium.
    let (muster_roh, optionen) = match options_spalten(ohne_ausnahme) {
        Some((m, o)) => (m, o),
        None => (ohne_ausnahme, ""),
    };

    let (andocken, muster_teil) = if let Some(rest) = muster_roh.strip_prefix("||") {
        (Andocken::Domain, rest)
    } else if let Some(rest) = muster_roh.strip_prefix('|') {
        (Andocken::Anfang, rest)
    } else {
        (Andocken::Irgendwo, muster_roh)
    };

    // Das `^` ist der Trenner, kein Text — und es kann MITTEN stehen:
    // `||beispiel.de^/werbung` meint „Host beispiel.de, danach der Pfad
    // /werbung".
    //
    // GEMESSENER FEHLER, den der Test `domainregel_mit_pfad` gefunden hat:
    // Ich habe nur ein `^` am ENDE abgeschnitten. Bei einem Pfad im
    // Muster wohnt das `^` in der Mitte, blieb dort stehen, und die
    // Regel traf gar nichts.
    // GEMESSENE ENTSCHEIDUNG nach drei Fehlversuchen:
    //   * `||beispiel.de^/werbung`  — `^` MITTEN, trennt Host und Pfad
    //   * `||google-analytics.com/analytics.js` — `/` ohne `^`, trennt auch
    //   * `||beispiel.de^`          — `^` am Ende, NICHTS dahinter
    // Der dritte Fall ist der, den eine naive Suche nach `^` zerstoe-rt:
    // leerer Schwanz heisst „kein Schwanz", nicht „Pfad ist leer".
    let (hostteil, schwanz): (&str, Option<&str>) = match muster_teil.find('^') {
        Some(i) => {
            let t = muster_teil[i + 1..].trim_matches('/');
            if t.is_empty() {
                (&muster_teil[..i], None)
            } else {
                (&muster_teil[..i], Some(t))
            }
        }
        None => match muster_teil.find('/') {
            Some(i) if i > 0 => {
                let t = muster_teil[i + 1..].trim_matches('/');
                if t.is_empty() {
                    (&muster_teil[..i], None)
                } else {
                    (&muster_teil[..i], Some(t))
                }
            }
            _ => (muster_teil, None),
        },
    };
    if hostteil.is_empty() {
        return None;
    }
    if !andocken_schafft_anker(andocken, hostteil) {
        return None;
    }
    let gekuerzt = hostteil.len() > MAX_ANTEIL;
    let muster = if gekuerzt {
        a.unbekannte_optionen += 1;
        hostteil[..MAX_ANTEIL].to_string()
    } else {
        hostteil.to_string()
    };

    let anteile: Vec<&str> = optionen.split(',').take(MAX_OPTIONEN).collect();
    let mut dritte_partei = false;
    let mut match_case = false;
    let mut domains: BTreeMap<String, bool> = BTreeMap::new();
    for o in &anteile {
        match *o {
            "third-party" | "3p" => dritte_partei = true,
            "match-case" => match_case = true,
            _ => {
                if let Some(wert) = o.strip_prefix("domain=") {
                    for teil in wert.split('|').take(MAX_DOMAIN_ANTEILE) {
                        let teil = teil.trim();
                        if teil.is_empty() {
                            continue;
                        }
                        if let Some(ohne) = teil.strip_prefix('~') {
                            domains.insert(ohne.to_ascii_lowercase(), false);
                        } else {
                            domains.insert(teil.to_ascii_lowercase(), true);
                        }
                    }
                // `o` ist `&&str`, weil die Liste `&str` haelt und
                // `for o in &anteile` darueber iteriert. Der Vergleich
                // braucht also das aufgeloeste `*o` — der Compiler hat
                // mich darauf hingewiesen.
                } else if !o.is_empty() && *o != "important" && !ist_typ(o) {
                    a.unbekannte_optionen += 1;
                }
            }
        }
    }
    let art = AnfrageArt::aus_optionen(&anteile);

    // GEMESSENER FEHLER, den `gross_kleinschreibung` gefunden hat: Ich
    // habe das Muster so gespeichert, wie es im Blatt stand. Der Host
    // wird klein geschrieben, das Muster nicht — also traf
    // `||WERBUNG.de^` nie einen Host `werbung.de`. Ohne `$match-case`
    // ist die Angabe case-insensitive, also wird das Muster auch
    // klein geschrieben.
    // Beide Arme liefern `(String, Option<String>)` — deshalb wird der
    // Schwanz auch im `match_case`-Arm zu einem `String`. Das war der
    // zweite Compilerfehler in dieser Funktion.
    let (muster, schwanz) = if match_case {
        (muster, schwanz.map(|t| t.to_string()))
    } else {
        (
            muster.to_ascii_lowercase(),
            schwanz.map(|t| t.to_ascii_lowercase()),
        )
    };
    let schwanz = schwanz.map(|t| {
        if t.len() > MAX_ANTEIL {
            a.unbekannte_optionen += 1;
            t[..MAX_ANTEIL].to_string()
        } else {
            t
        }
    });

    Some(Netzregel {
        ausnahme,
        andocken,
        muster,
        schwanz,
        art,
        dritte_partei,
        match_case,
        domains,
    })
}

/// Wo beginnen die Optionen? Das ERSTE `$`, dessen Rest mit einem
/// bekannten Stichwort beginnt.
///
/// Von hinten gesucht, weil `$domain=a.com,$script` beide Stellen hat
/// und nur die letzte die Optionen eroeffnet.
fn options_spalten(zeile: &str) -> Option<(&str, &str)> {
    let mut treffer: Option<usize> = None;
    for (i, _) in zeile.match_indices('$') {
        let rest = &zeile[i + 1..];
        // Der Rest darf nicht angebrochen sein — `||x^` mit nichts
        // dahinter ist kein Optionsteil.
        if rest.is_empty() {
            continue;
        }
        let erstes = rest.split(',').next().unwrap_or("");
        let name = erstes.split('=').next().unwrap_or("");
        if ist_option(name) {
            treffer = Some(i);
        }
    }
    treffer.map(|i| (&zeile[..i], &zeile[i + 1..]))
}

/// Ist das ein Stichwort, das EasyList als Option kennt?
///
/// GEMESSENE ENTSCHEIDUNG: ein UNBEKANNTES Stichwort trennt NICHT. Es
/// wird gezaehlt (siehe `liste_lesen`), aber es macht aus dem Rest
/// keine Optionen — sonst wuerde ein Tippfehler im Blatt eine Liste
///halb ausser Kraft setzen.
fn ist_option(name: &str) -> bool {
    if name == "domain" || name == "important" {
        return true;
    }
    if name == "3p" {
        return true;
    }
    ist_typ(name) || name == "match-case" || name == "third-party"
}

/// Ist das eine Typoption nach der EasyList-Spezifikation?
///
/// `important` ist keine, wird aber oft gesetzt und ist harmlos.
fn ist_typ(o: &str) -> bool {
    matches!(
        o,
        "image"
            | "script"
            | "stylesheet"
            | "xmlhttprequest"
            | "document"
            | "popup"
            | "font"
            | "media"
            | "ping"
            | "websocket"
            | "other"
            | "beacon"
    )
}

/// Kann dieses Muster ueberhaupt andocken?
///
/// GEMESSENE REGEL: `||` ohne Punkt im Host und `|` mit weniger als
/// vier Zeichen sind keine Regeln, sondern Tippfehler. Sie wuerden sonst
/// JEDE Anfrage treffen — das schlimmste moegliche Ergebnis fuer einen
/// Filter.
fn andocken_schafft_anker(andocken: Andocken, muster: &str) -> bool {
    match andocken {
        // Bei `||` muss ein Punkt im Hostteil stehen, sonst waere
        // `||x` eine Regel fuer jede Domain der Welt.
        Andocken::Domain => muster.contains('.'),
        Andocken::Anfang => muster.len() >= 4,
        Andocken::Irgendwo => true,
    }
}
