//! TLS 1.2 — die Handshake-Nachrichten (RFC 5246 Abschnitt 7).
//!
//! added 30.09.2026, als dritter Teil von T15.
//!
//! # Warum eine eigene Datei und nicht `nachricht.rs` (TLS 1.3)
//!
//! Die Nachrichtentypen sind dieselben Nummern, aber die **Kodierung ist
//! es nicht**:
//!
//! * TLS 1.3 hat kein `CertificateRequest` im Flug, und `ClientHello`
//!   traegt `supported_versions` statt `elliptic_curves`.
//! * `ServerKeyExchange` und `ClientKeyExchange` gibt es in TLS 1.3
//!   ueberhaupt nicht — das Schluesselmaterial kommt aus dem
//!   `key_share`.
//! * Die Laengen sind in 1.2 eine **dreistellige** Zahl in allen
//!   Koerpern, in 1.3 eine vierstellige an manchen Stellen.
//!
//! Wer die 1.3-Routinen hier aufruft, erhaelt Bytes, die ein Server
//! zurueckweist, und der Fehler ist nicht auffaellig.

use std::fmt;

/// Nachrichtentypen (RFC 5246 Abschnitt 7.4).
pub mod typen {
    pub const CLIENT_HELLO: u8 = 1;
    pub const SERVER_HELLO: u8 = 2;
    pub const CERTIFICATE: u8 = 11;
    pub const SERVER_KEY_EXCHANGE: u8 = 12;
    pub const CERTIFICATE_REQUEST: u8 = 13;
    pub const SERVER_HELLO_DONE: u8 = 14;
    pub const CERTIFICATE_VERIFY: u8 = 15;
    pub const CLIENT_KEY_EXCHANGE: u8 = 16;
    pub const FINISHED: u8 = 20;
    /// added 01.10.2026 22:1x UTC (Projektlabel 02.10.2026), S4. `CertificateStatus` (RFC 6066 3),
    /// Nummer **22**.
    ///
    /// GEMESSEN, warum das fehlte und was es kostete: `xkcd.com`,
    /// `www.netbsd.org` und `www.opensuse.org` (alle TLS 1.2) scheiterten am
    /// 02.10.2026 mit „TLS 1.2: Nachricht 14 erwartet, 22 bekommen". Wir
    /// bieten `status_request` an (das stand seit 30.09.2026 im
    /// TLS-1.2-ClientHello, `nachricht12.rs` Zeile 348), und der Server
    /// antwortet darauf — genau nach RFC 6066 8: „If the client sends a
    /// status_request extension, then the server MAY optionally return a
    /// single CertificateStatus message **immediately following the
    /// Certificate message**".
    pub const CERTIFICATE_STATUS: u8 = 22;
}

/// Erweiterungstypen, die hier vorkommen.
pub mod erweiterung {
    pub const SERVER_NAME: u16 = 0;
    pub const STATUS_REQUEST: u16 = 5;
    pub const SUPPORTED_GROUPS: u16 = 10;
    pub const EC_POINT_FORMATS: u16 = 11;
    pub const SIGNATURE_ALGORITHMS: u16 = 13;
    pub const ALPN: u16 = 16;
    pub const EXTENDED_MASTER_SECRET: u16 = 23;
    pub const SESSION_TICKET: u16 = 35;
    pub const RENEGOTIATION_INFO: u16 = 65281;
}

/// Die Diffie-Hellman-Gruppen, die dieser Auftrag nennt.
///
/// x25519 (0x001D, in RFC 8446 benannt) und secp256r1 (0x0017).
pub mod gruppe {
    pub const SECP256R1: u16 = 23;
    pub const X25519: u16 = 29;
}

/// Signaturverfahren (RFC 5246 Abschnitt 7.4.1.4.1 und die PSS-Verfahren
/// aus RFC 8446, die auch in 1.2 erlaubt sind).
///
/// DIE REIHENFOLGE IST BEDEUTUNGSLOS, die Vollstaendigkeit nicht: ein
/// Server, dessen bevorzugtes Verfahren nicht in dieser Liste steht,
/// schickt kein `ServerKeyExchange` mit ECDHE, sondern bricht ab oder
/// faellt auf eine Suite ohne ECDHE zurueck — und der Client, der nur
/// RSA kann, schickt dann ein Client-Zertifikat, das der Server nicht
/// erwartet.
pub mod sigverfahren {
    pub const RSA_PKCS1_SHA256: u16 = 0x0401;
    pub const RSA_PKCS1_SHA384: u16 = 0x0501;
    pub const RSA_PKCS1_SHA512: u16 = 0x0601;
    pub const ECDSA_SECP256R1_SHA256: u16 = 0x0403;
    pub const ECDSA_SECP384R1_SHA384: u16 = 0x0503;
    pub const RSA_PSS_RSAE_SHA256: u16 = 0x0804;
    pub const RSA_PSS_RSAE_SHA384: u16 = 0x0805;
    pub const RSA_PSS_RSAE_SHA512: u16 = 0x0806;
}

/// M3: die Obergrenze einer Handshake-Nachricht.
///
/// RFC 5246 Abschnitt 6.2.3.1: ein `certificate` mit einer Kette darf
/// nicht laenger sein als 2^24 Byte; alles darunter ist Auslegung.
/// 65536 ist gross genug fuer jede Kette, die ein Server schickt, und
/// begrenzt die Allokation auf einen Wert, der aus der Laenge im Kopf
/// folgt, den der Server selbst schreibt — also aus dem Netz.
pub const MAX_NACHRICHT: usize = 65536;

/// M3: die Obergrenze fuer die Liste der Zertifikate in einer
/// `Certificate`-Nachricht. 16 Stueck sind reichlich; ein Browser
/// akzeptiert auch nicht mehr.
pub const MAX_ZERTIFIKATE: usize = 16;

/// Fehler beim Lesen oder Bauen einer TLS-1.2-Nachricht.
///
/// Jede Variante nennt die Stelle, an der es liegen muss. "unbekannt"
/// ohne Offset ist bei einer Nachricht aus dem Netz keine Information.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Nachricht12Fehler {
    /// Die Laenge im Kopf passt nicht zu dem, was danach steht.
    Laenge { typ: u8, gewollt: usize, war: usize },
    /// Unerwarteter Nachrichtentyp.
    Typ { erwartet: u8, bekommen: u8 },
    /// Falsche Feldlaenge, zum Beispiel ein 3-Byte-Int wo 2 stehen.
    Feld {
        name: &'static str,
        braucht: usize,
        war: usize,
    },
    /// Die Liste ist laenger als erlaubt (M3/M4).
    ZuViele { grenze: usize, was: &'static str },
    /// Der Server nennt eine Suite, die nicht in meiner Liste steht.
    SuiteUnbekannt { code: u16 },
    /// Der Server nennt eine Gruppe, die ich nicht rechnen kann.
    GruppeUnbekannt { code: u16 },
    /// Der Server verlangt eine Erweiterung, die ich brauche.
    ErweiterungFehlt { code: u16 },
    /// Die SNI-Erweiterung war da, aber ohne Hostnamen.
    SniLeer,
    /// Falsches Signaturverfahren fuer die Suite.
    SignaturUnbekannt { code: u16 },
    /// Restbytes, die keiner Nachricht zugehoeren.
    UnbekannteErweiterung { code: u16 },
}

impl fmt::Display for Nachricht12Fehler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Nachricht12Fehler::Laenge { typ, gewollt, war } => {
                write!(
                    f,
                    "Nachricht {typ}: Laenge {gewollt} im Kopf, aber {war} Byte"
                )
            }
            Nachricht12Fehler::Typ { erwartet, bekommen } => {
                write!(f, "Nachricht {erwartet} erwartet, {bekommen} bekommen")
            }
            Nachricht12Fehler::Feld { name, braucht, war } => {
                write!(f, "Feld {name}: {braucht} Byte noetig, {war} bekommen")
            }
            Nachricht12Fehler::ZuViele { grenze, was } => {
                write!(f, "zu viele {was}: ueber {grenze}")
            }
            Nachricht12Fehler::SuiteUnbekannt { code } => {
                write!(f, "unbekannte Suite 0x{code:04x}")
            }
            Nachricht12Fehler::GruppeUnbekannt { code } => {
                write!(f, "unbekannte Gruppe {code}")
            }
            Nachricht12Fehler::ErweiterungFehlt { code } => {
                write!(f, "der Server nennt die Erweiterung {code} nicht")
            }
            Nachricht12Fehler::SniLeer => write!(f, "die SNI-Erweiterung ist ohne Hostnamen"),
            Nachricht12Fehler::SignaturUnbekannt { code } => {
                write!(f, "unbekanntes Signaturverfahren 0x{code:04x}")
            }
            Nachricht12Fehler::UnbekannteErweiterung { code } => {
                write!(f, "unbekannte Erweiterung {code}")
            }
        }
    }
}

/// Ein Leser fuer die TLS-1.2-Strukturen.
///
/// M3/M4: Jedes `nimm` prueft die verlangte Laenge, BEVOR es weiterliest.
/// Es gibt keinen Weg, eine Zahl zu lesen und danach zu fragen, ob
/// genug da war.
pub struct Leser<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Leser<'a> {
    pub fn neu(d: &'a [u8]) -> Self {
        Leser { d, p: 0 }
    }

    /// Wie viele Byte sind noch da?
    pub fn rest(&self) -> usize {
        self.d.len() - self.p
    }

    /// Ist nichts mehr da?
    pub fn leer(&self) -> bool {
        self.rest() == 0
    }

    /// Liest `n` Byte. Fehler, wenn weniger da sind.
    pub fn nimm(&mut self, n: usize, name: &'static str) -> Result<&'a [u8], Nachricht12Fehler> {
        if self.rest() < n {
            return Err(Nachricht12Fehler::Feld {
                name,
                braucht: n,
                war: self.rest(),
            });
        }
        let raus = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(raus)
    }

    /// Liest eine Ein-Byte-Zahl.
    pub fn u8(&mut self, name: &'static str) -> Result<u8, Nachricht12Fehler> {
        Ok(self.nimm(1, name)?[0])
    }

    /// Liest eine Zwei-Byte-Zahl.
    pub fn u16(&mut self, name: &'static str) -> Result<u16, Nachricht12Fehler> {
        let b = self.nimm(2, name)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    /// Liest eine Drei-Byte-Zahl. Der Kopf jeder TLS-1.2-Nachricht.
    pub fn u24(&mut self, name: &'static str) -> Result<usize, Nachricht12Fehler> {
        let b = self.nimm(3, name)?;
        Ok(((b[0] as usize) << 16) | ((b[1] as usize) << 8) | b[2] as usize)
    }

    /// Liest ein Byte-Feld mit einem Byte als Laengenprefix.
    pub fn feld8(&mut self, name: &'static str) -> Result<&'a [u8], Nachricht12Fehler> {
        let n = self.u8(name)? as usize;
        self.nimm(n, name)
    }

    /// Liest ein Byte-Feld mit zwei Byte als Laengenprefix.
    pub fn feld16(&mut self, name: &'static str) -> Result<&'a [u8], Nachricht12Fehler> {
        let n = self.u16(name)? as usize;
        self.nimm(n, name)
    }

    /// Liest ein Byte-Feld mit drei Byte als Laengenprefix.
    ///
    /// Das ist die Form der Zertifikate in der `Certificate`-Nachricht
    /// (RFC 5246 Abschnitt 7.4.2). Zwei Byte sind zu kurz: die
    /// Kettenliste selbst ist ein `uint24`.
    pub fn feld24_als_bytes(&mut self, name: &'static str) -> Result<&'a [u8], Nachricht12Fehler> {
        let n = self.u24(name)?;
        self.nimm(n, name)
    }

    /// Wie viele Bytes sind noch nicht gelesen?
    pub fn unlesbar(&self) -> &'a [u8] {
        &self.d[self.p..]
    }
}

/// Der ClientHello.
///
/// Der Aufbau folgt RFC 5246 Abschnitt 7.4.1.2. Pflicht sind
/// `client_version` = TLS 1.2, ein 32-Byte-Random und die
/// Kompressionsmethode `null`.
pub struct ClientHello;

/// Der ServerHello, wie ihn die Crate liest.
#[derive(Clone, Debug)]
pub struct ServerHello {
    pub version: u16,
    pub random: [u8; 32],
    pub session_id: Vec<u8>,
    pub cipher_suite: u16,
    pub kompression: u8,
    /// Die Erweiterungen, die der Server geschickt hat.
    pub erweiterungen: Vec<(u16, Vec<u8>)>,
}

impl ServerHello {
    /// Hat der Server eine Erweiterung geschickt?
    pub fn hat(&self, code: u16) -> bool {
        self.erweiterungen.iter().any(|(c, _)| *c == code)
    }

    /// Der Inhalt einer Erweiterung, falls sie da ist.
    pub fn hole(&self, code: u16) -> Option<&[u8]> {
        self.erweiterungen
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, d)| d.as_slice())
    }
}

impl ClientHello {
    /// Baut das ClientHello.
    ///
    /// `host` geht in die SNI-Erweiterung. Ohne SNI nimmt der groesste
    /// Teil der Server eine Default-Zertifikat fuer ihre eigene
    /// Domain — und `example.com` und `www.example.com` haben
    /// verschiedene.
    ///
    /// M4: die Suite- und Gruppenliste sind feste Konstanten dieses
    /// Auftrags, also keine Eingabegroesse. `alpn` kommt vom Aufrufer
    /// und ist durch `MAX_ALPN_EINTRAEGE` begrenzt.
    pub fn baue(
        random: &[u8; 32],
        host: &str,
        gruppen: &[u16],
        suites: &[u16],
        alpn: &[&[u8]],
        sigverfahren: &[u16],
    ) -> Result<Vec<u8>, Nachricht12Fehler> {
        // M3/M4: der Hostname und die ALPN-Namen kommen vom Aufrufer.
        if host.len() > 255 {
            return Err(Nachricht12Fehler::Feld {
                name: "server_name",
                braucht: 0,
                war: host.len(),
            });
        }
        if alpn.len() > MAX_ALPN_EINTRAEGE {
            return Err(Nachricht12Fehler::ZuViele {
                grenze: MAX_ALPN_EINTRAEGE,
                was: "ALPN-Eintraege",
            });
        }
        for a in alpn {
            if a.len() > 255 {
                return Err(Nachricht12Fehler::Feld {
                    name: "alpn",
                    braucht: 0,
                    war: a.len(),
                });
            }
        }
        // M4: alles ist bekannt, also wird exakt so viel reserviert, wie
        // gebraucht wird — plus Kopfschreiben in einem Rutsch.
        let mut koerper: Vec<u8> = Vec::with_capacity(256);
        koerper.extend_from_slice(&[0x03, 0x03]); // client_version: TLS 1.2
        koerper.extend_from_slice(random);
        koerper.push(0); // session_id: leer. Neuverhandlung, keine Sitzung.
        // cipher_suites
        koerper.extend_from_slice(&((suites.len() * 2) as u16).to_be_bytes());
        for s in suites {
            koerper.extend_from_slice(&s.to_be_bytes());
        }
        // compression_methods: nur null
        koerper.extend_from_slice(&[1, 0]);
        // extensions
        koerper.extend_from_slice(&[0, 0]); // Platzhalter, wird unten gefuellt
        let mut ext: Vec<u8> = Vec::with_capacity(128);

        // server_name
        let mut sni: Vec<u8> = Vec::with_capacity(host.len() + 5);
        sni.extend_from_slice(&((host.len() + 3) as u16).to_be_bytes());
        sni.push(0); // host_name
        sni.extend_from_slice(&(host.len() as u16).to_be_bytes());
        sni.extend_from_slice(host.as_bytes());
        ext.extend_from_slice(&erweiterung::SERVER_NAME.to_be_bytes());
        ext.extend_from_slice(&(sni.len() as u16).to_be_bytes());
        ext.extend_from_slice(&sni);

        // `status_request` (RFC 6066 Abschnitt 3).
        //
        // DER FEHLER, DEN DIESE ZEILE HATTE: die Erweiterung war EIN
        // Byte lang (`00`). Sie braucht fuenf:
        //   status_type      1 Byte   = 1 (ocsp)
        //   responder_id_list 2 Byte  = 0 (leer)
        //   request_extensions 2 Byte = 0 (leer)
        // Badssl.com (Cloudflare) beantwortete den ClientHello mit
        // einem Alert Stufe 2, Beschreibung 32 und schloss die
        // Verbindung. Die Suche nach der Ursache hat gedauert, weil
        // derselbe Alert auch bei einem zweiten, harmlosen Fehler
        // auftrat und damit erst als "Server ist eigenwillig"
        // gelesen wurde.
        //
        // GEFUNDEN DURCH HALBIERUNG, nicht durch Lesen: 1 Byte ->
        // Alert, 5 Byte -> ServerHello. Beides gegen dieselbe Adresse
        // zur selben Zeit (30.09.2026, 13:20 UTC).
        ext.extend_from_slice(&erweiterung::STATUS_REQUEST.to_be_bytes());
        ext.extend_from_slice(&5u16.to_be_bytes());
        ext.extend_from_slice(&[1, 0, 0, 0, 0]);

        // supported_groups
        ext.extend_from_slice(&erweiterung::SUPPORTED_GROUPS.to_be_bytes());
        ext.extend_from_slice(&((gruppen.len() * 2) as u16 + 2).to_be_bytes());
        ext.extend_from_slice(&((gruppen.len() * 2) as u16).to_be_bytes());
        for g in gruppen {
            ext.extend_from_slice(&g.to_be_bytes());
        }

        // ec_point_formats: nur uncompressed
        ext.extend_from_slice(&erweiterung::EC_POINT_FORMATS.to_be_bytes());
        ext.extend_from_slice(&2u16.to_be_bytes());
        ext.push(1);
        ext.push(0);

        // signature_algorithms
        ext.extend_from_slice(&erweiterung::SIGNATURE_ALGORITHMS.to_be_bytes());
        ext.extend_from_slice(&((sigverfahren.len() * 2) as u16 + 2).to_be_bytes());
        ext.extend_from_slice(&((sigverfahren.len() * 2) as u16).to_be_bytes());
        for s in sigverfahren {
            ext.extend_from_slice(&s.to_be_bytes());
        }

        // ALPN
        if !alpn.is_empty() {
            let mut inhalt: Vec<u8> = Vec::with_capacity(alpn.len() * 8);
            for a in alpn {
                inhalt.push(a.len() as u8);
                inhalt.extend_from_slice(a);
            }
            ext.extend_from_slice(&erweiterung::ALPN.to_be_bytes());
            ext.extend_from_slice(&((inhalt.len() + 2) as u16).to_be_bytes());
            ext.extend_from_slice(&(inhalt.len() as u16).to_be_bytes());
            ext.extend_from_slice(&inhalt);
        }

        // extended_master_secret: RFC 7627. OHNE diese Erweiterung
        // lehnt RFC 7627 Abschnitt 4 den Handshake ab, und sie ist
        // billiger zu fordern als zu erklaeren, warum sie fehlt.
        ext.extend_from_slice(&erweiterung::EXTENDED_MASTER_SECRET.to_be_bytes());
        ext.extend_from_slice(&0u16.to_be_bytes());

        // renegotiation_info, leer
        ext.extend_from_slice(&erweiterung::RENEGOTIATION_INFO.to_be_bytes());
        ext.extend_from_slice(&1u16.to_be_bytes());
        ext.push(0);

        // session_ticket, leer
        ext.extend_from_slice(&erweiterung::SESSION_TICKET.to_be_bytes());
        ext.extend_from_slice(&0u16.to_be_bytes());

        // Der Platzhalter fuer die Erweiterungslaenge stand ganz hinten
        // im Koerper. Seine Position wird VOR der Indexierung in eine
        // eigene Zahl gelegt: `koerper[koerper.len() - 2..]` leiht
        // `koerper` zweimal, und der Compiler laesst das nicht zu.
        let laenge = ext.len();
        let stelle = koerper.len() - 2;
        koerper[stelle..stelle + 2].copy_from_slice(&(laenge as u16).to_be_bytes());
        koerper.extend_from_slice(&ext);

        let mut raus: Vec<u8> = Vec::with_capacity(4 + koerper.len());
        raus.push(typen::CLIENT_HELLO);
        raus.push((koerper.len() >> 16) as u8);
        raus.push((koerper.len() >> 8) as u8);
        raus.push(koerper.len() as u8);
        raus.extend_from_slice(&koerper);
        Ok(raus)
    }
}

/// M3: hoechstens so viele ALPN-Namen.
pub const MAX_ALPN_EINTRAEGE: usize = 8;

/// Der ServerHello, gelesen.
pub fn server_hello_dekodiere(d: &[u8]) -> Result<ServerHello, Nachricht12Fehler> {
    if d.is_empty() {
        return Err(Nachricht12Fehler::Feld {
            name: "server_hello",
            braucht: 1,
            war: 0,
        });
    }
    if d[0] != typen::SERVER_HELLO {
        return Err(Nachricht12Fehler::Typ {
            erwartet: typen::SERVER_HELLO,
            bekommen: d[0],
        });
    }
    let mut l = Leser::neu(d);
    l.nimm(1, "typ")?;
    let n = l.u24("laenge")?;
    let koerper = l.nimm(n, "koerper")?;
    if koerper.len() != n {
        return Err(Nachricht12Fehler::Laenge {
            typ: typen::SERVER_HELLO,
            gewollt: n,
            war: koerper.len(),
        });
    }
    let mut k = Leser::neu(koerper);
    let version = k.u16("version")?;
    // added 02.10.2026 03:6x UTC (Projektlabel 02.10.2026), Rolle TLS: der
    // letzte `expect` im Produktivteil dieser Datei. `k.nimm(32, "random")`
    // gibt **garantiert** 32 Byte zurueck (sonst waere es schon mit `?`
    // abgebrochen), deshalb ist der Fall unerreichbar — und wird trotzdem
    // benannt statt behauptet, mit `Feld`, weil das die Form ist, die
    // `Leser::nimm` selbst benutzt. So liest sich die Zeile wie die
    // Nachbarzeilen darueber.
    // added 02.10.2026 03:6x: GEMESSEN, warum `.len()` hier nicht reicht —
    // der Uebersetzer meldete „expected `Vec<u8>`, found `TryFromSliceError`".
    // `k.nimm` liefert `&[u8]`, und `try_into` auf `&[u8]` nennt im Fehler
    // **keine** Laenge; also wird die Laenge vorher gemessen und der Fehler
    // trotzdem benannt.
    let zufall = k.nimm(32, "random")?;
    if zufall.len() != 32 {
        return Err(Nachricht12Fehler::Feld {
            name: "random",
            braucht: 32,
            war: zufall.len(),
        });
    }
    let mut random = [0u8; 32];
    random.copy_from_slice(zufall);
    let session_id = k.feld8("session_id")?.to_vec();
    let cipher_suite = k.u16("cipher_suite")?;
    let kompression = k.u8("kompression")?;
    let mut erweiterungen = Vec::new();
    if k.rest() >= 2 {
        let gesamt = k.u16("erweiterungen")? as usize;
        if gesamt > k.rest() {
            return Err(Nachricht12Fehler::Laenge {
                typ: typen::SERVER_HELLO,
                gewollt: gesamt,
                war: k.rest(),
            });
        }
        // `k` leiht `koerper`; `unlesbar()` leiht daraus. Deshalb
        // erst die Restlaenge in eine eigene Zahl, dann der Leser.
        let rest_roh: Vec<u8> = k.unlesbar()[..gesamt].to_vec();
        let mut e = Leser::neu(&rest_roh);
        while !e.leer() {
            let code = e.u16("erweiterung")?;
            let inhalt = e.feld16("erweiterungsdaten")?.to_vec();
            erweiterungen.push((code, inhalt));
        }
    }
    Ok(ServerHello {
        version,
        random,
        session_id,
        cipher_suite,
        kompression,
        erweiterungen,
    })
}

/// Ein `ServerKeyExchange` fuer ECDHE.
#[derive(Clone, Debug)]
pub struct ServerKeyExchange {
    /// `named_curve` aus der Nachricht.
    pub gruppe: u16,
    /// Der oeffentliche Punkt, 65 Byte (uncompressed) bei P-256.
    pub punkt: Vec<u8>,
    pub sigverfahren: u16,
    pub signatur: Vec<u8>,
}

/// Der Inhalt, ueber den die Signatur geht.
///
/// RFC 5246 Abschnitt 7.4.3: bei ECDHE sind das `client_random`,
/// `server_random` und der `ServerKeyExchange`-Parameterschnitt OHNE
/// die Signatur. Genau dieses Stueck wird signiert — und es ist nicht
/// die Nachricht, wie sie im Record steht.
pub fn ske_signaturinhalt(cr: &[u8], sr: &[u8], params: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(64 + params.len());
    v.extend_from_slice(cr);
    v.extend_from_slice(sr);
    v.extend_from_slice(params);
    v
}

/// Liest das `ServerKeyExchange` fuer ECDHE (und lehnt alles andere ab).
pub fn ske_dekodiere(d: &[u8]) -> Result<ServerKeyExchange, Nachricht12Fehler> {
    if d.first() != Some(&typen::SERVER_KEY_EXCHANGE) {
        return Err(Nachricht12Fehler::Typ {
            erwartet: typen::SERVER_KEY_EXCHANGE,
            bekommen: *d.first().unwrap_or(&0),
        });
    }
    let mut l = Leser::neu(d);
    l.nimm(1, "typ")?;
    let n = l.u24("laenge")?;
    let koerper = l.nimm(n, "koerper")?;
    let mut k = Leser::neu(koerper);
    // curve_type: 3 = named_curve
    if k.u8("curve_type")? != 3 {
        return Err(Nachricht12Fehler::Feld {
            name: "curve_type (nur 3)",
            braucht: 3,
            war: 0,
        });
    }
    let gruppe = k.u16("named_curve")?;
    let punkt = k.feld8("point")?.to_vec();
    let sigverfahren = k.u16("signaturverfahren")?;
    let signatur = k.feld16("signatur")?.to_vec();
    Ok(ServerKeyExchange {
        gruppe,
        punkt,
        sigverfahren,
        signatur,
    })
}

/// Der Parameterschnitt des `ServerKeyExchange` ohne die Signatur.
pub fn ske_parameter(ske: &ServerKeyExchange) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(4 + ske.punkt.len());
    v.push(3);
    v.extend_from_slice(&ske.gruppe.to_be_bytes());
    v.push(ske.punkt.len() as u8);
    v.extend_from_slice(&ske.punkt);
    v
}

/// Baut das `ClientKeyExchange` fuer ECDHE.
///
/// RFC 5246 Abschnitt 7.4.7: der Koerper ist der oeffentliche Punkt,
/// bei P-256 ein Byte Laenge plus 65 Byte.
pub fn cke_baue(punkt: &[u8]) -> Result<Vec<u8>, Nachricht12Fehler> {
    if punkt.len() > 255 {
        return Err(Nachricht12Fehler::Feld {
            name: "ecdh_client_public",
            braucht: 0,
            war: punkt.len(),
        });
    }
    let koerper_len = 1 + punkt.len();
    let mut raus: Vec<u8> = Vec::with_capacity(4 + koerper_len);
    raus.push(typen::CLIENT_KEY_EXCHANGE);
    raus.push((koerper_len >> 16) as u8);
    raus.push((koerper_len >> 8) as u8);
    raus.push(koerper_len as u8);
    raus.push(punkt.len() as u8);
    raus.extend_from_slice(punkt);
    Ok(raus)
}

/// Baut ein `finished`: `Typ || Laenge(3) || verify_data(12)`.
pub fn finished_baue(verify_data: &[u8]) -> Result<Vec<u8>, Nachricht12Fehler> {
    if verify_data.len() != crate::tls12::FINISHED_LAENGE {
        return Err(Nachricht12Fehler::Feld {
            name: "verify_data",
            braucht: crate::tls12::FINISHED_LAENGE,
            war: verify_data.len(),
        });
    }
    let mut raus: Vec<u8> = Vec::with_capacity(16);
    raus.push(typen::FINISHED);
    raus.extend_from_slice(&(verify_data.len() as u32).to_be_bytes()[1..]);
    raus.extend_from_slice(verify_data);
    Ok(raus)
}

/// Liest ein `finished` und gibt die 12 Byte zurueck.
pub fn finished_dekodiere(d: &[u8]) -> Result<Vec<u8>, Nachricht12Fehler> {
    if d.first() != Some(&typen::FINISHED) {
        return Err(Nachricht12Fehler::Typ {
            erwartet: typen::FINISHED,
            bekommen: *d.first().unwrap_or(&0),
        });
    }
    let mut l = Leser::neu(d);
    l.nimm(1, "typ")?;
    let n = l.u24("laenge")?;
    let v = l.nimm(n, "verify_data")?;
    if v.len() != crate::tls12::FINISHED_LAENGE {
        return Err(Nachricht12Fehler::Feld {
            name: "verify_data",
            braucht: crate::tls12::FINISHED_LAENGE,
            war: v.len(),
        });
    }
    Ok(v.to_vec())
}

/// Liest die Zertifikate aus einer `Certificate`-Nachricht.
///
/// RFC 5246 Abschnitt 7.4.2:
/// ```text
/// struct { ASN.1Cert certificate_list<0..2^24-1>; } Certificate;
/// ```
/// `certificate_list` traegt ein `uint24` als **Laengenprefix**, und
/// JEDES Element wieder eins.
///
/// DER FEHLER, DEN DIESE FUNKTION VERHINDERT: die erste Zahl wurde
/// lange als ANZAHL gelesen. Fuer einen echten Server ist die Zahl
/// 787, und die Meldung lautete daraufhin "zu viele Zertifikate:
/// ueber 8" — ein Kettenproblem, wo es ein Formfehler war. Die
/// Bytezahl ist eine Laenge.
///
/// M3/M4: die Laenge wird VOR der Schleife geprueft: sie steht im
/// Kopf, kommt aus dem Netz, und ohne diese Pruefung wuerde hier nach
/// einer 16-MiB-Zahl alloziert. `MAX_ZERTIFIKATE` begrenzt zusaetzlich
/// die Stueckzahl, weil eine Kette mit 20 Zertifikaten ein
/// DoS-Mittel und keine Kette ist.
pub fn zertifikatliste_lesen(nachricht: &[u8]) -> Result<Vec<Vec<u8>>, Nachricht12Fehler> {
    if nachricht.first() != Some(&typen::CERTIFICATE) {
        return Err(Nachricht12Fehler::Typ {
            erwartet: typen::CERTIFICATE,
            bekommen: *nachricht.first().unwrap_or(&0),
        });
    }
    let mut l = Leser::neu(nachricht);
    l.nimm(1, "typ")?;
    let n = l.u24("laenge")?;
    let koerper = l.nimm(n, "koerper")?;
    let mut k = Leser::neu(koerper);
    let gesamt = k.u24("zertifikatliste")?;
    if gesamt == 0 || gesamt > k.rest() || gesamt > MAX_NACHRICHT {
        return Err(Nachricht12Fehler::Laenge {
            typ: typen::CERTIFICATE,
            gewollt: gesamt,
            war: k.rest(),
        });
    }
    let mut raus: Vec<Vec<u8>> = Vec::with_capacity(4);
    let mut gelesen = 0usize;
    while gelesen < gesamt {
        if raus.len() >= MAX_ZERTIFIKATE {
            return Err(Nachricht12Fehler::ZuViele {
                grenze: MAX_ZERTIFIKATE,
                was: "Zertifikate",
            });
        }
        let z = k.feld24_als_bytes("zertifikat")?;
        if z.len() > crate::x509::MAX_ZERTIFIKAT {
            return Err(Nachricht12Fehler::Feld {
                name: "zertifikat",
                braucht: crate::x509::MAX_ZERTIFIKAT,
                war: z.len(),
            });
        }
        gelesen += 3 + z.len();
        raus.push(z.to_vec());
    }
    Ok(raus)
}

/// Der ContentType von `change_cipher_spec`, das seinen eigenen Record
/// traegt.
pub const CCS: u8 = 1;

/// Was in einer `CertificateStatus`-Nachricht steht (RFC 6066 3).
///
/// added 01.10.2026 22:1x UTC (Projektlabel 02.10.2026), S4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zertifikatsstatus {
    /// `status_type`: 1 = `ocsp`, 2 = `stapled SCTs` (RFC 6962).
    pub art: u8,
    /// Der Inhalt von `opaque certificate_status<1..2^24-1>`.
    pub inhalt: Vec<u8>,
}

/// Liest eine `CertificateStatus`-Nachricht.
///
/// added 01.10.2026 22:1x UTC (Projektlabel 02.10.2026), S4. DER AUFBAU (RFC 6066 3):
///
/// ```text
/// struct {
///     CertificateStatusType certificate_status_type;   1 Byte
///     select (certificate_status_type) { … }            keine Laenge
///     opaque certificate_status<1..2^24-1>;            3 Byte Laenge
/// } CertificateStatus;
/// ```
///
/// added 01.10.2026 22:1x UTC (Projektlabel 02.10.2026): die Dreierlaenge ist **kein** Zufall — es ist
/// dieselbe Form wie bei `Certificate` in 1.2, und wer sie als Zweierlaenge
/// liest, verschiebt jedes Byte um eins.
/// added 02.10.2026 02:9x UTC (Projektlabel 02.10.2026): **DIE GANZE
/// NACHRICHT, NICHT DER RUMPF** — das ist die Korrektur, und sie schliesst den
/// Befund aus `PROBLEME.txt` Nr. 19.
///
/// WAS GEMESSEN WURDE (02.10.2026 02:8x, live gegen `xkcd.com`, mit einem
/// Mitschnitt auf beiden Wegen): Der Server schickt eine echte
/// `CertificateStatus`-Nachricht (1721 Byte, `status_type` 1, Antwort 1462
/// Byte) **und** das `status_request`-Echo im ServerHello. Der Client meldete
/// trotzdem „keine OCSP-Antwort".
///
/// DER FEHLER: `strom12.rs` ruft diesen Leser mit der **ganzen** Nachricht
/// (Typ + Dreierlaenge + Rumpf) — so wie `zertifikatliste_lesen`, `ske_dekodiere`
/// und alle anderen Leser dieser Crate, die mit `nachricht.first() ==
/// typen::…` anfangen. Dieser Leser las stattdessen den Rumpf: `art` wurde zu
/// **22** (die Nachrichtennummer) statt 1, und `inhalt` begann mit den vier
/// Kopfbyte `[01 00 05 b6]` statt mit dem DER-Tag `30`. Der Aufrufer verglich
/// `zs.art == 1`, was nie zutraf, also wurde die Nutzlast **stillschweigend
/// verworfen** — kein Fehler, nur `ocsp_stufe::KEINE`.
///
/// WARUM ES NICHT AUFFIEL: mein eigener Vektortest
/// (`tests/ocsp_live_vektor.rs`) rief den Leser mit `&STATUS[4..]` auf, also
/// mit dem **Rumpf** — damit war der Test gruen an einer Fassung, die der
/// Handshake nie benutzt. Ein Test, der sich den Aufruf selbst zusammenbaut,
/// beweist den Aufruf nicht.
pub fn zertifikatsstatus_lesen(nachricht: &[u8]) -> Result<Zertifikatsstatus, Nachricht12Fehler> {
    // added 02.10.2026 02:9x: DER TYP WIRD GEPRUEFT, wie in
    // `zertifikatliste_lesen`. Ohne diese Pruefung laesst sich ein
    // `Certificate` (11) als Statusnachricht lesen, und der Fehler faellt erst
    // an der nachfolgenden Auswertung auf.
    if nachricht.first() != Some(&typen::CERTIFICATE_STATUS) {
        return Err(Nachricht12Fehler::Typ {
            erwartet: typen::CERTIFICATE_STATUS,
            bekommen: *nachricht.first().unwrap_or(&0),
        });
    }
    let mut l = Leser::neu(nachricht);
    l.nimm(1, "typ")?;
    let n = l.u24("laenge")?;
    let koerper = l.nimm(n, "koerper")?;
    if koerper.len() < 4 {
        // added 01.10.2026 22:2x UTC (Projektlabel 02.10.2026): **ein** Test
        // fuer beide Faelle. Die erste Fassung hatte zwei Fehlerformen fuer
        // „zu kurz" (einmal `Aufbau`, einmal `Laenge`) — und `Aufbau` gibt es
        // in diesem Enum nicht, das stand als `error[E0599]` im Weg.
        return Err(Nachricht12Fehler::Laenge {
            typ: typen::CERTIFICATE_STATUS,
            gewollt: 4,
            war: koerper.len(),
        });
    }
    let art = koerper[0];
    let laenge = u32::from_be_bytes([0, koerper[1], koerper[2], koerper[3]]) as usize;
    // added 01.10.2026 22:1x UTC (Projektlabel 02.10.2026): M4-Grenze.
    // `MAX_OCSP_BYTES` ist 1 MiB — ein OCSP-Antwortkoerper dieser Groesse ist
    // absurd, aber ein Server darf nicht den Speicher des Clients bestimmen.
    if laenge > MAX_OCSP_BYTES {
        return Err(Nachricht12Fehler::ZuViele {
            grenze: MAX_OCSP_BYTES,
            was: "stapelte OCSP-Antwort",
        });
    }
    if koerper.len() < 4 + laenge {
        return Err(Nachricht12Fehler::Laenge {
            typ: typen::CERTIFICATE_STATUS,
            gewollt: 4 + laenge,
            war: koerper.len(),
        });
    }
    Ok(Zertifikatsstatus {
        art,
        inhalt: koerper[4..4 + laenge].to_vec(),
    })
}

/// added 01.10.2026 22:1x UTC (Projektlabel 02.10.2026): Obergrenze fuer eine stapelte OCSP-Antwort.
pub const MAX_OCSP_BYTES: usize = 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha2::von_hex;

    /// M7: der ClientHello-Aufbau, an den Laengen.
    ///
    /// DER WEG ZU DEN ECHTEN SERVERBYTES: `mutation_md/tls/tls12_msg.txt`
    /// enthaelt den echten Handshake eines `openssl s_client -msg` gegen
    /// badssl.com vom 30.09.2026. Daraus stammen zwei Konstanten in
    /// dieser Datei: der 70-Byte-`ClientKeyExchange` unten und — als
    /// Beleg, dass die Laengen der Crate an echten Daten passen — die
    /// 4070 Byte der `Certificate`-Nachricht, die `strom12.rs` liest.
    #[test]
    fn client_hello_hat_die_richtige_struktur() {
        let random = [0x11u8; 32];
        let ch = ClientHello::baue(
            &random,
            "example.com",
            &[gruppe::X25519, gruppe::SECP256R1],
            &[0xC02F, 0xC02B],
            &[b"http/1.1"],
            &[
                sigverfahren::RSA_PSS_RSAE_SHA256,
                sigverfahren::ECDSA_SECP256R1_SHA256,
            ],
        )
        .expect("ClientHello");
        // M7: Typ, dreistellige Laenge, dann der Koerper.
        assert_eq!(ch[0], typen::CLIENT_HELLO);
        let n = ((ch[1] as usize) << 16) | ((ch[2] as usize) << 8) | ch[3] as usize;
        assert_eq!(n + 4, ch.len(), "M7: die Laenge deckt den Koerper");

        // Und die Pflichtfelder stehen an den richtigen Stellen.
        assert_eq!(&ch[4..6], &[0x03, 0x03], "client_version ist TLS 1.2");
        assert_eq!(&ch[6..38], &[0x11u8; 32], "das 32-Byte-Random");
        assert_eq!(ch[38], 0, "session_id ist leer");
        assert_eq!(&ch[39..41], &[0, 4], "zwei Suites, also 4 Byte");
        assert_eq!(&ch[41..45], &[0xC0, 0x2F, 0xC0, 0x2B]);
        assert_eq!(&ch[45..47], &[1, 0], "nur die Kompression null");

        // status_request hat genau fuenf Byte Inhalt: status_type plus
        // zwei leere Listen (RFC 6066 Abschnitt 3).
        let pos = ch
            .windows(4)
            .position(|w| w == [0x00, 0x05, 0x00, 0x05])
            .expect("status_request");
        assert_eq!(
            &ch[pos + 4..pos + 9],
            &[1, 0, 0, 0, 0],
            "M7: status_type ocsp, dann zwei leere Listen"
        );
        // extended_master_secret ist drin — ohne sie lehnt RFC 7627 ab.
        assert!(
            ch.windows(4).any(|w| w == [0x00, 0x17, 0x00, 0x00]),
            "M7: extended_master_secret (0x0017, Laenge 0) ist dabei"
        );
        // Und SNI nennt den Hostnamen.
        assert!(
            ch.windows(11).any(|w| w == b"example.com"),
            "M7: der Hostname steht in der Nachricht"
        );
    }

    /// M7: leere Listen und zu lange Felder werden abgelehnt.
    #[test]
    fn grenzen_im_client_hello() {
        let r = [0u8; 32];
        // Zu viele ALPN-Namen.
        let viel: Vec<&[u8]> = vec![b"a"; MAX_ALPN_EINTRAEGE + 1];
        assert!(matches!(
            ClientHello::baue(&r, "h", &[1], &[0xC02F], &viel, &[0x0401]),
            Err(Nachricht12Fehler::ZuViele { .. })
        ));
        // Ein zu langer Hostname.
        let lang = "a".repeat(300);
        assert!(matches!(
            ClientHello::baue(&r, &lang, &[1], &[0xC02F], &[], &[0x0401]),
            Err(Nachricht12Fehler::Feld { .. })
        ));
        // Genau an der Grenze geht es noch.
        assert!(ClientHello::baue(&r, &"a".repeat(255), &[1], &[0xC02F], &[], &[0x0401]).is_ok());
    }

    /// M7: `finished` hat 16 Byte: Typ, dreistellige Laenge 12, 12 Byte.
    #[test]
    fn finished_hat_16_byte() {
        let v = [0xaau8; 12];
        let f = finished_baue(&v).expect("finished");
        assert_eq!(f.len(), 16);
        assert_eq!(f[0], typen::FINISHED);
        assert_eq!(&f[1..4], &[0, 0, 12], "M7: die Laenge ist dreistellig");
        assert_eq!(finished_dekodiere(&f).expect("lesen"), v.to_vec());
        // Und eine falsche Laenge wird abgelehnt, auf beiden Seiten.
        assert!(finished_baue(&[0u8; 11]).is_err());
        let mut kaputt = f.clone();
        kaputt[1] = 0xFF;
        assert!(finished_dekodiere(&kaputt).is_err());
    }

    /// M7: das `ClientKeyExchange` traegt den Punkt mit Laengenprefix.
    #[test]
    fn cke_traegt_den_punkt() {
        let punkt: Vec<u8> = (0..65u32).map(|i| i as u8).collect();
        let c = cke_baue(&punkt).expect("cke");
        assert_eq!(c[0], typen::CLIENT_KEY_EXCHANGE);
        assert_eq!(c[4], 65, "M7: 65 Byte fuer P-256");
        assert_eq!(&c[5..70], &punkt[..]);
        assert!(cke_baue(&vec![0u8; 256]).is_err());
    }

    /// M7: der `ClientKeyExchange` gegen einen echten Server.
    ///
    /// Die 70 Byte sind aus `mutation_md/tls/tls12_msg.txt` GELESEN —
    /// das `-msg`-Protokoll eines `openssl s_client` gegen badssl.com
    /// vom 30.09.2026, Suite TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256.
    /// Es ist ein echtes ClientKeyExchange mit dem oeffentlichen
    /// P-256-Punkt, den ein Server angenommen hat.
    ///
    /// DER FEHLER, DEN DIESER TEST ZUERST HATTE: ich hatte die 65 Byte
    /// des Punktes abgeschrieben und dabei 74 bekommen. In Rust
    /// entfernt ein `\` am Zeilenende die nachfolgenden Leerzeichen —
    /// bei einem Hex-Block mit Leerzeichen zwischen den Gruppen genau
    /// der Fehler, den man nicht sieht.
    const ECHTER_CKE: &str = "1000004241043c92e8b8e391cfb6d643c8fa3f3dd8370ba9aee1991cce5d8b226082db3a8c609408b810d1571f28ed1395f95d534d2bf18c866caeeaba4314d05290f12b3840";

    #[test]
    fn der_echte_client_key_exchange_aus_badssl() {
        let c = von_hex(ECHTER_CKE).expect("Hex");
        assert_eq!(c.len(), 70, "M7: 4 Kopf + 66 Koerper");
        assert_eq!(c[0], typen::CLIENT_KEY_EXCHANGE);
        // Die Laenge im Kopf ist DREISTELLIG: 0x000042 = 66.
        assert_eq!(
            u32::from_be_bytes([0, c[1], c[2], c[3]]),
            66,
            "M7: Koerper 66 Byte"
        );
        assert_eq!(c[4], 65, "M7: der Punkt ist 65 Byte lang");
        assert_eq!(c[5], 0x04, "M7: uncompressed");
        // Und unser Bauer erzeugt aus demselben Punkt dieselbe Nachricht.
        let gebaut = cke_baue(&c[5..]).expect("cke");
        assert_eq!(gebaut, c, "M7: aus dem Punkt kommt dieselbe Nachricht");
    }
}
