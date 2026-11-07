//! Block entity data for plugins that capture structures: the NBT of the block entities in a box
//! as hex, and (through `/data merge block`, see `extra.rs`) writing it back.

use ferrite_plugin::{HOST_ITEM_SEPARATOR, HostCall};

/// The reply to `World#ferriteBlockEntities(x1, y1, z1, x2, y2, z2)`: one item per block entity,
/// `x,y,z,<hex of binary NBT>`, in a list.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "World" || call.method != "ferriteBlockEntities(int,int,int,int,int,int)" {
        return None;
    }
    let world = uuid::Uuid::parse_str(&call.subject).ok()?;
    let n: Vec<i32> = call
        .args
        .iter()
        .map(|a| a.parse().ok())
        .collect::<Option<_>>()?;
    let [x1, y1, z1, x2, y2, z2] = n[..] else {
        return None;
    };
    let Some(view) = ferrite_plugin::world_view() else {
        return Some(String::new());
    };
    let items: Vec<String> = view
        .block_entities(world, [x1, y1, z1], [x2, y2, z2])
        .into_iter()
        .map(|(at, bytes)| format!("{},{},{},{}", at[0], at[1], at[2], hex(&bytes)))
        .collect();
    Some(items.join(&HOST_ITEM_SEPARATOR.to_string()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_without_a_view_has_no_block_entities() {
        let call = HostCall {
            owner: "World".into(),
            subject: ferrite_plugin::default_world_id().to_string(),
            method: "ferriteBlockEntities(int,int,int,int,int,int)".into(),
            args: ["0", "0", "0", "1", "1", "1"].map(String::from).to_vec(),
        };
        assert_eq!(answer(&call).as_deref(), Some(""));
        assert_eq!(hex(&[0, 10, 255]), "000aff");
    }
}
