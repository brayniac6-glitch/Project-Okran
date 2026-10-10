# Project Okran

A from-scratch, open rewrite of **Kenshi**: its own engine and game code (Rust and Bevy), playing the
original game's data from **your own copy** of Kenshi. It's like OpenMW for Morrowind.

> **You need Kenshi.** Project Okran contains none of Kenshi's files: no data, models, textures,
> sounds or executable. It reads them from your installed copy (Steam or GOG) every time it runs,
> and it won't start without one. Buy Kenshi from Lo-Fi Games on
> [Steam](https://store.steampowered.com/app/233860/Kenshi/) or [GOG](https://www.gog.com/game/kenshi).

**Status: early and playable.** You can start a new game from Kenshi's starts, or continue, save,
load and import your Kenshi saves. You can walk, fight, trade, recruit, build, research and talk
your way across the world, but much is still missing or rough. Save often.

## Download and play (Windows)

1. Own and install Kenshi (Steam or GOG).
2. Go to this repo's **[Releases](https://github.com/brayniac6-glitch/Project-Okran/releases)** page and
   download `Project-Okran-windows.zip` from the newest release.
3. Unzip it anywhere (your Desktop is fine) and run `kenshi.exe`. If Windows SmartScreen warns about
   an unknown app, choose **More info**, then **Run anyway**.

That's all: the download is one program, and it reads everything else from your Kenshi install. It
finds a Steam install on its own. If yours is elsewhere (GOG, another drive), start it with the
Kenshi folder: `kenshi.exe "D:\Games\Kenshi"`.

## System requirements

Measured on the development PC; the minimum is an estimate, not tested on that hardware.

|           | Minimum                                  | Recommended                         |
| --------- | ---------------------------------------- | ----------------------------------- |
| OS        | Windows 10 or 11, 64-bit                 | Windows 11, 64-bit                  |
| CPU       | 4 cores (Intel i5-8400 / Ryzen 5 2600)   | 6 cores (Intel i5-12400F, tested)   |
| Memory    | 16 GB RAM                                | 32 GB RAM                           |
| Graphics  | DirectX 12 or Vulkan, 6 GB (GTX 1660)    | 8 GB (RTX 4060, tested)             |
| Storage   | Kenshi installed (about 14 GB), plus 100 MB | an SSD                           |

It uses a lot of memory: about 7.5 GB, and up to 13 GB committed, after a minute in a town. It ran
well on the 16 GB test PC, but close other big programs if you only have 16 GB.

## Building it yourself

1. Install Kenshi.
2. Install [Rust](https://rustup.rs) (stable, 1.85 or newer).
3. Build and run:

```bash
cargo build --release -p kenshi-game --bin kenshi
```

```bash
target/release/kenshi.exe
```

It finds a Steam install of Kenshi on its own. Otherwise, give it the Kenshi folder:
`kenshi.exe "D:\Games\Kenshi"`. Nothing is written into your Kenshi folder. Settings and saves go
to `%APPDATA%\kenshi-rewrite`, and Kenshi's own saves are only ever read (Import).

## Mods

Kenshi mods work: Steam Workshop subscriptions and `Kenshi/mods/<Name>/` folders, loaded in your
launcher's order. Project Okran can also keep its own mod list, and a mod's files replace the
game's by name. See **[MODDING.md](MODDING.md)** for making mods, the load order, replacing models,
textures and UI, and changing the code itself.

## How this repo works

The rewrite is planned in `sheet/` (the Spreadsheet Method). Every function of Kenshi's executable
is a row in `sheet/functions.tsv` with the subsystem it belongs to and a one-line description of
what it does. `sheet/port_map.tsv` maps Kenshi's systems to the rewrite's modules. Code is written
from those descriptions, never copied from decompiled code. See `CLAUDE.md` for the rules,
`sheet/SCHEMA.md` for the schema and `docs/DESIGN.md` for the plan.

```bash
cargo test
```

```bash
python tools/sheet.py coverage --by sub
```

## Legal

Not affiliated with Lo-Fi Games. No game files, executables or decompiled code are included; you
need your own copy of Kenshi. Kenshi is a trademark of Lo-Fi Games. Project Okran's own code is MIT
licensed (`LICENSE`).

The sound code (`src/audio`) rebuilds Kenshi's Wwise Vorbis sounds as Ogg Vorbis by ww2ogg's method
and ships ww2ogg's packed codebooks (`src/audio/data/packed_codebooks_aoTuV_603.bin`):
Copyright (c) 2002, Xiph.org Foundation; Copyright (c) 2009-2016, Adam Gashlin; BSD licence in
`src/audio/data/ww2ogg_COPYING`. Kenshi's own sounds are read from your install at run time, never
copied.
