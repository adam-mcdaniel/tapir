"""Check tapir-cuda's output for the fake CUDA app (see run.sh)."""
import csv, os, sys

d, stdout = sys.argv[1], open(sys.argv[2]).read()
blocks = list(csv.DictReader(open(os.path.join(d, "bbv.blocks.csv"))))
labels = {b["label"]: int(b["calls"]) for b in blocks}
libs = {b["function"]: b["lib"] for b in blocks}
for l, n in sorted(labels.items()):
    print(f"{n:3}  {l}")

def has(label, calls):
    assert labels.get(label) == calls, (label, calls, labels)

# Sizes by bit length, directions exact.
has("cudaMalloc(size=512..1023)", 2)
has("cudaMalloc(size=2147483648..4294967295)", 1)
has("cudaMemcpy(size=4096..8191,kind=1)", 1)
has("cudaMemcpy(size=4096..8191,kind=2)", 1)
has("cudaMemcpyAsync_ptsz(size=4096..8191,kind=1)", 1)
has("cudaMemset(size=65536..131071)", 1)
has("cudaFree()", 1)
# Launches: kernel by symbol (or binary+offset), total blocks, threads per block.
has("cudaLaunchKernel(kernel=axpy_kernel,blocks=128..255,threads=256..511)", 5)
has("cudaLaunchKernelExC(kernel=axpy_kernel,blocks=8..15,threads=64..127)", 1)
assert any(l.startswith("cudaLaunchKernel(kernel=app+0x") and l.endswith(",blocks=1..1,threads=32..63)") for l in labels), labels
# Math library through the generated wrappers; setup calls are left out.
has("cublasDgemm_v2(transa=0,transb=1,m=256..511,n=256..511,k=128..255,lda=256..511,ldb=256..511,ldc=256..511)", 2)
assert not any("Create" in l for l in labels), labels
assert libs["cublasDgemm_v2"] == "cublas" and libs["cudaLaunchKernel"] == "cuda", libs
# The real functions received exactly what the app passed.
for line in [
    "real cudaMalloc 3221225472",
    "real cudaMemcpyAsync_ptsz 5000 1 0x10",
    "real cudaLaunchKernel 1 104.2.1 256.1.1 1 64 0x20",
    "real cudaLaunchKernelExC 1 3.3.1 8.8.1 1",
    "real cublasCreate_v2",
    "real cublasDgemm_v2 0 1 400 400 250 1 400 400 0.5 400",
]:
    assert line in stdout, (line, stdout)
assert "bad return" not in stdout, stdout
print("OK")
