//! NPC shopping (port_map `ai`): a character's shopping list (the AI's +0x2b0 object), its
//! resets, the shop finder of the packages' Shopping (118) and Task_Shopping (119), which buys
//! at the shop counter.
//!
//! Specs: F050305 (the list from the CHARACTER record's "shopping" references), F050254 /
//! F050260 (the daily reset), F050253 (the reset in a new town), F050255 (condition 85: the list
//! done), F050261 (Shopping's score), F050263 (its finder) with F051403 / F051401 / F037733 /
//! F050245 and the fallback F051404 / F051400 / F037699, F030990 / F030927 (Task_Shopping's
//! start and update), F050309 (a purchase) and F043583 (one item off the counter).
//!
//! The plan: Shopping (118, generic) wants condition 85 on the target its finder gives (a shop
//! counter), which Task_Shopping (119) brings about once Moving has taken the shopper there.
//! Task_Shopping buys one item a second until a purchase fails; a shop of a faction not the
//! player's keeps the money and, four times in five, the item never reaches the buyer
//! (F043583's 0.2: it vanishes, paid for).

use crate::sim::{SHOPPING_VISIT, Sim, Target};
use kenshi_data::{GameData, GameDataContainer, ItemType};
use serde::{Deserialize, Serialize};

/// BuildingFunction BF_SHOP: a shop counter (BUILDING_FUNCTIONALITY "shop counter").
pub const BF_SHOP: i32 = 9;
/// Task_Shopping's pause between purchases (F030990 / F030927, seconds).
const PURCHASE_EVERY: f32 = 1.0;
/// F043583: a shop not the player's keeps a purchase the buyer cannot carry above this roll.
const VANISH_ROLL: f32 = 0.2;
/// F050260: the daily reset waits for the hour after 7.
const RESET_HOUR: i32 = 7;

/// One line of a shopping list (12 bytes): the "item function" wanted, how many, and how many
/// bought since the last reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopLine {
    pub function: i32,
    pub wanted: i32,
    pub bought: i32,
}

/// A character's shopping list: its lines, the day of its last reset (+0x28) and the town it
/// was last reset for (+0x30).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShoppingList {
    pub lines: Vec<ShopLine>,
    pub day: i64,
    pub town: Option<usize>,
}

impl ShoppingList {
    /// @fn F050305
    ///
    /// The list of a CHARACTER record: its "shopping" references (an ITEM, values: how many,
    /// the chance in percent, the group) group by group from 0 until a group has none, each
    /// line kept when `roll(0, 100)` comes within its chance, as the item's "item function".
    /// A record with no references: food (3), narcotics (8) and first aid (1), one of each on a
    /// coin flip.
    pub fn from_record(data: &GameDataContainer, record: &GameData, roll: &mut dyn FnMut(i32, i32) -> i32, coin: &mut dyn FnMut() -> bool) -> ShoppingList {
        let refs = record.refs("shopping");
        let mut lines = vec![];
        let mut group = 0;
        loop {
            let mut any = false;
            for (sid, [wanted, chance, g]) in refs {
                if *g != group {
                    continue;
                }
                any = true;
                if *chance < roll(0, 100) {
                    continue;
                }
                if let Some(item) = data.get(sid).filter(|r| r.item_type() == Some(ItemType::Item)) {
                    let function = item.ints.get("item function").copied().unwrap_or(0);
                    lines.push(ShopLine { function, wanted: *wanted, bought: 0 });
                }
            }
            if !any {
                break;
            }
            group += 1;
        }
        if refs.is_empty() {
            for function in [3, 8, 1] {
                if coin() {
                    lines.push(ShopLine { function, wanted: 1, bought: 0 });
                }
            }
        }
        ShoppingList { lines, day: 0, town: None }
    }

    /// @fn F050255
    ///
    /// Condition 85: every line bought. A list last reset for another town than the one the
    /// character is in (Character +0x88; (assumption) Town vtable +0x268, which F050253
    /// stores, answers the town itself) starts over first (F050254).
    pub fn done(&mut self, town: Option<usize>, day: i64) -> bool {
        if self.town.is_some() && town.is_some() && self.town != town {
            self.reset(day);
        }
        self.lines.iter().all(|l| l.wanted <= l.bought)
    }

    /// @fn F050254
    pub fn reset(&mut self, day: i64) {
        self.day = day;
        for l in &mut self.lines {
            l.bought = 0;
        }
    }

    /// @fn F050260
    ///
    /// A new day, once the hour is past 7: the day's purchases start over.
    pub fn daily(&mut self, day: i64, hour: i32) {
        if self.day != day && hour > RESET_HOUR {
            self.reset(day);
        }
    }

    /// @fn F050253
    ///
    /// Shopping in `town` (the shopper's squad's town): a list last reset for another starts
    /// over (the day kept).
    pub fn set_town(&mut self, town: Option<usize>) {
        if self.town != town {
            self.town = town;
            for l in &mut self.lines {
                l.bought = 0;
            }
        }
    }

    /// The item functions still wanted, in the list's order.
    pub fn wants(&self) -> Vec<i32> {
        self.lines.iter().filter(|l| l.bought < l.wanted).map(|l| l.function).collect()
    }
}

/// Task_Shopping's state (F030990): the time to the next purchase and whether one was made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShopVisit {
    pub timer: f32,
    pub bought: bool,
}

/// A shop counter in town: its container (`loot::BOX_OWNER` index), the town and the item
/// functions its stock holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Counter {
    pub k: usize,
    pub town: Option<usize>,
    pub functions: Vec<i32>,
}

/// A building of a town Shopping falls back on (F051400): its placement, the town, its first
/// shop counter, else (stand-in for the building itself, which is no target here) a door.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicBuilding {
    pub town: Option<usize>,
    pub counter: Option<usize>,
    pub door: Option<usize>,
}

/// An item's "item function" (Item +0x124, from its record).
pub fn item_function(data: &GameDataContainer, record: &str) -> i32 {
    data.get(record).and_then(|r| r.ints.get("item function").copied()).unwrap_or(0)
}

impl Sim {
    /// Whether container `k` is a shop counter (its record's functionality BF_SHOP).
    pub fn is_counter(&self, k: usize) -> bool {
        self.boxes.get(k).and_then(|b| self.data.get(&b.at.record)).and_then(|r| kenshi_world::furniture::functionality(&self.data, r)).is_some_and(|f| f.0 == BF_SHOP)
    }

    /// The towns' shop counters with what their stock sells (F051401's candidates: a shop
    /// counter is a shop of its own, F050245, whose stock F037733 searches).
    pub fn counters(&self) -> Vec<Counter> {
        (0..self.boxes.len())
            .filter(|&k| self.is_counter(k))
            .map(|k| {
                let mut functions: Vec<i32> = self
                    .inv(crate::loot::BOX_OWNER + k)
                    .map(|i| i.sections.iter().flat_map(|s| s.items.iter()).map(|p| item_function(&self.data, &p.item.record)).collect())
                    .unwrap_or_default();
                functions.sort_unstable();
                functions.dedup();
                let town = self.placed_town.get(self.boxes[k].at.placement).copied().flatten();
                Counter { k, town, functions }
            })
            .collect()
    }

    /// F051400's buildings: the towns' buildings open by day ("is public day", Building +0xc8,
    /// F037699), in towns not the player's (F050263 falls back only there). (not ported)
    /// F037699's exception for a building of type 1 or 34.
    pub fn public_buildings(&self) -> Vec<PublicBuilding> {
        let counters: Vec<usize> = (0..self.boxes.len()).filter(|&k| self.is_counter(k)).collect();
        let mut out = vec![];
        for (pi, p) in self.placed.iter().enumerate() {
            let Some(town) = self.placed_town.get(pi).copied().flatten() else { continue };
            if self.towns[town].faction.as_deref() == Some(crate::sim::PLAYER_FACTION) {
                continue;
            }
            let public = self.data.get(&p.building).and_then(|r| r.bools.get("is public day").copied()).unwrap_or(false);
            if !public {
                continue;
            }
            let counter = counters.iter().copied().find(|&k| self.boxes[k].at.placement == pi);
            let door = self.door_states.iter().position(|d| d.door.placement == pi);
            out.push(PublicBuilding { town: Some(town), counter, door });
        }
        out
    }

    /// The day and hour of the game clock (the global time's +8 and +0).
    pub fn day_hour(&self) -> (i64, i32) {
        let clock = self.start_hour + self.hours_gone();
        ((clock / 24.0).floor() as i64, clock.rem_euclid(24.0) as i32)
    }

    /// The shopping lists made as the characters' AIs are (F040025 -> F050306 -> F050305):
    /// (assumption) here on their first tick, once their CHARACTER record is known.
    fn make_shopping_lists(&mut self) {
        for i in 0..self.folk.len() {
            if self.folk[i].shopping.is_some() {
                continue;
            }
            let Some(record) = self.folk[i].character.clone().and_then(|c| self.data.get(&c).cloned()) else { continue };
            let rng = &mut self.rng;
            let mut list = {
                let mut roll = |lo: i32, hi: i32| lo + ((rng.next() * (hi - lo + 1) as f32) as i32).min(hi - lo);
                let r = std::cell::RefCell::new(&mut roll);
                let mut flip = || (r.borrow_mut())(0, 32767) as f32 / 32768.0 > 0.5;
                let list = ShoppingList::from_record(&self.data, &record, &mut |a, b| (r.borrow_mut())(a, b), &mut flip);
                list
            };
            list.day = self.day_hour().0;
            self.folk[i].shopping = Some(list);
        }
    }

    /// @fn F050309
    ///
    /// One purchase of `pid` at counter `k`: the list starts over in a new town (F050253); the
    /// first line still wanted that the counters of the counter's building sell (F037759 on the
    /// building's interior, BF_SHOP) gets one of a random stack of that item function, the line
    /// counting it bought whether or not it is had (F043583 may refuse it); a line no counter
    /// sells is passed over. Returns whether an item was bought. (not ported) The price
    /// floater ("c.{1,num}") and the "Sell_Item" sound.
    pub fn shop_purchase(&mut self, pid: usize, k: usize) -> bool {
        let town = self.folk[pid].town;
        let Some(list) = self.folk[pid].shopping.as_mut() else { return false };
        list.set_town(town);
        let Some(building) = self.boxes.get(k).map(|b| b.at.placement) else { return false };
        let counters: Vec<usize> = (0..self.boxes.len()).filter(|&c| self.boxes[c].at.placement == building && self.is_counter(c)).collect();
        let mut tried: Vec<i32> = vec![];
        let mut line = self.folk[pid].shopping.as_ref().and_then(|l| l.lines.iter().position(|l| l.bought < l.wanted));
        'lines: while let Some(n) = line {
            let function = self.folk[pid].shopping.as_ref().map_or(0, |l| l.lines[n].function);
            'again: loop {
                for &c in &counters {
                    let stacks: Vec<(usize, usize)> = self
                        .inv(crate::loot::BOX_OWNER + c)
                        .map(|i| {
                            i.sections
                                .iter()
                                .enumerate()
                                .flat_map(|(s, sec)| sec.items.iter().enumerate().map(move |(x, p)| (s, x, p)))
                                .filter(|(_, _, p)| item_function(&self.data, &p.item.record) == function)
                                .map(|(s, x, _)| (s, x))
                                .collect()
                        })
                        .unwrap_or_default();
                    if stacks.is_empty() {
                        continue;
                    }
                    let pick = stacks[((self.rng.next() * stacks.len() as f32) as usize).min(stacks.len() - 1)];
                    let list = self.folk[pid].shopping.as_mut().expect("list");
                    list.lines[n].bought += 1;
                    let (bought, wanted) = (list.lines[n].bought, list.lines[n].wanted);
                    if self.buy_one(pid, c, pick) {
                        return true;
                    }
                    if bought < wanted {
                        continue 'again;
                    }
                    break 'again;
                }
                tried.push(function);
                break;
            }
            line = self.folk[pid].shopping.as_ref().and_then(|l| l.lines.iter().position(|l| l.bought < l.wanted && !tried.contains(&l.function)));
            if line.is_none() {
                break 'lines;
            }
        }
        false
    }

    /// @fn F043583
    ///
    /// One of stack `at` of counter `c` to `pid`: its price (Item vtable +0x290; (assumption)
    /// the price the player pays) goes to the counter's trader. A shop not the player's keeps
    /// the item (paid for) when a roll comes above 0.2, else the buyer must carry it: no room
    /// and the item goes back, unsold. (not ported) The buyer's money (Inventory +0x80): the
    /// rewrite's NPCs carry none, so they always afford it, and the "can't afford" / "can't
    /// carry" messages.
    fn buy_one(&mut self, pid: usize, c: usize, at: (usize, usize)) -> bool {
        let owner = crate::loot::BOX_OWNER + c;
        let trader = self.counter_trader(c);
        let town = trader.and_then(|t| self.folk[t].shop.as_ref()).and_then(|s| s.town);
        let Some(inv) = self.inv_mut(owner) else { return false };
        let Some(mut item) = inv.take(at.0, at.1) else { return false };
        if item.quantity > 1 {
            let mut rest = item.clone();
            rest.quantity -= 1;
            item.quantity = 1;
            inv.add(rest, false);
        }
        let price = self.price(&item, town, true);
        let players = self.box_faction(c).as_deref() == Some(crate::sim::PLAYER_FACTION);
        let vanish = !players && self.rng.next() > VANISH_ROLL;
        let p = &mut self.folk[pid];
        if vanish || p.inventory.add(item.clone(), false) || p.inventory.bag_mut().is_some_and(|b| b.add(item.clone(), false)) {
            if let Some(shop) = trader.and_then(|t| self.folk[t].shop.as_mut()) {
                shop.money += price;
            }
            if std::env::var("KENSHI_SHOP_DEBUG").is_ok() {
                eprintln!("shopping: {pid} bought {} at counter {c} for c.{price}{}", item.name, if vanish { " (kept by the shop)" } else { "" });
            }
            return true;
        }
        if let Some(inv) = self.inv_mut(owner) {
            inv.add(item, false);
        }
        false
    }

    /// @fn F030927
    ///
    /// Task_Shopping's update for everyone at it: turned to the counter, a purchase every
    /// second while they succeed; the first to fail ends the task, done when it bought anything.
    /// Its start (F030990) takes a shop counter target (a building's nearest counter is not
    /// searched for: (not ported) the fallback's buildings have none).
    pub fn shopping_tick(&mut self, dt: f32) {
        self.make_shopping_lists();
        for pid in 0..self.folk.len() {
            let p = &self.folk[pid];
            let Some(step) = p.ai.queue.first() else { continue };
            if step.task != SHOPPING_VISIT || p.down || p.away {
                continue;
            }
            let k = match step.target {
                Target::Store(o) => o.checked_sub(crate::loot::BOX_OWNER).filter(|&k| self.is_counter(k)),
                _ => None,
            };
            let Some(k) = k else {
                let p = &mut self.folk[pid];
                p.ai.queue.clear();
                p.started = false;
                p.shop_visit = None;
                continue;
            };
            let at = self.boxes[k].at.position;
            let p = &mut self.folk[pid];
            if !p.started {
                p.shop_visit = Some(ShopVisit { timer: PURCHASE_EVERY, bought: false });
                p.started = true;
            }
            let me = p.mover.position_units();
            let (dx, dz) = (at[0] - me[0], at[2] - me[2]);
            let len = dx.hypot(dz);
            if len > 1e-3 {
                p.facing = [dx / len, 0.0, dz / len];
            }
            let Some(visit) = p.shop_visit.as_mut() else { continue };
            visit.timer -= dt;
            if visit.timer > 0.0 {
                continue;
            }
            let bought = self.shop_purchase(pid, k);
            let p = &mut self.folk[pid];
            let visit = p.shop_visit.as_mut().expect("visit");
            if bought {
                visit.bought = true;
                visit.timer = PURCHASE_EVERY;
                continue;
            }
            // done (+0x26c) after a purchase, else ended (F036890)
            if visit.bought {
                p.ai.queue.remove(0);
            } else {
                p.ai.queue.clear();
            }
            p.started = false;
            p.shop_visit = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resets() {
        let mut l = ShoppingList { lines: vec![ShopLine { function: 3, wanted: 2, bought: 2 }, ShopLine { function: 8, wanted: 1, bought: 0 }], day: 4, town: Some(1) };
        assert!(!l.done(Some(1), 4));
        assert_eq!(l.wants(), vec![8]);
        l.lines[1].bought = 1;
        assert!(l.done(Some(1), 4));
        // the same day, or before 8 on the next: kept
        l.daily(4, 12);
        l.daily(5, 7);
        assert!(l.done(Some(1), 5));
        l.daily(5, 8);
        assert_eq!(l.day, 5);
        assert_eq!(l.wants(), vec![3, 8]);
        // another town: starts over
        l.lines.iter_mut().for_each(|x| x.bought = x.wanted);
        assert!(!l.done(Some(2), 5));
        l.lines.iter_mut().for_each(|x| x.bought = x.wanted);
        l.set_town(Some(2));
        assert_eq!(l.wants(), vec![3, 8]);
    }
}
