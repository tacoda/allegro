use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BinOp, Expr, Pattern, Stmt, UnOp};
use crate::builtins;
use crate::value::{
    new_env, AgentObj, Class, Command, Env, Factory, Func, Graph, Harness, Message, Skill, Value,
};

// Control-flow signal so `return` can unwind through blocks.
enum Flow {
    Normal(Value),
    Return(Value),
}

pub struct Interp {
    pub global: Env,
}

impl Interp {
    pub fn new() -> Interp {
        let global = new_env(None);
        builtins::register(&global);
        install_env_hash(&global);
        Interp { global }
    }

    pub fn run(&mut self, program: &[Stmt]) -> Result<(), String> {
        let env = self.global.clone();
        for stmt in program {
            if let Flow::Return(_) = self.exec(stmt, &env)? {
                break;
            }
        }
        Ok(())
    }

    fn exec_block(&mut self, stmts: &[Stmt], env: &Env) -> Result<Flow, String> {
        let mut last = Value::Nil;
        for stmt in stmts {
            match self.exec(stmt, env)? {
                Flow::Return(v) => return Ok(Flow::Return(v)),
                Flow::Normal(v) => last = v,
            }
        }
        Ok(Flow::Normal(last))
    }

    fn exec(&mut self, stmt: &Stmt, env: &Env) -> Result<Flow, String> {
        match stmt {
            Stmt::Expr(e) => Ok(Flow::Normal(self.eval(e, env)?)),
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Nil,
                };
                Ok(Flow::Return(v))
            }
            Stmt::If {
                cond,
                then,
                elifs,
                els,
            } => self.exec_if(cond, then, elifs, els, env),
            Stmt::Match {
                subject,
                arms,
                els,
            } => self.exec_match(subject, arms, els, env),
            Stmt::While(cond, body) => self.exec_while(cond, body, env),
            Stmt::For(var, iter, body) => self.exec_for(var, iter, body, env),
            Stmt::Def(name, params, body) => {
                let f = Value::Func(Rc::new(Func {
                    params: params.clone(),
                    body: body.clone(),
                    closure: env.clone(),
                }));
                env.borrow_mut().define(name, f);
                Ok(Flow::Normal(Value::Nil))
            }
            Stmt::Class {
                name,
                base,
                methods,
            } => self.exec_class(name, base, methods, env),
        }
    }

    fn exec_class(
        &mut self,
        name: &str,
        base: &str,
        methods: &[(String, Vec<String>, Vec<Stmt>)],
        env: &Env,
    ) -> Result<Flow, String> {
        // A base of another class name means class-to-class inheritance;
        // otherwise it names a primitive kind.
        let (base_kind, parent) = match env.borrow().get(base) {
            Some(Value::Class(p)) => (p.base.clone(), Some(p)),
            _ => (base.to_string(), None),
        };
        let mut method_map = HashMap::new();
        for (mname, params, body) in methods {
            method_map.insert(
                mname.clone(),
                Rc::new(Func {
                    params: params.clone(),
                    body: body.clone(),
                    closure: env.clone(),
                }),
            );
        }
        let class = Value::Class(Rc::new(Class {
            name: name.to_string(),
            base: base_kind,
            parent,
            methods: method_map,
        }));
        env.borrow_mut().define(name, class);
        Ok(Flow::Normal(Value::Nil))
    }

    fn exec_if(
        &mut self,
        cond: &Expr,
        then: &[Stmt],
        elifs: &[(Expr, Vec<Stmt>)],
        els: &Option<Vec<Stmt>>,
        env: &Env,
    ) -> Result<Flow, String> {
        if self.eval(cond, env)?.truthy() {
            return self.exec_block(then, env);
        }
        for (c, body) in elifs {
            if self.eval(c, env)?.truthy() {
                return self.exec_block(body, env);
            }
        }
        if let Some(body) = els {
            return self.exec_block(body, env);
        }
        Ok(Flow::Normal(Value::Nil))
    }

    fn exec_match(
        &mut self,
        subject: &Expr,
        arms: &[(Pattern, Vec<Stmt>)],
        els: &Option<Vec<Stmt>>,
        env: &Env,
    ) -> Result<Flow, String> {
        let value = self.eval(subject, env)?;
        for (pat, body) in arms {
            match pat {
                Pattern::Wildcard => return self.exec_block(body, env),
                Pattern::Type(t) => {
                    if type_matches(t, &value) {
                        return self.exec_block(body, env);
                    }
                }
                Pattern::Bind(name) => {
                    let arm_env = new_env(Some(env.clone()));
                    arm_env.borrow_mut().define(name, value.clone());
                    return self.exec_block(body, &arm_env);
                }
                Pattern::Value(expr) => {
                    let pv = self.eval(expr, env)?;
                    if values_equal(&pv, &value) {
                        return self.exec_block(body, env);
                    }
                }
            }
        }
        if let Some(body) = els {
            return self.exec_block(body, env);
        }
        Ok(Flow::Normal(Value::Nil))
    }

    fn exec_while(&mut self, cond: &Expr, body: &[Stmt], env: &Env) -> Result<Flow, String> {
        while self.eval(cond, env)?.truthy() {
            if let Flow::Return(v) = self.exec_block(body, env)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Normal(Value::Nil))
    }

    fn exec_for(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
        env: &Env,
    ) -> Result<Flow, String> {
        let items = match self.eval(iter, env)? {
            Value::Array(a) => a.borrow().clone(),
            other => return Err(format!("cannot iterate over {}", other.type_name())),
        };
        for item in items {
            let loop_env = new_env(Some(env.clone()));
            loop_env.borrow_mut().define(var, item);
            if let Flow::Return(v) = self.exec_block(body, &loop_env)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Normal(Value::Nil))
    }

    fn eval(&mut self, expr: &Expr, env: &Env) -> Result<Value, String> {
        match expr {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nil => Ok(Value::Nil),
            Expr::Ident(name) => env
                .borrow()
                .get(name)
                .ok_or_else(|| format!("undefined variable '{}'", name)),
            Expr::IVar(name) => {
                let inst = self.current_instance(env)?;
                let v = inst.ivars.borrow().get(name).cloned();
                Ok(v.unwrap_or(Value::Nil))
            }
            Expr::Array(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for it in items {
                    vals.push(self.eval(it, env)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(vals))))
            }
            Expr::Hash(pairs) => {
                let mut map = HashMap::new();
                for (k, v) in pairs {
                    map.insert(k.clone(), self.eval(v, env)?);
                }
                Ok(Value::Hash(Rc::new(RefCell::new(map))))
            }
            Expr::Index(base, idx) => self.eval_index(base, idx, env),
            Expr::Unary(op, e) => self.eval_unary(*op, e, env),
            Expr::Binary(op, l, r) => self.eval_binary(*op, l, r, env),
            Expr::Assign(target, value) => self.eval_assign(target, value, env),
            Expr::Call(callee, args) => self.eval_call(callee, args, env),
            Expr::Method(recv, name, args) => self.eval_method(recv, name, args, env),
            Expr::Func(params, body) => Ok(Value::Func(Rc::new(Func {
                params: params.clone(),
                body: body.clone(),
                closure: env.clone(),
            }))),
        }
    }

    fn eval_index(&mut self, base: &Expr, idx: &Expr, env: &Env) -> Result<Value, String> {
        let b = self.eval(base, env)?;
        let i = self.eval(idx, env)?;
        match b {
            Value::Array(a) => {
                let n = as_index(&i)?;
                Ok(a.borrow().get(n).cloned().unwrap_or(Value::Nil))
            }
            Value::Hash(h) => Ok(h.borrow().get(&i.to_string()).cloned().unwrap_or(Value::Nil)),
            Value::Str(s) => {
                let n = as_index(&i)?;
                Ok(s.chars().nth(n).map(|c| Value::Str(c.to_string())).unwrap_or(Value::Nil))
            }
            other => Err(format!("cannot index into {}", other.type_name())),
        }
    }

    fn eval_unary(&mut self, op: UnOp, e: &Expr, env: &Env) -> Result<Value, String> {
        let v = self.eval(e, env)?;
        match op {
            UnOp::Not => Ok(Value::Bool(!v.truthy())),
            UnOp::Neg => match v {
                Value::Num(n) => Ok(Value::Num(-n)),
                other => Err(format!("cannot negate {}", other.type_name())),
            },
        }
    }

    fn eval_binary(&mut self, op: BinOp, l: &Expr, r: &Expr, env: &Env) -> Result<Value, String> {
        // Short-circuit logical operators.
        if let BinOp::And = op {
            let lv = self.eval(l, env)?;
            return if lv.truthy() { self.eval(r, env) } else { Ok(lv) };
        }
        if let BinOp::Or = op {
            let lv = self.eval(l, env)?;
            return if lv.truthy() { Ok(lv) } else { self.eval(r, env) };
        }

        let lv = self.eval(l, env)?;
        let rv = self.eval(r, env)?;
        eval_arith(op, lv, rv)
    }

    fn eval_assign(&mut self, target: &Expr, value: &Expr, env: &Env) -> Result<Value, String> {
        let val = self.eval(value, env)?;
        match target {
            Expr::Ident(name) => {
                env.borrow_mut().set(name, val.clone());
                Ok(val)
            }
            Expr::IVar(name) => {
                let inst = self.current_instance(env)?;
                inst.ivars.borrow_mut().insert(name.clone(), val.clone());
                Ok(val)
            }
            Expr::Index(base, idx) => {
                let b = self.eval(base, env)?;
                let i = self.eval(idx, env)?;
                match b {
                    Value::Array(a) => {
                        let n = as_index(&i)?;
                        let mut vec = a.borrow_mut();
                        if n < vec.len() {
                            vec[n] = val.clone();
                        } else {
                            return Err(format!("array index {} out of bounds", n));
                        }
                    }
                    Value::Hash(h) => {
                        h.borrow_mut().insert(i.to_string(), val.clone());
                    }
                    other => return Err(format!("cannot assign into {}", other.type_name())),
                }
                Ok(val)
            }
            _ => Err("invalid assignment target".into()),
        }
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr], env: &Env) -> Result<Value, String> {
        let f = self.eval(callee, env)?;
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        self.apply(f, argv)
    }

    fn apply(&mut self, f: Value, args: Vec<Value>) -> Result<Value, String> {
        match f {
            Value::Builtin(_, func) => func(&args),
            Value::Func(func) => {
                if args.len() != func.params.len() {
                    return Err(format!(
                        "function expects {} args, got {}",
                        func.params.len(),
                        args.len()
                    ));
                }
                let call_env = new_env(Some(func.closure.clone()));
                for (p, a) in func.params.iter().zip(args) {
                    call_env.borrow_mut().define(p, a);
                }
                match self.exec_block(&func.body, &call_env)? {
                    Flow::Return(v) => Ok(v),
                    Flow::Normal(v) => Ok(v),
                }
            }
            other => Err(format!("{} is not callable", other.type_name())),
        }
    }

    fn eval_method(
        &mut self,
        recv: &Expr,
        name: &str,
        args: &[Expr],
        env: &Env,
    ) -> Result<Value, String> {
        let target = self.eval(recv, env)?;
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        self.call_method(target, name, argv)
    }

    // Dispatch a method on any value. Primitives that invoke user
    // functions/agents are handled here; pure ones fall through to `methods`.
    fn call_method(&mut self, target: Value, name: &str, argv: Vec<Value>) -> Result<Value, String> {
        match target {
            Value::Agent(a) => self.agent_method(&a, name, argv),
            Value::Subagent(s) => self.subagent_method(&s, name, argv),
            Value::Command(c) => self.command_method(&c, name, argv),
            Value::Graph(g) => self.graph_method(&g, name, argv),
            Value::Factory(fac) => self.factory_method(&fac, name, argv),
            Value::Harness(h) => self.harness_method(&h, name, argv),
            Value::Class(c) => self.class_method(&c, name, argv),
            Value::Instance(i) => self.instance_method(&i, name, argv),
            other => crate::methods::dispatch(other, name, argv),
        }
    }

    // ---- agent runtime ----

    // Run an agent: before-hooks -> LLM -> after-hooks, producing a Message.
    fn agent_run(&mut self, agent: &Rc<AgentObj>, input: String) -> Result<Value, String> {
        let mut text = input;
        for hook in &agent.before.clone() {
            let (val, halt) = self.apply_hook(hook, Value::Str(text.clone()))?;
            if halt {
                return Ok(make_message(val.to_string(), &agent.name));
            }
            text = val.to_string();
        }

        let content = agent.core.run(&text)?;
        let mut msg = make_message(content, &agent.name);

        for hook in &agent.after.clone() {
            let (val, halt) = self.apply_hook(hook, msg.clone())?;
            msg = coerce_message(val, &agent.name);
            if halt {
                break;
            }
        }
        Ok(msg)
    }

    // Invoke a hook function, normalizing its return into (payload, halt?).
    fn apply_hook(&mut self, hook: &Value, arg: Value) -> Result<(Value, bool), String> {
        let action = match hook {
            Value::Hook(h) => h.action.clone(),
            other => other.clone(),
        };
        let result = self.apply(action, vec![arg])?;
        match result {
            Value::HookResult(r) => Ok((r.value.clone(), r.halt)),
            other => Ok((other, false)),
        }
    }

    fn agent_method(
        &mut self,
        agent: &Rc<AgentObj>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match name {
            "run" | "ask" | "invoke" => {
                let input = args.first().map(|v| v.to_string()).unwrap_or_default();
                self.agent_run(agent, input)
            }
            "use" => {
                let skill = self.resolve_skill(agent, args.first())?;
                let input = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                let composed = format!("{}\n\n{}", skill.instructions, input);
                self.agent_run(agent, composed)
            }
            "delegate" => {
                let target = args.first().map(|v| v.to_string()).unwrap_or_default();
                let task = args.get(1).map(|v| v.to_string()).unwrap_or_default();
                let sub = agent
                    .subagents
                    .iter()
                    .find(|(n, _)| *n == target)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| format!("agent has no sub-agent '{}'", target))?;
                match sub {
                    Value::Agent(a) => self.agent_run(&a, task),
                    Value::Subagent(s) => match &s.agent {
                        Value::Agent(a) => self.agent_run(a, task),
                        _ => Err("subagent has no worker agent".into()),
                    },
                    other => Err(format!("sub-agent '{}' is a {}", target, other.type_name())),
                }
            }
            "fan_out" => {
                let inputs: Vec<String> = match args.first() {
                    Some(Value::Array(items)) => {
                        items.borrow().iter().map(|v| v.to_string()).collect()
                    }
                    Some(other) => vec![other.to_string()],
                    None => return Err("fan_out expects a list of inputs".into()),
                };
                let results = agent.core.fan_out(inputs)?;
                let msgs: Vec<Value> = results
                    .into_iter()
                    .map(|c| make_message(c, &agent.name))
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(msgs))))
            }
            "name" => Ok(Value::Str(agent.name.clone())),
            "model" => Ok(Value::Str(agent.core.model.clone())),
            "skills" => {
                let items: Vec<Value> =
                    agent.skills.iter().map(|s| Value::Skill(s.clone())).collect();
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            _ => Err(format!("agent has no method '{}'", name)),
        }
    }

    fn resolve_skill(
        &self,
        agent: &Rc<AgentObj>,
        arg: Option<&Value>,
    ) -> Result<Rc<Skill>, String> {
        match arg {
            Some(Value::Skill(s)) => Ok(s.clone()),
            Some(other) => {
                let wanted = other.to_string();
                agent
                    .skills
                    .iter()
                    .find(|s| s.name == wanted)
                    .cloned()
                    .ok_or_else(|| format!("agent has no skill '{}'", wanted))
            }
            None => Err("'use' expects a skill".into()),
        }
    }

    fn subagent_method(
        &mut self,
        sub: &Rc<crate::value::Subagent>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match name {
            "name" => Ok(Value::Str(sub.name.clone())),
            "description" => Ok(Value::Str(sub.description.clone())),
            "agent" => Ok(sub.agent.clone()),
            // Anything else (run, ask, use, ...) delegates to the worker agent.
            _ => self.call_method(sub.agent.clone(), name, args),
        }
    }

    fn command_method(
        &mut self,
        cmd: &Rc<Command>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match name {
            "run" | "call" | "invoke" => {
                let input = args.into_iter().next().unwrap_or(Value::Nil);
                self.apply(cmd.action.clone(), vec![input])
            }
            "name" => Ok(Value::Str(cmd.name.clone())),
            "description" => Ok(Value::Str(cmd.description.clone())),
            _ => Err(format!("command has no method '{}'", name)),
        }
    }

    fn factory_method(
        &mut self,
        fac: &Rc<Factory>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match name {
            "create" | "build" | "make" => self.apply(fac.build.clone(), args),
            _ => Err(format!("factory has no method '{}'", name)),
        }
    }

    // ---- classes and instances ----

    fn class_method(
        &mut self,
        class: &Rc<Class>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match name {
            "new" => self.class_new(class, args),
            "name" => Ok(Value::Str(class.name.clone())),
            _ => Err(format!("class {} has no method '{}'", class.name, name)),
        }
    }

    // Build an instance: assemble config down the inheritance chain, construct
    // the base primitive, then run `init` if defined.
    fn class_new(&mut self, class: &Rc<Class>, args: Vec<Value>) -> Result<Value, String> {
        let cfg = self.class_config(class)?;
        let cfg_val = Value::Hash(Rc::new(RefCell::new(cfg)));
        let base = self.construct_base(&class.base, cfg_val)?;
        let inst = Rc::new(crate::value::Instance {
            class: class.clone(),
            base,
            ivars: RefCell::new(HashMap::new()),
        });
        if class.find_method("init").is_some() {
            self.instance_method(&inst, "init", args)?;
        }
        Ok(Value::Instance(inst))
    }

    // Merge `config` results from the root parent down to this class.
    fn class_config(&mut self, class: &Rc<Class>) -> Result<HashMap<String, Value>, String> {
        let mut cfg = match &class.parent {
            Some(p) => self.class_config(p)?,
            None => HashMap::new(),
        };
        if let Some(f) = class.methods.get("config") {
            match self.apply(Value::Func(f.clone()), vec![])? {
                Value::Hash(h) => {
                    for (k, v) in h.borrow().iter() {
                        cfg.insert(k.clone(), v.clone());
                    }
                }
                other => {
                    return Err(format!(
                        "config must return a hash, got {}",
                        other.type_name()
                    ))
                }
            }
        }
        Ok(cfg)
    }

    // Resolve `self` for an @ivar access — only valid inside a method body.
    fn current_instance(&self, env: &Env) -> Result<Rc<crate::value::Instance>, String> {
        match env.borrow().get("self") {
            Some(Value::Instance(i)) => Ok(i),
            _ => Err("@field used outside of an instance method".into()),
        }
    }

    fn construct_base(&mut self, kind: &str, cfg: Value) -> Result<Value, String> {
        crate::builtins::make(kind, cfg)
    }

    fn instance_method(
        &mut self,
        inst: &Rc<crate::value::Instance>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        // A method the class defines or inherits wins over the base primitive.
        if let Some(f) = inst.class.find_method(name) {
            return self.call_bound(inst, &f, args);
        }
        match name {
            "base" => Ok(inst.base.clone()),
            "class" => Ok(Value::Class(inst.class.clone())),
            // Otherwise behave like the underlying primitive.
            _ => self.call_method(inst.base.clone(), name, args),
        }
    }

    // Call a class method with `self` bound to the instance.
    fn call_bound(
        &mut self,
        inst: &Rc<crate::value::Instance>,
        f: &Rc<Func>,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        if args.len() != f.params.len() {
            return Err(format!(
                "method expects {} args, got {}",
                f.params.len(),
                args.len()
            ));
        }
        let call_env = new_env(Some(f.closure.clone()));
        {
            let mut e = call_env.borrow_mut();
            e.define("self", Value::Instance(inst.clone()));
            for (p, a) in f.params.iter().zip(args) {
                e.define(p, a);
            }
        }
        match self.exec_block(&f.body, &call_env)? {
            Flow::Return(v) | Flow::Normal(v) => Ok(v),
        }
    }

    // Invoke a harness: apply the charter's rules and before-hooks, run the
    // agent or graph, then apply the charter's after-hooks.
    fn harness_method(
        &mut self,
        harness: &Rc<Harness>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        match name {
            "invoke" | "run" | "trigger" => self.harness_invoke(harness, args),
            "command" => self.charter_lookup(harness, args, true),
            "skill" => self.charter_lookup(harness, args, false),
            _ => Err(format!("harness has no method '{}'", name)),
        }
    }

    fn harness_invoke(
        &mut self,
        harness: &Rc<Harness>,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let mut text = args.first().map(|v| v.to_string()).unwrap_or_default();

        // The charter's rules become a preamble; its before-hooks run first.
        if let Some(charter) = &harness.charter {
            if !charter.rules.is_empty() {
                let lines: Vec<String> =
                    charter.rules.iter().map(|r| format!("- {}", r.text)).collect();
                text = format!("Follow these rules:\n{}\n\n{}", lines.join("\n"), text);
            }
            for hook in &charter.before.clone() {
                let (val, halt) = self.apply_hook(hook, Value::Str(text.clone()))?;
                if halt {
                    return Ok(make_message(val.to_string(), "harness"));
                }
                text = val.to_string();
            }
        }

        // A harness runs its graph; a charter-only harness needs a model,
        // i.e. it must be combined into an agent to run.
        let mut result = if let Some(Value::Graph(g)) = &harness.graph {
            self.graph_method(g, "invoke", vec![Value::Str(text)])?
        } else {
            return Err("harness has no graph to run — add a model to make an agent".into());
        };

        if let Some(charter) = &harness.charter {
            for hook in &charter.after.clone() {
                let (val, halt) = self.apply_hook(hook, result.clone())?;
                result = coerce_message(val, "harness");
                if halt {
                    break;
                }
            }
        }
        Ok(result)
    }

    // harness.command("name") / harness.skill("name") — look up a charter entry.
    fn charter_lookup(
        &self,
        harness: &Rc<Harness>,
        args: Vec<Value>,
        command: bool,
    ) -> Result<Value, String> {
        let wanted = args.first().map(|v| v.to_string()).unwrap_or_default();
        let charter = harness
            .charter
            .as_ref()
            .ok_or("harness has no charter")?;
        if command {
            charter
                .commands
                .iter()
                .find(|c| c.name == wanted)
                .map(|c| Value::Command(c.clone()))
                .ok_or_else(|| format!("charter has no command '{}'", wanted))
        } else {
            charter
                .skills
                .iter()
                .find(|s| s.name == wanted)
                .map(|s| Value::Skill(s.clone()))
                .ok_or_else(|| format!("charter has no skill '{}'", wanted))
        }
    }

    // Walk a graph from its entry node until an edge resolves to "end".
    fn graph_method(
        &mut self,
        graph: &Rc<Graph>,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        if !matches!(name, "run" | "invoke" | "trigger") {
            return Err(format!("graph has no method '{}'", name));
        }
        let mut current = graph.entry.clone();
        let mut state = args.into_iter().next().unwrap_or(Value::Nil);

        for _ in 0..graph.max_steps {
            let node = graph
                .node(&current)
                .ok_or_else(|| format!("graph has no node '{}'", current))?
                .clone();
            state = match node {
                Value::Agent(a) => self.agent_run(&a, state.to_string())?,
                // A node can itself be a subgraph.
                Value::Graph(sub) => self.graph_method(&sub, "invoke", vec![state])?,
                callable => self.apply(callable, vec![state])?,
            };
            current = match self.next_node(graph, &current, &state)? {
                Some(next) => next,
                None => return Ok(state),
            };
        }
        Err(format!("graph exceeded {} steps (cycle?)", graph.max_steps))
    }

    // Resolve the edge out of `from`: a static name, or a router function that
    // takes the current state and returns the next node name. "end"/nil stops.
    fn next_node(
        &mut self,
        graph: &Rc<Graph>,
        from: &str,
        state: &Value,
    ) -> Result<Option<String>, String> {
        let edge = match graph.edge(from) {
            Some(e) => e.clone(),
            None => return Ok(None),
        };
        let target = match edge {
            Value::Func(_) | Value::Builtin(_, _) => self.apply(edge, vec![state.clone()])?,
            other => other,
        };
        match target {
            Value::Nil => Ok(None),
            v => {
                let s = v.to_string();
                if s == "end" || s.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(s))
                }
            }
        }
    }
}

fn make_message(content: String, from: &str) -> Value {
    Value::Message(Rc::new(Message {
        content,
        role: "assistant".to_string(),
        from: from.to_string(),
    }))
}

// Normalize a hook's return payload into a Message.
fn coerce_message(v: Value, from: &str) -> Value {
    match v {
        Value::Message(_) => v,
        other => make_message(other.to_string(), from),
    }
}

fn eval_arith(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    use BinOp::*;
    match op {
        Add => match (&l, &r) {
            (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a + b)),
            (Value::Str(_), _) | (_, Value::Str(_)) => {
                Ok(Value::Str(format!("{}{}", l, r)))
            }
            (Value::Array(a), Value::Array(b)) => {
                let mut v = a.borrow().clone();
                v.extend(b.borrow().iter().cloned());
                Ok(Value::Array(Rc::new(RefCell::new(v))))
            }
            _ => Err(format!("cannot add {} and {}", l.type_name(), r.type_name())),
        },
        Sub | Mul | Div | Mod => {
            let (a, b) = num_pair(&l, &r)?;
            Ok(Value::Num(match op {
                Sub => a - b,
                Mul => a * b,
                Div => a / b,
                Mod => a % b,
                _ => unreachable!(),
            }))
        }
        Eq => Ok(Value::Bool(values_equal(&l, &r))),
        Neq => Ok(Value::Bool(!values_equal(&l, &r))),
        Lt | Gt | Le | Ge => {
            let (a, b) = num_pair(&l, &r)?;
            Ok(Value::Bool(match op {
                Lt => a < b,
                Gt => a > b,
                Le => a <= b,
                Ge => a >= b,
                _ => unreachable!(),
            }))
        }
        And | Or => unreachable!("handled in eval_binary"),
    }
}

fn num_pair(l: &Value, r: &Value) -> Result<(f64, f64), String> {
    match (l, r) {
        (Value::Num(a), Value::Num(b)) => Ok((*a, *b)),
        _ => Err(format!(
            "expected two numbers, got {} and {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

// Compare a `when Type` pattern against a value, ignoring case and underscores
// so `HookResult`, `hook_result`, and `hookresult` all match.
fn type_matches(pat: &str, v: &Value) -> bool {
    let norm = |s: &str| s.to_lowercase().replace('_', "");
    norm(pat) == norm(v.type_name())
}

fn values_equal(l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Num(a), Value::Num(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        _ => false,
    }
}

fn as_index(v: &Value) -> Result<usize, String> {
    match v {
        Value::Num(n) if *n >= 0.0 => Ok(*n as usize),
        _ => Err(format!("invalid index {}", v)),
    }
}

fn install_env_hash(env: &Env) {
    let mut map = HashMap::new();
    for (k, v) in std::env::vars() {
        map.insert(k, Value::Str(v));
    }
    env.borrow_mut()
        .define("env", Value::Hash(Rc::new(RefCell::new(map))));
}
