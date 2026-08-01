#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Nil,
    Ident(String),
    IVar(String), // @field on the current instance
    Array(Vec<Expr>),
    Hash(Vec<(String, Expr)>),
    Index(Box<Expr>, Box<Expr>),
    // callee(args)
    Call(Box<Expr>, Vec<Expr>),
    // recv.method(args) — args empty means property access
    Method(Box<Expr>, String, Vec<Expr>),
    // anonymous function: def (params) ... end
    Func(Vec<String>, Vec<Stmt>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

// A `when` pattern in a `match`.
#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,          // _
    Type(String),      // a type name: String, Number, Message, Agent, ...
    Bind(String),      // a bare identifier: binds the subject to that name
    Value(Expr),       // any expression: matches by equality
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    If {
        cond: Expr,
        then: Vec<Stmt>,
        elifs: Vec<(Expr, Vec<Stmt>)>,
        els: Option<Vec<Stmt>>,
    },
    Match {
        subject: Expr,
        arms: Vec<(Pattern, Vec<Stmt>)>,
        els: Option<Vec<Stmt>>,
    },
    While(Expr, Vec<Stmt>),
    For(String, Expr, Vec<Stmt>),
    Def(String, Vec<String>, Vec<Stmt>),
    // class Name < base ... end  (methods are (name, params, body))
    Class {
        name: String,
        base: String,
        methods: Vec<(String, Vec<String>, Vec<Stmt>)>,
    },
    Return(Option<Expr>),
}
