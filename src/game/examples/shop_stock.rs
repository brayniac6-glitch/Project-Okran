//! Each trader's stock in a zone (headless): items, money and vendor template, at the start and
//! after a while: `cargo run --release -p kenshi-game --example shop_stock -- <install> X Y [seconds]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn report(sim: &Sim) {
    for p in sim.folk.iter() {
        let Some(s) = p.shop.as_ref() else { continue };
        let items: usize = s.stock.sections.iter().map(|sec| sec.items.len()).sum();
        let name = sim.data.get(&s.template).map_or(s.template.clone(), |r| r.name.clone());
        println!("  {} {:?}: {items} items, money {}, template {name}, restock at {:.1} h", p.id, p.name, s.money, s.restock_at);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let seconds: f32 = a.get(3).map_or(30.0, |s| s.parse().unwrap());
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:2".to_string()]);
    // (testing) no roaming squads or new nests: a random squad can't wander into the test
    sim.no_spawns();
    println!("at load:");
    report(&sim);
    while sim.t < seconds {
        sim.step();
    }
    println!("after {seconds} s:");
    report(&sim);
}
