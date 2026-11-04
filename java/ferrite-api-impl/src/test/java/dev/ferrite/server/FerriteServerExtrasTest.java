package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.UUID;
import org.bukkit.Location;
import org.junit.jupiter.api.Test;

class FerriteServerExtrasTest {
    @Test
    void worldBorderKeepsItsState() {
        var border = new FerriteWorldBorder();
        border.setSize(100);
        border.setCenter(10, -10);
        assertEquals(100, border.getSize());
        assertTrue(border.isInside(new Location(null, 55, 0, -10)));
        assertFalse(border.isInside(new Location(null, 61, 0, -10)));
        assertThrows(IllegalArgumentException.class, () -> border.setSize(0));
        border.reset();
        assertEquals(border.getMaxSize(), border.getSize());
    }

    @Test
    void profilesCarryIdNameAndProperties() {
        var id = UUID.randomUUID();
        var profile = new FerriteProfile(id, "Steve");
        assertTrue(profile.isComplete());
        profile.setProperty(new com.destroystokyo.paper.profile.ProfileProperty("textures", "abc"));
        assertTrue(profile.hasProperty("textures"));
        var copy = profile.clone();
        assertEquals(profile, copy);
        assertTrue(copy.removeProperty("textures"));
        assertTrue(profile.hasProperty("textures"));
        assertThrows(IllegalArgumentException.class, () -> new FerriteProfile(null, null));
    }
}
