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
import java.util.Objects;
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
        // Biome.CUSTOM is created through this bridge during Biome's own static
        // initializer. It is not a datapack biome, so index it after the generated ones.
        return new dev.ferrite.api.registry.FerriteBiome(
                NamespacedKey.minecraft("custom"),
                dev.ferrite.api.registry.VanillaData.get().biomes().size());
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
        return dev.ferrite.api.entity.FerriteSkinParts.allParts();
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
        final DamageType type = java.util.Objects.requireNonNull(damageType, "damageType");
        return new DamageSource.Builder() {
            private org.bukkit.entity.Entity causingEntity;
            private org.bukkit.entity.Entity directEntity;
            private org.bukkit.Location damageLocation;
            private net.kyori.adventure.pointer.Pointers context =
                    net.kyori.adventure.pointer.Pointers.empty();

            @Override
            public DamageSource.Builder withCausingEntity(org.bukkit.entity.Entity entity) {
                this.causingEntity = entity;
                return this;
            }

            @Override
            public DamageSource.Builder withDirectEntity(org.bukkit.entity.Entity entity) {
                this.directEntity = entity;
                return this;
            }

            @Override
            public DamageSource.Builder withDamageLocation(org.bukkit.Location location) {
                this.damageLocation = location == null ? null : location.clone();
                return this;
            }

            @Override
            public DamageSource.Builder withDamageContext(
                    java.util.function.Consumer<net.kyori.adventure.pointer.Pointers.Builder> consumer) {
                if (consumer != null) {
                    net.kyori.adventure.pointer.Pointers.Builder builder =
                            net.kyori.adventure.pointer.Pointers.builder();
                    consumer.accept(builder);
                    this.context = builder.build();
                }
                return this;
            }

            @Override
            public DamageSource build() {
                final org.bukkit.entity.Entity causing = this.causingEntity;
                final org.bukkit.entity.Entity direct = this.directEntity;
                final org.bukkit.Location loc = this.damageLocation == null ? null : this.damageLocation.clone();
                final net.kyori.adventure.pointer.Pointers pointers = this.context;
                return new DamageSource() {
                    @Override
                    public DamageType getDamageType() {
                        return type;
                    }

                    @Override
                    public org.bukkit.entity.Entity getCausingEntity() {
                        return causing;
                    }

                    @Override
                    public org.bukkit.entity.Entity getDirectEntity() {
                        return direct;
                    }

                    @Override
                    public org.bukkit.Location getDamageLocation() {
                        return loc == null ? null : loc.clone();
                    }

                    @Override
                    public org.bukkit.Location getSourceLocation() {
                        if (loc != null) {
                            return loc.clone();
                        }
                        if (direct != null) {
                            return direct.getLocation();
                        }
                        return causing == null ? null : causing.getLocation();
                    }

                    @Override
                    public boolean isIndirect() {
                        return causing != null && direct != null && causing != direct;
                    }

                    @Override
                    public float getFoodExhaustion() {
                        return 0.1f;
                    }

                    @Override
                    public boolean scalesWithDifficulty() {
                        return false;
                    }

                    @Override
                    public net.kyori.adventure.pointer.Pointers getDamageContext() {
                        return pointers;
                    }
                };
            }
        };
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
        Objects.requireNonNull(entityType, "entityType");
        return switch (entityType) {
            case ZOMBIE, SKELETON, CREEPER, SPIDER, CAVE_SPIDER, ENDERMAN, WITCH, SLIME,
                    MAGMA_CUBE, BLAZE, GHAST, PHANTOM, DROWNED, HUSK, STRAY, PILLAGER,
                    VINDICATOR, EVOKER, RAVAGER, VEX, WITHER, ENDER_DRAGON, WARDEN,
                    PIGLIN, PIGLIN_BRUTE, HOGLIN, ZOGLIN, GUARDIAN, ELDER_GUARDIAN,
                    SHULKER, SILVERFISH, ENDERMITE, BREEZE, BOGGED, CREAKING -> SpawnCategory.MONSTER;
            case COW, PIG, SHEEP, CHICKEN, HORSE, DONKEY, MULE, WOLF, CAT, OCELOT, FOX,
                    RABBIT, BEE, TURTLE, PANDA, POLAR_BEAR, LLAMA, TRADER_LLAMA, GOAT,
                    FROG, CAMEL, SNIFFER, ARMADILLO, MOOSHROOM, STRIDER, VILLAGER,
                    WANDERING_TRADER, IRON_GOLEM, SNOW_GOLEM -> SpawnCategory.ANIMAL;
            case DOLPHIN, SQUID -> SpawnCategory.WATER_ANIMAL;
            case COD, SALMON, PUFFERFISH, TROPICAL_FISH, TADPOLE -> SpawnCategory.WATER_AMBIENT;
            case GLOW_SQUID -> SpawnCategory.WATER_UNDERGROUND_CREATURE;
            case BAT -> SpawnCategory.AMBIENT;
            case AXOLOTL -> SpawnCategory.AXOLOTL;
            default -> SpawnCategory.MISC;
        };
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
        Objects.requireNonNull(statistic, "statistic");
        String prefix = switch (statistic.getType()) {
            case UNTYPED -> "minecraft.custom";
            case BLOCK -> "minecraft.mined";
            case ITEM -> "minecraft.used";
            case ENTITY -> "minecraft.killed";
        };
        return prefix + ":" + statistic.getKey().asString().replace(':', '.');
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
        return component;
    }

    @Override
    public ComponentFlattener componentFlattener() {
        return ComponentFlattener.basic();
    }

    @Override
    public <C> PersistentDataKey<C> createPersistentDataKey(
            Key key, PersistentDataType<?, C> dataType) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(dataType, "dataType");
        NamespacedKey namespacedKey = new NamespacedKey(key.namespace(), key.value());
        return new PersistentDataKey<>() {
            @Override
            public NamespacedKey key() {
                return namespacedKey;
            }

            @Override
            public PersistentDataType<?, C> dataType() {
                return dataType;
            }
        };
    }

    @Override
    public EnchantmentOffer[] rollEnchantmentOffers(
            ItemStack itemStack, int level, int enchantability, int seed) {
        throw unsupported("rollEnchantmentOffers");
    }
}
