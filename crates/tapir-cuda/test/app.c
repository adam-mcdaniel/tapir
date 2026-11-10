/* Calls the fake CUDA library the way a CUDA program does. */
#include <stddef.h>
#include <stdio.h>

typedef struct { unsigned x, y, z; } dim3;
typedef struct { dim3 grid, block; size_t smem; void *stream; void *attrs; unsigned n; } config_t;
int cudaMalloc(void **, size_t), cudaFree(void *), cudaMemcpy(void *, const void *, size_t, int);
int cudaMemcpyAsync_ptsz(void *, const void *, size_t, int, void *), cudaMemset(void *, int, size_t);
int cudaLaunchKernel(const void *, dim3, dim3, void **, size_t, void *);
int cudaLaunchKernelExC(const config_t *, const void *, void **);
int cublasCreate_v2(void **);
int cublasDgemm_v2(void *, int, int, int, int, int, const double *, const double *, int, const double *, int, const double *, double *, int);

/* Kernel host stubs: one exported (named by symbol), one static (binary+offset). */
void axpy_kernel(void) {}
static void local_kernel(void) {}

int main(void) {
    void *p, *h, *args[1] = {0};
    double one = 1.0, half = 0.5, a[4], b[4], c[4];
    cudaMalloc(&p, 1000);
    cudaMalloc(&p, 1023);            /* same size class */
    cudaMalloc(&p, (size_t)3 << 30); /* 2^31..2^32-1 */
    cudaMemcpy(p, a, 4096, 1);
    cudaMemcpy(a, p, 4096, 2);       /* other direction: another block */
    cudaMemcpyAsync_ptsz(p, a, 5000, 1, (void *)0x10);
    cudaMemset(p, 0, 70000);
    for (int i = 0; i < 5; i++) {
        dim3 g = {100 + i, 2, 1}, t = {256, 1, 1};
        int r = cudaLaunchKernel((const void *)axpy_kernel, g, t, args, 64, (void *)0x20);
        if (r != 7) printf("bad return %d\n", r);
    }
    dim3 g1 = {1, 1, 1}, t1 = {32, 1, 1};
    cudaLaunchKernel((const void *)local_kernel, g1, t1, args, 0, NULL);
    config_t cfg = {{3, 3, 1}, {8, 8, 1}, 0, NULL, NULL, 0};
    cudaLaunchKernelExC(&cfg, (const void *)axpy_kernel, args);
    cublasCreate_v2(&h);             /* setup call: not wrapped */
    cublasDgemm_v2(h, 0, 1, 300, 300, 200, &one, a, 300, b, 300, &half, c, 300);
    cublasDgemm_v2(h, 0, 1, 400, 400, 250, &one, a, 400, b, 400, &half, c, 400);
    cudaFree(p);
    return 0;
}
