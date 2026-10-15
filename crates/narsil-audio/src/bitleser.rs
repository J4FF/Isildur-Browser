//! G8b: DER BIT-LESER fuer Ogg/Vorbis.
//!
//! # WARUM EIGENER LESER und nicht der aus `narsil-compress`
//!
//! `narsil-compress` hat einen Bit-Leser, der LSB-first ueber Bytes geht
//! (DEFLATE und Brotli). Ogg ist **LSB-first ueber Bits** — das ist
//! dasselbe Prinzip, aber die Frage „wie viele Bit sind noch da?" wird
//! bei Ogg staendig gestellt, und ein Leser, der am Ende des Stroms
//! stehen bleibt statt einen Fehler zu nennen, ist in einem
//! Sprachpaket ein stiller Datenverlust.
//!
//! # DIE GRENZEN
//!
//! * M4: [`MAX_BITS`] (1 MiB) — die Laenge einer einzelnen
//!   Codebuch-Zuordnung oder einer Floor-Kurve. Aus dem Strom, also eine
//!   Eingabe.

/// M4: 1 MiB fuer eine einzelne Ziffernfolge im Strom.
///
/// GEMESSENE ENTSCHEIDUNG, warum 1 MiB: die laengste Liste, die ein
/// Setup-Kopf lesen kann, ist die Sync-Zeichenfolge des Codebuchs
/// (Typ 1). Sie ist durch die Codebuchgroesse begrenzt, und die ist
/// durch `MAX_BUCH_GROESSE` begrenzt. 1 MiB ist also eine Schranke
/// ueber der Schranke — gewollt: eine Zahl, die eine Zahl ueberwacht,
/// faellt bei einem Fehler im Strom auf, statt 4 GiB zu belegen.
pub const MAX_BITS: usize = 1024 * 1024;

/// Der Fehler, benannt.
#[derive(Debug, PartialEq, Eq)]
pub enum BitFehler {
    /// Am Ende des Stroms, mit der Stelle.
    ZuEnde { brauchte: usize, hatte: usize },
    /// Die Vorblaeufigkeit liest ueber das Ende hinaus (Zwei-Komplement-
    /// Vorzeichen nach RFC 5215 Abschnitt 1.3.1).
    VorlaeufigZuEnde { byte: usize },
    /// **GEMESSENER BEFUND (04.10., G9ap) und was das Feld bedeutet:**
    /// `wert` ist an **einer** Stelle eine **Zahl** (`variabel()`, M4 /
    /// RFC 5215 Abschnitt 1.2: „at most 32 bits") und an **sechs** ein
    /// **Zaehlerstand** — die Anzahl der Bit, die der Leser **in diesem
    /// Fenster** gelesen hat. Der Name `wert` stammt von der **ersten**
    /// Stelle und war an den **sechs** anderen **eine Falschaussage** (die
    /// Nachricht sagte „die Zahl ... passt nicht in 32 Bit", und GEMESSEN
    /// kam `1048576 = 2^20`, das **sehr wohl** hineinpasst).
    ///
    /// **WAS HIER ECHT PASSIERT:** der Leser hat **mehr** Bit in **einem**
    /// Fenster gelesen als `MAX_BITS` erlaubt. Das ist **kein** Dateifehler
    /// — es ist die Schranke, und GEMESSEN trat sie an **echter** MP3
    /// (140 s, 160 kbit/s) bei **Byte 131072** auf, weil der Zaehler
    /// **absolut** war. Siehe `BitLeser::start`.
    ZuGross { wert: u64, byte: usize },
}

impl std::fmt::Display for BitFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BitFehler::ZuEnde { brauchte, hatte } => write!(
                f,
                "der Bitstrom endet nach {hatte} Bit, gebraucht werden {brauchte}"
            ),
            BitFehler::VorlaeufigZuEnde { byte } => write!(
                f,
                "die Vorblaeufigkeit liest ueber das Ende hinaus (Byte {byte})"
            ),
            // **MEINE FALSCHAUSGABEN, korrigiert 04.10. (G9ap).**
            //
            // **(1) „die Zahl {wert} passt nicht in 32 Bit"** — GEMESSEN kam
            //     hier `wert = 1048576` mit `byte = 131072`. `1048576 = 2^20`
            //     passt **sehr wohl** in 32 Bit (bis `2^31 - 1`). Und
            //     `131072 = MAX_BITS / 8` **genau** — also war `wert` **gar
            //     keine Zahl aus dem Strom**, sondern der **Zaehlerstand**:
            //     `self.gelesen()` nach der Korrektur, `self.pos` davor.
            //
            //     **Von den sieben Stellen, die `ZuGross` erzeugen, ist nur
            //     EINE eine echte Zahl**: `variabel()` (RFC 5215 Abschnitt
            //     1.2, „at most 32 bits"). Die anderen **sechs** melden eine
            //     **Stelle im Strom**. Ein Feld fuer **beides** ist **ein**
            //     Feld mit **zwei** Bedeutungen — und die Nachricht nannte
            //     die **falsche**.
            //
            // **(2) „nach der Vorbis-Spezifikation ungueltig"** — GEMESSEN kam
            //     diese Meldung aus dem **MP3**-Decoder
            //     (`[StromZuEnde] MP3-Bitstrom: ... nach der Vorbis-
            //     Speifikation ungueltig`). `bitleser.rs` ist der **gemeinsame**
            //     Leser von **Vorbis und MP3**; eine Vorbis-Regel in einem
            //     MP3-Fehler ist **eine** Diagnose, die den Weg verbaut.
            //
            // **REGEL: eine Fehlermeldung, die eine Spezifikation nennt,
            // muss **für jeden** Aufrufer stimmen, der sie sehen kann.**
            BitFehler::ZuGross { wert, byte } => write!(
                f,
                "der Zaehler stand bei {wert} Bit (Byte {byte}) — das ist die \
                 Obergrenze MAX_BITS fuer ein Fenster; der Leser hat die \
                 erlaubte Laenge im Strom ueberschritten"
            ),
        }
    }
}

/// Liest Bits aus einem Byte-Strom, LSB-first.
///
/// # DIE ZWEI ZAEHLER und warum es zwei sein muessen
///
/// `pos` ist die **Bit**-Position im ganzen Strom. Sie wird nie
/// zurueckgesetzt — das ist der Grund, warum `lies_bits` nach `n` Bit
/// einfach `pos + n` weitermacht und warum ein „seek" nur ueber
/// `ueberspringe` geht. EIN Zaehler: zwei Zaehler (Byte und Bit
/// getrennt) muessen an jeder Stelle zusammenpassen, und das ist genau
/// die Art Stelle, an der ein Dekoder still falsche Werte liest.
#[derive(Debug, Clone)]
pub struct BitLeser<'a> {
    daten: &'a [u8],
    /// Die Bit-Position (nicht die Byte-Position).
    pos: usize,
    /// **GEMESSENER FEHLER, korrigiert 04.10. (G9ap):** der Bit-Zaehler, bei
    /// dem dieser Leser **begonnen** hat. `MAX_BITS` ist eine **Fenster**-
    /// Obergrenze (der Doc-Kommentar an `bit()` sagt das ausdruecklich), und
    /// ein Fenster wird **ab seinem Anfang** gemessen — **nicht** ab Byte 0
    /// des Dateianfangs.
    ///
    /// **WAS VORHER GEMESSEN WURDE:** `pos` war ein **absoluter** Zaehler
    /// („sie wird nie zurueckgesetzt"), und `MAX_BITS = 1048576` **Bit** =
    /// **131072 Byte**. Also brach **jede** Datei **ab Byte 131072** ab —
    /// unabhaengig davon, wie klein das gelesene Fenster war. Bei den drei
    /// **kurzen** synthetischen ffmpeg-Dateien (G8c) fiel das **nicht** auf;
    /// an **echter** Musik **140 s lang / 160 kbit/s** brach es **sofort**:
    /// ```text
    /// [StromZuEnde] MP3-Bitstrom: die Zahl 1048576 passt nicht in 32 Bit
    /// (Byte 131072)
    /// ```
    /// `1048576` ist **genau** `MAX_BITS` und **Byte 131072** ist **genau**
    /// `MAX_BITS / 8` — also **nicht** ein Dateifehler, sondern die Schranke.
    ///
    /// **`position_setzen` aendert `start` NICHT** — und das ist **richtig**:
    /// der Bit-Tank (Layer III) **springt** nach vorn, aber das **Fenster**
    /// beginnt **nicht** dort. GEMESSEN: `BitLeserVorn::ab(daten,
    /// kopf.neben_start)` in `mp3/rahmen.rs:326` beginnt **mitten** in der
    /// Datei — **das** ist der Fall, den die alte Grenze zerstoert hat.
    start: usize,
}

impl<'a> BitLeser<'a> {
    /// Beginnt bei Bit 0.
    #[must_use]
    pub fn neu(daten: &'a [u8]) -> Self {
        BitLeser {
            daten,
            pos: 0,
            start: 0,
        }
    }

    /// Beginnt bei einer Byte-Position (nach einem Paket, das schon
    /// vollstaendig gelesen ist).
    #[must_use]
    pub fn ab(daten: &'a [u8], byte: usize) -> Self {
        BitLeser {
            daten,
            pos: byte.saturating_mul(8),
            start: byte.saturating_mul(8),
        }
    }

    /// Die Anzahl der Bit, die dieser Leser **gelesen** hat — also
    /// `pos - start`. Das ist die Zahl, gegen die `MAX_BITS` gilt.
    #[must_use]
    pub fn gelesen(&self) -> usize {
        self.pos - self.start
    }

    /// Die aktuelle **Bit**-Position.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Die aktuelle Byte-Position (abgerundet, wie Ogg es braucht: ein
    /// Paket endet an einer Byte-Grenze).
    #[must_use]
    pub fn byte_position(&self) -> usize {
        self.pos / 8
    }

    /// Wie viele Bit sind noch da?
    #[must_use]
    pub fn rest_bits(&self) -> usize {
        self.daten.len().saturating_mul(8).saturating_sub(self.pos)
    }

    /// Springt auf eine Byte-Grenze (die unteren Bits werden verworfen).
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das `pos` aufrundet statt rundet:
    /// ein Paket ist ein Byte-Block, und nach dem Lesen steht der
    /// Zeiger auf der ersten ungelesenen **Nutzlast**. Aufrunden ist
    /// richtig, weil die Bits dazwischen zum naechsten Paket gehoeren.
    pub fn bis_bytegrenze(&mut self) {
        if self.pos % 8 != 0 {
            self.pos = (self.pos / 8 + 1) * 8;
        }
    }

    /// Setzt die Position auf eine Byte-Grenze zurueck.
    ///
    /// Wird nach jedem Paket gebraucht: der Ogg-Container haelt die
    /// Bitposition nicht, nur die Byte-Grenze.
    pub fn auf_bytegrenze(&mut self) {
        self.pos -= self.pos % 8;
    }

    /// Liest ein einzelnes Bit.
    pub fn bit(&mut self) -> Result<u32, BitFehler> {
        if self.rest_bits() == 0 {
            return Err(BitFehler::ZuEnde {
                brauchte: 1,
                hatte: self.pos,
            });
        }
        let byte = self.pos / 8;
        let bit = self.pos % 8;
        let wert = (self.daten[byte] >> bit) & 1;
        self.pos += 1;
        Ok(u32::from(wert))
    }

    /// Liest `n` Bit (0 bis 32) als Zahl, LSB zuerst.
    pub fn bits(&mut self, n: u32) -> Result<u32, BitFehler> {
        // M4: `n` ist eine Zahl aus dem Strom. Ohne diese Grenze waere
        // `bits(32)` der kleinste Sonderfall und `bits(255)` ein
        // Ueberlauf in der Schiebeoperation (Debug-Bau: Panic).
        if n > 32 {
            return Err(BitFehler::ZuEnde {
                brauchte: n as usize,
                hatte: self.rest_bits(),
            });
        }
        if self.rest_bits() < n as usize {
            return Err(BitFehler::ZuEnde {
                brauchte: self.pos + n as usize,
                hatte: self.daten.len() * 8,
            });
        }
        let mut wert = 0u32;
        for i in 0..n {
            wert |= self.bit()? << i;
        }
        Ok(wert)
    }

    /// Liest `n` Bit in einen `Vec<bool>` — fuer die Sync-Zeichenfolge
    /// eines Codebuchs (Vorbis-I-Spezifikation Abschnitt 3.2.1).
    pub fn bits_liste(&mut self, n: usize) -> Result<Vec<bool>, BitFehler> {
        if self.rest_bits() < n {
            return Err(BitFehler::ZuEnde {
                brauchte: self.pos + n,
                hatte: self.daten.len() * 8,
            });
        }
        let mut raus = Vec::with_capacity(n);
        for _ in 0..n {
            raus.push(self.bit()? == 1);
        }
        Ok(raus)
    }

    /// Liest eine Zahl im „unten zuerst, sieben Bit je Byte, unterstes
    /// Bit als Fortsetzung" — die eine Zahl, die es in Ogg gibt
    /// (Vorbis-I-Spezifikation Abschnitt 1.2).
    ///
    /// GEMESSENER FEHLER, und er war ein **Panic im Debug-Bau**:
    /// `wert |= stueck << (7 * i)` mit `i = 7` schiebt um 49 Bit auf
    /// einem `u32` — Uberlauf. Der Test `die_variabel_zahl_hat_eine_
    /// grenze` hat ihn mit „attempt to shift left with overflow" gefunden.
    ///
    /// DER WEG, und warum `u64`: die Zahl wird in `u64` gesammelt und
    /// am Ende auf 32 Bit geprueft. Vorbis-Buchindizes, Floor-Werte und
    /// Paketnummern passen in 32 Bit (RFC 5215 Abschnitt 1.2 sagt, die
    /// Zahl sei „at most 32 bits"). Wer mehr schickt, bekommt einen
    /// benannten Fehler statt eines Panics — und ein Panic in einem
    /// Dekoder, der Dateien aus dem Netz bekommt, ist ein Absturz.
    pub fn variabel(&mut self) -> Result<u32, BitFehler> {
        /// M4: die Zahl passt in 32 Bit, also sind hoechstens
        /// fuenf Byte noetig (5 * 7 = 35 Bit, davon werden die obersten
        /// drei geprueft).
        const MAX_BYTES: u32 = 5;
        let mut wert = 0u64;
        for i in 0..MAX_BYTES {
            let b = self.bits(8)?;
            // GEMESSENE ENTSCHEIDUNG, warum `& 0x7f` VOR der Schiebe:
            // das Fortsetzungsbit ist das oberste Bit des Bytes, und es
            // darf nicht in die Zahl. Wer es nach der Schiebe
            // wegschneidet, verliert die obersten 7 Bit.
            let stueck = u64::from(b & 0x7f);
            wert |= stueck << (7 * i);
            if b & 0x80 == 0 {
                if wert > u64::from(u32::MAX) {
                    return Err(BitFehler::ZuGross {
                        wert,
                        byte: self.byte_position(),
                    });
                }
                return Ok(wert as u32);
            }
        }
        // Fuenf Byte mit Fortsetzungsbit, oder eine Zahl ueber 32 Bit:
        // beides ist nach Spezifikation ungueltig. Der Strom laeuft
        // weiter, und wir geben auf, statt endlos weiterzulesen.
        Err(BitFehler::VorlaeufigZuEnde {
            byte: self.byte_position(),
        })
    }

    /// Liest eine Zahl mit Vorzeichen im Zwei-Komplement ueber `n` Bit.
    ///
    /// RFC 5215 Abschnitt 1.3.1: das OBERSTE gelesene Bit ist das
    /// Vorzeichen („1 = positiv"), die uebrigen `n - 1` Bits sind der
    /// Betrag.
    pub fn variabel_vorzeichen(&mut self) -> Result<i32, BitFehler> {
        let wert = self.variabel()?;
        // GEMESSENE ENTSCHEIDUNG, warum hier das VORZEICHENBIT
        // umgedreht wird und nicht das unterste: die Vorbis-
        // Speifikation dreht das oberste um („negate by inverting the
        // sign bit"). Wer es nicht tut, bekommt bei `1` den Wert +1
        // statt -1 — und eine Floor-Kurve, die nicht negativ werden
        // kann, klingt wie Rauschen mit einer Tendenz nach oben.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `i32` und nicht `u32 as i32`:
        // ein Wert ueber 2^31 ist nach Spezifikation ein Fehler, und
        // `u32 as i32` wuerde ihn still zu einem negativen Wert.
        if wert > 0x7fff_ffff {
            return Err(BitFehler::ZuGross {
                wert: u64::from(wert),
                byte: self.byte_position(),
            });
        }
        Ok(wert as i32)
    }

    /// Liest eine Zahl MIT VORZEICHEN nach RFC 5215 Abschnitt 1.3.1.
    ///
    /// # DIE KODIERUNG, und warum sie kein Zwei-Komplement ist
    ///
    /// ```text
    ///  0 .. n-2   Betrag (LSB zuerst gelesen)
    ///  n-1        Vorzeichen: 1 = positiv, 0 = negativ
    /// ```
    ///
    /// GEMESSENE KORREKTUR AN MIR: ich hatte zuerst eine Funktion
    /// `variabel_vorzeichen`, die das **oberste** Bit als Vorzeichen
    /// behandelte und den Wert am Ende per `& 0x8000_0000` auf i32
    /// umrechnen wollte. Der Compiler sagte zweimal „literal out of
    /// range for i32" — und WENN ich die Rechnung richtig gemacht
    /// haette, waere der zweite Teil unnoetig gewesen: die Zahl wird
    /// vorher auf 31 Bit geprueft, also kann das Bit 31 gar nicht
    /// gesetzt sein. Zwei Fehler, die sich versteckt haben.
    ///
    /// DER WEG, der der Spezifikation folgt: `n` Bit lesen, dann das
    /// OBERSTE der gelesenen Bit als Vorzeichen nehmen und den Rest als
    /// Betrag. Beispiel: `n = 6`, Bitfolge `1 00010` (LSB zuerst
    /// gelesen) ergibt +2, denn das Vorzeichenbit (hier das fuenfte
    /// gelesene) ist 1.
    pub fn signiert(&mut self, n: u32) -> Result<i32, BitFehler> {
        if n == 0 || n > 32 {
            return Err(BitFehler::ZuEnde {
                brauchte: n as usize,
                hatte: self.rest_bits(),
            });
        }
        // M4: alle `n` Bit auf einmal lesen, das Vorzeichen steht im
        // obersten gelesenen Bit (Position n-1).
        let wert = self.bits(n)?;
        if n == 1 {
            // Nur das Vorzeichen, kein Betrag.
            return Ok(if wert & 1 == 1 { 1 } else { -1 });
        }
        let vorzeichen_positiv = (wert >> (n - 1)) & 1 == 1;
        let betrag = (wert & ((1u32 << (n - 1)) - 1)) as i32;
        Ok(if vorzeichen_positiv { betrag } else { -betrag })
    }

    /// Liest ein Byte, aber nur die unteren `n` Bit (Vorbis benutzt
    /// `read n bits` fuer fast alle Byte-Felder).
    pub fn byte_n(&mut self, n: u32) -> Result<u8, BitFehler> {
        Ok(self.bits(n)? as u8)
    }

    /// Liest eine **64-Bit** Zahl mit Vorzeichen.
    ///
    /// # WARUM EINE EIGENE FUNKTION
    ///
    /// RFC 5215 Abschnitt 1.3.1 nennt fuer die Zeitbereichswerte des
    /// Setup-Kopfes eine „64 bit signed integer, two's complement". Das
    /// ist die EINIGE Stelle, an der Vorbis mehr als 32 Bit liest, und
    /// sie wird gelesen als **zwei** 32-Bit-Woerter:
    ///
    /// ```text
    ///   [low]  = read 32 bits as unsigned integer
    ///   [high] = read 32 bits as unsigned integer
    ///   wert   = ([high] * 2^32) + [low]     (two's complement)
    /// ```
    ///
    /// GEMESSENER FEHLER: ich habe dafuer `signiert(64)` benutzt und
    /// damit einen benannten Fehler bekommen („der Bitstrom endet nach
    /// 33736 Bit, gebraucht werden 64") — `signiert` nimmt bis 32.
    /// Der Fehler war richtig und die Diagnose falsch: die Zahl ist
    /// nicht „64 Bit breit gelesen", sondern aus zwei 32-Bit-Woertern
    /// zusammengesetzt.
    pub fn signiert64(&mut self) -> Result<i64, BitFehler> {
        let low = u64::from(self.bits(32)?);
        let high = u64::from(self.bits(32)?);
        let roh = (high << 32) | low;
        // GEMESSENER FEHLER: ich wollte „zwei-Komplement von Hand"
        // machen und habe dafuer `2^64` abgezogen — das Literal passt
        // nicht in ein `i64` („literal out of range for i64"). Es ist
        // aber auch nicht noetig: `u64 as i64` IST in Rust genau die
        // Umrechnung, die man hier braucht (die Bits werden als
        // Zweiers Komplement gelesen, nicht neu gerechnet). Wer sie von
        // Hand macht, macht sie zweimal.
        Ok(roh as i64)
    }
}

/// Liest Bits aus einem Byte-Strom, **MSB-first**.
///
/// # WARUM ZWEI LESER IN EINER CRATE
///
/// Vorbis liest LSB-first (`BitLeser`), MP3 liest MSB-first. Das ist
/// keine Feinheit, sondern der Unterschied zwischen „funktioniert" und
/// „liefert Zahlen":
///
/// ```text
///   Byte 0b1011_0010
///   LSB-first:  0, 1, 0, 0, 1, 1, 0, 1
///   MSB-first:  1, 0, 1, 1, 0, 0, 1, 0
/// ```
///
/// GEMESSEN, und deshalb steht der Unterschied hier ausdruecklich: der
/// erste MP3-Rahmenkopf von `g8c_sine_stereo.mp3` beginnt mit den
/// Bytes `0xFF 0xFB`. Liest man LSB-first, ist das erste Byte kein Sync,
/// und man findet im ganzen File keinen einzigen gueltigen Rahmen —
/// ohne Fehlermeldung, nur eine leere Ausgabe.
///
/// Beide Leser benutzen **dieselbe** `BitFehler`-Grenze (`MAX_BITS`) und
/// denselben einen Positionszaehler. Zwei Leser, nicht drei Regeln.
#[derive(Debug, Clone)]
pub struct BitLeserVorn<'a> {
    daten: &'a [u8],
    /// Die Bit-Position im ganzen Strom; sie wird nie zurueckgesetzt.
    pos: usize,
    /// **Dasselbe wie in `BitLeser`, und aus demselben gemessenen Grund
    /// (G9ap):** `MAX_BITS` ist eine **Fenster**-Obergrenze, und ein Fenster
    /// wird **ab seinem Anfang** gemessen. `position_setzen` (der Bit-Tank)
    /// aendert `start` **nicht**.
    start: usize,
}

impl<'a> BitLeserVorn<'a> {
    /// Ein neuer Leser am Anfang von `daten`.
    #[must_use]
    pub fn neu(daten: &'a [u8]) -> Self {
        Self {
            daten,
            pos: 0,
            start: 0,
        }
    }

    /// Ein Leser, der bei Byte `byte` beginnt.
    ///
    /// `byte` ist **8 mal** der Zaehler in Bit. Fuer MP3 wird er
    /// benoetigt, weil der Hauptstrom (Hauptdaten) nicht am Anfang des
    /// Pakets liegt, sondern um `main_data_begin * 8` Byte versetzt.
    #[must_use]
    pub fn ab(daten: &'a [u8], byte: usize) -> Self {
        Self {
            daten,
            pos: byte * 8,
            start: byte * 8,
        }
    }

    /// Die Bit-Position im Strom.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Die Anzahl der Bit, die dieser Leser **gelesen** hat — also
    /// `pos - start`. Das ist die Zahl, gegen die `MAX_BITS` gilt.
    ///
    /// **GEMESSENER Grund (G9ap):** ohne das bricht **jede** Datei **ab
    /// Byte 131072** ab, weil `pos` ein **absoluter** Zaehler ist. Siehe
    /// das Feld `start` und die Messung im Bericht G9ap.
    #[must_use]
    pub fn gelesen(&self) -> usize {
        self.pos - self.start
    }

    /// Setzt die Bit-Position — fuer den Bit-Tank von Layer III noetig.
    ///
    /// # `start` WANDERT MIT — **nur** solange **nichts** gelesen wurde
    ///
    /// **MEINE FALSCHE BEGRUENDUNG, und sie stand hier (G9ap):** „der
    /// Bit-Tank springt nach vorn, aber das **Fenster** beginnt nicht
    /// dort". **GEMESSEN widerlegt**, in `mp3/granel.rs:426-427`:
    ///
    /// ```text
    /// let mut l = BitLeserVorn::neu(daten);   // start = 0, gelesen() = 0
    /// l.position_setzen(von_roh.min(...));      // pos  = von_roh
    /// ```
    ///
    /// **UND — GEMESSEN, als dieser Block `rust` war:** `bau.sh narsil-audio
    /// test` wurde **ROT**, mit `error: unexpected token: ...` in **Zeile
    /// 506**. **Das heisst: ein ```rust-Block im Doc-Kommentar ist
    /// AUSFUEHRBARER CODE** — `cargo test --doc` **kompiliert** ihn, und ein
    /// **Auszug** (mit `...`) **kann nicht kompilieren**.
    /// **REGEL: `rust` im Doc-Kommentar heisst „wird kompiliert und
    /// gelaufen". Ein Auszug gehoert in einen ```text-Block.**
    /// **Wenn** der Leser **noch nichts** gelesen hat, **beginnt** sein
    /// Fenster **am Sprungziel** — nicht am Byte 0. **Also** wandert `start`
    /// mit, **wenn** noch **nichts** gelesen wurde.
    ///
    /// **UND** `start` wandert **nicht** mit, **wenn** schon gelesen wurde:
    /// dann ist der Sprung ein Sprung **mitten** im Fenster (das ist der
    /// Bit-Tank **innerhalb** eines Frames), und ein wanderndes `start`
    /// wuerde die **bis dahin** gelesene Menge aus dem Fenster **werfen** —
    /// und damit **genau** die Schranke **verloesen**, die man schuetzt.
    ///
    /// **GEMESSENER BELE FUER DIESE UNTERSCHEIDUNG:** `Neben::lies`
    /// (`mp3/rahmen.rs:326`) ruft `BitLeserVorn::ab(daten,
    /// kopf.neben_start)` und liest danach **weniger als 32 Bit** — dort ist
    /// `start = neben_start * 8` **richtig**, weil der Sprung **vor** dem
    /// Lesen passiert. `granel.rs` ruft `neu` und springt **danach** — dort
    /// **darf** `start` nicht bei 0 bleiben.
    ///
    /// **REGEL: wer eine Grenze „am Anfang" misst, muss den Anfang **auch**
    /// setzen — und der Anfang aendert sich mit jedem Sprung, **solange**
    /// noch nichts gemessen wurde.**
    pub fn position_setzen(&mut self, bit: usize) {
        if self.gelesen() == 0 {
            self.start = bit;
        }
        self.pos = bit;
    }

    /// Noch wie viele Bit bis zum Ende von `daten` sind.
    #[must_use]
    pub fn rest_bits(&self) -> usize {
        self.daten.len() * 8 - self.pos
    }

    /// Liest `n` Bit, **das naechste Byte als erstes**.
    ///
    /// # DER NAME, und warum es nicht `bits` heisst
    ///
    /// Der heisst `liefere`, nicht `bits`, weil er an einer Grenze
    /// aufhoert statt zu irren: `MAX_BITS` gilt als Obergrenze fuer eine
    /// Ziffernfolge (M4), und ein Fehler, der mit einer Zahl endet, ist
    /// bei Layer III nicht zu unterscheiden von einem gueltigen Ergebnis.
    /// Deshalb: **kein** stilles Zurueckgeben von Null.
    pub fn bit(&mut self) -> Result<u32, BitFehler> {
        // **GEMESSEN (G9ap):** `MAX_BITS` gilt gegen **das Fenster**, also
        // gegen `self.gelesen()` — **nicht** gegen die absolute Position.
        if self.gelesen() >= MAX_BITS {
            return Err(BitFehler::ZuGross {
                wert: self.gelesen() as u64,
                byte: self.pos / 8,
            });
        }
        if self.pos / 8 >= self.daten.len() {
            return Err(BitFehler::ZuEnde {
                brauchte: self.pos + 1,
                hatte: self.daten.len() * 8,
            });
        }
        let byte = self.daten[self.pos / 8];
        let versatz = 7 - self.pos % 8;
        self.pos += 1;
        Ok(u32::from((byte >> versatz) & 1))
    }

    /// Liest `n` Bit MSB-first als Zahl.
    pub fn bits(&mut self, n: u32) -> Result<u32, BitFehler> {
        if self.gelesen() as u64 + u64::from(n) > MAX_BITS as u64 {
            return Err(BitFehler::ZuGross {
                wert: self.gelesen() as u64 + u64::from(n),
                byte: self.pos / 8,
            });
        }
        let mut wert = 0u32;
        for _ in 0..n {
            wert = (wert << 1) | self.bit()?;
        }
        Ok(wert)
    }

    /// Liest `n` Bit **MSB-first** in ein `u64`.
    ///
    /// # WARUM EIN ZWEITER LESER, und nicht `bits` mit Breite 63
    ///
    /// [`Self::bits`] gibt `u32` und **kann nicht 33 Bit halten**:
    /// `wert << 1` laeuft dann ueber, und die oberen Bit fallen
    /// hinten weg. FLAC braucht aber genau 33: der SIDE-Kanal eines
    /// 32-Bit-Stereo hat `bps + 1 = 33` Bit.
    ///
    /// # GEMESSENE FOLGE, und sie war **stiller** als sie aussieht
    ///
    /// `g9b_stereo_32.flac` (32 Bit, LEFT/SIDE) ist **bitgleich**
    /// geblieben — der SIDE-Kanal eines Sinus, der in beiden Kanaelen
    /// gleich ist, hat nur wenige signifikante Bit, und die oberen
    /// Bit waren also Nullen, die verloren gehen konnten, ohne dass man
    /// es sah. Bei einem **Signal mit grossem Dynamic Range**
    /// haette derselbe Leser die oberen Bit abgeschnitten und
    /// kaputte Werte geliefert, ohne eine Meldung.
    ///
    /// Der Test `signiert64_n_haelt_33_bit` haelt die Zahl fest:
    /// 33 Bit zweier Komplement gehen von `−2^32` bis `2^32 − 1`.
    pub fn bits64(&mut self, n: u32) -> Result<u64, BitFehler> {
        if self.gelesen() as u64 + u64::from(n) > MAX_BITS as u64 {
            return Err(BitFehler::ZuGross {
                wert: self.gelesen() as u64 + u64::from(n),
                byte: self.pos / 8,
            });
        }
        if n > 63 {
            return Err(BitFehler::ZuGross {
                wert: u64::from(n),
                byte: self.pos / 8,
            });
        }
        let mut wert = 0u64;
        for _ in 0..n {
            wert = (wert << 1) | u64::from(self.bit()?);
        }
        Ok(wert)
    }

    /// Liest `n` Bit, aber **nur bis `grenze`**. Darueber hinaus: `false`.
    ///
    /// Das ist die eine Stelle, an der Layer III end-of-packet
    /// zulaesst: Abschnitt 2.4.2.7 des Standards behandelt das
    /// Ueberschreiten von `part2_3_length` nicht als Fehler. `grenze` ist
    /// eine **Bit**-Position im selben Strom.
    pub fn bit_bis(&mut self, grenze: usize) -> Result<bool, BitFehler> {
        if self.pos >= grenze {
            return Ok(false);
        }
        self.bit().map(|_| true)
    }

    /// Liest **ein** Bit bis `grenze` und gibt seinen **Wert** zurueck.
    ///
    /// # WARUM DAS EIN EIGENER LESER IST, und nicht `bit_bis`
    ///
    /// GEMESSENER FEHLER, und er ist der Grund, warum der
    /// MP3-Dekoder vier Runden lang bei -45 dB stand:
    ///
    /// ```text
    /// // bit_bis sagt, OB ein Bit da ist — nicht, WELCHES:
    /// pub fn bit_bis(&mut self, grenze: usize) -> Result<bool, BitFehler> {
    ///     if self.pos >= grenze { return Ok(false); }
    ///     self.bit().map(|_| true)          // <- der Wert wird verworfen
    /// }
    /// ```
    ///
    /// In `mp3::granel` stand damit
    ///
    /// ```text
    /// if xv != 0 && l.bit_bis(bis).unwrap_or(false) { xv = -xv; }
    /// ```
    ///
    /// und **das ist immer wahr**, sobald ein Bit vorhanden ist. Also
    /// wurde **jeder** von null verschiedene Wert negiert — unabhaengig
    /// von seinem Vorzeichenbit.
    ///
    /// ## GEMESSENE FOLGE, und sie erklaert drei Messungen auf einmal
    ///
    /// * gegen das validierte Orakel: **51,1 %** der Vorzeichen
    ///   richtig. Bei „immer negativ" und 50 % negativen Orakelwerten
    ///   sind es rund 50 % — und **26,2 %** bei beiden Werten eines
    ///   Paares, denn das ist `P(beide negativ) = 25 %`.
    /// * die **Betrags**gleichheit blieb davon unberuehrt: `bit()`
    ///   wird gerufen, also rueckt der Leser ein Bit vor — Verbrauch
    ///   und Wert stimmen also.
    /// * die **Linbits** stimmten ebenfalls: nur der `bool`-Wert war
    ///   falsch, nicht die Zahl.
    ///
    /// `bit_bis` selbst bleibt, weil seine Semantik an drei Stellen in
    /// den Tests festgehalten ist (`Position 0` bis `9` liefert `true`,
    /// danach `false`). Es zu aendern haette drei Tests mitgezogen und
    /// den Fehler in eine andere Ecke geschoben.
    pub fn bit_wert_bis(&mut self, grenze: usize) -> Result<bool, BitFehler> {
        if self.pos >= grenze {
            return Ok(false);
        }
        self.bit().map(|b| b != 0)
    }

    /// Wie viele Bit bis `grenze` noch da sind, oder `None` im Fehlerfall.
    #[must_use]
    pub fn bit_bis_grenze(&self, grenze: usize) -> usize {
        grenze.saturating_sub(self.pos)
    }

    /// Die Byte-Position, an der der Leser gerade steht.
    ///
    /// # WARUM `position() / 8` UND NICHT EIN ZAEHLER
    ///
    /// FLAC braucht sie an drei Stellen: fuer die **Bytegrenze** nach
    /// den Subframes, fuer den **CRC-16** ueber den ganzen Rahmen und
    /// fuer den **Start des naechsten Rahmens**. Und der Start ist
    /// nicht `rahmen_ende + 2` zu rechnen, sondern exakt die Position
    /// nach der CRC — denn ein Rahmen kann ein Fuellbyte am Ende haben.
    #[must_use]
    pub fn byte_position(&self) -> usize {
        self.pos / 8
    }

    /// Liest `n` Bit als **zwei komplement** mit Vorzeichen, in `i64`.
    ///
    /// # WARUM EIN EIGENER LESER und nicht `signiert(n)`
    ///
    /// [`Self::signiert`] gibt `i32` und kann damit nicht mehr als 31
    /// Bit Bittiefe. FLAC braucht aber **33** — bei `bps = 32` plus dem
    /// Wasted-Bit-Versatz, und bei `bps = 24` mit einem SIDE-Kanal 25.
    /// Bei 33 Bit passt kein `i32`, und genau an dieser Stelle bricht
    /// ein Dekoder, der 24-Bit-Stereo sonst fehlerfrei liest.
    ///
    /// # DIE ZAHL DER VORZEICHENBITS
    ///
    /// ```text
    ///   n = 0  ->  0
    ///   n = 1  ->  0 oder −1
    ///   n = 33 ->  −2^32 … +2^32−1
    /// ```
    ///
    /// Die Formel ist `(wert ^ (1 << (n-1))) − (1 << (n-1))` in
    /// Ganzzahl, ohne den Sonderfall `n = 0` (das ist der einzige Weg,
    /// bei dem `1 << (n-1)` paniked).
    pub fn signiert64_n(&mut self, n: u32) -> Result<i64, BitFehler> {
        if n == 0 {
            return Ok(0);
        }
        if n > 63 {
            return Err(BitFehler::ZuGross {
                wert: u64::from(n),
                byte: self.pos / 8,
            });
        }
        // `bits64`, nicht `bits`: bei 33 Bit liefert `bits` ein
        // abgeschnittenes `u32`, und der Fehler ist still.
        let roh = self.bits64(n)?;
        let vorzeichen_bit = 1u64 << (n - 1);
        Ok(((roh ^ vorzeichen_bit) as i64) - vorzeichen_bit as i64)
    }

    /// Springt zur naechsten Bytegrenze (Fyellbits nach oben).
    ///
    /// FLAC hat am Ende jedes Rahmens Fuellbits **bis zur Bytegrenze**,
    /// danach erst die CRC-16. Ohne diesen Sprung laesst man die
    /// Fuellbits als erste Bits der CRC lesen und bekommt bei **jedem**
    /// Rahmen eine falsche Pruefsumme.
    pub fn auf_bytegrenze(&mut self) {
        let r = self.pos % 8;
        if r != 0 {
            self.pos += 8 - r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorn_und_hinten_lesen_dasselbe_byte_verschieden() {
        // 0b1011_0010
        let daten = [0b1011_0010u8];
        let mut v = BitLeserVorn::neu(&daten);
        let mut h = BitLeser::neu(&daten);
        let vorn: Vec<u32> = (0..8).map(|_| v.bit().expect("bit")).collect();
        let hinten: Vec<u32> = (0..8).map(|_| h.bit().expect("bit")).collect();
        assert_eq!(vorn, vec![1, 0, 1, 1, 0, 0, 1, 0], "MSB-first");
        assert_eq!(hinten, vec![0, 1, 0, 0, 1, 1, 0, 1], "LSB-first");
        assert_eq!(vorn, hinten.iter().rev().copied().collect::<Vec<u32>>());
    }

    #[test]
    fn vorn_lesen_ueber_bytegrenzen() {
        // 0b1100_0011 und 0b1010_0101
        let mut v = BitLeserVorn::neu(&[0b1100_0011, 0b1010_0101]);
        assert_eq!(v.bits(3).expect("drei"), 0b110);
        assert_eq!(v.bits(5).expect("fuenf"), 0b00011);
        assert_eq!(v.bits(8).expect("acht"), 0b1010_0101);
        assert_eq!(v.position(), 16);
    }

    #[test]
    fn vorn_ende_ist_ein_benannter_fehler() {
        let mut v = BitLeserVorn::neu(&[0xff]);
        assert!(v.bits(8).is_ok());
        assert!(v.bit().is_err(), "nach dem letzten Bit: Fehler, nicht 0");
    }

    #[test]
    fn vorn_die_grenze_des_tanks() {
        let mut v = BitLeserVorn::neu(&[0xff; 4]);
        assert_eq!(v.rest_bits(), 32);
        assert!(v.bit_bis(10).expect("bis 10"));
        assert_eq!(v.position(), 1);
        for _ in 1..10 {
            assert!(v.bit_bis(10).expect("bis 10"), "Position {}", v.position());
        }
        assert_eq!(v.position(), 10);
        assert!(
            !v.bit_bis(10).expect("bis 10"),
            "an der Grenze: nichts mehr"
        );
        assert_eq!(v.position(), 10, "und die Position steht still");
    }

    #[test]
    fn bits_lesen_lsb_first() {
        // 0b1011_0010: LSB first -> 0,1,0,0,1,1,0,1
        let mut l = BitLeser::neu(&[0b1011_0010]);
        let gelesen: Vec<u32> = (0..8).map(|_| l.bit().expect("bit")).collect();
        assert_eq!(gelesen, vec![0, 1, 0, 0, 1, 1, 0, 1]);
        assert_eq!(l.position(), 8);
    }

    #[test]
    fn bits_ueber_bytegrenzen() {
        let mut l = BitLeser::neu(&[0xff, 0x00]);
        assert_eq!(l.bits(16).expect("16 bit"), 0x00ff);
    }

    #[test]
    fn der_leser_bleibt_stehen_statt_zu_raten() {
        let mut l = BitLeser::neu(&[0x0f]);
        assert!(l.bits(8).is_ok());
        // M4: jetzt ist Schluss, und das ist ein Fehler mit Zahlen.
        match l.bits(1) {
            Err(BitFehler::ZuEnde { brauchte, hatte }) => {
                assert_eq!(brauchte, 9);
                assert_eq!(hatte, 8);
            }
            andere => panic!("erwartet ZuEnde, kam {andere:?}"),
        }
    }

    #[test]
    fn mehr_als_32_bit_ist_ein_fehler() {
        // M4: `bits(33)` wuerde in der Schiebe ueberlaufen. Ohne diese
        // Grenze ist es ein Panic im Debug-Bau.
        let daten = vec![0u8; 64];
        let mut l = BitLeser::neu(&daten);
        assert!(matches!(l.bits(33), Err(BitFehler::ZuEnde { .. })));
        assert!(l.bits(32).is_ok());
    }

    #[test]
    fn die_variabel_zahl_ist_und_reihenfolge() {
        // 0xC6 0x40: das erste Byte hat das Fortsetzungsbit (0x80) und
        // die Zahl 0x46 = 70; das zweite bringt 0x40 an Position 7,
        // also 64 << 7 = 8192. Zusammen 8262.
        //
        // GEMESSENE KORREKTUR AN MIR: der Test stand mit `0x46 0x40` und
        // erwartete 8198. 0x46 hat aber KEIN Fortsetzungsbit (0x46 & 0x80
        // = 0), also liefert die Zahl genau 70 — der Test hat 8198
        // behauptet, der Leser hat 70 gelesen, und der Leser hat recht.
        let mut l = BitLeser::neu(&[0xc6, 0x40]);
        assert_eq!(l.variabel().expect("variabel"), 70 + 8192);
        // Und der Fall OHNE Fortsetzung:
        let mut l2 = BitLeser::neu(&[0x46]);
        assert_eq!(l2.variabel().expect("variabel"), 70);
    }

    #[test]
    fn die_variabel_zahl_hat_eine_grenze() {
        // Fuenf Byte mit Fortsetzungsbit: das ist kein gueltiges Ende,
        // und der Leser sagt es, statt endlos weiterzulesen.
        //
        // GEMESSENE KORREKTUR AN DER ZAHL: der Test sagte „acht Byte",
        // der Leser braucht fuenf (7 * 5 = 35 Bit, und 32 sind erlaubt).
        // Mit acht Byte wuerde der Leser beim fuenften aufgeben und nie
        // an die acht kommen — der Test haette also eine Grenze
        // geprueft, die es nicht gibt.
        let daten = vec![0xffu8; 16];
        let mut l = BitLeser::neu(&daten);
        assert!(matches!(
            l.variabel(),
            Err(BitFehler::VorlaeufigZuEnde { .. })
        ));
    }

    #[test]
    fn eine_zahl_ueber_32_bit_ist_ein_fehler_kein_panic() {
        // Der Fehler, der zuerst als PANIC aufgetreten ist
        // („attempt to shift left with overflow"): fuenf Byte, die zusammen
        // mehr als 32 Bit ergeben.
        let mut l = BitLeser::neu(&[0xff, 0xff, 0xff, 0xff, 0x7f]);
        match l.variabel() {
            Err(BitFehler::ZuGross { wert, .. }) => {
                assert!(
                    wert > u64::from(u32::MAX),
                    "{wert} passt noch in 32 Bit — der Test prueft die falsche Grenze"
                );
            }
            andere => panic!("erwartet ZuGross, kam {andere:?}"),
        }
    }

    #[test]
    fn die_vorzeichen_zahl_ist_sign_betrag_und_kein_zwei_komplement() {
        // RFC 5215 Abschnitt 1.3.1: `n` Bit lesen, das OBERSTE gelesene
        // Bit ist das Vorzeichen (1 = positiv), der Rest ist der Betrag.
        //
        // Byte 0x00 mit 8 Bit: alle Bit null -> Vorzeichen 0, Betrag 0
        //   -> -0, was 0 ist.
        // Byte 0x80 mit 8 Bit: Bit 7 gesetzt -> positiv, Betrag 0 -> +0.
        // Byte 0x40 mit 8 Bit: Bit 6 gesetzt -> negativ, Betrag 64 -> -64.
        let mut l = BitLeser::neu(&[0x00]);
        assert_eq!(l.signiert(8).expect("8 bit"), 0);
        let mut l = BitLeser::neu(&[0x80]);
        assert_eq!(l.signiert(8).expect("8 bit"), 0);
        let mut l = BitLeser::neu(&[0x40]);
        assert_eq!(l.signiert(8).expect("8 bit"), -64);
        let mut l = BitLeser::neu(&[0x20]);
        // GEMESSENE KORREKTUR AN DER ERWARTUNG: hier stand 32. 0x20
        // ist 0010_0000, das oberste gelesene Bit (Bit 7) ist NULL, also
        // ist die Zahl NEGATIV und der Betrag ist 32 -> -32. Der Leser
        // hatte recht.
        assert_eq!(l.signiert(8).expect("8 bit"), -32);
        // Und die Abgrenzung zum Zwei-Komplement, denn genau dort
        // verwechseln sich die beiden Kodierungen: bei 0x7F (0111_1111)
        // ist Zwei-Komplement +127, Sign-Betrag aber -127.
        let mut l = BitLeser::neu(&[0x7f]);
        assert_eq!(
            l.signiert(8).expect("8 bit"),
            -127,
            "Sign-Betrag, nicht Zwei-Komplement"
        );
        // Ein Bit: nur das Vorzeichen.
        let mut l = BitLeser::neu(&[0b01]);
        assert_eq!(l.signiert(1).expect("1 bit"), 1);
        let mut l = BitLeser::neu(&[0b00]);
        assert_eq!(l.signiert(1).expect("1 bit"), -1);
    }

    #[test]
    fn signiert_verlangt_eine_sinnvolle_breite() {
        // M4: `n = 0` waere eine Schiebe um -1, `n > 32` ein Ueberlauf.
        let daten = vec![0u8; 16];
        let mut l = BitLeser::neu(&daten);
        assert!(l.signiert(0).is_err());
        assert!(l.signiert(33).is_err());
        assert!(l.signiert(1).is_ok());
        assert!(l.signiert(32).is_ok());
    }

    #[test]
    fn die_64_bit_zahl_ist_zwei_woerter() {
        // GEMESSEN, warum das ein eigener Test ist: die Zeitbereichs-
        // werte des Vorbis-Setup-Kopfes sind die einzigen 64-Bit-Zahlen
        // in der Datei, und `signiert(64)` lehnt sie ab. Wer sie mit
        // `signiert` liest, bekommt einen benannten Fehler und weiss
        // nicht warum — es sieht aus wie ein abgeschnittener Strom.
        //
        // low = 0x0000_0002, high = 0x0000_0000 -> 2
        let mut l = BitLeser::neu(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        assert_eq!(l.signiert64().expect("64 bit"), 2);
        // high = 0x8000_0000 -> negativ: -2147483648 (0x8000_0000_0000_0000)
        let mut l = BitLeser::neu(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]);
        assert_eq!(l.signiert64().expect("64 bit"), i64::MIN);
        // -1: alle Bit gesetzt
        let mut l = BitLeser::neu(&[0xff; 8]);
        assert_eq!(l.signiert64().expect("64 bit"), -1);
    }

    #[test]
    fn ein_zu_kurzer_strom_bei_64_bit_ist_ein_fehler() {
        // M3: die Funktion liest 64 Bit, also braucht sie 8 Byte.
        let mut l = BitLeser::neu(&[0u8; 4]);
        assert!(matches!(l.signiert64(), Err(BitFehler::ZuEnde { .. })));
    }

    #[test]
    fn die_bytegrenze_ist_ein_schritt_und_kein_sprung() {
        // Nach drei Bit stehen wir mitten im Byte. `bis_bytegrenze`
        // raeumt die fuenf uebrigen Bit weg — der Sprung ist auf die
        // naechste Byte-Grenze, nicht auf die naechste fuenf Bit.
        let mut l = BitLeser::neu(&[0xff, 0xaa]);
        l.bits(3).expect("3 bit");
        assert_eq!(l.byte_position(), 0);
        l.bis_bytegrenze();
        assert_eq!(l.byte_position(), 1);
        assert_eq!(l.position(), 8);
        // Und `auf_bytegrenze` geht nie zurueck.
        l.bits(3).expect("3 bit");
        l.auf_bytegrenze();
        assert_eq!(l.position(), 8);
    }

    #[test]
    fn ab_einer_byteposition_lesen() {
        let mut l = BitLeser::ab(&[0x00, 0xff], 1);
        assert_eq!(l.bits(8).expect("8 bit"), 0xff);
    }

    #[test]
    fn die_rest_bits_zaehlen_richtig() {
        let mut l = BitLeser::neu(&[0x01, 0x02]);
        assert_eq!(l.rest_bits(), 16);
        l.bits(3).expect("3");
        assert_eq!(l.rest_bits(), 13);
    }
}

/// # DIE ZWEI LESER, und wozu es den zweiten gibt
///
/// `bit_bis` sagt, **ob** ein Bit da ist. `bit_wert_bis` sagt,
/// **welches**. Das sind verschiedene Fragen, und sie zu verwechseln
/// ist der Fehler, der den MP3-Dekoder vier Runden lang bei -45 dB
/// hielt: `mp3::granel` negierte damit **jeden** von null
/// verschiedenen Wert.
///
/// Der Test liest dieselben Bit zweimal — einmal mit dem einen, einmal
/// mit dem anderen Leser — und verlangt, dass die **Werte** das
/// Komplement der **Verfuegbarkeit** sind. Ohne diesen Test faellt
/// der Unterschied nicht auf, denn beide liefern `bool`.
#[test]
fn die_zwei_bis_leser_antworten_verschiedene_fragen() {
    for daten in [
        [0b1010_0110u8, 0b1100_0011, 0b0000_0001],
        [0b1000_0000u8, 0b0111_1111, 0b1000_0001],
        [0b0000_0001u8, 0b0000_0001, 0b0000_0000],
        [0b1111_1111u8, 0b1111_1111, 0b1111_1111],
    ] {
        // (1) `bit_wert_bis` muss genau das liefern, was `bit()`
        //     liefert — nur mit `grenze`.
        for grenze in 1..=20usize {
            let mut a = BitLeserVorn::neu(&daten);
            let mut b = BitLeserVorn::neu(&daten);
            for p in 0..grenze.min(24) {
                let mit = a.bit_wert_bis(grenze).expect("bis zur Grenze");
                let ohne = b.bit().expect("ohne Grenze") != 0;
                assert_eq!(mit, ohne, "Daten {daten:?} Position {p} Grenze {grenze}");
            }
        }
        // (2) DER UNTERSCHIED, den der Test festnagelt: sobald das
        //     erste Bit **null** ist, sagt `bit_bis` `true` (es
        //     fragt nach der Verfuegbarkeit) und `bit_wert_bis`
        //     `false` (es fragt nach dem Wert).
        if daten[0] & 0x80 == 0 {
            let mut v = BitLeserVorn::neu(&daten);
            assert!(v.bit_bis(16).expect("bis 16"), "verfuegbar");
            let mut w = BitLeserVorn::neu(&daten);
            assert!(!w.bit_wert_bis(16).expect("bis 16"), "Wert ist null");
        }
    }
}
