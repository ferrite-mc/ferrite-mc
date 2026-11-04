//! Client-visible entity state set by plugins: glowing, invisibility, fire, silence, gravity and
//! freeze ticks.
//!
//! These are entity data values (`set_entity_data`). Ferrite sends them to the player they belong
//! to, so the player sees the outline, the fire overlay or the frozen screen and the client's own
//! physics honours `setGravity`; other players are not told yet, because the server does not
//! broadcast player entity data to trackers at all.

use ferrite_plugin::{HostCall, PlayerCommand};
use uuid::Uuid;

const FIRE: u8 = 0x01;
const INVISIBLE: u8 = 0x20;
const GLOWING: u8 = 0x40;
/// Ticks the client needs to be fully frozen (vanilla `Entity.getTicksRequiredToFreeze`).
const MAX_FREEZE: i32 = 140;

/// What a plugin has set on one player so far; the flags byte is rebuilt from it every time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppearanceState {
    flags: u8,
}

fn flag(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn apply(state: &mut AppearanceState, bit: u8, on: bool) {
    if on {
        state.flags |= bit;
    } else {
        state.flags &= !bit;
    }
}

/// The entity data change a call makes, folding it into `state`; `None` when the call is not an
/// appearance call or carries a bad value (the state is then left untouched).
pub fn appearance_command(state: &mut AppearanceState, call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    let arg = call.args.first().map(String::as_str)?;
    let mut command = PlayerCommand::EntityData {
        player,
        flags: None,
        silent: None,
        no_gravity: None,
        freeze_ticks: None,
    };
    let PlayerCommand::EntityData {
        flags,
        silent,
        no_gravity,
        freeze_ticks,
        ..
    } = &mut command
    else {
        return None;
    };
    match call.method.as_str() {
        "setGlowing(boolean)" => apply(state, GLOWING, flag(arg)?),
        "setInvisible(boolean)" => apply(state, INVISIBLE, flag(arg)?),
        "setVisualFire(boolean)" => apply(state, FIRE, flag(arg)?),
        "setSilent(boolean)" => *silent = Some(flag(arg)?),
        // Entity data stores the inverse: `NoGravity`.
        "setGravity(boolean)" => *no_gravity = Some(!flag(arg)?),
        "setFreezeTicks(int)" => {
            *freeze_ticks = Some(arg.parse::<i32>().ok()?.clamp(0, MAX_FREEZE));
        }
        _ => return None,
    }
    if matches!(
        call.method.as_str(),
        "setGlowing(boolean)" | "setInvisible(boolean)" | "setVisualFire(boolean)"
    ) {
        *flags = Some(state.flags);
    }
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, arg: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-00000000000a".into(),
            method: method.into(),
            args: vec![arg.into()],
        }
    }

    fn data(
        command: Option<PlayerCommand>,
    ) -> (Option<u8>, Option<bool>, Option<bool>, Option<i32>) {
        match command {
            Some(PlayerCommand::EntityData {
                flags,
                silent,
                no_gravity,
                freeze_ticks,
                ..
            }) => (flags, silent, no_gravity, freeze_ticks),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn flags_accumulate_and_clear() {
        let mut state = AppearanceState::default();
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setGlowing(boolean)", "true")
            ))
            .0,
            Some(GLOWING)
        );
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setInvisible(boolean)", "true")
            ))
            .0,
            Some(GLOWING | INVISIBLE)
        );
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setGlowing(boolean)", "false")
            ))
            .0,
            Some(INVISIBLE)
        );
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setVisualFire(boolean)", "true")
            ))
            .0,
            Some(INVISIBLE | FIRE)
        );
    }

    #[test]
    fn gravity_is_sent_inverted_and_freeze_is_clamped() {
        let mut state = AppearanceState::default();
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setGravity(boolean)", "false")
            ))
            .2,
            Some(true)
        );
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setFreezeTicks(int)", "999")
            ))
            .3,
            Some(MAX_FREEZE)
        );
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setFreezeTicks(int)", "-5")
            ))
            .3,
            Some(0)
        );
        assert_eq!(
            data(appearance_command(
                &mut state,
                &call("setSilent(boolean)", "true")
            ))
            .1,
            Some(true)
        );
    }

    #[test]
    fn bad_values_change_nothing() {
        let mut state = AppearanceState::default();
        for bad in [
            call("setGlowing(boolean)", "yes"),
            call("setFreezeTicks(int)", "abc"),
            call("setFlySpeed(float)", "1"),
        ] {
            assert!(appearance_command(&mut state, &bad).is_none(), "{bad:?}");
        }
        assert_eq!(state, AppearanceState::default());
    }
}
