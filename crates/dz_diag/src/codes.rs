//! Stable diagnostic codes.
//!
//! Codes never change meaning once assigned, and never get reused. Tests refer to errors
//! by code rather than by message text, so message wording stays free to improve.
//!
//! | Range | Phase |
//! |-------|-------|
//! | DZ01xx | lexer |
//! | DZ02xx | parser |
//! | DZ03xx | name resolution and visibility |
//! | DZ04xx | ownership and flow analysis |
//! | DZ05xx | type checking |
//! | DZ06xx | macros |
//! | DZ07xx | C++ interop |

// --- DZ01xx: lexer -------------------------------------------------------------

/// The file is UTF-16, or otherwise not valid UTF-8 (spec §2.1).
pub const BAD_ENCODING: &str = "DZ0101";
/// A character that cannot begin any token.
pub const UNEXPECTED_CHARACTER: &str = "DZ0102";
/// A zero-width or otherwise invisible character in the source.
pub const INVISIBLE_CHARACTER: &str = "DZ0103";
/// A string literal that hits a newline or end of file before its closing quote.
pub const UNTERMINATED_STRING: &str = "DZ0104";
/// A `` ` ``-delimited raw string with no closing backtick.
pub const UNTERMINATED_RAW_STRING: &str = "DZ0105";
/// A `/*` block comment with no matching `*/`.
pub const UNTERMINATED_BLOCK_COMMENT: &str = "DZ0106";
/// An escape sequence that is not one of those listed in spec §2.5.
pub const UNKNOWN_ESCAPE: &str = "DZ0107";
/// A malformed numeric literal: bad prefix, stray letters, or more than one `.`.
pub const INVALID_NUMBER: &str = "DZ0108";
/// A digit that does not belong to the literal's radix, e.g. `0b102`.
pub const INVALID_DIGIT: &str = "DZ0109";
/// A type suffix that is not one of the integer or float suffixes.
pub const INVALID_SUFFIX: &str = "DZ0110";
/// A character literal that is empty, unterminated, or holds more than one character.
pub const INVALID_CHAR_LITERAL: &str = "DZ0111";
/// A `_` separator at the end of a numeric literal.
pub const TRAILING_UNDERSCORE: &str = "DZ0112";

// --- DZ02xx: parser ------------------------------------------------------------

/// A specific token was required here and something else was found.
pub const EXPECTED_TOKEN: &str = "DZ0201";
/// Expected the start of a declaration at file or block scope.
pub const EXPECTED_ITEM: &str = "DZ0202";
/// Expected an expression.
pub const EXPECTED_EXPR: &str = "DZ0203";
/// Expected a type.
pub const EXPECTED_TYPE: &str = "DZ0204";
/// `a < b < c` — comparison is non-associative (spec §11.1).
pub const CHAINED_COMPARISON: &str = "DZ0205";
/// A word reserved for future use was used as an identifier (spec §2.7).
pub const RESERVED_KEYWORD: &str = "DZ0206";
/// Expected a binding pattern in a `let` or `for`.
pub const EXPECTED_PATTERN: &str = "DZ0207";
/// A bracket or brace was opened and never closed.
pub const UNCLOSED_DELIMITER: &str = "DZ0208";
/// Assignment is a statement, not an expression (spec §11.1).
pub const ASSIGN_IN_EXPRESSION: &str = "DZ0209";
/// A struct literal in a position where `{` starts a block instead.
pub const AMBIGUOUS_STRUCT_LITERAL: &str = "DZ0210";
/// Two `or` clauses on one expression. `or` is non-associative (spec §9.4, §11.1).
pub const CHAINED_OR: &str = "DZ0211";

// --- DZ00xx: projects and manifests --------------------------------------------

/// A `dazzle.toml` that could not be read.
pub const MANIFEST_MISSING: &str = "DZ0001";
/// A syntax error inside a `dazzle.toml`.
pub const MANIFEST_SYNTAX: &str = "DZ0002";
/// A required manifest table or key is absent.
pub const MANIFEST_MISSING_FIELD: &str = "DZ0003";
/// A manifest key has the wrong type, or a value outside its allowed set.
pub const MANIFEST_BAD_VALUE: &str = "DZ0004";
/// A module's source patterns matched nothing.
pub const MANIFEST_NO_SOURCES: &str = "DZ0005";
/// Modules depend on one another in a cycle (spec §3.2).
pub const MODULE_CYCLE: &str = "DZ0006";

// --- DZ03xx: name resolution and visibility ------------------------------------

/// A name that does not resolve to anything.
pub const UNRESOLVED_NAME: &str = "DZ0301";
/// Two declarations with the same name in one module.
pub const DUPLICATE_DEFINITION: &str = "DZ0302";
/// An `import` naming a module that is not a dependency.
pub const UNKNOWN_MODULE: &str = "DZ0303";
/// A name that exists but is not visible from here (spec §4).
pub const PRIVATE_ITEM: &str = "DZ0304";
/// A member that does not exist on the named type or module.
pub const NO_SUCH_MEMBER: &str = "DZ0305";
/// A local binding that shadows another in the same scope (spec §7.1).
pub const SHADOWED_BINDING: &str = "DZ0306";
/// An `import` that is never used.
pub const UNUSED_IMPORT: &str = "DZ0307";
/// Two imports bringing the same name into scope.
pub const AMBIGUOUS_IMPORT: &str = "DZ0308";

// --- DZ04xx: ownership and flow analysis (spec §6) -----------------------------

/// A value read after it was moved out of (spec §6.4).
pub const USE_AFTER_MOVE: &str = "DZ0401";
/// `move` applied to a place the function does not own: behind a `&`, a `^`, or an
/// array element, where the compiler cannot reset the source to its defaults (spec §6.4).
pub const MOVE_OUT_OF_BORROW: &str = "DZ0402";
/// A move-only value used where a copy would be needed (spec §6.3).
pub const IMPLICIT_COPY: &str = "DZ0403";
/// A struct read as a whole after one of its fields was moved out (spec §6.4).
pub const USE_OF_PARTIAL_MOVE: &str = "DZ0404";
/// A function with a return type whose body can reach the end without returning.
pub const MISSING_RETURN: &str = "DZ0405";
/// A second `copy` or `drop` on one struct (spec §6.1).
pub const DUPLICATE_SPECIAL_MEMBER: &str = "DZ0411";
/// A struct with an `own` field and no `drop` (spec §6.5, §18.4).
pub const OWN_FIELD_WITHOUT_DROP: &str = "DZ0412";

// --- DZ05xx: type checking -----------------------------------------------------

/// A value of one type used where another was required.
pub const TYPE_MISMATCH: &str = "DZ0501";
/// A literal that does not fit the type the context gives it (spec §2.5).
pub const LITERAL_OUT_OF_RANGE: &str = "DZ0502";
/// A call on something that is not a function.
pub const NOT_CALLABLE: &str = "DZ0503";
/// Too many or too few arguments.
pub const WRONG_ARG_COUNT: &str = "DZ0504";
/// A field that does not exist on the receiver's type.
pub const NO_SUCH_FIELD: &str = "DZ0505";
/// A field or method that exists but is not visible here (spec §4).
pub const PRIVATE_FIELD: &str = "DZ0506";
/// An assignment to, or `&mut` of, something immutable.
pub const NOT_MUTABLE: &str = "DZ0507";
/// A condition that is not `bool` — there is no truthiness (spec §10.1).
pub const CONDITION_NOT_BOOL: &str = "DZ0508";
/// A `!T` whose result is neither handled nor propagated (spec §9.7).
pub const UNUSED_RESULT: &str = "DZ0510";
/// An `as` conversion between types that cannot convert.
pub const INVALID_CAST: &str = "DZ0511";
/// A struct literal missing a field that has no default.
pub const MISSING_FIELD: &str = "DZ0512";
/// A `switch` that does not cover every variant and has no `default` (spec §10.3).
pub const NON_EXHAUSTIVE_SWITCH: &str = "DZ0513";
/// An operator applied to operands it does not accept.
pub const INVALID_OPERANDS: &str = "DZ0514";
/// Indexing something that is not an array or slice.
pub const NOT_INDEXABLE: &str = "DZ0515";
/// An `or return` whose failure has nowhere to go: the enclosing signature cannot carry
/// it (spec §9.5). Named for `try` in v0.4, which is what this code originally reported.
pub const TRY_OUTSIDE_FALLIBLE: &str = "DZ0516";
/// A `&T` stored in a struct field (spec §5.2, §18.3).
pub const REF_IN_FIELD: &str = "DZ0517";
/// `^` applied to something that is not a pointer.
pub const NOT_DEREFERENCEABLE: &str = "DZ0518";
/// The same field declared or initialised twice.
pub const DUPLICATE_FIELD: &str = "DZ0519";
/// A method that does not exist on the receiver's type.
pub const NO_SUCH_METHOD: &str = "DZ0520";
/// `none` or `error(...)` where no optional or fallible type is expected (spec §9.1).
pub const NO_CONTEXT_FOR_LITERAL: &str = "DZ0521";
/// A `^T` used where a `&T` is required. The conversion is not implicit, because a
/// pointer may be null and a reference may not — it goes through an `or` clause (§5.2).
pub const POINTER_NEEDS_CHECK: &str = "DZ0522";
/// An `or` clause that neither produces the success type nor diverges (spec §9.4).
pub const OR_CLAUSE_TYPE: &str = "DZ0523";
/// `or ignore` in expression position: it produces no value to bind (spec §9.4).
pub const IGNORE_PRODUCES_NO_VALUE: &str = "DZ0524";
/// An `or` clause on something that cannot fail (spec §9.4).
pub const NOT_FALLIBLE: &str = "DZ0525";

// --- DZ08xx: code generation ---------------------------------------------------

/// A construct the code generator does not handle yet.
pub const CODEGEN_UNSUPPORTED: &str = "DZ0801";
/// The project has no `main`, or it has the wrong shape.
pub const NO_ENTRY_POINT: &str = "DZ0802";
/// The C++ compiler could not be found, or failed on the generated code.
pub const CPP_COMPILER_FAILED: &str = "DZ0803";
