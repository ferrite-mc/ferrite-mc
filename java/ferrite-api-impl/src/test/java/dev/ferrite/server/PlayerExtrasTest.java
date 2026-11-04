package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Statistic;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class PlayerExtrasTest {
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000aa", "Extras", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000aa", "Extras");
    }

    @Test void attributesApplyModifiersInVanillaOrder() {
        var speed = player.getAttribute(Attribute.MOVEMENT_SPEED);
        assertEquals(0.1, speed.getValue(), 1e-9);
        speed.setBaseValue(1.0);
        speed.addModifier(new AttributeModifier(new org.bukkit.NamespacedKey("test", "add"), 1.0, AttributeModifier.Operation.ADD_NUMBER));
        speed.addModifier(new AttributeModifier(new org.bukkit.NamespacedKey("test", "half"), 0.5, AttributeModifier.Operation.ADD_SCALAR));
        speed.addModifier(new AttributeModifier(new org.bukkit.NamespacedKey("test", "double"), 1.0, AttributeModifier.Operation.MULTIPLY_SCALAR_1));
        assertEquals((1.0 + 1.0) * 1.5 * 2.0, speed.getValue(), 1e-9);
        assertSame(speed, player.getAttribute(Attribute.MOVEMENT_SPEED));
    }

    @Test void statisticsTrackQualifiersAndRejectNegatives() {
        player.incrementStatistic(Statistic.JUMP, 3);
        player.decrementStatistic(Statistic.JUMP);
        assertEquals(2, player.getStatistic(Statistic.JUMP));
        player.setStatistic(Statistic.MINE_BLOCK, org.bukkit.Material.STONE, 5);
        assertEquals(5, player.getStatistic(Statistic.MINE_BLOCK, org.bukkit.Material.STONE));
        assertEquals(0, player.getStatistic(Statistic.MINE_BLOCK, org.bukkit.Material.DIRT));
        assertThrows(IllegalArgumentException.class, () -> player.getStatistic(Statistic.MINE_BLOCK));
        assertThrows(IllegalArgumentException.class, () -> player.setStatistic(Statistic.JUMP, -1));
    }

    @Test void conversationsRunAndAbandon() {
        var prompt = new org.bukkit.conversations.StringPrompt() {
            String got;
            @Override public String getPromptText(org.bukkit.conversations.ConversationContext c) { return "Name?"; }
            @Override public org.bukkit.conversations.Prompt acceptInput(org.bukkit.conversations.ConversationContext c, String input) { got = input; return END_OF_CONVERSATION; }
        };
        var conversation = new org.bukkit.conversations.ConversationFactory(null).withFirstPrompt(prompt).withLocalEcho(false).buildConversation(player);
        assertTrue(player.beginConversation(conversation));
        assertTrue(player.isConversing());
        assertNull(Bootstrap.fireChat("00000000-0000-0000-0000-0000000000aa", "Extras", "Alex"));
        assertEquals("Alex", prompt.got);
        assertFalse(player.isConversing());
    }

    @Test void bodyBookkeepingReadsBackWhatWasSet() {
        player.setArrowsInBody(4, false);
        assertEquals(4, player.getArrowsInBody());
        assertThrows(IllegalArgumentException.class, () -> player.setArrowsInBody(-1, false));
        player.setPlayerTime(6000, false);
        assertEquals(6000, player.getPlayerTime());
        assertFalse(player.isPlayerTimeRelative());
        player.resetPlayerTime();
        assertTrue(player.isPlayerTimeRelative());
        assertFalse(player.isInsideVehicle());
        assertTrue(player.getPassengers().isEmpty());
    }

    @Test void visibilityListingAndAlliesAreRemembered() {
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000cc", "Friend", false);
        org.bukkit.entity.Player other;
        try {
            var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
            ensure.setAccessible(true);
            other = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000cc", "Friend");
        } catch (ReflectiveOperationException e) { throw new AssertionError(e); }
        assertTrue(player.canSee(other));
        player.hidePlayer(other);
        assertFalse(player.canSee(other));
        player.showPlayer(other);
        assertTrue(player.canSee(other));
        assertTrue(player.isListed(other));
        assertTrue(player.unlistPlayer(other));
        assertFalse(player.isListed(other));
        assertTrue(player.listPlayer(other));
        assertTrue(player.isAlliedTo(player));
        player.addAdditionalAlliedRule(net.kyori.adventure.key.Key.key("test", "rule"), e -> true);
        assertTrue(player.isAlliedTo(other));
        player.removeAdditionalAlliedRule(net.kyori.adventure.key.Key.key("test", "rule"));
        assertTrue(player.getAdditionalAlliedRules().isEmpty());
        assertSame(player.getEnderChest(), player.getEnderChest());
        assertEquals(27, player.getEnderChest().getSize());
        player.resetIdleDuration();
        assertTrue(player.getIdleDuration().toSeconds() < 5);
    }
}
