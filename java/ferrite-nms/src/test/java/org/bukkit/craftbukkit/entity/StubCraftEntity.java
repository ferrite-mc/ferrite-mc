// Clean-room test stub for CraftEntity. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import java.util.Objects;
import java.util.UUID;

/** Concrete test subclass of {@link CraftEntity} for unit tests. */
@SuppressWarnings({"deprecation", "removal", "unchecked", "rawtypes"})
public final class StubCraftEntity extends CraftEntity {
    private final UUID uniqueId;

    public StubCraftEntity(UUID uniqueId) {
        this.uniqueId = Objects.requireNonNull(uniqueId, "uniqueId");
    }

    @Override
    public UUID getUniqueId() {
        return uniqueId;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof StubCraftEntity stub && uniqueId.equals(stub.uniqueId);
    }

    @Override
    public int hashCode() {
        return uniqueId.hashCode();
    }

    @Override
    public void addAdditionalAlliedRule(net.kyori.adventure.key.Key p0, java.util.function.Predicate<org.bukkit.entity.Entity> p1) {
        throw new UnsupportedOperationException("StubCraftEntity#addAdditionalAlliedRule");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0) {
        throw new UnsupportedOperationException("StubCraftEntity#addAttachment");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0, int p1) {
        throw new UnsupportedOperationException("StubCraftEntity#addAttachment");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0, java.lang.String p1, boolean p2) {
        throw new UnsupportedOperationException("StubCraftEntity#addAttachment");
    }

    @Override
    public org.bukkit.permissions.PermissionAttachment addAttachment(org.bukkit.plugin.Plugin p0, java.lang.String p1, boolean p2, int p3) {
        throw new UnsupportedOperationException("StubCraftEntity#addAttachment");
    }

    @Override
    public boolean addPassenger(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftEntity#addPassenger");
    }

    @Override
    public boolean addScoreboardTag(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#addScoreboardTag");
    }

    @Override
    public void broadcastHurtAnimation(java.util.Collection<org.bukkit.entity.Player> p0) {
        throw new UnsupportedOperationException("StubCraftEntity#broadcastHurtAnimation");
    }

    @Override
    public boolean collidesAt(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftEntity#collidesAt");
    }

    @Override
    public org.bukkit.entity.Entity copy() {
        throw new UnsupportedOperationException("StubCraftEntity#copy");
    }

    @Override
    public org.bukkit.entity.Entity copy(org.bukkit.Location p0) {
        throw new UnsupportedOperationException("StubCraftEntity#copy");
    }

    @Override
    public org.bukkit.entity.EntitySnapshot createSnapshot() {
        throw new UnsupportedOperationException("StubCraftEntity#createSnapshot");
    }

    @Override
    public net.kyori.adventure.text.Component customName() {
        throw new UnsupportedOperationException("StubCraftEntity#customName");
    }

    @Override
    public void customName(net.kyori.adventure.text.Component p0) {
        throw new UnsupportedOperationException("StubCraftEntity#customName");
    }

    @Override
    public boolean eject() {
        throw new UnsupportedOperationException("StubCraftEntity#eject");
    }

    @Override
    public boolean fromMobSpawner() {
        throw new UnsupportedOperationException("StubCraftEntity#fromMobSpawner");
    }

    @Override
    public java.util.Map<net.kyori.adventure.key.Key, java.util.function.Predicate<org.bukkit.entity.Entity>> getAdditionalAlliedRules() {
        throw new UnsupportedOperationException("StubCraftEntity#getAdditionalAlliedRules");
    }

    @Override
    public float getAirDrag() {
        throw new UnsupportedOperationException("StubCraftEntity#getAirDrag");
    }

    @Override
    public java.lang.String getAsString() {
        throw new UnsupportedOperationException("StubCraftEntity#getAsString");
    }

    @Override
    public org.bukkit.util.BoundingBox getBoundingBox() {
        throw new UnsupportedOperationException("StubCraftEntity#getBoundingBox");
    }

    @Override
    public java.lang.String getCustomName() {
        throw new UnsupportedOperationException("StubCraftEntity#getCustomName");
    }

    @Override
    public <T> T getData(io.papermc.paper.datacomponent.DataComponentType.Valued<T> p0) {
        throw new UnsupportedOperationException("StubCraftEntity#getData");
    }

    @Override
    public <T> T getDataOrDefault(io.papermc.paper.datacomponent.DataComponentType.Valued<? extends T> p0, T p1) {
        throw new UnsupportedOperationException("StubCraftEntity#getDataOrDefault");
    }

    @Override
    public double getDefaultGravity() {
        throw new UnsupportedOperationException("StubCraftEntity#getDefaultGravity");
    }

    @Override
    public java.util.Set<org.bukkit.permissions.PermissionAttachmentInfo> getEffectivePermissions() {
        throw new UnsupportedOperationException("StubCraftEntity#getEffectivePermissions");
    }

    @Override
    public int getEntityId() {
        throw new UnsupportedOperationException("StubCraftEntity#getEntityId");
    }

    @Override
    public org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason getEntitySpawnReason() {
        throw new UnsupportedOperationException("StubCraftEntity#getEntitySpawnReason");
    }

    @Override
    public org.bukkit.block.BlockFace getFacing() {
        throw new UnsupportedOperationException("StubCraftEntity#getFacing");
    }

    @Override
    public float getFallDistance() {
        throw new UnsupportedOperationException("StubCraftEntity#getFallDistance");
    }

    @Override
    public int getFireTicks() {
        throw new UnsupportedOperationException("StubCraftEntity#getFireTicks");
    }

    @Override
    public int getFreezeTicks() {
        throw new UnsupportedOperationException("StubCraftEntity#getFreezeTicks");
    }

    @Override
    public double getGravity() {
        throw new UnsupportedOperationException("StubCraftEntity#getGravity");
    }

    @Override
    public double getHeight() {
        throw new UnsupportedOperationException("StubCraftEntity#getHeight");
    }

    @Override
    public org.bukkit.event.entity.EntityDamageEvent getLastDamageCause() {
        throw new UnsupportedOperationException("StubCraftEntity#getLastDamageCause");
    }

    @Override
    public int getMaxFireTicks() {
        throw new UnsupportedOperationException("StubCraftEntity#getMaxFireTicks");
    }

    @Override
    public int getMaxFreezeTicks() {
        throw new UnsupportedOperationException("StubCraftEntity#getMaxFreezeTicks");
    }

    @Override
    public java.util.List<org.bukkit.metadata.MetadataValue> getMetadata(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#getMetadata");
    }

    @Override
    public java.lang.String getName() {
        throw new UnsupportedOperationException("StubCraftEntity#getName");
    }

    @Override
    public java.util.List<org.bukkit.entity.Entity> getNearbyEntities(double p0, double p1, double p2) {
        throw new UnsupportedOperationException("StubCraftEntity#getNearbyEntities");
    }

    @Override
    public org.bukkit.Location getOrigin() {
        throw new UnsupportedOperationException("StubCraftEntity#getOrigin");
    }

    @Override
    public org.bukkit.entity.Entity getPassenger() {
        throw new UnsupportedOperationException("StubCraftEntity#getPassenger");
    }

    @Override
    public org.bukkit.persistence.PersistentDataContainer getPersistentDataContainer() {
        throw new UnsupportedOperationException("StubCraftEntity#getPersistentDataContainer");
    }

    @Override
    public org.bukkit.inventory.ItemStack getPickItemStack() {
        throw new UnsupportedOperationException("StubCraftEntity#getPickItemStack");
    }

    @Override
    public org.bukkit.block.PistonMoveReaction getPistonMoveReaction() {
        throw new UnsupportedOperationException("StubCraftEntity#getPistonMoveReaction");
    }

    @Override
    public float getPitch() {
        throw new UnsupportedOperationException("StubCraftEntity#getPitch");
    }

    @Override
    public int getPortalCooldown() {
        throw new UnsupportedOperationException("StubCraftEntity#getPortalCooldown");
    }

    @Override
    public org.bukkit.entity.Pose getPose() {
        throw new UnsupportedOperationException("StubCraftEntity#getPose");
    }

    @Override
    public io.papermc.paper.entity.RemovalReason getRemovalReason() {
        throw new UnsupportedOperationException("StubCraftEntity#getRemovalReason");
    }

    @Override
    public org.bukkit.event.entity.EntityRemoveEvent.Cause getRemoveEventCause() {
        throw new UnsupportedOperationException("StubCraftEntity#getRemoveEventCause");
    }

    @Override
    public io.papermc.paper.threadedregions.scheduler.EntityScheduler getScheduler() {
        throw new UnsupportedOperationException("StubCraftEntity#getScheduler");
    }

    @Override
    public java.lang.String getScoreboardEntryName() {
        throw new UnsupportedOperationException("StubCraftEntity#getScoreboardEntryName");
    }

    @Override
    public java.util.Set<java.lang.String> getScoreboardTags() {
        throw new UnsupportedOperationException("StubCraftEntity#getScoreboardTags");
    }

    @Override
    public org.bukkit.Server getServer() {
        throw new UnsupportedOperationException("StubCraftEntity#getServer");
    }

    @Override
    public org.bukkit.SoundCategory getSoundCategory() {
        throw new UnsupportedOperationException("StubCraftEntity#getSoundCategory");
    }

    @Override
    public org.bukkit.entity.SpawnCategory getSpawnCategory() {
        throw new UnsupportedOperationException("StubCraftEntity#getSpawnCategory");
    }

    @Override
    public org.bukkit.Sound getSwimHighSpeedSplashSound() {
        throw new UnsupportedOperationException("StubCraftEntity#getSwimHighSpeedSplashSound");
    }

    @Override
    public org.bukkit.Sound getSwimSound() {
        throw new UnsupportedOperationException("StubCraftEntity#getSwimSound");
    }

    @Override
    public org.bukkit.Sound getSwimSplashSound() {
        throw new UnsupportedOperationException("StubCraftEntity#getSwimSplashSound");
    }

    @Override
    public int getTicksLived() {
        throw new UnsupportedOperationException("StubCraftEntity#getTicksLived");
    }

    @Override
    public java.util.Set<org.bukkit.entity.Player> getTrackedBy() {
        throw new UnsupportedOperationException("StubCraftEntity#getTrackedBy");
    }

    @Override
    public java.util.Set<org.bukkit.entity.Player> getTrackedPlayers() {
        throw new UnsupportedOperationException("StubCraftEntity#getTrackedPlayers");
    }

    @Override
    public net.kyori.adventure.util.TriState getVisualFire() {
        throw new UnsupportedOperationException("StubCraftEntity#getVisualFire");
    }

    @Override
    public org.bukkit.inventory.ItemStack getWeapon() {
        throw new UnsupportedOperationException("StubCraftEntity#getWeapon");
    }

    @Override
    public double getWidth() {
        throw new UnsupportedOperationException("StubCraftEntity#getWidth");
    }

    @Override
    public double getX() {
        throw new UnsupportedOperationException("StubCraftEntity#getX");
    }

    @Override
    public double getY() {
        throw new UnsupportedOperationException("StubCraftEntity#getY");
    }

    @Override
    public float getYaw() {
        throw new UnsupportedOperationException("StubCraftEntity#getYaw");
    }

    @Override
    public double getZ() {
        throw new UnsupportedOperationException("StubCraftEntity#getZ");
    }

    @Override
    public boolean hasData(io.papermc.paper.datacomponent.DataComponentType p0) {
        throw new UnsupportedOperationException("StubCraftEntity#hasData");
    }

    @Override
    public boolean hasFixedPose() {
        throw new UnsupportedOperationException("StubCraftEntity#hasFixedPose");
    }

    @Override
    public boolean hasGravity() {
        throw new UnsupportedOperationException("StubCraftEntity#hasGravity");
    }

    @Override
    public boolean hasMetadata(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#hasMetadata");
    }

    @Override
    public boolean hasNoPhysics() {
        throw new UnsupportedOperationException("StubCraftEntity#hasNoPhysics");
    }

    @Override
    public boolean hasPermission(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#hasPermission");
    }

    @Override
    public boolean hasPermission(org.bukkit.permissions.Permission p0) {
        throw new UnsupportedOperationException("StubCraftEntity#hasPermission");
    }

    @Override
    public boolean isAlliedTo(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftEntity#isAlliedTo");
    }

    @Override
    public boolean isCustomNameVisible() {
        throw new UnsupportedOperationException("StubCraftEntity#isCustomNameVisible");
    }

    @Override
    public boolean isDead() {
        throw new UnsupportedOperationException("StubCraftEntity#isDead");
    }

    @Override
    public boolean isEmpty() {
        throw new UnsupportedOperationException("StubCraftEntity#isEmpty");
    }

    @Override
    public boolean isFreezeTickingLocked() {
        throw new UnsupportedOperationException("StubCraftEntity#isFreezeTickingLocked");
    }

    @Override
    public boolean isFrozen() {
        throw new UnsupportedOperationException("StubCraftEntity#isFrozen");
    }

    @Override
    public boolean isGlowing() {
        throw new UnsupportedOperationException("StubCraftEntity#isGlowing");
    }

    @Override
    public boolean isInLava() {
        throw new UnsupportedOperationException("StubCraftEntity#isInLava");
    }

    @Override
    public boolean isInPowderedSnow() {
        throw new UnsupportedOperationException("StubCraftEntity#isInPowderedSnow");
    }

    @Override
    public boolean isInRain() {
        throw new UnsupportedOperationException("StubCraftEntity#isInRain");
    }

    @Override
    public boolean isInWater() {
        throw new UnsupportedOperationException("StubCraftEntity#isInWater");
    }

    @Override
    public boolean isInWorld() {
        throw new UnsupportedOperationException("StubCraftEntity#isInWorld");
    }

    @Override
    public boolean isInsideVehicle() {
        throw new UnsupportedOperationException("StubCraftEntity#isInsideVehicle");
    }

    @Override
    public boolean isInvisible() {
        throw new UnsupportedOperationException("StubCraftEntity#isInvisible");
    }

    @Override
    public boolean isInvulnerableTo(org.bukkit.damage.DamageSource p0) {
        throw new UnsupportedOperationException("StubCraftEntity#isInvulnerableTo");
    }

    @Override
    public boolean isMarkedInvulnerable() {
        throw new UnsupportedOperationException("StubCraftEntity#isMarkedInvulnerable");
    }

    @Override
    public boolean isOnGround() {
        throw new UnsupportedOperationException("StubCraftEntity#isOnGround");
    }

    @Override
    public boolean isOp() {
        throw new UnsupportedOperationException("StubCraftEntity#isOp");
    }

    @Override
    public boolean isPermissionSet(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#isPermissionSet");
    }

    @Override
    public boolean isPermissionSet(org.bukkit.permissions.Permission p0) {
        throw new UnsupportedOperationException("StubCraftEntity#isPermissionSet");
    }

    @Override
    public boolean isPersistent() {
        throw new UnsupportedOperationException("StubCraftEntity#isPersistent");
    }

    @Override
    public boolean isSilent() {
        throw new UnsupportedOperationException("StubCraftEntity#isSilent");
    }

    @Override
    public boolean isSneaking() {
        throw new UnsupportedOperationException("StubCraftEntity#isSneaking");
    }

    @Override
    public boolean isTicking() {
        throw new UnsupportedOperationException("StubCraftEntity#isTicking");
    }

    @Override
    public boolean isTrackedBy(org.bukkit.entity.Player p0) {
        throw new UnsupportedOperationException("StubCraftEntity#isTrackedBy");
    }

    @Override
    public boolean isUnderWater() {
        throw new UnsupportedOperationException("StubCraftEntity#isUnderWater");
    }

    @Override
    public boolean isValid() {
        throw new UnsupportedOperationException("StubCraftEntity#isValid");
    }

    @Override
    public boolean isVisibleByDefault() {
        throw new UnsupportedOperationException("StubCraftEntity#isVisibleByDefault");
    }

    @Override
    public boolean isVisualFire() {
        throw new UnsupportedOperationException("StubCraftEntity#isVisualFire");
    }

    @Override
    public boolean leaveVehicle() {
        throw new UnsupportedOperationException("StubCraftEntity#leaveVehicle");
    }

    @Override
    public void lockFreezeTicks(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#lockFreezeTicks");
    }

    @Override
    public void lookAt(double p0, double p1, double p2, io.papermc.paper.entity.LookAnchor p3) {
        throw new UnsupportedOperationException("StubCraftEntity#lookAt");
    }

    @Override
    public net.kyori.adventure.text.Component name() {
        throw new UnsupportedOperationException("StubCraftEntity#name");
    }

    @Override
    public void playEffect(org.bukkit.EntityEffect p0) {
        throw new UnsupportedOperationException("StubCraftEntity#playEffect");
    }

    @Override
    public void recalculatePermissions() {
        throw new UnsupportedOperationException("StubCraftEntity#recalculatePermissions");
    }

    @Override
    public void remove() {
        throw new UnsupportedOperationException("StubCraftEntity#remove");
    }

    @Override
    public void removeAdditionalAlliedRule(net.kyori.adventure.key.Key p0) {
        throw new UnsupportedOperationException("StubCraftEntity#removeAdditionalAlliedRule");
    }

    @Override
    public void removeAdditionalAlliedRule(net.kyori.adventure.key.Namespaced p0) {
        throw new UnsupportedOperationException("StubCraftEntity#removeAdditionalAlliedRule");
    }

    @Override
    public void removeAttachment(org.bukkit.permissions.PermissionAttachment p0) {
        throw new UnsupportedOperationException("StubCraftEntity#removeAttachment");
    }

    @Override
    public void removeMetadata(java.lang.String p0, org.bukkit.plugin.Plugin p1) {
        throw new UnsupportedOperationException("StubCraftEntity#removeMetadata");
    }

    @Override
    public boolean removePassenger(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftEntity#removePassenger");
    }

    @Override
    public boolean removeScoreboardTag(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#removeScoreboardTag");
    }

    @Override
    public void sendMessage(java.lang.String... p0) {
        throw new UnsupportedOperationException("StubCraftEntity#sendMessage");
    }

    @Override
    public void sendMessage(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#sendMessage");
    }

    @Override
    public void sendMessage(java.util.UUID p0, java.lang.String... p1) {
        throw new UnsupportedOperationException("StubCraftEntity#sendMessage");
    }

    @Override
    public void sendMessage(java.util.UUID p0, java.lang.String p1) {
        throw new UnsupportedOperationException("StubCraftEntity#sendMessage");
    }

    @Override
    public void setCustomName(java.lang.String p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setCustomName");
    }

    @Override
    public void setCustomNameVisible(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setCustomNameVisible");
    }

    @Override
    public void setFallDistance(float p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setFallDistance");
    }

    @Override
    public void setFireTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setFireTicks");
    }

    @Override
    public void setFreezeTicks(int p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setFreezeTicks");
    }

    @Override
    public void setGlowing(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setGlowing");
    }

    @Override
    public void setGravity(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setGravity");
    }

    @Override
    public void setInvisible(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setInvisible");
    }

    @Override
    public void setInvulnerable(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setInvulnerable");
    }

    @Override
    public void setLastDamageCause(org.bukkit.event.entity.EntityDamageEvent p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setLastDamageCause");
    }

    @Override
    public void setMetadata(java.lang.String p0, org.bukkit.metadata.MetadataValue p1) {
        throw new UnsupportedOperationException("StubCraftEntity#setMetadata");
    }

    @Override
    public void setNoPhysics(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setNoPhysics");
    }

    @Override
    public void setOp(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setOp");
    }

    @Override
    public boolean setPassenger(org.bukkit.entity.Entity p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setPassenger");
    }

    @Override
    public void setPersistent(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setPersistent");
    }

    @Override
    public void setPortalCooldown(int p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setPortalCooldown");
    }

    @Override
    public void setPose(org.bukkit.entity.Pose p0, boolean p1) {
        throw new UnsupportedOperationException("StubCraftEntity#setPose");
    }

    @Override
    public void setRotation(float p0, float p1) {
        throw new UnsupportedOperationException("StubCraftEntity#setRotation");
    }

    @Override
    public void setRotation(io.papermc.paper.math.Angle p0, io.papermc.paper.math.Angle p1) {
        throw new UnsupportedOperationException("StubCraftEntity#setRotation");
    }

    @Override
    public void setSilent(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setSilent");
    }

    @Override
    public void setSneaking(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setSneaking");
    }

    @Override
    public void setTicksLived(int p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setTicksLived");
    }

    @Override
    public void setVisibleByDefault(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setVisibleByDefault");
    }

    @Override
    public void setVisualFire(boolean p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setVisualFire");
    }

    @Override
    public void setVisualFire(net.kyori.adventure.util.TriState p0) {
        throw new UnsupportedOperationException("StubCraftEntity#setVisualFire");
    }

    @Override
    public net.kyori.adventure.sound.Sound.Source soundSource() {
        throw new UnsupportedOperationException("StubCraftEntity#soundSource");
    }

    @Override
    public boolean spawnAt(org.bukkit.Location p0, org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason p1) {
        throw new UnsupportedOperationException("StubCraftEntity#spawnAt");
    }

    @Override
    public org.bukkit.entity.Entity.Spigot spigot() {
        throw new UnsupportedOperationException("StubCraftEntity#spigot");
    }

    @Override
    public net.kyori.adventure.text.Component teamDisplayName() {
        throw new UnsupportedOperationException("StubCraftEntity#teamDisplayName");
    }

    @Override
    public boolean wouldCollideUsing(org.bukkit.util.BoundingBox p0) {
        throw new UnsupportedOperationException("StubCraftEntity#wouldCollideUsing");
    }
}
