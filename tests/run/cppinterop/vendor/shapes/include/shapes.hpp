// A small C++ library, vendored as source, exercising what spec §15.2 says C++ maps to:
// namespaces, a class with methods and a constructor, an overload set, a class C++
// destroys itself, and a scoped enum.
#ifndef SHAPES_HPP
#define SHAPES_HPP

namespace shapes {

struct Point {
    int x;
    int y;
};

// A scoped enum: its enumerators are reached only through it (spec §15.2, §18.38).
enum class Kind {
    Round = 1,
    Square = 2,
};

/**
 * @brief The area of a shape of the given kind.
 *
 * A round shape is approximated rather than measured, which is enough for a test.
 *
 * @param kind which shape to measure
 * @param size the shape's defining length
 * @return the area, in whole units
 * @note This is not a real geometry library.
 */
int area_of(Kind kind, int size);

// References, which a C++ API takes almost everywhere a C one would take a pointer:
// `const Point&` is a `&Point` and `Point&` a `&mut Point` (spec §15.2).
int length_sq(const Point& p);
void translate(Point& p, int dx, int dy);

namespace geom {

// A nested namespace, which Dazzle reaches as `shapes::geom::manhattan`.
int manhattan(Point a, Point b);

}  // namespace geom

// A class with a constructor, a const method, a mutating method, and an overload set.
class Counter {
public:
    explicit Counter(int start);

    int value() const;
    void add(int by);
    // Overloaded on argument type: §15.3 resolves these by exact match at the call site.
    void add(Point p);

private:
    int total;
};

// A class that owns something and destroys it itself, with no copy constructor: Dazzle
// makes it move-only and never emits a `drop` for it (spec §15.2).
class Buffer {
public:
    explicit Buffer(int count);
    Buffer(const Buffer&) = delete;
    ~Buffer();

    int len() const;
    int sum() const;
    void fill(int value);

private:
    int* data;
    int count;
};

// A template: not imported on its own. `[cpp.instantiate]` names the instantiations a
// project wants, and each comes in as an ordinary concrete type (spec §15.4).
template <class T>
class Pair {
public:
    Pair(T first, T second) : a(first), b(second) {}

    T sum() const { return a + b; }
    T first() const { return a; }

private:
    T a;
    T b;
};

}  // namespace shapes

#endif
