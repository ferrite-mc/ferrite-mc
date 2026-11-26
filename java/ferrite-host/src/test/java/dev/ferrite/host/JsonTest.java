package dev.ferrite.host;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class JsonTest {
    @Test
    void parses_nested_objects_arrays_and_escapes() {
        Map<String, Object> root =
                Json.asObject(
                        Json.parse(
                                "{\"list\":[1,2.5,true,null],\"nested\":{\"text\":\"x\\n\\u0041\"}}"));
        List<Object> list = Json.asList(root.get("list"));
        assertEquals(4, list.size());
        assertEquals(1L, list.get(0));
        assertEquals(2.5, list.get(1));
        assertEquals(Boolean.TRUE, list.get(2));
        assertNull(list.get(3));
        assertEquals("x\nA", Json.string(Json.asObject(root.get("nested")), "text", null));
    }

    @Test
    void stringify_round_trips_quotes_and_controls() {
        Map<String, Object> value =
                Json.object(
                        "text",
                        "quote \" slash \\ newline \n tab \t",
                        "array",
                        Json.array(1L, 2.5, Boolean.FALSE, null),
                        "object",
                        Json.object("nested", "ok"));
        Map<String, Object> parsed = Json.asObject(Json.parse(Json.stringify(value)));
        assertEquals(value, parsed);
    }

    @Test
    void integers_are_long_and_decimals_are_double() {
        assertInstanceOf(Long.class, Json.parse("42"));
        assertInstanceOf(Long.class, Json.parse("-7"));
        assertInstanceOf(Double.class, Json.parse("1e3"));
        assertInstanceOf(Double.class, Json.parse("1.5"));
    }

    @Test
    void malformed_json_is_rejected() {
        assertThrows(IllegalArgumentException.class, () -> Json.parse("{\"a\":}"));
        assertThrows(IllegalArgumentException.class, () -> Json.parse("\"unterminated"));
        assertThrows(IllegalArgumentException.class, () -> Json.parse("{} trailing"));
        assertThrows(IllegalArgumentException.class, () -> Json.parse(""));
    }

    @Test
    void object_requires_even_arguments_and_string_keys() {
        assertThrows(IllegalArgumentException.class, () -> Json.object("one"));
        assertTrue(Json.object("one", 1L).containsKey("one"));
    }
}
