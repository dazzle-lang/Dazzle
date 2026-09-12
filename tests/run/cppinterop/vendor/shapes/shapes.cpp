#include "shapes.hpp"

#include <stdlib.h>

namespace shapes {

int area_of(Kind kind, int size) {
    return kind == Kind::Square ? size * size : 3 * size;
}

int length_sq(const Point& p) { return p.x * p.x + p.y * p.y; }

void translate(Point& p, int dx, int dy) {
    p.x += dx;
    p.y += dy;
}

namespace geom {

static int abs_of(int v) { return v < 0 ? -v : v; }

int manhattan(Point a, Point b) {
    return abs_of(a.x - b.x) + abs_of(a.y - b.y);
}

}  // namespace geom

Counter::Counter(int start) : total(start) {}

int Counter::value() const { return total; }

void Counter::add(int by) { total += by; }

void Counter::add(Point p) { total += p.x + p.y; }

Buffer::Buffer(int count) : data((int*)calloc((size_t)count, sizeof(int))), count(count) {}

Buffer::~Buffer() { free(data); }

int Buffer::len() const { return count; }

int Buffer::sum() const {
    int total = 0;
    for (int i = 0; i < count; i++) {
        total += data[i];
    }
    return total;
}

void Buffer::fill(int value) {
    for (int i = 0; i < count; i++) {
        data[i] = value;
    }
}

}  // namespace shapes
