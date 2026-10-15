use narsil_fmt_html::spec_tokenizer::{SpecToken, SpecTokenizer};
fn main() {
    let input = "<!DOCTYPE root-element [SYSTEM OR PUBLIC FPI] \"uri\" [ \n<!-- internal declarations -->\n]>";
    let mut tok = SpecTokenizer::neu(input);
    loop {
        let t = tok.naechstes();
        println!("{:?}", t);
        if let SpecToken::Eof = t {
            break;
        }
    }
}
