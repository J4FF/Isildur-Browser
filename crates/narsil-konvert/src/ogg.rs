//! KONVERT_12 — **Ogg-Muxer ueber FLAC**: die **einzige** baubare Ogg-Ausgabe.
//!
//! # WARUM OGG-FLAC und nicht OGG-VORBIS
//!
//! GEMESSEN (KONVERT_8 Teil 1): ein **Vorbis-Paketschreiber** ist **kein**
//! Containerproblem, sondern ein **Codec**: `narsil-audio/src/vorbis.rs` Z. 119
//! `Setup { codebucher, boden, reste, … }` — die Codebuecher muessen **gewahlt**
//! werden. Das ist derselbe Aufwand wie der geparkte MP3-Encoder.
//!
//! **FLAC** ist **verlustfrei**, und diese Crate hat ihn seit K2. Es fehlte nur
//! der **Container** — und der ist eine reine **Format**frage ohne Mathematik
//! ausser einer CRC.
//!
//! # UND: das Ergebnis ist **pruefbar**, nicht behauptet
//!
//! GEMESSEN: `narsil-audio/src/ogg_flac.rs` Z. 98 hat `ist_ogg_flac` und Z. 246 ein
//! `dekodiere`. Also gibt es im Projekt **schon** einen Leser fuer genau das, was
//! hier entsteht — und ein geschlossener Rundlauf durch **zwei** Crates ist mehr
//! wert als eine Datei, die `file` erkennt.
//!
//! # DIE DREI SACHEN, DIE EIN OGG-CONTAINER BRAUCHT
//!
//! 1. **Der Seitenkopf**: `"OggS"`, Version 0, Kopf-Typ (`0x02` BOS, `0x04` EOS),
//!    **Granelposition**, Seriennummer, Seitennummer, CRC32, Lacing-Tabelle.
//! 2. **Die CRC32** mit dem Polynom `0x04c11db7`, **nicht** gespiegelt, Startwert
//!    `0`, **kein** End-XOR. GEMESSEN ist das der Unterschied zu der CRC32 in
//!    `ziele.rs`: eine **gespiegelte** CRC sieht gleich aus und ergibt andere
//!    Bytes — und eine falsche Pruefsumme wird von manchen Lesern **ignoriert**.
//! 3. **Das Lacing**: ein Paket der Laenge `L` braucht `⌈(L+1)/255⌉` Lacing-Werte,
//!    und **ein** abschliessender Wert **kleiner** als 255. GEMESSEN ist der
//!    Sonderfall: bei `L % 255 == 0` muss ein **0**-Segment **angehaengt**
//!    werden, sonst frisst der Leser das **naechste** Paket mit.

use crate::fehler::KonvertFehler;
use crate::ton::Pcm;

/// Und **wer** die Datei geschrieben hat — GEMESSEN als der Grund, warum der
/// zweite Kopfpaket **kein** Muell ist: `ffmpeg` schreibt dort
/// `Lavf61.7.103`, und ein Ogg **ohne** Hersteller behauptet gar nichts.
pub const HERSTELLER: &str = "narsil-konvert 0.1.0";

/// Die **Ogg-Seiten-Nutzdaten** eines Segments — GEMESSEN: 255 Werte mal 255
/// Byte.
pub const SEITE_MAX: usize = 255 * 255;

/// Und die **CRC32-Tabelle** fuer das Polynom `0x04c11db7`.
///
/// GEMESSEN und **nicht geraten**: es ist **kein** CRC32 wie in `ziele.rs`. Das
/// Polynom ist **gleich**, aber **nicht** gespiegelt (`0xedb88320` ist die
/// gespiegelte Form), es wird **MSB-zuerst** geschoben, der Startwert ist `0`
/// und es gibt **kein** End-XOR.
///
/// Und GEMESSEN ist auch, warum das **auffällt**: eine falsche Pruefsumme
/// entdeckt der Leser **nicht**. `ffmpeg` rechnet sie nach und meldet
/// „page checksum mismatch“ — aber nur, wenn es die Seite **prueft**.
fn crc_tabelle() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut r = (i as u32) << 24;
        let mut k = 0;
        while k < 8 {
            r = if r & 0x8000_0000 != 0 {
                (r << 1) ^ 0x04c1_1db7
            } else {
                r << 1
            };
            k += 1;
        }
        t[i] = r;
        i += 1;
    }
    t
}

/// Und die **CRC32** ueber eine **vollstaendige** Seite — mit dem CRC-Feld auf
/// **null**, weil die Pruefsumme **in** dem Feld steht, das sie prueft.
pub fn crc32(daten: &[u8]) -> u32 {
    let t = crc_tabelle();
    let mut r: u32 = 0;
    for &b in daten {
        r = (r << 8) ^ t[((r >> 24) as u8 ^ b) as usize];
    }
    r
}

/// Und **eine** Ogg-Seite.
///
/// # DIE REIHENFOLGE, und sie ist **nicht** verhandelbar
///
/// `0x4F 0x67 0x67 0x53` („OggS“) · Version · Kopf-Typ · 8 Byte Granelposition ·
/// 4 Byte Seriennummer · 4 Byte Seitennummer · 4 Byte CRC · 1 Byte Segmentzahl ·
/// Lacing-Tabelle · Nutzdaten.
///
/// Und GEMESSEN ist, wo der **Fehler** lauft, wenn man ihn macht: alle **vier**
/// Felder vor der Nutzlast sind **Feste Groessen**, und die Lacing-Tabelle steht
/// **vor** der Nutzlast, nicht danach. Wer sie hinten anhaengt, bekommt eine
/// Datei, deren Seiten **alle** gleich aussehen und **keine** lesbar ist.
fn seite(kopf_typ: u8, granel: u64, serien: u32, nummer: u32, pakete: &[&[u8]]) -> Vec<u8> {
    let mut lacing: Vec<u8> = Vec::new();
    let mut nutzlast = Vec::new();
    for p in pakete {
        let mut n = p.len();
        loop {
            if n >= 255 {
                lacing.push(255);
                n -= 255;
            } else {
                // Und GEMESSEN der Sonderfall: ist `p.len()` ein Vielfaches von
                // 255, dann endet die Schleife hier mit `0` — **kein**
                // abschliessender Wert **kleiner** als 255 waere sonst
                // vorhanden, und der Leser wuerde das **naechste** Paket mit
                // fressen.
                lacing.push(n as u8);
                break;
            }
        }
        nutzlast.extend_from_slice(p);
    }

    let mut out = Vec::with_capacity(27 + lacing.len() + nutzlast.len());
    out.extend_from_slice(b"OggS");
    out.push(0); // Version
    out.push(kopf_typ);
    out.extend_from_slice(&granel.to_le_bytes());
    out.extend_from_slice(&serien.to_le_bytes());
    out.extend_from_slice(&nummer.to_le_bytes());
    // Das CRC-Feld ist **zwei** Mal auf null: einmal beim Bauen, einmal beim
    // Rechnen. GEMESSEN ist das der Fehler, den man **einmal** macht: man
    // schreibt die Seite mit **leerem** CRC-Feld in die Datei und rechnet **nur**
    // darueber — dann stimmt die Pruefsumme **fuer die falsche Seite**.
    out.extend_from_slice(&[0u8; 4]);
    out.push(lacing.len() as u8);
    out.extend_from_slice(&lacing);
    out.extend_from_slice(&nutzlast);
    let c = crc32(&out);
    out[22..26].copy_from_slice(&c.to_le_bytes());
    out
}

/// Und **FLAC in Ogg** — der Weg, den der Auftrag gesucht hat.
///
/// # DIE ZWOETEN SEITE IST BESONDERS
///
/// Sie traegt den **Mapping-Kopf**: `0x7F` + „FLAC“ + Hauptversion + NebENversion
/// + **2 Byte** Zahl der Kopfpakete + „fLaC“ + der STREAMINFO-Metadatenblock.
///
/// GEMESSEN ist, dass dort **drei** Dinge gleichzeitig stimmen muessen: das
/// Byte `0x7F`, **nicht** `0x7E` (das ist **andere** Zuordnung), und die Zahl der
/// Kopfpakete **1** — steht dort eine andere Zahl, liest der Leser die
/// STREAMINFO als **Metadaten** und nicht als Kopf.
pub fn flac_in_ogg(pcm: &Pcm) -> Result<Vec<u8>, KonvertFehler> {
    let (si, rahmen, proben) = crate::flac::stuecke(pcm)?;

    // -- Seite 0: BOS, Mapping-Kopf + STREAMINFO --
    let mut kopf: Vec<u8> = Vec::with_capacity(9 + si.len() + 4);
    kopf.push(0x7F);
    kopf.extend_from_slice(b"FLAC");
    kopf.push(1); // Hauptversion
    kopf.push(0); // NebENversion
    kopf.extend_from_slice(&1u16.to_be_bytes()); // Zahl der Kopfpakete
    kopf.extend_from_slice(b"fLaC");
    //
    // Und **hier** war der Fehler, und er ist der **letzte** Unterschied zu dem,
    // was `ffmpeg` selbst schreibt:
    //
    // ```text
    //   GEMESSEN (ffmpeg, -c:a flac -f ogg), Seite 0, Nutzlast:
    //     33 7f 46 4c 41 43 01 00 00 01 66 4c 61 43 00 00 00 22 ...
    //                     ^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^
    //                          0x00 00 00 22  <- Metadatenblock-Kopf
    //
    //   GEMESSEN (meiner), Seite 0, Nutzlast:
    //     51 7f 46 4c 41 43 01 00 00 01 66 4c 61 43 80 00 00 22 ...
    //                     ^^^^^^^^^^^^^^^^^^^^  ^^^^^^^^
    //                          0x80 00 00 22  <- Metadatenblock-Kopf
    // ```
    //
    // Also: im **Ogg**-Mapping ist das Bit „**letzter** Metadatenblock“ **0**,
    // in einer **nativen** FLAC-Datei ist es **1**.
    //
    // > **Dasselbe Byte, zwei Bedeutungen.** Und der Unterschied ist **ein Bit**,
    // > und `file` sowie der eigene Leser von `narsil-audio` haben **beide** die
    // > Datei akzeptiert — `file` sagte „Ogg data, FLAC audio“, und
    // > `narsil-audio` las **22050** Proben **bitgleich**. Nur `ffmpeg` hat
    // > gemeckert: „Header processing failed“.
    //
    // Und **das** ist die Lektion des Tages: **zwei** Werkzeuge, die zustimmen,
    // sind **kein** Beweis. Der dritte, der zustimmt, macht es zum Beweis — und
    // das Bit, das den Unterschied macht, ist **keins**, das man **sieht**.
    kopf.push(0x00); // Typ 0 = STREAMINFO, und **nicht** „letzter Block“
    let laenge = si.len() as u32;
    kopf.push((laenge >> 16) as u8);
    kopf.push((laenge >> 8) as u8);
    kopf.push(laenge as u8);
    kopf.extend_from_slice(&si);

    // Und die **Seriennummer**: GEMESSEN ist, dass sie **beliebig** sein darf,
    // aber **nicht** `0` sinnvoll ist — 0 ist „kein Strom“. Also wird sie aus
    // dem Ton abgeleitet, damit **zwei** Dateien **nicht** dieselbe Nummer
    // tragen und beim Zusammenspielen ineinanderlaufen.
    let serien = {
        let mut h: u32 = 2166136261;
        for &b in &si {
            h ^= u32::from(b);
            h = h.wrapping_mul(16777619);
        }
        if h == 0 { 1 } else { h }
    };

    let mut out = seite(0x02, 0, serien, 0, &[&kopf]);

    // -- Seite 1: der **zweite** Kopf, ein VORBIS_COMMENT --
    //
    // Und **hier** steht der Grund fuer die Warnung
    // `[ogg] Headers mismatch for stream 0: expected 2 received 1.` GEMESSEN
    // ist sie an der Datei, die `ffmpeg` **selbst** schreibt — byteweise:
    //
    // ```text
    //   GEMESSEN (ffmpeg), 3 Seiten:
    //     Seite 0: typ=0x02 granel=0    seg=1  lacing=[51]   <- Mapping + STREAMINFO
    //     Seite 1: typ=0x00 granel=0    seg=1  lacing=[54]   <- der ZWEITE Kopf
    //     Seite 2: typ=0x04 granel=22050 seg=24 ...        <- Rahmen + EOS
    //
    //   GEMESSEN (ffmpeg), Nutzlast von Seite 1:
    //     84 00 00 32 0c 00 00 00 4c 61 76 66 36 31 2e 37 2e 31 30 33 ...
    //     ^^^^^^^^ ^^^^^^^^ ^^^^^^^^  L  a  v  f  6  1  .  7  .  1  0  3
    //     |        Laenge 0x32 = 50
    //     0x84 = „letzter Metadatenblock“ (0x80) + Typ **4** = VORBIS_COMMENT
    // ```
    //
    // Also: `ffmpeg` schreibt **zwei** Kopfpakete, und das Feld
    // `header_packets` sagt trotzdem **1**. Und `ffprobe` **warnt bei meiner
    // Datei, aber nicht bei der von `ffmpeg`** — also erwartet es den zweiten
    // Kopf **in der Datei**, nicht in der Zahl.
    //
    // Und der Block ist **kein** Muell: es ist der **Herstellerstring**, und ein
    // Ogg ohne ihn behauptet nicht, **wer** ihn geschrieben hat. Also wird er
    // geschrieben — mit **unserem** Namen, nicht mit `Lavf`.
    let mut kommentar: Vec<u8> = Vec::new();
    let hersteller = HERSTELLER.as_bytes();
    kommentar.extend_from_slice(&(hersteller.len() as u32).to_le_bytes());
    kommentar.extend_from_slice(hersteller);
    kommentar.extend_from_slice(&0u32.to_le_bytes()); // 0 Kommentare

    let mut kom_block: Vec<u8> = Vec::with_capacity(4 + kommentar.len());
    kom_block.push(0x84); // letzter Metadatenblock + Typ 4 = VORBIS_COMMENT
    // Release 08.10.2026: die Laenge eines FLAC-Metadatenblocks hat DREI
    // Byte (FLAC-Format, METADATA_BLOCK_HEADER: 1 Bit letzter, 7 Bit Typ,
    // 24 Bit Laenge). GEMESSEN vorher: hier standen VIER — der eigene
    // Dekoder (`narsil_audio::ogg_flac`) las die Laenge 0 und fand den
    // ersten Rahmen nicht („Rahmen 0 bei Byte 46: Sync 0x0705 statt
    // 0x3FFE"); jede „Als OGG"-Datei war unlesbar.
    let kl = kommentar.len() as u32;
    kom_block.push((kl >> 16) as u8);
    kom_block.push((kl >> 8) as u8);
    kom_block.push(kl as u8);
    kommentar.clear();
    kom_block.extend_from_slice(&{
        let h = HERSTELLER.as_bytes();
        let mut v = Vec::new();
        v.extend_from_slice(&(h.len() as u32).to_le_bytes());
        v.extend_from_slice(h);
        v.extend_from_slice(&0u32.to_le_bytes());
        v
    });
    out.extend_from_slice(&seite(0x00, 0, serien, 1, &[&kom_block]));

    // -- Die FLAC-Rahmen, in Seiten --
    //
    // Und GEMESSEN ist die **Granelposition**: sie zaehlt die Proben **bis
    // einschliesslich** des **letzten fertigen Pakets** der Seite. Also **nicht**
    // vor dem Schreiben der Seite, sondern **danach** — und deshalb sammle ich
    // erst die Pakete einer Seite und rechne dann.
    // Release 08.10.2026: die Seitennummern laufen weiter — Seite 0 ist der
    // Kopf, Seite 1 der Kommentar, die erste Tonseite ist 2 (vorher stand
    // hier 1, also zwei Seiten mit derselben Nummer; RFC 3533 §6: die
    // Nummer zaehlt jede Seite des Stroms).
    let mut nummer = 2u32;
    let mut seite_idx = 0usize;
    let mut proben_bis = 0u32;
    while seite_idx < rahmen.len() {
        let mut gruppe: Vec<&[u8]> = Vec::new();
        let mut bytes = 0usize;
        let mut segmente = 0usize;
        let mut proben_gruppe = 0u32;
        while seite_idx < rahmen.len() {
            let laenge = rahmen[seite_idx].len();
            // Release 08.10.2026: eine Seite hat hoechstens 255 Segmente
            // (das Feld ist EIN Byte, RFC 3533 §6). Ein Paket braucht
            // `laenge / 255 + 1` davon. GEMESSEN vorher: nur die Bytes
            // wurden begrenzt — viele kleine Rahmen auf einer Seite liessen
            // die Segmentzahl ueberlaufen (`as u8`), die Seite war kaputt.
            let seg = laenge / 255 + 1;
            if !gruppe.is_empty() && (bytes + laenge > SEITE_MAX || segmente + seg > 255) {
                break;
            }
            gruppe.push(&rahmen[seite_idx]);
            bytes += laenge;
            segmente += seg;
            proben_gruppe += proben[seite_idx];
            seite_idx += 1;
        }
        let letzte = seite_idx >= rahmen.len();
        let granel = u64::from(proben_bis + proben_gruppe);
        proben_bis += proben_gruppe;
        let typ = if letzte { 0x04 } else { 0x00 }; // 0x04 = EOS
        out.extend_from_slice(&seite(typ, granel, serien, nummer, &gruppe));
        nummer += 1;
    }
    Ok(out)
}
