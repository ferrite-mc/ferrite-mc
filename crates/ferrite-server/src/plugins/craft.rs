//! Crafting grid lookups for plugins, answered by the same recipe book the crafting menus use.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};

/// `Server#addRecipe`, `removeRecipe`, `clearRecipes` and `resetRecipes`: changes to the crafting
/// recipes every menu uses. `addCraftingRecipe` answers `true`, `false` (the id is taken) or `!`
/// followed by why the recipe cannot be added.
fn recipe_changes(call: &HostCall) -> Option<String> {
    match call.method.as_str() {
        "addCraftingRecipe(String,String)" => {
            let id = call.args.first()?;
            Some(
                match crate::game::add_plugin_recipe(id, call.args.get(1)?) {
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
    fn adding_and_removing_a_crafting_recipe_changes_what_the_grid_crafts() {
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
}
