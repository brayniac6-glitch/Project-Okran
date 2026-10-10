//! A building record's production data: `cargo run --release -p kenshi-game --example prod_check -- <install> <name>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let sim = kenshi_game::sim::Sim::load(std::path::Path::new(&a[0]), 30, 40, 0, &[]);
    for r in sim.data.of_type(kenshi_data::ItemType::Building).filter(|r| r.name == a[1]) {
        let f = kenshi_game::production::function_of(&sim.data, r);
        println!("{} [{}]: production {} function {:?}", r.name, r.string_id, sim.is_production_record(&r.string_id), f.map(|(f, k)| (f.name.clone(), k, f.bools.get("is production building").copied(), f.refs("produces").len(), f.refs("consumes").len())));
    }
}
