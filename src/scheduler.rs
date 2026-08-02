use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::interp::Interp;
use crate::value::{Class, Instance, Value};

// A child managed by a supervisor: the class to (re)start and the args to pass,
// plus the pid of the currently-running instance.
#[derive(Clone)]
pub(crate) struct Child {
    class: Rc<Class>,
    args: Vec<Value>,
    pid: u64,
}

// What a process does with each message it receives. Handlers run to completion
// — they never block mid-body — so the scheduler is a plain cooperative loop.
#[derive(Clone)]
pub(crate) enum Handler {
    // The top-level flow. Its mailbox is drained by `receive`.
    Main,
    // A bare actor: a function of (state, msg) that returns the new state.
    Actor(Value),
    // A GenServer instance: messages are tagged call/cast, dispatched to its
    // handle_call / handle_cast methods.
    GenServer(Rc<Instance>),
    // A one-shot green thread: runs its function once, result becomes its state.
    Task(Value),
    // A supervisor: restarts crashed children per its strategy.
    Supervisor {
        children: Vec<Child>,
        max_restarts: usize,
        restarts: usize,
    },
}

// A live process: a mailbox, its current state, and how it handles messages.
pub(crate) struct Proc {
    pub(crate) mailbox: VecDeque<Value>,
    pub(crate) state: Value,
    pub(crate) handler: Handler,
    pub(crate) monitors: Vec<u64>, // pids to notify with DOWN when this dies
    pub(crate) alive: bool,
}

// What running one message produced.
enum Outcome {
    State(Value),           // actor/genserver: the new state
    Done(Value),            // task: finished with a result, process exits
    Handler(Handler, Value), // supervisor: updated handler + state
}

// ---- the scheduler + OTP process model ----
//
// A cooperative, single-threaded green-thread scheduler. Every process is
// `state + a handler + a mailbox`. Handlers run to completion, so scheduling is
// a plain run-queue loop with no stack switching. `send`/`cast` enqueue and
// return immediately; `receive`/`call` pump the queue until it is idle. This is
// concurrency (interleaved logical parallelism), not multicore parallelism —
// the interpreter's values are `Rc`-based and single-threaded by construction.
impl Interp {
    fn spawn_proc(&mut self, handler: Handler, state: Value) -> u64 {
        let pid = self.next_pid;
        self.next_pid += 1;
        self.procs.insert(
            pid,
            Proc {
                mailbox: VecDeque::new(),
                state,
                handler,
                monitors: Vec::new(),
                alive: true,
            },
        );
        pid
    }

    fn resolve(&self, v: &Value) -> Option<u64> {
        match v {
            Value::Pid(id) => Some(*id),
            Value::Str(name) => self.registry.get(name).copied(),
            _ => None,
        }
    }

    // Put a message in a process's mailbox and mark it runnable.
    fn deliver(&mut self, pid: u64, msg: Value) {
        match self.procs.get_mut(&pid) {
            Some(p) if p.alive => p.mailbox.push_back(msg),
            _ => return,
        }
        self.schedule(pid);
    }

    // The main flow (pid 0) is driven by `receive`, not the run queue.
    fn schedule(&mut self, pid: u64) {
        if pid != 0 && !self.run_queue.contains(&pid) {
            self.run_queue.push_back(pid);
        }
    }

    // Drain the run queue until every process is idle.
    pub(crate) fn run_scheduler(&mut self) {
        while let Some(pid) = self.run_queue.pop_front() {
            let msg = match self.procs.get_mut(&pid) {
                Some(p) if p.alive => p.mailbox.pop_front(),
                _ => None,
            };
            let Some(msg) = msg else { continue };
            self.run_one(pid, msg);
            let more = self
                .procs
                .get(&pid)
                .map(|p| p.alive && !p.mailbox.is_empty())
                .unwrap_or(false);
            if more {
                self.schedule(pid);
            }
        }
    }

    fn run_one(&mut self, pid: u64, msg: Value) {
        let handler = match self.procs.get(&pid) {
            Some(p) => p.handler.clone(),
            None => return,
        };
        let state = self.procs.get(&pid).map(|p| p.state.clone()).unwrap_or(Value::Nil);
        let saved = self.current_pid;
        self.current_pid = pid;
        let result = match &handler {
            Handler::Main => Ok(Outcome::State(state)),
            Handler::Actor(f) => self.apply(f.clone(), vec![state, msg]).map(Outcome::State),
            Handler::Task(f) => self.apply(f.clone(), vec![]).map(Outcome::Done),
            Handler::GenServer(inst) => self.run_genserver(inst.clone(), state, msg),
            Handler::Supervisor { .. } => self.run_supervisor(pid, handler.clone(), msg),
        };
        self.current_pid = saved;
        match result {
            Ok(Outcome::State(ns)) => {
                if let Some(p) = self.procs.get_mut(&pid) {
                    p.state = ns;
                }
            }
            Ok(Outcome::Done(r)) => {
                if let Some(p) = self.procs.get_mut(&pid) {
                    p.state = r;
                    p.alive = false;
                }
            }
            Ok(Outcome::Handler(h, ns)) => {
                if let Some(p) = self.procs.get_mut(&pid) {
                    p.handler = h;
                    p.state = ns;
                }
            }
            Err(reason) => self.crash(pid, reason),
        }
    }

    // A crashed process dies and notifies its monitors with a DOWN message.
    fn crash(&mut self, pid: u64, reason: String) {
        let monitors = match self.procs.get_mut(&pid) {
            Some(p) => {
                p.alive = false;
                p.mailbox.clear();
                std::mem::take(&mut p.monitors)
            }
            None => return,
        };
        let down = down_msg(pid, &reason);
        for m in monitors {
            self.deliver(m, down.clone());
        }
    }

    // A clean stop: dies with reason "normal" (supervisors don't restart it).
    fn stop_proc(&mut self, pid: u64) {
        let monitors = match self.procs.get_mut(&pid) {
            Some(p) if p.alive => {
                p.alive = false;
                std::mem::take(&mut p.monitors)
            }
            _ => return,
        };
        let down = down_msg(pid, "normal");
        for m in monitors {
            self.deliver(m, down.clone());
        }
    }

    // ---- scheduler builtins (spawn/send/receive/monitor) ----

    pub(crate) fn b_spawn(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let f = args
            .first()
            .cloned()
            .ok_or("spawn expects a handler function")?;
        if !matches!(f, Value::Func(_)) {
            return Err("spawn's first argument must be a function".into());
        }
        let state = args.get(1).cloned().unwrap_or(Value::Nil);
        Ok(Value::Pid(self.spawn_proc(Handler::Actor(f), state)))
    }

    pub(crate) fn b_send(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let target = args.first().ok_or("send expects (target, message)")?;
        let msg = args.get(1).cloned().unwrap_or(Value::Nil);
        let pid = self
            .resolve(target)
            .ok_or_else(|| format!("send: no process for {}", target))?;
        self.deliver(pid, msg.clone());
        Ok(msg)
    }

    pub(crate) fn b_receive(&mut self, _args: Vec<Value>) -> Result<Value, String> {
        self.run_scheduler();
        Ok(self
            .procs
            .get_mut(&0)
            .and_then(|p| p.mailbox.pop_front())
            .unwrap_or(Value::Nil))
    }

    pub(crate) fn b_monitor(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let target = self
            .resolve(args.first().unwrap_or(&Value::Nil))
            .ok_or("monitor expects a pid")?;
        let me = self.current_pid;
        match self.procs.get_mut(&target) {
            Some(p) if p.alive => p.monitors.push(me),
            // Monitoring a dead process fires DOWN immediately.
            _ => {
                let down = down_msg(target, "noproc");
                self.deliver(me, down);
            }
        }
        Ok(Value::Nil)
    }

    // ---- pid methods ----

    pub(crate) fn pid_method(&mut self, id: u64, name: &str, argv: Vec<Value>) -> Result<Value, String> {
        match name {
            "send" => {
                let msg = argv.into_iter().next().unwrap_or(Value::Nil);
                self.deliver(id, msg);
                Ok(Value::Pid(id))
            }
            "cast" => {
                let payload = argv.into_iter().next().unwrap_or(Value::Nil);
                let m = envelope(vec![("__kind__", Value::Str("cast".into())), ("payload", payload)]);
                self.deliver(id, m);
                Ok(Value::Nil)
            }
            "call" => {
                let payload = argv.into_iter().next().unwrap_or(Value::Nil);
                let rf = self.next_ref;
                self.next_ref += 1;
                let from = self.current_pid;
                let m = envelope(vec![
                    ("__kind__", Value::Str("call".into())),
                    ("payload", payload),
                    ("from", Value::Pid(from)),
                    ("ref", Value::Num(rf as f64)),
                ]);
                self.deliver(id, m);
                self.run_scheduler();
                Ok(self.take_reply(from, rf).unwrap_or(Value::Nil))
            }
            "stop" => {
                self.stop_proc(id);
                Ok(Value::Nil)
            }
            "alive?" => Ok(Value::Bool(
                self.procs.get(&id).map(|p| p.alive).unwrap_or(false),
            )),
            "id" => Ok(Value::Num(id as f64)),
            "which_children" => {
                let pids = match self.procs.get(&id).map(|p| &p.handler) {
                    Some(Handler::Supervisor { children, .. }) => {
                        children.iter().map(|c| Value::Pid(c.pid)).collect()
                    }
                    _ => Vec::new(),
                };
                Ok(Value::Array(Rc::new(RefCell::new(pids))))
            }
            _ => Err(format!("pid has no method '{}'", name)),
        }
    }

    // Pull the reply matching `rf` out of a process's mailbox, leaving the rest.
    fn take_reply(&mut self, pid: u64, rf: u64) -> Option<Value> {
        let mb = &mut self.procs.get_mut(&pid)?.mailbox;
        let pos = mb.iter().position(|m| ref_of(m) == Some(rf))?;
        let env = mb.remove(pos)?;
        hget(&env, "__reply__")
    }

    // ---- GenServer ----

    pub(crate) fn genserver_start(&mut self, class: &Rc<Class>, args: Vec<Value>) -> Result<Value, String> {
        let inst = Rc::new(Instance {
            class: class.clone(),
            base: Value::Nil,
            ivars: RefCell::new(HashMap::new()),
        });
        let state = if class.find_method("init").is_some() {
            self.instance_method(&inst, "init", args)?
        } else {
            args.into_iter().next().unwrap_or(Value::Nil)
        };
        Ok(Value::Pid(self.spawn_proc(Handler::GenServer(inst), state)))
    }

    fn run_genserver(
        &mut self,
        inst: Rc<Instance>,
        state: Value,
        msg: Value,
    ) -> Result<Outcome, String> {
        let kind = hget(&msg, "__kind__").map(|v| v.to_string());
        match kind.as_deref() {
            Some("call") => {
                let payload = hget(&msg, "payload").unwrap_or(Value::Nil);
                let out = self.instance_method(&inst, "handle_call", vec![payload, state.clone()])?;
                // handle_call returns reply(value, new_state).
                let reply_val = hget(&out, "__reply__").unwrap_or(Value::Nil);
                let new_state = hget(&out, "__state__").unwrap_or(state);
                if let Some(Value::Pid(from)) = hget(&msg, "from") {
                    let rf = hget(&msg, "ref").unwrap_or(Value::Nil);
                    let env = envelope(vec![("__ref__", rf), ("__reply__", reply_val)]);
                    self.deliver(from, env);
                }
                Ok(Outcome::State(new_state))
            }
            Some("cast") => {
                let payload = hget(&msg, "payload").unwrap_or(Value::Nil);
                let ns = self.instance_method(&inst, "handle_cast", vec![payload, state])?;
                Ok(Outcome::State(ns))
            }
            // Any other message goes to handle_info if defined, else is ignored.
            _ => {
                if inst.class.find_method("handle_info").is_some() {
                    let ns = self.instance_method(&inst, "handle_info", vec![msg, state])?;
                    Ok(Outcome::State(ns))
                } else {
                    Ok(Outcome::State(state))
                }
            }
        }
    }

    // ---- Supervisor ----

    pub(crate) fn supervisor_static(&mut self, name: &str, argv: Vec<Value>) -> Result<Value, String> {
        if name != "start" {
            return Err(format!("Supervisor has no method '{}'", name));
        }
        let cfg = argv.into_iter().next().unwrap_or(Value::Nil);
        let max_restarts = match hget(&cfg, "max_restarts") {
            Some(Value::Num(n)) => n as usize,
            _ => 5,
        };
        let specs = as_list(hget(&cfg, "children").as_ref());
        // Spawn the supervisor first so children can be monitored by it.
        let sup_pid = self.spawn_proc(
            Handler::Supervisor {
                children: Vec::new(),
                max_restarts,
                restarts: 0,
            },
            Value::Nil,
        );
        let kids = self.start_children(sup_pid, specs)?;
        self.set_supervisor_children(sup_pid, kids);
        Ok(Value::Pid(sup_pid))
    }

    fn start_children(&mut self, sup_pid: u64, specs: Vec<Value>) -> Result<Vec<Child>, String> {
        let mut kids = Vec::new();
        for spec in specs {
            if let Some(child) = self.start_child(sup_pid, &spec)? {
                kids.push(child);
            }
        }
        Ok(kids)
    }

    fn set_supervisor_children(&mut self, sup_pid: u64, kids: Vec<Child>) {
        if let Some(p) = self.procs.get_mut(&sup_pid) {
            if let Handler::Supervisor { children, .. } = &mut p.handler {
                *children = kids;
            }
        }
    }

    // Start one child from its spec, monitored by the supervisor.
    fn start_child(&mut self, sup_pid: u64, spec: &Value) -> Result<Option<Child>, String> {
        let (class, args) = parse_childspec(spec)?;
        let Value::Pid(cpid) = self.genserver_start(&class, args.clone())? else {
            return Ok(None);
        };
        if let Some(p) = self.procs.get_mut(&cpid) {
            p.monitors.push(sup_pid);
        }
        Ok(Some(Child { class, args, pid: cpid }))
    }

    fn run_supervisor(
        &mut self,
        sup_pid: u64,
        handler: Handler,
        msg: Value,
    ) -> Result<Outcome, String> {
        let Handler::Supervisor { mut children, max_restarts, mut restarts } = handler else {
            return Ok(Outcome::State(Value::Nil));
        };
        // A crash (non-normal DOWN) of a known child triggers one restart,
        // while the restart budget lasts.
        if let Some(idx) = crashed_child(&msg, &children) {
            if restarts < max_restarts {
                restarts += 1;
                self.restart_child(sup_pid, &mut children, idx)?;
            }
        }
        Ok(Outcome::Handler(
            Handler::Supervisor { children, max_restarts, restarts },
            Value::Nil,
        ))
    }

    // Re-run a child's spec and hand the fresh process back to this supervisor.
    fn restart_child(
        &mut self,
        sup_pid: u64,
        children: &mut [Child],
        idx: usize,
    ) -> Result<(), String> {
        let child = children[idx].clone();
        if let Value::Pid(np) = self.genserver_start(&child.class, child.args.clone())? {
            if let Some(p) = self.procs.get_mut(&np) {
                p.monitors.push(sup_pid);
            }
            children[idx].pid = np;
        }
        Ok(())
    }

    // ---- Registry ----

    pub(crate) fn registry_method(&mut self, name: &str, argv: Vec<Value>) -> Result<Value, String> {
        match name {
            "register" => {
                let pid = match argv.first() {
                    Some(Value::Pid(id)) => *id,
                    _ => return Err("Registry.register expects (pid, name)".into()),
                };
                let key = argv.get(1).map(|v| v.to_string()).ok_or("Registry.register expects a name")?;
                self.registry.insert(key, pid);
                Ok(Value::Pid(pid))
            }
            "whereis" => {
                let key = argv.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(self.registry.get(&key).map(|id| Value::Pid(*id)).unwrap_or(Value::Nil))
            }
            "unregister" => {
                let key = argv.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(Value::Bool(self.registry.remove(&key).is_some()))
            }
            _ => Err(format!("Registry has no method '{}'", name)),
        }
    }

    // ---- Task (green-thread fan-out) ----

    pub(crate) fn task_method(&mut self, name: &str, argv: Vec<Value>) -> Result<Value, String> {
        match name {
            "async" => {
                let f = argv.into_iter().next().ok_or("Task.async expects a function")?;
                Ok(Value::Pid(self.spawn_task(f)))
            }
            "await" => {
                let pid = match argv.first() {
                    Some(Value::Pid(id)) => *id,
                    _ => return Err("Task.await expects a pid".into()),
                };
                self.run_scheduler();
                Ok(self.procs.get(&pid).map(|p| p.state.clone()).unwrap_or(Value::Nil))
            }
            "parallel" => {
                let fns = as_list(argv.first());
                let pids: Vec<u64> = fns.into_iter().map(|f| self.spawn_task(f)).collect();
                self.run_scheduler();
                let results: Vec<Value> = pids
                    .iter()
                    .map(|id| self.procs.get(id).map(|p| p.state.clone()).unwrap_or(Value::Nil))
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(results))))
            }
            _ => Err(format!("Task has no method '{}'", name)),
        }
    }

    fn spawn_task(&mut self, f: Value) -> u64 {
        let pid = self.spawn_proc(Handler::Task(f), Value::Nil);
        self.deliver(pid, Value::Str("__run__".into()));
        pid
    }

    // ---- dispatch entry points called from the interpreter ----

    // The free-function process builtins. Returns None for a non-scheduler name
    // so the caller can fall back to the plain builtin.
    pub(crate) fn sched_builtin(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let v = match name {
            "spawn" => self.b_spawn(args.to_vec())?,
            "send" => self.b_send(args.to_vec())?,
            "receive" => self.b_receive(args.to_vec())?,
            "monitor" => self.b_monitor(args.to_vec())?,
            "pid" => Value::Pid(self.current_pid),
            "drain" => {
                self.run_scheduler();
                Value::Nil
            }
            _ => return Ok(None),
        };
        Ok(Some(v))
    }

    // Class-level methods on a GenServer subclass. None means "not a GenServer
    // method" — fall through to the normal class dispatch (new/name).
    pub(crate) fn genserver_dispatch(
        &mut self,
        class: &Rc<Class>,
        name: &str,
        args: &[Value],
    ) -> Option<Result<Value, String>> {
        if class.base != "GenServer" {
            return None;
        }
        match name {
            "start" => Some(self.genserver_start(class, args.to_vec())),
            "child" => Some(Ok(child_spec(class, args.to_vec()))),
            _ => None,
        }
    }

    // Method dispatch for the OTP object primitives (Registry/Supervisor/Task).
    pub(crate) fn otp_object_method(
        &mut self,
        object: &str,
        name: &str,
        argv: Vec<Value>,
    ) -> Result<Value, String> {
        match object {
            "Registry" => self.registry_method(name, argv),
            "Supervisor" => self.supervisor_static(name, argv),
            "Task" => self.task_method(name, argv),
            _ => Err(format!("{} has no method '{}'", object, name)),
        }
    }
}

// The capitalized builtins whose methods the process runtime dispatches.
pub(crate) fn is_otp_object(name: &str) -> bool {
    matches!(name, "Registry" | "Supervisor" | "Task")
}

// A supervisor child spec: how to (re)start a GenServer — its class and args.
fn child_spec(class: &Rc<Class>, args: Vec<Value>) -> Value {
    envelope(vec![
        ("__childspec__", Value::Bool(true)),
        ("class", Value::Class(class.clone())),
        ("args", Value::Array(Rc::new(RefCell::new(args)))),
    ])
}

// ---- scheduler helpers ----

fn envelope(pairs: Vec<(&str, Value)>) -> Value {
    let mut h = HashMap::new();
    for (k, v) in pairs {
        h.insert(k.to_string(), v);
    }
    Value::Hash(Rc::new(RefCell::new(h)))
}

// The index of the child killed by a DOWN message, if it was an abnormal crash
// of a process this supervisor manages. Normal exits don't restart.
fn crashed_child(msg: &Value, children: &[Child]) -> Option<usize> {
    if !matches!(hget(msg, "down"), Some(Value::Bool(true))) {
        return None;
    }
    if hget(msg, "reason").map(|v| v.to_string()).as_deref() == Some("normal") {
        return None;
    }
    let Some(Value::Pid(dead)) = hget(msg, "pid") else {
        return None;
    };
    children.iter().position(|c| c.pid == dead)
}

fn down_msg(pid: u64, reason: &str) -> Value {
    envelope(vec![
        ("down", Value::Bool(true)),
        ("pid", Value::Pid(pid)),
        ("reason", Value::Str(reason.to_string())),
    ])
}

fn hget(m: &Value, key: &str) -> Option<Value> {
    match m {
        Value::Hash(h) => h.borrow().get(key).cloned(),
        _ => None,
    }
}

fn ref_of(m: &Value) -> Option<u64> {
    match hget(m, "__ref__") {
        Some(Value::Num(n)) => Some(n as u64),
        _ => None,
    }
}

fn as_list(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.borrow().clone(),
        Some(other) => vec![other.clone()],
        None => Vec::new(),
    }
}

// A supervisor child spec is a GenServer class (started with no args) or a hash
// carrying its class and construction args (produced by `SomeServer.child(...)`).
fn parse_childspec(spec: &Value) -> Result<(Rc<Class>, Vec<Value>), String> {
    match spec {
        Value::Class(c) => Ok((c.clone(), Vec::new())),
        Value::Hash(_) => match hget(spec, "class") {
            Some(Value::Class(c)) => Ok((c, as_list(hget(spec, "args").as_ref()))),
            _ => Err("child spec needs a GenServer 'class:'".into()),
        },
        other => Err(format!("invalid child spec: {}", other.type_name())),
    }
}
