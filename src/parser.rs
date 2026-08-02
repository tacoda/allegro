use crate::ast::{
    BinOp, CaseClause, Def, Expr, FnClause, ForClause, Pattern, StrPart, TopItem, UnOp,
};
use crate::patterns::expr_to_pattern;
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
        let mut struct_fields = None;
        loop {
            match self.peek() {
                Tok::Def | Tok::Defp => defs.push(self.def()?),
                Tok::Ident(s) if s == "defstruct" => struct_fields = Some(self.defstruct()?),
                _ => break,
            }
            self.skip_newlines();
        }
        self.eat(&Tok::End)?;
        Ok(TopItem::Module {
            name,
            defs,
            struct_fields,
        })
    }

    // `defstruct [:a, :b]` or `defstruct [a: default, b: default]`.
    fn defstruct(&mut self) -> Result<crate::ast::StructFields, String> {
        self.advance(); // defstruct
        let Expr::List(items) = self.expr()? else {
            return Err("defstruct expects a list of fields".into());
        };
        items.into_iter().map(defstruct_field).collect()
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
        let (params, rest) = if self.check(&Tok::LParen) {
            self.pattern_params()?
        } else {
            (Vec::new(), None)
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
            rest,
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

    // Parameters are patterns; an optional trailing `*rest` collects the
    // remaining arguments into a list.
    fn pattern_params(&mut self) -> Result<(Vec<Pattern>, Option<String>), String> {
        self.eat(&Tok::LParen)?;
        self.skip_newlines();
        let mut params = Vec::new();
        let mut rest = None;
        while !self.check(&Tok::RParen) {
            if self.check(&Tok::Star) {
                self.advance();
                rest = Some(self.ident_name()?);
                self.skip_newlines();
                break;
            }
            let e = self.expr()?;
            params.push(expr_to_pattern(e)?);
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RParen)?;
        Ok((params, rest))
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
        loop {
            // bracket access `base[key]` — same line only (newline ends it)
            if self.check(&Tok::LBracket) {
                self.advance();
                let key = self.expr()?;
                self.eat(&Tok::RBracket)?;
                e = Expr::Index(Box::new(e), Box::new(key));
                continue;
            }
            if !self.check(&Tok::Dot) {
                break;
            }
            match self.peek_at(1) {
                // anonymous-function call: `f.(args)`
                Tok::LParen => {
                    self.advance(); // .
                    let args = self.arg_list()?;
                    e = Expr::AnonCall(Box::new(e), args);
                }
                // field access: `value.field`
                Tok::Ident(_) => {
                    self.advance(); // .
                    let field = self.ident_name()?;
                    e = Expr::Field(Box::new(e), field);
                }
                _ => break,
            }
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
            Tok::Percent => self.struct_literal(),
            Tok::Fn => self.fn_literal(),
            Tok::If => self.if_expr(),
            Tok::Unless => self.unless_expr(),
            Tok::Case => self.case_expr(),
            Tok::Cond => self.cond_expr(),
            Tok::With => self.with_expr(),
            Tok::Receive => self.receive_expr(),
            Tok::For => self.for_expr(),
            Tok::Amp => self.capture_expr(),
            Tok::Caret => {
                self.advance();
                Ok(Expr::Pin(self.ident_name()?))
            }
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
            items.push(self.list_item()?);
            self.skip_newlines();
        }
        self.eat(&Tok::RBracket)?;
        Ok(Expr::List(items))
    }

    // A list element: a normal expression, or a `key: value` keyword pair
    // (which becomes a `{:key, value}` tuple), so `[:a, k: v]` is allowed.
    fn list_item(&mut self) -> Result<Expr, String> {
        if let Tok::KwKey(k) = self.peek().clone() {
            self.advance();
            self.skip_newlines();
            let v = self.expr()?;
            Ok(Expr::Tuple(vec![Expr::Atom(k), v]))
        } else {
            self.expr()
        }
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
        // update form `%{base | k: v, ...}`: the base is an expression, never a
        // `k:` key, and is followed by `|`.
        if let Some(update) = self.try_update(&Tok::RBrace)? {
            return Ok(update);
        }
        let pairs = self.map_pairs(&Tok::RBrace)?;
        self.eat(&Tok::RBrace)?;
        Ok(Expr::Map(pairs))
    }

    // If the next tokens are `<expr> |`, parse a map/struct update and return it.
    fn try_update(&mut self, end: &Tok) -> Result<Option<Expr>, String> {
        if self.check(end) || matches!(self.peek(), Tok::KwKey(_)) {
            return Ok(None);
        }
        let base = self.expr()?;
        if !self.check(&Tok::Bar) {
            // not an update: `base` is the first `key => value` key.
            self.eat(&Tok::FatArrow)?;
            self.skip_newlines();
            let v = self.expr()?;
            self.eat_comma();
            let mut pairs = vec![(base, v)];
            pairs.extend(self.map_pairs(end)?);
            self.eat(end)?;
            return Ok(Some(Expr::Map(pairs)));
        }
        self.advance(); // |
        self.skip_newlines();
        let updates = self.map_pairs(end)?;
        self.eat(end)?;
        Ok(Some(Expr::MapUpdate(Box::new(base), updates)))
    }

    // `key => v` / `key: v` pairs until `end`.
    fn map_pairs(&mut self, end: &Tok) -> Result<Vec<(Expr, Expr)>, String> {
        let mut pairs = Vec::new();
        while !self.check(end) {
            if let Tok::KwKey(k) = self.peek().clone() {
                self.advance();
                self.skip_newlines();
                pairs.push((Expr::Atom(k), self.expr()?));
            } else {
                let key = self.expr()?;
                self.eat(&Tok::FatArrow)?;
                self.skip_newlines();
                pairs.push((key, self.expr()?));
            }
            self.skip_newlines();
            self.eat_comma();
        }
        Ok(pairs)
    }

    fn eat_comma(&mut self) {
        if self.check(&Tok::Comma) {
            self.advance();
            self.skip_newlines();
        }
    }

    // `%User{field: expr, ...}` — a struct literal, or `%User{base | field: v}`
    // — a struct update.
    fn struct_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Percent)?;
        let name = self.module_path()?;
        self.eat(&Tok::LBrace)?;
        self.skip_newlines();
        // update form `%User{base | field: v}` reuses map-update semantics; the
        // base already carries its `__struct__` tag.
        if let Some(update) = self.try_update(&Tok::RBrace)? {
            return Ok(update);
        }
        let mut fields = Vec::new();
        while !self.check(&Tok::RBrace) {
            match self.advance() {
                Tok::KwKey(k) => {
                    self.skip_newlines();
                    fields.push((k, self.expr()?));
                }
                other => return Err(format!("expected `field:`, found {:?}", other)),
            }
            self.skip_newlines();
            self.eat_comma();
        }
        self.eat(&Tok::RBrace)?;
        Ok(Expr::Struct(name, fields))
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

    fn unless_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Unless)?;
        let cond = self.expr()?;
        // `unless c` runs its body when c is falsy: negate and reuse `if`.
        let neg = Expr::Unary(UnOp::Not, Box::new(cond));
        if self.check(&Tok::Comma) {
            self.advance();
            self.expect_kwkey("do")?;
            let then = self.expr()?;
            return Ok(Expr::If(Box::new(neg), vec![then], None));
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
        Ok(Expr::If(Box::new(neg), then, els))
    }

    fn case_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Case)?;
        let subject = self.expr()?;
        self.eat(&Tok::Do)?;
        let clauses = self.case_clauses()?;
        self.eat(&Tok::End)?;
        Ok(Expr::Case(Box::new(subject), clauses))
    }

    // `pattern [when guard] -> body` clauses, up to (not including) `end`.
    fn case_clauses(&mut self) -> Result<Vec<CaseClause>, String> {
        let mut clauses = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::End) {
            let pat = expr_to_pattern(self.expr()?)?;
            let guard = self.opt_guard()?;
            self.eat(&Tok::Arrow)?;
            let body = self.clause_body()?;
            clauses.push(CaseClause { pat, guard, body });
            self.skip_newlines();
        }
        Ok(clauses)
    }

    // `receive do <clauses> [after <ms> -> <body>] end`. The timeout expr is
    // parsed but ignored: the scheduler is single-threaded, so once it is idle
    // no further message can arrive — idle *is* the timeout.
    fn receive_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Receive)?;
        self.eat(&Tok::Do)?;
        let mut clauses = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::End) && !self.check(&Tok::After) {
            let pat = expr_to_pattern(self.expr()?)?;
            let guard = self.opt_guard()?;
            self.eat(&Tok::Arrow)?;
            let body = self.clause_body()?;
            clauses.push(CaseClause { pat, guard, body });
            self.skip_newlines();
        }
        let after = if self.check(&Tok::After) {
            self.advance();
            let _ms = self.expr()?; // timeout value, ignored (see above)
            self.eat(&Tok::Arrow)?;
            Some(self.clause_body()?)
        } else {
            None
        };
        self.eat(&Tok::End)?;
        Ok(Expr::Receive(clauses, after))
    }

    // `for <clause>, <clause>, ... do body end` or `for <clause>, do: expr`.
    // A clause is a generator `pat <- enumerable` or a boolean filter.
    fn for_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::For)?;
        let mut clauses = vec![self.for_clause()?];
        while self.check(&Tok::Comma) {
            self.advance();
            if matches!(self.peek(), Tok::KwKey(k) if k == "do") {
                break; // the trailing `, do:` keyword body
            }
            clauses.push(self.for_clause()?);
        }
        let body = if matches!(self.peek(), Tok::KwKey(k) if k == "do") {
            self.expect_kwkey("do")?;
            vec![self.expr()?]
        } else {
            self.eat(&Tok::Do)?;
            let b = self.block(&[Tok::End])?;
            self.eat(&Tok::End)?;
            b
        };
        Ok(Expr::For(clauses, body))
    }

    fn for_clause(&mut self) -> Result<ForClause, String> {
        let lhs = self.expr()?;
        if self.check(&Tok::LArrow) {
            self.advance();
            let src = self.expr()?;
            Ok(ForClause::Gen(expr_to_pattern(lhs)?, src))
        } else {
            Ok(ForClause::Filter(lhs))
        }
    }

    fn cond_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Cond)?;
        self.eat(&Tok::Do)?;
        let mut clauses = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::End) {
            let cond = self.expr()?;
            self.eat(&Tok::Arrow)?;
            let body = self.clause_body()?;
            clauses.push((cond, body));
            self.skip_newlines();
        }
        self.eat(&Tok::End)?;
        Ok(Expr::Cond(clauses))
    }

    fn with_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::With)?;
        let mut clauses = Vec::new();
        loop {
            let pat = expr_to_pattern(self.expr()?)?;
            self.eat(&Tok::LArrow)?;
            let src = self.expr()?;
            clauses.push((pat, src));
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            } else {
                break;
            }
        }
        self.eat(&Tok::Do)?;
        let body = self.block(&[Tok::Else, Tok::End])?;
        let els = if self.check(&Tok::Else) {
            self.advance();
            Some(self.case_clauses()?)
        } else {
            None
        };
        self.eat(&Tok::End)?;
        Ok(Expr::With(clauses, body, els))
    }

    fn opt_guard(&mut self) -> Result<Option<Expr>, String> {
        if self.check(&Tok::When) {
            self.advance();
            Ok(Some(self.expr()?))
        } else {
            Ok(None)
        }
    }

    // A clause body runs until `end` or the start of the next `... ->` clause.
    fn clause_body(&mut self) -> Result<Vec<Expr>, String> {
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::End) && !self.starts_new_clause() {
            body.push(self.expr()?);
            self.skip_newlines();
        }
        Ok(body)
    }

    // True if the current line is a clause head — a top-level `->` appears
    // before the line ends (nested blocks/brackets don't count).
    fn starts_new_clause(&self) -> bool {
        let mut i = self.pos;
        let mut depth = 0i32;
        loop {
            match self.toks.get(i).unwrap_or(&Tok::Eof) {
                Tok::Eof => return false,
                Tok::Newline if depth == 0 => return false,
                Tok::Arrow if depth == 0 => return true,
                Tok::LParen | Tok::LBracket | Tok::LBrace | Tok::MapOpen | Tok::Do | Tok::Fn => {
                    depth += 1
                }
                Tok::RParen | Tok::RBracket | Tok::RBrace | Tok::End => depth -= 1,
                _ => {}
            }
            i += 1;
        }
    }

    // `&fun/arity`, `&Mod.fun/arity`, or `&(expr with &1 &2 ...)`.
    fn capture_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Amp)?;
        // a bare slot `&1` inside a capture body
        if let Tok::Int(n) = self.peek().clone() {
            self.advance();
            return Ok(Expr::CaptureSlot(n as usize));
        }
        // function capture: `&name/arity` or `&Mod.fun/arity`
        if self.is_function_capture() {
            return self.function_capture();
        }
        // expression capture: `&(...)` — wrap in a fn over the slots used
        let body = self.unary_expr()?;
        let arity = max_slot(&body);
        let params = (1..=arity).map(|i| Pattern::Var(slot_name(i))).collect();
        Ok(Expr::Fn(vec![FnClause {
            params,
            guard: None,
            body: vec![body],
        }]))
    }

    fn is_function_capture(&self) -> bool {
        match self.peek() {
            Tok::Ident(_) => self.peek_at(1) == &Tok::Slash,
            Tok::Alias(_) => true,
            _ => false,
        }
    }

    fn function_capture(&mut self) -> Result<Expr, String> {
        let (path, fun) = if matches!(self.peek(), Tok::Alias(_)) {
            let path = self.module_path()?;
            self.eat(&Tok::Dot)?;
            (Some(path), self.ident_name()?)
        } else {
            (None, self.ident_name()?)
        };
        self.eat(&Tok::Slash)?;
        let arity = self.int_lit()?;
        let args: Vec<Expr> = (1..=arity).map(|i| Expr::Var(slot_name(i))).collect();
        let call = match path {
            Some(p) => Expr::RemoteCall(p, fun, args),
            None => Expr::LocalCall(fun, args),
        };
        let params = (1..=arity).map(|i| Pattern::Var(slot_name(i))).collect();
        Ok(Expr::Fn(vec![FnClause {
            params,
            guard: None,
            body: vec![call],
        }]))
    }

    fn int_lit(&mut self) -> Result<usize, String> {
        match self.advance() {
            Tok::Int(n) => Ok(n as usize),
            other => Err(format!("expected an integer, found {:?}", other)),
        }
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

// One `defstruct` field: `:atom` (no default) or `name: default`.
fn defstruct_field(it: Expr) -> Result<(String, Option<Expr>), String> {
    match it {
        Expr::Atom(a) => Ok((a, None)),
        Expr::Tuple(mut pair) if pair.len() == 2 => {
            let default = pair.pop().unwrap();
            match pair.pop().unwrap() {
                Expr::Atom(k) => Ok((k, Some(default))),
                _ => Err("defstruct field name must be an atom".into()),
            }
        }
        _ => Err("defstruct fields must be `:atom` or `name: default`".into()),
    }
}

fn slot_name(i: usize) -> String {
    format!("$c{}", i)
}

// Highest capture slot `&n` used in an expression — the arity of `&(...)`.
fn max_slot(e: &Expr) -> usize {
    match e {
        Expr::CaptureSlot(n) => *n,
        Expr::Unary(_, x) => max_slot(x),
        Expr::Field(x, _) => max_slot(x),
        Expr::Binary(_, l, r) => max_slot(l).max(max_slot(r)),
        Expr::Match(l, r) => max_slot(l).max(max_slot(r)),
        Expr::Cons(h, t) => max_slot(h).max(max_slot(t)),
        Expr::Tuple(xs) | Expr::List(xs) => xs.iter().map(max_slot).max().unwrap_or(0),
        Expr::LocalCall(_, xs) => xs.iter().map(max_slot).max().unwrap_or(0),
        Expr::RemoteCall(_, _, xs) => xs.iter().map(max_slot).max().unwrap_or(0),
        Expr::AnonCall(f, xs) => max_slot(f).max(xs.iter().map(max_slot).max().unwrap_or(0)),
        Expr::Map(ps) => ps
            .iter()
            .map(|(k, v)| max_slot(k).max(max_slot(v)))
            .max()
            .unwrap_or(0),
        _ => 0,
    }
}

// Convert an expression used in pattern position into a Pattern.
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
