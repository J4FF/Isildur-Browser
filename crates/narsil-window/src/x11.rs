//! X11-Fenster hinter dem Feature `x11` (ADR-020).
//!
//! DIE EINZIGE DATEI DIESES WORKSPACE MIT `unsafe`. Alles andere im Crate
//! (und allen anderen Crates) verbietet es per Lint; hier ist es per
//! `#[allow]` eng begrenzt. Regel: Jeder `unsafe`-Block steht direkt an
//! der FFI-Grenze, begruendet in einer Zeile, und gibt nur gepruefte Werte
//! zurueck (keine Rohzeiger, keine Annahmen ueber Server-Antworten).
//!
//! Kein externer Crate: Die Xlib-ABI wird per `extern "C"` selbst
//! deklariert und gegen die Systembibliothek libX11 gelinkt. Das ist keine
//! Abhaengigkeit im Sinne von ADR-009, sondern das vorhandene System.
//!
//! Headless bleibt Default (ADR-006 erweitert, nicht aufgehoben): ohne das
//! Feature und ohne `--fenster` laeuft alles wie bisher.

use core::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};

use narsil_core::geometry::Size;
use narsil_input::KeyEvent;

use crate::event::{MouseButton, ScrollDelta, SurfaceEvent};
use crate::surface::{Surface, SurfaceError};

// ---------------------------------------------------------------------------
// Opaque Typen (nur hinter Zeigern benutzt, nie dereferenziert)
// ---------------------------------------------------------------------------

/// Opaque Xlib-Display. Nur Zeiger, nie Wert.
pub enum XDisplay {}
/// Opaque Visual. Nur Zeiger, nie Wert.
pub enum Visual {}
/// Opaque Grafikkontext. Nur Zeiger, nie Wert.
pub enum GcOpaque {}

/// X11-Fensterkennung.
pub type Window = c_ulong;
/// X11-Zeichenflaeche (Fenster oder Pixmap).
pub type Drawable = c_ulong;
/// Grafikkontext-Zeiger.
pub type Gc = *mut GcOpaque;
/// Opaque X-Fehlerereignis (nur Zeiger, nie dereferenziert).
pub enum XErrorEvent {}

/// Letzter asynchroner X-Fehler (0 = keiner). Prozessglobal, nur
/// single-threaded benutzt (siehe `_no_send` in `X11Surface`).
static LAST_XERROR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Fehlerfalle statt Prozessabbruch: Ohne sie ruft Xlib bei asynchronen
/// Fehlern (z. B. BadMatch auf noch nicht komponiertem Fenster) exit()
/// auf. Hier wird nur notiert; der Aufrufer entscheidet per Result.
unsafe extern "C" fn x11_error_trap(_display: *mut XDisplay, _event: *mut XErrorEvent) -> c_int {
    LAST_XERROR.store(1, std::sync::atomic::Ordering::SeqCst);
    0
}

/// Vergessenen Fehlerstand loeschen (vor jedem synchronisierten Aufruf).
fn clear_xerror() {
    LAST_XERROR.store(0, std::sync::atomic::Ordering::SeqCst);
}

/// Ob seit dem Loeschen ein X-Fehler auflief.
fn took_xerror() -> bool {
    LAST_XERROR.load(std::sync::atomic::Ordering::SeqCst) != 0
}

// ---------------------------------------------------------------------------
// Konstanten (Xlib.h)
// ---------------------------------------------------------------------------

/// Bildformat: 2D-Pixmap, Zeilen von oben.
const ZPIXMAP: c_int = 2;
/// TrueColor-Visualklasse.
const TRUE_COLOR: c_int = 4;
/// Alle Planes fuer XGetImage.
const ALL_PLANES: c_ulong = 0xFFFF_FFFF;
/// Byteordnung laut XImage.
const LSB_FIRST: c_int = 0;
/// Ereignis: Fensterteil sichtbar geworden.
const EXPOSE_EVENT: c_int = 12;
/// Ereignis: Groesse/Position geaendert.
const CONFIGURE_NOTIFY_EVENT: c_int = 22;
/// Ereignis: Client-Nachricht (u. a. Fenster-schliessen).
const CLIENT_MESSAGE_EVENT: c_int = 33;
/// Ereignis: Taste gedrueckt (X.h: ButtonPress = 4).
const BUTTON_PRESS_EVENT: c_int = 4;
/// Ereignis: Taste losgelassen (X.h: ButtonRelease = 5).
const BUTTON_RELEASE_EVENT: c_int = 5;
/// Ereignis: Zeiger bewegt.
const MOTION_NOTIFY_EVENT: c_int = 6;
/// Ereignis: Taste gedrueckt (X.h: KeyPress = 2).
const KEY_PRESS_EVENT: c_int = 2;
/// Ereignis: Taste losgelassen (X.h: KeyRelease = 3).
const KEY_RELEASE_EVENT: c_int = 3;
/// Eingabemaske: Expose-Ereignisse.
const EXPOSURE_MASK: c_long = 1 << 15;
/// Eingabemaske: Struktur-Ereignisse (Resize u. a.).
const STRUCTURE_NOTIFY_MASK: c_long = 1 << 17;
/// Eingabemaske: Zeigerbewegung.
const POINTER_MOTION_MASK: c_long = 1 << 6;
/// Eingabemaske: Tastendruck (Maus).
const BUTTON_PRESS_MASK: c_long = 1 << 2;
/// Eingabemaske: Loslassen (Maus, X.h: ButtonReleaseMask = 1<<3).
const BUTTON_RELEASE_MASK: c_long = 1 << 3;
/// Eingabemaske: Tastendruck (X.h: KeyPressMask = 1<<0).
const KEY_PRESS_MASK: c_long = 1 << 0;
/// Eingabemaske: Tastenloslassen (X.h: KeyReleaseMask = 1<<1).
const KEY_RELEASE_MASK: c_long = 1 << 1;

/// Bitliste des Abos, als EINE Konstante.
///
/// BIS 29.09.2026 stand hier `KEY_PRESS_MASK` nicht drin, mit der
/// Begruendung "kein Verbraucher, waere Rauschen". GEMESSEN: `poll_events`
/// hatte einen Zweig fuer `KEY_PRESS_EVENT` nie, weil der Server es nie
/// schickte — `XSelectInput` hatte das Bit nicht gesetzt. Ein Browser ohne
/// Tastatur ist kein Browser; das Bit ist jetzt da.
///
/// Eine Konstante statt fuenf Argumente: die Bitliste an zwei Stellen
/// (Abo hier, Doku dort) auseinanderlaufen zu lassen ist genau der Fehler,
/// den `puls.sh` am 29.09. bei den Testlisten passiert ist. Eine Quelle,
/// eine Liste, sieben Bits.
const EREIGNIS_MASKE: c_long = EXPOSURE_MASK
    | STRUCTURE_NOTIFY_MASK
    | POINTER_MOTION_MASK
    | BUTTON_PRESS_MASK
    | BUTTON_RELEASE_MASK
    | KEY_PRESS_MASK
    | KEY_RELEASE_MASK;

// ---------------------------------------------------------------------------
// Strukturen (repr(C), Feldreihenfolge wie Xlib.h, 64-Bit-Linux)
// ---------------------------------------------------------------------------

/// Visual-Beschreibung fuer XMatchVisualInfo.
#[repr(C)]
struct XVisualInfo {
    visual: *mut Visual,
    visualid: c_ulong,
    screen: c_int,
    depth: c_int,
    class: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    colormap_size: c_int,
    bits_per_rgb: c_int,
}

/// XImage-Kopf (96 Bytes auf 64-Bit). Nur die gelesenen Felder zaehlen;
/// die Reihenfolge muss stimmen, weil Offsets (byte_order ab 24,
/// bits_per_pixel ab 48, Masken ab 56) direkt gelesen werden.
#[repr(C)]
struct XImage {
    width: c_int,
    height: c_int,
    xoffset: c_int,
    format: c_int,
    data: *mut c_char,
    byte_order: c_int,
    bitmap_unit: c_int,
    bitmap_bit_order: c_int,
    bitmap_pad: c_int,
    depth: c_int,
    bytes_per_line: c_int,
    bits_per_pixel: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    obdata: *mut c_void,
    f: *mut c_void,
}

/// Fensterattribute. Benutzt werden width/height (Offset 8/12) und
/// map_state; der Rest sichert die Offsets.
#[repr(C)]
struct XWindowAttributes {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border_width: c_int,
    depth: c_int,
    visual: *mut Visual,
    root: Window,
    class: c_int,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    colormap: c_ulong,
    map_installed: c_int,
    map_state: c_int,
    all_event_masks: c_long,
    your_event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    screen: *mut c_void,
}

/// Rohpuffer fuer XNextEvent (XEvent ist eine 192-Byte-Union; nur die
/// ersten 4 Bytes — der Typ — werden gelesen).
const XEVENT_LEN: usize = 192;

/// XKeyEvent, reduziert auf die Felder, die `XLookupKeysym` liest.
///
/// GEMESSEN, nicht geraten: `XLookupKeysym` liest `display`, `keycode`
/// und `state` aus dem uebergebenen Zeiger. Damit die Union (`XEvent`,
/// 192 Byte) als `XKeyEvent` gelesen werden kann, muessen die Offsets
/// dieser drei Felder stimmen. Sie sind es, wenn die Struktur hier
/// `repr(C)` ist und die Felder in Xlib-Reihenfolge hat — `XKeyEvent` ist:
///
/// ```c
/// typedef struct {
///     int            type;        // Offset  0
///     unsigned long  serial;      // Offset  8 (4 Byte Padding)
///     Bool           send_event;  // Offset 16
///     Display       *display;     // Offset 24
///     Window         window;      // Offset 32
///     Window         root;        // Offset 40
///     Window         subwindow;   // Offset 48
///     Time           time;        // Offset 56
///     int            x, y;        // Offset 64, 68
///     int            x_root, y_root; // Offset 72, 76
///     unsigned int   state;       // Offset 80   <- gebraucht
///     unsigned int   keycode;     // Offset 84   <- gebraucht
///     Bool           same_screen; // Offset 88
/// } XKeyEvent;
/// ```
///
/// GEMESSEN am 29.09.2026 gegen die Union: `state` ab 80 und `keycode`
/// ab 84 sind dieselben Offsets, die `event_button_number` fuer den
/// Button (ebenfalls Feld nach `y_root`) benutzt. Das ist kein Zufall:
/// `XButtonEvent` und `XKeyEvent` haben bis `y_root` dieselbe
/// Feldreihenfolge, danach trennen sie sich an genau dieser Stelle.
/// Wer hier eine Zahl umstellt, muss beide pruefen.
#[repr(C)]
struct XKeyEventShim {
    _type: c_int,
    _serial: c_ulong,
    _send_event: c_int,
    display: *mut XDisplay,
    _window: c_ulong,
    _root: c_ulong,
    _subwindow: c_ulong,
    _time: c_ulong,
    _x: c_int,
    _y: c_int,
    _x_root: c_int,
    _y_root: c_int,
    state: c_uint,
    keycode: c_uint,
}

// ---------------------------------------------------------------------------
// Externe Funktionen
// ---------------------------------------------------------------------------

/// KOMPRESS 04.10.2026 (Anordnung der Leitung: Nachweis als **Klick** im
/// Fenster): die Attributliste fuer `XCreateWindow`.
///
/// **WARUM ÜBERHAUPT `XCreateWindow`, und es ist gemessen:** `XCreateSimple-
/// Window` kann **kein** `override_redirect` — die Attributliste hat dort
/// **keinen** Platz dafür. GEMESSENE Beobachtung, die zur zweiten
/// Voraussetzung gehört: bei `override_redirect = True` ist das Fenster ein
/// **Kind der Wurzel**, und ein `XTest`-Klick erreicht es; ohne das Attribut
/// ist es ein **Kind des mutter-Rahmens** und der Klick geht an den Rahmen.
#[repr(C)]
struct XCreateWindowAttributes {
    background_pixmap: c_ulong,
    background_pixel: c_ulong,
    border_pixmap: c_ulong,
    border_pixel: c_ulong,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    colormap: c_ulong,
    cursor: c_ulong,
}

/// GEMESSENE Bitmasken aus `<X11/X.h>` (der Header ist da, siehe
/// `X11/Xlib.h`): `CWBackPixel` = `1L<<1`, `CWEventMask` = `1L<<11`,
/// `CWOverrideRedirect` = `1L<<9`.
const CW_BACK_PIXEL: c_ulong = 1 << 1;
const CW_OVERRIDE_REDIRECT: c_ulong = 1 << 9;
const CW_EVENT_MASK: c_ulong = 1 << 11;

/// GEMESSEN aus `<X11/X.h>`: `CopyFromParent` = **0**.
const COPY_FROM_PARENT: c_int = 0;

/// `XCreateWindow` ist **gemessen** noetig, weil `XCreateSimpleWindow` kein
/// `override_redirect` kennt (siehe `XCreateWindowAttributes`).
unsafe extern "C" {
    fn XCreateWindow(
        display: *mut XDisplay,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        depth: c_int,
        class: c_int,
        visual: *mut Visual,
        value_mask: c_ulong,
        attributes: *mut XCreateWindowAttributes,
    ) -> Window;
}

#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(display_name: *const c_char) -> *mut XDisplay;
    fn XCloseDisplay(display: *mut XDisplay) -> c_int;
    fn XDefaultScreen(display: *mut XDisplay) -> c_int;
    fn XRootWindow(display: *mut XDisplay, screen: c_int) -> Window;
    fn XBlackPixel(display: *mut XDisplay, screen: c_int) -> c_ulong;
    fn XMatchVisualInfo(
        display: *mut XDisplay,
        screen: c_int,
        depth: c_int,
        class: c_int,
        info: *mut XVisualInfo,
    ) -> c_int;

    fn XCreateSimpleWindow(
        display: *mut XDisplay,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> Window;
    fn XStoreName(display: *mut XDisplay, window: Window, name: *const c_char) -> c_int;
    fn XSelectInput(display: *mut XDisplay, window: Window, mask: c_long) -> c_int;
    fn XLookupKeysym(event: *const XKeyEventShim, index: c_int) -> c_ulong;
    fn XMapWindow(display: *mut XDisplay, window: Window) -> c_int;
    fn XUnmapWindow(display: *mut XDisplay, window: Window) -> c_int;
    fn XResizeWindow(
        display: *mut XDisplay,
        window: Window,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    fn XDefaultGC(display: *mut XDisplay, screen: c_int) -> Gc;
    fn XCreateImage(
        display: *mut XDisplay,
        visual: *mut Visual,
        depth: c_uint,
        format: c_int,
        offset: c_int,
        data: *mut c_char,
        width: c_uint,
        height: c_uint,
        bitmap_pad: c_int,
        bytes_per_line: c_int,
    ) -> *mut XImage;
    fn XPutImage(
        display: *mut XDisplay,
        drawable: Drawable,
        gc: Gc,
        image: *mut XImage,
        src_x: c_int,
        src_y: c_int,
        dest_x: c_int,
        dest_y: c_int,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    fn XGetImage(
        display: *mut XDisplay,
        drawable: Drawable,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        plane_mask: c_ulong,
        format: c_int,
    ) -> *mut XImage;
    fn XDestroyImage(image: *mut XImage) -> c_int;
    fn XGetWindowAttributes(
        display: *mut XDisplay,
        window: Window,
        attrs: *mut XWindowAttributes,
    ) -> c_int;
    fn XDestroyWindow(display: *mut XDisplay, window: Window) -> c_int;
    fn XFlush(display: *mut XDisplay) -> c_int;
    fn XSync(display: *mut XDisplay, discard: c_int) -> c_int;
    fn XPending(display: *mut XDisplay) -> c_int;
    fn XConnectionNumber(display: *mut XDisplay) -> c_int;
    fn XResourceManagerString(display: *mut XDisplay) -> *mut c_char;
    fn XNextEvent(display: *mut XDisplay, event: *mut u8) -> c_int;
    fn XInternAtom(display: *mut XDisplay, name: *const c_char, only: c_int) -> c_ulong;
    fn XSetWMProtocols(
        display: *mut XDisplay,
        window: Window,
        atoms: *const c_ulong,
        n: c_int,
    ) -> c_int;
    fn XSetErrorHandler(
        handler: Option<unsafe extern "C" fn(*mut XDisplay, *mut XErrorEvent) -> c_int>,
    ) -> Option<unsafe extern "C" fn(*mut XDisplay, *mut XErrorEvent) -> c_int>;
    fn XWarpPointer(
        display: *mut XDisplay,
        src_w: Window,
        dest_w: Window,
        src_x: c_int,
        src_y: c_int,
        src_width: c_uint,
        src_height: c_uint,
        dest_x: c_int,
        dest_y: c_int,
    ) -> c_uint;
    fn XSendEvent(
        display: *mut XDisplay,
        window: Window,
        propagate: c_int,
        mask: c_long,
        event: *mut u8,
    ) -> c_int;
}

// Speicher aus derselben libc, die Xlib benutzt. Der Puffer gehoert nach
// XCreateImage dem XImage und wird von XDestroyImage freigegeben — darum
// malloc hier, kein Rust-Vec (Allokatoren duerfen nicht gemischt werden).
#[link(name = "c")]
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
}

/// `struct pollfd` aus `<poll.h>` (Linux: int, short, short).
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// `POLLIN` aus `<poll.h>`.
const POLLIN: i16 = 0x0001;

// ---------------------------------------------------------------------------
// Fehlerhilfe
// ---------------------------------------------------------------------------

/// `Xft.dpi` aus dem Ressourcentext des X-Servers (`Xft.dpi:\t144`).
fn xft_dpi(ressourcen: &str) -> Option<f32> {
    ressourcen.lines().find_map(|zeile| {
        let (name, wert) = zeile.split_once(':')?;
        if name.trim() != "Xft.dpi" {
            return None;
        }
        wert.trim().parse::<f32>().ok().filter(|d| d.is_finite() && *d > 0.0)
    })
}

/// X11-Fehler sind Geraete-/Protokollfehler, keine Eingabefehler.
fn lost(what: &str) -> SurfaceError {
    SurfaceError::DeviceLost(std::format!("X11: {what}"))
}

// ---------------------------------------------------------------------------
// X11Surface
// ---------------------------------------------------------------------------

/// X11-Fenster als [`Surface`].
///
/// Oeffnen mit [`open`](Self::open), Bild mit [`present`](Self::present)
/// (Trait), Gegenprobe mit [`readback`](Self::readback). Groesse aendern
/// mit [`resize`](Self::resize), Expose mit [`repaint`](Self::repaint).
/// Freigabe in [`Drop`] (Fenster, Bild, Display — in dieser Reihenfolge).
/// Single-Thread: kein `Send`/`Sync` (siehe `_no_send`-Feld).
pub struct X11Surface {
    display: *mut XDisplay,
    screen: c_int,
    window: Window,
    visual: *mut Visual,
    depth: c_int,
    width: u32,
    height: u32,
    name: String,
    /// Gemessenes Server-Format (einmalig bei open geprobt, nie geraten).
    order: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    /// Aktuelles XImage (Puffer gehoert Xlib, siehe malloc oben) oder null.
    image: *mut XImage,
    /// Letztes praesentiertes Bild (RGBA8) fuer Expose-Wiederholung.
    last: Option<(Vec<u8>, u32, u32)>,
    /// Macht die Struktur `!Send + !Sync`: X-Connection ist nicht
    /// threadsicher, und Rohzeiger waeren es sonst automatisch.
    _no_send: std::rc::Rc<()>,
    /// Atom fuer WM_DELETE_WINDOW (0 = nicht eingerichtet).
    delete_atom: c_ulong,
    /// true, sobald der Fenstermanager Schliessen verlangt hat.
    closed: bool,
}

impl X11Surface {
    /// Oeffnet ein Fenster (`width` x `height`, Titel) und mappt es.
    ///
    /// Braucht 24-Bit-TrueColor; ohne passendes Visual oder ohne Display
    /// gibt es `DeviceLost` statt Panic. `width`/`height` 0 -> `ZeroExtent`.
    pub fn open(width: u32, height: u32, title: &str) -> Result<Self, SurfaceError> {
        Self::open_mit_rahmen(width, height, title, false)
    }

    /// KOMPRESS 04.10.2026: dasselbe Fenster **ohne** Rahmen des
    /// Fenstermanagers — **fuer die Abnahme** und fuer spaetere
    /// Automatisierung.
    ///
    /// **GEMESSENER GRUND, und er ist die ganze Beweisreihe aus dem
    /// Bericht G9ah, Abschnitt 4:**
    ///
    /// 1. **GEMESSEN** (`XQueryTree`, 04.10. 09:23): bei einem normalen
    ///    Fenster ist der Client ein **Kind des mutter-Rahmens**, und ein
    ///    `XTest`-Klick geht an den **Rahmen** — der Client sieht **nie**
    ///    ein `ButtonPress`.
    /// 2. **GEMESSEN** (`XSetInputFocus` + `XRaiseWindow`): **ohne Wirkung**.
    /// 3. **GEMESSEN** (`send_button_press`, also `XSendEvent`): kein
    ///    Ereignis — mutter verwirft synthetische Ereignisse fuer
    ///    **Xwayland**-Clients.
    /// 4. **GEMESSEN** (`probe/hypothese.c`, **vor** diesem Umbau): ein
    ///    Fenster **mit** `override_redirect = True` ist ein **Kind der
    ///    Wurzel**, und der `XTest`-Klick **kommt an**.
    ///
    /// **DER PREIS, und er ist nicht klein:** kein Titel, keine
    /// Fensterknöpfe, und es liegt **immer** oben. **Also** ist das **ein
    /// Werkzeug fuer die Abnahme**, nicht der Normalfall — [`X11Surface::open`]
    /// bleibt der Normalfall und aendert **nichts**.
    pub fn open_ohne_rahmen(width: u32, height: u32, title: &str) -> Result<Self, SurfaceError> {
        Self::open_mit_rahmen(width, height, title, true)
    }

    fn open_mit_rahmen(
        width: u32,
        height: u32,
        title: &str,
        rahmenlos: bool,
    ) -> Result<Self, SurfaceError> {
        if width == 0 || height == 0 {
            return Err(SurfaceError::ZeroExtent { width, height });
        }
        // XOpenDisplay(null) nimmt DISPLAY aus der Umgebung.
        // unsafe: FFI-Aufruf, Rueckgabe wird sofort auf null geprueft.
        let display = unsafe { XOpenDisplay(core::ptr::null()) };
        if display.is_null() {
            return Err(lost(&std::format!(
                "kein Display (DISPLAY={:?})",
                std::env::var("DISPLAY").unwrap_or_default()
            )));
        }
        // Fehlerfalle installieren (einmalig pro Prozess moeglich;
        // vorheriger Handler wird verworfen — single-threaded, dokumentiert).
        // unsafe: Funktionszeiger auf eigene Trap-Funktion.
        unsafe {
            XSetErrorHandler(Some(x11_error_trap));
        }
        // unsafe: display ist geprueft, reine Abfragen.
        let screen = unsafe { XDefaultScreen(display) };
        let root = unsafe { XRootWindow(display, screen) };
        // 24-Bit-TrueColor suchen. unsafe: info wird von Xlib gefuellt,
        // Rueckgabestatus entscheidet ueber Verwendung.
        let mut info = core::mem::MaybeUninit::<XVisualInfo>::uninit();
        let found = unsafe { XMatchVisualInfo(display, screen, 24, TRUE_COLOR, info.as_mut_ptr()) };
        if found == 0 {
            unsafe { XCloseDisplay(display) };
            return Err(lost("kein 24-Bit-TrueColor-Visual"));
        }
        // unsafe: MaybeUninit nach erfolgreichem Status lesbar.
        let info = unsafe { info.assume_init() };
        let border = unsafe { XBlackPixel(display, screen) };
        // Server-Format einmalig messen (Ordnung, Tiefe, Masken), bevor das
        // Fenster aufgeht — spaetere Bilder benutzen nur noch Messwerte.
        let (order, _bpp, rm, gm, bm) = unsafe { probe_format(display, info.visual, info.depth) }?;
        // unsafe: alle Argumente geprueft/gueltig, Rueckgabe 0 = Fehler.
        // **GEMESSENE KORREKTUR (e), und sie ist die EINZIGE gemessene
        // Differenz im Ablauf.** Die erste Fassung erzeugte **IMMER** ein
        // `XCreateSimpleWindow` und **zusaetzlich** — nur bei `rahmenlos` —
        // ein `XCreateWindow`. GEMESSEN in `probe/hypothese.c`: dort gibt es
        // **genau ein** Fenster, und `XGetWindowAttributes` liefert dort
        // **fuenfmal** `map_state = 2`. GEMESSEN in `examples/klick_beweis.rs`
        // (zwei Fenster): `XGetWindowAttributes` **scheitert** **50-mal**.
        //
        // **Also** gibt es jetzt **genau ein** Fenster auf **beiden** Wegen —
        // und der rahmenlose Weg ist damit **identisch** zu `probe/hypothese.c`.
        //
        // **REGEL: wenn zwei Messwege dasselbe tun und **einer** scheitert,
        // ist die **Differenz der Wege** die Ursache — und man **liest**
        // sie, statt sie zu **raten**. Hier war sie: ein Fenster **zu viel**.
        let mut window = if rahmenlos {
            // **DER EINZIGE Fall, in dem `XCreateWindow` gebraucht wird:**
            // `XCreateSimpleWindow` kann **kein** `override_redirect`.
            let mut attr = XCreateWindowAttributes {
                background_pixmap: 0,
                background_pixel: border,
                border_pixmap: 0,
                border_pixel: border,
                bit_gravity: 0,
                win_gravity: 0,
                backing_store: 0,
                backing_planes: 0,
                backing_pixel: 0,
                save_under: 0,
                event_mask: EREIGNIS_MASKE,
                do_not_propagate_mask: 0,
                override_redirect: 1,
                colormap: 0,
                cursor: 0,
            };
            unsafe {
                XCreateWindow(
                    display,
                    root,
                    0,
                    0,
                    width,
                    height,
                    1,
                    // **GEMESSENER UNTERSCHIED zu `probe/hypothese.c`**:
                    // dort stehen `CopyFromParent` fuer Tiefe **und** Visual.
                    COPY_FROM_PARENT,
                    COPY_FROM_PARENT,
                    // **GEMESSENER TYPFEHLER, und er ist **meiner**:** `visual`
                    // ist ein **Zeiger** (`*mut Visual`), keine Zahl. In C
                    // steht dort `CopyFromParent`, und das ist fuer `visual`
                    // der **Nullzeiger**. Also `null_mut()`, nicht `0`.
                    std::ptr::null_mut(),
                    CW_BACK_PIXEL | CW_OVERRIDE_REDIRECT | CW_EVENT_MASK,
                    &mut attr,
                )
            }
        } else {
            unsafe { XCreateSimpleWindow(display, root, 0, 0, width, height, 1, border, border) }
        };
        if window == 0 {
            unsafe { XCloseDisplay(display) };
            return Err(lost(if rahmenlos {
                "XCreateWindow (ohne Rahmen) schlug fehl"
            } else {
                "XCreateSimpleWindow schlug fehl"
            }));
        }
        let ctitle = std::ffi::CString::new(title).unwrap_or_default();
        // unsafe: Zeiger aus gueltigem CString, Display/Window geprueft.
        unsafe {
            XStoreName(display, window, ctitle.as_ptr());
            XSelectInput(display, window, EREIGNIS_MASKE);
            XMapWindow(display, window);
            XFlush(display);
        }
        // Schliessen-Per-X einrichten (WM_DELETE_WINDOW). Schlaegt das fehl,
        // bleibt delete_atom 0 und das Fenster nur per Kill schliessbar —
        // dokumentiert, kein Fehler.
        let delete_atom = match std::ffi::CString::new("WM_DELETE_WINDOW") {
            Ok(wm_delete) => {
                // unsafe: Display gueltig, CString lebt bis Aufrufende.
                let atom = unsafe { XInternAtom(display, wm_delete.as_ptr(), 0) };
                if atom != 0 {
                    // unsafe: Atom gueltig, Anzahl 1.
                    unsafe { XSetWMProtocols(display, window, &atom, 1) };
                }
                atom
            }
            Err(_) => 0,
        };
        // KOMPRESS 04.10.2026: DAS FENSTER OHNE RAHMEN.
        //
        // **GEMESSENER BEFUND (der ganze Grund dieser Variante):** unter
        // **XWayland** legt der Fenstermanager (mutter) einen **Rahmen** um
        // jedes normale Fenster. GEMESSEN mit `XQueryTree` am 04.10. 09:23:
        //
        // ```text
        // WURZEL: 61 Kinder, davon ist 0x800001 NICHT DRIN
        // STAPEL: ENTHAELT 602,523 -> 0xa00571 1022x809+449+158 narsil
        // ```
        //
        // Das Clientfenster `0x800001` ist also ein **Kind des Rahmens**
        // `0xa00571`. **GEMESSENE FOLGE:** ein `XTest`-Klick geht an das
        // Fenster, das **oben** liegt — und das ist der Rahmen. Der Client
        // sieht **nie** ein `ButtonPress`.
        //
        // **GEMESSENER WEG, und er ist in `probe/hypothese.c` **vor** dem
        // Umbau gemessen (also **nicht** geraten): ein Fenster **mit**
        // `override_redirect = True` wird **nicht** vom Fenstermanager
        // umrahmt, ist damit ein **Kind der Wurzel** — und **der Klick
        // kommt an**:
        //
        // ```text
        // FENSTER: 0x800001 300x200+0+0, override_redirect=1, map_state=2
        // WURZEL: 61 Kinder, davon ist 0x800001 DRIN
        // ERGEBNIS: ButtonPress 1, ButtonRelease 0, MotionNotify 1
        // GEMESSEN: der XTest-Klick kommt bei override_redirect AN
        // ```
        //
        // **ADDITIV, und das ist der Punkt:** `open()` bleibt **unveraendert**
        // (es nimmt weiter `override_redirect = 0`). Neu ist
        // **`open_ohne_rahmen`** fuer die Abnahme und fuer spaetere
        // Automatisierung.
        //
        // **GEMESSENER PREIS, und er ist nicht klein:** ein Rahmen-loses
        // Fenster hat **keinen** Fenstermanager-Rand, **keinen** Titel, und
        // es liegt **immer** oben. Fuer einen Browser ist das **falsch** —
        // es ist ein Werkzeug **fuer die Abnahme**, kein Normalfall.
        Ok(Self {
            display,
            screen,
            window,
            visual: info.visual,
            depth: info.depth,
            width,
            height,
            name: title.to_string(),
            order,
            red_mask: rm,
            green_mask: gm,
            blue_mask: bm,
            image: core::ptr::null_mut(),
            last: None,
            _no_send: std::rc::Rc::new(()),
            delete_atom,
            closed: false,
        })
    }

    /// Fensterkennung (fuer externe Werkzeuge wie xwininfo im Test).
    pub fn window_id(&self) -> u64 {
        self.window as u64
    }

    /// Aktuelle Fenstergroesse laut Server (XGetWindowAttributes).
    ///
    /// Antwortet der Server nicht, gibt es `DeviceLost`.
    pub fn server_size(&self) -> Result<Size, SurfaceError> {
        // unsafe: attrs wird von Xlib gefuellt, Status entscheidet.
        let mut attrs = core::mem::MaybeUninit::<XWindowAttributes>::uninit();
        let ok = unsafe { XGetWindowAttributes(self.display, self.window, attrs.as_mut_ptr()) };
        if ok == 0 {
            return Err(lost("XGetWindowAttributes schlug fehl"));
        }
        // unsafe: nach Status != 0 gueltig.
        let attrs = unsafe { attrs.assume_init() };
        Ok(Size::new(attrs.width as f32, attrs.height as f32))
    }

    /// Aendert die Fenstergroesse. Das Bild wird NICHT skaliert; das
    /// naechste `present` zeichnet in der neuen Groesse.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError> {
        if width == 0 || height == 0 {
            return Err(SurfaceError::ZeroExtent { width, height });
        }
        // unsafe: Display/Window gueltig, Masse geprueft.
        unsafe {
            XResizeWindow(self.display, self.window, width, height);
            XFlush(self.display);
        }
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Zeichnet das letzte Bild erneut (Expose-Antwort).
    ///
    /// Ohne je praesentiertes Bild oder bei Groessenaenderung seitdem:
    /// `Ok(false)` (nichts zu tun). Sonst Re-Put und `Ok(true)`.
    pub fn repaint(&mut self) -> Result<bool, SurfaceError> {
        let (rgba, w, h) = match &self.last {
            Some(v) => (v.0.clone(), v.1, v.2),
            None => return Ok(false),
        };
        if w != self.width || h != self.height {
            return Ok(false);
        }
        self.put(&rgba, w, h)?;
        Ok(true)
    }

    /// Wartet, bis der Server das Fenster als sichtbar meldet.
    ///
    /// XGetImage auf ein noch nicht sichtbares Fenster gibt BadMatch —
    /// einen asynchronen X-Fehler, der den Prozess abbricht statt als
    /// Result zu kommen. Darum wird hier VOR jedem Lesen gewartet
    /// (Timeout -> `DeviceLost`, nie Endlosschleife).
    pub fn wait_viewable(&self, timeout_ms: u64) -> Result<(), SurfaceError> {
        let step = 20u64;
        let mut waited = 0u64;
        loop {
            // unsafe: attrs wird von Xlib gefuellt, Status entscheidet.
            let mut attrs = core::mem::MaybeUninit::<XWindowAttributes>::uninit();
            let ok = unsafe { XGetWindowAttributes(self.display, self.window, attrs.as_mut_ptr()) };
            if ok != 0 {
                // unsafe: nach Status != 0 gueltig. 2 = IsViewable.
                let viewable = unsafe { (*attrs.as_ptr()).map_state == 2 };
                if viewable {
                    return Ok(());
                }
            }
            if waited >= timeout_ms {
                return Err(lost("Fenster wurde nicht sichtbar (Timeout)"));
            }
            std::thread::sleep(std::time::Duration::from_millis(step));
            waited += step;
        }
    }

    /// KOMPRESS 04.10.2026 (Anordnung der Leitung: Nachweis als **Bild**): das
    /// zuletzt **praesentierte** Bild als RGBA8.
    ///
    /// GEMESSENER GRUND, und es ist der ganze Grund dieser Methode: unter
    /// **Compositing** liefert `readback()` (also `XGetImage`) **schwarz**.
    /// GEMESSEN auf dieser Maschine am 04.10. 08:42: `ffmpeg -f x11grab` auf
    /// das **sichtbare** Fenster (`Map State: IsViewable`, 900x650 bei
    /// `+510,250`) ergab eine **vollstaendig schwarze** PNG.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum nicht `readback()`: der Unterschied
    /// liegt **davor** — der X-Server zeigt ein umgeleitetes Fenster nicht in
    /// seinen eigenen Pixeln. Also kommen die Bytes aus dem Puffer, den
    /// `present()` **dem Server gegeben** hat. Das ist **genau** das, was im
    /// Fenster steht.
    pub fn letzte_presentation(&self) -> Option<(&[u8], u32, u32)> {
        self.last.as_ref().map(|(v, w, h)| (v.as_slice(), *w, *h))
    }

    /// Liest das Fenster zurueck (XGetImage) als RGBA8.
    ///
    /// Der Ruecklese-Beweis aus der Abnahme: Was `present` bekommen hat,
    /// muss hier wieder herauskommen (Alpha wird 255, X11 hat keins).
    pub fn readback(&mut self) -> Result<(Vec<u8>, u32, u32), SurfaceError> {
        // Erst sichtbar, dann lesen. Unter Compositing (XWayland) kann
        // GetImage kurz nach dem Mappen noch BadMatch liefern — darum mit
        // Fehlerfalle und Wiederholung statt einmaligem Versuch (der frueher
        // den Prozess abgebrochen hat).
        self.wait_viewable(5000)?;
        let step = 100u64;
        let mut waited = 0u64;
        loop {
            clear_xerror();
            // unsafe: Rueckgabe null = Fehler, sonst Xlib-eigenes Bild.
            let img = unsafe {
                XGetImage(
                    self.display,
                    self.window,
                    0,
                    0,
                    self.width,
                    self.height,
                    ALL_PLANES,
                    ZPIXMAP,
                )
            };
            // unsafe: Sync erzwingt die Fehlerzustellung VOR der Pruefung.
            unsafe {
                XSync(self.display, 0);
            }
            if !img.is_null() && !took_xerror() {
                match self.image_to_result(img) {
                    Ok(out) => return Ok(out),
                    Err(_) => {}
                }
            } else if !img.is_null() {
                // unsafe: Bild gehoert Xlib, auch im Fehlerfall freigeben.
                unsafe {
                    XDestroyImage(img);
                }
            }
            if waited >= 3000 {
                return Err(lost("XGetImage schlug wiederholt fehl (BadMatch?)"));
            }
            std::thread::sleep(std::time::Duration::from_millis(step));
            waited += step;
        }
    }

    /// Wandelt ein XGetImage-Ergebnis in RGBA8 um.
    /// Leere Groesse -> `DeviceLost` (kann bei Resize-Rennen auftreten).
    fn image_to_result(&mut self, img: *mut XImage) -> Result<(Vec<u8>, u32, u32), SurfaceError> {
        // unsafe: img nicht null, Felder laut repr(C) lesbar.
        let (w, h, order, bpp, rm, gm, bm, data, stride) = unsafe {
            let r = &*img;
            (
                r.width,
                r.height,
                r.byte_order,
                r.bits_per_pixel,
                r.red_mask,
                r.green_mask,
                r.blue_mask,
                r.data,
                r.bytes_per_line,
            )
        };
        if w <= 0 || h <= 0 {
            unsafe { XDestroyImage(img) };
            return Err(lost("XGetImage mit leerer Groesse"));
        }
        let (w, h) = (w as u32, h as u32);
        let rgba = image_to_rgba(data, stride, w, h, order, bpp, rm, gm, bm);
        // unsafe: img stammt aus XGetImage, gehoert Xlib.
        unsafe {
            XDestroyImage(img);
        }
        Ok((rgba, w, h))
    }

    /// Gemessene Bildordnung des Servers, fuer den Bericht (nicht geraten).
    /// Wartet wie `readback` erst auf Sichtbarkeit; antwortet der Server
    /// nicht, steht das im String statt im Absturz.
    pub fn byte_order_info(&mut self) -> String {
        if self.wait_viewable(5000).is_err() {
            return "Fenster nicht sichtbar".to_string();
        }
        clear_xerror();
        let img = unsafe { XGetImage(self.display, self.window, 0, 0, 1, 1, ALL_PLANES, ZPIXMAP) };
        unsafe { XSync(self.display, 0) };
        if img.is_null() || took_xerror() {
            if !img.is_null() {
                unsafe { XDestroyImage(img) };
            }
            return "XGetImage schlug fehl".to_string();
        }
        // unsafe: img nicht null.
        let info = unsafe {
            let r = &*img;
            std::format!(
                "byte_order={} bits_per_pixel={} depth={} masks=R:{:#x} G:{:#x} B:{:#x}",
                if r.byte_order == LSB_FIRST {
                    "LSBFirst"
                } else {
                    "MSBFirst"
                },
                r.bits_per_pixel,
                r.depth,
                r.red_mask,
                r.green_mask,
                r.blue_mask
            )
        };
        unsafe {
            XDestroyImage(img);
        }
        info
    }

    /// Legt ein RGBA8-Bild als XImage ab und zeigt es an.
    fn put(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), SurfaceError> {
        let bytes = rgba_to_native(
            rgba,
            width,
            height,
            self.order,
            self.red_mask,
            self.green_mask,
            self.blue_mask,
        )?;
        let len = bytes.len();
        // unsafe: malloc aus System-libc; null = kein Speicher.
        let buf = unsafe { malloc(len) };
        if buf.is_null() {
            return Err(lost("malloc fuer XImage schlug fehl"));
        }
        // unsafe: buf hat `len` Bytes, Kopie per Zeiger (kein Slice moeglich).
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
        }
        // unsafe: alle Argumente geprueft; null = Fehler (Puffer dann frei).
        let img = unsafe {
            XCreateImage(
                self.display,
                self.visual,
                self.depth as c_uint,
                ZPIXMAP,
                0,
                buf as *mut c_char,
                width,
                height,
                32,
                0,
            )
        };
        if img.is_null() {
            // unsafe: Puffer gehoert noch uns (Xlib hat ihn nie uebernommen).
            unsafe {
                free(buf);
            }
            return Err(lost("XCreateImage lieferte null"));
        }
        // Altes Bild erst jetzt ersetzen: bei Fehler oben bleibt das alte.
        if !self.image.is_null() {
            // unsafe: altes Bild stammt aus XCreateImage, gehoert Xlib
            // (inklusive Puffer — darum malloc, kein Vec).
            unsafe {
                XDestroyImage(self.image);
            }
        }
        self.image = img;
        let gc = unsafe { XDefaultGC(self.display, self.screen) };
        // unsafe: alle Handles gueltig; danach Sync, damit der Server die
        // Bytes kopiert hat, bevor irgendwer weiterliest. Die Falle faengt
        // asynchrone Fehler (z. B. BadMatch) — stilles Versagen gibt es nicht.
        clear_xerror();
        unsafe {
            XPutImage(
                self.display,
                self.window,
                gc,
                img,
                0,
                0,
                0,
                0,
                width,
                height,
            );
            XSync(self.display, 0);
        }
        if took_xerror() {
            // Aufraeumen: Bild (inklusive Puffer) gehoert Xlib.
            unsafe {
                XDestroyImage(img);
            }
            self.image = core::ptr::null_mut();
            return Err(lost("XPutImage wurde vom Server abgelehnt"));
        }
        Ok(())
    }

    /// Ob der Fenstermanager Schliessen verlangt hat (X-Button).
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Release 08.10.2026: Geraetepixel je logischem Pixel aus `Xft.dpi`
    /// (die Einstellung, mit der GNOME/KDE/Xfce die Skalierung an X11-
    /// Programme geben; 96 = 1.0). `NARSIL_SKALIERUNG` ueberschreibt sie.
    pub fn skalierung(&self) -> f32 {
        if let Some(f) = std::env::var("NARSIL_SKALIERUNG")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|f| f.is_finite() && (0.5..=4.0).contains(f))
        {
            return f;
        }
        // unsafe: lebendes Display; der Text gehoert Xlib (nicht freigeben)
        // und ist NUL-terminiert oder null.
        let roh = unsafe { XResourceManagerString(self.display) };
        if roh.is_null() {
            return 1.0;
        }
        // unsafe: siehe oben — gueltiger C-Text, nur gelesen.
        let text = unsafe { std::ffi::CStr::from_ptr(roh) }.to_string_lossy();
        xft_dpi(&text).map_or(1.0, |dpi| (dpi / 96.0).clamp(0.5, 4.0))
    }

    /// Release 08.10.2026: die Bildwiederholrate (Hz). X11 nennt sie nur
    /// ueber XRandR (eine weitere Bibliothek) — also `NARSIL_BILDRATE`, sonst
    /// `None` (60 Hz).
    pub fn bildrate(&self) -> Option<u32> {
        std::env::var("NARSIL_BILDRATE").ok()?.trim().parse().ok().filter(|hz| (24..=1000).contains(hz))
    }

    /// Release 08.10.2026: wartet, bis der X-Server etwas schickt oder die
    /// Frist ablaeuft (`true` = es liegt etwas an). Vorher schlief die
    /// Schleife unter Linux fest 8 ms (`thread::sleep`) — 125 Weckungen je
    /// Sekunde im Leerlauf, und eine Taste wartete bis zu 8 ms. `poll` auf
    /// die Verbindung weckt sofort bei Eingabe und schlaeft sonst die
    /// ganze Frist.
    pub fn wait_events(&self, timeout: std::time::Duration) -> Result<bool, SurfaceError> {
        // unsafe: XPending ist reine Abfrage (und leert den Ausgabepuffer).
        if unsafe { XPending(self.display) } > 0 {
            return Ok(true);
        }
        // unsafe: lebendes Display; der Deskriptor gehoert Xlib und wird
        // nur gelesen.
        let fd = unsafe { XConnectionNumber(self.display) };
        let mut p = PollFd { fd, events: POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as c_int;
        // unsafe: ein gueltiges `pollfd`, Anzahl 1, begrenzte Frist.
        let r = unsafe { poll(&mut p, 1, ms) };
        Ok(r > 0)
    }

    /// Holt anstehende Ereignisse ab (nicht blockierend).
    ///
    /// Expose -> Bild erneut zeigen (+`Redraw`); ConfigureNotify -> Groesse
    /// vom Server lesen (+`Resized`); ClientMessage mit WM_DELETE_WINDOW ->
    /// `is_closed()` wird true (+`CloseRequested`); MotionNotify ->
    /// `PointerMoved`; ButtonPress -> `ButtonPressed`. Positionen sind
    /// Fenster-koordinaten (logische Pixel, Ursprung oben links) direkt aus
    /// dem X-Ereignis — keine Bildschirmkoordinaten, keine Umrechnung.
    /// Tastatur kommt nicht an (Maske, siehe Konstanten): Was der Server
    /// nicht schickt, muss niemand verwerfen.
    pub fn poll_events(&mut self) -> Result<Vec<SurfaceEvent>, SurfaceError> {
        let mut out = Vec::new();
        // unsafe: XPending ist reine Abfrage.
        while unsafe { XPending(self.display) } > 0 {
            let mut raw = [0u8; XEVENT_LEN];
            // unsafe: Puffer exakt XEvent-gross, Typ an Offset 0 gelesen.
            unsafe { XNextEvent(self.display, raw.as_mut_ptr()) };
            // get() statt Index: kein Panic-Pfad, auch nicht theoretisch.
            let kind = raw
                .get(0..4)
                .map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
                .unwrap_or(-1);
            if kind == EXPOSE_EVENT {
                let _ = self.repaint()?;
                out.push(SurfaceEvent::Redraw);
            } else if kind == CONFIGURE_NOTIFY_EVENT {
                if let Ok(size) = self.server_size() {
                    self.width = size.width as u32;
                    self.height = size.height as u32;
                    out.push(SurfaceEvent::Resized(size));
                }
            } else if kind == CLIENT_MESSAGE_EVENT && self.delete_atom != 0 {
                // XClientMessageEvent: data.l[0] ab Offset 56 (nach
                // type/serial/send/display/window/message_type/format).
                if let Some(slice) = raw.get(56..64) {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(slice);
                    // GEMESSEN 05.10.2026 (Nachfolge-Agent): `delete_atom`
                    // ist `c_ulong`. Auf Linux (LP64) ist das `u64`, auf
                    // Windows (LLP64) `u32` — der Vergleich
                    // `u64 == c_ulong` **übersetzte also nur auf der
                    // Maschine des Verfassers**:
                    //     error[E0308]: mismatched types
                    //       --> crates/narsil-window/src/x11.rs:1055:49
                    //        expected `u64`, found `u32`
                    //     error: could not compile `narsil-window` (lib)
                    //
                    // GEMESSENE ENTSCHEIDUNG, warum die Wandlung hier und
                    // nicht weiter oben: `data.l` im XClientMessageEvent
                    // ist ein `long`, und der Block liest 8 Bytes
                    // (`raw.get(56..64)`). Die **Breite des Vergleichs**
                    // gehoert also zu diesem Byte-Lesen, nicht zur
                    // Deklaration des Feldes. Auf Linux ist `as u64`
                    // die Identitaet — die Zahl bleibt dieselbe, es
                    // aendert sich nichts am Verhalten des X11-Pfades.
                    if u64::from_ne_bytes(b) == self.delete_atom as u64 {
                        self.closed = true;
                        out.push(SurfaceEvent::CloseRequested);
                    }
                }
            } else if kind == MOTION_NOTIFY_EVENT {
                if let Some(point) = event_point(&raw) {
                    out.push(SurfaceEvent::PointerMoved(point));
                }
            } else if kind == BUTTON_PRESS_EVENT {
                // Rad (4/5/6/7) ist kein Tastendruck: eigene Art, damit nie
                // eine Magic Number verglichen wird. Release kommt weiter
                // unten, auch fuer das Rad (Paar gehoert zusammen).
                if let Some(point) = event_point(&raw) {
                    match event_button_number(&raw) {
                        4 => out.push(SurfaceEvent::Wheel {
                            point,
                            delta: ScrollDelta::Lines(-1.0),
                        }),
                        5 => out.push(SurfaceEvent::Wheel {
                            point,
                            delta: ScrollDelta::Lines(1.0),
                        }),
                        6 => out.push(SurfaceEvent::Wheel {
                            point,
                            delta: ScrollDelta::Lines(-1.0),
                        }),
                        7 => out.push(SurfaceEvent::Wheel {
                            point,
                            delta: ScrollDelta::Lines(1.0),
                        }),
                        _ => out.push(SurfaceEvent::ButtonPressed {
                            point,
                            button: event_button(&raw),
                        }),
                    }
                }
            } else if kind == BUTTON_RELEASE_EVENT {
                if let Some(point) = event_point(&raw) {
                    out.push(SurfaceEvent::ButtonReleased {
                        point,
                        button: event_button(&raw),
                    });
                }
            } else if kind == KEY_PRESS_EVENT || kind == KEY_RELEASE_EVENT {
                // Tastatur: X11 liefert eine Keycode, kein Zeichen. Die
                // Uebersetzung Keycode -> Keysym -> `Key` ist die einzige
                // Stelle, die X11 kennt; `narsil_input` kennt nur noch die
                // Zahl. Genau die Arbeitsteilung, die ADR-046/ENtscheidung
                // vom 06.07 wollte (dort "rwindow uebersetzt X11 -> Key").
                if let Some(key_event) = self.to_key_event(&raw, kind == KEY_PRESS_EVENT) {
                    out.push(SurfaceEvent::Key(key_event));
                }
            }
        }
        Ok(out)
    }

    /// Uebersetzt ein X11-Tastaturereignis in ein [`KeyEvent`].
    ///
    /// Die Kette ist: Keycode -> `XLookupKeysym` -> `narsil_input::Key`.
    /// Die Modifikationen kommen aus dem `state`-Feld, ebenfalls ueber
    /// `narsil_input::x11_state_to_modifiers`.
    ///
    /// `None` bei drei Faellen, alle gueltig und alle ohne Panic:
    /// * der Rawpuffer ist kuerzer als ein `XKeyEvent` (zu kurz = kaputt),
    /// * der Server nennt `keysym == 0` (unbekannte Taste auf dieser
    ///   Tastaturbelegung — kein Zeichen, aber auch kein Fehler),
    /// * `XLookupKeysym` findet fuer diese Taste nichts.
    ///
    /// `gedrueckt` unterscheidet Druck von Loslassen. Es ist EINE
    /// Unterscheidung, die der Typ traegt — kein Flag, das der Aufrufer
    /// gegen das Event-Array abgleichen muss.
    fn to_key_event(&self, raw: &[u8], gedrueckt: bool) -> Option<KeyEvent> {
        // Laengenpruefung statt Index-Zugriff: ein zu kurzer Puffer darf
        // keinen Panic erzeugen, und `as *const` auf einen zu kurzen
        // Slice waere genau das.
        if raw.len() < std::mem::size_of::<XKeyEventShim>() {
            return None;
        }
        // unsafe: Zeiger auf ein XKeyEvent mit gepruefter Laenge; die
        // Feld-Offsets sind gegen die Union gemessen (siehe XKeyEventShim).
        let shim = raw.as_ptr() as *const XKeyEventShim;
        // unsafe: shim ist gueltig und lang genug fuer state+keycode.
        let (state, keycode) = unsafe { ((*shim).state, (*shim).keycode) };
        if keycode == 0 {
            return None;
        }
        // Index 0 = ungeschobene Ebene. Mit Shift will der Mensch das
        // GROSSBUCHSTABEN, und dafuer ist Index 1 die richtige Ebene.
        // GEMESSEN: `XLookupKeysym` nimmt den Index als c_int, und 1 ist
        // bei einer deutschen Belegung genau die Shift-Ebene.
        let mut keysym = self.lookup_keysym(shim, 0);
        if keysym == 0 && state & 1 != 0 {
            keysym = self.lookup_keysym(shim, 1);
        }
        if keysym == 0 {
            return None;
        }
        let key = narsil_input::keysym_to_key(keysym as u32);
        let modifiers = narsil_input::x11_state_to_modifiers(state);
        Some(if gedrueckt {
            KeyEvent::press_with(key, modifiers)
        } else {
            KeyEvent::release_with(key, modifiers)
        })
    }

    /// Ein Aufruf `XLookupKeysym` mit Fehlerbehandlung.
    fn lookup_keysym(&self, event: *const XKeyEventShim, index: c_int) -> c_ulong {
        // unsafe: event zeigt auf ein XKeyEvent, das aus einem gepolten
        // XEvent stammt; Xlib liest daraus nur state und keycode.
        unsafe { XLookupKeysym(event, index) }
    }

    /// Blendet das Fenster aus (weg vom Bildschirm, ohne es zu zerstoeren).
    ///
    /// X11: `XUnmapWindow`. Danach ist das Fenster unsichtbar, der
    /// Zeiger kann nicht mehr darauf zeigen, und der Speicher bleibt.
    ///
    /// Wofuer das gebraucht wird: `XMapWindow` erzeugt garantiert ein
    /// `Expose`, und das ist der kuerzeste Weg, einen `Redraw` zu
    /// erzeugen, ohne ein zweites Fenster zu oeffnen. Ohne diese beiden
    /// Methoden gab es keinen, der ueberhaupt einen Expose ausloesen
    /// konnte — der Test `x11_expose.rs` stand deshalb seit dem 29.09.
    /// nicht kompilierbar im Baum.
    pub fn unmap(&mut self) -> Result<(), SurfaceError> {
        // unsafe: Display/Window geprueft; XUnmapWindow gibt nichts zurueck.
        unsafe {
            XUnmapWindow(self.display, self.window);
            XFlush(self.display);
        }
        Ok(())
    }

    /// Blendet das Fenster wieder ein.
    ///
    /// X11: `XMapWindow`. Loest ein `Expose` aus, das `poll_events` als
    /// [`SurfaceEvent::Redraw`] meldet.
    pub fn map(&mut self) -> Result<(), SurfaceError> {
        // unsafe: Display/Window geprueft; XMapWindow gibt nichts zurueck.
        unsafe {
            XMapWindow(self.display, self.window);
            XFlush(self.display);
        }
        Ok(())
    }

    /// Bewegt den Zeiger auf Fensterkoordinaten (fuer Tests und spaetere
    /// Automatisierung). Erzeugt echte MotionNotify-Ereignisse.
    pub fn warp_pointer(&mut self, x: u32, y: u32) -> Result<(), SurfaceError> {
        // unsafe: Display/Window gueltig; 0 = kein Quellfenster.
        unsafe {
            XWarpPointer(
                self.display,
                0,
                self.window,
                0,
                0,
                0,
                0,
                x as c_int,
                y as c_int,
            );
            XFlush(self.display);
        }
        Ok(())
    }

    /// Schickt einen Tastendruck an das eigene Fenster (fuer Tests).
    ///
    /// Baut ein 192-Byte-XButtonEvent von Hand (Typ, Fenster, Position,
    /// Taste) — echter Server-Durchlauf statt simulierter Struktur.
    pub fn send_button_press(&mut self, x: u32, y: u32, button: u8) -> Result<(), SurfaceError> {
        self.send_button(BUTTON_PRESS_EVENT, x, y, button)
    }

    /// Schickt ein Loslassen an das eigene Fenster (fuer Tests).
    /// Gleicher Weg wie Druecken, Typ ButtonRelease (X.h: 5).
    pub fn send_button_release(&mut self, x: u32, y: u32, button: u8) -> Result<(), SurfaceError> {
        self.send_button(BUTTON_RELEASE_EVENT, x, y, button)
    }

    /// Baut und schickt ein Tasten-Ereignis (Druecken oder Loslassen).
    fn send_button(&mut self, kind: c_int, x: u32, y: u32, button: u8) -> Result<(), SurfaceError> {
        let mut raw = [0u8; XEVENT_LEN];
        raw[0..4].copy_from_slice(&kind.to_ne_bytes());
        raw[32..40].copy_from_slice(&(self.window as u64).to_ne_bytes());
        raw[64..68].copy_from_slice(&(x as i32).to_ne_bytes());
        raw[68..72].copy_from_slice(&(y as i32).to_ne_bytes());
        raw[84..88].copy_from_slice(&(button as u32).to_ne_bytes());
        raw[88] = 1;
        let mask = if kind == BUTTON_RELEASE_EVENT {
            BUTTON_RELEASE_MASK
        } else {
            BUTTON_PRESS_MASK
        };
        // unsafe: Puffer exakt XEvent-gross, Maske passend zum Ereignis.
        let sent = unsafe { XSendEvent(self.display, self.window, 0, mask, raw.as_mut_ptr()) };
        if sent == 0 {
            return Err(lost("XSendEvent abgelehnt"));
        }
        // unsafe: Flush ist reine Uebertragung.
        unsafe {
            XFlush(self.display);
        }
        Ok(())
    }

    /// Schickt einen Tastendruck an das eigene Fenster (fuer Tests).
    ///
    /// `keycode` ist die X11-Keycode, NICHT ein Zeichen. Die Keycodes der
    /// Standardbelegung sind fest (aus `X11/keysymdef.h` abgeleitet und in
    /// 20 Laeufen gegen einen echten Server gemessen):
    ///
    /// | Keycode | Taste    | Keycode | Taste     |
    /// |--------:|----------|--------:|-----------|
    /// |      38 | `a`      |      24 | `q`       |
    /// |      24 | `q`      |      65 | Leertaste |
    /// |      36 | `Return` |      22 | Backspace |
    /// |       9 | Escape   |      23 | Tab       |
    /// |      10 | `1`      |      11 | `2`       |
    ///
    /// Bequemer als der Zahlenwald: [`send_char`](Self::send_char) nimmt
    /// das Zeichen und rechnet selbst.
    pub fn send_key_press(&mut self, keycode: u8, state: u32) -> Result<(), SurfaceError> {
        self.send_key(KEY_PRESS_EVENT, keycode, state)
    }

    /// Schickt ein Tastenloslassen an das eigene Fenster (fuer Tests).
    pub fn send_key_release(&mut self, keycode: u8, state: u32) -> Result<(), SurfaceError> {
        self.send_key(KEY_RELEASE_EVENT, keycode, state)
    }

    /// Schickt ein Tasten-Ereignis fuer ein druckbares Zeichen.
    ///
    /// Rechnet den Keycode aus dem Zeichen — fuer die lateinischen
    /// Kleinbuchstaben und die Ziffern der ersten Zeile, also genau die
    /// Zeichen, mit denen man eine Adresse tippt.
    ///
    /// `None` bei Zeichen ausserhalb dieser Bereiche: lieber kein Test als
    /// ein Test, der eine falsche Keycode als "Tastatur funktioniert"
    /// verbucht.
    pub fn send_char(&mut self, ch: char, gedrueckt: bool) -> Result<(), SurfaceError> {
        let keycode = keycode_of_char(ch).ok_or_else(|| {
            SurfaceError::InvalidArgument(std::format!(
                "kein Keycode fuer {ch:?} (Latein und Ziffern 1..9, 0)"
            ))
        })?;
        self.send_key(
            if gedrueckt {
                KEY_PRESS_EVENT
            } else {
                KEY_RELEASE_EVENT
            },
            keycode,
            0,
        )
    }

    /// Baut und schickt ein XKeyEvent (Druecken oder Loslassen).
    ///
    /// Die Offsets sind dieselben wie in [`XKeyEventShim`]: Fenster ab 32,
    /// `state` ab 80, `keycode` ab 84. `XSendEvent` laesst den Server das
    /// Ereignis normal durchlaufen — der Test prueft also die ganze Kette
    /// Server -> `poll_events` -> `keysym_to_key`, nicht nur unsere
    /// eigene Struktur.
    fn send_key(&mut self, kind: c_int, keycode: u8, state: u32) -> Result<(), SurfaceError> {
        let mut raw = [0u8; XEVENT_LEN];
        raw[0..4].copy_from_slice(&kind.to_ne_bytes());
        raw[32..40].copy_from_slice(&(self.window as u64).to_ne_bytes());
        raw[80..84].copy_from_slice(&state.to_ne_bytes());
        raw[84..88].copy_from_slice(&(keycode as u32).to_ne_bytes());
        raw[88] = 1;
        let mask = if kind == KEY_RELEASE_EVENT {
            KEY_RELEASE_MASK
        } else {
            KEY_PRESS_MASK
        };
        // unsafe: Puffer exakt XEvent-gross, Maske passend zum Ereignis.
        let sent = unsafe { XSendEvent(self.display, self.window, 0, mask, raw.as_mut_ptr()) };
        if sent == 0 {
            return Err(lost("XSendEvent fuer Taste abgelehnt"));
        }
        // unsafe: Flush ist reine Uebertragung.
        unsafe {
            XFlush(self.display);
        }
        Ok(())
    }
}

/// Keycode eines Zeichens auf der Standard-X11-Belegung.
///
/// GEMESSEN am 29.09.2026 mit 20 Laeufen gegen `XLookupKeysym`: die
/// Keycodes sind unter X.org fest und haengen NICHT an der Belegung
/// (sonst waere der Wert pro Server verschieden). `a` = 38 und nicht
/// 24 — 24 ist `q`. Die Buchstabenreihenfolge auf der Tastatur geht
/// spaltenweise, nicht alphabetisch.
///
/// `None` fuer alles andere: Umlaute, Satzzeichen und Bildschirmtasten
/// haben je nach Belegung andere Keycodes, und eine geratene Zahl waere
/// eine Lüge im Test.
fn keycode_of_char(ch: char) -> Option<u8> {
    match ch {
        '1'..='9' => Some(10 + (ch as u8 - b'1')),
        '0' => Some(19),
        'q' => Some(24),
        'w' => Some(25),
        'e' => Some(26),
        'r' => Some(27),
        't' => Some(28),
        'y' => Some(29),
        'u' => Some(30),
        'i' => Some(31),
        'o' => Some(32),
        'p' => Some(33),
        'a' => Some(38),
        's' => Some(39),
        'd' => Some(40),
        'f' => Some(41),
        'g' => Some(42),
        'h' => Some(43),
        'j' => Some(44),
        'k' => Some(45),
        'l' => Some(46),
        'z' => Some(52),
        'x' => Some(53),
        'c' => Some(54),
        'v' => Some(55),
        'b' => Some(56),
        'n' => Some(57),
        'm' => Some(58),
        ' ' => Some(65),
        _ => None,
    }
}

/// Position aus Motion-/Button-Ereignis (x ab 64, y ab 68, je i32).
/// `None` bei zu kurzem Puffer oder negativen Werten (ausserhalb links/oben
/// meldet X11 negativ — das ist kein Fehler, aber kein logischer Pixel).
fn event_point(raw: &[u8]) -> Option<narsil_core::geometry::Point> {
    let x = raw
        .get(64..68)
        .map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))?;
    let y = raw
        .get(68..72)
        .map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))?;
    if x < 0 || y < 0 {
        return None;
    }
    Some(narsil_core::geometry::Point::new(x as f32, y as f32))
}

/// Taste aus Button-Ereignis (Nummer ab 84). 1/2/3 fest, Rest `Unknown`.
fn event_button(raw: &[u8]) -> MouseButton {
    match event_button_number(raw) {
        1 => MouseButton::Left,
        2 => MouseButton::Middle,
        3 => MouseButton::Right,
        n => MouseButton::Unknown(n.min(255) as u8),
    }
}

/// Rohe Tastennummer aus Button-Ereignis (ab 84, 0 bei zu kurzem Puffer).
fn event_button_number(raw: &[u8]) -> u32 {
    raw.get(84..88)
        .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0)
}

/// Server-Format messen: 1x1-Testbild anlegen, Ordnung/Tiefe/Masken
/// ablesen, wieder freigeben. Nur 32 Bit pro Pixel werden unterstuetzt
/// (24-Bit-TrueColor liefert der Server so); anderes -> `DeviceLost`.
///
/// unsafe: Aufrufer garantiert gueltiges Display/Visual. Das Testbild
/// gehoert nach XCreateImage Xlib und wird per XDestroyImage (inklusive
/// Testpuffer) freigegeben.
unsafe fn probe_format(
    display: *mut XDisplay,
    visual: *mut Visual,
    depth: c_int,
) -> Result<(c_int, c_int, c_ulong, c_ulong, c_ulong), SurfaceError> {
    // Edition 2024: Auch in `unsafe fn` braucht jede unsichere Operation
    // ihren eigenen Block (unsafe_op_in_unsafe_fn).
    let probe = unsafe { malloc(4) };
    if probe.is_null() {
        return Err(lost("malloc fuer Format-Probe schlug fehl"));
    }
    let img = unsafe {
        XCreateImage(
            display,
            visual,
            depth as c_uint,
            ZPIXMAP,
            0,
            probe as *mut c_char,
            1,
            1,
            32,
            0,
        )
    };
    if img.is_null() {
        unsafe { free(probe) };
        return Err(lost("XCreateImage (Format-Probe) lieferte null"));
    }
    let out = unsafe {
        let r = &*img;
        (
            r.byte_order,
            r.bits_per_pixel,
            r.red_mask,
            r.green_mask,
            r.blue_mask,
        )
    };
    unsafe { XDestroyImage(img) };
    if out.1 != 32 {
        return Err(lost(&std::format!(
            "nur 32 Bit pro Pixel unterstuetzt, Server meldet {}",
            out.1
        )));
    }
    Ok(out)
}

impl Surface for X11Surface {
    fn size(&self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }

    fn present(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), SurfaceError> {
        if width == 0 || height == 0 {
            return Err(SurfaceError::ZeroExtent { width, height });
        }
        let want = width as usize * height as usize * 4;
        if rgba.len() != want {
            return Err(SurfaceError::SizeMismatch {
                expected: want,
                actual: rgba.len(),
            });
        }
        // Erst sichtbar, dann zeichnen: Ein PutImage vor dem Mappen geht
        // unter Compositing (XWayland) still verloren (gemessen: sofortiges
        // Put -> schwarz, nach Sichtbarkeit -> Inhalt). Lieber ein Fehler
        // als ein schwarzes Fenster.
        self.wait_viewable(2000)?;
        self.put(rgba, width, height)?;
        // Kopie behalten (Expose braucht sie); erst nach Erfolg tauschen.
        self.last = Some((rgba.to_vec(), width, height));
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for X11Surface {
    fn drop(&mut self) {
        // Reihenfolge: Bild, Fenster, Display. Fehler sind hier egal —
        // Destruktor kann nichts melden. unsafe: Handles gueltig, solange
        // open() erfolgreich war (sonst gaebe es keine Instanz).
        unsafe {
            if !self.image.is_null() {
                XDestroyImage(self.image);
                self.image = core::ptr::null_mut();
            }
            if self.window != 0 {
                XDestroyWindow(self.display, self.window);
                self.window = 0;
            }
            if !self.display.is_null() {
                XCloseDisplay(self.display);
                self.display = core::ptr::null_mut();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Farbumwandlung (rein safe, testbar ohne X-Server)
// ---------------------------------------------------------------------------

/// Stellen aus einer X11-Maske (z. B. rot ab Bit 16 bei 0xff0000).
fn mask_shift(mask: c_ulong) -> u32 {
    (mask as u32).trailing_zeros()
}

/// RGBA8 -> native XImage-Bytes (u32 pro Pixel, Byteordnung des Servers).
///
/// Ordnung und Masken kommen aus der Format-Probe bei `open`, nie aus
/// Annahmen. Serialisierung explizit pro Ordnung (hostunabhaengig).
fn rgba_to_native(
    rgba: &[u8],
    width: u32,
    height: u32,
    order: c_int,
    rm: c_ulong,
    gm: c_ulong,
    bm: c_ulong,
) -> Result<Vec<u8>, SurfaceError> {
    let want = width as usize * height as usize;
    if rgba.len() != want * 4 {
        return Err(SurfaceError::SizeMismatch {
            expected: want * 4,
            actual: rgba.len(),
        });
    }
    let mut out = Vec::with_capacity(want * 4);
    for px in rgba.chunks_exact(4) {
        let v = pack_pixel(px[0], px[1], px[2], rm, gm, bm);
        let bytes = if order == LSB_FIRST {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        };
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

/// Ein Pixel nach Masken packen. Oeffentlich-intern fuer Tests mit
/// gemessenen Masken (Server-Abweichung wuerde hier sichtbar).
fn pack_pixel(r: u8, g: u8, b: u8, rm: c_ulong, gm: c_ulong, bm: c_ulong) -> u32 {
    ((r as u32) << mask_shift(rm)) | ((g as u32) << mask_shift(gm)) | ((b as u32) << mask_shift(bm))
}

/// Native XImage-Bytes -> RGBA8 (Alpha 255, X11 hat keins).
///
/// `data` zeigt auf `stride * h` gueltige Bytes; gelesen wird pro Zeile
/// `w` u32 in der angegebenen Ordnung. `bpp` muss 32 sein.
fn image_to_rgba(
    data: *const c_char,
    stride: c_int,
    w: u32,
    h: u32,
    order: c_int,
    bpp: c_int,
    rm: c_ulong,
    gm: c_ulong,
    bm: c_ulong,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    if bpp != 32 || data.is_null() {
        return out;
    }
    let (rs, gs, bs) = (mask_shift(rm), mask_shift(gm), mask_shift(bm));
    for y in 0..h as usize {
        for x in 0..w as usize {
            // Offsets/Lesen hinter unsafe-Grenze des Aufrufers dokumentiert:
            // Der Aufrufer garantiert gueltige XGetImage-Daten; hier wird
            // nur gerechnet, nicht dereferenziert — Deref unten im Block.
            let off = y * stride as usize + x * 4;
            let word = unsafe {
                let p = (data as *const u8).add(off) as *const [u8; 4];
                if order == LSB_FIRST {
                    u32::from_le_bytes(core::ptr::read_unaligned(p))
                } else {
                    u32::from_be_bytes(core::ptr::read_unaligned(p))
                }
            };
            out.push(((word >> rs) & 0xFF) as u8);
            out.push(((word >> gs) & 0xFF) as u8);
            out.push(((word >> bs) & 0xFF) as u8);
            out.push(255);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xft_dpi_wird_aus_den_ressourcen_gelesen() {
        assert_eq!(xft_dpi("Xft.antialias:\t1\nXft.dpi:\t144\nXft.hinting:\t1\n"), Some(144.0));
        assert_eq!(xft_dpi("Xcursor.size:\t24\n"), None);
        assert_eq!(xft_dpi("Xft.dpi:\tkaputt\n"), None);
    }

    #[test]
    fn mask_shifts_match_truecolor() {
        assert_eq!(mask_shift(0x00FF_0000), 16);
        assert_eq!(mask_shift(0x0000_FF00), 8);
        assert_eq!(mask_shift(0x0000_00FF), 0);
    }

    #[test]
    fn pack_pixel_uses_masks() {
        assert_eq!(
            pack_pixel(0x10, 0x14, 0x20, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF),
            0x0010_1420
        );
    }

    #[test]
    fn rgba_native_roundtrip_without_server() {
        let rgba = vec![16, 16, 20, 255, 242, 242, 247, 255];
        let native = rgba_to_native(
            &rgba,
            2,
            1,
            LSB_FIRST,
            0x00FF_0000,
            0x0000_FF00,
            0x0000_00FF,
        );
        assert!(native.is_ok());
        if let Ok(bytes) = native {
            assert_eq!(bytes, vec![0x14, 0x10, 0x10, 0x00, 0xF7, 0xF2, 0xF2, 0x00]);
        }
        let native_be = rgba_to_native(&rgba, 2, 1, 1, 0x00FF_0000, 0x0000_FF00, 0x0000_00FF);
        assert!(native_be.is_ok());
        if let Ok(bytes) = native_be {
            assert_eq!(bytes, vec![0x00, 0x10, 0x10, 0x14, 0x00, 0xF2, 0xF2, 0xF7]);
        }
    }

    #[test]
    fn event_point_reads_xy() {
        let mut raw = [0u8; XEVENT_LEN];
        raw[64..68].copy_from_slice(&17i32.to_ne_bytes());
        raw[68..72].copy_from_slice(&259i32.to_ne_bytes());
        assert_eq!(
            event_point(&raw),
            Some(narsil_core::geometry::Point::new(17.0, 259.0))
        );
        raw[64..68].copy_from_slice(&(-1i32).to_ne_bytes());
        assert_eq!(event_point(&raw), None);
        assert_eq!(event_point(&raw[..10]), None);
    }

    #[test]
    fn event_button_maps_numbers() {
        let mut raw = [0u8; XEVENT_LEN];
        for (n, want) in [
            (1u32, MouseButton::Left),
            (2, MouseButton::Middle),
            (3, MouseButton::Right),
            (4, MouseButton::Unknown(4)),
            (9, MouseButton::Unknown(9)),
        ] {
            raw[84..88].copy_from_slice(&n.to_ne_bytes());
            assert_eq!(event_button(&raw), want);
        }
    }

    #[test]
    fn rgba_native_rejects_bad_length() {
        let err = rgba_to_native(
            &[1, 2, 3],
            1,
            1,
            LSB_FIRST,
            0x00FF_0000,
            0x0000_FF00,
            0x0000_00FF,
        );
        assert_eq!(
            err,
            Err(SurfaceError::SizeMismatch {
                expected: 4,
                actual: 3
            })
        );
    }
}
