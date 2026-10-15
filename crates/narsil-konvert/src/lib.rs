//! KONVERT — Enkoder und Pipeline.
//!
//! Der Browser soll „Konvertiert herunterladen" anbieten (Nutzerwunsch
//! ANFORDERUNGEN.txt vom 02.10.2026). Die DEKODER dafuer sind schon im
//! Projekt (`narsil-image`, `narsil-audio`, `narsil-media`). Was fehlt, ist das
//! andere Ende: Datei lesbar machen UND Datei schreiben, ohne externes
//! Programm. Diese Crate baut die ENKODER und die Pipeline drumherum.
//!
//! Nur `std`, kein `unsafe`, kein Fremd-Crate (ADR-030).

// # EINE AUSNAHME, und sie ist **gemessen**
//
// Und GEMESSEN am 04.10.2026 (`clippy --all-targets`): **18** Waertungen in
// `filterbank.rs`, `mdct.rs` und `quant.rs` sind „float has excessive precision“.
//
// Und **die** Sache ist **nicht** „der Compiler meckert zu oft“, sondern die
// Gegenteilige: **genau diese 18 Stellen sind Gangkonstanten**, die **aus der
// Spezifikation abgeschrieben** sind (MP3-Filter, MDCT-Baender, Quantisierung).
//
// Und **abschneiden** waere **kein** Kosmetikum, sondern eine **Wertaenderung**:
// `0.98480775301220805936` -> `0.984807753` **aendert** das Ergebnis jedes Bildes
// um **ein** Bit — und damit **jede** gemessene PSNR in diesem Projekt.
//
// > **Ein Transkript einer Spezifikation wird *nicht* gerundet.** Und wer es
// > rundet, hat die Zahl nicht *kuerzer* gemacht, sondern **anders** — und merkt
// > es erst, wenn eine Messung um **0,1 dB** wandert.
//
// Und **die** Regel dahinter, und sie ist **billiger** als jede Ausnahme:
//
// > **Eine Warnung, deren Behebung eine Zahl **aendert**, ist **keine** Aufräum-
// > arbeit — sie ist eine **Messentscheidung**.** Und die wird **gemessen**, nicht
// > aus Bequemlichkeit getroffen.
//!
//! # Aufbau
//!
//! | Modul | Aufgabe |
//! |-------|---------|
//! | [`grenzen`] | alle harten Obergrenzen an EINER Stelle (M3/M4) |
//! | [`fehler`]  | [`Zielformat`] und [`KonvertFehler`] — Fehler sind Werte |
//! | [`huffman`] | praefixCodes mit Laengengrenze (JPEG, GIF, VP8L) |
//! | [`farbe`]   | RGB -> YCbCr, Median-Cut, Dithering |
//!
//! K1 (Bild): [`png`], [`jpeg`], [`bmp`], [`gif`], [`webp`], [`ico`].
//! K2 (Ton) und K3 (Video) kommen darueber; K4 baut die Pipeline in [`lib::konvertiere`].

// Der Linter der Crate: `unsafe_code = "forbid"` kommt aus dem Workspace.
// `missing_docs` NICHT — die Crate hat viele interne Helfer ohne Doku, und
// ein `#[allow]` hier waere ehrlicher als 400 Doc-Kommentare zu Pflichttext.
#![allow(clippy::too_many_arguments)]
#![allow(clippy::excessive_precision)] // GEMESSEN: 18 Gangkonstanten aus der
// MP3-Spezifikation. Abschneiden waere eine **Wertaenderung** und damit eine
// Aenderung **jeder** gemessenen PSNR — und ein Transkript wird nicht gerundet.

pub mod granulat;
pub mod grenzen;

pub mod auftrag;
pub mod baender;
pub mod bits;
pub mod bmp;
pub mod count1;
pub mod druck;
pub mod farbe;
pub mod fehler;
pub mod filterbank;
pub mod flac;
pub mod gif;
pub mod huffbig;
pub mod huffman;
pub mod ico;
pub mod jpeg;
pub mod lpc;
pub mod md5;
pub mod mdct;
pub mod mp3;
pub mod mp3ton;
pub mod ogg;
pub mod pdf;
pub mod pipeline;
pub mod png;
pub mod quant;
pub mod schnittstelle;
pub mod subset;
pub mod ton;
pub mod video;
pub mod wav;
pub mod webm;
pub mod webp;
pub mod ziele;

pub use fehler::{KonvertFehler, Zielformat};

/// Die Grenzen und Pruefungen aus [`grenzen`] — oeffentlich, damit die
/// Pipeline (K4) dieselben Pruefungen benutzt statt eigene zu bauen.
pub use grenzen::{MAX_KANTE, pruefe_bild, pruefe_laenge, pruefe_masse};
pub use ton::Pcm;

/// Der Pixel-Typ des Projekts (RGBA8, dicht gepackt).
///
/// Wiederverwendet statt nachgebaut: zwei `Bild`-Typen im selben Prozess
/// waeren eine Fehlerquelle ohne Nutzen — die ganze Kette
/// `narsil-image` -> `narsil-konvert` -> `narsil-download` soll einen Typ
/// benutzen.
pub use narsil_image::Bild;

/// Der Fehlertyp der Bilddekoder — wenn eine Eingabe nicht lesbar ist,
/// nennen wir DIESEN Fehler, nicht einen eigenen „Bild kaputt"-Wert.
pub use narsil_image::BildFehler;

/// Die Formate, die der Bild-Dekoder erkennt.
pub use narsil_image::Bildformat;
