//! Saving and loading (port_map `saves`).
//!
//! Saves are named, each a folder under %APPDATA%\kenshi-rewrite\saves (Kenshi's save
//! panels: Save, Load, Import; the quicksave and three autosaves among them): the rewrite's
//! whole state in `rewrite.ron` (the game time, the player's money and squad, each character's
//! place, stats, body, hunger, kit, look and bounties, the player's buildings with their
//! storage, research, and the world below), and the same game in Kenshi's own layout beside it
//! (crate::kenshi_save). Loading reads the rewrite's file; Kenshi's own saves are imported.
//!
//! The whole world's people are kept as Kenshi keeps its platoons: everyone loaded (townsfolk,
//! roaming squads, prisoners and slaves, the dead lying where they fell), each with their
//! place, squad, AI goals, stats, body, kit, shop, bounties, cage, shackles and master; with
//! them the zones loaded in their order, the world's squads and towns, the faction relations,
//! the roaming squads, the nests, what slaves made, the doors' locks and who has said what.
//! Loading builds the world again with the save's zones in the same order (so every building,
//! piece of furniture and node keeps its number) without their people, then puts back the
//! saved people under the same numbers. (assumption) What someone was doing at the moment of
//! saving (a step under way, a fight, an order) starts afresh: their goals choose again.
//! Saves made before people were kept put back the player's squad alone, as before.
use crate::sim::Sim;
use kenshi_char::medical::Medical;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The current save format.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    /// The zone the world was made around.
    pub zone: (i32, i32),
    /// Game seconds run and ticks.
    pub t: f32,
    pub tick: u64,
    pub money: i32,
    pub players: Vec<SavedPerson>,
    /// The player's buildings and what their storage holds (saves before base building have
    /// none).
    #[serde(default)]
    pub buildings: Vec<SavedBuilding>,
    #[serde(default)]
    pub research: SavedResearch,
    /// The world (`world`): the unique characters dead (the towns follow from them) and the
    /// campaigns already run. The squads abroad are kept with `world` (a save from before it
    /// has them made again).
    #[serde(default)]
    pub dead_uniques: Vec<String>,
    #[serde(default)]
    pub campaigns_run: Vec<(String, u32)>,
    /// The towns found (the map's), and those been at (named on it).
    #[serde(default)]
    pub discovered: Vec<String>,
    #[serde(default)]
    pub explored: Vec<String>,
    /// The towns' level ids in their order when saved (`Town::id`): the town numbers the save
    /// holds (people's towns, shops', caravans', squads' and the world's towns) are of this list
    /// (an older save's: one town a record, by record).
    #[serde(default)]
    pub town_ids: Vec<String>,
    /// Items on the ground (`loot`): each as an item (its `section` unused), where it lies
    /// (world units) and its turn; and the towns whose loot has spawned.
    #[serde(default)]
    pub ground: Vec<(SavedItem, [f32; 3], f32)>,
    #[serde(default)]
    pub looted: Vec<String>,
    /// The item spots taken and waiting to fill again (`loot::ItemSpot`): where each is (world
    /// units) and when it fills (game hours).
    #[serde(default)]
    pub spot_dues: Vec<([f32; 3], f64)>,
    /// The containers with something in them: their record, place (world units) and items.
    #[serde(default)]
    pub boxes: Vec<(String, [f32; 3], Vec<SavedItem>)>,
    /// The wilds' nests (`nests`): where each stands and whether wiped out, their camps and
    /// people kept with `people` (a save from before it has them made again when their ground
    /// loads).
    #[serde(default)]
    pub nests: Vec<crate::nests::Nest>,
    /// Everyone in the world, by number (empty in older saves), and the zones in the order
    /// they came in.
    #[serde(default)]
    pub people: Vec<SavedNpc>,
    #[serde(default)]
    pub zone_order: Vec<(i32, i32)>,
    #[serde(default)]
    pub world: Option<SavedWorld>,
}

/// A task of a person's AI goals (`TaskSystem::jobs`): its target only when a person.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTask {
    pub task: u16,
    pub classification: u8,
    pub person: Option<usize>,
    pub priority: f32,
    pub hours: (i32, i32),
}

/// A shopkeeper's shop: its goods, money and restocking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedShop {
    pub stock: Vec<SavedItem>,
    pub money: i32,
    pub restock_at: f64,
    pub template: String,
    pub town: Option<usize>,
}

/// One person of the world, as kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedNpc {
    pub name: String,
    pub character: Option<String>,
    pub race: Option<String>,
    pub faction: Option<String>,
    pub package: Option<String>,
    pub dialogue: Option<String>,
    pub role: u8,
    pub player: bool,
    pub platoon: Option<usize>,
    pub offset: [f32; 3],
    pub home: Option<usize>,
    pub town: Option<usize>,
    pub destination: Option<usize>,
    #[serde(default = "grown")]
    pub age: f32,
    /// Internal units (the mover's).
    pub position: [f32; 3],
    pub facing: [f32; 3],
    pub skills: Vec<(i32, f32)>,
    pub medical: Medical,
    pub down: bool,
    pub dead: bool,
    pub inventory: Vec<SavedItem>,
    pub limbs: Vec<Option<SavedItem>>,
    pub jobs: Vec<Vec<SavedTask>>,
    pub attacks_neutrals: bool,
    pub captures: bool,
    #[serde(default)]
    pub duties: u8,
    /// A wandering trader's trip: target town, hours to stay, state, when it arrived.
    pub caravan: Option<(Option<usize>, f32, u8, f64)>,
    pub shop: Option<SavedShop>,
    pub bounties: Vec<crate::law::Bounty>,
    pub prison: Option<crate::law::Prison>,
    pub shackled: bool,
    pub shaved: bool,
    pub escaped: Option<(String, f64)>,
    pub slave: Option<crate::slavery::Slavery>,
    pub arrest: Option<crate::law::Arrest>,
    pub surrendered: bool,
    pub escort: Option<(usize, bool)>,
    pub carrying: Option<usize>,
    pub carried_by: Option<usize>,
    pub world_squad: Option<usize>,
    pub away: bool,
    pub eat_timer: f32,
    #[serde(default)]
    pub shopping: Option<crate::shopping::ShoppingList>,
    #[serde(default)]
    pub disguise_blown: f32,
    #[serde(default)]
    pub female: Option<bool>,
    #[serde(default)]
    pub personality: i32,
    #[serde(default)]
    pub look_race: Option<String>,
}

/// The world beyond the people: its squads and towns, the next 6-hourly check, the faction
/// relations (faction, default, [(other, relation, forced ally)]), the roaming squads (members,
/// zone, template), what slaves made at the machines (furniture, part-made, storage), the doors
/// locked, and the dialogue memory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedWorld {
    pub squads: Vec<crate::world::WorldSquad>,
    pub towns: Vec<crate::world::TownState>,
    pub next_check: f64,
    pub relations: Vec<(String, f32, Vec<(String, f32, bool)>)>,
    pub roaming: Vec<(Vec<usize>, (i32, i32), String)>,
    /// The roaming squads' towns (by first member): the town list each came from, and its home
    /// (a town, or a nest when the flag is set).
    #[serde(default)]
    pub roaming_from: Vec<(usize, usize)>,
    #[serde(default)]
    pub roaming_home: Vec<(usize, bool, usize)>,
    pub machine_stock: Vec<(usize, Vec<f32>, Vec<SavedItem>)>,
    pub doors_locked: Vec<bool>,
    /// Each door's movement, how far open and whether broken (crate::doors).
    #[serde(default)]
    pub doors: Vec<(i32, f32, bool)>,
    pub dialog_unlocked: Vec<(usize, String)>,
    pub dialog_locked: Vec<(usize, String)>,
    pub dialog_spoken: Vec<((usize, String), f64)>,
    pub dialog_has: Vec<((usize, String), bool)>,
    #[serde(default)]
    pub dialog_met: Vec<((usize, usize), f64)>,
    #[serde(default)]
    pub dialog_tags: Vec<((usize, usize), u32)>,
    /// The player's faction ranks and reputations (crate::dialogue::DialogMemory).
    #[serde(default)]
    pub ranks: Vec<(String, i32)>,
    #[serde(default)]
    pub reputation: Vec<(String, f32)>,
    #[serde(default)]
    pub event_count: Vec<((usize, i32), u32)>,
    #[serde(default)]
    pub contracts: Vec<crate::dialogue::Contract>,
    /// Bounties paid out (crate::dialogue::DialogMemory::claimed), campaigns a dialogue
    /// triggered (due when) and by whom, campaigns locked (crate::world::World).
    #[serde(default)]
    pub claimed: Vec<(usize, String)>,
    #[serde(default)]
    pub triggered: Vec<(String, f64)>,
    #[serde(default)]
    pub trigger_side: Vec<(String, String)>,
    /// The war managers' scheduled campaigns (crate::world::World::requests).
    #[serde(default)]
    pub requests: Vec<crate::world::Request>,
    #[serde(default)]
    pub campaigns_locked: Vec<String>,
    /// The uniques whose death the player was involved in (crate::world::World::involved).
    #[serde(default)]
    pub involved: Vec<String>,
    /// The running campaigns' phases (crate::world::World::phases).
    #[serde(default)]
    pub phases: Vec<(String, crate::world::Phase)>,
    /// The factions' special squads (FactionUniqueSquadManager, F030228 / F030229).
    #[serde(default)]
    pub special: Vec<crate::world::SpecialSquads>,
    /// The Kenshi save this game was imported from: the base its saves in Kenshi's layout are
    /// written over (crate::kenshi_save_over).
    #[serde(default)]
    pub kenshi_base: Option<String>,
    /// The factions the player's faction knows (crate::world::World::known_factions).
    #[serde(default)]
    pub known_factions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedBuilding {
    pub building: crate::base::PlayerBuilding,
    pub storage: Vec<SavedItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedResearch {
    pub done: Vec<String>,
    pub queue: Vec<String>,
    pub progress: f32,
    pub paid: bool,
    /// The base's town panel settings (its name, open to the public), kept with its research.
    #[serde(default)]
    pub outpost_name: String,
    #[serde(default)]
    pub outpost_public: bool,
    #[serde(default)]
    pub ai_options: crate::sim::AiOptions,
    #[serde(default)]
    pub advanced: crate::advanced::Advanced,    /// The player's squads (their platoons and names) and the faction's own name.
    #[serde(default)]
    pub player_squads: Vec<crate::squads::PlayerSquad>,
    #[serde(default)]
    pub faction_name: String,
}

/// An inventory's items where they lie.
fn saved_items(inv: &kenshi_char::inventory::Inventory) -> Vec<SavedItem> {
    let mut out = Vec::new();
    for (s, sec) in inv.sections.iter().enumerate() {
        for it in &sec.items {
            out.push(SavedItem {
                section: s,
                record: it.item.record.clone(),
                quantity: it.item.quantity,
                x: it.x,
                y: it.y,
                food_left: it.item.food.or(it.item.ammo).map(|(_, l)| l),
                quality: Some(it.item.quality),
                maker: it.item.maker.clone(),
                model: it.item.model.clone(),
                uniform: it.item.uniform.clone(),
                owner: it.item.owner.clone(),
                crafter: it.item.crafter.clone(),
                unique: it.item.unique,
                contents: it.item.contents.as_ref().map(saved_items).unwrap_or_default(),
            });
        }
    }
    out
}

/// One item as kept (its place unused).
fn saved_item(item: &kenshi_char::inventory::Item) -> SavedItem {
    SavedItem {
        section: 0,
        record: item.record.clone(),
        quantity: item.quantity,
        x: 0,
        y: 0,
        food_left: item.food.or(item.ammo).map(|(_, l)| l),
        quality: Some(item.quality),
        maker: item.maker.clone(),
        model: item.model.clone(),
        uniform: item.uniform.clone(),
        owner: item.owner.clone(),
        crafter: item.crafter.clone(),
        unique: item.unique,
        contents: item.contents.as_ref().map(saved_items).unwrap_or_default(),
    }
}

/// A kept item made again.
fn item_of(data: &kenshi_data::GameDataContainer, si: &SavedItem) -> Option<kenshi_char::inventory::Item> {
    let r = data.get(&si.record)?;
    let mut item = kenshi_char::inventory::Item::from_record(r, si.quantity);
    if let (Some((full, _)), Some(left)) = (item.food, si.food_left) {
        item.food = Some((full, left));
    }
    if let (Some((full, _)), Some(left)) = (item.ammo, si.food_left) {
        item.ammo = Some((full, left));
    }
    let mut item = with_grade(data, item, si);
    if let Some(c) = item.contents.as_mut() {
        restore_items(data, c, &si.contents);
    }
    Some(item)
}

/// Saved items back into `inv`, each where it lay (else anywhere it fits).
/// A saved item's quality, maker and model back on it (its value made again).
fn with_grade(data: &kenshi_data::GameDataContainer, mut item: kenshi_char::inventory::Item, si: &SavedItem) -> kenshi_char::inventory::Item {
    item.maker = si.maker.clone();
    item.model = si.model.clone();
    item.uniform = si.uniform.clone();
    item.owner = si.owner.clone();
    item.crafter = si.crafter.clone();
    item.unique = si.unique;
    match si.quality {
        Some(q) => item.with_quality(data, q),
        None => item,
    }
}

pub(crate) fn restore_items(data: &kenshi_data::GameDataContainer, inv: &mut kenshi_char::inventory::Inventory, items: &[SavedItem]) {
    for si in items {
        let Some(r) = data.get(&si.record) else { continue };
        let mut item = kenshi_char::inventory::Item::from_record(r, si.quantity);
        if let (Some((full, _)), Some(left)) = (item.food, si.food_left) {
            item.food = Some((full, left));
        }
        if let (Some((full, _)), Some(left)) = (item.ammo, si.food_left) {
            item.ammo = Some((full, left));
        }
        let mut item = with_grade(data, item, si);
        // a backpack's contents
        if let Some(c) = item.contents.as_mut() {
            restore_items(data, c, &si.contents);
        }
        if let Err(item) = inv.put(si.section, item, si.x, si.y) {
            let _ = inv.add(item, false);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPerson {
    pub name: String,
    /// Internal units (the mover's).
    pub position: [f32; 3],
    pub facing: [f32; 3],
    pub character: Option<String>,
    /// (stat id, level).
    pub skills: Vec<(i32, f32)>,
    pub medical: Medical,
    pub down: bool,
    pub dead: bool,
    pub inventory: Vec<SavedItem>,
    /// Its look (race, head, hair, clothes), when made in the editor.
    pub look: Option<kenshi_render::character::CharacterSpec>,
    /// Its bounties (`law`).
    #[serde(default)]
    pub bounties: Vec<crate::law::Bounty>,
    /// Its combat stance and hold point (`kenshi_char::stance`).
    #[serde(default)]
    pub stance: kenshi_char::stance::Stance,
    #[serde(default)]
    pub hold_at: Option<[f32; 3]>,
    /// Its job list and the JOBS toggle.
    #[serde(default)]
    pub jobs: Vec<crate::orders::Job>,
    #[serde(default)]
    pub jobs_on: bool,
}

/// An item where it lies: section index, record, how many, cell, and the food (or ammo charges)
/// left in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedItem {
    pub section: usize,
    pub record: String,
    pub quantity: u32,
    pub x: u32,
    pub y: u32,
    pub food_left: Option<f32>,
    /// Its quality and a weapon's maker and model (`kenshi_char::quality`).
    #[serde(default)]
    pub quality: Option<f32>,
    #[serde(default)]
    pub maker: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// A faction uniform's faction (crate::disguise).
    #[serde(default)]
    pub uniform: Option<String>,
    /// Its owners when not its holder's (loot, F044581).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The smith who made it (Kenshi's "crafter", F043905 / F043906).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crafter: Option<String>,
    /// One of a kind (Kenshi's "unique").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unique: bool,
    /// A backpack's contents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<SavedItem>,
}

/// Where saves go: %APPDATA%\kenshi-rewrite\saves (else ./saves).
pub fn dir() -> PathBuf {
    std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("saves"), |a| PathBuf::from(a).join("kenshi-rewrite").join("saves"))
}

pub fn quicksave_path() -> PathBuf {
    dir().join("quicksave.ron")
}

pub fn write(save: &SaveGame, path: &std::path::Path) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let text = ron::ser::to_string(save).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

pub fn read(path: &std::path::Path) -> std::io::Result<SaveGame> {
    let text = std::fs::read_to_string(path)?;
    let save: SaveGame = ron::from_str(&text).map_err(std::io::Error::other)?;
    if save.version != VERSION {
        return Err(std::io::Error::other(format!("save version {} (this build reads {VERSION})", save.version)));
    }
    Ok(save)
}

/// A save of the player's: a folder under `dir()` named for it, holding `rewrite.ron` (this
/// game's whole state) and the same game in Kenshi's own layout (`quick.save`, `platoon/`;
/// crate::kenshi_save); or the single `quicksave.ron` older builds wrote. Kenshi's own saves
/// (%LOCALAPPDATA%\kenshi\save) are listed beside them to import, and never written.
#[derive(Debug, Clone)]
pub struct Slot {
    pub name: String,
    pub path: PathBuf,
    /// When it was saved (its files' time).
    pub when: std::time::SystemTime,
    /// One of Kenshi's own (only its Kenshi files to read).
    pub kenshi: bool,
}

/// The quicksave's name (Kenshi's F5 save).
pub const QUICKSAVE: &str = "quicksave";
/// The autosaves' name, then 0, 1, 2 in turn (F034847: "autosave" + ("autosaveindex" + 1) % 3).
pub const AUTOSAVE: &str = "autosave";
const STATE_FILE: &str = "rewrite.ron";

/// A save name with what a folder name can't hold taken out (F034833 drops such characters as
/// they are typed; (assumption) the characters Windows forbids).
pub fn clean_name(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()).collect::<String>().trim().to_string()
}

fn modified(p: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH)
}

/// The player's saves, newest first (Kenshi's list sorts by its Date column, F034863).
pub fn slots() -> Vec<Slot> {
    let mut out: Vec<Slot> = std::fs::read_dir(dir())
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().join(STATE_FILE).is_file())
                .map(|e| Slot { name: e.file_name().to_string_lossy().into_owned(), when: modified(&e.path().join(STATE_FILE)), path: e.path(), kenshi: false })
                .collect()
        })
        .unwrap_or_default();
    // an older build's quicksave, until one is saved in its place
    let old = quicksave_path();
    if old.is_file() && !out.iter().any(|s| s.name == QUICKSAVE) {
        out.push(Slot { name: QUICKSAVE.into(), when: modified(&old), path: old, kenshi: false });
    }
    out.sort_by(|a, b| b.when.cmp(&a.when));
    out
}

/// Kenshi's own saves (its save folder, read only), newest first.
pub fn kenshi_slots() -> Vec<Slot> {
    let Some(root) = std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("kenshi").join("save")) else { return Vec::new() };
    let mut out: Vec<Slot> = std::fs::read_dir(root)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().join("quick.save").is_file())
                .map(|e| Slot { name: e.file_name().to_string_lossy().into_owned(), when: modified(&e.path().join("quick.save")), path: e.path(), kenshi: true })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.when.cmp(&a.when));
    out
}

/// A save's name from its path (its folder's, or an older build's file without ".ron").
pub fn slot_name(path: &std::path::Path) -> String {
    let n = if path.is_dir() { path.file_name() } else { path.file_stem() };
    n.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The folder save `name` goes in.
pub fn slot_dir(name: &str) -> PathBuf {
    dir().join(name)
}

/// Save `name`: the game's state, and the same in Kenshi's layout.
pub fn write_slot(name: &str, save: &SaveGame, kenshi: &crate::kenshi_save::KenshiSave) -> std::io::Result<PathBuf> {
    let d = slot_dir(name);
    // the Kenshi files written afresh (no squads left from before)
    let _ = std::fs::remove_dir_all(d.join("platoon"));
    let _ = std::fs::remove_dir_all(d.join("zone"));
    std::fs::create_dir_all(&d)?;
    write(save, &d.join(STATE_FILE))?;
    // in Kenshi's layout: over the Kenshi save the game was imported from, else over the newest
    // of Kenshi's own (read only), else from scratch
    let base = save
        .world
        .as_ref()
        .and_then(|w| w.kenshi_base.clone())
        .map(PathBuf::from)
        .filter(|b| b.join("quick.save").is_file())
        .or_else(|| kenshi_slots().into_iter().next().map(|s| s.path));
    match base {
        Some(b) => crate::kenshi_save_over::write_over(&b, &d, kenshi).or_else(|e| {
            eprintln!("can't write over {}: {e}; written from scratch", b.display());
            crate::kenshi_save::write(&d, kenshi)
        })?,
        None => crate::kenshi_save::write(&d, kenshi)?,
    }
    Ok(d)
}

/// A save of ours read (a folder, or an older build's single file).
pub fn read_slot(path: &std::path::Path) -> std::io::Result<SaveGame> {
    if path.is_dir() { read(&path.join(STATE_FILE)) } else { read(path) }
}

/// A save of ours deleted (Kenshi's list: Delete with the save chosen, F034862). Kenshi's own
/// are never touched.
pub fn delete_slot(slot: &Slot) -> std::io::Result<()> {
    if slot.kenshi || !slot.path.starts_with(dir()) {
        return Err(std::io::Error::other("not one of this game's saves"));
    }
    if slot.path.is_dir() { std::fs::remove_dir_all(&slot.path) } else { std::fs::remove_file(&slot.path) }
}

/// The save Continue loads: the one last saved or loaded (settings.cfg "continue"), else the
/// newest.
pub fn continue_slot(last: Option<&str>) -> Option<Slot> {
    let all = slots();
    last.and_then(|n| all.iter().find(|s| s.name == n).cloned()).or_else(|| all.into_iter().next())
}

/// The next autosave's name after the one numbered `index` (0, 1, 2 in turn).
pub fn next_autosave(index: i32) -> (String, i32) {
    let i = (index + 1).rem_euclid(3);
    (format!("{AUTOSAVE}{i}"), i)
}

impl Sim {
    /// The state a save keeps. `look` gives a player character's look, if it has one.
    pub fn snapshot(&self, zone: (i32, i32), look: impl Fn(usize) -> Option<kenshi_render::character::CharacterSpec>) -> SaveGame {
        let players = self
            .folk
            .iter()
            .filter(|p| p.player)
            .map(|p| {
                let f = &self.arena.f[p.id];
                let inventory = saved_items(&p.inventory);
                SavedPerson {
                    name: p.name.clone(),
                    position: p.mover.position,
                    facing: p.facing,
                    character: p.character.clone(),
                    skills: f.skills.levels.iter().map(|(s, v)| (*s as i32, *v)).collect(),
                    medical: f.medical.clone(),
                    down: p.down,
                    dead: p.dead,
                    inventory,
                    look: look(p.id),
                    bounties: p.bounties.clone(),
                    stance: p.stance,
                    hold_at: p.hold_at,
                    jobs: p.jobs.clone(),
                    jobs_on: p.jobs_on,
                }
            })
            .collect();
        let buildings = self
            .buildings
            .iter()
            .filter(|b| b.confirmed)
            .map(|b| SavedBuilding { building: b.clone(), storage: b.storage.as_ref().map(saved_items).unwrap_or_default() })
            .collect();
        let research = SavedResearch {
            done: self.research_done.clone(),
            queue: self.research_queue.clone(),
            progress: self.research_progress,
            paid: self.research_paid,
            outpost_name: self.outpost_name.clone(),
            outpost_public: self.outpost_public,
            ai_options: self.ai_options.clone(),
            advanced: self.advanced.clone(),
            player_squads: self.player_squads.clone(),
            faction_name: self.faction_name.clone(),
        };
        let mut dead_uniques: Vec<String> = self.world.dead_uniques.iter().cloned().collect();
        dead_uniques.sort();
        let campaigns_run = self.world.campaigns_run.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let mut discovered: Vec<String> = self.world.discovered.iter().cloned().collect();
        discovered.sort();
        let mut explored: Vec<String> = self.world.explored.iter().cloned().collect();
        explored.sort();
        let ground = self
            .ground
            .iter()
            .map(|g| (saved_item(&g.item), g.at, g.yaw))
            .collect();
        let boxes = self
            .boxes
            .iter()
            .filter(|b| b.inventory.sections.iter().any(|s| !s.items.is_empty()))
            .map(|b| (b.at.record.clone(), b.at.position, saved_items(&b.inventory)))
            .collect();
        let mut looted: Vec<String> = self.looted.iter().cloned().collect();
        looted.sort();
        let spot_dues = self.spots.iter().filter(|s| s.item.is_none() && s.due.is_finite() && s.due > 0.0).map(|s| (s.at, s.due)).collect();

        // with the people kept, the nests keep their camps and people too
        let nests = self.nests.clone();
        let people = (0..self.folk.len()).map(|id| self.saved_npc(id)).collect();
        SaveGame {
            version: VERSION,
            zone,
            t: self.t,
            tick: self.tick,
            money: self.money,
            players,
            buildings,
            research,
            dead_uniques,
            campaigns_run,
            discovered,
            explored,
            town_ids: self.towns.iter().map(|t| t.id.clone()).collect(),
            ground,
            looted,
            spot_dues,
            boxes,
            nests,
            people,
            zone_order: self.zone_order.clone(),
            world: Some(self.saved_world()),
        }
    }

    /// Person `id` as kept.
    fn saved_npc(&self, id: usize) -> SavedNpc {
        let p = &self.folk[id];
        let f = &self.arena.f[id];
        let jobs = p
            .ai
            .jobs
            .iter()
            .map(|l| {
                l.iter()
                    .map(|t| SavedTask {
                        task: t.task,
                        classification: t.classification,
                        person: match t.target {
                            crate::sim::Target::Person(q) => Some(q),
                            _ => None,
                        },
                        priority: t.priority,
                        hours: t.hours,
                    })
                    .collect()
            })
            .collect();
        SavedNpc {
            name: p.name.clone(),
            character: p.character.clone(),
            race: p.race.clone(),
            faction: p.faction.clone(),
            package: p.package.clone(),
            dialogue: p.dialogue.clone(),
            role: p.role,
            player: p.player,
            platoon: p.platoon,
            offset: p.offset,
            home: p.home,
            age: p.age,
            town: p.town,
            destination: p.destination,
            position: p.mover.position,
            facing: p.facing,
            skills: f.skills.levels.iter().map(|(s, v)| (*s as i32, *v)).collect(),
            medical: f.medical.clone(),
            down: p.down,
            dead: p.dead,
            inventory: saved_items(&p.inventory),
            limbs: p.limbs.iter().map(|l| l.as_ref().map(saved_item)).collect(),
            jobs,
            attacks_neutrals: p.attacks_neutrals,
            captures: p.captures,
            duties: p.duties,
            caravan: p.caravan.as_ref().map(|c| (c.target, c.stay, c.state, c.arrived)),
            shop: p.shop.as_ref().map(|s| SavedShop { stock: saved_items(&s.stock), money: s.money, restock_at: s.restock_at, template: s.template.clone(), town: s.town }),
            bounties: p.bounties.clone(),
            prison: p.prison.clone(),
            shackled: p.shackled,
            shaved: p.shaved,
            escaped: p.escaped.clone(),
            slave: p.slave.clone().map(|mut s| {
                s.machine = None;
                s
            }),
            arrest: p.arrest.clone(),
            surrendered: p.surrendered,
            escort: p.escort,
            carrying: p.carrying,
            carried_by: p.carried_by,
            world_squad: p.world_squad,
            away: p.away,
            eat_timer: p.eat_timer,
            shopping: p.shopping.clone(),
            disguise_blown: p.disguise_blown,
            female: p.female,
            personality: p.personality,
            look_race: p.look_race.clone(),
        }
    }

    /// The world beyond the people, as kept.
    fn saved_world(&self) -> SavedWorld {
        // the relations as they have changed from the factions' records (the rest are those)
        let mut relations: Vec<(String, f32, Vec<(String, f32, bool)>)> = self
            .relations
            .values()
            .filter_map(|r| {
                let base = self.data.get(&r.faction).map(|d| kenshi_ai::faction::FactionRelations::from_data(d, &self.data)).unwrap_or_default();
                let mut v: Vec<(String, f32, bool)> =
                    r.relations.iter().filter(|(o, x)| base.relations.get(*o) != Some(*x)).map(|(o, x)| (o.clone(), x.relation, x.forced_ally)).collect();
                v.sort_by(|a, b| a.0.cmp(&b.0));
                (!v.is_empty() || base.default_relation != r.default_relation).then(|| (r.faction.clone(), r.default_relation, v))
            })
            .collect();
        relations.sort_by(|a, b| a.0.cmp(&b.0));
        let mut machine_stock: Vec<(usize, Vec<f32>, Vec<SavedItem>)> = self.machine_stock.iter().map(|(k, (made, inv))| (*k, made.clone(), saved_items(inv))).collect();
        machine_stock.sort_by_key(|m| m.0);
        let m = &self.dialog_memory;
        SavedWorld {
            squads: self.world.squads.clone(),
            towns: self.world.towns.clone(),
            next_check: self.world.next_check,
            relations,
            roaming: self.population.as_ref().map(|p| p.squads.clone()).unwrap_or_default(),
            roaming_from: self.population.as_ref().map(|p| {
                let mut v: Vec<(usize, usize)> = p.homes.iter().map(|(&l, &t)| (l, t)).collect();
                v.sort();
                v
            }).unwrap_or_default(),
            roaming_home: self.population.as_ref().map(|p| {
                let mut v: Vec<(usize, bool, usize)> = p.home_town.iter().map(|(&l, h)| match h {
                    Ok(t) => (l, false, *t),
                    Err(k) => (l, true, *k),
                }).collect();
                v.sort();
                v
            }).unwrap_or_default(),
            machine_stock,
            doors_locked: self.door_states.iter().map(|d| d.locked).collect(),
            doors: self.door_states.iter().map(|d| (d.phase, d.open, d.broken)).collect(),
            dialog_unlocked: m.unlocked.iter().cloned().collect(),
            dialog_locked: m.locked.iter().cloned().collect(),
            dialog_spoken: m.spoken.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            dialog_has: m.has.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            dialog_met: m.met.iter().map(|(k, v)| (*k, *v)).collect(),
            dialog_tags: m.tags.iter().map(|(k, v)| (*k, *v)).collect(),
            ranks: m.ranks.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            reputation: m.reputation.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            event_count: m.event_count.iter().map(|(k, v)| (*k, *v)).collect(),
            contracts: self.contracts.clone(),
            claimed: m.claimed.iter().cloned().collect(),
            triggered: self.world.triggered.clone(),
            trigger_side: self.world.trigger_side.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            requests: self.world.requests.clone(),
            campaigns_locked: self.world.campaigns_locked.iter().cloned().collect(),
            involved: self.world.involved.iter().cloned().collect(),
            phases: self.world.phases.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            special: self.world.special.clone(),
            kenshi_base: self.kenshi_base.clone(),
            known_factions: {
                let mut v: Vec<String> = self.world.known_factions.iter().cloned().collect();
                v.sort();
                v
            },
        }
    }

    /// A save's world squads, towns and relations put back (on the world's first tick, after
    /// it has made its own).
    pub fn apply_saved_world(&mut self, w: SavedWorld) {
        if !w.towns.is_empty() {
            for (t, s) in w.towns.into_iter().enumerate() {
                // (one the save did not know keeps the state the world gave it)
                if s.record.is_empty() || t >= self.world.towns.len() {
                    continue;
                }
                // its "replacementTown" as F031580 loads it (F051428)
                let record = s.record.clone();
                self.world.towns[t] = s;
                if record != self.towns[t].record {
                    self.set_town_record(t, &record);
                }
            }
        }
        self.world.squads = w.squads;
        self.world.next_check = w.next_check;
        self.world.triggered = w.triggered;
        self.world.requests = w.requests;
        self.world.trigger_side = w.trigger_side.into_iter().collect();
        self.world.campaigns_locked = w.campaigns_locked.into_iter().collect();
        self.world.involved = w.involved.into_iter().collect();
        self.world.phases = w.phases.into_iter().collect();
        if !w.special.is_empty() {
            self.world.special = w.special;
        }
        self.world.known_factions.extend(w.known_factions);
        for (faction, default, rels) in w.relations {
            let r = self.relations.entry(faction.clone()).or_insert_with(|| kenshi_ai::faction::FactionRelations { faction, ..Default::default() });
            r.default_relation = default;
            for (o, relation, forced_ally) in rels {
                r.relations.insert(o, kenshi_ai::faction::Relation { forced_ally, relation });
            }
        }
    }

    /// The towns of a save's town numbers: by its level ids, else (an older save, one town a
    /// TOWN record in record order) the first town of that record.
    pub(crate) fn town_remap(&self, ids: &[String]) -> Vec<Option<usize>> {
        if !ids.is_empty() {
            return ids.iter().map(|id| self.towns.iter().position(|t| t.id == *id)).collect();
        }
        let mut records: Vec<&String> = self.towns.iter().map(|t| &t.record).collect();
        records.sort();
        records.dedup();
        records.iter().map(|r| self.towns.iter().position(|t| t.record == **r)).collect()
    }

    /// A save's found and been-at towns as level ids: an older save's records stand for every
    /// town of the record.
    fn town_keys(&self, keys: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for k in keys {
            if self.towns.iter().any(|t| t.id == *k) {
                out.push(k.clone());
            } else {
                out.extend(self.towns.iter().filter(|t| t.record == *k).map(|t| t.id.clone()));
            }
        }
        out
    }

    /// Put a save's state on this world: the time, the money, and the player's characters in
    /// order onto this world's player characters (as many as both have). Returns the
    /// characters restored, with their looks.
    pub fn restore(&mut self, save: &SaveGame) -> Vec<(usize, Option<kenshi_render::character::CharacterSpec>)> {
        if !save.people.is_empty() {
            return self.restore_world(save);
        }
        self.t = save.t;
        self.tick = save.tick;
        self.money = save.money;
        let ids: Vec<usize> = self.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
        let data = self.data.clone();
        // the buildings, their storage made again from their records and refilled
        // the outposts are worked out again from the buildings (base::assign_outposts)
        self.outposts.clear();
        self.buildings = save
            .buildings
            .iter()
            .filter_map(|sb| {
                let rec = data.get(&sb.building.record)?;
                let mut b = sb.building.clone();
                b.storage = crate::base::PlayerBuilding::new(rec, b.at, b.yaw).storage;
                if let Some(inv) = b.storage.as_mut() {
                    restore_items(&data, inv, &sb.storage);
                }
                Some(b)
            })
            .collect();
        self.world.dead_uniques = save.dead_uniques.iter().cloned().collect();
        self.nests = save.nests.clone();
        let (found, been) = (self.town_keys(&save.discovered), self.town_keys(&save.explored));
        self.world.discovered.extend(found);
        self.world.explored.extend(been);
        // the ground's items as they lay; the towns already looted spawn none again
        self.looted = save.looted.iter().cloned().collect();
        // the containers' contents: those loaded now refilled, the others when their zone loads
        self.saved_boxes = save.boxes.clone();
        for b in &mut self.boxes {
            b.inventory.sections.iter_mut().for_each(|s| s.items.clear());
        }
        self.refill_boxes(0);
        // the item spots taken keep waiting (those loaded now, the others as their zone loads)
        self.saved_spot_dues = save.spot_dues.clone();
        self.apply_spot_dues(0);
        self.ground.clear();
        for (si, at, yaw) in &save.ground {
            let Some(r) = self.data.get(&si.record) else { continue };
            let mut item = kenshi_char::inventory::Item::from_record(r, si.quantity);
            if let (Some((full, _)), Some(left)) = (item.food, si.food_left) {
                item.food = Some((full, left));
            }
            if let (Some((full, _)), Some(left)) = (item.ammo, si.food_left) {
                item.ammo = Some((full, left));
            }
            let item = with_grade(&self.data, item, si);
            self.next_ground += 1;
            self.ground.push(crate::loot::GroundItem { id: self.next_ground, item, at: *at, yaw: *yaw });
        }
        self.world.campaigns_run = save.campaigns_run.iter().cloned().collect();
        // the towns as the dead make them (re-read on the world's first tick)
        self.world.started = false;
        self.world.squads.clear();
        self.world.war_ready = false;
        // the old squads' people set aside (the new world makes its own)
        for p in self.folk.iter_mut() {
            if p.world_squad.take().is_some() {
                p.away = true;
            }
        }
        self.research_done = save.research.done.clone();
        self.research_queue = save.research.queue.clone();
        self.research_progress = save.research.progress;
        self.research_paid = save.research.paid;
        if !save.research.outpost_name.is_empty() {
            self.outpost_name = save.research.outpost_name.clone();
        }
        self.outpost_public = save.research.outpost_public;
        self.ai_options = save.research.ai_options.clone();
        self.advanced = save.research.advanced.clone();
        self.arena.sh.damage_mult = self.advanced.damage;
        // (the fresh world's people: their squads made anew) the faction's name
        self.faction_name = save.research.faction_name.clone();
        self.player_squads.clear();
        self.squads_ready();
        for p in self.folk.iter_mut() {
            p.build = None;
            p.operate = None;
        }
        let race = self.data.get(&self.race_record).cloned();
        let mut out = Vec::new();
        for (&id, sp) in ids.iter().zip(&save.players) {
            let p = &mut self.folk[id];
            p.name = sp.name.clone();
            p.mover.position = sp.position;
            p.facing = sp.facing;
            p.character = sp.character.clone();
            p.down = sp.down;
            p.dead = sp.dead;
            p.bounties = sp.bounties.clone();
            p.stance = sp.stance;
            p.hold_at = sp.hold_at;
            p.jobs = sp.jobs.clone();
            p.jobs_on = sp.jobs_on;
            p.ai.queue.clear();
            // the kit, item by item where it lay
            // (an animal's: its sections, its backpack as saved)
            let mut inv = match p.race.as_ref().and(p.character.as_deref()).and_then(|c| data.get(c)) {
                Some(a) => crate::sim::animal_inventory(&data, a, false),
                None => kenshi_char::inventory::Inventory::character(),
            };
            restore_items(&data, &mut inv, &sp.inventory);
            p.inventory = inv;
            let f = &mut self.arena.f[id];
            f.mover.position = sp.position;
            f.medical = sp.medical.clone();
            f.down = sp.down;
            let mut skills = f.skills.clone();
            skills.levels = sp.skills.iter().filter_map(|(s, v)| kenshi_char::StatsEnumerated::from_i32(*s).map(|s| (s, *v))).collect();
            if let Some(race) = &race {
                skills.race_xp = kenshi_char::stats::race_multipliers(race);
                f.set_skills(&data, race, skills);
            }
            self.equip(id);
            out.push((id, sp.look.clone()));
        }
        out
    }
}

impl Sim {
    /// A save with its people put on a world just built at its zone (`Sim::load`): its zones
    /// in their order without their people, then everyone under their own numbers, and the
    /// rest of the save. Returns the player's characters with their looks.
    fn restore_world(&mut self, save: &SaveGame) -> Vec<(usize, Option<kenshi_render::character::CharacterSpec>)> {
        let remap = self.town_remap(&save.town_ids);
        let town = |t: Option<usize>| t.and_then(|k| remap.get(k).copied().flatten());
        // a game load: no town started yet, a save loading (ZoneManager +0x08 till its zones are
        // in, `world::zone_loaded`), none of the zones Kenshi keeps loaded yet
        self.towns_started.clear();
        for t in std::mem::take(&mut self.overridden) {
            let level = self.towns[t].record.clone();
            self.set_town_record(t, &level);
        }
        self.live_zones.clear();
        self.cells = Default::default();
        self.camera_zone = None;
        self.loading = true;
        // the zones, in their order, empty
        self.quiet_zones = true;
        let res = self.res.clone();
        for &(zx, zy) in &save.zone_order {
            self.add_zone(zx, zy, &res);
        }
        self.quiet_zones = false;
        // the world's own people let go: the save's take their numbers
        self.folk.clear();
        self.arena.f.clear();
        self.arena.layers.clear();
        self.arena.bolts.clear();
        self.manager = Default::default();
        self.operators.iter_mut().for_each(|o| *o = None);
        self.owners.iter_mut().for_each(|o| *o = None);
        self.bed_taken.iter_mut().for_each(|o| *o = None);
        self.slave_squads.clear();
        self.prisoner_squads.clear();
        self.start_slaves = false;
        self.met.clear();
        self.runs.clear();
        self.seen_player.clear();
        self.was_down.clear();
        self.looted_by.clear();
        self.npc_talks.clear();
        self.said.clear();
        self.logged.clear();
        self.pending_effects.clear();
        self.pick_events.clear();
        let data = self.data.clone();
        for (i, sp) in save.people.iter().enumerate() {
            let at = sp.position.map(|v| v / kenshi_world::navmesh::METRES);
            let id = self.add_person(at, Vec::new(), None, sp.platoon, sp.faction.clone(), sp.role);
            debug_assert_eq!(id, i);
            if let Some(animal) = sp.race.as_ref().and(sp.character.as_deref()) {
                self.folk[id].age = sp.age;
                self.make_animal(id, animal);
            }
            let p = &mut self.folk[id];
            p.mover.position = sp.position;
            p.facing = sp.facing;
            p.name = sp.name.clone();
            p.character = sp.character.clone();
            p.race = sp.race.clone();
            p.package = sp.package.clone();
            p.dialogue = sp.dialogue.clone();
            p.player = sp.player;
            p.offset = sp.offset;
            p.home = sp.home;
            p.age = sp.age;
            p.town = town(sp.town);
            p.destination = sp.destination;
            p.down = sp.down;
            p.dead = sp.dead;
            p.attacks_neutrals = sp.attacks_neutrals;
            p.captures = sp.captures;
            p.duties = sp.duties;
            p.caravan = sp.caravan.map(|(target, stay, state, arrived)| kenshi_ai::caravan::Caravan { target: town(target), stay, state, arrived });
            p.bounties = sp.bounties.clone();
            p.prison = sp.prison.clone();
            p.shackled = sp.shackled;
            p.shaved = sp.shaved;
            p.escaped = sp.escaped.clone();
            p.slave = sp.slave.clone();
            p.arrest = sp.arrest.clone();
            p.surrendered = sp.surrendered;
            p.escort = sp.escort;
            p.carrying = sp.carrying;
            p.carried_by = sp.carried_by;
            p.world_squad = sp.world_squad;
            p.away = sp.away;
            p.eat_timer = sp.eat_timer;
            p.shopping = sp.shopping.clone();
            p.disguise_blown = sp.disguise_blown;
            p.female = sp.female;
            p.personality = sp.personality;
            p.look_race = sp.look_race.clone();
            for (c, list) in sp.jobs.iter().enumerate().take(4) {
                p.ai.jobs[c] = list
                    .iter()
                    .map(|t| kenshi_ai::task_system::Task {
                        task: t.task,
                        classification: t.classification,
                        target: t.person.map_or(crate::sim::Target::None, crate::sim::Target::Person),
                        target2: crate::sim::Target::None,
                        priority: t.priority,
                        pos: [0.0; 3],
                        hours: t.hours,
                    })
                    .collect();
            }
            // (an animal's: its sections, its backpack as saved)
            let mut inv = match sp.race.as_ref().and(sp.character.as_deref()).and_then(|c| data.get(c)) {
                Some(a) => crate::sim::animal_inventory(&data, a, false),
                None => kenshi_char::inventory::Inventory::character(),
            };
            restore_items(&data, &mut inv, &sp.inventory);
            p.inventory = inv;
            for (k, l) in sp.limbs.iter().enumerate().take(4) {
                p.limbs[k] = l.as_ref().and_then(|si| item_of(&data, si));
            }
            p.shop = sp.shop.as_ref().map(|s| {
                let mut stock = crate::trade::shop_inventory();
                restore_items(&data, &mut stock, &s.stock);
                crate::trade::Shop { stock, money: s.money, restock_at: s.restock_at, template: s.template.clone(), town: town(s.town) }
            });
            // the body: stats of its race, its wounds and its kit
            let race = sp.race.clone().unwrap_or_else(|| self.race_record.clone());
            let f = &mut self.arena.f[id];
            f.mover.position = sp.position;
            f.medical = sp.medical.clone();
            f.down = sp.down;
            if sp.attacks_neutrals {
                f.foes = None;
            }
            if let Some(race) = data.get(&race) {
                let mut skills = f.skills.clone();
                skills.levels = sp.skills.iter().filter_map(|(s, v)| kenshi_char::StatsEnumerated::from_i32(*s).map(|s| (s, *v))).collect();
                skills.race_xp = kenshi_char::stats::race_multipliers(race);
                f.set_skills(&data, race, skills);
            }
            if sp.race.is_none() {
                self.equip(id);
            }
            if sp.limbs.iter().any(Option::is_some) {
                self.limb_mults(id);
            }
        }
        // the squads' bar states (a platoon's number is its place there)
        let top = self.folk.iter().filter_map(|p| p.platoon).max().map_or(0, |m| m + 1);
        self.bar_states = (0..top)
            .map(|pid| {
                let sig = self
                    .folk
                    .iter()
                    .find(|p| p.platoon == Some(pid))
                    .and_then(|p| p.package.as_deref())
                    .and_then(|k| data.get(k))
                    .and_then(|k| k.ints.get("signal func").copied())
                    .unwrap_or(0);
                [17, 26, 27].contains(&sig).then(crate::sim::BarState::default)
            })
            .collect();
        // the world beyond the people
        if let Some(w) = &save.world {
            if let Some(pop) = self.population.as_mut() {
                pop.squads = w.roaming.clone();
                pop.homes = w.roaming_from.iter().filter_map(|&(l, t)| Some((l, town(Some(t))?))).collect();
                pop.home_town = w
                    .roaming_home
                    .iter()
                    .filter_map(|&(l, nest, k)| Some((l, if nest { Err(k) } else { Ok(town(Some(k))?) })))
                    .collect();
            }
            self.machine_stock = w
                .machine_stock
                .iter()
                .map(|(k, made, items)| {
                    let mut inv = kenshi_char::inventory::Inventory { sections: vec![kenshi_char::inventory::Section::new("main", 8, 8, kenshi_char::inventory::ANY)] };
                    restore_items(&data, &mut inv, items);
                    (*k, (made.clone(), inv))
                })
                .collect();
            for (d, &locked) in self.door_states.iter_mut().zip(&w.doors_locked) {
                d.locked = locked;
            }
            for (d, &(phase, open, broken)) in self.door_states.iter_mut().zip(&w.doors) {
                (d.phase, d.open, d.broken) = (phase, open, broken);
            }
            let m = &mut self.dialog_memory;
            m.unlocked = w.dialog_unlocked.iter().cloned().collect();
            m.locked = w.dialog_locked.iter().cloned().collect();
            m.spoken = w.dialog_spoken.iter().cloned().collect();
            m.has = w.dialog_has.iter().cloned().collect();
            m.met = w.dialog_met.iter().cloned().collect();
            m.tags = w.dialog_tags.iter().cloned().collect();
            m.ranks = w.ranks.iter().cloned().collect();
            m.reputation = w.reputation.iter().cloned().collect();
            m.event_count = w.event_count.iter().cloned().collect();
            self.contracts = w.contracts.clone();
            m.claimed = w.claimed.iter().cloned().collect();
            self.kenshi_base = w.kenshi_base.clone();
            m.running.clear();
            // the world's towns and the squads' towns by this world's numbers
            let mut w = w.clone();
            if !w.towns.is_empty() {
                let mut towns: Vec<Option<crate::world::TownState>> = vec![None; self.towns.len()];
                for (k, s) in w.towns.drain(..).enumerate() {
                    if let Some(t) = town(Some(k)) {
                        towns[t] = Some(crate::world::TownState { id: self.towns[t].id.clone(), ..s });
                    }
                }
                // (a town the save did not know: as the world makes it, `apply_saved_world`)
                w.towns = towns.into_iter().enumerate().map(|(t, s)| s.unwrap_or_else(|| crate::world::TownState { record: String::new(), id: self.towns[t].id.clone() })).collect();
            }
            for sq in &mut w.squads {
                // its home town (crate::world's WorldSquad.home) by this world's numbers too
                sq.home = town(sq.home);
            }
            // (an older save's patrols and raids are gone on the world's first tick, `world`)
            self.pending_world = Some(w);
        }
        // the rest as an older save has it (its players already put back above)
        self.t = save.t;
        self.tick = save.tick;
        self.money = save.money;
        // the outposts are worked out again from the buildings (base::assign_outposts)
        self.outposts.clear();
        self.buildings = save
            .buildings
            .iter()
            .filter_map(|sb| {
                let rec = data.get(&sb.building.record)?;
                let mut b = sb.building.clone();
                b.storage = crate::base::PlayerBuilding::new(rec, b.at, b.yaw).storage;
                if let Some(inv) = b.storage.as_mut() {
                    restore_items(&data, inv, &sb.storage);
                }
                Some(b)
            })
            .collect();
        self.world.dead_uniques = save.dead_uniques.iter().cloned().collect();
        self.world.campaigns_run = save.campaigns_run.iter().cloned().collect();
        let (found, been) = (self.town_keys(&save.discovered), self.town_keys(&save.explored));
        self.world.discovered.extend(found);
        self.world.explored.extend(been);
        self.world.started = false;
        self.nests = save.nests.clone();
        self.looted = save.looted.iter().cloned().collect();
        self.saved_boxes = save.boxes.clone();
        for b in &mut self.boxes {
            b.inventory.sections.iter_mut().for_each(|s| s.items.clear());
        }
        self.refill_boxes(0);
        // the item spots taken keep waiting (those loaded now, the others as their zone loads)
        self.saved_spot_dues = save.spot_dues.clone();
        self.apply_spot_dues(0);
        self.ground.clear();
        for (si, at, yaw) in &save.ground {
            let Some(item) = item_of(&data, si) else { continue };
            self.next_ground += 1;
            self.ground.push(crate::loot::GroundItem { id: self.next_ground, item, at: *at, yaw: *yaw });
        }
        self.research_done = save.research.done.clone();
        self.research_queue = save.research.queue.clone();
        self.research_progress = save.research.progress;
        self.research_paid = save.research.paid;
        if !save.research.outpost_name.is_empty() {
            self.outpost_name = save.research.outpost_name.clone();
        }
        self.outpost_public = save.research.outpost_public;
        self.ai_options = save.research.ai_options.clone();
        self.advanced = save.research.advanced.clone();
        self.arena.sh.damage_mult = self.advanced.damage;
        // the squads as saved (the people keep their platoons)
        self.faction_name = save.research.faction_name.clone();
        self.player_squads = save.research.player_squads.clone();
        self.squads_ready();
        // the player's characters (in number order, as saved) with their looks
        let ids: Vec<usize> = self.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
        ids.iter().enumerate().map(|(k, &id)| (id, save.players.get(k).and_then(|sp| sp.look.clone()))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_round_trip() {
        let save = SaveGame {
            version: VERSION,
            zone: (34, 46),
            t: 12.5,
            tick: 750,
            money: 1234,
            players: vec![SavedPerson {
                name: "Beep".into(),
                position: [1.0, 2.0, 3.0],
                facing: [0.0, 0.0, 1.0],
                character: None,
                skills: vec![(1, 12.5)],
                medical: Medical::default(),
                down: false,
                dead: false,
                inventory: vec![SavedItem { section: 9, record: "x".into(), quantity: 2, x: 1, y: 3, food_left: Some(10.0), quality: None, maker: None, model: None, uniform: None, owner: None, crafter: None, unique: false, contents: Vec::new() }],
                look: None,
                bounties: Vec::new(),
                stance: Default::default(),
                hold_at: None,
                jobs: Vec::new(),
                jobs_on: false,
            }],
            buildings: Vec::new(),
            research: SavedResearch::default(),
            dead_uniques: Vec::new(),
            campaigns_run: Vec::new(),
            discovered: Vec::new(),
            explored: Vec::new(),
            town_ids: Vec::new(),
            ground: Vec::new(),
            looted: Vec::new(),
            spot_dues: Vec::new(),
            boxes: Vec::new(),
            nests: Vec::new(),
            people: Vec::new(),
            zone_order: Vec::new(),
            world: None,
        };
        let dir = std::env::temp_dir().join("kenshi-rewrite-save-test");
        let path = dir.join("t.ron");
        write(&save, &path).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.money, 1234);
        assert_eq!(back.players[0].name, "Beep");
        assert_eq!(back.players[0].inventory[0].food_left, Some(10.0));
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Saves from before ages were kept: grown.
fn grown() -> f32 {
    1.0
}
