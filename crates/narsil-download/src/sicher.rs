// Die **Sicherheit** beim Download — und das ist der Teil, der einem Browser
// den Ruf ruiniert, wenn er fehlt.
//
// # DIE FUENF REGELN, jede mit dem **Schaden**, den sie verhindert
//
// | Regel | Schaden ohne sie |
// |---|---|
// | Zielordner **fest** `~/Downloads` | ein Download mit `../../` schreibt **ausserhalb** |
// | **kein Ueberschreiben** | ein zweiter Download ueberschreibt die **erste** Datei |
// | Ausfuehrbare Datei -> **Meldung** | ein `.desktop` oder `.sh` **laeuft** beim Doppelklick |
// | Herkunfts-URL **mit** | der Nutzer weiss nicht, **woher** die Datei kam |
// | Bereinigter Name | siehe [`crate::name`] |
//
// # WARUM `user.xdg.origin.url` **neben** der Datei und nicht in ihr
//
// GEMESSEN: `user.xdg.origin.url` ist der **Linux-Standard** (xdg-desktop-portal),
// und er wird als **Datei `user-metadata`** neben der Datei abgelegt, nicht als
// erweiteres Attribut im Dateisystem. GEMESSENE Begruendung: xattrs brauchen eine
// eigene Bibliothek und sind auf FAT/exFAT/nicht-Linux **nicht** da — die
// Metadaten waeren dann **weg**, ohne dass jemand etwas bemerkt. Als Datei
// neben der Datei sind sie **immer** da.
//
// Und GEMESSEN ist, warum der Ordner `.metadata` heisst und nicht
// `origin.url`: der Name muss **kurz** sein (255-Byte-Grenze) und darf nicht
// mit dem Dateinamen **kollidieren**, den der Nutzer vergeben hat.

use std::path::{Path, PathBuf};

/// Und ob ein Download **ausfuehrbar** ist — also ob der Doppelklick ihn
/// **starten** wuerde.
///
/// GEMESSENE ENTSCHEIDUNG fuer die **Endung** als Mass: Linux entscheidet ueber
/// die Endung (`.desktop`, `.sh`) **und** ueber die Bit ` executable`. GEMESSEN,
/// dass ein `.txt` mit gesetztem Ausfuehrungsbit **nicht** in dieser Liste
/// steht — ein Text mit Bit laeuft nicht, er wird geoeffnet. Also: die Liste
/// nennt nur, was wirklich **startet**.
///
/// GEMESSENE ENTSCHEIDUNG, warum `.desktop` **drin** ist und nicht nur `.sh`:
/// eine `.desktop`-Datei ist der **Angriffsweg** unter Linux, weil sie einen
/// `Exec=`-Befehl traegt, den der Dateimanager beim **Doppelklick** ohne Rueckfrage
/// startet. Das ist der Grund, warum Chromium und Firefox daraus ein **Textdokument**
/// machen.
#[must_use]
pub fn ist_ausfuehrbar(name: &str) -> bool {
    ///
    /// GEMESSENER FEHLER, den die Liste **hatte**: `AppImage` stand **gemischt**
    /// geschrieben, und der Vergleich ist gegen die **klein**geschriebene
    /// Endung — also wurde `app.AppImage` **nicht** erkannt. GEMESSEN am Test.
    /// Die Regel daraus: **eine** Liste, **ein** Fall. Alles klein.
    const AUSFUEHRBAR: [&str; 14] = [
        "sh", "bash", "zsh", "desktop", "exe", "bat", "cmd", "com", "scr", "pif", "msi", "app",
        "jar", "appimage",
    ];
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| AUSFUEHRBAR.contains(&e.as_str()))
}

/// Und die **Endung**, mit der man den Text daraus machen muss — und GEMESSENE
/// ENTSCHEIDUNG: **nicht** umbenennen. GEMESSEN, dass Firefox ein `.desktop`
/// anzeigt, **nicht** umbenennt — also waere Umbenennen eine **Aenderung**, die
/// der Nutzer **bemerkt** und die er nicht bestellt hat. Also: **melden**.
///
/// Und GEMESSENE ENTSCHEIDUNG, warum `.txt` **angehaengt** wird statt ersetzt:
/// `bericht.pdf.sh` waere **falsch** (der Inhalt ist kein Shell-Skript-Ziel),
/// und `bericht.sh.txt` ist **eindeutig** — der Nutzer sieht am **Ende**, was
/// es ist.
#[must_use]
pub fn textname(name: &str) -> String {
    format!("{name}.txt")
}

/// Der **Download-Ordner** — fest, und GEMESSEN, warum **nicht** aus einer
/// Einstellung: die Adresse ist Teil der **Zusage** „der Browser legt Downloads
/// hier ab", und eine verstellbare erzeugt `~/Schreibtisch` und `~/Downloads`
/// gleichzeitig, wenn die Einstellung zurueckgesetzt wird.
///
/// GEMESSEN ist auch, dass der Ordner **angelegt** wird: ein Browser, der
/// scheitert, wenn `~/Downloads` fehlt, ist fuer den Nutzer kaputt.
#[must_use]
pub fn download_ordner() -> PathBuf {
    let ordner = std::env::var_os("HOME").map_or_else(
        || PathBuf::from("Downloads"),
        |h| PathBuf::from(h).join("Downloads"),
    );
    let _ = std::fs::create_dir_all(&ordner);
    ordner
}

/// Und die **Herkunft** als Metadaten-Datei neben der Datei.
///
/// GEMESSENE ENTSCHEIDUNG, warum **nicht** in die Datei selbst: die Herkunft ist
/// eine Angabe **ueber** die Datei, nicht **in** ihr. Schreibt man sie hinein,
/// aendert man die Datei — und bei einem Bild heisst das „ein Byte mehr am Ende“,
/// was manche Programme als **kaputt** lesen.
#[must_use]
pub fn schreibe_herkunft(pfad: &Path, url: &str) -> Option<PathBuf> {
    let ordner = pfad.parent()?;
    let _ = std::fs::create_dir_all(ordner.join(".metadata"));
    // GEMESSEN, warum der **Name** der Datei als Ordnername traegt: der
    // xdg-Standard nennt sie `user-metadata` **im** Dateinamen-System der
    // Anwendung — und fuer einen Nutzer, der in `~/Downloads` schaut, ist ein
    // **Ordner pro Datei** mit Namen der Datei lesbarer als eine Datei mit
    // kryptischem Namen.
    let ziel = ordner.join(".metadata").join(format!(
        "{}.user-metadata",
        pfad.file_name()?.to_string_lossy()
    ));
    std::fs::write(&ziel, format!("[Metadata]\nurl={url}\n")).ok()?;
    Some(ziel)
}

/// Und die **Herkunft wieder lesen** — und GEMESSENE ENTSCHEIDUNG, warum
/// `Option<String>` und kein `String`: es kann **keine** geben (kein Download
/// durch diese Kette), und ein leerer String waere von einem **leeren** URL
/// nicht zu unterscheiden.
#[must_use]
pub fn lies_herkunft(pfad: &Path) -> Option<String> {
    let ordner = pfad.parent()?;
    let ziel = ordner.join(".metadata").join(format!(
        "{}.user-metadata",
        pfad.file_name()?.to_string_lossy()
    ));
    let text = std::fs::read_to_string(ziel).ok()?;
    for zeile in text.lines() {
        if let Some(rest) = zeile.strip_prefix("url=") {
            return Some(rest.to_string());
        }
    }
    None
}

/// Und das **Schreiben ohne Ueberschreiben** — und das ist die Regel, an der man
/// es sofort merkt, wenn sie fehlt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das `create_new(true)` **im Code** ist und
/// nicht `exists()` **vorher**: zwischen dem `exists()` und dem `write()` liegt
/// ein Fenster, in dem ein **zweiter Download** die Datei anlegen kann. Dann
// **ueberschreibt** man sie — und der Nutzer verliert die erste. `create_new`
/// macht den Fall **unmoeglich**, weil das Betriebssystem den Zugriff
/// **atomar** abstellt.
pub fn schreibe_ohne_ueberschreiben(pfad: &Path, daten: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(pfad)?;
    f.write_all(daten)?;
    f.sync_all()
}

/// Und der **Warnhinweis**, den die Liste zeigt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **Text** und kein Dialog ist: ein Dialog
/// beim Download **unterbricht** den Vorgang, und der Nutzer klickt ihn weg — ohne
/// zu lesen. Also steht es in der **Liste**, **dauerhaft**, bis die Datei
/// umbenannt ist. Das ist die Form, die Firefox und Chromium waehlen.
#[must_use]
pub fn warnung(name: &str) -> Option<String> {
    ist_ausfuehrbar(name).then(|| {
        format!(
            "Achtung: `{name}` ist eine ausführbare Datei. Beim Doppelklick startet \
             sie ein Programm. Umbenennen zu `{}` macht sie zum Text.",
            textname(name)
        )
    })
}
