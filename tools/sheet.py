#!/usr/bin/env python3
"""Spreadsheet Method driver: the sheet/ TSVs are the source of truth, everything else is a projection.

Usage: python tools/sheet.py <ingest|coverage|validate|claim|release|next|decomp|find> [...]
"""
import csv, datetime, json, os, re, subprocess, sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHEET = ROOT / "sheet"
DECOMP = ROOT / "decomp"
SRC = ROOT / "src"
GHIDRA = ["ghidra", "--project", "kenshi", "--program", "kenshi_x64.exe"]

FN_COLS = ["fid", "addr", "size", "name", "ns", "sub", "sig", "calls", "xrefs", "status", "port", "spec"]
HUMAN_COLS = {"name", "ns", "sub", "sig", "status", "port", "spec"}  # never overwritten by ingest once edited
LADDER = ["raw", "named", "typed", "spec", "mapped", "impl", "verified"]
STATUSES = set(LADDER) | {"skip"}
SKIP_SUBS = {"thirdparty", "compiler"}

# First match wins: (regex on "ns::name", subsystem). Order matters.
CLASSIFY = [
    (r"(^|::)(Unwind@|Catch_All@|Catch@|thunk_)|`(scalar|vector)_deleting_destructor", "compiler"),
    (r"^(std|stdext|Concurrency|__crt|__scrt|__vcrt|_Init_|__acrt)|^(_|__)[a-z]|::(_|__)[a-z]|^operator |Catch|Unwind|`", "thirdparty"),
    (r"(?i)^Ogre::|^MyGUI::|^OIS::|^boost::|^Nx[A-Z]|PhysX|^Hydrax|^SkyX|^ParticleUniverse|steam|zlib|inflate|deflate|tinyxml|lua_|^Forests?::", "thirdparty"),
    # statically linked libs: Havok (hk/hkcd/hkgp, LOCALNAMESPACE::hk), Wwise (AK, CAk), MFC/ATL, Scythe (PhysX wrapper)
    (r"^(LOCALNAMESPACE::)?hk(cd|gp)?[A-Z]|^lektor<hk|ArrayImplementation|ArrayView|^AK::|^CAk|^C[A-Z]\w*(Ctrl|Box|Object|Font|Static|CriticalSection)\b|^CExCheckListBox|^Define_the_symbol__ATL|Scythe|^type_info|^UserStream|^MyUserNotify", "thirdparty"),
    # Kenshi classes (from RTTI) whose names the keyword rules below miss or would misfile
    (r"^(lektor<|hand\b|HandleList|HandleManager|ImportHandleList|RootObject|_ScytheRoot|FactoryCallbackInterface|ExitOnFlagCallback|AccessibilityCallback)", "core"),
    (r"^(Package_|Contract_|Mission|GoOutOnPatrol|Prisoner|SlaverPrisoner|GatherAndMove|RetreatAndRefresh|ChaseMode|Formation|Blackboard|GroupSense|MoveTo_|ActionDeque|CircleStateAI|NullMediator|Unprovoked|SubjectiveTags|Ownerships|TagsClass<SenseType>|StartInvasion|Nav(Instance|Volume)|FaceCallback|(Water)?EdgeFilter|WaterCostModifier|(Ex|In)teriorFilter)", "ai"),
    (r"^(Tutorial_|DataPanelLine|GameLauncher|LevelEditorPanel|ManagementScreen|TitleScreen|MessageBoxManager|MessageRoller|ScreenLabel|InfoText|SpeedSelector|EditableImage|wraps::|FogEditor)", "ui"),
    (r"^(Effect|AttachedEffect|Fog|GroundFog|KenshiCompositor|KenshiPassDef|DDSListener|LostDeviceListener|DebugLines|WaterOcclusion|GeometryBatcher)", "render"),
    (r"^(Appearance|RaceLimiter|CharMovement|AbstractMovementBase|Gear\b)", "char"),
    (r"^(Sword|GunClass)\b", "combat"),
    (r"^(DoorStuff|UseableStuff|UsageNode|PlacementObject|FeaturePlacement|AttachedEntity|AttachedObject|QueueListenerCutter|Layout)", "build"),
    (r"^(BasePopulationManager|SpawnInfoList|RoadManager|LocationNode|MapFeature|BirdManager|SpacialCell|Partitioner)", "world"),
    (r"^(Static(Box|Capsule)Entity|(Door|DoorAndWater)HitFilter|HitCallback|HkStuckCallback|(DynVolume)?Aabb|(Dyn|Nav)?VolumeClosestPoint|PointQuery|TriangleQuery|IConvexOverlap|IslandFillCallback)", "physics"),
    (r"^Serialisable\b", "save"),
    (r"(?i)medical|limb|wound|bleed|\bKO\b|unconscious|CharStats|\bstats?\b|race\b|hunger|starv", "char"),
    (r"(?i)attack|block(ing)?\b|weapon|armou?r|damage|combat|crossbow|turret|projectile|harpoon", "combat"),
    (r"(?i)platoon|squad|\bAI\b|Ai[A-Z]|task|order|goal|faction|relation|pathfind|navmesh|recast|detour", "ai"),
    (r"(?i)building|construct|furniture|production|farm|outpost|\bbase\b|wall|gate|turret", "build"),
    (r"(?i)inventory|item|trade|shop|money|\bcats\b|container|loot|backpack", "econ"),
    (r"(?i)dialog|conversation|speech|quest|worldstate|world_state", "dialog"),
    (r"(?i)selection|PlayerInterface|playerinput|command(er)?", "player"),
    (r"(?i)GameData|\.mod\b|\.base\b|FCS|record|template", "data"),
    (r"(?i)zone|town|nest|biome|weather|time ?of ?day|terrain|region|world", "world"),
    (r"(?i)save|load(game)?|serializ", "save"),
    (r"(?i)gui|hud|menu|window|widget|button|tooltip|map(screen|ui)", "ui"),
    (r"(?i)render|d3d|dxgi|shader|texture|mesh|material|particle|light|draw|ogre", "render"),
    (r"(?i)physic|collision|rigid|raycast|ragdoll|NxActor", "physics"),
    (r"(?i)anim|skelet|bone|blend|pose", "anim"),
    (r"(?i)sound|audio|music|voice", "audio"),
    (r"(?i)input|mouse|keyboard|camera|key(bind)?", "input"),
    (r"(?i)character|\bchar\b|body|person|npc", "char"),
    (r"(?i)memory|alloc|heap|thread|job|mutex|hash|string|log|assert|timer|clock|gameworld|update", "core"),
]


# ---------- TSV I/O ----------
def read(name):
    p = SHEET / f"{name}.tsv"
    if not p.exists():
        return []
    with p.open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f, delimiter="\t", quoting=csv.QUOTE_NONE))


def write(name, rows, cols):
    clean = lambda v: str(v if v is not None else "").replace("\t", " ").replace("\r", " ").replace("\n", " ")
    with (SHEET / f"{name}.tsv").open("w", encoding="utf-8", newline="") as f:
        f.write("\t".join(cols) + "\n")
        for r in rows:
            f.write("\t".join(clean(r.get(c, "")) for c in cols) + "\n")


def cols_of(name):
    with (SHEET / f"{name}.tsv").open(encoding="utf-8") as f:
        return f.readline().rstrip("\n").split("\t")


# ---------- Ghidra bridge ----------
def ghidra_json(*args):
    out = subprocess.run(GHIDRA[:1] + list(args) + GHIDRA[1:] + ["--json"],
                         capture_output=True, text=True, encoding="utf-8", errors="replace")
    if out.returncode != 0:
        sys.exit(f"ghidra {' '.join(args)} failed:\n{out.stderr[-2000:]}")
    txt = out.stdout.strip()
    data = json.loads(txt[txt.index(txt.lstrip()[0]):]) if txt else []
    if isinstance(data, dict):  # unwrap {"functions":[...]} / {"results":[...]} style envelopes
        lists = [v for v in data.values() if isinstance(v, list)]
        data = lists[0] if lists else [data]
    return data


def pick(d, *keys, default=""):
    for k in keys:
        if k in d and d[k] not in (None, ""):
            return d[k]
    return default


def norm_addr(a):
    a = str(a).lower().replace("0x", "")
    return a.split(":")[-1].lstrip("0") or "0"


def rtti_owners():
    """fn addr -> owning class, from decomp/rtti_vtables.tsv (tools/ghidra/ExportClasses.java).
    A function in several vtables is inherited: the owner is the class with the fewest slots (the base)."""
    p = DECOMP / "rtti_vtables.tsv"
    if not p.exists():
        return {}
    rows = list(csv.DictReader(p.open(encoding="utf-8"), delimiter="\t"))
    slots = Counter(r["class"] for r in rows)
    owner = {}
    for r in rows:
        a = norm_addr(r["fn"])
        c = r["class"]
        if a not in owner or slots[c] < slots[owner[a]]:
            owner[a] = c
    return owner


def classify(ns, name):
    key = f"{ns}::{name}" if ns else name
    for rx, sub in CLASSIFY:
        if re.search(rx, key):
            return sub
    return "unk"


def load_edges(name):
    """(a, b) address pairs from decomp/<name> (tools/ghidra/ExportRefs.java), or [] if not exported."""
    p = DECOMP / name
    if not p.exists():
        return []
    with p.open(encoding="utf-8") as f:
        next(f)
        return [tuple(norm_addr(x) for x in line.rstrip("\n").split("\t")) for line in f if "\t" in line]


def spread_thirdparty(rows):
    """Untriaged FUN_ rows whose every caller is third-party are library internals: mark them skip.
    Only this rule is applied automatically; game-subsystem votes over the call graph measured too
    imprecise (see docs/TRIAGE.md)."""
    by = {r["addr"]: r for r in rows}
    callers = defaultdict(set)
    for a, b in load_edges("calls.tsv"):
        callers[b].add(a)
    changed = True
    while changed:
        changed = False
        for r in rows:
            if r["status"] != "raw" or r["sub"] != "unk":
                continue
            cs = [by[c]["sub"] for c in callers[r["addr"]] if c in by and by[c]["sub"] != "compiler"]
            if cs and all(s == "thirdparty" for s in cs):
                r["sub"], r["status"] = "thirdparty", "skip"
                changed = True


# ---------- commands ----------
def cmd_ingest(_):
    """Refresh functions.tsv + strings.tsv from Ghidra. Machine columns refresh; human columns are kept."""
    funcs = ghidra_json("function", "list", "--limit", "0")
    old = {r["addr"]: r for r in read("functions")}
    next_id = max([int(r["fid"][1:]) for r in old.values()] or [0]) + 1
    rtti = rtti_owners()
    rows, seen = [], set()
    for f in funcs:
        addr = norm_addr(pick(f, "address", "entry", "entry_point", "addr"))
        if addr in seen:
            continue
        seen.add(addr)
        full = str(pick(f, "name", "symbol"))
        ns = str(pick(f, "namespace", "parent_namespace", "ns"))
        name = full
        if not ns and "::" in full:
            ns, name = full.rsplit("::", 1)
        if ns in ("Global", "<global>"):
            ns = ""
        if not ns:
            ns = rtti.get(addr, "")
        machine = {
            "addr": addr,
            "size": pick(f, "size", "body_size", "length", default=0),
            "calls": pick(f, "call_count", "calls", "callees_count", default=""),
            "xrefs": pick(f, "xref_count", "xrefs", "callers_count", "reference_count", default=""),
        }
        if addr in old:
            r = old[addr]
            r.update(machine)
            # untouched by humans (raw, or auto-skipped): keep in sync with Ghidra renames + classifier
            if r["status"] == "raw" or (r["status"] == "skip" and r["sub"] in SKIP_SUBS):
                r["name"], r["ns"] = name, ns
                r["sub"] = classify(ns, name)
                r["status"] = "skip" if r["sub"] in SKIP_SUBS else "raw"
        else:
            sub = classify(ns, name)
            r = dict(machine, fid=f"F{next_id:06d}", name=name, ns=ns, sub=sub, sig="",
                     status="skip" if sub in SKIP_SUBS else "raw", port="", spec="")
            next_id += 1
        rows.append(r)
    # keep rows Ghidra no longer reports (ids are forever) but flag them
    for a, r in old.items():
        if a not in seen:
            r["spec"] = ("[gone from ghidra] " + r["spec"]).strip()
            rows.append(r)
    rows.sort(key=lambda r: int(r["addr"], 16))
    spread_thirdparty(rows)
    write("functions", rows, FN_COLS)

    strs = ghidra_json("dump", "strings", "--limit", "0")
    fid_of = {r["addr"]: r["fid"] for r in rows}
    first_ref = {}
    for fn, s in load_edges("strrefs.tsv"):
        if fn in fid_of and (s not in first_ref or int(fn, 16) < int(first_ref[s], 16)):
            first_ref[s] = fn
    srows = []
    for s in strs:
        text = str(pick(s, "value", "string", "text"))
        if len(text) < 4:
            continue
        a = norm_addr(pick(s, "address", "addr"))
        srows.append({"addr": a, "fid": fid_of.get(first_ref.get(a), ""), "text": text[:200]})
    write("strings", srows, ["addr", "fid", "text"])
    for t in ("types", "fields", "enums"):
        if not (SHEET / f"{t}.tsv").exists():
            write(t, [], {"types": ["tid", "name", "kind", "size", "sub", "status", "notes"],
                          "fields": ["tid", "off", "name", "type", "notes"],
                          "enums": ["tid", "name", "value", "notes"]}[t])
    print(f"functions: {len(rows)}  strings: {len(srows)}")
    cmd_coverage([])


def cmd_coverage(args):
    rows = read("functions")
    by = defaultdict(Counter)
    for r in rows:
        by[r["sub"]][r["status"]] += 1
        by["ALL"][r["status"]] += 1
    order = LADDER + ["skip"]
    print(f"{'sub':<11}" + "".join(f"{s:>9}" for s in order) + f"{'total':>9}{'done%':>7}")
    for sub in sorted(by, key=lambda s: (s == "ALL", s)):
        c = by[sub]
        tot = sum(c.values())
        port = tot - c["skip"]
        pct = 100 * c["verified"] / port if port else 100
        print(f"{sub:<11}" + "".join(f"{c[s]:>9}" for s in order) + f"{tot:>9}{pct:>6.1f}%")


def cmd_validate(_):
    errs = []
    fns = read("functions")
    subs = {r["sub"] for r in read("subsystems")}
    port = {r["key"] for r in read("port_map")}
    fids, addrs = set(), set()
    for r in fns:
        if r["fid"] in fids: errs.append(f"dup fid {r['fid']}")
        if r["addr"] in addrs: errs.append(f"dup addr {r['addr']}")
        fids.add(r["fid"]); addrs.add(r["addr"])
        if r["status"] not in STATUSES: errs.append(f"{r['fid']} bad status {r['status']!r}")
        if r["sub"] not in subs: errs.append(f"{r['fid']} unknown sub {r['sub']!r}")
        if r["port"] and r["port"] not in port: errs.append(f"{r['fid']} unknown port key {r['port']!r}")
        if r["sig"] and r["status"] in ("raw", "named"): errs.append(f"{r['fid']} has sig but status {r['status']}")
        if r["status"] in ("mapped", "impl", "verified") and not r["port"]: errs.append(f"{r['fid']} {r['status']} without port")
    types = {r["tid"]: r for r in read("types")}
    for t in ("fields", "enums"):
        for r in read(t):
            if r["tid"] not in types: errs.append(f"{t}: unknown tid {r['tid']}")
    locks = Counter(r["sub"] for r in read("tasks") if r["state"] == "claimed")
    errs += [f"tasks: {s} claimed {n}x" for s, n in locks.items() if n > 1]
    # Anti-hallucination: every @fn/@type tag in src/ must resolve to a sheet row.
    known_types = {r["name"] for r in types.values()}
    for p in SRC.rglob("*") if SRC.exists() else []:
        if not p.is_file():
            continue
        for i, line in enumerate(p.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            for fid in re.findall(r"@fn\s+(F\d{6})", line):
                if fid not in fids: errs.append(f"{p.relative_to(ROOT)}:{i} unknown {fid}")
            for tn in re.findall(r"@type\s+(\w+)", line):
                if tn not in known_types: errs.append(f"{p.relative_to(ROOT)}:{i} unknown type {tn}")
    print("\n".join(errs) if errs else f"OK: {len(fns)} functions, {len(types)} types")
    sys.exit(1 if errs else 0)


def _set_task(sub, **kv):
    rows = read("tasks")
    hit = [r for r in rows if r["sub"] == sub]
    if not hit:
        sys.exit(f"no such sub {sub}")
    hit[0].update(kv)
    write("tasks", rows, cols_of("tasks"))


def cmd_claim(args):
    sub, agent = args[0], args[1] if len(args) > 1 else os.environ.get("USERNAME", "agent")
    cur = next((r for r in read("tasks") if r["sub"] == sub), None)
    if cur and cur["state"] == "claimed" and cur["agent"] != agent:
        sys.exit(f"{sub} already claimed by {cur['agent']} since {cur['since']}")
    _set_task(sub, agent=agent, state="claimed", since=datetime.date.today().isoformat())
    print(f"{agent} claimed {sub}")


def cmd_release(args):
    _set_task(args[0], state=args[1] if len(args) > 1 else "open", agent="", since="")
    print(f"released {args[0]}")


def cmd_next(args):
    """Highest-leverage unfinished rows: most-called first, so naming them propagates the furthest."""
    sub, n = args[0], int(args[1]) if len(args) > 1 else 25
    rows = [r for r in read("functions") if r["sub"] == sub and r["status"] not in ("verified", "skip")]
    rows.sort(key=lambda r: (LADDER.index(r["status"]), -int(r["xrefs"] or 0)))
    w = csv.writer(sys.stdout, delimiter="\t", lineterminator="\n")
    w.writerow(FN_COLS)
    for r in rows[:n]:
        w.writerow([r[c] for c in FN_COLS])


def cmd_find(args):
    rx = re.compile(args[0], re.I)
    for t in ("functions", "types", "strings", "port_map"):
        for r in read(t):
            line = "\t".join(r.values())
            if rx.search(line):
                print(f"{t}\t{line}")


def cmd_decomp(args):
    q = args[0]
    r = next((r for r in read("functions") if q in (r["fid"], r["addr"], norm_addr(q))), None)
    if not r:
        sys.exit(f"no row for {q}; add it to functions.tsv first")
    DECOMP.mkdir(exist_ok=True)
    out = DECOMP / f"{r['fid']}.c"
    if not out.exists():
        res = subprocess.run(["ghidra", "decompile", "0x" + r["addr"]] + GHIDRA[1:],
                             capture_output=True, text=True, encoding="utf-8", errors="replace")
        out.write_text(res.stdout or res.stderr, encoding="utf-8")
    print(out.read_text(encoding="utf-8"))


CMDS = {k[4:]: v for k, v in globals().items() if k.startswith("cmd_")}
if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in CMDS:
        sys.exit(__doc__)
    CMDS[sys.argv[1]](sys.argv[2:])
