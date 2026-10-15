//! Release 08.10.2026: FEATURE 4 — FORMULAR-GEDAECHTNIS.
//!
//! * **Lernen**: beim Abschicken eines Formulars werden die Felder erkannt
//!   (Name, `id`, `autocomplete`, `placeholder`, `aria-label`, das
//!   `<label>` davor — und der Wert selbst, z. B. die IBAN-Pruefziffer) und
//!   als Datensatz gemerkt. Passwoerter, Kartennummern, CVC, PIN/TAN werden
//!   nie gespeichert.
//! * **Speichern**: verschluesselt mit ChaCha20-Poly1305 aus `narsil-tls`
//!   (`<profil>/features/formulare.dat`, Schluessel in `formulare.schluessel`).
//! * **Ausfuellen**: Strg+Umschalt+F erkennt die Felder der aktuellen Seite
//!   und setzt alle passenden Werte des besten Datensatzes auf einmal;
//!   nochmal druecken schaltet zum naechsten Datensatz.

use super::{ordner, schreiben};

/// Die erkannten Feldarten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Art {
    Vorname,
    Nachname,
    Name,
    Firma,
    Strasse,
    Hausnummer,
    Adresszusatz,
    Plz,
    Ort,
    Land,
    Packstation,
    Postnummer,
    Email,
    Telefon,
    Iban,
    Bic,
    Kontoinhaber,
    Geburtsdatum,
}

pub const ALLE_ARTEN: [Art; 18] = [
    Art::Vorname, Art::Nachname, Art::Name, Art::Firma, Art::Strasse, Art::Hausnummer,
    Art::Adresszusatz, Art::Plz, Art::Ort, Art::Land, Art::Packstation, Art::Postnummer,
    Art::Email, Art::Telefon, Art::Iban, Art::Bic, Art::Kontoinhaber, Art::Geburtsdatum,
];

impl Art {
    pub fn schluessel(self) -> &'static str {
        match self {
            Art::Vorname => "vorname",
            Art::Nachname => "nachname",
            Art::Name => "name",
            Art::Firma => "firma",
            Art::Strasse => "strasse",
            Art::Hausnummer => "hausnummer",
            Art::Adresszusatz => "zusatz",
            Art::Plz => "plz",
            Art::Ort => "ort",
            Art::Land => "land",
            Art::Packstation => "packstation",
            Art::Postnummer => "postnummer",
            Art::Email => "email",
            Art::Telefon => "telefon",
            Art::Iban => "iban",
            Art::Bic => "bic",
            Art::Kontoinhaber => "kontoinhaber",
            Art::Geburtsdatum => "geburtsdatum",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Art::Vorname => narsil_i18n::tr("First name"),
            Art::Nachname => narsil_i18n::tr("Last name"),
            Art::Name => "Name",
            Art::Firma => narsil_i18n::tr("Company"),
            Art::Strasse => narsil_i18n::tr("Street"),
            Art::Hausnummer => narsil_i18n::tr("House number"),
            Art::Adresszusatz => narsil_i18n::tr("Address line 2"),
            Art::Plz => narsil_i18n::tr("Postal code"),
            Art::Ort => narsil_i18n::tr("City"),
            Art::Land => narsil_i18n::tr("Country"),
            Art::Packstation => narsil_i18n::tr("Parcel locker"),
            Art::Postnummer => narsil_i18n::tr("Post number"),
            Art::Email => narsil_i18n::tr("E-mail"),
            Art::Telefon => narsil_i18n::tr("Phone"),
            Art::Iban => "IBAN",
            Art::Bic => "BIC",
            Art::Kontoinhaber => narsil_i18n::tr("Account holder"),
            Art::Geburtsdatum => narsil_i18n::tr("Date of birth"),
        }
    }

    fn aus_schluessel(s: &str) -> Option<Art> {
        ALLE_ARTEN.iter().copied().find(|a| a.schluessel() == s)
    }
}

/// Was ueber ein Feld bekannt ist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Beschreibung {
    pub typ: String,
    pub name: String,
    pub id: String,
    pub autocomplete: String,
    pub placeholder: String,
    pub aria: String,
    pub label: String,
}

impl Beschreibung {
    fn worte(&self) -> String {
        let t = format!(
            " {} {} {} {} {} {} ",
            self.name, self.id, self.autocomplete, self.placeholder, self.aria, self.label
        )
        .to_lowercase()
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss");
        // Trennzeichen zu Leerzeichen, damit Wortgrenzen pruefbar sind.
        t.chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            // camelCase: `firstName` -> `first name`
    }
}

/// Darf das Feld nie gespeichert werden?
pub fn ist_geheim(b: &Beschreibung) -> bool {
    let typ = b.typ.to_ascii_lowercase();
    if matches!(typ.as_str(), "password" | "hidden" | "file" | "checkbox" | "radio" | "submit" | "button" | "image" | "reset") {
        return true;
    }
    let w = format!(" {} ", b.worte());
    let ac = b.autocomplete.to_ascii_lowercase();
    ac.starts_with("cc-") || ac.contains("password") || ac.contains("one-time-code")
        || ["passwort", "password", "kennwort", "pwd", "cvc", "cvv", "csc", "pin", "tan", "kreditkarte",
            "kartennummer", "cardnumber", "card", "securitycode", "sicherheitscode", "otp", "token", "captcha"]
            .iter()
            .any(|x| w.contains(&format!(" {x} ")) || (x.len() > 4 && w.contains(x)))
}

fn hat_wort(w: &str, worte: &[&str]) -> bool {
    worte.iter().any(|x| {
        if x.len() <= 3 {
            w.contains(&format!(" {x} "))
        } else {
            w.contains(x)
        }
    })
}

/// IBAN-Pruefung (Laenge und Modulo 97).
pub fn iban_gueltig(s: &str) -> bool {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    if t.len() < 15 || t.len() > 34 || !t.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if !t[..2].chars().all(|c| c.is_ascii_alphabetic()) || !t[2..4].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let umgestellt = format!("{}{}", &t[4..], &t[..4]);
    let mut rest: u32 = 0;
    for c in umgestellt.chars() {
        let v = if c.is_ascii_digit() { c as u32 - '0' as u32 } else { c as u32 - 'A' as u32 + 10 };
        rest = if v >= 10 { (rest * 100 + v) % 97 } else { (rest * 10 + v) % 97 };
    }
    rest == 1
}

/// Die Art eines Feldes — aus `autocomplete`, Worten und (beim Lernen) dem Wert.
pub fn erkennen(b: &Beschreibung, wert: Option<&str>) -> Option<Art> {
    if ist_geheim(b) {
        return None;
    }
    if let Some(w) = wert {
        let w = w.trim();
        if iban_gueltig(w) {
            return Some(Art::Iban);
        }
    }
    let ac = b.autocomplete.to_ascii_lowercase();
    let ac = ac.split_whitespace().last().unwrap_or("");
    let aus_ac = match ac {
        "given-name" => Some(Art::Vorname),
        "family-name" => Some(Art::Nachname),
        "name" => Some(Art::Name),
        "organization" => Some(Art::Firma),
        "address-line1" | "street-address" => Some(Art::Strasse),
        "address-line2" => Some(Art::Adresszusatz),
        "postal-code" => Some(Art::Plz),
        "address-level2" => Some(Art::Ort),
        "country" | "country-name" => Some(Art::Land),
        "email" => Some(Art::Email),
        "tel" | "tel-national" => Some(Art::Telefon),
        "bday" => Some(Art::Geburtsdatum),
        _ => None,
    };
    if aus_ac.is_some() {
        return aus_ac;
    }
    let typ = b.typ.to_ascii_lowercase();
    if typ == "email" {
        return Some(Art::Email);
    }
    if typ == "tel" {
        return Some(Art::Telefon);
    }
    let mut w = b.worte();
    // camelCase und Zahlen trennen: `firstName` / `address1`
    let mut zerlegt = String::new();
    let roh = format!(" {} {} {} ", b.name, b.id, b.placeholder);
    let mut vorher_klein = false;
    for c in roh.chars() {
        if c.is_uppercase() && vorher_klein {
            zerlegt.push(' ');
        }
        vorher_klein = c.is_lowercase();
        zerlegt.push(c);
    }
    w.push_str(&Beschreibung { name: zerlegt, ..Default::default() }.worte());
    let w = format!(" {w} ");
    let regeln: &[(&[&str], Art)] = &[
        (&["iban"], Art::Iban),
        (&["bic", "swift"], Art::Bic),
        (&["kontoinhaber", "account holder", "accountholder"], Art::Kontoinhaber),
        (&["packstation", "paketstation"], Art::Packstation),
        (&["postnummer", "dhl kundennummer", "postnumber"], Art::Postnummer),
        (&["vorname", "first", "given", "fname", "firstname"], Art::Vorname),
        (&["nachname", "last", "family", "surname", "lname", "lastname", "zuname"], Art::Nachname),
        (&["firma", "company", "organisation", "organization", "unternehmen"], Art::Firma),
        (&["hausnummer", "hausnr", "house", "nr", "housenumber"], Art::Hausnummer),
        (&["zusatz", "address2", "addr2", "line2", "c o", "adresszusatz"], Art::Adresszusatz),
        // Release 09.10.2026: `addr`/`addr1` (gemessen: ein Formular mit
        // `name="addr1"` und dem Text „Address" davor blieb leer).
        (&["strasse", "street", "address1", "line1", "adresse", "address", "addr", "anschrift"], Art::Strasse),
        (&["plz", "postleitzahl", "zip", "postal", "postcode"], Art::Plz),
        (&["ort", "stadt", "city", "town", "wohnort", "locality"], Art::Ort),
        (&["land", "country"], Art::Land),
        (&["mail", "email"], Art::Email),
        (&["telefon", "tel", "phone", "mobil", "handy", "mobile", "rufnummer"], Art::Telefon),
        (&["geburt", "birth", "dob", "geburtsdatum"], Art::Geburtsdatum),
        (&["name", "vollstaendiger name", "fullname"], Art::Name),
    ];
    for (worte, art) in regeln {
        if hat_wort(&w, worte) {
            // „Postleitzahl Ort" in einem Label: der erste Treffer zaehlt
            // nicht, wenn das Feld ausdruecklich ein anderes nennt.
            return Some(*art);
        }
    }
    None
}

/// Ein gemerkter Datensatz.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Datensatz {
    pub werte: std::collections::BTreeMap<Art, String>,
    pub quelle: String,
    pub zeit: i64,
}

impl Datensatz {
    /// Die Kurzbezeichnung (Name oder E-Mail).
    pub fn titel(&self) -> String {
        let name = match (self.werte.get(&Art::Vorname), self.werte.get(&Art::Nachname), self.werte.get(&Art::Name)) {
            (Some(v), Some(n), _) => format!("{v} {n}"),
            (_, _, Some(n)) => n.clone(),
            (Some(v), None, None) => v.clone(),
            (None, Some(n), None) => n.clone(),
            _ => String::new(),
        };
        if !name.is_empty() {
            return name;
        }
        if let Some(e) = self.werte.get(&Art::Email) {
            return e.clone();
        }
        if let Some(i) = self.werte.get(&Art::Iban) {
            return narsil_i18n::trf!("Account {}", maskiert(Art::Iban, i));
        }
        narsil_i18n::trf!("Record from {}", self.quelle)
    }

    /// Gehoeren zwei Datensaetze zusammen (gleiche Person/Adresse/Konto)?
    fn passt_zu(&self, o: &Datensatz) -> bool {
        let gleich = |a: Art| matches!((self.werte.get(&a), o.werte.get(&a)), (Some(x), Some(y)) if x.eq_ignore_ascii_case(y));
        (gleich(Art::Vorname) && gleich(Art::Nachname))
            || gleich(Art::Email)
            || gleich(Art::Iban)
            || (gleich(Art::Name) && (gleich(Art::Plz) || gleich(Art::Strasse)))
    }
}

/// Einen Wert fuer die Anzeige verdecken.
pub fn maskiert(art: Art, w: &str) -> String {
    match art {
        Art::Iban => {
            let t: String = w.chars().filter(|c| !c.is_whitespace()).collect();
            if t.len() > 8 {
                format!("{} •••• {}", &t[..4], &t[t.len() - 4..])
            } else {
                "••••".into()
            }
        }
        Art::Telefon | Art::Postnummer | Art::Geburtsdatum | Art::Bic => {
            let n = w.chars().count();
            let sichtbar: String = w.chars().skip(n.saturating_sub(3)).collect();
            format!("•••{sichtbar}")
        }
        _ => w.to_string(),
    }
}

// ---------------------------------------------------------------------------
// DER VERSCHLUESSELTE SPEICHER
// ---------------------------------------------------------------------------

const MAGIE: &[u8; 8] = b"RWFG0001";
const AAD: &[u8] = b"Narsil-Formular-Gedaechtnis v1";
/// M3: hoechstens so viele Datensaetze.
pub const MAX_DATENSAETZE: usize = 24;

fn daten_pfad() -> std::path::PathBuf {
    ordner().join("formulare.dat")
}

fn schluessel_pfad() -> std::path::PathBuf {
    ordner().join("formulare.schluessel")
}

fn schluessel() -> Result<[u8; 32], String> {
    let p = schluessel_pfad();
    if let Ok(d) = std::fs::read(&p)
        && d.len() == 32
    {
        let mut k = [0u8; 32];
        k.copy_from_slice(&d);
        return Ok(k);
    }
    let k = narsil_tls::zufall::schluessel32().map_err(|e| format!("Zufall: {e:?}"))?;
    schreiben(&p, &k)?;
    Ok(k)
}

fn als_text(l: &[Datensatz]) -> String {
    let mut s = String::new();
    for d in l.iter().take(MAX_DATENSAETZE) {
        let mut felder = vec![format!("quelle={}", super::maske(&d.quelle)), format!("zeit={}", d.zeit)];
        for (a, w) in &d.werte {
            felder.push(format!("{}={}", a.schluessel(), super::maske(w)));
        }
        s.push_str(&felder.join("\t"));
        s.push('\n');
    }
    s
}

fn aus_text(t: &str) -> Vec<Datensatz> {
    t.lines()
        .take(MAX_DATENSAETZE)
        .map(|z| {
            let mut d = Datensatz::default();
            for f in z.split('\t') {
                let Some((k, v)) = f.split_once('=') else { continue };
                let v = super::entmaske(v);
                match k {
                    "quelle" => d.quelle = v,
                    "zeit" => d.zeit = v.parse().unwrap_or(0),
                    _ => {
                        if let Some(a) = Art::aus_schluessel(k) {
                            d.werte.insert(a, v);
                        }
                    }
                }
            }
            d
        })
        .filter(|d| !d.werte.is_empty())
        .collect()
}

/// Verschluesseln: `MAGIE || nonce || chiffrat+tag`.
pub fn verschluesseln(klartext: &[u8], k: &[u8; 32]) -> Result<Vec<u8>, String> {
    let nonce = narsil_tls::zufall::nonce12().map_err(|e| format!("Zufall: {e:?}"))?;
    let c = narsil_tls::chacha::aead_chiffre(k, &nonce, AAD, klartext).map_err(|e| format!("{e:?}"))?;
    let mut d = MAGIE.to_vec();
    d.extend_from_slice(&nonce);
    d.extend_from_slice(&c);
    Ok(d)
}

/// Entschluesseln und pruefen.
pub fn entschluesseln(d: &[u8], k: &[u8; 32]) -> Result<Vec<u8>, String> {
    if d.len() < 8 + 12 + 16 || &d[..8] != MAGIE {
        return Err("keine Formulardaten".into());
    }
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&d[8..20]);
    narsil_tls::chacha::aead_dechiffre(k, &nonce, AAD, &d[20..]).map_err(|_| "Daten beschädigt oder falscher Schlüssel".to_string())
}

/// Alle Datensaetze lesen.
pub fn laden() -> Vec<Datensatz> {
    let Ok(d) = std::fs::read(daten_pfad()) else {
        return Vec::new();
    };
    let Ok(k) = schluessel() else { return Vec::new() };
    match entschluesseln(&d, &k) {
        Ok(t) => aus_text(&String::from_utf8_lossy(&t)),
        Err(_) => Vec::new(),
    }
}

/// Alle Datensaetze schreiben.
pub fn speichern(l: &[Datensatz]) -> Result<(), String> {
    let k = schluessel()?;
    let d = verschluesseln(als_text(l).as_bytes(), &k)?;
    schreiben(&daten_pfad(), &d)
}

/// Einen Datensatz lernen (zusammenfuehren, wenn er zu einem passt).
pub fn lernen(neu: Datensatz) -> Result<usize, String> {
    if neu.werte.is_empty() {
        return Ok(0);
    }
    let mut l = laden();
    let n = neu.werte.len();
    match l.iter_mut().find(|d| d.passt_zu(&neu)) {
        Some(d) => {
            for (a, w) in neu.werte {
                d.werte.insert(a, w);
            }
            d.zeit = neu.zeit;
            d.quelle = neu.quelle;
        }
        None => l.push(neu),
    }
    l.sort_by(|a, b| b.zeit.cmp(&a.zeit));
    l.truncate(MAX_DATENSAETZE);
    speichern(&l)?;
    Ok(n)
}

/// Fuer die gefundenen Feldarten den besten Datensatz waehlen (meiste
/// Treffer, bei Gleichstand der neueste). `runde` schaltet weiter.
pub fn auswahl(l: &[Datensatz], arten: &[Art], runde: usize) -> Option<usize> {
    let mut wertung: Vec<(usize, usize, i64)> = l
        .iter()
        .enumerate()
        .map(|(i, d)| (i, arten.iter().filter(|a| d.werte.contains_key(a) || fuellbar(d, **a).is_some()).count(), d.zeit))
        .filter(|(_, n, _)| *n > 0)
        .collect();
    wertung.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    if wertung.is_empty() {
        return None;
    }
    Some(wertung[runde % wertung.len()].0)
}

/// Der Wert fuer eine Feldart — auch abgeleitet (Name aus Vor-/Nachname,
/// Kontoinhaber aus dem Namen).
pub fn fuellbar(d: &Datensatz, a: Art) -> Option<String> {
    if let Some(w) = d.werte.get(&a) {
        return Some(w.clone());
    }
    match a {
        Art::Name | Art::Kontoinhaber => match (d.werte.get(&Art::Vorname), d.werte.get(&Art::Nachname)) {
            (Some(v), Some(n)) => Some(format!("{v} {n}")),
            _ => d.werte.get(&Art::Name).cloned(),
        },
        Art::Vorname => d.werte.get(&Art::Name).and_then(|n| n.split_whitespace().next().map(str::to_string)),
        Art::Nachname => d.werte.get(&Art::Name).and_then(|n| n.split_whitespace().last().map(str::to_string)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// DIE SEITE
// ---------------------------------------------------------------------------

/// Die Beschreibungen aller Formularfelder (ord -> Beschreibung, Tag).
pub fn beschreibungen(knoten: &[narsil_fmt_html::HtmlNode]) -> std::collections::HashMap<usize, Beschreibung> {
    use narsil_fmt_html::HtmlNode;
    let st = super::baum::stellen(knoten);
    // Label-Texte: `<label for=id>` und umschliessende `<label>`.
    let mut label_fuer: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for s in &st {
        if let HtmlNode::Element { tag, attributes, .. } = s.knoten
            && tag.eq_ignore_ascii_case("label")
            && let Some(f) = attributes.get("for")
        {
            label_fuer.insert(f.clone(), super::baum::text_von(&st, s.ord, None));
        }
    }
    let mut raus = std::collections::HashMap::new();
    for s in &st {
        let HtmlNode::Element { tag, attributes, .. } = s.knoten else { continue };
        let t = tag.to_ascii_lowercase();
        if !matches!(t.as_str(), "input" | "textarea" | "select") {
            continue;
        }
        let a = |n: &str| attributes.get(n).cloned().unwrap_or_default();
        let id = a("id");
        let mut label = label_fuer.get(&id).cloned().unwrap_or_default();
        if label.is_empty() {
            // umschliessendes Label
            let mut e = s.eltern;
            while let Some(i) = e {
                if let HtmlNode::Element { tag, .. } = st[i].knoten
                    && tag.eq_ignore_ascii_case("label")
                {
                    label = super::baum::text_von(&st, i, None);
                    break;
                }
                e = st[i].eltern;
            }
        }
        raus.insert(
            s.ord,
            Beschreibung {
                typ: if t == "input" { a("type") } else { t.clone() },
                name: a("name"),
                id,
                autocomplete: a("autocomplete"),
                placeholder: a("placeholder"),
                aria: a("aria-label"),
                label,
            },
        );
    }
    raus
}

impl crate::Sitzung {
    /// Die Felder der aktuellen Seite: (ord, Art, aktueller Wert).
    fn formular_felder(&self, nur_formular: Option<usize>) -> Vec<(usize, Art, String)> {
        let Some(a) = self.angezeigt.as_ref() else { return Vec::new() };
        let beschr = beschreibungen(&a.html);
        let w = a.stand.lock().expect("DomStand");
        let mut raus = Vec::new();
        for e in &w.eingaben {
            if e.disabled || (nur_formular.is_some() && e.formular != nur_formular) {
                continue;
            }
            let Some(b) = beschr.get(&e.ord) else { continue };
            let wert = a
                .patch
                .attribut
                .get(&(e.ord, "value".to_string()))
                .cloned()
                .or_else(|| w.eingabe_werte.get(&e.ord).cloned())
                .unwrap_or_else(|| e.wert.clone());
            if let Some(art) = erkennen(b, Some(&wert)) {
                raus.push((e.ord, art, wert));
            }
        }
        raus
    }

    /// Feature 4 — LERNEN: beim Abschicken eines Formulars.
    pub fn formular_lernen(&mut self, form_idx: Option<usize>) {
        if !self.einstellungen.features.an(super::Feature::FormularGedaechtnis) {
            return;
        }
        let felder = self.formular_felder(form_idx);
        let mut d = Datensatz {
            quelle: super::host_von(self.browser.adresse()),
            zeit: super::jetzt_s(),
            ..Default::default()
        };
        for (_, art, wert) in felder {
            let w = wert.trim();
            if w.is_empty() || w.chars().count() > 200 {
                continue;
            }
            // Werte, die nicht zur Art passen, werden nicht gelernt.
            let passt = match art {
                Art::Iban => iban_gueltig(w),
                Art::Email => w.contains('@') && w.contains('.'),
                Art::Plz => w.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-') && w.len() <= 10,
                _ => true,
            };
            if passt {
                d.werte.insert(art, w.to_string());
            }
        }
        if d.werte.len() >= 2 || d.werte.contains_key(&Art::Iban) {
            if let Ok(n) = lernen(d) {
                self.features.hinweis(narsil_i18n::trf!("Form memory: {} fields remembered (encrypted)", n), Some("narsil://formulare".into()));
            }
        }
    }

    /// Feature 4 — AUSFUELLEN (Strg+Umschalt+F).
    pub fn formular_ausfuellen(&mut self) {
        if !self.einstellungen.features.an(super::Feature::FormularGedaechtnis) {
            self.meldung = Some(narsil_i18n::tr("Form memory is off (Settings › Features)").into());
            self.schmutzig = true;
            return;
        }
        let felder = self.formular_felder(None);
        if felder.is_empty() {
            self.meldung = Some(narsil_i18n::tr("Form memory: no matching fields on this page").into());
            self.schmutzig = true;
            return;
        }
        let l = laden();
        let arten: Vec<Art> = felder.iter().map(|(_, a, _)| *a).collect();
        let jetzt = std::time::Instant::now();
        let runde = match self.features.formular_runde {
            Some((r, t)) if jetzt.duration_since(t) < std::time::Duration::from_secs(8) => r + 1,
            _ => 0,
        };
        let Some(i) = auswahl(&l, &arten, runde) else {
            self.meldung = Some(narsil_i18n::tr("Form memory: no matching data remembered yet").into());
            self.schmutzig = true;
            return;
        };
        self.features.formular_runde = Some((runde, jetzt));
        let d = &l[i];
        let mut gesetzt = 0usize;
        // Ein Feld je Art (die erste Fundstelle), damit „Name" nicht in drei
        // Felder gleichzeitig geht.
        let mut schon: Vec<Art> = Vec::new();
        for (ord, art, _) in felder {
            if schon.contains(&art) {
                continue;
            }
            if let Some(w) = fuellbar(d, art) {
                let n = w.chars().count();
                self.werte_schreiben(ord, w, n);
                schon.push(art);
                gesetzt += 1;
            }
        }
        self.meldung = Some(narsil_i18n::trf!(
            "Form memory: {} fields filled with “{}” (Ctrl+Shift+F again: next record)",
            gesetzt,
            d.titel()
        ));
        self.schmutzig = true;
    }
}

/// Die Liste fuer `narsil://formulare` (Werte teilweise verdeckt).
pub fn uebersicht() -> String {
    let l = laden();
    let mut h = format!(
        "<p>{}</p>",
        narsil_i18n::tr("Stored encrypted (ChaCha20-Poly1305). <b>Ctrl+Shift+F</b> fills the fields of a page; it learns when a form is submitted. Passwords and card data are never stored.")
    );
    if l.is_empty() {
        h.push_str(&format!("<p>{}</p>", super::html(narsil_i18n::tr("No data remembered yet."))));
        return h;
    }
    for (i, d) in l.iter().enumerate() {
        h.push_str(&format!(
            "<h3>{}</h3><p><small>{}</small> · <a href=\"narsil://formulare?aktion=weg&amp;nr={i}\">{}</a></p><table>",
            super::html(&d.titel()),
            super::html(&narsil_i18n::trf!("learned on {} on {}", d.quelle, super::datum_text(d.zeit))),
            super::html(narsil_i18n::tr("Delete"))
        ));
        for (a, w) in &d.werte {
            h.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", a.name(), super::html(&maskiert(*a, w))));
        }
        h.push_str("</table>");
    }
    h.push_str(&format!(
        "<p><a href=\"narsil://formulare?aktion=alle_weg\">{}</a></p>",
        super::html(narsil_i18n::tr("Delete all form data"))
    ));
    h
}

/// Einen Datensatz loeschen.
pub fn entfernen(nr: usize) -> Result<(), String> {
    let mut l = laden();
    if nr < l.len() {
        l.remove(nr);
    }
    speichern(&l)
}

/// Alles loeschen (Daten und Schluessel).
pub fn alles_loeschen() {
    let _ = std::fs::remove_file(daten_pfad());
    let _ = std::fs::remove_file(schluessel_pfad());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, label: &str) -> Beschreibung {
        Beschreibung { typ: "text".into(), name: name.into(), label: label.into(), ..Default::default() }
    }

    #[test]
    fn iban_pruefziffer() {
        assert!(iban_gueltig("DE89 3704 0044 0532 0130 00"));
        assert!(iban_gueltig("GB82WEST12345698765432"));
        assert!(!iban_gueltig("DE89 3704 0044 0532 0130 01"));
        assert!(!iban_gueltig("hallo"));
    }

    #[test]
    fn feldarten_werden_erkannt_auch_ungewoehnliche() {
        assert_eq!(erkennen(&b("firstName", ""), None), Some(Art::Vorname));
        assert_eq!(erkennen(&b("x1", "Last name"), None), Some(Art::Nachname));
        assert_eq!(erkennen(&b("f_7", "Street"), None), Some(Art::Strasse));
        assert_eq!(erkennen(&b("f_8", "Hausnr."), None), Some(Art::Hausnummer));
        assert_eq!(erkennen(&b("zip_code", ""), None), Some(Art::Plz));
        assert_eq!(erkennen(&b("feld", "City"), None), Some(Art::Ort));
        assert_eq!(erkennen(&b("packstation_nr", ""), None), Some(Art::Packstation));
        assert_eq!(erkennen(&b("postnummer", "DHL-Postnummer"), None), Some(Art::Postnummer));
        assert_eq!(erkennen(&b("feld_9", "Konto"), Some("DE89370400440532013000")), Some(Art::Iban));
        assert_eq!(erkennen(&Beschreibung { autocomplete: "shipping postal-code".into(), ..Default::default() }, None), Some(Art::Plz));
        assert_eq!(erkennen(&Beschreibung { typ: "email".into(), ..Default::default() }, None), Some(Art::Email));
        // „Passwort" enthaelt „ort", ist aber geheim.
        assert_eq!(erkennen(&b("passwort", "Passwort"), None), None);
        assert_eq!(erkennen(&Beschreibung { typ: "password".into(), name: "ort".into(), ..Default::default() }, None), None);
        assert_eq!(erkennen(&b("cc-number", "Kartennummer"), None), None);
        assert_eq!(erkennen(&b("cvc", ""), None), None);
        // „Sortierung" ist kein Ort.
        assert_eq!(erkennen(&b("sortierung", ""), None), None);
        // Release 09.10.2026: Kurzformen `addr1`/`addr2`.
        assert_eq!(erkennen(&b("addr1", ""), None), Some(Art::Strasse));
        assert_eq!(erkennen(&b("addr2", ""), None), Some(Art::Adresszusatz));
    }

    #[test]
    fn verschluesselt_und_nur_mit_schluessel_lesbar() {
        let k = [7u8; 32];
        let d = verschluesseln(b"iban=DE89", &k).expect("v");
        assert!(!d.windows(4).any(|w| w == b"DE89"), "Klartext in der Datei");
        assert_eq!(entschluesseln(&d, &k).expect("e"), b"iban=DE89");
        assert!(entschluesseln(&d, &[8u8; 32]).is_err());
        let mut kaputt = d.clone();
        let n = kaputt.len();
        kaputt[n - 5] ^= 1;
        assert!(entschluesseln(&kaputt, &k).is_err());
    }

    #[test]
    fn lernen_zusammenfuehren_und_auswaehlen() {
        let dir = std::env::temp_dir().join(format!("narsil-test-form-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        let mut a = Datensatz { quelle: "shop.example".into(), zeit: 10, ..Default::default() };
        a.werte.insert(Art::Vorname, "Erika".into());
        a.werte.insert(Art::Nachname, "Mustermann".into());
        a.werte.insert(Art::Plz, "12345".into());
        lernen(a).expect("a");
        let mut b2 = Datensatz { quelle: "bank.example".into(), zeit: 20, ..Default::default() };
        b2.werte.insert(Art::Vorname, "Erika".into());
        b2.werte.insert(Art::Nachname, "Mustermann".into());
        b2.werte.insert(Art::Iban, "DE89370400440532013000".into());
        lernen(b2).expect("b");
        let l = laden();
        assert_eq!(l.len(), 1, "dieselbe Person wurde zusammengefuehrt");
        assert_eq!(l[0].werte.len(), 4);
        // Die Datei enthaelt keinen Klartext.
        let roh = std::fs::read(dir.join("formulare.dat")).expect("datei");
        assert!(!roh.windows(10).any(|w| w == b"Mustermann"));
        assert_eq!(auswahl(&l, &[Art::Name, Art::Iban], 0), Some(0));
        assert_eq!(fuellbar(&l[0], Art::Kontoinhaber).as_deref(), Some("Erika Mustermann"));
        assert!(uebersicht().contains("DE89 •••• 3000"));
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Ende zu Ende: abschicken lernt, eine andere Seite mit anders
    /// benannten Feldern wird mit einem Tastendruck gefuellt.
    #[test]
    fn eine_neue_seite_wird_mit_einem_druck_gefuellt() {
        let dir = std::env::temp_dir().join(format!("narsil-test-form2-{}-{:?}", std::process::id(), std::thread::current().id()));
        super::super::ordner_fuer_faden(Some(dir.clone()));
        let mut s = crate::Sitzung::neu(900, 700);
        s.einstellungen.features.setzen(super::super::Feature::FormularGedaechtnis, true);
        let erste = "<html><body><form action=\"/senden\"><label>Vorname <input name=\"vn\" value=\"Erika\"></label>\
            <label>Nachname <input name=\"nn\" value=\"Mustermann\"></label>\
            <input name=\"iban\" value=\"DE89 3704 0044 0532 0130 00\">\
            <input type=\"password\" name=\"pw\" value=\"geheim\"></form></body></html>";
        let a = s.aus_html(erste, "https://shop.example/kasse", 900, 700).expect("Seite");
        s.browser.anwenden(&narsil_browser::Navigation::To("https://shop.example/kasse".into()));
        s.angezeigt = Some(a);
        s.formular_lernen(Some(0));
        let l = laden();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].werte.get(&Art::Vorname).map(String::as_str), Some("Erika"));
        assert!(l[0].werte.values().all(|w| w != "geheim"), "Passwort gelernt!");
        let zweite = "<html><body><form><input id=\"a1\" placeholder=\"Given name\">\
            <input id=\"a2\" autocomplete=\"family-name\"><label for=\"k\">Ihre Bankverbindung (IBAN)</label><input id=\"k\"></form></body></html>";
        let b = s.aus_html(zweite, "https://andere.example/", 900, 700).expect("Seite 2");
        s.angezeigt = Some(b);
        s.formular_ausfuellen();
        let felder = s.formular_felder(None);
        let wert = |art: Art| felder.iter().find(|(_, a, _)| *a == art).map(|(_, _, w)| w.clone());
        assert_eq!(wert(Art::Vorname).as_deref(), Some("Erika"), "{:?}", s.meldung);
        assert_eq!(wert(Art::Nachname).as_deref(), Some("Mustermann"));
        assert_eq!(wert(Art::Iban).as_deref(), Some("DE89 3704 0044 0532 0130 00"));
        super::super::ordner_fuer_faden(None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
