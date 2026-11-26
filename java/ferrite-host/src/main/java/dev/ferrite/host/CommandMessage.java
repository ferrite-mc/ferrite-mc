package dev.ferrite.host;

/** One message a plugin command sent to a player or the console. */
public record CommandMessage(
        String playerUuid, String playerName, String content, boolean adventure) {
    public static CommandMessage console(String content) {
        return new CommandMessage(null, null, content, false);
    }

    public static CommandMessage player(String uuid, String name, String content) {
        return new CommandMessage(uuid, name, content, false);
    }
}
