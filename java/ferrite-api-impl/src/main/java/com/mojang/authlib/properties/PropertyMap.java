package com.mojang.authlib.properties;

/** Properties of a profile, keyed by name; a multimap as in authlib. */
public class PropertyMap extends com.google.common.collect.ForwardingMultimap<String, Property> {
    private final com.google.common.collect.Multimap<String, Property> delegate = com.google.common.collect.LinkedHashMultimap.create();
    @Override protected com.google.common.collect.Multimap<String, Property> delegate() { return delegate; }
}
