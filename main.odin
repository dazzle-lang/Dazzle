package dazzle

import "core:sys/windows"
import "core:fmt"
import "core:unicode"
import "core:unicode/utf8"
import "core:unicode/utf8/utf8string"
import "core:strings"

Token :: struct {
	line:  u32,
	col:   u32,
	length: u32,
	value: [dynamic]rune,
}

rune_can_be_2char_token :: proc (c: rune) -> bool {
	/*
	+= -= *= /= == != >= <= && || ::
	*/
	return c == '=' || c == '+' || c == '-' || c == '*' || c == '/' || c == '&' || c == '|' || c == '!' || c == '>' || c == '<' || c == ':'
}

main :: proc() {

	windows.SetConsoleOutputCP(.UTF8)

	file_string: utf8string.String
	utf8string.init(&file_string, #load("main.dz", string))

	tokens: [dynamic]Token

	line :u32 = 0
	col :u32 = 0

	current_token: Token
	was_2char_token := false
	was_special_character := false

	for i := 0; i < utf8string.len(&file_string); i += 1 {
		finish_token := false

		c := utf8string.at(&file_string, i)
		col += 1

		if c == '\n' {
			col = 0
			line += 1
		}

		if c == '/' && utf8string.at(&file_string, i + 1) == '/' {
			for ; i < utf8string.len(&file_string); i += 1 {
				if utf8string.at(&file_string, i) == '\n' {
					col = 0
					line += 1
					break
				}
			}

			continue
		}

		is_space := unicode.is_space(c)
		is_special_character := !(unicode.is_letter(c) || unicode.is_number(c))
		is_2char_token := rune_can_be_2char_token(c)

		if (is_space || (was_special_character && !was_2char_token) || (is_special_character && !is_2char_token) || (was_2char_token && !is_2char_token) || (is_2char_token && !was_2char_token)) {
			if current_token.length > 0 {
				append(&tokens, current_token)
			}

			current_token = {
				col = col,
				line = line,
				length = 0,
				value = {}
			}

			if unicode.is_space(c) {
				finish_token = true
			}
		}

		if !finish_token {
			current_token.length += 1
			append(&current_token.value, c)
		}

		was_special_character = is_special_character
		was_2char_token = is_2char_token
	}

	for t in tokens {
		fmt.println(t)
	}
}
