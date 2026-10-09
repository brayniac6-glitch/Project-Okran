//! AI packages (port_map `ai`): the goals an AI_PACKAGE gives each member of a squad, and the
//! "AI Goals" a character or faction record carries.
//!
//! Spec: F028594 AIPackage::apply (the role lists, "inherits from" first), F028593
//! AIPackage::addGoals (per AI_TASK reference: targeting, ending, classification, the hours and
//! the weight), F028559 ActivePlatoon::addRoleGoal (role lists), F028583 rebuild (a member's job
//! list of one classification from its role list, F036925), F036828 addGoalsFromRecord.
//!
//! A package reference's values are (start hour, end hour, weight): the goal is a job of its
//! AI_TASK classification, taken only within its hours, its priority the weight / 100 (1.0 when
//! the weight is 0). A record's own "AI Goals" are (0, priority %, 0): always on.

use kenshi_data::{GameData, GameDataContainer, ItemType};

/// A squad member's role: which package list it takes (F028594 / F028559).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Squad = 0,
    Squad2 = 1,
    Leader = 2,
    Slave = 4,
}

impl Role {
    pub fn list(self) -> &'static str {
        match self {
            Role::Squad => "Squad AI Goals",
            Role::Squad2 => "Squad2 AI Goals",
            Role::Leader => "Leader AI Goals",
            Role::Slave => "Slave AI Goals",
        }
    }
}

/// One goal a package or record gives.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageGoal {
    /// AI_TASK "enum".
    pub task: u16,
    /// AI_TASK "classification": 1 fluff, 2 non-urgent, 3 urgent.
    pub classification: u8,
    pub priority: f32,
    /// (start, end) hour; (0, 100) for always.
    pub hours: (i32, i32),
    /// AI_TASK "targeting": 0 specific, 1 self, 2 leader, 3 squad mission, 4 home gate
    /// (TaskTargetType; F028593 resolves 2, 3 and 4).
    pub targeting: i32,
    /// AI_TASK "ending" (TaskEndEvent).
    pub ending: i32,
    /// The AI_TASK record's name.
    pub name: String,
}

fn task(data: &GameDataContainer, sid: &str) -> Option<(u16, u8, i32, i32, String)> {
    let t = data
        .get(sid)
        .filter(|t| t.item_type() == Some(ItemType::AiTask))?;
    let i = |k: &str| *t.ints.get(k).unwrap_or(&0);
    Some((
        i("enum") as u16,
        i("classification") as u8,
        i("targeting"),
        i("ending"),
        t.name.clone(),
    ))
}

/// @fn F028593
///
/// The goals a package gives a role: its "inherits from" packages' first (F028594), then its
/// "AI Goals" (every role but slaves), then its own list for the role.
pub fn package_goals(data: &GameDataContainer, package: &GameData, role: Role) -> Vec<PackageGoal> {
    let mut out = vec![];
    collect(data, package, role, &mut out, 0);
    out
}

fn collect(
    data: &GameDataContainer,
    p: &GameData,
    role: Role,
    out: &mut Vec<PackageGoal>,
    depth: u32,
) {
    if depth < 8 {
        for (sid, _) in p.refs("inherits from") {
            if let Some(parent) = data.get(sid) {
                collect(data, parent, role, out, depth + 1);
            }
        }
    }
    // the package's own "AI Goals" go to every role but slaves (F028594: the squad, squad2 and
    // leader lists), before the role's list
    let common: &[(String, [i32; 3])] = if role == Role::Slave {
        &[]
    } else {
        p.refs("AI Goals")
    };
    for (sid, v) in common.iter().chain(p.refs(role.list())) {
        let Some((task, classification, targeting, ending, name)) = task(data, sid) else {
            continue;
        };
        out.push(PackageGoal {
            task,
            classification,
            priority: if v[2] == 0 { 1.0 } else { v[2] as f32 * 0.01 },
            hours: (v[0], v[1]),
            targeting,
            ending,
            name,
        });
    }
}

/// @fn F036828
///
/// A character's or faction's own "AI Goals": always on, priority = val1 / 100.
pub fn record_goals(data: &GameDataContainer, record: &GameData) -> Vec<PackageGoal> {
    record
        .refs("AI Goals")
        .iter()
        .filter_map(|(sid, v)| {
            let (task, classification, targeting, ending, name) = task(data, sid)?;
            Some(PackageGoal {
                task,
                classification,
                priority: v[1] as f32 * 0.01,
                hours: (0, 100),
                targeting,
                ending,
                name,
            })
        })
        .collect()
}
