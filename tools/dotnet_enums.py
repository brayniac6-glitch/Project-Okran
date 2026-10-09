#!/usr/bin/env python3
"""Dump the enums (names and values) of a .NET assembly, e.g. the FCS editor shipped with Kenshi
("forgotten construction set.exe"), whose enums are the game data's schema (StatsEnumerated,
itemType, ...). Reads the ECMA-335 metadata tables directly; no .NET needed.

  python tools/dotnet_enums.py <assembly.exe> [enum name regex]
"""
import re
import struct
import sys


def rva_to_off(sections, rva):
    for va, vsz, raw in sections:
        if va <= rva < va + max(vsz, 1):
            return raw + rva - va
    raise ValueError(f"rva {rva:#x} not in a section")


def main():
    path = sys.argv[1]
    want = re.compile(sys.argv[2]) if len(sys.argv) > 2 else None
    b = open(path, "rb").read()
    pe = struct.unpack_from("<I", b, 0x3C)[0]
    nsec = struct.unpack_from("<H", b, pe + 6)[0]
    opt_size = struct.unpack_from("<H", b, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", b, opt)[0]
    dd = opt + (96 if magic == 0x10B else 112)  # data directories
    cli_rva = struct.unpack_from("<I", b, dd + 14 * 8)[0]
    sec = opt + opt_size
    sections = []
    for i in range(nsec):
        vsz, va, rsz, raw = struct.unpack_from("<IIII", b, sec + 40 * i + 8)
        sections.append((va, max(vsz, rsz), raw))
    cli = rva_to_off(sections, cli_rva)
    md_rva = struct.unpack_from("<I", b, cli + 8)[0]
    md = rva_to_off(sections, md_rva)
    assert b[md:md + 4] == b"BSJB", "no CLI metadata"
    vlen = struct.unpack_from("<I", b, md + 12)[0]
    p = md + 16 + vlen + 2
    nstreams = struct.unpack_from("<H", b, p)[0]
    p += 2
    streams = {}
    for _ in range(nstreams):
        off, size = struct.unpack_from("<II", b, p)
        p += 8
        end = b.index(b"\0", p)
        name = b[p:end].decode()
        p = (end + 4) & ~3
        streams[name] = (md + off, size)
    tbl, _ = streams.get("#~") or streams["#-"]
    strings, _ = streams["#Strings"]
    blob, _ = streams["#Blob"]

    heap_sizes = b[tbl + 6]
    valid = struct.unpack_from("<Q", b, tbl + 8)[0]
    p = tbl + 24
    rows = {}
    for t in range(64):
        if valid >> t & 1:
            rows[t] = struct.unpack_from("<I", b, p)[0]
            p += 4
    str_w = 4 if heap_sizes & 1 else 2
    guid_w = 4 if heap_sizes & 2 else 2
    blob_w = 4 if heap_sizes & 4 else 2

    def idx_w(*tables):
        return 4 if max(rows.get(t, 0) for t in tables) >= 0x10000 else 2

    def coded_w(tables, bits):
        return 4 if max(rows.get(t, 0) for t in tables if t is not None) >= (1 << (16 - bits)) else 2

    TYPEDEF, FIELD, METHOD, PARAM, TYPEREF, MODULE, MEMBERREF, CONSTANT, TYPESPEC, PROPERTY = 2, 4, 6, 8, 1, 0, 10, 11, 27, 23
    typedef_or_ref = coded_w([TYPEDEF, TYPEREF, TYPESPEC], 2)
    has_constant = coded_w([FIELD, PARAM, PROPERTY], 2)
    resolution_scope = coded_w([MODULE, 26, 35, TYPEREF], 2)
    # row sizes of the tables before Constant (0x0b), in order; only the ones we need to skip
    sizes = {
        0: 2 + str_w + 3 * guid_w,                         # Module
        1: resolution_scope + 2 * str_w,                   # TypeRef
        2: 4 + 2 * str_w + typedef_or_ref + idx_w(FIELD) + idx_w(METHOD),  # TypeDef
        3: idx_w(FIELD),                                   # FieldPtr
        4: 2 + str_w + blob_w,                             # Field
        5: idx_w(METHOD),                                  # MethodPtr
        6: 4 + 2 + 2 + str_w + blob_w + idx_w(PARAM),      # MethodDef
        7: idx_w(PARAM),                                   # ParamPtr
        8: 2 + 2 + str_w,                                  # Param
        9: idx_w(TYPEDEF) + typedef_or_ref,                # InterfaceImpl
        10: coded_w([TYPEDEF, TYPEREF, MODULE, METHOD, TYPESPEC], 3) + str_w + blob_w,  # MemberRef
        11: 2 + has_constant + blob_w,                     # Constant
    }
    starts = {}
    for t in sorted(rows):
        starts[t] = p
        if t > 11:
            break
        p += rows[t] * sizes[t]

    def rd(off, w):
        return struct.unpack_from("<I" if w == 4 else "<H", b, off)[0]

    def string(i):
        end = b.index(b"\0", strings + i)
        return b[strings + i:end].decode("utf-8", "replace")

    # TypeDefs: name, namespace, field list start
    types = []
    for r in range(rows[TYPEDEF]):
        o = starts[TYPEDEF] + r * sizes[TYPEDEF]
        name = string(rd(o + 4, str_w))
        ns = string(rd(o + 4 + str_w, str_w))
        o2 = o + 4 + 2 * str_w + typedef_or_ref
        types.append((name, ns, rd(o2, idx_w(FIELD))))
    fields = []
    for r in range(rows[FIELD]):
        o = starts[FIELD] + r * sizes[FIELD]
        fields.append(string(rd(o + 2, str_w)))
    # Constants keyed by field row (HasConstant tag 0 = Field)
    consts = {}
    for r in range(rows.get(CONSTANT, 0)):
        o = starts[CONSTANT] + r * sizes[CONSTANT]
        etype = b[o]
        parent = rd(o + 2, has_constant)
        if parent & 3 != 0:
            continue
        bo = blob + rd(o + 2 + has_constant, blob_w)
        n = b[bo]  # compressed length (small values only in practice)
        data = b[bo + 1:bo + 1 + n]
        fmt = {0x08: "<i", 0x09: "<I", 0x04: "<b", 0x05: "<B", 0x06: "<h", 0x07: "<H", 0x0A: "<q", 0x0B: "<Q"}.get(etype)
        if fmt:
            consts[(parent >> 2) - 1] = struct.unpack_from(fmt, data)[0]

    for k, (name, ns, first) in enumerate(types):
        last = types[k + 1][2] if k + 1 < len(types) else rows[FIELD] + 1
        members = [(fields[i - 1], consts[i - 1]) for i in range(first, last) if i - 1 in consts and fields[i - 1] != "value__"]
        if not members or (want and not want.search(name)):
            continue
        print(f"enum {ns + '.' if ns else ''}{name}")
        for m, v in members:
            print(f"  {m} = {v}")


if __name__ == "__main__":
    main()
