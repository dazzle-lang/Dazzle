// A game's frame loop with a draw list: the C++ half of a whole-program benchmark
// (tests/bench_programs/README.md). `src/main.dz` is the same program in Dazzle, and both
// must print the same checksum.
//
// Written as a C++ reviewer would accept it: a std::vector of commands that is clear()ed
// every frame, each command owning a std::string label and a std::vector trail, on the
// default heap. std::pmr::monotonic_buffer_resource could put a frame's commands in an
// arena; that is what the Dazzle compiler would choose by itself (T-0120), and is not
// what C++ does unless asked.
//
// Usage: frame [frames]
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace {

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

    uint64_t below(uint64_t n) { return next() % n; }

private:
    uint64_t state_;
};

constexpr int32_t kUnits = 10000;
constexpr int32_t kWorld = 4000;
constexpr int32_t kView = 2000;

constexpr std::string_view kKinds[6] = {"orc", "archer", "knight", "goblin", "wizard", "skeleton"};

struct Point {
    int32_t x = 0;
    int32_t y = 0;
};

struct Unit {
    std::string name;
    int32_t x = 0;
    int32_t y = 0;
    int32_t vx = 0;
    int32_t vy = 0;
    int32_t hp = 100;
};

// A speed of -5 to 5 that is never 0, so every unit moves.
int32_t speed(Rng& rng) {
    int32_t v = int32_t(rng.below(10)) - 5;
    return v >= 0 ? v + 1 : v;
}

// One thing to draw: both its label and its trail own memory.
struct Command {
    std::string label;
    std::vector<Point> trail;
    int32_t depth = 0;
};

// The draw list, rebuilt every frame and emptied all at once by `begin`.
class Renderer {
public:
    void begin() { commands_.clear(); }

    void submit(Command c) { commands_.push_back(std::move(c)); }

    // Fold every command into the checksum, as drawing would read it.
    void flush() {
        for (const Command& c : commands_) {
            for (char b : c.label) {
                checksum = (checksum ^ uint64_t(uint8_t(b))) * 1099511628211ull;
            }
            checksum = (checksum ^ uint64_t(int64_t(c.depth))) * 1099511628211ull;
            for (const Point& p : c.trail) {
                checksum = (checksum ^ uint64_t(int64_t(p.x + 100000))) * 1099511628211ull;
                checksum = (checksum ^ uint64_t(int64_t(p.y + 100000))) * 1099511628211ull;
            }
        }
        drawn += int64_t(commands_.size());
    }

    uint64_t checksum = 1469598103934665603ull;
    int64_t drawn = 0;

private:
    std::vector<Command> commands_;
};

// Move a unit one step, turning back at the edges of the world.
void step(Unit& u) {
    u.x += u.vx;
    if (u.x < 0 || u.x >= kWorld) {
        u.vx = -u.vx;
        u.x += 2 * u.vx;
    }
    u.y += u.vy;
    if (u.y < 0 || u.y >= kWorld) {
        u.vy = -u.vy;
        u.y += 2 * u.vy;
    }
}

}  // namespace

int main(int argc, char** argv) {
    int64_t frames = argc > 1 ? std::strtoll(argv[1], nullptr, 10) : 600;

    Rng rng(1234);
    std::vector<Unit> units;
    units.reserve(kUnits);
    for (int32_t i = 0; i < kUnits; ++i) {
        std::string name = std::string(kKinds[i % 6]) + "-" + std::to_string(i);
        // One statement per draw: C++ leaves the order of a call's arguments unspecified,
        // and the halves have to draw the same numbers.
        int32_t x = int32_t(rng.below(kWorld));
        int32_t y = int32_t(rng.below(kWorld));
        int32_t vx = speed(rng);
        int32_t vy = speed(rng);
        units.push_back(Unit{std::move(name), x, y, vx, vy});
    }

    Renderer renderer;
    for (int64_t frame = 0; frame < frames; ++frame) {
        for (size_t i = 0; i < units.size(); ++i) {
            step(units[i]);
            if ((frame + int64_t(i)) % 13 == 0) {
                units[i].hp -= 7;
                if (units[i].hp <= 0) {
                    units[i].hp = 100;
                }
            }
        }

        int32_t left = int32_t(frame * 3 % int64_t(kWorld - kView));
        renderer.begin();
        for (size_t i = 0; i < units.size(); ++i) {
            const Unit& u = units[i];
            if (u.x < left || u.x >= left + kView) {
                continue;
            }
            size_t length = 2 + i % 7;
            std::vector<Point> trail;
            trail.reserve(length);
            for (size_t k = 0; k < length; ++k) {
                trail.push_back(Point{u.x - int32_t(k) * u.vx, u.y - int32_t(k) * u.vy});
            }
            std::string label = u.name + " " + std::to_string(u.hp) + "/100";
            renderer.submit(Command{std::move(label), std::move(trail), u.y});
        }
        renderer.flush();
    }

    std::printf("%lld frames, %d units, %lld commands drawn\n", (long long)frames, kUnits,
                (long long)renderer.drawn);
    std::printf("checksum %llu\n", (unsigned long long)renderer.checksum);
    return 0;
}
