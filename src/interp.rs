use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BinOp, CaseClause, Def, Expr, Pattern, StrPart, TopItem, UnOp};
use crate::parser::expr_to_pattern;
use crate::value::{new_env, values_equal, Env, Fun, Value};

pub struct Interp {
    // module name -> (function name, arity) -> clauses (tried in order)
    modules: HashMap<String, HashMap<(String, usize), Vec<Rc<Def>>>>,
    global: Env,
    // stack of the module whose body is executing, for local-call resolution
    current: Vec<String>,
}

impl Interp {
    pub fn new() -> Interp {
        Interp {
            modules: HashMap::new(),
            global: new_env(None),
            current: Vec::new(),
        }
    }

    pub fn run(&mut self, program: &[TopItem]) -> Result<(), String> {
        for item in program {
            if let TopItem::Module { name, defs } = item {
                self.register(name, defs);
            }
        }
        for item in program {
            if let TopItem::Expr(e) = item {
                let env = self.global.clone();
                self.eval(e, &env)?;
            }
        }
        Ok(())
    }

    fn register(&mut self, name: &str, defs: &[Def]) {
        let table = self.modules.entry(name.to_string()).or_default();
        for def in defs {
            table
                .entry((def.name.clone(), def.params.len()))
                .or_default()
                .push(Rc::new(def.clone()));
        }
    }

    // ---- evaluation ----

    fn eval(&mut self, e: &Expr, env: &Env) -> Result<Value, String> {
        match e {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Float(f) => Ok(Value::Float(*f)),
            Expr::Atom(a) => Ok(Value::Atom(a.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),
            Expr::Str(parts) => self.eval_string(parts, env),
            Expr::Var(name) => env
                .borrow()
                .get(name)
                .ok_or_else(|| format!("undefined variable '{}'", name)),
            Expr::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    out.push(self.eval(it, env)?);
                }
                Ok(Value::list(out))
            }
            Expr::Cons(h, t) => self.eval_cons(h, t, env),
            Expr::Tuple(items) => Ok(Value::tuple(self.eval_args(items, env)?)),
            Expr::Map(pairs) => self.eval_map(pairs, env),
            Expr::Block(stmts) => self.eval_block(stmts, env),
            Expr::Match(lhs, rhs) => self.eval_match(lhs, rhs, env),
            Expr::Binary(op, l, r) => self.eval_binary(*op, l, r, env),
            Expr::Unary(op, x) => self.eval_unary(*op, x, env),
            Expr::LocalCall(name, args) => {
                let argv = self.eval_args(args, env)?;
                self.local_call(name, argv)
            }
            Expr::RemoteCall(m, f, args) => {
                let argv = self.eval_args(args, env)?;
                self.remote_call(m, f, argv)
            }
            Expr::Field(base, field) => self.eval_field(base, field, env),
            Expr::ModuleRef(m) => Err(format!("module {} is not a value", m)),
            Expr::Fn(clauses) => Ok(Value::Fun(Rc::new(Fun {
                clauses: clauses.clone(),
                closure: env.clone(),
            }))),
            Expr::AnonCall(f, args) => self.eval_anon_call(f, args, env),
            Expr::CaptureSlot(n) => env
                .borrow()
                .get(&format!("$c{}", n))
                .ok_or_else(|| format!("capture slot &{} is unbound", n)),
            Expr::If(cond, then, els) => {
                if self.eval(cond, env)?.truthy() {
                    self.eval_block(then, env)
                } else if let Some(body) = els {
                    self.eval_block(body, env)
                } else {
                    Ok(Value::Nil)
                }
            }
            Expr::Case(subject, clauses) => {
                let v = self.eval(subject, env)?;
                self.eval_case(&v, clauses, env)
            }
            Expr::Cond(clauses) => self.eval_cond(clauses, env),
            Expr::With(clauses, body, els) => self.eval_with(clauses, body, els, env),
            Expr::Pin(_) => Err("^pin is only valid inside a pattern".into()),
        }
    }

    fn eval_anon_call(&mut self, f: &Expr, args: &[Expr], env: &Env) -> Result<Value, String> {
        let callee = self.eval(f, env)?;
        let argv = self.eval_args(args, env)?;
        match callee {
            Value::Fun(fun) => self.call_fun(&fun, argv),
            other => Err(format!("cannot call a {}", other.type_name())),
        }
    }

    // A guard passes when absent, or when it evaluates truthy.
    fn guard_ok(&mut self, guard: &Option<Expr>, env: &Env) -> Result<bool, String> {
        match guard {
            Some(g) => Ok(self.eval(g, env)?.truthy()),
            None => Ok(true),
        }
    }

    fn call_fun(&mut self, fun: &Rc<Fun>, args: Vec<Value>) -> Result<Value, String> {
        for clause in &fun.clauses {
            if clause.params.len() != args.len() {
                continue;
            }
            let call_env = new_env(Some(fun.closure.clone()));
            if !self.match_seq(&clause.params, &args, &call_env)? {
                continue;
            }
            if self.guard_ok(&clause.guard, &call_env)? {
                return self.eval_block(&clause.body, &call_env);
            }
        }
        Err("no function clause matching the given arguments".into())
    }

    fn eval_case(
        &mut self,
        v: &Value,
        clauses: &[CaseClause],
        env: &Env,
    ) -> Result<Value, String> {
        for clause in clauses {
            let arm_env = new_env(Some(env.clone()));
            if !self.match_pattern(&clause.pat, v, &arm_env)? {
                continue;
            }
            if self.guard_ok(&clause.guard, &arm_env)? {
                return self.eval_block(&clause.body, &arm_env);
            }
        }
        Err(format!("no case clause matching: {}", v.inspect()))
    }

    fn eval_cond(&mut self, clauses: &[(Expr, Vec<Expr>)], env: &Env) -> Result<Value, String> {
        for (cond, body) in clauses {
            if self.eval(cond, env)?.truthy() {
                return self.eval_block(body, env);
            }
        }
        Err("no cond clause was truthy".into())
    }

    fn eval_with(
        &mut self,
        clauses: &[(Pattern, Expr)],
        body: &[Expr],
        els: &Option<Vec<CaseClause>>,
        env: &Env,
    ) -> Result<Value, String> {
        let with_env = new_env(Some(env.clone()));
        for (pat, src) in clauses {
            let v = self.eval(src, &with_env)?;
            if !self.match_pattern(pat, &v, &with_env)? {
                // a non-match short-circuits: run `else`, or return the value
                return match els {
                    Some(arms) => self.eval_case(&v, arms, env),
                    None => Ok(v),
                };
            }
        }
        self.eval_block(body, &with_env)
    }

    fn eval_block(&mut self, stmts: &[Expr], env: &Env) -> Result<Value, String> {
        let mut last = Value::Nil;
        for s in stmts {
            last = self.eval(s, env)?;
        }
        Ok(last)
    }

    fn eval_args(&mut self, args: &[Expr], env: &Env) -> Result<Vec<Value>, String> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a, env)?);
        }
        Ok(out)
    }

    fn eval_cons(&mut self, h: &Expr, t: &Expr, env: &Env) -> Result<Value, String> {
        let head = self.eval(h, env)?;
        match self.eval(t, env)? {
            Value::List(items) => {
                let mut v = Vec::with_capacity(items.len() + 1);
                v.push(head);
                v.extend(items.iter().cloned());
                Ok(Value::list(v))
            }
            other => Err(format!("[h | t] tail must be a list, got {}", other.type_name())),
        }
    }

    fn eval_map(&mut self, pairs: &[(Expr, Expr)], env: &Env) -> Result<Value, String> {
        let mut out: Vec<(Value, Value)> = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            let key = self.eval(k, env)?;
            let val = self.eval(v, env)?;
            if let Some(slot) = out.iter_mut().find(|(ek, _)| values_equal(ek, &key)) {
                slot.1 = val;
            } else {
                out.push((key, val));
            }
        }
        Ok(Value::Map(Rc::new(out)))
    }

    fn eval_match(&mut self, lhs: &Expr, rhs: &Expr, env: &Env) -> Result<Value, String> {
        let val = self.eval(rhs, env)?;
        let pat = expr_to_pattern(lhs.clone())?;
        if self.match_pattern(&pat, &val, env)? {
            Ok(val)
        } else {
            Err(format!("no match of right-hand side value: {}", val.inspect()))
        }
    }

    fn eval_field(&mut self, base: &Expr, field: &str, env: &Env) -> Result<Value, String> {
        match self.eval(base, env)? {
            Value::Map(pairs) => Value::map_get(&pairs, &Value::Atom(field.to_string()))
                .ok_or_else(|| format!("key :{} not found in map", field)),
            other => Err(format!("cannot access field on a {}", other.type_name())),
        }
    }

    fn eval_string(&mut self, parts: &[StrPart], env: &Env) -> Result<Value, String> {
        let mut s = String::new();
        for part in parts {
            match part {
                StrPart::Lit(t) => s.push_str(t),
                StrPart::Expr(e) => s.push_str(&self.eval(e, env)?.to_string()),
            }
        }
        Ok(Value::Str(s))
    }

    fn eval_unary(&mut self, op: UnOp, x: &Expr, env: &Env) -> Result<Value, String> {
        let v = self.eval(x, env)?;
        match op {
            UnOp::Not => Ok(Value::Bool(!v.truthy())),
            UnOp::Neg => match v {
                Value::Int(n) => Ok(Value::Int(-n)),
                Value::Float(f) => Ok(Value::Float(-f)),
                other => Err(format!("cannot negate a {}", other.type_name())),
            },
        }
    }

    fn eval_binary(&mut self, op: BinOp, l: &Expr, r: &Expr, env: &Env) -> Result<Value, String> {
        // Short-circuiting boolean operators return the deciding operand.
        match op {
            BinOp::And => {
                let lv = self.eval(l, env)?;
                return if lv.truthy() { self.eval(r, env) } else { Ok(lv) };
            }
            BinOp::Or => {
                let lv = self.eval(l, env)?;
                return if lv.truthy() { Ok(lv) } else { self.eval(r, env) };
            }
            _ => {}
        }
        let lv = self.eval(l, env)?;
        let rv = self.eval(r, env)?;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => arith(op, lv, rv),
            BinOp::Div => match (num(&lv), num(&rv)) {
                (Some(_), Some(0.0)) => Err("division by zero".into()),
                (Some(a), Some(b)) => Ok(Value::Float(a / b)),
                _ => Err("`/` expects numbers".into()),
            },
            BinOp::Eq => Ok(Value::Bool(values_equal(&lv, &rv))),
            BinOp::Neq => Ok(Value::Bool(!values_equal(&lv, &rv))),
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => compare(op, &lv, &rv),
            BinOp::Concat | BinOp::ListConcat | BinOp::ListDiff => collection_op(op, &lv, &rv),
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    // ---- calls ----

    fn local_call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let arity = args.len();
        if let Some(module) = self.current.last().cloned() {
            if self.has_def(&module, name, arity) {
                return self.call_user(&module, name, args);
            }
        }
        if is_kernel(name) {
            return kernel_call(name, args);
        }
        Err(format!("undefined function {}/{}", name, arity))
    }

    fn remote_call(&mut self, module: &str, fun: &str, args: Vec<Value>) -> Result<Value, String> {
        match module {
            "IO" => io_call(fun, args),
            "Kernel" => kernel_call(fun, args),
            _ if self.modules.contains_key(module) => {
                if self.has_def(module, fun, args.len()) {
                    self.call_user(module, fun, args)
                } else {
                    Err(format!(
                        "function {}.{}/{} is undefined",
                        module,
                        fun,
                        args.len()
                    ))
                }
            }
            _ => Err(format!("module {} is not available", module)),
        }
    }

    fn has_def(&self, module: &str, name: &str, arity: usize) -> bool {
        self.modules
            .get(module)
            .and_then(|t| t.get(&(name.to_string(), arity)))
            .is_some()
    }

    fn call_user(&mut self, module: &str, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let clauses = self
            .modules
            .get(module)
            .and_then(|t| t.get(&(name.to_string(), args.len())))
            .cloned()
            .unwrap_or_default();
        for def in &clauses {
            let call_env = new_env(Some(self.global.clone()));
            if !self.match_seq(&def.params, &args, &call_env)? {
                continue;
            }
            if self.guard_ok(&def.guard, &call_env)? {
                self.current.push(module.to_string());
                let result = self.eval_block(&def.body, &call_env);
                self.current.pop();
                return result;
            }
        }
        Err(format!(
            "no function clause matching {}.{}/{}",
            module,
            name,
            args.len()
        ))
    }

    // ---- pattern matching ----

    fn match_pattern(&mut self, pat: &Pattern, val: &Value, env: &Env) -> Result<bool, String> {
        match pat {
            Pattern::Wildcard => Ok(true),
            Pattern::Var(name) => {
                env.borrow_mut().define(name, val.clone());
                Ok(true)
            }
            Pattern::Tuple(pats) => self.match_tuple(pats, val, env),
            Pattern::List(pats) => self.match_list(pats, val, env),
            Pattern::Cons(h, t) => self.match_cons(h, t, val, env),
            Pattern::Map(pairs) => self.match_map(pairs, val, env),
            Pattern::Pin(name) => {
                let pinned = env
                    .borrow()
                    .get(name)
                    .ok_or_else(|| format!("^{} is unbound", name))?;
                Ok(values_equal(&pinned, val))
            }
            scalar => Ok(match_scalar(scalar, val)),
        }
    }

    fn match_tuple(&mut self, pats: &[Pattern], val: &Value, env: &Env) -> Result<bool, String> {
        match val {
            Value::Tuple(items) if items.len() == pats.len() => self.match_seq(pats, items, env),
            _ => Ok(false),
        }
    }

    fn match_list(&mut self, pats: &[Pattern], val: &Value, env: &Env) -> Result<bool, String> {
        match val {
            Value::List(items) if items.len() == pats.len() => self.match_seq(pats, items, env),
            _ => Ok(false),
        }
    }

    fn match_cons(
        &mut self,
        h: &Pattern,
        t: &Pattern,
        val: &Value,
        env: &Env,
    ) -> Result<bool, String> {
        match val {
            Value::List(items) if !items.is_empty() => {
                let tail = Value::list(items[1..].to_vec());
                Ok(self.match_pattern(h, &items[0], env)? && self.match_pattern(t, &tail, env)?)
            }
            _ => Ok(false),
        }
    }

    fn match_map(
        &mut self,
        pairs: &[(Expr, Pattern)],
        val: &Value,
        env: &Env,
    ) -> Result<bool, String> {
        let Value::Map(mpairs) = val else {
            return Ok(false);
        };
        for (kexpr, subpat) in pairs {
            let key = self.eval(kexpr, env)?;
            match Value::map_get(mpairs, &key) {
                Some(v) if self.match_pattern(subpat, &v, env)? => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    fn match_seq(&mut self, pats: &[Pattern], vals: &[Value], env: &Env) -> Result<bool, String> {
        for (p, v) in pats.iter().zip(vals.iter()) {
            if !self.match_pattern(p, v, env)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

// ---- numeric helpers ----

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn arith(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
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
fn collection_op(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
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

fn compare(op: BinOp, l: &Value, r: &Value) -> Result<Value, String> {
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

// ---- native modules ----

fn io_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
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

fn is_kernel(name: &str) -> bool {
    matches!(
        name,
        "div" | "rem" | "to_string" | "length" | "hd" | "tl" | "elem" | "tuple_size"
            | "map_size" | "is_nil" | "is_integer" | "is_float" | "is_number" | "is_atom"
            | "is_boolean" | "is_list" | "is_map" | "is_tuple" | "is_binary" | "is_function"
            | "abs" | "not"
    )
}

fn kernel_call(name: &str, args: Vec<Value>) -> Result<Value, String> {
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
fn match_scalar(pat: &Pattern, val: &Value) -> bool {
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
