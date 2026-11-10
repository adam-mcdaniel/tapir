/* A stand-in for libcudart and libcublas: every function prints what it
   received, so the test sees that arguments pass through the wrappers intact
   (dim3 by value in particular). */
#include <stddef.h>
#include <stdio.h>

typedef struct { unsigned x, y, z; } dim3;
typedef struct { dim3 grid, block; size_t smem; void *stream; void *attrs; unsigned n; } config_t;

int cudaMalloc(void **p, size_t size) { static char mem[16]; *p = mem; printf("real cudaMalloc %zu\n", size); return 0; }
int cudaFree(void *p) { (void)p; printf("real cudaFree\n"); return 0; }
int cudaMemcpy(void *d, const void *s, size_t n, int kind) { (void)d; (void)s; printf("real cudaMemcpy %zu %d\n", n, kind); return 0; }
int cudaMemcpyAsync_ptsz(void *d, const void *s, size_t n, int kind, void *st) { (void)d; (void)s; printf("real cudaMemcpyAsync_ptsz %zu %d %p\n", n, kind, st); return 0; }
int cudaMemset(void *d, int v, size_t n) { (void)d; printf("real cudaMemset %d %zu\n", v, n); return 0; }
int cudaLaunchKernel(const void *f, dim3 g, dim3 b, void **args, size_t smem, void *stream) {
    printf("real cudaLaunchKernel %d %u.%u.%u %u.%u.%u %d %zu %p\n", f != NULL, g.x, g.y, g.z, b.x, b.y, b.z, args != NULL, smem, stream);
    return 7;
}
int cudaLaunchKernelExC(const config_t *c, const void *f, void **args) {
    printf("real cudaLaunchKernelExC %d %u.%u.%u %u.%u.%u %d\n", f != NULL, c->grid.x, c->grid.y, c->grid.z, c->block.x, c->block.y, c->block.z, args != NULL);
    return 0;
}
int cudaDeviceSynchronize(void) { return 0; }
int cublasCreate_v2(void **h) { *h = NULL; printf("real cublasCreate_v2\n"); return 0; }
int cublasDgemm_v2(void *h, int ta, int tb, int m, int n, int k, const double *alpha, const double *A, int lda,
                   const double *B, int ldb, const double *beta, double *C, int ldc) {
    (void)h; (void)A; (void)B; (void)C;
    printf("real cublasDgemm_v2 %d %d %d %d %d %g %d %d %g %d\n", ta, tb, m, n, k, *alpha, lda, ldb, *beta, ldc);
    return 0;
}
