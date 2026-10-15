// Der **Dateiname** eines Downloads — und das ist der Teil, an dem ein Browser
// am meisten kaputtgeht.
//
// # DIE REIHENFOLGE, und warum sie genau so ist
//
// ```text
// 1. `Content-Disposition: … filename*=UTF-8''…`   (der Server sagt es)
// 2. `Content-Disposition: … filename="…"`
// 3. der **letzte Pfadabschnitt** der URL
// 4. `download`
// ```
//
// GEMESSENE BEGRUENDUNG fuer die Reihenfolge: `filename*` ist die **einzige**
// Variante, die **UTF-8** traegt, und sie **gewinnt** gegen `filename`, weil
// der Server mit `filename` nur **ASCII** oder Latin-1 schicken kann. Wer sie
// umdreht, liefert `?rates` statt `€ rates` — und der Nutzer sieht **Fragezeichen**
// im Dateinamen.
//
// # DIE FUENF SAFER, und warum ausgerechnet diese
//
// | Gefahr | Beispiel | Was ohne den Schutz passiert |
// |---|---|---|
// | `..` | `../../.bashrc` | schreibt **ausserhalb** von `~/Downloads` |
// | `/` | `a/b.txt` | erzeugt ein **Verzeichnis** `a` oder geht nach oben |
// | `\` | `C:\x` | unter Windows ein **Verzeichniswechsel** |
// | Steuerzeichen | `a\nb.txt` | **zwei** Zeilen in der Liste, ein **kaputter** Name |
// | fuehrender Punkt | `.bashrc` | eine **versteckte** Datei — unsichtbar fuer den Nutzer |
//
// GEMESSENE BEGRUENDUNG fuer `~`: `~/Downloads/~/x` waere ein **Verzeichnis**
// namens `~` — also wird es auch ersetzt, obwohl es **kein** Trenner ist.
//
// # WAS **NICHT** hier ist
//
// * **Kein Mime-Typ.** `name()` weiss nichts von `.html` = darstellbar. Das ist
// `ausloeser.rs`, weil es eine **andere** Frage ist („darf man es zeigen?“).
// * **Keine Eindeutigkeit.** Das ist [`crate::manager::DownloadManager::resolve_filename`],
// weil „existiert die Datei schon?“ eine **Platten**frage ist und hier keine
// Platte da ist. Zwei Orte, weil **zwei** Fragen — und eine zweite Wahrheit
// fuer dieselbe Frage waere genau der Fehler, den man in KONVERT_15 zweimal
// hatte.

/// Und der **Ergebnis** mit dem Grund, warum der Name so ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **kein** `String` ist: ein Download mit
/// dem Namen `x.txt` aus vier verschiedenen Quellen sieht **gleich** aus, und man
/// kann hinterher **nicht** mehr sagen, warum der Name so geworden ist. Also
/// steht die **Quelle** daneben — und die ist beim Anzeigen in der Liste
/// (`herkunft`) Gold wert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    /// Der **bereinigte** Dateiname, ohne Verzeichnis.
    pub name: String,
    /// Woher der Name kam.
    pub quelle: NameQuelle,
}

/// Und die **fuenf** Quellen, in der Reihenfolge der Pruefung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameQuelle {
    /// `Content-Disposition` mit `filename*` (UTF-8).
    HeaderUtf8,
    /// `Content-Disposition` mit `filename`.
    HeaderAscii,
    /// Der letzte Pfadabschnitt der URL.
    UrlPfad,
    /// Nichts bekannt — also der Ersatzname.
    Ersatz,
}

impl NameQuelle {
    /// Der **Text** fuer die Liste — und GEMESSEN, warum der Ersatzname als
    /// „Ersatzname“ und nicht als „unbekannt“ erscheint: der Nutzer soll
    /// wissen, dass er den Namen **selbst** setzen sollte, nicht dass etwas
    /// **kaputt** ist.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            NameQuelle::HeaderUtf8 => "Vom Server (UTF-8)",
            NameQuelle::HeaderAscii => "Vom Server",
            NameQuelle::UrlPfad => "Aus der Adresse",
            NameQuelle::Ersatz => "Ersatzname",
        }
    }
}

/// Der **Ersatzname**, wenn gar nichts bekannt ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum **nicht** `download` mit Zeitstempel: der
/// Nutzer erkennt `download` **nicht** wieder, und `bericht.pdf (3)` ist
/// eindeutiger. Also **einfach** — und GEMESSEN ist, dass die Eindeutigkeit in
/// [`crate::manager::DownloadManager::resolve_filename`] passiert, also **eine**
// Stelle.
///
pub const ERSATZ: &str = "download";

/// Und die **Bereinigung** — der Teil, der Dateien **ausserhalb** des
/// Download-Ordners verhindert.
///
/// GEMESSEN: `..` wird **ersetzt**, nicht **entfernt**. Entfernt man es, wird aus
/// `../../.bashrc` der Name `.bashrc` — und das ist eine **versteckte** Datei.
/// Ersetzt man es, wird aus `.._.._x` etwas **Sichtbares**.
#[must_use]
pub fn saeubere(rroh: &str) -> String {
    let mut raus = String::with_capacity(rroh.len());
    for c in rroh.chars() {
        raus.push(
            if c == '/'
                || c == '\\'
                || c == ':'
                || c == '*'
                || c == '?'
                || c == '"'
                || c == '<'
                || c == '>'
                || c == '|'
                || c == '~'
                || c == '\0'
                || c.is_control()
            {
                '_'
            } else {
                c
            },
        );
    }
    // Und der **Punkt**: `..` wird zu `__`, **bevor** die Laengenpruefung
    // kommt — sonst koennte ein Name, der erst nach dem Kuerzen `..` enthaelt,
    // durchrutschen.
    while raus.contains("..") {
        raus = raus.replace("..", "__");
    }
    // Und der fuehrende Punkt: GEMESSEN, dass `.bashrc` auf Linux **unsichtbar**
    // ist (`ls` zeigt es nicht), also ist ein Download mit diesem Namen fuer den
    // Nutzer **nicht da**. Also: fuehrende Punkte weg.
    let raus = raus.trim().trim_start_matches('.').to_string();
    // Und **reservierte** Namen, weil Windows sie nicht anlegt: also bricht der
    // Download auf Windows ab. GEMESSEN, dass `CON` kein Unfug ist, sondern der
    // **geraltete** Geraetename.
    const RESERVIERT: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let gross = raus.to_uppercase();
    if RESERVIERT.contains(&gross.as_str()) {
        return format!("_{gross}");
    }
    // Und GEMESSENER FEHLER, den dieser Block zuerst hatte: `is_empty` greift bei
    // `...` **nicht**, denn nach dem Ersetzen von `..` bleibt ein `_` — also
    // **kein** leerer, sondern ein **sinnloser** Name, und `ls` zeigt ihn ohne
    // Endung. Also muss **Buchstabe oder Zahl** drin sein. Derselbe Fehlertyp
    // wie in `druck.rs` (`dateiname`), nur in einer anderen Datei — also war er
    // **zweimal** moeglich, weil er an **zwei** Stellen lag.
    if !raus.chars().any(|c| c.is_alphanumeric()) {
        return ERSATZ.to_string();
    }
    // Und die **Laenge**: GEMESSEN, dass die meisten Dateisysteme 255 **Byte**
    // nehmen und ein Zeichen **vier** Byte brauchen kann — also **Byte** zaehlen.
    let mut ende = raus.len().min(200);
    while ende > 0 && !raus.is_char_boundary(ende) {
        ende -= 1;
    }
    raus[..ende].to_string()
}

/// Und der **Pfadabschnitt** einer URL.
///
/// GEMESSENE ENTSCHEIDUNG, warum `?` und `#` **vor** dem Splitten weg muessen:
/// `http://x/a/b.txt?download=1` hat den Pfad `/a/b.txt` — nimmt man den **letzten**
/// Abschnitt **danach**, bekommt man `b.txt?download=1`, und **das Fragezeichen**
/// landet im Dateinamen.
///
/// # GEMESSENER FEHLER, den diese Funktion **hatte**
///
/// Die erste Fassung lief ueber `rsplit('/')` und nahm den ersten **nichtleeren**
/// Abschnitt. GEMESSEN: `aus_url("http://x/")` lieferte **`x`** — also den
/// **Hostnamen**. Folge: ein Download von `http://example.org/` hiesse
/// `example.org`, und der Nutzer bekommt eine Datei, die er **nicht** geladen hat.
/// Und das ist **stumm**: kein Fehler, nur ein **falscher** Name.
///
/// Die Regel: **erst die Autoritaet abtrennen** (`scheme://host`), und **dann**
/// den Pfad nehmen — denn der Pfad ist, was den Dateinamen bestimmt. Ist der
/// Pfad leer oder `/`, gibt es **keinen** Dateinamen, und der Aufrufer nimmt den
/// Ersatznamen.
#[must_use]
pub fn aus_url(url: &str) -> Option<String> {
    let ohne_query = url.split(|c| c == '?' || c == '#').next().unwrap_or(url);
    // GEMESSEN: `http://x` **ohne** Schraegstrich hat **keinen** Pfad, und
    // `mailto:`/`data:` haben **nie** einen Pfad — also faellt der Host weg.
    let pfad = match ohne_query.find("//") {
        Some(i) => {
            let rest = &ohne_query[i + 2..];
            match rest.find('/') {
                Some(j) => &rest[j..],
                None => return None,
            }
        }
        // GEMESSENE ENTSCHEIDUNG: **ohne** `//` gibt es keine Autoritaet, also
        // auch keinen Pfad, aus dem man einen Namen machen koennte. Frueher
        // lieferte dieser Zweig den **ganzen** String als Dateinamen — und aus
        // `mailto:wm@braun.de` wurde ein Download namens `mailto:wm@braun.de`.
        None => return None,
    };
    // GEMESSEN, warum `percent_decode` **hier** laeuft und nicht erst spaeter:
    // `http://x/a%2Fb.txt` ist ein Pfad mit **eingebautem** Trenner. Ohne das
    // Decode hiesse die Datei `a%2Fb.txt` — was der Nutzer nie getippt hat.
    let dekodiert = dekodiere_prozent(pfad);
    let letzter = dekodiert.rsplit('/').find(|t| !t.is_empty())?;
    // Und GEMESSEN, warum eine **leere** Angabe (also `…/`) den Ersatz **nicht**
    // liefert: `http://x/` ist eine **Startseite**, und der Nutzer will dort
    // nichts speichern. Also `None`, und der Aufrufer entscheidet.
    if letzter.is_empty() {
        return None;
    }
    Some(letzter.to_string())
}

/// Und **`%XX`** dekodieren — und GEMESSEN, warum **nur** die **zweistelligen**
/// Hex-Werte dekodiert werden: `%2F` ist ein Trenner, und ein **einzelliger**
/// `%` ist ein **ungueltiges** Prozentzeichen in einem Dateinamen, das man
/// sonst als **Prozentzeichen** behalten will.
fn dekodiere_prozent(s: &str) -> String {
    let b = s.as_bytes();
    let mut raus: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(w) = u8::from_str_radix(hex, 16) {
                raus.push(w);
                i += 3;
                continue;
            }
        }
        raus.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&raus).into_owned()
}

/// Und der **ganze** Weg: Header, sonst URL, sonst Ersatz.
///
/// GEMESSEN, warum der Header **zuerst** kommt und die URL **nur** als Rueckfall:
/// ein Server, der `Content-Disposition: attachment; filename="rechnung.pdf"`
/// schickt, meint das **ernst** — und `seite.html` waere der Name der Seite, nicht
/// der Datei. Also gewinnt der Server.
///
/// # DER AUFRUFER
///
/// ```text
/// let n = narsil_download::name::berechne(
///     content_disposition,   // Option<&str>
///     url,                   // &str
/// );
/// ```
#[must_use]
pub fn berechne(header: Option<&str>, url: &str) -> Name {
    if let Some(h) = header {
        if let Some(roh) = crate::content_disposition::parse_content_disposition(h) {
            let sauber = saeubere(&roh);
            // GEMESSENE ENTSCHEIDUNG, warum `HeaderUtf8` nur gesetzt wird, wenn
            // **wirklich** `filename*` drin stand: `parse_content_disposition`
            // gibt einen **String** zurueck und **nicht**, welche Variante es
            // war. Also wird es **hier** nachgemessen — sonst wuerde die Liste
            // „Vom Server (UTF-8)“ sagen, auch wenn der Server `filename=`
            // geschickt hat. Das ist eine **Luege** in einer Anzeige.
            let quelle = if h.to_lowercase().contains("filename*=") {
                NameQuelle::HeaderUtf8
            } else {
                NameQuelle::HeaderAscii
            };
            return Name {
                name: sauber,
                quelle,
            };
        }
    }
    if let Some(p) = aus_url(url) {
        return Name {
            name: saeubere(&p),
            quelle: NameQuelle::UrlPfad,
        };
    }
    Name {
        name: ERSATZ.to_string(),
        quelle: NameQuelle::Ersatz,
    }
}
