package dazzle

import "core:fmt"
import "core:strings"

@(private="file")
RESET :: "\e[0m"

Output_Color :: enum {
	RESET,
	RED,
	GREEN,
	YELLOW,
	BLUE,
	BOLD,
}

println :: proc(args: ..any) {
	fmt.print(..args, sep = "")
    fmt.println(RESET)
}

print :: proc(args: ..any) {
	fmt.print(..args, sep = "")
    fmt.print(RESET)
}

output_color :: proc(colors: ..Output_Color) {
	builder: strings.Builder
	strings.builder_init(&builder)

	for c in colors {
		switch c {
		case .RESET:
			strings.write_string(&builder, RESET)
		case .RED:
			strings.write_string(&builder, "\e[31m")
		case .GREEN:
			strings.write_string(&builder, "\e[32m")
		case .YELLOW:
			strings.write_string(&builder, "\e[33m")
		case .BLUE:
			strings.write_string(&builder, "\e[34m")
		case .BOLD:
			strings.write_string(&builder, "\e[1m")
		}
	}

	fmt.print(strings.to_string(builder))
}
