# Animations: what Kenshi plays and what the rewrite plays

Every clip Kenshi plays comes from an ANIMATION record (people), an ANIMAL_ANIMATION record
(animals) or a COMBAT_TECHNIQUE (fight moves); the tables list each, where Kenshi triggers it
(the function, F-number) and whether the rewrite plays it: "before" is the state before the
animation audit (commit 9480dab), "now" this branch. "yes" means the rewrite's code reaches it
the way Kenshi does; "no" is listed with what is missing. Regenerate the record data with
`cargo run --release -p kenshi-anim --example anim_audit -- <install>`.

How Kenshi picks them: AnimationClass::updateBase (F037184) takes the situation's category (0,
4 swimming, 5 crouched / prone, 8 aiming) and, moving, blends the movement records by speed
(F037154: upper layer strict, lower loose), standing, the special named animation (a task's) or
the best idle (F037155, F037213); painPosture (F037181) adds a task's named overlay, the weather
animation and the worst hurt part's pain posture; breathing (F037162), head turning (F038922)
and strafing (F037179) are overlays. Tasks play their own clips by name (the Task_* functions
below); fights play the techniques on the action layer (F038914) and the stumbles (F037176).

| | records | played before | played now | disabled |
|---|---|---|---|---|
| ANIMATION | 124 | 79 | 95 | 7 |
| ANIMAL_ANIMATION | 45 | 26 | 36 | 0 |
| COMBAT_TECHNIQUE clips | 30 | 30 | 30 | 0 |

Skeleton clips no record, technique or code names (Kenshi never plays them): aimcrossH, arrow
dodge spin 2, block up upper, combat stance ref, combatwalk, coolstance, crawling-low, drunken
walk, flying big chop 3, gotobowing, gotoeating, gotouse, handtowounds, handtowoundsR,
idle_turnhead, idle_weapon_confident / guard / normal / relax / stiff / timid, kneeling hostage
kneel / stand, limp 1 full, newstrafe, ref08, shoulder lift upper walk, sidestep 2, stance 3,
stance2, stances, stances 01, strafe 2, strafe upper, swim, swim idle, swimbash.

Every humanoid race (Greenlander, Shek, Hiver, Skeleton, Fishman...) uses the male or female
skeleton, so the people's tables cover them all.

## ANIMATION records (people)

| record | clip | category, layer | Kenshi plays it | before | now | note |
|---|---|---|---|---|---|---|
| MA idle1 | ma idle1 | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode, unarmed | yes | yes | the arena idles |
| MA idle2 lower skill | ma idle2 | 0, upper | disabled (sortAnimations F038935 skips it) | disabled | disabled |  |
| aimH | aimshoot | 8, all | aiming a crossbow (F037165) | yes | yes |  |
| aimwalk | aimwalk | 8, upper | disabled | disabled | disabled |  |
| back blow high | back blow | 0, all | stumble (pickStumble F037176, onHit F033861) | yes | yes |  |
| back blow light | back blow light | 0, all | stumble (F037176) | yes | yes |  |
| back blow low | back blow low | 0, all | stumble (F037176) | yes | yes |  |
| bowing | bowing | 0, all | Task_WorshipTarget (F031242) | no | no | no worship task in the simulation |
| breathing | breathing | 0, overlay | overlay (F037162) | yes | yes |  |
| breathing noarms | breathing noarms | 0, overlay | overlay with no arms (F037162) | yes | yes |  |
| carry me | carry me | 3, upper | being carried (placeCarried F038904): the body hung at the carrier's "Bip01 L UpperArm" plus a height offset, turned by the bone (F039240); the race's ragdoll holds every body to it but the forearms' and the head's, which hang free (F038923, part flags 0x800) | yes | yes | before: a carry ragdoll pinned at its root on the clavicle |
| carrying left arm only | carrying left arm only | 0, overlay | carrying a body: F039254 marks the arm busy (+0x21e), painPosture F037181 overlays it by name at any category and speed (swimming too) | no | yes |  |
| carrying left arm only -swim | carrying left arm only | 4, overlay | none: painPosture asks for "carrying left arm only" by name (F037181), so swimming carriers play that record | no | no |  |
| carrying right arm only | carrying right arm only | 0, overlay | the same for a body on the right (+0x21f) | no | no | the rewrite carries on the left |
| chop down static | chop down static | 0, upper | BUILDING_FUNCTIONALITY "animation" (sword training); Task_KillPrisoner (F031310) | yes | yes | killing prisoners: no such task |
| combatstance | combatstance | 0, overlay | turret training "animation"; Task_UseTurret (F031217) over the turret's "stand 1" | training only | yes | the gunner overlay added |
| crawl idle down | crawl idle down | 5, lower | prone idle (category 5, F037184); getting up prone (playGetUp F037164) | legs at 0 (assumption) | yes | collapsed legs (F040452) |
| crawl idle up | crawl idle up | 5, lower | prone idle (category 5) | legs at 0 (assumption) | yes |  |
| crawl use | crawl use | 5, all | working prone (F037185 / F037186); a bedroll's "animation medic" / "animation lockpick" | no | yes |  |
| crawling move | crawl loop | 5, lower | prone movement (category 5) | legs at 0 (assumption) | yes |  |
| crouch run upper carrying | jog carrying left | 0, upper | movement (playBlended F037154 by speed, F037145), sneaking, carrying | yes | yes |  |
| crouch walk lower | crouch walk | 0, lower | movement (playBlended F037154 by speed, F037145), sneaking | yes | yes |  |
| crouch walk upper | crouch walk | 0, upper | movement (playBlended F037154 by speed, F037145), sneaking | yes | yes |  |
| crouch walk upper carrying | crouch walk | 0, upper | movement (playBlended F037154 by speed, F037145), sneaking, carrying | yes | yes |  |
| eat | cannibaleating | 0, all | Task_EatingCrops (F031241), Task_EatPrisoner (F031312) | no | no | no such tasks in the simulation |
| foetal | foetal | 0, all | a cage's "animation KO", at rate 1 (updateBase F037184) | yes | yes |  |
| generic use | use | 0, all | BUILDING_FUNCTIONALITY "animation" (Task_OperateMachine F031350) | yes | yes |  |
| gotobed | gotobed | 0, all | lying down in bed; Task_GetOutOfBed (F031237: at speed -1, done below 0.1) | no | yes |  |
| guard 1 | stance1 | 0, upper | disabled | disabled | disabled |  |
| guard 1h | guard 1h | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode | yes | yes |  |
| guard 3 | stance3 | 0, upper | disabled | disabled | disabled |  |
| guard katana high | guard hi | 0, upper | disabled | disabled | disabled |  |
| guard polearm | guard pole | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode | yes | yes |  |
| guard4 main | guard4 | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode | yes | yes |  |
| guard5 | guard5 | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode | yes | yes |  |
| guard6 | guard6 | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode | yes | yes |  |
| hand2chest | hand2chest | 0, overlay | pain posture (F037181, LOCATIONAL_DAMAGE "pain anim") | yes | yes |  |
| hand2chest ext | hand2chest ext | 0, overlay | none found (no reference in data or code) | no | no |  |
| hand2chest-R | hand2chest-R | 0, overlay | none found | no | no |  |
| hand2head | hand2head | 0, overlay | pain posture | yes | yes |  |
| hand2head-R | hand2head-R | 0, overlay | none found | no | no |  |
| hand2leftshoulder | hand2leftshoulder | 0, overlay | pain posture | yes | yes |  |
| hand2ribs | hand2ribs | 0, overlay | none found | no | no |  |
| hand2ribs-R | hand2ribs-R | 0, overlay | none found | no | no |  |
| hand2shoulder | hand2shoulder | 0, overlay | pain posture | yes | yes |  |
| hand2stomach | hand2stomach | 0, overlay | pain posture | yes | yes |  |
| hand2stomach-R | hand2stomach-R | 0, overlay | none found | no | no |  |
| head turning | head turning | 0, overlay | overlay (F038922) | yes | yes |  |
| idle_stand_confident | idle_stand_confident | 0, all | idle stance (appearance, F038938; the character editor) | no | yes | before: a weapon was always taken as drawn |
| idle_stand_guard | idle_stand_guard | 0, all | idle stance: brave (F021063) | no | yes |  |
| idle_stand_normal | idle_stand_normal | 0, all | idle stance: the default (F021063 / F021090) | no | yes |  |
| idle_stand_relax | idle_stand_relax | 0, all | idle stance (editor) | fallback only | yes |  |
| idle_stand_stiff | idle_stand_stiff | 0, all | idle stance (editor) | no | yes |  |
| idle_stand_timid | idle_stand_timid | 0, all | idle stance: fearful (F021063) | no | yes |  |
| jog carrying left | jog carrying left | 0, upper | movement (playBlended F037154 by speed, F037145), carrying | yes | yes |  |
| jog carrying right | jog carrying right | 0, upper | movement (playBlended F037154 by speed, F037145), carrying on the right | no | no | the rewrite carries on the left |
| jog lower | jog lower | 0, lower | movement (playBlended F037154 by speed, F037145) | yes | yes |  |
| jog lower combat fallback | jog lower | 0, lower | movement (playBlended F037154 by speed, F037145), combat mode | yes | yes |  |
| jog lower limp L | limprunL | 0, lower | movement (playBlended F037154 by speed, F037145), a hurt leg (rangeWeight F037142) | yes | yes |  |
| jog lower limp R | limprunR | 0, lower | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| jog upper | jog upper | 0, upper | movement (playBlended F037154 by speed, F037145), no weapon in hand | no | yes | before: a weapon was always taken as drawn |
| jog upper combat fallback | jog upper | 0, upper | movement (playBlended F037154 by speed, F037145), combat mode | yes | yes |  |
| jog upper limp L | limprunL | 0, upper | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| jog upper limp R | limprunR | 0, upper | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| jog upper sword | jog upper sword | 0, upper | movement (playBlended F037154 by speed, F037145), weapon drawn | always | yes |  |
| kneeling hostage idle | kneeling hostage idle | 0, all | none found | no | no |  |
| knockout | stealthKO | 0, all | Task_StealthKO (F031201 / F031279): the grab | down pose (stand-in) | yes | the knocked out go limp (ragdoll) |
| knockout training | stealthKO | 0, all | assassin training "animation" | yes | yes |  |
| limp L lower | limp L lower | 0, lower | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| limp L upper | limp L upper | 0, upper | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| limp R lower | limp R lower | 0, lower | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| limp R upper | limp R upper | 0, upper | movement (playBlended F037154 by speed, F037145), a hurt leg | yes | yes |  |
| medic | medic | 0, all | crouched work: Task_Build (F031256), first aid, Task_Loot (F031209), Task_CuffTarget (F031266: under 0.99 s), Task_StripWeapons (F031285), Task_ProcessNewSlave (F031348: its first 3 s), Task_Prospecting; functionalities | build, first aid | yes | looting and lockpicking added; restraining, stripping and processing slaves (`Sim::law_task_clip`) |
| mid blow | mid blow | 0, all | stumble (F037176) | yes | yes |  |
| mid blow drop | mid blow drop | 0, all | stumble | yes | yes |  |
| mid blow light | mid blow light | 0, all | stumble | yes | yes |  |
| pickup | pickup | 0, upper | Task_Pickup (F031214) | yes | yes |  |
| reload 1 phase | reload 1 | 8, all | reloading (F037165) | yes | yes |  |
| reload 2 phase | reload 2 | 8, all | reloading, CROSSBOW "reload anim" | yes | yes |  |
| reload 3 phase | reload 3 | 8, all | reloading, CROSSBOW "reload anim" | yes | yes |  |
| run lower | run lower | 0, lower | movement (playBlended F037154 by speed, F037145) | yes | yes |  |
| run lower stealth | ninjarun | 0, lower | movement (playBlended F037154 by speed, F037145), sneaking | yes | yes |  |
| run upper | run upper | 0, upper | movement (playBlended F037154 by speed, F037145), no weapon in hand | no | yes |  |
| run upper stealth | ninjarun | 0, upper | movement (playBlended F037154 by speed, F037145), sneaking | yes | yes |  |
| run upper sword | run upper sword | 0, upper | movement (playBlended F037154 by speed, F037145), weapon drawn | always | yes |  |
| run upper sword lefty | run upper sword lefty | 0, upper | movement (playBlended F037154 by speed, F037145), a left-hand weapon | no | no | no left-hand weapons |
| shoot | shoot | 8, all | none found | no | no |  |
| shoulder lift B | shoulder lift B | 0, all | only ever stopped (F037169 on the carried: F031291, F031294, F031343, F039236, F039243), never played | no | no | before this audit's fix it was played on putting down |
| shoulder lift left | shoulder lift left | 0, all | Task_LiftSomething (F031290: the body taken up at 0.4, done at 0.95, F031324); Task_PutDownObject at speed -1 (F031253: let go below 0.2, F031293) | no | yes | always the left (F040018 sets "left"); (stand-in) the put-down lets go at once, the clip running on from 0.2 |
| shoulder lift right | shoulder lift right | 0, all | Task_LiftSomething (F031290) on the right | no | no | never: F040018 always sets the left |
| sidestep-- | sidestep | 0, lower | disabled | disabled | disabled |  |
| sitting chair | sitting chair | 0, all | chair / throne "animation" | yes | yes |  |
| sitting dazed | sitting dazed | 0, all | a cage's "animation dazed" (Building F029399, getter F029281): updateBase (F037184) in a cage (Character +0x2f8 2), not knocked out, its health (Character +0x51c: MedicalSystem +0xc4, blood over the most lowered to the worst vital part's, F040452) under 0.05 at speed 0.025, up to 0.2 at 0.5; else the cage's "animation" at 0.5 (knocked out: "animation KO" at 1) | no | yes | (not ported) Character vtable 0x360's own call for the slowest daze |
| sitting idle | sitting idle | 0, all | a cage's "animation", at rate 0.5 (updateBase F037184); sitting on the floor | yes | yes |  |
| sleepinbed | sleepinbed | 0, all | a bed's "animation" (TaskRepertoire F030716), at rate 0.1 (updateBase F037184: Character +0x2f8 1, Task_UseBed) | yes | yes |  |
| sleeponfloor | sleeponfloor | 0, all | a bedroll's "animation" (Task_SleepOnFloor F031231) | no | yes |  |
| squat | squat | 0, all | none found in code (an idle record) | no | no |  |
| squat T | squat T | 0, all | Task_Crouch / UnCrouch (F031202..F031207), the looting lead-in (F031208..F031212) | no | yes | looting; (not ported) Task_Crouch |
| stand 1 | stand 1 | 0, all | the default idle (updateBase F037184); cages, shop counters, gun turrets | yes | yes |  |
| stand 1 crossbow | stand 1 sword | 8, all | idle (AnimationClass::playIdle F037155 over the candidates F037153), aiming | yes | yes |  |
| stand 1 sword | stand 1 sword | 0, all | idle (AnimationClass::playIdle F037155 over the candidates F037153), weapon drawn | always | yes |  |
| stand 1 sword lefty | stand 1 sword lefty | 0, all | idle (AnimationClass::playIdle F037155 over the candidates F037153), a left-hand weapon | no | no | no left-hand weapons |
| stand 1 sword noarms | stand 1 sword noarms | 0, all | disabled | disabled | disabled |  |
| stand carrying left | stand carrying left | 0, overlay | none found (an overlay idle; playIdle F037155 skips overlays) | yes (assumption) | no | replaced by the arm overlay Kenshi plays |
| stand carrying right | stand carrying right | 0, overlay | carrying on the right | no | no | the rewrite carries on the left |
| stand pole | stand pole | 0, all | a prisoner post's "animation" / KO / lockpick, at the cage's rates (0.5 / 1) | yes | yes |  |
| standing up 3 | standing up 3 | 0, all | getting up (playGetUp F037164) | yes | yes |  |
| stealth idle | crouch idle | 0, all | idle (AnimationClass::playIdle F037155 over the candidates F037153), sneaking | yes | yes |  |
| stonemine | stonemine | 0, all | functionalities; Task_BashDoor (F031240), Task_Dismantle (F031298) | machines | yes | bashing and dismantling added |
| strafe lower | strafe lower | 0, lower | strafing in a fight (F037179) | yes | yes |  |
| swim | swim2 | 4, all | movement (playBlended F037154 by speed, F037145), category 4 | yes | yes |  |
| swim carry | swim carry | 4, all | movement (playBlended F037154 by speed, F037145), category 4, carrying right | no | no | the rewrite carries on the left |
| swim carry left | swim carry left | 4, all | movement (playBlended F037154 by speed, F037145), category 4, carrying | yes | yes |  |
| swim idle | swimidle2 | 4, all | idle (AnimationClass::playIdle F037155 over the candidates F037153), category 4 | yes | yes |  |
| sword ready (carry) | sword ready | 0, upper | idle (AnimationClass::playIdle F037155 over the candidates F037153), combat mode (AnimationClass +0x244: set by CombatClass F029615 / F039804, cleared by CharMovement F040653) while carrying (+0x21e) | no | yes | a carrier in a fight: the arena's situation carries the busy arm (`Fighter::carrying`) |
| walk carrying left | walk carrying left | 0, upper | movement (playBlended F037154 by speed, F037145), carrying | yes | yes |  |
| walk carrying right | walk carrying right | 0, upper | movement (playBlended F037154 by speed, F037145), carrying on the right | no | no | the rewrite carries on the left |
| walk lower | walk lower | 0, lower | movement (playBlended F037154 by speed, F037145) | yes | yes |  |
| walk lower combat shuffle long BK | stepback | 0, lower | movement (playBlended F037154 by speed, F037145), combat mode | yes | yes |  |
| walk lower combat shuffle long FW | stepback | 0, lower | movement (playBlended F037154 by speed, F037145), combat mode | yes | yes |  |
| walk lower combat shuffle short | stepback short | 0, lower | movement (playBlended F037154 by speed, F037145), combat mode | yes | yes |  |
| walk upper | walk upper | 0, upper | movement (playBlended F037154 by speed, F037145), no weapon in hand | no | yes |  |
| walk upper sword | walk upper sword | 0, upper | movement (playBlended F037154 by speed, F037145), weapon drawn | always | yes |  |
| wind in face | windface | 0, overlay | weather overlay, type 1 (F037181, F039251) | no | yes |  |

## ANIMAL_ANIMATION records

| record | clip | Kenshi plays it | before | now | note |
|---|---|---|---|---|---|
| ANIMAL_ANIMATION |  | no reference | no | no |  |
| Crimper run | run | movement by speed (F037154) | yes | yes |  |
| beast run | run | movement by speed (F037154) | yes | yes |  |
| beast walk | walk | movement by speed (F037154) | yes | yes |  |
| bull run | run | movement by speed (F037154) | yes | yes |  |
| bull walk | walk | movement by speed (F037154) | yes | yes |  |
| cagebeast walk | walk | movement by speed (F037154) | yes | yes |  |
| dog run | run | movement by speed (F037154) | yes | yes |  |
| dog strafe | strafeleft | "strafe anim" (F038936): strafing in a fight (F037179) | no | yes |  |
| dog walk | walk | movement by speed (F037154) | yes | yes |  |
| duck run | run | movement by speed (F037154) | yes | yes |  |
| duck walk | walk | movement by speed (F037154) | yes | yes |  |
| eat | eat | "animations": eating (Task_EatingCrops F031241) | no | no | no grazing task |
| eat (attack1) | attack1 | "animations" | no | no |  |
| giraffe turning | turnleft | "turning anim" (F038936): turning on the spot (F037152, F037138) | no | yes |  |
| goat run | run | movement by speed (F037154) | yes | yes |  |
| goat walk | walk | movement by speed (F037154) | yes | yes |  |
| gorilla run | run | movement by speed (F037154) | yes | yes |  |
| gorilla walk | walk | movement by speed (F037154) | yes | yes |  |
| idle noloop | idle noloop | idle (playIdle F037155: the first of the best weight) | yes | yes | only the first idle of an animal plays (equal weights), as in Kenshi |
| idle1 | idle1 | idle (playIdle F037155: the first of the best weight) | yes | yes | only the first idle of an animal plays (equal weights), as in Kenshi |
| idle1 -crab | idle1 | idle (playIdle F037155: the first of the best weight) | yes | yes | only the first idle of an animal plays (equal weights), as in Kenshi |
| idle2 | idle2 | idle (playIdle F037155: the first of the best weight) | yes | yes | only the first idle of an animal plays (equal weights), as in Kenshi |
| idle3 | idle3 | idle (playIdle F037155: the first of the best weight) | yes | yes | only the first idle of an animal plays (equal weights), as in Kenshi |
| idle4 | idle4 | no reference | no | no |  |
| mouthclack | mouthclack | "animations" (no code reference) | no | no |  |
| mouthclose | mouthclose | "animations" (no code reference) | no | no |  |
| run | run | movement by speed (F037154) | yes | yes |  |
| skim strafe | strafe | "turning anim" (F038936): turning on the spot (F037152, F037138) | no | yes |  |
| skim walk | walk | movement by speed (F037154) | yes | yes |  |
| sleeponfloor | sleep | "animations": sleeping | no | no | animals do not sleep in the simulation |
| sleeponfloor | gotosleep | "animations": sleeping | no | no | animals do not sleep in the simulation |
| sleeponfloor | sleep | "animations": sleeping | no | no | animals do not sleep in the simulation |
| spider strafe crab | strafe | "strafe anim" (F038936): strafing in a fight (F037179) | no | yes |  |
| spider strafe insect | straferight | "strafe anim" (F038936): strafing in a fight (F037179) | no | yes |  |
| spider strafe robo | strafeleft | "strafe anim" (F038936): strafing in a fight (F037179) | no | yes |  |
| spider turn | walk | "turning anim" (F038936): turning on the spot (F037152, F037138) | no | yes |  |
| spider walk insect | walk | movement by speed (F037154) | yes | yes |  |
| spider walk robo | walk | movement by speed (F037154) | yes | yes |  |
| strafe | strafeleft | "animations" / "strafe anim" (F038936): strafing in a fight (F037179) | no | yes |  |
| strafe gorilla | strafeleft | "strafe anim" (F038936): strafing in a fight (F037179) | no | yes |  |
| turtle walk | walk | movement by speed (F037154) | yes | yes |  |
| walk giraffe | walk | movement by speed (F037154) | yes | yes |  |
| walk whale | walk | movement by speed (F037154) | yes | yes |  |
| whale turning | turnleft | "turning anim" (F038936): turning on the spot (F037152, F037138) | no | yes |  |

## COMBAT_TECHNIQUE clips

Played on the action layer by the fight (Character::playTechnique F038914; chosen by chooseAttack F047076 / chooseDefence F047075): all of them, before and now.

| clip | techniques |
|---|---|
| attack1 | dog type attack, duck attack, elephant type bash, giraffe type attack, gorillo attack1, insect spider attack, robot spider attack |
| attack2 | dog type attack long, elephant type bite, gar attack long, giraffe type attack-short, gorillo attack2 |
| attack3 | gar attack legs, gorillo attack3 |
| badpunch | ma badpunch |
| bigchopv2 | heavy downcut v2 |
| blk front hi | Block up |
| blk rear hi | Block up rear |
| blk rear left | Block right rear |
| blk right | Block right |
| blk thrust | Block thrust |
| block left upper | Block left |
| chop down static | Downward cut static |
| chop down | Downward cut |
| chop left | Cut left, Cut left static |
| desperate attack | ground attack L, ground attack R |
| dodgeback | Dodge back, Dodge back (copy for stumble) |
| dodgefall | Dodge back fall |
| dodgefly | Dodge back fly |
| downward combo | Downward combo |
| flykick | ma jumpkick |
| flyknee | ma jump knee |
| headbutt | ma headbutt |
| heavy downcut | heavy downcut |
| heavy swing | heavy swing |
| kicklow | ma kicklow |
| ma 2punchie | ma Double punch high |
| ma 2strike | ma Double strike |
| ma chudan | ma Double punch mid |
| megakick | ma mega kick |
| shoteiL | ma shotei L |
