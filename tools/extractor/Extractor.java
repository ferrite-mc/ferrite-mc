// Extracts the physical properties of every block state (and basic item data) from the official
// server jar, for the data the vanilla data generator does not report.
//
// This runs Mojang's own code locally; its generated output must not be committed.
// Build and run it against the deobfuscated 26.3 server jar and its libraries:
//
//     tools/extractor/build.sh 26.3
//
// or let `python tools/fetch_vanilla.py` do both. Output goes to
// vanilla/<version>/generated/ferrite/{block_states,items,entity_types}.json.

import java.io.IOException;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import java.util.stream.Stream;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.phys.shapes.BooleanOp;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.VanillaPackResources;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.BlockTags;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.entity.EntityAttachment;
import net.minecraft.world.entity.EntityDimensions;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.AttributeSupplier;
import net.minecraft.world.entity.ai.attributes.DefaultAttributes;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.FlowingFluid;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class Extractor {
    private static final VoxelShape FULL_BLOCK =
        Shapes.box(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    public static void main(String[] args) throws IOException {
        if (args.length < 1 || args.length > 2) {
            System.err.println("usage: Extractor <output-directory> [item-components-directory]");
            System.exit(2);
        }
        Path outDir = Path.of(args[0]);
        Files.createDirectories(outDir);

        // Bootstrap the registries exactly like the dedicated server does. Bootstrapping alone
        // leaves every tag empty, but the heightmap predicates are tag based, so read the
        // vanilla block tags from the server jar that is on the classpath.
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        loadBlockTags();

        int stateCount = Block.BLOCK_STATE_REGISTRY.size();
        StringBuilder states = new StringBuilder(1 << 24);
        states.append('[');
        for (int id = 0; id < stateCount; id++) {
            BlockState state = Block.BLOCK_STATE_REGISTRY.byId(id);
            if (state == null) {
                throw new IllegalStateException("block state registry has a gap at " + id);
            }
            if (id > 0) {
                states.append(",\n");
            }
            appendState(states, id, state);
        }
        states.append("\n]");

        StringBuilder blocks = new StringBuilder(1 << 20);
        List<Block> sortedBlocks = new ArrayList<>(BuiltInRegistries.BLOCK.stream().toList());
        sortedBlocks.sort(Comparator.comparing(block ->
            BuiltInRegistries.BLOCK.getKey(block).toString()));
        blocks.append('[');
        for (int i = 0; i < sortedBlocks.size(); i++) {
            Block block = sortedBlocks.get(i);
            if (i > 0) {
                blocks.append(",\n");
            }
            appendBlock(blocks, block);
        }
        blocks.append("\n]");

        // Item components (stack size, damage, rarity, food) come from the data generator's
        // per-item component report; binding them inside a bare JVM needs a loaded data pack.
        Map<String, JsonObject> components = args.length > 1
            ? readComponents(Path.of(args[1]))
            : new HashMap<>();
        StringBuilder items = new StringBuilder(1 << 20);
        List<Item> sortedItems = new ArrayList<>(BuiltInRegistries.ITEM.stream().toList());
        sortedItems.sort(Comparator.comparing(item ->
            BuiltInRegistries.ITEM.getKey(item).toString()));
        items.append('[');
        for (int i = 0; i < sortedItems.size(); i++) {
            Item item = sortedItems.get(i);
            if (i > 0) {
                items.append(",\n");
            }
            appendItem(items, item, components);
        }
        items.append("\n]");

        StringBuilder entities = new StringBuilder(1 << 16);
        int entityCount = appendEntityTypes(entities);

        StringBuilder root = new StringBuilder(states.length() + blocks.length() + items.length() + 128);
        root.append("{\"version\":");
        quote(root, SharedConstants.getCurrentVersion().name());
        root.append(",\"protocol\":").append(SharedConstants.getProtocolVersion());
        root.append(",\"stateCount\":").append(stateCount);
        root.append(",\"states\":").append(states);
        root.append(",\"blocks\":").append(blocks);
        root.append(",\"items\":").append(items);
        root.append("}\n");

        Path file = outDir.resolve("block_states.json");
        try (Writer writer = Files.newBufferedWriter(file, StandardCharsets.UTF_8)) {
            writer.write(root.toString());
        }
        System.out.printf(Locale.ROOT, "wrote %d block states, %d blocks, %d items to %s%n",
            stateCount, sortedBlocks.size(), sortedItems.size(), file);

        StringBuilder entityRoot = new StringBuilder(entities.length() + 128);
        entityRoot.append("{\"version\":");
        quote(entityRoot, SharedConstants.getCurrentVersion().name());
        entityRoot.append(",\"protocol\":").append(SharedConstants.getProtocolVersion());
        entityRoot.append(",\"count\":").append(entityCount);
        entityRoot.append(",\"types\":").append(entities);
        entityRoot.append("}\n");
        Path entityFile = outDir.resolve("entity_types.json");
        try (Writer writer = Files.newBufferedWriter(entityFile, StandardCharsets.UTF_8)) {
            writer.write(entityRoot.toString());
        }
        System.out.printf(Locale.ROOT, "wrote %d entity types to %s%n", entityCount, entityFile);
    }

    /**
     * The vanilla block tags, read from the server jar on the classpath.
     *
     * Bootstrapping freezes the registries before data packs bind tags. Load and apply
     * the vanilla tags before invoking the vanilla heightmap predicates.
     */
    @SuppressWarnings("unchecked")
    private static void loadBlockTags() throws IOException {
        VanillaPackResources vanilla = ServerPacksSource.createVanillaPackSource();
        TagLoader.ElementLookup<Holder<Block>> lookup =
            (TagLoader.ElementLookup<Holder<Block>>) (TagLoader.ElementLookup<?>)
                TagLoader.ElementLookup.fromFrozenRegistry(BuiltInRegistries.BLOCK);
        Map<TagKey<Block>, List<Holder<Block>>> loaded;
        try (MultiPackResourceManager resources = new MultiPackResourceManager(
            PackType.SERVER_DATA, List.of(vanilla.fullResources()))) {
            loaded = TagLoader.<Block>loadTagsForRegistry(
                resources, Registries.BLOCK, lookup);
        }
        BuiltInRegistries.BLOCK.prepareTagReload(new TagLoader.LoadResult<>(Registries.BLOCK, loaded)).apply();
    }

    // `isSolid()` (the blocks-motion predicate) is deprecated in 26.3 in favour of the
    // heightmap tags, but it is still the only API that reports motion blocking.
    @SuppressWarnings("deprecation")
    private static void appendState(
        StringBuilder out, int id, BlockState state
    ) {
        Block block = state.getBlock();
        BlockPos pos = BlockPos.ZERO;
        CollisionContext context = CollisionContext.empty();

        VoxelShape collision = state.getCollisionShape(EmptyBlockGetter.INSTANCE, pos, context);
        VoxelShape occlusion = state.getOcclusionShape();

        out.append("{\"id\":").append(id);
        out.append(",\"block\":");
        quote(out, BuiltInRegistries.BLOCK.getKey(block).toString());
        out.append(",\"lightEmission\":").append(state.getLightEmission());
        // 26.3 renamed the old `getLightBlock` to `getLightDampening`.
        out.append(",\"lightOpacity\":").append(state.getLightDampening());
        out.append(",\"canOcclude\":").append(state.canOcclude());
        out.append(",\"useShapeForLightOcclusion\":").append(state.useShapeForLightOcclusion());
        out.append(",\"propagatesSkylightDown\":").append(state.propagatesSkylightDown());
        out.append(",\"isAir\":").append(state.isAir());
        out.append(",\"blocksMotion\":").append(state.isSolid());
        out.append(",\"isSolidRender\":").append(state.isSolidRender());
        out.append(",\"isRedstoneConductor\":")
            .append(state.isRedstoneConductor(EmptyBlockGetter.INSTANCE, pos));
        // `BlockState.instrument()`: the note block sound of the block (`NoteBlock.setInstrument`).
        out.append(",\"instrument\":");
        quote(out, state.instrument().getSerializedName());

        FluidState fluid = state.getFluidState();
        out.append(",\"fluid\":");
        if (fluid.isEmpty()) {
            out.append("null");
        } else {
            out.append("{\"id\":");
            quote(out, BuiltInRegistries.FLUID.getKey(fluid.getType()).toString());
            out.append(",\"amount\":").append(fluid.getAmount());
            out.append(",\"falling\":").append(
                fluid.hasProperty(FlowingFluid.FALLING) && fluid.getValue(FlowingFluid.FALLING));
            out.append('}');
        }

        out.append(",\"hardness\":")
            .append(number(state.getDestroySpeed(EmptyBlockGetter.INSTANCE, pos)));
        out.append(",\"requiresCorrectTool\":").append(state.requiresCorrectToolForDrops());
        out.append(",\"replaceable\":").append(state.canBeReplaced());
        out.append(",\"collision\":");
        appendShape(out, collision);
        out.append(",\"occlusion\":");
        appendShape(out, occlusion);
        out.append(",\"faces\":[");
        Direction[] directions = {Direction.WEST, Direction.EAST, Direction.DOWN, Direction.UP, Direction.NORTH, Direction.SOUTH};
        for (int i = 0; i < directions.length; i++) {
            if (i > 0) out.append(',');
            appendShape(out, state.getFaceOcclusionShape(directions[i]));
        }
        out.append(']');

        int heightmaps = 0;
        if (Heightmap.Types.WORLD_SURFACE.isOpaque().test(state)) heightmaps |= 1;
        if (Heightmap.Types.OCEAN_FLOOR.isOpaque().test(state)) heightmaps |= 2;
        if (Heightmap.Types.MOTION_BLOCKING.isOpaque().test(state)) heightmaps |= 4;
        if (Heightmap.Types.MOTION_BLOCKING_NO_LEAVES.isOpaque().test(state)) heightmaps |= 8;
        out.append(",\"heightmaps\":").append(heightmaps);
        out.append(",\"randomlyTicking\":").append(state.isRandomlyTicking());
        out.append(",\"isSignalSource\":").append(state.isSignalSource());
        out.append(",\"hasAnalogOutput\":").append(state.hasAnalogOutputSignal());
        out.append(",\"friction\":").append(number(block.getFriction()));
        out.append(",\"speedFactor\":").append(number(block.getSpeedFactor()));
        out.append(",\"jumpFactor\":").append(number(block.getJumpFactor()));
        out.append('}');
    }

    /** Writes a shape as `"empty"`, `"full"` or `{"boxes":[[x1,y1,z1,x2,y2,z2],...]}`. */
    private static void appendShape(StringBuilder out, VoxelShape shape) {
        if (shape.isEmpty()) {
            out.append("\"empty\"");
            return;
        }
        if (!Shapes.joinIsNotEmpty(shape, FULL_BLOCK, BooleanOp.NOT_SAME)) {
            out.append("\"full\"");
            return;
        }
        out.append("{\"boxes\":[");
        List<AABB> boxes = shape.toAabbs();
        for (int i = 0; i < boxes.size(); i++) {
            AABB box = boxes.get(i);
            if (i > 0) {
                out.append(',');
            }
            out.append('[')
                .append(number(box.minX)).append(',')
                .append(number(box.minY)).append(',')
                .append(number(box.minZ)).append(',')
                .append(number(box.maxX)).append(',')
                .append(number(box.maxY)).append(',')
                .append(number(box.maxZ)).append(']');
        }
        out.append("]}");
    }

    private static void appendBlock(StringBuilder out, Block block) {
        out.append("{\"name\":");
        quote(out, BuiltInRegistries.BLOCK.getKey(block).toString());
        out.append(",\"defaultState\":")
            .append(Block.BLOCK_STATE_REGISTRY.getId(block.defaultBlockState()));
        out.append(",\"explosionResistance\":").append(number(block.getExplosionResistance()));
        Item item = block.asItem();
        String itemId = BuiltInRegistries.ITEM.getKey(item).toString();
        out.append(",\"item\":");
        if (itemId.equals("minecraft:air")) {
            out.append("null");
        } else {
            quote(out, itemId);
        }
        out.append('}');
    }

    /**
     * Writes every entity type in registry id order, with its dimensions (and the single-point
     * attachments vanilla defines), tracking settings and, for living entities, default
     * attribute values.
     */
    private static int appendEntityTypes(StringBuilder out) {
        List<EntityType<?>> types =
            new ArrayList<>(BuiltInRegistries.ENTITY_TYPE.stream().toList());
        types.sort(Comparator.comparingInt(type -> BuiltInRegistries.ENTITY_TYPE.getId(type)));
        out.append('[');
        for (int index = 0; index < types.size(); index++) {
            EntityType<?> type = types.get(index);
            int id = BuiltInRegistries.ENTITY_TYPE.getId(type);
            if (id != index) {
                throw new IllegalStateException("entity type registry has a gap at " + index);
            }
            if (index > 0) {
                out.append(",\n");
            }
            appendEntityType(out, id, type);
        }
        out.append("\n]");
        return types.size();
    }

    private static void appendEntityType(StringBuilder out, int id, EntityType<?> type) {
        EntityDimensions dimensions = type.getDimensions();
        out.append("{\"id\":").append(id);
        out.append(",\"key\":");
        quote(out, EntityType.getKey(type).toString());
        out.append(",\"category\":");
        quote(out, type.getCategory().getSerializedName());
        out.append(",\"width\":").append(number(dimensions.width()));
        out.append(",\"height\":").append(number(dimensions.height()));
        out.append(",\"eyeHeight\":").append(number(dimensions.eyeHeight()));
        out.append(",\"fixed\":").append(dimensions.fixed());
        out.append(",\"attachments\":");
        appendAttachments(out, dimensions);
        out.append(",\"clientTrackingRange\":").append(type.clientTrackingRange());
        out.append(",\"updateInterval\":").append(type.updateInterval());
        out.append(",\"fireImmune\":").append(type.fireImmune());
        out.append(",\"canSpawnFarFromPlayer\":").append(type.canSpawnFarFromPlayer());
        out.append(",\"isSummonable\":").append(type.canSummon());
        // 26.3 exposes one serialization flag; both spellings describe the same value.
        out.append(",\"isSerializable\":").append(type.canSerialize());
        out.append(",\"canBeSerialized\":").append(type.canSerialize());
        out.append(",\"attributes\":");
        appendAttributes(out, type);
        out.append('}');
    }

    /**
     * Single-point attachments as `attachment: [x, y, z]`. Multi-point attachments (the happy
     * ghast's four passenger seats) do not fit the simple point form and are skipped.
     */
    private static void appendAttachments(StringBuilder out, EntityDimensions dimensions) {
        out.append('{');
        boolean first = true;
        for (EntityAttachment attachment : EntityAttachment.values()) {
            List<Vec3> points = new ArrayList<>(1);
            for (int index = 0; ; index++) {
                Vec3 point = dimensions.attachments().getNullable(attachment, index, 0.0f);
                if (point == null) {
                    break;
                }
                points.add(point);
            }
            if (points.size() != 1) {
                continue;
            }
            if (!first) {
                out.append(',');
            }
            first = false;
            quote(out, attachment.name().toLowerCase(Locale.ROOT));
            Vec3 point = points.get(0);
            out.append(":[")
                .append(number((float) point.x)).append(',')
                .append(number((float) point.y)).append(',')
                .append(number((float) point.z)).append(']');
        }
        out.append('}');
    }

    /** Default attribute values (`attribute id -> base value`) of a living entity type. */
    @SuppressWarnings("unchecked")
    private static void appendAttributes(StringBuilder out, EntityType<?> type) {
        out.append('{');
        if (DefaultAttributes.hasSupplier(type)) {
            EntityType<? extends LivingEntity> living =
                (EntityType<? extends LivingEntity>) type;
            AttributeSupplier supplier = DefaultAttributes.getSupplier(living);
            TreeMap<String, Double> values = new TreeMap<>();
            for (int index = 0; index < BuiltInRegistries.ATTRIBUTE.size(); index++) {
                Attribute attribute = BuiltInRegistries.ATTRIBUTE.byId(index);
                Holder<Attribute> holder = BuiltInRegistries.ATTRIBUTE.wrapAsHolder(attribute);
                if (supplier.hasAttribute(holder)) {
                    values.put(
                        BuiltInRegistries.ATTRIBUTE.getKey(attribute).toString(),
                        supplier.getBaseValue(holder));
                }
            }
            boolean first = true;
            for (Map.Entry<String, Double> entry : values.entrySet()) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                quote(out, entry.getKey());
                out.append(':').append(number(entry.getValue()));
            }
        }
        out.append('}');
    }

    /** Reads `reports/minecraft/components/item/*.json` into `item id -> components`. */
    private static Map<String, JsonObject> readComponents(Path dir) throws IOException {
        Map<String, JsonObject> components = new HashMap<>();
        if (!Files.isDirectory(dir)) {
            System.out.println("no item component report at " + dir + ", writing stack sizes only");
            return components;
        }
        List<Path> files;
        try (Stream<Path> listing = Files.list(dir)) {
            files = listing
                .filter(file -> file.getFileName().toString().endsWith(".json"))
                .toList();
        }
        for (Path file : files) {
            String name = file.getFileName().toString();
            String id = "minecraft:" + name.substring(0, name.length() - ".json".length());
            JsonObject root = JsonParser.parseString(Files.readString(file)).getAsJsonObject();
            JsonObject value = root.getAsJsonObject("components");
            components.put(id, value == null ? new JsonObject() : value);
        }
        return components;
    }

    private static void appendItem(
        StringBuilder out, Item item, Map<String, JsonObject> componentsByName
    ) {
        String name = BuiltInRegistries.ITEM.getKey(item).toString();
        JsonObject components = componentsByName.getOrDefault(name, new JsonObject());
        out.append("{\"name\":");
        quote(out, name);

        JsonElement maxStackSize = primitive(components, "minecraft:max_stack_size");
        out.append(",\"maxStackSize\":").append(
            maxStackSize == null ? item.getDefaultMaxStackSize() : maxStackSize.getAsInt());
        JsonElement maxDamage = primitive(components, "minecraft:max_damage");
        out.append(",\"maxDamage\":").append(maxDamage == null ? 0 : maxDamage.getAsInt());

        out.append(",\"rarity\":");
        JsonElement rarity = primitive(components, "minecraft:rarity");
        quote(out, rarity == null ? "common" : rarity.getAsString().replace("minecraft:", ""));

        out.append(",\"food\":");
        JsonObject food = components.has("minecraft:food")
            ? components.getAsJsonObject("minecraft:food")
            : null;
        if (food == null) {
            out.append("null");
        } else {
            out.append("{\"nutrition\":").append(food.get("nutrition").getAsInt());
            out.append(",\"saturation\":").append(number(food.get("saturation").getAsFloat()));
            JsonElement canAlwaysEat = primitive(food, "can_always_eat");
            out.append(",\"canAlwaysEat\":")
                .append(canAlwaysEat != null && canAlwaysEat.getAsBoolean());
            out.append('}');
        }
        out.append('}');
    }

    /** A component value that is a JSON primitive, or null when it is absent. */
    private static JsonElement primitive(JsonObject components, String name) {
        return components.has(name) ? components.get(name) : null;
    }

    private static String number(double value) {
        return Double.toString(value);
    }

    /** Renders a float as a JSON number, keeping whole values short. */
    private static String number(float value) {
        if (value == (long) value) {
            return Long.toString((long) value);
        }
        return Float.toString(value);
    }

    private static void quote(StringBuilder out, String value) {
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char ch = value.charAt(i);
            switch (ch) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        out.append(String.format(Locale.ROOT, "\\u%04x", (int) ch));
                    } else {
                        out.append(ch);
                    }
                }
            }
        }
        out.append('"');
    }

    private Extractor() {
    }
}
