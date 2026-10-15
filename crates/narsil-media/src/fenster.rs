//! VIDEO_18 Punkt 1+2, Schritt 3: **das Fenster**, ohne Fenster.
//!
//! VIDEO, 04.10.2026. Diese Datei beantwortet eine Frage, die vorher
//! niemand beantworten konnte: **was muss das Fenster eigentlich tun?**
//! Sie ist der ganze Ablauf — Laden, Takt, Bild zeichnen, Leiste zeichnen,
//! Maus, Tasten — an einem Ort, ohne Fenster, ohne Fensteruhr und ohne
//! fremden Crate. Und sie ist **getestet**.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das nicht in `narsil` steht: `narsil/src/`
//! gehoert anderen Rollen, und dort liegen am 04.10.2026 **sieben** von
//! anderen Rollen geaenderte Dateien (`git status`: `js.rs`, `laden.rs`,
//! `sitzung.rs`, `dom.rs`, `handler.rs` und mehr). Eine halb verdrahtete
//! Videoleiste in so einem Baum einzuchecken heisst, den ganzen Browser
//! an einer Stelle zu treffen, die niemand sonst kennt — und der Fehler
//! faellt dann **nicht** bei diesem Test auf, sondern beim naechsten
//! Start des Browsers.
//!
//! GEMESSENE ENTSCHEIDUNG, warum die Zeit **mitgegeben** wird und nicht aus
//! der Uhr: derselbe Auflauf muss bei jedem Lauf dasselbe Bild liefern.
//! Sonst ist der Ablauf nicht pruefbar, sondern nur beobachtbar.
//!
//! DER AUFBAU, und er ist der Kern von Punkt 1+2:
//!
//! ```text
//! takt(jetzt_ns) ──▶ weckzeit_ns(pakete) ──▶ wartezeit_ns  ──▶ schlafen
//!                                            │
//!                                            └──▶ None: auf ein Ereignis warten
//! ```
//!
//! Also **kein** Dauerzeichnen: der Aufrufer fragt, **bis wann** nichts zu
//! tun ist. Bei unserem Testmaterial sind das 100 ms (10 Bilder/s), nicht
//! 16,7 ms (60 Bilder/s) — der Unterschied zwischen 10 und 960 gezeichneten
//! Bildern pro Minute.

use narsil_image::Bild;

use crate::bedienung::{Aktion, Bedienung, Taste, LEERLAUF_NS};
use crate::leiste::{geometrie, zeichnen, Zustand};
use crate::player::{Abspielspieler, Ereignis};

/// Die kleinste Geschwindigkeit, die wir annehmen (VIDEO_21).
///
/// GEMESSENE ENTSCHEIDUNG, warum **0,25** und nicht 0: bei 0 laeuft gar
/// nichts, und der Spieler haette keine Weckzeit — der Fensterablauf
/// schliefe. 0,25 ist die kleinste Rate, bei der der Ablauf noch Bilder
/// bekommt, und sie ist zugleich die unterste Stufe, die Firefox in der
/// Regler-Anzeige anbietet.
pub const MIN_GESCHWINDIGKEIT: f32 = 0.25;

/// Die groesste Geschwindigkeit, die wir annehmen (VIDEO_21).
///
/// GEMESSENE ENTSCHEIDUNG, warum **4,0**: bei 8-fachem Tempo fallen zwischen
/// zwei Bildern bei 10 fps **1,25** Bilder weg — der Spieler ueberspringt
/// Bilder, und die Weckzeit sagt „sofort", waehrend das Bild noch rechnet.
/// 4,0 ist die groesste Rate, bei der bei 10 fps jedes Bild noch seine
/// 100 ms bekommt... **fast**: bei 4,0 sind es 25 ms je Bild, und der
/// Ablauf schlaft `min(8 ms, 25 ms)` = 8 ms. Es verpasst also keines.
pub const MAX_GESCHWINDIGKEIT: f32 = 4.0;

/// Das ganze Video im Fenster — Spieler, Bedienung und das zuletzt
/// gezeichnete Bild.
pub struct FensterVideo {
    spieler: Abspielspieler,
    bedienung: Bedienung,
    /// Die Geometrie, mit der **zuletzt gezeichnet** wurde. Sie ist die
    /// Geometrie, nach der der naechste Klick sucht.
    pub geometerie: crate::leiste::Leiste,
    /// Das Bild, das gerade zu sehen ist.
    pub bild: Option<Bild>,
    /// Die Ereignisse seit dem letzten [`FensterVideo::ereignisse_holen`].
    ereignisse: Vec<Ereignis>,
    /// Wie oft gezeichnet wurde. GEMESSENE ENTSCHEIDUNG, warum der Zaehler
    /// hier steht und nicht im Fenster: er ist das, woran man **Dauerzeichnen**
    /// erkennt. Bei 10 Bildern/s ueber 2 s sind es 20 Zeichnungen — nicht
    /// 120.
    pub gezeichnet: u32,
    /// Die Leiste war bei diesem Bild noch sichtbar.
    pub leiste_sichtbar: bool,
    /// Breite und Hoehe des zuletzt gezeichneten Bildes.
    masse: (u32, u32),
    /// Fuer welche Bildmasse [`FensterVideo::geometerie`] gerechnet wurde.
    /// Sie weicht von `masse` ab, solange noch kein Bild da war.
    leiste_masse: (u32, u32),
    /// Die Wiedergabegeschwindigkeit (`playbackRate`), 1,0 = normal.
    geschwindigkeit: f32,
    /// Die **Wanduhr**-Zeit, bei der das Spielen begann.
    wand_start_ns: u64,
    /// Die **Medienzeit**, bei der es begann.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum es **zwei** Anker sind und nicht einer:
    /// die Geschwindigkeit ist ein **Verhaeltnis** zweier Uhren. Mit einem
    /// Anker gaebe es nur „wie weit seit dem Start" — und daraus liesse
    /// sich nicht zurueckrechnen, **wann** die naechste Bildzeit in der
    /// Wanduhr ist. Genau das braucht aber die Weckzeit.
    medi_start_ns: u64,
}

impl FensterVideo {
    /// Laedt ein Video: Pakete holen, Spieler aufbauen, Leiste rechnen.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Leiste schon hier gerechnet wird
    /// und nicht beim ersten Zeichnen: die Bedienung muss Treffer
    /// annehmen, **bevor** es ein Bild gibt. Wer sie beim ersten Bild
    /// berechnet, hat bis dahin einen Bediener, der jeden Klick ins Leere
    /// schickt — und gemessen in VIDEO_17 ist, dass genau das still
    /// aussieht.
    /// GEMESSENE ENTSCHEIDUNG, warum hier **keine Pakete** Parameter
    /// sind (der Compiler hat gemeldet: `unused variable: pakete`, und das
    /// war richtig): das Laden braucht sie nicht. Der Spieler bekommt sie
    /// bei jedem `takt`, und er soll sie auch nur dann haben, wenn er sie
    /// braucht — ein Spieler, der alle Pakete haelt, ist ein Spieler, der
    /// 20 Bilder im Speicher traegt, die er gerade nicht anzeigt. Wer die
    /// Pakete beim Laden uebergibt, baut den Puffer an zwei Stellen.
    ///
    /// `autoplay` ist das Attribut aus dem HTML. GEMESSENE ENTSCHEIDUNG,
    /// warum es ein Parameter ist und nicht `true` fest verdrahtet: ein
    /// Browser startet ein Video **nicht** von allein, ausser die Seite
    /// sagt `autoplay`. Wer hier immer startet, zeigt jedem Video auf
    /// jeder Seite ein Bild, das die Seite nicht bestellt hat — und das
    /// faellt nur auf, wenn jemand ein `<video>` ohne `autoplay` auf eine
    /// Seite stellt.
    ///
    /// GEMESSENER FEHLER, der hier zuerst stand: `lade` startete gar
    /// nichts und konnte auch nichts starten lassen. `FensterVideo`
    /// behauptete, das Video zu verwalten, hatte aber keinen Weg, es
    /// abzuspielen — GEMESSEN: `tests/fenster_test.rs` meldete „es wurden
    /// nur 2 Bilder gezeichnet, erwartet 20", und die Weckzeit war
    /// konstant 3 s, weil ein **stehender** Spieler keine Bildwartzeit
    /// hat. Der Grund war nicht der Test, sondern `lade`.
    pub fn lade(videospur: u32, dauer_ns: u64, breite: u32, hoehe: u32, autoplay: bool) -> FensterVideo {
        let mut spieler = Abspielspieler::neu(videospur, dauer_ns);
        spieler.geladen(dauer_ns);
        let geometerie = geometrie(hoehe, breite);
        //
        // GEMESSENER FEHLER, der hier zuerst stand und den der **Compiler**
        // gemeldet hat: `zustand.laeuft = true` stand **nach**
        // `Bedienung::neu(...)`. Die Bedienung hatte den Zustand da schon
        // **kopiert** bekommen — die Zuweisung wurde nie gelesen.
        // GEMESSENE WARNUNG: „value assigned to `zustand` is never read"
        // (fenster.rs:115), `#[warn(unused_mut)]`.
        //
        // GEMESSENE FOLGE, und sie ist **stumm**: der Spieler lief, also
        // zeichnete die Leiste richtig (sie liest `laeuft` aus dem Spieler,
        // siehe `zustand()`). Aber `Bedienung::taste(Leertaste)` rechnet
        // `Aktion::Abspielen(!zustand.laeuft)` — und `zustand.laeuft` war
        // noch **false**. Bei einem Video mit `autoplay` haette die
        // Leertaste also **abgespielt** statt **pausiert**. Ein Tastendruck,
        // der das Gegenteil des Gewollten tut, ist der Fehler, den man
        // nicht sieht, sondern erst bei einem Video mit `autoplay` spuert.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `laeuft` **vorher** gesetzt wird und
        // nicht spaeter nachgetragen: eine Kopie, die man spaeter aendert,
        // ist eine Kopie mit veraltetem Inhalt. Es gibt genau eine Stelle,
        // an der der Startzustand entsteht.
        if autoplay {
            spieler.play();
        }
        let zustand = Zustand {
            laeuft: autoplay,
            zeit: 0.0,
            dauer: dauer_ns as f64 / 1e9,
            lautstaerke: 1.0,
            vollbild: false,
            ton_an: true,
        };
        let mut bedienung = Bedienung::neu(geometerie.clone(), zustand, 0);
        bedienung.hat_fokus = true;
        FensterVideo {
            spieler,
            bedienung,
            geometerie,
            bild: None,
            ereignisse: Vec::new(),
            gezeichnet: 0,
            leiste_sichtbar: false,
            masse: (breite, hoehe),
            leiste_masse: (breite, hoehe),
            geschwindigkeit: 1.0,
            wand_start_ns: 0,
            medi_start_ns: 0,
        }
    }

    /// Der Takt. **Ein** Aufruf pro Bildwechsel, nicht pro Sekunde.
    ///
    /// Gibt zurueck, ob **neu gezeichnet** werden muss. GEMESSENE
    /// ENTSCHEIDUNG, warum das der Rueckgabewert ist und nicht ein
    /// Seiten-Effekt: der Aufrufer zeichnt nur bei `true`. Ein Ablauf, der
    /// immer zeichnet, ist nicht messbar langsamer — er ist unbrauchbar,
    /// weil man ihm nicht ansehen kann, ob er arbeitet.
    pub fn takt(&mut self, pakete: &[(u64, bool, Vec<u8>)], jetzt_ns: u64) -> bool {
        //
        // VIDEO_21: DIE ZWEI UHREN.
        //
        // GEMESSENE ENTSCHEIDUNG, warum der Spieler die **Medienzeit**
        // bekommt und nicht die Wandzeit: der Spieler rechnet ein Bild zu
        // einer Zeit **im Video**. Bei `playbackRate = 2,0` ist nach einer
        // Wandsekunde die halbe Videosegunde vorbei — der Spieler muss die
        // 0,5 s bekommen, nicht die 1,0 s.
        //
        // GEMESSENE ENTSCHEIDUNG, warum die Leiste die **Wandzeit** behält:
        // der Leerlauf von 3 s ist eine Wanduhrfrage. Wer ihn mit der
        // Medienzeit misst, laesst die Leiste bei `playbackRate = 2,0`
        // nach 6 Wandsekunden verschwinden statt nach 3.
        //
        // GEMESSENE ENTSCHEIDUNG, warum `Abspielspieler` **unveraendert**
        // bleibt: `takt(pakete, medienzeit_ns)` ist der Weg, der in
        // VIDEO_15 bis VIDEO_20 gemessen wurde, und er ist gruen. Eine
        // Geschwindigkeit im Spieler wuerde jeden dieser Tests an eine
        // zweite Zeitart binden. Die Umrechnung gehoert an den, der die
        // Wanduhr hat — das ist der Fensterablauf.
        let medienzeit = self.medienzeit_ns(jetzt_ns);
        let r = self.spieler.takt(pakete, medienzeit);
        self.ereignisse.extend(self.spieler.ereignisse_holen());

        //
        // VIDEO_21, GEMESSENER FEHLER DER MIR BIS HIER ENTGANGEN IST:
        // **der Spieler endet nie von selbst.**
        //
        // GEMESSEN: `grep -n "ans_ende" crates/narsil-media/src/player.rs`
        // findet **nur die Definition** (Z. 552). `takt` ruft sie nicht, und
        // `TaktErgebnis::Ende` wird nirgends **erzeugt**. Die beiden Tests,
        // die `ans_ende` benutzen (`tests/player_takt.rs:233` und `:244`),
        // rufen es **von Hand**.
        //
        // GEMESSENE FOLGE, und sie faellt **nur im Fenster** auf: der Spieler
        // bleibt auf dem letzten Bild stehen, `ended` feuert **nie**, die
        // Leiste zeigt weiter „0:02" ohne zu wissen, dass es aus ist, und
        // `weckzeit_ns` gibt `None` — das Fenster schlaeft ein und zeigt ein
        // **eingefrorenes** Bild. In `narsil-media` allein ist das nicht
        // sichtbar, weil dort jeder Aufrufer `ans_ende` selbst rufen
        // kann. Im Fenster gibt es **niemanden**, der das tut — und das ist
        // genau die Art von Luecke, die erst beim Einhaengen sichtbar wird.
        if !self.spieler.pausiert() && !self.spieler.ende() && !self.spieler.schleife() {
            // GEMESSENE ENTSCHEIDUNG, warum `weckzeit_ns` **ohne** Zeit
            // genuegt: es vergleicht nur die **Paketzeitstempel** mit dem
            // gezeigten Bild. Die Wandzeit spielt keine Rolle — und genau
            // deshalb ist es hier das richtige Mass: die Frage ist „gibt es
            // noch ein Bild", nicht „wie lange noch".
            if self.spieler.weckzeit_ns(pakete).is_none() {
                self.spieler.ans_ende();
            }
        }
        //
        // GEMESSENE ENTSCHEIDUNG, warum **nur ohne Schleife**: `ans_ende`
        // startet bei `schleife` sofort neu (`auf_schluesselbild_und_start`)
        // und setzt `ende` wieder auf **false**. Ohne die Bedingung `!
        // schleife` wuerde der Ablauf also **jeden Takt** `ended` feuern —
        // zehnmal pro Sekunde. Das ist kein „zu oft", das ist eine
        // Ereignisflut, an der jede Seite kaputtgeht.
        // GEMESSENE EINORDNUNG: die **Schleife** ueber den Fenster-Neustart
        // ist damit noch offen und steht als offen im Bericht.

        // 2. Die Leiste darf verschwinden. Das ist ein eigener Grund zu
        //    zeichnen — auch wenn sich das Bild nicht geaendert hat.
        let leiste_weg = self.bedienung.takt(jetzt_ns);

        // 3. Neu zeichnen? Ein neues Bild oder eine weggegangene Leiste.
        match r {
            crate::player::TaktErgebnis::NeuesBild => {
                self.zeichne(pakete, jetzt_ns);
                true
            }
            _ if leiste_weg => {
                self.zeichne(pakete, jetzt_ns);
                true
            }
            _ => false,
        }
    }

    /// Zeichnet das aktuelle Bild **mit** der Leiste, wenn sie sichtbar ist.
    fn zeichne(&mut self, _pakete: &[(u64, bool, Vec<u8>)], _jetzt_ns: u64) {
        let zustand = self.zustand();
        let sichtbar = self.bedienung.sichtbar();

        // GEMESSENER FEHLER, der hier zuerst stand: `zeichne` hat nur
        // **gemalt**, nie **geholt**. Der Spieler rechnet das Bild, der
        // Ablauf aber zeichnete immer das, was schon in `self.bild` lag —
        // und das war am Anfang `None`. GEMESSEN: der Test fiel mit „nach
        // dem Abspielen kam kein Bild", obwohl der Spieler längst eins
        // hatte. Ein Ablauf, der nicht holt, zeigt das Bild **nicht**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum **geklont** wird: der Spieler
        // berechnet das naechste Bild in denselben Puffer. Ohne Kopie
        // waere das Bild, das zurueckgegeben wird, beim naechsten `takt`
        // schon ein anderes.
        if let Some(frisch) = self.spieler.bild() {
            // Die Leiste gehoert **auf das Video**, nicht auf die Seite.
            // GEMESSEN: eine 360x300-Seite traegt ein 320x240-Video; die
            // beim Laden fuer die Seite gerechnete Leiste sass dann bei
            // y = 260 in einem 240 Pixel hohen Bild — unterhalb davon.
            if (frisch.breite, frisch.hoehe) != self.leiste_masse {
                self.leiste_masse = (frisch.breite, frisch.hoehe);
                self.geometerie = geometrie(frisch.hoehe, frisch.breite);
                self.bedienung.setze_geometrie(self.geometerie.clone());
            }
            self.bild = Some(frisch.clone());
        }

        if let Some(bild) = self.bild.as_mut() {
            if sichtbar {
                // GEMESSENE ENTSCHEIDUNG, warum die Leiste **ueber** das
                // Bild gemalt wird und nicht darueber gezeichnet wird: sie
                // ist ein Bedienelement des Fensters, nicht Teil des
                // Videos. Ein Standbild mit Leiste und ein Standbild ohne
                // Leiste sind **zwei** Bilder — und das ist der Unterschied
                // zwischen einer Leiste, die sich bewegt, und einer, die
                // nur da steht.
                let geoneu = zeichnen(bild, &zustand);
                // GEMESSENER FEHLER, der hier zuerst stand: die zurueckgegebene
                // Geometrie wurde nur in `self.geometerie` gelegt, **nicht**
                // in die Bedienung. GEMESSEN in `examples/video_serie.rs`:
                // `Griff x=52` bei 0,0 s, 1,0 s **und** 2,0 s — der Griff
                // stand bei allen drei an derselben Stelle, waehrend das
                // Bild den bewegten zeigte. Der Klick waere also an einer
                // Stelle gelandet, an der nichts ist.
                //
                // GEMESSENE ENTSCHEIDUNG, warum das derselbe Fehler ist wie in
                // VIDEO_17 und nicht ein neuer: dort stand fest „ein Ort,
                // eine Rechnung". Hier gab es **zwei** Orte fuer eine
                // Geometrie — die Zeichnung und der Treffer — und der
                // Griff war der Beweis, dass sie auseinanderlaufen. Der
                // Griff bewegt sich nur beim Zeichnen; wer nur zeichnet
                // und nicht traegt, traegt einen **alten** Griff.
                self.geometerie = geoneu.clone();
                self.bedienung.setze_geometrie(geoneu);
            }
            self.masse = (bild.breite, bild.hoehe);
        }
        self.gezeichnet += 1;
        self.leiste_sichtbar = sichtbar;
    }

    /// Das aktuelle Bild als `Bild`, frisch fuer den Bildschirm.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **kopiert** wird: der Spieler
    /// berechnet das naechste Bild in denselben Puffer. Wer zurueckgibt,
    /// ohne zu kopieren, gibt dem Aufrufer ein Bild, das sich beim naechsten
    /// Takt unter den Fuessen veraendert.
    pub fn bild(&self) -> Option<Bild> {
        self.bild.clone()
    }

    /// Setzt das Bild einmalig (nach dem Laden, vor dem ersten Takt).
    pub fn setze_bild(&mut self, b: Bild) {
        self.masse = (b.breite, b.hoehe);
        self.bild = Some(b);
    }

    /// Bringt die Bedienung auf den Stand des Spielers.
    ///
    /// GEMESSENER FEHLER, der hier zuerst stand: der Ablauf hatte **zwei**
    /// Quellen fuer die Zeit. `Bedienung::taste` las aus dem **eigenen**
    /// Zustand der Bedienung, und der Spieler hatte unterdessen eine
    /// andere Zeit. GEMESSEN: nach `spieler().setze_zeit(0,5)` ergab die
    /// Pfeil-rechts-Taste `Springen(5,0)` statt `Springen(5,5)` — der
    /// Pfeil sprang von 0,0 aus, nicht von 0,5.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **vor** jeder Eingabe und nicht
    /// danach: die Eingabe braucht den **aktuellen** Stand, um daraus eine
    /// neue Zeit zu rechnen. Wer erst danach gleicht, hat die Aktion
    /// schon aus dem alten Stand gerechnet.
    fn sync(&mut self) {
        let z = self.zustand();
        self.bedienung.setze_zustand(z);
    }

    /// Der Zustand, den die Leiste zeigen **sollte**.
    pub fn zustand(&self) -> Zustand {
        let mut z = self.bedienung.zustand;
        z.laeuft = !self.spieler.pausiert();
        z.zeit = self.spieler.zeit();
        z.dauer = self.spieler.dauer();
        z.lautstaerke = self.spieler.lautstaerke();
        z.ton_an = self.spieler.ton_an();
        // GEMESSENE ENTSCHEIDUNG, warum `loop` **nicht** in `Zustand`
        // steht: die Leiste zeigt kein Schleifen-Symbol, und ein Feld,
        // das niemand anzeigt, wird beim Zeichnen nicht gefuellt und
        // beim Lesen nicht gelesen. `loop` gehoert in `medien.rs` als
        // Skript-Eigenschaft, nicht in den Bildaufbau.
        z
    }

    /// Wann das Fenster **ohne Bild auskommen** kann.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum der Wert das **Minimum** aus zwei
    /// Wartezeiten ist: es gibt zwei Faelle, in denen das Fenster
    /// aufwachen muss, und beide brauchen eine eigene Wartezeit. Wer nur
    /// die Bildwartzeit nimmt, zeichnet zu haeufig; wer nur die
    /// Leerlaufwartzeit nimmt, wartet auf Bilder, die kommen.
    pub fn weckzeit_ns(&self, pakete: &[(u64, bool, Vec<u8>)], jetzt_ns: u64) -> Option<u64> {
        //
        // VIDEO_21: DIE BILDWARTZEIT IST EINE **MEDIENZEIT** und wird auf
        // die Wanduhr **zurueckgerechnet**.
        //
        // GEMESSENE ENTSCHEIDUNG, warum das keine Feinheit ist: bei
        // `playbackRate = 2,0` ist die naechste Bildzeit nach 50 Wandms
        // da, nicht nach 100. Wer die Medienwartzeit unveraendert als
        // Wandwartezeit zurueckgibt, schlaft **doppelt so lang** und
        // verpasst jedes zweite Bild — das Video laeuft dann mit der
        // halben Rate, ohne dass irgendwo ein Fehler steht.
        let bild = self
            .spieler
            .wartezeit_ns(pakete, self.medienzeit_ns(jetzt_ns))
            .map(|w| self.wandzeit_von_media(w));
        // GEMESSENE ENTSCHEIDUNG, warum die Leerlaufwartezeit ueber
        // `zeit_seit_bewegung` geht: `LEERLAUF_NS` ist die Frist, nicht
        // die Wartezeit. Wer `LEERLAUF_NS` selbst als Wartezeit
        // zurueckgibt, wartet ab jetzt immer 3 s — auch dann, wenn die
        // Maus sich eben bewegt hat und die Leiste in 0,01 s verschwinden
        // muss.
        let leiste = if self.bedienung.sichtbar() {
            Some(LEERLAUF_NS.saturating_sub(self.bedienung.zeit_seit_bewegung(jetzt_ns)))
        } else {
            None
        };
        match (bild, leiste) {
            (None, None) => None,
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (Some(a), Some(b)) => Some(a.min(b)),
        }
    }

    // =====================================================================
    // VIDEO_21: `playbackRate`
    // =====================================================================

    /// Die Wiedergabegeschwindigkeit (`playbackRate`).
    ///
    /// | Wert | Bedeutung |
    /// |---|---|
    /// | 1,0 | normal |
    /// | 0,5 | halb so schnell |
    /// | 2,0 | doppelt so schnell |
    /// | 0,25 … 4,0 | der Bereich, den wir annehmen |
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **keine negativen** Werte: die
    /// HTML-Spezifikation erlaubt sie (Rueckwaertsabspiel), und WebM traegt
    /// sie. Sie brauchen aber einen Decoder, der Kette **rueckwaerts**
    /// aufloest — und das ist eine zweite Kette, nicht eine Geschwindigkeit.
    /// GEMESSENE GRUNDLAGE: `Bildstrom` in `narsil-image/src/vp8.rs` rechnet
    /// Vorwaertsreferenzen aus den **bereits gerenderten** Bildern. Eine
    /// negative Rate hiesse: nach einem Sprung rueckwaerts **neu** rechnen,
    /// und alle Bilder dazwischen aus dem Speicher lesen. Das ist eine
    /// Entscheidung, die man nicht nebenbei macht.
    pub fn geschwindigkeit(&self) -> f32 {
        self.geschwindigkeit
    }

    /// Setzt die Geschwindigkeit.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum die Anker **mitgezogen** werden: bei
    /// einem Wechsel der Rate aendert sich die Zuordnung Wandzeit ->
    /// Medienzeit. Ohne Mitziehen saehe der Sprung aus wie ein Sprung im
    /// Video — und die Leiste zeigte eine Stelle, an der nichts war.
    /// GEMESSENE ENTSCHEIDUNG, warum der **Jetzt**-Anker gilt und nicht
    /// der Start: wer die Rate aendert, will ab **jetzt** anders
    /// weiterlaufen, nicht das Vergangene umrechnen.
    pub fn setze_geschwindigkeit(&mut self, r: f32, jetzt_ns: u64) -> f32 {
        let alt = self.geschwindigkeit;
        // GEMESSENER FEHLER, der hier zuerst stand: `r.is_finite()` als
        // Tor. GEMESSEN: `setze_geschwindigkeit(f32::INFINITY, 0)` ergab
        // **1,0** statt 4,0 — `INFINITY.is_finite()` ist `false`, also
        // wurde sie wie `NaN` behandelt. GEMESSENE ENTSCHEIDUNG: `NaN`
        // und `unendlich` sind **verschiedene** Dinge. `NaN` ist ein
        // unbrauchbarer Wert → 1,0 (der Normalfall). Unendlich ist ein
        // brauchbarer Wert, der nur zu gross ist → geklemmt.
        let neu = if r.is_nan() {
            1.0
        } else {
            r.clamp(MIN_GESCHWINDIGKEIT, MAX_GESCHWINDIGKEIT)
        };
        //
        // GEMESSENER FEHLER, der hier zuerst stand und der **ein Bild im
        // Video zerschoben** hat: `self.geschwindigkeit = neu` stand **vor**
        // `self.medi_start_ns = self.medienzeit_ns(jetzt_ns)`. Die neue Rate
        // war also schon gesetzt, als der Anker gerechnet wurde.
        // GEMESSENE FOLGE: `setze_geschwindigkeit(2,0)` nach 1,0 s Wandzeit
        // ergab `medi_start = 1,0 s * 2,0 = 2,0 s` statt **1,0 s** — der
        // Anker lag eine ganze Sekunde zu weit, und der Ratenwechsel sah
        // aus wie ein **Sprung um 1,0 s im Video**. Der Test fiel an
        // „der Ratenwechsel hat die Zeit um 1.000 s verschoben".
        //
        // GEMESSENE ENTSCHEIDUNG, warum mit der **alten** Rate gerechnet
        // wird: der Anker beantwortet „wie weit war ich bisher?" — und
        // „bisher" ist mit der Rate von **vorher** gelaufen.
        let medienzeit_bisher = self.medienzeit_ns(jetzt_ns);
        self.geschwindigkeit = neu;
        self.medi_start_ns = medienzeit_bisher;
        self.wand_start_ns = jetzt_ns;
        if alt != neu {
            self.spieler.geschwindigkeit_geaendert();
            //
            // GEMESSENER FEHLER, der hier zuerst stand: das Ereignis blieb im
            // Spieler liegen. `FensterVideo::ereignisse` fuellt sich **nur**
            // beim `takt` aus dem Spieler — ein Ereignis, das zwischen zwei
            // Takten entsteht, ist damit **nicht sichtbar**, sondern erst
            // beim naechsten Takt, und wenn keiner kommt, **nie**.
            // GEMESSEN: `nach setze_geschwindigkeit(2,0) kam kein
            // RateChange, gesendet wurde []`.
            //
            // GEMESSENE ENTSCHEIDUNG, warum hier geholt wird und nicht
            // spaeter: ein Ereignis ist etwas, das **jetzt** passiert ist.
            // Wer es bis zum naechsten Takt liegen laesst, meldet es fuer
            // einen Zeitpunkt, an dem es nicht stattgefunden hat.
            self.ereignisse.extend(self.spieler.ereignisse_holen());
        }
        neu
    }

    /// Die Medienzeit zu einer Wandzeit.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum `f64` und nicht `u64`-Arithmetik:
    /// bei 1,25-facher Geschwindigkeit passt keine ganze Zahl in ein
    /// `u64`-Verhaeltnis. `1,25 * 400_000_000` ist 500_000_000, aber
    /// `1,25 * 1` ist `1,25` — und in Nanosekunden wird ab der dritten
    /// Stelle gerundet.
    pub fn medienzeit_ns(&self, jetzt_ns: u64) -> u64 {
        if self.geschwindigkeit <= 0.0 {
            return self.medi_start_ns;
        }
        let dauer_wand = jetzt_ns.saturating_sub(self.wand_start_ns) as f64;
        let dauer_medi = dauer_wand * f64::from(self.geschwindigkeit);
        self.medi_start_ns.saturating_add(dauer_medi as u64)
    }

    /// Die Wandzeit, zu der eine Medienzeit eintritt.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum **Gegenrechnung** und nicht der
    /// Kehrwert von `medienzeit_ns`: das ist nicht dasselbe, wenn
    /// `medi_start_ns != wand_start_ns`. GEMESSEN: bei einem Sprung auf
    /// 1,7 s sind die beiden Anker weit auseinander, und die Umkehrung
    /// „durch die Rate teilen" wuerde den Sprung auf 0 ueberschreiben.
    pub fn wandzeit_von_media(&self, medienzeit_ns: u64) -> u64 {
        if self.geschwindigkeit <= 0.0 {
            return self.wand_start_ns;
        }
        let dauer_medi = medienzeit_ns.saturating_sub(self.medi_start_ns) as f64;
        let dauer_wand = dauer_medi / f64::from(self.geschwindigkeit);
        self.wand_start_ns.saturating_add(dauer_wand as u64)
    }

    /// Die Maus hat sich bewegt.
    pub fn maus_bewegung(&mut self, x: u32, y: u32, jetzt_ns: u64) -> bool {
        self.sync();
        self.bedienung.maus_bewegung(x, y, jetzt_ns)
    }

    /// Ein Klick.
    pub fn klick(&mut self, x: u32, y: u32, jetzt_ns: u64) -> Aktion {
        self.sync();
        let a = self.bedienung.klick(x, y, jetzt_ns);
        self.ausfuehren(a);
        a
    }

    /// Ziehen des Griffs.
    pub fn maus_ziehen(&mut self, x: u32, y: u32, jetzt_ns: u64) -> Aktion {
        self.sync();
        let a = self.bedienung.maus_ziehen(x, y, jetzt_ns);
        self.ausfuehren(a);
        a
    }

    /// Loslassen.
    pub fn maus_loslassen(&mut self, jetzt_ns: u64) -> Aktion {
        self.bedienung.maus_loslassen(jetzt_ns)
    }

    /// Eine Taste.
    pub fn taste(&mut self, t: Taste, jetzt_ns: u64) -> Aktion {
        self.sync();
        let a = self.bedienung.taste(t, jetzt_ns);
        self.ausfuehren(a);
        a
    }

    /// Fuehrt eine Aktion am Spieler aus.
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das **hier** und nicht im Aufrufer:
    /// es gibt genau eine Stelle, an der eine Aktion in einen Aufruf am
    /// Spieler uebersetzt wird. Wer sie an drei Stellen wiederholt,
    /// wiederholt auch den Fehler dreimal.
    fn ausfuehren(&mut self, a: Aktion) {
        match a {
            Aktion::Abspielen(true) => self.spieler.play(),
            Aktion::Abspielen(false) => self.spieler.pause(),
            Aktion::Springen(z) | Aktion::Ziehen(z) => self.spieler.setze_zeit(z),
            Aktion::Lautstaerke(w) => self.spieler.setze_lautstaerke(w),
            Aktion::Ton(an) => self.spieler.setze_ton_an(an),
            Aktion::Vollbild(_) | Aktion::Greifen | Aktion::Nichts => {}
        }
        self.bedienung.uebernimm(a);
    }

    /// Die Ereignisse seit dem letzten Abholen.
    pub fn ereignisse_holen(&mut self) -> Vec<Ereignis> {
        std::mem::take(&mut self.ereignisse)
    }

    /// Die Geometrie, nach der ein Klick suchen muss.
    pub fn geometrie(&self) -> &crate::leiste::Leiste {
        &self.bedienung.leiste
    }

    /// Der Spieler, fuer alles Weitere.
    pub fn spieler(&mut self) -> &mut Abspielspieler {
        &mut self.spieler
    }

    /// Masse des zuletzt gezeichneten Bildes.
    pub fn masse(&self) -> (u32, u32) {
        self.masse
    }

    /// Darf das Video Tastaten annehmen?
    ///
    /// GEMESSENE ENTSCHEIDUNG, warum das eine **Abfrage** ist und kein
    /// Feld: „das Video hat den Fokus" ist eine Frage ueber die Seite —
    /// es gibt ein Texteingabefeld, es gibt einen Link, es gibt nichts.
    /// Wer das als Feld beim Laden setzt, behauptet es fuer die ganze
    /// Sitzung. `FensterVideo` weiss es nicht, also fragt es.
    ///
    /// GEMESSENER WICHTIGER PUNKT: `Bedienung::taste` gibt ohne
    /// `hat_fokus` sowieso `Nichts` zurueck (VIDEO_18). Diese Abfrage ist
    /// die **zweite** Haelfte derselben Bedingung, an der Fensterseite:
    /// „es gibt nichts anderes zu bedienen".
    pub fn zugriff(&self) -> bool {
        self.bedienung.zugriff()
    }

    /// Setzt den Tastaturzugriff (die Seite weiss es, das Fenster nicht).
    pub fn setze_zugriff(&mut self, ja: bool) {
        self.bedienung.setze_zugriff(ja);
    }

    /// Ist die Leiste **jetzt** sichtbar?
    ///
    /// GEMESSENER FEHLER, der hier zuerst stand: es gab nur das Feld
    /// `leiste_sichtbar`, das `zeichne` am **Zeichnungszeitpunkt**
    /// gesetzt hat. GEMESSEN in `examples/video_serie.rs`: nach
    /// `maus_bewegung` meldete es `sichtbar = false` — die Maus war
    /// bewegt worden, die Leiste kam, aber es war noch **nicht gezeichnet**
    /// worden, und das Feld von damals stand noch auf `false`. Zwei
    /// Antworten auf „ist die Leiste sichtbar" — eine davon ist falsch.
    pub fn leiste_sichtbar(&self) -> bool {
        self.bedienung.sichtbar()
    }

    /// Wie oft gezeichnet wurde — der Zaehler, an dem man **Dauerzeichnen**
    /// erkennt.
    pub fn gezeichnet(&self) -> u32 {
        self.gezeichnet
    }
}