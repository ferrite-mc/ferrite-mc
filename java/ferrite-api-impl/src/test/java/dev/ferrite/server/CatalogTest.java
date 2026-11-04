package dev.ferrite.server;

import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.TreeSet;
import org.junit.jupiter.api.Test;

/** Writes the methods that only exist as routed stubs, i.e. the ones the host must answer. */
class CatalogTest {
    private static TreeSet<String> stubs(Class<?> impl, Class<?> stubs, String owner) {
        var keys = new TreeSet<String>();
        for (Method m : impl.getMethods()) {
            if (m.getDeclaringClass() != stubs || Modifier.isStatic(m.getModifiers())) continue;
            keys.add(owner + "#" + m.getName() + "(" + String.join(",", Arrays.stream(m.getParameterTypes()).map(Class::getSimpleName).toList()) + ")");
        }
        return keys;
    }

    @Test
    void writesStubCatalog() throws Exception {
        var all = new TreeSet<String>();
        all.addAll(stubs(FerritePlayer.class, PlayerStubs.class, "Player"));
        all.addAll(stubs(FerriteServer.class, ServerStubs.class, "Server"));
        Path out = Path.of("build/catalog/stubs.txt");
        Files.createDirectories(out.getParent());
        Files.write(out, all);
        System.out.println("CATALOG stub methods: " + all.size());
    }
}
