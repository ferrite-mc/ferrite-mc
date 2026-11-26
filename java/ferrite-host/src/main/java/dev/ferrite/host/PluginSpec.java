package dev.ferrite.host;

import java.nio.file.Path;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * The descriptor Rust parsed from {@code plugin.yml}/{@code paper-plugin.yml} and sent with
 * {@code Bootstrap.loadPlugin}. Java never reads YAML again.
 */
public record PluginSpec(
        String name,
        String version,
        String main,
        String apiVersion,
        String load,
        List<String> dependencies,
        List<String> softDependencies,
        List<String> loadBefore,
        List<String> provides,
        boolean foliaSupported,
        String format,
        Path jar,
        Path dataFolder,
        Map<String, CommandSpec> commands,
        Map<String, PermissionSpec> permissions) {

    static PluginSpec fromJson(Map<String, Object> json) {
        Map<String, CommandSpec> commands = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : Json.objectOrEmpty(json.get("commands")).entrySet()) {
            commands.put(entry.getKey(), CommandSpec.fromJson(Json.asObject(entry.getValue())));
        }
        Map<String, PermissionSpec> permissions = new LinkedHashMap<>();
        for (Map.Entry<String, Object> entry : Json.objectOrEmpty(json.get("permissions")).entrySet()) {
            permissions.put(entry.getKey(), PermissionSpec.fromJson(Json.asObject(entry.getValue())));
        }
        return new PluginSpec(
                Json.requireString(json, "name"),
                Json.requireString(json, "version"),
                Json.requireString(json, "main"),
                Json.string(json, "api_version", null),
                Json.string(json, "load", "POSTWORLD"),
                Json.strings(json.get("dependencies")),
                Json.strings(json.get("soft_dependencies")),
                Json.strings(json.get("load_before")),
                Json.strings(json.get("provides")),
                Json.boolValue(json, "folia_supported", false),
                Json.string(json, "format", "plugin.yml"),
                Path.of(Json.requireString(json, "jar")),
                Path.of(Json.requireString(json, "data_folder")),
                Collections.unmodifiableMap(commands),
                Collections.unmodifiableMap(permissions));
    }
}
