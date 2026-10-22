/* A stand-in for a GPU math library, so the interposition path can be
   tested anywhere. Mirrors rocBLAS calling conventions. */
#ifndef FAKEBLAS_H
#define FAKEBLAS_H
typedef struct _fake_handle *fake_handle;
typedef enum { fake_op_none = 111, fake_op_trans = 112 } fake_operation;
typedef int fake_int;

#define FAKE_EXPORT __attribute__((visibility("default")))

FAKE_EXPORT int fake_dgemm(fake_handle handle, fake_operation transA, fake_operation transB,
                           fake_int m, fake_int n, fake_int k, const double *alpha,
                           const double *A, fake_int lda, const double *B, fake_int ldb,
                           const double *beta, double *C, fake_int ldc);
FAKE_EXPORT int fake_dgetrf(fake_handle handle, const fake_int m, const fake_int n,
                            double *A, const fake_int lda, fake_int *ipiv, fake_int *info);
#endif
