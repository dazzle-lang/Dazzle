#include <stdlib.h>

#include "arena.h"

Thing *thing_make(int value) {
    Thing *t = (Thing *)malloc(sizeof(Thing));
    t->value = value;
    t->doubled = 0;
    return t;
}

void thing_free(Thing *t) {
    free(t);
}
