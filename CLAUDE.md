# Kenshi Rewrite: agent rules (Spreadsheet Method)

A from-scratch rewrite of **Kenshi** (Lo-Fi Games, 2018): our own engine and game code that plays Kenshi
using the data and assets from the player's own installed copy (like OpenMW for Morrowind). No game
files, no exe, no decompiled code are ever shipped or committed.

`sheet/` is the single source of truth. Code is a projection of it. Schema: `sheet/SCHEMA.md`.

## Hard rules
1. **Read rows, not code.** Use `python tools/sheet.py` queries/grep on `sheet/*.tsv`. Only decompile a
   single function (`sheet.py decomp <fid>`) when a row is missing information you need.
2. **Never invent an interface.** Any function, type, field, enum, or signature you reference must already
   exist as a sheet row. If it doesn't, add the row first (status `raw`/`named`), then use it.
   `python tools/sheet.py validate` must pass before any commit.
3. **Claim before editing.** Run `sheet.py claim <sub> <agent>` before touching rows or code of a subsystem.
   Only edit rows whose `sub` you hold. Release with `sheet.py release <sub>` when done.
4. **One subsystem, one output file set.** Code for a subsystem lives under `src/<sub>/` only. Never edit
   another subsystem's folder.
5. **Coverage is a query.** "What's left?" = `python tools/sheet.py coverage`. Don't guess.
6. **Clean room.** `decomp/` (raw Ghidra output) is gitignored and must never be committed or pasted into
   sheet cells or `src/`. The sheet stores our own names, signatures and one-line behaviour specs; code
   is written from the specs, never transcribed from decompiled output. Port behaviour, not code.

## Workflow per subsystem
triage (name + ns) → type (sig, types/fields) → spec (1-line behaviour) → map (port_map key) → impl → verify

## Tools
- Ghidra project: `kenshi`, program `kenshi_x64.exe` (ghidra-cli: `ghidra ... --project kenshi --program kenshi_x64.exe`).
  Imported from a copy outside Program Files (e.g. `C:\tools\kenshi-bin\kenshi_x64.exe`; Ghidra's launcher breaks on
  `Program Files (x86)`). The game itself: `C:\Program Files (x86)\Steam\steamapps\common\Kenshi`.
- `python tools/sheet.py ingest`: refresh sheet from Ghidra (preserves human-edited columns)
- `python tools/sheet.py coverage [--by sub]`
- `python tools/sheet.py validate`
- `python tools/sheet.py claim|release <sub> [agent]`
- `python tools/sheet.py decomp <fid|addr>` → `decomp/<fid>.c` (cached)
- `python tools/sheet.py next <sub> [n]`: highest-value unfinished rows to work on
- `python tools/rtti.py ...`: C++ classes/vtables (needs `decomp/rtti_vtables.tsv` from
  `tools/ghidra/ExportClasses.java`)
- `tools/ghidra/ExportRefs.java` → `decomp/calls.tsv`, `decomp/strrefs.tsv` (call graph + string refs; used by
  ingest for third-party spread and `strings.tsv` fids). Run scripts with
  `ghidra script run tools/ghidra/<X>.java --project kenshi --program kenshi_x64.exe -- <abs path to decomp>`.
  Triage findings: `docs/TRIAGE.md`.

## Kenshi notes
- Original engine: **Ogre3D** (`OgreMain_x64.dll`), **PhysX 2.8**, **MyGUI**, **OIS**, in separate DLLs: their
  code is `thirdparty` (status `skip`): we replace them, we don't port them. Only `kenshi_x64.exe` game
  logic is ported.
- Game data is in the install's `data/` folder: FCS `.mod`/`.base` files (records), Ogre `.mesh`/`.skeleton`
  and textures. The rewrite reads these from the player's install at runtime (`io` and `data` subsystems
  specify the formats).
- Prior community research (RE_Kenshi / KenshiLib, OpenKenshi docs) may name classes and file formats.
  Use it as evidence for naming rows, never paste its code; cite the source in the row's notes.

## Current state (hand-off)
1. Ghidra analysis done; sheet ingested (166,720 functions; 55k to port, 51k still `unk`: see `docs/TRIAGE.md`
   for why automatic triage stops here). Re-ingest = run `ExportClasses.java` + `ExportRefs.java`, then `ingest`.
2. Engine: Rust + Bevy (`docs/DESIGN.md`). Cargo workspace at the root; crates live in `src/<sub>/`.
3. `io`: FCS v16+v17 reader done (`src/io/fcs.rs`), parses every shipped and workshop file to EOF.
   `cargo run -p kenshi-io --bin fcsdump -- [--types|--records] <dir|file>`; full check:
   `KENSHI_DIR=<install> cargo test -p kenshi-io -- --ignored`.
4. `data`: `ItemType` (78 record kinds, T00007) mirrors the sheet (a test enforces it); 4 kinds `UNKNOWN_n`.
5. Mod loading + merge done (`src/io/mods.rs`, `src/data/gamedata.rs`; spec in rows F041671, F046684, F046696):
   `cargo run -p kenshi-data --bin gamedata -- <install> [--warnings] [--show <string_id>]`.
   Open: where the original orders core files (we assume header dependency order); `.translation` files
   are not loaded yet; meaning of the v17 header tail.
6. Ogre `.mesh` reader done (`src/io/ogre_mesh.rs`, rows T00010-T00015): `cargo run -p kenshi-io --bin meshdump -- <dir>`.
   v1.41/v1.8 files store wrong lengths for MESH/SUBMESH/GEOMETRY/MESH_LOD: those are read by content.
   Poses, animations, edge lists are skipped (not decoded yet).
7. Ogre `.skeleton` reader done (`src/io/ogre_skeleton.rs`, rows T00016-T00019): `skeldump` tool. Stored bone
   lengths omit the name (36 = no scale, 48 = scale).
8. First Bevy app (`src/render`, Bevy pinned =0.19.1): `cargo run -p kenshi-render --bin viewer -- <install>
   "<record name>" [--screenshot out.png] [--untextured]` renders an item from the install with its texture
   (item `material` ref -> MATERIAL_SPECS_* `texture map`/`normal map`). Debug examples:
   `kenshi-data --example record|files`, `kenshi-io --example elements`.
9. Characters: RACE records render as the skinned male body on their linked skeleton with animation
   (`src/anim` Rig + sampling; `viewer ... "Greenlander" --anim idle_stand_confident --time 0`). Weapons are
   textured via WEAPON_MANUFACTURER models. `kenshi-io --example anims <skeleton>` lists animations.
10. Heads + hair: head = body mesh's v<0 UV tile with the HEAD texture; hair/beards = ATTACHMENT meshes skinned to
    the body skeleton, their "Mod" textures decoded on CPU (`src/io/dds.rs`) and tinted. Resources resolve by file
    name (`src/io/resources.rs`). `viewer ... "Greenlander" --hair Beard02 --hair Haircut09-merc --focus "Bip01 Head"`.
11. Clothing: `viewer ... --wear Dustcoat --wear "Drifter's Leather Pants" --wear "Drifter's Boots"` (ARMOUR meshes
    skinned to the body skeleton, textured via `material`).
12. Terrain: heightmap `data/newland/land/fullmap.tif` (16385^2 u16) spans 294912 units, heights *9800/65535
    (constants from initTerrain F046700 -> setTerrainScale in Plugin_Terrain_x64.dll). `src/world`, TIFF reader
    `src/io/tiff.rs`, `cargo run -p kenshi-render --bin terrain -- <install> [--at fx,fz] [--size u] [--res n]`.
    Screenshots wait 180 frames: cold-start shader compilation otherwise yields a blank frame.
13. Towns: zone files (FCS v15 + id trailer, T00023) place buildings; world origin = map centre, col = x,
    row = z (verified against building heights). `cargo run -p kenshi-render --bin town -- <install> 34.46
    --view far` renders a zone with its buildings (BUILDING `parts` -> BUILDING_PART `phs or mesh`).
14. Terrain colour: the 8x8 painted tiles `overlaymaps/colour.X.Y.png` (T00024) map one-to-one onto the heightmap;
    terrain meshes are split per tile (`terrain_tiles`). Buildings textured per BUILDING_PART material.
15. Characters are reusable: `src/render/character.rs` (load_character/upload_character/spawn_character,
    `Animated` + animate_characters). `town ... 34.46 --people 12 --view street` puts animated townsfolk in town
    (placement avoids buildings using each part mesh's horizontal extent).
16. `src/render/zone.rs` (load_zone/ZoneScene) is shared scene code; `town` uses it. Layered lower/upper animation in
    `src/anim` (`sample_layered`), `Animated::play_layered`. No placeholder gameplay: interactive play waits for
    the original's movement/AI/order logic to be specced from the sheet.
17. Gameplay specs started (char): GLOBAL CONSTANTS live in a float struct at 0x142133dd0 (T00025, 81 named fields);
    code reads them by address, so a byte scan for RIP-relative uses of those addresses finds the gameplay code
    (Ghidra has no xrefs there). Medical cluster F0404xx: getKOPoint, computeKnockoutTime, applyBodyPartDamage
    specced; CharStats (T00026), BodyPart (T00027), MedicalSystem (T00028) layouts; lerp (F050479).
18. `src/char` (kenshi-char): first gameplay code, written only from the specs (@fn tags) with tests; Constants read
    from the merged GLOBAL CONSTANTS record. Healing tick F040473 specced and implemented.
19. Medical update driver, bleeding and max blood specced (F040477/F040438/F040410/F040422) and implemented.
    GlobalConstants map re-extracted (84 fields; `*param_1 =` form was missed before).
20. Hunger specced and implemented (`src/char/hunger.rs`): MedicalSystem +0x60 is hunger (starvation death), +0x163 is
    "downed" (blood loss or a destroyed limb).
21. The FCS editor (`forgotten construction set.exe`, .NET) carries the data schema's enums: `python tools/dotnet_enums.py
    <exe> [regex]` dumps them (91 enums). itemType confirmed all record kinds (T00007, 115 values), StatsEnumerated
    added (T00029). Rust enums are generated from sheet rows: `python tools/gen_enum.py <tid> <Name> <out.rs>`.
22. Stats training specced and implemented (`src/char/stats.rs`): addXP curve, train/trainUpTo.
23. Combat XP specced (trainFromCombat F047467, skillDiffMult F047454); skillDiffMult + trainToughness implemented.
    `tools/ghidra/Disasm.java <addr> <max bytes> <out>` lists instructions (disassembling bytes analysis skipped)
    for functions the decompiler mangles; output stays in decomp/.
24. `src/combat` (kenshi-combat): cut/blunt/pierce damage (lerp from base to "<x> damage 99" by skill + attributes)
    and block chance (base 70, +/- per level gap, soft cap 90, hard cap 95) from the original.
25. Hit resolution specced and implemented (`src/combat/hit.rs`, resolveHit F040476): weighted part pick
    (hit weight * focus, BodyPart +0x38/+0x3c; +0x3c was misnamed "bleeding"), armour stacking (Armour T00030:
    cut/blunt resistance, flat pierce, cut into stun, per-part coverage chance), penetration, toughness resistance
    (lerp damage resistance min/max), wound = max(cut,pierce) * bleed rate * bleed. Armour::Armour (F047099)
    specced in outline; the decompiler drops its lerp arguments (use tools/ghidra/Disasm.java).
    Float constants: read from the exe with a small PE reader (section table -> file offset), not Ghidra.
26. Armour specced and implemented (`src/combat/armour.rs`): Armour::Armour (F047099) from ARMOUR records +
    the hardcoded material/class tables (armourTables F047256, a static constructor; ArmourType T00032, ArmourClass
    T00033, LimbState T00031 from the FCS editor). ARMOUR "slot": 3 head, 5 body, 6 legs, 8 shirt, 9 boots.
    `cargo run -p kenshi-combat --example armour -- <install> [quality]` lists every shipped piece;
    `cargo run -p kenshi-data --example field -- <install> <type> <key>` surveys a field's values.
    Open: where item quality (+0x220) comes from; the other armour fields (+0x120, +0x2e8, speed/athletics mults).
27. Weapons specced and implemented (`src/combat/weapon.rs`): Weapon::setupStats (F047085) from WEAPON records
    (+ optional material spec / maker), CharStats::setWeapon (F047095) sets the wielder's damage factors and curve
    bottoms; WeaponCategory T00034. `cargo run -p kenshi-combat --example weapons -- <install> [quality] [level]`.
    Open: whether callers of cut/blunt damage scale further (race/robot/human/animal mults, min cut); item quality source.
28. Melee flow specced and implemented: attackDamage (F047066, `attack.rs`: weapon or unarmed damage, robot/animal/
    human/race mults; race +0x7c is the robot flag, vtable 0x248 animal, 0x250 human), defendOutcome (F040724,
    `melee.rs`: dodge window 0.1..0.98, block past 0.5 in the right direction, animals can't defend), punch
    feedback (F040731: unarmed strikes hurt the striker's limb by what they hit), BodyPart::damage (F040421),
    partForSlot (F040500). onMeleeHit (F033862) / onProjectileHit (F033863) specced as the glue.
29. Combat techniques (COMBAT_TECHNIQUE, `technique.rs`, F040738): timing, allowed weapon categories, per-hit frames
    (direction, power, blocked/stop times, limb). Body-part kind = LOCATIONAL_DAMAGE "body part type": 0 torso,
    1 leg, 2 arm, 3 head; technique limb slots: arms 0/1, legs 2/3, 4 weapon; "low strike" can't hit arms/head.
    `cargo run -p kenshi-combat --example techniques -- <install>`; data example `record` now prints fields.
30. Blocking: updateBlock (F039792) picks the most skilled attacker mid-swing (progress 0.1..0.99) and rolls
    blockChance; the gap is raw skill levels (constants stored * 0.1 per level: 1.5 / 1.2 - fixed). defenceLevel
    (F047048: CharStats +0xf0 "dodge" is the blocking skill; encumbrance/injury penalties), trainDefence (F047459).
31. Defence choice (chooseDefence F047075, `technique.rs`): a failed defenceChance roll (F047063) makes the
    defender pick a wrong block on the same side (front/rear, F040715) - which defendOutcome then lets through;
    staggered or lucky unarmed defenders dodge instead. Weighted by technique "chance".
32. reactToAttack (F039797, `technique.rs`): a defender reacts to a swing between 10% and 100% of its progress
    with chooseDefence, trains defence on dodges/no defence, reaction timers 0.2 / 1.2 / 0.1, animals 2.0.
33. Attack choice: loadCombatTechniques (F047079) splits enabled techniques into attacks / defences;
    gatherAttacks (F047074) weights attacks by reach vs distance, skill range, encumbrance, limb usability
    (F047059, input for now), repeat penalty. Technique numbers are floats (fixed: chance was read as int).
34. chooseAttack (F047076): weapon moves, else unarmed (strict, then weak) for non-animals; weighted pick.
    The weighted set (F047250) only takes weights > 0.
35. CombatClass state machine (T00037; SwordState T00038, names inferred): state +0x1f0, desired +0x1f4; state
    objects (F029594) 0 AttackState, 5 CircleStateAI, 6 WaitingStateAI (RTTI names read from the COLs - the
    RTTI export missed these two classes). AttackState canEnter/update/tryAttack (F029603/F029616/F029615)
    ported as decisions in `states.rs`. AI driver: CombatClassAI slot 9 (F039852) per tick, slot 10 (F039848)
    for state 3; blocking state update F039801 (CombatClass slot 12).
36. Circle/Waiting AI states specced (F029597..F029602; their movement is AI-subsystem territory). The swing:
    updateSwing (F039804) - frame index = frames whose impact time ("anim blocked frame") has passed (F040729);
    each new frame strikes (F039803): attackDamage * frame power -> target onMeleeHit (Character slot 62),
    recovery 0.6 hit / 0.3 blocked / 0.2 dodged (F047022), trainFromCombat 0/1/6. Ported in `states.rs`.
    Target collection in the swing arc (F039793) not decoded yet. Task_BashDoor (F031240) also calls damage.
37. Swing hit box (F039793): oriented box ahead sized by weapon length / radius / size (kicks: 13x8); the box
    math is ported (`swing_box`), the world query and pull-back are specced. Targets: hostile or my target.
38. Attack slots: an attacker in reach asks the TARGET (requestAttack F040732) which state to take; each grant needs
    a slot (AttackerSlots::add F040727; count F040725 = "max num attack slots" (1) + race int +0x84, 1 while the
    defender attacks/moves); a skill contest (F040707, "attack chance factor") turns weaker attackers away ->
    circle. Idle (F039837) and tryEngage (F039834) specced; slots, contest and idle ported in `states.rs`.
39. AI driver specced (CombatClassAI::update F039852) and the engaged step ported (F039848, `engaged_step`).
    The combat decision layer is now ported end to end; what's missing to run a fight is the runtime it sits on:
    movement/pathing (F029541, Havok AI), animation playback/progress (F038894, F038914/F038915), perception
    and targeting (F040734), hostility (Character vtable 1000/0x3f0).
40. Animation clock ported (`src/anim/clip.rs`): SingleAnimation (F038899 ctor: progress starts 0.01) and its
    update (F038819: dt*speed, loop/clamp, sync phase, progress = time/length, ordered events with wrap). Combat
    clips live in the race skeleton (male_skeleton.skeleton: "chop down", "downward combo", "block left upper").
    `technique` render binary: one COMBAT_TECHNIQUE as a strip of poses at start / each impact / end, timed by the
    ported clock (`cargo run -p kenshi-render --bin technique -- <install> "Downward combo" --weapon Katana
    --screenshot out.png`). Weapon on "Bip01 Prop2" without offset (attachment offsets not decoded).
    Observation to check: at hit 2's impact time the sword is still raised.
41. Animation layers ported (`src/anim/layer.rs`, AnimationLayer T00040): play (F038900) restarts or revives a
    fading clip; stop moves it to the fading list (F038861, via Character::stopAction F037160); update (F038905)
    fades weights at clamp(speed, 0.5, 10) * layer rate (F038818), normalises active + fading to the layer max,
    fires only the dominant clip's events, drops faded clips.
42. Weighted pose blending (Rig::sample_weighted, Ogre's average blend). Layers: created by F040013 with fade
    rate = GLOBAL CONSTANTS "animation blend rate" (4) * 2.0 base / 1.25 action / 1.0 overlay / 0.5 / 0.5
    (F040175); AnimationClass::update (F038910) runs action (normalised to 1), base (1 - action keep-weight
    clips), overlay. Open: which F038900 flags the technique player (F038914) passes (keep weight, root motion).
43. Technique player (F038914): attacks and dodges play with keep-weight + root motion (override the base layer,
    move the body), blocks blend over it and reverse while fading; `technique_play` in combat. The `technique`
    viewer now runs the ported layers (stance on base, move on action; `kenshi_render::character::Blend`
    poses a weighted snapshot). Note from the port: a lone clip on a normalised layer snaps to full weight at
    once (scale = max / sum); fades show between clips and on stop (5/s on the action layer).
44. findTarget (F040734) specced: keeps a target 1.0 s (0.3 s flagged), forced target first, else threat lists
    (own +0x1f8, squad, squad target's) filtered by F046525 - which is a threat-memory lookup (marked hostile /
    squad target), not faction relations - scored by the squad (F038638 / F038643; max 400 from the squad).
45. Subjective tags ported (`src/ai`, kenshi-ai): each platoon's memory (+0x98) keeps SubjectiveTags per subject
    (character, or a whole player squad): TagsClass bitsets of CharacterPerceptionTags_ShortTerm / _LongTerm
    (RTTI names) + time. Being attacked tags the attacker ST_TEMPORARY_ENEMY (not allies / allied squads,
    F040863); witnessing a crime (F046520, CrimeEnum at Character +0x148) tags ST_CRIMINAL and raises an
    EventTriggerEnum event (41 assault, 43 thief, 17 intruder, ...). Enums T00041..T00044 from the FCS editor.
    Faction relations ported (`ai/faction.rs`): FACTION 'relations' refs (-100..100) + 'default relation'; ally at
    >= 50 or forced (F041495), enemy at <= -30 (F041494). Open: whether relations are mirrored from the other
    faction's record (loader F041509/F041516), player relations (PlayerFactionRelations).
    Squad target scoring specced (Platoon::scoreTargets F038638): prefer the squad's own target, unarmed and
    downed targets; penalise moving targets and ones already crowded by attackers; distance and reachability
    terms. Open: the tuning object at squad +0x310 (fields +0x154/+0x158/+0x168/+0x16c - which record?).
    Next: identify that tuning record, F046525's map (F046521, flags +0x38/+0x39), movement (F029541);
47. Encumbrance ported (`char/encumbrance.rs`): load = clamp(invlerp(1.2 * arms * strength level + 15,
    0.05 w, w), 0, 1) (F047034; strength level = strength * stats+0x34, F040360 = CharStats vtable 0x10, also the
    unarmed formula's factor); strength XP from walking (F047455). Movement: CharMovement (29 methods) over Havok
    AI path following (third-party: needs our own pathfinding); race speeds in race data (F033689: speed max
    skill +0x48, min skill +0x4c, swim mult +0x60, walk speed +0x64). Run speed (F047018) = lerp(athletics
    * 0.01, min, max) * (0.5 + 0.5 * fitness) and swim speed (F047058) ported (`encumbrance.rs`). Character vtable
    0x398 = maturity (1 for people, an animal's growth), 0x390 = size. Move speed (F047064, CharStats +0x17c):
    (run - 11) * legs (worse leg * 1.8) * hunger (half when starving) * leg equipment * load * mult, sneaking
    capped by stealth toward 12, + 11; water (F039118: shallow <= 45, wading/deep by swimmer), prone 7.
    Next: how CharMovement applies the speed / steering (Havok AI replacement), walking (MoveSpeed WALK/JOG).
48. Mover ported (`char/movement.rs`): CharMovement (F040650 update, F040653 goTo, F040657 init) wraps a
    Havok AI character (named AICharacter here, inferred; F024338-F024366). Havok works in metres: positions,
    radius and max speed * 0.1; acceleration is the race value as is. Race fields "pathfind footprint radius"
    (* size), "pathfind acceleration" (lerp(maturity, 3a, a)), "water avoidance" (path cost), "combat move
    speed mult". steer (F024364): velocity moves toward max speed * direction by at most accel * dt; step
    (F024365): brake when distance - 0.1 < v^2 / 2a (target speed sqrt(2 a d): our reading, the original's
    perpendicular corner case is not ported), land on the goal when the step passes it. goTo ignores goals
    within 1 unit, or within 2 of the current goal. `movement` example. Open: the path itself (navmesh search
    and local avoidance are Havok's: the rewrite needs its own), formation steering (F029549, mode 1),
    who writes the walk speed +0xc0 (default 15, the race "walk speed" for most races, unconfirmed).
    Speed chain: move speed (CharStats +0x17c) -> AnimationClass max speed (F037131, via F040429/F040443)
    -> CharMovement own speed +0xb4 (F040651) -> min with the MoveSpeed cap +0xbc (F040700: WALK walk
    speed, JOG 55, RUN/GROUPED 999; grouped uses the group's pace F045542) -> mover max speed.
49. Engaging (`combat/engage.rs`): tryEngage reach check (F039834), requestAttack (F040732: the target answers
    Attack / Block (it is already striking at the asker) / Circle). Bodies from race data: BodyPart::init
    (F040462, LOCATIONAL_DAMAGE fields) and MedicalSystem::createBody (F040468, race "combat anatomy" refs =
    [hit weight, max health]). Attack/defence speeds (`char/combat_speed.rs`): weapon strain (F047010), handling
    (F047011), combatSpeed (F047012: attack 0.8..1.2, defence 0.6..4). GLOBAL CONSTANTS struct base 0x142133dd0
    (e.g. +0x108 max num attack slots, +0x10c min strength xp mult, +0x110/+0x114 weight strength diff 1x/max).
    NB scratchpad xref.py matches nearby addresses: check hits with Disasm.
50. Combat AI ported (`combat/fighter.rs`): CombatClass fields (offsets in comments), CombatClassAI::update
    (F039852), updateCommon (F039846: radius = footprint * 1.5 * size, reach margin, the locked block roll),
    AttackState tick (F039805), BlockState tick + updateBlockAction (F039802/F039801: re-choose per threat frame,
    hold by replaying), swingProgress (F040722: action progress / current frame's impact time -> 1 at the
    impact), nearestThreat (F040723), refreshAttackers (F040735: those targeting me with progress in (0, 1)).
    The game side is the `Fighting` trait; the `combat` example `duel` implements it for two fighters (idle /
    engagedStep / circle / waiting / strike / reactToAttack / tryEngage / requestAttack from the ported pieces).
    Circling (F029599) re-runs tryEngage each tick and goes idle once the target gives a slot; approaching
    when nothing reaches is formation steering (F029616). Hit reactions: stumbles (`stumble.rs`, F033861/F037176),
    the victim goes Locked (8) while the stumble plays (inferred). Armour on the fighters. Body parts carry
    their LOCATIONAL_DAMAGE id. CombatClass ctor (F039806): +0x280 = r + 2.5, +0x284 = r + 7.5, +0x288 = r + 19.
    Since ported: engagers (F040733: at most 5 per target, the rest turned away, double-buffered counts
    +0x210/+0x214 by +0x218 flipped in F039841), 'reachable' = among the target's engagers (F040717), the
    wind-up commit point (F039791), the waiting hover band by crowd (F029602), swing box pull-back and lunge
    distance (F039793), target lead 1.2 s (F033809). Neighbour separation (F030323) and strafe alternation
    only matter with more than two fighters (the target is excluded; 1v1 motion is radial).
    Animation choice ported (`anim/select.rs`): AnimationData (F038934, T00048), the situation filter and
    weights (F037153/F037148/F037145/F037142), the library lists (F038935), movement blend (F037154: upper list
    strict, lower loose; rate = play speed * speed / size), idle = best weighted (F037155/F037213), track
    stripping by delete flags and blocks' lower body (F046685/F046682), override bones (F046683), pain postures
    (F040458 -> AnimationClassHuman +0x268, played by F037181 with category 0 and speed <= 50). Root motion
    (F038905/F038910/F040615/F040617): a root-motion clip's Bip01 translation (weighted) per layer, summed;
    the change per tick (skipped when it shrinks by > 1), turned to the facing, drives the mover
    (animation-driven mode, +0x380 / dt); the pose leaves that translation out (setDisableTranslation).
    Blend formula assumption: per-bone normalisation over the clips animating the bone (Kenshi's Ogre fork
    not read). The duel harness and renderer use all of this.
    Still open: swing box ground height and max simultaneous hits (no effect 1v1 on flat ground); knockback
    (blunt only, consumer likely physics); knockdowns / ragdolls (Scythe .phs on PhysX 2.8). Character update
    order: characters update on worker threads (pointers <-> handles around the update, F039849/F039853), so
    the original has no fixed order to copy.
    Undershirts ("vest" textures) are composited over the body skin like the character shader (colour only).
51. Navmesh (`world/navmesh.rs`, Havok tagfile reader `io/hkt.rs`, T00049/T00050): Kenshi pathfinds with Havok
    AI; the shipped tiles `data/newland/land/navtiles/tileX.Y.hkt` hold the ground mesh of zone cell X.Y plus one
    mesh per building (placed at its "Info" position, unrotated), in Havok metres (units x 0.1, Y up). Section
    uids: ground Y*256+X, building = Info hex; streaming sets link edges (one edge may meet several) and cluster
    nodes across sections. Face data = Kenshi face type (3 water, 4 costly +5 m, 5 blocked). Ported: tile path
    (F032195), WaterCostModifier / EdgeFilter / WaterEdgeFilter (F032307/F032309/F032316), the request steps of
    the manager (F032235: start face 500 m, straight line with water shut F032207, goal 30 m then 500 m kept a
    radius off the boundary F032185, cluster reachability F032192, search F032208 with 100000 iterations).
    Havok's own search is replaced (A* over faces, edge width >= diameter, midpoint costs, funnel).
    `navpath <install> X Y sx,sz gx,gz out.png [water cost]` draws one; `navfit` checks tiles sit on the
    terrain; install test reads all 3995 tiles (~1.7% of shipped seam links are one way/stale).
    Path following: `GatePath` (F024361: heading string-pulled each tick from where the mover stands through
    the gates ahead, crossed gates dropped) and `char::movement::follow` (full-speed steering while gates are
    ahead, the braking step F024365 on the last segment, height from the face underfoot tracked by `walk`).
    `cargo run --release -p kenshi-char --example walk -- <install> 34 46 0.47,0.31 0.55,0.40 > walk.csv`
    records a run; `cargo run --release -p kenshi-render --bin walk -- <install> walk.csv --frames dir --gif
    out.gif --every 4` replays it in the zone with the movement blend (anim::select).
    Request queue `PathManager` (F032210 submit: priority 20 urgent / 10, F032211 cancel, F032235 merge: same
    owner replaces, highest priority first, one answer per update); `char::movement::Pathing` = CharMovement
    mode 0 state (goTo F040653 -> setGoal F024356 -> answer -> follow).
    Open: which tiles/sections are loaded when (streaming, building add/remove by hash F032233), the gate
    merging by clearance in F024361, failed-request retries (17 offset tries in goTo), local avoidance.
52. NPC AI (sub `ai`, notes in the sheet rows F0369xx/F0398xx): every character's AITaskSytem keeps a goal
    list (Task objects made by TaskRepertoire::create F030716 from a type), filled by setupDefaultGoals
    (F040015: IDLE, SELF_PRESERVATION, then the template's and faction's "AI Goals" via addGoalsFromRecord
    F036828: priority = AI_PACKAGE ref val1 * 0.01, targeting from the AI_TASK bools). decide (F036926):
    orders, urgent goals, urgent jobs, all goals, non-urgent then fluff jobs, the default IDLE goal; goals
    rank by list position (F036915), a candidate starts when its score > 0 (F036923, F039871). Each task
    type's definition (253, registered by F030685) is extracted into `src/ai/task_def.rs` (T00051): score =
    an AI virtual method (most are constants), steps/interrupts lists, prerequisite, raw fields by offset.
    Conditions: 147 checks (`src/ai/condition.rs`, F039876); a definition's +0xa8 list = preconditions,
    +0x4c list = completion conditions (= its effects), +0x128 = target condition. Planner (`src/ai/planner.rs`,
    F036920): regressive best-first search; failing preconditions are solved by tasks whose completion
    conditions make them hold (repertoire solver index F039872); generic over an `Agent` (checks, scores,
    target finders). Urgent goal pass: 16, 31, 45, 57, 58, 250 (F036879).
    AI nodes (`world/nodes.rs`): node BUILDING records ("is node", node type 0 shopkeeper / 1 general /
    2 guard post / 3 defensive / 4 use / 5 light) are placed by buildings' instances and, for general,
    shopkeeper and guard nodes, by furnishing layouts: interiors.level kind-30 records keyed by (associated
    building, layout name), chosen by the placement state record's "interior/exterior layout name"
    (`Layouts::load` merges leveldata/ then the zone folders). Node objects come from "is node" buildings
    (F038256); the manager picks a free one of a type in a town at random (F031809); guard posts are also
    spawned at gates (F031808). Wandering town (37): hook F050264 -> node type 1; plan Moving (1) -> Stand
    at node (51, claims the node, F031320/F031321); atNode < 0.95 * radius, isAtLocation < 12 units.
    AI packages (`src/ai/package.rs`): a package reference is (start hour, end hour, weight); per role (leader
    2 / squad 0 / squad2 1 / slave 4, F028559) the goals become JOBS of their AI_TASK classification
    (F028583 rebuilds jobs[class] in tryJobs F036925), ranked by score * priority (weight / 100, 1.0 at 0)
    within their hours (rankList F036914); "inherits from" packages first. A record's own "AI Goals" are
    (0, priority %, 0) goals (F036828). Squads come from the building state's "squad template"; a squad
    without a leader entry is led by its first member. Zone 34.46 is Mourn (not the Hub).
    Task methods (vtable: 0 dtor, 1 start, 2 update, 3 end, 4 finished) take (task, CharBody): CharBody
    (0x78 bytes, F040008) = +0x18 character, +0x50 AI, +0x58 CharMovement; AI +0x20 = AITaskSytem, AI +0x2f8
    = character, AI +0x10 = squad destination object (+0xf8 = destination town hand). Towns
    (`world/town.rs`): placed once each by `leveldata.level` (zone folders, later wins); radius by "type"
    times "town radius mult" (F048768: type 2 = 2500); current town (character +0x88, F038707) within
    1.0 / 1.5 radius. Patrolling town (36, Task_PatrolTown): move order (F039100) round the destination town,
    radius half the town's: CharMovement::order (F040635) takes a random point in that square and goTo,
    again whenever the mover is idle (`movement::area_point`).
    Squads (`ai/formation.rs`, `ai/follow.rs`): the faction's "squad formation" (0 loose, 1 caravan, 2
    ranks) lays out member offsets (F028591; +x is behind the leader, -x its facing). Following
    leader (141, Task_FollowLeader F031009): spot = leader + facing * leader speed + rotated offset
    (F030760); speed cap by distance to the spot; straight steering (CharMovement mode 2) within 50,
    goTo beyond. Carried/led characters in the layout and losing sight of the leader are not
    ported. Pathfinding replacement: a short edge only blocks when walls sit at both of its ends
    (`gap`), else followers got trapped on finely cut ground. Character vtable +0x248 = asAnimal.
    Furniture (`world/furniture.rs`): BUILDING records with a "functionality" (BUILDING_FUNCTIONALITY:
    "function" BuildingFunction 6 bed / 16 table / 17 chair, "max operators", "use range"), placed like
    nodes. A building's designation (BD_BAR 3, ...) is its "residents template"'s "building designation"
    (F048905). Package handlers come from the package's "signal func" (F028935: 17/26/27 ->
    Package_HangAroundBar). Relaxing (214, finder F050252 -> handler F029228): the nearest free chair
    in the squad's bar; Operating machine (87) walks there and sits (F031349/F031350). IS_WORKING (123)
    always answers false. Condition ids 0, 2, 3, 6, 8-11, 13, 14, 19 are never registered: no check,
    they pass. A package's plain "AI Goals" go to every role but slaves (F028594). The town sim spawns
    the town's "bar squads" at its bar (one of each: harness). `export-models` (src/export) converts a
    local install's character models to .glb (no game files bundled).
    Night routine (town sim, `TOWN_START_HOUR=22.5 TOWN_HOUR_SECONDS=30`): inside a building = inside its
    "interior mask" mesh volume (`world/interior.rs`, F037793; Mourn's floors are baked into the ground
    navmesh, so navmesh sections can't tell). Doors = the building's "doors" BUILDING records, placed by
    their part meshes in the building frame; lock state per door in the placement state ("locked{n}",
    "lock lvl{n}"). Going to bed (153/239) -> Locking up (155: Entering building 156, Moving to door
    inside 163, Locking door from inside 162) -> Sleeping (98) on a home bed (F047587) -> Getting out of
    bed (116) via the planner when a later task needs (83, false). Going to bed commits 2 + rand * 8 h.
    Layout lookup follows "shares interiors with" (the Old Storm House's "General Shop" is the Storm
    House's). Pathfinding replacement: a gate counts as crossed within GATE_REACH (corners on the
    gate line), and the follower steps onto the corridor's face at each gate (stairs over floors).
    Town sim: `cargo run --release -p kenshi-game --example town -- <install> 34 46 [wanderers] [s] > t.csv`,
    `--example town_map -- <install> t.csv 34 46 out.gif 1.0`. Harness assumptions are listed in town.rs
    (node radius 20, 1 game hour = 60 s, a plan reserves its node, unported goals score 0, a squad's
    destination town is the one it lives in). Squad members without Squad goals (Mourn's squatters) idle, as
    in the data: Following leader (141) only comes from packages.
    Combat in town (`squad:Fog raid@1`: `@n` = the template's "AI packages" slot, 0 home / 1 roaming /
    2 retreat): goal 16 Attacking enemies (score F038552, finder AI::findEnemy F038641 in `ai/enemy.rs`,
    target condition 6 isNoEnemiesInVicinity = AI +0xbc == 0 - the code keeping +0xbc is NOT found: the
    harness counts seen, standing enemies) -> Running 67 (until within 500, condition 41 = F038683 with
    500; 40 = with 100) -> Attacking 4 (score F038700; runs: f134 = 1) -> the fight in
    `kenshi_combat::arena` (the duel harness as a library: Arena/Fighter/Shared/Kit, `active` and
    `foes` per fighter). Sensor per think: perception::sees with daylight from the clock. Harness: random
    start facing (idle look-around not ported), visiting squads face the residents, the attacked fights
    back and tags the attacker's side (F040863), kit = katana + plain outfit at skill 30, arena movement
    ignores the navmesh. Crowd fixes found in the original: setTarget (F040712) unregisters from the
    old target's engagers (F040718 removeEngager); Circle/Waiting enter always succeed from the engaged
    state (F029598/F029601). GameDataContainer::of_type walks load order (deterministic runs).
    PLAYABLE SHELL (priority order agreed with the user: 1 shell, 2 combat in game, 3 stats/items/
    medical, 4 trade/dialogue, 5 building/research, 6 save/load; small AI items fill in later):
    `cargo run --release -p kenshi-game --bin kenshi -- <install> [34.46] [--player 3] [squad:...]`.
    kenshi_game::sim = the town sim as a library (Sim::load / step; order_move / order_attack go
    through the task system's orders list; player chars have no free will). The app (game/bin/
    kenshi.rs): zone render, people driven from the sim (arena layers while fighting, stealthKO when
    down, ported movement/idle clip choice otherwise), RTS camera, select / right-click orders,
    pause + speeds, HUD, roof hiding over the focus (render aid). Test hooks: --shot out.png
    --shot-at N, --auto-attack, --cam d,pitch,yaw. Harness: approach to 60 units on the navmesh
    before the arena (ENGAGE_RANGE), arena steps walked on the navmesh, attack order makes the
    victim's side hostile, isTargetDown (7) checked. Next: zone streaming beyond one zone, run/walk
    toggle, squad formation on orders, then items/inventory.
    DONE since: formation orders (Sim::order_move_squad), R run/walk, zone + navmesh streaming
    (render threads onto one origin; Sim::stream loads nav tiles round player chars). Roaming
    population ported (`world/biome.rs`, Sim::population_tick / spawn_homeless / spawn_template):
    biomemap.png (data/newland/land) colour = BIOME_GROUP "index"; map rows run AGAINST z (checked:
    Squin->Okran's Gulf, Admag->Vain; Mourn's zone = Spine Canyon); 2 s tick fills zones round the
    player up to mult x homeless spawn amount. Harness: entries with world-state conditions skipped,
    spawned bodies are greenlanders with the katana kit, roaming package = template slot 1,
    homeless squads don't wander yet (Patrolling 22 not ported), no despawning.
    KENSHI RENDERING (user asked for the lighting/map "exact same as Kenshi"; shaders ship in
    data/materials, ported to WGSL in src/render): kenshi_lighting.wgsl = deferred.hlsl main light +
    lightingFunctions + wet + HDR composite (exposure, raw output: sRGB conversion off) + atmosphere fog
    (post/fog.hlsl; pFogParams = (D, 0.06D, 0.6D), D = settings.cfg View Distance x 10, F040805/F046688)
    + sun_shadow. Materials: terrain (terrainfp4, BIOMES F051603/F051631, blendinfo.dat), objects
    (objects.hlsl, MATERIAL_SPEC F046402), characters (character.hlsl skin + hair/beard maps F037420,
    skin.hlsl clothing, hair_fs cards F037428), water (forward/water.hlsl, height 100 F050961, per-biome
    passes F050962, distant plane), SkyX skydome (+ starfield). sky.rs: SkyX sun path/colour (F040798,
    F040805, getColorAt), horizonClouds (clear-weather assumption). Far clip = D (F041473), dome at
    0.95 x far. Whole-map terrain: coarse per-blend-cell patches out to the far clip, holed under
    streamed zones, skirts on open edges. Mip chains generated for PNGs (Ogre does). Cut-out materials
    have a prepass shader (kenshi_cutout_prepass.wgsl) so discards leave no depth. Harness stand-ins
    (labelled in code): Bevy cascaded shadows for Kenshi's RTW/CSM, no planar water reflection, no
    luminance adaptation (exposure from the clamp), no clouds/moon yet, no weather. Test hooks:
    --pan dx,dz, KENSHI_WATER_MODE (water debug views), KENSHI_NO_WATER; water_zones example lists
    zones under the water (Bark = 51.8 is a coastal town to test with). Next for rendering: SkyX
    clouds, moon/planets, Kenshi's RTW shadows, foliage, luminance adaptation.
    Six TaskRepertoires (human, animal, chained, platoon, imprisoned, crippled) gate which types a character
    can take. Next: port AITaskSytem (goal list, decide cascade) on the table, then the non-constant score
    methods and the Task_* classes the common packages use (idle, wander/patrol town, attack enemies,
    protect allies, follow leader).
46. Unique characters: e.g. Beep = CHARACTER 57390-rebirth.mod, race "Hive Worker Drone", body sliders in
    data/character/bodies/Beep.bod2 (v15-style header, then name/float slider pairs: "Waist", "Head size", ...).
    .bod2 = an FCS v15 file with one CHARACTER_APPEARANCE record (fcsrec reads it). Face sliders are mesh poses
    (Ogre MESH_POSES, now parsed: `Mesh::poses`, `apply_pose`; `poses` example); body sliders (Waist, Height, ...)
    go through Kenshi's modified Ogre bone sizing (OldBone::setBoneSize / setBonePositionalSize) in F037414
    (Character body shape, ~880 lines) - not ported. `viewer <install> "Hive Worker Drone" --wear "Rag
    Loincloth" --body <install>/data/character/bodies/Beep.bod2 --focus "Bip01 Head" --frame 0.22`. then raise trainFromCombat events; first-aid actions,
    ai/player subsystems; port the viewer onto the character module, terrain detail textures (land/textures +
    blendmap/blendinfo.dat), head/body blending, animation layering, then gameplay systems (char/ai/combat).
