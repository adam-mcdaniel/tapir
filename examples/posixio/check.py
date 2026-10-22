"""Check the posixio example's tapir output."""
import csv, sys, os

d = sys.argv[1]
blocks = list(csv.DictReader(open(os.path.join(d, "bbv.blocks.csv"))))
labels = {b["label"]: int(b["calls"]) for b in blocks}
for l, n in sorted(labels.items()):
    print(f"{n:3}  {l}")

def has(prefix, calls):
    hits = [n for l, n in labels.items() if l.startswith(prefix)]
    assert hits and sum(hits) == calls, (prefix, hits, calls)

# Strings are keyed exactly and escaped (space -> %20); two files, two modes.
has("fopen(path=" + d.replace(" ", "%20").rsplit("/tapir", 1)[0], 6)
assert sum(1 for l in labels if l.startswith("fopen(")) == 4, labels
# 1000 and 1001..1002 byte writes share a size class; 4096-byte elements do not.
has("fwrite(size=1..1,n=512..1023)", 6)
has("fwrite(size=4096..8191,n=2..3)", 3)
# Callbacks by name: the exported one by symbol, the static one by offset.
has("qsort(n=64..127,size=4..7,compar=cmp_up)", 1)
has("qsort(n=32..63,size=4..7,compar=cmp_up)", 1)
assert any(l.startswith("qsort(n=64..127,size=4..7,compar=app+0x") for l in labels), labels
trace = open(os.path.join(d, "trace.csv")).read()
assert 'mode=\"\"wb\"\"' in trace and "compar=cmp_up" in trace, trace[:500]
print("OK")
