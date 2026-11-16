// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.test.FakeNmsHost;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class NmsHostBindingTest {
    private FakeNmsHost host;

    @BeforeEach
    void installFakeHost() {
        host = new FakeNmsHost();
        NmsHostBinding.install(host);
    }

    @AfterEach
    void removeFakeHost() {
        try {
            NmsHostBinding.uninstall(host);
        } catch (IllegalStateException ignored) {
            // A test may have uninstalled it deliberately.
        }
        host.close();
    }

    @Test
    void installsAndRequiresTheAttachedHost() {
        FakeNmsHost replacement = new FakeNmsHost();
        assertSame(host, NmsHostBinding.require());
        assertSame(host, NmsHostBinding.install(replacement));
        assertSame(replacement, NmsHostBinding.require());
        NmsHostBinding.uninstall(replacement);
        NmsHostBinding.install(host);
        replacement.close();
    }

    @Test
    void rejectsAHostWithAnotherVersionAndKeepsTheCurrentOne() {
        FakeNmsHost wrong = new FakeNmsHost();
        wrong.version = new dev.ferrite.nms.host.HostVersion("26.2", 776, 5000);
        NmsVersionMismatchException error =
                assertThrows(NmsVersionMismatchException.class, () -> NmsHostBinding.install(wrong));
        assertTrue(error.getMessage().contains("26.2"), error.getMessage());
        assertSame(host, NmsHostBinding.require());
        wrong.close();
    }

    @Test
    void uninstallRejectsTheWrongOwner() {
        FakeNmsHost other = new FakeNmsHost();
        assertThrows(IllegalStateException.class, () -> NmsHostBinding.uninstall(other));
        other.close();
    }

    @Test
    void requireWithoutAttachmentNamesTheMissingBridge() {
        NmsHostBinding.uninstall(host);
        IllegalStateException error =
                assertThrows(IllegalStateException.class, NmsHostBinding::require);
        assertTrue(error.getMessage().contains("HostBridge"), error.getMessage());
        assertTrue(error.getMessage().contains("not attached"), error.getMessage());
    }
}
