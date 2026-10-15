//! Every DialogConditionEnum and DialogActionEnum the base game's dialogue uses, how often, with
//! which "who" / "compare by" and values; and the events its packages start dialogues on:
//! `cargo run --release -p kenshi-data --example dialog_census -- <install>`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::{BTreeMap, BTreeSet};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let c = GameDataContainer::load(&order).unwrap();
    let mut conds: BTreeMap<i32, (usize, BTreeSet<(i32, i32)>, BTreeSet<i32>, BTreeSet<i32>)> = BTreeMap::new();
    let mut acts: BTreeMap<i32, (usize, BTreeSet<i32>, BTreeSet<String>)> = BTreeMap::new();
    let mut fields: BTreeMap<String, usize> = BTreeMap::new();
    for r in c.of_type(ItemType::DialogueLine).chain(c.of_type(ItemType::Dialogue)).chain(c.of_type(ItemType::WordSwaps)) {
        for (sid, v) in r.refs("conditions") {
            let Some(d) = c.get(sid) else { continue };
            let n = d.ints.get("condition name").copied().unwrap_or(0);
            let e = conds.entry(n).or_default();
            e.0 += 1;
            e.1.insert((d.ints.get("who").copied().unwrap_or(0), d.ints.get("compare by").copied().unwrap_or(0)));
            e.2.insert(v[0]);
            if let Some(t) = d.ints.get("tag") {
                e.3.insert(*t);
            }
        }
        for (sid, v) in r.refs("effects") {
            let Some(d) = c.get(sid) else { continue };
            let n = d.ints.get("action name").copied().unwrap_or(0);
            let e = acts.entry(n).or_default();
            e.0 += 1;
            e.1.insert(v[0]);
            if let Some(x) = d.ints.get("action value") {
                e.2.insert(format!("i{x}"));
            }
            if let Some(x) = d.floats.get("action value") {
                e.2.insert(format!("f{x}"));
            }
        }
        for (cat, list) in &r.references {
            if !list.is_empty() {
                *fields.entry(format!("ref {cat}")).or_default() += 1;
            }
        }
        for (k, b) in &r.bools {
            if *b {
                *fields.entry(format!("bool {k}")).or_default() += 1;
            }
        }
        for (k, v) in &r.ints {
            if *v != 0 {
                *fields.entry(format!("int {k}")).or_default() += 1;
            }
        }
        for (k, v) in &r.floats {
            if *v != 0.0 {
                *fields.entry(format!("float {k}")).or_default() += 1;
            }
        }
    }
    println!("== conditions (id: uses, (who, compare), values, tags)");
    for (n, (k, wc, vals, tags)) in &conds {
        let vals: Vec<_> = vals.iter().take(12).collect();
        println!("c{n}: {k} {wc:?} {vals:?} {tags:?}");
    }
    println!("== actions (id: uses, ref values, action values)");
    for (n, (k, vals, av)) in &acts {
        let vals: Vec<_> = vals.iter().take(12).collect();
        println!("a{n}: {k} {vals:?} {av:?}");
    }
    println!("== fields set");
    for (f, k) in &fields {
        println!("{f}: {k}");
    }
    let mut events: BTreeMap<i32, usize> = BTreeMap::new();
    for p in c.of_type(ItemType::DialoguePackage) {
        for (_, v) in p.refs("dialogs") {
            *events.entry(v[0]).or_default() += 1;
        }
    }
    for p in c.of_type(ItemType::Character) {
        if !p.refs("dialogue").is_empty() {
            *events.entry(1000).or_default() += 1;
        }
    }
    println!("== events (EventTriggerEnum: package entries; 1000 = CHARACTER \"dialogue\")");
    println!("{events:?}");
}
