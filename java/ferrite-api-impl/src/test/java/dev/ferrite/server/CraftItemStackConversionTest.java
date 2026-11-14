package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.craftbukkit.inventory.CraftItemStack;
import org.bukkit.inventory.EquipmentSlotGroup;
import org.bukkit.inventory.ItemFlag;
import org.bukkit.inventory.ItemStack;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** {@code CraftItemStack.asNMSCopy}/{@code asBukkitCopy} keep every metadata field, and a mirror writes through. */
class CraftItemStackConversionTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    private static org.bukkit.enchantments.Enchantment enchantment(String name) {
        return org.bukkit.Registry.ENCHANTMENT.get(NamespacedKey.minecraft(name));
    }

    @Test void nmsCopyKeepsEveryFieldAndTheBukkitCopyReadsThemBack() {
        var sword = new ItemStack(Material.DIAMOND_SWORD);
        var meta = (org.bukkit.inventory.meta.Damageable) sword.getItemMeta();
        meta.displayName(Component.text("Excalibur"));
        meta.lore(java.util.List.of(Component.text("old"), Component.text("relic")));
        meta.addEnchant(enchantment("sharpness"), 5, true);
        meta.setUnbreakable(true);
        meta.setDamage(7);
        meta.setCustomModelData(42);
        meta.addItemFlags(ItemFlag.HIDE_ENCHANTS, ItemFlag.HIDE_DYE);
        meta.getPersistentDataContainer().set(new NamespacedKey("ferrite", "owner"), PersistentDataType.STRING, "Steve");
        meta.setEnchantmentGlintOverride(Boolean.FALSE);
        meta.setMaxStackSize(16);
        meta.setMaxDamage(900);
        meta.setRarity(org.bukkit.inventory.ItemRarity.RARE);
        meta.setItemModel(NamespacedKey.minecraft("diamond_sword"));
        meta.setTooltipStyle(NamespacedKey.minecraft("default"));
        assertTrue(meta.addAttributeModifier(Attribute.ATTACK_DAMAGE, new AttributeModifier(NamespacedKey.minecraft("test_bonus"), 2.5,
                AttributeModifier.Operation.ADD_NUMBER, EquipmentSlotGroup.MAINHAND)));
        assertTrue(sword.setItemMeta(meta));

        var nms = CraftItemStack.asNMSCopy(sword);
        assertFalse(nms.isEmpty());
        assertEquals(1, nms.getCount());
        assertEquals(Material.DIAMOND_SWORD, nms.toBukkit().getType());
        var back = CraftItemStack.asBukkitCopy(nms);
        assertNotNull(back);
        var read = (org.bukkit.inventory.meta.Damageable) back.getItemMeta();
        assertTrue(read.hasDisplayName());
        assertEquals("Excalibur", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(read.displayName()));
        assertEquals(2, read.lore().size());
        assertEquals(5, read.getEnchantLevel(enchantment("sharpness")));
        assertTrue(read.isUnbreakable());
        assertEquals(7, read.getDamage());
        assertEquals(42, read.getCustomModelData());
        assertTrue(read.hasItemFlag(ItemFlag.HIDE_ENCHANTS));
        assertTrue(read.hasItemFlag(ItemFlag.HIDE_DYE));
        assertEquals("Steve", read.getPersistentDataContainer().get(new NamespacedKey("ferrite", "owner"), PersistentDataType.STRING));
        assertEquals(Boolean.FALSE, read.getEnchantmentGlintOverride());
        assertEquals(16, read.getMaxStackSize());
        assertEquals(900, read.getMaxDamage());
        assertEquals(org.bukkit.inventory.ItemRarity.RARE, read.getRarity());
        assertEquals(NamespacedKey.minecraft("diamond_sword"), read.getItemModel());
        assertEquals(NamespacedKey.minecraft("default"), read.getTooltipStyle());
        assertEquals(2.5, read.getAttributeModifiers(Attribute.ATTACK_DAMAGE).iterator().next().getAmount());
        assertTrue(read.equals(meta), "the round trip is lossless");
    }

    @Test void aMirrorWritesThroughToTheNmsStack() {
        var nms = CraftItemStack.asNMSCopy(new ItemStack(Material.STONE, 2));
        var mirror = CraftItemStack.asCraftMirror(nms);
        mirror.setAmount(9);
        mirror.setType(Material.DIAMOND);
        assertTrue(mirror.editMeta(meta -> meta.displayName(Component.text("Mirror"))));
        assertEquals(9, nms.getCount());
        assertEquals(Material.DIAMOND, nms.toBukkit().getType());
        assertTrue(nms.toBukkit().getItemMeta().hasDisplayName());
        // An ordinary copy does not alias the NMS stack.
        var copy = CraftItemStack.asCraftCopy(mirror);
        copy.setAmount(1);
        assertEquals(9, nms.getCount());
    }

    @Test void nullAndEmptyStacksConvertTheWayPluginsExpect() {
        assertTrue(CraftItemStack.asNMSCopy(null).isEmpty());
        assertNull(CraftItemStack.asBukkitCopy((net.minecraft.world.item.ItemStack) null));
        assertNull(CraftItemStack.asBukkitCopy(net.minecraft.world.item.ItemStack.EMPTY));
        assertTrue(net.minecraft.world.item.ItemStack.of(new ItemStack(Material.AIR)).isEmpty());
    }

    @Test void theWireCarriesTheExtraComponents() {
        var potion = new ItemStack(Material.POTION);
        var meta = (org.bukkit.inventory.meta.PotionMeta) potion.getItemMeta();
        meta.setBasePotionType(org.bukkit.potion.PotionType.HEALING);
        meta.addCustomEffect(new org.bukkit.potion.PotionEffect(org.bukkit.potion.PotionEffectType.SPEED, 200, 1, false, true, true), true);
        meta.setColor(org.bukkit.Color.RED);
        potion.setItemMeta(meta);
        var wire = ItemMetaWire.of(potion);
        var fields = wire.meta().split("\u001e", -1);
        assertEquals(10, fields.length);
        assertTrue(fields[9].contains("\"minecraft:potion_contents\""), fields[9]);
        assertTrue(fields[9].contains("\"minecraft:speed\""), fields[9]);
        assertTrue(fields[9].contains("custom_color"), fields[9]);

        var banner = new ItemStack(Material.WHITE_BANNER);
        var bannerMeta = (org.bukkit.inventory.meta.BannerMeta) banner.getItemMeta();
        bannerMeta.addPattern(new org.bukkit.block.banner.Pattern(org.bukkit.DyeColor.RED, org.bukkit.block.banner.PatternType.CROSS));
        banner.setItemMeta(bannerMeta);
        var bannerWire = ItemMetaWire.of(banner);
        assertTrue(bannerWire.meta().split("\u001e", -1)[9].contains("\"minecraft:banner_patterns\""));

        var rocket = new ItemStack(Material.FIREWORK_ROCKET);
        var rocketMeta = (org.bukkit.inventory.meta.FireworkMeta) rocket.getItemMeta();
        rocketMeta.addEffect(org.bukkit.FireworkEffect.builder().with(org.bukkit.FireworkEffect.Type.BALL_LARGE).withColor(org.bukkit.Color.BLUE).trail(true).build());
        rocketMeta.setPower(2);
        rocket.setItemMeta(rocketMeta);
        var rocketWire = ItemMetaWire.of(rocket);
        var extra = rocketWire.meta().split("\u001e", -1)[9];
        assertTrue(extra.contains("\"minecraft:fireworks\""), extra);
        assertTrue(extra.contains("\"large_ball\""), extra);
    }
}
