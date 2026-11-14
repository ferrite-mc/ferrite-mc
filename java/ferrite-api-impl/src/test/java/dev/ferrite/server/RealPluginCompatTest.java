package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Gaps that real third-party plugins (Skript, ACF/Multiverse, SkinsRestorer, cloud) hit while enabling. */
class RealPluginCompatTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @Test void modifyItemStackAppliesTheComponentsItemMetaCanHold() {
        var stack = org.bukkit.Bukkit.getUnsafe().modifyItemStack(new ItemStack(Material.DIAMOND_SWORD),
                "[custom_name='\"Blade\"',damage=7,unbreakable={},minecraft:enchantments={levels:{'minecraft:sharpness':3}}]");
        var meta = stack.getItemMeta();
        assertEquals("Blade", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(meta.displayName()));
        assertEquals(7, ((org.bukkit.inventory.meta.Damageable) meta).getDamage());
        assertTrue(meta.isUnbreakable());
        assertEquals(3, meta.getEnchantLevel(org.bukkit.enchantments.Enchantment.SHARPNESS));
    }

    @Test void namespacedValuesKeepTheirColonAndPotionsHoldABaseType() {
        var stack = org.bukkit.Bukkit.getUnsafe().modifyItemStack(new ItemStack(Material.POTION), "[potion_contents={potion:'minecraft:water'}]");
        assertEquals(org.bukkit.potion.PotionType.WATER, ((org.bukkit.inventory.meta.PotionMeta) stack.getItemMeta()).getBasePotionType());
        var bare = org.bukkit.Bukkit.getUnsafe().modifyItemStack(new ItemStack(Material.POTION), "[potion_contents=minecraft:water]");
        assertEquals(org.bukkit.potion.PotionType.WATER, ((org.bukkit.inventory.meta.PotionMeta) bare.getItemMeta()).getBasePotionType());
    }

    @Test void aComponentWithoutAMetaFieldIsKeptForMatchingButRefusedOnTheHostWire() {
        var stack = org.bukkit.Bukkit.getUnsafe().modifyItemStack(new ItemStack(Material.FIREWORK_ROCKET), "[fireworks={flight_duration:2}]");
        assertNotEquals(new ItemStack(Material.FIREWORK_ROCKET).getItemMeta(), stack.getItemMeta());
        assertThrows(UnsupportedOperationException.class, () -> ItemMetaWire.of(stack));
        assertThrows(UnsupportedOperationException.class, () -> org.bukkit.Bukkit.getUnsafe().modifyItemStack(new ItemStack(Material.STONE), "[!lore]"));
    }

    @Test void blockDataAcceptsAnEmptyOrBracketlessPropertyList() {
        assertEquals("minecraft:stone", org.bukkit.Bukkit.createBlockData(Material.STONE, "[]").getAsString());
        assertEquals("minecraft:oak_stairs[facing=east]", org.bukkit.Bukkit.createBlockData(Material.OAK_STAIRS, "[facing=east]").getAsString());
        assertEquals("minecraft:oak_stairs[facing=east]", org.bukkit.Bukkit.createBlockData(Material.OAK_STAIRS, "facing=east").getAsString());
    }

    @Test void gameProfileCarriesPropertiesLikeAuthlib() {
        var profile = new com.mojang.authlib.GameProfile(java.util.UUID.randomUUID(), "Skinned");
        assertTrue(profile.isComplete());
        profile.getProperties().put("textures", new com.mojang.authlib.properties.Property("textures", "value", "sig"));
        assertEquals("value", profile.getProperties().get("textures").iterator().next().value());
        assertSame(profile.getProperties(), profile.properties());
    }

    @Test void craftServerDeclaresGetCommandMapForReflectiveLookups() throws Exception {
        var method = org.bukkit.craftbukkit.CraftServer.class.getDeclaredMethod("getCommandMap");
        assertTrue(method.getReturnType().isAssignableFrom(org.bukkit.command.SimpleCommandMap.class));
        assertNotNull(org.bukkit.Bukkit.getServer().getCommandMap());
    }
}
