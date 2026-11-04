//! World game rules for plugins: reads answered from the live rule table the game loop shares,
//! writes validated here and applied through the `gamerule` console command.

use ferrite_command::gamerule::{GameRules, RuleValue, canonical_name, spec, specs};
use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};
use std::sync::{Arc, Mutex, OnceLock};

type Shared = Arc<Mutex<GameRules>>;

fn slot() -> &'static Mutex<Option<Shared>> {
    static SLOT: OnceLock<Mutex<Option<Shared>>> = OnceLock::new();
    SLOT.get_or_init(Default::default)
}

/// Called by the game loop when it creates its rule table (the latest world wins).
pub fn register(rules: Shared) {
    *slot().lock().unwrap() = Some(rules);
}

fn live() -> Option<Shared> {
    slot().lock().unwrap().clone()
}

fn text(value: RuleValue) -> String {
    value.text()
}

fn is_default_world(subject: &str) -> bool {
    uuid::Uuid::parse_str(subject).ok() == Some(ferrite_plugin::default_world_id())
}

/// The reply to a read-only game rule query, or `None` when `call` is something else.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "World" || !is_default_world(&call.subject) {
        return None;
    }
    let name = call.args.first().map(|a| canonical_name(a));
    match call.method.as_str() {
        "isGameRule(String)" => Some(spec(&name?).is_some().to_string()),
        "getGameRuleDefault(String)" => Some(text(spec(&name?)?.default)),
        "getGameRuleValue(String)" => {
            let name = name?;
            // An unknown rule reads as null, like Bukkit.
            let Some(spec) = spec(&name) else {
                return Some("\u{1b}null".to_owned());
            };
            let rules = live()?;
            let value = rules.lock().unwrap().get(&name).unwrap_or(spec.default);
            Some(text(value))
        }
        "getGameRules()" => {
            if !call.args.is_empty() {
                return None;
            }
            let names: Vec<_> = specs().iter().map(|s| s.name).collect();
            Some(names.join(&HOST_ITEM_SEPARATOR.to_string()))
        }
        _ => None,
    }
}

/// The `gamerule` command for a valid write, or `None` for an unknown rule or bad value.
pub fn set_line(call: &HostCall) -> Option<String> {
    if call.owner != "World"
        || call.method != "setGameRuleValue(String,String)"
        || !is_default_world(&call.subject)
    {
        return None;
    }
    let name = canonical_name(call.args.first()?);
    let value = call.args.get(1)?;
    GameRules::default().set(&name, value).ok()?;
    Some(format!("gamerule {name} {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: ferrite_plugin::default_world_id().to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn rules_read_live_values_and_defaults() {
        let mut table = GameRules::default();
        table.set("keep_inventory", "true").unwrap();
        register(Arc::new(Mutex::new(table)));
        let get = |name: &str| answer(&call("getGameRuleValue(String)", &[name]));
        assert_eq!(get("keepInventory").as_deref(), Some("true"));
        assert_eq!(get("minecraft:keep_inventory").as_deref(), Some("true"));
        assert_eq!(get("nope").as_deref(), Some("\u{1b}null"));
        assert_eq!(
            answer(&call("getGameRuleDefault(String)", &["keepInventory"])).as_deref(),
            Some("false")
        );
        assert_eq!(
            answer(&call("isGameRule(String)", &["mob_griefing"])).as_deref(),
            Some("true")
        );
        assert!(
            answer(&call("getGameRules()", &[]))
                .unwrap()
                .contains("keep_inventory")
        );
    }

    #[test]
    fn writes_are_validated_before_becoming_commands() {
        let set = |name: &str, value: &str| {
            set_line(&call("setGameRuleValue(String,String)", &[name, value]))
        };
        assert_eq!(
            set("keepInventory", "true").as_deref(),
            Some("gamerule keep_inventory true")
        );
        assert_eq!(set("keepInventory", "maybe"), None);
        assert_eq!(set("nope", "true"), None);
        assert_eq!(set("random_tick_speed", "3 extra"), None);
    }
}
