// JSON: the C++ half of a whole-program benchmark (tests/bench_programs/README.md).
// `src/main.dz` is the same program in Dazzle, and both must print the same checksum.
//
// Written as a C++ reviewer would accept it: a tree of std::variant with std::vector and
// std::string, a parser over std::string_view returning a small Result (the program is built
// with -fno-exceptions), std::from_chars and std::to_chars for numbers, and from_json and
// to_json written by hand for the record, since C++17 has no reflection.
//
// Usage: json [records]
#include <charconv>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
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

constexpr const char* kWords[16] = {"amber", "basalt", "cedar", "dune",  "ember",  "fjord",
                                    "garnet", "harbor", "iris", "jade", "kelp",   "lumen",
                                    "moss",  "nectar", "onyx", "pine"};

struct Vec3 {
    float x = 0, y = 0, z = 0;
};

struct Record {
    uint64_t id = 0;
    std::string name;
    std::vector<std::string> tags;
    Vec3 pos;
    bool active = false;
    double score = 0;
    bool has_score = false;
};

// --- errors -------------------------------------------------------------------------------

enum class Failure : uint32_t { Syntax = 1, Shape };

struct Error {
    Failure code;
    const char* message;
    size_t at;
};

template <class T>
class Result {
public:
    Result(T value) : v_(std::move(value)) {}
    Result(Error e) : v_(e) {}

    explicit operator bool() const { return v_.index() == 0; }
    T& value() { return *std::get_if<0>(&v_); }
    const Error& error() const { return *std::get_if<1>(&v_); }

private:
    std::variant<T, Error> v_;
};

// --- the input ----------------------------------------------------------------------------

void append_int(std::string& out, uint64_t v) {
    char buf[24];
    auto r = std::to_chars(buf, buf + sizeof buf, v);
    out.append(buf, r.ptr);
}

// `v` / 10^`places`, with exactly `places` digits after the point.
void write_fixed(std::string& out, int64_t v, int places) {
    int64_t scale = 1;
    for (int i = 0; i < places; ++i) scale *= 10;
    if (v < 0) out += '-';
    int64_t a = v < 0 ? -v : v;
    append_int(out, uint64_t(a / scale));
    out += '.';
    int64_t frac = a % scale;
    for (int64_t d = scale / 10; d > 0; d /= 10) out += char('0' + frac / d % 10);
}

std::string generate(int64_t count) {
    Rng rng(2718);
    std::string out = "[\n";
    for (int64_t i = 0; i < count; ++i) {
        if (i > 0) out += ",\n";
        out += "  {\"name\": \"";
        uint64_t words = 1 + rng.below(3);
        for (uint64_t w = 0; w < words; ++w) {
            if (w > 0) out += ' ';
            out += kWords[rng.below(16)];
        }
        if (rng.below(16) == 0) out += " \\\"the\\\\second\\\"";
        out += "\", \"id\": ";
        append_int(out, uint64_t(i) * 7 + 3);
        out += ", \"pos\": {\"x\": ";
        write_fixed(out, int64_t(rng.below(200001)) - 100000, 2);
        out += ", \"y\": ";
        write_fixed(out, int64_t(rng.below(200001)) - 100000, 2);
        out += ", \"z\": ";
        write_fixed(out, int64_t(rng.below(200001)) - 100000, 2);
        out += "}, \"tags\": [";
        uint64_t tags = rng.below(5);
        for (uint64_t t = 0; t < tags; ++t) {
            if (t > 0) out += ", ";
            out += '"';
            out += kWords[rng.below(16)];
            out += '"';
        }
        out += "], \"score\": ";
        if (rng.below(4) == 0) {
            out += "null";
        } else {
            write_fixed(out, int64_t(rng.below(100001)), 3);
        }
        out += ", \"is_active\": ";
        out += rng.below(2) == 0 ? "true" : "false";
        if (rng.below(3) == 0) {
            out += ", \"meta\": {\"seen\": [";
            uint64_t seen = 1 + rng.below(3);
            for (uint64_t s = 0; s < seen; ++s) {
                if (s > 0) out += ", ";
                append_int(out, rng.below(1000));
            }
            out += "], \"note\": \"";
            out += kWords[rng.below(16)];
            out += "\"}";
        }
        out += '}';
    }
    out += "\n]\n";
    return out;
}

// --- the tree, and the parser -------------------------------------------------------------

struct Member;

struct Json {
    std::variant<std::nullptr_t, bool, double, std::string, std::vector<Json>, std::vector<Member>> v;
};

struct Member {
    std::string key;
    Json value;
};

void append_utf8(std::string& out, uint32_t c) {
    if (c < 0x80) {
        out += char(c);
    } else if (c < 0x800) {
        out += char(0xC0 | (c >> 6));
        out += char(0x80 | (c & 0x3F));
    } else {
        out += char(0xE0 | (c >> 12));
        out += char(0x80 | ((c >> 6) & 0x3F));
        out += char(0x80 | (c & 0x3F));
    }
}

class Parser {
public:
    explicit Parser(std::string_view text) : text_(text) {}

    Result<Json> value() {
        skip_space();
        if (at_ >= text_.size()) return Error{Failure::Syntax, "unexpected end of the text", at_};
        char c = text_[at_];
        if (c == '{') return object();
        if (c == '[') return array();
        if (c == '"') {
            Result<std::string> s = string();
            if (!s) return s.error();
            return Json{std::move(s.value())};
        }
        if (c == 't') return word("true") ? Result<Json>(Json{true}) : Result<Json>(error("expected `true`"));
        if (c == 'f') return word("false") ? Result<Json>(Json{false}) : Result<Json>(error("expected `false`"));
        if (c == 'n') return word("null") ? Result<Json>(Json{nullptr}) : Result<Json>(error("expected `null`"));
        return number();
    }

private:
    Error error(const char* message) const { return Error{Failure::Syntax, message, at_}; }

    void skip_space() {
        while (at_ < text_.size()) {
            char c = text_[at_];
            if (c != ' ' && c != '\n' && c != '\r' && c != '\t') return;
            ++at_;
        }
    }

    bool expect(char c) {
        skip_space();
        if (at_ >= text_.size() || text_[at_] != c) return false;
        ++at_;
        return true;
    }

    bool word(std::string_view w) {
        if (text_.substr(at_, w.size()) != w) return false;
        at_ += w.size();
        return true;
    }

    Result<Json> number() {
        size_t start = at_;
        while (at_ < text_.size()) {
            char c = text_[at_];
            bool digit = c >= '0' && c <= '9';
            if (!digit && c != '-' && c != '+' && c != '.' && c != 'e' && c != 'E') break;
            ++at_;
        }
        double v = 0;
        auto r = std::from_chars(text_.data() + start, text_.data() + at_, v);
        if (r.ec != std::errc() || r.ptr != text_.data() + at_) {
            return Error{Failure::Syntax, "not a number", start};
        }
        return Json{v};
    }

    // A string's text, its escapes undone. The opening quote is next.
    Result<std::string> string() {
        ++at_;
        std::string out;
        size_t run = at_;
        while (at_ < text_.size()) {
            char c = text_[at_];
            if (c == '"') {
                out.append(text_.substr(run, at_ - run));
                ++at_;
                return out;
            }
            if (c != '\\') {
                ++at_;
                continue;
            }
            out.append(text_.substr(run, at_ - run));
            if (at_ + 1 >= text_.size()) break;
            char e = text_[at_ + 1];
            at_ += 2;
            switch (e) {
                case '"':
                case '\\':
                case '/': out += e; break;
                case 'n': out += '\n'; break;
                case 't': out += '\t'; break;
                case 'r': out += '\r'; break;
                case 'b': out += '\b'; break;
                case 'f': out += '\f'; break;
                case 'u': {
                    if (at_ + 4 > text_.size()) return error("a short \\u escape");
                    uint32_t code = 0;
                    for (size_t i = 0; i < 4; ++i) {
                        char h = text_[at_ + i];
                        uint32_t d;
                        if (h >= '0' && h <= '9') {
                            d = uint32_t(h - '0');
                        } else if (h >= 'a' && h <= 'f') {
                            d = uint32_t(h - 'a' + 10);
                        } else if (h >= 'A' && h <= 'F') {
                            d = uint32_t(h - 'A' + 10);
                        } else {
                            return error("not a hex digit");
                        }
                        code = code * 16 + d;
                    }
                    at_ += 4;
                    append_utf8(out, code);
                    break;
                }
                default: return error("an unknown escape");
            }
            run = at_;
        }
        return error("a string with no end");
    }

    Result<Json> array() {
        ++at_;
        std::vector<Json> items;
        skip_space();
        if (at_ < text_.size() && text_[at_] == ']') {
            ++at_;
            return Json{std::move(items)};
        }
        bool more = true;
        while (more) {
            Result<Json> v = value();
            if (!v) return v;
            items.push_back(std::move(v.value()));
            skip_space();
            more = at_ < text_.size() && text_[at_] == ',';
            if (more) ++at_;
        }
        if (!expect(']')) return error("expected `]`");
        return Json{std::move(items)};
    }

    Result<Json> object() {
        ++at_;
        std::vector<Member> members;
        skip_space();
        if (at_ < text_.size() && text_[at_] == '}') {
            ++at_;
            return Json{std::move(members)};
        }
        bool more = true;
        while (more) {
            skip_space();
            if (at_ >= text_.size() || text_[at_] != '"') return error("expected a key");
            Result<std::string> key = string();
            if (!key) return key.error();
            if (!expect(':')) return error("expected `:`");
            Result<Json> v = value();
            if (!v) return v;
            members.push_back(Member{std::move(key.value()), std::move(v.value())});
            skip_space();
            more = at_ < text_.size() && text_[at_] == ',';
            if (more) ++at_;
        }
        if (!expect('}')) return error("expected `}`");
        return Json{std::move(members)};
    }

    std::string_view text_;
    size_t at_ = 0;
};

// --- the tree to records, by hand ---------------------------------------------------------

Error shape(const char* message) { return Error{Failure::Shape, message, 0}; }

Result<Record> from_json(const std::vector<Member>& members) {
    Record r;
    for (const Member& m : members) {
        const std::string& key = m.key;
        if (key == "id") {
            const double* n = std::get_if<double>(&m.value.v);
            if (!n) return shape("`id` is not a number");
            r.id = uint64_t(*n);
        } else if (key == "name") {
            const std::string* s = std::get_if<std::string>(&m.value.v);
            if (!s) return shape("`name` is not a string");
            r.name = *s;
        } else if (key == "tags") {
            const auto* a = std::get_if<std::vector<Json>>(&m.value.v);
            if (!a) return shape("`tags` is not an array");
            for (const Json& t : *a) {
                const std::string* s = std::get_if<std::string>(&t.v);
                if (!s) return shape("a tag is not a string");
                r.tags.push_back(*s);
            }
        } else if (key == "pos") {
            const auto* o = std::get_if<std::vector<Member>>(&m.value.v);
            if (!o) return shape("`pos` is not an object");
            for (const Member& c : *o) {
                const double* n = std::get_if<double>(&c.value.v);
                if (!n) return shape("an axis is not a number");
                float v = float(*n);
                if (c.key == "x") {
                    r.pos.x = v;
                } else if (c.key == "y") {
                    r.pos.y = v;
                } else if (c.key == "z") {
                    r.pos.z = v;
                }
            }
        } else if (key == "is_active") {
            const bool* b = std::get_if<bool>(&m.value.v);
            if (!b) return shape("`is_active` is not a bool");
            r.active = *b;
        } else if (key == "score") {
            if (std::holds_alternative<std::nullptr_t>(m.value.v)) {
                r.has_score = false;
            } else {
                const double* n = std::get_if<double>(&m.value.v);
                if (!n) return shape("`score` is not a number");
                r.score = *n;
                r.has_score = true;
            }
        }
    }
    return r;
}

Result<std::vector<Record>> from_json(const Json& tree) {
    const auto* items = std::get_if<std::vector<Json>>(&tree.v);
    if (!items) return shape("the document is not an array");
    std::vector<Record> records;
    for (const Json& item : *items) {
        const auto* o = std::get_if<std::vector<Member>>(&item.v);
        if (!o) return shape("a record is not an object");
        Result<Record> r = from_json(*o);
        if (!r) return r.error();
        records.push_back(std::move(r.value()));
    }
    return records;
}

// --- records to JSON, by hand -------------------------------------------------------------

constexpr char kHex[] = "0123456789abcdef";

void write_text(std::string& out, std::string_view s) {
    out += '"';
    size_t run = 0;
    for (size_t i = 0; i < s.size(); ++i) {
        unsigned char c = (unsigned char)s[i];
        if (c != '"' && c != '\\' && c >= 0x20) continue;
        out.append(s.substr(run, i - run));
        if (c == '"') {
            out += "\\\"";
        } else if (c == '\\') {
            out += "\\\\";
        } else if (c == '\n') {
            out += "\\n";
        } else {
            out += "\\u00";
            out += kHex[c >> 4];
            out += kHex[c & 15];
        }
        run = i + 1;
    }
    out.append(s.substr(run));
    out += '"';
}

template <class F>
void write_number(std::string& out, F v) {
    char buf[32];
    auto r = std::to_chars(buf, buf + sizeof buf, v);
    out.append(buf, r.ptr);
}

void to_json(std::string& out, const Record& r) {
    out += "{\"id\":";
    write_number(out, r.id);
    out += ",\"name\":";
    write_text(out, r.name);
    out += ",\"tags\":[";
    for (size_t i = 0; i < r.tags.size(); ++i) {
        if (i > 0) out += ',';
        write_text(out, r.tags[i]);
    }
    out += "],\"pos\":{\"x\":";
    write_number(out, r.pos.x);
    out += ",\"y\":";
    write_number(out, r.pos.y);
    out += ",\"z\":";
    write_number(out, r.pos.z);
    out += "},\"is_active\":";
    out += r.active ? "true" : "false";
    out += ",\"score\":";
    if (r.has_score) {
        write_number(out, r.score);
    } else {
        out += "null";
    }
    out += '}';
}

std::string to_json(const std::vector<Record>& records) {
    std::string out = "[";
    for (size_t i = 0; i < records.size(); ++i) {
        if (i > 0) out += ',';
        to_json(out, records[i]);
    }
    out += ']';
    return out;
}

}  // namespace

int main(int argc, char** argv) {
    int64_t count = argc > 1 ? std::strtoll(argv[1], nullptr, 10) : 100000;

    const std::string text = generate(count);
    Parser parser(text);
    Result<Json> tree = parser.value();
    if (!tree) {
        std::fprintf(stderr, "%s at byte %zu\n", tree.error().message, tree.error().at);
        return 1;
    }
    Result<std::vector<Record>> records = from_json(tree.value());
    if (!records) {
        std::fprintf(stderr, "%s\n", records.error().message);
        return 1;
    }
    const std::string out = to_json(records.value());

    // FNV-1a over every byte written.
    uint64_t checksum = 1469598103934665603ull;
    for (char c : out) checksum = (checksum ^ uint64_t(uint8_t(c))) * 1099511628211ull;
    std::printf("%zu bytes read, %zu records, %zu bytes written\n", text.size(), records.value().size(),
                out.size());
    std::printf("checksum %llu\n", (unsigned long long)checksum);
    return 0;
}
