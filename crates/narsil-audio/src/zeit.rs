//! G9j: der **Text** der Zeit — die Form, in der `<audio>` sie zeigt.
//!
//! # WARUM DAS IN `narsil-audio` STEHT UND NICHT IM RENDERER
//!
//! GEMESSEN: in `narsil-page` ist der Zeittext ein **Literal**
//! (`lib.rs:5480`, `Some("0:00 / ?".to_string())`). Es gibt **keine**
//! Formatierungsfunktion — also muss **jeder**, der die Zeit anzeigt, sie
//! **selbst** formatieren. Und wenn das zwei Craten tun, gibt es **zwei**
//! Formate, die auseinanderlaufen.
//!
//! GEMESSENER Grund, warum sie **hierher** gehoert und nicht in einen
//! Renderer: `narsil-audio` haengt an **keinem anderen** Crate. Also kann
//! `narsil-page` **und** `narsil` dieselbe Funktion benutzen, ohne dass ein
//! Kreislauf entsteht. Das ist der Schnitt, den `Format::erkenne` und
//! `dekodiere_format` in G9h schon hatten (oeffentlich gemacht, weil der
//! Browser sie braucht).
//!
//! # DIE FORM, und sie ist gemessen, nicht erwaehlt
//!
//! GEMESSEN an `narsil-page` (`g9g01`, `g9g03`, `g9g06`):
//!
//! ```text
//!   G9G01 [Audio: ton.flac] | 0:00 / ?
//!   G9G03 der Anfangstext ist "0:00 / ?" — die Dauer fehlt
//!   G9G06 ohne src: [Audio: keine Datei] | 0:00 / ?
//! ```
//!
//! Daraus **zwei** gemessene Regeln:
//!
//! * **Sekunden zweistellig** (`00`), Minuten **ohne** fuehrende Null.
//!   Sonst waere es `00:00 / ?`.
//! * **`?` fuer die unbekannte Dauer.** GEMESSENE BEGRUENDUNG, die
//!   `narsil-page` selbst gibt (Zeile 5470f): `0:00 / 0:00` **behauptet**
//!   eine Datei von null Sekunden, `0:00 / ?` sagt „das weiss ich noch
//!   nicht". **Eine Anzeige, die nichts weiss, darf es nicht als Zahl
//!   ausgeben.**
//!
//! # ABRUNDEN, UND DAS IST GEMESSEN
//!
//! GEMESSEN: der Anfangstext ist `0:00`, nicht `0:01`. Also wird
//! **abgerundet**, nicht gerundet — bei `59,7` steht dort `0:59`, und im
//! naechsten Frame `1:00`. GEMESSENE BEGRUENDUNG: eine Anzeige, die
//! **frueher** eine Sekunde anzeigt, **behauptet**, es habe schon laenger
//! gespielt, als es hat.

use crate::kopf::dauer_s;

/// Die **Dauer eines Tons als Text**, oder `?` wenn sie unbekannt ist.
///
/// GEMESSENE Form aus `narsil-page`: `0:00` und — bei unbekannter Dauer —
/// `?`.
pub fn dauer_text(daten: &[u8]) -> String {
    match dauer_s(daten) {
        Ok(Some(s)) => zeit_text(s),
        // GEMESSENE ENTSCHEIDUNG, und sie ist die aus `narsil-page`: `?`
        // statt `0`. Eine Dauer von **null** Sekunden ist ein **anderes**
        // Ding als eine **unbekannte** Dauer, und `dauer_s` gibt die
        // beiden auch getrennt (`Ok(None)` / `Ok(Some(0.0))`).
        _ => "?".to_string(),
    }
}

/// Die **Zeit in Sekunden als Text**: `0:00`, `1:23`, `1:01:00`.
///
/// # DIE GRENZE ZUR STUNDE — und sie ist gemessen, nicht geraten
///
/// GEMESSEN an `narsil-page`: dort steht `0:00` fuer **null** Sekunden.
/// Also **keine** fuehrende Null bei den Minuten, und **zweistellig**
/// Sekunden. Das ist die **einzige** Form, die ich gemessen habe — und
/// eine Form, die ich **nicht** gemessen habe, darf ich nicht erfinden.
///
/// **AB EINER STUNDE** weicht die Form von der Browser-Oberflaeche ab:
/// die Browser schreiben `1:01:00` statt `61:00`. GEMESSEN ist diese
/// Abweichung **nicht** — ich habe **keine** Datei, die eine Stunde laeuft
/// (alle Testdateien sind 1 bis 3 Sekunden lang). Also **entscheide ich
/// es und sage, dass ich es entschieden habe**: `H:MM:SS` ab 3600 s, weil
/// `61:00` nicht lesbar ist und eine Zeitanzeige gelesen wird.
pub fn zeit_text(sekunden: f64) -> String {
    if !sekunden.is_finite() || sekunden < 0.0 {
        // GEMESSENER Randfall: `NaN` und `inf` kommen aus `dauer_s` nicht
        // heraus (die Rechnung ist `u64 / u32`). Aber `position_s()` kann
        // negativ werden, wenn jemand zurueckstellt. Eine Zeitanzeige, die
        // `-0:03` zeigt, **behauptet** eine Zeit vor dem Anfang.
        return "0:00".to_string();
    }
    // GEMESSEN: **einmal** abrunden, und danach **nicht** mehr.
    //
    // **EIGENER FEHLER, den dieser Test gefunden hat:** Ich hatte
    // `sekunden.floor()` **und** dann `format!("{min:.0}")` — also
    // **zwei** Rundungen. GEMESSENE Folge:
    //
    // ```text
    //   zeit_text(59,9)  -> "1:59"   statt "0:59"
    //   zeit_text(3599)  -> "60:59"  statt "0:59:59"
    // ```
    //
    // GEMESSENE DEUTUNG: bei 59,9 ist `floor() = 59`, aber `(59 % 3600)/60
    // = 0,983`, und `{:.0}` **rundet** das auf `1`. Die zweite Rundung hat
    // die erste aufgehoben und **erhoeht**. Das ist keine
    // Darstellungsfrage: eine Zeitanzeige, die bei 59,9 Sekunden `1:59`
    // zeigt, **behauptet** eine Zeit, die es noch nicht gibt.
    //
    // **REGEL: eine Zahl wird an EINER Stelle gerundet. Wer sie an zwei
    // Stellen rundet, hat sie zweimal falsch — und die zweite Rundung
    // hebt die erste auf.**
    //
    // GEMESSENE FORM DARAUS: **ganze Sekunden rechnen**, dann
    // formatieren. Bei ganzen Zahlen ist `{:02}` **exakt** und rundet
    // nicht.
    let ganz = sekunden.floor() as u64;
    let std = ganz / 3600;
    let min = (ganz % 3600) / 60;
    let sek = ganz % 60;
    if std >= 1 {
        // GEMESSENE ENTSCHEIDUNG ohne Messung: `H:MM:SS`. `61:00` waere
        // nicht lesbar, und eine Zeitanzeige wird **gelesen**. Der Bericht
        // sagt das ausdruecklich, damit niemand sie fuer gemessen haelt.
        format!("{std}:{min:02}:{sek:02}")
    } else {
        format!("{min}:{sek:02}")
    }
}

/// Die **ganze Zeile**, wie `narsil-page` sie heute als Literal traegt:
/// `0:00 / ?`.
///
/// GEMESSENE Form aus `g9g01`: **`Position / Dauer`**, mit `?` fuer die
/// unbekannte Dauer. Und GEMESSEN ist, dass der **Abstand** ein ` / `
/// mit je einem Leerzeichen ist — genau das steht in `lib.rs:5480`.
pub fn zeit_zeile(position_s: Option<f64>, dauer_s: Option<f64>) -> String {
    let pos = match position_s {
        Some(s) => zeit_text(s),
        None => "0:00".to_string(),
    };
    let dauer = match dauer_s {
        Some(s) => zeit_text(s),
        None => "?".to_string(),
    };
    format!("{pos} / {dauer}")
}

/// Die Zeile fuer **eine Datei**: Position aus dem **verbrauchten** Ton.
///
/// GEMESSENE VORGABE, und sie ist der ganze Grund fuer diese Funktion:
/// die Position kommt aus `Spieler::position_s()`, und das liest
/// `gelesen` — den **verbrauchten** Ton (`spieler.rs:323`). Also **nicht**
/// aus einem Zaehler der Frames: der stiege um die Warteschlange voraus
/// (GEMESSEN 512 KiB ≈ 5,3 s bei 48 kHz mono `i16`).
pub fn zeit_zeile_aus_daten(spieler: &crate::spieler::Spieler, daten: &[u8]) -> String {
    zeit_zeile(Some(spieler.position_s()), dauer_s(daten).ok().flatten())
}

/// Die **ehrliche** Zeile: sie behauptet die Position **nicht**, solange
/// sie um die Warteschlange vorausliegt.
///
/// # GEMESSENER GRUND, und er ist aus `g9k03` und `g9k04`
///
/// ```text
///   G9K03 ein Schritt: position_s() 0.0000 -> 0.0929 s, Untergrenze 2.9722 s
///   G9K04 nach 11 Schritten: position_s() = 1.0000 s = duration_s()
/// ```
///
/// `Spieler::position_s()` ist der **geschriebene** Ton — er laeuft der
///wirklichen Wiedergabe um die Warteschlange voraus. Also steht hier
/// `?` an der Position, bis `Spieler::position_verlaesslich_s()` eine Zahl
/// liefert.
///
/// **GEMESSENE BEGRUENDUNG fuer `?` und nicht `0:00`:** `0:00` **behauptet**
/// eine Position von null. GEMESSEN ist aber, dass sie **unbekannt** ist —
/// und `pw-play` hat noch **kein** Sample ausgegeben. Das sind **zwei**
/// verschiedene Sachen, und eine Anzeige darf sie nicht verwechseln.
///
/// **GEMESSENE GRENZE, die daraus folgt:** bei stereo 44100 Hz ist die
/// Untergrenze **2,9722 s**. Also zeigt die Anzeige fuer **jede** Datei
/// unter drei Sekunden **immer** `?` — GEMESSEN in `g9l02` an
/// `g9_ton.wav` (Dauer `1,0000 s`). Das ist **kein** Fehler, sondern eine
/// Eigenschaft der Puffergrenze, und sie muss **benannt** sein, weil sie
/// sonst wie ein Defekt aussieht.
pub fn zeit_zeile_verlaesslich(spieler: &crate::spieler::Spieler, daten: &[u8]) -> String {
    // **EIGENER FEHLER, den `g9l01` gefunden hat, und er ist ein Widerspruch
    // zu meiner eigenen Regel.** Diese Funktion sah zuerst so aus:
    //
    // ```text
    //     zeit_zeile(spieler.position_verlaesslich_s(), dauer_s(daten)...)
    // ```
    //
    // GEMESSEN: sie lieferte `"0:00 / 0:01"` statt `"? / 0:01"`. Der Grund
    // ist in [`zeit_zeile`]: dort wird `position = None` zu **`"0:00"`**.
    //
    // **UND DAS WIDERSPRICHT DER REGEL, die ich 40 Zeilen darunter
    // selbst festgeschrieben habe:** *„unbekannt" ist keine Eigenschaft
    // der Zeile, sondern der einzelnen Zahl.* Also: **Regel geschrieben
    // und im selben Zug verletzt**, an der Stelle, an der sie am
    // wichtigsten ist.
    //
    // **GEMESSENE DEUTUNG, und sie ist der Unterschied:**
    //
    // | Wert | bei `zeit_zeile` | bei `zeit_zeile_verlaesslich` |
    // |---|---|---|
    // | `0.0` | `0:00` = **Aussage** | `0:00` = **Aussage** |
    // | `None` | `0:00` = **falsch**, denn unbekannt ≠ null | **`?`** = richtig |
    //
    // **REGEL: eine Regel, die man gerade geschrieben hat, muss man am
    // naechsten Ort anwenden, an dem sie noetig ist — sonst ist sie
    // Dekoration.**
    let dauer = dauer_s(daten).ok().flatten();
    let pos = match spieler.position_verlaesslich_s() {
        Some(s) => zeit_text(s),
        None => "?".to_string(),
    };
    let ende = match dauer {
        Some(s) => zeit_text(s),
        None => "?".to_string(),
    };
    format!("{pos} / {ende}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_form_stimmt_mit_narsil_page_ueberein() {
        // **DIE ABNAHME GEGEN DAS LITERAL.** GEMESSEN an `narsil-page`
        // (`g9g01`, `g9g03`, `g9g06`) steht dort `0:00 / ?` als **Literal**
        // in `lib.rs:5480`. Wenn meine Funktion etwas anderes liefert,
        // laeuft die Anzeige auseinander — und **genau das** soll dieser
        // Test verhindern.
        assert_eq!(zeit_zeile(None, None), "0:00 / ?");
        assert_eq!(zeit_zeile(Some(0.0), None), "0:00 / ?");
        eprintln!(
            "G9J01 zeit_zeile(None, None) = \"{}\"",
            zeit_zeile(None, None)
        );
    }

    #[test]
    fn eine_bekannte_dauer_ersetzt_das_fragezeichen() {
        // GEMESSENE Regel aus `narsil-page`: `0:00 / 0:00` **behauptet**
        // eine Datei von null Sekunden. Also steht an der Stelle des `?`
        // die Zahl, sobald sie bekannt ist.
        assert_eq!(zeit_zeile(Some(0.0), Some(1.0)), "0:00 / 0:01");
        assert_eq!(zeit_zeile(Some(1.0), Some(1.0)), "0:01 / 0:01");
        assert_eq!(zeit_zeile(Some(2.5), Some(3.0)), "0:02 / 0:03");
        eprintln!(
            "G9J02 0,5 s -> \"{}\", 2,5 von 3 s -> \"{}\"",
            zeit_text(0.5),
            zeit_zeile(Some(2.5), Some(3.0))
        );
    }

    #[test]
    fn abgerundet_und_nicht_gerundet() {
        // **GEMESSEN, und das ist der Grund fuer `floor`:** `narsil-page`
        // zeigt bei **null** Sekunden `0:00`, und bei `0,999` muss es
        // ebenfalls `0:00` zeigen — nicht `0:01`. Wer **aufrundet**,
        // behauptet eine Sekunde, die noch nicht gespielt ist.
        assert_eq!(zeit_text(0.0), "0:00");
        assert_eq!(zeit_text(0.4), "0:00");
        assert_eq!(zeit_text(0.999), "0:00");
        assert_eq!(zeit_text(1.0), "0:01");
        assert_eq!(zeit_text(59.9), "0:59");
        assert_eq!(zeit_text(60.0), "1:00");
        assert_eq!(zeit_text(61.5), "1:01");
        eprintln!(
            "G9J03 0,999 -> {}, 59,9 -> {}, 60 -> {}",
            zeit_text(0.999),
            zeit_text(59.9),
            zeit_text(60.0)
        );
    }

    #[test]
    fn eine_zeit_vor_dem_anfang_ist_keine_minuszeit() {
        // GEMESSENER Randfall: `position_s()` kann negativ werden, wenn
        // jemand zurueckstellt. `-0:03` **behauptet** eine Zeit vor dem
        // Anfang des Tons — und es gibt keine.
        assert_eq!(zeit_text(-3.0), "0:00");
        assert_eq!(zeit_text(f64::NAN), "0:00");
        assert_eq!(zeit_text(f64::INFINITY), "0:00");
        eprintln!("G9J04 -3,0 -> {}", zeit_text(-3.0));
    }

    #[test]
    fn die_stunde_ist_eine_entscheidung_und_keine_messung() {
        // **Ehrlich benannt:** ich habe **keine** Messung fuer eine Stunde
        // — alle Testdateien sind 1 bis 3 Sekunden lang. Die Form
        // `H:MM:SS` ist meine **Entscheidung**, weil `61:00` nicht lesbar
        // ist. Der Test haelt sie fest, damit sie nicht stillschweigend
        // aendert.
        assert_eq!(zeit_text(3600.0), "1:00:00");
        assert_eq!(zeit_text(3661.0), "1:01:01");
        // **EIGENER FEHLER, und diesmal ist der SOLL-WERT falsch, nicht
        // der Code.** Ich hatte `3599 -> "0:59:59"` geschrieben. GEMESSEN
        // ist `3599 -> "59:59"`.
        //
        // GEMESSENE DEUTUNG: 3599 Sekunden sind **59 Minuten 59 Sekunden** —
        // das ist **keine** volle Stunde (3600), also greift der Zweig ohne
        // Stundenzahl. Die fuehrende Null bei den Minuten gehoert **nicht**
        // dazu; `0:59:59` waere eine Form, die der Zweig gar nicht
        // erzeugen kann.
        //
        // **REGEL, und sie ist dieselbe wie in Runde 11, nur auf der
        // anderen Seite:** *Normzahlen aus der Quelle holen, nicht raten.*
        // Dort ging es um eine Rate, die ich mir gemerkt hatte — hier um
        // einen Soll-Text, den ich **von Hand** geschrieben habe. Ein
        // Soll-Wert, den ich eintippe, ist **genauso fehlerhaft** wie eine
        // Zahl aus dem Gedaechtnis, und der Test hat es mir gesagt.
        assert_eq!(zeit_text(3599.0), "59:59");
        assert_eq!(
            zeit_text(3599.5),
            "59:59",
            "GEMESSEN: abgerundet, nicht gerundet"
        );
        eprintln!(
            "G9J05 3600 -> {}, 3661 -> {}, 3599 -> {}",
            zeit_text(3600.0),
            zeit_text(3661.0),
            zeit_text(3599.0)
        );
    }

    #[test]
    fn dauer_text_liest_den_kopf_und_sagt_es_wenn_es_nicht_geht() {
        // GEMESSENE Verbindung zu G9h: die Dauer kommt aus **demselben**
        // Kopf, den der Dekoder liest. Also kann sie hier nicht von der
        // dort abweichen — es ist dieselbe Funktion.
        let Ok(daten) = std::fs::read("tests/daten/g9b_mono_48k.flac") else {
            return;
        };
        assert_eq!(dauer_text(&daten), "0:01", "GEMESSEN: 48000/48000 = 1,0 s");
        assert_eq!(
            dauer_text(b"kein ton"),
            "?",
            "ohne erkennbaren Kopf gibt es kein Fragezeichen-ersatz"
        );
        eprintln!(
            "G9J06 FLAC 1,0 s -> \"{}\", kein Ton -> \"?\"",
            dauer_text(&daten)
        );
    }
}
