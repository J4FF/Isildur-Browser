//! Release 10.10.2026: der Schluesselname der Windows-Zeitzone
//! (`GetDynamicTimeZoneInformation`, `TimeZoneKeyName`, z. B.
//! `W. Europe Standard Time`).
//!
//! Der JS-Motor rechnet lokale Zeiten mit IANA-Zonendaten; unter Windows gibt
//! es weder `TZ` noch `/etc/localtime`. Die FFI bleibt wie Zwischenablage und
//! Dateidialog in dieser Crate (ADR-020); der Aufrufer bekommt nur den Namen
//! und ordnet ihn mit `narsil_jsvm::windows_zone_iana` zu.

#[repr(C)]
struct SystemTime {
    felder: [u16; 8],
}

/// `DYNAMIC_TIME_ZONE_INFORMATION` (timezoneapi.h).
#[repr(C)]
struct DynamischeZone {
    bias: i32,
    standard_name: [u16; 32],
    standard_date: SystemTime,
    standard_bias: i32,
    daylight_name: [u16; 32],
    daylight_date: SystemTime,
    daylight_bias: i32,
    time_zone_key_name: [u16; 128],
    dynamic_daylight_time_disabled: u8,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDynamicTimeZoneInformation(info: *mut DynamischeZone) -> u32;
}

/// `TIME_ZONE_ID_INVALID`.
const UNGUELTIG: u32 = 0xFFFF_FFFF;

/// Der Schluesselname der eingestellten Zone, `None` bei Fehler oder leerem
/// Namen.
pub fn schluesselname() -> Option<String> {
    let mut info = DynamischeZone {
        bias: 0,
        standard_name: [0; 32],
        standard_date: SystemTime { felder: [0; 8] },
        standard_bias: 0,
        daylight_name: [0; 32],
        daylight_date: SystemTime { felder: [0; 8] },
        daylight_bias: 0,
        time_zone_key_name: [0; 128],
        dynamic_daylight_time_disabled: 0,
    };
    // SAFETY: `info` ist eine gueltige, beschreibbare Struktur im Layout von
    // `DYNAMIC_TIME_ZONE_INFORMATION`; die Funktion fuellt sie nur.
    let r = unsafe { GetDynamicTimeZoneInformation(&mut info) };
    if r == UNGUELTIG {
        return None;
    }
    let n = info
        .time_zone_key_name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(info.time_zone_key_name.len());
    let name = String::from_utf16_lossy(&info.time_zone_key_name[..n]);
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn der_schluesselname_ist_lesbar() {
        let n = super::schluesselname().expect("Zonenname");
        assert!(n.len() > 2, "{n}");
    }
}
