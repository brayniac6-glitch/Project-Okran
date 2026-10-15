//! The Follow order (headless): two of the squad follow the first, which walks off; their
//! distance to it and their tasks every 3 s:
//! `cargo run --release -p kenshi-game --example follow -- <install> X Y [guard]`
use kenshi_game::sim::Sim;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (zx, zy): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let guard = a.get(3).is_some_and(|g| g == "guard");
    let mut sim = Sim::load(Path::new(&a[0]), zx, zy, 0, &["player:3".to_string()]);
    let ids: Vec<usize> = sim.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
    let (lead, rest) = (ids[0], ids[1..].to_vec());
    sim.order_follow(&rest, lead, guard);
    let at = sim.folk[lead].mover.position_units();
    sim.order_move(&[lead], [at[0] + 600.0, at[1], at[2] + 300.0]);
    while sim.t < 40.0 {
        sim.step();
        if sim.tick.is_multiple_of(180) {
            let l = sim.folk[lead].mover.position_units();
            let line: Vec<String> = rest
                .iter()
                .map(|&i| {
                    let q = sim.folk[i].mover.position_units();
                    format!("{i}: {:.0} away, task {:?} q {:?}", (q[0] - l[0]).hypot(q[2] - l[2]), sim.folk[i].ai.current.as_ref().map(|c| c.task), sim.folk[i].ai.queue.iter().map(|s| s.task).collect::<Vec<_>>())
                })
                .collect();
            println!("t {:4.0}  leader moved {:.0}  {}", sim.t, (l[0] - at[0]).hypot(l[2] - at[2]), line.join(" | "));
        }
    }
}
