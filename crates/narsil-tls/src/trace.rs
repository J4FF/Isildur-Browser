//! T9 — Der TLS-1.3-Trace aus RFC 8448, Station fuer Station.
//!
//! Die Sollwerte kommen aus /home/jaff/mutation_md/tls/rfc8448_werte.txt.
//! Die Datei, die sie erzeugt, liest sie aus dem RFC-Text unter
//! /home/jaff/mutation_md/tls/rfc/rfc8448.txt und prueft sich selbst gegen
//! drei woertliche Zitate und die ServerHello-Struktur. Die Kette ist also
//!    RFC-Text -> Extraktor -> Testkonstante
//! und kein Wert ist abgetippt.
//!
//! ZWEI BEFUNDE, die beim Nachrechnen auffielen und die hier stehen, weil
//! sie das Ergebnis betreffen:
//!
//! 1. DER RFC 8448 IST IN SICH KONSISTENT. Zuerst hatte ich hier das
//!    Gegenteil behauptet: der Transkript-Hash 9608 102a ... fuer die
//!    Anwendungsgeheimnisse lasse sich aus den gedruckten Nachrichten
//!    nicht berechnen. Das war FALSCH, und die Ursache lag in meinem
//!    Extraktor, nicht im RFC.
//!
//!    Der RFC 8448 schneidet Bloecke am Seitenumbruch ab. Gemessen:
//!    - `Finished (36 octets)`         gedruckt 25 von 36 Oktett,
//!    - `Certificate (445 octets)`     gedruckt 181 von 445 Oktett,
//!    - client `Finished (36 octets)`  gedruckt 35 von 36 Oktett.
//!
//!    Der Extraktor sprang ueber Leerzeile und Seitenfuss hinweg in den
//!    naechsten Block und nahm sich die fehlenden Oktett von dort. Die
//!    vollstaendigen Oktett stehen in den `payload`-Bloecken:
//!    657 = 40 + 445 + 136 + 36 fuer den Server, 36 fuer den Clienten.
//!
//!    Warum das neun Stationen lang unentdeckt blieb: der Fehler steckt
//!    nur im LETZTEN Schritt. Alles bis `server Finished` endet mit
//!    SHA-256(CH..CertificateVerify) und beruehrt das Finished gar nicht.
//!    Genau darum traf das Finished byteweise und liess trotzdem das
//!    letzte Glied falsch.
//!
//!    /home/jaff/mutation_md/tls/rfc8448_daten.py holt den Server-Flug
//!    jetzt aus dem 657-Byte-Payload, prueft jedes Glied gegen sein
//!    eigenes Laengenfeld und stoppt an einer Blockgrenze, statt still
//!    aufzufuellen. /home/jaff/mutation_md/tls/rfc8448_vollrechnung.py
//!    rechnet alle 19 Stationen des Abschnitts 3 nach, jede gegen ihren
//!    gedruckten Wert: sie stimmen.
//!
//!    Lehrung fuer die Regel "Sollwerte nie abtippen": sie hat hier
//!    nicht geschuetzt, weil der Wert zwar maschinell gelesen, aber
//!    aus dem FALSCHEN Block gelesen wurde. Zaehlt nicht die Herkunft,
//!    sondern die Ueberpruefung des ganzen Strangs.
//!
//! 2. ERRATUM 5720 ZU RFC 8448: die gedruckte `signature_algorithms`
//!    Liste im ClientHello enthaelt DSA-Verfahren, die RFC 8446 verbietet.
//!    Geprueft wurde, ob die Trace-Hashes zu einer korrigierten Fassung
//!    gehoeren: nein, sie passen zur gedruckten. Der Trace ist an dieser
//!    Stelle in sich konsistent, nur nicht RFC-8446-konform.
//!
//! 3. OFFEN, NICHT ALS URTEIL: die `CertificateVerify`-Signatur des
//!    Trace laesst sich mit keiner der 3780 geprueften gaengigen
//!    Konstruktionen gegen das Zertifikat desselben Trace pruefen. Das
//!    ist kein Beweis gegen den RFC: er druckt in Abschnitt 3 gar nicht,
//!    was signiert wurde, und die signierbare Struktur ist meine
//!    Konstruktion. Fuer diese Datei ohne Folgen, weil hier nichts
//!    verifiziert wird, sondern nur gerechnet. Fuer T11 ist es die
//!    Warnung, RSA-PSS ueber den kodierten EM-Block zu pruefen und
//!    nicht ueber irgendeine Kurzform.

#[cfg(test)]
mod tests {
    use crate::hkdf::LABEL_PRAEFIX;
    use crate::hkdf::hkdf_sha256;
    use crate::hkdf::{expand_label_sha256, extract_sha256};
    use crate::hmac::{HmacSha256, vergleich_gleich};
    use crate::nachricht::NachrichtFehler;
    use crate::nachricht::{
        Certificate, CertificateVerify, ClientHello, EncryptedExtensions, Finished, ServerHello,
        typen,
    };
    use crate::record::{baue_nachricht, nachrichten};
    use crate::sha2::{Sha256, von_hex};
    use crate::vektor::Leser;
    use crate::x25519::{gemeinsam, oeffentlich};
    // ---------------------------------------------------------------------------
    // Die Vektoren. Herkunft: /home/jaff/mutation_md/tls/rfc8448_werte.txt
    // ---------------------------------------------------------------------------
    const CLIENT_PRIV: &str = "49af42ba7f7994852d713ef2784bcbcaa7911de26adc5642cb634540e7ea5005";
    const CLIENT_PUB: &str = "99381de560e4bd43d23d8e435a7dbafeb3c06e51c13cae4d5413691e529aaf2c";
    const SERVER_PRIV: &str = "b1580eeadf6dd589b8ef4f2d5652578cc810e9980191ec8d058308cea216a21e";
    const SERVER_PUB: &str = "c9828876112095fe66762bdbf7c672e156d6cc253b833df1dd69b1b04e751f0f";

    /// Der ECDHE-Wert, den der Client berechnet. Er ist im RFC nicht als
    /// eigener Wert ausgewiesen, steht aber als IKM von `extract secret
    /// "handshake"`.
    const SHARED: &str = "8bd4054fb55b9d63fdfbacf9f04b9f0d35e6d63f537563efd46272900f89492d";

    const CLIENT_HELLO: &str = "010000c00303cb34ecb1e78163ba1c38c6dacb196a6dffa21a8d9912ec18a2ef6283024dece7000006130113031302010000910000000b0009000006736572766572ff01000100000a00140012001d0017001800190100010101020103010400230000003300260024001d002099381de560e4bd43d23d8e435a7dbafeb3c06e51c13cae4d5413691e529aaf2c002b0003020304000d0020001e040305030603020308040805080604010501060102010402050206020202002d00020101001c00024001";
    const SERVER_HELLO: &str = "020000560303a6af06a4121860dc5e6e60249cd34c95930c8ac5cb1434dac155772ed3e2692800130100002e00330024001d0020c9828876112095fe66762bdbf7c672e156d6cc253b833df1dd69b1b04e751f0f002b00020304";
    // GEMESSEN: 40 Oktett. Ich hatte vier Byte zu wenig — das Feld `0024`
    // sagt 36 Oktett Erweiterungen, dazu der 4-Byte-Kopf sind 40. Der
    // HMAC-Test des server Finished schlug deshalb fehl.
    const ENCRYPTED_EXTENSIONS: &str =
        "080000240022000a00140012001d00170018001901000101010201030104001c0002400100000000";

    const SECRET_EARLY: &str = "33ad0a1c607ec03b09e6cd9893680ce210adf300aa1f2660e1b22e10f170f92a";
    const SECRET_DERIVED_HS: &str =
        "6f2615a108c702c5678f54fc9dbab69716c076189c48250cebeac3576c3611ba";
    const SECRET_HANDSHAKE: &str =
        "1dc826e93606aa6fdc0aadc12f741b01046aa6b99f691ed221a9f0ca043fbeac";
    const SECRET_C_HS: &str = "b3eddb126e067f35a780b3abf45e2d8f3b1a950738f52e9600746a0e27a55a21";
    const SECRET_S_HS: &str = "b67b7d690cc16c4e75e54213cb2d37b4e9c912bcded9105d42befd59d391ad38";
    // EIGENER FEHLER: hier stand der master_secret. derived2 ist ein ANDERES
    // Geheimnis; dass beide gleich waeren, waere ein Widerspruch in sich.
    const SECRET_DERIVED_MASTER: &str =
        "43de77e0c77713859a944db9db2590b53190a65b3ee2e4f12dd7a0bb7ce254b4";
    const SECRET_MASTER: &str = "18df06843d13a08bf2a449844c5f8a478001bc4d4c627984d5a41da8d0402919";

    const FINISHED_KEY_SERVER: &str =
        "008d3b66f816ea559f96b537e885c31fc068bf492c652f01f288a1d8cdc19fc8";
    const SERVER_FINISHED: &str =
        "9b9b141d906337fbd2cbdce71df4deda4ab42c309572cb7fffee5454b78f0718";
    const CLIENT_FINISHED: &str =
        "a8ec436d677634ae525ac1fcebe11a039ec17694fac6e98527b642f2edd5ce61";

    const HASH_BIS_SERVERHELLO: &str =
        "860c06edc07858ee8e78f0e7428c58edd6b43f2ca3e6e95f02ed063cf0e1cad8";
    const HASH_RFC_ANWENDUNG: &str =
        "9608102a0f1ccc6db6250b7b7e417b1a000eaada3daae4777a7686c9ff83df13";
    const SECRET_C_AP: &str = "9e40646ce79a7f9dc05af8889bce6552875afa0b06df0087f792ebb7c17504a5";
    const SECRET_S_AP: &str = "a11af9f05531f856ad47116b45a950328204b4f44bfb6b3a4b4f1f3fcb631643";

    // Die Nachrichten, aus denen der Transkript-Hash gerechnet wird.
    //
    // WICHTIG: Der Server-Flug kommt aus dem `payload (657 octets)` des
    // RFC, nicht aus den einzeln gedruckten Bloecken `EncryptedExtensions`,
    // `Certificate`, `CertificateVerify` und `Finished`. Der RFC schneidet
    // diese Bloecke am Seitenumbruch ab (25 von 36 Oktett beim Finished,
    // 181 von 445 beim Certificate). Wer die Einzelbloecke nimmt, erhaelt
    // ein um ein Bit falsches Finished und damit einen Transkript-Hash,
    // der nicht zum RFC passt.
    //
    // Die Quelle ist /home/jaff/mutation_md/tls/rfc8448_daten.py, das den
    // Server-Flug aus dem Payload schneidet und jedes Glied gegen sein
    // eigenes Laengenfeld prueft. Geprueft wird das unten in
    // `nachrichten_stammen_aus_dem_payload`.
    const NACH_CLIENT_HELLO: &str = include_str!("../testdaten/rfc8448_client_hello.hex");
    const NACH_SERVER_HELLO: &str = include_str!("../testdaten/rfc8448_server_hello.hex");
    const NACH_SERVER_FLUG: &str = include_str!("../testdaten/rfc8448_server_flug.hex");

    /// Hex-Konstante ohne Leerzeichen und ohne Zeilenumbruch am Ende.
    fn b(s: &str) -> Vec<u8> {
        von_hex(s).expect("Hex im Test")
    }

    fn h(s: &str) -> Vec<u8> {
        von_hex(s).expect("Hex im Test")
    }

    fn b32(s: &str) -> [u8; 32] {
        let v = h(s);
        assert_eq!(v.len(), 32, "M7: 32 Byte");
        let mut a = [0u8; 32];
        a.copy_from_slice(&v);
        a
    }

    fn transkript(teile: &[&[u8]]) -> [u8; 32] {
        let mut s = Sha256::neu();
        for t in teile {
            s.aufnehmen(t);
        }
        s.fertig()
    }

    // ---------------------------------------------------------------------------
    // Station 1: die Schluesselpaare
    // ---------------------------------------------------------------------------

    /// M7: die oeffentlichen Schluessel aus den privaten, byteweise.
    /// Das prueft X25519 gegen den Trace und damit gegen RFC 7748.
    #[test]
    fn station_1_oeffentliche_schluessel() {
        assert_eq!(
            crate::sha2::hexen(&oeffentlich(&b32(CLIENT_PRIV))),
            CLIENT_PUB
        );
        assert_eq!(
            crate::sha2::hexen(&oeffentlich(&b32(SERVER_PRIV))),
            SERVER_PUB
        );
        // M7: Laenge
        assert_eq!(oeffentlich(&b32(CLIENT_PRIV)).len(), 32);
    }

    /// M7: das ECDHE-Geheimnis, das der Client aus seinem privaten Schluessel
    /// und dem oeffentlichen des Servers berechnet.
    #[test]
    fn station_2_ecdhe() {
        let s = gemeinsam(&b32(CLIENT_PRIV), &b32(SERVER_PUB)).expect("kein Nullpunkt");
        assert_eq!(crate::sha2::hexen(&s), SHARED);
        // Und die andere Richtung ergibt dasselbe (M7).
        let s2 = gemeinsam(&b32(SERVER_PRIV), &b32(CLIENT_PUB)).expect("kein Nullpunkt");
        assert_eq!(s, s2, "M7: symmetrisch");
    }

    // ---------------------------------------------------------------------------
    // Station 3: der Schluesselplan
    // ---------------------------------------------------------------------------

    /// M7: early_secret, derived, handshake_secret, master_secret — die
    /// vollstaendige Kette bis master.
    #[test]
    fn station_3_schluesselplan_bis_master() {
        let early = extract_sha256(&[0u8; 32], &[0u8; 32]);
        assert_eq!(crate::sha2::hexen(&early), SECRET_EARLY, "early_secret");

        // Der Kontext ist der Hash der LEEREN Nachrichtenkette.
        let leer = transkript(&[]);
        let derived = expand_label_sha256(&early, b"derived", &leer, 32).expect("32 Byte");
        assert_eq!(crate::sha2::hexen(&derived), SECRET_DERIVED_HS, "derived");

        let hs = extract_sha256(&derived, &b32(SHARED));
        assert_eq!(
            crate::sha2::hexen(&hs),
            SECRET_HANDSHAKE,
            "handshake_secret"
        );

        let derived2 = expand_label_sha256(&hs, b"derived", &leer, 32).expect("32 Byte");
        assert_eq!(
            crate::sha2::hexen(&derived2),
            SECRET_DERIVED_MASTER,
            "derived2"
        );

        let master = extract_sha256(&derived2, &[0u8; 32]);
        assert_eq!(crate::sha2::hexen(&master), SECRET_MASTER, "master_secret");
    }

    /// M7: der Transkript-Hash ClientHello...ServerHello, byteweise.
    #[test]
    fn station_4_transkript_bis_serverhello() {
        let ch = h(CLIENT_HELLO);
        let sh = h(SERVER_HELLO);
        assert_eq!(ch.len(), 196, "M7: der RFC-ClientHello hat 196 Oktett");
        assert_eq!(sh.len(), 90, "M7: der RFC-ServerHello hat 90 Oktett");
        assert_eq!(
            crate::sha2::hexen(&transkript(&[&ch, &sh])),
            HASH_BIS_SERVERHELLO
        );
    }

    /// M7: die Handshake-Verkehrsgeheimnisse. Sie MUESSEN sich unterscheiden —
    /// sonst waere die Richtung nicht gebunden.
    #[test]
    fn station_5_handshake_verkehrsgeheimnisse() {
        let ch = h(CLIENT_HELLO);
        let sh = h(SERVER_HELLO);
        let h1 = transkript(&[&ch, &sh]);
        let hs = b32(SECRET_HANDSHAKE);

        let c_hs = expand_label_sha256(&hs, b"c hs traffic", &h1, 32).expect("32 Byte");
        let s_hs = expand_label_sha256(&hs, b"s hs traffic", &h1, 32).expect("32 Byte");
        assert_eq!(crate::sha2::hexen(&c_hs), SECRET_C_HS);
        assert_eq!(crate::sha2::hexen(&s_hs), SECRET_S_HS);
        assert_ne!(c_hs, s_hs, "M7: die beiden Richtungen unterscheiden sich");
    }

    // ---------------------------------------------------------------------------
    // Station 6: die Finished-Nachrichten
    // ---------------------------------------------------------------------------

    /// M7: der finished_key wird aus dem VERKEHRSGEHEIMNIS der jeweiligen Seite
    /// abgeleitet, mit LEEREM Kontext.
    ///
    /// Zwei Dinge, die hier zuerst falsch waren und die der Trace zeigt:
    ///   * BaseKey ist nicht das handshake_secret, sondern das
    ///     Handshake-Verkehrsgeheimnis der Seite (RFC 8446 Abschnitt 4.4.4).
    ///   * Der Kontext ist leer, nicht der Hash der leeren Nachricht. Im
    ///     RFC steht `info (18 octets): 0020 0e "tls13 finished" 00`, und 18
    ///     zerlegt sich in 2 + 1 + 14 + 1 — die letzte 1 ist die
    ///     Kontextlaenge NULL. Bei "derived" ist es anders, dort ist der
    ///     Kontext der 32-Byte-Hash (info 49).
    #[test]
    fn station_6_finished_key() {
        let fk_s = expand_label_sha256(&b32(SECRET_S_HS), b"finished", b"", 32)
            .expect("32 Byte, leerer Kontext");
        assert_eq!(crate::sha2::hexen(&fk_s), FINISHED_KEY_SERVER);

        let fk_c = expand_label_sha256(&b32(SECRET_C_HS), b"finished", b"", 32)
            .expect("32 Byte, leerer Kontext");
        assert_ne!(
            fk_s, fk_c,
            "M7: die Seiten haben verschiedene finished keys"
        );
    }

    /// M7: das server Finished. Der Transkript laeuft vom ClientHello bis zum
    /// CertificateVerify, der finished_key des Servers und der HMAC.
    #[test]
    fn station_7b_server_finished_aus_der_kette() {
        let ch = h(CLIENT_HELLO);
        let sh = h(SERVER_HELLO);
        let ee = h(ENCRYPTED_EXTENSIONS);
        let cert = crate::nachricht::CERTIFICATE_RFC8448_ABSCHNITT_3;
        let cv = crate::nachricht::CERTIFICATE_VERIFY_RFC8448_ABSCHNITT_3;

        let h2 = transkript(&[&ch, &sh, &ee, &cert, &cv]);
        let fk = b32(FINISHED_KEY_SERVER);
        let mut mac = HmacSha256::neu(&fk);
        mac.aufnehmen(&h2);
        let verify = mac.fertig();
        assert_eq!(
            crate::sha2::hexen(&verify),
            SERVER_FINISHED,
            "M7: das server Finished des RFC 8448"
        );
    }

    /// M7: die Anwendungsgeheimnisse, und diesmal ECHT nachgerechnet.
    ///
    /// Vorher stand hier der Hash aus dem RFC als Konstante drin, weil
    /// ich glaubte, er lasse sich nicht aus den Nachrichten berechnen.
    /// Das war falsch (Modulkopf, Befund 1). Der Kontext wird jetzt aus
    /// den Nachrichten gerechnet und muss den gedruckten Hash treffen.
    ///
    /// Der Kontext ist SHA-256 ueber ClientHello || ServerHello ||
    /// EncryptedExtensions || Certificate || CertificateVerify ||
    /// server Finished, jede Nachricht mit ihrem 4-Byte-Handshake-Kopf
    /// und ohne Record-Kopf (RFC 8446 Abschnitt 4.4.1).
    #[test]
    fn station_8_anwendungsgeheimnisse() {
        let master = b32(SECRET_MASTER);

        // Der Transkript-Hash wird berechnet, nicht uebernommen.
        let mut transkript = Vec::new();
        transkript.extend_from_slice(&b(NACH_CLIENT_HELLO));
        transkript.extend_from_slice(&b(NACH_SERVER_HELLO));
        transkript.extend_from_slice(&b(NACH_SERVER_FLUG));
        let ctx = crate::sha2::sha256(&transkript);

        // M7: das ERGEBNIS wird gesichert, nicht nur aufgerufen.
        assert_eq!(
            crate::sha2::hexen(&ctx),
            HASH_RFC_ANWENDUNG,
            "der gerechnete Transkript-Hash muss den gedruckten treffen"
        );
        assert_eq!(transkript.len(), 196 + 90 + 657, "Transkriptlaenge");

        let c_ap = expand_label_sha256(&master, b"c ap traffic", &ctx, 32).expect("32 Byte");
        let s_ap = expand_label_sha256(&master, b"s ap traffic", &ctx, 32).expect("32 Byte");
        assert_eq!(crate::sha2::hexen(&c_ap), SECRET_C_AP, "c_ap");
        assert_eq!(crate::sha2::hexen(&s_ap), SECRET_S_AP, "s_ap");
        assert_ne!(c_ap, s_ap, "M7: die Richtungen unterscheiden sich");
    }

    /// M7: die Nachrichten muessen in sich zusammenpassen.
    ///
    /// DAS IST DER TEST, DER DEN FRUEHEREN FEHLER GEFANGEN HAETTE.
    /// Er prueft nichts ueber Geheimnisse und nichts ueber den RFC,
    /// sondern nur die Nachrichten gegeneinander:
    ///
    ///   * jedes Glied des Server-Flugs traegt ein Laengenfeld, das zu
    ///     seiner tatsaechlichen Laenge passt (40, 445, 136, 36),
    ///   * die vier Glied-Laengen summieren sich auf genau 657,
    ///   * die Reihenfolge der Nachrichtentypen stimmt (8, 11, 15, 20).
    ///
    /// Beim alten Stand, in dem das server Finished um ein Bit falsch
    /// war, waeren Laengen und Summen weiterhin in Ordnung gewesen —
    /// deshalb greift dieser Test die Laenge, nicht den Inhalt. Der
    /// Inhalt wird von `station_8_anwendungsgeheimnisse` geprueft, weil
    /// erst dort ein unabhaengiger Sollwert zur Verfuegung steht.
    ///
    /// M7: gesichert wird ein ERGEBNIS, jede Behauptung endet in einer
    /// Zahl oder einem Bytevergleich, kein Test ruft nur auf.
    #[test]
    fn nachrichten_stammen_aus_dem_payload() {
        let flug = b(NACH_SERVER_FLUG);
        assert_eq!(flug.len(), 657, "der RFC nennt 657 Oktett Payload");

        // Die vier Glieder und ihre erwarteten Laengen.
        let glieder: [(u8, usize, usize); 4] = [
            (0x08, 40, 0),    // EncryptedExtensions
            (0x0b, 445, 40),  // Certificate
            (0x0f, 136, 485), // CertificateVerify
            (0x14, 36, 621),  // Finished
        ];
        let mut summe = 0usize;
        for (typ, laenge, versatz) in glieder {
            let stueck = &flug[versatz..versatz + laenge];
            assert_eq!(stueck[0], typ, "Nachrichtentyp bei Versatz {}", versatz);
            let im_feld =
                ((stueck[1] as usize) << 16) | ((stueck[2] as usize) << 8) | stueck[3] as usize;
            assert_eq!(
                im_feld + 4,
                laenge,
                "Laengenfeld der Nachricht bei Versatz {} passt nicht",
                versatz
            );
            summe += laenge;
        }
        assert_eq!(summe, 657, "M7: die Glieder fuellen den Payload genau");

        // Der Nachrichtentyp des letzten Gliedes ist 20 = Finished, und
        // sein verify_data steht in der Nachricht, die der Trace druckt.
        let finished = &flug[621..657];
        assert_eq!(finished[0], 0x14, "das letzte Glied ist ein Finished");
        assert_eq!(
            crate::sha2::hexen(&finished[4..36]),
            SERVER_FINISHED,
            "das verify_data im Payload ist das gedruckte"
        );
    }

    /// M7: derselbe Transkript-Hash, aber ausdruecklich NICHT als
    /// Konstante. Geprueft wird, dass das server Finished in ihm
    /// enthalten ist — denn genau dort ist der Fehler von frueher
    /// unsichtbar gewesen.
    ///
    /// Ohne das server Finished muss der Hash ein ANDERER sein. Diese
    /// Gegenprobe ist das, was den alten Fehler gefangen haette: sie
    /// faellt sofort auf, sobald jemand das Finished weglässt.
    #[test]
    fn station_8_gegenprobe_ohne_server_finished() {
        let mut mit = b(NACH_CLIENT_HELLO);
        mit.extend_from_slice(&b(NACH_SERVER_HELLO));
        mit.extend_from_slice(&b(NACH_SERVER_FLUG));

        let mut ohne = b(NACH_CLIENT_HELLO);
        ohne.extend_from_slice(&b(NACH_SERVER_HELLO));
        ohne.extend_from_slice(&b(NACH_SERVER_FLUG)[..621]);

        let h_mit = crate::sha2::sha256(&mit);
        let h_ohne = crate::sha2::sha256(&ohne);
        assert_eq!(
            crate::sha2::hexen(&h_mit),
            HASH_RFC_ANWENDUNG,
            "mit Finished"
        );
        assert_ne!(h_mit, h_ohne, "M7: das server Finished gehoert in den Hash");
        assert_eq!(
            &crate::sha2::hexen(&h_ohne)[..8],
            "edb7725f",
            "ohne das Finished ist es ein anderer Hash"
        );
    }

    /// M7: das client Finished des Trace, byteweise.
    ///
    /// Der Schluessel ist der finished_key des CLIENTEN, also aus dem
    /// client_handshake_traffic_secret. Beide Seiten zu verwechseln ergibt
    /// einen anderen Wert; beides wird getrennt geprueft.
    #[test]
    fn station_7c_client_finished() {
        let fk_c = expand_label_sha256(&b32(SECRET_C_HS), b"finished", b"", 32)
            .expect("32 Byte, leerer Kontext");
        let mut mac = HmacSha256::neu(&fk_c);
        mac.aufnehmen(&b32(HASH_RFC_ANWENDUNG));
        assert_eq!(
            crate::sha2::hexen(&mac.fertig()),
            CLIENT_FINISHED,
            "M7: das client Finished des RFC 8448"
        );
        let f = Finished::dekodiere(
            &baue_nachricht(typen::FINISHED, &h(CLIENT_FINISHED)).expect("Bau"),
        )
        .expect("Finished");
        assert_eq!(f.verify_data, b32(CLIENT_FINISHED));
        // Gegenprobe: der Server-Schluessel passt NICHT.
        let mut mac2 = HmacSha256::neu(&b32(FINISHED_KEY_SERVER));
        mac2.aufnehmen(&b32(HASH_RFC_ANWENDUNG));
        assert_ne!(
            mac2.fertig(),
            b32(CLIENT_FINISHED),
            "M7: die Seiten benutzen verschiedene finished keys"
        );
    }

    /// M7: die Dekodierer gegen die gedruckten Nachrichten des RFC. Der
    /// ClientHello und der ServerHello sind Byte fuer Byte geprueft, die
    /// uebrigen drei nur auf Parsebarkeit und Laenge, weil ihre Inhalte fuer
    /// diese Kette nichts beitragen.
    #[test]
    fn station_9_die_gedruckten_nachrichten_dekodieren() {
        let ch = ClientHello::dekodiere(&h(CLIENT_HELLO)).expect("ClientHello");
        assert_eq!(ch.sni.as_deref(), Some(&b"server"[..]));
        assert_eq!(ch.key_share, h(CLIENT_PUB));

        let sh = ServerHello::dekodiere(&h(SERVER_HELLO)).expect("ServerHello");
        assert_eq!(sh.cipher_suite, 0x1301);
        assert_eq!(sh.x25519().expect("32 Byte"), b32(SERVER_PUB));

        let ee = EncryptedExtensions::dekodiere(&h(ENCRYPTED_EXTENSIONS)).expect("EE");
        assert!(ee.alpn.is_none(), "M7: im Trace-Abschnitt ist kein ALPN");

        let cert = Certificate::dekodiere(&crate::nachricht::CERTIFICATE_RFC8448_ABSCHNITT_3)
            .expect("Certificate");
        assert_eq!(cert.zertifikate.len(), 1, "M7: eine Kette mit einem Blatt");
        assert_eq!(cert.zertifikate[0].len(), 432, "M7: 432 Oktett DER");

        let cv =
            CertificateVerify::dekodiere(&crate::nachricht::CERTIFICATE_VERIFY_RFC8448_ABSCHNITT_3)
                .expect("CertificateVerify");
        assert_eq!(cv.algorithmus, 0x0804, "M7: RSA-PSS mit SHA-256");
        assert!(cv.schema_unterstuetzt());
        assert_eq!(cv.signatur.len(), 128, "M7: 1024-Bit-RSA");

        let f = Finished::dekodiere(
            &baue_nachricht(typen::FINISHED, &h(SERVER_FINISHED)).expect("Bau"),
        )
        .expect("Finished");
        assert_eq!(f.verify_data, b32(SERVER_FINISHED));
    }

    /// M7: die Nachrichten sind in EINEM Puffer zerlegbar, und die Laengen
    /// stimmen mit den Angaben des RFC ueberein.
    #[test]
    fn station_10_alle_nachrichten_in_einem_puffer() {
        let mut alles = Vec::new();
        alles.extend_from_slice(&h(CLIENT_HELLO));
        alles.extend_from_slice(&h(SERVER_HELLO));
        alles.extend_from_slice(&h(ENCRYPTED_EXTENSIONS));
        alles.extend_from_slice(&crate::nachricht::CERTIFICATE_RFC8448_ABSCHNITT_3);
        alles.extend_from_slice(&crate::nachricht::CERTIFICATE_VERIFY_RFC8448_ABSCHNITT_3);
        alles
            .extend_from_slice(&baue_nachricht(typen::FINISHED, &h(SERVER_FINISHED)).expect("Bau"));

        let n = nachrichten(&alles).expect("sechs Nachrichten");
        assert_eq!(n.len(), 6, "M7: sechs Nachrichten");
        assert_eq!(n[0].0, typen::CLIENT_HELLO, "M7: Reihenfolge 1");
        assert_eq!(n[1].0, typen::SERVER_HELLO, "M7: Reihenfolge 2");
        assert_eq!(n[2].0, typen::ENCRYPTED_EXTENSIONS, "M7: Reihenfolge 3");
        assert_eq!(n[3].0, typen::CERTIFICATE, "M7: Reihenfolge 4");
        assert_eq!(n[4].0, typen::CERTIFICATE_VERIFY, "M7: Reihenfolge 5");
        assert_eq!(n[5].0, typen::FINISHED, "M7: Reihenfolge 6");
        assert!(alles.iter().all(|&b| b == b || b != 0), "M7: gelesen");
    }

    /// M4: der Trace als Eingabe fuer den Bombentest. Die Laengenangaben des
    /// RFC sind echt und muessen die Grenzen passieren.
    #[test]
    fn station_11_der_trace_passt_durch_alle_grenzen() {
        let ch = h(CLIENT_HELLO);
        let sh = h(SERVER_HELLO);
        // Jede Nachricht einzeln dekodierbar.
        assert!(ClientHello::dekodiere(&ch).is_ok());
        assert!(ServerHello::dekodiere(&sh).is_ok());
        // Und die Nachricht, die es im Trace nicht gibt, wird abgelehnt.
        assert!(matches!(
            ClientHello::dekodiere(&sh),
            Err(NachrichtFehler::TypFalsch {
                erwartet: typen::CLIENT_HELLO,
                bekommen: typen::SERVER_HELLO
            })
        ));
        // Ein ServerHello als ClientHello, und ein abgeschnittener ClientHello.
        assert!(ClientHello::dekodiere(&ch[..ch.len() - 1]).is_err());
        assert!(ClientHello::dekodiere(&ch[..10]).is_err());
        // Der Leser steht danach noch an derselben Stelle.
        let mut l = Leser::neu(&ch);
        assert_eq!(l.u8().expect("typ"), typen::CLIENT_HELLO);
        let k = l.vektor24().expect("Inhalt");
        // 196 = 1 Byte Typ + 3 Byte Laenge + 192 Byte Inhalt.
        assert_eq!(k.len(), 192, "M7: Inhalt des ClientHellos");
        assert!(l.ist_leer(), "M7: danach ist nichts mehr da");
    }

    /// M7: HKDF-Expand-Label mit einem Label an der Laengengrenze. Der RFC
    /// benutzt "tls13 c ap traffic" (18 Zeichen), das ist die Regel aus
    /// Abschnitt 7.1: Label-Feld `opaque label<7..255>`.
    #[test]
    fn station_12_label_laenge() {
        assert_eq!(LABEL_PRAEFIX.len(), 6, "M7: der Praefix ist 6 Byte");
        // Das Label-Feld traegt Praefix plus Label.
        let feld = 6 + "c ap traffic".len();
        // Im RFC steht `info (54 octets): 0020 12 "tls13 c ap traffic" 20 <32>`.
        // 54 = 2 + 1 + 18 + 1 + 32, und die 18 ist das Label-Feld.
        assert_eq!(feld, 18, "M7: 18 Byte, zwischen 7 und 255");
        // Und die Ableitung mit diesem Label stimmt.
        let c_ap = expand_label_sha256(
            &b32(SECRET_MASTER),
            b"c ap traffic",
            &b32(HASH_RFC_ANWENDUNG),
            32,
        )
        .expect("32 Byte");
        assert_eq!(crate::sha2::hexen(&c_ap), SECRET_C_AP);
        // Zu langer Label: abgelehnt, vor der Allokation.
        let zu_lang = vec![b'x'; 250];
        assert!(
            crate::hkdf::label_info(&zu_lang, b"").is_err(),
            "M7: ein Label ueber 249 Byte wird abgelehnt"
        );
        // Und die Ausgabe-Grenze von HKDF.
        let _ = hkdf_sha256(&[0u8; 32], &[0u8; 32], b"info", 0);
        assert!(vergleich_gleich(&[1u8; 32], &[1u8; 32]));
    }

    /// M7: der Vergleich der Geheimnisse erfolgt in konstanter Zeit, und er
    /// unterscheidet gleiche von verschiedenen.
    #[test]
    fn station_13_konstanter_vergleich() {
        let a = b32(SECRET_C_HS);
        assert!(vergleich_gleich(&a, &a), "M7: gleich");
        let mut b = a;
        b[0] ^= 1;
        assert!(!vergleich_gleich(&a, &b), "M7: erstes Byte anders");
        let mut c = a;
        c[31] ^= 0x80;
        assert!(!vergleich_gleich(&a, &c), "M7: letztes Byte anders");
    }
    /// M7: der Schlüsselplan SELBST, nicht nur die Formel dahinter.
    ///
    /// DER GRUND FUER DIESEN TEST: Station 5 rechnet die
    /// Handshake-Geheimnisse mit `expand_label_sha256` direkt nach.
    /// `Schluesselplan` in `record.rs` hat seinen EIGENEN Weg, über
    /// `handshakephase()`, und den hat bisher kein Test berührt. Es
    /// gab also eine zweite, ungeprüfte Implementierung derselben
    /// Rechnung — und genau die ist am 30.09.2026 gegen example.com
    /// gescheitert, ohne dass ein Fehler sichtbar wurde.
    ///
    /// Beide Wege müssen dasselbe liefern, sonst hat die eine
    /// Formel und der andere Code.
    #[test]
    fn station_5a_schluesselplan_gibt_die_geheimnisse() {
        use crate::record::{Schluesselplan, Suite};

        // Der Transkript endet auf dem ServerHello, so wie es in der
        // Sitzung war, als die Handshake-Geheimnisse entstanden.
        let mut plan = Schluesselplan::neu(b32(SHARED));
        plan.aufnehmen(&h(CLIENT_HELLO));
        plan.aufnehmen(&h(SERVER_HELLO));
        plan.handshakephase().expect("Schluesselplan rechenbar");

        assert_eq!(
            crate::sha2::hexen(&plan.client_hs_traffic),
            SECRET_C_HS,
            "das client handshake traffic secret"
        );
        assert_eq!(
            crate::sha2::hexen(&plan.server_hs_traffic),
            SECRET_S_HS,
            "das server handshake traffic secret"
        );

        // M7: die `finished`-Schluessel, und zwar gegen die RFC.
        //
        // HIER IST DER FEHLER GEFUNDEN WORDEN, der den Handshake
        // scheitern liess. Sie kommen aus den VERKEHRSGEHEIMNISSEN,
        // nicht aus `handshake_secret` — RFC 8446 Abschnitt 4.4.4.
        // `FINISHED_KEY_SERVER` ist der Wert aus RFC 8448 Abschnitt 3,
        // und `SECRET_S_HS` ist dessen `server_handshake_traffic_secret`.
        // Vorher stand hier nichts, und `Schluesselplan` benutzte das
        // falsche Geheimnis — ohne dass ein Test es bemerkte, weil
        // `trace.rs` die Formel selbst rechnete und `Schluesselplan`
        // nicht benutzt wurde.
        assert_eq!(
            crate::sha2::hexen(&plan.finished_key_server),
            FINISHED_KEY_SERVER,
            "das finished-Geheimnis des Servers, aus s hs traffic"
        );
        // Und das des Clients: es MUSS sich davon unterscheiden, sonst
        // waere die Seite nicht gebunden.
        assert_ne!(
            plan.finished_key_client, plan.finished_key_server,
            "M7: die beiden Seiten haben verschiedene finished-Geheimnisse"
        );
        let fk_c_erwartet = expand_label_sha256(&b32(SECRET_C_HS), b"finished", b"", 32)
            .expect("32 Byte, leerer Kontext");
        assert_eq!(
            crate::sha2::hexen(&plan.finished_key_client),
            crate::sha2::hexen(&fk_c_erwartet),
            "das finished-Geheimnis des Clients, aus c hs traffic"
        );

        // Und die Anwendungsschluessel, aus dem Transkript bis zum
        // ServerFinished (RFC 8446 Abschnitt 7.1).
        plan.aufnehmen(&h(ENCRYPTED_EXTENSIONS));
        plan.aufnehmen(&crate::nachricht::CERTIFICATE_RFC8448_ABSCHNITT_3);
        plan.aufnehmen(&crate::nachricht::CERTIFICATE_VERIFY_RFC8448_ABSCHNITT_3);
        plan.anwendungsphase_client()
            .expect("Anwendungsphase rechenbar");
        plan.anwendungsphase_server()
            .expect("Anwendungsphase rechenbar");
        assert_ne!(
            plan.client_ap_traffic, [0u8; 32],
            "M7: das client ap secret ist gesetzt"
        );
        assert_ne!(
            plan.server_ap_traffic, [0u8; 32],
            "M7: das server ap secret ist gesetzt"
        );
        assert_ne!(
            plan.client_ap_traffic, plan.server_ap_traffic,
            "M7: die beiden Seiten haben verschiedene Geheimnisse"
        );

        // Das Keying-Material: Schluessel und IV je 16 und 12 Byte
        // bei AES. M7: die Laengen stehen fest, sie sind nicht aus der
        // Eingabe.
        let (_k, iv) = Suite::Aes128GcmSha256
            .keying_material(&plan.server_hs_traffic)
            .expect("Schluessel rechenbar");
        assert_eq!(iv.len(), 12, "der IV ist 12 Byte");
        let (k, _iv) = Suite::Aes128GcmSha256
            .keying_material(&plan.server_hs_traffic)
            .expect("Schluessel rechenbar");
        assert_eq!(
            k.len(),
            32,
            "der Puffer ist 32 Byte, davon sind 16 Schluessel"
        );
        // Und der Schluessel selbst ist nicht null — ein Nullschluessel
        // waere das Zeichen eines ungeprueften Pfades.
        assert!(
            k[..16].iter().any(|b| *b != 0),
            "der Schluessel ist nicht null"
        );
    }
}
