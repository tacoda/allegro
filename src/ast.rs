// Abstract syntax for the Elixir-flavored core.

#[derive(Debug, Clone)]
pub enum StrPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    Concat,     // <>
    ListConcat, // ++
    ListDiff,   // --
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Atom(String),
    Bool(bool),
    Nil,
    Str(Vec<StrPart>),
    Var(String),
    List(Vec<Expr>),
    Cons(Box<Expr>, Box<Expr>), // [head | tail]
    Tuple(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Block(Vec<Expr>),
    Match(Box<Expr>, Box<Expr>), // pattern = value
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    LocalCall(String, Vec<Expr>),          // foo(args)
    RemoteCall(String, String, Vec<Expr>), // Mod.fun(args)
    Field(Box<Expr>, String),              // value.field
    ModuleRef(String),                     // a bare module alias
    Fn(Vec<FnClause>),                     // fn ... end (phase 2 eval)
    If(Box<Expr>, Vec<Expr>, Option<Vec<Expr>>),
}

// A clause of an anonymous function (or, later, a multi-clause def).
#[derive(Debug, Clone)]
pub struct FnClause {
    pub params: Vec<Pattern>,
    pub guard: Option<Expr>,
    pub body: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Var(String),
    Int(i64),
    Float(f64),
    Atom(String),
    Bool(bool),
    Nil,
    Str(String),
    Tuple(Vec<Pattern>),
    List(Vec<Pattern>),
    Cons(Box<Pattern>, Box<Pattern>),
    Map(Vec<(Expr, Pattern)>), // key expression (evaluated) -> sub-pattern
}

#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub params: Vec<Pattern>,
    pub guard: Option<Expr>,
    pub body: Vec<Expr>,
    pub private: bool,
}

#[derive(Debug, Clone)]
pub enum TopItem {
    Module { name: String, defs: Vec<Def> },
    Expr(Expr),
}
