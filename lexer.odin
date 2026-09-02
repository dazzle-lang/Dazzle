package dazzle

import "core:fmt"
import "core:unicode"
import "core:unicode/utf8/utf8string"

Token_Kind :: enum {
	EQ,			// =
	PLUS,		// +
	MINUS, 		// -
	STAR, 		// *
	SLASH,		// /
	PIPE,		// |

	STAR_EQ, 	// *=
	PLUS_EQ,	// +=
	MINUS_EQ,	// -=
	SLASH_EQ,	// /=
	EQ_EQ,		// ==
	PIPE_PIPE,	// ||

	// Keywords
	LET,
	TRUE,
	FALSE
}

String_Literal :: struct {
	value: [dynamic]rune,
}

Numeric_Literal :: struct {
	value: [dynamic]rune,
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

lexer_peek :: proc(lexer: ^Lexer) -> rune {
	if lexer.rune_index >= utf8string.len(&lexer.file_string) {
		return 0
	}

	return utf8string.at(&lexer.file_string, lexer.rune_index)
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


	for lexer.rune_index < len {
		finish_token := false

		c := lexer_advance(lexer)
		next := lexer_peek(lexer)

		if c == '\n' {
			lexer.col = 0
			lexer.line += 1
			lexer_consume_token(lexer)
			continue
		}

		if c == '/' && next == '/' {
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

		switch c {
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
		}

		if unicode.is_space(c) {
			lexer_consume_token(lexer)
		}

		//is_space := unicode.is_space(c)
		//is_special_character := !(unicode.is_letter(c) || unicode.is_number(c))
		//is_2char_token := rune_can_be_2char_token(c)

		// if (is_space || (was_special_character && !was_2char_token) || (is_special_character && !is_2char_token) || (was_2char_token && !is_2char_token) || (is_2char_token && !was_2char_token)) {
		// 	if current_token.length > 0 {
		// 		append(&lexer.tokens, current_token)
		// 	}

		// 	current_token = {
		// 		col = lexer.col,
		// 		line = lexer.line,
		// 		length = 0,
		// 		value = {}
		// 	}

		// 	if unicode.is_space(c) {
		// 		finish_token = true
		// 	}
		// }

		// if !finish_token {
		// 	current_token.length += 1
		// 	append(&current_token.value, c)
		// }
	}
}
