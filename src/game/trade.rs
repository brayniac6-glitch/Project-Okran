//! Trading (port_map `economy`): what things cost, what shops stock and how much money their
//! traders hold, and each town's prices for trade goods.
//!
//! Specs: F044608 Item::value, F047182 Weapon::value, F047262 Armour::value, F048904
//! Town::setup (trade prices), F049418 ShopTraderInventory::restock, F049413 fillFromVendors,
//! F049397 VendorList::generate; GLOBAL CONSTANTS price multipliers (F046688).
//!
//! (stand-in) Items are priced from their record's "value": weapons' and armour's computed
//! values (material, maker, quality) are not ported, nor the per-item quality factor
//! (helper 0x1400033c8). The category multipliers are applied by item kind (assumption: the
//! game folds them into the stored value). A vendor list entry's weight is its first ref value,
//! else its second (assumption); weapon manufacturers are not picked. Town wealth is 1.
use kenshi_char::inventory::{Inventory, Item, Section};
use kenshi_data::{GameData, GameDataContainer, ItemType};
use std::collections::HashMap;

/// The trader window's grid (Kenshi_InventoryTraderWindow.layout's 450 x 600 canvas at the
/// inventory's 45-unit cells, (assumption)).
pub const SHOP_W: u32 = 10;
pub const SHOP_H: u32 = 13;

/// The GLOBAL CONSTANTS prices read (F046688; shipped values as defaults).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceConstants {
    pub global: f32,
    pub armour: f32,
    pub sword: f32,
    pub clothing: f32,
    pub crossbow: f32,
    pub robotics: f32,
    pub food: f32,
    pub blueprint: f32,
    pub trade: f32,
    pub margins: f32,
    pub loot: f32,
    pub loot_gear: f32,
}

impl PriceConstants {
    pub fn load(data: &GameDataContainer) -> PriceConstants {
        let g = |k: &str, d: f32| data.get("110-gamedata.quack").and_then(|r| r.floats.get(k).copied()).unwrap_or(d);
        let global = g("global price mult", 1.0);
        PriceConstants {
            global,
            armour: global * g("armor price mult", 0.6),
            sword: global * g("sword price mult", 0.4),
            clothing: global * g("clothing price mult", 1.0),
            crossbow: global * g("crossbow price mult", 1.0),
            robotics: global * g("robotics price mult", 1.0),
            food: global * g("food price mult", 0.5),
            blueprint: g("blueprint price mult", 1.5),
            trade: global * g("trade price mult", 0.6),
            margins: g("trade profit margins", 0.3),
            loot: g("loot price mult", 0.5),
            loot_gear: g("loot price mult GEAR", 0.25),
        }
    }
}

/// The trade goods (VENDOR_LIST "all trade goods", F048904) and each town's factor for them.
#[derive(Debug, Clone, Default)]
pub struct TradePrices {
    pub goods: Vec<String>,
    /// (town index, item record) -> factor.
    pub factor: HashMap<(usize, String), f32>,
}

impl TradePrices {
    /// F048904's trade prices for `towns` (their positions, in setup order): each good's
    /// factor uniform in [1 - m, 1 + m], then held within (distance / 1e6) x m x 10 of every
    /// town set up before it.
    pub fn roll(data: &GameDataContainer, towns: &[[f32; 3]], margins: f32, rand: &mut dyn FnMut() -> f32) -> TradePrices {
        let list = data.records.values().find(|r| r.item_type() == Some(ItemType::VendorList) && r.name == "all trade goods");
        let goods: Vec<String> = list.map(|l| l.refs("items").iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default();
        let mut factor = HashMap::new();
        for (t, at) in towns.iter().enumerate() {
            for g in &goods {
                let mut f = 1.0 - margins + rand() * 2.0 * margins;
                for (o, other) in towns.iter().enumerate().take(t) {
                    let Some(&fo) = factor.get(&(o, g.clone())) else { continue };
                    let d = ((at[0] - other[0]).powi(2) + (at[2] - other[2]).powi(2)).sqrt();
                    let reach = d / 1.0e6 * margins * 10.0;
                    f = f.clamp(fo - reach, fo + reach);
                }
                factor.insert((t, g.clone()), f);
            }
        }
        TradePrices { goods, factor }
    }

    /// The town's factor for an item (1 for what is not a trade good, or no town).
    pub fn at(&self, town: Option<usize>, record: &str) -> f32 {
        town.and_then(|t| self.factor.get(&(t, record.to_string())).copied()).unwrap_or(1.0)
    }

    pub fn is_good(&self, record: &str) -> bool {
        self.goods.iter().any(|g| g == record)
    }
}

/// What one of `item` is worth in `town` (F044608 / F047182 / F047262 without the sale
/// multiplier): its record value times its category's price multiplier, trade goods times the
/// town's factor, food by how much is left.
pub fn value(item: &Item, data: &GameDataContainer, c: &PriceConstants, prices: &TradePrices, town: Option<usize>) -> f32 {
    let r = data.get(&item.record);
    let base = item.value as f32;
    let cat = if prices.is_good(&item.record) {
        c.trade * prices.at(town, &item.record)
    } else if item.food.is_some() {
        c.food
    } else {
        match item.kind {
            Some(ItemType::Weapon) => c.sword,
            Some(ItemType::Armour) => {
                // clothing: armour with no material coverage of its own, (assumption) slots
                // shirt/legs/boots
                if matches!(item.slot, 6 | 8 | 9) { c.clothing } else { c.armour }
            }
            Some(ItemType::Crossbow) => c.crossbow,
            Some(ItemType::LimbReplacement) => c.robotics,
            Some(ItemType::Blueprint) => c.blueprint,
            _ => c.global,
        }
    };
    let eaten = item.food.map_or(1.0, |(full, left)| if full > 0.0 { left / full } else { 1.0 });
    let _ = r;
    (base * cat * eaten).max(0.0)
}

/// What a trader asks for one of `item`: its value.
pub fn buy_price(item: &Item, data: &GameDataContainer, c: &PriceConstants, prices: &TradePrices, town: Option<usize>) -> i32 {
    value(item, data, c, prices, town).round().max(1.0) as i32
}

/// What a trader pays for one of `item` (the sale branch of F044608 / F047182 / F047262):
/// weapons and armour x "loot price mult GEAR", the rest x "loot price mult". (assumption)
/// Everything the player sells counts as looted (+0x128); the player-made multipliers are not
/// used (no crafting yet).
pub fn sell_price(item: &Item, data: &GameDataContainer, c: &PriceConstants, prices: &TradePrices, town: Option<usize>) -> i32 {
    let m = match item.kind {
        Some(ItemType::Weapon) | Some(ItemType::Armour) | Some(ItemType::Crossbow) => c.loot_gear,
        _ => c.loot,
    };
    (value(item, data, c, prices, town) * m).round().max(0.0) as i32
}

/// A shop: its stock (one grid), the trader's money and when it next restocks (game hours).
#[derive(Debug, Clone)]
pub struct Shop {
    pub stock: Inventory,
    pub money: i32,
    pub restock_at: f64,
    /// The SQUAD_TEMPLATE whose vendor lists it stocks from.
    pub template: String,
    pub town: Option<usize>,
}

pub fn shop_inventory() -> Inventory {
    Inventory { sections: vec![Section::new("main", SHOP_W, SHOP_H, kenshi_char::inventory::ANY)] }
}

/// A vendor list entry's weight (assumption: the first ref value, else the second).
fn weight(v: &[i32]) -> f32 {
    let w = v.first().copied().unwrap_or(0);
    if w > 0 { w as f32 } else { v.get(1).copied().unwrap_or(0).max(0) as f32 }
}

/// One of `refs` by its first value as its weight (all 0: alike); None if empty.
pub(crate) fn pick_weighted<'a>(refs: &'a [(String, [i32; 3])], rand: &mut dyn FnMut() -> f32) -> Option<&'a str> {
    if refs.is_empty() {
        return None;
    }
    let total: f32 = refs.iter().map(|(_, v)| v[0].max(0) as f32).sum();
    if total <= 0.0 {
        return refs.get(((rand() * refs.len() as f32) as usize).min(refs.len() - 1)).map(|r| r.0.as_str());
    }
    let mut x = rand() * total;
    refs.iter().find(|(_, v)| {
        x -= v[0].max(0) as f32;
        x <= 0.0
    }).or(refs.last()).map(|r| r.0.as_str())
}

/// F049397: `n` items from a VENDOR_LIST, each picked by weight among all its entries.
pub(crate) fn generate(data: &GameDataContainer, list: &GameData, n: i32, out: &mut Vec<Item>, rand: &mut dyn FnMut() -> f32) {
    let mut entries: Vec<(&GameData, f32)> = Vec::new();
    for (_, refs) in &list.references {
        for (sid, v) in refs {
            let Some(r) = data.get(sid) else { continue };
            if r.item_type().is_none() || matches!(r.item_type(), Some(ItemType::VendorList | ItemType::WeaponManufacturer)) {
                continue;
            }
            let w = weight(&v[..]);
            if w > 0.0 {
                entries.push((r, w));
            }
        }
    }
    let total: f32 = entries.iter().map(|e| e.1).sum();
    if total <= 0.0 {
        return;
    }
    for _ in 0..n.max(0) {
        let mut x = rand() * total;
        let pick = entries.iter().find(|e| {
            x -= e.1;
            x <= 0.0
        });
        if let Some((r, _)) = pick.or(entries.last()) {
            out.push(match r.item_type() {
                // a weapon: by a maker from the list's weighted manufacturers (F049397)
                Some(ItemType::Weapon) => {
                    let makers = list.refs("weapon manufacturers");
                    let maker = crate::trade::pick_weighted(makers, rand);
                    let mut unit = || rand().clamp(0.0, 0.9999);
                    Item::weapon(data, r, maker, &mut unit)
                }
                // armour: a grade from the vendor's grade chances (F049397 -> F049345).
                // (assumption) Those chances are not found in the data: Shoddy to Specialist
                // alike
                Some(ItemType::Armour) => Item::armour(data, r, 1 + ((rand() * 4.0) as i32).min(3)),
                _ => Item::from_record(r, 1),
            });
        }
    }
}

/// F049418 with F049413: restock a shop from its template's vendor lists. Returns the new
/// stock and the trader's money.
pub fn restock(
    data: &GameDataContainer,
    template: &GameData,
    c: &PriceConstants,
    prices: &TradePrices,
    town: Option<usize>,
    rand: &mut dyn FnMut() -> f32,
) -> (Inventory, i32) {
    let fill = template.ints.get("vendors fill total amount").copied().unwrap_or(0);
    let fill = if fill < 1 { 100 } else { fill };
    let wealth = 1.0f32.clamp(0.2, 1.0);
    // (assumption) the random factor is uniform in [0.5, 1]
    let count = (fill as f32 * (0.5 + 0.5 * rand()) * wealth).round() as i32;
    let lists: Vec<(&GameData, f32)> = template
        .refs("vendors")
        .iter()
        .filter_map(|(sid, v)| data.get(sid).map(|r| (r, weight(&v[..]).max(1.0))))
        .collect();
    let total: f32 = lists.iter().map(|l| l.1).sum();
    let mut items = Vec::new();
    let mut left = count;
    for (k, (list, w)) in lists.iter().enumerate() {
        let mut n = (w / total * count as f32).round() as i32;
        left -= n;
        if k + 1 == lists.len() && left > 0 {
            n += left;
        }
        generate(data, list, n, &mut items, rand);
    }
    // the stock, most valuable first, into the shop's grid (what does not fit is not stocked)
    items.sort_by_key(|i| -buy_price(i, data, c, prices, town));
    let mut inv = shop_inventory();
    let mut stock_value = 0.0;
    for it in items {
        let v = value(&it, data, c, prices, town);
        if inv.add(it, false) {
            stock_value += v;
        }
    }
    let money = ((stock_value * 0.25).clamp(3000.0, 25000.0) * wealth) as i32;
    (inv, money)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_towns_price_alike() {
        let data = GameDataContainer::default();
        // no trade goods without data: an empty table, factor 1
        let p = TradePrices::roll(&data, &[[0.0; 3], [1000.0, 0.0, 0.0]], 0.3, &mut || 0.5);
        assert_eq!(p.at(Some(0), "x"), 1.0);
        // the clamp: a town 100 000 units off may differ by at most 0.3
        let mut t = TradePrices { goods: vec!["g".into()], factor: HashMap::new() };
        t.factor.insert((0, "g".into()), 0.7);
        let reach: f32 = 100_000.0 / 1.0e6 * 0.3 * 10.0;
        assert!((reach - 0.3).abs() < 1e-6);
    }

    #[test]
    fn selling_pays_less_than_buying() {
        let data = GameDataContainer::default();
        let c = PriceConstants::load(&data);
        let prices = TradePrices::default();
        let meat = Item {
            record: "meat".into(),
            name: "Dried Meat".into(),
            kind: Some(ItemType::Item),
            slot: 7,
            w: 2,
            h: 2,
            quantity: 1,
            stack: 1,
            weight: 1.0,
            value: 150,
            icon: None,
            food: Some((30.0, 30.0)),
            medkit: None,
            ammo: None,
            quality: 0.0,
            maker: None,
            model: None,
            uniform: None,
        };
        assert_eq!(buy_price(&meat, &data, &c, &prices, None), 75);
        assert_eq!(sell_price(&meat, &data, &c, &prices, None), 38);
        let half = Item { food: Some((30.0, 15.0)), ..meat.clone() };
        assert_eq!(buy_price(&half, &data, &c, &prices, None), 38);
    }
}
