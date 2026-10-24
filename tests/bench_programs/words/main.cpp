// Word frequencies over a generated text: the C++ half of a whole-program benchmark
// (tests/bench_programs/README.md). `src/main.dz` is the same program in Dazzle, and both
// must print the same checksum.
//
// Written as a C++ reviewer would accept it: std::string, a reused buffer for the current
// word, std::unordered_map<std::string, int>, a std::vector of pairs, std::sort.
//
// Usage: words [words]
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <string_view>
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

constexpr std::string_view kSyllables[24] = {
    "ka", "lo", "mi", "ra", "ten", "sho", "vu", "en", "dar", "is", "po", "qua",
    "zel", "ne", "fi", "gor", "ul", "be", "tha", "xo", "rin", "ma", "cu", "dei",
};

// `words` words of one to four syllables, favouring the first few syllables, as the Dazzle
// half does.
std::string generate(int64_t words) {
    Rng rng(1234);
    std::string text;
    for (int64_t i = 0; i < words; ++i) {
        uint64_t syllables = 1 + rng.below(4);
        bool capital = rng.below(8) == 0;
        for (uint64_t s = 0; s < syllables; ++s) {
            // Two statements, not one expression: C++ leaves the order of the two calls
            // unspecified, and the halves have to draw the same numbers.
            uint64_t bound = rng.below(24) + 1;
            std::string_view syllable = kSyllables[rng.below(bound)];
            if (s == 0 && capital) {
                text += char(syllable[0] - 32);
                text += syllable.substr(1);
            } else {
                text += syllable;
            }
        }
        uint64_t mark = rng.below(20);
        if (mark == 0) {
            text += ',';
        } else if (mark == 1) {
            text += '.';
        }
        text += i % 12 == 11 ? '\n' : ' ';
    }
    return text;
}

bool is_space(char c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

}  // namespace

int main(int argc, char** argv) {
    int64_t total = argc > 1 ? std::strtoll(argv[1], nullptr, 10) : 2000000;

    const std::string text = generate(total);

    std::unordered_map<std::string, int32_t> counts;
    std::string word;
    size_t i = 0;
    while (i < text.size()) {
        while (i < text.size() && is_space(text[i])) ++i;
        word.clear();
        while (i < text.size() && !is_space(text[i])) {
            char c = text[i++];
            if (c >= 'a' && c <= 'z') {
                word += c;
            } else if (c >= 'A' && c <= 'Z') {
                word += char(c + 32);
            }
        }
        if (!word.empty()) {
            ++counts[word];
        }
    }

    std::vector<std::pair<std::string, int32_t>> entries(counts.begin(), counts.end());
    // More often first, and alphabetically among equals: a total order, since every word is
    // distinct, so both halves sort into exactly the same list.
    std::sort(entries.begin(), entries.end(), [](const auto& a, const auto& b) {
        if (a.second != b.second) return a.second > b.second;
        return a.first < b.first;
    });

    uint64_t checksum = 1469598103934665603ull;
    for (const auto& [w, n] : entries) {
        for (char c : w) {
            checksum = (checksum ^ uint64_t(uint8_t(c))) * 1099511628211ull;
        }
        checksum = (checksum ^ uint64_t(int64_t(n))) * 1099511628211ull;
    }
    std::printf("%zu bytes, %lld words, %zu distinct\n", text.size(), (long long)total,
                entries.size());
    for (size_t k = 0; k < 3 && k < entries.size(); ++k) {
        std::printf("  %s %d\n", entries[k].first.c_str(), entries[k].second);
    }
    std::printf("checksum %llu\n", (unsigned long long)checksum);
    return 0;
}
