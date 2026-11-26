package dev.ferrite.host;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/** One permission node declared in {@code plugin.yml}. */
public record PermissionSpec(
        String description, String defaultPermission, Map<String, Boolean> children) {
    static PermissionSpec fromJson(Map<String, Object> json) {
        return new PermissionSpec(
                Json.string(json, "description", null),
                Json.string(json, "default", "op"),
                Collections.unmodifiableMap(new LinkedHashMap<>(Json.booleans(json.get("children")))));
    }
}
