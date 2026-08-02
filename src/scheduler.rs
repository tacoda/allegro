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

pub const ROOT: u64 = 0;

// What runs when a process receives a message.
pub enum Handler {
    Root,             // the top-level flow; never auto-stepped
    Module(String),   // dispatch to `Module.handle/2`
    Fun(Rc<Fun>),     // an anonymous `fn state, msg -> ...`
}

pub struct Proc {
    pub handler: Handler,
    pub state: Value,
    pub mailbox: VecDeque<Value>,
    pub alive: bool,
    pub monitors: Vec<u64>, // pids to notify with {:DOWN, pid, reason} on death
}

pub struct Scheduler {
    next: u64,
    procs: HashMap<u64, Proc>,
    ready: VecDeque<u64>, // actor pids with pending mail
    pub current: u64,     // pid whose flow is executing (ROOT while at top level)
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
            current: ROOT,
        }
    }

    pub fn spawn(&mut self, handler: Handler, state: Value) -> u64 {
        let pid = self.next;
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
    pub fn deliver(&mut self, pid: u64, msg: Value) {
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

    pub fn is_alive(&self, pid: u64) -> bool {
        self.procs.get(&pid).map_or(false, |p| p.alive)
    }

    pub fn monitor(&mut self, watcher: u64, target: u64) {
        if let Some(p) = self.procs.get_mut(&target) {
            p.monitors.push(watcher);
        }
    }

    // Pop the next ready actor and one of its messages, if any.
    pub fn next_ready(&mut self) -> Option<(u64, Value)> {
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

    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    pub fn state_of(&self, pid: u64) -> Value {
        self.procs.get(&pid).map_or(Value::Nil, |p| p.state.clone())
    }

    pub fn set_state(&mut self, pid: u64, state: Value) {
        if let Some(p) = self.procs.get_mut(&pid) {
            p.state = state;
        }
    }

    pub fn handler_of(&self, pid: u64) -> Option<HandlerRef> {
        self.procs.get(&pid).map(|p| match &p.handler {
            Handler::Root => HandlerRef::Root,
            Handler::Module(m) => HandlerRef::Module(m.clone()),
            Handler::Fun(f) => HandlerRef::Fun(f.clone()),
        })
    }

    // Take a message out of a process's mailbox at `idx` (used by `receive`
    // after a clause matches). Returns the removed message.
    pub fn take_message(&mut self, pid: u64, idx: usize) -> Option<Value> {
        self.procs.get_mut(&pid).and_then(|p| p.mailbox.remove(idx))
    }

    pub fn mailbox_snapshot(&self, pid: u64) -> Vec<Value> {
        self.procs
            .get(&pid)
            .map(|p| p.mailbox.iter().cloned().collect())
            .unwrap_or_default()
    }

    // Mark a process dead and return its monitors so the caller can deliver
    // `{:DOWN, pid, reason}` to each.
    pub fn kill(&mut self, pid: u64) -> Vec<u64> {
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
