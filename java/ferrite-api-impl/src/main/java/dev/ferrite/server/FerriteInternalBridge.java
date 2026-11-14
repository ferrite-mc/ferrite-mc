package dev.ferrite.server;
@SuppressWarnings({"deprecation","unchecked","rawtypes"})
public class FerriteInternalBridge implements io.papermc.paper.InternalAPIBridge {
public com.destroystokyo.paper.SkinParts.Mutable allSkinParts() { return (com.destroystokyo.paper.SkinParts.Mutable) HostBridge.call("InternalBridge", "", "allSkinParts()", com.destroystokyo.paper.SkinParts.Mutable.class); }
public net.kyori.adventure.text.flattener.ComponentFlattener componentFlattener() { return (net.kyori.adventure.text.flattener.ComponentFlattener) HostBridge.call("InternalBridge", "", "componentFlattener()", net.kyori.adventure.text.flattener.ComponentFlattener.class); }
public org.bukkit.block.Biome constructLegacyCustomBiome() { return FerriteRegistryAccess.customBiome(); }
public org.bukkit.damage.DamageEffect getDamageEffect(java.lang.String name) { return FerriteDamageData.canonical().effect(name); }
public io.papermc.paper.world.damagesource.CombatEntry createCombatEntry(org.bukkit.damage.DamageSource a0, float a1, io.papermc.paper.world.damagesource.FallLocationType a2, float a3) { return (io.papermc.paper.world.damagesource.CombatEntry) HostBridge.call("InternalBridge", "", "createCombatEntry(DamageSource,float,FallLocationType,float)", io.papermc.paper.world.damagesource.CombatEntry.class, a0, a1, a2, a3); }
public io.papermc.paper.world.damagesource.CombatEntry createCombatEntry(org.bukkit.entity.LivingEntity a0, org.bukkit.damage.DamageSource a1, float a2) { return (io.papermc.paper.world.damagesource.CombatEntry) HostBridge.call("InternalBridge", "", "createCombatEntry(LivingEntity,DamageSource,float)", io.papermc.paper.world.damagesource.CombatEntry.class, a0, a1, a2); }
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
public io.papermc.paper.entity.poi.PoiType.Occupancy createOccupancy(java.lang.String a0) { return new Occupancy(a0); }
private record Occupancy(String name) implements io.papermc.paper.entity.poi.PoiType.Occupancy { @Override public String toString() { return name; } }
public <C> io.papermc.paper.persistence.PersistentDataKey<C> createPersistentDataKey(net.kyori.adventure.key.Key a0, org.bukkit.persistence.PersistentDataType<?, C> a1) { return (io.papermc.paper.persistence.PersistentDataKey<C>) HostBridge.call("InternalBridge", "", "createPersistentDataKey(Key,PersistentDataType)", io.papermc.paper.persistence.PersistentDataKey.class, a0, a1); }
public io.papermc.paper.plugin.lifecycle.event.LifecycleEventManager<org.bukkit.plugin.Plugin> createPluginLifecycleEventManager(org.bukkit.plugin.java.JavaPlugin a0, java.util.function.BooleanSupplier a1) { return new FerriteLifecycleManager(a0, a1); }
public net.kyori.adventure.text.Component defaultMannequinDescription() { return (net.kyori.adventure.text.Component) HostBridge.call("InternalBridge", "", "defaultMannequinDescription()", net.kyori.adventure.text.Component.class); }
public io.papermc.paper.datacomponent.item.ResolvableProfile defaultMannequinProfile() { return (io.papermc.paper.datacomponent.item.ResolvableProfile) HostBridge.call("InternalBridge", "", "defaultMannequinProfile()", io.papermc.paper.datacomponent.item.ResolvableProfile.class); }
public org.bukkit.inventory.ItemStack deserializeItem(byte[] a0) { return (org.bukkit.inventory.ItemStack) HostBridge.call("InternalBridge", "", "deserializeItem(byte[])", org.bukkit.inventory.ItemStack.class, a0); }
public org.bukkit.attribute.Attributable getDefaultEntityAttributes(org.bukkit.NamespacedKey a0) { return (org.bukkit.attribute.Attributable) HostBridge.call("InternalBridge", "", "getDefaultEntityAttributes(NamespacedKey)", org.bukkit.attribute.Attributable.class, a0); }
public org.bukkit.entity.SpawnCategory getSpawnCategory(org.bukkit.entity.EntityType a0) { return (org.bukkit.entity.SpawnCategory) HostBridge.call("InternalBridge", "", "getSpawnCategory(EntityType)", org.bukkit.entity.SpawnCategory.class, a0); }
public java.lang.String getStatisticCriteriaKey(org.bukkit.Statistic a0) { return (java.lang.String) HostBridge.call("InternalBridge", "", "getStatisticCriteriaKey(Statistic)", java.lang.String.class, a0); }
public java.lang.String getTranslationKey(org.bukkit.entity.EntityType a0) { return (java.lang.String) HostBridge.call("InternalBridge", "", "getTranslationKey(EntityType)", java.lang.String.class, a0); }
public boolean hasDefaultEntityAttributes(org.bukkit.NamespacedKey a0) { return HostBridge.callBoolean("InternalBridge", "", "hasDefaultEntityAttributes(NamespacedKey)", a0); }
public <MODERN, LEGACY> org.bukkit.GameRule<LEGACY> legacyGameRuleBridge(org.bukkit.GameRule<MODERN> a0, java.util.function.Function<LEGACY, MODERN> a1, java.util.function.Function<MODERN, LEGACY> a2, java.lang.Class<LEGACY> a3) { return new FerriteLegacyGameRule<>(a0, a1, a2, a3); }
public net.kyori.adventure.text.Component resolveWithContext(net.kyori.adventure.text.Component a0, org.bukkit.command.CommandSender a1, org.bukkit.entity.Entity a2, boolean a3) { return (net.kyori.adventure.text.Component) HostBridge.call("InternalBridge", "", "resolveWithContext(Component,CommandSender,Entity,boolean)", net.kyori.adventure.text.Component.class, a0, a1, a2, a3); }
public java.util.function.Predicate<io.papermc.paper.command.brigadier.CommandSourceStack> restricted(java.util.function.Predicate<io.papermc.paper.command.brigadier.CommandSourceStack> a0, boolean a1) { return (java.util.function.Predicate<io.papermc.paper.command.brigadier.CommandSourceStack>) HostBridge.call("InternalBridge", "", "restricted(Predicate,boolean)", java.util.function.Predicate.class, a0, a1); }
public org.bukkit.enchantments.EnchantmentOffer[] rollEnchantmentOffers(org.bukkit.inventory.ItemStack a0, int a1, int a2, int a3) { return (org.bukkit.enchantments.EnchantmentOffer[]) HostBridge.call("InternalBridge", "", "rollEnchantmentOffers(ItemStack,int,int,int)", org.bukkit.enchantments.EnchantmentOffer[].class, a0, a1, a2, a3); }
public java.util.Set<org.bukkit.entity.Pose> validMannequinPoses() { return (java.util.Set<org.bukkit.entity.Pose>) HostBridge.call("InternalBridge", "", "validMannequinPoses()", java.util.Set.class); }
}
