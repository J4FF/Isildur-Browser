//! Win32-Senke: derselbe Surface-/Ereignisvertrag wie X11, ohne Fremd-Crates.
//! Unsafe bleibt an der Windows-FFI-Grenze; Fenster gehoeren ihrem UI-Thread.
use std::{cell::RefCell, collections::HashMap, ffi::c_void, ptr, rc::Rc};
use narsil_core::geometry::{Point, Size};
use narsil_input::{Key, KeyEvent, Modifiers};
use crate::{MouseButton, ScrollDelta, Surface, SurfaceError, SurfaceEvent};

type Handle = *mut c_void;
type WndProc = unsafe extern "system" fn(Handle, u32, usize, isize) -> isize;
#[repr(C)]
struct WindowClass {
    size: u32, style: u32, proc: WndProc, class_extra: i32, window_extra: i32,
    instance: Handle, icon: Handle, cursor: Handle, background: Handle,
    menu: *const u16, name: *const u16, small_icon: Handle,
}
#[repr(C)]
#[derive(Default)]
struct WinPoint { x: i32, y: i32 }
#[repr(C)]
#[derive(Default)]
struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
#[repr(C)]
#[derive(Default)]
struct Message { window: Handle, message: u32, wparam: usize, lparam: isize, time: u32, point: WinPoint, private: u32 }
#[repr(C)]
struct BitmapInfo {
    size: u32, width: i32, height: i32, planes: u16, bits: u16,
    compression: u32, image_size: u32, xppm: i32, yppm: i32, used: u32, important: u32,
    color: u32,
}
#[repr(C)]
struct NcCalcSizeParams { rgrc: [Rect; 3], pos: *mut c_void }
#[repr(C)]
struct TrackMouseEventInfo { size: u32, flags: u32, window: Handle, hover_time: u32 }
#[repr(C)]
struct Margins { left: i32, right: i32, top: i32, bottom: i32 }
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WindowClass) -> u16;
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32,
        x: i32, y: i32, width: i32, height: i32, parent: Handle, menu: Handle, instance: Handle, param: Handle) -> Handle;
    fn DefWindowProcW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    fn DestroyWindow(window: Handle) -> i32;
    fn ShowWindow(window: Handle, command: i32) -> i32;
    fn AdjustWindowRectEx(rect: *mut Rect, style: u32, menu: i32, ex: u32) -> i32;
    fn SetWindowPos(window: Handle, after: Handle, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    fn PeekMessageW(message: *mut Message, window: Handle, min: u32, max: u32, flags: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn MsgWaitForMultipleObjectsEx(count: u32, handles: *const Handle, timeout: u32, mask: u32, flags: u32) -> u32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn ValidateRect(window: Handle, rect: *const Rect) -> i32;
    fn GetDC(window: Handle) -> Handle;
    fn ReleaseDC(window: Handle, dc: Handle) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn ScreenToClient(window: Handle, point: *mut WinPoint) -> i32;
    fn ClientToScreen(window: Handle, point: *mut WinPoint) -> i32;
    fn SetCursorPos(x: i32, y: i32) -> i32;
    fn SetCapture(window: Handle) -> Handle;
    fn ReleaseCapture() -> i32;
    fn PostMessageW(window: Handle, message: u32, wparam: usize, lparam: isize) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn SystemParametersInfoW(aktion: u32, param: u32, daten: *mut c_void, flags: u32) -> i32;
    fn IsZoomed(window: Handle) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn TrackMouseEvent(info: *mut TrackMouseEventInfo) -> i32;
    fn SetWindowTextW(window: Handle, text: *const u16) -> i32;
    fn SetProcessDPIAware() -> i32;
    fn MonitorFromWindow(window: Handle, flags: u32) -> Handle;
    fn GetMonitorInfoW(monitor: Handle, info: *mut c_void) -> i32;
    fn EnumDisplaySettingsW(device: *const u16, mode: u32, devmode: *mut c_void) -> i32;
}
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(window: Handle, attribute: u32, value: *const c_void, size: u32) -> i32;
    fn DwmExtendFrameIntoClientArea(window: Handle, margins: *const Margins) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn SetDIBitsToDevice(dc: Handle, x: i32, y: i32, w: u32, h: u32, sx: i32, sy: i32,
        first: u32, lines: u32, bits: *const c_void, info: *const BitmapInfo, colors: u32) -> i32;
    fn StretchDIBits(dc: Handle, x: i32, y: i32, w: i32, h: i32, sx: i32, sy: i32,
        sw: i32, sh: i32, bits: *const c_void, info: *const BitmapInfo, colors: u32, rop: u32) -> i32;
}
const STYLE: u32 = 0x00cf0000; // WS_OVERLAPPEDWINDOW
/// Release 08.10.2026: die Titelzone eines rahmenlosen Fensters — die
/// Tableiste ist die Titelleiste. `frei` sind die Kaesten darin, die die
/// Oberflaeche selbst bedient (Tabs, Knoepfe); dazwischen zieht man das
/// Fenster. `maximieren` ist der Maximieren-Knopf (Windows 11 zeigt dort
/// das Andock-Menue).
#[derive(Default, Clone)]
struct Titelzone { hoehe: i32, frei: Vec<(i32, i32, i32, i32)>, maximieren: Option<(i32, i32, i32, i32)> }
#[derive(Default)]
struct State {
    events: Vec<SurfaceEvent>, closed: bool, size: Size, surrogate: Option<u16>,
    /// Release 08.10.2026: ohne System-Titelleiste (die Oberflaeche zeichnet sie).
    rahmenlos: bool,
    titel: Titelzone,
    /// Bildpunkte je Zoll des Bildschirms, auf dem das Fenster steht.
    dpi: u32,
    /// Wird die Maus fuer `WM_MOUSELEAVE` verfolgt?
    verfolgt: bool,
}
thread_local! { static WINDOWS: RefCell<HashMap<usize, Rc<RefCell<State>>>> = RefCell::new(HashMap::new()); }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }
fn error(operation: &str) -> SurfaceError {
    SurfaceError::DeviceLost(format!("{operation}: {}", std::io::Error::last_os_error()))
}
fn modifiers() -> Modifiers {
    // SAFETY: GetKeyState takes virtual-key constants, no pointers.
    unsafe { Modifiers { shift: GetKeyState(0x10) < 0, ctrl: GetKeyState(0x11) < 0, alt: GetKeyState(0x12) < 0 } }
}
// WM_CHAR contains the text already translated by the keyboard layout.
// AltGr is reported as Ctrl+Alt by Windows, but must reach text fields as
// text rather than a browser shortcut. Raw key messages keep their modifiers.
fn text_event(c: char, mut m: Modifiers) -> Option<SurfaceEvent> {
    if c.is_control() || m.ctrl != m.alt { return None; }
    if m.ctrl && m.alt { m.ctrl = false; m.alt = false; }
    Some(SurfaceEvent::Key(KeyEvent { key: if c == ' ' { Key::Space } else { Key::Char(c) }, modifiers: m, pressed: true }))
}
fn control_key(vk: usize) -> Option<Key> {
    Some(match vk {
        8 => Key::Backspace, 9 => Key::Tab, 13 => Key::Enter, 27 => Key::Escape,
        16 => Key::Shift, 17 => Key::Control, 18 => Key::Alt,
        33 => Key::PageUp, 34 => Key::PageDown, 35 => Key::End, 36 => Key::Home,
        37 => Key::Left, 38 => Key::Up, 39 => Key::Right, 40 => Key::Down, 46 => Key::Delete,
        112..=123 => Key::F((vk - 111) as u8), _ => return None,
    })
}
/// Eine Funktion aus `user32.dll`, die es erst ab einer Windows-Version gibt
/// — zur Laufzeit gesucht, damit das Programm auch ohne sie startet.
fn user32_funktion(name: &[u8]) -> *mut c_void {
    let modul = wide("user32.dll");
    // SAFETY: NUL-terminierte Namen; user32 ist in jedem GUI-Prozess geladen.
    unsafe {
        let h = GetModuleHandleW(modul.as_ptr());
        if h.is_null() { return ptr::null_mut(); }
        GetProcAddress(h, name.as_ptr())
    }
}
/// Release 08.10.2026: PER-MONITOR-DPI (v2). GEMESSEN vorher: der Prozess
/// war nicht DPI-bewusst — auf 150-%-Bildschirmen streckte Windows das ganze
/// Bild unscharf. Einmal je Prozess, vor dem ersten Fenster.
fn dpi_bewusst_machen() {
    static EINMAL: std::sync::Once = std::sync::Once::new();
    EINMAL.call_once(|| {
        let f = user32_funktion(b"SetProcessDpiAwarenessContext\0");
        if !f.is_null() {
            // SAFETY: Signatur BOOL(DPI_AWARENESS_CONTEXT); -4 = PER_MONITOR_AWARE_V2.
            let f: unsafe extern "system" fn(isize) -> i32 = unsafe { std::mem::transmute(f) };
            if unsafe { f(-4) } != 0 { return; }
        }
        // SAFETY: parameterlos.
        unsafe { SetProcessDPIAware(); }
    });
}
fn dpi_von(window: Handle) -> u32 {
    let f = user32_funktion(b"GetDpiForWindow\0");
    if f.is_null() { return 96; }
    // SAFETY: Signatur UINT(HWND), lebendes Fenster.
    let f: unsafe extern "system" fn(Handle) -> u32 = unsafe { std::mem::transmute(f) };
    let d = unsafe { f(window) };
    if d == 0 { 96 } else { d }
}
fn metrik(index: i32, dpi: u32) -> i32 {
    let f = user32_funktion(b"GetSystemMetricsForDpi\0");
    if !f.is_null() {
        // SAFETY: Signatur int(int, UINT).
        let f: unsafe extern "system" fn(i32, u32) -> i32 = unsafe { std::mem::transmute(f) };
        return unsafe { f(index, dpi) };
    }
    // SAFETY: Konstante Indizes.
    unsafe { GetSystemMetrics(index) }
}
/// Der Rand, um den ein MAXIMIERTES Fenster ueber den Bildschirm ragt.
fn rahmen_maximiert(dpi: u32) -> i32 {
    metrik(33, dpi) + metrik(92, dpi) // SM_CYFRAME + SM_CXPADDEDBORDER
}
fn liegt_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3
}
fn pointer(lp: isize) -> Point {
    Point::new((lp as u16 as i16) as f32, ((lp >> 16) as u16 as i16) as f32)
}
unsafe extern "system" fn window_proc(window: Handle, message: u32, wp: usize, lp: isize) -> isize {
    if !matches!(message, 0x0010 | 0x0005 | 0x000f | 0x0014 | 0x0200..=0x0202 | 0x0204..=0x0205 | 0x0207..=0x0208 | 0x020a | 0x0100..=0x0102 | 0x0104..=0x0105
        | 0x0083 | 0x0084 | 0x00a0 | 0x00a1 | 0x00a2 | 0x02e0 | 0x02a2 | 0x02a3) {
        // SAFETY: unhandled messages, including reentrant WM_CAPTURECHANGED,
        // are forwarded without borrowing the per-window state.
        return unsafe { DefWindowProcW(window, message, wp, lp) };
    }
    // SAFETY: capture calls can synchronously reenter the window procedure.
    // Perform them before borrowing State, never across a RefCell borrow.
    unsafe {
        if matches!(message, 0x0201 | 0x0204 | 0x0207) { SetCapture(window); }
        if matches!(message, 0x0202 | 0x0205 | 0x0208) { ReleaseCapture(); }
    }
    // Use the same close path as the title-bar button, including for synthetic
    // events where GetKeyState does not contain the injected Alt modifier.
    let message = if message == 0x0104 && wp == 0x73 && lp & (1 << 29) != 0 {
        0x0010
    } else { message };
    let state = WINDOWS.with(|all| all.borrow().get(&(window as usize)).cloned());
    // Release 08.10.2026: der RAHMENLOSE Weg — vor dem Ereignis-Teil, weil
    // er die Standardprozedur ruft und dabei keinen Zustand halten darf.
    if let Some(state) = state.as_ref() {
        let (rahmenlos, titel, dpi) = {
            let s = state.borrow();
            (s.rahmenlos, s.titel.clone(), s.dpi)
        };
        if let Some(antwort) = unsafe { rahmen_nachricht(window, message, wp, lp, rahmenlos, &titel, dpi, state) } {
            return antwort;
        }
    }
    if let Some(state) = state {
        let mut state = state.borrow_mut();
        let event = match message {
            0x0010 => { state.closed = true; Some(SurfaceEvent::CloseRequested) } // WM_CLOSE
            0x0005 => {
                let size = Size::new((lp as u16) as f32, ((lp >> 16) as u16) as f32);
                if size.width > 0.0 && size.height > 0.0 { state.size = size; Some(SurfaceEvent::Resized(size)) } else { None }
            }
            0x000f => {
                // SAFETY: live HWND, null means the whole update region.
                unsafe { ValidateRect(window, ptr::null()); }
                Some(SurfaceEvent::Redraw)
            }
            0x0014 => return 1, // WM_ERASEBKGND: complete frame provided by rasterizer
            0x0200 => {
                if !state.verfolgt {
                    let mut info = TrackMouseEventInfo { size: std::mem::size_of::<TrackMouseEventInfo>() as u32, flags: 0x0002, window, hover_time: 0 };
                    // SAFETY: initialisierte Struktur, lebendes Fenster (TME_LEAVE).
                    unsafe { TrackMouseEvent(&mut info); }
                    state.verfolgt = true;
                }
                Some(SurfaceEvent::PointerMoved(pointer(lp)))
            }
            // WM_MOUSELEAVE / WM_NCMOUSELEAVE: die Maus ist draussen — ein
            // Punkt ausserhalb nimmt jeden Hover-Zustand zurueck.
            0x02a3 | 0x02a2 => {
                state.verfolgt = false;
                Some(SurfaceEvent::PointerMoved(Point::new(-1.0, -1.0)))
            }
            0x0201 | 0x0204 | 0x0207 => {
                Some(SurfaceEvent::ButtonPressed { point: pointer(lp), button: mouse_button(message) })
            }
            0x0202 | 0x0205 | 0x0208 => {
                Some(SurfaceEvent::ButtonReleased { point: pointer(lp), button: mouse_button(message) })
            }
            0x020a => {
                let point = pointer(lp);
                let mut p = WinPoint { x: point.x as i32, y: point.y as i32 };
                // SAFETY: valid HWND and initialized writable point.
                unsafe { ScreenToClient(window, &mut p); }
                Some(SurfaceEvent::Wheel { point: Point::new(p.x as f32, p.y as f32), delta: ScrollDelta::Lines(-((wp >> 16) as u16 as i16) as f32 / 120.0) })
            }
            0x0100 | 0x0101 | 0x0104 | 0x0105 => {
                let m = modifiers();
                let key = control_key(wp).or_else(|| {
                    if (m.ctrl != m.alt) && (0x30..=0x5a).contains(&wp) {
                        char::from_u32(wp as u32).map(|c| Key::Char(c.to_ascii_lowercase()))
                    } else { None }
                });
                key.map(|key| SurfaceEvent::Key(KeyEvent { key, modifiers: m, pressed: matches!(message, 0x0100 | 0x0104) }))
            }
            0x0102 => {
                let m = modifiers();
                let unit = wp as u16;
                if m.ctrl != m.alt { None } else if (0xd800..=0xdbff).contains(&unit) {
                    state.surrogate = Some(unit); None
                } else {
                    let ch = if (0xdc00..=0xdfff).contains(&unit) {
                        state.surrogate.take().and_then(|hi| char::from_u32(0x10000 + ((hi as u32 - 0xd800) << 10) + unit as u32 - 0xdc00))
                    } else { state.surrogate = None; char::from_u32(unit as u32) };
                    ch.and_then(|c| text_event(c, m))
                }
            }
            _ => {
                drop(state);
                // SAFETY: pass the exact OS callback arguments to the default procedure.
                return unsafe { DefWindowProcW(window, message, wp, lp) };
            }
        };
        if let Some(event) = event { state.events.push(event); }
        return 0;
    }
    // SAFETY: creation/destruction messages are handled before/after state registration.
    unsafe { DefWindowProcW(window, message, wp, lp) }
}
/// Release 08.10.2026: die Nachrichten des rahmenlosen Fensters.
/// `Some(antwort)` = erledigt, `None` = normaler Weg.
///
/// * `WM_NCCALCSIZE`: der Kundenbereich reicht bis zur Oberkante (die
///   Seitenraender und der untere Rand bleiben — Groesse aendern, Schatten
///   und Andocken funktionieren wie bei jedem Fenster); maximiert wird um
///   den Rand eingerueckt, der dann ueber den Bildschirm ragt.
/// * `WM_NCHITTEST`: oben ein Streifen zum Groesse-Aendern, in der
///   Titelzone ausserhalb der Bedienelemente `HTCAPTION` (Ziehen,
///   Doppelklick, Aero-Snap), ueber dem Maximieren-Knopf `HTMAXBUTTON`
///   (Andock-Menue von Windows 11).
#[allow(clippy::too_many_arguments)]
unsafe fn rahmen_nachricht(
    window: Handle, message: u32, wp: usize, lp: isize,
    rahmenlos: bool, titel: &Titelzone, dpi: u32, state: &Rc<RefCell<State>>,
) -> Option<isize> {
    match message {
        0x0083 if rahmenlos && wp != 0 => {
            // SAFETY: bei wParam = TRUE zeigt lParam auf NCCALCSIZE_PARAMS.
            let params = lp as *mut NcCalcSizeParams;
            let oben = unsafe { (*params).rgrc[0].top };
            unsafe { DefWindowProcW(window, message, wp, lp); }
            let mut neu_oben = oben;
            if unsafe { IsZoomed(window) } != 0 {
                neu_oben += rahmen_maximiert(dpi);
            }
            unsafe { (*params).rgrc[0].top = neu_oben; }
            Some(0)
        }
        0x0084 if rahmenlos => {
            let standard = unsafe { DefWindowProcW(window, message, wp, lp) };
            if standard != 1 { return Some(standard); } // nur HTCLIENT verfeinern
            let mut p = WinPoint { x: (lp as u16 as i16) as i32, y: ((lp >> 16) as u16 as i16) as i32 };
            unsafe { ScreenToClient(window, &mut p); }
            let mut client = Rect::default();
            unsafe { GetClientRect(window, &mut client); }
            let breit = client.right - client.left;
            let maximiert = unsafe { IsZoomed(window) } != 0;
            let rand = (5 * dpi as i32 / 96).max(4);
            if !maximiert && p.y < rand {
                return Some(if p.x < rand * 2 { 13 } else if p.x >= breit - rand * 2 { 14 } else { 12 });
            }
            if p.y < titel.hoehe {
                if titel.maximieren.is_some_and(|r| liegt_in((p.x, p.y), r)) { return Some(9); }
                if !titel.frei.iter().any(|r| liegt_in((p.x, p.y), *r)) { return Some(2); }
            }
            Some(1)
        }
        // WM_NCMOUSEMOVE: ueber der Titelzone kommt kein WM_MOUSEMOVE — der
        // Hover-Zustand (Maximieren-Knopf) braucht den Punkt trotzdem.
        0x00a0 if rahmenlos => {
            let mut p = WinPoint { x: (lp as u16 as i16) as i32, y: ((lp >> 16) as u16 as i16) as i32 };
            unsafe { ScreenToClient(window, &mut p); }
            let mut s = state.borrow_mut();
            if !s.verfolgt {
                let mut info = TrackMouseEventInfo { size: std::mem::size_of::<TrackMouseEventInfo>() as u32, flags: 0x0002 | 0x0010, window, hover_time: 0 };
                unsafe { TrackMouseEvent(&mut info); }
                s.verfolgt = true;
            }
            s.events.push(SurfaceEvent::PointerMoved(Point::new(p.x as f32, p.y as f32)));
            None
        }
        // HTMAXBUTTON gehoert der Oberflaeche: Druecken schlucken, Loslassen
        // schaltet um (sonst malte die Standardprozedur einen alten Knopf).
        0x00a1 if rahmenlos && wp == 9 => Some(0),
        0x00a2 if rahmenlos && wp == 9 => {
            let cmd = if unsafe { IsZoomed(window) } != 0 { 9 } else { 3 };
            unsafe { ShowWindow(window, cmd); }
            Some(0)
        }
        // WM_DPICHANGED: neuer Massstab, Windows schlaegt das Rechteck vor.
        0x02e0 => {
            let neu = ((wp >> 16) & 0xffff) as u32;
            state.borrow_mut().dpi = if neu == 0 { 96 } else { neu };
            // SAFETY: lParam zeigt auf das vorgeschlagene RECT.
            let r = unsafe { &*(lp as *const Rect) };
            unsafe { SetWindowPos(window, ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, 0x0014); }
            Some(0)
        }
        _ => None,
    }
}
fn mouse_button(message: u32) -> MouseButton {
    match message { 0x0201 | 0x0202 => MouseButton::Left, 0x0204 | 0x0205 => MouseButton::Right, _ => MouseButton::Middle }
}

pub struct Win32Surface {
    window: Handle, state: Rc<RefCell<State>>, last: Option<(Vec<u8>, u32, u32)>,
    bgra: RefCell<Vec<u8>>,
}
impl Win32Surface {
    pub fn open(width: u32, height: u32, title: &str) -> Result<Self, SurfaceError> {
        Self::create(width, height, title, true)
    }
    /// Hidden native window for automated OS tests, with the same rendering path.
    pub fn open_hidden(width: u32, height: u32, title: &str) -> Result<Self, SurfaceError> {
        Self::create(width, height, title, false)
    }
    fn create(width: u32, height: u32, title: &str, visible: bool) -> Result<Self, SurfaceError> {
        check_dimensions(width, height)?;
        dpi_bewusst_machen();
        if title.contains('\0') { return Err(SurfaceError::InvalidArgument("NUL im Fenstertitel".into())); }
        let name = wide("NarsilNativeWindow");
        let title = wide(title);
        // SAFETY: synchronous APIs; all structures/UTF-16 strings live for the calls.
        let window = unsafe {
            let instance = GetModuleHandleW(ptr::null());
            let class = WindowClass { size: std::mem::size_of::<WindowClass>() as u32, style: 0,
                proc: window_proc, class_extra: 0, window_extra: 0, instance, icon: ptr::null_mut(),
                cursor: LoadCursorW(ptr::null_mut(), 32512usize as *const u16), background: ptr::null_mut(),
                menu: ptr::null(), name: name.as_ptr(), small_icon: ptr::null_mut() };
            if RegisterClassExW(&class) == 0 && std::io::Error::last_os_error().raw_os_error() != Some(1410) { return Err(error("RegisterClassExW")); }
            // 06.10.2026: DIE ARBEITSFLAECHE BEGRENZT DAS FENSTER.
            //
            // GEMESSENER GRUND (Nutzerwunsch „standardmaessig 1920x1080"):
            // ein 1920x1080-CLIENT ist mit Rahmen und Titelleiste rund
            // 1936x1119 — auf einem 1080p-Bildschirm haengt die
            // Titelleiste ueber dem Rand, und der Nutzer kommt an
            // Minimieren/Schliessen nicht mehr heran. Das Fenster wird
            // deshalb auf die Arbeitsflaeche gekappt; `main` liest die
            // tatsaechliche Client-Groesse danach mit `server_size` und
            // gibt sie der Sitzung, damit beide dieselbe Zahl rechnen.
            let rect = outer_rect(width, height)?;
            let rect = clamp_to_work_area(rect, width, height);
            CreateWindowExW(0, name.as_ptr(), title.as_ptr(), STYLE, i32::MIN, i32::MIN,
                rect.right - rect.left, rect.bottom - rect.top, ptr::null_mut(), ptr::null_mut(), instance, ptr::null_mut())
        };
        if window.is_null() { return Err(error("CreateWindowExW")); }
        let state = Rc::new(RefCell::new(State { size: Size::new(width as f32, height as f32), dpi: dpi_von(window), ..State::default() }));
        WINDOWS.with(|all| all.borrow_mut().insert(window as usize, Rc::clone(&state)));
        // Release 08.10.2026: dunkler Fensterrahmen (der Rand, den Windows
        // weiterhin zeichnet, passt zur dunklen Oberflaeche).
        let dunkel: i32 = 1;
        // SAFETY: lebendes Fenster, Zeiger auf einen gueltigen BOOL.
        unsafe { DwmSetWindowAttribute(window, 20, (&dunkel as *const i32).cast(), 4); }
        if visible {
            // SAFETY: newly created HWND, state registration is complete.
            unsafe { ShowWindow(window, 5); }
            state.borrow_mut().events.push(SurfaceEvent::Shown);
        }
        Ok(Self { window, state, last: None, bgra: RefCell::new(Vec::new()) })
    }
    pub fn window_id(&self) -> u64 { self.window as usize as u64 }
    /// Release 08.10.2026: ohne System-Titelleiste — die Oberflaeche zeichnet
    /// Tabs und Fensterknoepfe selbst (siehe `setze_titelzone`).
    pub fn setze_rahmenlos(&mut self, an: bool) {
        if self.state.borrow().rahmenlos == an { return; }
        self.state.borrow_mut().rahmenlos = an;
        let m = Margins { left: 0, right: 0, top: if an { 1 } else { 0 }, bottom: 0 };
        // SAFETY: lebendes Fenster; SWP_FRAMECHANGED|NOMOVE|NOSIZE|NOZORDER|NOACTIVATE
        // laesst WM_NCCALCSIZE neu rechnen.
        unsafe {
            DwmExtendFrameIntoClientArea(self.window, &m);
            SetWindowPos(self.window, ptr::null_mut(), 0, 0, 0, 0, 0x0020 | 0x0002 | 0x0001 | 0x0004 | 0x0010);
        }
    }
    /// Die Titelzone in Geraetepixeln: ihre Hoehe, die Bedienelemente darin
    /// (`x, y, breite, hoehe`) und der Maximieren-Knopf.
    pub fn setze_titelzone(&mut self, hoehe: u32, frei: &[(u32, u32, u32, u32)], maximieren: Option<(u32, u32, u32, u32)>) {
        let wandel = |r: &(u32, u32, u32, u32)| (r.0 as i32, r.1 as i32, r.2 as i32, r.3 as i32);
        let mut s = self.state.borrow_mut();
        s.titel = Titelzone { hoehe: hoehe as i32, frei: frei.iter().map(wandel).collect(), maximieren: maximieren.as_ref().map(wandel) };
    }
    pub fn minimieren(&mut self) {
        // SAFETY: lebendes Fenster, SW_MINIMIZE.
        unsafe { ShowWindow(self.window, 6); }
    }
    pub fn maximieren_umschalten(&mut self) {
        // SAFETY: lebendes Fenster, SW_RESTORE bzw. SW_MAXIMIZE.
        unsafe { ShowWindow(self.window, if IsZoomed(self.window) != 0 { 9 } else { 3 }); }
    }
    pub fn ist_maximiert(&self) -> bool {
        // SAFETY: lebendes Fenster.
        unsafe { IsZoomed(self.window) != 0 }
    }
    /// Geraetepixel je logischem Pixel (1.0 bei 96 dpi).
    pub fn skalierung(&self) -> f32 {
        // `NARSIL_SKALIERUNG` (0.5..4) ueberschreibt den Bildschirm — zum
        // Pruefen der HiDPI-Oberflaeche auf einem 96-dpi-Bildschirm.
        if let Some(f) = std::env::var("NARSIL_SKALIERUNG").ok().and_then(|v| v.trim().parse::<f32>().ok())
            && f.is_finite()
            && (0.5..=4.0).contains(&f)
        {
            return f;
        }
        self.state.borrow().dpi.max(48) as f32 / 96.0
    }
    /// Release 08.10.2026: die Bildwiederholrate des Bildschirms, auf dem
    /// das Fenster steht (Hz; `None`, wenn Windows keine nennt). Damit
    /// laufen Animationen auf 120/144/240-Hz-Bildschirmen in deren Takt
    /// statt fest mit 60 Bildern.
    pub fn bildrate(&self) -> Option<u32> {
        // MONITORINFOEXW (104 Byte): cbSize, rcMonitor, rcWork, dwFlags
        // (40 Byte), dann szDevice[32] (UTF-16). Als u32-Feld, damit die
        // Ausrichtung stimmt.
        let mut info = [0u32; 26];
        info[0] = 104;
        // DEVMODEW (220 Byte, x86/x64 little endian): dmSize ist das untere
        // Halbwort bei Byte 68 (= u32 Nr. 17), dmDisplayFrequency Byte 184
        // (= u32 Nr. 46).
        let mut modus = [0u32; 55];
        modus[17] = 220;
        // SAFETY: lebendes Fenster; beide Puffer sind so gross, wie ihre
        // Groessenfelder sagen; MONITOR_DEFAULTTONEAREST (2) liefert immer
        // einen Bildschirm; ENUM_CURRENT_SETTINGS = 0xFFFF_FFFF; szDevice
        // ist NUL-terminiert (Windows schreibt es so).
        let hz = unsafe {
            let monitor = MonitorFromWindow(self.window, 2);
            if monitor.is_null() || GetMonitorInfoW(monitor, info.as_mut_ptr().cast()) == 0 {
                return None;
            }
            let geraet = info.as_ptr().add(10).cast::<u16>();
            if EnumDisplaySettingsW(geraet, 0xFFFF_FFFF, modus.as_mut_ptr().cast()) == 0 {
                return None;
            }
            modus[46]
        };
        // 0 und 1 heissen „Standard der Hardware“.
        (hz > 1 && hz <= 1000).then_some(hz)
    }
    /// Der Fenstertitel (Taskleiste, Alt+Tab).
    pub fn setze_titel(&mut self, titel: &str) {
        let t: String = titel.chars().filter(|c| *c != '\0').take(256).collect();
        let w = wide(&t);
        // SAFETY: lebendes Fenster, NUL-terminierter UTF-16-Text.
        unsafe { SetWindowTextW(self.window, w.as_ptr()); }
    }
    pub fn is_closed(&self) -> bool { self.state.borrow().closed }
    pub fn server_size(&self) -> Result<Size, SurfaceError> {
        let mut rect = Rect::default();
        // SAFETY: live HWND and writable initialized RECT.
        if unsafe { GetClientRect(self.window, &mut rect) } == 0 { return Err(error("GetClientRect")); }
        Ok(Size::new((rect.right - rect.left) as f32, (rect.bottom - rect.top) as f32))
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError> {
        check_dimensions(width, height)?;
        let rect = outer_rect(width, height)?;
        // SAFETY: live HWND; SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE.
        if unsafe { SetWindowPos(self.window, ptr::null_mut(), 0, 0, rect.right - rect.left, rect.bottom - rect.top, 0x0016) } == 0 { return Err(error("SetWindowPos")); }
        Ok(())
    }
    pub fn poll_events(&mut self) -> Result<Vec<SurfaceEvent>, SurfaceError> {
        let mut msg = Message::default();
        // GEMESSENER FEHLER (06.10.2026, ~107 000 Schleifendurchlaeufe je
        // Sekunde am lebenden Fenster): hier stand `PeekMessageW(..,
        // self.window, 0, 0, ..)` — NUR Nachrichten dieses Fensters.
        // `wait_events` fragt aber die THREAD-Warteschlange ab
        // (`MsgWaitForMultipleObjectsEx` mit QS_ALLINPUT): liegt dort
        // eine Nachricht, die nicht fuer dieses Fenster ist (WM_TIMER
        // ohne Fenster, eine Nachricht eines Hilfsfensters), weckt der
        // Wartende SOFORT, und `PeekMessage` holt sie nie ab — der Loop
        // wartet nicht mehr und verbrennt einen Kern, ohne etwas zu tun.
        // GEMESSEN: 643 000 Durchlaeufe in 6 s, davon EINE Zeichnung.
        //
        // DIE BEHEBUNG ist die Standard-Form: die ganze
        // Thread-Warteschlange abarbeiten (hwnd = NULL). Es gibt in
        // diesem Prozess kein anderes Fenster, dessen Nachrichten dabei
        // verloren gehen koennten — und selbst wenn: `DispatchMessage`
        // schickt sie an ihre eigene Fensterprozedur.
        // SAFETY: initialized MSG; the whole thread queue is pumped, as in
        // the canonical Win32 message loop.
        unsafe { while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, 1) != 0 {
            TranslateMessage(&msg); DispatchMessageW(&msg);
        } }
        Ok(std::mem::take(&mut self.state.borrow_mut().events))
    }
    /// Wait until input is available or the animation deadline is reached.
    /// Unlike sleep, this wakes immediately for keyboard and mouse messages.
    pub fn wait_events(&self, timeout: std::time::Duration) -> Result<bool, SurfaceError> {
        if !self.state.borrow().events.is_empty() { return Ok(true); }
        let ms = timeout.as_millis().min((u32::MAX - 1) as u128) as u32;
        // SAFETY: zero handles permits a null pointer; bounded timeout,
        // QS_ALLINPUT and MWMO_INPUTAVAILABLE include already queued input.
        let result = unsafe { MsgWaitForMultipleObjectsEx(0, ptr::null(), ms, 0x04ff, 0x0004) };
        match result {
            0 => Ok(true),
            258 => Ok(false), // WAIT_TIMEOUT
            _ => Err(error("MsgWaitForMultipleObjectsEx")),
        }
    }
    pub fn letzte_presentation(&self) -> Option<(&[u8], u32, u32)> { self.last.as_ref().map(|(p, w, h)| (p.as_slice(), *w, *h)) }
    pub fn repaint(&mut self) -> Result<bool, SurfaceError> {
        if let Some((rgba, w, h)) = &self.last { self.put(rgba, *w, *h)?; return Ok(true); }
        Ok(false)
    }
    fn put(&self, rgba: &[u8], width: u32, height: u32) -> Result<(), SurfaceError> {
        // GEMESSEN (06.10.2026, Nutzerbefund „Fenster laesst sich schlecht
        // minimieren"): ein MINIMIERTES Fenster hat ein LEERES
        // Client-Rechteck. `SetDIBitsToDevice` kopiert dann keine Zeile
        // und meldet 0 — die bisherige Fassung las das als Fehler, und
        // `main` beendet den Browser mit `return EXIT_FENSTER`. Wer
        // waehrend einer Zeichnung minimiert, haette ihn also geschlossen.
        // Ein leeres Ziel ist kein Fehler: der Puffer bleibt in `last`
        // stehen und wird beim Zuruecknehmen neu geschickt (`repaint`).
        let mut client = Rect::default();
        // SAFETY: live HWND and writable initialized RECT.
        if unsafe { GetClientRect(self.window, &mut client) } == 0 {
            return Err(error("GetClientRect"));
        }
        if client.right <= client.left || client.bottom <= client.top {
            return Ok(());
        }
        // Reuse the conversion buffer across frames instead of allocating
        // another full-window image on every keystroke or scroll step.
        let mut bgra = self.bgra.borrow_mut();
        bgra.resize(rgba.len(), 0);
        rgba_nach_bgra(rgba, &mut bgra);
        let info = BitmapInfo { size: 40, width: width as i32, height: -(height as i32), planes: 1,
            bits: 32, compression: 0, image_size: 0, xppm: 0, yppm: 0, used: 0, important: 0, color: 0 };
        // ZIELGROESSE: das Client-Rechteck kann von der Puffergroesse
        // abweichen — nach einem Maximieren, bevor der entprellte Umbau
        // die Seite neu gebaut hat (siehe `umbau_seit` in narsil).
        let ziel_breit = client.right - client.left;
        let ziel_hoch = client.bottom - client.top;
        // SAFETY: live HWND; BGRA storage is complete, top-down and DWORD-aligned.
        unsafe {
            let dc = GetDC(self.window);
            if dc.is_null() { return Err(error("GetDC")); }
            // GEMESSENER NUTZERBEFUND (06.10.2026): „die Seiten
            // verschieben sich beim Minimieren/Maximieren/Verschieben
            // nicht mit". GEMESSENE URSACHE: hier stand IMMER
            // `SetDIBitsToDevice` in Puffergroesse — bei einem 1920er
            // Fenster mit 900er Puffer wurde nur das obere linke Eck
            // gemalt, der Rest blieb stehen. Jetzt wird ein abweichendes
            // Ziel GESTRECKT (`StretchDIBits`, SRCCOPY): die Seite zieht
            // sichtbar mit, bis der Umbau sie in der neuen Breite malt.
            let zeilen = if ziel_breit == width as i32 && ziel_hoch == height as i32 {
                SetDIBitsToDevice(dc, 0, 0, width, height, 0, 0, 0, height, bgra.as_ptr().cast(), &info, 0)
            } else {
                StretchDIBits(dc, 0, 0, ziel_breit, ziel_hoch, 0, 0, width as i32, height as i32,
                    bgra.as_ptr().cast(), &info, 0, 0x00cc_0020)
            };
            ReleaseDC(self.window, dc);
            if zeilen == 0 { return Err(error("SetDIBitsToDevice")); }
        }
        Ok(())
    }
    pub fn warp_pointer(&mut self, x: u32, y: u32) -> Result<(), SurfaceError> {
        let mut point = WinPoint { x: i32::try_from(x).map_err(|_| SurfaceError::InvalidArgument("x".into()))?, y: i32::try_from(y).map_err(|_| SurfaceError::InvalidArgument("y".into()))? };
        // SAFETY: live HWND and initialized point; conversion checked above.
        unsafe { if ClientToScreen(self.window, &mut point) == 0 || SetCursorPos(point.x, point.y) == 0 { return Err(error("SetCursorPos")); } }
        Ok(())
    }
    pub fn send_button_press(&mut self, x: u32, y: u32, button: u8) -> Result<(), SurfaceError> { self.send_button(x, y, button, true) }
    pub fn send_button_release(&mut self, x: u32, y: u32, button: u8) -> Result<(), SurfaceError> { self.send_button(x, y, button, false) }
    fn send_button(&self, x: u32, y: u32, button: u8, press: bool) -> Result<(), SurfaceError> {
        if x > i16::MAX as u32 || y > i16::MAX as u32 { return Err(SurfaceError::InvalidArgument("Mauskoordinate zu gross".into())); }
        let msg = match button { 1 => 0x0201, 2 => 0x0207, 3 => 0x0204, _ => return Err(SurfaceError::InvalidArgument("Maustaste".into())) } + u32::from(!press);
        self.post(msg, 0, (x | (y << 16)) as isize)
    }
    pub fn request_close(&self) -> Result<(), SurfaceError> { self.post(0x0010, 0, 0) }
    fn post(&self, msg: u32, wp: usize, lp: isize) -> Result<(), SurfaceError> {
        // SAFETY: live HWND, messages contain values and no borrowed pointers.
        if unsafe { PostMessageW(self.window, msg, wp, lp) } == 0 { return Err(error("PostMessageW")); }
        Ok(())
    }
}
/// Release 08.10.2026: RGBA -> BGRA in EINEM Durchgang, je Pixel ein
/// 32-Bit-Wort (Rot und Blau tauschen). GEMESSEN vorher (Kopie + Tausch
/// je Byte): 135 ms je Bild bei 1888 x 954 im Debug-Bau — mehr als Raster
/// und Liste zusammen.
fn rgba_nach_bgra(quelle: &[u8], ziel: &mut [u8]) {
    for (z, q) in ziel.chunks_exact_mut(16).zip(quelle.chunks_exact(16)) {
        for i in 0..4 {
            let o = i * 4;
            let w = u32::from_le_bytes([q[o], q[o + 1], q[o + 2], q[o + 3]]);
            let t = (w & 0xff00_ff00) | ((w >> 16) & 0xff) | ((w & 0xff) << 16);
            z[o..o + 4].copy_from_slice(&t.to_le_bytes());
        }
    }
    let rest = quelle.len() / 16 * 16;
    for (z, q) in ziel[rest..].chunks_exact_mut(4).zip(quelle[rest..].chunks_exact(4)) {
        z[0] = q[2];
        z[1] = q[1];
        z[2] = q[0];
        z[3] = q[3];
    }
}

fn check_dimensions(w: u32, h: u32) -> Result<(), SurfaceError> {
    if w == 0 || h == 0 { return Err(SurfaceError::ZeroExtent { width: w, height: h }); }
    if w > 32767 || h > 32767 { return Err(SurfaceError::InvalidArgument("Fenstergroesse groesser als 32767".into())); }
    Ok(())
}
fn outer_rect(w: u32, h: u32) -> Result<Rect, SurfaceError> {
    let mut rect = Rect { left: 0, top: 0, right: w as i32, bottom: h as i32 };
    // SAFETY: initialized writable RECT and constant window style.
    if unsafe { AdjustWindowRectEx(&mut rect, STYLE, 0, 0) } == 0 { return Err(error("AdjustWindowRectEx")); }
    Ok(rect)
}

/// Kappt ein Aussenmass auf die Arbeitsflaeche des Bildschirms.
///
/// GEMESSENER GRUND (06.10.2026, Nutzerwunsch „standardmaessig
/// 1920x1080"): der Rahmen und die Titelleiste kommen ZUR
/// Kundenflaeche hinzu. Ein 1920x1080-Client ist aussen rund 1936x1119,
/// und auf einem 1080p-Bildschirm liegt der obere Rand dann hinter dem
/// Bildschirmrand — die Knoepfe zum Minimieren und Schliessen sind nicht
/// mehr erreichbar. `SPI_GETWORKAREA` (0x0030) liefert die Flaeche ohne
/// Taskleiste; was nicht hineinpasst, wird gekappt.
///
/// Die Untergrenzen (320x240) sind willkuerlich klein gehalten: sie
/// greifen nur, wenn die Arbeitsflaeche selbst winzig gemeldet wird, und
/// ein Fenster mit 0 Breite waere ein Fehler statt einer Antwort.
/// Release 09.10.2026: die Schriftglaettung des Systems, fuer Subpixel-Text
/// in der Leiste. `Some(false)` = ClearType mit RGB-Teilpixeln,
/// `Some(true)` = ClearType mit BGR, `None` = Graustufen oder aus.
///
/// Gelesen wird nur (`SPI_GETFONTSMOOTHING`, `…TYPE`, `…ORIENTATION`) — so
/// folgen Firefox und Chrome der Einstellung „ClearType-Text".
pub fn cleartype_bgr() -> Option<bool> {
    let mut an: i32 = 0;
    let mut art: u32 = 0;
    let mut ordnung: u32 = 1;
    // SAFETY: jede Abfrage fuellt genau den uebergebenen Wert (BOOL/UINT);
    // es wird kein Zeiger gespeichert.
    let ok = unsafe {
        SystemParametersInfoW(0x004A, 0, (&mut an as *mut i32).cast(), 0) != 0
            && SystemParametersInfoW(0x200A, 0, (&mut art as *mut u32).cast(), 0) != 0
    };
    if !ok || an == 0 || art != 2 {
        return None;
    }
    // SAFETY: wie oben. Schlaegt sie fehl, bleibt RGB (die Vorgabe).
    unsafe {
        SystemParametersInfoW(0x2012, 0, (&mut ordnung as *mut u32).cast(), 0);
    }
    Some(ordnung == 0)
}

/// Release 09.10.2026: „Animationseffekte" des Systems an? (Windows-
/// Gegenstueck zu `prefers-reduced-motion`, `SPI_GETCLIENTAREAANIMATION`).
/// Schlaegt die Abfrage fehl, gelten Animationen als erlaubt.
pub fn animationen_an() -> bool {
    let mut an: i32 = 1;
    // SAFETY: die Abfrage fuellt genau den uebergebenen BOOL; kein Zeiger
    // wird gespeichert.
    let ok = unsafe { SystemParametersInfoW(0x1042, 0, (&mut an as *mut i32).cast(), 0) };
    ok == 0 || an != 0
}

fn clamp_to_work_area(rect: Rect, client_w: u32, client_h: u32) -> Rect {
    let mut area = Rect::default();
    // SAFETY: `SPI_GETWORKAREA` fuellt ausschliesslich das uebergebene
    // RECT; es wird kein Zeiger gespeichert.
    let ok = unsafe { SystemParametersInfoW(0x0030, 0, (&mut area as *mut Rect).cast(), 0) };
    if ok == 0 {
        return rect;
    }
    let breit = (area.right - area.left).max(320);
    let hoch = (area.bottom - area.top).max(240);
    if rect.right - rect.left <= breit && rect.bottom - rect.top <= hoch {
        return rect;
    }
    let rahmen_w = (rect.right - rect.left) - client_w as i32;
    let rahmen_h = (rect.bottom - rect.top) - client_h as i32;
    Rect {
        left: rect.left,
        top: rect.top,
        right: rect.left + (breit - rahmen_w).max(320),
        bottom: rect.top + (hoch - rahmen_h).max(240),
    }
}
impl Surface for Win32Surface {
    fn size(&self) -> Size { self.state.borrow().size }
    fn name(&self) -> &str { "Win32" }
    fn present(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), SurfaceError> {
        super::surface::check_present_args(rgba.len(), width, height)?;
        check_dimensions(width, height)?;
        self.put(rgba, width, height)?;
        if let Some((pixels, w, h)) = self.last.as_mut() {
            pixels.resize(rgba.len(), 0);
            pixels.copy_from_slice(rgba);
            *w = width;
            *h = height;
        } else {
            self.last = Some((rgba.to_vec(), width, height));
        }
        Ok(())
    }
}
impl Drop for Win32Surface {
    fn drop(&mut self) {
        WINDOWS.with(|all| all.borrow_mut().remove(&(self.window as usize)));
        // SAFETY: this object uniquely owns the HWND and cannot leave its UI thread (Rc).
        unsafe { DestroyWindow(self.window); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_surface_present_resize_input_close() {
        let mut surface = Win32Surface::open_hidden(120, 80, "Narsil OS test").expect("create");
        assert_eq!(surface.server_size().unwrap(), Size::new(120.0, 80.0));
        assert!(matches!(surface.present(&[], 0, 1), Err(SurfaceError::ZeroExtent { .. })));
        assert!(matches!(surface.present(&[0; 3], 1, 1), Err(SurfaceError::SizeMismatch { .. })));
        let mut rgba = [0x20, 0x40, 0x80, 255].repeat(120 * 80);
        surface.present(&rgba, 120, 80).expect("GDI presentation");
        rgba[0] = 255;
        assert_eq!(surface.letzte_presentation().unwrap().0[0], 0x20);
        assert!(surface.repaint().unwrap());
        surface.resize(160, 90).unwrap();
        assert_eq!(surface.server_size().unwrap(), Size::new(160.0, 90.0));
        surface.send_button_press(17, 23, 1).unwrap();
        surface.send_button_release(17, 23, 1).unwrap();
        surface.post(0x0102, 'ä' as usize, 0).unwrap();
        surface.post(0x0102, 0xd83d, 0).unwrap();
        surface.post(0x0102, 0xde00, 0).unwrap();
        surface.post(0x0100, 13, 0).unwrap();
        surface.post(0x020a, (120usize) << 16, 0).unwrap();
        surface.request_close().unwrap();
        let events = surface.poll_events().unwrap();
        assert!(events.contains(&SurfaceEvent::ButtonPressed { point: Point::new(17.0, 23.0), button: MouseButton::Left }));
        assert!(events.contains(&SurfaceEvent::ButtonReleased { point: Point::new(17.0, 23.0), button: MouseButton::Left }));
        for key in [Key::Char('ä'), Key::Char('😀'), Key::Enter] {
            assert!(events.iter().any(|e| matches!(e, SurfaceEvent::Key(k) if k.pressed && k.key == key)), "{key:?}: {events:?}");
        }
        assert!(events.iter().any(|e| matches!(e, SurfaceEvent::Wheel { delta: ScrollDelta::Lines(n), .. } if *n == -1.0)));
        assert!(events.contains(&SurfaceEvent::CloseRequested));
        assert!(surface.is_closed());
    }

    #[test]
    fn windows_wait_wakes_for_text_without_consuming_it() {
        let mut surface = Win32Surface::open_hidden(120, 80, "input wake").unwrap();
        surface.poll_events().unwrap();
        surface.post(0x0102, '@' as usize, 0).unwrap();
        assert!(surface.wait_events(std::time::Duration::from_secs(2)).unwrap());
        let events = surface.poll_events().unwrap();
        assert!(events.iter().any(|e| matches!(e, SurfaceEvent::Key(k) if k.typed_char() == Some('@'))));
        // Events already translated by poll_events also prevent waiting.
        surface.state.borrow_mut().events.push(SurfaceEvent::CloseRequested);
        assert!(surface.wait_events(std::time::Duration::from_secs(2)).unwrap());
    }

    #[test]
    fn windows_alt_f4_closes_the_window() {
        let mut surface = Win32Surface::open_hidden(120, 80, "Alt F4").unwrap();
        surface.poll_events().unwrap();
        surface.post(0x0104, 0x73, 1 << 29).unwrap();
        let events = surface.poll_events().unwrap();
        assert!(events.contains(&SurfaceEvent::CloseRequested));
        assert!(surface.is_closed());
    }

    #[test]
    fn windows_reuses_frame_buffers_and_converts_color_channels() {
        let mut surface = Win32Surface::open_hidden(120, 80, "buffer reuse").unwrap();
        let rgba = [0x20, 0x40, 0x80, 255].repeat(120 * 80);
        surface.present(&rgba, 120, 80).unwrap();
        let rgba_ptr = surface.letzte_presentation().unwrap().0.as_ptr();
        let bgra_ptr = surface.bgra.borrow().as_ptr();
        let next = [0x12, 0x34, 0x56, 255].repeat(120 * 80);
        surface.present(&next, 120, 80).unwrap();
        assert_eq!(surface.letzte_presentation().unwrap().0, next);
        assert_eq!(surface.letzte_presentation().unwrap().0.as_ptr(), rgba_ptr);
        assert_eq!(surface.bgra.borrow().as_ptr(), bgra_ptr);
        assert_eq!(&surface.bgra.borrow()[..4], &[0x56, 0x34, 0x12, 255]);
        assert!(surface.repaint().unwrap());
        assert_eq!(surface.bgra.borrow().as_ptr(), bgra_ptr);
        surface.present(&[1, 2, 3, 255], 1, 1).unwrap();
        assert_eq!(surface.letzte_presentation().unwrap().0, &[1, 2, 3, 255]);
        assert_eq!(&*surface.bgra.borrow(), &[3, 2, 1, 255]);
    }

    #[test]
    fn windows_altgr_text_is_not_a_shortcut() {
        for c in ['@', '€', '{', '\\'] {
            let e = text_event(c, Modifiers { ctrl: true, alt: true, shift: false }).unwrap();
            let SurfaceEvent::Key(k) = e else { panic!("text event"); };
            assert_eq!(k.typed_char(), Some(c));
            assert!(!k.modifiers.ctrl && !k.modifiers.alt);
        }
        assert!(text_event('c', Modifiers { ctrl: true, alt: false, shift: false }).is_none());
        assert!(text_event('\n', Modifiers::NONE).is_none());
        let SurfaceEvent::Key(k) = text_event('Ä', Modifiers { ctrl: false, alt: false, shift: true }).unwrap() else { panic!("text event"); };
        assert_eq!(k.typed_char(), Some('Ä'));
        assert!(k.modifiers.shift);
    }

    #[test]
    fn windows_invalid_dimensions_and_signed_mouse_coordinates() {
        assert!(matches!(Win32Surface::open_hidden(0, 20, "test"), Err(SurfaceError::ZeroExtent { .. })));
        assert!(matches!(Win32Surface::open_hidden(u32::MAX, 20, "test"), Err(SurfaceError::InvalidArgument(_))));
        assert_eq!(pointer((0xfffeu32 | (0xfffdu32 << 16)) as isize), Point::new(-2.0, -3.0));
    }
}
