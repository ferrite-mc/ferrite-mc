package dev.ferrite.server;
@SuppressWarnings({"deprecation","unchecked","rawtypes"})
public class FerriteInternalBridge implements io.papermc.paper.InternalAPIBridge {
public com.destroystokyo.paper.SkinParts.Mutable allSkinParts() { throw new UnsupportedOperationException("Ferrite: not implemented: Server#allSkinParts"); }
public net.kyori.adventure.text.flattener.ComponentFlattener componentFlattener() { throw new UnsupportedOperationException("Ferrite: not implemented: Server#componentFlattener"); }
public org.bukkit.block.Biome constructLegacyCustomBiome() { return FerriteRegistryAccess.customBiome(); }
public org.bukkit.damage.DamageEffect getDamageEffect(java.lang.String name) { return org.bukkit.damage.DamageEffect.HURT; }
public io.papermc.paper.world.damagesource.CombatEntry createCombatEntry(org.bukkit.damage.DamageSource a0, float a1, io.papermc.paper.world.damagesource.FallLocationType a2, float a3) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#createCombatEntry"); }
public io.papermc.paper.world.damagesource.CombatEntry createCombatEntry(org.bukkit.entity.LivingEntity a0, org.bukkit.damage.DamageSource a1, float a2) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#createCombatEntry"); }
public org.bukkit.damage.DamageSource.Builder createDamageSourceBuilder(org.bukkit.damage.DamageType type) {
    java.util.Objects.requireNonNull(type, "type");
    return new org.bukkit.damage.DamageSource.Builder() {
        private org.bukkit.entity.Entity causing, direct;
        private org.bukkit.Location location;
        public org.bukkit.damage.DamageSource.Builder withCausingEntity(org.bukkit.entity.Entity entity) { causing = entity; return this; }
        public org.bukkit.damage.DamageSource.Builder withDirectEntity(org.bukkit.entity.Entity entity) { direct = entity; return this; }
        public org.bukkit.damage.DamageSource.Builder withDamageLocation(org.bukkit.Location value) { location = value; return this; }
        public org.bukkit.damage.DamageSource.Builder withDamageContext(java.util.function.Consumer<net.kyori.adventure.pointer.Pointers.Builder> consumer) { return this; }
        public org.bukkit.damage.DamageSource build() {
            return new org.bukkit.damage.DamageSource() {
                public org.bukkit.damage.DamageType getDamageType() { return type; }
                public org.bukkit.entity.Entity getCausingEntity() { return causing; }
                public org.bukkit.entity.Entity getDirectEntity() { return direct; }
                public org.bukkit.Location getDamageLocation() { return location; }
                public org.bukkit.Location getSourceLocation() { return location; }
                public boolean isIndirect() { return causative(); }
                private boolean causative() { return causing != null && direct != null && causing != direct; }
                public float getFoodExhaustion() { return 0.0f; }
                public boolean scalesWithDifficulty() { return false; }
                public net.kyori.adventure.pointer.Pointers getDamageContext() { return net.kyori.adventure.pointer.Pointers.empty(); }
                public String toString() { return "DamageSource[" + type.getKey() + "]"; }
            };
        }
    };
}
public org.bukkit.inventory.ItemStack createEmptyStack() { return new FerriteItemStack(org.bukkit.Material.AIR, 0); }
public io.papermc.paper.entity.poi.PoiType.Occupancy createOccupancy(java.lang.String a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#createOccupancy"); }
public <C> io.papermc.paper.persistence.PersistentDataKey<C> createPersistentDataKey(net.kyori.adventure.key.Key a0, org.bukkit.persistence.PersistentDataType<?, C> a1) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#createPersistentDataKey"); }
public io.papermc.paper.plugin.lifecycle.event.LifecycleEventManager<org.bukkit.plugin.Plugin> createPluginLifecycleEventManager(org.bukkit.plugin.java.JavaPlugin a0, java.util.function.BooleanSupplier a1) { return config -> { throw new UnsupportedOperationException("Ferrite: lifecycle handlers are not implemented"); }; }
public net.kyori.adventure.text.Component defaultMannequinDescription() { throw new UnsupportedOperationException("Ferrite: not implemented: Server#defaultMannequinDescription"); }
public io.papermc.paper.datacomponent.item.ResolvableProfile defaultMannequinProfile() { throw new UnsupportedOperationException("Ferrite: not implemented: Server#defaultMannequinProfile"); }
public org.bukkit.inventory.ItemStack deserializeItem(byte[] a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#deserializeItem"); }
public org.bukkit.attribute.Attributable getDefaultEntityAttributes(org.bukkit.NamespacedKey a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#getDefaultEntityAttributes"); }
public org.bukkit.entity.SpawnCategory getSpawnCategory(org.bukkit.entity.EntityType a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#getSpawnCategory"); }
public java.lang.String getStatisticCriteriaKey(org.bukkit.Statistic a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#getStatisticCriteriaKey"); }
public java.lang.String getTranslationKey(org.bukkit.entity.EntityType a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#getTranslationKey"); }
public boolean hasDefaultEntityAttributes(org.bukkit.NamespacedKey a0) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#hasDefaultEntityAttributes"); }
public <MODERN, LEGACY> org.bukkit.GameRule<LEGACY> legacyGameRuleBridge(org.bukkit.GameRule<MODERN> a0, java.util.function.Function<LEGACY, MODERN> a1, java.util.function.Function<MODERN, LEGACY> a2, java.lang.Class<LEGACY> a3) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#legacyGameRuleBridge"); }
public net.kyori.adventure.text.Component resolveWithContext(net.kyori.adventure.text.Component a0, org.bukkit.command.CommandSender a1, org.bukkit.entity.Entity a2, boolean a3) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#resolveWithContext"); }
public java.util.function.Predicate<io.papermc.paper.command.brigadier.CommandSourceStack> restricted(java.util.function.Predicate<io.papermc.paper.command.brigadier.CommandSourceStack> a0, boolean a1) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#restricted"); }
public org.bukkit.enchantments.EnchantmentOffer[] rollEnchantmentOffers(org.bukkit.inventory.ItemStack a0, int a1, int a2, int a3) { throw new UnsupportedOperationException("Ferrite: not implemented: Server#rollEnchantmentOffers"); }
public java.util.Set<org.bukkit.entity.Pose> validMannequinPoses() { throw new UnsupportedOperationException("Ferrite: not implemented: Server#validMannequinPoses"); }
}
