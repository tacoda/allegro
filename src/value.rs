use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::Stmt;
use crate::openai::Agent;

pub type BuiltinFn = fn(&[Value]) -> Result<Value, String>;

#[derive(Clone)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Hash(Rc<RefCell<HashMap<String, Value>>>),
    // Harness primitives
    Model(Rc<ModelObj>),
    Agent(Rc<AgentObj>),
    Subagent(Rc<Subagent>),
    Tool(Rc<Tool>),
    Rule(Rc<Rule>),
    Skill(Rc<Skill>),
    Hook(Rc<Hook>),
    Command(Rc<Command>),
    Graph(Rc<Graph>),
    Factory(Rc<Factory>),
    Charter(Rc<Charter>),
    Harness(Rc<Harness>),
    // User-defined subclasses of the primitives, and their instances
    Class(Rc<Class>),
    Instance(Rc<Instance>),
    // Core data types produced by the primitives
    Message(Rc<Message>),
    HookResult(Rc<HookResult>),
    // Callables
    Func(Rc<Func>),
    Builtin(&'static str, BuiltinFn),
}

pub struct Func {
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub closure: Env,
}

// A named, specialized delegate (the Claude Code "agent" primitive): a worker
// agent plus a description of when to use it. A top-level agent delegates to it.
pub struct Subagent {
    pub name: String,
    pub description: String,
    pub agent: Value, // Value::Agent — the worker that does the work
}

// A model provider + name. Forward-looking: only "openai" is wired up today.
pub struct ModelObj {
    pub provider: String,
    pub name: String,
    pub temperature: f64,
}

// A callable the model can invoke during a run (OpenAI function calling).
// `action` is a function that takes the tool's string input and returns a result.
pub struct Tool {
    pub name: String,
    pub description: String,
    pub action: Value,
}

// A configured agent: an LLM core plus the harness machinery wrapped around it.
pub struct AgentObj {
    pub name: String,
    pub core: Agent,
    pub before: Vec<Value>,          // hook functions run on input
    pub after: Vec<Value>,           // hook functions run on output
    pub skills: Vec<Rc<Skill>>,      // named capabilities
    pub tools: Vec<Rc<Tool>>,        // callables the model may invoke
    pub subagents: Vec<(String, Value)>, // delegates, keyed by name
}

// A named constraint injected into an agent's system prompt.
pub struct Rule {
    pub name: String,
    pub text: String,
}

// A reusable, named instruction block an agent can apply on demand.
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum HookEvent {
    BeforeRun,
    AfterRun,
}

// Intercepts an agent's input or output. `action` is an allegro function.
pub struct Hook {
    pub event: HookEvent,
    pub action: Value,
}

// A user-invokable named workflow (a slash command). `action` is a function.
pub struct Command {
    pub name: String,
    pub description: String,
    pub action: Value,
}

// A stateful workflow: nodes (agents or functions) wired by edges. Running it
// walks from `entry`, feeding each node's output to the next until an edge
// resolves to "end" (langgraph-style routing).
pub struct Graph {
    pub entry: String,
    pub nodes: Vec<(String, Value)>, // name -> agent or function
    pub edges: Vec<(String, Value)>, // name -> next-name string, or router function
    pub max_steps: usize,
}

impl Graph {
    pub fn node(&self, name: &str) -> Option<&Value> {
        self.nodes.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn edge(&self, name: &str) -> Option<&Value> {
        self.edges.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

// Produces configured primitives on demand from a spec. `build` is a function
// spec -> value (usually an agent), so one definition can stamp out many.
pub struct Factory {
    pub build: Value,
}

// A pure definition: the rules, hooks, skills, and commands that govern an
// agent. Not invocable on its own — it is the input to a harness.
pub struct Charter {
    pub rules: Vec<Rc<Rule>>,
    pub before: Vec<Value>,       // before_run hook functions
    pub after: Vec<Value>,        // after_run hook functions
    pub skills: Vec<Rc<Skill>>,
    pub commands: Vec<Rc<Command>>,
}

// Intakes a charter (and optionally a graph). A harness plus a model makes an
// agent; a harness with a graph can be invoked on its own.
pub struct Harness {
    pub charter: Option<Rc<Charter>>,
    pub graph: Option<Value>, // Value::Graph
}

// A user-defined subclass of a primitive. `base` is the primitive kind it
// ultimately builds ("agent", "harness", "graph", "factory", "charter", ...).
// Methods override or extend the base; `config` supplies build config.
pub struct Class {
    pub name: String,
    pub base: String,
    pub parent: Option<Rc<Class>>,
    pub methods: HashMap<String, Rc<Func>>,
}

impl Class {
    // Walk the inheritance chain (most-derived first) for a method.
    pub fn find_method(&self, name: &str) -> Option<Rc<Func>> {
        if let Some(m) = self.methods.get(name) {
            Some(m.clone())
        } else if let Some(p) = &self.parent {
            p.find_method(name)
        } else {
            None
        }
    }
}

// A live instance of a Class: the built primitive plus its class for dispatch.
pub struct Instance {
    pub class: Rc<Class>,
    pub base: Value,
    pub ivars: RefCell<HashMap<String, Value>>,
}

// The output of an agent run — a structured message, not a bare string.
pub struct Message {
    pub content: String,
    pub role: String,
    pub from: String, // name of the agent that produced it
}

// The result of invoking a hook: the (possibly transformed) payload plus
// whether the pipeline should stop here.
pub struct HookResult {
    pub value: Value,
    pub halt: bool,
}

impl Value {
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "bool",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Array(_) => "array",
            Value::Hash(_) => "hash",
            Value::Model(_) => "model",
            Value::Agent(_) => "agent",
            Value::Subagent(_) => "subagent",
            Value::Tool(_) => "tool",
            Value::Rule(_) => "rule",
            Value::Skill(_) => "skill",
            Value::Hook(_) => "hook",
            Value::Command(_) => "command",
            Value::Graph(_) => "graph",
            Value::Factory(_) => "factory",
            Value::Charter(_) => "charter",
            Value::Harness(_) => "harness",
            Value::Class(_) => "class",
            Value::Instance(_) => "instance",
            Value::Message(_) => "message",
            Value::HookResult(_) => "hook_result",
            Value::Func(_) => "function",
            Value::Builtin(_, _) => "builtin",
        }
    }

    // Like Display but quotes strings, for nesting inside collections.
    pub fn inspect(&self) -> String {
        match self {
            Value::Str(s) => format!("{:?}", s),
            other => other.to_string(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Nil => write!(f, "nil"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Str(s) => write!(f, "{}", s),
            Value::Array(a) => {
                let items: Vec<String> = a.borrow().iter().map(|v| v.inspect()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Value::Hash(h) => {
                let items: Vec<String> = h
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.inspect()))
                    .collect();
                write!(f, "{{{}}}", items.join(", "))
            }
            Value::Model(m) => write!(f, "#<model {}/{}>", m.provider, m.name),
            Value::Agent(a) => write!(f, "#<agent {} model={}>", a.name, a.core.model),
            Value::Subagent(s) => write!(f, "#<subagent {}>", s.name),
            Value::Tool(t) => write!(f, "#<tool {}>", t.name),
            Value::Rule(r) => write!(f, "#<rule {}>", r.name),
            Value::Skill(s) => write!(f, "#<skill {}>", s.name),
            Value::Hook(h) => {
                let ev = match h.event {
                    HookEvent::BeforeRun => "before_run",
                    HookEvent::AfterRun => "after_run",
                };
                write!(f, "#<hook {}>", ev)
            }
            Value::Command(c) => write!(f, "#<command {}>", c.name),
            Value::Graph(g) => write!(f, "#<graph entry={}>", g.entry),
            Value::Factory(_) => write!(f, "#<factory>"),
            Value::Charter(c) => write!(
                f,
                "#<charter rules={} hooks={} skills={} commands={}>",
                c.rules.len(),
                c.before.len() + c.after.len(),
                c.skills.len(),
                c.commands.len()
            ),
            Value::Harness(_) => write!(f, "#<harness>"),
            Value::Class(c) => write!(f, "#<class {} < {}>", c.name, c.base),
            Value::Instance(i) => write!(f, "#<{} instance>", i.class.name),
            // A message renders as its content so it prints like a string.
            Value::Message(m) => write!(f, "{}", m.content),
            Value::HookResult(r) => write!(f, "{}", r.value),
            Value::Func(_) => write!(f, "#<function>"),
            Value::Builtin(n, _) => write!(f, "#<builtin {}>", n),
        }
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

    // Assign to an existing binding in the scope chain, else define locally.
    pub fn set(&mut self, name: &str, val: Value) {
        if self.vars.contains_key(name) {
            self.vars.insert(name.to_string(), val);
        } else if let Some(p) = &self.parent {
            if p.borrow().get(name).is_some() {
                p.borrow_mut().set(name, val);
                return;
            }
            self.vars.insert(name.to_string(), val);
        } else {
            self.vars.insert(name.to_string(), val);
        }
    }

    pub fn define(&mut self, name: &str, val: Value) {
        self.vars.insert(name.to_string(), val);
    }
}
