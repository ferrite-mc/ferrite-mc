package dev.ferrite.worldgen;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.lang.classfile.ClassFile;
import java.lang.classfile.ClassModel;
import java.lang.classfile.MethodModel;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import java.util.jar.JarOutputStream;
import java.util.zip.ZipEntry;

/**
 * Builds the patched server jar on the user's machine from the jar the user downloaded.
 *
 * <p>The only thing this class knows is how to replace a method body with a trivial one (return
 * nothing, or a constant). Which methods, and why, is listed in a rule file that names classes
 * and methods only; it contains no Minecraft code. The output never leaves the machine.
 *
 * <p>Rule file lines: {@code class method descriptor action}, with {@code #} comments. Class
 * names use slashes. Actions: {@code empty} (void), {@code false}, {@code true}, {@code null},
 * {@code int:N}. A rule whose target is missing aborts the build: a Minecraft update changed the
 * jar and the rule needs a human.
 *
 * <p>Arguments: {@code --in jar --rules file --out jar}.
 */
public final class Patcher {
    private record Rule(String owner, String name, String descriptor, String action, int line) {
        String key() {
            return owner + "." + name + descriptor;
        }
    }

    private Patcher() {}

    public static void main(String[] args) throws Exception {
        Path in = null;
        Path rules = null;
        Path out = null;
        for (int i = 0; i + 1 < args.length; i += 2) {
            switch (args[i]) {
                case "--in" -> in = Path.of(args[i + 1]);
                case "--rules" -> rules = Path.of(args[i + 1]);
                case "--out" -> out = Path.of(args[i + 1]);
                default -> throw new IllegalArgumentException("unknown argument " + args[i]);
            }
        }
        if (in == null || rules == null || out == null) {
            throw new IllegalArgumentException("--in, --rules and --out are required");
        }
        patch(in, parseRules(rules), out);
    }

    static List<Rule> parseRules(Path file) throws IOException {
        List<Rule> rules = new ArrayList<>();
        List<String> lines = Files.readAllLines(file);
        for (int i = 0; i < lines.size(); i++) {
            String text = lines.get(i);
            int hash = text.indexOf('#');
            if (hash >= 0) {
                text = text.substring(0, hash);
            }
            text = text.strip();
            if (text.isEmpty()) {
                continue;
            }
            String[] parts = text.split("\\s+");
            if (parts.length != 4) {
                throw new IllegalArgumentException(
                        file + ":" + (i + 1) + ": expected `class method descriptor action`");
            }
            rules.add(new Rule(parts[0], parts[1], parts[2], parts[3], i + 1));
        }
        return rules;
    }

    static void patch(Path in, List<Rule> rules, Path out) throws IOException {
        Map<String, List<Rule>> byClass = new HashMap<>();
        for (Rule rule : rules) {
            byClass.computeIfAbsent(rule.owner(), k -> new ArrayList<>()).add(rule);
        }
        Set<String> applied = new HashSet<>();
        Path parent = out.toAbsolutePath().getParent();
        Files.createDirectories(parent);
        Path temp = Files.createTempFile(parent, "patched", ".jar");
        try (JarFile jar = new JarFile(in.toFile());
                OutputStream file = Files.newOutputStream(temp);
                JarOutputStream sink = new JarOutputStream(file)) {
            var entries = jar.entries();
            while (entries.hasMoreElements()) {
                JarEntry entry = entries.nextElement();
                String name = entry.getName();
                // Signatures would no longer match a rewritten class.
                if (name.startsWith("META-INF/")
                        && (name.endsWith(".SF") || name.endsWith(".RSA") || name.endsWith(".DSA"))) {
                    continue;
                }
                byte[] data;
                try (InputStream stream = jar.getInputStream(entry)) {
                    data = stream.readAllBytes();
                }
                if (name.endsWith(".class")) {
                    String owner = name.substring(0, name.length() - ".class".length());
                    List<Rule> wanted = byClass.get(owner);
                    if (wanted != null) {
                        data = rewrite(data, wanted, applied);
                    }
                }
                JarEntry copy = new JarEntry(name);
                copy.setTime(entry.getTime());
                sink.putNextEntry(copy);
                sink.write(data);
                sink.closeEntry();
            }
        } catch (IOException | RuntimeException error) {
            Files.deleteIfExists(temp);
            throw error;
        }
        for (Rule rule : rules) {
            if (!applied.contains(rule.key())) {
                Files.deleteIfExists(temp);
                throw new IllegalStateException(
                        "rule on line " + rule.line() + " matches nothing: " + rule.key()
                                + " (did the Minecraft version change?)");
            }
        }
        Files.move(temp, out, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
    }

    private static byte[] rewrite(byte[] original, List<Rule> rules, Set<String> applied) {
        ClassFile classFile = ClassFile.of();
        ClassModel model = classFile.parse(original);
        return classFile.transformClass(
                model,
                (builder, element) -> {
                    if (element instanceof MethodModel method) {
                        for (Rule rule : rules) {
                            if (method.methodName().stringValue().equals(rule.name())
                                    && method.methodType().stringValue().equals(rule.descriptor())) {
                                applied.add(rule.key());
                                builder.withMethodBody(
                                        method.methodName(),
                                        method.methodType(),
                                        method.flags().flagsMask(),
                                        code -> emit(code, rule));
                                return;
                            }
                        }
                    }
                    builder.with(element);
                });
    }

    private static void emit(java.lang.classfile.CodeBuilder code, Rule rule) {
        String action = rule.action();
        String descriptor = rule.descriptor();
        char returns = descriptor.charAt(descriptor.indexOf(')') + 1);
        if (action.equals("empty")) {
            if (returns != 'V') {
                throw new IllegalArgumentException(rule.key() + ": `empty` needs a void method");
            }
            code.return_();
            return;
        }
        switch (action) {
            case "false" -> code.iconst_0();
            case "true" -> code.iconst_1();
            case "null" -> code.aconst_null();
            default -> {
                if (!action.startsWith("int:")) {
                    throw new IllegalArgumentException(
                            "rule on line " + rule.line() + ": unknown action " + action);
                }
                code.ldc(code.constantPool().intEntry(Integer.parseInt(action.substring(4))));
            }
        }
        switch (returns) {
            case 'Z', 'B', 'C', 'S', 'I' -> code.ireturn();
            case 'L', '[' -> code.areturn();
            default -> throw new IllegalArgumentException(rule.key() + ": unsupported return type");
        }
    }
}
