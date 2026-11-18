package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerriteEntitySnapshot;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.util.Map;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.inventory.EntityEquipment;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.Test;

class EntityEquipmentTest {

    @Test
    void playerEquipmentCoversAllSixSlotsAndDelegatesToPlayerInventory() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "EquipAlice", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
            EntityEquipment equipment = player.getEquipment();
            assertNotNull(equipment);

            ItemStack sword = new ItemStack(Material.DIAMOND_SWORD, 1);
            ItemStack shield = new ItemStack(Material.SHIELD, 1);
            ItemStack helmet = new ItemStack(Material.DIAMOND_HELMET, 1);
            ItemStack chest = new ItemStack(Material.IRON_CHESTPLATE, 1);
            ItemStack legs = new ItemStack(Material.CHAINMAIL_LEGGINGS, 1);
            ItemStack boots = new ItemStack(Material.LEATHER_BOOTS, 1);

            server.onMain(() -> {
                equipment.setItemInMainHand(sword);
                equipment.setItemInOffHand(shield);
                equipment.setHelmet(helmet);
                equipment.setChestplate(chest);
                equipment.setLeggings(legs);
                equipment.setBoots(boots);
            });

            // Defensive copy on set & get.
            sword.setAmount(42);
            ItemStack readSword = equipment.getItemInMainHand();
            assertEquals(Material.DIAMOND_SWORD, readSword.getType());
            assertEquals(1, readSword.getAmount());
            assertNotSame(readSword, equipment.getItemInMainHand());

            assertEquals(Material.SHIELD, equipment.getItem(EquipmentSlot.OFF_HAND).getType());
            assertEquals(Material.DIAMOND_HELMET, equipment.getHelmet().getType());
            assertEquals(Material.IRON_CHESTPLATE, equipment.getChestplate().getType());
            assertEquals(Material.CHAINMAIL_LEGGINGS, equipment.getLeggings().getType());
            assertEquals(Material.LEATHER_BOOTS, equipment.getBoots().getType());

            // PlayerInventory reflects the equipment writes.
            assertEquals(Material.DIAMOND_SWORD, player.getInventory().getItemInMainHand().getType());
            assertEquals(Material.DIAMOND_HELMET, player.getInventory().getHelmet().getType());

            ItemStack[] armor = equipment.getArmorContents();
            assertEquals(4, armor.length);
            assertEquals(Material.DIAMOND_HELMET, armor[0].getType());
            assertEquals(Material.IRON_CHESTPLATE, armor[1].getType());
            assertEquals(Material.CHAINMAIL_LEGGINGS, armor[2].getType());
            assertEquals(Material.LEATHER_BOOTS, armor[3].getType());

            // Unsupported slot and drop chance accessors fail loudly.
            assertThrows(UnsupportedOperationException.class, () -> equipment.getItem(EquipmentSlot.BODY));
            assertThrows(UnsupportedOperationException.class, () -> equipment.getItemInMainHandDropChance());
            assertThrows(UnsupportedOperationException.class, () -> equipment.setHelmetDropChance(0.5f));

            server.onMain(equipment::clear);
            assertTrue(equipment.getItemInMainHand().getType().isAir());
            assertNull(equipment.getHelmet());
            assertNull(equipment.getChestplate());
            assertNull(equipment.getLeggings());
            assertNull(equipment.getBoots());
        }
    }

    @Test
    void livingEntitySnapshotEquipmentSupportsAllSixSlotsAndDefensiveCopies() throws Exception {
        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            Entity entity = FerriteEntitySnapshot.create(
                    world,
                    UUID.randomUUID(),
                    EntityType.ZOMBIE,
                    "Zombie",
                    null,
                    2.5,
                    65.0,
                    3.5,
                    0.0f,
                    0.0f,
                    0.0,
                    0.0,
                    0.0,
                    20.0,
                    12,
                    Map.of(
                            EquipmentSlot.HAND, new ItemStack(Material.IRON_SWORD, 1),
                            EquipmentSlot.HEAD, new ItemStack(Material.GOLDEN_HELMET, 1)));

            assertTrue(entity instanceof LivingEntity);
            LivingEntity living = (LivingEntity) entity;
            EntityEquipment equipment = living.getEquipment();
            assertNotNull(equipment);

            assertEquals(Material.IRON_SWORD, equipment.getItemInMainHand().getType());
            assertEquals(Material.GOLDEN_HELMET, equipment.getHelmet().getType());
            assertNull(equipment.getChestplate());

            ItemStack offhand = new ItemStack(Material.TOTEM_OF_UNDYING, 1);
            equipment.setItem(EquipmentSlot.OFF_HAND, offhand, true);
            offhand.setType(Material.APPLE);
            assertEquals(Material.TOTEM_OF_UNDYING, equipment.getItemInOffHand().getType());

            ItemStack[] newArmor = new ItemStack[] {
                new ItemStack(Material.IRON_BOOTS, 1),
                new ItemStack(Material.IRON_LEGGINGS, 1),
                new ItemStack(Material.IRON_CHESTPLATE, 1),
                null
            };
            equipment.setArmorContents(newArmor);
            assertEquals(Material.IRON_BOOTS, equipment.getBoots().getType());
            assertEquals(Material.IRON_LEGGINGS, equipment.getLeggings().getType());
            assertEquals(Material.IRON_CHESTPLATE, equipment.getChestplate().getType());
            assertNull(equipment.getHelmet());

            assertThrows(UnsupportedOperationException.class, () -> equipment.setItem(EquipmentSlot.BODY, null));
            assertThrows(UnsupportedOperationException.class, () -> equipment.getHelmetDropChance());

            equipment.clear();
            assertTrue(equipment.getItemInMainHand().getType().isAir());
            assertTrue(equipment.getItemInOffHand().getType().isAir());
            assertNull(equipment.getBoots());
        }
    }
}
