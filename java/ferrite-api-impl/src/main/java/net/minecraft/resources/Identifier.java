package net.minecraft.resources;

/** Namespaced id (`minecraft:stone`). Clean-room shim of the Mojang-mapped class for plugins that use NMS. */
public final class Identifier implements Comparable<Identifier> {
    public static final String DEFAULT_NAMESPACE = "minecraft";
    private final String namespace;
    private final String path;

    private Identifier(String namespace, String path) { this.namespace = namespace; this.path = path; }

    public static Identifier fromNamespaceAndPath(String namespace, String path) { return new Identifier(namespace, path); }
    public static Identifier withDefaultNamespace(String path) { return new Identifier(DEFAULT_NAMESPACE, path); }
    public static Identifier parse(String id) {
        Identifier parsed = tryParse(id);
        if (parsed == null) throw new IllegalArgumentException("Not a valid identifier: " + id);
        return parsed;
    }
    public static Identifier tryParse(String id) {
        if (id == null || id.isEmpty()) return null;
        int colon = id.indexOf(':');
        String namespace = colon < 0 ? DEFAULT_NAMESPACE : id.substring(0, colon);
        String path = colon < 0 ? id : id.substring(colon + 1);
        return valid(namespace, true) && valid(path, false) ? new Identifier(namespace, path) : null;
    }
    private static boolean valid(String text, boolean namespace) {
        if (text.isEmpty() && namespace) return true;
        if (text.isEmpty()) return false;
        for (char c : text.toCharArray()) {
            boolean ok = (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '.' || (!namespace && c == '/');
            if (!ok) return false;
        }
        return true;
    }

    public String getNamespace() { return namespace; }
    public String getPath() { return path; }
    public Identifier withPath(String path) { return new Identifier(namespace, path); }
    public Identifier withPrefix(String prefix) { return new Identifier(namespace, prefix + path); }
    public Identifier withSuffix(String suffix) { return new Identifier(namespace, path + suffix); }
    @Override public int compareTo(Identifier other) { int c = path.compareTo(other.path); return c != 0 ? c : namespace.compareTo(other.namespace); }
    @Override public boolean equals(Object other) { return other instanceof Identifier id && id.namespace.equals(namespace) && id.path.equals(path); }
    @Override public int hashCode() { return 31 * namespace.hashCode() + path.hashCode(); }
    @Override public String toString() { return namespace + ":" + path; }
}
