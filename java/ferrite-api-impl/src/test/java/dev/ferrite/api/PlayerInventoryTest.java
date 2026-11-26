package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.entity.FerritePlayerInventory;
import dev.ferrite.api.inventory.PlayerInventorySnapshot;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.Test;

class PlayerInventoryTest {

    @Test
    void slotAccessCopySemanticsAndSnapshotBoundaryPreserveMetaAndPdc() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "InvAlice", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
            FerritePlayerInventory inv = (FerritePlayerInventory) player.getInventory();

            List<PlayerInventorySnapshot> published = new ArrayList<>();
            inv.setSnapshotSink(published::add);

            NamespacedKey key = new NamespacedKey("ferrite", "soulbound");
            ItemStack sword = new ItemStack(Material.DIAMOND_SWORD, 1);
            sword.editMeta(meta -> {
                meta.setDisplayName("Host Blade");
                meta.setLore(List.of("line 1"));
                ((Damageable) meta).setDamage(5);
                meta.setCustomModelData(101);
                meta.getPersistentDataContainer().set(key, PersistentDataType.BOOLEAN, true);
            });

            server.onMain(() -> {
                inv.setItem(0, sword);
                inv.setItemInMainHand(sword);
                inv.setItem(EquipmentSlot.HEAD, new ItemStack(Material.DIAMOND_HELMET, 1));
            });

            // Mutating original stack after insertion must not change stored inventory.
            sword.setAmount(1);
            sword.editMeta(meta -> {
                meta.setDisplayName("Mutated");
                meta.getPersistentDataContainer().set(key, PersistentDataType.BOOLEAN, false);
            });

            ItemStack stored = inv.getItem(0);
            assertNotSame(sword, stored);
            assertEquals("Host Blade", stored.getItemMeta().getDisplayName());
            assertEquals(5, stored.getDurability());
            assertEquals(101, stored.getItemMeta().getCustomModelData());
            assertEquals(true, stored.getPersistentDataContainer().get(key, PersistentDataType.BOOLEAN));

            // Mutating item read from inventory must not affect stored state.
            stored.editMeta(meta -> meta.setDisplayName("MutatedRead"));
            assertEquals("Host Blade", inv.getItem(0).getItemMeta().getDisplayName());
            assertEquals("Host Blade", inv.getItemInMainHand().getItemMeta().getDisplayName());
            assertEquals(Material.DIAMOND_HELMET, inv.getHelmet().getType());
            assertFalse(published.isEmpty());

            // Snapshot & serialize boundary round-trip.
            Map<String, Object> serializedSnapshot = inv.serializeSnapshot();
            server.onMain(() -> inv.clear());
            assertTrue(inv.isEmpty());

            server.onMain(() -> inv.restoreSnapshot(serializedSnapshot));
            ItemStack restored = inv.getItem(0);
            assertEquals("Host Blade", restored.getItemMeta().getDisplayName());
            assertEquals(List.of("line 1"), restored.getLore());
            assertEquals(5, restored.getDurability());
            assertEquals(101, restored.getItemMeta().getCustomModelData());
            assertEquals(true, restored.getPersistentDataContainer().get(key, PersistentDataType.BOOLEAN));
            assertEquals(Material.DIAMOND_HELMET, inv.getHelmet().getType());
        }
    }

    @Test
    void addItemAndRemoveItemRespectStackingMetadataAndOverflow() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "InvBob", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
            FerritePlayerInventory inv = (FerritePlayerInventory) player.getInventory();

            NamespacedKey key = new NamespacedKey("ferrite", "tier");
            ItemStack gemA = new ItemStack(Material.DIAMOND, 40);
            gemA.editPersistentDataContainer(pdc -> pdc.set(key, PersistentDataType.INTEGER, 1));
            ItemStack gemB = new ItemStack(Material.DIAMOND, 30);
            gemB.editPersistentDataContainer(pdc -> pdc.set(key, PersistentDataType.INTEGER, 2));
            ItemStack moreGemA = new ItemStack(Material.DIAMOND, 30);
            moreGemA.editPersistentDataContainer(pdc -> pdc.set(key, PersistentDataType.INTEGER, 1));

            server.onMain(() -> {
                assertTrue(inv.addItem(gemA, gemB, moreGemA).isEmpty());
            });

            // gemA (40) + moreGemA (30) should merge into slot 0 (64) and slot 2 (6),
            // leaving gemB separate in slot 1 (30).
            assertEquals(64, inv.getItem(0).getAmount());
            assertEquals(1, inv.getItem(0).getPersistentDataContainer().get(key, PersistentDataType.INTEGER));
            assertEquals(30, inv.getItem(1).getAmount());
            assertEquals(2, inv.getItem(1).getPersistentDataContainer().get(key, PersistentDataType.INTEGER));
            assertEquals(6, inv.getItem(2).getAmount());

            // Remove 65 of tier-1 diamonds -> removes slot 0 (64) and 1 from slot 2 (leaving 5).
            ItemStack removeReq = new ItemStack(Material.DIAMOND, 65);
            removeReq.editPersistentDataContainer(pdc -> pdc.set(key, PersistentDataType.INTEGER, 1));
            HashMap<Integer, ItemStack> unremoved = server.onMain(() -> inv.removeItem(removeReq));
            assertTrue(unremoved.isEmpty());
            assertNull(inv.getItem(0));
            assertEquals(5, inv.getItem(2).getAmount());

            // Request removing more than present returns exact remainder.
            ItemStack tooMany = new ItemStack(Material.DIAMOND, 10);
            tooMany.editPersistentDataContainer(pdc -> pdc.set(key, PersistentDataType.INTEGER, 1));
            HashMap<Integer, ItemStack> missing = server.onMain(() -> inv.removeItem(tooMany));
            assertEquals(1, missing.size());
            assertEquals(5, missing.get(0).getAmount());
            assertNull(inv.getItem(2));

            // Fill all 36 storage slots with unstackable swords and test addItem overflow.
            server.onMain(() -> inv.clear());
            ItemStack[] full = new ItemStack[36];
            for (int i = 0; i < 36; i++) {
                full[i] = new ItemStack(Material.IRON_SWORD, 1);
            }
            server.onMain(() -> inv.setStorageContents(full));
            HashMap<Integer, ItemStack> overflow = server.onMain(
                    () -> inv.addItem(new ItemStack(Material.ENDER_PEARL, 20)));
            assertEquals(1, overflow.size());
            assertEquals(20, overflow.get(0).getAmount());
            assertEquals(Material.ENDER_PEARL, overflow.get(0).getType());

            // removeItemAnySlot removes from offhand too, whereas removeItem only removes storage.
            server.onMain(() -> {
                inv.clear();
                inv.setItemInOffHand(new ItemStack(Material.ARROW, 12));
                assertEquals(1, inv.removeItem(new ItemStack(Material.ARROW, 5)).size());
                assertEquals(12, inv.getItemInOffHand().getAmount());
                assertTrue(inv.removeItemAnySlot(new ItemStack(Material.ARROW, 5)).isEmpty());
                assertEquals(7, inv.getItemInOffHand().getAmount());
                assertEquals(0, inv.firstEmpty());
            });
        }
    }

    @Test
    void outOfBoundsAndOffThreadMutationsFailExplicitly() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "InvThread", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
            FerritePlayerInventory inv = (FerritePlayerInventory) player.getInventory();

            assertThrows(IndexOutOfBoundsException.class, () -> inv.getItem(-1));
            assertThrows(IndexOutOfBoundsException.class, () -> inv.getItem(41));
            assertThrows(IndexOutOfBoundsException.class,
                    () -> server.onMain(() -> inv.setItem(41, new ItemStack(Material.STONE))));
            assertThrows(IllegalArgumentException.class,
                    () -> server.onMain(() -> inv.setHeldItemSlot(9)));
            assertThrows(IllegalArgumentException.class,
                    () -> server.onMain(() -> inv.setContents(new ItemStack[42])));
            assertThrows(IllegalArgumentException.class,
                    () -> server.onMain(() -> inv.setStorageContents(new ItemStack[37])));
            assertThrows(IllegalArgumentException.class,
                    () -> server.onMain(() -> inv.setArmorContents(new ItemStack[5])));
            assertThrows(IllegalArgumentException.class,
                    () -> server.onMain(() -> inv.setExtraContents(new ItemStack[2])));

            // setContents(getContents()) round-trips all 41 slots cleanly.
            server.onMain(() -> {
                inv.setItem(0, new ItemStack(Material.APPLE, 3));
                inv.setItemInOffHand(new ItemStack(Material.SHIELD, 1));
                inv.setContents(inv.getContents());
            });
            assertEquals(Material.APPLE, inv.getItem(0).getType());
            assertEquals(Material.SHIELD, inv.getItemInOffHand().getType());

            // Mutations off the main thread fail immediately with IllegalStateException.
            IllegalStateException offThread = assertThrows(
                    IllegalStateException.class,
                    () -> inv.setItem(0, new ItemStack(Material.DIRT)));
            assertTrue(offThread.getMessage().contains("main thread"));
            assertThrows(IllegalStateException.class, () -> inv.addItem(new ItemStack(Material.DIRT)));
            assertThrows(IllegalStateException.class, () -> inv.removeItem(new ItemStack(Material.DIRT)));
            assertThrows(IllegalStateException.class, inv::clear);
        }
    }
}
