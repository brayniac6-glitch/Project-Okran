//! The DIALOGUE_LINEs whose text has a phrase, and the records leading to them (up to the
//! DIALOGUE): `cargo run -p kenshi-data --example line_of -- <install> "<phrase>"`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let want = a[1].to_lowercase();
    for l in c.of_type(ItemType::DialogueLine) {
        if !l.strings.values().any(|t| t.to_lowercase().contains(&want)) {
            continue;
        }
        println!("line {} {:?}", l.string_id, l.strings.get("text0"));
        // walk up: whoever references it
        let mut at = l.string_id.clone();
        for _ in 0..12 {
            let Some(up) = c.records.values().find(|r| r.references.iter().any(|(_, v)| v.iter().any(|(s, _)| *s == at))) else { break };
            println!("  <- {} '{}' [{:?}]", up.string_id, up.name, up.item_type());
            if up.item_type() == Some(ItemType::Dialogue) {
                break;
            }
            at = up.string_id.clone();
        }
    }
}
