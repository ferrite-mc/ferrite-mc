package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.ferrite.api.entity.FerritePlayerInventory;
import dev.ferrite.api.inventory.FerriteHostInventoryBridge;
import dev.ferrite.api.inventory.PlayerInventorySnapshot;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.api.test.TestServer;
import java.util.Base64;
import java.util.List;
import java.util.UUID;
import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.Test;

class InventoryNbtBridgeTest {

    @Test
    void nbtBase64RoundTripsComponentsEnchantmentsAndNestedPdc() {
        try (TestServer ignored = new TestServer()) {
            NamespacedKey tagKey = new NamespacedKey("ferrite", "owner");
            NamespacedKey nestedKey = new NamespacedKey("ferrite", "nested");
            NamespacedKey levelKey = new NamespacedKey("ferrite", "level");

            ItemStack sword = new ItemStack(Material.DIAMOND_SWORD, 1);
            sword.editMeta(meta -> {
                meta.displayName(Component.text("Nbt Blade"));
                meta.lore(List.of(Component.text("line one"), Component.text("line two")));
                ((Damageable) meta).setDamage(12);
                meta.setUnbreakable(true);
                meta.setCustomModelData(42);
                meta.setHideTooltip(true);
                meta.addEnchant(Enchantment.SHARPNESS, 5, true);
                meta.getPersistentDataContainer().set(tagKey, PersistentDataType.STRING, "alice");
                var nested = meta.getPersistentDataContainer().getAdapterContext().newPersistentDataContainer();
                nested.set(levelKey, PersistentDataType.INTEGER, 9);
                meta.getPersistentDataContainer().set(nestedKey, PersistentDataType.TAG_CONTAINER, nested);
            });

            ItemStack[] contents = new ItemStack[PlayerInventorySnapshot.SIZE];
            contents[0] = sword;
            contents[1] = new ItemStack(Material.STONE, 16);
            PlayerInventorySnapshot snapshot = PlayerInventorySnapshot.capture(contents, 2);

            String setJson = FerriteHostInventoryBridge.encodeSetRequest(UUID.randomUUID(), snapshot);
            var parsed = JsonParser.parseString(setJson).getAsJsonObject();
            var slots = parsed.getAsJsonObject("slots");
            assertNotNull(slots.getAsJsonObject("0").get("nbt_base64"));
            assertFalse(slots.getAsJsonObject("1").has("nbt_base64"));
            assertTrue(slots.get("2") == null || slots.get("2").isJsonNull());

            // Simulate a player.inventory_get response with the same slot payloads.
            PlayerInventorySnapshot decoded = FerriteHostInventoryBridge.decodeGetResponse(setJson);
            assertEquals(2, decoded.heldSlot());
            ItemStack restored = decoded.item(0);
            assertNotNull(restored);
            assertEquals(Material.DIAMOND_SWORD, restored.getType());
            assertTrue(restored.getItemMeta().isUnbreakable());
            assertTrue(restored.getItemMeta().isHideTooltip());
            assertEquals(12, ((Damageable) restored.getItemMeta()).getDamage());
            assertEquals(42, restored.getItemMeta().getCustomModelData());
            assertEquals(5, restored.getEnchantmentLevel(Enchantment.SHARPNESS));
            assertEquals(
                    "alice",
                    restored.getItemMeta().getPersistentDataContainer().get(tagKey, PersistentDataType.STRING));
            var restoredNested = restored.getItemMeta().getPersistentDataContainer()
                    .get(nestedKey, PersistentDataType.TAG_CONTAINER);
            assertNotNull(restoredNested);
            assertEquals(9, restoredNested.get(levelKey, PersistentDataType.INTEGER));

            assertEquals(Material.STONE, decoded.item(1).getType());
            assertEquals(16, decoded.item(1).getAmount());
            assertNull(decoded.item(2));
        }
    }

    @Test
    void envelopeResultIsUnwrappedAndErrorsAreExplicit() {
        try (TestServer ignored = new TestServer()) {
            ItemStack[] contents = new ItemStack[PlayerInventorySnapshot.SIZE];
            contents[0] = new ItemStack(Material.DIAMOND, 3);
            PlayerInventorySnapshot snapshot = PlayerInventorySnapshot.capture(contents, 4);
            String setJson = FerriteHostInventoryBridge.encodeSetRequest(UUID.randomUUID(), snapshot);

            JsonObject envelope = new JsonObject();
            envelope.addProperty("ok", true);
            envelope.add("result", JsonParser.parseString(setJson));
            PlayerInventorySnapshot decoded = FerriteHostInventoryBridge.decodeGetResponse(envelope.toString());
            assertEquals(4, decoded.heldSlot());
            assertEquals(Material.DIAMOND, decoded.item(0).getType());
            assertEquals(3, decoded.item(0).getAmount());
            assertNull(decoded.item(1));

            JsonObject error = new JsonObject();
            error.addProperty("ok", false);
            JsonObject detail = new JsonObject();
            detail.addProperty("code", "player_not_found");
            detail.addProperty("message", "missing player");
            error.add("error", detail);
            IllegalStateException thrown = assertThrows(
                    IllegalStateException.class,
                    () -> FerriteHostInventoryBridge.decodeGetResponse(error.toString()));
            assertTrue(thrown.getMessage().contains("player_not_found"));
            assertTrue(thrown.getMessage().contains("missing player"));
        }
    }

    @Test
    void applyHostSnapshotDoesNotEchoBackToTheHost() {
        FerritePlayerInventory inventory = new FerritePlayerInventory(null);
        int[] pushed = {0};
        inventory.setSnapshotSink(snapshot -> pushed[0]++);
        ItemStack[] contents = new ItemStack[PlayerInventorySnapshot.SIZE];
        ItemStack stack = new ItemStack(Material.EMERALD, 2);
        contents[0] = stack;
        inventory.applyHostSnapshot(PlayerInventorySnapshot.capture(contents, 3));
        stack.setAmount(1);
        assertEquals(Material.EMERALD, inventory.getItem(0).getType());
        assertEquals(2, inventory.getItem(0).getAmount());
        assertEquals(3, inventory.getHeldItemSlot());
        assertEquals(0, pushed[0]);
    }

    @Test
    void invalidOrCorruptNbtBase64IsRejectedExplicitly() {
        try (TestServer ignored = new TestServer()) {
            assertNull(FerriteItemNbt.encodeItemBase64(null));
            assertNull(FerriteItemNbt.encodeItemBase64(new ItemStack(Material.STONE, 4)));

            assertThrows(
                    IllegalArgumentException.class,
                    () -> FerriteItemNbt.decodeItemBase64("not-valid-base64!!!", Material.STONE, 1));
            String nonCompound = Base64.getEncoder().encodeToString(new byte[] {3, 0, 0, 0, 0, 1});
            assertThrows(
                    IllegalArgumentException.class,
                    () -> FerriteItemNbt.decodeItemBase64(nonCompound, Material.STONE, 1));
        }
    }
}
