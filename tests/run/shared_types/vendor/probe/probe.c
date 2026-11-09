/* Found only through the recipe's `uses = ["arena"]`: without it, this does not compile. */
#include "arena.h"
#include "probe.h"

int probe_value(const Thing *t) {
    return t->value + t->doubled;
}

void probe_double(Thing *t) {
    t->doubled = t->value * 2;
}
