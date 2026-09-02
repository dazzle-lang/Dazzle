#+feature dynamic-literals
package dazzle

import "core:fmt"
import "core:unicode"
import "core:unicode/utf8/utf8string"

Lexer :: struct {
	file_string:   utf8string.String,
	tokens:        [dynamic]Token,
	line:          u32,
	col:           u32,
	rune_index:    int,
	current_token: Token,
}

KEYWORDS: map[string]Token_Kind = {
	"let"   = .LET,
	"true"  = .TRUE,
	"false" = .FALSE,
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

	if string_literal, ok := t.value.(String_Literal); ok {
		s := utf8string.slice(
			&lexer.file_string,
			string_literal.offset,
			string_literal.offset + int(t.length),
		)

		if token_kind, ok := KEYWORDS[s]; ok {
			t.value = token_kind
		}
	}

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

				if unicode.is_alpha(next) && literal.has_dot {
					lexer_push_token_literal(lexer)
					fmt.println("Number format is invalid at", lexer.line + 1, ":", lexer.col + 1)
					continue loop
				}

				if (!unicode.is_number(next) && !unicode.is_alpha(next)) ||
				   lexer.rune_index == len {
					if next == '.' {
						if !literal.has_dot {
							literal.has_dot = true
						} else {
							lexer_push_token_literal(lexer)
							fmt.println(
								"Number format is invalid at",
								lexer.line + 1,
								":",
								lexer.col + 1,
							)
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

		if c == '"' {
			lexer.current_token.value = String_Literal {
				offset = lexer.rune_index - 1,
			}

			previous_char := c

			for lexer.rune_index < len {
				c = lexer_advance(lexer)

				if c == '"' && previous_char != '\\' {
					lexer_push_token_literal(lexer)
					break
				}

				previous_char = c
			}
		}

		if c == '\n' {
			lexer_push_token(lexer, .TERMINATOR)
			lexer.col = 0
			lexer.line += 1
		} else if c == '/' && next == '/' {
			for lexer.rune_index < len {
				c = lexer_advance(lexer)

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
				handle_dot(lexer, next)
			case '+':
				handle_plus(lexer, next)
			case '-':
				handle_minus(lexer, next)
			case '*':
				handle_star(lexer, next)
			case '/':
				handle_slash(lexer, next)
			case '=':
				handle_equal(lexer, next)
			case '|':
				handle_pipe(lexer, next)
			case ',':
				handle_comma(lexer, next)
			case '(':
				handle_left_paren(lexer, next)
			case ')':
				handle_right_paren(lexer, next)
			case '{':
				handle_left_brace(lexer, next)
			case '}':
				handle_right_brace(lexer, next)
			case:
				{
					lexer_consume_token(lexer)
					fmt.println("Undefined token:", c, "at", lexer.line + 1, ":", lexer.col + 1)
				}
			}
		}
	}
}

@(private = "file")
handle_dot :: proc(lexer: ^Lexer, next: rune) {
	lexer_push_token(lexer, .DOT)
}

@(private = "file")
handle_plus :: proc(lexer: ^Lexer, next: rune) {
	if next == '=' {
		lexer_advance(lexer)
		lexer_push_token(lexer, .PLUS_EQ)
	} else {
		lexer_push_token(lexer, .PLUS)
	}
}

@(private = "file")
handle_minus :: proc(lexer: ^Lexer, next: rune) {
	if next == '=' {
		lexer_advance(lexer)
		lexer_push_token(lexer, .MINUS_EQ)
	} else {
		lexer_push_token(lexer, .MINUS)
	}
}

@(private = "file")
handle_star :: proc(lexer: ^Lexer, next: rune) {
	if next == '=' {
		lexer_advance(lexer)
		lexer_push_token(lexer, .STAR_EQ)
	} else {
		lexer_push_token(lexer, .STAR)
	}
}

@(private = "file")
handle_slash :: proc(lexer: ^Lexer, next: rune) {
	if next == '=' {
		lexer_advance(lexer)
		lexer_push_token(lexer, .SLASH_EQ)
	} else {
		lexer_push_token(lexer, .SLASH)
	}
}

@(private = "file")
handle_equal :: proc(lexer: ^Lexer, next: rune) {
	if next == '=' {
		lexer_advance(lexer)
		lexer_push_token(lexer, .EQ_EQ)
	} else {
		lexer_push_token(lexer, .EQ)
	}
}

@(private = "file")
handle_pipe :: proc(lexer: ^Lexer, next: rune) {
	if next == '|' {
		lexer_advance(lexer)
		lexer_push_token(lexer, .PIPE_PIPE)
	} else {
		lexer_push_token(lexer, .PIPE)
	}
}

@(private = "file")
handle_comma :: proc(lexer: ^Lexer, next: rune) {
	lexer_push_token(lexer, .COMMA)
}

@(private = "file")
handle_left_paren :: proc(lexer: ^Lexer, next: rune) {
	lexer_push_token(lexer, .LEFT_PAREN)
}

@(private = "file")
handle_right_paren :: proc(lexer: ^Lexer, next: rune) {
	lexer_push_token(lexer, .RIGHT_PAREN)
}

@(private = "file")
handle_left_brace :: proc(lexer: ^Lexer, next: rune) {
	lexer_push_token(lexer, .LEFT_BRACE)
}

@(private = "file")
handle_right_brace :: proc(lexer: ^Lexer, next: rune) {
	lexer_push_token(lexer, .RIGHT_BRACE)
}
