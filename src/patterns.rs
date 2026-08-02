// Convert an already-parsed expression into a match pattern. The parser reuses
// its expression grammar for pattern positions (function heads, `=`, `case`,
// `for` generators) and then reinterprets the result here.

use crate::ast::{Expr, Pattern, StrPart};

pub fn expr_to_pattern(e: Expr) -> Result<Pattern, String> {
    Ok(match e {
        Expr::Match(l, r) => {
            Pattern::And(Box::new(expr_to_pattern(*l)?), Box::new(expr_to_pattern(*r)?))
        }
        Expr::Pin(name) => Pattern::Pin(name),
        Expr::Var(name) if name == "_" || name.starts_with('_') => {
            if name == "_" {
                Pattern::Wildcard
            } else {
                Pattern::Var(name)
            }
        }
        Expr::Var(name) => Pattern::Var(name),
        Expr::Int(n) => Pattern::Int(n),
        Expr::Float(f) => Pattern::Float(f),
        Expr::Atom(a) => Pattern::Atom(a),
        Expr::Bool(b) => Pattern::Bool(b),
        Expr::Nil => Pattern::Nil,
        Expr::Str(parts) => match single_literal(&parts) {
            Some(s) => Pattern::Str(s),
            None => return Err("string interpolation is not allowed in a pattern".into()),
        },
        Expr::Tuple(items) => {
            Pattern::Tuple(items.into_iter().map(expr_to_pattern).collect::<Result<_, _>>()?)
        }
        Expr::List(items) => {
            Pattern::List(items.into_iter().map(expr_to_pattern).collect::<Result<_, _>>()?)
        }
        Expr::Cons(h, t) => {
            Pattern::Cons(Box::new(expr_to_pattern(*h)?), Box::new(expr_to_pattern(*t)?))
        }
        Expr::Map(pairs) => {
            let mut out = Vec::new();
            for (k, v) in pairs {
                out.push((k, expr_to_pattern(v)?));
            }
            Pattern::Map(out)
        }
        Expr::Struct(name, fields) => {
            let mut out = Vec::new();
            for (k, v) in fields {
                out.push((k, expr_to_pattern(v)?));
            }
            Pattern::Struct(name, out)
        }
        other => return Err(format!("invalid pattern: {:?}", other)),
    })
}

fn single_literal(parts: &[StrPart]) -> Option<String> {
    match parts {
        [] => Some(String::new()),
        [StrPart::Lit(s)] => Some(s.clone()),
        _ => None,
    }
}
