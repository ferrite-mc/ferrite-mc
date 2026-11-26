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

    /**
     * Attach a snapshot sink and seed the Java inventory from the host's current state.
     * Later mutations queue to the host without blocking the server thread.
     */
    public static void attach(FerritePlayer player) {
        Objects.requireNonNull(player, "player");
        if (player.getInventory() instanceof FerritePlayerInventory ferriteInventory) {
            UUID uuid = player.getUniqueId();
            ferriteInventory.setSnapshotSink(snapshot -> pushAsync(uuid, snapshot));
            try {
                ferriteInventory.applyHostSnapshot(pullSnapshot(uuid));
            } catch (RuntimeException | UnsatisfiedLinkError error) {
                // Pure-Java harnesses have no native bridge; a real host can also race the
                // player inventory registration. Keep the empty inventory and let later
                // mutations push; do not hide the reason.
                System.err.println("Ferrite: initial inventory sync skipped for " + uuid + ": " + error.getMessage());
            }
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
        JsonObject root = unwrapResult(JsonParser.parseString(responseJson).getAsJsonObject(), "player.inventory_get");
        int heldSlot = root.has("held_slot") && !root.get("held_slot").isJsonNull()
                ? root.get("held_slot").getAsInt()
                : 0;
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
                if (!patch.has("id") || patch.get("id").isJsonNull()
                        || !patch.has("count") || patch.get("count").isJsonNull()) {
                    throw new IllegalArgumentException("slot " + slot + " is missing id or count");
                }
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
        String response = Bootstrap.playerInventorySetAsync(encodeSetRequest(playerUuid, snapshot));
        try {
            unwrapResult(JsonParser.parseString(response).getAsJsonObject(), "player.inventory_set_async");
        } catch (IllegalStateException error) {
            if (isMissingNativeHost(response)) {
                // Pure-Java harnesses have no JNI host; a plugin join must still succeed there.
                System.err.println("Ferrite: inventory write skipped for " + playerUuid + " (no native host)");
                return response;
            }
            throw error;
        }
        return response;
    }

    private static boolean isMissingNativeHost(String response) {
        return response.contains("\"code\":\"java_exception\"")
                && (response.contains("UnsatisfiedLinkError") || response.contains("NoClassDefFoundError"));
    }

    /**
     * Accept both a raw result object (unit tests, legacy callers) and the contract envelope
     * {@code {ok, result}}. An error envelope becomes an explicit exception, never an empty
     * inventory or a silent success.
     */
    private static JsonObject unwrapResult(JsonObject parsed, String op) {
        if (!parsed.has("ok")
                || !parsed.get("ok").isJsonPrimitive()
                || !parsed.get("ok").getAsJsonPrimitive().isBoolean()) {
            return parsed;
        }
        if (!parsed.get("ok").getAsBoolean()) {
            String code = "unknown";
            String message = op + " failed";
            if (parsed.has("error") && parsed.get("error").isJsonObject()) {
                JsonObject error = parsed.getAsJsonObject("error");
                if (error.has("code") && error.get("code").isJsonPrimitive()) {
                    code = error.get("code").getAsString();
                }
                if (error.has("message") && error.get("message").isJsonPrimitive()) {
                    message = error.get("message").getAsString();
                }
            }
            throw new IllegalStateException(op + " failed: " + code + ": " + message);
        }
        if (!parsed.has("result") || !parsed.get("result").isJsonObject()) {
            throw new IllegalStateException(op + " returned no result object");
        }
        return parsed.getAsJsonObject("result");
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
