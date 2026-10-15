//! The FACTION_CAMPAIGNs: which faction lists each, whether it is territorial, targets
//! characters, its chance, ranges, repeat limit and follow-ups:
//! `cargo run -p kenshi-data --example campaigns -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    for camp in c.of_type(ItemType::FactionCampaign) {
        let by: Vec<&str> = c.of_type(ItemType::Faction).filter(|f| f.refs("campaigns").iter().any(|(s, _)| *s == camp.string_id)).map(|f| f.name.as_str()).collect();
        let b = |k: &str| camp.bools.get(k).copied();
        let f = |k: &str| camp.floats.get(k).copied().or_else(|| camp.ints.get(k).map(|v| *v as f32));
        let refs = |k: &str| camp.refs(k).iter().map(|(s, v)| format!("{}{v:?}", c.get(s).map_or(s.as_str(), |r| r.name.as_str()))).collect::<Vec<_>>();
        println!(
            "{:40} by {by:?} terr {:?} chars {:?} chance {:?} near {:?} far {:?} repeat {:?} tech {:?} win {:?} lose {:?}",
            camp.name,
            b("territorial triggers"),
            b("target characters"),
            f("chance per day"),
            f("range near"),
            f("range far"),
            f("repeat limit"),
            f("target tech level >="),
            refs("victory trigger"),
            refs("loss trigger"),
        );
    }
}
