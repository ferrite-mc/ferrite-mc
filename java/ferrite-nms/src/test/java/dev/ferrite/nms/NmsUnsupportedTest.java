// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import net.minecraft.server.level.ServerLevel;
import org.junit.jupiter.api.Test;

class NmsUnsupportedTest {
    @Test
    void messagesNameTheClassMethodAndReason() {
        UnsupportedOperationException error = NmsUnsupported.operation(
                "net.minecraft.server.network.ServerGamePacketListenerImpl",
                "send",
                "packet listeners are outside the selected 26.3 surface");
        assertTrue(error.getMessage().contains("ServerGamePacketListenerImpl#send"), error.getMessage());
        assertTrue(error.getMessage().contains("packet listeners are outside"), error.getMessage());
        assertTrue(error.getMessage().contains("26.3"), error.getMessage());
    }

    @Test
    void syncWriteRefusalNamesBothTheRefusedAndTheSupportedPath() {
        UnsupportedOperationException error = NmsUnsupported.operation(
                ServerLevel.class.getName(), "setBlock", "use setBlockAsync");
        assertTrue(error.getMessage().contains("ServerLevel#setBlock"), error.getMessage());
        assertTrue(error.getMessage().contains("setBlockAsync"), error.getMessage());
    }
}
