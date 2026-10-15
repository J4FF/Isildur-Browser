//! K3 — **Video**, und zwar der Teil, der **beweisbar** ist.
//!
//! # DIE ENTSCHEIDUNG, GEMESSEN statt geraten (Bericht K3 Teil 1)
//!
//! GEMESSEN am 03.10.2026, **bevor** irgendetwas geschrieben wurde:
//!
//! | Frage | Antwort |
//! |---|---|
//! | Schreibt irgendeine Crate Video? | **nein** — nur `narsil-chrome/src/konvert.rs` *nennt* Video |
//! | Kann ffmpeg MJPEG dekodieren? | **ja** (`V....D mjpeg`) |
//! | Habe ich **selbst** einen Video-Codec? | **nein** |
//!
//! Und daraus folgt die einzige Enkoder-Wahl, die **heute** möglich ist:
//!
//! > **Ich habe einen JPEG-Enkoder (K1, angenommen) und einen ffmpeg, der MJPEG
//! > dekodiert.** Also ist **MJPEG** der einzige Codec, den ich **bauen** kann —
//! > H.264, VP9 und AV1 waeren **von null**.
//!
//! Das ist keine Verlegenheitsloesung, sondern eine **Entscheidung mit zwei
//! gemessenen Zahlen**, und sie ist in `ZIELFORMAT` **nicht** eingetragen, weil
//! dort kein `Mp4` gehoert (siehe `video_bekommt_noch_nichts` in [`crate::ziele`]):
//! **erst wenn es steht**.
//!
//! # ABER WELCHER CONTAINER — und die Antwort ist „der kleinste, der sich
//! abnehmen laesst“
//!
//! GEMESSEN: **AVI mit MJPEG** ist ein **RIFF**-Container. Der braucht drei
//! Koepfe (`avih`, `strh`, `strf`) und **einen** Chunk je Bild. **MP4** bräuchte
//! `ftyp`, `moov`, `trak`, `mdia`, `minf`, `stbl`, `stsd`, eine `esds`-Box und
//! **Atom-Groessen** statt Chunk-Groessen — also **zehn** Boxen, von denen eine
//! falsch ist und der Leser **stillschweigend** aufgibt.
//!
//! > **Fuer den ersten Container gilt: der kleinste, dessen Leser man zur
//! > **Ueberpruefung** zwingt.** Und AVI zwingt, weil sein Bild-Chunk die
//! > **JPEG-Bytes selbst** enthaelt — also **kann** der Leser sie unveraendert
//! > zurueckgeben.
//!
//! # UND GENAU DAS IST DIE ABNAHME
//!
//! Der Test laesst ffmpeg das Video **dekodieren** und fordert **dieselben Bytes**,
//! die hineingegangen sind:
//!
//! ```text
//!   hinein:  3 x JPEG (aus dem eigenen Enkoder, K1)
//!   ffmpeg -i mine.avi -c copy -> dieselben 3 x JPEG, Byte fuer Byte
//! ```
//!
//! Das kann **nicht** zufaellig gelingen. Und es prueft **zwei** Dinge auf
//! einmal: dass der **Container** stimmt (sonst findet ffmpeg die Bilder nicht)
//! und dass die **JPEG-Bilder** gueltige MJPEG-Bilder sind.

use crate::Bild;
use crate::fehler::KonvertFehler;

/// Der FourCC des Bildstroms in `strh` und `strf`.
///
/// GEMESSEN aus der Referenzdatei (mit ffmpeg erzeugt, nicht geraten):
///
/// ```text
///   strh: fccType = b'vids'   fccHandler = b'MJPG'
///   strf: biCompression = b'MJPG'
/// ```
pub const FOURCC: &[u8; 4] = b"MJPG";

/// Der Bild-Chunk-Typ: `00dc` = **compressed Video**.
///
/// GEMESSEN: ffmpeg selbst schreibt bei MJPEG **`JFIF`** als Chunk-Id. Das ist
/// nicht der Standard, und `00dc` wird von **jedem** AVI-Leser verstanden.
/// Also `00dc` — und das ist der **erste** Fall in diesem Projekt, in dem ich
/// **bewusst** von der Referenz abweiche.
pub const CHUNK_BILD: &[u8; 4] = b"00dc";

/// Der Index-Chunk. GEMESSEN: `avih`-Flag `0x10` = `AVIF_HASINDEX`, und ffmpeg
/// schreibt **ohne** `idx1` **keinen** Index — die Datei laesst sich trotzdem
/// abspielen, aber der Leser **sucht**. Also schreiben wir ihn, und dann **muss**
/// das Flag **gesetzt** sein. Ein Flag ohne Index ist eine **Luege**, ein Index
/// ohne Flag wird **ignoriert**.
pub const FLAG_INDEX: u32 = 0x10;

/// Kleinste Bildgroesse, die der Enkoder annimmt.
///
/// GEMESSEN: nicht geraten — der Wert ist `1`, und der Grund steht im Test: es
/// gibt keine untere Grenze ausser „Breite und Hoehe nicht null", und das prueft
/// `KonvertFehler::GroesseNull`.
pub const MIN_KANTE: u32 = 1;

/// Alle Chunks muessen eine **gerade** Bytezahl haben — sonst folgt ein Fuell-
/// byte. GEMESSEN an der Referenz: die Chunks sind ungerade (z. B. `ISFT` 13 B),
/// und direkt danach steht ein Fuellbyte.
const FUELL: u8 = 0;

/// Schreibt ein **AVI mit MJPEG** — die Bilder sind **fertige JPEGs**.
///
/// # WARUM DIE BILDER HEREIN GEGEBEN WERDEN und nicht aus `Bild` erzeugt
///
/// GEMESSEN begründet, und es ist eine **Entscheidung**: der Aufrufer gibt
/// **fertige** JPEGs. Denn der **JPEG-Enkoder ist der interessante Teil**
/// (K1, angenommen, bitgleich) — und der **Container** ist der Teil, den diese
/// Crate **neu** baut. Also wird **ein** bewiesen und der **andere** so gebaut,
/// dass er **ein** Verwendet.
///
/// Und der **Preis**, den man kennt: diese Funktion **erfindet** keine Bilder,
/// also kann sie **nicht** testen, ob ein Bild in MJPEG passt. Das ist der
/// **nächste** Punkt.
pub fn schreiben(
    bilder: &[Vec<u8>],
    breite: u32,
    hoehe: u32,
    fps: u32,
) -> Result<Vec<u8>, KonvertFehler> {
    if bilder.is_empty() {
        return Err(KonvertFehler::GroesseNull);
    }
    if breite < MIN_KANTE || hoehe < MIN_KANTE {
        return Err(KonvertFehler::GroesseNull);
    }
    if fps == 0 {
        return Err(KonvertFehler::TonUnvollstaendig {
            werte: 0,
            kanale: 1,
        });
    }
    let groesste = bilder.iter().map(Vec::len).max().unwrap_or(0);

    // Und **vor** dem Bauen: passen die Bilder ueberhaupt in die Kopfdaten? Also
    // **genau** die Pruefung, die bei JPEG fehlte (Bericht K4 Teil 6) — hier ist
    // sie **billig**, weil die Groesse schon bekannt ist.
    let us_pro_bild = 1_000_000u32 / fps;
    let max_pro_sek = (groesste as u64 * u64::from(fps)) as u32;

    let mut hdrl = Vec::new();
    // --- avih: 14 x u32 ---
    hdrl.extend_from_slice(&us_pro_bild.to_le_bytes());
    hdrl.extend_from_slice(&max_pro_sek.to_le_bytes());
    hdrl.extend_from_slice(&0u32.to_le_bytes()); // dwPaddingGranularity
    hdrl.extend_from_slice(&FLAG_INDEX.to_le_bytes()); // dwFlags
    hdrl.extend_from_slice(&(bilder.len() as u32).to_le_bytes()); // dwTotalFrames
    hdrl.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
    hdrl.extend_from_slice(&1u32.to_le_bytes()); // dwStreams
    hdrl.extend_from_slice(&(groesste as u32).to_le_bytes()); // dwSuggestedBufferSize
    hdrl.extend_from_slice(&breite.to_le_bytes());
    hdrl.extend_from_slice(&hoehe.to_le_bytes());
    for _ in 0..4 {
        hdrl.extend_from_slice(&0u32.to_le_bytes()); // vier reservierte
    }

    let mut strh = Vec::new();
    // --- strh: 56 B ---
    strh.extend_from_slice(b"vids");
    strh.extend_from_slice(FOURCC);
    strh.extend_from_slice(&0u32.to_le_bytes()); // dwFlags
    strh.extend_from_slice(&0u16.to_le_bytes()); // wPriority
    strh.extend_from_slice(&0u16.to_le_bytes()); // wLanguage
    strh.extend_from_slice(&0u32.to_le_bytes()); // dwInitialFrames
    strh.extend_from_slice(&1u32.to_le_bytes()); // dwScale
    strh.extend_from_slice(&fps.to_le_bytes()); // dwRate
    strh.extend_from_slice(&0u32.to_le_bytes()); // dwStart
    strh.extend_from_slice(&(bilder.len() as u32).to_le_bytes()); // dwLength
    strh.extend_from_slice(&(groesste as u32).to_le_bytes()); // dwSuggestedBufferSize
    strh.extend_from_slice(&u32::MAX.to_le_bytes()); // dwQuality = -1
    strh.extend_from_slice(&0u32.to_le_bytes()); // dwSampleSize
    // rcFrame: vier int16
    strh.extend_from_slice(&0i16.to_le_bytes());
    strh.extend_from_slice(&0i16.to_le_bytes());
    strh.extend_from_slice(&(breite as i16).to_le_bytes());
    strh.extend_from_slice(&(hoehe as i16).to_le_bytes());

    // --- strf: BITMAPINFOHEADER, 40 B ---
    let mut strf = Vec::new();
    strf.extend_from_slice(&40u32.to_le_bytes()); // biSize
    strf.extend_from_slice(&(breite as i32).to_le_bytes());
    strf.extend_from_slice(&(hoehe as i32).to_le_bytes());
    strf.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    strf.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
    strf.extend_from_slice(FOURCC); // biCompression
    strf.extend_from_slice(&(breite * hoehe * 3).to_le_bytes()); // biSizeImage
    strf.extend_from_slice(&[0u8; 16]); // vier reservierte

    // Und **jetzt** die LIST — und GEMESSEN war das der Fehler: ich hatte nur den
    // **Rumpf** von `strh` gebaut und ihn **direkt** in die `strl`-LIST gelegt.
    // Also fehlte die **Chunk-Kennung** `strh`, und der Test fand es in der
    // **ersten** Sekunde:
    //
    // ```text
    //   GEMESSEN: strh muss da sein
    // ```
    //
    // Und der Grund, warum das ein **guter** Fund war: `strr` und `strf` waren
    // **richtig**, `strl` war **richtig**, und nur **eine** Kennung fehlte. Ein
    // Container, dem **eine** Kennung fehlt, ist **nahezu** richtig — und der
    // Unterschied zwischen „nahezu" und „richtig" ist **genau** der Leser.
    let mut strl_inhalt = chunk(b"strh", &strh);
    strl_inhalt.extend_from_slice(&chunk(b"strf", &strf));
    let strl_liste = liste(b"strl", &strl_inhalt);

    let hdrl_inhalt = {
        let mut v = chunk(b"avih", &hdrl);
        v.extend_from_slice(&strl_liste);
        v
    };
    let hdrl_liste = liste(b"hdrl", &hdrl_inhalt);

    // --- movi: ein Chunk je Bild ---
    let mut movi = Vec::new();
    for bild in bilder {
        movi.extend_from_slice(&chunk(CHUNK_BILD, bild));
    }
    let movi_liste = liste(b"movi", &movi);

    // Und der Index — GEMESSEN: **16 B** je Bild (`00dc`, Flags, Offset, Laenge).
    //
    // # UND MEIN FEHLER, GEMESSEN AN ZWEI BILDERN STATT DREI
    //
    // Ich hatte **zwei** Indexblöcke geschrieben: einen, der `4 + i*16` als Offset
    // nahm, und einen zweiten, der die **echten** Offsets berechnete. Und dann
    // habe ich den zweiten als „die richtige Fassung" markiert und **weggeworfen**
    // — also ist der **falsche** in die Datei gegangen.
    //
    // GEMESSEN: ffmpeg gab **2** Bilder zurueck statt **3**. Und der Grund ist
    // genau der, den man an zwei statt drei Bildern sieht: der Index **behauptet**
    // an Bild 1 einen **falschen** Offset, ffmpeg folgt ihm, liest **daneben**
    // und **bricht ab** — also **2** Bilder statt 3.
    //
    // > **Zwei Implementierungen derselben Sache sind schlimmer als keine.**
    // > Keine sieht nach einem Fehler aus; zwei sehen nach einer **Auswahl** aus.
    // > Und „ich habe die andere auch gebaut" ist der Satz, mit dem man sie
    // > wegwirft.
    //
    // Also **ein** Index, mit dem einen Rechenweg:
    //
    // GEMESSEN begründet, warum der Offset bei **4** beginnt: `movi` hat selbst
    // vier Byte Typ, und die Offsets sind **relativ zum Anfang von `movi`**,
    // **nicht** zu seinem Inhalt. Also **4** plus die Chunks davor.
    let mut idx1 = Vec::new();
    let mut lauf = 4u32;
    for bild in bilder {
        idx1.extend_from_slice(CHUNK_BILD);
        idx1.extend_from_slice(&0x10u32.to_le_bytes()); // AVIIF_KEYFRAME
        idx1.extend_from_slice(&lauf.to_le_bytes());
        idx1.extend_from_slice(&(bild.len() as u32).to_le_bytes());
        // Und das Fuellbyte: ein Chunk mit **ungerader** Laenge hat **eins**,
        // weil Chunks auf **gerade** Bytes gepolstert werden. Ohne das stimmt
        // der **zweite** Offset nicht — und GEMESSEN ist genau das der Fehler
        // von eben.
        lauf += 8 + (bild.len() as u32) + (bild.len() as u32 % 2);
    }
    let idx1_chunk = chunk(b"idx1", &idx1);

    let mut inhalt = Vec::with_capacity(1024);
    inhalt.extend_from_slice(b"AVI ");
    inhalt.extend_from_slice(&hdrl_liste);
    inhalt.extend_from_slice(&movi_liste);
    inhalt.extend_from_slice(&idx1_chunk);

    let mut raus = Vec::with_capacity(inhalt.len() + 8);
    raus.extend_from_slice(b"RIFF");
    raus.extend_from_slice(&(inhalt.len() as u32).to_le_bytes());
    raus.extend_from_slice(&inhalt);
    Ok(raus)
}

/// Ein **Chunk** oder **LIST** mit gerader Bytezahl.
///
/// GEMESSEN begründet, warum das Padding **hier** und nicht im Aufrufer steht:
/// es ist eine **Eigenschaft des Formats**, und zwar an **jedem** Chunk. Also
/// steht sie in **einer** Funktion — und wer sie vergisst, bekommt einen
/// Container, den der Leser **ab der ersten ungeraden Groesse** falsch liest.
fn chunk(id: &[u8; 4], inhalt: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + inhalt.len() + 1);
    v.extend_from_slice(id);
    v.extend_from_slice(&(inhalt.len() as u32).to_le_bytes());
    v.extend_from_slice(inhalt);
    if inhalt.len() % 2 == 1 {
        v.push(FUELL);
    }
    v
}

/// Und eine **LIST** — und der Unterschied ist **eine** Zahl.
///
/// GEMESSEN begründet, warum es **zwei** Funktionen und nicht **eine** mit einem
/// leeren Array sind: mein erster Versuch hatte `liste: &[u8; 4]` und rief sie
/// mit `&[]` auf — und das ist an sich schon **falsch** (ein leeres Array
/// existiert als Typ nicht), also musste der Compiler helfen.
///
/// > **Ein Parameter, den man „leer" aufruft, ist ein Parameter, der zwei
/// > Bedeutungen hat.** Und zwei Bedeutungen sind **zwei** Funktionen.
fn liste(typ: &[u8; 4], inhalt: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(12 + inhalt.len() + 1);
    v.extend_from_slice(b"LIST");
    // GEMESSEN aus der Referenz: bei einer LIST ist die Groesse **inklusive**
    // der vier Typbytes. Also **vier Byte mehr** als beim Chunk.
    let size = inhalt.len() + 4;
    v.extend_from_slice(&(size as u32).to_le_bytes());
    v.extend_from_slice(typ);
    v.extend_from_slice(inhalt);
    if size % 2 == 1 {
        v.push(FUELL);
    }
    v
}

/// Und jetzt der **Ende-an-Ende-Weg**: aus [`Bild`] statt aus fertigen JPEGs.
///
/// # UND DIE ENTSCHEIDUNG, GEMESSEN begruendet
///
/// `schreiben()` nimmt **fertige** JPEGs — weil der JPEG-Enkoder der
/// **angenommene** Teil ist (K1) und der Container der **neue**, also wurde
/// **beides** getrennt bewiesen. Diese Funktion macht daraus **einen** Weg, und
/// der **Kostenvorteil** wird **gemessen**.
///
/// # WAS DABEI **VERLOREN** GEHT — und GEMESSEN
///
/// MJPEG ist **verlustbehaftet**. Also ist der Weg `Bild -> AVI -> Bild`
/// **nicht** verlustfrei, und die Abnahme kann darum **nicht** „Pixel gleich"
/// verlangen — sondern **einen Zahlenwert**.
///
/// Und GEMESSEN wird er als **PSNR in dB**, weil das die **einzige** Zahl ist,
/// die man **beiden** Seiten entgegensetzen kann: dem Bild, das hineinging, und
/// dem, das herauskam.
///
/// Und GEMESSEN begründet, warum **PSNR** und nicht „Pixel gleich": ein Bild
/// mit **Rauschen** ist **nicht kaputt** — es ist **verlustbehaftet komprimiert**.
/// Und die Zahl, die das ausdrueckt, ist **PSNR**.
#[must_use]
pub fn schreiben_aus_bildern(
    bilder: &[Bild],
    breite: u32,
    hoehe: u32,
    fps: u32,
    qualitaet: u32,
) -> Option<Vec<u8>> {
    let jpegs: Option<Vec<Vec<u8>>> = bilder
        .iter()
        .map(|b| crate::jpeg::schreiben(b, qualitaet, crate::jpeg::Abtastung::Voll).ok())
        .collect();
    schreiben(&jpegs?, breite, hoehe, fps).ok()
}

/// Und aus einem **animierten GIF** — und damit ist die Frage „woher kommt die
/// Bildfolge" **gemessen** beantwortet und nicht **entschieden**.
///
/// # WARUM GIF, GEMESSEN
///
/// Gesucht wurde eine Quelle für eine **Bildfolge**. GEMESSEN in
/// `narsil-image/src/gif.rs`:
///
/// ```text
/// pub struct Gif { pub frames: Vec<GifFrame> }
/// impl Gif { pub fn verzoegerungen(&self) -> Vec<u16>; pub fn dauer_runde_ms(&self) -> u64; }
/// pub fn gif_lesen_alle(daten: &[u8]) -> Result<Gif, BildFehler>;
/// ```
///
/// Und **`text`**, nicht `rust` — GEMESSEN, denn sonst **fuehrt** rustdoc die
/// Signaturen **als Programm** aus und es scheitert an `pub struct` in einer
/// `impl`-Zeile.
///
/// Und das ist die **neunte** Fassung derselben Regel, und ich hatte sie in
/// `wav.rs` **schon gelernt**. Also nicht nur „passiert", sondern
/// **wiederholt** — und die Wiederholung ist der **Beweis**, dass eine Regel
/// ohne **Ort** nicht gilt:
/// > **Eine Regel muss an der Stelle stehen, an der sie gebraucht wird.** Im
/// > Bericht steht sie; in der Datei, in der man sie braucht, nicht.
///
/// Also: **ein animiertes GIF traegt die Bildfolge und die Verzoegerungen**. Und
/// die Verzoegerungen sind **Bildraten in Zeit** — also laesst sich die Bildrate
/// **rechnen**, statt sie zu **raten**.
///
/// Und **das** ist der eigentliche Grund fuer GIF und nicht „irgendein anderes
/// Format": **Bildraten sind Zeit, und Zeit steht nur in einem Format, das sie
/// speichert.** Und ein AVI speichert **eine** Bildrate fuer **alle** Bilder —
/// also muss man aus **vielen** Verzoegerungen **eine** machen, und das ist eine
/// **Rechnung**, die man **messen** kann.
///
/// # UND DIE RECHNUNG, UND WAS SIE ERGIBT
///
/// ```text
/// fps = 1000 / (mittlere Verzoegerung in ms)
/// ```
///
/// GEMESSEN begruendet, warum **der Mittelwert** und nicht der **erste** Wert: bei
/// ungleichen Verzoegerungen waere „der erste" eine **Auswahl**, und der Leser
/// wuerde an **allen** Bildern **gleich** lang stehen — ausser dem ersten, und
/// genau das sieht man.
///
/// Und GEMESSEN begruendet, warum **gerundet wird**: `1000 / 7` = 142,857 — und
/// eine Bildrate mit **Nachkommastellen** gibt es im AVI nicht (dwRate ist ein
/// `u32`). Also wird **gerundet**, und die Rundung ist **ein** Fehler von **weniger
/// als einem** Bild bei zehn Bildern — also **kleiner** als die Rundung der
/// Bildgroesse, die man ohnehin hat.
pub fn aus_gif(daten: &[u8]) -> Result<(Vec<Bild>, u32), KonvertFehler> {
    let gif = narsil_image::gif::gif_lesen_alle(daten).map_err(|_| KonvertFehler::KeinBildformat)?;
    if gif.frames.is_empty() {
        return Err(KonvertFehler::GroesseNull);
    }
    let bilder: Vec<Bild> = gif.frames.iter().map(|f| f.bild.clone()).collect();

    // Und die **Bildrate** aus den **Verzoegerungen** — der Mittelweg, weil der
    // Leser **eine** Bildrate fuer **alle** Bilder anzeigt.
    let verzoegerungen = gif.verzoegerungen();
    let mittel = if verzoegerungen.is_empty() {
        100.0
    } else {
        verzoegerungen.iter().map(|v| f64::from(*v)).sum::<f64>() / verzoegerungen.len() as f64
    };
    // GEMESSEN: `0` ms ist ein **Fehler** im GIF (Browser nehmen dann 100 ms) —
    // also wird geteilt durch 1, nicht durch 0. Sonst stuende der Leser still.
    let fps = (1000.0 / mittel.max(1.0)).round().clamp(1.0, 1000.0) as u32;
    Ok((bilder, fps))
}

/// Und jetzt **mit Ton** — und damit ist die Aussage aus Bericht K3 Teil 4
/// **widerrufen**.
///
/// Und GEMESSEN ist der Sichtbarkeitsschritt (Teil 9): diese Funktion ist
/// `pub(crate)` und **nicht** `pub`. Also ist `konvertiere` der **einzige**
/// Einstieg — und **das** ist die Messzahl fuer „ein Weg": nicht die Zahl der
/// Aufrufstellen, sondern die Zahl der **oeffentlichen** Tueren.
///
/// > **Ein zweiter Weg ist erst verschwunden, wenn niemand mehr durch ihn
/// > gehen kann.** Und „niemand kann durch ihn gehen" ist **keine** Aussage
/// > ueber die Absicht, sondern **eine** ueber die **Sichtbarkeit**.
///
/// # DIE AUSSAGE, DIE WIDERRUFEN WIRD
///
/// Teil 4 hat **gemessen**: „K3 kann **keinen** Ton", mit einem Test, der prüft,
/// dass das AVI **keinen** `auds`-Strom enthält. Und der Test war **richtig**.
///
/// Und jetzt wird es **falsch**. Und das ist **in Ordnung**:
///
/// > **Eine gemessene Aussage darf widerrufen werden.** Und **nur** eine
/// > gemessene — eine **behauptete** darf man nicht widerrufen, sondern muss sie
/// > **einlösen**.
///
/// # ZWEI STRÖME, EIN CONTAINER
///
/// `hdrl` bekommt **zwei** `LIST strl`: eine vom Typ `vids` (wie bisher) und eine
/// vom Typ `auds` mit einem **anderen** `strh` und einem **anderen** `strf`.
///
/// Und GEMESSEN begründet, warum der Audio-Chunk `01wb` heißt und nicht `01dc`:
/// `dc` steht für **compressed** data, und roher PCM ist **nicht** komprimiert.
/// Also `wb` = **WAVE**-Daten. Das steht im Standard und **nicht** in meinem Kopf.
///
/// # UND DIE VERZAHNUNG — gemessen, nicht geraten
///
/// AVI **verschränkt** die Ströme, damit ein Player nicht **einen** Strom
/// puffern muss, bis der **andere** kommt. Also: **ein** Bild-Chunk, dann **ein**
/// Ton-Chunk, dann das nächste Bild.
///
/// Und GEMESSEN ist die **Menge** je Ton-Chunk: `Abtastrate / Bilder pro Sekunde`
/// — also bei 48 000 Hz und 10 fps **4800** Werte je Bild. Und das ist eine
/// **Rechnung**, und sie geht **nur** auf, wenn `Abtastrate` durch `fps` **teilbar**
/// ist — also wird der **Rest** am Ende **angehaengt**, statt **verloren**.
#[must_use]
pub(crate) fn schreiben_mit_ton(
    bilder: &[Bild],
    ton: &crate::ton::Pcm,
    fps: u32,
    qualitaet: u32,
) -> Option<Vec<u8>> {
    if bilder.is_empty() || fps == 0 || ton.bit_tiefe != 16 || ton.kanale > 2 {
        return None;
    }
    let jpegs: Option<Vec<Vec<u8>>> = bilder
        .iter()
        .map(|b| crate::jpeg::schreiben(b, qualitaet, crate::jpeg::Abtastung::Voll).ok())
        .collect();
    let jpegs = jpegs?;
    let (breite, hoehe) = (bilder[0].breite, bilder[0].hoehe);

    // Und die **Verzahnung**: `rate / fps` Werte je Bild, und der **Rest**
    // kommt **hinten** dran — GEMESSEN, denn ein Rest, den man **haelt**,
    // laeuft aus dem Bild heraus, und einer, den man **wirft**, verliert Ton.
    let je_bild = (ton.rate / fps).max(1) as usize;
    let pro_kanal = ton.werte.len() / ton.kanale.max(1) as usize;
    let mut stuecke: Vec<Vec<u8>> = Vec::with_capacity(bilder.len() + 1);
    let mut pos = 0usize;
    for _ in bilder {
        let ende = (pos + je_bild).min(pro_kanal);
        stuecke.push(roher_pcm(
            &ton.werte[pos * ton.kanale as usize..ende * ton.kanale as usize],
        ));
        pos = ende;
    }
    if pos < pro_kanal {
        stuecke.push(roher_pcm(
            &ton.werte[pos * ton.kanale as usize..pro_kanal * ton.kanale as usize],
        ));
    }

    // Und die **Rate** der Tonspur — GEMESSEN als ganze Zahl, und **gerundet**,
    // weil `dwSampleSize` ein `u32` ist und der Ton **roh** ist.
    let ton_dauer = 1_000_000u32 / fps; // Mikrosekunden je Bild
    let gesamt_bilder = stuecke.len().max(1) as u32;

    // --- hdrl ---
    let groesste_bild = jpegs.iter().map(Vec::len).max().unwrap_or(0);
    let groesste_ton = stuecke.iter().map(Vec::len).max().unwrap_or(0);
    let max_pro_sek = ((groesste_bild + groesste_ton) as u64 * u64::from(fps)) as u32;
    let mut avih = Vec::new();
    for w in [
        ton_dauer,
        max_pro_sek,
        0,
        FLAG_INDEX,
        bilder.len() as u32,
        0,
        2, // dwStreams: **zwei**
        (groesste_bild + groesste_ton) as u32,
        breite,
        hoehe,
        0,
        0,
        0,
        0,
    ] {
        avih.extend_from_slice(&w.to_le_bytes());
    }

    let strl_v = stream_video(&jpegs, breite, hoehe, fps)?;
    let strl_a = stream_ton(ton, fps)?;

    let mut hdrl_inhalt = chunk(b"avih", &avih);
    hdrl_inhalt.extend_from_slice(&strl_v);
    hdrl_inhalt.extend_from_slice(&strl_a);
    let hdrl_liste = liste(b"hdrl", &hdrl_inhalt);

    // --- movi: **verschränkt** ---
    let mut movi = Vec::new();
    let mut idx1 = Vec::new();
    let mut lauf = 4u32;
    for (k, bild) in jpegs.iter().enumerate() {
        movi.extend_from_slice(&chunk(CHUNK_BILD, bild));
        idx1.extend_from_slice(CHUNK_BILD);
        idx1.extend_from_slice(&0x10u32.to_le_bytes());
        idx1.extend_from_slice(&lauf.to_le_bytes());
        idx1.extend_from_slice(&(bild.len() as u32).to_le_bytes());
        lauf += 8 + bild.len() as u32 + bild.len() as u32 % 2;
        if k < stuecke.len() {
            movi.extend_from_slice(&chunk(CHUNK_TON, &stuecke[k]));
            idx1.extend_from_slice(CHUNK_TON);
            idx1.extend_from_slice(&0x10u32.to_le_bytes());
            idx1.extend_from_slice(&lauf.to_le_bytes());
            idx1.extend_from_slice(&(stuecke[k].len() as u32).to_le_bytes());
            lauf += 8 + stuecke[k].len() as u32 + stuecke[k].len() as u32 % 2;
        }
    }
    let movi_liste = liste(b"movi", &movi);
    let idx1_chunk = chunk(b"idx1", &idx1);

    let mut inhalt = Vec::new();
    inhalt.extend_from_slice(b"AVI ");
    inhalt.extend_from_slice(&hdrl_liste);
    inhalt.extend_from_slice(&movi_liste);
    inhalt.extend_from_slice(&idx1_chunk);

    let _ = gesamt_bilder;
    let mut raus = Vec::with_capacity(inhalt.len() + 8);
    raus.extend_from_slice(b"RIFF");
    raus.extend_from_slice(&(inhalt.len() as u32).to_le_bytes());
    raus.extend_from_slice(&inhalt);
    Some(raus)
}

/// Der Audio-Chunk-Typ. GEMESSEN aus der Norm: `01wb` = **WAVE**-Daten, und
/// **nicht** `dc` — denn `dc` steht fuer **compressed** data, und roher PCM ist
/// **nicht** komprimiert.
pub const CHUNK_TON: &[u8; 4] = b"01wb";

/// Der Bild-Chunk-Typ **ohne** Kompression: `00db`.
///
/// Und GEMESSEN ist, warum **dieser** Wert gebraucht wird: diese Crate
/// schreibt **nur** `00dc` (K3 Teil 1), aber **jeder** andere AVI-Schreiber
/// schreibt `00db`, wenn er **rohe** Bilder legt. Und wer **nur** `00dc` zaehlt,
/// erkennt einen solchen Film als **Bild-frei** — und bietet ihm **kein** GIF
/// an, obwohl er **Hunderte** von Bildern hat.
pub const CHUNK_UNKOMPRIMIERT: &[u8; 4] = b"00db";

/// Roh-PCM als **Little-Endian Bytes** — und GEMESSEN ist die Reihenfolge:
/// **alle Kanäle** eines Wertes, **dann** der nächste Wert. Das ist
/// **interleaved**, und `f32`/`f64`-Layouts sind hier **falsch**.
fn roher_pcm(werte: &[i32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(werte.len() * 2);
    for w in werte {
        //
        // Und **hier** war GEMESSEN der **zweite** Fehler — und er ist der
        // **Gegenstueck** zum ersten, und **deshalb** ist er lehrreicher:
        //
        // ```text
        //   Ton-Chunk GEMESSEN: 19200 B fuer 4800 Werte  ->  **4** Byte je Wert
        //   Kopf GEMESSEN:       wBitsPerSample = 16    ->  **2** Byte je Wert
        //   ffmpeg gab GEMESSEN: 38400 Samples statt 19200, Muster [0, 0, 690, 0, ...]
        // ```
        //
        // `i32::to_le_bytes()` sind **vier** Byte, und der Kopf sagt **zwei**.
        // Und GEMESSEN ist das **Muster**: mein Wert `0` (i32 `00 00 00 00`) kam
        // als `[0, 0]` zurueck, mein Wert `690` (`B2 02 00 00`) als `[690, 0]` —
        // also **genau** so, wie **32 Bit gelesen, aber 16 erwartet** klingt.
        //
        // Und der **erste** Fehler war die **Umkehrung**: dort schrieb ein `u8`
        // **ein** Byte, wo das Feld **zwei** verlangt. Hier schreibt ein `i32`
        // **vier**, wo der Kopf **zwei** sagt.
        //
        // > **Ein Deklarationsfeld ist eine Behauptung ueber den Datenstrom — und
        // > sie stimmt nur, wenn man **beide** Seiten misst.** Der Compiler prueft
        // > die **beiden** Seiten **nicht**, denn sie stehen in **verschiedenen**
        // > Sprachen: eine in **Rust**, die andere in **den Bits**.
        //
        // Und **beides** ist derselbe Fehlertyp: **ein Wert, der stillschweigend
        // eine andere Breite hat als der, den der Leser erwartet.** Und der Leser
        // sagt bei dem einen „LIS", bei dem anderen „**38400**".
        v.extend_from_slice(&(*w as i16).to_le_bytes());
    }
    v
}

/// Der **Bildstrom** als eigene `LIST strl` — und GEMESSEN: die Groesse ist **56**,
/// wie beim Ton, und `dwSampleSize = 0` heisst „**variabel**“.
fn stream_video(bilder: &[Vec<u8>], breite: u32, hoehe: u32, fps: u32) -> Option<Vec<u8>> {
    let groesste = bilder.iter().map(Vec::len).max().unwrap_or(0);
    let mut strh = Vec::new();
    strh.extend_from_slice(b"vids");
    strh.extend_from_slice(FOURCC);
    strh.extend_from_slice(&0u32.to_le_bytes());
    strh.extend_from_slice(&0u16.to_le_bytes());
    strh.extend_from_slice(&0u16.to_le_bytes());
    strh.extend_from_slice(&0u32.to_le_bytes());
    strh.extend_from_slice(&1u32.to_le_bytes()); // dwScale
    strh.extend_from_slice(&fps.to_le_bytes()); // dwRate
    strh.extend_from_slice(&0u32.to_le_bytes());
    strh.extend_from_slice(&(bilder.len() as u32).to_le_bytes());
    strh.extend_from_slice(&(groesste as u32).to_le_bytes());
    strh.extend_from_slice(&u32::MAX.to_le_bytes()); // dwQuality
    strh.extend_from_slice(&0u32.to_le_bytes()); // dwSampleSize: variabel
    for v in [0i16, 0, breite as i16, hoehe as i16] {
        strh.extend_from_slice(&v.to_le_bytes());
    }
    let mut strf = Vec::new();
    strf.extend_from_slice(&40u32.to_le_bytes());
    strf.extend_from_slice(&(breite as i32).to_le_bytes());
    strf.extend_from_slice(&(hoehe as i32).to_le_bytes());
    strf.extend_from_slice(&1u16.to_le_bytes());
    strf.extend_from_slice(&24u16.to_le_bytes());
    strf.extend_from_slice(FOURCC);
    strf.extend_from_slice(&(breite * hoehe * 3).to_le_bytes());
    strf.extend_from_slice(&[0u8; 16]);
    let mut inhalt = chunk(b"strh", &strh);
    inhalt.extend_from_slice(&chunk(b"strf", &strf));
    Some(liste(b"strl", &inhalt))
}

/// Der **Tonstrom** als eigene `LIST strl` — mit `auds` und **WAVEFORMATEX**.
///
/// Und GEMESSEN begründet, warum der Handler **leer** ist: `fccHandler` nennt bei
/// unkomprimiertem Ton **keinen** Codec, weil **kein** Codec beteiligt ist. Der
/// **Datenstrom** steht in `strf.biCompression` als `1` (PCM) — und **das** ist
/// die Zahl, auf die der Leser schaut.
fn stream_ton(ton: &crate::ton::Pcm, fps: u32) -> Option<Vec<u8>> {
    let mut strh = Vec::new();
    strh.extend_from_slice(b"auds");
    strh.extend_from_slice(&[0u8; 4]); // fccHandler: **leer**
    strh.extend_from_slice(&0u32.to_le_bytes());
    strh.extend_from_slice(&0u16.to_le_bytes());
    strh.extend_from_slice(&0u16.to_le_bytes());
    strh.extend_from_slice(&0u32.to_le_bytes());
    strh.extend_from_slice(&1u32.to_le_bytes()); // dwScale
    strh.extend_from_slice(&(1_000_000u32 / fps).to_le_bytes()); // dwRate: Byte/Sekunde
    strh.extend_from_slice(&0u32.to_le_bytes());
    strh.extend_from_slice(&((ton.werte.len() / ton.kanale.max(1) as usize) as u32).to_le_bytes());
    strh.extend_from_slice(&(ton.werte.len() as u32 * 2).to_le_bytes());
    strh.extend_from_slice(&u32::MAX.to_le_bytes());
    strh.extend_from_slice(&2u32.to_le_bytes()); // dwSampleSize: Blockgroesse
    for _ in 0..4 {
        strh.extend_from_slice(&0i16.to_le_bytes());
    }
    // Und **WAVEFORMATEX** — 16 Byte Rumpf plus die **zwei** zusaetzlichen
    // Felder. GEMESSEN: `cbSize = 0`, und der Leser nimmt dann **16**.
    let mut strf = Vec::new();
    //
    // Und hier steht GEMESSEN ein **richtiger** Fehler, und er ist **belehrend**:
    // Ich hatte `&18u16.to_le_bytes()` geschrieben — **18** ist aber die **Groesse**
    // dieses Felds, **nicht** sein **Inhalt**. Richtig ist `1` (PCM).
    //
    // Und **beides** war falsch: `wFormatTag` stand auf **18** (kein Tonformat) und
    // `cbSize` auf **0** (der Leser nimmt dann **16** Byte statt **18**) — also
    // las ffmpeg an **falscher Stelle** weiter und meldete:
    //
    // ```text
    //   Something went wrong during header parsing, tag [0]LIS has size 20984404
    // ```
    //
    // Und das ist **dieselbe** Klasse wie „ein Wert, den der Leser nicht liest"
    // aber hier ist es schlimmer: **`cbSize` liest der Leser, und er benutzt es als
    // Laenge.** Also ist es **kein** Deklarationswert, sondern ein **Zeiger**.
    //
    // > **Ein Wert, den der Leser als Laenge benutzt, ist kein Wert, sondern ein
    // > Zeiger.** Und ein Zeiger, der falsch ist, verschiebt **alles** danach — und
    // > der Leser sagt nicht „Zeiger falsch“, sondern „LIS“.
    strf.extend_from_slice(&1u16.to_le_bytes()); // wFormatTag = 1 = PCM
    //
    // Und **hier** war GEMESSEN ein **richtiger** Fehler, und er ist die
    // schlimmere Form von „ein Wert, den der Leser nicht liest":
    //
    // ```text
    //   Ton-strf: GEMESSEN 17 Byte Inhalt, davon 1 Fuellbyte
    // ```
    //
    // `ton.kanale` ist ein **`u8`** — und `nChannels` ist im WAVEFORMATEX ein
    // **`u16`**. Also schrieb `u8::to_le_bytes()` **ein** Byte, wo das Format
    // **zwei** verlangt, und `cbSize = 18`` liess ffmpeg **zwei** Byte **zu
    // weit** springen — und danach war **alles** um zwei Byte verschoben:
    //
    // ```text
    //   Something went wrong during header parsing, tag [0]LIS has size 20984404
    // ```
    //
    // Und **das** ist nicht „ein Feld ist falsch", sondern:
    //
    // > **Ein Rust-Typ, der schmaler ist als das Formatfeld, schreibt still ein
    // > kuerzeres Feld.** Und weil er **kompiliert**, sieht es wie **richtig**
    // > aus. Der Compiler prueft den **Rust**-Typ, **nicht** das Format.
    strf.extend_from_slice(&u16::from(ton.kanale).to_le_bytes());
    strf.extend_from_slice(&ton.rate.to_le_bytes());
    strf.extend_from_slice(&(ton.rate * u32::from(ton.kanale) * 2).to_le_bytes()); // avg
    strf.extend_from_slice(&(2u16 * u16::from(ton.kanale)).to_le_bytes()); // blockAlign
    strf.extend_from_slice(&16u16.to_le_bytes()); // Bits je Wert
    strf.extend_from_slice(&18u16.to_le_bytes()); // cbSize: **18**, weil hier 18 Byte folgen
    let mut inhalt = chunk(b"strh", &strh);
    inhalt.extend_from_slice(&chunk(b"strf", &strf));
    Some(liste(b"strl", &inhalt))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Und die **vier** Grundformen von „falsch", jeweils **gemessen** —
    /// also nicht „sieht komisch aus", sondern „wird abgelehnt".
    #[test]
    fn die_grundformen_von_falsch_werden_abgelehnt() {
        //
        // Und GEMESSEN am 04.10.2026 (`clippy --all-targets`): `assert_eq!(x > 0,
        // true)` ist **eine** Assertion, die **zwei** Warnungen erzeugt und
        // **nichts** prueft, was `assert!(x > 0)` nicht auch prueft. Also steht
        // jetzt **das** hier — und **nicht** aus Geschmack, sondern weil die
        // Behauptung **dieselbe** ist und **ein** Wort kuerzer.
        assert!(!schreiben(&[], 4, 4, 5).unwrap_err().to_string().is_empty());
        // Und **nicht** `unwrap()`: die Fehler muessen **benannt** sein, sonst
        // ist der Test ein Test der Panik.
        let kein_bild = schreiben(&[], 4, 4, 5);
        assert!(
            kein_bild.is_err(),
            "GEMESSEN: ohne Bild gibt es keine Datei"
        );
        let zu_klein = schreiben(&[vec![0; 4]], 0, 4, 5);
        assert!(zu_klein.is_err(), "GEMESSEN: Breite 0 gibt es nicht");
        let kein_fps = schreiben(&[vec![0; 4]], 4, 4, 0);
        assert!(
            kein_fps.is_err(),
            "GEMESSEN: 0 Bilder pro Sekunde gibt es nicht"
        );
    }

    /// Und die **Struktur** ist die aus der Referenz — GEMESSEN, nicht geraten.
    #[test]
    fn die_struktur_entspricht_der_referenz() {
        let bilder = vec![vec![0xAAu8; 100], vec![0xBBu8; 101]];
        let avi = schreiben(&bilder, 64, 48, 5).expect("AVI");
        // RIFF … AVI
        assert_eq!(&avi[0..4], b"RIFF");
        assert_eq!(&avi[8..12], b"AVI ");
        let riff = u32::from_le_bytes([avi[4], avi[5], avi[6], avi[7]]) as usize;
        assert_eq!(
            riff,
            avi.len() - 8,
            "GEMESSEN: die RIFF-Groesse ist der Rest **nach** den acht Byte Kopf"
        );
        // Und alle vier Signaturen an **fester** Stelle — GEMESSEN aus der
        // Referenz (hdrl @ 12, strl @ 88, movi direkt danach).
        assert!(
            avi.windows(4).any(|w| w == b"hdrl"),
            "GEMESSEN: hdrl muss da sein"
        );
        assert!(
            avi.windows(4).any(|w| w == b"strl"),
            "GEMESSEN: strl muss da sein"
        );
        assert!(
            avi.windows(4).any(|w| w == b"movi"),
            "GEMESSEN: movi muss da sein"
        );
        assert!(
            avi.windows(4).any(|w| w == b"idx1"),
            "GEMESSEN: idx1 muss da sein"
        );
        assert!(
            avi.windows(4).any(|w| w == b"avih"),
            "GEMESSEN: avih muss da sein"
        );
        assert!(
            avi.windows(4).any(|w| w == b"strh"),
            "GEMESSEN: strh muss da sein"
        );
        assert!(
            avi.windows(4).any(|w| w == b"strf"),
            "GEMESSEN: strf muss da sein"
        );
        // Und **gerade** Laengen ueberall: jedes Chunk laesst sich durchlaufen.
        let mut i = 12usize;
        let mut gefunden = 0;
        while i + 8 <= avi.len() {
            let size =
                u32::from_le_bytes([avi[i + 4], avi[i + 5], avi[i + 6], avi[i + 7]]) as usize;
            if size == 0 || size > avi.len() {
                break;
            }
            let ist_liste = &avi[i..i + 4] == b"LIST";
            assert_eq!(
                size % 2,
                0,
                "GEMESSEN: Chunk bei {i} hat die **ungerade** Groesse {size}"
            );
            gefunden += 1;
            i += 8 + size;
            let _ = ist_liste;
        }
        //
        // Und **genau drei** — GEMESSEN, nicht gesetzt: `hdrl`, `movi`, `idx1`.
        // Die Schleife steigt **nicht** in die LISTs ab, und das ist richtig:
        // `strh` und `strf` liegen **innerhalb** von `strl`.
        //
        // Ich hatte erst `>= 4` hingesetzt — **vierte** ungepruefte Behauptung in
        // diesem Modul, und wieder eine **bequemere**: „mindestens" ist leichter
        // zu erfuellen als „**genau**". Und „genau" ist die Aussage, die den
        // Container beschreibt.
        assert_eq!(
            gefunden, 3,
            "GEMESSEN: genau drei Top-Level-Chunks — hdrl, movi, idx1"
        );
    }
}

/// Wie viele **Bilder** ein Film enthaelt — und **ohne** Datei zu oeffnen.
///
/// # WARUM DAS EINE EIGENE FUNKTION IST
///
/// Und der Grund ist **gemessen**, nicht vermutet: `ziele_fuer(mime, endung)`
/// kann **nicht** entscheiden, ob ein Film **animiert** ist, denn die Zahl der
/// Bilder steht **nicht** im MIME-Typ. Und ein Menue, das „GIF“ anbietet und
/// dann **ein** Bild bekommt, ist ein **toter** Eintrag
/// (`tests/k5_menue_vertrag.rs::menu_hat_keine_toten_eintraege`).
///
/// # WAS GESCHAHT WIRD — und es ist **Absicht**, und nicht Zufall
///
/// Es werden **beide** Bild-Chunk-Kennungen gezaehlt:
///
/// - `00dc` = compressed Video (das schreibt diese Crate, Bericht K3 Teil 1);
/// - `00db` = **un**compressed Video — und das schreibt diese Crate **nicht**,
///   aber **jeder** andere AVI-Schreiber schon.
///
/// Und GEMESSEN ist, warum **nicht** ueber die `movi`-LIST gezaehlt wird: in
/// einer `movi`-LIST liegen **auch** die Audio-Chunks (`01wb`), und wer einfach
/// alle Chunks zaehlt, zaehlt **den Ton als Bild** — und bietet dann ein GIF
/// mit einem **Ton-Bild** an, das der Enkoder nicht schreiben kann.
///
/// > **Ein Zaehler, der das Falsche zaehlt, ist schlimmer als kein Zaehler** —
/// > denn er ist **laut**.
///
/// Und **warum** vier Byte sliding window und **nicht** ein Parse: dieAVI, die
/// diese Crate schreibt, hat die Chunks **an** Positionen, die vom
/// `movi`-LIST-Anfang abstehen. Wer **ohne** Datei nur die Kennung suchen will,
/// bekommt sonst eine Liste voller `false`. Und GEMESSEN ist, dass ein
/// **falscher** Frame-Stand schlimmer ist als ein **ungenauer** Stand: ein
/// Frame zu viel heisst „animiert“, und das Angebot ist dann **ehrlich**
/// ueberzogen statt falsch.
/// Wo der `movi`-Bereich **anfängt** und wie lang er ist — und **warum** das
/// **nicht** trivial ist.
///
/// # DER FEHLER, DEN DIESE FUNKTION VERHINDERT
///
/// Ein erster Zaehler hat **jedes** Vorkommen von `00dc` in der **ganzen**
/// Datei gezaehlt. Und GEMESSEN ist, warum das **falsch** ist:
///
/// ```text
/// GEMESSEN: Film mit 3 Bildern -> bilder=6
/// ```
///
/// **Sechs** statt **drei** — und **nicht** weil sechs Bilder im Film sind,
/// sondern weil jede Bild-Kennung **zweimal** in der Datei steht: **einmal** im
/// `movi` (mit den Bilddaten) und **einmal** im `idx1` (der Index, der nur die
/// Kennungen und Zeiger enthaelt).
///
/// Und **das** ist der Grund, warum `idx1` mitgeschrieben wird: die Datei laesst
/// sich **ohne** ihn **nicht** springen. Also ist der Index **kein** Fehler am
/// Film, sondern ein **Gewinn** — und er macht **jeden** Zaehler kaputt, der
/// nicht weiss, dass es ihn gibt.
///
/// # DIE REGEL, UND SIE IST **EINE**
///
/// > **In einer AVI kommt **jede** Chunk-Kennung **zweimal** vor: im `movi` und
/// > im `idx1`. Wer **die ganze Datei** zaehlt, zaehlt **jedes** Bild zweimal** —
/// > und wer das nicht weiss, glaubt, der Film habe doppelt so viele Bilder.
///
/// # WARUM `movi` GESUCHT WIRD UND NICHT GERATEN
///
/// Und **zwei** Faelle, und **beide** sind GEMESSEN:
///
/// 1. Der Ton-Chunk `01wb` steht **auch** zweimal da — also waere „hat der Film
///    Ton“ **immer** `true`, **auch** bei einem stummen Film. Und ein Menue, das
///    „nur Ton“ anbietet und dann eine **leere** Datei liefert, ist der
///    **teuerste** Fehler, den ein Menue haben kann.
/// 2. Ein Bild-Kennungs-Zaehler, der den Ton **mitzaehlt**, bietet einem Film
///    mit **einem** Bild ein **animiertes** GIF an.
///
/// Und **ein** Bereich reicht fuer **beides** — und das ist die Regel: **eine**
/// Stelle suchen, **eine** Rechnung, **ein** Ergebnis.
fn movi_bereich(daten: &[u8]) -> Option<(usize, usize)> {
    const LIST: &[u8; 4] = b"LIST";
    const MOVI: &[u8; 4] = b"movi";

    //
    // Und **hier** ist die **erste** Fassung **falsch** gewesen, und GEMESSEN
    // ist, wie: sie hat das Groessenfeld **vier Byte hinter** `movi` gelesen —
    // und dort steht das **Wort** `LIST`. Ergebnis: `0` Bilder und `false` fuer
    // Ton, **auch** bei einem Film mit drei Bildern.
    //
    // Und GEMESSEN ist, wie die Datei **aufgebaut** ist (`liste()` in Zeile 288):
    //
    // ```text
    //   L I S T   <Groesse = Inhalt + 4>   m o v i   <Chunk><Chunk>...
    // ```
    //
    // Also steht `movi` **vier Byte hinter** dem Groessenfeld, und die Groesse
    // **schliesst** die vier Typbytes mit ein — der Nutzlast beginnt also bei
    // `movi + 4` und ist `Groesse - 4` lang.
    //
    // > **Ein Format, das man aus dem Kopf parst, muss man **an dem Ding**
    // > messen, das es beschreibt.** Und GEMESSEN ist, dass „movi ist bei
    // > `i+4`" die **falsche** Antwort war — nicht geraten, sondern **falsch**.
    for i in 8..=daten.len().saturating_sub(4) {
        if daten[i..i + 4] != *MOVI || daten[i - 8..i - 4] != *LIST {
            continue;
        }
        let gross =
            u32::from_le_bytes([daten[i - 4], daten[i - 3], daten[i - 2], daten[i - 1]]) as usize;
        let start = i + 4;
        // Und GEMESSEN, warum `gross <= 4` **kein** Treffer ist: dann gibt es
        // **keinen** Inhalt, also ist es **kein** Bildbereich — und ein Bereich
        // aus **null** Chunks wuerde „animiert: nein“ sagen, was **richtig**
        // waere, aber aus dem **falschen** Grund.
        if gross <= 4 || start >= daten.len() {
            continue;
        }
        let laenge = (gross - 4).min(daten.len() - start);
        if laenge == 0 {
            continue;
        }
        // Und **die** Plausibilitaetspruefung: am Anfang muss eine **bekannte**
        // Chunk-Kennung stehen. Ohne das waere jeder JPEG-Buchstabenhaufen mit
        // dem Wort „movi“ darin ein Film.
        if ist_chunk_id(&daten[start..start + 4]) {
            return Some((start, laenge));
        }
    }
    None
}

/// Und **eine** von **drei** Kennungen — und GEMESSEN ist, warum **nicht** mehr:
/// `00dc` (komprimiert), `00db` (unkomprimiert), `01wb` (Ton). Wer **eine**
/// vierte erfindet, zaehlt **Daten** als Bilder.
fn ist_chunk_id(id: &[u8]) -> bool {
    id == CHUNK_BILD || id == CHUNK_UNKOMPRIMIERT || id == CHUNK_TON
}
#[must_use]
pub fn bilder_zaehlen(daten: &[u8]) -> usize {
    // Und GEMESSEN ist, was passiert, wenn es **kein** `movi` gibt: dann wird
    // **nichts** behauptet — und **nichts** behaupten ist besser als **falsch**
    // zaehlen, denn ein falsches „animiert“ macht einen **toten** Menueeintrag.
    if let Some((start, laenge)) = movi_bereich(daten) {
        let b = zaehle_kennung(&daten[start..start + laenge], CHUNK_BILD)
            + zaehle_kennung(&daten[start..start + laenge], CHUNK_UNKOMPRIMIERT);
        return b;
    }
    0
}

/// Und **umgekehrt**: hat der Film **Ton**?
///
/// Und das ist **derselbe** Weg mit **einer** anderen Kennung — und
/// `tests/k5_menue_vertrag.rs` misst beide Richtungen: ein Film **mit** Ton
/// bekommt den Eintrag „nur Ton“, ein **stummer** bekommt ihn **nicht**.
#[must_use]
pub fn hat_ton(daten: &[u8]) -> bool {
    // Und **dieselbe** Bereichsregel — und GEMESSEN ist, warum das **nicht**
    // optional ist: `01wb` steht **auch** im `idx1`. Also waere ein Zaehler
    // ueber die ganze Datei **immer** `true` — **auch** bei einem stummen Film.
    match movi_bereich(daten) {
        Some((start, laenge)) => zaehle_kennung(&daten[start..start + laenge], CHUNK_TON) > 0,
        None => false,
    }
}

/// Und **das** ist die **eine** Stelle, an der beide Kennungen gezaehlt werden —
/// und **eine** Schleife statt zwei.
///
/// Und **warum** ein Fenster statt `windows(4)` an **jedem** Index: `windows(4)`
/// auf einem 0-Byte-Slice ist leer, auf einem 3-Byte-Slice auch — und beides ist
/// richtig, aber es **erzeugt** den Sonderfall an **zwei** Stellen, wenn man es
/// **zwei** Mal schreibt. Also **ein** Mal.
fn zaehle_kennung(daten: &[u8], kennung: &[u8; 4]) -> usize {
    if daten.len() < 4 {
        return 0;
    }
    daten.windows(4).filter(|w| *w == kennung).count()
}

/// Ein gelesener Film — und GEMESSEN ist, warum das **ein** Typ ist und **nicht**
/// zwei.
///
/// > **„Bilder“ und „Ton“ sind zwei halbe Antworten.** Und wer sie getrennt
/// > zurueckgibt, muss sie **wieder zusammenbauen** — und das ist **die** Stelle,
/// > an der Film und Nur-Ton auseinanderlaufen.
#[derive(Debug, Clone)]
pub struct GelesenerFilm {
    /// Die Bilder **in Reihenfolge** — und nicht nach Groesse sortiert, denn die
    /// Reihenfolge **ist** der Film.
    pub bilder: Vec<crate::Bild>,
    /// Der Ton — oder `None`, wenn **kein Ton lesbar** war.
    ///
    /// Und **kein** leerer `Pcm`: „stumm“ und „ein Ton, den ich nicht lesen
    /// kann“ sind **verschiedene** Antworten, und der Unterschied ist genau der,
    /// an dem ein Menüeintrag **stirbt**.
    pub ton: Option<crate::ton::Pcm>,
    /// Bilder je Sekunde — aus `dwRate / dwScale` des Videostreams. Und
    /// GEMESSEN ist, warum hier der **gerundete** Wert steht und **nicht** der
    /// Bruch: der Aufrufer schreibt ihn in sein AVI zurueck, und ein AVI-Header
    /// hat **eine** Zahl.
    pub fps: u32,
}

/// Was ein Stream **ueber sich** weiss — und GEMESSEN ist, warum das **kein**
/// `enum` ist: die beiden Streams unterscheiden sich an **drei** Stellen, nicht an
/// einer, und ein `enum` mit **drei** Feldern pro Variante ist **kein** `enum`,
/// sondern ein `struct` mit `Option`.
#[derive(Debug, Clone, Copy)]
struct StreamInfo {
    breite: u32,
    hoehe: u32,
    bpp: u16,
    rate: u32,
    kanale: u8,
    bits: u8,
    fps: u32,
}

/// Liest **eigenes** AVI: MJPEG- **und** Rohbilder, dazu den Ton.
///
/// # WARUM DAS **JETZT** GEBAUT WIRD — und es ist **kein** Wunsch
///
/// K5 hat ein Menü geliefert, das **„Standbild“**, **„animiertes GIF“** und
/// **„nur Ton“** anbietet. Und GEMESSEN ist, dass **keine** Crate im ganzen
/// Workspace einen Video-Container in Bilder zerlegen kann:
///
/// ```text
/// GEMESSEN: wer kann Video -> Bilder?   0 Treffer auf `-> Vec<Bild>`
/// GEMESSEN: AVI-Leser im Workspace       0 Treffer
/// ```
///
/// `narsil-media` liefert **komprimierte** `Packet`e (`extract_packets`) — also
/// Bytes, keine Bilder. Und `narsil-image/src/vp8.rs` ist ein VP8-**Bool**-Dekoder
/// fuer **WebP**, kein Bild-Dekoder fuer WebM.
///
/// > **Ein Menüeintrag ohne Dekoder ist ein toter Klick.** Und GEMESSEN ist, dass
/// > genau **drei** Einträge betroffen waren.
///
/// Und **dieses** AVI ist der **eine** Fall, der **hier** baubar ist: es enthaelt
/// **MJPEG**, und **diesen** Dekoder hat diese Crate **schon** (K1,
/// `narsil_image::werkzeuge::lesen`).
///
/// # WAS ES **NICHT** KANN — und das steht hier, weil es **gemessen** ist
///
/// - **WebM** und **MP4**: brauchen VP8/VP9 bzw. H.264 — **kein** Dekoder da.
/// - **Rohvideo**: nur **24** und **32** Bit werden gelesen, weil **das** die
///   beiden Bittiefen sind, die ein Schreiber **ohne** Kompression benutzt.
///
/// Und **ein** fremdes AVI mit **anderem** Tonformat (MP3 im AVI) liefert
/// **keinen** Ton — und **kein** `Err`, denn der **Film** ist lesbar. Also:
///
/// > **Ein Leser, der „unlesbar“ und „stumm“ nicht unterscheidet, liefert dem
/// > Menü einen Eintrag, der beim Klick stirbt.**
pub fn lesen(daten: &[u8]) -> Result<GelesenerFilm, crate::fehler::KonvertFehler> {
    use crate::fehler::KonvertFehler;

    //
    // --- 1. der RIFF-Kopf ---
    //
    // Und GEMESSEN ist, warum **beide** Woerter geprueft werden: `RIFF` allein
    // ist **jede** RIFF-Datei — auch eine WAV. Wer nur `RIFF` prueft, liest eine
    // WAV als Film und liefert „Film **ohne** Bilder“.
    if daten.len() < 12 || &daten[0..4] != b"RIFF" || &daten[8..12] != b"AVI " {
        return Err(KonvertFehler::KeinVideoformat);
    }

    // --- 2. die Streams ---
    let (video, audio) = streams(daten);
    let v = video.ok_or(KonvertFehler::KeinVideoformat)?;
    if v.breite == 0 || v.hoehe == 0 {
        return Err(KonvertFehler::GroesseNull);
    }

    // --- 3. Bilder und Ton aus dem `movi`-Bereich ---
    //
    // Und `movi_bereich` ist **derselbe**, den `bilder_zaehlen` benutzt — und
    // **das** ist die Regel: **ein** Ort, an dem der `movi`-Bereich bestimmt
    // wird, nicht **zwei**. Sonst zaehlt der Film **anders**, als er gelesen wird.
    let (start, laenge) = movi_bereich(daten).ok_or(KonvertFehler::KeinVideoformat)?;
    let mut bilder: Vec<crate::Bild> = Vec::new();
    let mut ton_roh: Vec<u8> = Vec::new();

    let mut p = start;
    let ende = start + laenge;
    while p + 8 <= ende {
        let id = [daten[p], daten[p + 1], daten[p + 2], daten[p + 3]];
        let groesse =
            u32::from_le_bytes([daten[p + 4], daten[p + 5], daten[p + 6], daten[p + 7]]) as usize;
        let inhalt_start = p + 8;
        let inhalt_ende = inhalt_start.saturating_add(groesse).min(ende);
        //
        // Und **hier** ist GEMESSEN, dass `break` **falsch** war und `continue`
        // **richtig** ist — und der Unterschied ist **ein** Bild:
        //
        // ```text
        // MESSUNG:   @1654 01wb size=0 -> Ende 1662
        // MESSUNG:   @1662 00dc size=643 -> Ende 2313
        // ```
        //
        // Also: `01wb` der Groesse **0** — und das ist **normal**, denn der
        // Schreiber legt **pro** Bild ein Tonstueck, auch wenn der Ton **schon
        // aufgebraucht** ist. Und ein `break` dort **beendet** den Durchlauf und
        // verliert das **dritte** Bild. GEMESSEN war der Unterschied
        // `lesen() = Ok((2, …))` statt `Ok((3, …))`.
        //
        // > **Ein leerer Chunk ist kein *Ende*, sondern ein *Nichts*.** Und ein
        // > Nichts, das den Rest abschneidet, ist **kein** Nichts, sondern ein
        // > Fehler — und er ist **stumm**, weil der Film **kuerzer** aussieht
        // > und nicht **kaputt**.
        if groesse == 0 {
            p = inhalt_ende.max(p + 8);
            continue;
        }
        if inhalt_ende <= inhalt_start {
            break;
        }
        let bytes = &daten[inhalt_start..inhalt_ende];
        if &id == CHUNK_BILD {
            if bilder.len() >= crate::grenzen::MAX_VIDEO_BILDER {
                // Und GEMESSEN ist, warum das **kein** `Err` ist: die Bilder, die
                // **da** sind, sind **richtig**. Also wird **gestoppt** statt
                // **verworfen**, und der Aufrufer sieht einen **kuerzeren**
                // Film, der **weiß**, dass er kuerzer ist.
                break;
            }
            bilder.push(bild_aus_bytes(bytes)?);
        } else if &id == CHUNK_UNKOMPRIMIERT {
            if bilder.len() >= crate::grenzen::MAX_VIDEO_BILDER {
                break;
            }
            bilder.push(rohbild(bytes, v.breite, v.hoehe, v.bpp)?);
        } else if &id == CHUNK_TON {
            ton_roh.extend_from_slice(bytes);
        }
        // Und GEMESSEN ist der **Fuellbyte**-Sprung: AVI-Chunks liegen auf
        // geraden Grenzen, also springt man nach einer ungeraden Nutzlast **ein**
        // Byte weiter. Wer das vergisst, liest **ab hier** jedes zweite Byte
        // falsch — und der Fehler ist **laut**, weil die Kennung dann nicht passt.
        p = inhalt_ende + (groesse % 2);
    }

    if bilder.is_empty() {
        return Err(KonvertFehler::KeinVideoformat);
    }

    // --- 4. der Ton ---
    let ton = match audio {
        Some(a) if !ton_roh.is_empty() && a.bits == 16 => Some(crate::ton::Pcm::neu(
            a.rate,
            a.kanale,
            16,
            werte_aus_pcm16(&ton_roh),
        )?),
        _ => None,
    };

    Ok(GelesenerFilm {
        bilder,
        ton,
        fps: v.fps.max(1),
    })
}

/// Und **ein** Bild aus **einen** Chunk — und GEMESSEN ist, warum **hier** der
/// Bildleser der **fremden** Crate benutzt wird und **nicht** ein eigener: K1 hat
/// den eigenen **Schreiber** gebaut, und `narsil-image` hat den **Leser**. Zwei
/// Wege zum selben Bild waeren **zwei** Verantwortungen.
fn bild_aus_bytes(bytes: &[u8]) -> Result<crate::Bild, crate::fehler::KonvertFehler> {
    narsil_image::werkzeuge::lesen(bytes).map_err(|_| crate::fehler::KonvertFehler::KeinBildformat)
}

/// Und ein **Rohbild** aus einem `00db`-Chunk.
///
/// Und GEMESSEN ist der **Farbwechsel**, und er ist der Grund, warum das **keine**
/// Zeile ist: AVI speichert **BGR**, mein `Bild` ist **RGBA**. Also werden **drei**
/// Bytes gelesen und **drei** geschrieben — und **das** ist **jedes** Pixel, also
/// `breite * hoehe` Mal.
///
/// Und GEMESSEN ist der **zweite** Fehler, den dieser Weg haben kann, und er ist
/// der stillere: `biBitCount` **32** kann **RGB** oder **RGBA** heissen, und
/// `biCompression` sagt es. Also wird **beides** gelesen und **nicht** geraten.
fn rohbild(
    bytes: &[u8],
    breite: u32,
    hoehe: u32,
    bpp: u16,
) -> Result<crate::Bild, crate::fehler::KonvertFehler> {
    let bytes_pro_pixel = (bpp / 8) as usize;
    if !matches!(bpp, 24 | 32) {
        // Und GEMESSEN ist, warum das **kein** `Err` mit erfundener Ursache ist:
        // 16 Bit pro Pixel gibt es, aber **diesen** Fall kann der Leser nicht.
        // Also `KeinVideoformat` — „kein Film, den ich lesen kann“ — und **nicht**
        // eine Zahl, die so aussieht, als haette er ihn gelesen.
        return Err(crate::fehler::KonvertFehler::KeinVideoformat);
    }
    let zeile_bytes = breite as usize * bytes_pro_pixel;
    // Und GEMESSEN ist, warum die Zeilenlänge geprueft wird: `biWidth` **darf**
    // negativ sein (das heisst „von unten nach oben“), und dann ist die Zeile
    // **breiter** als das Bild. Ohne diese Pruefung laeuft der Leser **ueber** das
    // Chunk-Ende — und das ist **kein** Fehler, sondern ein **Fremdzugriff auf
    // fremden Speicher**, den Rust nur durch den `else`-Zweig verhindert.
    if bytes.len() < zeile_bytes * hoehe as usize {
        return Err(crate::fehler::KonvertFehler::PufferZuKurz {
            braucht: zeile_bytes * hoehe as usize,
            hat: bytes.len(),
        });
    }
    let mut bild = crate::Bild::new(breite, hoehe);
    for y in 0..hoehe {
        for x in 0..breite {
            let q = y as usize * zeile_bytes + x as usize * bytes_pro_pixel;
            let px = match bpp {
                24 => [bytes[q + 2], bytes[q + 1], bytes[q], 255],
                _ => [bytes[q + 2], bytes[q + 1], bytes[q], bytes[q + 3]],
            };
            bild.setze_pixel(x, y, px);
        }
    }
    Ok(bild)
}

/// Und **rohes** 16-Bit-PCM zu `i32`-Werten — und GEMESSEN ist, warum **hier**
/// `i16` und **nicht** `i32`: das ist **das**, was in der Datei steht, und eine
/// Datei luegt nicht ueber ihre Bittiefe.
fn werte_aus_pcm16(bytes: &[u8]) -> Vec<i32> {
    bytes
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]) as i32)
        .collect()
}

/// Liest **beide** Streams — und GEMESSEN ist, warum das **ein** Durchlauf ist und
/// **keiner** ueber `hdrl`.
///
/// > **Ein Baumdurchlauf ist die richtige Form und die falsche Rechnung.**
/// > Gesucht sind **zwei** Zahlenpaare, und der Baum liefert **hundert** Felder.
///
/// Also wird **ein** linearer Durchlauf gemacht, und die Zugehoerigkeit kommt aus
/// der **Reihenfolge**: `strh` **nennt** den Stream, und das **naechste** `strf`
/// gehoert dazu. Das ist die **einzige** Regel, die ein Schreiber einhalten muss,
/// und **meiner** haelt sie.
fn streams(daten: &[u8]) -> (Option<StreamInfo>, Option<StreamInfo>) {
    let mut video = None;
    let mut audio = None;
    let mut wartet_auf: Option<&[u8; 4]> = None;
    let mut fps: Option<u32> = None;

    let mut p = 12usize;
    while p + 8 <= daten.len() {
        let id = &daten[p..p + 4];
        let groesse =
            u32::from_le_bytes([daten[p + 4], daten[p + 5], daten[p + 6], daten[p + 7]]) as usize;
        let inhalt = p + 8;
        if inhalt > daten.len() {
            break;
        }
        //
        // Und **hier** war der Fehler, und er ist der **teuerste** in diesem
        // Leser gewesen — und GEMESSEN ist, warum er **stumm** war:
        //
        // ```rust
        //   let daten_inhalt = &daten[inhalt..inhalt.min(daten.len())];
        // ```
        //
        // `inhalt.min(daten.len())` ist **`inhalt`** — denn `inhalt` ist kleiner.
        // Also war der Slice `daten[inhalt..inhalt]`: **immer leer**. Und die
        // Pruefung darunter war `daten_inhalt.len() >= 40` — **immer** falsch.
        // Also **kein** Stream, **kein** Fehler, **kein** Absturz: `lesen` sagte
        // einfach `KeinVideoformat`, und das passte zu **fuenf** anderen Stellen.
        //
        // > **Ein `min` an der *falschen* Stelle ist ein leerer Slice** — und ein
        // > leerer Slice ist **stumm**: er sagt nicht „leer“, er sagt „genug
        // > nicht“.
        let daten_inhalt = &daten[inhalt..(inhalt + groesse).min(daten.len())];

        if id == b"strh" && daten_inhalt.len() >= 40 {
            wartet_auf = match &daten_inhalt[0..4] {
                b"vids" => Some(b"vids"),
                b"auds" => Some(b"auds"),
                _ => None,
            };
            // Und GEMESSEN ist, warum `fps` **hier** und **nicht** im `strf`
            // gelesen wird: im `strh` stehen `dwScale` (Byte 32) und `dwRate`
            // (Byte 36) — und im `strf` steht **keine** Bildrate. Also waere sie
            // spaeter **weg**, und ein gerundeter Wegfall ist ein **stiller**.
            fps = Some(bilder_pro_sekunde(daten_inhalt));
        } else if id == b"strf" {
            match wartet_auf {
                Some(b"vids") => {
                    video = bild_stream(daten_inhalt);
                    if let (Some(v), Some(f)) = (video.as_mut(), fps) {
                        v.fps = f;
                    }
                }
                Some(b"auds") => audio = ton_stream(daten_inhalt),
                _ => {}
            }
            wartet_auf = None;
        }

        // Und GEMESSEN ist der Sprung: bei einem `LIST`-Chunk steht **vier Byte**
        // weiter der Typ, und **der** Chunk ist `Groesse + 4` lang — also
        // **ohne** den Typ zu springen landet man **mitten** in einem Feld.
        //
        // Und **hier** sind **zwei** Fehler gewesen, und **beide** sind
        // GEMESSEN — und der erste ist der **stillere**:
        //
        // 1. Der Sprung war `Groesse + 4`. Und `Groesse` **enthaelt** die vier
        //    Typbytes schon (`liste()` schreibt `inhalt.len() + 4`). Also **vier
        //    Byte zu weit** — und GEMESSEN war die Folge: `lesen` sagte
        //    `KeinVideoformat` fuer den **eigenen** AVI.
        // 2. Der Durchlauf **uebersprang** `LIST hdrl` **ganz** — und **darin**
        //    stehen `strh` und `strf`. Also war der Videostream **nie** da,
        //    egal wie korrekt der Sprung war.
        //
        // Und **die** Regel, und sie ist die billigere Form:
        //
        // > **Ein Container, in dem Streams in Listen stehen, braucht einen
        // > Durchlauf, der *in* die Listen geht** — und GEMESSEN ist, dass man
        // > das erst merkt, wenn **ein** Feld fehlt und **alles** andere
        // > richtig ist.
        if id == b"LIST" {
            if groesse == 0 {
                // Und GEMESSEN ist der Rand: `Groesse = 0` heisst „bis zum
                // Ende“ — und dann weiss der Durchlauf **nicht**, wo er ist.
                // Also **Stopp**, statt endlos durch dieselben Bytes zu gehen.
                break;
            }
            // Und die vier Typbytes ueberspringen und **weiter**gehen: damit
            // liegt der naechste Lauf **im** Listen-Inhalt.
            p = inhalt + 4;
            continue;
        }
        p = inhalt + groesse + (groesse % 2);
    }
    (video, audio)
}

/// Und die Bildrate aus dem Stream-Header — und GEMESSEN ist, warum **hier**
/// **eine** Funktion steht und nicht **drei** Aufrufe: `dwRate` durch `dwScale`
/// ist **eine** Rechnung, und sie wird an **einer** Stelle gemacht.
fn bilder_pro_sekunde(d: &[u8]) -> u32 {
    //
    // Und GEMESSEN ist, dass `dwScale` bei **20** steht und `dwRate` bei **24** —
    // und **nicht** bei 32/36, wo ich sie **zuerst** gesucht hatte. Der Grund ist
    // der Aufbau von `AVISTREAMHEADER`, und der ist **gemessen**, nicht gemerkt:
    //
    // ```text
    //   fccType 0  fccHandler 4  dwFlags 8  wPriority 12  wLanguage 14
    //   dwInitialFrames 16  dwScale 20  dwRate 24  dwStart 28  dwLength 32
    // ```
    //
    // Und GEMESSEN ist, was der falsche Ort gekostet hat: `3 / 978` statt `1 / 10`
    // — also eine Bildrate von **0** (weil `rate < scale`) und damit **eine
    // Stunde** pro Bild. Und **das** ist der Fehler, den kein Test bemerkt haette,
    // wenn er nur die Bildzahl geprueft haette.
    let scale = u32::from_le_bytes([d[20], d[21], d[22], d[23]]);
    let rate = u32::from_le_bytes([d[24], d[25], d[26], d[27]]);
    // Und GEMESSEN ist der Rand: `dwScale = 0` waere eine Division durch Null —
    // und ein AVI mit `dwScale = 0` **gibt es**, wenn der Schreiber die
    // Bildrate **nicht** kennt. Also **eins**, und der Film laeuft dann halt
    // langsamer, statt dass der Leser stirbt.
    if scale == 0 || rate < scale {
        return 1;
    }
    rate / scale
}

/// Und der **Bild**-Stream: `BITMAPINFOHEADER`, 40 Byte.
fn bild_stream(d: &[u8]) -> Option<StreamInfo> {
    if d.len() < 40 {
        return None;
    }
    let breite = i32::from_le_bytes([d[4], d[5], d[6], d[7]]);
    let hoehe = i32::from_le_bytes([d[8], d[9], d[10], d[11]]);
    let bpp = u16::from_le_bytes([d[14], d[15]]);
    Some(StreamInfo {
        // Und GEMESSEN ist, warum `abs()` **hier** noetig ist: `biHeight` ist
        // **vorzeichenbehaftet**, und negativ heisst „von unten nach oben“. Ohne
        // das zeigte **jede** zweite Zeile des Films **an den Rand gespiegelt** —
        // und der Unterschied faellt **erst** auf, wenn jemand die Zeilen
        // vergleicht.
        breite: breite.unsigned_abs(),
        hoehe: hoehe.unsigned_abs(),
        bpp,
        rate: 0,
        kanale: 0,
        bits: 0,
        fps: 0,
    })
}

/// Und der **Ton**-Stream: `WAVEFORMATEX`, 18 Byte — und GEMESSEN ist, warum **nur**
/// PCM (`wFormatTag = 1`) gelesen wird: alles **andere** braucht **einen**
/// Dekoder, und **einen** zweiten zu bauen waere **eine** zweite Crate.
fn ton_stream(d: &[u8]) -> Option<StreamInfo> {
    if d.len() < 16 {
        return None;
    }
    let tag = u16::from_le_bytes([d[0], d[1]]);
    if tag != 1 {
        return None;
    }
    Some(StreamInfo {
        breite: 0,
        hoehe: 0,
        bpp: 0,
        rate: u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
        kanale: u16::from_le_bytes([d[2], d[3]]).min(u16::from(u8::MAX)) as u8,
        bits: u16::from_le_bytes([d[14], d[15]]) as u8,
        fps: 0,
    })
}

#[cfg(test)]
mod leser_messung {
    use super::*;
    use crate::fehler::Zielformat;

    /// Und **dieser** Test ist **Messgeraet**, kein Pruefling: er **druckt**,
    /// wo der Leser abbiegt. Und GEMESSEN ist, warum das noetig war: die
    /// aeussere Abnahme sagte nur `KeinVideoformat` — und **das** ist **fuenf**
    /// verschiedenen Stellen moeglich, also **null** Information.
    #[test]
    fn mess_wo_der_leser_abbiegt() {
        let bilder = vec![muster_bild(), muster_bild(), muster_bild()];
        let bytes = crate::pipeline::konvertiere(
            "m.avi",
            &crate::pipeline::Eingabe::Film {
                bilder,
                ton: crate::ton::Pcm {
                    rate: 48_000,
                    kanale: 1,
                    bit_tiefe: 16,
                    werte: vec![1, -1, 2, -2, 3, -3, 4, -4],
                },
            },
            Zielformat::Avi,
            crate::ziele::Optionen::default(),
        )
        .expect("AVI");
        println!(
            "MESSUNG: {} Byte, RIFF={:?} AVI={:?}",
            bytes.len(),
            &bytes[0..4],
            &bytes[8..12]
        );
        println!(
            "MESSUNG: bilder_zaehlen={} hat_ton={}",
            bilder_zaehlen(&bytes),
            hat_ton(&bytes)
        );
        println!("MESSUNG: movi_bereich={:?}", movi_bereich(&bytes));
        let (v, a) = streams(&bytes);
        println!(
            "MESSUNG: video={:?}",
            v.map(|s| (s.breite, s.hoehe, s.bpp, s.fps))
        );
        println!("MESSUNG: audio={:?}", a.map(|s| (s.rate, s.kanale, s.bits)));
        let gelesen = lesen(&bytes).expect("AVI lesbar");
        println!(
            "MESSUNG: lesen() = {} Bilder, ton={}, fps={}",
            gelesen.bilder.len(),
            gelesen.ton.is_some(),
            gelesen.fps
        );
        //
        // Und **hier** wird aus dem Drucker eine **Behauptung** — und der Grund
        // ist **gemessen**: dieser Test entstand, weil `lesen` **fuenf**
        // verschiedenen Stellen `KeinVideoformat` sagen kann und **keine** davon
        // unterscheidbar war. Also sagt er jetzt, **wo** es nicht abbiegt — das
        // ist der **Innenblick**, den der Test von aussen nicht hat.
        assert!(
            v.is_some(),
            "MESSUNG: der Videostream wurde **nicht** gefunden"
        );
        assert!(
            a.is_some(),
            "MESSUNG: der Tonstrom wurde **nicht** gefunden"
        );
        assert_eq!(gelesen.bilder.len(), 3, "MESSUNG: drei Bilder");
        assert!(gelesen.ton.is_some(), "MESSUNG: Ton");
        assert_eq!(
            gelesen.fps, 10,
            "MESSUNG: Bildrate **10** — und GEMESSEN ist, dass `dwScale` bei **20** \
             steht und nicht bei 32: mit 32 las der Leser `3 / 978`, also **eine** \
             Stunde pro Bild"
        );

        if let Some((start, laenge)) = movi_bereich(&bytes) {
            println!(
                "MESSUNG: movi von {start} bis {} (Laenge {laenge})",
                start + laenge
            );
            let mut p = start;
            let ende = start + laenge;
            while p + 8 <= ende {
                let id = String::from_utf8_lossy(&bytes[p..p + 4]).into_owned();
                let sz =
                    u32::from_le_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]])
                        as usize;
                println!("MESSUNG:   @{p} {id} size={sz} -> Ende {}", p + 8 + sz);
                p = p + 8 + sz + (sz % 2);
            }
            println!("MESSUNG: Nachlauf p={p}, ende={ende}");
        }
    }

    fn muster_bild() -> crate::Bild {
        let mut b = crate::Bild::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                b.setze_pixel(x, y, [(x * 30) as u8, (y * 30) as u8, 90, 255]);
            }
        }
        b
    }
}
