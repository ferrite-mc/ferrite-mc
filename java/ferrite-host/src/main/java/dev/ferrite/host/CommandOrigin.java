package dev.ferrite.host;

import java.util.Map;

/** Who ran a plugin command. */
public record CommandOrigin(String type, String playerUuid, String playerName) {
    public static CommandOrigin console() {
        return new CommandOrigin("console", null, null);
    }

    public static CommandOrigin player(String uuid, String name) {
        return new CommandOrigin("player", uuid, name);
    }

    static CommandOrigin fromJson(Map<String, Object> json) {
        String type = Json.requireString(json, "type");
        return switch (type) {
            case "console" -> console();
            case "player" -> player(
                    Json.requireString(json, "uuid"), Json.string(json, "name", ""));
            default -> throw new IllegalArgumentException("unknown command origin '" + type + "'");
        };
    }
}
