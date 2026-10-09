//! Dialogue (port_map `dialogue`): talking to people. A person's DIALOGUE_PACKAGE lists
//! DIALOGUEs by the event that starts them (EventTrigger, T00044); talking to someone runs its
//! EV_PLAYER_TALK_TO_ME dialogue whose conditions pass. A DIALOGUE's "lines" open it; each
//! DIALOGUE_LINE has text variants ("text0".."textN"), a speaker (TalkerEnum), conditions and
//! effects (DIALOG_ACTION records: DialogConditionEnum / DialogActionEnum / ComparisonEnum, from
//! the FCS editor), and its replies in "lines".
//!
//! (stand-in) Not decompiled: the game's choice among passing dialogues and lines (here: the
//! first in order), repetition limits, interjectors. Conditions not ported read as 0; effects
//! not ported do nothing.
use crate::sim::{PLAYER_FACTION, Sim};
use kenshi_data::GameData;

/// EventTrigger EV_PLAYER_TALK_TO_ME.
pub const EV_PLAYER_TALK_TO_ME: i32 = 1;
/// EventTrigger EV_I_SEE_NEUTRAL_SQUAD: a squad meets someone it is not at war with.
pub const EV_I_SEE_NEUTRAL_SQUAD: i32 = 3;

/// TalkerEnum: who says a line.
pub const T_ME: i32 = 0;

/// What each character's dialogues remember (per speaker): those unlocked and locked by lines
/// ("unlocks", "unlock but keep me", "locks"), when each was last spoken (its "repetition
/// limit"), its "chance permanent" rolled once, and the dialogue running.
#[derive(Debug, Clone, Default)]
pub struct DialogMemory {
    pub unlocked: std::collections::HashSet<(usize, String)>,
    pub locked: std::collections::HashSet<(usize, String)>,
    pub spoken: std::collections::HashMap<(usize, String), f64>,
    pub has: std::collections::HashMap<(usize, String), bool>,
    pub running: std::collections::HashMap<usize, String>,
    /// When each NPC last met each of the player's characters (game hours): DC_MET_TARGET_BEFORE,
    /// DC_TARGET_LAST_SEEN_X_HOURS_AGO, DA_REMEMBER_CHARACTER.
    pub met: std::collections::HashMap<(usize, usize), f64>,
    /// Whom each NPC in a conversation talks to.
    pub partner: std::collections::HashMap<usize, usize>,
    /// What each person remembers of another (CharacterPerceptionTags_LongTerm, a bit each):
    /// (who, of whom) -> tags. DC_HAS_TAG.
    pub tags: std::collections::HashMap<(usize, usize), u32>,
}

/// CharacterPerceptionTags_LongTerm.
pub const LT_MY_LIFESAVER: u32 = 2;
pub const LT_FREED_ME: u32 = 3;
pub const LT_STOLE_FROM_ME: u32 = 4;
pub const LT_MY_CAPTOR: u32 = 5;
pub const LT_DEFEATED_MY_SQUAD_ONCE: u32 = 7;
pub const LT_SQUAD_LOST_TO_ME_ONCE: u32 = 8;
pub const LT_KILLED_MY_FRIEND: u32 = 9;

/// DialogRepetitionEnum -> the game hours before a dialogue may be spoken again: none, 2, 6,
/// 48, a week, never, 0.2. (assumption) The hours as the enum's names give them.
pub fn repetition_hours(limit: i32) -> f64 {
    match limit {
        1 => 2.0,
        2 => 6.0,
        3 => 48.0,
        4 => 168.0,
        5 => f64::INFINITY,
        6 => 0.2,
        _ => 0.0,
    }
}

/// What a choice did, for the game to act on.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Trade,
    Recruit,
    Attack,
    Money(i32),
    Relations(f32),
    /// DA_CRIME_ALARM: raised on the one spoken to.
    Alarm,
    /// CHOOSE_SLAVES_SELLING / CHOOSE_SLAVES_BUYING: the slave trade windows.
    SellSlaves,
    BuySlaves,
    /// DA_ENSLAVE_TARGETS_CARRIED_PERSON: whom the player carries becomes the speaker's slave.
    EnslaveCarried,
    /// DA_TALK_TO_LEADER: the conversation goes to the speaker's squad leader.
    TalkToLeader,
}

/// A conversation in progress: the NPC's line and the player's choices (line id, text).
#[derive(Debug, Clone)]
pub struct Conversation {
    pub npc: usize,
    pub player: usize,
    pub npc_text: String,
    pub choices: Vec<(String, String)>,
}

/// One of a line's text variants, at random.
fn text(line: &GameData, rand: f32) -> String {
    let variants: Vec<&String> = (0..).map_while(|i| line.strings.get(&format!("text{i}"))).filter(|t| !t.is_empty()).collect();
    if variants.is_empty() {
        return String::new();
    }
    variants[((rand * variants.len() as f32) as usize).min(variants.len() - 1)].clone()
}

impl Sim {
    /// The NPC's relation to the player's faction (-100 .. 100).
    fn relation_to_player(&self, npc: usize) -> f32 {
        let f = self.folk[npc].faction.as_deref();
        f.and_then(|f| self.relations.get(f)).map_or(0.0, |r| r.relation(PLAYER_FACTION))
    }

    /// A DialogConditionEnum's value for (me, target).
    fn condition_value(&self, cond: i32, me: usize, target: usize) -> f32 {
        let b = |x: bool| if x { 1.0 } else { 0.0 };
        let (m, t) = (&self.folk[me], &self.folk[target]);
        match cond {
            // the NPC's faction toward the player, whichever side is asked about
            1 => self.relation_to_player(if m.player { target } else { me }),
            2 => self.money as f32,
            9 => b(m.shop.is_some()),
            11 => 0.0,
            16 => 1.0,
            17 => b(t.inside.is_some() && t.inside == m.home),
            // DC_IS_LEADER: the squad's leader (made with the leader role, 5)
            19 => b(m.role == 5 || m.player),
            24 => b(self.relation_to_player(me) >= 50.0),
            25 => b(self.relation_to_player(me) <= -30.0),
            35 => self.folk.iter().filter(|p| p.player && !p.dead).count() as f32,
            36 => b(m.player),
            41 => b(m.down),
            44 => b(m.dead),
            56 => b(self.arena.f[me].active),
            71 => b(m.player && self.arena.f[me].medical.hunger.shown() < 50.0),
            // DC_IS_IMPRISONED, DC_IS_SLAVE, DC_IS_IN_LOCKED_CAGE, DC_WEARING_LOCKED_SHACKLES
            14 => b(m.prison.is_some()),
            59 => b(m.prison.as_ref().is_some_and(|p| p.slave)),
            63 => b(m.prison.is_some() && !m.shackled),
            64 => b(m.shackled),
            // DC_IMPRISONED_BY_TARGET / _BY_OTHER: by the target's faction or another's
            7 => b(m.prison.as_ref().is_some_and(|p| Some(&p.faction) == t.faction.as_ref())),
            8 => b(m.prison.as_ref().is_some_and(|p| Some(&p.faction) != t.faction.as_ref())),
            // DC_IS_RUNNING (the player's run toggle; NPCs run in a fight), DC_IS_SNEAKING,
            // DC_IS_INDOORS
            32 => b(if m.player { self.player_run } else { self.arena.f[me].active }),
            67 => b(m.sneaking),
            68 => b(m.inside.is_some()),
            // DC_BOUNTY_AMOUNT_ACTUAL / _PERCEIVED (assumption: as known), DC_CAN_AFFORD_BOUNTY
            40 | 47 => m.bounties.iter().map(|x| x.amount).sum::<i32>() as f32,
            66 => b(self.money >= m.bounties.iter().map(|x| x.amount).sum::<i32>()),
            // DC_SQUAD_IS_DOWN: every one of the squad (platoon) down
            43 => b(self.folk.iter().filter(|p| if m.player { p.player } else { m.platoon.is_some() && p.platoon == m.platoon }).all(|p| p.down || p.dead)),
            // DC_IS_SAME_RACE_AS_ME, DC_HAS_ROBOT_LIMBS
            65 => b(m.race.is_some() && m.race == t.race),
            // DC_IN_A_NON_PLAYER_TOWN, DC_WITHIN_TOWN_WALLS: (assumption) within 2500 units of
            // a town's centre
            31 | 57 => {
                let q = m.mover.position_units();
                b(self.towns.iter().any(|tw| (tw.kind == 1 || tw.kind == 2) && (tw.position[0] - q[0]).hypot(tw.position[2] - q[2]) < 2500.0 * tw.scale.max(1.0)))
            }
            // DC_COPS_AROUND: (assumption) a guard of the town within 600 units
            33 => {
                let q = m.mover.position_units();
                b((0..self.folk.len()).any(|g| g != me && !self.folk[g].down && self.is_guard(g) && {
                    let r = self.folk[g].mover.position_units();
                    (r[0] - q[0]).hypot(r[2] - q[2]) < 600.0
                }))
            }
            // DC_BROKEN_LEG / DC_BROKEN_ARM: a leg (arm) below no health
            27 | 28 => {
                let kind = if cond == 27 { 1 } else { 2 };
                b(self.arena.f[me].medical.parts.iter().any(|p| p.kind == kind && p.health() <= 0.0))
            }
            79 => b(m.limbs.iter().any(|l| l.is_some())),
            // DC_CARRYING_BOUNTY_ALIVE / _DEAD: carrying someone with a bounty, alive or dead
            4 | 5 => b(m.carrying.is_some_and(|c| !self.folk[c].bounties.is_empty() && self.folk[c].dead == (cond == 5))),
            // DC_PLAYER_TECH_LEVEL: (assumption) the highest "level" of the research done
            12 => self.research_done.iter().filter_map(|r| self.data.get(r)).filter_map(|r| r.ints.get("level").copied()).max().unwrap_or(0) as f32,
            // DC_TARGET_LAST_SEEN_X_HOURS_AGO / DC_MET_TARGET_BEFORE: the NPC's memory of the
            // player's character (never met: 1000 hours)
            18 => {
                let (npc, pc) = if m.player { (target, me) } else { (me, target) };
                self.dialog_memory.met.get(&(npc, pc)).map_or(1000.0, |&at| (self.hours_gone() - at) as f32)
            }
            20 => {
                let (npc, pc) = if m.player { (target, me) } else { (me, target) };
                b(self.dialog_memory.met.contains_key(&(npc, pc)))
            }
            // DC_WEAKER_THAN_ME / DC_STRONGER_THAN_ME: the target's squad against mine
            // (assumption: each squad's strength as the world counts it, 1 + (attack + defence) /
            // 40 a standing member)
            21 => b(self.squad_strength(target) < self.squad_strength(me)),
            22 => b(self.squad_strength(target) > self.squad_strength(me)),
            // DC_DAMAGED_HEAD: the head hurt; DC_NEARLY_KO / DC_IS_NEARLY_KO: (assumption) a
            // vital part (head, chest, stomach) under a fifth of its health
            29 => b(self.arena.f[me].medical.parts.iter().any(|p| p.kind == 3 && p.health() < p.max_health())),
            30 | 42 => b(!m.down && self.arena.f[me].medical.parts.iter().any(|p| (p.kind == 0 || p.kind == 3) && p.health() < p.max_health() * 0.2)),
            // DC_NUM_BACKPACKS: the backpacks its squad wears
            37 => self.squad_of(me).iter().filter(|&&k| self.folk[k].inventory.equipped().any(|i| i.slot == 12)).count() as f32,
            // DC_SQUAD_ONLY_ANIMALS: every one of its squad an animal
            38 => b(self.squad_of(me).iter().all(|&k| self.folk[k].race.is_some())),
            // DC_IS_OUTNUMBERED: its squad's standing people fewer than the target's
            39 => b(self.squad_of(me).iter().filter(|&&k| !self.folk[k].down).count() < self.squad_of(target).iter().filter(|&&k| !self.folk[k].down).count()),
            45 => b(self.is_female(me)),
            // DC_CARRYING_SOMEONE_TO_ENSLAVE: carrying someone, as a slaver
            46 => b(m.captures && m.carrying.is_some()),
            // DC_IM_UNARMED: no weapon worn
            48 => b(!m.inventory.equipped().any(|i| i.kind == Some(kenshi_data::ItemType::Weapon))),
            // DC_I_LOVE_THIS_GUY / DC_I_HATE_THIS_GUY / DC_I_SHOULD_SCREW_THIS_GUY_OVER /
            // DC_I_SHOULD_HELP_THIS_GUY: (assumption) from its faction's relation to the target's
            // and its personality: love an ally (relation 50+), hate an enemy (-30 or less); a
            // traitorous or cruel one screws over who it does not love, an honourable or kind one
            // helps who it does not hate
            52 => b(self.faction_relation(me, target) >= 50.0),
            53 => b(self.faction_relation(me, target) <= -30.0),
            54 => b([2, 12].contains(&m.personality) && self.faction_relation(me, target) < 50.0),
            55 => b([1, 11].contains(&m.personality) && self.faction_relation(me, target) > -30.0),
            // DC_HAS_A_BASE_NEARBY / DC_IN_A_PLAYER_TOWN: the player's buildings within 5000 /
            // 2000 units
            60 | 75 => {
                let q = m.mover.position_units();
                let r = if cond == 60 { 5000.0 } else { 2000.0 };
                b(self.buildings.iter().any(|bd| bd.confirmed && (bd.at[0] - q[0]).hypot(bd.at[2] - q[2]) < r))
            }
            // DC_TARGET_IS_SLAVE_OF_MY_FACTION, DC_IS_ESCAPED_SLAVE
            61 => b(t.slave.as_ref().is_some_and(|s| Some(&s.faction) == m.faction.as_ref())),
            62 => b(m.slave.is_some() && m.prison.is_none() && (m.escaped.is_some() || m.slave.as_ref().is_some_and(|s| s.fled || !s.obedient))),
            // DC_HAS_ILLEGAL_ITEM: carrying an item the target's faction's "trade culture" lists
            // in its "illegal goods"
            69 => {
                let culture = t.faction.as_deref().and_then(|f| self.data.get(f)).and_then(|f| f.refs("trade culture").first().map(|(c, _)| c.clone())).and_then(|c| self.data.get(&c));
                let illegal: Vec<&String> = culture.map(|c| c.refs("illegal goods").iter().map(|(s, _)| s).collect()).unwrap_or_default();
                b(m.inventory.sections.iter().flat_map(|s| s.items.iter()).any(|p| illegal.contains(&&p.item.record)))
            }
            // DC_MIXED_GENDER_GROUP: its squad has both
            72 => {
                let sq = self.squad_of(me);
                let f = sq.iter().filter(|&&k| self.is_female(k)).count();
                b(f > 0 && f < sq.len())
            }
            // DC_TARGET_CHARACTER_EXISTS: (stand-in) yes
            76 => 1.0,
            // DC_IS_RECRUITABLE: (assumption) its CHARACTER's "NPC class" is OT_ADVENTURER (9)
            // or OT_CIVILIAN (4), not a shopkeeper, not in a cage
            77 => b(!m.player && m.shop.is_none() && m.prison.is_none() && m.character.as_deref().and_then(|c| self.data.get(c)).is_some_and(|c| matches!(c.ints.get("NPC class").copied().unwrap_or(0), 4 | 9))),
            // not kept here (reputation, faction ranks, dialogue repeats, death row, town levels
            // and walls, training equipment, contracts): 0
            _ => 0.0,
        }
    }

    /// A list of DIALOG_ACTION conditions (with their ref values) all pass for (npc, player).
    fn conditions_pass(&self, rec: &GameData, npc: usize, player: usize) -> bool {
        rec.refs("conditions").iter().all(|(sid, v)| {
            let Some(c) = self.data.get(sid) else { return true };
            let name = c.ints.get("condition name").copied().unwrap_or(0);
            if name == 0 {
                return true;
            }
            // who: 0 the speaker (the NPC), 1 the target (the player)
            let who = c.ints.get("who").copied().unwrap_or(0);
            let (me, target) = if who == 1 { (player, npc) } else { (npc, player) };
            // DC_HAS_TAG: the NPC's memory of the player's character (the record's "tag")
            let lhs = if name == 23 {
                let tag = c.ints.get("tag").copied().unwrap_or(0).clamp(0, 31) as u32;
                let bits = self.dialog_memory.tags.get(&(npc, player)).copied().unwrap_or(0);
                if bits & (1 << tag) != 0 { 1.0 } else { 0.0 }
            } else if name == 26 {
                // DC_PERSONALITY_TAG: whether its personality is the record's "tag"
                let tag = c.ints.get("tag").copied().unwrap_or(0);
                if self.folk[me].personality == tag { 1.0 } else { 0.0 }
            } else {
                self.condition_value(name, me, target)
            };
            let rhs = v[0] as f32;
            let cmp = c.ints.get("compare by").copied().or_else(|| match c.strings.get("compare by").map(String::as_str) {
                Some("<") => Some(1),
                Some(">") => Some(2),
                _ => Some(0),
            });
            match cmp.unwrap_or(0) {
                1 => lhs < rhs,
                2 => lhs > rhs,
                _ => (lhs - rhs).abs() < 0.5,
            }
        })
    }

    /// The DIALOGUE_PACKAGE person `id` talks with.
    pub fn dialogue_package(&self, id: usize) -> Option<&GameData> {
        let p = &self.folk[id];
        p.dialogue.as_deref().and_then(|d| self.data.get(d))
    }

    /// The first line among `refs` (DIALOGUE_LINE ids) whose conditions pass.
    fn first_line<'a>(&'a self, refs: &[(String, [i32; 3])], npc: usize, player: usize) -> Option<&'a GameData> {
        refs.iter().filter_map(|(sid, _)| self.data.get(sid)).find(|l| self.line_open(l, npc, player))
    }

    /// Whether line `l` may be said in the conversation of `npc` and `player`: its conditions,
    /// and its filters for its speaker (`speaker_of`) and the one it is said to.
    fn line_open(&self, l: &GameData, npc: usize, player: usize) -> bool {
        if !self.conditions_pass(l, npc, player) {
            return false;
        }
        match self.speaker_of(l, npc, player) {
            Some(s) => self.filters_pass(l, s, if s == player { npc } else { player }),
            None => false,
        }
    }

    /// Who says line `l` (TalkerEnum): the NPC (T_ME), the player's character (T_TARGET,
    /// T_TARGET_IF_PLAYER, T_TARGET_WITH_RACE), an interjector (T_INTERJECTOR1..3: another of
    /// the two squads, the first whose filters and conditions pass), the NPC for its whole
    /// squad (T_WHOLE_SQUAD). None: no one can.
    fn speaker_of(&self, l: &GameData, npc: usize, player: usize) -> Option<usize> {
        match l.ints.get("speaker").copied().unwrap_or(T_ME) {
            1 | 2 | 7 => Some(player),
            3..=5 => {
                let mut near: Vec<usize> = self.squad_of(npc);
                near.extend(self.squad_of(player));
                near.into_iter().filter(|&k| k != npc && k != player && !self.folk[k].down && !self.folk[k].dead).find(|&k| self.filters_pass(l, k, if self.folk[k].player { npc } else { player }))
            }
            _ => Some(npc),
        }
    }

    /// A dialogue's or line's filters for speaker `me` and the one it speaks to (fcs.def's
    /// DIALOGUE / DIALOGUE_LINE refs): "world state" (each ref's first value 1: must hold, 0:
    /// must not), "is character", "my race" / "my subrace", "target race", "my faction",
    /// "target faction", "in town of" (a town of that faction), "has package" (its dialogue
    /// package), "target carrying character", "target has item", "target has item type".
    /// (assumption) A race filter names the person's race record, the look's for a person.
    fn filters_pass(&self, d: &GameData, me: usize, target: usize) -> bool {
        let any = |k: &str, f: &dyn Fn(&str) -> bool| {
            let r = d.refs(k);
            r.is_empty() || r.iter().any(|(s, _)| f(s))
        };
        let (m, t) = (&self.folk[me], &self.folk[target]);
        let my_race = self.race_of(me);
        let their_race = self.race_of(target);
        let in_town_of = |f: &str| {
            let q = m.mover.position_units();
            self.towns.iter().any(|tw| tw.faction.as_deref() == Some(f) && tw.contains(q, 1.0))
        };
        d.refs("world state").iter().all(|(s, v)| self.world_state(s) == (v[0] != 0))
            && any("is character", &|c| m.character.as_deref() == Some(c))
            && any("my race", &|r| my_race.as_deref() == Some(r))
            && any("my subrace", &|r| my_race.as_deref() == Some(r))
            && any("target race", &|r| their_race.as_deref() == Some(r))
            && any("my faction", &|f| m.faction.as_deref() == Some(f))
            && any("target faction", &|f| t.faction.as_deref() == Some(f))
            && any("in town of", &in_town_of)
            && any("has package", &|p| m.dialogue.as_deref() == Some(p))
            && any("target carrying character", &|c| t.carrying.is_some_and(|k| self.folk[k].character.as_deref() == Some(c)))
            && any("target has item", &|i| t.inventory.sections.iter().flat_map(|s| s.items.iter()).any(|p| p.item.record == i))
            && any("target has item type", &|i| {
                let kind = self.data.get(i).and_then(|r| r.item_type());
                t.inventory.sections.iter().flat_map(|s| s.items.iter()).any(|p| p.item.kind.is_some() && p.item.kind == kind)
            })
    }

    /// `who` remembers `of` by long-term tag `tag` (LT_*).
    pub fn remember(&mut self, who: usize, of: usize, tag: u32) {
        if who != of {
            *self.dialog_memory.tags.entry((who, of)).or_default() |= 1 << tag;
        }
    }

    /// Every second: deaths and squads beaten remembered (LT_KILLED_MY_FRIEND by the dead's
    /// squad of those fighting it; LT_DEFEATED_MY_SQUAD_ONCE / LT_SQUAD_LOST_TO_ME_ONCE between a
    /// squad all down and those fighting it). (assumption) Who did it: whoever had the fallen
    /// as its combat target.
    pub fn memory_tick(&mut self) {
        if !self.tick.is_multiple_of(60) {
            return;
        }
        let n = self.folk.len();
        if self.mem_down.len() < n {
            self.mem_down.resize(n, (false, false));
        }
        for d in 0..n {
            let now = (self.folk[d].down, self.folk[d].dead);
            let was = self.mem_down[d];
            self.mem_down[d] = now;
            if now == was || !(now.0 || now.1) {
                continue;
            }
            let attackers: Vec<usize> = (0..n).filter(|&k| k != d && self.arena.f[k].focus == Some(d) && !self.folk[k].dead).collect();
            if attackers.is_empty() {
                continue;
            }
            let squad = self.squad_of_any(d);
            if now.1 && !was.1 {
                let mates: Vec<usize> = squad.iter().copied().filter(|&m| m != d && !self.folk[m].dead).collect();
                for m in mates {
                    for &a in &attackers {
                        self.remember(m, a, LT_KILLED_MY_FRIEND);
                    }
                }
            }
            if squad.iter().all(|&m| self.folk[m].down || self.folk[m].dead) {
                for &m in &squad {
                    for &a in &attackers {
                        self.remember(m, a, LT_DEFEATED_MY_SQUAD_ONCE);
                        self.remember(a, m, LT_SQUAD_LOST_TO_ME_ONCE);
                    }
                }
            }
        }
    }

    /// The people of `id`'s squad, the dead too.
    fn squad_of_any(&self, id: usize) -> Vec<usize> {
        let p = &self.folk[id];
        if p.player {
            return self.folk.iter().filter(|q| q.player).map(|q| q.id).collect();
        }
        match p.platoon {
            Some(pl) => self.folk.iter().filter(|q| q.platoon == Some(pl)).map(|q| q.id).collect(),
            None => vec![id],
        }
    }

    /// Whom `npc` is in a conversation with.
    pub fn talking_to(&self, npc: usize) -> Option<usize> {
        self.dialog_memory.partner.get(&npc).copied()
    }

    /// Person `id` on AI package `pkg` ("change AI"): its goals made again from it, as a
    /// leader or one of the squad.
    pub fn set_package(&mut self, id: usize, pkg: &str) {
        let data = self.data.clone();
        let Some(p) = data.get(pkg) else { return };
        let role = if self.folk[id].role == 5 { kenshi_ai::package::Role::Leader } else { kenshi_ai::package::Role::Squad };
        let leader = self.folk[id].platoon.and_then(|pl| self.folk.iter().find(|q| q.platoon == Some(pl) && q.role == 5).map(|q| q.id));
        let goals = kenshi_ai::package::package_goals(&data, p, role);
        let jobs = crate::sim::jobs_of(goals, leader);
        let person = &mut self.folk[id];
        person.ai.jobs = jobs;
        person.ai.queue.clear();
        person.package = Some(pkg.to_string());
        person.rethink = true;
    }

    /// The people of `id`'s squad (its platoon; the player's characters for one of them).
    pub fn squad_of(&self, id: usize) -> Vec<usize> {
        let p = &self.folk[id];
        if p.player {
            return self.folk.iter().filter(|q| q.player && !q.dead).map(|q| q.id).collect();
        }
        match p.platoon {
            Some(pl) => self.folk.iter().filter(|q| q.platoon == Some(pl) && !q.dead).map(|q| q.id).collect(),
            None => vec![id],
        }
    }

    /// A squad's strength (the world's measure, crate::world): 1 + (attack + defence) / 40 a
    /// standing member.
    fn squad_strength(&self, id: usize) -> f32 {
        use kenshi_char::StatsEnumerated::*;
        self.squad_of(id)
            .iter()
            .filter(|&&k| !self.folk[k].down)
            .map(|&k| 1.0 + (self.arena.f[k].level(StatMeleeAttack) + self.arena.f[k].level(StatMeleeDefence)) / 40.0)
            .sum()
    }

    /// `me`'s faction's relation to `target`'s (the player's side for the player's people).
    fn faction_relation(&self, me: usize, target: usize) -> f32 {
        let (m, t) = (&self.folk[me], &self.folk[target]);
        let theirs = if t.player { Some(PLAYER_FACTION.to_string()) } else { t.faction.clone() };
        match (m.faction.as_deref().and_then(|f| self.relations.get(f)), theirs) {
            (Some(r), Some(f)) => r.relation(&f),
            _ => 0.0,
        }
    }

    /// Person `id`'s race record: an animal's own, else as its look draws it.
    pub fn race_of(&self, id: usize) -> Option<String> {
        let p = &self.folk[id];
        if let Some(r) = &p.race {
            return Some(r.clone());
        }
        if let Some(r) = &p.look_race {
            return Some(r.clone());
        }
        crate::looks_roll::race_and_sex(&self.data, &self.install, p, "Greenlander").map(|(r, _, _)| r.string_id.clone())
    }

    /// Whether person `id` is a woman (the character editor's, else as its look draws it).
    pub fn is_female(&self, id: usize) -> bool {
        let p = &self.folk[id];
        p.female.unwrap_or_else(|| crate::looks_roll::race_and_sex(&self.data, &self.install, p, "Greenlander").is_some_and(|(_, f, _)| f))
    }

    /// @fn F040031
    ///
    /// A personality (PersonalityTags) for person `id` from its CHARACTER's "personality", else
    /// its faction's (a PERSONALITY record): one of its "tags always" when it has any; else by
    /// weight, "tags common" 3, "tags rare" 0.2, every other tag but the "tags never" 1.
    pub fn roll_personality(&mut self, id: usize) -> i32 {
        let data = self.data.clone();
        let p = &self.folk[id];
        let rec = p
            .character
            .as_deref()
            .and_then(|c| data.get(c))
            .and_then(|c| c.refs("personality").first().map(|(s, _)| s.clone()))
            .or_else(|| p.faction.as_deref().and_then(|f| data.get(f)).and_then(|f| f.refs("personality").first().map(|(s, _)| s.clone())));
        let Some(rec) = rec.and_then(|r| data.get(&r)) else { return 0 };
        let tags = |k: &str| -> Vec<i32> {
            let mut out: Vec<i32> = rec.ints.iter().filter(|(n, _)| n.starts_with(k)).map(|(_, v)| *v as i32).filter(|&v| v != 0).collect();
            out.sort();
            out
        };
        let always = tags("tags always");
        if !always.is_empty() {
            let k = (self.rng.uniform() * always.len() as f32) as usize;
            return always[k.min(always.len() - 1)];
        }
        let (common, rare, never) = (tags("tags common"), tags("tags rare"), tags("tags never"));
        let weight = |t: i32| {
            if never.contains(&t) {
                0.0
            } else if common.contains(&t) {
                3.0
            } else if rare.contains(&t) {
                0.2
            } else {
                1.0
            }
        };
        // PT_HONORABLE .. PT_MANIACAL (the unnamed 3, 4, 7, 8 and PT_NORMAL 13 too)
        let all: Vec<(i32, f32)> = (1..15).map(|t| (t, weight(t))).filter(|x| x.1 > 0.0).collect();
        let total: f32 = all.iter().map(|x| x.1).sum();
        let mut r = self.rng.uniform() * total;
        for (t, w) in &all {
            r -= w;
            if r < 0.0 {
                return *t;
            }
        }
        all.last().map_or(0, |x| x.0)
    }

    /// A line's text with its word swaps filled ("/NAME/": the WORD_SWAPS record of that
    /// name, its first line whose conditions pass, else its first; "/YOURNAME/" the player
    /// character's name). (stand-in) unknown words are dropped.
    fn words(&mut self, said: &str, npc: usize, player: usize) -> String {
        let mut out = String::new();
        let mut rest = said;
        while let Some(a) = rest.find('/') {
            let Some(b) = rest[a + 1..].find('/') else { break };
            let token = &rest[a + 1..a + 1 + b];
            if token.is_empty() || !token.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                out.push_str(&rest[..a + 1]);
                rest = &rest[a + 1..];
                continue;
            }
            out.push_str(&rest[..a]);
            let word = if token == "YOURNAME" {
                let n = &self.folk[player].name;
                if n.is_empty() { "you".to_string() } else { n.clone() }
            } else {
                let rec = self.data.records.values().find(|r| r.item_type() == Some(kenshi_data::ItemType::WordSwaps) && r.name == token).cloned();
                match rec {
                    Some(w) => {
                        let lines: Vec<GameData> = w.refs("lines").iter().filter_map(|(l, _)| self.data.get(l).cloned()).collect();
                        let pick = lines.iter().find(|l| !l.refs("conditions").is_empty() && self.conditions_pass(l, npc, player)).or(lines.first()).cloned();
                        let r = self.rng.uniform();
                        pick.map(|l| text(&l, r)).unwrap_or_default()
                    }
                    None => String::new(),
                }
            };
            out.push_str(&word);
            rest = &rest[a + 2 + b..];
        }
        out.push_str(rest);
        out
    }

    /// The conversation at `line` (an NPC line): its text and the player's replies.
    fn at_line(&mut self, line: &GameData, npc: usize, player: usize) -> Conversation {
        let r = self.rng.uniform();
        let said = text(line, r);
        let mut npc_text = self.words(&said, npc, player);
        let mut line = line.clone();
        let mut choices = Vec::new();
        // the NPC's own follow-on lines (speaker T_ME) run on in its speech; the rest are the
        // player's replies
        for _ in 0..6 {
            let mut next_npc = None;
            for (sid, _) in line.refs("lines") {
                let Some(reply) = self.data.get(sid).cloned() else { continue };
                if !self.line_open(&reply, npc, player) {
                    continue;
                }
                // the NPC's, its squad's and the interjectors' lines run on in its speech
                if matches!(reply.ints.get("speaker").copied().unwrap_or(T_ME), 0 | 3..=6) {
                    if next_npc.is_none() {
                        next_npc = Some(reply);
                    }
                    continue;
                }
                let r = self.rng.uniform();
                let t = text(&reply, r);
                let t = self.words(&t, npc, player);
                choices.push((sid.clone(), t));
            }
            match next_npc {
                Some(n) if choices.is_empty() => {
                    let r = self.rng.uniform();
                    let t = text(&n, r);
                    let t = self.words(&t, npc, player);
                    // an interjector's line under its name
                    let by = self.speaker_of(&n, npc, player).filter(|&s| s != npc);
                    if !t.is_empty() {
                        npc_text = match by {
                            Some(s) => format!("{npc_text}\n\n{}: {t}", self.display_name(s)),
                            None => format!("{npc_text} {t}"),
                        };
                    }
                    let fx = self.effects(&n, npc);
                    self.pending_effects.extend(fx);
                    line = n;
                }
                _ => break,
            }
        }
        Conversation { npc, player, npc_text, choices }
    }

    /// Player character `player` talks to `npc`: its EV_PLAYER_TALK_TO_ME dialogue whose
    /// conditions pass, at its first passing line.
    pub fn talk(&mut self, npc: usize, player: usize) -> Option<Conversation> {
        self.talk_on(npc, player, EV_PLAYER_TALK_TO_ME, false).map(|(c, _)| c)
    }

    /// `npc` starts a dialogue of event `event` with `player` (its "dialogs" entry's chance,
    /// the entry's second value, rolled when `roll`): the conversation and the opening line's
    /// effects.
    pub fn talk_on(&mut self, npc: usize, player: usize, event: i32, roll: bool) -> Option<(Conversation, Vec<Effect>)> {
        // its dialogue package (and what it inherits) and its squad's AI package
        // its dialogue packages: its own (its squad template's, else its character's), the
        // rest of its CHARACTER's "dialogue package" list, each with what it inherits
        let mut packages: Vec<GameData> = Vec::new();
        let mut own: Vec<GameData> = self.dialogue_package(npc).cloned().into_iter().collect();
        if !self.folk[npc].player
            && let Some(c) = self.folk[npc].character.as_deref().and_then(|c| self.data.get(c))
        {
            for (sid, _) in c.refs("dialogue package") {
                if let Some(p) = self.data.get(sid)
                    && !own.iter().any(|o| o.string_id == p.string_id)
                {
                    own.push(p.clone());
                }
            }
        }
        // and its faction's "dialog default"
        if !self.folk[npc].player
            && let Some(f) = self.folk[npc].faction.as_deref().and_then(|f| self.data.get(f))
        {
            for (sid, _) in f.refs("dialog default") {
                if let Some(p) = self.data.get(sid)
                    && !own.iter().any(|o| o.string_id == p.string_id)
                {
                    own.push(p.clone());
                }
            }
        }
        for package in own {
            let inherited: Vec<GameData> = package.refs("inheritsFrom").iter().filter_map(|(sid, _)| self.data.get(sid).cloned()).collect();
            packages.push(package);
            packages.extend(inherited);
        }
        if let Some(p) = self.folk[npc].package.as_deref().and_then(|p| self.data.get(p)) {
            packages.push(p.clone());
        }
        // its CHARACTER's own "dialogue" (talking to it)
        let personal: Vec<String> = if event == EV_PLAYER_TALK_TO_ME {
            self.folk[npc].character.as_deref().and_then(|c| self.data.get(c)).map(|c| c.refs("dialogue").iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default()
        } else {
            Vec::new()
        };
        if packages.is_empty() && personal.is_empty() {
            return None;
        }
        // every dialogue of the event whose conditions pass (its chance rolled); (assumption)
        // the highest "score bonus" is spoken, ties at random (the game's scoring is not
        // decompiled)
        let mut best: Option<(f32, GameData, GameData)> = None;
        let candidates: Vec<(String, [i32; 3])> = packages
            .iter()
            .flat_map(|pkg| pkg.refs("dialogs").iter().filter(|(_, v)| v[0] == event).cloned())
            .chain(personal.iter().map(|s| (s.clone(), [event, 0, 0])))
            .collect();
        {
            for (sid, v) in &candidates {
                if roll && v[1] > 0 && self.rng.uniform() * 100.0 >= v[1] as f32 {
                    continue;
                }
                let Some(d) = self.data.get(sid.as_str()).cloned() else { continue };
                // its world states, character, town, factions, races and items
                let dbg = std::env::var("KENSHI_MEET_DEBUG").is_ok();
                if !self.filters_pass(&d, npc, player) {
                    if dbg {
                        eprintln!("  {}: filters fail", d.name);
                    }
                    continue;
                }
                if !self.dialogue_open(&d, npc, player, event) {
                    if dbg {
                        eprintln!("  {}: not open", d.name);
                    }
                    continue;
                }
                let dbg = std::env::var("KENSHI_MEET_DEBUG").is_ok();
                if !self.conditions_pass(&d, npc, player) {
                    if dbg {
                        let failed: Vec<String> = d
                            .refs("conditions")
                            .iter()
                            .filter_map(|(sid, v)| self.data.get(sid).map(|c| (c, v)))
                            .map(|(c, v)| format!("{}{}={}", c.ints.get("condition name").copied().unwrap_or(0), if c.ints.get("who") == Some(&1) { "(t)" } else { "" }, v[0]))
                            .collect();
                        eprintln!("  {} conditions fail: {failed:?}", d.name);
                    }
                    continue;
                }
                let Some(line) = self.first_line(d.refs("lines"), npc, player).cloned() else {
                    if dbg {
                        eprintln!("  {}: no line passes", d.name);
                    }
                    continue;
                };
                let score = d.ints.get("score bonus").copied().unwrap_or(0) as f32 + self.rng.uniform() * 0.5;
                if best.as_ref().is_none_or(|b| score > b.0) {
                    best = Some((score, d, line));
                }
            }
        }
        if let Some((_, d, line)) = best {
            if std::env::var("KENSHI_MEET_DEBUG").is_ok() {
                eprintln!("  chose dialogue {} ({})", d.name, d.string_id);
            }
            let now = self.hours_gone();
            self.dialog_memory.spoken.insert((npc, d.string_id.clone()), now);
            self.dialog_memory.met.insert((npc, player), now);
            self.dialog_memory.partner.insert(npc, player);
            self.dialog_memory.running.insert(npc, d.string_id.clone());
            let mut effects = self.effects(&line, npc);
            let conv = self.at_line(&line, npc, player);
            effects.append(&mut self.pending_effects);
            return Some((conv, effects));
        }
        None
    }

    /// The player picks reply `choice` (a line id): its effects, then the NPC's answer (None:
    /// the conversation is over).
    pub fn choose(&mut self, conv: &Conversation, choice: &str) -> (Vec<Effect>, Option<Conversation>) {
        let Some(line) = self.data.get(choice).cloned() else { return (Vec::new(), None) };
        let mut effects = self.effects(&line, conv.npc);
        // the NPC's answer: the reply's first passing NPC line; a reply line can itself carry on
        let answer = self.first_line(line.refs("lines"), conv.npc, conv.player).cloned();
        let next = answer.map(|a| {
            effects.extend(self.effects(&a, conv.npc));
            let speaker = a.ints.get("speaker").copied().unwrap_or(T_ME);
            if speaker == T_ME {
                self.at_line(&a, conv.npc, conv.player)
            } else {
                // a player line in a row: offered as the one choice
                let r = self.rng.uniform();
                Conversation { npc: conv.npc, player: conv.player, npc_text: conv.npc_text.clone(), choices: vec![(a.string_id.clone(), text(&a, r))] }
            }
        });
        effects.append(&mut self.pending_effects);
        (effects, next)
    }

    /// Whether `npc` may speak dialogue `d` to `player` now (the DIALOGUE fields, fcs.def): not
    /// "locked" unless a line unlocked it, not locked since; "for enemies" or the speaker's
    /// faction not at war with the player's; its "repetition limit" passed since last spoken;
    /// its "chance permanent" (rolled once per speaker) and "chance temporary" (each time).
    /// (stand-in) "target is type" (CharacterTypeEnum) is not read: such dialogues are not
    /// spoken.
    fn dialogue_open(&mut self, d: &GameData, npc: usize, player: usize, event: i32) -> bool {
        let key = (npc, d.string_id.clone());
        let flag = |k: &str| d.bools.get(k).copied().unwrap_or(false);
        let num = |k: &str, default: f32| d.floats.get(k).copied().or_else(|| d.ints.get(k).map(|&v| v as f32)).unwrap_or(default);
        if self.dialog_memory.locked.contains(&key) || (flag("locked") && !self.dialog_memory.unlocked.contains(&key)) {
            return false;
        }
        if d.ints.get("target is type").copied().unwrap_or(0) != 0 {
            return false;
        }
        if event == EV_PLAYER_TALK_TO_ME && !flag("for enemies") {
            let (mine, theirs) = (self.folk[npc].faction.clone(), self.folk[player].faction.clone());
            if mine.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(theirs.as_deref())) {
                return false;
            }
        }
        let wait = repetition_hours(d.ints.get("repetition limit").copied().unwrap_or(0));
        if let Some(&last) = self.dialog_memory.spoken.get(&key)
            && self.hours_gone() < last + wait
        {
            return false;
        }
        let permanent = num("chance permanent", 100.0);
        if permanent < 100.0 {
            let roll = self.rng.uniform() * 100.0;
            if !*self.dialog_memory.has.entry(key.clone()).or_insert(roll < permanent) {
                return false;
            }
        }
        let temporary = num("chance temporary", 100.0);
        !(temporary < 100.0 && self.rng.uniform() * 100.0 >= temporary)
    }

    /// A line's effects (DialogActionEnum), the money and relation ones applied here; its
    /// "unlocks" (the running dialogue locked after), "unlock but keep me" and "locks" refs and
    /// DA_LOCK_THIS_DIALOG (36) applied to the speaker's dialogues.
    fn effects(&mut self, line: &GameData, npc: usize) -> Vec<Effect> {
        let running = self.dialog_memory.running.get(&npc).cloned();
        for (sid, _) in line.refs("unlocks") {
            self.dialog_memory.unlocked.insert((npc, sid.clone()));
            self.dialog_memory.locked.remove(&(npc, sid.clone()));
            if let Some(r) = &running {
                self.dialog_memory.locked.insert((npc, r.clone()));
                self.dialog_memory.unlocked.remove(&(npc, r.clone()));
            }
        }
        for (sid, _) in line.refs("unlock but keep me") {
            self.dialog_memory.unlocked.insert((npc, sid.clone()));
            self.dialog_memory.locked.remove(&(npc, sid.clone()));
        }
        for (sid, _) in line.refs("locks") {
            self.dialog_memory.locked.insert((npc, sid.clone()));
        }
        let mut out = Vec::new();
        for (sid, v) in line.refs("effects") {
            let Some(a) = self.data.get(sid) else { continue };
            let value = a.ints.get("action value").copied().map(|x| x as f32).or_else(|| a.floats.get("action value").copied()).unwrap_or(v[0] as f32);
            let action = a.ints.get("action name").copied().unwrap_or(0);
            match action {
                1 => out.push(Effect::Trade),
                3 | 18 => out.push(Effect::Recruit),
                4 => out.push(Effect::Relations(value)),
                6 | 21 => out.push(Effect::Attack),
                // DA_TAKE_MONEY: the speaker takes the player's; DA_GIVE_MONEY: gives
                8 => out.push(Effect::Money(-(value as i32))),
                9 => out.push(Effect::Money(value as i32)),
                36 => {
                    if let Some(r) = &running {
                        self.dialog_memory.locked.insert((npc, r.clone()));
                    }
                }
                // DA_GO_HOME / DA_RUN_AWAY: the speaker's squad leaves for home. (stand-in) A
                // squad of the world goes home as its goal does ("run away" at the same pace);
                // a town's own people stay.
                7 | 34 => {
                    if let Some(k) = self.world.squads.iter().position(|s| s.members.contains(&npc)) {
                        self.go_home(k);
                    }
                }
                // DA_FLAG_TEMP_ALLY: the speaker's squad no longer fights the player's side.
                // (assumption) Until the zone is reloaded: the flag's timeout is not read.
                20 => self.temp_ally(npc),
                // DA_CRIME_ALARM: the speaker's people about (within 1000 units, its faction) turn
                // on the player's side and go for the one it talks to. (assumption) As the alarm
                // a kidnapping raises (F031292); the guards' arresting is not ported.
                33 => out.push(Effect::Alarm),
                40 => out.push(Effect::EnslaveCarried),
                41 => out.push(Effect::SellSlaves),
                42 => out.push(Effect::BuySlaves),
                2 => out.push(Effect::TalkToLeader),
                // DA_PAY_BOUNTY / DA_CLEAR_BOUNTY: the bounty with the speaker's faction paid /
                // cleared
                10 | 49 => {
                    if let (Some(f), Some(p)) = (self.folk[npc].faction.clone(), self.talking_to(npc)) {
                        if action == 10 {
                            self.pay_bounty(p, &f);
                        } else {
                            self.folk[p].bounties.retain(|b| b.faction != f);
                        }
                    }
                }
                // DA_FORCE_SPEECH_TIMER: the speaker says nothing more for that many hours
                12 => {
                    let at = self.hours_gone() + value.max(0.0) as f64;
                    self.bark_next.insert(npc, at);
                }
                // DA_FOLLOW_WHILE_TALKING: (assumption) the speaker keeps up with the one it
                // talks to
                16 => {
                    if let Some(p) = self.talking_to(npc) {
                        self.order_follow(&[npc], p, false);
                    }
                }
                // DA_REMEMBER_CHARACTER
                19 => {
                    if let Some(p) = self.talking_to(npc) {
                        let now = self.hours_gone();
                        self.dialog_memory.met.insert((npc, p), now);
                    }
                }
                // DA_MATES_KILL_ME: its own squad turns on it
                22 => {
                    let mates: Vec<usize> = self.squad_of(npc).into_iter().filter(|&k| k != npc).collect();
                    if !mates.is_empty() {
                        self.order_attack(&mates, npc);
                    }
                }
                // DA_GIVE_TARGET_MY_SLAVES: the slaves of its squad join the player's, still
                // shackled (as bought ones)
                24 => {
                    let owner = self.folk[npc].platoon;
                    let slaves: Vec<usize> = self.folk.iter().filter(|p| owner.is_some() && p.slave.as_ref().is_some_and(|s| s.owner == owner)).map(|p| p.id).collect();
                    for s in slaves {
                        if let Some(c) = self.folk[s].prison.as_ref().map(|pr| pr.cage) {
                            self.leave_cage(s, c);
                        }
                        self.folk[s].slave = None;
                        self.folk[s].prison = None;
                        self.folk[s].shackled = false;
                        self.recruit(s);
                    }
                }
                // DA_TAG_ESCAPED_SLAVE: the one it talks to is taken for its faction's escaped
                // slave
                25 => {
                    if let (Some(f), Some(p)) = (self.folk[npc].faction.clone(), self.talking_to(npc)) {
                        let owner = self.folk[npc].platoon;
                        self.folk[p].slave = Some(crate::slavery::Slavery { owner, faction: f.clone(), obedient: false, doing: crate::slavery::SLAVE_CAGE, work_at: None, sent: None, fled: true, machine: None });
                        self.folk[p].escaped = Some((f, self.hours_gone() + 24.0));
                    }
                }
                // DA_FREE_TARGET_SLAVE / DA_REMOVE_SLAVE_STATUS
                26 | 53 => {
                    let who = if action == 53 { Some(npc) } else { self.talking_to(npc) };
                    if let Some(s) = who {
                        if let Some(c) = self.folk[s].prison.as_ref().filter(|pr| pr.slave).map(|pr| pr.cage) {
                            self.leave_cage(s, c);
                            self.folk[s].prison = None;
                        }
                        self.folk[s].slave = None;
                        self.folk[s].escaped = None;
                        self.remove_shackles(s);
                    }
                }
                // DA_SEPARATE_TO_MY_OWN_SQUAD: the speaker leaves its squad for one of its own
                28 => {
                    let pid = self.bar_states.len();
                    self.bar_states.push(None);
                    self.folk[npc].platoon = Some(pid);
                    self.folk[npc].role = 5;
                }
                // DA_ARREST_TARGET / DA_ARREST_TARGETS_CARRIED_PERSON
                29 | 30 => {
                    if let Some(p) = self.talking_to(npc) {
                        let target = if action == 30 { self.folk[p].carrying } else { Some(p) };
                        if let Some(t) = target {
                            self.folk[npc].arrest = Some(crate::law::Arrest { target: t, slaver: false });
                            if !self.folk[t].down && action == 29 {
                                self.order_attack(&[npc], t);
                            }
                        }
                    }
                }
                // DA_ASSIGN_BOUNTY: a bounty with its faction ("action value" cats, else as a
                // crime of assault)
                32 => {
                    if let (Some(f), Some(p)) = (self.folk[npc].faction.clone(), self.talking_to(npc)) {
                        self.add_bounty(p, &f, kenshi_ai::CrimeEnum::CrimeAssault);
                        if value > 0.0
                            && let Some(b) = self.folk[p].bounties.iter_mut().find(|b| b.faction == f)
                        {
                            b.amount = b.amount.max(value as i32);
                        }
                    }
                }
                // DA_ATTACK_TOWN / DA_ASSAULT_PHASE / DA_ATTACK_STAY_NEAR_HOME /
                // DA_ATTACK_IF_NO_COEXIST: (assumption) its squad attacks the one it talks to;
                // DA_RETREAT_PHASE: its squad goes home; DA_VICTORY_PHASE: nothing more
                31 | 37 | 55 | 57 => out.push(Effect::Attack),
                38 => {
                    if let Some(k) = self.world.squads.iter().position(|s| s.members.contains(&npc)) {
                        self.go_home(k);
                    }
                }
                // DA_MASSIVE_ALARM: as the crime alarm
                56 => out.push(Effect::Alarm),
                // DA_KNOCKOUT: the one it talks to is knocked out
                58 => {
                    if let Some(p) = self.talking_to(npc) {
                        self.knock_out(p);
                    }
                }
                // DA_SUMMON_MY_SQUAD: its squad comes to it
                52 => {
                    let mates: Vec<usize> = self.squad_of(npc).into_iter().filter(|&k| k != npc).collect();
                    let at = self.folk[npc].mover.position_units();
                    if !mates.is_empty() {
                        self.order_move_squad(&mates, at);
                    }
                }
                // not ported: reputation, faction ranks, war and peace declarations, the
                // character editor, clearing the AI, thug hunters, running faster, merging
                // squads, bail, conscription, recruiting and hiring windows, animals' surrender
                // and buying, selling prisoners, opening gates
                _ => {}
            }
        }
        // the line's other fields: "change relations" (its faction's relation to the
        // player's by the ref's value), "change AI" / "AI contract" (the speaker's squad on
        // that package; (assumption) a contract's hours not kept), "give item" (to the one
        // spoken to, the ref's first value of them), "trigger campaign" (due between the ref's
        // first and second values of hours, its third value the % chance)
        for (f, v) in line.refs("change relations") {
            if let Some(rel) = self.relations.get_mut(f) {
                let r = rel.relations.entry(PLAYER_FACTION.to_string()).or_insert(kenshi_ai::faction::Relation { forced_ally: false, relation: rel.default_relation });
                r.relation = (r.relation + v[0] as f32).clamp(-100.0, 100.0);
            }
        }
        if let Some((pkg, _)) = line.refs("change AI").first().or(line.refs("AI contract").first()) {
            let squad = self.squad_of(npc);
            for k in squad {
                self.set_package(k, pkg);
            }
        }
        if let Some(p) = self.talking_to(npc) {
            for (item, v) in line.refs("give item") {
                if let Some(r) = self.data.get(item) {
                    let it = kenshi_char::inventory::Item::from_record(r, v[0].max(1) as u32);
                    if !self.folk[p].inventory.add(it.clone(), false) {
                        self.drop_item(p, it);
                    }
                }
            }
        }
        for (c, v) in line.refs("trigger campaign") {
            let chance = if v[2] > 0 { v[2] as f32 } else { 100.0 };
            if self.rng.uniform() * 100.0 < chance {
                let (lo, hi) = (v[0].max(0) as f32, v[1].max(v[0]).max(0) as f32);
                let at = self.hours_gone() + (lo + (hi - lo) * self.rng.uniform()) as f64;
                self.world.triggered.push((c.clone(), at));
            }
        }
        for e in &out {
            match e {
                Effect::Money(m) => self.money = (self.money + m).max(0),
                Effect::Relations(d) => {
                    if let Some(f) = self.folk[npc].faction.clone()
                        && let Some(rel) = self.relations.get_mut(&f)
                    {
                        let r = rel.relations.entry(PLAYER_FACTION.to_string()).or_insert(kenshi_ai::faction::Relation { forced_ally: false, relation: rel.default_relation });
                        r.relation = (r.relation + d).clamp(-100.0, 100.0);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// DA_FLAG_TEMP_ALLY: `npc`'s squad stops counting the player's side among its foes.
    pub fn temp_ally(&mut self, npc: usize) {
        let player_side = self.side(&Some(PLAYER_FACTION.to_string()));
        let pl = self.folk[npc].platoon;
        for k in 0..self.folk.len() {
            if k == npc || (pl.is_some() && self.folk[k].platoon == pl) {
                if let Some(foes) = &mut self.arena.f[k].foes {
                    foes.retain(|&s| s != player_side);
                }
            }
        }
    }

    /// DA_CRIME_ALARM: `npc`'s faction's people within 1000 units turn on `target`'s side and go
    /// for them.
    pub fn alarm(&mut self, npc: usize, target: usize) {
        let side = self.arena.f[target].team;
        let (faction, at) = (self.folk[npc].faction.clone(), self.folk[npc].mover.position_units());
        let mut answering = Vec::new();
        for k in 0..self.folk.len() {
            let w = &self.folk[k];
            if w.player || w.down || w.dead || w.faction != faction {
                continue;
            }
            let q = w.mover.position_units();
            if (q[0] - at[0]).hypot(q[2] - at[2]) < 1000.0 {
                if let Some(foes) = &mut self.arena.f[k].foes
                    && !foes.contains(&side)
                {
                    foes.push(side);
                }
                answering.push(k);
            }
        }
        if !answering.is_empty() {
            self.order_attack(&answering, target);
        }
    }

    /// DA_JOIN_SQUAD: the NPC joins the player's squad.
    pub fn recruit(&mut self, npc: usize) {
        // its dialogue as one of the player's: its CHARACTER's "dialogue package player"
        let player_pkg = self.folk[npc].character.as_deref().and_then(|c| self.data.get(c)).and_then(|c| c.refs("dialogue package player").first().map(|(s, _)| s.clone()));
        if player_pkg.is_some() {
            self.folk[npc].dialogue = player_pkg;
        }
        let p = &mut self.folk[npc];
        p.player = true;
        p.faction = Some(PLAYER_FACTION.to_string());
        p.ai.queue.clear();
        p.home = None;
        p.shop = None;
        let side = self.side(&Some(PLAYER_FACTION.to_string()));
        self.arena.f[npc].team = side;
    }
}

impl Sim {
    /// Every second: roaming squads' leaders that see one of the player's characters near
    /// (within 400 units) start their EV_I_SEE_NEUTRAL_SQUAD dialogue with it ((assumption)
    /// the game's repetition limits not read), walking up to it; the
    /// conversations wait in `npc_talks` for the game to show.
    pub fn meet_tick(&mut self) {
        let players: Vec<(usize, [f32; 3])> = self.folk.iter().filter(|p| p.player && !p.down).map(|p| (p.id, p.mover.position_units())).collect();
        if players.is_empty() {
            return;
        }
        let now = self.hours_gone();
        let ids: Vec<usize> = self
            .folk
            .iter()
            .filter(|p| !p.player && !p.down && !p.dead && (p.dialogue.is_some() || p.package.is_some()) && p.home.is_none() && p.race.is_none() && p.role == 5)
            .filter(|p| self.met.get(&p.id).is_none_or(|&next| now >= next))
            .map(|p| p.id)
            .collect();
        for npc in ids {
            if self.arena.f[npc].active {
                continue;
            }
            let at = self.folk[npc].mover.position_units();
            let Some(&(player, _)) = players.iter().find(|(_, q)| (q[0] - at[0]).hypot(q[2] - at[2]) < 400.0) else { continue };
            let r = self.talk_on(npc, player, EV_I_SEE_NEUTRAL_SQUAD, true);
            // (assumption) a missed roll is tried again in a tenth of an hour; one spoken, not
            // for two hours
            self.met.insert(npc, now + if r.is_some() { 2.0 } else { 0.1 });
            if std::env::var("KENSHI_MEET_DEBUG").is_ok() {
                eprintln!("meet: {npc} ({:?} / {:?}) sees {player}: {}", self.folk[npc].dialogue, self.folk[npc].package, if r.is_some() { "talks" } else { "nothing" });
            }
            // (assumption) the greeting is said where it stands: no standing order to walk over,
            // which would hold it there (a caravan would never leave)
            // a line with replies is a conversation; one without is said aloud
            if let Some((mut conv, effects)) = r {
                if conv.choices.is_empty() {
                    let text = std::mem::take(&mut conv.npc_text);
                    self.say(npc, text);
                    if effects.is_empty() {
                        continue;
                    }
                }
                self.npc_talks.push((conv, effects));
            }
        }
    }
}

