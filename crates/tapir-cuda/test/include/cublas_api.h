/* Two prototypes in cuBLAS's header style, for the GPU-free test (run.sh)
   on machines without a CUDA toolkit. Not the real header. */
typedef enum { CUBLAS_STATUS_SUCCESS = 0 } cublasStatus_t;
typedef enum { CUBLAS_OP_N = 0, CUBLAS_OP_T = 1 } cublasOperation_t;
typedef struct cublasContext* cublasHandle_t;

CUBLASAPI cublasStatus_t CUBLASWINAPI cublasCreate_v2(cublasHandle_t* handle);
CUBLASAPI cublasStatus_t CUBLASWINAPI cublasDgemm_v2(cublasHandle_t handle,
                                                     cublasOperation_t transa,
                                                     cublasOperation_t transb,
                                                     int m,
                                                     int n,
                                                     int k,
                                                     const double* alpha,
                                                     const double* A,
                                                     int lda,
                                                     const double* B,
                                                     int ldb,
                                                     const double* beta,
                                                     double* C,
                                                     int ldc);
