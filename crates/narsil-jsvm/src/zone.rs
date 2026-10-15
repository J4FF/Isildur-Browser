//! J24: die **lokale Zeitzone des Hosts** — `getTimezoneOffset()` und die
//! lokalen `get*`/`set*`-Natives rechnen jetzt wirklich lokal.
//!
//! # WARUM EIGENTLICH SELBST GELESEN
//!
//! GEMESSEN (node 22 auf dieser Maschine, `new Date(0).getTimezoneOffset()`):
//! `-60` — der Motor lieferte `0`. Die Zone kommt aus
//! `/etc/localtime -> /usr/share/zoneinfo/Europe/Berlin`, einem **TZif**-
//! Binärfile (RFC 8536). Ohne `unsafe` gibt es in der Standardbibliothek
//! keine Zonenabfrage; `std` liest Dateien, also liest der Motor die Datei
//! selbst: 100 Zeilen, keine Fremdcrate, keine `unsafe`.
//!
//! # WAS GELIESEN WIRD
//!
//! * `TZ` aus der Umgebung, wenn gesetzt und unter
//!   `/usr/share/zoneinfo/<name>` zu finden (das, was node tut);
//! * sonst `/etc/localtime`;
//! * Version 2/3/4: der **64-Bit-Block** wird benutzt (der 32-Bit-Block
//!   deckt nur 1901–2037 ab — `new Date(0)` faellt also sonst auf die
//!   Vorgabe zurueck).
//!
//! # DIE REGEL VOR DER ERSTEN UMSTELLUNG
//!
//! Vor dem ersten Uebergang gilt laut tzfile-Spezifikation der erste **nicht**
//! in der Sommerzeit liegende Typ (sonst Typ 0). Genau daran lag der
//! Unterschied zu node bei `new Date(0)`.

use std::sync::RwLock;

/// Eine Zone: die Uebergänge (Zeit -> Verschiebung), die **Kürzel** der
/// Typen und die Vorgabe davor.
#[derive(Debug, Default)]
struct Zone {
    /// `(Zeitpunkt in ms, Verschiebung in Sekunden)`, aufsteigend.
    uebergaenge: Vec<(i64, i32, usize)>,
    /// Typ-Index -> Kürzel (`CET`, `CEST`, …) — das steht im Dateihead und
    /// ist der Teil von `toString()`, den der Motor wirklich liefern kann
    /// (node schreibt den **ausgeschriebenen** localized Namen, das ist eine
    /// ICU-Sache — siehe der Modulkopf).
    desigs: Vec<String>,
    /// Verschiebung **vor** dem ersten Uebergang.
    vorgabe: i32,
    vorgabe_typ: usize,
}

impl Zone {
    /// Der Index des Typs zur Zeit `t` (ms) — der letzte Uebergang mit
    /// `zeit <= t` gewinnt, davor die Vorgabe.
    fn typ_bei(&self, t: i64) -> usize {
        // Die Uebergangstabelle traegt parallel `typen` (siehe `block_lesen`).
        match self.uebergaenge.binary_search_by(|p| p.0.cmp(&t)) {
            Ok(i) => self.uebergaenge[i].2,
            Err(0) => self.vorgabe_typ,
            Err(i) => self.uebergaenge[i - 1].2,
        }
    }

    /// Das Kuerzel eines Typs (Index aus `typen`, Werte aus `kennungen`).
    fn typ_desig(&self, typ: usize) -> Option<String> {
        self.desigs.get(typ).cloned().filter(|s| !s.is_empty())
    }

    /// Die Verschiebung (Sekunden **oestlich** von UTC) zur Zeit `t` (ms).
    fn bei(&self, t: i64) -> i32 {
        match self.uebergaenge.binary_search_by(|p| p.0.cmp(&t)) {
            Ok(i) => self.uebergaenge[i].1,
            Err(0) => self.vorgabe,
            Err(i) => self.uebergaenge[i - 1].1,
        }
    }
}

/// Die Zone des Hosts — **einmal** gelesen, danach aus dem Speicher.
///
/// Release 10.10.2026: ein `RwLock` statt `OnceLock`, damit der Host die
/// Zone SETZEN kann (`lokale_zone_setzen`) — unter Windows gibt es weder
/// `TZ` noch `/etc/localtime`, die Zone kennt nur das System (die FFI liegt
/// in `narsil-window`, ADR-020). Eine gesetzte Zone wird geleakt (`'static`,
/// hoechstens eine je Setzen).
static ZONE: RwLock<Option<&'static Zone>> = RwLock::new(None);

/// Die lokale Zone (der Fall "keine Zone lesbar" ergibt UTC).
fn zone() -> &'static Zone {
    if let Some(z) = *ZONE.read().unwrap_or_else(|e| e.into_inner()) {
        return z;
    }
    let mut w = ZONE.write().unwrap_or_else(|e| e.into_inner());
    *w.get_or_insert_with(|| Box::leak(Box::new(zone_lesen().unwrap_or_default())))
}

/// Release 10.10.2026: die lokale Zone vom Host setzen (IANA-Name, z. B.
/// `Europe/Berlin`), gelesen aus `/usr/share/zoneinfo` oder den
/// eingebetteten Daten (`tzdata.rs`). `TZ` aus der Umgebung geht vor (wie
/// bei node) — dann aendert sich nichts. `false`, wenn der Name unbekannt
/// ist oder `TZ` gilt.
///
/// GEMESSEN vorher (Windows, node 22 in Klammern):
/// `new Date(0).getTimezoneOffset()` -> `0`  (`-60`), jede lokale Uhrzeit
/// einer Seite stand in UTC.
pub fn lokale_zone_setzen(iana: &str) -> bool {
    if std::env::var_os("TZ").is_some_and(|t| !t.is_empty()) {
        return false;
    }
    let Some(z) = benannte_daten(iana).and_then(|d| tzif_lesen(&d)) else {
        return false;
    };
    *ZONE.write().unwrap_or_else(|e| e.into_inner()) = Some(Box::leak(Box::new(z)));
    true
}

/// Release 10.10.2026: ein Windows-Zonenschluessel (`TimeZoneKeyName`, z. B.
/// `W. Europe Standard Time`) als IANA-Name — die Zuordnung von CLDR
/// `windowsZones.xml`, Gebiet `001` (der Vertreter je Zone), mit den
/// heutigen IANA-Namen der eingebetteten Daten (`Asia/Kolkata`, nicht
/// `Asia/Calcutta`). Jeder Name steht in `tzdata.rs`.
pub fn windows_zone_iana(schluessel: &str) -> Option<&'static str> {
    const TABELLE: &[(&str, &str)] = &[
        ("AUS Central Standard Time", "Australia/Darwin"),
        ("AUS Eastern Standard Time", "Australia/Sydney"),
        ("Afghanistan Standard Time", "Asia/Kabul"),
        ("Alaskan Standard Time", "America/Anchorage"),
        ("Aleutian Standard Time", "America/Adak"),
        ("Altai Standard Time", "Asia/Barnaul"),
        ("Arab Standard Time", "Asia/Riyadh"),
        ("Arabian Standard Time", "Asia/Dubai"),
        ("Arabic Standard Time", "Asia/Baghdad"),
        ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
        ("Astrakhan Standard Time", "Europe/Astrakhan"),
        ("Atlantic Standard Time", "America/Halifax"),
        ("Aus Central W. Standard Time", "Australia/Eucla"),
        ("Azerbaijan Standard Time", "Asia/Baku"),
        ("Azores Standard Time", "Atlantic/Azores"),
        ("Bahia Standard Time", "America/Bahia"),
        ("Bangladesh Standard Time", "Asia/Dhaka"),
        ("Belarus Standard Time", "Europe/Minsk"),
        ("Bougainville Standard Time", "Pacific/Bougainville"),
        ("Canada Central Standard Time", "America/Regina"),
        ("Cape Verde Standard Time", "Atlantic/Cape_Verde"),
        ("Caucasus Standard Time", "Asia/Yerevan"),
        ("Cen. Australia Standard Time", "Australia/Adelaide"),
        ("Central America Standard Time", "America/Guatemala"),
        ("Central Asia Standard Time", "Asia/Almaty"),
        ("Central Brazilian Standard Time", "America/Cuiaba"),
        ("Central Europe Standard Time", "Europe/Budapest"),
        ("Central European Standard Time", "Europe/Warsaw"),
        ("Central Pacific Standard Time", "Pacific/Guadalcanal"),
        ("Central Standard Time", "America/Chicago"),
        ("Central Standard Time (Mexico)", "America/Mexico_City"),
        ("Chatham Islands Standard Time", "Pacific/Chatham"),
        ("China Standard Time", "Asia/Shanghai"),
        ("Cuba Standard Time", "America/Havana"),
        ("Dateline Standard Time", "Etc/GMT+12"),
        ("E. Africa Standard Time", "Africa/Nairobi"),
        ("E. Australia Standard Time", "Australia/Brisbane"),
        ("E. Europe Standard Time", "Europe/Chisinau"),
        ("E. South America Standard Time", "America/Sao_Paulo"),
        ("Easter Island Standard Time", "Pacific/Easter"),
        ("Eastern Standard Time", "America/New_York"),
        ("Eastern Standard Time (Mexico)", "America/Cancun"),
        ("Egypt Standard Time", "Africa/Cairo"),
        ("Ekaterinburg Standard Time", "Asia/Yekaterinburg"),
        ("FLE Standard Time", "Europe/Kyiv"),
        ("Fiji Standard Time", "Pacific/Fiji"),
        ("GMT Standard Time", "Europe/London"),
        ("GTB Standard Time", "Europe/Bucharest"),
        ("Georgian Standard Time", "Asia/Tbilisi"),
        ("Greenland Standard Time", "America/Nuuk"),
        ("Greenwich Standard Time", "Atlantic/Reykjavik"),
        ("Haiti Standard Time", "America/Port-au-Prince"),
        ("Hawaiian Standard Time", "Pacific/Honolulu"),
        ("India Standard Time", "Asia/Kolkata"),
        ("Iran Standard Time", "Asia/Tehran"),
        ("Israel Standard Time", "Asia/Jerusalem"),
        ("Jordan Standard Time", "Asia/Amman"),
        ("Kaliningrad Standard Time", "Europe/Kaliningrad"),
        ("Korea Standard Time", "Asia/Seoul"),
        ("Libya Standard Time", "Africa/Tripoli"),
        ("Line Islands Standard Time", "Pacific/Kiritimati"),
        ("Lord Howe Standard Time", "Australia/Lord_Howe"),
        ("Magadan Standard Time", "Asia/Magadan"),
        ("Magallanes Standard Time", "America/Punta_Arenas"),
        ("Marquesas Standard Time", "Pacific/Marquesas"),
        ("Mauritius Standard Time", "Indian/Mauritius"),
        ("Middle East Standard Time", "Asia/Beirut"),
        ("Montevideo Standard Time", "America/Montevideo"),
        ("Morocco Standard Time", "Africa/Casablanca"),
        ("Mountain Standard Time", "America/Denver"),
        ("Mountain Standard Time (Mexico)", "America/Mazatlan"),
        ("Myanmar Standard Time", "Asia/Yangon"),
        ("N. Central Asia Standard Time", "Asia/Novosibirsk"),
        ("Namibia Standard Time", "Africa/Windhoek"),
        ("Nepal Standard Time", "Asia/Kathmandu"),
        ("New Zealand Standard Time", "Pacific/Auckland"),
        ("Newfoundland Standard Time", "America/St_Johns"),
        ("Norfolk Standard Time", "Pacific/Norfolk"),
        ("North Asia East Standard Time", "Asia/Irkutsk"),
        ("North Asia Standard Time", "Asia/Krasnoyarsk"),
        ("North Korea Standard Time", "Asia/Pyongyang"),
        ("Omsk Standard Time", "Asia/Omsk"),
        ("Pacific SA Standard Time", "America/Santiago"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("Pacific Standard Time (Mexico)", "America/Tijuana"),
        ("Pakistan Standard Time", "Asia/Karachi"),
        ("Paraguay Standard Time", "America/Asuncion"),
        ("Qyzylorda Standard Time", "Asia/Qyzylorda"),
        ("Romance Standard Time", "Europe/Paris"),
        ("Russia Time Zone 10", "Asia/Srednekolymsk"),
        ("Russia Time Zone 11", "Asia/Kamchatka"),
        ("Russia Time Zone 3", "Europe/Samara"),
        ("Russian Standard Time", "Europe/Moscow"),
        ("SA Eastern Standard Time", "America/Cayenne"),
        ("SA Pacific Standard Time", "America/Bogota"),
        ("SA Western Standard Time", "America/La_Paz"),
        ("SE Asia Standard Time", "Asia/Bangkok"),
        ("Saint Pierre Standard Time", "America/Miquelon"),
        ("Sakhalin Standard Time", "Asia/Sakhalin"),
        ("Samoa Standard Time", "Pacific/Apia"),
        ("Sao Tome Standard Time", "Africa/Sao_Tome"),
        ("Saratov Standard Time", "Europe/Saratov"),
        ("Singapore Standard Time", "Asia/Singapore"),
        ("South Africa Standard Time", "Africa/Johannesburg"),
        ("South Sudan Standard Time", "Africa/Juba"),
        ("Sri Lanka Standard Time", "Asia/Colombo"),
        ("Sudan Standard Time", "Africa/Khartoum"),
        ("Syria Standard Time", "Asia/Damascus"),
        ("Taipei Standard Time", "Asia/Taipei"),
        ("Tasmania Standard Time", "Australia/Hobart"),
        ("Tocantins Standard Time", "America/Araguaina"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("Tomsk Standard Time", "Asia/Tomsk"),
        ("Tonga Standard Time", "Pacific/Tongatapu"),
        ("Transbaikal Standard Time", "Asia/Chita"),
        ("Turkey Standard Time", "Europe/Istanbul"),
        ("Turks And Caicos Standard Time", "America/Grand_Turk"),
        ("US Eastern Standard Time", "America/Indiana/Indianapolis"),
        ("US Mountain Standard Time", "America/Phoenix"),
        ("UTC", "Etc/UTC"),
        ("UTC+12", "Etc/GMT-12"),
        ("UTC+13", "Etc/GMT-13"),
        ("UTC-02", "Etc/GMT+2"),
        ("UTC-08", "Etc/GMT+8"),
        ("UTC-09", "Etc/GMT+9"),
        ("UTC-11", "Etc/GMT+11"),
        ("Ulaanbaatar Standard Time", "Asia/Ulaanbaatar"),
        ("Venezuela Standard Time", "America/Caracas"),
        ("Vladivostok Standard Time", "Asia/Vladivostok"),
        ("Volgograd Standard Time", "Europe/Volgograd"),
        ("W. Australia Standard Time", "Australia/Perth"),
        ("W. Central Africa Standard Time", "Africa/Lagos"),
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("W. Mongolia Standard Time", "Asia/Hovd"),
        ("West Asia Standard Time", "Asia/Tashkent"),
        ("West Bank Standard Time", "Asia/Hebron"),
        ("West Pacific Standard Time", "Pacific/Port_Moresby"),
        ("Yakutsk Standard Time", "Asia/Yakutsk"),
        ("Yukon Standard Time", "America/Whitehorse"),
    ];
    let s = schluessel.trim();
    TABELLE.iter().find(|(w, _)| *w == s).map(|(_, i)| *i)
}

/// Die lokale Verschiebung in Sekunden **oestlich** von UTC zur Zeit `t`
/// (ms seit 1970). Ohne lesbare Zone: 0 (UTC), das ist die alte
/// dokumentierte Naeherung.
pub(crate) fn verschiebung_s(t: f64) -> i32 {
    if !t.is_finite() {
        return 0;
    }
    // Der clamps auf i64: ein Datum weit ausserhalb des Fensters braucht
    // keine Genauigkeit — die Zone hat nur wenige Uebergaenge.
    let ms = t.clamp(-4.0e15, 4.0e15) as i64;
    zone().bei(ms)
}

/// Das Kuerzel der Zone zur Zeit `t` (ms): `CET`, `CEST`, `UTC` … Das ist
/// der Teil von `toString()`, der im TZif-Dateihead steht (node schreibt den
/// ausgeschriebenen Namen, das ist eine ICU-Sache).
pub(crate) fn kenner(t: f64) -> String {
    if !t.is_finite() {
        return String::new();
    }
    let ms = t.clamp(-4.0e15, 4.0e15) as i64;
    let z = zone();
    let typ = z.typ_bei(ms);
    // Die Kuerzel stehen in der Reihenfolge der Typindizes.
    let k = z.typ_desig(typ);
    match k {
        Some(k) if !k.is_empty() => k,
        _ if z.bei(ms) == 0 => "UTC".to_string(),
        _ => String::new(),
    }
}

/// `getTimezoneOffset()` in Minuten: **positive** fuer westlich von UTC
/// (Spec 21.4.4.39) — also das Negative der Verschiebung.
pub(crate) fn offset_minuten(t: f64) -> i64 {
    -(verschiebung_s(t) as i64) / 60
}

// ---------------------------------------------------------------------
// Das Lesen
// ---------------------------------------------------------------------

/// Die Datei, aus der gelesen wird (`TZ` zuerst, wie bei node).
fn zonendatei() -> Option<std::path::PathBuf> {
    if let Ok(tz) = std::env::var("TZ") {
        let name = tz.strip_prefix(':').unwrap_or(&tz);
        if !name.is_empty() {
            let p = std::path::Path::new("/usr/share/zoneinfo").join(name);
            if p.is_file() {
                return Some(p);
            }
            // `TZ=UTC0` und Verwandte: der Name ist selbst eine Definition.
            // (Gemessen wird weiter unten nur am Offset.)
        }
    }
    let p = std::path::PathBuf::from("/etc/localtime");
    if p.exists() { Some(p) } else { None }
}

/// Die Datei lesen und die Zone bestimmen.
fn zone_lesen() -> Option<Zone> {
    if let Ok(name) = std::env::var("TZ") {
        if let Some(data) = benannte_daten(name.strip_prefix(':').unwrap_or(&name)) {
            return tzif_lesen(&data);
        }
    }
    let daten = std::fs::read(zonendatei()?).ok()?;
    tzif_lesen(&daten)
}

pub(crate) fn benannte_daten(name: &str) -> Option<Vec<u8>> {
    if name.is_empty() || name.starts_with('/') || name.contains("..") ||
        !name.chars().all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c)) {
        return None;
    }
    std::fs::read(std::path::Path::new("/usr/share/zoneinfo").join(name)).ok()
        .or_else(|| crate::tzdata::bytes(name).map(<[u8]>::to_vec))
}

pub(crate) fn benannte_verschiebung(name: &str, zeit_ms: f64) -> Option<i64> {
    if !zeit_ms.is_finite() { return None; }
    let data = benannte_daten(name)?;
    let zone = tzif_lesen(&data)?;
    Some(zone.bei(zeit_ms.clamp(-4.0e15, 4.0e15) as i64) as i64)
}

/// Ein Lesefehler wird zu `None` (die Zone bleibt UTC) — die Datei kann
///waechrend des Lesens kippen; das ist kein Grund fuer einen Fehler.
struct Leser {
    d: Vec<u8>,
    i: usize,
}

impl Leser {
    fn u8(&mut self) -> Option<u8> {
        let w = *self.d.get(self.i)?;
        self.i += 1;
        Some(w)
    }
    fn u32be(&mut self) -> Option<u32> {
        let b = self.d.get(self.i..self.i + 4)?;
        self.i += 4;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// 32-Bit **vorzeichenbehaftet** (GEMESSEN: die Uebergaengen liegen vor
    /// 1970, `u32` lieferte 4,29 Mrd. zu viel).
    fn i32be(&mut self) -> Option<i64> {
        let b = self.d.get(self.i..self.i + 4)?;
        self.i += 4;
        Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64)
    }
    fn i64be(&mut self) -> Option<i64> {
        let b = self.d.get(self.i..self.i + 8)?;
        self.i += 8;
        Some(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    fn ueber(&mut self, n: usize) -> Option<()> {
        self.i = self.i.checked_add(n)?;
        Some(())
    }
}

/// Ein TZif-Block (Header + Daten) lesen. `breit` = 8-Byte-Zeiten.
///
/// Liefert `(Zeit ms, Verschiebung s, Typ-Index)`, die Vorgabe-Verschiebung,
/// den Vorgabe-Typ und die Kürzel der Typen.
/// Ein gelesener Block: Uebergaenge, Vorgabe-Verschiebung, Vorgabe-Typ
/// und die Kuerzel je Typ.
type Block = (Vec<(i64, i32, usize)>, i32, usize, Vec<String>);

fn block_lesen(r: &mut Leser, breit: bool) -> Option<Block> {
    r.ueber(15)?; // reserviert
    let isutcnt = r.u32be()? as usize;
    let isstdcnt = r.u32be()? as usize;
    let leapcnt = r.u32be()? as usize;
    let timecnt = r.u32be()? as usize;
    let typecnt = r.u32be()? as usize;
    let charcnt = r.u32be()? as usize;
    if typecnt == 0 {
        return None;
    }

    // 1. Uebergangszeiten
    let mut zeiten: Vec<i64> = Vec::with_capacity(timecnt);
    for _ in 0..timecnt {
        zeiten.push((if breit { r.i64be()? } else { r.i32be()? }) * 1000);
    }
    // 2. Typ-Index je Uebergang
    let mut indizes: Vec<u8> = Vec::with_capacity(timecnt);
    for _ in 0..timecnt {
        indizes.push(r.u8()?);
    }
    // 3. Typen (utoff, isdst, desig)
    let mut typen: Vec<(i32, bool, usize)> = Vec::with_capacity(typecnt);
    for _ in 0..typecnt {
        let off = r.u32be()? as i32;
        let ist_dst = r.u8()? != 0;
        let desig = r.u8()? as usize;
        typen.push((off, ist_dst, desig));
    }
    // 4. Die Kuerzel der Typen (NUL-getrennt) — `toString()` braucht sie.
    let beschriftung_start = r.i;
    r.ueber(charcnt)?;
    let beschriftung_ende = r.i;
    // Schaltsekunden, Standard-/UTC-Flags ueberspringen
    r.ueber(leapcnt * if breit { 16 } else { 8 })?;
    r.ueber(isstdcnt)?;
    r.ueber(isutcnt)?;

    // Die Vorgabe vor dem ersten Uebergang. GEMESSEN (siehe Modulkopf): der
    // Typ des **ersten Uebergangs ohne Sommerzeit** — nicht der erste
    // Typ der Tabelle (das waere bei Berlin die LMT +0:53:28 und damit
    // -53 statt der von node gelieferten -60). Fällt kein solcher
    // Uebergang, gilt der erste Typ der Tabelle.
    // **Der Typindex** des ersten Uebergangs ohne Sommerzeit (nicht die
    // Position in der Liste — das war LMT statt HMT bei Kolkata).
    let vorgabe_typ = zeiten
        .iter()
        .zip(indizes.iter())
        .map(|(_, i)| *i as usize)
        .find(|i| typen.get(*i).is_some_and(|(_, dst, _)| !*dst))
        .unwrap_or(0);
    let vorgabe = typen.get(vorgabe_typ)?.0;

    let uebergaenge: Vec<(i64, i32, usize)> = zeiten
        .iter()
        .zip(indizes.iter())
        .filter_map(|(z, i)| {
            typen
                .get(*i as usize)
                .map(|(o, _, _)| (*z, *o, *i as usize))
        })
        .collect();
    // Die Kuerzel je Typ: `desigidx` ist ein **Byte-Offset** in den Block
    // (GEMESSEN, Europe/Berlin: 0, 4, 9, 4, 9, 13, 13, 4, 9 bei
    // „LMT\0CEST\0CET\0CEMT\0").
    let block = &r.d[beschriftung_start..beschriftung_ende];
    let desigs: Vec<String> = typen
        .iter()
        .map(|(_, _, d)| {
            let rest = block.get(*d..).unwrap_or(&[]);
            let ende = rest.iter().position(|b| *b == 0).unwrap_or(rest.len());
            String::from_utf8_lossy(&rest[..ende]).to_string()
        })
        .collect();
    Some((uebergaenge, vorgabe, vorgabe_typ, desigs))
}

/// Ein ganzes TZif-File lesen (Version 2/3/4: der zweite Block gewinnt).
fn tzif_lesen(daten: &[u8]) -> Option<Zone> {
    if daten.len() < 44 || &daten[0..4] != b"TZif" {
        return None;
    }
    let version = daten[4];
    let mut r = Leser {
        d: daten.to_vec(),
        i: 0,
    };
    r.i = 5; // magic (4) + version (1); die 15 reservierten Bytes
    // ueberspringt `block_lesen` selbst.
    let (mut uebergaenge, mut vorgabe, mut vorgabe_typ, mut desigs) = block_lesen(&mut r, false)?;
    if version >= b'2' {
        // Der zweite Header steht direkt hinter dem ersten Block.
        if &r.d.get(r.i..r.i + 4)? != b"TZif" {
            return Some(Zone {
                uebergaenge,
                desigs,
                vorgabe,
                vorgabe_typ,
            });
        }
        // Nur magic (4) + version (1) — die 15 reservierten Bytes
        // ueberspringt `block_lesen` selbst.
        r.i += 5;
        if let Some((u2, v2, v3, d2)) = block_lesen(&mut r, true) {
            uebergaenge = u2;
            vorgabe = v2;
            vorgabe_typ = v3;
            desigs = d2;
        }
    }
    Some(Zone {
        uebergaenge,
        desigs,
        vorgabe,
        vorgabe_typ,
    })
}

/// Nur fuer die Tests: frisch aus Bytes lesen (ohne Dateisystem).
#[cfg(test)]
pub(crate) fn aus_bytes(daten: &[u8]) -> Option<Zone> {
    tzif_lesen(daten)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kaputte_datei_ergibt_keine_zone() {
        assert!(tzif_lesen(b"kein tzif").is_none());
        assert!(tzif_lesen(b"").is_none());
    }

    #[test]
    fn die_zonendatei_ist_lesbar_und_gibt_eine_zone() {
        // GEMESSEN (node 22, dieselbe Maschine): `new Date(0)
        // .getTimezoneOffset()` -> -60, also +3600 s. Ohne Datei (Container
        // ohne /etc/localtime) waere 0 der Wert — dann ist der Test hier
        // gegenstandslos, aber nicht falsch.
        let z = zone();
        let verschiebung = verschiebung_s(0.0);
        assert!(
            (-14.0 * 3600.0..=14.0 * 3600.0).contains(&(verschiebung as f64)),
            "Verschiebung {verschiebung} s liegt ausserhalb der Welt"
        );
        if zonendatei().is_some() {
            assert_ne!(z.uebergaenge.len(), 0, "Zone ohne Uebergaenge?");
            assert_eq!(
                verschiebung % 900,
                0,
                "Verschiebung ist keine volle Stunde: {verschiebung}"
            );
        }
    }

    #[test]
    fn offset_minuten_hat_umgekehrtes_vorzeichen() {
        // Spec 21.4.4.39: `getTimezoneOffset()` ist **westlich** positiv,
        // die Verschiebung **oestlich** — also das Negative.
        let t = 1_600_000_000_000.0;
        assert_eq!(offset_minuten(t), -(verschiebung_s(t) as i64) / 60);
    }

    #[test]
    fn eine_zone_aus_bytes_liefert_ihre_uebergaenge() {
        // Von Hand gebaut: ein 32-Bit-Block mit zwei Uebergaengen
        // (Winter +3600 ab 1970, Sommer +7200 ab 1980) und die Vorgabe +3600.
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"TZif");
        d.push(0);
        d.extend_from_slice(&[0u8; 15]);
        d.extend_from_slice(&0u32.to_be_bytes()); // isutcnt
        d.extend_from_slice(&0u32.to_be_bytes()); // isstdcnt
        d.extend_from_slice(&0u32.to_be_bytes()); // leapcnt
        d.extend_from_slice(&2u32.to_be_bytes()); // timecnt
        d.extend_from_slice(&2u32.to_be_bytes()); // typecnt
        d.extend_from_slice(&3u32.to_be_bytes()); // charcnt
        d.extend_from_slice(&0i32.to_be_bytes()); // Uebergang 1: 1970
        d.extend_from_slice(&(315_532_800i32).to_be_bytes()); // 1980
        d.push(0); // Typ 0
        d.push(1); // Typ 1
        d.extend_from_slice(&3600i32.to_be_bytes());
        d.push(0);
        d.push(0);
        d.extend_from_slice(&7200i32.to_be_bytes());
        d.push(1);
        d.push(0);
        d.extend_from_slice(b"UTC"); // 3 Bezeichner
        let z = aus_bytes(&d).expect("Zone lesen");
        assert_eq!(z.vorgabe, 3600, "Vorgabe vor dem ersten Uebergang");
        assert_eq!(z.bei(-1), 3600, "vor 1970 gilt die Vorgabe");
        assert_eq!(z.bei(1), 3600, "1970 ist Winterzeit");
        assert_eq!(z.bei(315_532_800_000), 7200, "1980 ist Sommerzeit");
        assert_eq!(z.bei(400_000_000_000), 7200, "nach dem letzten Uebergang");
    }
}

#[cfg(test)]
mod zone_tests {
    use super::*;
    #[test]
    fn eingebettete_zonen_sind_echte_tzif_daten() {
        for name in ["Europe/Berlin", "America/New_York", "Asia/Tokyo", "Australia/Sydney", "Etc/UTC"] {
            let bytes = crate::tzdata::bytes(name).expect("bundled zone");
            assert!(tzif_lesen(bytes).is_some(), "{name}");
        }
        assert_eq!(benannte_verschiebung("Asia/Tokyo", 2_208_988_800_000.0), Some(32400));
    }
    #[test]
    fn benannte_zonen_lesen_keine_fremden_pfade() {
        for name in ["../etc/passwd", "/etc/localtime", "C:/Windows/system.ini", "missing/zone"] {
            assert!(benannte_daten(name).is_none(), "{name}");
        }
    }

    /// Die Zone einer Datei direkt lesen (ohne `TZ`).
    fn zone_von(name: &str) -> Zone {
        let d = benannte_daten(name).unwrap_or_else(|| panic!("{name} lesen"));
        tzif_lesen(&d).unwrap_or_default()
    }

    /// **GEMESSEN (node 22, dieselbe Maschine, `TZ=<zone>`)** — der Motor
    /// muss dieselben fuenf Werte liefern. Die Zone dieser Maschine ist
    /// Europe/Berlin; ohne `/usr/share/zoneinfo` sind die Zeilen nicht
    /// pruefbar, dann behauptet der Test nur dieParse-Faehigkeit.
    const SOLL: &[(&str, [i64; 5])] = &[
        // 0 ms (1970), 1780000000000 (2026-05-29), Juli 2026, Dezember 2026,
        // 1759276800000 (2025-10-01).
        ("Europe/Berlin", [-60, -120, -120, -60, -120]),
        ("America/New_York", [300, 240, 240, 300, 240]),
        ("Asia/Kolkata", [-330, -330, -330, -330, -330]),
        ("Australia/Sydney", [-600, -600, -600, -660, -600]),
        ("Europe/London", [-60, -60, -60, 0, -60]),
        ("America/Sao_Paulo", [180, 180, 180, 180, 180]),
        ("Pacific/Kiritimati", [640, -840, -840, -840, -840]),
    ];
    const ZEITEN: [f64; 5] = [
        0.0,
        1_780_000_000_000.0,
        1_783_036_800_000.0,
        1_795_977_600_000.0,
        1_759_276_800_000.0,
    ];

    #[test]
    fn sieben_zonen_fuenf_zeitpunkte_stimmen_wie_node() {
        for (name, soll) in SOLL {
            let z = zone_von(name);
            for (t, s) in ZEITEN.iter().zip(soll.iter()) {
                assert_eq!(
                    -(z.bei(*t as i64) as i64) / 60,
                    *s,
                    "{name} bei {t} ms: Zone weicht von node ab"
                );
            }
        }
    }

    #[test]
    fn die_kuerzel_kommen_aus_dem_dateihead() {
        // GEMESSEN (TZif-Dateihead von Europe/Berlin): die Typen tragen
        // `LMT`, `CEST`, `CET`, `CEMT`. `desigidx` sind Byte-Offsets.
        let z = zone_von("Europe/Berlin");
        assert_eq!(z.desigs[0], "LMT");
        assert_eq!(z.desigs[1], "CEST");
        assert_eq!(z.desigs[2], "CET");
        // Winterzeit 1970 -> CET, Sommerzeit Juli 2026 -> CEST.
        assert_eq!(z.desigs[z.typ_bei(0)], "CET");
        assert_eq!(z.desigs[z.typ_bei(1_783_036_800_000)], "CEST");
    }

    #[test]
    fn der_64_bit_block_gilt_vor_dem_32_bit_block() {
        // GEMESSEN/GEFUNDEN: sind beide Bloecke vorhanden, gewinnt der
        // 64-Bit-Block (er reicht ueber 2038 hinaus). Asia/Kolkata hat im
        // 32-Bit-Block sechs, im 64-Bit-Block sieben Uebergaenge.
        let z = zone_von("Asia/Kolkata");
        assert_eq!(
            z.uebergaenge.len(),
            7,
            "der 64-Bit-Block wurde nicht genommen"
        );
        assert_eq!(z.vorgabe, 21200, "Vorgabe vor dem ersten Uebergang");
    }

    #[test]
    fn die_zeiten_sind_alle_in_millisekunden() {
        // GEMESSEN/GEFUNDEN: Sekunden im 64-Bit-Block und Millisekunden im
        // 32-Bit-Block gemischt ergaben eine Tabelle, in der `bei()` nach dem
        // ersten Eintrag nie etwas fand (immer `vorgabe`). Die Werte liegen
        // also zwischen 1893 und 2037, mit Abstand.
        let z = zone_von("Europe/Berlin");
        assert_eq!(z.uebergaenge.len(), 143);
        assert!(
            z.uebergaenge.first().unwrap().0 < -2_000_000_000_000,
            "1893"
        );
        assert!(z.uebergaenge.last().unwrap().0 < 2_200_000_000_000, "2037");
        // Und die Suche findet tatsaechlich den Sommerzeit-typ.
        assert_eq!(z.bei(1_783_036_800_000), 7200, "Juli 2026 ist Sommerzeit");
        assert_eq!(
            z.bei(1_795_977_600_000),
            3600,
            "Dezember 2026 ist Winterzeit"
        );
    }

    #[test]
    fn ein_file_ohne_block_gibt_die_erste_nicht_sommerzeit_typ_vor() {
        // Von Hand gebaut (der gleiche Aufbau wie in `tests` oben): zwei
        // Uebergaenge, Winter zuerst.
        let mut d: Vec<u8> = Vec::new();
        d.extend_from_slice(b"TZif");
        d.push(0);
        d.extend_from_slice(&[0u8; 15]);
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&0u32.to_be_bytes());
        d.extend_from_slice(&2u32.to_be_bytes());
        d.extend_from_slice(&2u32.to_be_bytes());
        d.extend_from_slice(&3u32.to_be_bytes());
        d.extend_from_slice(&0i32.to_be_bytes());
        d.extend_from_slice(&(315_532_800i32).to_be_bytes());
        d.push(0);
        d.push(1);
        d.extend_from_slice(&3600i32.to_be_bytes());
        d.push(0);
        d.push(0);
        d.extend_from_slice(&7200i32.to_be_bytes());
        d.push(1);
        d.push(0);
        d.extend_from_slice(b"UTC");
        let z = aus_bytes(&d).expect("Zone lesen");
        assert_eq!(z.vorgabe, 3600, "Vorgabe vor dem ersten Uebergang");
        assert_eq!(z.bei(-1), 3600, "vor 1970 gilt die Vorgabe");
        assert_eq!(z.bei(1), 3600, "1970 ist Winterzeit");
        assert_eq!(z.bei(315_532_800_000), 7200, "1980 ist Sommerzeit");
        assert_eq!(z.bei(400_000_000_000), 7200, "nach dem letzten Uebergang");
    }
}
