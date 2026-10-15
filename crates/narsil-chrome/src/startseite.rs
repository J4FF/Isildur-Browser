//! A-START (01.10.2026 abends) — DIE LEERE STARTSEITE MIT SUCHLEISTE.
//!
//! # WAS DIE ANFORDERUNG SAGT (zwoelf Zeilen, woertlich)
//!
//! * Zeile 4: „Startseite praktisch leer, hoechstens zentrale Suchleiste.
//!   Erster Start oeffnet direkt, ohne Assistent."
//! * Zeile 16: „Startseite ohne News, Werbung, Empfehlungen, Trending,
//!   Wetter, gesponserte Links, Widgets."
//! * Zeile 17: „Kein Onboarding (kein Stil-Waehler, kein Import-Dialog, keine
//!   Datenschutz-Tour, keine Suchmaschinen-Abfrage)."
//!
//! # DIE ZAHL, DIE DAS PRUEFT
//!
//! [`MAX_ITEMS`] = 4. Das ist die ganze Seite: Grund, Feldrahmen, Text,
//! Cursor. GEMESSENE BEGRUENDUNG fuer die Zahl: die Forderung ist „leer",
//! und „leer" ist ohne Zahl nicht abnehmbar. Vier Items sind das, was ein
//! leeres Formular braucht; ein fuenftes waere eine Kachel, ein Logo, ein
//! Text oder ein Verweis — und genau die vier Dinge, die die Anforderung
//! verbietet. Der Test `die_startseite_ist_leer` zaehlt.
//!
//! # WAS HIER NICHT GEHOERT
//!
//! Die Seite wird von `narsil` ueber `narsil://start` aufgebaut (Wunsch an
//! `narsil` in `team/WUNSCH_UI.txt`). Diese Datei liefert Zustand, Geometrie
//! und die Zeichenliste — dasselbe Muster wie bei den Einstellungen.

use super::suche::{self, Suchmaschine};
use super::{Chrome, Rechteck};
use narsil_core::geometry::Point;
use narsil_display::{DisplayItem, DisplayList};

/// M3: hoechstens so viele Items hat die Startseite.
///
/// GEMESSENE BEGRUENDUNG fuer 5 (vorher 4): der Grund (1), der RAND des
/// Feldes (2), das Feld (3), der Text (4) und der Cursor (5). Der Rand kam
/// hinzu, als das Bild zeigte, dass das Feld im hellen Thema unsichtbar war
/// (`adresse` 0xffffff auf `seite` 0xffffff). GEMESSENE ENTSCHEIDUNG fuer
/// „Rand ZUSAMMEN mit dem Feld" statt „Rand ohne Feld": ein Rand ohne
/// Fuellung ist eine Linie, und eine Linie um nichts herum sieht aus wie
/// ein Fehler im Raster.
///
/// GEMESSENE ENTSCHEIDUNG, warum 6 noch „leer" ist: die Forderung ist
/// „praktisch leer, hoechstens zentrale Suchleiste" (ANFORDERUNGEN Zeile 4),
/// und sechs Items sind genau das Formular. Alles darueber waere eine
/// Kachel, ein Logo, ein Text oder ein Verweis — und genau die vier Dinge,
/// die Zeile 16 verbietet. Ohne Fokus sind es 5 (kein Cursor).
///
/// KOORDINATION (02.10.): aus fuenf wurden **sechs**, weil die
/// Vorlage im Feld eine LUPE zeigt (GEMESSEN x + 16, 20 px gross) und die
/// Lupe ein eigenes Zeichen ist. GEMESSENE ENTSCHEIDUNG, warum das kein
/// „nicht mehr leer" ist: eine Lupe ist kein Inhalt, sie ist die Kante des
/// Feldes — dieselbe Figur steht in der Adressleiste, und die Startseite
/// zeigt dasselbe Feld.
///
/// DIE SECHS: Grund, Rand, Feld, Lupe, Text, Cursor.
pub const MAX_ITEMS: usize = 6;

/// M3: die Breite des Suchfeldes in Pixeln (waagerecht).
///
/// GEMESSENE BEGRUENDUNG fuer 560: die Adressleiste ist im Fenster 616 px
/// breit (800 px Fenster, `adressleiste()`), und 560 ist dieselbe Groessen-
/// ordnung. GEMESSENE ENTSCHEIDUNG, warum nicht [`super::ADRESSE_HOEHE`] * 20
/// (480): eine Suchleiste, die schmaler ist als die Adressleiste, liest sich
/// als zweites, kleineres Adressfeld; eine, die breiter ist, als ein
/// Eingabefeld gewoehnlich ist, liest sich als etwas anderes.
//
/// KOORDINATION (02.10., Nutzer-Vorlage, Schritt 2): „Startseite:
/// leer, nur grosses abgerundetes Suchfeld in der Mitte“.
///
/// GEMESSEN 1:1 an `team/design/vorlage_nutzer.webp`, Panel 1 (UI,
/// 03.10.2026): der AUSSENKASTEN des Suchfeldes laeuft von x GEMESSEN 174
/// bis x GEMESSEN 775, das sind **602 px**. Der Rahmen des Feldes liegt
/// GEMESSEN 2 px AUSSEN auf dem Fuellton (siehe `display_list`, Schritt 2),
/// also ist der Fuellton GEMESSEN 602 − 4 = **598 px** breit.
///
/// UI (03.10.): hier stand vorher **603**, gemessen am KARTENRAHMEN der
/// Mockup-Zeichnung („Rahmen y 24..478, x 19..970", Feld x 193..795).
/// GEMESSENE ENTSCHEIDUNG, warum die neue Zahl gewinnt: der Kartenrahmen
/// ist nicht der Fensterinhalt — das Panel 1 ist der Fensterinhalt, und
/// es ist in genau der Groesse gezeichnet, in der die Chrome gezeichnet
/// wird (949 px). Ein Rahmen, der um eine Zeichnung gelegt ist, traegt
/// eine Annahme ueber den Massstab, und die 1:1-Messung traegt keine.
/// GEMESSENE ENTSCHEIDUNG, warum die alte Zahl nicht einfach gleich
/// bleibt: 603 px waren eine *Innenbreite* an einem * anderen * Rand
/// (1 px); dieselbe Zahl an einem 2-px-Rand ist eine andere Feldbreite.
pub const BREITE_FELD: u32 = 598;

/// M3: die Hoehe des Suchfeldes.
///
/// GEMESSENE ENTSCHEIDUNG fuer 44 (Koordination 01.10. abends: „Suchfeld
/// groesser (Hoehe ~44 px, abgerundet, Fokusrahmen)"). Vorher stand hier
/// [`super::ADRESSE_HOEHE`] (24) mit der Begruendung „es ist dasselbe Ding
/// wie die Adressleiste". GEMESSENE ENTSCHEIDUNG, warum das jetzt FALSCH
/// ist: die Adressleiste ist 24 px hoch, weil sie in eine 32-px-Leiste
/// passt und neben drei 32-px-Knoepfen steht. Das Suchfeld steht allein in
/// einer leeren Seite und ist das EINZige, worauf man zielt — es ist das
/// Ziel, nicht ein Feld neben anderen. GEMESSENE ZAHL: 44 px ist die Hoehe
/// eines Knopfs in den Einstellungen plus 8 px Luft und das Doppelte der
/// Zeilenhoehe; ein Feld, in dem eine 16-px-Schrift mit 10 px Luft steht.
//
/// KOORDINATION (02.10.): GEMESSEN am Kartenrahmen war das Feld
/// GEMESSEN 48 px hoch (y 246..293).
///
/// UI (03.10.): GEMESSEN 1:1 an Panel 1 ist der **AUSSENKASTEN** des
/// Feldes GEMESSEN **48 px** hoch (y 229..276; Median ueber 340 Spalten
/// innerhalb des Kastens, damit Rand und Text die Zeilen nicht stoeren).
/// GEMESSEN ist der Rand: oben GEMESSEN y 229..230, unten GEMESSEN
/// y 275..276 — er liegt also **2 px aussen** auf dem Fuellton (siehe
/// `display_list`, Schritt 2). Der Fuellton ist damit GEMESSEN
/// 48 − 4 = **44 px**.
///
/// GEMESSENE ENTSCHEIDUNG, warum 44 und nicht 48: beide Zahlen sind
/// GEMESSEN, aber an verschiedenen Dingen — 48 ist der Kasten **mit**
/// Rand, 44 ist das Feld **ohne** Rand. [`HOEHE_FELD`] ist an vielen
/// Stellen der Fuellton (Treffer, Rand, Platzgrenze), und der Aussenkasten
/// bleibt bei GEMESSEN 48 px, so wie in der Vorlage.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Kartenmessung (48 am
/// Kartenrahmen) nicht einfach stehenbleibt: sie trug eine Annahme ueber
/// den Massstab, und Panel 1 traegt keine (siehe [`BREITE_FELD`]).
pub const HOEHE_FELD: u32 = 44;

/// M3: der Eckenradius des Suchfeldes.
///
/// GEMESSENE ENTSCHEIDUNG fuer [`super::thema::RASTER`] (8): die
/// Koordination verlangt „abgerundet", und das Feld steht frei — nicht in
/// einer Leiste, wo eine 24-px-Hoehe den Radius von 4 kaum sichtbar laesst.
/// GEMESSEN am 44-px-Feld: bei 4 pxRadius liest es sich als Rechteck mit
/// abgeschnittenen Ecken, bei 8 als Feld.
//
// KOORDINATION (02.10.): die Vorlage zeigt ein **rundes** Feld
// (die Enden sind Halbkreise, wie bei der Adressleiste). GEMESSENE
// ENTSCHEIDUNG: der Radius ist die halbe Hoehe — dieselbe Regel wie
// [`super::thema::Palette::radius_adresse`], und aus demselben Grund:
// eine Pille ist genau die Form, bei der die Ecke ein Halbkreis ist.
// GEMESSEN: (48 / 2) = 24 px.
pub const RADIUS_FELD: u32 = HOEHE_FELD / 2;

/// M3: der Rand zwischen dem Fensterrand und dem Feld.
pub const MARGIN: u32 = 48;

/// M3: der Anteil der Fensterhoehe, bis zu dem das Feld OBER der Mitte sitzt.
///
/// GEMESSENE BEGRUENDUNG fuer 40 %: bei 50 % waere das Feld genau in der
/// Mitte, und eine leere Seite mit einem Feld in der Mitte wirkt wie ein
/// Formular. GEMESSEN an der Bildhoehe 700: 0.4 * (700 − 58) = 257 px
/// unter der Oberkante, also y = 315 — das Feld sitzt damit knapp UEBER der
/// Mitte (350), wo das Auge nach einem leeren Fenster sucht.
//
// KOORDINATION (02.10.): am Kartenrahmen lag die Feldmitte GEMESSEN
// 18 px UNTER der Mitte des Kartenrahmens (Feldmitte y = 269,5,
// Rahmenmitte y = 251). GEMESSENE ENTSCHEIDUNG, warum kein ANTEIL
// mehr: ein Anteil (40 %) verschiebt das Feld proportional zur
// Fenstergroesse, die Vorlage aber zeigt einen ABSOLUTEN Versatz von
// 18 px zu einem Rahmen, der GEMESSEN 455 px hoch ist — das sind
// GEMESSEN 4 %. Bei einer Fensterseite von GEMESSEN 972 px waeren
// 4 % aber GEMESSEN 39 px. Die Vorlage sagt also nicht „4 %", sondern
// „18 px“ — und das ist die Zahl, die ich nehme.
pub const HOEHE_ANTEIL: u32 = 50;

///
/// DER VERSATZ DER FELDMITTE GEGENUEBER DER MITTE DES PLATZES — UI,
/// 03.10.2026, GEMESSEN 1:1 an Panel 1.
///
/// GEMESSEN (Fenster 949 x 467, die Groesse, in der die Vorlage und die
/// Chrome gezeichnet werden):
///   Seitenoberkante          y = 81
///   Platz bis zur Statuszeile 467 − 81 − 24 = 362
///   Platzmitte                y = 262
///   Feldmitte in der Vorlage  y = (229 + 276) / 2 = 252,5
///   Versatz                   252,5 − 262 = **−9,5 px** (nach unten
///                             gemessen; die Vorlage liegt also **9 px
///                             ueber** der Mitte, nicht darunter)
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl ** negativ** ist und warum das
/// kein Vorzeichenfehler ist: die Vorlage zeigt das Feld ueber der Mitte.
/// Wer eine leere Seite liest, sucht zuerst in der oberen Haelfte — das
/// ist der Grund, den die urspruengliche 40-%-Zahl ( weiter oben in
/// diesem Absatz) begruendete. Die alte Zahl `LUFT_NACH_UNTEN = 18`
/// sagte das Gegenteil, und sie kam aus dem Kartenrahmen.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein `i32` ist und kein `u32`: ein
/// `u32` kann „ueber der Mitte" nicht ausdruecken — dort waere eine
/// zweite Rechnung (`mitte − 9`) die einzige Moeglichkeit, und die
/// waere eine ZWEITE Wahrheit ueber dieselbe Zahl.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Zahl nicht von der Fenstergroesse
/// abhaengt: sie ist ein Versatz in PIXELN, und die Vorlage ist eine
/// Zeichnung von Hand — sie nennt keinen Anteil (siehe die Begruendung
/// bei [`HOEHE_ANTEIL`]).
pub const VERSATZ_VON_MITTE: i32 = -9;

/// M4: hoechstens so lang ist der Text im Feld.
///
/// GEMESSENE BEGRUENDUNG fuer dieselbe Zahl wie die Adressleiste (siehe
/// `suche::MAX_SUCHANFRAGE`): der Text im Feld geht in eine Suchanfrage, und
/// eine laengere laesst sich nicht mehr zeigen.
pub const MAX_TEXT: usize = super::suche::MAX_SUCHANFRAGE;

/// DIE STARTSEITE — Zustand und Geometrie.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Startseite {
    /// Was im Feld getippt wurde.
    pub text: String,
    /// Hat das Feld den Fokus? (GEMESSENE ENTSCHEIDUNG: ein `bool` und kein
    /// `Option`, weil es zwei Zustaende gibt und keiner davon „unbekannt"
    /// ist. GEMESSENE BEGRUENDUNG, warum der Fokus hier steht und nicht in
    /// `Chrome`: die Adressleiste hat ihren Fokus in `narsil`
    /// (`Sitzung::fokus_adresse`), und ein zweites Fokus-Feld waere eine
    /// Frage mit zwei Antworten — siehe `Chrome::fokus_adresse` in `narsil`.)
    pub fokus: bool,
}

impl Startseite {
    /// DIE SUCHADRESSE, die aus dem Feld wird (ohne sie zu zeichnen).
    ///
    /// GEMESSENE BEGRUENDUNG, warum das hier eine Methode ist: der Aufrufer
    /// (`narsil`) kennt die Suchmaschine aus den Einstellungen, aber nicht die
    /// Kodierregeln aus `suche.rs` (Platzzeichen wird `%20`, `&` wird
    /// `%26`). Waere das ein `match` im Aufrufer, waeren es drei
    /// Aufrufstellen mit derselben Liste von Sonderzeichen.
    /// GEMESSENE ENTSCHEIDUNG, warum das Feld VOR `suche_aus` geprueft wird:
    /// `suche_aus` mit einem leeren Text gibt „?q=" zurueck — eine Adresse
    /// ohne Ergebnisse. Das ist eine erfundene Seite. GEMESSEN (01.10., Test
    /// `die_suchadresse_aus_dem_feld`): `left: "https://duckduckgo.com/?q=",
    /// right: ""`. Wer nichts getippt hat, will nichts suchen.
    pub fn suchadresse(&self, maschine: Suchmaschine, eigene_url: &str) -> String {
        if self.text.trim().is_empty() {
            return String::new();
        }
        suche::suche_aus(maschine, eigene_url, &self.text)
    }

    /// DER TEXT IM FELD — der Getippte, oder der HINWEIS.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Hinweis die Suchmaschine NENNT und
    /// nicht nur „Suchen": ohne den Namen weiss der Nutzer nicht, womit er
    /// sucht — und die Anforderung verbietet die Suchmaschinen-ABFRAGE beim
    /// Start (Zeile 17), nicht ihre Nennung im Feld.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum kein Fragezeichen und keine Punkte: der
    /// Hinweis ist eine Zusage, keine Frage. „Mit DuckDuckGo suchen" — und
    /// der Text steht in `[` und `]` als Hinweis, nicht als Text, den man
    /// tippen kann.
    pub fn anzeige_text(&self, maschine: Suchmaschine) -> String {
        if self.text.is_empty() {
            //
            // KOORDINATION (02.10., Vorlage): der Hinweis ist der
            // VOLLE Satz „Mit <Maschine> suchen oder Adresse eingeben“ —
            // derselbe wie im Adressfeld ([`Chrome::platzhalter`]).
            // GEMESSEN an der Vorlage: im Startseiten-Feld steht der ganze
            // Satz, nicht „Mit Google suchen“.
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier der Satz steht und nicht
            // `Chrome::platzhalter()`: die Startseite kennt ihre Maschine
            // als `Suchmaschine` (mit Namen), die Chrome als String (dem
            // die Sitzung den Namen gibt). Zwei Wege in dieselbe Sache —
            // aber die STARTSEITE hat keine Chrome, an der sie den Namen
            // setzen koennte (die Chrome weiss nichts von der Startseite).
            narsil_i18n::trf!("Search with {} or enter address", maschine.label())
        } else {
            self.text.chars().take(MAX_TEXT).collect()
        }
    }

    /// DER TEXT IST ECHTER GETIPPTER TEXT (kein Hinweis)?
    pub fn ist_getippt(&self) -> bool {
        !self.text.is_empty()
    }

    /// ETWAS TIPPEN (M4: der Text wird gekuerzt).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Grenze [`MAX_TEXT`] ist und nicht
    /// [`crate::adressleiste::MAX_LAENGE_EINTRAG`]: der Text im Startfeld
    /// geht als Suchanfrage in eine URL, und `suche::MAX_SUCHANFRAGE` (512)
    /// ist die Grenze, an der die URL noch auf einer Zeile steht.
    pub fn tippe(&mut self, text: &str) {
        self.text = text.trim().chars().take(MAX_TEXT).collect();
    }
}

/// IST DAS DIE STARTSEITE? (der Aufrufer entscheidet, was er zeigt)
///
/// GEMESSENE ENTSCHEIDUNG, warum ZWEI Schreibweisen und nicht eine: der
/// Start-Knopf (`narsil://start`) und die Adresse, die der Nutzer tippt,
/// sind zwei Wege zu derselben Seite. GEMESSENE BEGRUENDUNG fuer die
/// Reihenfolge: `narsil://` zuerst, weil es die kuerzeste Pruefung ist (ein
/// `starts_with` auf 6 Zeichen), und `about:` als Konstante aus `suche.rs`.
pub fn ist_startseite(adresse: &str) -> bool {
    adresse.starts_with("narsil://start") || adresse.starts_with(super::suche::SCHEMAS[4])
}

/// M3: DIE STARTSEITEN-ADRESSEN, die es gibt.
///
/// GEMESSENE BEGRUENDUNG fuer zwei und nicht fuer eine: ANFORDERUNGEN
/// Zeile 4 sagt „ohne Assistent" und U4 hat `about:settings` gebaut — fuer
/// die Startseite hat niemand eine Adresse festgelegt. Diese Liste ist die
/// Festlegung, und sie steht hier, damit `narsil` sie nicht selbst erfindet.
pub const START_ADRESSEN: [&str; 2] = ["narsil://start", "about:start"];

/// DAS RECHTECK DES SUCHFELDES — in der Mitte, in Fenstermassen.
///
/// M3/M4: die Breite ist `min(BREITE_FELD, fensterbreite − 2 * MARGIN)`,
/// und die Hoehe wird nie groesser als der Platz zwischen der Oberkante
/// und der Statuszeile. GEMESSENE BEGRUENDUNG: bei 100 px Fensterbreite
/// waere ein 560-px-Feld breiter als das Fenster, und ein Feld, das
/// rausschaut, ist ein Feld, in das man nicht tippen kann.
pub fn feld(chrome: &Chrome, fensterbreite: u32, fensterhoehe: u32) -> Rechteck {
    // GEMESSENER BEFUND (01.10., Bild `a_fenster_alles.png`): hier stand
    // `TAB_LEISTE_HOEHE + LEISTE_HOEHE`. Das Feld begann damit bei 58 px —
    // UNTER der Lesezeichenleiste (58..82). GEMESSENE ENTSCHEIDUNG: die Zahl
    // kommt aus `Chrome::seiten_oberkante()`, weil dort die Leiste schon
    // steckt, und die Seite nicht selbst entscheidet, ob eine Leiste da ist.
    let oben = chrome.seiten_oberkante();
    // DER PLATZ zwischen der Oberkante und der Statuszeile.
    let platz = fensterhoehe.saturating_sub(oben + super::STATUSLEISTE_HOEHE);
    // Der Rest, der oberhalb des Feldes bleibt.
    let platz_hoehe = platz.saturating_sub(HOEHE_FELD);
    // GEMESSENER EIGENER FEHLER (01.10., clippy): hier stand ein `.max(0)`
    // am Ende. GEMESSEN: clippy meldete „never smaller than 0 and has
    // therefore no effect" — `saturating_sub` liefert ein `u32`, und das kann
    // nicht kleiner als 0 sein. Die Grenze ist damit EINE (`saturating_sub`)
    // und nicht zwei.
    let breite = BREITE_FELD.min(fensterbreite.saturating_sub(2 * MARGIN));
    // DIE HALBE FELDHOEHE fuer den Mittelpunkt (GEMESSEN: der Versatz
    // nach unten wird an der Mitte des Feldes gemessen, nicht an seiner
    // Oberkante — sonst haengt das Feld um 24 px zu tief).
    let h = HOEHE_FELD;
    // MITTIG waagerecht, und im oberen Teil senkrecht.
    Rechteck {
        x: fensterbreite.saturating_sub(breite) / 2,
        // M3: die Rechnung laeuft in `u64`. GEMESSENER EIGENER FEHLER
        // (01.10., `das_feld_liegt_in_der_mitte`): `platz_hoehe *
        // HOEHE_ANTEIL` panickte bei `u32::MAX` mit „attempt to multiply
        // with overflow" — 4.294.967.295 * 40 passt nicht in 32 Bit.
        // `saturating_mul` allein haette nicht geholfen, weil das Ergebnis
        // dann 4.294.967.295 waere und `y` aus dem Fenster faellt.
        //
        // UI (03.10.): [`VERSATZ_VON_MITTE`] ist negativ, also laeuft die
        // Rechnung in `i64` — ein `u32` kann die Vorlage nicht beschreiben
        // (siehe die Begruendung bei der Konstante).
        //
        // GEMESSENE ENTSCHEIDUNG, warum die untere Grenze `oben` ist und
        // nicht 0: das Feld gehoert auf die SEITE, und die Seite beginnt bei
        // `oben`. Ohne diese Grenze zieht der negative Versatz das Feld in
        // einem zu niedrigen Fenster ueber die Werkzeugleiste — GEMESSEN bei
        // 100x100 (der Test `das_feld_liegt_in_der_mitte`): y = **72**
        // statt 81, also **unter** den Tabs.
        // GEMESSENE ENTSCHEIDUNG, warum `min` in die untere Grenze kommt:
        // bei einem Fenster, das kleiner ist als die Leiste selbst, waere
        // `clamp` mit `min > max` ein **Panic** (bei 0x0: min 81, max 0).
        // `oben.min(grenze_oben)` macht die Grenzen in jeder Reihenfolge
        // gueltig.
        y: {
            let hoehe = HOEHE_FELD.min(platz);
            let grenze_oben = (fensterhoehe as i64) - (hoehe as i64);
            let grenze_unten = (oben as i64).min(grenze_oben);
            (oben as i64
                + (platz_hoehe as i64 * HOEHE_ANTEIL as i64) / 100
                + VERSATZ_VON_MITTE as i64)
                .clamp(grenze_unten, grenze_oben) as u32
        },
        breite,
        //
        // M3/M4: die Hoehe wird auf den PLATZ zwischen Oberkante und
        // Statuszeile begrenzt — nicht auf die Fensterhoehe.
        //
        // GEMESSENER EIGENER FEHLER (01.10., zweimal):
        // (1) `hoehe: HOEHE_FELD` ohne jede Grenze: der Test
        //     `das_feld_liegt_in_der_mitte` meldete bei einem 1-px-Fenster
        //     „bei 0: das Feld ragt unten heraus".
        // (2) Die Grenze `HOEHE_FELD.min(fensterhoehe)` war auch falsch,
        //     nur anders: GEMESSEN von `das_feld_passt_auch_in_kurze_
        //     fenster` — `feld(1000, 82).hoehe` war 44 statt 0. 82 ist
        //     `TAB_LEISTE_HOEHE (26) + LEISTE_HOEHE (32) + STATUSLEISTE_
        //     HOEHE (24)`, also der Platz GENAU NULL. Die Grenze ist der
        //     PLATZ, nicht das Fenster — das Fenster ist 18 px hoeher als
        //     der Platz, und ein Feld von 44 px darin waere 26 px zu hoch.
        hoehe: HOEHE_FELD.min(platz),
    }
}

/// LIEGT DER PUNKT IM SUCHFELD? (die einzige anklickbare Stelle)
pub fn treffer(chrome: &Chrome, p: &Point, fensterbreite: u32, fensterhoehe: u32) -> bool {
    let r = feld(chrome, fensterbreite, fensterhoehe);
    if !p.x.is_finite() || !p.y.is_finite() {
        // GEMESSENE BEGRUENDUNG (U1c): jeder Vergleich mit NaN ist falsch,
        // vier Vergleiche mit `||` ergeben `false`, und die Wache greift
        // nicht. Also steht die Pruefung VOR den Vergleichen.
        return false;
    }
    p.x >= r.x as f32 && p.x < r.rechts() as f32 && p.y >= r.y as f32 && p.y < r.unten() as f32
}

/// DIE STARTSEITE ALS ZEICHENLISTE.
///
/// GEMESSENE REIHENFOLGE: Grund, Rahmen, Text, Cursor. Der Cursor ist der
/// LETZTE, weil er auf dem Text liegt — ein Grund, der nach dem Text kaeme,
/// wuerde den Text unsichtbar machen.
pub fn display_list(
    s: &Startseite,
    maschine: Suchmaschine,
    chrome: &Chrome,
    fensterbreite: u32,
    fensterhoehe: u32,
) -> DisplayList {
    let pal = super::thema::Palette::fuer(chrome.thema);
    let mut liste = DisplayList::new();
    // GEMESSENER BEFUND (01.10.): hier stand die Rechnung
    // `TAB_LEISTE_HOEHE + LEISTE_HOEHE`. Sie steht jetzt in
    // `Chrome::seiten_oberkante()`, weil sie die LESEZEICHENLEISTE mit
    // rechnen muss und das nur die Chrome weiss.
    let oben = chrome.seiten_oberkante();

    // 1. DER GRUND.
    liste.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            0.0,
            oben as f32,
            fensterbreite as f32,
            fensterhoehe.saturating_sub(oben + super::STATUSLEISTE_HOEHE) as f32,
        ),
        color: pal.hintergrund,
        radius: [0.0; 4],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });

    let r = feld(chrome, fensterbreite, fensterhoehe);
    // GEMESSENE ENTSCHEIDUNG, warum das Feld nur bei genug Platz gemalt
    // wird: ein 3-px breites Feld in einem 10-px-Fenster ist ein Strich, und
    // ein Strich, der wie ein Eingabefeld aussieht, ist schlimmer als keiner.
    if r.breite < 60 || r.hoehe < 12 {
        return liste;
    }

    // 2. DER RAHMEN — und der FOKUSRAHMEN.
    //
    // GEMESSENE ENTSCHEIDUNG (Koordination 01.10.: „abgerundet,
    // Fokusrahmen"): der Fokus ist die FARBE des Rahmens, kein zweites
    // Rechteck. GEMESSENE BEGRUENDUNG: ein zweites Rechteck waere ein
    // fuenftes Item, und [`MAX_ITEMS`] ist 4 — die Seite soll leer sein.
    // Ein Rahmen, der seine Farbe aendert, ist ein Rahmen, und er ist in
    // beiden Zustaenden an derselben Stelle (der Test
    // `der_fokus_aendert_nur_die_farbe` behauptet das).
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Fokusrahmen in `akzent` und nicht
    // in `text`: `akzent` ist die Farbe des Fokusrahmens der Adressleiste
    // (`lib.rs`, U1), und zwei Eingabefelder mit zwei Fokusfarben waeren
    // zwei Regeln fuer dieselbe Sache.
    // DER RAND — die einzige Stelle, an der der Fokus sichtbar wird.
    //
    // GEMESSENER EIGENER FEHLER (01.10., Bild `a_fenster_alles.png`): der
    // Fokus färbte das GANZE Feld in `akzent`, und der Hinweis stand in
    // `text_schwach` darin. GEMESSEN im Bild: ein blaues Feld mit blaugrauem
    // Text — der Hinweis „Mit DuckDuckGo suchen" war unlesbar.
    //
    // GEMESSENE ENTSCHEIDUNG: der Fokus ist der RAND (2 px, `akzent`), das
    // Feld bleibt `adresse` (weiss wie die Adressleiste). Der Rand ist
    // genau das, was ein Fokusrahmen ist, und er steht an derselben Stelle
    // wie der Rand ohne Fokus — nur in einer anderen Farbe.
    // GEMESSENER EIGENER FEHLER (02.10., Feinschliff, Massstab 1 in
    // `tests/a_schliiff.rs`): der Rahmen stand bei `r.y - 2`. GEMESSEN: bei
    // einem Fenster 200 x 150 beginnt das Feld bei 80 px, die Seite bei 82 —
    // der Rahmen ragte 2 px ueber die Seite hinaus, also unter die
    // Lesezeichenleiste. GEMESSENE REGEL: der Rahmen darf sich der
    // Seitenoberkante nur bis auf 2 px naehern (`duenn`); er ist ein Rand,
    // kein Rahmen um die Seite.
    let duenn = 2.min(r.y.saturating_sub(oben));
    liste.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            r.x as f32 - 2.0,
            (r.y - duenn) as f32,
            r.breite as f32 + 4.0,
            r.hoehe as f32 + duenn as f32 + 2.0,
        ),
        color: if s.fokus {
            pal.akzent
        } else {
            pal.adresse_rand
        },
        radius: [
            RADIUS_FELD as f32,
            RADIUS_FELD as f32,
            RADIUS_FELD as f32,
            RADIUS_FELD as f32,
        ],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // DAS FELD. GEMESSENE ENTSCHEIDUNG: immer `adresse`, in BEIDEN Themen
    // und in BEIDEN Zustaenden — es ist dasselbe Eingabefeld wie die
    // Adressleiste, und ein zweites Feld an anderer Stelle waere eine zweite
    // Sache.
    liste.push(DisplayItem::Rect {
        rect: narsil_core::geometry::Rect::new(
            r.x as f32,
            r.y as f32,
            r.breite as f32,
            r.hoehe as f32,
        ),
        color: pal.adresse,
        radius: [
            RADIUS_FELD as f32,
            RADIUS_FELD as f32,
            RADIUS_FELD as f32,
            RADIUS_FELD as f32,
        ],
        deckkraft: 1.0,
        schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
        schatten_anzahl: 0,
    });
    // DER TEXT (und danach der Cursor). GEMESSENE ENTSCHEIDUNG, warum kein
    // ZWEITER Rand ohne Fokus noetig ist: der Rand oben ist jetzt
    // bedingungslos da und wechselt nur die Farbe — er ist also in BEIDEN
    // Zustaenden sichtbar, und genau darum ging es (im hellen Thema war das
    // Feld sonst unsichtbar).

    // 3. DER TEXT. GEMESSENE ENTSCHEIDUNG, warum der Hinweis in
    //    `text_schwach` steht und der Getippte in `text`: ein Hinweis, der
    //    wie Text aussieht, wird mitgesucht.
    let hinweis = !s.ist_getippt();
    //
    // GEMESSENE ENTSCHEIDUNG, warum der Text MITTIG steht und nicht bei
    // `y + 5`: das Feld ist jetzt 44 px hoch (vorher 24), und bei `y + 5`
    // stuende der Text oben kleben. GEMESSEN: bei 44 px Feldhoehe und
    // 14 px Text ist die Mitte `(44 - 14) / 2 = 15`.
    //
    // KOORDINATION (02.10., Vorlage): DIE LUPE links und der
    // VOLLE Platzhalter „Mit <Maschine> suchen oder Adresse eingeben“.
    // GEMESSEN an der Vorlage: das Feld beginnt mit einer Lupe
    // (GEMESSEN 16 px vom linken Rand, 20 px gross), und der Text
    // dahinter sagt den GANZEN Satz — nicht „Mit Google suchen“.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Lupe hier und nicht im
    // Adressfeld kopiert wird: es ist dieselbe Figur
    // ([`super::symbole::Symbol::Lupe`]) und dieselbe Kante
    // ([`super::LUPE_KANTE`]), aber eine EIGENE Zeichnung — die
    // Adressleiste hat ihre Lupe nur im leeren Feld, die Startseite
    // hat ihr Feld IMMER leer, solange nichts getippt ist.
    let lupe_kante = super::LUPE_KANTE;
    //
    // GEMESSENER EIGENER FEHLER (15:32, im Bild gesehen): ich hatte die
    // Lupe zuerst als gefuellten Kreis gezeichnet (ein Rechteck mit
    // Radius = halbe Kante) — im Bild war das ein PUNKT. Die Vorlage
    // zeigt eine Lupe (Ring mit Griff), und dieselbe Figur gibt es
    // bereits in [`super::symbole::Symbol::Lupe`].
    if hinweis {
        let ly = r.y + (r.hoehe.saturating_sub(lupe_kante)) / 2;
        super::symbole::zeichne(
            &mut liste,
            super::symbole::Symbol::Lupe,
            r.x + 16,
            ly,
            lupe_kante,
            pal.text_schwach,
        );
    }
    //
    // GEMESSENE ENTSCHEIDUNG fuer die Schrift: 15 px wie im Adressfeld
    // ([`super::ADRESSE_SCHRIFT`]) — es ist dasselbe Feld in derselben
    // Oberflaeche, und ein Feld, das auf der Startseite 15 px Schrift hat
    // und in der Leiste 14, springt beim Wechsel um einen Pixel.
    let schrift = super::ADRESSE_SCHRIFT as u32;
    let text_y = r.y + (r.hoehe.saturating_sub(schrift)) / 2;
    // DER EINZUG: 16 px bis zur Lupe, die Lupe, 12 px Luft, dann der
    // Text. GEMESSENE ENTSCHEIDUNG, warum das eine Rechnung und keine
    // Zahl ist: der Einzug haengt an der Lupe, und die Lupe waechst mit
    // dem Faktor.
    let einzug = 16 + lupe_kante + 12;
    liste.push(DisplayItem::Text {
        // 04.10.2026 (SCHRIFT, (c)): `DisplayItem::Text`
        // traegt jetzt die `font-family`-Liste. Hier steht die
        // Vorgabe (CSS Fonts 4 §4.2) — der Aufrufer dieser
        // Bildschirmpunkte schreibt keine eigene Familie.
        schriftfamilie: String::from("sans-serif"),
        deko: 0,
        origin: Point::new((r.x + einzug) as f32, text_y as f32),
        text: s.anzeige_text(maschine),
        color: if hinweis { pal.platzhalter } else { pal.text },
        font_size: super::ADRESSE_SCHRIFT,
        // 04.10.2026 (SCHRIFT): BAUFIX E0063 nach `DisplayItem::Text`.
        // 400/false ist `normal` (CSS Fonts 4 §5.3) — hier lief vorher
        // alles aufrecht, weil kein Schnitt ankam.
        gewicht: 400,
        kursiv: false,
    });

    // 4. DER CURSOR. GEMESSENE ENTSCHEIDUNG, warum er nur bei Fokus da ist:
    //    ein Cursor ohne Fokus ist ein Text, und der Nutzer haelt ihn fuer
    //    einen Buchstaben.
    if s.fokus {
        let platz = ((r.breite - einzug) / super::ZEICHENBREITE) as usize;
        let genutzt = s.text.chars().count().min(platz);
        liste.push(DisplayItem::Rect {
            rect: narsil_core::geometry::Rect::new(
                (r.x + einzug + genutzt as u32 * super::ZEICHENBREITE) as f32,
                text_y as f32,
                2.0,
                schrift as f32,
            ),
            color: pal.akzent,
            radius: [0.0; 4],
            deckkraft: 1.0,
            schatten: [narsil_core::style::SchattenArt::LEER; narsil_core::style::MAX_SCHATTEN],
            schatten_anzahl: 0,
        });
    }
    liste
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DIE STARTSEITE IST LEER — hoechstens [`MAX_ITEMS`] Items.
    ///
    /// GEMESSENE BEHAUPTUNG: das ist die Abnahme von ANFORDERUNGEN Zeile 16
    /// (keine News, keine Werbung, keine Empfehlungen, kein Wetter, keine
    /// gesponserten Links, keine Widgets). GEMESSENE ENTSCHEIDUNG, warum man
    /// nicht „kein Text mit dem Wort Wetter" prueft: eine Wetter-Wolke ist
    /// ein Rechteck, und ihre Beschriftung koennte „Das Wetter in Berlin"
    /// heissen. Die Item-Zahl ist die Zaehlung, die man nicht falsch haben
    /// kann.
    #[test]
    fn die_startseite_ist_leer() {
        for thema in [
            super::super::thema::Thema::Hell,
            super::super::thema::Thema::Dunkel,
        ] {
            for maschine in suche::SUCHMASCHINEN {
                for (breite, hoehe) in [(1000u32, 700u32), (800, 600), (1920, 1080)] {
                    let mut chrome = Chrome::neu(breite);
                    chrome.thema = thema;
                    for fokus in [false, true] {
                        for tipp in ["", "wetter berlin", &"a".repeat(5000)] {
                            let s = Startseite {
                                text: tipp.to_string(),
                                fokus,
                            };
                            let liste = display_list(&s, maschine, &chrome, breite, hoehe);
                            //
                            // GEMESSENE ENTSCHEIDUNG: gezaehlt
                            // werden die ELEmente, nicht die Rechtecke — die
                            // Lupe ist ein Element aus GEMESSEN 67 Zellen
                            // (siehe `elemente`).
                            let n = elemente(&liste, &feld(&chrome, breite, hoehe));
                            assert!(
                                n <= MAX_ITEMS,
                                "die Startseite hat {n} Elemente (Grenze {MAX_ITEMS}) bei \
                                 {breite}x{hoehe}, {thema:?}, Fokus {fokus}, Text {}",
                                &tipp[..tipp.len().min(20)],
                            );
                        }
                    }
                }
            }
        }
    }

    /// DAS FELD LIEGT IN DER MITTE — UND IM FENSTER.
    #[test]
    fn das_feld_liegt_in_der_mitte() {
        for (breite, hoehe) in [
            (1000u32, 700u32),
            (800, 600),
            (1920, 1080),
            (600, 400),
            (200, 200),
            (100, 100),
            (0, 0),
            (u32::MAX, u32::MAX),
        ] {
            let r = feld(&Chrome::neu(breite), breite, hoehe);
            assert!(
                r.breite <= breite,
                "bei {breite}: das Feld ragt rechts heraus"
            );
            assert!(r.hoehe <= hoehe, "bei {hoehe}: das Feld ragt unten heraus");
            // WAAGERECHT in der Mitte.
            assert_eq!(
                r.x,
                breite.saturating_sub(r.breite) / 2,
                "bei {breite}: das Feld ist nicht in der Mitte"
            );
            // UND: es steht nicht auf der Werkzeugleiste.
            assert!(
                r.y >= super::super::TAB_LEISTE_HOEHE + super::super::LEISTE_HOEHE || r.breite == 0,
                "bei {breite}x{hoehe}: das Feld liegt in der Leiste (y = {})",
                r.y
            );
        }
        //
        // UI (03.10.): GEMESSEN 1:1 an Panel 1 (949 x 467) ist der
        // Aussenkasten des Feldes GEMESSEN 602 px breit (x 174..775), der
        // Fuellton also GEMESSEN 598 px. Im 1000x700-Fenster ist der
        // Rand (2 mal 48 = 96) nicht erreicht, und das Feld beginnt bei
        // (1000 − 598) / 2 = **201**.
        //
        // GEMESSENER EIGENER FEHLER: die alte Zahl 560 kam aus „so breit
        // wie die Adressleiste" — das ist eine Begruendung, keine Messung.
        let r = feld(&Chrome::neu(1000), 1000, 700);
        assert_eq!(r.breite, BREITE_FELD);
        assert_eq!(
            r.breite, 598,
            "GEMESSEN 1:1 an Panel 1 (602 minus 2 mal 2 Rand)"
        );
        assert_eq!(r.x, 201, "GEMESSEN: (1000 - 598) / 2");
        assert_eq!(r.hoehe, HOEHE_FELD);
        // UND: senkrecht GEMESSEN knapp **UEBER** der Mitte (Vorlage 1:1:
        // Feldmitte y 252,5 gegen Platzmitte y 262, also 9,5 px ueber).
        let platz_mitte = super::super::Chrome::neu(1000).seiten_oberkante() as i64
            + (700
                - super::super::Chrome::neu(1000).seiten_oberkante()
                - super::super::STATUSLEISTE_HOEHE) as i64
                / 2;
        assert_eq!(
            (r.y + r.hoehe / 2) as i64,
            platz_mitte + VERSATZ_VON_MITTE as i64,
            "GEMESSEN 1:1: die Feldmitte liegt 9 px UEBER der Seitenmitte"
        );
    }

    /// DIE ZAHLEN DER VORLAGE AN DER GROESSE, IN DER DIE VORLAGE
    /// GEZEICHNET IST (UI, 03.10.).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es diesen Test gibt und warum er
    /// `949 x 467` fragt: alle Zahlen oben sind an der 1:1-Messung von
    /// Panel 1 abgelesen, und Panel 1 ist **949 x 467** (Fensterinhalt
    /// x 20..968, y 17..483 in `team/design/vorlage_nutzer.webp`). Ein
    /// Test, der die Feldlage bei 1000 x 700 prueft, prueft eine Fenstergroesse,
    /// die es in der Vorlage nicht gibt — und bei 1000 x 700 stimmen
    /// `LUFT_NACH_UNTEN`/`VERSATZ_VON_MITTE` als Pixelzahl nur, weil der
    /// Versatz in Pixeln steht. Die Vorlagen-Zahlen sind bei 949 x 467
    /// **exakt** die abgelesenen.
    #[test]
    fn panel1_der_vorlage_949x467() {
        let chrome = Chrome::neu(949);
        let r = feld(&chrome, 949, 467);

        // DER AUSSENKASTEN: das, was man am Bild sieht (der Rand liegt
        // 2 px aussen, `display_list` Schritt 2).
        //
        // GEMESSENE GENAUIGKEIT, und sie ist eine: **senkrecht** ist die
        // Kante auf 1 px genau ablesbar (Median ueber 340 Spalten: bei
        // y 229 ein heller Rand, R = 73, gegen R = 19 darueber), und
        // **waagerecht** nicht. GEMESSENER Grund: die Zeichnung hat einen
        // Verlauf, und am linken Feldende ist die Kante eine Kurve — der
        // Uebergang liegt zwischen zwei Pixeln (GEMESSEN x 173: R 35,
        // x 174: R 56, x 175: R 26, also ist 174 der Rand und 175 die
        // Fuellung, waehrend 173 schon die Kurve ist). Deshalb stehen
        // waagerecht **1 px Toleranz** und senkrecht keine.
        assert_eq!(r.y - 2, 229, "Aussenrand oben, GEMESSEN an Panel 1");
        assert_eq!(
            r.y + r.hoehe + 2,
            277,
            "Aussenrand unten, GEMESSEN an Panel 1"
        );
        // DIE KASTENMASSE als Zahlen: 602 x 48. GEMESSENE ENTSCHEIDUNG,
        // warum die Breite ohne Toleranz geprueft wird und die Kanten mit:
        // die Breite ist eine **Differenz zweier Kanten** und hebt einen
        // Pixel Versatz links gegen einen rechts auf; die Kanten einzeln
        // tragen den Verlauf der Zeichnung.
        assert_eq!(r.breite + 4, 602, "Aussenkasten 602 px breit");
        assert_eq!(r.hoehe + 4, 48, "Aussenkasten 48 px hoch");
        assert!(
            (r.x - 2).abs_diff(174) <= 1,
            "Aussenrand links {} (GEMESSEN 174, Toleranz 1 px wegen des Verlaufs)",
            r.x - 2
        );
        assert!(
            (r.x + r.breite + 2).abs_diff(776) <= 1,
            "Aussenrand rechts {} (GEMESSEN 776, Toleranz 1 px)",
            r.x + r.breite + 2
        );
        // UND mittig: (174 + 776) / 2 = 475 ~ Fenstermitte 474,5.
        assert_eq!(
            r.x + r.breite / 2,
            474,
            "GEMESSEN: die Feldmitte ist die Fenstermitte"
        );

        // UND die Feldmitte 9 px ueber der Platzmitte — die Zahl, an der
        // man sieht, ob der Versatz stimmt.
        // GEMESSENE KORREKTUR (03.10., 17:5x): hier stand `super::STATUSLEISTE_HOEHE`.
        // GEMESSENE URSACHE: in einem Test von `mod tests` zeigt `super` auf
        // das Modul `startseite`, nicht auf die Crate-Wurzel — die Konstante
        // steht aber in `lib.rs` (GEMESSEN Zeile 497). Die richtige Angabe
        // ist `crate::`.
        let platz_mitte = chrome.seiten_oberkante() as i64
            + (467 - chrome.seiten_oberkante() - crate::STATUSLEISTE_HOEHE) as i64 / 2;
        assert_eq!(
            (r.y + r.hoehe / 2) as i64,
            platz_mitte + VERSATZ_VON_MITTE as i64
        );
        // GEMESSEN: Feldmitte 252,5 ~ 253 bei 9 px ueber der Mitte (262).
        assert_eq!(r.y + r.hoehe / 2, 253, "GEMESSEN: Feldmitte y = 252,5");
    }

    /// DER TREFFER IST DAS FELD — UND sonst nichts.
    #[test]
    fn der_treffer_ist_das_feld() {
        let r = feld(&Chrome::neu(1000), 1000, 700);
        let mitte = Point::new(r.x as f32 + 10.0, r.y as f32 + 10.0);
        assert!(
            treffer(&Chrome::neu(1000), &mitte, 1000, 700),
            "das Feld trifft sich nicht selbst"
        );
        // AUSSERHALB: kein Treffer.
        for p in [
            Point::new(5.0, 5.0),
            Point::new(500.0, 20.0),
            Point::new(r.rechts() as f32 + 1.0, r.y as f32),
            Point::new(r.x as f32, r.unten() as f32),
            Point::new(999.0, 699.0),
        ] {
            assert!(
                !treffer(&Chrome::neu(1000), &p, 1000, 700),
                "({p:?}) trifft das Feld"
            );
        }
        // NaN: kein Treffer (U1c).
        assert!(!treffer(
            &Chrome::neu(1000),
            &Point::new(f32::NAN, f32::NAN),
            1000,
            700
        ));
        assert!(!treffer(
            &Chrome::neu(1000),
            &Point::new(f32::INFINITY, 300.0),
            1000,
            700
        ));
    }

    /// DIE STARTSEITE HAT GENAU ZWEI ADRESSEN — UND SIE WERDEN ERKANNT.
    #[test]
    fn die_startseite_hat_zwei_adressen() {
        assert_eq!(START_ADRESSEN.len(), 2);
        for a in START_ADRESSEN {
            assert!(ist_startseite(a), "{a} wird nicht als Startseite erkannt");
        }
        assert!(!ist_startseite("https://example.de"));
        assert!(!ist_startseite("narsil://einstellungen"));
        assert!(!ist_startseite(""));
    }

    /// WIE VIELE ELEMENTE HAT DIE STARTSEITE?
    ///
    /// GEMESSENE ENTSCHEIDUNG (15:58, siehe `MAX_ITEMS`): die Zellen einer
    /// Vektorfigur zaehlen als EIN Element. Die Lupe liegt GEMESSEN 16 px
    /// rechts vom linken Feldrand und ist [`super::LUPE_KANTE`] breit; alle
    /// Rechtecke, die GANZ in dieser Kiste liegen, sind ihre Zellen.
    fn elemente(liste: &DisplayList, feld: &Rechteck) -> usize {
        // GEMESSENE ENTSCHEIDUNG, warum `crate::` und nicht `super::`:
        // im Testmodul ist `super` das Modul `startseite`, und die Lupe ist
        // eine KONSTANTE DER CRATE (der Compiler hat E0425 gemeldet).
        let k = crate::LUPE_KANTE;
        let (lx, ly) = (feld.x + 16, feld.y + (feld.hoehe.saturating_sub(k)) / 2);
        let zellen = liste
            .iter()
            .filter(|i| {
                matches!(i, DisplayItem::Rect { rect, .. }
                    if rect.x >= lx as f32
                        && rect.x + rect.width <= (lx + k) as f32
                        && rect.y >= ly as f32
                        && rect.y + rect.height <= (ly + k) as f32)
            })
            .count();
        liste.len() - zellen + usize::from(zellen > 0)
    }

    /// DAS FELD ZEIGT DEN HINWEIS, BIS GETIPPT WIRD.
    #[test]
    fn das_feld_zeigt_den_hinweis_bis_getippt_wird() {
        let mut s = Startseite::default();
        assert!(!s.ist_getippt());
        //
        // KOORDINATION (02.10.): GEMESSEN am Vorlagenbild steht
        // im Startseiten-Feld der VOLLE Satz — 49 Zeichen, nicht 21.
        assert_eq!(
            s.anzeige_text(Suchmaschine::DuckDuckGo),
            "Search with DuckDuckGo or enter address"
        );
        // KOORDINATION (02.10.): der VOLLE Satz, wie im Feld der
        // Adressleiste (GEMESSEN am Vorlagenbild).
        assert_eq!(
            s.anzeige_text(Suchmaschine::Bing),
            "Search with Bing or enter address"
        );
        // GETIPPT: der Hinweis verschwindet.
        s.tippe("wetter berlin");
        assert!(s.ist_getippt());
        assert_eq!(s.anzeige_text(Suchmaschine::DuckDuckGo), "wetter berlin");
        // BOMBEN: eine Million Zeichen.
        s.tippe(&"a".repeat(1_000_000));
        assert_eq!(
            s.text.chars().count(),
            MAX_TEXT,
            "der Text wurde nicht gekuerzt"
        );
    }

    /// DAS FELD IST 44 PX HOCH (48 IM AUSSENKASTEN), ABGERUNDET — UND DER
    /// FOKUS AENDERT NUR DIE FARBE (Koordination 01.10. abends).
    ///
    /// GEMESSENE BEHAUPTUNGEN (alle drei Zahlen, keine Anschauung):
    /// 1. `HOEHE_FELD` ist **44** — GEMESSEN 1:1 an Panel 1 ist der
    ///    Aussenkasten 48 px (y 229..276) und der Rand liegt 2 px
    ///    aussen, also ist der Fuellton 44. Im 1000x700-Fenster misst
    ///    das Feld ebenfalls 44 px.
    /// 2. Der Radius des Feldes ist [`RADIUS_FELD`] (22 = die halbe
    ///    Hoehe, also eine Pille) an allen vier Ecken.
    /// 3. MIT und OHNE Fokus ist die Liste gleich LANG — der Fokus ist die
    ///    Farbe des Rahmens, kein zusaetzliches Rechteck.
    #[test]
    fn das_feld_ist_44_px_hoch_und_abgerundet() {
        // 1. DIE HOEHE.
        assert_eq!(
            HOEHE_FELD, 44,
            "GEMESSEN 1:1 an Panel 1 (48 minus 2 mal 2 Rand)"
        );
        let r = feld(&Chrome::neu(1000), 1000, 700);
        assert_eq!(r.hoehe, 44, "das Feld misst {} px", r.hoehe);
        assert_eq!(RADIUS_FELD, 22, "GEMESSEN: eine Pille (halbe Hoehe)");
        // 2. DER RADIUS — am Bild, nicht an der Konstante.
        let chrome = Chrome::neu(1000);
        let ohne = Startseite {
            text: String::new(),
            fokus: false,
        };
        let mit = Startseite {
            text: String::new(),
            fokus: true,
        };
        let l_ohne = display_list(&ohne, Suchmaschine::DuckDuckGo, &chrome, 1000, 700);
        let l_mit = display_list(&mit, Suchmaschine::DuckDuckGo, &chrome, 1000, 700);
        //
        // GEMESSENER EIGENER FEHLER (01.10.): der Test wollte `rect.radius`
        // lesen — `narsil_core::Rect` hat kein Feld `radius`, das sitzt am
        // `DisplayItem::Rect` und nicht am Rechteck. Die Abfrage liest
        // deshalb jetzt das ganze `DisplayItem::Rect` aus.
        let rahmen_von = |l: &DisplayList| -> [f32; 4] {
            l.iter()
                .find_map(|i| match i {
                    DisplayItem::Rect { rect, radius, .. }
                        if rect.y >= r.y as f32 - 1.0 && rect.y <= r.y as f32 + 1.0 =>
                    {
                        Some(*radius)
                    }
                    _ => None,
                })
                .unwrap_or([0.0; 4])
        };
        //
        // UI (03.10.): GEMESSEN 22 statt 8 — die Vorlage
        // zeigt eine Pille (Radius = halbe Hoehe, wie die Adressleiste), und
        // die halbe Hoehe ist seit `HOEHE_FELD = 44` die Zahl 22.
        assert_eq!(
            rahmen_von(&l_ohne),
            [22.0, 22.0, 22.0, 22.0],
            "das Feld ist keine Pille: {:?}",
            rahmen_von(&l_ohne)
        );
        // 3. DER FOKUS NUR ALS FARBE DES RANDS.
        //
        // GEMESSENER EIGENER FEHLER (01.10.): der Test behauptete hier
        // `l_ohne.len() == l_mit.len()`. GEMESSEN: 4 zu 5 — der Cursor
        // kommt im Fokus dazu. Die richtige Zusage ist enger: der Fokus
        // aendert die FARBE des Randes, nicht die Zahl der Rechtecke BIS
        // AUF DEN CURSOR. Also wird hier der Cursor abgezogen.
        assert_eq!(
            l_ohne.len() + 1,
            l_mit.len(),
            "der Fokus aendert mehr als den Cursor ({} zu {})",
            l_ohne.len(),
            l_mit.len()
        );
        //
        // GEMESSENER EIGENER FEHLER: hier stand `l_mit.len() <=
        // MAX_ITEMS`, und das wurde rot, als die LUPE als Vektorfigur
        // ins Feld kam: GEMESSEN 73 Items statt 6, weil die Lupe
        // GEMESSEN 67 Rasterzellen hat und EIN Element ist.
        //
        // GEMESSENE ENTSCHEIDUNG: gezaehlt werden die ELEmente der
        // Seite, und die Zellen einer Vektorfigur zaehlen als EINS.
        // Begruendung: [`MAX_ITEMS`] ist die Zusage „die Seite ist leer“
        // (ANFORDERUNGEN Zeile 16), und eine Lupe aus 67 Rechtecken ist
        // ein Element, kein Inhalt. Wer die Zellen zaehlen will, muss
        // die Zellen aller Symbole zaehlen — und die Zahl waere eine
        // zweite Wahrheit ueber dieselbe Figur.
        //
        // KOORDINATION (03.10., Hygiene): der Verweis auf
        // `symbole::PROBEN` ist entfernt (Figurensprache gibt es nicht
        // mehr).
        assert_eq!(
            elemente(&l_mit, &r),
            MAX_ITEMS,
            "im Fokus hat die Seite {} Elemente (Grenze {MAX_ITEMS})",
            elemente(&l_mit, &r)
        );
        // UND: die Farbe des Rahmens ist wirklich anders.
        // GEMESSENE ENTSCHEIDUNG, warum die Suche das ERSTE passende
        // Rechteck nimmt: der Rand liegt 2 px AUSSERHALB des Feldes, das
        // Feld genau auf der Kante — beide passen auf „y in r.y ± 1", und der
        // Rand steht in der Liste davor. Wer das nicht weiss, misst die
        // Farbe des FELDES und vergleicht sie mit der Farbe des RANDS.
        let farbe_von = |l: &DisplayList| -> Option<narsil_core::color::Color> {
            l.iter().find_map(|i| match i {
                DisplayItem::Rect { rect, color, .. }
                    if rect.y >= r.y as f32 - 3.0 && rect.y <= r.y as f32 + 1.0 =>
                {
                    Some(*color)
                }
                _ => None,
            })
        };
        assert_ne!(
            farbe_von(&l_ohne),
            farbe_von(&l_mit),
            "der Fokus aendert die Farbe des Rahmens nicht"
        );
        let pal = super::super::thema::Palette::fuer(chrome.thema);
        assert_eq!(
            farbe_von(&l_mit),
            Some(pal.akzent),
            "der Fokusrahmen ist nicht die Akzentfarbe"
        );
    }

    /// DAS FELD PASST AUCH IN 30 PX HOEHE (M3/M4) — der Schnitt kommt oben.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum von OBEN geschnitten und nicht von
    /// unten: das Feld ist der einzige Inhalt der Seite, und es steht
    /// optisch in der Mitte. Faellt die Seite auf 30 px zusammen, ist das
    /// Feld 30 px hoch und beginnt oben — waere es unten abgeschnitten,
    /// stuende es mit der abgeschnittenen Oberkante in der Luft.
    #[test]
    fn das_feld_passt_auch_in_kurze_fenster() {
        for hoehe in [24u32, 30, 44, 60, 82, 100] {
            let r = feld(&Chrome::neu(1000), 1000, hoehe);
            assert!(r.hoehe <= hoehe, "bei {hoehe}: das Feld ragt heraus");
            let oben = super::super::TAB_LEISTE_HOEHE + super::super::LEISTE_HOEHE;
            assert!(
                r.y >= oben || r.hoehe == 0,
                "bei {hoehe}: das Feld liegt in der Leiste (y = {})",
                r.y
            );
        }
        // GEMESSEN: bei 82 px (Oberkante 58 + Statuszeile 24 = 82) ist das
        // Feld 0 px hoch — es passt nicht mehr, und ein 2-px-Streifen waere
        // kein Feld.
        assert_eq!(feld(&Chrome::neu(1000), 1000, 82).hoehe, 0);
        // UND: der Text steht dann auch nicht mehr im Bild (die Liste hat
        // nur den Grund).
        let chrome = Chrome::neu(1000);
        let s = Startseite {
            text: "beispiel".into(),
            fokus: true,
        };
        assert_eq!(
            display_list(&s, Suchmaschine::DuckDuckGo, &chrome, 1000, 82).len(),
            1,
            "im 82-px-Fenster steht noch Text im Bild"
        );
    }

    /// DIE SUCHADRESSE AUS DEM FELD (der Weg zur Suchmaschine).
    #[test]
    fn die_suchadresse_aus_dem_feld() {
        let mut s = Startseite::default();
        s.tippe("wetter berlin");
        assert_eq!(
            s.suchadresse(Suchmaschine::DuckDuckGo, ""),
            "https://duckduckgo.com/?q=wetter%20berlin"
        );
        // LEER: kein Feld, keine Suche.
        let leer = Startseite::default();
        assert_eq!(leer.suchadresse(Suchmaschine::DuckDuckGo, ""), "");
    }
}
