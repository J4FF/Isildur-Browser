pub mod pdf;
use std::process::Command;
use std::io::Write;
use narsil_image::Bild;

pub fn list_printers() -> Result<Vec<String>, String> {
    let output = Command::new("lpstat")
        .arg("-a")
        .output()
        .map_err(|e| format!("Fehler beim Aufruf von lpstat: {}", e))?;
        
    if !output.status.success() {
        return Err("lpstat fehlgeschlagen. Ist CUPS installiert?".to_string());
    }
    
    let out_str = String::from_utf8_lossy(&output.stdout);
    let mut printers = Vec::new();
    for line in out_str.lines() {
        if let Some(printer) = line.split_whitespace().next() {
            printers.push(printer.to_string());
        }
    }
    
    if printers.is_empty() {
        Err("Keine Drucker gefunden.".to_string())
    } else {
        Ok(printers)
    }
}

pub fn print_document(printer: &str, pages: &[(Option<&Bild>, &str)]) -> Result<(), String> {
    let mut writer = pdf::PdfWriter::new();
    for (img, text) in pages {
        writer.add_page(*img, text);
    }
    let pdf_data = writer.finish();
    
    let mut child = Command::new("lp")
        .arg("-d")
        .arg(printer)
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Fehler beim Aufruf von lp: {}", e))?;
        
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&pdf_data).map_err(|e| format!("Fehler beim Senden der PDF-Daten: {}", e))?;
    }
    
    let status = child.wait().map_err(|e| format!("Fehler beim Warten auf lp: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("lp beendete sich mit Fehlercode: {:?}", status.code()))
    }
}
