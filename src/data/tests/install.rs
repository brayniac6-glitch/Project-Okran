//! Loads and merges a real install. Needs the player's copy, so it is opt-in:
//!   KENSHI_DIR="C:/Program Files (x86)/Steam/steamapps/common/Kenshi" cargo test -p kenshi-data -- --ignored

use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::{CORE_BASE, Install};

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn merges_core_files() {
    let install = Install::new(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR"));
    let mut order = install.load_order().unwrap();
    order.files.retain(|f| f.core); // player mods vary; the core result does not
    assert_eq!(order.files[0].name, CORE_BASE);
    let c = GameDataContainer::load(&order).unwrap();
    assert!(c.records.len() > 50_000, "{} records", c.records.len());
    assert!(
        c.records
            .values()
            .all(|r| ItemType::from_i32(r.kind).is_some())
    );
    // rebirth.mod raises the base game's squad size (10 -> 20): later files override per field
    let k = c.get("110-gamedata.quack").expect("GLOBAL CONSTANTS");
    assert_eq!(k.item_type(), Some(ItemType::Constants));
    assert_eq!(k.ints["max squad size"], 20);
    assert_eq!(k.ints["max faction size"], 30);
}
