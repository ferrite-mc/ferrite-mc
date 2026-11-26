// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import com.mojang.datafixers.util.Pair;
import java.math.BigDecimal;
import java.util.List;
import java.util.Map;
import java.util.stream.Stream;

/** {@link DynamicOps} implementation backed by {@link JsonElement}. */
public final class JsonOps implements DynamicOps<JsonElement> {
    public static final JsonOps INSTANCE = new JsonOps();

    private JsonOps() {}

    @Override
    public JsonElement empty() {
        return JsonNull.INSTANCE;
    }

    @Override
    public <U> U convertTo(DynamicOps<U> outOps, JsonElement input) {
        if (input == null || input.isJsonNull()) {
            return outOps.empty();
        }
        if (input.isJsonObject()) {
            return convertMap(outOps, input);
        }
        if (input.isJsonArray()) {
            return convertList(outOps, input);
        }
        JsonPrimitive prim = input.getAsJsonPrimitive();
        if (prim.isBoolean()) {
            return outOps.createBoolean(prim.getAsBoolean());
        }
        if (prim.isNumber()) {
            BigDecimal bd = prim.getAsBigDecimal();
            try {
                long l = bd.longValueExact();
                if (l >= Integer.MIN_VALUE && l <= Integer.MAX_VALUE) {
                    return outOps.createInt((int) l);
                }
                return outOps.createLong(l);
            } catch (ArithmeticException e) {
                return outOps.createDouble(bd.doubleValue());
            }
        }
        return outOps.createString(prim.getAsString());
    }

    @Override
    public DataResult<Number> getNumberValue(JsonElement input) {
        if (input instanceof JsonPrimitive p) {
            if (p.isNumber()) {
                return DataResult.success(p.getAsNumber());
            }
            if (p.isBoolean()) {
                return DataResult.success(p.getAsBoolean() ? 1 : 0);
            }
        }
        return DataResult.error(() -> "Not a number: " + input);
    }

    @Override
    public JsonElement createNumeric(Number i) {
        return new JsonPrimitive(i);
    }

    @Override
    public DataResult<Boolean> getBooleanValue(JsonElement input) {
        if (input instanceof JsonPrimitive p) {
            if (p.isBoolean()) {
                return DataResult.success(p.getAsBoolean());
            }
            if (p.isNumber()) {
                return DataResult.success(p.getAsNumber().byteValue() != 0);
            }
        }
        return DataResult.error(() -> "Not a boolean: " + input);
    }

    @Override
    public JsonElement createBoolean(boolean value) {
        return new JsonPrimitive(value);
    }

    @Override
    public DataResult<String> getStringValue(JsonElement input) {
        if (input instanceof JsonPrimitive p && p.isString()) {
            return DataResult.success(p.getAsString());
        }
        return DataResult.error(() -> "Not a string: " + input);
    }

    @Override
    public JsonElement createString(String value) {
        return new JsonPrimitive(value);
    }

    @Override
    public DataResult<JsonElement> mergeToList(JsonElement list, JsonElement value) {
        if (!(list instanceof JsonArray) && list != empty()) {
            return DataResult.error(() -> "mergeToList called with not a list: " + list, list);
        }
        JsonArray result = new JsonArray();
        if (list instanceof JsonArray arr) {
            arr.forEach(result::add);
        }
        result.add(value);
        return DataResult.success(result);
    }

    @Override
    public DataResult<JsonElement> mergeToMap(JsonElement map, JsonElement key, JsonElement value) {
        if (!(map instanceof JsonObject) && map != empty()) {
            return DataResult.error(() -> "mergeToMap called with not a map: " + map, map);
        }
        if (!(key instanceof JsonPrimitive p) || !p.isString()) {
            return DataResult.error(() -> "key is not a string: " + key, map);
        }
        JsonObject result = new JsonObject();
        if (map instanceof JsonObject obj) {
            obj.entrySet().forEach(e -> result.add(e.getKey(), e.getValue()));
        }
        result.add(p.getAsString(), value);
        return DataResult.success(result);
    }

    @Override
    public DataResult<Stream<Pair<JsonElement, JsonElement>>> getMapValues(JsonElement input) {
        if (!(input instanceof JsonObject obj)) {
            return DataResult.error(() -> "Not a JSON object: " + input);
        }
        return DataResult.success(obj.entrySet().stream()
                .map(e -> Pair.of(new JsonPrimitive(e.getKey()), e.getValue())));
    }

    @Override
    public JsonElement createMap(Stream<Pair<JsonElement, JsonElement>> map) {
        JsonObject result = new JsonObject();
        map.forEach(p -> result.add(p.getFirst().getAsString(), p.getSecond()));
        return result;
    }

    @Override
    public DataResult<Stream<JsonElement>> getStream(JsonElement input) {
        if (!(input instanceof JsonArray arr)) {
            return DataResult.error(() -> "Not a JSON array: " + input);
        }
        List<JsonElement> copy = new java.util.ArrayList<>(arr.size());
        arr.forEach(copy::add);
        return DataResult.success(copy.stream());
    }

    @Override
    public JsonElement createList(Stream<JsonElement> input) {
        JsonArray result = new JsonArray();
        input.forEach(result::add);
        return result;
    }

    @Override
    public JsonElement remove(JsonElement input, String key) {
        if (input instanceof JsonObject obj) {
            JsonObject result = new JsonObject();
            for (Map.Entry<String, JsonElement> entry : obj.entrySet()) {
                if (!entry.getKey().equals(key)) {
                    result.add(entry.getKey(), entry.getValue());
                }
            }
            return result;
        }
        return input;
    }
}
