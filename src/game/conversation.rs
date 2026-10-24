//! Conversations played out line by line (port_map `dialogue`): Kenshi's Dialogue (Character
//! +0x280) as it runs an event's dialogue between its speaker and the one it is said to, a line
//! at a time, each line shown over its speaker's head until its time is up.
//!
//! - F040925 (a character's event, `bark`): none from the dead (+0x5bc, F040010), none from the
//!   knocked out or imprisoned but the events their mask lets through (0x141eeaecc); a dialogue
//!   free (F040828) raises it (F040920); one busy with another event (or `forced`) only when
//!   the new event has a line to say: its conversation is ended (F040866) and the new one raised.
//! - F040920 (Dialogue::triggerEvent): none while the event's cooldown (+0xd0, a second after a
//!   sighting: events 3, 4, 27, 66, 67) runs, none from the dead; one in a conversation it does
//!   not lead whose owner is busy is not raised unless the event may cut in (F040811); none
//!   from the knocked out but the mask's; none from one inside a thing that is not a building
//!   (its +0x1d8 handle of another type) but events 39, 6 and 11; a busy dialogue starts none
//!   (EV_PLAYER_TALK_TO_ME aside, the player's window: crate::dialogue); else the event's best
//!   line for the target (F040839 / F040892) opens the conversation (F040917).
//! - F040917 (startConversation): the event, its "far" (+0x149, F040813) and "loud" (+0x14a,
//!   F040811) flags, the target; the speaker turns to it (CharMovement F040605); the speaker
//!   leads (+0x2a0, the target's too); the opening line said (F040914).
//! - F040914 (sayLine): a line of a "monologue" DIALOGUE (or of none) said aloud (F040913 /
//!   F040910: over the head for 2 s + 0.12 s a letter, +0x230, into the log, its effects
//!   F040912); one of a dialogue that is not a monologue the player's window's (the text logged,
//!   its effects, the replies offered: none, it ends).
//! - F040923 (Dialogue::update): the event cooldowns down; a line over its time and its
//!   DA_FORCE_SPEECH_TIMER (+0x234) has the conversation go on: a line of a dialogue not a
//!   monologue ends it; else the line's replies ("lines") go on from its speaker when it was
//!   said by the leader's side (T_ME), else from the leader (F040919); none left, it ends; a
//!   speaker knocked out mid-line ends it (unless its event is the mask's). A dialogue that has
//!   handed on drops its partner (+0x2c0) once that one is free.
//! - F040919 (goOn): of the line's replies the leader's best for its target (F040892), its
//!   speaker by TalkerEnum (F040848) said with the leader and target copied to it; the leader,
//!   when another speaks, hands on (+0x2c0) and ends its own line.
//! - F040866 (endConversation): the event's repetition counted (+0xf8 counter, with the game
//!   time); free; the leader forgotten once it hands on to no one; the line gone; the event
//!   delivered (+0xf8 map, read by isDialogDelivered F040852) once it ends with no partner (or
//!   for an announcement of a squad on an AI contract), the dialogue and event then cleared.
//! - F040924 (attacked while speaking, crate::speech): one busy and not on events 66, 15, 16,
//!   17 or 24 ends and says EV_SPEECH_INTERRUPTED_ATTACKED_BY_TARGET (28) or _BY_STRANGERS (29).
use crate::dialogue::{Conversation, Effect, T_ME};
use crate::sim::Sim;
use kenshi_data::GameData;
use std::collections::BTreeMap;

/// A character's dialogue as it runs a conversation (Kenshi's Dialogue).
#[derive(Debug, Clone)]
pub struct Run {
    /// +0x188: the event of the conversation (0 none).
    pub event: i32,
    /// +0x158: whom it is said to.
    pub target: Option<usize>,
    /// +0x2a0: who leads the conversation (whose replies it goes on with).
    pub owner: Option<usize>,
    /// +0x2c0: the one it handed the conversation on to.
    pub partner: Option<usize>,
    /// +0x190: the DIALOGUE it runs.
    pub root: Option<String>,
    /// +0x198: the line it said last, to go on from.
    pub line: Option<String>,
    /// +0x228: the line over its head (the speech bubble), and +0x230 / +0x234 its time and
    /// DA_FORCE_SPEECH_TIMER's (seconds).
    pub bubble: Option<String>,
    pub timer: f32,
    pub wait: f32,
    /// +0x148: in no conversation.
    pub free: bool,
    /// +0x149 / +0x14a (F040813 / F040811 of the event): the bubble seen from further.
    pub far: bool,
    pub loud: bool,
    /// The interjectors its lines picked (+0x1c8, +0x1e8, +0x208: T_INTERJECTOR1..3).
    pub interjectors: [Option<usize>; 3],
    /// +0xd0: each event's cooldown (seconds).
    pub cooldown: BTreeMap<i32, f32>,
}

impl Default for Run {
    fn default() -> Self {
        Run { event: 0, target: None, owner: None, partner: None, root: None, line: None, bubble: None, timer: 0.0, wait: 0.0, free: true, far: false, loud: false, interjectors: [None; 3], cooldown: BTreeMap::new() }
    }
}

/// F040910: a line stays over its speaker 2 s and 0.12 s a letter (0x141682ad0, 0x1416e1278).
pub const LINE_SECONDS: f32 = 2.0;
pub const LETTER_SECONDS: f32 = 0.12;
/// F040920: a sighting's event waits a second before it is raised again (EV_I_SEE_NEUTRAL_SQUAD
/// 3, 4, EV_I_SEE_ANIMAL_SQUAD 27, EV_I_SEE_ENEMY_PLAYER 66, EV_I_SEE_ALLY_PLAYER 67).
const COOLDOWN_EVENTS: [i32; 5] = [3, 0x43, 0x42, 4, 0x1b];
const COOLDOWN: f32 = 1.0;
/// F040920: the events one inside a thing not a building may still raise.
const INSIDE_EVENTS: [i32; 3] = [0x27, 6, 0xb];
/// F040813 / F040811 (the switches at 0x1406713a0 / 0x1406712f0).
const FAR_EVENTS: [i32; 10] = [2, 11, 17, 26, 31, 34, 36, 40, 45, 74];
const LOUD_EVENTS: [i32; 17] = [2, 6, 11, 13, 17, 26, 30, 31, 34, 36, 40, 45, 46, 48, 51, 54, 74];
/// F040924: the events an attack does not cut short.
const UNINTERRUPTED: [i32; 5] = [0x42, 0xf, 0x10, 0x11, 0x18];
/// EventTriggerEnum EV_SPEECH_INTERRUPTED_ATTACKED_BY_TARGET / _BY_STRANGERS, EV_ANNOUNCEMENT.
const EV_INTERRUPTED_BY_TARGET: i32 = 28;
const EV_INTERRUPTED_BY_STRANGERS: i32 = 29;
const EV_ANNOUNCEMENT: i32 = 2;

fn speaker_enum(l: &GameData) -> i32 {
    l.ints.get("speaker").copied().unwrap_or(T_ME)
}

impl Sim {
    fn run(&self, id: usize) -> Option<&Run> {
        self.runs.get(&id)
    }

    fn run_mut(&mut self, id: usize) -> &mut Run {
        self.runs.entry(id).or_default()
    }

    /// @fn F040828
    ///
    /// Whether `id`'s dialogue is free: in no conversation, no line over its head and no one
    /// handed on to.
    pub fn run_free(&self, id: usize) -> bool {
        self.run(id).is_none_or(|r| r.free && r.bubble.is_none() && r.partner.is_none())
    }

    /// Whether `id` is saying a line (its bubble up).
    pub fn saying(&self, id: usize) -> bool {
        self.run(id).is_some_and(|r| r.bubble.is_some())
    }

    /// F044938: imprisoned (Character +0x3d4) or knocked out.
    fn incapable(&self, id: usize) -> bool {
        let p = &self.folk[id];
        p.down || p.prison.as_ref().is_some_and(|pr| !pr.out)
    }

    fn down_masked(event: i32) -> bool {
        (crate::speech::DOWN_EVENTS >> (event & 31)) & 1 != 0
    }

    /// @fn F040925
    ///
    /// `who` raises `event` at `target` (see the module's notes); `forced` cuts in on the
    /// conversation it is in even with the same event. Whether it spoke.
    pub fn raise_event(&mut self, who: usize, target: Option<usize>, event: i32, forced: bool) -> bool {
        if who >= self.folk.len() || self.folk[who].dead || self.folk[who].away {
            return false;
        }
        if self.folk[who].down && !Self::down_masked(event) {
            return false;
        }
        if self.run_free(who) {
            return self.trigger_event(who, target, event);
        }
        if forced || self.run(who).is_none_or(|r| r.event != event) {
            let to = target.unwrap_or(who);
            if self.pick_opening(who, to, event, &[]).is_some() {
                self.end_run(who);
                return self.trigger_event(who, target, event);
            }
        }
        false
    }

    /// @fn F040920
    ///
    /// Dialogue::triggerEvent (see the module's notes). Whether it spoke.
    pub fn trigger_event(&mut self, who: usize, target: Option<usize>, event: i32) -> bool {
        self.trigger_with(who, target, event, &[])
    }

    /// `trigger_event` with `extra` dialogues listed under the event too (an announcement's).
    fn trigger_with(&mut self, who: usize, target: Option<usize>, event: i32, extra: &[String]) -> bool {
        if self.run(who).is_some_and(|r| r.cooldown.get(&event).is_some_and(|&c| c > 0.0)) || self.folk[who].dead {
            return false;
        }
        // in a conversation another leads, busy: only an event that may cut in
        if self.run(who).is_none_or(|r| r.root.is_none()) {
            let owner = self.run(who).and_then(|r| r.owner).filter(|&o| o < self.folk.len());
            if !LOUD_EVENTS.contains(&event) && owner.is_some_and(|o| !self.run_free(o)) {
                return false;
            }
            self.run_mut(who).owner = Some(who);
        }
        if self.folk[who].down && !Self::down_masked(event) {
            return false;
        }
        // EV_UNLOCK_MY_CAGE_OR_SHACKLES: one freed by whom it does not hold as its captor
        // (its squad's memory, long-term tag 5) remembers the freer as LT_FREED_ME (F040853)
        // and a temporary ally (F040862)
        if event == crate::speech::EV_UNLOCK_MY_CAGE_OR_SHACKLES
            && let Some(t) = target.filter(|&t| t != who && t < self.folk.len())
            && self.squad_tags(who, t).0 & (1 << crate::dialogue::LT_MY_CAPTOR) == 0
        {
            *self.dialog_memory.tags.entry((who, t)).or_default() |= 1 << crate::dialogue::LT_FREED_ME;
            self.tag_short(who, t, kenshi_ai::PerceptionShortTerm::StTemporaryAlly);
        }
        // in a building (Character::insideBuilding, F039242) with no interior (+0x1f0): only
        // these events
        if !INSIDE_EVENTS.contains(&event)
            && let Some(b) = self.folk[who].inside
            && self.placed.get(b).and_then(|pl| self.data.get(&pl.building)).is_none_or(|r| r.refs("interior").is_empty())
        {
            return false;
        }
        if event == crate::dialogue::EV_PLAYER_TALK_TO_ME {
            // the player's window (F040918, crate::dialogue and the UI)
            return target.is_some_and(|t| self.talk_window(who, t));
        }
        if !self.run_free(who) {
            return false;
        }
        // (assumption) with no one it is said to (an announcement), its lines' target
        // conditions read the speaker
        let to = target.unwrap_or(who);
        let Some((d, line, to)) = self.pick_opening(who, to, event, extra) else { return false };
        let target = target.map(|_| to);
        let said = self.start_run(who, target, &d, &line, event);
        if said && COOLDOWN_EVENTS.contains(&event) {
            self.run_mut(who).cooldown.insert(event, COOLDOWN);
        }
        said
    }

    /// @fn F040927
    ///
    /// `who` makes an announcement (EV_ANNOUNCEMENT, to no one): dialogue `extra` (a campaign's
    /// "dialog announcement") among its announcement dialogues; none from the dead, none from
    /// the knocked out (the mask lets EV_ANNOUNCEMENT through); a free dialogue raises it, one
    /// busy with another event only when the announcement has a line: its conversation ended
    /// first. Whether it spoke.
    pub fn announce_run(&mut self, who: usize, extra: Option<&str>) -> bool {
        if who >= self.folk.len() || self.folk[who].dead || self.folk[who].away {
            return false;
        }
        if self.folk[who].down && !Self::down_masked(EV_ANNOUNCEMENT) {
            return false;
        }
        let extra: Vec<String> = extra.into_iter().filter(|d| !self.data.get(*d).is_some_and(crate::dialogue::unfinished)).map(str::to_string).collect();
        if self.run_free(who) {
            return self.trigger_with(who, None, EV_ANNOUNCEMENT, &extra);
        }
        if self.run(who).is_some_and(|r| r.event != EV_ANNOUNCEMENT) && self.pick_opening(who, who, EV_ANNOUNCEMENT, &extra).is_some() {
            self.end_run(who);
            return self.trigger_with(who, None, EV_ANNOUNCEMENT, &extra);
        }
        false
    }

    /// The player's window on `who`'s EV_PLAYER_TALK_TO_ME dialogue with `target` (F040918):
    /// its line, and with replies the conversation the window opens (`npc_talks`); one with
    /// none said aloud.
    fn talk_window(&mut self, who: usize, target: usize) -> bool {
        let Some((mut conv, effects)) = self.talk_on(who, target, crate::dialogue::EV_PLAYER_TALK_TO_ME, true) else { return false };
        if conv.choices.is_empty() {
            let text = std::mem::take(&mut conv.npc_text);
            self.say(who, text);
            if effects.is_empty() {
                return true;
            }
        }
        self.npc_talks.push((conv, effects));
        true
    }

    /// @fn F040917
    ///
    /// `who` opens dialogue `d` at `line` for `event` with `target`. Whether it did.
    pub(crate) fn start_run(&mut self, who: usize, target: Option<usize>, d: &GameData, line: &GameData, event: i32) -> bool {
        let now = self.hours_gone();
        // what the dialogues remember of it (crate::dialogue)
        if let Some(ev) = self.dialog_memory.running_event.remove(&who) {
            *self.dialog_memory.event_count.entry((who, ev)).or_default() += 1;
        }
        self.dialog_memory.running_event.insert(who, event);
        self.dialog_memory.spoken.insert((who, d.string_id.clone()), now);
        if let Some(t) = target {
            self.dialog_memory.met.insert((who, t), now);
        }
        self.dialog_memory.running.insert(who, d.string_id.clone());
        let r = self.run_mut(who);
        r.far = FAR_EVENTS.contains(&event);
        r.loud = r.far || LOUD_EVENTS.contains(&event);
        r.event = event;
        r.root = None;
        r.target = target;
        r.free = false;
        r.line = None;
        r.owner = Some(who);
        r.root = Some(d.string_id.clone());
        // the target's dialogue follows the speaker's lead
        if let Some(t) = target.filter(|&t| t != who) {
            self.run_mut(t).owner = Some(who);
            // F040605: the speaker turns to it
            if !self.folk[who].down {
                let (a, b) = (self.folk[who].mover.position_units(), self.folk[t].mover.position_units());
                let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
                let l = dx.hypot(dz);
                if l > 1e-3 {
                    self.folk[who].facing = [dx / l, 0.0, dz / l];
                }
            }
        }
        if std::env::var("KENSHI_TALK_DEBUG").is_ok() {
            eprintln!("conversation: {who} opens {} with {target:?} (event {event})", d.name);
        }
        self.say_run_line(who, line);
        true
    }

    /// @fn F040914
    /// @fn F040913
    /// @fn F040910
    ///
    /// `who` says `line` in its conversation (see the module's notes).
    pub(crate) fn say_run_line(&mut self, who: usize, line: &GameData) {
        let r = self.run_mut(who);
        r.partner = None;
        r.free = false;
        r.line = Some(line.string_id.clone());
        let target = r.target.unwrap_or(who);
        let root = r.root.clone();
        let monologue = root.as_deref().and_then(|d| self.data.get(d)).is_none_or(|d| d.bools.get("monologue").copied().unwrap_or(false));
        let now = self.hours_gone();
        self.dialog_memory.spoken.insert((who, line.string_id.clone()), now);
        let roll = self.rng.uniform();
        let said = crate::dialogue::text(line, roll);
        let cost = self.line_cost(line);
        let said = self.words(&said, who, target, cost);
        // its effects as its speaker's, said to the speaker's target (F040912)
        self.dialog_memory.partner.insert(who, target);
        let mut effects = self.effects(line, who);
        effects.append(&mut self.pending_effects);
        if std::env::var("KENSHI_TALK_DEBUG").is_ok() {
            eprintln!("conversation: {who} says {said:?} to {target} (monologue {monologue})");
        }
        if monologue {
            // over the head, for its time; logged as said
            let len = said.len() as f32;
            self.say(who, said.clone());
            let r = self.run_mut(who);
            r.bubble = Some(said);
            r.timer = len * LETTER_SECONDS + LINE_SECONDS;
            self.ui_effects(who, target, effects, Vec::new());
            return;
        }
        // the player's window's: the line logged (F040861; the window logs it as it opens),
        // the replies it offers (F040911: a reply of the speaker's own a lone "[...]")
        let said_text = said.clone();
        let passing: Vec<GameData> = line.refs("lines").iter().filter_map(|(sid, _)| self.data.get(sid).cloned()).filter(|l| self.conditions_pass(l, who, target)).collect();
        let mut replies: Vec<(String, String)> = Vec::new();
        for l in passing {
            if speaker_enum(&l) == T_ME {
                replies.push((line.string_id.clone(), "[...]".to_string()));
                break;
            }
            let r = self.rng.uniform();
            let t = crate::dialogue::text(&l, r);
            let c = self.line_cost(&l);
            let said = self.words(&t, target, who, c);
            replies.push((l.string_id.clone(), said));
        }
        if replies.is_empty() || !self.folk[target].player {
            self.logged.push((who, said));
        }
        if replies.is_empty() {
            self.ui_effects(who, target, effects, Vec::new());
            self.end_run(who);
            return;
        }
        self.ui_effects_said(who, target, effects, replies, said_text);
    }

    /// What a line did that the game shows (the trade window, the editor, a reply window) for
    /// one of the player's characters it was said to (crate::dialogue's `Effect`s, the UI's).
    fn ui_effects(&mut self, who: usize, target: usize, effects: Vec<Effect>, choices: Vec<(String, String)>) {
        self.ui_effects_said(who, target, effects, choices, String::new());
    }

    fn ui_effects_said(&mut self, who: usize, target: usize, effects: Vec<Effect>, choices: Vec<(String, String)>, npc_text: String) {
        if target >= self.folk.len() || !self.folk[target].player || (effects.is_empty() && choices.is_empty()) {
            return;
        }
        self.npc_talks.push((Conversation { npc: who, player: target, npc_text, choices }, effects));
    }

    /// @fn F040848
    ///
    /// Who of `owner`'s conversation says a line of TalkerEnum `talker`: T_ME and T_WHOLE_SQUAD
    /// the leader, T_TARGET its target, T_TARGET_IF_PLAYER its target when of the player's,
    /// T_TARGET_WITH_RACE the first of the player's squad (assumption: the target's, the one
    /// on show) of one of the line's "target race", T_INTERJECTOR1..3 the interjectors picked.
    fn speaker_by(&self, owner: usize, talker: i32, line: &GameData) -> Option<usize> {
        let r = self.run(owner)?;
        let target = r.target;
        match talker {
            0 | 6 => Some(owner),
            1 => target,
            2 => target.filter(|&t| self.folk[t].player),
            7 => {
                let t = target?;
                let races: Vec<&String> = line.refs("target race").iter().map(|(s, _)| s).collect();
                self.folk[t].player.then(|| self.squad_of(t)).into_iter().flatten().find(|&k| self.race_of(k).is_some_and(|x| races.contains(&&x)))
            }
            3..=5 => r.interjectors[(talker - 3) as usize],
            _ => None,
        }
    }

    /// @fn F040919
    ///
    /// The conversation `owner` leads goes on from `line` (see the module's notes).
    fn go_on(&mut self, owner: usize, line: &GameData) {
        let said_to = self.run(owner).and_then(|r| r.target);
        let target = said_to.unwrap_or(owner);
        // said to no one: no line of the target's (F040848 finds no speaker for them)
        let refs: Vec<(String, [i32; 3])> = line.refs("lines").iter().filter(|(sid, _)| said_to.is_some() || self.data.get(sid).is_none_or(|l| !matches!(speaker_enum(l), 1 | 2 | 7))).cloned().collect();
        let next = self.choose_line(&refs, owner, target);
        let Some(next) = next else {
            self.end_run(owner);
            return;
        };
        let talker = speaker_enum(&next);
        if (3..=5).contains(&talker) {
            let k = self.pick_interjector(&next, owner, target);
            self.run_mut(owner).interjectors[(talker - 3) as usize] = k;
        }
        let Some(speaker) = self.speaker_by(owner, talker, &next) else {
            self.end_run(owner);
            return;
        };
        // the speaker takes the leader and the leader's target
        let (lead, root) = {
            let r = self.run_mut(owner);
            if r.owner.is_none() {
                r.owner = Some(owner);
            }
            (r.owner, r.root.clone())
        };
        let event = self.run(owner).map_or(0, |r| r.event);
        {
            // (its own dialogue and event stay: one with none says the line aloud)
            let s = self.run_mut(speaker);
            s.owner = lead;
            s.target = Some(target);
        }
        let _ = (root, event);
        self.say_run_line(speaker, &next);
        if speaker == owner {
            return;
        }
        self.run_mut(owner).partner = Some(speaker);
        self.end_run(owner);
    }

    /// @fn F040866
    ///
    /// `who`'s conversation ends (see the module's notes).
    pub fn end_run(&mut self, who: usize) {
        let now = self.hours_gone();
        let Some(r) = self.runs.get_mut(&who) else { return };
        if r.root.is_some() {
            // the event's repetitions (Dialogue::RepetitionCounter: its count and game time)
            let ev = r.event;
            *self.dialog_memory.event_count.entry((who, ev)).or_default() += 1;
            self.dialog_memory.event_time.insert((who, ev), now);
            if self.dialog_memory.running_event.get(&who) == Some(&ev) {
                self.dialog_memory.running_event.remove(&who);
            }
        }
        let r = self.runs.get_mut(&who).expect("run");
        r.free = true;
        let alone = r.partner.is_none();
        if alone {
            r.owner = None;
        }
        r.bubble = None;
        r.timer = 0.0;
        let ev = r.event;
        // delivered (+0xf8: the event, by its squad): ended with no one handed on to, or an
        // announcement of a squad on an AI contract (+0x100 of its platoon's AI)
        let contract = self.contracts.iter().any(|c| c.members.contains(&who));
        if alone || (contract && ev == EV_ANNOUNCEMENT) {
            self.dialog_memory.delivered.insert((who, ev));
            // (isDialogDelivered reads it at once: the talk tasks' view too, crate::talk)
            if alone {
                self.talk_view.delivered.insert((who, ev));
            }
            let r = self.runs.get_mut(&who).expect("run");
            r.root = None;
            r.event = 0;
        }
        let r = self.runs.get_mut(&who).expect("run");
        r.line = None;
        if std::env::var("KENSHI_TALK_DEBUG").is_ok() {
            eprintln!("conversation: {who} ends (event {ev}, alone {alone})");
        }
    }

    /// @fn F040924
    ///
    /// `who`, busy talking, is attacked by `attacker`: unless its event is one an attack does
    /// not cut short, its conversation ends and it says EV_SPEECH_INTERRUPTED_ATTACKED_BY_TARGET
    /// (attacked by the one it talks to) or _BY_STRANGERS.
    pub fn interrupted(&mut self, who: usize, attacker: usize) {
        let Some(r) = self.run(who) else { return };
        if r.free || UNINTERRUPTED.contains(&r.event) {
            return;
        }
        let by_target = r.target == Some(attacker);
        self.end_run(who);
        self.trigger_event(who, Some(attacker), if by_target { EV_INTERRUPTED_BY_TARGET } else { EV_INTERRUPTED_BY_STRANGERS });
    }

    /// @fn F040923
    ///
    /// Every tick each dialogue's update (its frame time, Character +0xc4): see the module's
    /// notes.
    pub fn runs_tick(&mut self, dt: f32) {
        let ids: Vec<usize> = self.runs.keys().copied().collect();
        for who in ids {
            if who >= self.folk.len() {
                self.runs.remove(&who);
                continue;
            }
            let Some(r) = self.runs.get_mut(&who) else { continue };
            for c in r.cooldown.values_mut() {
                if *c > 0.0 {
                    *c -= dt;
                }
            }
            if r.bubble.is_some() {
                let event = r.event;
                if self.incapable(who) && !Self::down_masked(event) {
                    self.end_run(who);
                    continue;
                }
                let r = self.runs.get_mut(&who).expect("run");
                r.timer -= dt;
                r.wait -= dt;
                if r.timer >= 0.0 || r.wait >= 0.0 {
                    continue;
                }
                let root = r.root.clone();
                let line = r.line.clone();
                let owner = r.owner;
                let monologue = root.as_deref().and_then(|d| self.data.get(d)).is_none_or(|d| d.bools.get("monologue").copied().unwrap_or(false));
                if root.is_some() && !monologue {
                    self.end_run(who);
                    continue;
                }
                let Some(line) = line.and_then(|l| self.data.get(&l).cloned()) else {
                    self.end_run(who);
                    continue;
                };
                if line.refs("lines").is_empty() {
                    self.end_run(who);
                    continue;
                }
                if speaker_enum(&line) == T_ME {
                    self.go_on(who, &line);
                    continue;
                }
                // a reply: the leader goes on with it
                let owner = owner.filter(|&o| o < self.folk.len() && !self.folk[o].dead);
                self.end_run(who);
                if let Some(o) = owner {
                    self.go_on(o, &line);
                }
                continue;
            }
            // handed on: the partner dropped once it is free
            if let Some(p) = r.partner
                && (p >= self.folk.len() || self.run_free(p))
            {
                self.runs.get_mut(&who).expect("run").partner = None;
            }
        }
    }
}
