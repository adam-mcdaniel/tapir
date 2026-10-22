/*
 * Nugget analysis hook with CPU + GPU basic-block vectors.
 *
 * This is the upstream CPU hook (LSMS-bench-adam toolchain/Nugget/hooks/
 * analysis.c) plus GPU library-call vectors from tapir. Drop it, together with
 * tapir.h, into toolchain/Nugget/hooks/ in place of analysis.c, then run the
 * analysis binary with:
 *
 *     TAPIR_PROFILERS=bbv LD_PRELOAD=/path/to/libtapir_rocm.so ./app-analysis-exec ...
 *
 * Without tapir loaded the hook behaves exactly like the CPU-only original
 * (the gpu_* rows are simply absent).
 *
 * Output (analysis-output.csv, or $NUGGET_ANALYSIS_OUTPUT), per region r:
 *     bbv,r,0,...            CPU BB counts (non-zero only)
 *     csv,r,0,...            IR-instruction stamps
 *     bb_id,r,0,...          CPU BB ids
 *     region_inst,r,0,N      IR instructions in region r
 *     timestamp,r,0,MS       wall clock (ms since epoch) at the end of region r
 *     gpu_bb_id,r,...        tapir block hashes (bucketed library calls)
 *     gpu_bbv,r,...          calls per block in region r
 * which is what simpoint_analysis.ipynb parses. The GPU rows are keyed by the
 * hook's own region index, so CPU and GPU data cannot drift apart.
 *
 * Environment:
 *     NUGGET_ANALYSIS_OUTPUT   output path (default analysis-output.csv)
 *     NUGGET_GPU_PROFILER      tapir profiler to sample (default "bbv")
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "tapir.h"

#define ARRAY_SIZE 10000

static uint64_t IR_inst_counter = 0;

static int if_start = 0;
static int in_hook = 0;
static uint64_t region = 0;
static uint64_t total_num_bbs = 0;
static uint64_t total_IR_inst = 0;

static uint64_t** bbv_array = NULL;
static uint64_t** count_stamp_array = NULL;

static uint64_t* bbv = NULL;
static uint64_t* count_stamp = NULL;
static uint64_t* counter_array = NULL;
static uint64_t* timestamp_array = NULL;

static uint64_t file_line_index = 0;
static uint64_t batch_index = 0;

static FILE* fptr = NULL;
static const char* gpu_profiler = "bbv";

static uint64_t now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
}

/* Take (close) the current interval of tapir profiler `name` and write it as
   two rows of the analysis CSV:
       <prefix>bb_id,<region>,<id>,<id>,...
       <prefix>bbv,<region>,<weight>,<weight>,...
   With prefix "gpu_" these are exactly the rows the Nuggets notebooks parse.
   Rows are written even for an empty interval, so every region has a record.
   f == NULL discards the interval instead (e.g. work done before the ROI).
   Returns the number of features, or -1 (and writes nothing) when no tapir
   profiler `name` is loaded. */
static int64_t write_vector_rows(FILE *f, const char *name, const char *prefix,
                                           uint64_t region) {
    uint64_t ids_s[256];
    double w_s[256];
    uint64_t *ids = ids_s;
    double *w = w_s;
    size_t cap = 256;
    int64_t n;
    for (;;) {
        n = tapir_try_vector(name, 1, ids, w, cap);
        if (n < 0 || (size_t)n <= cap) break;
        if (ids != ids_s) { free(ids); free(w); }
        cap = (size_t)n * 2;
        ids = (uint64_t *)malloc(cap * sizeof *ids);
        w = (double *)malloc(cap * sizeof *w);
        if (!ids || !w) { free(ids); free(w); return -1; }
    }
    if (n >= 0 && f) {
        fprintf(f, "%sbb_id,%llu", prefix, (unsigned long long)region);
        for (int64_t i = 0; i < n; i++) fprintf(f, ",%llu", (unsigned long long)ids[i]);
        fprintf(f, "\n%sbbv,%llu", prefix, (unsigned long long)region);
        for (int64_t i = 0; i < n; i++) fprintf(f, ",%.17g", w[i]);
        fputc('\n', f);
    }
    if (ids != ids_s) { free(ids); free(w); }
    return n;
}

/* Close region `r` on the GPU side: its library-call vector goes straight
   into the CSV, keyed by the same region index as the CPU rows. */
static void gpu_region_end(uint64_t r) {
    write_vector_rows(fptr, gpu_profiler, "gpu_", r);
}

static void write_down_data(uint64_t limit) {
    for (uint64_t i = 0; i < limit; i++) {
        fprintf(fptr, "bbv,%llu,0", (unsigned long long)file_line_index);
        for (uint64_t k = 0; k < total_num_bbs; k++) {
            if (bbv_array[i][k] != 0) {
                fprintf(fptr, ",%llu", (unsigned long long)bbv_array[i][k]);
            }
        }
        fprintf(fptr, "\n");
        fprintf(fptr, "csv,%llu,0", (unsigned long long)file_line_index);
        for (uint64_t k = 0; k < total_num_bbs; k++) {
            if (count_stamp_array[i][k] != 0) {
                fprintf(fptr, ",%llu", (unsigned long long)count_stamp_array[i][k]);
            }
        }
        fprintf(fptr, "\n");
        fprintf(fptr, "bb_id,%llu,0", (unsigned long long)file_line_index);
        for (uint64_t k = 0; k < total_num_bbs; k++) {
            if (bbv_array[i][k] != 0) {
                fprintf(fptr, ",%llu", (unsigned long long)k);
            }
        }
        fprintf(fptr, "\n");
        fprintf(fptr, "region_inst,%llu,0,%llu\n", (unsigned long long)file_line_index,
                (unsigned long long)counter_array[i]);
        fprintf(fptr, "timestamp,%llu,0,%llu\n", (unsigned long long)file_line_index,
                (unsigned long long)timestamp_array[i]);
        file_line_index++;
    }
}

static void reset_array(void) {
    for (uint64_t i = 0; i < ARRAY_SIZE; i++) {
        memset(bbv_array[i], 0, total_num_bbs * sizeof(uint64_t));
        memset(count_stamp_array[i], 0, total_num_bbs * sizeof(uint64_t));
    }
}

static void delete_array(void) {
    for (uint64_t i = 0; i < ARRAY_SIZE; i++) {
        free(bbv_array[i]);
        free(count_stamp_array[i]);
    }
    free(bbv_array);
    free(count_stamp_array);
    free(counter_array);
    free(timestamp_array);
}

static void close_region(void) {
    counter_array[batch_index] = IR_inst_counter;
    timestamp_array[batch_index] = now_ms();
    gpu_region_end(region);
    total_IR_inst += IR_inst_counter;
    region++;
    batch_index++;
}

static void process_data(void) {
    close_region();
    if (batch_index >= ARRAY_SIZE) {
        write_down_data(ARRAY_SIZE);
        reset_array();
        batch_index = 0;
    }
    bbv = bbv_array[batch_index];
    count_stamp = count_stamp_array[batch_index];
    IR_inst_counter = 0;
}

void nugget_init(uint64_t total_bb_count) {
    total_num_bbs = total_bb_count;
    bbv_array = (uint64_t**)malloc(ARRAY_SIZE * sizeof(uint64_t*));
    count_stamp_array = (uint64_t**)malloc(ARRAY_SIZE * sizeof(uint64_t*));
    counter_array = (uint64_t*)malloc(ARRAY_SIZE * sizeof(uint64_t));
    timestamp_array = (uint64_t*)malloc(ARRAY_SIZE * sizeof(uint64_t));
    if (bbv_array == NULL || count_stamp_array == NULL || counter_array == NULL ||
        timestamp_array == NULL) {
        printf("Failed to allocate memory for analysis arrays\n");
        exit(1);
    }
    for (uint64_t i = 0; i < ARRAY_SIZE; i++) {
        bbv_array[i] = (uint64_t*)malloc(total_num_bbs * sizeof(uint64_t));
        count_stamp_array[i] = (uint64_t*)malloc(total_num_bbs * sizeof(uint64_t));
        if (bbv_array[i] == NULL || count_stamp_array[i] == NULL) {
            printf("Failed to allocate memory for analysis arrays\n");
            exit(1);
        }
        memset(bbv_array[i], 0, total_num_bbs * sizeof(uint64_t));
        memset(count_stamp_array[i], 0, total_num_bbs * sizeof(uint64_t));
    }
    bbv = bbv_array[0];
    count_stamp = count_stamp_array[0];
}

void nugget_roi_begin_(void) {
    if_start = 1;
    printf("ROI begin\n");
    const char* path = getenv("NUGGET_ANALYSIS_OUTPUT");
    fptr = fopen(path && *path ? path : "analysis-output.csv", "w");
    if (fptr == NULL) {
        printf("Error: cannot open output file\n");
        exit(1);
    }
    fprintf(fptr, "type,region,thread,data\n");
    const char* prof = getenv("NUGGET_GPU_PROFILER");
    if (prof && *prof) gpu_profiler = prof;
    /* Drop GPU work done before the ROI so it does not land in region 0. */
    write_vector_rows(NULL, gpu_profiler, "gpu_", 0);
    printf("GPU vectors: %s\n", tapir_try_active() ? "tapir active" : "tapir not loaded (CPU only)");
}

void nugget_roi_end_(void) {
    if_start = 0;

    close_region();
    write_down_data(batch_index);

    printf("file_line_index: %llu\n", (unsigned long long)file_line_index);
    printf("region: %llu\n", (unsigned long long)region);

    fclose(fptr);
    delete_array();
    printf("ROI end\n");
    printf("Total IR instructions: %llu\n", (unsigned long long)total_IR_inst);
}

// Re-entrance guard: helper functions in this file will also be instrumented
// by PhaseAnalysisPass (they're not in the nugget_functions skip list), so
// nugget_bb_hook calls from within process_data/write_down_data must be ignored.
void nugget_bb_hook(uint64_t inst_count, uint64_t bb_id, uint64_t threshold) {
    if (in_hook) return;
    if (if_start) {
        in_hook = 1;
        IR_inst_counter += inst_count;
        bbv[bb_id] += 1;
        count_stamp[bb_id] = IR_inst_counter;
        if (IR_inst_counter > threshold) {
            process_data();
        }
        in_hook = 0;
    }
}
