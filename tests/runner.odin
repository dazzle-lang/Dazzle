package tests

import dazzle "../src"
import "core:fmt"
import "core:os"
import "core:strings"
import "core:testing"
import "core:unicode/utf8/utf8string"

@(test)
run_numeric :: proc(t: ^testing.T) {
	lexer: dazzle.Lexer

	path := "tests/cases/numeric.dz"
	content, ok := os.read_entire_file(path, context.temp_allocator)

	if ok != os.General_Error.None {
		testing.fail_now(t, fmt.tprint("File doesn't exist:", path))
	}

	dazzle.lexer_init(&lexer, strings.clone_from(content))
	dazzle.lexer_run(&lexer)

	result := lexer_serialize(&lexer)
	err := os.write_entire_file_from_string("tests/cases/numeric.snap", result)

	if err != os.General_Error.None {
		testing.fail_now(t, fmt.tprint("Error writing snapshot file of:", path))
	}
}

lexer_serialize :: proc(lexer: ^dazzle.Lexer) -> string {
	builder: strings.Builder
	strings.builder_init(&builder)

	strings.write_string(&builder, "===TOKENS===\n")

	for t in lexer.tokens {
		value: string

		switch v in t.value {
		case dazzle.Identifier:
			{
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint(s)
			}
		case dazzle.String_Literal:
			{
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint("STRING(\"", s, "\")", sep = "")
			}
		case dazzle.Numeric_Literal:
			{
				s := utf8string.slice(&lexer.file_string, v.offset, v.offset + int(t.length))
				value = fmt.tprint("NUMBER(", s, ")", sep = "")
			}
		case dazzle.Token_Kind:
			{
				value = fmt.tprint(".", v, sep = "")
			}
		}

		strings.write_string(
			&builder,
			fmt.tprint(
				t.line + 1,
				":",
				t.col + 1,
				" length: ",
				t.length,
				" ",
				value,
				"\n",
				sep = "",
			),
		)
	}

	strings.write_string(&builder, "\n===DIAGNOSTICS===")

	for d in lexer.diagnostics {
		line_text := lexer.lines[d.line]
		message := d.message
		line := d.line + 1
		col := d.column + 1
		length := d.length

        strings.write_string(&builder, fmt.tprint("\n", d.level, sep = ""))
		strings.write_string(&builder, fmt.tprint( " examples/main.dz(", line, ":", col, ") len=", length, " ", sep = ""))
		strings.write_string(&builder, message)
    }

	return strings.to_string(builder)
}
