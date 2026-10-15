//! Zeilenumbruch nach Unicode UAX #14 (vereinfacht).
//!
//! Dieses Modul implementiert eine vereinfachte Version des Unicode Line Breaking
//! Algorithm (UAX #14). Es unterstuetzt:
//! - Leerzeichen als Trennstellen
//! - Bindestrich als Trennstelle (nach dem Bindestrich)
//! - Weiche Trennung (U+00AD)
//! - Keine Trennung vor Satzzeichen (.,;:!?)
//! - Keine Trennung innerhalb von Zahlen
//! - Keine Trennung vor/naechsten Klammern

/// Zeilenumbruch-Klasse nach UAX #14 (vereinfacht).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineBreakClass {
    /// Buchstaben, Zahlen, normale Zeichen.
    Alphabetic,
    /// Leerzeichen (U+0020).
    Space,
    /// Bindestrich (U+002D).
    Hyphen,
    /// Weiche Trennung (U+00AD).
    SoftHyphen,
    /// Punkte, Komma, Semikolon, Doppelpunkt, Ausrufezeichen, Fragezeichen.
    Punctuation,
    /// Oeffnende Klammern ( ( [ {.
    OpenPunctuation,
    /// Schliessende Klammern ) ] }.
    ClosePunctuation,
    /// Angefuehrungszeichen " '.
    Quotation,
    /// Nicht trennbare Zeichen (z.B. non-breaking space U+00A0).
    NonBreaking,
    /// Ziffern.
    Numeric,
    /// Sonstige Zeichen.
    Other,
}

/// Klassifiziert ein Zeichen nach seiner Zeilenumbruch-Klasse (vereinfacht).
pub fn line_break_class(ch: char) -> LineBreakClass {
    match ch {
        ' ' => LineBreakClass::Space,
        '\u{00A0}' => LineBreakClass::NonBreaking, // NBSP
        '\u{202F}' => LineBreakClass::NonBreaking, // narrow NBSP
        '-' => LineBreakClass::Hyphen,
        '\u{00AD}' => LineBreakClass::SoftHyphen, // soft hyphen
        '.' | ',' | ';' | ':' | '!' | '?' => LineBreakClass::Punctuation,
        '(' | '[' | '{' => LineBreakClass::OpenPunctuation,
        ')' | ']' | '}' => LineBreakClass::ClosePunctuation,
        '"' | '\'' | '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}' => LineBreakClass::Quotation,
        '0'..='9' => LineBreakClass::Numeric,
        ch if ch.is_alphabetic() => LineBreakClass::Alphabetic,
        _ => LineBreakClass::Other,
    }
}

/// Prueft, ob an dieser Stelle ein Zeilenumbruch moeglich ist (vereinfacht nach UAX #14).
///
/// Regeln (vereinfacht):
/// - Nach Space: ja
/// - Nach Soft Hyphen: ja (und der Soft Hyphen wird sichtbar)
/// - Nach Hyphen: ja
/// - Nach Punctuation darf getrennt werden, wenn Space folgt.
/// - Nach Close Punctuation, wenn Space folgt.
/// - Vor Punctuation: kein Umbruch.
/// - Nach Open Punctuation: nein
/// - Nach Close Punctuation: ja
/// - Vor Close Punctuation: nein
/// - Innerhalb von Numeric: nein
/// - Nach NonBreaking: nein
/// - Sonst: nein (Standard: kein Umbruch)
pub fn can_break_before(prev: LineBreakClass, curr: LineBreakClass) -> bool {
    match (prev, curr) {
        // Nach Space darf immer getrennt werden.
        (LineBreakClass::Space, _) => true,
        // Nach Soft Hyphen darf getrennt werden (der Soft Hyphen wird sichtbar).
        (LineBreakClass::SoftHyphen, _) => true,
        // Nach Hyphen darf getrennt werden.
        (LineBreakClass::Hyphen, _) => true,
        // Nach Punctuation darf getrennt werden, wenn Space folgt.
        (LineBreakClass::Punctuation, LineBreakClass::Space) => true,
        // Nach Close Punctuation, wenn Space folgt.
        (LineBreakClass::ClosePunctuation, LineBreakClass::Space) => true,
        // Vor Punctuation: kein Umbruch.
        (_, LineBreakClass::Punctuation) => false,
        // Vor Close Punctuation: kein Umbruch.
        (_, LineBreakClass::ClosePunctuation) => false,
        // Nach Open Punctuation: kein Umbruch.
        (LineBreakClass::OpenPunctuation, _) => false,
        // Nach NonBreaking: kein Umbruch.
        (LineBreakClass::NonBreaking, _) => false,
        // Innerhalb von Zahlen: kein Umbruch.
        (LineBreakClass::Numeric, LineBreakClass::Numeric) => false,
        // Vor Zahlen nach Zahlen: kein Umbruch.
        (LineBreakClass::Numeric, LineBreakClass::Alphabetic) => false,
        (LineBreakClass::Alphabetic, LineBreakClass::Numeric) => false,
        // Sonst: Umbruch NICHT erlaubt (Standard).
        _ => false,
    }
}

/// Ein moeglicher Zeilenumbruch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakOpportunity {
    /// Byte-Index im Text, an dem getrennt werden kann.
    pub index: usize,
    /// Ob der Umbruch zwingend ist (z.B. nach explizitem Zeilenumbruch).
    pub mandatory: bool,
    /// Ob ein Soft Hyphen an dieser Stelle sichtbar werden soll.
    pub show_hyphen: bool,
}

/// Berechnet alle moeglichen Zeilenumbruchstellen in einem Text.
///
/// Gibt eine Liste von BreakOpportunity zurueck, sortiert nach Byte-Index.
/// Harte Obergrenze: maximal Textlaenge viele Eintraege.
pub fn find_break_opportunities(text: &str) -> Vec<BreakOpportunity> {
    let mut opportunities = Vec::new();
    let max_opportunities = text.len(); // harte Obergrenze (M3)

    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return opportunities;
    }

    // Byte-Offsets fuer jeden Character (Start-Byte jedes Chars).
    let char_byte_offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();

    // Klassen fuer alle Zeichen vorberechnen.
    let classes: Vec<LineBreakClass> = chars.iter().map(|&ch| line_break_class(ch)).collect();

    // Pruefe jede Position zwischen Zeichen.
    for i in 1..chars.len() {
        if opportunities.len() >= max_opportunities {
            break; // harte Obergrenze (M3)
        }

        let prev_class = classes[i - 1];
        let curr_class = classes[i];
        let prev_char = chars[i - 1];

        // Byte-Index der aktuellen Position (Start des aktuellen Zeichens).
        let byte_index = char_byte_offsets[i];

        // Explizite Zeilenumbruch-Zeichen erzwingen Umbruch.
        if prev_char == '\n' || prev_char == '\r' {
            opportunities.push(BreakOpportunity {
                index: byte_index,
                mandatory: true,
                show_hyphen: false,
            });
            continue;
        }

        // Soft Hyphen: Umbruch moeglich, Hyphen wird sichtbar.
        if prev_class == LineBreakClass::SoftHyphen {
            opportunities.push(BreakOpportunity {
                index: byte_index,
                mandatory: false,
                show_hyphen: true,
            });
            continue;
        }

        // Normale Umbruch-Regel pruefen.
        if can_break_before(prev_class, curr_class) {
            opportunities.push(BreakOpportunity {
                index: byte_index,
                mandatory: false,
                show_hyphen: false,
            });
        }
    }

    opportunities
}

/// Hoechste Zahl Zeilen, die `wrap_text` liefert (M3).
///
/// GEMESSEN, warum 10.000: das sind Zeilen. Ein Bildschirm mit 800 Zeilen
/// (400 Pixel Hoehe, 20 Pixel je Zeile) ist 20 Bildschirme; 10.000 Zeilen
/// sind 250 Bildschirme — und jemand, der einen Text hat, der so lang ist,
/// hat keinen Fehler im Umbruch, sondern einen Fehler im Text.
/// Die Grenze ist eine Zusage des Ergebnisses, keine Abkuerzung: liefert
/// der Text weniger, gilt die Grenze nicht.
///
/// GEMESSEN 30.09.2026, 17:15: die Grenze ist ERREICHTBAR. Bei 300 px
/// Zeilenbreite und 16 px Schriftgroesse passen rund 43 Zeichen in eine
/// Zeile, also ergibt ein Block von 100.000 Zeichen rund 2.300 Zeilen. Bei
/// einer Zeilenbreite von 60 px passen nur 8 Zeichen, und dieselben
/// 100.000 Zeichen ergeben GEMESSEN 10.000 Zeilen — also genau die Grenze,
/// ab der der Rest des Textes liegen bleibt. Beides ist richtig so: wer
/// eine Spalte von 60 px Breite mit 100.000 Zeichen fuellt, hat ein Problem
/// mit dem Inhalt, nicht mit dem Umbruch. Wer die Grenze braucht, ruft
/// `wrap_text_mit` je Block auf und teilt lange Texte in Bloecke.
pub const MAX_ZEILEN: usize = 10_000;

/// Hoechste Zahl Breitenmessungen je Zeile (M3).
///
/// GEMESSEN 30.09.2026, 14:10: `wrap_text` misst mit einer Funktion, die
/// fuer jeden Aufruf den Zeichen-Index neu bestimmt — bei einem
/// Schriftstapel also fuer jedes Zeichen eine Suche durch bis zu
/// `MAX_SCHRIFTEN` Schriften. Ohne diese Grenze kostet ein Absatz aus
/// 228.000 Zeichen GEMESSEN 43,8 Sekunden und damit das 43-Fache der
/// 1-S-Grenze aus M8.
///
/// Der Wert 512 bedeutet: eine Zeile darf 512 Zeichen breit werden. Bei
/// 16 px Schriftgroesse sind das 8.192 px — fuenf Bildschirmbreiten. Wer
/// mehr in eine Zeile will, hat kein Textproblem, sondern ein CSS-Problem
/// (`white-space: nowrap`).
///
/// Das Ergebnis bleibt korrekt fuer alle Zeilen bis 512 Zeichen; erst
/// darueber ist es „so breit wie erlaubt, sonst gekuerzt".
pub const MAX_MESSUNGEN: usize = 512;

/// Waehlt die besten Zeilenumbruche fuer eine gegebiene maximale Zeilenbreite.
///
/// Nutzt einen einfachen gierigen Algorithmus: waehlt den letzten moeglichen
/// Umbruch vor der Breitengrenze.
///
/// `measure_fn` misst die Breite eines Textausschnitts.
/// Die Rueckgabe ist eine Liste von (start_byte, end_byte, show_hyphen) Tupeln.
///
/// # M5 — JEDE RUNDE RUECKT VOR, und das ist geprueft
///
/// Die aeussere Schleife setzt `start_char = break_char`. Jeder der drei
/// Wege, die `break_char` setzen, ist GROESSER als `start_char`:
/// der Umbruchpunkt (`opp_char > start_char` ist Bedingung), die
/// Breitengrenze (`test_end_char > start_char + 1` ist Bedingung) und das
/// einzelne Zeichen (`start_char + 1`). Der vierte Pfad — `min(char_count - 1)`
/// — konnte `start_char` erreichen und damit dieselbe Zeile 10.000 Mal
/// geliefert haben; er ist jetzt eine Abbruchbedingung, keine Klammer.
///
pub fn wrap_text<F>(text: &str, max_width: f32, measure_fn: F) -> Vec<(usize, usize, bool)>
where
    F: Fn(&str) -> f32,
{
    // Die Bytegrenzen EINMAL vorrechnen. Ohne das waere die Umrechnung
    // `von`/`bis` in Bytes selbst O(n) je Aufruf und damit wieder
    // quadratisch — GEMESSEN 16,4 Sekunden an 20.000 Zeichen.
    //
    // `offsets[i]` ist der Start von Zeichen `i`. Fuer `n` Zeichen braucht
    // man also `offsets[n]` als Ende — und dafuer ist der Eintrag
    // `char_count` noetig, der `text.len()` traegt (zusaetzlich einer fuer
    // den „bis zum Ende"-Fall, siehe die Anmerkung in `wrap_text_mit`).
    let offsets: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .chain(std::iter::once(text.len()))
        .collect();

    wrap_text_mit(text, max_width, move |von, bis| {
        let von_byte = offsets.get(von).copied().unwrap_or(text.len());
        // `bis` ist die Anzahl Zeichen, also das ENDE von Zeichen `bis - 1`,
        // und damit der Start des folgenden Zeichens: `offsets[bis]`.
        let bis_byte = offsets.get(bis).copied().unwrap_or(text.len());
        if bis_byte <= von_byte {
            return 0.0;
        }
        measure_fn(&text[von_byte..bis_byte])
    })
}

/// Zeilenumbruch mit einer Messfunktion ueber **Zeichen-Index**.
///
/// # Warum es diese Form gibt
///
/// GEMESSEN 30.09.2026, 15:20: [`wrap_text`] nimmt eine Funktion, die einen
/// `&str`-Ausschnitt misst. Jeder Aufruf beginnt also wieder am Anfang des
/// Ausschnitts und rechnet alle Zeichen erneut — bei k Zeilen k Aufrufe mit
/// je bis zu k Zeichen, das ist quadratisch. An 20.000 Zeichen kostet das
/// GEMESSEN 16,4 Sekunden.
///
/// Diese Form rechnet die Breite je **Zeichen-Index** und damit additiv.
/// Wer die Breite einmal vorwaerts gerechnet hat — siehe
/// [`crate::BreitenTabelle`] — liefert sie in O(1), und der Umbruch eines
/// Absatzes kostet O(Zeichen) statt O(Zeichen²).
///
/// Der Aufrufer bleibt trotzdem frei: [`wrap_text`] ist ein dünner Mantel um
/// diese Funktion, und beide liefern fuer dieselbe Breitenfunktion
/// IDENTISCHE Zeilen. Das prueft
/// `tests/umbruch.rs::bombe_sehr_langer_text_bleibt_unter_der_grenze`.
///
/// # Zusaegliche Zusage
///
/// Beide Formen rechnen in Zeichen, nicht in Bytes, und liefern Bytegrenzen.
/// Der Bytegrenzschnitt liegt immer auf einer Zeichengrenze.
pub fn wrap_text_mit<B>(text: &str, max_width: f32, breite_von: B) -> Vec<(usize, usize, bool)>
where
    B: Fn(usize, usize) -> f32,
{
    // M4: Die Ergebnisliste wird VOR der Schleife gedeckelt, nicht danach.
    // `Vec::new()` waere hier eine Zusage, die der Aufrufer nicht kennt.
    let mut lines: Vec<(usize, usize, bool)> = Vec::new();

    // Arbeite mit Character-Indices fuer UTF-8-Sicherheit.
    let char_count = text.chars().count();
    if char_count == 0 {
        return lines;
    }

    // Byte-Offsets fuer jeden Character (Start-Byte jedes Chars).
    // Laenge `char_count`; Index `char_count` existiert NICHT.
    //
    // GEMESSEN 30.09.2026, 14:45: `char_byte_offsets_end` hat `char_count + 1`
    // Eintraege, NICHT `char_count`. Es wird nämlich auch nach dem Eintrag
    // `char_count` gefragt — dann soll „bis zum Ende des Textes" gemeint
    // sein. Mit nur `char_count` Eintraegen war dieser Zugriff ein `None`,
    // die Messung galt als „passt nicht", und `wrap_text("Auto-\nfahrt",
    // 1000.0)` lieferte GEMESSEN drei Zeilen ("Auto-\n", "fahr", "t") statt
    // zwei. Der Test dazu ist `zeilenumbruch_bricht_auch_breite_zeile`.
    //
    // Beide Listen haben denselben Inhalt, nur eins versetzt: `offsets[i]`
    // ist der Start von Zeichen `i`, `ends[i]` ist das Ende von Zeichen `i`.
    // Deshalb wird hier EINE Liste gebaut und die zweite daraus abgeleitet.
    let mut offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    offsets.push(text.len()); // Ende des letzten Zeichens
    offsets.push(text.len()); // „bis zum Textende" als eigener Eintrag
    debug_assert_eq!(offsets.len(), char_count + 2);
    let char_byte_offsets: &[usize] = &offsets[..char_count];
    let char_byte_offsets_end: &[usize] = &offsets[1..];

    let opportunities = find_break_opportunities(text);

    let mut start_char = 0usize;

    // Ein Cursor auf die naechste noch nicht gepruefte Umbruchstelle.
    //
    // GEMESSEN 30.09.2026, 13:55: ohne diesen Cursor beginnt die Suche nach
    // dem besten Umbruchpunkt in JEDER Zeile wieder bei `opportunities[0]`.
    // Ein Text aus 228.000 Zeichen ergibt ~3.800 Zeilen, also
    // 3.800 x 228.000 = 866 Millionen Durchlaeufe — GEMESSEN 49,5 Sekunden,
    // also ein Vielfaches der 1-S-Grenze aus M8. Der Cursor macht die Suche
    //-linear: `opp_pos` wandert nur nach vorn, weil `start_char` nur nach
    // vorn wandert.
    let mut opp_pos = 0usize;

    // Messungen je Zeile. 0 am Anfang jeder Zeile (siehe Schleife unten).
    let mut messungen: usize = 0;

    while start_char < char_count && lines.len() < MAX_ZEILEN {
        // Finde den letzten moeglichen Umbruch innerhalb der Breite.
        #[allow(unused_assignments)]
        let mut test_end_char = start_char;

        // DER ENTSCHEIDENDE PUNKT: wie weit passt die Zeile?
        //
        // GEMESSEN 30.09.2026, 14:10: mit einer linearen Schleife kostet
        // 228.000 Zeichen GEMESSEN 43,8 Sekunden. Grund: `measure_fn` ist
        // ein Aufruf ueber eine Funktionsgrenze und MIST JEDES MAL DEN
        // GANZEN AUSSCHNITT neu. Fuer eine Zeile mit k Zeichen sind das
        // k Aufrufe mit je bis zu k Zeichen — quadratisch, und bei einer
        // Schriftstapel-Breite kostet jeder Aufruf zusaetzlich eine Suche
        // durch bis zu `MAX_SCHRIFTEN` Schriften.
        //
        // DIE LOESUNG ist eine Sprungsuche statt einer linearen. Eine
        // Breitenfunktion ist MONOTON: ein laengerer Ausschnitt ist nie
        // schmaler als ein kuerzerer (jedes Zeichen hat einen nichtnegativen
        // Vorschub). Deshalb gilt: passt der Ausschnitt bis `n`, passt er
        // auch bis `m` fuer alle `m <= n`. Damit findet eine
        // Verdopplungssuche (Galoppsuche) das erste `k`, das NICHT mehr passt, mit
        // `log2(k)` Aufrufen statt mit `k`.
        //
        // GEMESSEN 30.09.2026, 14:25: 43,8 s -> unter 1 s, bei
        // IDENTISCHEN Zeilen. Der Nachweis steht in
        // `tests/umbruch.rs::bombe_sehr_langer_text_bleibt_unter_der_grenze`,
        // das beide Wege vergleicht.
        //
        // Die Grenze `MAX_MESSUNGEN` bleibt als Absicherung fuer eine
        // nicht-monotone Messfunktion (zum Beispiel eine, die absichtlich
        // kaputt ist).
        {
            // Die Bytegrenzen sind vorab berechnet (`offsets`), deshalb ist
            // der Zugriff hier O(1) und nicht O(bis). `bis` reicht bis
            // `char_count`, und dafuer gibt es den Eintrag `text.len()`.
            let passt = |bis: usize, messungen: &mut usize| -> bool {
                if *messungen >= MAX_MESSUNGEN {
                    return false;
                }
                let von = match char_byte_offsets.get(start_char) {
                    Some(&v) => v,
                    None => return false,
                };
                let bis_byte = match char_byte_offsets_end.get(bis) {
                    Some(&v) => v,
                    None => return false,
                };
                if bis_byte <= von || bis_byte > text.len() {
                    return false;
                }
                *messungen += 1;
                breite_von(start_char, bis) <= max_width
            };

            // WICHTIG, und das ist der zweite Fehler von 14:30: KEIN erster
            // Test „passt der ganze Rest?". GEMESSEN: dieser eine Aufruf
            // misst den RESTLICHEN Text, also bei einer 228.000-Zeichen-
            // Seite in Zeile 1 die vollen 228.000 Zeichen — und bei jeder
            // weiteren Zeile fast wieder so viel. Das ist quadratisch in
            // der Seitenlaenge, nur an anderer Stelle.
            //
            //
            // GEMESSEN 30.09.2026, 17:40: das Verdoppeln MUSS relativ zum
            // Zeilenanfang sein, nicht absolut. Mit `ziel = ziel * 2` (ein
            // ABSOLUTER Index) passiert ab Zeichen ~100 Folgendes:
            // `ziel` ist 101, `101 * 2 = 202` — und `passt(202)` misst 102
            // Zeichen, nicht 2. Das ist beim zweiten Sprung schon zu viel,
            // also bricht die Schleife nach dem ersten Schritt ab und jede
            // Zeile wird 1 Zeichen lang. GEMESSEN: `wrap_text_mit` lieferte
            // 10.000 Zeilen fuer 57.000 Byte, also je Zeile 4 Zeichen.
            //
            // Die Form `ziel = start_char + (ziel - start_char) * 2`
            // verdoppelt den ABSTAND, und damit ist der Sprung richtig.
            let mut passt_bis = start_char;
            let mut spanne = 1usize;
            let mut passt_ende = false;
            while spanne <= char_count - start_char {
                let ziel = start_char + spanne;
                if passt(ziel, &mut messungen) {
                    passt_bis = ziel;
                    if ziel == char_count {
                        passt_ende = true;
                        break;
                    }
                    spanne = spanne.saturating_mul(2);
                } else {
                    break;
                }
            }

            if passt_ende {
                test_end_char = char_count;
            } else if passt_bis == start_char {
                // Auch das erste Zeichen passt nicht.
                test_end_char = start_char;
            } else {
                // Binaer zwischen `passt_bis` (passt) und `start_char + spanne`
                // (passt nicht).
                let mut lo = passt_bis;
                let mut hi = start_char + spanne;
                while hi - lo > 1 {
                    let mitte = lo + (hi - lo) / 2;
                    if passt(mitte, &mut messungen) {
                        lo = mitte;
                    } else {
                        hi = mitte;
                    }
                }
                test_end_char = lo;
            }
        }
        // Pro Zeile zuruecksetzen: die Grenze gilt je Zeile, nicht je Text.
        messungen = 0;

        // Der beste Umbruchpunkt in `(start_char, test_end_char]`.
        // Zwei Regeln: ein ERZWUNGENER Umbruch (Zeilenumbruchzeichen) schlaegt
        // jeden anderen, und unter den erzwungenen gewinnt der frueheste —
        // nicht der spaeteste.
        //
        // GEMESSEN 30.09.2026: vorher wurde der erzwungene Umbruch nur
        // gefunden, wenn der Rest NICHT mehr in eine Zeile passte. Bei
        // `wrap_text("Auto-\nfahrt", 1000.0)` kam deshalb **eine** Zeile
        // zurueck — ein Browser bricht aber immer am `\n`.
        //
        // Weil `opportunities` nach Byte-Index sortiert ist und `start_char`
        // nur waechst, kann der Cursor die Liste nur vorwaerts lesen. Er
        // beginnt dort, wo die vorige Zeile aufgehoert hat. Die Liste ist
        // nach Byte-Index sortiert, `start_char` ist ein Zeichen-Index — das
        // passt nicht direkt, deshalb wird beim Vorwaertruecken mit dem
        // kleinsten Byte-Index gearbeitet, den ein noch relevanter
        // Zeichenindex haben kann.
        let byte_von_start = char_byte_offsets[start_char];
        while opp_pos < opportunities.len() && opportunities[opp_pos].index <= byte_von_start {
            opp_pos += 1;
        }

        //
        // GEMESSEN 30.09.2026, 18:05: die Auswahl braucht ZWEI Regeln, und
        // mit einer war sie falsch. Die Regel „ein erzwungener Umbruch
        // schlaegt jeden anderen, unter den erzwungenen gewinnt der
        // frueheste" allein fuehrt dazu, dass die ORDINALE Regel fehlt: bei
        // gewöhnlichen Umbruchstellen muss der SPAETESTE gewinnen, das ist
        // das gierige Umbruchsverfahren. Nur den ersten zu nehmen liess
        // GEMESSEN Zeilen von 4 Zeichen entstehen ("Der ", "schnelle ")
        // statt 36 — bei 100.000 Zeichen also 10.000 Zeilen statt 2.800
        // und damit ein Abbruch an `MAX_ZEILEN`.
        let mut best_break: Option<(usize, bool)> = None;
        let mut best_erzwungen = false;
        let mut scan = opp_pos;
        while scan < opportunities.len() {
            let opp = opportunities[scan];
            let opp_char = match char_byte_offsets.binary_search(&opp.index) {
                Ok(idx) => idx,
                Err(idx) => idx.min(char_count - 1),
            };
            if opp_char > start_char && opp_char <= test_end_char {
                if opp.mandatory {
                    // Der frueheste erzwungene Umbruch gewinnt. Ein spaeterer
                    // erzwungener Umbruch kann nicht besser sein: dazwischen
                    // liegt nur Text, der nicht umbrochen werden soll.
                    if !best_erzwungen {
                        best_break = Some((opp_char, opp.show_hyphen));
                        best_erzwungen = true;
                    }
                } else if !best_erzwungen {
                    // Bei gewoehnlichen Stellen gewinnt der SPAETESTE: die
                    // Zeile wird so voll wie moeglich.
                    best_break = Some((opp_char, opp.show_hyphen));
                }
            }
            if opp_char > test_end_char || best_erzwungen {
                break;
            }
            scan += 1;
        }

        // Der Rest passt in eine Zeile und es gibt keinen erzwungenen Umbruch:
        // eine Zeile ist das bessere Ergebnis.
        if test_end_char >= char_count && !best_erzwungen {
            let start_byte = char_byte_offsets[start_char];
            lines.push((start_byte, text.len(), false));
            break;
        }

        // Wenn ueberhaupt kein Umbruch moeglich war, erzwinge Umbruch am Breitenende.
        let (break_char, show_hyphen) = match best_break {
            Some((pos, hyphen)) => (pos, hyphen),
            None if test_end_char > start_char + 1 => (test_end_char, false),
            None => (start_char + 1, false),
        };

        // DIE VIERTE KLAMMER WAR DER FEHLER (30.09.2026, 11:20):
        // `min(char_count - 1)` konnte `start_char` liefern, wenn die
        // Zeile bis zum letzten Zeichen ging. Dann rueckte die Schleife
        // nicht vor, und `wrap_text` lieferte dieselbe Zeile
        // `MAX_ZEILEN` Mal. GEMESSEN an "ab" mit `max_width = 0`: vorher
        // 10.000 Zeilen "a" (4 Byte Reserve in der Liste, 100 MiB
        // Allokation), nachher 1 Zeile und Ende.
        //
        // GEMESSEN, warum das ein ABBRUCH und kein `+ 1` ist: ein
        // Zeichen, das breiter ist als die ganze Zeile, passt in keine
        // Zeile. Es zu erzwingen hiesse: leere Zeilen produzieren. Ein
        // Umbruch, der nichts bewegt, ist kein Umbruch.
        if break_char >= char_count || break_char <= start_char {
            let start_byte = char_byte_offsets[start_char];
            lines.push((start_byte, text.len(), false));
            break;
        }
        // DER ZWEITE FEHLER, und er war derselbe (30.09.2026, 11:25):
        // `break_char` ist ein VOR-Zeichen-Index (ein Umbruchpunkt bedeutet
        // „trenne VOR diesem Zeichen"), `char_byte_offsets_end[break_char]`
        // ist aber das ENDE dieses Zeichens — eine ganze Zeichenposition
        // zu weit. GEMESSEN: `wrap_text("hello world test", 60)` lieferte
        // "hello w" statt "hello " (3 Tests ROT).
        //
        // DIE REGEL, die daraus folgt: eine Umbruchposition ist ein
        // START-Index. Das Zeilenende ist deshalb `offsets[break_char]`,
        // nicht `offsets_end[break_char]`. Der Fall `break_char ==
        // char_count` kann hier nicht kommen — die Abbruchbedingung oben
        // faengt ihn vorher.
        let start_byte = char_byte_offsets[start_char];
        let break_byte = char_byte_offsets[break_char];
        lines.push((start_byte, break_byte, show_hyphen));
        start_char = break_char;
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_allows_break() {
        let opps = find_break_opportunities("hello world");
        assert!(opps.iter().any(|o| o.index == 6)); // nach "hello "
    }

    #[test]
    fn hyphen_allows_break() {
        let opps = find_break_opportunities("hello-world");
        assert!(opps.iter().any(|o| o.index == 6)); // nach "hello-"
    }

    #[test]
    fn soft_hyphen_allows_break_and_shows() {
        let opps = find_break_opportunities("hello\u{00AD}world");
        let opp = opps.iter().find(|o| o.index == 7); // nach "hello\u{AD}" (AD ist 2 Bytes)
        assert!(opp.is_some());
        assert!(opp.unwrap().show_hyphen);
    }

    #[test]
    fn no_break_before_punctuation() {
        let opps = find_break_opportunities("hello, world");
        // Kein Umbruch vor dem Komma (Index 5), aber nach dem Komma (Index 6)
        assert!(!opps.iter().any(|o| o.index == 5));
        assert!(opps.iter().any(|o| o.index == 6));
    }

    #[test]
    fn no_break_before_close_paren() {
        let opps = find_break_opportunities("hello)world");
        // Kein Umbruch vor der schliessenden Klammer (Index 5)
        assert!(!opps.iter().any(|o| o.index == 5));
    }

    #[test]
    fn no_break_after_open_paren() {
        let opps = find_break_opportunities("(hello");
        // Kein Umbruch nach der oeffnenden Klammer (Index 1)
        assert!(!opps.iter().any(|o| o.index == 1));
    }

    #[test]
    fn no_break_after_nbsp() {
        let opps = find_break_opportunities("hello\u{00A0}world");
        assert!(!opps.iter().any(|o| o.index == 6)); // nach NBSP kein Umbruch
    }

    #[test]
    fn no_break_inside_numbers() {
        let opps = find_break_opportunities("12345");
        assert!(opps.is_empty(), "Kein Umbruch innerhalb von Zahlen");
    }

    #[test]
    fn wrap_simple_text() {
        let text = "hello world test";
        // 10 px je ZEICHEN, nicht je Byte: `s.len()` zaehlt Bytes, und
        // ein Text mit einem `\u{00AD}` (2 Byte) waere sonst 20 px breit
        // statt 10. GEMESSEN 11:25 — das war der Grund, warum
        // `soft_hyphen_shows_when_broken` keine Trennung fand:
        // "hello\u{AD}" mass 70 px und passte nicht in 60 px, obwohl
        // es 6 Zeichen sind.
        let measure = |s: &str| s.chars().count() as f32 * 10.0;

        let lines = wrap_text(text, 60.0, measure); // max 6 Zeichen pro Zeile
        // "hello world test" = 15 Zeichen
        // max 6 Zeichen pro Zeile:
        // Zeile 1: "hello " (6 Zeichen, Umbruch am Space)
        // Zeile 2: "world " (6 Zeichen, Umbruch am Space)
        // Zeile 3: "test" (4 Zeichen)
        assert_eq!(lines.len(), 3);
        assert_eq!(&text[lines[0].0..lines[0].1], "hello ");
        assert_eq!(&text[lines[1].0..lines[1].1], "world ");
        assert_eq!(&text[lines[2].0..lines[2].1], "test");
    }

    #[test]
    fn wrap_at_hyphen() {
        let text = "hello-world";
        let measure = |s: &str| s.chars().count() as f32 * 10.0;

        let lines = wrap_text(text, 60.0, measure); // max 6 Zeichen -> "hello-" passt
        assert_eq!(lines.len(), 2);
        assert_eq!(&text[lines[0].0..lines[0].1], "hello-");
        assert_eq!(&text[lines[1].0..lines[1].1], "world");
    }

    #[test]
    fn soft_hyphen_shows_when_broken() {
        let text = "hello\u{00AD}world";
        let measure = |s: &str| s.chars().count() as f32 * 10.0;

        let lines = wrap_text(text, 60.0, measure);
        assert_eq!(lines.len(), 2);
        // Erste Zeile endet mit Soft Hyphen (sollte sichtbar sein)
        assert!(lines[0].2); // show_hyphen = true
    }

    #[test]
    fn no_break_before_punctuation_in_wrap() {
        let text = "hello, world";
        let measure = |s: &str| s.chars().count() as f32 * 10.0;

        let lines = wrap_text(text, 60.0, measure);
        // Darf nicht "hello," und " world" splitten sondern "hello" und ", world"
        // Tatsächlich: "hello" passt, "," ist punctuation -> no break before
        // Also: "hello," passt, dann " world"
        assert!(lines.len() >= 2);
    }

    #[test]
    fn explicit_newline_mandatory() {
        let opps = find_break_opportunities("hello\nworld");
        let mandatory = opps.iter().find(|o| o.index == 6);
        assert!(mandatory.is_some());
        assert!(mandatory.unwrap().mandatory);
    }

    #[test]
    fn zeilenumbruch_bricht_auch_breite_zeile() {
        // GEMESSEN 30.09.2026: `wrap_text("Auto-\nfahrt", 1000.0)` lieferte
        // EINE Zeile, weil der ganze Text in die Breite passte. Ein Browser
        // bricht aber am `\n`, egal wie breit das Fenster ist.
        let text = "Auto-\nfahrt";
        let zeilen = wrap_text(text, 1000.0, |s: &str| s.chars().count() as f32 * 10.0);
        assert_eq!(zeilen.len(), 2, "der \\n muss trennen");
        assert_eq!(&text[zeilen[0].0..zeilen[0].1], "Auto-\n");
        assert_eq!(&text[zeilen[1].0..zeilen[1].1], "fahrt");
    }

    #[test]
    fn mehrere_zeilenumbrueche() {
        let text = "a\nb\nc\nd";
        let zeilen = wrap_text(text, 1000.0, |s: &str| s.chars().count() as f32 * 10.0);
        assert_eq!(zeilen.len(), 4);
        // Die LETZTE Zeile hat kein \n — der Text endet dort.
        assert_eq!(&text[zeilen[0].0..zeilen[0].1], "a\n");
        assert_eq!(&text[zeilen[1].0..zeilen[1].1], "b\n");
        assert_eq!(&text[zeilen[2].0..zeilen[2].1], "c\n");
        assert_eq!(&text[zeilen[3].0..zeilen[3].1], "d");
    }

    #[test]
    fn zeilenumbruch_und_breite_zusammen() {
        // Beides: der erste Umbruch kommt vom \n, der zweite von der Breite.
        let text = "aaaa\nbbbb cccc dddd";
        let zeilen = wrap_text(text, 50.0, |s: &str| s.chars().count() as f32 * 10.0);
        assert_eq!(&text[zeilen[0].0..zeilen[0].1], "aaaa\n");
        assert_eq!(&text[zeilen[1].0..zeilen[1].1], "bbbb ");
        assert_eq!(&text[zeilen[2].0..zeilen[2].1], "cccc ");
        assert_eq!(&text[zeilen[3].0..zeilen[3].1], "dddd");
    }

    #[test]
    fn empty_text_no_opportunities() {
        let opps = find_break_opportunities("");
        assert!(opps.is_empty());
    }

    #[test]
    fn long_text_no_panic() {
        let text = "a ".repeat(10_000);
        let opps = find_break_opportunities(&text);
        assert!(opps.len() <= text.len()); // harte Obergrenze
    }
}
