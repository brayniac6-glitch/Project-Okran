//! Finding an enemy to attack (port_map `ai`): the "Attacking enemies" hook's target.
//!
//! Spec: F038641 AI::findEnemy (the known characters seen or detected that are not carried, not
//! lying down, not allied, inside the AI's area and its hold leash; the first, or the squad's best
//! by F038638), F038539 the AI's area test, F038560 the hold-area leash, F038606 the area test with
//! its exceptions (my squad's target squad, someone swinging at me).
//!
//! Not ported here: the squad's scoring of the candidates (F038638, which needs the squad's tuning
//! object): callers pass a score, and the harness scores by nearness. The prisoner branch
//! (F030709 / F039148: a character my faction holds counts only once let out) is left to the caller.

/// The AI's area of interest (AI +0x2bc mode, +0x2c0 centre (x, z), +0x8 / +0xc radii).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Area {
    /// Mode 0: a circle around a fixed centre.
    Fixed { centre: [f32; 2], r1: f32, r2: f32 },
    /// Mode 1: a wider circle around a fixed centre.
    Wide { centre: [f32; 2], r1: f32, r2: f32 },
    /// Mode 2: around the character itself (the centre follows it).
    Me,
}

/// @fn F038539
///
/// Whether a point lies inside the AI's area: the zero point always does; mode 2 within 6517 units
/// (squared 4.24673e7) of me; mode 0 within r1 + 40 + r2 of the centre (or, as the original
/// compares, within the squared square of my own distance from it); mode 1 within r1 + 200 + r2.
pub fn in_area(area: &Area, me: [f32; 3], p: [f32; 3]) -> bool {
    if p == [0.0; 3] {
        return true;
    }
    let sq = |c: [f32; 2], q: [f32; 3]| (c[0] - q[0]).powi(2) + (c[1] - q[2]).powi(2);
    match *area {
        Area::Me => sq([me[0], me[2]], p) < 4.246_73e7,
        Area::Fixed { centre, r1, r2 } => {
            let d = sq(centre, p);
            let mine = sq(centre, me);
            let r = r1 + 40.0 + r2;
            // outside only when beyond both (the original squares the already squared distance)
            !(r * r <= d && mine * mine <= d)
        }
        Area::Wide { centre, r1, r2 } => {
            let r = r1 + 200.0 + r2;
            sq(centre, p) < r * r
        }
    }
}

/// A character's hold position (+0x450 +0x130), when it has one (F029593 and not F045489).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hold {
    pub position: [f32; 3],
}

/// What the leash test reads of the target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeashTarget {
    pub position: [f32; 3],
    /// Its medical +0xb8 below 2 (a creature too weak to stand), its stance (vtable +0x368)
    /// prone, or its medical +0x37c flag: it is only fought within 40 units (squared 1600).
    pub helpless: bool,
    /// Fighting (F033833) and facing me (its +0xd0 heading against the way to me, F050491): the
    /// leash grows from 80 to 140.
    pub fighting_toward_me: bool,
}

/// @fn F038560
///
/// The hold leash: without a hold position, anyone; a helpless target only within 40 of me;
/// beyond 80 (140 when it fights toward me) of the hold position, only within 60 (squared 3600)
/// of me.
pub fn in_leash(hold: Option<&Hold>, me: [f32; 3], t: &LeashTarget) -> bool {
    let Some(h) = hold else { return true };
    let sq = |a: [f32; 3], b: [f32; 3]| {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
    };
    if t.helpless && sq(t.position, me) > 1600.0 {
        return false;
    }
    let leash = if t.fighting_toward_me { 140.0 } else { 80.0 };
    if leash * leash < sq(h.position, t.position) && sq(t.position, me) > 3600.0 {
        return false;
    }
    true
}

/// A known character, as findEnemy reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Known<Id> {
    pub id: Id,
    pub position: [f32; 3],
    /// The knowledge entry's seen (+0x38) or detected (+0x39) flag.
    pub seen: bool,
    /// Being carried by someone (F044450's carrier).
    pub carried: bool,
    /// Lying down (stance vtable +0x368 at least 1, with vtable +0x78 set): passed over.
    pub lying: bool,
    /// Its faction is mine or an ally of it (F041488).
    pub ally: bool,
    /// Its squad is the one my squad targets (F038606's first exception).
    pub squad_target: bool,
    /// Its swing at me is under way (F038606: progress strictly between 0 and 1).
    pub swinging_at_me: bool,
    pub leash: LeashTarget,
}

/// @fn F038641
///
/// The enemy to attack: of the known characters, those seen or detected, not carried, not lying
/// down, not allied, inside the hold leash (F038560) and the area (F038606: unless my squad
/// targets theirs or they swing at me); with `first` the first such, else the best by `score`
/// (the squad's F038638; above -9999).
pub fn find_enemy<Id: Copy>(
    known: &[Known<Id>],
    me: [f32; 3],
    area: &Area,
    hold: Option<&Hold>,
    first: bool,
    mut score: impl FnMut(&Known<Id>) -> f32,
) -> Option<Id> {
    let mut best: Option<(f32, Id)> = None;
    for k in known {
        if !k.seen || k.carried || k.lying || k.ally {
            continue;
        }
        if !in_leash(hold, me, &k.leash) {
            continue;
        }
        if !(k.squad_target || in_area(area, me, k.position) || k.swinging_at_me) {
            continue;
        }
        if first {
            return Some(k.id);
        }
        let s = score(k);
        if s > -9999.0 && best.is_none_or(|(b, _)| b < s) {
            best = Some((s, k.id));
        }
    }
    best.map(|(_, id)| id)
}

/// (harness) The squad's scoring stand-in: the nearer, the better.
pub fn nearest_score<Id>(me: [f32; 3]) -> impl Fn(&Known<Id>) -> f32 {
    move |k| -((k.position[0] - me[0]).powi(2) + (k.position[2] - me[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(id: u32, x: f32) -> Known<u32> {
        Known {
            id,
            position: [x, 0.0, 0.0],
            seen: true,
            carried: false,
            lying: false,
            ally: false,
            squad_target: false,
            swinging_at_me: false,
            leash: LeashTarget {
                position: [x, 0.0, 0.0],
                helpless: false,
                fighting_toward_me: false,
            },
        }
    }

    #[test]
    fn picks_the_nearest_enemy() {
        let me = [0.0; 3];
        let mut list = vec![k(1, 300.0), k(2, 100.0), k(3, 50.0)];
        list[2].ally = true;
        let s = nearest_score(me);
        assert_eq!(find_enemy(&list, me, &Area::Me, None, false, &s), Some(2));
        assert_eq!(find_enemy(&list, me, &Area::Me, None, true, &s), Some(1));
        list[1].seen = false;
        assert_eq!(find_enemy(&list, me, &Area::Me, None, false, &s), Some(1));
    }

    #[test]
    fn hold_leash() {
        let h = Hold { position: [0.0; 3] };
        let me = [0.0; 3];
        let mut t = k(1, 100.0).leash;
        // beyond 80 of the hold and 60 of me
        assert!(!in_leash(Some(&h), me, &t));
        t.fighting_toward_me = true;
        assert!(in_leash(Some(&h), me, &t));
        assert!(in_leash(None, me, &t));
    }

    #[test]
    fn areas() {
        let me = [0.0; 3];
        assert!(in_area(&Area::Me, me, [6000.0, 0.0, 0.0]));
        assert!(!in_area(&Area::Me, me, [7000.0, 0.0, 0.0]));
        let w = Area::Wide {
            centre: [0.0, 0.0],
            r1: 100.0,
            r2: 0.0,
        };
        assert!(in_area(&w, me, [250.0, 0.0, 0.0]));
        assert!(!in_area(&w, me, [350.0, 0.0, 0.0]));
    }
}
