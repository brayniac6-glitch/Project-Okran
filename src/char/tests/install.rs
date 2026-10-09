//! Reads the medical constants from a real install (opt-in):
//!   KENSHI_DIR="C:/Program Files (x86)/Steam/steamapps/common/Kenshi" cargo test -p kenshi-char -- --ignored

use kenshi_char::{Constants, Medical, Stats};
use kenshi_data::GameDataContainer;
use kenshi_io::mods::Install;

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn constants_come_from_global_constants() {
    let install = Install::new(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR"));
    let mut order = install.load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let c = Constants::from_record(data.get("110-gamedata.quack").expect("GLOBAL CONSTANTS"));
    assert_eq!(
        (c.min_toughness_ko_point, c.max_toughness_ko_point),
        (10.0, 85.0)
    );
    assert_eq!(
        (c.knockout_time_base, c.knockout_mult_1, c.knockout_mult_99),
        (20.0, 3.0, 0.75)
    );
    assert_eq!(c.immediate_blood_loss, 0.2);
    // a toughness-50 character with the shipped constants
    assert_eq!(
        Medical::ko_point(
            &Stats {
                toughness: 50.0,
                toughness_scale: 1.0
            },
            &c
        ),
        -47.5
    );
}

#[test]
#[ignore = "needs KENSHI_DIR pointing at a Kenshi install"]
fn a_greenlander_body_from_race_data() {
    let install = Install::new(std::env::var("KENSHI_DIR").expect("set KENSHI_DIR"));
    let mut order = install.load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    let race = data
        .of_type(kenshi_data::ItemType::Race)
        .find(|r| r.name == "Greenlander")
        .unwrap();
    let m = Medical::from_race(race, &data, 1.0, 1.0);
    assert_eq!(m.parts.len(), 7);
    let head = m.parts.iter().find(|p| p.kind == 3).unwrap();
    assert!(head.vital && head.collapses);
    assert_eq!(
        (head.hit_weight, head.max_health(), head.ko_weight),
        (80.0, 100.0, 2.0)
    );
    // one of each limb slot: arms 0/1, legs 2/3
    let mut slots: Vec<i32> = m
        .parts
        .iter()
        .map(|p| p.limb_slot())
        .filter(|s| *s < 4)
        .collect();
    slots.sort();
    assert_eq!(slots, [0, 1, 2, 3]);
}
