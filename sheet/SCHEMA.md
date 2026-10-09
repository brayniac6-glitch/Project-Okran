# Sheet schema

All tables are TSV (tab-separated, UTF-8, one header row, no quoting). One row = one fact.
IDs are stable forever: never renumber, never reuse. Keys are joined across tables.

## functions.tsv — one row per function in kenshi_x64.exe
| col | meaning |
|---|---|
| fid | `F` + 6 digits, stable id |
| addr | hex VA in kenshi_x64.exe (join key back to Ghidra) |
| size | bytes |
| name | best-known name. `FUN_*` until someone names it |
| ns | C++ namespace/class from RTTI, or empty |
| sub | subsystem id (→ subsystems.tsv) |
| sig | confirmed C signature, or empty. Only `typed`+ rows may have one |
| calls | count of outgoing calls (triage signal) |
| xrefs | count of callers (triage signal) |
| status | see ladder below |
| port | rewrite target (→ port_map.tsv key), or empty |
| spec | ≤1 line behavioural summary. No code |

## Status ladder
`raw` → `named` → `typed` → `spec` → `mapped` → `impl` → `verified`
Plus `skip` for code we will never port (CRT, STL, Ogre, MyGUI, PhysX, OIS, boost, Steam, etc.).
A row may only move forward one step per edit, and only by the agent holding its subsystem claim.

## types.tsv — structs, classes, enums, typedefs
`tid  name  kind(struct|class|enum|typedef)  size  sub  status  notes`

## fields.tsv — members of types.tsv structs/classes
`tid  off(hex)  name  type  notes`

## enums.tsv — values of types.tsv enums
`tid  name  value  notes`

## strings.tsv — interesting strings (triage + naming evidence)
`addr  fid(first referencing function, or empty)  text`

## subsystems.tsv
`sub  title  desc  prio(1-5)`

## tasks.tsv — the lock table for parallel agents
`sub  agent  state(open|claimed|review|done)  since(ISO date)  note`
Exactly one `claimed` row per `sub` at a time.

## port_map.tsv — Kenshi concept → rewrite module
`key  kenshi  rewrite(module / crate path)  kind(system|data|ui|world|render)  status(proposed|spec|impl|verified)  notes`
