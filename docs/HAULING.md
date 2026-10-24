# Hauling, feeding and machines: Kenshi's planner tasks

Decoded from kenshi_x64.exe on 2026-10-17 .. 2026-10-21. Ported in `src/game/jobs.rs` (the
planner's conditions, finders, scores and executors), `src/game/production.rs` (the machines'
containers and production) and `src/game/base.rs` (the building classes and inventories).
Headless checks: `haul_trip <install> 21 42 Well` (a full machine emptied), `feed_trip <install> 21
42` (machines fed from storage).

## Building classes and their inventories

The building factory (F038257) makes a class from a BUILDING record: "farm data" a FarmBuilding,
else by its functionality's "function": 1 and 4 ProductionBuilding, 2 StorageBuilding, 3
ResearchBuilding, 5 a GeneratorBuilding ("consumes") or a WindGeneratorBuilding, 0x17
GeneratorBuilding, 8 a TortureBuilding ("produces" or "flags"), 10 CraftingBuilding, 0xe
F029417's, 0x1a RainCollectorBuilding; anything else (function 13 "General storage" too) a plain
building. `crate::base::BuildingClass`.

- **ProductionBuilding** (and those made with its constructor F029411: generators, torture
  buildings, rain collectors, farms): the inventory is made when the functionality "consumes"
  or "produces" (F029411); setupFunctionality (F029392) adds a container for each "consumes"
  ingredient, "in1", "in2".., of the item's footprint, kept to it, its stack (and the entry's
  limit) the ingredient's first value; and "out" for the first "produces", its limit its first
  value (F029412). The machine window shows them in its widgets of the same names (F024440).
- **StorageBuilding** (F029410): the building's storage section (F037797: "storage size width"
  x "height", weapons only for an "itemtype limit" of 2, armour only for 3, its stack "stackable
  bonus minimum" times "stackable bonus mult") kept to its "limit inventory" items; one item:
  the section its footprint, the typed container's limit its "stackable bonus mult"; more: no
  limit. Its category (+0x440) is its "itemtype limit"; functionality "flags" 1 keeps it from
  emptying (+0x444, the food stores).
- **Plain buildings with "has inventory"** (general storage chests): the storage section only;
  they have no hauling object (+0x2f8 null), so the planner's searches pass them by.

A section's stack (F043593): the more of the item's "stackable" (1 in a section kept to some
items) and the section's +0x88, times its +0x8c. Its test (F043601): kept items, else armour
only, else its slot.

## Production

- F029326 ProductionBuilding::operate: the amount (rate x frame time x the operator's skill
  multiplier, Task_OperateMachine F031350; 1 for a machine that needs no operator, F029366)
  limited by each ingredient's banked level and container (F029360) and the output's room under
  its limit; ingredients taken one by one as needed (F029359); output made at a level of 0.999,
  the excess let go, each banked ingredient cut back to none or 1 (F029325); the state (+0x468)
  2 working, else 1 (full) when the output takes no more.
- F029391: a building needs an operator (+0x3a9) unless its "max operators" (else the
  functionality's when negative) is none; F029366 works one that needs none on its own each
  frame, and lets a full one go once its output has room.

## Tasks

| Task | Name | Kind | Score | Preconditions | Target condition | Done when | Finder |
|---|---|---|---|---|---|---|---|
| 87 | Operating machine | runnable | 1.0 | 38, 73, 2, 157 false, 63 false, 62 false, 89 false, 169 false, 93, 1 | | 123 | |
| 152 | Operating automatic machine | goal | Method 21 (F038636) | 2, 157 false | 62 false | | F038713 |
| 113 | Try emptying machine | goal | 1.0 | | 62 false | 63 false, 105 true, 66 false, 89 false | F038713 |
| 114 | Ditching to make space | goal | 1.0 | | 62 false | 82 true | F038713 |
| 92 | Collecting resource (Task_EmptyMachine) | runnable | Method 22 (F038709) | 82 true, 1 | | 66 true, 63 false, 169 false | |
| 93 | Filling machine (Task_FillMachine) | runnable | 1.0 | 2, 67, 169 false, 1 | | 62 false or 63 true | |
| 94 | Try filling machine | goal | 1.0 | | 66 true | 67 true | F038633 |
| 124 | Hauling to (Task_FillMachine) | runnable | 1.0 | 2, 67, 1 | | | |

The task factory (F030716) makes 93 and 124 a Task_FillMachine, 92 a Task_EmptyMachine on a
building (+0x78 clear) and another class on a ground item.

## The right-click menu (F044603)

A building with a hauling object: a storage (+0x3b0 null) gives Hauling to (0x7c, 124); a machine
of no operators (+0x3ac zero) Operating automatic machine (0x98, 152); another Operate (0x57,
87). F044568 labels 0x7c and 0x98 "Auto-Haul". As a job, 152 takes the follow-up 124 (F036821).

## Inventory methods (Building +0x2f8)

| Slot | Machine (ProductionBuilding) | StorageBuilding |
|---|---|---|
| +0x4a0 | F029329: its ingredient's container for the item not full | F029328: room, and kept (none kept: of its category) |
| +0x4a8 (62) | F029332: an ingredient used, its container empty, nothing banked | F029373: the typed container under its limit (none: room) |
| +0x548 | F029350: the output's item while it holds some | F029349: its items by type (none when kept) |
| +0x550 | F029371: the output's item | the typed container's, else the category's (2, 3) |
| +0x558 | F029265: the output's count | the typed container's |
| +0x560 | F029353: ingredients used whose container is empty | F029352: its kept items while the typed container has room |
| +0x568 | F029351: ingredients not full | F029372: +0x560, the category's item, typed containers not full |
| +0x578 (63) | F029267: the output's state 1 | F029266: false |
| +0x580 (105) | F029269: the output empty | F029268: the inventory empty |
| +0x588 / +0x590 | its ingredients | the typed container, then its list |

## Conditions

- 62 F038534, 63 F038677, 105 F038679: the methods above.
- 66 F038535: I carry (Character +0x178, my own inventory) the target's +0x550 item.
- 67 F038703: of category 4 (machines, item storages), one of its containers I carry that is not
  full (my last food kept back from one of "item function" 3); a weapon or armour storage, a
  thing of its category; no hauling object (a research bench), something its inventory takes.
- 82 F038628: room for the target's +0x550 item, else Raw Stone.
- 89 F038680: of category 4, carrying its +0x550 item (not my last food) with a building for it
  (F038621); else a thing of its category.

## Finders, searches and scores

- **F038621 (the building for an item):** the player's buildings with a hauling object, built,
  not the excluded one, that take it in (+0x4a0), the nearest within the AI's area (mode 2:
  4.24673e7 squared).
- **F038708 (F038713's with what I carry; F038709's without):** its items (carried; a storage
  target's held items; a machine target's output I carry or have room for; a character
  target's "death items" of 3979-gamedata.base), less the materials of an Engineer goal
  followed; the machines of my goals and the target that take one in are candidates and keep
  that item from the storages; for the rest, F038621's building (not while getting up,
  Character +0x278); the nearest.
- **F038632 (item search):** of the items lying loose within 1200 of me, the first of it I may
  take (F050233: in my squad's town of the player's one not in a foreign building, else one of
  no owners or my faction's), as the planner's `Target::Item`; else the nearest StorageBuilding
  holding the item (its typed container not empty), else the nearest machine whose output holds
  it. Collecting resource (92, and 247) on a loose item is Task_Pickup (F031289): picked up into
  my inventory or backpack, one in a foreign faction's building stolen (the crime, the item kept
  as theirs, thieving trained), the pick-up sound; none fitting, "I can't pick that up".
- **F038633 (94's finder):** the target's +0x560, else +0x568; the goal's building (the task
  system's own target) holding one, else the item search for each in order.
- **F031335 (Task_EmptyMachine's count):** one if I carry no food and it is food; for each
  building about that takes it in, the container of the item: its limit less what it holds (no
  limit: one if it has room); a container of another item in a building whose inventory takes
  it: 5 at once.
- **F038709 (Method 22):** room for the target's first +0x548 item: 1 when my squad has no town
  (Character +0x78), else 1 when F038708 finds a building; else none (0.1 for a crafting bench).
- **F038636 (Method 21):** 1 with an operator slot for me (F023204) or the machine full; else 1
  carrying one of its +0x568 items or with F038633 finding a source (through the AI's search
  cache F039866); else none.

## Executors

- **Task_EmptyMachine (F031048):** of the target's +0x548 items, the first with F031335's count
  above none; as many taken as the count while one more fits me; the state 1 -> 2 for a
  ProductionBuilding (type 4).
- **Task_FillMachine (F031179):** for my inventory then my backpack: a machine's containers
  filled one by one (F031147); a StorageBuilding given my things of its category in order,
  stopping at the first it does not take (F031167); a building with an inventory but no
  hauling object given everything (F031146). Nothing put in: the task ends (F036886).

## Plans

- **An operator of a machine with an empty input:** 87's 62 fails -> 93 at the machine -> 67
  fails -> 94 (F038633: the storage holding the input) -> 92 at the storage (as many as the
  input has room for) -> 93 fills the machine -> 87.
- **An operator of a full machine:** 63 fails -> 92 at the machine; then 89 -> 113 (F038708: a
  building that takes the output in, a machine of my goals first) -> 93 there.
- **The Auto-Haul job on an automatic machine:** 152 while it is full or wants an input found;
  its follow-up 124 feeds it (94 -> 92 at the storage -> 124 fills it). The order alone stops
  once the machine is neither full nor wanting: what was collected is ditched by the ditch habit.

## Still the old way

- The crafting benches (CraftingBuilding, F029659 and its methods) and the furnaces (F029393)
  keep their storage grid; their containers are the next step. Farms have their containers
  (water in, crop out) and their own "It's empty." (F022884: only while growing) and full
  (F022880: not while dead).
- The generators burn their fuel in `power_tick`, from their input container first, then the
  player's storage (their operate, F029326 with F029359's +0x538 factor, is the next step).

## Town NPCs

No AI_PACKAGE, character or faction goal list uses 92, 113, 124 or 152. The check was a
`goal_users` run, which finds 157 in town guard packages, so the tool works.
