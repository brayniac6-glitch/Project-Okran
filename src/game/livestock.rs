//! Buying animals (port_map `econ`): an animal trader's CHOOSE_ANIMALS_BUYING line (F040912's
//! action 0x30) opens "Buying Animals" (CharacterTrading_AnimalBuying, F041441) on the animals
//! it offers: of its faction, within 500 units of it and of its town, and its own squad's within
//! reach; none a pet already. Each costs (the line's price when it gives one, else 14000) x (its
//! age + 0.1) (F041441); its strength, toughness and age (percent) are shown. Bought (F041451),
//! it joins the player's squad.
//!
//! (assumption) "Within reach" of the trader for its squad's animals is taken as 500 units too,
//! and "a pet already" as one of the player's. Not ported: F041451's refusal when the squad is
//! at its size limit (no limit is kept here).
use crate::sim::Sim;

/// F040912: how far around the trader animals are offered (units).
const OFFER_RANGE: f32 = 500.0;
/// F041441: an animal's price before its age.
const BASE_PRICE: i32 = 14000;

impl Sim {
    /// The animals `trader` offers (see the module notes).
    pub fn animals_for_sale(&self, trader: usize) -> Vec<usize> {
        let t = &self.folk[trader];
        let at = t.mover.position_units();
        self.folk
            .iter()
            .filter(|p| p.race.is_some() && !p.player && !p.dead && !p.down && p.prison.is_none())
            .filter(|p| {
                let q = p.mover.position_units();
                let near = (q[0] - at[0]).hypot(q[2] - at[2]) < OFFER_RANGE;
                let squad = t.platoon.is_some() && p.platoon == t.platoon;
                near && (squad || (p.faction == t.faction && p.town == t.town))
            })
            .map(|p| p.id)
            .collect()
    }

    /// F041441: what animal `id` costs (`price` when the line set one).
    pub fn animal_price(&self, id: usize, price: i32) -> i32 {
        let base = if price > 0 { price } else { BASE_PRICE };
        ((self.folk[id].age + 0.1) * base as f32) as i32
    }

    /// F041451: buy animal `id` from `trader` for `price` into the player's squad. None when
    /// the money falls short.
    pub fn buy_animal(&mut self, id: usize, trader: usize, price: i32) -> Option<i32> {
        if self.money < price {
            return None;
        }
        self.money -= price;
        if let Some(s) = self.folk[trader].shop.as_mut() {
            s.money += price;
        }
        self.recruit(id);
        Some(price)
    }
}
