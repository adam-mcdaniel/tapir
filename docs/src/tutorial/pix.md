# The example library

The tutorial wraps `pix`, a tiny C image library. It is small, but its API
has every kind of parameter a wrapper must handle:

| Parameter | C type | Kind |
|---|---|---|
| `img` | `pix_image` (a `struct pix_image *`) | opaque handle |
| `path` | `const char *` | string |
| `width`, `height` | `pix_int` (an `int`) | integer |
| `filter` | `pix_filter` | enum |
| `radius` | `float` | floating-point scalar |
| `format` | `char` | single character |
| `fn` | `int (*)(int)` | callback |

## Directory layout

Put the tutorial in a working directory next to your tapir checkout:

```text
work/
├── tapir/        # the tapir repository, built as in "Building tapir"
├── pix/          # the C library and the application (this chapter)
└── tapir-pix/    # the wrapper library (next chapter)
```

> The wrapper crate must **not** be inside the `tapir/` directory. Cargo
> would treat it as an unlisted member of tapir's workspace and refuse to
> build it.

## `pix/pix.h`

```c
/* pix: a tiny image library, used as the example throughout the tapir book. */
#ifndef PIX_H
#define PIX_H
#include <stddef.h>

#define PIX_API __attribute__((visibility("default")))

typedef struct pix_image *pix_image;                         /* opaque handle */
typedef enum { PIX_NEAREST = 0, PIX_BILINEAR = 1 } pix_filter;
typedef int pix_int;

PIX_API pix_image pix_load(const char *path);
PIX_API int pix_save(pix_image img, const char *path);
PIX_API int pix_resize(pix_image img, pix_int width, pix_int height, pix_filter filter);
PIX_API int pix_blur(pix_image img, float radius);
PIX_API int pix_convert(pix_image img, char format);         /* 'g' gray, 'c' color */
PIX_API int pix_map(pix_image img, int (*fn)(int value));    /* apply fn to every pixel */
PIX_API size_t pix_bytes(pix_image img);
PIX_API void pix_free(pix_image img);
#endif
```

## `pix/pix.c`

The implementation does just enough to be realistic about timing: resizing
allocates and clears memory, and `pix_map` calls the callback once per
pixel.

```c
#include <stdlib.h>
#include <string.h>
#include "pix.h"

struct pix_image { pix_int w, h; int *px; };

pix_image pix_load(const char *path) {
    pix_image img = malloc(sizeof *img);
    img->w = img->h = 64 + (int)(strlen(path) % 7);
    img->px = calloc((size_t)img->w * img->h, sizeof(int));
    return img;
}
int pix_save(pix_image img, const char *path) { (void)img; (void)path; return 0; }
int pix_resize(pix_image img, pix_int w, pix_int h, pix_filter f) {
    (void)f;
    img->px = realloc(img->px, (size_t)w * h * sizeof(int));
    memset(img->px, 0, (size_t)w * h * sizeof(int));
    img->w = w; img->h = h;
    return 0;
}
int pix_blur(pix_image img, float r) { (void)img; (void)r; return 0; }
int pix_convert(pix_image img, char fmt) { (void)img; return fmt == 'g' || fmt == 'c' ? 0 : -1; }
int pix_map(pix_image img, int (*fn)(int)) {
    for (long i = 0; i < (long)img->w * img->h; i++) img->px[i] = fn(img->px[i]);
    return 0;
}
size_t pix_bytes(pix_image img) { return (size_t)img->w * img->h * sizeof(int); }
void pix_free(pix_image img) { free(img->px); free(img); }
```

## `pix/app.c`

The application is a thumbnailer with two phases: small thumbnails, then
large ones.

```c
/* A thumbnailer: two phases with different image sizes. */
#include <stdio.h>
#include "pix.h"

int invert(int v) { return 255 - v; }          /* exported: named by symbol */
static int clamp(int v) { return v > 255 ? 255 : v; }  /* static: named by offset */

int main(void) {
    const char *files[] = {"cat.png", "dog.png", "bird.png"};
    for (int phase = 0; phase < 2; phase++) {
        int size = phase == 0 ? 128 : 1000;
        for (int i = 0; i < 3; i++) {
            pix_image img = pix_load(files[i]);
            pix_resize(img, size, size + i, PIX_BILINEAR);
            pix_blur(img, 1.5f + i);
            pix_convert(img, 'g');
            pix_map(img, i % 2 ? invert : clamp);
            pix_save(img, "out.png");
            pix_free(img);
        }
    }
    printf("done\n");
    return 0;
}
```

## Build and run

```sh
cd work/pix
gcc -shared -fPIC -O1 -o libpix.so pix.c
gcc -O1 -rdynamic -o app app.c -L. -lpix -Wl,-rpath,'$ORIGIN'
./app        # prints "done"
```

Two details matter for interposition:

- **`libpix.so` is a shared library.** tapir only sees calls that cross a
  shared-library boundary. If `pix.c` were compiled into `app`, or linked as
  a static archive, there would be nothing to intercept.
- **`-rdynamic`** exports `invert` from the executable, so tapir can report
  the callback by name. `clamp` is `static`, so it never has a symbol name.
  You will see how tapir names it anyway.

`-Wl,-rpath,'$ORIGIN'` only lets `app` find `libpix.so` next to itself.
You could set `LD_LIBRARY_PATH` instead.
