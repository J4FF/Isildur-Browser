//! Grenzen der WebAssembly-Engine. M3/M4 aus `team/REGELN.txt`:
//! JEDE Groesse, die aus der Eingabe kommt, wird VOR der Allokation geprueft,
//! und jede Schleife ueber Eingaben hat eine harte Obergrenze als benannte
//! Konstante mit Begruendung.
//!
//! Doppelte Sicherung: `Grenzen` ist zur Laufzeit einstellbar (Web-Instanzen
//! duerfen kleine Limits setzen, damit eine Seite keine 4 GiB belegt), JEDER
//! Wert wird aber beim Bauen der `Grenzen` gegen eine absolute Konstante
//! hier gekappt. Ein Modul im Browser kann sich also keine grossen Limits
//! selbst bewilligen.

/// Maximale Zahl Sektionen in einem Modul. Spec erlaubt beliebig viele
/// benutzerdefinierte Sektionen; 4096 reicht fuer echte Werkzeuge (wasi-sdk
/// liefert ~10) und haelt den Parser in < 1 s (M8).
pub const MAX_SEKTIONEN_HART: usize = 4096;
/// Maximale Zahl Funktionen je Modul. Chrome selbst haelt 1 000 000 ein,
/// wir nehmen deutlich weniger: der Interpreter laeuft ohne JIT.
pub const MAX_FUNKTIONEN_HART: usize = 200_000;
/// Maximale Zahl Typen je Modul (Fremdtypen jeder Import brauchen einen).
pub const MAX_TYPEN_HART: usize = 100_000;
/// Maximale Zahl Lokalvariablen je Funktion (Local-Set Angriff: 2^32 Eintraege).
pub const MAX_LOKALEN_HART: usize = 50_000;
/// Maximale Laenge einer Namenszeile (Export/Import/Custom-Sektion).
/// 64 KiB reicht fuer jedes Werkzeug; darueber ist es ein Angriff.
pub const MAX_NAME_BYTES_HART: usize = 65_536;
/// Maximale Groesse des Codes einer Funktion in Bytes. Ohne diese Grenze
/// alloziert ein Modul mit `end`-SchleifeGB an Befehlen (M4).
pub const MAX_CODE_BYTES_HART: usize = 4 * 1024 * 1024;
/// Maximale Zahl Befehle je Funktion (schuetzt vor 1-Byte-Befehlsfluten).
pub const MAX_BEFEHLE_HART: usize = 2 * 1024 * 1024;
/// Maximale Zahl der Elemente in EINEM GC-Array (Wasm 3.0 GC).
///
/// GEMESSEN 03.10.: ohne diese Grenze beendete `array.new_default` mit
/// der Laenge `0xffff_ffff` den ganzen Prozess (SIGKILL, OOM). Der
/// Wert 4 Mi ist die groesste, die die Testsuite braucht (dort 108),
/// und ergibt rund 64 MB im schlimmsten Fall.
pub const MAX_GC_ELEMENTS_HART: usize = 4 * 1024 * 1024;

/// Maximale Zahl der Array-Elemente **aller** GC-Arrays einer Instanz
/// zusammen — der Haufen waechst sonst unbegrenzt (eine Schleife, die
/// `array.new` aufruft).
///
/// GEMESSEN 03.10. (KOORDINATION): der Bombentest brauchte 4,5 GB.
pub const MAX_GC_SLOTS_HART: usize = 4 * 1024 * 1024;

/// Maximale Zahl der GC-Objekte (Arrays, Strukturen, `ref.i31`) einer
/// Instanz. Jedes Objekt kostet auch ohne Elemente seinen Platz.
pub const MAX_GC_OBJEKTE_HART: usize = 1_000_000;

/// Maximale Verschachtelungstiefe (block/loop/if). M5: Rekursion bzw.
/// verschachtelte Spruenge ohne Grenze = Stackueberlauf.
///
/// GEMESSEN 01.10.: Der Textleser baut einen Baum, und BEIM ABBAUEN dieses
/// Baums laeuft der Drop-Programmcode ueber die Tiefe — in einem 2-MiB-
/// Thread (so laeuft `cargo test`) reissen 600 verschachtelte `block`,
/// 400 noch nicht. Der Wert 256 ist deshalb rund 6-mal so tief wie die
/// tiefste Datei der offiziellen Testsuite (`loop.wast` = 43) und laesst
/// genug Reserve fuer jedes Werkzeug. Der INTERPRETER ist davon nicht
/// betroffen: sein Rahmenstapel ist ein `Vec` (keine Rust-Rekursion).
pub const MAX_VERSCHACHTELUNG_HART: usize = 256;
/// Maximale Zahl Tabellen / Speicher / Globale je Modul. Ohne
/// Multi-Memory (Wasm 3.0, nicht 2.0) ist Speicher auf 1 begrenzt.
pub const MAX_TABELLEN_HART: usize = 10_000;
pub const MAX_GLOBALE_HART: usize = 100_000;
/// Maximale Zahl Exports/Imports je Modul.
pub const MAX_EXPORTE_HART: usize = 20_000;
pub const MAX_IMPORTE_HART: usize = 20_000;
/// Maximale Zahl Element-/Datenabschnitte je Modul.
pub const MAX_ELEMENTE_HART: usize = 100_000;
pub const MAX_DATEN_HART: usize = 100_000;
/// Maximale Eintragszahl in einer Tabelle.
///
/// Die Spec erlaubt 2^32-1 Eintraege; das sind bei 16 Byte je Eintrag
/// 64 GiB — das kann keine Engine in einem Prozess mit 2-6 GiB
/// bereitstellen. Deshalb ist die HARTE Grenze deutlich kleiner, und die
/// Voreinstellung noch einmal zehnmal kleiner.
///
/// GEMESSEN 01.10. (`table.wast`): `(table 0xffff_ffff funcref)` liess vorher
/// 64 GiB anfordern und der Prozess lief in einen Abbruch (Exit 1, kein
/// Fehler). Jetzt: benannter Linkfehler „Tabelle zu gross" — genau das, was
/// die Testsuite erwartet (`assert_unlinkable`).
pub const MAX_TABELLENPLAETZE_HART: u64 = 1 << 22; // 4 194 304 Plaetze
/// Voreinstellung fuer Web-Instanzen: 100 000 Eintraege (16 MB).
pub const MAX_TABELLENPLAETZE_VORGABE: u64 = 100_000;
/// Maximale Zahl WebAssembly-Seiten (64 KiB je Seite). 65536 Seiten = 4 GiB,
/// das ist die Spec-Grenze fuer 32-bit-Speicher. Unsere Prozessgrenze ist
/// 6 GiB (werkzeuge/bau.sh), deshalb ist der Vorgabewert 4096 Seiten =
/// 256 MiB; eine Web-Instanz darf hoeher, aber nie ueber die Spec-Grenze.
pub const MAX_SEITEN_HART: u64 = 65_536;
/// Voreinstellung: 256 MiB Speicher je Modul (M4, Kompressionsbomben-
/// Analogie: entpackte Groesse deckeln).
pub const MAX_SEITEN_VORGABE: u64 = 4096;
/// Maximales Wachstum einer Tabelle je `table.grow`-Aufruf (sonst alloziert
/// ein Modul in einem Aufruf GiB).
pub const MAX_WACHSTUM_HART: u64 = 1 << 24;
/// Brennstoff je Aufruf: Zahl der ausgefuehrten Befehle, danach Trap.
/// 100 Mio Befehle sind in der Optimierung ~1 s, im Debug-Bau ~30 s; wir
/// nehmen 50 Mio als Standard und testen mit kleinen Werten (M2/M3).
pub const MAX_BRENNSTOFF_HART: u64 = 4_000_000_000;
pub const MAX_BRENNSTOFF_VORGABE: u64 = 50_000_000;
/// Maximale Aufruftiefe (WASM-Rahmen liegen im `Vec`, nicht im Rust-Stapel,
/// aber die Verschachtelung von `call_indirect`-Ketten soll begrenzt sein).
pub const MAX_AUFRUFTIEFE_HART: usize = 10_000;
/// Maximale Zahl gleichzeitig offener Fremdaufrufe (Hostfunktionen).
pub const MAX_FREMDOFFEN_HART: usize = 1024;
/// Maximale Einbettungstiefe: eine Hostfunktion darf wieder WASM aufrufen.
/// Dieser Weg nutzt den RUST-Stapel (ein verschachtelter Lauf je Ebene),
/// deshalb ist er streng begrenzt (M5).
pub const MAX_EINBETTUNG_HART: usize = 64;
/// Maximale Groesse eines .wast-Skripts, das der Leser annimmt.
pub const MAX_WAST_BYTES_HART: usize = 16 * 1024 * 1024;
/// Maximale Zahl Befehle (assertions) in einem .wast-Skript.
pub const MAX_WAST_BEFEHLE_HART: usize = 200_000;
/// Maximale Zeilen in einer DWARF-Zeilentabelle (custom sections).
pub const MAX_DWARF_ZEILEN_HART: usize = 200_000;
/// Maximale Sektionen/Records beim DWARF-Lesen.
pub const MAX_DWARF_RECORDS_HART: usize = 1_000_000;
/// Maximale Schritte in einer Debugger-Spur. Jeder Schritt speichert eine
/// Kopie von Lokalen und Operandstapel — ohne diese Grenze waere ein
/// Endlosschleifen-Modul ein Speicherfresser (M4).
pub const MAX_DEBUG_SCHRITTE_HART: usize = 2_000_000;
/// Vorgabe: 200 000 Schritte (reicht fuer die Entwicklerwerkzeuge).
pub const MAX_DEBUG_SCHRITTE_VORGABE: usize = 200_000;

/// Alle Grenzen einer Engine. Kopieren erlaubt (`#[derive(Clone)]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grenzen {
    /// Hoechstlaenges Modul in Bytes (vor der Allokation geprueft, M4).
    pub modul_bytes: usize,
    pub sektionen: usize,
    pub typen: usize,
    pub funktionen: usize,
    pub lokale_je_funktion: usize,
    pub name_bytes: usize,
    pub code_bytes: usize,
    pub befehle_je_funktion: usize,
    pub verschachtelung: usize,
    pub tabellen: usize,
    pub globale: usize,
    pub exporte: usize,
    pub importe: usize,
    pub elementabschnitte: usize,
    pub datenabschnitte: usize,
    pub tabelle_plaetze: u64,
    pub speicher_seiten: u64,
    pub wachstum_je_aufruf: u64,
    pub brennstoff: u64,
    pub aufruftiefe: usize,
    pub fremd_offen: usize,
    pub einbettung: usize,
    pub wast_bytes: usize,
    pub wast_befehle: usize,
    pub dwarf_zeilen: usize,
    pub dwarf_records: usize,
    /// W5: Schritte in einer Debugger-Spur.
    pub debug_schritte: usize,
}

impl Grenzen {
    /// Vorgaben fuer den Browser: grosszuegig, aber von der Maschine
    /// (6 GiB-Prozessgrenze, GEMESSEN bau.sh) weit entfernt.
    pub fn standard() -> Self {
        Self::neu(MAX_SEITEN_VORGABE, MAX_BRENNSTOFF_VORGABE)
    }

    /// Lueckenlose, strenge Grenzen fuer Tests. Ein Modul mit 3 Funktionen
    /// passt, eins mit 4 nicht.
    pub fn eng() -> Self {
        Self {
            modul_bytes: 1 << 20,
            sektionen: 64,
            typen: 32,
            funktionen: 16,
            lokale_je_funktion: 8,
            name_bytes: 256,
            code_bytes: 4096,
            befehle_je_funktion: 2048,
            verschachtelung: 8,
            tabellen: 4,
            globale: 8,
            exporte: 16,
            importe: 16,
            elementabschnitte: 8,
            datenabschnitte: 8,
            tabelle_plaetze: 16,
            speicher_seiten: 2,
            wachstum_je_aufruf: 4,
            brennstoff: 20_000,
            aufruftiefe: 32,
            fremd_offen: 8,
            einbettung: 4,
            wast_bytes: 1 << 20,
            wast_befehle: 2_000,
            dwarf_zeilen: 256,
            dwarf_records: 4_096,
            debug_schritte: 2_000,
        }
        .gekappt()
    }

    ///Fuer Bomben-Tests: so klein wie noch sinnvoll.
    pub fn bombe() -> Self {
        let mut g = Self::eng();
        g.modul_bytes = 1024;
        g.code_bytes = 512;
        g.befehle_je_funktion = 64;
        g.brennstoff = 1_000;
        g.speicher_seiten = 1;
        g
    }

    /// Nur Speicher und Brennstoff aendern (Web-Instanzen).
    pub fn neu(speicher_seiten: u64, brennstoff: u64) -> Self {
        Self {
            modul_bytes: 64 * 1024 * 1024,
            sektionen: MAX_SEKTIONEN_HART,
            typen: MAX_TYPEN_HART,
            funktionen: MAX_FUNKTIONEN_HART,
            lokale_je_funktion: MAX_LOKALEN_HART,
            name_bytes: MAX_NAME_BYTES_HART,
            code_bytes: MAX_CODE_BYTES_HART,
            befehle_je_funktion: MAX_BEFEHLE_HART,
            verschachtelung: MAX_VERSCHACHTELUNG_HART,
            tabellen: MAX_TABELLEN_HART,
            globale: MAX_GLOBALE_HART,
            exporte: MAX_EXPORTE_HART,
            importe: MAX_IMPORTE_HART,
            elementabschnitte: MAX_ELEMENTE_HART,
            datenabschnitte: MAX_DATEN_HART,
            tabelle_plaetze: MAX_TABELLENPLAETZE_VORGABE,
            speicher_seiten,
            wachstum_je_aufruf: MAX_WACHSTUM_HART,
            brennstoff,
            aufruftiefe: MAX_AUFRUFTIEFE_HART,
            fremd_offen: MAX_FREMDOFFEN_HART,
            einbettung: MAX_EINBETTUNG_HART,
            wast_bytes: MAX_WAST_BYTES_HART,
            wast_befehle: MAX_WAST_BEFEHLE_HART,
            dwarf_zeilen: MAX_DWARF_ZEILEN_HART,
            dwarf_records: MAX_DWARF_RECORDS_HART,
            debug_schritte: MAX_DEBUG_SCHRITTE_VORGABE,
        }
        .gekappt()
    }

    /// Alle Werte gegen die absoluten Konstanten kappen. Wer eine
    /// `Grenzen` von Hand baut (Struktur-Literal) ist damit abgesichert.
    pub fn gekappt(mut self) -> Self {
        self.modul_bytes = self.modul_bytes.min(MAX_WAST_BYTES_HART);
        self.sektionen = self.sektionen.min(MAX_SEKTIONEN_HART);
        self.typen = self.typen.min(MAX_TYPEN_HART);
        self.funktionen = self.funktionen.min(MAX_FUNKTIONEN_HART);
        self.lokale_je_funktion = self.lokale_je_funktion.min(MAX_LOKALEN_HART);
        self.name_bytes = self.name_bytes.min(MAX_NAME_BYTES_HART);
        self.code_bytes = self.code_bytes.min(MAX_CODE_BYTES_HART);
        self.befehle_je_funktion = self.befehle_je_funktion.min(MAX_BEFEHLE_HART);
        self.verschachtelung = self.verschachtelung.min(MAX_VERSCHACHTELUNG_HART);
        self.tabellen = self.tabellen.min(MAX_TABELLEN_HART);
        self.globale = self.globale.min(MAX_GLOBALE_HART);
        self.exporte = self.exporte.min(MAX_EXPORTE_HART);
        self.importe = self.importe.min(MAX_IMPORTE_HART);
        self.elementabschnitte = self.elementabschnitte.min(MAX_ELEMENTE_HART);
        self.datenabschnitte = self.datenabschnitte.min(MAX_DATEN_HART);
        self.tabelle_plaetze = self.tabelle_plaetze.min(MAX_TABELLENPLAETZE_HART);
        self.speicher_seiten = self.speicher_seiten.min(MAX_SEITEN_HART);
        self.wachstum_je_aufruf = self.wachstum_je_aufruf.min(MAX_WACHSTUM_HART);
        self.brennstoff = self.brennstoff.min(MAX_BRENNSTOFF_HART);
        self.aufruftiefe = self.aufruftiefe.min(MAX_AUFRUFTIEFE_HART);
        self.fremd_offen = self.fremd_offen.min(MAX_FREMDOFFEN_HART);
        self.einbettung = self.einbettung.min(MAX_EINBETTUNG_HART);
        self.wast_bytes = self.wast_bytes.min(MAX_WAST_BYTES_HART);
        self.wast_befehle = self.wast_befehle.min(MAX_WAST_BEFEHLE_HART);
        self.dwarf_zeilen = self.dwarf_zeilen.min(MAX_DWARF_ZEILEN_HART);
        self.dwarf_records = self.dwarf_records.min(MAX_DWARF_RECORDS_HART);
        self.debug_schritte = self.debug_schritte.min(MAX_DEBUG_SCHRITTE_HART);
        self
    }

    /// Seiten in Bytes. Der Aufrufer muss das Ergebnis gegen sein eigenes
    /// Limit pruefen (Multiplikation kann sonst reissen).
    pub fn speicher_bytes(&self) -> u64 {
        self.speicher_seiten.saturating_mul(65_536)
    }
}

impl Default for Grenzen {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod einheit {
    use super::*;

    /// M7: Jeder Test sichert ein Ergebnis zu.
    #[test]
    fn kappung_haelt_hart_grenzen_ein() {
        let g = Grenzen::neu(u64::MAX, u64::MAX);
        assert_eq!(g.speicher_seiten, MAX_SEITEN_HART);
        assert_eq!(g.brennstoff, MAX_BRENNSTOFF_HART);
        let g2 = Grenzen::bombe();
        assert_eq!(g2.brennstoff, 1_000);
        assert_eq!(g2.speicher_seiten, 1);
    }

    #[test]
    fn eng_lehnt_vier_funktionen_ab() {
        let g = Grenzen::eng();
        assert!(3 <= g.funktionen);
        assert_eq!(g.funktionen, 16);
        assert_eq!(Grenzen::standard().speicher_bytes(), 4096 * 65_536);
    }
}
