import java.io.*;
import java.lang.classfile.*;
import java.lang.classfile.constantpool.*;
import java.lang.constant.*;
import java.net.*;
import java.nio.file.*;
import java.util.*;
import java.util.jar.*;

/**
 * Compatibility scanner: lists every external class, method and field a plugin jar references and checks
 * each against the API + bridge classpath. Usage: java Scan.java plugin.jar classpath-entry...
 * Output groups gaps as bukkit-api, craftbukkit, nms (net.minecraft / com.mojang) and other.
 */
public class Scan {
  static String bucket(String n) {
    if (n.startsWith("org.bukkit.craftbukkit")) return "craftbukkit";
    if (n.startsWith("net.minecraft.") || n.startsWith("com.mojang.")) return "nms";
    if (n.startsWith("org.bukkit.") || n.startsWith("io.papermc.") || n.startsWith("com.destroystokyo.") || n.startsWith("net.kyori.") || n.startsWith("net.md_5.") || n.startsWith("org.spigotmc.")) return "bukkit-api";
    return "other";
  }
  static boolean jdk(String n) { return n.startsWith("java.") || n.startsWith("javax.") || n.startsWith("jdk.") || n.startsWith("sun.") || n.startsWith("com.sun.") || n.startsWith("org.w3c.") || n.startsWith("org.xml."); }
  static String cn(ClassEntry e) { String s = e.asInternalName(); while (s.startsWith("[")) s = s.substring(1); if (s.startsWith("L") && s.endsWith(";")) s = s.substring(1, s.length() - 1); return s.replace('/', '.'); }

  public static void main(String[] args) throws Exception {
    Path plugin = Path.of(args[0]);
    List<URL> urls = new ArrayList<>();
    for (int i = 1; i < args.length; i++) urls.add(Path.of(args[i]).toUri().toURL());
    Set<String> own = new HashSet<>();
    Map<String, byte[]> classes = new TreeMap<>();
    try (JarFile jar = new JarFile(plugin.toFile())) {
      for (var e = jar.entries(); e.hasMoreElements();) { JarEntry je = e.nextElement(); if (je.getName().endsWith(".class") && !je.getName().startsWith("META-INF/")) { String n = je.getName().substring(0, je.getName().length() - 6).replace('/', '.'); own.add(n); classes.put(n, jar.getInputStream(je).readAllBytes()); } }
    }
    URLClassLoader cl = new URLClassLoader(urls.toArray(new URL[0]), ClassLoader.getPlatformClassLoader());
    Map<String, Set<String>> missingClass = new TreeMap<>(), missingMember = new TreeMap<>();
    Map<String, Integer> seen = new TreeMap<>();
    Set<String> done = new HashSet<>();
    for (var entry : classes.entrySet()) {
      ClassModel cm = ClassFile.of().parse(entry.getValue());
      for (PoolEntry pe : cm.constantPool()) {
        String owner = null, member = null, mname = null, mdesc = null;
        if (pe instanceof ClassEntry ce) owner = cn(ce);
        else if (pe instanceof MemberRefEntry mr) { owner = cn(mr.owner()); mname = mr.name().stringValue(); mdesc = mr.type().stringValue(); member = mname + mdesc; }
        if (owner == null || own.contains(owner) || jdk(owner)) continue;
        String b = bucket(owner);
        String key = owner + (member == null ? "" : "#" + member);
        if (!done.add(key)) continue;
        seen.merge(b, 1, Integer::sum);
        Class<?> c;
        try { c = Class.forName(owner, false, cl); } catch (Throwable t) { missingClass.computeIfAbsent(b, k -> new TreeSet<>()).add(owner); continue; }
        if (member == null) continue;
        String name = mname;
        boolean method = mdesc.startsWith("(");
        if (!method) { boolean ok = false; try { c.getField(name); ok = true; } catch (Throwable t) { try { c.getDeclaredField(name); ok = true; } catch (Throwable t2) {} } if (!ok) missingMember.computeIfAbsent(b, k -> new TreeSet<>()).add(owner + "." + name); continue; }
        if (name.equals("<init>")) continue;
        MethodTypeDesc mt = MethodTypeDesc.ofDescriptor(mdesc);
        boolean ok = false;
        try {
          Deque<Class<?>> q = new ArrayDeque<>(); q.add(c); Set<Class<?>> vis = new HashSet<>();
          while (!q.isEmpty() && !ok) { Class<?> k = q.poll(); if (!vis.add(k)) continue;
            for (var m : k.getDeclaredMethods()) if (m.getName().equals(name) && m.getParameterCount() == mt.parameterCount()) { ok = true; break; }
            if (k.getSuperclass() != null) q.add(k.getSuperclass()); q.addAll(Arrays.asList(k.getInterfaces())); }
          if (!ok && c.isInterface()) for (var m : Object.class.getMethods()) if (m.getName().equals(name)) ok = true;
        } catch (Throwable t) { ok = true; }
        if (!ok) missingMember.computeIfAbsent(b, k -> new TreeSet<>()).add(owner + "#" + member);
      }
    }
    System.out.println("== " + plugin.getFileName() + ": " + own.size() + " own classes");
    for (String b : List.of("bukkit-api", "craftbukkit", "nms", "other")) {
      int total = seen.getOrDefault(b, 0), mc = missingClass.getOrDefault(b, Set.of()).size(), mm = missingMember.getOrDefault(b, Set.of()).size();
      System.out.printf("%-12s references=%d missing-classes=%d missing-members=%d%n", b, total, mc, mm);
    }
    boolean full = Arrays.asList(args).contains("--full");
    for (var e : missingClass.entrySet()) { if (e.getKey().equals("other") && !full) continue; System.out.println("-- missing classes [" + e.getKey() + "]"); e.getValue().stream().limit(full ? 100000 : 40).forEach(s -> System.out.println("   " + s)); }
    for (var e : missingMember.entrySet()) { if (e.getKey().equals("other") && !full) continue; System.out.println("-- missing members [" + e.getKey() + "]"); e.getValue().stream().limit(full ? 100000 : 40).forEach(s -> System.out.println("   " + s)); }
  }
}
