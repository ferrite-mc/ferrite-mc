package dev.ferrite.server;

import org.bukkit.NamespacedKey;
import org.bukkit.generator.structure.StructureType;

/** A vanilla structure identified by its registry key; placement queries go to the world generation host. */
final class FerriteStructure extends org.bukkit.generator.structure.Structure {
    private final NamespacedKey key;

    FerriteStructure(NamespacedKey key) { this.key = key; }

    @Override public StructureType getStructureType() {
        throw new UnsupportedOperationException("Ferrite: not implemented: Structure#getStructureType() for " + key);
    }

    @Override public NamespacedKey getKey() { return key; }
    @Override public int hashCode() { return key.hashCode(); }
    @Override public boolean equals(Object other) { return other instanceof FerriteStructure s && s.key.equals(key); }
    @Override public String toString() { return key.toString(); }
}
