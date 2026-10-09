# Modding Project Okran

Project Okran plays Kenshi's own data, so **Kenshi mods are Project Okran mods**. There are three
ways to mod it, from easiest to most powerful:

1. Use Kenshi mods you already have.
2. Make a mod with Kenshi's own tools, the same way you would for Kenshi.
3. Change the game's code (it's all open, MIT licensed).

Nothing here touches Kenshi itself: you need your own copy of Kenshi, and Project Okran never
writes into your Kenshi folder.

## 1. Using mods

Project Okran finds mods where Kenshi does:

- **Steam Workshop:** items you subscribe to (`steamapps/workshop/content/233860/<item>/`).
- **Local mods:** `Kenshi/mods/<Name>/<Name>.mod`.

It loads them in the same order as Kenshi:

1. `data/gamedata.base`
2. Kenshi's own `data/*.mod` files (Newwworld, Dialogue, rebirth...)
3. Your mods, in the order of your mod list

**Your mod list.** By default this is Kenshi's launcher list (`Kenshi/data/mods.cfg`), so the mods
you turn on in Kenshi's launcher are on here too. To give Project Okran its own list without
changing Kenshi's, create `%APPDATA%\kenshi-rewrite\mods.cfg`. When that file exists, it's used
instead of Kenshi's:

```
# one mod per line, first loaded first; lines starting with # are ignored
Better Bandits.mod
My Retexture.mod
```

A mod named in the list but not installed is skipped.

## 2. Making a mod

### Game data: characters, items, factions, dialogue, buildings...

Use the **Forgotten Construction Set** (`forgotten construction set.exe` in your Kenshi folder),
Kenshi's own editor. Make your changes and save them as a `.mod`. Then put it in
`Kenshi/mods/<Name>/<Name>.mod` and add `<Name>.mod` to your mod list.

Records load on top of each other the way Kenshi does it: a mod can add new records, change
fields of existing ones, and add or remove references. Later mods win.

### Models, textures, sounds and UI: replacing files

Any file in an enabled mod's folder **replaces the game's file with the same name**, wherever the
game looks for it. To change how something looks, copy the file from `Kenshi/data/...`, edit it,
and put it anywhere in your mod's folder:

```
Kenshi/mods/My Retexture/
    My Retexture.mod                       (can be an empty mod made in the FCS)
    character/skins/human_male/Head_HI.dds (replaces the game's Head_HI.dds)
    Kenshi_MainMenu.layout                 (replaces the main menu's layout)
```

- Folders inside the mod don't matter: files are matched by name, ignoring upper and lower case.
- When two mods have the same file, the one later in the list wins.
- This works for meshes (`.mesh`), skeletons, animations, textures (`.dds`, `.png`), and the UI's
  files: layouts (`.layout`), images and fonts.
- The UI's skin definitions (the `.xml` files in `data/gui`) are still read from Kenshi only.

New models and textures that your `.mod` points to by path also work, like in Kenshi.

### Testing your mod

Start Project Okran from a terminal to see what it loaded. A mod in the list that isn't found is
reported as missing.

## 3. Changing the code

Everything is in this repository, in Rust with [Bevy](https://bevyengine.org). Fork it, change it
and build it:

```bash
cargo build --release -p kenshi-game --bin kenshi
```

The code is split into crates under `src/`, each one part of the game:

| Crate | What it does |
|---|---|
| `src/io` | Reading Kenshi's files: FCS data (`.base`, `.mod`, saves), configs, the mod load order, finding resources |
| `src/data` | The game-data records, with mods layered on top |
| `src/world` | The terrain, zones, towns, biomes and weather |
| `src/char` | Characters: stats, the limb-based medical system, inventories, movement |
| `src/combat` | Fighting: damage, blocking, knockouts |
| `src/ai` | Character AI: perception, crimes, targeting, factions |
| `src/anim` | Skeletal animation |
| `src/physics` | Ragdolls |
| `src/render` | Drawing Kenshi's models, terrain, sky, water, weather and effects |
| `src/gui` | Kenshi's menus and windows (its MyGUI layouts) |
| `src/audio` | Kenshi's sounds |
| `src/game` | The simulation (`sim.rs`, `world.rs`, `law.rs`, `slavery.rs`, `dialogue.rs`, `save.rs`...) and the playable app (`bin/kenshi.rs` and its `*_ui.rs` windows) |

Good places to start:

- **Game rules:** `src/game/sim.rs` (the simulation step), `src/game/world.rs` (squads, towns,
  raids), `src/game/dialogue.rs` (conversations), `src/char` (stats and injuries).
- **Windows and menus:** `src/game/bin/*_ui.rs`.
- **Which code does what Kenshi did:** each Kenshi system is listed in `sheet/port_map.tsv`, and
  doc comments name the Kenshi functions they port (`F012345`, rows in `sheet/functions.tsv`).
  Anything that isn't Kenshi's own behaviour is marked `(assumption)` or `(stand-in)`.

Run `cargo test` before sending changes. Pull requests are welcome.
