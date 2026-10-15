//! Which FACTION lists each campaign (by name) in its "campaigns", and which dialogues and
//! campaigns trigger it:
//! `cargo run -p kenshi-data --example campaign_lists -- <install> <campaign name>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for camp in c.of_type(ItemType::FactionCampaign).filter(|r| r.name == a[1]) {
        println!("campaign {} ({})", camp.name, camp.string_id);
        for f in c.of_type(ItemType::Faction) {
            if let Some((_, v)) = f.refs("campaigns").iter().find(|(s, _)| *s == camp.string_id) {
                println!("  listed by faction {} ({}) {v:?}", f.name, f.string_id);
            }
        }
        for r in c.records.values() {
            for (cat, refs) in &r.references {
                if refs.iter().any(|(s, _)| *s == camp.string_id) && cat.as_str() != "campaigns" {
                    println!("  {cat} of {} {} ({})", r.kind, r.name, r.string_id);
                }
            }
        }
    }
}
