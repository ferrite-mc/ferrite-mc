package com.mojang.authlib;

/** Player id and name, as Mojang's authlib names it. */
public class GameProfile {
    private final java.util.UUID id;
    private final String name;
    public GameProfile(java.util.UUID id, String name) { this.id = id; this.name = name; }
    public java.util.UUID getId() { return id; }
    public java.util.UUID id() { return id; }
    public String getName() { return name; }
    public String name() { return name; }
    @Override public boolean equals(Object other) { return other instanceof GameProfile g && java.util.Objects.equals(g.id, id) && java.util.Objects.equals(g.name, name); }
    @Override public int hashCode() { return java.util.Objects.hash(id, name); }
    @Override public String toString() { return "GameProfile[id=" + id + ", name=" + name + "]"; }
}
