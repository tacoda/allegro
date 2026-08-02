use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BinOp, CaseClause, Def, Expr, Pattern, StrPart, StructFields, TopItem, UnOp};
use crate::parser::expr_to_pattern;
use crate::scheduler::{Handler, HandlerRef, Pid, Scheduler};
use crate::value::{new_env, values_equal, Env, Fun, Value};

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
            Expr::Struct(name, fields) => self.eval_struct(name, fields, env),
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

fn ok_tuple(v: Value) -> Value {
    Value::tuple(vec![Value::Atom("ok".into()), v])
}

fn process_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
    match (fun, args.as_slice()) {
        ("sleep", [Value::Int(ms)]) => {
            std::thread::sleep(std::time::Duration::from_millis((*ms).max(0) as u64));
            Ok(Value::Atom("ok".into()))
        }
        _ => Err(format!("Process.{}/{} is undefined", fun, args.len())),
    }
}

// Ordering for sort/sort_by: numbers numerically, else by string form.
fn cmp_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.to_string().cmp(&b.to_string()),
    }
}

// Enum functions that don't call a user function.
fn enum_pure(fun: &str, args: &[Value]) -> Result<Value, String> {
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

fn string_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
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

fn map_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
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

fn map_put(m: &[(Value, Value)], key: Value, val: Value) -> Vec<(Value, Value)> {
    let mut out: Vec<(Value, Value)> = m.to_vec();
    if let Some(slot) = out.iter_mut().find(|(k, _)| values_equal(k, &key)) {
        slot.1 = val;
    } else {
        out.push((key, val));
    }
    out
}

fn list_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
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

fn integer_call(fun: &str, args: Vec<Value>) -> Result<Value, String> {
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
