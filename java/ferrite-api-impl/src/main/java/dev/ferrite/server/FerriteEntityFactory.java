package dev.ferrite.server;

/** Entity snapshots (Server#getEntityFactory); parsing entity SNBT needs the host's NBT code. */
final class FerriteEntityFactory implements org.bukkit.entity.EntityFactory {
    @Override public org.bukkit.entity.EntitySnapshot createEntitySnapshot(String input) {
        java.util.Objects.requireNonNull(input, "input");
        String reply;
        try {
            reply = (String) HostBridge.call("EntityFactory", "", "createEntitySnapshot(String)", String.class, input);
        } catch (UnsupportedOperationException noHost) {
            throw new IllegalStateException("Ferrite: no host is available to parse entity snapshots", noHost);
        }
        if (reply == null || reply.equals(FerriteEntitySnapshot.INVALID)) {
            throw new IllegalArgumentException("Ferrite: invalid entity snapshot: " + input);
        }
        return FerriteEntitySnapshot.fromWire(reply, input);
    }
}
