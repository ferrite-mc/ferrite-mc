package dev.ferrite.api.entity;

import dev.ferrite.api.internal.Unsupported;
import java.util.Objects;
import org.bukkit.entity.Entity;
import org.bukkit.inventory.EntityEquipment;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;

/**
 * Six-slot {@link EntityEquipment} facade for {@link FerritePlayer}, backed by
 * {@link FerritePlayer#getInventory()} so reads and writes route through the player's
 * owning region and host inventory sync.
 */
public final class FerritePlayerEquipment implements EntityEquipment {

    private final FerritePlayer player;

    public FerritePlayerEquipment(FerritePlayer player) {
        this.player = Objects.requireNonNull(player, "player");
    }

    @Override
    public void setItem(EquipmentSlot slot, ItemStack item) {
        setItem(slot, item, false);
    }

    @Override
    public void setItem(EquipmentSlot slot, ItemStack item, boolean silent) {
        Objects.requireNonNull(slot, "slot");
        var inv = player.getInventory();
        switch (slot) {
            case HAND -> inv.setItemInMainHand(item);
            case OFF_HAND -> inv.setItemInOffHand(item);
            case HEAD -> inv.setHelmet(item);
            case CHEST -> inv.setChestplate(item);
            case LEGS -> inv.setLeggings(item);
            case FEET -> inv.setBoots(item);
            // Only the six vanilla equipment slots are mapped to the player inventory.
            default -> throw Unsupported.exception("EntityEquipment#setItem(" + slot + ")");
        }
    }

    @Override
    public ItemStack getItem(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        var inv = player.getInventory();
        return switch (slot) {
            case HAND -> inv.getItemInMainHand();
            case OFF_HAND -> inv.getItemInOffHand();
            case HEAD -> inv.getHelmet();
            case CHEST -> inv.getChestplate();
            case LEGS -> inv.getLeggings();
            case FEET -> inv.getBoots();
            // Only the six vanilla equipment slots are mapped to the player inventory.
            default -> throw Unsupported.exception("EntityEquipment#getItem(" + slot + ")");
        };
    }

    @Override
    public ItemStack getItemInMainHand() {
        ItemStack item = player.getInventory().getItemInMainHand();
        return item != null ? item : new ItemStack(org.bukkit.Material.AIR);
    }

    @Override
    public void setItemInMainHand(ItemStack item) {
        player.getInventory().setItemInMainHand(item);
    }

    @Override
    public void setItemInMainHand(ItemStack item, boolean silent) {
        setItemInMainHand(item);
    }

    @Override
    public ItemStack getItemInOffHand() {
        ItemStack item = player.getInventory().getItemInOffHand();
        return item != null ? item : new ItemStack(org.bukkit.Material.AIR);
    }

    @Override
    public void setItemInOffHand(ItemStack item) {
        player.getInventory().setItemInOffHand(item);
    }

    @Override
    public void setItemInOffHand(ItemStack item, boolean silent) {
        setItemInOffHand(item);
    }

    @Override
    @Deprecated
    public ItemStack getItemInHand() {
        return getItemInMainHand();
    }

    @Override
    @Deprecated
    public void setItemInHand(ItemStack stack) {
        setItemInMainHand(stack);
    }

    @Override
    public ItemStack getHelmet() {
        return player.getInventory().getHelmet();
    }

    @Override
    public void setHelmet(ItemStack helmet) {
        player.getInventory().setHelmet(helmet);
    }

    @Override
    public void setHelmet(ItemStack helmet, boolean silent) {
        setHelmet(helmet);
    }

    @Override
    public ItemStack getChestplate() {
        return player.getInventory().getChestplate();
    }

    @Override
    public void setChestplate(ItemStack chestplate) {
        player.getInventory().setChestplate(chestplate);
    }

    @Override
    public void setChestplate(ItemStack chestplate, boolean silent) {
        setChestplate(chestplate);
    }

    @Override
    public ItemStack getLeggings() {
        return player.getInventory().getLeggings();
    }

    @Override
    public void setLeggings(ItemStack leggings) {
        player.getInventory().setLeggings(leggings);
    }

    @Override
    public void setLeggings(ItemStack leggings, boolean silent) {
        setLeggings(leggings);
    }

    @Override
    public ItemStack getBoots() {
        return player.getInventory().getBoots();
    }

    @Override
    public void setBoots(ItemStack boots) {
        player.getInventory().setBoots(boots);
    }

    @Override
    public void setBoots(ItemStack boots, boolean silent) {
        setBoots(boots);
    }

    @Override
    public ItemStack[] getArmorContents() {
        return player.getInventory().getArmorContents();
    }

    @Override
    public void setArmorContents(ItemStack[] items) {
        player.getInventory().setArmorContents(items);
    }

    @Override
    public void clear() {
        setItemInMainHand(null);
        setItemInOffHand(null);
        setHelmet(null);
        setChestplate(null);
        setLeggings(null);
        setBoots(null);
    }

    // Drop chances are mob loot state; a player's equipment never drops on death.
    @Override
    @Deprecated
    public float getItemInHandDropChance() {
        throw Unsupported.exception("EntityEquipment#getItemInHandDropChance");
    }

    @Override
    @Deprecated
    public void setItemInHandDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setItemInHandDropChance");
    }

    @Override
    public float getItemInMainHandDropChance() {
        throw Unsupported.exception("EntityEquipment#getItemInMainHandDropChance");
    }

    @Override
    public void setItemInMainHandDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setItemInMainHandDropChance");
    }

    @Override
    public float getItemInOffHandDropChance() {
        throw Unsupported.exception("EntityEquipment#getItemInOffHandDropChance");
    }

    @Override
    public void setItemInOffHandDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setItemInOffHandDropChance");
    }

    @Override
    public float getHelmetDropChance() {
        throw Unsupported.exception("EntityEquipment#getHelmetDropChance");
    }

    @Override
    public void setHelmetDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setHelmetDropChance");
    }

    @Override
    public float getChestplateDropChance() {
        throw Unsupported.exception("EntityEquipment#getChestplateDropChance");
    }

    @Override
    public void setChestplateDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setChestplateDropChance");
    }

    @Override
    public float getLeggingsDropChance() {
        throw Unsupported.exception("EntityEquipment#getLeggingsDropChance");
    }

    @Override
    public void setLeggingsDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setLeggingsDropChance");
    }

    @Override
    public float getBootsDropChance() {
        throw Unsupported.exception("EntityEquipment#getBootsDropChance");
    }

    @Override
    public void setBootsDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setBootsDropChance");
    }

    @Override
    public Entity getHolder() {
        return player;
    }

    @Override
    public float getDropChance(EquipmentSlot slot) {
        // Drop chances are mob loot state; a player's equipment never drops on death.
        throw Unsupported.exception("EntityEquipment#getDropChance");
    }

    @Override
    public void setDropChance(EquipmentSlot slot, float chance) {
        throw Unsupported.exception("EntityEquipment#setDropChance");
    }
}
