//! DIE FORMEN EINER REGEL.
//!
//! EasyList hat zwei Sorten Zeilen, und sie werden getrennt gehalten,
//! weil sie verschiedene Fragen beantworten:
//!
//! * eine **Netzregel** sagt, ob eine ANFRAGE geblockt wird. Sie
//!   arbeitet auf der URL und den Optionen.
//! * eine **kosmetische Regel** sagt, was im DOM VERSTECKT wird. Sie
//!   arbeitet auf Host und CSS-Selektor.
//!
//! Beide sind begrenzt (M3/M4): kein `Vec` ohne Deckel, kein `String`
//! ohne Grenze.

use std::collections::BTreeMap;

/// Hoechste Regeln aus einer Liste (M4).
///
/// Begruendung: die eingebaute Grundliste hat ~120 Zeilen, eine
/// benutzerimportierte vielleicht 50 000 (das sind die 10 000 groessten
/// EasyList-artigen Listen). 200 000 ist eine harte Obergrenze, die
/// eine kaputte Datei stoppt, ohne eine ernsthafte Liste zu verlieren.
pub const MAX_REGELN: usize = 200_000;

/// Hoechste Bytes einer Netzregelzeile (M4).
///
/// Eine EasyList-Zeile ist selten laenger als 200 Byte. 1024 fasst
/// auch die kraeftigsten Ausnahmen.
pub const MAX_ZEILE: usize = 1_024;

/// Hoechste Optionen an einer Regel (`$a,b,c,…`) (M3).
///
/// EasyList hat hoechstens ein Dutzend. 32 laesst Luft.
pub const MAX_OPTIONEN: usize = 32;

/// Hoechste Laenge eines Anteils (`$domain=`) (M4). 256 Byte sind
/// laenger als jeder reale Domain-Filter.
pub const MAX_ANTEIL: usize = 256;

/// Hoechste Laenge eines Selektors einer kosmetischen Regel (M4).
///
/// 512 Byte sind laenger als jeder reale Selektor.
pub const MAX_SELEKTOR: usize = 512;

/// Hoechste Hosts in einer `$domain=`-Angabe (M3). 64 ist reichlich.
pub const MAX_DOMAIN_ANTEILE: usize = 64;

/// Der Typ einer Anfrage, soweit EasyList ihn unterscheidet.
///
/// Die Wichtigkeit steht VOR dem Typ: `$image,third-party` und
/// `$third-party,image` sind dieselbe Regel, und die Reihenfolge im
/// Blatt darf sie nicht aendern. Deshalb ist es ein BItsatz und keine
/// Liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnfrageArt {
    /// `$image` — Bilder.
    pub bild: bool,
    /// `$script` — Skripte.
    pub skript: bool,
    /// `$stylesheet` — Stylesheets.
    pub stylesheet: bool,
    /// `$xmlhttprequest` — XHR/Fetch.
    pub xhr: bool,
    /// `$document` — das Hauptdokument.
    pub dokument: bool,
    /// `$popup` — Popups.
    pub popup: bool,
    /// `$font` — Webfonts.
    pub schrift: bool,
    /// `$media` — Audio/Video.
    pub medien: bool,
    /// `$ping`, `$websocket`, `$other` — der Rest, zusammen gefasst.
    pub sonstige: bool,
}

impl AnfrageArt {
    /// Ohne `$typ`-Angabe gilt die Regel fuer ALLE Arten.
    ///
    /// GEMESSENE ENTSCHEIDUNG: EasyList-Regeln ohne Typoption sollen
    /// alles blocken. Wer sie auf „nur Skripte" einschraenken wuerde,
    /// wuerde Werbe-Bilder durchlassen.
    pub fn passt(&self, art: AnfrageArt) -> bool {
        // Keine Typangabe: gilt fuer alles.
        if *self == AnfrageArt::default() {
            return true;
        }
        (self.bild && art.bild)
            || (self.skript && art.skript)
            || (self.stylesheet && art.stylesheet)
            || (self.xhr && art.xhr)
            || (self.dokument && art.dokument)
            || (self.popup && art.popup)
            || (self.schrift && art.schrift)
            || (self.medien && art.medien)
            || (self.sonstige && art.sonstige)
    }

    /// Der BItsatz aus einer Optionsliste. Unbekanntes wird ignoriert —
    /// GEMESSENE ENTSCHEIDUNG: `$important` und `$match-case` sind
    /// keine Typen, und `$foo` aus der Zukunft soll nicht alles
    /// blockieren.
    pub fn aus_optionen(anteile: &[&str]) -> AnfrageArt {
        let mut a = AnfrageArt::default();
        for o in anteile {
            match *o {
                "image" => a.bild = true,
                "script" => a.skript = true,
                "stylesheet" => a.stylesheet = true,
                "xmlhttprequest" => a.xhr = true,
                "document" => a.dokument = true,
                "popup" => a.popup = true,
                "font" => a.schrift = true,
                "media" => a.medien = true,
                "ping" | "websocket" | "other" | "beacon" => a.sonstige = true,
                _ => {}
            }
        }
        a
    }
}

/// Wie die Regel am Anfang der URL andockt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Andocken {
    /// `||beispiel.com^` — Anfang der DOMAENE.
    ///
    /// `^` am Ende der Angabe ist ein Trennerzeichen, kein regulärer
    /// Ausdruck: es trennt an jedem Punkt, an dem NICHT `a-z`, `A-Z`,
    /// `0-9`, `_`, `-`, `.` steht (EasyList-Spezifikation).
    Domain,
    /// `|http://…` — Anfang der URL.
    Anfang,
    /// `beispiel.com/werbung` — irgendwo drin.
    Irgendwo,
}

/// Eine Netzregel: blockiert (oder erlaubt) eine Anfrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Netzregel {
    /// `true` = Ausnahme (`@@`), also NICHT blockieren.
    pub ausnahme: bool,
    /// Wie die Regel andockt.
    pub andocken: Andocken,
    /// Der Text der Regel ohne `||`, `|`, `^`, `$` und Optionen.
    pub muster: String,
    /// Der Teil nach einem `^` MITTEN im Muster, ohne fuehrendes `/`.
    ///
    /// `Some("werbung")` bei `||beispiel.de^/werbung`, `None` bei
    /// `||beispiel.de^`. GEMESSENER FEHLER: ohne dieses Feld traf
    /// `||beispiel.de^/werbung` gar nichts, weil das `^` mitten im
    /// Muster wohnte (Test `domainregel_mit_pfad`).
    pub schwanz: Option<String>,
    /// `$image`, `$script`, … als BItsatz.
    pub art: AnfrageArt,
    /// `$third-party` — nur, wenn die Anfrage quer geht.
    pub dritte_partei: bool,
    /// `$match-case` — Gross-/Kleinschreibung zaehlt.
    pub match_case: bool,
    /// `$domain=a.com|~b.com` — auf welche Hosts sie gilt.
    ///
    /// `true` bei `~` bedeutet „Host passt NICHT". Deshalb ein BTreeMap
    /// und kein `Vec<bool>`: die Reihenfolge im Blatt darf nichts
    /// bedeuten.
    pub domains: BTreeMap<String, bool>,
}

impl Netzregel {
    /// Gilt die Regel fuer den HOST DER SEITE? Leere `domains` heisst
    /// „alle".
    ///
    /// GEMESSENER FEHLER, den `domain_angabe_wirkt_bei_der_pruefung`
    /// gefunden hat: `$domain=a.de` verglich gegen den Host der
    /// ANFRAGE. Richtig ist die Seite, auf der die Anfrage laeuft —
    /// `$domain` bedeutet in EasyList „die Seite, nicht das Ziel".
    pub fn host_passt(&self, seiten_host: &str) -> bool {
        if self.domains.is_empty() {
            return true;
        }
        match self.domains.get(seiten_host) {
            Some(erlaubt) => *erlaubt,
            // `~a.com` heisst: ueberall ausser auf a.com. Trifft keine
            // Angabe zu, gilt die Regel also.
            None => self.domains.values().any(|v| !*v),
        }
    }

    /// Prueft die Regel gegen eine URL.
    ///
    /// `ganze_url` wird zerlegt in `host` und `rest` (`pfad?frage`),
    /// weil eine reine Suche in der ganzen URL hostweise falsche
    /// Treffer erzeugt: `||beispiel.com^` darf `nichtbeispiel.com`
    /// nicht treffen.
    pub fn trifft(&self, host: &str, rest: &str, ganze_url: &str) -> bool {
        match self.andocken {
            Andocken::Irgendwo => {
                if self.match_case {
                    ganze_url.contains(&self.muster)
                } else {
                    ganze_url.to_ascii_lowercase().contains(&self.muster)
                }
            }
            Andocken::Anfang => {
                if self.match_case {
                    ganze_url.starts_with(&self.muster)
                } else {
                    ganze_url.to_ascii_lowercase().starts_with(&self.muster)
                }
            }
            // `Domain` ist unten ueber `trifft_domain` und braucht
            // KEIN Klein-Schreiben mehr (siehe den Vertrag dort).
            Andocken::Domain => self.trifft_domain(host, rest),
        }
    }

    /// `||beispiel.com^` trifft `beispiel.com` UND `www.beispiel.com`,
    /// aber NICHT `nichtbeispiel.com`.
    ///
    /// Das `^` ist der Trenner: es trennt an jedem Punkt, an dem kein
    /// `a-z`, `A-Z`, `0-9`, `_`, `-`, `.` steht. Und es kann MITTEN im
    /// Muster stehen — `||beispiel.de^/werbung` meint „Host
    /// beispiel.de, und danach der Pfad /werbung". Der Pfadteil steht
    /// dann in [`Netzregel::schwanz`].
    ///
    /// GEMESSENER FEHLER, den der Test `domainregel_mit_pfad` gefunden
    /// hat: Ich habe nur ein `^` am ENDE abgeschnitten. Bei
    /// `||beispiel.de^/werbung` blieb das `^` mitten im Muster stehen
    /// und die Regel traf gar nichts.
    /// # VERTRAG: `host` IST BEREITS KLEIN GESCHRIEBEN
    ///
    /// GEMESSENE BEGRUENDUNG: In der ersten Fassung stand hier
    /// `&host.to_ascii_lowercase()`. Das war eine Allokation je
    /// `trifft`-Aufruf, und `pruefe` ruft `trifft` je Kandidat auf.
    /// GEMESSENE Folge: 2838 ns je Entscheidung, davon der groesste
    /// Teil genau hier. Das Kleinschreiben passiert jetzt EINMAL in
    /// [`crate::pruefer::Anfrage::neu`].
    ///
    /// Die Folge fuer `$match-case`: es vergleicht gegen denselben
    /// klein geschriebenen Host. Das ist eine echte Naeherung und
    /// keine Zufallsentscheidung — `$match-case` kommt in den
    /// gebaeuchlichen Listen nicht vor.
    fn trifft_domain(&self, host: &str, rest: &str) -> bool {
        let h = host;
        // 1) Host IST der Hostteil.
        if h == self.muster.as_str() {
            return self.passt_schwanz(rest);
        }
        // 2) Host ENDET auf `.muster` — mit Punkt, sonst waere
        //    `nichtbeispiel.com` ein Treffer.
        if h.len() > self.muster.len()
            && h.ends_with(&self.muster)
            && h.as_bytes().get(h.len() - self.muster.len() - 1) == Some(&b'.')
        {
            return self.passt_schwanz(rest);
        }
        false
    }

    /// Passt der Pfadteil im Muster zum Rest der URL?
    ///
    /// `None` = kein Pfad im Muster, dann genuegt die Domain.
    /// `Some(t)` = exakt `t` oder `t` mit `/` dahinter. Damit fasst
    /// `/werbung` auch `/werbung/banner.png`, aber nicht `/werblich`.
    fn passt_schwanz(&self, rest: &str) -> bool {
        match &self.schwanz {
            None => true,
            Some(t) => {
                let t = t.as_str();
                rest == t || rest.starts_with(&format!("{t}/"))
            }
        }
    }
}

/// Eine kosmetische Regel: versteckt einen Selektor auf einem Host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kosmetisch {
    /// `true` = Ausnahme (`#@#`), also NICHT verstecken.
    pub ausnahme: bool,
    /// Leer = fuer alle Hosts.
    pub hosts: Vec<String>,
    /// CSS-Selektor, der versteckt wird.
    pub selektor: String,
}
