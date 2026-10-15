//! **Opus** in Ogg — der Kopf und die Dauer. **Kein Decoder.**
//!
//! # WAS HIER GEBAUT WIRD, UND WAS NICHT
//!
//! GEMESSEN, dass Opus der **letzte offene Punkt** dieser Crate ist — und
//! dass der **Dekoder** nicht gebaut werden kann: Opus ist ein
//! **verlustbehalteter** Codec mit **SILK** (Sprache) und **CELT** (Musik),
//! und beide sind **keine** Tabellenarbeit wie Vorbis.
//!
//! Also baut dieses Modul **genau den Teil, der ohne Decoder geht**:
//!
//! * [`ist_opus`] — die **Erkennung**
//! * [`Kopf`] und [`lies_kopf`] — der **`OpusHead`**
//! * [`dauer_s`] — die **Dauer** aus dem **letzten `granulepos`**
//!
//! Und es liefert **benannte Fehler** statt zu raten: [`Format::Opus`]
//! existiert, damit eine Opus-Datei **nicht** als Vorbis durchgeht.
//!
//! # DIE DAUER, UND SIE IST GEMESSEN — nicht aus der Norm gelesen
//!
//! GEMESSEN an vier echten `ffmpeg`-Dateien (`libopus`, vier verschiedene
//! Kanäle und Laufzeiten):
//!
//! ```text
//!   g9o_mono_1s.opus     Rate=48000 granule= 48312 -> 1.000000 s
//!   g9o_mono_8k_5s.opus  Rate= 8000 granule=240312 -> 5.000000 s
//!   g9o_stereo_3s.opus   Rate=48000 granule=144312 -> 3.000000 s
//!   g9o_ton_48k.opus     Rate=48000 granule=144312 -> 3.000000 s
//! ```
//!
//! Die Formel ist also
//!
//! ```text
//!   dauer = (granulepos der LETZTEN Seite - Vorrat) / 48000
//! ```
//!
//! # DIE FALLE, UND SIE IST DER GANZE PUNKT DIESER DATEI
//!
//! **GEMESSEN:** `g9o_mono_8k_5s.opus` trägt im `OpusHead` die
//! **Ausgabemate 8000**. Wer die benutzt, rechnet `240312 / 8000` = **`30,039
//! s`** — und die Datei ist **5,000000 s** lang. Das ist **Faktor 6**
//! falsch.
//!
//! **GEMESSENE DEUTUNG:** der `granulepos` zählt in **48-kHz-Einheiten**,
//! **unabhängig** von der Ausgabemate. Das ist keine Besonderheit von Opus,
//! sondern die Bedeutung von „Granule" in Ogg: eine Position in der
//! **Codec-internen** Rate.
//!
//! **REGEL: eine Zahl aus einem Kopf ist nicht die Zahl, die man braucht.
//! Man muss messen, welche das ist.** Ich hatte im Status notiert, die Rate
//! stehe im TOC — sie steht **nirgends** für diese Rechnung, und sie wird
//! **gar nicht gebraucht**. **GEMESSENER Beweis:** der TOC
//! (`OpusTags`) ist reiner **Herstellertext** (`Lavf61.7.103`,
//! `encoder=...`) und enthält **keine** Zahl zur Dauer.
//!
//! # DER VORRAT, UND WARUM ER ABGEZOGEN WIRD
//!
//! GEMESSEN: `OpusHead` Byte 18..20 ist der **Vorrat** (`pre-skip`), bei
//! allen vier `ffmpeg`-Dateien **`312`**. GEMESSEN ist auch, dass
//! `granulepos − Vorrat` durch **48000** teilbar ist und **ohne** den
//! Abzug **nicht**:
//!
//! ```text
//!   g9o_ton_48k.opus: granule = 144312
//!     granule / 48000              = 3.006500 s   (Soll 3,0 — FALSCH)
//!     (granule - 312) / 48000      = 3.000000 s   (Soll 3,0 — RICHTIG)
//! ```
//!
//! **GEMESSENE DEUTUNG:** der Vorrat sind die Samples, die der Encoder am
//! Anfang **weglässt** (das Vorrausrollen des Filters), und der
//! `granulepos` **zählt** sie mit. Ohne Abzug ist die Anzeige um
//! `312 / 48000` = **`6,5 ms`** zu lang — und das ist **genau** die
//! Differenz `3,006500 − 3,000000`.

use crate::fehler::{Art, Ergebnis, Fehler};

// ############################################################################
// DER DEKODER — CELT, nach RFC 6716
//
// Die Quelle jeder Zeile unten ist die Referenz der **Norm selbst**, nicht
// eine Erinnerung: RFC 6716, Anhang A.3 (base64-kodiertes Quellpaket),
// ausgepackt nach `/tmp/opencode/opus-rfc6716/`.
//
// **WARUM das hier ausdruecklich steht:** die Doku dieser Datei (Zeile 6)
// sagt, der Opus-Dekoder koenne **nicht** gebaut werden. Die Leitung hat
// das fuer CELT ueberstimmt. Und **GEMESSEN** ist die Groesse der Aufgabe:
// die Kerndateien des CELT-Dekoders der Norm sind
//
// ```text
//    254  entdec.c        (Bereichsdecoder)
//    652  cwrs.c          (PVQ)
//    646  rate.c          (Bitverteilung)
//   1304  bands.c         (Baender, Faltung, Stereo)
//    340  mdct.c          (Transformation)
//    727  kiss_fft.c      (FFT)
//   2855  celt.c          (Rahmen, Filter, Deemphasis)
//   ----
//   6875  Zeilen C fuer den Kern — ohne die Modustabellen
// ```
//
// **Also** ist das eine Arbeit ueber mehrere Schichten, und jede Schicht
// wird **gemessen**, bevor die naechste beginnt.
// ############################################################################

/// Die Bitbreite eines Symbols im Strom (mfrngcod.h: `EC_SYM_BITS`).
const SYM_BITS: u32 = 8;
/// Die Bitbreite des Codes (mfrngcod.h: `EC_CODE_BITS`).
const CODE_BITS: u32 = 32;
/// Das groesste Symbol: `255` (mfrngcod.h: `EC_SYM_MAX`).
const SYM_MAX: u32 = (1 << SYM_BITS) - 1;
/// Die obere Grenze: `1 << 31` (mfrngcod.h: `EC_CODE_TOP`).
const CODE_TOP: u32 = 1 << (CODE_BITS - 1);
/// Die untere Grenze, bis zu der normalisiert wird (`EC_CODE_BOT`).
const CODE_BOT: u32 = CODE_TOP >> SYM_BITS;
/// Die **zusatzlichen** Bits am Anfang (mfrngcod.h: `EC_CODE_EXTRA`).
///
/// **GEMESSEN:** `(32 - 2) % 8 + 1` = `30 % 8 + 1` = **`7`**.
const CODE_EXTRA: u32 = (CODE_BITS - 2) % SYM_BITS + 1;
/// Die Bitbreite eines `ec_uint`-Schritts (entcode.h: `EC_UINT_BITS`).
const UINT_BITS: u32 = 8;

/// Die Zahl der **gesetzten** Bits — `EC_ILOG`.
///
/// **GEMESSENE ENTSCHEIDUNG, warum das **nicht** `leading_zeros` ist:**
/// `EC_ILOG(x)` ist die **Bitposition des hoechsten** Bits plus **1**, und
/// fuer `x = 0` ist das Ergebnis in der Referenz **undefiniert**
/// (Kommentar in `ec_dec_uint`: „it is undefined for the value 0"). Hier
/// ist es gemessen **0** statt geraten — und `ec_ilog(0)` wird von keinem
/// Weg benutzt, weil `_ft > 1` geprueft wird, bevor sie gerufen wird.
#[must_use]
fn ilog(x: u32) -> u32 {
    if x == 0 {
        0
    } else {
        u32::BITS - x.leading_zeros()
    }
}

/// Der Bereichsdecoder (`entdec.c`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Felder `pub(crate)` sind und der
/// Typ nicht exportiert wird:** `opus.rs` ist die einzige Stelle, die ihn
/// braucht. Wer ihn von aussen benutzt, benutzt `dekodiere`.
#[derive(Debug, Clone)]
pub struct Bereich {
    /// Der Strom.
    buf: Vec<u8>,
    /// Wie viele Byte von **vorn** gelesen sind.
    offs: usize,
    /// Wie viele Byte von **hinten** gelesen sind (fuer `ec_dec_bits`).
    end_offs: usize,
    /// Der Bereich.
    rng: u32,
    /// Der Wert.
    val: u32,
    /// Das letzte Symbol, aus dem noch Bits uebrig sind.
    rem: u32,
    /// Das Fenster fuer `ec_dec_bits`.
    end_window: u32,
    /// Wie viele Bits im Fenster liegen.
    nend_bits: u32,
    /// Die Zahl der gelesenen Bits (fuer `ec_tell`).
    nbits_total: u32,
    /// Die Zwischensumme von `ec_decode`.
    ext: u32,
    /// Gesetzt, wenn `ec_dec_uint` einen Wert ausserhalb fand.
    error: bool,
    /// **Nur zur Messung** (04.10.2026): `true`, wenn `ext`
    /// vom **unmittelbar vorhergehenden** `ec_decode`/
    /// `ec_decode_bin` gesetzt wurde. `entcode.h` nennt `ext`
    /// „*the saved normalization factor from `ec_decode()`*",
    /// und `ec_dec_update` rechnet `IMUL32(ext, ft-fh)` — **ohne
    /// den passenden `ec_decode` ist es also nicht definiert.**
    ext_gesetzt: bool,
}

impl Bereich {
    /// Startet den Decoder auf einem Paket.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warus `buf` **kopiert** und nicht
    /// geborgt wird:** `ec_read_byte` liefert **0**, wenn der Zeiger
    /// ueber das Ende geht — das ist der eingebaute Weg fuer „Strom zu
    /// Ende", und es braucht **keine** Fehlerbehandlung im Rueckgabewert.
    /// Wer stattdessen einen Fehler liefern wuerde, muesste ihn an
    /// **jedem** Lesevorgang pruefen; die Referenz tut das ausdruecklich
    /// nicht, und ein Decoder, der sich an denselben Bitstrom haelt,
    /// **muss** sich auch gleich verhalten.
    #[must_use]
    pub fn neu(paket: &[u8]) -> Self {
        let mut b = Bereich {
            buf: paket.to_vec(),
            offs: 0,
            end_offs: 0,
            end_window: 0,
            nend_bits: 0,
            // entdec.c: `EC_CODE_BITS+1-((EC_CODE_BITS-EC_CODE_EXTRA)/
            // EC_SYM_BITS)*EC_SYM_BITS` = 32+1-((32-7)/8)*8 = 33-24 = **9**
            nbits_total: CODE_BITS + 1 - ((CODE_BITS - CODE_EXTRA) / SYM_BITS) * SYM_BITS,
            rng: 1 << CODE_EXTRA,
            val: 0,
            rem: 0,
            ext: 0,
            error: false,
            ext_gesetzt: false,
        };
        b.rem = b.lese_byte();
        // GEMESSEN: `val = rng-1-(rem>>(8-7))` — der **Vorrat** eines
        // halben Symbols geht von der ersten Byte-Grenze ab.
        b.val = b.rng - 1 - (b.rem >> (SYM_BITS - CODE_EXTRA));
        b.normalisiere();
        b
    }

    /// Ein Byte von **vorn**, oder `0` am Ende (`ec_read_byte`).
    fn lese_byte(&mut self) -> u32 {
        if self.offs < self.buf.len() {
            let b = u32::from(self.buf[self.offs]);
            self.offs += 1;
            b
        } else {
            0
        }
    }

    /// Ein Byte von **hinten**, oder `0` am Ende (`ec_read_byte_from_end`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum von **hinten**:** `ec_dec_bits` liest
    /// die naechsten Bits fuer die **letzten** Symbole, und die liegen am
    /// **Ende** des Pakets. Von vorn gelesen kaeme das **falsche** Byte.
    fn lese_byte_von_hinten(&mut self) -> u32 {
        if self.end_offs < self.buf.len() {
            self.end_offs += 1;
            u32::from(self.buf[self.buf.len() - self.end_offs])
        } else {
            0
        }
    }

    /// Normalisiert, bis der Bereich ganz im oberen Symbol liegt
    /// (`ec_dec_normalize`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das `wrapping_shl` und nicht `<<`
    /// ist:** in C laeuft `val<<8` auf einem 32-Bit-Wort **ueber** und
    /// **nimmt** die oberen Bits mit. In Rust **paniked** `<<` im
    /// `debug` bei Werten ab `2^24` — und `val` ist gerade genau so gross.
    /// Das ist **kein** Detail: ohne dieses `wrapping` waere der Decoder im
    /// Testbau **rot** und im Auslieferungsbau **anders** als die Norm.
    fn normalisiere(&mut self) {
        while self.rng <= CODE_BOT {
            self.nbits_total += SYM_BITS;
            self.rng <<= SYM_BITS;
            let sym = self.rem;
            self.rem = self.lese_byte();
            // Die restlichen Bits aus dem neuen Symbol holen.
            let s = ((sym << SYM_BITS) | self.rem) >> (SYM_BITS - CODE_EXTRA);
            self.val = self.val.wrapping_shl(SYM_BITS).wrapping_add(!s & SYM_MAX) & (CODE_TOP - 1);
        }
        // **GEMESSENE ZUSAGE (04.10.2026), die den Fehler
        // **lokalisieren** soll statt ihn zu beschreiben.**
        // `val` ist in der Referenz **immer** `< CODE_TOP`: nur
        // die Schleife maskiert, und sie laeuft nur, wenn
        // `rng <= CODE_BOT`. Bei `rng > CODE_BOT` wird also
        // **nicht** maskiert — dort traegt allein die Zusage,
        // dass `val` schon im Fenster liegt, und die entsteht
        // nur aus `val -= ext*(ft-fh)` ohne Unterlauf.
        //
        // **GEMESSEN:** genau diese Zusage ist verletzt. Im
        // SNR-Lauf stand `val = 4156245400` bei `rng = 186641049`
        // — also **doppelt so gross wie das Fenster** und
        // groesser als `CODE_TOP - 1 = 2147483647`. Das ist der
        // erste **messbare** Beweis, dass der Decoder den
        // Bitstrom verlassen hat: nicht die Symbole sind falsch,
        // sondern `val` selbst liegt draussen.
        // **Die Bedingung lautet `< CODE_TOP`, nicht
        // `<= CODE_TOP-1`.** Fuer Ganzzahlen ist das
        // **identisch**, und clippy (`int_plus_one`) weist die
        // andere Form zurueck — ein `#[allow]` auf einem
        // `debug_assert!` wird **igniert** (GEMESSEN). **Die
        // Referenz-Schreibweise `val <= CODE_TOP-1` steht in der
        // Meldung**, damit die Zuordnung beim Lesen erhalten
        // bleibt.
        debug_assert!(
            self.val < CODE_TOP,
            "Bereichsdecoder: val = {} ist groesser als CODE_TOP-1 = {} (rng = {}) — \
             `val` ist aus der Maske gefallen, der Decoder war schon vorher ausserhalb",
            self.val,
            CODE_TOP - 1,
            self.rng
        );
    }

    /// **GEMESSENE ZUSAGE (04.10.2026): `val < rng`.**
    ///
    /// Das ist die **eigene** Zusage des Referenzdecoders: `val`
    /// ist „*the difference between the top of the current range
    /// and the input value, minus one*" (entcode.h), also immer
    /// **kleiner als das Fenster**.
    ///
    /// **Warum `val <= CODE_TOP-1` nicht reichte:** das stimmt,
    /// solange die Maske in `ec_dec_normalize` gelaufen ist — sie
    /// laeuft aber **nur**, solange `rng <= EC_CODE_BOT`. Bei
    /// `rng > CODE_BOT` wird nicht maskiert, und ein `val` aus dem
    /// Unterlauf bleibt sichtbar stehen. **Deshalb wird hier die
    /// Zusage geprueft, die der Decoder wirklich hat: `val < rng`
    /// am Anfang jedes Symbolsitzes.** Sie faellt fuer einen
    /// korrekten Decoder nie — im `ec_dec_update`-Pfad folgt sie
    /// aus `fm in [fl, fh)`, im `bit_logp`-Pfad aus `d < r`.
    fn zusage_fenster(&self, was: &str) {
        debug_assert!(
            self.val < self.rng,
            "Bereichsdecoder: val = {} ist nicht kleiner als rng = {} — vor `{was}`. \
             Das ist die Zusage, die der Referenzdecoder selbst haelt; ihr Bruch ist die \
             Stelle, an der der Decoder den Bitstrom verlassen hat.",
            self.val,
            self.rng
        );
    }

    /// Die kumulative Frequenz des naechsten Symbols (`ec_decode`).
    fn dekodiere(&mut self, ft: u32) -> u32 {
        self.zusage_fenster("dekodiere");
        self.ext_gesetzt = true;
        self.dekodiere_intern(ft)
    }

    /// Der gemeinsame Kern von [`Bereich::dekodiere`] und
    /// [`Bereich::dekodiere_roh`] (`ec_decode`): setzt `ext` und
    /// liefert `ft - min(s+1, ft)`. **Verengt den Bereich nicht.**
    fn dekodiere_intern(&mut self, ft: u32) -> u32 {
        self.ext = self.rng / ft;
        let s = self.val / self.ext;
        // **GEMESSENE ZUSAGE (04.10.2026), die als einzige
        // wirklich etwas ueber den Zustand sagt:**
        // `rng = ext*ft + rem` mit `rem < ft`, also reicht
        // `val/ext` bis `ft + rem/ext`. **Faellt `val` auf
        // `ext*ft` oder darueber, ist `s >= ft` — und dann wird
        // der zurueckgegebene Wert per `.min()` auf `0`
        // **maskiert statt gemeldet**. Genau dort laeuft ein
        // Decoder still falsch: er liest weiter, verbraucht
        // Bits und liefert plausible, aber falsche Symbole.
        //
        // **Warum die vier Laplace-Behauptungen nicht reichen:**
        // `fl <= fm < fl+fs` gilt fuer **jedes** `fm`, weil die
        // `while`-Schleife vorher schon auf `[fl, fl+2*fs)`
        // eingeklemmt hat. Sie sind **leer** (Beweis dort).
        // Dasselbe gilt fuer `val < rng`: es ist auf dem
        // Laplace-Pfad aequivalent zu `fm in [fl, fh)`.
        // **Diese eine hier nicht.**
        debug_assert!(
            s < ft,
            "Bereichsdecoder: Symbolindex {s} ist nicht kleiner als ft = {ft} \
             (rng = {}, ext = {}, val = {}) — der Decoder laeuft ausserhalb seines Fensters",
            self.rng,
            self.ext,
            self.val
        );
        ft - (s + 1).min(ft)
    }

    /// Ein Symbol aus `ft` gleichwahrscheinlichen (`ec_decode_bin`).
    #[must_use]
    pub fn bit(&mut self, bits: u32) -> u32 {
        self.zusage_fenster("bit");
        self.ext_gesetzt = true;
        self.ext = self.rng >> bits;
        let s = self.val / self.ext;
        // **Dieselbe Zusage wie in `dekodiere`, siehe dort.**
        // `ec_laplace_decode` liest hier **15** Bit — das ist
        // genau die Stelle, an der ein aus dem Ruder
        // gelaufener Decoder zuerst **still** falsch wird.
        debug_assert!(
            s < (1 << bits),
            "Bereichsdecoder: Symbolindex {s} ist nicht kleiner als {} \
             (rng = {}, ext = {}, val = {}) — der Decoder laeuft ausserhalb seines Fensters",
            1u32 << bits,
            self.rng,
            self.ext,
            self.val
        );
        (1 << bits) - (s + 1).min(1 << bits)
    }

    /// Wie [`Bereich::aktualisiere`], aber von aussen
    /// erreichbar (`ec_dec_update` wird in `laplace.c`
    /// gebraucht, das nicht Teil von `entdec.c` ist).
    pub(crate) fn aktualisiere_priv(&mut self, fl: u32, fh: u32, ft: u32) {
        self.aktualisiere(fl, fh, ft);
    }

    /// Setzt den Bereich auf `[fl, fh)` (`ec_dec_update`).
    pub fn aktualisiere(&mut self, fl: u32, fh: u32, ft: u32) {
        let s = self.ext.wrapping_mul(ft - fh);
        // **GEMESSENE ZUSAGE (04.10.2026): `ext` MUSS vom
        // unmittelbar vorhergehenden `ec_decode`/`ec_decode_bin`
        // stammen.** `entcode.h`: „*the saved normalization
        // factor from `ec_decode()`*". `ec_dec_update` rechnet
        // `IMUL32(ext, ft-fh)` und `IMUL32(ext, fh-fl)` — **ohne
        // den passenden `ec_decode` sind beide Zahlen
        // bedeutungslos**, und genau daraus kann ein `s` kommen,
        // das groesser als `val` ist.
        //
        // **Warum das nicht von selbst herausfaellt:** `ec_dec_icdf`
        // und `ec_dec_bits` setzen `val`/`rng` **direkt**, ohne
        // `ec_decode` — das ist in der Norm so ("*No call to
        // `ec_dec_update()` is necessary*"). Wer danach doch
        // `aktualisiere` aufruft, benutzt ein **fremdes** `ext`.
        debug_assert!(
            self.ext_gesetzt,
            "Bereichsdecoder: `aktualisiere(fl={fl}, fh={fh}, ft={ft})` ohne vorheriges \
             `dekodiere`/`bit` — `ext` = {} stammt aus einem anderen Fenster (rng = {})",
            self.ext, self.rng
        );
        self.ext_gesetzt = false;
        // **GEMESSENE ZUSAGE (04.10.2026) — die Stelle, an der
        // der Decoder den Bitstrom verlässt.** `val -= s` darf
        // **nicht** unterlaufen: `ec_dec_update` setzt voraus,
        // dass `val` im Intervall `[ext*(ft-fh), ext*ft)` liegt,
        // und genau daraus folgt `s <= val`.
        //
        // **Warum der Unterlauf so lange unbemerkt blieb:** er
        // dreht `val` per `wrapping_sub` auf rund `4*10^9`, und
        // `ec_dec_normalize` **maskt `val` nur innerhalb seiner
        // Schleife**. Die Schleife laeuft aber nur, solange
        // `rng <= EC_CODE_BOT`. Ist `rng` gross genug, laeuft sie
        // **nicht**, `val` bleibt unmaskiert — und der Decoder
        // liest mit einem Wert weiter, der **doppelt so gross
        // wie sein Fenster** ist. Genau das ist gemessen worden
        // (`val = 4156245400` bei `rng = 186641049`), und
        // deshalb ist es **still** falsch geblieben: die
        // Laplace-Behauptungen sind leer (siehe dort), und ein
        // kaputter `val` faellt durch die Maske hindurch.
        debug_assert!(
            s <= self.val,
            "Bereichsdecoder: ec_dec_update unterlaeuft — s = ext*({ft}-{fh}) = {} > val = {} \
             (rng = {}, ext = {}). `val` wird auf {} gedreht und, weil rng > CODE_BOT, nicht \
             mehr maskiert. Das ist die Stelle, an der der Decoder den Strom verlassen hat.",
            s,
            self.val,
            self.rng,
            self.ext,
            self.val.wrapping_sub(s)
        );
        // **GEMESSENE Korrektur:** `dec->val -= s` ist in C
        // **vorzeichenlos** und laeuft bewusst ueber. Ohne
        // `wrapping_` panicte der erste SNR-Lauf, weil `s`
        // fast so gross ist wie `val`.
        self.val = self.val.wrapping_sub(s);
        self.rng = if fl > 0 {
            self.ext.wrapping_mul(fh.wrapping_sub(fl))
        } else {
            self.rng.wrapping_sub(s)
        };
        self.normalisiere();
    }

    /// Das naechste Symbol aus `ft` gleichwahrscheinlichen (`ec_decode`).
    ///
    /// Dies ist der **Weg**, den CELT fuer die Bandgroessen und die
    /// PVQ-Groessen nimmt: erst die kumulative Frequenz, dann
    /// [`Bereich::aktualisiere`].
    pub fn symbol(&mut self, ft: u32) -> u32 {
        let s = self.dekodiere(ft);
        self.aktualisiere(s, s + 1, ft);
        s
    }

    /// **NUR** `ec_decode`: setzt `ext` und gibt die kumulative
    /// Frequenz zurueck — **ohne** den Bereich zu verengen.
    ///
    /// **GEMESSENER FEHLER, gefunden am 04.10.2026, und er war
    /// der stille Grund fuer alles.** Ich hatte an zwei Stellen
    /// `symbol(ft)` benutzt und danach noch einmal
    /// `aktualisiere(fl, fl+fs, ft)` mit den **nachgerechneten**
    /// Grenzen aufgerufen. `symbol` verengt den Bereich aber
    /// **schon**. **Der Bereich wurde damit zweimal verengt**, das
    /// zweite Mal mit einem **veralteten** `ext` — und genau
    /// daran lief `val` unter und kam aus der Maske von
    /// `ec_dec_normalize` heraus.
    ///
    /// **Die Referenz (`bands.c` Zeilen 789..794 und 819..835)
    /// ist unmissverstaendlich:**
    /// ```c
    /// fs=ec_decode(ec,ft);                    /* setzt nur ext */
    /// if (fs<(x0+1)*p0) x=fs/p0; else ...;
    /// ec_dec_update(ec, x<=x0?p0*x:..., ..., ft);   /* EIN Update */
    /// ```
    /// **`ec_decode` verengt nicht.** `ec_decode` + `ec_dec_update`
    /// ist das Paar; wer `ec_decode` durch etwas ersetzt, das
    /// schon verengt, verengt **zweimal**.
    ///
    /// **Merksatz: `ec_decode` und `ec_dec_update` sind zwei
    /// Schritte, nicht einer — und wer sie zu einem
    /// bequemen `symbol()` zusammenfasst, muss an jeder Stelle
    /// nachzaehlen, ob der Aufrufer danach noch einmal
    /// verengt.**
    #[must_use]
    pub fn dekodiere_roh(&mut self, ft: u32) -> u32 {
        self.zusage_fenster("dekodiere_roh");
        self.ext_gesetzt = true;
        self.dekodiere_intern(ft)
    }

    /// Ein Bit mit Wahrscheinlichkeit `1/(1<<logp)` (`ec_dec_bit_logp`).
    #[must_use]
    pub fn bit_logp(&mut self, logp: u32) -> bool {
        self.ext_gesetzt = false;
        self.zusage_fenster("bit_logp");
        let r = self.rng;
        let d = self.val;
        let s = r >> logp;
        let ret = d < s;
        if !ret {
            self.val = d - s;
        }
        self.rng = if ret { s } else { r - s };
        self.normalisiere();
        ret
    }

    /// `bits` Bit **am Ende** des Stroms (`ec_dec_bits`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das Fenster `u32` ist:** in der
    /// Referenz ist `ec_window` ein `opus_uint32`, also
    /// `EC_WINDOW_SIZE = 32` (entcode.h: `sizeof(ec_window)*CHAR_BIT`).
    #[must_use]
    pub fn bits(&mut self, bits: u32) -> u32 {
        self.ext_gesetzt = false;
        let mut window = self.end_window;
        let mut available = self.nend_bits;
        while available < bits {
            window |= self.lese_byte_von_hinten() << available;
            available += SYM_BITS;
        }
        let ret = window & ((1u32 << bits) - 1);
        window >>= bits;
        available -= bits;
        self.end_window = window;
        self.nend_bits = available;
        self.nbits_total += bits;
        ret
    }

    /// Eine Gleichverteilung von `ft` Werten (`ec_dec_uint`).
    #[must_use]
    pub fn uint(&mut self, ft: u32) -> u32 {
        self.ext_gesetzt = false;
        debug_assert!(ft > 1, "ec_dec_uint verlangt ft > 1");
        let ft = ft - 1;
        let ftb = ilog(ft);
        if ftb > UINT_BITS {
            let ftb = ftb - UINT_BITS;
            let n = (ft >> ftb) + 1;
            let s = self.dekodiere(n);
            self.aktualisiere(s, s + 1, n);
            let t = (s << ftb) | self.bits(ftb);
            if t <= ft {
                t
            } else {
                self.error = true;
                ft
            }
        } else {
            let n = ft + 1;
            let s = self.dekodiere(n);
            self.aktualisiere(s, s + 1, n);
            s
        }
    }

    /// Das naechste Symbol aus einer **kumulativen** Tabelle
    /// (`ec_dec_icdf`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `icdf` hier `&[u8]` und kein
    /// Slice mit Endmarke ist:** in der Referenz ist `_icdf` ein
    /// `const unsigned char *`, und die Schleife laeuft, **bis** `d >= s`.
    /// Das letzte Element muss **nicht** 0 sein — es wird nie gelesen, weil
    /// die Schleife vorher endet. **Also** ist die Abbruchbedingung das
    /// **Ende** des Slices, und ein fehlendes Element ergaebe in C einen
    /// Lesefehler; hier endet die Schleife einfach.
    ///
    /// **GEMESSEN, wo CELT das benutzt:** `unquant_energy` liest die
    /// Energie mit `ec_dec_icdf` aus den drei Skalen
    /// (`minihdr`, `midi`, `max`), und jedes Band laeuft ueber
    /// `ec_dec_icdf(logp, ftb)` fuer seine Verteilung.
    #[must_use]
    pub fn icdf(&mut self, icdf: &[u8], ftb: u32) -> usize {
        self.ext_gesetzt = false;
        let r = self.rng >> ftb;
        let d = self.val;
        let mut s = self.rng;
        let mut ret = 0usize;
        // **GEMESSENER UEBERSETZUNGSFEHLER, und Rust hat ihn gemeldet:**
        // in C steht `t` im `do{...}while`, wird aber **nachher**
        // gelesen — in Rust war es im `let` **innerhalb** der Schleife
        // gebunden: `error[E0425]: cannot find value 't' in this
        // scope`.
        //
        // **GEMESSENE ZWEITE UEBERSETZUNG, und sie ist besser als
        // die erste:** ein `let mut t = s` mit `t = s` in der
        // Schleife bringt zwei Fehler — `error[E0384]` (doppelt
        // zugewiesen) und `warning: value assigned to 't' is never
        // read` (der Anfangswert ist tot, weil die Referenz als
        // `do{...}while` laeuft). **Also** wird `t` **nur** an der
        // **einen** Abbruchstelle gesetzt: der Bereich **vor** dem
        // letzten Schritt. Das ist **dieselbe** Zahl wie in C, ohne
        // totem Speicher und ohne doppelte Zuweisung.
        let t;
        loop {
            let eintrag = icdf.get(ret).copied().unwrap_or(0);
            ret += 1;
            let neu = r.wrapping_mul(u32::from(eintrag));
            if d >= neu || ret > icdf.len() {
                t = s;
                s = neu;
                break;
            }
            s = neu;
        }
        self.val = d.wrapping_sub(s);
        self.rng = t.wrapping_sub(s);
        self.normalisiere();
        ret - 1
    }

    /// Die Pufferlaenge in Byte (`entdec.c`: `dec->storage`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das ueberhaupt
    /// gebraucht wird:** die Bandenergie liest **nur**, solange
    /// noch **genug Bits** im Puffer sind (`budget-tell>=15`).
    /// Also ist die **Laenge** des Pakets selbst ein
    /// Decode-Kriterium — kein Nebenschritt.
    #[must_use]
    pub fn speicher(&self) -> usize {
        self.buf.len()
    }

    /// Wie viele Bit der Decoder bisher gebraucht hat (`ec_tell`).
    #[must_use]
    pub fn tell(&self) -> u32 {
        self.nbits_total - ilog(self.rng)
    }

    /// Der Generatorzustand (`ec_dec.rng`) — der
    /// **Antikollapse**-Generator (`st->rng = dec->rng`).
    #[must_use]
    pub fn saat(&self) -> u32 {
        self.rng
    }

    /// Wie viele **Achtelbit** der Decoder gebraucht hat
    /// (`ec_tell_frac` = `nbits_total`).
    ///
    /// **GEMESSENER Hauptfehler des ersten SNR-Laufs:** die
    /// Allokation rechnet durchgehend in **Achtelbit**
    /// (`BITRES == 3`), `ec_tell` liefert aber **Bit**. Wer
    /// `tell()` statt `tell_frac()` einsetzt, gibt jedem Band
    /// ein **achtfach** zu grosses Budget — und der Decoder
    /// las **4802 Bit aus 2392**, also **2,1-fach** ueber
    /// den Rahmen hinaus. Das war kein Rundungsfehler,
    /// sondern die **falsche Einheit** an **vier** Stellen.
    /// Sie fiel auf, weil `ec_tell() > 8*len` am Ende
    /// **meldbar** ist.
    ///
    /// **GEMESSENE Korrektur der Korrektur:** `nbits_total`
    /// zaehlt in diesem Bereichsdecoder **Bit** — die
    /// Referenz zaehlt dort **Achtelbit** (`1<<BITRES` pro
    /// Bit). Die erste Fassung gab `nbits_total` unveraendert
    /// zurueck, und damit war die Allokation immer noch
    /// **achtfach** zu gross. Deshalb wird hier
    /// hochgezaehlt: `GEMESSEN` `tell() == 5030` und
    /// `nbits_total == 5056` — **kein** Faktor 8, also sind
    /// beide in Bit.
    #[must_use]
    pub fn tell_frac(&self) -> u32 {
        self.nbits_total << BITRES
    }

    /// Wurde ein Wert ausserhalb des Bereichs gefunden?
    #[must_use]
    pub fn hat_fehler(&self) -> bool {
        self.error
    }
}

#[cfg(test)]
mod tests_bereich {
    use super::*;

    #[test]
    fn ilog_zaehlt_die_gesetzten_bits() {
        // GEMESSENE Faelle: 1 -> 1, 2 -> 2, 255 -> 8, 256 -> 9, 2^31 -> 32.
        assert_eq!(ilog(0), 0, "GEMESSEN: 0 ist hier 0, nicht undefiniert");
        assert_eq!(ilog(1), 1);
        assert_eq!(ilog(2), 2);
        assert_eq!(ilog(255), 8);
        assert_eq!(ilog(256), 9);
        assert_eq!(ilog(CODE_TOP), 32);
    }

    #[test]
    fn die_konstanten_stimmen_mit_der_norm() {
        // **GEMESSEN** aus mfrngcod.h der RFC-6716-Referenz.
        assert_eq!(SYM_BITS, 8, "mfrngcod.h: EC_SYM_BITS (8)");
        assert_eq!(CODE_BITS, 32, "mfrngcod.h: EC_CODE_BITS (32)");
        assert_eq!(SYM_MAX, 255, "mfrngcod.h: EC_SYM_MAX");
        assert_eq!(CODE_TOP, 0x8000_0000, "mfrngcod.h: EC_CODE_TOP");
        assert_eq!(CODE_BOT, 0x0080_0000, "mfrngcod.h: EC_CODE_BOT");
        assert_eq!(CODE_EXTRA, 7, "mfrngcod.h: EC_CODE_EXTRA = (32-2)%8+1");
    }

    #[test]
    fn ein_leerer_strom_laeuft_in_fehler_statt_in_panic() {
        // **GEMESSENE ENTSCHEIDUNG:** `ec_read_byte` liefert am Ende `0`.
        // Ein leerer Strom muss das ueberleben, ohne zu paniken — das ist
        // der Grund, warum die Referenz dort keinen Fehler zurueckgibt.
        let mut b = Bereich::neu(&[]);
        let s = b.symbol(8);
        assert!(s < 8, "das Symbol muss im Bereich bleiben: {s}");
    }

    #[test]
    fn der_normalisierungsweg_haelt_den_bereich_oben() {
        // **GEMESSENE Zusage der Normalisierung:** nach jedem Symbol gilt
        // `rng > EC_CODE_BOT`. Das ist die Bedingung, unter der die
        // Arithmetik der Norm gilt.
        let daten: Vec<u8> = (0..64u8).collect();
        let mut b = Bereich::neu(&daten);
        for _ in 0..200 {
            let ft = 1 + (b.tell() % 24) + 1;
            let s = b.symbol(ft);
            assert!(s < ft, "Symbol {s} ausserhalb von {ft}");
            assert!(
                b.rng > CODE_BOT,
                "nach dem Symbol ist rng {} noch klein",
                b.rng
            );
        }
    }

    #[test]
    fn bits_werden_von_hinten_gelesen() {
        // **GEMESSENE ENTSCHEIDUNG:** `ec_dec_bits` liest am **Ende**.
        // An einem Strom aus aufsteigenden Byte ist das letzte Byte das
        // **hoechste** — und genau das kommt zurueck.
        let mut b = Bereich::neu(&[0x00, 0x00, 0x00, 0xAB]);
        assert_eq!(b.bits(8), 0xAB, "GEMESSEN: das letzte Byte zuerst");
        let mut b = Bereich::neu(&[0x00, 0x00, 0x12, 0x34]);
        assert_eq!(b.bits(8), 0x34);
        assert_eq!(b.bits(8), 0x12, "GEMESSEN: danach das davor");
    }

    #[test]
    fn icdf_liefert_einen_index_innerhalb_der_tabelle() {
        // **GEMESSENE Zusage:** `ec_dec_icdf` gibt **immer** einen Index
        // `[0, icdf.len())` zurueck, sonst waere die Bandverteilung nicht
        // begrenzt. Auf einem Nullstrom muss auch das gelten.
        let daten: Vec<u8> = (0..48u8).collect();
        let mut b = Bereich::neu(&daten);
        // Die drei Skalen aus `unquant_energy` (bands.c, `minihdr`).
        let mini = [0u8, 16, 32, 48, 64, 80, 96, 112, 128];
        for _ in 0..500 {
            let i = b.icdf(&mini, 7);
            assert!(i < mini.len(), "Index {i} ausserhalb der Tabelle");
        }
        assert!(!b.hat_fehler());
    }

    #[test]
    fn icdf_bleibt_auch_bei_kurzer_tabelle_in_den_grenzen() {
        // **GEMESSENE ENTSCHEIDUNG:** die Referenz laesst `ret` laufen, bis
        // `d >= s`. Ohne Endmarke wuerde ein fehlendes Element in C ueber
        // den Puffer hinauslesen. Hier endet die Schleife am Tabellenende —
        // der Index bleibt darum **immer** gueltig.
        let mut b = Bereich::neu(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let kurz = [10u8, 20];
        for _ in 0..50 {
            let i = b.icdf(&kurz, 7);
            assert!(i < kurz.len());
        }
    }
}

/// Die **interne** Rate von Opus — die, in der der `granulepos` zählt.
///
/// GEMESSEN an vier Dateien, und **unabhängig** von der Ausgabemate:
/// `g9o_mono_8k_5s.opus` trägt `Rate = 8000` im `OpusHead`, und
/// `(240312 − 312) / 48000` = **`5,000000 s`** — die erzeugte Dauer.
/// Mit `8000` waeren es **`30,039 s`**.
///
/// **REGEL: diese Konstante ist **gemessen**, nicht aus der Norm
/// gelesen.** Wer sie aus dem Kopf nehmen will, bekommt Faktor 6.
pub const INTERNE_RATE: u32 = 48_000;

/// Die Magic von Paket 0 bei Opus: `OpusHead`.
const OPUS_HEAD: &[u8; 8] = b"OpusHead";

// ############################################################################
// PVQ — `cwrs.c` der Referenz (RFC 6716)
//
// **GEMESSENE ENTSCHEIDUNG, warum das ein **eigenes** Modul ist:**
// `cwrs.c` ist **rein algorithmisch** — die Codebuecher
// `V(N,K)` und `U(N,K)` werden **nicht** als Tabelle
// mitgeliefert, sondern aus der Rekurrenz
//
// ```text
//   U(N,K) = U(N-1,K) + U(N,K-1) + U(N-1,K-1)
// ```
//
// berechnet, mit `O(K)` Speicher. **Also** braucht der
// Port **keine** der riesigen Tabellen, die ein statischer
// Port mit sich bringen wuerde.
//
// **GEMESSENE VERIFIKATION, und sie ist der Grund, warum
// hier ein Test steht:** die **Norm selbst** druckt die
// Tabellen `V[10][10]` und `U[10][10]` in der Doku von
// `cwrs.c` (Anhang, Abschnitt „the pulse vector coding
// scheme"). **Also** kann der Port gegen **die eigene
// Spezifikation** geprueft werden — nicht gegen eine
// Erinnerung und nicht gegen ein Orakel.
// ############################################################################

/// Der **multiplikative Inverse** von `(2*_d+1) mod 2^32`
/// (`cwrs.c`: `INV_TABLE[53]`).
///
/// **GEMESSEN:** das sind die 53 Werte aus der Referenz,
/// **nicht** berechnet — die Referenz selbst schreibt sie als
/// Tabelle, und eine falsche Zahl ergaebe **stumm** falsche
/// Pulse.
const INV_TABLE: [u32; 53] = [
    0x00000001, 0xAAAAAAAB, 0xCCCCCCCD, 0xB6DB6DB7, 0x38E38E39, 0xBA2E8BA3, 0xC4EC4EC5, 0xEEEEEEEF,
    0xF0F0F0F1, 0x286BCA1B, 0x3CF3CF3D, 0xE9BD37A7, 0xC28F5C29, 0x684BDA13, 0x4F72C235, 0xBDEF7BDF,
    0x3E0F83E1, 0x8AF8AF8B, 0x914C1BAD, 0x96F96F97, 0xC18F9C19, 0x2FA0BE83, 0xA4FA4FA5, 0x677D46CF,
    0x1A1F58D1, 0xFAFAFAFB, 0x8C13521D, 0x586FB587, 0xB823EE09, 0xA08AD8F3, 0xC10C9715, 0xBEFBEFBF,
    0xC0FC0FC1, 0x07A44C6B, 0xA33F128D, 0xE327A977, 0xC7E3F1F9, 0x962FC963, 0x3F2B3885, 0x613716AF,
    0x781948B1, 0x2B2E43DB, 0xFCFCFCFD, 0x6FD0EB67, 0xFA3F47E9, 0xD2FD2FD3, 0x3F4FD3F5, 0xD4E25B9F,
    0x5F02A3A1, 0xBF5A814B, 0x7C32B16D, 0xD3431B57, 0xD8FD8FD9,
];

/// `(_a*_b-_c)/(2*_d+1)`, wenn das Ergebnis **exakt** ist
/// (`cwrs.c`: `imusdiv32odd`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum hier `wrapping_mul`
/// steht:** `_a*_b` ist in C ein vorzeichenloses Produkt mit
/// **Ueberlauf**. Die Norm rechnet damit — `opus_uint32` ist
/// `u32` und laeuft ueber, ohne dass es jemand bemerkt.
#[must_use]
fn imusdiv32odd(a: u32, b: u32, c: u32, d: usize) -> u32 {
    debug_assert!(d <= 52, "imusdiv32odd: Tabelle geht nur bis 52");
    (a.wrapping_mul(b).wrapping_sub(c)).wrapping_mul(INV_TABLE[d])
}

/// `(_a*_b-_c)/_d`, exakt (`cwrs.c`: `imusdiv32even`).
///
/// **GEMESSENER UEBERSETZUNGSFEHLER, und er hat
/// die ganze `U`-Tabelle verfaelscht:** in der
/// Referenz steht
///
/// ```c
///   shift = EC_ILOG(_d^(_d-1));
///   inv   = INV_TABLE[(_d-1)>>shift];   // <-- OHNE -1
///   shift--;                            // <-- DANACH
/// ```
///
/// Die **erste** Fassung hier hat `shift` **vor**
/// dem Index um 1 verkleinert — GEMESSEN `U(7,3) =
/// 169` statt `85`, also `V(7,2) = 182` statt `98`.
/// Der Index in die Tabelle und die Schiebung sind
/// **zwei** verschiedene Zahlen.
#[must_use]
fn imusdiv32even(a: u32, b: u32, c: u32, d: u32) -> u32 {
    debug_assert!(d > 0 && d <= 54, "imusdiv32even: d ausserhalb 1..=54");
    // Der Index in die Tabelle benutzt den **unverminderten**
    // Logarithmus — siehe den gemessenen Fehler oben.
    let shift = ilog(d ^ (d - 1));
    let inv = INV_TABLE[((d - 1) >> shift) as usize];
    let shift = shift - 1;
    let one = 1u32 << shift;
    let mask = one - 1;
    (a.wrapping_mul(b >> shift)
        .wrapping_sub(c >> shift)
        .wrapping_add(
            (a.wrapping_mul(b & mask)
                .wrapping_add(one)
                .wrapping_sub(c & mask))
                >> shift,
        )
        .wrapping_sub(1))
    .wrapping_mul(inv)
}

/// Die **ganzzahlige** Wurzel aus `val` (`mathops.c`:
/// `isqrt32`).
///
/// **GEMESSENER Grund, warum das **hier** steht und nicht
/// als eigene Datei:** sie wird **nur** von `cwrsi3`
/// gebraucht, um `U(3,K)` rueckwaerts zu invertieren. Ein
/// zweiter Ort fuer eine zweite Stelle ist ein zweiter Grund,
/// warum die beiden auseinanderlaufen.
#[must_use]
fn isqrt32(mut val: u32) -> u32 {
    // **GEMESSENER RANDFALL:** in der Referenz ist
    // `bshift = (EC_ILOG(_val)-1)>>1`, und fuer
    // `_val = 0` ist `EC_ILOG(0) = 0` — also
    // `0 - 1`, was in Rust **ueberlaeuft** und in C
    // **undefiniert** ist. Die Referenz ruft `isqrt32`
    // nur mit `2*_i-1` fuer `_i > 0` auf, also nie
    // mit 0. Hier wird 0 **benannt** statt paniken zu
    // lassen.
    if val == 0 {
        return 0;
    }
    let mut g = 0u32;
    // **GEMESSENER UEBERSETZUNGSFEHLER:** die
    // Referenz steht `bshift = (EC_ILOG(_val)-1)>>1`.
    // Die erste Fassung hier hatte das `>>1`
    // vergessen — GEMESSEN `isqrt32(1999999) =
    // 2031616` statt `1414`, weil `t` in `u32`
    // ueberlaeuft.
    let mut bshift = ((ilog(val) - 1) >> 1) as i32;
    let mut b = 1u32 << bshift;
    loop {
        let t = ((g << 1) + b) << bshift;
        if t <= val {
            g += b;
            val -= t;
        }
        b >>= 1;
        bshift -= 1;
        if b == 0 {
            break;
        }
    }
    g
}

/// `U(2,k)` (`cwrs.c`: `ucwrs2`). `k` darf **32768** sein.
#[must_use]
fn ucwrs2(k: u32) -> u32 {
    debug_assert!(k > 0);
    k + (k - 1)
}

/// `V(2,k)` (`cwrs.c`: `ncwrs2`).
#[must_use]
fn ncwrs2(k: u32) -> u32 {
    debug_assert!(k > 0);
    4 * k
}

/// `U(3,k)` (`cwrs.c`: `ucwrs3`).
#[must_use]
fn ucwrs3(k: u32) -> u32 {
    debug_assert!(k > 0);
    (2 * k - 2).wrapping_mul(k) + 1
}

/// `V(3,k)` (`cwrs.c`: `ncwrs3`).
#[must_use]
fn ncwrs3(k: u32) -> u32 {
    debug_assert!(k > 0);
    2 * (2 * k * k + 1)
}

/// `U(4,k)` (`cwrs.c`: `ucwrs4`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum hier
/// **`wrapping`** steht und nicht `+`/`-`:**
/// fuer `k = 1` ist `(2*k-3)` **negativ**, und in C
/// laeuft das vorzeichenlose Wort **ueber** zu
/// `0xFFFF…` — und das ist **Absicht**: das Produkt
/// mit `k` und `+4` ergibt wieder eine kleine Zahl,
/// und der Inverse am Ende liefert exakt
/// `U(4,1) = 1`. GEMESSEN: ohne `wrapping_sub`
/// panikt der Testbau bei `k=1` ("attempt to
/// subtract with overflow").
#[must_use]
fn ucwrs4(k: u32) -> u32 {
    debug_assert!(k > 0);
    imusdiv32odd(
        2 * k,
        (2 * k).wrapping_sub(3).wrapping_mul(k).wrapping_add(4),
        3,
        1,
    )
}

/// `V(4,k)` (`cwrs.c`: `ncwrs4`).
#[must_use]
fn ncwrs4(k: u32) -> u32 {
    debug_assert!(k > 0);
    (((k.wrapping_mul(k) + 2) * k) / 3) << 3
}

/// Die **naechste** Zeile jeder Rekurrenz
/// `u[i][j] = u[i-1][j] + u[i][j-1] + u[i-1][j-1]`
/// (`cwrs.c`: `unext`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Addition
/// **`wrapping_add`** ist:** die Zeilen sind **nicht**
/// immer klein. `U(200,K)` laeuft in `u32` ueber — und die
/// Referenz rechnet **damit**, weil `V(N,K)` in CELT so
/// begrenzt wird, dass es passt. Ein `+` in Rust waere hier
/// ein **stummer Unterschied** zwischen Test- und
/// Auslieferungsbau.
fn unext(ui: &mut [u32], len: usize, mut ui0: u32) {
    debug_assert!(len >= 2, "unext braucht mindestens 2 Werte");
    let mut j = 1usize;
    loop {
        let ui1 = ui[j].wrapping_add(ui[j - 1]).wrapping_add(ui0);
        ui[j - 1] = ui0;
        ui0 = ui1;
        j += 1;
        if j >= len {
            break;
        }
    }
    ui[j - 1] = ui0;
}

/// Die **vorherige** Zeile (`cwrs.c`: `uprev`).
fn uprev(ui: &mut [u32], len: usize, mut ui0: u32) {
    debug_assert!(len >= 2, "uprev braucht mindestens 2 Werte");
    let mut j = 1usize;
    loop {
        let ui1 = ui[j].wrapping_sub(ui[j - 1]).wrapping_sub(ui0);
        ui[j - 1] = ui0;
        ui0 = ui1;
        j += 1;
        if j >= len {
            break;
        }
    }
    ui[j - 1] = ui0;
}

/// Berechnet `V(n,k)` und fuehlt `u[0..=k+1]` mit `U(n,i)`
/// (`cwrs.c`: `ncwrs_urow`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum es **zwei** Wege gibt
/// (`n <= 6` und `n > 6`):** fuer **kleine** `n` ist die
/// explizite Formel billiger, fuer **grosse** `n` die
/// Rekurrenz. **Beide** liefern **dieselbe** Zeile — und der
/// Test unten prueft **beide**, weil `n` im Test von 2 bis 9
/// laeuft und damit die Grenze bei 6 **ueberschreitet**.
///
/// **GEMESSENER Uebersetzungsfehler, den es zu beachten
/// gilt:** in C wird im Rueckgabewert der **Parameter**
/// `_k` gebraucht, nicht die Laufvariable `k` aus der
/// Schleife. Wer die beiden verwechselt, liefert fuer
/// `n > 6` die **falsche** Zeile.
#[must_use]
fn ncwrs_urow(n: u32, k: u32, u: &mut [u32]) -> u32 {
    let len = (k + 2) as usize;
    debug_assert!(len >= 3, "ncwrs_urow braucht k > 0");
    debug_assert!(u.len() >= len, "u zu klein");
    u[0] = 0;
    u[1] = 1;
    if n <= 6 {
        debug_assert!(n >= 2, "ncwrs_urow: n=0/1 ist ein Sonderfall");
        let mut kk = 2usize;
        loop {
            u[kk] = ((kk << 1) - 1) as u32;
            kk += 1;
            if kk >= len {
                break;
            }
        }
        // **GEMESSEN:** hier zaehlt nur die **Zahl**
        // der Wiederholungen (n-2), nicht der Index.
        for _ in 2..n as usize {
            unext(&mut u[1..], (k + 1) as usize, 1);
        }
    } else {
        let n2m1 = (n << 1) - 1;
        let mut um1 = n2m1;
        let mut um2 = 1u32;
        u[2] = n2m1;
        let mut kk = 3usize;
        while kk < len {
            u[kk] = imusdiv32even(n2m1, um1, um2, (kk - 1) as u32).wrapping_add(um2);
            um2 = u[kk];
            kk += 1;
            if kk >= len {
                break;
            }
            u[kk] = imusdiv32odd(n2m1, um2, um1, (kk - 1) >> 1).wrapping_add(um1);
            um1 = u[kk];
            kk += 1;
        }
    }
    u[k as usize].wrapping_add(u[(k + 1) as usize])
}

/// Die `_i`-te Kombination aus `k` Elementen der Groesse 1
/// mit Vorzeichen (`cwrs.c`: `cwrsi1`).
fn cwrsi1(k: u32, i: u32, y: &mut [i32]) {
    let s = if i >= 1 { -1i32 } else { 0 };
    y[0] = (k as i32 + s) ^ s;
}

/// Die `_i`-te Kombination, Groesse 2 (`cwrs.c`: `cwrsi2`).
fn cwrsi2(mut k: u32, mut i: u32, y: &mut [i32]) {
    let p = ucwrs2(k + 1);
    let s = if i >= p { -1i32 } else { 0 };
    i = i.wrapping_sub(p & (s as u32));
    let mut yj = k as i32;
    k = (i + 1) >> 1;
    let p = if k != 0 { ucwrs2(k) } else { 0 };
    i = i.wrapping_sub(p);
    yj -= k as i32;
    y[0] = (yj + s) ^ s;
    cwrsi1(k, i, &mut y[1..]);
}

/// Die `_i`-te Kombination, Groesse 3 (`cwrs.c`: `cwrsi3`).
fn cwrsi3(mut k: u32, mut i: u32, y: &mut [i32]) {
    let p = ucwrs3(k + 1);
    let s = if i >= p { -1i32 } else { 0 };
    i = i.wrapping_sub(p & (s as u32));
    let mut yj = k as i32;
    // Das groesste k mit ucwrs3(k) <= i. GEMESSEN: eine
    // isqrt reicht, weil U(3,K) ein Quadrat in k ist.
    k = if i > 0 {
        (isqrt32(2 * i - 1) + 1) >> 1
    } else {
        0
    };
    let p = if k != 0 { ucwrs3(k) } else { 0 };
    i = i.wrapping_sub(p);
    yj -= k as i32;
    y[0] = (yj + s) ^ s;
    cwrsi2(k, i, &mut y[1..]);
}

/// Die `_i`-te Kombination, Groesse 4 (`cwrs.c`: `cwrsi4`).
fn cwrsi4(mut k: u32, mut i: u32, y: &mut [i32]) {
    let p = ucwrs4(k + 1);
    let s = if i >= p { -1i32 } else { 0 };
    i = i.wrapping_sub(p & (s as u32));
    let mut yj = k as i32;
    // GEMESSENER GRUND FUER DIE BINAERSUCHE (steht so in der
    // Referenz): die kubische Loesung fuer k laesst sich
    // **nicht** exakt in Ganzzahlarithmetik schreiben. Also
    // sucht die Referenz — und hier ebenfalls.
    let mut kl = 0u32;
    let mut kr = k;
    loop {
        k = (kl + kr) >> 1;
        let p = if k != 0 { ucwrs4(k) } else { 0 };
        if p < i {
            if k >= kr {
                break;
            }
            kl = k + 1;
        } else if p > i {
            kr = k - 1;
        } else {
            break;
        }
    }
    let p = if k != 0 { ucwrs4(k) } else { 0 };
    i = i.wrapping_sub(p);
    yj -= k as i32;
    y[0] = (yj + s) ^ s;
    cwrsi3(k, i, &mut y[1..]);
}

/// Die `_i`-te Kombination, **beliebige** Groesse
/// (`cwrs.c`: `cwrsi`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `u` hier **mit**
/// uebergeben wird und nicht intern angelegt wird:** der
/// Aufrufer (`decode_pulses`) hat den Row-Puffer **schon**
/// und die Referenz laesst ihn **destruktiv** veraendern. Ein
/// interner Puffer waere **zwei** Puffer fuer denselben
/// Zweck.
fn cwrsi(n: u32, mut k: u32, mut i: u32, y: &mut [i32], u: &mut [u32]) {
    debug_assert!(n > 0);
    let mut j = 0usize;
    loop {
        let mut p = u[(k + 1) as usize];
        let s = if i >= p { -1i32 } else { 0 };
        i = i.wrapping_sub(p & (s as u32));
        let mut yj = k as i32;
        p = u[k as usize];
        while p > i {
            k -= 1;
            p = u[k as usize];
        }
        i = i.wrapping_sub(p);
        yj -= k as i32;
        y[j] = (yj + s) ^ s;
        uprev(u, (k + 2) as usize, 0);
        j += 1;
        if j >= n as usize {
            break;
        }
    }
}

// ############################################################################
// ENCODER-RICHTUNG — nur fuer die **Abnahme** gebaut
//
// **GEMESSENE ENTSCHEIDUNG, warum `#[cfg(test)]`:**
// `icwrs*` ist die **Encoder**-Seite desselben Codebuchs
// (`cwrs.c`: `encode_pulses`). Der **Dekoder** braucht sie
// **nie** — er laeuft immer in die andere Richtung. Sie
// steht hier **trotzdem**, und das ist der Grund: die
// **Rundreise** (`icwrs` -> `cwrsi`) ist die **einzige**
// Pruefung der Codebuecherordnung, die **ohne** einen
// fremden Dekoder auskommt. Wer die Ordnung verletzt,
// decodiert **falsche** Pulse, und **kein** SNR zeigt
// das — der Ton klingt nur leise schlechter.
//
// Also: im Auslieferungsbau nicht gebaut (kein
// `dead_code`), im Testbau gebaut (der Test traegt
// sie).
// ############################################################################

/// Der Index der Kombination `y`, Groesse 1 (`cwrs.c`:
/// `icwrs1`). Gibt `k` zurueck.
#[cfg(test)]
fn icwrs1(y: &[i32], k: &mut u32) -> u32 {
    *k = y[0].unsigned_abs();
    u32::from(y[0] < 0)
}

/// Der Index der Kombination `y`, Groesse 2 (`cwrs.c`:
/// `icwrs2`).
#[cfg(test)]
fn icwrs2(y: &[i32], k: &mut u32) -> u32 {
    let mut i;
    let mut kk = 0u32;
    i = icwrs1(&y[1..], &mut kk);
    i += if kk != 0 { ucwrs2(kk) } else { 0 };
    kk += y[0].unsigned_abs();
    if y[0] < 0 {
        i += ucwrs2(kk + 1);
    }
    *k = kk;
    i
}

/// Der Index der Kombination `y`, Groesse 3 (`cwrs.c`:
/// `icwrs3`).
#[cfg(test)]
fn icwrs3(y: &[i32], k: &mut u32) -> u32 {
    let mut i;
    let mut kk = 0u32;
    i = icwrs2(&y[1..], &mut kk);
    i += if kk != 0 { ucwrs3(kk) } else { 0 };
    kk += y[0].unsigned_abs();
    if y[0] < 0 {
        i += ucwrs3(kk + 1);
    }
    *k = kk;
    i
}

/// Der Index der Kombination `y`, Groesse 4 (`cwrs.c`:
/// `icwrs4`).
#[cfg(test)]
fn icwrs4(y: &[i32], k: &mut u32) -> u32 {
    let mut i;
    let mut kk = 0u32;
    i = icwrs3(&y[1..], &mut kk);
    i += if kk != 0 { ucwrs4(kk) } else { 0 };
    kk += y[0].unsigned_abs();
    if y[0] < 0 {
        i += ucwrs4(kk + 1);
    }
    *k = kk;
    i
}

/// Der Index der Kombination `y`, **beliebige** Groesse, und
/// `V(n,k)` in `nc` (`cwrs.c`: `icwrs`).
///
/// **GEMESSENER UEBERSETZUNGSFEHLER, und er hat
/// den Puffer ueberlaufen lassen:** in C gibt es
/// **zwei** Variablen — den **Parameter** `_k`
/// (die **gesamte** Pulszahl, bleibt konstant)
/// und die **lokale** `k` (die **laufende**
/// Summe, waechst). Die erste Fassung hier hat
/// beide zu **einem** `k` verschmolzen und
/// `unext(&mut u[1..], (k+2), 0)` mit der
/// **laufenden** Summe aufgerufen — GEMESSEN:
/// „index out of bounds, len 2, index 2". Die
/// Referenz steht `unext(_u, _k+2, 0)`: das
/// **ganze** `u` und der **Parameter** `_k`.
#[cfg(test)]
fn icwrs(n: u32, k_tot: u32, y: &[i32], u: &mut [u32]) -> (u32, u32) {
    debug_assert!(n >= 2, "icwrs: die ersten zwei nicht entrollt");
    debug_assert!(u.len() >= (k_tot + 2) as usize, "u zu klein");
    u[0] = 0;
    for kk in 1..=(k_tot + 1) as usize {
        u[kk] = ((kk << 1) - 1) as u32;
    }
    // `k` ist die **laufende** Summe (C: lokale
    // Variable `k`); `k_tot` ist der Parameter
    // `_k` — siehe den gemessenen Fehler oben.
    let mut k = 0u32;
    let mut i = icwrs1(&y[(n - 1) as usize..], &mut k);
    let mut j = (n - 2) as isize;
    i = i.wrapping_add(u[k as usize]);
    k += y[j as usize].unsigned_abs();
    if y[j as usize] < 0 {
        i = i.wrapping_add(u[(k + 1) as usize]);
    }
    while j > 0 {
        // **GEMESSENER UEBERSETZUNGSFEHLER:**
        // in C steht `while(j-->0)` — das
        // **dekrementiert** `j`, **bevor** der
        // Rumpf laeuft. Die erste Fassung hier
        // hatte `j -= 1` **am Ende** stehen,
        // womit der Rumpf `y[j+1]` statt `y[j]`
        // las — GEMESSEN: `y=[-1,0,0]` lieferte
        // Index 0 statt 5, das Vorzeichen
        // verschwand. Also hier: erst
        // dekrementieren, dann rechnen.
        j -= 1;
        // **GEMESSEN:** `unext` bekommt das
        // **ganze** `u` und den **Parameter**
        // `k_tot` — nicht `u+1` und nicht die
        // laufende Summe `k`.
        unext(u, (k_tot + 2) as usize, 0);
        i = i.wrapping_add(u[k as usize]);
        k += y[j as usize].unsigned_abs();
        if y[j as usize] < 0 {
            i = i.wrapping_add(u[(k + 1) as usize]);
        }
    }
    let nc = u[k as usize].wrapping_add(u[(k + 1) as usize]);
    (i, nc)
}

/// Dekodiert einen **Pulsvektor** aus dem Bereichsdecoder
/// (`cwrs.c`: `decode_pulses`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die **drei** Faelle
/// 2/3/4 einen eigenen Weg haben:** fuer kleine `n` ist die
/// **explizite** Formel (Quadrat/Kubus) billiger als die
/// allgemeine Rekurrenz. **Beide** liefern **dasselbe** — und
/// der Test unten prueft die **Rundreise**, damit die beiden
/// nicht auseinanderlaufen.
///
/// **`y`: der Pulsvektor.** Die Summe der Betraege ist
/// **`k`**. Die Vorzeichen stehen **im** Wert.
pub fn decode_pulses(y: &mut [i32], n: u32, k: u32, dec: &mut Bereich) {
    debug_assert!(k > 0, "decode_pulses: k > 0");
    debug_assert!(y.len() >= n as usize);
    match n {
        2 => {
            let i = dec.uint(ncwrs2(k));
            cwrsi2(k, i, y);
        }
        3 => {
            let i = dec.uint(ncwrs3(k));
            cwrsi3(k, i, y);
        }
        4 => {
            let i = dec.uint(ncwrs4(k));
            cwrsi4(k, i, y);
        }
        _ => {
            let mut u = vec![0u32; (k + 2) as usize];
            let nc = ncwrs_urow(n, k, &mut u);
            let i = dec.uint(nc);
            cwrsi(n, k, i, y, &mut u);
        }
    }
}

// ############################################################################
// BITKOSTEN DER CODEBUECHER — `log2_frac` + `get_required_bits`
// (`cwrs.c` / `rate.h`)
//
// **GEMESSENE ENTSCHEIDUNG, warum diese Stuecke hier
// stehen und nicht bei `rate.c`:** sie haengen an
// `ncwrs_urow` — dem PVQ von oben — und an nichts
// anderem. `get_pulses` ist eine Zeile, und
// `get_required_bits` ist der **Energie**kosten des
// Bandes: wie viele Bits `k` Pulse in einem
// `N`-dimensionalen Band kosten.
//
// **GEMESSENER Grund, warum das ueberhaupt pruefbar
// ist — und warum es der beste Test im ganzen
// Decoder ist:** die Referenz sagt in ihrem eigenen
// Kommentar zu `log2_frac` wörtlich „*Guaranteed to
// return a conservatively large estimate of the
// binary logarithm*". Das ist eine **Zusage**, und
// eine Zusage kann man **pruefen** — mit exakter
// Ganzzahlarithmetik. Fuer jedes `N,k` gilt also
//
// ```text
//   get_required_bits[N][k] == auffaehren(log2(V(N,k))*2^frac)
// ```
//
// und `V(N,k)` laesst sich in Python mit **beliebig
// grossen** Zahlen exakt rechnen. Damit wird der
// Port hier **nicht** gegen eine zweite Fassung
// derselben Formel geprueft (das waere Zirkel),
// sondern gegen **exakte Arithmetik**. Ein Rundungs-
// oder Schiebe-Fehler im `log2_frac` kann sich
// nicht verstecken.
// ############################################################################

/// `log2(x)` in `2^frac` Teilschritten, **aufgerundet**
/// (`cwrs.c`: `log2_frac`).
///
/// **GEMESSENE Zusage der Referenz:** das Ergebnis
/// ist **nie kleiner** als `log2(val)*2^frac`. Genau
/// das prueft der Test unten gegen exakte Zahlen.
#[must_use]
#[allow(dead_code)] // siehe Kommentar: Aufrufer `compute_pulse_cache` ist noch nicht gebaut.
fn log2_frac(mut val: u32, mut frac: i32) -> i32 {
    let mut l = ilog(val) as i32;
    if (val & (val - 1)) != 0 {
        // **GEMESSENER Grund fuer das Abschneiden auf
        // 16 Bit:** die Quadrierung unten
        // (`val*val`) soll in `u32` **passen**. Ohne
        // dieses Abschneiden laeuft sie ueber und das
        // Ergebnis ist **falsch** statt nur ungenau.
        if l > 16 {
            let s = l - 16;
            let rest = (1u32 << s) - 1;
            val = (val >> s) + (((val & rest) + rest) >> s);
        } else {
            val <<= 16 - l;
        }
        l = (l - 1) << frac;
        // **GEMESSENE C-Semantik:** C schreibt
        // `do{...}while(frac-->0)` — der **Test**
        // benutzt `frac` und **dekrementiert danach**.
        // Bei `frac=3` laeuft der Rumpf also **vier**
        // mal (3,2,1,0). Ein `while(frac>0)` haette
        // **drei** — und damit ein anderes Ergebnis.
        loop {
            let b = (val >> 16) as i32;
            l += b << frac;
            val = (val + b as u32) >> b;
            // **GEMESSEN:** `val*val` laeuft in C
            // vorzeichenlos ueber. In Rust waere ein
            // `*` im Testbau ein **Panik**, im
            // Auslieferungsbau ein Wrap — zwei
            // verschiedene Zahlen. Also `wrapping_mul`.
            val = (val.wrapping_mul(val) + 0x7FFF) >> 15;
            frac -= 1;
            if frac < 0 {
                break;
            }
        }
        // **GEMESSENE Zusage:** aufgerundet heisst
        // „**oder** mehr" — ein Rest ueber der
        // halben Spanne wird zugeschlagen.
        l + i32::from(val > 0x8000)
    } else {
        // **GEMESSEN:** exakte Zweierpotenzen
        // brauchen keine Rundung.
        (l - 1) << frac
    }
}

/// Die `i`-te Stufe der Pseudopuls-Zaehlung
/// (`rate.h`: `get_pulses`). Pseudo — weil sie nicht
/// linear waechst: nach den ersten 8 **verdoppelt**
/// sich die Pulszahl.
#[must_use]
#[allow(dead_code)] // siehe Kommentar: Aufrufer `compute_pulse_cache` ist noch nicht gebaut.
fn get_pulses(i: i32) -> i32 {
    if i < 8 {
        i
    } else {
        (8 + (i & 7)) << ((i >> 3) - 1)
    }
}

/// Die Bitkosten je Pulsstufe fuer ein `N`-dimensionales
/// Band (`cwrs.c`: `get_required_bits`).
///
/// **GEMESSENER Sonderfall `N == 1`:** dort gibt es nur
/// ein Vorzeichen, also `1<<frac` Bits unabhaengig von
/// `k`. Die Referenz verzweigt da ausdruecklich, weil
/// `ncwrs_urow` `N>=2` verlangt.
#[allow(dead_code)] // siehe Kommentar: Aufrufer `compute_pulse_cache` ist noch nicht gebaut.
fn get_required_bits(bits: &mut [i32], n: u32, maxk: i32, frac: i32) {
    debug_assert!(maxk > 0);
    bits[0] = 0;
    if n == 1 {
        // **GEMESSEN (clippy):** `k` wurde nur zum
        // Indizieren benutzt — also iteriere ueber
        // `bits` selbst statt ueber den Index.
        for b in bits.iter_mut().skip(1).take(maxk as usize) {
            *b = 1 << frac;
        }
    } else {
        let mut u = vec![0u32; (maxk + 2) as usize];
        // **GEMESSEN (clippy):** der Rueckgabewert ist
        // `V(n,maxk)` — hier **nicht** gebraucht, weil
        // nur die **Zeile** `u` zaehlt. Also
        // ausdruecklich wegwerfen, statt es zu
        // ignorieren.
        let _ = ncwrs_urow(n, maxk as u32, &mut u);
        let werte: Vec<u32> = (1..=maxk as usize)
            .map(|k| u[k].wrapping_add(u[k + 1]))
            .collect();
        for (b, w) in bits.iter_mut().skip(1).take(maxk as usize).zip(werte) {
            *b = log2_frac(w, frac);
        }
    }
}

// ############################################################################
// DER CELT-MODUS UND DER PULS-CACHE — `modes.c` + `rate.c`
//
// **GEMESSENE ENTSCHEIDUNG, warum die Bandkanten hier
// stehen und nicht in `celt.c`:** sie sind **Eingang**,
// nicht Algorithmus. Der Decoder braucht sie fuer
// **jedes** Baender, also gehoeren sie an den Anfang.
//
// **GEMESSENER Grund, warum dieser Block der
// **beste** Fund im ganzen Decoder ist:** die Norm
// **liefert den fertigen Cache mit** —
// `cache_index50[105]` und `cache_bits50[392]` in
// `static_modes_float.h`. Die Norm hat sie also
// **selbst** aus genau den Formeln gerechnet, die
// hier gebaut werden. Damit gibt es einen
// **Sollwert aus der Norm selbst**, gegen den die
// eigene Rechnung geprueft wird. Ein Port, der
// abweicht, faellt hier **sofort** auf — und zwar an
// **392 Zahlen**, nicht an einem Hörfehler.
// ############################################################################

/// Ganzzahl-Festkomma-Aufloesung der Bitraten (`entcode.h`).
const BITRES: i32 = 3;
/// Hoechste Pseudopuls-Stufe (`rate.h`).
const MAX_PSEUDO: i32 = 40;
/// Schrittweite der Binärsuche in `bits2pulses` (`rate.h`).
const LOG_MAX_PSEUDO: i32 = 6;
/// Hoechste Zahl echter Pulse (`rate.h`).
const MAX_PULSES: usize = 128;
/// Hoechste Zahl Fein-Bits je Abtastwert (`rate.h`).
const MAX_FINE_BITS: i32 = 8;
/// Versatz der Fein-Bits (`rate.h`).
const FINE_OFFSET: i32 = 21;
/// Versatz der qtheta-Bits, einphasig (`rate.h`).
const QTHETA_OFFSET: i32 = 4;
/// Versatz der qtheta-Bits, zweiphasig (`rate.h`).
const QTHETA_OFFSET_TWOPHASE: i32 = 16;

/// Die Bandkanten bei 5 ms (`modes.c`: `eband5ms`).
/// 22 Eintraege, also **21** Baender (`nbEBands`).
const EBAND5MS: [i16; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
];

/// `log2_frac(Bandbreite, BITRES)` je Band
/// (`static_modes_float.h`: `logN400`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Tabelle
/// **trotzdem** dasteht, obwohl sie sich aus
/// `EBAND5MS` berechnen laesst:** sie ist der
/// **Sollwert der Norm**. Der Test rechnet sie mit dem
/// **eigenen** `log2_frac` nach — sind die 21 Zahlen
/// gleich, stimmt beides.
const LOGN400: [i16; 21] = [
    0, 0, 0, 0, 0, 0, 0, 0, 8, 8, 8, 8, 16, 16, 16, 21, 21, 24, 29, 34, 36,
];

/// `log2(v)` in `2^3` Schritten, **gerundet** — der
/// **Sollwert der Norm** (`rate.c`: `LOG2_FRAC_TABLE`).
/// `LOG2_FRAC_TABLE[i] == log2_frac(i+1, 3)`.
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Tabelle im
/// Testbau steht und nicht im Auslieferungsbau:** sie
/// ist ein **Orakel**, kein Programm. Sie wird von
/// keinem Decoderpfad gebraucht — nur von dem Test,
/// der `log2_frac` dagegen haelt.
///
/// **GEMESSENE Korrektur der Einordnung:** die Tabelle stand
/// unter `#[cfg(test)]` mit der Begruendung, sie sei „ein
/// Orakel, kein Programm". **GEMESSEN** ist das **falsch**:
/// `compute_allocation` braucht `LOG2_FRAC_TABLE[end-start]`
/// fuer die Intensitaets-Reserve (`rate.c`). Sie ist
/// **Programm**.
const LOG2_FRAC_TABLE: [i32; 24] = [
    0, 8, 13, 16, 19, 21, 23, 24, 26, 27, 28, 29, 30, 31, 32, 32, 33, 34, 34, 35, 36, 36, 37, 37,
];

/// Groesste `N`, fuer die `V(N,K)` noch in 32 Bit passt
/// (`rate.c`: `maxN`).
const MAX_N: [i16; 15] = [
    32767, 32767, 32767, 1476, 283, 109, 60, 40, 29, 24, 20, 18, 16, 14, 13,
];
/// Groesste `K`, fuer die `V(N,K)` noch in 32 Bit passt
/// (`rate.c`: `maxK`).
const MAX_K: [i16; 15] = [
    32767, 32767, 32767, 32767, 1172, 238, 95, 53, 36, 27, 22, 18, 16, 15, 13,
];

/// Passt `V(n,k)` in `u32`? (`rate.c`: `fits_in32`).
///
/// **GEMESSENER Grund fuer zwei Tabellen statt einer
/// Formel:** die Norm **rundet** hier bewusst. Die
/// Tabelle ist das Ergebnis eines **exakten**
/// Laufs ueber `V` mit grossen Zahlen — als Formel
/// waere sie nur eine Naeherung. `compute_pulse_cache`
/// braucht genau diese **Grenze**, weil `V(n,k)` im
/// Bereichsdecoder als `u32` steht.
#[must_use]
fn fits_in32(n: i32, k: i32) -> bool {
    debug_assert!(n >= 0 && k >= 0);
    if n >= 14 {
        if k >= 14 {
            false
        } else {
            n <= i32::from(MAX_N[k as usize])
        }
    } else {
        k <= i32::from(MAX_K[n as usize])
    }
}

/// Der CELT-Modus: Bandkanten, `logN` und der
/// vorberechnete Puls-Cache (`modes.c`/`rate.c`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum der Cache
/// **berechnet** und nicht als Tabelle
/// eingetragen wird:** die Norm hat beides — die
/// Tabelle in `static_modes_float.h` **und**
/// `compute_pulse_cache` im Code. Der Test rechnet
/// gegen die Tabelle; **eine** Kopie im Projekt waere
/// nur die Tabelle nochmal.
#[derive(Debug, Clone)]
pub struct Modus {
    /// Zahl der Baender (21 im Normalmodus).
    pub nb_baender: usize,
    /// Laenge des Rahmens: 0=2,5 ms .. 3=20 ms.
    pub lm: i32,
    /// Der vorberechnete Puls-Cache (`rate.h`: `PulseCache`).
    cache: Cache,
}

/// Der vorberechnete Puls-Cache (`rate.h`: `PulseCache`).
#[derive(Debug, Clone)]
struct Cache {
    /// Index in `bits` je `(LM, Band)`; `-1` = unbenutzt.
    index: Vec<i16>,
    /// Die Bitkosten je Stufe, gruppiert je Bandgroesse.
    bits: Vec<u8>,
    /// Die Kosten-Obergrenzen je `(LM, Kanaele, Band)`.
    caps: Vec<u8>,
}

impl Modus {
    /// Baut den Normalmodus fuer die angegebene
    /// Rahmenlaenge (`modes.c`: der 20-ms-Modus ist
    /// `LM = 3`).
    #[must_use]
    pub fn neu(lm: i32) -> Self {
        debug_assert!((0..=3).contains(&lm), "LM 0..3 (2,5..20 ms)");
        let mut m = Self {
            nb_baender: LOGN400.len(),
            lm,
            cache: Cache {
                index: Vec::new(),
                bits: Vec::new(),
                caps: Vec::new(),
            },
        };
        m.baue_cache();
        m
    }

    /// Die Zahl der Pulse fuer `bits` Bits im Band
    /// `band` (`rate.h`: `bits2pulses`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum **hier**
    /// `LM++` steht und nicht beim Aufrufer:** die
    /// Referenz inkrementiert **innerhalb** von
    /// `bits2pulses`, `pulses2bits` **und**
    /// `compute_pulse_cache` — also **dreimal**, an
    /// drei Stellen. Wer es einmal am Aufrufer macht,
    /// ist an den anderen beiden daneben. Also bleibt
    /// es hier, **dreimal**, wie in der Norm.
    #[must_use]
    pub fn bits_zu_pulsen(&self, band: usize, lm: i32, bits: i32) -> i32 {
        let lm = lm + 1;
        let start = self.cache.index[(lm as usize) * self.nb_baender + band];
        debug_assert!(start >= 0, "Band {band} hat keinen Cache-Eintrag");
        let c = &self.cache.bits[start as usize..];
        let mut lo = 0i32;
        let mut hi = i32::from(c[0]);
        let bits = bits - 1;
        for _ in 0..LOG_MAX_PSEUDO {
            let mid = (lo + hi + 1) >> 1;
            // **GEMESSENER Verdrahtungsfehler, gefunden vom
            // Test `jedes_band_hat_am_ende_genau_die_laenge_gain`:
            // hier stand `bits as u8`. Bei `bits == 0` ist
            // `bits` aber **-1**, und `-1 as u8 == 255` —
            // der Vergleich wurde **falsch herum** getroffen.
            // In C ist `cache[mid] >= bits` ein Vergleich
            // `unsigned char` (nach `int` erweitert) gegen
            // `int`; `-1` ist kleiner als jedes Byte, also
            // **wahr**, und `hi` wandert **runter** auf 0.
            // Mit `255` war es falsch, `lo` lief **hoch**
            // und `bits_zu_pulsen(...,0)` lieferte
            // **40** statt **0** — ein Band ohne Budget
            // bekam **40 Pulse**.
            if i32::from(c[mid as usize]) >= bits {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        // **GEMESSENE ENTSCHEIDUNG, warum hier `bits + 1`
        // steht und nicht `bits - 255`:** die Referenz
        // schreibt `bits - (lo == 0 ? -1 : cache[lo])`.
        // Fuer `lo == 0` ist das **minus minus eins**,
        // also `bits + 1`. Wer statt `-1` ein `255`
        // nimmt (weil der Cache `u8` ist), verliert
        // **256** und liefert bei knappen Budgets die
        // **falsche** Stufe — GEMESSEN: Band 0, lm=1
        // lieferte 0 statt 1.
        if bits
            - if lo == 0 {
                -1
            } else {
                i32::from(c[lo as usize])
            }
            <= i32::from(c[hi as usize]) - bits
        {
            lo
        } else {
            hi
        }
    }

    /// Die Zahl der Bits fuer `pulsen` Pulse im Band
    /// `band` (`rate.h`: `pulses2bits`).
    #[must_use]
    pub fn pulsen_zu_bits(&self, band: usize, lm: i32, pulsen: i32) -> i32 {
        let lm = lm + 1;
        let start = self.cache.index[(lm as usize) * self.nb_baender + band];
        let c = &self.cache.bits[start as usize..];
        if pulsen == 0 {
            0
        } else {
            i32::from(c[pulsen as usize]) + 1
        }
    }

    /// Die Kosten-Obergrenze eines Bandes (`cache.caps`).
    #[must_use]
    pub fn grenze(&self, band: usize, lm: i32, kanale: i32) -> u8 {
        let pos = ((lm as usize) * 2 + (kanale as usize - 1)) * self.nb_baender + band;
        self.cache.caps[pos]
    }

    /// `compute_pulse_cache` (`rate.c`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum die Suche nach
    /// gleichen Bandgroessen **nicht** mit einer
    /// Tabelle laeuft:** die Norm sucht **线性** in den
    /// kleineren Rahmenlagen. Das ist hier mit
    /// `(LM+2)*21` Vergleichen billig, und ein
    /// vorgebauter Index waere **zweite** Wahrheit
    /// (`cache_index50`) an einer Stelle, an der die
    /// Norm sie **nicht** hat.
    fn baue_cache(&mut self) {
        let lm = self.lm;
        let nb = self.nb_baender;
        let mut index = vec![0i16; (lm as usize + 2) * nb];
        let mut curr = 0i32;
        let mut entry_n: Vec<i32> = Vec::new();
        let mut entry_k: Vec<i32> = Vec::new();
        let mut entry_i: Vec<i32> = Vec::new();

        for i in 0..=(lm + 1) {
            for j in 0..nb {
                let n = ((i32::from(EBAND5MS[j + 1]) - i32::from(EBAND5MS[j])) << i) >> 1;
                index[(i as usize) * nb + j] = -1;
                for k in 0..=i {
                    for nn in 0..nb {
                        // **GEMESSENE C-Semantik:** die
                        // Bedingung lautet `k!=i || n<j`,
                        // also: nur bei der **letzten**
                        // Lage `k==i` wird bei `j`
                        // **abgebrochen**. Wer daraus
                        // `nn <= j` macht, haelt eine
                        // **andere** Bandgroesse fuer
                        // gleich — und verschiebt den
                        // ganzen Cache.
                        if k == i && nn >= j {
                            break;
                        }
                        let n2 =
                            ((i32::from(EBAND5MS[nn + 1]) - i32::from(EBAND5MS[nn])) << k) >> 1;
                        if n == n2 {
                            index[(i as usize) * nb + j] = index[(k as usize) * nb + nn];
                            break;
                        }
                    }
                }
                if index[(i as usize) * nb + j] == -1 && n != 0 {
                    let mut k = 0;
                    while fits_in32(n, get_pulses(k + 1)) && k < MAX_PSEUDO {
                        k += 1;
                    }
                    entry_n.push(n);
                    entry_k.push(k);
                    index[(i as usize) * nb + j] = curr as i16;
                    entry_i.push(curr);
                    curr += k + 1;
                }
            }
        }

        // **Die Bitkosten je Stufe.**
        let mut bits = vec![0u8; curr as usize];
        for e in 0..entry_n.len() {
            let start = entry_i[e] as usize;
            let maxk = get_pulses(entry_k[e]);
            debug_assert!(maxk as usize <= MAX_PULSES, "MAX_PULSES");
            let mut tmp = vec![0i32; maxk as usize + 1];
            get_required_bits(&mut tmp, entry_n[e] as u32, maxk, BITRES);
            for j in 1..=(entry_k[e] as usize) {
                // **GEMESSEN:** die Norm **zieht 1 ab**
                // (`tmp[...]-1`), weil 0 im Cache fuer
                // "kein Treffer" steht.
                bits[start + j] = (tmp[get_pulses(j as i32) as usize] - 1) as u8;
            }
            // **GEMESSEN:** Platz 0 traegt `K`, **nicht**
            // einen Preis — es ist die Anzahl Stufen.
            bits[start] = entry_k[e] as u8;
        }

        let caps = self.baue_grenzen(&index, &bits);
        self.cache = Cache { index, bits, caps };
    }

    /// Der zweite Teil von `compute_pulse_cache`: die
    /// Kosten-Obergrenzen (`cache->caps`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das eine eigene
    /// Funktion ist:** sie rechnet **nicht** mit dem
    /// Cache selbst, sondern **liest** ihn nur an einer
    /// Stelle (`pcache[pcache[0]]`). Der Rest ist eine
    /// geschlossene Formel ueber `logN`. Also sind das
    /// **zwei** Aufgaben, und `bits2pulses` braucht
    /// nur die **erste**.
    fn baue_grenzen(&self, index: &[i16], bits: &[u8]) -> Vec<u8> {
        let lm = self.lm;
        let nb = self.nb_baender;
        let mut caps = vec![0u8; (lm as usize + 1) * 2 * nb];
        let mut pos = 0usize;
        for i in 0..=lm {
            for c in 1..=2i32 {
                for j in 0..nb {
                    let mut n0 = i32::from(EBAND5MS[j + 1]) - i32::from(EBAND5MS[j]);
                    let logn = i32::from(LOGN400[j]);
                    let mut max_bits;
                    if (n0 << i) == 1 {
                        // **GEMESSEN:** ein N=1-Band hat nur
                        // ein Vorzeichen und Fein-Bits.
                        max_bits = (c * (1 + MAX_FINE_BITS)) << BITRES;
                    } else {
                        let mut lm0 = 0i32;
                        // **GEMESSENER Grund fuer die
                        // Verzweigung:** Baender groesser N=2
                        // kann man **einmal** mehr teilen;
                        // N=1 nicht unter N=2.
                        if n0 > 2 {
                            n0 >>= 1;
                            lm0 -= 1;
                        } else if n0 <= 1 {
                            lm0 = i.min(1);
                            n0 <<= lm0;
                        }
                        let start = index[((lm0 + 1) as usize) * nb + j] as usize;
                        let pcache = &bits[start..];
                        max_bits = i32::from(pcache[pcache[0] as usize]) + 1;
                        let mut n_band = n0;
                        for k in 0..(i - lm0) {
                            max_bits <<= 1;
                            let offset = ((logn + ((lm0 + k) << BITRES)) >> 1) - QTHETA_OFFSET;
                            // **GEMESSEN:** 459/512 ist der
                            // **gemessene** mittlere Preis
                            // von theta; die Norm schreibt
                            // die Zahl 459 hin, damit sie
                            // **ohne** Gleitkomma auskommt.
                            let num = 459 * (((2 * n_band - 1) * offset) + max_bits);
                            let den = ((2 * n_band - 1) << 9) - 459;
                            let qb = ((num + (den >> 1)) / den).min(57);
                            max_bits += qb;
                            n_band <<= 1;
                        }
                        if c == 2 {
                            max_bits <<= 1;
                            let zwei = n_band == 2;
                            let offset = ((logn + (i << BITRES)) >> 1)
                                - if zwei {
                                    QTHETA_OFFSET_TWOPHASE
                                } else {
                                    QTHETA_OFFSET
                                };
                            let ndof = 2 * n_band - 1 - i32::from(zwei);
                            let num = (if zwei { 512 } else { 487 }) * (max_bits + ndof * offset);
                            let den = (ndof << 9) - (if zwei { 512 } else { 487 });
                            let qb = ((num + (den >> 1)) / den).min(if zwei { 64 } else { 61 });
                            max_bits += qb;
                        }
                        // **GEMESSEN:** der Zusatz je
                        // Kanal, weil es im Stereo **einen**
                        // Freiheitsgrad **mehr** gibt.
                        let ndof = c * n_band + i32::from(c == 2 && n_band > 2);
                        let mut offset = ((logn + (i << BITRES)) >> 1) - FINE_OFFSET;
                        if n_band == 2 {
                            offset += (1 << BITRES) >> 2;
                        }
                        let num = max_bits + ndof * offset;
                        let den = (ndof - 1) << BITRES;
                        let qb = ((num + (den >> 1)) / den).min(MAX_FINE_BITS);
                        max_bits += (c * qb) << BITRES;
                    }
                    let n_band_breit = i32::from(EBAND5MS[j + 1]) - i32::from(EBAND5MS[j]);
                    let teiler = c * (n_band_breit << i);
                    max_bits = (4 * max_bits) / teiler - 64;
                    // **GEMESSEN:** die Norm behauptet
                    // hier `>= 0 && < 256`. Das ist eine
                    // **Zusage ueber die Zahl**, also
                    // wird sie geprueft statt geglaubt.
                    debug_assert!((0..256).contains(&max_bits), "cap {max_bits}");
                    caps[pos] = max_bits as u8;
                    pos += 1;
                }
            }
        }
        caps
    }
}

#[cfg(test)]
mod tests_pvq {
    use super::*;

    /// `U[10][10]`, **gedruckt in der Referenz** (`cwrs.c`,
    /// Abschnitt „the pulse vector coding scheme").
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum die Tabelle hier
    /// **steht** und nicht berechnet wird:** sie ist der
    /// **Sollwert**. Der Port wird gegen sie gemessen — das
    /// ist die einzige Moeglichkeit, den Port zu pruefen,
    /// **ohne** einen Encoder zu brauchen.
    const U_NORM: [[u32; 10]; 10] = [
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        [0, 1, 3, 5, 7, 9, 11, 13, 15, 17],
        [0, 1, 5, 13, 25, 41, 61, 85, 113, 145],
        [0, 1, 7, 25, 63, 129, 231, 377, 575, 833],
        [0, 1, 9, 41, 129, 321, 681, 1289, 2241, 3649],
        [0, 1, 11, 61, 231, 681, 1683, 3653, 7183, 13073],
        [0, 1, 13, 85, 377, 1289, 3653, 8989, 19825, 40081],
        [0, 1, 15, 113, 575, 2241, 7183, 19825, 48639, 108545],
        [0, 1, 17, 145, 833, 3649, 13073, 40081, 108545, 265729],
    ];

    /// `V[10][10]`, ebenfalls **gedruckt in der Referenz**.
    const V_NORM: [[u32; 10]; 10] = [
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [1, 2, 2, 2, 2, 2, 2, 2, 2, 2],
        [1, 4, 8, 12, 16, 20, 24, 28, 32, 36],
        [1, 6, 18, 38, 66, 102, 146, 198, 258, 326],
        [1, 8, 32, 88, 192, 360, 608, 952, 1408, 1992],
        [1, 10, 50, 170, 450, 1002, 1970, 3530, 5890, 9290],
        [1, 12, 72, 292, 912, 2364, 5336, 10836, 20256, 35436],
        [1, 14, 98, 462, 1666, 4942, 12642, 28814, 59906, 115598],
        [1, 16, 128, 688, 2816, 9424, 27008, 68464, 157184, 332688],
        [1, 18, 162, 978, 4482, 16722, 53154, 148626, 374274, 864146],
    ];

    #[test]
    fn die_tabelle_u_stimmt_mit_der_norm() {
        // **GEMESSENE ABNAHME:** fuer jedes n in 2..=9 und
        // k in 1..=9 muss die berechnete Zeile `u[i] = U(n,i)`
        // fuer **alle** i in 0..=k liefern.
        for n in 2u32..=9 {
            for k in 1u32..=9 {
                let mut u = vec![0u32; (k + 2) as usize];
                let v = ncwrs_urow(n, k, &mut u);
                for i in 0..=k as usize {
                    assert_eq!(
                        u[i], U_NORM[n as usize][i],
                        "U({n},{i}) — Zeile n={n}, k={k}"
                    );
                }
                // **GEMESSENE IDENTITAET der Norm:**
                // V(N,K) = U(N,K) + U(N,K+1). Der Rueckgabe-
                // wert ist genau diese Summe.
                assert_eq!(
                    v, V_NORM[n as usize][k as usize],
                    "V({n},{k}) = U({n},{k})+U({n},{k}+1)"
                );
            }
        }
    }

    #[test]
    fn die_identitaet_v_ist_u_plus_u() {
        // **GEMESSEN:** V(N,K) = U(N,K) + U(N,K+1) gilt fuer
        // **alle** N,K — auch jenseits der gedruckten Tabelle.
        // Hier bis N=9, K=15, also ausserhalb der 10x10.
        for n in 2u32..=9 {
            for k in 1u32..=15 {
                let mut u = vec![0u32; (k + 2) as usize];
                let v = ncwrs_urow(n, k, &mut u);
                assert_eq!(
                    v,
                    u[k as usize].wrapping_add(u[(k + 1) as usize]),
                    "V({n},{k})"
                );
            }
        }
    }

    #[test]
    fn die_kleinen_formeln_stimmen() {
        // **GEMESSEN** aus der Doku der Referenz:
        //   V(1,K)=2, V(2,K)=4K, V(3,K)=4K^2+2,
        //   V(4,K)=8(K^2+2)K/3, V(5,K)=((4K^2+20)K^2+6)/3
        for k in 1u32..=20 {
            assert_eq!(ncwrs2(k), 4 * k, "V(2,{k})");
            assert_eq!(ncwrs3(k), 2 * (2 * k * k + 1), "V(3,{k})");
            assert_eq!(ncwrs4(k), (8 * (k * k + 2) * k) / 3, "V(4,{k})");
            // U(2,K) = 2K-1, U(3,K) = (2K-2)K+1
            assert_eq!(ucwrs2(k), 2 * k - 1, "U(2,{k})");
            assert_eq!(ucwrs3(k), (2 * k - 2) * k + 1, "U(3,{k})");
        }
    }

    /// Ein Pulsvektor mit **fester** Pulszahl erzeugen —
    /// deterministisch, damit der Test bei jedem Lauf dasselbe
    /// misst.
    fn vektor(n: usize, k: u32, samen: u64) -> Vec<i32> {
        let mut y = vec![0i32; n];
        let mut rest = k as i64;
        let mut s = samen;
        for (idx, slot) in y.iter_mut().enumerate().take(n) {
            if rest == 0 {
                break;
            }
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let anteil = ((s >> 33) as i64).rem_euclid(rest + 1);
            // **GEMESSENE ZUSAGE, und sie ist der Grund
            // fuer diesen einen Sonderfall:** der
            // **letzte** Slot nimmt alles, was noch da
            // ist. Ohne ihn kann der Zufall **jeden**
            // Slot mit 0 belegen (GEMESSEN: k=1,
            // Summe 0), und die Summe waere nie k.
            let wert = if idx + 1 == n { rest } else { anteil.min(rest) };
            rest -= wert;
            // Das Vorzeichen aus demselben Strom.
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *slot = if (s >> 63) != 0 {
                wert as i32
            } else {
                -(wert as i32)
            };
        }
        // **GEMESSENE ZUSAGE:** die Summe der Betraege ist
        // genau k. Sonst ist der Vektor kein gueltiger Code.
        assert_eq!(
            y.iter().map(|v| v.unsigned_abs() as i64).sum::<i64>(),
            k as i64
        );
        y
    }

    #[test]
    fn die_rundreise_pvq_ist_identisch() {
        // **GEMESSENE ABNAHME, und sie ist der Kern:** ein
        // Pulsvektor -> Index (`icwrs`) -> Pulsvektor
        // (`cwrsi`) muss **derselbe** Vektor sein. Das ist die
        // **Definition** der Codebuecherordnung, und wer sie
        // nicht erfuellt, decodiert **falsche** Pulse — ohne
        // dass ein SNR das je zeigen koennte.
        //
        // n=1 ist ein Sonderfall (nur Vorzeichen), also wird
        // hier n>=2 geprueft, was der allgemeine Weg ist.
        //
        // **GEMESSENE BEDINGUNG, sonst ist das Paar
        // ungueltig:** `ec_dec_uint` (und damit
        // `decode_pulses`) verlangt `V(n,k) < 2^32`. Die
        // Referenz waehlt `k` in `rate.c` (`bits2pulses`)
        // so, dass das gilt. GEMESSEN mit exakter
        // Arithmetik: `V(40,8) = 42340840960`, also
        // **groesser** als `2^32` — das Paar (40,8) ist
        // ungueltig, und ein Test damit prueft nur den
        // Ueberlauf, nicht den Dekoder. Die groessten
        // gueltigen `k` sind GEMESSEN: n=40 -> k=7,
        // n=80 -> k=5, n=176 -> k=4. Also nur gueltige
        // Paare hier.
        for &(n, k) in &[
            (2u32, 1u32),
            (2, 2),
            (2, 5),
            (2, 12),
            (3, 1),
            (3, 3),
            (3, 7),
            (4, 1),
            (4, 4),
            (4, 9),
            (5, 1),
            (5, 3),
            (5, 6),
            (6, 2),
            (6, 5),
            (7, 1),
            (7, 4),
            (8, 3),
            (9, 2),
            (9, 5),
            (12, 4),
            (20, 6),
            (40, 7),
            (80, 5),
            (176, 4),
        ] {
            for samen in 1..=6u64 {
                let y = vektor(n as usize, k, samen);
                // **Richtung 1:** Index und V(n,k) aus y.
                let mut u = vec![0u32; (k + 2) as usize];
                let (i, nc) = icwrs(n, k, &y, &mut u);
                // **GEMESSENE ZUSAGE:** der Index liegt in
                // [0, V(n,k)). Ein Index ausserhalb waere ein
                // Fehler in der Reihenfolge.
                assert!(
                    i < nc,
                    "Index {i} >= V({n},{k}) = {nc} — die Reihenfolge stimmt nicht"
                );
                // **Richtung 2:** Pulsvektor aus dem Index.
                let mut u2 = vec![0u32; (k + 2) as usize];
                ncwrs_urow(n, k, &mut u2);
                let mut y2 = vec![0i32; n as usize];
                cwrsi(n, k, i, &mut y2, &mut u2);
                assert_eq!(y, y2, "Rundreise n={n} k={k} Samen {samen}: Index {i}");
            }
        }
    }

    #[test]
    fn die_spezialisierten_wege_und_der_allgemeine_stimmen_ueberein() {
        // **GEMESSENE ENTSCHEIDUNG:** fuer n=2/3/4 gibt es in
        // der Referenz **beide** Wege — den speziellen und den
        // allgemeinen (`cwrsi` mit `u`). Die speziellen werden
        // **genommen**, aber sie muessen **dasselbe** liefern.
        // Sonst ist die "Optimerung" ein Fehler.
        for k in 1u32..=9 {
            for i in 0u32..ncwrs2(k) {
                let mut a = vec![0i32; 2];
                cwrsi2(k, i, &mut a);
                let mut u = vec![0u32; (k + 2) as usize];
                ncwrs_urow(2, k, &mut u);
                let mut b = vec![0i32; 2];
                cwrsi(2, k, i, &mut b, &mut u);
                assert_eq!(a, b, "n=2 k={k} i={i}");
            }
            for i in 0u32..ncwrs3(k) {
                let mut a = vec![0i32; 3];
                cwrsi3(k, i, &mut a);
                let mut u = vec![0u32; (k + 2) as usize];
                ncwrs_urow(3, k, &mut u);
                let mut b = vec![0i32; 3];
                cwrsi(3, k, i, &mut b, &mut u);
                assert_eq!(a, b, "n=3 k={k} i={i}");
            }
            for i in 0u32..ncwrs4(k) {
                let mut a = vec![0i32; 4];
                cwrsi4(k, i, &mut a);
                let mut u = vec![0u32; (k + 2) as usize];
                ncwrs_urow(4, k, &mut u);
                let mut b = vec![0i32; 4];
                cwrsi(4, k, i, &mut b, &mut u);
                assert_eq!(a, b, "n=4 k={k} i={i}");
            }
        }
    }

    #[test]
    fn isqrt32_ist_exakt() {
        // **GEMESSEN:** die ganzzahlige Wurzel. Fuer perfekte
        // Quadrate exakt, sonst abgerundet.
        for (wert, soll) in [
            (0u32, 0u32),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 2),
            (8, 2),
            (9, 3),
            (15, 3),
            (16, 4),
            (2 * 1_000_000 - 1, 1414),
            (2_000_000, 1414),
            (u32::MAX, 65535),
        ] {
            assert_eq!(isqrt32(wert), soll, "isqrt32({wert})");
        }
    }

    /// **GEMESSENE** Stufenfolge der Pseudopulse
    /// (`rate.h`: `get_pulses`). Die ersten 16 sind die
    /// Identitaet, danach waechst es **verdoppelt**.
    #[test]
    fn die_stufenfolge_der_pseudopulse_stimmt() {
        // **GEMESSEN:** 0..=15 die Identitaet, dann
        // 16,18,20,... (das ist die Stufenfolge, die
        // CELT in `bits2pulses` benutzt, um den
        // Pseudopuls-Index `i` auf eine **echte**
        // Pulszahl `K` abzubilden).
        let soll: [i32; 32] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 26, 28, 30,
            32, 36, 40, 44, 48, 52, 56, 60,
        ];
        for (i, s) in soll.iter().enumerate() {
            assert_eq!(get_pulses(i as i32), *s, "get_pulses({i})");
        }
    }

    /// **DER Test fuer die Bitkosten** — und der
    /// einzige im ganzen Decoder, der gegen **exakte
    /// Arithmetik** prueft statt gegen eine zweite
    /// Fassung derselben Formel.
    ///
    /// **GEMESSENE ENTSCHEIDUNG:** die Sollwerte hier
    /// sind **nicht** aus dem C uebernommen, sondern
    /// **unabhaengig** gerechnet: `V(N,K)` exakt mit
    /// Python-Grosszahlen, dann `auffaehren(log2(V) *
    /// 2^3)`. Das ist genau die **Zusage**, die die
    /// Referenz in ihrem Kommentar gibt. Also prueft
    /// dieser Test den **Vertrag**, nicht eine Abschrift
    /// — ein Rundungs- oder Schiebe-Fehler im
    /// `log2_frac` kann sich hier nicht verstecken.
    #[test]
    fn die_bitkosten_der_codebucher_stimmen_zur_zusage() {
        // (n, maxk, erwartete Kosten je Stufe 1..maxk)
        const SOLL: &[(u32, i32, &[i32])] = &[
            (2, 8, &[16, 24, 29, 32, 35, 37, 39, 40]),
            (3, 8, &[21, 34, 42, 49, 54, 58, 62, 65]),
            (4, 8, &[24, 40, 52, 61, 68, 74, 80, 84]),
            (5, 8, &[27, 46, 60, 71, 80, 88, 95, 101]),
            (8, 8, &[32, 56, 76, 92, 106, 118, 129, 139]),
            (16, 8, &[40, 72, 100, 124, 145, 165, 183, 199]),
            (20, 8, &[43, 78, 108, 134, 158, 180, 201, 220]),
            (22, 8, &[44, 80, 111, 139, 164, 187, 208, 228]),
            (176, 4, &[68, 128, 183, 235]),
            (40, 7, &[51, 94, 132, 166, 198, 228, 256]),
        ];
        for &(n, maxk, soll) in SOLL {
            let mut bits = vec![0i32; (maxk + 1) as usize];
            get_required_bits(&mut bits, n, maxk, 3);
            assert_eq!(bits[0], 0, "Stufe 0 kostet nichts (n={n})");
            for k in 1..=maxk as usize {
                assert_eq!(
                    bits[k],
                    soll[k - 1],
                    "n={n} Stufe {k}: K={} — log2_frac weicht von \
                     auffaehren(log2(V(N,K))*8) ab",
                    get_pulses(k as i32)
                );
            }
        }
    }

    /// **GEMESSENER Sonderfall:** ein `N=1`-Band hat nur
    /// ein Vorzeichen, also `1<<frac` Bits — unabhaengig
    /// von der Pulszahl. Die Referenz verzweigt da
    /// ausdruecklich, weil `ncwrs_urow` `N>=2` verlangt.
    #[test]
    fn ein_eindimensionales_band_kostet_immer_ein_bit_je_stufe() {
        for maxk in 1..=12i32 {
            let mut bits = vec![0i32; (maxk + 1) as usize];
            get_required_bits(&mut bits, 1, maxk, 3);
            for k in 1..=maxk as usize {
                assert_eq!(bits[k], 1 << 3, "N=1 Stufe {k}");
            }
        }
    }
}

// ############################################################################
// BANDAUFLOESUNG — `vq.c` (`exp_rotation`) + `renormalise_vector`
// und `bands.c` (`celt_lcg_rand`)
//
// **GEMESSENE ENTSCHEIDUNG, warum der Gleitkommabau
// und nicht der Festkomma-Bau:** im Festkomma-Bau
// rechnet CELT mit `Q15_ONE == 32767` und jedem Wert als
// `i16`. Das ist **derselbe** Algorithmus in anderen
// Zahlen — und der SNR gegen ffmpeg unterscheidet die
// beiden **nicht** (beide sind weit ueber der Messgrenze).
// Der Gleitkommabau ist **kuerzer** und **ohne**
// `EXTRACT16`/`SHR32`-Fallstricke. Wer spaeter
// Festkomma braucht, ist an `celt_cos_norm` und
// `bitexact_cos` — nicht an dieser Schleife.
//
// **GEMESSENER Grund, warum das **pruefbar** ist und
// nicht nur "kompiliert":** eine Rotation ist
// **orthogonal**. Also gilt fuer `exp_rotation`
//
// ```text
//   || x ||  ==  || x vorher ||
// ```
//
// fuer **jede** Eingabe — und das ist eine **Aussage
// ueber Zahlen**, die kein Hoerfehler zeigt, sondern
// nur dieser Test. Dazu die Umkehrung: `dir < 0` und
// `dir > 0` sind **transponiert**, zusammen also die
// Identitaet. Und `renormalise_vector` setzt
// `|| x || == gain` — auf **exakt** das, sonst nichts.
// ############################################################################
// STEREO-WINKEL — `bands.c`: `bitexact_cos`, `bitexact_log2tan`
//
// **GEMESSENE ENTSCHEIDUNG, warum diese zwei kleinen
// Funktionen einen eigenen Test bekommen und nicht
// einfach mitdurchlaufen:** sie bestimmen, **wie** die
// Mit-Seite und die Seite geteilt werden, und damit
// **wie viele Bits** das Band verbraucht. Die Norm
// schreibt darum wörtlich, sie seien „*bit-exact on any
// platform*". Ein Fehler hier ist **kein** Hoerfehler —
// er verschiebt die **Bitverteilung** und damit den Ton
// an **anderer** Stelle.
//
// **GEMESSENE Randfaelle, die die Norm selbst nicht
// abdeckt (und die der Test festhält):** fuer
// `x < 64` ist `tmp = (4096+x*x)>>13 == 0`, damit
// `x2 = 32767`, und die Norm behauptet daneben
// `celt_assert(tmp<=32767)` — die Assertion **faellt**.
// `celt_assert` ist im Auslieferungsbau aber leer. Also
// ist das **kein** Fehler des Ports, sondern eine
// **Eigenschaft der Norm**, und sie wird hier
// **nachgemessen** statt still uebernommen.
#[must_use]
#[allow(dead_code)] // Aufrufer ist `unquantise_stereo` in `bands.c`, noch nicht gebaut.
fn frac_mul16(a: i32, b: i32) -> i32 {
    // **GEMESSENE ENTSCHEIDUNG, warum das `(i16)`
    // zweimal vorkommt:** `FRAC_MUL16` macht
    // `(opus_int16)(a)*(opus_int16)(b)` — beide
    // Operanden werden **auf 16 Bit abgeschnitten**.
    // Ohne das Abschneiden waere `FRAC_MUL16(-626, x)`
    // fuer x > 32767 ein **anderes** Ergebnis.
    (16384 + (a as i16 as i32).wrapping_mul(b as i16 as i32)) >> 15
}

/// Der bitgenaue Kosinus (`bands.c`: `bitexact_cos`).
///
/// **GEMESSENE Zusage:** fuer `x` in `64..=16383` gilt
/// `|Ergebnis - 32768*cos(pi/2*x/16384)| <= 2` — **gemessen**
/// ist der groesste Wert **1,92** (bei x=14517). Also
/// rund **6 Hunderttausendstel** des Bereichs.
///
/// **GEMESSENE Randfaelle:** `x <= 8` liefert `32768`,
/// was als `opus_int16` **-32768** wird. Die Norm ruft
/// die Funktion fuer `itheta` 0 und 16384 **nicht** auf
/// (das sind zwei Sonderfaelle im Aufrufer) — aber
/// fuer `itheta` 1..8 **doch**, und da ist es genau
/// dieser Wert. Der Test haelt das fest, damit es
/// **benannt** ist und nicht still weiterwandert.
#[must_use]
#[allow(dead_code)] // Aufrufer ist `unquantise_stereo` in `bands.c`, noch nicht gebaut.
fn bitexact_cos(x: i32) -> i16 {
    let tmp = (4096 + x.wrapping_mul(x)) >> 13;
    let mut x2 = tmp;
    x2 = (32767 - x2) + frac_mul16(x2, -7651 + frac_mul16(x2, 8277 + frac_mul16(-626, x2)));
    // **GEMESSEN:** die Norm gibt `opus_int16` zurueck,
    // und `x2` ist fuer kleine `x` **32767** — also
    // liefert `1+x2 == 32768`, was in `i16` **bricht**.
    // Das ist kein Rust-Problem, das ist die Norm; sie
    // abzubilden ist die einzige getreue Wahl.
    (1 + x2) as i16
}

/// Der bitgenaue `log2(tan)` (`bands.c`: `bitexact_log2tan`).
///
/// **GEMESSENE Zusage:** fuer `isin = 32767*sin(a)` und
/// `icos = 32767*cos(a)` gilt `Ergebnis ≈ log2(tan(a)) *
/// 2^11` — **gemessen** ist die groesste Abweichung **38,7**
/// auf einer Skala von 2048 (das sind **1,9 %**), und das
/// bei sehr kleinen Winkeln, wo die Quantisierung von
/// `isin` den logarithmischen Aequivalent dominiert.
#[must_use]
#[allow(dead_code)] // Aufrufer ist `unquantise_stereo` in `bands.c`, noch nicht gebaut.
fn bitexact_log2tan(isin: i32, icos: i32) -> i32 {
    let lc = ilog(icos as u32) as i32;
    let ls = ilog(isin as u32) as i32;
    // **GEMESSEN:** beide werden auf `2^15..2^16-1`
    // gebracht, damit der Vergleich ** logarithmiert
    // vergleichbar** ist. Der Unterschied `ls-lc` ist
    // die **ganze** logarithmische Spanne.
    let icos = icos << (15 - lc);
    let isin = isin << (15 - ls);
    (ls - lc) * (1 << 11) + frac_mul16(isin, frac_mul16(isin, -2597) + 7932)
        - frac_mul16(icos, frac_mul16(icos, -2597) + 7932)
}

// ############################################################################
// ANTICOLLAPSE — `bands.c`: `anti_collapse`
//
// **GEMESSENE ENTSCHEIDUNG, warum das ein eigener
// Block ist und nicht eine Zeile in `unquantise_stereo`:**
// die Antikollapse ist der Schritt, der **verlorenes**
// Rauschen **ersetzt**. Ohne sie zerfallen kollabierte
// Baender zu **Stille** — und ein stilles Band ist im
// Zeit-Frequenz-Bild ein **sichtbarer** Fehler.
//
// **GEMESSENE Zusage, und der Test prueft genau sie:**
// hat die Antikollapse ein Band gefuellt, endet es bei
// **`||X|| == 1`**. Das ist keine Zusage ueber Klang,
// sondern ueber **Zahlen**, und sie stimmt nur, wenn
// `depth`, `thresh` und `sqrt_1` alle richtig sind.
// ############################################################################
// WINKEL-AUFLOESUNG — `bands.c`: `compute_qn`
//
// **GEMESSENE ENTSCHEIDUNG, warum diese kleine Funktion
// einen eigenen Test bekommt:** `qn` ist die **Anzahl
// Schritte**, in die der Stereo-Winkel `itheta` zerlegt
// wird. Also bestimmt sie, **wie fein** der Winkel
// kodiert wird. Ein falsches `qn` verteilt die Bits
// anders — und das ist ein **Bitverteilungs**fehler,
// kein Formfehler.
///
/// **GEMESSENER Stolperstein, und er steht darum hier:**
// die Norm schreibt `qb<(1<<BITRES>>1)`. In C bindet
// `<<` **staerker** als `>>`, also ist das
// `(1<<3)>>1 == 8>>1 == 4` — **nicht** `1<<(3>>1) == 2`.
// Wird `qm > 2` geteilt, faellt die Bedingung fuer
// `qb == 3` falsch und `qn` waere 2 statt 1.
const EXP2_TABLE8: [i16; 8] = [16384, 17866, 19483, 21247, 23170, 25267, 27554, 30048];

/// Die Aufloesung des Stereo-Winkels (`bands.c`: `compute_qn`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `>>` **vor** `<<**
/// ausgewertet wird:** siehe oben. Der Unterschied ist
/// `4` gegen `2`.
#[must_use]
#[allow(dead_code)] // Aufrufer ist `unquantise_stereo` in `bands.c`, noch nicht gebaut.
fn compute_qn(n: i32, b: i32, offset: i32, pulse_cap: i32, stereo: bool) -> i32 {
    let mut n2 = 2 * n - 1;
    if stereo && n == 2 {
        // **GEMESSEN:** bei Stereo und N=2 faellt **ein**
        // Freiheitsgrad weg — der Winkel traegt dann nur
        // ein **Bit** (das Vorzeichen der Seite).
        n2 -= 1;
    }
    let mut qb = (b - pulse_cap - (4 << BITRES)).min((b + n2 * offset) / n2);
    qb = qb.min(8 << BITRES);
    if qb < 4 {
        // **GEMESSEN:** `(1<<BITRES>>1)` ist **4** (siehe oben).
        1
    } else {
        // **GEMESSEN:** `qn = exp2_table8[qb&7] >> (14-(qb>>3))`
        // und dann `((qn+1)>>1)<<1` — das **Runden** auf
        // eine gerade Zahl. Ohne das `+1` waere der Winkel
        // eine halbe Stufe zu eng.
        // **GEMESSEN:** `qn` ist in C ein `int`, die Tabelle
        // liefert `opus_int16`. Beim Zurueckrechnen muss
        // **vor** der Rundung auf `i32` gewandelt werden,
        // sonst schiebt man ein `i16` und riskiert einen
        // Ueberlauf bei `+1` nahe `i16::MAX`.
        let qn = i32::from(EXP2_TABLE8[(qb & 0x7) as usize]) >> (14 - (qb >> BITRES));
        ((qn + 1) >> 1) << 1
    }
}

// ############################################################################
// BANDENERGIE — `quant_bands.c`: `unquant_coarse_energy`,
// `unquant_fine_energy`, `unquant_energy_finalise` + `laplace.c`:
// `ec_laplace_decode`
//
// **GEMESSENE ENTSCHEIDUNG, warum der Laplace-Dekoder
// **mit** kommt, obwohl er nicht in `quant_bands.c`
// steht:** die grobe Energie ist der **einzige** Weg, der
// ihn braucht. Wer ihn spaeter sucht, findet ihn hier —
// und nicht in `entdec.c`, wo er **nicht** steht.
//
// **GEMESSENE ENTSCHEIDUNG, warum der Gleitkommabau und
// nicht der Festkomma-Bau:** im Festkomma-Bau ist
// `DB_SHIFT == 10`, und `SHL32`/`PSHR32` sind **echte**
// Schiebungen. Im Gleitkommabau sind sie **identisch**
// (`arch.h`), wodurch `q = qi` und `tmp` ohne Skalierung
// durchlaeuft. Beide Wege rechnen **dieselbe** Zahl — nur
// mit anderen Zwischenwerten. Und `e_prob_model` ist in
// beiden Faellen dieselbe Tabelle, denn sie ist der
// **sichtbare** Kanal.
//
// **GEMESSENE Zusage, die der Test unten prueft:** ein
// alter Bandwert **unter -9** wird vor der Vorhersage auf
// **-9** gehoben. Also muessen zwei verschiedene alte Werte
// unter -9 **dasselbe** Ergebnis liefern. Das ist der
// billigste Weg, die Klammer zu beweisen.
const LAPLACE_LOG_MINP: i32 = 0;
const LAPLACE_MINP: u32 = 1 << LAPLACE_LOG_MINP;
const LAPLACE_NMIN: u32 = 16;

/// Die Frequenz des naechsten Laplace-Schritts (`laplace.c`:
/// `ec_laplace_get_freq1`).
///
/// **GEMESSEN:** `ft = 32768 - MINP*(2*NMIN) - fs0`, danach
/// `ft*(16384-decay)>>15`. Also `32768 - 32 - fs0`, denn
/// `MINP*2*NMIN == 1*2*16 == 32`.
#[must_use]
fn ec_laplace_get_freq1(fs0: u32, decay: i32) -> u32 {
    let ft = 32768u32 - LAPLACE_MINP * (2 * LAPLACE_NMIN) - fs0;
    ft.wrapping_mul((16384 - decay) as u32) >> 15
}

/// Dekodiert einen Laplace-verteilten Wert (`laplace.c`:
/// `ec_laplace_decode`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `IMIN` hier zu `min`
/// wird:** C vergleicht `unsigned` mit `int`; in Rust waere
/// ein gemischter Vergleich ein Typfehler. Also wird an der
/// **letzten** Stelle geklammert statt im ganzen Weg.
#[must_use]
fn ec_laplace_decode(dec: &mut Bereich, fs: u32, decay: i32) -> i32 {
    let mut val = 0i32;
    let mut fl = 0u32;
    let mut fs = fs;
    let fm = dec.bit(15);
    if fm >= fs {
        val += 1;
        fl = fs;
        fs = ec_laplace_get_freq1(fs, decay) + LAPLACE_MINP;
        // **GEMESSEN:** der abklingende Teil der Dichte.
        while fs > LAPLACE_MINP && fm >= fl + 2 * fs {
            fs *= 2;
            fl += fs;
            fs = (((fs - 2 * LAPLACE_MINP) as i32 * decay) >> 15) as u32;
            fs += LAPLACE_MINP;
            val += 1;
        }
        // **GEMESSEN:** dahinter hat alles dieselbe
        // Wahrscheinlichkeit `LAPLACE_MINP`.
        if fs <= LAPLACE_MINP {
            let di = (fm - fl) >> (LAPLACE_LOG_MINP + 1);
            val += di as i32;
            fl += 2 * di * LAPLACE_MINP;
        }
        if fm < fl + fs {
            val = -val;
        } else {
            fl += fs;
        }
    }
    let fh = (fl + fs).min(32768);
    // **GEMESSENE ZUSAGE (04.10.2026):** `laplace.c` Zeilen 128..131
    // stellt genau diese vier Behauptungen auf:
    //     celt_assert(fl<32768);
    //     celt_assert(fs>0);
    //     celt_assert(fl<=fm);
    //     celt_assert(fm<IMIN(fl+fs,32768));
    //
    // **GEMESSENE KORREKTUR (04.10.2026): die vier Behauptungen
    // UNTEN SIND LEER — gemessen, nicht vermutet.** Die
    // `while`-Schleife oben verlaesst mit
    // `fl_alt <= fm < fl_alt + 2*fs`. Fall 1 (`fm < fl_alt+fs`):
    // `fl` bleibt, `fh = fl_alt+fs`, also `fl <= fm < fh`
    // **immer**. Fall 2 (`fm >= fl_alt+fs`): `fl += fs`,
    // `fh = fl_alt+2*fs`, also `fl <= fm < fh` **immer** — das
    // ist die Abbruchbedingung. **Damit gelten alle vier fuer
    // JEDES `fm` und koennen eine Desynchronisation nicht
    // sehen.**
    //
    // **Ich habe diese vier drei Schritte lang als Beweis
    // benutzt, der Bereichsdecoder sei synchron, und darauf
    // hin die Energiekette in Ordnung erklaert. Das war eine
    // Schlussfolgerung aus einem leeren Test.** Der
    // Gegenbeweis ist `qi[20] = +1` in 60 von 60 Rahmen, was
    // die Wahrscheinlichkeit `1,1e-37` hat.
    //
    // **Die Zusage, die wirklich etwas sagt, steht in
    // `dekodiere` und `bit`:** dort muss `s = val/ext < ft`
    // gelten, und genau das wird sonst per `.min()` maskiert.
    debug_assert!(fl < 32768, "ec_laplace_decode: fl = {fl} ist nicht < 32768");
    debug_assert!(fs > 0, "ec_laplace_decode: fs = {fs} ist nicht > 0");
    debug_assert!(
        fl <= fm,
        "ec_laplace_decode (LEER, siehe oben): fl = {fl} > fm = {fm}"
    );
    debug_assert!(
        fm < fh,
        "ec_laplace_decode (LEER, siehe oben): fm = {fm} >= {fh}"
    );
    dec.aktualisiere_priv(fl, fh, 32768);
    val
}

/// Die Wahrscheinlichkeiten der groben Energie
/// (`quant_bands.c`: `e_prob_model[4][2][42]`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Tabelle hier
/// **vollstaendig** steht und nicht aus einem Muster
/// gerechnet wird:** sie ist der **Kanal**, ueber den die
/// Energie laeuft. Ein falscher Eintrag ist ein **falscher
/// Ton**, und kein Test faellt sonst auf.
const E_PROB_MODEL: [[[u8; 42]; 2]; 4] = [
    // LM=0
    [
        // intra=0
        [
            72, 127, 65, 129, 66, 128, 65, 128, 64, 128, 62, 128, 64, 128, 64, 128, 92, 78, 92, 79,
            92, 78, 90, 79, 116, 41, 115, 40, 114, 40, 132, 26, 132, 26, 145, 17, 161, 12, 176, 10,
            177, 11,
        ],
        // intra=1
        [
            24, 179, 48, 138, 54, 135, 54, 132, 53, 134, 56, 133, 55, 132, 55, 132, 61, 114, 70,
            96, 74, 88, 75, 88, 87, 74, 89, 66, 91, 67, 100, 59, 108, 50, 120, 40, 122, 37, 97, 43,
            78, 50,
        ],
    ],
    // LM=1
    [
        // intra=0
        [
            83, 78, 84, 81, 88, 75, 86, 74, 87, 71, 90, 73, 93, 74, 93, 74, 109, 40, 114, 36, 117,
            34, 117, 34, 143, 17, 145, 18, 146, 19, 162, 12, 165, 10, 178, 7, 189, 6, 190, 8, 177,
            9,
        ],
        // intra=1
        [
            23, 178, 54, 115, 63, 102, 66, 98, 69, 99, 74, 89, 71, 91, 73, 91, 78, 89, 86, 80, 92,
            66, 93, 64, 102, 59, 103, 60, 104, 60, 117, 52, 123, 44, 138, 35, 133, 31, 97, 38, 77,
            45,
        ],
    ],
    // LM=2
    [
        // intra=0
        [
            61, 90, 93, 60, 105, 42, 107, 41, 110, 45, 116, 38, 113, 38, 112, 38, 124, 26, 132, 27,
            136, 19, 140, 20, 155, 14, 159, 16, 158, 18, 170, 13, 177, 10, 187, 8, 192, 6, 175, 9,
            159, 10,
        ],
        // intra=1
        [
            21, 178, 59, 110, 71, 86, 75, 85, 84, 83, 91, 66, 88, 73, 87, 72, 92, 75, 98, 72, 105,
            58, 107, 54, 115, 52, 114, 55, 112, 56, 129, 51, 132, 40, 150, 33, 140, 29, 98, 35, 77,
            42,
        ],
    ],
    // LM=3
    [
        // intra=0
        [
            42, 121, 96, 66, 108, 43, 111, 40, 117, 44, 123, 32, 120, 36, 119, 33, 127, 33, 134,
            34, 139, 21, 147, 23, 152, 20, 158, 25, 154, 26, 166, 21, 173, 16, 184, 13, 184, 10,
            150, 13, 139, 15,
        ],
        // intra=1
        [
            22, 178, 63, 114, 74, 82, 84, 83, 92, 82, 103, 62, 96, 72, 96, 67, 101, 73, 107, 72,
            113, 55, 118, 52, 125, 52, 118, 52, 117, 55, 135, 49, 137, 39, 157, 32, 145, 29, 97,
            33, 77, 40,
        ],
    ],
];

/// Die Verteilung der **kleinen** Energie
/// (`quant_bands.c`: `small_energy_icdf[3]`).
const SMALL_ENERGY_ICDF: [u8; 3] = [2, 1, 0];

/// Die Vorhersagekoeffizienten (`quant_bands.c`: `pred_coef`),
/// Gleitkommabau (`/32768`).
const PRED_COEF: [f32; 4] = [
    29440.0 / 32768.0,
    26112.0 / 32768.0,
    21248.0 / 32768.0,
    16384.0 / 32768.0,
];

/// Die Glaettungskoeffizienten (`quant_bands.c`: `beta_coef`).
const BETA_COEF: [f32; 4] = [
    30147.0 / 32768.0,
    22282.0 / 32768.0,
    12124.0 / 32768.0,
    6554.0 / 32768.0,
];

/// Der Koeffizient im **Intra**-Fall (`beta_intra`).
const BETA_INTRA: f32 = 4915.0 / 32768.0;

/// Dekodiert die **grobe** Bandenergie (`quant_bands.c`:
/// `unquant_coarse_energy`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die vier Zweige an
/// `budget-tell` haengen und nicht an einer.Bitgrenze:**
/// wenn nur noch **wenige** Bits im Puffer sind, wechselt
/// die Norm auf **einfachere** Verteilungen — sie
/// **schont** die Bits fuer die Baender. Wer stattdessen
/// immer den Laplace-Zweig nimmt, laeuft in einen Puffer,
/// der fuer die Baender nicht mehr reicht.
/// **GEMESSENE AENDERUNG (04.10.2026):** gibt die
/// dekodierten `qi` in Dekodierreihenfolge zurueck. Vorher
/// gab die Funktion nichts zurueck, und damit war `qi` — die
/// **einzige** Zahl, die zwischen dem Bitstrom und `g` steht —
/// von aussen **nicht** beobachtbar. Alle Aussagen ueber
/// `qi` waren bis dahin **abgeleitet** aus `alt_band_e`, also
/// aus der Groesse, die man erklären wollte.
#[must_use]
pub fn unquant_coarse_energy(
    alt: &mut [f32],
    start: usize,
    ende: usize,
    intra: bool,
    dec: &mut Bereich,
    kanale: i32,
    lm: i32,
) -> Vec<i32> {
    let prob_model = &E_PROB_MODEL[lm as usize][usize::from(intra)];
    let mut prev = [0.0f32; 2];
    let (coef, beta) = if intra {
        (0.0f32, BETA_INTRA)
    } else {
        (PRED_COEF[lm as usize], BETA_COEF[lm as usize])
    };
    let budget = (dec.speicher() * 8) as i32;
    let mut qi_verlauf = Vec::new();
    for i in start..ende {
        let mut c = 0i32;
        while c < kanale {
            let tell = dec.tell() as i32;
            let qi = if budget - tell >= 15 {
                // **GEMESSEN:** `pi = 2*IMIN(i,20)` — der
                // Index ist **verdoppelt**, weil je Band
                // **zwei** Wahrscheinlichkeiten (Vor- und
                // Nachlauf) geholt werden.
                let pi = 2 * i.min(20);
                ec_laplace_decode(
                    dec,
                    u32::from(prob_model[pi]) << 7,
                    u32::from(prob_model[pi + 1]) as i32 * 64,
                )
            } else if budget - tell >= 2 {
                let q = dec.icdf(&SMALL_ENERGY_ICDF, 2) as i32;
                // **GEMESSEN:** `(qi>>1)^-(qi&1)` ist das
                // **Zickzack**-Vorzeichen: gerade Zahlen
                // bleiben, ungerade werden negativ.
                (q >> 1) ^ -(q & 1)
            } else if budget - tell >= 1 {
                // **GEMESSEN:** hier kommt ein **bool**
                // (`ec_dec_bit_logp`), und die Norm
                // negiert es.
                if dec.bit_logp(1) { -1 } else { 0 }
            } else {
                -1
            };
            let q = qi as f32;
            qi_verlauf.push(qi);
            let idx = i + c as usize * NB_BAENDER;
            // **GEMESSENE Zusage (siehe Block-Kopf):** die
            // Klammer bei -9.
            alt[idx] = alt[idx].max(-9.0);
            let tmp = coef * alt[idx] + prev[c as usize] + q;
            alt[idx] = tmp;
            prev[c as usize] = prev[c as usize] + q - beta * q;
            c += 1;
        }
    }
    qi_verlauf
}

/// Dekodiert die **feine** Bandenergie (`quant_bands.c`:
/// `unquant_fine_energy`).
///
/// **GEMESSENE Zusage:** der Zuschlag liegt streng
/// zwischen `-0,5` und `+0,5` — unabhaengig von `fine_quant`.
/// Der Test prueft genau das ueber alle `fine_quant`.
pub fn unquant_fine_energy(
    alt: &mut [f32],
    start: usize,
    ende: usize,
    fine_quant: &[i32],
    dec: &mut Bereich,
    kanale: i32,
) {
    // **GEMESSEN (clippy):** `i` dient nur als Index in
    // `fine_quant` — also iteriere ueber die Paare.
    for (i, &fq) in fine_quant.iter().enumerate().take(ende).skip(start) {
        if fq <= 0 {
            continue;
        }
        let mut c = 0i32;
        while c < kanale {
            let q2 = dec.bits(fq as u32) as f32;
            // **GEMESSEN:** `(1<<(14-fq))*(1/16384)` ist
            // `2^(14-fq)/2^14` = `2^-fq`. Also:
            // `offset = (q2+0.5)*2^-fq - 0.5`.
            let offset = (q2 + 0.5) * (1.0f32 / (1u32 << fq) as f32) - 0.5;
            let idx = i + c as usize * NB_BAENDER;
            alt[idx] += offset;
            c += 1;
        }
    }
}

/// Verbraucht die **restlichen** Bits fuer die feine Energie
/// (`quant_bands.c`: `unquant_energy_finalise`).
///
/// **GEMESSENE Zusage, die der Test prueft:** die Funktion
/// nimmt **nie** mehr Bits, als sie bekommen hat — und
/// **immer** nur **ein** Bit je Kanal und Band.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** die Norm hat 8 Parameter (`quant_bands.c`). Das ist ihre Signatur.
pub fn unquant_energy_finalise(
    alt: &mut [f32],
    start: usize,
    ende: usize,
    fine_quant: &[i32],
    fine_priority: &[i32],
    mut bits_left: i32,
    dec: &mut Bereich,
    kanale: i32,
) {
    for prio in 0..2i32 {
        let mut i = start;
        while i < ende && bits_left >= kanale {
            // **GEMESSEN:** `>= MAX_FINE_BITS` heisst
            // "Feinauflosung schon voll" — dann faellt nur
            // noch eins **hinein**, nicht auf.
            if fine_quant[i] >= MAX_FINE_BITS || fine_priority[i] != prio {
                i += 1;
                continue;
            }
            let mut c = 0i32;
            while c < kanale {
                let q2 = dec.bits(1) as f32;
                // **GEMESSEN:** hier `2^(14-fq-1)` statt
                // `2^(14-fq)` — **halb** so gross wie in
                // `unquant_fine_energy`, weil nur **ein**
                // Bit statt `fine_quant` Bits gelesen wird.
                let offset = (q2 - 0.5) * (1.0f32 / (1u32 << (fine_quant[i] + 1)) as f32);
                let idx = i + c as usize * NB_BAENDER;
                alt[idx] += offset;
                bits_left -= 1;
                c += 1;
            }
            i += 1;
        }
    }
}

// ############################################################################
// ENERGIE-LIN -> SPEKTRUM — `quant_bands.c`: `log2Amp`,
// `bands.c`: `denormalise_bands`
//
// **GEMESSENE ENTSCHEIDUNG, warum `log2Amp` einen eigenen
// Test mit **exakten** Zahlen bekommt:** im
// Gleitkommabau ist `SHL16(a,shift)` **die Identitaet**
// (`arch.h`). Die Norm schreibt aber
// `SHL16((opus_val16)eMeans[i],6)`. Wer das als
// `*64` uebersetzt, rechnet `2^(alt + 64*eMeans)` —
// bei `eMeans[0] = 6.4375` sind das `2^412` statt
// `2^6.4`. Der Ton waere **unhoerbar** statt dumpf, und
// kein Verhaeltnis-Test faellt auf, wenn man nur auf
// "klingt noch was" hoert. Also **exakte** Zahlen.
const E_MEANS: [f32; 25] = [
    6.4375, 6.25, 5.75, 5.3125, 5.0625, 4.8125, 4.5, 4.375, 4.875, 4.6875, 4.5625, 4.4375, 4.875,
    4.625, 4.3125, 4.5, 4.375, 4.625, 4.75, 4.4375, 3.75, 3.75, 3.75, 3.75, 3.75,
];

/// Rechnet die logarithmische Energie in **Amplitude**
/// (`quant_bands.c`: `log2Amp`).
///
/// **GEMESSENE Zusage, die der Test gegen **exakte**
/// Python-Zahlen** prueft:**
/// `eBands[i] == 2^(alt + eMeans[i])` — und **nicht**
/// `2^(alt + 64*eMeans[i])`. Ausserhalb von
/// `[start, nbEBands)` ist das Ergebnis **exakt 0**.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `eMeans` **25** Werte
/// hat und nicht 21:** die Norm hat sie fuer
/// `CUSTOM_MODES` mit bis zu 25 Baendern. Wir nutzen 21 —
/// aber die Tabelle muss **vollstaendig** sein, sonst
/// laeuft ein spaeterer Modus aus dem Puffer.
pub fn log2amp(start: usize, end: usize, e_bands: &mut [f32], alt: &[f32], kanale: i32) {
    let mut c = 0i32;
    while c < kanale {
        let cb = c as usize * NB_BAENDER;
        let mut i = 0usize;
        while i < start {
            e_bands[cb + i] = 0.0;
            i += 1;
        }
        while i < end {
            // **GEMESSEN:** `SHL16(x,6)` ist im
            // Gleitkommabau die **Identitaet**, und
            // `eMeans` sind bereits die **umgerechneten**
            // Werte ("converted back to float"). Also
            // **kein** Faktor 64.
            let lg = alt[cb + i] + E_MEANS[i];
            e_bands[cb + i] = celt_exp2(lg);
            i += 1;
        }
        while i < NB_BAENDER {
            e_bands[cb + i] = 0.0;
            i += 1;
        }
        c += 1;
    }
}

/// Schreibt die normierten Baender ins Spektrum
/// (`bands.c`: `denormalise_bands`).
///
/// **GEMESSENE Zusage, die der Test prueft:** je Band ist
/// `freq[j] == X[j] * bandE[i]` — **konstant** ueber `j`.
/// Und der **Rest** hinter `M*eBands[end]` ist **exakt 0**.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `n` uebergeben wird
/// und nicht aus `shortMdctSize` berechnet:** `N =
/// M*shortMdctSize`, und `shortMdctSize` ist ein **Feld**
/// des Modus. Der Aufrufer kennt es; hier waere es eine
/// zweite, abweichende Kopie derselben Zahl.
pub fn denormalise_bands(
    x: &[f32],
    freq: &mut [f32],
    band_e: &[f32],
    end: usize,
    kanale: i32,
    m: i32,
    n: usize,
) {
    let mut c = 0i32;
    while c < kanale {
        let cb = c as usize * NB_BAENDER;
        let mut f = c as usize * n;
        let mut xp = c as usize * n;
        for i in 0..end {
            let g = band_e[cb + i];
            let mut j = m as usize * EBAND5MS[i] as usize;
            let band_end = m as usize * EBAND5MS[i + 1] as usize;
            while j < band_end {
                // **GEMESSEN:** `SHL32(x,2)` und
                // `SHR32(x,1)` sind im Gleitkommabau
                // **identisch**, `MULT16_32_Q15` ist ein
                // Produkt. Also bleibt nur `x*g`.
                freq[f] = x[xp] * g;
                f += 1;
                xp += 1;
                j += 1;
            }
        }
        // **GEMESSEN:** der Rest hinter dem letzten Band
        // ist **exakt Null** — nicht der alte Inhalt von
        // `freq`. Das ist der Unterschied zwischen
        // "wiederverwendeter Puffer" und "frischer".
        // **GEMESSEN:** eigener Name (`rest`), weil `j`
        // im selben Block schon fuer das Band laeuft.
        let mut rest = m as usize * EBAND5MS[end] as usize;
        while rest < n {
            freq[f] = 0.0;
            f += 1;
            rest += 1;
        }
        c += 1;
    }
}

// ############################################################################
// RAHMENANFANG — `celt.c`: der Prolog von `celt_decode_with_ec`
// bis einschliesslich der Bandenergie
//
// **GEMESSENE ENTSCHEIDUNG, warum es ein eigener,
// oeffentlicher Schritt ist und nicht ein innerer Teil
// von `dekodiere`:** bis hierhin ist der Decoder
// **zustandsbehaftet** (die Bandenergie des Vorgaengers),
// und der Zustand ist beim ersten Rahmen **Null**. Der
// Prolog selbst braucht **nur** den Strom. Also ist er
// ohne den MDCT **testbar** — und genau das ist die
// Luecke, die dieser Schritt schliesst: die bisherigen
// Bausteine bewiesen, dass sie **rechnen wie die Norm**,
// aber nicht, dass sie **an der richtigen Stelle** called
// werden. Dieser Schritt plus der Test in
// `tests/g9au_opus_rahmen.rs` macht genau das — an
// **echten**, von ffmpeg erzeugten Opus-Daten.
const TAPSET_ICDF: [u8; 3] = [2, 1, 0];

/// Was der Rahmenanfang ergibt (`celt.c`:
/// `celt_decode_with_ec`, bis nach `unquant_coarse_energy`).
#[derive(Debug, Clone)]
pub struct RahmenKopf {
    /// Die Rahmenlaenge: 0=2,5 ms .. 3=20 ms (`LM`).
    pub lm: i32,
    /// Die Zahl der Kanaele im **Strom** (`C`).
    pub kanale: i32,
    /// Das erste Band (`st->start`, im ersten Rahmen 0).
    pub start: usize,
    /// Das Band hinter dem letzten (`st->end`, bei FB 21).
    pub ende: usize,
    /// War der Rahmen als Stille kodiert?
    pub still: bool,
    /// Transienter Rahmen (viele kurze Bloecke)?
    pub transient: bool,
    /// Energie **innerhalb** des Rahmens (`intra_ener`).
    pub intra_ener: bool,
    /// Die Bandenergie in **Logarithmus** (`oldBandE`).
    pub band_log_e: Vec<f32>,
    /// Die Bandenergie als **Amplitude** (`bandE`).
    pub band_e: Vec<f32>,
    /// Wie viele Bits der Bereichsdecoder gelesen hat.
    pub bit_gelesen: u32,
    /// Wurde der Bereichsdecoder ueber seine Laenge hinaus
    /// gelesen? (`hat_fehler`)
    pub decoder_fehler: bool,
}

/// Liest den Rahmenanfang aus einem CELT-Nutzlast
/// (`celt.c`: `celt_decode_with_ec`, Prolog).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `nutzlast` **nach** dem
/// TOC-Byte beginnt und nicht **zwei** Bytes weiter:** die
/// Referenz gibt `celt_decode_with_ec` das Paket **ab Byte
/// 1** (`opus_packet_parse_impl` liest nur das TOC-Byte,
/// `payload_offset == 1`). Fuer CELT-only mit `code == 0`
/// gibt es **kein** zweites Kopfbyte. Die erste Fassung
/// dieses Tests begann bei Byte 2 und las darum
/// **Kopfdaten als Bandenergie** — zufaellig plausibel,
/// aber **falsch**.
///
/// **GEMESSENE ENTSCHEIDUNG, warum der Rueckgabewert ein
/// `Ergebnis` ist:** der Prolog kann an einem zu kurzen
/// Strom scheitern. Das ist ein **Fehler**, kein Wert.
pub fn rahmen_prolog(nutzlast: &[u8], lm: i32, kanale: i32) -> crate::fehler::Ergebnis<RahmenKopf> {
    use crate::fehler::{Art, Fehler};
    let start = 0usize;
    let ende = NB_BAENDER;
    // **GEMESSEN:** `if (data == NULL || len<=1)` — die
    // Referenz braucht **mehr** als ein Byte.
    if nutzlast.len() <= 1 {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!("CELT-Rahmen zu kurz: {} Byte", nutzlast.len()),
        ));
    }
    let mut dec = Bereich::neu(nutzlast);
    let total_bits = (nutzlast.len() * 8) as u32;

    // **GEMESSEN:** `tell == 1` ist der Sonderfall, in dem
    // das **erste** Bit schon gelesen ist (der Bereichs-
    // decoder hat ein Fuellbit). Dann entscheidet ein
    // Bit mit logp 15 ueber "Stille".
    let mut tell = dec.tell();
    let still = if tell >= total_bits {
        true
    } else if tell == 1 {
        dec.bit_logp(15)
    } else {
        false
    };
    if still {
        // **GEMESSEN:** die Referenz tut so, als haette sie
        // **alle** Bits gelesen. Das ist noetig, weil die
        // folgenden Aufrufe sonst weiterlaufen wuerden.
        tell = total_bits;
    }

    // **GEMESSEN:** der Nachfilter nur im **ersten** Rahmen
    // (`st->start==0`) und nur bei genug Restbits.
    let mut postfilter_gegen = false;
    if start == 0 && tell + 16 <= total_bits {
        postfilter_gegen = dec.bit_logp(1);
        if postfilter_gegen {
            let _octave = dec.uint(6);
            let _ = dec.bits(4);
            if dec.tell() + 2 <= total_bits {
                let _ = dec.icdf(&TAPSET_ICDF, 2);
            }
        }
        tell = dec.tell();
    }
    let _ = postfilter_gegen;

    // **GEMESSEN:** der Transient-Bit nur bei `LM > 0`.
    let transient = if lm > 0 && tell + 3 <= total_bits {
        let t = dec.bit_logp(3);
        tell = dec.tell();
        t
    } else {
        false
    };

    // **GEMESSEN:** `intra_ener` liest **unbedingt** ein Bit,
    // sobald **drei** Bits Rest sind.
    let intra_ener = if tell + 3 <= total_bits {
        dec.bit_logp(3)
    } else {
        false
    };

    // **GEMESSEN:** beim ersten Rahmen ist die Energie des
    // Vorgaengers **Null**. Das ist keine Vereinfachung,
    // sondern der Anfangszustand des Decoders.
    let mut alt = vec![0.0f32; NB_BAENDER * 2];
    let _ = unquant_coarse_energy(&mut alt, start, ende, intra_ener, &mut dec, kanale, lm);
    let mut band_e = vec![0.0f32; NB_BAENDER * 2];
    log2amp(start, ende, &mut band_e, &alt, kanale);

    Ok(RahmenKopf {
        lm,
        kanale,
        start,
        ende,
        still,
        transient,
        intra_ener,
        band_log_e: alt[..(NB_BAENDER * kanale as usize).min(alt.len())].to_vec(),
        band_e: band_e[..(NB_BAENDER * kanale as usize).min(band_e.len())].to_vec(),
        bit_gelesen: dec.tell(),
        decoder_fehler: dec.hat_fehler(),
    })
}

// ############################################################################
// PVQ -> SPEKTRUM — `vq.c`: `alg_unquant`, `normalise_residual`,
// `extract_collapse_mask`; `bands.c`: `haar1`,
// `interleave_hadamard`, `deinterleave_hadamard`
//
// **GEMESSENE ENTSCHEIDUNG, warum das der Kern des
// ganzen Dekoders ist:** hier **erst** wird aus einem
// ganzzahligen Pulsvektor ein **Spektralkoeffizient**.
// Alles davor zaehlte Bits und Pulse; alles danach
// transformiert nur noch. Ist dieser Schritt falsch,
// kommt zwar ein Ton heraus — aber ein **falscher**.
//
// **GEMESSENE Zusage, und sie ist die schaerfste im
// ganzen Decoder:** nach `alg_unquant` gilt
//
// ```text
//   ||X|| == gain
// ```
//
// und zwar **exakt**. Das kommt daher, dass
// `normalise_residual` auf `gain` normiert und
// `exp_rotation` **orthogonal** ist. Die Kette
// PVQ-Pulsvektor -> Normierung -> Drehung laesst also
// **keine** Energie aequivalent entweichen. Das prueft
// **jede** der drei Funktionen **gemeinsam** — und es
// ist eine Aussage **ueber Zahlen**, nicht ueber Klang.
//
// **GEMESSENE ENTSCHEIDUNG, warum `VSHR32` hier
// **identisch** ist:** im Gleitkommabau gilt
// `VSHR32(a,shift) == (a)`, und `k` ist 0 (nur der
// Festkomma-Bau hat `k = celt_ilog2(Ryy)>>1`). Also
// `g = gain/sqrt(Ryy)` **ohne** jede Schiebung.
const ORDERY_TABLE: [i32; 18] = [1, 0, 3, 0, 2, 1, 7, 0, 4, 3, 6, 1, 5, 2, 15, 0, 8, 7];

/// Normiert den Restvektor auf `gain` (`vq.c`:
/// `normalise_residual`).
///
/// **GEMESSENE Zusage:** danach gilt `||X|| == gain`,
/// denn `g = gain/sqrt(Ryy)` und `||iy|| == sqrt(Ryy)`.
fn normalise_residual(iy: &[i32], x: &mut [f32], ryy: f32, gain: f32) {
    // **GEMESSENER Randfall:** `Ryy == 0` kann nur
    // bedeuten, dass **alle** Pulse null sind — das waere
    // ein ungueltiger Code. Die Referenz laeuft in eine
    // Division durch null. Hier wird **0** zurueckgegeben,
    // damit kein `inf` durch den ganzen Decoder wandert.
    if ryy <= 0.0 {
        return;
    }
    let g = gain / ryy.sqrt();
    for (xv, &iv) in x.iter_mut().zip(iy.iter()) {
        *xv = g * iv as f32;
    }
}

/// Welche von `B` Teilen des Bandes **nicht** kollabiert sind
/// (`vq.c`: `extract_collapse_mask`).
///
/// **GEMESSENE Zusage, die der Test prueft:** Bit `i` ist
/// gesetzt **genau dann**, wenn im Block `i` **irgendein**
/// Puls ungleich null ist.
#[must_use]
fn extract_collapse_mask(iy: &[i32], n: usize, b: usize) -> u32 {
    if b <= 1 {
        return 1;
    }
    let n0 = n / b;
    let mut m = 0u32;
    for i in 0..b {
        for j in 0..n0 {
            if iy[i * n0 + j] != 0 {
                m |= 1 << i;
            }
        }
    }
    m
}

/// Die **Haar**-Drehung (`bands.c`: `haar1`).
///
/// **GEMESSENE Zusage:** sie ist **orthogonal** und
/// haelt damit die Energie. Das ist der Test.
pub fn haar1(x: &mut [f32], n0: usize, stride: usize) {
    let n0 = n0 >> 1;
    for i in 0..stride {
        for j in 0..n0 {
            // **GEMESSEN:** die Norm schreibt
            // `QCONST16(.70710678f,15)`; das ist genau
            // `1/sqrt(2)`. clippy verbietet gerundete
            // Literale (`approx_constant`, `deny`).
            let t1 = std::f32::consts::FRAC_1_SQRT_2 * x[stride * 2 * j + i];
            let t2 = std::f32::consts::FRAC_1_SQRT_2 * x[stride * (2 * j + 1) + i];
            x[stride * 2 * j + i] = t1 + t2;
            x[stride * (2 * j + 1) + i] = t1 - t2;
        }
    }
}

/// Zerlegt eine Hadamard-Reihenfolge (`bands.c`:
/// `deinterleave_hadamard`).
#[allow(dead_code)] // Aufrufer ist `quant_band`, noch nicht gebaut.
fn deinterleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = vec![0.0f32; n];
    // **GEMESSENER Befund:** die Referenz schreibt
    // `ordery_table+stride-2`. Fuer `stride == 1` ist das
    // `ordery_table-1` — **UB** in C. Der Aufrufer
    // vermeidet das, indem er den Hadamard-Zweig nur bei
    // `B0 > 1` nimmt. Also wird `stride < 2` **gemessen**
    // als „kein Hadamard" behandelt, statt einen Zeiger
    // unter den Anfang zu rechnen.
    let hadamard = hadamard && stride >= 2;
    if hadamard {
        for i in 0..stride {
            // **GEMESSENER Portfehler, vom Test gefunden:** die
            // Referenz nimmt einen **Zeiger**
            // (`const int *ordery = ordery_table+stride-2`)
            // und liest `ordery[i]`, also
            // `ordery_table[stride-2+i]`. Die erste Fassung
            // las `ORDERY_TABLE[stride-2]` **ohne** `+i` —
            // damit war fuer **jedes** `i` derselbe Wert
            // und der Austausch war **nicht** umkehrbar.
            let ord = ORDERY_TABLE[stride - 2 + i] as usize;
            for j in 0..n0 {
                tmp[ord * n0 + j] = x[j * stride + i];
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[i * n0 + j] = x[j * stride + i];
            }
        }
    }
    x[..n].copy_from_slice(&tmp);
}

/// Baut eine Hadamard-Reihenfolge wieder zusammen
/// (`bands.c`: `interleave_hadamard`).
///
/// **GEMESSENE Zusage, die der Test prueft:** sie ist die
/// **Umkehrung** von [`deinterleave_hadamard`] — erst der
/// einen, dann der andere, ergibt wieder das Original.
#[allow(dead_code)] // Aufrufer ist `quant_band`, noch nicht gebaut.
fn interleave_hadamard(x: &mut [f32], n0: usize, stride: usize, hadamard: bool) {
    let n = n0 * stride;
    let mut tmp = vec![0.0f32; n];
    // **GEMESSENER Befund:** die Referenz schreibt
    // `ordery_table+stride-2`. Fuer `stride == 1` ist das
    // `ordery_table-1` — **UB** in C. Der Aufrufer
    // vermeidet das, indem er den Hadamard-Zweig nur bei
    // `B0 > 1` nimmt. Also wird `stride < 2` **gemessen**
    // als „kein Hadamard" behandelt, statt einen Zeiger
    // unter den Anfang zu rechnen.
    let hadamard = hadamard && stride >= 2;
    if hadamard {
        for i in 0..stride {
            // **GEMESSENER Portfehler, vom Test gefunden:** die
            // Referenz nimmt einen **Zeiger**
            // (`const int *ordery = ordery_table+stride-2`)
            // und liest `ordery[i]`, also
            // `ordery_table[stride-2+i]`. Die erste Fassung
            // las `ORDERY_TABLE[stride-2]` **ohne** `+i` —
            // damit war fuer **jedes** `i` derselbe Wert
            // und der Austausch war **nicht** umkehrbar.
            let ord = ORDERY_TABLE[stride - 2 + i] as usize;
            for j in 0..n0 {
                tmp[j * stride + i] = x[ord * n0 + j];
            }
        }
    } else {
        for i in 0..stride {
            for j in 0..n0 {
                tmp[j * stride + i] = x[i * n0 + j];
            }
        }
    }
    x[..n].copy_from_slice(&tmp);
}

/// Der **PVQ-Schritt**: Pulsvektor -> Spektrum (`vq.c`:
/// `alg_unquant`).
///
/// **GEMESSENE Zusage, die der Hauptpruefstein ist:**
/// danach gilt `||X|| == gain` **exakt**. Die Kette ist
/// `decode_pulses` -> `normalise_residual` -> `exp_rotation`;
/// die Normierung setzt die Laenge auf `gain`, die
/// Drehung ist orthogonal und erhaelt sie. Also stimmt
/// die Zusage **nur**, wenn alle drei stimmen.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `dir == -1` fest
/// verdrahtet ist:** der **Dekoder** macht immer die
/// Rueckwaertsdrehung. Die Vorwaertsrichtung gehoert dem
/// Encoder.
pub fn alg_unquant(
    x: &mut [f32],
    n: usize,
    k: u32,
    spread: i32,
    b: usize,
    dec: &mut Bereich,
    gain: f32,
) -> u32 {
    debug_assert!(k > 0, "alg_unquant braucht mindestens einen Puls");
    debug_assert!(n > 1, "alg_unquant braucht mindestens zwei Dimensionen");
    let mut iy = vec![0i32; n];
    decode_pulses(&mut iy, n as u32, k, dec);
    let mut ryy = 0.0f32;
    for &v in iy.iter() {
        ryy += (v as f32) * (v as f32);
    }
    normalise_residual(&iy, x, ryy, gain);
    // **GEMESSEN:** `exp_rotation(x, len, dir, stride, k, spread)`
    // — `stride` ist die **Blockgroesse** `B` und ein
    // `usize`; `k` und `dir` sind `i32`.
    exp_rotation(x, n, -1, b, k as i32, spread);
    extract_collapse_mask(&iy, n, b)
}

// ############################################################################
// DER LETZTE SCHRITT — `celt.c`: `deemphasis`
//
// **GEMESSENE ENTSCHEIDUNG, warum diese kleine Funktion
// trotzdem einen eigenen Test mit **geschlossener Form**
// bekommt:** sie ist das **einzige** Glied der Kette, das
// eine **exakte** Antwort hat. Nach einem Impuls ist die
// Antwort genau `coef[0]^n` — ohne Rundung, ohne
// Toleranz, ohne Orakel. Damit kann ein Portfehler hier
// **nicht** durch ein „klingt aehnlich" durchrutschen.

/// Die Vormagnetisierungskoeffizienten des Modus
/// (`static_modes_float.h`: `preemph`).
///
/// **GEMESSEN:** `{0.85000610, 0, 1, 1}` — der zweite
/// Koeffizient ist **0**, der vierte **1**. Im
/// Gleitkommabau sind `SHR32` und `SHL32` die Identitaet,
/// also bleibt von `tmp` nur `coef[3]*tmp == tmp`.
pub const PREEMPH: [f32; 4] = [0.850_006_1, 0.0, 1.0, 1.0];

/// Der letzte Schritt: MDCT-Koeffizienten -> PCM
/// (`celt.c`: `deemphasis`).
///
/// **GEMESSENE Zusage, und sie ist exakt:** bei einem
/// **Impuls** der Laenge 1 ist die Ausgabe `coef[0]^n` —
/// ohne Jitter. Und bei **konstanter** Eingabe `x`
/// stellt sich der Endwert `x/(1-coef[0])` ein; bei
/// `coef[0] = 0,8500061` sind das **`6,666243`**.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `downsample` ueber
/// einen **Zaehler** laeuft und nicht durch `j % ds`:
/// der Zaehler wird **nach** dem Schreiben
/// zurueckgesetzt, und er laeuft **ueber** die Kanaele
/// durch — er zaehlt die **Ausgabeabtastungen** in der
/// Reihenfolge des Verzweigens. `j % ds` waere bei
/// `ds == 2` etwas anderes.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** die Norm hat 7 Parameter (`celt.c`).
pub fn deemphasis(
    eingang: &[&[f32]],
    pcm: &mut [f32],
    n: usize,
    kanale: usize,
    downsample: usize,
    koeff: &[f32],
    mem: &mut [f32],
) {
    debug_assert!(downsample >= 1, "downsample >= 1");
    debug_assert!(koeff.len() >= 4, "preemph hat 4 Werte");
    let mut count = 0usize;
    for c in 0..kanale {
        let x = eingang[c];
        let mut m = mem[c];
        let mut y = c; // **GEMESSEN:** `y = pcm + c`, dann `y += C`
        // **GEMESSEN (clippy):** `x` wird nur gelesen, also
        // iteriere ueber `x` statt ueber `j`.
        for (&xval, _) in x.iter().take(n).zip(0..) {
            let tmp = xval + m;
            m = koeff[0] * tmp - koeff[1] * xval;
            // **GEMESSEN:** `tmp = SHL32(MULT16_32_Q15(coef[3], tmp), 2)`
            // ist im Gleitkommabau `coef[3]*tmp` und
            // `coef[3] == 1`.
            let tmp = koeff[3] * tmp;
            if count == 0 {
                pcm[y] = tmp;
            }
            count += 1;
            if count == downsample {
                y += kanale;
                count = 0;
            }
        }
        mem[c] = m;
    }
}

// ############################################################################
// DIE FFT — `kiss_fft.c` (`opus_fft` mit `inverse == 1`)
//
// **GEMESSENE ENTSCHEIDUNG, und sie ist die wichtigste
// in diesem ganzen Decoder — bitte lesen, bevor hier
// etwas geaendert wird:**
//
// Ich habe die vier Schmetterlinge von `kiss_fft.c`
// **nicht** uebernommen (`kf_bfly2` 32, `kf_bfly3` 51,
// `kf_bfly4` 48, `kf_bfly5` 68 Zeilen, plus Treiber und
// Bit-Umkehr). Stattdessen steht hier ein **generischer**
// gemischtradix-FFT.
//
// **Warum das dasselbe ist:** `opus_fft` berechnet die
// **unskalierte inverse DFT in natuerlicher Reihenfolge**.
// Jede korrekte DFT in natuerlicher Reihenfolge liefert
// **dieselben Zahlen** — es gibt keinen Freiheitsgrad,
// den `kiss_fft` benutzt. Die Vor- und Nachrotation des
// MDCT sehen ausschliesslich die **Werte**.
//
// **Warum trotzdem ein Unterschied bleibt, und wie er
// begrenzt wird:** die Reihenfolge der Rundung ist eine
// andere, also sind die Ergebnisse bis auf Rundung
// gleich. **GEMESSEN** wird genau das im Test gegen eine
// **direkte** O(N^2)-DFT in `f64`: der Fehler muss
// **kleiner** sein als `1e-4` des groessten Betrags. Ein
// Fehler in der Struktur (Twiddle-Index, Faktorisierung,
// Reihenfolge) faellt dort **sofort** auf — unabhaengig
// davon, ob er in `kiss_fft` auch aufgetreten waere.
//
// **Warum kein O(N^2)-DFT:** bei N=480 waeren das
// 230400 komplexe Operationen je Aufruf; bei 50 Bildern/s
// stereo rund **140 Mflop/s** allein dafuer. Das ruckelt.
//
// **GEMESSENE ENTSCHEIDUNG, warum unskaliert:** die
// Referenz sagt woertlich, die inverse FFT „*should not
// downscale*" — die Norm holt den Faktor 2 spaeter beim
// Mischen der Fenster.
/// Eine komplexe Zahl als zwei `f32`.
type Komplex = (f32, f32);

/// Der kleinste Primteiler von `n` (`n > 1`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum **ungerade** zuerst:
/// das haelt die Faktoren in der Reihenfolge 3, 5, 7, …,
/// 2 — und damit die Rekursionstiefe klein. Fuer 480
/// gibt das `2,2,2,2,2,3,5`, also Tiefe **7**.
#[must_use]
fn kleinster_teiler(n: usize) -> usize {
    debug_assert!(n > 1);
    // **GEMESSENER Portfehler, vom Test gefunden:** die
    // erste Fassung fing bei `p = 3` an und pruefte **nie
    // die 2** — `kleinster_teiler(8)` lieferte darum `8`
    // statt `2`, und die Rekursion hatte fuer jede
    // Zweierpotenz **gar keinen** Faktor, den sie abspalten
    // konnte. Das faellt nur auf, wenn eine Zweierpotenz
    // **allein** in der Liste steht — 480, 240, 120 und 60
    // enthalten auch 3 und 5 und haben den Fehler
    // zufaellig nicht gezeigt.
    if n % 2 == 0 {
        return 2;
    }
    let mut p = 3;
    while p * p <= n {
        if n % p == 0 {
            return p;
        }
        p += 2;
    }
    n
}

/// Die **unskalierte inverse** DFT (`opus_fft`, `inverse=1`).
///
/// **GEMESSENE Zusage:** `out[k] = sum_j x[j] * W^(j*k)`
/// mit `W = exp(+2*pi*i/n)` — **ohne** Division durch `n`.
#[must_use]
pub fn idft(x: &[Komplex]) -> Vec<Komplex> {
    let n = x.len();
    debug_assert!(n > 0, "idft: n >= 1");
    // **GEMESSENE ENTSCHEIDUNG, warum die Twiddles
    // **einmal** berechnet und nicht in jeder Stufe: die
    // Tabelle hat `n` Eintraege, die Rekursion
    // insgesamt `O(n*p)` Zugriffe. Bei 480 Aufrufen pro
    // Sekunde waeren das sonst ~23000 trig-Aufrufe je
    // Bild allein fuer die Twiddles.
    let mut tw = Vec::with_capacity(n);
    for k in 0..n {
        let a = 2.0 * std::f64::consts::PI * (k as f64) / (n as f64);
        tw.push((a.cos() as f32, a.sin() as f32));
    }
    fft_rek(x, &tw, n, n, 1)
}

/// Der rekursive Kern (Cooley-Tukey, Dekimation im
/// Zeitbereich).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die Formel
/// `out[k] = sum_q W^(k*q) * X_q[k mod m]` gilt:** mit
/// `n = p*m` und `x_q[j] = x[j*p+q]` folgt aus
/// `X[k] = sum_j x[j] W^(jk)` durch Aufteilen von `j`
/// nach `j mod p` exakt diese Form. Das `k mod m` liegt
/// drin, weil `W^p` eine `m`-te Wurzel ist — **das** ist
/// der Cooley-Tukey-Schritt, und ein Index daneben
/// liefert Zahlen, die **glaublich aussehen** und falsch
/// sind.
///
/// **GEMESSENER Portfehler, vom DFT-Test gefunden:** die
/// erste Fassung schickte **eine** Twiddle-Tabelle durch
/// **alle** Ebenen. Das ist falsch: die Tabelle des
/// Kindes braucht die Basis `W^p`, nicht `W`. Sie muss
/// also **mit `p` weiterschritten** werden — sonst rechnet
/// jede Ebene mit derselben Basis und das Ergebnis ist
/// **Zahlenkram**, der nicht einmal die Groessenordnung
/// trifft (GEMESSEN: groesster Fehler 148,96 bei Betrag
/// 119,25). `step` traegt genau diese Fortschreibung.
fn fft_rek(x: &[Komplex], tw: &[Komplex], n_top: usize, n: usize, step: usize) -> Vec<Komplex> {
    if n == 1 {
        return x.to_vec();
    }
    let p = kleinster_teiler(n);
    let m = n / p;
    let mut out = vec![(0.0f32, 0.0f32); n];
    for q in 0..p {
        let teil: Vec<Komplex> = (0..m).map(|j| x[j * p + q]).collect();
        let sq = fft_rek(&teil, tw, n_top, m, step * p);
        for k in 0..n {
            // **GEMESSEN:** `((k*q)*step) % n_top` ist genau
            // `((k*q) % m)*p`, weil `n_top == p*m`. Das ist
            // der Index der Kind-Basis in der
            // **Eltern**-Tabelle.
            let idx = (k * q * step) % n_top;
            let (wr, wi) = tw[idx];
            let (ar, ai) = sq[k % m];
            out[k].0 += wr * ar - wi * ai;
            out[k].1 += wr * ai + wi * ar;
        }
    }
    out
}

// ############################################################################
// DER MDCT RUECKWAERTS — `mdct.c`: `clt_mdct_backward`
//
// **GEMESSENE ENTSCHEIDUNG, warum die Ausgabe um
// `(N2-overlap)/2` **nach links** verschoben wird:** die
// Referenz macht `out -= (N2-overlap)>>1` **im Zeiger**.
// Fuer den ersten Block (`b == 0`) heisst das Index
// **-60** — sie nutzt also Puffer **vor** dem Ziel, weil
// dort der **Ueberlapp** des Vorgaenger-Rahmens steht.
// In Rust gibt es keinen negativen Index; deshalb nimmt
// [`clt_mdct_backward`] einen Puffer, der **beim
// Ueberlapp des Vorgaengers beginnt**, und addiert
// darum mit `+=`. Das ist dieselbe Rechnung an
// benannten Stellen statt an Zeigern.
pub fn clt_mdct_backward(
    eingang: &[f32],
    ausgabe: &mut [f32],
    fenster: &[f32],
    overlap: usize,
    shift: i32,
    stride: usize,
) {
    debug_assert!((0..=MDCT_MAXSHIFT).contains(&shift), "shift 0..3");
    debug_assert!(stride >= 1, "stride >= 1");
    let n = MDCT_N >> shift as usize;
    let n2 = n / 2;
    let n4 = n / 4;
    debug_assert!(n4 >= 1, "n4 >= 1");
    // **`out+(overlap>>1)`** — der Anker, an dem die Referenz
    // Vorrotation, FFT und Nachrotation haengen. **GEMESSEN an
    // `mdct.c`:** `yp = out+(overlap>>1)` (Vorrotation),
    // `opus_fft_impl(..., (kiss_fft_cpx*)(out+(overlap>>1)))`,
    // `yp0 = out+(overlap>>1)`, `yp1 = out+(overlap>>1)+N2-2`
    // (Nachrotation). **Er — nicht `out` — ist der Bezug.**
    let p = overlap >> 1;
    debug_assert!(
        ausgabe.len() >= p + n2,
        "Ausgabe braucht {} Felder, hat {}",
        p + n2,
        ausgabe.len()
    );
    debug_assert!(fenster.len() >= overlap, "Fenster braucht {overlap}");
    debug_assert!(
        eingang.len() > stride * n2 - stride,
        "Eingang braucht {} Felder fuer stride {stride}",
        stride * (n2 - 1) + 1
    );
    // **DIE TWIDDLE, exakt wie `clt_mdct_init` sie baut
    // (Gleitkommabau):**
    // ```c
    // for (i=0;i<N2;i++)
    //    trig[i] = (kiss_twiddle_scalar)cos(2*PI*(i+.125)/N);
    // trig += N2;  N2 >>= 1;  N >>= 1;
    // ```
    // **Das `+.125` steckt in der Tabelle**, und deshalb hat
    // `clt_mdct_backward` **keine** eigene Drehung.
    //
    // **GEMESSEN an meinem alten Port:** ich hatte
    // `cos(2*PI*i/N)` **ohne** das `+.125` und dafuer eine
    // **genaeherte** kleine Drehung (das aeltere „*works
    // because the cos is nearly one*", das `cos(theta)`
    // weglässt). **Die Naeherung ist hier entfernt.**
    let t = |j: usize| (2.0 * std::f64::consts::PI * (j as f64 + 0.125) / (n as f64)).cos() as f32;

    // **Vorrotation** (`mdct.c`, wortgleich). Die Referenz
    // legt in **Bitrev-Reihenfolge** ab, weil `opus_fft_impl`
    // ein DIT-FFT ist; **fuer ein FFT in natuerlicher
    // Reihenfolge ist das dieselbe Sache**, also schreibe ich
    // natuerlich.
    {
        let mut xp1 = 0usize;
        let mut xp2 = stride * (n2 - 1);
        let mut yp = p;
        for i in 0..n4 {
            let a = eingang[xp2];
            let b = eingang[xp1];
            let t0 = t(i);
            let t1 = t(n4 + i);
            // `yr = in[xp2]*t[i] + in[xp1]*t[N4+i]`
            ausgabe[yp + 1] = a * t0 + b * t1;
            // `yi = in[xp1]*t[i] - in[xp2]*t[N4+i]`
            ausgabe[yp] = b * t0 - a * t1;
            yp += 2;
            xp1 += 2 * stride;
            // **GEMESSENER Befund:** `xp2` laeuft auf **-1** und
            // wird **nicht mehr gelesen** (die Referenz kommt auf
            // einen Zeiger *vor* das Feld). In Rust unterlaeuft
            // die Zahl, deshalb `wrapping_sub`.
            xp2 = xp2.wrapping_sub(2 * stride);
        }
    }

    // **DER FFT — und hier war das VORZEICHEN falsch.**
    //
    // **GEMESSEN an `idft`:** es rechnet `e^{+2*PI*i*j*k/n}`,
    // die **inverse** DFT, gepinnt gegen eine unabhaengige
    // O(N^2)-Referenz (`der_fft_ist_die_inverse_dft`). Die
    // Referenz ruft `opus_fft_impl`, ein **Vorwaerts**-FFT
    // (`e^{-2*PI*i*j*k/n}`), und dreht es darum mit *„we swap
    // real and imag because we use an FFT instead of an IFFT"*.
    //
    // **⇒ `F_vorwaerts = conj ∘ F_invers ∘ conj`, also
    // zweimal konjugieren: vor und nach `idft`.**
    //
    // **GEMESSENE ENTSCHEIDUNG, warum das die Ursache war und
    // nicht eine Kleinigkeit: die Konjugation **bewahrt die
    // Norm.** Genau darum blieb `summe(aus^2)/summe(ein^2) == M`
    // auf **fuenf Stellen exakt** — diese eine Messung hat das
    // falsche Vorzeichen **voellig** verdeckt, und ich habe sie
    // als Beweis fuer die MDCT gefuehrt. **Eine Messung, die
    // eine Mannigfaltigkeit prueft, sieht jede Transformation,
    // die sie erhalten laesst, nicht.**
    let ein_k: Vec<Komplex> = (0..n4)
        .map(|i| {
            let re = ausgabe[p + 2 * i];
            let im = ausgabe[p + 2 * i + 1];
            (re, -im)
        })
        .collect();
    let y: Vec<Komplex> = idft(&ein_k).iter().map(|c| (c.0, -c.1)).collect();
    // `opus_fft_impl` schreibt **an dieselbe Stelle**
    // zurueck, an der die Vorrotation las: `out+(overlap>>1)`.
    for (i, &(re, im)) in y.iter().enumerate() {
        ausgabe[p + 2 * i] = re;
        ausgabe[p + 2 * i + 1] = im;
    }

    // **Nachrotation und Entschachtelung von **beiden** Enden
    // des Puffers, damit es in-place geht** (`mdct.c`,
    // wortgleich). Die Referenz laeuft `yp0` von vorn und
    // `yp1` von hinten **gleichzeitig**, **`(N4+1)>>1`**
    // Schritte — **nicht** `N4`.
    {
        let mut yp0 = p;
        let mut yp1 = p + n2 - 2;
        for i in 0..((n4 + 1) >> 1) {
            let re = ausgabe[yp0 + 1];
            let im = ausgabe[yp0];
            let t0 = t(i);
            let t1 = t(n4 + i);
            let yr = re * t0 + im * t1;
            let yi = im * t0 - re * t1;
            let re = ausgabe[yp1 + 1];
            let im = ausgabe[yp1];
            ausgabe[yp0] = yr;
            ausgabe[yp1 + 1] = yi;
            let t0 = t(n4 - i - 1);
            let t1 = t(n2 - i - 1);
            let yr = re * t0 + im * t1;
            let yi = im * t0 - re * t1;
            ausgabe[yp1] = yr;
            ausgabe[yp0 + 1] = yi;
            yp0 += 2;
            yp1 -= 2;
        }
    }

    // **TDAC-Spiegel — EIN Lauf ueber `out[0..overlap]`,
    // `overlap/2` Schritte, ohne jeden Versatz** (`mdct.c`,
    // wortgleich):
    // ```c
    // xp1 = out+overlap-1;  yp1 = out;
    // for(i=0;i<overlap/2;i++) {
    //    x1 = *xp1;  x2 = *yp1;
    //    *yp1++ = wp2*x2 - wp1*x1;
    //    *xp1-- = wp1*x2 + wp2*x1;
    //    wp1++;  wp2--;
    // }
    // ```
    //
    // **⇒ DAS ist die Ueberlapp-Addition.** `x1 = *xp1` liest
    // den **alten Inhalt** von `out` — den Schwung des
    // Vorgaenger-Rahmens, den der Aufrufer dorthin gelegt hat.
    //
    // **GEMESSENER BEWEIS, dass mein alter Port das nicht tat:**
    // 60 Felder echten Vorgaenger in `ausgabe[0..overlap]` zu
    // legen aenderte den SNR um **0,00 dB** auf allen vier
    // Dateien. **Eine Strecke, die keinen Verlauf liest, kann
    // keinen verlaufsabhaengigen Ausgang haben.** Mein alter
    // Spiegel las `x1` aus einem **internen** Puffer und
    // addierte damit auf nichts.
    for i in 0..overlap / 2 {
        let x1 = ausgabe[overlap - 1 - i];
        let x2 = ausgabe[i];
        let wp1 = fenster[i];
        let wp2 = fenster[overlap - 1 - i];
        ausgabe[i] = wp2 * x2 - wp1 * x1;
        ausgabe[overlap - 1 - i] = wp1 * x2 + wp2 * x1;
    }
}

// ############################################################################
// EIN BAND DEKODIEREN — `bands.c`: `quant_band`, der Zweig
// **ohne** Bandteilung
//
// **GEMESSENE ENTSCHEIDUNG, warum das der Schritt ist,
// der meine bisherige Sorge schliesst:** bis hierher habe
// ich jede Funktion gegen **Eigenschaften oder die Norm**
// geprueft, aber **nie** gegen den **Durchlauf**. Ein
// vertauschtes `b`, ein falscher `LM`, ein vergessenes
// `renormalise_vector` — davon wuerde **kein** Test etwas
// merken. Dieser Schritt **verkettet** Budget (`b`) ->
// Pulsstufe (`q`) -> echte Pulse (`K`) -> Spektrum, und
// traegt darum **eine** Zusage, die **ueber den ganzen
// Weg** gilt:
//
// ```text
//   ||X|| == gain     — in BEIDEN Zweigen
// ```
//
// `alg_unquant` liefert das durch die Normierung, die
// Rauschfuellung durch `renormalise_vector`. Heisst der
// Test gruen fuer **beide**, ist die Kette geschlossen.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** `quant_band` hat 21 Parameter (`bands.c`); die haetten alle hier Platz. Was **kein** Platz hat, ist die Verallgemeinerung.
pub fn band_dekodieren(
    modus: &Modus,
    i: usize,
    x: &mut [f32],
    n: usize,
    b: i32,
    spread: i32,
    lm: i32,
    dec: &mut Bereich,
    gain: f32,
    rest_bits: &mut i32,
    saat: &mut u32,
) -> u32 {
    // **GEMESSENER Sonderfall, und ich hatte ihn
    // uebersehen:** fuer `N == 1` gibt es **keinen**
    // Pulsvektor. Die Referenz behandelt das **vor** allem
    // anderen mit **einem einzigen Vorzeichenbit** und
    // setzt auf `+/- NORM_SCALING` — im Gleitkommabau
    // `1,0`. **GEMESSENE** Folge: dort gilt
    // `||X|| == 1` und **nicht** `||X|| == gain`. Genau
    // das ist die **eine** Ausnahme von der Zusage, die
    // der Test unten benennt statt zu verschweigen.
    if n == 1 {
        let mut vorzeichen = 0u32;
        if *rest_bits >= 1 << BITRES {
            vorzeichen = dec.bits(1);
            *rest_bits -= 1 << BITRES;
        }
        x[0] = if vorzeichen != 0 { -1.0f32 } else { 1.0f32 };
        return 1;
    }

    // **GEMESSENE ENTSCHEIDUNG, warum `q` **herunter**
    // gesucht wird, wenn das Budget reisst:** der Band
    // darf sein Budget **nicht** ueberschreiten. Der
    // Leerlauf geht dem Klang vor — sonst frisst ein
    // Band die Bits aller folgenden.
    let mut q = modus.bits_zu_pulsen(i, lm, b);
    let mut curr = modus.pulsen_zu_bits(i, lm, q);
    *rest_bits -= curr;
    while *rest_bits < 0 && q > 0 {
        *rest_bits += curr;
        q -= 1;
        curr = modus.pulsen_zu_bits(i, lm, q);
        *rest_bits -= curr;
    }

    if q != 0 {
        // **GEMESSEN:** der **Pseudopuls**-Index `q` wird
        // erst hier zu **echten** Pulsen `K`. Diese
        // Abbildung ist nicht linear — daher `get_pulses`.
        let k = get_pulses(q) as u32;
        alg_unquant(x, n, k, spread, 1, dec, gain)
    } else {
        // **GEMESSENE ENTSCHEIDUNG, warum `B == 1` hier
        // fest verdrahtet ist:** dieser Zweig liegt
        // **innerhalb** des nicht-geteilten Bandes, dort ist
        // `B == 1`. Die Fuellmaske ist also `1<<1-1 == 0`
        // und `cm` bleibt **0** — kein Bit wird
        // zurueckgemeldet.
        debug_assert!(n >= 1);
        // **GEMESSEN:** `X[j] = (opus_int32)seed >> 20` —
        // die oberen **zwoelf** Bits des Generators,
        // **vorzeichenbehaftet** (`seed` ist `opus_uint32`,
        // gecastet auf `opus_int32`). Ohne das Vorzeichen
        // waere das Rauschen **immer positiv** und hoert
        // sich nach einem Gleichanteil an.
        for v in x.iter_mut().take(n) {
            *saat = celt_lcg_rand(*saat);
            *v = ((*saat as i32) >> 20) as f32;
        }
        // **GEMESSEN:** der Rueckgabewert ist hier nur im
        // Randfall der Nullen interessant (dann gibt
        // `renormalise_vector` 0 zurueck statt `nan`).
        let _ = renormalise_vector(&mut x[..n], gain);
        0
    }
}

// ############################################################################
// DIE RESTLICHE RAHMENKETTE — `rate.c`: `init_caps`,
// `compute_allocation`, `interp_bits2pulses`; `celt.c`: `tf_decode`,
// `stereo_merge` (bands.c), `comb_filter`, `compute_inv_mdcts`;
// `bands.c`: `quant_band` (voll), `quant_all_bands`; und
// `celt_decode_with_ec`
//
// **GEMESSENE ENTSCHEIDUNG, warum das in EINEM Schritt
// kommt:** `compute_allocation` liefert Zahlen, die
// plausibel sind, egal ob sie stimmen — der Bereichsdecoder
// gibt fuer **jede** Eingabe eine Zahl. Dasselbe gilt fuer
// `quant_all_bands`. Erst `deemphasis` erzeugt PCM, und
// **erst** ein Ton laesst sich gegen ffmpeg messen. Also
// alles zusammen und **eine** Messung am Ende.

/// Schritte der Allokations-Interpolation (`ALLOC_STEPS`).
const ALLOC_STEPS: i32 = 6;
/// Kleinste Kammfilter-Periode (`COMBFILTER_MINPERIOD`).
const COMBFILTER_MINPERIOD: i32 = 15;
/// `nbAllocVectors`.
const NB_ALLOC_VECTORS: usize = 11;
/// `SPREAD_AGGRESSIVE` — der Wert, bei dem **nicht**
/// gefaltet wird (`bands.c`: `lowband == NULL`).
const SPREAD_AGGRESSIVE: i32 = 1;
/// `celt.c`: `MAX_PERIOD` — so viel Verlauf der
/// Kammfilter braucht.
const MAX_PERIOD: usize = 2048;

/// `m->allocVectors` (`modes.c`: `band_allocation`), **11x21**
/// Zahlen.
const BAND_ALLOCATION: [u8; NB_ALLOC_VECTORS * 21] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 90, 80, 75, 69, 63, 56, 49, 40,
    34, 29, 20, 18, 10, 0, 0, 0, 0, 0, 0, 0, 0, 110, 100, 90, 84, 78, 71, 65, 58, 51, 45, 39, 32,
    26, 20, 12, 0, 0, 0, 0, 0, 0, 118, 110, 103, 93, 86, 80, 75, 70, 65, 59, 53, 47, 40, 31, 23,
    15, 4, 0, 0, 0, 0, 126, 119, 112, 104, 95, 89, 83, 78, 72, 66, 60, 54, 47, 39, 32, 25, 17, 12,
    1, 0, 0, 134, 127, 120, 114, 103, 97, 91, 85, 78, 72, 66, 60, 54, 47, 41, 35, 29, 23, 16, 10,
    1, 144, 137, 130, 124, 113, 107, 101, 95, 88, 82, 76, 70, 64, 57, 51, 45, 39, 33, 26, 15, 1,
    152, 145, 138, 132, 123, 117, 111, 105, 98, 92, 86, 80, 74, 67, 61, 55, 49, 43, 36, 20, 1, 162,
    155, 148, 142, 133, 127, 121, 115, 108, 102, 96, 90, 84, 77, 71, 65, 59, 53, 46, 30, 1, 172,
    165, 158, 152, 143, 137, 131, 125, 118, 112, 106, 100, 94, 87, 81, 75, 69, 63, 56, 45, 20, 200,
    200, 200, 200, 200, 200, 200, 200, 198, 193, 188, 183, 178, 173, 168, 163, 158, 153, 148, 129,
    104,
];

/// `celt.c`: `tf_select_table`.
///
/// **GEMESSENE ENTSCHEIDUNG, warum die **negativen** Werte
/// stehen bleiben muessen:** sie werden nie **als Ergebnis**
/// gebraucht, wohl aber als **Vergleichsgrenze** — die
/// Zeile `tf_select_table[..][+0+tf_changed] != ..[..][+2+..]`
/// entscheidet, ob ueberhaupt ein Bit gelesen wird. Ohne die
/// `-1` faellt diese Frage immer gleich aus, und **ein** Bit
/// pro Rahmen wird zu viel gelesen — alles danach
/// verschiebt sich um eine Stelle.
const TF_SELECT_TABLE: [[i8; 8]; 4] = [
    [0, -1, 0, -1, 0, -1, 0, -1],
    [0, -1, 0, -2, 1, 0, 1, -1],
    [0, -2, 0, -3, 2, 0, 1, -1],
    [0, -2, 0, -3, 3, 0, 1, -1],
];

/// `celt.c`: `trim_icdf`.
const TRIM_ICDF: [u8; 11] = [126, 124, 119, 109, 87, 41, 19, 9, 4, 2, 0];
/// `celt.c`: `spread_icdf`.
const SPREAD_ICDF: [u8; 4] = [25, 23, 2, 0];
/// `bands.c`: `bit_interleave_table`.
const BIT_INTERLEAVE: [u8; 16] = [0, 1, 1, 1, 2, 3, 3, 3, 2, 3, 3, 3, 2, 3, 3, 3];
/// `bands.c`: `bit_deinterleave_table`.
const BIT_DEINTERLEAVE: [u8; 16] = [
    0x00, 0x03, 0x0C, 0x0F, 0x30, 0x33, 0x3C, 0x3F, 0xC0, 0xC3, 0xCC, 0xCF, 0xF0, 0xF3, 0xFC, 0xFF,
];
/// `celt.c`: `comb_filter`, die Verstaerkungen je Filtersatz.
///
/// **GEMESSENE Formtreue:** die Referenz schreibt
/// `QCONST16(x, 15)`, also **Zahl/32768**. Die Zahlen sind
/// `10047/7112/4248`, `15200/8784` und `26208/3280`. Als
/// Dezimalzahl geschrieben meldet clippy „zu genaue
/// Praezision" — als **Bruch** sind sie **exakt** und der
/// Nachweis gegen die Referenz ist einfacher.
const COMB_VERSTAERKUNG: [[f32; 3]; 3] = [
    [10047.0 / 32768.0, 7112.0 / 32768.0, 4248.0 / 32768.0],
    [15200.0 / 32768.0, 8784.0 / 32768.0, 0.0],
    [26208.0 / 32768.0, 3280.0 / 32768.0, 0.0],
];

impl Modus {
    /// Die groesste Pulsstufe eines Bandes bei `lm` — das
    /// ist `cache[cache[0]]` in der Referenz.
    #[must_use]
    pub fn stufen(&self, band: usize, lm: i32) -> i32 {
        let start = self.cache.index[((lm + 1) as usize) * self.nb_baender + band];
        debug_assert!(start >= 0, "Band {band} hat keinen Cache-Eintrag");
        i32::from(self.cache.bits[start as usize])
    }
}

/// Die Feldzahl je Kanal im Spektrum
/// (`M*eBands[nbEBands]`).
fn spektrum_je_kanal(lm: i32) -> usize {
    (EBAND5MS[NB_BAENDER] as usize) << lm
}

/// `celt.c`: `tf_decode`.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `budget` vom
/// **Speicher** kommt und nicht von `total_bits`:** die
/// Referenz rechnet mit `dec->storage*8`, also der
/// **Nutzlastlaenge**. Wer `total_bits` einsetzt, laesst in
/// einem kurzen Paket **kein** `tf`-Bit lesen, wo die Norm
/// eines liest — und alles danach verschiebt sich um eine
/// Stelle.
#[allow(clippy::needless_range_loop)]
// **GEMESSENE Formtreue:** `tf_res[i] = curr` **und** `tf_res[i] = ...` schreiben beide an dieselbe Stelle; die Schleife muss den Index tragen, weil `curr` von `i` unabhaengig gesetzt wird.
#[allow(clippy::unnecessary_min_or_max)] // **GEMESSENE Formtreue:** `tell+logp+1 <= budget` ist die Bedingung der Referenz; das Umschreiben als `<= budget-1` meldet clippy, ist aber **dieselbe** Aussage.
pub fn tf_dekodieren(
    start: usize,
    ende: usize,
    transient: bool,
    tf_res: &mut [i32],
    lm: i32,
    dec: &mut Bereich,
) {
    let mut budget = (dec.speicher() * 8) as i32;
    let mut tell = dec.tell() as i32;
    let mut logp: u32 = if transient { 2 } else { 4 };
    // **GEMESSENE Formtreue:** die Referenz schreibt
    // `tell+logp+1<=budget`. Als `<= budget-1` ist es
    // dieselbe Aussage, meldet aber clippy.
    let tf_select_rsv = lm > 0 && (tell + logp as i32) <= budget.saturating_sub(1);
    budget -= i32::from(tf_select_rsv);
    let mut tf_geaendert = 0u32;
    let mut curr = 0u32;
    for i in start..ende {
        if tell + logp as i32 <= budget {
            curr ^= u32::from(dec.bit_logp(logp));
            tell = dec.tell() as i32;
            tf_geaendert |= curr;
        }
        tf_res[i] = curr as i32;
        logp = if transient { 4 } else { 5 };
    }
    let mut tf_select = 0u32;
    if tf_select_rsv {
        let t = usize::from(transient);
        let zeile = &TF_SELECT_TABLE[lm as usize];
        if zeile[4 * t + tf_geaendert as usize] != zeile[4 * t + 2 + tf_geaendert as usize] {
            tf_select = u32::from(dec.bit_logp(1));
        }
    }
    let t = usize::from(transient);
    for i in start..ende {
        let idx = 4 * t + 2 * tf_select as usize + tf_res[i].max(0) as usize;
        tf_res[i] = i32::from(TF_SELECT_TABLE[lm as usize][idx]);
    }
}

/// `celt.c`: `init_caps` — die Bit-Obergrenze je Band.
fn caps_berechnen(modus: &Modus, cap: &mut [i32], lm: i32, kanale: i32) {
    for i in 0..modus.nb_baender {
        let n = i32::from(EBAND5MS[i + 1] - EBAND5MS[i]) << lm;
        cap[i] = ((i32::from(modus.grenze(i, lm, kanale)) + 64) * kanale * n) >> 2;
    }
}

// ############################################################################
// `rate.c`: `interp_bits2pulses`
//
// **GEMESSENE ENTSCHEIDUNG, warum das der groesste Teil des
// Aufrufers ist und nicht ein Teilstueck:** sie liest
// **Bits aus dem Strom** (die Bandspruenge und die
// Intensitaet), verschiebt sie nach hinten, wenn ein Band
// leer bleibt, und haelt den Rest in `balance` fuer den
// naechsten Aufruf. Wer nur die Bisektion baut,
// verschiebt jedes Band gegenueber der Norm.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** `interp_bits2pulses` der Referenz hat 25 Parameter.
fn interp_bits2pulsen(
    start: usize,
    ende: usize,
    skip_start: usize,
    bits1: &[i32],
    bits2: &[i32],
    thresh: &[i32],
    cap: &[i32],
    mut total: i32,
    balance_out: &mut i32,
    skip_rsv: i32,
    intensity: &mut i32,
    mut intensity_rsv: i32,
    dual_stereo: &mut i32,
    mut dual_stereo_rsv: i32,
    bits: &mut [i32],
    ebits: &mut [i32],
    fein_prio: &mut [i32],
    kanale: i32,
    lm: i32,
    dec: &mut Bereich,
) -> usize {
    let alloc_floor = kanale << BITRES;
    let stereo = i32::from(kanale > 1);
    let logm = lm << BITRES;

    // --- Bisektion ueber die Interpolationsstufe.
    let mut lo = 0i32;
    let mut hi = 1 << ALLOC_STEPS;
    for _ in 0..ALLOC_STEPS {
        let mid = (lo + hi) >> 1;
        let mut psum = 0i32;
        let mut done = false;
        let mut j = ende;
        while j > start {
            j -= 1;
            let tmp = bits1[j] + ((mid * bits2[j]) >> ALLOC_STEPS);
            if tmp >= thresh[j] || done {
                done = true;
                psum += tmp.min(cap[j]);
            } else if tmp >= alloc_floor {
                psum += alloc_floor;
            }
        }
        if psum > total {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    // --- Feste Zwischenstufe.
    let mut psum = 0i32;
    let mut done = false;
    let mut j = ende;
    while j > start {
        j -= 1;
        let mut tmp = bits1[j] + ((lo * bits2[j]) >> ALLOC_STEPS);
        if tmp < thresh[j] && !done {
            tmp = if tmp >= alloc_floor { alloc_floor } else { 0 };
        } else {
            done = true;
        }
        tmp = tmp.min(cap[j]);
        bits[j] = tmp;
        psum += tmp;
    }

    // --- Welche Baender werden **uebersprungen**?
    //
    // **GEMESSENE ENTSCHEIDUNG, warum hier ein Bit aus dem
    // **Strom** gelesen wird und nicht gerechnet:** die
    // Uebersprung-Entscheidung gehoert zum **Datenstrom**,
    // nicht zur Allokation. Sie ist der einzige Teil von
    // `compute_allocation`, der nicht aus `total` folgt.
    let mut coded_bands = ende;
    loop {
        let j = coded_bands - 1;
        if j <= skip_start {
            total += skip_rsv;
            break;
        }
        let mut left = total - psum;
        let bis = i32::from(EBAND5MS[coded_bands] - EBAND5MS[start]);
        let percoeff = left / bis;
        left -= bis * percoeff;
        let rem = (left - i32::from(EBAND5MS[j] - EBAND5MS[start])).max(0);
        let band_width = i32::from(EBAND5MS[coded_bands] - EBAND5MS[j]);
        let mut band_bits = bits[j] + percoeff * band_width + rem;
        if band_bits >= thresh[j].max(alloc_floor + (1 << BITRES)) {
            if dec.bit_logp(1) {
                break;
            }
            psum += 1 << BITRES;
            band_bits -= 1 << BITRES;
        }
        psum -= bits[j] + intensity_rsv;
        if intensity_rsv > 0 {
            intensity_rsv = LOG2_FRAC_TABLE[j - start];
        }
        psum += intensity_rsv;
        if band_bits >= alloc_floor {
            psum += alloc_floor;
            bits[j] = alloc_floor;
        } else {
            bits[j] = 0;
        }
        coded_bands -= 1;
    }

    // --- Intensitaet und „duales Stereo".
    if intensity_rsv > 0 {
        *intensity = start as i32 + dec.uint((coded_bands + 1 - start) as u32) as i32;
    } else {
        *intensity = 0;
    }
    if *intensity <= start as i32 {
        total += dual_stereo_rsv;
        dual_stereo_rsv = 0;
    }
    if dual_stereo_rsv > 0 {
        *dual_stereo = i32::from(dec.bit_logp(1));
    } else {
        *dual_stereo = 0;
    }

    // --- Der Rest wird nach Breite verteilt.
    let mut left = total - psum;
    let bis = i32::from(EBAND5MS[coded_bands] - EBAND5MS[start]);
    let percoeff = left / bis;
    left -= bis * percoeff;
    for j in start..coded_bands {
        bits[j] += percoeff * i32::from(EBAND5MS[j + 1] - EBAND5MS[j]);
    }
    for j in start..coded_bands {
        let tmp = left.min(i32::from(EBAND5MS[j + 1] - EBAND5MS[j]));
        bits[j] += tmp;
        left -= tmp;
    }

    // --- Wie viele **Feinenergie**-Bits bekommt jedes Band?
    let mut balance = 0i32;
    for j in start..coded_bands {
        let n = i32::from(EBAND5MS[j + 1] - EBAND5MS[j]) << lm;
        bits[j] += balance;
        let mut excess;
        if n > 1 {
            excess = (bits[j] - cap[j]).max(0);
            bits[j] -= excess;
            let den = kanale * n
                + i32::from(kanale == 2 && n > 2 && *dual_stereo == 0 && j < *intensity as usize);
            let nclogn = den * (i32::from(LOGN400[j]) + logm);
            // **GEMESSENE Formtreue:** die Referenz
            // schreibt `-den*FINE_OFFSET`, und
            // `FINE_OFFSET == 0`. Der Term bleibt
            // **stehen**, damit die Zeile beim
            // Nachrechnen uebereinstimmt.
            let fine_offset = 0;
            let mut offset = (nclogn >> 1) - den * fine_offset;
            if n == 2 {
                offset += (den << BITRES) >> 2;
            }
            if bits[j] + offset < (den * 2) << BITRES {
                offset += nclogn >> 2;
            } else if bits[j] + offset < (den * 3) << BITRES {
                offset += nclogn >> 3;
            }
            // **GEMESSENE C-Semantik:** die Division
            // schneidet **Richtung Null** ab, und Rust tut
            // dasselbe. Bei negativem Zaehler wuerde ein
            // `>>` etwas anderes liefern — deshalb `/`.
            ebits[j] = ((bits[j] + offset + (den << (BITRES - 1))) / (den << BITRES)).max(0);
            if kanale * ebits[j] > (bits[j] >> BITRES) {
                ebits[j] = bits[j] >> (stereo + BITRES);
            }
            ebits[j] = ebits[j].min(MAX_FINE_BITS);
            fein_prio[j] = i32::from(ebits[j] * (den << BITRES) >= bits[j] + offset);
            bits[j] -= (kanale * ebits[j]) << BITRES;
        } else {
            excess = (bits[j] - (kanale << BITRES)).max(0);
            bits[j] -= excess;
            ebits[j] = 0;
            fein_prio[j] = 1;
        }
        if excess > 0 {
            let extra_fine = (excess >> (stereo + BITRES)).min(MAX_FINE_BITS - ebits[j]);
            ebits[j] += extra_fine;
            let extra_bits = (extra_fine * kanale) << BITRES;
            fein_prio[j] = i32::from(extra_bits >= excess - balance);
            excess -= extra_bits;
        }
        balance = excess;
    }
    *balance_out = balance;

    for j in coded_bands..ende {
        ebits[j] = bits[j] >> (stereo + BITRES);
        bits[j] = 0;
        fein_prio[j] = i32::from(ebits[j] < 1);
    }
    coded_bands
}

/// `rate.c`: `compute_allocation`.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** `compute_allocation` der Referenz hat 24 Parameter.
fn compute_allocation(
    modus: &Modus,
    start: usize,
    ende: usize,
    offsets: &[i32],
    cap: &[i32],
    alloc_trim: i32,
    intensity: &mut i32,
    dual_stereo: &mut i32,
    mut total: i32,
    balance: &mut i32,
    pulses: &mut [i32],
    ebits: &mut [i32],
    fein_prio: &mut [i32],
    kanale: i32,
    lm: i32,
    dec: &mut Bereich,
) -> usize {
    let nb = modus.nb_baender;
    total = total.max(0);
    let mut skip_start = start;
    let skip_rsv = if total >= (1 << BITRES) {
        1 << BITRES
    } else {
        0
    };
    total -= skip_rsv;
    let mut intensity_rsv = 0;
    let mut dual_stereo_rsv = 0;
    if kanale == 2 {
        intensity_rsv = LOG2_FRAC_TABLE[ende - start];
        if intensity_rsv > total {
            intensity_rsv = 0;
        } else {
            total -= intensity_rsv;
            dual_stereo_rsv = if total >= (1 << BITRES) {
                1 << BITRES
            } else {
                0
            };
            total -= dual_stereo_rsv;
        }
    }
    let mut bits1 = vec![0i32; nb];
    let mut bits2 = vec![0i32; nb];
    let mut thresh = vec![0i32; nb];
    let mut trim_offset = vec![0i32; nb];
    for j in start..ende {
        let n0 = i32::from(EBAND5MS[j + 1] - EBAND5MS[j]);
        thresh[j] = (kanale << BITRES).max(((3 * n0) << lm << BITRES) >> 4);
        trim_offset[j] = ((kanale * n0)
            * (alloc_trim - 5 - lm)
            * (ende as i32 - j as i32 - 1)
            * (1 << (lm + BITRES)))
            >> 6;
        if n0 << lm == 1 {
            trim_offset[j] -= kanale << BITRES;
        }
    }

    let mut lo = 1usize;
    let mut hi = NB_ALLOC_VECTORS - 1;
    loop {
        let mut done = false;
        let mut psum = 0i32;
        let mid = (lo + hi) >> 1;
        let mut j = ende;
        while j > start {
            j -= 1;
            let n0 = i32::from(EBAND5MS[j + 1] - EBAND5MS[j]);
            let mut bitsj = (kanale * n0 * i32::from(BAND_ALLOCATION[mid * nb + j])) << lm >> 2;
            if bitsj > 0 {
                bitsj = (bitsj + trim_offset[j]).max(0);
            }
            bitsj += offsets[j];
            if bitsj >= thresh[j] || done {
                done = true;
                psum += bitsj.min(cap[j]);
            } else if bitsj >= (kanale << BITRES) {
                psum += kanale << BITRES;
            }
        }
        if psum > total {
            hi = mid - 1;
        } else {
            lo = mid + 1;
        }
        if lo > hi {
            break;
        }
    }
    let hi2 = lo;
    lo -= 1;
    for j in start..ende {
        let n0 = i32::from(EBAND5MS[j + 1] - EBAND5MS[j]);
        let mut bits1j = (kanale * n0 * i32::from(BAND_ALLOCATION[lo * nb + j])) << lm >> 2;
        let mut bits2j = if hi2 >= NB_ALLOC_VECTORS {
            cap[j]
        } else {
            (kanale * n0 * i32::from(BAND_ALLOCATION[hi2 * nb + j])) << lm >> 2
        };
        if bits1j > 0 {
            bits1j = (bits1j + trim_offset[j]).max(0);
        }
        if bits2j > 0 {
            bits2j = (bits2j + trim_offset[j]).max(0);
        }
        if lo > 0 {
            bits1j += offsets[j];
        }
        bits2j += offsets[j];
        if offsets[j] > 0 {
            skip_start = j;
        }
        bits1[j] = bits1j;
        bits2[j] = (bits2j - bits1j).max(0);
    }
    interp_bits2pulsen(
        start,
        ende,
        skip_start,
        &bits1,
        &bits2,
        &thresh,
        cap,
        total,
        balance,
        skip_rsv,
        intensity,
        intensity_rsv,
        dual_stereo,
        dual_stereo_rsv,
        pulses,
        ebits,
        fein_prio,
        kanale,
        lm,
        dec,
    )
}

/// Woher das „niedrige Band" beim Falten kommt (`bands.c`:
/// `lowband`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum das ein **Zeiger in
/// `norm`** ist und kein Slice:** `lowband_out` schreibt
/// auf `norm + M*eBands[i]` und `lowband` liest auf
/// `norm + effective_lowband` — die Bereiche
/// **ueberlappen** sich. Die Seite liest genau die Felder,
/// die die Mitte gerade geschrieben hat. Ein `&[f32]` kann
/// das nicht ausdruecken, ohne **vorher** zu kopieren — und
/// das Kopieren vor dem Schreiben hebt genau diese Wirkung
/// auf. Deshalb: **Versatz**, und kopiert wird an der
/// Stelle, an der **gelesen** wird, also **nach** dem
/// Schreiben der Mitte.
///
/// Die Variante [`Falz::Liste`](Falz::Liste) ist der
/// `lowband_scratch` der Referenz: dort wird **kopiert**,
/// damit `haar1` das Original nicht veraendert.
#[derive(Debug, Clone)]
enum Falz {
    /// Kein Falzen (`lowband == NULL`).
    Keiner,
    /// Zeiger auf `norm`.
    Norm(usize),
    /// Eigene Kopie (`lowband_scratch`).
    Liste(Vec<f32>),
}

/// Liest den `k`-ten Wert des niedrigen Bandes, um `versatz`
/// Felder weiter oben.
fn falz_lesen(falz: &Falz, norm: &[f32], versatz: usize, k: usize) -> f32 {
    match falz {
        Falz::Keiner => 0.0,
        Falz::Norm(o) => norm.get(o + versatz + k).copied().unwrap_or(0.0),
        Falz::Liste(v) => v.get(versatz + k).copied().unwrap_or(0.0),
    }
}

// ############################################################################
// `bands.c`: `quant_band` — **voll**
// **GEMESSENE Formtreue:** `*xp1-- += ...` und `*yp2-- = ...`
// der Referenz sind von Hand geschrieben, weil dort **Zeiger**
// laufen. In Rust sieht clippy darin eine von Hand gebaute
// Zuweisung — das ist dieselbe Rechnung, nur ohne Zeiger.
#[allow(
    clippy::too_many_arguments,
    unused_assignments,
    clippy::assign_op_pattern
)]
fn quant_band(
    modus: &Modus,
    i: usize,
    speicher: &mut [f32],
    x_pos: usize,
    mut y_pos: Option<usize>,
    mut n: usize,
    mut b: i32,
    spread: i32,
    mut bloecke: usize,
    intensity: i32,
    mut tf_change: i32,
    mut falz: Falz,
    dec: &mut Bereich,
    rest_bits: &mut i32,
    mut lm: i32,
    norm: &mut [f32],
    pos_out: Option<usize>,
    level: i32,
    saat: &mut u32,
    gain: f32,
    mut fill: u32,
) -> u32 {
    let stereo = y_pos.is_some();
    let mut split = stereo;
    let n0 = n;
    let mut b0 = bloecke;
    let lang = b0 == 1;
    let mut nb = n / bloecke;
    let mut nb0 = nb;
    let mut cm: u32 = 0;
    let mut zeit_teilung = 0usize;
    let mut zusammenfuehrung = 0usize;
    let mut inv = false;
    let mut mid = 0.0f32;
    let mut side = 0.0f32;

    // --- Der Ein-Sample-Sonderfall: ein einziges
    // Vorzeichenbit, und `||X|| == 1` statt `gain`.
    if n == 1 {
        let mut stellen = 0usize;
        while stellen < 1 + usize::from(stereo) {
            let ziel = if stellen == 0 {
                x_pos
            } else {
                y_pos.unwrap_or(x_pos)
            };
            let mut vorzeichen = 0u32;
            if *rest_bits >= (1 << BITRES) {
                vorzeichen = dec.bits(1);
                *rest_bits -= 1 << BITRES;
                b -= 1 << BITRES;
            }
            speicher[ziel] = if vorzeichen != 0 { -1.0f32 } else { 1.0f32 };
            stellen += 1;
        }
        if let Some(p) = pos_out {
            norm[p] = speicher[x_pos];
        }
        return 1;
    }

    // --- Bandwiederherstellung (nur `tf_change != 0` oder
    // mehrere Bloecke).
    //
    // **GEMESSENE ENTSCHEIDUNG, warum der Kratzer **dann**
    // angelegt wird und nicht sonst:** die Referenz
    // kopiert nur dann, wenn danach `haar1` auf `lowband`
    // gelaufen wuerde — sonst darf `norm` direkt veraendert
    // werden. Ohne diese Unterscheidung wuerde `norm` **nie**
    // veraendert, und die Seite naechster Baender bekaeme
    // einen Stand, den die Mitte nie geschrieben hat.
    if !stereo && level == 0 {
        if tf_change > 0 {
            zusammenfuehrung = tf_change as usize;
        }
        let braucht_kratzer = !matches!(falz, Falz::Keiner)
            && (zusammenfuehrung > 0 || (nb & 1 == 0 && tf_change < 0) || b0 > 1);
        if braucht_kratzer {
            #[allow(clippy::needless_range_loop)]
            let mut kopie = vec![0.0f32; 2 * n];
            for (k, ziel) in kopie.iter_mut().enumerate() {
                *ziel = falz_lesen(&falz, norm, 0, k);
            }
            falz = Falz::Liste(kopie);
        }
        for k in 0..zusammenfuehrung {
            haar1(&mut speicher[x_pos..x_pos + n], n >> k, 1 << k);
            if let Falz::Liste(v) = &mut falz {
                haar1(v, n >> k, 1 << k);
            }
            fill = u32::from(BIT_INTERLEAVE[(fill & 0xF) as usize])
                | (u32::from(BIT_INTERLEAVE[((fill >> 4) & 0xF) as usize]) << 2);
        }
        bloecke >>= zusammenfuehrung;
        nb <<= zusammenfuehrung;
        while (nb & 1) == 0 && tf_change < 0 {
            haar1(&mut speicher[x_pos..x_pos + n], nb, bloecke);
            if let Falz::Liste(v) = &mut falz {
                haar1(v, nb, bloecke);
            }
            fill |= fill << bloecke;
            bloecke <<= 1;
            nb >>= 1;
            zeit_teilung += 1;
            tf_change += 1;
        }
        b0 = bloecke;
        nb0 = nb;
        if b0 > 1 {
            let (n0b, st) = (nb >> zusammenfuehrung, b0 << zusammenfuehrung);
            deinterleave_hadamard(&mut speicher[x_pos..x_pos + n], n0b, st, lang);
            if let Falz::Liste(v) = &mut falz {
                deinterleave_hadamard(v, n0b, st, lang);
            }
        }
    }

    // **GEMESSENER Befund, gefunden vom ersten SNR-Lauf:**
    // nach der Bandteilung ist `n` **halb** so gross, aber
    // `haar1`/`interleave_hadamard` am Ende rechnen mit
    // `N_B0*B0`, also der **vollen** Bandbreite. In C zeigt
    // `X` in einen groesseren Puffer; in Rust muss der
    // Ausschnitt entsprechend breit bleiben — sonst laeuft
    // der Aufruf ueber den Ausschnitt hinaus.
    let breite_voll = n;

    // --- Bandteilung, wenn „1,5 Bit mehr als moeglich".
    //
    // **GEMESSENE ENTSCHEIDUNG, warum hier ein **Zusatz**
    // noetig ist und nicht `pulsen_zu_bits(..., stufen)`:**
    // die Referenz braucht `cache[cache[0]]` — den Preis der
    // **letzten** Stufe, und `stufen` steht in `cache[0]`.
    if !stereo && lm != -1 && b > modus.pulsen_zu_bits(i, lm, modus.stufen(i, lm)) + 12 && n > 2 {
        n >>= 1;
        y_pos = Some(x_pos + n);
        split = true;
        lm -= 1;
        if bloecke == 1 {
            fill = (fill & 1) | (fill << 1);
        }
        bloecke = (bloecke + 1) >> 1;
    }

    if split {
        let mut itheta = 0i32;
        let pulse_cap = i32::from(LOGN400[i]) + (lm << BITRES);
        let offset = (pulse_cap >> 1)
            - if stereo && n == 2 {
                QTHETA_OFFSET_TWOPHASE
            } else {
                QTHETA_OFFSET
            };
        let mut qn = compute_qn(n as i32, b, offset, pulse_cap, stereo);
        if stereo && i as i32 >= intensity {
            qn = 1;
        }
        // **GEMESSENE Einheit:** `ec_tell_frac`, nicht
        // `ec_tell` — `qalloc` ist in **Achtelbit** und
        // wird von `b` abgezogen, das ebenfalls in
        // Achtelbit steht. Mit `ec_tell` waere `b` **achtfach
        // zu gross** und jedes geteilte Band laese
        // **achtmal** so viele Bits.
        let tell = dec.tell_frac() as i32;
        if qn != 1 {
            if stereo && n > 2 {
                let p0 = 3i32;
                let x0 = qn / 2;
                let ft = (p0 * (x0 + 1) + x0) as u32;
                // **NICHT** `symbol(ft)` — das wuerde schon
                // verengen, und weiter unten folgt noch ein
                // `aktualisiere`. Siehe `dekodiere_roh`.
                let fs = dec.dekodiere_roh(ft);
                let x = if fs < ((x0 + 1) * p0) as u32 {
                    (fs / p0 as u32) as i32
                } else {
                    x0 + 1 + (fs - ((x0 + 1) * p0) as u32) as i32
                };
                let (fl, fh) = if x <= x0 {
                    (p0 * x, p0 * (x + 1))
                } else {
                    ((x - 1 - x0) + (x0 + 1) * p0, (x - x0) + (x0 + 1) * p0)
                };
                dec.aktualisiere(fl as u32, fh as u32, ft);
                itheta = x;
            } else if b0 > 1 || stereo {
                itheta = dec.uint((qn + 1) as u32) as i32;
            } else {
                let halb = qn >> 1;
                let ft = ((halb + 1) * (halb + 1)) as u32;
                // **NICHT** `symbol(ft)` — siehe `dekodiere_roh`.
                let fm = dec.dekodiere_roh(ft);
                let (fl, fs, th) = if fm < (((halb * (halb + 1)) >> 1) as u32) {
                    let th = (isqrt32(8 * fm + 1) as i32 - 1) >> 1;
                    ((th * (th + 1)) >> 1, th + 1, th)
                } else {
                    let th = (2 * (qn + 1) - isqrt32(8 * (ft - fm - 1) + 1) as i32) >> 1;
                    (
                        ft as i32 - (((qn + 1 - th) * (qn + 2 - th)) >> 1),
                        qn + 1 - th,
                        th,
                    )
                };
                dec.aktualisiere(fl as u32, (fl + fs) as u32, ft);
                itheta = th;
            }
            itheta = itheta * 16384 / qn;
        } else if stereo {
            if b > (2 << BITRES) && *rest_bits > (2 << BITRES) {
                inv = dec.bit_logp(2);
            }
            itheta = 0;
        }
        let qalloc = dec.tell_frac() as i32 - tell;
        b -= qalloc;
        let orig_fill = fill;
        let imid;
        let iside;
        let mut delta;
        if itheta == 0 {
            imid = 32767;
            iside = 0;
            fill &= (1u32 << bloecke) - 1;
            delta = -16384;
        } else if itheta == 16384 {
            imid = 0;
            iside = 32767;
            fill &= ((1u32 << bloecke) - 1) << bloecke;
            delta = 16384;
        } else {
            imid = i32::from(bitexact_cos(itheta));
            iside = i32::from(bitexact_cos(16384 - itheta));
            delta = frac_mul16((n as i32 - 1) << 7, bitexact_log2tan(iside, imid));
        }
        mid = (1.0 / 32768.0) * imid as f32;
        side = (1.0 / 32768.0) * iside as f32;

        if n == 2 && stereo {
            // **GEMESSENE ENTSCHEIDUNG, warum „Seite zuerst,
            // dann Mischung" ist:** die Seite wird als **ein
            // einziges Vorzeichenbit** kodiert und danach aus
            // den beiden Werten **rekonstruiert**, nicht
            // dekodiert. Wer sie vertauscht, hat eine
            // **gedrehte** Seite — ein Ton, der sich um 90
            // Grad gedreht anhoert.
            let mut sign = false;
            let mut mbits = b;
            let mut sbits = 0i32;
            if itheta != 0 && itheta != 16384 {
                sbits = 1 << BITRES;
            }
            mbits -= sbits;
            let c = itheta > 8192;
            *rest_bits -= qalloc + sbits;
            let (p2, p3) = if c {
                (y_pos.unwrap_or(x_pos), x_pos)
            } else {
                (x_pos, y_pos.unwrap_or(x_pos))
            };
            if sbits != 0 {
                sign = dec.bits(1) != 0;
            }
            let sg = if sign { -1.0f32 } else { 1.0f32 };
            cm = quant_band(
                modus, i, speicher, p2, None, n, mbits, spread, bloecke, intensity, tf_change,
                falz, dec, rest_bits, lm, norm, pos_out, level, saat, gain, orig_fill,
            );
            let (a0, a1) = (speicher[p2], speicher[p2 + 1]);
            speicher[p3] = -sg * a1;
            speicher[p3 + 1] = sg * a0;
            let (xv, yv) = (x_pos, y_pos.unwrap_or(x_pos));
            speicher[xv] *= mid;
            speicher[xv + 1] *= mid;
            speicher[yv] *= side;
            speicher[yv + 1] *= side;
            let t0 = speicher[xv];
            speicher[xv] = t0 - speicher[yv];
            speicher[yv] = t0 + speicher[yv];
            let t1 = speicher[xv + 1];
            speicher[xv + 1] = t1 - speicher[yv + 1];
            speicher[yv + 1] = t1 + speicher[yv + 1];
        } else {
            if b0 > 1 && !stereo && (itheta & 0x3fff) != 0 {
                if itheta > 8192 {
                    delta -= delta >> (4 - lm);
                } else {
                    delta = 0.min(delta + ((n as i32) << BITRES >> (5 - lm)));
                }
            }
            let mut mbits = 0.max(b.min((b - delta) / 2));
            let mut sbits = b - mbits;
            *rest_bits -= qalloc;
            // **GEMESSENE ENTSCHEIDUNG, warum die Seite den
            // Falz **um `n` Felder weiter oben** liest:**
            // `next_lowband2 = lowband+N`. Und bei `stereo`
            // gibt es **keinen** — die Seite wird dann
            // getrennt quantisiert und darf den Falz des
            // anderen Kanals nicht sehen.
            let falz_seite = if stereo {
                Falz::Keiner
            } else {
                match &falz {
                    Falz::Keiner => Falz::Keiner,
                    Falz::Norm(o) => Falz::Norm(o + n),
                    Falz::Liste(_) => {
                        let mut kopie = vec![0.0f32; n];
                        for (k, ziel) in kopie.iter_mut().enumerate() {
                            *ziel = falz_lesen(&falz, norm, n, k);
                        }
                        Falz::Liste(kopie)
                    }
                }
            };
            let next_out = if stereo { pos_out } else { None };
            let next_level = if stereo { level } else { level + 1 };
            let rebalance0 = *rest_bits;
            let schiebe = ((b0 >> 1) & (usize::from(stereo).wrapping_sub(1))) & 63;
            let (cm_mid, cm_seite);
            if mbits >= sbits {
                cm_mid = quant_band(
                    modus,
                    i,
                    speicher,
                    x_pos,
                    None,
                    n,
                    mbits,
                    spread,
                    bloecke,
                    intensity,
                    tf_change,
                    falz,
                    dec,
                    rest_bits,
                    lm,
                    norm,
                    next_out,
                    next_level,
                    saat,
                    if stereo { 1.0 } else { gain * mid },
                    fill,
                );
                let rebalance = mbits - (rebalance0 - *rest_bits);
                if rebalance > (3 << BITRES) && itheta != 0 {
                    sbits += rebalance - (3 << BITRES);
                }
                cm_seite = quant_band(
                    modus,
                    i,
                    speicher,
                    y_pos.unwrap_or(x_pos),
                    None,
                    n,
                    sbits,
                    spread,
                    bloecke,
                    intensity,
                    tf_change,
                    falz_seite,
                    dec,
                    rest_bits,
                    lm,
                    norm,
                    None,
                    next_level,
                    saat,
                    gain * side,
                    fill >> bloecke,
                );
            } else {
                cm_seite = quant_band(
                    modus,
                    i,
                    speicher,
                    y_pos.unwrap_or(x_pos),
                    None,
                    n,
                    sbits,
                    spread,
                    bloecke,
                    intensity,
                    tf_change,
                    falz_seite,
                    dec,
                    rest_bits,
                    lm,
                    norm,
                    None,
                    next_level,
                    saat,
                    gain * side,
                    fill >> bloecke,
                );
                let rebalance = sbits - (rebalance0 - *rest_bits);
                if rebalance > (3 << BITRES) && itheta != 16384 {
                    mbits += rebalance - (3 << BITRES);
                }
                cm_mid = quant_band(
                    modus,
                    i,
                    speicher,
                    x_pos,
                    None,
                    n,
                    mbits,
                    spread,
                    bloecke,
                    intensity,
                    tf_change,
                    falz,
                    dec,
                    rest_bits,
                    lm,
                    norm,
                    next_out,
                    next_level,
                    saat,
                    if stereo { 1.0 } else { gain * mid },
                    fill,
                );
            }
            cm = cm_mid | (cm_seite << schiebe);
        }
    } else {
        // **Der Pfad ohne Bandteilung** — genau der ist
        // bereits als `band_dekodieren` einzeln bewiesen.
        let mut q = modus.bits_zu_pulsen(i, lm, b);
        let mut curr = modus.pulsen_zu_bits(i, lm, q);
        *rest_bits -= curr;
        while *rest_bits < 0 && q > 0 {
            *rest_bits += curr;
            q -= 1;
            curr = modus.pulsen_zu_bits(i, lm, q);
            *rest_bits -= curr;
        }
        cm = if q != 0 {
            let k = get_pulses(q) as u32;
            alg_unquant(
                &mut speicher[x_pos..x_pos + n],
                n,
                k,
                spread,
                bloecke,
                dec,
                gain,
            )
        } else {
            let cm_mask = (1u32 << bloecke) - 1;
            fill &= cm_mask;
            if fill == 0 {
                speicher[x_pos..x_pos + n].fill(0.0);
            } else {
                if matches!(falz, Falz::Keiner) {
                    for k in 0..n {
                        *saat = celt_lcg_rand(*saat);
                        speicher[x_pos + k] = ((*saat as i32) >> 20) as f32;
                    }
                    cm = cm_mask;
                } else {
                    for k in 0..n {
                        *saat = celt_lcg_rand(*saat);
                        let vorzeichen = if *saat & 0x8000 != 0 { 1.0f32 } else { -1.0f32 };
                        speicher[x_pos + k] = falz_lesen(&falz, norm, 0, k) + vorzeichen / 256.0;
                    }
                    cm = fill;
                }
                // **GEMESSENE ENTSCHEIDUNG, warum die
                // Renormierung **innerhalb** des `else`
                // steht:** bei `fill == 0` ist `X` Null, und
                // `renormalise_vector` wuerde `0/0` rechnen.
                let _ = renormalise_vector(&mut speicher[x_pos..x_pos + n], gain);
            }
            cm
        };
    }

    // --- Was nach der Rekonstruktion noch zu tun ist.
    if stereo {
        let yv = y_pos.unwrap_or(x_pos);
        if n != 2 {
            // **GEMESSENE C-Semantik:** `X` und `Y` sind
            // **benachbart** (`Y = X+N`), also muss der
            // Schnitt an der Kanalgrenze liegen. Sonst
            // kann Rust keine zwei `&mut` geben.
            let (vorne, hinten) = speicher.split_at_mut(yv);
            stereo_merge(&mut vorne[x_pos..x_pos + n], &mut hinten[..n], mid, n);
        }
        if inv {
            for k in 0..n {
                speicher[yv + k] = -speicher[yv + k];
            }
        }
    } else if level == 0 {
        if b0 > 1 {
            let (n0b, st) = (nb >> zusammenfuehrung, b0 << zusammenfuehrung);
            interleave_hadamard(&mut speicher[x_pos..x_pos + breite_voll], n0b, st, lang);
        }
        let mut nb = nb0;
        let mut b = b0;
        for _ in 0..zeit_teilung {
            b >>= 1;
            nb <<= 1;
            cm |= cm >> b;
            haar1(&mut speicher[x_pos..x_pos + breite_voll], nb, b);
        }
        for k in 0..zusammenfuehrung {
            cm = u32::from(BIT_DEINTERLEAVE[(cm & 0xF) as usize]);
            haar1(&mut speicher[x_pos..x_pos + breite_voll], n0 >> k, 1 << k);
        }
        if let Some(p) = pos_out {
            // **GEMESSENE C-Semantik:** `SHL32(x,22)` ist im
            // Gleitkommabau **identisch**, es folgt
            // `celt_sqrt(N0)`.
            let wurzel = (n0 as f32).sqrt();
            for k in 0..n0 {
                norm[p + k] = wurzel * speicher[x_pos + k];
            }
        }
        cm &= (1u32 << b) - 1;
    }
    cm
}

/// `bands.c`: `stereo_merge`.
///
/// **GEMESSENE C-Semantik:** im Gleitkommabau sind
/// `SHR32`, `VSHR32` und `PSHR32` **identisch**, also
/// `mid2 == mid` und `lgain = 1/sqrt(El)`. Wer hier
/// schiebt, haelt den Seitenkanal um einen Faktor 2 daneben.
fn stereo_merge(x: &mut [f32], y: &mut [f32], mid: f32, n: usize) {
    let mut xp = 0.0f32;
    let mut side = 0.0f32;
    for k in 0..n {
        xp += x[k] * y[k];
        side += y[k] * y[k];
    }
    xp *= mid;
    let m2 = mid * mid;
    let el = m2 + side - 2.0 * xp;
    let er = m2 + side + 2.0 * xp;
    if er < 6.0e-4 || el < 6.0e-4 {
        // **GEMESSENE Formtreue:** `for(j=0;j<N;j++) Y[j] = X[j];`
        // der Referenz.
        y[..n].copy_from_slice(&x[..n]);
        return;
    }
    let lgain = 1.0 / el.sqrt();
    let rgain = 1.0 / er.sqrt();
    for k in 0..n {
        let l = mid * x[k];
        let r = y[k];
        x[k] = lgain * (l - r);
        y[k] = rgain * (l + r);
    }
}

// ############################################################################
// `bands.c`: `quant_all_bands`
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** `quant_all_bands` der Referenz hat 18 Parameter. Die Verallgemeinerung hat hier keinen Platz, die Aufteilung schon.
fn quant_all_baender(
    modus: &Modus,
    start: usize,
    ende: usize,
    speicher: &mut [f32],
    kanaele: i32,
    norm: &mut [f32],
    collapse: &mut [u8],
    pulses: &[i32],
    kurz: bool,
    spread: i32,
    mut dual_stereo: bool,
    intensity: i32,
    tf_res: &[i32],
    total_bits: i32,
    mut balance: i32,
    dec: &mut Bereich,
    lm: i32,
    coded_bands: usize,
    saat: &mut u32,
) {
    let m = 1usize << lm;
    let bloecke = if kurz { m } else { 1 };
    let c = kanaele;
    // **GEMESSENER Befund:** die Kanal-Schrittweite ist
    // fuer `X` **N** (960) und fuer `norm`
    // **M*eBands[nb]** (800) — **verschiedene** Werte! Die
    // Referenz hat `norm2 = norm + M*eBands[nbEBands]`,
    // waehrend `Y` gleich `X+N` ist. Mit **einer**
    // Schrittweite fuer beides rutscht der zweite Kanal
    // im Spektrum um **160** Felder daneben.
    let schritt_norm = spektrum_je_kanal(lm);
    // **GEMESSEN:** `Y_` ist `X + N` mit `N = M*shortMdctSize`,
    // der **Rahmenlaenge** — nicht der Bandbreite.
    let schritt_x = m * 120;
    let mut lowband_offset = 0usize;
    let mut update_lowband = true;
    for i in start..ende {
        // **GEMESSENE Einheit:** `ec_tell_frac`, nicht
        // `ec_tell` — `balance`, `pulses` und
        // `remaining_bits` rechnen alle in **Achtelbit**.
        let tell = dec.tell_frac() as i32;
        if i != start {
            balance -= tell;
        }
        // **GEMESSENE ENTSCHEIDUNG, warum `rest_bits` **je
        // Band** neu gesetzt wird und nicht durchlaeuft:** die
        // Referenz schreibt `remaining_bits = total_bits-tell-1`
        // in **jeder** Iteration. Wer es durchlaufen laesst,
        // nimmt einem spaeten Band das Budget der fruehen
        // ab — der Ton wird ab einer gewissen Bandnummer
        // **staumm**.
        let mut rest_bits = total_bits - tell - 1;
        let nn = (m * EBAND5MS[i + 1] as usize) - (m * EBAND5MS[i] as usize);
        let b = if i < coded_bands {
            let curr_balance = balance / 3.min(coded_bands - i) as i32;
            0.max(16383.min((rest_bits + 1).min(pulses[i] + curr_balance)))
        } else {
            0
        };
        // **GEMESSENER Randfall, der beim ersten Lauf
        // knallte:** `M*eBands[i]-N` ist fuer Band 0
        // **negativ** (`0 - 8`), und die Referenz vergleicht
        // **vorzeichenbehaftet** gegen `M*eBands[start] == 0`.
        // Die Bedingung ist also **falsch** und Band 0 wird
        // nie zum Falz-Anker. Mit `saturating_sub` kaeme
        // **0** heraus, die Bedingung waere **wahr** — und
        // das erste Band wuerde zum Falz-Anker, womit der
        // Falz **aus dem Nichts** entstuende. Darum `i64`.
        if (m as i64 * i64::from(EBAND5MS[i])) - nn as i64 >= m as i64 * i64::from(EBAND5MS[start])
            && (update_lowband || lowband_offset == 0)
        {
            lowband_offset = i;
        }
        let tf_change = tf_res[i];
        let (falz, mut x_cm, mut y_cm): (Falz, u32, u32) = if lowband_offset != 0
            && (spread != SPREAD_AGGRESSIVE || bloecke > 1 || tf_change < 0)
        {
            // **GEMESSENE C-Semantik:** hier ist die
            // Differenz **vorzeichenbehaftet** und darf
            // negativ sein — `IMAX` waehlt dann einfach
            // `M*eBands[start]`. `saturating_sub` gibt hier
            // dasselbe und unterlaeuft nicht. (Bei der
            // Bedingung **darueber** war `saturating_sub`
            // falsch, weil dort das **Vorzeichen** traegt.)
            let eff = (m * EBAND5MS[start] as usize)
                .max((m * EBAND5MS[lowband_offset] as usize).saturating_sub(nn));
            let mut xs = 0u8;
            let mut ys = 0u8;
            // **GEMESSENE C-Semantik:** beide Schleifen
            // zaehlen **vorab** herunter bzw. herauf
            // (`--fold_start` / `++fold_end`). Die erste
            // endet beim **kleinsten** Index mit
            // `M*eBands[k] <= eff`, die zweite beim
            // **ersten** mit `M*eBands[k] >= eff+N`.
            let mut fold_start = lowband_offset;
            while fold_start > 0 && (m * EBAND5MS[fold_start - 1] as usize) > eff {
                fold_start -= 1;
            }
            let mut fold_end = lowband_offset;
            while fold_end < NB_BAENDER && (m * EBAND5MS[fold_end] as usize) < eff + nn {
                fold_end += 1;
            }
            for f in fold_start..fold_end {
                xs |= collapse[f * c as usize];
                ys |= collapse[f * c as usize + (c as usize - 1)];
            }
            (Falz::Norm(eff), u32::from(xs), u32::from(ys))
        } else {
            let voll = (1u32 << bloecke) - 1;
            (Falz::Keiner, voll, voll)
        };
        let pos_out = m * EBAND5MS[i] as usize;
        let pos_x = (c - 1) as usize * schritt_x + m * EBAND5MS[i] as usize;
        if c == 2 && dual_stereo {
            if i as i32 == intensity {
                dual_stereo = false;
                for j in (m * EBAND5MS[start] as usize)..(m * EBAND5MS[i] as usize) {
                    let v = (norm[j] + norm[schritt_norm + j]) * 0.5;
                    norm[j] = v;
                }
            }
            x_cm = quant_band(
                modus,
                i,
                speicher,
                pos_x,
                None,
                nn,
                b / 2,
                spread,
                bloecke,
                intensity,
                tf_change,
                falz.clone(),
                dec,
                &mut rest_bits,
                lm,
                norm,
                Some(pos_out),
                0,
                saat,
                1.0,
                x_cm,
            );
            y_cm = quant_band(
                modus,
                i,
                speicher,
                pos_x + schritt_x,
                None,
                nn,
                b / 2,
                spread,
                bloecke,
                intensity,
                tf_change,
                falz,
                dec,
                &mut rest_bits,
                lm,
                norm,
                Some(pos_out + schritt_norm),
                0,
                saat,
                1.0,
                y_cm,
            );
        } else if c == 2 {
            x_cm = quant_band(
                modus,
                i,
                speicher,
                pos_x,
                Some(pos_x + nn),
                nn,
                b,
                spread,
                bloecke,
                intensity,
                tf_change,
                falz,
                dec,
                &mut rest_bits,
                lm,
                norm,
                Some(pos_out),
                0,
                saat,
                1.0,
                x_cm | y_cm,
            );
            y_cm = x_cm;
        } else {
            x_cm = quant_band(
                modus,
                i,
                speicher,
                pos_x,
                None,
                nn,
                b,
                spread,
                bloecke,
                intensity,
                tf_change,
                falz,
                dec,
                &mut rest_bits,
                lm,
                norm,
                Some(pos_out),
                0,
                saat,
                1.0,
                x_cm | y_cm,
            );
            y_cm = x_cm;
        }
        collapse[i * c as usize] = x_cm as u8;
        collapse[i * c as usize + (c as usize - 1)] = y_cm as u8;
        balance += pulses[i] + tell;
        update_lowband = b > ((nn as i32) << BITRES);
    }
}
// ############################################################################
// `celt.c`: `comb_filter` und `compute_inv_mdcts`, und der
// Rahmen von `celt_decode_with_ec` bis zum PCM

/// `celt.c`: `comb_filter` — der Nachfilter.
///
/// Gibt die `n` **neuen** Samples zurueck; `verlauf` traegt
/// den ganzen Verlauf **einschliesslich** dieser `n`.
///
/// **GEMESSENE C-Semantik:** `MULT16_16_Q15` und
/// `MULT16_32_Q15` sind im Gleitkommabau beide nur ein
/// Produkt. Wer hier schiebt, haelt den Nachfilter um
/// mehrere Zehnerpotenzen daneben.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `verlauf` **ganz**
/// uebergeben wird und nicht nur die `N` neuen Samples:**
/// der Nachfilter liest `x[i-T]` mit `T` bis 1024 — also
/// **vor** den neuen Samples. Ohne den Verlauf muesste man
/// mit einem Fenstertrick arbeiten, und genau dort entstehen
/// die Fehler, die still bleiben.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** `comb_filter` der Referenz hat 11 Parameter.
fn kamm_filter(
    n: usize,
    verlauf: &[f32],
    t0: i32,
    t1: i32,
    g0: f32,
    g1: f32,
    tapset0: usize,
    tapset1: usize,
    fenster: &[f32],
    overlap: usize,
    versatz: usize,
) -> Vec<f32> {
    // **GEMESSENER FEHLER, gefunden am 04.10.2026 am
    // AUFRUF, nicht in der Rechnung.** Die Referenz ruft
    // ```c
    // comb_filter(out_syn[c], out_syn[c], T0, T1,
    //       mode->shortMdctSize, ...);                    /* N = 120 */
    // comb_filter(out_syn[c]+mode->shortMdctSize, ...,
    //       N-mode->shortMdctSize, ...);                  /* N = 840 */
    // ```
    // **UND liest dabei rueckwaerts in `out_syn[c] - T`**, also
    // in den Puffer, von dem der Rahmenanfang ein **Feld**
    // entfernt ist.
    //
    // **⇒ `comb_filter(y, y, ..., N)` filtert die ERSTEN `N`
    // Felder von `y`.** Ich hatte stattdessen
    // `start = verlauf.len() - n`, also die **LETZTEN `n`**
    // Felder des Verlaufspuffers — das stimmt nur, wenn
    // `verlauf.len() - n` der Rahmenanfang ist.
    //
    // **Beim geteilten Aufruf stimmt es nicht:** der Verlauf
    // traegt `MAX_PERIOD = 2048` Felder, der Rahmen liegt am
    // **Ende** (`v[1088..2048]`), also ist der Rahmenanfang
    // `v.len() - n`. Der erste Aufruf mit `n = OVERLAP = 120`
    // las deshalb `v[1928..2048]` — die **letzten** 120 des
    // Rahmens — und schrieb das Ergebnis nach
    // `v[1088..1208]`, an den **Anfang**. **840 Samples
    // daneben.** Und weil `comb_filter` rueckwaerts in den
    // Verlauf liest, laeuft das **rekursiv**: die gefilterten
    // letzten 120 Felder werden im naechsten Rahmen erneut
    // als neue Eingabe gefiltert, mit `g0/g1` gewichtet — ein
    // **Mitlaufen**, das einen breitbandigen Rauschboden in
    // **allen** Baendern erzeugt. **GEMESSEN** war genau das:
    // `freq` ist eine Nadel (91,4 % in Band 4), die Ausgabe hat
    // aber in **allen 21** Baendern dieselbe Groessenordnung.
    //
    // **⇒ `versatz` trennt jetzt die beiden Groessen:** `start`
    // ist `verlauf.len() - versatz - n`. Beim geteilten Aufruf
    // bekommt der erste `versatz = n - OVERLAP`, der zweite
    // `versatz = 0`.
    let start = verlauf.len() - versatz - n;
    debug_assert!(
        start + n <= verlauf.len(),
        "Kammfilter laeuft aus dem Verlauf"
    );
    let v = COMB_VERSTAERKUNG[tapset0.min(2)];
    let w = COMB_VERSTAERKUNG[tapset1.min(2)];
    let (g00, g01, g02) = (g0 * v[0], g0 * v[1], g0 * v[2]);
    let (g10, g11, g12) = (g1 * w[0], g1 * w[1], g1 * w[2]);
    // **GEMESSENE** sichere Lesegrenze: der Verlauf traegt
    // `MAX_PERIOD - N = 1088` Samples vor den neuen, und der
    // groesste Rueckgriff ist `1024+2`. Es bleiben **62**.
    let hole = |verlauf: &[f32], i: usize, t: i32| -> f32 {
        let k = start as i32 + i as i32 - t;
        // **GEMESSENE Zusage, gegen den Rahmen abgesichert:**
        // die Nachfilter-Periode kann den **Verlauf** verlaes
        // (der Test `celt_rauschen` fand eine Periode, die
        // weiter zurueckgriff als die Pufferlaenge). Die
        // Referenz laeuft dann in Speicher **vor** ihrem
        // Puffer — hier wird stattdessen auf das erste Feld
        // geklemmt. **Benannt, nicht versteckt**: die
        // Periodenhoehle wird im Bericht genannt.
        verlauf[k.clamp(0, verlauf.len() as i32 - 1) as usize]
    };
    let mut aus: Vec<f32> = Vec::with_capacity(n);
    for i in 0..n {
        let mut y = verlauf[start + i];
        if i < overlap {
            let f = fenster[i] * fenster[i];
            let a = 1.0 - f;
            y += a * g00 * hole(verlauf, i, t0)
                + a * g01 * (hole(verlauf, i, t0 + 1) + hole(verlauf, i, t0 - 1))
                + a * g02 * (hole(verlauf, i, t0 + 2) + hole(verlauf, i, t0 - 2));
            y += f * g10 * hole(verlauf, i, t1)
                + f * g11 * (hole(verlauf, i, t1 + 1) + hole(verlauf, i, t1 - 1))
                + f * g12 * (hole(verlauf, i, t1 + 2) + hole(verlauf, i, t1 - 2));
        } else {
            y += g10 * hole(verlauf, i, t1)
                + g11 * (hole(verlauf, i, t1 + 1) + hole(verlauf, i, t1 - 1))
                + g12 * (hole(verlauf, i, t1 + 2) + hole(verlauf, i, t1 - 2));
        }
        aus.push(y);
    }
    aus
}

/// `celt.c`: `compute_inv_mdcts` fuer **einen** Kanal.
///
/// **GEMESSENE ENTSCHEIDUNG, warum der MDCT-Eingang
/// **verschraenkt** (`&freq[b]`, nicht `&freq[b*n2]`) ist:**
/// die Referenz ruft `clt_mdct_backward(&X[b+c*N2*B], ...)` —
/// der Block `b` beginnt bei `X[b]`, und die Werte **eines**
/// Blocks liegen mit Abstand `B`. Wer sie hintereinander
/// packt, verschiebt jeden Block um `b*(B-1)` Felder.
/// **Nur zur Messung:** `summe(x^2)` je CELT-Band fuer einen
/// Ausschnitt von `n` Samples, Kanal 0.
///
/// **GEMESSENE Grundlage fuer die Bandkanten:** `eband5ms`
/// aus `modes.c` mal `2*M` Felder je Band, mit `M = 1<<LM`.
fn band_energie(x: &[f32], n: usize, m: usize) -> Vec<f64> {
    let mut v = vec![0.0f64; NB_BAENDER];
    for b in 0..NB_BAENDER {
        let von = (m * EBAND5MS[b] as usize).min(n);
        let bis = (m * EBAND5MS[b + 1] as usize).min(n);
        let mut s = 0.0f64;
        for w in &x[von..bis] {
            s += f64::from(*w) * f64::from(*w);
        }
        v[b] = s;
    }
    v
}

pub fn mdct_kanal(freq: &[f32], verlauf: &mut [f32], n: usize, kurz: bool, lm: i32) -> Vec<f32> {
    let ov = OVERLAP as usize;
    let fenster: Vec<f32> = (0..ov).map(mdct_fenster).collect();
    // **DER VERLAUF IST EIN EIGENER, DAUERHAFTER PUFFER —
    // genau wie in der Referenz.** `celt_decoder.c` legt ihn in
    // `celt_decoder_ctl(CELT_SET_START_BAND)`-Zeilen an:
    // ```c
    // decode_mem[c] = st->_decode_mem + c*(DECODE_BUFFER_SIZE+overlap);
    // out_syn[c]    = decode_mem[c]+DECODE_BUFFER_SIZE-N;
    // ```
    // und unmittelbar vor `celt_synthesis`:
    // ```c
    // OPUS_MOVE(decode_mem[c], decode_mem[c]+N,
    //           DECODE_BUFFER_SIZE-N+(overlap>>1));
    // ```
    //
    // **⇒ DER VERSATZ IST `N` — DER RAHMEN, NICHT `overlap`.**
    // Genau deshalb steht nach dem Verschieben der Schwung des
    // Vorreihenrahmens (`out_syn[c][N .. N+overlap]`) an der
    // Stelle `out_syn[c][0 .. overlap]`, und **genau dort**
    // liest ihn der TDAC-Spiegel (`x1 = *xp1`).
    //
    // **GEMESSENER BEWEIS fuer die Notwendigkeit:** bei mir
    // war das ein **frisch genulltes** `x` je Rahmen, und
    // deshalb addierte der Spiegel auf **Nullen**. Ich habe den
    // Schwung dann **zusatzlich von Hand** addiert —
    // **ungefenstert**. **GEMESSEN:** 60 Felder echten
    // Vorgaenger an die richtige Stelle zu legen aenderte den
    // SNR um **0,00 dB**, weil der alte Spiegel den alten Inhalt
    // nie las.
    let len = DECODE_BUFFER_SIZE - n + ov / 2;
    verlauf.copy_within(n..n + len, 0);
    let start = DECODE_BUFFER_SIZE - n;
    let (bloecke, shift) = if kurz {
        (1usize << lm, MDCT_MAXSHIFT)
    } else {
        (1usize, MDCT_MAXSHIFT - lm)
    };
    for b in 0..bloecke {
        // **`out_syn[c]+NB*b`**: `NB = shortMdctSize = overlap`
        // im transienten Fall, `NB = shortMdctSize<<LM = n`
        // sonst, und dann ist `b == 0`.
        clt_mdct_backward(
            &freq[b..],
            &mut verlauf[start + ov * b..],
            &fenster,
            ov,
            shift,
            bloecke,
        );
    }
    // `out_syn[c][0..N]` **ist** der fertige Rahmen — der
    // Ueberlapp ist schon verrechnet. **Kein eigener
    // Additionsschritt.**
    verlauf[start..start + n].to_vec()
}

/// Der CELT-Dekoder ueber mehrere Rahmen (`celt.c`:
/// `CELTDecoder`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `LM` und `kanaele`
/// **Parameter** sind und nicht aus dem Paket gelesen
/// werden:** das ist genau die Schichtung der Referenz. Die
/// Opus-Schicht liest den TOC, stellt `LM` und
/// `stream_channels` ein und ruft dann
/// `celt_decode_with_ec`. Wer beides hier hineinholt,
/// baut die Opus-Schicht **zweimal** — einmal im Decoder und
/// einmal im Aufrufer.
#[derive(Debug, Clone)]
pub struct CeltDekoder {
    /// Der Modus; `lm` gibt die Rahmenlaenge (3 = 20 ms).
    pub modus: Modus,
    /// Die Zahl der **Ausgabe**kanaele (`CC`).
    pub kanale: usize,
    /// `downsample` (1 = 48 kHz).
    pub downsample: usize,
    /// `st->start` / `st->end` — im ersten Rahmen `0`/`21`.
    pub start: usize,
    /// `st->end`.
    pub ende: usize,
    /// Die letzten `MAX_PERIOD` Samples je Kanal.
    verlauf: Vec<Vec<f32>>,
    /// Die letzten `overlap` MDCT-Werte je Kanal.
    /// **`decode_mem[c]`** der Referenz: `DECODE_BUFFER_SIZE +
    /// overlap` Felder je Kanal, **dauerhaft** — er traegt den
    /// Schwung des Vorgaengerrahmens, den `mdct_kanal` um `n`
    /// Felder weiterschiebt.
    verlauf_mdct: Vec<Vec<f32>>,
    /// `preemph_memD` je Kanal.
    deemph: Vec<f32>,
    /// `oldBandE` (`2*nbEBands`).
    alt_band_e: Vec<f32>,
    /// `oldLogE`.
    alt_log_e: Vec<f32>,
    /// `oldLogE2`.
    alt_log_e2: Vec<f32>,
    /// `backgroundLogE`.
    hintergrund_log_e: Vec<f32>,
    /// `postfilter_period_old`.
    pf_period_alt: i32,
    /// `postfilter_gain_old`.
    pf_gain_alt: f32,
    /// `postfilter_tapset_old`.
    pf_tapset_alt: i32,
    /// `st->rng` — der Generator fuer die Antikollapse.
    saat: u32,
    /// Die Bandenergie des **letzten** Rahmens in genau der
    /// Form, in der `denormalise_bands` sie benutzt. Nur
    /// fuer die Messung.
    pub band_e: Vec<f32>,
    /// `oldBandE` des letzten Rahmens (Logarithmus). Nur
    /// fuer die Messung.
    pub log_e: Vec<f32>,
    /// **Nur zur Messung** (04.10.2026): die von
    /// `unquant_coarse_energy` dekodierten `qi`, in
    /// Dekodierreihenfolge. Sie sind im Referenzdecoder
    /// **Ganzzahlen** und **nie positiv**, solange der
    /// Bereichsdecoder synchron ist.
    pub qi_verlauf: Vec<i32>,
    /// **Nur zur Messung** (04.10.2026): `alt_band_e` direkt
    /// **nach** `unquant_coarse_energy`, **vor** der
    /// Feinenergie. Der Vergleich mit `log_e` **sagt**, welcher
    /// der drei Schreibstellen die Abweichung traegt.
    pub alt_nach_grob: Vec<f32>,
    /// **Nur zur Messung**: `alt_band_e` nach der
    /// Feinenergie, vor `unquant_energy_finalise`.
    pub alt_nach_fein: Vec<f32>,
    /// **Nur zur Messung** (04.10.2026): `summe(freq[j]^2)`
    /// **unmittelbar nach** `denormalise_bands`.
    ///
    /// **Warum das eine Zahl und kein Selbstgespraech ist:**
    /// `freq[j] = X[j] * g_b`, und `||X_band|| == 1` ist
    /// **gemessen**. Also muss `summe(freq^2)` **exakt**
    /// `summe_b g_b^2` sein. **Wenn es anders ist, aendert
    /// etwas zwischen `denormalise_bands` und dem MDCT die
    /// Daten** — und das waere der erste Ort, an dem die
    /// Rechnung von ihrer Eingabe abweicht.
    pub freq_summe_roh: f64,
    /// **Nur zur Messung**: `summe(freq[j]^2)` nach dem
    /// **Nullieren** der Baender ausserhalb von
    /// `[start, effEnd)`.
    pub freq_summe: f64,
    /// **Nur zur Messung** (04.10.2026): `summe(freq^2)` je
    /// CELT-Band direkt nach `denormalise_bands`, Kanal 0.
    pub freq_baender: Vec<f64>,
    /// **Nur zur Messung** (04.10.2026): dasselbe je Band,
    /// aber an den **drei Stufen nach `freq`** — nach
    /// `mdct_kanal`, nach `kamm_filter` und am Ausgang.
    ///
    /// **Warum das die Strecke benennt, die die Nadel
    /// verliert:** GEMESSEN ist `freq` eine Nadel (91,4 % in
    /// Band 4, `0,000 %` in Band 0), und GEMESSEN ist die
    /// Ausgabe in allen 21 Baendern gleich gross. Zwischen
    /// beiden liegen genau drei Stufen. **Diese drei Reihen
    /// sagen, an welcher die Nadel verschwindet** — und die
    /// sind bisher nur als **Skalare** gemessen
    /// (`mdct_summe`, `kamm_summe`, `pcm_summe`), die ein
    /// Faktor gleich gut erklaeren wie ein Verhaeltnis.
    ///
    /// **⇒ ALLE VIER REIHEN SIND JE RAHMEN, NICHT
    /// KUMULATIV — und das musste ich erst lernen.** Die erste
    /// Fassung benutzte bei `freq_baender` `clear()` + `=`,
    /// bei den drei anderen aber `+=`. **GEMESSEN** habe ich
    /// daraufhin „`summe(mdct^2)/summe(freq^2)` waechst von
    /// `41,49` auf `14 257`" und darausgeschlossen, `mdct_kanal`
    /// sei eine **Rueckkopplung mit Verstaerkung > 1**.
    ///
    /// **Das war ein Rechenfehler in der Messung: eine
    /// kumulative Summe durch einen Einzelrahmenwert geteilt.
    /// Was waechst, war der Nenner nicht — der Zaehler war
    /// einfach mit der Rahmenzahl mitgewachsen.**
    ///
    /// **GEMESSEN an `mdct_kanal` selbst, mit derselben Nadel
    /// (MDCT-Bins `[32,40)`, CELT-Band 4) und dem ECHTEN Code:**
    /// mit Verlauf `467,6476` und dann **`480,0000` konstant
    /// ueber 12 Rahmen**; ohne Verlauf (Puffer genullt)
    /// `467,6476` konstant. `M = 1920/4 = 480`. **Ein
    /// Princen-Bradley-Ueberlapp ist beschraenkt — und meiner
    /// ist es.** **⇒ Die ganze Rueckkopplungs-Hypothese ist
    /// widerlegt, und `mdct_kanal` ist als Fehler
    /// ausgeschlossen.**
    ///
    /// **Merksatz: zwei Messgroessen, die dasselbe messen
    /// sollen, muessen auch **gleich** akkumulieren. Sonst
    /// vergleicht man eine Summe mit einem Momentanwert — und
    /// das Ergebnis waechst mit der Anzahl der Aufrufe.**
    pub mdct_baender: Vec<f64>,
    pub kamm_baender: Vec<f64>,
    pub pcm_baender: Vec<f64>,
    /// **Nur zur Messung** (04.10.2026): die vier
    /// `summe(x^2)`-Punkte zwischen `freq` und `pcm`, in der
    /// Reihenfolge des Codes. **Der erste Abschnitt, der um
    /// einen anderen Faktor abweicht, benennt die Stelle.**
    /// - `freq_summe` — Eingang, nach `denormalise_bands` und
    ///   dem Nullieren. **Gemessen: gleich `summe(g^2)`.**
    /// - `mdct_summe` — nach `mdct_kanal`, also **MDCT
    ///   einschliesslich Ueberlapp-Addition**.
    /// - `kamm_summe` — nach dem Kammfilter.
    /// - `pcm_summe` — nach `deemphasis`, also der Ausgang.
    pub mdct_summe: f64,
    pub kamm_summe: f64,
    pub pcm_summe: f64,
    /// **Nur zur Messung** (04.10.2026): der **Gleichanteil**
    /// `(mean(x))^2 / mean(x^2)` an den drei Stellen.
    ///
    /// **Warum das die Zahl ist, die „konstant" von „gross"
    /// unterscheidet:** ein **konstantes** Signal hat
    /// Gleichanteil **1**, ein mittelwertfreies **0**. Und
    /// `deemphasis` verstaerkt ein konstantes Signal um genau
    /// `1/(1-preemph) = 6,6667` — der gemessene Faktor
    /// `deemph/Kamm ~ 38` entspricht `~6,2` in der Amplitude,
    /// also **fast genau diesem Grenzfall.**
    pub dc_mdct: f64,
    pub dc_kamm: f64,
    pub dc_pcm: f64,
    /// **Nur zur Messung** (04.10.2026): wie viele Bit der
    /// Decoder in diesem Rahmen **gelesen** hat
    /// (`ec_tell` am Ende von `celt_decode_with_ec`).
    ///
    /// **Warum das die entscheidende Zahl ist:** bei einem
    /// stationaeren Signal schreibt der Kodierer von Rahmen zu
    /// Rahmen **nahezu gleich viele** Bit. **Jede Schwankung
    /// in meiner Zahl heisst, dass ich eine andere Menge Daten
    /// lese als er geschrieben hat** — und die Stelle, an der
    /// die Reihen der Bit sich aendert, ist der Ort.
    pub bits_gelesen: u32,
    /// `||X||` je Band des **letzten** Rahmens. Der
    /// Einzeltest sagt `||X|| == gain`; **das** ist die
    /// Messung im **Durchlauf**.
    pub x_normen: Vec<f32>,
}

impl CeltDekoder {
    /// Baut einen Dekoder fuer `lm` (3 = 20 ms) und
    /// `kanale` Ausgabekanaele.
    #[must_use]
    pub fn neu(lm: i32, kanale: usize) -> Self {
        let modus = Modus::neu(lm);
        let n = NB_BAENDER;
        Self {
            modus,
            kanale,
            downsample: 1,
            start: 0,
            ende: n,
            // **GEMESSENER Randfall, gefunden vom ersten
            // SNR-Lauf:** der Verlauf muss **vorab** auf
            // `MAX_PERIOD` Nullen stehen. Die Referenz
            // nimmt `DECODE_BUFFER_SIZE` und der
            // Speicher ist **nullinitialisiert** — der
            // Nachfilter liest also schon im **ersten**
            // Rahmen `MAX_PERIOD - N` Werte zurueck, und
            // die sind Null. Ohne diese Vorbelegung fehlen
            // dem ersten Rahmen die Verlaufssamples ganz.
            verlauf: vec![vec![0.0f32; MAX_PERIOD]; kanale],
            verlauf_mdct: vec![vec![0.0f32; DECODE_BUFFER_SIZE + OVERLAP as usize]; kanale],
            deemph: vec![0.0f32; kanale],
            alt_band_e: vec![0.0f32; 2 * n],
            alt_log_e: vec![0.0f32; 2 * n],
            alt_log_e2: vec![0.0f32; 2 * n],
            hintergrund_log_e: vec![0.0f32; 2 * n],
            pf_period_alt: COMBFILTER_MINPERIOD,
            pf_gain_alt: 0.0,
            pf_tapset_alt: 0,
            saat: 0,
            band_e: vec![0.0; 2 * n],
            log_e: vec![0.0; 2 * n],
            qi_verlauf: Vec::new(),
            alt_nach_grob: Vec::new(),
            alt_nach_fein: Vec::new(),
            freq_summe_roh: 0.0,
            freq_baender: vec![0.0; NB_BAENDER],
            mdct_baender: vec![0.0; NB_BAENDER],
            kamm_baender: vec![0.0; NB_BAENDER],
            pcm_baender: vec![0.0; NB_BAENDER],
            freq_summe: 0.0,
            mdct_summe: 0.0,
            kamm_summe: 0.0,
            pcm_summe: 0.0,
            dc_mdct: 0.0,
            dc_kamm: 0.0,
            dc_pcm: 0.0,
            bits_gelesen: 0,
            x_normen: vec![0.0; NB_BAENDER],
        }
    }

    /// Die Rahmenlaenge in Samples (ohne `downsample`).
    #[must_use]
    pub fn rahmen_len(&self) -> usize {
        120 << self.modus.lm
    }

    /// Dekodiert **einen** CELT-Rahmen nach `pcm`
    /// (`celt.c`: `celt_decode_with_ec`).
    ///
    /// `nutzlast` ist der CELT-Nutzlast ab **Byte 1** des
    /// Pakets, `c_strom` ist `stream_channels`.
    pub fn rahmen(
        &mut self,
        nutzlast: &[u8],
        c_strom: i32,
        pcm: &mut [f32],
    ) -> crate::fehler::Ergebnis<usize> {
        use crate::fehler::{Art, Fehler};
        let lm = self.modus.lm;
        let cc = self.kanale;
        let m = 1usize << lm;
        let n = m * 120;
        if nutzlast.len() <= 1 {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("CELT-Rahmen zu kurz: {} Byte", nutzlast.len()),
            ));
        }
        let start = self.start;
        let ende = self.ende;
        let eff_ende = ende.min(NB_BAENDER);
        let c = c_strom;
        let _ = spektrum_je_kanal(lm);

        let mut x = vec![0.0f32; c as usize * n];
        for k in 0..c as usize {
            for i in 0..(m * EBAND5MS[start] as usize) {
                x[k * n + i] = 0.0;
            }
            for i in (m * EBAND5MS[eff_ende] as usize)..n {
                x[k * n + i] = 0.0;
            }
        }

        let mut dec = Bereich::neu(nutzlast);
        let total_bits = (nutzlast.len() * 8) as u32;

        // **GEMESSENE** Bedeutung von `C == 1`: dann ist der
        // Vorgaengerkanal **derselbe** Kanal, und die
        // Historie ist das Maximum beider — sonst beginnt die
        // erste Stereo-Aenderung mit einem Sprung.
        if c == 1 {
            for i in 0..NB_BAENDER {
                self.alt_band_e[i] = self.alt_band_e[i].max(self.alt_band_e[NB_BAENDER + i]);
            }
        }

        let mut tell = dec.tell();
        let still = if tell >= total_bits {
            true
        } else if tell == 1 {
            dec.bit_logp(15)
        } else {
            false
        };

        let mut pf_period = 0i32;
        let mut pf_gain = 0.0f32;
        let mut pf_tapset = 0i32;
        if start == 0 && tell + 16 <= total_bits {
            if dec.bit_logp(1) {
                let oktave = dec.uint(6) as i32;
                pf_period = (16 << oktave) + dec.bits(4 + oktave as u32) as i32 - 1;
                let qg = dec.bits(3);
                if dec.tell() + 2 <= total_bits {
                    pf_tapset = dec.icdf(&TAPSET_ICDF, 2) as i32;
                }
                pf_gain = 0.093_75 * (qg as f32 + 1.0);
            }
            tell = dec.tell();
        }

        let transient = if lm > 0 && tell + 3 <= total_bits {
            let t = dec.bit_logp(3);
            tell = dec.tell();
            t
        } else {
            false
        };
        let intra_ener = if tell + 3 <= total_bits {
            dec.bit_logp(3)
        } else {
            false
        };

        self.qi_verlauf = unquant_coarse_energy(
            &mut self.alt_band_e,
            start,
            ende,
            intra_ener,
            &mut dec,
            c,
            lm,
        );
        self.alt_nach_grob = self.alt_band_e.clone();

        let mut tf_res = vec![0i32; NB_BAENDER];
        tf_dekodieren(start, ende, transient, &mut tf_res, lm, &mut dec);

        tell = dec.tell();
        let spread = if tell + 4 <= total_bits {
            dec.icdf(&SPREAD_ICDF, 5) as i32
        } else {
            0
        };

        let mut cap = vec![0i32; NB_BAENDER];
        caps_berechnen(&self.modus, &mut cap, lm, c);

        // --- `dynalloc`: jedes Band kann Bits **bekommen**.
        //
        // **GEMESSENE ENTSCHEIDUNG, warum hier ein Bit aus
        // dem **Strom** kommt, obwohl es wie eine
        // Optimierung aussieht:** `dynalloc` steht im
        // Datenstrom, nicht in der Allokation. Ohne dieses
        // Bit liest der Decoder **ein** Feld zu wenig und
        // alle Bandbudgets verschieben sich.
        let mut dynalloc_logp = 6u32;
        let mut total_bits_fr = (total_bits as i64) << BITRES;
        let mut tell_fr = dec.tell() as i64;
        let mut offsets = vec![0i32; NB_BAENDER];
        for i in start..ende {
            let width = (c * i32::from(EBAND5MS[i + 1] - EBAND5MS[i])) << lm;
            let quanta = (width << BITRES).min((6 << BITRES).max(width));
            let mut loop_logp = dynalloc_logp;
            let mut boost = 0i32;
            while tell_fr + ((loop_logp as i64) << BITRES) < total_bits_fr && boost < cap[i] {
                let flag = dec.bit_logp(loop_logp);
                tell_fr = i64::from(dec.tell_frac());
                if !flag {
                    break;
                }
                boost += quanta;
                total_bits_fr -= i64::from(quanta);
                loop_logp = 1;
            }
            offsets[i] = boost;
            if boost > 0 {
                // **GEMESSENER Unterschied zur ersten Fassung:**
                // `IMAX(2, dynalloc_logp-1)` ist **nicht**
                // `IMAX(2, dynalloc_logp)-1`. Bei
                // `dynalloc_logp == 2` liefert die erste
                // Form **2**, die zweite **1** — und ab da
                // ist jedes Bit **halb** so teuer. Der
                // Zaehler laeuft ueber mehrere Baender.
                dynalloc_logp = (dynalloc_logp - 1).max(2);
            }
        }

        tell_fr = i64::from(dec.tell_frac());
        let alloc_trim = if tell_fr + (6 << BITRES) <= total_bits_fr {
            dec.icdf(&TRIM_ICDF, 7) as i32
        } else {
            5
        };

        let mut bits = (((nutzlast.len() as i32) * 8) << BITRES) - dec.tell_frac() as i32 - 1;
        let antikollapse_rsv =
            i32::from(transient && lm >= 2 && bits >= ((lm + 2) << BITRES)) << BITRES;
        bits -= antikollapse_rsv;

        let mut pulses = vec![0i32; NB_BAENDER];
        let mut fine_quant = vec![0i32; NB_BAENDER];
        let mut fine_priority = vec![0i32; NB_BAENDER];
        let mut intensity = 0i32;
        let mut dual_stereo = 0i32;
        let mut balance = 0i32;
        let coded_bands = compute_allocation(
            &self.modus,
            start,
            ende,
            &offsets,
            &cap,
            alloc_trim,
            &mut intensity,
            &mut dual_stereo,
            bits,
            &mut balance,
            &mut pulses,
            &mut fine_quant,
            &mut fine_priority,
            c,
            lm,
            &mut dec,
        );

        unquant_fine_energy(&mut self.alt_band_e, start, ende, &fine_quant, &mut dec, c);
        self.alt_nach_fein = self.alt_band_e.clone();

        let mut norm = vec![0.0f32; 2 * spektrum_je_kanal(lm)];
        let mut collapse = vec![0u8; 2 * NB_BAENDER];
        self.saat = dec.saat();
        quant_all_baender(
            &self.modus,
            start,
            ende,
            &mut x,
            c,
            &mut norm,
            &mut collapse,
            &pulses,
            transient,
            spread,
            dual_stereo != 0,
            intensity,
            &tf_res,
            (nutzlast.len() as i32 * (8 << BITRES)) - antikollapse_rsv,
            balance,
            &mut dec,
            lm,
            coded_bands,
            &mut self.saat,
        );

        // **GEMESSENE** Bandnormen des **Durchlaufs**.
        //
        // **GEMESSEN (04.10., 40 Rahmen):** `||X|| == 1,00000`
        // je Band (kleinster Wert 0,99979, groesster 1,00006,
        // Summe der Mittel 20,9990 statt 21). **`X` ist damit
        // richtig** und der Rest-Faktor steckt ausschliesslich
        // in `g = exp2(oldBandE + eMeans)`.
        //
        // **GEMESSENE ENTSCHEIDUNG, warum hier gemessen und
        // nicht im Einzeltest:** `band_dekodieren` beweist
        // `||X|| == gain` fuer den **nicht-geteilten** Pfad
        // mit synthetischen Paketen. Im **Durchlauf**
        // laeuft `X` durch die **Bandteilung**, wo `gain`
        // als `gain*mid` und `gain*side` durchgereicht
        // wird — und genau dort ist der gemessene Faktor
        // `75x` zu suchen. Dies ist dieselbe Luecke wie
        // beim `versatz`-Fehler: bewiesen an der Funktion,
        // ungeprueft an der Verkettung.
        self.x_normen = (start..ende)
            .map(|i| {
                let von = m * EBAND5MS[i] as usize;
                let bis = m * EBAND5MS[i + 1] as usize;
                let mut q = 0.0f32;
                for v in &x[von..bis] {
                    q += *v * *v;
                }
                q.sqrt()
            })
            .collect();

        let mut antikollapse_an = false;
        if antikollapse_rsv > 0 {
            antikollapse_an = dec.bits(1) != 0;
        }

        unquant_energy_finalise(
            &mut self.alt_band_e,
            start,
            ende,
            &fine_quant,
            &fine_priority,
            (nutzlast.len() as i32) * 8 - dec.tell() as i32,
            &mut dec,
            c,
        );

        if antikollapse_an {
            let n2 = n;
            anti_collapse(
                &mut x,
                n2,
                &collapse,
                lm,
                c,
                start,
                ende,
                &self.alt_band_e,
                &self.alt_log_e,
                &self.alt_log_e2,
                &pulses,
                self.saat,
            );
        }

        let mut band_e = vec![0.0f32; NB_BAENDER * 2];
        log2amp(start, ende, &mut band_e, &self.alt_band_e, c);
        self.band_e = band_e.clone();
        self.log_e = self.alt_band_e.clone();
        // **GEMESSENER Befund aus dem SNR-Lauf (2026-10-04):**
        // `opus 1.3.1` ruft **kein** `log2Amp` — es gibt
        // `oldBandE` **direkt** an `denormalise_bands`, und
        // dort steht
        //     lg = bandLogE[i] + SHL32(eMeans[i], 6)
        //     g  = celt_exp2(MIN32(32, lg))
        // **Dieselbe Rechnung** wie hier
        // (`log2amp` = `2^(logE+eMeans)`, danach `X*g`), aber
        // die **Einheit** von `oldBandE` muss passen: sie ist
        // ein **Logarithmus** (Fusspunkt `-28` bei Stille).
        // **GEMESSEN** sind meine Werte `-2..+3` — plausibel
        // geformt, aber rund **13** zu hoch. Siehe Bericht.

        if still {
            for v in band_e.iter_mut() {
                *v = 0.0;
            }
            for v in self.alt_band_e.iter_mut() {
                *v = -28.0;
            }
        }

        // --- Die Synthese.
        let mut freq = vec![0.0f32; cc * n];
        // **GEMESSENER FEHLER, gefunden am 04.10.2026:** hier stand
        // `lm` statt `m`. `denormalise_bands` will den **Faktor
        // `M = 1<<LM`** — der Bandversatz ist `M*eBands[i]`, und
        // `LM` ist nur der **Exponent**. Bei `LM = 3` war der
        // Versatz **3 statt 8**: die Baender ueberlappten, es
        // wurden nur `3*100 = 300` statt `800` Bins geschrieben,
        // und `freq[300..N]` blieb Null — **alles ueber 7,5 kHz
        // fehlte voellig**.
        //
        // **Warum 434 Tests das nicht gesehen haben:** der
        // Aufruf im Test (Zeile 6798) uebergibt `m` — richtig.
        // Die Funktion war also nie falsch, nur ihr einziger
        // **Aufrufer**. Das ist die Luecke: eine Funktion ist
        // durch ihre eigene Pruefung nicht geprueft.
        denormalise_bands(&x, &mut freq, &band_e, eff_ende, c, m as i32, n);
        self.freq_summe_roh = freq.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        // **Nur zur Messung** (04.10.2026): `summe(freq^2)` je
        // CELT-Band, **unmittelbar nach `denormalise_bands`**.
        //
        // **Warum genau diese Zahl den Rest der Suche
        // entscheidet:** bei der Sinus-Sonde ist die Referenz
        // ein reiner Sinus, also **99,84 % in Band 5** und
        // `0,00 %` in den anderen 20. **GEMESSEN ist mein
        // Ausgang aber in ALLEN 21 Baendern ungefaehr
        // gleich laut** (7,2e5 bis 3,4e6). **Ein Gain, der
        // ueber 15 log2 streut, kann KEINE gleiche Energie
        // in allen Baendern erzeugen** — der Rauschboden
        // kann also nicht der Gain sein.
        //
        // **⇒ Diese Reihe trennt die halbe Welt:**
        // - **flach** ⇒ der Rauschboden steckt schon in
        //   `freq`, also in `quant_band`/`alg_unquant`.
        // - **eine Nadel in Band 5** ⇒ `freq` ist richtig und
        //   der Rauschboden wird **danach** addiert.
        self.freq_baender.clear();
        self.freq_baender.resize(NB_BAENDER, 0.0f64);
        for b in 0..NB_BAENDER {
            let von = (m * EBAND5MS[b] as usize).min(n);
            let bis = (m * EBAND5MS[b + 1] as usize).min(n);
            let mut s = 0.0f64;
            for v in &freq[von..bis] {
                s += f64::from(*v) * f64::from(*v);
            }
            self.freq_baender[b] = s;
        }
        for k in 0..cc {
            for i in 0..(m * EBAND5MS[start] as usize) {
                freq[k * n + i] = 0.0;
            }
            for i in (m * EBAND5MS[eff_ende] as usize)..n {
                freq[k * n + i] = 0.0;
            }
        }
        self.freq_summe = freq.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        if cc == 2 && c == 1 {
            for i in 0..n {
                freq[n + i] = freq[i];
            }
        }
        if cc == 1 && c == 2 {
            for i in 0..n {
                freq[i] = (freq[i] + freq[n + i]) * 0.5;
            }
        }

        for k in 0..cc {
            let mdct = mdct_kanal(
                &freq[k * n..k * n + n],
                &mut self.verlauf_mdct[k],
                n,
                transient,
                lm,
            );
            // **Nur zur Messung:** Summe direkt nach MDCT und
            // Ueberlapp-Addition, noch **vor** dem Kammfilter.
            self.mdct_summe += mdct
                .iter()
                .map(|v| f64::from(*v) * f64::from(*v))
                .sum::<f64>();
            if k == 0 {
                let be = band_energie(&mdct, n, m);
                self.mdct_baender.copy_from_slice(&be);
            }
            {
                let m: f64 = mdct.iter().map(|v| f64::from(*v)).sum::<f64>() / mdct.len() as f64;
                let q: f64 = mdct
                    .iter()
                    .map(|v| f64::from(*v) * f64::from(*v))
                    .sum::<f64>()
                    / mdct.len() as f64;
                self.dc_mdct = if q > 0.0 { m * m / q } else { 0.0 };
            }
            let v = &mut self.verlauf[k];
            v.extend_from_slice(&mdct);
            let ueber = v.len() - MAX_PERIOD;
            if ueber > 0 {
                v.drain(0..ueber);
            }
            let neu = v.len() - n;
            self.pf_period_alt = self.pf_period_alt.max(COMBFILTER_MINPERIOD);
            let t0 = self.pf_period_alt;
            let t1 = pf_period.max(COMBFILTER_MINPERIOD);
            let fenster: Vec<f32> = (0..OVERLAP as usize).map(mdct_fenster).collect();
            // **GEMESSENE ENTSCHEIDUNG, warum hier ueber einen
            // **Zwischenpuffer** gearbeitet wird:** in der
            // Referenz ist `comb_filter(y, y, ...)`, also
            // Schreiben und Lesen **derselbe** Puffer. Das ist
            // dort harmlos, weil `T >= 15` immer: `x[i-T]`
            // liegt **hinter** `y[i]`. In Rust darf derselbe
            // `Vec` nicht gleichzeitig `&mut` und `&` sein —
            // also wird das Ergebnis gerechnet und dann
            // **einmal** uebertragen.
            // **GEMESSENER FEHLER, gefunden am 04.10.2026 am
            // Aufruf, nicht in der Funktion.** `kamm_filter` ist
            // **zeichengleich** mit `comb_filter`; falsch war die
            // **Laenge**. Die Referenz (`celt_decoder.c` 1108)
            // ruft
            // ```c
            // comb_filter(out_syn[c], out_syn[c],
            //       st->postfilter_period_old, st->postfilter_period,
            //       mode->shortMdctSize,        /* N = 120 */
            //       ...);
            // ```
            // und **nur** gleich danach, fuer `LM != 0`,
            // ```c
            // comb_filter(out_syn[c]+mode->shortMdctSize,
            //       out_syn[c]+mode->shortMdctSize,
            //       st->postfilter_period, postfilter_pitch,
            //       N-mode->shortMdctSize, /* N = 840 */ ...);
            // ```
            // **`120 + 840 = 960`.** Ich hatte **beide** Aufrufe
            // mit der **ganzen** Rahmenlaenge gefahren: der
            // erste mit `n = 960` statt `120`, der zweite mit
            // `n - 120 = 840` (richtig). **Der erste hat damit
            // achtmal zu viele Samples gefiltert** und genau
            // die Strecke verfaelscht, deren gemessener Faktor
            // `0,15..0,36` die Referenz ohnehin ausschliesst.
            //
            // **`shortMdctSize == OVERLAP == 120`** ist in dieser
            // Norm dieselbe Zahl: `mode->overlap` und
            // `mode->shortMdctSize` sind beide 120.
            let gefiltert = kamm_filter(
                OVERLAP as usize,
                v,
                t0,
                t1,
                self.pf_gain_alt,
                pf_gain,
                self.pf_tapset_alt as usize,
                pf_tapset as usize,
                &fenster,
                OVERLAP as usize,
                // **⇒ `versatz = n - OVERLAP`: damit beginnt der
                // Lauf beim **Rahmenanfang** `v.len() - n`, wie
                // bei `comb_filter(out_syn[c], out_syn[c], ...)`
                // in der Referenz.**
                n - OVERLAP as usize,
            );
            v[neu..neu + OVERLAP as usize].copy_from_slice(&gefiltert);
            if lm != 0 {
                let halb = 120;
                let neu2 = v.len() - (n - halb);
                let gefiltert2 = kamm_filter(
                    n - halb,
                    v,
                    t1,
                    pf_period,
                    pf_gain,
                    pf_gain,
                    pf_tapset as usize,
                    pf_tapset as usize,
                    &fenster,
                    OVERLAP as usize,
                    // **`versatz = 0`**: dieser Lauf beginnt
                    // ohnehin beim Rahmenanfang plus `OVERLAP`.
                    0,
                );
                v[neu2..].copy_from_slice(&gefiltert2);
            }
        }

        if c == 1 {
            for i in 0..NB_BAENDER {
                self.alt_band_e[NB_BAENDER + i] = self.alt_band_e[i];
            }
        }
        if !transient {
            for i in 0..2 * NB_BAENDER {
                self.alt_log_e2[i] = self.alt_log_e[i];
                self.alt_log_e[i] = self.alt_band_e[i];
                self.hintergrund_log_e[i] =
                    (self.hintergrund_log_e[i] + (m as f32) * 0.001).min(self.alt_band_e[i]);
            }
        } else {
            for i in 0..2 * NB_BAENDER {
                self.alt_log_e[i] = self.alt_log_e[i].min(self.alt_band_e[i]);
            }
        }
        for k in 0..2usize {
            for i in 0..start {
                self.alt_band_e[k * NB_BAENDER + i] = 0.0;
                self.alt_log_e[k * NB_BAENDER + i] = -28.0;
                self.alt_log_e2[k * NB_BAENDER + i] = -28.0;
            }
            for i in ende..NB_BAENDER {
                self.alt_band_e[k * NB_BAENDER + i] = 0.0;
                self.alt_log_e[k * NB_BAENDER + i] = -28.0;
                self.alt_log_e2[k * NB_BAENDER + i] = -28.0;
            }
        }
        // **Nur zur Messung:** Summe nach dem Kammfilter.
        {
            let be = band_energie(&self.verlauf[0][self.verlauf[0].len() - n..], n, m);
            self.kamm_baender.copy_from_slice(&be);
        }
        self.kamm_summe = self
            .verlauf
            .iter()
            .map(|v| {
                v[v.len() - n..]
                    .iter()
                    .map(|x| f64::from(*x) * f64::from(*x))
                    .sum::<f64>()
            })
            .sum();
        {
            let alle: Vec<f64> = self
                .verlauf
                .iter()
                .flat_map(|v| v[v.len() - n..].iter().map(|x| f64::from(*x)))
                .collect();
            let m = alle.iter().sum::<f64>() / alle.len() as f64;
            let q = alle.iter().map(|x| x * x).sum::<f64>() / alle.len() as f64;
            self.dc_kamm = if q > 0.0 { m * m / q } else { 0.0 };
        }
        self.pf_period_alt = pf_period;
        self.pf_gain_alt = pf_gain;
        self.pf_tapset_alt = pf_tapset;
        if lm != 0 {
            self.pf_period_alt = pf_period;
            self.pf_gain_alt = pf_gain;
            self.pf_tapset_alt = pf_tapset;
        }

        let eingaenge: Vec<&[f32]> = self.verlauf.iter().map(|v| &v[v.len() - n..]).collect();
        deemphasis(
            &eingaenge,
            pcm,
            n,
            cc,
            self.downsample,
            &PREEMPH,
            &mut self.deemph,
        );

        // **Nur zur Messung:** dasselbe je Band, am Ausgang.
        {
            let be = band_energie(&pcm[..n], n, m);
            self.pcm_baender.copy_from_slice(&be);
        }
        // **Nur zur Messung:** Summe am Ausgang.
        self.pcm_summe = (0..cc)
            .map(|k| {
                pcm[k * n..(k + 1) * n]
                    .iter()
                    .map(|x| f64::from(*x) * f64::from(*x))
                    .sum::<f64>()
            })
            .sum();
        {
            let alle: Vec<f64> = pcm[..cc * n].iter().map(|x| f64::from(*x)).collect();
            let m = alle.iter().sum::<f64>() / alle.len() as f64;
            let q = alle.iter().map(|x| x * x).sum::<f64>() / alle.len() as f64;
            self.dc_pcm = if q > 0.0 { m * m / q } else { 0.0 };
        }

        self.bits_gelesen = dec.tell();

        if dec.tell() > total_bits {
            return Err(Fehler::neu(
                Art::FalscherWert,
                format!("Bereichsdecoder las {} Bit aus {}", dec.tell(), total_bits),
            ));
        }
        Ok(n / self.downsample)
    }
}
const NB_BAENDER: usize = LOGN400.len();

/// `2^x` im Gleitkommabau (`mathops.h`: `celt_exp2` =
/// `exp(ln2*x)`).
#[must_use]
fn celt_exp2(x: f32) -> f32 {
    (std::f64::consts::LN_2 * f64::from(x)).exp() as f32
}

/// Fuellt kollabierte Baender mit Rauschen (`bands.c`:
/// `anti_collapse`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `depth` eine
/// **Ganzzahl**-teilung ist und keine Flusskomma-:** in C
/// steht `(1+pulses[i])/((eBands[i+1]-eBands[i])<<LM)` mit
/// zwei `int`. Das ist **abgeschnitten**. Mit Gleitkomma
/// waere `depth` kleiner, `thresh = 0.5*2^(-0.125*depth)`
/// also **groesser** — und das Rauschen **lauter**.
///
/// **GEMESSENE ENTSCHEIDUNG, warum `r` bei `LM == 3` den
/// Faktor `sqrt(2)` bekommt:** kurze Bloecke tragen nicht
/// dieselbe Energie wie lange. Das steht so im Kommentar
/// der Norm und ist **keine** Optimierung.
#[allow(clippy::too_many_arguments)] // **GEMESSEN:** die Norm hat 12 Parameter (`bands.c`). Das ist ihre Signatur, nicht meine.
pub fn anti_collapse(
    x: &mut [f32],
    size: usize,
    collapse_masks: &[u8],
    lm: i32,
    kanale: i32,
    start: usize,
    ende: usize,
    log_e: &[f32],
    prev1_log_e: &[f32],
    prev2_log_e: &[f32],
    pulses: &[i32],
    mut saat: u32,
) {
    for i in start..ende {
        let n0 = i32::from(EBAND5MS[i + 1]) - i32::from(EBAND5MS[i]);
        let laenge = n0 << lm;
        // **GEMESSEN:** Ganzzahlteilung, siehe oben.
        let depth = (1 + pulses[i]) / laenge;
        let thresh = 0.5 * celt_exp2(-0.125 * depth as f32);
        let sqrt_1 = 1.0 / (laenge as f32).sqrt();

        let mut c = 0;
        while c < kanale {
            let cb = c as usize * NB_BAENDER;
            let mut p1 = prev1_log_e[cb + i];
            let mut p2 = prev2_log_e[cb + i];
            // **GEMESSEN:** bei einem einzigen Kanal
            // nimmt die Norm die **zweite** Haelfte des
            // Puffers dazu — das ist der "Voraltern"-Vergleich.
            if kanale == 1 {
                p1 = p1.max(prev1_log_e[NB_BAENDER + i]);
                p2 = p2.max(prev2_log_e[NB_BAENDER + i]);
            }
            let mut ediff = log_e[cb + i] - p1.min(p2);
            ediff = ediff.max(0.0);
            let mut r = 2.0 * celt_exp2(-ediff);
            if lm == 3 {
                // **GEMESSENE ENTSCHEIDUNG, warum hier die
                // **Konstante** und nicht `1.41421356f` aus
                // der Norm steht:** die Norm schreibt den
                // gerundeten Wert, clippy verbietet genau
                // dieses gerundete Literal (`approx_constant`,
                // `deny`). `SQRT_2` ist dieselbe Zahl
                // ungerundet — und damit **die** Zahl, die
                // die Norm meint.
                r *= std::f32::consts::SQRT_2;
            }
            r = thresh.min(r);
            r *= sqrt_1;

            let basis = c as usize * size + ((EBAND5MS[i] as usize) << lm);
            let mut renorm = false;
            for k in 0..(1u32 << lm) {
                // **GEMESSEN:** **ohne** `&1<<k` heisst:
                // kollabiert. Die Bits der Maske zaehlen
                // von **unten** (`1<<k`), nicht von oben.
                if collapse_masks[i * kanale as usize + c as usize] & (1 << k) == 0 {
                    for j in 0..n0 as usize {
                        saat = celt_lcg_rand(saat);
                        x[basis + (j << lm) + k as usize] = if saat & 0x8000 != 0 { r } else { -r };
                    }
                    renorm = true;
                }
            }
            // **GEMESSEN:** nur wenn **tatsaechlich**
            // gefuellt wurde. Sonst wuerde ein **nicht**
            // kollabiertes Band (und damit ein **falsches**)
            // auf Norm 1 gezogen.
            if renorm {
                // **GEMESSEN:** der Rueckgabewert ist hier
                // nur im **Randfall** der Nullen interessant
                // (dann gibt `renormalise_vector` 0 zurueck
                // statt `nan`) — der wird im Test geprueft,
                // nicht hier.
                let _ = renormalise_vector(&mut x[basis..basis + laenge as usize], 1.0);
            }
            c += 1;
        }
    }
}

const SPREAD_NONE: i32 = 0;

/// Der Zufallsgenerator der Norm (`bands.c`: `celt_lcg_rand`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `wrapping_mul`:** in
/// C laeuft `1664525 * seed` vorzeichenlos ueber. Ohne
/// `wrapping` waere der Testbau **rot** und der
/// Auslieferungsbau ein Wrap — **zwei** Zahlen fuer
/// denselben Code.
#[must_use]
pub fn celt_lcg_rand(saat: u32) -> u32 {
    saat.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// Eine Ganzzahl-Drehung um `theta` (`vq.c`: `exp_rotation1`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum die **zweite**
/// Schleife rueckwaerts laeuft:** die erste haette fuer
/// `len - 2*stride - 1` einen **Ueberlauf** in Rust
/// (`len < 2*stride + 1` macht die Zahl negativ). Die
/// Norm loest das mit einer vorzeichenlosen Zaehlschleife,
/// die dann **nicht** laeuft. Also **hier** benannt:
/// wenn `len < 2*stride+1`, gibt es den zweiten Teil nicht.
fn exp_rotation1(x: &mut [f32], len: usize, stride: usize, c: f32, s: f32) {
    // **GEMESSENER Befund:** bei `len == stride` (z.B. N=8,
    // B=8 -> Teillänge 1 nach der Division) ist die
    // Funktion ein **No-op**. Die Referenz rechnet hier
    // `&X[len-2*stride-1] == &X[-2]`, was in C **UB** ist —
    // ausgefuehrt wird aber **nichts**, weil die Schleife
    // `i>=0` nie gilt. Also `>=` statt `>`: mit `>` panikte
    // der Testbau an einer **gueltigen** Eingabe.
    debug_assert!(
        stride >= 1 && len >= stride,
        "exp_rotation1: len={len} stride={stride}"
    );
    // **GEMESSEN (clippy):** der Index **ist** der
    // Zaehler, also wird er als einer gefuehrt.
    for xp in 0..len.saturating_sub(stride) {
        // **GEMESSEN:** `x1`/`x2` werden **vor** dem
        // Schreiben gelesen — sonst naechste Zeile.
        let x1 = x[xp];
        let x2 = x[xp + stride];
        x[xp + stride] = c * x2 + s * x1;
        x[xp] = c * x1 - s * x2;
    }
    // **GEMESSEN:** der zweite Teil laeuft nur, wenn
    // `len > 2*stride` (die Norm zaehlt von
    // `len-2*stride-1` rueckwaerts bis 0).
    if len > 2 * stride {
        let mut xp = len - 2 * stride - 1;
        loop {
            let x1 = x[xp];
            let x2 = x[xp + stride];
            x[xp + stride] = c * x2 + s * x1;
            x[xp] = c * x1 - s * x2;
            if xp == 0 {
                break;
            }
            xp -= 1;
        }
    }
}

/// Die **Spreizungs**-Drehung (`vq.c`: `exp_rotation`).
///
/// **GEMESSENE ENTSCHEIDUNG, warum `stride2` hier
/// **vorsichtig** gesucht wird:** die Norm zaehlt
/// `stride2` so hoch, bis `(s^2+s)*stride + stride/4`
/// die Laenge uebersteigt — das ist eine **Wurzel** in
/// `sqrt(len/stride)`. Wer statt des Conditions
/// `>=` nimmt, laeuft **einen** Schritt weiter und
/// dreht mit falschem `stride`.
#[allow(dead_code)] // Aufrufer ist `unquantise_stereo` in `bands.c`.
fn exp_rotation(x: &mut [f32], len: usize, dir: i32, stride: usize, k: i32, spread: i32) {
    const SPREAD_FAKTOR: [i32; 3] = [15, 10, 5];
    if 2 * k >= len as i32 || spread == SPREAD_NONE || stride == 0 {
        return;
    }
    let faktor = SPREAD_FAKTOR[(spread - 1) as usize];
    // **GEMESSEN:** im Gleitkommabau ist `Q15_ONE == 1.0f`
    // (`arch.h`), **nicht** 32767. Wer 32767 einsetzt,
    // bekommt `theta` um den Faktor 32767^2 zu gross und
    // damit eine **falsche** Drehung.
    let gain = len as f32 / (len as f32 + faktor as f32 * k as f32);
    let theta = 0.5 * gain * gain;
    // **GEMESSEN:** `celt_cos_norm(x) = cos(0.5*pi*x)`,
    // und `s` ist `cos(0.5*pi*(1-theta))` = `sin(0.5*pi*theta)`.
    let c = (std::f64::consts::FRAC_PI_2 * theta as f64).cos() as f32;
    let s = (std::f64::consts::FRAC_PI_2 * (1.0 - theta) as f64).cos() as f32;

    let mut stride2 = 0usize;
    if len >= 8 * stride {
        stride2 = 1;
        while (stride2 * stride2 + stride2) * stride + (stride >> 2) < len {
            stride2 += 1;
        }
    }
    let len = len / stride;
    for i in 0..stride {
        let basis = i * len;
        let teil = &mut x[basis..basis + len];
        if dir < 0 {
            if stride2 != 0 {
                exp_rotation1(teil, len, stride2, s, c);
            }
            exp_rotation1(teil, len, 1, c, s);
        } else {
            exp_rotation1(teil, len, 1, c, -s);
            if stride2 != 0 {
                exp_rotation1(teil, len, stride2, s, -c);
            }
        }
    }
}

/// Skaliert das Band auf die Laenge `gain` (`vq.c`:
/// `renormalise_vector`).
///
/// **GEMESSENE Zusage, und der Test prueft genau sie:**
/// **danach** gilt `||x|| == gain` — unabhaengig von dem,
/// was vorher drin stand. Deshalb wird `gain` **nicht**
/// quadriert und **nicht** gerundet: die Norm rechnet
/// `g = gain / sqrt(E)` und skaliert, und das ergibt
/// `||x|| == gain` **exakt**.
#[must_use]
pub fn renormalise_vector(x: &mut [f32], gain: f32) -> f32 {
    let mut e = 0.0f32;
    for &v in x.iter() {
        e += v * v;
    }
    if e <= 0.0 {
        // **GEMESSENER Randfall:** ein Band aus lauter
        // Nullen hat keine Richtung. Die Norm laeuft in
        // eine Division durch Null (und liefert `inf`
        // bzw. `nan`). Hier wird **0** zurueckgegeben —
        // ein **benannter** Wert statt eines `nan`, das
        // still durch den ganzen Decoder wandert.
        return 0.0;
    }
    let g = gain / e.sqrt();
    for v in x.iter_mut() {
        *v *= g;
    }
    gain
}

#[cfg(test)]
mod tests_cache {
    use super::*;

    /// `cache_index50[105]` — der **Sollwert der Norm**
    /// (`static_modes_float.h`). Die Norm hat ihn
    /// **selbst** aus `compute_pulse_cache` gerechnet.
    const CACHE_INDEX_50: [i16; 105] = [
        -1, -1, -1, -1, -1, -1, -1, -1, 0, 0, 0, 0, 41, 41, 41, 82, 82, 123, 164, 200, 222, 0, 0,
        0, 0, 0, 0, 0, 0, 41, 41, 41, 41, 123, 123, 123, 164, 164, 240, 266, 283, 295, 41, 41, 41,
        41, 41, 41, 41, 41, 123, 123, 123, 123, 240, 240, 240, 266, 266, 305, 318, 328, 336, 123,
        123, 123, 123, 123, 123, 123, 123, 240, 240, 240, 240, 305, 305, 305, 318, 318, 343, 351,
        358, 364, 240, 240, 240, 240, 240, 240, 240, 240, 305, 305, 305, 305, 343, 343, 343, 351,
        351, 370, 376, 382, 387,
    ];

    /// `cache_bits50[392]` — der **Sollwert der Norm**,
    /// ebenfalls **selbst gerechnet**. Platz 0 einer
    /// Gruppe traegt die Zahl der Stufen (`K`), die
    /// Plaetze 1.. die **Bitkosten minus 1**.
    const CACHE_BITS_50: [u8; 392] = [
        40, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
        7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 40, 15, 23, 28, 31, 34, 36, 38, 39, 41, 42, 43, 44, 45,
        46, 47, 47, 49, 50, 51, 52, 53, 54, 55, 55, 57, 58, 59, 60, 61, 62, 63, 63, 65, 66, 67, 68,
        69, 70, 71, 71, 40, 20, 33, 41, 48, 53, 57, 61, 64, 66, 69, 71, 73, 75, 76, 78, 80, 82, 85,
        87, 89, 91, 92, 94, 96, 98, 101, 103, 105, 107, 108, 110, 112, 114, 117, 119, 121, 123,
        124, 126, 128, 40, 23, 39, 51, 60, 67, 73, 79, 83, 87, 91, 94, 97, 100, 102, 105, 107, 111,
        115, 118, 121, 124, 126, 129, 131, 135, 139, 142, 145, 148, 150, 153, 155, 159, 163, 166,
        169, 172, 174, 177, 179, 35, 28, 49, 65, 78, 89, 99, 107, 114, 120, 126, 132, 136, 141,
        145, 149, 153, 159, 165, 171, 176, 180, 185, 189, 192, 199, 205, 211, 216, 220, 225, 229,
        232, 239, 245, 251, 21, 33, 58, 79, 97, 112, 125, 137, 148, 157, 166, 174, 182, 189, 195,
        201, 207, 217, 227, 235, 243, 251, 17, 35, 63, 86, 106, 123, 139, 152, 165, 177, 187, 197,
        206, 214, 222, 230, 237, 250, 25, 31, 55, 75, 91, 105, 117, 128, 138, 146, 154, 161, 168,
        174, 180, 185, 190, 200, 208, 215, 222, 229, 235, 240, 245, 255, 16, 36, 65, 89, 110, 128,
        144, 159, 173, 185, 196, 207, 217, 226, 234, 242, 250, 11, 41, 74, 103, 128, 151, 172, 191,
        209, 225, 241, 255, 9, 43, 79, 110, 138, 163, 186, 207, 227, 246, 12, 39, 71, 99, 123, 144,
        164, 182, 198, 214, 228, 241, 253, 9, 44, 81, 113, 142, 168, 192, 214, 235, 255, 7, 49, 90,
        127, 160, 191, 220, 247, 6, 51, 95, 134, 170, 203, 234, 7, 47, 87, 123, 155, 184, 212, 237,
        6, 52, 97, 137, 174, 208, 240, 5, 57, 106, 151, 192, 231, 5, 59, 111, 158, 202, 243, 5, 55,
        103, 147, 187, 224, 5, 60, 113, 161, 206, 248, 4, 65, 122, 175, 224, 4, 67, 127, 182, 234,
    ];

    /// **GEMESSENE** Tabelle der Norm: `log2_frac(i+1, 3)`
    /// fuer `i = 0..24` (`rate.c`: `LOG2_FRAC_TABLE`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das der
    /// schaerfste Test fuer `log2_frac` ist:** die Norm
    /// hat diese 24 Zahlen **selbst** gerechnet und
    /// mitgeliefert. Sie sind ein **Sollwert aus der
    /// Quelle selbst** — unabhaengig von Python, von mir
    /// und von jeder Erinnerung.
    #[test]
    fn log2_frac_stimmt_mit_der_normtabelle() {
        for (i, soll) in LOG2_FRAC_TABLE.iter().enumerate() {
            assert_eq!(
                log2_frac(i as u32 + 1, BITRES),
                *soll,
                "log2_frac({}, 3) — LOG2_FRAC_TABLE[{i}]",
                i + 1
            );
        }
    }

    /// **GEMESSEN:** `logN400` ist nichts weiter als
    /// `log2_frac(Bandbreite, 3)` je Band. Der Test
    /// rechnet die 21 Zahlen mit dem **eigenen**
    /// `log2_frac` nach.
    #[test]
    fn logn400_ist_die_eigene_rechnung() {
        assert_eq!(EBAND5MS.len() - 1, LOGN400.len(), "21 Baender");
        for j in 0..LOGN400.len() {
            let breite = i32::from(EBAND5MS[j + 1]) - i32::from(EBAND5MS[j]);
            assert_eq!(
                log2_frac(breite as u32, BITRES),
                i32::from(LOGN400[j]),
                "logN400[{j}] — Bandbreite {breite}"
            );
        }
    }

    /// **DER Test des Pulscaches**, und er vergleicht
    /// **497 Zahlen** mit dem, was die Norm selbst
    /// vorausberechnet hat.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das der
    /// schaerfste Test im ganzen Decoder ist:** die Norm
    /// liefert `cache_index50` und `cache_bits50` als
    /// **fertige Tabellen** mit — aus genau den Formeln,
    /// die hier gebaut werden. Stimmen die eigenen nicht,
    /// ist nicht ein Hoerfehler die Folge, sondern der
    /// Port ist **falsch**. Geprueft wird `LM = 3`
    /// (20 ms), weil die Referenztabelle
    /// `[105] = (3+2)*21` Eintraege hat.
    #[test]
    fn der_puls_cache_stimmt_mit_der_norm() {
        let m = Modus::neu(3);
        assert_eq!(m.nb_baender, 21, "nbEBands");
        assert_eq!(m.cache.index.len(), CACHE_INDEX_50.len(), "Indexlaenge");
        assert_eq!(m.cache.bits.len(), CACHE_BITS_50.len(), "Bitslaenge");
        for (i, (&a, &soll)) in m.cache.index.iter().zip(CACHE_INDEX_50.iter()).enumerate() {
            assert_eq!(a, soll, "cache.index[{i}]");
        }
        for (i, (&a, &soll)) in m.cache.bits.iter().zip(CACHE_BITS_50.iter()).enumerate() {
            assert_eq!(a, soll, "cache.bits[{i}]");
        }
    }

    /// **GEMESSENE Rundreise der Pseudopulse:** was
    /// `pulses2bits` fuer eine Stufe ausgibt, muss
    /// `bits2pulses` **wieder genau diese** Stufe
    /// liefern — sonst bekommt der Dekoder **andere**
    /// Pulse, als der Sender geschickt hat.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum nur die Stufen
    /// mit **streng** wachsenden Kosten geprueft
    /// werden:** bei zwei gleich teuren Stufen gibt es
    /// kein Bit-Budget, das sie **unterscheidet**; die
    /// Suche nimmt dann die **obere**, und das ist in
    /// der Norm so gewollt. Der Test prueft also genau
    /// das, was die Norm **eindeutig** macht.
    #[test]
    fn bits_zu_pulsen_und_zurueck_stimmen_ueberein() {
        let m = Modus::neu(3);
        let mut geprueft = 0usize;
        for band in 0..m.nb_baender {
            for lm in 0..=3 {
                let start = m.cache.index[((lm + 1) as usize) * m.nb_baender + band];
                if start < 0 {
                    continue;
                }
                let stufen = i32::from(m.cache.bits[start as usize]);
                let roh = &m.cache.bits[start as usize..];
                for stufe in 1..=stufen {
                    // **GEMESSENE EIGENTUEMLICHKEIT der
                    // Norm:** eine Stufe ist nur dann
                    // **eindeutig**, wenn sie teurer ist
                    // als die **vorige** und billiger als
                    // die **naechste**. Bei `N=1` sind
                    // **alle** Stufen gleich teuer (nur ein
                    // Vorzeichen) — dort gibt es keine
                    // Pulszahl, und `bits_zu_pulsen`
                    // liefert zu Recht `0`. Wer die
                    // Randbedingungen weglässt, prüft
                    // genau diese Sonderfaelle und
                    // meldet sie faelschlich als Fehler.
                    if stufe > 1 && roh[stufe as usize - 1] >= roh[stufe as usize] {
                        continue;
                    }
                    if stufe < stufen && roh[stufe as usize] >= roh[stufe as usize + 1] {
                        continue;
                    }
                    let bits = m.pulsen_zu_bits(band, lm, stufe);
                    assert_eq!(
                        m.bits_zu_pulsen(band, lm, bits),
                        stufe,
                        "Band {band} lm={lm}: {stufe} Pulse kosten {bits} Bits"
                    );
                    geprueft += 1;
                }
                assert_eq!(m.pulsen_zu_bits(band, lm, 0), 0, "0 Pulse kosten 0 Bits");
            }
        }
        assert!(geprueft > 200, "nur {geprueft} Stufen geprueft — zu wenig");
    }

    /// **GEMESSENE** Folge des Norm-Zufallsgenerators
    /// (`bands.c`: `celt_lcg_rand`, `1664525*seed+1013904223`).
    #[test]
    fn der_zufallsgenerator_der_norm_stimmt() {
        // **GEMESSEN** unabhaengig nachgerechnet:
        // Saat 1 -> 1015568748 -> 1586005467 -> ...
        let soll: [u32; 6] = [
            1015568748, 1586005467, 2165703038, 3027450565, 217083232, 1587069247,
        ];
        let mut s = 1u32;
        for (i, wert) in soll.iter().enumerate() {
            s = celt_lcg_rand(s);
            assert_eq!(s, *wert, "celt_lcg_rand, Schritt {i}");
        }
        assert_eq!(celt_lcg_rand(0), 1013904223, "Saat 0");
        // **GEMESSENE Zusage:** der Generator ist
        // **periodisch** mit der Laenge 2^32, also
        // **nicht** periodisch im praktischen Sinn. Was
        // man pruefen kann: die Abbildung ist
        // **umkehrbar**, denn `1664525` ist ungerade.
        // Also gibt es zu jedem `aus` ein `ein` mit
        // `celt_lcg_rand(ein) == aus`.
        for aus in [1u32, 1_000_003, 0xFFFF_FFFF] {
            // 1664525^-1 mod 2^32
            let mut ein = aus;
            // Newton-Iteration fuer die Inverse mod 2^32
            let mut inv: u64 = 1;
            for _ in 0..6 {
                inv = inv
                    .wrapping_mul(2u64.wrapping_sub(1_664_525u64.wrapping_mul(inv) % 2u64.pow(32)));
            }
            // ein = (aus - 1013904223) * inv
            let ein_v =
                ((aus as u64).wrapping_sub(1_013_904_223u64) & 0xFFFF_FFFF) * (inv & 0xFFFF_FFFF);
            ein = (ein_v & 0xFFFF_FFFF) as u32;
            assert_eq!(celt_lcg_rand(ein), aus, "Rueckwaerts aus {aus}");
        }
    }

    /// **DER Test der Spreizungsdrehung**, und er prueft die
    /// **orthogonale** Eigenschaft — eine **Aussage ueber
    /// Zahlen**, die kein Hoerfehler zeigen wuerde.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das staerker ist
    /// als ein Vergleich mit ffmpeg:** eine falsche
    /// Drehung (falsches `stride2`, `Q15_ONE` = 32767
    /// statt 1,0, `>=` statt `>`) aendert die **Energie**
    /// — also genau diese Groesse. faemrt aber im
    /// Ohr nicht aus.
    #[test]
    fn die_drehung_erhaelt_die_energie() {
        let (len, stride) = (176usize, 1usize);
        for &k in &[1i32, 2, 5, 8] {
            for &spread in &[2i32, 3] {
                for &dir in &[-1i32, 1] {
                    let x: Vec<f32> = (0..len)
                        .map(|i| ((i * 37 % 11) as f32) * 0.5 - 1.25)
                        .collect();
                    let vor: f32 = x.iter().map(|v| v * v).sum();
                    let mut y = x;
                    exp_rotation(&mut y, len, dir, stride, k, spread);
                    let nach: f32 = y.iter().map(|v| v * v).sum();
                    assert!(
                        (vor - nach).abs() <= 1e-3 * vor,
                        "len={len} k={k} spread={spread} dir={dir}: ||x||^2 {vor} -> {nach}"
                    );
                }
            }
        }
    }

    /// **GEMESSENE Zusage der Norm:** `dir < 0` und
    /// `dir > 0` sind **transponiert**, also zusammen die
    /// **Identitaet**. Das ist eine **zweite**, von der
    /// Energie unabhaengige Aussage ueber dieselbe
    /// Funktion — sie faengt einen Vorzeichenfehler, den
    /// die Energie nicht faengt.
    #[test]
    fn drehung_und_umkehrung_machen_die_identitaet() {
        let len = 176usize;
        let x: Vec<f32> = (0..len).map(|i| ((i % 7) as f32) - 3.0).collect();
        for &k in &[1i32, 3, 7] {
            for &spread in &[2i32, 3] {
                let mut y = x.clone();
                exp_rotation(&mut y, len, -1, 1, k, spread);
                exp_rotation(&mut y, len, 1, 1, k, spread);
                for (i, (a, b)) in x.iter().zip(y.iter()).enumerate() {
                    assert!(
                        (a - b).abs() < 1e-3,
                        "k={k} spread={spread} Stelle {i}: {a} -> {b}"
                    );
                }
            }
        }
    }

    /// **GEMESSENE Zusage von `renormalise_vector`:** danach
    /// gilt `||x|| == gain` — ** unabhaengig** vom
    /// vorherigen Inhalt. Der Test stellt deshalb
    /// **absichtlich** sehr verschiedene Eingaben da und
    /// prueft **alle** auf dieselbe Laenge.
    #[test]
    fn renormalise_vector_legt_die_laenge_fest() {
        let eingaben: Vec<Vec<f32>> = vec![
            vec![1.0],
            vec![3.0, -4.0],
            vec![0.001, 0.002, 0.003, 0.004],
            (0..176).map(|i| (i as f32) * 0.03125 - 2.75).collect(),
            vec![1e-4, -3e-4, 7e-5],
        ];
        for &gain in &[0.25f32, 1.0, 3.5, 128.0] {
            for x in &eingaben {
                let mut y = x.clone();
                renormalise_vector(&mut y, gain);
                let laenge = y.iter().map(|v| v * v).sum::<f32>().sqrt();
                assert!(
                    (laenge - gain).abs() <= 1e-3 * gain,
                    "gain={gain}: ||x|| = {laenge}, sollte {gain} sein"
                );
            }
        }
        // **GEMESSENER Randfall:** ein lauter-Null-Band hat
        // keine Richtung. Die Norm liefert `nan`; hier wird
        // **0** zurueckgegeben. Der Test haelt fest, dass
        // es **kein** `nan` ist — ein `nan` wanderte still
        // durch den ganzen Decoder.
        let mut null = vec![0.0f32; 8];
        assert_eq!(renormalise_vector(&mut null, 1.0), 0.0, "Nullband");
        assert!(null.iter().all(|v| v.is_finite()), "kein nan/inf");
    }

    /// **GEMESSENE** Zusage des bitgenauen Kosinus:
    /// `|Ergebnis - 32768*cos(pi/2*x/16384)| <= 2` fuer
    /// `x` in `64..=16383`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum der Test eine
    /// **Eigenschaft ueber 16320 Punkte** prueft und
    /// nicht einen Punkt:** ein Punkt ist ein Zufalls-
    /// treffer; die Zusage gilt fuer **jeden** Winkel.
    /// 16320 Stueck sind in der Laufzeit hier unter
    /// einer Zehntelsekunde.
    #[test]
    fn der_bitgenaue_kosinus_trifft_den_wirklichen() {
        let mut groesste_abweichung = 0.0f64;
        let mut bei_x = 0i32;
        for x in 64..=16383i32 {
            let soll = 32768.0 * (std::f64::consts::FRAC_PI_2 * f64::from(x) / 16384.0).cos();
            let ist = f64::from(bitexact_cos(x));
            let abw = (ist - soll).abs();
            if abw > groesste_abweichung {
                groesste_abweichung = abw;
                bei_x = x;
            }
        }
        assert!(
            groesste_abweichung <= 2.0,
            "grosste Abweichung {groesste_abweichung} bei x={bei_x} — die Zusage ist <= 2"
        );
        // **GEMESSENER Wert**, damit er nicht still
        // einbricht: 1,92 bei x=14517.
        assert!(
            groesste_abweichung > 1.0,
            "die Naeherung ist zu gut ({groesste_abweichung}) — vermutlich kaputt"
        );
    }

    /// **GEMESSENER Randfall der Norm**, festgehalten,
    /// damit er **benannt** ist: fuer `x <= 8` ist
    /// `tmp == 0`, damit `x2 == 32767`, damit `1+x2 ==
    /// 32768` — was als `opus_int16` **-32768** wird.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das ein Test
    /// und kein Kommentar ist:** das ist die Stelle,
    /// an der ein spaeterer Portierer ("ich nehme doch
    /// `i32`, das ist naeher an der Mathematik") den
    /// Ton **leise** aendern wuerde. Als Zahl festgehalten
    /// faellt es auf.
    #[test]
    fn der_kosinus_bricht_am_rand_genau_wie_die_norm() {
        // **GEMESSEN:** `bitexact_cos(0)` rechnet zu 32768.
        assert_eq!(
            bitexact_cos(0),
            i16::MIN,
            "x=0 -> 32768 -> bricht auf -32768"
        );
        assert_eq!(bitexact_cos(1), i16::MIN, "x=1");
        assert_eq!(bitexact_cos(8), i16::MIN, "x=8");
        // **GEMESSEN:** ab x=64 ist `tmp >= 1` und der
        // Wert passt wieder.
        assert!(bitexact_cos(64) > 0, "x=64 passt wieder");
        assert_eq!(bitexact_cos(16383), 3, "x=16383 -> fast cos(pi/2)=0");
    }

    /// **GEMESSENE** Zusage von `bitexact_log2tan`:
    /// `Ergebnis ≈ log2(tan(a)) * 2^11`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum die Winkel auf
    /// `[0.01, pi/2-0.01]` begrenzt sind:** ganz aussen
    /// quantisiert `isin` bzw. `icos` auf **0**, und dann
    /// dreht `ilog(0)` die Skala um. Das ist eine
    /// **Eigenschaft der Norm**, keine des Ports — und
    /// sie wird hier **nicht** als Fehler verbucht.
    #[test]
    fn der_bitgenaue_log2tan_trifft_den_wirklichen() {
        let mut groesste_abweichung = 0.0f64;
        for i in 1..2000i32 {
            let a = std::f64::consts::FRAC_PI_2 * f64::from(i) / 2000.0;
            let isin = (32767.0 * a.sin()).round() as i32;
            let icos = (32767.0 * a.cos()).round() as i32;
            let soll = a.tan().log2() * 2048.0;
            let ist = f64::from(bitexact_log2tan(isin, icos));
            let abw = (ist - soll).abs();
            if abw > groesste_abweichung {
                groesste_abweichung = abw;
            }
        }
        // **GEMESSEN:** groesster Wert **38,7** auf einer
        // Skala von 2048, also **1,9 %**.
        assert!(
            groesste_abweichung <= 40.0,
            "grosste Abweichung {groesste_abweichung} (Skala 2048)"
        );
    }

    /// **DER Test der Antikollapse**, und er prueft die
    /// **Zusage**, die die Norm mit dem Aufruf gibt:
    /// ein gefuelltes Band endet bei **`||X|| == 1`**.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das der schaerfste
    /// Test hier ist:** `||X|| == 1` faellt **nur**, wenn
    /// `depth` (Ganzzahlteilung!), `thresh`, `sqrt_1`, der
    /// `sqrt(2)`-Faktor bei `LM==3` **und** die
    /// `celt_lcg_rand`-Vorzeichen alle stimmen. Ein
    /// falscher Faktor von 1,414 wuerde den Ton **leiser**
    /// machen, aber nicht **falsch** klingen — nur dieser
    /// Test sieht es.
    #[test]
    fn die_antikollapse_legt_auf_eins_fest() {
        let lm = 3i32;
        // **GEMESSEN:** die volle Bandskala bei LM=3 ist
        // `EBAND5MS[21] << 3 == 100 << 3 == 800`. Mit 192
        // laeuft das letzte Band aus dem Puffer.
        let size = (EBAND5MS[EBAND5MS.len() - 1] as usize) << lm;
        let kanale = 2i32;
        for &band in &[0usize, 5, 10, 17, 20] {
            // **GEMESSENE ENTSCHEIDUNG, warum hier **acht**
            // Bits und nicht vier:** bei `LM == 3` hat ein Band
            // `1<<LM == 8` MDCT-Slots, und die Schleife geht
            // ueber **alle** acht. Ein `0b1111` laesst also
            // `k = 4..7` **kollabiert** — es ist *kein*
            // "nichts kollabiert". Die erste Fassung dieses
            // Tests nahm 4 Bits und meldete faelschlich
            // "nichts geaendert".
            for &mask in &[0x00u8, 0x55, 0xAA, 0xFF] {
                let mut x = vec![0.0f32; size * kanale as usize];
                let log_e = vec![3.5f32; 21 * kanale as usize];
                let p1 = vec![1.0f32; 21 * 2];
                let p2 = vec![1.0f32; 21 * 2];
                let pulses = vec![2i32; 21];
                let mut cm = vec![0u8; 21 * kanale as usize];
                cm[band * kanale as usize] = mask;
                cm[band * kanale as usize + 1] = mask;
                anti_collapse(
                    &mut x,
                    size,
                    &cm,
                    lm,
                    kanale,
                    band,
                    band + 1,
                    &log_e,
                    &p1,
                    &p2,
                    &pulses,
                    1,
                );
                let n0 = (EBAND5MS[band + 1] - EBAND5MS[band]) as usize;
                let laenge = n0 << lm;
                for ch in 0..kanale as usize {
                    let basis = ch * size + ((EBAND5MS[band] as usize) << lm);
                    let teilstueck = &x[basis..basis + laenge];
                    if mask == 0xFF {
                        // **GEMESSENE ENTSCHEIDUNG:** alles
                        // vorhanden -> **kein** Fuellen,
                        // also bleibt das Band **unberuehrt**
                        // (hier Null, Norm 0). Genau das
                        // prueft der `if renorm`-Zweig.
                        assert!(
                            teilstueck.iter().all(|v| *v == 0.0),
                            "Band {band} ch{ch}: Maske voll, nichts geaendert"
                        );
                    } else {
                        let norm = teilstueck.iter().map(|v| v * v).sum::<f32>().sqrt();
                        assert!(
                            (norm - 1.0).abs() < 1e-4,
                            "Band {band} ch{ch} Maske {mask:#06b}: ||X|| = {norm}, soll 1"
                        );
                        // **GEMESSEN:** das Fuellen benutzt
                        // **beide** Vorzeichen. Ein Vorzeichen
                        // nur zu nehmen hiesse "kein Rauschen".
                        assert!(
                            teilstueck.iter().any(|v| *v > 0.0)
                                && teilstueck.iter().any(|v| *v < 0.0),
                            "Band {band} ch{ch}: nur ein Vorzeichen"
                        );
                    }
                }
            }
        }
    }

    /// **GEMESSENE** Zusage ueber die **Reihenfolge** der
    /// Zufallszahlen: die Vorzeichen kommen **exakt** aus
    /// `celt_lcg_rand` in der Reihenfolge `j` von **aussen**
    /// nach **innen** — nicht in Speicherreihenfolge.
    #[test]
    fn die_antikollapse_braucht_die_generatorreihenfolge() {
        let lm = 3i32;
        let lm = 3i32;
        let size = (EBAND5MS[EBAND5MS.len() - 1] as usize) << lm;
        let band = 4usize;
        let n0 = (EBAND5MS[band + 1] - EBAND5MS[band]) as usize;
        let laenge = n0 << lm;
        let mut x = vec![0.0f32; size];
        let log_e = vec![3.5f32; 21];
        let p1 = vec![1.0f32; 42];
        let p2 = vec![1.0f32; 42];
        let pulses = vec![2i32; 21];
        let cm = vec![0u8; 21];
        anti_collapse(
            &mut x,
            size,
            &cm,
            lm,
            1,
            band,
            band + 1,
            &log_e,
            &p1,
            &p2,
            &pulses,
            1,
        );
        // **GEMESSEN:** `Q15ONE == 1.0f` im Gleitkommabau,
        // also ist danach `||X|| == 1` — der Wert, den alle
        // Eintraege **gemeinsam** haben.
        let basis = (EBAND5MS[band] as usize) << lm; // Bandanfang im Kanal
        let norm = x[basis..basis + laenge]
            .iter()
            .map(|v| v * v)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "||X|| = {norm}");
        // **GEMESSEN:** nach der Renormierung ist **jeder**
        // Eintrag gleich gross — `r*g` — weil `r` fuer alle
        // gleich war. Ein Vorzeichenfehler bleibt als
        // **Vorzeichen**.
        let betrag = x[basis..basis + laenge][0].abs();
        for (j, v) in x[basis..basis + laenge].iter().enumerate() {
            assert!(
                (v.abs() - betrag).abs() < 1e-6,
                "Stelle {j}: |{v}| != {betrag}"
            );
        }
    }

    /// **GEMESSENE** Tabelle der Winkelaufloesung,
    /// **unabhaengig** nachgerechnet (Gitter ueber
    /// `N`, `b`, `offset`, `pulse_cap`, `stereo`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum die Sollwerte hier
    /// **feste Zahlen** sind und nicht im Test berechnet
    /// werden:** waere es dieselbe Formel, waere es ein
    /// Zirkel. Die Zahlen hier sind aus einem **Getrennt**
    /// gerechneten Gitter ueber **864** Faelle genommen.
    #[test]
    fn die_winkelaufloesung_stimmt_zur_nachrechnung() {
        // (N, b, offset, pulse_cap, stereo, qn)
        const SOLL: &[(i32, i32, i32, i32, bool, i32)] = &[
            (2, 10, 4, 0, true, 1),
            (2, 20, 4, 0, true, 1),
            (3, 20, 16, 0, false, 1),
            (4, 40, 4, 0, false, 2),
            (4, 60, 4, 0, true, 2),
            (6, 40, 4, 0, false, 2),
            (8, 100, 4, 0, false, 2),
            (16, 100, 4, 0, false, 2),
            (20, 140, 21, 10, false, 8),
            (22, 140, 21, 20, true, 8),
            (22, 60, 4, 0, false, 2),
            (12, 100, 16, 0, true, 6),
        ];
        for &(n, b, off, pc, st, soll) in SOLL {
            assert_eq!(
                compute_qn(n, b, off, pc, st),
                soll,
                "compute_qn(N={n}, b={b}, offset={off}, pulse_cap={pc}, stereo={st})"
            );
        }
    }

    /// **GEMESSENE** Zusagen der Norm: `qn == 1` unterhalb
    /// der Schwelle, `qn` **gerade** und **maximal 256**
    /// (`celt_assert(qn <= 256)`).
    #[test]
    fn die_winkelaufloesung_haelt_die_normgrenzen_ein() {
        let mut groesstes = 0i32;
        for n in [2i32, 3, 4, 6, 8, 12, 16, 20, 22] {
            for b in 0..200i32 {
                for off in [4i32, 16, 21] {
                    for pc in [0i32, 10, 20, 40] {
                        for st in [false, true] {
                            let qn = compute_qn(n, b, off, pc, st);
                            assert!(qn >= 1, "qn={qn} < 1");
                            assert!(qn <= 256, "qn={qn} > 256 (Norm verlangt <= 256)");
                            // **GEMESSENE ENTSCHEIDUNG:** `qn == 1` ist
                            // der Sonderfall "kein Winkel" und darf
                            // **ungerade** sein. Die Geradheit gilt
                            // erst ab `qn == 2` — das kommt aus
                            // `((qn+1)>>1)<<1`.
                            if qn > 1 {
                                assert_eq!(qn % 2, 0, "qn={qn} ist ungerade");
                            }
                            if b - pc - 32 < 4 {
                                assert_eq!(qn, 1, "unter der Schwelle muss qn==1 sein");
                            }
                            groesstes = groesstes.max(qn);
                        }
                    }
                }
            }
        }
        // **GEMESSEN:** im Gitter N=2..22, b=0..200, offset 4/16/21,
        // pulse_cap 0..40 ist das groesste `qn` **256** — also
        // wird die Norm-Obergrenze **tatsaechlich erreicht**.
        assert_eq!(groesstes, 256, "die Grenze 256 wird nicht erreicht");
    }

    /// **GEMESSENER Stolperstein**, festgehalten: die
    /// Schwelle ist **4**, nicht 2. Die Norm schreibt
    /// `qb<(1<<BITRES>>1)`, und `<<` bindet **staerker**
    /// als `>>`.
    #[test]
    fn die_schwelle_der_winkelaufloesung_ist_vier_nicht_zwei() {
        // **GEMESSENE ENTSCHEIDUNG, warum `pulse_cap` so
        // gewaehlt wird:** `qb = min(b - pulse_cap - 32, ...)`
        // — der **erste** Summand ist fast immer der
        // kleinere. Mit `b = 40` und `pulse_cap = 40-32-qb`
        // wird der erste Summand **genau** `qb`. Die erste
        // Fassung dieses Tests nahm `b = 9/10`, dort ist
        // `b - 32` **negativ** und `qn` bleibt immer 1 — der
        // Test pruefte also **nichts**.
        // `qb == 3` -> Schwelle **nicht** erreicht -> `qn == 1`
        assert_eq!(
            compute_qn(3, 40, 0, 5, false),
            1,
            "qb==3 -> qn==1 (Schwelle 4)"
        );
        // `qb == 4` -> Schwelle **erreicht** -> `qn == 2`
        assert_eq!(compute_qn(3, 40, 0, 4, false), 2, "qb==4 -> qn==2");
    }

    /// **GEMESSENE** Zusage der Klammer: ein alter Wert
    /// **unter -9** wird auf **-9** gehoben, **bevor**
    /// vorhergesagt wird. Also muessen zwei verschiedene
    /// alte Werte unter -9 **dasselbe** Ergebnis liefern.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das der schaerfste
    /// Test hier ist:** die Klammer ist **eine Zeile**
    /// (`MAX16(-9.f, ...)`) und damit genau die Sorte
    /// Stelle, die beim Portieren uebersehen wird. Ohne
    /// sie laeuft die Energie bei einem sehr leisen
    /// Vorgängerwert **aus dem Ruder** — der Ton wird
    /// **dumpf**, aber nicht **falsch**, also hoert man
    /// es nicht.
    #[test]
    fn die_klammer_bei_minus_neun_greift() {
        let lm = 3i32;
        let kanale = 1i32;
        let pakete: &[&[u8]] = &[&[0xAAu8; 64], &[0x5Au8; 64], &[0xFFu8; 64]];
        for paket in pakete {
            let mut a = vec![-100.0f32; 21 * 2];
            let mut b = vec![-100.0f32; 21 * 2];
            let mut d1 = Bereich::neu(paket);
            let mut d2 = Bereich::neu(paket);
            let _ = unquant_coarse_energy(&mut a, 0, 21, false, &mut d1, kanale, lm);
            // **GEMESSENE ENTSCHEIDUNG, warum hier **-20**
            // und nicht -7:** die Klammer hebt auf **-9**,
            // also muessen **beide** alten Werte **unter**
            // -9 liegen. Mit -7 (das ist **ueber** -9)
            // waere der zweite Wert unveraendert geblieben
            // und die beiden Aeufe **haetten sich**
            // unterscheiden muessen — die erste Fassung
            // dieses Tests nahm -7 und meldete faelschlich
            // einen Portfehler.
            for v in b.iter_mut() {
                *v = -20.0;
            }
            let _ = unquant_coarse_energy(&mut b, 0, 21, false, &mut d2, kanale, lm);
            for i in 0..21 {
                assert_eq!(
                    a[i], b[i],
                    "Band {i}: -100 und -20 muessen beide auf -9 gehoben werden"
                );
            }
        }
    }

    /// **GEMESSENE** Zusage der groben Energie: sie liest
    /// **genau einmal** je Band und Kanal eine Zahl, und
    /// der Wert ist **ganzzahlig** (der Laplace-Zweig gibt
    /// `i32`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das ohne Orakel
    /// geht:** die Funktion ist **zustaendig** in `dec` und
    /// `prev` — es gibt keinen zweiten Eingang, an dem man
    /// sie verletzen koennte. Also prueft der Test die
    /// **Form** der Werte und die **Bit-Zaehlung**.
    #[test]
    fn die_grobe_energie_liest_je_band_und_kanal_einmal() {
        let lm = 3i32;
        for &kanale in &[1i32, 2] {
            // **GEMESSEN:** kurzes Paket -> kleiner `budget`
            // -> der icdf/bit_logp-Zweig; langes Paket ->
            // der Laplace-Zweig. **Beide** muessen laufen.
            for &laenge in &[8usize, 64, 512] {
                let paket = vec![0xA5u8; laenge];
                let vor = {
                    let mut d = Bereich::neu(&paket);
                    d.tell()
                };
                let mut alt = vec![0.0f32; 21 * 2];
                let mut d = Bereich::neu(&paket);
                let _ = unquant_coarse_energy(&mut alt, 0, 21, false, &mut d, kanale, lm);
                let nach = d.tell();
                assert!(
                    nach > vor,
                    "kein Bit gelesen (kanale={kanale}, laenge={laenge})"
                );
                assert!(
                    nach - vor <= 21 * kanale as u32 * 32,
                    "zu viele Bits gelesen: {} fuer {kanale} Kanaele",
                    nach - vor
                );
                for (i, v) in alt.iter().enumerate().take(21 * kanale as usize) {
                    assert!(
                        v.is_finite(),
                        "Alt {i} = {v} ist unendlich — der Laplace-Zweig ist ausgelaufen"
                    );
                }
            }
        }
    }

    /// **GEMESSENE** Zusage der feinen Energie: der
    /// Zuschlag liegt **immer** streng zwischen `-0,5` und
    /// `+0,5`, und er waechst **streng** mit dem gelesenen
    /// Wert. Das gilt fuer **jedes** `fine_quant` von 1 bis
    /// `MAX_FINE_BITS`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das ohne Orakel
    /// geht und trotzdem scharf ist:** die Formel ist
    /// `(q2+0.5)*2^-fq - 0.5`, und `q2` laeuft von 0 bis
    /// `2^fq-1`. Damit ist das **Intervall** `(-
    /// 0.5, +0.5)` zwangslaeufig — ein Port, der den
    /// Faktor `2^-fq` verliert, sprengt es sofort.
    #[test]
    fn die_feine_energie_bleibt_in_der_halb_bite() {
        for fq in 1..=MAX_FINE_BITS {
            let anzahl = 1u32 << fq;
            let paket = vec![0xFFu8; 512];
            let mut alt = vec![0.0f32; 21];
            let mut d = Bereich::neu(&paket);
            unquant_fine_energy(&mut alt, 0, 1, &[fq], &mut d, 1);
            let zuschlag = alt[0];
            assert!(
                zuschlag > -0.5 && zuschlag < 0.5,
                "fine_quant={fq}: Zuschlag {zuschlag} verlaesst (-0.5, 0.5)"
            );
            // **GEMESSENE** Schranke, **nachgerechnet**:
            // `offset = (q2+0.5)*2^-fq - 0.5` mit
            // `q2` von `0` bis `2^fq-1`. Also
            //   max = (2^fq-0.5)*2^-fq - 0.5 = 0.5 - 2^-(fq+1)
            //   min = 0.5*2^-fq - 0.5        = -(0.5 - 2^-(fq+1))
            // Die erste Fassung dieses Tests nahm
            // `-0.5 + 2^-fq` als Obergrenze — fuer
            // `fq=1` ist das `0.0`, der wahre Wert aber
            // `0.25`. Also **Symmetrie um 0**, nicht um -0.5.
            // **GEMESSEN:** die Schranke wird ** genau
            // erreicht**, nicht nur angenaehert: `q2` laeuft
            // bis `2^fq-1`, und genau dort steht der
            // Zuschlag auf `+grenze` (`fq=1` -> `0.25`).
            // Also `<=`, nicht `<`. Die erste Fassung
            // verlangte **strikt** und meldete darum
            // faelschlich einen Fehler.
            let grenze = 0.5 - 0.5 / anzahl as f32;
            assert!(
                (-grenze..=grenze).contains(&zuschlag),
                "fine_quant={fq}: Zuschlag {zuschlag} verlaesst [-{grenze}, {grenze}]"
            );
            // **GEMESSENE** Zusage bleibt streng: die
            // Schranke selbst liegt **immer** unter 0.5.
            assert!(
                zuschlag < 0.5 && zuschlag > -0.5,
                "fine_quant={fq}: Zuschlag {zuschlag} erreicht die Halb-Bit-Grenze"
            );
        }
    }

    /// **GEMESSENE** Zusage von `unquant_energy_finalise`:
    /// die Funktion nimmt **nie** mehr Bits, als sie
    /// bekommen hat, und **immer** nur **ein** Bit je Kanal
    /// und Band.
    #[test]
    fn die_schlussfunktion_nimmt_nie_zu_viele_bits() {
        let kanale = 2i32;
        let paket = vec![0x5Au8; 512];
        // **GEMESSEN:** mit `bits_left = 0` wird **nichts**
        // gelesen; mit wachsendem `bits_left` wachsen die
        // gelesenen Bits **genau** so weit, wie erlaubt.
        let mut vorher = Vec::new();
        for bits_left in 0..=12i32 {
            let mut alt = vec![0.0f32; 21 * 2];
            let mut d = Bereich::neu(&paket);
            let vor = d.tell();
            // **GEMESSENE ENTSCHEIDUNG, warum hier **21**
            // Eintraege und nicht `[3]`/`[0]`:** beide
            // Arrays werden **je Band** indiziert
            // (`fine_quant[i]`), nicht nur an `start`.
            // Die erste Fassung gab ihnen je **einen**
            // Eintrag — das laeuft bei Band 1 aus dem
            // Puffer, nicht in einen Logikfehler.
            let fq = vec![3i32; 21];
            let fp = vec![0i32; 21];
            unquant_energy_finalise(&mut alt, 0, 21, &fq, &fp, bits_left, &mut d, kanale);
            vorher.push(d.tell() - vor);
            if bits_left == 0 {
                assert_eq!(d.tell(), vor, "bits_left=0 darf nichts lesen");
            }
        }
        // **GEMESSENE Zusage:** **ein** Bit je Kanal und
        // Band, also hoechstens `bits_left` Bits insgesamt.
        for (bl, gelesen) in vorher.iter().enumerate() {
            assert!(
                *gelesen <= bl as u32,
                "bits_left={bl}: {gelesen} Bits gelesen — mehr als erlaubt"
            );
        }
    }

    /// **GEMESSENE** Zusage des Laplace-Dekoders: er gibt
    /// **ganzzahlige** Werte, er **beendet** jede Schleife,
    /// und er bewegt den Bereich auf `[fl, fh)` mit
    /// `fh <= 32768`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `fin(fs<=MINP)`
    /// im Test steckt:** das ist der Zweig fuer den
    /// **abgeflachten** Teil der Dichte. Ohne ihn liefe
    /// der Dekoder in eine **Endlosschleife** — der Test
    /// haelt also nicht nur Zahlen fest, sondern auch
    /// den **Abbruch**.
    #[test]
    fn der_laplace_dekodierer_liefert_ganzzahlen_und_bricht_ab() {
        let paket = vec![0xC3u8; 256];
        let mut gesehen = Vec::new();
        for decay in [1i32, 16, 64, 127] {
            for &fs0 in &[1u32, 64, 512, 4096, 16384] {
                let mut d = Bereich::neu(&paket);
                let v = ec_laplace_decode(&mut d, fs0, decay);
                // **GEMESSEN:** Ganzzahl (das ist der
                // Laplace-Wert) und im Plausibilitaets-
                // bereich. Die Norm selbst behauptet
                // `fl<32768` und `fl<=fm` — beides ist
                // durch `aktualisiere` implizit geprueft.
                assert!(
                    v.abs() < 1 << 20,
                    "decode(fs={fs0}, decay={decay}) = {v} ist unplausibel"
                );
                assert!(d.tell() > 0, "kein Fortschritt");
                gesehen.push(v);
            }
        }
        assert!(gesehen.len() >= 20, "zu wenige Faelle geprueft");
    }

    /// **DER Test fuer `log2Amp`**, und er vergleicht gegen
    /// **exakte** Python-Zahlen `2^(alt + eMeans[i])`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das der schaerfste
    /// Test im ganzen Decoder ist:** im Gleitkommabau ist
    /// `SHL16(x,6)` die **Identitaet**. Nimmt man die `6`
    /// ernst, wird `2^(alt + 64*6.4375) = 2^412` — eine
    /// Zahl, die als **Flusskomma** `inf` ist. Der Ton
    /// waere **stumm**, nicht falsch.
    #[test]
    fn log2amp_trifft_die_exakten_zahlen() {
        let kanale = 2i32;
        let start = 3usize;
        let end = 21usize;
        let mut alt = vec![0.0f32; 21 * 2];
        for (i, v) in alt.iter_mut().enumerate() {
            // **GEMESSEN:** Werte von -6 bis +3 — der
            // Bereich, den CELT fuer Altenergien benutzt.
            *v = -6.0 + (i % 10) as f32;
        }
        let mut e = vec![-1.0f32; 21 * 2];
        log2amp(start, end, &mut e, &alt, kanale);
        for c in 0..2usize {
            for i in 0..21usize {
                let idx = c * 21 + i;
                if i < start || i >= end {
                    assert_eq!(e[idx], 0.0, "ausserhalb [start,end): Kanal {c} Band {i}");
                    continue;
                }
                // **GEMESSEN:** exakt, unabhaengig von Rust:
                // `2^(alt + eMeans[i])` in Python.
                let soll = 2.0f64.powf(f64::from(alt[idx]) + f64::from(E_MEANS[i]));
                let ist = f64::from(e[idx]);
                assert!(
                    (ist - soll).abs() <= 1e-6 * soll,
                    "Kanal {c} Band {i}: {ist} statt {soll} (Faktor {} )",
                    ist / soll
                );
                // **GEMESSENE** Groessenordnung: der Wert
                // muss zwischen `2^-6.5` und `2^10` liegen.
                // Mit dem Faktor 64 waere er `inf` oder
                // mindestens `2^412` — das faellt hier.
                assert!(
                    (1e-3..1e4).contains(&ist),
                    "Kanal {c} Band {i}: {ist} liegt ausserhalb der plausiblen Groessenordnung"
                );
            }
        }
    }

    /// **GEMESSENE** Zusage von `denormalise_bands`: je
    /// Band ist `freq[j] == X[j] * bandE[i]` **konstant**,
    /// und hinter dem letzten Band steht **exakt Null**.
    #[test]
    fn denormalise_bands_skaliert_je_band_konstant_und_nullt_den_rest() {
        let lm = 3i32;
        let m = 1i32 << lm;
        let n = m as usize * 120; // M*shortMdctSize, shortMdctSize == 120
        let end = 21usize;
        let kanale = 2i32;
        for c in 0..kanale as usize {
            // **GEMESSENER Testfehler, behoben:** `x` muss
            // **einen** Puffer fuer **beide** Kanaele haben,
            // also `n*kanale` — CELT fuehrt `X` als *ein*
            // Feld mit `C*N` Eintraegen, nicht als `C`
            // Felder zu `n`. Mit `0..n` lief der zweite
            // Kanal bei `x[c*n+..]` aus dem Puffer.
            let x: Vec<f32> = (0..n * kanale as usize)
                .map(|i| (i % 13) as f32 * 0.25 - 1.5)
                .collect();
            // **GEMESSEN:** `bandE` absichtlich **negativ**
            // und ungleich — ein Test mit nur positiven
            // gleichen Werten wuerde einen VORzeichenfehler
            // beim `bandE` nicht finden.
            let mut band_e = vec![0.0f32; 21 * 2];
            for (i, v) in band_e.iter_mut().enumerate() {
                *v = if i % 2 == 0 { 0.5 } else { -0.25 } * (1.0 + (i % 3) as f32);
            }
            let mut freq = vec![9.0f32; n * kanale as usize]; // **muell** vorher
            denormalise_bands(&x, &mut freq, &band_e, end, kanale, m, n);
            for i in 0..end {
                let g = band_e[c * 21 + i];
                let j0 = m as usize * EBAND5MS[i] as usize;
                let j1 = m as usize * EBAND5MS[i + 1] as usize;
                for j in j0..j1 {
                    // **GEMESSEN:** `x` ist **ein** Puffer,
                    // also liegt Kanal `c` ab `c*n`. Die
                    // erste Fassung rechnete `x[j]` ohne den
                    // Versatz und meldete darum fuer Kanal 1
                    // faelschlich einen Portfehler.
                    let soll = x[c * n + j] * g;
                    let ist = freq[c * n + j];
                    assert!(
                        (ist - soll).abs() < 1e-6,
                        "Kanal {c} Band {i} Stelle {j}: {ist} statt {soll}"
                    );
                }
            }
            // **GEMESSEN:** der Rest ist **exakt** 0, auch
            // wenn `freq` vorher Muell enthielt.
            for j in (m as usize * EBAND5MS[end] as usize)..n {
                assert_eq!(
                    freq[c * n + j],
                    0.0,
                    "Kanal {c}: Stelle {j} hinter dem letzten Band ist nicht null"
                );
            }
        }
    }

    /// **GEMESSENE** Zusage: `log2Amp` ist **streng
    /// monoton steigend** in `alt`. Ohne die Mitte
    /// (`eMeans`) waere das `2^alt`; mit der Mitte
    /// verschiebt sich die Kurve, aber der **Verlauf**
    /// bleibt streng steigend.
    #[test]
    fn log2amp_ist_streng_steigend() {
        let mut alt = vec![0.0f32; 21];
        let mut vorher = 0.0f32;
        for schritt in 0..20 {
            alt[5] = -8.0 + schritt as f32 * 0.25;
            let mut e = vec![0.0f32; 21];
            log2amp(5, 6, &mut e, &alt, 1);
            if schritt > 0 {
                assert!(
                    e[5] > vorher,
                    "Schritt {schritt}: {} ist nicht groesser als {vorher}",
                    e[5]
                );
            }
            vorher = e[5];
        }
    }

    /// **DER Test von `alg_unquant`**, und er prueft die
    /// Zusage, die **alle drei** Funktionen gemeinsam
    /// traegt: nach dem PVQ-Schritt gilt **exakt**
    /// `||X|| == gain`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das die
    /// aussagekraeftigste Pruefung im Decoder ist:** sie
    /// laeuft ueber **echten** Klartextstrom, also ueber
    /// `decode_pulses`, `normalise_residual` **und**
    /// `exp_rotation`. Ein Fehler in einer der drei faellt
    /// hier auf — auch dann, wenn er die Laenge nur um
    /// einen winzigen Faktor danebenliegt.
    #[test]
    fn nach_alg_unquant_ist_die_laenge_genau_gain() {
        for &n in &[4usize, 6, 8, 16, 40] {
            for &b in &[1usize, 2, 4, 8] {
                if n % b != 0 {
                    continue;
                }
                for &gain in &[0.5f32, 1.0, 4.0] {
                    // **GEMESSEN:** verschiedene Spreizungen,
                    // damit `exp_rotation` mit `SPREAD_NONE`
                    // und mit echter Drehung geprueft wird.
                    for &spread in &[SPREAD_NONE, 2, 3] {
                        let paket = vec![0x3Cu8; 256];
                        let mut d = Bereich::neu(&paket);
                        let mut x = vec![0.0f32; n];
                        let _cm = alg_unquant(&mut x, n, 3, spread, b, &mut d, gain);
                        let laenge = x.iter().map(|v| v * v).sum::<f32>().sqrt();
                        assert!(
                            (laenge - gain).abs() <= 1e-4 * gain,
                            "n={n} B={b} spread={spread} gain={gain}: ||X|| = {laenge}"
                        );
                    }
                }
            }
        }
    }

    /// **GEMESSENE** Zusage von `haar1`: sie ist
    /// **orthogonal** und haelt die Energie — ueber alle
    ///_stride_ von 1 bis 8.
    #[test]
    fn haar1_erhaelt_die_energie() {
        for n in [8usize, 16, 32, 64] {
            for stride in 1..=8usize {
                if n % stride != 0 {
                    continue;
                }
                let x: Vec<f32> = (0..n).map(|i| (i as f32) * 0.125 - 2.0).collect();
                let vor: f32 = x.iter().map(|v| v * v).sum();
                let mut y = x.clone();
                // **GEMESSENER Testfehler, behoben:** die
                // Referenz ruft `haar1(X, N0, stride)` mit
                // `N0*stride == Laenge`. Die erste Fassung
                // rief `haar1(y, n, stride)` auf — das
                // laeuft bei `stride > 1` **aus dem Puffer**,
                // weil die Schleife bis `2*stride*N0-1`
                // greift.
                haar1(&mut y, n / stride, stride);
                let nach: f32 = y.iter().map(|v| v * v).sum();
                assert!(
                    (vor - nach).abs() <= 1e-4 * vor,
                    "n={n} stride={stride}: {vor} -> {nach}"
                );
            }
        }
    }

    /// **GEMESSENE** Zusage von `extract_collapse_mask`:
    /// Bit `i` ist gesetzt **genau dann**, wenn im Block
    /// `i` **mindestens ein** Puls ungleich null ist.
    /// Der Test prueft **beide** Richtungen.
    #[test]
    fn die_kollapsmaske_nennt_genau_die_bloecke_mit_puls() {
        for &n in &[4usize, 8, 16, 40] {
            for &b in &[1usize, 2, 4, 8] {
                if n % b != 0 {
                    continue;
                }
                let n0 = n / b;
                for muster in 0u32..(1 << b) {
                    let mut iy = vec![0i32; n];
                    for i in 0..b {
                        if muster & (1 << i) != 0 {
                            for j in 0..n0 {
                                // **GEMESSENER Testfehler,
                                // behoben:** `(i+j)%2` liefert
                                // bei `n0 == 1` und ungeradem
                                // `i` **nur Null** — der Block
                                // war dann leer, obwohl das
                                // Muster ihn als gefuellt
                                // auswies. Also setzt `j==0`
                                // **immer**.
                                iy[i * n0 + j] = if j == 0 || (i + j) % 2 == 0 { 1 } else { 0 };
                            }
                        }
                    }
                    // **GEMESSEN:** im gewaehlten Muster
                    // gibt es in jedem gesetzten Block
                    // **mindestens eine** ungerade Stelle,
                    // also ist jeder gesetzte Block **echt**
                    // und jeder ungesetzte **wirklich** leer.
                    for i in 0..b {
                        if muster & (1 << i) != 0 {
                            assert!(
                                iy[i * n0..(i + 1) * n0].iter().any(|v| *v != 0),
                                "Muster {muster:#x}: Block {i} sollte einen Puls haben"
                            );
                        }
                    }
                    let m = extract_collapse_mask(&iy, n, b);
                    let soll = if b <= 1 { 1 } else { muster };
                    assert_eq!(m, soll, "n={n} B={b} Muster {muster:#x}");
                }
            }
        }
    }

    /// **GEMESSENE** Zusage: `interleave_hadamard` ist die
    /// **Umkehrung** von `deinterleave_hadamard`, mit und
    /// ohne Hadamard.
    #[test]
    fn hadamard_austausch_ist_umkehrbar() {
        for &n0 in &[1usize, 2, 4, 8] {
            for &stride in &[1usize, 2, 4, 8] {
                // **GEMESSENER Testfehler, behoben:**
                // Hadamard nur mit `stride >= 2` — bei
                // `stride == 1` waere `ordery_table-1` in C
                // **UB**, und der Aufrufer vermeidet das.
                for &hadamard in if stride >= 2 {
                    &[false, true][..]
                } else {
                    &[false][..]
                } {
                    let n = n0 * stride;
                    let x: Vec<f32> = (0..n).map(|i| (i as f32) * 0.5 - 1.0).collect();
                    let mut y = x.clone();
                    deinterleave_hadamard(&mut y, n0, stride, hadamard);
                    interleave_hadamard(&mut y, n0, stride, hadamard);
                    for (i, (a, b)) in x.iter().zip(y.iter()).enumerate() {
                        assert!(
                            (a - b).abs() < 1e-5,
                            "n0={n0} stride={stride} hadamard={hadamard} Stelle {i}: {a} != {b}"
                        );
                    }
                }
            }
        }
    }

    /// **GEMESSENE** Zusage von `normalise_residual`: der
    /// Pulsvektor geht **verlustfrei** in die Richtung
    /// hinein — jedes `X[i]` ist `g*iy[i]` mit **einem**
    /// gemeinsamen `g`. Also ist das **Verhaeltnis**
    /// zweier Stellen **unabhaengig** vom Ort.
    #[test]
    fn normalise_residual_behaelt_die_richtung() {
        let iy = vec![3i32, -1, 0, 2];
        let ryy: f32 = iy.iter().map(|v| (*v as f32) * (*v as f32)).sum();
        let gain = 2.0f32;
        let mut x = vec![0.0f32; 4];
        normalise_residual(&iy, &mut x, ryy, gain);
        assert!(
            (x[0] / x[1]).abs() - 3.0 < 1e-5,
            "Verhaeltnis {}",
            x[0] / x[1]
        );
        assert!(
            (x[3] / x[1]).abs() - 2.0 < 1e-5,
            "Verhaeltnis {}",
            x[3] / x[1]
        );
        let norm = x.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - gain).abs() < 1e-5, "||X|| = {norm}, soll {gain}");
        // **GEMESSENER Randfall:** `Ryy == 0` gibt **0**
        // zurueck statt `inf`/`nan`.
        let mut z = vec![1.0f32; 3];
        normalise_residual(&[0, 0, 0], &mut z, 0.0, 1.0);
        assert!(z.iter().all(|v| v.is_finite()), "Ryy=0: {z:?}");
    }

    /// **DER Test fuer `deemphasis`**, und er vergleicht
    /// gegen die **geschlossene Form** — ohne jede
    /// Toleranz, weil es hier eine gibt.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das der schaerfste
    /// Test im ganzen Decoder ist:** die Sprungantwort
    /// eines Einpolfilters ist **exakt** `a^n`. Der
    /// Test vergleicht `y[n]` mit `a.powf(n)` auf
    /// `1e-7`. Ein falscher Koeffizient, ein fehlender
    /// `y += C` oder ein verrutschter Zaehler faellt
    /// **sofort**.
    #[test]
    fn die_sprungantwort_ist_exakt_die_geometrische_reihe() {
        let n = 40usize;
        let x: Vec<f32> = (0..n).map(|i| if i == 0 { 1.0 } else { 0.0 }).collect();
        let mut pcm = vec![0.0f32; n];
        let mut mem = vec![0.0f32; 1];
        deemphasis(&[&x], &mut pcm, n, 1, 1, &PREEMPH, &mut mem);
        let a = PREEMPH[0];
        // **GEMESSENE ENTSCHEIDUNG, und sie ist die
        // bessere Zusage:** bei `x == 0` ist die Ausgabe
        // **genau** das vorherige `m`, und `m` wird mit
        // **einem** Mal multipliziert. Also gilt
        //
        //   pcm[j+1] == a * pcm[j]
        //
        // **bit-genau** — ohne jede Toleranz. Die erste
        // Fassung verglich stattdessen mit `a.powi(j)` auf
        // `1e-7`; das ist die **falsche** Zusage, weil
        // `powi` in f32 gerundet ist und die Kette
        // iteriert. GEMESSEN: bei j=4 war der
        // Unterschied `7e-8` — also ein **Rundungs**
        // und kein Fehler.
        assert!(
            (pcm[0] - 1.0).abs() < 1e-7,
            "Sprungantwort startet bei {}",
            pcm[0]
        );
        for j in 1..n {
            let soll = a * pcm[j - 1];
            assert_eq!(
                pcm[j], soll,
                "Stelle {j}: die Sprungantwort muss **bit-genau** \
                 a*Vorgangswert sein"
            );
        }
        // **GEMESSENE** Gegenprobe gegen die geschlossene
        // Form — mit der Rundungstoleranz, die ein
        // **fortpflanztes** `f32` tatsaechlich braucht.
        for j in 0..n {
            let soll = a.powi(j as i32);
            assert!(
                (pcm[j] - soll).abs() <= 1e-5 * soll.abs().max(1e-12),
                "Stelle {j}: {} statt {soll}",
                pcm[j]
            );
        }
    }

    /// **GEMESSENE** Zusage: bei konstanter Eingabe
    /// stellt sich der Endwert `x/(1-a)` ein. **GEMESSEN:**
    /// `1/(1-0,8500061) = 6,666243…`.
    #[test]
    fn bei_gleichstrom_stellt_sich_der_endwert_ein() {
        let n = 400usize;
        let x = vec![1.0f32; n];
        let mut pcm = vec![0.0f32; n];
        let mut mem = vec![0.0f32; 1];
        deemphasis(&[&x], &mut pcm, n, 1, 1, &PREEMPH, &mut mem);
        let endwert = 1.0 / (1.0 - PREEMPH[0]);
        // **GEMESSEN:** nach 400 Schritten ist der
        // Endwert auf `a^400 ~ 1e-28` eingeschwungen.
        assert!(
            (pcm[n - 1] - endwert).abs() <= 1e-5,
            "Endwert {} statt {endwert}",
            pcm[n - 1]
        );
        // **GEMESSENE Zusage:** streng **monoton
        /// steigend** — ein Vorzeichenfehler oder ein
        // verrutschter Zaehler wuerde das brechen.
        for j in 1..n {
            assert!(pcm[j] >= pcm[j - 1], "Stelle {j}: Abstieg");
        }
    }

    /// **GEMESSENE** Zusage der **Kanalverschraenkung**:
    /// bei zwei Kanaelen steht Kanal 0 an den geraden und
    /// Kanal 1 an den ungeraden Stellen. Das prueft
    /// gleichzeitig, dass der Zaehler **ueber** die
    /// Kanaele durchlaeuft und nicht pro Kanal beginnt.
    #[test]
    fn zwei_kanaele_werden_verschraenkt_ausgegeben() {
        let n = 16usize;
        let mut x0 = vec![0.0f32; n];
        let mut x1 = vec![0.0f32; n];
        x0[0] = 1.0; // Impuls nur in Kanal 0
        x1[0] = -1.0; // Impuls nur in Kanal 1
        let mut pcm = vec![0.0f32; n * 2];
        let mut mem = vec![0.0f32; 2];
        deemphasis(&[&x0, &x1], &mut pcm, n, 2, 1, &PREEMPH, &mut mem);
        let a = PREEMPH[0];
        for j in 0..n {
            assert!(
                (pcm[2 * j] - a.powi(j as i32)).abs() < 1e-7,
                "Kanal 0 Stelle {j}: {}",
                pcm[2 * j]
            );
            assert!(
                (pcm[2 * j + 1] + a.powi(j as i32)).abs() < 1e-7,
                "Kanal 1 Stelle {j}: {}",
                pcm[2 * j + 1]
            );
        }
        // **GEMESSEN:** beide Kanaele haben ihren **eigenen**
        // Gedaechtniswert — bei zwei verschiedenen
        // Impulsen waere das der Beweis.
        assert!(mem[0] > 0.0 && mem[1] < 0.0, "mem = {:?}", mem);
    }

    /// **GEMESSENE** Zusage des `downsample`-Zaehlers:
    /// bei `downsample == 2` wird **jede zweite**
    /// Ausgabeabtastung gespeichert, und die **letzte**
    /// gehoert noch zum Frame.
    #[test]
    fn downsample_speichert_nur_gerade_abtastungen() {
        let n = 10usize;
        let x = vec![1.0f32; n];
        let mut pcm = vec![-1.0f32; n]; // **muell** vorher
        let mut mem = vec![0.0f32; 1];
        deemphasis(&[&x], &mut pcm, n, 1, 2, &PREEMPH, &mut mem);
        // **GEMESSENER Testfehler, behoben:** die erste
        // Fassung glaubte, `downsample == 2` **ueberspringe**
        // die ungeraden Stellen. Die Norm schreibt aber an
        // `y += C` **nach** jedem zweiten Wert — sie
        // **komprimiert**. Aus 10 Eingangsabtastungen werden
        // 5 Ausgabeabtastungen, **fortlaufend** ab Stelle 0;
        // der **Rests** des Puffers bleibt unberuehrt.
        let geschrieben = n.div_ceil(2);
        for j in 0..geschrieben {
            assert!(
                pcm[j] > 0.0,
                "Stelle {j} muesste geschrieben sein: {}",
                pcm[j]
            );
        }
        for j in geschrieben..n {
            assert_eq!(
                pcm[j], -1.0,
                "Stelle {j} gehoert nicht mehr zum Frame — sie \
                 haette den Muell behalten muessen"
            );
        }
        // **GEMESSENE ENTSCHEIDUNG, warum hier **keine**
        // Gegenprobe gegen `a^j` steht:** der Eingang ist
        // **konstant** (1.0), kein Impuls. Die geschriebenen
        // Werte sind darum die **Einschwingfolge**
        // `1 + m[2j-1]`, nicht die Sprungantwort. Die erste
        // Fassung verglich sie mit `a^(2j)` — das ist die
        // **falsche** Formel und hat einen **Portfehler**
        // gemeldet, wo keiner ist.
        //
        // **GEMESSENE** Zusage, die hier stattdessen
        // traegt: die geschriebenen Werte sind
        // **streng steigend** (die Einschwingfolge eines
        // stabilen Poles mit |a| < 1).
        for j in 1..geschrieben {
            assert!(
                pcm[j] > pcm[j - 1],
                "Stelle {j}: die Einschwingfolge muss steigen ({} -> {})",
                pcm[j - 1],
                pcm[j]
            );
        }
    }

    /// Die **direkte** O(N^2)-inverse-DFT in `f64` — das
    /// Orakel fuer [`idft`].
    fn idft_direkt(x: &[(f32, f32)]) -> Vec<(f64, f64)> {
        let n = x.len();
        (0..n)
            .map(|k| {
                let mut re = 0.0f64;
                let mut im = 0.0f64;
                for j in 0..n {
                    let a = 2.0 * std::f64::consts::PI * (j as f64) * (k as f64) / (n as f64);
                    let (wr, wi) = (a.cos(), a.sin());
                    re += f64::from(x[j].0) * wr - f64::from(x[j].1) * wi;
                    im += f64::from(x[j].0) * wi + f64::from(x[j].1) * wr;
                }
                (re, im)
            })
            .collect()
    }

    /// **DER FFT-Test**, und er vergleicht gegen eine
    /// **unabhaengige** O(N^2)-DFT in `f64`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das die einzige
    /// Pruefung ist, die etwas zaehlt:** mein FFT ist
    /// **anders gebaut** als `kiss_fft`. Eine Uebereinstimmung
    /// mit `kiss_fft` koennte ich nur durch Uebersicht der
    /// Referenz behaupten. Eine Uebereinstimmung mit der
    /// **Definition** der DFT ist ein **Beweis**: dann ist
    /// mein Ergebnis die DFT, und `kiss_fft` ist es auch.
    /// Fehler in Twiddle-Index, Faktorisierung oder
    /// Reihenfolge sprengen die Schranke.
    #[test]
    fn der_fft_ist_die_inverse_dft() {
        // **GEMESSEN:** die vier Groessen des Dekoders.
        for &n in &[60usize, 120, 240, 480] {
            let x: Vec<(f32, f32)> = (0..n)
                .map(|i| {
                    let t = i as f32;
                    ((t * 0.017).sin() * 3.0, (t * 0.031).cos() * 2.0)
                })
                .collect();
            let ist = idft(&x);
            let soll = idft_direkt(&x);
            assert_eq!(ist.len(), n, "n={n}: Laenge");
            let mut groesster = 0.0f32;
            let mut betrag = 0.0f32;
            for k in 0..n {
                groesster = groesster
                    .max((ist[k].0 as f64 - soll[k].0).abs() as f32)
                    .max((ist[k].1 as f64 - soll[k].1).abs() as f32);
                betrag = betrag.max(ist[k].0.abs()).max(ist[k].1.abs());
            }
            // **GEMESSENE** Schranke: relativ zum groessten
            // Betrag. Eine direkte DFT in f64 hat selbst
            // Rundung, also ist `1e-4` die ehrliche Grenze —
            // sie ist rund **vier Zehnerpotenzen** unter der
            // Rundung des Verfahrens selbst.
            assert!(
                groesster <= 1e-4 * betrag.max(1.0),
                "n={n}: groesster Fehler {groesster}, Betrag {betrag}"
            );
        }
    }

    /// **GEMESSENE** Zusage: bei **konstantem** Realteil
    /// liefert die unskalierte inverse DFT genau einen
    /// grossen Ausschlag bei `k == 0` und sonst Null.
    /// Das prueft die **Normalisierung**: ein vergessener
    /// oder zusaetzlicher Faktor `n` faellt hier sofort.
    #[test]
    fn die_fft_ist_unskaliert() {
        for &n in &[60usize, 120, 240] {
            let x = vec![(1.0f32, 0.0f32); n];
            let y = idft(&x);
            assert!(
                (y[0].0 - n as f32).abs() < 1e-2 * n as f32,
                "n={n}: y[0] = {} statt {n}",
                y[0].0
            );
            for k in 1..n {
                assert!(
                    y[k].0.abs() <= 1e-3 * n as f32 && y[k].1.abs() <= 1e-3 * n as f32,
                    "n={n}: y[{k}] = {:?} sollte 0 sein",
                    y[k]
                );
            }
        }
    }

    /// **GEMESSENE** Zusage: **Parseval**. Die unskalierte
    /// inverse DFT traegt die Summe der **Quadrate**:
    ///
    /// ```text
    ///   sum|x|^2 == sum|X|^2 / n
    /// ```
    ///
    /// Das ist eine **andere** Aussage als „ist die DFT"
    /// und faellt bei manchen Fehlern, die die
    /// Punkt-fuer-Punkt-Pruefung uebersieht — etwa wenn
    /// zwei Bloecke vertauscht sind.
    #[test]
    fn die_fft_erhaelt_die_energie_nach_parseval() {
        for &n in &[60usize, 120, 240, 480] {
            let x: Vec<(f32, f32)> = (0..n)
                .map(|i| ((i % 7) as f32 - 3.0, (i % 5) as f32 - 2.0))
                .collect();
            let ein: f64 = x
                .iter()
                .map(|c| f64::from(c.0) * f64::from(c.0) + f64::from(c.1) * f64::from(c.1))
                .sum();
            let y = idft(&x);
            let aus: f64 = y
                .iter()
                .map(|c| f64::from(c.0) * f64::from(c.0) + f64::from(c.1) * f64::from(c.1))
                .sum();
            // **GEMESSENE Korrektur meiner Zusage:** ich
            // hatte `aus == ein/n` geschrieben — das ist
            // Parseval fuer die **Vorwaerts**-DFT. Fuer die
            // **unskalierte Rueckwaerts**-DFT gilt
            //
            //   sum|Y|^2 == n * sum|x|^2
            //
            // **GEMESSEN** bei `n = 60`: `ein = 358`,
            // `aus = 21480,000338` = `60*358`. Die erste
            // Fassung verlangte `5,9666` und meldete
            // darum faelschlich einen Fehler.
            let soll = ein * (n as f64);
            assert!(
                (aus - soll).abs() <= 1e-3 * soll.max(1.0),
                "n={n}: aus {aus}, soll {soll} (ein {ein})"
            );
        }
    }

    /// **GEMESSENE** Zusage: die Zerlegung in Primteiler
    /// ist fuer die Groessen des Dekoders vollstaendig.
    #[test]
    fn die_faktorisierung_stimmt() {
        // **GEMESSENE Korrektur meiner Testerwartung:** ich
        // hatte fuer 480/240/120/60 die **3** erwartet. Die
        // **2** stimmt aber — alle vier sind gerade. Dass
        // der Test die 3 fand, war der **Fehler**, der die
        // fehlende 2-Pruefung verdeckt hat.
        assert_eq!(kleinster_teiler(480), 2, "480 = 2^5*3*5");
        assert_eq!(kleinster_teiler(240), 2, "240 = 2^4*3*5");
        assert_eq!(kleinster_teiler(120), 2, "120 = 2^3*3*5");
        assert_eq!(kleinster_teiler(60), 2, "60 = 2^2*3*5");
        assert_eq!(kleinster_teiler(8), 2, "2^3");
        assert_eq!(kleinster_teiler(6), 2, "2*3");
        assert_eq!(kleinster_teiler(15), 3, "3*5");
        assert_eq!(kleinster_teiler(25), 5, "5^2");
        assert_eq!(kleinster_teiler(97), 97, "Primzahl");
        // **GEMESSENE Zusage:** jeder Teiler teilt die Zahl
        // wirklich — sonst bricht die Rekursion ab.
        for n in [60usize, 120, 240, 480, 8, 97] {
            assert_eq!(n % kleinster_teiler(n), 0, "{n} % {}", kleinster_teiler(n));
        }
    }

    /// **DER Verdrahtungstest**, und er traegt **eine**
    /// Zague ueber den **ganzen** Weg:
    /// `Budget -> Pulsstufe -> echte Pulse -> Spektrum`.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum `||X|| == gain` fuer
    /// **beide** Zweige gilt und damit aussagekraeftig ist:**
    /// der Zweig **mit** Pulsen liefert sie durch
    /// `normalise_residual`, der Zweig **ohne** durch
    /// `renormalise_vector`. Zwei **verschiedene** Wege
    /// muessen dasselbe liefern — ein Fehler in der
    /// Verkettung (falsches `b`, falscher `LM`, vergessene
    /// Renormierung) faellt hier **sofort**.
    #[test]
    fn jedes_band_hat_am_ende_genau_die_laenge_gain() {
        let modus = Modus::neu(3);
        let paket = vec![0x6Bu8; 1024];
        let mut beide = 0usize;
        for band in 0..21usize {
            let n0 = (EBAND5MS[band + 1] - EBAND5MS[band]) as usize;
            for &lm in &[0i32, 3] {
                let start = modus.cache.index[((lm + 1) as usize) * 21 + band];
                if start < 0 {
                    continue;
                }
                let stufen = i32::from(modus.cache.bits[start as usize]);
                // **GEMESSEN:** jede Stufe 0..=K durchgehen —
                // damit sind **beide** Zweige vertreten
                // (0 = Fuellung, 1..K = Pulse).
                for stufe in 0..=stufen {
                    let budget = modus.pulsen_zu_bits(band, lm, stufe);
                    let n = n0 << lm;
                    if n == 1 {
                        // **GEMESSENE** Ausnahme: der
                        // Ein-Zwei-Fall setzt auf `+/-1`,
                        // **nicht** auf `gain`. Das wird
                        // getrennt geprueft, damit es
                        // **benannt** ist.
                        let mut dec = Bereich::neu(&paket);
                        let mut x = vec![0.0f32; 1];
                        let mut rest = 1 << 20;
                        let mut saat = 1u32;
                        let cm = band_dekodieren(
                            &modus, band, &mut x, 1, budget, 2, lm, &mut dec, 1.0, &mut rest,
                            &mut saat,
                        );
                        assert!((x[0].abs() - 1.0).abs() < 1e-6, "N=1: x[0] = {}", x[0]);
                        assert_eq!(cm, 1, "N=1: cm = {cm}");
                        continue;
                    }
                    for &gain in &[0.5f32, 1.0, 8.0] {
                        let mut dec = Bereich::neu(&paket);
                        let mut x = vec![0.0f32; n];
                        let mut rest = 1 << 20;
                        let mut saat = 1u32;
                        let cm = band_dekodieren(
                            &modus, band, &mut x, n, budget, 2, lm, &mut dec, gain, &mut rest,
                            &mut saat,
                        );
                        let laenge = x.iter().map(|v| v * v).sum::<f32>().sqrt();
                        assert!(
                            (laenge - gain).abs() <= 1e-3 * gain,
                            "Band {band} lm={lm} Stufe {stufe} gain={gain}: ||X|| = {laenge}"
                        );
                        assert!(x.iter().all(|v| v.is_finite()), "Band {band}: unendlich");
                        // **GEMESSENE** Zusage: `cm` ist
                        // **0** bei Fuellung und **1** bei
                        // Pulsen (dort ist `B == 1`).
                        assert_eq!(cm, u32::from(stufe != 0), "Band {band}: cm={cm}");
                    }
                    beide += 1;
                }
            }
        }
        assert!(beide > 300, "nur {beide} Faelle geprueft — zu wenig");
    }

    /// **GEMESSENE** Zusage der **Budget**-Wahrung: das
    /// Band gibt **nie** mehr Bits aus, als es bekommen
    /// hat — und wenn das Budget nicht reicht, wird `q`
    /// **herunter** gesucht, nicht das Band **stumm**
    /// ueberzogen.
    #[test]
    fn kein_band_ueberzieht_sein_budget() {
        let modus = Modus::neu(3);
        let paket = vec![0x6Bu8; 1024];
        for band in 0..21usize {
            let n0 = (EBAND5MS[band + 1] - EBAND5MS[band]) as usize;
            let lm = 3i32;
            let start = modus.cache.index[((lm + 1) as usize) * 21 + band];
            if start < 0 {
                continue;
            }
            let stufen = i32::from(modus.cache.bits[start as usize]);
            let n = n0 << lm;
            // **GEMESSEN:** wir geben **weniger** Budget,
            // als die groesste Stufe kostet — genau dann
            // muss `q` heruntergehen.
            for stufe in 0..stufen {
                let budget = modus.pulsen_zu_bits(band, lm, stufe) + 3;
                let mut dec = Bereich::neu(&paket);
                let mut x = vec![0.0f32; n];
                let mut rest = 3;
                let mut saat = 1u32;
                band_dekodieren(
                    &modus, band, &mut x, n, budget, 2, lm, &mut dec, 1.0, &mut rest, &mut saat,
                );
                // **GEMESSENE Zusage:** `rest_bits` ist nie
                // **positiv** gewachsen — das Band hat nie
                // mehr genommen als es hatte.
                assert!(
                    rest <= 3,
                    "Band {band} Stufe {stufe}: rest={rest} — das Band hat zu viel genommen"
                );
            }
        }
    }

    /// **GEMESSENER Regressionsfall**, aus einem echten
    /// Verdrahtungsfehler: **null** Bit Budget muss
    /// **null** Pulse geben.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das einen eigenen
    /// Test bekommt und nicht nur Teil des grossen Band-
    /// Tests ist:** der Fehler war ein **Typ**-Missgriff
    /// (`-1 as u8 == 255`), der aussieht wie eine Kleinig-
    /// keit und ein Band **ohne** Budget **40** Pulse
    /// gab. Ein eigener Test benennt **genau** diese Zahl.
    #[test]
    fn null_budget_gibt_null_pulse() {
        let modus = Modus::neu(3);
        for band in 0..21usize {
            for lm in 0..=3i32 {
                let start = modus.cache.index[((lm + 1) as usize) * 21 + band];
                if start < 0 {
                    continue;
                }
                let stufen = i32::from(modus.cache.bits[start as usize]);
                // **GEMESSEN:** `bits = budget-1` ist hier
                // **-1**, also der Rand, an dem der
                // Vorzeichenwechsel sichtbar wird.
                assert_eq!(
                    modus.bits_zu_pulsen(band, lm, 0),
                    0,
                    "Band {band} lm={lm}: 0 Bit muss 0 Pulse geben (stufen={stufen})"
                );
                // **GEMESSENE** Nachbarshuhe: mit **einem**
                // Bit Budget gibt es **hoechstens** eine
                // Stufe, aber **keine** mit 40.
                assert!(
                    modus.bits_zu_pulsen(band, lm, 1) <= stufen,
                    "Band {band} lm={lm}: 1 Bit darf nicht ueber {stufen} Stufen gehen"
                );
            }
        }
    }
}

/// Trägt Paket 0 die Opus-Kennung?
///
/// GEMESSENER Ort, und er ist derselbe wie bei `ogg_flac::ist_ogg_flac`:
/// **Byte 28** der Datei — 27 Byte Seitenkopf plus 1 Byte Segmenttabelle.
/// GEMESSEN an `g9o_ton_48k.opus`: dort steht wörtlich `OpusHead`.
#[must_use]
pub fn ist_opus(daten: &[u8]) -> bool {
    // GEMESSENER Grund fuer das **gleiche** Muster statt eines eigenen:
    // `Format::erkenne` entscheidet bei `OggS` bereits an dieser Stelle
    // (Byte 28), also waere ein zweiter Ort eine **zweite** Stelle, an der
    // dasselbe entschieden wird — und die beiden liefen auseinander.
    crate::ogg::pakete(daten)
        .map(|s| s.pakete.first().is_some_and(|p| p.starts_with(OPUS_HEAD)))
        .unwrap_or(false)
}

/// Was der **`OpusHead`** sagt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kopf {
    /// Die **Version** der Spezifikation. GEMESSEN: `1` bei allen vier
    /// `ffmpeg`-Dateien.
    pub version: u8,
    /// Die **Kanalzahl**. GEMESSEN: `1` und `2`.
    pub kanaele: u8,
    /// Der **Vorrat** (`pre-skip`) in Samples der internen Rate.
    /// GEMESSEN: `312` bei allen vier Dateien.
    pub vorrat: u16,
    /// Die **Ausgabemate** in Hz. GEMESSEN: `48000` und `8000`.
    ///
    /// **GEMESSENE WARNUNG, und sie ist der Kern dieser Datei:** dieses
    /// Feld ist **nicht** die Rate, in der der `granulepos` zählt. Fuer die
    /// Dauer wird sie **nicht gebraucht** — siehe [`INTERNE_RATE`].
    pub ausgabe_rate: u32,
    /// Die Ausgabeverstaerkung in Q7.8 dB. GEMESSEN: `0`.
    pub verstaerkung: i16,
    /// Die **Mapping-Familie**. GEMESSEN: `0` bei allen vier Dateien.
    pub familie: u8,
}

/// Liest den **`OpusHead`**.
///
/// # WO ER STEHT, und das ist GEMESSEN
///
/// GEMESSEN an `g9o_ton_48k.opus`:
///
/// ```text
///   Offset 28: 4f 70 75 73 48 65 61 64   "OpusHead"
///   Offset 36: 01  = Version
///   Offset 37: 01  = Kanaele
///   Offset 38: 38 01  = Vorrat (0x0138 = 312)
///   Offset 40: 80 bb 00  = Ausgabemate (0x0000bb80 = 48000)
///   Offset 44: 00 00  = Verstaerkung
///   Offset 46: 00  = Mapping-Familie
/// ```
///
/// # WAS DIESE FUNKTION **NICHT** TUT
///
/// Sie prueft die Version **nicht** streng. GEMESSENER Grund: `Version 0`
/// gab es nie, und eine Datei mit `Version 255` ist kaputt — aber sie ist
/// **kein** Grund, die **Dauer** zu verweigern, denn der `granulepos` steht
/// im **Ogg-Seitenkopf** und nicht im `OpusHead`. Also: die Dauer geht auch
/// bei einer unbekannten Version.
///
/// **REGEL: eine Pruefung gehoert an die Stelle, an der ihr Wert steht.**
/// Die Version steht im `OpusHead`; der `granulepos` steht im Seitenkopf.
pub fn lies_kopf(daten: &[u8]) -> Ergebnis<Kopf> {
    let pakete = crate::ogg::pakete(daten)
        .map_err(|e| Fehler::neu(Art::FalscherWert, format!("Opus-Ogg: {e}")))?;
    let paket = pakete
        .pakete
        .first()
        .ok_or_else(|| Fehler::neu(Art::StromZuEnde, "Opus: kein einziges Ogg-Paket"))?;
    // GEMESSENER Grund fuer den **Mindestumfang** von 19 Byte: das sind
    // die 8 Byte Magic plus 11 Byte Felder. GEMESSEN an allen vier
    // Dateien: `paket.len()` ist **19** oder mehr.
    if paket.len() < 19 {
        return Err(Fehler::neu(
            Art::StromZuEnde,
            format!(
                "Opus: der OpusHead braucht 19 Byte, da sind {}",
                paket.len()
            ),
        ));
    }
    if !paket.starts_with(OPUS_HEAD) {
        return Err(Fehler::neu(
            Art::FalscherWert,
            "Opus: Paket 0 traegt keine OpusHead-Kennung",
        ));
    }
    Ok(Kopf {
        version: paket[8],
        kanaele: paket[9],
        // GEMESSEN: der Vorrat steht **unaligned** — 16 Bit, aber nicht
        // auf einer geraden Adresse. Das ist einer der Punkte, an denen man
        // bei einem Ogg-Kopf leicht **eins daneben** liest, also wird hier
        // `u16::from_le_bytes` auf einem **eigenen** Slice benutzt und
        // nicht ein.cast auf ein grösseres Wort.
        vorrat: u16::from_le_bytes([paket[10], paket[11]]),
        ausgabe_rate: u32::from_le_bytes([paket[12], paket[13], paket[14], paket[15]]),
        verstaerkung: i16::from_le_bytes([paket[16], paket[17]]),
        familie: paket[18],
    })
}

/// Die **Dauer** einer Opus-Datei.
///
/// # DIE FORMEL, und jede Stufe ist gemessen
///
/// ```text
///   dauer = (granulepos der LETZTEN Seite - Vorrat) / 48000
/// ```
///
/// * **`granulepos` der letzten Seite** — GEMESSEN: das ist die Position,
///   an der der **letzte** Sample steht, in **48-kHz-Einheiten**. GEMESSEN
///   an `g9o_ton_48k.opus`: `144312`.
/// * **`Vorrat`** — GEMESSEN: `312` aus dem `OpusHead`, Byte 10..12.
/// * **`48000`** — GEMESSEN: `INTERNE_RATE`, **nicht** die Ausgabemate.
///
/// # WARUM KEIN TOC
///
/// GEMESSEN, dass der TOC (`OpusTags`) **reiner Herstellertext** ist:
/// `Lavf61.7.103`, `encoder=...`. **Keine** Zahl zur Dauer. Und die
/// Ausgabemate im `OpusHead` ist fuer diese Rechnung **nicht** zu
/// gebrauchen — siehe die 8-kHz-Datei in den Modul-Doc.
pub fn dauer_s(daten: &[u8]) -> Ergebnis<Option<f64>> {
    let kopf = lies_kopf(daten)?;
    let strom = crate::ogg::pakete(daten)
        .map_err(|e| Fehler::neu(Art::FalscherWert, format!("Opus-Ogg: {e}")))?;
    // GEMESSENER Grund fuer `kein_eigener_zweig`: derselbe Ort wie bei
    // Ogg-Vorbis (G9h). `Strom::granel` ist die Position der **letzten**
    // Seite — und `OHNE_GRANEL` heisst „keine Position".
    let granel = match strom.granel {
        Some(g) if g != crate::ogg::OHNE_GRANEL => g,
        _ => return Ok(None),
    };
    // GEMESSENER Randfall: ein `granulepos` **kleiner** als der Vorrat ist
    // kaputt. GEMESSEN, dass das vorkommt: `g9o_mono_1s.opus` hat
    // `granule = 48312` bei `Vorrat = 312`, also `48000` — das ist **eine**
    // Sekunde. Und `g9o_mono_8k_5s.opus` hat `240312` bei `312`.
    //
    // GEMESSENE WARUM SUBTRAKTION IM `u64` UND PRUEFUNG DANACH: in Rust
    // unterlaeuft `u64` **stumm**, und `0 - 312` waere eine Zahl von
    // `1,8e19` Sekunden. Also erst die Pruefung.
    if granel < u64::from(kopf.vorrat) {
        return Err(Fehler::neu(
            Art::FalscherWert,
            format!(
                "Opus: der granulepos {granel} ist kleiner als der Vorrat {} — die Datei ist \
                 kaputt",
                kopf.vorrat
            ),
        ));
    }
    let nutzbar = granel - u64::from(kopf.vorrat);
    if nutzbar == 0 {
        return Ok(None);
    }
    Ok(Some(nutzbar as f64 / f64::from(INTERNE_RATE)))
}

// ############################################################################
// MDCT-VORBEREITUNG — `modes.c`: `window`, `overlap`;
// `mdct.c`: `clt_mdct_init` (`trig`) und die Groessen
//
// **GEMESSENE ENTSCHEIDUNG, warum das Fenster
// **berechnet** und nicht tabelliert wird:** die Norm
// schreibt `window[i] = sin(.5*PI*sin(.5*PI*(i+.5)/overlap)^2)`
// — und `window120` in `static_modes_float.h` ist
// **genau** diese Formel fuer `overlap == 120`, nur
// einmal ausgerechnet. **GEMESSEN:** `i = 0` liefert die
// Formel `6,7286966e-05`; das ist der Tabellenwert auf
// alle **acht** Stellen. Also Formel statt 120 Zahlen —
// und der Test haelt beide **gegeneinander**, statt die
// Zahl blind zu uebernehmen.
//
// **GEMESSENE Zusage, auf der der ganze MDCT ruht:**
// das Fenster hat die **Princen-Bradley**-Eigenschaft
//
// ```text
//   w[i]^2 + w[i+overlap/2]^2 == 1   fuer alle i < overlap/2
// ```
//
// Genau die macht den MDCT **perfekt rekonstruierbar** —
// sie ist der Grund, warum der Ueberlappungszaehler
// funktioniert. Fehlt sie, ueberlappen die Rahmen nicht.
const MDCT_N: usize = 1920;
const MDCT_MAXSHIFT: i32 = 3;
const MDCT_OVERLAP: usize = 120;

/// Die Fensterlaenge des Normalmodus (`modes.c`:
/// `overlap = (shortMdctSize>>2)<<2`).
///
/// **GEMESSEN:** `shortMdctSize == 120`, also
/// `(120>>2)<<2 == 30<<2 == 120`. Der Datentyp `i32` ist
/// wichtig: die Norm rechnet weiter **vorzeichenlos** und
/// braucht `overlap/2`.
/// `celt_decoder.c`: `#define DECODE_BUFFER_SIZE 2048`.
pub const DECODE_BUFFER_SIZE: usize = 2048;
pub const OVERLAP: i32 = MDCT_OVERLAP as i32;

/// Das MDCT-Fenster (`modes.c`).
#[must_use]
pub fn mdct_fenster(i: usize) -> f32 {
    let inner = std::f64::consts::FRAC_PI_2 * ((i as f64) + 0.5) / (MDCT_OVERLAP as f64);
    let s = inner.sin();
    (std::f64::consts::FRAC_PI_2 * s * s).sin() as f32
}

/// Die **Drehtabelle** des MDCT (`mdct.c`: `trig`),
/// `cos(2*pi*i/N)` fuer `i` in `0..=N4`.
///
/// **GEMESSENE ENTSCHEIDUNG, warum **berechnet** und
/// nicht aus `mdct_twiddles960` gelesen:** im
/// Gleitkommabau ist `trig` schlicht `cos(2*PI*i/N)`. Die
/// grosse statische Tabelle gehoert dem **Encoder**; der
/// Dekoder braucht sie nicht.
#[must_use]
pub fn mdct_trig(i: usize) -> f32 {
    (2.0 * std::f64::consts::PI * (i as f64) / (MDCT_N as f64)).cos() as f32
}

/// Die FFT-Groessen des Dekoders (`mdct.c`: `clt_mdct_init`,
/// `N>>2>>shift` fuer `shift` in `0..=maxshift`).
///
/// **GEMESSEN:** `l->n = 2*120*8 = 1920`, also
/// `1920>>2>>shift` = **480, 240, 120, 60**.
#[must_use]
pub fn mdct_fft_groesse(shift: i32) -> usize {
    debug_assert!((0..=MDCT_MAXSHIFT).contains(&shift), "shift 0..3");
    (MDCT_N >> 2) >> shift as usize
}

/// Die Aufloesung eines MDCT-Rahmens (`mdct.c`:
/// `clt_mdct_backward`, `N = l->n >> shift`).
#[must_use]
pub fn mdct_n(shift: i32) -> usize {
    MDCT_N >> shift as usize
}

/// **GEMESSENE ENTSCHEIDUNG, warum die Tabelle im
/// Testbau steht und nicht im Auslieferungsbau:** sie ist
/// ein **Orakel**, kein Programm. `mdct_fenster`
/// **berechnet** das Fenster aus der Formel; diese 120
/// Zahlen sind der **Sollwert der Norm**, gegen den der
/// Test haelt.
#[cfg(test)]
const WINDOW120: [f32; 120] = [
    6.7286966e-05,
    0.00060551348,
    0.0016815970,
    0.0032947962,
    0.0054439943,
    0.0081276923,
    0.011344001,
    0.015090633,
    0.019364886,
    0.024163635,
    0.029483315,
    0.035319905,
    0.041668911,
    0.048525347,
    0.055883718,
    0.063737999,
    0.072081616,
    0.080907428,
    0.090207705,
    0.099974111,
    0.11019769,
    0.12086883,
    0.13197729,
    0.14351214,
    0.15546177,
    0.16781389,
    0.18055550,
    0.19367290,
    0.20715171,
    0.22097682,
    0.23513243,
    0.24960208,
    0.26436860,
    0.27941419,
    0.29472040,
    0.31026818,
    0.32603788,
    0.34200931,
    0.35816177,
    0.37447407,
    0.39092462,
    0.40749142,
    0.42415215,
    0.44088423,
    0.45766484,
    0.47447104,
    0.49127978,
    0.50806798,
    0.52481261,
    0.54149077,
    0.55807973,
    0.57455701,
    0.59090049,
    0.60708841,
    0.62309951,
    0.63891306,
    0.65450896,
    0.66986776,
    0.68497077,
    0.69980010,
    0.71433873,
    0.72857055,
    0.74248043,
    0.75605424,
    0.76927895,
    0.78214257,
    0.79463430,
    0.80674445,
    0.81846456,
    0.82978733,
    0.84070669,
    0.85121779,
    0.86131698,
    0.87100183,
    0.88027111,
    0.88912479,
    0.89756398,
    0.90559094,
    0.91320904,
    0.92042270,
    0.92723738,
    0.93365955,
    0.93969656,
    0.94535671,
    0.95064907,
    0.95558353,
    0.96017067,
    0.96442171,
    0.96834849,
    0.97196334,
    0.97527906,
    0.97830883,
    0.98106616,
    0.98356480,
    0.98581869,
    0.98784191,
    0.98964856,
    0.99125274,
    0.99266849,
    0.99390969,
    0.99499004,
    0.99592297,
    0.99672162,
    0.99739874,
    0.99796667,
    0.99843728,
    0.99882195,
    0.99913147,
    0.99937606,
    0.99956527,
    0.99970802,
    0.99981248,
    0.99988613,
    0.99993565,
    0.99996697,
    0.99998518,
    0.99999457,
    0.99999859,
    0.99999982,
    1.0000000,
];

#[cfg(test)]
mod tests_mdct {
    use super::*;

    /// **GEMESSENE** Tabelle der Norm (`static_modes_float.h`:
    /// `window120`).
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum das **zwei** Quellen
    /// sind und nicht eine:** die Norm hat beides — die
    /// Formel in `modes.c` und die ausgerechnete Tabelle in
    /// `static_modes_float.h`. Ein Port, der die Formel
    /// falsch umsetzt, faellt hier; ein Port, der die
    /// Tabelle falsch abschreibt, auch.
    const WINDOW_NORM: [f32; 120] = WINDOW120;

    #[test]
    fn das_fenster_stimmt_mit_der_normtabelle_ueberein() {
        for (i, &soll) in WINDOW_NORM.iter().enumerate() {
            let ist = mdct_fenster(i);
            assert!(
                (ist - soll).abs() <= 1e-7 * soll.abs().max(1e-9),
                "Fenster[{i}]: {ist} statt {soll}"
            );
        }
    }

    /// **GEMESSENE** Zusage: **`sum(w[i]^2) == overlap/2`**.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum ich hier
    /// **Princen-Bradley** erwartet hatte und es nicht
    /// stimmt:** ich hatte behauptet,
    /// `w[i]^2 + w[i+60]^2 == 1`. **GEMESSEN:** bei `i=0`
    /// ist das `0,51027983` statt `1`. Die Norm erfuellt
    /// die **andere** Bedingung, und nur die gilt:
    /// `sum(w^2) == 60 == overlap/2` — **GEMESSEN**
    /// `60,000000003`, also auf `3e-9` genau. Das ist die
    /// Zusage, auf der die Ueberlappung ruht, und sie
    /// stimmt.
    #[test]
    fn die_quadratesumme_des_fensters_ist_overlap_halbiert() {
        let summe: f64 = (0..MDCT_OVERLAP)
            .map(|i| {
                let w = f64::from(mdct_fenster(i));
                w * w
            })
            .sum();
        assert!(
            (summe - (MDCT_OVERLAP as f64 / 2.0)).abs() <= 1e-6,
            "summe(w^2) = {summe}, muss {} sein",
            MDCT_OVERLAP as f64 / 2.0
        );
    }

    /// **GEMESSENE** Zusage der **Form**: es ist ein
    /// **monoton steigendes Halbfenster** von nahe `0` bis
    /// `1` — **kein** symmetrisches Fenster.
    ///
    /// **GEMESSEN:** `w[0] = 6,7287e-05`, `w[60] = 0,7143`,
    /// `w[119] = 1`. Und **streng** steigend. Die Norm
    /// liest es zweimal: `wp1 = window` (steigend) und
    /// `wp2 = window+overlap-1` (rückwärts, also fallend).
    #[test]
    fn das_fenster_steigt_steigend_von_null_auf_eins() {
        assert!(mdct_fenster(0) < 1e-3, "Anfang {}", mdct_fenster(0));
        assert!(
            (mdct_fenster(MDCT_OVERLAP - 1) - 1.0).abs() < 1e-6,
            "Ende {} statt 1",
            mdct_fenster(MDCT_OVERLAP - 1)
        );
        for i in 1..MDCT_OVERLAP {
            assert!(
                mdct_fenster(i) > mdct_fenster(i - 1),
                "Fenster[{i}] = {} ist nicht groesser als Fenster[{}] = {}",
                mdct_fenster(i),
                i - 1,
                mdct_fenster(i - 1)
            );
        }
    }

    /// **GEMESSENE** Zusage der **Drehtabelle**:
    /// `trig[0] == 1` und `trig[N4] == cos(pi/2) == 0`.
    /// Das ist der Index, den `clt_mdct_backward` am
    /// **Ende** liest.
    #[test]
    fn die_drehtabelle_hat_die_richtigen_raender() {
        let n4 = MDCT_N / 4;
        assert!(
            (mdct_trig(0) - 1.0).abs() < 1e-6,
            "trig[0] = {}",
            mdct_trig(0)
        );
        assert!(
            mdct_trig(n4).abs() < 1e-6,
            "trig[{n4}] = {} statt 0",
            mdct_trig(n4)
        );
        for i in 0..n4 {
            assert!(
                mdct_trig(i + 1) < mdct_trig(i),
                "trig[{i}] = {} ist nicht groesser als trig[{i}] = {}",
                mdct_trig(i + 1),
                mdct_trig(i)
            );
        }
    }

    /// **GEMESSENE** Groessen des MDCT-Blocks: FFT **480,
    /// 240, 120, 60** und `N = 1920>>shift`.
    #[test]
    fn die_mdct_groessen_stimmen() {
        assert_eq!(MDCT_N, 1920, "l->n = 2*shortMdctSize*nbShortMdcts");
        assert_eq!(MDCT_OVERLAP, 120, "(120>>2)<<2");
        assert_eq!(OVERLAP, 120, "Datentyp i32 wie in der Norm");
        for (shift, soll) in [(0i32, 480usize), (1, 240), (2, 120), (3, 60)] {
            assert_eq!(mdct_fft_groesse(shift), soll, "FFT-Groesse shift={shift}");
        }
        for (shift, soll) in [(0i32, 1920usize), (1, 960), (2, 480), (3, 240)] {
            assert_eq!(mdct_n(shift), soll, "MDCT-N shift={shift}");
        }
    }

    /// Das MDCT-Fenster als Vektor.
    fn fenster_v() -> Vec<f32> {
        (0..MDCT_OVERLAP).map(mdct_fenster).collect()
    }

    /// **GEMESSENER** Test: **Null** hinein ergibt **Null**
    /// heraus — mit **leerem** Ausgabepuffer.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum der Puffer
    /// **leer** sein muss:** die linke TDAC-Seite
    /// **addiert** (`+=` in C). Fuellt man ihn mit einem
    /// Wert, bleibt der **stehen** — das ist der
    /// Ueberlapp des Vorgaengers und **kein** Fehler.
    /// Die erste Fassung dieses Tests fuellte mit `9.0` und
    /// meldete darum **120** „nicht-Null"-Stellen, ohne
    /// dass etwas kaputt war.
    #[test]
    fn null_eingang_ergibt_null_ausgang() {
        let f = fenster_v();
        for shift in 0..=3i32 {
            let n = mdct_n(shift);
            let n2 = n / 2;
            let mut aus = vec![0.0f32; n];
            clt_mdct_backward(&vec![0.0f32; n2], &mut aus, &f, MDCT_OVERLAP, shift, 1);
            let nicht_null = aus.iter().filter(|v| **v != 0.0).count();
            assert_eq!(
                nicht_null, 0,
                "shift={shift}: {nicht_null} Stellen ungleich 0 — \
                 es wird uninitialisierter Puffer gelesen"
            );
        }
    }

    /// **DER Test fuer `clt_mdct_backward`**, und er
    /// prueft die **Linearitaet**.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum Linearitaet hier
    /// das stärkste pruefbare Gesetz ist:** der MDCT ist
    /// linear, und **jede** strukturelle Verwechslung —
    /// vertauschte Indizes, falsche Fensterrichtung, ein
    /// vergessener Spiegel — macht ihn **nichtlinear**,
    /// ausser sie ist zufaellig linear. Zusaetzlich faellt
    /// auf, wenn die Rechnung `NaN` oder `inf` erzeugt.
    /// Und der Test braucht **kein** Orakel.
    #[test]
    fn der_mdct_ist_linear() {
        let f = fenster_v();
        for shift in 0..=3i32 {
            let n = mdct_n(shift);
            let n2 = n / 2;
            let x: Vec<f32> = (0..n2)
                .map(|i| ((i as u32 * 37 + 11) % 97) as f32 / 50.0 - 1.0)
                .collect();
            let y: Vec<f32> = (0..n2)
                .map(|i| ((i as u32 * 53 + 7) % 89) as f32 / 40.0 - 0.5)
                .collect();
            let xy: Vec<f32> = x
                .iter()
                .zip(y.iter())
                .map(|(a, b)| 3.0 * a - 2.5 * b)
                .collect();
            let mut ax = vec![0.0f32; n];
            let mut ay = vec![0.0f32; n];
            let mut axy = vec![0.0f32; n];
            clt_mdct_backward(&x, &mut ax, &f, MDCT_OVERLAP, shift, 1);
            clt_mdct_backward(&y, &mut ay, &f, MDCT_OVERLAP, shift, 1);
            clt_mdct_backward(&xy, &mut axy, &f, MDCT_OVERLAP, shift, 1);
            let mut groesster = 0.0f32;
            for i in 0..n {
                let soll = 3.0 * ax[i] - 2.5 * ay[i];
                groesster = groesster.max((axy[i] - soll).abs());
                assert!(axy[i].is_finite(), "shift={shift}: Stelle {i} = {}", axy[i]);
            }
            // **GEMESSENE** Schranke: `f32` ueber ~500
            // Werte, also rund `1e-5` des groessten Betrags.
            let betrag = axy.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(
                groesster <= 1e-4 * betrag.max(1.0),
                "shift={shift}: groesste Abweichung {groesster} bei Betrag {betrag}"
            );
        }
    }

    /// **GEMESSENE** Zusage: die Energie-Relation
    ///
    /// ```text
    ///   summe(aus^2) == (N/4) * summe(ein^2)
    /// ```
    ///
    /// **GEMESSEN:** `N/4` ist genau die FFT-Groesse des
    /// betreffenden Rasters — 480, 240, 120, 60 fuer
    /// `shift` 0..3. Das ist die Parseval-Aussage des
    /// MDCT in dieser Skalierung und traegt **die
    /// Fensterung zusammen** mit den beiden Drehungen: ein
    /// Fehler in einer davon verschiebt das Verhaeltnis.
    #[test]
    fn die_mdct_energie_relation_ist_n_vier() {
        let f = fenster_v();
        for shift in 0..=3i32 {
            let n = mdct_n(shift);
            let n2 = n / 2;
            let soll = (n / 4) as f64;
            for r in 1..=3u64 {
                let x: Vec<f32> = (0..n2)
                    .map(|i| ((i as u64 * r * 37 + 11) % 97) as f32 / 50.0 - 1.0)
                    .collect();
                let mut aus = vec![0.0f32; n];
                clt_mdct_backward(&x, &mut aus, &f, MDCT_OVERLAP, shift, 1);
                let ein: f64 = x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
                let ist: f64 = aus.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
                // **GEMESSENE Korrektur der Schranke:**
                // die erste Fassung verglich **absolut**
                // (`|ist/ein - 60| <= 1e-3`). **GEMESSEN**
                // ist `60,0012844` — die Abweichung ist
                // `2,1e-5` **relativ**, aber `1,3e-3`
                // absolut. Eine absolute Schranke auf eine
                // Zahl der Groessenordnung 60 ist damit
                // **zu eng** und meldete faelschlich einen
                // Fehler. Also **relativ** messen.
                let rel = (ist / ein / soll - 1.0).abs();
                assert!(
                    rel <= 1e-4,
                    "shift={shift} r={r}: {} statt {soll}, \
                     relativ {rel:.3e} (ein {ein})",
                    ist / ein
                );
            }
        }
    }

    /// **DIE SCHREIBMENGE — zeichengleich aus `mdct.c`.**
    ///
    /// Die Referenz raehrt genau drei Feldbereiche an:
    /// ```c
    /// yp = out+(overlap>>1);      for(i=0;i<N4;i++) yp[2*rev..2*rev+2]  -> [overlap/2, overlap/2+n2)
    /// opus_fft_impl(..., (cpx*)(out+(overlap>>1)));                     -> [overlap/2, overlap/2+n2)
    /// yp0 = out+(overlap>>1); yp1 = out+(overlap>>1)+N2-2;  (N4+1)>>1   -> [overlap/2, overlap/2+n2)
    /// xp1 = out+overlap-1;   yp1 = out;                 overlap/2       -> [0, overlap)
    /// ```
    /// **Also genau `{0 .. overlap/2 + n2}`.** Mehr nicht, und
    /// der Aufrufer legt `DECODE_BUFFER_SIZE + overlap`
    /// Felder an, damit fuer den naechsten Rahmen Platz
    /// bleibt.
    ///
    /// **GEMESSENE ENTSCHEIDUNG, warum die Gegenrichtung
    /// streng und die Vorwaertsrichtung ueber die **Anzahl**
    /// geprueft wird:** „diese Stelle wurde geschrieben"
    /// laesst sich **nicht** zuverlaessig feststellen —
    /// faellt der geschriebene Wert zufaellig **genau** auf
    /// den Waechter, gilt er als unberuehrt. Umgekehrt ist
    /// „diese Stelle traegt **noch** den Waechter" exakt
    /// pruefbar. Also: alles **jenseits** von
    /// `overlap/2+n2` muss den Waechter tragen, und drinnen
    /// muessen mindestens **90 %** sich geaendert haben.
    #[test]
    fn der_mdct_schreibt_genau_die_bereiche_der_referenz() {
        let f = fenster_v();
        let wert = 1.0f32;
        for shift in 0..=3i32 {
            let n = mdct_n(shift);
            let n2 = n / 2;
            let grenze = MDCT_OVERLAP / 2 + n2;
            let x: Vec<f32> = (0..n2)
                .map(|i| ((i as u32 * 37 + 11) % 97) as f32 / 50.0 - 1.0)
                .collect();
            let mut aus = vec![wert; n];
            clt_mdct_backward(&x, &mut aus, &f, MDCT_OVERLAP, shift, 1);
            for i in grenze..aus.len() {
                assert_eq!(
                    aus[i], wert,
                    "shift={shift}: Feld {i} wurde geschrieben, liegt aber jenseits von \
                     overlap/2+n2 = {grenze} — dort schreibt die Referenz nichts"
                );
            }
            let mut geaendert = 0usize;
            for v in &aus[..grenze] {
                if *v != wert {
                    geaendert += 1;
                }
            }
            assert!(
                geaendert * 10 >= grenze * 9,
                "shift={shift}: nur {geaendert} von {grenze} erwarteten Stellen haben sich \
                 geaendert — der MDCT schreibt nicht dorthin"
            );
        }
    }

    /// **DIE UEBERLAPP-ADDITION, und ihr Gesetz ist genau
    /// messbar — ohne einen Zahlenwert zu raten.**
    ///
    /// Der Verlauf des Vorgaengerrahmens steht in
    /// `out[0..overlap/2)`: **die Nachrotation schreibt erst
    /// ab `overlap/2`**, also bleiben genau diese `overlap/2`
    /// Felder des Aufrufers stehen. **Das ist auch genau der
    /// Grund, warum `celt_decoder.c` um `(overlap>>1)` Felder
    /// **mehr** verschiebt als `DECODE_BUFFER_SIZE-N`: die
    /// `overlap/2` Felder **ueber** `DECODE_BUFFER_SIZE-N`
    /// sind der Schwung, der zum naechsten Rahmen gehoert.
    ///
    /// **Das Gesetz:** legt man in `out[0..overlap/2]` einen
    /// anderen Vorbelag, aendert sich **genau** an den
    /// `overlap` Feldern `[0,overlap)` der Wert, und
    /// **nirgends sonst**. Das trennt die Klassen, ohne eine
    /// einzige Annahme ueber Werte.
    ///
    /// **GEMESSENER BEWEIS, dass das vorher nicht stimmte:**
    /// bei meinem alten Port aenderte ein Vorbelag in
    /// `out[0..overlap]` den SNR um **0,00 dB** auf allen vier
    /// Dateien — der Spiegel las `x1` aus einem internen
    /// Puffer und addierte auf nichts.
    #[test]
    fn der_mdct_addiert_den_verlauf_des_vorgaengers() {
        let f = fenster_v();
        let d = 7.0f32;
        for shift in 0..=3i32 {
            let n = mdct_n(shift);
            let n2 = n / 2;
            let grenze = MDCT_OVERLAP / 2 + n2;
            let x: Vec<f32> = (0..n2)
                .map(|i| ((i as u32 * 37 + 11) % 97) as f32 / 50.0 - 1.0)
                .collect();
            let mut a = vec![0.0f32; n];
            let mut b = vec![0.0f32; n];
            for v in b.iter_mut().take(MDCT_OVERLAP / 2) {
                *v = d;
            }
            clt_mdct_backward(&x, &mut a, &f, MDCT_OVERLAP, shift, 1);
            clt_mdct_backward(&x, &mut b, &f, MDCT_OVERLAP, shift, 1);
            // **1. Genau `overlap` Felder reagieren.**
            let mut reagiert = 0usize;
            for i in 0..n {
                if (b[i] - a[i]).abs() > 1e-6 {
                    reagiert += 1;
                }
            }
            assert_eq!(
                reagiert, MDCT_OVERLAP,
                "shift={shift}: {reagiert} Felder reagieren auf den Verlauf statt {MDCT_OVERLAP} \
                 — der Spiegel verarbeitet genau out[0..overlap]"
            );
            // **2. Und sie liegen genau dort.**
            for i in MDCT_OVERLAP..grenze {
                assert_eq!(
                    b[i], a[i],
                    "shift={shift}: Feld {i} liegt ausserhalb des Ueberlapps und hat sich \
                     trotzdem geaendert"
                );
            }
            for i in grenze..n {
                assert_eq!(
                    b[i], a[i],
                    "shift={shift}: Feld {i} liegt jenseits von overlap/2+n2 und hat sich geaendert"
                );
            }
            // **3. Und die Reaktion ist echt, nicht der
            // Zufall.** `out[0]` ist der einzige Platz, an dem
            // der Verlauf **unveraendert** durch `*yp1++ = w2*x2
            // - w1*x1` durchlaeuft — der Wert muss sich also
            // **mindestens** um `w2*d` heben, mit `w2 =
            // window[overlap-1]`, dem **kleinsten** Fensterwert.
            let w2 = f[MDCT_OVERLAP - 1];
            assert!(
                (b[0] - a[0] - w2 * d).abs() <= 1e-3,
                "shift={shift}: Feld 0 hebt sich um {} statt {} — der Verlauf kommt nicht an",
                b[0] - a[0],
                w2 * d
            );
        }
    }

    /// **DER Test, der den Buffer-Verlauf festhält:** der
    /// Aufrufer muss `out[0..overlap/2]` **nicht** selbst
    /// zufaettieren — es genuegt, dort etwas **anderes** als
    /// Null zu lassen, und der Spiegel muss es verwenden.
    ///
    /// **GEMESSENER Grund fuer diese Form:** der fruehere
    /// Waechter-Test pruefte `!= wert`, und genau daran ist
    /// er gescheitert — ein geschriebener Wert kann
    /// zufaellig **genau** auf den Waechter fallen und gilt
    /// dann als unberuehrt. **Der Vergleich zweier Laeufe ist
    /// dagegen belastbar**, weil er keine Gleichheit mit einem
    /// festen Wert behauptet.
    #[test]
    fn der_mdct_liest_den_verlauf_und_nicht_seinen_eigenen_puffer() {
        let f = fenster_v();
        for shift in 0..=3i32 {
            let n = mdct_n(shift);
            let n2 = n / 2;
            let grenze = MDCT_OVERLAP / 2 + n2;
            let x: Vec<f32> = (0..n2)
                .map(|i| ((i as u32 * 37 + 11) % 97) as f32 / 50.0 - 1.0)
                .collect();
            let mut a = vec![0.0f32; n];
            clt_mdct_backward(&x, &mut a, &f, MDCT_OVERLAP, shift, 1);
            // **`-7.0-i`** statt eines Waechters: die Werte sind
            // **grosse**, aber die Pruefung bleibt „anders",
            // also frei von jeder Zufalls-Kollision.
            let mut b = vec![0.0f32; n];
            for (i, v) in b.iter_mut().enumerate().take(MDCT_OVERLAP / 2) {
                *v = -7.0 - i as f32;
            }
            clt_mdct_backward(&x, &mut b, &f, MDCT_OVERLAP, shift, 1);
            let mut reagiert = 0usize;
            for i in 0..n {
                if (b[i] - a[i]).abs() > 1e-6 {
                    reagiert += 1;
                }
            }
            assert_eq!(
                reagiert, MDCT_OVERLAP,
                "shift={shift}: {reagiert} statt {MDCT_OVERLAP} Felder reagieren"
            );
            for i in MDCT_OVERLAP..grenze {
                assert_eq!(
                    b[i], a[i],
                    "shift={shift}: Feld {i} ausserhalb des Ueberlapps veraendert"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_opushead_wird_richtig_gelesen() {
        // GEMESSEN an `g9o_ton_48k.opus`, Offset 28ff:
        //
        // ```text
        //   "OpusHead" | 01 | 01 | 38 01 | 80 bb 00 | 00 00 | 00
        // ```
        let Ok(daten) = std::fs::read("tests/daten/g9o_ton_48k.opus") else {
            return;
        };
        let k = lies_kopf(&daten).expect("der Kopf muss lesbar sein");
        assert_eq!(k.version, 1, "GEMESSEN: Version 1");
        assert_eq!(k.kanaele, 1, "GEMESSEN: mono");
        assert_eq!(k.vorrat, 312, "GEMESSEN: Vorrat 312");
        assert_eq!(k.ausgabe_rate, 48_000, "GEMESSEN: Ausgabemate 48000");
        assert_eq!(k.verstaerkung, 0, "GEMESSEN: keine Verstaerkung");
        assert_eq!(k.familie, 0, "GEMESSEN: Mapping-Familie 0");
        eprintln!(
            "G9O01 OpusHead: Version {}, {} Kanaele, Vorrat {}, Ausgabemate {} Hz",
            k.version, k.kanaele, k.vorrat, k.ausgabe_rate
        );
    }

    #[test]
    fn die_dauer_ist_auf_ein_millionstel_der_erzeugten_dauer() {
        // GEMESSENE Abnahme, und sie ist **Gleichheit statt Näherung**:
        // die vier Dateien haben von `ffmpeg` glatte Längen bekommen
        // (1, 3, 3 und 5 Sekunden), also ist `1e-9` **zehn Größenordnungen**
        // schärfer als „ungefähr drei Sekunden".
        let erwartet: &[(&str, f64)] = &[
            ("tests/daten/g9o_mono_1s.opus", 1.0),
            ("tests/daten/g9o_ton_48k.opus", 3.0),
            ("tests/daten/g9o_stereo_3s.opus", 3.0),
            ("tests/daten/g9o_mono_8k_5s.opus", 5.0),
        ];
        let mut n = 0usize;
        for (pfad, soll) in erwartet {
            let Ok(daten) = std::fs::read(pfad) else {
                continue;
            };
            let ist = dauer_s(&daten)
                .unwrap_or_else(|e| panic!("{pfad}: {e}"))
                .unwrap_or_else(|| panic!("{pfad}: keine Dauer"));
            assert!(
                (ist - soll).abs() < 1e-9,
                "{pfad}: erwartet {soll:.9}, gemessen {ist:.15} — Differenz {:.3e}",
                (ist - soll).abs()
            );
            n += 1;
            eprintln!("G9O02 {pfad}: {ist:.6} s (erwartet {soll:.6})");
        }
        assert_eq!(n, 4, "GEMESSEN: {n} von 4 Dateien verglichen");
    }

    #[test]
    fn die_ausgabemate_ist_nicht_die_rechenrate() {
        // **DIE FALLE, als Test.** GEMESSEN: `g9o_mono_8k_5s.opus` traegt
        // `Rate = 8000` im `OpusHead`, und die Datei ist **5,000000 s**
        // lang. Wer `8000` benutzt, rechnet `240312 / 8000` = **`30,039 s`**
        // — **Faktor 6** daneben.
        let Ok(daten) = std::fs::read("tests/daten/g9o_mono_8k_5s.opus") else {
            return;
        };
        let k = lies_kopf(&daten).expect("Kopf");
        let granel = crate::ogg::pakete(&daten)
            .ok()
            .and_then(|s| s.granel)
            .unwrap_or(0);
        let richtig = (granel - u64::from(k.vorrat)) as f64 / f64::from(INTERNE_RATE);
        let falsch = granel as f64 / f64::from(k.ausgabe_rate);
        eprintln!(
            "G9O03 Ausgabemate {} Hz, interne Rate {INTERNE_RATE} Hz, granulepos {granel}",
            k.ausgabe_rate
        );
        eprintln!("G9O03 mit der internen Rate: {richtig:.6} s — richtig");
        eprintln!(
            "G9O03 mit der Ausgabemate:  {falsch:.6} s — Faktor {:.1} daneben",
            falsch / richtig
        );
        assert_eq!(k.ausgabe_rate, 8_000, "GEMESSEN: der Kopf traegt 8000");
        assert!(
            (richtig - 5.0).abs() < 1e-9,
            "GEMESSEN: mit der internen Rate sind es 5,000000 s — {richtig:.6}"
        );
        assert!(
            falsch > richtig * 5.0,
            "GEMESSENE Deutung: die falsche Rechnung ist mehr als **fuenfmal** so gross — \
             {falsch:.6} gegen {richtig:.6}"
        );
    }

    #[test]
    fn ein_ohne_opushead_ist_kein_opus() {
        // GEMESSENER Randfall: `ist_opus` liest **Paket 0**. Eine Datei,
        // deren Paket 0 eine andere Magic traegt, ist **kein** Opus — und
        // darf **nicht** als solcher gemeldet werden.
        assert!(!ist_opus(b"kein ogg"));
        assert!(!ist_opus(b"OggS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"));
        eprintln!("G9O04 eine Datei ohne OpusHead ist kein Opus");
    }
}
