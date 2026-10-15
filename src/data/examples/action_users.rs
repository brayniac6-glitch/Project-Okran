//! How many dialogue lines use each dialogue action (DIALOG_ACTION "action name"), with an
//! example line: `cargo run --release -p kenshi-data --example action_users -- <install> [action]`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let install = std::path::Path::new(&a[0]);
    let only: Option<i32> = a.get(1).and_then(|v| v.parse().ok());
    let order = kenshi_io::mods::Install::new(install).game_order().unwrap();
    let data = kenshi_data::GameDataContainer::load(&order).unwrap();
    let mut by: std::collections::BTreeMap<i32, (usize, String)> = Default::default();
    for line in data.records.values() {
        for (sid, _) in line.refs("effects") {
            let Some(e) = data.get(sid) else { continue };
            let Some(&act) = e.ints.get("action name") else { continue };
            if only.is_some_and(|o| o != act) {
                continue;
            }
            let entry = by.entry(act).or_insert((0, String::new()));
            entry.0 += 1;
            if entry.1.is_empty() {
                entry.1 = format!("{} {:?}", line.string_id, line.strings.get("text0"));
            }
        }
    }
    for (act, (n, ex)) in by {
        println!("action {act}: {n} lines, e.g. {ex}");
    }
}
