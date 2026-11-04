package dev.ferrite.server;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import org.junit.jupiter.api.Test;

/** Calls every un-overridden Player stub with default arguments and records which ones throw "not implemented". */
class StubProbeTest {
    private static Object dflt(Class<?> t) {
        if (t == boolean.class) return false;
        if (t == int.class) return 0;
        if (t == long.class) return 0L;
        if (t == double.class) return 0.0;
        if (t == float.class) return 0f;
        if (t == short.class) return (short) 0;
        if (t == byte.class) return (byte) 0;
        if (t == char.class) return 'a';
        return null;
    }

    @Test
    void probePlayer() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerPlayer("00000000-0000-0000-0000-000000000001", "Probe", false);
        var player = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        player.setAccessible(true);
        var target = player.invoke(null, "00000000-0000-0000-0000-000000000001", "Probe");
        var missing = new TreeSet<String>();
        for (var m : target.getClass().getMethods()) {
            if (!m.getDeclaringClass().getSimpleName().endsWith("Stubs") || Modifier.isStatic(m.getModifiers())) continue;
            var args = new Object[m.getParameterCount()];
            var types = m.getParameterTypes();
            for (int i = 0; i < args.length; i++) args[i] = dflt(types[i]);
            var params = new StringJoiner(",");
            for (var p : types) params.add(p.getSimpleName());
            try {
                m.invoke(target, args);
            } catch (InvocationTargetException e) {
                var cause = e.getCause();
                if (cause instanceof UnsupportedOperationException u && String.valueOf(u.getMessage()).contains("not implemented"))
                    missing.add(m.getName() + "(" + params + ")");
            } catch (Throwable ignored) {}
        }
        Path out = Path.of("build/catalog/player-missing.txt");
        Files.createDirectories(out.getParent());
        Files.write(out, missing);
    }
}
