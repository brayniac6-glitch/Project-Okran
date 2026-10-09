//! Subjective tags: what one platoon thinks of another character (intruder, aggressor, temporary
//! enemy, criminal, ...), kept per platoon.
//!
//! Specs: sheet rows F039305 TagsClass::set, F039306 TagsClass::has, F040830 / F040829
//! SubjectiveTags::setShort / setLong, F040862 PlatoonMemory::addTag, F040863 Character::tag;
//! types T00045 SubjectiveTags, T00046 PlatoonMemory, T00041 / T00042 the tag enums.

use crate::{PerceptionLongTerm, PerceptionShortTerm};
use std::collections::HashMap;
use std::hash::Hash;

/// A set of tags as one bitmask (TagsClass).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TagSet(pub u32);

impl TagSet {
    /// @fn F039305
    pub fn set(&mut self, tag: i32) {
        self.0 |= 1 << tag;
    }

    /// @fn F039306
    pub fn has(&self, tag: i32) -> bool {
        self.0 & (1 << tag) != 0
    }
}

/// @type SubjectiveTags
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SubjectiveTags {
    pub long_term: TagSet,
    pub short_term: TagSet,
    /// When a tag was last set (seconds of game clock).
    pub time: f64,
}

impl SubjectiveTags {
    /// @fn F040830
    ///
    /// `clock` is the game clock (microseconds).
    pub fn set_short(&mut self, tag: PerceptionShortTerm, clock: f64) {
        self.short_term.set(tag as i32);
        self.time = clock * 1e-6;
    }

    /// @fn F040829
    pub fn set_long(&mut self, tag: PerceptionLongTerm, clock: f64) {
        self.long_term.set(tag as i32);
        self.time = clock * 1e-6;
    }
}

/// Who a memory entry is about: a character, or a whole (player) squad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject<C, S> {
    Character(C),
    Squad(S),
}

/// @type PlatoonMemory
#[derive(Debug, Clone, Default)]
pub struct PlatoonMemory<C: Eq + Hash, S: Eq + Hash> {
    pub entries: HashMap<Subject<C, S>, SubjectiveTags>,
    /// Set when a player squad is tagged a temporary ally (+0x28).
    pub has_temporary_ally: bool,
}

impl<C: Eq + Hash + Copy, S: Eq + Hash + Copy> PlatoonMemory<C, S> {
    /// @fn F040862
    ///
    /// Tag `subject`; `own_platoon` is whether it belongs to this memory's platoon, `player_squad`
    /// its squad when that is a player squad (then the whole squad is tagged, and the member's
    /// own entry is stamped too).
    pub fn add_tag(
        &mut self,
        subject: C,
        own_platoon: bool,
        player_squad: Option<S>,
        tag: PerceptionShortTerm,
        clock: f64,
    ) {
        if own_platoon && tag == PerceptionShortTerm::StTemporaryEnemy {
            return;
        }
        let key = player_squad.map_or(Subject::Character(subject), Subject::Squad);
        self.entries.entry(key).or_default().set_short(tag, clock);
        if player_squad.is_some() {
            if tag == PerceptionShortTerm::StTemporaryAlly {
                self.has_temporary_ally = true;
            }
            if let Some(e) = self.entries.get_mut(&Subject::Character(subject)) {
                e.time = clock * 1e-6;
            }
        }
    }

    /// The tags this platoon keeps on `subject` (or its player squad).
    pub fn tags(&self, subject: C, player_squad: Option<S>) -> Option<&SubjectiveTags> {
        let key = player_squad.map_or(Subject::Character(subject), Subject::Squad);
        self.entries.get(&key)
    }
}

/// @fn F040863
///
/// Whether a character's tag on `other` reaches its platoon's memory: a temporary-enemy tag is
/// dropped when `other`'s squad is allied with mine or `other` is my ally.
pub fn should_tag(tag: PerceptionShortTerm, allied_squads: bool, ally: bool) -> bool {
    !(tag == PerceptionShortTerm::StTemporaryEnemy && (allied_squads || ally))
}

#[cfg(test)]
mod tests {
    use super::*;
    use PerceptionShortTerm::*;

    #[test]
    fn tags_are_bits() {
        let mut t = TagSet::default();
        t.set(StCriminal as i32);
        assert!(t.has(7) && !t.has(4));
    }

    #[test]
    fn platoons_remember_who_attacked_them() {
        let mut m: PlatoonMemory<u32, u32> = PlatoonMemory::default();
        m.add_tag(7, false, None, StTemporaryEnemy, 5_000_000.0);
        let e = m.tags(7, None).unwrap();
        assert!(e.short_term.has(StTemporaryEnemy as i32));
        assert_eq!(e.time, 5.0);
        // never a temporary enemy of your own platoon
        m.add_tag(8, true, None, StTemporaryEnemy, 0.0);
        assert!(m.tags(8, None).is_none());
        // player squads are remembered as a whole
        m.add_tag(9, false, Some(100), StIntruder, 0.0);
        assert!(
            m.tags(42, Some(100))
                .unwrap()
                .short_term
                .has(StIntruder as i32)
        );
        assert!(!should_tag(StTemporaryEnemy, true, false));
        assert!(should_tag(StCriminal, true, true));
    }
}
