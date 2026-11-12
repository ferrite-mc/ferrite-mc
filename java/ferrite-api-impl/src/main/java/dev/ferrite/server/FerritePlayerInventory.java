package dev.ferrite.server;

final class FerritePlayerInventory implements java.lang.reflect.InvocationHandler {
    private final FerritePlayer player;
    private final org.bukkit.inventory.ItemStack[] items = new org.bukkit.inventory.ItemStack[41];
    final org.bukkit.inventory.PlayerInventory proxy;
    FerritePlayerInventory(FerritePlayer player) {
        this.player = player;
        proxy = (org.bukkit.inventory.PlayerInventory) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.PlayerInventory.class}, this);
    }
    void setSnapshot(int slot, String material, int amount) {
        var type = java.util.Objects.requireNonNull(org.bukkit.Material.matchMaterial(material), "unknown material");
        items[slot] = new org.bukkit.inventory.ItemStack(type, amount);
    }
    private org.bukkit.inventory.ItemStack read(int slot) { var item = items[slot]; return item == null ? null : item.clone(); }
    private void write(int slot, org.bukkit.inventory.ItemStack item) {
        int damage = 0;
        String meta = null;
        if (item != null && item.hasItemMeta()) {
            // The host carries wear, name, lore, enchantments, unbreakable, model, repair cost,
            // item flags and the persistent data container; anything else is refused so nothing
            // is silently dropped.
            var worn = item.getItemMeta();
            var fresh = org.bukkit.Bukkit.getItemFactory().getItemMeta(item.getType());
            if (worn instanceof org.bukkit.inventory.meta.Damageable w && fresh instanceof org.bukkit.inventory.meta.Damageable f) { damage = w.getDamage(); f.setDamage(damage); }
            var gson = net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson();
            String name = worn.hasDisplayName() ? gson.serialize(worn.displayName()) : "";
            if (worn.hasDisplayName()) fresh.displayName(worn.displayName());
            var lore = worn.hasLore() ? worn.lore() : null;
            if (lore != null) fresh.lore(lore);
            var enchants = new java.util.ArrayList<String>();
            for (var e : worn.getEnchants().entrySet()) { enchants.add(e.getKey().getKey() + "=" + e.getValue()); fresh.addEnchant(e.getKey(), e.getValue(), true); }
            fresh.setUnbreakable(worn.isUnbreakable());
            Integer model = worn.hasCustomModelData() ? worn.getCustomModelData() : null;
            if (model != null) fresh.setCustomModelData(model);
            int repair = worn instanceof org.bukkit.inventory.meta.Repairable r && r.hasRepairCost() ? r.getRepairCost() : 0;
            if (repair > 0 && fresh instanceof org.bukkit.inventory.meta.Repairable fr) fr.setRepairCost(repair);
            var flags = new java.util.TreeSet<String>();
            for (var flag : worn.getItemFlags()) flags.add(flag.name());
            if (!flags.isEmpty()) fresh.addItemFlags(worn.getItemFlags().toArray(org.bukkit.inventory.ItemFlag[]::new));
            var data = new FerriteDataContainer();
            worn.getPersistentDataContainer().copyTo(data, true);
            data.copyTo(fresh.getPersistentDataContainer(), true);
            if (!fresh.equals(worn)) throw new UnsupportedOperationException("Ferrite: inventory metadata beyond name, lore, enchantments, unbreakable, model data, repair cost, item flags and persistent data is not implemented");
            meta = String.join("\u001e", name,
                lore == null ? "" : String.join("\u001d", lore.stream().map(gson::serialize).toList()),
                String.join("\u001d", enchants), worn.isUnbreakable() ? "1" : "0", model == null ? "" : model.toString(), Integer.toString(repair),
                String.join(",", flags), data.toSnbt());
            if (name.isEmpty() && lore == null && enchants.isEmpty() && !worn.isUnbreakable() && model == null && repair == 0 && flags.isEmpty() && data.isEmpty()) meta = null;
        }
        String material = item == null ? "" : item.getType().getKey().toString();
        int amount = item == null ? 0 : item.getAmount();
        if (!Native.playerInventory(player.getUniqueId().toString(), slot, material, amount)) throw new IllegalStateException("No player command handler");
        if (damage > 0) HostBridge.callVoid("Player", player.getUniqueId().toString(), "setItemDamage(int,int)", slot, damage);
        if (meta != null) HostBridge.callVoid("Player", player.getUniqueId().toString(), "setItemMeta(int,String)", slot, meta);
        items[slot] = item == null ? null : item.clone();
    }
    /** An inventory index, or the index an equipment slot lives at (0 main hand, 36 boots .. 39 helmet, 40 off hand). */
    private static int slotOf(Object slot) {
        if (slot instanceof org.bukkit.inventory.EquipmentSlot equipment) {
            return switch (equipment) {
                case HAND -> 0;
                case OFF_HAND -> 40;
                case FEET -> 36;
                case LEGS -> 37;
                case CHEST -> 38;
                case HEAD -> 39;
                // Player#canUseSlot is false for exactly these two in vanilla: they exist for animals' armor and saddles.
                case BODY, SADDLE -> throw new UnsupportedOperationException("Ferrite: a player has no " + equipment + " equipment slot (vanilla Player#canUseSlot is false for BODY and SADDLE)");
            };
        }
        return (int) slot;
    }
    public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "getSize" -> items.length;
            case "getHolder" -> player;
            case "getType" -> org.bukkit.event.inventory.InventoryType.PLAYER;
            case "getItem" -> read(slotOf(args[0]));
            case "setItem" -> { write(slotOf(args[0]), (org.bukkit.inventory.ItemStack) args[1]); yield null; }
            case "getItemInMainHand" -> read(0);
            case "setItemInMainHand" -> { write(0, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "getItemInOffHand" -> read(40);
            case "setItemInOffHand" -> { write(40, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "getHelmet" -> read(39); case "getChestplate" -> read(38); case "getLeggings" -> read(37); case "getBoots" -> read(36);
            case "setHelmet" -> { write(39, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "setChestplate" -> { write(38, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "setLeggings" -> { write(37, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "setBoots" -> { write(36, (org.bukkit.inventory.ItemStack) args[0]); yield null; }
            case "getHeldItemSlot" -> 0;
            case "getContents" -> { var copy = new org.bukkit.inventory.ItemStack[items.length]; for (int i = 0; i < items.length; i++) copy[i] = read(i); yield copy; }
            case "getStorageContents" -> { var copy = new org.bukkit.inventory.ItemStack[36]; for (int i = 0; i < copy.length; i++) copy[i] = read(i); yield copy; }
            case "getViewers" -> java.util.List.of(player);
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            case "toString" -> "PlayerInventory[" + player.getUniqueId() + "]";
            default -> HostBridge.dispatch("PlayerInventory", "", method, args);
        };
    }
}
