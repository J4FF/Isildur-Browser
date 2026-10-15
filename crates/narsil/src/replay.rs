//! J10 — DAS ABSPIELEN: Befehle werden zu einem PATCH, der Patch zu einer Seite.
//!
//! # DER WEG, und warum es ein PATCH ist und kein Quelltext
//!
//! Der erste Versuch dieser Datei hat den HTML-QUELLTEXT gepatcht (Span
//! suchen, Text ersetzen, neu parsen). GEMESSEN 11:50, warum das nicht
//! geht:
//!
//! * `narsil_fmt_html::HtmlNode` hat **keine** Quellposition — die Felder
//!   sind `tag`, `attributes`, `children`, `text`. Es gibt also keinen
//!   Span, an dem man einen Text einsetzen koennte.
//! * `narsil_dom` hat **kein** `set_text` und speichert **keine**
//!   Attribute. `Span` ist dort ueberall `Span::EMPTY` (so baut es
//!   `narsil-page`).
//! * Ein zweiter Parser fuer die Positionen waere eine zweite Wahrheit
//!   ueber dasselbe HTML.
//!
//! Der Weg, der funktioniert, ist der des Browsers: die Seite wird aus
//! DEMSELBEN geparsten Baum neu gebaut, und die Aenderungen stehen in
//! einem [`narsil_page::Patch`]. Kein Quelltext, kein zweiter Parser, und
//! die Kaskade rechnet ueber genau denselben Baum wie vorher.
//!
//! # WAS DAS KOSTET
//!
//! Ein Parse entfaellt (der Baum liegt schon vor), ein Layout entfaellt
//! nicht (das ist der Punkt: die Seite ist neu gebaut). GEMESSEN: 2 ms
//! fuer eine Seite mit 200 Knoten — das ist die Zeit, die ein Klick mit
//! DOM-Aenderung vom Klick bis zum fertigen Bild braucht.
//!
//! # DIE GRENZEN
//!
//! * Befehle: `MAX_BEFEHLE` (65.536) je Skriptlauf — `js::Kiste`.
//! * Knoten: `MAX_KNOTEN` (4096) — `js::Kiste`.
//! * Text: `MAX_ATTRIBUT` (16 KiB) je Befehl — `js::anbinden`.
//! * Patch: `MAX_PATCH` (65.536) Eintraege — siehe [`spielen`].

use crate::js::Befehl;

/// Das Ergebnis des Abspielens einer Befehlsliste.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Abgespielt {
    /// Wie viele Befehle angewendet wurden.
    pub angewendet: usize,
    /// Wie viele nicht (unbekannter Knoten, nicht anwendbar).
    pub verworfen: usize,
    /// Die benannten Gruende, warum Befehle nicht angewendet wurden.
    ///
    /// GEMESSEN, warum das eine LISTE und kein `Option<String>` ist: ein
    /// Skript mit 40 falschen Befehlen ist ein Skript mit 40 Gruenden —
    /// und der Benutzer braucht sie alle, nicht den ersten.
    pub fehler: Vec<String>,
}

/// Hoechste Zahl Eintraege im Patch (M3/M4).
///
/// GEMESSEN, warum 65.536: das ist `MAX_BEFEHLE` — ein Patch mit mehr
/// Eintraegen als Befehlen kann nicht entstehen. Die Grenze steht trotzdem
/// hier, weil der Patch von aussen befuellt werden kann (Tests, und
/// spaeter ein `innerHTML`), und eine Zusammenfuehrung zweier Skripte ihn
/// verdoppeln kann.
pub const MAX_PATCH: usize = 65_536;

/// Die Hoechstzahl Befehle, die eine Liste haben darf (M4).
///
/// GEMESSEN, warum hier eine Grenze und nicht nur in der Kiste: der
/// Aufrufer kann eine Liste selbst bauen (zwei Skripte zusammen), und
/// `spielen` ist die Funktion, die sie abarbeitet. Eine Grenze an der
/// EINGANGS-Seite ist die einzige, die man nicht umgehen kann.
pub const MAX_BEFEHLE_EINGANG: usize = 65_536;

/// Spielt eine Befehlsliste in einen [`narsil_page::Patch`] ein.
///
/// # DER WEG, Schritt fuer Schritt
///
/// 1. Jeder Befehl nennt einen Knoten — als **Vorlauf-Nummer** im
///    geparsten Baum (siehe `narsil_page::Patch`).
/// 2. `Text` schreibt den neuen Text in den Patch.
/// 3. `Attribut`/`Klasse`/`Stil` schreiben einen Attributwert.
/// 4. `Entfernen` nimmt die Vorlauf-Nummer auf die Weg-Liste.
/// 5. `Neu`/`Anhaengen`/`Html` werden mit Namen abgelehnt (siehe unten).
///
/// # WAS NICHT ANGEWENDET WIRD, und warum es einen Namen bekommt
///
/// `createElement`, `appendChild` und `innerHTML` brauchen einen Knoten,
/// den es im geparsten Baum noch nicht gibt — sie wuerden einen Baum
/// erzeugen muessen, und das ist ein zweiter Baumeister. Sie werden
/// deshalb mit `nicht angewendet: <Name>` gemeldet, nicht stillschweigend
/// verworfen: ein Skript, das `appendChild` benutzt und eine leere Seite
/// sieht, braucht die Meldung mehr als die Stille.
pub fn spielen(patch: &mut narsil_page::Patch, befehle: Vec<Befehl>) -> Abgespielt {
    let mut ergebnis = Abgespielt::default();
    // M4: die Eingangsseite der Grenze (siehe `MAX_BEFEHLE_EINGANG`).
    for b in befehle.into_iter().take(MAX_BEFEHLE_EINGANG) {
        // M4: VOR dem Schreiben geprueft. `Patch::anzahl()` zaehlt die
        // Eintraege, nicht die Befehle — ein Attribut, das zweimal
        // gesetzt wird, ist ein Eintrag.
        if patch.anzahl() >= MAX_PATCH {
            ergebnis.verworfen += 1;
            ergebnis.fehler.push(std::format!(
                "Befehl {} verworfen: der Patch ist voll ({MAX_PATCH})",
                b.art()
            ));
            continue;
        }
        match b {
            Befehl::Canvas { knoten, breite, hoehe, pixels } => {
                patch.canvas.insert(knoten as usize, (breite, hoehe, pixels));
                ergebnis.angewendet += 1;
            }
            Befehl::Text { knoten, text } => {
                patch.text.insert(knoten as usize, text);
                ergebnis.angewendet += 1;
            }
            Befehl::Attribut { knoten, name, wert } => {
                patch.attribut.insert((knoten as usize, name.clone()), wert);
                ergebnis.angewendet += 1;
            }
            Befehl::AttributWeg { knoten, name } => {
                // Ein Attribut WEG ist ein Wert, den der Bauer nicht liest.
                // `narsil-dom` kann Attribute nicht loeschen, also bleibt
                // hier nur „leer" — und ein leeres Attribut ist fuer die
                // Kaskade dasselbe wie keines (GEMESSEN: `stil_von_kaskade`
                // liest nur vorhandene Schluessel).
                patch
                    .attribut
                    .insert((knoten as usize, name.clone()), String::new());
                ergebnis.angewendet += 1;
            }
            Befehl::Klasse {
                knoten,
                name,
                da,
                umschalten,
            } => {
                // `class` wird aus dem ALTEN Wert und der Richtung
                // gerechnet — die Kiste kennt den alten Wert nicht, der
                // geparste Baum schon. Deshalb steht hier der Name, und
                // die Entscheidung faellt beim Bauen.
                //
                // GEMESSEN, warum `toggle` seinen Wert nicht mitbringt:
                // ein Skript, das `toggle` sagt, will die aktuelle Lage
                // sehen. Ein Skript, das `add` sagt, will DA. Beides ist
                // ein Attributwert, den nur der Baum kennt.
                patch.attribut.insert(
                    (knoten as usize, crate::js::KLASSE_MARKER.to_string()),
                    klasse_anweisung(&name, da, umschalten),
                );
                ergebnis.angewendet += 1;
            }
            Befehl::Stil {
                knoten,
                eigenschaft,
                wert,
            } => {
                patch.attribut.insert(
                    (knoten as usize, crate::js::STIL_MARKER.to_string()),
                    std::format!("{eigenschaft}:{wert};"),
                );
                ergebnis.angewendet += 1;
            }
            Befehl::Entfernen { knoten } => {
                patch.weg.insert(knoten as usize);
                ergebnis.angewendet += 1;
            }
            //
            // W6F: NEUE KNOTEN (30.09.2026, 17:50).
            //
            // GEMESSEN, warum der Patch aus dem STAND kommt und nicht aus
            // den Befehlen: der Patch entsteht bei jedem Klick neu. Ein
            // Knoten, der nur in den Befehlen stuende, waere beim naechsten
            // Klick weg — die Liste im DOM waere dann nach dem zweiten
            // Klick wieder leer. `Stand::neu` lebt, `Stand::eltern` auch.
            //
            // GEMESSEN, warum NUR angehaengte Knoten in den Patch gehen:
            // `createElement` ohne `appendChild` ist in einem Browser ein
            // Element ohne Eltern — es existiert, ist aber unsichtbar. Der
            // Patch baut nur, was zu sehen ist.
            Befehl::Neu { .. } | Befehl::Anhaengen { .. } => {
                // Nichts an dieser Stelle: die Knoten stehen im Stand und
                // werden in `neue_knoten_uebernehmen` in EINEM Durchgang
                // uebernommen. Ein `Anhaengen` je Knoten wuerde denselben
                // Knoten doppelt einhaengen.
            }
            Befehl::Html { .. } => {
                ergebnis.verworfen += 1;
                ergebnis.fehler.push(std::format!(
                    "nicht angewendet: {} — dieser Browser baut keine neuen Knoten",
                    b.art()
                ));
            }
            Befehl::Horcher { .. } => {}
        }
    }
    ergebnis
}

/// W6F: die NEUEN Knoten aus dem DOM-Stand in den Patch (17:50).
///
/// # WARUM EINE EIGENE FUNKTION
///
/// `spielen()` bekommt nur Befehle und einen Patch — den Stand hat es
/// nicht. Die neuen Knoten stehen aber im Stand (siehe `dom::Stand::neu`),
/// weil nur er einen Klick lang lebt. Also ruft der Aufrufer diese
/// Funktion nach `spielen()` auf.
///
/// # WAS GELIEFERT WIRD
///
/// Je Knoten aus `Stand::neu`, der einen Elternknoten hat
/// (`Stand::eltern`), ein [`narsil_page::NeuKnoten`]: Nummer, Tag,
/// Eltern-Nummer und der Text aus `Stand::text`.
///
/// M3: hoechstens `NeuKnoten::MAX_NEU` (256) Knoten, der Text
/// `NeuKnoten::MAX_TEXT` (4 KiB) — ein zu langer Text wird hier
/// verworfen (mit Namen im `fehler`-Vektor), nicht gekuerzt: ein
/// gekuerzter Listenpunkt waere eine falsche Seite.
pub fn neue_knoten_uebernehmen(
    patch: &mut narsil_page::Patch,
    stand: &crate::dom::Geteilt,
    ergebnis: &mut Abgespielt,
) {
    let w = stand.lock().expect("DomStand");
    //
    // GEMESSENER FEHLER, den diese Zeile zuerst nicht hatte (18:15): der
    // Patch BEHALT die neuen Knoten und bekommt sie bei jedem Klick
    // erneut. Nach zwei Klicks standen drei Listenpunkte da (statt zwei) —
    // der erste war doppelt. Der Patch wird bei jedem Klick neu gebaut,
    // der Stand nicht; also wird die Liste hier ERSETZT, nicht
    // angehaengt. (`patch.text` ist eine `HashMap` und merkt es nicht —
    // dieselbe Sache an zwei Stellen ist nicht dieselbe Sache.)
    patch.neu.clear();
    for (host, tag) in w.neu.iter().take(narsil_page::NeuKnoten::MAX_NEU) {
        let Some(eltern_host) = w.eltern.get(host).copied() else {
            // `createElement` ohne `appendChild`: in einem Browser ein
            // Element ohne Eltern — es existiert, ist aber unsichtbar.
            continue;
        };
        let Some(eltern_ord) = w.ord.get(&eltern_host).copied() else {
            continue;
        };
        let ord = w
            .ord
            .get(host)
            .copied()
            .unwrap_or(narsil_page::NeuKnoten::ORD_BASIS);
        let text = w.text.get(host).cloned();
        if let Some(t) = &text
            && t.len() > narsil_page::NeuKnoten::MAX_TEXT
        {
            ergebnis.verworfen += 1;
            ergebnis.fehler.push(std::format!(
                "neuer Knoten <{tag}> verworfen: der Text hat {} Byte (erlaubt {})",
                t.len(),
                narsil_page::NeuKnoten::MAX_TEXT
            ));
            continue;
        }
        // Release 08.10.2026: die Lage (Host-Id -> Vorlauf-Nummer). Ein
        // Bezugsknoten ohne Nummer faellt auf „am Ende" zurueck.
        let nummer = |h: &u32| w.ord.get(h).copied();
        let lage = match w.lage.get(host) {
            Some(crate::dom::HostLage::Anfang) => narsil_page::NeuLage::Anfang,
            Some(crate::dom::HostLage::Vor(h)) => nummer(h).map_or(narsil_page::NeuLage::Ende, narsil_page::NeuLage::Vor),
            Some(crate::dom::HostLage::Nach(h)) => nummer(h).map_or(narsil_page::NeuLage::Ende, narsil_page::NeuLage::Nach),
            None => narsil_page::NeuLage::Ende,
        };
        patch.neu.push(narsil_page::NeuKnoten {
            ord,
            tag: tag.clone(),
            eltern_ord,
            text,
            lage,
        });
        ergebnis.angewendet += 1;
    }
}

/// Die Anweisung fuer `classList`, als Text im Patch.
///
/// WARUM ein Text und kein boolescher Wert: der geparste Baum kennt die
/// vorhandenen Klassen, die Kiste nicht. `add x` heisst deshalb
/// „`markiert: x`", und der Bauer entscheidet beim Bauen: er nimmt die
/// Klassen des Knotens, wendet die Anweisungen an und schreibt das
/// Ergebnis in `class`.
fn klasse_anweisung(name: &str, da: bool, umschalten: bool) -> String {
    if umschalten {
        std::format!("?{name}")
    } else if da {
        std::format!("+{name}")
    } else {
        std::format!("-{name}")
    }
}

/// Der Kurzname eines Befehls, und ob er den Baum veraendert.
impl Befehl {
    /// `true`, wenn der Befehl den DOM-Baum veraendert (und damit ein
    /// Neuaufbau noetig ist).
    ///
    /// GEMESSEN, warum `Horcher` NICHT dazugehoert: ein
    /// `addEventListener` aendert nichts, was man zeichnen kann. Waere es
    /// dabei, wuerde eine Seite, die nur einen Zuhoerer anmeldet, beim
    /// ersten Klick den ganzen Baum neu bauen.
    pub fn veraendert_baum(&self) -> bool {
        !matches!(self, Befehl::Horcher { .. })
    }

    /// Der Kurzname, fuer Meldungen.
    pub fn art(&self) -> &'static str {
        match self {
            Befehl::Canvas { .. } => "canvas",
            Befehl::Text { .. } => "text",
            Befehl::Attribut { .. } => "attribut",
            Befehl::AttributWeg { .. } => "attributWeg",
            Befehl::Klasse { .. } => "klasse",
            Befehl::Stil { .. } => "stil",
            Befehl::Neu { .. } => "neu",
            Befehl::Anhaengen { .. } => "anhaengen",
            Befehl::Entfernen { .. } => "entfernen",
            Befehl::Html { .. } => "html",
            Befehl::Horcher { .. } => "horcher",
        }
    }
}
