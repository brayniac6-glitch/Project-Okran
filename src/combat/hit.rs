//! Hit resolution: which body part a hit lands on, what the armour stops, toughness resistance,
//! then the damage into the medical system and the wound it opens.
//!
//! Specs: sheet rows F040476 resolveHit, F040594 armourCovers, F050502 randRange; types T00030
//! Armour, T00027 BodyPart, T00028 MedicalSystem.

use kenshi_char::medical::lerp;
use kenshi_char::{BodyPart, Constants, Damage, Medical, Stats};
use std::collections::HashMap;

/// @type Armour
///
/// The values of one worn armour piece that hit resolution reads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Armour {
    pub cut_resistance: f32,
    pub blunt_resistance: f32,
    /// Flat pierce damage stopped.
    pub pierce_reduction: f32,
    /// Share of stopped cut (and pierce) damage that becomes stun.
    pub cut_into_stun: f32,
    pub material: i32,
    /// Body part index -> chance this piece covers it on a hit.
    pub coverage: HashMap<usize, f32>,
}

impl Armour {
    /// @fn F040594
    ///
    /// `roll` is uniform in [0, 1).
    pub fn covers(&self, part: usize, roll: f32) -> bool {
        self.coverage.get(&part).is_some_and(|&c| roll <= c)
    }
}

/// One hit as the attack delivers it (F040476's damage vector).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hit {
    pub cut: f32,
    pub blunt: f32,
    pub pierce: f32,
    /// Bleed factor of the weapon.
    pub bleed: f32,
    /// Armour penetration: lowers the armour's resistances by this fraction.
    pub penetration: f32,
}

/// The GLOBAL CONSTANTS values hit resolution reads (T00025).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitConstants {
    pub damage_resistance_min: f32,
    pub damage_resistance_max: f32,
    /// "bleed rate" as stored (record value * 0.1).
    pub bleed_rate: f32,
}

impl Default for HitConstants {
    fn default() -> Self {
        HitConstants {
            damage_resistance_min: -0.65,
            damage_resistance_max: 0.65,
            bleed_rate: 0.01 * 0.1,
        }
    }
}

impl HitConstants {
    /// @type GlobalConstants
    pub fn from_floats(get: impl Fn(&str) -> Option<f32>) -> Self {
        let d = HitConstants::default();
        HitConstants {
            damage_resistance_min: get("damage resistance min").unwrap_or(d.damage_resistance_min),
            damage_resistance_max: get("damage resistance max").unwrap_or(d.damage_resistance_max),
            bleed_rate: get("bleed rate").map_or(d.bleed_rate, |v| v * 0.1),
        }
    }
}

/// The situation of the hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitContext {
    /// A low strike (technique "low strike"): arms and the head (kinds 2, 3) can't be hit.
    pub low_strike: bool,
    /// Attack direction code: 1 down, 5 up, 2/3 sideways, 6 ignores part focus.
    pub direction: i32,
    /// The victim is of a robot race (race flag +0x7c): every hit sounds as plate (material 3).
    pub robot: bool,
    /// The global damage factor (G2, 0x142133580).
    pub damage_mult: f32,
    pub race_bleed: f32,
}

/// What a resolved hit did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitResult {
    pub part: usize,
    /// The damage applied to the part.
    pub damage: Damage,
    /// Hit material (-1 = flesh); from the last covering armour piece, or 3.
    pub material: i32,
    /// Knockback strength: blunt * 1000 along [`knockback_dir`].
    pub knockback: f32,
    /// The wound opened (added to `Medical::wounds`).
    pub wound: f32,
    /// What the damage floater shows.
    pub floater: Floater,
}

/// The numbers a damage floater shows (F040476, after the armour and before toughness): the damage
/// through the armour and what the armour stopped.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Floater {
    pub cut: f32,
    pub blunt: f32,
    pub pierce: f32,
    pub stun: f32,
    pub cut_stopped: f32,
    pub blunt_stopped: f32,
    pub pierce_stopped: f32,
    /// The armour's blunt resistance (the full-info blunt bracket shows only when above 0).
    pub blunt_res: f32,
}

impl Floater {
    /// The floater's text: `full` is the [Full Info] setting (floaters 2), `part` the hit part's
    /// name. Normal: the total, and the total the armour stopped in brackets "(-n)". Full info:
    /// "part: " then the cut (with pierce when above 1) and its stopped bracket, then " + " the
    /// blunt and stun when the blunt is at least 1, with the blunt stopped.
    pub fn text(&self, full: bool, part: &str) -> String {
        let stopped = |v: f32| if 0.0 < v { format!(" (-{})", v as i32) } else { String::new() };
        if !full {
            let total = self.pierce + self.cut + self.blunt + self.stun;
            let held = (self.pierce_stopped + self.cut_stopped + self.blunt_stopped) as i32;
            return format!("{}{}", total as i32, stopped(held as f32));
        }
        let mut s = format!("{part}: ");
        if self.pierce <= 1.0 {
            s += &format!("{}{}", self.cut as i32, stopped(self.cut_stopped));
        } else {
            s += &format!("{}{}", (self.pierce + self.cut) as i32, stopped(self.pierce_stopped + self.cut_stopped));
        }
        if 1.0 <= self.blunt {
            s += &format!(" + {}", (self.blunt + self.stun) as i32);
            if 0.0 < self.blunt_res {
                s += &stopped(self.blunt_stopped);
            }
        }
        s
    }
}

fn alive(p: &BodyPart) -> bool {
    -p.max_health() < p.flesh
}

/// @fn F040476
///
/// `rand_range(a, b)` is uniform in [a, b) (F050502).
#[allow(clippy::too_many_arguments)]
pub fn resolve_hit(
    m: &mut Medical,
    hit: Hit,
    armour: &[Armour],
    stats: &Stats,
    ctx: &HitContext,
    hc: &HitConstants,
    c: &Constants,
    mut rand_range: impl FnMut(f32, f32) -> f32,
) -> HitResult {
    let i = pick_part(&m.parts, ctx, &mut rand_range);
    update_focus(&mut m.parts[i]);

    // armour over the part
    let mut material = -1;
    let (mut cut_res, mut blunt_res, mut flat, mut conv) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for a in armour {
        if a.coverage.contains_key(&i) && a.covers(i, rand_range(0.0, 1.0)) {
            let open = 1.0 - cut_res;
            cut_res += a.cut_resistance * open;
            flat += a.pierce_reduction;
            conv += a.cut_resistance * a.cut_into_stun * open;
            blunt_res += (1.0 - blunt_res) * a.blunt_resistance;
            material = a.material;
        }
    }
    let p = &m.parts[i];
    if ctx.robot || (p.is_limb() && p.slot == 2) {
        material = 3;
    }
    if hit.penetration != 0.0 {
        cut_res = (cut_res - hit.penetration * cut_res).min(0.9);
        blunt_res = (blunt_res - hit.penetration * blunt_res).min(0.9);
    }
    let mut d = Damage {
        cut: hit.cut - hit.cut * cut_res,
        blunt: hit.blunt - hit.blunt * blunt_res,
        pierce: hit.pierce - flat,
        stun: hit.cut * conv,
        bleed: hit.bleed,
    };
    if d.pierce < 0.0 {
        d.pierce = 0.0;
    }
    d.stun += (hit.pierce - d.pierce) * conv;
    let floater = Floater {
        cut: d.cut,
        blunt: d.blunt,
        pierce: d.pierce,
        stun: d.stun,
        cut_stopped: hit.cut * cut_res,
        blunt_stopped: hit.blunt * blunt_res,
        pierce_stopped: hit.pierce - d.pierce,
        blunt_res,
    };

    // toughness
    let t = (stats.toughness * stats.toughness_scale * 0.01).clamp(0.0, 1.0);
    let r = lerp(t, hc.damage_resistance_min, hc.damage_resistance_max);
    d.cut -= d.cut * r;
    d.blunt -= d.blunt * r;
    d.pierce -= d.pierce * r;
    d.stun -= d.stun * r;
    if d.cut < 0.0 {
        d.cut = 0.0;
    }
    if d.blunt < 0.0 {
        d.blunt = 0.0;
    }
    d.cut *= ctx.damage_mult;
    d.blunt *= ctx.damage_mult;
    d.pierce *= ctx.damage_mult;
    d.stun *= ctx.damage_mult;

    m.apply_damage(i, d, ctx.race_bleed, c);
    let wound = d.cut.max(d.pierce) * hc.bleed_rate * d.bleed;
    m.wounds.push(wound);
    HitResult {
        part: i,
        damage: d,
        material,
        knockback: d.blunt * 1000.0,
        wound,
        floater,
    }
}

/// The part a hit lands on: weighted by hit weight and focus among parts that can be hit.
fn pick_part(
    parts: &[BodyPart],
    ctx: &HitContext,
    rand_range: &mut impl FnMut(f32, f32) -> f32,
) -> usize {
    let mut table: Vec<(f32, usize)> = Vec::new();
    let mut total = 0.0;
    for (i, p) in parts.iter().enumerate() {
        if !alive(p)
            || (p.is_limb() && p.slot == 1)
            || (ctx.low_strike && (p.kind == 2 || p.kind == 3))
        {
            continue;
        }
        let mut w = p.hit_weight;
        if ctx.direction != 6 {
            w *= p.focus;
        }
        if 0.0 < w {
            total += w;
            table.push((total, i));
        }
    }
    match table.len() {
        0 => parts.len() - 1,
        1 => table[0].1,
        _ => {
            let r = rand_range(0.0, total);
            // the first part whose running total is above the roll
            table
                .iter()
                .find(|&&(k, _)| r < k)
                .map_or(parts.len() - 1, |&(_, i)| i)
        }
    }
}

fn update_focus(p: &mut BodyPart) {
    let cap = if alive(p) {
        if p.health_frac < 0.0 {
            if 2.0 < p.focus {
                p.focus = 2.0;
            }
            1.0
        } else {
            4.0
        }
    } else {
        p.focus = 0.0;
        -999.0
    };
    if p.focus < cap {
        p.focus += 1.0;
    }
}

/// Knockback direction for an attack direction code; `to_attacker` is the normalised vector from
/// the victim to the attacker.
pub fn knockback_dir(direction: i32, to_attacker: [f32; 3]) -> [f32; 3] {
    let [x, _, z] = to_attacker;
    match direction {
        1 => [0.0, -1.0, 0.0],
        5 => [0.0, 1.0, 0.0],
        2 => [-z, 0.0, x],
        3 => [z, 0.0, -x],
        _ => to_attacker,
    }
}

/// Which side a hit came from: 2 when victim and attacker face within a quarter turn of each other
/// (a hit from behind), else 1.
pub fn hit_side(victim_yaw: f32, attacker_yaw: f32) -> i32 {
    let d = (victim_yaw - attacker_yaw).abs();
    if d < std::f32::consts::FRAC_PI_2 || std::f32::consts::PI * 1.5 < d {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(weight: f32) -> BodyPart {
        BodyPart {
            hit_weight: weight,
            focus: 1.0,
            flesh: 100.0,
            max_a: 100.0,
            max_b: 1.0,
            max_c: 1.0,
            ..Default::default()
        }
    }

    fn ctx() -> HitContext {
        HitContext {
            low_strike: false,
            direction: 0,
            robot: false,
            damage_mult: 1.0,
            race_bleed: 1.0,
        }
    }

    #[test]
    fn parts_are_picked_by_weight_and_focus() {
        let mut parts = vec![part(1.0), part(3.0), part(1.0)];
        // running totals 1, 4, 5
        assert_eq!(pick_part(&parts, &ctx(), &mut |_, _| 0.5), 0);
        assert_eq!(pick_part(&parts, &ctx(), &mut |_, _| 1.0), 1);
        assert_eq!(pick_part(&parts, &ctx(), &mut |_, _| 4.5), 2);
        parts[1].focus = 0.0; // focus 0 drops the part, unless the direction ignores focus
        assert_eq!(pick_part(&parts, &ctx(), &mut |_, _| 1.5), 2);
        let d6 = HitContext {
            direction: 6,
            ..ctx()
        };
        assert_eq!(pick_part(&parts, &d6, &mut |_, _| 1.5), 1);
        // lost limbs and destroyed parts can't be hit
        parts[0].kind = 1;
        parts[0].slot = 1;
        parts[2].flesh = -200.0;
        parts[1].focus = 1.0;
        assert_eq!(pick_part(&parts, &ctx(), &mut |_, _| unreachable!()), 1);
    }

    #[test]
    fn focus_grows_with_hits() {
        let mut p = part(1.0);
        for _ in 0..5 {
            update_focus(&mut p);
        }
        assert_eq!(p.focus, 4.0);
        p.health_frac = -0.5;
        update_focus(&mut p);
        assert_eq!(p.focus, 2.0);
        p.flesh = -200.0;
        update_focus(&mut p);
        assert_eq!(p.focus, 0.0);
    }

    #[test]
    fn armour_and_toughness_reduce_damage() {
        let c = Constants::from_record(&Default::default());
        let mut m = Medical {
            blood: 100.0,
            max_blood: 100.0,
            parts: vec![part(1.0)],
            ..Default::default()
        };
        let plate = Armour {
            cut_resistance: 0.5,
            blunt_resistance: 0.25,
            pierce_reduction: 5.0,
            cut_into_stun: 0.5,
            material: 7,
            coverage: HashMap::from([(0, 1.0)]),
        };
        let hit = Hit {
            cut: 40.0,
            blunt: 20.0,
            pierce: 3.0,
            bleed: 1.0,
            penetration: 0.0,
        };
        // toughness 50: resistance lerp(0.5, -0.65, 0.65) = 0
        let stats = Stats {
            toughness: 50.0,
            toughness_scale: 1.0,
        };
        let hc = HitConstants::default();
        let r = resolve_hit(&mut m, hit, &[plate], &stats, &ctx(), &hc, &c, |a, _| a);
        assert_eq!(r.part, 0);
        assert_eq!(r.material, 7);
        assert_eq!(r.damage.cut, 20.0);
        assert_eq!(r.damage.blunt, 15.0);
        assert_eq!(r.damage.pierce, 0.0);
        // stun: 40 * 0.25 from cut + all 3 pierce * 0.25
        assert_eq!(r.damage.stun, 10.0 + 0.75);
        assert_eq!(r.knockback, 15000.0);
        assert!((r.wound - 20.0 * hc.bleed_rate).abs() < 1e-9);
        assert_eq!(m.wounds.len(), 1);
        // no toughness: damage taken * 1.65
        let weak = Stats {
            toughness: 0.0,
            toughness_scale: 1.0,
        };
        let r = resolve_hit(&mut m, hit, &[], &weak, &ctx(), &hc, &c, |a, _| a);
        assert!((r.damage.cut - 40.0 * 1.65).abs() < 1e-4);
    }

    #[test]
    fn directions_and_sides() {
        assert_eq!(knockback_dir(2, [1.0, 0.0, 0.0]), [-0.0, 0.0, 1.0]);
        assert_eq!(knockback_dir(5, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
        assert_eq!(hit_side(0.0, 0.1), 2);
        assert_eq!(hit_side(0.0, 3.0), 1);
    }
}
