// The C++ half of `ecs` (T-0172): the battle of `src/main.dz`, on the library in `ecs.hpp`.
// `separate` nests a query in a query over the same storages and writes both units'
// velocities, and `fight` writes another unit's health while walking positions, as the
// Dazzle half does. C++ checks none of it: a `despawn` inside either loop would compile.
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <optional>
#include <vector>

#include "ecs.hpp"

using ecs::Entity;
using ecs::World;

constexpr float ARENA = 800;
constexpr float PERSONAL = 64;
constexpr float PUSH = 0.01f;
constexpr float REACH = 900;
constexpr float STEER = 0.0005f;
constexpr float FRICTION = 0.98f;

struct Position {
    float x, y;
};
struct Velocity {
    float x, y;
};
struct Team {
    uint32_t side;
};
struct Health {
    int32_t hp;
};
struct Fallen {
    uint64_t count;
};

struct Rng {
    uint64_t state;
    uint64_t next() {
        state += 0x9E3779B97F4A7C15ull;
        uint64_t z = state;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
        return z ^ (z >> 31);
    }
    float unit() { return static_cast<float>(next() >> 40) / 16777216.0f; }
    uint64_t below(uint64_t n) { return next() % n; }
};

static void spawn_unit(World& world, Rng& rng, uint32_t side) {
    Entity e = world.spawn();
    float x = rng.unit() * ARENA;
    float y = rng.unit() * ARENA;
    world.insert(e, Position{x, y});
    float vx = rng.unit() - 0.5f;
    float vy = rng.unit() - 0.5f;
    world.insert(e, Velocity{vx, vy});
    world.insert(e, Team{side});
    world.insert(e, Health{50 + static_cast<int32_t>(rng.below(50))});
}

static void bounce(float& p, float& v) {
    if (p < 0) {
        p = -p;
        v = -v;
    }
    if (p > ARENA) {
        p = 2 * ARENA - p;
        v = -v;
    }
}

static void movement(World& world) {
    for (auto [e, p, v] : world.query2<Position, Velocity>()) {
        (void)e;
        v.x *= FRICTION;
        v.y *= FRICTION;
        p.x += v.x;
        p.y += v.y;
        bounce(p.x, v.x);
        bounce(p.y, v.y);
    }
}

static void separate(World& world) {
    for (auto [a, pa, va] : world.query2<Position, Velocity>()) {
        for (auto [b, pb, vb] : world.query2<Position, Velocity>()) {
            if (a.index < b.index) {
                float dx = pa.x - pb.x;
                float dy = pa.y - pb.y;
                float d2 = dx * dx + dy * dy;
                if (d2 < PERSONAL) {
                    float s = (PERSONAL - d2) * PUSH;
                    va.x += dx * s;
                    va.y += dy * s;
                    vb.x -= dx * s;
                    vb.y -= dy * s;
                }
            }
        }
    }
}

static void fight(World& world) {
    auto& health = world.storage<Health>();
    auto& velocity = world.storage<Velocity>();
    for (auto [a, pa, ta] : world.query2<Position, Team>()) {
        std::optional<Entity> best;
        float best_d2 = 0, bx = 0, by = 0;
        for (auto [b, pb, tb] : world.query2<Position, Team>()) {
            if (tb.side != ta.side) {
                float dx = pb.x - pa.x;
                float dy = pb.y - pa.y;
                float d2 = dx * dx + dy * dy;
                if (!best || d2 < best_d2) {
                    best = b;
                    best_d2 = d2;
                    bx = dx;
                    by = dy;
                }
            }
        }
        if (!best) continue;
        if (best_d2 < REACH) {
            health.get(*best)->hp -= 1;
        } else {
            Velocity* v = velocity.get(a);
            v->x += bx * STEER;
            v->y += by * STEER;
        }
    }
}

static void reinforce(World& world) {
    std::vector<Entity> dead;
    std::vector<uint32_t> sides;
    for (auto [e, h, t] : world.query2<Health, Team>()) {
        if (h.hp <= 0) {
            dead.push_back(e);
            sides.push_back(t.side);
        }
    }
    Rng rng = world.resource<Rng>();
    for (size_t i = 0; i < dead.size(); ++i) {
        world.despawn(dead[i]);
        spawn_unit(world, rng, sides[i]);
    }
    world.set_resource(rng);
    world.resource<Fallen>().count += dead.size();
}

int main(int argc, char** argv) {
    int64_t units = argc > 1 ? std::atoll(argv[1]) : 600;
    int64_t steps = argc > 2 ? std::atoll(argv[2]) : 100;

    World world;
    Rng rng{42};
    for (int64_t i = 0; i < units; ++i) spawn_unit(world, rng, static_cast<uint32_t>(i % 2));
    world.set_resource(rng);
    world.set_resource(Fallen{0});
    world.add_system(movement);
    world.add_system(separate);
    world.add_system(fight);
    world.add_system(reinforce);
    for (int64_t i = 0; i < steps; ++i) world.step();

    uint64_t checksum = 1469598103934665603ull;
    uint64_t alive = 0;
    int64_t hp = 0;
    for (size_t i = 0; i < world.slots(); ++i) {
        std::optional<Entity> e = world.entity_in(i);
        if (!e) continue;
        Position* p = world.storage<Position>().get(*e);
        if (!p) continue;
        Health* h = world.storage<Health>().get(*e);
        Team* t = world.storage<Team>().get(*e);
        uint64_t values[] = {e->index, e->generation, t->side, static_cast<uint64_t>(h->hp),
                             static_cast<uint64_t>(static_cast<int64_t>(p->x * 1024)),
                             static_cast<uint64_t>(static_cast<int64_t>(p->y * 1024))};
        for (uint64_t v : values) checksum = (checksum ^ v) * 1099511628211ull;
        alive += 1;
        hp += h->hp;
    }
    std::printf("%lld units, %lld steps: %llu alive, %llu fallen, %lld hp\n", static_cast<long long>(units),
                static_cast<long long>(steps), static_cast<unsigned long long>(alive),
                static_cast<unsigned long long>(world.resource<Fallen>().count), static_cast<long long>(hp));
    std::printf("checksum %llu\n", static_cast<unsigned long long>(checksum));
    return 0;
}
