package net.minecraft.world.entity;

public class LivingEntity extends Entity {
    protected final org.bukkit.entity.LivingEntity living;
    public LivingEntity(org.bukkit.entity.LivingEntity living) { super(living); this.living = living; }
    public float getHealth() { return (float) living.getHealth(); }
    public void setHealth(float health) { living.setHealth(health); }
    public float getMaxHealth() { return (float) living.getMaxHealth(); }
    public boolean isDeadOrDying() { return living.isDead() || living.getHealth() <= 0.0; }
    public void kill() { living.setHealth(0.0); }
    public int getArmorValue() { return 0; }
    public float getYHeadRot() { return living.getLocation().getYaw(); }
    public float getAbsorptionAmount() { return (float) living.getAbsorptionAmount(); }
    public void setAbsorptionAmount(float amount) { living.setAbsorptionAmount(amount); }
    public boolean isSleeping() { return living.isSleeping(); }
    public boolean isFallFlying() { return living.isGliding(); }
    public boolean isUsingItem() { return living.isHandRaised(); }
    public int getArrowCount() { return living.getArrowsInBody(); }
    public boolean hasEffect(String key) { var type = org.bukkit.Registry.POTION_EFFECT_TYPE.get(org.bukkit.NamespacedKey.fromString(key)); return type != null && living.hasPotionEffect(type); }
    public boolean removeAllEffects() { boolean had = !living.getActivePotionEffects().isEmpty(); living.clearActivePotionEffects(); return had; }
}
