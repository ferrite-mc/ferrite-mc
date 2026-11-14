package com.mojang.authlib;

/** Player id and name, as Mojang's authlib names it. */
public class GameProfile {
    private final java.util.UUID id;
    private final String name;
    private final com.mojang.authlib.properties.PropertyMap properties = new com.mojang.authlib.properties.PropertyMap();
    public GameProfile(java.util.UUID id, String name) { this.id = id; this.name = name; }
    public GameProfile(java.util.UUID id, String name, com.mojang.authlib.properties.PropertyMap properties) { this(id, name); this.properties.putAll(properties); }
    public com.mojang.authlib.properties.PropertyMap getProperties() { return properties; }
    public com.mojang.authlib.properties.PropertyMap properties() { return properties; }
    public boolean isComplete() { return id != null && name != null && !name.isEmpty(); }
    public java.util.UUID getId() { return id; }
    public java.util.UUID id() { return id; }
    public String getName() { return name; }
    public String name() { return name; }
    @Override public boolean equals(Object other) { return other instanceof GameProfile g && java.util.Objects.equals(g.id, id) && java.util.Objects.equals(g.name, name); }
    @Override public int hashCode() { return java.util.Objects.hash(id, name); }
    @Override public String toString() { return "GameProfile[id=" + id + ", name=" + name + "]"; }
}
