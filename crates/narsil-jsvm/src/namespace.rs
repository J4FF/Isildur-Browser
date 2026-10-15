//! Qualified-name validation shared by the VM DOM and the browser bridge.
//!
//! Release 13.10.2026: die gelockerten Namensregeln von DOM §1.4 (2024,
//! „valid element local name", „valid attribute local name", „valid
//! namespace prefix") statt der XML-`QName`-Produktion. GEMESSEN vorher (WPT
//! dom/nodes/Document-createElementNS 540/596, Document-createAttribute
//! 22/36): `createElementNS(null, "f}oo")`, `"f:o:o"` mit Namensraum und
//! `createAttribute("invalid^Name")` warfen InvalidCharacterError.

/// Infra „ASCII whitespace".
fn leer(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ')
}

/// DOM §1.4 „valid namespace prefix": nicht leer, kein ASCII-Leerraum, NUL,
/// `/` oder `>`.
pub fn prefix_ok(p: &str) -> bool {
    !p.is_empty() && !p.chars().any(|c| leer(c) || matches!(c, '\0' | '/' | '>'))
}

/// DOM §1.4 „valid attribute local name": nicht leer, kein ASCII-Leerraum,
/// NUL, `/`, `>` oder `=`.
pub fn attribute_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.chars().any(|c| leer(c) || matches!(c, '\0' | '/' | '>' | '=')) {
        return Err("InvalidCharacterError");
    }
    Ok(())
}

/// DOM §1.4 „validate and extract" (Kontext Element oder Attribut): das
/// Praefix steht vor dem ERSTEN `:`, der lokale Name ist der Rest.
fn extrahieren(
    namespace: Option<&str>,
    name: &str,
    element: bool,
) -> Result<(Option<String>, String), &'static str> {
    let ns = namespace.filter(|s| !s.is_empty());
    let (prefix, local) = match name.split_once(':') {
        Some((p, l)) => {
            if !prefix_ok(p) {
                return Err("InvalidCharacterError");
            }
            (Some(p), l)
        }
        None => (None, name),
    };
    if element {
        element_name(local)?;
    } else {
        attribute_name(local)?;
    }
    if prefix.is_some() && ns.is_none()
        || prefix == Some("xml") && ns != Some("http://www.w3.org/XML/1998/namespace")
        || (name == "xmlns" || prefix == Some("xmlns")) && ns != Some("http://www.w3.org/2000/xmlns/")
        || ns == Some("http://www.w3.org/2000/xmlns/") && name != "xmlns" && prefix != Some("xmlns")
    {
        return Err("NamespaceError");
    }
    Ok((prefix.map(str::to_string), local.to_string()))
}

/// „validate and extract" fuer Elemente (`createElementNS`, `createDocument`).
pub fn namespace_name(
    namespace: Option<&str>,
    name: &str,
) -> Result<(Option<String>, String), &'static str> {
    extrahieren(namespace, name, true)
}

/// „validate and extract" fuer Attribute (`setAttributeNS`,
/// `createAttributeNS`).
pub fn attribute_namespace_name(
    namespace: Option<&str>,
    name: &str,
) -> Result<(Option<String>, String), &'static str> {
    extrahieren(namespace, name, false)
}

/// DOM §1.4 „valid element local name".
pub fn element_name(name: &str) -> Result<(), &'static str> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return Err("InvalidCharacterError") };
    let ascii = first.is_ascii_alphabetic();
    if !(ascii || first == ':' || first == '_' || first as u32 >= 128)
        || !chars.all(|c| if ascii {
            !matches!(c, '\0' | '\t' | '\n' | '\x0c' | '\r' | ' ' | '/' | '>')
        } else {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '_') || c as u32 >= 128
        })
    {
        return Err("InvalidCharacterError");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gelockerte_namen() {
        assert!(namespace_name(None, "f}oo").is_ok());
        assert!(namespace_name(None, "}foo").is_err());
        assert_eq!(namespace_name(Some("u"), "f:o:o"), Ok((Some("f".into()), "o:o".into())));
        assert_eq!(namespace_name(Some("u"), ":foo"), Err("InvalidCharacterError"));
        assert_eq!(namespace_name(Some("u"), "foo:"), Err("InvalidCharacterError"));
        assert_eq!(namespace_name(Some("u"), "a:0"), Err("InvalidCharacterError"));
        assert!(namespace_name(Some("u"), "0:a").is_ok());
        assert_eq!(namespace_name(None, "f:oo"), Err("NamespaceError"));
        assert_eq!(namespace_name(None, "xmlns"), Err("NamespaceError"));
        assert!(attribute_name("invalid^Name").is_ok());
        assert!(attribute_name("").is_err());
        assert!(attribute_name("a=b").is_err());
        assert!(attribute_namespace_name(Some("u"), "p:1x").is_ok());
    }
}
