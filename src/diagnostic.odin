package dazzle

DiagnosticLevel :: enum {
    ERROR,
    WARNING
}

Diagnostic :: struct {
    message: string,
    line: int,
    column: int,
    length: int,
    level: DiagnosticLevel
}

diagnostic_push :: proc(diagnostics: ^[dynamic]Diagnostic, message: string, line: int, column: int, length: int, level: DiagnosticLevel) {
    append(diagnostics, Diagnostic {
        message = message,
        line = line,
        column = column,
        length = length,
        level = level
    })
}