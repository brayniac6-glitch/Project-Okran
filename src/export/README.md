# export-models

Converts the character models of **your own Kenshi install** to glTF 2.0 (`.glb`), the format
Blender, Unity, Unreal and Godot import directly. The tool contains none of the game's files; it
only reads the install you point it at.

```
export-models <Kenshi install folder> <output folder>
```

For example:

```
export-models "C:\Program Files (x86)\Steam\steamapps\common\Kenshi" kenshi-models
```

Build it from this repository with `cargo build --release -p kenshi-export`; the program is
`target/release/export-models.exe`. A full run takes about a minute and writes about 900 MB.

## What you get

| folder | contents |
|---|---|
| `bodies/` | every race's body, male and female (head cut away) |
| `heads/<race> <sex>/` | each head the race offers, on the body's head shape |
| `hair/` | hair and beards, tinted brown, alpha-masked |
| `armour/` | clothing and armour, male and female fits |
| `textures/` | the textures as PNG, shared by the models (keep this folder next to the others) |
| `MANIFEST.txt` | every model and the install files it came from |

Models are static, in their bind pose, in metres (a human stands 1.71 m), Y up. A body, a head, a
hair and clothing of the same race and sex line up when loaded at the origin.

Texture notes: Kenshi's normal maps are stored swizzled (X in alpha); they are converted to
standard tangent-space normal maps. Hair textures are Kenshi's coverage maps, tinted with one
brown; recolour them as you like.

## Using the output

Kenshi and its models are © Lo-Fi Games. The export is for your own use and for Kenshi mods. Do
not hand the exported files on: anyone who owns Kenshi can run this tool and make their own.
