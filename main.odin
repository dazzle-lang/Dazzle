package dazzle

import "core:sys/windows"
import "core:fmt"
import "core:unicode"
import "core:unicode/utf8"
import "core:unicode/utf8/utf8string"
import "core:strings"


main :: proc() {

	windows.SetConsoleOutputCP(.UTF8)

	lexer: Lexer
	lexer_init(&lexer)

	lexer_run(&lexer)

	for t in lexer.tokens {
		value : string

		switch v in t.value {
			case String_Literal: {
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint("\"", s, "\"", sep = "")
			}
			case Numeric_Literal: {
				s:= utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint(" \"", s, "\"", sep = "")
			}
			case Token_Kind: {
				value = fmt.tprint(".", v, sep = "")
			}
		}


		fmt.println(t.line + 1, ":", t.col + 1, "\tlength:", t.length,  value)
	}
}
