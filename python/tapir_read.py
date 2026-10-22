"""Readers for tapir output (stdlib only).

    import tapir_read as tr
    blocks = tr.read_blocks("tapir-out/bbv.blocks.r0.csv")     # hash -> Block
    vecs   = tr.read_bb("tapir-out/bbv.r0.bb")                 # [{block: weight}]
    rows   = tr.read_vector_rows("analysis.csv", "gpu_")       # region -> {hash: weight}

Offline re-bucketing: record with `bbv(key=exact)`, then merge blocks at any
granularity without re-running, by tag:

    key = tr.rebucket(blocks, size=lambda v: tr.relbucket(v, 3), scale=None)
    # key[hash] == ("resize", (("mode", "78"), ("w", 1024), ...))
"""
import csv
from collections import namedtuple

# block: dense 1-based id (SimPoint .bb column); hash: stable id (what
# tapir_vector reports); args: {name: (tag, value string)}, tag "" if none.
Block = namedtuple("Block", "block hash lib function label args calls weight")


def read_blocks(path):
    """`<name>.blocks.*.csv` -> {hash: Block}."""
    out = {}
    with open(path, newline="") as f:
        for r in csv.DictReader(f):
            args = {}
            for tok in (r.get("args") or "").split():
                name_tag, _, val = tok.partition("=")
                name, _, tag = name_tag.partition("@")
                args[name] = (tag, unescape(val))
            b = Block(int(r["block"]), int(r["hash"]), r["lib"], r["function"], r["label"], args,
                      int(r["calls"]), int(r["weight"]))
            out[b.hash] = b
    return out


def read_bb(path):
    """SimPoint `.bb` -> list (one per interval) of {block: weight}."""
    vecs = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line.startswith("T"):
                continue
            v = {}
            for tok in line[1:].split():
                _, b, w = tok.split(":")
                v[int(b)] = int(w)
            vecs.append(v)
    return vecs


def unescape(v):
    """Undo the percent-escaping tapir applies to strings in labels."""
    if "%" not in v:
        return v
    out, i = [], 0
    while i < len(v):
        if v[i] == "%" and i + 3 <= len(v):
            out.append(chr(int(v[i + 1:i + 3], 16)))
            i += 3
        else:
            out.append(v[i])
            i += 1
    return "".join(out)


def read_vector_rows(path, prefix):
    """`<prefix>bb_id,<region>,ids...` / `<prefix>bbv,<region>,weights...` row
    pairs (as written from tapir_vector by an online driver) ->
    {region: {hash: weight}}."""
    ids, vals = {}, {}
    with open(path) as f:
        for line in f:
            p = line.rstrip("\n").split(",")
            if p[0] == prefix + "bb_id":
                ids[int(p[1])] = [int(x) for x in p[2:] if x]
            elif p[0] == prefix + "bbv":
                vals[int(p[1])] = [float(x) for x in p[2:] if x]
    return {r: dict(zip(ids[r], vals.get(r, []))) for r in ids}


def relbucket(v, bits):
    """Round to `bits` significant bits (same as tapir `sig/<bits>`)."""
    a = abs(int(v))
    if a < (1 << bits):
        return int(v)
    step = 1 << (a.bit_length() - bits)
    return ((a + (step >> 1)) // step) * step


def rebucket(blocks, **by_tag):
    """Re-key exact blocks: `by_tag` maps a tag ("size", "dim", ...) to a
    function on the integer value, or None to drop arguments with that tag.
    Untagged arguments and tags not given are kept as-is. Returns
    {hash: (function, ((arg, value), ...))}; blocks with equal tuples belong
    to the same coarser block."""
    out = {}
    for h, b in blocks.items():
        key = []
        for name, (tag, val) in b.args.items():
            fn = by_tag.get(tag, lambda x: x) if tag else (lambda x: x)
            if fn is None:
                continue
            try:
                key.append((name, fn(int(val))))
            except ValueError:   # already bucketed (e.g. "512..1023")
                key.append((name, val))
        out[h] = (b.function, tuple(key))
    return out
