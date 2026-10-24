#ifndef TALLY_H
#define TALLY_H

/* Installed beside this header by the build, and nowhere in the source tree. */
#include "tally_config.h"

#ifndef TALLY_VIA_PKG_CONFIG
#error "the define in tally.pc's Cflags did not reach this include"
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* total + times * TALLY_STEP */
int tally_add(int total, int times);
/* The square root, from the C library's maths, which tally.pc says to link. */
double tally_root(double x);

#ifdef __cplusplus
}
#endif

#endif
