//! Unicode clipboard access. The FFI and ownership transfer stay in the window crate.
use std::{ffi::c_void, ptr};
type Handle = *mut c_void;
#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(owner: Handle) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn GetClipboardData(format: u32) -> Handle;
    fn SetClipboardData(format: u32, data: Handle) -> Handle;
    fn GetActiveWindow() -> Handle;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalFree(data: Handle) -> Handle;
    fn GlobalLock(data: Handle) -> *mut c_void;
    fn GlobalUnlock(data: Handle) -> i32;
    fn GlobalSize(data: Handle) -> usize;
}
const UNICODE_TEXT: u32 = 13;
const MAX_UNITS: usize = 1_048_576;
struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after successfully opening the clipboard.
        unsafe { CloseClipboard(); }
    }
}
pub fn lesen() -> Option<String> {
    // SAFETY: handles are checked, clipboard remains open while the data is read,
    // and the slice is bounded by GlobalSize and our allocation limit.
    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 { return None; }
        let _guard = Clipboard;
        let data = GetClipboardData(UNICODE_TEXT);
        if data.is_null() { return None; }
        let units = GlobalSize(data) / 2;
        if units == 0 || units > MAX_UNITS { return None; }
        let text = GlobalLock(data).cast::<u16>();
        if text.is_null() { return None; }
        let slice = std::slice::from_raw_parts(text, units);
        let result = slice.iter().position(|u| *u == 0)
            .map(|end| String::from_utf16_lossy(&slice[..end]));
        GlobalUnlock(data);
        result
    }
}
pub fn schreiben(text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().take(MAX_UNITS - 1).chain([0]).collect();
    // SAFETY: checked movable allocation, sized copy, unlocked before transferring
    // ownership to Windows. Failed transfers free the allocation locally.
    unsafe {
        let owner = GetActiveWindow();
        if owner.is_null() || OpenClipboard(owner) == 0 { return false; }
        let _guard = Clipboard;
        let data = GlobalAlloc(0x0002, units.len() * 2);
        if data.is_null() { return false; }
        let dest = GlobalLock(data).cast::<u16>();
        if dest.is_null() { GlobalFree(data); return false; }
        ptr::copy_nonoverlapping(units.as_ptr(), dest, units.len());
        GlobalUnlock(data);
        if EmptyClipboard() == 0 || SetClipboardData(UNICODE_TEXT, data).is_null() {
            GlobalFree(data);
            return false;
        }
        true
    }
}
