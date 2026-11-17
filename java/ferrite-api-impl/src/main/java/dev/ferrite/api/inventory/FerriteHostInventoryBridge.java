package dev.ferrite.api.inventory;

import com.google.gson.Gson;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.entity.FerritePlayerInventory;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.host.Bootstrap;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;

/**
 * Bridges {@link FerritePlayerInventory} mutations to the Rust host's contract-v2
 * {@code player.inventory_get} and {@code player.inventory_set_async} ops via {@link Bootstrap},
 * including {@code nbt_base64} item component and PDC payloads.
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
                String nbtBase64 = FerriteItemNbt.encodeItemBase64(item);
                if (nbtBase64 != null) {
                    patch.addProperty("nbt_base64", nbtBase64);
                }
                slots.add(key, patch);
            }
        }
        root.add("slots", slots);
        return GSON.toJson(root);
    }

    /** Decode a {@code player.inventory_get} JSON response into a {@link PlayerInventorySnapshot}. */
    public static PlayerInventorySnapshot decodeGetResponse(String responseJson) {
        Objects.requireNonNull(responseJson, "responseJson");
        JsonObject root = JsonParser.parseString(responseJson).getAsJsonObject();
        int heldSlot = root.has("held_slot") ? root.get("held_slot").getAsInt() : 0;
        ItemStack[] contents = new ItemStack[PlayerInventorySnapshot.SIZE];
        if (root.has("slots") && root.get("slots").isJsonObject()) {
            JsonObject slots = root.getAsJsonObject("slots");
            for (Map.Entry<String, JsonElement> entry : slots.entrySet()) {
                int slot = Integer.parseInt(entry.getKey());
                if (slot < 0 || slot >= PlayerInventorySnapshot.SIZE) {
                    throw new IllegalArgumentException("slot index out of range: " + slot);
                }
                if (entry.getValue() == null || entry.getValue().isJsonNull()) {
                    contents[slot] = null;
                    continue;
                }
                JsonObject patch = entry.getValue().getAsJsonObject();
                String id = patch.get("id").getAsString();
                int count = patch.get("count").getAsInt();
                Material material = Material.matchMaterial(id);
                if (material == null) {
                    throw new IllegalArgumentException("unknown item id: " + id);
                }
                if (patch.has("nbt_base64") && !patch.get("nbt_base64").isJsonNull()) {
                    String nbtBase64 = patch.get("nbt_base64").getAsString();
                    contents[slot] = FerriteItemNbt.decodeItemBase64(nbtBase64, material, count);
                } else {
                    contents[slot] = count > 0 ? new ItemStack(material, count) : null;
                }
            }
        }
        return PlayerInventorySnapshot.capture(contents, heldSlot);
    }

    /** Queue an asynchronous inventory update to the embedded Rust host without blocking. */
    public static String pushAsync(UUID playerUuid, PlayerInventorySnapshot snapshot) {
        return Bootstrap.playerInventorySetAsync(encodeSetRequest(playerUuid, snapshot));
    }

    /** Read a point-in-time inventory snapshot from the embedded Rust host. */
    public static PlayerInventorySnapshot pullSnapshot(UUID playerUuid) {
        Objects.requireNonNull(playerUuid, "playerUuid");
        JsonObject req = new JsonObject();
        req.addProperty("uuid", playerUuid.toString());
        String response = Bootstrap.playerInventoryGet(GSON.toJson(req));
        return decodeGetResponse(response);
    }
}
