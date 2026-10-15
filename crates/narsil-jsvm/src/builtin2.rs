// BEGRUENDUNG fuer `dead_code`: Teile dieser Datei sind eine
// Uebernahme aus narsil-js (dort genutzt, in der VM-Bauart noch nicht)
// oder sind vorbereitete Oberflaeche (statische Felder, `exec`,
// Generator-Fortsetzen). Sie zu benennen, statt sie zu loeschen,
// haelt die Datei lesbar und die Nummern stabil. GEMESSEN am
// 30.09.2026, im Bericht `team/BERICHT_JS_W6.txt` benannt.
#![allow(dead_code)]
//! Eingebaute Objekte, Teil 2 (J4): String, Number, Boolean, Math, JSON,
//! Fehler, Map, Set, Date, Promise, RegExp, console.

use crate::builtin::{Nativ, ParamsLauf, UrlFeld};
use crate::fehler::JsFehler;
use crate::heap::{Eigenschaft, GVal, HObj, Heap, HeapRef};
use crate::interp::{Engine, Stoerung};

impl Engine {
    pub(crate) fn teil2_aufbauen(
        &mut self,
        math: HeapRef,
        json: HeapRef,
        console: HeapRef,
        performance: HeapRef,
        map_ctor: HeapRef,
        set_ctor: HeapRef,
        date_ctor: HeapRef,
        prom_ctor: HeapRef,
        regex_ctor: HeapRef,
    ) {
        let reg = |e: &mut Engine, ziel: HeapRef, tabelle: &[(&str, Nativ)]| {
            for (n, nat) in tabelle {
                let f = e.nativ_anlegen(n, nat.clone(), e.proto_funktion);
                let o = e.heap.holen_mut(ziel);
                // Ziele sind Gegenstaende (Protos) ODER Nativ-Ctors.
                let map = match o {
                    HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
                    HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                        Some(eigenschaften)
                    }
                    // JS2 (03.10.2026, 08:10): **`Array.prototype` ist jetzt ein
                    // Feld** (ECMA-262 23.1.2.1: „`Array.prototype` is an array
                    // exotic object"), und ein Feld haelt seine **Nicht-Index**-
                    // Eigenschaften in `rest` — dieselbe Trennung, die
                    // `setze_eigen` macht. Ohne diesen Arm war `reg` hier
                    // wirkungslos, und **alle 40 `Array.prototype`-Methoden
                    // waren weg** (`typeof [].map` -> `undefined`).
                    //
                    // GEMESSEN: das war keine theoretische Sorge, sondern der
                    // Messwert der ersten Fassung (08:05). Deshalb gehoert
                    // diese Zeile in den Bericht und nicht in eine Fussnote:
                    // **eine Typaenderung am Prototyp trifft zuerst die
                    // Registrierung, nicht die Methoden.**
                    HObj::Feld { rest, .. } => Some(rest),
                    _ => None,
                };
                if let Some(eigenschaften) = map {
                    eigenschaften.insert(
                        n.to_string(),
                        Eigenschaft {
                            konfigurierbar: true,
                            wert: GVal::Ref(f),
                            getter: None,
                            setter: None,
                            aufzählbar: false,
                            beschreibbar: true,
                        },
                    );
                }
            }
        };
        // String statisch + Prototyp.
        reg(
            self,
            self.ctor_text,
            &[
                ("fromCharCode", Nativ::TextAusZeichen),
                ("fromCodePoint", Nativ::TextAusPunkten),
                ("raw", Nativ::TextRoh),
            ],
        );
        // Release 14.10.2026: `String.prototype[@@iterator]` (see `n_text_iter`).
        if let Ok(sym) = self.n_symbol_bekannt(GVal::Undefiniert, &[], Nativ::SymIterator)
            && let Ok(schl) = self.schluessel_text(sym)
        {
            let f = self.nativ_anlegen("[Symbol.iterator]", Nativ::TextIterWerte, self.proto_funktion);
            let _ = self.eigenschaft_nicht_aufzaehlbar(self.proto_text, &schl, GVal::Ref(f));
        }
        reg(
            self,
            self.proto_text,
            &[
                ("charAt", Nativ::TextZeichenAn),
                ("charCodeAt", Nativ::TextZeichenKode),
                ("codePointAt", Nativ::TextPunktKode),
                ("indexOf", Nativ::TextIndexVon),
                ("lastIndexOf", Nativ::TextLetzterIndex),
                ("includes", Nativ::TextEnthaelt),
                ("startsWith", Nativ::TextBeginnt),
                ("endsWith", Nativ::TextEndet),
                ("slice", Nativ::TextScheibe),
                ("substring", Nativ::TextTeil),
                ("substr", Nativ::TextTeilAlt),
                ("split", Nativ::TextTeilen),
                ("trim", Nativ::TextStutzen),
                ("normalize", Nativ::TextNormalisieren),
                ("trimStart", Nativ::TextStutzenStart),
                ("trimEnd", Nativ::TextStutzenEnde),
                ("toUpperCase", Nativ::TextGross),
                ("toLowerCase", Nativ::TextKlein),
                ("toString", Nativ::TextProtoText),
                ("valueOf", Nativ::TextWertVon),
                ("repeat", Nativ::TextWiederholen),
                ("padStart", Nativ::TextFuellenStart),
                ("padEnd", Nativ::TextFuellenEnde),
                ("replace", Nativ::TextErsetzen),
                ("replaceAll", Nativ::TextErsetzenAlle),
                // W47 (QUIC, 04.10.2026): `String.prototype.matchAll`.
                // **`node 22`: `length` 1** — es nimmt `(regexp)` und sonst
                // nichts; die Zahl steht in `parameterzahl`.
                ("matchAll", Nativ::TextMatchAlle),
                ("match", Nativ::TextTreffer),
                ("search", Nativ::TextSuchen),
                ("concat", Nativ::TextVerketten),
                ("at", Nativ::TextAn),
                ("localeCompare", Nativ::TextVergleich),
            ],
        );
        // Number statisch + Prototyp.
        reg(
            self,
            self.ctor_zahl,
            &[
                ("isNaN", Nativ::ZahlIstNaN),
                ("isFinite", Nativ::ZahlIstEndlich),
                ("isInteger", Nativ::ZahlIstGanz),
                ("isSafeInteger", Nativ::ZahlIstSicher),
                ("parseInt", Nativ::ParseInt),
                ("parseFloat", Nativ::ParseFloat),
            ],
        );
        reg(
            self,
            self.proto_zahl,
            &[
                ("toString", Nativ::ZahlProtoText),
                ("toLocaleString", Nativ::ZahlLokal),
                ("toFixed", Nativ::ZahlFest),
                ("toExponential", Nativ::ZahlExpo),
                ("toPrecision", Nativ::ZahlGenau),
                ("valueOf", Nativ::ZahlWertVon),
            ],
        );
        // Boolean.
        reg(
            self,
            self.proto_bool,
            &[
                ("toString", Nativ::BoolProtoText),
                ("valueOf", Nativ::BoolWertVon),
            ],
        );
        // Math.
        reg(
            self,
            math,
            &[
                ("abs", Nativ::MatheAbs),
                ("floor", Nativ::MatheBoden),
                ("ceil", Nativ::MatheDecke),
                ("round", Nativ::MatheRund),
                ("trunc", Nativ::MatheKapp),
                ("sign", Nativ::MatheVor),
                ("sqrt", Nativ::MatheWurzel),
                ("cbrt", Nativ::MatheKubik),
                ("pow", Nativ::MathePotenz),
                ("exp", Nativ::MatheExp),
                ("expm1", Nativ::MatheExpm1),
                ("log", Nativ::MatheLog),
                ("log2", Nativ::MatheLog2),
                ("log10", Nativ::MatheLog10),
                ("log1p", Nativ::MatheLog1p),
                ("sin", Nativ::MatheSin),
                ("cos", Nativ::MatheCos),
                ("tan", Nativ::MatheTan),
                ("asin", Nativ::MatheAsin),
                ("acos", Nativ::MatheAcos),
                ("atan", Nativ::MatheAtan),
                ("atan2", Nativ::MatheAtan2),
                ("sinh", Nativ::MatheSinh),
                ("cosh", Nativ::MatheCosh),
                ("tanh", Nativ::MatheTanh),
                ("max", Nativ::MatheMax),
                ("min", Nativ::MatheMin),
                ("random", Nativ::MatheZufall),
                ("hypot", Nativ::MatheHypot),
                ("clz32", Nativ::MatheClz),
                ("imul", Nativ::MatheImul),
                ("fround", Nativ::MatheFround),
                // W55 (QUIC, 05.10.2026): die drei hyperbolischen Funktionen.
                // **14 rote Faelle, eine Ursache** — sie waren nie registriert.
                // Der Deskriptor ist der der uebrigen Math-Funktionen
                // (`{writable:true, enumerable:false, configurable:true}`,
                // `Math/acosh/prop-desc.js`), also reicht dieselbe `reg`-Zeile.
                ("acosh", Nativ::MatheAcosh),
                ("asinh", Nativ::MatheAsinh),
                ("atanh", Nativ::MatheAtanh),
            ],
        );
        // JSON.
        reg(
            self,
            json,
            &[
                ("parse", Nativ::JsonLesen),
                ("stringify", Nativ::JsonSchreiben),
            ],
        );
        // Fehler-Prototyp: toString.
        reg(
            self,
            self.proto_fehler,
            &[("toString", Nativ::FehlerProtoText)],
        );
        // Map.
        reg(
            self,
            self.proto_karte,
            &[
                ("get", Nativ::KarteHolen),
                ("set", Nativ::KarteSetzen),
                // JS2 (03.10.2026): ES2026 `upsert`. Die Logik liegt in
                // `eingebaut_menge.rs` (meine Datei); hier nur die
                // Registrierung. Gemeldet in `team/WUNSCH_JS.txt`.
                ("getOrInsert", Nativ::KarteGetOrInsert),
                ("getOrInsertComputed", Nativ::KarteGetOrInsertBerechnet),
                ("has", Nativ::KarteHaben),
                ("delete", Nativ::KarteLoeschen),
                ("clear", Nativ::KarteLeeren),
                ("keys", Nativ::KarteSchluessel),
                ("values", Nativ::KarteWerte),
                ("entries", Nativ::KarteEintraege),
                ("forEach", Nativ::KarteFuerJedes),
            ],
        );
        self.nativ_getter(self.proto_karte, "size", Nativ::KarteGroesse);
        // ECMA-262 23.2.7.1 Schritt 2: `@@iterator` **ist** `entries`.
        self.symbol_iterator_als_alias(self.proto_karte, "entries");
        // Set.
        reg(
            self,
            self.proto_menge,
            &[
                ("add", Nativ::MengeHinzu),
                ("has", Nativ::MengeHaben),
                ("delete", Nativ::MengeLoeschen),
                ("clear", Nativ::MengeLeeren),
                // JS2 (03.10.2026, 04:20): `keys` ist **dieselbe
                // Funktion** wie `values` — ECMA-262 24.2.4.4 sagt woertlich
                // „The initial value of the `keys` property is the same
                // function object as the initial value of the `values`
                // property". Wir hatten **zwei** `Nativ`-Objekte mit
                // demselben `Nativ`, also waren sie verschieden.
                //
                // GEMESSEN 04:20: `Set.prototype.keys === Set.prototype.values`
                // war `false` (der rote Fall `Set/prototype/keys/keys.js`
                // sagt genau das). Der Unterschied ist **beobachtbar** —
                // deshalb ist es kein „beide tun dasselbe".
                //
                // **WARUM DIE REIHENFOLGE ZAEHLT:** `reg` laeuft von links
                // nach rechts und legt fuer jeden Eintrag ein **neues**
                // Objekt an. Also muss `values` **vor** `keys` stehen, damit
                // `keys` sich dessen Objekt holen kann.
                ("values", Nativ::MengeWerte),
                ("entries", Nativ::MengeEintraege),
                ("forEach", Nativ::MengeFuerJedes),
            ],
        );
        // `keys` = `values`, dasselbe Objekt (siehe ECMA-262 24.2.4.4 oben).
        // `eigene_lese` statt `eigene_schreiben`, weil der Pfad ein
        // `Gegenstand` ist und `hole` die Prototypkette mitgeht — hier will
        // ich die **eigene** Eigenschaft von `Set.prototype` treffen.
        if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(self.proto_menge)
            && let Some(werte) = eigenschaften.get("values").map(|e| e.wert)
        {
            eigenschaften.insert(
                "keys".to_string(),
                crate::heap::Eigenschaft {
                    wert: werte,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
        self.nativ_getter(self.proto_menge, "size", Nativ::MengeGroesse);
        // ECMA-262 23.2.2.1 Schritt 2: `@@iterator` **ist** `values`.
        self.symbol_iterator_als_alias(self.proto_menge, "values");
        // Date statisch + Prototyp.
        reg(
            self,
            date_ctor,
            &[
                ("now", Nativ::DatumJetzt),
                ("parse", Nativ::DatumLesen),
                ("UTC", Nativ::DatumUTC),
            ],
        );
        reg(
            self,
            self.proto_datum,
            &[
                ("getTime", Nativ::DatumZeit),
                ("valueOf", Nativ::DatumWertVon),
                ("toISOString", Nativ::DatumIso),
                ("toString", Nativ::DatumText),
                // J24: `toDateString`/`toTimeString` liefern nur ihren Teil
                // (vorher der volle `toString`-Text — der Test
                // `datum_text_ist_lokal` fand das).
                ("toDateString", Nativ::DatumTextDatum),
                ("toTimeString", Nativ::DatumTextZeit),
                ("getFullYear", Nativ::DatumJahrLokal),
                ("getMonth", Nativ::DatumMonatLokal),
                ("getDate", Nativ::DatumTagLokal),
                ("getDay", Nativ::DatumWocheLokal),
                ("getHours", Nativ::DatumStundenLokal),
                ("getMinutes", Nativ::DatumMinutenLokal),
                ("getSeconds", Nativ::DatumSekundenLokal),
                ("getMilliseconds", Nativ::DatumMillisLokal),
                ("getTimezoneOffset", Nativ::DatumZone),
                ("setTime", Nativ::DatumZeitSetzen),
                // J24: `getUTC*` liest **UTC**, `get*` liest die **lokale**
                // Zone des Hosts (`zone.rs`, GEMESSEN gegen node 22:
                // `new Date(0).getTimezoneOffset()` -> -60 statt 0). Vor J24
                // zeigten beide auf dieselben Natives — das war die
                // dokumentierte Naeherung.
                ("getUTCFullYear", Nativ::DatumJahr),
                ("getUTCMonth", Nativ::DatumMonat),
                ("getUTCDate", Nativ::DatumTag),
                ("getUTCDay", Nativ::DatumWoche),
                ("getUTCHours", Nativ::DatumStunden),
                ("getUTCMinutes", Nativ::DatumMinuten),
                ("getUTCSeconds", Nativ::DatumSekunden),
                ("getUTCMilliseconds", Nativ::DatumMillis),
                ("setUTCFullYear", Nativ::DatumSetzenJahr),
                ("setFullYear", Nativ::DatumSetzenJahrLokal),
                // `setYear` bleibt der UTC-Pfad (alte 2-Arg-Form).
                ("setYear", Nativ::DatumSetzenJahr),
                ("setUTCMonth", Nativ::DatumSetzenMonat),
                ("setMonth", Nativ::DatumSetzenMonatLokal),
                ("setUTCDate", Nativ::DatumSetzenTag),
                ("setDate", Nativ::DatumSetzenTagLokal),
                ("setUTCHours", Nativ::DatumSetzenStunden),
                ("setHours", Nativ::DatumSetzenStundenLokal),
                ("setUTCMinutes", Nativ::DatumSetzenMinuten),
                ("setMinutes", Nativ::DatumSetzenMinutenLokal),
                ("setUTCSeconds", Nativ::DatumSetzenSekunden),
                ("setSeconds", Nativ::DatumSetzenSekundenLokal),
                ("setUTCMilliseconds", Nativ::DatumSetzenMillis),
                ("setMilliseconds", Nativ::DatumSetzenMillisLokal),
                ("toUTCString", Nativ::DatumUTCString),
                ("toJSON", Nativ::DatumJson),
                // J-CACHE (04.10.2026): toLocale*-Grundformate (Weisung,
                // Logik in eingebaut_intl.rs). Fehlen kostet alle
                // toLocaleDateString/toLocaleTimeString-Dateien.
                ("toLocaleDateString", Nativ::DatumLokalDatum),
                ("toLocaleTimeString", Nativ::DatumLokalZeit),
                ("toLocaleString", Nativ::DatumLokal),
            ],
        );
        // Promise statisch + Prototyp.
        //
        // J-CACHE (03.10.2026): `("any", VersprechenIrgendeine)` ist NEU.
        // GEMESSEN gegen `node 22`: `typeof Promise.any` war `undefined`,
        // die Suite hat aber ein eigenes Verzeichnis `test/built-ins/
        // Promise/any/` — der ganze Bereich fiel allein daran.
        reg(
            self,
            prom_ctor,
            &[
                ("resolve", Nativ::VersprechenErfuellt),
                ("reject", Nativ::VersprechenAbgelehnt),
                ("all", Nativ::VersprechenAlle),
                ("race", Nativ::VersprechenRennen),
                ("allSettled", Nativ::VersprechenAlleErledigt),
                ("any", Nativ::VersprechenIrgendeine),
                // J-CACHE (04.10.2026, Weisung Web-API).
                ("withResolvers", Nativ::VersprechenMitAufloesern),
            ],
        );
        // J-CACHE: `length` ist 1 fuer alle sechs statischen (ES2023
        // 27.2.4.x). `reg` traegt keine Laengenangabe, darum hier nach.
        for name in ["resolve", "reject", "all", "race", "allSettled", "any"] {
            if let Some(GVal::Ref(f)) = self.eigene_lese(prom_ctor, name) {
                let o = self.heap.holen_mut(f);
                if let HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } = o
                {
                    eigenschaften.insert(
                        "length".to_string(),
                        Eigenschaft {
                            wert: GVal::Zahl(1.0),
                            getter: None,
                            setter: None,
                            aufzählbar: false,
                            beschreibbar: false,
                            // J-CACHE 03.10.2026 (19:4x, wiederhergestellt):
                            // **`configurable: true`** — ES2023, Function objects:
                            // `{ [[Writable]]: false, [[Enumerable]]: false,
                            // [[Configurable]]: true }`. Mit `false` meldet
                            // `test/built-ins/Promise/all/length.js`
                            // „length descriptor should be configurable".
                            konfigurierbar: true,
                        },
                    );
                }
            }
        }
        reg(
            self,
            self.proto_versprechen,
            &[
                ("then", Nativ::VersprechenDann),
                ("catch", Nativ::VersprechenFang),
                ("finally", Nativ::VersprechenEndlich),
            ],
        );
        // A4: Response + Headers (Methoden auf dem Prototyp, die Daten als
        // nicht aufzaehlbare Felder `__…` im Objekt — siehe `netz.rs`).
        reg(
            self,
            self.proto_antwort,
            &[
                ("text", Nativ::AntwortText),
                ("json", Nativ::AntwortJson),
                ("arrayBuffer", Nativ::AntwortBytes),
                ("bytes", Nativ::AntwortBytesU8),
                ("blob", Nativ::AntwortKlecks),
            ],
        );
        reg(
            self,
            self.proto_kopf,
            &[
                ("get", Nativ::KopfGet),
                ("has", Nativ::KopfHat),
                ("entries", Nativ::KopfEintraege),
            ],
        );
        // RegExp.
        reg(
            self,
            self.proto_regex,
            &[
                ("test", Nativ::RegexTest),
                ("exec", Nativ::RegexAusfuehren),
                ("toString", Nativ::RegexProtoText),
            ],
        );
        // RegExp.prototype Getter (ECMA-262 22.2.6)
        self.nativ_getter(self.proto_regex, "source", Nativ::RegexSource);
        self.nativ_getter(self.proto_regex, "flags", Nativ::RegexFlags);
        self.nativ_getter(self.proto_regex, "global", Nativ::RegexGlobal);
        self.nativ_getter(self.proto_regex, "ignoreCase", Nativ::RegexIgnoreCase);
        self.nativ_getter(self.proto_regex, "multiline", Nativ::RegexMultiline);
        self.nativ_getter(self.proto_regex, "dotAll", Nativ::RegexDotAll);
        self.nativ_getter(self.proto_regex, "unicode", Nativ::RegexUnicode);
        self.nativ_getter(self.proto_regex, "sticky", Nativ::RegexSticky);
        self.nativ_getter(self.proto_regex, "hasIndices", Nativ::RegexHasIndices);
        self.nativ_getter(self.proto_regex, "unicodeSets", Nativ::RegexUnicodeSets);
        // J23: URL — die Felder sind **Getter** (Spec: `url.href` ist eine
        // Eigenschaft), `toString`/`toJSON` sind Methoden. Die Setter
        // (`hash`, `search`, `href`, `pathname`, `host`) sind eigene
        // Natives mit `this`.
        reg(
            self,
            self.proto_url,
            &[("toString", Nativ::UrlText), ("toJSON", Nativ::UrlText)],
        );
        reg(
            self,
            self.proto_params,
            &[
                ("toString", Nativ::ParamsText),
                ("get", Nativ::ParamsGet),
                ("getAll", Nativ::ParamsGetAlle),
                ("has", Nativ::ParamsHat),
                ("set", Nativ::ParamsSet),
                ("append", Nativ::ParamsAnhaengen),
                ("delete", Nativ::ParamsLoeschen),
                ("keys", Nativ::ParamsSchluessel),
                ("values", Nativ::ParamsWerte),
                ("entries", Nativ::ParamsEintraege),
                ("forEach", Nativ::ParamsFuerAlle),
                ("sort", Nativ::ParamsSortieren),
            ],
        );
        // `size` ist eine **Eigenschaft** (Getter), keine Methode.
        for (n, nat) in [
            ("href", Nativ::UrlText),
            ("protocol", Nativ::UrlSchema),
            ("host", Nativ::UrlHost),
            ("hostname", Nativ::UrlHostname),
            ("port", Nativ::UrlPort),
            ("pathname", Nativ::UrlPfad),
            ("search", Nativ::UrlAbfrage),
            ("hash", Nativ::UrlFragment),
            ("origin", Nativ::UrlOrigin),
            ("username", Nativ::UrlNutzer),
            ("password", Nativ::UrlPasswort),
            ("searchParams", Nativ::UrlParams),
        ] {
            self.nativ_getter(self.proto_url, n, nat);
        }
        self.nativ_getter(self.proto_params, "size", Nativ::ParamsGroesse);
        // URL-Standard §6.2 (WebIDL „iterable<USVString, USVString>"):
        // `@@iterator` ist `entries`. GEMESSEN vorher (WPT
        // url/urlsearchparams-sort): `for (const p of params)` warf „nicht
        // iterierbar".
        self.symbol_iterator_als_alias(self.proto_params, "entries");
        // J24: `BigInt.prototype` mit `toString` (Radix 2..36) und
        // `valueOf`. Die Zahl braucht **ihren** Prototyp, sonst waere
        // `(255n).toString(16)` unmöglich.
        reg(
            self,
            self.proto_bigint,
            &[
                ("toString", Nativ::BigIntText),
                ("valueOf", Nativ::BigIntText),
            ],
        );
        // J24: Symbol.prototype (toString, description) und die
        // WeakMap-/WeakSet-Prototypen.
        reg(self, self.proto_symbol, &[("toString", Nativ::SymText)]);
        self.nativ_getter(self.proto_symbol, "description", Nativ::SymBeschreibung);
        reg(
            self,
            self.proto_weakmap,
            &[
                ("get", Nativ::WeakGet),
                ("has", Nativ::WeakHat),
                ("set", Nativ::WeakSetSet),
                ("delete", Nativ::WeakDelete),
            ],
        );
        reg(
            self,
            self.proto_weakset,
            &[
                ("add", Nativ::WeakSetAdd),
                ("has", Nativ::WeakHat),
                ("delete", Nativ::WeakDelete),
            ],
        );
        // Setter auf `URL.prototype` (das Nativ schreibt direkt).
        //
        // GEMESSEN/GEFUNDEN: der erste Versuch hat `eigenschaften.insert`
        // mit `getter: None` benutzt und damit den **Getter desselben
        // Namens ueberschrieben** — `url.href`, `url.host`, `url.pathname`,
        // `url.search` und `url.hash` gaben danach `undefined`, waehrend
        // `url.hostname` (ohne Setter) stimmte. Jetzt wird der vorhandene
        // Deskriptor **behalten** und nur der Setter ergaenzt.
        for (n, nat) in [
            ("hash", Nativ::UrlSetzeHash),
            ("search", Nativ::UrlSetzeSearch),
            ("href", Nativ::UrlSetzeHref),
            ("pathname", Nativ::UrlSetzePfad),
            ("host", Nativ::UrlSetzeHost),
        ] {
            let f = self.nativ_anlegen(n, nat, self.proto_funktion);
            let o = self.heap.holen_mut(self.proto_url);
            if let HObj::Gegenstand { eigenschaften, .. } = o {
                match eigenschaften.get_mut(n) {
                    Some(d) => d.setter = Some(f),
                    None => {
                        eigenschaften.insert(
                            n.to_string(),
                            crate::heap::Eigenschaft {
                                wert: GVal::Undefiniert,
                                getter: None,
                                setter: Some(f),
                                aufzählbar: false,
                                beschreibbar: true,
                                konfigurierbar: true,
                            },
                        );
                    }
                }
            }
        }
        // `TextEncoder.prototype.encode` / `TextDecoder.prototype.decode`:
        // Release 10.10.2026 an den eigenen Prototypen (builtin.rs), nicht
        // mehr an `Function.prototype`.
        // console.
        reg(
            self,
            console,
            &[
                ("log", Nativ::KonsoleLog),
                ("error", Nativ::KonsoleFehler),
                ("warn", Nativ::KonsoleWarn),
                ("info", Nativ::KonsoleInfo),
            ],
        );
        // performance (J6).
        reg(
            self,
            performance,
            &[
                ("now", Nativ::PerformanceNow),
                ("mark", Nativ::PerformanceMark),
                ("measure", Nativ::PerformanceMeasure),
                // 05.10.2026: die Navigation-Timing-Abfragen (youtube u. a.).
                ("getEntries", Nativ::PerformanceEintraege),
                ("getEntriesByType", Nativ::PerformanceEintraege),
                ("getEntriesByName", Nativ::PerformanceEintraege),
            ],
        );
        // Globale Ctor-Namen, die mk_ctor nicht gebunden hat.
        let _ = (map_ctor, set_ctor, date_ctor, prom_ctor, regex_ctor);
    }

    /// ECMA-262 10.2.11 `SetFunctionName` — ein **Accessor** bekommt den
    /// Namen `"get "` (bzw. `"set "`) **mit dem Schluessel**, nicht den
    /// Schluessel allein.
    ///
    /// ## GEMESSEN (JS2, 03.10.2026, 09:55), `node 22`:
    ///
    /// ```js
    /// Object.getOwnPropertyDescriptor(Map.prototype, 'size').get.name
    ///   -> node 22: "get size"        bei uns: "size"
    /// ```
    ///
    /// Das sind `Map/prototype/size/name.js` und `Set/prototype/size/name.js` —
    /// **zwei** rote Faelle, und beide sind genau diese eine Zeile.
    ///
    /// **UND DIE ZAHLEN SIND ZWO STELLEN, DIE MAN VERWECHSELT:**
    ///
    /// ```js
    /// Object.getOwnPropertyDescriptor(Map.prototype, 'size').name
    ///   -> undefined          <- bei einer ACCESSOR-Eigenschaft gibt es
    ///                             kein `name`; node sagt auch undefined
    /// Object.getOwnPropertyDescriptor(Map.prototype, 'size').get.name
    ///   -> "get size"         <- DER Name der GETTER-FUNKTION
    /// ```
    ///
    /// Vorher habe ich beim ersten gemessen (der war richtig) und daraus
    /// geschlossen, es stimme. **Das sind zwei Aussagen an zwei Orten** —
    /// die dritte Vermischung dieser Art in dieser Sitzung (`every`,
    /// `__pos`/`for…in`, `Array.isArray(arguments)`). Der Test unten prueft
    /// **beide**, damit das nicht wieder passiert.
    ///
    /// **WARUM ES NICHT `format!` IN `nativ_anlegen` GEHOERT:** dort wird der
    /// Name fuer **Methoden** benutzt, und fuer eine Methode ist der Name
    /// **genau** der Schluessel (`Map.prototype.get.name === "get"` — das
    /// stimmt bei uns). Wer die Regel in `nativ_anlegen` reinzieht, macht
    /// aus **Methoden** `"get get"`. **Die Regel gehoert an den Accessor,
    /// nicht an die Funktion.**
    ///
    /// M3: **eine** `String`-Verknuepfung pro Getter-Registrierung, und die
    /// Registrierung laeuft **einmal** beim Aufbau — nicht pro Aufruf. Keine
    /// Schleife, keine fremde Eingabe.
    pub(crate) fn nativ_getter(&mut self, ziel: HeapRef, name: &str, nativ: Nativ) {
        let f = self.nativ_anlegen(&format!("get {name}"), nativ, self.proto_funktion);
        let o = self.heap.holen_mut(ziel);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                Some(eigenschaften)
            }
            _ => None,
        };
        if let Some(eigenschaften) = map {
            eigenschaften.insert(
                name.to_string(),
                Eigenschaft {
                    wert: GVal::Undefiniert,
                    getter: Some(f),
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    /// J-AR: ein **Setter** an einer vorhandenen Getter-Eigenschaft
    /// (`document.cookie = "a=b"`). Es gab nur `nativ_getter`; die
    /// Zuweisung lief ins Leere (der Getter lieferte `undefined`).
    /// ECMA-262 10.2.11 `SetFunctionName`, zweite Haelfte: ein **Setter**
    /// heisst `"set "` + Schluessel. Siehe `nativ_getter` fuer den Grund,
    /// warum die Regel am Accessor und nicht an `nativ_anlegen` steht.
    ///
    /// GEMESSEN: `document.cookie` ist ein Getter/Setter-Paar, und der
    /// **Setter** bekam vorher den blossen Schluessel. Die Suite misst das
    /// fuer keinen eingebauten Setter (0 Faelle in der Auswahl, 09:55) — die
    /// Aenderung ist trotzdem richtig, und der Zeuge ist mein Probe, nicht
    /// die Suite.
    pub(crate) fn nativ_setter(&mut self, ziel: HeapRef, name: &str, nativ: Nativ) {
        let f = self.nativ_anlegen(&format!("set {name}"), nativ, self.proto_funktion);
        let o = self.heap.holen_mut(ziel);
        let map = match o {
            HObj::Gegenstand { eigenschaften, .. } => Some(eigenschaften),
            HObj::Funktion { eigenschaften, .. } | HObj::Nativ { eigenschaften, .. } => {
                Some(eigenschaften)
            }
            _ => None,
        };
        if let Some(eigenschaften) = map {
            let e = eigenschaften
                .entry(name.to_string())
                .or_insert_with(|| Eigenschaft {
                    wert: GVal::Undefiniert,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                });
            e.setter = Some(f);
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn rufe_nativ_teil2(
        &mut self,
        nativ: Nativ,
        fn_ref: HeapRef,
        this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        use Nativ::*;
        match nativ {
            // ## ECMA-262 22.1.2.2 / 22.1.2.3 / 22.1.2.4 — **kein**
            // [[Construct]].
            //
            // `String.fromCharCode`, `String.fromCodePoint` und `String.raw`
            // sind gewöhnliche eingebaute Funktionen; die Norm gibt ihnen
            // **keinen** [[Construct]]- internen Platz. `new` darauf ist
            // deshalb ein `TypeError` — und der Motor lieferte statt dessen
            // ein **Ergebnis**, was das Schlimmste ist: kein Fehler, der
            // sichtbar wäre, sondern ein falsches.
            //
            // **GEMESSEN (03.10.2026, 20:5x), gegen `node 22`:**
            // ```text
            // new (String.fromCharCode)(65,66,66,65)     node: TypeError   bei uns: "ABBA"
            // new (String.fromCodePoint)(65)              node: TypeError   bei uns: "A"
            // ```
            // Der rote Fall ist `String/fromCharCode/S15.5.3.2_A4.js` und
            // sagt genau das: *„String.fromCharCode has not [[construct]]
            // method"*. Er holt sich die Funktion vorher weg
            // (`delete String.fromCharCode`) und ruft sie mit `new` — der
            // Wurf muss also am **Aufruf** kommen, nicht an der
            // Eigenschaftsloeschung.
            //
            // **WARUM EINE `match`-REGEL und nicht drei Stellen in den
            // Funktionen:** die drei sind eine Regel (ECMA-262 22.1.2,
            // „nicht-constructible static methods"), und `n_text_aus_zeichen`
            // wird ohnehin von **zwei** Nativs geteilt — eine Regel an
            // **einer** Stelle kann nicht halb greifen.
            //
            // M3: **keine** Allokation. Ein Vergleich auf ein `bool`, das
            // der Aufrufmechanismus ohnehin schon berechnet.
            TextAusZeichen | TextAusPunkten | TextRoh if als_konstruktor => {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "String: statische Methode ohne new"),
                ));
            }
            // --- String ---
            TextCtor => self.n_text_ctor(this, args, als_konstruktor),
            TextAusZeichen => self.n_text_aus_zeichen(args, false),
            TextAusPunkten => self.n_text_aus_zeichen(args, true),
            TextRoh => self.n_text_roh(args),
            TextZeichenAn => self.n_text_zeichen(this, args, 0),
            TextZeichenKode => self.n_text_zeichen(this, args, 1),
            TextPunktKode => self.n_text_zeichen(this, args, 2),
            TextIndexVon => self.n_text_index(this, args, false),
            TextLetzterIndex => self.n_text_index(this, args, true),
            TextEnthaelt => self.n_text_enthaelt(this, args),
            TextBeginnt => self.n_text_beginnt(this, args, true),
            TextEndet => self.n_text_beginnt(this, args, false),
            TextScheibe => self.n_text_scheibe(this, args, false),
            TextTeil => self.n_text_scheibe(this, args, true),
            TextTeilAlt => self.n_text_teil_alt(this, args),
            TextTeilen => self.n_text_teilen(this, args),
            TextStutzen => self.n_text_stutzen(this, 0),
            TextNormalisieren => self.n_text_normalisieren(this, args),
            TextStutzenStart => self.n_text_stutzen(this, 1),
            TextStutzenEnde => self.n_text_stutzen(this, 2),
            TextGross => self.n_text_fall(this, true),
            TextKlein => self.n_text_fall(this, false),
            TextWiederholen => self.n_text_wiederholen(this, args),
            TextFuellenStart => self.n_text_füllen(this, args, true),
            TextFuellenEnde => self.n_text_füllen(this, args, false),
            TextErsetzen => self.n_text_ersetzen(this, args, false),
            TextErsetzenAlle => self.n_text_ersetzen(this, args, true),
            TextTreffer => self.n_text_treffer(this, args),
            TextSuchen => self.n_text_suchen(this, args),
            TextVerketten => self.n_text_verketten(this, args),
            TextAn => self.n_text_an(this, args),
            TextWertVon => self.n_text_wert_von(this),
            TextProtoText => self.n_text_wert_von(this),
            TextVergleich => self.n_text_vergleich(this, args),
            // --- Number/Boolean ---
            ZahlCtor => self.n_zahl_ctor(this, args, als_konstruktor),
            ZahlIstNaN => Ok(GVal::Bool(
                matches!(self.arg(args, 0), GVal::Zahl(n) if n.is_nan()),
            )),
            ZahlIstEndlich => Ok(GVal::Bool(
                matches!(self.arg(args, 0), GVal::Zahl(n) if n.is_finite()),
            )),
            ZahlIstGanz => Ok(GVal::Bool(
                matches!(self.arg(args, 0), GVal::Zahl(n) if n.is_finite() && n.fract() == 0.0),
            )),
            ZahlIstSicher => Ok(GVal::Bool(
                matches!(self.arg(args, 0), GVal::Zahl(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0),
            )),
            ZahlProtoText => self.n_zahl_text(this, args),
            ZahlLokal => self.n_zahl_lokal(this),
            ZahlFest => self.n_zahl_fest(this, args),
            ZahlExpo => self.n_zahl_expo(this, args),
            ZahlGenau => self.n_zahl_genau(this, args),
            ZahlWertVon => self.n_zahl_wert_von(this),
            BoolCtor => self.n_bool_ctor(this, args, als_konstruktor),
            BoolProtoText => self.n_bool_text(this),
            BoolWertVon => self.n_bool_wert(this),
            // --- Math ---
            MatheAbs => self.n_mathe1(args, |x| x.abs()),
            MatheBoden => self.n_mathe1(args, |x| x.floor()),
            MatheDecke => self.n_mathe1(args, |x| x.ceil()),
            MatheRund => self.n_mathe1(args, |x| (x + 0.5).floor()),
            MatheKapp => self.n_mathe1(args, |x| x.trunc()),
            MatheVor => self.n_mathe1(args, |x| {
                if x.is_nan() {
                    f64::NAN
                } else if x == 0.0 {
                    x
                } else if x > 0.0 {
                    1.0
                } else {
                    -1.0
                }
            }),
            MatheWurzel => self.n_mathe1(args, |x| x.sqrt()),
            MatheKubik => self.n_mathe1(args, |x| x.cbrt()),
            MathePotenz => self.n_mathe2(args, |x, y| x.powf(y)),
            MatheExp => self.n_mathe1(args, |x| x.exp()),
            MatheExpm1 => self.n_mathe1(args, |x| x.exp_m1()),
            MatheLog => self.n_mathe1(args, |x| x.ln()),
            MatheLog2 => self.n_mathe1(args, |x| x.log2()),
            MatheLog10 => self.n_mathe1(args, |x| x.log10()),
            MatheLog1p => self.n_mathe1(args, |x| x.ln_1p()),
            MatheSin => self.n_mathe1(args, |x| x.sin()),
            MatheCos => self.n_mathe1(args, |x| x.cos()),
            MatheTan => self.n_mathe1(args, |x| x.tan()),
            MatheAsin => self.n_mathe1(args, |x| x.asin()),
            MatheAcos => self.n_mathe1(args, |x| x.acos()),
            MatheAtan => self.n_mathe1(args, |x| x.atan()),
            MatheAtan2 => self.n_mathe2(args, |x, y| x.atan2(y)),
            MatheSinh => self.n_mathe1(args, |x| x.sinh()),
            MatheCosh => self.n_mathe1(args, |x| x.cosh()),
            MatheTanh => self.n_mathe1(args, |x| x.tanh()),
            MatheRundenExakt => self.n_mathe1(args, |x| x),
            MatheMax => self.n_mathe_minmax(args, true),
            MatheMin => self.n_mathe_minmax(args, false),
            MatheZufall => self.n_mathe_zufall(),
            MatheHypot => self.n_mathe_hypot(args),
            MatheClz => self.n_mathe_clz(args),
            MatheImul => self.n_mathe_imul(args),
            MatheFround => self.n_mathe1(args, |x| x as f32 as f64),
            // W55 (QUIC): `f64::acosh/asinh/atanh` aus der Stdlib. **Die
            // Formeln NICHT selbst geschrieben** — `acosh(x) =
            // ln(x + sqrt(x^2 - 1))` verliert dicht an `x = 1` die Genauigkeit,
            // und genau das ist `arg-is-one.js` (verlangt **+0**).
            //
            // **UND `atanh` ist der Beleg dafuer, dass man keine eigene Formel
            // schreibt:** sie liefert `NaN` fuer `|x| > 1` **und** fuer `±∞`.
            // `ln((1+x)/(1-x))` waere bei `1.9` per Zufall `NaN` (`ln` einer
            // negativen Zahl), aber bei einem Wert nahe `1` waere sie
            // **falsch** — der Quotient verliert dort alle Stellen.
            // *Zwei Zufallsfaelle sind keine Regel.*
            MatheAcosh => self.n_mathe1(args, |x| x.acosh()),
            MatheAsinh => self.n_mathe1(args, |x| x.asinh()),
            MatheAtanh => self.n_mathe1(args, |x| x.atanh()),
            // --- JSON/Fehler ---
            JsonLesen => self.n_json_lesen(args),
            JsonSchreiben => self.n_json_schreiben(args),
            FehlerCtor(art) => self.n_fehler_ctor(&art, args, als_konstruktor),
            FehlerProtoText => self.n_fehler_text(this),
            // --- J10: ES-Module (die vier `__jsvm_…`-Helfer; in der
            // globalen **Umgebung** gebunden, nicht im globalen Objekt —
            // `Object.keys(globalThis)` zeigt sie deshalb nicht).
            ModulImport => self.n_modul_import(args),
            ModulImportAlle => self.n_modul_import_alle(args),
            ModulExport => self.n_modul_export(args),
            ModulExportAlle => self.n_modul_export_alle(args),
            ModulMeldung => self.n_modul_meldung(args),
            // J12: dynamischer Import (`import('…')`).
            ModulImportDyn => self.n_modul_import_dyn(args),
            // --- Map/Set ---
            KarteCtor => self.n_karte_ctor(args, als_konstruktor),
            KarteHolen => self.n_karte_holen(this, args),
            KarteSetzen => self.n_karte_setzen(this, args),
            KarteGetOrInsert => self.n_karte_get_or_insert(this, args),
            KarteGetOrInsertBerechnet => self.n_karte_get_or_insert_berechnet(this, args),
            KarteHaben => self.n_karte_haben(this, args),
            KarteLoeschen => self.n_karte_loeschen(this, args),
            KarteLeeren => self.n_karte_leeren(this),
            KarteGroesse => self.n_karte_groesse(this),
            KarteSchluessel => self.n_karte_iter(this, 0),
            KarteWerte => self.n_karte_iter(this, 1),
            KarteEintraege => self.n_karte_iter(this, 2),
            KarteFuerJedes => self.n_karte_fuer_jedes(this, args),
            MengeCtor => self.n_menge_ctor(args, als_konstruktor),
            MengeHinzu => self.n_menge_hinzu(this, args),
            MengeHaben => self.n_menge_haben(this, args),
            MengeLoeschen => self.n_menge_loeschen(this, args),
            MengeLeeren => self.n_menge_leeren(this),
            MengeGroesse => self.n_menge_groesse(this),
            MengeWerte => self.n_menge_iter(this),
            MengeEintraege => self.n_menge_eintraege(this),
            MengeFuerJedes => self.n_menge_fuer_jedes(this, args),
            // --- Datum ---
            DatumCtor => self.n_datum_ctor(this, args, als_konstruktor),
            DatumJetzt => Ok(GVal::Zahl(crate::builtin3::jetzt_millis())),
            DatumLesen => self.n_datum_lesen(args),
            DatumUTC => self.n_datum_utc(args),
            DatumZeit => self.n_datum_zeit(this),
            DatumWertVon => self.n_datum_zeit(this),
            DatumIso => self.n_datum_iso(this),
            DatumJson => self.n_datum_json(this),
            DatumLokalDatum => self.n_datum_lokal(this, args, 0),
            DatumLokalZeit => self.n_datum_lokal(this, args, 1),
            DatumLokal => self.n_datum_lokal(this, args, 2),
            // J-CACHE (04.10.2026, Freigabe Leitung): Intl-Verdrahtung.
            // Konstruktoren laufen mit UND ohne new (Spec) — fn_ref ist
            // der Ctor (Proto kommt von ctor.prototype).
            IntlDatumZeit => self.n_intl_ctor(fn_ref, args, 0),
            IntlZahl => self.n_intl_ctor(fn_ref, args, 1),
            IntlPlural => self.n_intl_ctor(fn_ref, args, 2),
            IntlVergleicher => self.n_intl_ctor(fn_ref, args, 3),
            IntlRelativ => self.n_intl_ctor(fn_ref, args, 4),
            IntlListe => self.n_intl_ctor(fn_ref, args, 5),
            IntlFormat => self.n_intl_format(this, args),
            IntlTeile => self.n_intl_teile(this, args),
            IntlOptionen => self.n_intl_optionen(this),
            IntlWaehlen => self.n_intl_wahl(this, args, false),
            IntlVergleichen => self.n_intl_wahl(this, args, true),
            IntlGestuetzt => self.n_intl_gestuetzt(args),
            IntlKanonisch => self.n_intl_kanonisch(args),
            IntlAnzeige => self.n_intl_ctor(fn_ref, args, 6),
            IntlAnzeigeVon => self.n_intl_anzeige_von(this, args),
            DatumText => self.n_datum_text(this, 0),
            DatumTextDatum => self.n_datum_text(this, 1),
            DatumTextZeit => self.n_datum_text(this, 2),
            DatumJahr => self.n_datum_teil(this, 0),
            DatumMonat => self.n_datum_teil(this, 1),
            DatumTag => self.n_datum_teil(this, 2),
            DatumWoche => self.n_datum_teil(this, 3),
            DatumStunden => self.n_datum_teil(this, 4),
            DatumMinuten => self.n_datum_teil(this, 5),
            DatumSekunden => self.n_datum_teil(this, 6),
            DatumMillis => self.n_datum_teil(this, 7),
            DatumZone => {
                // J24: die **lokale** Zone (vorher 0 — siehe `zone.rs`).
                let m = self.datum_als_ref(this, "getTimezoneOffset")?;
                if !m.is_finite() {
                    return Ok(GVal::Zahl(f64::NAN));
                }
                Ok(GVal::Zahl(crate::zone::offset_minuten(m) as f64))
            }
            DatumJahrLokal => self.n_datum_teil_lokal(this, 0),
            DatumMonatLokal => self.n_datum_teil_lokal(this, 1),
            DatumTagLokal => self.n_datum_teil_lokal(this, 2),
            DatumWocheLokal => self.n_datum_teil_lokal(this, 3),
            DatumStundenLokal => self.n_datum_teil_lokal(this, 4),
            DatumMinutenLokal => self.n_datum_teil_lokal(this, 5),
            DatumSekundenLokal => self.n_datum_teil_lokal(this, 6),
            DatumMillisLokal => self.n_datum_teil_lokal(this, 7),
            DatumZeitSetzen => self.n_datum_zeit_setzen(this, args),
            DatumSetzenJahr => self.n_datum_setzen(this, args, 0, false),
            // J24: die `set*`-Natives rechnen in der **lokalen** Zone (vorher
            // wie `setUTC*` — der Test `datum_konstruktor_...` fand das).
            DatumSetzenJahrLokal => self.n_datum_setzen(this, args, 0, true),
            DatumSetzenMonatLokal => self.n_datum_setzen(this, args, 1, true),
            DatumSetzenTagLokal => self.n_datum_setzen(this, args, 2, true),
            DatumSetzenStundenLokal => self.n_datum_setzen(this, args, 4, true),
            DatumSetzenMinutenLokal => self.n_datum_setzen(this, args, 5, true),
            DatumSetzenSekundenLokal => self.n_datum_setzen(this, args, 6, true),
            DatumSetzenMillisLokal => self.n_datum_setzen(this, args, 7, true),
            DatumSetzenMonat => self.n_datum_setzen(this, args, 1, false),
            DatumSetzenTag => self.n_datum_setzen(this, args, 2, false),
            DatumSetzenStunden => self.n_datum_setzen(this, args, 4, false),
            DatumSetzenMinuten => self.n_datum_setzen(this, args, 5, false),
            DatumSetzenSekunden => self.n_datum_setzen(this, args, 6, false),
            DatumSetzenMillis => self.n_datum_setzen(this, args, 7, false),
            DatumUTCString => self.n_datum_utc_string(this),
            // --- Promise ---
            VersprechenCtor => self.n_versprechen_ctor(args),
            VersprechenErfuellt => self.n_versprechen_statisch(this, args, true),
            VersprechenAbgelehnt => self.n_versprechen_statisch(this, args, false),
            AufloeserErfuellt => self.n_versprechen_resolver(fn_ref, args, true),
            AufloeserAbgelehnt => self.n_versprechen_resolver(fn_ref, args, false),
            EndlichWeiter(erfuellt) => self.n_endlich_weiter(fn_ref, erfuellt),
            VersprechenAlle => self.n_versprechen_alle(this, args, 0),
            VersprechenRennen => self.n_versprechen_alle(this, args, 1),
            VersprechenAlleErledigt => self.n_versprechen_alle(this, args, 2),
            VersprechenIrgendeine => self.n_versprechen_alle(this, args, 3),
            VersprechenMitAufloesern => self.n_versprechen_mit_aufloesern(),
            VersprechenExecutor => self.versprechen_executor_impl(fn_ref, args),
            // Release 15.10.2026: the derived promise follows the species
            // constructor (`es_modern.rs`, `versprechen_spezies`).
            VersprechenDann => {
                let r = self.n_versprechen_dann(this, args)?;
                self.versprechen_spezies(this, r)
            }
            VersprechenFang => {
                let r = self.n_versprechen_dann(this, &[GVal::Undefiniert, self.arg(args, 0)])?;
                self.versprechen_spezies(this, r)
            }
            VersprechenEndlich => {
                let r = self.n_versprechen_endlich(this, args)?;
                self.versprechen_spezies(this, r)
            }
            AllHelfer => self.all_helfer_impl(fn_ref, args),
            // --- RegExp/Konsole ---
            RegexCtor => self.n_regex_ctor(args, als_konstruktor),
            RegexTest => self.n_regex_test(this, args),
            RegexAusfuehren => self.n_regex_exec(this, args),
            RegexProtoText => self.n_regex_text(this),
            RegexSource => self.n_regex_source(this),
            RegexFlags => self.n_regex_flags(this),
            RegexGlobal => self.n_regex_global(this),
            RegexIgnoreCase => self.n_regex_ignore_case(this),
            RegexMultiline => self.n_regex_multiline(this),
            RegexDotAll => self.n_regex_dot_all(this),
            RegexUnicode => self.n_regex_unicode(this),
            RegexSticky => self.n_regex_sticky(this),
            RegexHasIndices => self.n_regex_has_indices(this),
            RegexUnicodeSets => self.n_regex_unicode_sets(this),
            KonsoleLog | KonsoleFehler | KonsoleWarn | KonsoleInfo => self.n_konsole(args),
            // J6: performance — Zeit seit Laufbeginn (ms), Markenliste
            // gedeckelt (M4: hoechstens 4096 Marken, die aeltesten fallen weg).
            PerformanceNow => {
                let ms = self.time_origin_ms();
                Ok(GVal::Zahl(ms))
            }
            // 05.10.2026: `getEntries`/`getEntriesByType`/`getEntriesByName`.
            //
            // GEMESSEN an www.youtube.com: `performance.timing.responseStart`
            // und die Navigation-Timing-Liste werden im ersten Skript gelesen;
            // ohne Eintraege brach die Messung ab. `navigation` liefert EINEN
            // Eintrag, alles andere eine leere Liste (Ressourcen-Timing
            // fuehren wir nicht — eine leere Liste ist die ehrliche Antwort,
            // kein erfundenes Messergebnis).
            PerformanceEintraege => {
                let art = self.zu_text(self.arg(args, 0)).unwrap_or_default();
                let feld = self
                    .leeres_feld_mit_proto(self.proto_feld)
                    .map_err(Stoerung::Fehler)?;
                if art == "navigation" {
                    let e = self
                        .neues_objekt(Some(self.proto_objekt))
                        .map_err(Stoerung::Fehler)?;
                    let text = |s: &mut Self, w: &str| -> Result<GVal, Stoerung> {
                        s.text_wert(w).map_err(Stoerung::Fehler)
                    };
                    let name = text(self, "")?;
                    let typ = text(self, "navigation")?;
                    let art_t = text(self, "navigate")?;
                    for (n, v) in [
                        ("name", name),
                        ("entryType", typ),
                        ("type", art_t),
                        ("startTime", GVal::Zahl(0.0)),
                        ("duration", GVal::Zahl(4.0)),
                        ("responseStart", GVal::Zahl(1.0)),
                        ("responseEnd", GVal::Zahl(2.0)),
                        ("domComplete", GVal::Zahl(4.0)),
                        ("loadEventEnd", GVal::Zahl(4.0)),
                        ("redirectCount", GVal::Zahl(0.0)),
                    ] {
                        self.setze_eigen(e, n, v);
                    }
                    self.setze(GVal::Ref(feld), "0", GVal::Ref(e))?;
                }
                Ok(GVal::Ref(feld))
            }
            // --- J23: URL / URLSearchParams / TextEncoder / TextDecoder ---
            UrlCtor => self.n_url_ctor(this, args, als_konstruktor),
            UrlKann => self.n_url_kann(args),
            UrlText => self.n_url_text(this, args),
            UrlSchema => self.n_url_schema(this, args),
            UrlHost => self.n_url_host(this, args),
            UrlHostname => self.n_url_hostname(this, args),
            UrlPort => self.n_url_port(this, args),
            UrlPfad => self.n_url_pfad(this, args),
            UrlAbfrage => self.n_url_abfrage(this, args),
            UrlFragment => self.n_url_fragment(this, args),
            UrlOrigin => self.n_url_origin(this, args),
            UrlNutzer => self.n_url_nutzer(this, args),
            UrlPasswort => self.n_url_passwort(this, args),
            UrlParams => self.n_url_params(this, args),
            UrlSetzeHash => self.n_url_setzen(this, args, UrlFeld::Hash),
            UrlSetzeSearch => self.n_url_setzen(this, args, UrlFeld::Search),
            UrlSetzeHref => self.n_url_setzen(this, args, UrlFeld::Href),
            UrlSetzePfad => self.n_url_setzen(this, args, UrlFeld::Pfad),
            UrlSetzeHost => self.n_url_setzen(this, args, UrlFeld::Host),
            ParamsCtor => self.n_params_ctor(this, args, als_konstruktor),
            ParamsGet => self.n_params_get(this, args),
            ParamsGetAlle => self.n_params_get_all(this, args),
            ParamsHat => self.n_params_hat(this, args),
            ParamsSet => self.n_params_set(this, args),
            ParamsAnhaengen => self.n_params_anhaengen(this, args),
            ParamsLoeschen => self.n_params_loeschen(this, args),
            ParamsText => self.n_params_text(this, args),
            ParamsGroesse => self.n_params_groesse(this, args),
            ParamsSchluessel => self.n_params_schluessel(this, args, ParamsLauf::Schluessel),
            ParamsWerte => self.n_params_schluessel(this, args, ParamsLauf::Werte),
            ParamsEintraege => self.n_params_schluessel(this, args, ParamsLauf::Eintraege),
            ParamsFuerAlle => self.n_params_fuer_alle(this, args),
            ParamsSortieren => self.n_params_sortieren(this),
            TextEncoderCtor => self.n_text_encoder_ctor(fn_ref),
            TextEncoderEncode => self.n_text_encoder_encode(args),
            TextEncoderEncodeInto => self.n_text_encoder_encode_into(args),
            TextDecoderCtor => self.n_text_decoder_ctor(fn_ref, args),
            TextDecoderDecode => self.n_text_decoder_decode(this, args),
            // --- J24: Symbol / WeakMap / WeakSet ---
            // J24: BigInt
            BigIntCtor => self.n_bigint_ctor(args),
            BigIntAsIntN => self.n_bigint_n(args, true),
            BigIntAsUintN => self.n_bigint_n(args, false),
            BigIntText => self.n_bigint_text(this, args),
            SymCtor => self.n_symbol_ctor(this, args, als_konstruktor, false),
            // GEMESSEN/GEFUNDEN: `new Symbol.for('x')` muss ein TypeError
            // sein (node). Mit festem `false` fuer `als_konstruktor` sah der
            // Konstruktorpfad das nie — der Aufruf lieferte ein Symbol.
            SymFor => self.n_symbol_ctor(this, args, als_konstruktor, true),
            SymKeyFor => self.n_symbol_key_for(this, args),
            SymIterator => self.n_symbol_bekannt(this, args, Nativ::SymIterator),
            SymAsyncIterator => self.n_symbol_bekannt(this, args, Nativ::SymAsyncIterator),
            SymToPrimitive => self.n_symbol_bekannt(this, args, Nativ::SymToPrimitive),
            SymHasInstance => self.n_symbol_bekannt(this, args, Nativ::SymHasInstance),
            SymSpecies => self.n_symbol_bekannt(this, args, Nativ::SymSpecies),
            SymToStringTag => self.n_symbol_bekannt(this, args, Nativ::SymToStringTag),
            // Release 10.10.2026: der Zweig fehlte — `Symbol.unscopables`
            // warf „Nativ unbekannt" (bbc.com, core-js im Next.js-Buendel).
            SymUnscopables => self.n_symbol_bekannt(this, args, Nativ::SymUnscopables),
            SymText => self.n_symbol_text(this, args, false),
            SymBeschreibung => self.n_symbol_text(this, args, true),
            WeakMapCtor => self.n_weak_ctor(this, args, false),
            WeakSetCtor => self.n_weak_ctor(this, args, true),
            WeakGet => self.n_weak_get(this, args),
            WeakHat => self.n_weak_hat(this, args),
            WeakSetSet => self.n_weak_set(this, args, false),
            WeakSetAdd => self.n_weak_adder(this, args, true),
            WeakDelete => self.n_weak_delete(this, args),
            PerformanceMark => {
                let name = self.zu_text(self.arg(args, 0))?;
                let ms = self.time_origin_ms();
                self.performance_marken.push((name, ms));
                if self.performance_marken.len() > 4096 {
                    self.performance_marken.remove(0);
                }
                Ok(GVal::Undefiniert)
            }
            PerformanceMeasure => {
                // `measure(name, von, bis)`: Dauer der beiden Marken, sonst
                // `undefined` (ohne Marken gibt es nichts zu messen).
                let von = self.arg(args, 1);
                let bis = self.arg(args, 2);
                let hole = |m: &Vec<(String, f64)>, k: &str| -> Option<f64> {
                    m.iter().rev().find(|(n, _)| n == k).map(|(_, v)| *v)
                };
                let von_name = self.zu_text(von)?;
                let bis_name = self.zu_text(bis)?;
                let a = hole(&self.performance_marken, &von_name);
                let b = hole(&self.performance_marken, &bis_name);
                match (a, b) {
                    (Some(a), Some(b)) => Ok(GVal::Zahl(b - a)),
                    _ => Ok(GVal::Undefiniert),
                }
            }
            // ## W53 (JS, 05.10.2026) — ArrayBuffer / SharedArrayBuffer / DataView
            //
            // **Diese 24 Varianten waren seit JS_12 in `Nativ` und hatten
            // keinen Arm.** Das ist kein Tippfehler, sondern der Grund, warum
            // `ArrayBuffer` nicht erreichbar war: der Aufbau in `aufbauen()`
            // band die Konstruktoren nie an den globalen Bereich.
            //
            // GEMESSEN (node 22, v22.23.3), die Formen:
            //   `new ArrayBuffer(8)`                    -> ArrayBuffer(8)
            //   `new SharedArrayBuffer(4)`              -> SharedArrayBuffer(4)
            //   `new DataView(new ArrayBuffer(8), 2)`    -> DataView(8, 2)
            //   `new DataView(buf, 2).byteOffset`        -> 2
            //   `dv.setInt16(0, -2, true); dv.getInt16(0, true)` -> -2
            //   `dv.setFloat64(0, 1.5, true); dv.getFloat64(0, true)` -> 1.5
            // **Endianness:** das dritte Argument ist `littleEndian`; ohne
            // Argument ist es **Big** (`false`).
            PufferCtor => {
                if !als_konstruktor {
                    // **WERFEN, nicht zurueckgeben.** Die erste Fassung gab das
                    // Fehler-Objekt als **Wert** zurueck
                    // (`Ok(self.fehler_wert(...))`), also lief
                    // `ArrayBuffer(8)` **durch** und lieferte ein „Objekt".
                    // GEMESSEN vorher: der Aufruf kam nicht um. node 22
                    // (v22.23.3): `ArrayBuffer(8)` -> **TypeError**.
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "ArrayBuffer braucht new"),
                    ));
                }
                self.n_puffer_ctor(args, false)
            }
            SharedPufferCtor => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "SharedArrayBuffer braucht new"),
                    ));
                }
                self.n_puffer_ctor(args, true)
            }
            PufferByteLaenge => self.n_puffer_bytelaenge(this),
            PufferGrowable => self.n_puffer_growable(this),
            PufferDetached => self.n_puffer_detached(this),
            PufferTransfer => self.n_puffer_transfer(this, args),
            PufferSlice => self.n_puffer_slice(this, args),
            PufferIstAnsicht => {
                let v = self.arg(args, 0);
                let ja = match v {
                    GVal::Ref(r) => {
                        matches!(self.heap.holen(r), HObj::TypedArray { .. })
                            || (self.eigene_lese(r, "#es_dv_puffer").is_some() && self.eigene_lese(r, "#es_dv_umfang").is_some())
                    }
                    _ => false,
                };
                Ok(GVal::Bool(ja))
            }
            DataviewCtor => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("TypeError", "DataView braucht new"),
                    ));
                }
                self.n_dataview_ctor(args)
            }
            DataviewByteLaenge => self.n_dataview_groesse(this),
            DataviewByteVersatz => self.n_dataview_versatz(this),
            DataviewPuffer => self.n_dataview_puffer(this),
            // Gemeinsamer Dispatch fuer Number- und BigInt-Elemente.
            DataviewGetUint8 | DataviewGetInt8 | DataviewGetUint16 | DataviewGetInt16
            | DataviewGetUint32 | DataviewGetInt32 | DataviewGetFloat32
            | DataviewGetFloat64 | DataviewGetBigInt64 | DataviewGetBigUint64 => {
                self.n_dataview_get(this, args, crate::eingebaut_typed::dataview_art_fuer(nativ))
            }
            DataviewSetUint8 | DataviewSetInt8 | DataviewSetUint16 | DataviewSetInt16
            | DataviewSetUint32 | DataviewSetInt32 | DataviewSetFloat32
            | DataviewSetFloat64 | DataviewSetBigInt64 | DataviewSetBigUint64 => {
                self.n_dataview_set(this, args, crate::eingebaut_typed::dataview_art_fuer(nativ))
            }

            // ## W55 (JS, 05.10.2026) — TypedArray-Konstruktoren (11 Arten).
            TypedArrayCtorUint8 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Uint8Array braucht new")));
                }
                self.n_typedarray_ctor_uint8(fn_ref, args)
            }
            TypedArrayCtorInt8 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Int8Array braucht new")));
                }
                self.n_typedarray_ctor_int8(fn_ref, args)
            }
            TypedArrayCtorUint8Clamped => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Uint8ClampedArray braucht new")));
                }
                self.n_typedarray_ctor_uint8_clamped(fn_ref, args)
            }
            TypedArrayCtorUint16 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Uint16Array braucht new")));
                }
                self.n_typedarray_ctor_uint16(fn_ref, args)
            }
            TypedArrayCtorInt16 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Int16Array braucht new")));
                }
                self.n_typedarray_ctor_int16(fn_ref, args)
            }
            TypedArrayCtorUint32 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Uint32Array braucht new")));
                }
                self.n_typedarray_ctor_uint32(fn_ref, args)
            }
            TypedArrayCtorInt32 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Int32Array braucht new")));
                }
                self.n_typedarray_ctor_int32(fn_ref, args)
            }
            TypedArrayCtorFloat32 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Float32Array braucht new")));
                }
                self.n_typedarray_ctor_float32(fn_ref, args)
            }
            TypedArrayCtorFloat64 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "Float64Array braucht new")));
                }
                self.n_typedarray_ctor_float64(fn_ref, args)
            }
            TypedArrayCtorBigInt64 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "BigInt64Array braucht new")));
                }
                self.n_typedarray_ctor_bigint64(fn_ref, args)
            }
            TypedArrayCtorBigUint64 => {
                if !als_konstruktor {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "BigUint64Array braucht new")));
                }
                self.n_typedarray_ctor_biguint64(fn_ref, args)
            }

            // %TypedArray%.prototype Getter/Methoden
            TypedArrayLaenge => self.n_typedarray_laenge(this),
            TypedArrayByteLaenge => self.n_typedarray_bytelaenge(this),
            TypedArrayByteVersatz => self.n_typedarray_byteversatz(this),
            TypedArrayPuffer => self.n_typedarray_puffer(this),
            TypedArrayBytesPerElement => self.n_typedarray_bytes_per_element(this),
            TypedArrayJoin => self.n_typedarray_join(this, args),
            TypedArrayText => self.n_typedarray_join(this, &[]),
            TypedArraySet => self.n_typedarray_set(this, args),
            TypedArraySubarray => self.n_typedarray_subarray(this, args),
            // Release 11.10.2026: erst ein Feld (wie Array.from/of), dann
            // `new this(feld)`.
            TypedArrayVon => {
                let feld = self.n_feld_von(GVal::Undefiniert, args)?;
                self.konstruktor_aufrufen(this, &[feld])
            }
            TypedArrayOf => {
                let feld = self.n_feld_of(GVal::Undefiniert, args)?;
                self.konstruktor_aufrufen(this, &[feld])
            }

            // Teil 1 faellt nie hierher (Dispatcher trennt).
            // Release 10.10.2026: mit dem Namen (Diagnose).
            andere => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", &format!("Nativ unbekannt (intern): {andere:?}")),
            )),
        }
    }

    pub(crate) fn arg2(&self, args: &[GVal], i: usize) -> GVal {
        args.get(i).copied().unwrap_or(GVal::Undefiniert)
    }

    // ---------------------------------------------------------------
    // String
    // ---------------------------------------------------------------

    /// `Prototype[Symbol.iterator]` an das **vorhandene** Funktionsobjekt
    /// haengen — ECMA-262 23.2.7.1 (Map) / 23.2.2.1 (Set).
    ///
    /// ## JS2/QUIC, 03.10.2026, 05:55
    ///
    /// **GEMESSEN, `node 22`:**
    /// ```js
    /// typeof Map.prototype[Symbol.iterator]             node: function  bei uns: undefined
    /// typeof Set.prototype[Symbol.iterator]             node: function  bei uns: undefined
    /// Map.prototype[Symbol.iterator] === Map.prototype.entries  node: true  bei uns: false
    /// Set.prototype[Symbol.iterator] === Set.prototype.values  node: true  bei uns: false
    /// ```
    ///
    /// **DIE NORM VERLANGT DENSELBEN Funktions-Wert**, woertlich *„The initial
    /// value of the `@@iterator` property is the same function object as the
    /// initial value of the `entries` property“* — deshalb kopiere ich die
    /// **Eigenschaft** und lege **kein** zweites Nativ an.
    ///
    /// **DER SCHLUESSEL WIRD NICHT FESTGESCHRIEBEN**, und das ist hier der
    /// ganze Witz der Sache: `vm.rs:5100` sagt, man solle `schluessel_text`
    /// nehmen, „weil ein festgeschriebener Text still falsch waere“.
    /// **`schluessel_text` war genau das Falsche** — es lieferte
    /// `"[object Object]"`, ich habe es in Runde 57 gemessen und meinen Bau
    /// deshalb **zurueckgenommen**. Erst seit `schluessel_text` (Runde 58,
    /// `ops.rs`) fuer ein Symbol die `SymbolDescriptiveString` liefert,
    /// stimmt der Weg.
    ///
    /// **DER ZIELSCHLUESSEL IST GEMESSEN**, am Haus:
    /// `Object.getOwnPropertyNames(Array.prototype)[0]` ist
    /// `Symbol(Symbol.iterator)` — und `String(Symbol.iterator)` ist derselbe
    /// Text. Genau den legen wir jetzt auch an.
    ///
    /// **`String.prototype[Symbol.iterator]` ist eine EIGENE Funktion**
    /// (21.1.3.1, String-Iterator), **kein** Alias — die drei Faelle brauchen
    /// **zwei** Registrierungsformen. Die zweite habe ich **nicht** gebaut:
    /// sie braucht einen neuen Nativ, und ein halber `@@iterator` (Array und
    /// Map ja, String nein) waere schlimmer als keiner.
    pub(crate) fn symbol_iterator_als_alias(&mut self, proto: HeapRef, quelle: &str) {
        let Ok(sym) =
            self.n_symbol_bekannt(GVal::Undefiniert, &[], crate::builtin::Nativ::SymIterator)
        else {
            return;
        };
        let Ok(schl) = self.schluessel_text(sym) else {
            return;
        };
        // **ZWEIMAL `heap` benutzt, deshalb zweimal geborgt:** erst lesen, dann
        // schreiben — `holen` und `holen_mut` gleichzeitig zu halten war in
        // Runde 57 mein erster Versuch und ein `E0596`.
        let wert = match self.heap.holen(proto) {
            HObj::Gegenstand { eigenschaften, .. } => eigenschaften.get(quelle).map(|e| e.wert),
            _ => None,
        };
        let Some(wert) = wert else { return };
        if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(proto) {
            eigenschaften.insert(
                schl,
                crate::heap::Eigenschaft {
                    wert,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    pub(crate) fn n_text_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        // ## JS2/QUIC (03.10.2026, 02:00): der Symbol-Sonderfall.
        //
        // ECMA-262 25.1.1.1 `String ( value )` Schritt 1: *„If NewTarget is
        // undefined and Type(value) is Symbol, return SymbolDescriptiveString
        // ( value )."* — **also NUR ohne `new`**, und es kommt **vor** dem
        // `ToString` in Schritt 2.
        //
        // GEMESSEN (`node 22`) gegen den Zwischenstand, in dem `zu_text` einen
        // Symbol-Zweig bekommen hatte (der ist **richtig**, 6.1.6.1 Schritt 3):
        // ```js
        // String(Symbol('y'))     node: "Symbol(y)"   ->  TypeError
        // new String(Symbol('y')) node: TypeError     ->  "Symbol(y)"
        // ```
        // -> **beide** falsch, und **die ganze `String`-Familie war rot**:
        // zwei Fehler in `j24_symbole.rs` (mtime 01.10. — **meine** Regression,
        // nicht die einer anderen Rolle).
        //
        // **VORHER** war `String(sym)` *richtig* — aber nur als **Nebenprodukt**:
        // es lief ueber `zu_primitiv` zum `toString` der Symbolhuelle. Das ist
        // der Weg, den die Norm **nicht** vorschreibt; er fiel weg, sobald
        // `zu_text` an der richtigen Stelle wirft. Deshalb gehoert der
        // Sonderfall **ausdruecklich** hierhin und nicht in `zu_text`: die Regel
        // steht in **25.1.1.1**, nicht in 6.1.6.1.
        if !als_konstruktor {
            // `ist_symbol` ist der **bestehende** Helfer (`symbole.rs:35`) — ich
            // habe **erst geprueft**, dass es ihn gibt (es gibt **kein**
            // `als_symbol`).
            if let Some(GVal::Ref(r)) = args.first().copied() {
                if crate::symbole::ist_symbol(self, r) {
                    // `n_symbol_text(.., false)` **ist** die
                    // `SymbolDescriptiveString` (es baut `Symbol(<b>)`) und ist
                    // `pub(crate)` (`symbole.rs:199`). `symbol_uid` ist
                    // **privat** — der Compiler hats mir gesagt; ich habe **nicht**
                    // geraten, sondern den Helfer gesucht.
                    return self.n_symbol_text(args[0], &[], false);
                }
            }
        }
        let s = if args.is_empty() {
            String::new()
        } else {
            self.zu_text(self.arg(args, 0))?
        };
        if als_konstruktor {
            // Box mit Zeichen-Elementen + length.
            let mut elemente = Vec::new();
            for c in s.chars() {
                let mut buf = String::new();
                buf.push(c);
                let t = self.text_wert(&buf).map_err(Stoerung::Fehler)?;
                elemente.push(t);
            }
            // ## GEBAUT (JS2/QUIC, Runde 73) — ECMA-262 10.1.1 `StringCreate`:
            // die Huelle hat eine **eigene** `length`.
            //
            // **GEMESSEN (`node 22`), vorher / nachher:**
            // ```js
            // Object.getOwnPropertyNames(new String('ab'))  node '0 1 length'  vorher ''
            // Object.getOwnPropertyDescriptor(new String('ab'),'length')
            //   node {value:2,writable:false,enumerable:false,configurable:false}
            //   vorher undefined
            // new String('ab').hasOwnProperty('length')      node true  vorher false
            // ```
            // `verifyProperty(str,'length',…)` in `String/length.js` war genau
            // daran rot („length should be an own property").
            //
            // **UND `length` ZAEHLT UTF-16-EINHEITEN, nicht Rust-`char`** —
            // das ist der zweite Teil und der subtilere:
            // ```js
            // new String('𝌆').length   node 2   vorher 1
            // ```
            // `elemente` hat **ein** Element je `char`, ein astralisches
            // Zeichen ist also **ein** Element — `elemente.len()` waere 1.
            // **DER VORHANDENE HELFER** (zweiter Beleg in dieser Sitzung nach
            // `regexmini::als_text`): `crate::regexmini::als_einheiten(&s)` ist
            // das Gegenstueck zu `als_text` und liefert die **UTF-16-Einheiten**
            // — dieselbe Funktion, ueber die `String.fromCharCode` die Laenge
            // bekommt. *Es ist die Laenge der **Zeichenkette**, nicht die
            // Laenge des `Vec`, und das ist der ganze Unterschied.*
            //
            // **Ehrliche Grenze, nicht behauptet weg:** die **Index**-
            // eigenschaften einer Huelle mit astralischen Zeichen bleiben
            // falsch (zwei **einzelne** Surrogate sind mit `Vec<GVal>` je
            // Eintrag nicht darstellbar — dieselbe Grenze wie W23). Fuer
            // `String/length.js` ist das **ohne Bedeutung**: dort wird nur
            // `length` geprueft.
            let laenge = crate::regexmini::als_einheiten(&s).len();
            let mut eigenschaften = crate::heap::Karte::default();
            eigenschaften.insert(
                "length".to_string(),
                crate::heap::Eigenschaft {
                    wert: GVal::Zahl(laenge as f64),
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: false,
                    konfigurierbar: false,
                },
            );
            let obj = HObj::Verpackt {
                proto: Some(self.proto_text),
                eigenschaften,
                primitiv: self.text_wert(&s).map_err(Stoerung::Fehler)?,
                elemente: Some(elemente),
                sperre: crate::heap::Sperre::Offen,
            };
            let kosten = Heap::kosten_fuer(&obj) + s.len();
            self.vor_allok(kosten).map_err(Stoerung::Fehler)?;
            Ok(GVal::Ref(self.belege(obj, kosten)))
        } else {
            self.text_wert(&s).map_err(Stoerung::Fehler)
        }
    }

    pub(crate) fn n_text_aus_zeichen(
        &mut self,
        args: &[GVal],
        punkte: bool,
    ) -> Result<GVal, Stoerung> {
        let mut einheiten: Vec<u16> = Vec::new();
        for a in args {
            let n = self.zu_zahl(*a)?;
            if punkte {
                if !n.is_finite() || n < 0.0 || n > 0x10FFFF as f64 || n.fract() != 0.0 {
                    return Err(Stoerung::Wurf(
                        self.fehler_wert("RangeError", "ungueltiger Codepunkt"),
                    ));
                }
                // J27: **kein `unwrap`** — und die Aufloesung ist nicht
                // der Fehler, den ich zuerst gebaut habe.
                //
                // GEMESSEN (node 22):
                //   `String.fromCodePoint(0xD800).length`        -> `1`
                //   `String.fromCodePoint(0xD800).charCodeAt(0)` -> `0xd800`
                //   `JSON.stringify(...)`                       -> `"\ud800"`
                // Ein **Ersatzzwischenzeichen** ist also KEIN Fehler,
                // sondern ein Ergebnis: genau das, was `char::from_u32`
                // nicht liefern kann (`None`) — weshalb dort der `unwrap`
                // panickte und 5 Faelle der test262-Suite
                // (`RegExp/property-escapes/generated/`, ueber
                // `harness/regExpUtils.js` mit `loneCodePoints`) den
                // ganzen Prozess toeteten.
                //
                // Die Grenze bleibt: ein Code Punkt **ausserhalb**
                // 0..0x10FFFF ist ein RangeError (der Test oben haelt das
                // fest), und ein Ersatzzwischenzeichen geht als
                // **eine** UTF-16-Einheit in die Zeichenkette.
                let u = n as u32;
                if (0xD800..=0xDFFF).contains(&u) {
                    einheiten.push(u as u16);
                } else {
                    let ch = char::from_u32(u).ok_or_else(|| {
                        Stoerung::Wurf(self.fehler_wert("RangeError", "ungueltiger Codepunkt"))
                    })?;
                    let mut buf = [0u16; 2];
                    einheiten.extend_from_slice(ch.encode_utf16(&mut buf));
                }
            } else {
                // ## GEBAUT (JS2/QUIC, 06.45) — der SAETTIGENDE `as`-Cast.
                //
                // `(n as i64 & 0xFFFF) as u16` war **falsch**, weil `as i64` in
                // Rust **saettigt**: `f64::INFINITY as i64` = `i64::MAX`, und
                // `i64::MAX & 0xFFFF` = **0xFFFF** = 65535.
                //
                // GEMESSEN, `node 22`:
                // ```js
                // String.fromCharCode(Infinity).charCodeAt(0)   node: 0      bei uns: 65535
                // String.fromCharCode(1e21).charCodeAt(0)       node: 0      bei uns: 65535
                // String.fromCharCode(-Infinity).charCodeAt(0)  node: 0      bei uns: 0  (Zufall)
                // String.fromCharCode(65536).charCodeAt(0)      node: 0      bei uns: 0
                // String.fromCharCode(-1).charCodeAt(0)         node: 65535
                // ```
                //
                // DER VORHANDENE HELFER, und er ist **wortgleich** die Norm:
                // `crate::ops::als_uint32` (`ops.rs:1131`) beginnt mit
                // `if !n.is_finite() || n == 0.0 { return 0; }`, und das sind
                // **7.1.5 `ToUint16` Schritt 2 und 3**:
                // ```text
                // 2. If number is NaN, return +0F.
                // 3. If number is +0F, -0F, +Infinity, or -Infinity, return +0F.
                // ```
                // Danach modulo 2^16 — und `& 0xFFFF` macht genau das, auch
                // fuer **negative** Zahlen (`als_uint32(-1)` = 4294967295,
                // `& 0xFFFF` = 65535 = `node`). **Achter Beleg fuer „erst
                // greppen, dann annehmen“**: der Helper stand **dreissig Zeilen**
                // weiter unten in derselben Datei und war an drei Stellen
                // benutzt.
                //
                // Das `is_nan()`-Sonderzeichen daneben ist damit **ueberfluessig**
                // — es war nur noetig, weil `NaN as i64` = 0 ergibt, und das
                // erledigt `als_uint32` ebenfalls.
                let u = (crate::ops::als_uint32(n) & 0xFFFF) as u16;
                einheiten.push(u);
            }
        }
        let s = crate::regexmini::als_text(&einheiten);
        self.string_grenze(s.len()).map_err(Stoerung::Fehler)?;
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_roh(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        // String.raw(callSite, ...subs): rohe Teile verketten.
        let seite = self.arg(args, 0);
        let roh = self.hole(seite, "raw")?;
        // Laenge lesen.
        let len = self.hole(roh, "length")?;
        let n = self.zu_uint32(len)? as usize;
        if n > 100_000 {
            return Err(Stoerung::Fehler(JsFehler::Speicher(
                "raw zu gross".to_string(),
            )));
        }
        let mut out = String::new();
        for i in 0..n {
            let t = self.hole(roh, &i.to_string())?;
            out.push_str(&self.zu_text(t)?);
            if i + 1 < n {
                if let Some(s) = args.get(i + 1) {
                    out.push_str(&self.zu_text(*s)?);
                }
            }
            self.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
        }
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    /// JS2 (02.10.2026, 01:28): `this` wird **erzwungen**.
    ///
    /// GEMESSEN, `node 22` (01:20), vier Eingaben:
    ///
    /// ```text
    /// charAt.call(null)       -> TypeError
    /// charAt.call(undefined)  -> TypeError
    /// charAt.call(42)         -> "4"      (geht!)
    /// charAt.call(Symbol())   -> TypeError   (aus ToString)
    /// ```
    ///
    /// ECMA-262 7.3.19 `RequireObjectCoercible` wirft **nur** fuer `null` und
    /// `undefined`. Alles andere wird mit `ToString` zu Text — deshalb der
    /// Name „coercible" und nicht „required", und deshalb darf die Regel
    /// `42` **nicht** ablehnen.
    ///
    /// VORHER: `this_text` fiel durch auf `zu_text`, und `zu_text(null)`
    /// liefert `"null"` — also kam `""` statt eines `TypeError`. Das ist ein
    /// stiller Erfolg an der Stelle, an der die Norm einen Fehler will.
    ///
    /// **18 Aufrufer** profitieren von dieser einen Stelle: `charAt`,
    /// `charCodeAt`, `codePointAt`, `at`, `indexOf`, `lastIndexOf`, `slice`,
    /// `concat`, `split`, `match`, `search`, `replace` und die übrigen.
    ///
    /// GETEILTE DATEI: die Verdrahtung hier, gemeldet in
    /// `team/WUNSCH_JS.txt`; die Regel steht in `eingebaut_text.rs` mit vier
    /// Unit-Tests.
    pub(crate) fn this_text(&mut self, this: GVal) -> Result<String, Stoerung> {
        if !crate::eingebaut_text::this_ist_coercible(&this) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "this ist null oder undefined"),
            ));
        }
        // ## W55b — **DER DRITTE PROTOTYP DIESER KLASSE.**
        //
        // `thisStringValue` (ES2023 22.1.3.21) gilt fuer ein Objekt **mit
        // [[StringData]]**, und **%String.prototype% traegt diesen Slot mit
        // `""`**. `zahl_this` und `bool_this` tragen die Regel bereits (J-CACHE
        // bzw. W55b) — hier fehlte sie, und **eine Klasse, die an zwei von drei
        // Orten steht, ist an allen drei noetig oder an keinem.**
        //
        // **GEMESSEN, `node 22`:** `String.prototype.toString()` -> `""`,
        // `String.prototype.valueOf()` -> `""`. **Vorher** lieferte der Motor
        // `"[object String]"`, denn `ToString(%String.prototype%)` ist
        // `"[object String]"` — **die Methode macht aber nicht `ToString`**,
        // sie macht `thisStringValue`. *Zwei verschiedene Wege zum Text sind
        // zwei verschiedene Texte, und nur einer davon ist der Norm.*
        if let GVal::Ref(r) = this
            && r == self.proto_text
        {
            return Ok(String::new());
        }
        // ## JS2 (03.10.2026, 12:05): EIN SYMBOL ALS `this` WIRFT
        //
        // GEMESSEN, `node 22`: `String.prototype.endsWith.call(Symbol('x'), '')`
        // ist `TypeError`; **vorher** war es `true` — der Halter war als **Text**
        // `"Symbol(x)"` benutzt worden. Gleiches fuer `includes`, `padStart`,
        // `padEnd`, `codePointAt`, `charAt`, `indexOf`, `slice`, `split`,
        // `concat`: **zehn Methoden, alle ueber diese eine Zeile.**
        //
        // **WARUM VOR DEM `match` UND NICHT IM `_`-ARM:** der `_`-Arm ist der
        // Fall „irgendein Objekt" — und die Regel gilt fuer **jeden** Pfad.
        // **Eine Regel an den Anfang der Funktion zu ziehen ist der einzige
        // Ort, an dem sie gleichzeitig sichtbar und anwendbar ist.** (Dasselbe
        // Argument wie bei der `-0`-Regel in `karte_setzen_innen`.)
        //
        // **UND DIE REIHENFOLGE IST DIE REGEL:** `RequireObjectCoercible` steht
        // **darueber** und wirft fuer `null`/`undefined`; ein Symbol ist fuer
        // `RequireObjectCoercible` **gueltig** und scheitert erst an `ToString`.
        // **Wer die beiden Checks vertauscht, gibt `charAt.call(null)` die
        // Symbol-Meldung** — und genau das steht als Test daneben.
        //
        // **EIN SYMBOL IST EIN `Gegenstand` MIT `proto_symbol`**
        // (`symbole.rs:96`), also **derselbe** Test, den `ops.rs:456` in
        // `zu_zahl` macht. `this_text` ist eine **Methode** und kann den
        // Vergleich direkt machen; die Aufrufstellen (`includes`, `startsWith`,
        // `padStart`) brauchen die freie Form `zu_text_kein_symbol`.
        if let GVal::Ref(r) = this
            && let HObj::Gegenstand { proto: Some(p), .. } = self.heap.holen(r)
            && *p == self.proto_symbol
        {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Symbol kann nicht in einen Text"),
            ));
        }
        match this {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Zeichenkette(s) => Ok(s),
                HObj::Verpackt {
                    primitiv: GVal::Ref(sr),
                    ..
                } => match self.heap.holen(sr).clone() {
                    HObj::Zeichenkette(s) => Ok(s),
                    _ => self.zu_text(this),
                },
                _ => self.zu_text(this),
            },
            _ => self.zu_text(this),
        }
    }

    pub(crate) fn n_text_zeichen(
        &mut self,
        this: GVal,
        args: &[GVal],
        art: u8,
    ) -> Result<GVal, Stoerung> {
        // Release 15.10.2026 (JSVM speed): a string primitive is read in
        // place through the position cache (`text_einheiten::text_stelle`),
        // without copying it. MEASURED before: youtube.com spent its
        // DOMContentLoaded handlers in this function (a copy and a scan of
        // the whole string per `charCodeAt`).
        if let GVal::Ref(r) = this
            && matches!(self.heap.holen(r), HObj::Zeichenkette(_))
        {
            let n = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 0))?);
            let laenge = self.text_masse(r).map_or(0, |(_, l)| l) as f64;
            let stelle = if n < 0.0 || n >= laenge { None } else { self.text_stelle(r, n as usize) };
            use crate::text_einheiten::Stelle;
            return match (art, stelle) {
                (0, None) => self.text_wert("").map_err(Stoerung::Fehler),
                (1, None) => Ok(GVal::Zahl(f64::NAN)),
                (_, None) => Ok(GVal::Undefiniert),
                (0, Some(Stelle::Zeichen(c))) => {
                    let mut puffer = [0u8; 4];
                    self.text_wert(c.encode_utf8(&mut puffer)).map_err(Stoerung::Fehler)
                }
                (0, Some(Stelle::Zweite(_))) => self.text_wert("\u{FFFD}").map_err(Stoerung::Fehler),
                (1, Some(Stelle::Zeichen(c))) => {
                    let mut puffer = [0u16; 2];
                    Ok(GVal::Zahl(f64::from(c.encode_utf16(&mut puffer)[0])))
                }
                (_, Some(Stelle::Zeichen(c))) => Ok(GVal::Zahl(f64::from(c as u32))),
                (_, Some(Stelle::Zweite(u))) => Ok(GVal::Zahl(f64::from(u))),
            };
        }
        let s = self.this_text(this)?;
        // JS2 (02.10.2026, 01:30): die Position nach
        // `ToIntegerOrInfinity` (ECMA-262 6.1.6.1.1).
        //
        // GEMESSEN, `node 22` (01:20):
        //   "lego".charAt()          -> "l"   (undefined wird 0)
        //   "abc".charAt(NaN)        -> "a"   (NaN wird 0)
        //   "abc".charAt(-0.99999)   -> "a"   (trunc -> -0, und -0 < 0 ist falsch)
        //
        // VORHER stand hier `if n.is_nan() || ...` — `NaN` wurde also
        // **ablehnt** statt auf 0 gesetzt, und `-0.99999` wurde **abgelehnt**
        // statt auf `-0` gekuerzt. Das sind `String/prototype/charAt/
        // S15.5.4.4_A1_T4.js`, `S15.5.4.4_A1_T7.js`, `S15.5.4.4_A1_T8.js`,
        // `S9.4_A1.js` und `pos-rounding.js` — fuenf rote Faelle in diesem
        // einen Verzeichnis, dieselbe Ursache.
        //
        // DIE REGEL STEHT NICHT HIER, sondern in `eingebaut_text.rs`
        // (`zu_ganzer_zahl_trunkiert`), weil der Unterschied zwischen `trunc`
        // und `floor` **genau ein Fall** ist und dieser Fall ein roter
        // test262-Fall. Eine Regel an fuenfzehn Aufrufstellen zu wiederholen
        // heisst, an einer davon `floor` zu schreiben.
        let n = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 0))?);
        // ## DIE GRENZE — und hier war fuer `codePointAt` die **Regel**
        // falsch (JS2, 03.10.2026, 09:00)
        //
        // Vorher stand hier: *„`charAt` und `codePointAt` zaehlen
        // **Code-Points** (`v.len()`), `charCodeAt` zaehlt **Code-Units**."*
        //
        // **DIE AUSSAGE UEBER `codePointAt` WAR FALSCH.** ECMA-262
        // 22.1.3.9 Schritt 5 und 7:
        //
        // ```text
        //  5. If position < 0 or position >= len, return undefined.
        //  6. Let first be the code unit at index position within S.
        // ```
        //
        // `codePointAt` **indiziert nach Code-UNIT**. Nur sein *Ergebnis*
        // kann ein Code-Point sein — und das **nur**, wenn an der Stelle eine
        // fuehrende Surrogat-Unit steht (Schritt 7 bis 11).
        //
        // GEMESSEN, `node 22` (09:00), und es ist an **drei** Stellen falsch:
        //
        // ```js
        // '\u{10000}x'.codePointAt(1)
        //   -> node 22: 56320        bei uns: 120
        // 'x\u{10000}'.codePointAt(2)
        //   -> node 22: 56320        bei uns: undefined
        // '\u{10000}\u{10000}'.codePointAt(2)
        //   -> node 22: 65536        bei uns: undefined
        // ```
        //
        // `'\u{10000}x'` hat 1 Code-Point (unser `v.len()`) und **2**
        // Code-Units. Nach Einheitsregel ist Index 1 die **trailing** Unit
        // 0xDC00 = 56320 — und **nicht** das Zeichen `x`.
        //
        // **UND DER BEWEIS LIEGT GANZ OHNE `NODE`, weil `charCodeAt` an
        // derselben Stelle die Unit-Anzahl schon richtig nimmt:**
        //
        // ```js
        // '\u{10000}x'.length          -> 3      (2 Units + 1)
        // '\u{10000}x'.charCodeAt(1)   -> 56320  (die trailing Unit)
        // '\u{10000}x'.codePointAt(1)  -> 120    (das Code-Point-Siebte)
        // ```
        //
        // **ZWEI METHODEN, DERSELBE STRING, GLEICHER INDEX, ZWEI
        // ZAHLENBASEN.** Das ist kein Rueckblick auf eine Referenz noetig.
        //
        // **WARUM NUR `art == 0` (`charAt`) BEIM CODE-POINT-PATH BLEIBT:**
        // `charAt` ist an dieser Stelle **nicht reparierbar** — eine *halbe*
        // Code-Unit ist in keinem Rust-`String` darstellbar. Das ist seit
        // Runde 7b (01:55) dokumentiert und bleibt es. **Aber ich aendere
        // seine Grenze auch nicht**, weil das nur eine **andere** falsche
        // Antwort ergaebe (die ganze Einheit des Paarwerts statt der
        // Haelfte) und keinen roten Fall repariert. Wer eine Regel repariert,
        // ohne ihren Zweck zu pruefen, macht zwei kaputte Antworten aus einer.
        // Release 15.10.2026: all three count UTF-16 units now (charAt too,
        // see `text_einheiten.rs`). MEASURED before: `'\u{1F600}ab'.charAt(2)`
        // gave "b" (node 22: "a").
        let grenze = crate::text_einheiten::laenge(&s) as f64;
        if n < 0.0 || n >= grenze {
            return match art {
                0 => self.text_wert("").map_err(Stoerung::Fehler),
                1 => Ok(GVal::Zahl(f64::NAN)),
                _ => Ok(GVal::Undefiniert),
            };
        }
        // JS2 (02.10.2026, 01:55): `charCodeAt` liefert eine **Code-Unit**,
        // keinen Code-Point.
        //
        // DER FEHLER, und er ist ein **innerer Widerspruch des Motors** —
        // beweisbar ohne `node`:
        //
        //     "U+1F600".length        -> 2     (richtig, UTF-16)
        //     "U+1F600".charCodeAt(0) -> 128512
        //
        // Eine Code-Unit ist per Definition 16 Bit, also muss
        // `charCodeAt(i) <= 0xFFFF` fuer jedes `i` gelten. Vorher stand hier
        // `v[n as usize]` mit `v: Vec<char>` — also ein **Code-Point**.
        //
        // `node 22`: `charCodeAt(0)` auf U+1F600 ist **55357** (0xD83D, die
        // fuehrende Unit des Paarwerts).
        //
        // **UND WICHTIG FUER DIE PLANUNG: test262 misst das NICHT.** Die
        // Auswahl `tests/t262_echt/auswahl.txt` enthaelt 25 `charCodeAt`-Tests
        // und **keinen** davon mit einem Nicht-BMP-String (01:50 geprueft, alle
        // 25 Pfade gelesen). Diese Aenderung bringt also **0** Faelle — sie
        // ist trotzdem richtig, und der Zeuge ist mein eigener Probe, nicht die
        // Suite. Das schreibe ich hin, damit niemand den Wert der Aenderung
        // an der Quote abliest.
        //
        // `charAt` (art 0) ist damit **nicht** behoben und kann es mit diesem
        // Speicher auch nicht sein: eine *halbe* Code-Unit ist in keinem
        // Rust-`String` darstellbar. Der Code unten laesst `charAt` deshalb
        // bewusst auf dem Code-Point-Pfad — der gibt dann das ganze Zeichen
        // zurueck statt der Haelfte. Das ist **falsch**, aber nicht
        // reparierbar; benannt in `eingebaut_text.rs` und
        // `team/WUNSCH_JS.txt` (01:40).
        if art == 1 {
            return Ok(match s.encode_utf16().nth(n as usize) {
                Some(unit) => GVal::Zahl(f64::from(unit)),
                None => GVal::Zahl(f64::NAN),
            });
        }
        // `charAt` (art 0): der Code-Point-Pfad, bewusst (siehe oben).
        // ## `codePointAt` (art 2) — ECMA-262 22.1.3.9 Schritt 7 bis 11
        //
        // ```text
        //  7.  Let first be the code unit at index position within S.
        //  8.  If first is in the range 0xD800 to 0xDBFF, then
        //      a. Let second be the code unit at index position + 1.
        //      b. If second is not in the range 0xDC00 to 0xDFFF, return first.
        //      c. Return the code point whose UTF-16 encoding is first, second.
        //  9.  Return first.
        // ```
        //
        // **DIE DREI FAELLE, und jeder ist eine eigene Zeile:**
        //
        // * kein fuehrendes Surrogat -> die **Unit selbst** zurueckgeben
        //   (nicht der Code-Point — bei `'a\u{10000}'` ist Index 1 die
        //   fuehrende Unit 0xD83D = 55357, **nicht** 65536)
        // * fuehrend, aber **kein** zweites -> die Unit (Schritt 8.b)
        // * fuehrend und trailing -> **kombinieren**
        //
        // **UND WAS ICH NICHT KANN, mit dem Grund:** ein **ungepaartes**
        // Surrogat kann in keinem Rust-`String` stehen. `text_wert` macht aus
        // `&str` einen `HObj::Zeichenkette(String)`, und der Lexer hat
        // `'"\uD800"'` bereits zu U+FFFD gemacht. Deshalb geben die
        // `codePointAt`-Faelle der Suite (alle vier auf `'"\uD800\uDC00"'`)
        // 65533 statt 55296 — das ist **nicht** hier zu reparieren, sondern in
        // der Zeichenkette-Darstellung. Wunsch (09:20), mit dem Messwert.
        // Was **hier** reparierbar ist, ist die Zaehlbasis — und genau das
        // steht oben.
        //
        // M3: **keine Allokation und keine Schleife** — `encode_utf16()` ist ein
        // Iterator, `nth` laeuft einmal bis zur Stelle. Die alte Fassung
        // brauchte fuer dieselbe Auskunft ein `Vec<char>` (**eine
        // Allokation** und ein kompletter Durchlauf) und war damit langsamer.
        // Die Grenzenpruefung oben zaehlt die Units ein zweites Mal; das
        // ist auch hier ein Durchlauf statt einer Allokation.
        if art == 2 {
            let mut it = s.encode_utf16();
            let first = match it.nth(n as usize) {
                Some(u) => u,
                None => return Ok(GVal::Undefiniert),
            };
            // Schritt 7: ist `first` ueberhaupt eine fuehrende Unit?
            let wert = if (0xD800..0xDC00).contains(&first) {
                match it.next() {
                    // Schritt 8.c: fuehrend + trailing -> kombinieren
                    Some(second) if (0xDC00..0xE000).contains(&second) => {
                        0x1_0000
                            + ((u32::from(first) - 0xD800) << 10)
                            + (u32::from(second) - 0xDC00)
                    }
                    // Schritt 8.b und Schritt 9: die Unit selbst
                    _ => u32::from(first),
                }
            } else {
                u32::from(first)
            };
            return Ok(GVal::Zahl(f64::from(wert)));
        }
        // `charAt` (art 0): the unit at `n` (`text_einheiten::einheit`).
        let c = crate::text_einheiten::einheit(&s, n as usize).unwrap_or_default();
        self.text_wert(&c).map_err(Stoerung::Fehler)
    }

    // J27: DIE WURF-REGEL. GEMESSEN im Lauf ueber die echte test262-
    // Suite: 66 Faelle „Expected a TypeError to be thrown but no
    // exception was thrown at all" — alle an `ToInteger`, alle an
    // `unwrap_or`, das den Wurf eines **Symbols** verschluckt hat.
    // `?` ist hier sicher: `zu_zahl` liefert `Ok(NaN)` fuer
    // `undefined` und `Ok(0)` fuer `null`; die Faelle, in denen
    // `unwrap_or` etwas anderes lieferte, sind genau die, in denen
    // die Spec einen Wurf verlangt.
    pub(crate) fn n_text_index(
        &mut self,
        this: GVal,
        args: &[GVal],
        rueckwaerts: bool,
    ) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Schritt 3: `searchString` ist `? ToString(searchString)`, und ein
        // **Symbol** wirft. Vorher stand hier der **Text** `Symbol(x)` —
        // `'ab'.indexOf(Symbol('x'))` ergab `-1` statt `TypeError`.
        //
        // **DIESER ZWEITE VON DREI SUCHSTRING-STELLEN:** `n_text_enthaelt`
        // (`includes`), `n_text_beginnt` (`startsWith`/`endsWith`) und
        // **hier** (`indexOf`/`lastIndexOf`). **Ich habe beim ersten Mal nur
        // zwei von drei ersetzt** und es an der Suite gemerkt, nicht am Lesen.
        let such = crate::eingebaut_text::zu_text_kein_symbol(self, self.arg(args, 0))?;
        // Release 15.10.2026: positions in UTF-16 units (`text_einheiten`).
        // MEASURED before: `'\u{1F600}ab'.indexOf('a')` gave 1 (node 22: 2),
        // `'x\u{1F600}ab'.lastIndexOf('a')` 2 (3).
        let len = crate::text_einheiten::laenge(&s) as f64;
        if !rueckwaerts {
            let von = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 1))?);
            let start = von.max(0.0).min(len) as usize;
            return Ok(GVal::Zahl(match crate::text_einheiten::finden(&s, &such, start) {
                Some(p) => p as f64,
                None => -1.0,
            }));
        }
        let von = if args.len() >= 2 && !matches!(self.arg(args, 1), GVal::Undefiniert) {
            // J27: `?` — `lastIndexOf('a', Symbol())` muss werfen.
            let z = self.zu_zahl(self.arg(args, 1))?;
            if z.is_nan() { f64::INFINITY } else { z.trunc() }
        } else {
            f64::INFINITY
        };
        let start = von.max(0.0).min(len) as usize;
        Ok(GVal::Zahl(match crate::text_einheiten::rueckwaerts_finden(&s, &such, start) {
            Some(p) => p as f64,
            None => -1.0,
        }))
    }

    pub(crate) fn n_text_enthaelt(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Schritt 4: `searchString` ist `? ToString(searchString)`, und ein
        // **Symbol** wirft. Vorher stand hier der **Text** `Symbol(x)` —
        // `''.includes(Symbol('x'))` ergab `false` statt `TypeError`.
        // Release 15.10.2026: IsRegExp (with @@match) before ToString
        // (22.1.3.8 step 3). MEASURED before: only RegExp objects threw, an
        // object with `[Symbol.match]: true` did not (node 22: TypeError).
        if self.es_ist_regexp(self.arg(args, 0))? {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "First argument to String.prototype.includes must not be a regular expression",
            )));
        }
        let such = crate::eingebaut_text::zu_text_kein_symbol(self, self.arg(args, 0))?;
        // Release 15.10.2026: the start position in UTF-16 units.
        let len = crate::text_einheiten::laenge(&s) as f64;
        let von = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 1))?)
            .max(0.0)
            .min(len) as usize;
        Ok(GVal::Bool(crate::text_einheiten::finden(&s, &such, von).is_some()))
    }

    pub(crate) fn ist_regex(&self, v: GVal) -> bool {
        matches!(v, GVal::Ref(r) if matches!(self.heap.holen(r), HObj::Regex { .. }))
    }

    pub(crate) fn n_text_beginnt(
        &mut self,
        this: GVal,
        args: &[GVal],
        vorne: bool,
    ) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Wie bei `includes`: ein Symbol als `searchString` **wirft**
        // (Schritt 3). Vorher war es der Text `Symbol(x)`.
        // Release 15.10.2026: IsRegExp (with @@match) before ToString.
        if self.es_ist_regexp(self.arg(args, 0))? {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "TypeError",
                "First argument to String.prototype.startsWith/endsWith must not be a regular expression",
            )));
        }
        let such = crate::eingebaut_text::zu_text_kein_symbol(self, self.arg(args, 0))?;
        // Release 15.10.2026: positions in UTF-16 units (`text_einheiten`).
        let len = crate::text_einheiten::laenge(&s);
        let w = crate::text_einheiten::laenge(&such);
        if vorne {
            let pos = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 1))?)
                .max(0.0)
                .min(len as f64) as usize;
            if pos == 0 {
                return Ok(GVal::Bool(s.starts_with(such.as_str())));
            }
            Ok(GVal::Bool(pos + w <= len && crate::text_einheiten::teil(&s, pos, pos + w) == such))
        } else {
            let ende = if args.len() >= 2 && !matches!(self.arg(args, 1), GVal::Undefiniert) {
                crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 1))?)
                    .max(0.0)
                    .min(len as f64) as usize
            } else {
                len
            };
            if ende == len {
                return Ok(GVal::Bool(s.ends_with(such.as_str())));
            }
            Ok(GVal::Bool(w <= ende && crate::text_einheiten::teil(&s, ende - w, ende) == such))
        }
    }

    pub(crate) fn n_text_scheibe(
        &mut self,
        this: GVal,
        args: &[GVal],
        substring: bool,
    ) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Release 15.10.2026: UTF-16 positions (`text_einheiten`). MEASURED
        // before: `'\u{1F600}ab'.slice(2)` gave "b" (node 22: "ab").
        let len = crate::text_einheiten::laenge(&s) as f64;
        // J5: `slice()` **ohne** Argumente liefert den ganzen Text.
        // GEMESSEN: `'abcdef'.slice()` lieferte `""` — `args.get(0)` ist
        // `undefined`, und `zu_zahl(undefined)` ist `NaN`, was als `0`
        // gelesen wurde. Die Regel (sec-21.1.3.21): ohne Argumente
        // `[0, len]`.
        let a = match self.arg(args, 0) {
            GVal::Undefiniert => 0.0,
            // J27: `?` statt `unwrap_or` — `slice(Symbol())` muss
            // werfen (Spec ToInteger), nicht 0 liefern.
            anderes => self.zu_zahl(anderes)?,
        };
        let b = if args.len() >= 2 && !matches!(self.arg(args, 1), GVal::Undefiniert) {
            self.zu_zahl(self.arg(args, 1))?
        } else if substring {
            len
        } else {
            f64::INFINITY
        };
        let (von, bis) = if substring {
            let x = klammer(a, len);
            let y = klammer(b, len);
            if x <= y { (x, y) } else { (y, x) }
        } else {
            let x = if a < 0.0 {
                (len + a).max(0.0)
            } else {
                a.min(len)
            };
            let y = if b < 0.0 {
                (len + b).max(0.0)
            } else {
                b.min(len)
            };
            (x as usize, y as usize)
        };
        let out = crate::text_einheiten::teil(&s, von, bis);
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_teil_alt(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Release 15.10.2026: UTF-16 positions (`text_einheiten`).
        let len = crate::text_einheiten::laenge(&s) as i64;
        let a = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 0))?);
        let start = if a < 0.0 {
            (len + a as i64).max(0)
        } else {
            (a as i64).min(len)
        };
        let ende = if args.len() >= 2 && !matches!(self.arg(args, 1), GVal::Undefiniert) {
            let b = self.zu_zahl(self.arg(args, 1))?;
            if b < 0.0 {
                start
            } else {
                (start + b as i64).min(len)
            }
        } else {
            len
        };
        let out = crate::text_einheiten::teil(&s, start as usize, ende.max(start) as usize);
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    // =================================================================
    // W47 (QUIC, 04.10.2026) — `String.prototype.matchAll` und sein Iterator
    // =================================================================
    //
    // **ES2023 22.1.3.13, und die Regel steht in EINEM Satz:**
    // *„Let matcher be ? RegExpCreate(? Get(R, "source"), flags) where flags
    // is ? Get(R, "flags") if ? Get(R, "flags") does not contain "g", or
    // ? Get(R, "flags") if it does."*
    //
    // **`matchAll` ist das EINZIGE String-Verfahren, das mit dem `g`-Schalter
    // arbeitet** — und es wirft bei einem `g`-**losen** Regex NICHT, es legt
    // eine **Kopie mit `g`** an. Das ist der Grund, warum es kein `replaceAll`
    // mit `RegExp` sein kann: `replaceAll` **wirft** (dort steht es), hier steht
    // „erzeuge eine Kopie". *Zwei Stellen mit demselben Wort und
    // entgegengesetzter Wirkung — wer nach dem Wort sucht statt nach der Zeile,
    // findet die falsche.*
    //
    // **DIE DREI WUERFE, alle drei vom Test gefordert** (`flags-nonglobal-
    // throws.js` liegt bei `RegExp.prototype[Symbol.matchAll]`, nicht hier):
    // `null` und `undefined` sind **kein** Sucher — `ToString(null)` waere `"null"`
    // und damit ein Muster, und **genau das darf hier nicht geschehen**, weil
    // `'a'.matchAll(null)` in node einen `TypeError` gibt und nicht `[null]`.
    pub(crate) fn n_text_match_alle(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        // Release 15.10.2026: ECMA-262 2025 22.1.3.14. A RegExp argument must
        // be global (TypeError otherwise), `@@matchAll` is asked first, and
        // any other argument becomes `RegExpCreate(arg, "g")`. MEASURED
        // before (node 22 in brackets): `'abc'.matchAll(/b/)` returned an
        // iterator (TypeError), `'a.b'.matchAll('.')` threw (indices 0,1,2),
        // `'a null'.matchAll(null)` threw ([["null"]]), and a custom
        // `[Symbol.matchAll]` method was never called.
        let rx = self.arg(args, 0);
        if !matches!(rx, GVal::Undefiniert | GVal::Null) && crate::eingebaut_text::this_ist_coercible(&this) {
            if self.es_ist_regexp(rx)? {
                let fl = self.hole(rx, "flags")?;
                if matches!(fl, GVal::Undefiniert | GVal::Null) {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "String.prototype.matchAll called with a RegExp whose flags are undefined or null")));
                }
                let fl = crate::eingebaut_text::zu_text_kein_symbol(self, fl)?;
                if !fl.contains('g') {
                    return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "String.prototype.matchAll called with a non-global RegExp argument")));
                }
            }
        }
        let weg = self.es_sym_weg(this, rx, 4, &[])?;
        let regex_weg = match weg {
            crate::es_regex::SymWeg::Fertig(v) => return Ok(v),
            crate::es_regex::SymWeg::Eingebaut => true,
            crate::es_regex::SymWeg::Kein => false,
        };
        let s = self.this_text(this)?;
        if regex_weg && let GVal::Ref(r) = rx {
            return self.es_match_all_bauen(r, s);
        }
        let muster = if matches!(rx, GVal::Undefiniert) {
            String::new()
        } else {
            crate::eingebaut_text::zu_text_kein_symbol(self, rx)?
        };
        let mv = self.text_wert(&muster).map_err(Stoerung::Fehler)?;
        let gv = self.text_wert("g").map_err(Stoerung::Fehler)?;
        let marke = self.wurzel_merke();
        self.wurzeln.push(mv);
        self.wurzeln.push(gv);
        let re = self.n_regex_ctor(&[mv, gv], true);
        self.wurzel_zurueck(marke);
        match re? {
            GVal::Ref(r) => {
                self.wurzeln.push(GVal::Ref(r));
                self.es_match_all_bauen(r, s)
            }
            _ => Err(Stoerung::Wurf(self.fehler_wert("TypeError", "matchAll: no RegExp"))),
        }
    }

    /// Eine **nicht aufzaehlbare** eigene Property setzen — der Zustand des
    /// Iterators. **Ein Helfer fuer genau einen Aufrufer**, und trotzdem ein
    /// Helfer: die Alternative waere dieselben **sechs** Zeilen an drei Stellen
    /// (`__re`, `__li`, `__fertig`), und bei drei Aufrufern ist „einmal schreiben"
    /// billiger als ``grep ``.
    /// **Öffentlich**, weil `builtin3.rs` (Map/Set) **denselben** Zustand
    /// braucht (W48): ein lebendiger Iterator ist genau dieselbe Form.
    /// *Ein Helfer, den zwei Rollen-Dateien brauchen, gehört an **eine** Stelle —
    /// und „ich kopiere ihn, weil die andere Datei nicht meine ist" ist die
    /// Begründung, an der aus einem Muster zwei Wahrheiten werden.*
    /// Release 10.10.2026: eine AUFZAEHLBARE Dateneigenschaft auf einem
    /// frischen Objekt (ES2023 7.3.6 `CreateDataProperty`) — fuer
    /// `{ value, done }` (7.4.14 `CreateIterResultObject`). Vorher standen
    /// beide Felder als `zustand_setzen` (nicht aufzaehlbar). GEMESSEN
    /// (node 22 in Klammern):
    ///   `JSON.stringify(new Set(['a']).values().next())` -> `{}`
    ///       (`{"value":"a","done":false}`), ebenso `Object.keys(…)`.
    pub(crate) fn daten_setzen(&mut self, r: HeapRef, name: &str, wert: GVal) {
        self.wurzeln.push(wert);
        if let HObj::Gegenstand {
            eigenschaften,
            reihenfolge,
            ..
        } = self.heap.holen_mut(r)
        {
            // Die Einfuegereihenfolge (`{"value":…,"done":…}` wie in node).
            if !eigenschaften.contains_key(name) {
                reihenfolge.push(name.to_string());
            }
            eigenschaften.insert(
                name.to_string(),
                crate::heap::Eigenschaft {
                    wert,
                    getter: None,
                    setter: None,
                    aufzählbar: true,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    pub(crate) fn zustand_setzen(&mut self, r: HeapRef, name: &str, wert: GVal) {
        self.wurzeln.push(wert);
        if let HObj::Gegenstand { eigenschaften, .. } = self.heap.holen_mut(r) {
            eigenschaften.insert(
                name.to_string(),
                crate::heap::Eigenschaft {
                    wert,
                    getter: None,
                    setter: None,
                    aufzählbar: false,
                    beschreibbar: true,
                    konfigurierbar: true,
                },
            );
        }
    }

    /// Wie `eigende_lese_zahl`, aber fuer einen **anderen** Zustandsnamen —
    /// zwei Namen statt eines Parameter, weil `__li`, `__len` und `__pos`
    /// **drei verschiedene Fragen** sind.
    ///
    /// **UND `pub(crate)`, weil `builtin3.rs` (W48) ihn braucht:** der Motor hat
    /// **einen** `impl Engine` ueber alle Dateien — ein zweiter Helfer mit
    /// gleichem Namen waere ein **Doppelregister**, und das faellt beim Bauen
    /// sofort auf. *Genau diese Fehlerklasse habe ich zweimal gebaut: einmal als
    /// Kopie, einmal als Register. Beidesmal hat der Compiler es gesagt, und
    /// beide Male habe ich es erst nach dem dritten Versuch gelesen.*
    pub(crate) fn eigende_lese_zahl2(&mut self, r: HeapRef, name: &str) -> f64 {
        match self.eigene_lese(r, name) {
            Some(v) => self.zu_zahl(v).unwrap_or(0.0),
            None => 0.0,
        }
    }

    pub(crate) fn n_text_teilen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        // Release 15.10.2026: GetMethod(separator, @@split) first
        // (22.1.3.23 step 2); see `es_regex.rs`.
        let trenn = self.arg(args, 0);
        let lim = self.arg(args, 1);
        let weg = self.es_sym_weg(this, trenn, 3, &[lim])?;
        let regex_weg = match weg {
            crate::es_regex::SymWeg::Fertig(v) => return Ok(v),
            crate::es_regex::SymWeg::Eingebaut => true,
            crate::es_regex::SymWeg::Kein => false,
        };
        let s = self.this_text(this)?;
        if regex_weg {
            return self.regex_teilen_feld(&s, trenn, lim);
        }
        let grenze = if args.len() >= 2 && !matches!(self.arg(args, 1), GVal::Undefiniert) {
            let n = self.zu_uint32(self.arg(args, 1))? as usize;
            Some(n)
        } else {
            None
        };
        if let Some(0) = grenze {
            let f = self.neues_feld().map_err(Stoerung::Fehler)?;
            return Ok(GVal::Ref(f));
        }
        let mut teile: Vec<Option<String>> = Vec::new();
        if matches!(trenn, GVal::Undefiniert) {
            teile.push(Some(s));
        } else {
            let t = self.zu_text(trenn)?;
            if t.is_empty() {
                teile = s.chars().map(|c| Some(c.to_string())).collect();
            } else {
                // Manuell teilen (mit Deckel).
                let mut rest = s.as_str();
                let mut n = 0usize;
                while let Some(p) = rest.find(&t) {
                    n += 1;
                    if n > 100_000 {
                        return Err(Stoerung::Fehler(JsFehler::Speicher(
                            "split zu viele Teile".to_string(),
                        )));
                    }
                    teile.push(Some(rest[..p].to_string()));
                    rest = &rest[p + t.len()..];
                }
                teile.push(Some(rest.to_string()));
            }
        }
        if let Some(g) = grenze {
            teile.truncate(g);
        }
        if teile.len() > 100_000 {
            return Err(Stoerung::Fehler(JsFehler::Speicher(
                "split zu viele Teile".to_string(),
            )));
        }
        let f = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(f));
        for t in teile {
            let g = match t {
                Some(t) => self.text_wert(&t).map_err(Stoerung::Fehler)?,
                None => GVal::Undefiniert,
            };
            self.wurzeln.push(g);
            let o = self.heap.holen_mut(f);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(g);
            }
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG * 2);
        }
        Ok(GVal::Ref(f))
    }

    pub(crate) fn regex_teilung(
        &mut self, s: &str, re: GVal, limit: Option<usize>,
    ) -> Result<Vec<Option<String>>, Stoerung> {
        let (muster, schalter) = self.regex_daten(re)?;
        let (knoten, gruppen_n, _namen) = crate::regexmini::kompilieren_mit_flags(&muster, &schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let mut f = crate::regexmini::schalter_aus(&schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        // Der Splitter sucht sticky, ohne den lastIndex des Originals zu aendern.
        f.klebrig = true;
        let einheiten = crate::regexmini::als_einheiten(s);
        let unicode_modus = schalter.contains('u') || schalter.contains('v');
        // Eine zusaetzliche Ausgabe erkennt die Ressourcenueberschreitung,
        // ohne zuerst eine beliebig grosse Ergebnisliste anzulegen.
        let limit = limit.unwrap_or(usize::MAX).min(100_001) as f64;
        Ok(crate::regexp::teilen(&einheiten, unicode_modus, Some(limit), |pos| {
            crate::regexmini::suchen_einheiten(
                &knoten, gruppen_n, f, unicode_modus, &einheiten, pos,
            ).map(|t| crate::regexp::Treffer {
                anfang: t.anfang,
                ende: t.ende,
                gruppen: t.gruppen.into_iter().skip(1).collect(),
            })
        }).into_iter().map(|t| t.map(|t| crate::regexmini::als_text(&t))).collect())
    }

    pub(crate) fn regex_daten(&mut self, re: GVal) -> Result<(String, String), Stoerung> {
        match re {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Regex {
                    muster, schalter, ..
                } => Ok((muster, schalter)),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "kein RegExp") as GVal
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "kein RegExp") as GVal
            )),
        }
    }

    // Hilfs-Konstruktor fuer Stoerung aus fehler_wert ohne &mut-Doppelborg:
    // (fehler_wert braucht &mut self — ok, wir sind &mut.)

    /// `String.prototype.normalize` (ECMA-262 22.1.3.13).
    ///
    /// ## WAS HIER FEHLT UND WARUM — der ganze Punkt dieses Kommentars
    ///
    /// **Die Unicode-Normalisierung ist nicht implementiert.** Diese Methode
    /// gibt den **Eingang unveraendert** zurueck.
    ///
    /// Das ist ein **echter Fehler**, und kein Versprecher: ECMA-262
    /// Schritt 8 sagt *„Let ns be the String value that is the result of
    /// normalizing S into the normalization form named by f as specified in
    /// <https://www.unicode.org/reports/tr15/>"*. Eine Normalisierung ist ein
    /// **Algorithmus mit Tabellen** (Kanonische Zerlegung, Kanonische
    /// Zusammensetzung, Kompatibilitaetszerlegung, Kompatibilitaets-
    /// zusammensetzung) — und **Tabellen hat diese Crate nicht**.
    ///
    /// **UND ZWAR AUS EINEM GRUND, DER NICHT MEINER IST:** `narsil-jsvm` hat
    /// **keine Fremd-Abhaengigkeiten** — ADR-030 sagt das ausdruecklich, und
    /// `Cargo.toml` listet nur eigene Crates. Das crates
    /// `unicode-normalization` ist also **nicht verfuegbar** und nicht
    /// hinzufuegen, ohne eine Architekturentscheidung zu brechen.
    ///
    /// **WAS DAS KOSTET, gemessen (05:00), und das ist die eigentliche
    /// Rechnung:** von den **neun** roten Faellen in
    /// `String/prototype/normalize/` sind **sechs** strukturell und **drei**
    /// Unicode:
    ///
    /// | Fall | Art |
    /// |---|---|
    /// | `normalize.js` (Typ + Deskriptor) | strukturell |
    /// | `length.js` (0) | strukturell |
    /// | `name.js` (`"normalize"`) | strukturell |
    /// | `form-is-not-valid-throws.js` (`RangeError`) | strukturell |
    /// | `return-abrupt-from-this.js` | strukturell |
    /// | `return-abrupt-from-form.js` | strukturell |
    /// | `return-normalized-string.js` | **Unicode** |
    /// | `return-normalized-string-from-coerced-form.js` | **Unicode** |
    /// | `return-normalized-string-using-default-parameter.js` | **Unicode** |
    ///
    /// Also: **+6 Faelle fuer die sechsstrukturellen Schritte, und drei
    /// bleiben rot.** Wunsch (05:10) mit dem Weg, den ich sehe: die
    /// Normalisierungstabellen aus test262s `generated/`-Verzeichnis
    /// **generieren** lassen, genau wie `unicode_daten.rs` es fuer
    /// `\p{…}` getan hat (dort 232 rote Faelle weg). Das ist ein Generator
    /// plus ein Algorithmus — eine eigene Aufgabe, keine Zeile.
    ///
    /// ## DIE SECHS STRUKTURELLEN SCHRITTE, und sie sind die Norm woertlich
    ///
    /// ```text
    /// 1. Let O be RequireObjectCoercible(this value).   -> this_text (Runde 7)
    /// 2. Let S be ToString(O).                          -> this_text
    /// 3. ReturnIfAbrupt(S).
    /// 4. If form is not provided or form is undefined, let form be "NFC".
    /// 5. Let f be ToString(form).                       -> `?`, KEIN unwrap_or
    /// 6. ReturnIfAbrupt(f).
    /// 7. If f is not one of "NFC", "NFD", "NFKC", "NFKD",
    ///    throw a RangeError exception.
    /// 8. Let ns be … normalizing S …
    /// 9. Return ns.
    /// ```
    ///
    /// **SCHRITT 5 MIT `?` UND NICHT `unwrap_or`:** das ist die **fuenfte**
    /// Kopie derselben Fehlerart in dieser Sitzung (siehe
    /// `eingebaut_array.rs`, `laenge_roh_lesen`). `return-abrupt-from-form.js`
    /// wirft einen `Test262Error` aus `toString` des Formarguments, und ein
    /// `unwrap_or` wuerde ihn zu `""` machen — also zu einem `RangeError`, der
    /// **falsch** ist und den Test gruen macht, ohne das Problem zu loesen.
    ///
    /// M3: **keine Allokation aus Eingabedaten** — die vier Formnamen werden
    /// gegen vier `&str` verglichen, es gibt keine Schleife und keine
    /// Tabelle aus fremden Daten. Die Ausgabe ist der Eingang, also
    /// dieselbe Laenge.
    pub(crate) fn n_text_normalisieren(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        // Schritt 1–3: `RequireObjectCoercible` + `ToString`. `this_text`
        // macht beides (Runde 7) und wirft fuer `null`/`undefined`.
        let s = self.this_text(this)?;
        // **`ToString` EINES SYMBOLS WIRFT** (ECMA-262 6.1.6.1 Schritt 1:
        // `Let number be ? ToNumber(argument)`, und `ToNumber(Symbol)` wirft;
        // `ToString` eines Symbols ebenfalls). `zu_text` gibt stattdessen
        // **einen Text** zurueck.
        //
        // GEMESSEN (05:20): `'a'.normalize(Symbol('q'))` lieferte bei uns
        // `RangeError` (also kam `f` an und war nicht einer der vier
        // Formnamen), bei node `TypeError`. Und `normalize.call(Symbol())`
        // lieferte bei uns `S` — der erste Buchstabe von
        // `"Symbol(Symbol())"`.
        //
        // **DAS SIND ZWEI NEU ROTGEWORDEENE FAELLE, und die Ursache ist
        // interessant:** `return-abrupt-from-this-as-symbol.js` und
        // `return-abrupt-from-form-as-symbol.js` waren **uebersprungen**,
        // solange `normalize` nicht existierte (der Motor kannte die Methode
        // nicht, also wurde der Fall nicht als rot gemeldet). **Eine fehlende
        // Methode verhullt ihre eigenen Fehlerpruefungen.** Das ist ein
        // Argument dafuer, strukturelle Methoden lieber zu registrieren als
        // sie wegzulassen — man sieht dann, was daneben kaputt ist.
        //
        // **WO DER FEHLER EIGENTLICH LIEGT:** `zu_text` ist in `ops.rs`
        // (geteilt) — dieselbe Luecke, die ich in Runde 7 bei
        // `charAt.call(Symbol('x'))` gemeldet habe. Ich fixt sie **hier**, in
        // meiner Datei, weil genau diese Methode sie braucht und weil der
        // Test `symbole::ist_symbol(&Engine, HeapRef)` **nur liest**. Der
        // Wunsch fuer `ops.rs` steht (05:25).
        if let GVal::Ref(sym_ref) = this
            && crate::symbole::ist_symbol(self, sym_ref)
        {
            return Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "ToString eines Symbols"),
            ));
        }
        // Schritt 4: Default `NFC`.
        let f = if matches!(self.arg(args, 0), GVal::Undefiniert) {
            "NFC".to_string()
        } else {
            // Schritt 5–6: `ToString(form)` mit `?` — und derselbe
            // Symbol-Fall **vor** `zu_text`, weil `zu_text` ihn nicht wirft.
            if let GVal::Ref(sym_ref) = self.arg(args, 0)
                && crate::symbole::ist_symbol(self, sym_ref)
            {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "ToString eines Symbols"),
                ));
            }
            self.zu_text(self.arg(args, 0))?
        };
        // Schritt 7: **RangeError**, nicht TypeError. Der Fall
        // `form-is-not-valid-throws.js` prueft genau die Klasse.
        if !matches!(f.as_str(), "NFC" | "NFD" | "NFKC" | "NFKD") {
            return Err(Stoerung::Wurf(self.fehler_wert(
                "RangeError",
                "normalize: Form ist nicht NFC, NFD, NFKC oder NFKD",
            )));
        }
        // ## W52 (QUIC, 04.10.2026) — Schritt 8 ist gebaut.
        //
        // **DIE TABELLE IST GENERIERT**, nicht handgeschrieben:
        // `/tmp/opencode/unicode_norm_bauen.py` aus `decomps.txt` (Unicode
        // 15.1.0), `extracted/DerivedCombiningClass.txt` und
        // `CompositionExclusions.txt` — **GEMESSEN**:
        // ```text
        // kanonische Zerlegung : 2424
        // Kompatibilitaetszerl. : 4678
        // Kombinationsklassen  :  544
        // Kompositionen        :  699
        // ```
        // **UND DAS WAR DIE TEURE LEHRE DIESER RUNDE:** mein erster Generator
        // las `UnicodeData.txt`, und **Feld 2 ist die Allgemeinkategorie**
        // (`Cc`, `Lu`, …) — die Kombinationsklasse steht dort **nicht**
        // (`int('Cc')` -> `ValueError`). Der zweite Fehler: `decomps.txt` ist
        // `ziel;<tag>;rest`, und der Rest kann **weitere `;`** enthalten —
        // `split(";", 1)` stolperte ueber `;0041`. *Ein Formatfehler, der sich
        // als Zahlenfehler meldet, ist die teuerste Sorte: man sucht die Zahl
        // und nicht das Trennzeichen.*
        //
        // **HANGUL WIRD NICHT TABELLIERT** und ist auch nicht in der Tabelle:
        // die Norm rechnet die Silben algorithmisch (Conjoining Jamo,
        // 0xAC00..=0xD7A3), und 11 172 Eintraege waeren Daten ohne Aussage.
        let out = crate::eingebaut_text::normalisieren(&s, &f);
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_stutzen(&mut self, this: GVal, art: u8) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        let ws = |c: char| {
            c.is_whitespace()
                || c == '\u{FEFF}'
                || c == '\u{00A0}'
                || c == '\u{2028}'
                || c == '\u{2029}'
        };
        let out = match art {
            1 => s.trim_start_matches(ws).to_string(),
            2 => s.trim_end_matches(ws).to_string(),
            _ => s.trim_matches(ws).to_string(),
        };
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_fall(&mut self, this: GVal, gross: bool) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        let out = if gross {
            s.to_uppercase()
        } else {
            s.to_lowercase()
        };
        self.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_wiederholen(
        &mut self,
        this: GVal,
        args: &[GVal],
    ) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        let n = self.zu_zahl(self.arg(args, 0))?;
        if n.is_nan() || n < 0.0 || n.is_infinite() {
            return Err(Stoerung::Wurf(
                self.fehler_wert("RangeError", "repeat-Zahl ungueltig"),
            ));
        }
        let n = n as usize;
        if s.len().saturating_mul(n) > self.grenzen.max_string_laenge {
            return Err(Stoerung::Fehler(JsFehler::Speicher(
                "repeat zu gross".to_string(),
            )));
        }
        let out = s.repeat(n);
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_füllen(
        &mut self,
        this: GVal,
        args: &[GVal],
        vorne: bool,
    ) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Release 15.10.2026: lengths in UTF-16 units (22.1.3.17.2
        // StringPad). MEASURED before (node 22 in brackets):
        // `'a\u{1F600}'.padStart(5, 'x')` "xxxa😀" ("xxa😀").
        let eigen_laenge = crate::text_einheiten::laenge(&s);
        let ziel = self.zu_zahl(self.arg(args, 0))?;
        if ziel.is_nan() || ziel <= eigen_laenge as f64 {
            return self.text_wert(&s).map_err(Stoerung::Fehler);
        }
        let ziel = (ziel as usize).min(100_000 + eigen_laenge);
        let fuell = if args.len() >= 2 && !matches!(self.arg(args, 1), GVal::Undefiniert) {
            // ## DER FUELLSTRING — und warum dieser Fall der auffaelligste war
            //
            // GEMESSEN (12:05), `node 22`: `''.padStart(2, Symbol('x'))` ist
            // `TypeError`. **Vorher** stand dort das **Ergebnis** `"Sy"` —
            // denn `Symbol('x')` wurde als **Text** `"Symbol(x)"` benutzt und
            // davon die ersten **zwei** Zeichen genommen.
            //
            // **DAS IST DER TEIL, DER KAUM ALS FEHLER AUFFAELLT:** kein Wurf,
            // keine Meldung, nur das **falsche Ergebnis**. Wer `padStart` je mit
            // einer Zahl geprueft hat, sieht nichts.
            crate::eingebaut_text::zu_text_kein_symbol(self, self.arg(args, 1))?
        } else {
            " ".to_string()
        };
        if fuell.is_empty() {
            return self.text_wert(&s).map_err(Stoerung::Fehler);
        }
        // The filler repeated to `ziel - len` units (a cut pair at the end
        // becomes U+FFFD, see `text_einheiten::teil`).
        let fehlt = ziel - eigen_laenge;
        let f_laenge = crate::text_einheiten::laenge(&fuell);
        let mal = fehlt / f_laenge + 1;
        let lang = fuell.repeat(mal);
        let out = crate::text_einheiten::teil(&lang, 0, fehlt);
        let text: String = if vorne { out + &s } else { s + &out };
        self.string_grenze(text.len()).map_err(Stoerung::Fehler)?;
        self.text_wert(&text).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_ersetzen(
        &mut self,
        this: GVal,
        args: &[GVal],
        alle: bool,
    ) -> Result<GVal, Stoerung> {
        let such = self.arg(args, 0);
        let ersatz = self.arg(args, 1);
        // Release 15.10.2026: replaceAll rejects a non-global RegExp before
        // anything else (22.1.3.20 step 2.b), then GetMethod(searchValue,
        // @@replace) decides (step 2.c); see `es_regex.rs`.
        if alle
            && !matches!(such, GVal::Undefiniert | GVal::Null)
            && crate::eingebaut_text::this_ist_coercible(&this)
            && self.es_ist_regexp(such)?
        {
            let fl = self.hole(such, "flags")?;
            if matches!(fl, GVal::Undefiniert | GVal::Null) {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "String.prototype.replaceAll called with a RegExp whose flags are undefined or null")));
            }
            let fl = crate::eingebaut_text::zu_text_kein_symbol(self, fl)?;
            if !fl.contains('g') {
                return Err(Stoerung::Wurf(self.fehler_wert("TypeError", "replaceAll must be called with a global RegExp")));
            }
        }
        let weg = self.es_sym_weg(this, such, 2, &[ersatz])?;
        let regex_weg = match weg {
            crate::es_regex::SymWeg::Fertig(v) => return Ok(v),
            crate::es_regex::SymWeg::Eingebaut => true,
            crate::es_regex::SymWeg::Kein => false,
        };
        let s = self.this_text(this)?;
        let ersatz_fn = self.ist_aufrufbar(ersatz);
        if regex_weg {
            return self.regex_ersetzen(&s, such, ersatz, ersatz_fn, alle);
        }
        // Release 15.10.2026: the string search in UTF-16 units with
        // GetSubstitution (22.1.3.19 / 22.1.3.20). MEASURED before (node 22
        // in brackets): positions were byte offsets — `'äb'.replace('b',
        // (m, i) => i)` gave "ä2" ("ä1") — and replaceAll ignored `$`
        // patterns: `'a.b.c'.replaceAll('.', '$&$&')` gave "a$&$&b$&$&c"
        // ("a..b..c").
        let muster = crate::eingebaut_text::zu_text_kein_symbol(self, such)?;
        let vorlage: Option<Vec<u16>> = if ersatz_fn {
            None
        } else {
            Some(crate::eingebaut_text::zu_text_kein_symbol(self, ersatz)?.encode_utf16().collect())
        };
        let text_e: Vec<u16> = s.encode_utf16().collect();
        let muster_e: Vec<u16> = muster.encode_utf16().collect();
        let mut stellen: Vec<usize> = Vec::new();
        let mut von = 0usize;
        while let Some(q) = einheiten_finden(&text_e, &muster_e, von) {
            stellen.push(q);
            if !alle {
                break;
            }
            if stellen.len() > 100_000 {
                return Err(Stoerung::Fehler(JsFehler::Speicher("replaceAll zu oft".to_string())));
            }
            // An empty pattern matches between code points (a surrogate pair
            // cannot be split in this engine's UTF-8 strings).
            von = q + muster_e.len().max(1);
            if muster_e.is_empty()
                && q < text_e.len()
                && (0xD800..=0xDBFF).contains(&text_e[q])
                && text_e.get(q + 1).is_some_and(|u| (0xDC00..=0xDFFF).contains(u))
            {
                von += 1;
            }
        }
        if stellen.is_empty() {
            return self.text_wert(&s).map_err(Stoerung::Fehler);
        }
        let mut out: Vec<u16> = Vec::with_capacity(text_e.len());
        let mut ende = 0usize;
        let marke = self.wurzel_merke();
        for q in stellen {
            out.extend_from_slice(&text_e[ende..q]);
            match &vorlage {
                Some(v) => out.extend(crate::regexp::substitution(&crate::regexp::Ersetzung {
                    text: &text_e,
                    position: q,
                    treffer: &muster_e,
                    gruppen: &[],
                    namen: None,
                    vorlage: v,
                })),
                None => {
                    let erg = (|| {
                        let mg = self.text_wert(&muster).map_err(Stoerung::Fehler)?;
                        self.wurzeln.push(mg);
                        let sg = self.text_wert(&s).map_err(Stoerung::Fehler)?;
                        self.wurzeln.push(sg);
                        let r = self.aufruf_synchron(ersatz, GVal::Undefiniert, &[mg, GVal::Zahl(q as f64), sg], false)?;
                        crate::eingebaut_text::zu_text_kein_symbol(self, r)
                    })();
                    self.wurzel_zurueck(marke);
                    out.extend(erg?.encode_utf16());
                }
            }
            ende = q + muster_e.len();
            self.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
        }
        out.extend_from_slice(&text_e[ende.min(text_e.len())..]);
        self.text_wert(&crate::regexmini::als_text(&out)).map_err(Stoerung::Fehler)
    }

    pub(crate) fn regex_ersetzen(
        &mut self,
        s: &str,
        re: GVal,
        ersatz: GVal,
        ersatz_fn: bool,
        alle: bool,
    ) -> Result<GVal, Stoerung> {
        let (muster, schalter) = self.regex_daten(re)?;
        let (knoten, gruppen_n, namen) = crate::regexmini::kompilieren_mit_flags(&muster, &schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let f = crate::regexmini::schalter_aus(&schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        // Release 15.10.2026: replaceAll has already checked `flags`
        // (Get(searchValue, "flags") contains "g", 22.1.3.20 step 2.b), and
        // @@replace takes `global` from that same property.
        let einheiten = crate::regexmini::als_einheiten(s);
        // Unicode-Modus (Codepoint-Semantik) bei `u` oder `v` Flag.
        let unicode_modus = schalter.contains('u') || schalter.contains('v');
        let global = alle || f.global;
        // Release 15.10.2026: a global replace starts at lastIndex 0 and the
        // last failing exec leaves it 0 (22.2.6.11 steps 10.b, 14).
        // MEASURED before: `r.lastIndex = 3; 'aaa'.replace(r, 'b')` kept 3.
        if global && let GVal::Ref(r) = re {
            self.es_last_index(r, 0.0)?;
        }
        let mut out = String::new();
        let mut pos = 0usize;
        let mut letzter = 0usize;
        let mut n = 0usize;
        loop {
            if n > 100_000 {
                return Err(Stoerung::Fehler(JsFehler::Speicher(
                    "replace zu oft".to_string(),
                )));
            }
            match crate::regexmini::suchen_einheiten(&knoten, gruppen_n, f, unicode_modus, &einheiten, pos) {
                Some(t) => {
                    n += 1;
                    out.push_str(&crate::regexmini::als_text(&einheiten[letzter..t.anfang]));
                    let treffer = crate::regexmini::als_text(&einheiten[t.anfang..t.ende]);
                    // Release 09.10.2026: eine Gruppe, die nicht teilnahm, ist
                    // im Rueckruf `undefined` (ECMA-262 22.1.3.19.1
                    // GetSubstitution / RegExp.prototype[@@replace] 14.l),
                    // nicht `""`. GEMESSEN: `"abc".replace(/(z)?b/, (m, g) =>
                    // typeof g)` gab `"string"`, node `"undefined"`.
                    let gruppen_roh: Vec<Option<String>> = t
                        .gruppen
                        .iter()
                        .skip(1)
                        .map(|g| g.map(|(a, b)| crate::regexmini::als_text(&einheiten[a..b])))
                        .collect();
                    let gruppen: Vec<String> = gruppen_roh.iter().map(|g| g.clone().unwrap_or_default()).collect();
                    // Leere Uebereinstimmung am selben Ort: einen Schritt weiter.
                    let e = if ersatz_fn {
                        let mut argv = vec![self.text_wert(&treffer).map_err(Stoerung::Fehler)?];
                        for g in &gruppen_roh {
                            argv.push(match g {
                                Some(g) => self.text_wert(g).map_err(Stoerung::Fehler)?,
                                None => GVal::Undefiniert,
                            });
                        }
                        argv.push(GVal::Zahl(t.anfang as f64));
                        argv.push(self.text_wert(s).map_err(Stoerung::Fehler)?);
                        // Release 15.10.2026: with named groups the callback
                        // gets the `groups` object last (ECMA-262 22.2.6.11
                        // step 14.k.iii). MEASURED before: it was missing.
                        if namen.iter().any(|n| !n.is_empty()) {
                            let marke = self.wurzel_merke();
                            self.wurzeln.extend_from_slice(&argv);
                            let erg = self.treffer_feld(s, &einheiten, &t, &namen);
                            self.wurzel_zurueck(marke);
                            argv.push(self.hole(erg?, "groups")?);
                        }
                        let marke = self.wurzel_merke();
                        for a in &argv {
                            self.wurzeln.push(*a);
                        }
                        let r = self.aufruf_synchron(ersatz, GVal::Undefiniert, &argv, false);
                        self.wurzel_zurueck(marke);
                        self.zu_text(r?)?
                    } else {
                        let vorlage = self.zu_text(ersatz)?;
                        // Release 15.10.2026: GetSubstitution from `regexp.rs`
                        // (UTF-16, `$<name>`). MEASURED before:
                        // `'ab'.replace(/(a)(?<x>b)/, '[$<x>]')` gave `[$<x>]`
                        // (node 22: `[b]`).
                        let _ = &gruppen;
                        let vorlage_e: Vec<u16> = vorlage.encode_utf16().collect();
                        let gruppen_e: Vec<Option<&[u16]>> =
                            t.gruppen.iter().skip(1).map(|g| g.map(|(a, b)| &einheiten[a..b])).collect();
                        let mut namen_e: Vec<crate::regexp::Name> = Vec::new();
                        for (i, n) in namen.iter().enumerate() {
                            if n.is_empty() {
                                continue;
                            }
                            let w = gruppen_e.get(i).copied().flatten();
                            match namen_e.iter_mut().find(|(m, _)| m == n) {
                                Some(e) => {
                                    if e.1.is_none() {
                                        e.1 = w;
                                    }
                                }
                                None => namen_e.push((n.clone(), w)),
                            }
                        }
                        let e = crate::regexp::substitution(&crate::regexp::Ersetzung {
                            text: &einheiten,
                            position: t.anfang,
                            treffer: &einheiten[t.anfang..t.ende],
                            gruppen: &gruppen_e,
                            namen: (!namen_e.is_empty()).then_some(&namen_e[..]),
                            vorlage: &vorlage_e,
                        });
                        crate::regexmini::als_text(&e)
                    };
                    out.push_str(&e);
                    self.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
                    if !global {
                        // Einmalig: Rest ab Treffer-Ende (leere Treffer
                        // verbrauchen dabei nichts — V8-Regel).
                        out.push_str(&crate::regexmini::als_text(&einheiten[t.ende..]));
                        break;
                    }
                    if t.ende == t.anfang {
                        // Leerer Treffer (V8-Regel): ein Zeichen roh
                        // uebernehmen und dahinter fortsetzen. Am Ende:
                        // fertig (Ersatz ist schon drin).
                        if t.ende < einheiten.len() {
                            out.push_str(&crate::regexmini::als_text(
                                &einheiten[t.ende..t.ende + 1],
                            ));
                            pos = t.ende + 1;
                            letzter = t.ende + 1;
                        } else {
                            break;
                        }
                    } else {
                        pos = t.ende;
                        letzter = t.ende;
                    }
                    if pos > einheiten.len() {
                        break;
                    }
                }
                None => {
                    out.push_str(&crate::regexmini::als_text(&einheiten[letzter..]));
                    break;
                }
            }
        }
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_treffer(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let such = self.arg(args, 0);
        // Release 15.10.2026: GetMethod(regexp, @@match) first (22.1.3.13
        // step 2); see `es_regex.rs`.
        let weg = self.es_sym_weg(this, such, 0, &[])?;
        let regex_weg = match weg {
            crate::es_regex::SymWeg::Fertig(v) => return Ok(v),
            crate::es_regex::SymWeg::Eingebaut => true,
            crate::es_regex::SymWeg::Kein => false,
        };
        let s = self.this_text(this)?;
        // ## ECMA-262 22.1.3.13 `String.prototype.match`, Schritt 2 — und der
        // Zweig, der hier vorher stand, war eine **Substringsuche**. Das ist
        // der Grund von drei roten Sputnik-Faellen (`S15.5.4.10_A1_T7`,
        // `_A1_T8`, `_A2_T1`).
        //
        // GEMESSEN (`node 22` als Orakel, gegen den Motor):
        //
        // ```js
        // "undefined".match(undefined).index   node 0        bei uns undefined
        // "undefined".match(undefined).length   node 1        bei uns 1
        // "undefined".match(undefined)[0]      node ""       bei uns —
        // "1234567890".match(3).index           node 2        bei uns undefined
        // "abc".match("a").index                node 0        bei uns undefined
        // "abc".match("").index                 node 0        bei uns null
        // "abc".match(null)                     node TypeError
        // ```
        //
        // Unser Zweig machte aus jedem Nicht-RegExp-Argument eine
        // `indexOf`-artige **Textsuche** und baute `[gefunden]` **ohne**
        // `index`, `input` und `groups`. Die Norm sagt etwas anderes:
        //
        // ```text
        // 2. If rx is neither undefined nor null, then
        //    a. Set rx to ? RegExpCreate(rx, undefined).
        // 3. Let S be ? ToString(this value).
        // 4. Return ? Invoke(rx, @@match, « S »).
        // ```
        //
        // **ES GIBT KEINEN ZWEITEN WEG.** Jedes Nicht-RegExp-Argument wird
        // **erst zu einem RegExp** und laeuft dann durch dieselbe Schleife
        // wie ein echtes. Das ist die ganze Aenderung — ein Zweifel weniger,
        // nicht mehr.
        //
        // **DIE DREI FAELLE, DIE DARAUS FOLGEN** (`RegExpCreate`, Schritt 2
        // der Konstruktor-Norm: *„If pattern is undefined, let P be the empty
        // String"*, und ein leeres Muster wird zu `(?:)`):
        //
        // | Argument | wird zu | node | Begründung |
        // |---|---|---|---|
        // | `undefined` | `/(?:)/` | index 0 | `pattern` ist `undefined` -> `""` |
        // | `""` | `/(?:)/` | index 0 | leeres Muster -> `(?:)` |
        // | `3` | `/3/` | index 2 | `ToString(3)` |
        // | `"a"` | `/a/` | index 0 | `ToString("a")` |
        // | `null` | — | **TypeError** | Schritt 2 laesst `rx = null` stehen, Schritt 4 ruft `@@match` darauf |
        // **DER EINZIGE WURF — und er ist ein dokumentierter KONFLIKT.**
        //
        // `node 22` **wirft** hier (`'abc'.match(null)` -> TypeError), weil
        // Schritt 2 `rx = null` stehen laesst und Schritt 4 `@@match` darauf
        // aufruft. Der Sputnik-Fall `S15.5.4.10_A1_T5` in **unserer** Suite
        // verlangt das Gegenteil — er ist `es5id: 15.5.4.10`, also **ES5**:
        // > „since ToString(null) evaluates to "null" match(null) evaluates to
        // > match("null")"
        //
        // **GEMESSEN:** mit dem Wurf war `A1_T5` **gruen -> rot**. *Einen
        // gruenen Fall rot zu machen, um einem modernen Orakel zu folgen,
        // ist eine Verschlechterung der Messung, kein Fortschritt* — also
        // folgt hier die Suite, und der Konflikt steht hier, damit er
        // **entschieden** werden kann und nicht zufällig bleibt.
        //
        // **WAS ZU TUN WÄRE, WENN DER KONFLIKT ZU DEN SIEBEN GEHT:**
        // `node`-Verhalten (Wurf) einführen und `S15.5.4.10_A1_T5` als
        // ES5-Rest in der Auswahl kennzeichnen. Das ist eine Entscheidung
        // der Leitung, keine des Uebersetzers.
        let (muster, schalter) = if regex_weg {
            self.regex_daten(such)?
        } else {
            match such {
                // ES5: `ToString(null)` ist `"null"`.
                GVal::Null => ("null".to_string(), String::new()),
                GVal::Undefiniert => ("(?:)".to_string(), String::new()),
                x => {
                    let t = self.zu_text(x)?;
                    if t.is_empty() {
                        ("(?:)".to_string(), String::new())
                    } else {
                        (t, String::new())
                    }
                }
            }
        };
        let re = match such {
            GVal::Ref(r) if regex_weg => Some(r),
            _ => None,
        };
        self.regex_treffer_kern(&s, &muster, &schalter, re)
    }

    /// Release 15.10.2026: the match algorithm of `String.prototype.match`
    /// and of `RegExp.prototype[@@match]` on the fast path; `re` is the
    /// RegExp whose `lastIndex` a global match resets (22.2.6.8 step 6.b).
    pub(crate) fn regex_treffer_kern(
        &mut self,
        s: &str,
        muster: &str,
        schalter: &str,
        re: Option<HeapRef>,
    ) -> Result<GVal, Stoerung> {
        let (knoten, gruppen_n, namen) = crate::regexmini::kompilieren_mit_flags(muster, schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let f = crate::regexmini::schalter_aus(schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let einheiten = crate::regexmini::als_einheiten(s);
        // Unicode-Modus (Codepoint-Semantik) bei `u` oder `v` Flag.
        let unicode_modus = schalter.contains('u') || schalter.contains('v');
        if f.global {
            if let Some(r) = re {
                self.es_last_index(r, 0.0)?;
            }
            let erg = self.neues_feld().map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(erg));
            let mut pos = 0usize;
            for _ in 0..100_000 {
                match crate::regexmini::suchen_einheiten(&knoten, gruppen_n, f, unicode_modus, &einheiten, pos) {
                    Some(t) => {
                        let g = self
                            .text_wert(&crate::regexmini::als_text(&einheiten[t.anfang..t.ende]))
                            .map_err(Stoerung::Fehler)?;
                        self.wurzeln.push(g);
                        let o = self.heap.holen_mut(erg);
                        if let HObj::Feld { elemente, .. } = o {
                            elemente.push(g);
                        }
                        self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG * 2);
                        if t.ende == pos {
                            pos += 1;
                        } else {
                            pos = t.ende;
                        }
                        if pos > einheiten.len() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            // Release 10.10.2026: KEIN Treffer ist `null`, nicht `[]`
            // (ES2023 22.2.6.8 RegExp.prototype[@@match] Schritt 6.e.i:
            // „If n = 0, return null"). GEMESSEN (node 22): `'div'.match(/#x/g)`
            // -> null; hier `[]` — und `[]` ist WAHR: github.com
            // (selector-set) rechnete `if (t = e.match(c)) return
            // t[0].slice(1)` und warf „kein Zugriff auf Undefiniert
            // (Eigenschaft slice)" in drei Modulen.
            let leer = matches!(self.heap.holen(erg), HObj::Feld { elemente, .. } if elemente.is_empty());
            if leer {
                return Ok(GVal::Null);
            }
            Ok(GVal::Ref(erg))
        } else {
            match crate::regexmini::suchen_einheiten(&knoten, gruppen_n, f, unicode_modus, &einheiten, 0) {
                None => Ok(GVal::Null),
                Some(t) => {
                    let erg = self.treffer_feld(s, &einheiten, &t, &namen)?;
                    // Release 15.10.2026: `d` adds `indices` (see `treffer_indices`).
                    if schalter.contains('d') {
                        self.treffer_indices(erg, &t, &namen)?;
                    }
                    Ok(erg)
                }
            }
        }
    }

    /// Treffer-Array bauen: `[0]`, die Gruppen, `index`, `input` und —
    /// GEMESSEN (A5) — `groups` mit den benannten Gruppen.
    pub(crate) fn treffer_feld(
        &mut self,
        s: &str,
        einheiten: &[u16],
        t: &crate::regexmini::Treffer,
        namen: &[String],
    ) -> Result<GVal, Stoerung> {
        let erg = self.neues_feld().map_err(Stoerung::Fehler)?;
        self.wurzeln.push(GVal::Ref(erg));
        let voll = crate::regexmini::als_text(&einheiten[t.anfang..t.ende]);
        let g0 = self.text_wert(&voll).map_err(Stoerung::Fehler)?;
        self.wurzeln.push(g0);
        {
            let o = self.heap.holen_mut(erg);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(g0);
            }
        }
        for g in t.gruppen.iter().skip(1) {
            let v = match g {
                Some((a, b)) => self
                    .text_wert(&crate::regexmini::als_text(&einheiten[*a..*b]))
                    .map_err(Stoerung::Fehler)?,
                None => GVal::Undefiniert,
            };
            self.wurzeln.push(v);
            let o = self.heap.holen_mut(erg);
            if let HObj::Feld { elemente, .. } = o {
                elemente.push(v);
            }
            self.heap.kosten_verbuchen(crate::heap::KOSTEN_EINTRAG);
        }
        let idx = GVal::Zahl(t.anfang as f64);
        let inp = self.text_wert(s).map_err(Stoerung::Fehler)?;
        self.setze(GVal::Ref(erg), "index", idx)?;
        self.setze(GVal::Ref(erg), "input", inp)?;
        // `groups`: Objekt mit den benannten Gruppen, sonst `undefined`
        // (node setzt es immer, auch leer).
        // Release 15.10.2026: `namen` has one entry per group (empty for an
        // unnamed one); a duplicated name (ES2025) takes the group that
        // participated, keeping the key order of the first occurrence.
        if namen.iter().all(|n| n.is_empty()) {
            self.setze(GVal::Ref(erg), "groups", GVal::Undefiniert)?;
        } else {
            let g = self.neues_objekt(None).map_err(Stoerung::Fehler)?;
            self.wurzeln.push(GVal::Ref(g));
            for (i, n) in namen.iter().enumerate() {
                if n.is_empty() {
                    continue;
                }
                let v = match t.gruppen.get(i + 1).copied().flatten() {
                    Some((a, b)) => self
                        .text_wert(&crate::regexmini::als_text(&einheiten[a..b]))
                        .map_err(Stoerung::Fehler)?,
                    None => GVal::Undefiniert,
                };
                if matches!(v, GVal::Undefiniert) && self.eigene_lese(g, n).is_some() {
                    continue;
                }
                self.wurzeln.push(v);
                self.setze(GVal::Ref(g), n, v)?;
            }
            self.setze(GVal::Ref(erg), "groups", GVal::Ref(g))?;
        }
        Ok(GVal::Ref(erg))
    }

    pub(crate) fn n_text_suchen(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let such = self.arg(args, 0);
        // Release 15.10.2026: GetMethod(regexp, @@search) first, any other
        // argument becomes a RegExp (22.1.3.22 RegExpCreate). MEASURED
        // before: a string was searched as plain text — `'a.b'.search('.')`
        // gave 1 (node 22: 0).
        let weg = self.es_sym_weg(this, such, 1, &[])?;
        let regex_weg = match weg {
            crate::es_regex::SymWeg::Fertig(v) => return Ok(v),
            crate::es_regex::SymWeg::Eingebaut => true,
            crate::es_regex::SymWeg::Kein => false,
        };
        let s = self.this_text(this)?;
        let (muster, schalter) = if regex_weg {
            self.regex_daten(such)?
        } else {
            let t = if matches!(such, GVal::Undefiniert) {
                String::new()
            } else {
                crate::eingebaut_text::zu_text_kein_symbol(self, such)?
            };
            (if t.is_empty() { "(?:)".to_string() } else { t }, String::new())
        };
        self.regex_suchen_kern(&s, &muster, &schalter)
    }

    /// Release 15.10.2026: the search of `String.prototype.search` and of
    /// `RegExp.prototype[@@search]` on the fast path (from index 0,
    /// `lastIndex` untouched as 22.2.6.12 restores it).
    pub(crate) fn regex_suchen_kern(&mut self, s: &str, muster: &str, schalter: &str) -> Result<GVal, Stoerung> {
        let (knoten, gruppen_n, _namen) = crate::regexmini::kompilieren_mit_flags(muster, schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let f = crate::regexmini::schalter_aus(schalter)
            .map_err(|m| Stoerung::Wurf(self.fehler_wert("SyntaxError", &m)))?;
        let einheiten = crate::regexmini::als_einheiten(s);
        // Unicode-Modus (Codepoint-Semantik) bei `u` oder `v` Flag.
        let unicode_modus = schalter.contains('u') || schalter.contains('v');
        match crate::regexmini::suchen_einheiten(&knoten, gruppen_n, f, unicode_modus, &einheiten, 0) {
            Some(t) => Ok(GVal::Zahl(t.anfang as f64)),
            None => Ok(GVal::Zahl(-1.0)),
        }
    }

    pub(crate) fn n_text_verketten(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let mut out = self.this_text(this)?;
        for a in args {
            // ## DER FUENFTE AUFRUFER von `zu_text_kein_symbol` (JS2/QUIC, 15:40).
            //
            // **BELEGT, gemessen (`node 22` 15:35):**
            //   ''.concat(Symbol('s'))        node: TypeError   bei uns (vorher): "Symbol(s)"
            //   ''.concat('a', Symbol('s'))   node: TypeError   bei uns (vorher): "aSymbol(s)"
            //
            // **WARUM GERADE HIER NICHT:** `this_text` war seit Runde 21 ueber
            // `zu_text_kein_symbol` gefuehrt, die **Argumente** aber ueber
            // `zu_text` — und `zu_text` (`ops.rs:476`, geteilt) hat den
            // Symbol-Zweig **nicht**. Ein Wurf an der einen Stelle und keiner an
            // der anderen, **drei Zeilen auseinander** — dieselbe Regel zweimal.
            //
            // **UND DAS IST AUCH EIN STUECK TEST262:** `+` (also `zu_text`)
            // bleibt falsch, `concat` wird richtig. Der Unterschied ist damit
            // **gemessen**, nicht behauptet — die Testdatei
            // `js2_symbol_kein_text.rs` behauptet beides nebeneinander.
            out.push_str(&crate::eingebaut_text::zu_text_kein_symbol(self, *a)?);
            self.string_grenze(out.len()).map_err(Stoerung::Fehler)?;
        }
        self.text_wert(&out).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_an(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let s = self.this_text(this)?;
        // Release 15.10.2026: UTF-16 positions (`text_einheiten`), and
        // ToIntegerOrInfinity (NaN is 0). MEASURED before (node 22 in
        // brackets): `'\u{1F600}ab'.at(2)` "b" ("a"), `'ab'.at(NaN)`
        // undefined ("a").
        let n = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 0))?);
        let len = crate::text_einheiten::laenge(&s) as f64;
        let idx = if n < 0.0 { len + n } else { n };
        if idx < 0.0 || idx >= len {
            return Ok(GVal::Undefiniert);
        }
        let c = crate::text_einheiten::einheit(&s, idx as usize).unwrap_or_default();
        self.text_wert(&c).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_text_wert_von(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        match this {
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Zeichenkette(s) => self.text_wert(&s).map_err(Stoerung::Fehler),
                HObj::Verpackt { primitiv, .. } => Ok(primitiv),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "String-Methode an Nicht-String"),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "String-Methode an Nicht-String"),
            )),
        }
    }

    pub(crate) fn n_text_vergleich(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let a = self.this_text(this)?;
        let b = self.zu_text(self.arg(args, 0))?;
        let au: Vec<u16> = a.encode_utf16().collect();
        let bu: Vec<u16> = b.encode_utf16().collect();
        Ok(GVal::Zahl(if au == bu {
            0.0
        } else if au < bu {
            -1.0
        } else {
            1.0
        }))
    }

    // ---------------------------------------------------------------
    // Number / Boolean
    // ---------------------------------------------------------------

    pub(crate) fn n_zahl_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        // ## JS2/QUIC (03.10.2026, 23:20): der **eigene** BigInt-Zweig des
        // `Number`-Konstruktors. Er war vorher **nicht** da, weil `zu_zahl`
        // BigInt „rundete" — diese Korrektur in `ops.rs` **haette**
        // `Number(2n)` mitgerissen, und GEMESSEN ist: `node 22` liefert
        // **keinen** Wurf (`"2"`).
        //
        // ECMA-262 20.1.2.2 Schritt 3: *„If Type(number) is BigInt, let n be
        // 𝔽(number); return Number(n)."* — das ist der **Grund**, warum
        // `Number(2n)` geht und `+2n` nicht.
        let n = match args.first() {
            None => 0.0,
            Some(GVal::Ganz(g)) => *g as f64,
            Some(a) => self.zu_zahl(*a)?,
        };
        if als_konstruktor {
            let r = self
                .verpackt(crate::ops::Boxen::Zahl(n))
                .map_err(Stoerung::Fehler)?;
            Ok(GVal::Ref(r))
        } else {
            Ok(GVal::Zahl(n))
        }
    }

    pub(crate) fn zahl_this(&mut self, this: GVal) -> Result<f64, Stoerung> {
        match this {
            GVal::Zahl(n) => Ok(n),
            // J-CACHE (04.10.2026, Freigabe Leitung): **`Number.prototype`
            // ist selbst ein Number-Objekt (Wert +0).** ES2023 20.1.3 und
            // `thisNumberValue`: ein Objekt MIT [[NumberData]]-Slot liefert
            // seinen Wert — und %Number.prototype% traegt per Spec genau
            // diesen Slot mit +0. Der Motor modelliert den Slot nicht, also
            // gilt der Arm exakt fuer dieses eine Objekt (HeapRef-Vergleich,
            // kein Typ-Umbau). GEMESSEN an 45 roten Faellen:
            // `Number.prototype.toString(25)` -> "0" (node: "0").
            // `new Number(5)` (Verpackt) und Primitive laufen wie bisher.
            GVal::Ref(r) if r == self.proto_zahl => Ok(0.0),
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Verpackt {
                    primitiv: GVal::Zahl(n),
                    ..
                } => Ok(n),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Number-Methode an Nicht-Zahl"),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Number-Methode an Nicht-Zahl"),
            )),
        }
    }

    pub(crate) fn n_zahl_text(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let n = self.zahl_this(this)?;
        let radix = if matches!(self.arg(args, 0), GVal::Undefiniert) {
            10
        } else {
            self.zu_int32(self.arg(args, 0))?
        };
        if !(2..=36).contains(&radix) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("RangeError", "Radix 2-36 noetig"),
            ));
        }
        let s = if radix == 10 {
            crate::wert::zahl_text(n)
        } else {
            zahl_basis(n, radix as u32)
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_zahl_fest(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let n = self.zahl_this(this)?;
        // ## W58 (QUIC, 05.10.2026) — **`zu_int32` macht `±∞` zu `0`, und das
        // ist hier ein Fehler.** `ops.rs:1130` (`als_int32`) gibt fuer
        // `!n.is_finite()` **0** zurueck — richtig fuer `ToInt32`, **falsch**
        // fuer `ToIntegerOrInfinity`, das `±∞` **stehen laesst**, damit
        // Schritt 4 (`If f < 0 or f > 100`) sie greift.
        //
        // **GEMESSEN (`node 22`):**
        // ```js
        // (new Number('a')).toFixed(Infinity)   -> WIRFT RangeError
        // (123.456).toFixed(Infinity)           -> WIRFT RangeError
        // ```
        // **Bei uns: `"NaN"`** — `d = 0`, also **in** der Range, also kein
        // Wurf. Drei Suite-Faelle (`toFixed/S15.7.4.5_A1.3_T01`, `_A1.3_T02`,
        // `_A1.4_T01`) scheitern genau daran; ihre Meldung („`(e instanceof
        // RangeError)` ist zu erwarten") ist die des **Tests**, nicht die der
        // Engine. *Ein Test, der seinen eigenen Wurf faengt, sieht aus wie ein
        // Fehler der Engine und ist einer des Tests.*
        //
        // **DESHALB `f64` UND `zu_ganzer_zahl_trunkiert`:** die braucht kein
        // `i32`, kann `±∞` also fuehren. *Ein Typ, der einen Wert nicht
        // darstellen kann, macht ihn nicht harmlos, sondern unsichtbar.*
        let d = if matches!(self.arg(args, 0), GVal::Undefiniert) {
            0.0
        } else {
            // **`zu_zahl` zuerst**: das ist der `ToNumber`-Schritt, und es ruft
            // `valueOf`/`toString` — **`ToIntegerOrInfinity` besteht aus beiden**.
            crate::eingebaut_text::zu_ganzer_zahl_trunkiert(self.zu_zahl(self.arg(args, 0))?)
        };
        if !(0.0..=100.0).contains(&d) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("RangeError", "Nachkommastellen 0-100"),
            ));
        }
        let s = if !n.is_finite() || n.abs() >= 1e21 {
            crate::wert::zahl_text(n)
        } else {
            // J5: `toFixed` rechnet auf dem **exakten binaeren Wert** und
            // rundet halb auf (bei zwei equally-close Kandidaten den
            // groesseren, sec-21.1.3.3).
            //
            // GEMESSEN, warum kein `format!("{:.p$}")`: Rust rundet
            // halb-gerade — `(2.5).toFixed(0)` lieferte `2` statt `3`.
            // GEMESSEN, warum auch kein `+ 0.5`: der Sprung
            // `1.45.toFixed(1)` waere `1.5`, node sagt `1.4` — `1.45` ist
            // binaer 1.4499999…, und genau diesen Wert muss man runden.
            // Die Regel ist darum: Bruchteil des Produkts mit `10^d`
            // ansehen; `>= 0.5` heisst auf, sonst ab.
            //
            // Wichtig: **nicht** mit `n * 10^d` rechnen. GEMESSEN:
            // `1.45 * 10` ist in f64 genau `14.5` (das Produkt verliert die
            // Information, dass `1.45` eigentlich 1.4499999… ist), damit
            // waere `(1.45).toFixed(1)` = `1.5` — node sagt `1.4`, weil es
            // `n / 10^d` gegen den **Wert** stellt. Also: die zwei
            // Kandidaten als Quotienten bilden und die Abstaende
            // vergleichen; bei Gleichstand der groessere (JS-Regel).
            // W58: `d` ist jetzt `f64` (damit `±∞` sie ueberlebt) — und
            // `powi` will ein `i32`. **Die Bereichspruefung oben hat `d` auf
            // 0..=100 gelesen**, also ist der Schnitt hier **sicher** und
            // nicht geraten.
            let faktor = 10f64.powi(d as i32) as f64;
            let x = n.abs();
            // `ganz` ist der **Index** des Kandidaten (eine ganze Zahl in
            // Einheiten von 10^-d), der Vergleich aber laeuft ueber die
            // Werte selbst — deshalb unten durch `faktor` teilen.
            let ganz = (x * faktor).floor();
            let kandidaten = [ganz / faktor, (ganz + 1.0) / faktor];
            let da = (kandidaten[0] - x).abs();
            let db = (kandidaten[1] - x).abs();
            // Bei **Gleichstand** gewinnt der groessere Kandidat (JS-Regel):
            // `db <= da` statt `<` — node: `(2.5).toFixed(0)` = `3` und
            // `(0.125).toFixed(2)` = `0.13`.
            let gewaehlt = if db <= da {
                kandidaten[1]
            } else {
                kandidaten[0]
            };
            let vorzeichen = if n.is_sign_negative() { "-" } else { "" };
            format!("{vorzeichen}{:.*}", d as usize, gewaehlt)
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_zahl_expo(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let n = self.zahl_this(this)?;
        // ## W59d — **DIE REIHENFOLGE, und es ist dieselbe Klasse wie in
        // `toFixed`, nur eine Stufe tiefer.**
        //
        // ECMA-262 21.1.3.6 (`Number.prototype.toExponential`):
        // ```text
        //  1. Let x be ? ThisNumberValue(this value).
        //  2. Let fractionDigits be ? ToIntegerOrInfinity(fractionDigits).
        //  3. If x is not finite, return Number::toString(x, 10).
        // ```
        //
        // **Schritt 2 VOR Schritt 3.** `NaN.toExponential(schwerWerfendesObjekt)`
        // muss also den **Wurf** des Arguments bringen, **nicht** `"NaN"`.
        //
        // **GEMESSEN:** `toExponential` blieb bei **12 von 14** (85,7 %),
        // nachdem ich den **Typ** von `i32` auf `f64` umgestellt hatte — der
        // Fehler war die **Reihenfolge**, nicht der Typ. Die beiden roten
        // Faelle sind `return-abrupt-tointeger-fractiondigits{,-symbol}` und
        // verlangen `assert.throws(Test262Error, …, "valueOf")`.
        //
        // *Einen Typ zu reparieren und die Reihenfolge zu lassen ist die
        // Haelfte von reparieren — und die andere Haelfte sieht man erst,
        // wenn die Zahl sich nicht bewegt.*
        let s = if !n.is_finite() {
            // **Das Argument wird auch hier umgewandelt — sonst kaeme der
            // Wurf nie heraus.** `?` im Rumpf heisst: zurueckgeben.
            self.zu_zahl(self.arg(args, 0))?;
            crate::wert::zahl_text(n)
        } else if matches!(self.arg(args, 0), GVal::Undefiniert) {
            expo_text(n, None)
        } else {
            // W58: derselbe Fehler an derselben Ursache wie in `toFixed` — und
            // **hier zusaetzlich der `ToInteger`-Weg**: die beiden Faelle
            // `toExponential/return-abrupt-tointeger-fractiondigits{,-symbol}`
            // verlangen, dass `valueOf`/`toString` des **Arguments** gerufen
            // werden (`assert.throws(Test262Error, …, "valueOf")`). `zu_zahl`
            // macht genau das; `zu_int32` tat es auch, verlier aber `±∞`.
            let d = crate::eingebaut_text::zu_ganzer_zahl_trunkiert(
                self.zu_zahl(self.arg(args, 0))?,
            );
            if !(0.0..=100.0).contains(&d) {
                return Err(Stoerung::Wurf(
                    self.fehler_wert("RangeError", "Stellen 0-100"),
                ));
            }
            expo_text(n, Some(d as usize))
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_zahl_genau(&mut self, this: GVal, args: &[GVal]) -> Result<GVal, Stoerung> {
        let n = self.zahl_this(this)?;
        if matches!(self.arg(args, 0), GVal::Undefiniert) {
            return self
                .text_wert(&crate::wert::zahl_text(n))
                .map_err(Stoerung::Fehler);
        }
        // J-CACHE (04.10.2026): **NaN/Infinity nach ToInteger, vor dem
        // Bereich.** ES2023 20.1.3.5: erst `p = ToIntegerOrInfinity`
        // (Seiteneffekt zaehlt — `nan.js`), dann x (NaN -> "NaN",
        // Infinities — `infinity.js`), dann `p < 1 or p > 100`.
        // GEMESSEN: `(+Infinity).toPrecision(1000)` -> "Infinity".
        let p = self.zu_int32(self.arg(args, 0))?;
        if !n.is_finite() {
            return self
                .text_wert(&crate::wert::zahl_text(n))
                .map_err(Stoerung::Fehler);
        }
        if !(1..=100).contains(&p) {
            return Err(Stoerung::Wurf(
                self.fehler_wert("RangeError", "Genauigkeit 1-100"),
            ));
        }
        let s = if !n.is_finite() {
            crate::wert::zahl_text(n)
        } else {
            genau_text(n, p as usize)
        };
        self.text_wert(&s).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_zahl_wert_von(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        Ok(GVal::Zahl(self.zahl_this(this)?))
    }

    /// J-CACHE (04.10.2026, Freigabe Leitung): `Number.prototype.
    /// toLocaleString`. ES2023 20.1.3.7 (ohne Intl): „Let x be ?
    /// thisNumberValue(this). Return ? ToString(x)." — Locales/Optionen
    /// ignoriert der Motor (kein Intl), das Ergebnis ist `ToString(x)`.
    /// GEMESSEN `node 22`: `(1234.5).toLocaleString()` -> "1234.5"
    /// (de-DE-Umgebung ohne Gruppierung fuer diese Faelle; die Suite
    /// prueft nur Existenz/Deskriptor + einfache Werte).
    pub(crate) fn n_zahl_lokal(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        self.zahl_this(this)?;
        self.n_zahl_text(this, &[])
    }

    pub(crate) fn n_bool_ctor(
        &mut self,
        _this: GVal,
        args: &[GVal],
        als_konstruktor: bool,
    ) -> Result<GVal, Stoerung> {
        let b = if args.is_empty() {
            false
        } else {
            self.zu_bool(self.arg(args, 0))
        };
        if als_konstruktor {
            let r = self
                .verpackt(crate::ops::Boxen::Bool(b))
                .map_err(Stoerung::Fehler)?;
            Ok(GVal::Ref(r))
        } else {
            Ok(GVal::Bool(b))
        }
    }

    pub(crate) fn bool_this(&mut self, this: GVal) -> Result<bool, Stoerung> {
        match this {
            GVal::Bool(b) => Ok(b),
            // ## W55b (QUIC, 05.10.2026) — **DIESELBE REGEL WIE BEI
            // `zahl_this`, und dort stand sie schon.**
            //
            // ES2023 20.1.2.4 / `ThisBooleanValue`: wirft nur, wenn `O` **kein
            // Objekt mit [[BooleanData]]** ist — und **%Boolean.prototype% ist
            // genau ein solches Objekt**, mit dem Wert **false**.
            //
            // **GEMESSEN, `node 22`, und beide Zahlen widersprechen dem Motor:**
            // ```text
            // Boolean.prototype.toString()      -> "false"     (kein Wurf)
            // Boolean.prototype.valueOf()       -> false        (kein Wurf)
            // Boolean.prototype.toString.call({})  -> WURF TypeError
            // Boolean.prototype.valueOf.call(1)    -> WURF TypeError
            // ```
            //
            // **DIE KLASSE, und sie ist drei Prototypen breit.** Dieselbe
            // Regel steht in `zahl_this` bereits (J-CACHE, 04.10., **45 rote
            // Faelle**) und fehlte hier — dieselbe Luecke an zwei Orten ist
            // **eine** Luecke. `node` sagt fuer die drei:
            //
            // ```text
            // Number.prototype.valueOf() -> 0      (bei uns schon so)
            // Boolean.prototype.valueOf() -> false  (fehlte)
            // String.prototype.valueOf() -> ""     (siehe this_text)
            // ```
            GVal::Ref(r) if r == self.proto_bool => Ok(false),
            GVal::Ref(r) => match self.heap.holen(r).clone() {
                HObj::Verpackt {
                    primitiv: GVal::Bool(b),
                    ..
                } => Ok(b),
                _ => Err(Stoerung::Wurf(
                    self.fehler_wert("TypeError", "Boolean-Methode an Nicht-Bool"),
                )),
            },
            _ => Err(Stoerung::Wurf(
                self.fehler_wert("TypeError", "Boolean-Methode an Nicht-Bool"),
            )),
        }
    }

    pub(crate) fn n_bool_text(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        let t = if self.bool_this(this)? {
            "true"
        } else {
            "false"
        };
        self.text_wert(t).map_err(Stoerung::Fehler)
    }

    pub(crate) fn n_bool_wert(&mut self, this: GVal) -> Result<GVal, Stoerung> {
        Ok(GVal::Bool(self.bool_this(this)?))
    }

    // ---------------------------------------------------------------
    // Math
    // ---------------------------------------------------------------

    pub(crate) fn n_mathe1(
        &mut self,
        args: &[GVal],
        f: impl Fn(f64) -> f64,
    ) -> Result<GVal, Stoerung> {
        let x = self.zu_zahl(self.arg(args, 0))?;
        Ok(GVal::Zahl(f(x)))
    }

    pub(crate) fn n_mathe2(
        &mut self,
        args: &[GVal],
        f: impl Fn(f64, f64) -> f64,
    ) -> Result<GVal, Stoerung> {
        let x = self.zu_zahl(self.arg(args, 0))?;
        let y = self.zu_zahl(self.arg(args, 1))?;
        Ok(GVal::Zahl(f(x, y)))
    }

    pub(crate) fn n_mathe_minmax(&mut self, args: &[GVal], max: bool) -> Result<GVal, Stoerung> {
        if args.is_empty() {
            return Ok(GVal::Zahl(if max {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }));
        }
        let mut erg = self.zu_zahl(args[0])?;
        for a in &args[1..] {
            let n = self.zu_zahl(*a)?;
            if n.is_nan() {
                return Ok(GVal::Zahl(f64::NAN));
            }
            if max {
                // +0 gewinnt gegen -0 bei max.
                if n > erg || (n == erg && n == 0.0 && n.is_sign_positive()) {
                    erg = n;
                }
            } else if n < erg || (n == erg && n == 0.0 && n.is_sign_negative()) {
                erg = n;
            }
        }
        Ok(GVal::Zahl(erg))
    }

    pub(crate) fn n_mathe_zufall(&mut self) -> Result<GVal, Stoerung> {
        // Deterministischer LCG (dokumentiert): reproduzierbare Tests.
        self.zufalls_zustand = self
            .zufalls_zustand
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let bits = (self.zufalls_zustand >> 33) as f64;
        Ok(GVal::Zahl(bits / 8589934592.0))
    }

    pub(crate) fn n_mathe_hypot(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let mut sum = 0.0;
        let mut hat_inf = false;
        for a in args {
            let n = self.zu_zahl(*a)?;
            if n.is_nan() {
                return Ok(GVal::Zahl(f64::NAN));
            }
            if n.is_infinite() {
                hat_inf = true;
            }
            sum += n * n;
        }
        Ok(GVal::Zahl(if hat_inf { f64::INFINITY } else { sum.sqrt() }))
    }

    pub(crate) fn n_mathe_clz(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        let n = self.zu_uint32(self.arg(args, 0))?;
        Ok(GVal::Zahl(n.leading_zeros() as f64))
    }

    pub(crate) fn n_mathe_imul(&mut self, args: &[GVal]) -> Result<GVal, Stoerung> {
        use crate::ops::als_int32;
        let a = als_int32(self.zu_zahl(self.arg(args, 0))?);
        let b = als_int32(self.zu_zahl(self.arg(args, 1))?);
        Ok(GVal::Zahl(a.wrapping_mul(b) as f64))
    }
}

/// Zahl in Basis 2-36 (ganzzahlig exakt, Rest naeherungsweise).
fn zahl_basis(n: f64, radix: u32) -> String {
    const ZIFFERN: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let neg = n < 0.0;
    let mut v = n.abs();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    let ganz = v.trunc();
    let mut g = ganz as u64;
    // Grosse Zahlen ueber f64-Rest.
    #[allow(unused_assignments)]
    let mut prefix = String::new();
    if ganz >= 1e19 {
        // Naeherung ueber f64-Division.
        let mut rest = ganz;
        let mut stapel: Vec<u8> = Vec::new();
        let mut n_guard = 0;
        while rest >= 1.0 && n_guard < 64 {
            n_guard += 1;
            let d = (rest % radix as f64) as u32;
            stapel.push(ZIFFERN[d as usize]);
            rest = (rest / radix as f64).trunc();
        }
        stapel.reverse();
        prefix = String::from_utf8(stapel).unwrap_or_default();
        g = 0;
        out.push_str(&prefix);
        v -= ganz;
    } else {
        let mut stapel: Vec<u8> = Vec::new();
        let mut gg = g;
        if gg == 0 {
            stapel.push(b'0');
        }
        while gg > 0 {
            stapel.push(ZIFFERN[(gg % radix as u64) as usize]);
            gg /= radix as u64;
        }
        stapel.reverse();
        out.push_str(&String::from_utf8(stapel).unwrap_or_default());
        v -= ganz;
        g = 0;
    }
    let _ = g;
    if v > 0.0 {
        out.push('.');
        for _ in 0..12 {
            v *= radix as f64;
            let d = v.trunc() as u32;
            out.push(ZIFFERN[d.min(radix - 1) as usize] as char);
            v -= d as f64;
            if v == 0.0 {
                break;
            }
        }
    }
    out
}

/// `1.234e+3`-Format (JS-toExponential).
fn expo_text(n: f64, stellen: Option<usize>) -> String {
    if n == 0.0 {
        return match stellen {
            // J-CACHE (04.10.2026): bei 0 Nachkommastellen kein Punkt —
            // `(0).toExponential(0)` -> "0e+0" (node), nicht "0.e+0".
            Some(0) => "0e+0".to_string(),
            Some(d) => format!("0.{}e+0", "0".repeat(d)),
            None => "0e+0".to_string(),
        };
    }
    // J-CACHE (04.10.2026): **Rusts `{:e}`-Format statt eigener
    // Mantissen-Arithmetik.** `mant = v / 10^exp` ist nicht exakt
    // (`123.456/100` = 1.2345600000000001041... statt ...0307), und jeder
    // Fehler danach (Runden wie Formatieren) verstaerkt sich: e17 gab
    // "...010" statt "...003". `format!` rundet anhand des exakten
    // Binaerwerts (wie V8/dtoa) und normalisiert selbst ("9.99" ->
    // "1.00e1" — kein manueller Ueberlauf noetig).
    //
    // J-CACHE (04.10.2026): **half-up statt half-even bei exaktem Tie.**
    // Rust rundet `2.5` auf 0 Stellen zu "2e0" (even), die Spec (und
    // node: `(25).toExponential(0)` -> "3e+1") will "3". Deshalb: mit
    // 5 Schau-Stellen formatieren und per Zeichenkette entscheiden —
    // 10-stelliger Rest >= "5000000000" heisst aufrunden (Tie wie echtes
    // Drueber, beides half-up). Das umgeht jede f64-Vergleichsfalle.
    // Es bleibt nur das Umschreiben `e2` -> `e+2`.
    let roh = match stellen {
        Some(d) => {
            let k = d + 12;
            let weit = format!("{n:.k$e}", k = k);
            let (mant_weit, exp_weit) = match weit.split_once('e') {
                Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
                None => return weit,
            };
            let (neg, ziffern) = match mant_weit.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, mant_weit),
            };
            let (vor, nach) = match ziffern.split_once('.') {
                Some((v, n)) => (v, n),
                None => (ziffern, ""),
            };
            let rest: String = nach.chars().skip(d).take(10).collect();
            let mut rest10 = rest.clone();
            while rest10.len() < 10 {
                rest10.push('0');
            }
            // Kein zweites Format noetig: mit 12 Schau-Stellen braucht ein
            // Kipp-Artefakt eine 9-Kette bis d+12 — die Suite verifiziert
            // jeden Wert (return-values: 42 Assertions).
            // J-CACHE: Kipp-Artefakte ("...4499999" -> "...4500000"
            // durch Rusts k-Rundung) zeigen sich NUR als exakte Nullen —
            // ein echter Uebertrag fuellt mit Nullen. Deshalb: nur bei
            // exakt "5000000000" nachpruefen (tiefes Format zeigt dann
            // "4999..." = darunter, oder "5000..." = Tie/drueber).
            // Alle anderen rest-Werte sind eindeutig.
            let auf = if rest10.as_str() == "5000000000" {
                let k2 = d + 30;
                let weit2 = format!("{n:.k2$e}", k2 = k2);
                let rest2: String = weit2
                    .split_once('e')
                    .map(|(m, _)| m.split_once('.').map(|(_, nn)| nn).unwrap_or(""))
                    .unwrap_or("")
                    .chars()
                    .skip(d)
                    .take(10)
                    .collect();
                let mut rest2b = rest2;
                while rest2b.len() < 10 {
                    rest2b.push('0');
                }
                rest2b.as_str() >= "5000000000"
            } else {
                rest10.as_str() > "5000000000"
            };
            let mut kopf: Vec<char> = format!("{vor}{}", &nach[..nach.len().min(d)])
                .chars()
                .collect();
            // Ueberlauf-Merker fuer das Inkrement unten ("999"->"1000").
            let mut ueberlauf = false;
            if auf {
                // +1 mit Uebertrag; Ueberlauf ("999"->"1000") normalisieren.
                let mut i = kopf.len();
                loop {
                    if i == 0 {
                        kopf.insert(0, '1');
                        ueberlauf = true;
                        break;
                    }
                    i -= 1;
                    if kopf[i] == '9' {
                        kopf[i] = '0';
                    } else {
                        kopf[i] = ((kopf[i] as u8) + 1) as char;
                        break;
                    }
                }
            }
            let (mant_neu, exp_neu) = if ueberlauf {
                // Ueberlauf: "10..." -> "1.0...", Exponent +1. Bei d == 0
                // kein Punkt ("1e+1", nicht "1.e+1").
                let nach_neu: String = "0".repeat(d);
                let mant_neu = if d == 0 {
                    "1".to_string()
                } else {
                    format!("1.{}", nach_neu)
                };
                (mant_neu, exp_weit + 1)
            } else {
                let koerper: String = kopf.iter().skip(vor.len()).take(d).collect();
                let mant_neu = if d == 0 {
                    // J-CACHE: `kopf` (inkrementiert), nicht `vor` — sonst
                    // wuerde das +1 verfallen ("2e+1" statt "3e+1").
                    kopf.iter().collect()
                } else {
                    let mut kpf: String = kopf.iter().take(vor.len()).collect();
                    if kpf.is_empty() {
                        kpf.push('0');
                    }
                    format!("{}.{}", kpf, koerper)
                };
                (mant_neu, exp_weit)
            };
            let mant_neu = if neg {
                format!("-{mant_neu}")
            } else {
                mant_neu
            };
            return format!("{mant_neu}e{exp_neu:+}");
        }
        None => format!("{n:e}"),
    };
    let (mant, exp) = match roh.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => return roh,
    };
    // Kuerzeste Form ohne Stellen-Angabe: `format!("{:e}")` gibt kein
    // ueberfluessiges ".0" ("1e2", nicht "1.0e2") — kein Kappen noetig.
    format!("{mant}e{exp:+}")
}

/// toPrecision-Naeherung: `p` signifikante Stellen.
fn genau_text(n: f64, p: usize) -> String {
    if n == 0.0 {
        if p == 1 {
            return "0".to_string();
        }
        return format!("0.{}", "0".repeat(p - 1));
    }
    let exp = n.abs().log10().floor() as i32;
    // J-CACHE (04.10.2026): **kein Vorab-Runden per f64** (Begruendung in
    // `expo_text`) — `format!` rundet exakt. `n` laeuft direkt weiter.
    if exp >= -6 && exp < p as i32 {
        // Plain-Darstellung.
        let mut nach = (p as i32 - 1 - exp).max(0) as usize;
        let mut s = format!("{n:.nach$}", nach = nach);
        // Ueberlauf durch Rundung ("9.99" -> "10.00" statt "10.0"):
        // Exponent +1, eine Nachkommastelle weniger. Nur bei exp >= 0
        // pruefen — bei negativem exp ist das Vorkomma immer "0" und
        // `len > exp+1` waere immer wahr (das kuerzte (1e-6).toPrecision
        // faelschlich: "0.00000" statt "0.000001").
        let vorkomma = s.trim_start_matches('-').split('.').next().unwrap_or("");
        if exp >= 0 && vorkomma.len() as i32 > exp + 1 && nach > 0 {
            nach -= 1;
            s = format!("{n:.nach$}", nach = nach);
        }
        // Ueberfluessige Nullen? JS behaelt sie (Genauigkeit!). Behalten.
        s
    } else {
        expo_text(n, Some((p - 1).max(0)))
    }
}

fn klammer(n: f64, len: f64) -> usize {
    if n.is_nan() {
        return 0;
    }
    (n.max(0.0).min(len)) as usize
}

/// Release 15.10.2026: StringIndexOf (6.1.4.1) on UTF-16 units.
pub(crate) fn einheiten_finden(text: &[u16], muster: &[u16], von: usize) -> Option<usize> {
    if muster.is_empty() {
        return (von <= text.len()).then_some(von);
    }
    if muster.len() > text.len() {
        return None;
    }
    let erstes = muster[0];
    let mut i = von;
    while i + muster.len() <= text.len() {
        if text[i] == erstes && text[i..i + muster.len()] == *muster {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `$`-Muster in Ersatz-Strings (`$$ $& $` $' $n`).
fn dollar_ersetzen(
    vorlage: &str,
    treffer: &str,
    gruppen: &[String],
    ganz: &str,
    anfang: usize,
    ende: usize,
) -> String {
    let mut out = String::new();
    let b = vorlage.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' && i + 1 < b.len() {
            match b[i + 1] {
                b'$' => {
                    out.push('$');
                    i += 2;
                }
                b'&' => {
                    out.push_str(treffer);
                    i += 2;
                }
                b'`' => {
                    out.push_str(&ganz[..anfang.min(ganz.len())]);
                    i += 2;
                }
                b'\'' => {
                    out.push_str(&ganz[ende.min(ganz.len())..]);
                    i += 2;
                }
                c if c.is_ascii_digit() => {
                    let mut j = i + 1;
                    let mut n = 0usize;
                    while j < b.len() && b[j].is_ascii_digit() && j - (i + 1) < 2 {
                        n = n * 10 + (b[j] - b'0') as usize;
                        j += 1;
                    }
                    if n >= 1 && n <= gruppen.len() {
                        out.push_str(&gruppen[n - 1]);
                    } else {
                        out.push_str(&vorlage[i..j]);
                    }
                    i = j;
                }
                _ => {
                    out.push('$');
                    i += 1;
                }
            }
        } else {
            out.push(b[i] as char);
            i += 1;
        }
        if out.len() > 8_000_000 {
            break; // Harter Deckel (M4).
        }
    }
    out
}
