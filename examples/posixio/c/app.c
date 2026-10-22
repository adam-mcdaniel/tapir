/* Exercises the posixio wrappers: string arguments, size classes and
   callbacks (one exported, one file-static). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int cmp_up(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }
static int cmp_down(const void *a, const void *b) { return *(const int *)b - *(const int *)a; }

int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : ".";
    char path[4096];
    static char buf[1 << 20];
    memset(buf, 'x', sizeof buf);
    for (int i = 0; i < 3; i++) {
        snprintf(path, sizeof path, "%s/data %d.bin", dir, i % 2); /* names with a space */
        FILE *f = fopen(path, "wb");
        fwrite(buf, 1, 1000, f);       /* 1000 bytes: 512..1023 class */
        fwrite(buf, 1, 1000 + i, f);   /* same class */
        fwrite(buf, 4096, 3, f);       /* 4096-byte elements */
        fclose(f);
        f = fopen(path, "rb");
        while (fread(buf, 1, 70000, f) > 0) {}
        fclose(f);
    }
    int v[100];
    for (int i = 0; i < 100; i++) v[i] = (i * 37) % 100;
    qsort(v, 100, sizeof v[0], cmp_up);
    qsort(v, 100, sizeof v[0], cmp_down);
    qsort(v, 60, sizeof v[0], cmp_up);
    printf("sorted: %d..%d\n", v[0], v[99]);
    return 0;
}
