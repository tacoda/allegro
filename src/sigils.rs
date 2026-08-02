// Sigils `~x<content>mods`. Lexing (delimiter scan) and expansion into an
// expression live together here. Supported: `~s`/`~S` (strings, `~s`
// interpolates) and `~w`/`~W` (word lists; `a` modifier yields atoms).

use crate::ast::{Expr, StrPart};
use crate::token::Tok;

// Lex a sigil starting at `~` (index `start`), returning the token and the
// index just past it.
pub fn lex(c: &[char], start: usize) -> Result<(Tok, usize), String> {
    let letter = c[start + 1];
    let open = *c.get(start + 2).ok_or("sigil is missing a delimiter")?;
    let close = closing(open).ok_or_else(|| format!("invalid sigil delimiter '{}'", open))?;
    let (content, mut i) = read_body(c, start + 3, open, close)?;
    let mut modifiers = String::new();
    while i < c.len() && c[i].is_ascii_alphabetic() {
        modifiers.push(c[i]);
        i += 1;
    }
    Ok((Tok::Sigil(letter, content, modifiers), i))
}

// Read up to the matching close delimiter (paired delimiters nest); returns the
// raw content and the index past the close.
fn read_body(c: &[char], from: usize, open: char, close: char) -> Result<(String, usize), String> {
    let paired = open != close;
    let mut depth: i32 = 1;
    let mut i = from;
    let mut content = String::new();
    while i < c.len() {
        depth += depth_delta(c[i], open, close, paired);
        if depth == 0 {
            return Ok((content, i + 1));
        }
        content.push(c[i]);
        i += 1;
    }
    Err("unterminated sigil".into())
}

fn depth_delta(ch: char, open: char, close: char, paired: bool) -> i32 {
    if paired && ch == open {
        1
    } else if ch == close {
        -1
    } else {
        0
    }
}

fn closing(open: char) -> Option<char> {
    Some(match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        '/' | '|' | '"' | '\'' => open,
        _ => return None,
    })
}

// Expand a lexed sigil into an expression.
pub fn expand(letter: char, content: &str, mods: &str) -> Result<Expr, String> {
    match letter {
        's' => Ok(Expr::Str(crate::parser::parse_interpolation(content)?)),
        'S' => Ok(Expr::Str(vec![StrPart::Lit(content.to_string())])),
        'w' | 'W' => Ok(Expr::List(words(content, mods.contains('a')))),
        other => Err(format!("unsupported sigil ~{}", other)),
    }
}

fn words(content: &str, atoms: bool) -> Vec<Expr> {
    content
        .split_whitespace()
        .map(|word| {
            if atoms {
                Expr::Atom(word.to_string())
            } else {
                Expr::Str(vec![StrPart::Lit(word.to_string())])
            }
        })
        .collect()
}
