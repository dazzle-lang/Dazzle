#+feature dynamic-literals
package dazzle

import "core:fmt"
import "core:strconv"
import "core:strings"
import "core:unicode"
import "core:unicode/utf8/utf8string"

RESET  :: "\e[0m"
RED    :: "\e[31m"
GREEN  :: "\e[32m"
YELLOW :: "\e[33m"
BLUE   :: "\e[34m"
BOLD   :: "\e[1m"

Lexer :: struct {
	file_string:   utf8string.String,
	tokens:        [dynamic]Token,
	line:          u32,
	col:           u32,
	rune_index:    int,
	current_token: Token,
	lines:         []string,
}

KEYWORDS: map[string]Token_Kind = {
	"let"   = .LET,
	"true"  = .TRUE,
	"false" = .FALSE,
}

lexer_init :: proc(lexer: ^Lexer) -> bool {

	s := #load("examples/main.dz", string)

	// Check if file is Unicode
	if len(s) >= 2 && ((s[0] == 0xFE && s[1] == 0xFF) || (s[0] == 0xFF && s[1] == 0xFE)) {
		fmt.println(RED, "This file is in Unicode format which is not compatible. Please convert it to UTF8.", RESET)
		return false
	// Check if file is UTF8
	} else if len(s) >= 3 && (s[0] == 0xEF && s[1] == 0xBB && s[2] == 0xBF) {
		s = s[3:]
	}

	new_s, ok := strings.replace_all(s, "\r\n", "\n")
	new_s, ok = strings.replace_all(new_s, "\r", "\n")

	lexer.lines = strings.split(new_s, "\n")

	utf8string.init(&lexer.file_string, new_s)

	return true
}

lexer_peek :: proc(lexer: ^Lexer, offset := 0) -> rune {
	if lexer.rune_index + offset >= utf8string.len(&lexer.file_string) {
		return 0
	}

	return utf8string.at(&lexer.file_string, lexer.rune_index + offset)
}

lexer_advance :: proc(lexer: ^Lexer) -> rune {
	if lexer.rune_index >= utf8string.len(&lexer.file_string) {
		return 0
	}

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

	// Check if the identifier is a keyword
	if identifier, ok := t.value.(Identifier); ok {
		s := utf8string.slice(
			&lexer.file_string,
			identifier.offset,
			identifier.offset + int(t.length),
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

lexer_error :: proc(lexer: ^Lexer, text: string, col_offset: int = 0) {
	line_text := lexer.lines[lexer.line]
	line := lexer.line + 1
	col := int(lexer.col) + col_offset
	fmt.println(RED, "Error: ", RESET, BOLD, text, RESET, sep = "")
	fmt.println(BLUE, " -> ", RESET, "examples/main.dz:", line, ":", col, sep = "")

	fmt.println()
	line_nr := fmt.tprint(line)
	fmt.println(BLUE, line_nr, " | ", RESET, line_text, sep = "")

	error_indication: strings.Builder
	strings.builder_init(&error_indication)

	for i in 0 ..< len(line_nr) + 3 {
		strings.write_rune(&error_indication, ' ')
	}

	i :u32 = 0
	
	for r in line_text {
		if i >= lexer.current_token.col + lexer.current_token.length || (i == lexer.current_token.col && lexer.current_token.length == 1) {
			break
		}

		if i >= lexer.current_token.col {
			strings.write_rune(&error_indication, '~')
		} else {
			// \t is a space, so we need to append it rather than writing a space
			if strings.is_space(r) {
				strings.write_rune(&error_indication, r)
			}
			else {
				strings.write_rune(&error_indication, ' ')
			}
		}

		i += 1
	}
	
	strings.write_rune(&error_indication, '^')

	fmt.println(RED, strings.to_string(error_indication), RESET, sep = "")
	fmt.println()

}

lexer_run :: proc(lexer: ^Lexer) {
	lexer.tokens = make([dynamic]Token, context.temp_allocator)

	len := utf8string.len(&lexer.file_string)


	loop: for lexer.rune_index < len {
		finish_token := false

		c := lexer_advance(lexer)
		next := lexer_peek(lexer)

		if is_character(c) {
			lexer.current_token.value = Identifier {
				offset = lexer.rune_index - 1,
			}

			for lexer.rune_index <= len {
				next = lexer_peek(lexer)

				if next == 0xFEFF || next == 0x200B {
					lexer_error(lexer, "Unsupported character. There is an invisible unsupported character here. Delete it", 1)
				} else if !is_character(next) && !is_number(next) || lexer.rune_index == len {
					lexer_push_token_literal(lexer)
					continue loop
				}

				c = lexer_advance(lexer)
			}
		} else if is_number(c) {
			lexer.current_token.value = Numeric_Literal {
				offset = lexer.rune_index - 1,
			}

			for lexer.rune_index <= len {
				literal := &lexer.current_token.value.(Numeric_Literal)
				next = lexer_peek(lexer)
				following := lexer_peek(lexer, 1)

				if is_character(next) && literal.has_dot {
					lexer_error(lexer, "Number format is invalid")
					lexer_push_token_literal(lexer)
					continue loop
				}

				if (!is_number(next) && !is_character(next)) ||
				   lexer.rune_index == len {
					if next == '.' {
						if is_character(following) {
							lexer_push_token_literal(lexer)
							continue loop
						}

						if !literal.has_dot {
							literal.has_dot = true
						} else {
							lexer_error(lexer, "Number format is invalid", 1)
							//lexer_push_token_literal(lexer)
							//continue loop
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
				offset = lexer.rune_index,
			}

			lexer.current_token.length -= 1
			previous_c := c

			for lexer.rune_index <= len {
				c = lexer_advance(lexer)

				if c == '\\' {
					c = lexer_advance(lexer)

					if c == '\n' {
						lexer.current_token.length -= 1
						lexer_error(lexer, "Unknown escape sequence")

						lexer.line += 1
						lexer.col = 0
						lexer_push_token_literal(lexer)

						continue loop
					}

					if c == 0 {
						lexer_error(lexer, "Unterminated string at the end of the file", -1)
						lexer_push_token_literal(lexer)
						continue loop
					}

					continue
				}

				if c == '"' {
					lexer.current_token.length -= 1
					lexer_push_token_literal(lexer)
					continue loop
				} else if lexer.rune_index == len || c == '\n' {
					if c == '\n' {
						lexer.current_token.length -= 1
					}

					lexer_error(lexer, "Unterminated string", -1)
					lexer_push_token_literal(lexer)

					if c == '\n' do break
					else do continue loop
				}
			}
		}

		if c == 0xFEFF || c == 0x200B {
			lexer_error(lexer, "Unsupported character. There is an invisible unsupported character here. Delete it")
			lexer_consume_token(lexer)
		}
		if c == '\n' {
			lexer_push_token(lexer, .TERMINATOR)
			lexer.col = 0
			lexer.line += 1
		} else if c == '/' && next == '/' {
			lexer_push_token(lexer, .TERMINATOR)

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

		if c == '\t' {
			lexer_consume_token(lexer)
		}
		else if unicode.is_space(c) {
			lexer_consume_token(lexer)
		} else {
			switch c {
			case ';':
				lexer_push_token(lexer, .TERMINATOR)
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
					lexer_error(lexer, fmt.tprint("Undefined token:", c))
					lexer_consume_token(lexer)
				}
			}
		}
	}

	lexer_push_token(lexer, .EOF)
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

@(private = "file")
is_number :: #force_inline proc(r: rune) -> bool {
	return r >= '0' && r <= '9'
}

is_character :: #force_inline proc(r: rune) -> bool {
	return unicode.is_alpha(r) || r == '_'
}