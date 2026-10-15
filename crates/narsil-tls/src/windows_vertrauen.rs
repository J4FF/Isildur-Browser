//! Read-only enumeration of the user's Windows ROOT certificate store.
//! Context lifetime: CertEnumCertificatesInStore frees the previous context.
use std::{ffi::c_void, ptr};
use crate::kette::{MAX_SPEICHER, MAX_ZERTIFIKATE};
#[repr(C)]
struct Context { encoding: u32, bytes: *const u8, len: u32, info: *const c_void, store: *mut c_void }
#[link(name = "crypt32")]
unsafe extern "system" {
    fn CertOpenStore(provider: *const u8, encoding: u32, crypt: usize, flags: u32, param: *const c_void) -> *mut c_void;
    fn CertEnumCertificatesInStore(store: *mut c_void, previous: *const Context) -> *const Context;
    fn CertFreeCertificateContext(context: *const Context) -> i32;
    fn CertCloseStore(store: *mut c_void, flags: u32) -> i32;
}
struct Store { handle: *mut c_void, current: *const Context }
impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: uniquely owned store and the one remaining enumerated context.
        unsafe {
            if !self.current.is_null() { CertFreeCertificateContext(self.current); }
            CertCloseStore(self.handle, 0);
        }
    }
}
pub(crate) fn root_der() -> Result<Vec<Vec<u8>>, String> {
    let root: Vec<u16> = "ROOT".encode_utf16().chain([0]).collect();
    // SAFETY: CERT_STORE_PROV_SYSTEM_W=10, current-user/read-only/open-existing;
    // null crypt handle, live UTF-16 store name. No system store is modified.
    let handle = unsafe { CertOpenStore(10usize as *const u8, 0, 0, 0x0001c000, root.as_ptr().cast()) };
    if handle.is_null() { return Err(format!("Windows ROOT: {}", std::io::Error::last_os_error())); }
    let mut store = Store { handle, current: ptr::null() };
    let mut roots = Vec::new();
    let mut total = 0usize;
    loop {
        // SAFETY: valid store; the API frees current and transfers ownership of next.
        store.current = unsafe { CertEnumCertificatesInStore(store.handle, store.current) };
        if store.current.is_null() {
            let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
            if code != 0x80092004 && code != 18 { return Err(format!("Windows ROOT enumeration: {code:#010x}")); }
            break;
        }
        // SAFETY: context remains valid until the next enumeration or Store::drop.
        let context = unsafe { &*store.current };
        let len = context.len as usize;
        if len == 0 || context.bytes.is_null() || len > crate::x509::MAX_ZERTIFIKAT {
            return Err("Windows ROOT: ungueltige Zertifikatlaenge".into());
        }
        total = total.checked_add(len).ok_or("Windows ROOT: Laengenueberlauf")?;
        if roots.len() >= MAX_ZERTIFIKATE || total > MAX_SPEICHER { return Err("Windows ROOT: Speichergrenze".into()); }
        // SAFETY: OS-owned DER buffer with verified length; copy before freeing context.
        roots.push(unsafe { std::slice::from_raw_parts(context.bytes, len) }.to_vec());
    }
    if roots.is_empty() { return Err("Windows ROOT ist leer".into()); }
    Ok(roots)
}
