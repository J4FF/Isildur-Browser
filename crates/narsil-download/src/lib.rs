pub mod ausloeser;
pub mod content_disposition;
pub mod lauf;
pub mod liste;
pub mod manager;
pub mod media;
pub mod name;
pub mod oeffnen;
pub mod plan;
pub mod sicher;

pub use manager::{DownloadManager, DownloadState};
pub use media::{MediaSource, find_media_source};

// Und die **zwei** Typen, die ein Aufrufer braucht, ohne den Modulpfad zu
// kennen — GEMESSENE ENTSCHEIDUNG, warum **nicht** `pub use liste::*`: ein
// `*` nimmt auch `Plan` mit, und `Plan` gehoert **nicht** an die Anzeige.
pub use liste::{Eintrag, Liste, Zustand};
