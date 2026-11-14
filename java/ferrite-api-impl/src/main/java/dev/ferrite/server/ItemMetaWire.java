package dev.ferrite.server;

/**
 * The wire form of an item's metadata that the host reads ({@code setItemMeta(int,String)} and the metadata of a tile
 * snapshot): wear and a map id travel separately, everything else is one string of fields joined by U+001E.
 * Metadata the host cannot carry is refused with {@link UnsupportedOperationException}, never dropped.
 *
 * <p>Fields: name, lore, enchantments, unbreakable, custom model data, repair cost, item flags, persistent data,
 * book, extra components. The last field is an SNBT compound of the remaining components (attributes, potion
 * contents, glint override, limits, banner patterns, fireworks, profile, map colour), keyed by component name.
 * The host inserts each one with its normal NBT shape, so nothing is flattened or guessed.
 */
record ItemMetaWire(int damage, Integer mapId, String meta) {
    static ItemMetaWire of(org.bukkit.inventory.ItemStack item) {
        int damage = 0;
        Integer mapId = null;
        String meta = null;
        if (item != null && item.hasItemMeta()) {
            // The host carries wear, name, lore, enchantments, unbreakable, model, repair cost,
            // item flags, the persistent data container, book content and every other component
            // ItemMeta can model; anything else is refused so nothing is silently dropped.
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
            int repair = FerriteItemMeta.repairCost(worn);
            if (repair > 0) FerriteItemMeta.setRepairCost(fresh, repair);
            var flags = new java.util.TreeSet<String>();
            for (var flag : worn.getItemFlags()) flags.add(flag.name());
            if (!flags.isEmpty()) fresh.addItemFlags(worn.getItemFlags().toArray(org.bukkit.inventory.ItemFlag[]::new));
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
            // Every remaining component the Bukkit meta can hold. A component the host does not
            // know is not invented here: the equality check below still refuses it by type.
            var components = new java.util.LinkedHashMap<String, Object>();
            copyCoreFields(worn, fresh, components, gson);
            copySpecialised(worn, fresh, components, gson);
            var data = new FerriteDataContainer();
            worn.getPersistentDataContainer().copyTo(data, true);
            data.copyTo(fresh.getPersistentDataContainer(), true);
            for (var kept : FerriteItemMeta.components(worn).entrySet()) {
                components.put(kept.getKey(), kept.getValue());
            }
            if (!fresh.equals(worn)) throw new UnsupportedOperationException("Ferrite: item metadata for " + worn.getClass().getInterfaces()[0].getSimpleName()
                    + " beyond the components the host encodes is not implemented: " + components.keySet());
            meta = String.join("\u001e", name,
                lore == null ? "" : String.join("\u001d", lore.stream().map(gson::serialize).toList()),
                String.join("\u001d", enchants), worn.isUnbreakable() ? "1" : "0", model == null ? "" : model.toString(), Integer.toString(repair),
                String.join(",", flags), data.toSnbt(), book == null ? "" : book,
                components.isEmpty() ? "" : snbt(components));
            if (name.isEmpty() && lore == null && enchants.isEmpty() && !worn.isUnbreakable() && model == null && repair == 0 && flags.isEmpty() && data.isEmpty() && book == null && components.isEmpty()) meta = null;
        }
        return new ItemMetaWire(damage, mapId, meta);
    }

    /** Fields on the base {@link org.bukkit.inventory.meta.ItemMeta} interface. */
    private static void copyCoreFields(org.bukkit.inventory.meta.ItemMeta worn, org.bukkit.inventory.meta.ItemMeta fresh,
            java.util.Map<String, Object> out, net.kyori.adventure.text.serializer.gson.GsonComponentSerializer gson) {
        if (worn.hasItemName()) { out.put("minecraft:item_name", gson.serialize(worn.itemName())); fresh.itemName(worn.itemName()); }
        if (worn.hasEnchantmentGlintOverride()) { out.put("minecraft:enchantment_glint_override", worn.getEnchantmentGlintOverride() ? (byte) 1 : (byte) 0); fresh.setEnchantmentGlintOverride(worn.getEnchantmentGlintOverride()); }
        if (worn.hasMaxStackSize()) { out.put("minecraft:max_stack_size", worn.getMaxStackSize()); fresh.setMaxStackSize(worn.getMaxStackSize()); }
        if (worn instanceof org.bukkit.inventory.meta.Damageable wd && wd.hasMaxDamage()) { out.put("minecraft:max_damage", wd.getMaxDamage()); if (fresh instanceof org.bukkit.inventory.meta.Damageable fd) fd.setMaxDamage(wd.getMaxDamage()); }
        if (worn.hasRarity()) { out.put("minecraft:rarity", worn.getRarity().name().toLowerCase(java.util.Locale.ROOT)); fresh.setRarity(worn.getRarity()); }
        if (worn.hasItemModel()) { out.put("minecraft:item_model", worn.getItemModel().toString()); fresh.setItemModel(worn.getItemModel()); }
        if (worn.hasTooltipStyle()) { out.put("minecraft:tooltip_style", worn.getTooltipStyle().toString()); fresh.setTooltipStyle(worn.getTooltipStyle()); }
        if (worn.isHideTooltip()) { fresh.setHideTooltip(true); }
        if (worn.hasUseRemainder()) {
            var remainder = worn.getUseRemainder();
            var compound = new java.util.LinkedHashMap<String, Object>();
            compound.put("id", remainder.getType().getKey().toString());
            compound.put("count", remainder.getAmount());
            out.put("minecraft:use_remainder", compound);
            fresh.setUseRemainder(remainder);
        }
        var attributes = worn.getAttributeModifiers();
        if (!attributes.isEmpty()) {
            var list = new java.util.ArrayList<Object>();
            for (var entry : attributes.entries()) {
                var modifier = entry.getValue();
                var compound = new java.util.LinkedHashMap<String, Object>();
                compound.put("type", entry.getKey().getKey().toString());
                compound.put("id", modifier.getKey() == null ? "minecraft:" + modifier.getName() : modifier.getKey().toString());
                compound.put("amount", modifier.getAmount());
                compound.put("operation", switch (modifier.getOperation()) {
                    case ADD_NUMBER -> "add_value";
                    case ADD_SCALAR -> "add_multiplied_base";
                    case MULTIPLY_SCALAR_1 -> "add_multiplied_total";
                });
                compound.put("slot", slotName(modifier.getSlotGroup()));
                list.add(compound);
            }
            out.put("minecraft:attribute_modifiers", list);
            fresh.setAttributeModifiers(attributes);
        }
    }

    /** Components only a specialised meta interface exposes. */
    private static void copySpecialised(org.bukkit.inventory.meta.ItemMeta worn, org.bukkit.inventory.meta.ItemMeta fresh,
            java.util.Map<String, Object> out, net.kyori.adventure.text.serializer.gson.GsonComponentSerializer gson) {
        if (worn instanceof org.bukkit.inventory.meta.PotionMeta wp && fresh instanceof org.bukkit.inventory.meta.PotionMeta fp) {
            var compound = new java.util.LinkedHashMap<String, Object>();
            if (wp.hasBasePotionType()) { compound.put("potion", wp.getBasePotionType().getKey().toString()); fp.setBasePotionType(wp.getBasePotionType()); }
            if (wp.hasColor()) { compound.put("custom_color", wp.getColor().asRGB()); fp.setColor(wp.getColor()); }
            if (wp.hasCustomEffects()) {
                var effects = new java.util.ArrayList<Object>();
                for (var effect : wp.getCustomEffects()) {
                    var entry = new java.util.LinkedHashMap<String, Object>();
                    entry.put("id", effect.getType().getKey().toString());
                    entry.put("amplifier", effect.getAmplifier());
                    entry.put("duration", effect.getDuration());
                    entry.put("ambient", effect.isAmbient() ? (byte) 1 : (byte) 0);
                    entry.put("show_particles", effect.hasParticles() ? (byte) 1 : (byte) 0);
                    entry.put("show_icon", effect.hasIcon() ? (byte) 1 : (byte) 0);
                    effects.add(entry);
                    fp.addCustomEffect(effect, true);
                }
                compound.put("custom_effects", effects);
            }
            if (!compound.isEmpty()) out.put("minecraft:potion_contents", compound);
        }
        if (worn instanceof org.bukkit.inventory.meta.MapMeta wm && fresh instanceof org.bukkit.inventory.meta.MapMeta fm) {
            // 26.3 keeps a filled map's colour, scale and location name in its saved-map
            // record, not in an item component; the map id already travels separately and the
            // map store (FerriteMapView) owns the rest. Copying them keeps the comparison below
            // honest without putting a non-existent component on the stack.
            if (wm.hasColor()) fm.setColor(wm.getColor());
            if (wm.hasLocationName()) fm.setLocationName(wm.getLocationName());
            if (wm.isScaling()) fm.setScaling(true);
        }
        if (worn instanceof org.bukkit.inventory.meta.BannerMeta wb && fresh instanceof org.bukkit.inventory.meta.BannerMeta fb) {
            var patterns = wb.getPatterns();
            if (!patterns.isEmpty()) {
                var list = new java.util.ArrayList<Object>();
                for (var pattern : patterns) {
                    var entry = new java.util.LinkedHashMap<String, Object>();
                    entry.put("pattern", pattern.getPattern().getKey().toString());
                    entry.put("color", pattern.getColor().name().toLowerCase(java.util.Locale.ROOT));
                    list.add(entry);
                }
                out.put("minecraft:banner_patterns", list);
                fb.setPatterns(patterns);
            }
        }
        if (worn instanceof org.bukkit.inventory.meta.FireworkMeta wf && fresh instanceof org.bukkit.inventory.meta.FireworkMeta ff) {
            var compound = new java.util.LinkedHashMap<String, Object>();
            compound.put("flight_duration", wf.getPower());
            var explosions = new java.util.ArrayList<Object>();
            for (var effect : wf.getEffects()) {
                var entry = new java.util.LinkedHashMap<String, Object>();
                entry.put("shape", switch (effect.getType()) {
                    case BALL -> "small_ball";
                    case BALL_LARGE -> "large_ball";
                    case STAR -> "star";
                    case CREEPER -> "creeper";
                    case BURST -> "burst";
                });
                entry.put("colors", effect.getColors().stream().map(org.bukkit.Color::asRGB).toList());
                entry.put("fade_colors", effect.getFadeColors().stream().map(org.bukkit.Color::asRGB).toList());
                entry.put("trail", effect.hasTrail() ? (byte) 1 : (byte) 0);
                entry.put("twinkle", effect.hasFlicker() ? (byte) 1 : (byte) 0);
                explosions.add(entry);
            }
            compound.put("explosions", explosions);
            out.put("minecraft:fireworks", compound);
            ff.setPower(wf.getPower());
            for (var effect : wf.getEffects()) ff.addEffect(effect);
        }
        if (worn instanceof org.bukkit.inventory.meta.SkullMeta ws && fresh instanceof org.bukkit.inventory.meta.SkullMeta fs) {
            var profile = ws.getPlayerProfile();
            if (profile != null) {
                var compound = new java.util.LinkedHashMap<String, Object>();
                if (profile.getName() != null) compound.put("name", profile.getName());
                if (profile.getId() != null) {
                    java.util.UUID id = profile.getId();
                    compound.put("id", new int[] { (int) (id.getMostSignificantBits() >> 32), (int) id.getMostSignificantBits(), (int) (id.getLeastSignificantBits() >> 32), (int) id.getLeastSignificantBits() });
                }
                var properties = new java.util.ArrayList<Object>();
                for (var property : profile.getProperties()) {
                    var entry = new java.util.LinkedHashMap<String, Object>();
                    entry.put("name", property.getName());
                    entry.put("value", property.getValue());
                    if (property.getSignature() != null) entry.put("signature", property.getSignature());
                    properties.add(entry);
                }
                if (!properties.isEmpty()) compound.put("properties", properties);
                out.put("minecraft:profile", compound);
                fs.setPlayerProfile(profile);
            }
            if (ws.getNoteBlockSound() != null) { out.put("minecraft:note_block_sound", ws.getNoteBlockSound().toString()); fs.setNoteBlockSound(ws.getNoteBlockSound()); }
        }
    }

    private static String slotName(org.bukkit.inventory.EquipmentSlotGroup group) {
        if (group == null) return "any";
        if (group == org.bukkit.inventory.EquipmentSlotGroup.MAINHAND) return "mainhand";
        if (group == org.bukkit.inventory.EquipmentSlotGroup.OFFHAND) return "offhand";
        if (group == org.bukkit.inventory.EquipmentSlotGroup.FEET) return "feet";
        if (group == org.bukkit.inventory.EquipmentSlotGroup.LEGS) return "legs";
        if (group == org.bukkit.inventory.EquipmentSlotGroup.CHEST) return "chest";
        if (group == org.bukkit.inventory.EquipmentSlotGroup.HEAD) return "head";
        return group.toString();
    }

    // ---- a small SNBT writer for component values ------------------------------------------------
    private static String snbt(Object value) {
        StringBuilder out = new StringBuilder();
        write(out, value);
        return out.toString();
    }

    private static void write(StringBuilder out, Object value) {
        if (value == null) { out.append("null"); return; }
        if (value instanceof String text) { out.append(quote(text)); return; }
        if (value instanceof Boolean flag) { out.append(flag ? "1b" : "0b"); return; }
        if (value instanceof Byte b) { out.append(b).append('b'); return; }
        if (value instanceof Short s) { out.append(s).append('s'); return; }
        if (value instanceof Integer i) { out.append(i); return; }
        if (value instanceof Long l) { out.append(l).append('l'); return; }
        if (value instanceof Float f) { out.append(Float.isFinite(f) ? f + "f" : (f.isNaN() ? "NaNf" : (f > 0 ? "Infinityf" : "-Infinityf"))); return; }
        if (value instanceof Double d) { out.append(Double.isFinite(d) ? d + "d" : (d.isNaN() ? "NaNd" : (d > 0 ? "Infinityd" : "-Infinityd"))); return; }
        if (value instanceof int[] ints) {
            out.append("[I;");
            for (int i = 0; i < ints.length; i++) { if (i > 0) out.append(','); out.append(ints[i]); }
            out.append(']');
            return;
        }
        if (value instanceof java.util.Map<?, ?> map) {
            out.append('{');
            boolean first = true;
            for (var entry : map.entrySet()) {
                if (!first) out.append(',');
                first = false;
                out.append(quote(String.valueOf(entry.getKey()))).append(':');
                write(out, entry.getValue());
            }
            out.append('}');
            return;
        }
        if (value instanceof Iterable<?> list) {
            out.append('[');
            boolean first = true;
            for (var element : list) { if (!first) out.append(','); first = false; write(out, element); }
            out.append(']');
            return;
        }
        if (value instanceof Object[] array) { write(out, java.util.Arrays.asList(array)); return; }
        throw new UnsupportedOperationException("Ferrite: an item component value of type " + value.getClass().getName() + " has no SNBT form");
    }

    private static String quote(String text) {
        var out = new StringBuilder(text.length() + 2).append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (c == '\\' || c == '"') out.append('\\');
            out.append(c);
        }
        return out.append('"').toString();
    }
}
