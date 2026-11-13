package dev.ferrite.server;

/**
 * The wire form of an item's metadata that the host reads ({@code setItemMeta(int,String)} and the metadata of a tile
 * snapshot): wear and a map id travel separately, everything else is one string of fields joined by U+001E.
 * Metadata the host cannot carry is refused with {@link UnsupportedOperationException}, never dropped.
 */
record ItemMetaWire(int damage, Integer mapId, String meta) {
    static ItemMetaWire of(org.bukkit.inventory.ItemStack item) {
        int damage = 0;
    Integer mapId = null;
    String meta = null;
    if (item != null && item.hasItemMeta()) {
        // The host carries wear, name, lore, enchantments, unbreakable, model, repair cost,
        // item flags and the persistent data container; anything else is refused so nothing
        // is silently dropped.
        var worn = item.getItemMeta();
        var fresh = org.bukkit.Bukkit.getItemFactory().getItemMeta(item.getType());
        if (worn instanceof org.bukkit.inventory.meta.Damageable w && fresh instanceof org.bukkit.inventory.meta.Damageable f) { damage = w.getDamage(); f.setDamage(damage); }
        // A filled map's identity travels as its own host call; the view's settings are
        // already saved in the host's map store (see FerriteMapView).
        if (worn instanceof org.bukkit.inventory.meta.MapMeta wm && wm.hasMapView() && fresh instanceof org.bukkit.inventory.meta.MapMeta fm) {
            mapId = wm.getMapView().getId();
            fm.setMapView(wm.getMapView());
        }
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
        // A book's title, author, generation and pages travel as one JSON field.
        String book = null;
        if (worn instanceof org.bukkit.inventory.meta.BookMeta wb && fresh instanceof org.bukkit.inventory.meta.BookMeta fb) {
            boolean written = item.getType() == org.bukkit.Material.WRITTEN_BOOK;
            var legacy = net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection();
            var json = new com.google.gson.JsonObject();
            json.addProperty("k", written ? "B" : "W");
            if (written && wb.hasTitle()) { fb.title(wb.title()); json.addProperty("t", legacy.serialize(wb.title())); }
            if (written && wb.hasAuthor()) { fb.author(wb.author()); json.addProperty("a", legacy.serialize(wb.author())); }
            int generation = written && wb.hasGeneration() ? wb.getGeneration().ordinal() : 0;
            if (generation > 0) fb.setGeneration(wb.getGeneration());
            json.addProperty("g", generation);
            var pages = new com.google.gson.JsonArray();
            for (var page : wb.pages()) pages.add(written ? gson.serialize(page) : legacy.serialize(page));
            fb.pages(wb.pages());
            json.add("p", pages);
            if (!wb.pages().isEmpty() || (written && (wb.hasTitle() || wb.hasAuthor() || generation > 0))) book = json.toString();
        }
        var data = new FerriteDataContainer();
        worn.getPersistentDataContainer().copyTo(data, true);
        data.copyTo(fresh.getPersistentDataContainer(), true);
        if (!fresh.equals(worn)) throw new UnsupportedOperationException("Ferrite: inventory metadata beyond name, lore, enchantments, unbreakable, model data, repair cost, item flags, persistent data, book content and a map view is not implemented");
        meta = String.join("\u001e", name,
            lore == null ? "" : String.join("\u001d", lore.stream().map(gson::serialize).toList()),
            String.join("\u001d", enchants), worn.isUnbreakable() ? "1" : "0", model == null ? "" : model.toString(), Integer.toString(repair),
            String.join(",", flags), data.toSnbt(), book == null ? "" : book);
        if (name.isEmpty() && lore == null && enchants.isEmpty() && !worn.isUnbreakable() && model == null && repair == 0 && flags.isEmpty() && data.isEmpty() && book == null) meta = null;
    }
        return new ItemMetaWire(damage, mapId, meta);
    }
}
