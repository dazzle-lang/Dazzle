package dazzle

Token_Kind :: enum {
	EQ,             // =
	PLUS,           // +
	MINUS,          // -
	STAR,           // *
	SLASH,          // /
	PIPE,           // |
	STAR_EQ,        // *=
	PLUS_EQ,        // +=
	MINUS_EQ,       // -=
	SLASH_EQ,       // /=
	EQ_EQ,          // ==
	PIPE_PIPE,      // ||
    
	DOT,            // .
    COMMA,          // ,
    LEFT_PAREN,     // (
    RIGHT_PAREN,    // )
    LEFT_BRACE,     // {
    RIGHT_BRACE,    // }

	// Keywords
	LET,
	TRUE,
	FALSE,
}

String_Literal :: struct {
	offset: int,
}

Numeric_Literal :: struct {
	offset:  int,
	has_dot: bool,
}

Token_Value :: union {
	Token_Kind,
	String_Literal,
	Numeric_Literal,
}

Token :: struct {
	line:   u32,
	col:    u32,
	length: u32,
	value:  Token_Value,
}