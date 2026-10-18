//! Every ANIMATION and ANIMAL_ANIMATION record, as a TSV for the animation audit
//! (docs/ANIMATIONS.md): its kind, string id, name, clip, category, layer, its selection
//! fields, and who refers to it (the referring records' type and reference category):
//! `cargo run --release -p kenshi-anim --example anim_audit -- <install> > anims.tsv`
use kenshi_data::{GameDataContainer, ItemType};
use kenshi_io::mods::Install;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut order = Install::new(&a[0]).load_order().unwrap();
    order.files.retain(|f| f.core);
    let data = GameDataContainer::load(&order).unwrap();
    // who refers to each record
    let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in data.records.values() {
        for (cat, v) in &r.references {
            for (sid, _) in v {
                let kind = r.item_type().map_or("?".to_string(), |t| t.name().to_string());
                by.entry(sid.clone()).or_default().push(format!("{kind}:{cat}:{}", r.name));
            }
        }
    }
    println!("kind\tsid\tname\tclip\tcategory\tlayer\tidle\tmove\tstrafe\tweather\taction\tloop\trelocates\tcombat\tstealth\tcrouched\tprone\tcarryL\tcarryR\tcarried\tweaponL\tweaponR\tstumble\tdisabled\trefs");
    for (t, kind) in [(ItemType::Animation, "ANIMATION"), (ItemType::AnimalAnimation, "ANIMAL_ANIMATION")] {
        let mut recs: Vec<_> = data.of_type(t).collect();
        recs.sort_by(|x, y| x.name.cmp(&y.name));
        for r in recs {
            let b = |k: &str| r.bools.get(k).copied().unwrap_or(false);
            let i = |k: &str| r.ints.get(k).copied().unwrap_or(0);
            let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
            let s = |k: &str| r.strings.get(k).cloned().unwrap_or_default();
            let refs = by.get(&r.string_id).map(|v| {
                let mut v = v.clone();
                v.sort();
                v.dedup();
                v.join("; ")
            });
            println!(
                "{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                r.string_id,
                r.name,
                s("anim name"),
                i("category"),
                s("layer"),
                b("idle"),
                f("move speed"),
                f("strafe speed max"),
                i("weather type"),
                b("is action"),
                b("loop"),
                b("relocates"),
                i("is combat mode"),
                i("stealth mode"),
                b("crouched"),
                b("prone"),
                b("carrying left"),
                b("carrying right"),
                b("being carried"),
                i("has weapon L"),
                i("has weapon R"),
                !r.refs("stumbles").is_empty(),
                b("disabled"),
                refs.unwrap_or_default()
            );
        }
    }
}
