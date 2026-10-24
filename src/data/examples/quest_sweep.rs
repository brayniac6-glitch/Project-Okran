//! Every unique character's dialogue, swept for what could keep its quest lines from running in
//! the rewrite: dialogues with lasting effects (joining, relations, war, campaigns, items,
//! bounties, unlocks, AI changes) listed under events the rewrite never raises, lines that want
//! someone carried who no squad brings into the world, and uniques no squad template spawns:
//! `cargo run --release -p kenshi-data --example quest_sweep -- <install> [raised events,...]`
use kenshi_data::{GameData, GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The events the rewrite raises (EventTrigger; 1000: a CHARACTER's own "dialogue", talked to).
const RAISED: &[i32] = &[1, 2, 3, 8, 9, 13, 14, 15, 17, 18, 19, 20, 21, 22, 23, 24, 25, 27, 28, 29, 30, 31, 34, 35, 36, 37, 40, 41, 42, 43, 44, 45, 46, 51, 54, 55, 57, 58, 59, 61, 62, 63, 65, 66, 67, 69, 70, 71, 73, 74, 1000];

/// DialogActionEnum values with lasting effects.
const LASTING: &[i32] = &[3, 4, 5, 8, 9, 10, 13, 14, 18, 20, 24, 25, 28, 29, 30, 32, 35, 37, 38, 39, 40, 49, 53];
const LASTING_REFS: &[&str] = &["trigger campaign", "give item", "change relations", "unlocks", "unlock but keep me", "AI contract", "change AI"];

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let raised: Vec<i32> = a.get(1).map(|s| s.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_else(|| RAISED.to_vec());
    // squad templates something spawns, and the characters they bring
    let mut used: HashSet<String> = HashSet::new();
    for r in c.records.values() {
        for (cat, list) in &r.references {
            if matches!(cat.as_str(), "bar squads" | "default resident" | "residents" | "roaming squads" | "squad default" | "squads" | "homeless spawns" | "housemates" | "prisoners" | "slaves" | "spawns" | "main squad" | "squads to use" | "squad") {
                for (s, _) in list {
                    if c.get(s).is_some_and(|t| t.item_type() == Some(ItemType::SquadTemplate)) {
                        used.insert(s.clone());
                    }
                }
            }
        }
    }
    let mut spawned: HashSet<String> = HashSet::new();
    let mut leads: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in c.of_type(ItemType::SquadTemplate) {
        for cat in ["leader", "squad", "squad2", "choosefrom list"] {
            for (ch, _) in t.refs(cat) {
                leads.entry(ch.clone()).or_default().push(t.string_id.clone());
                if used.contains(&t.string_id) {
                    spawned.insert(ch.clone());
                }
            }
        }
    }
    // a line's tree's lasting effects (the line and every line under it)
    fn lasting(c: &GameDataContainer, l: &GameData, seen: &mut HashSet<String>, out: &mut BTreeSet<String>) {
        if !seen.insert(l.string_id.clone()) {
            return;
        }
        for (sid, _) in l.refs("effects") {
            if let Some(n) = c.get(sid).and_then(|e| e.ints.get("action name").copied())
                && LASTING.contains(&(n as i32))
            {
                out.insert(format!("a{n}"));
            }
        }
        for k in LASTING_REFS {
            if !l.refs(k).is_empty() {
                out.insert(k.to_string());
            }
        }
        for (sid, _) in l.refs("lines") {
            if let Some(n) = c.get(sid) {
                lasting(c, n, seen, out);
            }
        }
    }
    fn packages<'a>(c: &'a GameDataContainer, sid: &str, out: &mut Vec<&'a GameData>, depth: u32) {
        let Some(p) = c.get(sid) else { return };
        if out.iter().any(|o| o.string_id == p.string_id) || depth > 12 {
            return;
        }
        out.push(p);
        for (s, _) in p.refs("inheritsFrom") {
            packages(c, s, out, depth + 1);
        }
    }
    // a campaign's "special leader" set out with the squad it leads (world.rs
    // special_leader_squad, F050801)
    for k in c.of_type(ItemType::FactionCampaign) {
        for (l, _) in k.refs("special leader") {
            if leads.contains_key(l) {
                spawned.insert(l.clone());
            }
        }
    }
    // the factions' "special squads" (UNIQUE_SQUAD_TEMPLATEs, F030223): Kenshi keeps them but
    // never sends one out (F030219 / F030218, kenshi_game::world::SpecialSquads)
    let mut unique_squads: Vec<String> = Vec::new();
    for f in c.of_type(ItemType::Faction) {
        for (t, _) in f.refs("special squads") {
            if let Some(t) = c.get(t) {
                let leader: Vec<String> = t.refs("leader").iter().filter_map(|(l, _)| c.get(l)).map(|l| l.name.clone()).collect();
                unique_squads.push(format!("{} ({}: {leader:?})", t.name, f.name));
            }
        }
    }
    // the campaigns' announcements (said on EV_ANNOUNCEMENT whatever a package lists them under)
    let mut announced: HashSet<String> = c.of_type(ItemType::FactionCampaign).flat_map(|k| k.refs("dialog announcement").iter().map(|(d, _)| d.clone())).collect();
    // and the dialogues a line's "interrupt" starts (dialogue.rs: when the player talks next)
    announced.extend(c.of_type(ItemType::DialogueLine).flat_map(|l| l.refs("interrupt").iter().map(|(d, _)| d.clone())));
    let mut uniques: Vec<&GameData> = c.of_type(ItemType::Character).filter(|r| r.bools.get("unique").copied().unwrap_or(false)).collect();
    uniques.sort_by(|x, y| x.name.cmp(&y.name));
    let (mut blocked_total, mut never) = (0, Vec::new());
    for u in uniques {
        if !spawned.contains(&u.string_id) {
            never.push(u.name.clone());
        }
        // its packages: its own, its templates' dialog leader / squad, its faction's default
        let mut pk: Vec<&GameData> = Vec::new();
        for (s, _) in u.refs("dialogue package") {
            packages(&c, s, &mut pk, 0);
        }
        for t in leads.get(&u.string_id).into_iter().flatten().filter_map(|t| c.get(t)) {
            for cat in ["dialog leader", "dialog squad"] {
                for (s, _) in t.refs(cat) {
                    packages(&c, s, &mut pk, 0);
                }
            }
        }
        for (f, _) in u.refs("faction") {
            if let Some(f) = c.get(f) {
                for (s, _) in f.refs("dialog default") {
                    packages(&c, s, &mut pk, 0);
                }
            }
        }
        let mut entries: Vec<(String, Vec<i32>)> = pk.iter().flat_map(|p| p.refs("dialogs").iter().map(|(s, v)| (s.clone(), v.iter().copied().filter(|&e| e != 0 && e != 100).collect()))).collect();
        entries.extend(u.refs("dialogue").iter().map(|(s, _)| (s.clone(), vec![1000])));
        let mut blocked: Vec<String> = Vec::new();
        for (sid, events) in &entries {
            let Some(d) = c.get(sid) else { continue };
            if d.name.contains("Unfinished") || events.iter().any(|e| raised.contains(e)) || announced.contains(&d.string_id) {
                continue;
            }
            let mut out = BTreeSet::new();
            lasting(&c, d, &mut HashSet::new(), &mut out);
            if !out.is_empty() {
                blocked.push(format!("{} (events {events:?}): {}", d.name, out.into_iter().collect::<Vec<_>>().join(" ")));
            }
        }
        // lines wanting a carried character no squad spawns
        let mut carried: BTreeSet<String> = BTreeSet::new();
        for (sid, _) in &entries {
            let Some(d) = c.get(sid) else { continue };
            let mut stack = vec![d];
            let mut seen = HashSet::new();
            while let Some(l) = stack.pop() {
                if !seen.insert(l.string_id.clone()) {
                    continue;
                }
                for (ch, _) in l.refs("target carrying character") {
                    if !spawned.contains(ch) {
                        carried.insert(c.get(ch).map_or(ch.clone(), |x| x.name.clone()));
                    }
                }
                stack.extend(l.refs("lines").iter().filter_map(|(s, _)| c.get(s)));
            }
        }
        if !blocked.is_empty() || !carried.is_empty() {
            println!("{} ({} dialogues):", u.name, entries.len());
            for b in &blocked {
                println!("    never started: {b}");
            }
            for ch in &carried {
                println!("    wants carried, never spawned: {ch}");
            }
            blocked_total += blocked.len();
        }
    }
    println!("{blocked_total} dialogues with lasting effects on events not raised");
    println!("uniques no spawned squad brings: {never:?}");
    println!("factions' special squads (UNIQUE_SQUAD_TEMPLATE, never sent out in Kenshi): {unique_squads:?}");
}
