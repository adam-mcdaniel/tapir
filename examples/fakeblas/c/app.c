/* A toy "application" driven like a Nuggets-instrumented binary: it closes
   an interval every iteration and peeks at the GPU BBV line online. The
   tapir_try_* helpers resolve at runtime, so the app also runs without tapir. */
#include <stdio.h>
#include <stdint.h>
#include "fakeblas.h"
#include "../../../include/tapir.h"

int main(void) {
    double buf[4];
    fake_int info, ipiv[4];
    char line[4096];
    for (int it = 0; it < 6; it++) {
        int sz = (it < 3) ? 512 : 4096;          /* phase change after it 2 */
        for (int r = 0; r < 4; r++) {
            fake_dgemm(0, fake_op_none, fake_op_trans, sz, sz, 64, 0, buf, sz, buf, sz, 0, buf, sz);
            fake_dgemm(0, fake_op_none, fake_op_none, sz + 7, sz, 64, 0, buf, sz, buf, sz, 0, buf, sz);
        }
        if (it % 2 == 1) fake_dgetrf(0, 256, 256, buf, 256, ipiv, &info);
        if (tapir_try_snapshot("bbv", 0, line, sizeof line) >= 0)
            printf("iter %d peek: %s", it, line);
        tapir_try_interval(-1);
    }
    printf("done (tapir %s)\n", tapir_try_active() ? "active" : "absent");
    return 0;
}
