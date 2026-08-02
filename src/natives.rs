// Native standard-library functions that need no interpreter state: arithmetic
// and comparison operators, and the pure parts of the Kernel/IO/Enum/String/
// Map/List/Integer/Process modules. Stateful or user-callback-driven functions
// (Enum.map, Agent.run, the process primitives) stay in the interpreter.

use std::rc::Rc;

use crate::ast::{BinOp, Pattern};
use crate::value::{values_equal, Value};

// ---- numeric helpers ----

pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

pub fn arith(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    if let (Value::Int(a), Value::Int(b)) = (&l, &r) {
        return Ok(Value::Int(int_arith(op, *a, *b)));
    }
    match (num(&l), num(&r)) {
        (Some(a), Some(b)) => Ok(Value::Float(float_arith(op, a, b))),
        _ => Err(format!(
            "arithmetic on {} and {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

fn int_arith(op: BinOp, a: i64, b: i64) -> i64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        _ => unreachable!(),
    }
}

fn float_arith(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        _ => unreachable!(),
    }
}

// `<>` on strings, `++`/`--` on lists.
pub fn collection_op(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    match (op, l, r) {
        (BinOp::Concat, Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
        (BinOp::Concat, _, _) => Err("`<>` expects strings".into()),
        (BinOp::ListConcat, Value::List(a), Value::List(b)) => {
            let mut v = (**a).clone();
            v.extend(b.iter().cloned());
            Ok(Value::list(v))
        }
        (BinOp::ListConcat, _, _) => Err("`++` expects lists".into()),
        (BinOp::ListDiff, Value::List(a), Value::List(b)) => {
            let kept: Vec<Value> = a
                .iter()
                .filter(|x| !b.iter().any(|y| values_equal(x, y)))
                .cloned()
                .collect();
            Ok(Value::list(kept))
        }
        _ => Err("`--` expects lists".into()),
    }
}

pub fn compare(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
    let ord = match (num(l), num(r)) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => match (l, r) {
            (Value::Str(a), Value::Str(b)) => Some(a.cmp(b)),
            (Value::Atom(a), Value::Atom(b)) => Some(a.cmp(b)),
            _ => None,
        },
    };
    let ord = ord.ok_or_else(|| {
        format!("cannot compare {} and {}", l.type_name(), r.type_name())
    })?;
    use std::cmp::Ordering::*;
    let result = match op {
        BinOp::Lt => ord == Less,
        BinOp::Gt => ord == Greater,
        BinOp::Le => ord != Greater,
        BinOp::Ge => ord != Less,
        _ => unreachable!(),
    };
    Ok(Value::Bool(result))
}

// Bracket access `base[key]`: map lookup, keyword-list lookup, or list index.
// Missing keys (and indexing `nil`) yield `nil`, matching Elixir's Access.
pub fn index(base: &Value, key: &Value) -> Result<Value, String> {
    match base {
        Value::Map(m) => Ok(Value::map_get(m, key).unwrap_or(Value::Nil)),
        Value::Nil => Ok(Value::Nil),
        Value::List(l) => Ok(index_list(l, key)),
        other => Err(format!("cannot index a {}", other.type_name())),
    }
}

fn index_list(l: &[Value], key: &Value) -> Value {
    // integer subscript, else keyword-list access (`[key: v]`)
    if let Value::Int(i) = key {
        return l.get(*i as usize).cloned().unwrap_or(Value::Nil);
    }
    l.iter()
        .find_map(|it| match it {
            Value::Tuple(t) if t.len() == 2 && values_equal(&t[0], key) => Some(t[1].clone()),
            _ => None,
        })
        .unwrap_or(Value::Nil)
}

// ---- native modules ----

pub fn io_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("puts", [v]) => {
            println!("{}", v);
            Ok(Value::Atom("ok".into()))
        }
        ("puts", []) => {
            println!();
            Ok(Value::Atom("ok".into()))
        }
        // inspect/debug print and return their input, so they drop into a pipe.
        ("inspect", [v]) => {
            println!("{}", v.inspect());
            Ok(v.clone())
        }
        ("debug", [v]) => {
            eprintln!("[debug] {}", v.inspect());
            Ok(v.clone())
        }
        ("write", [v]) => {
            print!("{}", v);
            Ok(Value::Atom("ok".into()))
        }
        _ => Err(format!("IO.{}/{} is undefined", fun, args.len())),
    }
}

pub fn is_kernel(name: &str) -> bool {
    matches!(
        name,
        "div" | "rem" | "to_string" | "length" | "hd" | "tl" | "elem" | "tuple_size"
            | "map_size" | "is_nil" | "is_integer" | "is_float" | "is_number" | "is_atom"
            | "is_boolean" | "is_list" | "is_map" | "is_tuple" | "is_binary" | "is_function"
            | "abs" | "not"
    )
}

pub fn kernel_call(name: &str, args: Vec<Value>) -> Result<Value, String> {
    match (name, args.as_slice()) {
        ("div", [Value::Int(a), Value::Int(b)]) => {
            if *b == 0 {
                Err("division by zero".into())
            } else {
                Ok(Value::Int(a / b))
            }
        }
        ("rem", [Value::Int(a), Value::Int(b)]) => {
            if *b == 0 {
                Err("division by zero".into())
            } else {
                Ok(Value::Int(a % b))
            }
        }
        ("to_string", [v]) => Ok(Value::Str(v.to_string())),
        ("length", [Value::List(l)]) => Ok(Value::Int(l.len() as i64)),
        ("hd", [Value::List(l)]) => l.first().cloned().ok_or_else(|| "hd of empty list".into()),
        ("tl", [Value::List(l)]) => {
            if l.is_empty() {
                Err("tl of empty list".into())
            } else {
                Ok(Value::list(l[1..].to_vec()))
            }
        }
        ("elem", [Value::Tuple(t), Value::Int(i)]) => t
            .get(*i as usize)
            .cloned()
            .ok_or_else(|| "elem index out of range".into()),
        ("tuple_size", [Value::Tuple(t)]) => Ok(Value::Int(t.len() as i64)),
        ("map_size", [Value::Map(m)]) => Ok(Value::Int(m.len() as i64)),
        ("abs", [Value::Int(n)]) => Ok(Value::Int(n.abs())),
        ("abs", [Value::Float(f)]) => Ok(Value::Float(f.abs())),
        ("not", [v]) => Ok(Value::Bool(!v.truthy())),
        (pred, [v]) if pred.starts_with("is_") => Ok(Value::Bool(type_pred(pred, v))),
        _ => Err(format!("Kernel.{}/{} is undefined", name, args.len())),
    }
}

// Literal / scalar patterns match by equality with the corresponding value.
pub fn match_scalar(pat: &Pattern, val: &Value) -> bool {
    match pat {
        Pattern::Int(n) => matches!(val, Value::Int(m) if m == n),
        Pattern::Float(f) => matches!(val, Value::Float(g) if g == f),
        Pattern::Atom(a) => matches!(val, Value::Atom(b) if b == a),
        Pattern::Bool(b) => matches!(val, Value::Bool(c) if c == b),
        Pattern::Nil => matches!(val, Value::Nil),
        Pattern::Str(s) => matches!(val, Value::Str(t) if t == s),
        _ => false,
    }
}

fn ok_tuple(v: Value) -> Value {
    Value::tuple(vec![Value::Atom("ok".into()), v])
}

pub fn process_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("sleep", [Value::Int(ms)]) => {
            std::thread::sleep(std::time::Duration::from_millis((*ms).max(0) as u64));
            Ok(Value::Atom("ok".into()))
        }
        _ => Err(format!("Process.{}/{} is undefined", fun, args.len())),
    }
}

// Ordering for sort/sort_by: numbers numerically, else by string form.
pub fn cmp_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.to_string().cmp(&b.to_string()),
    }
}

// Enum functions that don't call a user function.
pub fn enum_pure(fun: &str, args: &[Value]) -> Result<Value, String> {
    match (fun, args) {
        ("count", [Value::List(l)]) => Ok(Value::Int(l.len() as i64)),
        ("sum", [Value::List(l)]) => {
            let mut acc = Value::Int(0);
            for x in l.iter() {
                acc = arith(BinOp::Add, acc, x.clone())?;
            }
            Ok(acc)
        }
        ("join", [Value::List(l), Value::Str(sep)]) => Ok(Value::Str(join_list(l, sep))),
        ("join", [Value::List(l)]) => Ok(Value::Str(join_list(l, ""))),
        ("sort", [Value::List(l)]) => {
            let mut v = (**l).clone();
            v.sort_by(cmp_values);
            Ok(Value::list(v))
        }
        ("reverse", [Value::List(l)]) => {
            let mut v = (**l).clone();
            v.reverse();
            Ok(Value::list(v))
        }
        ("member?", [Value::List(l), x]) => {
            Ok(Value::Bool(l.iter().any(|y| values_equal(y, x))))
        }
        _ => Err(format!("Enum.{}/{} is undefined", fun, args.len())),
    }
}

fn join_list(l: &[Value], sep: &str) -> String {
    l.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(sep)
}

pub fn string_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("upcase", [Value::Str(s)]) => Ok(Value::Str(s.to_uppercase())),
        ("downcase", [Value::Str(s)]) => Ok(Value::Str(s.to_lowercase())),
        ("trim", [Value::Str(s)]) => Ok(Value::Str(s.trim().to_string())),
        ("length", [Value::Str(s)]) => Ok(Value::Int(s.chars().count() as i64)),
        ("reverse", [Value::Str(s)]) => Ok(Value::Str(s.chars().rev().collect())),
        ("split", [Value::Str(s), Value::Str(sep)]) => {
            Ok(Value::list(s.split(sep.as_str()).map(|p| Value::Str(p.to_string())).collect()))
        }
        ("split", [Value::Str(s)]) => {
            Ok(Value::list(s.split_whitespace().map(|p| Value::Str(p.to_string())).collect()))
        }
        ("contains?", [Value::Str(s), Value::Str(sub)]) => Ok(Value::Bool(s.contains(sub.as_str()))),
        ("starts_with?", [Value::Str(s), Value::Str(p)]) => Ok(Value::Bool(s.starts_with(p.as_str()))),
        ("ends_with?", [Value::Str(s), Value::Str(p)]) => Ok(Value::Bool(s.ends_with(p.as_str()))),
        ("replace", [Value::Str(s), Value::Str(a), Value::Str(b)]) => {
            Ok(Value::Str(s.replace(a.as_str(), b)))
        }
        ("to_string", [v]) => Ok(Value::Str(v.to_string())),
        _ => Err(format!("String.{}/{} is undefined", fun, args.len())),
    }
}

pub fn map_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("get", [Value::Map(m), k]) => Ok(Value::map_get(m, k).unwrap_or(Value::Nil)),
        ("get", [Value::Map(m), k, default]) => {
            Ok(Value::map_get(m, k).unwrap_or_else(|| default.clone()))
        }
        ("fetch", [Value::Map(m), k]) => Ok(match Value::map_get(m, k) {
            Some(v) => ok_tuple(v),
            None => Value::Atom("error".into()),
        }),
        ("put", [Value::Map(m), k, v]) => Ok(Value::Map(Rc::new(map_put(m, k.clone(), v.clone())))),
        ("delete", [Value::Map(m), k]) => {
            let kept = m.iter().filter(|(ek, _)| !values_equal(ek, k)).cloned().collect();
            Ok(Value::Map(Rc::new(kept)))
        }
        ("keys", [Value::Map(m)]) => Ok(Value::list(m.iter().map(|(k, _)| k.clone()).collect())),
        ("values", [Value::Map(m)]) => Ok(Value::list(m.iter().map(|(_, v)| v.clone()).collect())),
        ("has_key?", [Value::Map(m), k]) => Ok(Value::Bool(Value::map_get(m, k).is_some())),
        ("merge", [Value::Map(a), Value::Map(b)]) => {
            let mut out = (**a).clone();
            for (k, v) in b.iter() {
                out = map_put(&out, k.clone(), v.clone());
            }
            Ok(Value::Map(Rc::new(out)))
        }
        _ => Err(format!("Map.{}/{} is undefined", fun, args.len())),
    }
}

pub fn map_put(m: &[(Value, Value)], key: Value, val: Value) -> Vec<(Value, Value)> {
    let mut out: Vec<(Value, Value)> = m.to_vec();
    if let Some(slot) = out.iter_mut().find(|(k, _)| values_equal(k, &key)) {
        slot.1 = val;
    } else {
        out.push((key, val));
    }
    out
}

pub fn list_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("first", [Value::List(l)]) => Ok(l.first().cloned().unwrap_or(Value::Nil)),
        ("last", [Value::List(l)]) => Ok(l.last().cloned().unwrap_or(Value::Nil)),
        ("reverse", [Value::List(l)]) => {
            let mut v = (**l).clone();
            v.reverse();
            Ok(Value::list(v))
        }
        ("at", [Value::List(l), Value::Int(i)]) => {
            Ok(l.get(*i as usize).cloned().unwrap_or(Value::Nil))
        }
        ("member?", [Value::List(l), x]) => Ok(Value::Bool(l.iter().any(|y| values_equal(y, x)))),
        _ => Err(format!("List.{}/{} is undefined", fun, args.len())),
    }
}

pub fn integer_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("to_string", [Value::Int(n)]) => Ok(Value::Str(n.to_string())),
        ("parse", [Value::Str(s)]) => Ok(match s.trim().parse::<i64>() {
            Ok(n) => ok_tuple(Value::Int(n)),
            Err(_) => Value::Atom("error".into()),
        }),
        _ => Err(format!("Integer.{}/{} is undefined", fun, args.len())),
    }
}

fn type_pred(pred: &str, v: &Value) -> bool {
    match pred {
        "is_nil" => matches!(v, Value::Nil),
        "is_integer" => matches!(v, Value::Int(_)),
        "is_float" => matches!(v, Value::Float(_)),
        "is_number" => matches!(v, Value::Int(_) | Value::Float(_)),
        "is_atom" => matches!(v, Value::Atom(_) | Value::Bool(_) | Value::Nil),
        "is_boolean" => matches!(v, Value::Bool(_)),
        "is_list" => matches!(v, Value::List(_)),
        "is_map" => matches!(v, Value::Map(_)),
        "is_tuple" => matches!(v, Value::Tuple(_)),
        "is_binary" => matches!(v, Value::Str(_)),
        "is_function" => matches!(v, Value::Fun(_)),
        _ => false,
    }
}
