/* Stands in for a Nugget-instrumented application: calls the hook's
   nugget_bb_hook() for "basic blocks" and a GPU-like library in between, with
   a phase change halfway through. */
#include <stdint.h>
#include <stdio.h>
#include "../../../examples/fakeblas/c/fakeblas.h"

void nugget_init(uint64_t total_bb_count);
void nugget_roi_begin_(void);
void nugget_roi_end_(void);
void nugget_bb_hook(uint64_t inst_count, uint64_t bb_id, uint64_t threshold);

int main(void) {
    double buf[4];
    fake_int ipiv[4], info;
    nugget_init(16);
    fake_dgemm(0, fake_op_none, fake_op_none, 64, 64, 64, 0, buf, 64, buf, 64, 0, buf, 64); /* pre-ROI */
    nugget_roi_begin_();
    for (int step = 0; step < 40; step++) {
        int phase = step < 20 ? 0 : 1;
        for (int b = 0; b < 8; b++) nugget_bb_hook(10, phase * 8 + b, 1000);
        if (phase == 0)
            fake_dgemm(0, fake_op_none, fake_op_trans, 1000, 1000, 64, 0, buf, 1000, buf, 1000, 0, buf, 1000);
        else
            fake_dgetrf(0, 3000, 3000, buf, 3000, ipiv, &info);
    }
    nugget_roi_end_();
    return 0;
}
