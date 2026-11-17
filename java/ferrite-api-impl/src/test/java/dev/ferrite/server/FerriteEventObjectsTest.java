package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.UUID;
import org.bukkit.Location;
import org.bukkit.entity.Enderman;
import org.bukkit.entity.EntityType;
import org.junit.jupiter.api.Test;

/** Event proxies expose the concrete Bukkit interfaces listeners cast to. */
class FerriteEventObjectsTest {
    @Test void endermenArePublishedAsEndermen() {
        var entity = FerriteEventObjects.living(
            UUID.fromString("00000000-0000-0000-0000-0000000000e9"),
            EntityType.ENDERMAN,
            new Location(null, 0.5, 64.0, 0.5),
            20.0
        );
        assertInstanceOf(Enderman.class, entity);
        assertEquals(EntityType.ENDERMAN, ((Enderman) entity).getType());
    }
}
