use crate::token::Tok;

pub fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();

    while i < c.len() {
        let ch = c[i];
        match ch {
            ' ' | '\t' | '\r' => i += 1,
            '\n' | ';' => {
                out.push(Tok::Newline);
                i += 1;
            }
            '#' => skip_comment(&c, &mut i),
            '"' => {
                let (s, ni) = lex_string(&c, i)?;
                out.push(Tok::Str(s));
                i = ni;
            }
            ':' if starts_atom(&c, i + 1) => {
                let (a, ni) = lex_atom(&c, i)?;
                out.push(Tok::Atom(a));
                i = ni;
            }
            _ if ch.is_ascii_digit() => {
                let (t, ni) = lex_number(&c, i);
                out.push(t);
                i = ni;
            }
            _ if is_ident_start(ch) => lex_ident(&c, &mut i, &mut out),
            _ => match symbol_token(&c, i) {
                Some((t, len)) => {
                    out.push(t);
                    i += len;
                }
                None => return Err(format!("unexpected character '{}'", ch)),
            },
        }
    }
    out.push(Tok::Eof);
    Ok(fix_or(out))
}

// `||` shares its leading char with `|` and `|>`, so it lexes as two `Bar`
// tokens; collapse an adjacent pair into a single `OrOr`.
fn fix_or(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        if toks[i] == Tok::Bar && toks.get(i + 1) == Some(&Tok::Bar) {
            out.push(Tok::OrOr);
            i += 2;
        } else {
            out.push(toks[i].clone());
            i += 1;
        }
    }
    out
}

fn skip_comment(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i] != '\n' {
        *i += 1;
    }
}

fn peek(c: &[char], i: usize) -> Option<char> {
    c.get(i).copied()
}

// Punctuation and operators: try two-char operators first, then one-char.
fn symbol_token(c: &[char], i: usize) -> Option<(Tok, usize)> {
    if let (Some(a), Some(b)) = (peek(c, i), peek(c, i + 1)) {
        if let Some(t) = two_char(a, b) {
            return Some((t, 2));
        }
    }
    one_char(c[i]).map(|t| (t, 1))
}

fn two_char(a: char, b: char) -> Option<Tok> {
    Some(match (a, b) {
        ('%', '{') => Tok::MapOpen,
        ('|', '>') => Tok::Pipe,
        ('-', '>') => Tok::Arrow,
        ('-', '-') => Tok::ListDiff,
        ('+', '+') => Tok::ListConcat,
        ('<', '>') => Tok::Concat,
        ('<', '-') => Tok::LArrow,
        ('&', '&') => Tok::AndAnd,
        ('=', '>') => Tok::FatArrow,
        ('=', '=') => Tok::EqEq,
        ('!', '=') => Tok::NotEq,
        ('<', '=') => Tok::Le,
        ('>', '=') => Tok::Ge,
        _ => return None,
    })
}

fn one_char(ch: char) -> Option<Tok> {
    Some(match ch {
        '(' => Tok::LParen,
        ')' => Tok::RParen,
        '[' => Tok::LBracket,
        ']' => Tok::RBracket,
        '{' => Tok::LBrace,
        '}' => Tok::RBrace,
        ',' => Tok::Comma,
        '.' => Tok::Dot,
        ':' => Tok::Colon,
        '|' => Tok::Bar,
        '%' => Tok::Percent,
        '&' => Tok::Amp,
        '^' => Tok::Caret,
        '=' => Tok::Match,
        '+' => Tok::Plus,
        '-' => Tok::Minus,
        '*' => Tok::Star,
        '/' => Tok::Slash,
        '<' => Tok::Lt,
        '>' => Tok::Gt,
        '!' => Tok::Bang,
        _ => return None,
    })
}

fn starts_atom(c: &[char], i: usize) -> bool {
    matches!(peek(c, i), Some(ch) if is_ident_start(ch)) || peek(c, i) == Some('"')
}

// A keyword key `name:` — the colon must be followed by a separator/space,
// otherwise it's `:atom` etc.
fn kwkey_follows(c: &[char], i: usize) -> bool {
    match peek(c, i) {
        None => true,
        Some(ch) => matches!(ch, ' ' | '\t' | '\r' | '\n' | ',' | ']' | '}' | ')'),
    }
}

fn lex_ident(c: &[char], i: &mut usize, out: &mut Vec<Tok>) {
    let (word, ni) = read_word(c, *i);
    if peek(c, ni) == Some(':') && kwkey_follows(c, ni + 1) {
        out.push(Tok::KwKey(word));
        *i = ni + 1;
    } else {
        out.push(classify(word));
        *i = ni;
    }
}

fn lex_string(c: &[char], start: usize) -> Result<(String, usize), String> {
    let mut i = start + 1;
    let mut s = String::new();
    while i < c.len() {
        match c[i] {
            '"' => return Ok((s, i + 1)),
            // Copy an interpolation `#{ ... }` verbatim; its inner quotes and
            // braces must not terminate the string. Parsed later.
            '#' if c.get(i + 1) == Some(&'{') => i = copy_interpolation(c, i, &mut s),
            '\\' => i = push_escape(c, i, &mut s)?,
            other => {
                s.push(other);
                i += 1;
            }
        }
    }
    Err("unterminated string".into())
}

// Copies `#{ ... }` (including delimiters) into `s`, returning the index after
// it. Tracks brace depth and skips nested strings so their braces don't count.
fn copy_interpolation(c: &[char], start: usize, s: &mut String) -> usize {
    s.push('#');
    s.push('{');
    let mut i = start + 2;
    let mut depth = 1;
    while i < c.len() && depth > 0 {
        match c[i] {
            '{' => depth += 1,
            '}' => depth -= 1,
            '"' => {
                i = copy_nested_string(c, i, s);
                continue;
            }
            _ => {}
        }
        s.push(c[i]);
        i += 1;
    }
    i
}

// Copies a `"..."` inside an interpolation verbatim, returning the next index.
fn copy_nested_string(c: &[char], start: usize, s: &mut String) -> usize {
    s.push('"');
    let mut i = start + 1;
    while i < c.len() && c[i] != '"' {
        if c[i] == '\\' {
            s.push(c[i]);
            i += 1;
        }
        if i < c.len() {
            s.push(c[i]);
            i += 1;
        }
    }
    if i < c.len() {
        s.push('"');
        i += 1;
    }
    i
}

fn push_escape(c: &[char], at: usize, s: &mut String) -> Result<usize, String> {
    let i = at + 1;
    let ch = match c.get(i) {
        Some('n') => '\n',
        Some('t') => '\t',
        Some('"') => '"',
        Some('\\') => '\\',
        Some('#') => '#',
        Some(o) => *o,
        None => return Err("unterminated string escape".into()),
    };
    s.push(ch);
    Ok(i + 1)
}

fn lex_atom(c: &[char], start: usize) -> Result<(String, usize), String> {
    if peek(c, start + 1) == Some('"') {
        return lex_string(c, start + 1);
    }
    Ok(read_word(c, start + 1))
}

fn lex_number(c: &[char], start: usize) -> (Tok, usize) {
    let mut i = scan_digits(c, start);
    if float_dot(c, i) {
        i = scan_digits(c, i + 1);
        (Tok::Float(number(c, start, i)), i)
    } else {
        (Tok::Int(number(c, start, i) as i64), i)
    }
}

fn scan_digits(c: &[char], mut i: usize) -> usize {
    while i < c.len() && (c[i].is_ascii_digit() || c[i] == '_') {
        i += 1;
    }
    i
}

fn float_dot(c: &[char], i: usize) -> bool {
    peek(c, i) == Some('.') && matches!(peek(c, i + 1), Some(d) if d.is_ascii_digit())
}

fn number(c: &[char], start: usize, end: usize) -> f64 {
    let s: String = c[start..end].iter().filter(|ch| **ch != '_').collect();
    s.parse().unwrap_or(0.0)
}

fn read_word(c: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    while i < c.len() && is_ident_part(c[i]) {
        i += 1;
    }
    if matches!(peek(c, i), Some('?') | Some('!')) {
        i += 1;
    }
    (c[start..i].iter().collect(), i)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_part(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn classify(word: String) -> Tok {
    match word.as_str() {
        "defmodule" => Tok::Defmodule,
        "def" => Tok::Def,
        "defp" => Tok::Defp,
        "do" => Tok::Do,
        "end" => Tok::End,
        "fn" => Tok::Fn,
        "when" => Tok::When,
        "case" => Tok::Case,
        "cond" => Tok::Cond,
        "if" => Tok::If,
        "unless" => Tok::Unless,
        "else" => Tok::Else,
        "with" => Tok::With,
        "receive" => Tok::Receive,
        "after" => Tok::After,
        "for" => Tok::For,
        "and" => Tok::And,
        "or" => Tok::Or,
        "not" => Tok::Not,
        "true" => Tok::True,
        "false" => Tok::False,
        "nil" => Tok::Nil,
        _ if word.chars().next().map(|ch| ch.is_ascii_uppercase()).unwrap_or(false) => {
            Tok::Alias(word)
        }
        _ => Tok::Ident(word),
    }
}
