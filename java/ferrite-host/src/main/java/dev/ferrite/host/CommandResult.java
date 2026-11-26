package dev.ferrite.host;

import java.util.List;

/** What a plugin command did. */
public final class CommandResult {
    private static final CommandResult NOT_HANDLED = new CommandResult(false, List.of());

    private final boolean handled;
    private final List<CommandMessage> messages;

    private CommandResult(boolean handled, List<CommandMessage> messages) {
        this.handled = handled;
        this.messages = List.copyOf(messages);
    }

    public static CommandResult notHandled() {
        return NOT_HANDLED;
    }

    public static CommandResult handled(CommandMessage... messages) {
        return new CommandResult(true, List.of(messages));
    }

    public static CommandResult handled(List<CommandMessage> messages) {
        return new CommandResult(true, messages);
    }

    public boolean handled() {
        return handled;
    }

    public List<CommandMessage> messages() {
        return messages;
    }
}
