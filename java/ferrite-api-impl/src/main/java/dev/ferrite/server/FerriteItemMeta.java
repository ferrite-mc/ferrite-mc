package dev.ferrite.server;

final class FerriteItemMeta implements java.lang.reflect.InvocationHandler {
    private net.kyori.adventure.text.Component display;
    private java.util.List<net.kyori.adventure.text.Component> lore;
    private Integer model;
    private FerriteDataContainer data = new FerriteDataContainer();
    private final java.util.Map<org.bukkit.enchantments.Enchantment, Integer> enchants = new java.util.HashMap<>();
    private final java.util.Set<org.bukkit.inventory.ItemFlag> flags = java.util.EnumSet.noneOf(org.bukkit.inventory.ItemFlag.class);
    private boolean unbreakable;
    private Integer damage;
    private int repairCost;
    /** Written and writable books also answer to BookMeta. */
    private boolean book;
    /** Filled maps also answer to MapMeta. */
    private boolean map;
    private org.bukkit.map.MapView mapView;
    private org.bukkit.Color mapColor;
    private boolean scaling;
    private net.kyori.adventure.text.Component locationName;
    private net.kyori.adventure.text.Component title, author;
    private org.bukkit.inventory.meta.BookMeta.Generation generation;
    /** A signed book (`written_book`); a book and quill has pages only. */
    private boolean written;
    /** Potions and tipped arrows also answer to PotionMeta; enchanted books to EnchantmentStorageMeta. Held on the Java side only: the host wire refuses them. */
    private boolean potion, storage;
    private org.bukkit.potion.PotionType basePotion;
    /** Firework rockets also answer to FireworkMeta: flight duration (0..127) and the stars. Held on the Java side; `fireworkBoost` reads them. */
    private boolean firework;
    private int power;
    private final java.util.List<org.bukkit.FireworkEffect> fireworkEffects = new java.util.ArrayList<>();
    /** Vanilla components ItemMeta has no field for (fireworks, trim, banner patterns ...), kept as SNBT so equality and matching see them; the host wire refuses any stack that carries one. */
    private final java.util.Map<String, Object> extra = new java.util.TreeMap<>();
    /** Keeps a component this meta cannot model; false when {@code meta} is not one of ours. */
    static boolean keepComponent(org.bukkit.inventory.meta.ItemMeta meta, String name, Object value) {
        if (!java.lang.reflect.Proxy.isProxyClass(meta.getClass()) || !(java.lang.reflect.Proxy.getInvocationHandler(meta) instanceof FerriteItemMeta m)) return false;
        m.extra.put(name, value);
        return true;
    }
    private final java.util.Map<org.bukkit.enchantments.Enchantment, Integer> stored = new java.util.HashMap<>();
    private final java.util.List<net.kyori.adventure.text.Component> pages = new java.util.ArrayList<>();
    /** Vanilla limits: a title is at most 32 characters, a book at most 100 pages, a book and quill page at most 1024. */
    static final int MAX_TITLE = 32, MAX_PAGES = 100, MAX_WRITABLE_PAGE = 1024;
    private static final net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer LEGACY = net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection();
    static org.bukkit.inventory.meta.ItemMeta create() { return new FerriteItemMeta().proxy(); }
    static org.bukkit.inventory.meta.ItemMeta create(org.bukkit.Material material) {
        var meta = new FerriteItemMeta();
        meta.book = material == org.bukkit.Material.WRITTEN_BOOK || material == org.bukkit.Material.WRITABLE_BOOK;
        meta.written = material == org.bukkit.Material.WRITTEN_BOOK;
        meta.map = material == org.bukkit.Material.FILLED_MAP;
        meta.potion = material == org.bukkit.Material.POTION || material == org.bukkit.Material.SPLASH_POTION || material == org.bukkit.Material.LINGERING_POTION || material == org.bukkit.Material.TIPPED_ARROW;
        meta.storage = material == org.bukkit.Material.ENCHANTED_BOOK;
        meta.firework = material == org.bukkit.Material.FIREWORK_ROCKET;
        return meta.proxy();
    }
    private org.bukkit.inventory.meta.ItemMeta proxy() { return (org.bukkit.inventory.meta.ItemMeta) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), book ? new Class<?>[]{org.bukkit.inventory.meta.BookMeta.class} : firework ? new Class<?>[]{org.bukkit.inventory.meta.FireworkMeta.class} : map ? new Class<?>[]{org.bukkit.inventory.meta.MapMeta.class} : potion ? new Class<?>[]{org.bukkit.inventory.meta.PotionMeta.class} : storage ? new Class<?>[]{org.bukkit.inventory.meta.EnchantmentStorageMeta.class} : new Class<?>[]{org.bukkit.inventory.meta.ItemMeta.class, org.bukkit.inventory.meta.Damageable.class}, this); }
    private org.bukkit.inventory.meta.ItemMeta copy() { var c = new FerriteItemMeta(); c.display = display; c.lore = lore == null ? null : java.util.List.copyOf(lore); c.model = model; c.data = data.copy(); c.enchants.putAll(enchants); c.flags.addAll(flags); c.unbreakable = unbreakable; c.damage = damage; c.repairCost = repairCost; c.book = book; c.map = map; c.mapView = mapView; c.mapColor = mapColor; c.scaling = scaling; c.locationName = locationName; c.title = title; c.author = author; c.generation = generation; c.written = written; c.potion = potion; c.extra.putAll(extra); c.storage = storage; c.basePotion = basePotion; c.firework = firework; c.power = power; c.fireworkEffects.addAll(fireworkEffects); c.stored.putAll(stored); c.pages.addAll(pages); return c.proxy(); }
    private boolean sameAs(FerriteItemMeta o) {
        return java.util.Objects.equals(display, o.display) && java.util.Objects.equals(lore, o.lore) && java.util.Objects.equals(model, o.model)
            && enchants.equals(o.enchants) && flags.equals(o.flags) && unbreakable == o.unbreakable
            && java.util.Objects.equals(damage == null ? 0 : damage, o.damage == null ? 0 : o.damage) && repairCost == o.repairCost && java.util.Objects.equals(title, o.title) && java.util.Objects.equals(author, o.author) && generation == o.generation && pages.equals(o.pages) && basePotion == o.basePotion && extra.equals(o.extra) && stored.equals(o.stored) && firework == o.firework && power == o.power && fireworkEffects.equals(o.fireworkEffects) && data.sameAs(o.data);
    }
    /** A page past the 100th is dropped, and a book and quill page is cut to 1024 characters, as Paper does. */
    private void addPage(net.kyori.adventure.text.Component page) {
        if (pages.size() >= MAX_PAGES) return;
        if (!written) {
            var text = LEGACY.serialize(page);
            if (text.length() > MAX_WRITABLE_PAGE) page = LEGACY.deserialize(text.substring(0, MAX_WRITABLE_PAGE));
        }
        pages.add(page);
    }
    @SuppressWarnings("unchecked") public Object invoke(Object self, java.lang.reflect.Method method, Object[] args) {
        return switch (method.getName()) {
            case "hasDisplayName", "hasCustomName" -> display != null;
            case "displayName", "customName" -> { if (args == null || args.length == 0) yield display; display = (net.kyori.adventure.text.Component) args[0]; yield null; }
            case "getDisplayName" -> display == null ? "" : LEGACY.serialize(display);
            case "setDisplayName" -> { display = args[0] == null ? null : LEGACY.deserialize((String) args[0]); yield null; }
            case "hasLore" -> lore != null && !lore.isEmpty();
            case "lore" -> { if (args == null || args.length == 0) yield lore == null ? null : java.util.List.copyOf(lore); lore = args[0] == null ? null : java.util.List.copyOf((java.util.List<net.kyori.adventure.text.Component>) args[0]); yield null; }
            case "getLore" -> lore == null ? null : lore.stream().map(LEGACY::serialize).toList();
            case "setLore" -> { lore = args[0] == null ? null : ((java.util.List<String>) args[0]).stream().<net.kyori.adventure.text.Component>map(LEGACY::deserialize).toList(); yield null; }
            case "hasCustomModelData" -> model != null;
            case "getCustomModelData" -> { if (model == null) throw new IllegalStateException("No custom model data"); yield model; }
            case "setCustomModelData" -> { model = (Integer) args[0]; yield null; }
            case "hasEnchants" -> !enchants.isEmpty();
            case "hasEnchant" -> enchants.containsKey(args[0]);
            case "getEnchantLevel" -> enchants.getOrDefault(args[0], 0);
            case "getEnchants" -> java.util.Map.copyOf(enchants);
            case "addEnchant" -> { Integer old = enchants.put((org.bukkit.enchantments.Enchantment) args[0], (Integer) args[1]); yield !java.util.Objects.equals(old, args[1]); }
            case "removeEnchant" -> enchants.remove(args[0]) != null;
            case "removeEnchantments" -> { enchants.clear(); yield null; }
            case "getPersistentDataContainer" -> data;
            case "clone" -> copy();
            case "addItemFlags" -> { for (var flag : (org.bukkit.inventory.ItemFlag[]) args[0]) flags.add(flag); yield null; }
            case "removeItemFlags" -> { for (var flag : (org.bukkit.inventory.ItemFlag[]) args[0]) flags.remove(flag); yield null; }
            case "getItemFlags" -> java.util.Set.copyOf(flags);
            case "hasItemFlag" -> flags.contains(args[0]);
            case "isUnbreakable" -> unbreakable;
            case "setUnbreakable" -> { unbreakable = (boolean) args[0]; yield null; }
            case "hasDamage" -> damage != null && damage > 0;
            case "getDamage" -> damage == null ? 0 : damage;
            case "setDamage" -> { damage = (int) args[0]; yield null; }
            case "hasMapView", "hasMapId" -> mapView != null;
            case "getMapView" -> mapView;
            case "setMapView" -> { mapView = (org.bukkit.map.MapView) args[0]; yield null; }
            case "getMapId" -> { if (mapView == null) throw new IllegalStateException("No map id"); yield mapView.getId(); }
            case "setMapId" -> { mapView = org.bukkit.Bukkit.getMap((int) args[0]); yield null; }
            case "hasColor" -> mapColor != null;
            case "getColor" -> mapColor;
            case "setColor" -> { mapColor = (org.bukkit.Color) args[0]; yield null; }
            case "isScaling" -> scaling;
            case "setScaling" -> { scaling = (boolean) args[0]; yield null; }
            case "hasLocationName" -> locationName != null;
            case "getLocationName" -> locationName;
            case "setLocationName" -> { locationName = (net.kyori.adventure.text.Component) args[0]; yield null; }
            case "hasTitle" -> title != null;
            case "hasAuthor" -> author != null;
            case "hasPages" -> !pages.isEmpty();
            case "getPageCount" -> pages.size();
            case "title" -> { if (args == null || args.length == 0) yield title; title = (net.kyori.adventure.text.Component) args[0]; yield method.getReturnType().isInstance(self) ? self : null; }
            case "author" -> { if (args == null || args.length == 0) yield author; author = (net.kyori.adventure.text.Component) args[0]; yield method.getReturnType().isInstance(self) ? self : null; }
            case "getTitle" -> title == null ? null : LEGACY.serialize(title);
            case "getAuthor" -> author == null ? null : LEGACY.serialize(author);
            case "setTitle" -> {
                // Like Paper, an over-long title is refused and the old one stays.
                if (args[0] != null && ((String) args[0]).length() > MAX_TITLE) yield false;
                title = args[0] == null ? null : LEGACY.deserialize((String) args[0]); yield true;
            }
            case "hasGeneration" -> generation != null;
            case "getGeneration" -> generation;
            case "setGeneration" -> { generation = (org.bukkit.inventory.meta.BookMeta.Generation) args[0]; yield null; }
            case "setAuthor" -> { author = args[0] == null ? null : LEGACY.deserialize((String) args[0]); yield null; }
            case "pages" -> {
                if (args == null || args.length == 0) yield java.util.List.copyOf(pages);
                var next = args[0] instanceof java.util.List<?> l ? (java.util.List<net.kyori.adventure.text.Component>) l : java.util.List.of((net.kyori.adventure.text.Component[]) args[0]);
                pages.clear();
                for (var page : next) addPage(page);
                yield method.getReturnType().isInstance(self) ? self : null;
            }
            case "addPages" -> { for (var page : (Object[]) args[0]) addPage(page instanceof String text ? LEGACY.deserialize(text) : (net.kyori.adventure.text.Component) page); yield null; }
            case "getPages" -> pages.stream().map(LEGACY::serialize).toList();
            case "getPage" -> LEGACY.serialize(pages.get((int) args[0] - 1));
            case "setPages" -> { pages.clear(); var list = args[0] instanceof java.util.List<?> l ? l : java.util.Arrays.asList((Object[]) args[0]); for (var page : list) addPage(LEGACY.deserialize((String) page)); yield null; }
            case "getPower" -> power;
            case "setPower" -> {
                if ((int) args[0] < 0 || (int) args[0] > 127) throw new IllegalArgumentException("Power cannot be less than zero or greater than 127: " + args[0]);
                power = (int) args[0]; yield null;
            }
            case "hasEffects" -> !fireworkEffects.isEmpty();
            case "getEffects" -> java.util.List.copyOf(fireworkEffects);
            case "getEffectsSize" -> fireworkEffects.size();
            case "addEffect" -> { fireworkEffects.add(java.util.Objects.requireNonNull((org.bukkit.FireworkEffect) args[0], "effect")); yield null; }
            case "addEffects" -> {
                var added = args[0] instanceof Iterable<?> it ? it : java.util.Arrays.asList((Object[]) args[0]);
                for (var effect : added) fireworkEffects.add(java.util.Objects.requireNonNull((org.bukkit.FireworkEffect) effect, "effect"));
                yield null;
            }
            case "removeEffect" -> {
                int index = (int) args[0];
                if (index < 0 || index >= fireworkEffects.size()) throw new IndexOutOfBoundsException("Index: " + index + ", Size: " + fireworkEffects.size());
                fireworkEffects.remove(index); yield null;
            }
            case "clearEffects" -> { fireworkEffects.clear(); yield null; }
            case "hasBasePotionType" -> basePotion != null;
            case "getBasePotionType" -> basePotion;
            case "setBasePotionType" -> { basePotion = (org.bukkit.potion.PotionType) args[0]; yield null; }
            case "hasCustomEffects" -> false;
            case "getCustomEffects" -> java.util.List.of();
            case "hasStoredEnchants" -> !stored.isEmpty();
            case "getStoredEnchants" -> java.util.Map.copyOf(stored);
            case "hasStoredEnchant" -> stored.containsKey((org.bukkit.enchantments.Enchantment) args[0]);
            case "getStoredEnchantLevel" -> stored.getOrDefault((org.bukkit.enchantments.Enchantment) args[0], 0);
            case "addStoredEnchant" -> { var had = stored.put((org.bukkit.enchantments.Enchantment) args[0], (int) args[1]); yield had == null || had != (int) args[1]; }
            case "removeStoredEnchant" -> stored.remove((org.bukkit.enchantments.Enchantment) args[0]) != null;
            case "hasRepairCost" -> repairCost > 0;
            case "getRepairCost" -> repairCost;
            case "setRepairCost" -> { repairCost = (int) args[0]; yield null; }
            case "hashCode" -> java.util.Objects.hash(display, lore, model, enchants, flags, unbreakable, damage, repairCost);
            case "equals" -> self == args[0] || (args[0] != null && java.lang.reflect.Proxy.isProxyClass(args[0].getClass()) && java.lang.reflect.Proxy.getInvocationHandler(args[0]) instanceof FerriteItemMeta o && sameAs(o));
            case "toString" -> "FerriteItemMeta{" + (display == null ? "" : "name=" + display + ", ") + "enchants=" + enchants + ", flags=" + flags + "}";
            default -> HostBridge.dispatch("ItemMeta", "", method, args);
        };
    }
}
