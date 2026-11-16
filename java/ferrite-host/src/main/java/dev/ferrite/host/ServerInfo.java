package dev.ferrite.host;

import java.util.Map;

/** Server identity and settings from {@code host.info} and the start config. */
public record ServerInfo(
        String name,
        String version,
        String motd,
        int maxPlayers,
        boolean onlineMode,
        int viewDistance) {

    static ServerInfo fromJson(Map<String, Object> json) {
        return new ServerInfo(
                Json.string(json, "name", "Ferrite"),
                Json.string(json, "version", "0.0.0"),
                Json.string(json, "motd", ""),
                Json.intValue(json, "max_players", 20),
                Json.boolValue(json, "online_mode", true),
                Json.intValue(json, "view_distance", 10));
    }
}
