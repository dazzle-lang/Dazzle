// Generic code instantiated many times: the C++ half of a whole-program benchmark
// (tests/bench_programs/README.md). `src/main.dz` is the same program in Dazzle, and both
// must print the same checksum.
//
// The same generic code as the Dazzle half, as templates: an AVL tree map in one
// std::vector, a quicksort, a binary search and a count of distinct elements, a Pair, and a
// driver taking the key and value it makes as function pointers, each instantiated for six
// key types. They are written out rather than taken from <map> and <algorithm> because
// the question is what instantiating the program's own generic code costs, at run time
// and at build time (T-0075); std::map and std::sort would compare two libraries instead.
//
// Usage: generics [n]
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <tuple>
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

uint64_t mix(uint64_t seed, uint64_t h) { return (seed ^ h) * 1099511628211ull; }

// What a value adds to the checksum.
uint64_t digest(int64_t v) { return static_cast<uint64_t>(v); }
uint64_t digest(int32_t v) { return static_cast<uint64_t>(static_cast<int64_t>(v)); }
uint64_t digest(uint64_t v) { return v; }
uint64_t digest(uint32_t v) { return v; }
uint64_t digest(uint16_t v) { return v; }
// Every value is a whole number of halves, so twice it is exact.
uint64_t digest(double v) { return static_cast<uint64_t>(static_cast<int64_t>(v * 2.0)); }

uint64_t digest(const std::string& s) {
    uint64_t h = 1469598103934665603ull;
    for (unsigned char b : s) h = mix(h, b);
    return h;
}

// A grid position, ordered by row and then by column.
struct Cell {
    int16_t x = 0;
    int16_t y = 0;

    bool operator==(const Cell& o) const { return x == o.x && y == o.y; }
    bool operator!=(const Cell& o) const { return !(*this == o); }
    bool operator<(const Cell& o) const { return y != o.y ? y < o.y : x < o.x; }
};

uint64_t digest(const Cell& c) {
    return mix(static_cast<uint64_t>(static_cast<int64_t>(c.x)), static_cast<uint64_t>(static_cast<int64_t>(c.y)));
}

// Two values ordered by the first, then the second.
template <class A, class B>
struct Pair {
    A first;
    B second;

    bool operator==(const Pair& o) const { return first == o.first && second == o.second; }
    bool operator!=(const Pair& o) const { return !(*this == o); }
    bool operator<(const Pair& o) const { return std::tie(first, second) < std::tie(o.first, o.second); }
};

template <class A, class B>
uint64_t digest(const Pair<A, B>& p) {
    return mix(digest(p.first), digest(p.second));
}

// An ordered map: an AVL tree whose nodes live in one vector, linked by their index in it,
// so a node is never allocated or freed on its own. Adding a key that is there already
// adds the two values.
template <class K, class V>
class TreeMap {
public:
    size_t size() const { return nodes_.size(); }

    // The value of `key`, or null when it is not in the map.
    const V* find(const K& key) const {
        int32_t at = root_;
        while (at >= 0) {
            const Node& node = nodes_[at];
            if (key < node.key) {
                at = node.left;
            } else if (node.key < key) {
                at = node.right;
            } else {
                return &node.value;
            }
        }
        return nullptr;
    }

    V* find(const K& key) { return const_cast<V*>(static_cast<const TreeMap*>(this)->find(key)); }

    // `value` under `key`: a new entry, or added to the value already there.
    void add(K key, V value) {
        if (V* old = find(key)) {
            *old += value;
            return;
        }
        int32_t fresh = static_cast<int32_t>(nodes_.size());
        nodes_.push_back(Node{std::move(key), std::move(value)});
        root_ = link(root_, fresh);
    }

    // A digest of every entry, in the order of the keys.
    uint64_t checksum() const {
        uint64_t sum = 0;
        std::vector<int32_t> stack;
        int32_t at = root_;
        while (at >= 0 || !stack.empty()) {
            while (at >= 0) {
                stack.push_back(at);
                at = nodes_[at].left;
            }
            at = stack.back();
            stack.pop_back();
            const Node& node = nodes_[at];
            sum = mix(mix(sum, digest(node.key)), digest(node.value));
            at = node.right;
        }
        return sum;
    }

private:
    struct Node {
        K key;
        V value;
        int32_t left = -1;
        int32_t right = -1;
        int32_t height = 1;
    };

    // Hang the node at `fresh` under the subtree at `at`, and balance it again on the way
    // back up: the subtree's new root.
    int32_t link(int32_t at, int32_t fresh) {
        if (at < 0) return fresh;
        if (nodes_[fresh].key < nodes_[at].key) {
            int32_t left = link(nodes_[at].left, fresh);
            nodes_[at].left = left;
        } else {
            int32_t right = link(nodes_[at].right, fresh);
            nodes_[at].right = right;
        }
        return balance(at);
    }

    int32_t height(int32_t at) const { return at < 0 ? 0 : nodes_[at].height; }

    void update(int32_t at) {
        int32_t left = height(nodes_[at].left);
        int32_t right = height(nodes_[at].right);
        nodes_[at].height = (left > right ? left : right) + 1;
    }

    int32_t rotate_right(int32_t at) {
        int32_t top = nodes_[at].left;
        nodes_[at].left = nodes_[top].right;
        nodes_[top].right = at;
        update(at);
        update(top);
        return top;
    }

    int32_t rotate_left(int32_t at) {
        int32_t top = nodes_[at].right;
        nodes_[at].right = nodes_[top].left;
        nodes_[top].left = at;
        update(at);
        update(top);
        return top;
    }

    int32_t balance(int32_t at) {
        update(at);
        int32_t left = nodes_[at].left;
        int32_t right = nodes_[at].right;
        int32_t lean = height(left) - height(right);
        if (lean > 1) {
            if (height(nodes_[left].left) < height(nodes_[left].right)) {
                nodes_[at].left = rotate_left(left);
            }
            return rotate_right(at);
        }
        if (lean < -1) {
            if (height(nodes_[right].right) < height(nodes_[right].left)) {
                nodes_[at].right = rotate_right(right);
            }
            return rotate_left(at);
        }
        return at;
    }

    std::vector<Node> nodes_;
    int32_t root_ = -1;
};

// Sort `items` by `<`: quicksort on the median of three, and insertion sort below 16.
template <class T>
void quicksort(std::vector<T>& items, size_t from, size_t to) {
    size_t lo = from;
    size_t hi = to;
    while (hi - lo > 16) {
        size_t last = hi - 1;
        size_t mid = lo + (hi - lo) / 2;
        if (items[mid] < items[lo]) std::swap(items[mid], items[lo]);
        if (items[last] < items[lo]) std::swap(items[last], items[lo]);
        if (items[mid] < items[last]) std::swap(items[mid], items[last]);
        // The median of the three is at `last` now, the pivot.
        size_t store = lo;
        for (size_t i = lo; i < last; ++i) {
            if (items[i] < items[last]) {
                std::swap(items[i], items[store]);
                ++store;
            }
        }
        std::swap(items[store], items[last]);
        // The smaller side by recursion, the larger by the loop, so the depth is `log n`.
        if (store - lo < hi - store) {
            quicksort(items, lo, store);
            lo = store + 1;
        } else {
            quicksort(items, store + 1, hi);
            hi = store;
        }
    }
    for (size_t i = lo + 1; i < hi; ++i) {
        for (size_t j = i; j > lo && items[j] < items[j - 1]; --j) {
            std::swap(items[j], items[j - 1]);
        }
    }
}

template <class T>
void sort(std::vector<T>& items) {
    quicksort(items, 0, items.size());
}

// Where the first element not less than `x` is in the sorted `items`.
template <class T>
size_t lower_bound(const std::vector<T>& items, const T& x) {
    size_t lo = 0;
    size_t hi = items.size();
    while (lo < hi) {
        size_t mid = lo + (hi - lo) / 2;
        if (items[mid] < x) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    return lo;
}

// How many different elements the sorted `items` holds; it aborts when they are not sorted.
template <class T>
size_t distinct(const std::vector<T>& items) {
    if (items.empty()) return 0;
    size_t count = 1;
    for (size_t i = 1; i < items.size(); ++i) {
        if (items[i] < items[i - 1]) {
            std::fputs("not sorted\n", stderr);
            std::abort();
        }
        if (items[i] != items[i - 1]) ++count;
    }
    return count;
}

// One instance of everything: `n` keys made by `key` from numbers below `n / 2` added to
// a map with values made by `value`, `n` lookups of which about half miss, then `n` more
// keys sorted, counted and searched `n / 4` times.
template <class K, class V>
uint64_t run(const char* name, uint64_t n, uint64_t seed, K (*key)(uint64_t), V (*value)(uint64_t)) {
    Rng rng(seed);
    uint64_t range = n / 2 + 1;

    TreeMap<K, V> map;
    for (uint64_t i = 0; i < n; ++i) {
        K k = key(rng.below(range));
        V v = value(rng.next());
        map.add(std::move(k), std::move(v));
    }

    uint64_t found = 0;
    for (uint64_t i = 0; i < n; ++i) {
        K k = key(rng.below(range * 2));
        if (const V* v = map.find(k)) found = mix(found, digest(*v));
    }

    std::vector<K> items;
    items.reserve(n);
    for (uint64_t i = 0; i < n; ++i) items.push_back(key(rng.below(range)));
    sort(items);
    size_t different = distinct(items);
    uint64_t ranks = 0;
    for (uint64_t i = 0; i < n / 4; ++i) {
        K k = key(rng.below(range));
        ranks += lower_bound(items, k);
    }

    uint64_t sum = mix(mix(mix(map.checksum(), found), ranks), different);
    std::printf("%s: %zu keys, %zu sorted of which %zu distinct, %llu\n", name, map.size(), items.size(),
                different, static_cast<unsigned long long>(sum));
    return sum;
}

const char* const SYLLABLES[24] = {"ka", "lo", "mi", "ra",  "ten", "sho", "vu", "en",  "dar", "is", "po", "qua",
                                   "zel", "ne", "fi", "gor", "ul",  "be",  "tha", "xo", "rin", "ma", "cu", "dei"};

// A word for `r`: its digits in base 24, a syllable each, the lowest first.
std::string word(uint64_t r) {
    std::string text;
    uint64_t rest = r;
    while (true) {
        text += SYLLABLES[rest % 24];
        rest /= 24;
        if (rest == 0) break;
    }
    return text;
}

int64_t key_i64(uint64_t r) { return static_cast<int64_t>(r) * 7 - 1000; }
uint32_t key_u32(uint64_t r) { return static_cast<uint32_t>(r * 2654435761ull); }
Cell key_cell(uint64_t r) { return Cell{static_cast<int16_t>(r % 1000), static_cast<int16_t>(r / 1000)}; }
std::string key_word(uint64_t r) { return word(r); }
Pair<int32_t, int32_t> key_pair(uint64_t r) {
    return Pair<int32_t, int32_t>{static_cast<int32_t>(r % 64), static_cast<int32_t>(r / 64)};
}
Pair<std::string, uint16_t> key_named(uint64_t r) {
    return Pair<std::string, uint16_t>{word(r % 500), static_cast<uint16_t>(r / 500)};
}

int64_t value_i64(uint64_t r) { return static_cast<int64_t>(r % 1000) - 500; }
double value_f64(uint64_t r) { return static_cast<double>(r % 1000) * 0.5; }
uint64_t value_u64(uint64_t r) { return r >> 40; }
int32_t value_i32(uint64_t) { return 1; }
uint16_t value_u16(uint64_t r) { return static_cast<uint16_t>(r % 7); }

}  // namespace

int main(int argc, char** argv) {
    uint64_t n = argc > 1 ? std::strtoull(argv[1], nullptr, 10) : 200000;

    uint64_t checksum = 0;
    checksum = mix(checksum, run<int64_t, int64_t>("i64 -> i64", n, 1, key_i64, value_i64));
    checksum = mix(checksum, run<uint32_t, double>("u32 -> f64", n, 2, key_u32, value_f64));
    checksum = mix(checksum, run<Cell, uint64_t>("Cell -> u64", n, 3, key_cell, value_u64));
    checksum = mix(checksum, run<std::string, int32_t>("String -> i32", n, 4, key_word, value_i32));
    checksum = mix(checksum, run<Pair<int32_t, int32_t>, int64_t>("Pair<i32, i32> -> i64", n, 5, key_pair, value_i64));
    checksum = mix(checksum,
                   run<Pair<std::string, uint16_t>, uint16_t>("Pair<String, u16> -> u16", n, 6, key_named, value_u16));
    std::printf("checksum %llu\n", static_cast<unsigned long long>(checksum));
    return 0;
}
