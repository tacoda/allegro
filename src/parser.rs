use crate::ast::{BinOp, Expr, Pattern, Stmt, UnOp};
use crate::token::Tok;

// Type names usable as `when` patterns.
const TYPE_NAMES: &[&str] = &[
    "Nil", "Bool", "Number", "String", "Array", "Hash", "Model", "Agent", "Rule", "Skill", "Hook",
    "Command", "Graph", "Factory", "Charter", "Harness", "Class", "Instance", "Message",
    "HookResult", "Function",
];

pub struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

pub fn parse(toks: Vec<Tok>) -> Result<Vec<Stmt>, String> {
    let mut p = Parser { toks, pos: 0 };
    p.program()
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos]
    }

    fn advance(&mut self) -> Tok {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
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

    fn program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !self.check(&Tok::Eof) {
            stmts.push(self.statement()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    // Parse a block until one of the terminators is seen (terminator not consumed).
    fn block(&mut self, terminators: &[Tok]) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !terminators.contains(self.peek()) && !self.check(&Tok::Eof) {
            stmts.push(self.statement()?);
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn statement(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Tok::If => self.if_stmt(),
            Tok::Match => self.match_stmt(),
            Tok::While => self.while_stmt(),
            Tok::For => self.for_stmt(),
            Tok::Def => self.def_stmt(),
            Tok::Class => self.class_stmt(),
            Tok::Return => self.return_stmt(),
            _ => Ok(Stmt::Expr(self.expr_statement()?)),
        }
    }

    // Expression statement, with Ruby-style paren-less command calls:
    //   puts "hi"      -> puts("hi")
    //   agent.run x    -> agent.run(x)
    fn expr_statement(&mut self) -> Result<Expr, String> {
        let e = self.expr()?;
        if !self.starts_argument() {
            return Ok(e);
        }
        let args = self.command_args()?;
        match e {
            Expr::Ident(_) => Ok(Expr::Call(Box::new(e), args)),
            Expr::Method(recv, name, existing) if existing.is_empty() => {
                Ok(Expr::Method(recv, name, args))
            }
            _ => Err("unexpected argument after expression".into()),
        }
    }

    fn command_args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = vec![self.expr()?];
        while self.check(&Tok::Comma) {
            self.advance();
            self.skip_newlines();
            args.push(self.expr()?);
        }
        Ok(args)
    }

    // True when the next token can begin an argument to a paren-less call.
    fn starts_argument(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Num(_)
                | Tok::Str(_)
                | Tok::Ident(_)
                | Tok::True
                | Tok::False
                | Tok::Nil
                | Tok::LBracket
                | Tok::Bang
        )
    }

    fn if_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::If)?;
        let cond = self.expr()?;
        let then = self.block(&[Tok::Elsif, Tok::Else, Tok::End])?;
        let mut elifs = Vec::new();
        while self.check(&Tok::Elsif) {
            self.advance();
            let c = self.expr()?;
            let b = self.block(&[Tok::Elsif, Tok::Else, Tok::End])?;
            elifs.push((c, b));
        }
        let els = if self.check(&Tok::Else) {
            self.advance();
            Some(self.block(&[Tok::End])?)
        } else {
            None
        };
        self.eat(&Tok::End)?;
        Ok(Stmt::If {
            cond,
            then,
            elifs,
            els,
        })
    }

    fn match_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::Match)?;
        let subject = self.expr()?;
        self.skip_newlines();
        let mut arms = Vec::new();
        while self.check(&Tok::When) {
            self.advance();
            let pat = self.pattern()?;
            let body = self.block(&[Tok::When, Tok::Else, Tok::End])?;
            arms.push((pat, body));
        }
        let els = if self.check(&Tok::Else) {
            self.advance();
            Some(self.block(&[Tok::End])?)
        } else {
            None
        };
        self.eat(&Tok::End)?;
        Ok(Stmt::Match {
            subject,
            arms,
            els,
        })
    }

    fn pattern(&mut self) -> Result<Pattern, String> {
        match self.peek().clone() {
            Tok::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            Tok::Ident(name) if TYPE_NAMES.contains(&name.as_str()) => {
                self.advance();
                Ok(Pattern::Type(name))
            }
            Tok::Ident(name) => {
                self.advance();
                Ok(Pattern::Bind(name))
            }
            _ => Ok(Pattern::Value(self.expr()?)),
        }
    }

    fn while_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::While)?;
        let cond = self.expr()?;
        let body = self.block(&[Tok::End])?;
        self.eat(&Tok::End)?;
        Ok(Stmt::While(cond, body))
    }

    fn for_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::For)?;
        let var = self.ident_name()?;
        self.eat(&Tok::In)?;
        let iter = self.expr()?;
        let body = self.block(&[Tok::End])?;
        self.eat(&Tok::End)?;
        Ok(Stmt::For(var, iter, body))
    }

    fn def_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::Def)?;
        let name = self.ident_name()?;
        let params = self.param_list()?;
        let body = self.block(&[Tok::End])?;
        self.eat(&Tok::End)?;
        Ok(Stmt::Def(name, params, body))
    }

    // class Name < base
    //   def method(params) ... end
    // end
    fn class_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::Class)?;
        let name = self.ident_name()?;
        self.eat(&Tok::Lt)?;
        let base = self.ident_name()?;
        let mut methods = Vec::new();
        self.skip_newlines();
        while self.check(&Tok::Def) {
            self.advance();
            let mname = self.ident_name()?;
            let params = self.param_list()?;
            let body = self.block(&[Tok::End])?;
            self.eat(&Tok::End)?;
            methods.push((mname, params, body));
            self.skip_newlines();
        }
        self.eat(&Tok::End)?;
        Ok(Stmt::Class {
            name,
            base,
            methods,
        })
    }

    // Anonymous function used as a value: def (params) ... end
    fn lambda_expr(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::Def)?;
        let params = self.param_list()?;
        let body = self.block(&[Tok::End])?;
        self.eat(&Tok::End)?;
        Ok(Expr::Func(params, body))
    }

    fn param_list(&mut self) -> Result<Vec<String>, String> {
        let mut params = Vec::new();
        if self.check(&Tok::LParen) {
            self.advance();
            self.skip_newlines();
            while !self.check(&Tok::RParen) {
                params.push(self.ident_name()?);
                self.skip_newlines();
                if self.check(&Tok::Comma) {
                    self.advance();
                    self.skip_newlines();
                }
            }
            self.eat(&Tok::RParen)?;
        }
        Ok(params)
    }

    fn return_stmt(&mut self) -> Result<Stmt, String> {
        self.eat(&Tok::Return)?;
        if self.check(&Tok::Newline) || self.check(&Tok::Eof) || self.check(&Tok::End) {
            Ok(Stmt::Return(None))
        } else {
            Ok(Stmt::Return(Some(self.expr()?)))
        }
    }

    fn ident_name(&mut self) -> Result<String, String> {
        match self.advance() {
            Tok::Ident(s) => Ok(s),
            other => Err(format!("expected identifier, found {:?}", other)),
        }
    }

    // ---- expressions ----

    fn expr(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, String> {
        let left = self.or_expr()?;
        if self.check(&Tok::Assign) {
            self.advance();
            let value = self.assignment()?;
            match &left {
                Expr::Ident(_) | Expr::IVar(_) | Expr::Index(_, _) => {
                    Ok(Expr::Assign(Box::new(left), Box::new(value)))
                }
                _ => Err("invalid assignment target".into()),
            }
        } else {
            Ok(left)
        }
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.and_expr()?;
        while self.check(&Tok::OrOr) || self.check(&Tok::Or) {
            self.advance();
            let right = self.and_expr()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.equality()?;
        while self.check(&Tok::AndAnd) || self.check(&Tok::And) {
            self.advance();
            let right = self.equality()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn equality(&mut self) -> Result<Expr, String> {
        let mut left = self.comparison()?;
        loop {
            let op = match self.peek() {
                Tok::Eq => BinOp::Eq,
                Tok::Neq => BinOp::Neq,
                _ => break,
            };
            self.advance();
            let right = self.comparison()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.term()?;
        loop {
            let op = match self.peek() {
                Tok::Lt => BinOp::Lt,
                Tok::Gt => BinOp::Gt,
                Tok::Le => BinOp::Le,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.term()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut left = self.factor()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.factor()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Tok::Minus => {
                self.advance();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)))
            }
            Tok::Bang | Tok::Not => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.advance();
                    let name = self.ident_name()?;
                    let args = if self.check(&Tok::LParen) {
                        self.arg_list()?
                    } else {
                        Vec::new()
                    };
                    e = Expr::Method(Box::new(e), name, args);
                }
                Tok::LParen => {
                    let args = self.arg_list()?;
                    e = Expr::Call(Box::new(e), args);
                }
                // `name { ... }` is a call with a single config hash, so
                // constructors read naturally: rule { name: "x", text: "y" }.
                Tok::LBrace => {
                    let hash = self.hash_literal()?;
                    e = Expr::Call(Box::new(e), vec![hash]);
                }
                Tok::LBracket => {
                    self.advance();
                    self.skip_newlines();
                    let idx = self.expr()?;
                    self.skip_newlines();
                    self.eat(&Tok::RBracket)?;
                    e = Expr::Index(Box::new(e), Box::new(idx));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn arg_list(&mut self) -> Result<Vec<Expr>, String> {
        self.eat(&Tok::LParen)?;
        self.skip_newlines();
        let mut args = Vec::new();
        while !self.check(&Tok::RParen) {
            args.push(self.expr()?);
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RParen)?;
        Ok(args)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Tok::Num(n) => {
                self.advance();
                Ok(Expr::Num(n))
            }
            Tok::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
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
            Tok::Ident(name) => {
                self.advance();
                Ok(Expr::Ident(name))
            }
            Tok::IVar(name) => {
                self.advance();
                Ok(Expr::IVar(name))
            }
            Tok::LParen => {
                self.advance();
                self.skip_newlines();
                let e = self.expr()?;
                self.skip_newlines();
                self.eat(&Tok::RParen)?;
                Ok(e)
            }
            Tok::LBracket => self.array_literal(),
            Tok::LBrace => self.hash_literal(),
            Tok::Def => self.lambda_expr(),
            other => Err(format!("unexpected token {:?}", other)),
        }
    }

    fn array_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::LBracket)?;
        self.skip_newlines();
        let mut items = Vec::new();
        while !self.check(&Tok::RBracket) {
            items.push(self.expr()?);
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RBracket)?;
        Ok(Expr::Array(items))
    }

    fn hash_literal(&mut self) -> Result<Expr, String> {
        self.eat(&Tok::LBrace)?;
        self.skip_newlines();
        let mut pairs = Vec::new();
        while !self.check(&Tok::RBrace) {
            let key = self.hash_key()?;
            self.eat(&Tok::Colon)?;
            self.skip_newlines();
            let val = self.expr()?;
            pairs.push((key, val));
            self.skip_newlines();
            if self.check(&Tok::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.eat(&Tok::RBrace)?;
        Ok(Expr::Hash(pairs))
    }

    fn hash_key(&mut self) -> Result<String, String> {
        match self.advance() {
            Tok::Ident(s) => Ok(s),
            Tok::Str(s) => Ok(s),
            other => Err(format!("expected hash key, found {:?}", other)),
        }
    }

}
