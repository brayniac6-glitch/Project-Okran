//! Weather and ground that hurt (port_map `weather`): CharacterHuman's weather update (F039251)
//! and the MedicalSystem's hurts it calls (F040446..F040451).
//!
//! Each frame, for every human not dead or away (WeatherAffecting: 1 dust, 2 acid, 3 burning,
//! 4 gas; a race's "weather immunity" types pass it by):
//! - acidic ground (the BIOME_GROUP's "acidic ground"): outdoors, not carried or held, with no
//!   boots (slot 9), both legs burn (F040447: 0.2 x level a second, blunt), each only while its
//!   flesh is over 50; "Feet Burning!".
//! - the effects touching it (`Harm`, a running EFFECT with an "affect type" within its "effect
//!   radius"): burning (F040450: 10 x strength a second as cut on every part) and poison gas
//!   (F040449: 4 x strength a second, blunt, on the chest), each less its gear's protection.
//! - outdoors, its region's weather (`Climate`): acid rain (F040448: 0.08 a second, blunt on
//!   every part, less the gear's protection) unless under a roof that is an acid shelter
//!   (BUILDING "is shelter"); a dust storm (F040451): its strength less the gear's protection is
//!   the dust its aim suffers (`Person::dust`).
//!
//! Gear's protection (F047069): the sum of the worn clothing's "weather protection amount" whose
//! "weather protection" types list the hurt. Robotic limbs take no acid, burning or gas
//! (F037455). Blunt hurts the flesh by GLOBAL CONSTANTS "blunt permanent organ damage" and the
//! rest stuns (F040421).
//!
//! - swimming (`Person::water` 3) in acidic water (the BIOME_GROUP's "acidic water", F040446):
//!   0.25 x level a second, blunt on every part; "Acid!".
//!
//! (stand-in) The medical system's
//! flag that spares a character gas (+0x161) is not identified: races go by their weather
//! immunities. The shelter's roof is
//! taken as within 400 units of the shelter building (the game casts a ray up from the
//! character to the roof above it).
use crate::sim::Sim;
use kenshi_char::medical::Damage;
use std::collections::HashMap;

/// WeatherAffecting.
pub const WA_DUST: i32 = 1;
pub const WA_ACID: i32 = 2;
pub const WA_BURNING: i32 = 3;
pub const WA_GAS: i32 = 4;

/// What a zone's weather region brings now: its weather's "affect type" and strength, and its
/// BIOME_GROUP's "acidic ground" / "acidic water".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Climate {
    pub affect: i32,
    pub strength: f32,
    pub acid_ground: f32,
    pub acid_water: f32,
}

/// A running effect that hurts: its "affect type", where it is (world units), its reach and
/// strength.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Harm {
    pub affect: i32,
    pub at: [f32; 3],
    pub radius: f32,
    pub strength: f32,
}

/// A hurt felt this frame, for the status box.
#[derive(Debug, Clone, PartialEq)]
pub struct Hazard {
    pub affect: i32,
    /// The gear's protection (0..1).
    pub protection: f32,
    /// Acidic ground rather than weather.
    pub ground: bool,
}

impl Hazard {
    /// The status line, as the game words it (F040447..F040451).
    pub fn text(&self) -> String {
        let pct = (self.protection.clamp(0.0, 1.0) * 100.0).round();
        match (self.affect, self.ground) {
            (WA_ACID, true) => "Feet Burning!".into(),
            (WA_ACID, false) => format!("Acid Rain ({pct}% protection)"),
            (WA_BURNING, _) => format!("Burning ({pct}% protection)"),
            (WA_GAS, _) => format!("Poison Gas ({pct}% protection)"),
            _ => format!("Dust Storm ({pct}% protection)"),
        }
    }
}

impl Sim {
    /// A race's "weather immunity" types.
    pub(crate) fn immune(&self, id: usize, affect: i32) -> bool {
        let race = self.race_of(id);
        race.is_some_and(|r| self.weather_kinds_of(&r, "weather immunity").0.contains(&affect))
    }

    /// Record `sid`'s weather types under `key` ("weather protection" / "weather immunity") and
    /// its "weather protection amount", read once.
    fn weather_kinds_of(&self, sid: &str, key: &str) -> (Vec<i32>, f32) {
        let mut cache = self.weather_kinds.lock().unwrap();
        if let Some(v) = cache.get(sid) {
            return v.clone();
        }
        let v = self.data.get(sid).map_or((Vec::new(), 0.0), |r| {
            (r.ints.iter().filter(|(k, _)| k.starts_with(key)).map(|(_, v)| *v).collect(), r.floats.get("weather protection amount").copied().unwrap_or(0.0))
        });
        cache.insert(sid.to_string(), v.clone());
        v
    }

    /// @fn F047069
    ///
    /// Person `id`'s worn gear's protection from `affect`.
    pub fn weather_protection(&self, id: usize, affect: i32) -> f32 {
        self.folk[id]
            .inventory
            .equipped()
            .map(|i| self.weather_kinds_of(&i.record, "weather protection"))
            .filter(|(kinds, _)| kinds.contains(&affect))
            .map(|(_, amount)| amount)
            .fold(0.0, |a, b| a + b)
    }

    /// Whether person `id` stands under an acid shelter.
    fn sheltered(&mut self, id: usize) -> bool {
        if self.shelters.is_none() {
            let set: std::collections::HashSet<String> =
                self.data.of_type(kenshi_data::ItemType::Building).filter(|b| b.bools.get("is shelter").copied().unwrap_or(false)).map(|b| b.string_id.clone()).collect();
            self.shelters = Some(set);
        }
        if self.shelters.as_ref().is_some_and(|s| s.is_empty()) {
            return false;
        }
        // the placed shelters' spots, taken once per placements loaded (asked every tick for
        // everyone out in acid rain)
        if self.shelter_spots.0 != self.placed.len() {
            let set = self.shelters.as_ref().unwrap();
            let spots = self.placed.iter().zip(&self.placed_at).filter(|(p, _)| set.contains(&p.building)).map(|(_, at)| *at).collect();
            self.shelter_spots = (self.placed.len(), spots);
        }
        let set = self.shelters.as_ref().unwrap();
        let q = self.folk[id].mover.position_units();
        let near = |at: [f32; 3]| (at[0] - q[0]).hypot(at[2] - q[2]) < 400.0;
        self.shelter_spots.1.iter().any(|at| near(*at)) || self.buildings.iter().any(|b| b.confirmed && set.contains(&b.record) && near(b.at))
    }

    /// Blunt (or cut) `amount` on part `k` of person `id`, unless it is a robotic limb.
    fn hurt_part(&mut self, id: usize, k: usize, amount: f32, cut: bool) {
        let organ = self.arena.sh.mc.blunt_permanent_organ_damage;
        let part = &self.arena.f[id].medical.parts[k];
        let slot = part.limb_slot();
        if part.is_limb() && self.folk[id].limbs.get(slot as usize).is_some_and(|l| l.is_some()) {
            return;
        }
        let d = if cut { Damage { cut: amount, ..Default::default() } } else { Damage { blunt: amount, ..Default::default() } };
        self.arena.f[id].medical.parts[k].damage(d, organ);
    }

    /// The weather's and the ground's hurts this frame (F039251).
    pub fn hazards_tick(&mut self, dt: f32) {
        if dt <= 0.0 || (self.climate.is_empty() && self.harms.is_empty()) {
            for p in self.folk.iter_mut() {
                p.dust = 0.0;
                p.hazard = None;
            }
            return;
        }
        let harms = self.harms.clone();
        for id in 0..self.folk.len() {
            self.folk[id].dust = 0.0;
            self.folk[id].hazard = None;
            let p = &self.folk[id];
            if p.dead || p.away || p.race.is_some() {
                continue;
            }
            let q = p.mover.position_units();
            let outdoors = p.inside.is_none();
            let climate = self.climate.get(&kenshi_world::zone_of(q[0], q[2])).copied().unwrap_or_default();
            let mut hazard = None;
            // acidic ground: bare feet outdoors (F040447)
            if climate.acid_ground > 0.0
                && outdoors
                && p.carried_by.is_none()
                && p.prison.is_none()
                && !p.inventory.equipped().any(|i| i.slot == 9)
                && !self.immune(id, WA_ACID)
            {
                let amount = dt * climate.acid_ground * 0.2;
                let legs: Vec<usize> = (0..self.arena.f[id].medical.parts.len()).filter(|&k| self.arena.f[id].medical.parts[k].kind == 1).collect();
                for k in legs {
                    if self.arena.f[id].medical.parts[k].flesh > 50.0 {
                        self.hurt_part(id, k, amount, false);
                    }
                }
                hazard = Some(Hazard { affect: WA_ACID, protection: 0.0, ground: true });
            }
            // swimming in acidic water (F040446)
            if climate.acid_water > 0.0 && self.folk[id].water == 3 && !self.immune(id, WA_ACID) {
                let amount = dt * climate.acid_water * 0.25;
                for k in 0..self.arena.f[id].medical.parts.len() {
                    self.hurt_part(id, k, amount, false);
                }
                hazard = Some(Hazard { affect: WA_ACID, protection: 0.0, ground: true });
            }
            // the hurting effects it is in: burning, gas
            for h in &harms {
                if (h.at[0] - q[0]).hypot(h.at[2] - q[2]) > h.radius || (h.affect != WA_BURNING && h.affect != WA_GAS) || self.immune(id, h.affect) {
                    continue;
                }
                let prot = self.weather_protection(id, h.affect);
                let parts = self.arena.f[id].medical.parts.len();
                if h.affect == WA_BURNING {
                    let amount = dt * h.strength * 10.0 * (1.0 - prot);
                    if amount > 0.0 {
                        for k in 0..parts {
                            self.hurt_part(id, k, amount, true);
                        }
                    }
                } else {
                    let amount = dt * h.strength * 4.0 * (1.0 - prot);
                    let chest = (0..parts).find(|&k| self.arena.f[id].medical.parts[k].kind == 0);
                    if let (true, Some(k)) = (amount > 0.0, chest) {
                        self.hurt_part(id, k, amount, false);
                    }
                }
                hazard = Some(Hazard { affect: h.affect, protection: prot, ground: false });
            }
            // the region's weather, outdoors
            if outdoors && climate.affect != 0 && climate.strength > 0.0 && !self.immune(id, climate.affect) {
                let prot = self.weather_protection(id, climate.affect);
                match climate.affect {
                    WA_DUST => {
                        let dust = (climate.strength * (1.0 - prot)).clamp(0.0, 1.0);
                        self.folk[id].dust = dust;
                        if hazard.is_none() {
                            hazard = Some(Hazard { affect: WA_DUST, protection: prot, ground: false });
                        }
                    }
                    WA_ACID if !self.sheltered(id) => {
                        let amount = dt * 0.08 * (1.0 - prot);
                        if amount > 0.0 {
                            for k in 0..self.arena.f[id].medical.parts.len() {
                                self.hurt_part(id, k, amount, false);
                            }
                        }
                        hazard = Some(Hazard { affect: WA_ACID, protection: prot, ground: false });
                    }
                    _ => {}
                }
            }
            self.folk[id].hazard = hazard;
        }
    }

}

/// The zones round the player's characters (their own and the eight about), for the game to
/// fill `Sim::climate` with.
pub fn zones_round(sim: &Sim) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for p in sim.folk.iter().filter(|p| p.player && !p.dead) {
        let q = p.mover.position_units();
        let (zx, zy) = kenshi_world::zone_of(q[0], q[2]);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if !out.contains(&(zx + dx, zy + dy)) {
                    out.push((zx + dx, zy + dy));
                }
            }
        }
    }
    out
}

/// A zone map of climates.
pub type Climates = HashMap<(i32, i32), Climate>;
