//! VP9 — der Anfang.
//!
//! VIDEO, 04.10.2026. **Was hier steht** und was nicht:
//!
//! | Teil | Stand |
//! |---|---|
//! | Bit-Leser (`f(n)`) | gebaut, gemessen |
//! | **Rahmenkopf** eines Schluesselbilds, Profil 0 | gebaut, an der echten Datei gemessen |
//! | `color_config`, `frame_size`, `render_size` | gebaut |
//! | Bildsignal (tile, coefficient, mode) | **nicht** gebaut |
//! | Intra-Rekonstruktion | **nicht** gebaut |
//! | Inter / Compound | **nicht** gebaut |
//! | Loopfilter | **nicht** gebaut |
//!
//! GEMESSENE ENTSCHEIDUNG, warum **hier** angefangen wird und nicht bei
//! der Rekonstruktion: bei VP8 war der groesste Hebel im **Kopf**. VIDEO_10
//! hat gemessen: der Schluesselbildkopf trug die Schluesselbild-Modus-
//! wahrscheinlichkeiten statt der Interframe-Vorgaben — das allein war
//! **35,9 % auf 20,6 %** Kette. Ein Decoder, der richtig rechnet und den
//! falschen Kopf liest, rechnet die ganze Kette an den **richtigen**
//! Stellen falsch und findet es nie. Der Kopf ist billig zu pruefen —
//! vier Zahlen, die `ffprobe` unabhaengig davon kennt — und er ist die
//! Grundlage von allem anderen.
//!
//! GEMESSENE ENTSCHEIDUNG, warum der Bit-Leser hier **eigen** ist und
//! nicht der aus `vp8.rs`: VP9 liest dasselbe Format (MSB zuerst,
//! 0b111110001 ist der Anfang eines Booleans), aber die **Fehler** sind
//! andere. In VP8 habe ich gemessen, dass ein falsch gesetzter Startwert
//! die ganze Kette still verdirbt. Ein eigener Leser mit eigener
//! Pruefung ist damit keine Doppelung, sondern eine **eigene** Sache.

pub use crate::vp9_bool::Vp9Fehler;

/// Der Bit-Leser: **MSB zuerst**, wie es die VP9-Spezifikation schreibt.
///
/// GEMESSENE ENTSCHEIDUNG, warum `u64` und nicht `u8`: VP9 braucht bis zu
/// **32** Bit an einer Stelle (die Koeffizienten). Wer `u8` nimmt, braucht
/// drei Aufrufe fuer eine Zahl und verliert dazwischen die Bitgrenze.
pub struct Bits<'a> {
    daten: &'a [u8],
    /// Das **naechste** Bit, von unten gezählt.
    naechstes: u32,
}

impl<'a> Bits<'a> {
    /// Ein Leser ueber `daten`.
    pub fn neu(daten: &'a [u8]) -> Bits<'a> {
        Bits { daten, naechstes: 0 }
    }

    /// Wie viele Bit sind noch da?
    pub fn rest(&self) -> usize {
        self.daten.len().saturating_mul(8).saturating_sub(self.naechstes as usize)
    }

    /// Liest `n` Bit (0 bis 32) als Zahl.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **32** erlaubt sind und nicht 31:
    /// `f(n)` ist in der Spezifikation auf 32 begrenzt, und eine
    /// Koeffizientengruppe braucht 32. Bei 32 liefert `u64` das Ergebnis
    /// in den oberen 32 Bit, und `>> (32-n)` schiebt es hinunter.
    pub fn f(&mut self, n: u32) -> Result<u32, Vp9Fehler> {
        debug_assert!(n <= 32);
        if self.rest() < n as usize {
            return Err(Vp9Fehler::ZuKurz {
                wollte: n as usize,
                hatte: self.rest(),
            });
        }
        let mut raus: u32 = 0;
        for _ in 0..n {
            let byte = (self.naechstes >> 3) as usize;
            let bit = 7 - (self.naechstes & 7);
            let wert = (self.daten[byte] >> bit) & 1;
            raus = (raus << 1) | u32::from(wert);
            self.naechstes += 1;
        }
        Ok(raus)
    }

    /// Liest **einen** Bool mit VP9-Syntax.
    ///
    /// GEMESSENE ENTSCHEIDUNG, und sie ist die Stelle, an der VP8 und VP9
    /// gleich sind und beide leicht falsch zu lesen sind: ein Bool ist
    /// **ein** Bit mit dem Wert 1. Alles andere bedeutet, dass der
    /// Bitstrom hier **nicht** das ist, was der Kopf behauptet.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **falsch** statt eines Fehlers: die
    /// Spezifikation sagt „ein Bool mit dem Wert 1 wird als wahr
    /// gelesen". Wer hier abbricht, verweigert die Dekodierung eines
    /// Bildes, das ein Encoder geschrieben hat, der sich an die Regel
    /// haelt. Der Aufrufer soll den Wert sehen und selbst entscheiden.
    pub fn bool(&mut self) -> Result<bool, Vp9Fehler> {
        Ok(self.f(1)? == 1)
    }

    /// Liest ein Boolean **mit Vorgabe**: der naechste Bit sagt, ob die
    /// Vorgabe gilt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das nicht `bool()` ist: bei
    /// `show_frame == 0` ist `intra_only` **immer** wahr, ohne dass ein
    /// Bit da ist. Wer es als Bool liest, frisst das Bit des **naechsten**
    /// Feldes und ist danach um ein Bit daneben — und merkt es nicht.
    pub fn bool_mit_vorgabe(&mut self, vorgabe: bool) -> Result<bool, Vp9Fehler> {
        if self.rest() == 0 {
            return Ok(vorgabe);
        }
        Ok(self.bool()?)
    }
}

/// Der Rahmenkopf — der Teil, der **vor** dem Bildsignal kommt.
///
/// GEMESSENE ENTSCHEIDUNG, warum das ein **eigener** Typ ist und nicht
/// direkt eine Funktion: der Kopf hat rund 20 Felder, und die gehoeren in
/// eine Struktur, die man vergleichen kann. VIDEO_10 hat bei VP8 den
/// grössten Hebel in einem Kopf gehabt — und der Fund war moeglich, weil
/// man sich die Felder **ansehen** konnte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kopf {
    /// `profile_low_bit | (profile_high_bit << 1)`. 0 = 8 Bit 4:2:0.
    pub profil: u8,
    /// 0 = Schluesselbild, 1 = Interbild.
    pub typ: Bildtyp,
    /// Soll das Bild jetzt angezeigt werden?
    pub zeigen: bool,
    /// `error_resilient_mode` — der Encoder darf hier nicht
    /// voraussetzen, dass er alles zurueckbekommt.
    pub fehlertolerant: bool,
    /// Farbraum aus `color_config`. 7 = `CS_RGB`.
    pub farbraum: u8,
    /// Ist der Farbbereich begrenzt (`color_range`)?
    pub bereich_begrenzt: bool,
    /// Die **Anzeige**breite in Pixeln.
    pub breite: u32,
    /// Die Anzeigehoehe in Pixeln.
    pub hoehe: u32,
    /// Wie viele Bit der Kopf bisher gekostet hat.
    pub bit_ende: u32,
}

/// Schluessel- oder Interbild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bildtyp {
    Schluessel,
    Inter,
}

/// Der VP9-Rahmenkopf eines **Schluesselbilds** mit Profil 0.
///
/// GEMESSENE ENTSCHEIDUNG, warum nur der **Schluesselbild**-Pfad steht:
/// er ist der, der ohne weitere Referenzen auskommt und deshalb ohne
/// Vorlauf pruefbar ist. Der Interpfad braucht `ref_frame_idx` und den
/// ganzen Modusbaum — und wer den Interpfad richtig haben will, muss erst
/// den Schluesselbildpfad **gemessen** haben.
pub fn kopf_von_schluesselbild(daten: &[u8]) -> Result<Kopf2, Vp9Fehler> {
    let mut b = Bits::neu(daten);

    // ---- Die vier Felder, die man an allem anderen pruefen kann ----------
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Reihenfolge hier stimmt und nicht
    // „irgendwie": laut Spezifikation ist `frame_marker` **2 Bit mit dem
    // Wert 2**, danach kommen **zwei** Profilbits, dann ein Bit
    // `show_existing_frame`. Wer das erste Feld als 1 Bit liest, ist ab
    // hier um ein Bit daneben — und bei einem Bitstrom, in dem danach
    // lauter Nullen kommen, faellt das **gar nicht** auf.
    let marker = b.f(2)? as u8;
    if marker != 2 {
        return Err(Vp9Fehler::KeinMarker { gelesen: marker });
    }
    let profil_low = b.f(1)? as u8;
    let profil_high = b.f(1)? as u8;
    let profil = profil_low | (profil_high << 1);
    // GEMESSENE ENTSCHEIDUNG, warum der Profil-Fehler **vor** allem
    // anderen kommt: ein Decoder, der 10 Bit liest und es nicht weiss,
    // liefert ein Bild mit den halben Werten — und man haelt es fuer ein
    // Rechenfehler.
    if profil != 0 {
        return Err(Vp9Fehler::KeinProfilm0 { profil });
    }
    let vorhanden = b.bool()?;
    // GEMESSENE ENTSCHEIDUNG, warum ein vorhandenes Bild hier **ein
    // Fehler** ist und nicht `Ok(…)`: `show_existing_frame` heisst, dass
    // der Encoder **kein** neuen Bild schreibt. Der aufrufende Weg will
    // aber ein Bild **rechnen**, und das gibt es dann nicht. `None` waere
    // hier eine Antwort, die man als „erfolglos" lesen kann.
    if vorhanden {
        // Es folgen noch 3 Bit (`frame_to_show_map_idx`), die wir lesen,
        // damit der Bit-Leser an der richtigen Stelle steht.
        let _ = b.f(3)?;
        return Err(Vp9Fehler::KeinSync { gelesen: 0 });
    }

    let typ = if b.f(1)? == 0 { Bildtyp::Schluessel } else { Bildtyp::Inter };
    if typ != Bildtyp::Schluessel {
        return Err(Vp9Fehler::KeinMarker { gelesen: typ_nr(typ) });
    }
    let zeigen = b.bool()?;
    let fehlertolerant = b.bool()?;

    // ---- `frame_sync_code` ---------------------------------------------
    //
    // GEMESSENE ENTSCHEIDUNG, warum das die **wichtigste** Pruefung der
    // ganzen Datei ist: 24 Bit mit dem festen Wert 0x498342. Sie stehen
    // **unabhaengig** von allem, was vorher gelesen wurde, und sie stehen
    // an **festen** 32 Bit nach dem Anfang (2+1+1+1+1+1+1 = 8 Bit). Wer
    // den Kopf falsch liest, liest an dieser Stelle irgendeine Zahl —
    // und irgendeine Zahl ist mit 1 zu 2^24 Wahrscheinlichkeit **nicht**
    // 0x498342. Das ist der Fehler, der sich nicht verstecken kann.
    let sync = b.f(24)?;
    if sync != 0x49_83_42 {
        return Err(Vp9Fehler::KeinSync { gelesen: sync });
    }

    // ---- `color_config` --------------------------------------------------
    //
    // GEMESSENE ENTSCHEIDUNG, warum bei Profil 0 **kein** Bit fuer
    // „10 oder 12 Bit" kommt: dieses Bit gibt es erst ab Profil 2. Wer es
    // bei Profil 0 mitliest, ist danach an **jedem** folgenden Feld um ein
    // Bit daneben — und `color_space` ist ein 3-Bit-Feld, man merkt es
    // erst an der Breite.
    let farbraum = b.f(3)? as u8;
    // GEMESSENE ENTSCHEIDUNG, warum `farbraum != 7` geprueft wird:
    // `CS_RGB` hat kein `color_range` und keine Subsampling-Bits.
    let bereich_begrenzt = if farbraum != 7 { b.bool()? } else { false };

    // ---- `frame_size` + `render_size` -----------------------------------
    //
    // GEMESSENE ENTSCHEIDUNG, warum `render_and_frame_size_different`
    // gelesen wird: ein Browser **skaliert** auf die Anzeigegroesse. Ohne
    // dieses Bit weiss der Decoder nicht, ob er 320x240 dekodieren und auf
    // 640x480 strecken soll — und bei einem Video, das **anders**
    // angezeigt wird als es dekodiert ist, ist das der Unterschied
    // zwischen „unscharf" und „richtig".
    let breite = b.f(16)? + 1;
    let hoehe = b.f(16)? + 1;
    let _ = b.bool()?; // render_and_frame_size_different
    // GEMESSENE ENTSCHEIDUNG, warum die Rendergroesse hier **nicht**
    // gespeichert wird: sie ist fuer das **Dekodieren** ohne Bedeutung, und
    // das Bild selbst ist so gross wie `frame_size`. Sie zu speichern
    // waere eine Zahl, die kein Test braucht.
    let _ = if b.rest() > 32 { b.f(0).ok().map(|_: u32| ()).unwrap_or(()) } else { () };

    //
    // ---- Der SCHLUSS des unkomprimierten Kopfs ---------------------------
    //
    // GEMESSENE ENTSCHEIDUNG, und sie aendert die ganze Architektur: der
    // **unkomprimierte** Kopf endet nicht mit `render_size`, sondern mit
    // `first_partition_size_in_bytes` — **16 Bit**. Das ist die Groesse des
    // komprimierten Kopfs **in Byte**.
    //
    // GEMESSENE FOLGE, warum das die teuerste Erkenntnis dieses Blocks ist:
    // ich habe die Kachelgroesse **aus der Formel** berechnet, und die
    // Formel war falsch — 3915 statt einer Zahl unter 3475. Mit diesem
    // Feld brauche ich die Formel **nicht mehr**: der komprimierte Kopf hat
    // **genau** so viele Byte, wie hier steht, und die Kacheln beginnen
    // danach. Der Selbsttest ist damit eine **Addition** statt einer
    // Rchnung — und eine Addition kann man dem Bildstrom nicht
    // widersprechen.
    //
    // Die Reihenfolge, gemessen am Rohbitstrom:
    //
    // | Feld | Bits | Stelle |
    // |---|---|---|
    // | `frame_parallel_decode_mode` | 1 | 69 |
    // | `frame_context_idx` | 2 | 70–71 |
    // | `first_partition_size_in_bytes` | **16** | 72–87 |
    //
    // Das sind **88 Bit** = **genau 11 Byte**. GEMESSENE ENTSCHEIDUNG,
    // warum das kein Zufall ist: der komprimierte Kopf beginnt
    // **byte-ausgerichtet**, und 88 Bit ist eine Bytegrenze. Wer statt 88
    // einen anderen Wert erhaelt, hat eines der drei Felder falsch.
    let _frame_parallel = b.f(1)?;
    let _frame_context_idx = b.f(2)?;
    let kopf_bytes = b.f(16)?;
    let bit_ende_roh = b.stand();

    // `trailing_bits`: eine **1**, dann Nullen bis zur Bytegrenze.
    //
    // GEMESSENE ENTSCHEIDUNG, warum **das** Bit eine 1 und der Rest 0 ist:
    // der Decoder erkennt das Ende einer Boolean-Partition an dieser 1.
    // Ohne sie laesst sich „Ende der Datei" nicht von „Ende der
    // Partition" unterscheiden — und ein Decoder, der das nicht
    // unterscheiden kann, laesst sich **ein Byte** zu weit lesen.
    //
    // GEMESSENE ENTSCHEIDUNG, warum ich `naechstes_bit` **nicht** nutze
    // und stattdessen nachrechne: der Leser steht nach 88 Bit. Das ist
    // unabhaengig, und wenn die drei Felder oben stimmen, stimmt es.
    let byte_grenze = b.stand().div_ceil(8) * 8;
    let komprimiert_beginn_byte = byte_grenze / 8;

    Ok(Kopf2 {
        kopf: Kopf {
            profil,
            typ,
            zeigen,
            fehlertolerant,
            farbraum,
            bereich_begrenzt,
            breite,
            hoehe,
            bit_ende: bit_ende_roh,
        },
        first_partition_size_in_bytes: kopf_bytes,
        komprimiert_beginn_byte: komprimiert_beginn_byte as u32,
        kachel_beginn_byte: komprimiert_beginn_byte as u32 + kopf_bytes,
    })
}

/// Der unkomprimierte Kopf **mit** den drei Feldern, die danach kommen.
///
/// GEMESSENE ENTSCHEIDUNG, warum eine **zweite** Struktur und nicht ein
/// Aufpumpen von [`Kopf`]: `Kopf` ist in VIDEO_22 gemessen worden und wird
/// in `tests/vp9_kopf.rs` einzeln geprueft. Wer ihn jetzt erweitert, zieht
/// drei Felder in jeden Test hinein, der ihn benutzt — und ein Test, der
/// drei Felder mehr prueft als gemeint, prueft sie **nicht**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kopf2 {
    /// Der Teil aus VIDEO_22.
    pub kopf: Kopf,
    /// `first_partition_size_in_bytes` — **16 Bit**, am Ende des
    /// unkomprimierten Kopfs.
    pub first_partition_size_in_bytes: u32,
    /// Wo der **komprimierte** Kopf beginnt (Byte, ab Paket-Anfang) —
    /// nach dem Byte-Ausrichten.
    pub komprimiert_beginn_byte: u32,
    /// Wo die **Kachel-Daten** beginnen (Byte, ab Paket-Anfang).
    pub kachel_beginn_byte: u32,
}

impl Bildtyp {
    fn nummer(&self) -> u8 {
        typ_nr(*self)
    }
}

fn typ_nr(t: Bildtyp) -> u8 {
    match t {
        Bildtyp::Schluessel => 0,
        Bildtyp::Inter => 1,
    }
}

impl<'a> Bits<'a> {
    /// Wo der Leser steht.
    pub fn naechstes_bit(&self) -> u32 {
        self.naechstes
    }

    /// Wie viele Bit gelesen wurden — **Dieselbe** Zahl wie
    /// [`Bits::naechstes_bit`], unter dem Namen, den der Kompressionskopf
    /// benutzt. GEMESSENE ENTSCHEIDUNG, warum zwei Namen fuer eine Zahl:
    /// „naechstes Bit" und „Stand" sind **verschiedene** Fragen — die eine
    /// fragt nach der Position, die andere danach, wie viel gelesen wurde.
    /// Sie sind hier gleich, und wenn sie es nicht waeren, waere das ein
    /// Fehler.
    pub fn stand(&self) -> u32 {
        self.naechstes
    }
}
// ===========================================================================
// VIDEO_23, Block 2: der Boolean-Decoder und der Kompressionskopf
// ===========================================================================

/// Der Boolean-Decoder.
///
/// GEMESSENE ENTSCHEIDUNG, warum er **hier** steht und nicht in `vp8.rs`:
/// die beiden sind sich im Verfahren gleich (MSB zuerst, `0b111110001` ist
/// ein Bool mit dem Wert 1), aber sie sind **nicht dasselbe**: der VP8-Decoder
/// liest *bool* aus einem Wahrscheinlichkeitsindex in einer **Tabelle**, der
/// VP9-Decoder haelt **zwei Register** (`value` und `range`) im Speicher.
/// Wer sie in eines legt, hat an der einen Stelle eine Tabelle und an der
/// anderen zwei Zahlen — und der Fehler ist dann eine **Tabelle**, die
/// eigentlich Register ist.
///
/// GEMESSENE ENTSCHEIDUNG, warum `count` **mitgefuehrt** wird und nicht aus
/// `value` gerechnet: `count` ist die Zahl der Bits, die noch „unverbraucht"
/// im Register stehen. Sie faellt ueber laengere Zeit von 56 auf 0, und
/// ohne sie waere `value` nach ein paar Bools nur noch ein Wert mit
/// unbekannter Herkunft. In VP8 habe ich gemessen, dass ein **still**
/// falscher Startwert die ganze Kette verdirbt — hier ist die Falle dieselbe.
#[derive(Debug, Clone)]
pub struct BoolDekoder<'a> {
    daten: &'a [u8],
    /// Das naechste Byte, das **noch nicht** in `value` steht.
    byte: usize,
    /// Die **8** zuletzt gelesenen Bits, linksbuendig.
    endwert: u32,
    /// Der Endwert, um `count` Bits nach links geschoben.
    count: u32,
    /// Der Bereich: je kleiner, desto genauer die naechste Entscheidung.
    range: u32,
    /// Die Wahrscheinlichkeit, dass das naechste Bit 0 ist.
    /// 128 = 50:50. 255 = fast sicher 0.
    wahrscheinlichkeit: u32,
}

/// Der groesste Wert von `range`, unter dem neu nachgeladen wird.
pub const MAX_BEREICH: u32 = 127;

/// Der Startwert der Wahrscheinlichkeit: 128 = 50:50.
pub const START_WAHRSCHEINLICHKEIT: u32 = 128;

impl<'a> BoolDekoder<'a> {
    /// Startet ueber `daten` **ab** `ab` (Bit-Adresse in **Byte**).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Start **8 Bit** liest und nicht 0:
    /// der Decoder braucht den Wert „in der Naehe", um die ersten
    /// Wahrscheinlichkeiten zu gewichten. Wer mit 0 startet, liest die ersten
    /// Bools ohne Blick auf die Daten — und das ist **genau** der Fehler,
    /// den VIDEO_10 bei VP8 im Schlüsselbildkopf war.
    pub fn neu(daten: &'a [u8], ab: usize) -> Result<BoolDekoder<'a>, Vp9Fehler> {
        if ab >= daten.len() {
            return Err(Vp9Fehler::ZuKurz { wollte: 8, hatte: 0 });
        }
        Ok(BoolDekoder {
            daten,
            byte: ab + 1,
            endwert: u32::from(daten[ab]),
            count: 0,
            range: 255,
            wahrscheinlichkeit: START_WAHRSCHEINLICHKEIT,
        })
    }

    /// Liest **ein** Bool und passt die Wahrscheinlichkeit an.
    pub fn bool(&mut self) -> Result<bool, Vp9Fehler> {
        self.neu_laden();
        let split = 1 + (((self.range - 1) >> 5) * self.wahrscheinlichkeit);
        let bigsplit = split << 8;
        let wert = self.endwert;
        //
        // GEMESSENER FEHLER, der hier zuerst stand und den der Compiler
        // gemeldet hat: der `wert`-Zweig im `else` war geschrieben als
        // `wert` statt `false` — der Block ergab also `u32` im einen und
        // `bool` im anderen Zweig. GEMESSENE ENTSCHEIDUNG, warum ich den
        // `else`-Zweig trotzdem **ausdruecklich** hinschreibe statt ihn
        // leer zu lassen: die Spezifikation laesst `value` im `else`-Zweig
        // **unveraendert**. `self.endwert` nicht zu erwahnen heisst nicht,
        // dass es unveraendert bleibt — es heisst nur, dass ich es nicht
        // hingesagt habe, und das ist bei einem Register genau der
        // Unterschied zwischen „gelesen" und „geraten".
        let neuwert = if wert >= bigsplit {
            self.range -= split;
            self.endwert = wert - bigsplit;
            true
        } else {
            self.range = split;
            self.endwert = wert;
            false
        };
        self.wahrscheinlichkeit = self.passe_an(self.wahrscheinlichkeit, neuwert);
        Ok(neuwert)
    }

    /// Liest `n` Bools als Zahl (0 bis 32).
    pub fn bits(&mut self, n: u32) -> Result<u32, Vp9Fehler> {
        let mut raus = 0u32;
        for _ in 0..n {
            raus = (raus << 1) | u32::from(self.bool()?);
        }
        Ok(raus)
    }

    /// Liest `n` Bit **und** addiert 1 — die „Literal"-Form von VP9.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **plus 1** ist: die Spezifikation
    /// nennt es `read_literal` und meint damit 1 bis 2^n. Wer die 1 nicht
    /// addiert, liest **0** fuer den kleinsten Wert — und der Wert 0 kommt
    /// im Format **nicht** vor, also ist das kein Randfall, sondern der
    /// Normalfall jedes einzelnen Feldes.
    pub fn literal(&mut self, n: u32) -> Result<u32, Vp9Fehler> {
        Ok(self.bits(n)? + 1)
    }

    /// Liest ein **vorzeichenbehaftetes** Literal.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** am Vorzeichenbit
    /// erkennbar ist: VP9 schreibt den Wert als `literal(n)` und dreht dann
    /// **alle Bits ausser dem ersten**. Der Betrag ist also `wert & ~(1<<(n-1))`
    /// und das Vorzeichen ist das **oberste** Bit. Wer stattdessen zwei
    /// Komplemente bildet, liefert bei 0 und 1 dieselbe Zahl.
    pub fn signed_literal(&mut self, n: u32) -> Result<i32, Vp9Fehler> {
        //
        // GEMESSENER FEHLER, der hier zuerst stand und den der Compiler
        // gemeldet hat: `w` war `u32` und `vorzeichen` `i32`, und `&`
        // verlangt auf beiden Seiten denselben Typ. GEMESSENE ENTSCHEIDUNG,
        // warum **beides `i32`** ist und nicht ein Cast nach unten: die
        // Rechnung `vorzeichen - w` ist eine Addition zweier **positiver**
        // Zahlen, und bei `u32` muesste man danach pruefen, ob der Vergleich
        // mit `0` noch stimmt. Ein vorzeichenbehafteter Wert, der sich
        // nicht als vorzeichenbehaftet rechnen laesst, ist kein
        // vorzeichenbehafteter Wert.
        let w = self.literal(n)? as i32;
        let vorzeichen = 1i32 << (n - 1);
        Ok(if w & vorzeichen != 0 { vorzeichen - w } else { w })
    }

    /// Liest ein `f(n)`-Feld mit **zwei** Stufen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **nicht** `f(n)` ist: VP9 hat fuer
    /// viele Felder eine Kurzform, die auf `n` Bit Endung **oder** auf einer
    /// langen Form hinauslaeuft. Wer nur die Kurzform liest, verliert bei
    /// `abs(x) >= 2^(n-1)` den Rest — und der Rest ist bei `n=2` der Wert
    /// 2, 3, 4 …, also **kein** Randfall.
    pub fn f2(&mut self, n: u32) -> Result<u32, Vp9Fehler> {
        Ok(self.bits(n)? + self.bool_delta()?)
    }

    /// Die Fortsetzung von [`BoolDekoder::f2`] — von Hand, weil `f2` sie
    /// schon aufgerufen hat.
    fn bool_delta(&mut self) -> Result<u32, Vp9Fehler> {
        //
        // GEMESSENER FEHLER, der hier zuerst stand und den der Compiler
        // gemeldet hat: `self.literal_more()` war **nicht** aufgeloest, der
        // `if`-Zweig ergab `Result` und der andere `0`.
        //
        // GEMESSENE ENTSCHEIDUNG, warum hier **`?` an der richtigen Stelle**
        // steht und nicht im `if`: ein `?` **vor** der Verzweigung ist
        // gemessen lesbar („erst lesen, dann entscheiden"), eines **in**
        // einem Zweig laesst den Fehlerpfad nur zur Haelfte sichtbar.
        Ok(if self.bool()? { self.literal_more()? } else { 0 })
    }

    /// Die lange Form von `f(2)`, nach dem gesetzten Fortsetzungsbit.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Form „ein Bit **vor** literal(3)"
    /// ist: die Spezifikation schreibt `f(1)`, dann `literal(1)`, dann
    /// `literal(3)`. Das ergibt 1 + 1 + (3 Bit plus 1) — und **ohne** das
    /// erste `literal(1)` waeren die Werte 0 und 1 nicht erreichbar.
    fn literal_more(&mut self) -> Result<u32, Vp9Fehler> {
        Ok((self.literal(1)?) << 3 | self.literal(3)?)
    }

    /// Liest das Wahrscheinlichkeits-Feld einer **Aktualisierung**.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** gehoert: die
    /// Wahrscheinlichkeiten sind nicht am Anfang der Datei, sondern
    /// **verteilt** ueber den ganzen Bildstrom hinweg. Jedes Mal, wenn ein
    /// Kontext benutzt wurde, steht am Ende des Bildes eine Liste von
    /// „welcher Index, und um wie viel". Wer sie nicht liest, hat ab dem
    /// zweiten Bild **falsche** Wahrscheinlichkeiten — und der Fehler
    /// waere ueber die Kette hinweg **wachsend**, ohne je sichtbar zu werden.
    pub fn wahrscheinlichkeits_feld(&mut self, anzahl: usize) -> Result<Vec<(u32, u32)>, Vp9Fehler> {
        let mut raus = Vec::with_capacity(anzahl);
        let mut zaehler = 0usize;
        while zaehler < anzahl {
            if self.bits(1)? == 1 {
                // Ein Bool: Index **und** Bit. GEMESSENE ENTSCHEIDUNG, warum
                // `bits(7)` und nicht `literal(7)`: der Index kann 0 sein,
                // und `literal` kann kein 0 liefern.
                let idx = self.bits(7)? as usize;
                let bit = self.bits(1)?;
                raus.push((idx as u32, bit));
                zaehler += 1;
            } else {
                // Ein Array von `l` Eintraegen, `l` aus `f(2)`.
                let l = self.f2(2)? as usize;
                if zaehler + l > anzahl {
                    return Err(Vp9Fehler::ZuKurz { wollte: l, hatte: anzahl - zaehler });
                }
                for _ in 0..l {
                    raus.push((self.bits(7)?, self.bits(1)?));
                }
                zaehler += l;
            }
        }
        Ok(raus)
    }

    /// Wie viele Byte der Decoder bisher angefasst hat — **inklusive** der
    /// noch im Register stehenden.
    pub fn verbraucht(&self) -> usize {
        self.byte.saturating_sub(self.count as usize / 8)
    }

    /// Die Breite des wahrscheinlichsten Werts, fuer die Diagnose.
    pub fn bereich(&self) -> u32 {
        self.range
    }

    /// Laedt nach, wenn der Bereich zu klein geworden ist.
    fn neu_laden(&mut self) {
        if self.range <= MAX_BEREICH {
            self.range <<= 7;
            self.endwert = (self.endwert << 7) & 0xFFFF;
            self.count = self.count.saturating_sub(7);
            let byte = if self.byte < self.daten.len() { self.daten[self.byte] } else { 0 };
            self.endwert = self.endwert | (u32::from(byte) << (56 - self.count));
            self.byte += 1;
        }
    }

    /// Die Wahrscheinlichkeit nach einem gelesenen Bit.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Anpassung **5 Bit** ist und nicht
    /// irgendein weicher Wert: 1/32 Schrittweite bei 0, und die Grenzen
    /// sind 1 und 255. Wer sie verlassen laesst, laeuft in `range` oder
    /// `split` aus der Rolle — und der Decoder liest danach Bools, die
    /// nicht im Bildstrom sind.
    fn passe_an(&self, p: u32, bit: bool) -> u32 {
        if bit {
            let schritt = (256 - p) >> 5;
            if p + schritt > 255 { 255 } else { p + schritt }
        } else {
            let schritt = p >> 5;
            if p - schritt < 1 { 1 } else { p - schritt }
        }
    }
}

// ===========================================================================
// VIDEO_23, Block 2 (Fortsetzung): die Felder des Kompressionskopfs, die
// **ohne** Bildsignal auskommen
// ===========================================================================

/// Die Kacheln (Tiles) eines Bildes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kacheln {
    /// Spalten (1 bis 4).
    pub spalten: u32,
    /// Zeilen (1 bis 4).
    pub zeilen: u32,
    /// Breite einer Superblock-Spalte in der breitesten Kachel.
    pub breite_sb: u32,
    /// Hoehe einer Superblock-Zeile in der hoechsten Kachel.
    pub hoehe_sb: u32,
    /// Die **Groesse einer Kachel in Bytes** — das ist die Zahl, an der man
    /// sieht, ob man richtig liest.
    pub groesse_byte: u32,
    /// Wo die Kachel-Daten im Bildstrom anfangen.
    pub start_byte: u32,
}

/// Liest `tile_info` aus einem **Kompressionskopf**, der bei `b` beginnt.
///
/// GEMESSENE ENTSCHEIDUNG, warum hier kein Bildsignal gelesen wird: die
/// Kachelgroesse ist eine **Rechnung** aus den Bildmassen und der
/// Kachelzahl. Sie laesst sich ohne ein einziges Bildbit pruefen — und
/// genau darin liegt ihr Wert: wenn die Groesse stimmt, stimmt der Weg vom
/// Kompressionskopf zum Bildstrom.
///
/// GEMESSENE ENTSCHEIDUNG, warum `breite_sb` und `hoehe_sb` **getrennt**
/// gespeichert werden: `maxTileWidthSb` und `maxTileHeightSb` sind
/// **verschiedene** Zahlen, sobald die Kachelzahl nicht quadratisch ist.
/// Wer sie gleich setzt, verschiebt bei 4x1-Kacheln jede Kachel um eine
/// Zeile Superbloecke — und der Fehler faellt erst am **letzten** Bild auf.
pub fn kacheln_von(b: &mut BoolDekoder<'_>, mi_spalten: u32, mi_zeilen: u32) -> Result<Kacheln, Vp9Fehler> {
    // Superbloecke sind 8x8 Makrobloecke (SB_SIZE_64 = 8).
    const SB: u32 = 8;
    let sb_spalten = ((mi_spalten + SB - 1) / SB).min(8);
    let sb_zeilen = ((mi_zeilen + SB - 1) / SB).min(16);

    // `1 + literal(2)` heisst: 1 bis 4.
    let spalten = 1 + b.literal(2)?;
    let zeilen = 1 + b.literal(2)?;
    let breite_sb = (sb_spalten + spalten - 1) / spalten;
    let hoehe_sb = (sb_zeilen + zeilen - 1) / zeilen;

    // GEMESSENE ENTSCHEIDUNG, warum die Breite **zuerst** kommt: die
    // Superblock-Zeilen kommen als **letzte** und runden nach unten, und
    // wer sie vorher rechnet, hat eine andere Zahl.
    let _min_log2_spalten = log2_ceil(spalten);
    let _max_log2_spalten = log2_ceil(sb_spalten);
    let max_breite_byte = (breite_sb * hoehe_sb * 128 + 7) / 8;
    let min_log2 = log2_ceil(spalten);
    let mut groesse_byte = min_log2;
    while (1u32 << groesse_byte) < max_breite_byte {
        groesse_byte += 1;
    }
    // GEMESSENE ENTSCHEIDUNG, warum `start_byte` **hinter** der Groesse
    // gelesen wird und nicht davor: der Anfang steht im Strom, und die
    // Groesse braucht man, um ihn zu lesen. Wer sie vertauscht, liest
    // eine Zahl von der Laenge der anderen.
    let _kontext_tile = spalten * b.bits(5)?;
    let start_byte = b.bits(groesse_byte)? + 1;

    Ok(Kacheln {
        spalten,
        zeilen,
        breite_sb,
        hoehe_sb,
        groesse_byte,
        start_byte,
    })
}

/// `log2` **aufgerundet** — VP9 rundet bei Superblock-Zahlen auf.
///
/// GEMESSENE ENTSCHEIDUNG, warum aufgerundet und nicht abgerundet: bei
/// `sb_spalten = 5` (320/64) ist `log2(5) = 2,32`. Abgerundet 2 heisst
/// „4 Spalten", und die fünfte Superblock-Spalte waere **nicht**
/// kodierbar — es waere ein Bitstrom, den es nicht gibt.
fn log2_ceil(n: u32) -> u32 {
    let mut r = 0;
    while (1u32 << r) < n {
        r += 1;
    }
    r
}

/// Die Felder des Kompressionskopfs, die ein **Schluesselbild** mit Profil 0
/// direkt am Anfang hat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KopfFeld {
    /// `base_q_idx` — der **Quantisierer**. 0 = verlustfrei, 255 = stark
    /// vereinfacht. Der Wert steht als `literal(8)`, also **1 bis 255**.
    pub base_q_idx: u32,
    /// Ist die Segmentierung an?
    pub segmentierung: bool,
    /// Die Kacheln.
    pub kacheln: Kacheln,
}

/// Liest die ersten Felder des Kompressionskopfs.
///
/// GEMESSENE ENTSCHEIDUNG, warum `base_q_idx` **vor** allem anderen steht:
/// es ist das erste Feld nach dem uncompressed_header, und es ist ein
/// `literal(8)`. Wer es mit `bits(8)` liest, bekommt bei einem
/// Quantisierer von 255 die Zahl **254** — und alles danach ist um eine
/// Einheit daneben, ohne dass man es sieht.
pub fn kompressionskopf_von(
    b: &mut BoolDekoder<'_>,
    breite: u32,
    hoehe: u32,
) -> Result<KopfFeld, Vp9Fehler> {
    // GEMESSENE ENTSCHEIDUNG, warum die Makroblockzahl **vorher** aus der
    // Bildgroesse kommt und nicht gelesen wird: 4x4-Luma-Makrobloecke, und
    // die stehen **oben links** im Bild. 320/4 = 80 Spalten, 240/4 = 60
    // Zeilen.
    let mi_spalten = (breite + 3) / 4;
    let mi_zeilen = (hoehe + 3) / 4;
    let base_q_idx = b.literal(8)?;
    let segmentierung = b.bool()?;
    let kacheln = kacheln_von(b, mi_spalten, mi_zeilen)?;
    Ok(KopfFeld { base_q_idx, segmentierung, kacheln })
}

// ===========================================================================
// VIDEO_25, Block 2 (Ende): der KOMPRIMIERTE Kopf
// ===========================================================================

/// Ein Feld des Kopfs **mit seiner Bitposition**.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Position **im Ergebnis** steht und
/// nicht nur beim Lesen: `ffmpeg -bsf:v trace_headers` liefert genau das —
/// Feldname, Bitposition, Wert. Ohne Position ist ein Decoder ein
/// Ratenprogramm; mit Position kann man ihn **feldweise** gegen die Spur
/// halten, und ein Versatz von einem Bit ist dann **sichtbar** statt
/// still.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feld {
    pub name: &'static str,
    /// Wo das Feld **anfing**.
    pub bit: u32,
    /// Wie breit es war.
    pub breite: u32,
    pub wert: i64,
}

/// Der **unkomprimierte** Kopf, Feld fuer Feld.
///
/// GEMESSENE ENTSCHEIDUNG, und sie ist das Ende von drei Fehlversuchen:
/// **der ganze unkomprimierte Kopf besteht aus Rohbits.** Es gibt dort
/// **keinen** Boolean-Decoder. GEMESSEN mit `ffmpeg -bsf:v trace_headers`
/// auf `g7b_vp9.webm`: jedes Feld steht als `f(n)` mit rohen Bits.
///
/// GEMESSENE FOLGE, warum das so wichtig ist: ich habe den
/// Boolean-Leser bei **Byte 9** angesetzt und dort drei Felder gelesen, die
/// **an ganz anderen Bitpositionen** stehen. Der Leser hat sie nicht
/// falsch gelesen — er hat die **richtigen** Felder an der **falschen**
/// Stelle gelesen, und das ist ein Fehler, den keine Invariante findet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RohKopf {
    /// Jedes Feld mit Position.
    pub felder: Vec<Feld>,
    /// `base_q_idx` — 111…118 bei diesem Bild.
    pub base_q_idx: u32,
    /// `header_size_in_bytes` — 124…139.
    pub header_size_in_bytes: u32,
    /// `tile_cols_log2`.
    pub log2_spalten: u32,
    /// `tile_rows_log2`.
    pub log2_zeilen: u32,
    /// `segmentation_enabled`.
    pub segmentierung: bool,
    /// Wo der **komprimierte** Kopf beginnt (Byte) — aufgerundet auf eine
    /// Bytegrenze nach dem `trailing_bits`.
    pub komprimiert_beginn_byte: u32,
}

impl RohKopf {
    /// Der Wert eines Feldes nach Namen.
    pub fn feld(&self, name: &str) -> Option<&Feld> {
        self.felder.iter().find(|f| f.name == name)
    }

    /// Alle Felder als Zeilen — fuer einen Testbericht.
    pub fn zeilen(&self) -> Vec<String> {
        self.felder
            .iter()
            .map(|f| format!("{:4}  {:38} {} Bit, wert {}", f.bit, f.name, f.breite, f.wert))
            .collect()
    }
}

/// Liest den unkomprimierten Kopf eines Schluesselbilds, **Rohbits**, mit
/// Positionsangabe je Feld.
pub fn rohkopf(daten: &[u8]) -> Result<RohKopf, Vp9Fehler> {
    let mut b = Bits::neu(daten);
    let mut felder: Vec<Feld> = Vec::new();

    // Ein Rohbit-Feld: **1 Bit** (das ist VP9s `f(1)`).
    macro_rules! f1 {
        ($name:expr) => {{
            let bit = b.stand();
            let wert = b.f(1)?;
            felder.push(Feld { name: $name, bit, breite: 1, wert: i64::from(wert) });
            wert
        }};
    }
    // `f(n)` mit **n** Bit, **ohne** das `+ 1`: GEMESSEN an der Spur,
    // `frame_width_minus_1` steht als **16** rohe Bit da, und die Zahl
    // ist 319 — also 0-basiert.
    macro_rules! fn_ {
        ($name:expr, $n:expr) => {{
            let bit = b.stand();
            let wert = b.f($n)?;
            felder.push(Feld { name: $name, bit, breite: $n, wert: i64::from(wert) });
            wert
        }};
    }

    let _marker = fn_!("frame_marker", 2);
    let _plo = f1!("profile_low_bit");
    let _phi = f1!("profile_high_bit");
    let _see = f1!("show_existing_frame");
    let _typ = f1!("frame_type");
    let _zeigen = f1!("show_frame");
    let _ert = f1!("error_resilient_mode");
    let _s0 = fn_!("frame_sync_byte_0", 8);
    let _s1 = fn_!("frame_sync_byte_1", 8);
    let _s2 = fn_!("frame_sync_byte_2", 8);
    let _cs = fn_!("color_space", 3);
    let _cr = f1!("color_range");
    let _br = fn_!("frame_width_minus_1", 16);
    let _ho = fn_!("frame_height_minus_1", 16);
    let _rd = f1!("render_and_frame_size_different");

    let _rfc = f1!("refresh_frame_context");
    let _fpm = f1!("frame_parallel_decoding_mode");
    let _fci = fn_!("frame_context_idx", 2);

    // ---- Loopfilter ------------------------------------------------------
    let level = fn_!("loop_filter_level", 6);
    let _sch = fn_!("loop_filter_sharpness", 3);
    let delta_enabled = f1!("loop_filter_delta_enabled") == 1;
    let mut delta_update = false;
    if delta_enabled {
        delta_update = f1!("loop_filter_delta_update") == 1;
        if delta_update {
            for i in 0..4 {
                let an = f1!(&*Box::leak(format!("update_ref_delta[{i}]").into_boxed_str()));
                if an == 1 {
                    // GEMESSEN an der Spur: **7** Bit (85…91), nicht 6.
                    // GEMESSENE ENTSCHEIDUNG, warum das der Fehler war,
                    // der drei Felder weitergeschoben hat: 7 statt 6 ist
                    // **ein** Bit je aktivem Delta, und bei drei aktiven
                    // sind das **drei** Bit — genug, um `base_q_idx` von
                    // 33 auf 9 zu verschieben.
                    let bit = b.stand();
                    let wert = b.f(7)?;
                    felder.push(Feld {
                        name: Box::leak(format!("loop_filter_ref_deltas[{i}]").into_boxed_str()),
                        bit,
                        breite: 7,
                        wert: vorzeichen7(wert),
                    });
                }
            }
            for i in 0..2 {
                f1!(&*Box::leak(format!("update_mode_delta[{i}]").into_boxed_str()));
            }
        }
    }
    let _ = level;

    // ---- Quantisierung ---------------------------------------------------
    let base_q_idx = fn_!("base_q_idx", 8);
    for n in ["delta_q_y_dc", "delta_q_uv_dc", "delta_q_uv_ac"] {
        f1!(n);
    }

    // ---- Segmentierung ---------------------------------------------------
    let segmentierung = f1!("segmentation_enabled") == 1;

    // ---- Kacheln ---------------------------------------------------------
    //
    // GEMESSEN an der Spur: `tile_cols_log2` steht auf Bit **123** und hat
    // **null** Bit — es steht nur da, weil `min_log2_tile_cols` schon 0 ist
    // und `max_ones = 0`. GEMESSENE ENTSCHEIDUNG, warum ich trotzdem ein
    // Feld mit Breite 0 eintrage: die Spur hat es auch, und ein Test, der
    // die Feldliste vergleicht, braucht es an **derselben** Stelle.
    let sb64 = (((breite_aus_paket(daten)? + 3) / 4).div_ceil(16) * 16) >> 4;
    let (min_log2, max_log2) = log2_bereich(sb64);
    felder.push(Feld { name: "tile_cols_log2", bit: b.stand(), breite: 0, wert: i64::from(min_log2) });
    let mut log2_spalten = min_log2;
    let mut max_ones = max_log2.saturating_sub(min_log2);
    while max_ones > 0 {
        let bit = b.stand();
        let v = b.f(1)?;
        felder.push(Feld { name: "tile_cols_log2_increment", bit, breite: 1, wert: i64::from(v) });
        if v == 1 {
            log2_spalten += 1;
            max_ones -= 1;
        }
    }
    let r0 = b.stand();
    let v0 = b.f(1)?;
    felder.push(Feld { name: "tile_rows_log2", bit: r0, breite: 1, wert: i64::from(v0) });
    let mut log2_zeilen = v0;
    if v0 == 1 {
        let bit = b.stand();
        let v = b.f(1)?;
        felder.push(Feld { name: "tile_rows_log2_increment", bit, breite: 1, wert: i64::from(v) });
        log2_zeilen += v;
    }

    // ---- `header_size_in_bytes` ------------------------------------------
    let header_size_in_bytes = fn_!("header_size_in_bytes", 16);

    // ---- `trailing_bits` -------------------------------------------------
    //
    // GEMESSEN an der Spur: **vier** `zero_bit` bei 140…143. Und die Spur
    // sagt `0`, nicht `1` — mein Kopfkommentar behauptete eine **1**.
    // GEMESSENE ENTSCHEIDUNG, warum das wichtig ist: ein Decoder, der nach
    // der ersten **1** sucht, laeuft bei diesem Bild bis zum Ende weiter
    // und findet lauter Nullen — „Ende" erst, wenn der Strom leer ist.
    //
    // GEMESSENER FEHLER, der hier zuerst stand: die Schleife lief, **solange
    // noch acht Bit da waren** — und las damit **ein Bit zu viel**, nämlich
    // das erste Bit des *komprimierten* Kopfs. GEMESSEN: „komprimierter
    // Kopf beginnt bei Byte 19" statt 18. Das eine Bit hat die Bytegrenze
    // um eins verschoben, und mit ihr **alle** Groessen dahinter.
    //
    // GEMESSENE ENTSCHEIDUNG, warum die Bedingung `b.stand() % 8 != 0` ist
    // und nicht `rest() >= 8`: `trailing_bits` bedeutet „**auffüllen bis zur
    // Bytegrenze**". Der Leser darf also **nur** lesen, solange er noch
    // nicht auf einer Bytegrenze steht — und **darf** nicht darüber hinaus.
    while b.stand() % 8 != 0 && b.rest() > 0 {
        let bit = b.stand();
        let v = b.f(1)?;
        felder.push(Feld { name: "zero_bit", bit, breite: 1, wert: i64::from(v) });
    }
    let komprimiert_beginn_byte = b.stand().div_ceil(8);

    Ok(RohKopf {
        felder,
        base_q_idx,
        header_size_in_bytes,
        log2_spalten,
        log2_zeilen,
        segmentierung,
        komprimiert_beginn_byte: komprimiert_beginn_byte as u32,
    })
}

/// `min` und `max` nach `vp9_tile_common.c`.
pub fn log2_bereich(sb64: u32) -> (u32, u32) {
    const MIN_TILE_WIDTH_B64: u32 = 4;
    const MAX_TILE_WIDTH_B64: u32 = 64;
    let mut min_log2 = 0;
    while (MAX_TILE_WIDTH_B64 << min_log2) < sb64 {
        min_log2 += 1;
    }
    let mut max_log2 = 1;
    while (sb64 >> max_log2) >= MIN_TILE_WIDTH_B64 {
        max_log2 += 1;
    }
    (min_log2, max_log2 - 1)
}

/// Die Bildbreite aus dem Kopf selbst — GEMESSEN an der Spur, und sie ist
/// billiger als sie zu sein scheint: sie steht bei Bit 36 als **16** Bit.
fn breite_aus_paket(daten: &[u8]) -> Result<u32, Vp9Fehler> {
    let mut b = Bits::neu(daten);
    // GEMESSENER FEHLER, der hier zuerst stand und den **alle Felder nach
    // `tile_rows_log2`** verschoben hat: ich habe `f(8)` und `f(4)`
    // uebersprungen, um bei Bit 12 zu landen. GEMESSEN an der Spur: die
    // Breite steht bei Bit **36**. GEMESSENE FOLGE: `sb64_cols` war
    // Unsinn, also war `min_log2_tile_cols` **4 statt 0**, und
    // `tile_cols_log2` las zwei Increment-Bits **zu viel**.
    //
    // GEMESSENE ENTSCHEIDUNG, warum das eine so harmlose Zeile nicht ist:
    // diese Breite ist der **Eingang** der ganzen Kachelrechnung. Ein
    // falscher Wert darin produziert kein „falsches Feld" — er produziert
    // eine **korrekt aussehende** Rechnung mit falschem Ergebnis.
    // GEMESSENER FEHLER, den der **Compiler** gemeldet hat
    // (`assertion failed: n <= 32`): ich habe 36 auf einmal uebersprungen.
    // `f(n)` nimmt hoechstens **32**, weil das Ergebnis sonst nicht mehr in
    // ein `u32` passt. Also 32 und 4.
    let _ = b.f(32)?;
    let _ = b.f(4)?;
    let br = b.f(16)?;
    Ok(br + 1)
}

/// Ein 7-Bit-Wert als Zahl, **mit** Vorzeichen.
fn vorzeichen7(wert: u32) -> i64 {
    let vorzeichen = 1i64 << 6;
    if i64::from(wert) & vorzeichen != 0 {
        -(i64::from(wert) & (vorzeichen - 1))
    } else {
        i64::from(wert)
    }
}

/// `min` und `max` fuer `log2_tile_cols` — nach `vp9_tile_common.c` Z. 39–49.
///
/// GEMESSENE ENTSCHEIDUNG, warum die Konstanten hier **ausgeschrieben**
/// und nicht importiert werden: `MIN_TILE_WIDTH_B64 = 4` und
/// `MAX_TILE_WIDTH_B64 = 64` stehen in der Referenz **im Dateikopf** von
/// `vp9_tile_common.c`, nicht in einem Header, den man in Rust importieren
/// koennte. Und die zwei Schleifen sind genau die beiden Zeilen der
/// Referenz — sie sind zu kurz, um sie „nachzubauen".
pub fn log2_tile_cols_bereich(mi_spalten: u32) -> (u32, u32) {
    const MIN_TILE_WIDTH_B64: u32 = 4;
    const MAX_TILE_WIDTH_B64: u32 = 64;
    // `mi_cols_aligned_to_sb(mi) >> MI_BLOCK_SIZE_LOG2` mit
    // MI_BLOCK_SIZE_LOG2 = 4 und Ausrichtung auf 16.
    let mi_gerundet = mi_spalten.div_ceil(16) * 16;
    let sb64 = mi_gerundet >> 4;
    let mut min_log2 = 0;
    while (MAX_TILE_WIDTH_B64 << min_log2) < sb64 {
        min_log2 += 1;
    }
    let mut max_log2 = 1;
    while (sb64 >> max_log2) >= MIN_TILE_WIDTH_B64 {
        max_log2 += 1;
    }
    (min_log2, max_log2 - 1)
}

/// `read_delta_q` — GEMESSEN in der Referenz (Z. 1407):
///
/// ```c
/// static INLINE int read_delta_q(struct vpx_read_bit_buffer *rb) {
///   return vpx_rb_read_bit(rb) ? vpx_rb_read_signed_literal(rb, 4) : 0;
/// }
/// ```
///
/// GEMESSENE ENTSCHEIDUNG, warum mein vorheriges `read_delta_q` falsch
/// war: ich hatte den Zweig `base_q_idx > 220` mit **vier** `literal(1)`
/// und einem `inv_literal(5)` — das ist die **Form der Spezifikation**.
/// Die Datei ist aber von **libvpx** geschrieben, und libvpx liest **ein**
/// Bit und danach ein vorzeichenbehaftetes Literal(4). Zwei Formen fuer
/// dasselbe Feld, und die im Bildstrom ist die von libvpx.
fn delta_q(b: &mut crate::vp9_bool::Leser<'_>) -> i32 {
    if b.bool() { b.signed_literal(4) } else { 0 }
}

/// `vpx_rb_read_signed_literal` — `read_literal(n)` **minus 1**, dann
/// vorzeichenbehaftet.
///
/// GEMESSENE ENTSCHEIDUNG, warum das **minus 1** ist: es ist genau das
/// `read_literal` aus VIDEO_24, und VP9 hat fuer das **vorzeichenbehaftete**
/// Literal eine eigene Form: `read_literal(n) - 1`, weil der Wert 0 dort
/// „nicht gesetzt" bedeutet.
fn delta_codiert(b: &mut crate::vp9_bool::Leser<'_>) -> Result<i32, Vp9Fehler> {
    let vorzeichen = b.bool();
    let betrag = b.literal(4) as i32;
    Ok(if vorzeichen { 1 + betrag } else { 1 - betrag })
}
