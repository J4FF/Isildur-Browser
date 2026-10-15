//! Release 10.10.2026: der Datei-Oeffnen-Dialog von Windows
//! (`GetOpenFileNameW`, Explorer-Stil) fuer `<input type="file">`.
//!
//! Die FFI bleibt wie bei der Zwischenablage in dieser Crate; der Aufrufer
//! bekommt nur Pfade. Der Dialog ist modal zum aktiven Fenster und blockiert
//! den aufrufenden Thread, bis der Nutzer waehlt oder abbricht.
use std::ffi::c_void;
use std::path::PathBuf;

type Handle = *mut c_void;

#[repr(C)]
struct OpenFileNameW {
    l_struct_size: u32,
    hwnd_owner: Handle,
    h_instance: Handle,
    lpstr_filter: *const u16,
    lpstr_custom_filter: *mut u16,
    n_max_cust_filter: u32,
    n_filter_index: u32,
    lpstr_file: *mut u16,
    n_max_file: u32,
    lpstr_file_title: *mut u16,
    n_max_file_title: u32,
    lpstr_initial_dir: *const u16,
    lpstr_title: *const u16,
    flags: u32,
    n_file_offset: u16,
    n_file_extension: u16,
    lpstr_def_ext: *const u16,
    l_cust_data: isize,
    lpfn_hook: *const c_void,
    lp_template_name: *const u16,
    pv_reserved: *mut c_void,
    dw_reserved: u32,
    flags_ex: u32,
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetActiveWindow() -> Handle;
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
}

const OFN_HIDEREADONLY: u32 = 0x0000_0004;
const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
const OFN_ALLOWMULTISELECT: u32 = 0x0000_0200;
const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
const OFN_EXPLORER: u32 = 0x0008_0000;
const COINIT_APARTMENTTHREADED: u32 = 0x2;

/// M4: der Puffer fuer die gewaehlten Pfade (64 Ki Zeichen — Windows meldet
/// einen Fehler statt abzuschneiden, wenn eine Mehrfachwahl nicht passt).
const PUFFER_ZEICHEN: usize = 65_536;

fn breit(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Oeffnet den Dialog. `filter` sind Paare (Beschreibung, Muster wie
/// `*.png;*.jpg`); „Alle Dateien" kommt immer dazu. `None` bei Abbruch.
pub fn oeffnen(titel: &str, mehrere: bool, filter: &[(String, String)]) -> Option<Vec<PathBuf>> {
    // Die Filterliste: Paare mit Nullen getrennt, am Ende eine doppelte Null.
    let mut f: Vec<u16> = Vec::new();
    for (beschreibung, muster) in filter.iter().chain(std::iter::once(&("Alle Dateien (*.*)".to_string(), "*.*".to_string()))) {
        f.extend(breit(beschreibung));
        f.push(0);
        f.extend(breit(muster));
        f.push(0);
    }
    f.push(0);
    let mut titel_w = breit(titel);
    titel_w.push(0);
    let mut puffer = vec![0u16; PUFFER_ZEICHEN];
    let mut flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_HIDEREADONLY;
    if mehrere {
        flags |= OFN_ALLOWMULTISELECT;
    }
    // SAFETY: alle Zeiger zeigen auf Puffer, die bis zum Ende des Aufrufs
    // leben; die Struktur ist nach der Windows-SDK-Definition gebaut und mit
    // ihrer Groesse gekennzeichnet. `CoInitializeEx` ist wiederholbar
    // (S_FALSE, wenn der Thread schon initialisiert ist).
    let ok = unsafe {
        let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        let mut ofn = OpenFileNameW {
            l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
            hwnd_owner: GetActiveWindow(),
            h_instance: std::ptr::null_mut(),
            lpstr_filter: f.as_ptr(),
            lpstr_custom_filter: std::ptr::null_mut(),
            n_max_cust_filter: 0,
            n_filter_index: 1,
            lpstr_file: puffer.as_mut_ptr(),
            n_max_file: PUFFER_ZEICHEN as u32,
            lpstr_file_title: std::ptr::null_mut(),
            n_max_file_title: 0,
            lpstr_initial_dir: std::ptr::null(),
            lpstr_title: titel_w.as_ptr(),
            flags,
            n_file_offset: 0,
            n_file_extension: 0,
            lpstr_def_ext: std::ptr::null(),
            l_cust_data: 0,
            lpfn_hook: std::ptr::null(),
            lp_template_name: std::ptr::null(),
            pv_reserved: std::ptr::null_mut(),
            dw_reserved: 0,
            flags_ex: 0,
        };
        GetOpenFileNameW(&mut ofn) != 0
    };
    if !ok {
        return None;
    }
    // Ergebnis: bei EINER Datei der volle Pfad; bei mehreren
    // „Ordner\0Name1\0Name2\0\0".
    let teile: Vec<String> = puffer
        .split(|&c| c == 0)
        .take_while(|t| !t.is_empty())
        .map(String::from_utf16_lossy)
        .collect();
    match teile.len() {
        0 => None,
        1 => Some(vec![PathBuf::from(&teile[0])]),
        _ => {
            let ordner = PathBuf::from(&teile[0]);
            Some(teile[1..].iter().map(|n| ordner.join(n)).collect())
        }
    }
}
