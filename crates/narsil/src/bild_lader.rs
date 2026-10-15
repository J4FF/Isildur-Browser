//! Begrenzte Lazy-Bildladung; Netz und Decoder laufen ausserhalb des UI-Takts.
use std::sync::{Arc, mpsc::{self, Receiver, SyncSender}};
use crate::bilder::{BildCache, BildFehler, GeladenesBild};

pub(crate) const MAX_LADUNGEN: usize = 4;
struct Auftrag { seite: Arc<Vec<narsil_fmt_html::HtmlNode>>, quellen: Vec<String>, originale: bool }
pub(crate) struct Antwort {
    pub seite: Arc<Vec<narsil_fmt_html::HtmlNode>>,
    /// Release 08.10.2026 (Bild-Diaet): dazu die Originalbytes, wenn der
    /// Auftrag sie verlangt hat (daraus wird spaeter neu dekodiert).
    pub bilder: Vec<(String, Result<GeladenesBild, BildFehler>, Option<Arc<Vec<u8>>>)>,
}
pub(crate) struct Lader {
    sender: SyncSender<Auftrag>,
    empfaenger: Receiver<Antwort>,
    pub laeuft: bool,
}
impl Lader {
    pub fn neu(dienste: crate::dienste::GeminiDienste) -> std::io::Result<Self> {
        let (sender, auftraege) = mpsc::sync_channel::<Auftrag>(1);
        let (fertig, empfaenger) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("narsil-bilder".into()).stack_size(8 * 1024 * 1024).spawn(move || {
            while let Ok(auftrag) = auftraege.recv() {
                let mut bilder = Vec::new();
                for q in auftrag.quellen {
                    let ergebnis = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if matches!(dienste.evaluate_request(&q, "image"), narsil_ext::dnr::ActionType::Block) {
                            return (Err(BildFehler::Blockiert), None);
                        }
                        // Der bestehende Abruf behaelt Adblock/CSP/Mixed-Content,
                        // Dateigroessen- und Decodergrenzen unveraendert bei.
                        let mut cache = BildCache::neu();
                        cache.originale_behalten = auftrag.originale;
                        let bild = cache.hole(&q).cloned();
                        (bild, cache.original(&q))
                    })).unwrap_or_else(|_| (Err(BildFehler::Format("Bildnachlader fehlgeschlagen".into())), None));
                    bilder.push((q, ergebnis.0, ergebnis.1));
                }
                if fertig.send(Antwort { seite: auftrag.seite, bilder }).is_err() { break; }
            }
        })?;
        Ok(Self { sender, empfaenger, laeuft: false })
    }
    pub fn starten(&mut self, seite: Arc<Vec<narsil_fmt_html::HtmlNode>>, quellen: Vec<String>, originale: bool) -> bool {
        if self.laeuft || quellen.is_empty() { return false; }
        if self.sender.try_send(Auftrag { seite, quellen, originale }).is_err() { return false; }
        self.laeuft = true;
        true
    }
    pub fn abholen(&mut self) -> Option<Antwort> {
        match self.empfaenger.try_recv() {
            Ok(a) => { self.laeuft = false; Some(a) }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => { self.laeuft = false; None }
        }
    }
}
