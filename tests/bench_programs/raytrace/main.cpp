// A ray tracer: the C++ half of a whole-program benchmark (tests/bench_programs/README.md).
// `src/main.dz` is the same program in Dazzle, and both must print the same checksum.
//
// Written as a C++ reviewer would accept it: a `Vec3` struct with operators, passed by value,
// std::variant for the shapes and materials, std::optional for a hit, std::vector, and
// <cmath>'s float overloads. Every float expression is the one the Dazzle half writes, in the
// same order, so the two compute the same bits.
//
// Usage: raytrace [width] [height] [grid]
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <optional>
#include <variant>
#include <vector>

namespace {

struct Vec3 {
    float x = 0, y = 0, z = 0;

    float dot(Vec3 r) const { return x * r.x + y * r.y + z * r.z; }
    float length() const { return std::sqrt(x * x + y * y + z * z); }
    Vec3 normalized() const {
        float len = length();
        if (len == 0) return Vec3{};
        return Vec3{x / len, y / len, z / len};
    }
    Vec3 lerp(Vec3 to, float t) const {
        return Vec3{x + (to.x - x) * t, y + (to.y - y) * t, z + (to.z - z) * t};
    }
};

Vec3 operator+(Vec3 a, Vec3 b) { return Vec3{a.x + b.x, a.y + b.y, a.z + b.z}; }
Vec3 operator-(Vec3 a, Vec3 b) { return Vec3{a.x - b.x, a.y - b.y, a.z - b.z}; }
Vec3 operator*(Vec3 a, Vec3 b) { return Vec3{a.x * b.x, a.y * b.y, a.z * b.z}; }
Vec3 operator*(Vec3 a, float s) { return Vec3{a.x * s, a.y * s, a.z * s}; }
Vec3 operator/(Vec3 a, float s) { return Vec3{a.x / s, a.y / s, a.z / s}; }

Vec3 reflect(Vec3 v, Vec3 normal) { return v - normal * (2 * v.dot(normal)); }

constexpr float TAU = 6.28318530717959f;

// SplitMix64, the same generator the Dazzle half writes out.
class Rng {
public:
    explicit Rng(uint64_t seed) : state_(seed) {}

    uint64_t next() {
        state_ += 0x9E3779B97F4A7C15ull;
        uint64_t z = state_;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
        return z ^ (z >> 31);
    }

    // In [0, 1): the top 24 bits, which a float holds exactly.
    float next_f32() { return float(next() >> 40) / 16777216.0f; }

    Vec3 unit_vec3() {
        float z = next_f32() * 2.0f - 1.0f;
        float angle = next_f32() * TAU;
        float r = std::sqrt(1.0f - z * z);
        return Vec3{r * std::cos(angle), r * std::sin(angle), z};
    }

private:
    uint64_t state_;
};

constexpr float LIGHT_RADIUS = 0.7f;
constexpr int MAX_DEPTH = 5;

struct Ray {
    Vec3 origin;
    Vec3 dir;
};

struct Matte { Vec3 color; };
struct Mirror { Vec3 tint; };
struct Checker { Vec3 a, b; };
using Material = std::variant<Matte, Mirror, Checker>;

struct Sphere { Vec3 center; float radius; Material material; };
struct Floor { float y; Material material; };
using Shape = std::variant<Sphere, Floor>;

struct Hit {
    float t;
    Vec3 point;
    Vec3 normal;
    Material material;
};

// Where `ray` first meets `shape`, if it does before `t_max`.
std::optional<Hit> intersect(const Shape& shape, const Ray& ray, float t_max) {
    if (const Sphere* s = std::get_if<Sphere>(&shape)) {
        Vec3 oc = ray.origin - s->center;
        float b = oc.dot(ray.dir);
        float c = oc.dot(oc) - s->radius * s->radius;
        float disc = b * b - c;
        if (disc < 0) return std::nullopt;
        float t = -b - std::sqrt(disc);
        if (t < 0.001f || t > t_max) return std::nullopt;
        Vec3 p = ray.origin + ray.dir * t;
        return Hit{t, p, (p - s->center) / s->radius, s->material};
    }
    const Floor& f = std::get<Floor>(shape);
    if (ray.dir.y > -0.0001f) return std::nullopt;
    float t = (f.y - ray.origin.y) / ray.dir.y;
    if (t < 0.001f || t > t_max) return std::nullopt;
    Vec3 p = ray.origin + ray.dir * t;
    return Hit{t, p, Vec3{0, 1, 0}, f.material};
}

struct Scene {
    std::vector<Shape> shapes;
    Vec3 light{-4, 7, -3};

    std::optional<Hit> closest(const Ray& ray, float t_max) const {
        std::optional<Hit> best;
        float limit = t_max;
        for (const Shape& shape : shapes) {
            if (std::optional<Hit> hit = intersect(shape, ray, limit)) {
                limit = hit->t;
                best = hit;
            }
        }
        return best;
    }

    Vec3 sky(Vec3 dir) const {
        float t = 0.5f * (dir.y + 1);
        return Vec3{1, 0.95f, 0.9f}.lerp(Vec3{0.35f, 0.55f, 0.95f}, t);
    }

    // Light arriving along `ray`.
    Vec3 trace(const Ray& ray, int depth, Rng& rng) const {
        std::optional<Hit> found = closest(ray, 1000);
        if (!found) return sky(ray.dir);
        const Hit& hit = *found;

        Vec3 base;
        if (const Mirror* m = std::get_if<Mirror>(&hit.material)) {
            if (depth >= MAX_DEPTH) return Vec3{};
            Ray bounce{hit.point, reflect(ray.dir, hit.normal)};
            return trace(bounce, depth + 1, rng) * m->tint;
        } else if (const Matte* m = std::get_if<Matte>(&hit.material)) {
            base = m->color;
        } else {
            const Checker& c = std::get<Checker>(hit.material);
            bool odd = int32_t(std::floor(hit.point.x) + std::floor(hit.point.z)) % 2 != 0;
            base = odd ? c.a : c.b;
        }

        // Lambert shading and a Blinn-Phong highlight, with a shadow ray towards a point on
        // the light.
        Vec3 to_light = light + rng.unit_vec3() * LIGHT_RADIUS - hit.point;
        Vec3 l = to_light.normalized();
        Ray shadow{hit.point, l};
        bool blocked = closest(shadow, to_light.length()).has_value();
        float lit = blocked ? 0 : std::max(hit.normal.dot(l), 0.0f);
        float shine =
            blocked ? 0 : std::pow(std::max(hit.normal.dot((l - ray.dir).normalized()), 0.0f), 60.0f) * 0.5f;
        float fog = std::exp(-hit.t * 0.02f);
        Vec3 color = base * (0.12f + 0.88f * lit) + Vec3{shine, shine, shine};
        return color * fog + sky(ray.dir) * (1 - fog);
    }
};

Scene build_scene() {
    Scene scene;
    scene.shapes.push_back(Floor{0, Checker{{0.9f, 0.9f, 0.88f}, {0.2f, 0.22f, 0.28f}}});
    scene.shapes.push_back(Sphere{{0, 1, 4}, 1, Mirror{{0.95f, 0.85f, 0.6f}}});
    scene.shapes.push_back(Sphere{{-2.2f, 0.7f, 5}, 0.7f, Matte{{0.9f, 0.25f, 0.2f}}});
    scene.shapes.push_back(Sphere{{2.1f, 0.6f, 3.4f}, 0.6f, Matte{{0.2f, 0.6f, 0.95f}}});
    scene.shapes.push_back(Sphere{{0.9f, 0.35f, 2.3f}, 0.35f, Mirror{{0.9f, 0.9f, 0.9f}}});
    return scene;
}

// One colour channel, gamma-corrected, as a byte.
uint8_t to_byte(float v) {
    return uint8_t(std::pow(std::clamp(v, 0.0f, 1.0f), 1.0f / 2.2f) * 255.0f + 0.5f);
}

// The image, three bytes a pixel. Each pixel takes `grid` x `grid` rays, one jittered within
// each cell of a grid across it.
std::vector<uint8_t> render(const Scene& scene, int width, int height, int grid, Rng& rng) {
    std::vector<uint8_t> pixels(size_t(width) * height * 3);
    const Vec3 eye{0, 1.4f, -1.5f};
    float aspect = float(width) / float(height);
    for (int y = 0; y < height; ++y) {
        for (int x = 0; x < width; ++x) {
            Vec3 sum;
            for (int sy = 0; sy < grid; ++sy) {
                for (int sx = 0; sx < grid; ++sx) {
                    float px = float(x) + (float(sx) + rng.next_f32()) / float(grid);
                    float py = float(y) + (float(sy) + rng.next_f32()) / float(grid);
                    float u = px / float(width) * 2 - 1;
                    float v = 1 - py / float(height) * 2;
                    Vec3 dir = Vec3{u * aspect * 0.6f, v * 0.6f - 0.18f, 1}.normalized();
                    sum = sum + scene.trace(Ray{eye, dir}, 0, rng);
                }
            }
            Vec3 c = sum / float(grid * grid);
            size_t i = (size_t(y) * width + x) * 3;
            pixels[i] = to_byte(c.x);
            pixels[i + 1] = to_byte(c.y);
            pixels[i + 2] = to_byte(c.z);
        }
    }
    return pixels;
}

}  // namespace

int main(int argc, char** argv) {
    int width = argc > 1 ? int(std::strtol(argv[1], nullptr, 10)) : 960;
    int height = argc > 2 ? int(std::strtol(argv[2], nullptr, 10)) : 540;
    int grid = argc > 3 ? int(std::strtol(argv[3], nullptr, 10)) : 4;

    const Scene scene = build_scene();
    Rng rng(2024);
    const std::vector<uint8_t> pixels = render(scene, width, height, grid, rng);

    // FNV-1a over every byte, and the mean brightness as a sanity line a reader can check.
    uint64_t checksum = 1469598103934665603ull;
    uint64_t total = 0;
    for (uint8_t b : pixels) {
        checksum = (checksum ^ uint64_t(b)) * 1099511628211ull;
        total += b;
    }
    std::printf("%dx%d, %d rays a pixel, mean byte %llu\n", width, height, grid * grid,
                (unsigned long long)(total / pixels.size()));
    std::printf("checksum %llu\n", (unsigned long long)checksum);
    return 0;
}
