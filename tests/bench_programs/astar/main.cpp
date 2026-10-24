// A* path-finding on a generated grid: the C++ half of a whole-program benchmark
// (tests/bench_programs/README.md). `src/main.dz` is the same program in Dazzle, and both
// must print the same checksum.
//
// Written as a C++ reviewer would accept it: std::priority_queue, std::unordered_map with
// a hash for the key, std::vector, values and RAII throughout.
//
// Usage: astar [side] [queries]
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <optional>
#include <queue>
#include <unordered_map>
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

struct Cell {
    int32_t x = 0;
    int32_t y = 0;

    bool operator==(const Cell& o) const { return x == o.x && y == o.y; }
    bool operator!=(const Cell& o) const { return !(*this == o); }
};

struct CellHash {
    size_t operator()(const Cell& c) const noexcept {
        size_t seed = std::hash<int32_t>{}(c.x);
        size_t h = std::hash<int32_t>{}(c.y);
        return seed ^ (h + 0x9E3779B97F4A7C15ull + (seed << 6) + (seed >> 2));
    }
};

class Grid {
public:
    Grid(int32_t width, int32_t height, Rng& rng) : width_(width), height_(height) {
        walls_.reserve(size_t(width) * size_t(height));
        for (int64_t i = 0; i < int64_t(width) * height; ++i) {
            walls_.push_back(rng.below(100) < 30);
        }
    }

    int32_t width() const { return width_; }

    bool is_open(Cell c) const {
        if (c.x < 0 || c.y < 0 || c.x >= width_ || c.y >= height_) {
            return false;
        }
        return !walls_[size_t(c.y) * size_t(width_) + size_t(c.x)];
    }

    Cell random_open(Rng& rng) const {
        Cell c;
        do {
            c.x = int32_t(rng.below(uint64_t(width_)));
            c.y = int32_t(rng.below(uint64_t(height_)));
        } while (!is_open(c));
        return c;
    }

private:
    int32_t width_;
    int32_t height_;
    // Not std::vector<bool>: the Dazzle half stores one byte per cell, and a reviewer would
    // not want the bit-packed specialisation in a hot loop either.
    std::vector<uint8_t> walls_;
};

// The same total order as the Dazzle `Node`: by f, then the deeper node, then position.
struct Node {
    int32_t f;
    int32_t g;
    Cell cell;
};

bool operator>(const Node& a, const Node& b) {
    if (a.f != b.f) return a.f > b.f;
    if (a.g != b.g) return a.g < b.g;
    if (a.cell.y != b.cell.y) return a.cell.y > b.cell.y;
    return a.cell.x > b.cell.x;
}

int32_t distance(Cell a, Cell b) {
    return std::abs(a.x - b.x) + std::abs(a.y - b.y);
}

struct Found {
    std::optional<std::pair<int32_t, uint64_t>> path;
    int32_t expanded = 0;
};

constexpr int32_t kStepsX[4] = {1, 0, -1, 0};
constexpr int32_t kStepsY[4] = {0, 1, 0, -1};

std::pair<int32_t, uint64_t> trace(const std::unordered_map<Cell, Cell, CellHash>& came_from,
                                   Cell start, Cell goal, int32_t width) {
    std::vector<Cell> path;
    Cell at = goal;
    path.push_back(at);
    while (at != start) {
        at = came_from.at(at);
        path.push_back(at);
    }
    std::reverse(path.begin(), path.end());
    uint64_t h = 1469598103934665603ull;
    for (const Cell& c : path) {
        h = (h ^ uint64_t(int64_t(c.y * width + c.x))) * 1099511628211ull;
    }
    return {int32_t(path.size() - 1), h};
}

Found find_path(const Grid& grid, Cell start, Cell goal) {
    std::priority_queue<Node, std::vector<Node>, std::greater<Node>> open;
    std::unordered_map<Cell, int32_t, CellHash> best;
    std::unordered_map<Cell, Cell, CellHash> came_from;
    Found found;

    best[start] = 0;
    open.push(Node{distance(start, goal), 0, start});
    while (!open.empty()) {
        Node node = open.top();
        open.pop();
        // A stale entry: this cell was reached more cheaply after it was pushed.
        if (node.g > best.at(node.cell)) {
            continue;
        }
        ++found.expanded;
        if (node.cell == goal) {
            found.path = trace(came_from, start, goal, grid.width());
            return found;
        }
        for (int d = 0; d < 4; ++d) {
            Cell next{node.cell.x + kStepsX[d], node.cell.y + kStepsY[d]};
            if (!grid.is_open(next)) {
                continue;
            }
            int32_t g = node.g + 1;
            auto [it, inserted] = best.try_emplace(next, INT32_MAX);
            if (g >= it->second) {
                continue;
            }
            it->second = g;
            came_from[next] = node.cell;
            open.push(Node{g + distance(next, goal), g, next});
        }
    }
    return found;
}

}  // namespace

int main(int argc, char** argv) {
    int64_t side = argc > 1 ? std::strtoll(argv[1], nullptr, 10) : 400;
    int64_t queries = argc > 2 ? std::strtoll(argv[2], nullptr, 10) : 200;

    Rng rng(42);
    Grid grid(int32_t(side), int32_t(side), rng);

    uint64_t checksum = 0;
    int64_t reached = 0;
    int64_t total_length = 0;
    int64_t total_expanded = 0;
    for (int64_t q = 0; q < queries; ++q) {
        Cell start = grid.random_open(rng);
        Cell goal = grid.random_open(rng);
        Found found = find_path(grid, start, goal);
        total_expanded += found.expanded;
        checksum = checksum * 31 + uint64_t(found.expanded);
        if (found.path) {
            auto [length, h] = *found.path;
            ++reached;
            total_length += length;
            checksum = (checksum ^ h) * 31 + uint64_t(length);
        }
    }
    std::printf("reached %lld of %lld, total length %lld, expanded %lld\n", (long long)reached,
                (long long)queries, (long long)total_length, (long long)total_expanded);
    std::printf("checksum %llu\n", (unsigned long long)checksum);
    return 0;
}
