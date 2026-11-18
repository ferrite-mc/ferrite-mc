//! Loads the real vanilla 26.3 `commands.json`, serializes it, and decodes the packet
//! with an independent decoder. Skipped (with a note) if `vanilla/` has not been
//! generated (`python tools/fetch_vanilla.py`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use ferrite_command::{
    ArgumentType, CommandDispatcher, LoadOptions, NodeId, NodeKind, ParserRegistry,
    PermissionLevel, PermissionSource, Suggester, SuggestionKind, argument, literal,
};
use ferrite_protocol::{Decode, VarInt};

#[derive(Clone)]
struct Op(PermissionLevel);

impl PermissionSource for Op {
    fn has_permission(&self, level: PermissionLevel) -> bool {
        self.0 >= level
    }
}

fn reports_dir() -> Option<PathBuf> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated/reports");
    if dir.join("commands.json").exists() && dir.join("registries.json").exists() {
        Some(dir)
    } else {
        eprintln!("skipping: {} not generated", dir.display());
        None
    }
}

// ---- independent decoder --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct DecodedNode {
    flags: u8,
    children: Vec<usize>,
    redirect: Option<usize>,
    name: Option<String>,
    parser: Option<(String, Vec<u8>)>,
    suggestions: Option<String>,
}

fn varint(buf: &mut &[u8]) -> usize {
    let v = VarInt::decode(buf).expect("varint").0;
    usize::try_from(v).expect("non-negative")
}

fn string(buf: &mut &[u8]) -> String {
    String::decode(buf).expect("string")
}

fn take(buf: &mut &[u8], n: usize) -> Vec<u8> {
    let (a, b) = buf.split_at(n);
    *buf = b;
    a.to_vec()
}

/// Reads parser properties according to the layouts in the protocol docs, returning the
/// raw bytes consumed.
fn properties(parser: &str, buf: &mut &[u8]) -> Vec<u8> {
    let start = *buf;
    match parser {
        "brigadier:float" | "brigadier:integer" | "brigadier:double" | "brigadier:long" => {
            let width = match parser {
                "brigadier:float" | "brigadier:integer" => 4,
                _ => 8,
            };
            let flags = take(buf, 1)[0];
            assert_eq!(flags & !3, 0, "unknown number flags");
            let n = (flags & 1) as usize + ((flags >> 1) & 1) as usize;
            take(buf, n * width);
        }
        "brigadier:string" => {
            let kind = varint(buf);
            assert!(kind <= 2);
        }
        "minecraft:entity" | "minecraft:score_holder" => {
            take(buf, 1);
        }
        "minecraft:time" => {
            take(buf, 4);
        }
        "minecraft:resource_or_tag"
        | "minecraft:resource_or_tag_key"
        | "minecraft:resource"
        | "minecraft:resource_key"
        | "minecraft:resource_selector" => {
            let registry = string(buf);
            assert!(registry.contains(':'));
        }
        _ => {}
    }
    start[..start.len() - buf.len()].to_vec()
}

fn decode(body: &[u8], ids: &[&str]) -> (Vec<DecodedNode>, usize) {
    let mut buf = body;
    let count = varint(&mut buf);
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let flags = take(&mut buf, 1)[0];
        assert_eq!(flags & 0xC0, 0, "unknown node flags {flags:#x}");
        let kind = flags & 3;
        assert!(kind <= 2);
        let n = varint(&mut buf);
        let children = (0..n).map(|_| varint(&mut buf)).collect();
        let redirect = (flags & 0x08 != 0).then(|| varint(&mut buf));
        let name = (kind != 0).then(|| string(&mut buf));
        let parser = (kind == 2).then(|| {
            let id = ids[varint(&mut buf)].to_owned();
            let props = properties(&id, &mut buf);
            (id, props)
        });
        let suggestions = (kind == 2 && flags & 0x10 != 0).then(|| string(&mut buf));
        nodes.push(DecodedNode {
            flags,
            children,
            redirect,
            name,
            parser,
            suggestions,
        });
    }
    let root = varint(&mut buf);
    assert!(buf.is_empty(), "{} trailing bytes", buf.len());
    for node in &nodes {
        for &c in node.children.iter().chain(node.redirect.iter()) {
            assert!(c < nodes.len(), "dangling index {c}");
        }
    }
    (nodes, root)
}

/// Re-encodes decoded nodes, independently of the crate's encoder.
fn encode(nodes: &[DecodedNode], root: usize, ids: &HashMap<&str, usize>) -> Vec<u8> {
    use ferrite_protocol::Encode;
    let mut out = Vec::new();
    VarInt(nodes.len() as i32).encode(&mut out);
    for n in nodes {
        out.push(n.flags);
        VarInt(n.children.len() as i32).encode(&mut out);
        for &c in &n.children {
            VarInt(c as i32).encode(&mut out);
        }
        if let Some(r) = n.redirect {
            VarInt(r as i32).encode(&mut out);
        }
        if let Some(name) = &n.name {
            name.encode(&mut out);
        }
        if let Some((parser, props)) = &n.parser {
            VarInt(ids[parser.as_str()] as i32).encode(&mut out);
            out.extend_from_slice(props);
        }
        if let Some(s) = &n.suggestions {
            s.encode(&mut out);
        }
    }
    VarInt(root as i32).encode(&mut out);
    out
}

/// Asserts the decoded graph mirrors the dispatcher's visible graph.
fn assert_matches<S>(
    d: &CommandDispatcher<S>,
    source: &S,
    nodes: &[DecodedNode],
    root: usize,
) -> usize {
    let mut map: HashMap<usize, NodeId> = HashMap::from([(root, d.root())]);
    let mut stack = vec![root];
    let mut visited = 0;
    while let Some(i) = stack.pop() {
        visited += 1;
        let decoded = &nodes[i];
        let node = d.node(map[&i]);
        assert_eq!(
            decoded.flags & 4 != 0,
            node.is_executable(),
            "{:?}",
            decoded.name
        );
        assert_eq!(decoded.flags & 0x20 != 0, node.is_restricted());
        match node.kind() {
            NodeKind::Root => assert_eq!(decoded.flags & 3, 0),
            NodeKind::Literal(l) => {
                assert_eq!(decoded.flags & 3, 1);
                assert_eq!(decoded.name.as_deref(), Some(l.as_str()));
            }
            NodeKind::Argument { name, ty } => {
                assert_eq!(decoded.flags & 3, 2);
                assert_eq!(decoded.name.as_deref(), Some(name.as_str()));
                let (parser, props) = decoded.parser.as_ref().unwrap();
                assert_eq!(parser, ty.parser_id());
                let mut expected = Vec::new();
                ty.write_properties(&mut expected);
                assert_eq!(props, &expected);
            }
        }
        let visible: Vec<NodeId> = node
            .children()
            .iter()
            .copied()
            .filter(|&c| d.node(c).can_use(source))
            .collect();
        assert_eq!(decoded.children.len(), visible.len());
        for (&ci, &cid) in decoded.children.iter().zip(&visible) {
            map.insert(ci, cid);
            stack.push(ci);
        }
        if let Some(r) = decoded.redirect {
            let target = node
                .redirect()
                .expect("decoded redirect without source redirect");
            if let Some(&mapped) = map.get(&r) {
                assert_eq!(mapped, target);
            } else {
                // Target is decoded later; check by path name instead.
                assert_eq!(
                    nodes[r].name.as_deref().unwrap_or(""),
                    d.node(target).name()
                );
            }
        }
    }
    visited
}

// ---- tests ------------------------------------------------------------------------------

#[test]
fn registry_order_matches_report() {
    let Some(dir) = reports_dir() else { return };
    let registry = ParserRegistry::load(&dir).unwrap();
    let ids = registry.identifiers();
    assert_eq!(ids[0], "brigadier:bool");
    assert_eq!(registry.id("brigadier:string"), Some(5));
    assert_eq!(registry.id("minecraft:entity"), Some(6));
    // Every parser this crate models explicitly is in the 26.3 registry.
    for ty in [
        ArgumentType::Bool,
        ArgumentType::float(),
        ArgumentType::double(),
        ArgumentType::integer(),
        ArgumentType::long(),
        ArgumentType::word(),
        ArgumentType::entity(),
        ArgumentType::GameProfile,
        ArgumentType::BlockPos,
        ArgumentType::ColumnPos,
        ArgumentType::Vec3,
        ArgumentType::Vec2,
        ArgumentType::BlockState,
        ArgumentType::Component,
        ArgumentType::Message,
        ArgumentType::ResourceLocation,
        ArgumentType::Rotation,
        ArgumentType::Dimension,
        ArgumentType::Gamemode,
        ArgumentType::Time { min: 0 },
        ArgumentType::ScoreHolder { multiple: true },
    ] {
        assert!(registry.id(ty.parser_id()).is_some(), "{}", ty.parser_id());
    }
    for p in ferrite_command::arguments::RegistryParser::ALL {
        assert!(registry.id(p.id()).is_some(), "{}", p.id());
    }
}

#[test]
fn vanilla_tree_loads_serializes_and_round_trips() {
    let Some(dir) = reports_dir() else { return };
    let registry = ParserRegistry::load(&dir).unwrap();
    let mut d = CommandDispatcher::<Op>::new();
    let report = d.load_vanilla_dir(&dir, LoadOptions::default()).unwrap();
    assert!(report.nodes > 1000, "only {} nodes", report.nodes);
    assert_eq!(
        report.implicit_root_redirects,
        vec![vec!["execute", "run"], vec!["return", "run"]]
    );
    for parser in &report.opaque_parsers {
        assert!(
            registry.id(parser).is_some(),
            "vanilla uses parser {parser} missing from registries.json"
        );
    }

    let ids = registry.identifiers();
    let id_map: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, &s)| (s, i)).collect();
    let mut sizes = Vec::new();
    for level in [
        PermissionLevel::All,
        PermissionLevel::Gamemasters,
        PermissionLevel::Owners,
    ] {
        let op = Op(level);
        let packet = d.serialize_commands(&op, &registry).unwrap();
        assert!(
            packet.unknown_parsers.is_empty(),
            "unknown parsers: {:?}",
            packet.unknown_parsers
        );
        let (nodes, root) = decode(&packet.body, &ids);
        assert_eq!(nodes.len(), packet.node_count);
        assert_eq!(root, 0);
        assert_eq!(
            encode(&nodes, root, &id_map),
            packet.body,
            "byte round trip"
        );
        assert_eq!(
            assert_matches(&d, &op, &nodes, root),
            nodes.len(),
            "all reachable"
        );
        sizes.push(nodes.len());

        // execute run -> root, execute as <targets> -> execute.
        let names = |i: usize| nodes[i].name.clone().unwrap_or_default();
        let execute = nodes[root]
            .children
            .iter()
            .copied()
            .find(|&i| names(i) == "execute");
        if level >= PermissionLevel::Gamemasters {
            let execute = execute.expect("execute visible to ops");
            let run = nodes[execute]
                .children
                .iter()
                .copied()
                .find(|&i| names(i) == "run")
                .unwrap();
            assert_eq!(nodes[run].redirect, Some(root));
            assert!(nodes[execute].flags & 0x20 != 0, "execute is restricted");
        } else {
            assert!(execute.is_none());
        }
    }
    assert!(sizes[0] < sizes[1] && sizes[1] < sizes[2], "{sizes:?}");

    // Every registry parser vanilla's tree uses is covered with a known id.
    let mut used = std::collections::BTreeSet::new();
    let packet = d
        .serialize_commands(&Op(PermissionLevel::Owners), &registry)
        .unwrap();
    for node in decode(&packet.body, &ids).0 {
        if let Some((p, _)) = node.parser {
            used.insert(p);
        }
    }
    assert!(used.len() > 50, "only {} parsers used", used.len());
}

#[test]
fn vanilla_tree_parses_real_commands() {
    let Some(dir) = reports_dir() else { return };
    let mut d = CommandDispatcher::<Op>::new();
    d.load_vanilla_dir(
        &dir,
        LoadOptions {
            requirement: Box::new(|level| Arc::new(move |s: &Op| s.0 >= level)),
            command: Box::new(|path| {
                let path = path.join(" ");
                Arc::new(move |_| Err(ferrite_command::CommandError::failed(path.clone())))
            }),
        },
    )
    .unwrap();
    let op = Op(PermissionLevel::Owners);
    let run = |cmd: &str| match d.execute(cmd, op.clone()) {
        Err(ferrite_command::CommandError::Failed(path)) => Ok(path),
        Err(ferrite_command::CommandError::Syntax(e)) => Err(e.to_string()),
        Ok(_) => unreachable!(),
    };
    assert_eq!(
        run("gamemode creative @a").unwrap(),
        "gamemode gamemode target"
    );
    assert_eq!(run("tp @s ~ ~10 ~").unwrap(), "teleport targets location");
    assert_eq!(
        run("give @p diamond_sword[enchantments={sharpness:5}] 1").unwrap(),
        "give targets item count"
    );
    assert_eq!(
        run("execute as @e[type=zombie] at @s run say hello there").unwrap(),
        "say message"
    );
    assert_eq!(run("time set 1d").unwrap(), "time set time");
    assert_eq!(
        run("setblock 1 2 3 oak_stairs[facing=east]").unwrap(),
        "setblock pos block"
    );
    assert_eq!(
        run("tellraw @a {\"text\":\"hi there\"}").unwrap(),
        "tellraw targets message"
    );
    let e = run("gamemode god").unwrap_err();
    assert!(e.starts_with("Unknown game mode: god"), "{e}");
    let e = run("weather clear -1").unwrap_err();
    assert!(e.contains("must not be less than"), "{e}");

    let s = d.suggest("gamemode ", op.clone());
    assert_eq!(
        s.texts(),
        ["adventure", "creative", "spectator", "survival"]
    );
    let s = d.suggest("execute as @a ", op.clone());
    assert!(s.texts().contains(&"run"));
    let s = d.suggest("execute run gamem", op);
    assert_eq!(s.texts(), ["gamemode"]);
}

#[test]
fn opaque_parser_trees_still_serialize() {
    let registry = ParserRegistry::from_ordered(["brigadier:bool", "brigadier:string"]);
    let mut d = CommandDispatcher::<()>::new();
    d.register(
        literal("custom")
            .then(argument("thing", ArgumentType::opaque("custom:thing")).executes(|_| Ok(1))),
    );
    d.register(
        literal("flag").then(
            argument("on", ArgumentType::Bool)
                .suggester(Suggester {
                    kind: SuggestionKind::AskServer,
                    provider: None,
                })
                .executes(|_| Ok(1)),
        ),
    );
    let packet = d.serialize_commands(&(), &registry).unwrap();
    assert_eq!(
        packet.unknown_parsers.into_iter().collect::<Vec<_>>(),
        ["custom:thing"]
    );
    let (nodes, root) = decode(&packet.body, &registry.identifiers());
    let thing = nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("thing"))
        .unwrap();
    assert_eq!(
        thing.parser.as_ref().unwrap(),
        &("brigadier:string".to_owned(), vec![0])
    );
    assert_eq!(thing.suggestions.as_deref(), Some("minecraft:ask_server"));
    let on = nodes
        .iter()
        .find(|n| n.name.as_deref() == Some("on"))
        .unwrap();
    assert_eq!(on.flags, 2 | 4 | 0x10);
    assert_eq!(on.suggestions.as_deref(), Some("minecraft:ask_server"));
    assert_eq!(root, 0);
    // Without the fallback parser, serialization reports it.
    let err = d
        .serialize_commands(&(), &ParserRegistry::from_ordered(["brigadier:bool"]))
        .unwrap_err();
    assert_eq!(
        err,
        ferrite_command::SerializeError::MissingParser("brigadier:string".into())
    );
}

#[test]
fn redirect_to_hidden_node_is_dropped() {
    let registry = ParserRegistry::from_ordered(["brigadier:bool"]);
    let mut d = CommandDispatcher::<u8>::new();
    let secret = d.register(
        literal("secret")
            .requires(|l: &u8| *l > 0)
            .executes(|_| Ok(1)),
    );
    d.register(literal("alias").redirect(secret));
    let (nodes, _) = decode(&d.serialize_commands(&0, &registry).unwrap().body, &[]);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1].name.as_deref(), Some("alias"));
    assert_eq!(nodes[1].redirect, None);
    let (nodes, _) = decode(
        &d.serialize_commands(&1, &registry).unwrap().body,
        &["brigadier:bool"],
    );
    assert_eq!(nodes.len(), 3);
    let alias = nodes
        .iter()
        .position(|n| n.name.as_deref() == Some("alias"))
        .unwrap();
    let secret = nodes
        .iter()
        .position(|n| n.name.as_deref() == Some("secret"))
        .unwrap();
    assert_eq!(nodes[alias].redirect, Some(secret));
}
