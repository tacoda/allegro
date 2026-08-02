use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{
    BinOp, CaseClause, Def, Expr, ForClause, Pattern, StrPart, StructFields, TopItem, UnOp,
};
use crate::natives::*;
use crate::patterns::expr_to_pattern;
use crate::scheduler::{Handler, HandlerRef, Pid, Scheduler};
use crate::value::{new_env, values_equal, Env, Fun, Value};

// The loop-invariant context threaded through a `for` comprehension: the
// clauses still to process, the body, and the current environment.
struct Frame<'a> {
    rest: &'a [ForClause],
    body: &'a [Expr],
    env: &'a Env,
}

pub struct Interp {
    // module name -> function name -> clauses (tried in order; arity handled per-clause)
    modules: HashMap<String, HashMap<String, Vec<Rc<Def>>>>,
    // module name -> its declared struct fields (from `defstruct`)
    structs: HashMap<String, StructFields>,
    global: Env,
    // stack of the module whose body is executing, for local-call resolution
    current: Vec<String>,
    // the actor scheduler (spawn/send/receive)
    sched: Scheduler,
}

impl Interp {
    pub fn new() -> Interp {
        Interp {
            modules: HashMap::new(),
            structs: HashMap::new(),
            global: new_env(None),
            current: Vec::new(),
            sched: Scheduler::new(),
        }
    }

    pub fn run(&mut self, program: &[TopItem]) -> Result<(), String> {
        self.load_prelude()?;
        self.register_modules(program);
        for item in program {
            if let TopItem::Expr(e) = item {
                let env = self.global.clone();
                self.eval(e, &env)?;
            }
        }
        // Drain any spawned processes that still have mail to handle.
        self.drain()?;
        Ok(())
    }

    fn register_modules(&mut self, program: &[TopItem]) {
        for item in program {
            if let TopItem::Module {
                name,
                defs,
                struct_fields,
            } = item
            {
                self.register(name, defs, struct_fields);
            }
        }
    }

    // The allegro-written standard library, compiled into the binary and
    // registered before user code so its modules are always available.
    fn load_prelude(&mut self) -> Result<(), String> {
        const PRELUDE: &str = include_str!("../std/prelude.al");
        let toks = crate::lexer::lex(PRELUDE)?;
        let program = crate::parser::parse(toks)?;
        self.register_modules(&program);
        Ok(())
    }

    fn register(&mut self, name: &str, defs: &[Def], struct_fields: &Option<StructFields>) {
        let table = self.modules.entry(name.to_string()).or_default();
        for def in defs {
            table.entry(def.name.clone()).or_default().push(Rc::new(def.clone()));
        }
        if let Some(fields) = struct_fields {
            self.structs.insert(name.to_string(), fields.clone());
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
            Expr::MapUpdate(base, updates) => self.eval_map_update(base, updates, env),
            Expr::Struct(name, fields) => self.eval_struct(name, fields, env),
            Expr::Index(base, key) => self.eval_index(base, key, env),
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
            // A module is a value — its name as an atom (Elixir semantics); this
            // is what lets `spawn(Counter, ...)` name a handler module.
            Expr::ModuleRef(m) => Ok(Value::Atom(m.clone())),
            Expr::Fn(clauses) => Ok(Value::Fun(Rc::new(Fun {
                clauses: clauses.clone(),
                closure: env.clone(),
                module: self.current.last().cloned(),
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
            Expr::Receive(clauses, after) => self.eval_receive(clauses, after, env),
            Expr::For(clauses, body) => self.eval_for(clauses, body, env),
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

    pub(crate) fn call_fun(&mut self, fun: &Rc<Fun>, args: Vec<Value>) -> Result<Value, String> {
        for clause in &fun.clauses {
            if clause.params.len() != args.len() {
                continue;
            }
            let call_env = new_env(Some(fun.closure.clone()));
            if !self.match_seq(&clause.params, &args, &call_env)? {
                continue;
            }
            if self.guard_ok(&clause.guard, &call_env)? {
                return self.eval_in_module(&fun.module, &clause.body, &call_env);
            }
        }
        Err("no function clause matching the given arguments".into())
    }

    // Evaluate a body with the given module pushed as the current one, so
    // unqualified calls resolve lexically.
    fn eval_in_module(
        &mut self,
        module: &Option<String>,
        body: &[Expr],
        env: &Env,
    ) -> Result<Value, String> {
        match module {
            Some(m) => {
                self.current.push(m.clone());
                let result = self.eval_block(body, env);
                self.current.pop();
                result
            }
            None => self.eval_block(body, env),
        }
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

    // A `for` comprehension: walk the generators (cartesian across several),
    // drop combinations rejected by a filter or a non-matching generator
    // pattern, and collect each body result into a list. Its bindings are
    // scoped to the comprehension and do not leak out.
    fn eval_for(&mut self, clauses: &[ForClause], body: &[Expr], env: &Env) -> Result<Value, String> {
        Ok(Value::list(self.for_walk(clauses, body, env)?))
    }

    fn for_walk(
        &mut self,
        clauses: &[ForClause],
        body: &[Expr],
        env: &Env,
    ) -> Result<Vec<Value>, String> {
        let Some((clause, rest)) = clauses.split_first() else {
            return Ok(vec![self.eval_block(body, env)?]);
        };
        let frame = Frame { rest, body, env };
        match clause {
            ForClause::Filter(cond) => self.for_filter(cond, frame),
            ForClause::Gen(pat, src) => self.for_gen(pat, src, frame),
        }
    }

    // A filter keeps the remaining clauses only when it evaluates truthy.
    fn for_filter(&mut self, cond: &Expr, f: Frame) -> Result<Vec<Value>, String> {
        if self.eval(cond, f.env)?.truthy() {
            self.for_walk(f.rest, f.body, f.env)
        } else {
            Ok(Vec::new())
        }
    }

    // A generator binds each enumerated item (skipping non-matches) and walks
    // the remaining clauses in that scope, concatenating the results.
    fn for_gen(&mut self, pat: &Pattern, src: &Expr, f: Frame) -> Result<Vec<Value>, String> {
        let coll = self.eval(src, f.env)?;
        let mut out = Vec::new();
        for item in self.enumerable(&coll)? {
            let scope = new_env(Some(f.env.clone()));
            if self.match_pattern(pat, &item, &scope)? {
                out.extend(self.for_walk(f.rest, f.body, &scope)?);
            }
        }
        Ok(out)
    }

    // The values a comprehension iterates: list elements, or `{key, value}`
    // tuples for a map.
    fn enumerable(&self, v: &Value) -> Result<Vec<Value>, String> {
        match v {
            Value::List(l) => Ok((**l).clone()),
            Value::Map(m) => Ok(m
                .iter()
                .map(|(k, val)| Value::tuple(vec![k.clone(), val.clone()]))
                .collect()),
            other => Err(format!("cannot enumerate a {}", other.type_name())),
        }
    }

    // ---- processes (actor scheduler) ----

    // `spawn(Module, init)` (dispatches `Module.handle/2`) or
    // `spawn(fn state, msg -> ... end, init)`. A module is passed as its atom.
    fn spawn_proc(&mut self, handler: &Value, state: Value) -> Result<Value, String> {
        let h = match handler {
            Value::Fun(f) => Handler::Fun(f.clone()),
            Value::Atom(name) if self.modules.contains_key(name) => Handler::Module(name.clone()),
            other => {
                return Err(format!(
                    "spawn expects a module or fn handler, got a {}",
                    other.type_name()
                ))
            }
        };
        Ok(Value::Pid(self.sched.spawn(h, state).id()))
    }

    // `receive` matches the current process's mailbox against the clauses. On a
    // miss it steps other ready actors and retries; when the scheduler is idle
    // (no message can ever arrive) it runs the `after` body, or deadlocks.
    fn eval_receive(
        &mut self,
        clauses: &[CaseClause],
        after: &Option<Vec<Expr>>,
        env: &Env,
    ) -> Result<Value, String> {
        let me = self.sched.current;
        loop {
            if let Some(value) = self.match_mailbox(clauses, me, env)? {
                return Ok(value);
            }
            match self.sched.next_ready() {
                Some((pid, m)) => self.step(pid, m)?,
                None => return self.receive_timeout(after, env),
            }
        }
    }

    // Scan the process's mailbox for the first message matching a clause; on a
    // hit, remove it and evaluate the clause body.
    fn match_mailbox(
        &mut self,
        clauses: &[CaseClause],
        me: Pid,
        env: &Env,
    ) -> Result<Option<Value>, String> {
        for (idx, msg) in self.sched.mailbox_snapshot(me).into_iter().enumerate() {
            for clause in clauses {
                let arm_env = new_env(Some(env.clone()));
                if self.match_pattern(&clause.pat, &msg, &arm_env)?
                    && self.guard_ok(&clause.guard, &arm_env)?
                {
                    self.sched.take_message(me, idx);
                    return Ok(Some(self.eval_block(&clause.body, &arm_env)?));
                }
            }
        }
        Ok(None)
    }

    fn receive_timeout(&mut self, after: &Option<Vec<Expr>>, env: &Env) -> Result<Value, String> {
        match after {
            Some(body) => self.eval_block(body, env),
            None => {
                Err("deadlock: receive with an empty mailbox and no runnable processes".into())
            }
        }
    }

    // Run all ready actors until the scheduler is idle.
    fn drain(&mut self) -> Result<(), String> {
        while let Some((pid, msg)) = self.sched.next_ready() {
            self.step(pid, msg)?;
        }
        Ok(())
    }

    // Deliver one message to a process: run its handler with the process's
    // current state. A handler that raises (returns Err) crashes the process
    // rather than aborting the whole program.
    fn step(&mut self, pid: Pid, msg: Value) -> Result<(), String> {
        let handler = match self.sched.handler_of(pid) {
            Some(h) => h,
            None => return Ok(()),
        };
        let state = self.sched.state_of(pid);
        let prev = self.sched.current;
        self.sched.current = pid;
        let result = match handler {
            HandlerRef::Root => Ok(Value::Nil),
            HandlerRef::Module(m) => self.remote_call(&m, "handle", vec![state, msg]),
            HandlerRef::Fun(f) => self.call_fun(&f, vec![state, msg]),
        };
        self.sched.current = prev;
        match result {
            Ok(ret) => self.apply_return(pid, ret),
            Err(reason) => self.terminate(pid, Value::Str(reason)),
        }
        Ok(())
    }

    // Interpret a handler's return: `{:noreply, s}` updates state,
    // `{:stop, reason[, s]}` terminates, anything else becomes the new state.
    fn apply_return(&mut self, pid: Pid, ret: Value) {
        if let Value::Tuple(t) = &ret {
            match t.as_slice() {
                [Value::Atom(a), s] if a == "noreply" => {
                    return self.sched.set_state(pid, s.clone());
                }
                [Value::Atom(a), reason, s] if a == "stop" => {
                    self.sched.set_state(pid, s.clone());
                    return self.terminate(pid, reason.clone());
                }
                [Value::Atom(a), reason] if a == "stop" => {
                    return self.terminate(pid, reason.clone());
                }
                _ => {}
            }
        }
        self.sched.set_state(pid, ret);
    }

    // Kill a process and notify its monitors with `{:DOWN, pid, reason}`.
    fn terminate(&mut self, pid: Pid, reason: Value) {
        for watcher in self.sched.kill(pid) {
            let down = Value::tuple(vec![
                Value::Atom("DOWN".into()),
                Value::Pid(pid.id()),
                reason.clone(),
            ]);
            self.sched.deliver(watcher, down);
        }
    }

    // `Process.register/2`, `Process.whereis/1` (the registry); `sleep/1` and
    // any other Process functions delegate to the plain native.
    fn process_module(&mut self, fun: &str, args: Vec<Value>) -> Result<Value, String> {
        match (fun, args.as_slice()) {
            ("register", [Value::Pid(pid), Value::Atom(name)]) => {
                self.sched.register(name.clone(), Pid(*pid));
                Ok(Value::Atom("ok".into()))
            }
            ("whereis", [Value::Atom(name)]) => Ok(self
                .sched
                .whereis(name)
                .map(|p| Value::Pid(p.id()))
                .unwrap_or(Value::Nil)),
            _ => process_call(fun, args),
        }
    }

    // `Store` — a mutable cell for state that must persist across calls and
    // process restarts (values are otherwise immutable).
    fn store_call(&mut self, fun: &str, args: Vec<Value>) -> Result<Value, String> {
        match (fun, args.as_slice()) {
            ("new", []) => Ok(Value::Ref(Rc::new(RefCell::new(Value::Nil)))),
            ("new", [init]) => Ok(Value::Ref(Rc::new(RefCell::new(init.clone())))),
            ("get", [Value::Ref(cell)]) => Ok(cell.borrow().clone()),
            ("put", [Value::Ref(cell), v]) => {
                *cell.borrow_mut() = v.clone();
                Ok(v.clone())
            }
            ("update", [Value::Ref(cell), Value::Fun(f)]) => {
                let current = cell.borrow().clone();
                let next = self.call_fun(f, vec![current])?;
                *cell.borrow_mut() = next.clone();
                Ok(next)
            }
            _ => Err(format!("Store.{}/{} is undefined", fun, args.len())),
        }
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

    // `%{base | k => v, ...}` / `%Struct{base | field: v}` — update an existing
    // map or struct; a struct's `__struct__` tag rides along in its base.
    fn eval_map_update(
        &mut self,
        base: &Expr,
        updates: &[(Expr, Expr)],
        env: &Env,
    ) -> Result<Value, String> {
        let mut pairs = match self.eval(base, env)? {
            Value::Map(m) => (*m).clone(),
            other => return Err(format!("cannot update a {} with `%{{ | }}`", other.type_name())),
        };
        for (ke, ve) in updates {
            let key = self.eval(ke, env)?;
            let val = self.eval(ve, env)?;
            pairs = map_put(&pairs, key, val);
        }
        Ok(Value::Map(Rc::new(pairs)))
    }

    fn eval_index(&mut self, base: &Expr, key: &Expr, env: &Env) -> Result<Value, String> {
        let b = self.eval(base, env)?;
        let k = self.eval(key, env)?;
        index(&b, &k)
    }

    fn eval_struct(
        &mut self,
        name: &str,
        fields: &[(String, Expr)],
        env: &Env,
    ) -> Result<Value, String> {
        let def = self
            .structs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("{} is not a struct", name))?;
        let mut out: Vec<(Value, Value)> =
            vec![(Value::Atom("__struct__".into()), Value::Atom(name.to_string()))];
        for (fname, default) in &def {
            let v = match default {
                Some(e) => self.eval(e, env)?,
                None => Value::Nil,
            };
            out.push((Value::Atom(fname.clone()), v));
        }
        for (fname, expr) in fields {
            if !def.iter().any(|(f, _)| f == fname) {
                return Err(format!("unknown field :{} for %{}{{}}", fname, name));
            }
            let v = self.eval(expr, env)?;
            if let Some(slot) = out
                .iter_mut()
                .find(|(k, _)| matches!(k, Value::Atom(a) if a == fname))
            {
                slot.1 = v;
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
        if let Some(module) = self.current.last().cloned() {
            if self.has_def(&module, name) {
                return self.call_user(&module, name, args);
            }
        }
        if let Some(v) = self.process_builtin(name, &args)? {
            return Ok(v);
        }
        if is_kernel(name) {
            return kernel_call(name, args);
        }
        Err(format!("undefined function {}/{}", name, args.len()))
    }

    // Auto-imported process primitives (Elixir's `Kernel`): `spawn/1,2`,
    // `self/0`, `send/2`, `monitor/1`.
    fn process_builtin(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, String> {
        match (name, args) {
            ("spawn", [handler]) => Ok(Some(self.spawn_proc(handler, Value::Nil)?)),
            ("spawn", [handler, state]) => Ok(Some(self.spawn_proc(handler, state.clone())?)),
            ("self", []) => Ok(Some(Value::Pid(self.sched.current.id()))),
            ("send", [Value::Pid(pid), msg]) => {
                self.sched.deliver(Pid(*pid), msg.clone());
                Ok(Some(msg.clone()))
            }
            ("send", [Value::Atom(name), msg]) => match self.sched.whereis(name) {
                Some(pid) => {
                    self.sched.deliver(pid, msg.clone());
                    Ok(Some(msg.clone()))
                }
                None => Err(format!("send/2: no process registered as :{}", name)),
            },
            ("send", [other, _]) => {
                Err(format!("send/2 expects a pid or registered name, got a {}", other.type_name()))
            }
            ("monitor", [Value::Pid(pid)]) => {
                let me = self.sched.current;
                self.sched.monitor(me, Pid(*pid));
                Ok(Some(Value::Pid(*pid)))
            }
            _ => Ok(None),
        }
    }

    fn remote_call(&mut self, module: &str, fun: &str, args: Vec<Value>) -> Result<Value, String> {
        match module {
            "IO" => io_call(fun, args),
            "Kernel" => kernel_call(fun, args),
            "Enum" => self.enum_call(fun, args),
            "String" => string_call(fun, args),
            "Map" => map_call(fun, args),
            "List" => list_call(fun, args),
            "Integer" => integer_call(fun, args),
            "Process" => self.process_module(fun, args),
            "Store" => self.store_call(fun, args),
            _ if crate::prims::is_ai_module(module) => {
                crate::prims::dispatch(self, module, fun, args)
            }
            _ if self.modules.contains_key(module) => self.call_module(module, fun, args),
            _ => Err(format!("module {} is not available", module)),
        }
    }

    fn call_module(&mut self, module: &str, fun: &str, args: Vec<Value>) -> Result<Value, String> {
        if self.has_def(module, fun) {
            self.call_user(module, fun, args)
        } else {
            Err(format!("function {}.{}/{} is undefined", module, fun, args.len()))
        }
    }

    fn has_def(&self, module: &str, name: &str) -> bool {
        self.modules.get(module).map_or(false, |t| t.contains_key(name))
    }

    // Bind a clause's parameters (and any `*rest`) against the call arguments.
    fn bind_params(&mut self, def: &Def, args: &[Value], env: &Env) -> Result<bool, String> {
        let n = def.params.len();
        match &def.rest {
            None => {
                if args.len() != n {
                    return Ok(false);
                }
                self.match_seq(&def.params, args, env)
            }
            Some(rest_name) => {
                if args.len() < n {
                    return Ok(false);
                }
                if !self.match_seq(&def.params, &args[..n], env)? {
                    return Ok(false);
                }
                env.borrow_mut().define(rest_name, Value::list(args[n..].to_vec()));
                Ok(true)
            }
        }
    }

    fn call_user(&mut self, module: &str, name: &str, args: Vec<Value>) -> Result<Value, String> {
        let clauses = self
            .modules
            .get(module)
            .and_then(|t| t.get(name))
            .cloned()
            .unwrap_or_default();
        for def in &clauses {
            let call_env = new_env(Some(self.global.clone()));
            if !self.bind_params(def, &args, &call_env)? {
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

    // ---- Enum (higher-order; calls user functions) ----

    fn apply_callable(&mut self, f: &Value, args: Vec<Value>) -> Result<Value, String> {
        match f {
            Value::Fun(fun) => self.call_fun(fun, args),
            other => Err(format!("expected a function, got {}", other.type_name())),
        }
    }

    // The higher-order Enum functions dispatch to a helper per operation; the
    // rest (no function argument) fall through to `enum_pure`.
    fn enum_call(&mut self, fun: &str, args: Vec<Value>) -> Result<Value, String> {
        match (fun, args.as_slice()) {
            ("map", [Value::List(l), f]) => self.enum_map(l, f),
            ("filter", [Value::List(l), f]) => self.enum_keep(l, f, true),
            ("reject", [Value::List(l), f]) => self.enum_keep(l, f, false),
            ("each", [Value::List(l), f]) => self.enum_each(l, f),
            ("reduce", [Value::List(l), acc, f]) => self.enum_reduce(l, acc.clone(), f),
            ("find", [Value::List(l), f]) => self.enum_find(l, f),
            ("count", [Value::List(l), f]) => self.enum_count(l, f),
            ("any?", [Value::List(l), f]) => self.enum_quantify(l, f, false),
            ("all?", [Value::List(l), f]) => self.enum_quantify(l, f, true),
            ("sort_by", [Value::List(l), f]) => self.enum_sort_by(l, f),
            _ => enum_pure(fun, &args),
        }
    }

    fn enum_map(&mut self, l: &[Value], f: &Value) -> Result<Value, String> {
        let mut out = Vec::with_capacity(l.len());
        for x in l.iter() {
            out.push(self.apply_callable(f, vec![x.clone()])?);
        }
        Ok(Value::list(out))
    }

    fn enum_keep(&mut self, l: &[Value], f: &Value, keep: bool) -> Result<Value, String> {
        let mut out = Vec::new();
        for x in l.iter() {
            if self.apply_callable(f, vec![x.clone()])?.truthy() == keep {
                out.push(x.clone());
            }
        }
        Ok(Value::list(out))
    }

    fn enum_each(&mut self, l: &[Value], f: &Value) -> Result<Value, String> {
        for x in l.iter() {
            self.apply_callable(f, vec![x.clone()])?;
        }
        Ok(Value::Atom("ok".into()))
    }

    fn enum_reduce(&mut self, l: &[Value], acc: Value, f: &Value) -> Result<Value, String> {
        let mut a = acc;
        for x in l.iter() {
            a = self.apply_callable(f, vec![a, x.clone()])?;
        }
        Ok(a)
    }

    fn enum_find(&mut self, l: &[Value], f: &Value) -> Result<Value, String> {
        for x in l.iter() {
            if self.apply_callable(f, vec![x.clone()])?.truthy() {
                return Ok(x.clone());
            }
        }
        Ok(Value::Nil)
    }

    fn enum_count(&mut self, l: &[Value], f: &Value) -> Result<Value, String> {
        let mut n = 0i64;
        for x in l.iter() {
            if self.apply_callable(f, vec![x.clone()])?.truthy() {
                n += 1;
            }
        }
        Ok(Value::Int(n))
    }

    fn enum_sort_by(&mut self, l: &[Value], f: &Value) -> Result<Value, String> {
        let mut keyed = Vec::with_capacity(l.len());
        for x in l.iter() {
            keyed.push((self.apply_callable(f, vec![x.clone()])?, x.clone()));
        }
        keyed.sort_by(|(a, _), (b, _)| cmp_values(a, b));
        Ok(Value::list(keyed.into_iter().map(|(_, v)| v).collect()))
    }

    fn enum_quantify(&mut self, l: &[Value], f: &Value, require_all: bool) -> Result<Value, String> {
        for x in l.iter() {
            let t = self.apply_callable(f, vec![x.clone()])?.truthy();
            if t != require_all {
                return Ok(Value::Bool(!require_all));
            }
        }
        Ok(Value::Bool(require_all))
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
            Pattern::Struct(name, fields) => self.match_struct(name, fields, val, env),
            Pattern::And(a, b) => {
                Ok(self.match_pattern(a, val, env)? && self.match_pattern(b, val, env)?)
            }
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

    fn match_struct(
        &mut self,
        name: &str,
        fields: &[(String, Pattern)],
        val: &Value,
        env: &Env,
    ) -> Result<bool, String> {
        let Value::Map(mpairs) = val else {
            return Ok(false);
        };
        // the value must be a struct of this module
        match Value::map_get(mpairs, &Value::Atom("__struct__".into())) {
            Some(Value::Atom(a)) if a == name => {}
            _ => return Ok(false),
        }
        for (fname, subpat) in fields {
            match Value::map_get(mpairs, &Value::Atom(fname.clone())) {
                Some(v) if self.match_pattern(subpat, &v, env)? => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
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
