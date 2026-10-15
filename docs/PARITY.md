# Feature parity with Kenshi

A checklist of Kenshi's player-facing features and how far Project Okran has them.
`[x]` done, `[~]` partly done or a stand-in, `[ ]` missing. The score counts a done
feature as 1, a partial one as 0.5 and a missing one as 0, every feature weighing the same.
Run `python tools/parity.py` to recount after editing.

<!-- parity:score -->

**Overall: 97%** (132 done, 9 partial, 0 missing of 141 features)

| Section | Score | Done | Partial | Missing |
|---|---|---|---|---|
| World | 95% | 20 | 2 | 0 |
| Characters and health | 100% | 17 | 0 | 0 |
| Combat | 97% | 15 | 1 | 0 |
| Orders and AI | 92% | 11 | 2 | 0 |
| Law, crime and slavery | 96% | 11 | 1 | 0 |
| Dialogue and factions | 96% | 11 | 1 | 0 |
| Economy | 100% | 5 | 0 | 0 |
| Your base | 96% | 11 | 1 | 0 |
| Animals | 100% | 4 | 0 | 0 |
| Interface | 97% | 17 | 1 | 0 |
| Saves and mods | 100% | 4 | 0 | 0 |
| Sound and graphics | 100% | 6 | 0 | 0 |

## World

- [x] The whole world map, streamed zone by zone, with its terrain and textures
- [x] Towns, outposts and ruins placed from the game data
- [x] Map features (features.dat, F032046: the canyons' cliff blocks, rock slabs and cogs, statues, wrecks; 1821 of them) placed by zone with their meshes, getObjectMaterial's texture mode and collision; (assumption) "local coordinates" and the draw distance not read
- [x] Day and night, sun and sky
- [x] Weather visuals (rain, dust, fog) by biome and season
- [~] Weather damage: acid rain, acidic ground and water, poison gas and burning clouds hurt; a dust storm's dust (less the gear's protection) is kept and shown as Kenshi's F040451 keeps it (MedicalSystem +0x14c), but no reader of it was found: what it spoils is not ported
- [x] Water and swimming: shallows, wading and swimming by depth, swim speed and skill, floating at the surface
- [x] Foliage and rocks
- [x] Buildings with interiors, doors, stairs and floors
- [x] World states and town overrides (towns destroyed or taken over)
- [x] A town changing hands: its people change the next time the player comes (re-peopled out of sight)
- [x] Town population: squads made as Kenshi makes them (leader, random members, counts), bar squads by number and chance, bar seating, regenerating squads; residents and bar crowds set up once per town as Kenshi's Town::setupResidents is (F048906's done flag), a town's zone loaded once
- [x] Roaming squads, caravans and patrols travelling the world
- [x] The road network (RoadManager): the level's 2109 roads between their junctions; a mover sent over 10000 units (1000 for a character) takes the roads when the walks to and from them plus the way along them times its faction's road factor (1 - "road preference") is no longer than the straight way (F040644), else goes straight; an A* over the junctions (F034412), the walk marker to marker (F034400); a squad away from the player follows the markers, a caravan walks them on the navmesh (CharMovement, F040650/F040653); (stand-in) a way whose next marker is off the loaded navmesh waits there
- [x] Wandering squads round the player as Kenshi spawns them: the biome's homeless list (patrols and others that hang on world states, like "Phoenix alive", only while those stand), the nearby town or camp's own roaming squads (wolf dens, bandit camps, slaver camps), the install's "Global population multiplier"
- [x] Faction campaigns on the player (FactionWarMgr, F050818): scheduled campaigns (a line's "trigger campaign", victory / loss follow-ups, "trigger player ally") picked by one roll over their absolute chances and set out when due (put off while there is no target, dropped when world states or special leaders fail); every 6 hours a faction with none running nor scheduled sets out at most one territorial campaign, picked by chance (chance per day x 0.25, range scaling from its nearest town, the options' raid frequency, population minimum, base level, repeat limit in days); special leaders (any one alive and free) lead with their own squad; "{1} has arrived at {2}", "Warning: {1} moving towards {2}"
- [x] No off-screen battles or NPC town raids (as in Kenshi: towns change only by world states)
- [x] Nests and camps: laid out by Kenshi's own generator (its seeded rand, centres, buildings and loose debris); a biome with "nests at fixed markers only" (Fog Islands, Watcher's Rim) raises its nests on the level's nest markers, none within 2500 units of a town or another nest (F048115, F048808, F048793)
- [~] Loot: buildings' item spots fill and refill 24-48 hours after they are taken (a town's are stealing); a town's ground loot still spawns once
- [x] Artifacts and ruins loot
- [x] Discovering towns on the map
- [x] Birds: flocks of each bird type by the biomes' numbers per zone, spawned, flocking behind leaders, circling, drawn to attractors (ruins, Leviathans) and turning back at their biomes' edge as Kenshi's BirdManager does; map features' attractors (those of the zones asked for); drawn with HWBirds (Object_FP)

## Characters and health

- [x] Races, looks and bodies from the game data
- [x] Stats and skills, trained by doing
- [x] Body parts, wounds, bleeding, knockouts and death
- [x] First aid and medical kits
- [x] Splinting broken limbs (Task_FirstAidRig: the splint, the part's +0x4c support, grows at (1 + 14 x level / 100) x "medic speed mult" a second up to min(medic skill, kit quality) + 1 and to the part's stun and treatment, on any part not vital once bandaged (F040455, F040439), one limb at a time, the kit's charges draining; it counts in the limb's health and wears off as the limb heals past 50, F040473)
- [x] Robotic limbs
- [x] Losing limbs
- [x] Hunger for the player's squad; eating as Kenshi's autoEat (F039268, F039117, F039189): a bite (at most 50) when nothing is left to digest and the hunger is under 2.5, NPCs only under 2.1; Share Food from a squad mate's backpack within 200 units under 2.25
- [x] Hunger for NPCs: everyone's falls; an NPC's stomach refills it to its cap (F039188): its squad's level (full, or 100-220 for malnourished squads such as the starving bandits and the Rock Bottom start), 200 for one held by another faction, none for the player's people and the player's own prisoners and slaves (they must be fed); starving (under 170) in dialogue
- [x] Encumbrance and carrying people
- [x] Knockdowns and ragdolls; a disabled limb hangs limp (F038923: an arm from the shoulder, a leg's thigh and calf with the foot kept to the animation, the head and neck); (assumption, the callers not found) a part at no health sets its flag
- [x] Playing dead and standing up
- [x] Sleeping in beds and resting to heal
- [x] Character creation and the character editor; the name box edits the name it shows (it used to type on after the layout's "NAME"), with MyGUI's blinking cursor and its MaxTextLength (30)
- [x] Saving and loading characters in the editor (EXPORT / IMPORT; the rewrite's own files, not Kenshi's)
- [x] Bone scales and face morphs, for everyone: the Body sliders size the bones (F037414) with Kenshi's muscle (the highest of Strength and the two smithing skills, F037377) and thinness (the hunger level's: the hungry look thinner); the Face sliders' poses are baked into each head (F021065); an NPC without a body file rolls its sliders as Kenshi does (F021064: the middle plus or minus the spread, random groups on one roll, "appearance random deviation percentage"), a named one has its body file's; the editor's RANDOMISE rolls the same way
- [x] Personalities

## Combat

- [x] Melee combat, blocking, techniques and animations
- [x] Ranged weapons and crossbows (aim at Bip01 Spine2 led by the target's velocity, an animal's Bip01 Spine1, F033844; bolts meet the characters' physics hulls, the race's hull size, F040654; the ground and the buildings' solids block the line of fire and stop bolts; a bolt's victim trains and staggers, F033863; stuck bolts kept to the options' Harpoon limit; the aim-mode animation category 8 for the crossbow stance; turrets choose targets by F033823's scores, their damage and turning by the building's output rate with research, F029301; the ranged skills by the body's fitness, F047040)
- [x] Melee as CharStats has it: the attack skill (attack bonus + melee attack, F047043), the melee defence (F047044) choosing the right block (F047063) and the dodge stat rolling the block (F047056, F047048); unarmed fighters dodge (F047075); damage by the body's fitness (F040433) and strength, against the victim's kind and race (F047066); every enemy in the swing box up to the move's "max simultaneous hits" (F039803); punch feedback (F040731); trainFromCombat (F047467) with the dodged event; collapsed legs fight prone with the desperate attack (F040415, F040452); animals' size, maturity and HP mult (F040054), their weapon skill (CharStatsAnimal), their lunges' root motion
- [~] Turrets: a town's people man its turrets through their packages' jobs Man the turrets (149, F047566: a free turret of the town they are in) and Turret Gunner Homebuilding (222, F047567: of the home building), scored by F038553 (2 with enemies about, 0.4 when hungry, none beyond 1000 units or passive) and planned into Turret gunner (146, Task_UseTurret) after walking there; a turret another's plan holds is passed over; (assumption) the finder's search taken as the nearest, AI +0x28 as the squared vision range; (stand-in) power taken as full
- [x] Building parts that move by themselves (F037803, F037682, the wind listener 0x140549c60): "rotation function" 1 spins at its top speed (x 0.95-1.05), 2 by the machine's work rate, 3 by the wind between its wind min and max, 4 a vane turned across the wind; windmills, wind generators, fans and machines
- [x] Unarmed and martial arts
- [x] Stealth, sneaking and stealth knockouts
- [x] Assassination
- [x] Squad threat scoring: Kenshi's target scoring (F038638) with the stances (taunt draws attackers, passive less wanted, hold keeps near), the player's leash (400, a crossbow's range in ranged mode), crossbow-aimers x3, getting up x0.1, crippled -10, +10 for one's own attack's target while under two attack it; the AI's ignore map (+0x218): one it can't get to (a path that fails, F044546 from Task_Move's update and the other moving tasks) is left alone 20 s, passed over by the scoring and the enemy finder (F038789), and let go once reachable again (F036917, F040601), else held 20 s more
- [x] Combat stances (BLOCK, HOLD, PASSIVE, TAUNT, RANGED on the main panel, saved): their parts in target scoring; block never steps in to attack from circling or waiting, only fighting back (F029599, F029602); hold keeps the spot where it was switched on (F039205) and gives the goal Attacking enemies within its hold leash (F038560); the hold point moves to where the character stands as a move, follow or looting order ends or it gets up (Task_FollowLeader::end F030762, Task_Loot_Order, Task_GetUp); Kenshi walks no holder back after a fight (its Holding position task, 30, is a Task_Blank no package or code uses); passive turns that goal off (F038552); the JOBS button (the layout's OrdersChaseButton, order 0xf: the task system's +0x35, F036819, which lets it weigh all its goals, F036915) turns a character's job list on and off; (stand-in, Kenshi's job list not decompiled) jobs are orders given with shift (operate or build a building, fill a storage with what it keeps of what is carried, train on equipment, attack or pick up someone, fetch items of a kind lying about and take them to a storage that keeps them), redone whenever the character is idle with no orders, listed in the jobs panel as Kenshi lists them (F043047: a Kenshi_OrdersPanelOrder row each, "n: name" in #afa68b, faded with JOBS off but for the jobs always done, F036879; a row's X removes its job, a row dragged onto another moves it, F036800 / F036810)
- [x] Weapon draw and sheathe timing: drawn at once (drawWeapon F039274; Kenshi has no draw animation) when an attack starts, so it is in hand on the way to the target; sheathed (the goal Sheathing weapon) a second after the last fight or attack task, put away with a body knocked out; (assumption) the one second
- [x] Training on dummies
- [x] Starting a fight: an attacker runs the whole way (a new plan used to drop it to a walk); its reach is known before its first swing (setWeapon ends with chooseAttack at 199, F047095, so tryEngage asks within the weapon's longest move); the last 7 m are closed at 18 units/s unless the target runs, as FormationSteering does (F029549, F029532); a fighter whose straight way to its target is blocked steers round by a navmesh path (CombatClass's path list, +0x200) and a step into a wall slides along it; a slope between two fighters no longer keeps them apart; the player's people not on hold take on whoever attacks their side (the Nameless faction's Protect allies goal; the Hold tooltip: one not on hold "will run off to fight enemies ... to protect his allies"); one ordered elsewhere runs there through the fight; only a turret's own town or building mans it (a passing bandit used to)

- [x] Towns' people keep to their own town: a bar squad to its own bar (the world's other bars were in the list), a node picked from those within 300 m of home (a guard sent to a post in a town 600 m off)
- [x] People stand on a building's floor however near the ground it is (a bar's floor a metre up had them sunk into it to the terrain)
- [x] Furniture collision: every piece of a town's furniture of "path mode" 1 or 2 (beds, tables, counters, storage, machines, lamp posts; 0, stools and rugs, are walked over) is cut out of the navmesh by its parts' collision, inside buildings too (their own navmesh), so people walk round it; (stand-in for the Havok character squeezing through) a spot people go to that tightly packed furniture would shut in keeps the furniture round it uncut; a step into a wall or furniture slides along it; a goal that just found no path is not asked for again for a second unless it moves (the path manager answers one request a tick)

## Orders and AI

- [x] Right-click menu orders (move, attack, pick up, kidnap, loot, trade, talk...)
- [x] Put in bed, sleep, put down, splint, cages, shackles, set free, stand up
- [x] Shoot at, attack all, auto-haul, throw out of town / home, corpse disposal
- [x] No pickpocketing of the conscious: Kenshi's menu (F044603) offers none, its "Steal" (F044568) is taking what an NPC faction owns (below)
- [x] Stealing from shop shelves and containers
- [x] Lockpicking doors, containers and shackles
- [~] Job queues (shift-orders): as Kenshi's (F039153 -> F036821) a shift order becomes a job only when its task can be one, in its job form (Building and Repairing -> Engineer, Attacking -> Attacking enemies; Operating machine, Training), a kind without +0x08 kept once; the JOBS toggle is the AI's free will (F036819) and a new job turns it on; Pick Up (Lifting, Collecting item) is no job, just the order; the player's people get their faction's AI Goals (Nameless: Protect allies, F040015) and the AI option Help Allies is that job's (the idle Help Allies errand is gone); (stand-in) jobs are carried out as their orders when idle, not planned, and Engineer's finder (the nearest unfinished or damaged building) is assumed
- [x] Follow and bodyguard: Task_Follow (Task_FollowLeader's start, update and end with the one followed as its target, F031269); a bodyguard attacks anyone fighting the one it guards, then follows again; (stand-in) one whose follow task is not taken up walks after them
- [~] NPC AI packages and goals: Home territory aggression (144, F038644: guards fight the known enemies within their town's radius x 1.3 or fighting them; Admag against a Holy Nation patrol: 13 defenders by it), Self preservation (32: fighting back whoever attacks me), Static town guard (157: a guard post of the town), Staying home / Reigning / Sitting around (43 / 255 / 275: furniture of the home or the building one is in used for show, the throne for the squad's leader, as Operating machine 221), NPC looting strips armour unless the looter's squad is hungry (Task_Loot_AI); Protect allies and Protect close allies run through the planner as Kenshi's do (15 / 143 -> Attacking attackers of, 13 -> Attacking, 4): their finders (F050285 / F050287) pick the known friendly most in need (F050268: 0.25 per attacker less 0.2, 0.01 in a fight or when its one attacker is itself fought), the close one of the own squad only; F038601's score (none passive or an obedient slave); the player's people the same score with F050517's falloff and the Help Allies option for other squads; isAlliesFighting (F050269) and the attackers-of finder (F050284: within 150 units of the ally); assumptions: Character +0x2b0 (isAggressionTowards) taken as in a fight, F050234's leave as granted, the squad's protect list not ported; Getting out of bed! (F038651: 2 attacked, 1.5 with enemies about; the intruder case not ported); the goals of every one of a squad's packages (an "addon-" one too) run as routines: Law enforcer (Attacking trouble makers, 34) and Arrest bounties (Bounty hunting, 204: only with a free cage of the faction, F038567 / F047589) are who arrests the wanted (no longer every armed faction member); Kidnapper (Find and kidnap with cages, 254) captures the knocked out like the slavers, none while two of the squad carry someone or with no cage (F038655); Watching slaves (183, F038543: the Cage Warden's, slave guards') stands an idle watcher by its faction's nearest slave (stand-in for its target finder and handler); Getting food (243, F038547: 1 for any but the player's) plans into Grabbing food (244), one food taken from the nearest container with food (F047552), then eaten as anyone eats (gotAFoodItem, F038549); a squad without a faction takes its zone's town's when its building stands outside the town; not ported: Retreating (a return home, F038670, for the raid and return packages)
- [x] Squad AI options: sleep when injured, sit when idle, help allies (the Protect allies job's), heal / rescue allies (as direct errands), share food, feed animals, turret shoot-first (F033850: neutrals under -30 with the player's faction, and (assumption) wild animals), the defaults the player object's (F045553: heal / help / rescue allies and shoot first off), formation (group moves), ditch items (an idle character's resources into the storage that keeps them)
- [x] Share food
- [x] Medic and Rescue orders (the orders panel's buttons)
- [x] Hiring mercenaries on contracts

## Law, crime and slavery

- [x] Crimes, witnesses and bounties: committing (F046495 / F046496: no crime by a town's guard, against one's own or disguised-as faction, or against "not real" / type-8 factions; a fight's or caught thief's crime blows the disguise), every NPC that sees it weighing it by crime and faction (F046520: its own event, ST_CRIMINAL, the crime kept 20 s), the bounty only from a town's guard of a faction that "offers bounties" (F046511: none for bandits, diplomats, law), carrying a slave or captive (F046519), assault in fights with town guards (F040724), picking locks and shackles as crimes (F031344 / F031345 / F031308 / F031346)
- [~] Guards arresting, prisons and sentences: the bounty a guard sees only while the disguise is blown (F046512), bounty hunters' targets (F050281), release with EV_PRISONER_FREE_TO_GO and an hour's grace (F031336); (stand-in) the arrest goals run as routines (their planner tasks 34 / 204 not)
- [x] Escaping cages and shackles: out of an open cage an escaped prisoner until out of town, a slave escaping (F031326, F038706), lifted out by the player's people (F031291), caged again a slave outside prison towns (F030719); the skill 10 less in a cage
- [x] Slavery: slavers, slave camps, work and escape; a CHARACTER with a "slave" state (the Slaves start's) begins as a slave (F040051 -> F038704), obedient to the nearest slaving faction's squad (assumption: Kenshi names no master); enslaved, a character has the permanent job "Obedient Slave" (F030705: job 187, shown in the jobs panel, cleared with the rest); an obedient slave its masters want caged walks to a free cage of theirs and gets in (Task_UseCage, F031221 / F031340: the Slaves start's "Get into a cage straight away to avoid a beating"); one left loose (its job cleared) is beaten by the masters' cage wardens (Capture Slaves, 167: its target shackled, F038662) and carried into a cage; the slave states (F038704..F038706: escaping 12 h, ex-slave 4 days, far from its owners outside town, owners gone or down: EV_SLAVE_ESCAPE_OPPORTUNITY_*), "Looks like slave" by the outfit (F038707), NPCs spotting slaves getting away and found-out escapees (F046547, F046526), new slaves processed (Task_ProcessNewSlave F031348: kit taken by the slaver, slave clothing, shackles by the boots section)
- [x] The selected character's extended stats (ExtendedStatsText, F047093): Athletics, Strength and Toughness XP, KO point, run speed, hunger rate, with their tooltips
- [x] Squads without a faction of their own take their owner's, the town's (F038271): Rebirth's garrison and others were factionless
- [x] Buying and selling slaves
- [x] Bail (the cages of the jailer's town: the player's people and NPC prisoners not hostile, under fighting level 36; twice the bounty, at least 2000 for NPCs; the freed say EV_BOUGHT_ME_FROM_SLAVERY)
- [x] Bounty hunters (their bounty spotted dialogues: attack, arrest to the bounty's faction); characters made with their CHARACTER's bounty ("bounty chance", amount and fuzz, "bounty factions" or the United Cities: the uniques' prices on their heads)
- [x] Guard "halt" checks and searches (the gate guards' dialogues: halt, the search for their faction's illegal goods, arrest and bounty)
- [x] Trespassing and private buildings: residents shoo the player's people from private homes, a home locked up for the night (or a shop after 22:00) makes them intruders with a trespassing bounty, and the knocked-out are carried outside
- [x] Disguises (wearing a faction's uniform): the outfit's disguise (F038707), passing over 0.6 (F044631, F046494), the observers' notice meters (F046537 / F046536: stealth, same faction, owners, knows-me, secure, away from home; blown 250 s, or EV_BOUNTY_SPOTTED past the class's threshold, F046483), uniform imposters (F046547: EV_I_SEE_UNIFORM_IMPOSTER, uniform theft)

## Dialogue and factions

- [x] Talking to people, conversations and replies
- [x] Kenshi's choice of what is said (line and dialogue scores)
- [x] Barks and greetings, crowd triggers, interrupts: a package lists a dialogue under up to three events; the sensors' new sightings (F046547: neutral, enemy, ally, animal, training equipment); getting up, lost limbs, freed from a cage, first aid start / finish / kit empty, a squad beaten, the weather's hurts, bought out; the dead raise none, the knocked out only torture and get-up; campaign announcements (EV_ANNOUNCEMENT with the campaign's dialogue added)
- [x] Recruiting; DA_JOIN_SQUAD_WITH_EDIT (3) opens the character editor on the new member, as Kenshi does (DA_JOIN_SQUAD_FAST, 18, does not)
- [x] Faction relations, allies and enemies (the player's faction's list is the listed factions' feeling toward the player, both ways, as PlayerFactionRelations reads it); a squad takes up its members' fights
- [x] Reputation and faction ranks
- [x] War and peace declarations
- [x] Short-term and long-term memories of the player
- [~] Dialogue conditions (death row reads false, as in Kenshi: F040880 has no case for it; within town walls is the town's radius, not Kenshi's walls area)
- [x] Dialogue effects: DA_CLEAR_AI (F040912 case 0xf: the speaker's AI begun afresh), DA_THUG_HUNTER (0x11: task 165 "Stalking" on the one talked to: followed until out of town, then attacked; the handler assumed from the lines), DA_MAKE_TARGET_RUN_FASTER (0x17: the one talked to runs); DC_TOWN_WALLS_LOCKED_UP (the gates about all shut); (selling prisoners and surrendering a comrade of another race do nothing, as in Kenshi: their one line each is in the unfinished cannibal diplomat dialogue, and F040912 has no case for them; merging squads has no lines)
- [x] Intruders spotted in buildings (EV_INTRUDER_FOUND / EV_SHOO_FROM_MY_BUILDING from the game's dialogues)
- [x] Unique characters and world events (world states as F050361: "player involvement" from the player's hand in a unique's death or captivity, kept as a bounty is handed on; factions' own view of the player; "town okay" not read, as in Kenshi)

## Economy

- [x] Trading with shops, prices, trade culture and illegal goods
- [x] NPC shopping: each character's shopping list from its record's "shopping" references (F050305), reset daily after 7 and in a new town; the packages' Shopping goal (civilians, farmers, bar-goers) goes to a random shop counter of the town that stocks a wanted item function (F050263) and buys one item a second there (Task_Shopping, F050309 / F043583: a shop not the player's keeps the money and, four times in five, the item)
- [x] Weapon and armour prices from material, maker and quality (shops' weapons by their lists' makers; F044582's half value is for items stolen from the player, and items carry no owner here)
- [x] Faction uniforms worth 0.3 of their value (Armour::value)
- [x] Money and items in inventories, backpacks (a worn backpack's own grid, opened with Open bag; its contents at its encumbrance effect's weight; its combat, speed and stealth modifiers; not its stacking bonus)

## Your base

- [x] Building and placing buildings
- [x] Walls and gates
- [x] Power: generators, batteries, fuel
- [x] Farms and water
- [x] Mining and prospecting
- [x] Production, refining and crafting
- [x] Research
- [x] Storage
- [x] Hauling: machines' output carried to storage; builders fetch materials from the nearest storage, as many as they can carry, add them to the site and build (Kenshi's Finding building mats / Adding materials / Building)
- [x] Open to public: visitors at player outposts are the factions' visit campaigns (ported); the flag itself feeds only the wandering traders' town finder (F048860, types 2 3 5 6), which never picks a player town (type 1)
- [x] Town panel and outpost size
- [~] Raids on player outposts: the factions' campaigns march on the base and fight; the leader makes the campaign's "dialog announcement" (its assembly package's "Diplomat mission": the bandits' demands) to the player's nearest on arriving, or as it comes near when it "can talk before arrival"; hostile ones bash the player's shut gates in their way ("assault player town gates"), free their faction's people the player has caged ("liberate captives") and go for the player's people; won, slavers and cannibals carry the knocked out off home (caged or shackled there), occupiers stay running their victory package's jobs (Getting food takes the base's food), food raids and the Shek's tributes run Looting food (131 -> Looting, 284, F047552's search: (assumption) the nearest container with food within 3000 units) and leave once they carry it; going home they carry their knocked out ("Pickup downed leader"); (stand-in) "run around town looking for people" is going for the player's nearest, wherever

## Animals

- [x] Wild animals and nests
- [x] Animal combat
- [x] Pack animals and animal companions: animals' own grid and worn bags (a pack bull's 20x20 at a tenth of the weight) in the animal window, the bags' icon on their slot
- [x] Buying animals through dialogue (the animal traders' Buying Animals window, priced by age)

## Interface

- [x] Game starts as Kenshi offers them (F046699): the base game's own (a mod's starts left out), each begun at its forced position or one of its towns at random (no town: an open town of type 2 or 3), the squad on the navmesh there
- [x] Main panel, portraits, squad list and status; the INV and STA buttons toggle their windows (a pretend key press used to stay held, so a second click did nothing)
- [x] Inventory, equipment and trading windows
- [x] Tooltips as Kenshi's ToolTip classes (F048667 - F048706): the game's tooltip in Kenshi_GenericTextBoxSkin a quarter of the screen wide after the mouse rests half a second, its bottom right at the mouse; the main panel's in its TooltipPanel over the portraits (ToolTipFixed), with the game's texts for the orders, shortcut, build and floor buttons (F043076, F043077); an inventory item's beside it at once (ToolTipInventory, Kenshi_FloatingPanelLight); a line a left and a right text in the game's colour strings (#afa68b, #140806, #492620, #59231a, #a8b774); the map's towns (F048704: name, faction, relations, residents heading)
- [x] Item tooltip lines (armour weather protections as F044593 lists them)
- [x] Map, faction and squad overview; several squads (crate::squads): NEW SQUAD up to GLOBAL CONSTANTS "max squads" (F035111), DELETE only an empty squad and never the last (F035112), a portrait dragged to another squad while it is under "max squad size" (F035150), or to DRAG HERE TO DISMISS ("Dismiss character '{1}'?", the last one kept: F035103 makes it a Tech Hunter in a "Wandering Squad"), squad and faction names typed (F035090, F035125); a tab per squad over the portraits (the shown one follows the selection, a tab selects its squad, Tab cycles them); saved, and written to Kenshi's format one squad per squad
- [x] Research and crafting windows
- [x] Build menu
- [x] Options and key bindings as F032960 makes them: its rows, ranges (100 steps a unit), choices and tooltips, Reset Tutorials, the compositors.cfg ticks, the resolution and screen mode (kenshi.cfg), the key bindings' list with its Build mode and Tools sections and Defaults, a Mods tab of the active mods; the main panel's keys (stances, jobs, medic, rescue, prospect, floors, next / previous character, toggle_bar, toggle_faction) (stand-in: choices step on a click instead of a drop-down list)
- [x] The camera's rotation marker (rotate.mesh, F041487 / F041486): shown while turning with the mouse, over the camera's centre, scaled with its distance; it and the movement marker follow their options ("Show rotation marker", "Show movement marker"). These are Kenshi's only green circles: it has no ring under selected characters (no mesh, texture, decal type or shader for one), so the rewrite's debug rings (green selected, red fighting, grey down) are gone
- [~] Tutorials (the tutorials window's list scrolls with the wheel; the pages' pointers at the GUI are not shown)
- [x] Speech bubbles and messages
- [x] The combat stats box as F047092 writes it: by the stats' mode (F047026) Martial Arts (no weapon), Crossbows (a crossbow in the ranged stance) or Attack and Defense, then Strength, Dexterity, Toughness and (not with the crossbow) Dodge; a bonus "(+N)" in green, a penalty in red; the block stance's +20 defence; the run speed in red whenever it differs (F047093)
- [x] The inventory window's info box (F039272 mode 15): total weight against what can be carried, Overloaded / Heavy / Moderate / Lightweight / Weightless by the load factor, disguise, slave of, looks like slave (stand-in: the disguise's percentage and breakdown)
- [x] The new game panel's Difficulty, Cash and Play Style lines (F048538); ADVANCED OPTIONS
- [x] The map's camera marker (CameraMarker, turned with the camera) and the player's markers in "MarkerPlayer" blue; a picture-only button shows no caption (the centre button's stray "O"); a scroll view's bars hidden (MyGUI shows them only for a larger canvas)
- [x] The faction tab lists only the factions met (the player's relations map, F035139); its info has the Active Contracts and Upcoming Events (F035126)
- [x] Names: the player's people and named NPCs get Kenshi's random names (F040393, F040051); the selected one's tag in brackets (F042119); portraits and the inventory title by name

## Saves and mods

- [x] Saving and loading; a save loaded or a new game begun from inside a game replaces it in the program (the game unloaded first), no restart
- [x] Importing Kenshi saves
- [x] Writing saves in Kenshi's format: every save also writes one in Kenshi's layout (quick.save, platoon and zone files) over a real Kenshi save, the one the game was imported from, else the newest of Kenshi's own (read only), so all the rewrite does not model (towns, wars, biomes, trust, handles, missions, tags) is a real save's; the player's squads, characters (handles Kenshi's TYPE 1 with the squad's), stats, wounds, looks, items with their bags' contents, money, time, research and relations are the rewrite's (crate::kenshi_save_over; the save_diff example finds every record type and field Kenshi writes). (not written: the player's buildings, which stay as the base had them; untested in Kenshi itself, as nothing is written into its save folder: copy the save's folder there to load it)
- [x] Mods and Steam Workshop mods

## Sound and graphics

- [x] Sound effects and music (Kenshi's Wwise banks)
- [x] Footsteps and the other animation sounds (swimming, mining and farming impacts) from the animations' own events (ANIMATION_EVENT "event", its override switch), footsteps by surface (the biome's ground type for the terrain layer underfoot, water, a building's interior or exterior material, shod or barefoot names, F040611/F051635) and armour
- [x] Lighting, shadows and materials
- [x] Particles and blood
- [x] Rendering details: the interior mask (terrain, map features, grass and foliage cut inside buildings' interiors; building parts' own CLIP_INTERIOR left off, see object_material.rs), lightning aimed at the nearest town; no FOLIAGE_BUILDING, effect light or effect sound is used by the game's data; the distant terrain is the ported LOD quadtree with its neighbour rules (a split splits neighbours more than 2 levels coarser, a merge waits for neighbours 3 deeper); (stand-in) skirts hide the cracks the plugin leaves between levels; lights' PULSE and SHIMMER as the deferred light pass scales them (F030155, FLICKER does nothing); switched-off campfires and lamps (zone "power on") unlit, without their fire
- [x] Texture quality and terrain detail options as Kenshi applies them: "texture resolution gimping" drops DXT textures' top mip levels on load (F046209, F046205), "terrain detail" / "terrain distant" / "terrain threshold" set the terrain's detail threshold and its falloff with distance (F045916, the plugin's error); (assumption) data/newland/land/textures counted as the General group

## Known bugs

- Video memory: about 4.5 GB at the Hub on the default options (Texture Quality Medium,
  Terrain Detail 1), 3 GB at Fugly / 0.6; the terrain's colour and overlay maps (about 900 MB)
  are kept uncompressed as Kenshi keeps them.
- Unconfirmed against Kenshi's code: town level, bail price, how long short-term memories last,
  how long interrupts last.
