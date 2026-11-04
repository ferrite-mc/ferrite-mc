package net.minecraft.world.entity.player;

public class Player extends net.minecraft.world.entity.LivingEntity {
    protected final org.bukkit.entity.Player player;
    public Player(org.bukkit.entity.Player player) { super(player); this.player = player; }
    public net.minecraft.network.chat.Component getDisplayName() { return net.minecraft.network.chat.Component.literal(player.getName()); }
    public String getScoreboardName() { return player.getName(); }
    public com.mojang.authlib.GameProfile getGameProfile() { return new com.mojang.authlib.GameProfile(player.getUniqueId(), player.getName()); }
    public int getFoodLevel() { return player.getFoodLevel(); }
    public boolean isCreative() { return player.getGameMode() == org.bukkit.GameMode.CREATIVE; }
    public boolean isSpectator() { return player.getGameMode() == org.bukkit.GameMode.SPECTATOR; }
    public boolean isLocalPlayer() { return false; }
    public net.minecraft.world.item.ItemStack getMainHandItem() { return net.minecraft.world.item.ItemStack.of(player.getInventory().getItemInMainHand()); }
    public net.minecraft.world.item.ItemStack getOffhandItem() { return net.minecraft.world.item.ItemStack.of(player.getInventory().getItemInOffHand()); }
}
