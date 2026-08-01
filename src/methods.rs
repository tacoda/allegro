use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{Message, Value};

// Dispatch for value types whose methods are pure (no user-function calls).
// Agent and Command methods are handled in the interpreter because they invoke
// user-defined functions (hooks, command bodies).
pub fn dispatch(recv: Value, name: &str, args: Vec<Value>) -> Result<Value, String> {
    match &recv {
        Value::Str(s) => string_method(s, name, &args),
        Value::Array(a) => array_method(a, name, &args),
        Value::Hash(h) => hash_method(h, name, &args),
        Value::Num(n) => number_method(*n, name),
        Value::Memory(m) => memory_method(m, name, &args),
        Value::Message(m) => message_method(m, name),
        Value::HookResult(r) => hook_result_method(r, name),
        Value::Model(m) => match name {
            "name" => Ok(Value::Str(m.name.clone())),
            "provider" => Ok(Value::Str(m.provider.clone())),
            "temperature" => Ok(Value::Num(m.temperature)),
            _ => Err(format!("model has no method '{}'", name)),
        },
        Value::Rule(r) => match name {
            "name" => Ok(Value::Str(r.name.clone())),
            "text" => Ok(Value::Str(r.text.clone())),
            _ => Err(format!("rule has no method '{}'", name)),
        },
        Value::Skill(s) => match name {
            "name" => Ok(Value::Str(s.name.clone())),
            "description" => Ok(Value::Str(s.description.clone())),
            "instructions" => Ok(Value::Str(s.instructions.clone())),
            _ => Err(format!("skill has no method '{}'", name)),
        },
        Value::Charter(c) => match name {
            "rules" => Ok(list(c.rules.iter().map(|r| Value::Rule(r.clone())))),
            "skills" => Ok(list(c.skills.iter().map(|s| Value::Skill(s.clone())))),
            "commands" => Ok(list(c.commands.iter().map(|c| Value::Command(c.clone())))),
            _ => Err(format!("charter has no method '{}'", name)),
        },
        other => Err(format!("{} has no method '{}'", other.type_name(), name)),
    }
}

fn arg0<'a>(args: &'a [Value], method: &str) -> Result<&'a Value, String> {
    args.first()
        .ok_or_else(|| format!("'{}' expects an argument", method))
}

fn list(items: impl Iterator<Item = Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items.collect())))
}

fn string_method(s: &str, name: &str, args: &[Value]) -> Result<Value, String> {
    match name {
        "upcase" => Ok(Value::Str(s.to_uppercase())),
        "downcase" => Ok(Value::Str(s.to_lowercase())),
        "strip" | "trim" => Ok(Value::Str(s.trim().to_string())),
        "length" | "size" => Ok(Value::Num(s.chars().count() as f64)),
        "to_s" => Ok(Value::Str(s.to_string())),
        "split" => {
            let sep = arg0(args, "split")?.to_string();
            let parts: Vec<Value> = if sep.is_empty() {
                s.chars().map(|c| Value::Str(c.to_string())).collect()
            } else {
                s.split(&sep).map(|p| Value::Str(p.to_string())).collect()
            };
            Ok(Value::Array(Rc::new(RefCell::new(parts))))
        }
        "contains?" | "include?" => {
            let needle = arg0(args, name)?.to_string();
            Ok(Value::Bool(s.contains(&needle)))
        }
        _ => Err(format!("string has no method '{}'", name)),
    }
}

fn array_method(a: &Rc<RefCell<Vec<Value>>>, name: &str, args: &[Value]) -> Result<Value, String> {
    match name {
        "length" | "size" | "count" => Ok(Value::Num(a.borrow().len() as f64)),
        "first" => Ok(a.borrow().first().cloned().unwrap_or(Value::Nil)),
        "last" => Ok(a.borrow().last().cloned().unwrap_or(Value::Nil)),
        "push" | "append" => {
            a.borrow_mut().push(arg0(args, name)?.clone());
            Ok(Value::Array(a.clone()))
        }
        "reverse" => {
            let mut v = a.borrow().clone();
            v.reverse();
            Ok(Value::Array(Rc::new(RefCell::new(v))))
        }
        "join" => {
            let sep = args.first().map(|v| v.to_string()).unwrap_or_default();
            let parts: Vec<String> = a.borrow().iter().map(|v| v.to_string()).collect();
            Ok(Value::Str(parts.join(&sep)))
        }
        "get" => {
            let idx = arg0(args, "get")?;
            if let Value::Num(n) = idx {
                Ok(a.borrow().get(*n as usize).cloned().unwrap_or(Value::Nil))
            } else {
                Err("get expects a number".into())
            }
        }
        _ => Err(format!("array has no method '{}'", name)),
    }
}

fn hash_method(
    h: &Rc<std::cell::RefCell<std::collections::HashMap<String, Value>>>,
    name: &str,
    args: &[Value],
) -> Result<Value, String> {
    match name {
        "keys" => {
            let keys: Vec<Value> = h.borrow().keys().map(|k| Value::Str(k.clone())).collect();
            Ok(Value::Array(Rc::new(RefCell::new(keys))))
        }
        "values" => {
            let vals: Vec<Value> = h.borrow().values().cloned().collect();
            Ok(Value::Array(Rc::new(RefCell::new(vals))))
        }
        "get" => {
            let key = arg0(args, "get")?.to_string();
            Ok(h.borrow().get(&key).cloned().unwrap_or(Value::Nil))
        }
        "set" => {
            let key = arg0(args, "set")?.to_string();
            let val = args.get(1).cloned().unwrap_or(Value::Nil);
            h.borrow_mut().insert(key, val.clone());
            Ok(val)
        }
        "has?" => {
            let key = arg0(args, "has?")?.to_string();
            Ok(Value::Bool(h.borrow().contains_key(&key)))
        }
        // A no-arg unknown method acts as property lookup: env.MODEL
        _ if args.is_empty() => Ok(h.borrow().get(name).cloned().unwrap_or(Value::Nil)),
        _ => Err(format!("hash has no method '{}'", name)),
    }
}

fn number_method(n: f64, name: &str) -> Result<Value, String> {
    match name {
        "to_s" => Ok(Value::Str(Value::Num(n).to_string())),
        "round" => Ok(Value::Num(n.round())),
        "floor" => Ok(Value::Num(n.floor())),
        "ceil" => Ok(Value::Num(n.ceil())),
        _ => Err(format!("number has no method '{}'", name)),
    }
}

fn memory_method(
    m: &Rc<crate::value::Memory>,
    name: &str,
    args: &[Value],
) -> Result<Value, String> {
    match name {
        "remember" | "set" => {
            let key = arg0(args, name)?.to_string();
            let val = args.get(1).map(|v| v.to_string()).unwrap_or_default();
            m.store.borrow_mut().insert(key, val.clone());
            Ok(Value::Str(val))
        }
        "recall" | "get" => {
            let key = arg0(args, name)?.to_string();
            Ok(m.store.borrow().get(&key).cloned().map(Value::Str).unwrap_or(Value::Nil))
        }
        "forget" => {
            let key = arg0(args, "forget")?.to_string();
            Ok(Value::Bool(m.store.borrow_mut().remove(&key).is_some()))
        }
        "has?" => {
            let key = arg0(args, "has?")?.to_string();
            Ok(Value::Bool(m.store.borrow().contains_key(&key)))
        }
        "keys" => Ok(list(
            m.store.borrow().keys().map(|k| Value::Str(k.clone())),
        )),
        "size" | "length" => Ok(Value::Num(m.store.borrow().len() as f64)),
        _ => Err(format!("memory has no method '{}'", name)),
    }
}

fn message_method(m: &Rc<Message>, name: &str) -> Result<Value, String> {
    match name {
        "content" | "text" | "to_s" => Ok(Value::Str(m.content.clone())),
        "role" => Ok(Value::Str(m.role.clone())),
        "from" => Ok(Value::Str(m.from.clone())),
        "length" | "size" => Ok(Value::Num(m.content.chars().count() as f64)),
        _ => Err(format!("message has no method '{}'", name)),
    }
}

fn hook_result_method(
    r: &Rc<crate::value::HookResult>,
    name: &str,
) -> Result<Value, String> {
    match name {
        "value" => Ok(r.value.clone()),
        "halt?" | "halted?" => Ok(Value::Bool(r.halt)),
        _ => Err(format!("hook_result has no method '{}'", name)),
    }
}
