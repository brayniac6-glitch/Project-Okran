//! Character inventories: the sections CharacterHuman::setupInventorySections makes (F040052) and
//! the items in them, placed by their footprint ("inventory footprint width / height").
//!
//! A section takes items whose "slot" matches its slot type (7, the main grid, takes any); the
//! main grid places items on its cells, an equipment section holds one item (assumption: as the
//! game shows them, the section's grid sized to fit one of its kind).
use kenshi_data::{GameData, GameDataContainer, ItemType};

/// A thing in an inventory.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub record: String,
    pub name: String,
    pub kind: Option<ItemType>,
    /// The section slot type it goes in (ATTACH: 0 weapon, 3 head, 5 armour, 6 legs, 7 any,
    /// 8 shirt, 9 boots, 12 backpack, 14 belt).
    pub slot: i32,
    pub w: u32,
    pub h: u32,
    pub quantity: u32,
    /// "stackable": how many share a cell (1: one each).
    pub stack: u32,
    pub weight: f32,
    pub value: i32,
    /// The icon file ("icon"), when the item has one rather than a picture drawn from its mesh.
    pub icon: Option<String>,
    /// Food ("item function" 3): its food value ("charges") and what is left in the one being
    /// eaten (+0x118).
    pub food: Option<(f32, f32)>,
    /// A medical kit ("item function" 1, a robot repair kit 12): its "quality" (the skill it
    /// lets a medic use, +0x11c), the charges left in the one in use (+0x118, from
    /// "charges"), and whether it is a robot kit.
    pub medkit: Option<Medkit>,
    /// Crossbow ammo ("item function" 16): the pack's "charges" (one a reload) and those left in
    /// it (+0x118, F033853).
    pub ammo: Option<(f32, f32)>,
    /// Its quality, 0 .. 100 (`quality`: armour's grade, a weapon's model).
    pub quality: f32,
    /// A weapon's maker (WEAPON_MANUFACTURER) and model (MATERIAL_SPECS_WEAPON).
    pub maker: Option<String>,
    pub model: Option<String>,
    /// Branded as a faction's uniform (CHARACTER "wears uniform"): that faction.
    pub uniform: Option<String>,
    /// Its owners (Item +0x140, the original owner's handle): a faction; none for one's own.
    /// An item whose owners are not its holder's faction is loot (F044581).
    pub owner: Option<String>,
    /// How many of the stack are buyback (the trade GUI's map of items to counts, global
    /// 0x142132c18: F042723 reads it, F042730 toggles it as the item crosses in a trade, F042731
    /// moves counts with merged or split stacks; cleared only with the game, F042716 from
    /// F042099), so not saved.
    pub buyback: i32,
    /// The smith who made it (Item +0x1f8, set by addFinishedCraftItem F029664; saved as
    /// "crafter", F043905 / F043906): weapons, armour, crossbows and limbs (vtable slot 88).
    pub crafter: Option<String>,
    /// One of a kind (Item +0x12a, the "[Unique]" tag): the world's artifacts as their town hands
    /// them out (F022232 from Town::distributeArtifacts F048891) and the towns' "special items"
    /// (F049408); saved as "unique" (F043903 / F043904).
    pub unique: bool,
    /// A backpack's storage (CONTAINER "storage size width / height": its own grid) and what
    /// its contents weigh against their own weight ("encumbrance effect", F043920 +0x23c).
    /// (assumption) the contents' weight is the effect times theirs.
    pub contents: Option<Inventory>,
    pub carry: f32,
}

impl Item {
    pub fn from_record(r: &GameData, quantity: u32) -> Item {
        let int = |k: &str| r.ints.get(k).copied().or_else(|| r.floats.get(k).map(|f| *f as i32));
        let kind = r.item_type();
        let slot = int("slot").unwrap_or(match kind {
            Some(ItemType::Weapon) => 0,
            _ => 7,
        });
        Item {
            record: r.string_id.clone(),
            name: r.name.clone(),
            kind,
            slot,
            w: int("inventory footprint width").unwrap_or(1).max(1) as u32,
            h: int("inventory footprint height").unwrap_or(1).max(1) as u32,
            quantity: quantity.max(1),
            stack: int("stackable").unwrap_or(1).max(1) as u32,
            weight: r.floats.get("weight kg").copied().unwrap_or(0.0),
            value: int("value").unwrap_or(0),
            icon: r.files.get("icon").filter(|f| !f.is_empty()).cloned(),
            food: (int("item function") == Some(3)).then(|| {
                let c = int("charges").unwrap_or(0).max(1) as f32;
                (c, c)
            }),
            medkit: matches!(int("item function"), Some(1 | 12)).then(|| {
                let c = int("charges").unwrap_or(0).max(1) as f32;
                Medkit {
                    quality: r.floats.get("quality").copied().or_else(|| int("quality").map(|v| v as f32)).unwrap_or(0.0),
                    charges: c,
                    full: c,
                    robot: int("item function") == Some(12),
                }
            }),
            ammo: (int("item function") == Some(16)).then(|| {
                let c = int("charges").unwrap_or(0).max(1) as f32;
                (c, c)
            }),
            // (assumption) an item made with no grade or maker given: Standard armour (grade
            // 2), a weapon at 15 (Ancient's lowest model)
            quality: match kind {
                Some(ItemType::Armour) => 40.0,
                Some(ItemType::Weapon) => 15.0,
                _ => 0.0,
            },
            maker: None,
            model: None,
            uniform: None,
            owner: None,
            buyback: 0,
            crafter: None,
            unique: false,
            contents: (kind == Some(ItemType::Container)).then(|| int("storage size width").zip(int("storage size height"))).flatten().filter(|&(w, h)| w > 0 && h > 0).map(|(w, h)| Inventory {
                sections: vec![Section::new("main", w as u32, h as u32, ANY)],
            }),
            carry: r.floats.get("encumbrance effect").copied().unwrap_or(1.0),
        }
    }
}

/// A medical kit's state ([`Item::medkit`]).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Medkit {
    pub quality: f32,
    pub charges: f32,
    pub full: f32,
    pub robot: bool,
}

/// An item and the cell its top-left sits on.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub item: Item,
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub name: &'static str,
    pub w: u32,
    pub h: u32,
    pub slot: i32,
    pub items: Vec<Placed>,
    /// The items it takes, when kept to some (InventorySection +0x98 / +0xa0, F014ce0 adds one:
    /// a machine's container, F029392; a storage's "limit inventory", F029410); empty for any.
    pub only: Vec<String>,
    /// Its stack size: a stack in it holds the more of this (+0x88: a machine container's limit,
    /// a building's "stackable bonus minimum") and the item's own "stackable" (1 for a section
    /// kept to some items), times `mult` (+0x8c, a building's "stackable bonus mult"; F043593).
    pub stack: u32,
    pub mult: f32,
    /// +0x38: it takes only armour (a building's storage of "itemtype limit" 3, F037797;
    /// F043601).
    pub armour: bool,
}

/// The main grid's slot type: it takes anything.
pub const ANY: i32 = 7;

impl Section {
    pub fn new(name: &'static str, w: u32, h: u32, slot: i32) -> Section {
        Section { name, w, h, slot, items: Vec::new(), only: Vec::new(), stack: 0, mult: 1.0, armour: false }
    }

    /// A machine's container of `w` x `h` (the item's footprint) taking only `record`, a stack
    /// of up to `stack` of it (F029392).
    pub fn container(name: &'static str, w: u32, h: u32, record: &str, stack: u32) -> Section {
        Section { name, w, h, slot: ANY, items: Vec::new(), only: vec![record.to_string()], stack, mult: 1.0, armour: false }
    }

    /// @fn F043593
    ///
    /// How many of `item` a stack in this section holds (F0103ed): the more of the item's
    /// "stackable" (1 in a section kept to some items) and the section's stack size, times its
    /// multiplier; at least 1.
    pub fn stack_of(&self, item: &Item) -> u32 {
        let own = if self.only.is_empty() { item.stack } else { 1 };
        ((own.max(self.stack) as f32 * self.mult) as u32).max(1)
    }

    /// How many items it holds (F746840: a container's count).
    pub fn count(&self) -> u32 {
        self.items.iter().map(|p| p.item.quantity).sum()
    }

    /// An equipment section: one item of its kind.
    pub fn is_equipment(&self) -> bool {
        self.slot != ANY
    }

    /// @fn F043601
    ///
    /// Whether the section takes `item`: one kept to some items takes those; else one of armour
    /// takes armour; else its slot type (7 any).
    pub fn takes(&self, item: &Item) -> bool {
        if !self.only.is_empty() {
            return self.only.contains(&item.record);
        }
        if self.armour {
            return item.kind == Some(ItemType::Armour);
        }
        self.slot == ANY || item.slot == self.slot
    }

    /// Whether `item` can sit at (x, y), leaving out item `except` (the one being moved).
    pub fn fits(&self, item: &Item, x: u32, y: u32, except: Option<usize>) -> bool {
        if !self.takes(item) {
            return false;
        }
        if self.is_equipment() {
            return self.items.iter().enumerate().all(|(i, _)| Some(i) == except);
        }
        if x + item.w > self.w || y + item.h > self.h {
            return false;
        }
        self.items.iter().enumerate().filter(|(i, _)| Some(*i) != except).all(|(_, p)| {
            x + item.w <= p.x || p.x + p.item.w <= x || y + item.h <= p.y || p.y + p.item.h <= y
        })
    }

    /// The first free cell for `item`, row by row.
    pub fn first_fit(&self, item: &Item) -> Option<(u32, u32)> {
        if self.is_equipment() {
            return self.fits(item, 0, 0, None).then_some((0, 0));
        }
        (0..self.h).flat_map(|y| (0..self.w).map(move |x| (x, y))).find(|&(x, y)| self.fits(item, x, y, None))
    }

    /// @fn F043580
    ///
    /// A section arranged (its ARRANGE button, F042738 -> F043557 over the inventory's sections):
    /// its items taken out and put back, each where it first fits row by row and onto a stack of
    /// its kind with room (F043608), the biggest first: by their longer side, else (not all fitting)
    /// by their area, else by their width; the stacks emptied on the way are dropped between tries.
    /// Nothing fitting all three ways, the items left are put back where they were.
    /// (assumption) Items of the same size keep the section's order (Kenshi's is its item map's).
    pub fn arrange(&mut self) {
        if self.is_equipment() || self.items.is_empty() {
            return;
        }
        let mut entries: Vec<Placed> = std::mem::take(&mut self.items);
        for mode in 0..3 {
            let key = |p: &Placed| -> u32 {
                match mode {
                    0 => p.item.w.max(p.item.h),
                    1 => p.item.w * p.item.h,
                    _ => p.item.w,
                }
            };
            let mut order: Vec<usize> = (0..entries.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(key(&entries[i])));
            // placed: which entry sits where
            let mut placed: Vec<(usize, u32, u32)> = Vec::new();
            let mut all = true;
            for &i in &order {
                // onto a stack of its kind with room
                for &(j, ..) in &placed {
                    let (a, b) = (&entries[i].item, &entries[j].item);
                    let most = self.stack_of(b);
                    if a.record == b.record && most > 1 && b.quantity < most && a.quantity > 0 {
                        let take = (most - b.quantity).min(a.quantity);
                        entries[j].item.quantity += take;
                        entries[i].item.quantity -= take;
                        let n = (take as i32).min(entries[i].item.buyback);
                        entries[j].item.buyback += n;
                        entries[i].item.buyback -= n;
                    }
                }
                if entries[i].item.quantity == 0 {
                    continue;
                }
                self.items = placed.iter().map(|&(j, x, y)| Placed { item: entries[j].item.clone(), x, y }).collect();
                match self.first_fit(&entries[i].item) {
                    Some((x, y)) => placed.push((i, x, y)),
                    None => all = false,
                }
            }
            if all {
                self.items = placed.into_iter().map(|(j, x, y)| Placed { item: entries[j].item.clone(), x, y }).collect();
                return;
            }
            if mode != 2 {
                entries.retain(|p| p.item.quantity > 0);
            }
        }
        // put back where they were
        self.items = entries.into_iter().filter(|p| p.item.quantity > 0).collect();
    }

    /// The item covering cell (x, y).
    pub fn at(&self, x: u32, y: u32) -> Option<usize> {
        if self.is_equipment() {
            return (!self.items.is_empty()).then_some(0);
        }
        self.items.iter().position(|p| x >= p.x && x < p.x + p.item.w && y >= p.y && y < p.y + p.item.h)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inventory {
    pub sections: Vec<Section>,
}

impl Inventory {
    /// @fn F040052
    ///
    /// A character's sections: hip 7x1 and back 10x2 (weapons), boots 4x2, head 4x3,
    /// backpack_attach 5x4, shirt 4x2, armour 4x6, legs 4x5, belt 2x2 and the main 8x10.
    pub fn character() -> Inventory {
        Inventory {
            sections: vec![
                Section::new("hip", 7, 1, 0),
                Section::new("back", 10, 2, 0),
                Section::new("boots", 4, 2, 9),
                Section::new("head", 4, 3, 3),
                Section::new("backpack_attach", 5, 4, 12),
                Section::new("shirt", 4, 2, 8),
                Section::new("armour", 4, 6, 5),
                Section::new("legs", 4, 5, 6),
                Section::new("belt", 2, 2, 14),
                Section::new("main", 8, 10, ANY),
            ],
        }
    }

    /// @fn F040052
    ///
    /// An animal's sections: its backpack slot and a main grid of its "inventory w" x
    /// "inventory h" (ANIMAL_CHARACTER).
    pub fn animal(w: u32, h: u32) -> Inventory {
        Inventory { sections: vec![Section::new("backpack_attach", 5, 4, 12), Section::new("main", w.max(1), h.max(1), ANY)] }
    }

    /// The worn backpack's storage, if one is worn.
    pub fn bag(&self) -> Option<&Inventory> {
        self.sections.iter().find(|s| s.name == "backpack_attach")?.items.first()?.item.contents.as_ref()
    }

    pub fn bag_mut(&mut self) -> Option<&mut Inventory> {
        self.sections.iter_mut().find(|s| s.name == "backpack_attach")?.items.first_mut()?.item.contents.as_mut()
    }

    pub fn section(&self, name: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.name == name)
    }

    /// Stack onto the same item where there is room, else equip it (`equip`) in a free section
    /// of its kind, else the first free cell of the main grid; false when it does not fit.
    pub fn add(&mut self, mut item: Item, equip: bool) -> bool {
        for s in &mut self.sections {
            let most = s.stack_of(&item);
            for p in &mut s.items {
                if p.item.record == item.record && most > 1 && p.item.quantity < most {
                    let take = (most - p.item.quantity).min(item.quantity);
                    p.item.quantity += take;
                    item.quantity -= take;
                    // InventorySection::addItem (F043608): the buyback count of what merged
                    // goes with it (F042731)
                    let n = (take as i32).min(item.buyback);
                    p.item.buyback += n;
                    item.buyback -= n;
                    if item.quantity == 0 {
                        return true;
                    }
                }
            }
        }
        // the sections kept to the item first (a machine's containers, F031147 / F029325)
        let mut order: Vec<usize> = (0..self.sections.len()).filter(|&i| self.sections[i].only.contains(&item.record) && self.sections[i].name != "main").collect();
        if equip {
            order.extend((0..self.sections.len()).filter(|&i| self.sections[i].is_equipment()));
        }
        order.extend(self.section("main"));
        for i in order {
            if let Some((x, y)) = self.sections[i].first_fit(&item) {
                // (a container's stack holds no more than its size: the rest goes on)
                let most = self.sections[i].stack_of(&item);
                if item.quantity > most && (!self.sections[i].only.is_empty() || self.sections[i].stack > 0 || self.sections[i].mult != 1.0) {
                    let mut rest = item.clone();
                    rest.quantity -= most;
                    item.quantity = most;
                    self.sections[i].items.push(Placed { item, x, y });
                    return self.add(rest, equip);
                }
                self.sections[i].items.push(Placed { item, x, y });
                return true;
            }
        }
        // (assumption) what does not fit in the main grid goes in the worn backpack
        match self.bag_mut() {
            Some(bag) => bag.add(item, false),
            None => false,
        }
    }

    /// Move item `idx` of section `from` to cell (x, y) of section `to`, if it fits there.
    pub fn move_item(&mut self, from: usize, idx: usize, to: usize, x: u32, y: u32) -> bool {
        let Some(item) = self.sections.get(from).and_then(|s| s.items.get(idx)).map(|p| p.item.clone()) else {
            return false;
        };
        let except = (from == to).then_some(idx);
        if !self.sections.get(to).is_some_and(|s| s.fits(&item, x, y, except)) {
            return false;
        }
        let p = self.sections[from].items.remove(idx);
        self.sections[to].items.push(Placed { item: p.item, x, y });
        true
    }

    /// Take item `idx` out of section `s`.
    /// F039256 on the first food carried: eat one bite for a hunger shown at `shown`; the
    /// food item loses it, one off its stack (or gone) when used up. The food's name and the
    /// bite, or None with no food.
    pub fn eat(&mut self, shown: f32) -> Option<(String, f32)> {
        for sec in &mut self.sections {
            let Some(i) = sec.items.iter().position(|p| p.item.food.is_some()) else { continue };
            let item = &mut sec.items[i].item;
            let (full, left) = item.food.expect("food");
            let b = crate::hunger::bite(left, shown);
            if b <= 0.0 {
                return None;
            }
            let left = left - b;
            let name = item.name.clone();
            if left <= 0.0 {
                if item.quantity > 1 {
                    item.quantity -= 1;
                    item.food = Some((full, full));
                } else {
                    sec.items.remove(i);
                }
            } else {
                item.food = Some((full, left));
            }
            return Some((name, b));
        }
        self.bag_mut()?.eat(shown)
    }

    /// The charges of ammo item `record` carried (every pack's left, packs after the first full).
    pub fn ammo_charges(&self, record: &str) -> u32 {
        let own = self
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .filter(|p| p.item.record == record)
            .filter_map(|p| p.item.ammo.map(|(full, left)| left + full * (p.item.quantity.max(1) - 1) as f32))
            .sum::<f32>() as u32;
        own + self.bag().map_or(0, |b| b.ammo_charges(record))
    }

    /// One charge of ammo item `record` spent (F033853): from the first pack, a pack emptied
    /// taken away. False when there is none.
    pub fn spend_ammo(&mut self, record: &str) -> bool {
        for sec in &mut self.sections {
            let Some(i) = sec.items.iter().position(|p| p.item.record == record && p.item.ammo.is_some()) else { continue };
            let item = &mut sec.items[i].item;
            let (full, mut left) = item.ammo.expect("ammo");
            left -= 1.0;
            if left <= 0.0 {
                if item.quantity > 1 {
                    item.quantity -= 1;
                    item.ammo = Some((full, full));
                } else {
                    sec.items.remove(i);
                }
            } else {
                item.ammo = Some((full, left));
            }
            return true;
        }
        self.bag_mut().is_some_and(|b| b.spend_ammo(record))
    }

    /// Put `item` in section `s` with its top-left at (x, y) if it fits there (back otherwise).
    #[allow(clippy::result_large_err)]
    pub fn put(&mut self, s: usize, item: Item, x: u32, y: u32) -> Result<(), Item> {
        match self.sections.get_mut(s) {
            Some(sec) if sec.takes(&item) && sec.fits(&item, x, y, None) => {
                sec.items.push(Placed { item, x, y });
                Ok(())
            }
            _ => Err(item),
        }
    }

    pub fn take(&mut self, s: usize, idx: usize) -> Option<Item> {
        let sec = self.sections.get_mut(s)?;
        (idx < sec.items.len()).then(|| sec.items.remove(idx).item)
    }

    /// Everything carried, by weight (kg).
    pub fn weight(&self) -> f32 {
        self.sections
            .iter()
            .flat_map(|s| &s.items)
            .map(|p| p.item.weight * p.item.quantity as f32 + p.item.contents.as_ref().map_or(0.0, |c| c.weight() * p.item.carry))
            .sum()
    }

    /// The items equipped (every section but the main grid).
    pub fn equipped(&self) -> impl Iterator<Item = &Item> {
        self.sections.iter().filter(|s| s.is_equipment()).flat_map(|s| s.items.iter().map(|p| &p.item))
    }

    /// Put `item` in the first free place of section `s` (one that takes it), else back.
    #[allow(clippy::result_large_err)]
    pub fn add_to(&mut self, s: usize, item: Item) -> Result<(), Item> {
        match self.sections.get(s).filter(|sec| sec.takes(&item)).and_then(|sec| sec.first_fit(&item)) {
            Some((x, y)) => self.put(s, item, x, y),
            None => Err(item),
        }
    }

    /// A CHARACTER template's starting kit: its "clothing" (worn) and "inventory" items, each
    /// entry (count, chance %) rolled with `roll` (0..100), and its weapons (`template_weapons`).
    pub fn from_template(data: &GameDataContainer, character: &GameData, roll: &mut impl FnMut() -> f32) -> Inventory {
        let mut inv = Inventory::character();
        for (cat, equip) in [("clothing", true), ("weapons", true), ("inventory", false)] {
            if cat == "weapons" {
                inv.template_weapons(data, character, roll);
                continue;
            }
            // (assumption) a template's clothing is tried in random order, so of those for a slot
            // that pass their chance a random one is worn (Kenshi's own choice, F030256's
            // catalogue by slot and class, is not ported); in list order every character of a
            // template wore the same
            let mut entries: Vec<_> = character.refs(cat).iter().collect();
            if cat == "clothing" {
                for i in (1..entries.len()).rev() {
                    let j = ((roll() * 0.01 * (i + 1) as f32) as usize).min(i);
                    entries.swap(i, j);
                }
            }
            for (sid, v) in entries {
                let Some(r) = data.get(sid) else { continue };
                let count = v.first().copied().unwrap_or(1).max(1) as u32;
                let chance = v.get(1).copied().unwrap_or(100) as f32;
                if roll() >= chance {
                    continue;
                }
                let item = Item::from_record(r, count);
                if item.stack > 1 {
                    inv.add(item, equip);
                } else {
                    for _ in 0..count {
                        let one = make_one(data, character, r, roll);
                        inv.add(one, equip);
                    }
                }
            }
        }
        inv
    }

    /// @fn F040052
    ///
    /// A CHARACTER template's "weapons": of the entries whose first value is above 0, those
    /// whose second is 0 are the hip's and the rest the back's, each weighing its third value
    /// (0 counts 100). For each list (the hip's first) a roll of 0 .. 100 under the list's total
    /// weight takes the entry whose running total first passes it; the weapon made (by a maker
    /// of the "weapon level" list, `make_one`) goes in its section ("hip", "back"), else
    /// the inventory takes it (its add, equipping: vtable 0x10 with 1).
    fn template_weapons(&mut self, data: &GameDataContainer, character: &GameData, roll: &mut impl FnMut() -> f32) {
        let mut lists: [Vec<(&str, f32)>; 2] = [Vec::new(), Vec::new()];
        for (sid, v) in character.refs("weapons") {
            if v.first().copied().unwrap_or(0) <= 0 {
                continue;
            }
            let w = match v.get(2).copied().unwrap_or(0) {
                0 => 100.0,
                w => w as f32,
            };
            if w <= 0.0 {
                continue;
            }
            let back = v.get(1).copied().unwrap_or(0) != 0;
            lists[back as usize].push((sid.as_str(), w));
        }
        let mut picks: Vec<(&str, &str)> = Vec::new();
        for (list, section) in lists.iter().zip(["hip", "back"]) {
            if list.is_empty() {
                continue;
            }
            let total: f32 = list.iter().map(|e| e.1).sum();
            let r = roll();
            if r >= total {
                continue;
            }
            let mut acc = 0.0;
            if let Some((sid, _)) = list.iter().find(|e| {
                acc += e.1;
                r < acc
            }) {
                picks.push((sid, section));
            }
        }
        for (sid, section) in picks {
            let Some(r) = data.get(sid) else { continue };
            let item = make_one(data, character, r, roll);
            let placed = match self.section(section) {
                Some(s) => self.add_to(s, item),
                None => Err(item),
            };
            if let Err(item) = placed {
                self.add(item, true);
            }
        }
    }
}

/// F040052: one of a CHARACTER template's kit items: armour at the character's "armour grade",
/// one higher by its "armour upgrade chance" (grade below 5); a weapon by a maker from its
/// "weapon level" (F038067: one of them at random; none: Ancient).
fn make_one(data: &GameDataContainer, character: &GameData, r: &GameData, roll: &mut impl FnMut() -> f32) -> Item {
    match r.item_type() {
        Some(ItemType::Armour) => {
            let grade = character.ints.get("armour grade").copied().unwrap_or(1);
            let chance = character.ints.get("armour upgrade chance").copied().unwrap_or(0) as f32;
            let up = (grade < 5 && roll() < chance) as i32;
            Item::armour(data, r, grade + up)
        }
        Some(ItemType::Weapon) => {
            let makers = character.refs("weapon level");
            let maker = (!makers.is_empty()).then(|| makers[((roll() * 0.01 * makers.len() as f32) as usize).min(makers.len() - 1)].0.clone());
            let mut unit = || (roll() * 0.01).clamp(0.0, 0.9999);
            Item::weapon(data, r, maker.as_deref(), &mut unit)
        }
        _ => Item::from_record(r, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(slot: i32, w: u32, h: u32) -> Item {
        Item {
            record: format!("{slot}-{w}x{h}"),
            name: String::new(),
            kind: None,
            slot,
            w,
            h,
            quantity: 1,
            stack: 1,
            weight: 1.0,
            value: 0,
            quality: 0.0,
            maker: None,
            model: None,
            uniform: None,
            owner: None,
            buyback: 0,
            crafter: None,
            unique: false,
            icon: None,
            food: None,
            medkit: None,
            ammo: None,
            contents: None,
            carry: 1.0,
        }
    }

    #[test]
    fn arrange_packs_the_biggest_first() {
        let mut s = Section::new("main", 4, 4, ANY);
        // the 2x2 goes first, to the top left; the 1x1 after it
        s.items.push(Placed { item: item(7, 1, 1), x: 0, y: 0 });
        s.items.push(Placed { item: item(7, 2, 2), x: 2, y: 2 });
        s.arrange();
        let big = s.items.iter().find(|p| p.item.w == 2).unwrap();
        assert_eq!((big.x, big.y), (0, 0));
        let small = s.items.iter().find(|p| p.item.w == 1).unwrap();
        assert_eq!((small.x, small.y), (2, 0));
    }

    #[test]
    fn arrange_merges_stacks() {
        let mut s = Section::new("main", 4, 4, ANY);
        let mut a = item(7, 1, 1);
        a.stack = 10;
        a.quantity = 3;
        let mut b = a.clone();
        b.quantity = 4;
        s.items.push(Placed { item: a, x: 3, y: 3 });
        s.items.push(Placed { item: b, x: 0, y: 2 });
        s.arrange();
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].item.quantity, 7);
        assert_eq!((s.items[0].x, s.items[0].y), (0, 0));
    }

    #[test]
    fn main_grid_packs_by_footprint() {
        let mut inv = Inventory::character();
        let main = inv.section("main").unwrap();
        for _ in 0..4 {
            assert!(inv.add(item(7, 4, 5), false));
        }
        // 8x10 holds four 4x5 items and nothing more
        assert!(!inv.add(item(7, 1, 1), false));
        assert_eq!(inv.sections[main].items.len(), 4);
    }

    #[test]
    fn equipment_takes_its_kind_once() {
        let mut inv = Inventory::character();
        assert!(inv.add(item(6, 4, 4), true));
        let legs = inv.section("legs").unwrap();
        assert_eq!(inv.sections[legs].items.len(), 1);
        // a second pair goes to the main grid
        assert!(inv.add(item(6, 4, 4), true));
        assert_eq!(inv.sections[legs].items.len(), 1);
        assert_eq!(inv.sections[inv.section("main").unwrap()].items.len(), 1);
        // moving a weapon into the legs is refused
        assert!(inv.add(item(0, 7, 1), true));
        let hip = inv.section("hip").unwrap();
        assert!(!inv.move_item(hip, 0, legs, 0, 0));
    }

    #[test]
    fn stacks_fill_before_new_cells() {
        let mut inv = Inventory::character();
        let mut kit = item(7, 2, 1);
        kit.stack = 3;
        kit.quantity = 2;
        assert!(inv.add(kit.clone(), false));
        kit.quantity = 2;
        assert!(inv.add(kit, false));
        let main = &inv.sections[inv.section("main").unwrap()];
        assert_eq!(main.items.len(), 2);
        assert_eq!(main.items[0].item.quantity, 3);
        assert_eq!(main.items[1].item.quantity, 1);
    }

    #[test]
    fn a_backpack_carries_what_the_grid_cannot_at_its_weight() {
        let mut inv = Inventory::character();
        let mut bag = item(12, 3, 3);
        bag.contents = Some(Inventory { sections: vec![Section::new("main", 2, 2, ANY)] });
        bag.carry = 0.5;
        assert!(inv.add(bag, true));
        assert!(inv.bag().is_some());
        // fill the main grid, then one more goes in the bag
        let main = inv.section("main").unwrap();
        while inv.sections[main].first_fit(&item(7, 1, 1)).is_some() {
            assert!(inv.add(item(7, 1, 1), false));
        }
        let before = inv.weight();
        assert!(inv.add(item(7, 1, 1), false));
        assert_eq!(inv.bag().unwrap().sections[0].items.len(), 1);
        // its weight counted at half
        assert!((inv.weight() - before - 0.5).abs() < 1e-5);
    }
}
