package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.BookMeta;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Books written through `PlayerInventory#setItem` reach the host with every field intact. */
class BookMetaBridgeTest {
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000f1", "books_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000ab", "Books", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000ab", "Books");
    }

    @org.junit.jupiter.api.AfterAll static void stop() {
        var world = org.bukkit.Bukkit.getWorld("books_world");
        if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    /** The `setItemMeta` payloads the host would receive, by slot. */
    private static java.util.Map<Integer, String> send(ItemStack... items) {
        var sent = new java.util.HashMap<Integer, String>();
        HostBridge.transport = (owner, subject, method, args) -> {
            if (method.equals("setItemMeta(int,String)")) {
                int split = args.indexOf(HostBridge.ARG);
                sent.put(Integer.parseInt(args.substring(0, split)), args.substring(split + 1));
            }
            return "";
        };
        try {
            for (int slot = 0; slot < items.length; slot++) {
                try { player.getInventory().setItem(slot, items[slot]); } catch (IllegalStateException noNative) { /* no host in unit tests */ }
            }
        } finally { HostBridge.transport = null; }
        return sent;
    }

    private static com.google.gson.JsonObject bookField(String payload) {
        var fields = payload.split("\u001e", -1);
        assertEquals(10, fields.length, "the book is the ninth of ten fields: " + payload);
        return com.google.gson.JsonParser.parseString(fields[8]).getAsJsonObject();
    }

    @Test void aSignedBookSendsTitleAuthorGenerationAndUnicodePages() {
        var book = new ItemStack(Material.WRITTEN_BOOK);
        var meta = (BookMeta) book.getItemMeta();
        meta.title(Component.text("Das Buch ✨"));
        meta.author(Component.text("Álvaro"));
        meta.setGeneration(BookMeta.Generation.COPY_OF_COPY);
        meta.addPages(Component.text("Seite 1: Grüße 日本語 😀"), Component.text("a\u001eb\u001fc").color(net.kyori.adventure.text.format.NamedTextColor.RED));
        assertTrue(book.setItemMeta(meta));
        var sent = send(book);
        var json = bookField(sent.get(0));
        assertEquals("B", json.get("k").getAsString());
        assertEquals("Das Buch ✨", json.get("t").getAsString());
        assertEquals("Álvaro", json.get("a").getAsString());
        assertEquals(2, json.get("g").getAsInt());
        var pages = json.getAsJsonArray("p");
        assertEquals(2, pages.size());
        assertTrue(pages.get(0).getAsString().contains("日本語 😀"), pages.get(0).getAsString());
        // Separator characters inside a page cannot split the payload: it arrived as nine fields.
        assertTrue(pages.get(1).getAsString().contains("\"color\":\"red\""), pages.get(1).getAsString());
        var back = net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().deserialize(pages.get(1).getAsString());
        assertEquals("a\u001eb\u001fc", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(back));
    }

    @Test void aBookAndQuillSendsPlainSectionCodePages() {
        var quill = new ItemStack(Material.WRITABLE_BOOK);
        var meta = (BookMeta) quill.getItemMeta();
        meta.setPages("§cred page", "", "😀 Grüße");
        quill.setItemMeta(meta);
        var json = bookField(send(quill).get(0));
        assertEquals("W", json.get("k").getAsString());
        assertFalse(json.has("t") || json.has("a"), "a book and quill has no title or author");
        assertEquals(0, json.get("g").getAsInt());
        assertEquals("§cred page", json.getAsJsonArray("p").get(0).getAsString());
        assertEquals("", json.getAsJsonArray("p").get(1).getAsString());
        assertEquals("😀 Grüße", json.getAsJsonArray("p").get(2).getAsString());
    }

    @Test void vanillaLimitsApplyBeforeAnythingIsSent() {
        var book = new ItemStack(Material.WRITTEN_BOOK);
        var meta = (BookMeta) book.getItemMeta();
        assertFalse(meta.setTitle("x".repeat(33)), "an over-long title is refused");
        assertFalse(meta.hasTitle());
        assertTrue(meta.setTitle("x".repeat(32)));
        for (int i = 0; i < 120; i++) meta.addPages(Component.text("p" + i));
        assertEquals(100, meta.getPageCount(), "pages past the 100th are dropped");
        book.setItemMeta(meta);
        assertEquals(100, bookField(send(book).get(0)).getAsJsonArray("p").size());
        var quill = new ItemStack(Material.WRITABLE_BOOK);
        var quillMeta = (BookMeta) quill.getItemMeta();
        quillMeta.setPages("y".repeat(2000));
        quill.setItemMeta(quillMeta);
        assertEquals(1024, bookField(send(quill).get(0)).getAsJsonArray("p").get(0).getAsString().length());
    }

    @Test void cloningABookIsolatesItsPages() {
        var book = new ItemStack(Material.WRITTEN_BOOK);
        var meta = (BookMeta) book.getItemMeta();
        meta.addPages(Component.text("one"));
        meta.setGeneration(BookMeta.Generation.ORIGINAL);
        book.setItemMeta(meta);
        var clone = book.clone();
        assertTrue(clone.isSimilar(book));
        var cloneMeta = (BookMeta) clone.getItemMeta();
        cloneMeta.addPages(Component.text("two"));
        clone.setItemMeta(cloneMeta);
        assertFalse(clone.isSimilar(book), "an added page makes a different book");
        assertEquals(1, ((BookMeta) book.getItemMeta()).getPageCount(), "the original is untouched");
        cloneMeta.setGeneration(BookMeta.Generation.TATTERED);
        assertNotEquals(((BookMeta) book.getItemMeta()).getGeneration(), cloneMeta.getGeneration());
        // Changing the meta after getting it from the item does not change the item until set.
        var detached = (BookMeta) book.getItemMeta();
        detached.addPages(Component.text("detached"));
        assertEquals(1, ((BookMeta) book.getItemMeta()).getPageCount());
    }

    @Test void anEmptyBookSendsNoBookFieldAndOtherItemsNeverDo() {
        var empty = new ItemStack(Material.WRITABLE_BOOK);
        var sword = new ItemStack(Material.DIAMOND_SWORD);
        var named = sword.clone();
        var meta = named.getItemMeta();
        meta.displayName(Component.text("n"));
        named.setItemMeta(meta);
        var sent = send(empty, named);
        assertNull(sent.get(0), "nothing to write for a book with no content");
        assertEquals("", sent.get(1).split("\u001e", -1)[8], "an item without a book has an empty book field");
    }
}
