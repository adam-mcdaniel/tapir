#include "fakeblas.h"
/* fake_dgetrf calls fake_dgemm internally through the PLT, like rocSOLVER
   calling rocBLAS, so nested-call handling can be tested. */
int fake_dgemm(fake_handle h, fake_operation ta, fake_operation tb, fake_int m, fake_int n,
               fake_int k, const double *alpha, const double *A, fake_int lda,
               const double *B, fake_int ldb, const double *beta, double *C, fake_int ldc) {
    (void)h; (void)ta; (void)tb; (void)alpha; (void)A; (void)lda; (void)B; (void)ldb; (void)beta; (void)C; (void)ldc;
    volatile double s = 0; for (long i = 0; i < (long)m * n * k / 64; i++) s += i;
    return 0;
}
int fake_dgetrf(fake_handle h, const fake_int m, const fake_int n, double *A,
                const fake_int lda, fake_int *ipiv, fake_int *info) {
    (void)ipiv;
    for (int j = 0; j < n; j += 64)
        fake_dgemm(h, fake_op_none, fake_op_none, m - j, 64, 64, 0, A, lda, A, lda, 0, A, lda);
    *info = 0;
    return 0;
}
