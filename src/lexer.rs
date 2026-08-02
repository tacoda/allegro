use crate::token::Tok;

pub fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '\n' => {
                out.push(Tok::Newline);
                i += 1;
            }
            ';' => {
                out.push(Tok::Newline);
                i += 1;
            }
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '"' => {
                let (s, ni) = lex_string(&chars, i)?;
                out.push(Tok::Str(s));
                i = ni;
            }
            '(' => push(&mut out, Tok::LParen, &mut i),
            ')' => push(&mut out, Tok::RParen, &mut i),
            '{' => push(&mut out, Tok::LBrace, &mut i),
            '}' => push(&mut out, Tok::RBrace, &mut i),
            '[' => push(&mut out, Tok::LBracket, &mut i),
            ']' => push(&mut out, Tok::RBracket, &mut i),
            ',' => push(&mut out, Tok::Comma, &mut i),
            '.' => push(&mut out, Tok::Dot, &mut i),
            ':' => push(&mut out, Tok::Colon, &mut i),
            '+' => push(&mut out, Tok::Plus, &mut i),
            '-' => push(&mut out, Tok::Minus, &mut i),
            '*' => push(&mut out, Tok::Star, &mut i),
            '/' => push(&mut out, Tok::Slash, &mut i),
            '%' => push(&mut out, Tok::Percent, &mut i),
            '=' => two(&chars, &mut i, &mut out, '=', Tok::Eq, Tok::Assign),
            '!' => two(&chars, &mut i, &mut out, '=', Tok::Neq, Tok::Bang),
            '<' => two(&chars, &mut i, &mut out, '=', Tok::Le, Tok::Lt),
            '>' => two(&chars, &mut i, &mut out, '=', Tok::Ge, Tok::Gt),
            '|' if peek(&chars, i + 1) == Some('|') => {
                out.push(Tok::OrOr);
                i += 2;
            }
            '&' if peek(&chars, i + 1) == Some('&') => {
                out.push(Tok::AndAnd);
                i += 2;
            }
            '@' => {
                let (word, ni) = lex_ident(&chars, i + 1);
                out.push(Tok::IVar(word));
                i = ni;
            }
            _ if c.is_ascii_digit() => {
                let (n, ni) = lex_number(&chars, i);
                out.push(Tok::Num(n));
                i = ni;
            }
            _ if is_ident_start(c) => {
                let (word, ni) = lex_ident(&chars, i);
                out.push(keyword_or_ident(word));
                i = ni;
            }
            _ => return Err(format!("unexpected character '{}'", c)),
        }
    }
    out.push(Tok::Eof);
    Ok(out)
}

fn push(out: &mut Vec<Tok>, t: Tok, i: &mut usize) {
    out.push(t);
    *i += 1;
}

fn two(chars: &[char], i: &mut usize, out: &mut Vec<Tok>, next: char, both: Tok, single: Tok) {
    if peek(chars, *i + 1) == Some(next) {
        out.push(both);
        *i += 2;
    } else {
        out.push(single);
        *i += 1;
    }
}

fn peek(chars: &[char], i: usize) -> Option<char> {
    chars.get(i).copied()
}

fn lex_string(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let mut i = start + 1;
    let mut s = String::new();
    while i < chars.len() {
        match chars[i] {
            '"' => return Ok((s, i + 1)),
            '\\' => {
                i += 1;
                match chars.get(i) {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(other) => s.push(*other),
                    None => return Err("unterminated string escape".into()),
                }
                i += 1;
            }
            other => {
                s.push(other);
                i += 1;
            }
        }
    }
    Err("unterminated string".into())
}

fn lex_number(chars: &[char], start: usize) -> (f64, usize) {
    let mut i = start;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        i += 1;
    }
    let s: String = chars[start..i].iter().collect();
    (s.parse().unwrap_or(0.0), i)
}

fn lex_ident(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    while i < chars.len() && is_ident_part(chars[i]) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '?' || c == '!'
}

fn keyword_or_ident(word: String) -> Tok {
    match word.as_str() {
        "class" => Tok::Class,
        "module" => Tok::Module,
        "match" => Tok::Match,
        "when" => Tok::When,
        "if" => Tok::If,
        "elsif" => Tok::Elsif,
        "else" => Tok::Else,
        "end" => Tok::End,
        "while" => Tok::While,
        "for" => Tok::For,
        "in" => Tok::In,
        "def" => Tok::Def,
        "return" => Tok::Return,
        "true" => Tok::True,
        "false" => Tok::False,
        "nil" => Tok::Nil,
        "and" => Tok::And,
        "or" => Tok::Or,
        "not" => Tok::Not,
        _ => Tok::Ident(word),
    }
}
