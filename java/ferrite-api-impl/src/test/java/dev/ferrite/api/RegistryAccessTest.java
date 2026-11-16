package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.item.FerriteItemMeta;
import dev.ferrite.api.registry.VanillaData;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.entity.poi.PoiType;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.util.Map;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.block.Biome;
import org.bukkit.damage.DamageType;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.Test;

/**
 * Regression coverage for the generated-data registry. Before this slice every registry was
 * an empty proxy, so touching {@code Enchantment.PROTECTION}, {@code Biome.PLAINS} or
 * {@code DamageType.ARROW} threw {@code NoSuchElementException} from their static
 * initializers and ItemMeta deserialization could not resolve an enchantment.
 */
class RegistryAccessTest {

    private static final NamespacedKey PROBE = new NamespacedKey("ferrite", "probe");

    @Test
    void generatedVanillaDataIsAvailable() {
        assertTrue(
                VanillaData.get().available(),
                "vanilla/26.3/generated is missing; run `python tools/fetch_vanilla.py`");
    }

    @Test
    void staticConstantsResolveFromGeneratedData() {
        assertEquals(NamespacedKey.minecraft("protection"), Enchantment.PROTECTION.getKey());
        assertEquals(4, Enchantment.PROTECTION.getMaxLevel());
        assertEquals(Enchantment.PROTECTION, Enchantment.PROTECTION);
        assertFalse(Enchantment.PROTECTION.isCursed());
        assertTrue(Enchantment.BINDING_CURSE.isCursed());

        assertEquals(NamespacedKey.minecraft("plains"), Biome.PLAINS.getKey());

        assertEquals(NamespacedKey.minecraft("arrow"), DamageType.ARROW.getKey());
        assertEquals("death.attack.arrow", DamageType.ARROW.getTranslationKey());
        assertEquals(0.1F, DamageType.ARROW.getExhaustion(), 0.0001F);
    }

    @Test
    void registryLookupsReturnDataBackedEntries() {
        Registry<Enchantment> enchantments =
                RegistryAccess.registryAccess().getRegistry(RegistryKey.ENCHANTMENT);
        assertTrue(enchantments.size() >= 40, "enchantment registry size " + enchantments.size());
        assertSame(Enchantment.PROTECTION, enchantments.get(NamespacedKey.minecraft("protection")));
        assertEquals(NamespacedKey.minecraft("protection"), enchantments.getKey(Enchantment.PROTECTION));
        assertEquals(enchantments.size(), enchantments.keyStream().count());

        Registry<Biome> biomes = RegistryAccess.registryAccess().getRegistry(RegistryKey.BIOME);
        assertTrue(biomes.size() >= 60, "biome registry size " + biomes.size());
        assertSame(Biome.PLAINS, biomes.get(NamespacedKey.minecraft("plains")));

        Registry<DamageType> damageTypes =
                RegistryAccess.registryAccess().getRegistry(RegistryKey.DAMAGE_TYPE);
        assertTrue(damageTypes.size() >= 50, "damage type registry size " + damageTypes.size());
        assertSame(DamageType.ARROW, damageTypes.get(NamespacedKey.minecraft("arrow")));

        Registry<PoiType> poiTypes =
                RegistryAccess.registryAccess().getRegistry(RegistryKey.POINT_OF_INTEREST_TYPE);
        PoiType armorer = poiTypes.get(NamespacedKey.minecraft("armorer"));
        assertNotNull(armorer);
        assertEquals(NamespacedKey.minecraft("armorer"), armorer.getKey());

        Registry<DataComponentType> components =
                RegistryAccess.registryAccess().getRegistry(RegistryKey.DATA_COMPONENT_TYPE);
        DataComponentType customData = components.get(NamespacedKey.minecraft("custom_data"));
        assertNotNull(customData);
        assertTrue(customData.isPersistent());

        RegistryAccess access = RegistryAccess.registryAccess();
        assertSame(EntityType.ZOMBIE, access.getRegistry(EntityType.class)
                .get(NamespacedKey.minecraft("zombie")));
        assertSame(Material.DIAMOND_SWORD, access.getRegistry(Material.class)
                .get(NamespacedKey.minecraft("diamond_sword")));
    }

    @Test
    void itemMetaEnchantmentAndPersistentDataPathsWork() {
        ItemMeta meta = new ItemStack(Material.DIAMOND_SWORD).getItemMeta();
        assertNotNull(meta);
        assertTrue(meta.addEnchant(Enchantment.SHARPNESS, 5, true));
        assertEquals(5, meta.getEnchantLevel(Enchantment.SHARPNESS));

        meta.getPersistentDataContainer().set(PROBE, PersistentDataType.STRING, "ferrite");
        assertEquals("ferrite",
                meta.getPersistentDataContainer().get(PROBE, PersistentDataType.STRING));

        FerriteItemMeta deserialized = FerriteItemMeta.deserialize(
                Map.of("enchants", Map.of("minecraft:sharpness", 3)));
        assertEquals(3, deserialized.getEnchantLevel(Enchantment.SHARPNESS));

        ItemStack stack = new ItemStack(Material.DIAMOND_SWORD);
        stack.setItemMeta(meta);
        assertEquals(5, stack.getItemMeta().getEnchantLevel(Enchantment.SHARPNESS));
    }

    @Test
    void attributeSoundJukeboxSongAndDataComponentTypesArePopulated() {
        RegistryAccess access = RegistryAccess.registryAccess();

        assertNotNull(org.bukkit.attribute.Attribute.ATTACK_DAMAGE);
        assertSame(
                org.bukkit.attribute.Attribute.ATTACK_DAMAGE,
                access.getRegistry(RegistryKey.ATTRIBUTE)
                        .get(NamespacedKey.minecraft("attack_damage")));

        assertNotNull(org.bukkit.Sound.ENTITY_EXPERIENCE_ORB_PICKUP);
        assertSame(
                org.bukkit.Sound.ENTITY_EXPERIENCE_ORB_PICKUP,
                access.getRegistry(RegistryKey.SOUND_EVENT)
                        .get(NamespacedKey.minecraft("entity.experience_orb.pickup")));

        assertNotNull(org.bukkit.JukeboxSong.CAT);
        assertSame(
                org.bukkit.JukeboxSong.CAT,
                access.getRegistry(RegistryKey.JUKEBOX_SONG)
                        .get(NamespacedKey.minecraft("cat")));

        assertNotNull(io.papermc.paper.datacomponent.DataComponentTypes.TOOL);
        assertNotNull(io.papermc.paper.datacomponent.DataComponentTypes.EQUIPPABLE);
        assertNotNull(io.papermc.paper.datacomponent.DataComponentTypes.USE_COOLDOWN);
        assertNotNull(io.papermc.paper.datacomponent.DataComponentTypes.JUKEBOX_PLAYABLE);
        assertNotNull(io.papermc.paper.datacomponent.DataComponentTypes.TOOLTIP_DISPLAY);
    }
}
