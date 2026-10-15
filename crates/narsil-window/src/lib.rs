//! Praesentations-Abstraktion: Surface-Trait, Viewport-Policy, Resize.
//!
//! Diese Schicht bedient spaeter das Fenster. Aktuell laeuft der Browser
//! headless (ADR-006): es gibt noch kein Fenster, und fuer den MVP wird
//! keines gebraucht. Der Trait in [`surface`] ist die Schnittstelle, die
//! ein spaeteres echtes Fenster implementiert.
//!
//! Bewusst nimmt [`Surface::present`](surface::Surface::present) rohe
//! `&[u8]`-Pixeldaten entgegen statt eines `narsil-raster`-Typs: so haengt
//! diese Schicht nicht am Software-Rasterizer und kann spaeter auch
//! GPU-Puffer bekommen.
//!
//! ADR-020 (X11): `unsafe` ist in diesem Crate per `#![deny]` verboten —
//! einzige Ausnahme ist [`x11`] (X11-Fenster hinter dem nicht-default
//! Feature `x11`), wo es per `#[allow]` eng begrenzt ist. Headless bleibt
//! Default; das Fenster ist ein zusaetzlicher Weg zum selben Ergebnis.
#![deny(unsafe_code)]

pub mod event;
pub mod surface;
pub mod viewport;

#[cfg(all(target_os = "linux", any(feature = "x11", feature = "desktop")))]
#[allow(unsafe_code)]
pub mod x11;
#[cfg(all(target_os = "linux", any(feature = "x11", feature = "desktop")))]
pub use x11::X11Surface;

#[cfg(all(windows, feature = "desktop"))]
#[allow(unsafe_code)]
pub mod win32;
#[cfg(all(windows, feature = "desktop"))]
pub use win32::Win32Surface as NativeSurface;
#[cfg(all(target_os = "linux", any(feature = "x11", feature = "desktop")))]
pub use x11::X11Surface as NativeSurface;
#[cfg(all(feature = "desktop", not(any(windows, target_os = "linux"))))]
compile_error!("desktop unterstuetzt derzeit Linux/X11 und Windows/Win32");

// GEMESSEN 05.10.2026 (Nachfolge-Agent): Das Feature `x11` ist eine
// **Linux**-Eigenschaft, und deshalb scheitert es auf anderen Plattformen
// erst beim **Linken**, mit einer Meldung, die nichts erklaert:
//     = note: LINK : fatal error LNK1181:
//       Eingabedatei "X11.lib" kann nicht geoeffnet werden.
//
// GEMESSENE ENTSCHEIDUNG, warum hier ein `compile_error!` und keine
// Stille: `#[cfg(not(target_os = "linux"))]` auf `mod x11` wuerde das
// Modul verschwinden lassen — und `narsil`/`narsil-probe`, die es
// benutzen, bekamen dann `unresolved import X11Surface`. Zwei
// Werkzeuge, die **beweisen** sollen, dass etwas geht, wuerden dann so
// tun, als ginge es, ohne dass jemand etwas sieht. Ein lauter Fehler
// ist hier die ehrliche Form: X11 gibt es unter Windows nicht, also
// sagt der Compiler das jetzt **vor** dem Linken und **mit** dem Grund.
#[cfg(all(feature = "x11", not(target_os = "linux")))]
compile_error!(
    "narsil-window/feature \"x11\" ist Linux-only: es bindet libX11 \
     (#[link(name = \"X11\")]). Unter diesem Ziel gibt es keine \
     X11-Bibliothek — GEMESSEN: LINK LNK1181 \"X11.lib\" nicht \
     gefunden. Ohne Fenster reicht `cargo build -p narsil-window` ohne \
     das Feature; `narsil` und `narsil-probe` bauen hier nicht, weil sie \
     ein echtes Fenster brauchen."
);

pub use event::{MouseButton, RedrawScheduler, ScrollDelta, SurfaceEvent, is_click_pair};
pub use surface::{MemorySurface, Surface, SurfaceError};
pub use viewport::ViewportPolicy;

#[cfg(windows)]
#[allow(unsafe_code)]
pub mod clipboard_win32;
// Release 10.10.2026: der Datei-Oeffnen-Dialog (`<input type="file">`).
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod dateidialog_win32;
// Release 10.10.2026: der Name der Windows-Zeitzone (lokale Zeit im JS-Motor).
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod zeitzone_win32;
