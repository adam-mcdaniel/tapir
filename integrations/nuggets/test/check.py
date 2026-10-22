"""Parse analysis-output.csv with the exact parser from simpoint_analysis.ipynb
and check that CPU and GPU rows line up per region."""
import sys

def parse_analysis_csv(path):  # verbatim from nuggets-rocHPL/simpoint_analysis.ipynb
    regions = {}
    with open(path) as f:
        next(f)
        for line in f:
            parts = line.rstrip('\n').split(',')
            if not parts or not parts[0]:
                continue
            row_type = parts[0]
            try:
                region = int(parts[1])
            except (IndexError, ValueError):
                continue
            if row_type in ('gpu_bb_id', 'gpu_bbv'):
                data = [int(x) for x in parts[2:] if x != '']
            elif row_type == 'gpu_dims':
                data = parts[2:]
            else:
                data = [int(x) for x in parts[3:] if x != '']
            regions.setdefault(region, {})[row_type] = data
    return regions

regions = parse_analysis_csv(sys.argv[1])
n = max(regions) + 1
kinds = set()
for r in range(n):
    rec = regions[r]
    assert {'bbv', 'bb_id', 'region_inst', 'gpu_bb_id', 'gpu_bbv'} <= rec.keys(), (r, rec.keys())
    assert len(rec['gpu_bb_id']) == len(rec['gpu_bbv']), r
    kinds.add(tuple(rec['gpu_bb_id']))
first, last = regions[0]['gpu_bb_id'], regions[n - 1]['gpu_bb_id']
print(f"{n} regions; GPU block sets seen: {len(kinds)}")
print(f"  region 0:   ids={first} counts={regions[0]['gpu_bbv']}")
print(f"  region {n-1}: ids={last} counts={regions[n-1]['gpu_bbv']}")
assert first != last, "phase change not visible in GPU vectors"
print("OK")
