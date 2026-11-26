// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import org.junit.jupiter.api.Test;

class FerriteNmsVersionTest {
    @Test
    void pinsTheSingleSupportedTarget() {
        assertEquals("26.3", FerriteNmsVersion.MINECRAFT_VERSION);
        assertEquals(777, FerriteNmsVersion.PROTOCOL_VERSION);
        assertEquals(5023, FerriteNmsVersion.DATA_VERSION);
        assertEquals("26.3.build.227-stable", FerriteNmsVersion.PAPER_API);
        assertEquals(
                new dev.ferrite.nms.host.HostVersion("26.3", 777, 5023),
                FerriteNmsVersion.hostVersion());
    }

    @Test
    void rejectsAnotherMinecraftVersionWithTheReasonInTheMessage() {
        NmsVersionMismatchException error = assertThrows(
                NmsVersionMismatchException.class,
                () -> FerriteNmsVersion.requireSupported("26.2", 777, 5023));
        assertEquals(
                "Ferrite NMS supports Minecraft 26.3 (protocol 777, data 5023) only; "
                        + "the host reports 26.2 (protocol 777, data 5023)",
                error.getMessage());
    }

    @Test
    void rejectsAnotherProtocolOrDataVersion() {
        assertThrows(NmsVersionMismatchException.class,
                () -> FerriteNmsVersion.requireSupported("26.3", 776, 5023));
        assertThrows(NmsVersionMismatchException.class,
                () -> FerriteNmsVersion.requireSupported("26.3", 777, 5000));
    }
}
