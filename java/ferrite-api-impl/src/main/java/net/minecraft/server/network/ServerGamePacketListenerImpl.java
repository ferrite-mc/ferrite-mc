package net.minecraft.server.network;

/**
 * Player connection. Only packets the host can express are sent; any other packet fails loudly
 * with its class name instead of being dropped, so a plugin never believes a packet arrived.
 */
public class ServerGamePacketListenerImpl {
    public net.minecraft.server.level.ServerPlayer player;
    private net.kyori.adventure.text.Component pendingSubtitle;
    private net.minecraft.network.protocol.game.ClientboundSetTitlesAnimationPacket pendingTimes;
    /** NMS container ids mapped to the Bukkit view this connection opened for them. */
    private final java.util.Map<Integer, org.bukkit.inventory.InventoryView> openContainers = new java.util.LinkedHashMap<>();
    /** The last state id applied for each open container; an older packet is stale and ignored. */
    private final java.util.Map<Integer, Integer> containerStates = new java.util.HashMap<>();
    private static final int PLAYER_MENU_SLOTS = 36;
    /** Menus the host opens as block-less stations. */
    private static final java.util.Set<String> STATIONS = java.util.Set.of("CRAFTING", "ANVIL", "ENCHANTMENT", "GRINDSTONE", "LOOM", "SMITHING", "STONECUTTER", "CARTOGRAPHY_TABLE");
    private final java.util.Set<Integer> stationContainers = new java.util.HashSet<>();
    public ServerGamePacketListenerImpl(net.minecraft.server.level.ServerPlayer player) { this.player = player; }
    public net.minecraft.server.level.ServerPlayer getPlayer() { return player; }
    public void send(net.minecraft.network.protocol.Packet<?> packet) {
        org.bukkit.entity.Player target = player.getBukkitEntity();
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSystemChatPacket chat) {
            if (chat.overlay()) target.sendActionBar(chat.content().adventure()); else target.sendMessage(chat.content().adventure());
            return;
        }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetActionBarTextPacket bar) { target.sendActionBar(bar.text().adventure()); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetSubtitleTextPacket subtitle) { pendingSubtitle = subtitle.text().adventure(); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetTitlesAnimationPacket times) { pendingTimes = times; return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetTitleTextPacket title) {
            var timing = pendingTimes == null ? net.kyori.adventure.title.Title.DEFAULT_TIMES
                    : net.kyori.adventure.title.Title.Times.times(java.time.Duration.ofMillis(pendingTimes.fadeIn() * 50L), java.time.Duration.ofMillis(pendingTimes.stay() * 50L), java.time.Duration.ofMillis(pendingTimes.fadeOut() * 50L));
            target.showTitle(net.kyori.adventure.title.Title.title(title.text().adventure(), pendingSubtitle == null ? net.kyori.adventure.text.Component.empty() : pendingSubtitle, timing));
            pendingSubtitle = null;
            return;
        }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundClearTitlesPacket clear) {
            if (clear.resetTimes()) { target.resetTitle(); pendingTimes = null; } else target.clearTitle();
            pendingSubtitle = null;
            return;
        }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundTabListPacket tab) { target.sendPlayerListHeaderAndFooter(tab.header().adventure(), tab.footer().adventure()); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundContainerClosePacket close) { closeContainer(target, close.getContainerId()); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundOpenScreenPacket open) { openContainer(target, open); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket content) { setContent(target, content); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket slot) { setSlot(target, slot); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetExperiencePacket exp) { target.sendExperienceChange(exp.experienceProgress(), exp.experienceLevel()); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetHealthPacket health) { target.sendHealthUpdate(health.getHealth(), health.getFood(), health.getSaturation()); return; }
        dev.ferrite.server.HostBridge.callVoid("NMS", target.getUniqueId().toString(), "send(" + packet.getClass().getSimpleName() + ")", packet.toString());
    }
    private void openContainer(org.bukkit.entity.Player target, net.minecraft.network.protocol.game.ClientboundOpenScreenPacket packet) {
        int id = packet.getContainerId();
        if (id <= 0) throw new IllegalArgumentException("Ferrite: invalid NMS container id " + id);
        int rows = packet.getType().getChestRows();
        String name = packet.getType().getName();
        if (rows == 0 && name.equals("SHULKER_BOX")) {
            var shulker = org.bukkit.Bukkit.createInventory(null, org.bukkit.event.inventory.InventoryType.SHULKER_BOX, packet.getTitle().getString());
            if (!openContainers.isEmpty() || !stationContainers.isEmpty()) { target.closeInventory(); openContainers.clear(); stationContainers.clear(); containerStates.clear(); }
            var view = target.openInventory(shulker);
            if (view == null) throw new IllegalStateException("Ferrite: the player did not accept NMS container " + id);
            openContainers.put(id, view);
            containerStates.put(id, 0);
            return;
        }
        if (rows == 0) {
            // Crafting-style screens have no contents the server owns: the client drives them, so the host opens the station itself
            // and later content packets for it are refused instead of silently dropped.
            if (!STATIONS.contains(name)) throw new UnsupportedOperationException("Ferrite: not implemented: NMS open screen menu " + name
                + " (it needs a block entity or entity the packet does not carry; open it with the Player#open* method for that block instead)");
            if (!openContainers.isEmpty() || !stationContainers.isEmpty()) { target.closeInventory(); openContainers.clear(); stationContainers.clear(); containerStates.clear(); }
            var at = target.getLocation();
            switch (name) {
                case "CRAFTING" -> target.openWorkbench(at, true);
                case "ANVIL" -> target.openAnvil(at, true);
                case "ENCHANTMENT" -> target.openEnchanting(at, true);
                case "GRINDSTONE" -> target.openGrindstone(at, true);
                case "LOOM" -> target.openLoom(at, true);
                case "SMITHING" -> target.openSmithingTable(at, true);
                case "STONECUTTER" -> target.openStonecutter(at, true);
                default -> target.openCartographyTable(at, true);
            }
            stationContainers.add(id);
            containerStates.put(id, 0);
            return;
        }
        var inventory = org.bukkit.Bukkit.createInventory(null, rows * 9, packet.getTitle().getString());
        if (!openContainers.isEmpty()) { target.closeInventory(); openContainers.clear(); containerStates.clear(); }
        var view = target.openInventory(inventory);
        if (view == null) throw new IllegalStateException("Ferrite: the player did not accept NMS container " + id);
        openContainers.put(id, view);
        containerStates.put(id, 0);
    }

    private void setContent(org.bukkit.entity.Player target, net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket packet) {
        var view = container(target, packet.containerId());
        if (stale(packet.containerId(), packet.stateId())) return;
        int slots = menuSlots(view);
        if (packet.items().size() != slots) throw new IllegalArgumentException("Ferrite: NMS container " + packet.containerId() + " has " + slots + " slots, packet has " + packet.items().size());
        for (int i = 0; i < slots; i++) view.setItem(i, bukkitStack(packet.items().get(i)));
        view.setCursor(bukkitStack(packet.carriedItem()));
        target.updateInventory();
    }

    private void setSlot(org.bukkit.entity.Player target, net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket packet) {
        var view = container(target, packet.getContainerId());
        if (stale(packet.getContainerId(), packet.getStateId())) return;
        int slot = packet.getSlot();
        if (slot == -1) view.setCursor(bukkitStack(packet.getItem()));
        else {
            int slots = menuSlots(view);
            if (slot < 0 || slot >= slots) throw new IllegalArgumentException("Ferrite: NMS container " + packet.getContainerId() + " has no slot " + slot);
            view.setItem(slot, bukkitStack(packet.getItem()));
        }
        target.updateInventory();
    }

    /** Whether a packet carries a state id older than one already applied for this container. */
    private boolean stale(int containerId, int stateId) {
        if (stateId < containerStates.getOrDefault(containerId, 0)) return true;
        containerStates.put(containerId, stateId);
        return false;
    }

    private void closeContainer(org.bukkit.entity.Player target, int containerId) {
        containerStates.remove(containerId);
        if (stationContainers.remove(containerId)) { target.closeInventory(); return; }
        var view = container(target, containerId);
        openContainers.remove(containerId);
        target.closeInventory();
    }

    private org.bukkit.inventory.InventoryView container(org.bukkit.entity.Player target, int containerId) {
        if (stationContainers.contains(containerId)) throw new UnsupportedOperationException("Ferrite: NMS container " + containerId + " is a crafting-style screen the client drives; its contents cannot be set from a packet");
        var view = openContainers.get(containerId);
        if (view == null) throw new IllegalStateException("Ferrite: NMS container " + containerId + " is not open");
        if (target.getOpenInventory() != view) {
            openContainers.remove(containerId);
            throw new IllegalStateException("Ferrite: NMS container " + containerId + " is no longer the player's open view");
        }
        return view;
    }

    private static int menuSlots(org.bukkit.inventory.InventoryView view) {
        return view.getTopInventory().getSize() + PLAYER_MENU_SLOTS;
    }

    private static org.bukkit.inventory.ItemStack bukkitStack(net.minecraft.world.item.ItemStack stack) {
        return stack == null || stack.isEmpty() ? null : stack.toBukkit();
    }

    public void disconnect(net.minecraft.network.chat.Component reason) { player.getBukkitEntity().kick(reason.adventure()); }
    public void disconnect(String reason) { player.getBukkitEntity().kickPlayer(reason); }
}
