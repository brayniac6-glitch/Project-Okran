//! How many items lie on a zone's ground after a while: `cargo run --release -p kenshi-game --example ground_count -- <install> X Y [seconds]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let secs: f32 = a.get(3).map_or(10.0, |s| s.parse().unwrap());
    let mut sim = kenshi_game::sim::Sim::load(std::path::Path::new(&a[0]), zx, zy, 0, &["player:1".to_string()]);
    while sim.t < secs {
        sim.step();
    }
    let me = sim.folk.iter().find(|p| p.player).map(|p| p.mover.position_units()).unwrap_or([0.0; 3]);
    println!("{} items on the ground; player at {:?}", sim.ground.len(), me.map(|v| v as i32));
    for g in sim.ground.iter().take(10) {
        println!("  {} x{} at {:?} ({:.0} from the player)", g.item.name, g.item.quantity, g.at.map(|v| v as i32), (g.at[0] - me[0]).hypot(g.at[2] - me[2]));
    }
}
