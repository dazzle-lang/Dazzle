// A tokenizer, parser and evaluator for a small expression language: the C++ half of a
// whole-program benchmark (tests/bench_programs/README.md). `src/main.dz` is the same
// program in Dazzle, and both must print the same checksum.
//
// Written as a C++ reviewer would accept it: std::string, std::string_view tokens and map
// keys, std::variant nodes owning their children through std::unique_ptr, std::visit.
// Built without exceptions, as Dazzle is, so a failure is a returned value — the small
// `Result` below, which is what a codebase built with -fno-exceptions writes in place of
// C++23's std::expected.
//
// Usage: expr [statements]
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
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

// --- errors -------------------------------------------------------------------------------

enum class Failure : uint32_t { Syntax = 1, UnknownName, DivideByZero };

struct Error {
    Failure code;
    const char* message;
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

// --- generating the program ---------------------------------------------------------------

// Weighted: division is rarer than the rest, as in the Dazzle half.
constexpr char kOperators[8] = {'+', '+', '-', '-', '*', '*', '/', '%'};

int precedence(char op) {
    return op == '+' || op == '-' ? 1 : 2;
}

void generate_expr(std::string& out, Rng& rng, int64_t index, int depth, int context) {
    if (depth == 0 || rng.below(4) == 0) {
        uint64_t pick = rng.below(1000);
        if (pick < 600 || index == 0) {
            out += std::to_string(rng.below(1000));
        } else if (pick < 998) {
            int64_t back = int64_t(rng.below(index < 50 ? uint64_t(index) : 50));
            out += 'v';
            out += std::to_string(index - 1 - back);
        } else {
            // A name no statement defines: evaluating this fails.
            out += 'u';
            out += std::to_string(rng.below(100));
        }
        return;
    }
    if (rng.below(10) == 0) {
        out += '-';
        generate_expr(out, rng, index, depth - 1, 3);
        return;
    }
    char op = kOperators[rng.below(8)];
    int p = precedence(op);
    bool wrap = p < context || rng.below(10) == 0;
    if (wrap) out += '(';
    generate_expr(out, rng, index, depth - 1, p);
    out += ' ';
    out += op;
    out += ' ';
    generate_expr(out, rng, index, depth - 1, p + 1);
    if (wrap) out += ')';
}

std::string generate(int64_t statements) {
    Rng rng(7);
    std::string text;
    for (int64_t i = 0; i < statements; ++i) {
        text += "let v";
        text += std::to_string(i);
        text += " = ";
        generate_expr(text, rng, i, 5, 0);
        text += '\n';
    }
    return text;
}

// --- tokens -------------------------------------------------------------------------------

enum class Kind : uint8_t { Number, Name, Let, Assign, Operator, Open, Close, End };

struct Token {
    Kind kind = Kind::End;
    std::string_view text;
    int64_t value = 0;
};

bool is_digit(char c) { return c >= '0' && c <= '9'; }

bool is_letter(char c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_'; }

Result<std::vector<Token>> tokenize(std::string_view text) {
    std::vector<Token> tokens;
    size_t i = 0;
    while (i < text.size()) {
        char c = text[i];
        if (c == ' ') {
            ++i;
            continue;
        }
        size_t start = i;
        if (is_digit(c)) {
            int64_t value = 0;
            while (i < text.size() && is_digit(text[i])) {
                value = value * 10 + (text[i] - '0');
                ++i;
            }
            tokens.push_back(Token{Kind::Number, text.substr(start, i - start), value});
            continue;
        }
        if (is_letter(c)) {
            while (i < text.size() && (is_letter(text[i]) || is_digit(text[i]))) {
                ++i;
            }
            std::string_view word = text.substr(start, i - start);
            tokens.push_back(Token{word == "let" ? Kind::Let : Kind::Name, word});
            continue;
        }
        ++i;
        Kind kind;
        switch (c) {
            case '\n': kind = Kind::End; break;
            case '=': kind = Kind::Assign; break;
            case '(': kind = Kind::Open; break;
            case ')': kind = Kind::Close; break;
            case '+': case '-': case '*': case '/': case '%': kind = Kind::Operator; break;
            default: return Error{Failure::Syntax, "unexpected character"};
        }
        tokens.push_back(Token{kind, text.substr(start, 1)});
    }
    return tokens;
}

// --- the tree -----------------------------------------------------------------------------

struct Expr;
using ExprPtr = std::unique_ptr<Expr>;

struct Number {
    int64_t value;
};
struct Name {
    std::string_view text;
};
struct Negate {
    ExprPtr inner;
};
struct Binary {
    char op;
    ExprPtr lhs;
    ExprPtr rhs;
};

struct Expr {
    std::variant<Number, Name, Negate, Binary> node;
};

template <class T>
ExprPtr make(T node) {
    return std::make_unique<Expr>(Expr{std::move(node)});
}

struct Statement {
    std::string_view name;
    ExprPtr value;
};

class Parser {
public:
    Parser(const Token* tokens, size_t count) : tokens_(tokens), count_(count) {}

    // `let name = expr`, then the end of the line.
    Result<Statement> statement() {
        if (!expect(Kind::Let)) return syntax("unexpected token");
        if (!expect(Kind::Name)) return syntax("unexpected token");
        std::string_view name = tokens_[at_ - 1].text;
        if (!expect(Kind::Assign)) return syntax("unexpected token");
        auto value = expr();
        if (!value) return value.error();
        if (!expect(Kind::End)) return syntax("unexpected token");
        return Statement{name, std::move(value.value())};
    }

private:
    static Error syntax(const char* message) { return Error{Failure::Syntax, message}; }

    Kind peek() const { return at_ < count_ ? tokens_[at_].kind : Kind::End; }

    char peek_operator(char a, char b, char c) const {
        if (peek() != Kind::Operator) return 0;
        char op = tokens_[at_].text[0];
        return op == a || op == b || op == c ? op : 0;
    }

    bool expect(Kind kind) {
        if (peek() != kind) return false;
        ++at_;
        return true;
    }

    Result<ExprPtr> expr() {
        auto lhs = term();
        if (!lhs) return lhs;
        ExprPtr tree = std::move(lhs.value());
        while (char op = peek_operator('+', '-', '-')) {
            ++at_;
            auto rhs = term();
            if (!rhs) return rhs;
            tree = make(Binary{op, std::move(tree), std::move(rhs.value())});
        }
        return tree;
    }

    Result<ExprPtr> term() {
        auto lhs = unary();
        if (!lhs) return lhs;
        ExprPtr tree = std::move(lhs.value());
        while (char op = peek_operator('*', '/', '%')) {
            ++at_;
            auto rhs = unary();
            if (!rhs) return rhs;
            tree = make(Binary{op, std::move(tree), std::move(rhs.value())});
        }
        return tree;
    }

    Result<ExprPtr> unary() {
        if (peek_operator('-', '-', '-')) {
            ++at_;
            auto inner = unary();
            if (!inner) return inner;
            return make(Negate{std::move(inner.value())});
        }
        return primary();
    }

    Result<ExprPtr> primary() {
        switch (peek()) {
            case Kind::Number:
                ++at_;
                return make(Number{tokens_[at_ - 1].value});
            case Kind::Name:
                ++at_;
                return make(Name{tokens_[at_ - 1].text});
            case Kind::Open: {
                ++at_;
                auto inner = expr();
                if (!inner) return inner;
                if (!expect(Kind::Close)) return syntax("unexpected token");
                return inner;
            }
            default:
                return syntax("expected a number, a name or `(`");
        }
    }

    const Token* tokens_;
    size_t count_;
    size_t at_ = 0;
};

// --- evaluating ---------------------------------------------------------------------------

// Every result is reduced by this, so no value overflows and both halves agree exactly.
constexpr int64_t kModulus = 1000003;

using Names = std::unordered_map<std::string_view, int64_t>;

Result<int64_t> eval(const Expr& e, const Names& names);

struct Evaluator {
    const Names& names;

    Result<int64_t> operator()(const Number& n) const { return n.value; }

    Result<int64_t> operator()(const Name& n) const {
        auto it = names.find(n.text);
        if (it == names.end()) return Error{Failure::UnknownName, "unknown name"};
        return it->second;
    }

    Result<int64_t> operator()(const Negate& n) const {
        auto inner = eval(*n.inner, names);
        if (!inner) return inner;
        return -inner.value();
    }

    Result<int64_t> operator()(const Binary& b) const {
        auto lhs = eval(*b.lhs, names);
        if (!lhs) return lhs;
        auto rhs = eval(*b.rhs, names);
        if (!rhs) return rhs;
        int64_t l = lhs.value();
        int64_t r = rhs.value();
        if ((b.op == '/' || b.op == '%') && r == 0) {
            return Error{Failure::DivideByZero, "division by zero"};
        }
        int64_t value;
        switch (b.op) {
            case '+': value = l + r; break;
            case '-': value = l - r; break;
            case '*': value = l * r; break;
            case '/': value = l / r; break;
            default: value = l % r; break;
        }
        return value % kModulus;
    }
};

Result<int64_t> eval(const Expr& e, const Names& names) {
    return std::visit(Evaluator{names}, e.node);
}

}  // namespace

int main(int argc, char** argv) {
    int64_t statements = argc > 1 ? std::strtoll(argv[1], nullptr, 10) : 100000;

    std::string program = generate(statements);
    auto tokenized = tokenize(program);
    if (!tokenized) {
        std::fprintf(stderr, "%s\n", tokenized.error().message);
        return 1;
    }
    const std::vector<Token>& all = tokenized.value();

    Names names;
    uint64_t checksum = 0;
    int64_t failures[4] = {};
    size_t start = 0;
    while (start < all.size()) {
        // One statement is the tokens up to and including the end of its line.
        size_t end = start;
        while (end < all.size() && all[end].kind != Kind::End) ++end;
        end = end < all.size() ? end + 1 : end;
        Parser parser(all.data() + start, end - start);
        start = end;

        auto statement = parser.statement();
        if (!statement) {
            uint32_t code = uint32_t(statement.error().code);
            failures[code] += 1;
            checksum = checksum * 31 + code;
            continue;
        }
        // A statement that fails still defines its name, as one, so one failure does not
        // make every statement that mentions it fail too.
        auto result = eval(*statement.value().value, names);
        int64_t value;
        if (result) {
            value = result.value();
        } else {
            uint32_t code = uint32_t(result.error().code);
            failures[code] += 1;
            checksum = checksum * 31 + code;
            value = 1;
        }
        names[statement.value().name] = value;
        checksum = checksum * 31 + uint64_t(value);
    }
    std::printf("%zu bytes, %zu tokens, %zu names, %lld unknown-name, %lld divide-by-zero, "
                "%lld syntax\n",
                program.size(), all.size(), names.size(), (long long)failures[2],
                (long long)failures[3], (long long)failures[1]);
    std::printf("checksum %llu\n", (unsigned long long)checksum);
    return 0;
}
