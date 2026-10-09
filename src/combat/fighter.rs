//! A fighter's combat AI (CombatClass / CombatClassAI): the per-tick update that runs the state
//! machine, and the attack and block states' ticks, in the original's control flow.
//!
//! Everything outside combat (finding targets, the character's animation actions, movement
//! orders, other fighters' combat state, hit resolution) goes through [`Fighting`], which the game
//! implements. Field comments give the CombatClass offsets.
//!
//! Specs: sheet rows F039852 CombatClassAI::update, F039846 CombatClass::updateCommon, F039805
//! AttackState::tick, F039789 CombatClass::switchToBlock, F039802 BlockState::tick, F039801
//! CombatClass::updateBlockAction, F040722 CombatClass::swingProgress, F040723
//! CombatClass::nearestThreat, F040728 CombatClass::faceAttacker, F033810 Character::isStaggered.

use crate::SwordState;
use crate::technique::CombatTechnique;

/// @type CombatClass
///
/// The combat state of one fighter.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatClass<Id> {
    /// +0x1f0
    pub state: SwordState,
    /// +0x1f4: the state wanted behind an engaged state (SwordState value, 2 = no target).
    pub desired: i32,
    /// +0x120: defending (someone is attacking me).
    pub defending: bool,
    /// +0x124: cooldown of the block roll.
    pub block_cooldown: f32,
    /// +0x140: my swing's progress toward its next impact (F040722).
    pub swing_progress: f32,
    /// +0x144: waiting out a hit or a finished move.
    pub waiting: bool,
    /// +0x148: how long the wait lasts.
    pub recovery: f32,
    /// +0x14c: reaction timer.
    pub reaction: f32,
    /// +0x1a8: the attacker I am reacting to.
    pub threat: Option<Id>,
    /// +0x1d0: that attacker's frame I last chose a block for.
    pub block_frame: i32,
    /// +0x1d4: this action's swing targets were collected.
    pub collected: bool,
    /// +0x220 / +0x228: fighters attacking me.
    pub attackers: Vec<Id>,
    /// +0x268 / +0x26c: the current action's frame index and the last struck.
    pub frame: i32,
    pub struck: i32,
    /// +0x274: block re-check timer.
    pub block_timer: f32,
    /// +0x278: my combat radius (collision radius * 1.5 * size).
    pub radius: f32,
    /// +0x27c: reach margin ((collision radius * 1.5 + weapon length) * size).
    pub reach_margin: f32,
    /// +0x290: target.
    pub target: Option<Id>,
    /// +0x2f0: age (seconds since the last target pick, used by findTarget).
    pub age: f64,
    /// +0x1f8 (count +0x200): the fighters engaging me, at most 5.
    pub engagers: Vec<Id>,
    /// +0x210 / +0x214: fighters turned away (engagers full), counted per tick in alternate
    /// slots chosen by +0x218.
    pub turned_away: [u32; 2],
    pub flip: bool,
}

impl<Id> Default for CombatClass<Id> {
    fn default() -> Self {
        CombatClass {
            state: SwordState::Idle,
            desired: 5,
            defending: false,
            block_cooldown: 0.0,
            swing_progress: 0.0,
            waiting: false,
            recovery: 0.0,
            reaction: 0.0,
            threat: None,
            block_frame: -1,
            collected: false,
            attackers: Vec::new(),
            frame: 0,
            struck: 0,
            block_timer: 0.0,
            radius: 0.0,
            reach_margin: 0.0,
            target: None,
            age: 0.0,
            engagers: Vec::new(),
            turned_away: [0; 2],
            flip: false,
        }
    }
}

/// What the game provides to a fighter's combat AI.
pub trait Fighting {
    type Id: Copy + PartialEq;
    /// Knocked out, dying or otherwise down (F044938).
    fn is_down(&self, who: Self::Id) -> bool;
    /// An animal (Character vtable 0x248 set).
    fn is_animal(&self) -> bool;
    /// A stumble is playing (F033810).
    fn staggered(&self) -> bool;
    /// The combat collision radius (CombatClass +0x1a0), size (vtable 0x390), weapon length
    /// (CharStats +0x200).
    fn collision_radius(&self) -> f32;
    fn size(&self) -> f32;
    fn weapon_length(&self) -> f32;
    /// The current action (CombatClass +0x150), its normalised progress (F038894) and whether it
    /// still plays (F038889).
    fn action(&self) -> Option<&CombatTechnique>;
    fn action_progress(&self) -> f32;
    fn action_playing(&self) -> bool;
    /// The action layer's own clip progress (F038855), for the stop frame while defending.
    fn action_clip_progress(&self) -> f32;
    /// Stop the current action (F038875).
    fn stop_action(&mut self);
    /// Another fighter's swing progress at me (F040709) and whether its action is a block.
    fn progress_at_me(&self, who: Self::Id) -> f32;
    fn is_blocking(&self, who: Self::Id) -> bool;
    /// The frame index of another fighter's action (its +0x268).
    fn frame_of(&self, who: Self::Id) -> i32;
    /// Pick a target (F040734).
    fn find_target(&mut self) -> Option<Self::Id>;
    /// The fighters around me that may be attacking me (CombatClass +0x208 and the +0x260
    /// registrations): each with whether it targets me and its swing progress (+0x140).
    fn candidates(&self) -> Vec<(Self::Id, bool, f32)>;
    /// The idle state's tick (F039837), the engaged step (F039848), the circle (F029599) and
    /// waiting (F029602) states' ticks: the parts already ported elsewhere, run by the game.
    fn idle(&mut self, c: &mut CombatClass<Self::Id>);
    fn engaged(&mut self, c: &mut CombatClass<Self::Id>);
    fn circle(&mut self, c: &mut CombatClass<Self::Id>, dt: f32);
    fn wait(&mut self, c: &mut CombatClass<Self::Id>, dt: f32);
    /// The commit point of my wind-up against `target` (F039791 with its defence and crowd).
    fn commit_point(&self, target: Self::Id) -> f32;
    /// Collect swing targets (F039793) and strike them with frame `frame` (F039803).
    fn strike(&mut self, c: &mut CombatClass<Self::Id>, frame: i32);
    /// Tell the target I stopped attacking it (F040720) / am still attacking it (F039813).
    fn leave_attack(&mut self, target: Self::Id);
    fn keep_attacking(&mut self, target: Self::Id);
    /// Movement: face and close on a fighter during the wind-up (F029534 + goTo), stop (vtable
    /// 0x98), hold a band around a fighter (F029541).
    fn close_on(&mut self, who: Self::Id, far: bool);
    fn stop_moving(&mut self);
    /// reactToAttack (F039797): choose and play a block or dodge against the nearest threat;
    /// true when it found one (`dodging`: a dodge is under way).
    fn react(&mut self, c: &mut CombatClass<Self::Id>, dodging: bool) -> bool;
    /// The block direction `threat`'s current frame needs from me (F033831 with F033811).
    fn needed_block(&self, threat: Self::Id) -> i32;
    /// Replay the current block or dodge so it is held (F038914 at the defence speed CharStats
    /// +0x18c, 1 for a dodge, times the technique's speed).
    fn hold_block(&mut self);
    /// The attack skill of another fighter (F047043), my block chance against it (F047056) and
    /// defence training (F047459).
    fn attack_skill(&self, who: Self::Id) -> f32;
    fn block_chance(&self, skill: f32) -> f32;
    fn train_defence(&mut self, skill: f32, blocked: bool);
    /// A uniform roll in [0, 1) (F050500).
    fn rand01(&mut self) -> f32;
}

impl<Id: Copy + PartialEq> CombatClass<Id> {
    /// @fn F039841
    ///
    /// Before each update: switch the turned-away counter to the other slot and clear it.
    pub fn begin_tick(&mut self) {
        self.flip = !self.flip;
        if self.flip {
            self.turned_away[0] = 0;
        } else {
            self.turned_away[1] = 0;
        }
    }

    /// @fn F040717
    ///
    /// Whether `who` is one of my engagers (the original's 'reachable' for the state entry
    /// checks: the asker is among its target's engagers).
    pub fn is_engager(&self, who: Id) -> bool {
        self.engagers.contains(&who)
    }

    /// @fn F040733
    ///
    /// May `who` engage me: yes if it already does; with 5 engagers no (and it counts as turned
    /// away this tick); else it joins them.
    pub fn accept_engager(&mut self, who: Id) -> bool {
        if self.is_engager(who) {
            return true;
        }
        if 4 < self.engagers.len() {
            self.turned_away[if self.flip { 0 } else { 1 }] += 1;
            return false;
        }
        self.engagers.push(who);
        true
    }

    /// The crowd on me: engagers plus last tick's turned-away count (+0x200 + the other slot).
    pub fn crowd(&self) -> u32 {
        self.engagers.len() as u32 + self.turned_away[if self.flip { 1 } else { 0 }]
    }

    /// @fn F040722
    ///
    /// My swing's progress toward its next impact (+0x140): -100 when down, -1 locked; while
    /// attacking with a strike (not a block), the action's progress divided by the impact time
    /// of the current frame (1 once past the last frame); else 0.
    pub fn swing_progress<W: Fighting<Id = Id>>(&self, w: &W, me: Id) -> f32 {
        if w.is_down(me) {
            return -100.0;
        }
        if self.state == SwordState::Locked {
            return -1.0;
        }
        let attacking = self.state == SwordState::Attack || self.desired == 0;
        match w.action() {
            Some(t) if attacking && !t.is_block => {
                let p = w.action_progress();
                if self.frame < t.frames.len() as i32 {
                    let impact = t.frame(self.frame).map_or(1.0, |f| f.impact);
                    (1.0 / impact) * p
                } else {
                    1.0
                }
            }
            _ => 0.0,
        }
    }

    /// @fn F040723
    ///
    /// The attacker whose swing at me is furthest along (at least `from`, below 1).
    pub fn nearest_threat<W: Fighting<Id = Id>>(&self, w: &W, from: f32) -> Option<Id> {
        let mut best = None;
        let mut at = from;
        for &a in &self.attackers {
            if w.is_down(a) {
                continue;
            }
            let p = w.progress_at_me(a);
            if at <= p && 0.0 < p && p < 1.0 {
                best = Some(a);
                at = p;
            }
        }
        best
    }

    /// @fn F039789
    ///
    /// While my own swing is early (progress below `early`), switch to blocking when an
    /// attacker's swing at me is at least `threat` along.
    pub fn switch_to_block<W: Fighting<Id = Id>>(
        &mut self,
        w: &W,
        early: f32,
        threat: f32,
    ) -> bool {
        if w.is_animal() || early <= self.swing_progress {
            return false;
        }
        if self.nearest_threat(w, threat).is_some() {
            self.state = SwordState::Engaged;
            self.desired = 1;
            return true;
        }
        false
    }

    /// @fn F039791
    ///
    /// How far into a wind-up a fighter keeps closing on its target: the larger of 0.6 *
    /// invlerp(target defence level (F047060) - my attack skill (F047043), 20, 0) and
    /// invlerp(the target's crowd, 4, 1) (at least 0.1).
    pub fn commit_point(target_defence: f32, my_attack: f32, target_crowd: u32) -> f32 {
        use kenshi_char::medical::invlerp;
        let mut a = invlerp(target_crowd as f32, 4.0, 1.0);
        if a < 0.1 {
            a = 0.1;
        }
        let b = (invlerp(target_defence - my_attack, 20.0, 0.0) as f64 * 0.6) as f32;
        if a < b { b } else { a }
    }

    /// @fn F039805
    ///
    /// The attack state's tick: strike each frame as the action passes its impact, stop at the
    /// end (or at the frame's stop time while defending), switch to blocking when someone is
    /// about to strike me first.
    pub fn attack_tick<W: Fighting<Id = Id>>(&mut self, w: &mut W, distance: f32) -> bool {
        let Some(target) = self.target else {
            self.waiting = true;
            return false;
        };
        if self.switch_to_block(w, 0.5, 0.5) {
            self.defending = true;
            self.desired = 1;
        }
        self.defending = !self.attackers.is_empty();
        let progress = if w.action_playing() {
            w.action_progress()
        } else {
            1.01
        };
        // frames whose impact the action has passed
        let n = w.action().map_or(0, |t| {
            t.frames.iter().filter(|f| f.impact < progress).count()
        });
        self.frame = n as i32;
        if self.struck < self.frame {
            let f = self.struck;
            w.strike(self, f);
            if w.action().is_none() {
                w.leave_attack(target);
                return true;
            }
        }
        self.struck = self.frame;
        let last = w.action().map_or(0, |t| t.frames.len() as i32) - 1;
        if last < self.frame {
            self.frame = last;
            w.leave_attack(target);
        } else if self.frame == 0 && self.swing_progress < w.commit_point(target) && 8.0 < distance
        {
            // still winding up: keep closing on the target
            w.close_on(target, 5.0 < distance);
        }
        if progress < 0.0 {
            self.set_state(w, SwordState::Idle);
        }
        if self.defending {
            let clip = w.action_clip_progress();
            let stop = w
                .action()
                .and_then(|t| t.frame(self.frame.min(last).max(0)))
                .map_or(1.0, |f| f.stop);
            if stop <= clip {
                self.set_state(w, SwordState::Idle);
            }
        }
        if self.reaction <= 0.0 {
            if w.action_playing() {
                w.keep_attacking(target);
            } else if !self.waiting {
                self.waiting = true;
                w.stop_moving();
            }
        }
        true
    }

    /// @fn F039802
    ///
    /// The block state's tick: keep (or re-choose) the block against the most pressing attacker.
    pub fn block_tick<W: Fighting<Id = Id>>(&mut self, w: &mut W, dodging: bool, dt: f32) -> bool {
        if w.action().is_none() {
            self.set_state(w, SwordState::Idle);
            return false;
        }
        let dodging = dodging || w.action().is_some_and(|t| t.is_dodge);
        self.update_block_action(w, dodging, dt);
        if w.is_animal() && self.reaction <= 0.0 {
            self.set_state(w, SwordState::Idle);
        }
        true
    }

    /// @fn F039801
    ///
    /// Re-check the block when the threat or its frame changed or the re-check timer (+0x274)
    /// ran out: a threat that is itself blocking means waiting; a block whose direction already
    /// answers the threat's frame is kept, else reactToAttack chooses again. A kept block is held
    /// by replaying it; with nobody attacking, the fighter waits.
    pub fn update_block_action<W: Fighting<Id = Id>>(&mut self, w: &mut W, dodging: bool, dt: f32) {
        if self.threat.is_none() {
            self.threat = self.nearest_threat(w, 0.0);
        }
        if w.is_animal() {
            return;
        }
        let Some(old) = self.threat else {
            if !self.waiting {
                self.recovery = 0.2; // F047022 kind 2
            }
            self.waiting = true;
            w.stop_moving();
            return;
        };
        if w.action().is_none() {
            w.react(self, self.defending);
            return;
        }
        if let Some(t) = self.nearest_threat(w, 0.0)
            && t != old
        {
            self.threat = Some(t);
        }
        let threat = self.threat.unwrap_or(old);
        // F040728: face the attacker
        self.block_timer -= dt;
        let frame = w.frame_of(threat);
        let recheck = threat != old || frame != self.block_frame || self.block_timer < 0.0;
        if recheck {
            let my_block = w.action().and_then(|t| t.frame(0)).map(|f| f.direction);
            if w.is_blocking(threat) {
                // the attacker is blocking, not striking: wait
                if !self.waiting && !dodging {
                    self.recovery = 0.2;
                    self.waiting = true;
                    w.stop_moving();
                }
            } else if !w.action().is_some_and(|t| t.is_dodge)
                && my_block != Some(w.needed_block(threat))
                && !w.react(self, dodging)
            {
                self.set_state(w, SwordState::Idle);
                return;
            }
            self.block_timer = 1.0; // F047062 reaction factor
        }
        let holds = w
            .action()
            .is_some_and(|t| !t.is_dodge || w.action_progress() <= 0.98);
        if !holds {
            self.set_state(w, SwordState::Idle);
            return;
        }
        w.hold_block();
        if self.attackers.is_empty() && !self.waiting && !dodging {
            self.recovery = 0.2;
            self.waiting = true;
            w.stop_moving();
        }
        self.block_frame = frame;
    }

    /// @fn F039846
    ///
    /// The update shared by every fighter: reach margins, the reaction timer, and while locked
    /// the block roll; then the attack (0) or block (1) state's tick.
    pub fn update_common<W: Fighting<Id = Id>>(&mut self, w: &mut W, dt: f32, distance: f32) {
        self.collected = false;
        let r = (w.collision_radius() as f64 * 1.5) as f32;
        self.radius = r * w.size();
        self.reach_margin = (r + w.weapon_length()) * w.size();
        self.reaction -= dt;
        match self.state {
            SwordState::Locked => {
                // F039792: while locked (staggered), roll to block the strongest attacker
                // whose swing at me is 10..99% along
                self.block_cooldown -= dt;
                if !w.staggered() {
                    self.state = SwordState::Idle;
                    return;
                }
                if self.defending && self.block_cooldown < 0.0 {
                    let mut best: Option<(f32, f32)> = None;
                    for &a in &self.attackers {
                        let p = w.progress_at_me(a);
                        let s = w.attack_skill(a);
                        if (0.1..0.99).contains(&p) && best.is_none_or(|(bs, _)| bs < s) {
                            best = Some((s, p));
                        }
                    }
                    if let Some((skill, _)) = best {
                        let roll = w.rand01() * 100.0;
                        if w.block_chance(skill) <= roll {
                            w.train_defence(skill, false);
                            self.block_cooldown = 1.2;
                        } else {
                            self.desired = 1;
                            self.state = SwordState::Engaged;
                            self.reaction = -1.0;
                        }
                    }
                }
            }
            SwordState::Attack => {
                self.attack_tick(w, distance);
            }
            SwordState::Block => {
                self.block_tick(w, false, dt);
            }
            _ => {}
        }
    }

    /// @fn F039842
    pub fn set_state<W: Fighting<Id = Id>>(&mut self, w: &mut W, to: SwordState) {
        let acting = w.action().is_some();
        if crate::states::set_state(&mut self.state, to, acting) {
            w.stop_action();
        }
    }

    /// @fn F040735
    ///
    /// Rebuild the list of fighters attacking me (+0x220): those targeting me whose swing is
    /// under way (progress strictly between 0 and 1), each once.
    pub fn refresh_attackers(&mut self, candidates: &[(Id, bool, f32)]) {
        self.attackers.clear();
        for &(who, targets_me, progress) in candidates {
            if targets_me && 0.0 < progress && progress < 1.0 && !self.attackers.contains(&who) {
                self.attackers.push(who);
            }
        }
    }

    /// @fn F039852
    ///
    /// One tick: drop a fallen target, count a pending wait down (waiting state 6 meanwhile),
    /// run idle, the shared update, then the engaged / circle / waiting states.
    pub fn update<W: Fighting<Id = Id>>(&mut self, w: &mut W, me: Id, dt: f32, distance: f32) {
        self.age += dt as f64;
        let candidates = w.candidates();
        self.refresh_attackers(&candidates);
        self.swing_progress = self.swing_progress(w, me);
        if let Some(t) = self.target
            && w.is_down(t)
        {
            self.target = None;
            if self.state != SwordState::Locked {
                if w.action().is_some() {
                    w.stop_action();
                }
                self.state = SwordState::Idle;
            }
        }
        if self.waiting && self.state != SwordState::TargetLost {
            if self.state != SwordState::Locked {
                self.state = SwordState::Waiting;
            }
            if !w.action_playing() {
                self.recovery -= dt;
            }
            if self.recovery < 0.0 {
                if self.state != SwordState::Locked {
                    if w.action().is_some() {
                        w.stop_action();
                    }
                    self.state = SwordState::Idle;
                }
                self.waiting = false;
            }
        }
        if self.state == SwordState::Idle {
            if self.target.is_none() {
                self.target = w.find_target();
            }
            w.idle(self);
        }
        self.update_common(w, dt, distance);
        match self.state {
            SwordState::Engaged => w.engaged(self),
            SwordState::Locked => self.defending = !self.attackers.is_empty(),
            SwordState::Waiting => w.wait(self, dt),
            SwordState::Circle => w.circle(self, dt),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engagers_and_the_crowd() {
        let mut c: CombatClass<u32> = CombatClass::default();
        c.begin_tick();
        for who in 0..5 {
            assert!(c.accept_engager(who));
        }
        assert!(c.accept_engager(3));
        assert!(!c.accept_engager(9));
        assert!(!c.accept_engager(10));
        // the turned-away count shows from the next tick
        assert_eq!(c.crowd(), 5);
        c.begin_tick();
        assert_eq!(c.crowd(), 7);
        c.begin_tick();
        assert_eq!(c.crowd(), 5);
    }

    #[test]
    fn wind_up_commit() {
        // an even match, one on one: chase through the whole wind-up
        assert_eq!(CombatClass::<u32>::commit_point(40.0, 40.0, 1), 1.0);
        // a better defender in a crowd of 4: stop chasing early
        assert!((CombatClass::<u32>::commit_point(60.0, 40.0, 4) - 0.1).abs() < 1e-6);
        // a much weaker defender: keep closing past the impact time
        assert!((CombatClass::<u32>::commit_point(20.0, 40.0, 1) - 1.2).abs() < 1e-6);
        let mut c: CombatClass<u32> = CombatClass {
            attackers: vec![1],
            ..Default::default()
        };
        c.refresh_attackers(&[(1, true, 0.5), (2, false, 0.5), (3, true, 1.0)]);
        assert_eq!(c.attackers, vec![1]);
    }
}
