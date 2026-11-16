package dev.ferrite.host;

import java.util.List;
import java.util.Map;

/** One command declared in {@code plugin.yml}. */
public record CommandSpec(String description, String usage, List<String> aliases, String permission) {
    static CommandSpec fromJson(Map<String, Object> json) {
        return new CommandSpec(
                Json.string(json, "description", null),
                Json.string(json, "usage", null),
                Json.strings(json.get("aliases")),
                Json.string(json, "permission", null));
    }
}
