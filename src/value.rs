use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::FnClause;

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Atom(String),
    Bool(bool),
    Nil,
    Str(String),
    List(Rc<Vec<Value>>),
    Tuple(Rc<Vec<Value>>),
    Map(Rc<Vec<(Value, Value)>>), // insertion order; keys deduped on build
    Fun(Rc<Fun>),
}

// An anonymous function: clauses tried in order, closing over its definition env.
pub struct Fun {
    pub clauses: Vec<FnClause>,
    pub closure: Env,
}

impl Value {
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "integer",
            Value::Float(_) => "float",
            Value::Atom(_) => "atom",
            Value::Bool(_) => "boolean",
            Value::Nil => "nil",
            Value::Str(_) => "string",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Map(_) => "map",
            Value::Fun(_) => "function",
        }
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(items))
    }

    pub fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(Rc::new(items))
    }

    // Look up a map key by structural equality.
    pub fn map_get(pairs: &[(Value, Value)], key: &Value) -> Option<Value> {
        pairs
            .iter()
            .find(|(k, _)| values_equal(k, key))
            .map(|(_, v)| v.clone())
    }

    // `inspect`-style rendering, used for collections and IO.inspect.
    pub fn inspect(&self) -> String {
        match self {
            Value::Str(s) => format!("{:?}", s),
            Value::Atom(a) => format!(":{}", a),
            Value::List(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.inspect()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Tuple(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.inspect()).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Map(pairs) => inspect_map(pairs),
            other => other.to_string(),
        }
    }
}

// `to_string`/interpolation rendering: scalars render bare, collections inspect.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(x) => write!(f, "{}", format_float(*x)),
            Value::Atom(a) => write!(f, "{}", a),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Nil => write!(f, ""),
            Value::Str(s) => write!(f, "{}", s),
            Value::List(_) | Value::Tuple(_) | Value::Map(_) => write!(f, "{}", self.inspect()),
            Value::Fun(_) => write!(f, "#Function"),
        }
    }
}

// A struct (a map tagged with `:__struct__`) renders as `%Name{...}`; a plain
// map as `%{...}`.
fn inspect_map(pairs: &[(Value, Value)]) -> String {
    let struct_name = pairs.iter().find_map(|(k, v)| match (k, v) {
        (Value::Atom(a), Value::Atom(name)) if a == "__struct__" => Some(name.clone()),
        _ => None,
    });
    let inner: Vec<String> = pairs
        .iter()
        .filter(|(k, _)| !matches!(k, Value::Atom(a) if a == "__struct__"))
        .map(|(k, v)| match k {
            Value::Atom(a) => format!("{}: {}", a, v.inspect()),
            other => format!("{} => {}", other.inspect(), v.inspect()),
        })
        .collect();
    match struct_name {
        Some(name) => format!("%{}{{{}}}", name, inner.join(", ")),
        None => format!("%{{{}}}", inner.join(", ")),
    }
}

fn format_float(x: f64) -> String {
    if x == x.trunc() && x.is_finite() {
        format!("{:.1}", x) // 2.0 not 2
    } else {
        format!("{}", x)
    }
}

pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64) == *y,
        (Value::Atom(x), Value::Atom(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Nil, Value::Nil) => true,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::List(x), Value::List(y)) | (Value::Tuple(x), Value::Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| values_equal(p, q))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| Value::map_get(y, k).map_or(false, |w| values_equal(v, &w)))
        }
        _ => false,
    }
}

// ---- environment ----

pub type Env = Rc<RefCell<Scope>>;

pub struct Scope {
    vars: HashMap<String, Value>,
    parent: Option<Env>,
}

pub fn new_env(parent: Option<Env>) -> Env {
    Rc::new(RefCell::new(Scope {
        vars: HashMap::new(),
        parent,
    }))
}

impl Scope {
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.vars.get(name) {
            Some(v.clone())
        } else if let Some(p) = &self.parent {
            p.borrow().get(name)
        } else {
            None
        }
    }

    pub fn define(&mut self, name: &str, val: Value) {
        self.vars.insert(name.to_string(), val);
    }
}
