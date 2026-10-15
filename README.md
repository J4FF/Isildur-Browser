# Isildur Browser

Modular browser engine written in Rust: HTML/CSS, JavaScript bytecode VM (`narsil-jsvm`), DOM, layout engine, rasterizer, network, TLS, and UI chrome.

## Structure

The project consists of modular crates located under `crates/`:

- `narsil`: Main browser application and session coordinator
- `narsil-chrome`: Browser user interface (tabs, address bar, navigation, menus)
- `narsil-window`: Window management and surface handling
- `narsil-page`: Page representation, HTML/CSS binding, and styling
- `narsil-layout-engine`: Flow, flex, and table layout
- `narsil-layout`: Layout tree and box primitives
- `narsil-display`: Display list generation
- `narsil-raster`: 2D software rasterizer
- `narsil-text`: Font loading, OpenType parsing, and glyph rasterization
- `narsil-jsvm`: Custom JavaScript bytecode engine and runtime
- `narsil-dom`: Document Object Model tree implementation
- `narsil-css`: CSS tokenizer, parser, and cascade matching
- `narsil-fmt-html`: HTML tokenizer and parser
- `narsil-net`: HTTP/1.1 networking client
- `narsil-tls`: TLS 1.2/1.3 protocol and X.509 certificate handling
- `narsil-quic`: QUIC / HTTP/3 transport
- `narsil-image`: Image decoders (PNG, JPEG, BMP, VP8)
- `narsil-audio`: Audio playback and decoding
- `narsil-media`: WebM and media container parsing
- `narsil-canvas`: Canvas 2D rendering implementation
- `narsil-svg`: SVG vector rendering
- `narsil-pdf`: PDF renderer and document viewer
- `narsil-adblock`: Content blocking and rule matching
- `narsil-core`: Core geometry, color, and primitive types
- ... and additional utility crates.

## Building

To check/compile the browser:

```bash
cargo check -p narsil
```

To build the executable:

```bash
cargo build -p narsil
```