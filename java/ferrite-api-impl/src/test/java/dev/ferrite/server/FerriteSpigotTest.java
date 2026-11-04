package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class FerriteSpigotTest {
    private static final String PLAYER_ID = "00000000-0000-0000-0000-0000000000ab";
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerPlayer(PLAYER_ID, "Spigot", false);
        player = registered(PLAYER_ID, "Spigot");
    }

    private static org.bukkit.entity.Player registered(String id, String name) throws ReflectiveOperationException {
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        return (org.bukkit.entity.Player) ensure.invoke(null, id, name);
    }

    @Test void spigotReturnsOneCachedFacade() {
        var spigot = player.spigot();
        assertNotNull(spigot);
        assertSame(spigot, player.spigot());
        assertSame(spigot, ((org.bukkit.entity.Player) player).spigot());
    }

    @Test void textComponentsReachThePlayerMessagePath() {
        var sent = new java.util.ArrayList<String>();
        var recording = (org.bukkit.entity.Player) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.entity.Player.class}, (proxy, method, args) -> {
            if (method.getName().equals("sendMessage") && args != null) {
                if (args.length == 1 && args[0] instanceof String text) { sent.add(text); return null; }
                if (args.length == 2 && args[0] instanceof java.util.UUID && args[1] instanceof String text) { sent.add(text); return null; }
            }
            if (method.getName().equals("sendActionBar") && args != null && args.length == 1 && args[0] instanceof String text) { sent.add("action-bar:" + text); return null; }
            try {
                return method.invoke(player, args);
            } catch (java.lang.reflect.InvocationTargetException failure) {
                throw failure.getCause();
            }
        });
        var spigot = new FerritePlayerSpigot(recording);
        spigot.sendMessage(new net.md_5.bungee.api.chat.TextComponent("hello"));
        spigot.sendMessage(new net.md_5.bungee.api.chat.TextComponent("two"), new net.md_5.bungee.api.chat.TextComponent(" parts"));
        spigot.sendMessage(java.util.UUID.fromString("00000000-0000-0000-0000-0000000000aa"), new net.md_5.bungee.api.chat.TextComponent("from"));
        spigot.sendMessage(net.md_5.bungee.api.ChatMessageType.ACTION_BAR, new net.md_5.bungee.api.chat.TextComponent("bar"));
        assertEquals(4, sent.size());
        assertEquals("hello", plain(sent.get(0)));
        assertEquals("two parts", plain(sent.get(1)));
        assertEquals("from", plain(sent.get(2)));
        assertEquals("action-bar:bar", plain(sent.get(3)));
    }

    /** Bungee's legacy serializer prefixes each component with a reset code. */
    private static String plain(String legacy) { return legacy.replaceAll("\u00a7.", ""); }

    @Test void hiddenPlayersFollowVisibility() throws Exception {
        var otherId = "00000000-0000-0000-0000-0000000000ba";
        Bootstrap.registerPlayer(otherId, "Hidden", false);
        var other = registered(otherId, "Hidden");
        assertTrue(player.canSee(other));
        assertFalse(player.spigot().getHiddenPlayers().contains(other));
        player.hidePlayer(other);
        assertTrue(player.spigot().getHiddenPlayers().contains(other));
        player.showPlayer(other);
        assertFalse(player.spigot().getHiddenPlayers().contains(other));
    }
}
