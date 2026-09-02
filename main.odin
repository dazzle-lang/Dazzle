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
		fmt.println(t)
	}
}
