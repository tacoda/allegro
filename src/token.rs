#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // literals
    Int(i64),
    Float(f64),
    Str(String),     // raw content, may contain `#{...}` interpolation
    Atom(String),    // :ok, :"quoted"
    Ident(String),   // lowercase / _leading — variables and function names
    Alias(String),   // Capitalized — module names
    KwKey(String),   // `name:` in keyword/option position (incl `do:`)
    True,
    False,
    Nil,
    // keywords
    Defmodule,
    Def,
    Defp,
    Do,
    End,
    Fn,
    When,
    Case,
    Cond,
    If,
    Unless,
    Else,
    With,
    And,
    Or,
    Not,
    // punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    MapOpen, // %{
    Comma,
    Dot,
    Bar,      // |  (cons / map update)
    Arrow,    // ->
    LArrow,   // <-
    FatArrow, // =>
    Amp,      // &  (capture)
    Caret,    // ^  (pin)
    Colon,   // :
    // operators
    Match,   // =
    Pipe,    // |>
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    Concat,     // <>
    ListConcat, // ++
    ListDiff,   // --
    AndAnd,     // &&
    OrOr,       // ||
    Bang,       // !
    // structural
    Newline,
    Eof,
}
