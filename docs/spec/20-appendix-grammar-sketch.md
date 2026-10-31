# 20. Appendix: grammar sketch

Informal EBNF. `TERM` is a terminator (§2.2). This is a sketch for orientation, not the
normative grammar — the parser is the normative grammar, and this appendix is updated to
match it.

```ebnf
file          = { import | declaration } ;

import        = "import" ( module_path "as" ident
                         | module_path "::" "{" ident { "," ident } "}" ) TERM ;
module_path   = ident { "::" ident } ;

declaration   = { attribute } [ visibility ] ( fn_decl | struct_decl | union_decl
                                             | enum_decl | trait_decl | impl_decl
                                             | const_decl | static_decl | macro_decl
                                             | macro_invoke ) ;
static_decl   = "static" ident ":" type "=" expr TERM ;   (* §7.7 *)
cfg_if<T>     = "#" "if" cfg_cond "{" { T } "}"                  (* §12.10: T is a declaration, *)
                [ "#" "else" ( cfg_if<T> | "{" { T } "}" ) ] ;   (* a member or a statement    *)
cfg_cond      = cfg_and { "||" cfg_and } ;
cfg_and       = cfg_unary { "&&" cfg_unary } ;
cfg_unary     = "!" cfg_unary | "(" cfg_cond ")" | ident [ "::" ident ] ;
visibility    = "pub" [ "(" "module" ")" ] ;
attribute     = "#" "[" ident [ "(" token_tree ")" ] "]" ;

fn_decl       = [ "unsafe" ] [ "mut" ] "fn" ident [ generics ] "(" [ params ] ")"
                [ ( "->" | "!->" ) type [ "in" in_list ] ]
                [ "kills" kill_list ] block ;   (* §6.7 *)
kill_list     = tie_path { "," tie_path } | "(" tie_path { "," tie_path } ")" ;
in_list       = tie_path | "(" tie_path { "," tie_path } ")" ;
params        = param { "," param } ;
param         = [ "&" | "&mut" ] "self" | ident ":" type [ "in" tie_path ] [ "=" expr ] ;
tie_path      = ( ident | "self" ) { "." ident | "[" "*" "]" | "@" ident } ;
generics      = "<" gparam { "," gparam } ">" ;
gparam        = ident [ ":" bound { "+" bound } ] ;

struct_decl   = "struct" ident [ generics ] [ ":" path ] "{" { member } "}" ;  (* one base *)
member        = field | fn_decl | ctor | tie_decl ;
field         = { attribute } [ visibility ] [ "own" ] ident ":" type
                [ "=" expr ] [ raw_string ] TERM ;  (* the tag, §13.1 *)
tie_decl      = "tie" ( "@" ident | "[" "*" "]" ) "=" kill_list TERM ;  (* paths from fields, §6.7 *)
ctor          = ident "(" [ params ] ")" block ;
impl_decl     = "impl" [ generics ] path [ "<" type_list ">" ] "for" type "{" { impl_item } "}" ;
impl_item     = "type" ident "=" type TERM | fn_decl ;

union_decl    = "union" ident [ generics ] "{" { variant | fn_decl } "}" ;   (* methods, §7.5 *)
variant       = ident [ "{" field_list "}" ] TERM ;   (* a bare ident carries that struct, §7.5 *)
enum_decl     = "enum" ident [ ":" type ] "{" { ident [ "=" expr ] TERM } "}" ;
trait_decl    = "trait" ident [ generics ] [ ":" trait_list ] "{" { trait_item } "}" ;
trait_list    = path [ "<" type_list ">" ] { "+" path [ "<" type_list ">" ] } ;
trait_item    = "type" ident TERM | fn_decl_sig | fn_decl ;

type          = access_type [ "?" ] ;
access_type   = "^" type | "&" [ "mut" ] type
              | "[" [ expr ] "]" type
              | "own" "^" type
              | "const" type    (* only `const str`, §5.3 *)
              | "fn" "(" [ fn_type_params ] ")"
                [ ( "->" | "!->" ) type [ "in" ( tie_path | "(" tie_path { "," tie_path } ")" ) ] ]
                [ "kills" ( tie_path | "(" tie_path { "," tie_path } ")" ) ]
                (* names needed only by a clause, §6.7; a comma after a bare path begins the
                   next parameter, so more than one path is parenthesised here; an `in`
                   after the result is the type's, not an enclosing parameter's *)
              | "(" type_list ")"
              | path [ "<" type_list ">" ] ;
path          = ident { "::" ident } ;
fn_type_params = fn_type_param { "," fn_type_param } ;
fn_type_param = [ ident ":" ] type ;
```

`?` binds to the innermost complete type to its left, so `^T?` parses as `^(T?)`. Inside a
function type, a trailing `?` belongs to the return type; `(fn(i32) -> bool)?` is how you
spell an optional function pointer.

```ebnf

statement     = let_stmt | assign | expr_stmt | if_stmt | while_stmt | for_stmt
              | switch_stmt | return_stmt | defer_stmt | block | "unsafe" block
              | macro_invoke
              | "break" [ ident ] | "continue" [ ident ] ;   (* no value after `break` *)

let_stmt      = "let" [ "mut" ] pattern [ ":" type ] [ "=" expr ] TERM ;
assign        = place ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) expr TERM ;
if_stmt       = "if" if_head block { "else" "if" if_head block } [ "else" block ] ;
if_head       = expr | "let" [ "mut" ] ident "=" expr ;
while_stmt    = [ ident ":" ] "while" expr block ;
for_stmt      = [ ident ":" ] "for" for_binding { "," for_binding } "in" expr block ;
for_binding   = [ "&" [ "mut" ] ] ( ident | "_" ) ;
switch_stmt   = "switch" expr "{" { case_arm } [ "default" block ] "}" ;
case_arm      = "case" case_label { "," case_label } block ;
case_label    = ident [ "(" [ "&" [ "mut" ] ] ident ")" ]  (* a variant, or a constant's name *)
              | expr ;                      (* a constant: `1`, `"idle"`, `lib::MAX` *)
or_expr       = expr "or" or_clause ;
or_clause     = "{" [ ident "->" ] { statement } "}"
              | "return" [ expr ]
              | "break" [ ident ] | "continue" [ ident ]
              | "ignore" | "unreachable"
              | expr ;
return_stmt   = "return" [ expr ] TERM ;
defer_stmt    = "defer" ( statement | block ) ;

expr          = assign_free_expr ;   (* precedence per §11.1; includes `or` *)
primary       = literal | "none" | path | struct_lit | call | index | field_access
              | "(" expr ")" | "move" "(" expr ")"
              | "error" "(" [ expr { "," expr } ] ")"
              | "&" expr | "&mut" expr | unary | intrinsic | macro_invoke
              | "." ident [ struct_lit_body ]
              | "unsafe" block
              | array_lit ;
array_lit     = "[" [ expr { "," expr } [ "," ] ] "]" ;
struct_lit    = path struct_lit_body ;
struct_lit_body = "{" [ field_init { "," field_init } [ "," ] ] "}" ;
field_init    = ident ":" expr | expr ;
intrinsic     = "#" ident [ "(" [ expr ] ")" ] ;   (* a built-in macro, §12.7 *)

(* Macros, §12. A `token` is any token but an unbalanced bracket; a `token_tree` is a
   bracket-balanced run of them. *)
macro_decl    = "macro" ident "->" macro_kind "{" { syntax_rule } pattern_rule expand_rule "}" ;
macro_kind    = "decl" | "stmt" | "expr" ;                (* `type` is reserved *)
syntax_rule   = "syntax" ident "{" pattern "}" ;
pattern_rule  = "pattern" "{" pattern "}" ;
pattern       = { capture | "$" "(" pattern ")" "?" | token } ;
capture       = "$" ident ":" [ "[" [ token ] "]" ] frag_kind [ "?" ] ;   (* §12.3 *)
frag_kind     = "ident" | "literal" | "type" | "expr" | "stmt" | "block" | "code" | "decl"
              | ident ;                                    (* a `syntax` rule *)
expand_rule   = "expand" "{" template "}" ;
template      = { "$" ident { "." ident }
              | "#" "for" ident "in" "$" ident { "." ident } [ "[" token "]" ] "{" template "}"
              | "#" "if" [ "!" ] "$" ident { "." ident } "{" template "}"
                [ "#" "else" "{" template "}" ]
              | "#" "error" "(" [ "$" ident { "." ident } "," ] string ")"
              | token } ;
macro_invoke  = "#" path ( token_tree_to_line_end        (* declaration, statement position *)
                         | bracket_group ) ;            (* expression position *)
```
