//! An item's tooltip lines, as Kenshi's items give them (DatapanelGUI lines, a label on the left
//! and its value on the right): F044624 (melee weapons), F044623 (crossbows), F044593 (armour),
//! F044610 (backpacks), F044611 (blueprints), F044622 (the rest: food, kits, ammunition), each
//! after the name (the title) and "Buyback" (F044592) / "Stolen" (F044591), and ending with the
//! weight and the value (F044583: "Value" at the price here, "(+n%)" / "(-n%)" over its worth
//! when it is not, and "Sell value").
//!
//! The lines are kept as text (`tooltip_ui`'s format): "label\tvalue\tcolour" each, the first
//! the title (colour t, the game's #492620: F044593 writes the name in it), then the lines (v: a
//! label in #140806 and its value in #afa68b; c: a class line in #afa68b; g / r: a value in the
//! bonus #a8b774 / penalty #59231a; #rrggbb: both in that colour, the grade line in the item's,
//! `item_colour`); a line with no tab is a paragraph. The damage against robots, humans, animals
//! and races (F044624 shows their labels by a helper not traced) read "{n}x", shown when not 1;
//! armour penetration reads "{n}%".
use super::Game;
use kenshi_char::inventory::Item;
use kenshi_data::{GameData, ItemType};

/// @fn F024535
/// @fn F024537
///
/// An item's colour (kenshi_colours.xml's ratings, read at start by F024546), as 0xrrggbb: a
/// weapon, armour, crossbow, limb, bag, nest item, map item or plain item by its kind (others
/// "DropItemLabelBackDefault", #ffffff); a plain item (ITEM) "ItemArtifact" (#0dfceb) when its
/// record is an "artifact", else "ItemStandard" (#e0cccc); the rest by their quality level (a
/// bag's, nest item's and map item's 0): under 16 "ItemGrade0" (#666666), under 36 "ItemGrade1"
/// (#d9ccbf), under 51 "ItemGrade2" (#4099b3), under 66 "ItemGrade3" (#66fc80), to 80
/// "ItemGrade4" (#fceb0d), over that "ItemGrade5" (not in the xml: the game's own (0.99, 0.92,
/// 0.05), F162738). The tooltips write their grade lines in it (getHex), the ground's item labels
/// take it (F024557).
pub(super) fn item_colour(data: &kenshi_data::GameDataContainer, item: &Item) -> u32 {
    match item.kind {
        Some(ItemType::Item) => {
            if data.get(&item.record).is_some_and(|r| r.bools.get("artifact").copied().unwrap_or(false)) {
                0x0dfceb
            } else {
                0xe0cccc
            }
        }
        Some(ItemType::Weapon | ItemType::Armour | ItemType::Crossbow | ItemType::LimbReplacement | ItemType::Container | ItemType::NestItem | ItemType::MapItem) => {
            let level = if matches!(item.kind, Some(ItemType::Container | ItemType::NestItem | ItemType::MapItem)) { 0 } else { item.quality as i32 };
            match level {
                ..16 => 0x666666,
                16..36 => 0xd9ccbf,
                36..51 => 0x4099b3,
                51..66 => 0x66fc80,
                66..=80 => 0xfceb0d,
                _ => 0xfcea0c,
            }
        }
        _ => 0xffffff,
    }
}

fn num(r: &GameData, k: &str) -> f32 {
    r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|v| *v as f32)).unwrap_or(0.0)
}

struct Lines(Vec<String>);

impl Lines {
    fn line(&mut self, label: &str, value: impl Into<String>, colour: &str) {
        // a plain line: its value in #afa68b
        let colour = if colour.is_empty() { "v" } else { colour };
        self.0.push(format!("{label}\t{}\t{colour}", value.into()));
    }
    /// A bonus: "+n" in green, a penalty in red, nothing for none.
    fn bonus(&mut self, good: &str, bad: &str, v: i32) {
        if v > 0 {
            self.line(good, format!("+{v}"), "g");
        } else if v < 0 {
            self.line(bad, v.to_string(), "r");
        }
    }
    fn separator(&mut self) {
        self.0.push("----------------\t\t".into());
    }
}

/// The tooltip of `item`; `price`: what it costs here (a trade), else none.
pub fn tip(game: &Game, item: &Item, price: Option<i32>) -> String {
    let data = &game.sim.data;
    let mut l = Lines(vec![format!("{}\t\tt", item.shown_name(data))]);
    let rec = data.get(&item.record);
    match (item.kind, rec) {
        (Some(ItemType::Weapon), Some(r)) => weapon(&mut l, game, item, r),
        (Some(ItemType::Crossbow), Some(r)) => crossbow(&mut l, game, item, r),
        (Some(ItemType::Armour), Some(r)) => armour(&mut l, game, item, r),
        (Some(ItemType::Container), Some(r)) => {
            weight(&mut l, item);
            value(&mut l, game, item, price);
            // F044610: the bag's effects after its value
            l.separator();
            let enc = num(r, "encumbrance effect");
            if enc != 0.0 && enc != 1.0 {
                l.line("Encumbrance reduction", format!("{}%", ((1.0 - enc) * 100.0).round() as i32), if enc < 1.0 { "g" } else { "r" });
            }
            l.bonus("Combat skills bonus", "Combat skills bonus", num(r, "combat skill bonus") as i32);
            for (label, k) in [("Athletics effect", "athletics mult"), ("Combat speed effect", "combat speed mult"), ("Stealth skill effect", "stealth mult"), ("Dodge skill effect", "dodge mult")] {
                let m = r.floats.get(k).copied().unwrap_or(1.0);
                if m != 1.0 {
                    let p = ((m - 1.0) * 100.0).round() as i32;
                    l.line(label, format!("{}{p}%", if p > 0 { "+" } else { "" }), if p > 0 { "g" } else { "r" });
                }
            }
            return l.0.join("\n");
        }
        (Some(ItemType::Blueprint), Some(r)) => {
            // F044611
            l.line("Research Blueprint", "", "c");
            let tech = r.refs("blueprint").first().and_then(|(s, _)| data.get(s)).unwrap_or(r);
            l.line("Tech Level", format!("{}", num(tech, "level") as i32), "");
            let known = game.sim.research_done.contains(&tech.string_id);
            l.0.push(if known { "Research already known".to_string() } else { "Right-click to learn tech".to_string() });
            weight(&mut l, item);
            value(&mut l, game, item, price);
            return l.0.join("\n");
        }
        _ => {
            // F044622
            if let Some((full, left)) = item.food {
                l.line("Nutrition", format!("{} nu", (if left < full { left } else { full }) as i32), "");
            }
            if let Some(k) = &item.medkit {
                l.line("Charges", format!("{}/{}", k.charges as i32, k.full as i32), "");
                l.line("Quality", format!("{}/100", k.quality as i32), "");
            }
            if let Some((full, left)) = item.ammo {
                l.line("Charges", format!("{}/{}", left as i32, full as i32), "");
            }
            weight(&mut l, item);
        }
    }
    value(&mut l, game, item, price);
    l.0.join("\n")
}

fn weight(l: &mut Lines, item: &Item) {
    l.line("Weight", format!("{} kg", item.weight.round() as i32), "");
}

/// F044583: "Value" (here: the price, "(+n%)" over its worth when dearer), "Sell value".
fn value(l: &mut Lines, game: &Game, item: &Item, price: Option<i32>) {
    let worth = item.value.max(0);
    match price {
        Some(p) if worth > 0 && p != worth => {
            let pct = ((p as f32 / worth as f32 - 1.0) * 100.0).round() as i32;
            let sign = if pct >= 0 { format!(" (+{pct}%)") } else { format!(" ({pct}%)") };
            l.line("Value", format!("c.{p}{sign}"), if pct > 0 { "r" } else { "g" });
        }
        Some(p) => l.line("Value", format!("c.{p}"), ""),
        None => l.line("Value", format!("c.{worth}"), ""),
    }
    let town = game.selected.first().and_then(|&id| game.sim.folk[id].town);
    l.line("Sell value", format!("c.{}", game.sim.price(item, town, false)), "");
}

/// F044624: the weapon's class, damage, blood loss, penetration, bonuses, damage against.
fn weapon(l: &mut Lines, game: &Game, item: &Item, r: &GameData) {
    let data = &game.sim.data;
    let class = match num(r, "skill category") as i32 {
        0 => "Katana class",
        1 => "Sabre class",
        4 => "Hacker class",
        3 => "Heavy weapons class",
        6 => "Crossbow class",
        8 => "Polearm class",
        2 => "Blunt weapon class",
        _ => "",
    };
    if !class.is_empty() {
        l.line(&format!("[{class}]"), "", "c");
    }
    let maker = item.maker.as_ref().and_then(|m| data.get(m));
    let model = item.model.as_ref().and_then(|m| data.get(m));
    // GLOBAL CONSTANTS "weapon inventory weight mult"
    let g = data.get("110-gamedata.quack").and_then(|c| c.floats.get("weapon inventory weight mult").copied()).unwrap_or(0.5);
    let w = kenshi_combat::weapon::Weapon::from_record(r, maker, model, item.quality * 0.01, g);
    l.line("Cutting damage", format!("{:.2}", w.cut_mult), "c");
    l.line("Blunt damage", format!("{:.2}", w.blunt_mult), "c");
    l.line("Blood loss", format!("{:.2}x", w.bleed), "c");
    let pen = num(r, "armour penetration") * 100.0;
    if pen != 0.0 {
        l.line("Armour penetration", format!("{}%", pen.round() as i32), if pen > 0.0 { "g" } else { "r" });
    }
    l.bonus("Attack bonus", "Attack bonus", w.attack_mod);
    l.bonus("Defence bonus", "Defence bonus", w.defence_mod);
    l.bonus("Indoors bonus", "Indoors penalty", w.indoors_mod);
    against(l, game, r);
    l.separator();
    l.line("Weight", format!("{} kg", w.inventory_weight.max(item.weight) as i32), "");
}

/// The damage against robots, humans, animals and races.
fn against(l: &mut Lines, game: &Game, r: &GameData) {
    for (label, k) in [("Damage vs robots", "robot damage mult"), ("Damage vs humans", "human damage mult"), ("Damage vs animals", "animal damage mult")] {
        let m = r.floats.get(k).copied().unwrap_or(1.0);
        if m != 1.0 {
            l.line(label, format!("{m:.2}x"), if m > 1.0 { "g" } else { "r" });
        }
    }
    for (race, v) in r.refs("race damage") {
        let Some(rr) = game.sim.data.get(race) else { continue };
        let m = v[0] as f32 * 0.01;
        l.line(&format!("Damage vs {}", rr.name), format!("{m:.2}x"), if m > 1.0 { "g" } else { "r" });
    }
}

/// F044623: a crossbow's damage, accuracy, reload, range, velocity, skill, ammunition.
fn crossbow(l: &mut Lines, game: &Game, item: &Item, r: &GameData) {
    let data = &game.sim.data;
    l.line("[Crossbow class]", "", "c");
    let gun = kenshi_combat::ranged::Gun::personal(data, r, item.quality, game.sim.advanced.damage);
    l.line("Damage:", format!("{}-{}", gun.damage_min, gun.damage_max), "c");
    l.line("Accuracy deviation:", format!("{:.2} degrees", gun.deviation), "c");
    l.line("Reload time:", format!("{:.2}", (gun.reload_min + gun.reload_max) * 0.5), "c");
    l.line("Range:", format!("{:.0}", gun.range), "c");
    l.line("Velocity:", format!("{:.0}", gun.shot_speed), "c");
    l.line("Required skill:", format!("{:.0}", gun.accuracy_perfect), "c");
    if let Some(a) = gun.ammo.as_ref().and_then(|a| data.get(a)) {
        l.separator();
        l.line("Ammunition type:", a.name.clone(), "");
    }
    against(l, game, r);
    l.separator();
    weight(l, item);
}

/// F044593: an armour piece's uniform, grade, class and material, resistances.
fn armour(l: &mut Lines, game: &Game, item: &Item, r: &GameData) {
    let data = &game.sim.data;
    if let Some(f) = item.uniform.as_ref().and_then(|f| data.get(f)) {
        l.line("Faction Uniform:", f.name.clone(), "r");
    }
    let graded = kenshi_char::quality::graded(data, &item.record);
    if graded && 0.0 < item.quality && item.quality < 100.0 {
        // in the item's colour (F044593: getHex of F024535's)
        l.line(&format!("{} Grade", kenshi_char::quality::grade_name(item.quality)), "", &format!("#{:06x}", item_colour(data, item)));
    }
    if graded {
        let class = match num(r, "class") as i32 {
            1 => "Light Armour Class",
            2 => "Medium Armour Class",
            3 => "Heavy Armour Class",
            _ => "",
        };
        let material = match num(r, "material type") as i32 {
            1 => "Leather",
            2 => "Chain",
            3 => "Metal Plate",
            _ => "Cloth",
        };
        if !class.is_empty() {
            l.line(&format!("[{class}]"), format!("[{material}]"), "c");
        } else {
            l.line(&format!("[{material}]"), "", "c");
        }
    }
    if let Some(a) = kenshi_combat::hit::Armour::from_record(r, item.quality * 0.01, |_| Some(0)) {
        l.line("Blunt resistance", format!("{:.2}%", a.blunt_resistance * 100.0), "");
        l.line("Cut resistance", format!("{:.2}%", a.cut_resistance * 100.0), "");
        if a.cut_into_stun > 0.0 {
            l.line("Cut resistance efficiency", format!("{:.2}%", (1.0 - a.cut_into_stun) * 100.0), "");
        }
        if a.pierce_reduction > 1.0 {
            l.line("Harpoon resistance", format!("{} pts", a.pierce_reduction as i32), "");
        }
    }
    // its weather protections: "[Weather Protections]", then "-<weather>" with the amount for
    // each it lists (WeatherAffecting names, F044593 via its name table)
    let mut kinds: Vec<i32> = r.ints.iter().filter(|(k, _)| k.starts_with("weather protection") && *k != "weather protection amount").map(|(_, v)| *v).filter(|v| (1..=5).contains(v)).collect();
    kinds.sort();
    kinds.dedup();
    if !kinds.is_empty() {
        l.line("[Weather Protections]", "", "c");
        let amount = r.floats.get("weather protection amount").copied().unwrap_or(0.0);
        for k in kinds {
            let name = ["", "Duststorms", "Acid", "Burning", "Gas", "Rain"][k as usize];
            l.line(&format!("-{name}"), format!("{}%", (amount * 100.0) as i32), "");
        }
    }
    weight(l, item);
}
