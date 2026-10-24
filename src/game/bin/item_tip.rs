//! An item's tooltip lines, as Kenshi's ToolTipInventory gathers them (F048703): the item's
//! tooltip lines (its vtable slot 79) and then its description lines (slot 80), each a
//! DatapanelGUI line, a label on the left and its value on the right.
//!
//! Slot 79: F044624 (melee weapons), F044623 (crossbows), F044593 (armour), F044609 (robot
//! limbs), F044610 (backpacks), F044611 (blueprints), F044622 (the rest: food, kits,
//! ammunition, books). Each starts with the name (the title, #492620) and "Buyback" (F044592) /
//! "Stolen" (F044591), and ends with the weight and the value lines (F044583: "-Value",
//! "-Sell value").
//! (not ported) the line Kenshi puts beside "Stolen" while an item is dragged over a trader
//! (F042764: "ILLEGAL GOODS!" / "NO!", "Illegal goods?" / "No problem!", "Stolen goods?",
//! "Fencing chance:", "Stealing chance:"): it needs the trade's fencing and stealing chances
//! (F042770's checks, 0x140036dd1, 0x1400420a0), which the trade port has not got.
//! Slot 80: the record's "description" (F044584 / F044586 / F044587 / F044589 / F044590); a
//! weapon's model and maker after it (F044588), an armour piece's coverage and skill effects
//! before it (F044595).
//!
//! Kenshi writes most labels as "-" and the name, in the label's colour ("-Cutting damage").
//!
//! The lines are kept as text (`tooltip_ui`'s format): "label\tvalue\tcolour" each, the first
//! the title (colour t, #492620), then the lines (v: a label in #140806 and its value in
//! #afa68b; c: both #afa68b; g / r: a value in the bonus #a8b774 / penalty #59231a; #rrggbb:
//! both in that colour, the grade line in the item's, `item_colour`; #rrggbb/#rrggbb: the
//! label's and the value's); a line with no tab is a paragraph; MyGUI colour tags inside either
//! side colour what follows them. Kenshi's colour strings (initialised by F163756..F163764): #afa68b, #140806,
//! #492620, #59231a, #bf4834, #a8b774, #444444 (separators), #777777, #14ffdc (artifacts).
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

/// Kenshi's "{1,num}" of a float: the stream's default, six significant digits, no trailing
/// zeros ("0.5", "12", "19.5").
fn g6(v: f32) -> String {
    let v = v as f64;
    if v == 0.0 {
        return "0".into();
    }
    let digits = 6 - 1 - v.abs().log10().floor() as i32;
    let s = format!("{:.*}", digits.max(0) as usize, v);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

struct Lines(Vec<String>);

/// Kenshi's dark text, #140806: both sides of the weight line, the descriptions.
const DARK: &str = "#140806";
/// The separator lines' #444444.
const GREY: &str = "#444444";

impl Lines {
    fn line(&mut self, label: &str, value: impl Into<String>, colour: &str) {
        // a plain line: its value in #afa68b
        let colour = if colour.is_empty() { "v" } else { colour };
        self.0.push(format!("{label}\t{}\t{colour}", value.into()));
    }
    /// A stat line: "-" and its label (Kenshi's label prefix).
    fn stat(&mut self, label: &str, value: impl Into<String>, colour: &str) {
        self.line(&format!("-{label}"), value, colour);
    }
    /// A bonus: "+n" in green, a penalty "-n" in red, nothing for none.
    fn bonus(&mut self, good: &str, bad: &str, v: i32) {
        if v > 0 {
            self.stat(good, format!("+{v}"), "g");
        } else if v < 0 {
            self.stat(bad, v.to_string(), "r");
        }
    }
    /// F044585: "-label" and int(v) ("+n" green, "-n" red; with `percent` a "%" after it),
    /// nothing when it is 0.
    fn signed(&mut self, label: &str, v: f32, percent: bool) {
        let n = v as i32;
        let pc = if percent { "%" } else { "" };
        if n > 0 {
            self.stat(label, format!("+{n}{pc}"), "g");
        } else if n < 0 {
            self.stat(label, format!("{n}{pc}"), "r");
        }
    }
    /// F044594: "-label" and "{v:.2}x", green from 1 up, red under it, nothing at 1.
    fn effect(&mut self, label: &str, v: f32) {
        if v != 1.0 {
            self.stat(label, format!("{v:.2}x"), if 1.0 <= v { "g" } else { "r" });
        }
    }
    fn separator(&mut self, dashes: usize, colour: &str) {
        self.0.push(format!("{}\t\t{colour}", "-".repeat(dashes)));
    }
    /// An empty line (a space: a line's height, as MyGUI gives an empty caption).
    fn blank(&mut self) {
        self.0.push(" \t\t".into());
    }
    /// The record's "description" in #140806 (F044584 and its kind).
    fn description(&mut self, r: &GameData, colour: &str) {
        let d = r.strings.get("description").cloned().unwrap_or_default();
        self.line(&d, "", colour);
    }
}

/// @fn F044592
///
/// "Buyback" in red when some of the stack is buyback (F042723), " (n)" after it when not all.
fn buyback(l: &mut Lines, item: &Item) {
    if item.buyback != 0 {
        let mut s = "Buyback".to_string();
        if item.quantity as i32 != item.buyback {
            s += &format!(" ({})", item.buyback);
        }
        l.line(&s, "", RED);
    }
}

/// The faction of an item's holder (Item +0x160: whose inventory it is in): a person's, a worn
/// bag's wearer's (F044581's container case), a town container's, the player's buildings'.
fn holder_faction(sim: &kenshi_game::sim::Sim, holder: usize) -> Option<String> {
    use kenshi_game::{base::BUILDING_OWNER, loot::{BAG_OWNER, BOX_OWNER}};
    if let Some(p) = holder.checked_sub(BAG_OWNER) {
        return sim.folk.get(p)?.faction.clone();
    }
    if let Some(k) = holder.checked_sub(BOX_OWNER) {
        return sim.counter_trader(k).and_then(|t| sim.folk[t].faction.clone()).or_else(|| sim.box_faction(k));
    }
    if holder.checked_sub(BUILDING_OWNER).is_some() {
        return Some(kenshi_game::sim::PLAYER_FACTION.to_string());
    }
    sim.folk.get(holder)?.faction.clone()
}

/// @fn F044581
/// @fn F044591
///
/// Whether an item is stolen (F044581 asked for its owner's handle): it has owners (Item
/// +0x140, here a faction) that are not its holder's faction, the holder's faction neither of
/// "fundamental type" 8 nor "not real" (none held: stolen); then "Stolen from:" and the owners'
/// name in red (F044591; "Stolen" alone when the owners are gone).
fn stolen(l: &mut Lines, game: &Game, item: &Item, holder: Option<usize>) {
    let sim = &game.sim;
    let Some(owner) = item.owner.as_ref() else { return };
    let held = holder.and_then(|h| holder_faction(sim, h));
    if let Some(f) = held.as_deref()
        && (f == owner.as_str() || sim.fundamental_type(f) == 8 || sim.faction_not_real(f))
    {
        return;
    }
    match sim.data.get(owner) {
        Some(f) => {
            let name = if owner == kenshi_game::sim::PLAYER_FACTION { sim.player_faction_name() } else { f.name.clone() };
            l.line("Stolen from:", name, RED);
        }
        None => l.line("Stolen", "", RED),
    }
}

/// The tooltip of `item`; `price`: what it costs here (a trade), else none; `holder`: whose
/// inventory it is in (`kenshi_game::sim::Sim::inv`'s owner numbers).
pub fn tip(game: &Game, item: &Item, price: Option<i32>, holder: Option<usize>) -> String {
    let data = &game.sim.data;
    let mut l = Lines(vec![format!("{}\t\tt", item.shown_name(data))]);
    let rec = data.get(&item.record);
    match (item.kind, rec) {
        (Some(ItemType::Weapon), Some(r)) => {
            buyback(&mut l, item);
            stolen(&mut l, game, item, holder);
            weapon(&mut l, game, item, r);
            value(&mut l, game, item, price);
            weapon_about(&mut l, game, item, r);
        }
        (Some(ItemType::Crossbow), Some(r)) => {
            crossbow(&mut l, game, item, r, holder);
            value(&mut l, game, item, price);
            l.description(r, DARK);
        }
        (Some(ItemType::Armour), Some(r)) => {
            armour(&mut l, game, item, r, holder);
            value(&mut l, game, item, price);
            armour_about(&mut l, game, item, r);
        }
        (Some(ItemType::Container), Some(r)) => {
            bag(&mut l, game, item, r, price, holder);
            l.description(r, DARK);
        }
        (Some(ItemType::Blueprint | ItemType::Research), Some(r)) => {
            // F044611 (BlueprintItem, its research +0xc0: here the item's own RESEARCH record, as
            // the item spots' "blueprints" make them): the title is the research's name
            let tech = r.refs("blueprint").first().and_then(|(s, _)| data.get(s)).unwrap_or(r);
            l.0[0] = format!("{}\t\tt", tech.name);
            l.line("[Research Blueprint]", "", "t");
            buyback(&mut l, item);
            stolen(&mut l, game, item, holder);
            blueprint(&mut l, game, tech);
            l.stat("Tech Level", format!("{}", num(tech, "level") as i32), "");
            value(&mut l, game, item, price);
            requirements(&mut l, game, tech);
            // F044590: the research's description, in the text's own colour
            let d = tech.strings.get("description").cloned().unwrap_or_default();
            l.0.push(format!("{d}\t\t"));
        }
        (Some(ItemType::LimbReplacement), Some(r)) => {
            // F044609: its grade ("[Shoddy]") in the item's colour
            l.line(&format!("[{}]", kenshi_char::quality::grade_name(item.quality)), "", &format!("#{:06x}", item_colour(data, item)));
            buyback(&mut l, item);
            stolen(&mut l, game, item, holder);
            limb(&mut l, item, r);
            l.separator(16, GREY);
            l.stat("Weight", format!("{} kg", item.weight as i32), DARK);
            value(&mut l, game, item, price);
            l.description(r, DARK);
        }
        (_, r) => {
            plain(&mut l, game, item, r, holder);
            value(&mut l, game, item, price);
            if let Some(r) = r {
                // F044622: an ITEM's charges (more than 1) and quality (not 1) after its value,
                // but food's (function 3) and ammunition's (16)
                let function = num(r, "item function") as i32;
                if item.kind == Some(ItemType::Item) && function != 3 && function != 16 {
                    let full = num(r, "charges");
                    let left = item.medkit.map_or(full, |k| k.charges);
                    if 1.0 < full {
                        l.stat("Charges", format!("{}/{}", left as i32, full as i32), "c");
                    }
                    let q = item.medkit.map_or(num(r, "quality"), |k| k.quality);
                    if q != 1.0 {
                        l.stat("Quality", format!("{}/100", q as i32), "c");
                    }
                }
                l.description(r, DARK);
            }
        }
    }
    let text = l.0.join("\n");
    // (testing) KENSHI_TIP_DUMP=<text>: each item tooltip with that text printed as it is made
    if let Ok(want) = std::env::var("KENSHI_TIP_DUMP")
        && text.contains(want.as_str())
    {
        eprintln!("tip:\n{text}\n--");
    }
    text
}

/// F044622: the lines of an item of no kind of its own (food, kits, ammunition, books, parts):
/// its tags, then nutrition and ingredients, an ammunition pack's damage and shots, and the
/// weight ("{n} x " and the whole stack's after it when there are more than one).
fn plain(l: &mut Lines, game: &Game, item: &Item, r: Option<&GameData>, holder: Option<usize>) {
    let data = &game.sim.data;
    if item.quantity > 1 {
        l.0[0] = format!("{} ({})\t\tt", item.shown_name(data), item.quantity);
    }
    buyback(l, item);
    let function = r.map_or(0, |r| num(r, "item function") as i32);
    if function == 16 {
        l.line("[Ammunition]", "", "c");
    }
    // Item +0x180: its record's "artifact" (F043898); +0x12a: one of a kind
    if r.is_some_and(|r| r.bools.get("artifact").copied().unwrap_or(false)) {
        l.line("[Research Artifact]", "", "#14ffdc");
    }
    if item.unique {
        l.line("[Unique]", "", "#14ffdc");
    }
    if function == 13 {
        l.0.push("[Book]\t\t".into());
    }
    if let (true, Some(r)) = (function == 3 || function == 15, r) {
        // the food left (+0x118: the one being eaten's, else its "charges")
        let left = item.food.map_or(num(r, "charges"), |f| f.1);
        if function == 15 {
            // (not ported) "-Only for:" and the races its "special food" feeds
            l.line("[Possibly Edible]", "", "#a8b774");
        } else {
            l.line("[Food]", "", "#a8b774");
        }
        l.line("Nutrition", format!("{} nu", left as i32), "c");
        // the ingredients' names, ", " between them (only once the list has some text)
        let mut names = String::new();
        for (sid, _) in r.refs("ingredients") {
            if let Some(i) = data.get(sid) {
                if !names.is_empty() {
                    names.push_str(", ");
                }
                names.push_str(&i.name);
            }
        }
        l.line("Ingredients", names, DARK);
    }
    stolen(l, game, item, holder);
    if let (Some((full, left)), Some(r)) = (item.ammo, r) {
        // the pack's damage, its quality / 100 ("1x"), and the shots left
        l.line("Damage:", format!("{}x", g6(num(r, "quality") * 0.01)), "c");
        l.stat("Shots", format!("{}/{}", left as i32, full as i32), "c");
    }
    let tenth = |w: f32| (w * 10.0) as i32 as f32 * 0.1;
    let mut w = String::new();
    if item.quantity > 1 {
        w = format!("{} x ", item.quantity);
    }
    w += &format!("{} kg", g6(tenth(item.weight)));
    if item.quantity > 1 {
        w += &format!(" ({} kg)", g6(tenth(item.weight * item.quantity as f32)));
    }
    l.stat("Weight", w, DARK);
}

/// F044583: "-Value" (here: the price, "(+n%)" over its worth when dearer), "-Sell value".
fn value(l: &mut Lines, game: &Game, item: &Item, price: Option<i32>) {
    let worth = item.value.max(0);
    match price {
        Some(p) if worth > 0 && p != worth => {
            let pct = ((p as f32 / worth as f32 - 1.0) * 100.0).round() as i32;
            let sign = if pct >= 0 { format!(" (+{pct}%)") } else { format!(" ({pct}%)") };
            l.stat("Value", format!("c.{p}{sign}"), if pct > 0 { "r" } else { "g" });
        }
        Some(p) => l.stat("Value", format!("c.{p}"), ""),
        None => l.stat("Value", format!("c.{worth}"), ""),
    }
    let town = game.selected.first().and_then(|&id| game.sim.folk[id].town);
    l.stat("Sell value", format!("c.{}", game.sim.price(item, town, false)), "");
}

/// F044624: the weapon's class, damage, blood loss, penetration, bonuses, damage against,
/// weight.
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
    l.stat("Cutting damage", format!("{:.2}", w.cut_mult), "c");
    l.stat("Blunt damage", format!("{:.2}", w.blunt_mult), "c");
    l.stat("Blood loss", format!("{:.2}x", w.bleed), "c");
    let pen = num(r, "armour penetration") * 100.0;
    if pen != 0.0 {
        l.signed("Armour penetration", pen, true);
    }
    l.bonus("Attack bonus", "Attack bonus", w.attack_mod);
    l.bonus("Defence bonus", "Defence bonus", w.defence_mod);
    l.bonus("Indoors bonus", "Indoors penalty", w.indoors_mod);
    against(l, game, r);
    l.separator(16, GREY);
    l.stat("Weight", format!("{} kg", w.inventory_weight.max(item.weight) as i32), DARK);
}

/// F044588: a weapon's description, a line, "[Model ## {model}]" in the item's colour (MyGUI
/// shows "##" as "#"), then "[Manufactured by {maker}]" (#afa68b) and the maker's "company
/// description"; one the player's smith made (its crafter's name, Item +0x1f8) "[Crafted by
/// {1}]" and "Crafted by the smith {1} of {2}." (quality under 45) / "A masterful weapon
/// crafted by the renowned smith {1} of {2}." ({2}: the player's faction's name).
fn weapon_about(l: &mut Lines, game: &Game, item: &Item, r: &GameData) {
    let data = &game.sim.data;
    l.description(r, DARK);
    l.blank();
    if let Some(m) = item.model.as_ref().and_then(|m| data.get(m)) {
        l.line(&format!("[Model ## {}]", m.name), "", &format!("#{:06x}", item_colour(data, item)));
    }
    match item.crafter.as_ref().filter(|c| !c.is_empty()) {
        None => {
            if let Some(m) = item.maker.as_ref().and_then(|m| data.get(m)) {
                l.line(&format!("[Manufactured by {}]", m.name), "", "c");
                l.line(&m.strings.get("company description").cloned().unwrap_or_default(), "", DARK);
            }
        }
        Some(smith) => {
            l.line(&format!("[Crafted by {smith}]"), "", "c");
            let of = game.sim.player_faction_name();
            let text = if (item.quality as i32) < 45 {
                format!("Crafted by the smith {smith} of {of}.")
            } else {
                format!("A masterful weapon crafted by the renowned smith {smith} of {of}.")
            };
            l.line(&text, "", DARK);
        }
    }
}

/// The damage against robots, humans, animals (F044585: (mult - 1) x 100 as "+n%") and races
/// (the race's value - 100).
fn against(l: &mut Lines, game: &Game, r: &GameData) {
    for (label, k) in [("Damage vs robots", "robot damage mult"), ("Damage vs humans", "human damage mult"), ("Damage vs animals", "animal damage mult")] {
        let m = r.floats.get(k).copied().unwrap_or(1.0);
        l.signed(label, (m - 1.0) * 100.0, true);
    }
    for (race, v) in r.refs("race damage") {
        let Some(rr) = game.sim.data.get(race) else { continue };
        l.signed(&format!("Damage vs {}", rr.name), (v[0] - 100) as f32, true);
    }
}

/// F044623: a crossbow's grade, class, damage, accuracy, reload, range, velocity, blood loss,
/// skill, ammunition, damage against, weight.
fn crossbow(l: &mut Lines, game: &Game, item: &Item, r: &GameData, holder: Option<usize>) {
    let data = &game.sim.data;
    // its grade ("[Shoddy]", F044535's names) in the item's colour
    l.line(&format!("[{}]", kenshi_char::quality::grade_name(item.quality)), "", &format!("#{:06x}", item_colour(data, item)));
    buyback(l, item);
    stolen(l, game, item, holder);
    l.line("[Crossbow class]", "", "c");
    let gun = kenshi_combat::ranged::Gun::personal(data, r, item.quality, game.sim.advanced.damage);
    l.stat("Damage:", format!("{}-{}", gun.damage_min, gun.damage_max), "");
    l.stat("Accuracy deviation:", format!("{:.2} degrees", gun.deviation), "");
    l.stat("Reload time:", format!("{:.2}-{:.2}s", gun.reload_min, gun.reload_max), "");
    // the range in metres (x 0.1), the velocity in m/s (x 0.23)
    l.stat("Range:", format!("{:.0}m", gun.range * 0.1), "");
    l.stat("Velocity:", format!("{:.0}m/s", gun.shot_speed * 0.23), "");
    // Weapon +0x228: the record's "bleed mult" (the Weapon constructor F047096)
    l.stat("Blood loss:", format!("{:.2}x", num(r, "bleed mult")), "");
    l.stat("Required skill:", format!("{:.0}", gun.accuracy_perfect), "");
    l.separator(15, GREY);
    let ammo = gun.ammo.as_ref().and_then(|a| data.get(a)).map_or("???".to_string(), |a| a.name.clone());
    l.stat("Ammunition type:", ammo, "");
    against(l, game, r);
    l.separator(16, GREY);
    l.stat("Weight", format!("{} kg", item.weight as i32), DARK);
}

/// F044593: an armour piece's uniform, grade, class and material, resistances, weight and
/// weather protections.
fn armour(l: &mut Lines, game: &Game, item: &Item, r: &GameData, holder: Option<usize>) {
    let data = &game.sim.data;
    if let Some(f) = item.uniform.as_ref().and_then(|f| data.get(f)) {
        // in red, the faction's name in #140806 when the player's faction takes it for an ally
        // (FactionRelations::isAlly F041495 on the player's faction's relations)
        let sim = &game.sim;
        let mine = kenshi_game::sim::PLAYER_FACTION;
        let rel = sim.relations.get(mine).cloned().or_else(|| data.get(mine).map(|r| kenshi_ai::faction::FactionRelations::from_data(r, data)));
        let ally = rel.is_some_and(|r| r.is_ally(Some(f.string_id.as_str())));
        l.line("Faction Uniform:", f.name.clone(), if ally { "#59231a/#140806" } else { RED });
    }
    buyback(l, item);
    stolen(l, game, item, holder);
    let graded = kenshi_char::quality::graded(data, &item.record);
    if graded && 0.0 < item.quality && item.quality < 100.0 {
        // "[{1} Grade]" in the item's colour (getHex of F024535's)
        l.line(&format!("[{} Grade]", kenshi_char::quality::grade_name(item.quality)), "", &format!("#{:06x}", item_colour(data, item)));
    }
    if graded {
        let class = match num(r, "class") as i32 {
            1 => "[Light Armour Class]",
            2 => "[Medium Armour Class]",
            3 => "[Heavy Armour Class]",
            _ => "",
        };
        let material = match num(r, "material type") as i32 {
            1 => "Leather",
            2 => "Chain",
            3 => "Metal Plate",
            _ => "Cloth",
        };
        // the class on the left, the material on the right
        l.line(class, format!("[{material}]"), "c");
    }
    if let Some(a) = kenshi_combat::hit::Armour::from_record(r, item.quality * 0.01, |_| Some(0)) {
        // whole percents ("{1,num=fix,p=2}%" of an int)
        l.stat("Blunt resistance", format!("{}%", (a.blunt_resistance * 100.0) as i32), "");
        l.stat("Cut resistance", format!("{}%", (a.cut_resistance * 100.0) as i32), "");
        if a.cut_into_stun > 0.0 {
            l.stat("Cut resistance efficiency", format!("{}%", ((1.0 - a.cut_into_stun) * 100.0) as i32), "");
        }
        if a.pierce_reduction > 1.0 {
            l.stat("Harpoon resistance", format!("{} pts", a.pierce_reduction as i32), "");
        }
    }
    l.stat("Weight", format!("{} kg", item.weight as i32), DARK);
    // its weather protections: "[Weather Protections]", then "-<weather>" with the amount for
    // each it lists, both green (WeatherAffecting names, by its name table)
    let mut kinds: Vec<i32> = r.ints.iter().filter(|(k, _)| k.starts_with("weather protection") && *k != "weather protection amount").map(|(_, v)| *v).filter(|v| (1..=5).contains(v)).collect();
    kinds.sort();
    kinds.dedup();
    if !kinds.is_empty() {
        l.line("[Weather Protections]", "", "c");
        let amount = r.floats.get("weather protection amount").copied().unwrap_or(0.0);
        for k in kinds {
            let name = ["", "Duststorms", "Acid", "Burning", "Gas", "Rain"][k as usize];
            l.stat(name, format!("{}%", (amount * 100.0) as i32), "#a8b774");
        }
    }
}

/// F044595: an armour piece's coverage ("[Coverage]" and "-{part} {n}%", else "[No Armour
/// Coverage]" in #444444), its skill effects ("[Skills]", left out with none of them), a line,
/// its description; one the player's smith made then a line, "[Crafted by {1}]" and "Crafted by
/// the smith {1} of {2}." (quality under 70) / "High quality armour crafted by the renowned smith
/// {1} of {2}." ({2}: the player's faction's name).
fn armour_about(l: &mut Lines, game: &Game, item: &Item, r: &GameData) {
    let data = &game.sim.data;
    let parts = r.refs("part coverage");
    if parts.is_empty() {
        l.line("[No Armour Coverage]", "", GREY);
    } else {
        l.line("[Coverage]", "", "c");
    }
    for (sid, v) in parts {
        if let Some(p) = data.get(sid) {
            l.stat(&p.name, format!("{}%", v[0]), DARK);
        }
    }
    let before = l.0.len();
    l.line("[Skills]", "", "c");
    if let Some(m) = kenshi_combat::armour::piece_mods(r, item.quality * 0.01) {
        l.effect("Stealth effect", m.stealth);
        l.effect("Crossbows effect", m.ranged);
        l.effect("Dodge skill effect", m.dodge);
        if 1.0 < m.athletics || m.athletics < 0.97 {
            l.effect("Athletics effect", m.athletics);
        }
        l.effect("Combat speed effect", m.combat_speed);
        l.effect("Dexterity effect", m.dexterity);
        l.effect("Attack damage effect", m.damage_output);
        l.effect("Assassination skill", m.assassination);
        if m.fist_injury < 1.0 {
            l.stat("Fist protection", format!("{}%", g6((1.0 - m.fist_injury) * 100.0)), "g");
        }
        if m.attack != 0 {
            l.signed(if m.attack == m.defence { "Melee attack/defence bonus" } else { "Melee attack bonus" }, m.attack as f32, false);
        }
        if m.defence != 0 && m.defence != m.attack {
            l.signed("Melee defence bonus", m.defence as f32, false);
        }
        if m.unarmed != 0 {
            l.signed("Martial arts bonus", m.unarmed as f32, false);
        }
        if m.perception != 0 {
            l.signed("Perception bonus", m.perception as f32, false);
        }
    }
    if l.0.len() == before + 1 {
        l.0.pop();
    }
    l.blank();
    l.description(r, DARK);
    if let Some(smith) = item.crafter.as_ref().filter(|c| !c.is_empty()) {
        l.blank();
        l.line(&format!("[Crafted by {smith}]"), "", "c");
        let of = game.sim.player_faction_name();
        let text = if (item.quality as i32) < 70 {
            format!("Crafted by the smith {smith} of {of}.")
        } else {
            format!("High quality armour crafted by the renowned smith {smith} of {of}.")
        };
        l.line(&text, "", DARK);
    }
}

/// F044610: a backpack's weight and value, then (under "------------") what it does to its
/// wearer: "-Encumbrance reduction" (#a8b774, its value #492620, always), "-Combat skills
/// bonus", and its effects as "{n:.2}x" (F044578: #afa68b at 1, green over, red under).
/// The effects' labels are #afa68b.
fn bag(l: &mut Lines, game: &Game, item: &Item, r: &GameData, price: Option<i32>, holder: Option<usize>) {
    if item.contents.as_ref().is_some_and(|c| c.sections.iter().all(|s| s.items.is_empty())) {
        l.0[0] = format!("{} (empty)\t\tt", item.shown_name(&game.sim.data));
    }
    buyback(l, item);
    stolen(l, game, item, holder);
    l.stat("Weight", format!("{} kg", item.weight as i32), DARK);
    value(l, game, item, price);
    l.0.push("------------\t\t".into());
    let enc = num(r, "encumbrance effect");
    l.line("-Encumbrance reduction", format!("{}%", ((1.0 - enc) * 100.0) as i32), "#a8b774/#492620");
    let bonus = num(r, "combat skill bonus") as i32;
    if bonus != 0 {
        l.bonus("Combat skills bonus", "Combat skills bonus", bonus);
    }
    let mult = |k: &str| r.floats.get(k).copied().unwrap_or(1.0);
    let effect = |l: &mut Lines, label: &str, v: f32| {
        l.stat(label, format!("{v:.2}x"), if v == 1.0 { "c" } else if v > 1.0 { "#afa68b/#a8b774" } else { "#afa68b/#59231a" });
    };
    effect(l, "Athletics effect", mult("athletics mult"));
    effect(l, "Combat speed effect", mult("combat speed mult"));
    effect(l, "Stealth skill effect", mult("stealth mult"));
    // Kenshi's dodge line reads the stealth multiplier again (+0x248)
    effect(l, "Dodge skill effect", mult("stealth mult"));
}

/// Kenshi's red, #59231a ("Buyback", "Stolen", the uniform's faction).
const RED: &str = "#59231a";

/// @fn F044611
///
/// A blueprint's learning line: known, "Research already known" (#492620); else, its "level"
/// above the best research bench built (F046245), "Needs research bench level {n}" (#bf4834);
/// else, its requirements all known (F046260), "Right-click to learn tech" (#492620), or "Cannot
/// learn this yet" (#bf4834).
fn blueprint(l: &mut Lines, game: &Game, tech: &GameData) {
    let sim = &game.sim;
    if sim.research_done.contains(&tech.string_id) {
        l.line("Research already known", "", "t");
        return;
    }
    let level = num(tech, "level") as i32;
    if level > sim.bench_level() {
        l.line(&format!("Needs research bench level {level}"), "", "#bf4834");
    } else if tech.refs("requirements").iter().all(|(sid, _)| sim.data.get(sid).is_none_or(|r| r.item_type() != Some(ItemType::Research)) || sim.research_done.contains(sid)) {
        l.line("Right-click to learn tech", "", "t");
    } else {
        l.line("Cannot learn this yet", "", "#bf4834");
    }
}

/// @fn F044561
///
/// After a blueprint's value: "Requirements:" with its first requirement on the right, the
/// rest on a line of their own (", " between them; each in #bf4834 until known, then
/// #777777), then a line for each building, weapon class, weapon grade, armour and item it
/// brings ("New Building:" .. "New Items:", the name in #a8b774, F044561).
fn requirements(l: &mut Lines, game: &Game, tech: &GameData) {
    let sim = &game.sim;
    let reqs = tech.refs("requirements");
    let (mut first, mut rest) = (String::new(), String::new());
    for (i, (sid, _)) in reqs.iter().enumerate() {
        let Some(r) = sim.data.get(sid).filter(|r| r.item_type() == Some(ItemType::Research)) else { continue };
        let colour = if sim.research_done.contains(sid) { "#777777" } else { "#bf4834" };
        let to = if i < 1 { &mut first } else { &mut rest };
        to.push_str(&format!("{colour}{}", r.name));
        if i + 1 < reqs.len() {
            to.push_str(", ");
        }
    }
    l.0.push(format!("Requirements:\t{first}\t"));
    if !rest.is_empty() {
        l.0.push(format!("\t{rest}\t"));
    }
    for (key, label) in [("enable buildings", "New Building:"), ("enable weapon type", "New Weapon Class:"), ("enable weapon model", "New Weapons Grade:"), ("enable armour", "New Armour Type:"), ("enable item", "New Items:")] {
        for (sid, _) in tech.refs(key) {
            if let Some(r) = sim.data.get(sid) {
                l.0.push(format!("{label}\t#a8b774{}\t", r.name));
            }
        }
    }
}

/// F022540: a robot limb's stat multiplier at `q` (0..100) from its worst `lo` to its best
/// `hi`: straight across when both are on one side of 1; else from `lo` at 5 to 1 at 40, then
/// from 1 at 40 to `hi` at 95.
fn limb_curve(q: i32, lo: f32, hi: f32) -> f32 {
    let lerp = |t: f32, a: f32, b: f32| a + (b - a) * t;
    let inv = |x: f32, a: f32, b: f32| (x - a) / (b - a);
    if hi < 1.0 || lo >= 1.0 {
        return lerp(q as f32 * 0.01, lo, hi);
    }
    if q < 40 {
        lerp(inv(q as f32, 5.0, 40.0).clamp(0.0, 1.0), lo, if 1.0 > hi { hi } else { 1.0 })
    } else {
        lerp(inv(q as f32, 40.0, 95.0).clamp(0.0, 1.0), if 1.0 > hi { hi } else { 1.0 }, hi)
    }
}

/// F047078: a stat's name (StatsEnumerated) as the tooltips write it.
fn stat_label(stat: i32) -> &'static str {
    match stat {
        1 => "Strength",
        2 => "Attack",
        3 => "Labouring",
        4 => "Science",
        5 => "Engineering",
        6 => "Robotics",
        7 => "Weapon Smithing",
        8 => "Armour Smithing",
        9 => "First Aid",
        10 => "Thievery",
        11 => "Turrets",
        12 => "Farming",
        13 => "Cooking",
        14 | 15 => "Vet",
        16 => "Stealth",
        17 => "Athletics",
        18 => "Dexterity",
        19 => "Defence",
        20 => "Weaponry",
        21 => "Toughness",
        22 => "Assassination",
        23 => "Swimming",
        24 => "Perception",
        25 => "Katanas",
        26 => "Sabres",
        27 => "Hackers",
        28 => "Heavy weapons",
        29 => "Blunt",
        30 => "Martial arts",
        32 => "Dodging",
        35 => "Crossbows",
        36 => "Precision Shooting",
        37 => "Lockpicking",
        38 => "Crossbow Smithing",
        64 => "Encumbrance",
        65 => "Combat Speed",
        _ => "",
    }
}

/// F047081 with F043426's colour: "+n" in #a8b774, "n" under 0 in #59231a, "0" in #140806.
fn signed_colour(n: i32) -> &'static str {
    if n < 0 {
        "#59231a"
    } else if n > 0 {
        "#a8b774"
    } else {
        "#140806"
    }
}

/// F044609 with RobotLimbItem's figures (F022544): "-Max HP:" (lerp(quality, "HP", "HP 1")
/// to the nearest 5, F050516), "-----STATS-----" (#492620), each stat multiplier not 1 as
/// "+n%" ((mult - 1) x 100, labels #afa68b): swimming (lerp(quality, "swimming mult", "...
/// 1")), then by F022540's curve a leg's (slot 52 / 53) athletics and stealth, an arm's strength,
/// dexterity, thievery, lockpicking (the record's thievery multipliers again) and crossbows
/// ("ranged mult"); then "-Unarmed damage bonus" (int(lerp(quality, "unarmed damage bonus",
/// "... 1"))) when not 0. (assumption) The stats in the order Kenshi's hash map keeps them, the
/// last added first.
fn limb(l: &mut Lines, item: &Item, r: &GameData) {
    let int = |k: &str| r.ints.get(k).copied().map(|v| v as f32).or_else(|| r.floats.get(k).copied()).unwrap_or(0.0);
    let float = |k: &str| r.floats.get(k).copied().or_else(|| r.ints.get(k).map(|v| *v as f32)).unwrap_or(0.0);
    let lerp = |t: f32, a: f32, b: f32| a + (b - a) * t;
    let q = item.quality * 0.01;
    let hp = lerp(q, int("HP"), int("HP 1"));
    let hp = ((5.0 * 0.5 + hp as f64) / 5.0) as f32;
    let hp = hp.floor() * 5.0;
    l.stat("Max HP:", g6(hp), "c");
    l.line("-----STATS-----", "", "t");
    let qi = item.quality as i32;
    let mut stats: Vec<(i32, f32)> = vec![(23, lerp(q, float("swimming mult"), float("swimming mult 1")))];
    let slot = r.ints.get("slot").copied().unwrap_or(0);
    let curve = |k: &str| limb_curve(qi, float(k), float(&format!("{k} 1")));
    if slot == 0x34 || slot == 0x35 {
        stats.push((17, curve("athletics mult")));
        stats.push((16, curve("stealth mult")));
    } else {
        stats.push((1, curve("strength mult")));
        stats.push((18, curve("dexterity mult")));
        stats.push((10, curve("thievery mult")));
        stats.push((37, curve("thievery mult")));
        stats.push((35, curve("ranged mult")));
    }
    for (stat, m) in stats.into_iter().rev() {
        if m != 1.0 {
            let n = ((m - 1.0) * 100.0) as i32;
            let shown = if n < 1 { format!("{n}%") } else { format!("+{n}%") };
            l.stat(stat_label(stat), shown, &format!("#afa68b/{}", signed_colour(n)));
        }
    }
    let unarmed = lerp(q, int("unarmed damage bonus"), int("unarmed damage bonus 1")) as i32;
    if unarmed as f32 != 0.0 {
        let shown = if unarmed < 1 { unarmed.to_string() } else { format!("+{unarmed}") };
        l.stat("Unarmed damage bonus", shown, &format!("#140806/{}", signed_colour(unarmed)));
    }
}

