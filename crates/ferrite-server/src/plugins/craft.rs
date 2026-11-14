//! Crafting grid lookups for plugins, answered by the same recipe book the crafting menus use.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};

/// `Server#addRecipe`, `removeRecipe`, `clearRecipes` and `resetRecipes`: changes to the recipes
/// every menu uses. `addRecipe(String,String,String,int)` takes the id, the recipe JSON, the result's
/// metadata (the `setItemMeta` payload, empty for none) and its wear, and answers `true`, `false`
/// (the id is taken) or `!` followed by why the recipe cannot be added.
fn recipe_changes(call: &HostCall) -> Option<String> {
    match call.method.as_str() {
        "addRecipe(String,String,String,int)" | "addCraftingRecipe(String,String)" => {
            let id = call.args.first()?;
            let (meta, damage) = if call.method.starts_with("addRecipe") {
                let payload = call.args.get(2)?;
                let meta = if payload.is_empty() {
                    None
                } else {
                    match super::inventory::parse_item_meta(payload) {
                        Some(meta) => Some(meta),
                        None => return Some("!the result's item metadata is not readable".into()),
                    }
                };
                (meta, call.args.get(3)?.parse::<i32>().ok().filter(|d| *d >= 0)?)
            } else {
                (None, 0)
            };
            Some(
                match crate::game::add_plugin_recipe(id, call.args.get(1)?, meta, damage) {
                    Ok(added) => added.to_string(),
                    Err(why) => format!("!{why}"),
                },
            )
        }
        "removeCraftingRecipe(String)" => {
            Some(crate::game::remove_recipe(call.args.first()?).to_string())
        }
        "clearCraftingRecipes()" => {
            crate::game::clear_recipes();
            Some(String::new())
        }
        "resetCraftingRecipes()" => {
            crate::game::reset_recipes();
            Some(String::new())
        }
        _ => None,
    }
}

/// `World#setVoidDamage…`: the rules the game loop applies below the world.
/// Arguments are `enabled`, `amount` and `offset`.
fn void_rules(call: &HostCall) -> Option<String> {
    if call.owner != "World" || call.method != "setVoidDamage(boolean,float,double)" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    let enabled = match call.args.first()?.as_str() {
        "true" => true,
        "false" => false,
        _ => return None,
    };
    let amount: f32 = call
        .args
        .get(1)?
        .parse()
        .ok()
        .filter(|a: &f32| a.is_finite() && *a >= 0.0)?;
    let offset: f64 = call
        .args
        .get(2)?
        .parse()
        .ok()
        .filter(|o: &f64| o.is_finite())?;
    super::facts::set_void_damage(
        world,
        super::facts::VoidDamage {
            enabled,
            amount,
            offset,
        },
    );
    Some(String::new())
}

/// The reply to `craftMatrix(int,String)`: the recipe id, the result item key and its count joined
/// by the item separator, or `None` when nothing matches or the grid is malformed.
///
/// The grid is `width` columns wide; its items are Bukkit material names (`AIR` is empty).
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner == "Server"
        && let Some(reply) = recipe_changes(call)
    {
        return Some(reply);
    }
    if let Some(reply) = void_rules(call) {
        return Some(reply);
    }
    if call.owner != "Server" || call.method != "craftMatrix(int,String)" {
        return None;
    }
    let width: usize = call
        .args
        .first()?
        .parse()
        .ok()
        .filter(|w| (2..=3).contains(w))?;
    let keys: Vec<Option<String>> = call
        .args
        .get(1)?
        .split(HOST_ITEM_SEPARATOR)
        .map(|material| match material {
            "AIR" | "" => Some(None),
            name if name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') =>
            {
                Some(Some(format!("minecraft:{}", name.to_ascii_lowercase())))
            }
            _ => None,
        })
        .collect::<Option<_>>()?;
    if keys.len() != width * width {
        return None;
    }
    let names: Vec<Option<&str>> = keys.iter().map(Option::as_deref).collect();
    let (id, item, count) = crate::game::match_crafting_for_plugin(&names, width, width)?;
    Some(format!(
        "{id}{sep}{item}{sep}{count}",
        sep = HOST_ITEM_SEPARATOR
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(width: &str, items: &[&str]) -> HostCall {
        HostCall {
            owner: "Server".into(),
            subject: "server".into(),
            method: "craftMatrix(int,String)".into(),
            args: vec![width.into(), items.join(&HOST_ITEM_SEPARATOR.to_string())],
        }
    }

    #[test]
    fn a_log_in_a_two_by_two_grid_crafts_planks() {
        let reply = answer(&call("2", &["AIR", "OAK_LOG", "AIR", "AIR"]));
        let reply = reply.expect("the recipe book is loaded from the vanilla data");
        let parts: Vec<_> = reply.split(HOST_ITEM_SEPARATOR).collect();
        assert_eq!(parts[1], "minecraft:oak_planks");
        assert_eq!(parts[2], "4");
    }

    #[test]
    fn malformed_grids_and_unknown_items_craft_nothing() {
        assert_eq!(answer(&call("2", &["OAK_LOG"])), None);
        assert_eq!(answer(&call("4", &["AIR"; 16])), None);
        assert_eq!(answer(&call("2", &["AIR", "bad name", "AIR", "AIR"])), None);
        assert_eq!(answer(&call("2", &["DIRT", "AIR", "AIR", "AIR"])), None);
    }

    fn change(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Server".into(),
            subject: "server".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn void_rules_reach_the_registry_and_reject_bad_input() {
        let world = uuid::Uuid::from_u128(0x77_0001);
        let set = |args: &[&str]| {
            answer(&HostCall {
                owner: "World".into(),
                subject: world.to_string(),
                method: "setVoidDamage(boolean,float,double)".into(),
                args: args.iter().map(|a| (*a).to_owned()).collect(),
            })
        };
        assert_eq!(super::super::facts::void_damage(world), Default::default());
        assert_eq!(set(&["false", "9.5", "-32"]).as_deref(), Some(""));
        let rules = super::super::facts::void_damage(world);
        assert!(!rules.enabled);
        assert_eq!((rules.amount, rules.offset), (9.5, -32.0));
        assert_eq!(set(&["maybe", "1", "0"]), None);
        assert_eq!(set(&["true", "-1", "0"]), None);
        assert_eq!(super::super::facts::void_damage(world), rules);
    }

    #[test]
    fn adding_and_removing_a_crafting_recipe_changes_what_the_grid_crafts() {
        let _guard = crate::game::crafting_overlay_lock();
        let json = r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:sand","minecraft:sand"],"result":{"id":"minecraft:emerald","count":2}}"#;
        let grid = call("2", &["SAND", "SAND", "AIR", "AIR"]);
        assert_eq!(answer(&grid), None);
        let add = change(
            "addCraftingRecipe(String,String)",
            &["plugin:route_two_sand", json],
        );
        assert_eq!(answer(&add).as_deref(), Some("true"));
        let reply = answer(&grid).expect("the plugin recipe crafts");
        let parts: Vec<_> = reply.split(HOST_ITEM_SEPARATOR).collect();
        assert_eq!(
            (parts[0], parts[1], parts[2]),
            ("plugin:route_two_sand", "minecraft:emerald", "2")
        );
        assert_eq!(answer(&add).as_deref(), Some("false"), "the id is taken");
        let bad = change(
            "addCraftingRecipe(String,String)",
            &["plugin:route_bad", "{"],
        );
        assert!(answer(&bad).is_some_and(|reply| reply.starts_with('!')));
        let remove = change("removeCraftingRecipe(String)", &["plugin:route_two_sand"]);
        assert_eq!(answer(&remove).as_deref(), Some("true"));
        assert_eq!(answer(&remove).as_deref(), Some("false"));
        assert_eq!(answer(&grid), None);
    }

    /// A `setItemMeta` payload with a name and nothing else (the nine fields joined by U+001E).
    fn name_only(name: &str) -> String {
        format!("{name}\u{1e}\u{1e}\u{1e}0\u{1e}\u{1e}0\u{1e}\u{1e}\u{1e}")
    }

    #[test]
    fn add_recipe_takes_any_kind_with_its_result_metadata_and_refuses_what_it_cannot_read() {
        let _guard = crate::game::crafting_overlay_lock();
        let smelt = r#"{"type":"minecraft:smelting","ingredient":"minecraft:gravel","result":{"id":"minecraft:glass"},"experience":0.1,"cookingtime":100}"#;
        let named = name_only(r#"{"text":"Fused"}"#);
        let add = |id: &str, json: &str, meta: &str, damage: &str| {
            answer(&change(
                "addRecipe(String,String,String,int)",
                &[id, json, meta, damage],
            ))
        };
        assert_eq!(add("plugin:route_fuse", smelt, &named, "0").as_deref(), Some("true"));
        assert_eq!(add("plugin:route_fuse", smelt, "", "0").as_deref(), Some("false"), "taken");
        // Metadata the host cannot read refuses the recipe instead of adding it bare.
        let unreadable = add("plugin:route_unreadable", smelt, "not a payload", "0");
        assert!(unreadable.is_some_and(|reply| reply.starts_with('!')));
        // A negative wear is a malformed call.
        assert_eq!(add("plugin:route_wear", smelt, "", "-1"), None);
        let transmute = r#"{"type":"minecraft:crafting_transmute","input":"minecraft:dirt","material":"minecraft:sand","result":"minecraft:stone"}"#;
        assert!(add("plugin:route_transmute", transmute, "", "0").is_some_and(|r| r.starts_with('!')));
        assert_eq!(
            answer(&change("removeCraftingRecipe(String)", &["plugin:route_fuse"])).as_deref(),
            Some("true")
        );
    }
}
