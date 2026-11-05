// Encodes the vanilla advancement tree with the server's own network codecs, locally from the
// user's server jar and generated data. Output is gitignored.
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.*;
import java.util.stream.Stream;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.*;
import com.mojang.serialization.Lifecycle;
import net.minecraft.core.Holder;
import net.minecraft.core.Registry;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.RegistrationInfo;
import net.minecraft.resources.ResourceKey;
import net.minecraft.core.RegistryAccess;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundUpdateAdvancementsPacket.PositionedAdvancement;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;

public final class AdvancementExtractor {
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static Registry<?> copy(HolderLookup.Provider lookup, ResourceKey key) {
        MappedRegistry registry = new MappedRegistry(key, Lifecycle.stable());
        List<Holder.Reference> all = new ArrayList<>();
        HolderLookup.RegistryLookup source = lookup.lookupOrThrow(key);
        source.listElements().forEach(h -> all.add((Holder.Reference) h));
        all.sort(Comparator.comparing(h -> h.key().identifier().toString()));
        for (Holder.Reference h : all) {
            try {
                registry.register(h.key(), h.value(), RegistrationInfo.BUILT_IN);
            } catch (IllegalStateException duplicate) {
                // processor-style registries hold equal values; none of them is used by an icon
            }
        }
        return registry;
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 2) throw new IllegalArgumentException("expected data dir and output file");
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        RegistryAccess access = RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY);
        HolderLookup.Provider lookup = VanillaRegistries.createWorldLookup();
        List<Registry<?>> dynamic = new ArrayList<>();
        BuiltInRegistries.REGISTRY.forEach(dynamic::add);
        lookup.listRegistryKeys().forEach(key -> {
            if (BuiltInRegistries.REGISTRY.containsKey(key.identifier())) return;
            dynamic.add(copy(lookup, key));
        });
        RegistryAccess encoding = new RegistryAccess.ImmutableRegistryAccess(dynamic);
        RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, lookup);
        Path root = Path.of(args[0]).resolve("minecraft/advancement");
        TreeMap<String, JsonObject> files = new TreeMap<>();
        try (Stream<Path> walk = Files.walk(root)) {
            for (Path p : (Iterable<Path>) walk.filter(f -> f.toString().endsWith(".json"))::iterator) {
                String rel = root.relativize(p).toString().replace('\\', '/');
                files.put("minecraft:" + rel.substring(0, rel.length() - 5),
                        JsonParser.parseString(Files.readString(p)).getAsJsonObject());
            }
        }
        List<AdvancementHolder> holders = new ArrayList<>();
        Map<String, List<List<String>>> reqs = new HashMap<>();
        for (var e : files.entrySet()) {
            JsonObject json = e.getValue();
            if (!json.has("display")) continue;
            var display = DisplayInfo.CODEC.parse(ops, json.get("display")).getOrThrow();
            List<String> names = new ArrayList<>(json.getAsJsonObject("criteria").keySet());
            AdvancementRequirements requirements;
            if (json.has("requirements")) {
                requirements = AdvancementRequirements.CODEC.parse(ops, json.get("requirements")).getOrThrow();
            } else {
                requirements = AdvancementRequirements.allOf(names);
            }
            Optional<Identifier> parent = json.has("parent")
                    ? Optional.of(Identifier.parse(json.get("parent").getAsString())) : Optional.empty();
            Advancement adv = new Advancement(parent, Optional.of(display), AdvancementRewards.EMPTY,
                    Map.of(), requirements, false);
            holders.add(new AdvancementHolder(Identifier.parse(e.getKey()), adv));
            reqs.put(e.getKey(), requirements.requirements());
        }
        AdvancementTree tree = new AdvancementTree();
        tree.addAll(holders);
        for (AdvancementNode root0 : tree.roots()) TreeNodePosition.run(root0);
        StringBuilder out = new StringBuilder("[\n");
        boolean first = true;
        for (AdvancementNode node : tree.nodes()) {
            if (node.advancement().display().isEmpty()) continue;
            ByteBuf buf = Unpooled.buffer();
            PositionedAdvancement.STREAM_CODEC.encode(new RegistryFriendlyByteBuf(buf, encoding), PositionedAdvancement.fromNode(node));
            byte[] bytes = new byte[buf.readableBytes()];
            buf.readBytes(bytes);
            StringBuilder hex = new StringBuilder();
            for (byte b : bytes) hex.append(String.format("%02x", b));
            String id = node.holder().id().toString();
            if (!first) out.append(",\n");
            first = false;
            out.append("{\"id\":\"").append(id).append("\",\"parent\":");
            out.append(node.parent() == null ? "null" : "\"" + node.parent().holder().id() + "\"");
            out.append(",\"hidden\":").append(node.advancement().display().get().hidden());
            out.append(",\"requirements\":[");
            boolean f1 = true;
            for (List<String> group : reqs.get(id)) {
                if (!f1) out.append(',');
                f1 = false;
                out.append('[');
                boolean f2 = true;
                for (String n : group) {
                    if (!f2) out.append(',');
                    f2 = false;
                    out.append('"').append(n).append('"');
                }
                out.append(']');
            }
            out.append("],\"wire\":\"").append(hex).append("\"}");
        }
        out.append("\n]\n");
        Files.writeString(Path.of(args[1]), out, StandardCharsets.UTF_8);
    }
}
