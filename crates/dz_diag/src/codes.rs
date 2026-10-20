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
/// `break` followed by a value. A loop is a statement and has no result to give, so the
/// language has no such form (spec §10.2, §18.28).
pub const BREAK_WITH_VALUE: &str = "DZ0212";
/// `drop()` or `copy(other: &Self)` written as a member. They are implementations of the
/// `Drop` and `Copy` traits (spec §6.1).
pub const SPECIAL_MEMBER_IS_A_TRAIT: &str = "DZ0213";
/// `mut fn`, removed in r15: the receiver alone says whether a method mutates, so
/// `fn f(&mut self)` is the one spelling (spec §7.3, §18.34).
pub const MUT_FN_REMOVED: &str = "DZ0214";
/// `name!(…)`, removed in r62: a macro is invoked `#name(…)` in every position (spec
/// §12.5, §18.9).
pub const BANG_MACRO_REMOVED: &str = "DZ0215";
/// Something in a union's body that is neither a variant nor a method (spec §7.5).
pub const BAD_UNION_MEMBER: &str = "DZ0216";

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
/// A build flag in an `#if`, a dependency's `flags = [...]` or a `--flag` that no package
/// registers: a flag is declared under `[flags]` or built in (spec §12.10).
pub const UNKNOWN_FLAG: &str = "DZ0309";
/// An `import` that is never used.
pub const UNUSED_IMPORT: &str = "DZ0307";
/// Two imports bringing the same name into scope.
pub const AMBIGUOUS_IMPORT: &str = "DZ0308";
/// A bare name that more than one opened dependency exports (spec §3.2).
pub const AMBIGUOUS_NAME: &str = "DZ0310";
/// A `pub` declaration whose signature names a type from a private dependency (spec §3.2).
pub const PRIVATE_DEPENDENCY_IN_PUBLIC: &str = "DZ0311";
/// `import m` with no alias or list, or `import cpp`: what dazzle.toml now declares
/// (spec §3.2, §15.1).
pub const IMPORT_FORM_REMOVED: &str = "DZ0312";
/// A `[dependencies]` entry nothing in the module names (spec §3.2).
pub const UNUSED_DEPENDENCY: &str = "DZ0313";
/// An attribute the language does not have (spec §12.8).
pub const UNKNOWN_ATTRIBUTE: &str = "DZ0314";
/// A known attribute on a declaration it does not apply to, with arguments it does not take,
/// or written twice (spec §12.8).
pub const MISPLACED_ATTRIBUTE: &str = "DZ0315";
/// An attribute the specification describes that the compiler does not implement yet
/// (spec §12.8).
pub const ATTRIBUTE_NOT_IMPLEMENTED: &str = "DZ0316";
/// An `extern fn` outside the standard library, or inside a body: its body is C++ only the
/// compiler supplies (spec §7.2, 18.65).
pub const EXTERN_OUTSIDE_STD: &str = "DZ0317";

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
/// A warning: a local or parameter nothing reads. A leading `_` in the name says it is
/// unused on purpose.
pub const UNUSED_VARIABLE: &str = "DZ0406";
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
/// A formatting call whose format string does not fit its arguments: not a string literal,
/// a `{}` count that differs from the argument count, an unknown `{:spec}` or one that does
/// not suit its argument, an unbalanced brace, or a `{name}` (spec §12.9).
pub const BAD_FORMAT_STRING: &str = "DZ0526";
/// A formatting argument whose type has no way to be written (spec §12.9).
pub const NOT_FORMATTABLE: &str = "DZ0527";
/// A raw-memory operation outside `unsafe`: reading or writing through a `^T`, indexing
/// one, a pointer cast, `construct`/`destroy`/`memcpy`/`free`, or a call to an `unsafe fn`
/// (spec §5.2). Forming, copying, storing and comparing a pointer are all safe.
pub const UNSAFE_OPERATION: &str = "DZ0528";
/// `if let` on a fallible call. Its `else` has no `Error` to hand over, so the failure
/// would be silently dropped; that is `or`'s job (spec §9.4, §9.5).
pub const IF_LET_FALLIBLE: &str = "DZ0529";

// --- DZ053x: generics and traits (spec §8, §7.6) -------------------------------
//
// Still type checking, so still DZ05xx; a block of their own only so they are easy to
// find. DZ06xx is spoken for by macros.

/// A generic bound, or a `struct S : X`, naming something that is not a trait (§8.1).
pub const NOT_A_TRAIT: &str = "DZ0530";
/// Something done with a generic parameter that its bounds do not permit (spec §8.1).
/// The whole point of definition-site generics: this is reported inside the generic body,
/// at the offending line, and never at an instantiation (§8.6, principle 4).
pub const UNSATISFIED_BOUND: &str = "DZ0531";
/// A type declaring `: Trait` that does not implement one of the trait's methods (§7.6).
pub const MISSING_TRAIT_MEMBER: &str = "DZ0532";
/// A trait method implemented with a signature that does not match the trait's (§7.6).
pub const TRAIT_MEMBER_MISMATCH: &str = "DZ0533";
/// A type argument list of the wrong length, or a type argument that cannot be inferred
/// (spec §8.1).
pub const WRONG_TYPE_ARG_COUNT: &str = "DZ0534";
// DZ0535 was a duplicate generic parameter. Name resolution binds generic parameters
// through the same path as every other binding, so `<T, T>` is DZ0306 and this code was
// never reachable. Codes are not reused, so the number is retired rather than reassigned.
/// Monomorphization that does not terminate, bounded by the compiler rather than left to
/// C++'s template depth limit (spec §8.4).
pub const RECURSIVE_INSTANTIATION: &str = "DZ0536";
/// Two methods with one name that are not implementations of one trait at different
/// arguments — the only overloading there is (spec §7.6, §18.5, §18.27).
pub const DUPLICATE_METHOD: &str = "DZ0537";
/// A call that more than one of a type's same-named methods would accept (spec §18.27).
pub const AMBIGUOUS_CALL: &str = "DZ0538";
/// An `impl` written where it may not be: outside both the trait's module and the type's
/// (the orphan rule), for every type at once, or a `Drop`/`Copy` that is not its struct's
/// own (spec §6.1, §7.6).
pub const ORPHAN_IMPL: &str = "DZ0539";
/// Two implementations of one trait that could both apply to the same type (spec §7.6).
pub const OVERLAPPING_IMPL: &str = "DZ0540";
/// `struct Button : UiElement` where `UiElement` cannot be a base: not a struct, itself,
/// an imported type, a generic one, or a chain that comes back round (spec §7.3, 18.32).
pub const BAD_BASE: &str = "DZ0541";
/// A base's field or method redeclared in the derived type. Dazzle has no hiding: a name
/// means one thing in a type, whichever of its declarations it came from (spec §7.3).
pub const SHADOWS_BASE_MEMBER: &str = "DZ0542";
/// A constructor that leaves out `: Base(…)` where the base has a constructor of its own,
/// or a struct literal of a type that has one (spec §7.3, §7.2, 18.32).
pub const MISSING_BASE_INIT: &str = "DZ0543";
/// An `override` with nothing to override, one whose signature has drifted from the
/// method it replaces, or one aimed at a base method that is not `virtual` (spec §7.3).
pub const BAD_OVERRIDE: &str = "DZ0544";
/// A value of a type with a `virtual` method that has no body. The body is missing, so
/// there is nothing to call: only a derived type that supplies one can exist (spec §7.3).
pub const ABSTRACT_TYPE: &str = "DZ0545";
/// A `#[test]` on something that cannot be run as one: a function that takes an argument,
/// returns a value, is fallible, is generic, or has no body (spec §19 M14).
pub const BAD_TEST: &str = "DZ0546";
/// A `case` label that is not a constant, or not the kind of label the subject takes: a
/// value on an enum or union, a payload binding on an integer, a `switch` on a float
/// (spec §10.3).
pub const BAD_CASE_LABEL: &str = "DZ0547";
/// The same variant or constant named by two `case` labels of one `switch` (spec §10.3).
pub const DUPLICATE_CASE: &str = "DZ0548";
/// A `for` or `case` binding that does not fit what it names: `&e` over values nothing can
/// refer to, a count of names that is not the element tuple's size, `&` on the pass count
/// of `indexed()` (spec §10.4).
pub const BAD_BINDING: &str = "DZ0549";
/// `break` or `continue` outside any loop, or naming a label no enclosing loop has
/// (spec §10.2).
pub const JUMP_OUTSIDE_LOOP: &str = "DZ0550";
/// A bare union variant that names no struct to carry: nothing of that name is in scope, or
/// it is not a struct, or it is a generic one with nowhere to write the arguments. A variant
/// with no payload is written `Name {}` (spec §7.5, 18.45).
pub const BAD_VARIANT: &str = "DZ0551";
/// A `return`, or `or return`, inside a constructor's `: Base(…)`. The base is built before
/// the constructor begins, so there is nothing to return from (spec §7.3).
pub const RETURN_IN_BASE_INIT: &str = "DZ0552";
/// A `static` whose initialiser runs code: a call, a read of another static, anything but
/// constants and literals built of them. It is built before `main`, in an order C++ does not
/// promise across files (spec §7.7).
pub const STATIC_INITIALISER: &str = "DZ0553";
/// A variadic parameter `xs: ..T` that is not last, has a default, or is written where a
/// function cannot take one; or a spread `..xs` that is not the only argument for it, or
/// not a slice (spec §7.2).
pub const BAD_VARIADIC: &str = "DZ0554";
/// A reflection call in a build whose `type_info` is `"none"` (spec §13.2).
pub const REFLECTION_DISABLED: &str = "DZ0555";
/// A function with no body that is not a trait's required method, a body-less `virtual`
/// method, or an `extern fn` of the standard library; or an `extern fn` that
/// has a body, or is generic or fallible (spec §7.2).
pub const MISSING_BODY: &str = "DZ0556";
/// A method with type parameters of its own where one cannot be instantiated per call: a
/// `virtual` or `override` method, whose one slot in the vtable cannot be many functions, or
/// a trait's method, which a trait object would have to call without knowing them
/// (spec §8.1, §7.6).
pub const GENERIC_METHOD_NOT_ALLOWED: &str = "DZ0557";
/// A union method named `kind`, which every union already has (spec §7.5).
pub const RESERVED_UNION_METHOD: &str = "DZ0558";

// --- DZ06xx: macros (spec §12) -------------------------------------------------

/// `#name` names no macro in scope (spec §12.5).
pub const UNKNOWN_MACRO: &str = "DZ0601";
/// An invocation that does not fit the macro's pattern (spec §12.3, §12.6).
pub const MACRO_NO_MATCH: &str = "DZ0602";
/// A macro invoked where its kind cannot go: a `decl` macro in an expression, say
/// (spec §12.5).
pub const MACRO_WRONG_POSITION: &str = "DZ0603";
/// `#error(…)` in a macro's `expand` (spec §12.6).
pub const MACRO_ERROR: &str = "DZ0604";
/// Expansions invoking macros more than 64 deep (spec §12.5).
pub const MACRO_TOO_DEEP: &str = "DZ0605";
/// A malformed macro declaration: an unknown kind, an unknown `syntax` rule, a `$x` that
/// nothing captured, a `#for` over what is not a repetition (spec §12.1–§12.3).
pub const BAD_MACRO: &str = "DZ0606";
/// A macro named like an intrinsic, or `error` (spec §12.8).
pub const RESERVED_MACRO_NAME: &str = "DZ0607";
/// An expansion that declares a macro (spec §12.5).
pub const MACRO_IN_EXPANSION: &str = "DZ0608";
/// Two macros of one name in one module.
pub const DUPLICATE_MACRO: &str = "DZ0609";

// --- DZ07xx: C and C++ interop (spec §15) --------------------------------------

/// A library vendored as source failed to compile. Neither a Dazzle bug nor a mistake in
/// Dazzle code: the library's own source is wrong, or its recipe in `dazzle.toml` is
/// missing a define or an include dir (spec §15.1).
pub const CPP_LIBRARY_FAILED: &str = "DZ0701";
/// An `import cpp` header that clang could not read: not found through the include dirs,
/// or with errors of its own (spec §15.1).
pub const CPP_HEADER_FAILED: &str = "DZ0702";
/// `import cpp` on a machine where libclang could not be loaded (spec §15.1).
pub const CPP_UNAVAILABLE: &str = "DZ0703";
/// An imported type whose header only declares it — a handle — used where a value of it
/// would have to exist. Nothing here knows its size (spec §15.2, §18.40).
pub const OPAQUE_BY_VALUE: &str = "DZ0704";
/// A literal of an imported C `union` that sets more than one member. They are the same
/// memory, so all but the last would be overwritten (spec §15.2, §18.41).
pub const UNION_LITERAL: &str = "DZ0705";
/// `&` or `&mut` on an imported C bit-field. It is a few bits inside a larger member, and
/// C++ has no pointer or reference to one (spec §15.2).
pub const BITFIELD_ADDRESS: &str = "DZ0706";
/// An imported C array member taken as a value. C++ cannot copy or assign a raw array, so
/// it is indexed, iterated or pointed at instead (spec §15.2).
pub const C_ARRAY_BY_VALUE: &str = "DZ0707";

// --- DZ08xx: code generation ---------------------------------------------------

/// A construct the code generator does not handle yet.
pub const CODEGEN_UNSUPPORTED: &str = "DZ0801";
/// The project has no `main`, or it has the wrong shape.
pub const NO_ENTRY_POINT: &str = "DZ0802";
/// The C++ compiler could not be found, or failed on the generated code.
pub const CPP_COMPILER_FAILED: &str = "DZ0803";
/// A compiler bug: a later phase met something an earlier phase should have reported or
/// never produced — an error node with no diagnostic behind it, say. Always our fault, and
/// said so, rather than surfacing as bad C++ (invariant 7).
pub const INTERNAL_ERROR: &str = "DZ0804";
