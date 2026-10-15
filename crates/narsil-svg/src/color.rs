//! Colors in SVG: CSS named colors, hex, the functional notations (via
//! `narsil_canvas::css_color`), `currentColor` and system colors.

use narsil_canvas::Color;

/// The CSS named colors (CSS Color 4 §6.1).
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff), ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000),
    ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00), ("chocolate", 0xd2691e),
    ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9), ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9), ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f), ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000), ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff), ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700),
    ("goldenrod", 0xdaa520), ("gray", 0x808080), ("green", 0x008000), ("greenyellow", 0xadff2f),
    ("grey", 0x808080), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080), ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1), ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899), ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de), ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0x800000), ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3), ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1), ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead), ("navy", 0x000080), ("oldlace", 0xfdf5e6), ("olive", 0x808000),
    ("olivedrab", 0x6b8e23), ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9), ("peru", 0xcd853f), ("pink", 0xffc0cb),
    ("plum", 0xdda0dd), ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072), ("sandybrown", 0xf4a460), ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d), ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa), ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4), ("tan", 0xd2b48c), ("teal", 0x008080), ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347), ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3),
    ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00), ("yellowgreen", 0x9acd32),
];

fn rgb(v: u32) -> Color {
    Color::from_rgba8((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
}

/// A keyword or hex color (`current` stands for `currentColor`).
fn keyword(k: &str, current: Color) -> Option<Color> {
    let k = k.trim();
    if let Some(hex) = k.strip_prefix('#') {
        let d: Vec<u8> = hex.bytes().map(|c| (c as char).to_digit(16).map(|v| v as u8)).collect::<Option<Vec<u8>>>()?;
        let x2 = |h: u8, l: u8| h * 16 + l;
        return Some(match d.len() {
            3 => Color::from_rgba8(d[0] * 17, d[1] * 17, d[2] * 17, 255),
            4 => Color::from_rgba8(d[0] * 17, d[1] * 17, d[2] * 17, d[3] * 17),
            6 => Color::from_rgba8(x2(d[0], d[1]), x2(d[2], d[3]), x2(d[4], d[5]), 255),
            8 => Color::from_rgba8(x2(d[0], d[1]), x2(d[2], d[3]), x2(d[4], d[5]), x2(d[6], d[7])),
            _ => return None,
        });
    }
    let lower = k.to_ascii_lowercase();
    match lower.as_str() {
        "currentcolor" => return Some(current),
        "transparent" => return Some(Color::TRANSPARENT),
        _ => {}
    }
    if let Ok(i) = NAMED.binary_search_by(|(n, _)| (*n).cmp(lower.as_str())) {
        return Some(rgb(NAMED[i].1));
    }
    system_color(&lower)
}

fn system_color(s: &str) -> Option<Color> {
    let c = |v| Some(rgb(v));
    match s {
        "canvas" | "window" | "field" | "background" | "menu" => c(0xffffff),
        "canvastext" | "windowtext" | "fieldtext" | "buttontext" | "menutext" | "captiontext" | "infotext" => c(0x000000),
        "graytext" => c(0x6e6e73),
        "highlight" | "selecteditem" | "accentcolor" => c(0x0075ff),
        "highlighttext" | "selecteditemtext" | "accentcolortext" => c(0xffffff),
        "buttonface" | "buttonhighlight" | "buttonshadow" | "threedface" => c(0xefefef),
        "buttonborder" | "threeddarkshadow" | "threedshadow" | "threedhighlight" | "threedlightshadow" | "activeborder" | "inactiveborder" | "windowframe" => c(0x767676),
        "linktext" => c(0x0000ee),
        "visitedtext" => c(0x551a8b),
        "activetext" => c(0xff0000),
        "mark" => c(0xffff00),
        "marktext" => c(0x000000),
        _ => None,
    }
}

/// Parses a CSS color.
#[must_use]
pub fn parse(s: &str, current: Color) -> Option<Color> {
    narsil_canvas::css_color::parse_color(s, &|k| keyword(k, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_colors_are_sorted_and_found() {
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(parse("lime", Color::BLACK).unwrap().to_rgba8(), [0, 255, 0, 255]);
        assert_eq!(parse("#0f08", Color::BLACK).unwrap().to_rgba8(), [0, 255, 0, 136]);
        assert_eq!(parse("rgb(0 128 0 / 50%)", Color::BLACK).unwrap().to_rgba8(), [0, 128, 0, 128]);
        assert_eq!(parse("currentColor", Color::from_rgba8(1, 2, 3, 255)).unwrap().to_rgba8(), [1, 2, 3, 255]);
    }
}
