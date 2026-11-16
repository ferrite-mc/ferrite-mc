// Clean-room test stub for CraftPlayer. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import java.util.Objects;
import java.util.UUID;

/** Concrete test subclass of {@link CraftPlayer} for unit tests. */
@SuppressWarnings({"deprecation", "removal", "unchecked", "rawtypes"})
public final class StubCraftPlayer extends CraftPlayer {
    private final UUID uniqueId;

    public StubCraftPlayer(UUID uniqueId) {
        this.uniqueId = Objects.requireNonNull(uniqueId, "uniqueId");
    }

    @Override
    public UUID getUniqueId() {
        return uniqueId;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof StubCraftPlayer stub && uniqueId.equals(stub.uniqueId);
    }

    @Override
    public int hashCode() {
        return uniqueId.hashCode();
    }

    @Override
    public void abandonConversation(org.bukkit.conversations.Conversation p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#abandonConversation");
    }

    @Override
    public void abandonConversation(org.bukkit.conversations.Conversation p0, org.bukkit.conversations.ConversationAbandonedEvent p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#abandonConversation");
    }

    @Override
    public void acceptConversationInput(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#acceptConversationInput");
    }

    @Override
    public java.lang.Iterable<? extends net.kyori.adventure.bossbar.BossBar> activeBossBars() {
        throw new UnsupportedOperationException("StubCraftPlayer#activeBossBars");
    }

    @Override
    public void addAdditionalAlliedRule(net.kyori.adventure.key.Key p0, java.util.function.Predicate<org.bukkit.entity.Entity> p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#addAdditionalAlliedRule");
    }

    @Override
    public void addAdditionalChatCompletions(java.util.Collection<java.lang.String> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addAdditionalChatCompletions");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addAttachment");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#addAttachment");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0, java.lang.String p1, boolean p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#addAttachment");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0, java.lang.String p1, boolean p2, int p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#addAttachment");
    }

    @Override
    public void addCustomChatCompletions(java.util.Collection<java.lang.String> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addCustomChatCompletions");
    }

    @Override
    public boolean addPassenger(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addPassenger");
    }

    @Override
    public boolean addPotionEffect(org.bukkit.potion.PotionEffect p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addPotionEffect");
    }

    @Override
    public boolean addPotionEffects(java.util.Collection<org.bukkit.potion.PotionEffect> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addPotionEffects");
    }

    @Override
    public void addResourcePack(java.util.UUID p0, java.lang.String p1, byte[] p2, java.lang.String p3, boolean p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#addResourcePack");
    }

    @Override
    public boolean addScoreboardTag(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#addScoreboardTag");
    }

    @Override
    public int applyMending(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#applyMending");
    }

    @Override
    public void attack(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#attack");
    }

    @Override
    public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(java.lang.String p0, java.time.Duration p1, java.lang.String p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#ban");
    }

    @Override
    public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(java.lang.String p0, java.time.Duration p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#ban");
    }

    @Override
    public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(java.lang.String p0, java.time.Instant p1, java.lang.String p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#ban");
    }

    @Override
    public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(java.lang.String p0, java.time.Instant p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#ban");
    }

    @Override
    public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(java.lang.String p0, java.util.Date p1, java.lang.String p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#ban");
    }

    @Override
    public <E extends org.bukkit.BanEntry<? super com.destroystokyo.paper.profile.PlayerProfile>> E ban(java.lang.String p0, java.util.Date p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#ban");
    }

    @Override
    public org.bukkit.BanEntry<java.net.InetAddress> banIp(java.lang.String p0, java.time.Duration p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#banIp");
    }

    @Override
    public org.bukkit.BanEntry<java.net.InetAddress> banIp(java.lang.String p0, java.time.Instant p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#banIp");
    }

    @Override
    public org.bukkit.BanEntry<java.net.InetAddress> banIp(java.lang.String p0, java.util.Date p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#banIp");
    }

    @Override
    public boolean beginConversation(org.bukkit.conversations.Conversation p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#beginConversation");
    }

    @Override
    public boolean breakBlock(org.bukkit.block.Block p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#breakBlock");
    }

    @Override
    public void broadcastHurtAnimation(java.util.Collection<org.bukkit.entity.Player> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#broadcastHurtAnimation");
    }

    @Override
    public void broadcastSlotBreak(org.bukkit.inventory.EquipmentSlot p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#broadcastSlotBreak");
    }

    @Override
    public void broadcastSlotBreak(org.bukkit.inventory.EquipmentSlot p0, java.util.Collection<org.bukkit.entity.Player> p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#broadcastSlotBreak");
    }

    @Override
    public int calculateTotalExperiencePoints() {
        throw new UnsupportedOperationException("StubCraftPlayer#calculateTotalExperiencePoints");
    }

    @Override
    public boolean canBreatheUnderwater() {
        throw new UnsupportedOperationException("StubCraftPlayer#canBreatheUnderwater");
    }

    @Override
    public boolean canCriticalAttack(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#canCriticalAttack");
    }

    @Override
    public boolean canSee(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#canSee");
    }

    @Override
    public boolean canSee(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#canSee");
    }

    @Override
    public boolean canUseEquipmentSlot(org.bukkit.inventory.EquipmentSlot p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#canUseEquipmentSlot");
    }

    @Override
    public void chat(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#chat");
    }

    @Override
    public void clearActiveItem() {
        throw new UnsupportedOperationException("StubCraftPlayer#clearActiveItem");
    }

    @Override
    public boolean clearActivePotionEffects() {
        throw new UnsupportedOperationException("StubCraftPlayer#clearActivePotionEffects");
    }

    @Override
    public void closeInventory(org.bukkit.event.inventory.InventoryCloseEvent.Reason p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#closeInventory");
    }

    @Override
    public boolean collidesAt(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#collidesAt");
    }

    @Override
    public void completeUsingActiveItem() {
        throw new UnsupportedOperationException("StubCraftPlayer#completeUsingActiveItem");
    }

    @Override
    public org.bukkit.entity.Entity copy() {
        throw new UnsupportedOperationException("StubCraftPlayer#copy");
    }

    @Override
    public org.bukkit.entity.Entity copy(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#copy");
    }

    @Override
    public org.bukkit.entity.EntitySnapshot createSnapshot() {
        throw new UnsupportedOperationException("StubCraftPlayer#createSnapshot");
    }

    @Override
    public net.kyori.adventure.text.Component customName() {
        throw new UnsupportedOperationException("StubCraftPlayer#customName");
    }

    @Override
    public void customName(net.kyori.adventure.text.Component p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#customName");
    }

    @Override
    public void damage(double p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#damage");
    }

    @Override
    public void damage(double p0, org.bukkit.damage.DamageSource p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#damage");
    }

    @Override
    public void damage(double p0, org.bukkit.entity.Entity p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#damage");
    }

    @Override
    public void damageItemStack(org.bukkit.inventory.EquipmentSlot p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#damageItemStack");
    }

    @Override
    public org.bukkit.inventory.ItemStack damageItemStack(org.bukkit.inventory.ItemStack p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#damageItemStack");
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic p0) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#decrementStatistic");
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic p0, int p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#decrementStatistic");
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic p0, org.bukkit.Material p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#decrementStatistic");
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic p0, org.bukkit.Material p1, int p2) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#decrementStatistic");
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic p0, org.bukkit.entity.EntityType p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#decrementStatistic");
    }

    @Override
    public void decrementStatistic(org.bukkit.Statistic p0, org.bukkit.entity.EntityType p1, int p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#decrementStatistic");
    }

    @Override
    public int discoverRecipes(java.util.Collection<org.bukkit.NamespacedKey> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#discoverRecipes");
    }

    @Override
    public net.kyori.adventure.text.Component displayName() {
        throw new UnsupportedOperationException("StubCraftPlayer#displayName");
    }

    @Override
    public void displayName(net.kyori.adventure.text.Component p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#displayName");
    }

    @Override
    public boolean dropItem(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#dropItem");
    }

    @Override
    public org.bukkit.entity.Item dropItem(int p0, int p1, boolean p2, java.util.function.Consumer<org.bukkit.entity.Item> p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#dropItem");
    }

    @Override
    public org.bukkit.entity.Item dropItem(org.bukkit.inventory.EquipmentSlot p0, int p1, boolean p2, java.util.function.Consumer<org.bukkit.entity.Item> p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#dropItem");
    }

    @Override
    public org.bukkit.entity.Item dropItem(org.bukkit.inventory.ItemStack p0, boolean p1, java.util.function.Consumer<org.bukkit.entity.Item> p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#dropItem");
    }

    @Override
    public boolean eject() {
        throw new UnsupportedOperationException("StubCraftPlayer#eject");
    }

    @Override
    public org.bukkit.entity.Firework fireworkBoost(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#fireworkBoost");
    }

    @Override
    public boolean fromMobSpawner() {
        throw new UnsupportedOperationException("StubCraftPlayer#fromMobSpawner");
    }

    @Override
    public double getAbsorptionAmount() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAbsorptionAmount");
    }

    @Override
    public org.bukkit.inventory.ItemStack getActiveItem() {
        throw new UnsupportedOperationException("StubCraftPlayer#getActiveItem");
    }

    @Override
    public org.bukkit.inventory.EquipmentSlot getActiveItemHand() {
        throw new UnsupportedOperationException("StubCraftPlayer#getActiveItemHand");
    }

    @Override
    public int getActiveItemRemainingTime() {
        throw new UnsupportedOperationException("StubCraftPlayer#getActiveItemRemainingTime");
    }

    @Override
    public int getActiveItemUsedTime() {
        throw new UnsupportedOperationException("StubCraftPlayer#getActiveItemUsedTime");
    }

    @Override
    public java.util.Collection<org.bukkit.potion.PotionEffect> getActivePotionEffects() {
        throw new UnsupportedOperationException("StubCraftPlayer#getActivePotionEffects");
    }

    @Override
    public java.util.Map<net.kyori.adventure.key.Key, java.util.function.Predicate<org.bukkit.entity.Entity>> getAdditionalAlliedRules() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAdditionalAlliedRules");
    }

    @Override
    public java.net.InetSocketAddress getAddress() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAddress");
    }

    @Override
    public org.bukkit.advancement.AdvancementProgress getAdvancementProgress(org.bukkit.advancement.Advancement p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getAdvancementProgress");
    }

    @Override
    public boolean getAffectsSpawning() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAffectsSpawning");
    }

    @Override
    public float getAirDrag() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAirDrag");
    }

    @Override
    public boolean getAllowFlight() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAllowFlight");
    }

    @Override
    public int getArrowCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#getArrowCooldown");
    }

    @Override
    public int getArrowsInBody() {
        throw new UnsupportedOperationException("StubCraftPlayer#getArrowsInBody");
    }

    @Override
    public java.lang.String getAsString() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAsString");
    }

    @Override
    public float getAttackCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#getAttackCooldown");
    }

    @Override
    public org.bukkit.attribute.AttributeInstance getAttribute(org.bukkit.attribute.Attribute p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getAttribute");
    }

    @Override
    public org.bukkit.Location getBedLocation() {
        throw new UnsupportedOperationException("StubCraftPlayer#getBedLocation");
    }

    @Override
    public int getBeeStingerCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#getBeeStingerCooldown");
    }

    @Override
    public int getBeeStingersInBody() {
        throw new UnsupportedOperationException("StubCraftPlayer#getBeeStingersInBody");
    }

    @Override
    public float getBodyYaw() {
        throw new UnsupportedOperationException("StubCraftPlayer#getBodyYaw");
    }

    @Override
    public org.bukkit.util.BoundingBox getBoundingBox() {
        throw new UnsupportedOperationException("StubCraftPlayer#getBoundingBox");
    }

    @Override
    public boolean getCanPickupItems() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCanPickupItems");
    }

    @Override
    public org.bukkit.entity.EntityCategory getCategory() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCategory");
    }

    @Override
    public java.lang.String getClientBrandName() {
        throw new UnsupportedOperationException("StubCraftPlayer#getClientBrandName");
    }

    @Override
    public <T> T getClientOption(com.destroystokyo.paper.ClientOption<T> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getClientOption");
    }

    @Override
    public int getClientViewDistance() {
        throw new UnsupportedOperationException("StubCraftPlayer#getClientViewDistance");
    }

    @Override
    public java.util.Set<java.util.UUID> getCollidableExemptions() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCollidableExemptions");
    }

    @Override
    public io.papermc.paper.world.damagesource.CombatTracker getCombatTracker() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCombatTracker");
    }

    @Override
    public org.bukkit.Location getCompassTarget() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCompassTarget");
    }

    @Override
    public io.papermc.paper.connection.PlayerGameConnection getConnection() {
        throw new UnsupportedOperationException("StubCraftPlayer#getConnection");
    }

    @Override
    public int getCooldown(net.kyori.adventure.key.Key p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getCooldown");
    }

    @Override
    public int getCooldown(org.bukkit.Material p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getCooldown");
    }

    @Override
    public int getCooldown(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getCooldown");
    }

    @Override
    public float getCooldownPeriod() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCooldownPeriod");
    }

    @Override
    public float getCooledAttackStrength(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getCooledAttackStrength");
    }

    @Override
    public org.bukkit.Input getCurrentInput() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCurrentInput");
    }

    @Override
    public java.lang.String getCustomName() {
        throw new UnsupportedOperationException("StubCraftPlayer#getCustomName");
    }

    @Override
    public <T> T getData(io.papermc.paper.datacomponent.DataComponentType.Valued<T> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getData");
    }

    @Override
    public <T> T getDataOrDefault(io.papermc.paper.datacomponent.DataComponentType.Valued<? extends T> p0, T p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getDataOrDefault");
    }

    @Override
    public int getDeathScreenScore() {
        throw new UnsupportedOperationException("StubCraftPlayer#getDeathScreenScore");
    }

    @Override
    public org.bukkit.Sound getDeathSound() {
        throw new UnsupportedOperationException("StubCraftPlayer#getDeathSound");
    }

    @Override
    public double getDefaultGravity() {
        throw new UnsupportedOperationException("StubCraftPlayer#getDefaultGravity");
    }

    @Override
    public java.util.Set<org.bukkit.NamespacedKey> getDiscoveredRecipes() {
        throw new UnsupportedOperationException("StubCraftPlayer#getDiscoveredRecipes");
    }

    @Override
    public java.lang.String getDisplayName() {
        throw new UnsupportedOperationException("StubCraftPlayer#getDisplayName");
    }

    @Override
    public org.bukkit.Sound getDrinkingSound(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getDrinkingSound");
    }

    @Override
    public org.bukkit.Sound getEatingSound(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getEatingSound");
    }

    @Override
    public java.util.Set<org.bukkit.permissions.PermissionAttachmentInfo> getEffectivePermissions() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEffectivePermissions");
    }

    @Override
    public int getEnchantmentSeed() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEnchantmentSeed");
    }

    @Override
    public org.bukkit.inventory.Inventory getEnderChest() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEnderChest");
    }

    @Override
    public java.util.Collection<org.bukkit.entity.EnderPearl> getEnderPearls() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEnderPearls");
    }

    @Override
    public int getEntityId() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEntityId");
    }

    @Override
    public org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason getEntitySpawnReason() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEntitySpawnReason");
    }

    @Override
    public org.bukkit.inventory.EntityEquipment getEquipment() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEquipment");
    }

    @Override
    public float getExhaustion() {
        throw new UnsupportedOperationException("StubCraftPlayer#getExhaustion");
    }

    @Override
    public float getExp() {
        throw new UnsupportedOperationException("StubCraftPlayer#getExp");
    }

    @Override
    public int getExpCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#getExpCooldown");
    }

    @Override
    public int getExpToLevel() {
        throw new UnsupportedOperationException("StubCraftPlayer#getExpToLevel");
    }

    @Override
    public int getExperiencePointsNeededForNextLevel() {
        throw new UnsupportedOperationException("StubCraftPlayer#getExperiencePointsNeededForNextLevel");
    }

    @Override
    public double getEyeHeight() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEyeHeight");
    }

    @Override
    public double getEyeHeight(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getEyeHeight");
    }

    @Override
    public org.bukkit.Location getEyeLocation() {
        throw new UnsupportedOperationException("StubCraftPlayer#getEyeLocation");
    }

    @Override
    public org.bukkit.block.BlockFace getFacing() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFacing");
    }

    @Override
    public org.bukkit.Sound getFallDamageSound(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getFallDamageSound");
    }

    @Override
    public org.bukkit.Sound getFallDamageSoundBig() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFallDamageSoundBig");
    }

    @Override
    public org.bukkit.Sound getFallDamageSoundSmall() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFallDamageSoundSmall");
    }

    @Override
    public float getFallDistance() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFallDistance");
    }

    @Override
    public int getFireTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFireTicks");
    }

    @Override
    public long getFirstPlayed() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFirstPlayed");
    }

    @Override
    public org.bukkit.entity.FishHook getFishHook() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFishHook");
    }

    @Override
    public float getFlySpeed() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFlySpeed");
    }

    @Override
    public float getForwardsMovement() {
        throw new UnsupportedOperationException("StubCraftPlayer#getForwardsMovement");
    }

    @Override
    public int getFreezeTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFreezeTicks");
    }

    @Override
    public net.kyori.adventure.util.TriState getFrictionState() {
        throw new UnsupportedOperationException("StubCraftPlayer#getFrictionState");
    }

    @Override
    public double getGravity() {
        throw new UnsupportedOperationException("StubCraftPlayer#getGravity");
    }

    @Override
    public java.net.InetSocketAddress getHAProxyAddress() {
        throw new UnsupportedOperationException("StubCraftPlayer#getHAProxyAddress");
    }

    @Override
    public double getHealthScale() {
        throw new UnsupportedOperationException("StubCraftPlayer#getHealthScale");
    }

    @Override
    public double getHeight() {
        throw new UnsupportedOperationException("StubCraftPlayer#getHeight");
    }

    @Override
    public float getHurtDirection() {
        throw new UnsupportedOperationException("StubCraftPlayer#getHurtDirection");
    }

    @Override
    public org.bukkit.Sound getHurtSound() {
        throw new UnsupportedOperationException("StubCraftPlayer#getHurtSound");
    }

    @Override
    public org.bukkit.Sound getHurtSound(org.bukkit.damage.DamageSource p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getHurtSound");
    }

    @Override
    public java.time.Duration getIdleDuration() {
        throw new UnsupportedOperationException("StubCraftPlayer#getIdleDuration");
    }

    @Override
    public org.bukkit.inventory.PlayerInventory getInventory() {
        throw new UnsupportedOperationException("StubCraftPlayer#getInventory");
    }

    @Override
    public org.bukkit.inventory.ItemStack getItemInHand() {
        throw new UnsupportedOperationException("StubCraftPlayer#getItemInHand");
    }

    @Override
    public org.bukkit.inventory.ItemStack getItemInUse() {
        throw new UnsupportedOperationException("StubCraftPlayer#getItemInUse");
    }

    @Override
    public int getItemInUseTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getItemInUseTicks");
    }

    @Override
    public org.bukkit.inventory.ItemStack getItemOnCursor() {
        throw new UnsupportedOperationException("StubCraftPlayer#getItemOnCursor");
    }

    @Override
    public org.bukkit.entity.Player getKiller() {
        throw new UnsupportedOperationException("StubCraftPlayer#getKiller");
    }

    @Override
    public double getLastDamage() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastDamage");
    }

    @Override
    public org.bukkit.event.entity.EntityDamageEvent getLastDamageCause() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastDamageCause");
    }

    @Override
    public org.bukkit.Location getLastDeathLocation() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastDeathLocation");
    }

    @Override
    public long getLastLogin() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastLogin");
    }

    @Override
    public long getLastPlayed() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastPlayed");
    }

    @Override
    public long getLastSeen() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastSeen");
    }

    @Override
    public java.util.List<org.bukkit.block.Block> getLastTwoTargetBlocks(java.util.Set<org.bukkit.Material> p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getLastTwoTargetBlocks");
    }

    @Override
    public org.bukkit.entity.Entity getLeashHolder() throws java.lang.IllegalStateException {
        throw new UnsupportedOperationException("StubCraftPlayer#getLeashHolder");
    }

    @Override
    public int getLevel() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLevel");
    }

    @Override
    public java.util.List<org.bukkit.block.Block> getLineOfSight(java.util.Set<org.bukkit.Material> p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getLineOfSight");
    }

    @Override
    public java.util.Set<java.lang.String> getListeningPluginChannels() {
        throw new UnsupportedOperationException("StubCraftPlayer#getListeningPluginChannels");
    }

    @Override
    public java.lang.String getLocale() {
        throw new UnsupportedOperationException("StubCraftPlayer#getLocale");
    }

    @Override
    public org.bukkit.inventory.MainHand getMainHand() {
        throw new UnsupportedOperationException("StubCraftPlayer#getMainHand");
    }

    @Override
    public int getMaxFireTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getMaxFireTicks");
    }

    @Override
    public int getMaxFreezeTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getMaxFreezeTicks");
    }

    @Override
    public double getMaxHealth() {
        throw new UnsupportedOperationException("StubCraftPlayer#getMaxHealth");
    }

    @Override
    public int getMaximumAir() {
        throw new UnsupportedOperationException("StubCraftPlayer#getMaximumAir");
    }

    @Override
    public int getMaximumNoDamageTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getMaximumNoDamageTicks");
    }

    @Override
    public <T> T getMemory(org.bukkit.entity.memory.MemoryKey<T> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getMemory");
    }

    @Override
    public java.util.List<org.bukkit.metadata.MetadataValue> getMetadata(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getMetadata");
    }

    @Override
    public java.util.List<org.bukkit.entity.Entity> getNearbyEntities(double p0, double p1, double p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#getNearbyEntities");
    }

    @Override
    public int getNoActionTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getNoActionTicks");
    }

    @Override
    public int getNoDamageTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getNoDamageTicks");
    }

    @Override
    public org.bukkit.inventory.InventoryView getOpenInventory() {
        throw new UnsupportedOperationException("StubCraftPlayer#getOpenInventory");
    }

    @Override
    public org.bukkit.Location getOrigin() {
        throw new UnsupportedOperationException("StubCraftPlayer#getOrigin");
    }

    @Override
    public org.bukkit.entity.Entity getPassenger() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPassenger");
    }

    @Override
    public org.bukkit.persistence.PersistentDataContainer getPersistentDataContainer() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPersistentDataContainer");
    }

    @Override
    public org.bukkit.inventory.ItemStack getPickItemStack() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPickItemStack");
    }

    @Override
    public int getPing() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPing");
    }

    @Override
    public org.bukkit.block.PistonMoveReaction getPistonMoveReaction() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPistonMoveReaction");
    }

    @Override
    public float getPitch() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPitch");
    }

    @Override
    public org.bukkit.entity.Player getPlayer() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayer");
    }

    @Override
    public java.lang.String getPlayerListFooter() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerListFooter");
    }

    @Override
    public java.lang.String getPlayerListHeader() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerListHeader");
    }

    @Override
    public java.lang.String getPlayerListName() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerListName");
    }

    @Override
    public int getPlayerListOrder() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerListOrder");
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile getPlayerProfile() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerProfile");
    }

    @Override
    public long getPlayerTime() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerTime");
    }

    @Override
    public long getPlayerTimeOffset() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerTimeOffset");
    }

    @Override
    public org.bukkit.WeatherType getPlayerWeather() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPlayerWeather");
    }

    @Override
    public int getPortalCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPortalCooldown");
    }

    @Override
    public org.bukkit.entity.Pose getPose() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPose");
    }

    @Override
    public org.bukkit.Location getPotentialRespawnLocation() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPotentialRespawnLocation");
    }

    @Override
    public org.bukkit.potion.PotionEffect getPotionEffect(org.bukkit.potion.PotionEffectType p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getPotionEffect");
    }

    @Override
    public org.bukkit.GameMode getPreviousGameMode() {
        throw new UnsupportedOperationException("StubCraftPlayer#getPreviousGameMode");
    }

    @Override
    public int getProtocolVersion() {
        throw new UnsupportedOperationException("StubCraftPlayer#getProtocolVersion");
    }

    @Override
    public int getRemainingAir() {
        throw new UnsupportedOperationException("StubCraftPlayer#getRemainingAir");
    }

    @Override
    public io.papermc.paper.entity.RemovalReason getRemovalReason() {
        throw new UnsupportedOperationException("StubCraftPlayer#getRemovalReason");
    }

    @Override
    public org.bukkit.event.entity.EntityRemoveEvent.Cause getRemoveEventCause() {
        throw new UnsupportedOperationException("StubCraftPlayer#getRemoveEventCause");
    }

    @Override
    public boolean getRemoveWhenFarAway() {
        throw new UnsupportedOperationException("StubCraftPlayer#getRemoveWhenFarAway");
    }

    @Override
    public org.bukkit.event.player.PlayerResourcePackStatusEvent.Status getResourcePackStatus() {
        throw new UnsupportedOperationException("StubCraftPlayer#getResourcePackStatus");
    }

    @Override
    public org.bukkit.Location getRespawnLocation(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#getRespawnLocation");
    }

    @Override
    public int getSaturatedRegenRate() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSaturatedRegenRate");
    }

    @Override
    public float getSaturation() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSaturation");
    }

    @Override
    public io.papermc.paper.threadedregions.scheduler.EntityScheduler getScheduler() {
        throw new UnsupportedOperationException("StubCraftPlayer#getScheduler");
    }

    @Override
    public org.bukkit.scoreboard.Scoreboard getScoreboard() {
        throw new UnsupportedOperationException("StubCraftPlayer#getScoreboard");
    }

    @Override
    public java.lang.String getScoreboardEntryName() {
        throw new UnsupportedOperationException("StubCraftPlayer#getScoreboardEntryName");
    }

    @Override
    public java.util.Set<java.lang.String> getScoreboardTags() {
        throw new UnsupportedOperationException("StubCraftPlayer#getScoreboardTags");
    }

    @Override
    public int getSendViewDistance() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSendViewDistance");
    }

    @Override
    public java.util.Set<java.lang.Long> getSentChunkKeys() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSentChunkKeys");
    }

    @Override
    public java.util.Set<org.bukkit.Chunk> getSentChunks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSentChunks");
    }

    @Override
    public org.bukkit.Server getServer() {
        throw new UnsupportedOperationException("StubCraftPlayer#getServer");
    }

    @Override
    public org.bukkit.entity.Entity getShoulderEntityLeft() {
        throw new UnsupportedOperationException("StubCraftPlayer#getShoulderEntityLeft");
    }

    @Override
    public org.bukkit.entity.Entity getShoulderEntityRight() {
        throw new UnsupportedOperationException("StubCraftPlayer#getShoulderEntityRight");
    }

    @Override
    public float getSidewaysMovement() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSidewaysMovement");
    }

    @Override
    public int getSimulationDistance() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSimulationDistance");
    }

    @Override
    public int getSleepTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSleepTicks");
    }

    @Override
    public java.net.SocketAddress getSocketAddress() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSocketAddress");
    }

    @Override
    public org.bukkit.SoundCategory getSoundCategory() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSoundCategory");
    }

    @Override
    public float getSoundPitch() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSoundPitch");
    }

    @Override
    public float getSoundVolume() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSoundVolume");
    }

    @Override
    public org.bukkit.entity.SpawnCategory getSpawnCategory() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSpawnCategory");
    }

    @Override
    public org.bukkit.entity.Entity getSpectatorTarget() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSpectatorTarget");
    }

    @Override
    public int getStarvationRate() {
        throw new UnsupportedOperationException("StubCraftPlayer#getStarvationRate");
    }

    @Override
    public int getStatistic(org.bukkit.Statistic p0) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#getStatistic");
    }

    @Override
    public int getStatistic(org.bukkit.Statistic p0, org.bukkit.Material p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#getStatistic");
    }

    @Override
    public int getStatistic(org.bukkit.Statistic p0, org.bukkit.entity.EntityType p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#getStatistic");
    }

    @Override
    public org.bukkit.Sound getSwimHighSpeedSplashSound() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSwimHighSpeedSplashSound");
    }

    @Override
    public org.bukkit.Sound getSwimSound() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSwimSound");
    }

    @Override
    public org.bukkit.Sound getSwimSplashSound() {
        throw new UnsupportedOperationException("StubCraftPlayer#getSwimSplashSound");
    }

    @Override
    public org.bukkit.block.Block getTargetBlock(int p0, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetBlock");
    }

    @Override
    public org.bukkit.block.Block getTargetBlock(java.util.Set<org.bukkit.Material> p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetBlock");
    }

    @Override
    public org.bukkit.block.Block getTargetBlockExact(int p0, org.bukkit.FluidCollisionMode p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetBlockExact");
    }

    @Override
    public org.bukkit.block.BlockFace getTargetBlockFace(int p0, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetBlockFace");
    }

    @Override
    public org.bukkit.block.BlockFace getTargetBlockFace(int p0, org.bukkit.FluidCollisionMode p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetBlockFace");
    }

    @Override
    public com.destroystokyo.paper.block.TargetBlockInfo getTargetBlockInfo(int p0, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetBlockInfo");
    }

    @Override
    public org.bukkit.entity.Entity getTargetEntity(int p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetEntity");
    }

    @Override
    public com.destroystokyo.paper.entity.TargetEntityInfo getTargetEntityInfo(int p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#getTargetEntityInfo");
    }

    @Override
    public int getTicksLived() {
        throw new UnsupportedOperationException("StubCraftPlayer#getTicksLived");
    }

    @Override
    public int getTotalExperience() {
        throw new UnsupportedOperationException("StubCraftPlayer#getTotalExperience");
    }

    @Override
    public java.util.Set<org.bukkit.entity.Player> getTrackedBy() {
        throw new UnsupportedOperationException("StubCraftPlayer#getTrackedBy");
    }

    @Override
    public java.util.Set<org.bukkit.entity.Player> getTrackedPlayers() {
        throw new UnsupportedOperationException("StubCraftPlayer#getTrackedPlayers");
    }

    @Override
    public int getUnsaturatedRegenRate() {
        throw new UnsupportedOperationException("StubCraftPlayer#getUnsaturatedRegenRate");
    }

    @Override
    public float getUpwardsMovement() {
        throw new UnsupportedOperationException("StubCraftPlayer#getUpwardsMovement");
    }

    @Override
    public int getViewDistance() {
        throw new UnsupportedOperationException("StubCraftPlayer#getViewDistance");
    }

    @Override
    public java.net.InetSocketAddress getVirtualHost() {
        throw new UnsupportedOperationException("StubCraftPlayer#getVirtualHost");
    }

    @Override
    public net.kyori.adventure.util.TriState getVisualFire() {
        throw new UnsupportedOperationException("StubCraftPlayer#getVisualFire");
    }

    @Override
    public float getWalkSpeed() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWalkSpeed");
    }

    @Override
    public int getWardenTimeSinceLastWarning() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWardenTimeSinceLastWarning");
    }

    @Override
    public int getWardenWarningCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWardenWarningCooldown");
    }

    @Override
    public int getWardenWarningLevel() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWardenWarningLevel");
    }

    @Override
    public org.bukkit.Color getWaypointColor() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWaypointColor");
    }

    @Override
    public net.kyori.adventure.key.Key getWaypointStyle() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWaypointStyle");
    }

    @Override
    public org.bukkit.inventory.ItemStack getWeapon() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWeapon");
    }

    @Override
    public double getWidth() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWidth");
    }

    @Override
    public org.bukkit.WorldBorder getWorldBorder() {
        throw new UnsupportedOperationException("StubCraftPlayer#getWorldBorder");
    }

    @Override
    public double getX() {
        throw new UnsupportedOperationException("StubCraftPlayer#getX");
    }

    @Override
    public double getY() {
        throw new UnsupportedOperationException("StubCraftPlayer#getY");
    }

    @Override
    public float getYaw() {
        throw new UnsupportedOperationException("StubCraftPlayer#getYaw");
    }

    @Override
    public double getZ() {
        throw new UnsupportedOperationException("StubCraftPlayer#getZ");
    }

    @Override
    public io.papermc.paper.entity.PlayerGiveResult give(java.util.Collection<org.bukkit.inventory.ItemStack> p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#give");
    }

    @Override
    public void giveExp(int p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#giveExp");
    }

    @Override
    public void giveExpLevels(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#giveExpLevels");
    }

    @Override
    public boolean hasAI() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasAI");
    }

    @Override
    public boolean hasActiveItem() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasActiveItem");
    }

    @Override
    public boolean hasCooldown(org.bukkit.Material p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasCooldown");
    }

    @Override
    public boolean hasCooldown(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasCooldown");
    }

    @Override
    public boolean hasData(io.papermc.paper.datacomponent.DataComponentType p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasData");
    }

    @Override
    public boolean hasDiscoveredRecipe(org.bukkit.NamespacedKey p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasDiscoveredRecipe");
    }

    @Override
    public boolean hasFixedPose() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasFixedPose");
    }

    @Override
    public net.kyori.adventure.util.TriState hasFlyingFallDamage() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasFlyingFallDamage");
    }

    @Override
    public boolean hasGravity() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasGravity");
    }

    @Override
    public boolean hasLineOfSight(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasLineOfSight");
    }

    @Override
    public boolean hasLineOfSight(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasLineOfSight");
    }

    @Override
    public boolean hasMetadata(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasMetadata");
    }

    @Override
    public boolean hasNoPhysics() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasNoPhysics");
    }

    @Override
    public boolean hasPermission(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasPermission");
    }

    @Override
    public boolean hasPermission(org.bukkit.permissions.Permission p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasPermission");
    }

    @Override
    public boolean hasPlayedBefore() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasPlayedBefore");
    }

    @Override
    public boolean hasPotionEffect(org.bukkit.potion.PotionEffectType p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hasPotionEffect");
    }

    @Override
    public boolean hasSeenWinScreen() {
        throw new UnsupportedOperationException("StubCraftPlayer#hasSeenWinScreen");
    }

    @Override
    public void heal(double p0, org.bukkit.event.entity.EntityRegainHealthEvent.RegainReason p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#heal");
    }

    @Override
    public void hideEntity(org.bukkit.plugin.Plugin p0, org.bukkit.entity.Entity p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#hideEntity");
    }

    @Override
    public void hidePlayer(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#hidePlayer");
    }

    @Override
    public void hideTitle() {
        throw new UnsupportedOperationException("StubCraftPlayer#hideTitle");
    }

    @Override
    public void increaseWardenWarningLevel() {
        throw new UnsupportedOperationException("StubCraftPlayer#increaseWardenWarningLevel");
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic p0) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#incrementStatistic");
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic p0, int p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#incrementStatistic");
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic p0, org.bukkit.Material p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#incrementStatistic");
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic p0, org.bukkit.Material p1, int p2) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#incrementStatistic");
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic p0, org.bukkit.entity.EntityType p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#incrementStatistic");
    }

    @Override
    public void incrementStatistic(org.bukkit.Statistic p0, org.bukkit.entity.EntityType p1, int p2) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#incrementStatistic");
    }

    @Override
    public boolean isAlliedTo(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isAlliedTo");
    }

    @Override
    public boolean isAllowingServerListings() {
        throw new UnsupportedOperationException("StubCraftPlayer#isAllowingServerListings");
    }

    @Override
    public boolean isBanned() {
        throw new UnsupportedOperationException("StubCraftPlayer#isBanned");
    }

    @Override
    public boolean isBlocking() {
        throw new UnsupportedOperationException("StubCraftPlayer#isBlocking");
    }

    @Override
    public boolean isChunkSent(long p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isChunkSent");
    }

    @Override
    public boolean isClimbing() {
        throw new UnsupportedOperationException("StubCraftPlayer#isClimbing");
    }

    @Override
    public boolean isCollidable() {
        throw new UnsupportedOperationException("StubCraftPlayer#isCollidable");
    }

    @Override
    public boolean isConnected() {
        throw new UnsupportedOperationException("StubCraftPlayer#isConnected");
    }

    @Override
    public boolean isConversing() {
        throw new UnsupportedOperationException("StubCraftPlayer#isConversing");
    }

    @Override
    public boolean isCustomNameVisible() {
        throw new UnsupportedOperationException("StubCraftPlayer#isCustomNameVisible");
    }

    @Override
    public boolean isDead() {
        throw new UnsupportedOperationException("StubCraftPlayer#isDead");
    }

    @Override
    public boolean isDeeplySleeping() {
        throw new UnsupportedOperationException("StubCraftPlayer#isDeeplySleeping");
    }

    @Override
    public boolean isEmpty() {
        throw new UnsupportedOperationException("StubCraftPlayer#isEmpty");
    }

    @Override
    public boolean isFlying() {
        throw new UnsupportedOperationException("StubCraftPlayer#isFlying");
    }

    @Override
    public boolean isFreezeTickingLocked() {
        throw new UnsupportedOperationException("StubCraftPlayer#isFreezeTickingLocked");
    }

    @Override
    public boolean isFrozen() {
        throw new UnsupportedOperationException("StubCraftPlayer#isFrozen");
    }

    @Override
    public boolean isGliding() {
        throw new UnsupportedOperationException("StubCraftPlayer#isGliding");
    }

    @Override
    public boolean isGlowing() {
        throw new UnsupportedOperationException("StubCraftPlayer#isGlowing");
    }

    @Override
    public boolean isHandRaised() {
        throw new UnsupportedOperationException("StubCraftPlayer#isHandRaised");
    }

    @Override
    public boolean isHealthScaled() {
        throw new UnsupportedOperationException("StubCraftPlayer#isHealthScaled");
    }

    @Override
    public boolean isInLava() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInLava");
    }

    @Override
    public boolean isInPowderedSnow() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInPowderedSnow");
    }

    @Override
    public boolean isInRain() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInRain");
    }

    @Override
    public boolean isInWater() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInWater");
    }

    @Override
    public boolean isInWorld() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInWorld");
    }

    @Override
    public boolean isInsideVehicle() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInsideVehicle");
    }

    @Override
    public boolean isInvisible() {
        throw new UnsupportedOperationException("StubCraftPlayer#isInvisible");
    }

    @Override
    public boolean isInvulnerableTo(org.bukkit.damage.DamageSource p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isInvulnerableTo");
    }

    @Override
    public boolean isJumping() {
        throw new UnsupportedOperationException("StubCraftPlayer#isJumping");
    }

    @Override
    public boolean isLeashed() {
        throw new UnsupportedOperationException("StubCraftPlayer#isLeashed");
    }

    @Override
    public boolean isListed(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isListed");
    }

    @Override
    public boolean isMarkedInvulnerable() {
        throw new UnsupportedOperationException("StubCraftPlayer#isMarkedInvulnerable");
    }

    @Override
    public boolean isOnGround() {
        throw new UnsupportedOperationException("StubCraftPlayer#isOnGround");
    }

    @Override
    public boolean isOnline() {
        throw new UnsupportedOperationException("StubCraftPlayer#isOnline");
    }

    @Override
    public boolean isOp() {
        throw new UnsupportedOperationException("StubCraftPlayer#isOp");
    }

    @Override
    public boolean isPermissionSet(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isPermissionSet");
    }

    @Override
    public boolean isPermissionSet(org.bukkit.permissions.Permission p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isPermissionSet");
    }

    @Override
    public boolean isPersistent() {
        throw new UnsupportedOperationException("StubCraftPlayer#isPersistent");
    }

    @Override
    public boolean isPlayerTimeRelative() {
        throw new UnsupportedOperationException("StubCraftPlayer#isPlayerTimeRelative");
    }

    @Override
    public boolean isRiptiding() {
        throw new UnsupportedOperationException("StubCraftPlayer#isRiptiding");
    }

    @Override
    public boolean isSilent() {
        throw new UnsupportedOperationException("StubCraftPlayer#isSilent");
    }

    @Override
    public boolean isSleeping() {
        throw new UnsupportedOperationException("StubCraftPlayer#isSleeping");
    }

    @Override
    public boolean isSleepingIgnored() {
        throw new UnsupportedOperationException("StubCraftPlayer#isSleepingIgnored");
    }

    @Override
    public boolean isSneaking() {
        throw new UnsupportedOperationException("StubCraftPlayer#isSneaking");
    }

    @Override
    public boolean isSprinting() {
        throw new UnsupportedOperationException("StubCraftPlayer#isSprinting");
    }

    @Override
    public boolean isSwimming() {
        throw new UnsupportedOperationException("StubCraftPlayer#isSwimming");
    }

    @Override
    public boolean isTicking() {
        throw new UnsupportedOperationException("StubCraftPlayer#isTicking");
    }

    @Override
    public boolean isTrackedBy(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#isTrackedBy");
    }

    @Override
    public boolean isTransferred() {
        throw new UnsupportedOperationException("StubCraftPlayer#isTransferred");
    }

    @Override
    public boolean isUnderWater() {
        throw new UnsupportedOperationException("StubCraftPlayer#isUnderWater");
    }

    @Override
    public boolean isValid() {
        throw new UnsupportedOperationException("StubCraftPlayer#isValid");
    }

    @Override
    public boolean isVisibleByDefault() {
        throw new UnsupportedOperationException("StubCraftPlayer#isVisibleByDefault");
    }

    @Override
    public boolean isVisualFire() {
        throw new UnsupportedOperationException("StubCraftPlayer#isVisualFire");
    }

    @Override
    public boolean isWhitelisted() {
        throw new UnsupportedOperationException("StubCraftPlayer#isWhitelisted");
    }

    @Override
    public void kick(net.kyori.adventure.text.Component p0, org.bukkit.event.player.PlayerKickEvent.Cause p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#kick");
    }

    @Override
    public void kickPlayer(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#kickPlayer");
    }

    @Override
    public void kill(org.bukkit.damage.DamageSource p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#kill");
    }

    @Override
    public void knockback(double p0, double p1, double p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#knockback");
    }

    @Override
    public <T extends org.bukkit.entity.Projectile> T launchProjectile(java.lang.Class<? extends T> p0, org.bukkit.util.Vector p1, java.util.function.Consumer<? super T> p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#launchProjectile");
    }

    @Override
    public boolean leaveVehicle() {
        throw new UnsupportedOperationException("StubCraftPlayer#leaveVehicle");
    }

    @Override
    public boolean listPlayer(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#listPlayer");
    }

    @Override
    public void loadData() {
        throw new UnsupportedOperationException("StubCraftPlayer#loadData");
    }

    @Override
    public java.util.Locale locale() {
        throw new UnsupportedOperationException("StubCraftPlayer#locale");
    }

    @Override
    public void lockFreezeTicks(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#lockFreezeTicks");
    }

    @Override
    public void lookAt(double p0, double p1, double p2, io.papermc.paper.entity.LookAnchor p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#lookAt");
    }

    @Override
    public void lookAt(org.bukkit.entity.Entity p0, io.papermc.paper.entity.LookAnchor p1, io.papermc.paper.entity.LookAnchor p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#lookAt");
    }

    @Override
    public net.kyori.adventure.text.Component name() {
        throw new UnsupportedOperationException("StubCraftPlayer#name");
    }

    @Override
    public org.bukkit.inventory.InventoryView openAnvil(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openAnvil");
    }

    @Override
    public void openBook(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#openBook");
    }

    @Override
    public org.bukkit.inventory.InventoryView openCartographyTable(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openCartographyTable");
    }

    @Override
    public org.bukkit.inventory.InventoryView openEnchanting(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openEnchanting");
    }

    @Override
    public org.bukkit.inventory.InventoryView openGrindstone(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openGrindstone");
    }

    @Override
    public org.bukkit.inventory.InventoryView openInventory(org.bukkit.inventory.Inventory p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#openInventory");
    }

    @Override
    public void openInventory(org.bukkit.inventory.InventoryView p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#openInventory");
    }

    @Override
    public org.bukkit.inventory.InventoryView openLoom(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openLoom");
    }

    @Override
    public org.bukkit.inventory.InventoryView openMerchant(org.bukkit.inventory.Merchant p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openMerchant");
    }

    @Override
    public void openSign(org.bukkit.block.Sign p0, org.bukkit.block.sign.Side p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openSign");
    }

    @Override
    public org.bukkit.inventory.InventoryView openSmithingTable(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openSmithingTable");
    }

    @Override
    public org.bukkit.inventory.InventoryView openStonecutter(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openStonecutter");
    }

    @Override
    public void openVirtualSign(io.papermc.paper.math.Position p0, org.bukkit.block.sign.Side p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openVirtualSign");
    }

    @Override
    public org.bukkit.inventory.InventoryView openWorkbench(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#openWorkbench");
    }

    @Override
    public boolean performCommand(java.lang.String p0) throws org.bukkit.command.CommandException {
        throw new UnsupportedOperationException("StubCraftPlayer#performCommand");
    }

    @Override
    public void playEffect(org.bukkit.EntityEffect p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#playEffect");
    }

    @Override
    public void playEffect(org.bukkit.Location p0, org.bukkit.Effect p1, int p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#playEffect");
    }

    @Override
    public <T> void playEffect(org.bukkit.Location p0, org.bukkit.Effect p1, T p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#playEffect");
    }

    @Override
    public void playHurtAnimation(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#playHurtAnimation");
    }

    @Override
    public void playNote(org.bukkit.Location p0, org.bukkit.Instrument p1, org.bukkit.Note p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#playNote");
    }

    @Override
    public void playPickupItemAnimation(org.bukkit.entity.Item p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#playPickupItemAnimation");
    }

    @Override
    public void playSound(org.bukkit.Location p0, java.lang.String p1, org.bukkit.SoundCategory p2, float p3, float p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.Location p0, java.lang.String p1, org.bukkit.SoundCategory p2, float p3, float p4, long p5) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.Location p0, org.bukkit.Sound p1, org.bukkit.SoundCategory p2, float p3, float p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.Location p0, org.bukkit.Sound p1, org.bukkit.SoundCategory p2, float p3, float p4, long p5) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.entity.Entity p0, java.lang.String p1, org.bukkit.SoundCategory p2, float p3, float p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.entity.Entity p0, java.lang.String p1, org.bukkit.SoundCategory p2, float p3, float p4, long p5) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.entity.Entity p0, org.bukkit.Sound p1, org.bukkit.SoundCategory p2, float p3, float p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public void playSound(org.bukkit.entity.Entity p0, org.bukkit.Sound p1, org.bukkit.SoundCategory p2, float p3, float p4, long p5) {
        throw new UnsupportedOperationException("StubCraftPlayer#playSound");
    }

    @Override
    public net.kyori.adventure.text.Component playerListFooter() {
        throw new UnsupportedOperationException("StubCraftPlayer#playerListFooter");
    }

    @Override
    public net.kyori.adventure.text.Component playerListHeader() {
        throw new UnsupportedOperationException("StubCraftPlayer#playerListHeader");
    }

    @Override
    public net.kyori.adventure.text.Component playerListName() {
        throw new UnsupportedOperationException("StubCraftPlayer#playerListName");
    }

    @Override
    public void playerListName(net.kyori.adventure.text.Component p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#playerListName");
    }

    @Override
    public io.papermc.paper.entity.PlayerPostEffects postEffects() {
        throw new UnsupportedOperationException("StubCraftPlayer#postEffects");
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceBlocks(double p0, org.bukkit.FluidCollisionMode p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#rayTraceBlocks");
    }

    @Override
    public org.bukkit.util.RayTraceResult rayTraceEntities(int p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#rayTraceEntities");
    }

    @Override
    public void recalculatePermissions() {
        throw new UnsupportedOperationException("StubCraftPlayer#recalculatePermissions");
    }

    @Override
    public void registerAttribute(org.bukkit.attribute.Attribute p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#registerAttribute");
    }

    @Override
    public org.bukkit.entity.Entity releaseLeftShoulderEntity() {
        throw new UnsupportedOperationException("StubCraftPlayer#releaseLeftShoulderEntity");
    }

    @Override
    public org.bukkit.entity.Entity releaseRightShoulderEntity() {
        throw new UnsupportedOperationException("StubCraftPlayer#releaseRightShoulderEntity");
    }

    @Override
    public void remove() {
        throw new UnsupportedOperationException("StubCraftPlayer#remove");
    }

    @Override
    public void removeAdditionalAlliedRule(net.kyori.adventure.key.Key p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeAdditionalAlliedRule");
    }

    @Override
    public void removeAdditionalAlliedRule(net.kyori.adventure.key.Namespaced p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeAdditionalAlliedRule");
    }

    @Override
    public void removeAdditionalChatCompletions(java.util.Collection<java.lang.String> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeAdditionalChatCompletions");
    }

    @Override
    public void removeAttachment(org.bukkit.permissions.PermissionAttachment p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeAttachment");
    }

    @Override
    public void removeCustomChatCompletions(java.util.Collection<java.lang.String> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeCustomChatCompletions");
    }

    @Override
    public void removeMetadata(java.lang.String p0, org.bukkit.plugin.Plugin p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeMetadata");
    }

    @Override
    public boolean removePassenger(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removePassenger");
    }

    @Override
    public void removePotionEffect(org.bukkit.potion.PotionEffectType p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removePotionEffect");
    }

    @Override
    public void removeResourcePack(java.util.UUID p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeResourcePack");
    }

    @Override
    public void removeResourcePacks() {
        throw new UnsupportedOperationException("StubCraftPlayer#removeResourcePacks");
    }

    @Override
    public boolean removeScoreboardTag(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#removeScoreboardTag");
    }

    @Override
    public void resetCooldown() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetCooldown");
    }

    @Override
    public void resetFlyingTicks() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetFlyingTicks");
    }

    @Override
    public void resetIdleDuration() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetIdleDuration");
    }

    @Override
    public void resetMaxHealth() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetMaxHealth");
    }

    @Override
    public void resetPlayerTime() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetPlayerTime");
    }

    @Override
    public void resetPlayerWeather() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetPlayerWeather");
    }

    @Override
    public void resetTitle() {
        throw new UnsupportedOperationException("StubCraftPlayer#resetTitle");
    }

    @Override
    public java.util.concurrent.CompletableFuture<byte[]> retrieveCookie(org.bukkit.NamespacedKey p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#retrieveCookie");
    }

    @Override
    public void saveData() {
        throw new UnsupportedOperationException("StubCraftPlayer#saveData");
    }

    @Override
    public void sendActionBar(net.md_5.bungee.api.chat.BaseComponent... p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendActionBar");
    }

    @Override
    public void sendActionBar(char p0, java.lang.String p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendActionBar");
    }

    @Override
    public void sendActionBar(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendActionBar");
    }

    @Override
    public void sendBlockChange(org.bukkit.Location p0, org.bukkit.Material p1, byte p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendBlockChange");
    }

    @Override
    public void sendBlockChange(org.bukkit.Location p0, org.bukkit.block.data.BlockData p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendBlockChange");
    }

    @Override
    public void sendBlockChanges(java.util.Collection<org.bukkit.block.BlockState> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendBlockChanges");
    }

    @Override
    public void sendBlockDamage(org.bukkit.Location p0, float p1, int p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendBlockDamage");
    }

    @Override
    public void sendBlockDamage(org.bukkit.Location p0, float p1, org.bukkit.entity.Entity p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendBlockDamage");
    }

    @Override
    public void sendBlockUpdate(org.bukkit.Location p0, org.bukkit.block.TileState p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#sendBlockUpdate");
    }

    @Override
    public void sendEntityEffect(org.bukkit.EntityEffect p0, org.bukkit.entity.Entity p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendEntityEffect");
    }

    @Override
    public void sendEquipmentChange(org.bukkit.entity.LivingEntity p0, java.util.Map<org.bukkit.inventory.EquipmentSlot, org.bukkit.inventory.ItemStack> p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendEquipmentChange");
    }

    @Override
    public void sendEquipmentChange(org.bukkit.entity.LivingEntity p0, org.bukkit.inventory.EquipmentSlot p1, org.bukkit.inventory.ItemStack p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendEquipmentChange");
    }

    @Override
    public void sendExperienceChange(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendExperienceChange");
    }

    @Override
    public void sendExperienceChange(float p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendExperienceChange");
    }

    @Override
    public void sendHealthUpdate() {
        throw new UnsupportedOperationException("StubCraftPlayer#sendHealthUpdate");
    }

    @Override
    public void sendHealthUpdate(double p0, int p1, float p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendHealthUpdate");
    }

    @Override
    public void sendHurtAnimation(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendHurtAnimation");
    }

    @Override
    public void sendLinks(org.bukkit.ServerLinks p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendLinks");
    }

    @Override
    public void sendMap(org.bukkit.map.MapView p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendMap");
    }

    @Override
    public void sendMessage(java.lang.String... p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendMessage");
    }

    @Override
    public void sendMessage(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendMessage");
    }

    @Override
    public void sendMessage(java.util.UUID p0, java.lang.String... p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendMessage");
    }

    @Override
    public void sendMessage(java.util.UUID p0, java.lang.String p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendMessage");
    }

    @Override
    public void sendMultiBlockChange(java.util.Map<? extends io.papermc.paper.math.Position, org.bukkit.block.data.BlockData> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendMultiBlockChange");
    }

    @Override
    public void sendOpLevel(byte p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendOpLevel");
    }

    @Override
    public void sendPluginMessage(org.bukkit.plugin.Plugin p0, java.lang.String p1, byte[] p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendPluginMessage");
    }

    @Override
    public void sendPotionEffectChange(org.bukkit.entity.LivingEntity p0, org.bukkit.potion.PotionEffect p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendPotionEffectChange");
    }

    @Override
    public void sendPotionEffectChangeRemove(org.bukkit.entity.LivingEntity p0, org.bukkit.potion.PotionEffectType p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendPotionEffectChangeRemove");
    }

    @Override
    public void sendRawMessage(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendRawMessage");
    }

    @Override
    public void sendRawMessage(java.util.UUID p0, java.lang.String p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendRawMessage");
    }

    @Override
    public void sendSignChange(org.bukkit.Location p0, java.lang.String[] p1, org.bukkit.DyeColor p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendSignChange");
    }

    @Override
    public void sendSignChange(org.bukkit.Location p0, java.util.List<? extends net.kyori.adventure.text.Component> p1, org.bukkit.DyeColor p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendSignChange");
    }

    @Override
    public void sendTitle(com.destroystokyo.paper.Title p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendTitle");
    }

    @Override
    public void sendTitle(java.lang.String p0, java.lang.String p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendTitle");
    }

    @Override
    public void sendTitle(java.lang.String p0, java.lang.String p1, int p2, int p3, int p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#sendTitle");
    }

    @Override
    public java.util.Map<java.lang.String, java.lang.Object> serialize() {
        throw new UnsupportedOperationException("StubCraftPlayer#serialize");
    }

    @Override
    public void setAI(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setAI");
    }

    @Override
    public void setAbsorptionAmount(double p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setAbsorptionAmount");
    }

    @Override
    public void setActiveItemRemainingTime(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setActiveItemRemainingTime");
    }

    @Override
    public void setAffectsSpawning(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setAffectsSpawning");
    }

    @Override
    public void setAllowFlight(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setAllowFlight");
    }

    @Override
    public void setArrowCooldown(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setArrowCooldown");
    }

    @Override
    public void setArrowsInBody(int p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setArrowsInBody");
    }

    @Override
    public void setBeeStingerCooldown(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setBeeStingerCooldown");
    }

    @Override
    public void setBeeStingersInBody(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setBeeStingersInBody");
    }

    @Override
    public void setBodyYaw(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setBodyYaw");
    }

    @Override
    public void setCanPickupItems(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCanPickupItems");
    }

    @Override
    public void setCollidable(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCollidable");
    }

    @Override
    public void setCompassTarget(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCompassTarget");
    }

    @Override
    public void setCooldown(net.kyori.adventure.key.Key p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCooldown");
    }

    @Override
    public void setCooldown(org.bukkit.inventory.ItemStack p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCooldown");
    }

    @Override
    public void setCustomChatCompletions(java.util.Collection<java.lang.String> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCustomChatCompletions");
    }

    @Override
    public void setCustomName(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCustomName");
    }

    @Override
    public void setCustomNameVisible(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setCustomNameVisible");
    }

    @Override
    public void setDeathScreenScore(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setDeathScreenScore");
    }

    @Override
    public void setDisplayName(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setDisplayName");
    }

    @Override
    public void setEnchantmentSeed(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setEnchantmentSeed");
    }

    @Override
    public void setExhaustion(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setExhaustion");
    }

    @Override
    public void setExp(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setExp");
    }

    @Override
    public void setExpCooldown(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setExpCooldown");
    }

    @Override
    public void setExperienceLevelAndProgress(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setExperienceLevelAndProgress");
    }

    @Override
    public void setFallDistance(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setFallDistance");
    }

    @Override
    public void setFireTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setFireTicks");
    }

    @Override
    public void setFlySpeed(float p0) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#setFlySpeed");
    }

    @Override
    public void setFlying(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setFlying");
    }

    @Override
    public void setFlyingFallDamage(net.kyori.adventure.util.TriState p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setFlyingFallDamage");
    }

    @Override
    public void setFreezeTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setFreezeTicks");
    }

    @Override
    public void setFrictionState(net.kyori.adventure.util.TriState p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setFrictionState");
    }

    @Override
    public void setGliding(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setGliding");
    }

    @Override
    public void setGlowing(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setGlowing");
    }

    @Override
    public void setGravity(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setGravity");
    }

    @Override
    public void setHasSeenWinScreen(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setHasSeenWinScreen");
    }

    @Override
    public void setHealthScale(double p0) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#setHealthScale");
    }

    @Override
    public void setHealthScaled(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setHealthScaled");
    }

    @Override
    public void setHurtDirection(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setHurtDirection");
    }

    @Override
    public void setInvisible(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setInvisible");
    }

    @Override
    public void setInvulnerable(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setInvulnerable");
    }

    @Override
    public void setItemInHand(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setItemInHand");
    }

    @Override
    public void setItemInUseTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setItemInUseTicks");
    }

    @Override
    public void setItemOnCursor(org.bukkit.inventory.ItemStack p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setItemOnCursor");
    }

    @Override
    public void setJumping(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setJumping");
    }

    @Override
    public void setKiller(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setKiller");
    }

    @Override
    public void setLastDamage(double p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setLastDamage");
    }

    @Override
    public void setLastDamageCause(org.bukkit.event.entity.EntityDamageEvent p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setLastDamageCause");
    }

    @Override
    public void setLastDeathLocation(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setLastDeathLocation");
    }

    @Override
    public boolean setLeashHolder(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setLeashHolder");
    }

    @Override
    public void setLevel(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setLevel");
    }

    @Override
    public void setMaxHealth(double p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setMaxHealth");
    }

    @Override
    public void setMaximumAir(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setMaximumAir");
    }

    @Override
    public void setMaximumNoDamageTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setMaximumNoDamageTicks");
    }

    @Override
    public <T> void setMemory(org.bukkit.entity.memory.MemoryKey<T> p0, T p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setMemory");
    }

    @Override
    public void setMetadata(java.lang.String p0, org.bukkit.metadata.MetadataValue p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setMetadata");
    }

    @Override
    public void setNoActionTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setNoActionTicks");
    }

    @Override
    public void setNoDamageTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setNoDamageTicks");
    }

    @Override
    public void setNoPhysics(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setNoPhysics");
    }

    @Override
    public void setOp(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setOp");
    }

    @Override
    public boolean setPassenger(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPassenger");
    }

    @Override
    public void setPersistent(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPersistent");
    }

    @Override
    public void setPlayerListFooter(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListFooter");
    }

    @Override
    public void setPlayerListHeader(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListHeader");
    }

    @Override
    public void setPlayerListHeaderFooter(net.md_5.bungee.api.chat.BaseComponent[] p0, net.md_5.bungee.api.chat.BaseComponent[] p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListHeaderFooter");
    }

    @Override
    public void setPlayerListHeaderFooter(java.lang.String p0, java.lang.String p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListHeaderFooter");
    }

    @Override
    public void setPlayerListHeaderFooter(net.md_5.bungee.api.chat.BaseComponent p0, net.md_5.bungee.api.chat.BaseComponent p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListHeaderFooter");
    }

    @Override
    public void setPlayerListName(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListName");
    }

    @Override
    public void setPlayerListOrder(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerListOrder");
    }

    @Override
    public void setPlayerProfile(com.destroystokyo.paper.profile.PlayerProfile p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerProfile");
    }

    @Override
    public void setPlayerTime(long p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerTime");
    }

    @Override
    public void setPlayerWeather(org.bukkit.WeatherType p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPlayerWeather");
    }

    @Override
    public void setPortalCooldown(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPortalCooldown");
    }

    @Override
    public void setPose(org.bukkit.entity.Pose p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setPose");
    }

    @Override
    public void setRemainingAir(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setRemainingAir");
    }

    @Override
    public void setRemoveWhenFarAway(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setRemoveWhenFarAway");
    }

    @Override
    public void setResourcePack(java.lang.String p0, byte[] p1, java.lang.String p2, boolean p3) {
        throw new UnsupportedOperationException("StubCraftPlayer#setResourcePack");
    }

    @Override
    public void setResourcePack(java.util.UUID p0, java.lang.String p1, byte[] p2, java.lang.String p3, boolean p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#setResourcePack");
    }

    @Override
    public void setResourcePack(java.util.UUID p0, java.lang.String p1, byte[] p2, net.kyori.adventure.text.Component p3, boolean p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#setResourcePack");
    }

    @Override
    public void setRespawnLocation(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setRespawnLocation");
    }

    @Override
    public void setRiptiding(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setRiptiding");
    }

    @Override
    public void setRotation(float p0, float p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setRotation");
    }

    @Override
    public void setRotation(io.papermc.paper.math.Angle p0, io.papermc.paper.math.Angle p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setRotation");
    }

    @Override
    public void setSaturatedRegenRate(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSaturatedRegenRate");
    }

    @Override
    public void setSaturation(float p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSaturation");
    }

    @Override
    public void setScoreboard(org.bukkit.scoreboard.Scoreboard p0) throws java.lang.IllegalArgumentException, java.lang.IllegalStateException {
        throw new UnsupportedOperationException("StubCraftPlayer#setScoreboard");
    }

    @Override
    public void setSendViewDistance(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSendViewDistance");
    }

    @Override
    public void setShoulderEntityLeft(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setShoulderEntityLeft");
    }

    @Override
    public void setShoulderEntityRight(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setShoulderEntityRight");
    }

    @Override
    public void setSilent(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSilent");
    }

    @Override
    public void setSimulationDistance(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSimulationDistance");
    }

    @Override
    public void setSleepingIgnored(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSleepingIgnored");
    }

    @Override
    public void setSneaking(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSneaking");
    }

    @Override
    public void setSpectatorTarget(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSpectatorTarget");
    }

    @Override
    public void setSprinting(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSprinting");
    }

    @Override
    public void setStarvationRate(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setStarvationRate");
    }

    @Override
    public void setStatistic(org.bukkit.Statistic p0, int p1) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#setStatistic");
    }

    @Override
    public void setStatistic(org.bukkit.Statistic p0, org.bukkit.Material p1, int p2) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#setStatistic");
    }

    @Override
    public void setStatistic(org.bukkit.Statistic p0, org.bukkit.entity.EntityType p1, int p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#setStatistic");
    }

    @Override
    public void setSubtitle(net.md_5.bungee.api.chat.BaseComponent[] p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSubtitle");
    }

    @Override
    public void setSubtitle(net.md_5.bungee.api.chat.BaseComponent p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSubtitle");
    }

    @Override
    public void setSwimming(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setSwimming");
    }

    @Override
    public void setTicksLived(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setTicksLived");
    }

    @Override
    public void setTitleTimes(int p0, int p1, int p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#setTitleTimes");
    }

    @Override
    public void setTotalExperience(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setTotalExperience");
    }

    @Override
    public void setUnsaturatedRegenRate(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setUnsaturatedRegenRate");
    }

    @Override
    public void setViewDistance(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setViewDistance");
    }

    @Override
    public void setVisibleByDefault(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setVisibleByDefault");
    }

    @Override
    public void setVisualFire(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setVisualFire");
    }

    @Override
    public void setVisualFire(net.kyori.adventure.util.TriState p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setVisualFire");
    }

    @Override
    public void setWalkSpeed(float p0) throws java.lang.IllegalArgumentException {
        throw new UnsupportedOperationException("StubCraftPlayer#setWalkSpeed");
    }

    @Override
    public void setWardenTimeSinceLastWarning(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWardenTimeSinceLastWarning");
    }

    @Override
    public void setWardenWarningCooldown(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWardenWarningCooldown");
    }

    @Override
    public void setWardenWarningLevel(int p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWardenWarningLevel");
    }

    @Override
    public void setWaypointColor(org.bukkit.Color p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWaypointColor");
    }

    @Override
    public void setWaypointStyle(net.kyori.adventure.key.Key p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWaypointStyle");
    }

    @Override
    public void setWhitelisted(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWhitelisted");
    }

    @Override
    public boolean setWindowProperty(org.bukkit.inventory.InventoryView.Property p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWindowProperty");
    }

    @Override
    public void setWorldBorder(org.bukkit.WorldBorder p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#setWorldBorder");
    }

    @Override
    public void showDemoScreen() {
        throw new UnsupportedOperationException("StubCraftPlayer#showDemoScreen");
    }

    @Override
    public void showElderGuardian(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#showElderGuardian");
    }

    @Override
    public void showEntity(org.bukkit.plugin.Plugin p0, org.bukkit.entity.Entity p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#showEntity");
    }

    @Override
    public void showPlayer(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#showPlayer");
    }

    @Override
    public void showTitle(net.md_5.bungee.api.chat.BaseComponent[] p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#showTitle");
    }

    @Override
    public void showTitle(net.md_5.bungee.api.chat.BaseComponent[] p0, net.md_5.bungee.api.chat.BaseComponent[] p1, int p2, int p3, int p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#showTitle");
    }

    @Override
    public void showTitle(net.md_5.bungee.api.chat.BaseComponent p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#showTitle");
    }

    @Override
    public void showTitle(net.md_5.bungee.api.chat.BaseComponent p0, net.md_5.bungee.api.chat.BaseComponent p1, int p2, int p3, int p4) {
        throw new UnsupportedOperationException("StubCraftPlayer#showTitle");
    }

    @Override
    public void showWinScreen() {
        throw new UnsupportedOperationException("StubCraftPlayer#showWinScreen");
    }

    @Override
    public boolean sleep(org.bukkit.Location p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#sleep");
    }

    @Override
    public net.kyori.adventure.sound.Sound.Source soundSource() {
        throw new UnsupportedOperationException("StubCraftPlayer#soundSource");
    }

    @Override
    public boolean spawnAt(org.bukkit.Location p0, org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#spawnAt");
    }

    @Override
    public <T> void spawnParticle(org.bukkit.Particle p0, double p1, double p2, double p3, int p4, double p5, double p6, double p7, double p8, double p9, double p10, T p11, boolean p12, org.bukkit.Particle.RandomizationType p13) {
        throw new UnsupportedOperationException("StubCraftPlayer#spawnParticle");
    }

    @Override
    public org.bukkit.entity.Player.Spigot spigot() {
        throw new UnsupportedOperationException("StubCraftPlayer#spigot");
    }

    @Override
    public void startRiptideAttack(int p0, float p1, org.bukkit.inventory.ItemStack p2) {
        throw new UnsupportedOperationException("StubCraftPlayer#startRiptideAttack");
    }

    @Override
    public void startUsingItem(org.bukkit.inventory.EquipmentSlot p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#startUsingItem");
    }

    @Override
    public void stopAllSounds() {
        throw new UnsupportedOperationException("StubCraftPlayer#stopAllSounds");
    }

    @Override
    public void stopSound(java.lang.String p0, org.bukkit.SoundCategory p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#stopSound");
    }

    @Override
    public void stopSound(org.bukkit.SoundCategory p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#stopSound");
    }

    @Override
    public void storeCookie(org.bukkit.NamespacedKey p0, byte[] p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#storeCookie");
    }

    @Override
    public void swingHand(org.bukkit.inventory.EquipmentSlot p0, io.papermc.paper.datacomponent.item.SwingAnimation p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#swingHand");
    }

    @Override
    public void swingMainHand() {
        throw new UnsupportedOperationException("StubCraftPlayer#swingMainHand");
    }

    @Override
    public void swingOffHand() {
        throw new UnsupportedOperationException("StubCraftPlayer#swingOffHand");
    }

    @Override
    public net.kyori.adventure.text.Component teamDisplayName() {
        throw new UnsupportedOperationException("StubCraftPlayer#teamDisplayName");
    }

    @Override
    public void transfer(java.lang.String p0, int p1) {
        throw new UnsupportedOperationException("StubCraftPlayer#transfer");
    }

    @Override
    public int undiscoverRecipes(java.util.Collection<org.bukkit.NamespacedKey> p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#undiscoverRecipes");
    }

    @Override
    public boolean unlistPlayer(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#unlistPlayer");
    }

    @Override
    public void unsetFixedPose() {
        throw new UnsupportedOperationException("StubCraftPlayer#unsetFixedPose");
    }

    @Override
    public void updateCommands() {
        throw new UnsupportedOperationException("StubCraftPlayer#updateCommands");
    }

    @Override
    public void updateInventory() {
        throw new UnsupportedOperationException("StubCraftPlayer#updateInventory");
    }

    @Override
    public void updateTitle(com.destroystokyo.paper.Title p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#updateTitle");
    }

    @Override
    public void wakeup(boolean p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#wakeup");
    }

    @Override
    public boolean wouldCollideUsing(org.bukkit.util.BoundingBox p0) {
        throw new UnsupportedOperationException("StubCraftPlayer#wouldCollideUsing");
    }
}
