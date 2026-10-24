//! Dialogue (port_map `dialogue`): talking to people. A person's DIALOGUE_PACKAGE lists
//! DIALOGUEs by the event that starts them (EventTrigger, T00044); talking to someone runs its
//! EV_PLAYER_TALK_TO_ME dialogue whose conditions pass. A DIALOGUE's "lines" open it; each
//! DIALOGUE_LINE has text variants ("text0".."textN"), a speaker (TalkerEnum), conditions and
//! effects (DIALOG_ACTION records: DialogConditionEnum / DialogActionEnum / ComparisonEnum, from
//! the FCS editor), and its replies in "lines".
//!
//! Which is said (F040900, F040892, F040903): every DIALOGUE and DIALOGUE_LINE has a score
//! from how particular it is (`line_score`); a speaker says the best-scoring line that passes
//! (ties at random), and of the dialogues an event may start, the one whose own score, opening
//! line's and best reply's add up highest. Conditions not ported read as 0; effects not
//! ported do nothing.
use crate::sim::{PLAYER_FACTION, Sim};
use kenshi_data::GameData;

/// EventTrigger EV_PLAYER_TALK_TO_ME.
pub const EV_PLAYER_TALK_TO_ME: i32 = 1;
/// EventTrigger EV_ANNOUNCEMENT: a squad's leader announcing itself (a campaign's demands).
pub const EV_ANNOUNCEMENT: i32 = 2;
/// EventTrigger EV_I_SEE_NEUTRAL_SQUAD: a squad meets someone it is not at war with.
pub const EV_I_SEE_NEUTRAL_SQUAD: i32 = 3;
/// EventTrigger EV_I_DEFEATED_SQUAD: the last of a squad beaten down.
pub const EV_I_DEFEATED_SQUAD: i32 = 15;

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
    /// Each faction's rank (its FactionRelations +0x10: DA_INCREASE_FACTION_RANK, F040912 case
    /// 0x23, read by DC_FACTION_RANK) and reputation (+0x18, the lines' values added up,
    /// F041492: DA_AFFECT_REPUTATION, DC_REPUTATION).
    pub ranks: std::collections::HashMap<String, i32>,
    pub reputation: std::collections::HashMap<String, f32>,
    /// How many conversations each NPC has had of each event (DC_NUM_DIALOG_EVENT_REPEATS,
    /// F040866: counted as one ends; here as the NPC's next starts), and the event of the one
    /// it is in.
    pub event_count: std::collections::HashMap<(usize, i32), u32>,
    pub running_event: std::collections::HashMap<usize, i32>,
    /// The dialogue each NPC starts if the player talks to it now (a line's "interrupt"), and
    /// until when (game hours). (assumption) Kept an hour: the game keeps it to the end of the
    /// NPC's conversation, and a line said aloud has none.
    pub interrupt: std::collections::HashMap<usize, (String, f64)>,
    /// The bounties paid out by DA_PAY_BOUNTY (CharacterCrimes' entry +0x10, F046514): (the
    /// wanted, the faction).
    pub claimed: std::collections::HashSet<(usize, String)>,
    /// Who sees which of the player's characters (the sensors' sightings: a new one raises
    /// the sighting events, `speech::sighted`).
    pub in_sight: std::collections::HashSet<(usize, usize)>,
    /// Each medic's patient while first aid runs (its events, `Sim::first_aid_events`), and the
    /// medics who have said their kit is empty.
    pub healing: std::collections::HashMap<usize, usize>,
    pub kit_empty: std::collections::HashSet<usize>,
    /// Who was fighting at the last second (`speech::combat_events`).
    pub fighting: std::collections::HashSet<usize>,
    /// Whether each (package, character, faction)'s packages answer sightings of others
    /// (`speech::answers_sightings`).
    pub sighting_packages: std::collections::HashMap<(Option<String>, Option<String>, Option<String>), bool>,
    /// The dialogue events each NPC has delivered (the Dialogue's +0xf8 map, event -> its squad's
    /// handles, filled as a conversation of the event ends, F040866, and read by
    /// isDialogDelivered, F040852; never emptied while the Dialogue lives): recorded as its
    /// conversation ends with no one handed on to (crate::conversation, F040866), for the
    /// speaker itself; (stand-in) the player's window's as it opens (its close not kept here);
    /// not saved.
    pub delivered: std::collections::HashSet<(usize, i32)>,
    /// When each NPC's conversation of each event last ended (game hours: the repetition
    /// counter's time, F040866).
    pub event_time: std::collections::HashMap<(usize, i32), f64>,
}

/// A squad hired on a line's "AI contract" (F028601): on the package for the ref's first value
/// of hours, then back on its own, its leader saying the package's "contract end talk
/// passive" dialogue aloud or starting its "contract end dialog delivery" with whoever hired it
/// (F028935), else its own EV_CONTRACT_JOB_ENDED dialogue.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Contract {
    pub members: Vec<usize>,
    pub leader: usize,
    pub hirer: usize,
    pub package: String,
    /// Each member's package before.
    pub was: Vec<Option<String>>,
    /// When it ends (game hours).
    pub until: f64,
}

/// (assumption) How long a squad keeps a short-term tag after it was last set (game hours): the
/// game keeps the time (SubjectiveTags +0x08, saved as "tagtime") but where it lets them go is
/// not decompiled.
pub const SHORT_TAG_HOURS: f64 = 24.0;


/// EventTrigger EV_BOUGHT_ME_FROM_SLAVERY (bailed out too, F041464).
pub const EV_BOUGHT_ME_FROM_SLAVERY: i32 = 51;
/// EventTrigger EV_CROWD_TRIGGERED.
pub const EV_CROWD_TRIGGERED: i32 = 74;

/// EventTrigger EV_CONTRACT_JOB_ENDED.
pub const EV_CONTRACT_JOB_ENDED: i32 = 30;

/// (assumption) A player outpost's size level (0..3, the campaigns' "target tech level >="
/// and DC_TOWN_LEVEL_*) from its buildings: none 0, under 5 one, under 15 two, else three. The
/// game keeps it per player town (+0x388, saved as "plevel"); how it is worked out is not
/// decompiled.
pub fn town_level(buildings: usize) -> i32 {
    match buildings {
        0 => 0,
        1..=4 => 1,
        5..=14 => 2,
        _ => 3,
    }
}

/// CharacterPerceptionTags_LongTerm.
pub const LT_MY_LIFESAVER: u32 = 2;
pub const LT_FREED_ME: u32 = 3;
pub const LT_STOLE_FROM_ME: u32 = 4;
pub const LT_MY_CAPTOR: u32 = 5;
pub const LT_DEFEATED_MY_SQUAD_ONCE: u32 = 7;
pub const LT_SQUAD_LOST_TO_ME_ONCE: u32 = 8;
pub const LT_KILLED_MY_FRIEND: u32 = 9;

/// @fn F040900
///
/// DialogRepetitionEnum -> the game hours before a dialogue or line may be said again by the
/// same speaker: none, 2, 6, 48, a week, once only (5000, and "once only" set), 0.33.
pub fn repetition_hours(limit: i32) -> f64 {
    match limit {
        1 => 2.0,
        2 => 6.0,
        3 => 48.0,
        4 => 168.0,
        5 => f64::INFINITY,
        6 => 0.33,
        _ => 0.0,
    }
}

/// @fn F040900
///
/// A DIALOGUE's or DIALOGUE_LINE's score, worked out when the game loads it: its "score bonus";
/// +1 said by anyone but the speaker (TalkerEnum not T_ME), +1 more by an interjector or the
/// whole squad (3..6); +2 "locked" (a DIALOGUE); +2 each for having "in town of", "my
/// faction", "target faction", "target has item type", "target race"; +2 each "target has
/// item" and "world state"; +1 each condition (+1 more for a DC_HAS_TAG or DC_PERSONALITY_TAG
/// that must hold); +1 "unique"; +1 "chance permanent" under 99; +1 "target is type"; +2
/// "interjection".
pub fn line_score(data: &kenshi_data::GameDataContainer, l: &GameData) -> i32 {
    let int = |k: &str| l.ints.get(k).copied().unwrap_or(0);
    let flag = |k: &str| l.bools.get(k).copied().unwrap_or(false);
    let has = |k: &str| !l.refs(k).is_empty();
    let speaker = int("speaker");
    let mut s = int("score bonus") as i32;
    if speaker != T_ME {
        s += 1;
    }
    if l.item_type() == Some(kenshi_data::ItemType::Dialogue) && flag("locked") {
        s += 2;
    }
    for k in ["in town of", "my faction", "target faction", "target has item type", "target race"] {
        if has(k) {
            s += 2;
        }
    }
    s += 2 * l.refs("target has item").len() as i32;
    s += 2 * l.refs("world state").len() as i32;
    for (sid, v) in l.refs("conditions") {
        let Some(c) = data.get(sid) else { continue };
        s += 1;
        let name = c.ints.get("condition name").copied().unwrap_or(0);
        if (name == 23 || name == 26) && v[0] == 1 {
            s += 1;
        }
    }
    if flag("unique") {
        s += 1;
    }
    let permanent = l.floats.get("chance permanent").copied().unwrap_or(100.0);
    if permanent < 99.0 {
        s += 1;
    }
    if int("target is type") > 0 {
        s += 1;
    }
    if flag("interjection") {
        s += 2;
    }
    if (3..=6).contains(&speaker) {
        s += 1;
    }
    s
}

/// What a choice did, for the game to act on.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Trade,
    Recruit,
    /// DA_JOIN_SQUAD_WITH_EDIT: joins, then the character editor on it.
    RecruitEdit,
    Attack,
    Money(i32),
    Relations(f32),
    /// DA_CRIME_ALARM: raised on the one spoken to.
    Alarm,
    /// CHOOSE_SLAVES_SELLING / CHOOSE_SLAVES_BUYING: the slave trade windows.
    SellSlaves,
    BuySlaves,
    /// CHOOSE_PRISONER_BAIL: the window to pay out the player's people the speaker's faction
    /// holds.
    Bail,
    /// DA_ENSLAVE_TARGETS_CARRIED_PERSON: whom the player carries becomes the speaker's slave.
    EnslaveCarried,
    /// CHOOSE_ANIMALS_BUYING: the animal trade window on the speaker's animals, at this price
    /// each when over 0 (the action's value).
    BuyAnimals(i32),
    /// DA_CHARACTER_EDITOR: the character editor on the one spoken to (the plastic surgeon).
    CharacterEditor,
    /// DA_MASSIVE_ALARM: raised on the one spoken to (`Sim::massive_alarm`).
    MassiveAlarm,
    /// DA_TALK_TO_LEADER: the speaker's squad leader (itself when it leads) talks with the one
    /// spoken to.
    TalkToLeader,
    /// The talk ends (F040866): the player's faction too full for a recruit.
    End,
}

/// A conversation in progress: the NPC's line and the player's choices (line id, text).
#[derive(Debug, Clone)]
pub struct Conversation {
    pub npc: usize,
    pub player: usize,
    pub npc_text: String,
    pub choices: Vec<(String, String)>,
}

/// Whether a DIALOGUE_PACKAGE or DIALOGUE is left out as unfinished (F040909, F040904,
/// F040927): its name has "Unfinished" in it.
pub fn unfinished(r: &GameData) -> bool {
    r.name.contains("Unfinished")
}

/// A number as Kenshi's "{1,num}" formats it: thousands separated by commas.
pub fn num_text(n: i32) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// One of a line's text variants, at random.
pub(crate) fn text(line: &GameData, rand: f32) -> String {
    let variants: Vec<&String> = (0..).map_while(|i| line.strings.get(&format!("text{i}"))).filter(|t| !t.is_empty()).collect();
    if variants.is_empty() {
        return String::new();
    }
    variants[((rand * variants.len() as f32) as usize).min(variants.len() - 1)].clone()
}

impl Sim {
    /// @fn F041489
    ///
    /// Faction `a`'s relation to faction `b`: its own 100; else its entry for `b`, else its
    /// default (from its record when not yet kept).
    fn relation_of(&self, a: Option<&str>, b: Option<&str>) -> f32 {
        let (Some(a), Some(b)) = (a, b) else { return 0.0 };
        if a == b {
            return 100.0;
        }
        match self.relations.get(a) {
            Some(r) => r.relation(b),
            None => self.data.get(a).map_or(0.0, |r| kenshi_ai::faction::FactionRelations::from_data(r, &self.data).relation(b)),
        }
    }

    /// Person `id`'s faction as relations read it (the player's people: the player's).
    pub(crate) fn faction_key(&self, id: usize) -> Option<String> {
        let p = &self.folk[id];
        if p.player { Some(PLAYER_FACTION.to_string()) } else { p.faction.clone() }
    }

    /// What `holder`'s squad remembers of `of` (its platoon memory, PlatoonMemory +0x98: here
    /// each member's long-term tags together), and whether it has an entry for `of` at all (met,
    /// or tagged).
    pub(crate) fn squad_tags(&self, holder: usize, of: usize) -> (u32, bool) {
        let mut bits = 0;
        let mut any = false;
        for m in self.squad_of_any(holder) {
            if let Some(&t) = self.dialog_memory.tags.get(&(m, of)) {
                bits |= t;
                any = true;
            }
            any |= self.dialog_memory.met.contains_key(&(m, of));
        }
        (bits, any)
    }

    /// @fn F040850
    ///
    /// Game hours since `holder`'s squad's memory of `of` was last made (its platoon memory
    /// entry's time, set as it tags or remembers it, F040862 / F040854, or talks with it:
    /// `DialogMemory::met`); no entry, 9999.99.
    fn hours_since_seen(&self, holder: usize, of: usize) -> f32 {
        let now = self.hours_gone();
        self.squad_of_any(holder).iter().filter_map(|&m| self.dialog_memory.met.get(&(m, of))).map(|&at| (now - at) as f32).fold(None, |a: Option<f32>, h| Some(a.map_or(h, |a| a.min(h)))).unwrap_or(9999.99)
    }

    /// @fn F044493
    ///
    /// Whether person `id`'s squad is down (F040880 asks with 1): more than two of it, at most
    /// one still standing; two or fewer, none.
    fn squad_down(&self, id: usize) -> bool {
        let squad = self.squad_of(id);
        let allowed = if squad.len() <= 2 { 0 } else { 1 };
        squad.iter().filter(|&&k| !self.folk[k].down).count() <= allowed
    }

    /// @fn F047045
    ///
    /// A character's fighting level for recruiting: (the better of martial arts (CharStats
    /// +0x10c) and melee attack (+0x120), of dodge (+0xf0) and melee defence (+0x124), of
    /// dexterity (+0x88) and strength (+0x80)) / 3.
    fn recruit_level(&self, id: usize) -> f32 {
        use kenshi_char::StatsEnumerated::*;
        let l = |s| self.arena.f[id].level(s);
        (l(StatMartialarts).max(l(StatMeleeAttack)) + l(StatDodge).max(l(StatMeleeDefence)) + l(StatDexterity).max(l(StatStrength))) / 3.0
    }

    /// GLOBAL CONSTANTS "max faction size" (+0x180; 30 with the game's data): how many people
    /// the player's faction may have.
    pub fn max_faction_size(&self) -> usize {
        self.data.get("110-gamedata.quack").and_then(|g| g.ints.get("max faction size").copied()).unwrap_or(30).max(1) as usize
    }

    /// The player's faction's people (F045498: its squads' members), the dead not counted.
    pub fn player_count(&self) -> usize {
        self.folk.iter().filter(|p| p.player && !p.dead).count()
    }

    /// @fn F040880
    ///
    /// One DialogConditionEnum `cond` asked of `subject` (the speaker, or the one it talks to:
    /// the record's "who", F040888) by `speaker`, talking to `target`, against `value` by
    /// ComparisonEnum `compare`. A yes/no condition holds as its value is over 0 or not; a
    /// count or amount is compared by `compare` (CE_LESS_THAN, CE_MORE_THAN, else equal), some
    /// (relations, money, reputation, bounties, last seen) only as less than, or at least.
    /// Conditions it does not know hold (the faction's own test, F040815, says yes).
    #[allow(clippy::too_many_arguments)]
    fn condition(&self, cond: i32, compare: i32, value: i32, subject: usize, speaker: usize, target: usize) -> bool {
        let on = value > 0;
        let eq = |x: bool| x == on;
        let cmp3 = |x: i64| match compare {
            1 => x < value as i64,
            2 => x > value as i64,
            _ => x == value as i64,
        };
        let lt_ge = |x: i64| if compare == 1 { x < value as i64 } else { x >= value as i64 };
        let (s, sp) = (&self.folk[subject], &self.folk[speaker]);
        let sp_faction = self.faction_key(speaker);
        let at = |k: usize| self.folk[k].mover.position_units();
        let flat = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
        match cond {
            // DC_NUM_DIALOG_EVENT_REPEATS (F041344): the speaker's conversations of the event
            // it is in
            13 => {
                let ev = self.dialog_memory.running_event.get(&speaker).copied().unwrap_or(0);
                cmp3(self.dialog_memory.event_count.get(&(speaker, ev)).copied().unwrap_or(0) as i64)
            }
            // DC_TARGET_CHARACTER_EXISTS: here the one asked about always does
            76 => eq(true),
            // DC_BUILDING_IS_CLOSED_AND_SECURED (F044452 +0xe8): the speaker's home locked up
            11 => eq(sp.home.is_some_and(|h| self.locked_up(h))),
            // DC_PLAYER_TECH_LEVEL (the research's +8). (assumption) The highest "level" of the
            // research done.
            12 => cmp3(self.research_done.iter().filter_map(|r| self.data.get(r)).filter_map(|r| r.ints.get("level").copied()).max().unwrap_or(0) as i64),
            // DC_IS_ALLY / DC_IS_ENEMY (F041495, F041494): the speaker's faction toward the
            // subject's (50 or more, or forced, or its own; -30 or less)
            24 | 25 => {
                let theirs = self.faction_key(subject);
                let rel = self.relation_of(sp_faction.as_deref(), theirs.as_deref());
                let forced = sp_faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_ally(theirs.as_deref()));
                if cond == 24 { eq(forced || rel >= kenshi_ai::faction::ALLY_RELATION) } else { eq(sp_faction != theirs && rel <= kenshi_ai::faction::ENEMY_RELATION) }
            }
            // DC_TOWN_LEVEL_CURRENT_LOCATION: the player's outpost the speaker stands in (+0x388,
            // `town_level`); in none, it fails whatever the value
            73 => {
                let here = self.outpost_level(at(speaker));
                here > 0 && cmp3(here as i64)
            }
            // DC_PLAYERS_BEST_TOWN_LEVEL: the biggest of the player's outposts, -1 with none
            74 => cmp3(self.buildings.iter().filter(|b| b.confirmed).map(|b| self.outpost_level(b.at)).max().map_or(-1, |l| l as i64)),
            // DC_TARGET_IN_TALKING_RANGE: on the same floor (vtable 0x60: (assumption) in the same
            // building, or both outside) and within 260 units (67600 squared)
            16 => eq(s.inside == sp.inside && flat(at(subject), at(speaker)) < 260.0),
            // DC_TARGET_LAST_SEEN_X_HOURS_AGO: as less than, or at least
            18 => lt_ge(if subject == speaker { 9999 } else { self.hours_since_seen(speaker, subject) as i64 }),
            // DC_MET_TARGET_BEFORE (F040872): the speaker's squad's memory has it
            20 => eq(subject != speaker && self.squad_tags(speaker, subject).1),
            // DC_I_LOVE_THIS_GUY (F038591): it remembers the subject saved it or freed it
            // (LT_MY_LIFESAVER, LT_FREED_ME); DC_I_HATE_THIS_GUY (F038592): stole from it,
            // caged it, beat its squad or killed its friend
            52 => eq(self.squad_tags(speaker, subject).0 & 0xc != 0),
            53 => eq(self.squad_tags(speaker, subject).0 & 0x2b0 != 0),
            // DC_I_SHOULD_HELP_THIS_GUY (F038615): the speaker's class (its CHARACTER "NPC
            // class", +0xdc) the subject's; not when it is an escaping slave (+0xd8 2) or a
            // bandit; yes for an ally of its (Character vtable 0x3f0: here its faction's ally,
            // or itself), for two slaves (+0xd8 1 both), for one its squad remembers kindly
            // (LT_MY_LIFESAVER, LT_FREED_ME); never for one it remembers ill (0x2b0)
            55 => {
                let tags = self.squad_tags(speaker, subject).0;
                let ally = subject == speaker || self.relation_of(sp_faction.as_deref(), self.faction_key(subject).as_deref()) >= kenshi_ai::faction::ALLY_RELATION;
                let mut help = self.npc_class(speaker) == self.npc_class(subject);
                if self.slave_state(speaker) == 2 || self.npc_class(speaker) == 8 {
                    help = false;
                }
                if ally || (self.slave_state(speaker) == 1 && self.slave_state(subject) == 1) || tags & 0xc != 0 {
                    help = true;
                }
                if tags & 0x2b0 != 0 {
                    help = false;
                }
                eq(help)
            }
            // DC_I_SHOULD_SCREW_THIS_GUY_OVER (F038616): either a bandit, not of one class; yes
            // for an escaping slave, no for an ally or a slave; yes against its owners' faction
            // (+0xe0), no for one remembered kindly, yes for one remembered ill, yes for one
            // knocked out (+0x5bc)
            54 => {
                let tags = self.squad_tags(speaker, subject).0;
                let ally = subject == speaker || self.relation_of(sp_faction.as_deref(), self.faction_key(subject).as_deref()) >= kenshi_ai::faction::ALLY_RELATION;
                let mut screw = self.npc_class(speaker) == 8 || self.npc_class(subject) == 8;
                if self.npc_class(speaker) == self.npc_class(subject) {
                    screw = false;
                }
                if self.slave_state(speaker) == 2 {
                    screw = true;
                }
                if ally || self.slave_state(speaker) == 1 {
                    screw = false;
                }
                if sp.slave.as_ref().is_some_and(|x| Some(&x.faction) == self.faction_key(subject).as_ref()) {
                    screw = true;
                }
                if tags & 0xc != 0 {
                    screw = false;
                }
                if tags & 0x2b0 != 0 || s.down {
                    screw = true;
                }
                eq(screw)
            }
            // DC_TARGET_IS_SLAVE_OF_MY_FACTION (+0x1a0 +0xe0): the subject a slave of the
            // speaker's faction
            61 => eq(s.slave.as_ref().is_some_and(|x| Some(&x.faction) == sp.faction.as_ref())),
            // DC_IS_SAME_RACE_AS_ME: the race each is drawn as
            65 => eq(self.race_of(subject).is_some() && self.race_of(subject) == self.race_of(speaker)),
            // DC_STARVING: hunger under 1.7
            71 => eq(self.starving(subject)),
            // DC_IS_LEADER (F044494): it leads its squad
            19 => eq(s.role == 5 || s.platoon.is_none() || (s.player && self.squad_of(subject).first() == Some(&subject))),
            67 => eq(s.sneaking),
            68 => eq(s.inside.is_some()),
            // DC_BROKEN_LEG (+0x4d8 / +0x4e0, +0x60): a leg below no health; DC_BROKEN_ARM (+0x5bd,
            // +0x5be: the arms' use) (assumption) an arm below no health
            27 | 28 => {
                let kind = if cond == 27 { 1 } else { 2 };
                eq(self.arena.f[subject].medical.parts.iter().any(|p| p.kind == kind && p.health() < 0.0))
            }
            // DC_DAMAGED_HEAD (F040425): the head's flesh less its stun under 0.2
            29 => eq(self.arena.f[subject].medical.parts.iter().any(|p| p.kind == 3 && p.flesh - p.stun < 0.2)),
            // DC_HAS_AI_CONTRACT (+0x650, F036793 +0x210): the speaker's squad on a contract
            78 => eq(self.contracts.iter().any(|c| c.members.contains(&speaker))),
            // DC_IN_A_NON_PLAYER_TOWN: in a town (vtable 0x70 +0x88) not the player's, not a nest
            // (0, 8); ruins only the speaker's squad's own
            31 => eq(s.town.is_some_and(|t| {
                let tw = &self.towns[t];
                tw.faction.as_deref() != Some(PLAYER_FACTION) && tw.kind != 0 && tw.kind != 8 && (tw.kind != 4 || sp.home.and_then(|h| self.placed_town.get(h).copied().flatten()) == Some(t))
            })),
            // DC_IN_A_PLAYER_TOWN: in a town of the player's (here: among the player's
            // buildings, `outpost_level`)
            75 => eq(s.town.is_some_and(|t| self.towns[t].faction.as_deref() == Some(PLAYER_FACTION)) || self.outpost_level(at(subject)) > 0),
            // DC_USING_MY_TRAINING_EQUIPMENT (+0x158): the training equipment of the speaker's
            // faction
            70 => eq(self.training_owner(subject).is_some_and(|f| Some(&f) == sp.faction.as_ref())),
            56 => eq(self.arena.f[subject].active),
            // DC_IM_UNARMED (F038618): no weapon
            48 => eq(!s.inventory.equipped().any(|i| i.kind == Some(kenshi_data::ItemType::Weapon))),
            // DC_WITHIN_TOWN_WALLS (vtable 0x118, F044467): the gate code it stands in is above
            // 0 (a region behind a door or gate, crate::gate_codes)
            57 => eq(self.area(subject) > 0),
            // DC_TOWN_WALLS_LOCKED_UP (the subject's town's vtable 0x300, F048825): its gates
            // all shut (one open, or with no door, no); no gates or in no town, no.
            // (assumption) The town's gates: those within its radius.
            58 => eq(s.town.is_some_and(|t| {
                let gates: Vec<usize> = (0..self.door_states.len())
                    .filter(|&d| self.is_gate(d) && !self.door_states[d].gone && !self.door_states[d].broken)
                    .filter(|&d| self.towns[t].contains(self.door_states[d].door.position, 1.0))
                    .collect();
                !gates.is_empty() && gates.iter().all(|&d| self.door_states[d].phase == crate::sim::DOOR_CLOSED)
            })),
            // DC_IS_SLAVE (slave state 1 or 2), DC_IS_ESCAPED_SLAVE (2 or more)
            59 => eq(matches!(self.slave_state(subject), 1 | 2)),
            62 => eq(self.slave_state(subject) >= 2),
            // DC_IMPRISONED_BY_TARGET / _BY_OTHER: caged (+0x2f8 2), its cage's owner the
            // speaker's faction (the target's, asked of the speaker itself), or another
            7 | 8 => {
                let whose = if subject == speaker { self.faction_key(target) } else { sp_faction.clone() };
                match &s.prison {
                    Some(p) => eq((Some(&p.faction) == whose.as_ref()) == (cond == 7)),
                    None => eq(false),
                }
            }
            9 => eq(s.shop.is_some()),
            14 => eq(s.prison.is_some()),
            // DC_FACTION_RANK: the player's rank with the speaker's faction (FactionRelations
            // +0x10)
            10 => cmp3(sp.faction.as_ref().and_then(|f| self.dialog_memory.ranks.get(f)).copied().unwrap_or(0) as i64),
            // DC_IS_IN_LOCKED_CAGE (F029432): held in a cage; DC_WEARING_LOCKED_SHACKLES
            63 => eq(s.prison.as_ref().is_some_and(|p| !p.out)),
            64 => eq(s.shackled),
            // DC_BOUNTY_AMOUNT_PERCEIVED (F046512) / _ACTUAL (F046513): its bounty with the
            // speaker's faction, as less than or at least. (assumption) Perceived as actual.
            40 | 47 => lt_ge(sp.faction.as_deref().map_or(0, |f| self.bounty_with(subject, f)) as i64),
            36 => eq(s.player),
            41 => eq(s.down),
            44 => eq(s.dead),
            // DC_NEARLY_KO / DC_IS_NEARLY_KO (Character +0x508 under 0.1). (assumption) A vital
            // part (head, chest) under a fifth of its health.
            30 | 42 => eq(!s.down && self.arena.f[subject].medical.parts.iter().any(|p| (p.kind == 0 || p.kind == 3) && p.health() < p.max_health() * 0.2)),
            43 => eq(self.squad_down(subject)),
            // DC_NUM_BACKPACKS (F039211): its squad's people wearing one
            37 => cmp3(self.squad_of(subject).iter().filter(|&&k| self.folk[k].inventory.equipped().any(|i| i.slot == 12)).count() as i64),
            // DC_SQUAD_ONLY_ANIMALS (the squad's +0x70)
            38 => eq(self.squad_of(subject).iter().all(|&k| self.folk[k].race.is_some())),
            // DC_COPS_AROUND (F046505 over the speaker's squad's sightings, kinds 0x80 / 0x4219).
            // (assumption) A town's law or military, not of the speaker's squad, within 600 units
            33 => {
                let q = at(speaker);
                let own = self.squad_of_any(speaker);
                eq((0..self.folk.len()).any(|g| {
                    g != speaker && !own.contains(&g) && !self.folk[g].down && !self.folk[g].dead && (self.guards_town(g) || self.folk[g].role == 2) && flat(at(g), q) < 600.0
                }))
            }
            // DC_SQUAD_SIZE: its squad's people
            35 => cmp3(self.squad_of(subject).len() as i64),
            // DC_IS_OUTNUMBERED: its squad's foes (+0x94) over 1.3 x its standing people (+0x4c).
            // (assumption) The foes: the other side of the conversation's squad standing.
            39 => {
                let other = if subject == speaker { target } else { speaker };
                let up = |k: usize| self.squad_of(k).iter().filter(|&&m| !self.folk[m].down).count() as f32;
                eq(up(other) > up(subject) * 1.3)
            }
            // DC_STRONGER_THAN_ME: the subject's squad over 1.5 x the speaker's;
            // DC_WEAKER_THAN_ME: the speaker's over 1.5 x the subject's (+0x1a0 +0x90)
            22 => eq(self.squad_strength(subject) > self.squad_strength(speaker) * 1.5),
            21 => eq(self.squad_strength(speaker) > self.squad_strength(subject) * 1.5),
            // DC_IS_RUNNING (F040638): asked of the speaker itself, yes; else moving at over 0.85
            // of its top speed to somewhere over 50 units from the speaker and further from it
            // than it is (x 1.2 squared): running away
            32 => {
                if subject == speaker {
                    eq(true)
                } else {
                    let v = s.mover.velocity;
                    let speed = (v[0] * v[0] + v[2] * v[2]).sqrt();
                    let me = at(speaker);
                    let d2 = |p: [f32; 3]| (p[0] - me[0]).powi(2) + (p[1] - me[1]).powi(2) + (p[2] - me[2]).powi(2);
                    let away = s.pathing.last_goal.is_some_and(|g| d2(g) > 2500.0 && d2(at(subject)) * 1.2 < d2(g));
                    eq(speed > s.mover.max_speed * 0.85 && away)
                }
            }
            // DC_CARRYING_BOUNTY_ALIVE / _DEAD: carrying (+0x388) someone alive / dead with a
            // bounty with the speaker's faction, or a diplomat (OT_DIPLOMAT) of a faction the
            // speaker's is at war with
            4 | 5 => {
                let Some(c) = s.carrying else { return eq(false) };
                if self.folk[c].dead != (cond == 5) {
                    return eq(false);
                }
                let wanted = sp.faction.as_deref().is_some_and(|f| self.bounty_with(c, f) > 0);
                let class = self.folk[c].character.as_deref().and_then(|r| self.data.get(r)).and_then(|r| r.ints.get("NPC class").copied()).unwrap_or(0);
                let enemy = sp_faction.as_deref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(self.folk[c].faction.as_deref()));
                eq(wanted || (class == 5 && enemy))
            }
            // DC_CARRYING_SOMEONE_TO_ENSLAVE: carrying someone alive (a value of 0 with someone
            // carried never holds) of class adventurer, bandit, slave or civilian. (not ported)
            // The slave price the speaker would pay (F038653: none, no) is not asked.
            46 => {
                let Some(c) = s.carrying else { return eq(false) };
                if !on || self.folk[c].dead {
                    return false;
                }
                let class = self.folk[c].character.as_deref().and_then(|r| self.data.get(r)).and_then(|r| r.ints.get("NPC class").copied()).unwrap_or(9);
                matches!(class, 4 | 6 | 8 | 9)
            }
            // DC_IN_MY_BUILDING: in the speaker's squad's home (else the building the speaker is
            // in)
            17 => {
                let home = sp.home.or(sp.inside);
                eq(home.is_some() && s.inside == home)
            }
            45 => eq(self.is_female(subject)),
            // DC_HAS_ROBOT_LIMBS: a robot limb on (F040400); (not ported) a robot race (race +0x7c)
            79 => eq(s.limbs.iter().any(|l| l.is_some())),
            // DC_IS_RECRUITABLE: the player's faction not full ("max faction size"), its fighting
            // level (F047045) under 25, not law, military, trader nor diplomat
            77 => {
                let class = s.character.as_deref().and_then(|c| self.data.get(c)).and_then(|c| c.ints.get("NPC class").copied()).unwrap_or(0);
                eq(self.player_count() < self.max_faction_size() && self.recruit_level(subject) < 25.0 && !matches!(class, 1 | 2 | 3 | 5))
            }
            // DC_MIXED_GENDER_GROUP: its squad has both
            72 => {
                let sq = self.squad_of(subject);
                let f = sq.iter().filter(|&&k| self.is_female(k)).count();
                eq(f > 0 && f < sq.len())
            }
            // DC_HAS_ILLEGAL_ITEM: carrying an item the speaker's faction's "trade culture" lists
            // in its "illegal goods"
            69 => {
                let culture = sp.faction.as_deref().and_then(|f| self.data.get(f)).and_then(|f| f.refs("trade culture").first().map(|(c, _)| c.clone())).and_then(|c| self.data.get(&c));
                let illegal: Vec<&String> = culture.map(|c| c.refs("illegal goods").iter().map(|(x, _)| x).collect()).unwrap_or_default();
                eq(s.inventory.sections.iter().flat_map(|x| x.items.iter()).any(|p| illegal.contains(&&p.item.record)))
            }
            // DC_HAS_A_BASE_NEARBY (F048796 / F048792): a player town within 23040 units of the
            // speaker (530841600 squared): here the player's buildings
            60 => {
                let q = at(speaker);
                eq(self.buildings.iter().any(|bd| bd.confirmed && flat(bd.at, q) < 23040.0))
            }
            // DC_CAN_AFFORD_BOUNTY: one of the player's with the money for its bounty with the
            // speaker's faction; anyone else fails whatever the value
            66 => s.player && eq(self.money >= sp.faction.as_deref().map_or(0, |f| self.bounty_with(subject, f))),
            // DC_TOWN_HAS_FORTIFICATIONS_WALLS (the subject's town's vtable 0x308, F048826: its
            // gate building nearest the speaker, within FLT_MAX: any): its town has a gate.
            // (assumption) The town's gates: those within its radius.
            49 => eq(self.town_of(subject).is_some_and(|t| (0..self.door_states.len()).any(|d| self.is_gate(d) && self.towns[t].contains(self.door_states[d].door.position, 1.0)))),
            // DC_TARGET_IS_MY_MISSION_TARGET (F0106d1 -> F044489: the squad's blackboard mission
            // target, Character +0x658). The only mission target kept here is a contract's
            // (F028601 sets it to whoever hired the squad).
            50 => eq(self.contracts.iter().any(|c| c.members.contains(&speaker) && c.hirer == subject)),
            // F040815, the speaker's faction's own: DC_RELATIONS (toward the subject's faction),
            // DC_PLAYERMONEY, DC_REPUTATION (the subject's faction's, +0x18), as less than or at
            // least; anything else holds
            1 => lt_ge(self.relation_of(sp_faction.as_deref(), self.faction_key(subject).as_deref()) as i64),
            2 => lt_ge(self.money as i64),
            3 => lt_ge(self.faction_key(subject).and_then(|f| self.dialog_memory.reputation.get(&f).copied()).unwrap_or(0.0) as i64),
            _ => true,
        }
    }

    /// @fn F040888
    ///
    /// A record's (DIALOGUE, DIALOGUE_LINE, WORD_SWAPS) "conditions" for `npc` talking to
    /// `player`: F040900 gathers the DC_HAS_TAG and DC_PERSONALITY_TAG ones into must / must-not
    /// masks by the record's "tag" (a value of 1 must, any other must not); F040868 then asks
    /// the speaker's personality to be one of the must ones and none of the must-nots, and its
    /// squad's memory of the one it talks to to have every must tag and no must-not (no memory:
    /// no must tags); the rest each F040880, of the speaker ("who" 0) or the one it talks to.
    pub(crate) fn conditions_pass(&self, rec: &GameData, npc: usize, player: usize) -> bool {
        let (mut tag_must, mut tag_not, mut pers_must, mut pers_not) = (0u32, 0u32, 0u32, 0u32);
        let mut rest: Vec<(i32, i32, i32, i32)> = Vec::new();
        for (sid, v) in rec.refs("conditions") {
            let Some(c) = self.data.get(sid) else { continue };
            let name = c.ints.get("condition name").copied().unwrap_or(0);
            let bit = 1u32 << (c.ints.get("tag").copied().unwrap_or(0) & 31);
            let compare = c.ints.get("compare by").copied().or_else(|| match c.strings.get("compare by").map(String::as_str) {
                Some("<") => Some(1),
                Some(">") => Some(2),
                _ => None,
            });
            match name {
                23 if v[0] == 1 => tag_must |= bit,
                23 => tag_not |= bit,
                26 if v[0] == 1 => pers_must |= bit,
                26 => pers_not |= bit,
                _ => rest.push((name, compare.unwrap_or(0), c.ints.get("who").copied().unwrap_or(0), v[0])),
            }
        }
        let pers = 1u32 << (self.folk[npc].personality & 31);
        if (pers_must != 0 && pers_must & pers == 0) || pers_not & pers != 0 {
            return false;
        }
        if npc != player {
            let (tags, known) = self.squad_tags(npc, player);
            if !known && tag_must != 0 {
                return false;
            }
            if tags & tag_must != tag_must || tags & tag_not != 0 {
                return false;
            }
        }
        rest.iter().all(|&(name, compare, who, value)| {
            let subject = if who == 0 { npc } else { player };
            self.condition(name, compare, value, subject, npc, player)
        })
    }

    /// The DIALOGUE_PACKAGE person `id` talks with.
    pub fn dialogue_package(&self, id: usize) -> Option<&GameData> {
        let p = &self.folk[id];
        p.dialogue.as_deref().and_then(|d| self.data.get(d))
    }

    /// @fn F040827
    ///
    /// Line `l`'s score said to `target`: its own, +2 when the target's race is one of its
    /// "target race".
    fn score_for(&self, l: &GameData, target: usize) -> i32 {
        let race = self.race_of(target);
        line_score(&self.data, l) + if l.refs("target race").iter().any(|(r, _)| race.as_deref() == Some(r.as_str())) { 2 } else { 0 }
    }

    /// @fn F040892
    ///
    /// The line among `refs` (DIALOGUE_LINE ids) the speaker says: the lines best-scoring first
    /// (`score_for` the player's character), the passing ones of the best score, one of them at
    /// random. (assumption) The lines' order among equal scores as listed (the game's sort is
    /// not decompiled); its pass-through lines (a virtual call, not identified) not told apart.
    pub(crate) fn choose_line(&mut self, refs: &[(String, [i32; 3])], npc: usize, player: usize) -> Option<GameData> {
        let mut lines: Vec<(i32, GameData)> = refs.iter().filter_map(|(sid, _)| self.data.get(sid)).map(|l| (self.score_for(l, player), l.clone())).collect();
        lines.sort_by(|a, b| b.0.cmp(&a.0));
        let mut best = -999;
        let mut picks: Vec<GameData> = Vec::new();
        for (s, l) in lines {
            if s < best {
                break;
            }
            if self.line_open(&l, npc, player) {
                picks.push(l);
                best = s;
            }
        }
        match picks.len() {
            0 => None,
            1 => picks.pop(),
            n => {
                let k = ((self.rng.uniform() * n as f32) as usize).min(n - 1);
                Some(picks.swap_remove(k))
            }
        }
    }

    /// @fn F040903
    ///
    /// Dialogue `d`'s score with its opening line `line`: its own, the line's and the best of
    /// the line's replies' (all of them, passing or not).
    fn dialogue_score(&self, d: &GameData, line: &GameData, player: usize) -> i32 {
        let replies = line.refs("lines").iter().filter_map(|(sid, _)| self.data.get(sid)).map(|r| self.score_for(r, player)).max().unwrap_or(0).max(0);
        line_score(&self.data, d) + self.score_for(line, player) + replies
    }

    /// "target is type" (CharacterTypeEnum, F040888): the target's CHARACTER's "NPC class"
    /// must be it; for OT_SLAVE (6) a slave or one held as one passes too.
    fn type_ok(&self, d: &GameData, target: usize) -> bool {
        let want = d.ints.get("target is type").copied().unwrap_or(0);
        if want <= 0 {
            return true;
        }
        let t = &self.folk[target];
        if want == 6 && (t.slave.is_some() || t.prison.as_ref().is_some_and(|p| p.slave)) {
            return true;
        }
        // (assumption) one of the player's with no CHARACTER record is an adventurer (OT_ADVENTURER
        // 9), as the game starts' characters are
        let class = t.character.as_deref().and_then(|c| self.data.get(c)).and_then(|c| c.ints.get("NPC class").copied());
        class.or(t.player.then_some(9)) == Some(want)
    }

    /// Whether `npc` has said line or dialogue `id` within its repetition limit (a "once only"
    /// one: ever).
    fn said_lately(&self, l: &GameData, npc: usize) -> bool {
        let once = l.bools.get("once only").copied().unwrap_or(false);
        let wait = if once { f64::INFINITY } else { repetition_hours(l.ints.get("repetition limit").copied().unwrap_or(0)) };
        wait > 0.0 && self.dialog_memory.spoken.get(&(npc, l.string_id.clone())).is_some_and(|&last| self.hours_gone() < last + wait)
    }

    /// Whether line `l` may be said in the conversation of `npc` and `player`: its repetition
    /// limit and "target is type", its conditions, and its filters for its speaker
    /// (`speaker_of`) and the one it is said to.
    fn line_open(&self, l: &GameData, npc: usize, player: usize) -> bool {
        if self.said_lately(l, npc) || !self.type_ok(l, player) {
            return false;
        }
        // an interjector's line: its conditions and filters are its own (`interjectors`)
        if (3..=5).contains(&l.ints.get("speaker").copied().unwrap_or(T_ME)) {
            return self.speaker_of(l, npc, player).is_some();
        }
        if !self.conditions_pass(l, npc, player) {
            return false;
        }
        match self.speaker_of(l, npc, player) {
            Some(s) => self.filters_pass(l, s, if s == player { npc } else { player }),
            None => false,
        }
    }

    /// Who says line `l` (TalkerEnum): the NPC (T_ME), the player's character (T_TARGET,
    /// T_TARGET_IF_PLAYER, T_TARGET_WITH_RACE), an interjector (T_INTERJECTOR1..3: of
    /// `interjectors`, the likeliest), the NPC for its whole squad (T_WHOLE_SQUAD). None: no
    /// one can.
    fn speaker_of(&self, l: &GameData, npc: usize, player: usize) -> Option<usize> {
        match l.ints.get("speaker").copied().unwrap_or(T_ME) {
            1 | 2 | 7 => Some(player),
            3..=5 => self.interjectors(l, npc, player).into_iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0),
            _ => Some(npc),
        }
    }

    /// @fn F040891
    ///
    /// Those of `npc`'s squad who could interject line `l` in its talk with `target`, each with
    /// its weight: standing, not `npc` nor the one talked to, no animal, not saying a line of
    /// its own, not "dumb" (CHARACTER, F039418) unless the line names it ("is character"), the
    /// line's conditions and filters passing for it as the speaker (F040888); weighted 1 within
    /// 30 units of `npc` falling to 0 at 450 (squared 900 .. 202800, F050517), a tenth on
    /// another floor (not in the same building).
    fn interjectors(&self, l: &GameData, npc: usize, target: usize) -> Vec<(usize, f32)> {
        let at = self.folk[npc].mover.position_units();
        let named: Vec<&String> = l.refs("is character").iter().map(|(c, _)| c).collect();
        self.squad_of(npc)
            .into_iter()
            .filter(|&k| k != npc && k != target && !self.folk[k].down && !self.folk[k].dead && self.folk[k].race.is_none())
            .filter(|&k| !self.saying(k))
            .filter(|&k| {
                let dumb = self.folk[k].character.as_deref().and_then(|c| self.data.get(c)).is_some_and(|c| c.bools.get("dumb").copied().unwrap_or(false));
                !dumb || self.folk[k].character.as_ref().is_some_and(|c| named.contains(&c))
            })
            .filter(|&k| self.conditions_pass(l, k, target) && self.filters_pass(l, k, target))
            .filter_map(|k| {
                let q = self.folk[k].mover.position_units();
                let d2 = (q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2);
                let mut w = ((d2 - 202_800.0) / (900.0 - 202_800.0)).min(1.0).max(0.0);
                if self.folk[k].inside != self.folk[npc].inside {
                    w *= 0.1;
                }
                (w > 0.0).then_some((k, w))
            })
            .collect()
    }

    /// One of `interjectors` at random by weight (F041399 / F041256).
    pub(crate) fn pick_interjector(&mut self, l: &GameData, npc: usize, target: usize) -> Option<usize> {
        let list = self.interjectors(l, npc, target);
        let total: f32 = list.iter().map(|x| x.1).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = self.rng.uniform() * total;
        for (k, w) in &list {
            r -= w;
            if r < 0.0 {
                return Some(*k);
            }
        }
        list.last().map(|x| x.0)
    }

    /// The town person `id` is in (Character vtable 0x70 +0x88, F038707): as the world last
    /// found it, else by where it stands.
    pub(crate) fn town_of(&self, id: usize) -> Option<usize> {
        self.folk[id].town.or_else(|| kenshi_world::town::current_town(&self.towns, self.folk[id].mover.position_units(), None))
    }

    /// Whether person `k` has item `item` (F043523), or with `item` None one of item type
    /// `kind` (F043528): worn, carried or in its backpack.
    fn has_item(&self, k: usize, item: Option<&str>, kind: Option<kenshi_data::ItemType>) -> bool {
        let inv = &self.folk[k].inventory;
        inv.sections.iter().chain(inv.bag().into_iter().flat_map(|b| b.sections.iter())).flat_map(|s| s.items.iter()).any(|p| match item {
            Some(i) => p.item.record == i,
            None => p.item.kind.is_some() && p.item.kind == kind,
        })
    }

    /// Whether person `k` has an item of "item function" `function` (F04b84e): worn, carried
    /// or in its backpack.
    fn has_function(&self, k: usize, function: i32) -> bool {
        let inv = &self.folk[k].inventory;
        inv.sections
            .iter()
            .chain(inv.bag().into_iter().flat_map(|b| b.sections.iter()))
            .flat_map(|s| s.items.iter())
            .any(|p| self.data.get(&p.item.record).and_then(|r| r.ints.get("item function").copied()) == Some(function))
    }

    /// A dialogue's or line's filters for speaker `me` and the one it speaks to: `filters_target`
    /// passes.
    fn filters_pass(&self, d: &GameData, me: usize, target: usize) -> bool {
        self.filters_target(d, me, target).is_some()
    }

    /// @fn F040888
    ///
    /// A dialogue's or line's filters (fcs.def) for speaker `me` talking to `target`, in the
    /// game's order: "in town of" (the town it stands in is of one of them, F044518), "my
    /// faction", "target faction", "has package" (one of its dialogue packages), "is character";
    /// of the one it talks to's squad (that one, or another of it the speaker sees, F046525):
    /// "target carrying character"; "world state" (each ref's value 1: must hold, 0: must not,
    /// F041242); of the squad again, "target has item" / "target has item type" (worn, carried
    /// or in its backpack) and "target race" (of the race's group, F033685); "my race" (of the
    /// group) and "my subrace" (the race itself). Of the squad filters, the member that passes
    /// becomes the one talked to (F040824): the one returned. None: they fail.
    /// (assumption) A race filter names the person's race record, the look's for a person.
    fn filters_target(&self, d: &GameData, me: usize, target: usize) -> Option<usize> {
        let any = |k: &str, f: &dyn Fn(&str) -> bool| {
            let r = d.refs(k);
            r.is_empty() || r.iter().any(|(s, _)| f(s))
        };
        let m = &self.folk[me];
        let in_town_of = |f: &str| self.town_of(me).is_some_and(|t| self.towns[t].faction.as_deref() == Some(f));
        let packages = self.packages_of(me);
        if !(any("in town of", &in_town_of)
            && any("my faction", &|f| m.faction.as_deref() == Some(f))
            && any("target faction", &|f| self.faction_key(target).as_deref() == Some(f))
            && any("has package", &|p| packages.iter().any(|x| x.string_id == p))
            && any("is character", &|c| m.character.as_deref() == Some(c)))
        {
            return None;
        }
        // the one talked to's squad: itself first, then the rest the speaker can see
        let squad = |t: usize| -> Vec<usize> {
            let mut v = vec![t];
            if t != me {
                v.extend(self.squad_of(t).into_iter().filter(|&k| k != t && k != me && !self.folk[k].away && self.can_see(me, k)));
            }
            v
        };
        let mut target = target;
        let pick = |t: usize, cat: &str, test: &dyn Fn(usize, &str) -> bool| -> Option<usize> {
            let refs = d.refs(cat);
            if refs.is_empty() {
                return Some(t);
            }
            refs.iter().find_map(|(r, _)| squad(t).into_iter().find(|&k| test(k, r)))
        };
        target = pick(target, "target carrying character", &|k, c| self.folk[k].carrying.is_some_and(|x| self.folk[x].character.as_deref() == Some(c)))?;
        if !d.refs("world state").iter().all(|(s, v)| self.world_state(s) == (v[0] != 0)) {
            return None;
        }
        target = pick(target, "target has item", &|k, i| self.has_item(k, Some(i), None))?;
        // "target has item type" (+0x1b0): the first ref's "item function" (F040900); none (0,
        // the books, the trade goods) is no test at all
        let function = d.refs("target has item type").first().and_then(|(i, _)| self.data.get(i)).and_then(|r| r.ints.get("item function").copied()).unwrap_or(0);
        if function != 0 {
            target = pick(target, "target has item type", &|k, _| self.has_function(k, function))?;
        }
        target = pick(target, "target race", &|k, r| self.race_of(k).is_some_and(|x| self.same_race_group(&x, r)))?;
        let my_race = self.race_of(me);
        if !(any("my race", &|r| my_race.as_deref().is_some_and(|x| self.same_race_group(x, r))) && any("my subrace", &|r| my_race.as_deref() == Some(r))) {
            return None;
        }
        Some(target)
    }

    /// The DIALOGUE_PACKAGEs person `id` talks with (the dialogue's +0xa0 list): its own (its
    /// squad template's, else its character's), the rest of its CHARACTER's "dialogue package"
    /// list and its faction's "dialog default", each followed by what it inherits
    /// ("inheritsFrom", in turn: F040909 adds them as it loads each). One whose name has
    /// "Unfinished" is not loaded, nor what it inherits.
    pub fn packages_of(&self, id: usize) -> Vec<GameData> {
        let mut own: Vec<GameData> = self.dialogue_package(id).cloned().into_iter().collect();
        let p = &self.folk[id];
        if !p.player {
            let lists = [
                p.character.as_deref().and_then(|c| self.data.get(c)).map(|c| c.refs("dialogue package").to_vec()),
                p.faction.as_deref().and_then(|f| self.data.get(f)).map(|f| f.refs("dialog default").to_vec()),
            ];
            for (sid, _) in lists.into_iter().flatten().flatten() {
                if let Some(r) = self.data.get(&sid)
                    && !own.iter().any(|o| o.string_id == r.string_id)
                {
                    own.push(r.clone());
                }
            }
        }
        fn load(data: &kenshi_data::GameDataContainer, p: &GameData, out: &mut Vec<GameData>, depth: u32) {
            if unfinished(p) || out.iter().any(|o| o.string_id == p.string_id) {
                return;
            }
            out.push(p.clone());
            if depth > 16 {
                return;
            }
            for (sid, _) in p.refs("inheritsFrom") {
                if let Some(r) = data.get(sid) {
                    load(data, r, out, depth + 1);
                }
            }
        }
        let mut out = Vec::new();
        for package in &own {
            load(&self.data, package, &mut out, 0);
        }
        out
    }

    /// @fn F040051
    ///
    /// A character's bounty as it is made (not one loaded): its CHARACTER's "bounty chance"
    /// (0..100) against randInt(0, 100); then "bounty amount" + randInt(0, "bounty amount fuzz"
    /// x 2) x 0.5 with each of its "bounty factions", none listed the record
    /// "defaultEmpireFactionSID" (the United Cities) (F046510). The uniques with a price on
    /// their heads (Bugmaster, Boss Simion, Bo...) and some bandits.
    pub fn roll_bounty(&mut self, id: usize) {
        let Some(c) = self.folk[id].character.as_deref().and_then(|c| self.data.get(c)).cloned() else { return };
        let int = |k: &str| c.ints.get(k).copied().unwrap_or(0) as i32;
        let chance = int("bounty chance");
        if chance <= 0 || (self.rng.uniform() * 101.0).floor() as i32 >= chance {
            return;
        }
        let fuzz = (int("bounty amount fuzz") as f32 * 2.0) as i32;
        let roll = ((self.rng.uniform() * (fuzz.max(0) + 1) as f32).floor() as i32).min(fuzz.max(0));
        let amount = (roll as f32 * 0.5 + int("bounty amount") as f32) as i32;
        let mut factions: Vec<String> = c.refs("bounty factions").iter().filter(|(f, _)| self.data.get(f).is_some()).map(|(f, _)| f.clone()).collect();
        if c.refs("bounty factions").is_empty() && self.data.get("defaultEmpireFactionSID").is_some() {
            factions.push("defaultEmpireFactionSID".to_string());
        }
        // (F046510's adding, quietly: no one is told of a bounty a character is made with)
        let now = self.now_hours();
        for f in factions {
            let p = &mut self.folk[id];
            match p.bounties.iter_mut().find(|b| b.faction == f) {
                Some(b) => {
                    b.amount += amount;
                    b.stamp = now;
                }
                None => p.bounties.push(crate::law::Bounty { faction: f, amount, crimes: 0, stamp: now }),
            }
        }
    }

    /// `who` remembers `of` by long-term tag `tag` (LT_*). Freed (LT_FREED_ME: a cage's lock
    /// picked, F031344) it raises EV_UNLOCK_MY_CAGE_OR_SHACKLES at its freer, forced; treated
    /// (LT_MY_LIFESAVER, a medic beside it) the first aid's events begin (`first_aid_events`).
    pub fn remember(&mut self, who: usize, of: usize, tag: u32) {
        if who == of {
            return;
        }
        *self.dialog_memory.tags.entry((who, of)).or_default() |= 1 << tag;
        let now = self.hours_gone();
        self.dialog_memory.met.insert((who, of), now);
        match tag {
            LT_FREED_ME => {
                self.bark_forced(who, of, crate::speech::EV_UNLOCK_MY_CAGE_OR_SHACKLES);
            }
            LT_MY_LIFESAVER if self.dialog_memory.healing.get(&of) != Some(&who) => {
                // Task_FirstAid::start (F031287): the medic EV_HEALING_OTHER_START at its
                // patient, the patient (not caged) EV_BEING_HEALED_START back
                self.dialog_memory.healing.insert(of, who);
                self.bark(of, who, crate::speech::EV_HEALING_OTHER_START);
                if self.folk[who].prison.is_none() {
                    self.bark(who, of, crate::speech::EV_BEING_HEALED_START);
                }
            }
            _ => {}
        }
    }

    /// Every second: first aid's end (Task_FirstAid::update, F031333): its medic done (not
    /// down, the patient alive) says EV_HEALING_OTHER_FINISHED at its patient, the patient (not
    /// caged) EV_BEING_HEALED_FINISHED back; a medic with no kit left (Task_FirstAidRig,
    /// F031288) EV_FIRSTAID_KIT_EMPTY, once.
    fn first_aid_events(&mut self) {
        // (in a fixed order: the map's own is the hasher's, and the barks roll dice)
        let mut pairs: Vec<(usize, usize)> = self.dialog_memory.healing.iter().map(|(&m, &p)| (m, p)).collect();
        pairs.sort_unstable();
        for (medic, patient) in pairs {
            if medic >= self.folk.len() || patient >= self.folk.len() {
                self.dialog_memory.healing.remove(&medic);
                continue;
            }
            if self.folk[medic].first_aid == Some(patient) {
                let kit = self.folk[medic].inventory.sections.iter().flat_map(|s| s.items.iter()).any(|i| i.item.medkit.is_some_and(|k| 0.0 < k.charges));
                if !kit && self.dialog_memory.kit_empty.insert(medic) {
                    self.bark(medic, patient, crate::speech::EV_FIRSTAID_KIT_EMPTY);
                }
                continue;
            }
            self.dialog_memory.healing.remove(&medic);
            self.dialog_memory.kit_empty.remove(&medic);
            if self.folk[medic].down || self.folk[medic].dead || self.folk[patient].dead {
                continue;
            }
            self.bark(medic, patient, crate::speech::EV_HEALING_OTHER_FINISHED);
            if self.folk[patient].prison.is_none() {
                self.bark(patient, medic, crate::speech::EV_BEING_HEALED_FINISHED);
            }
        }
    }

    /// Every second: deaths and squads beaten remembered (LT_KILLED_MY_FRIEND by the dead's
    /// squad of those fighting it; LT_DEFEATED_MY_SQUAD_ONCE / LT_SQUAD_LOST_TO_ME_ONCE between a
    /// squad all down and those fighting it, F033862, and EV_I_DEFEATED_SQUAD). (assumption) Who
    /// did it: whoever had the fallen as its combat target, else the nearest hostile fighter.
    pub fn memory_tick(&mut self) {
        if !self.tick.is_multiple_of(60) {
            return;
        }
        self.contract_tick();
        self.short_tag_tick();
        self.first_aid_events();
        self.unique_captivity_tick();
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
            let mut attackers: Vec<usize> = (0..n).filter(|&k| k != d && self.arena.f[k].focus == Some(d) && !self.folk[k].dead).collect();
            if attackers.is_empty() {
                // (assumption) its focus moved on as it fell: the nearest fighter standing
                // within 300 units whose foes are its side
                let (team, at) = (self.arena.f[d].team, self.folk[d].mover.position_units());
                let near = (0..n)
                    .filter(|&k| k != d && !self.folk[k].dead && !self.folk[k].down && self.arena.f[k].active && self.arena.f[k].foes.as_ref().is_some_and(|f| f.contains(&team)))
                    .map(|k| {
                        let q = self.folk[k].mover.position_units();
                        (k, (q[0] - at[0]).hypot(q[2] - at[2]))
                    })
                    .filter(|x| x.1 < 300.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                attackers.extend(near.map(|x| x.0));
            }
            // a unique's death the player had a hand in (the world states' "player involvement",
            // F050474): (assumption) one of the player's fighting it, or it in the player's
            // custody (caged or carried)
            if now.1 && !was.1
                && let Some(c) = self.folk[d].character.clone()
                && self.data.get(&c).is_some_and(|r| r.bools.get("unique").copied().unwrap_or(false))
            {
                let custody = self.folk[d].prison.as_ref().is_some_and(|p| p.faction == PLAYER_FACTION || self.folk.iter().any(|q| q.player && q.faction.as_deref() == Some(p.faction.as_str())))
                    || self.folk[d].carried_by.is_some_and(|k| self.folk[k].player);
                if custody || attackers.iter().any(|&a| self.folk[a].player) {
                    self.world.involved.insert(c);
                }
            }
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
                // F033862: the one whose hit downed the last of the squad (F044493 with 0: all
                // down) says EV_I_DEFEATED_SQUAD over it ("tinfist defeated squad", the bandits'
                // gloating)
                self.bark(attackers[0], d, EV_I_DEFEATED_SQUAD);
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
        self.set_package_for(id, pkg, None);
    }

    /// `set_package` with the squad's mission target (a contract's hirer).
    pub fn set_package_for(&mut self, id: usize, pkg: &str, mission: Option<usize>) {
        let data = self.data.clone();
        let Some(p) = data.get(pkg) else { return };
        let role = if self.folk[id].role == 5 { kenshi_ai::package::Role::Leader } else { kenshi_ai::package::Role::Squad };
        let leader = self.folk[id].platoon.and_then(|pl| self.folk.iter().find(|q| q.platoon == Some(pl) && q.role == 5).map(|q| q.id));
        let goals = kenshi_ai::package::package_goals(&data, p, role);
        let jobs = crate::sim::jobs_of(goals, leader, mission);
        let person = &mut self.folk[id];
        person.ai.jobs = jobs;
        person.ai.queue.clear();
        person.package = Some(pkg.to_string());
        person.rethink = true;
    }

    /// The people of `id`'s squad (its platoon; for one of the player's, the player's in its
    /// squad, the animals with them).
    pub fn squad_of(&self, id: usize) -> Vec<usize> {
        let p = &self.folk[id];
        if p.player {
            return self.folk.iter().filter(|q| q.player && !q.dead && (q.platoon == p.platoon || q.race.is_some() || p.race.is_some())).map(|q| q.id).collect();
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
    pub(crate) fn faction_relation(&self, me: usize, target: usize) -> f32 {
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
        // (drawn once: the roll walks the template's and faction's race lists)
        let key = (id, p.character.clone(), p.faction.clone());
        if let Some(r) = self.race_rolls.lock().unwrap().get(&key) {
            return r.clone();
        }
        let r = crate::looks_roll::race_and_sex(&self.data, &self.install, p, "Greenlander").map(|(r, _, _)| r.string_id.clone());
        self.race_rolls.lock().unwrap().insert(key, r.clone());
        r
    }

    /// Whether races `a` and `b` are one kind (F033685: the same race, or both in one
    /// RACE_GROUP's "races": Greenlanders and Scorchlanders are Human).
    pub fn same_race_group(&self, a: &str, b: &str) -> bool {
        a == b
            || self.data.of_type(kenshi_data::ItemType::RaceGroup).any(|g| {
                let races = g.refs("races");
                races.iter().any(|(r, _)| r == a) && races.iter().any(|(r, _)| r == b)
            })
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

    /// @fn F040893
    ///
    /// One word swap ("/NAME/"): MYNAME the speaker's name, YOURNAME the one spoken to's ("you"
    /// without one), TARGETFACTION their faction's, PLAYERFACTION the player's faction's; any
    /// other the WORD_SWAPS record of that name, its line chosen as a conversation's are
    /// (F040892: the best-scoring passing ones, one at random), nothing when none passes or
    /// there is no such record; COST the line's price (`line_cost`) as "{1,num}" formats it.
    fn word(&mut self, token: &str, npc: usize, player: usize, cost: i32) -> String {
        let faction_name = |s: &Self, k: usize| {
            if s.folk[k].faction.as_deref() == Some(PLAYER_FACTION) {
                return s.player_faction_name();
            }
            s.folk[k].faction.as_deref().and_then(|f| s.data.get(f)).map_or(String::new(), |f| f.name.clone())
        };
        match token {
            "MYNAME" => self.display_name(npc),
            "YOURNAME" if player == npc => "you".to_string(),
            "YOURNAME" => self.display_name(player),
            "TARGETFACTION" if player == npc => "you".to_string(),
            "TARGETFACTION" => faction_name(self, player),
            "PLAYERFACTION" => self.player_faction_name(),
            "COST" => num_text(cost),
            _ => {
                // (in load order: the records' map order is the hasher's, new each run)
                let rec = self.data.order.iter().filter_map(|s| self.data.get(s)).find(|r| r.item_type() == Some(kenshi_data::ItemType::WordSwaps) && r.name == token).cloned();
                match rec {
                    Some(w) => {
                        let refs = w.refs("lines").to_vec();
                        let pick = self.choose_line(&refs, npc, player);
                        let r = self.rng.uniform();
                        pick.map(|l| text(&l, r)).unwrap_or_default()
                    }
                    None => String::new(),
                }
            }
        }
    }

    /// @fn F040826
    ///
    /// A line's price, for its COST word: the value of its first DA_GIVE_MONEY or
    /// DA_TAKE_MONEY effect, else of its first DC_PLAYERMONEY condition, else 0.
    pub(crate) fn line_cost(&self, l: &GameData) -> i32 {
        let first = |cat: &str, key: &str, want: &[i32]| {
            l.refs(cat).iter().find(|(sid, _)| self.data.get(sid).and_then(|r| r.ints.get(key).copied()).is_some_and(|k| want.contains(&k))).map(|(_, v)| v[0])
        };
        first("effects", "action name", &[8, 9]).or_else(|| first("conditions", "condition name", &[2])).unwrap_or(0)
    }

    /// @fn F040906
    ///
    /// A line's text with its word swaps filled: the first "/.../" pair's word (`word`) put in
    /// its place, capitalised at the start of the text or after a sentence's end (".", "!" or
    /// "?" two characters before it, three with a space there), then the text again from the
    /// start, so a swap's own swaps are filled too. (A guard of 64 swaps a line.)
    pub(crate) fn words(&mut self, said: &str, npc: usize, player: usize, cost: i32) -> String {
        let mut s = said.to_string();
        for _ in 0..64 {
            let Some(a) = s.find('/') else { break };
            let Some(b) = s[a + 1..].find('/').map(|b| a + 1 + b) else { break };
            let token = s[a + 1..b].to_string();
            let mut word = self.word(&token, npc, player, cost);
            let bytes = s.as_bytes();
            let capital = a == 0 || {
                let back = if a >= 2 && bytes[a - 2] == b' ' { 3 } else { 2 };
                a >= back && matches!(bytes[a - back], b'.' | b'!' | b'?')
            };
            if capital && let Some(c) = word.chars().next() {
                word = c.to_uppercase().collect::<String>() + &word[c.len_utf8()..];
            }
            s.replace_range(a..=b, &word);
        }
        s
    }

    /// The conversation at `line` (an NPC line): its text and the player's replies.
    fn at_line(&mut self, line: &GameData, npc: usize, player: usize) -> Conversation {
        let now = self.hours_gone();
        self.dialog_memory.spoken.insert((npc, line.string_id.clone()), now);
        let r = self.rng.uniform();
        let said = text(line, r);
        let mut npc_text = self.words(&said, npc, player, self.line_cost(line));
        let mut line = line.clone();
        let mut choices = Vec::new();
        // the NPC's own follow-on lines (speaker T_ME) run on in its speech; the rest are the
        // player's replies
        for _ in 0..6 {
            // the NPC's, its squad's and the interjectors' lines run on in its speech: the one
            // it says (`choose_line`); the player's are the choices
            let npc_side: Vec<(String, [i32; 3])> = line
                .refs("lines")
                .iter()
                .filter(|(sid, _)| self.data.get(sid).is_some_and(|r| matches!(r.ints.get("speaker").copied().unwrap_or(T_ME), 0 | 3..=6)))
                .cloned()
                .collect();
            let next_npc = self.choose_line(&npc_side, npc, player);
            for (sid, _) in line.refs("lines") {
                let Some(reply) = self.data.get(sid).cloned() else { continue };
                if matches!(reply.ints.get("speaker").copied().unwrap_or(T_ME), 0 | 3..=6) || !self.line_open(&reply, npc, player) {
                    continue;
                }
                let r = self.rng.uniform();
                let t = text(&reply, r);
                let t = self.words(&t, npc, player, self.line_cost(&reply));
                choices.push((sid.clone(), t));
            }
            match next_npc {
                Some(n) if choices.is_empty() => {
                    let r = self.rng.uniform();
                    let t = text(&n, r);
                    let t = self.words(&t, npc, player, self.line_cost(&n));
                    // an interjector's line under its name
                    let by = match n.ints.get("speaker").copied().unwrap_or(T_ME) {
                        3..=5 => self.pick_interjector(&n, npc, player),
                        _ => self.speaker_of(&n, npc, player),
                    }
                    .filter(|&s| s != npc);
                    if !t.is_empty() {
                        npc_text = match by {
                            Some(s) => format!("{npc_text}\n\n{}: {t}", self.display_name(s)),
                            None => format!("{npc_text} {t}"),
                        };
                    }
                    let fx = self.effects(&n, npc);
                    self.pending_effects.extend(fx);
                    let now = self.hours_gone();
                    self.dialog_memory.spoken.insert((npc, n.string_id.clone()), now);
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

    /// `npc` starts a dialogue of event `event` with `player`: the conversation and the opening
    /// line's effects. A package's "dialogs" entry lists its dialogue under up to three events,
    /// its three values (F040909: each not 0 nor 100 adds it, F040904); the entries carry no
    /// chance (a dialogue's own "chance temporary" is rolled, `dialogue_open`). (`_roll` is
    /// unused: kept for callers.)
    pub fn talk_on(&mut self, npc: usize, player: usize, event: i32, _roll: bool) -> Option<(Conversation, Vec<Effect>)> {
        self.talk_event(npc, player, event, &[])
    }

    /// @fn F040927
    ///
    /// `npc` makes an announcement (EV_ANNOUNCEMENT) to `player`: dialogue `sid` (a campaign's
    /// "dialog announcement") added to its package's announcement dialogues (not one named
    /// "Unfinished"), the best of them said.
    pub fn announce_to(&mut self, npc: usize, player: usize, sid: &str) -> Option<(Conversation, Vec<Effect>)> {
        self.talk_event(npc, player, EV_ANNOUNCEMENT, &[sid.to_string()])
    }

    /// `talk_on` with `extra` dialogues listed under the event too.
    fn talk_event(&mut self, npc: usize, player: usize, event: i32, extra: &[String]) -> Option<(Conversation, Vec<Effect>)> {
        // the NPC's last conversation counted; this one's event
        if let Some(ev) = self.dialog_memory.running_event.remove(&npc) {
            *self.dialog_memory.event_count.entry((npc, ev)).or_default() += 1;
        }
        self.dialog_memory.running_event.insert(npc, event);
        let Some((d, line, player)) = self.pick_opening(npc, player, event, extra) else {
            self.dialog_memory.running_event.remove(&npc);
            return None;
        };
        let now = self.hours_gone();
        self.dialog_memory.spoken.insert((npc, d.string_id.clone()), now);
        self.dialog_memory.met.insert((npc, player), now);
        self.dialog_memory.partner.insert(npc, player);
        self.dialog_memory.running.insert(npc, d.string_id.clone());
        self.dialog_memory.delivered.insert((npc, event));
        let mut effects = self.effects(&line, npc);
        let conv = self.at_line(&line, npc, player);
        effects.append(&mut self.pending_effects);
        Some((conv, effects))
    }

    /// @fn F040839
    /// @fn F040892
    ///
    /// The dialogue `npc` opens on `event` with `player` (its lines for the event, F040839, and
    /// of them the best, F040892) and its opening line, with whom of `player`'s squad it is
    /// said to; `extra` dialogues listed under the event too.
    pub(crate) fn pick_opening(&mut self, npc: usize, player: usize, event: i32, extra: &[String]) -> Option<(GameData, GameData, usize)> {
        // its dialogue packages (`packages_of`) and its squad's AI package
        let mut packages = self.packages_of(npc);
        if let Some(p) = self.folk[npc].package.as_deref().and_then(|p| self.data.get(p)) {
            packages.push(p.clone());
        }
        // its CHARACTER's own "dialogue" (talking to it)
        let mut personal: Vec<String> = if event == EV_PLAYER_TALK_TO_ME {
            self.folk[npc].character.as_deref().and_then(|c| self.data.get(c)).map(|c| c.refs("dialogue").iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default()
        } else {
            Vec::new()
        };
        personal.extend(extra.iter().cloned());
        if packages.is_empty() && personal.is_empty() {
            return None;
        }
        // every dialogue of the event whose conditions pass (its chance rolled), at the line it
        // would open with: the best `dialogue_score` is spoken, the first of equals (F045590)
        let mut best: Option<(i32, GameData, GameData, usize)> = None;
        // talked to while it has an interrupt: that dialogue
        let now = self.hours_gone();
        if event == EV_PLAYER_TALK_TO_ME
            && let Some((sid, until)) = self.dialog_memory.interrupt.remove(&npc)
            && now < until
            && let Some(d) = self.data.get(&sid).cloned()
            && let Some(line) = self.choose_line(d.refs("lines"), npc, player)
        {
            best = Some((0, d, line, player));
        }
        let candidates: Vec<(String, [i32; 3])> = if best.is_some() {
            Vec::new()
        } else {
            let mut v: Vec<(String, [i32; 3])> = Vec::new();
            let personal: Vec<(String, [i32; 3])> = personal.iter().map(|s| (s.clone(), [event, 0, 0])).collect();
            for (sid, e) in packages.iter().flat_map(|pkg| pkg.refs("dialogs").iter()).chain(personal.iter()) {
                if event != 0 && event != 100 && e.contains(&event) && !v.iter().any(|(x, _)| x == sid) {
                    v.push((sid.clone(), *e));
                }
            }
            v
        };
        {
            for (sid, _) in &candidates {
                let Some(d) = self.data.get(sid.as_str()).filter(|d| !unfinished(d)).cloned() else { continue };
                // its world states, character, town, factions, races and items: the one of the
                // player's squad it passes for is the one talked to
                let dbg = std::env::var("KENSHI_MEET_DEBUG").is_ok();
                let Some(to) = self.filters_target(&d, npc, player) else {
                    if dbg {
                        eprintln!("  {}: filters fail", d.name);
                    }
                    continue;
                };
                if !self.dialogue_open(&d, npc, to, event) {
                    if dbg {
                        eprintln!("  {}: not open", d.name);
                    }
                    continue;
                }
                if !self.conditions_pass(&d, npc, to) {
                    if dbg {
                        // each failing condition: its name, whose (t: the target), the comparison
                        // and the line's value
                        let failed: Vec<String> = d
                            .refs("conditions")
                            .iter()
                            .filter(|(sid, v)| !self.conditions_pass(&GameData { references: vec![("conditions".into(), vec![(sid.clone(), *v)])], ..Default::default() }, npc, to))
                            .filter_map(|(sid, v)| self.data.get(sid).map(|c| (c, v)))
                            .map(|(c, v)| {
                                let name = c.ints.get("condition name").copied().unwrap_or(0);
                                let who = c.ints.get("who").copied().unwrap_or(0);
                                format!("{}{} {} {}", name, if who != 0 { "(t)" } else { "" }, ["==", "<", ">"].get(c.ints.get("compare by").copied().unwrap_or(0) as usize).unwrap_or(&"?"), v[0])
                            })
                            .collect();
                        eprintln!("  {} conditions fail: {failed:?}", d.name);
                    }
                    continue;
                }
                let Some(line) = self.choose_line(d.refs("lines"), npc, to) else {
                    if dbg {
                        eprintln!("  {}: no line passes", d.name);
                    }
                    continue;
                };
                let score = self.dialogue_score(&d, &line, to);
                if dbg {
                    eprintln!("  {} scores {score}", d.name);
                }
                if best.as_ref().is_none_or(|b| score > b.0) {
                    best = Some((score, d, line, to));
                }
            }
        }
        let (_, d, line, player) = best?;
        if std::env::var("KENSHI_MEET_DEBUG").is_ok() {
            eprintln!("  chose dialogue {} ({})", d.name, d.string_id);
        }
        Some((d, line, player))
    }

    /// The player picks reply `choice` (a line id): its effects, then the NPC's answer (None:
    /// the conversation is over).
    pub fn choose(&mut self, conv: &Conversation, choice: &str) -> (Vec<Effect>, Option<Conversation>) {
        let Some(line) = self.data.get(choice).cloned() else { return (Vec::new(), None) };
        let mut effects = self.effects(&line, conv.npc);
        // the NPC's answer: the line it says to the reply (`choose_line`); a reply line can
        // itself carry on
        let now = self.hours_gone();
        self.dialog_memory.spoken.insert((conv.npc, line.string_id.clone()), now);
        let answer = self.choose_line(line.refs("lines"), conv.npc, conv.player);
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
    /// its "chance permanent" (rolled once per speaker) and "chance temporary" (each time), 99
    /// or more always (F040888); "target is type" (`type_ok`).
    fn dialogue_open(&mut self, d: &GameData, npc: usize, player: usize, event: i32) -> bool {
        let key = (npc, d.string_id.clone());
        let flag = |k: &str| d.bools.get(k).copied().unwrap_or(false);
        let num = |k: &str, default: f32| d.floats.get(k).copied().or_else(|| d.ints.get(k).map(|&v| v as f32)).unwrap_or(default);
        if self.dialog_memory.locked.contains(&key) || (flag("locked") && !self.dialog_memory.unlocked.contains(&key)) {
            return false;
        }
        if !self.type_ok(d, player) {
            return false;
        }
        // "one at a time" (F040888 +0x27c): none other of its squad still running it (its
        // dialogue on it and not free, F040828, crate::conversation; or in the player's window
        // on it)
        if flag("one at a time")
            && self.squad_of(npc).iter().any(|&k| {
                k != npc
                    && ((!self.run_free(k) && self.runs.get(&k).is_some_and(|r| r.root.as_deref() == Some(d.string_id.as_str())))
                        || (self.dialog_memory.running.get(&k) == Some(&d.string_id) && self.npc_talks.iter().any(|(c, _)| c.npc == k)))
            })
        {
            return false;
        }
        if event == EV_PLAYER_TALK_TO_ME && !flag("for enemies") {
            let (mine, theirs) = (self.folk[npc].faction.clone(), self.folk[player].faction.clone());
            if mine.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(theirs.as_deref())) {
                return false;
            }
        }
        if self.said_lately(d, npc) {
            return false;
        }
        let permanent = num("chance permanent", 100.0);
        if permanent < 99.0 {
            let roll = self.rng.uniform() * 100.0;
            if !*self.dialog_memory.has.entry(key.clone()).or_insert(roll < permanent) {
                return false;
            }
        }
        let temporary = num("chance temporary", 100.0);
        !(temporary < 99.0 && self.rng.uniform() * 100.0 > temporary)
    }

    /// @fn F040912
    ///
    /// A line's effects on the conversation of `npc` (the dialogue's owner) and the one it talks
    /// to, in the game's order: "unlocks" (those dialogues unlocked, the running one locked),
    /// "unlock but keep me", "locks"; "change relations" (that faction's relation with the
    /// player's, F041503, "Relations with {1} improved by {2}" / "decreased by {2}"); "crowd
    /// trigger"; "give item"; "trigger campaign"; each DialogActionEnum of its "effects" (the
    /// DIALOG_ACTION's "action name", the ref's value); then "change AI" / "AI contract". What
    /// the game does through its windows (trading, the slave and bail windows, the character
    /// editor) and the talk to a squad's leader are returned for the game to show.
    pub(crate) fn effects(&mut self, line: &GameData, npc: usize) -> Vec<Effect> {
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
        // "change relations": that faction toward the player's by the ref's value, said
        for (f, v) in line.refs("change relations").to_vec() {
            if v[0] != 0 {
                let name = self.data.get(&f).map_or(f.clone(), |r| r.name.clone());
                self.messages.push(if v[0] < 0 { format!("Relations with {name} decreased by {}", v[0]) } else { format!("Relations with {name} improved by {}", v[0]) });
            }
            self.affect_relation(&f, v[0] as f32);
        }
        // "crowd trigger" (+0x2c8: its first ref, F040901; the value, fcs.def's "% of
        // squadmembers", is never read): the squad's people say that dialogue too
        // (EV_CROWD_TRIGGERED), not from a line a crowd trigger itself started
        if self.dialog_memory.running_event.get(&npc) != Some(&EV_CROWD_TRIGGERED)
            && let Some((sid, _)) = line.refs("crowd trigger").first().cloned()
        {
            self.crowd(npc, &sid, 100.0);
        }
        let target = self.talking_to(npc);
        // "give item": conjured into the one talked to's inventory, a count of 2 .. 49 as one
        // stack of that many ("Received {1}")
        if let Some(p) = target {
            for (item, v) in line.refs("give item").to_vec() {
                if let Some(r) = self.data.get(&item) {
                    let count = if (2..50).contains(&v[0]) { v[0] as u32 } else { 1 };
                    let it = kenshi_char::inventory::Item::from_record(r, count);
                    let name = r.name.clone();
                    if !self.folk[p].inventory.add(it.clone(), false) {
                        self.drop_item(p, it);
                    }
                    self.messages.push(format!("Received {name}"));
                }
            }
        }
        // "trigger campaign" (F040912 -> F050819): on a town, the one talked to's home town (its
        // squad's, F044526 +0x30), the player's outpost nearest the speaker when it is one of
        // the player's (F044512), with no one the town the speaker stands in (Character +0x1a0
        // +0x88: the player's outpost it announces at); none, nothing.
        // One of the list by a single roll over their chances (the refs' third values,
        // `pick_campaign`), scheduled by the speaker's faction's war manager between the ref's
        // first and second values of hours (F050814), from the speaker's squad's home, as a
        // follow-up of the speaker's faction's
        let list: Vec<(String, Vec<i32>)> = line.refs("trigger campaign").iter().map(|(c, v)| (c.clone(), v.to_vec())).collect();
        // (a line said to no one, an announcement's (F040920 with no target): the port keeps
        // the speaker as its partner)
        let target = target.filter(|&t| t != npc);
        let town = match target {
            Some(t) if self.folk[t].player => {
                let at = self.folk[npc].mover.position_units();
                (0..self.outposts.len()).min_by(|&a, &b| (self.outposts[a][0] - at[0]).hypot(self.outposts[a][2] - at[2]).total_cmp(&(self.outposts[b][0] - at[0]).hypot(self.outposts[b][2] - at[2]))).map(crate::world::CampaignTarget::Outpost)
            }
            Some(t) => self.home_town(t).map(crate::world::CampaignTarget::Town),
            None => self.speaker_town(npc),
        };
        if !list.is_empty() && std::env::var("KENSHI_WORLD_DEBUG").is_ok() {
            eprintln!("world: {npc}'s line triggers a campaign on {town:?} (talking to {target:?}, its town {:?})", self.folk[npc].town);
        }
        if let Some(town) = town
            && let Some(f) = self.folk[npc].faction.clone()
            && let Some((c, lo, hi)) = self.pick_campaign(&list)
        {
            let home = self.home_town(npc).map(crate::world::Place::Town);
            self.schedule_campaign(&c, &f, lo.max(0.0), hi.max(lo).max(0.0), true, town, home, Some(&f));
        }
        // (fcs.def's "lock campaign" / "unlock campaign" are never read by the game: nothing)
        let mut out = Vec::new();
        let actions: Vec<(i32, i32)> = line.refs("effects").iter().filter_map(|(sid, v)| self.data.get(sid).map(|a| (a.ints.get("action name").copied().unwrap_or(0), v[0]))).collect();
        for (action, value) in actions {
            if self.effect(action, value, npc, target, &mut out) {
                return out;
            }
        }
        // "change AI" / "AI contract" (F028604): the speaker's squad on that package
        if let Some((pkg, _)) = line.refs("change AI").first() {
            let squad = self.squad_of(npc);
            for k in squad {
                self.set_package(k, pkg);
            }
        }
        if let Some((pkg, v)) = line.refs("AI contract").first().cloned() {
            self.hire(npc, &pkg, v[0].max(0) as f64);
        }
        // "interrupt": the dialogue the speaker starts if the player talks to it now
        if let Some((sid, _)) = line.refs("interrupt").first() {
            let until = self.hours_gone() + 1.0;
            self.dialog_memory.interrupt.insert(npc, (sid.clone(), until));
        }
        out
    }

    /// @fn F040912
    ///
    /// One DialogActionEnum `action` (with the line's `value`) of `npc`'s conversation with
    /// `target`; what the game shows goes into `out`. Whether the conversation ends here (the
    /// rest of the line's effects not done).
    fn effect(&mut self, action: i32, value: i32, npc: usize, target: Option<usize>, out: &mut Vec<Effect>) -> bool {
        use kenshi_ai::PerceptionShortTerm::*;
        let faction = self.folk[npc].faction.clone();
        match action {
            // DA_TRADE: the talk ends, the trade window opens
            1 => out.push(Effect::Trade),
            // DA_TALK_TO_LEADER: with one of the player's, the speaker's squad leader makes its
            // announcement to it (task 102)
            2 => {
                if target.is_some_and(|t| self.folk[t].player) {
                    out.push(Effect::TalkToLeader);
                }
            }
            // DA_JOIN_SQUAD_WITH_EDIT / DA_JOIN_SQUAD_FAST (F041107): it joins the player's (with
            // the character editor); the player's faction full ("max faction size"): "It looks
            // like your squad is a little full." and the talk ends. (not ported) Joining an NPC's
            // squad.
            3 | 18 => {
                if target.is_some_and(|t| self.folk[t].player) {
                    if self.player_count() >= self.max_faction_size() {
                        self.messages.push("It looks like your squad is a little full.".to_string());
                        out.push(Effect::End);
                        return true;
                    }
                    self.recruit(npc);
                    out.push(if action == 3 { Effect::RecruitEdit } else { Effect::Recruit });
                }
            }
            // DA_AFFECT_RELATIONS (F041503): the speaker's faction toward the target's
            4 => {
                if let (Some(f), Some(t)) = (faction.clone(), target) {
                    match self.faction_key(t) {
                        Some(p) if p == PLAYER_FACTION => self.affect_relation(&f, value as f32),
                        Some(o) => self.affect_relation_between(&f, &o, value as f32),
                        None => {}
                    }
                }
                out.push(Effect::Relations(value as f32));
            }
            // DA_AFFECT_REPUTATION (F041492): the speaker's faction's reputation (+0x18)
            5 => {
                if let Some(f) = faction.clone() {
                    *self.dialog_memory.reputation.entry(f).or_default() += value as f32;
                }
            }
            // DA_ATTACK_CHASE_FOREVER ("Assault", 101), DA_ATTACK_STAY_NEAR_HOME ("Attacking
            // Squad", 248), DA_ATTACK_IF_NO_COEXIST (the attack unless the speaker's faction
            // lists the target's in its "coexistence", F041488): the target tagged
            // ST_TEMPORARY_ENEMY and gone for
            6 | 55 | 57 => {
                let Some(t) = target else { return false };
                if action == 57 {
                    let theirs = self.faction_key(t);
                    let coexist = faction.as_deref().and_then(|f| self.data.get(f)).is_some_and(|f| theirs.as_ref().is_some_and(|x| f.refs("coexistence").iter().any(|(s, _)| s == x)));
                    if coexist {
                        return false;
                    }
                }
                self.tag_short(npc, t, StTemporaryEnemy);
                self.dialogue_attack(npc, t);
                out.push(Effect::Attack);
            }
            // DA_GO_HOME ("Going home", 19) / DA_RUN_AWAY ("Running away", 280): the target
            // tagged ST_AGGRESSOR, the speaker's squad off home / away from it
            7 | 34 => {
                if let Some(t) = target {
                    self.tag_short(npc, t, StAggressor);
                    if action == 34 {
                        self.run_from(npc, t);
                        return false;
                    }
                }
                self.go_home_from_talk(npc);
            }
            // DA_TAKE_MONEY: the target's faction pays it ("Paid c.{1}", over 100000 "Paid
            // c.{1}k" in thousands)
            8 => {
                if target.is_some_and(|t| self.folk[t].player) {
                    self.money = (self.money - value).max(0);
                    out.push(Effect::Money(-value));
                }
                self.messages.push(if value as f32 <= 100000.0 { format!("Paid c.{value}") } else { format!("Paid c.{}k", (value as f32 * 0.001) as i32) });
            }
            // DA_GIVE_MONEY: the target's faction's money
            9 => {
                if target.is_some_and(|t| self.folk[t].player) {
                    self.money += value;
                    out.push(Effect::Money(value));
                }
            }
            // DA_PAY_BOUNTY: the bounty on whom the target carries (+0x388), paid to the
            // target's faction once (F046515 / F046514: claimed), half for a dead one ("Received
            // c.{1} reward."); the speaker takes them ("Lifting/Snatching", 213) and cages them
            // ("Find and put in cage", 112), both remembered as their captors
            10 => {
                if let Some(t) = target {
                    self.claim_bounty(npc, t);
                }
            }
            // DA_CHARACTER_EDITOR: the talk ends, the editor on the target
            11 => {
                out.push(Effect::CharacterEditor);
                return true;
            }
            // DA_FORCE_SPEECH_TIMER (case 0xc): the dialogue's +0x234 set to the value (seconds),
            // counted down by the dialogue's update (F040923, crate::conversation) before the
            // conversation goes on
            12 => {
                self.runs.entry(npc).or_default().wait = value as f32;
            }
            // DA_DECLARE_WAR (F041505) / DA_END_WAR (F041504): the speaker's faction and the
            // target's
            13 | 14 => {
                if let (Some(f), Some(t)) = (faction.clone(), target)
                    && let Some(o) = self.faction_key(t)
                {
                    if action == 13 { self.declare_war(&f, &o) } else { self.end_war(&f, &o) }
                }
            }
            // DA_CLEAR_AI (case 0xf): the speaker's AI begins afresh (its planner's rethink flag,
            // +0x130, set and its timers cleared)
            15 => {
                let p = &mut self.folk[npc];
                p.ai.queue.clear();
                p.rethink = true;
            }
            // DA_FOLLOW_WHILE_TALKING ("Following and talking", 164): it keeps up with the target
            16 => {
                if let Some(t) = target {
                    self.order_follow(&[npc], t, false);
                }
            }
            // DA_THUG_HUNTER ("Stalking", 165, `stalking_tick`)
            17 => {
                if let Some(t) = target {
                    self.folk[npc].stalk = Some(t);
                }
            }
            // DA_REMEMBER_CHARACTER (F040854, tag 0): its memory of the target made
            19 => {
                if let Some(t) = target {
                    let now = self.hours_gone();
                    self.dialog_memory.met.insert((npc, t), now);
                }
            }
            // DA_FLAG_TEMP_ALLY: the speaker's squad tags the target ST_TEMPORARY_ALLY and no
            // longer fights its side until the tag lapses (`short_tag_tick`)
            20 => {
                if let Some(t) = target {
                    self.tag_short(npc, t, StTemporaryAlly);
                }
                self.temp_ally(npc);
            }
            // DA_FLAG_TEMP_ENEMY (case 0x15): the speaker's squad's memory tags the target
            // ST_TEMPORARY_ENEMY (F04d9c8, and nothing more): its AI then takes it for an enemy
            // (`temp_enemies`, read by the AI view's enemy test). (stand-in) The squad is also
            // ordered at it: of the planners' goals only Attacking enemies (16) goes for the
            // enemies the view counts, and squads without it (guards) would not fight.
            21 => {
                if let Some(t) = target {
                    self.tag_short(npc, t, StTemporaryEnemy);
                    if self.folk[npc].prison.is_none() {
                        self.dialogue_attack(npc, t);
                    }
                }
                out.push(Effect::Attack);
            }
            // DA_MAKE_TARGET_RUN_FASTER (case 0x17): the target moves at a run (its movement's
            // speed mode, +0x640 +0x20, set to RUN)
            23 => {
                if let Some(p) = target {
                    let mode = kenshi_char::movement::MoveSpeed::Run.cap(15.0).unwrap();
                    let own = if self.folk[p].own_speed > 0.0 { self.folk[p].own_speed } else { self.run };
                    let q = &mut self.folk[p];
                    q.mover.set_max_speed(kenshi_char::movement::speed_limit(own, mode, None));
                    q.ordered_cap = Some(mode);
                }
            }
            // DA_GIVE_TARGET_MY_SLAVES (case 0x18): the slaves of its squad (their cages
            // opened) the target's squad's (F01f235); (stand-in) given to the player's, they
            // join it free
            24 => {
                let owner = self.folk[npc].platoon;
                let slaves: Vec<usize> = self.folk.iter().filter(|p| owner.is_some() && p.slave.as_ref().is_some_and(|s| s.owner == owner)).map(|p| p.id).collect();
                for s in slaves {
                    if let Some(c) = self.folk[s].prison.as_ref().map(|pr| pr.cage) {
                        self.leave_cage(s, c);
                    }
                    self.folk[s].prison = None;
                    match target.filter(|&t| !self.folk[t].player) {
                        Some(t) => {
                            let f = self.folk[t].faction.clone().unwrap_or_default();
                            self.make_slave(s, self.folk[t].platoon, &f);
                        }
                        None => {
                            self.folk[s].slave = None;
                            self.folk[s].shackled = false;
                            self.recruit(s);
                        }
                    }
                }
            }
            // DA_TAG_ESCAPED_SLAVE (F038704 2): the target an escaping slave of the speaker's
            // faction
            25 => {
                if let (Some(f), Some(p)) = (faction.clone(), target) {
                    let owner = self.folk[npc].platoon;
                    self.tag_escaped_slave(p, &f, owner);
                }
            }
            // DA_SEPARATE_TO_MY_OWN_SQUAD (F039244): the speaker into a squad of its own (one of
            // the player's speaking: the target)
            28 => {
                let who = if self.folk[npc].player { target } else { Some(npc) };
                if let Some(w) = who {
                    let pid = self.bar_states.len();
                    self.bar_states.push(None);
                    self.folk[w].platoon = Some(pid);
                    self.folk[w].role = 5;
                }
            }
            // DA_ARREST_TARGET: the speaker at its squad's home town arrests the target
            // ("Capture Target" 240, "Find and put in cage" 112; the target's squad remembers it
            // as captor); elsewhere it tags it ST_TEMPORARY_ENEMY and attacks ("Attacking", 4)
            29 => {
                let Some(t) = target else { return false };
                if self.at_home_town(npc) && self.folk[t].race.is_none() {
                    let mine = faction.clone().unwrap_or_default();
                    // a bounty hunter's catch goes to the bounty's faction (assumption)
                    let to = (self.bounty_with(t, &mine) == 0).then(|| self.top_bounty(t)).flatten();
                    self.folk[npc].arrest = Some(crate::law::Arrest { target: t, slaver: false, to });
                    if !self.folk[t].down {
                        self.order_attack(&[npc], t);
                    }
                } else {
                    self.tag_short(npc, t, StTemporaryEnemy);
                    self.order_attack(&[npc], t);
                }
            }
            // DA_ARREST_TARGETS_CARRIED_PERSON: at its squad's home town, whom the target
            // carries is taken from it and caged
            30 => {
                if let Some(t) = target
                    && self.at_home_town(npc)
                {
                    self.take_carried(npc, t, false);
                }
            }
            // DA_ASSIGN_BOUNTY (0x20): on one not in the middle of a crime, a bounty of the
            // line's value with the speaker's faction (F046510); on one committing a crime, that
            // crime is caught (F046511)
            32 => {
                if let (Some(f), Some(p)) = (faction.clone(), target) {
                    match self.folk[p].crime.clone() {
                        None if value > 0 => self.add_bounty_amount(p, &f, value),
                        Some(c) => self.add_bounty(p, &f, c.kind),
                        None => {}
                    }
                }
            }
            // DA_CRIME_ALARM (F040912 0x21): the target's crime seen and the town's alarm
            // raised (crate::law `crime_alarm`)
            33 => {
                if let Some(t) = target {
                    self.crime_alarm(npc, t);
                }
                out.push(Effect::Alarm);
            }
            // DA_INCREASE_FACTION_RANK (0x23): one rank up while under the line's value (0:
            // always)
            35 => {
                if let Some(f) = faction.clone() {
                    let rank = self.dialog_memory.ranks.entry(f).or_default();
                    if value == 0 || *rank < value {
                        *rank += 1;
                    }
                }
            }
            // DA_LOCK_THIS_DIALOG
            36 => {
                if let Some(r) = self.dialog_memory.running.get(&npc).cloned() {
                    self.dialog_memory.locked.insert((npc, r));
                }
            }
            // DA_ASSAULT_PHASE / DA_RETREAT_PHASE / DA_VICTORY_PHASE (0x25..0x27): the campaign
            // the speaker's squad is on (F03da87) to its assault (1, or over without one), over
            // (2) or victory (3), its clocks restarted (`set_campaign_phase`). On none, nothing.
            37..=39 => {
                if let Some(c) = self.campaign_of(npc) {
                    let phase = match action {
                        37 => crate::world::PHASE_ASSAULT,
                        38 => crate::world::PHASE_OVER,
                        _ => crate::world::PHASE_VICTORY,
                    };
                    self.set_campaign_phase(&c, phase);
                }
                if action == 37 {
                    out.push(Effect::Attack);
                }
            }
            // DA_ENSLAVE_TARGETS_CARRIED_PERSON: whom the target carries is taken (out of the
            // player's squad) and caged as the speaker's faction's slave
            40 => {
                if let Some(t) = target {
                    self.take_carried(npc, t, true);
                }
                out.push(Effect::EnslaveCarried);
            }
            41 => out.push(Effect::SellSlaves),
            42 => out.push(Effect::BuySlaves),
            43 => out.push(Effect::Bail),
            48 => out.push(Effect::BuyAnimals(value)),
            // SURRENDER_NON_HUMANS (case 0x2f, `surrender_non_humans`)
            47 => {
                if let Some(t) = target {
                    self.surrender_non_humans(npc, t);
                }
            }
            // DA_CLEAR_BOUNTY (0x31): the target's bounty with the speaker's faction and its
            // crime cleared, its heat gone
            49 => {
                if let (Some(f), Some(p)) = (faction.clone(), target) {
                    self.folk[p].bounties.retain(|b| b.faction != f);
                    self.folk[p].crime = None;
                    self.folk[p].heat = 0.0;
                }
            }
            // DA_SUMMON_MY_SQUAD (0x34): its squad comes to it ("Running", 67)
            52 => {
                let mates: Vec<usize> = self.squad_of(npc).into_iter().filter(|&k| k != npc).collect();
                let at = self.folk[npc].mover.position_units();
                if !mates.is_empty() {
                    self.order_move_squad(&mates, at);
                }
            }
            // DA_REMOVE_SLAVE_STATUS (0x35): the target no longer a slave ("Free!"), its
            // shackles unlocked
            53 => {
                if let Some(s) = target {
                    if self.folk[s].shackled || self.folk[s].slave.is_some() {
                        self.messages.push(format!("{}: Free!", self.display_name(s)));
                    }
                    if let Some(c) = self.folk[s].prison.as_ref().filter(|pr| pr.slave).map(|pr| pr.cage) {
                        self.leave_cage(s, c);
                        self.folk[s].prison = None;
                    }
                    self.folk[s].slave = None;
                    self.folk[s].escaped = None;
                    self.remove_shackles(s);
                }
            }
            // DA_OPEN_NEAREST_GATE (case 0x36 -> F029271)
            54 => self.open_nearest_gate(npc),
            // DA_MASSIVE_ALARM (0x38, `massive_alarm`)
            56 => {
                if let Some(t) = target {
                    self.massive_alarm(npc, t);
                }
                out.push(Effect::MassiveAlarm);
            }
            // DA_KNOCKOUT (0x3a): the line's value on the speaker's +0x4f8 (its prone timer,
            // F039115). (stand-in) It goes down for that many seconds ("*Hrk!*").
            58 => {
                self.knock_out(npc);
                self.arena.f[npc].medical.knockout_time = value.max(1) as f32;
            }
            // F040912 has no case for DA_MATES_KILL_ME, DA_FREE_TARGET_SLAVE,
            // DA_MERGE_WITH_SIMILAR_SQUADS, DA_ATTACK_TOWN, CHOOSE_CONSCRIPTION,
            // CHOOSE_RECRUITING, CHOOSE_HIRING_CONTRACT, DA_PLAYER_SELL_PRISONERS,
            // DA_PLAYER_SURRENDER_MEMBER_DIFFERENT_RACE: nothing
            _ => {}
        }
        false
    }

    /// The attack of a dialogue (DA_ATTACK_*, the assault phase): the speaker's squad, those
    /// standing, on `target`.
    fn dialogue_attack(&mut self, npc: usize, target: usize) {
        let squad: Vec<usize> = self.squad_of(npc).into_iter().filter(|&k| !self.folk[k].down).collect();
        let squad = if squad.is_empty() { vec![npc] } else { squad };
        self.order_attack(&squad, target);
    }

    /// Whether `npc` is at its squad's home town (Platoon +0x30 = the town it is in, vtable 0x70
    /// +0x88): its home building's town, the one it stands in.
    fn at_home_town(&self, npc: usize) -> bool {
        let home = self.folk[npc].home.and_then(|h| self.placed_town.get(h).copied().flatten());
        home.is_some() && home == self.town_of(npc)
    }

    /// "Going home" (task 19): `npc`'s squad home: a world squad to its town (`go_home`), a
    /// town's people to their home building.
    fn go_home_from_talk(&mut self, npc: usize) {
        if let Some(k) = self.world.squads.iter().position(|s| s.members.contains(&npc)) {
            self.go_home(k);
        } else if let Some(h) = self.folk[npc].home {
            let at = self.placed_at[h];
            let squad: Vec<usize> = self.squad_of(npc).into_iter().filter(|&k| !self.folk[k].down).collect();
            self.order_move_squad(&squad, at);
        }
    }

    /// "Running away" (task 280) from `from`: (assumption) `npc`'s squad runs 2000 units
    /// straight away from it.
    fn run_from(&mut self, npc: usize, from: usize) {
        let (a, b) = (self.folk[npc].mover.position_units(), self.folk[from].mover.position_units());
        let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
        let d = dx.hypot(dz).max(1.0);
        let to = [a[0] + dx / d * 2000.0, a[1], a[2] + dz / d * 2000.0];
        let squad: Vec<usize> = self.squad_of(npc).into_iter().filter(|&k| !self.folk[k].down).collect();
        self.order_move_squad(&squad, to);
    }

    /// DA_PAY_BOUNTY (F040912 case 10): the bounty on whom `target` carries, with `npc`'s
    /// faction (F046513), paid to the target's side once (F046515 / F046514, the bounty kept but
    /// claimed), halved for a dead one; "Received c.{1} reward."; then `npc` takes them
    /// (`take_carried`).
    pub fn claim_bounty(&mut self, npc: usize, target: usize) {
        let Some(c) = self.folk[target].carrying else { return };
        if let Some(f) = self.folk[npc].faction.clone()
            && !self.dialog_memory.claimed.contains(&(c, f.clone()))
        {
            let mut amount = self.bounty_with(c, &f);
            if self.folk[c].dead {
                amount /= 2;
            }
            if self.folk[target].player {
                self.money += amount;
            }
            self.dialog_memory.claimed.insert((c, f));
            self.messages.push(format!("Received c.{amount} reward."));
        }
        self.take_carried(npc, target, false);
    }

    /// "Lifting/Snatching" (213) and "Find and put in cage" (112): `npc` takes whom `target`
    /// carries (put down, then picked up by `npc`) to a cage of its faction, as a prisoner or
    /// (`slave`) its faction's slave (`law_tick`), remembering both as its captors
    /// (LT_MY_CAPTOR). (stand-in) A dead one is left where it is put down; (not ported) F039244
    /// putting the taken in a squad of its own.
    pub fn take_carried(&mut self, npc: usize, target: usize, slave: bool) {
        let Some(c) = self.folk[target].carrying else { return };
        self.order_drop(&[target]);
        self.remember(c, npc, LT_MY_CAPTOR);
        self.remember(c, target, LT_MY_CAPTOR);
        if self.folk[c].dead {
            return;
        }
        self.folk[npc].arrest = Some(crate::law::Arrest { target: c, slaver: slave, to: None });
    }

    /// Whose memory person `id`'s is: its platoon's, else its own.
    fn memory_key(&self, id: usize) -> u64 {
        match self.folk[id].platoon {
            Some(p) => p as u64,
            None => (1u64 << 40) + id as u64,
        }
    }

    /// @fn F040863
    ///
    /// `holder`'s squad tags `subject` (short-term, PerceptionShortTerm): a temporary-enemy tag
    /// dropped when the subject is its ally; one of the player's characters tagged as the whole
    /// player squad (F040862).
    pub fn tag_short(&mut self, holder: usize, subject: usize, tag: kenshi_ai::PerceptionShortTerm) {
        if holder == subject {
            return;
        }
        let ally = self.faction_relation(holder, subject) >= kenshi_ai::faction::ALLY_RELATION;
        if !kenshi_ai::tags::should_tag(tag, ally, ally) {
            return;
        }
        let own = self.folk[holder].platoon.is_some() && self.folk[holder].platoon == self.folk[subject].platoon;
        let squad = self.folk[subject].player.then_some(0u8);
        let clock = self.hours_gone() * 3600.0 * 1e6;
        let now = self.hours_gone();
        self.dialog_memory.met.insert((holder, subject), now);
        let key = self.memory_key(holder);
        self.platoon_memory.entry(key).or_default().add_tag(subject, own, squad, tag, clock);
    }

    /// Each person's squad's temporary enemies now (its platoon memory's ST_TEMPORARY_ENEMY
    /// tags set within `SHORT_TAG_HOURS`; a player squad's entry is all the player's people):
    /// the AI's enemy test takes them for enemies (Character::isEnemy, F044572), so a squad that
    /// tags someone (DA_FLAG_TEMP_ENEMY) goes for it as it would a foe.
    pub fn temp_enemies(&self) -> Vec<Vec<usize>> {
        let now = self.hours_gone() * 3600.0;
        let flag = kenshi_ai::PerceptionShortTerm::StTemporaryEnemy as i32;
        let players: Vec<usize> = self.folk.iter().filter(|p| p.player).map(|p| p.id).collect();
        let mut by_key: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
        for (k, m) in &self.platoon_memory {
            let mut v = Vec::new();
            for (s, t) in &m.entries {
                if !t.short_term.has(flag) || now - t.time >= SHORT_TAG_HOURS * 3600.0 {
                    continue;
                }
                match s {
                    kenshi_ai::tags::Subject::Character(c) => v.push(*c),
                    kenshi_ai::tags::Subject::Squad(_) => v.extend(players.iter().copied()),
                }
            }
            if !v.is_empty() {
                // (in a fixed order: the tags' map order is the hasher's)
                v.sort_unstable();
                by_key.insert(*k, v);
            }
        }
        (0..self.folk.len()).map(|id| by_key.get(&self.memory_key(id)).cloned().unwrap_or_default()).collect()
    }

    /// Whether `holder`'s squad has short-term tag `tag` on `subject`, set within
    /// `SHORT_TAG_HOURS`.
    pub fn has_short(&self, holder: usize, subject: usize, tag: kenshi_ai::PerceptionShortTerm) -> bool {
        let squad = self.folk[subject].player.then_some(0u8);
        let now = self.hours_gone() * 3600.0;
        self.platoon_memory
            .get(&self.memory_key(holder))
            .and_then(|m| m.tags(subject, squad))
            .is_some_and(|t| t.short_term.has(tag as i32) && now - t.time < SHORT_TAG_HOURS * 3600.0)
    }

    /// @fn F040881
    ///
    /// Whether `holder`'s squad memory tags `subject` with any of the short-term tags of `mask`
    /// (bits by PerceptionShortTerm; the entry's +0x28), however long ago (one of the player's
    /// as the whole player squad, as `tag_short` keeps them).
    pub(crate) fn tagged_short_mask(&self, holder: usize, subject: usize, mask: u32) -> bool {
        let squad = self.folk[subject].player.then_some(0u8);
        self.platoon_memory.get(&self.memory_key(holder)).and_then(|m| m.tags(subject, squad)).is_some_and(|t| t.short_term.0 & mask != 0)
    }

    /// @fn F044487
    ///
    /// The leader of the player's platoon `platoon` (its active platoon's): its leader while
    /// up and free, else the first of it up and free.
    pub(crate) fn player_squad_leader(&self, platoon: usize) -> Option<usize> {
        let free = |q: &crate::sim::Person| q.player && q.platoon == Some(platoon) && !q.dead && !q.down && !q.away && q.prison.is_none() && q.bed.is_none() && q.slave.is_none();
        self.folk.iter().filter(|q| free(q)).find(|q| q.role == 5).or_else(|| self.folk.iter().find(|q| free(q))).map(|q| q.id)
    }

    /// Every second: squads whose temporary-ally tag on the player's has run out
    /// (`SHORT_TAG_HOURS`) count the player's side among their foes again when their faction
    /// is its enemy.
    fn short_tag_tick(&mut self) {
        let player_side = self.side(&Some(PLAYER_FACTION.to_string()));
        let now = self.hours_gone() * 3600.0;
        let lapsed: Vec<u64> = self
            .platoon_memory
            .iter_mut()
            .filter_map(|(k, m)| {
                let t = m.entries.get_mut(&kenshi_ai::tags::Subject::Squad(0))?;
                let ally = kenshi_ai::PerceptionShortTerm::StTemporaryAlly as i32;
                (t.short_term.has(ally) && now - t.time >= SHORT_TAG_HOURS * 3600.0).then(|| {
                    t.short_term.0 &= !(1 << ally);
                    *k
                })
            })
            .collect();
        for k in lapsed {
            for id in 0..self.folk.len() {
                if self.memory_key(id) != k || self.folk[id].player {
                    continue;
                }
                let enemy = self.folk[id].faction.as_ref().and_then(|f| self.relations.get(f)).is_some_and(|r| r.is_enemy(Some(PLAYER_FACTION)));
                if enemy && let Some(foes) = &mut self.arena.f[id].foes && !foes.contains(&player_side) {
                    foes.push(player_side);
                }
            }
        }
    }

    /// A line's "crowd trigger" (F040912 +0x2c8): each member of `npc`'s squad not down, not
    /// an animal and not a slave (state 1) starts dialogue `sid` (EV_CROWD_TRIGGERED, forced,
    /// F040917) with the one `npc` talks to: its line said aloud, its effects done.
    /// (assumption) The speaker itself, in its conversation, is left out; one that has just
    /// said it does not again. (`_percent` unused: kept for callers.)
    pub fn crowd(&mut self, npc: usize, sid: &str, _percent: f32) {
        if self.data.get(sid).is_none() {
            return;
        }
        let target = self.talking_to(npc).unwrap_or(npc);
        let now = self.hours_gone();
        let mates: Vec<usize> = self
            .squad_of(npc)
            .into_iter()
            .filter(|&k| k != npc && !self.folk[k].down && !self.folk[k].dead && self.folk[k].race.is_none() && self.slave_state(k) != 1)
            .filter(|&k| self.dialog_memory.spoken.get(&(k, sid.to_string())) != Some(&now))
            .collect();
        for m in mates {
            self.dialog_memory.running_event.insert(m, EV_CROWD_TRIGGERED);
            if let Some((mut conv, _)) = self.start_dialogue(m, target, sid) {
                let said = std::mem::take(&mut conv.npc_text);
                self.say(m, said);
            }
        }
    }

    /// The size level (`town_level`) of the player's outpost at `at`: its buildings within
    /// 2500 units.
    pub fn outpost_level(&self, at: [f32; 3]) -> i32 {
        town_level(self.buildings.iter().filter(|b| b.confirmed && (b.at[0] - at[0]).hypot(b.at[2] - at[2]) < 2500.0).count())
    }

    /// A line's "AI contract": `npc`'s squad on package `pkg` for `hours`, hired by the one it
    /// talks to (a contract it is already on replaced).
    pub fn hire(&mut self, npc: usize, pkg: &str, hours: f64) {
        let members = self.squad_of(npc);
        let hirer = self.talking_to(npc).unwrap_or(npc);
        let mut was = Vec::new();
        for (k, &m) in members.iter().enumerate() {
            let old = self.contracts.iter().position(|c| c.members.contains(&m));
            let before = match old {
                Some(c) => self.contracts[c].members.iter().position(|&x| x == m).and_then(|i| self.contracts[c].was.get(i).cloned().flatten()),
                None => self.folk[m].package.clone(),
            };
            was.push(before);
            let _ = k;
        }
        self.contracts.retain(|c| !c.members.iter().any(|m| members.contains(m)));
        for &m in &members {
            self.set_package_for(m, pkg, Some(hirer));
        }
        let leader = members.iter().copied().find(|&m| self.folk[m].role == 5).unwrap_or(npc);
        let until = self.hours_gone() + hours;
        self.contracts.push(Contract { members, leader, hirer, package: pkg.to_string(), was, until });
    }

    /// Dialogue `sid` started by `npc` with `player` at the line it would open with.
    pub fn start_dialogue(&mut self, npc: usize, player: usize, sid: &str) -> Option<(Conversation, Vec<Effect>)> {
        let d = self.data.get(sid).cloned()?;
        let line = self.choose_line(d.refs("lines"), npc, player)?;
        let now = self.hours_gone();
        self.dialog_memory.spoken.insert((npc, d.string_id.clone()), now);
        self.dialog_memory.partner.insert(npc, player);
        self.dialog_memory.running.insert(npc, d.string_id.clone());
        let mut effects = self.effects(&line, npc);
        let conv = self.at_line(&line, npc, player);
        effects.append(&mut self.pending_effects);
        Some((conv, effects))
    }

    /// Every second: contracts run out, their squads back on their own packages and their
    /// leaders telling whoever hired them (`Contract`).
    pub fn contract_tick(&mut self) {
        let now = self.hours_gone();
        let done: Vec<Contract> = self.contracts.iter().filter(|c| now >= c.until).cloned().collect();
        self.contracts.retain(|c| now < c.until);
        for c in done {
            for (m, was) in c.members.iter().zip(&c.was) {
                if let Some(p) = was {
                    self.set_package(*m, p);
                }
            }
            if self.folk[c.leader].down || self.folk[c.leader].dead {
                continue;
            }
            self.dialog_memory.partner.insert(c.leader, c.hirer);
            let pkg = self.data.get(&c.package).cloned();
            let delivery = pkg.as_ref().and_then(|p| p.refs("contract end dialog delivery").first().map(|(s, _)| s.clone()));
            let passive = pkg.as_ref().and_then(|p| p.refs("contract end talk passive").first().map(|(s, _)| s.clone()));
            let said = match (delivery, passive) {
                (Some(s), _) => self.start_dialogue(c.leader, c.hirer, &s),
                (None, Some(s)) => self.start_dialogue(c.leader, c.hirer, &s).map(|(mut conv, fx)| {
                    conv.choices.clear();
                    (conv, fx)
                }),
                (None, None) => self.talk_on(c.leader, c.hirer, EV_CONTRACT_JOB_ENDED, false),
            };
            if let Some((mut conv, effects)) = said {
                if conv.choices.is_empty() {
                    let text = std::mem::take(&mut conv.npc_text);
                    self.say(c.leader, text);
                    if effects.is_empty() {
                        continue;
                    }
                }
                self.npc_talks.push((conv, effects));
            }
        }
    }

    /// @fn F040912
    ///
    /// CHOOSE_PRISONER_BAIL (case 0x2b): the bail window's prisoners, those in the cages of
    /// the town `jailer` stands in, alive: the player's people, and others not its faction's
    /// enemies, not holding it in hate (F038592) unless also in love (F038591), and of a
    /// fighting level (F038617) under 36. In no town, none.
    pub fn bail_list(&self, jailer: usize) -> Vec<usize> {
        let Some(t) = self.town_of(jailer) else { return Vec::new() };
        let cages: Vec<usize> = (0..self.furn.len()).filter(|&k| self.furn[k].function == kenshi_world::furniture::BF_CAGE && self.towns[t].contains(self.furn[k].position, 1.0)).collect();
        let theirs = self.folk[jailer].faction.as_deref().and_then(|f| self.relations.get(f));
        self.folk
            .iter()
            .filter(|p| !p.dead && p.prison.as_ref().is_some_and(|pr| cages.contains(&pr.cage)))
            .filter(|p| {
                let tags = self.squad_tags(p.id, jailer).0;
                p.player || (!theirs.is_some_and(|r| r.is_enemy(self.faction_key(p.id).as_deref())) && (tags & 0x2b0 == 0 || tags & 0xc != 0) && self.fighting_level(p.id) < 36.0)
            })
            .map(|p| p.id)
            .collect()
    }

    /// @fn F038617
    ///
    /// A character's fighting level for the bail window: (toughness + the better of martial
    /// arts and melee attack + the better of strength and dexterity) / 3. (assumption) The
    /// toughness's factor (+0x34) 1, the arts' and attack's weapon bonuses (F047041, F047042)
    /// left out.
    fn fighting_level(&self, id: usize) -> f32 {
        use kenshi_char::StatsEnumerated::*;
        let l = |s| self.arena.f[id].level(s);
        (l(StatToughness) + l(StatMartialarts).max(l(StatMeleeAttack)) + l(StatStrength).max(l(StatDexterity))) / 3.0
    }

    /// @fn F041452
    ///
    /// A prisoner's bail ("Pay Bounty"): twice its bounty where it is held (F046513 with no
    /// faction: its cage owner's, F046482); one not of the player's at least 2000.
    pub fn bail_price(&self, id: usize) -> i32 {
        let f = self.folk[id].prison.as_ref().map(|p| p.faction.clone()).unwrap_or_default();
        let price = 2 * self.bounty_with(id, &f);
        if self.folk[id].player { price } else { price.max(2000) }
    }

    /// @fn F041464
    ///
    /// The player pays prisoner `id` out: the bail paid, out of its cage and shackles, no
    /// slave, its bounties cleared; it says EV_BOUGHT_ME_FROM_SLAVERY to (assumption) the
    /// nearest of the player's (the trade window's buyer). None: not enough money.
    pub fn bail_out(&mut self, id: usize) -> Option<i32> {
        let price = self.bail_price(id);
        if self.money < price {
            return None;
        }
        let Some(pr) = self.folk[id].prison.clone() else { return None };
        self.money -= price;
        self.leave_cage(id, pr.cage);
        self.folk[id].prison = None;
        self.folk[id].shackled = false;
        self.folk[id].bounties.clear();
        if self.folk[id].slave.is_some() {
            self.set_slave_state(id, 0);
        }
        let at = self.folk[id].mover.position_units();
        let buyer = self
            .folk
            .iter()
            .filter(|p| p.player && !p.dead && p.id != id)
            .min_by(|a, b| {
                let d = |p: &crate::sim::Person| {
                    let q = p.mover.position_units();
                    (q[0] - at[0]).hypot(q[2] - at[2])
                };
                d(a).total_cmp(&d(b))
            })
            .map(|p| p.id);
        if let Some(b) = buyer {
            self.bark_forced(id, b, EV_BOUGHT_ME_FROM_SLAVERY);
        }
        Some(price)
    }

    /// Faction `a`'s relation entry toward `b` (made from its record when not yet kept): its
    /// value now.
    fn relation_entry(&mut self, a: &str, b: &str) -> Option<&mut kenshi_ai::faction::Relation> {
        if !self.relations.contains_key(a)
            && let Some(r) = self.data.get(a)
        {
            let rel = kenshi_ai::faction::FactionRelations::from_data(r, &self.data);
            self.relations.insert(a.to_string(), rel);
        }
        let rel = self.relations.get_mut(a)?;
        let now = rel.relation(b);
        Some(rel.relations.entry(b.to_string()).or_insert(kenshi_ai::faction::Relation { forced_ally: false, relation: now }))
    }

    /// @fn F041503
    ///
    /// Faction `a`'s relation toward `b` moved by `d`: a fall only while above -100, a rise only
    /// while under 100, then held within -100 .. 100. Its value before and after.
    fn shift_relation(&mut self, a: &str, b: &str, d: f32) -> Option<(f32, f32)> {
        let r = self.relation_entry(a, b)?;
        let old = r.relation;
        if (d < 0.0 && old > -100.0) || (d > 0.0 && old < 100.0) {
            r.relation += d;
        }
        r.relation = r.relation.clamp(-100.0, 100.0);
        Some((old, r.relation))
    }

    /// @fn F041501
    ///
    /// The messages of faction `f`'s relation toward the player's going from `old` to `new`:
    /// across -30 "{1} are now hostile towards you" / "{1} are no longer hostile towards you",
    /// across 50 "{1} are no longer your ally" / "{1} are now your allies".
    fn relation_messages(&mut self, f: &str, old: f32, new: f32) {
        use kenshi_ai::faction::{ALLY_RELATION, ENEMY_RELATION};
        let name = self.data.get(f).map_or(f.to_string(), |r| r.name.clone());
        if old > ENEMY_RELATION && new <= ENEMY_RELATION {
            self.messages.push(format!("{name} are now hostile towards you"));
        }
        if old <= ENEMY_RELATION && new > ENEMY_RELATION {
            self.messages.push(format!("{name} are no longer hostile towards you"));
        }
        if old > ALLY_RELATION && new <= ALLY_RELATION {
            self.messages.push(format!("{name} are no longer your ally"));
        }
        if old <= ALLY_RELATION && new > ALLY_RELATION {
            self.messages.push(format!("{name} are now your allies"));
        }
    }

    /// Faction `f`'s relation with the player's changed by `d` (F041503), both ways (its toward
    /// the player's, which the game keeps, and the player's toward it, which the world states'
    /// "player ally" / "player enemy" read here), with the messages of crossing hostile or
    /// allied (F041501).
    pub fn affect_relation(&mut self, f: &str, d: f32) {
        if f == PLAYER_FACTION {
            return;
        }
        let change = self.shift_relation(f, PLAYER_FACTION, d);
        self.shift_relation(PLAYER_FACTION, f, d);
        if let Some((old, new)) = change {
            self.relation_messages(f, old, new);
        }
    }

    /// Faction `a`'s relation toward `b` changed by `d` (F041503; the player's both ways).
    pub fn affect_relation_between(&mut self, a: &str, b: &str, d: f32) {
        if b == PLAYER_FACTION {
            return self.affect_relation(a, d);
        }
        if a == PLAYER_FACTION {
            return self.affect_relation(b, d);
        }
        self.shift_relation(a, b, d);
    }

    /// Faction `a`'s relation toward `b` set to `to` (the player's both ways, with its
    /// messages).
    fn set_relation(&mut self, a: &str, b: &str, to: f32) {
        let player = if b == PLAYER_FACTION { Some(a) } else if a == PLAYER_FACTION { Some(b) } else { None };
        let (x, y) = match player {
            Some(f) => (f, PLAYER_FACTION),
            None => (a, b),
        };
        let Some(r) = self.relation_entry(x, y) else { return };
        let old = r.relation;
        r.relation = to;
        if let Some(f) = player {
            if let Some(r) = self.relation_entry(PLAYER_FACTION, f) {
                r.relation = to;
            }
            self.relation_messages(f, old, to);
        }
    }

    /// @fn F041505
    ///
    /// DA_DECLARE_WAR: faction `a` on `b`, unless already hostile (-30 or less): "War breaks out
    /// between {1} and {2}", the relation down to -35.
    pub fn declare_war(&mut self, a: &str, b: &str) {
        let now = self.relation_of(Some(a), Some(b));
        if now < kenshi_ai::faction::ENEMY_RELATION {
            return;
        }
        let name = |s: &Self, f: &str| if f == PLAYER_FACTION { s.player_faction_name() } else { s.data.get(f).map_or(f.to_string(), |r| r.name.clone()) };
        let (na, nb) = (name(self, a), name(self, b));
        self.messages.push(format!("War breaks out between {na} and {nb}"));
        if now >= -35.0 {
            self.set_relation(a, b, -35.0);
        }
    }

    /// @fn F041504
    ///
    /// DA_END_WAR: faction `a`'s war on `b` over, a relation of -25 or less raised to -25.
    pub fn end_war(&mut self, a: &str, b: &str) {
        if self.relation_of(Some(a), Some(b)) <= -25.0 {
            self.set_relation(a, b, -25.0);
        }
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

    /// DA_MASSIVE_ALARM (F040912 0x38): when `npc` is in a town (its +0x88), every squad of its
    /// faction about (the faction's active platoons, +0x208: here its people loaded) turns on
    /// `target`: each says its EV_I_SEE_ENEMY_PLAYER line and attacks (task 0x1d).
    pub fn massive_alarm(&mut self, npc: usize, target: usize) {
        if self.folk[npc].town.is_none() {
            return;
        }
        let side = self.arena.f[target].team;
        let faction = self.folk[npc].faction.clone();
        let answering: Vec<usize> = (0..self.folk.len()).filter(|&k| {
            let w = &self.folk[k];
            !w.player && !w.down && !w.dead && !w.away && w.faction == faction && w.prison.is_none() && w.slave.is_none()
        }).collect();
        for &k in &answering {
            if let Some(foes) = &mut self.arena.f[k].foes
                && !foes.contains(&side)
            {
                foes.push(side);
            }
            self.bark(k, target, crate::speech::EV_I_SEE_ENEMY_PLAYER);
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
        // into the squad of the one it talked with (else the player's first): its platoon
        let squad = self.talking_to(npc).filter(|&t| self.folk[t].player).or_else(|| self.folk.iter().position(|q| q.player && !q.dead && q.race.is_none())).and_then(|t| self.folk[t].platoon);
        let p = &mut self.folk[npc];
        p.player = true;
        if squad.is_some() {
            p.platoon = squad;
        }
        p.faction = Some(PLAYER_FACTION.to_string());
        p.ai.queue.clear();
        p.home = None;
        p.shop = None;
        let side = self.side(&Some(PLAYER_FACTION.to_string()));
        self.arena.f[npc].team = side;
    }
}

impl Sim {
    /// Every second: those stalking someone (task 165, "Stalking", given by DA_THUG_HUNTER:
    /// "We're gonna get you later!", "Nah, wait 'till /HE/ leaves town.") keep after them and
    /// go for them once they are out of any town; the stalked one dead, or the stalker down,
    /// it ends.
    /// (assumption) The task's handler is the AI's (TaskRepertoire::create, F030716, makes the
    /// generic task for 165): the lines' own words are followed.
    pub fn stalking_tick(&mut self) {
        for id in 0..self.folk.len() {
            let Some(t) = self.folk[id].stalk else { continue };
            if self.folk[id].dead || self.folk[id].down || self.folk.get(t).is_none_or(|q| q.dead) {
                self.folk[id].stalk = None;
                continue;
            }
            let at = self.folk[t].mover.position_units();
            if kenshi_world::town::current_town(&self.towns, at, None).is_none() {
                self.folk[id].stalk = None;
                self.tag_short(id, t, kenshi_ai::PerceptionShortTerm::StTemporaryEnemy);
                let squad: Vec<usize> = self.squad_of(id).into_iter().filter(|&k| !self.folk[k].down).collect();
                self.order_attack(&squad, t);
                continue;
            }
            if self.folk[id].escort.is_none_or(|(e, _)| e != t) {
                self.order_follow(&[id], t, false);
            }
        }
    }

    /// The roaming squads' greetings: their sensors' new sightings of the player's characters,
    /// as everyone's (`speech::sighted`, F046547: EV_I_SEE_NEUTRAL_SQUAD); nothing more here.
    pub fn meet_tick(&mut self) {}
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_repetition() {
        assert_eq!(num_text(0), "0");
        assert_eq!(num_text(950), "950");
        assert_eq!(num_text(3000), "3,000");
        assert_eq!(num_text(1234567), "1,234,567");
        assert_eq!(num_text(-2500), "-2,500");
        assert_eq!(repetition_hours(3), 48.0);
        assert!(repetition_hours(5).is_infinite());
        assert_eq!(town_level(0), 0);
        assert_eq!(town_level(5), 2);
    }

    #[test]
    fn unfinished_records() {
        let mut d = GameData::default();
        d.name = "Bar talk (Unfinished)".into();
        assert!(unfinished(&d));
        d.name = "Bar talk".into();
        assert!(!unfinished(&d));
    }
}
