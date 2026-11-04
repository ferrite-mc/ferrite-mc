use ferrite_plugin::{HostCall, PlayerCommand};
pub fn message_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" || call.args.len() != 1 {
        return None;
    }
    let action = match call.method.as_str() {
        "addCustomChatCompletions(Collection)" => 0,
        "removeCustomChatCompletions(Collection)" => 1,
        "setCustomChatCompletions(Collection)" => 2,
        _ => return None,
    };
    let player = call.subject.parse().ok()?;
    let entries: Vec<String> = serde_json::from_str(&call.args[0]).ok()?;
    if entries.len() > 4096 || entries.iter().any(|entry| entry.len() > 32767) {
        return None;
    }
    Some(PlayerCommand::CustomChatCompletions {
        player,
        action,
        entries,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_completions_keep_empty_and_separator_entries() {
        let mut call = HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000007".into(),
            method: "addCustomChatCompletions(Collection)".into(),
            args: vec!["[\"\",\"a\\u001db\"]".into()],
        };
        let player = call.subject.parse().unwrap();
        for (method, action) in [
            ("addCustomChatCompletions(Collection)", 0),
            ("removeCustomChatCompletions(Collection)", 1),
            ("setCustomChatCompletions(Collection)", 2),
        ] {
            call.method = method.into();
            assert_eq!(
                message_command(&call),
                Some(PlayerCommand::CustomChatCompletions {
                    player,
                    action,
                    entries: vec!["".into(), "a\u{1d}b".into()]
                })
            );
        }
        call.args[0] = "[null]".into();
        assert_eq!(message_command(&call), None);
    }
}
