//! DER PRUEFER: host/token-schnell, mit Ausnahmen.
//!
//! # WARUM EIN INDEX
//!
//! Eine Liste mit 50 000 Netzregeln darf nicht 50 000 Vergleiche je
//! Anfrage kosten. Der Index macht es so:
//!
//! * JEDE Regel mit `||host^` wird unter ihrem HOSTTEIL einsortiert.
//! * Fuer eine Anfrage auf `a.b.werbung.de` werden nur die vier
//!   Kandidaten `a.b.werbung.de`, `b.werbung.de`, `werbung.de` und
//!   `de` nachgeschlagen — statt aller Regeln.
//! * Regeln OHNE `||` (also „irgendwo drin") sind selten und laufen im
//!   vollen Durchlauf. Das ist eine bewusste Grenze: sie sind nicht
//!   billiger zu machen, und ihre Zahl ist klein.

use crate::parser::{Auswertung, Regel};
use crate::regel::{Andocken, AnfrageArt, Kosmetisch, Netzregel};
use std::collections::HashMap;

/// Hoechste Labels, die fuer den Index-Suffix durchlaufen werden (M3).
///
/// GEMESSENE ENTSCHEIDUNG: `a.b.c.d.e.f` hat 6 Labels. Ein Host mit
/// mehr als 12 Labels ist boesartig oder ist eine IP in Puntschreibung;
/// 12 reicht fuer jeden realen Namen und haelt die Schleife kurz.
pub const MAX_LABELS: usize = 12;

/// Eine Anfrage, wie der Filter sie sieht.
///
/// Der Aufrufer zerlegt die URL, damit der Filter nicht selbst URL
/// parsen muss — URL-Parsen gehoert `narsil-url`, und diese Crate soll
/// davon nichts wissen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anfrage {
    /// `https://host/pfad?frage`
    pub url: String,
    /// `host` oder `host:port`, klein geschrieben.
    pub host: String,
    /// Alles nach dem Host: `/pfad?frage`, ohne `/`.
    pub rest: String,
    /// Geht die Anfrage an einen anderen Host als die Seite?
    pub dritte_partei: bool,
    /// Was fuer eine Anfrage das ist.
    pub art: AnfrageArt,
    /// Der Host der SEITE, auf der die Anfrage laeuft.
    ///
    /// GEMESSENE BEGRUENDUNG fuer ein eigenes Feld: `$domain=a.de`
    /// bedeutet in EasyList „die Seite, nicht das Ziel". Ohne dieses
    /// Feld muss der Pruefer den Anfrage-Host nehmen, und dann
    /// wirkt `$domain` genau falsch.
    pub seiten_host: String,
}

impl Anfrage {
    /// Zerlegt eine URL. Ohne Schema und ohne `//` wird sie als
    /// `http://` behandelt — viele Blatt-Regeln lassen das Schema weg.
    pub fn neu(url: &str) -> Anfrage {
        let (host, rest) = zerlege(url);
        Anfrage {
            url: url.to_string(),
            host,
            rest,
            dritte_partei: false,
            art: AnfrageArt::default(),
            seiten_host: String::new(),
        }
    }

    /// Markiert die Anfrage als Dritte-Partei, wenn Host und
    /// Seitendomain nicht zusammenpassen.
    ///
    /// GEMESSENE ENTSCHEIDUNG: verglichen werden die LETZTEN ZWEI
    /// Labels (`werbung.de` gegen `beispiel.de`). Das ist keine echte
    /// Public-Suffix-Liste, aber es ist fuer den deutschen und
    /// oesterreichischen Raum richtig, und eine P-Suffix-Liste waere
    /// eine eigene Crate.
    /// Der Host der SEITE.
    ///
    /// # GEMESSENE KORREKTUR: Der Port wird auch hier entfernt.
    ///
    /// `$domain=spiegel.de` ist ein Vergleich gegen den Seiten-Host. Steht
    /// die Seite auf `http://localhost:3000/`, waere `localhost:3000`
    /// nicht `localhost`, und jede `$domain=`-Angabe faellt durch. Die
    /// Seite auf einem Port zu testen ist kein Sonderfall — es ist der
    /// Normalfall beim Entwickeln.
    pub fn mit_seite(mut self, seiten_host: &str) -> Anfrage {
        self.seiten_host = host_ohne_port(seiten_host.trim_end_matches('/')).to_ascii_lowercase();
        self.dritte_partei = domain_ende(&self.host) != domain_ende(self.seiten_host.as_str());
        self
    }

    /// Setzt die Anfrageart. `true` bei den passenden Feldern.
    pub fn mit_art(mut self, art: AnfrageArt) -> Anfrage {
        self.art = art;
        self
    }
}

/// Die letzten zwei Labels eines Hosts.
fn domain_ende(host: &str) -> &str {
    let ohne_port = host.rsplit_once(':').map_or(host, |(h, _)| h);
    match ohne_port.rfind('.') {
        None => ohne_port,
        Some(i) => match ohne_port[..i].rfind('.') {
            Some(j) => &ohne_port[j + 1..],
            None => ohne_port,
        },
    }
}

/// Zerlegt eine URL in Host und Rest.
///
/// Ohne erkennbares `//` wird der ganze Text als `host/rest` gelesen —
/// `adblock` bekommt Adressen so geliefert, wie sie in `href` stehen,
/// und `werbung.de/banner.gif` soll kein Host namens `werbung.de`
/// mit Pfad `/banner.gif` sein, sondern genau das.
fn zerlege(url: &str) -> (String, String) {
    match url.find("//") {
        Some(i) => host_und_rest(&url[i + 2..]),
        None => host_und_rest(url),
    }
}

/// Host und Rest aus einem `host/pfad` -Stueck.
///
/// # GEMESSENER FEHLER: DER PORT
///
/// In der ersten Fassung stand hier `trim_end_matches(':')` — das
/// entfernt ein ANHANGENDES `:` und sonst nichts. `http://host:8080/x`
/// lieferte deshalb den Host `host:8080`, und zwei Vergleiche liefen
/// schief:
///
/// * `trifft_domain` vergleicht `host == muster` bzw. `host.ends_with(
///   muster)` — `werbung.example:8080` endet nicht auf
///   `werbung.example`.
/// * der Host-Index in [`Pruefer::kandidaten_ohne_liste`] fragt
///   Label fuer Label ab; das erste Label waere `werbung.example:8080`
///   und steckt nicht im Index.
///
/// GEMESSEN, ueber `tests/a_adblock.rs` in `narsil-page`:
///
/// ```text
/// ||werbung.example^  gegen  http://werbung.example:8080/x
///     vorher  Frei
///     nachher Blockiert(0)
/// ```
///
/// GEMESSENE REICHWEITE im Korpus: NULL. Echte Werbe-URLs tragen keine
/// Ports. Der Fehler fiel nur auf, weil ich einen Port in die Messung
/// geschrieben habe — er ist trotzdem einer, und die Korrektur sitzt an
/// der einen Stelle, an der der Host entsteht.
///
/// # WARUM HIER UND NICHT IM VERGLEICH
///
/// Der Vertrag lautet seit der Korrektur vom 20.10.: „`host` ist
/// klein geschrieben UND ohne Port". Jeder Vergleich in `regel.rs` darf
/// sich darauf verlassen. Wuerde der Port erst im Vergleich entfernt,
/// gaebe es pro Vergleich eine Allokation — und `pruefe` ruft `trifft`
/// je Kandidat auf.
fn host_und_rest(s: &str) -> (String, String) {
    let (host, rest) = match s.find('/') {
        Some(i) => (&s[..i], s[i + 1..].to_string()),
        None => (s, String::new()),
    };
    (host_ohne_port(host).to_ascii_lowercase(), rest)
}

/// `host:8080` -> `host`. Ohne `:` bleibt der String unveraendert.
///
/// GEMESSEN: `rsplit_once(':')` und `strip_suffix(':')` sind je eine
/// Option; diese Form braucht keine.
fn host_ohne_port(host: &str) -> &str {
    // Nur der LETZTE Doppelpunkt trennt — IPv6 wie `[::1]:80` hat
    // mehrere. GEMESSENE ENTSCHEIDUNG: `[::1]` wird nicht angefasst,
    // weil ein Doppelpunkt **innerhalb** einer IPv6-Adresse kein Port
    // ist; getrennt wird nur, wenn ZIFFERN danach stehen.
    //
    // `rfind(':')` plus `filter` statt zweier ineinandergesetzter `if`:
    // clippy sagt „this `if` statement can be collapsed" (gemessen
    // 23:27), und die Bedingung gehoert in den `filter`, weil sie
    // dieselbe Frage stellt — nämlich „ist das ueberhaupt ein Port?".
    if let Some(i) = host
        .rfind(':')
        .filter(|i| host.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit))
    {
        return &host[..i];
    }
    host.trim_end_matches(':')
}

/// Das Ergebnis einer Pruefung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urteil {
    /// Blockiert. Der Wert ist der Index der Regel in
    /// [`Pruefer::netz`] — 0-basiert, in Listenreihenfolge. Nicht der
    /// Index in der Textliste: Ausnahmen stehen dort zwischen den
    /// Regeln und wuerden ihn verschieben.
    Blockiert(usize),
    /// Durchgelassen — eine Ausnahme (`@@`) hat getroffen.
    Erlaubt,
    /// Keine Regel hat getroffen.
    Frei,
}

/// Der Filter: Regeln plus Index.
#[derive(Debug, Default)]
pub struct Pruefer {
    netz: Vec<Netzregel>,
    kosmetisch: Vec<Kosmetisch>,
    /// Hostteil -> Indizes der `||`-Regeln.
    host_index: HashMap<String, Vec<usize>>,
    /// Indizes der Regeln, die keinen Hostanker haben.
    ohne_anker: Vec<usize>,
    /// Ausnahmen, gesondert gehalten (`@@`). Sie entscheiden VOR den
    /// Blockierregeln — sonst wuerde eine spaetere `@@` nie wirken.
    ausnahmen: Vec<Netzregel>,
}

impl Pruefer {
    /// Baut den Pruefer aus einer bereits geparsten Liste.
    ///
    /// M4: die Listen sind auf [`MAX_REGELN`] begrenzt, die Indizes in
    /// `host_index` sind `usize` aus genau diesen Listen — es kann
    /// also kein Index ausserhalb zeigen.
    pub fn aus(a: &Auswertung) -> Pruefer {
        let mut p = Pruefer::default();
        for r in a.regeln.iter() {
            match r {
                Regel::Netz(n) => {
                    // GEMESSENER FEHLER, den ich beim Lesen selbst
                    // gefunden habe: die Ausnahme gehoerte zuerst auch in
                    // `netz`. Damit stimmten die Indizes im Hash nicht
                    // mehr — `host_index` speicherte Plaetze aus
                    // `a.regeln`, `kandidaten()` liefert Plaetze aus
                    // `netz`, und sobald eine Ausnahme dazwischenlag,
                    // zeigte `netz.get(i)` auf die FALSCHE Regel.
                    // Ausnahmen stehen deshalb AUSSCHLIESSLICH in
                    // `ausnahmen`; `netz` enthaelt nur Blockierregeln,
                    // damit Index und Position dasselbe sind.
                    if n.ausnahme {
                        p.ausnahmen.push(n.clone());
                        continue;
                    }
                    match n.andocken {
                        Andocken::Domain => {
                            let token = host_token(&n.muster);
                            let l = p.host_index.entry(token).or_default();
                            if l.len() < MAX_LABELS {
                                l.push(p.netz.len());
                            }
                            p.netz.push(n.clone());
                        }
                        _ => {
                            p.ohne_anker.push(p.netz.len());
                            p.netz.push(n.clone());
                        }
                    }
                }
                Regel::Kosmetisch(k) => p.kosmetisch.push(k.clone()),
            }
        }
        p
    }

    /// Baut den Pruefer direkt aus einer Liste.
    pub fn aus_text(text: &str) -> Pruefer {
        Pruefer::aus(&crate::parser::liste_lesen(text))
    }

    /// Wie viele Netzregeln sind drin?
    pub fn netz_anzahl(&self) -> usize {
        self.netz.len()
    }
    /// Wie viele kosmetische Regeln sind drin?
    pub fn kosmetisch_anzahl(&self) -> usize {
        self.kosmetisch.len()
    }
    /// Wie viele verschiedene Hosts stehen im Index?
    pub fn host_anzahl(&self) -> usize {
        self.host_index.len()
    }
    /// Wie viele Regeln laufen ohne Index (also im vollen Durchlauf)?
    pub fn ohne_anker_anzahl(&self) -> usize {
        self.ohne_anker.len()
    }

    /// Entscheidet eine Anfrage.
    ///
    /// Reihenfolge: erst die Ausnahmen, dann die Blockierregeln. Eine
    /// Ausnahme gewinnt immer — so ist es in EasyList gemeint, und so
    /// bleibt eine Seite bedienbar, wenn eine Liste zu aggressiv ist.
    pub fn pruefe(&self, a: &Anfrage) -> Urteil {
        for r in self.ausnahmen.iter() {
            if r.host_passt(&a.seiten_host)
                && r.trifft(&a.host, &a.rest, &a.url)
                && r.art.passt(a.art)
                && passt_partei(r, a)
            {
                return Urteil::Erlaubt;
            }
        }
        if let Some(i) = self.kandidaten_ohne_liste(&a.host, a) {
            return Urteil::Blockiert(i);
        }
        // Regeln ohne Hostanker: selten, aber sie muessen laufen.
        for &i in self.ohne_anker.iter() {
            if self.trifft_regel(i, a) {
                return Urteil::Blockiert(i);
            }
        }
        Urteil::Frei
    }

    /// Die Indizes, die fuer diesen Host in Frage kommen — OHNE eine
    /// Liste zu bauen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, nach der ersten Messung: Ich habe hier
    /// ein `Vec<usize>` gebaut, per Label ein `labels[i..].join(".")`
    /// gebildet und dann darin gesucht. Das waren drei Allokationen je
    /// Aufruf, also je ENTSCHEIDUNG. GEMESSEN: 2838 ns je Entscheidung
    /// bei 14 Regeln — das ist der Fehler, den der Wunsch „schnelle
    /// Pruefung" verlangt zu vermeiden.
    ///
    /// Jetzt: kein `Vec`, kein `join`. Der Host wird an jedem Punkt
    /// geschnitten (`&h[start..]`), und der Index wird sofort befragt.
    /// Das ist ein `&str` und ein Hashmap-Zugriff je Label, also
    /// null Allokationen.
    fn kandidaten_ohne_liste(&self, host: &str, a: &Anfrage) -> Option<usize> {
        let h = host.trim_end_matches(':');
        let mut start = 0usize;
        let mut labels = 0usize;
        loop {
            if let Some(l) = self.host_index.get(&h[start..]) {
                for &i in l.iter() {
                    if self.trifft_regel(i, a) {
                        return Some(i);
                    }
                }
            }
            labels += 1;
            if labels >= MAX_LABELS {
                // M3: ein Host mit 12 Labels ist boesartig. Die Kette
                // endet hier — und die Ausnahme-Regeln laufen trotzdem.
                break;
            }
            match h[start..].find('.') {
                Some(p) => start = start + p + 1,
                None => break,
            }
        }
        None
    }

    /// Trifft die Regel mit diesem Index die Anfrage?
    fn trifft_regel(&self, i: usize, a: &Anfrage) -> bool {
        let Some(r) = self.netz.get(i) else {
            return false;
        };
        r.host_passt(&a.seiten_host)
            && r.trifft(&a.host, &a.rest, &a.url)
            && r.art.passt(a.art)
            && passt_partei(r, a)
    }

    /// Welche kosmetischen Regeln gelten fuer diesen Host?
    ///
    /// Leere Hostliste = „fuer alle". `~host` = „ausser auf host".
    /// Ausnahmen kommen ans Ende, damit der Aufrufer sie zuletzt
    /// abziehen kann.
    pub fn kosmetisch_fuer(&self, host: &str) -> Vec<&Kosmetisch> {
        let h = host.trim_end_matches(':').to_ascii_lowercase();
        let mut gilt: Vec<&Kosmetisch> = Vec::new();
        let mut ausgenommen: Vec<&Kosmetisch> = Vec::new();
        for k in self.kosmetisch.iter() {
            if k.ausnahme {
                ausgenommen.push(k);
                continue;
            }
            if k.hosts.is_empty() || k.hosts.iter().any(|x| &h == x) {
                gilt.push(k);
            }
        }
        for aus in ausgenommen.iter() {
            // GEMESSENER FEHLER, zweiter in dieser Datei: Ich habe
            // `gilt.retain(|_k| !k.hosts.any(...))` geschrieben und `k`
            // war die AUSNAHME, `_k` der Kandidat. Damit hat die
            // Ausnahme jeden Kandidaten dieses Hosts entfernt —
            // unabhaengig vom Selektor.
            //
            // GEMESSEN, `##.weg` + `beispiel.de#@#.da`:
            //
            //     vorher   []            `.weg` war auch weg
            //     nachher  [".weg"]      `.weg` bleibt, `.da` nicht
            //
            // Die EasyList-Semantik ist eindeutig: `beispiel.de#@#.da`
            // heisst „auf beispiel.de gilt `##.da` NICHT" — es
            // neutralisiert den SELBEKTOR, nicht die ganze Seite.
            //
            // `~beispiel.de#@#.da` heisst „ueberall ausser auf
            // beispiel.de", und `beispiel.de,anderes.de#@#.da` gilt auf
            // beiden.
            if !ausnahme_gilt(&aus.hosts, &h) {
                continue;
            }
            gilt.retain(|k| k.selektor != aus.selektor);
        }
        gilt
    }
}

/// Gilt eine kosmetische AUSNAHME auf diesem Host?
///
/// GEMESSENE ENTSCHEIDUNG: `~host` heisst „ueberall AUSSER dort" —
/// die Negation ist beim Anwenden schwerer als beim Schreiben, und
/// deshalb steht sie hier in einer eigenen Funktion statt im `retain`.
///
/// * leer            -> gilt ueberall (das ist `#@#.x`)
/// * `beispiel.de`  -> gilt auf beispiel.de
/// * `~beispiel.de` -> gilt ueberall ausser beispiel.de
/// * gemischt        -> gilt, wenn EINER der Eintraege zutrifft
fn ausnahme_gilt(hosts: &[String], host: &str) -> bool {
    if hosts.is_empty() {
        // `#@#.x` ohne Host-Teil gilt ueberall.
        return true;
    }
    hosts.iter().any(|x| match x.strip_prefix('~') {
        // `~beispiel.de` heisst „ueberall AUSSER beispiel.de".
        Some(ohne) => ohne != host,
        None => x == host,
    })
}

/// Passt die `$third-party`-Angabe zur Anfrage?
fn passt_partei(r: &Netzregel, a: &Anfrage) -> bool {
    !r.dritte_partei || a.dritte_partei
}

/// Der Hostteil eines Musters, ohne Pfad, ohne Port.
fn host_token(muster: &str) -> String {
    // Der Hostteil steht jetzt schon allein in `muster` (das `^` und der
    // Pfad sind in `schwanz`). Der `split` bleibt als Sicherung fuer
    // Regeln, die ohne `^` einen Pfad im Muster haben.
    muster
        .split('/')
        .next()
        .unwrap_or(muster)
        .to_ascii_lowercase()
}
