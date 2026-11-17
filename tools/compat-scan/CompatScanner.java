import java.io.*;
import java.lang.classfile.*;
import java.lang.classfile.constantpool.*;
import java.lang.constant.*;
import java.lang.reflect.*;
import java.net.*;
import java.nio.file.*;
import java.util.*;
import java.util.jar.*;

/**
 * Reproducible static compatibility scanner for third-party plugin jars against Ferrite's
 * built runtime classpath (`ferrite-host`, `ferrite-api-impl`, `ferrite-nms`, and Paper API deps).
 *
 * Usage: java tools/compat-scan/CompatScanner.java <plugin.jar> <classpath-entry>...
 */
public class CompatScanner {

  static String bucket(String name) {
    if (name.startsWith("org.bukkit.craftbukkit.") || name.equals("org.bukkit.craftbukkit")) {
      return "nms";
    }
    if (name.startsWith("net.minecraft.") || name.startsWith("com.mojang.")) {
      return "nms";
    }
    if (name.startsWith("io.papermc.")
        || name.startsWith("com.destroystokyo.")
        || name.startsWith("net.kyori.")) {
      return "paper-api";
    }
    if (name.startsWith("org.bukkit.")
        || name.startsWith("org.spigotmc.")
        || name.startsWith("net.md_5.")) {
      return "bukkit-api";
    }
    return "other";
  }

  static boolean isJdk(String name) {
    return name.startsWith("java.")
        || name.startsWith("javax.")
        || name.startsWith("jdk.")
        || name.startsWith("sun.")
        || name.startsWith("com.sun.")
        || name.startsWith("org.w3c.")
        || name.startsWith("org.xml.")
        || name.startsWith("org.ietf.")
        || name.startsWith("org.omg.");
  }

  static String className(ClassEntry entry) {
    String s = entry.asInternalName();
    while (s.startsWith("[")) {
      s = s.substring(1);
    }
    if (s.startsWith("L") && s.endsWith(";")) {
      s = s.substring(1, s.length() - 1);
    }
    return s.replace('/', '.');
  }

  static boolean isPrimitiveOrArrayMarker(String name) {
    return name.length() == 1 && "BCDFIJSZV".contains(name);
  }

  static String packageOf(String className) {
    int dot = className.lastIndexOf('.');
    return dot < 0 ? "<default>" : className.substring(0, dot);
  }

  static String descriptorOf(Method m) {
    StringBuilder sb = new StringBuilder("(");
    for (Class<?> p : m.getParameterTypes()) {
      sb.append(p.descriptorString());
    }
    sb.append(')').append(m.getReturnType().descriptorString());
    return sb.toString();
  }

  public static void main(String[] args) throws Exception {
    if (args.length < 2) {
      System.err.println("Usage: java CompatScanner.java <plugin.jar> <classpath-entry>...");
      System.exit(2);
    }

    Path plugin = Path.of(args[0]);
    List<URL> urls = new ArrayList<>();
    for (int i = 1; i < args.length; i++) {
      urls.add(Path.of(args[i]).toUri().toURL());
    }

    Set<String> own = new HashSet<>();
    Map<String, byte[]> classes = new TreeMap<>();
    try (JarFile jar = new JarFile(plugin.toFile())) {
      for (var e = jar.entries(); e.hasMoreElements(); ) {
        JarEntry je = e.nextElement();
        if (je.getName().endsWith(".class") && !je.getName().startsWith("META-INF/")) {
          String n = je.getName().substring(0, je.getName().length() - 6).replace('/', '.');
          own.add(n);
          classes.put(n, jar.getInputStream(je).readAllBytes());
        }
      }
    }

    try (URLClassLoader cl = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getPlatformClassLoader())) {
      Map<String, Set<String>> missingClass = new TreeMap<>();
      Map<String, Set<String>> missingMethod = new TreeMap<>();
      Map<String, Set<String>> missingField = new TreeMap<>();
      Map<String, Integer> seen = new TreeMap<>();
      Map<String, Map<String, Integer>> pkgGaps = new TreeMap<>();
      Set<String> done = new HashSet<>();

      for (var entry : classes.entrySet()) {
        ClassModel cm = ClassFile.of().parse(entry.getValue());
        for (PoolEntry pe : cm.constantPool()) {
          String owner = null, member = null, mname = null, mdesc = null;
          if (pe instanceof ClassEntry ce) {
            owner = className(ce);
          } else if (pe instanceof MemberRefEntry mr) {
            owner = className(mr.owner());
            mname = mr.name().stringValue();
            mdesc = mr.type().stringValue();
            member = mname + mdesc;
          }
          if (owner == null || isPrimitiveOrArrayMarker(owner) || own.contains(owner) || isJdk(owner)) {
            continue;
          }
          String b = bucket(owner);
          String key = owner + (member == null ? "" : "#" + member);
          if (!done.add(key)) {
            continue;
          }
          seen.merge(b, 1, Integer::sum);

          Class<?> c;
          try {
            c = Class.forName(owner, false, cl);
          } catch (Throwable t) {
            missingClass.computeIfAbsent(b, k -> new TreeSet<>()).add(owner);
            pkgGaps.computeIfAbsent(b, k -> new TreeMap<>()).merge(packageOf(owner), 1, Integer::sum);
            continue;
          }

          if (member == null) {
            continue;
          }
          boolean isMethod = mdesc.startsWith("(");
          if (!isMethod) {
            boolean ok = false;
            try {
              c.getField(mname);
              ok = true;
            } catch (Throwable t) {
              try {
                c.getDeclaredField(mname);
                ok = true;
              } catch (Throwable ignored) {
              }
            }
            if (!ok) {
              missingField.computeIfAbsent(b, k -> new TreeSet<>()).add(owner + "." + mname);
              pkgGaps.computeIfAbsent(b, k -> new TreeMap<>()).merge(packageOf(owner), 1, Integer::sum);
            }
            continue;
          }

          if (mname.equals("<init>") || mname.equals("<clinit>")) {
            continue;
          }

          boolean ok = false;
          try {
            Deque<Class<?>> q = new ArrayDeque<>();
            q.add(c);
            Set<Class<?>> vis = new HashSet<>();
            while (!q.isEmpty() && !ok) {
              Class<?> k = q.poll();
              if (!vis.add(k)) {
                continue;
              }
              for (Method m : k.getDeclaredMethods()) {
                if (m.getName().equals(mname) && descriptorOf(m).equals(mdesc)) {
                  ok = true;
                  break;
                }
              }
              if (k.getSuperclass() != null) {
                q.add(k.getSuperclass());
              }
              q.addAll(Arrays.asList(k.getInterfaces()));
            }
            if (!ok && c.isInterface()) {
              for (Method m : Object.class.getMethods()) {
                if (m.getName().equals(mname) && descriptorOf(m).equals(mdesc)) {
                  ok = true;
                  break;
                }
              }
            }
          } catch (Throwable t) {
            ok = true;
          }

          if (!ok) {
            missingMethod.computeIfAbsent(b, k -> new TreeSet<>()).add(owner + "#" + member);
            pkgGaps.computeIfAbsent(b, k -> new TreeMap<>()).merge(packageOf(owner), 1, Integer::sum);
          }
        }
      }

      System.out.println("== " + plugin.getFileName() + ": own_classes=" + own.size());
      for (String b : List.of("bukkit-api", "paper-api", "nms", "other")) {
        int total = seen.getOrDefault(b, 0);
        int mc = missingClass.getOrDefault(b, Set.of()).size();
        int mm = missingMethod.getOrDefault(b, Set.of()).size();
        int mf = missingField.getOrDefault(b, Set.of()).size();
        System.out.printf(
            "BUCKET %-10s refs=%-6d missing_classes=%-5d missing_methods=%-5d missing_fields=%-4d%n",
            b, total, mc, mm, mf);
      }

      for (String b : List.of("bukkit-api", "paper-api", "nms", "other")) {
        Map<String, Integer> pkgs = pkgGaps.getOrDefault(b, Map.of());
        if (!pkgs.isEmpty()) {
          System.out.println("-- top packages [" + b + "]");
          pkgs.entrySet().stream()
              .sorted((x, y) -> Integer.compare(y.getValue(), x.getValue()))
              .limit(10)
              .forEach(e -> System.out.println("   " + e.getKey() + " (" + e.getValue() + ")"));
        }
        Set<String> mc = missingClass.getOrDefault(b, Set.of());
        if (!mc.isEmpty()) {
          System.out.println("-- sample missing classes [" + b + "]");
          mc.stream().limit(15).forEach(s -> System.out.println("   " + s));
        }
        Set<String> mm = missingMethod.getOrDefault(b, Set.of());
        if (!mm.isEmpty()) {
          System.out.println("-- sample missing methods [" + b + "]");
          mm.stream().limit(15).forEach(s -> System.out.println("   " + s));
        }
      }
    }
  }
}
