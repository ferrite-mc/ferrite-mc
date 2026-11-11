//! Player actions a plugin triggers on behalf of a player: `Player#chat` with a command and
//! `Player#breakBlock`. Both run under the player's own rules, never the console's.
use ferrite_plugin::{HostCall, PlayerCommand};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use uuid::Uuid;

/// Longest chat line the game loop accepts from a client.
const MAX_CHAT: usize = 256;
/// How long a plugin thread waits for the owning region to answer a break request. A region
/// answers within a tick; the wait only runs out if the player left or the region stalled, and
/// then the break counts as not done.
const BREAK_WAIT: Duration = Duration::from_secs(2);

fn pending() -> &'static Mutex<HashMap<u64, Sender<bool>>> {
    static PENDING: OnceLock<Mutex<HashMap<u64, Sender<bool>>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

fn begin() -> (u64, Receiver<bool>) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let ticket = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = channel();
    pending().lock().unwrap().insert(ticket, tx);
    (ticket, rx)
}

/// Delivers the verdict of a break request. An unknown or expired ticket is ignored.
pub fn complete(ticket: u64, broke: bool) {
    if let Some(tx) = pending().lock().unwrap().remove(&ticket) {
        let _ = tx.send(broke);
    }
}

/// The command a `chat(String)` call runs, without its leading slash. Plain chat lines go
/// through the chat event in the plugin runtime; only slash commands reach the host.
fn command_line(message: &str) -> Option<&str> {
    let line = message.strip_prefix('/')?.trim_start_matches('/');
    let valid = !line.is_empty()
        && message.chars().count() <= MAX_CHAT
        && !line.chars().any(char::is_control);
    valid.then_some(line)
}

/// Answers the calls this module owns, sending the player commands through `send`.
pub fn handle(call: &HostCall, send: impl Fn(PlayerCommand)) -> Option<String> {
    if call.owner != "Player" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    match call.method.as_str() {
        "chat(String)" => {
            let line = command_line(call.args.first()?)?;
            send(PlayerCommand::Chat {
                player,
                message: line.to_owned(),
            });
            Some(String::new())
        }
        "breakBlock(int,int,int)" => {
            let mut axis = call.args.iter().map(|v| v.parse::<i32>().ok());
            let position = [axis.next()??, axis.next()??, axis.next()??];
            let (ticket, verdict) = begin();
            send(PlayerCommand::BreakBlock {
                player,
                position,
                ticket,
            });
            let broke = verdict.recv_timeout(BREAK_WAIT).unwrap_or(false);
            pending().lock().unwrap().remove(&ticket);
            Some(broke.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: Uuid::from_u128(9).to_string(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn chat_commands_run_as_the_player_and_plain_lines_are_not_the_hosts() {
        let sent = RefCell::new(Vec::new());
        let reply = handle(&call("chat(String)", &["/gamemode creative"]), |c| {
            sent.borrow_mut().push(c)
        });
        assert_eq!(reply.as_deref(), Some(""));
        assert_eq!(
            sent.into_inner(),
            vec![PlayerCommand::Chat {
                player: Uuid::from_u128(9),
                message: "gamemode creative".into()
            }]
        );
        for bad in [
            "hello",
            "/",
            "//",
            "/a\nb",
            &format!("/{}", "x".repeat(300)),
        ] {
            let sent = RefCell::new(0);
            assert_eq!(
                handle(&call("chat(String)", &[bad]), |_| *sent.borrow_mut() += 1),
                None,
                "{bad:?}"
            );
            assert_eq!(sent.into_inner(), 0);
        }
    }

    #[test]
    fn break_block_waits_for_the_regions_verdict() {
        for verdict in [true, false] {
            let reply = handle(&call("breakBlock(int,int,int)", &["1", "64", "-3"]), |c| {
                let PlayerCommand::BreakBlock {
                    position, ticket, ..
                } = c
                else {
                    panic!("unexpected {c:?}")
                };
                assert_eq!(position, [1, 64, -3]);
                complete(ticket, verdict);
            });
            assert_eq!(reply, Some(verdict.to_string()));
        }
    }

    #[test]
    fn break_block_without_an_answer_is_not_a_break() {
        // The command is dropped (the player left), so the wait runs out.
        let reply = handle(&call("breakBlock(int,int,int)", &["0", "0", "0"]), |_| {});
        assert_eq!(reply.as_deref(), Some("false"));
        assert!(pending().lock().unwrap().is_empty());
        assert_eq!(
            handle(&call("breakBlock(int,int,int)", &["x", "0", "0"]), |_| {}),
            None
        );
    }
}
