package dev.ferrite.api.inventory;

import com.google.gson.Gson;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.entity.FerritePlayerInventory;
import dev.ferrite.host.Bootstrap;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;

/**
 * Bridges {@link FerritePlayerInventory} mutations to the Rust host's contract-v2
 * {@code player.inventory_set_async} op via {@link Bootstrap}.
 */
public final class FerriteHostInventoryBridge {

    private static final Gson GSON = new Gson();

    private FerriteHostInventoryBridge() {}

    /** Attach a snapshot sink to {@code player}'s inventory so mutations queue to the host. */
    public static void attach(FerritePlayer player) {
        Objects.requireNonNull(player, "player");
        if (player.getInventory() instanceof FerritePlayerInventory ferriteInventory) {
            UUID uuid = player.getUniqueId();
            ferriteInventory.setSnapshotSink(snapshot -> pushAsync(uuid, snapshot));
        }
    }

    /** Encode a {@link PlayerInventorySnapshot} as a {@code player.inventory_set_async} JSON payload. */
    public static String encodeSetRequest(UUID playerUuid, PlayerInventorySnapshot snapshot) {
        Objects.requireNonNull(playerUuid, "playerUuid");
        Objects.requireNonNull(snapshot, "snapshot");
        JsonObject root = new JsonObject();
        root.addProperty("uuid", playerUuid.toString());
        root.addProperty("held_slot", snapshot.heldSlot());
        JsonObject slots = new JsonObject();
        for (int i = 0; i < PlayerInventorySnapshot.SIZE; i++) {
            ItemStack item = snapshot.item(i);
            String key = Integer.toString(i);
            if (item == null || item.getType() == Material.AIR || item.getAmount() <= 0) {
                slots.add(key, JsonNull.INSTANCE);
            } else {
                JsonObject patch = new JsonObject();
                patch.addProperty("id", item.getType().getKey().toString());
                patch.addProperty("count", item.getAmount());
                slots.add(key, patch);
            }
        }
        root.add("slots", slots);
        return GSON.toJson(root);
    }

    /** Queue an asynchronous inventory update to the embedded Rust host without blocking. */
    public static String pushAsync(UUID playerUuid, PlayerInventorySnapshot snapshot) {
        return Bootstrap.playerInventorySetAsync(encodeSetRequest(playerUuid, snapshot));
    }
}
