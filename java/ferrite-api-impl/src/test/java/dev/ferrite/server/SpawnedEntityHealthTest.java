package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.UUID;
import org.bukkit.entity.EntityType;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

/** A spawned mob's health getters agree with what the host reports for the entity. */
class SpawnedEntityHealthTest {
    @AfterEach void reset() { HostBridge.transport = null; }

    @Test void healthAndMaxHealthComeFromTheHost() {
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "getHealth()" -> "4.0";
            case "getMaxHealth()" -> "10.0";
            default -> throw new UnsupportedOperationException(method);
        };
        var cow = (org.bukkit.entity.LivingEntity) SpawnedEntity.of(EntityType.COW, null, null, UUID.randomUUID());
        assertEquals(4.0, cow.getHealth());
        assertEquals(10.0, cow.getMaxHealth());
    }

    @Test void aGoneEntityIsNotAnsweredWithAGuess() {
        HostBridge.transport = (owner, subject, method, args) -> { throw new UnsupportedOperationException(method); };
        var cow = (org.bukkit.entity.LivingEntity) SpawnedEntity.of(EntityType.COW, null, null, UUID.randomUUID());
        assertThrows(UnsupportedOperationException.class, cow::getMaxHealth);
    }
}
