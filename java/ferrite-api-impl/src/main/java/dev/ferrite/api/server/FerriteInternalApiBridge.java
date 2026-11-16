package dev.ferrite.api.server;

import dev.ferrite.api.internal.FerriteContext;
import io.papermc.paper.InternalAPIBridge;
import io.papermc.paper.command.brigadier.CommandSourceStack;
import io.papermc.paper.datacomponent.item.ResolvableProfile;
import io.papermc.paper.entity.poi.PoiType;
import io.papermc.paper.persistence.PersistentDataKey;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEventManager;
import io.papermc.paper.world.damagesource.CombatEntry;
import io.papermc.paper.world.damagesource.FallLocationType;
import java.io.IOException;
import java.util.Set;
import java.util.function.BooleanSupplier;
import java.util.function.Function;
import java.util.function.Predicate;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.flattener.ComponentFlattener;
import org.bukkit.GameRule;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Statistic;
import org.bukkit.attribute.Attributable;
import org.bukkit.block.Biome;
import org.bukkit.command.CommandSender;
import org.bukkit.damage.DamageEffect;
import org.bukkit.damage.DamageSource;
import org.bukkit.damage.DamageType;
import org.bukkit.enchantments.EnchantmentOffer;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Pose;
import org.bukkit.entity.SpawnCategory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Supplies the Paper API's {@code InternalAPIBridge} service so API classes that assume a
 * CraftBukkit server can be constructed.
 *
 * <p>Only the bridge calls used by the implemented slice are real: lifecycle-manager
 * construction (required by every {@link JavaPlugin}), empty-stack creation, a component
 * flattener, pose defaults and command-source restriction. Everything else is explicitly
 * unsupported and names the method plus the calling plugin.
 */
public final class FerriteInternalApiBridge implements InternalAPIBridge {

    private static UnsupportedOperationException unsupported(String method) {
        String plugin = FerriteContext.currentPlugin();
        String suffix = plugin == null ? "" : " (called by plugin '" + plugin + "')";
        return new UnsupportedOperationException(
                "Paper InternalAPIBridge." + method
                        + " is not supported by the Ferrite facade yet" + suffix);
    }

    @Override
    public Biome constructLegacyCustomBiome() {
        throw unsupported("constructLegacyCustomBiome");
    }

    @Override
    public CombatEntry createCombatEntry(
            LivingEntity entity, DamageSource source, float damage) {
        throw unsupported("createCombatEntry");
    }

    @Override
    public CombatEntry createCombatEntry(
            DamageSource source, float damage, FallLocationType fallLocation, float fallDistance) {
        throw unsupported("createCombatEntry");
    }

    @Override
    public Predicate<CommandSourceStack> restricted(
            Predicate<CommandSourceStack> predicate, boolean restricted) {
        return predicate;
    }

    @Override
    public ResolvableProfile defaultMannequinProfile() {
        throw unsupported("defaultMannequinProfile");
    }

    @Override
    public com.destroystokyo.paper.SkinParts.Mutable allSkinParts() {
        throw unsupported("allSkinParts");
    }

    @Override
    public Component defaultMannequinDescription() {
        return Component.empty();
    }

    @Override
    public <MODERN, LEGACY> GameRule<LEGACY> legacyGameRuleBridge(
            GameRule<MODERN> rule,
            Function<LEGACY, MODERN> toModern,
            Function<MODERN, LEGACY> toLegacy,
            Class<LEGACY> legacyType) {
        throw unsupported("legacyGameRuleBridge");
    }

    @Override
    public Set<Pose> validMannequinPoses() {
        return Set.of(Pose.STANDING);
    }

    @Override
    public PoiType.Occupancy createOccupancy(String occupancy) {
        throw unsupported("createOccupancy");
    }

    @Override
    public DamageSource.Builder createDamageSourceBuilder(DamageType damageType) {
        throw unsupported("createDamageSourceBuilder");
    }

    @Override
    public DamageEffect getDamageEffect(String key) {
        throw unsupported("getDamageEffect");
    }

    @Override
    public String getTranslationKey(EntityType entityType) {
        return "entity.minecraft." + entityType.getKey().getKey();
    }

    @Override
    public SpawnCategory getSpawnCategory(EntityType entityType) {
        throw unsupported("getSpawnCategory");
    }

    @Override
    public ItemStack deserializeItem(byte[] data) {
        throw unsupported("deserializeItem");
    }

    @Override
    public boolean hasDefaultEntityAttributes(NamespacedKey key) {
        return false;
    }

    @Override
    public Attributable getDefaultEntityAttributes(NamespacedKey key) {
        throw unsupported("getDefaultEntityAttributes");
    }

    @Override
    public String getStatisticCriteriaKey(Statistic statistic) {
        throw unsupported("getStatisticCriteriaKey");
    }

    @Override
    public LifecycleEventManager<Plugin> createPluginLifecycleEventManager(
            JavaPlugin plugin, BooleanSupplier allowsLifecycleRegistration) {
        return new FerriteLifecycleEventManager();
    }

    @Override
    public ItemStack createEmptyStack() {
        return new ItemStack(Material.AIR, 0);
    }

    @Override
    public Component resolveWithContext(
            Component component, CommandSender sender, Entity entity, boolean stripEvents)
            throws IOException {
        throw unsupported("resolveWithContext");
    }

    @Override
    public ComponentFlattener componentFlattener() {
        return ComponentFlattener.basic();
    }

    @Override
    public <C> PersistentDataKey<C> createPersistentDataKey(
            Key key, PersistentDataType<?, C> dataType) {
        throw unsupported("createPersistentDataKey");
    }

    @Override
    public EnchantmentOffer[] rollEnchantmentOffers(
            ItemStack itemStack, int level, int enchantability, int seed) {
        throw unsupported("rollEnchantmentOffers");
    }
}
