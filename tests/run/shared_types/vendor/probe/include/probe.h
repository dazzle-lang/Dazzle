#ifndef PROBE_H
#define PROBE_H

#ifdef __cplusplus
extern "C" {
#endif

/* Only declares `Thing`: taking another library's pointer needs no layout, as ImGui's SDL3
   backend takes an `SDL_Window *`. */
typedef struct Thing Thing;

int probe_value(const Thing *t);
void probe_double(Thing *t);

#ifdef __cplusplus
}
#endif

#endif
