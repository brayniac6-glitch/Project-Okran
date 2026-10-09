# Triage of unsorted (`unk`) functions

Measured 2026-10-01 on kenshi_x64.exe (166,720 functions). Labels known from names/RTTI were hidden
(20% holdout) and each signal was scored on recovering them.

| signal | result | used? |
|---|---|---|
| Name/RTTI-class regexes (`CLASSIFY` in `tools/sheet.py`) | primary source; ~8.7k game-code rows | yes |
| Third-party spread: every caller is `thirdparty` → `thirdparty`/`skip` | 5/5 holdout correct; ~1.4k rows | yes (`spread_thirdparty`) |
| Address locality (neighbouring functions share a sub) | 44% agreement: linker order does not follow source files | no |
| One-hop call-graph vote | 87% precision on 23 predictions; ~0.9k rows | no |
| One-hop vote + string keywords | 71% precision; ~2.7k rows | no |
| Iterative label propagation | ~26% precision: labels flood through shared utility hubs | no |
| `__FILE__` paths in strings | only 135 path strings, mostly Havok/Ogre/MyGUI dependencies | evidence only |

Conclusion: most of the ~57k unnamed `FUN_*` rows cannot be sorted automatically at useful precision.
They are triaged on demand: work outward from a subsystem's named rows (`sheet.py next <sub>`), using
callers/callees (`decomp/calls.tsv`) and referenced strings (`strings.tsv` `fid` column) as evidence.

Inputs (local only, gitignored): `decomp/rtti_vtables.tsv` (`ExportClasses.java`), `decomp/calls.tsv` and
`decomp/strrefs.tsv` (`ExportRefs.java`). Run both scripts before `sheet.py ingest`.
