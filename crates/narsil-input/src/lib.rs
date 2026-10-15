//! narsil-input — Tastatureingabe als eigener Typ.
//!
//! Diese Crate ist die SPRACHE der Eingabe und kennt kein X11, kein
//! Wayland, kein Terminal. `narsil-window` uebersetzt X11/Wayland-Keysyms
//! in [`Key`]; `narsil-chrome` reagiert darauf.
//!
//! ZUSTAENDIGKEIT: kritischer Pfad Punkt 5 — ohne Tastatur keine
//! Adresszeile, ohne Adresszeile kein Browser.
//!
//! REGELN:
//! * Kein `unsafe`.
//! * Keine Abhaengigkeit ausser `std`.
//! * Jede Taste ist ein eigener Variant, kein Magic-Number-Vergleich.
//! * `to_char()` gibt `Some('a')` fuer druckbare Zeichen, `None` fuer
//!   Steuerungstasten — die Unterscheidung liegt im Typ, nicht im Aufrufer.

/// Eine Taste auf der Tastatur.
///
/// Druckbare Zeichen sind `Key::Char('a')`. Steuerungstasten haben eigene
/// Variants, damit `match` ohne Magic Numbers funktioniert. Unbekannte
/// Tasten sind `Key::Unknown(u32)` mit dem rohen Keysym — kein stilles
/// Verschlucken, aber auch kein Absturz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Ein druckbares Zeichen (Buchstabe, Ziffer, Satzzeichen).
    Char(char),

    // --- Steuerungstasten ---
    /// Eingabetaste.
    Enter,
    /// Tabulator.
    Tab,
    /// Ruecktaste (ein Zeichen loeschen).
    Backspace,
    /// Entfernen-Taste (Zeichen nach dem Cursor).
    Delete,
    /// Escape.
    Escape,
    /// Leertaste — eigener Variant statt `Char(' ')`, weil sie in der
    /// Adressleiste eine Sonderbedeutung haben kann (Autocomplete).
    Space,

    // --- Navigation ---
    /// Pfeil links.
    Left,
    /// Pfeil rechts.
    Right,
    /// Pfeil hoch.
    Up,
    /// Pfeil runter.
    Down,
    /// Pos1 / Home.
    Home,
    /// Ende / End.
    End,
    /// Bild hoch / Page Up.
    PageUp,
    /// Bild runter / Page Down.
    PageDown,

    // --- Modifier (als Taste, nicht als Zustand) ---
    /// Shift (links oder rechts).
    Shift,
    /// Strg / Control.
    Control,
    /// Alt / Meta.
    Alt,

    // --- Funktionstasten ---
    /// F1 bis F12.
    F(u8),

    /// Unbekannte Taste mit rohem Keysym. Kein stilles Verschlucken:
    /// der Aufrufer sieht, dass etwas kam, und kann entscheiden.
    Unknown(u32),
}

impl Key {
    /// Druckbares Zeichen, wenn die Taste eines ist.
    ///
    /// `Space` gibt `Some(' ')`, Steuerungstasten geben `None`.
    /// Die Unterscheidung liegt im Typ: wer ein Zeichen will, fragt
    /// `to_char()`; wer eine Aktion will, matched auf den Variant.
    pub fn to_char(self) -> Option<char> {
        match self {
            Key::Char(c) => Some(c),
            Key::Space => Some(' '),
            Key::Enter => Some('\n'),
            Key::Tab => Some('\t'),
            _ => None,
        }
    }

    /// Ob die Taste druckbar ist (erzeugt ein sichtbares Zeichen).
    pub fn is_printable(self) -> bool {
        matches!(self, Key::Char(_) | Key::Space)
    }

    /// Ob die Taste eine Navigationstaste ist (Pfeile, Home, End, etc.).
    pub fn is_navigation(self) -> bool {
        matches!(
            self,
            Key::Left
                | Key::Right
                | Key::Up
                | Key::Down
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
        )
    }

    /// Ob die Taste ein Modifier ist.
    pub fn is_modifier(self) -> bool {
        matches!(self, Key::Shift | Key::Control | Key::Alt)
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{c}"),
            Key::Enter => write!(f, "Enter"),
            Key::Tab => write!(f, "Tab"),
            Key::Backspace => write!(f, "Backspace"),
            Key::Delete => write!(f, "Delete"),
            Key::Escape => write!(f, "Escape"),
            Key::Space => write!(f, "Space"),
            Key::Left => write!(f, "Left"),
            Key::Right => write!(f, "Right"),
            Key::Up => write!(f, "Up"),
            Key::Down => write!(f, "Down"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::PageUp => write!(f, "PageUp"),
            Key::PageDown => write!(f, "PageDown"),
            Key::Shift => write!(f, "Shift"),
            Key::Control => write!(f, "Control"),
            Key::Alt => write!(f, "Alt"),
            Key::F(n) => write!(f, "F{n}"),
            Key::Unknown(code) => write!(f, "Unknown({code:#x})"),
        }
    }
}

/// Modifier-Zustand: welche Modifiertasten gerade gedrueckt sind.
///
/// KEIN Bitfeld, sondern drei benannte Felder — damit `match` und
/// Abfragen ohne Bitmasken funktionieren.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift ist gedrueckt.
    pub shift: bool,
    /// Strg / Control ist gedrueckt.
    pub ctrl: bool,
    /// Alt / Meta ist gedrueckt.
    pub alt: bool,
}

impl Modifiers {
    /// Kein Modifier gedrueckt.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };

    /// Ob irgendein Modifier gedrueckt ist.
    pub fn any(self) -> bool {
        self.shift || self.ctrl || self.alt
    }

    /// Ob genau Ctrl gedrueckt ist (ohne Shift und Alt).
    pub fn only_ctrl(self) -> bool {
        self.ctrl && !self.shift && !self.alt
    }

    /// Ob genau Shift gedrueckt ist (ohne Ctrl und Alt).
    pub fn only_shift(self) -> bool {
        self.shift && !self.ctrl && !self.alt
    }

    /// Ob genau Alt gedrueckt ist (ohne Shift und Ctrl).
    ///
    /// added 02.10.2026, UI4. DIE DREI `only_*` SIND EINE FAMILIE, und sie
    /// ist nur dann vollstaendig, wenn auch Alt dazugehoert: ein Browser
    /// braucht `Alt+Links` und `Alt+Rechts` (UI4), und ohne diese Methode
    /// gaebe es dafuer nur zwei Wege — ein neues `Modifiers`-Feld (es gibt
    /// keins mehr) oder ein Vergleich von Hand an der Aufrufstelle. Beides
    /// waere eine zweite Wahrheit ueber dieselbe Frage.
    ///
    /// WARUM `!self.shift`: `Alt` und `Shift` kommen auf einer Tastatur
    /// gleichzeitig nur als `AltGr` daher, und `AltGr` liefert je nach
    /// Tastaturlayout `{` oder `\\`. Ein `Alt+Shift+pfeil` ist deshalb
    /// **kein** Navigationsbefehl, sondern ein Zeichen, und ein Browser,
    /// der daraus zurueckblättert, verschluckt Zeichen.
    pub fn only_alt(self) -> bool {
        self.alt && !self.shift && !self.ctrl
    }
}

impl std::fmt::Display for Modifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        if parts.is_empty() {
            write!(f, "(keine)")
        } else {
            write!(f, "{}", parts.join("+"))
        }
    }
}

/// Ein Tastaturereignis: Taste plus Modifier-Zustand.
///
/// Absichtlich `Copy`: ein Ereignis ist klein (Taste + 3 Bools) und wird
/// nie veraendert — es wird gelesen und verglichen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// Welche Taste.
    pub key: Key,
    /// Welche Modifier gerade gedrueckt sind.
    pub modifiers: Modifiers,
    /// Ob die Taste gedrueckt (true) oder losgelassen (false) wurde.
    pub pressed: bool,
}

impl KeyEvent {
    /// Neues Ereignis: Taste gedrueckt, ohne Modifier.
    pub fn press(key: Key) -> Self {
        KeyEvent {
            key,
            modifiers: Modifiers::NONE,
            pressed: true,
        }
    }

    /// Neues Ereignis: Taste losgelassen, ohne Modifier.
    pub fn release(key: Key) -> Self {
        KeyEvent {
            key,
            modifiers: Modifiers::NONE,
            pressed: false,
        }
    }

    /// Neues Ereignis: Taste gedrueckt, mit Modifiern.
    pub fn press_with(key: Key, modifiers: Modifiers) -> Self {
        KeyEvent {
            key,
            modifiers,
            pressed: true,
        }
    }

    /// Neues Ereignis: Taste losgelassen, mit Modifiern.
    ///
    /// SPALTENZURUECK 29.09.2026: es gab `press_with`, aber kein
    /// `release_with`. Das ist keine symmetrische Auslassung, sondern eine
    /// Luecke: `narsil-window` muss beim Loslassen KNOW, ob Shift gedrueckt
    /// war, und konnte es nicht. Ein Loslassen ohne Modifier-Zustand ist
    /// eine erfundene Angabe.
    pub fn release_with(key: Key, modifiers: Modifiers) -> Self {
        KeyEvent {
            key,
            modifiers,
            pressed: false,
        }
    }

    /// Ob das Ereignis Ctrl+C ist (Abbrechen).
    pub fn is_ctrl_c(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('c')
    }

    /// Ob das Ereignis Ctrl+Q ist (Beenden).
    pub fn is_ctrl_q(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('q')
    }

    /// Ob das Ereignis Ctrl+V ist (Einfuegen).
    pub fn is_ctrl_v(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('v')
    }

    /// Ob das Ereignis Ctrl+A ist (Alles auswaehlen).
    pub fn is_ctrl_a(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('a')
    }

    /// Ob das Ereignis Ctrl+L ist (Adressleiste fokussieren).
    pub fn is_ctrl_l(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('l')
    }

    /// W7C A7: Welcher Zoom-Befehl? `Some(+1)`, `Some(-1)`, `Some(0)`.
    ///
    /// GEMESSEN, warum `Some(i32)` und nicht drei `bool`: die drei
    /// Befehle sind EIN Feld, und wer sie in drei Methoden schreibt,
    /// schreibt die Erkennung dreimal. Die Tasten: `Strg++` (auch
    /// `Strg+=`, wie auf einer Tastatur ohne Nummernblock), `Strg+-` und
    /// `Strg+0`.
    pub fn zoom_befehl(self) -> Option<i32> {
        if !self.pressed || !self.modifiers.ctrl || self.modifiers.alt {
            return None;
        }
        // GEMESSENER FEHLER (ein Test rot): `only_ctrl()` verlangt „nur
        // Strg" — aber `+` kommt auf einer Tastatur ohne Nummernblock als
        // `Shift+=`. `Strg+Shift+=` war damit kein Zoom, und genau so
        // drueckt man es auf einem Laptop. Also: Shift ist erlaubt, und
        // `_` (Shift+-) zaehlt genauso wie `-`.
        match self.key {
            Key::Char('+') | Key::Char('=') => Some(1),
            Key::Char('-') | Key::Char('_') => Some(-1),
            // `Strg+0`: nur ohne Shift, denn `Strg+Shift+0` ist in manchen
            // Anwendungen „Zuruecksetzen" — und der Zoom soll daran nicht
            // haengen.
            Key::Char('0') if !self.modifiers.shift => Some(0),
            _ => None,
        }
    }

    /// Ob das Ereignis Strg+T ist (neuer Tab, W7C A5).
    pub fn is_ctrl_t(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('t')
    }

    /// Ob das Ereignis Strg+W ist (Tab schliessen, W7C A5).
    pub fn is_ctrl_w(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('w')
    }

    /// Ob das Ereignis Strg+Tab ist (Tab wechseln). `rueckwaerts` heisst
    /// Shift: in jedem Browser wechselt Strg+Shift+Tab zurueck.
    pub fn ist_ctrl_tab(self) -> Option<bool> {
        if !self.pressed || self.key != Key::Tab {
            return None;
        }
        if self.modifiers.ctrl && !self.modifiers.shift {
            return Some(true);
        }
        if self.modifiers.ctrl && self.modifiers.shift {
            return Some(false);
        }
        None
    }

    /// Ob das Ereignis Strg+F ist (in der Seite suchen, W7C A6).
    pub fn is_ctrl_f(self) -> bool {
        self.pressed && self.modifiers.only_ctrl() && self.key == Key::Char('f')
    }

    /// Ob das Ereignis F3 ist (naechster Suchtreffer, W7C A6).
    ///
    /// GEMESSEN, warum F3 und nicht Enter: Enter ist im Eingabefeld
    /// „abschicken". Eine Suche, die sich dazwischendraengt, kaeme nie
    /// zum zweiten Treffer, weil der Nutzer nach dem Muster ja Enter
    /// drueckt. F3 ist in jedem Browser der naechste Treffer.
    pub fn is_f3(self) -> bool {
        self.pressed && self.key == Key::F(3)
    }

    /// Ob das Ereignis Ctrl+R / F5 ist (Seite neu laden).
    /// Ob das Ereignis `Alt+Pfeil links` oder `Alt+Pfeil rechts` ist.
    ///
    /// added 02.10.2026, UI4: die Tastenkuerzel der Oberflaeche.
    /// `Some(true)` heisst vorwaerts (rechts), `Some(false)` zurueck
    /// (links), `None` heisst „kein Befehl".
    ///
    /// WARUM EINE METHODE UND ZWEI: links und rechts sind derselbe Befehl
    /// mit einer Richtung, und die Richtung ist die einzige Verschiedenheit.
    /// Zwei Methoden wuerden die Moduspruefung doppelt schreiben — und das
    /// ist genau der Fehler, den `zoom_befehl` schon einmal hatte (drei
    /// `bool` statt eines Feldes, siehe dessen Kopf).
    ///
    /// DIE TASTEN, und sie sind die, die ein echter Browser nimmt (UI4 in
    /// `AUTONOM.txt`): `Alt+Links` zurueck, `Alt+Rechts` vorwaerts. Ohne
    /// Modifier gehoeren die Pfeile dem Textcursor, deshalb verlangt die
    /// Methode `only_alt` — siehe deren Begruendung, warum `Shift` nicht
    /// erlaubt ist.
    pub fn alt_navigation(self) -> Option<bool> {
        if !self.pressed || !self.modifiers.only_alt() {
            return None;
        }
        match self.key {
            Key::Left => Some(false),
            Key::Right => Some(true),
            _ => None,
        }
    }

    /// Ob das Ereignis `F5` oder `Strg+R` ist (Neu laden).
    pub fn is_reload(self) -> bool {
        self.pressed
            && ((self.modifiers.only_ctrl() && self.key == Key::Char('r'))
                || (self.modifiers == Modifiers::NONE && self.key == Key::F(5)))
    }

    /// Das druckbare Zeichen, wenn es eines ist und die Taste gedrueckt wurde.
    /// Nur bei gedrueckt — beim Loslassen tippt man nicht.
    pub fn typed_char(self) -> Option<char> {
        if self.pressed && !self.modifiers.ctrl && !self.modifiers.alt {
            self.key.to_char()
        } else {
            None
        }
    }
}

impl std::fmt::Display for KeyEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let action = if self.pressed { "↓" } else { "↑" };
        if self.modifiers.any() {
            write!(f, "{action} {}+{}", self.modifiers, self.key)
        } else {
            write!(f, "{action} {}", self.key)
        }
    }
}

/// Uebersetzt ein X11-Keysym in einen [`Key`].
///
/// Diese Funktion gehoert hierher und nicht nach narsil-window, weil die
/// Zuordnung X11-Keysym → logische Taste eine Eigenschaft der SPRACHE
/// der Eingabe ist, nicht des Fensters. narsil-window liefert nur die Zahl.
///
/// Die Keysym-Werte sind aus X11/keysymdef.h (oeffentlich, stabil seit
/// 30 Jahren, keine Lizenzfrage).
pub fn keysym_to_key(keysym: u32) -> Key {
    match keysym {
        // Latin-1 druckbare Zeichen: 0x0020..0x007E
        0x0020 => Key::Space,
        0x0021..=0x007E => Key::Char(keysym as u8 as char),
        // Deutsche Umlaute und Sonderzeichen
        0x00E4 => Key::Char('ä'),
        0x00C4 => Key::Char('Ä'),
        0x00F6 => Key::Char('ö'),
        0x00D6 => Key::Char('Ö'),
        0x00FC => Key::Char('ü'),
        0x00DC => Key::Char('Ü'),
        0x00DF => Key::Char('ß'),
        // Steuerung
        0xFF08 => Key::Backspace,
        0xFF09 => Key::Tab,
        0xFF0D => Key::Enter,
        0xFF1B => Key::Escape,
        0xFFFF => Key::Delete,
        // Navigation
        0xFF50 => Key::Home,
        0xFF51 => Key::Left,
        0xFF52 => Key::Up,
        0xFF53 => Key::Right,
        0xFF54 => Key::Down,
        0xFF55 => Key::PageUp,
        0xFF56 => Key::PageDown,
        0xFF57 => Key::End,
        // Modifier
        0xFFE1 | 0xFFE2 => Key::Shift,   // Shift_L, Shift_R
        0xFFE3 | 0xFFE4 => Key::Control, // Control_L, Control_R
        0xFFE9 | 0xFFEA => Key::Alt,     // Alt_L, Alt_R
        // Funktionstasten F1..F12
        k @ 0xFFBE..=0xFFC9 => Key::F((k - 0xFFBE + 1) as u8),
        // Keypad-Ziffern
        0xFFB0..=0xFFB9 => Key::Char((b'0' + (keysym - 0xFFB0) as u8) as char),
        0xFFAD => Key::Char('-'), // KP_Subtract
        0xFFAB => Key::Char('+'), // KP_Add
        0xFFAA => Key::Char('*'), // KP_Multiply
        0xFFAF => Key::Char('/'), // KP_Divide
        0xFF8D => Key::Enter,     // KP_Enter
        0xFFAE => Key::Char('.'), // KP_Decimal
        // Unbekannt — kein stilles Verschlucken
        _ => Key::Unknown(keysym),
    }
}

/// Baut den [`Modifiers`]-Zustand aus der X11-Modifier-Maske (state-Feld
/// in XKeyEvent). Bits: Shift=1, Control=4, Mod1(Alt)=8.
pub fn x11_state_to_modifiers(state: u32) -> Modifiers {
    Modifiers {
        shift: state & 1 != 0,
        ctrl: state & 4 != 0,
        alt: state & 8 != 0,
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// added 02.10.2026, UI4. ALT+PFEIL ist der Verlauf, und **ohne**
    /// Modifier gehoeren die Pfeile dem Textcursor.
    ///
    /// DIE GEGENPROBE STEHT IN DIESEM TEST, und sie ist der Grund, warum er
    /// mehr als eine `assert!` hat: `alt_navigation` gibt `Option<bool>`
    /// zurueck, und `None` heisst „kein Befehl". Ein Test, der nur den
    /// positiven Fall prueft, wuerde eine Implementierung passieren lassen,
    /// die **alles** als Befehl liest — und die wuerde beim Tippen in der
    /// Adressleiste den Verlauf verschieben.
    #[test]
    fn alt_pfeil_ist_der_verlauf_und_ohne_alt_nicht() {
        let links = KeyEvent {
            pressed: true,
            key: Key::Left,
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        };
        let rechts = KeyEvent {
            pressed: true,
            key: Key::Right,
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        };
        assert_eq!(links.alt_navigation(), Some(false), "Alt+Links = zurueck");
        assert_eq!(
            rechts.alt_navigation(),
            Some(true),
            "Alt+Rechts = vorwaerts"
        );

        // OHNE Modifier: das ist der Textcursor in der Adressleiste.
        //
        // DER ERSTE VERSUCH DIESER SCHLEIFE WAR EIN LUEGENTEST: sie pruefte
        // `links` (dessen Modifier **immer** Alt sind) gegen `m`, ohne `m`
        // je zu verwenden — und war damit rot, ohne dass `alt_navigation`
        // etwas falsch gemacht haette. Ein Test, der eine Variable in der
        // Meldung nennt und sonst nirgends benutzt, testet nichts.
        let mit = |k: Key, m: Modifiers| KeyEvent {
            pressed: true,
            key: k,
            modifiers: m,
        };
        for m in [
            Modifiers::NONE,
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            // added 02.10.2026: ALTGR ist SHIFT+ALT. Wer `AltGr+{` tippt,
            // darf nicht zurueckblaettern — siehe `only_alt`.
            Modifiers {
                alt: true,
                shift: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ctrl: true,
                ..Modifiers::NONE
            },
        ] {
            assert_eq!(
                mit(Key::Left, m).alt_navigation(),
                None,
                "Pfeil links mit {m:?} darf kein Verlauf sein"
            );
            assert_eq!(
                mit(Key::Right, m).alt_navigation(),
                None,
                "Pfeil rechts mit {m:?} darf kein Verlauf sein"
            );
        }

        // BEIM LOSLASSEN: kein Befehl. Sonst zaehlte jedes Loslassen als
        // ein Zurueckblaettern.
        assert_eq!(
            KeyEvent {
                pressed: false,
                key: Key::Left,
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                },
            }
            .alt_navigation(),
            None,
            "das Loslassen ist kein Befehl"
        );

        // EINE ANDERE TASTE MIT ALT: `Alt+A` ist kein Verlauf. Sonst wuerde
        // die Reihenfolge der Abfragen entscheiden, ob ein Zug endet.
        assert_eq!(
            KeyEvent {
                pressed: true,
                key: Key::Char('a'),
                modifiers: Modifiers {
                    alt: true,
                    ..Modifiers::NONE
                },
            }
            .alt_navigation(),
            None,
            "Alt+A ist kein Verlauf"
        );
    }

    /// added 02.10.2026, UI4. `only_alt` ist das dritte Glied der Familie
    /// `only_ctrl` / `only_shift` / `only_alt`, und es wird **getrennt**
    /// geprueft, weil `zoom_befehl` (Strg) und die Textbefehle (Shift) an
    /// ihm vorbeikommen.
    #[test]
    fn only_alt_verlangt_genau_alt() {
        let nur_alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        assert!(nur_alt.only_alt());
        assert!(!nur_alt.only_ctrl());
        assert!(!nur_alt.only_shift());
        // Die beiden anderen Familien funktionieren unveraendert.
        assert!(Modifiers::NONE == Modifiers::NONE);
        assert!(
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
            .only_ctrl()
        );
        assert!(
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            }
            .only_shift()
        );
        assert!(
            Modifiers {
                alt: true,
                shift: true,
                ..Modifiers::NONE
            }
            .only_alt()
                == false,
            "AltGr ist nicht Alt"
        );
    }

    #[test]
    fn druckbare_zeichen_haben_char() {
        assert_eq!(Key::Char('a').to_char(), Some('a'));
        assert_eq!(Key::Space.to_char(), Some(' '));
        assert!(Key::Char('z').is_printable());
        assert!(Key::Space.is_printable());
    }

    #[test]
    fn steuerungstasten_haben_keinen_char() {
        assert_eq!(Key::Backspace.to_char(), None);
        assert_eq!(Key::Delete.to_char(), None);
        assert_eq!(Key::Left.to_char(), None);
        assert!(!Key::Left.is_printable());
    }

    #[test]
    fn enter_hat_newline() {
        assert_eq!(Key::Enter.to_char(), Some('\n'));
        assert!(!Key::Enter.is_printable()); // Enter ist Aktion, kein Zeichen
    }

    #[test]
    fn navigation_erkennung() {
        assert!(Key::Left.is_navigation());
        assert!(Key::Right.is_navigation());
        assert!(Key::Home.is_navigation());
        assert!(Key::End.is_navigation());
        assert!(!Key::Char('a').is_navigation());
        assert!(!Key::Enter.is_navigation());
    }

    #[test]
    fn modifier_erkennung() {
        assert!(Key::Shift.is_modifier());
        assert!(Key::Control.is_modifier());
        assert!(Key::Alt.is_modifier());
        assert!(!Key::Enter.is_modifier());
    }

    #[test]
    fn modifiers_none_hat_nichts() {
        let m = Modifiers::NONE;
        assert!(!m.any());
        assert!(!m.only_ctrl());
        assert!(!m.only_shift());
        assert_eq!(m, Modifiers::default());
    }

    #[test]
    fn modifiers_nur_ctrl() {
        let m = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert!(m.any());
        assert!(m.only_ctrl());
        assert!(!m.only_shift());
    }

    #[test]
    fn modifiers_ctrl_plus_shift_ist_nicht_nur_ctrl() {
        let m = Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        };
        assert!(m.any());
        assert!(!m.only_ctrl());
        assert!(!m.only_shift());
    }

    #[test]
    fn key_event_press_und_release() {
        let p = KeyEvent::press(Key::Char('a'));
        assert!(p.pressed);
        assert_eq!(p.key, Key::Char('a'));
        assert_eq!(p.modifiers, Modifiers::NONE);

        let r = KeyEvent::release(Key::Char('a'));
        assert!(!r.pressed);
    }

    #[test]
    fn typed_char_nur_bei_press_ohne_ctrl() {
        let p = KeyEvent::press(Key::Char('a'));
        assert_eq!(p.typed_char(), Some('a'));

        let r = KeyEvent::release(Key::Char('a'));
        assert_eq!(r.typed_char(), None); // Loslassen tippt nicht

        let ctrl_a = KeyEvent::press_with(
            Key::Char('a'),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(ctrl_a.typed_char(), None); // Ctrl+A ist Aktion, kein Zeichen
    }

    #[test]
    fn ctrl_shortcuts() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert!(KeyEvent::press_with(Key::Char('c'), ctrl).is_ctrl_c());
        assert!(KeyEvent::press_with(Key::Char('v'), ctrl).is_ctrl_v());
        assert!(KeyEvent::press_with(Key::Char('a'), ctrl).is_ctrl_a());
        assert!(KeyEvent::press_with(Key::Char('l'), ctrl).is_ctrl_l());
        assert!(KeyEvent::press_with(Key::Char('r'), ctrl).is_reload());
        assert!(KeyEvent::press(Key::F(5)).is_reload());

        // Loslassen ist kein Shortcut
        assert!(!KeyEvent::release(Key::Char('c')).is_ctrl_c());
    }

    #[test]
    fn keysym_buchstaben() {
        assert_eq!(keysym_to_key(0x0061), Key::Char('a'));
        assert_eq!(keysym_to_key(0x007A), Key::Char('z'));
        assert_eq!(keysym_to_key(0x0041), Key::Char('A'));
        assert_eq!(keysym_to_key(0x005A), Key::Char('Z'));
        assert_eq!(keysym_to_key(0x0030), Key::Char('0'));
        assert_eq!(keysym_to_key(0x0039), Key::Char('9'));
    }

    #[test]
    fn keysym_sonderzeichen() {
        assert_eq!(keysym_to_key(0x002F), Key::Char('/')); // Slash
        assert_eq!(keysym_to_key(0x003A), Key::Char(':')); // Doppelpunkt
        assert_eq!(keysym_to_key(0x002E), Key::Char('.')); // Punkt
        assert_eq!(keysym_to_key(0x002D), Key::Char('-')); // Minus
        assert_eq!(keysym_to_key(0x005F), Key::Char('_')); // Unterstrich
    }

    #[test]
    fn keysym_umlaute() {
        assert_eq!(keysym_to_key(0x00E4), Key::Char('ä'));
        assert_eq!(keysym_to_key(0x00C4), Key::Char('Ä'));
        assert_eq!(keysym_to_key(0x00F6), Key::Char('ö'));
        assert_eq!(keysym_to_key(0x00FC), Key::Char('ü'));
        assert_eq!(keysym_to_key(0x00DF), Key::Char('ß'));
    }

    #[test]
    fn keysym_steuerung() {
        assert_eq!(keysym_to_key(0xFF08), Key::Backspace);
        assert_eq!(keysym_to_key(0xFF09), Key::Tab);
        assert_eq!(keysym_to_key(0xFF0D), Key::Enter);
        assert_eq!(keysym_to_key(0xFF1B), Key::Escape);
        assert_eq!(keysym_to_key(0xFFFF), Key::Delete);
        assert_eq!(keysym_to_key(0x0020), Key::Space);
    }

    #[test]
    fn keysym_navigation() {
        assert_eq!(keysym_to_key(0xFF51), Key::Left);
        assert_eq!(keysym_to_key(0xFF52), Key::Up);
        assert_eq!(keysym_to_key(0xFF53), Key::Right);
        assert_eq!(keysym_to_key(0xFF54), Key::Down);
        assert_eq!(keysym_to_key(0xFF50), Key::Home);
        assert_eq!(keysym_to_key(0xFF57), Key::End);
    }

    #[test]
    fn keysym_modifier() {
        assert_eq!(keysym_to_key(0xFFE1), Key::Shift); // Shift_L
        assert_eq!(keysym_to_key(0xFFE2), Key::Shift); // Shift_R
        assert_eq!(keysym_to_key(0xFFE3), Key::Control); // Control_L
        assert_eq!(keysym_to_key(0xFFE4), Key::Control); // Control_R
        assert_eq!(keysym_to_key(0xFFE9), Key::Alt); // Alt_L
        assert_eq!(keysym_to_key(0xFFEA), Key::Alt); // Alt_R
    }

    #[test]
    fn keysym_funktionstasten() {
        assert_eq!(keysym_to_key(0xFFBE), Key::F(1));
        assert_eq!(keysym_to_key(0xFFC9), Key::F(12));
    }

    #[test]
    fn keysym_keypad() {
        assert_eq!(keysym_to_key(0xFFB0), Key::Char('0'));
        assert_eq!(keysym_to_key(0xFFB9), Key::Char('9'));
        assert_eq!(keysym_to_key(0xFF8D), Key::Enter); // KP_Enter
    }

    #[test]
    fn keysym_unbekannt() {
        assert_eq!(keysym_to_key(0xDEAD), Key::Unknown(0xDEAD));
    }

    #[test]
    fn x11_state_modifier_bits() {
        assert_eq!(x11_state_to_modifiers(0), Modifiers::NONE);
        assert_eq!(
            x11_state_to_modifiers(1),
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            x11_state_to_modifiers(4),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            x11_state_to_modifiers(8),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            x11_state_to_modifiers(5),
            Modifiers {
                shift: true,
                ctrl: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            x11_state_to_modifiers(13),
            Modifiers {
                shift: true,
                ctrl: true,
                alt: true
            }
        );
    }

    #[test]
    fn display_formatierung() {
        assert_eq!(format!("{}", Key::Char('a')), "a");
        assert_eq!(format!("{}", Key::Enter), "Enter");
        assert_eq!(format!("{}", Key::F(5)), "F5");
        assert_eq!(format!("{}", Key::Unknown(0xDEAD)), "Unknown(0xdead)");

        assert_eq!(format!("{}", Modifiers::NONE), "(keine)");
        assert_eq!(
            format!(
                "{}",
                Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                }
            ),
            "Ctrl"
        );
        assert_eq!(
            format!(
                "{}",
                Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::NONE
                }
            ),
            "Ctrl+Shift"
        );

        assert_eq!(format!("{}", KeyEvent::press(Key::Char('a'))), "↓ a");
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            format!("{}", KeyEvent::press_with(Key::Char('c'), ctrl)),
            "↓ Ctrl+c"
        );
    }
}
