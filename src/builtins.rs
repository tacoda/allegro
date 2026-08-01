use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::openai::Agent;
use crate::value::{
    AgentObj, Charter, Command, Env, Factory, Graph, Harness, Hook, HookEvent, HookResult, Message,
    ModelObj, Rule, Skill, Subagent, Tool, Value,
};

pub fn register(env: &Env) {
    let mut e = env.borrow_mut();
    // I/O and conversions
    e.define("puts", Value::Builtin("puts", b_puts));
    e.define("print", Value::Builtin("print", b_print));
    e.define("len", Value::Builtin("len", b_len));
    e.define("str", Value::Builtin("str", b_str));
    e.define("num", Value::Builtin("num", b_num));
    e.define("range", Value::Builtin("range", b_range));
    e.define("type_of", Value::Builtin("type_of", b_type_of));
    // patterns
    e.define("fan_out", Value::Builtin("fan_out", b_fan_out));
    e.define("pipeline", Value::Builtin("pipeline", b_pipeline));
    // data types
    e.define("halt", Value::Builtin("halt", b_halt));
    e.define("keep", Value::Builtin("keep", b_keep));
    e.define("message", Value::Builtin("message", b_message));
    // primitive constructors — built up as definitions, then invoked
    e.define("model", Value::Builtin("model", b_model));
    e.define("agent", Value::Builtin("agent", b_agent));
    e.define("subagent", Value::Builtin("subagent", b_subagent));
    e.define("tool", Value::Builtin("tool", b_tool));
    e.define("rule", Value::Builtin("rule", b_rule));
    e.define("skill", Value::Builtin("skill", b_skill));
    e.define("hook", Value::Builtin("hook", b_hook));
    e.define("command", Value::Builtin("command", b_command));
    e.define("graph", Value::Builtin("graph", b_graph));
    e.define("factory", Value::Builtin("factory", b_factory));
    e.define("charter", Value::Builtin("charter", b_charter));
    e.define("harness", Value::Builtin("harness", b_harness));
}

// Build a non-agent primitive by kind from a config hash. Used by subclass
// instantiation (`Name.new`). Agents are built by the interpreter.
pub(crate) fn make(kind: &str, cfg: Value) -> Result<Value, String> {
    let args = [cfg];
    match kind {
        "model" => b_model(&args),
        "agent" => b_agent(&args),
        "subagent" => b_subagent(&args),
        "tool" => b_tool(&args),
        "rule" => b_rule(&args),
        "skill" => b_skill(&args),
        "hook" => b_hook(&args),
        "command" => b_command(&args),
        "graph" => b_graph(&args),
        "factory" => b_factory(&args),
        "charter" => b_charter(&args),
        "harness" => b_harness(&args),
        other => Err(format!("cannot subclass '{}'", other)),
    }
}

// ---- config-hash helpers for the constructors ----

fn cfg<'a>(args: &'a [Value], what: &str) -> Result<&'a Rc<RefCell<HashMap<String, Value>>>, String> {
    match args.first() {
        Some(Value::Hash(h)) => Ok(h),
        _ => Err(format!("{} expects a config hash", what)),
    }
}

fn get_str(h: &HashMap<String, Value>, key: &str, default: &str) -> String {
    h.get(key).map(|v| v.to_string()).unwrap_or_else(|| default.to_string())
}

fn get_pairs(h: &HashMap<String, Value>, key: &str) -> Vec<(String, Value)> {
    match h.get(key) {
        Some(Value::Hash(m)) => m.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Vec::new(),
    }
}

fn b_model(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "model")?.borrow();
    let temperature = match h.get("temperature") {
        Some(Value::Num(n)) => *n,
        _ => 0.7,
    };
    Ok(Value::Model(Rc::new(ModelObj {
        provider: get_str(&h, "provider", "openai"),
        name: get_str(&h, "name", "gpt-4o-mini"),
        temperature,
    })))
}

fn b_agent(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "agent")?.borrow();
    build_agent(&h)
}

// A subagent wraps a worker agent with a name and a "when to use" description.
// It either adopts an existing agent (`agent:`) or builds one from the config.
fn b_subagent(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "subagent")?.borrow();
    let agent = match h.get("agent") {
        Some(v @ Value::Agent(_)) => v.clone(),
        Some(other) => {
            return Err(format!("subagent 'agent:' must be an agent, got {}", other.type_name()))
        }
        None => build_agent(&h)?,
    };
    Ok(Value::Subagent(Rc::new(Subagent {
        name: get_str(&h, "name", "subagent"),
        description: get_str(&h, "description", ""),
        agent,
    })))
}

// Assemble an agent: compose its system prompt from the base system plus any
// attached rules and skills, and wire up hooks and sub-agents.
pub(crate) fn build_agent(cfg: &HashMap<String, Value>) -> Result<Value, String> {
    // `model:` may be a model primitive or a bare string (openai shorthand).
    let (provider, model, model_temp) = match cfg.get("model") {
        Some(Value::Model(m)) => (m.provider.clone(), m.name.clone(), m.temperature),
        Some(other) => ("openai".to_string(), other.to_string(), 0.7),
        None => ("openai".to_string(), "gpt-4o-mini".to_string(), 0.7),
    };
    // An explicit agent temperature overrides the model's default.
    let temperature = match cfg.get("temperature") {
        Some(Value::Num(n)) => *n,
        _ => model_temp,
    };

    // Governance (rules, hooks, skills) comes from an attached harness or
    // charter, plus any inline config — an agent is a harness plus a model.
    let mut rules: Vec<Rc<Rule>> = Vec::new();
    let mut skills: Vec<Rc<Skill>> = Vec::new();
    let (mut before, mut after) = (Vec::new(), Vec::new());

    let mut charters: Vec<Rc<Charter>> = Vec::new();
    if let Some(Value::Harness(h)) = cfg.get("harness") {
        if let Some(c) = &h.charter {
            charters.push(c.clone());
        }
    }
    if let Some(Value::Charter(c)) = cfg.get("charter") {
        charters.push(c.clone());
    }
    for c in &charters {
        rules.extend(c.rules.iter().cloned());
        skills.extend(c.skills.iter().cloned());
        before.extend(c.before.iter().cloned());
        after.extend(c.after.iter().cloned());
    }
    // inline rules/skills/hooks
    for v in value_list(cfg.get("rules")) {
        if let Value::Rule(r) = v {
            rules.push(r);
        }
    }
    for v in value_list(cfg.get("skills")) {
        if let Value::Skill(s) = v {
            skills.push(s);
        }
    }
    for v in value_list(cfg.get("hooks")) {
        if let Value::Hook(hook) = &v {
            match hook.event {
                HookEvent::BeforeRun => before.push(v),
                HookEvent::AfterRun => after.push(v),
            }
        }
    }

    let mut system = cfg.get("system").map(|v| v.to_string()).unwrap_or_default();
    if !rules.is_empty() {
        system.push_str("\n\n# Rules\n");
        let lines: Vec<String> = rules.iter().map(|r| format!("- {}", r.text)).collect();
        system.push_str(&lines.join("\n"));
    }
    if !skills.is_empty() {
        system.push_str("\n\n# Skills\n");
        for s in &skills {
            system.push_str(&format!("- {}: {}\n", s.name, s.description));
        }
    }

    let tools: Vec<Rc<Tool>> = value_list(cfg.get("tools"))
        .into_iter()
        .filter_map(|v| if let Value::Tool(t) = v { Some(t) } else { None })
        .collect();

    // Subagents this agent can delegate to, keyed by name.
    let mut subagents: Vec<(String, Value)> = Vec::new();
    for v in value_list(cfg.get("subagents")).into_iter().chain(value_list(cfg.get("agents"))) {
        match &v {
            Value::Subagent(s) => subagents.push((s.name.clone(), v.clone())),
            Value::Agent(a) => subagents.push((a.name.clone(), v.clone())),
            _ => {}
        }
    }

    let name = cfg
        .get("name")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "agent".to_string());

    let core = Agent::new(&provider, model, system, temperature)?;
    Ok(Value::Agent(Rc::new(AgentObj {
        name,
        core,
        before,
        after,
        skills,
        tools,
        subagents,
    })))
}

fn b_tool(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "tool")?.borrow();
    let action = h
        .get("run")
        .or_else(|| h.get("do"))
        .cloned()
        .ok_or("tool needs a 'run:' function")?;
    Ok(Value::Tool(Rc::new(Tool {
        name: get_str(&h, "name", "tool"),
        description: get_str(&h, "description", ""),
        action,
    })))
}

fn b_rule(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "rule")?.borrow();
    Ok(Value::Rule(Rc::new(Rule {
        name: get_str(&h, "name", "rule"),
        text: get_str(&h, "text", ""),
    })))
}

fn b_skill(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "skill")?.borrow();
    Ok(Value::Skill(Rc::new(Skill {
        name: get_str(&h, "name", "skill"),
        description: get_str(&h, "description", ""),
        instructions: get_str(&h, "instructions", ""),
    })))
}

fn b_hook(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "hook")?.borrow();
    let event = match get_str(&h, "on", "before_run").as_str() {
        "after_run" | "after" => HookEvent::AfterRun,
        _ => HookEvent::BeforeRun,
    };
    let action = h
        .get("do")
        .or_else(|| h.get("run"))
        .cloned()
        .ok_or("hook needs a 'do:' function")?;
    Ok(Value::Hook(Rc::new(Hook { event, action })))
}

fn b_command(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "command")?.borrow();
    let action = h
        .get("run")
        .or_else(|| h.get("do"))
        .cloned()
        .ok_or("command needs a 'run:' function")?;
    Ok(Value::Command(Rc::new(Command {
        name: get_str(&h, "name", "command"),
        description: get_str(&h, "description", ""),
        action,
    })))
}

fn b_graph(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "graph")?.borrow();
    let max_steps = match h.get("max_steps") {
        Some(Value::Num(n)) => *n as usize,
        _ => 100,
    };
    Ok(Value::Graph(Rc::new(Graph {
        entry: get_str(&h, "entry", "start"),
        nodes: get_pairs(&h, "nodes"),
        edges: get_pairs(&h, "edges"),
        max_steps,
    })))
}

fn b_factory(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "factory")?.borrow();
    let build = h.get("build").cloned().ok_or("factory needs a 'build:' function")?;
    Ok(Value::Factory(Rc::new(Factory { build })))
}

fn value_list(v: Option<&Value>) -> Vec<Value> {
    match v {
        Some(Value::Array(a)) => a.borrow().clone(),
        Some(other) => vec![other.clone()],
        None => Vec::new(),
    }
}

fn b_charter(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "charter")?.borrow();
    let rules = value_list(h.get("rules"))
        .into_iter()
        .filter_map(|v| if let Value::Rule(r) = v { Some(r) } else { None })
        .collect();
    let skills = value_list(h.get("skills"))
        .into_iter()
        .filter_map(|v| if let Value::Skill(s) = v { Some(s) } else { None })
        .collect();
    let commands = value_list(h.get("commands"))
        .into_iter()
        .filter_map(|v| if let Value::Command(c) = v { Some(c) } else { None })
        .collect();
    let (mut before, mut after) = (Vec::new(), Vec::new());
    for hv in value_list(h.get("hooks")) {
        if let Value::Hook(hook) = &hv {
            match hook.event {
                HookEvent::BeforeRun => before.push(hv),
                HookEvent::AfterRun => after.push(hv),
            }
        }
    }
    Ok(Value::Charter(Rc::new(Charter {
        rules,
        before,
        after,
        skills,
        commands,
    })))
}

fn b_harness(args: &[Value]) -> Result<Value, String> {
    let h = cfg(args, "harness")?.borrow();
    let graph = match h.get("graph") {
        Some(v @ Value::Graph(_)) => Some(v.clone()),
        Some(other) => {
            return Err(format!("harness 'graph:' must be a graph, got {}", other.type_name()))
        }
        None => None,
    };
    let charter = match h.get("charter") {
        Some(Value::Charter(c)) => Some(c.clone()),
        Some(other) => {
            return Err(format!("harness 'charter:' must be a charter, got {}", other.type_name()))
        }
        None => None,
    };
    if charter.is_none() && graph.is_none() {
        return Err("harness needs a 'charter:' or a 'graph:'".into());
    }
    Ok(Value::Harness(Rc::new(Harness { charter, graph })))
}

fn b_puts(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        println!();
    }
    for a in args {
        println!("{}", a);
    }
    Ok(Value::Nil)
}

fn b_print(args: &[Value]) -> Result<Value, String> {
    let line: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    print!("{}", line.join(""));
    Ok(Value::Nil)
}

fn b_len(args: &[Value]) -> Result<Value, String> {
    let v = args.first().ok_or("len expects an argument")?;
    let n = match v {
        Value::Str(s) => s.chars().count(),
        Value::Array(a) => a.borrow().len(),
        Value::Hash(h) => h.borrow().len(),
        other => return Err(format!("cannot take len of {}", other.type_name())),
    };
    Ok(Value::Num(n as f64))
}

fn b_str(args: &[Value]) -> Result<Value, String> {
    let v = args.first().ok_or("str expects an argument")?;
    Ok(Value::Str(v.to_string()))
}

fn b_num(args: &[Value]) -> Result<Value, String> {
    let v = args.first().ok_or("num expects an argument")?;
    match v {
        Value::Num(n) => Ok(Value::Num(*n)),
        Value::Str(s) => s
            .trim()
            .parse::<f64>()
            .map(Value::Num)
            .map_err(|_| format!("cannot parse '{}' as a number", s)),
        other => Err(format!("cannot convert {} to a number", other.type_name())),
    }
}

// halt(value) -> a hook_result that stops the agent pipeline with `value`.
fn b_halt(args: &[Value]) -> Result<Value, String> {
    let value = args.first().cloned().unwrap_or(Value::Nil);
    Ok(Value::HookResult(Rc::new(HookResult { value, halt: true })))
}

// keep(value) -> a hook_result that continues with `value` (explicit form).
fn b_keep(args: &[Value]) -> Result<Value, String> {
    let value = args.first().cloned().unwrap_or(Value::Nil);
    Ok(Value::HookResult(Rc::new(HookResult { value, halt: false })))
}

fn b_message(args: &[Value]) -> Result<Value, String> {
    let content = args.first().map(|v| v.to_string()).unwrap_or_default();
    let from = args.get(1).map(|v| v.to_string()).unwrap_or_else(|| "user".into());
    Ok(Value::Message(Rc::new(Message {
        content,
        role: "user".to_string(),
        from,
    })))
}

fn b_type_of(args: &[Value]) -> Result<Value, String> {
    let v = args.first().ok_or("type_of expects an argument")?;
    Ok(Value::Str(v.type_name().to_string()))
}

fn b_range(args: &[Value]) -> Result<Value, String> {
    let nums: Vec<f64> = args
        .iter()
        .map(|v| match v {
            Value::Num(n) => Ok(*n),
            other => Err(format!("range expects numbers, got {}", other.type_name())),
        })
        .collect::<Result<_, _>>()?;
    let (start, end) = match nums.as_slice() {
        [n] => (0i64, *n as i64),
        [a, b] => (*a as i64, *b as i64),
        _ => return Err("range expects range(n) or range(start, end)".into()),
    };
    let vals: Vec<Value> = (start..end).map(|i| Value::Num(i as f64)).collect();
    Ok(Value::Array(Rc::new(RefCell::new(vals))))
}

// fan_out(agent, [inputs]) -> [messages] — runs the agent on each input concurrently.
fn b_fan_out(args: &[Value]) -> Result<Value, String> {
    let agent = match args.first() {
        Some(Value::Agent(a)) => a,
        _ => return Err("fan_out expects an agent as its first argument".into()),
    };
    let inputs: Vec<String> = match args.get(1) {
        Some(Value::Array(items)) => items.borrow().iter().map(|v| v.to_string()).collect(),
        Some(other) => vec![other.to_string()],
        None => return Err("fan_out expects a list of inputs".into()),
    };
    let results = agent.core.fan_out(inputs)?;
    let vals: Vec<Value> = results
        .into_iter()
        .map(|c| {
            Value::Message(Rc::new(Message {
                content: c,
                role: "assistant".to_string(),
                from: agent.name.clone(),
            }))
        })
        .collect();
    Ok(Value::Array(Rc::new(RefCell::new(vals))))
}

// pipeline(input, agent1, agent2, ...) -> feeds each agent's output into the next.
fn b_pipeline(args: &[Value]) -> Result<Value, String> {
    let mut acc = match args.first() {
        Some(v) => v.to_string(),
        None => return Err("pipeline expects an input and at least one agent".into()),
    };
    for a in &args[1..] {
        match a {
            Value::Agent(agent) => acc = agent.core.run(&acc)?,
            other => return Err(format!("pipeline expects agents, got {}", other.type_name())),
        }
    }
    Ok(Value::Str(acc))
}
