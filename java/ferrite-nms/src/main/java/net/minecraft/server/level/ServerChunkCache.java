// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.server.level;

import java.util.Objects;

/** Chunk-loaded reads for one specific world; it cannot reach another world. */
public final class ServerChunkCache {
    private final ServerLevel level;

    ServerChunkCache(ServerLevel level) {
        this.level = Objects.requireNonNull(level, "level");
    }

    public boolean isChunkLoaded(int chunkX, int chunkZ) {
        return level.isChunkLoaded(chunkX, chunkZ);
    }

    public ServerLevel level() {
        return level;
    }
}
