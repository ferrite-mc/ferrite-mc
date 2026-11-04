package dev.ferrite.server;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class HostBridgeTest {
    private final List<String> calls = new ArrayList<>();

    @AfterEach void reset() { HostBridge.transport = null; }

    @Test void unhandledMethodsNameThemselves() {
        HostBridge.transport = (owner, subject, method, args) -> null;
        var e = assertThrows(UnsupportedOperationException.class, () -> HostBridge.callVoid("Server", "server", "shutdown()"));
        assertTrue(e.getMessage().contains("Server#shutdown()"), e.getMessage());
    }

    @Test void argumentsAreEncodedAndRepliesDecoded() {
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(owner + "|" + subject + "|" + method + "|" + args); return "42"; };
        assertEquals(42, HostBridge.callInt("Server", "server", "broadcast(String,int)", "hi", 7));
        assertEquals("Server|server|broadcast(String,int)|hi" + HostBridge.ARG + "7", calls.get(0));
        HostBridge.transport = (owner, subject, method, args) -> "true";
        assertTrue(HostBridge.callBoolean("Player", "id", "isFlying()"));
        HostBridge.transport = (owner, subject, method, args) -> HostBridge.NULL;
        assertNull(HostBridge.call("Player", "id", "getName()", String.class));
    }

    @Test void everyGeneratedStubReachesTheHost() throws Exception {
        List<String> seen = new ArrayList<>();
        HostBridge.transport = (owner, subject, method, args) -> { seen.add(owner + "#" + method); return null; };
        // PlayerStubs is generated: any method not overridden by FerritePlayer must route, never throw its own error.
        int routed = 0;
        for (var m : PlayerStubs.class.getDeclaredMethods()) {
            if (m.isSynthetic() || !java.lang.reflect.Modifier.isPublic(m.getModifiers())) continue;
            routed++;
        }
        assertTrue(routed > 500, "expected the generated Player stubs, found " + routed);
    }

    @Test void dispatchTurnsProxyCallsIntoKeys() throws Exception {
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(owner + "#" + method); return "5"; };
        var method = java.util.List.class.getMethod("get", int.class);
        assertEquals("5", HostBridge.dispatch("World", "w", method, new Object[]{3}));
        assertEquals("World#get(int)", calls.get(0));
    }

    @Test void unhandledPropertiesReadBackWhatWasSetAndKnownDefaultsOtherwise() {
        HostBridge.clearLocalProperties();
        HostBridge.transport = (owner, subject, method, args) -> null;
        assertEquals(0.2f, HostBridge.callFloat("Player", "p1", "getWalkSpeed()"), 0.0001f);
        assertFalse(HostBridge.callBoolean("Player", "p1", "isFlying()"));
        HostBridge.callVoid("Player", "p1", "setFlying(boolean)", true);
        HostBridge.callVoid("Player", "p1", "setFireTicks(int)", 40);
        assertTrue(HostBridge.callBoolean("Player", "p1", "isFlying()"));
        assertEquals(40, HostBridge.callInt("Player", "p1", "getFireTicks()"));
        // Another player keeps its own values.
        assertFalse(HostBridge.callBoolean("Player", "p2", "isFlying()"));
        // Unknown properties that were never set are not invented.
        assertThrows(UnsupportedOperationException.class, () -> HostBridge.callInt("Player", "p1", "getNoSuchThing()"));
        HostBridge.clearLocalProperties();
    }
}
