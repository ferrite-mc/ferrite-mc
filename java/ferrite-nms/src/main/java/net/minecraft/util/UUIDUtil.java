// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.util;

import com.mojang.serialization.Codec;
import com.mojang.serialization.DataResult;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.UUID;

/** Clean-room UUID helper matching 26.3 {@code net.minecraft.util.UUIDUtil}. */
public final class UUIDUtil {
    public static final Codec<UUID> CODEC = Codec.INT.listOf().comapFlatMap(
            list -> list.size() == 4
                    ? DataResult.success(uuidFromIntArray(
                            new int[] {list.get(0), list.get(1), list.get(2), list.get(3)}))
                    : DataResult.error(() -> "Expected 4 ints for UUID, got " + list.size()),
            uuid -> {
                int[] ints = uuidToIntArray(uuid);
                return Arrays.stream(ints).boxed().toList();
            });

    private UUIDUtil() {}

    public static UUID createOfflinePlayerUUID(String username) {
        return UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(StandardCharsets.UTF_8));
    }

    public static int[] uuidToIntArray(UUID uuid) {
        long msb = uuid.getMostSignificantBits();
        long lsb = uuid.getLeastSignificantBits();
        return new int[] {
            (int) (msb >> 32), (int) msb, (int) (lsb >> 32), (int) lsb
        };
    }

    public static UUID uuidFromIntArray(int[] bits) {
        return new UUID(
                ((long) bits[0] << 32) | (bits[1] & 0xFFFFFFFFL),
                ((long) bits[2] << 32) | (bits[3] & 0xFFFFFFFFL));
    }
}
