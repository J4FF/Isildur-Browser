//! Debug-Hilfe: den Baum zu einer Eingabe-Datei oder -Zeile dumpen.
//!
//! Aufruf (nur ueber bau.sh, kein direktes cargo):
//!   werkzeuge/bau.sh narsil-fmt-html run --bin debug_trace -- '<p><b>'
//!   werkzeuge/bau.sh narsil-fmt-html run --bin debug_trace -- pfad.txt
//!
//! Ohne Argument wird die frueher hart verdrahtete Eingabe
//! `<template><template><table>` verwendet — der Fall, an dem die
//! "leere <body>"-Frage hing.

use narsil_fmt_html::dom;
use narsil_fmt_html::treebuilder;

fn main() {
    let arg = std::env::args().nth(1);
    let html = match arg {
        Some(p) if std::path::Path::new(&p).is_file() => {
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p} nicht lesbar: {e}"))
        }
        Some(s) => s,
        None => "<template><template><table>".to_string(),
    };
    match treebuilder::parse_dokument_mit_scripting(&html, false) {
        Ok((dok, w)) => println!("{}", dom::dump(&dok, w)),
        Err(f) => println!("!{f:?}"),
    }
}
