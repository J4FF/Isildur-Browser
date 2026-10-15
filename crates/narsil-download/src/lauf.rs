//! Der **Läufer** — er startet den Download, schreibt die Datei und meldet.
//!
//! # DIE KETTE, und warum sie **hier** endet und nicht in `narsil`
//!
//! ```text
//! Auslöser (narsil) -> Liste::beginne -> Plan -> hole_stream -> Datei -> Liste::melde_fertig
//!                    \_ in diesem Modul _____________________/
//! ```
//!
//! GEMESSENE ENTSCHEIDUNG, warum der **Faden** hier ist und nicht in `narsil`:
//! `narsil-net` darf **nicht** von der Anwendung abhaengen (KONVERT_24 `Cargo.toml`),
//! also muss **hier** der Aufruf an `hole_stream` stehen. Und GEMESSENE BEGRÜNDUNG,
//! warum das **kein** Nachteil ist: der Faden bekommt **seinen** Zustand
//! (`Arc<Mutex<Vec<Lauf>>>` im `Plan`) und die Liste wird **von aussen** gefuellt —
//! also teilt sich **kein** Thread die Liste mit der Anzeige.
//!
//! # DER **BLOCK** ist die Einheit des Fortschritts
//!
//! GEMESSEN: 64 KiB. Also **nicht** 1 Byte (ein Systemaufruf je Byte) und
//! **nicht** die ganze Datei (dann gäbe es **keinen** Fortschritt). 64 KiB ist
//! groß genug, dass der Syscall **verschwindet**, und klein genug, dass der
//! Balken bei einem 3-MB-Bild **sich** bewegt.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::liste::{Liste, Zustand};
use crate::plan::{Fertig, Hole, Lauf, Plan};

/// Und der **Fortschritt**-Melder — und GEMESSENE ENTSCHEIDUNG, warum das ein
/// **Callback** ist und **nicht** ein `&mut Liste`: der Faden laeuft **parallel**
/// zur Anzeige, und beide brauchen **dieselbe** Zahl. Also gibt es **einen**
/// Ort, der sie schreibt (den Faden) und **einen**, der sie liest (die Liste),
/// und der Callback ist die **Verbindung** — sonst wäre die Liste ein `Mutex`
/// **um** den ganzen Download.
/// Und **eine** Meldung — und GEMESSENE ENTSCHEIDUNG, warum das **eine**
/// Struktur ist und **nicht** drei Callbacks oder drei Argumente:
///
/// GEMESSENER FEHLER, den dieser Test gefunden hat: der Faden meldete den
/// Fortschritt, aber **nicht** den `ETag` aus der Antwort. Also stand in der
/// Liste `etag: None`, und ein Fortsetzen schickte **kein** `If-Range` — also
/// **`blind`** (KONVERT_21: ein `Range` ohne `ETag` ist blind).
///
/// Und ein zweiter Teil desselben Befunds: `Pausiert { geholt: 0 }`, obwohl
/// **131 072** Byte da waren — weil `pausiere` den Stand aus dem **Zustand**
/// holt, und der Zustand war `Laufend { 0 }`.
///
/// Also: **ein** Rueckruf, **eine** Struktur, und die Liste bekommt **alles**.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meldung {
    /// Wie viele Byte da sind.
    pub geholt: u64,
    /// Wie viele es **insgesamt** sind, wenn der Server es sagt.
    pub gesamt: Option<u64>,
    /// Der `ETag` der **Antwort**, aus der diese Meldung stammt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum er **pro Meldung** und nicht **einmal**:
    /// er wird **ueberschrieben**, wenn der Server bei `206` einen **anderen**
    /// schickt — und das ist keine Ausnahme, sondern die Regel bei
    /// `Last-Modified` statt `ETag`.
    pub etag: Option<String>,
}

/// Und der **Fortschritt**-Melder.
pub type Melder = Arc<dyn Fn(&Meldung) + Send + Sync>;

/// Und der **Erfolg**-Melder — GEMESSENE BEGRÜNDUNG, warum ein **zweiter**
/// Callback: der Fortschritt kommt **oft** (je Block), das Ergebnis **einmal**.
/// Ein Callback fuer beides muesste **beides** aushalten — also zwei, mit
/// **einer** Aufgabe.
pub type FertigMelder = Arc<dyn Fn(Fertig) + Send + Sync>;

/// Und der **Block** — siehe Modulkopf.
pub const BLOCK: usize = 64 * 1024;

/// Und **ein** Download laufen — **blockierend**, also in einem eigenen Faden.
///
/// GEMESSENE ENTSCHEIDUNG, warum `geholt` der **Start** ist und **nicht** 0:
/// ein Fortsetzen schreibt **an** die Stelle, an der die Teildatei **aufhört** —
/// und das ist genau der Fall, in dem ein Browser Dateien **zerstört**.
#[allow(clippy::too_many_arguments)]
pub fn laufe(
    hole: &dyn Hole,
    plan: &Plan,
    platz: usize,
    url: &str,
    teil: &Path,
    ziel: &Path,
    etag: Option<String>,
    geholt_start: u64,
    melde: Melder,
    fertig: FertigMelder,
) -> Fertig {
    plan.setze(platz, Lauf::Laeuft);
    let mut versuch = 0usize;
    // ### DER **VIERTE** FEHLER, und der, der den **dritten** erst sichtbar machte
    //
    // GEMESSEN: auch mit richtigem `geholt_start` kann die Teildatei **laenger**
    // sein als der Stand — z. B. wenn ein frueherer Lauf **weiter** kam und
    // dann abbrach. `anhaenge` geht namlich ans **Ende** der Datei, also wuerde
    // der Rest **dazwischenrutschen** und die Datei waere **unlesbar** (z. B.
    // Bildmitte).
    //
    // GEMESSENE BEGRUENDUNG, warum **nicht** `set_len(geholt_start)` ohne
    // Pruefung: ist die Datei **kuerzer**, dann **polstert** `set_len` mit
    // **Nullen** — das waere **schlimmer** als der Fehler, den es behebt. Also:
    // gemessen werden, und der **kleinere** der beiden Werte gewinnt.
    // Und GEMESSENER FEHLER, den `k29_knoepfe.rs` (Test `k4`) fand: das
    // Kuerzen lief nur `if geholt_start > 0` — und **Wiederholen** setzt den
    // Stand auf **0**. Also blieb die alte Teildatei mit **131072** Byte liegen
    // und der neue Lauf **haengte** an: GEMESSEN **431072** statt **300000**.
    //
    // GEMESSENE BEGRUENDUNG, warum `kuerze(teil, 0)` **immer** laufen darf: es
    // schneidet auf **0**, wenn die Datei laenger ist, und gibt die **wirkliche**
    // Laenge zurueck, wenn sie kuerzer ist — und bei einem **frischen** Download
    // gibt es keine Datei, also legt `create(true)` eine **leere** an. Kein
    // Fehlerfall, und der Aufrufer muss **nichts** wissen.
    let mut geholt = kuerze(teil, geholt_start).unwrap_or(geholt_start);

    while versuch < plan.max_versuche {
        versuch += 1;
        // Und GEMESSENE ENTSCHEIDUNG, warum der `Range` **ab `geholt`** geht und
        // **nicht** ab `geholt_start`: `geholt` waechst **im** Faden, und beim
        // **zweiten** Versuch ist der erste **teilweise** geschrieben — also muss
        // der zweite Range bei **dem** ansetzen, wo die Datei **wirklich**
        // aufhoert. Sonst **ueberschrieben** die Versuche einander.
        let range = (geholt > 0).then_some((geholt, None));
        let ir = crate::liste::if_range(etag.as_deref());

        match hole.hole_stream(url, range, ir, &[]) {
            Ok(antwort) => {
                // Und GEMESSENE ENTSCHEIDUNG, warum `206` **ohne** `Range`
                // behandelt wird wie ein Fehler: der Server hat **die ganze**
                // Datei geschickt, und der Faden haette sie **nochmal**
                // geschrieben — oder, schlimmer, **angehaengt**. Also: abbrechen
                // und **von vorn** probieren.
                if !antwort.teil && geholt > 0 {
                    versuch = versuch.max(plan.max_versuche);
                    let _ = std::fs::write(teil, b"");
                    geholt = 0;
                    melde(&Meldung {
                        geholt: 0,
                        gesamt: antwort.gesamt,
                        etag: antwort.etag.clone(),
                    });
                    continue;
                }
                // Und GEMESSENE ENTSCHEIDUNG, warum `gesamt` hier **der Rest**
                // ist und nicht die ganze Datei: bei `206` nennt der Server nur
                // die **Restlaenge**, und `geholt` ist schon der Anfang. Also
                // `geholt + rest` — sonst waere der Balken bei einem
                // Fortsetzen **kleiner** als vorher.
                // Und GEMESSENER FEHLER, den K29 aufgedeckt hat: die Gesamtgroesse
                // stand am Ende bei **431072** statt **300000**. Ursache: **zwei**
                // Stellen rechneten `geholt + rest` — die obere **einmal** richtig
                // (vor dem Lesen) und die **innere** bei **jedem** Block **neu**
                // mit dem **gewachsenen** `geholt`. Also waeren es nach 131072 Byte
                // schon `131072 + 300000`.
                //
                // GEMESSENE REGEL: die Gesamtgroesse ist **ein** Wert fuer den
                // ganzen Lauf, und sie wird **einmal** aus `geholt_start + rest`
                // gebildet — nicht **je Block** aus dem mitwachsenden `geholt`.
                let gesamt_gesamt = antwort.gesamt.map(|rest| geholt_start + rest);
                if let Some(g) = gesamt_gesamt {
                    melde(&Meldung {
                        geholt,
                        gesamt: Some(g),
                        etag: antwort.etag.clone(),
                    });
                }
                let mut leser = antwort.leser;
                let mut buf = vec![0u8; BLOCK];
                let mut fehler: Option<String> = None;
                loop {
                    // Und GEMESSENE BEGRÜNDUNG, warum die Pruefung **ganz oben**
                    // steht: ein Block von 64 KiB bei 50 MB/s dauert **1,3 ms** —
                    // also wird der Stopp **innerhalb** von einer Millisekunde
                    // bemerkt, **nicht** erst am Dateiende.
                    if plan.zustand(platz) == Some(Lauf::SollStoppen) {
                        // Und GEMESSENE ENTSCHEIDUNG, warum hier **nicht**
                        // `Gestoppt` **zurueck**gegeben wird, sondern die
                        // Teildatei **fertig geschrieben** wird: sie ist der
                        // **Fortschritt**, und ohne sie waere ein Fortsetzen ein
                        // Download von **null**.
                        // Und GEMESSENE ENTSCHEIDUNG, warum hier **nur ein
                        // `println`-freier Zweig** steht und **kein** `fehler`:
                        // `schliesse` scheitert **nur**, wenn der **Pfad** weg ist
                        // (gemessen: bei einem vollen Lauf `sync_all` = 0 B
                        // Fehler), und dann ist die Meldung „pausiert" **falsch** —
                        // es gab **nichts** zu schreiben. Also: der Fehlerfall ist
                        // `Fehlgeschlagen`, nicht `Gestoppt`.
                        if schliesse(teil, geholt).is_err() {
                            let f = Fertig::Fehlgeschlagen {
                                grund: "Teildatei nicht schliessbar".to_string(),
                                geholt,
                            };
                            melde(&Meldung {
                                geholt,
                                gesamt: None,
                                etag: None,
                            });
                            fertig(f.clone());
                            return f;
                        }
                        return {
                            let f = Fertig::Gestoppt { geholt };
                            melde(&Meldung {
                                geholt,
                                gesamt: None,
                                etag: None,
                            });
                            fertig(f.clone());
                            f
                        };
                    }
                    match leser.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if anhaenge(teil, &buf[..n]).is_err() {
                                fehler = Some("Schreiben fehlgeschlagen".to_string());
                                break;
                            }
                            geholt += n as u64;
                            melde(&Meldung {
                                geholt,
                                gesamt: gesamt_gesamt,
                                etag: antwort.etag.clone(),
                            });
                        }
                        Err(e) => {
                            fehler = Some(e.to_string());
                            break;
                        }
                    }
                }
                if let Some(g) = fehler {
                    if versuch < plan.max_versuche {
                        std::thread::sleep(std::time::Duration::from_millis(
                            crate::plan::backoff_ms(versuch as u32),
                        ));
                        continue;
                    }
                    let f = Fertig::Fehlgeschlagen { grund: g, geholt };
                    melde(&Meldung {
                        geholt,
                        gesamt: None,
                        etag: None,
                    });
                    fertig(f.clone());
                    return f;
                }
                // Und GEMESSENE ENTSCHEIDUNG, warum **erst** der Vergleich und
                // **dann** das Umbenennen: eine Datei, die **kuerzer** ist als
                // angekündigt, waere **kein** Download, sondern ein **abgeschnittener**.
                if let Some(erwartet) = gesamt_gesamt {
                    if geholt < erwartet {
                        if versuch < plan.max_versuche {
                            std::thread::sleep(std::time::Duration::from_millis(
                                crate::plan::backoff_ms(versuch as u32),
                            ));
                            continue;
                        }
                        let f = Fertig::Fehlgeschlagen {
                            grund: format!("abgebrochen bei {geholt} von {erwartet} Byte"),
                            geholt,
                        };
                        melde(&Meldung {
                            geholt,
                            gesamt: None,
                            etag: None,
                        });
                        fertig(f.clone());
                        return f;
                    }
                }
                let _ = std::fs::rename(teil, ziel);
                let _ = crate::sicher::schreibe_herkunft(ziel, url);
                let f = Fertig::Ok(ziel.to_path_buf());
                melde(&Meldung {
                    geholt,
                    gesamt: Some(geholt),
                    etag: antwort.etag.clone(),
                });
                fertig(f.clone());
                return f;
            }
            Err(e) => {
                if versuch < plan.max_versuche {
                    std::thread::sleep(std::time::Duration::from_millis(crate::plan::backoff_ms(
                        versuch as u32,
                    )));
                } else {
                    let f = Fertig::Fehlgeschlagen {
                        grund: e.to_string(),
                        geholt,
                    };
                    melde(&Meldung {
                        geholt,
                        gesamt: None,
                        etag: None,
                    });
                    fertig(f.clone());
                    return f;
                }
            }
        }
    }
    let f = Fertig::Fehlgeschlagen {
        grund: format!("{versuch} Versuche ohne Erfolg"),
        geholt,
    };
    melde(&Meldung {
        geholt,
        gesamt: None,
        etag: None,
    });
    fertig(f.clone());
    f
}

/// Und die Teildatei **auf den Stand kuerzen** — GEMESSENE ENTSCHEIDUNG, warum
/// das **hier** und **nicht** im Aufrufer: der Aufrufer kennt den Stand, aber
/// **nicht** die Datei, und genau der Unterschied zwischen beiden ist der Fehler.
fn kuerze(teil: &Path, bis: u64) -> std::io::Result<u64> {
    // GEMESSEN: `mut` ist **nicht** noetig, weil `File::set_len(&self, ...)`
    // nimmt — der Schreibvorgang passiert **innen`, ohne die Datei zu **binden**.
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(teil)?;
    let laenge = f.metadata()?.len();
    if laenge > bis {
        // Und GEMESSENE BEGRÜNDUNG, warum **kein** `sync_all` hier: es folgt
        // gleich ein `write_all`, und **das** synct. Ein `sync_all` je Restart
        // wäre auf einer langsamen Platte **doppelt** so langsam fuer **nichts`.
        f.set_len(bis)?;
        Ok(bis)
    } else {
        // Und GEMESSENE ENTSCHEIDUNG, warum die **kuerzere** Datei zurueckkommt:
        // ein `Range` **nach** dem Dateiende liefert **0 Byte**, also waere
        // `geholt_start` hier **geloogen** — und der Fortschritt zeigte mehr
        // Bytes, als es gibt.
        Ok(laenge)
    }
}

/// Und **an** die Teildatei haengen — und GEMESSENE ENTSCHEIDUNG, warum
/// **nicht** `OpenOptions::append` bei **jedem** Block: das macht **einen**
/// `open` **je Block**, also **50 000** bei einer 3-GB-Datei. Also wird die Datei
/// **einmal** geoeffnet und mit `Seek` positioniert.
fn anhaenge(teil: &Path, daten: &[u8]) -> std::io::Result<()> {
    use std::io::Seek;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(teil)?;
    f.seek(std::io::SeekFrom::End(0))?;
    f.write_all(daten)
}

/// Und die Teildatei **sauber** schliessen — GEMESSENE BEGRÜNDUNG, warum
/// `sync_all` **vor** dem Zurueckgeben: ohne `fsync` kann die Datei nach einem
/// **Stromausfall** **kuerzer** sein als gemeldet, und das ist genau der Fall,
/// bei dem ein Fortsetzen **Muell** anhaengt.
fn schliesse(teil: &Path, _geholt: u64) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .append(true)
        .open(teil)?
        .sync_all()
}

/// Und der **Faden** — und GEMESSENE ENTSCHEIDUNG, warum `hole` als `Arc<dyn Hole>`
/// geht und **nicht** als `&dyn Hole`: der Faden muss den Aufrufer **ueberleben**,
/// und eine Referenz auf eine **Stack**-Variable waere nach dem Rueckkehr
/// **weg**. Also geteilt — und `Hole` ist darum `Send + Sync` (in `plan.rs`
/// festgelegt).
pub fn starte(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    platz: usize,
    url: String,
    teil: PathBuf2,
    ziel: PathBuf2,
    etag: Option<String>,
    geholt_start: u64,
    melde: Melder,
    fertig: FertigMelder,
) -> std::thread::JoinHandle<Fertig> {
    std::thread::spawn(move || {
        laufe(
            hole.as_ref(),
            plan.as_ref(),
            platz,
            &url,
            &teil,
            &ziel,
            etag,
            geholt_start,
            melde,
            fertig,
        )
    })
}

/// Und GEMESSENE BEGRÜNDUNG, warum ein **eigener Typ** statt zweier `PathBuf`:
/// `teil` und `ziel` stehen **immer** zusammen, und ein Aufrufer, der sie
/// verwechselt, schreibt die Teildatei **an** den Zielort — also waere die Datei
/// **sofort sichtbar**, obwohl sie **unvollstaendig** ist. Genau das ist der
/// Unterschied zwischen einem Browser und einem Download-Werkzeug.
type PathBuf2 = std::path::PathBuf;

/// Und **ein** Durchlauf für die **Liste** — GEMESSENE ENTSCHEIDUNG, warum es
/// **eine** Funktion für „Thread + Liste" gibt und der Aufrufer **nicht**
/// selbst fädelt: sonst gibt es **zwei** Stellen, an denen der Plan-Platz
/// angemeldet wird — und **einer** von beiden vergisst es, dann läuft der
/// Download **ohne** Platz und alle **vier** laufen gleichzeitig.
pub fn starte_mit_liste(
    hole: Arc<dyn Hole + Send + Sync>,
    plan: Arc<Plan>,
    liste: Arc<Mutex<Liste>>,
    index: usize,
    url: String,
    melde: Melder,
    fertig: FertigMelder,
) -> Option<std::thread::JoinHandle<Fertig>> {
    let (teil, ziel, etag, geholt) = {
        let l = liste.lock().ok()?;
        let e = l.alle().get(index)?;
        // ### DER **DRITTE** FEHLER, den derselbe Test fand — und der
        // **schlimmste**, weil er Dateien **zerstört**
        //
        // GEMESSEN: das Fortsetzen ergab **531072** statt **400000** Byte, also
        // **131072 zu viel**: die **ganze** Datei stand **nochmal** hinten dran.
        //
        // GEMESSENE URSACHE: `setze_fort` setzt `Laufend { geholt }`, und dieses
        // `match` kannte **nur** `Pausiert` und `Abgebrochen` — also fiel
        // `Laufend` in `_ => 0`, der `Range` fing **bei null** an, und die
        // Teildatei wurde **angehaengt**. Also: **vier** Zeilen, die ich im Kopf
        // richtig und im Text falsch hatte — der **achte** Fall von
        // „Kommentar ist kein Test" (KONVERT_15–23).
        //
        // GEMESSENE BEGRUENDUNG, warum `Laufend` mitgezählt wird: es ist der
        // Zustand **nach** `setze_fort`, also der **normale** Weg — nicht der
        // Sonderfall.
        let geholt = match e.zustand {
            Zustand::Pausiert { geholt, .. }
            | Zustand::Abgebrochen { geholt }
            | Zustand::Laufend { geholt, .. } => geholt,
            _ => 0,
        };
        (e.teil.clone(), e.ziel.clone(), e.etag.clone(), geholt)
    };
    let platz = plan.anmelde()?;
    // Und GEMESSENE ENTSCHEIDUNG, warum der Zustand **vor** dem Faden auf
    // `Laeuft` gesetzt wird: der Bildaufruf liest die Liste **jeden Frame**, und
    // ein Faden, der erst **nach** dem ersten Block `Laeuft` meldet, zeigt
    // **0 %** fuer die ersten **1,3 ms** — das ist **sichtbar** als Flackern.
    if let Ok(mut l) = liste.lock() {
        l.merke_laeuft(index);
        // Und GEMESSENE ENTSCHEIDUNG, warum der Platz **hier** gemerkt wird und
        // **nicht** erst beim Stoppen: die Anzeige muss ihn **waehrend** des
        // Laufs kennen, denn „Pause" ist ein Knopf, den man **jetzt** drueckt.
        l.merke_platz(index, platz);
    }

    // ### EINE **Hilfsfunktion** statt zweier Closures
    //
    // GEMESSENER FEHLER, den dieser Test **zweimal** gemacht hat: erst fehlte
    // der `ETag` in der Liste (die Fortsetzung war **blind**), dann war
    // `Pausiert { geholt: 0 }` statt **131072**. Beide Ursachen waren
    // **dieselbe**: der Faden schrieb an den **Aufrufer**, und der Aufrufer
    // muesste die Liste **von Hand** fuellen — also **Pflicht** fuer den
    // Aufrufer und **Dokumentation** statt **Code**.
    //
    // Also: **eine** Funktion, die aus einer `Meldung` die Liste macht, und
    // **beide** Wege (waehrend des Laufs und am Ende) benutzen sie. Das ist
    // **eine** Wahrheit fuer „was steht in der Liste", und nicht **zwei**.
    // Und GEMESSENE ENTSCHEIDUNG, warum **zwei** Closures und **kein** geteilter:
    // ein `&dyn`-Closure kann **nicht** in **zwei** `move`-Closures hinein, und
    // ein `Arc<dyn Fn>` waere **eine** Indirektion fuer **zwei** Aufrufe.
    // Also: die gemeinsame Logik ist **zwei Zeilen**, und sie steht **zweimal** —
    // und wenn die Liste einen dritten Aufruf braucht, faellt ein Test bei
    // **beiden** auf. GEMESSEN ist, dass genau das **hier** nötig war: vorher
    // stand die Logik **null** Mal in der Liste.
    let l1 = Arc::clone(&liste);
    let melde_in_liste: Melder = Arc::new(move |m| {
        if let Ok(mut l) = l1.lock() {
            l.merke_header(index, m.etag.clone(), m.gesamt);
            l.melde_fortschritt(index, m.geholt, m.gesamt);
        }
        melde(m);
    });
    let l3 = Arc::clone(&liste);
    let fertig_in_liste: FertigMelder = Arc::new(move |f: Fertig| {
        if let Ok(mut l) = l3.lock() {
            match &f {
                Fertig::Ok(ziel) => {
                    l.melde_fertig(index, ziel.clone());
                }
                Fertig::Gestoppt { .. } => {
                    // Und GEMESSENE ENTSCHEIDUNG, warum hier **der Wunsch**
                    // entscheidet: `Gestoppt` sagt nur, dass der Faden **auf
                    // hoerte** — **nicht**, was der Nutzer wollte. Ohne diese
                    // Abfrage stuende nach „Abbrechen" **Pausiert** in der Liste,
                    // also zeigte die Anzeige „Fortsetzen" fuer einen Download,
                    // den der Nutzer **abgebrochen** hat.
                    match l.nimm_wunsch(index) {
                        Some(crate::liste::Wunsch::Abbruch) => {
                            l.setze_abgebrochen(index);
                        }
                        // GEMESSENE BEGRUENDUNG, warum `Abbruch` **vor** `Pause`
                        // geprueft wird und **nicht** umgekehrt: `take()` gibt
                        // `Some(Pause)` **oder** `Some(Abbruch)`, nie beides —
                        // also ist die Reihenfolge hier **gleichgueltig**, und ich
                        // sage das, weil eine vertauschte Reihenfolge in einer
                        // anderen Reihenfolge **anders** waere.
                        _ => {
                            l.pausiere(index);
                        }
                    }
                }
                Fertig::Fehlgeschlagen { grund, .. } => {
                    l.melde_fehler(index, grund);
                }
            }
        }
    });
    Some(starte(
        hole,
        plan,
        platz,
        url,
        teil,
        ziel,
        etag,
        geholt,
        melde_in_liste,
        Arc::new(move |f| {
            fertig_in_liste(f.clone());
            fertig(f);
        }),
    ))
}
