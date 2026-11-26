//! 英字入力の候補の試し（実語彙）: `cargo run --release -p nagi-english --example assist_probe -- <words.fst> <語>...`
use nagi_english::{Assistant, Fixes, Lexicon, NoLearned, SpellCosts};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("words.fst");
    let lex = Lexicon::load(std::path::Path::new(&path)).unwrap();
    let costs = SpellCosts::default();
    let a = Assistant {
        lexicon: Some(&lex),
        fixes: Fixes::builtin(),
        learned: &NoLearned,
        costs: &costs,
    };
    for w in args {
        let t = Instant::now();
        let r = a.assist(&w, 6);
        let us = t.elapsed().as_micros();
        let items: Vec<&str> = r.items.iter().map(|i| i.text.as_str()).collect();
        println!(
            "{w:>14} -> {:<14} {:>6}us {:?}",
            r.autocorrect.unwrap_or_default(),
            us,
            items
        );
    }
}
