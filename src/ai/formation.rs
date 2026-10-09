//! Squad formations (port_map `ai`): where each member of a platoon stands relative to its leader,
//! and the spot a follower heads for.
//!
//! Spec: F028591 ActivePlatoon::layoutFormation (the faction's "squad formation": 0 loose, 1
//! caravan, 2 ranks), F028558 pushApart, F028569 grid, F050533 roundHalf, F028592 the offset
//! lookup, F030760 / F030759 the follower's spot (Task_FollowLeader).
//!
//! Offsets are in units, in the leader's frame: -x is the leader's facing, so positive x lies
//! behind. The original lays out characters other members carry or lead (the platoon's list at
//! Platoon vtable +0x98, kept per member) as well; those are not ported, so the loose
//! formation's flank step and the ranks' first block never run here.

use std::f32::consts::PI;

pub type Vec3 = [f32; 3];

/// The faction's "squad formation".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormationKind {
    Loose = 0,
    Caravan = 1,
    Ranks = 2,
}

impl FormationKind {
    pub fn from_int(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Loose),
            1 => Some(Self::Caravan),
            2 => Some(Self::Ranks),
            _ => None,
        }
    }
}

/// One member as the layout sees it (an entry of F028591, 0x50 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Member {
    /// The squad role (AITaskSytem +0x25c): 0 squad, 1 squad2, 2 leader; slaves (4) are left out.
    pub role: i32,
    /// The mover's radius (CharMovement F040603: footprint * size), units.
    pub radius: f32,
    /// An animal (Character vtable +0x248, CharacterAnimal::asAnimal, is set).
    pub animal: bool,
}

/// Entry radius: the mover radius times 1.5.
const ENTRY_RADIUS: f32 = 1.5;
/// Push-apart margin (F028591 passes 5).
const SPACING: f32 = 5.0;

/// @fn F050533
///
/// Round half away from zero.
pub fn round_half(x: f32) -> i32 {
    if x < 0.0 {
        (x - 0.5).ceil() as i32
    } else {
        (x + 0.5).floor() as i32
    }
}

/// A placed point and its radius (the layout's 16-byte list).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    p: Vec3,
    r: f32,
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn scale(a: Vec3, s: f32) -> Vec3 {
    a.map(|v| v * s)
}
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// Ogre's Vector3::normalise: unchanged when the length is 0.
pub fn normalise(a: Vec3) -> Vec3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

/// @fn F028558
///
/// Move `p` out of every point it overlaps (closer across than both radii plus `spacing`): along
/// the line from that point (a random one when they coincide; once pushed, a direction against
/// the first push takes the first's instead) to 10 beyond the overlap, then check every point
/// again. Returns how many pushes it took.
fn push_apart(
    points: &[Point],
    spacing: f32,
    p: &mut Point,
    rand01: &mut impl FnMut() -> f32,
) -> i32 {
    let mut pushes = 0;
    let mut first: Vec3 = [0.0; 3];
    let mut i = 0;
    while i < points.len() {
        let q = points[i];
        let (dz, dx) = (p.p[2] - q.p[2], p.p[0] - q.p[0]);
        let reach = q.r + p.r + spacing;
        let reach2 = reach * reach;
        if dz * dz + dx * dx < reach2 {
            let mut dir = sub(p.p, q.p);
            if dir == [0.0; 3] {
                dir[0] = rand01() - 0.5;
                dir[2] = rand01() - 0.5;
            }
            if pushes == 0 {
                first = dir;
            } else if dot(dir, first) < 0.0 {
                dir = first;
            }
            let dir = normalise(dir);
            let v = sub(p.p, q.p);
            let b = dot(v, dir);
            let c = dot(v, v);
            let t = (b * b - (c - reach2)).sqrt();
            p.p = add(p.p, scale(dir, (t - b) + 10.0));
            pushes += 1;
            i = 0;
            continue;
        }
        i += 1;
    }
    pushes
}

/// @fn F028569
///
/// `count` spots in rows of `per_row` across the leader's path (z, `2 * radius + z_gap` apart,
/// centred), rows stepping back (x, `2 * radius + x_gap` apart) from `x0 + radius`; a short last
/// row is centred too.
pub fn grid(per_row: i32, count: i32, x0: f32, radius: f32, x_gap: f32, z_gap: f32) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(count.max(0) as usize);
    if count == 0 {
        return out;
    }
    let mut x = x0 + radius;
    let k = if per_row <= count { per_row } else { count };
    let zstep = radius * 2.0 + z_gap;
    let xstep = radius * 2.0 + x_gap;
    let half = zstep * 0.5;
    let mut span = (k - 1) as f32 * half;
    let mut z = -span;
    let mut j = 0;
    for i in 0..count {
        out.push([x, 0.0, z]);
        j += 1;
        z += zstep;
        if j == per_row {
            j = 0;
            let rem = count - i;
            if rem - 1 < per_row {
                span = (rem - 2) as f32 * half;
            }
            x += xstep;
            z = -span;
        }
    }
    out
}

/// @fn F028591
///
/// Each member's offset from the leader; `members[0]` is the leader (F028591 puts it first) and
/// `leader_radius` its mover radius (F039126). Members the layout does not place keep the zero
/// offset, as a failed lookup gives (F028592). `rand01` is the game's rand() / 32768 (F050500).
pub fn layout(
    kind: FormationKind,
    members: &[Member],
    leader_radius: f32,
    rand01: &mut impl FnMut() -> f32,
) -> Vec<Vec3> {
    let mut off = vec![[0.0f32; 3]; members.len()];
    let n = members.len();
    if n <= 1 {
        return off;
    }
    let r = |m: &Member| m.radius * ENTRY_RADIUS;
    // randRange (F050502): uniform in [a, b)
    let range = |a: f32, b: f32, rand01: &mut dyn FnMut() -> f32| rand01() * (b - a) + a;
    // coin (F050501): rand() / 32768 above 0.5
    let coin = |rand01: &mut dyn FnMut() -> f32| rand01() > 0.5;
    match kind {
        FormationKind::Loose => {
            let mut pts = vec![Point {
                p: [0.0; 3],
                r: leader_radius,
            }];
            let reach = (n as f32 * 100.0).sqrt();
            let mut max_x = 0.0f32;
            for (i, m) in members.iter().enumerate() {
                let d = rand01() * reach;
                let a = rand01() * (2.0 * PI);
                let mut p = Point {
                    p: [a.cos() * d, 0.0, a.sin() * d],
                    r: r(m),
                };
                push_apart(&pts, SPACING, &mut p, rand01);
                off[i] = p.p;
                if p.p[0] > max_x {
                    max_x = p.p[0];
                }
                pts.push(p);
            }
        }
        FormationKind::Caravan => {
            let mut pts = vec![Point {
                p: [0.0; 3],
                r: r(&members[0]),
            }];
            let mut x = range(10.0, 20.0, rand01) + r(&members[0]);
            let (mut animals, mut others) = (vec![], vec![]);
            for (i, m) in members.iter().enumerate().skip(1) {
                if m.role == 0 {
                    let jitter = range(10.0, 20.0, rand01);
                    x += r(m);
                    let px = jitter + x;
                    off[i] = [px, 0.0, 0.0];
                    x = px + r(m);
                    pts.push(Point { p: off[i], r: r(m) });
                } else if m.animal {
                    animals.push(i);
                } else {
                    others.push(i);
                }
            }
            if members.iter().any(|m| m.role == 1) {
                let lo = range(5.0, 10.0, rand01) + r(&members[0]);
                let hi = range(20.0, 40.0, rand01) + x;
                let start = pts.len();
                let clear = |pts: &[Point], p: Vec3, rr: f32| {
                    pts.iter().all(|q| {
                        let (dx, dz) = (p[0] - q.p[0], p[2] - q.p[2]);
                        let s = rr + q.r;
                        s * s <= dz * dz + dx * dx
                    })
                };
                for &i in &others {
                    let mut p = [range(lo, hi, rand01), 0.0, 0.0];
                    let side = if coin(rand01) { 1.0 } else { -1.0 };
                    loop {
                        p[2] += range(10.0, 50.0, rand01) * side;
                        if clear(&pts, p, r(&members[i])) {
                            break;
                        }
                    }
                    off[i] = p;
                    pts.push(Point {
                        p,
                        r: r(&members[i]),
                    });
                }
                let last = pts.len() as i64 - 1;
                for (j, &i) in animals.iter().enumerate() {
                    let ri = r(&members[i]);
                    let mut p = [0.0f32; 3];
                    if (start + j) as i64 <= last {
                        // beside the one in the same place in the list of those set out above
                        let q = pts[start + j];
                        let sx = if coin(rand01) { 1.0 } else { -1.0 };
                        let sz = if coin(rand01) { 1.0 } else { -1.0 };
                        p[0] = (q.r + ri) * sx + q.p[0];
                        p[2] = (q.r + ri) * sz + q.p[2];
                        loop {
                            p[0] += range(2.0, 5.0, rand01) * sx;
                            p[2] += range(2.0, 5.0, rand01) * sz;
                            if clear(&pts, p, ri) {
                                break;
                            }
                        }
                    } else {
                        p[0] = range(lo, hi, rand01);
                        let side = if coin(rand01) { 1.0 } else { -1.0 };
                        loop {
                            p[2] += range(10.0, 50.0, rand01) * side;
                            if clear(&pts, p, ri) {
                                break;
                            }
                        }
                    }
                    off[i] = p;
                    pts.push(Point { p, r: ri });
                }
            }
        }
        FormationKind::Ranks => {
            let (mut people, mut animals) = (vec![], vec![]);
            for (i, m) in members.iter().enumerate().skip(1) {
                if m.animal {
                    animals.push(i);
                } else {
                    people.push(i);
                }
            }
            let rows_of = people.len().max(2) as f32;
            let cols = round_half(rows_of.sqrt());
            let per_row = (rows_of / cols as f32).ceil() as i32;
            let mut x0 = 25.0f32;
            let block = |ids: &[usize], x0: &mut f32, off: &mut Vec<Vec3>| {
                let radius = ids
                    .iter()
                    .map(|&i| r(&members[i]))
                    .fold(0.0f32, |a, b| if b > a { b } else { a });
                let spots = grid(per_row, ids.len() as i32, *x0, radius, 10.0, 10.0);
                for (k, &i) in ids.iter().enumerate() {
                    off[i] = spots[k];
                }
                if let Some(l) = spots.last() {
                    *x0 = radius + l[0] + 20.0;
                }
            };
            if !animals.is_empty() {
                let (small, big): (Vec<usize>, Vec<usize>) =
                    animals.iter().partition(|&&i| r(&members[i]) < 10.0);
                if !small.is_empty() {
                    block(&small, &mut x0, &mut off);
                }
                if !big.is_empty() {
                    block(&big, &mut x0, &mut off);
                }
            }
            if !people.is_empty() {
                block(&people, &mut x0, &mut off);
            }
        }
    }
    off
}

/// A unit quaternion (w, x, y, z), as Ogre keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    fn normalised(self) -> Quat {
        let len = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        let f = 1.0 / len;
        Quat {
            w: self.w * f,
            x: self.x * f,
            y: self.y * f,
            z: self.z * f,
        }
    }

    /// Ogre's Vector3::getRotationTo without a fallback axis.
    pub fn rotation_to(from: Vec3, dest: Vec3) -> Quat {
        let v0 = normalise(from);
        let v1 = normalise(dest);
        let d = dot(v0, v1);
        if d >= 1.0 {
            return Quat::IDENTITY;
        }
        if d < 1e-6 - 1.0 {
            let mut axis = cross([1.0, 0.0, 0.0], from);
            if dot(axis, axis) < 1e-6 * 1e-6 {
                axis = cross([0.0, 1.0, 0.0], from);
            }
            let axis = normalise(axis);
            let half = PI * 0.5;
            let s = half.sin();
            return Quat {
                w: half.cos(),
                x: s * axis[0],
                y: s * axis[1],
                z: s * axis[2],
            };
        }
        let s = ((1.0 + d) * 2.0).sqrt();
        let inv = 1.0 / s;
        let c = cross(v0, v1);
        Quat {
            w: s * 0.5,
            x: c[0] * inv,
            y: c[1] * inv,
            z: c[2] * inv,
        }
        .normalised()
    }

    /// Ogre's Quaternion * Vector3.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let q = [self.x, self.y, self.z];
        let uv = cross(q, v);
        let uuv = cross(q, uv);
        add(add(v, scale(uv, 2.0 * self.w)), scale(uuv, 2.0))
    }
}

const NEG_UNIT_X: Vec3 = [-1.0, 0.0, 0.0];

/// @fn F030760
///
/// A follower's spot in its own platoon: the formation faces where the leader moves (its
/// facing, CharMovement +0xd0, or -x when it has none), turning that way at half a unit per
/// second while the leader moves; the spot is the leader's position one second ahead (its speed
/// +0xb8 along the formation's facing) plus the offset turned from -x to that facing. Returns the
/// spot; `facing` is the task's own (+0xa0). (A leader inside a building the follower may not
/// enter puts the spot at that building's door instead: not ported.)
pub fn formation_spot(
    facing: &mut Vec3,
    leader_pos: Vec3,
    leader_facing: Vec3,
    leader_speed: f32,
    offset: Vec3,
    dt: f32,
) -> Vec3 {
    let mut dir = normalise([leader_facing[0], 0.0, leader_facing[2]]);
    if dot(dir, dir) < 1e-6 * 1e-6 {
        dir = NEG_UNIT_X;
    }
    if leader_speed > 0.0 {
        *facing = add(*facing, scale(scale(sub(dir, *facing), 0.5), dt));
        facing[1] = 0.0;
        *facing = normalise(*facing);
    }
    let rot = Quat::rotation_to(NEG_UNIT_X, *facing);
    let turned = rot.rotate(offset);
    add(add(leader_pos, scale(*facing, leader_speed)), turned)
}

/// @fn F030759
///
/// A follower outside the leader's platoon heads for the point `distance` (+0x90, 40) from the
/// leader on its own side.
pub fn trailing_spot(me: Vec3, leader: Vec3, distance: f32) -> Vec3 {
    let mut d = sub(me, leader);
    d[1] = 0.0;
    add(leader, scale(normalise(d), distance))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(v: &[f32]) -> impl FnMut() -> f32 + '_ {
        let mut i = 0;
        move || {
            let x = v[i % v.len()];
            i += 1;
            x
        }
    }

    #[test]
    fn rounding() {
        assert_eq!(round_half(2.5), 3);
        assert_eq!(round_half(2.449), 2);
        assert_eq!(round_half(-2.5), -3);
    }

    #[test]
    fn grid_rows_centre() {
        // 5 in rows of 3: a row of 3, then a centred row of 2
        let g = grid(3, 5, 25.0, 6.0, 10.0, 10.0);
        assert_eq!(g[0], [31.0, 0.0, -22.0]);
        assert_eq!(g[2], [31.0, 0.0, 22.0]);
        assert_eq!(g[3], [53.0, 0.0, -11.0]);
        assert_eq!(g[4], [53.0, 0.0, 11.0]);
    }

    #[test]
    fn ranks_put_people_behind() {
        let m = Member {
            role: 0,
            radius: 4.0,
            animal: false,
        };
        let members = vec![m; 7];
        let off = layout(FormationKind::Ranks, &members, 4.0, &mut seq(&[0.5]));
        assert_eq!(off[0], [0.0; 3]);
        // 6 people: 2 columns -> 3 per row, rows 2 * 6 + 10 apart
        assert_eq!(off[1], [31.0, 0.0, -22.0]);
        assert_eq!(off[4], [53.0, 0.0, -22.0]);
    }

    #[test]
    fn loose_members_do_not_overlap() {
        let m = Member {
            role: 0,
            radius: 4.0,
            animal: false,
        };
        let members = vec![m; 8];
        let mut s = 12345u32;
        let mut rnd = move || {
            s = s.wrapping_mul(214013).wrapping_add(2531011);
            ((s >> 16) & 0x7fff) as f32 / 32768.0
        };
        let off = layout(FormationKind::Loose, &members, 4.0, &mut rnd);
        for i in 1..off.len() {
            for j in 0..i {
                let d = ((off[i][0] - off[j][0]).powi(2) + (off[i][2] - off[j][2]).powi(2)).sqrt();
                assert!(d >= 12.0, "{i} {j} {d}");
            }
        }
    }

    #[test]
    fn caravan_squad_in_a_line() {
        let m = Member {
            role: 0,
            radius: 4.0,
            animal: false,
        };
        let off = layout(FormationKind::Caravan, &[m, m, m], 4.0, &mut seq(&[0.0]));
        // first gap 10 + 6, then each 6 + 10 + 6 on
        assert_eq!(off[1], [6.0 + 10.0 + 6.0 + 10.0, 0.0, 0.0]);
        assert_eq!(off[2], [off[1][0] + 6.0 + 10.0 + 6.0, 0.0, 0.0]);
    }

    #[test]
    fn spot_turns_with_the_leader() {
        // leader heading +z: an offset behind (+x) ends up at -z
        let mut facing = [0.0, 0.0, 1.0];
        let s = formation_spot(
            &mut facing,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            0.0,
            [10.0, 0.0, 0.0],
            0.0,
        );
        assert!((s[0]).abs() < 1e-4 && (s[2] + 10.0).abs() < 1e-4, "{s:?}");
        // no facing yet: identity
        let mut none = [0.0; 3];
        let s = formation_spot(&mut none, [0.0; 3], [0.0; 3], 0.0, [10.0, 0.0, 3.0], 0.0);
        assert_eq!(s, [10.0, 0.0, 3.0]);
    }
}
