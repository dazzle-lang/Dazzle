#include "tally.h"

#include <math.h>

int tally_add(int total, int times) {
    return total + times * TALLY_STEP;
}

double tally_root(double x) {
    return sqrt(x);
}
