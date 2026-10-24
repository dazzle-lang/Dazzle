#include "mini.h"

#ifndef MINI_SCALE
#error "MINI_SCALE comes from the recipe's `defines` in dazzle.toml"
#endif

int mini_scale(int v) { return v * MINI_SCALE; }

MiniVec2 mini_add(MiniVec2 a, MiniVec2 b) {
    MiniVec2 r;
    r.x = a.x + b.x;
    r.y = a.y + b.y;
    return r;
}

float mini_length_sq(MiniVec2 v) { return v.x * v.x + v.y * v.y; }

int mini_sides(MiniShape s) { return s == MINI_CIRCLE ? 0 : 4; }

void mini_fill(int *out, int n, int value) {
    for (int i = 0; i < n; ++i) {
        out[i] = value;
    }
}

int mini_sum(const int *xs, int n) {
    int total = 0;
    for (int i = 0; i < n; ++i) {
        total += xs[i];
    }
    return total;
}

const char *mini_name(void) { return "mini"; }

MiniEvent mini_event(int pressed) {
    MiniEvent e;
    e.kind = 1;
    e.pressed = pressed;
    return e;
}

void mini_upper(char *text) {
    for (int i = 0; text[i] != '\0'; ++i) {
        if (text[i] >= 'a' && text[i] <= 'z') {
            text[i] = (char)(text[i] - 'a' + 'A');
        }
    }
}

long mini_long_sum(const long *values, int n) {
    long total = 0;
    for (int i = 0; i < n; ++i) {
        total += values[i];
    }
    return total;
}

int mini_apply(MiniMap map, int value) { return map(value); }

MiniFlags mini_flags(void) {
    MiniFlags f;
    f.visible = 1;
    f.level = 5;
    f.samples[0] = 1;
    f.samples[1] = 2;
    f.samples[2] = 3;
    f.samples[3] = 4;
    return f;
}

int mini_sample_sum(const MiniFlags *f) {
    int total = 0;
    for (int i = 0; i < 4; ++i) {
        total += f->samples[i];
    }
    return total;
}

MiniValue mini_int_value(int n) {
    MiniValue v;
    v.as_int = n;
    return v;
}

int mini_read_int(const MiniValue *v) { return v->as_int; }

/* The handle's definition lives here, where only the library can see it. */
struct MiniCounter {
    int n;
};

static struct MiniCounter the_counter;

MiniCounter *mini_counter(void) { return &the_counter; }

void mini_bump(MiniCounter *c) { c->n += 1; }

int mini_count(const MiniCounter *c) { return c->n; }

MiniConfig mini_config(void) {
    MiniConfig c = {0};
    c.rate = 100;
    c.output.channels = 2;
    c.output.gain.left = 3;
    return c;
}

int mini_config_total(const MiniConfig *c) {
    return c->rate + c->output.channels + c->output.gain.left + c->output.gain.right;
}

int mini_length(const char *s) {
    int n = 0;
    while (s[n] != '\0') {
        ++n;
    }
    return n;
}

static void shout(char *text, int n) {
    for (int i = 0; i < n; i++) {
        if (text[i] >= 'a' && text[i] <= 'z') text[i] = (char)(text[i] - 'a' + 'A');
    }
}

MiniShoutProc mini_shout_ptr = shout;
