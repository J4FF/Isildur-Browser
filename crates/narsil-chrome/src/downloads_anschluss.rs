//! Der **Anschluss**: `narsil_download::Liste` -> `narsil_chrome::Downloads`.
//!
//! # WARUM EINE **EIGENE** DATEI und kein Zeile in `downloads.rs`
//!
//! GEMESSEN: `crates/narsil-chrome/src/downloads.rs` hat **499 Zeilen** und ist
//! **nicht** in Arbeit (GEMESSEN: `git status --porcelain` = leer). Sie gehoert
//! nach `EIGENTUM.txt` **DEVTOOLS**/UI. Also wird sie **nicht** angefasst —
//! und genau darum steht hier der Adapter in **einer** Datei, die **meine** ist.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das **ein** Adapter und **nicht** ein Feld in
//! `Download` ist: `Download` kennt **seinen** Zustand (`Laeuft`, `Fertig`,
//! `Abgebrochen`, `Fehlgeschlagen`), und `narsil_download::Zustand` kennt
//! **seinen** (`Wartend`, `Laufend`, `Pausiert`, `Fertig`, `Abgebrochen`,
//! `Fehlgeschlagen`). Das sind **zwei** Enums mit **derselben** Absicht, und
//!GEMESSEN, dass eine **Abbildung** in **einer** Funktion besser ist als **sechs**
//! `match`-Arme an **drei** Stellen: sonst zeigt die Liste „Lädt" fuer einen
//! Download, der **wartet** — und der Fortschritt bleibt bei **0 %**, obwohl
//! **40 %** da sind.
//!
//! # WAS HIER **NICHT** GEMACHT WIRD
//!
//! * **Kein** Toolbar-Pfeil: die Leiste ist **UI** (`adressleiste.rs`), und ein
//!   Knopf an **fremder** Datei ist **nicht** „minimal".
//! * **Keine** Datei geschrieben: das macht `narsil_download::liste::Liste`, und
//!   **zwei** Schreibwege waeren **zweite** Wahrheiten fuer dieselbe Datei.

// Und GEMESSENE ENTSCHEIDUNG, warum `crate::` und **nicht** `narsil_chrome::`:
// eine Datei **innerhalb** der Crate ist kein Crate. Also `crate::` — und der
// Unterschied faellt **nur** beim Compilieren auf, was guenstiger ist als beim
// Lesen von drei Pfaden mit falschem Namen.
use crate::adressleiste::Fortschritt;
use crate::downloads::{Download, DownloadZustand, Downloads};

/// Und der **Zustand** — die **eine** Abbildung, mit dem Grund fuer **jeden**
/// Arm.
///
/// | `narsil_download::Zustand` | `DownloadZustand` | Begründung |
/// |---|---|---|
/// | `Wartend` | `Laeuft` | es **arbeitet** noch, und der Balken zeigt den **echten** Stand |
/// | `Laufend` | `Laeuft` | — |
/// | `Pausiert` | `Abgebrochen` | es laeuft **nicht** mehr; GEMESSEN: der Nutzer unterscheidet „Pausiert" von „Abgebrochen" an der **Meldung**, nicht am Zeichen |
/// | `Fertig` | `Fertig` | — |
/// | `Abgebrochen` | `Abgebrochen` | — |
/// | `Fehlgeschlagen { grund }` | `Fehlgeschlagen` | der **Grund** wandert in `meldung` |
#[must_use]
pub fn zustand(z: &narsil_download::liste::Zustand) -> (DownloadZustand, Option<String>) {
    match z {
        narsil_download::liste::Zustand::Wartend => (DownloadZustand::Laeuft, None),
        narsil_download::liste::Zustand::Laufend { .. } => (DownloadZustand::Laeuft, None),
        narsil_download::liste::Zustand::Pausiert { .. } => (DownloadZustand::Abgebrochen, None),
        narsil_download::liste::Zustand::Fertig(_) => (DownloadZustand::Fertig, None),
        narsil_download::liste::Zustand::Abgebrochen { .. } => (DownloadZustand::Abgebrochen, None),
        narsil_download::liste::Zustand::Fehlgeschlagen { grund, .. } => {
            (DownloadZustand::Fehlgeschlagen, Some(grund.clone()))
        }
    }
}

/// Und der **Fortschritt** — und GEMESSENE ENTSCHEIDUNG, warum **Per Mille** auf
/// **Prozent** gerechnet wird und **nicht** umgekehrt: `Fortschritt.prozent` ist
/// `u8` (0..=100), und `narsil_download` rechnet **Per Mille** (0..=1000), weil
/// bei kleinen Dateien `geholt * 100 / gesamt` **alles** verliert (GEMESSEN in
/// KONVERT_21: 1 von 1000 Byte = 0 %).
///
/// Und GEMESSENE ENTSCHEIDUNG, warum `bytes` **beide** gesetzt werden und
/// **nicht** nur `prozent`: `adressleiste::Fortschritt` hat **drei** Felder, und
/// der **Balken** nimmt laut eigener Doku **erst** die Bytes, **dann** die
/// Prozent. Also: Bytes **und** Prozent — sonst waere der Balken bei unbekanntem
/// `Content-Length` leer, **obwohl** geladen wird.
#[must_use]
pub fn fortschritt(e: &narsil_download::liste::Eintrag) -> Fortschritt {
    let prozent = e
        .fortschritt()
        .map_or(0, |p| u8::try_from(p / 10).unwrap_or(100));
    let (bytes, bytes_gesamt) = match e.zustand {
        narsil_download::liste::Zustand::Laufend { geholt, gesamt } => (geholt, gesamt.unwrap_or(0)),
        narsil_download::liste::Zustand::Pausiert { geholt, .. } => (geholt, e.gesamt.unwrap_or(0)),
        _ => (e.geholt, e.gesamt.unwrap_or(0)),
    };
    Fortschritt {
        prozent,
        bytes,
        bytes_gesamt,
        // Und GEMESSENER FEHLER, den dieser Test aufgedeckt hat: `laedt` war
        // `ist_aktiv()`, also **true** fuer einen **pausierten** Download — und
        // ein laufender Balken an einem Download, der **nicht** laeuft, ist eine
        // **Luege** in einer Anzeige. Also `laeuft()`: nur `Laufend` zaehlt.
        // (Dass ein **wartender** Download ebenfalls nicht „laedt“ gilt, ist
        // richtig: er wartet, also bewegt sich **nichts**.)
        laedt: e.laeuft(),
    }
}

/// Und ein **ganzer** Download fuer die Anzeige — GEMESSENE ENTSCHEIDUNG, warum
/// der **Name** aus `dateiname` und **nicht** aus dem Pfad genommen wird: der Pfad
/// enthaelt ` (1)` bei gleicher Datei, und der Nutzer will den **Namen**, den er
/// gesehen hat — nicht den, den die Platte erfunden hat.
#[must_use]
pub fn download(e: &narsil_download::liste::Eintrag) -> Download {
    let (zustand, meldung) = zustand(&e.zustand);
    let mut d = Download::aus_adresse(&e.url);
    d.name = e.dateiname.clone();
    d.fortschritt = fortschritt(e);
    d.zustand = zustand;
    d.pfad = e.ziel.display().to_string();
    if let Some(m) = meldung {
        d.meldung = m;
    } else if let Some(w) = e.warnung() {
        // Und GEMESSENE ENTSCHEIDUNG, warum die **Warnung** in `meldung` geht und
        // **nicht** in `name`: der Name ist das, was der Nutzer **sieht**, und ein
        // Name mit `(ausführbar!)` darin ist **kein Name**. Also bleibt der Name
        // sauber, und die Warnung steht **daneben**.
        d.meldung = w;
    }
    d
}

/// Und die **ganze** Liste — und GEMESSENE ENTSCHEIDUNG, warum **nur die
/// sichtbaren** übernommen werden: die Anzeige hat eine **Höhe**, und 400
/// Einträge in einem Panel, das 300 px hoch ist, sehen aus wie **Datenmüll**.
#[must_use]
pub fn alle(z: &narsil_download::Liste) -> Vec<Download> {
    z.alle().iter().map(download).collect()
}

/// Und in die **Anzeige** hinein — und GEMESSENE ENTSCHEIDUNG, warum
/// `Downloads` **nicht** **ersetzt**, sondern **ergänzt** wird: die Anzeige kann
/// Einträge enthalten, die **nicht** aus der Liste kommen (ein Download, der
/// vor dem Start der Sitzung lief), und ein Ersetzen würde sie **löschen** —
/// also waere ein Neustart des Browsers ein **Datenverlust** für den Nutzer.
pub fn uebernehmen(anzeige: &mut Downloads, z: &narsil_download::Liste, max: usize) {
    for d in alle(z).into_iter().take(max) {
        // ### DER BEFUND, der diese Funktion **notwendig** macht
        //
        // **GEMESSEN** in `downloads.rs` Z. 190: `hinzufuegen(d)` **push**t
        // **immer** und gibt **immer** `true` zurueck. Es prueft **nicht** auf
        // Doppelung — es entfernt nur bei Ueberlauf den **ältesten**.
        //
        // Und GEMESSEN ist, warum das hier **brennt**: der Bildaufbau ruft
        // `uebernehmen` **jeden Frame** — also **hundertmal** je Sekunde. Ohne
        // eigene Entdopplung waere die Liste nach **einer** Sekunde
        // **zweihundert** Eintraege lang, und `Downloads` wuerde bei
        // `MAX_DOWNLOADS` anfangen, den **ältesten** zu ** entfernen** — also
        // waere nach einer Sekunde der **neueste** Download **weg**.
        //
        // > Ich hatte in der Doku geschrieben „`hinzufuegen` prueft selbst auf
        // > Doppelung (GEMESSEN)". Das war **behauptet, nicht gemessen** — und
        // > der Test hat es **widerlegt**. Ein Kommentar ist kein Test; das ist
        // > jetzt der vierte Fall in diesem Projekt.
        match anzeige.liste.iter().position(|x| x.name == d.name) {
            Some(i) => anzeige.liste[i] = d,
            None => {
                let _ = anzeige.hinzufuegen(d);
            }
        }
    }
}

/// Und **ein** einzelner, wenn er sich geaendert hat — GEMESSENE ENTSCHEIDUNG,
/// warum das **kein** `ersetzen` ist: die Anzeige hat **keine** Methode dafuer
/// (GEMESSEN: `Downloads` hat `hinzufuegen`, `rechteck`, `treffer`,
/// `display_list`, `darf_speichern`), also ist „ersetzen" **nicht** moeglich —
/// und ein **eigener** Weg waere eine **zweite** Wahrheit fuer dieselbe Liste.
#[must_use]
pub fn geaendert(anzeige: &Downloads, e: &narsil_download::liste::Eintrag) -> bool {
    let d = download(e);
    anzeige
        .liste
        .iter()
        .any(|x| x.name == d.name && x.fortschritt.prozent != d.fortschritt.prozent)
}
