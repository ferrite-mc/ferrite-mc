package dev.ferrite.server;

import java.io.IOException;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.jar.JarEntry;
import java.util.jar.JarOutputStream;
import javax.tools.SimpleJavaFileObject;
import javax.tools.ToolProvider;

/** Compiles plugin sources and packs them with their descriptor into a jar a real class loader can load. */
final class PluginFixture {
    private PluginFixture() {}

    private static final class Source extends SimpleJavaFileObject {
        private final String code;
        Source(String className, String code) {
            super(URI.create("string:///" + className.replace('.', '/') + ".java"), Kind.SOURCE);
            this.code = code;
        }
        @Override public CharSequence getCharContent(boolean ignoreEncodingErrors) { return code; }
    }

    /**
     * Builds {@code name.jar} in {@code dir}. {@code sources} maps a class name to its Java source, {@code resources}
     * maps a jar path ({@code plugin.yml}, {@code paper-plugin.yml}) to its text.
     */
    static Path jar(Path dir, String name, Map<String, String> sources, Map<String, String> resources) throws IOException {
        var compiler = ToolProvider.getSystemJavaCompiler();
        if (compiler == null) throw new IllegalStateException("the integration tests need a JDK, not a JRE");
        var classes = Files.createDirectories(dir.resolve(name + "-classes"));
        var units = new ArrayList<Source>();
        sources.forEach((className, code) -> units.add(new Source(className, code)));
        var diagnostics = new javax.tools.DiagnosticCollector<javax.tools.JavaFileObject>();
        var options = List.of("-classpath", System.getProperty("test.compile.classpath"), "-d", classes.toString(), "-proc:none", "-nowarn");
        var task = compiler.getTask(null, null, diagnostics, options, null, units);
        if (!task.call()) throw new IllegalStateException("fixture plugin " + name + " does not compile: " + diagnostics.getDiagnostics());
        var jar = dir.resolve(name + ".jar");
        try (var out = new JarOutputStream(Files.newOutputStream(jar)); var walk = Files.walk(classes)) {
            for (var file : (Iterable<Path>) walk.filter(Files::isRegularFile)::iterator) {
                out.putNextEntry(new JarEntry(classes.relativize(file).toString().replace('\\', '/')));
                out.write(Files.readAllBytes(file));
                out.closeEntry();
            }
            for (var entry : resources.entrySet()) {
                out.putNextEntry(new JarEntry(entry.getKey()));
                out.write(entry.getValue().getBytes(java.nio.charset.StandardCharsets.UTF_8));
                out.closeEntry();
            }
        }
        return jar;
    }
}
