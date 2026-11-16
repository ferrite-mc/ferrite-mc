package dev.ferrite.api.item;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import org.bukkit.Material;
import org.bukkit.Tag;
import org.bukkit.inventory.meta.components.ToolComponent;

/**
 * Facade implementation of {@link ToolComponent} storing defaultMiningSpeed, damagePerBlock
 * and tool rules with defensive copying and serialization.
 */
public final class FerriteToolComponent implements ToolComponent {

    private float defaultMiningSpeed = 1.0F;
    private int damagePerBlock = 1;
    private final List<ToolRule> rules = new ArrayList<>();

    public FerriteToolComponent() {
    }

    public FerriteToolComponent(ToolComponent source) {
        Objects.requireNonNull(source, "source");
        setDefaultMiningSpeed(source.getDefaultMiningSpeed());
        setDamagePerBlock(source.getDamagePerBlock());
        setRules(source.getRules());
    }

    @Override
    public float getDefaultMiningSpeed() {
        return defaultMiningSpeed;
    }

    @Override
    public void setDefaultMiningSpeed(float speed) {
        if (Float.isNaN(speed) || Float.isInfinite(speed) || speed < 0.0F) {
            throw new IllegalArgumentException(
                    "defaultMiningSpeed must be finite and >= 0, was " + speed);
        }
        this.defaultMiningSpeed = speed;
    }

    @Override
    public int getDamagePerBlock() {
        return damagePerBlock;
    }

    @Override
    public void setDamagePerBlock(int damage) {
        if (damage < 0) {
            throw new IllegalArgumentException("damagePerBlock must be >= 0, was " + damage);
        }
        this.damagePerBlock = damage;
    }

    @Override
    public List<ToolRule> getRules() {
        List<ToolRule> copy = new ArrayList<>(rules.size());
        for (ToolRule rule : rules) {
            copy.add(new FerriteToolRule(rule));
        }
        return Collections.unmodifiableList(copy);
    }

    @Override
    public void setRules(List<ToolRule> rules) {
        Objects.requireNonNull(rules, "rules");
        List<ToolRule> copy = new ArrayList<>(rules.size());
        for (ToolRule rule : rules) {
            copy.add(new FerriteToolRule(Objects.requireNonNull(rule, "rule")));
        }
        this.rules.clear();
        this.rules.addAll(copy);
    }

    @Override
    public ToolRule addRule(Material block, Float speed, Boolean correctForDrops) {
        Objects.requireNonNull(block, "block");
        FerriteToolRule rule = new FerriteToolRule(Set.of(block), speed, correctForDrops);
        this.rules.add(rule);
        return rule;
    }

    @Override
    public ToolRule addRule(Collection<Material> blocks, Float speed, Boolean correctForDrops) {
        Objects.requireNonNull(blocks, "blocks");
        FerriteToolRule rule = new FerriteToolRule(blocks, speed, correctForDrops);
        this.rules.add(rule);
        return rule;
    }

    @Override
    public ToolRule addRule(Tag<Material> tag, Float speed, Boolean correctForDrops) {
        Objects.requireNonNull(tag, "tag");
        FerriteToolRule rule = new FerriteToolRule(tag.getValues(), speed, correctForDrops);
        this.rules.add(rule);
        return rule;
    }

    @Override
    public boolean removeRule(ToolRule rule) {
        Objects.requireNonNull(rule, "rule");
        return this.rules.remove(rule);
    }

    public FerriteToolComponent copy() {
        return new FerriteToolComponent(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("default-mining-speed", defaultMiningSpeed);
        result.put("damage-per-block", damagePerBlock);
        if (!rules.isEmpty()) {
            List<Map<String, Object>> encodedRules = new ArrayList<>(rules.size());
            for (ToolRule rule : rules) {
                encodedRules.add(rule.serialize());
            }
            result.put("rules", encodedRules);
        }
        return result;
    }

    public static FerriteToolComponent deserialize(Map<?, ?> args) {
        Objects.requireNonNull(args, "args");
        FerriteToolComponent component = new FerriteToolComponent();
        Object speed = args.get("default-mining-speed");
        if (speed instanceof Number number) {
            component.setDefaultMiningSpeed(number.floatValue());
        }
        Object damage = args.get("damage-per-block");
        if (damage instanceof Number number) {
            component.setDamagePerBlock(number.intValue());
        }
        Object rawRules = args.get("rules");
        if (rawRules instanceof List<?> list) {
            List<ToolRule> decoded = new ArrayList<>(list.size());
            for (Object item : list) {
                if (!(item instanceof Map<?, ?> map)) {
                    throw new IllegalArgumentException("Expected map entry in tool rules");
                }
                decoded.add(FerriteToolRule.deserialize(map));
            }
            component.setRules(decoded);
        }
        return component;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteToolComponent other)) {
            return false;
        }
        return Float.compare(defaultMiningSpeed, other.defaultMiningSpeed) == 0
                && damagePerBlock == other.damagePerBlock
                && Objects.equals(rules, other.rules);
    }

    @Override
    public int hashCode() {
        return Objects.hash(defaultMiningSpeed, damagePerBlock, rules);
    }

    @Override
    public String toString() {
        return "FerriteToolComponent{defaultMiningSpeed="
                + defaultMiningSpeed
                + ", damagePerBlock="
                + damagePerBlock
                + ", rules="
                + rules
                + "}";
    }

    /** Facade implementation of {@link ToolRule}. */
    public static final class FerriteToolRule implements ToolRule {

        private final Set<Material> blocks = new LinkedHashSet<>();
        private Float speed;
        private Boolean correctForDrops;

        public FerriteToolRule(
                Collection<Material> blocks, Float speed, Boolean correctForDrops) {
            setBlocks(blocks);
            setSpeed(speed);
            setCorrectForDrops(correctForDrops);
        }

        public FerriteToolRule(ToolRule source) {
            Objects.requireNonNull(source, "source");
            setBlocks(source.getBlocks());
            setSpeed(source.getSpeed());
            setCorrectForDrops(source.isCorrectForDrops());
        }

        @Override
        public Collection<Material> getBlocks() {
            return Collections.unmodifiableSet(new LinkedHashSet<>(blocks));
        }

        @Override
        public void setBlocks(Material block) {
            Objects.requireNonNull(block, "block");
            this.blocks.clear();
            this.blocks.add(block);
        }

        @Override
        public void setBlocks(Collection<Material> blocks) {
            Objects.requireNonNull(blocks, "blocks");
            Set<Material> copy = new LinkedHashSet<>(blocks.size());
            for (Material material : blocks) {
                copy.add(Objects.requireNonNull(material, "block material"));
            }
            this.blocks.clear();
            this.blocks.addAll(copy);
        }

        @Override
        public void setBlocks(Tag<Material> tag) {
            Objects.requireNonNull(tag, "tag");
            setBlocks(tag.getValues());
        }

        @Override
        public Float getSpeed() {
            return speed;
        }

        @Override
        public void setSpeed(Float speed) {
            if (speed != null
                    && (Float.isNaN(speed) || Float.isInfinite(speed) || speed <= 0.0F)) {
                throw new IllegalArgumentException(
                        "tool rule speed must be finite and > 0, was " + speed);
            }
            this.speed = speed;
        }

        @Override
        public Boolean isCorrectForDrops() {
            return correctForDrops;
        }

        @Override
        public void setCorrectForDrops(Boolean correct) {
            this.correctForDrops = correct;
        }

        @Override
        public Map<String, Object> serialize() {
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("blocks", blocks.stream().map(Material::name).toList());
            if (speed != null) {
                result.put("speed", speed);
            }
            if (correctForDrops != null) {
                result.put("correct-for-drops", correctForDrops);
            }
            return result;
        }

        public static FerriteToolRule deserialize(Map<?, ?> args) {
            Objects.requireNonNull(args, "args");
            Set<Material> mats = new LinkedHashSet<>();
            if (args.get("blocks") instanceof Collection<?> list) {
                for (Object entry : list) {
                    Material mat = Material.matchMaterial(String.valueOf(entry));
                    if (mat == null) {
                        throw new IllegalArgumentException("Unknown block material: " + entry);
                    }
                    mats.add(mat);
                }
            }
            Float speed = args.get("speed") instanceof Number n ? n.floatValue() : null;
            Boolean correct =
                    args.get("correct-for-drops") instanceof Boolean b ? b : null;
            return new FerriteToolRule(mats, speed, correct);
        }

        @Override
        public boolean equals(Object object) {
            if (this == object) {
                return true;
            }
            if (!(object instanceof FerriteToolRule other)) {
                return false;
            }
            return Objects.equals(blocks, other.blocks)
                    && Objects.equals(speed, other.speed)
                    && Objects.equals(correctForDrops, other.correctForDrops);
        }

        @Override
        public int hashCode() {
            return Objects.hash(blocks, speed, correctForDrops);
        }
    }
}
