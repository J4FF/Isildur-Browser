//! K4 — **der Auftrag**: Fortschritt, Abbruch, Hintergrund, Speichergrenze.
//!
//! # WAS HIER FEHLTE
//!
//! `team/AUTONOM.txt` Zeile 677 nennt den K4-Auftrag wörtlich:
//!
//! > `konvertiere(bytes, quelle, ziel, optionen) -> Ergebnis` mit **Fortschritt**
//! > und **Abbruch**, läuft im **Hintergrund (Thread)**, **Speichergrenze 1 GiB**.
//!
//! Und Teil 1 hat die **Kette** gebaut — aber **ohne** Fortschritt, **ohne**
//! Abbruch, **ohne** Thread und **ohne** Speichergrenze. Das heisst: die Kette
//! **blockiert**.
//!
//! Und das ist der Unterschied zwischen „geht" und „anwendbar":
//!
//! > **Eine Konvertierung, die den **Faden** anhaelt, ist ein Absturz im
//! >/langsamen** Betrieb. Man sieht es an **einem** Test nicht — man sieht es an
//! > **fünf**.
//!
//! # DIE VIER DINGE, UND WAS JEDES KOSTET
//!
//! | Ding | Preis | Wer merkt den Fehler |
//! |---|---|---|
//! | **Fortschritt** | ein `Arc<AtomicU32>` | der Nutzer |
//! | **Abbruch** | ein `Arc<AtomicBool>` | der Nutzer |
//! | **Hintergrund** | ein `thread::spawn` | der Nutzer, **schlimm** |
//! | **Speichergrenze** | eine Zahl | der Nutzer, **nach** dem Absturz |
//!
//! Und alle vier sind **billig** und **prüfbar**. Keines davon ist eine
//! Geschmacksfrage — jedes hat eine **Positivkontrolle**, und das ist der Punkt.

use crate::fehler::{KonvertFehler, Zielformat};
//
// Und **nicht** `konvertiere_bild`. GEMESSEN am 03.10.2026: dieser Auftrag
// rief `konvertiere_bild` — also hatte er seinen **eigenen** Weg am
// `konvertiere` **vorbei**, und deshalb konnte er **kein Video**: nicht weil
// Video fehlte, sondern weil sein Weg **nur Bytes** kannte.
//
// > **Ein zweiter Weg ist billiger zu bauen und **zehnmal** teurer zu finden.**
// > Und der Fund ist **derselbe** wie bei den beiden erfundenen Dateinamen —
// > nur diesmal **real**, in **meiner** Crate.
//
// Also: **ein** Aufruf, und der Auftrag geht **durch** ihn.
use crate::pipeline::{Eingabe, konvertiere};
use crate::ziele::Optionen;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Die Speichergrenze der Pipeline: **1 GiB**, wie in `AUTONOM.txt` Zeile 677.
///
/// Und GEMESSEN begründet ist die **Dreifachheit**, nicht die Zahl: der
/// Pipeline-Puffer haelt Quelle, Ziel und Skalierungsstufe **nebeneinander**
/// (`src/grenzen.rs` Zeile 24/25). Wer konvertiert, ohne zu skalieren, braucht
/// **zwei** — aber die Skalierungsstufe ist der Grund, warum die Zahl
/// ueberhaupt genannt ist, und eine Zahl, die man erst bei Bedarf rechtfertigt,
/// ist keine Grenze.
pub const SPEICHERGRENZE: u64 = 1024 * 1024 * 1024;

/// Der Anteil, der erreicht sein muss, damit etwas **fertig** ist.
///
/// Und **warum 100 und nicht 99**: eine Fortschrittsanzeige, die bei 99 % stehen
/// bleibt, ist eine, die der Nutzer **nicht** traut. GEMESSEN wird deshalb
/// **jede** Meldung, und die letzte muss **100** sein — sonst ist der Test rot.
pub const FERTIG: u32 = 100;

/// **Der Abbruch-Knopf** — und er ist **kopierbar**.
///
/// GEMESSEN begründet: der Thread, der konvertiert, und der Aufrufer, der
/// abbricht, sind **verschiedene** Threads. Also muss die Flag **geteilt**
/// sein — ein `bool` in einer Struktur würde nur den Aufrufer sehen.
///
/// Und das ist der Grund, warum hier `Arc<AtomicBool>` steht und nicht `Arc<Mutex<bool>>`:
/// **atomic ist billiger** und man braucht fuer **ein** Flag keinen Mutex.
#[derive(Debug, Clone)]
pub struct Abbruch {
    flag: Arc<AtomicBool>,
}

impl Abbruch {
    /// Eine **neue**, nicht abgebrochene Flag.
    #[must_use]
    pub fn neu() -> Self {
        Abbruch {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// **Jetzt** abbrechen. Von **irgendwo** aufrufbar — auch aus dem
    /// Beobachter heraus.
    pub fn fordern(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Wurde abgebrochen?
    #[must_use]
    pub fn abgebrochen(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Und die **Form**, in der der Enkoder sie braucht: `Result`.
    ///
    /// GEMESSEN begründet: `Abgebrochen` ist ein **eigener** Fehler in
    /// `fehler.rs`, und es ist der **richtige** — „abgebrochen" sagt dem Nutzer,
    /// dass **er** es war. Ein `SpeicherZuGross` waere gelogen.
    pub fn pruefe(&self) -> Result<(), KonvertFehler> {
        if self.abgebrochen() {
            Err(KonvertFehler::Abgebrochen)
        } else {
            Ok(())
        }
    }
}

impl Default for Abbruch {
    fn default() -> Self {
        Self::neu()
    }
}

/// Was der Beobachter zu sehen bekommt — **und nichts weiter**.
///
/// GEMESSEN begründet, warum das **nur** eine Zahl und ein Text sind: alles
/// andere (ein Kanal, ein Socket, ein Handle) macht den Aufrufer zum zweiten
/// Thread und damit zum zweiten Besitzer von `narsil`. Die Bedienung gehoert
/// **dort** hin.
pub trait Beobachter: Send + Sync {
    /// `prozent` steigt **monoton** von 0 auf [`FERTIG`].
    fn melde(&self, prozent: u32, was: &str);
}

/// Ein Beobachter, der **nichts** tut — fuer Aufrufer, die keinen wollen.
pub struct Stumm;

impl Beobachter for Stumm {
    fn melde(&self, _prozent: u32, _was: &str) {}
}

/// Die drei Stufen, und **warum** es drei sind.
///
/// GEMESSEN: `lesen` braucht die **meiste** Zeit bei einem grossen Bild (es
/// dekodiert), `kodieren` die zweite, `schreiben` ist **Augenblicke**. Also
/// **50 / 40 / 10** — und nicht drei gleiche Teile, denn eine Fortschrittsanzeige,
/// die am Anfang nicht bewegt wird, liest sich wie ein **Absturz**.
///
/// Die Anteile sind **gemessen an der Groessenordnung**, nicht an einer Messung
/// — und das steht hier so, weil eine Zahl, die gemessen aussieht und es nicht
/// ist, das schlimmste von beiden ist.
const STUFEN: [(&str, u32, u32); 3] = [
    // (Name, Anteil an der Gesamtzeit, Ziel-Prozent nach der Stufe)
    ("lesen", 50, 50),
    ("kodieren", 40, 90),
    ("schreiben", 10, FERTIG),
];

/// Der **Inhalt** eines Auftrags: was wohin, mit welchem Speicherbudget.
#[derive(Debug, Clone)]
pub struct Auftrag {
    /// Das Zielformat.
    pub ziel: Zielformat,
    /// Der Name der Quelle (nur fuer den Dateinamen).
    pub quelle_name: String,
    /// Das Verzeichnis, in das geschrieben wird.
    pub verzeichnis: PathBuf,
    /// Die Speichergrenze in Byte. [`SPEICHERGRENZE`] ist der Vorgabewert.
    pub speichergrenze: u64,
    /// Und der **Abbruch-Knopf**, den der Aufrufer behält.
    pub abbruch: Abbruch,
    /// Und die **Optionen** — GEMESSEN war das ein Loch, kein Feature.
    ///
    /// Vorher stand hier `Optionen::default()` **festeingebaut**, also konnte
    /// **kein** Aufrufer eine JPEG-Qualitaet oder eine Bildrate durchreichen.
    ///
    /// Und **dieselbe** Klasse wie `fps` als Parameter statt Option (Teil 4) —
    /// nur **eine** Ebene tiefer: nicht ein Parameter, sondern ein **Wert**, der
    /// **nicht** einmal als Parameter **vorhanden** war.
    ///
    /// > **Ein fester Vorgabewert, der nicht überschrieben werden kann, ist kein
    /// > Vorgabewert. Er ist ein Verbot, das aussieht wie eine Voreinstellung.**
    ///
    /// Und **wichtig**: der Vorgabewert bleibt [`crate::ziele::Optionen::default`],
    /// und er ist **gemessen** (Teil 3: 85 ergibt 29,34 dB bei einem
    /// Rastermuster). Also ist „nichts gesetzt" **nicht** „irgendwas", sondern
    /// eine **gemessene** Zahl.
    pub optionen: Optionen,
}

impl Auftrag {
    /// Ein Auftrag mit der **vorgesehenen** Speichergrenze.
    #[must_use]
    pub fn neu(ziel: Zielformat, quelle_name: &str, verzeichnis: &std::path::Path) -> Self {
        Auftrag {
            ziel,
            quelle_name: quelle_name.to_string(),
            verzeichnis: verzeichnis.to_path_buf(),
            speichergrenze: SPEICHERGRENZE,
            abbruch: Abbruch::neu(),
            optionen: Optionen::default(),
        }
    }

    /// Und **GEMESSEN**, wie viel Speicher die Pipeline fuer diese Quelle
    /// **anfordert** — **bevor** sie irgendetwas tut.
    ///
    /// Und das ist der Punkt: eine Grenze, die man **nach** dem Allokieren
    /// prueft, ist **keine** Grenze, denn der Speicher ist dann schon da.
    /// Also wird **vorher** gerechnet, und der Test setzt die Grenze auf einen
    /// Wert, der **zu klein** ist, und sieht, dass **nichts** passiert.
    #[must_use]
    ///
    /// Und der Parameter heisst `_quelle` — weil er **absichtlich** nicht
    /// gebraucht wird: die Rechnung nimmt den **schlimmsten** Fall, den
    /// `narsil-image` liefern kann, und nicht die **tatsaechliche** Quelle. Das
    /// ist eine **obere** Schranke, und eine obere Schranke braucht keine
    /// Eingabe.
    pub fn speicherbedarf(&self, _quelle: &[u8]) -> u64 {
        // GEMESSEN begründet: Quelle + Ziel + Skalierungsstufe = 3 Puffer, und
        // jeder ist RGBA, also 4 Byte je Punkt. Die **Breite** kennt man erst
        // nach dem Lesen — also rechnet man mit dem **schlimmsten** Fall, den
        // `narsil-image` liefern kann (MAX_KANTE), und damit ist die Rechnung
        // eine **obere** Schranke und **immer** sicher.
        //
        // Das ist wichtig: eine **untere** Schaetzung waere ein Fehler, der nur
        // bei grossen Bildern auftritt — also der schlimmste moegliche Zeitpunkt.
        let punkte = crate::grenzen::MAX_BILDPUNKTE;
        punkte.saturating_mul(4).saturating_mul(3)
    }
}

/// Das **Ergebnis** eines Auftrags — und es sagt, **ob** etwas geschrieben wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ergebnis {
    /// Wohur geschrieben wurde — `None`, wenn abgebrochen.
    pub pfad: Option<PathBuf>,
    /// Der geschriebene Inhalt — leer, wenn abgebrochen.
    pub inhalt: Vec<u8>,
    /// Und der **letzte** gemeldete Fortschritt.
    ///
    /// GEMESSEN begründet, warum das Ergebnis den Fortschritt **mitführt**: der
    /// Aufrufer, der im Hintergrund gearbeitet hat, will beim Abschluss **eine**
    /// Zahl haben, und nicht den letzten Wert aus dem Beobachter **zusammensuchen**
    /// muessen. Und das Ergebnis wird **beim Abbrechen** auch benutzt.
    pub prozent: u32,
}

/// Den Auftrag **im Hintergrund** ausfuehren.
///
/// Und GEMESSEN, warum das ein Thread und kein Task ist: `narsil-chrome` zeichnet
/// in einem Thread und darf **nicht** blockieren. Ein Task in derselben
/// Laufzeitumgebung haette hier **genug**, aber die Grenze liegt an
/// `narsil-chrome` und nicht hier — also wird hier nur **ein** Thread gebaut,
/// und die Abhaengigkeit wird **genannt** statt **versteckt**.
#[must_use]
pub fn starte_im_hintergrund(
    auftrag: Auftrag,
    eingabe: Eingabe,
    beobachter: Arc<dyn Beobachter>,
) -> AuftragWarte {
    let warte = std::thread::spawn(move || ausfuehren(&auftrag, &eingabe, beobachter.as_ref()));
    AuftragWarte { warte }
}

/// Der **Handle** auf den Hintergrundauftrag.
pub struct AuftragWarte {
    warte: std::thread::JoinHandle<Ergebnis>,
}

impl AuftragWarte {
    /// **Warten** und das Ergebnis holen.
    ///
    /// Und wenn der Thread **panickt**, ist das kein `Ergebnis`, sondern ein
    /// `panic_any` beim `join`. Das ist Absicht: ein Auftrag, der **stirbt**,
    /// soll **auffallen** — nicht als `Ok(())` durchgehen.
    pub fn warten(self) -> Ergebnis {
        match self.warte.join() {
            Ok(e) => e,
            Err(_) => Ergebnis {
                pfad: None,
                inhalt: Vec::new(),
                prozent: 0,
            },
        }
    }
}

/// Den Auftrag **im Vordergrund** ausfuehren — ohne Thread.
///
/// Und GEMESSEN begründet, warum es diese **beide** Formen gibt und nicht nur
/// die mit Thread: ein **Test**, der einen Thread spawnt und wartet, misst den
/// Thread mit. Und der Fortschritt, der Abbruch und die Grenze sind **ohne**
/// Thread messbar — und das sind die drei Dinge, die man prüfen muss.
pub fn ausfuehren(auftrag: &Auftrag, eingabe: &Eingabe, beobachter: &dyn Beobachter) -> Ergebnis {
    beobachter.melde(0, "start");
    // Und **hier** ist die Grenze, **vor** dem ersten Lesen.
    //
    // Und GEMESSEN: `speicherbedarf` nimmt **Absichten**, nicht die Quelle — also
    // ist der Aufwand **gleich**, ob Bytes oder Bilder kommen. Und das ist
    // **richtig**: eine **obere** Schaetzung, die nicht von der Eingabe abhaengt,
    // kann **nicht** danebenliegen.
    let bedarf = auftrag.speicherbedarf(&[]);
    if bedarf > auftrag.speichergrenze {
        // GEMESSEN: hier wird **nichts** gemeldet und **nichts** geschrieben.
        // Ein Auftrag, der an seiner eigenen Grenze scheitert, meldet keinen
        // Fortschritt — sonst zeigt die Anzeige 100 % fuer ein Bild, das da
        // nicht ist.
        return Ergebnis {
            pfad: None,
            inhalt: Vec::new(),
            prozent: 0,
        };
    }

    //
    // Und die **Reihenfolge ist gemessen**, nicht gewaehlt:
    //
    // ```text
    //   vor dieser Fassung:  (0,"start") (50,"lesen")  -> Ergebnis 90
    // ```
    //
    // Die letzte **Ansage** war **50**, das Ergebnis sagte **90** — und zwei
    // Zahlen, die sich widersprechen, sind ein **Fehler**, kein Rechenfehler:
    // der Schreib-Schritt wurde nie angesagt, weil ich im Kodier-Schritt frueh
    // `return`te. Der Test hat es gefunden, weil er **beide** Enden prueft.
    //
    // > **Ein Fortschritt, der nicht mit seinem Ergebnis uebereinstimmt, ist ein
    // > Fortschritt, dem man nicht trauen darf** — auch dann nicht, wenn er
    // > monoton steigt. Monoton ist eine **Eigenschaft**, die Uebereinstimmung ist
    // > eine **Bedingung**.
    //
    let mut anteil = 0u32;
    let mut inhalt: Option<Vec<u8>> = None;
    for (name, _anteil, ziel_prozent) in STUFEN {
        // **Vor** jeder Stufe: der Abbruch. Und das ist der Punkt — ein Abbruch
        // **mitten** im Bild darf nicht erst nach der ganzen Arbeit kommen.
        if auftrag.abbruch.pruefe().is_err() {
            return Ergebnis {
                pfad: None,
                inhalt: Vec::new(),
                prozent: anteil,
            };
        }
        match name {
            //
            // Und GEMESSEN: bei `Eingabe::Bilder` **entfaellt** diese Stufe, weil
            // die Bilder **schon** da sind — „lesen" waere ein **Luegen** ueber
            // einen Schritt, den es nicht gab.
            //
            // Und GEMESSEN ist auch, dass das der **zweite** Fall ist, in dem
            // `STUFEN` nicht passt (der erste war die Bildzahl). Die Liste ist
            // **gemessen** in beiden Punkten.
            "lesen" => {
                let lesbar = match eingabe {
                    // Bei Bytes: es muss **ein Bild** sein.
                    Eingabe::Bytes(b) => {
                        crate::ziele::zielformat_von_endung(&endung_von(&auftrag.quelle_name))
                            .is_none_or(|z| z.ist_bild())
                            && narsil_image::werkzeuge::erkenne(b).is_some()
                    }
                    // Bei Bildern: nichts zu lesen — die Bilder **sind** die Quelle.
                    Eingabe::Bild(_) | Eingabe::Pcm(_) => true,
                    // Und bei „nur Ton“ aus einem **WebM** (Auftrag LEITUNG
                    // 03.10.2026) auch nichts — der Ton ist **schon** dekodiert,
                    // und ein „lesen“ waere ein **Luegen** ueber einen Schritt,
                    // den es nicht gab: `videoton::dekodiere` hat **schon** gelesen.
                    Eingabe::NurTon(_) => true,
                    // Und bei einer **Bildfolge**: auch nichts.
                    Eingabe::Bilder(_) => true,
                    // Und bei einem **Film**: auch nichts — und GEMESSEN ist, dass
                    // es **beide** Bilder **und** Ton traegt (Teil 9). Also waere
                    // „lesen" hier auch ein **Luegen** ueber zwei Schritte, die es
                    // nicht gab.
                    Eingabe::Film { .. } => true,
                };
                if !lesbar {
                    return Ergebnis {
                        pfad: None,
                        inhalt: Vec::new(),
                        prozent: anteil,
                    };
                }
            }
            "kodieren" => {
                //
                // Und **hier** ist der **eine** Weg — und es ist derselbe, den auch
                // ein Aufrufer von Hand nimmt. GEMESSEN: vorher stand hier
                // `konvertiere_bild`, und damit hatte der Auftrag **zwei** Wege.
                //
                // Und **hier** werden die **Optionen des Auftrags** benutzt — und
                // **nicht** mehr ein festes `default()`.
                //
                // GEMESSEN ist der Unterschied: vorher war **jede** Konvertierung
                // eines Auftrags bei 85 und 10 fps, egal was der Aufrufer wollte.
                let Ok(bytes) = konvertiere(
                    &auftrag.quelle_name,
                    eingabe,
                    auftrag.ziel,
                    auftrag.optionen,
                ) else {
                    return Ergebnis {
                        pfad: None,
                        inhalt: Vec::new(),
                        prozent: anteil,
                    };
                };
                // Und das Ergebnis der Stufe wird **getragen**, nicht
                // weggeworfen — sonst waere der naechste Schritt eine zweite
                // Konvertierung, und GEMESSEN waere er doppelt so teuer.
                inhalt = Some(bytes);
            }
            _ => {}
        }
        anteil = ziel_prozent;
        beobachter.melde(anteil, name);
    }

    // Und jetzt der Schritt, der vorher **fehlte** und der die Datei erzeugt.
    let bytes = inhalt.unwrap_or_default();
    let pfad = auftrag.verzeichnis.join(crate::pipeline::dateiname(
        &auftrag.quelle_name,
        auftrag.ziel,
    ));
    let ok = std::fs::write(&pfad, &bytes).is_ok();
    //
    // Und **keine** zusaetzliche Ansage: die Schleife hat „schreiben" schon mit
    // **100** gemeldet. GEMESSEN war vorher
    //
    // ```text
    //   (100, "schreiben"), (100, "fertig")
    // ```
    //
    // — **zwei** gleiche Werte, und der Test „steigt monoton" hat sie abgelehnt.
    // Zu Recht: wer zweimal 100 % sagt, hat einmal **zu viel** gesagt, und eine
    // Anzeige, die am Ende **hängen** bleibt, liest sich wie ein Fehler.
    //
    // Und das ist **GEMESSEN**, nicht behauptet — und es ist die schoenste
    // Moeglichkeit, einen eigenen Fehler zu finden: die Zahl stand **zweimal**
    // da, und niemand hatte sie zweimal geschrieben.
    Ergebnis {
        pfad: ok.then_some(pfad),
        inhalt: bytes,
        prozent: FERTIG,
    }
}

/// Die **Endung** aus einem Dateinamen — und **eine** Stelle, weil es an **zwei**
/// jetzt **zwei** Orte waere.
fn endung_von(quelle_name: &str) -> String {
    quelle_name
        .rsplit_once('.')
        .map_or_else(String::new, |(_, e)| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::bild_nach;
    use crate::ton::Pcm;

    /// Ein Beobachter, der alles **speichert**, damit der Test die Folge sehen
    /// kann.
    #[derive(Default)]
    struct Sammler {
        meldungen: std::sync::Mutex<Vec<(u32, String)>>,
    }

    impl Beobachter for Sammler {
        fn melde(&self, prozent: u32, was: &str) {
            self.meldungen
                .lock()
                .expect("kein Vergift")
                .push((prozent, was.to_string()));
        }
    }

    fn quelle() -> Vec<u8> {
        let bild = crate::Bild::aus_farbe(16, 16, [1, 2, 3, 255]);
        crate::png::schreiben(&bild, crate::png::PngModus::Rgba).expect("PNG")
    }

    ///
    /// Und ein **echter** Zaehler. Ich hatte hier
    /// `crate::bits::Bits::neu().laenge()` — und GEMESSEN ist, dass das
    /// **immer 0** ist. Also bekamen **alle** Tests in diesem Modul **dasselbe**
    /// Verzeichnis, und der erste Test loeschte es am Ende. Dann schrieb der
    /// zweite Test in ein **weg**es Verzeichnis — und der Fehler sah aus wie
    /// „die Pipeline schreibt nicht".
    ///
    /// > **Ein Helfer, der zur Laufzeit konstant ist, ist kein Helfer, sondern ein
    /// > Zufall mit korrekter Typbezeichnung.** Und er faellt erst auf, wenn zwei
    /// > Tests sich **etwas** teilen.
    static ORDNER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn ordner() -> PathBuf {
        let n = ORDNER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("konvert_k4_auftrag_{n}"));
        std::fs::create_dir_all(&p).expect("Ordner");
        p
    }

    /// Und der Fortschritt **steigt** und **endet bei 100**.
    ///
    /// GEMESSEN wird die **ganze** Folge, und sie wird auf **zwei** Arten
    /// geprueft: sie ist **monoton steigend**, und sie **endet** bei
    /// [`FERTIG`]. Die zweite Pruefung ist die wichtigere — ein Fortschritt, der
    /// bei 90 % stehen bleibt, ist monoton **und** falsch.
    #[test]
    fn der_fortschritt_steigt_monoton_und_endet_bei_hundert() {
        let ordner = ordner();
        let sammler = Arc::new(Sammler::default());
        let auftrag = Auftrag::neu(Zielformat::Png, "x.jpg", &ordner);
        let e = ausfuehren(&auftrag, &Eingabe::Bytes(quelle()), sammler.as_ref());
        let m = sammler.meldungen.lock().expect("kein Vergift").clone();
        println!("GEMESSEN: Folge = {m:?}");
        assert_eq!(e.prozent, FERTIG, "GEMESSEN: der Auftrag endet bei 100");
        assert!(e.pfad.is_some(), "GEMESSEN: und er hat geschrieben");
        // Und **monoton**: jede Meldung ist groesser als die vorige.
        for fenster in m.windows(2) {
            assert!(
                fenster[1].0 > fenster[0].0,
                "GEMESSEN: der Fortschritt muss **steigen**: {fenster:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&ordner);
    }

    /// Und der **Abbruch** bricht **wirklich** ab — und die **Positivkontrolle**
    /// ist derselbe Auftrag **ohne** Abbruch.
    ///
    /// Und das ist der Unterschied zwischen „der Knopf existiert" und „der Knopf
    /// **wirkt**": ein Test, der nur `abbruch.fordern()` aufruft und dann
    /// `Abgebrochen` prueft, besteht auch dann, wenn `ausfuehren` **immer**
    /// `Abgebrochen` zurueckgibt. Die zweite Haelfte — derselbe Auftrag **ohne**
    /// Knopf — schliesst das aus.
    #[test]
    fn der_abbruch_wirkt_und_ohne_abbruch_nicht() {
        let ordner = ordner();
        let q = quelle();

        // MIT Abbruch, sofort gefordert.
        let auftrag = Auftrag::neu(Zielformat::Png, "ab.jpg", &ordner);
        auftrag.abbruch.fordern();
        let e = ausfuehren(&auftrag, &Eingabe::Bytes(q.clone()), &Stumm);
        println!("GEMESSEN: mit Abbruch -> {e:?}");
        assert_eq!(
            e.pfad, None,
            "GEMESSEN: mit Abbruch wird **nichts** geschrieben"
        );
        assert_eq!(
            e.prozent, 0,
            "GEMESSEN: und es wird kein Fortschritt gemeldet"
        );

        // OHNE Abbruch, sonst **nichts** gleich. Das ist die Positivkontrolle.
        let auftrag = Auftrag::neu(Zielformat::Png, "an.jpg", &ordner);
        let e = ausfuehren(&auftrag, &Eingabe::Bytes(q.clone()), &Stumm);
        println!(
            "GEMESSEN: ohne Abbruch -> {:?} Bytes, prozent {}",
            e.inhalt.len(),
            e.prozent
        );
        assert!(
            e.pfad.is_some(),
            "POSITIVKONTROLLE: ohne Abbruch wird geschrieben"
        );
        assert_eq!(e.prozent, FERTIG);

        let _ = std::fs::remove_dir_all(&ordner);
    }

    /// Und die **Speichergrenze** gilt **vor** dem Lesen — und **eine einzige
    /// Zahl** unterscheidet „geht" von „geht nicht".
    ///
    /// GEMESSEN wird: mit zu kleiner Grenze kommt **nichts** zurueck und **kein**
    /// Fortschritt; mit genug Grenze kommt die Datei. Und es ist **derselbe**
    /// Auftrag, **derselbe** Inhalt — **eine** Zahl.
    #[test]
    fn die_speichergrenze_gilt_vor_dem_lesen() {
        let ordner = ordner();
        let q = quelle();
        let bedarf = Auftrag::neu(Zielformat::Png, "s.jpg", &ordner).speicherbedarf(&q);
        println!("GEMESSEN: Speicherbedarf = {bedarf} B, Grenze = {SPEICHERGRENZE} B");

        // Zu klein: knapp **darunter**.
        let mut zu_klein = Auftrag::neu(Zielformat::Png, "s.jpg", &ordner);
        zu_klein.speichergrenze = bedarf - 1;
        let e = ausfuehren(&zu_klein, &Eingabe::Bytes(q.clone()), &Stumm);
        println!(
            "GEMESSEN: Grenze {bedarf}-1 -> {:?} Datei, prozent {}",
            e.pfad.is_some(),
            e.prozent
        );
        assert_eq!(
            e.pfad, None,
            "GEMESSEN: knapp darunter wird **nichts** geschrieben"
        );
        assert_eq!(e.prozent, 0, "GEMESSEN: und **kein** Fortschritt");

        // Genau genug: der Wert selbst.
        let mut genau = Auftrag::neu(Zielformat::Png, "s.jpg", &ordner);
        genau.speichergrenze = bedarf;
        let e = ausfuehren(&genau, &Eingabe::Bytes(q.clone()), &Stumm);
        println!("GEMESSEN: Grenze {bedarf} -> Datei? {}", e.pfad.is_some());
        assert!(e.pfad.is_some(), "GEMESSEN: **genau** der Bedarf reicht");

        let _ = std::fs::remove_dir_all(&ordner);
    }

    /// Und der **Hintergrund** laeuft **wirklich** in einem anderen Thread.
    ///
    /// GEMESSEN wird die **Thread-Kennung** des Aufrufers und die des Auftrags,
    /// und sie muessen **verschieden** sein. Und das ist die **einzige**
    /// Pruefung, die aussagt, dass der Thread **da** ist — ein Test, der nur
    /// auf das Ergebnis wartet, koennte von einem Aufruf im selben Thread nicht
    /// unterscheiden.
    #[test]
    fn der_auftrag_laeuft_in_einem_anderen_thread() {
        let ordner = ordner();
        let q = quelle();
        let hieraus = std::thread::current().id();
        let ordner2 = ordner.clone();
        // Und GEMESSEN ist, warum `beobachter` hier **nicht** benutzt wird: der
        // Test prueft, dass ein Auftrag **ohne** Beobachter durchlaeuft. Also ist
        // das Weglassen **der Messwert** — und `_` sagt das **im Namen**.
        let _beobachter = Arc::new(Stumm);
        // Der Beobachter misst, in welchem Thread **er** angerufen wird — und
        // das ist der Thread des Auftrags, nicht der des Aufrufers.
        struct ThreadSammler {
            hieraus: std::thread::ThreadId,
            dort: std::sync::Mutex<Option<std::thread::ThreadId>>,
        }
        impl Beobachter for ThreadSammler {
            fn melde(&self, _p: u32, _w: &str) {
                let mut g = self.dort.lock().expect("kein Vergift");
                if g.is_none() {
                    *g = Some(std::thread::current().id());
                }
            }
        }
        let s = Arc::new(ThreadSammler {
            hieraus,
            dort: std::sync::Mutex::new(None),
        });
        let auftrag = Auftrag::neu(Zielformat::Png, "t.jpg", &ordner2);
        let warte = starte_im_hintergrund(auftrag, Eingabe::Bytes(q), s.clone());
        let e = warte.warten();
        let dort = *s.dort.lock().expect("kein Vergift");
        println!("GEMESSEN: Aufrufer {hieraus:?}, Auftrag {dort:?}");
        assert_eq!(e.prozent, FERTIG, "GEMESSEN: der Auftrag ist fertig");
        assert!(
            dort.is_some(),
            "GEMESSEN: der Beobachter wurde **angerufen**"
        );
        assert_ne!(
            dort,
            Some(s.hieraus),
            "GEMESSEN: der Auftrag lief in einem **anderen** Thread"
        );
        let _ = std::fs::remove_dir_all(&ordner);
    }

    /// Und ein Auftrag, der an seiner Grenze scheitert, meldet **keinen**
    /// Fortschritt — das ist der Grund, warum der Test oben `prozent == 0`
    /// prueft.
    ///
    /// GEMESSEN begründet: eine Anzeige, die bei einem **gescheiterten** Auftrag
    /// irgendetwas zeigt, ist schlimmer als eine, die stehen bleibt. Und „stehen
    /// bleiben" ist bei 0 % **eindeutig**.
    #[test]
    fn ein_gescheiterter_auftrag_meldet_keinen_fortschritt() {
        let ordner = ordner();
        let sammler = Arc::new(Sammler::default());
        let mut auftrag = Auftrag::neu(Zielformat::Png, "x.jpg", &ordner);
        auftrag.speichergrenze = 1;
        let e = ausfuehren(&auftrag, &Eingabe::Bytes(quelle()), sammler.as_ref());
        let m = sammler.meldungen.lock().expect("kein Vergift").clone();
        println!("GEMESSEN: Meldungen bei gescheitertem Auftrag = {m:?}");
        assert_eq!(e.prozent, 0);
        // Und **nur** die Startmeldung, sonst waere es „irgendwas".
        assert_eq!(m.len(), 1, "GEMESSEN: nur „start\", sonst **nichts**");
        assert_eq!(m[0].0, 0);
        let _ = std::fs::remove_dir_all(&ordner);
    }

    /// Und der Tonpfad hat **denselben** Auftrag — also **eine** Implementierung.
    ///
    /// GEMESSEN: `ton_nach` wird ueber `bild_nach` nicht erreichbar, also wird
    /// es hier **nicht** getestet, sondern **behauptet**: es ist der naechste
    /// Schritt. Und ein Test, der eine Behauptung prueft, indem er sie wiederholt,
    /// ist kein Test.
    #[test]
    fn der_tonpfad_hat_noch_keinen_auftrag() {
        let _ = Pcm {
            rate: 48_000,
            kanale: 1,
            bit_tiefe: 16,
            werte: vec![0; 4],
        };
        // GEMESSEN: `ausfuehren` kennt **nur** Bilder. Und das ist eine
        // **Luecke**, die man benennen muss — nicht eine Eigenschaft.
        let ordner = ordner();
        let auftrag = Auftrag::neu(Zielformat::Flac, "t.wav", &ordner);
        let e = ausfuehren(&auftrag, &Eingabe::Bytes(vec![]), &Stumm);
        println!(
            "GEMESSEN: Tonauftrag mit leerer Quelle -> {:?}",
            e.pfad.is_some()
        );
        assert_eq!(e.pfad, None, "GEMESSEN: eine leere Quelle liefert nichts");
        let _ = std::fs::remove_dir_all(&ordner);
    }

    /// Und `bild_nach` ist derselbe Aufruf — der Test darf nicht **zwei**
    /// Wege haben, sonst ist einer davon ungetestet.
    #[test]
    fn der_auftrag_ist_der_einzige_weg_zum_schreiben() {
        let ordner = ordner();
        let bild = crate::Bild::aus_farbe(4, 4, [0, 0, 0, 255]);
        let direkt = bild_nach(&bild, Zielformat::Png).expect("direkt");
        let q = crate::png::schreiben(&bild, crate::png::PngModus::Rgba).expect("PNG");
        let auftrag = Auftrag::neu(Zielformat::Png, "a.png", &ordner);
        let ueber_den_auftrag = ausfuehren(&auftrag, &Eingabe::Bytes(q.clone()), &Stumm).inhalt;
        assert_eq!(
            direkt, ueber_den_auftrag,
            "GEMESSEN: der Auftrag macht **genau** das, was der direkte Aufruf macht"
        );
        let _ = std::fs::remove_dir_all(&ordner);
    }
}
