#!/usr/bin/env python3
"""Query the RTTI export (decomp/rtti_vtables.tsv from tools/ghidra/ExportClasses.java).

  python tools/rtti.py classes <regex>        classes matching, with slot counts and vtables
  python tools/rtti.py vtable <Class> [n]     slots of the class's primary vtable, with function sizes
  python tools/rtti.py owners <fn-addr>       which classes' vtables contain that function
  python tools/rtti.py derived <Class>        classes sharing the most of <Class>'s slots (likely subclasses/bases)
"""
import csv, re, sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RTTI = ROOT / "decomp" / "rtti_vtables.tsv"


def load():
    rows = list(csv.DictReader(RTTI.open(encoding="utf-8"), delimiter="\t"))
    fns = {r["addr"]: r for r in csv.DictReader((ROOT / "sheet" / "functions.tsv").open(encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE)}
    return rows, fns


def norm(a):
    return a.lower().replace("0x", "").lstrip("0")


def primary(rows, cls):
    vts = defaultdict(list)
    for r in rows:
        if r["class"] == cls:
            vts[r["vtable"]].append(r)
    if not vts:
        sys.exit(f"no class {cls}")
    # the primary vtable is the longest one (secondary ones come from multiple inheritance)
    return max(vts.items(), key=lambda kv: len(kv[1]))


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    rows, fns = load()
    cmd, arg = sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else ""
    if cmd == "classes":
        by = defaultdict(set)
        cnt = defaultdict(int)
        for r in rows:
            if re.search(arg, r["class"]):
                by[r["class"]].add(r["vtable"])
                cnt[r["class"]] += 1
        for c in sorted(by, key=len):
            print(f"{c}\tslots={cnt[c]}\tvtables={','.join(sorted(by[c]))}")
    elif cmd == "vtable":
        vt, slots = primary(rows, arg)
        limit = int(sys.argv[3]) if len(sys.argv) > 3 else 9999
        print(f"{arg} vtable {vt}")
        for r in slots[:limit]:
            f = fns.get(norm(r["fn"]), {})
            print(f"{int(r['slot']):>4}  {r['fn']}  size={f.get('size', '?'):>6}  {f.get('fid', '')}  {f.get('name', '')}")
    elif cmd == "owners":
        a = norm(arg)
        for r in rows:
            if norm(r["fn"]) == a:
                print(f"{r['class']}\tvtable={r['vtable']}\tslot={r['slot']}")
    elif cmd == "dis":
        # compact disassembly of each address up to its first RET (or n instructions)
        import json, subprocess
        for a in sys.argv[2:]:
            out = subprocess.run(["ghidra", "disasm", "0x" + norm(a), "-n", "40"], capture_output=True, text=True).stdout
            lines = []
            for ins in json.loads(out[out.index("["):]):
                lines.append(f"{ins['mnemonic'].lower()} {', '.join(ins['operands'])}".strip())
                if ins["mnemonic"] in ("RET", "JMP"):
                    break
            print(f"{norm(a)}: " + " ; ".join(lines))
    elif cmd == "derived":
        _, base = primary(rows, arg)
        basefns = [norm(r["fn"]) for r in base]
        score = defaultdict(int)
        for r in rows:
            if r["class"] != arg and norm(r["fn"]) in basefns:
                score[r["class"]] += 1
        for c, s in sorted(score.items(), key=lambda kv: -kv[1])[:30]:
            print(f"{s:>4}/{len(basefns)}  {c}")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
