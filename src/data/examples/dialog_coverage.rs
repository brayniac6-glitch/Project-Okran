//! How much of the game's dialogue the rewrite's dialogue runs: the DIALOGUE_LINEs (and
//! DIALOGUEs) whose conditions and effects are all among the ported ones, the conditions and
//! actions used most that are not, and the unique characters:
//! `cargo run --release -p kenshi-data --example dialog_coverage -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::HashMap;

/// DialogConditionEnum values `kenshi_game::dialogue` reads (keep in step with it; the rest
/// hold, as F040880's faction test F040815 says of them).
const CONDITIONS: &[i32] = &[1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79];
/// DialogActionEnum values it applies (F040912 has no case for 22, 26, 27, 31, 44, 45, 46, 50,
/// 51: they do nothing).
const ACTIONS: &[i32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 23, 24, 25, 28, 29, 30, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 47, 48, 49, 52, 53, 54, 55, 56, 57, 58];

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut missing_c: HashMap<i32, usize> = HashMap::new();
    let mut missing_a: HashMap<i32, usize> = HashMap::new();
    let (mut lines, mut ok) = (0, 0);
    let check = |r: &kenshi_data::GameData, mc: &mut HashMap<i32, usize>, ma: &mut HashMap<i32, usize>| -> bool {
        let mut good = true;
        for (sid, _) in r.refs("conditions") {
            if let Some(d) = c.get(sid) {
                let n = d.ints.get("condition name").copied().unwrap_or(0) as i32;
                if n != 0 && !CONDITIONS.contains(&n) {
                    *mc.entry(n).or_default() += 1;
                    good = false;
                }
            }
        }
        for (sid, _) in r.refs("effects") {
            if let Some(d) = c.get(sid) {
                let n = d.ints.get("action name").copied().unwrap_or(0) as i32;
                if n != 0 && !ACTIONS.contains(&n) {
                    *ma.entry(n).or_default() += 1;
                    good = false;
                }
            }
        }
        good
    };
    for r in c.of_type(ItemType::DialogueLine).chain(c.of_type(ItemType::Dialogue)) {
        lines += 1;
        if check(r, &mut missing_c, &mut missing_a) {
            ok += 1;
        }
    }
    println!("dialogue lines and dialogues: {lines}, fully supported {ok} ({:.0}%)", ok as f32 * 100.0 / lines.max(1) as f32);
    let mut v: Vec<_> = missing_c.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("conditions not read, by use: {:?}", &v[..v.len().min(25)]);
    let mut v: Vec<_> = missing_a.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("actions not applied, by use: {:?}", &v[..v.len().min(25)]);
    let uniques: Vec<&str> = c.of_type(ItemType::Character).filter(|r| r.bools.get("unique").copied().unwrap_or(false)).map(|r| r.name.as_str()).collect();
    println!("unique characters: {}", uniques.len());
    // which squads they are in
    let in_squads = uniques
        .iter()
        .filter(|n| {
            c.of_type(ItemType::SquadTemplate).any(|s| ["leader", "squad", "choosefrom list"].iter().any(|k| s.refs(k).iter().any(|(sid, _)| c.get(sid).is_some_and(|x| x.name == **n))))
        })
        .count();
    println!("  of them in some squad template: {in_squads}");
}
