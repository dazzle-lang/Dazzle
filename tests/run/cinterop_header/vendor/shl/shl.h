/* A single-header C library, the shape stb, RGFW and miniaudio use: declarations here, the
   implementation behind a define, and a `SHLDEF` that is `inline` unless the build says
   otherwise — because the library expects to be included in one translation unit.

   Dazzle compiles two against it: the library's own source, and the generated C++ that
   includes the header to call it. So the recipe puts `SHLDEF=` in `[cpp]`, which reaches
   both, and `SHL_IMPLEMENTATION` in the library's own table, which reaches only its source
   (spec §15.1). Getting either wrong is a link error, which is what this fixture pins. */
#ifndef SHL_H
#define SHL_H

#ifdef __cplusplus
extern "C" {
#endif

#ifndef SHLDEF
#define SHLDEF inline
#endif

SHLDEF int shl_double(int v);

#ifdef SHL_IMPLEMENTATION
SHLDEF int shl_double(int v) { return v * 2; }
#endif

#ifdef __cplusplus
}
#endif

#endif
