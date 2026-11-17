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

// Default arguments: a call may leave them out, and C++ supplies them (spec §15.2).
int scaled(int value, int times = 2, int plus = 0);

// An overloaded free function: every overload is imported, and the call chooses by its
// arguments' count and types (spec §15.3). The second takes a default, so `describe(p)`
// reaches it with one argument.
int describe(int value);
int describe(Point p, int weight = 10);
int describe(unsigned value, unsigned bias);

// An array parameter is a pointer to its first element in C++, and the whole array by
// reference in Dazzle: `&mut [3]f32` (spec §15.2).
void halve(float rgb[3]);
float total(const float rgb[3]);

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

// An overload set whose members differ in a parameter Dazzle passes with a cast: an
// array, which reaches C++ as the pointer to its first element. The cast is the overload's
// own, so the call must know which one it reached (T-0017).
class Summer {
public:
    int total(const int xs[4]) const;
    int total(int a, int b) const;
};

// A class whose move is code of its own: it keeps its own address, and its move constructor
// sets it again. Copying its bytes would leave it pointing at the old object, so a `List`
// of it moves each element through its move constructor (D-0105). It has no default
// constructor: a `List` of it compiles though `List::resize` would need one, and a call to
// `resize` is refused (T-0230).
class Anchored {
public:
    explicit Anchored(int id);
    Anchored(Anchored&& other);
    Anchored(const Anchored&) = delete;
    ~Anchored();

    // Its id, or -1 if it was moved without its move constructor.
    int id() const;

private:
    const Anchored* self;
    int value;
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
