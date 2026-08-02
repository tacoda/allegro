use crate::ast::{BinOp, Def, Expr, FnClause, Pattern, StrPart, TopItem, UnOp};
use crate::token::Tok;

pub fn parse(toks: Vec<Tok>) -> Result<Vec<TopItem>, String> {
    let mut p = Parser::new(toks);
    p.program()
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Parser {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        self.toks.get(self.pos).unwrap_or(&Tok::Eof)
    }

    fn peek_at(&self, k: usize) -> &Tok {
        self.toks.get(self.pos + k).unwrap_or(&Tok::Eof)
    }

    fn advance(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn check(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn eat(&mut self, t: &Tok) -> Result<(), String> {
        if self.check(t) {
            self.advance();
            Ok(())
        } else {
            Err(format!("expected {:?}, found {:?}", t, self.peek()))
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&Tok::Newline) {
            self.advance();
        }
    }

    // ---- top level ----

    fn program(&mut self) -> Result<Vec<TopItem>, String> {
        let mut items = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::Eof) {
            if self.check(&Tok::Defmodule) {
                items.push(self.module()?);
            } else {
                items.push(TopItem::Expr(self.expr()?));
            }
            self.skip_newlines();
        }
        Ok(items)
    }

    fn module(&mut self) -> Result<TopItem, String> {
        self.eat(&Tok::Defmodule)?;
        let name = self.module_path()?;
        self.eat(&Tok::Do)?;
        self.skip_newlines();
        let mut defs = Vec::new();
        while self.check(&Tok::Def) || self.check(&Tok::Defp) {
            defs.push(self.def()?);
            self.skip_newlines();
        }
        self.eat(&Tok::End)?;
        Ok(TopItem::Module { name, defs })
    }

    // `Alias(.Alias)*`
    fn module_path(&mut self) -> Result<String, String> {
        let mut path = self.alias_name()?;
        while self.check(&Tok::Dot) && matches!(self.peek_at(1), Tok::Alias(_)) {
            self.advance(); // .
            path.push('.');
            path.push_str(&self.alias_name()?);
        }
        Ok(path)
    }

    fn alias_name(&mut self) -> Result<String, String> {
        match self.advance() {
            Tok::Alias(a) => Ok(a),
            other => Err(format!("expected a module alias, found {:?}", other)),
        }
    }

    fn ident_name(&mut self) -> Result<String, String> {
        match self.advance() {
            Tok::Ident(s) => Ok(s),
            other => Err(format!("expected an identifier, found {:?}", other)),
        }
    }

    fn def(&mut self) -> Result<Def, String> {
        let private = self.check(&Tok::Defp);
        self.advance(); // def / defp
        let name = self.ident_name()?;
        let params = if self.check(&Tok::LParen) {
            self.pattern_params()?
        } else {
            Vec::new()
        };
        let guard = if self.check(&Tok::When) {
            self.advance();
            Some(self.expr()?)
        } else {
            None
        };
        let body = self.def_body()?;
        Ok(Def {
            name,
            params,
            guard,
            body,
            private,
        })
    }

    // Either `do ... end` or `, do: expr`.
    fn def_body(&mut self) -> Result<Vec<Expr>, String> {
        if self.check(&Tok::Comma) {
            self.advance();
            self.expect_kwkey("do")?;
            Ok(vec![self.expr()?])
        } else {
            self.eat(&Tok::Do)?;
            let body = self.block(&[Tok::End])?;
            self.eat(&Tok::End)?;
            Ok(body)
        }
    }

    fn expect_kwkey(&mut self, key: &str) -> Result<(), String> {
        match self.advance() {
            Tok::KwKey(k) if k == key => Ok(()),
            other => Err(format!("expected `{}:`, found {:?}", key, other)),
        }
    }

    // Parameters are patterns: parse as expressions, convert to patterns.
    fn pattern_params(&mut self) -> Result<Vec<Pattern>, String> {
        self.eat(&Tok::LParen)?;
        self.skip_newlines();
        let mut params = Vec::new();
        while !self.check(&Tok::RParen) {
            let e = self.expr()?;
            params.push(expr_to_pattern(e)?);
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RParen)?;
        Ok(params)
    }

    // A block: newline/`;`-separated expressions until a terminator.
    fn block(&mut self, terminators: &[Tok]) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        self.skip_newlines();
        while !terminators.contains(self.peek()) && !self.check(&Tok::Eof) {
            out.push(self.expr()?);
            self.skip_newlines();
        }
        Ok(out)
    }

    // ---- expressions (precedence climbing) ----

    fn expr(&mut self) -> Result<Expr, String> {
        self.match_expr()
    }

    fn match_expr(&mut self) -> Result<Expr, String> {
        let left = self.pipe_expr()?;
        if self.check(&Tok::Match) {
            self.advance();
            let right = self.match_expr()?; // right associative
            Ok(Expr::Match(Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn pipe_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.or_expr()?;
        while self.check(&Tok::Pipe) {
            self.advance();
            self.skip_newlines();
            let rhs = self.or_expr()?;
            left = pipe_into(left, rhs)?;
        }
        Ok(left)
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.and_expr()?;
        while self.check(&Tok::Or) || self.check(&Tok::OrOr) {
            self.advance();
            let right = self.and_expr()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.cmp_expr()?;
        while self.check(&Tok::And) || self.check(&Tok::AndAnd) {
            self.advance();
            let right = self.cmp_expr()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn cmp_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.concat_expr()?;
        loop {
            let op = match self.peek() {
                Tok::EqEq => BinOp::Eq,
                Tok::NotEq => BinOp::Neq,
                Tok::Lt => BinOp::Lt,
                Tok::Gt => BinOp::Gt,
                Tok::Le => BinOp::Le,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.concat_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn concat_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.add_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Concat => BinOp::Concat,
                Tok::ListConcat => BinOp::ListConcat,
                Tok::ListDiff => BinOp::ListDiff,
                _ => break,
            };
            self.advance();
            let right = self.add_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn add_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.mul_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn mul_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.unary_expr()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary_expr(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Tok::Minus => {
                self.advance();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary_expr()?)))
            }
            Tok::Not | Tok::Bang => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary_expr()?)))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        // Field access on a value: `value.field` (lowercase field).
        while self.check(&Tok::Dot) && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.advance(); // .
            let field = self.ident_name()?;
            if self.check(&Tok::LParen) {
                return Err("calling a method on a value is not supported; use Module.fun(value)".into());
            }
            e = Expr::Field(Box::new(e), field);
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.advance();
                Ok(Expr::Int(n))
            }
            Tok::Float(f) => {
                self.advance();
                Ok(Expr::Float(f))
            }
            Tok::Atom(a) => {
                self.advance();
                Ok(Expr::Atom(a))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Tok::Nil => {
                self.advance();
                Ok(Expr::Nil)
            }
            Tok::Str(raw) => {
                self.advance();
                Ok(Expr::Str(parse_interpolation(&raw)?))
            }
            Tok::Ident(name) => {
                self.advance();
                if self.check(&Tok::LParen) {
                    let args = self.arg_list()?;
                    Ok(Expr::LocalCall(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Tok::Alias(_) => self.alias_primary(),
            Tok::LParen => {
                self.advance();
                self.skip_newlines();
                let e = self.expr()?;
                self.skip_newlines();
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Tok::LBracket => self.list_literal(),
            Tok::LBrace => self.tuple_literal(),
            Tok::MapOpen => self.map_literal(),
            Tok::Fn => self.fn_literal(),
            Tok::If => self.if_expr(),
            other => Err(format!("unexpected token {:?}", other)),
        }
    }

    // A capitalized alias: a module path, then optionally `.fun(args)`.
    fn alias_primary(&mut self) -> Result<Expr, String> {
        let path = self.module_path()?;
        if self.check(&Tok::Dot) && matches!(self.peek_at(1), Tok::Ident(_)) {
            self.advance(); // .
            let fun = self.ident_name()?;
            let args = if self.check(&Tok::LParen) {
                self.arg_list()?
            } else {
                Vec::new()
            };
            Ok(Expr::RemoteCall(path, fun, args))
        } else {
            Ok(Expr::ModuleRef(path))
        }
    }

    // Positional args, then trailing `key: value` folded into one keyword list.
    fn arg_list(&mut self) -> Result<Vec<Expr>, String> {
        self.eat(&Tok::LParen)?;
        self.skip_newlines();
        let mut args = Vec::new();
        let mut kw = Vec::new();
        while !self.check(&Tok::RParen) {
            if let Tok::KwKey(k) = self.peek().clone() {
                self.advance();
                self.skip_newlines();
                let v = self.expr()?;
                kw.push(Expr::Tuple(vec![Expr::Atom(k), v]));
            } else {
                args.push(self.expr()?);
            }
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RParen)?;
        if !kw.is_empty() {
            args.push(Expr::List(kw));
        }
        Ok(args)
    }

    fn list_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::LBracket)?;
        self.skip_newlines();
        if self.check(&Tok::RBracket) {
            self.advance();
            return Ok(Expr::List(Vec::new()));
        }
        // keyword-list form: `[key: v, ...]`
        if matches!(self.peek(), Tok::KwKey(_)) {
            let pairs = self.keyword_pairs(&Tok::RBracket)?;
            self.eat(&Tok::RBracket)?;
            return Ok(Expr::List(pairs));
        }
        let head = self.expr()?;
        if self.check(&Tok::Bar) {
            self.advance();
            let tail = self.expr()?;
            self.skip_newlines();
            self.eat(&Tok::RBracket)?;
            return Ok(Expr::Cons(Box::new(head), Box::new(tail)));
        }
        let mut items = vec![head];
        self.skip_newlines();
        while self.check(&Tok::Comma) {
            self.advance();
            self.skip_newlines();
            if self.check(&Tok::RBracket) {
                break;
            }
            items.push(self.expr()?);
            self.skip_newlines();
        }
        self.eat(&Tok::RBracket)?;
        Ok(Expr::List(items))
    }

    fn tuple_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::LBrace)?;
        self.skip_newlines();
        let mut items = Vec::new();
        while !self.check(&Tok::RBrace) {
            items.push(self.expr()?);
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RBrace)?;
        Ok(Expr::Tuple(items))
    }

    fn map_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::MapOpen)?;
        self.skip_newlines();
        let mut pairs = Vec::new();
        while !self.check(&Tok::RBrace) {
            if let Tok::KwKey(k) = self.peek().clone() {
                self.advance();
                self.skip_newlines();
                let v = self.expr()?;
                pairs.push((Expr::Atom(k), v));
            } else {
                let key = self.expr()?;
                self.eat(&Tok::FatArrow)?;
                self.skip_newlines();
                let v = self.expr()?;
                pairs.push((key, v));
            }
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RBrace)?;
        Ok(Expr::Map(pairs))
    }

    // `key: v, key2: v2` — used inside a keyword list literal.
    fn keyword_pairs(&mut self, end: &Tok) -> Result<Vec<Expr>, String> {
        let mut pairs = Vec::new();
        while !self.check(end) {
            let key = match self.advance() {
                Tok::KwKey(k) => k,
                other => return Err(format!("expected `key:`, found {:?}", other)),
            };
            self.skip_newlines();
            let v = self.expr()?;
            pairs.push(Expr::Tuple(vec![Expr::Atom(key), v]));
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        Ok(pairs)
    }

    fn fn_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Fn)?;
        let mut clauses = Vec::new();
        loop {
            self.skip_newlines();
            let params = self.fn_params()?;
            let guard = if self.check(&Tok::When) {
                self.advance();
                Some(self.expr()?)
            } else {
                None
            };
            self.eat(&Tok::Arrow)?;
            let body = self.block(&[Tok::End, Tok::Fn])?;
            clauses.push(FnClause { params, guard, body });
            self.skip_newlines();
            if self.check(&Tok::End) {
                break;
            }
        }
        self.eat(&Tok::End)?;
        Ok(Expr::Fn(clauses))
    }

    // fn params: `a, b ->` (no surrounding parens in Elixir).
    fn fn_params(&mut self) -> Result<Vec<Pattern>, String> {
        let mut params = Vec::new();
        while !self.check(&Tok::Arrow) && !self.check(&Tok::When) {
            let e = self.expr()?;
            params.push(expr_to_pattern(e)?);
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                break;
            }
        }
        Ok(params)
    }

    fn if_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::If)?;
        let cond = self.expr()?;
        // keyword form: `if c, do: x, else: y`
        if self.check(&Tok::Comma) {
            self.advance();
            self.expect_kwkey("do")?;
            let then = self.expr()?;
            let els = if self.check(&Tok::Comma) {
                self.advance();
                self.expect_kwkey("else")?;
                Some(vec![self.expr()?])
            } else {
                None
            };
            return Ok(Expr::If(Box::new(cond), vec![then], els));
        }
        self.eat(&Tok::Do)?;
        let then = self.block(&[Tok::Else, Tok::End])?;
        let els = if self.check(&Tok::Else) {
            self.advance();
            Some(self.block(&[Tok::End])?)
        } else {
            None
        };
        self.eat(&Tok::End)?;
        Ok(Expr::If(Box::new(cond), then, els))
    }
}

// `left |> f(args)` becomes `f(left, args...)`.
fn pipe_into(left: Expr, rhs: Expr) -> Result<Expr, String> {
    match rhs {
        Expr::LocalCall(name, mut args) => {
            args.insert(0, left);
            Ok(Expr::LocalCall(name, args))
        }
        Expr::RemoteCall(m, f, mut args) => {
            args.insert(0, left);
            Ok(Expr::RemoteCall(m, f, args))
        }
        Expr::Var(name) => Ok(Expr::LocalCall(name, vec![left])),
        Expr::ModuleRef(_) => Err("cannot pipe into a bare module".into()),
        _ => Err("the right side of |> must be a function call".into()),
    }
}

// Convert an expression used in pattern position into a Pattern.
pub fn expr_to_pattern(e: Expr) -> Result<Pattern, String> {
    Ok(match e {
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

// Split a raw string into literal chunks and `#{ expr }` interpolations.
fn parse_interpolation(raw: &str) -> Result<Vec<StrPart>, String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' && chars.get(i + 1) == Some(&'{') {
            if !lit.is_empty() {
                parts.push(StrPart::Lit(std::mem::take(&mut lit)));
            }
            let (inner, next) = scan_interpolation(&chars, i + 2)?;
            parts.push(StrPart::Expr(parse_embedded(&inner)?));
            i = next;
        } else {
            lit.push(chars[i]);
            i += 1;
        }
    }
    if !lit.is_empty() {
        parts.push(StrPart::Lit(lit));
    }
    Ok(parts)
}

// Reads the body of a `#{ ... }` starting just after `#{`, returning the inner
// source and the index past the closing `}`.
fn scan_interpolation(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let mut depth = 1;
    let mut j = start;
    let mut inner = String::new();
    while j < chars.len() && depth > 0 {
        match chars[j] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        inner.push(chars[j]);
        j += 1;
    }
    if depth != 0 {
        return Err("unterminated #{ } in string".into());
    }
    Ok((inner, j + 1))
}

// Parse a single embedded expression (an interpolation body).
fn parse_embedded(src: &str) -> Result<Expr, String> {
    let toks = crate::lexer::lex(src)?;
    let mut p = Parser::new(toks);
    p.skip_newlines();
    let e = p.expr()?;
    p.skip_newlines();
    if !p.check(&Tok::Eof) {
        return Err("unexpected tokens in interpolation".into());
    }
    Ok(e)
}
