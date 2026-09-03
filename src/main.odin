package dazzle

import "core:fmt"
import "core:strings"
import "core:sys/windows"
import "core:unicode"
import "core:unicode/utf8"
import "core:unicode/utf8/utf8string"


main :: proc() {

	windows.SetConsoleOutputCP(.UTF8)

	lexer: Lexer

	if !lexer_init(&lexer, #load("../examples/main.dz", string)) {
		return
	}

	lexer_run(&lexer)

	for t in lexer.tokens {
		value: string

		switch v in t.value {
		case Identifier:
			{
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint(s)
			}
		case String_Literal:
			{
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint("STRING(\"", s, "\")", sep = "")
			}
		case Numeric_Literal:
			{
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint("NUMBER(", s, ")", sep = "")
			}
		case Token_Kind:
			{
				value = fmt.tprint(".", v, sep = "")
			}
		}


		println(t.line + 1, " : ", t.col + 1, "\tlength: ", t.length, "\t", value)
	}

	println()

	for d in lexer.diagnostics {
		handle_error(&lexer, d)
	}
}

handle_error :: proc(lexer: ^Lexer, diagnostic: Diagnostic) {
	line_text := lexer.lines[diagnostic.line]
	message := diagnostic.message
	line := diagnostic.line + 1
	col := diagnostic.column
	length := diagnostic.length

	output_color(.BOLD)
	print("examples/main.dz(", line, ":", col + 1, ") ")

	switch diagnostic.level {
	case .ERROR:
		output_color(.RED)
		print("Error: ")
	case .WARNING:
		output_color(.YELLOW)
		print("Warning: ")
	}

	println(message)

	line_nr := fmt.tprint(line)
	output_color(.BLUE)
	print(line_nr, " | ")
	println(line_text)

	error_indication: strings.Builder
	strings.builder_init(&error_indication)

	for i in 0 ..< len(line_nr) + 3 {
		strings.write_rune(&error_indication, ' ')
	}

	i := 0

	for r in line_text {
		if i >= col + length - 1 {
			break
		}

		if i >= col {
			strings.write_rune(&error_indication, '~')
		} else {
			// \t is a space, so we need to append it rather than writing a space
			if strings.is_space(r) {
				strings.write_rune(&error_indication, r)
			} else {
				strings.write_rune(&error_indication, ' ')
			}
		}

		i += 1
	}

	strings.write_rune(&error_indication, '^')

	output_color(.RED)
	println(strings.to_string(error_indication))
	println()
}
