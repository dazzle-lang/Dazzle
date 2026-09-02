package dazzle

import "core:fmt"
import "core:unicode"
import "core:unicode/utf8/utf8string"

Token_Kind :: enum {
	EQ, // =
	PLUS, // +
	MINUS, // -
	STAR, // *
	SLASH, // /
	PIPE, // |
	STAR_EQ, // *=
	PLUS_EQ, // +=
	MINUS_EQ, // -=
	SLASH_EQ, // /=
	EQ_EQ, // ==
	PIPE_PIPE, // ||
	DOT, // .

	// Keywords
	LET,
	TRUE,
	FALSE,
}

Numeric_Type :: enum {
	INT,
	REAL,
	BINARY,
	OCTAL,
	HEX,
}

String_Literal :: struct {
	offset: int,
}

Numeric_Literal :: struct {
	offset:      int,
	prefix_zero: bool,
	type:        Numeric_Type,
}

Token_Value :: union {
	Token_Kind,
	String_Literal,
	Numeric_Literal,
}

Token :: struct {
	line:   u32,
	col:    u32,
	length: u32,
	value:  Token_Value,
}

Lexer :: struct {
	file_string:   utf8string.String,
	tokens:        [dynamic]Token,
	line:          u32,
	col:           u32,
	rune_index:    int,
	current_token: Token,
}

lexer_init :: proc(lexer: ^Lexer) {
	utf8string.init(&lexer.file_string, #load("examples/main.dz", string))
}

lexer_peek :: proc(lexer: ^Lexer, offset := 0) -> rune {
	if lexer.rune_index >= utf8string.len(&lexer.file_string) {
		return 0
	}

	return utf8string.at(&lexer.file_string, lexer.rune_index + offset)
}

lexer_advance :: proc(lexer: ^Lexer) -> rune {
	c := utf8string.at(&lexer.file_string, lexer.rune_index)
	lexer.col += 1
	lexer.rune_index += 1
	lexer.current_token.length += 1

	return c
}

lexer_push_token :: proc(lexer: ^Lexer, kind: Token_Kind) {
	t := lexer_consume_token(lexer)
	t.value = kind

	append(&lexer.tokens, t)
}

lexer_push_token_literal :: proc(lexer: ^Lexer) {
	t := lexer_consume_token(lexer)
	append(&lexer.tokens, t)
}

lexer_consume_token :: proc(lexer: ^Lexer) -> Token {
	t := lexer.current_token

	lexer.current_token = {
		line   = lexer.line,
		col    = lexer.col,
		length = 0,
		value  = {},
	}

	return t
}

lexer_run :: proc(lexer: ^Lexer) {
	lexer.tokens = make([dynamic]Token, context.temp_allocator)

	len := utf8string.len(&lexer.file_string)


	loop: for lexer.rune_index < len {
		finish_token := false

		c := lexer_advance(lexer)
		next := lexer_peek(lexer)

		if unicode.is_alpha(c) {
			lexer.current_token.value = String_Literal {
				offset = lexer.rune_index - 1,
			}

			for lexer.rune_index <= len {
				next = lexer_peek(lexer)

				if !unicode.is_alpha(next) && !unicode.is_number(next) || lexer.rune_index == len {
					lexer_push_token_literal(lexer)
					continue loop
				}

				c = lexer_advance(lexer)
			}
		} else if unicode.is_number(c) {
			lexer.current_token.value = Numeric_Literal {
				offset = lexer.rune_index - 1,
			}

			for lexer.rune_index <= len {
				literal := &lexer.current_token.value.(Numeric_Literal)
				next = lexer_peek(lexer)

				if c == '0' && lexer.current_token.length == 1 && !literal.prefix_zero {
					literal.prefix_zero = true
				}

				if unicode.is_alpha(next) {
					if next == 'x' && literal.prefix_zero && literal.type == .INT {
						literal.type = .HEX
					} else if next == 'b' && literal.prefix_zero && literal.type == .INT {
						literal.type = .BINARY
					} else if next == 'o' && literal.prefix_zero && literal.type == .INT {
						literal.type = .OCTAL
					} else {
						lexer_push_token_literal(lexer)
						fmt.println("Number format is invalid at", lexer.line + 1, ":",lexer.col + 1 )
						continue loop
					}
				}
				else if !unicode.is_number(next) || lexer.rune_index == len {
					if next == '.' {
						if literal.type == .INT {
							literal.type = .REAL
						}
						else {
							lexer_push_token_literal(lexer)
							fmt.println("Number format is invalid at", lexer.line + 1, ":",lexer.col + 1 )
							continue loop
						}
					} else {
						lexer_push_token_literal(lexer)
						continue loop
					}
				}

				c = lexer_advance(lexer)
			}
		}

		if c == '\n' {
			lexer.col = 0
			lexer.line += 1
		} else if c == '/' && next == '/' {
			for lexer.rune_index < len {
				c := lexer_advance(lexer)

				if c == '\n' {
					lexer.col = 0
					lexer.line += 1
					break
				}
			}

			lexer_consume_token(lexer)
			continue
		}

		if unicode.is_space(c) {
			lexer_consume_token(lexer)
		} else {
			switch c {
			case '.':
				{
					lexer_push_token(lexer, .DOT)
				}
			case '+':
				{
					if next == '=' {
						lexer_advance(lexer)
						lexer_push_token(lexer, .PLUS_EQ)
					} else {
						lexer_push_token(lexer, .PLUS)
					}
				}
			case '-':
				{
					if next == '=' {
						lexer_advance(lexer)
						lexer_push_token(lexer, .MINUS_EQ)
					} else {
						lexer_push_token(lexer, .MINUS)
					}
				}
			case '*':
				{
					if next == '=' {
						lexer_advance(lexer)
						lexer_push_token(lexer, .STAR_EQ)
					} else {
						lexer_push_token(lexer, .STAR)
					}
				}
			case '/':
				{
					if next == '=' {
						lexer_advance(lexer)
						lexer_push_token(lexer, .SLASH_EQ)
					} else {
						lexer_push_token(lexer, .SLASH)
					}
				}
			case '=':
				{
					if next == '=' {
						lexer_advance(lexer)
						lexer_push_token(lexer, .EQ_EQ)
					} else {
						lexer_push_token(lexer, .EQ)
					}
				}
			case '|':
				{
					if next == '|' {
						lexer_advance(lexer)
						lexer_push_token(lexer, .PIPE_PIPE)
					} else {
						lexer_push_token(lexer, .PIPE)
					}
				}
			case:
				{
					lexer_consume_token(lexer)
					fmt.println("Undefined token:", c, "at", lexer.line + 1, ":", lexer.col + 1)
				}
			}
		}
	}
}
