package dev.ferrite.server;

import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import org.junit.jupiter.api.Test;

/** Calls every World method with default arguments and records the ones that throw "not implemented". */
class WorldProbeTest {
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
    void probeWorld() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000f1", "probe", 0, 0, 64, 0);
        var world = org.bukkit.Bukkit.getWorld("probe");
        var missing = new TreeSet<String>();
        for (var m : org.bukkit.World.class.getMethods()) {
            if (Modifier.isStatic(m.getModifiers()) || m.isDefault()) continue;
            var types = m.getParameterTypes();
            var args = new Object[types.length];
            for (int i = 0; i < args.length; i++) args[i] = dflt(types[i]);
            var params = new StringJoiner(",");
            for (var p : types) params.add(p.getSimpleName());
            try {
                m.invoke(world, args);
            } catch (InvocationTargetException e) {
                var c = e.getCause();
                if (c instanceof UnsupportedOperationException u && String.valueOf(u.getMessage()).contains("not implemented"))
                    missing.add(m.getName() + "(" + params + ")");
            } catch (Throwable ignored) {}
        }
        ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
        var serverMissing = new TreeSet<String>();
        for (var m : org.bukkit.Server.class.getMethods()) {
            if (Modifier.isStatic(m.getModifiers()) || m.isDefault()) continue;
            var types = m.getParameterTypes();
            var args = new Object[types.length];
            for (int i = 0; i < args.length; i++) args[i] = dflt(types[i]);
            var params = new StringJoiner(",");
            for (var p : types) params.add(p.getSimpleName());
            if (m.getName().equals("shutdown") || m.getName().equals("restart") || m.getName().equals("stop")) continue;
            try {
                m.invoke(org.bukkit.Bukkit.getServer(), args);
            } catch (InvocationTargetException e) {
                var c = e.getCause();
                if (c instanceof UnsupportedOperationException u && String.valueOf(u.getMessage()).contains("not implemented"))
                    serverMissing.add(m.getName() + "(" + params + ")");
            } catch (Throwable ignored) {}
        }
        Files.createDirectories(Path.of("build/catalog"));
        Files.write(Path.of("build/catalog/server-missing.txt"), serverMissing);
        Path out = Path.of("build/catalog/world-missing.txt");
        Files.createDirectories(out.getParent());
        Files.write(out, missing);
    }
}
