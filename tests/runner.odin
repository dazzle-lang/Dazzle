package tests

import dazzle "../src"
import "core:fmt"
import "core:os"
import "core:strings"
import "core:testing"
import "core:unicode/utf8/utf8string"

@(test)
run_numeric :: proc(t: ^testing.T) {
	run_test(t, "tests/cases/numeric.dz")
}

@(private="file")
run_test :: proc(t: ^testing.T, file_path: string) {
	lexer: dazzle.Lexer
	snapshot_path := fmt.tprint(file_path[:len(file_path)-2], "snap", sep="")
	snapshot_path_new := fmt.tprint(file_path[:len(file_path)-2], "snap.new", sep="")

	content, ok := os.read_entire_file(file_path, context.temp_allocator)

	if ok != os.General_Error.None {
		testing.expectf(t, false, "File doesn't exist: %s", file_path)
	}

	dazzle.lexer_init(&lexer, strings.clone_from(content, context.temp_allocator))
	defer dazzle.lexer_destroy(&lexer)

	dazzle.lexer_run(&lexer)

	result := lexer_serialize(&lexer, file_path)

	snapshot_content, has_snapshot := os.read_entire_file(snapshot_path, context.temp_allocator)

	if has_snapshot == os.General_Error.None {
		if result != strings.clone_from_bytes(snapshot_content, context.temp_allocator) {
			err := os.write_entire_file_from_string(snapshot_path_new, result)
			testing.expectf(t, false, "Snapshot changed. Wrote \"%s\"", snapshot_path_new)
		}

		return
	}

	err := os.write_entire_file_from_string(snapshot_path, result)

	if err != os.General_Error.None {
		testing.expectf(t, false, "Error writing snapshot file:%s", snapshot_path)
	}
}

lexer_serialize :: proc(lexer: ^dazzle.Lexer, file_path: string) -> string {
	builder: strings.Builder
	strings.builder_init(&builder)
	defer strings.builder_destroy(&builder)

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
				sep = ""
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
		strings.write_string(&builder, fmt.tprint( " ", file_path,"(", line, ":", col, ") len=", length, " ", sep = ""))
		strings.write_string(&builder, message)
    }

	return strings.to_string(builder)
}
