package dev.ferrite.server;

/** Entity snapshots (Server#getEntityFactory); parsing entity SNBT needs the host's NBT code. */
final class FerriteEntityFactory implements org.bukkit.entity.EntityFactory {
    @Override public org.bukkit.entity.EntitySnapshot createEntitySnapshot(String input) {
        throw new UnsupportedOperationException("Ferrite: not implemented: EntityFactory#createEntitySnapshot");
    }
}
