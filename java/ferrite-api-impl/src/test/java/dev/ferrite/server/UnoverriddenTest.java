package dev.ferrite.server;

import java.nio.file.*;
import java.util.*;
import org.junit.jupiter.api.Test;

/** Writes the stub methods no Ferrite class overrides (build/catalog/*-unoverridden.txt). */
class UnoverriddenTest {
    private static void dump(Class<?> type, String file) throws Exception {
        var lines = new TreeSet<String>();
        for (var m : type.getMethods()) {
            if (!m.getDeclaringClass().getSimpleName().endsWith("Stubs")) continue;
            if (java.lang.reflect.Modifier.isStatic(m.getModifiers())) continue;
            var params = new StringJoiner(",");
            for (var p : m.getParameterTypes()) params.add(p.getSimpleName());
            lines.add(m.getName() + "(" + params + ")");
        }
        Path out = Path.of("build/catalog/" + file);
        Files.createDirectories(out.getParent());
        Files.write(out, lines);
    }

    @Test
    void dumpUnoverridden() throws Exception {
        dump(FerritePlayer.class, "player-unoverridden.txt");
        dump(FerriteServer.class, "server-unoverridden.txt");
        dump(FerriteWorld.class, "world-unoverridden.txt");
    }
}
