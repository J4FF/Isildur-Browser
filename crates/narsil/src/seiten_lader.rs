use crate::{Sitzung, sitzung::{Angezeigt, Fehler}};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::Instant;

pub(crate) struct Auftrag {
    pub id: u64,
    pub tab: narsil_browser::TabId,
    pub adresse: String,
    pub breite: u32,
    pub hoehe: u32,
    pub messung: narsil_layout::Messung,
    pub farbschema: narsil_css::Farbschema,
    pub einstellungen: narsil_chrome::einstellungen::Einstellungen,
    pub kein_cache: bool,
    pub start: Instant,
}

pub(crate) struct Antwort {
    pub id: u64,
    pub tab: narsil_browser::TabId,
    pub adresse: String,
    pub start: Instant,
    pub seite: Result<Angezeigt, Fehler>,
    pub ansicht: (u32, u32),
    pub netz: narsil_devtools::d4_netzwerk::NetzEreignis,
    pub zeiten: Vec<(&'static str, u64)>,
    /// Release 10.10.2026: eine Vorschau (Seite vor ihren Skripten); das
    /// Endergebnis derselben Ladung folgt.
    pub vorschau: bool,
}

pub(crate) struct Lader {
    sender: SyncSender<Auftrag>,
    empfaenger: Receiver<Antwort>,
    wartend: Option<Auftrag>,
    pub id: u64,
    pub beendet: bool,
}

impl Lader {
    pub fn neu(dienste: crate::dienste::GeminiDienste) -> std::io::Result<Self> {
        let (sender, auftraege) = mpsc::sync_channel::<Auftrag>(1);
        let (antworten, empfaenger) = mpsc::sync_channel(1);
        std::thread::Builder::new().name("narsil-seiten".into()).stack_size(32 * 1024 * 1024).spawn(move || {
            let mut sitzung = Sitzung::neu(1, 1);
            sitzung.dienste = dienste;
            while let Ok(mut auftrag) = auftraege.recv() {
                while let Ok(neuer) = auftraege.try_recv() { auftrag = neuer; }
                sitzung.breite = auftrag.breite;
                sitzung.hoehe = auftrag.hoehe;
                sitzung.setze_messung(auftrag.messung);
                sitzung.farbschema = auftrag.farbschema;
                sitzung.einstellungen = auftrag.einstellungen;
                crate::zeiten::sammel_leeren();
                // Release 10.10.2026: der Vorschau-Rueckruf dieser Ladung
                // (`Sitzung::vorschau_senden`); `try_send`, damit der Lader
                // nie an einer Vorschau haengt.
                {
                    let kanal = antworten.clone();
                    let (id, tab, adresse, start) =
                        (auftrag.id, auftrag.tab, auftrag.adresse.clone(), auftrag.start);
                    let ansicht = (auftrag.breite, auftrag.hoehe);
                    let rueckruf: Box<dyn FnMut(Angezeigt) + Send> = Box::new(move |a| {
                        let _ = kanal.try_send(Antwort {
                            id,
                            tab,
                            adresse: adresse.clone(),
                            start,
                            seite: Ok(a),
                            ansicht,
                            netz: narsil_devtools::d4_netzwerk::NetzEreignis::leer(),
                            zeiten: Vec::new(),
                            vorschau: true,
                        });
                    });
                    if let Ok(mut g) = sitzung.vorschau_senden.lock() {
                        *g = Some(rueckruf);
                    }
                }
                let mut netz = narsil_devtools::d4_netzwerk::NetzEreignis::leer();
                let seite = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sitzung.laden_mit_breite_ereignis_und_cache(&auftrag.adresse, auftrag.breite,
                        auftrag.hoehe, &mut netz, auftrag.kein_cache)
                })).unwrap_or_else(|_| Err(Fehler::Seite(narsil_i18n::tr("Page loader failed").into())));
                if let Ok(mut g) = sitzung.vorschau_senden.lock() {
                    *g = None;
                }
                let antwort = Antwort { id: auftrag.id, tab: auftrag.tab, adresse: auftrag.adresse,
                    start: auftrag.start, seite, ansicht: (auftrag.breite, auftrag.hoehe), netz, zeiten: crate::zeiten::gesammelt(),
                    vorschau: false };
                if antworten.send(antwort).is_err() { break; }
            }
        })?;
        Ok(Self { sender, empfaenger, wartend: None, id: 0, beendet: false })
    }

    pub fn verwerfen(&mut self) {
        self.id = self.id.wrapping_add(1);
        self.wartend = None;
    }

    pub fn laden(&mut self, mut auftrag: Auftrag) -> Result<(), Fehler> {
        self.verwerfen();
        auftrag.id = self.id;
        self.senden(auftrag)
    }

    fn senden(&mut self, auftrag: Auftrag) -> Result<(), Fehler> {
        match self.sender.try_send(auftrag) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(a)) => { self.wartend = Some(a); Ok(()) },
            Err(TrySendError::Disconnected(_)) => { self.beendet = true; Err(Fehler::Seite(narsil_i18n::tr("Page loader ended").into())) },
        }
    }

    pub fn abholen(&mut self) -> Option<Antwort> {
        if let Some(a) = self.wartend.take() { let _ = self.senden(a); }
        match self.empfaenger.try_recv() {
            Ok(a) => Some(a),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => { self.beendet = true; None },
        }
    }
}
