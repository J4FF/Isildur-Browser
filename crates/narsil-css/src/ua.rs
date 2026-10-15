//! Browser-Standardstylesheet (UA-Blatt, C5).
//!
//! Was ein Browser ohne jede Autor-Angabe darstellt: Ueberschriften,
//! Absaetze, Listen, Links, Tabellen — und was er versteckt (head,
//! script, style). In der Kaskade hat es die niedrigste Prioritaet
//! (erstes Blatt in `berechnen`).

use super::parse::stylesheet_lesen;
use super::typen::Stylesheet;

/// Das UA-Stylesheet als Quelle (zum Nachlesen und Testen).
/// Groessen in px bei 16 px Basis (h1 = 2em = 32 px ... h6 = 0.67em).
/// Raender als px-Naeherung der Browser-Voreinstellung (8/16/21 px).
pub const UA_CSS: &str = "
head, script, style, title, meta, link { display: none; }
/* Release 07.10.2026: weitere versteckte Elemente aus dem HTML-Standard
   (§15.3.1, §15.5.15/16). GEMESSEN: die Seitenleiste von
   developer.mozilla.org besteht aus 11 `<details>` und stand komplett
   aufgeklappt da; ein geschlossener `<dialog>` stand mitten im Text. */
template, datalist, noembed, noframes, param, rp, area, base { display: none; }
/* Release 12.10.2026: HTML §15.3.1 — ein Slot hat keinen eigenen Kasten. */
slot { display: contents; }
dialog:not([open]) { display: none; }
/* Release 15.10.2026: HTML §15.3.1 — `[hidden]` is a UA rule, so author CSS
   can show the element again (`[hidden] { display: block }`), and its
   scripts still run like any `display: none` subtree. MEASURED before: the
   page builder dropped `hidden` elements with all their children, including
   `<script>` (found by Agent F), and author `display` was ignored. */
[hidden]:not([hidden=until-found i]) { display: none; }
[hidden=until-found i] { display: none; }
/* Release 11.10.2026: HTML §15.3.4 (Popover) und §15.3.11 (modaler Dialog).
   Ein Popover ist ohne `showPopover()` unsichtbar; offen liegt es mittig im
   Fenster. */
dialog { display: block; position: absolute; inset-inline-start: 0; inset-inline-end: 0; width: fit-content; height: fit-content; margin: auto; border: solid; padding: 1em; background-color: white; color: black; }
[popover]:not(:popover-open):not(dialog[open]) { display: none; }
[popover] { position: fixed; inset: 0; width: fit-content; height: fit-content; margin: auto; border: solid; padding: 0.25em; overflow: auto; color: CanvasText; background-color: Canvas; }
dialog:modal { position: fixed; inset-block: 0; max-width: calc(100vw - 6px - 2em); max-height: calc(100vh - 6px - 2em); overflow: auto; visibility: visible; }
details:not([open]) > :not(summary:first-of-type) { display: none; }
/* GEMESSENER FEHLER (03.10.2026, Rolle CSS; BRETT 19:05 und 19:49 von DEVPANEL).
   Fuer `html` und `div` stand hier **keine** Regel, und CSS Display 3 §2.1
   nennt `inline` als Anfangswert von `display` — den ein Browser erst durch
   sein UA-Blatt auf `block` hebt. Ohne die Regel blieb der Anfangswert stehen.

   GEMESSEN, UA-Blatt allein, keine Autorregel (Brett-Meldung DEVPANEL,
   gegengeprueft in `tests/ua_tabelle.rs::div_und_html_ohne_regel_waeren_inline`):

     <body> -> Block    (hat eine UA-Regel)           richtig
     <span> -> Inline   (keine Regel, aber RICHTIG so) richtig
     <div>  -> Inline   (KEINE Regel, obwohl Firefox block sagt) FALSCH

   `span` bleibt `inline` — das ist der einzige Fall, in dem der Anfangswert
   der gewollte ist, und deshalb steht es im Test ausdruecklich daneben.
   Siehe auch den DevTools-Panel-Befund: dort zeigte `<html>` `Inline`. */
html, div { display: block; }
body { display: block; margin: 8px; }
/* Release 12.10.2026 (Agent CSS2): Schriftgroessen und Raender in `em` wie im
   HTML-Standard (§15.3.3, §15.3.6/7) — sie folgen der geerbten Schrift.
   GEMESSEN vorher (WPT `css1/c5502-imrgn-r-*`, `c5504-imrgn-l-*`): ein `<p>`
   in `div { font: 15px Ahem }` hatte 16 statt 15 px Rand. Bei 16 px Basis
   aendert sich nur der Bruchteil der Ueberschriften-Raender (21 -> 21,44). */
h1 { display: block; font-size: 2em; font-weight: bold; margin: 0.67em 0; }
h2 { display: block; font-size: 1.5em; font-weight: bold; margin: 0.83em 0; }
h3 { display: block; font-size: 1.17em; font-weight: bold; margin: 1em 0; }
h4 { display: block; font-size: 1em; font-weight: bold; margin: 1.33em 0; }
h5 { display: block; font-size: 0.83em; font-weight: bold; margin: 1.67em 0; }
h6 { display: block; font-size: 0.67em; font-weight: bold; margin: 2.33em 0; }
p { display: block; margin: 1em 0; }
ul, ol { display: block; margin: 1em 0; padding-left: 40px; }
li { display: list-item; }
/* Release 07.10.2026: die Listenmarken aus dem HTML-Standard (§15.3.8).
   GEMESSEN vorher: `<ol>` zeigte einen Stern statt 1. (Wikipedia-Einzelnachweise). */
ol { list-style-type: decimal; }
ul ul, ol ul { list-style-type: circle; }
ul ul ul, ul ol ul, ol ul ul, ol ol ul { list-style-type: square; }
ul ul, ul ol, ol ul, ol ol { margin-top: 0; margin-bottom: 0; }
a { color: #0000ee; text-decoration: underline; }
blockquote { display: block; margin: 1em 40px; }
pre { display: block; font-family: monospace; white-space: pre; margin: 1em 0; }
pre[dir=auto], textarea[dir=auto] { unicode-bidi: plaintext; }
/* GEMESSENER FEHLER (03.10.2026, Rolle CSS): hier stand
   `table { display: block; }` und `tr, td, th { display: block; }`.
   Das war **durchaus falsch und actively schaedlich**: die Kaskade liest
   `display: table` / `table-cell` / `table-row` einwandfrei und liefert
   `Anzeige::Tabelle` (GEMESSEN, alle sieben Tabellenschluessel, 0 verworfen:
   table, table-cell, table-row, table-row-group, table-header-group,
   inline-table, table-footer-group). Das UA-Blatt hat diese richtige
   Antwort mit `block` **ueberschrieben** — jede Tabelle jeder Seite kam als
   Block an.

   Zwei Befunde aus `team/VERGLEICH_FIREFOX.txt` sind genau das:
   docs.rs: die Release-Tabelle ist keine Tabelle, jede Zeile steht als drei
   untereinander liegende Zeilen (Name, Beschreibung, Alter). Und
   de.wikipedia.org: die Infobox fehlt, kein Kasten Rust mit Logo, keine
   Basisdaten-Tabelle.

   `Anzeige` hat (bewusst) KEINE eigene Variante fuer Zeile/Zelle/
   Zeilengruppe: CSS Tables 3 §2.1 liefert sie, die Kaskade legt sie alle auf
   `Tabelle` (siehe `typen.rs::Anzeige`). Wer sie trennen will, braucht eine
   neue Variante plus den Tabellen-Leg in `narsil-layout-engine/src/tabelle.rs`
   — das ist eine eigene Baustelle, BRETT an CODEX/SERVER. */
table { display: table; }
tr { display: table-row; }
td, th { display: table-cell; }
thead, tbody, tfoot { display: table-row-group; }
/* Release 11.10.2026 (Agent CSS2): HTML 15.3.8 — Kopf- und Fussgruppe,
   Beschriftung, Spalten und box-sizing der Tabelle. (vertical-align und
   padding der Zellen stehen in der Kaskade bei den Praesentations-
   attributen, damit valign/cellpadding ueber ihnen stehen.) GEMESSEN
   vorher (WPT css/CSS2/tables): thead/tfoot waren gewoehnliche
   Rumpfgruppen, caption ein Block im Raster. */
table { box-sizing: border-box; }
thead { display: table-header-group; }
tfoot { display: table-footer-group; }
caption { display: table-caption; text-align: center; }
colgroup { display: table-column-group; }
col { display: table-column; }
b, strong { font-weight: bold; }
i, em, cite { font-style: italic; }
u, ins { text-decoration: underline; }
s, del { text-decoration: line-through; }

/* SCHRIFT, 04.10.2026 (freigegeben von der Leitung): `th`, `small`,
   `sub`/`sup`, `code`/`pre`/`kbd`/`samp`/`tt`.

   GEMESSEN, was vorher fehlte, an `texttest.html` gegen Firefox (eigenes
   Profil `/home/jaff/.narsil_schrift/profil`):
     * `th` war **nicht fett** und **nicht zentriert**. Firefox
       (HTML Rendering: `th { font-weight: bold; text-align: center }`).
     * `small` war 16 px wie jeder andere Absatz. Firefox: `smaller`,
       GEMESSEN 13,33 px bei einem 16-px-Vater = 0,8333 em.
     * `sub`/`sup` waren normale Groessen auf der Grundlinie. Firefox:
       `vertical-align: sub|sup` und `font-size: smaller`.
     * `code`/`pre`/`kbd` waren in der Serifen-Schrift des Fliesstexts.
       Firefox: `monospace` (DejaVuSansMono auf dieser Maschine).

   GEMESSEN, warum die Groessen als **em** und nicht als Pixel stehen:
   `small` ist `smaller` in CSS 2.1 §16.2 — dort steht woertlich
   `one font size smaller`, also relativ. `0,8333 em` ist die Groesse,
   die Firefox GEMESSEN bei einem 16-px-Vater zeigt (13,33 px). In Pixeln
   waere es bei einem 12-px-Vater wieder 16 px gross.
   GEMESSEN (SCHRIFT 04.10.2026, 04:1x), warum hier keine Anfuehrungs-
   zeichen im Text stehen: dieses Blatt ist ein Raw-String mit Anfuehrungs-
   zeichen als Delimiter, und ein einzelnes Anfuehrungszeichen darin
   zerlegt ihn — der Build bricht dann mit `unknown start of token` ab
   (CSS_6, genau dieser Fehler). Deutschsprachige Anfuehrungszeichen
   (`«»`) waeren die Alternative, aber ein Zitat ohne Anfuehrungszeichen
   ist hier eindeutiger.

   GEMESSEN, warum `sup`/`sub` **nur** die Groesse bekommen und nicht die
   `vertical-align`: die Zeilenhoehe des Fliesstextes wird im Layout
   entschieden (`narsil_layout::Messung::aufstieg`), und eine
   `vertical-align`-Anhebung aendert die Grundlinie, nicht die
   Schriftgroesse. Das ist der naechste Schritt und steht im Bericht
   SCHRIFT_5 als offen.

   GEMESSEN, warum `h1..h6` hier **nicht** angefasst werden: sie stehen
   schon ab Zeile 33 mit Groesse und Rand. */
th { font-weight: bold; text-align: center; }
small { font-size: 0.8333em; }
sub { font-size: 0.8333em; }
sup { font-size: 0.8333em; }
code, pre, kbd, samp, tt { font-family: monospace; }
/* GEMESSENE LUECKE (03.10.2026, Rolle CSS): 18 HTML5-Elemente hatten **keine**
   `display`-Regel und bekamen damit den Anfangswert `inline`
   (CSS Display 3 §2.1) — also Einzeilentext, wo Firefox einen Block
   erzeugt. Das ist dieselbe Fehlerklasse wie der `html`/`div`-Befund von
   19:05/19:49 (BRETT), nur an mehr Stellen.

   GEMESSEN, wie viele Knoten das auf den zehn Referenzseiten trifft
   (`team/log/codex8/vor/html/*.html`, ausgezaehlt mit grep, HTML5-Blockliste
   gegen die Selektoren dieses Blattes):

     option 125, section 59, center 33, dd 28, nav 27, dt 27, dl 22,
     header 16, article 14, form 11, hr 9, footer 7, main 6, figure 2,
     details 2, figcaption 1, summary 1, fieldset 1     = 391 Knoten

   de.wikipedia: 45 Knoten (section 25, nav 12, header 2, form 2, main 1,
   dl 1). MDN 75, wikipedia.org 86, github 52, debian 40, HN 34.

   WICHTIG: das ist **kein Rueckschritt**, sondern ein Defekt, der seit je
   bestand. Ein Element, das immer `inline` war, kann keinen Sprung
   verursacht haben — es repariert nur, was vorher falsch war.

   `center` ist nicht in HTML5, Firefox haelt es aber als `display: block`
   (HTML Rendering: `center { display: block }`). `hr` ist im Browser
   zusaetzlich `border-style: inset`; die Breite gehoert nicht hierher,
   sondern in den Rahmen-Leg.

   `dt`/`dd` haben im Browser `margin: 1em 40px` neben `display: block` —
   hier steht nur das `display`, weil die Raender ungemessen bleiben.

   # WARUM DIE LISTE GETEILT IST (03.10.2026, GEMESSEN)

   `MAX_SELEKTOREN_JE_REGEL` ist 16 (`lib.rs:252`), und `kaskade.rs:543`
   schneidet mit `.take(MAX_SELEKTOREN_JE_REGEL)` **stillschweigend** ab.
   GEMESSEN mit einer Wegwerf-Messung in `tests/`:

     `..., dd, center { }`              (16 Selektoren) -> center = Block
     `..., dd, center, option { }`      (17 Selektoren) -> option = **Inline**
     18 Selektoren, einzeilig           -> option = **Inline**, hr = **Inline**
     18 Selektoren, **zweizeilig**      -> option = **Inline**, hr = **Inline**

   Der 17. und der 18. Selektor gehen also **ohne jede Fehlermeldung**
   verloren, und ein Zeilenumbruch in der Liste aendert daran nichts. Die
   erste Fassung stand mit 18 Selektoren in einer Regel da und liess
   `option` (125 Knoten!) und `hr` (9 Knoten) unveraendert auf `inline` —
   GEMESSEN, der Test `ua_blockluecken` ist genau daran rot geworden.

   Die Grenze selbst ist **richtig**: sie deckt die Verschachtelung
   (`eltern_teile * kind_teile`, `parse.rs:1863`) und verhindert dort eine
   kombinatorische Explosion. Eine flache Komma-Liste hat dieses Risiko
   nicht, wird aber von derselben Zahl gekappt. Das Blatt umzubauen ist
   Sache der Leitung (GEMESSENE Meldung im BRETT); fuer diesen Fix reicht
   das Teilen, und deshalb sind es zwei Regeln. */
section, nav, header, article, main, footer, figure, figcaption, details,
summary, form, fieldset, dl, dt, dd, center { display: block; }
option, hr { display: block; }
/* Release 08.10.2026: HTML Rendering §15.3.11 — die Trennlinie ist ein
   leerer Kasten mit einem 1-px-Rahmen, kein Text. GEMESSEN vorher: jedes
   `<hr>` stand als Zeile `────────────────` im Bild, auch wenn die Seite es
   gestaltet oder versteckt (t-online.de: acht Strichzeilen am linken Rand). */
hr { border-style: inset; border-width: 1px; border-color: #a0a0a0; margin: 0.5em 0; height: 0; overflow: hidden; }
/* Release 11.10.2026: HTML Rendering §15.4.4 / UA-Blaetter von Chrome und
   Firefox — der Rahmen eines `iframe`. */
iframe { border: 2px inset; }

/* ===================================================================
   BILD_37: FORMULARFELDER. GEMESSEN an Firefox 157 am 04.10.2026.
   Anlage `team/testseiten/ua_formular.html` (ohne jede eigene
   Formatiierung an den Feldern — es soll der UA-Stil sein, der spricht),
   Referenz `team/testseiten/ua_ff/ff.png`, Werte in `messung.txt`.
   ===================================================================

   **WAS VORHER STAND: NICHTS.** GEMESSEN: `grep` nach `input`,
   `select`, `textarea`, `button` in dieser Datei ergab **null** Treffer —
   unsere Formularfelder hatten **0 px** Rahmen, Firefox **1 px**.

   | Feld            | GEMESSEN                          |
   |-----------------|-----------------------------------|
   | `input`         | 1 px `#8f8f9d`, einfarbig         |
   | `textarea`      | 1 px `#8f8f9d` (identisch)         |
   | `input:search`  | 1 px `#8f8f9d` (identisch)         |
   | `input:disabled`| 1 px `#c7c7ce` — **heller**       |
   | `::placeholder` | `#757575`                         |
   | Eckenradius     | **2 px** (Oberkante beginnt 2 px nach der linken Kastenkante) |
   | Polsterung      | erste Glyphe 3 px nach der Innenkante bei 1 px Rahmen -> **2 px** |

   **DREI DINGE SIND AUSDRUECKLICH NICHT EINGETRAGEN, weil ich sie nicht
   gemessen habe oder weil sie keine `border`-Regel sind:**

   1. **`:focus` ist NICHT gemessen.** GEMESSEN: im Screenshot mit
      `autofocus` gibt es **keinen** blaustichigen Pixel — headless malt
      keinen Fokusring. **Ich trage deshalb KEINE Fokusfarbe ein.** (Die
      Leitung nannte `#0060df`; **GEMESSEN ist `#0060df` der AKKENT der
      geprueften Checkbox**, 65 von 196 Pixeln im 14x14 — das ist etwas
      anderes.)
   2. **`button` und `select` sind KEIN einfarbiger Rahmen.** GEMESSEN:
      `button` ist ein 3-D-Relief aus **sieben** Farbwerten
      (`#e9e9ed` mit `#dadade`/`#9c9c9f`/`#363637`/`#535355` und einer
      schwarzen Linie), `select` hat eine 3-D-Kante. **Das ist Fussmalerei,
      kein `border`** — eine `border`-Regel wuerde etwas erzeugen, das es
      in Firefox nicht gibt. Deshalb stehen sie hier **nicht**.
   3. **`checkbox` und `radio`** sind **gezeichnete Elemente** (GEMESSEN
      14 x 14 px, ungeprueft `#8f8f9d`, geprueft mit `#0060df` gefuellt).
      **Rahmen und Fuellung sind Zeichnung, keine Kaskade** — das gehoert
      in `narsil-raster`/`narsil-chrome` und ist mit UI abzustimmen.

   **UND `#0060df` IST DER AKKENT** (GEMESSEN an der geprueften Checkbox).
   Wer die Checkbox farblich richtig haben will, braucht **diese** Farbe —
   nicht die des Fokusrings, den es nicht gibt. */
/* 05.10.2026: Sichtbare flache Button-Raender und Platz fuer die Beschriftung.
   Das oben beschriebene Firefox-Relief bleibt ein separater Vergleichspunkt. */
button { border: 1px solid #8f8f9d; border-radius: 2px; padding: 2px 6px; }
input, textarea { border: 1px solid #8f8f9d; border-radius: 2px; padding: 1px 2px; }
/* Release 08.10.2026: die Feldflaechen (Firefox: Knopf #e9e9ed, Felder weiss)
   gehoeren ins UA-Blatt — dann kann `background: transparent` der Seite sie
   aufheben. GEMESSEN vorher: der Bau malte ohne Hintergrund immer weiss
   (t-online.de: weisse Saeule hinter dem Knopf Telekom Services). */
button { background-color: #e9e9ed; }
input, textarea, select { background-color: #ffffff; }
/* Release 09.10.2026: die Auswahl hat einen Rahmen wie die anderen Felder
   (GEMESSEN vorher: randlos, vom Fliesstext nicht zu unterscheiden). */
select { border: 1px solid #8f8f9d; border-radius: 2px; padding: 1px 4px; }
/* Release 09.10.2026: Eingabe-Knoepfe sehen aus wie `<button>`. */
input[type=submit], input[type=button], input[type=reset] { background-color: #e9e9ed; padding: 2px 6px; }
input::placeholder, textarea::placeholder { color: #757575; }
input:disabled, textarea:disabled { border-color: #c7c7ce; }
";

/// Das geparste UA-Blatt (klein, begrenzt — parsen dauert < 1 ms).
/// In `berechnen` als ERSTES Blatt einordnen (niedrigste Prioritaet).
pub fn ua_stylesheet() -> Stylesheet {
    // Release 09.10.2026: EINMAL lesen, danach klonen (jeder Seitenbau
    // fragte es neu an und las 250 Zeilen CSS jedesmal).
    static UA: std::sync::OnceLock<Stylesheet> = std::sync::OnceLock::new();
    UA.get_or_init(|| stylesheet_lesen(UA_CSS)).clone()
}
