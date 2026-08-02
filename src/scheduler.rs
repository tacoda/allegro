// The actor scheduler: cooperative, single-threaded, run-to-completion.
//
// A process is `state + a handler`; the scheduler delivers one message at a
// time and the handler runs to completion (it never blocks). Only the root
// flow (pid 0) drives the loop and may `receive` — it sits at the bottom of
// the Rust stack, so "blocking" there is just running the driver inline.
//
// Message isolation is free: values are immutable `Rc`, so a message shared
// between processes is indistinguishable from a copy.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::value::{Fun, Value};

// A process identifier. A newtype (not a bare integer) so the scheduler API
// speaks in domain terms and pids never mix with other counts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Pid(pub u64);

impl Pid {
    pub fn id(self) -> u64 {
        self.0
    }
}

pub const ROOT: Pid = Pid(0);

// What runs when a process receives a message.
pub enum Handler {
    Root,           // the top-level flow; never auto-stepped
    Module(String), // dispatch to `Module.handle/2`
    Fun(Rc<Fun>),   // an anonymous `fn state, msg -> ...`
}

pub struct Proc {
    pub handler: Handler,
    pub state: Value,
    pub mailbox: VecDeque<Value>,
    pub alive: bool,
    pub monitors: Vec<Pid>, // pids to notify with {:DOWN, pid, reason} on death
}

pub struct Scheduler {
    next: u64,
    procs: HashMap<Pid, Proc>,
    ready: VecDeque<Pid>,        // actor pids with pending mail
    names: HashMap<String, Pid>, // the registry: name -> pid
    pub current: Pid,            // pid whose flow is executing (ROOT at top level)
}

impl Scheduler {
    pub fn new() -> Scheduler {
        let mut procs = HashMap::new();
        procs.insert(
            ROOT,
            Proc {
                handler: Handler::Root,
                state: Value::Nil,
                mailbox: VecDeque::new(),
                alive: true,
                monitors: Vec::new(),
            },
        );
        Scheduler {
            next: 1,
            procs,
            ready: VecDeque::new(),
            names: HashMap::new(),
            current: ROOT,
        }
    }

    // Registry: bind a name to a pid; resolve a live pid by name.
    pub fn register(&mut self, name: String, pid: Pid) {
        self.names.insert(name, pid);
    }

    pub fn whereis(&self, name: &str) -> Option<Pid> {
        self.names.get(name).copied().filter(|pid| self.is_alive(*pid))
    }

    pub fn spawn(&mut self, handler: Handler, state: Value) -> Pid {
        let pid = Pid(self.next);
        self.next += 1;
        self.procs.insert(
            pid,
            Proc {
                handler,
                state,
                mailbox: VecDeque::new(),
                alive: true,
                monitors: Vec::new(),
            },
        );
        pid
    }

    // Deliver a message; a live actor with new mail becomes ready to step.
    pub fn deliver(&mut self, pid: Pid, msg: Value) {
        if let Some(p) = self.procs.get_mut(&pid) {
            if !p.alive {
                return;
            }
            p.mailbox.push_back(msg);
            if pid != ROOT && !self.ready.contains(&pid) {
                self.ready.push_back(pid);
            }
        }
    }

    pub fn is_alive(&self, pid: Pid) -> bool {
        self.procs.get(&pid).map_or(false, |p| p.alive)
    }

    pub fn monitor(&mut self, watcher: Pid, target: Pid) {
        if let Some(p) = self.procs.get_mut(&target) {
            p.monitors.push(watcher);
        }
    }

    // Pop the next ready actor and one of its messages, if any.
    pub fn next_ready(&mut self) -> Option<(Pid, Value)> {
        while let Some(pid) = self.ready.pop_front() {
            let Some(p) = self.procs.get_mut(&pid) else { continue };
            if !p.alive {
                continue;
            }
            let Some(msg) = p.mailbox.pop_front() else { continue };
            if !p.mailbox.is_empty() {
                self.ready.push_back(pid); // more mail: revisit later
            }
            return Some((pid, msg));
        }
        None
    }

    pub fn state_of(&self, pid: Pid) -> Value {
        self.procs.get(&pid).map_or(Value::Nil, |p| p.state.clone())
    }

    pub fn set_state(&mut self, pid: Pid, state: Value) {
        if let Some(p) = self.procs.get_mut(&pid) {
            p.state = state;
        }
    }

    pub fn handler_of(&self, pid: Pid) -> Option<HandlerRef> {
        self.procs.get(&pid).map(|p| match &p.handler {
            Handler::Root => HandlerRef::Root,
            Handler::Module(m) => HandlerRef::Module(m.clone()),
            Handler::Fun(f) => HandlerRef::Fun(f.clone()),
        })
    }

    // Take a message out of a process's mailbox at `idx` (used by `receive`
    // after a clause matches). Returns the removed message.
    pub fn take_message(&mut self, pid: Pid, idx: usize) -> Option<Value> {
        self.procs.get_mut(&pid).and_then(|p| p.mailbox.remove(idx))
    }

    pub fn mailbox_snapshot(&self, pid: Pid) -> Vec<Value> {
        self.procs
            .get(&pid)
            .map(|p| p.mailbox.iter().cloned().collect())
            .unwrap_or_default()
    }

    // Mark a process dead and return its monitors so the caller can deliver
    // `{:DOWN, pid, reason}` to each.
    pub fn kill(&mut self, pid: Pid) -> Vec<Pid> {
        match self.procs.get_mut(&pid) {
            Some(p) => {
                p.alive = false;
                std::mem::take(&mut p.monitors)
            }
            None => Vec::new(),
        }
    }
}

// A snapshot of a process's handler, safe to hold while re-borrowing the
// scheduler during a step.
pub enum HandlerRef {
    Root,
    Module(String),
    Fun(Rc<Fun>),
}
