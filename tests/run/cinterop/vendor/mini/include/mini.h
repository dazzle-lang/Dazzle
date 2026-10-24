/* A small C library for M7a's end-to-end test (spec §15.1): vendored as source, never
   prebuilt, and compiled by `dz`. Each declaration is one shape the importer maps. */
#ifndef MINI_H
#define MINI_H

#ifdef __cplusplus
extern "C" {
#endif

#define MINI_VERSION 7
#define MINI_HALF 0.5f

typedef struct MiniVec2 {
    float x;
    float y;
} MiniVec2;

typedef enum MiniShape {
    MINI_CIRCLE = 1,
    MINI_SQUARE = 4
} MiniShape;

int mini_scale(int v);
MiniVec2 mini_add(MiniVec2 a, MiniVec2 b);
float mini_length_sq(MiniVec2 v);
int mini_sides(MiniShape s);
void mini_fill(int *out, int n, int value);
int mini_sum(const int *xs, int n);
const char *mini_name(void);
int mini_length(const char *s);

/* A handle: declared here and never defined, the way RGFW's window and C's `FILE` are.
   Dazzle imports it as an opaque type — passed as a pointer, never held by value, since
   nothing outside this library knows its size (spec §15.2). */
typedef struct MiniCounter MiniCounter;

/* `char *` is a writable C string. Dazzle's `^mut i8` is the right type but a different
   C++ one — `int8_t` is `signed char` — so the call is cast at the boundary (spec §15.2). */
void mini_upper(char *text);
long mini_long_sum(const long *values, int n);

/* A callback: a function pointer, which is Dazzle's own `fn(i32) -> i32` (spec §15.2). */
typedef int (*MiniMap)(int value);

int mini_apply(MiniMap map, int value);

/* A struct with a C array member and bit-fields — the shapes raylib's `Matrix` and many
   device structs are built from (spec §15.2). */
typedef struct MiniFlags {
    unsigned int visible : 1;
    unsigned int level : 3;
    int samples[4];
} MiniFlags;

/* An anonymous union written inside a struct: C makes its members members of the struct
   itself, so `e.pressed` reads the same memory as `e.moved` (spec §15.2). */
typedef struct MiniEvent {
    int kind;
    union {
        int pressed;
        float moved;
    };
} MiniEvent;

MiniEvent mini_event(int pressed);

MiniFlags mini_flags(void);
int mini_sample_sum(const MiniFlags *f);

/* A C union: its members are the same memory, as RGFW's event is (spec §15.2). */
typedef union MiniValue {
    int as_int;
    float as_float;
} MiniValue;

MiniValue mini_int_value(int n);
int mini_read_int(const MiniValue *v);

MiniCounter *mini_counter(void);
void mini_bump(MiniCounter *c);
int mini_count(const MiniCounter *c);

/* A field whose struct has no name at all, as miniaudio's `ma_device_config::playback`
   is, with another nested inside it. Dazzle names them `MiniConfig_output` and
   `MiniConfig_output_gain`; C++ spells them through `decltype` (spec §15.2). */
typedef struct MiniConfig {
    int rate;
    struct {
        int channels;
        struct {
            int left, right;
        } gain;
    } output;
} MiniConfig;

MiniConfig mini_config(void);
int mini_config_total(const MiniConfig *c);

#ifdef __cplusplus
}

/* sokol's headers add a reference overload beside each pointer function when compiled as
   C++. A call written `mini_config_total(&c)` must still reach the pointer one, rather than
   find the pair ambiguous (spec §15.3). */
inline int mini_config_total(const MiniConfig &c) { return mini_config_total(&c); }
#endif

#endif
