// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.resources;

import com.mojang.serialization.Codec;
import com.mojang.serialization.DataResult;
import java.util.Objects;

/** Immutable namespaced identifier matching 26.3 {@code net.minecraft.resources.Identifier}. */
public final class Identifier implements Comparable<Identifier> {
    public static final Codec<Identifier> CODEC = Codec.STRING.comapFlatMap(
            Identifier::read, Identifier::toString);

    private final String namespace;
    private final String path;

    private Identifier(String namespace, String path) {
        this.namespace = Objects.requireNonNull(namespace, "namespace");
        this.path = Objects.requireNonNull(path, "path");
        if (namespace.isEmpty() || !isValidNamespace(namespace)) {
            throw new IllegalArgumentException("Invalid namespace: " + namespace);
        }
        if (!isValidPath(path)) {
            throw new IllegalArgumentException("Invalid path: " + path);
        }
    }

    public static Identifier fromNamespaceAndPath(String namespace, String path) {
        return new Identifier(namespace, path);
    }

    public static Identifier withDefaultNamespace(String path) {
        return new Identifier("minecraft", path);
    }

    public static Identifier parse(String location) {
        int colon = location.indexOf(':');
        if (colon < 0) {
            return new Identifier("minecraft", location);
        }
        String ns = colon == 0 ? "minecraft" : location.substring(0, colon);
        return new Identifier(ns, location.substring(colon + 1));
    }

    public static Identifier tryParse(String location) {
        try {
            return parse(location);
        } catch (IllegalArgumentException e) {
            return null;
        }
    }

    public static DataResult<Identifier> read(String location) {
        try {
            return DataResult.success(parse(location));
        } catch (IllegalArgumentException e) {
            return DataResult.error(() -> "Not a valid identifier: " + location);
        }
    }

    public String getNamespace() {
        return namespace;
    }

    public String getPath() {
        return path;
    }

    private static boolean isValidNamespace(String ns) {
        for (int i = 0; i < ns.length(); i++) {
            char c = ns.charAt(i);
            if (!(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == '.')) {
                return false;
            }
        }
        return true;
    }

    private static boolean isValidPath(String p) {
        for (int i = 0; i < p.length(); i++) {
            char c = p.charAt(i);
            if (!(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == '.' || c == '/')) {
                return false;
            }
        }
        return true;
    }

    @Override
    public int compareTo(Identifier other) {
        int cmp = path.compareTo(other.path);
        return cmp != 0 ? cmp : namespace.compareTo(other.namespace);
    }

    @Override
    public boolean equals(Object o) {
        return this == o
                || o instanceof Identifier id
                        && namespace.equals(id.namespace)
                        && path.equals(id.path);
    }

    @Override
    public int hashCode() {
        return Objects.hash(namespace, path);
    }

    @Override
    public String toString() {
        return namespace + ":" + path;
    }
}
