//! System-CSPRNG statt /dev/urandom unter Windows.
//! https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcryptgenrandom
use crate::zufall::{MAX_BLOCK, ZufallFehler};

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(algorithm: *mut std::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

pub(crate) fn fuellen(ziel: &mut [u8; MAX_BLOCK]) -> Result<(), ZufallFehler> {
    // SAFETY: null algorithm with BCRYPT_USE_SYSTEM_PREFERRED_RNG (2).
    // The fixed writable array remains alive throughout this synchronous call.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), ziel.as_mut_ptr(), MAX_BLOCK as u32, 2) };
    if status < 0 {
        // No fallback: failure must abort the handshake, never substitute weak entropy.
        ziel.fill(0);
        return Err(ZufallFehler::LeseFehler(format!("BCryptGenRandom NTSTATUS {:#010x}", status as u32)));
    }
    Ok(())
}
