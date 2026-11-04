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
    public boolean removeAllEffects() { boolean had = !living.getActivePotionEffects().isEmpty(); living.clearActivePotionEffects(); return had; }
}
