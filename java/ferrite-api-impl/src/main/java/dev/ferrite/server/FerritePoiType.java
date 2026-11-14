package dev.ferrite.server;

import org.bukkit.NamespacedKey;
import org.bukkit.block.data.BlockData;

final class FerritePoiType implements io.papermc.paper.entity.poi.PoiType {
    private final NamespacedKey key;
    private final FerritePoiData.Definition data;
    FerritePoiType(NamespacedKey key) { this.key = key; data = FerritePoiData.definition(key); }
    /** The block names that can hold this point of interest. */
    java.util.Set<String> blockNames() { return FerriteBlockStates.blockNames(data.states()); }
    @Override public NamespacedKey getKey() { return key; }
    @Override public boolean hasOccupants() { return data.maxTickets() != 0; }
    @Override public boolean is(BlockData block) {
        java.util.Objects.requireNonNull(block, "blockData");
        return data.states().contains(FerriteBlockStates.stateId(block.getAsString()));
    }
    @Override public boolean equals(Object other) { return other instanceof FerritePoiType value && key.equals(value.key); }
    @Override public int hashCode() { return key.hashCode(); }
    @Override public String toString() { return "PoiType[" + key + "]"; }
}
