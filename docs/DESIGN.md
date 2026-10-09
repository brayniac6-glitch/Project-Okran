# Kenshi Rewrite: design

## Goal
Play Kenshi on a new, open engine: same world, rules and feel, our own code. The player supplies the game
(their own Steam copy); the rewrite reads its `data/` at runtime. Like OpenMW (Morrowind) or OpenRCT2.

## Target (decided 2026-10-01)
**Rust + Bevy** (ECS fits Kenshi's many independent characters and squads; cross-platform; one language).
Considered and rejected: C++ + Ogre-Next (closest asset compatibility, but C++ build/safety cost) and
Godot 4 + GDExtension (two-language seam, open-world streaming at Kenshi's scale).

Stack: Bevy pinned to one release (upgrade deliberately, never drift); our own FCS and Ogre
`.mesh`/`.skeleton`/`.material` readers in `io`/`data`; Bevy PBR + custom terrain for `render`;
avian or bevy_rapier for `physics`; bevy_ui/egui for `ui`; simulation as ECS systems on a fixed tick.

## Layers
1. **Data** (`io`, `data`): read FCS `.mod`/`.base` records, Ogre `.mesh`/`.skeleton`/`.material`, terrain and
   textures from the install. Nothing converted is shipped.
2. **Simulation** (`char`, `combat`, `ai`, `econ`, `build`, `world`, `dialog`, `save`): Kenshi's rules, written
   from the sheet's specs (limb medical system, stats and training, squads and orders, factions, trade,
   building, world states).
3. **Presentation** (`render`, `anim`, `audio`, `ui`, `input`, `physics`): replacements for Ogre/MyGUI/PhysX/OIS.

## Order
1. Data readers: load and list every record type; render the terrain and one town.
2. One character walking with the original animations; camera; selection and move orders.
3. Stats, medical system, combat; then squads/AI, economy, building, dialogue, saves.

`port_map.tsv` maps each Kenshi concept to the rewrite module that implements it; `coverage` tracks progress.
