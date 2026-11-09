#ifndef ARENA_H
#define ARENA_H

#ifdef __cplusplus
extern "C" {
#endif

/* Defines `Thing`, which `probe.h` only declares. */
typedef struct Thing {
    int value;
    int doubled;
} Thing;

Thing *thing_make(int value);
void thing_free(Thing *t);

#ifdef __cplusplus
}
#endif

#endif
