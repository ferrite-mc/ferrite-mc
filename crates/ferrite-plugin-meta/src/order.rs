//! Load-order resolution for a set of parsed plugins.
//!
//! The algorithm follows Paper's `ModernPluginLoadingStrategy`: hard and soft
//! dependencies plus `loadbefore` become ordering edges, a plugin whose hard
//! dependencies are missing is skipped, plugins caught in a dependency cycle
//! are skipped as a group while everything else still loads, and the survivors
//! are topologically sorted deterministically.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::error::PluginError;
use crate::model::{PaperDependencyLoad, PluginLoadOrder, PluginMeta, ThreadingMode};

/// A plugin jar that was read and validated, ready to be ordered.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    /// Jar the metadata came from.
    pub source: PathBuf,
    /// Validated metadata.
    pub meta: PluginMeta,
}

/// A plugin that will be loaded, in load order.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// Jar the metadata came from.
    pub source: PathBuf,
    /// Validated metadata.
    pub meta: PluginMeta,
}

impl LoadedPlugin {
    /// Plugin name, as declared in the descriptor.
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    /// How Ferrite should schedule this plugin.
    pub fn threading_mode(&self) -> ThreadingMode {
        self.meta.threading_mode()
    }
}

/// A plugin jar that will not load, and why.
#[derive(Debug)]
pub struct FailedPlugin {
    /// Jar that failed.
    pub source: PathBuf,
    /// Plugin name, when the descriptor could be read far enough to know it.
    pub name: Option<String>,
    /// Reason the plugin is not in the load plan.
    pub error: PluginError,
}

/// The outcome of resolving load order.
#[derive(Debug, Default)]
pub struct LoadPlan {
    /// Plugins with `load: STARTUP`, in load order.
    pub startup: Vec<LoadedPlugin>,
    /// Plugins with `load: POSTWORLD`, in load order.
    pub postworld: Vec<LoadedPlugin>,
    /// Plugins that will not load, with their reasons.
    pub failed: Vec<FailedPlugin>,
}

impl LoadPlan {
    /// Every plugin that will load, startup phase first.
    pub fn loaded(&self) -> impl Iterator<Item = &LoadedPlugin> {
        self.startup.iter().chain(self.postworld.iter())
    }

    /// Whether any plugin will load at all.
    pub fn is_empty(&self) -> bool {
        self.startup.is_empty() && self.postworld.is_empty()
    }
}

/// Resolve the load order of already-parsed plugins.
///
/// Both phases are ordered together (a POSTWORLD plugin may depend on a
/// STARTUP plugin) and then split, so [`LoadPlan::startup`] always precedes
/// [`LoadPlan::postworld`]. Duplicate plugin names are resolved in favor of the
/// first entry; later entries are reported as failures.
pub fn resolve_load_order(entries: Vec<PluginEntry>) -> LoadPlan {
    let count = entries.len();
    let relations: Vec<Relations> = entries
        .iter()
        .map(|entry| Relations::from_meta(&entry.meta))
        .collect();
    let mut alive = vec![true; count];
    let mut failures: Vec<(usize, PluginError)> = Vec::new();

    // 1. A plugin name may only be claimed once; the first jar wins.
    let mut claimed: HashMap<String, usize> = HashMap::new();
    for i in 0..count {
        let key = entries[i].meta.name.to_ascii_lowercase();
        let first = *claimed.entry(key).or_insert(i);
        if first != i {
            alive[i] = false;
            failures.push((
                i,
                PluginError::DuplicateName {
                    name: entries[i].meta.name.clone(),
                    first: entries[first].source.clone(),
                },
            ));
        }
    }

    // 2. Names and `provides` aliases resolve to the first plugin claiming them.
    let mut identity: HashMap<String, usize> = HashMap::new();
    for i in 0..count {
        if !alive[i] {
            continue;
        }
        for name in entries[i].meta.identity_names() {
            identity.entry(name.to_ascii_lowercase()).or_insert(i);
        }
    }

    // 3. Hard dependencies must exist, or the plugin is skipped. Every missing
    //    dependency is listed at once, like Paper does.
    for i in 0..count {
        if !alive[i] {
            continue;
        }
        let missing: Vec<String> = relations[i]
            .hard
            .iter()
            .filter(|dependency| !identity.contains_key(&dependency.to_ascii_lowercase()))
            .cloned()
            .collect();
        if !missing.is_empty() {
            alive[i] = false;
            failures.push((
                i,
                PluginError::MissingDependencies {
                    plugin: entries[i].meta.name.clone(),
                    dependencies: missing,
                },
            ));
        }
    }

    // 4. Ordering edges between survivors: `edges[a]` holds plugins that must
    //    load after `a`. A plugin listed in both `loadbefore` and
    //    `depend`/`softdepend` is only ordered before it, like Paper.
    let mut edges = vec![Vec::new(); count];
    for i in 0..count {
        if !alive[i] {
            continue;
        }
        for dependency in &relations[i].after {
            if let Some(&j) = identity.get(&dependency.to_ascii_lowercase())
                && j != i
                && alive[j]
            {
                edges[j].push(i);
            }
        }
        for dependency in &relations[i].before {
            if let Some(&j) = identity.get(&dependency.to_ascii_lowercase())
                && j != i
                && alive[j]
            {
                edges[i].push(j);
            }
        }
    }
    for successors in &mut edges {
        successors.sort_unstable();
        successors.dedup();
    }

    // 5. Every plugin in a dependency cycle is skipped; the rest still load.
    for component in strongly_connected_components(&edges) {
        if component.len() < 2 {
            continue;
        }
        let cycle: Vec<String> = component
            .iter()
            .map(|&i| entries[i].meta.name.clone())
            .collect();
        let chain = format!("{} -> {}", cycle.join(" -> "), cycle[0]);
        for &i in &component {
            alive[i] = false;
            failures.push((
                i,
                PluginError::Cycle {
                    cycle: chain.clone(),
                },
            ));
        }
    }

    // 6. A plugin whose required dependency was skipped fails with it, until
    //    the failure set stops growing.
    loop {
        let mut changed = false;
        for i in 0..count {
            if !alive[i] {
                continue;
            }
            let missing: Vec<String> = relations[i]
                .hard
                .iter()
                .filter(
                    |dependency| match identity.get(&dependency.to_ascii_lowercase()) {
                        Some(&j) => j != i && !alive[j],
                        None => false,
                    },
                )
                .cloned()
                .collect();
            if !missing.is_empty() {
                alive[i] = false;
                failures.push((
                    i,
                    PluginError::MissingDependencies {
                        plugin: entries[i].meta.name.clone(),
                        dependencies: missing,
                    },
                ));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 7. Topological sort of the survivors, alphabetical among equals so the
    //    plan does not depend on directory enumeration order.
    let mut indegree = vec![0usize; count];
    for i in 0..count {
        if !alive[i] {
            continue;
        }
        for &j in &edges[i] {
            if alive[j] {
                indegree[j] += 1;
            }
        }
    }
    let mut ready: BTreeSet<(String, usize)> = (0..count)
        .filter(|&i| alive[i] && indegree[i] == 0)
        .map(|i| (relations[i].key.clone(), i))
        .collect();
    let mut order = Vec::with_capacity(count);
    while let Some((_, i)) = ready.pop_first() {
        order.push(i);
        for &j in &edges[i] {
            if !alive[j] {
                continue;
            }
            indegree[j] -= 1;
            if indegree[j] == 0 {
                ready.insert((relations[j].key.clone(), j));
            }
        }
    }
    debug_assert_eq!(order.len(), alive.iter().filter(|alive| **alive).count());

    // 8. Split the order into the startup and postworld phases.
    let mut startup = Vec::new();
    let mut postworld = Vec::new();
    for &i in &order {
        let loaded = LoadedPlugin {
            source: entries[i].source.clone(),
            meta: entries[i].meta.clone(),
        };
        match relations[i].load {
            PluginLoadOrder::Startup => startup.push(loaded),
            PluginLoadOrder::PostWorld => postworld.push(loaded),
        }
    }

    failures.sort_by_key(|(index, _)| *index);
    let failed = failures
        .into_iter()
        .map(|(index, error)| FailedPlugin {
            source: entries[index].source.clone(),
            name: Some(entries[index].meta.name.clone()),
            error,
        })
        .collect();

    LoadPlan {
        startup,
        postworld,
        failed,
    }
}

/// Ordering-relevant projection of a plugin's metadata.
struct Relations {
    /// Lowercase plugin name, used for deterministic tie-breaks.
    key: String,
    load: PluginLoadOrder,
    /// Names this plugin loads after.
    after: Vec<String>,
    /// Names this plugin loads before.
    before: Vec<String>,
    /// Dependencies that must resolve.
    hard: Vec<String>,
}

impl Relations {
    fn from_meta(meta: &PluginMeta) -> Self {
        let mut hard = dedupe_names(&meta.depend);
        let mut after: Vec<String> = meta
            .depend
            .iter()
            .chain(meta.softdepend.iter())
            .cloned()
            .collect();
        let mut before = meta.loadbefore.clone();

        if let Some(paper) = &meta.paper {
            for (name, dependency) in &paper.dependencies.server {
                match dependency.load {
                    PaperDependencyLoad::Before => after.push(name.clone()),
                    PaperDependencyLoad::After => before.push(name.clone()),
                    PaperDependencyLoad::Omit => {}
                }
                // Paper only validates dependencies that join the classpath.
                if dependency.required && dependency.join_classpath {
                    hard.push(name.clone());
                }
            }
        }

        let mut after = dedupe_names(&after);
        let before = dedupe_names(&before);
        hard = dedupe_names(&hard);

        let before_keys: HashSet<String> = before
            .iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();
        after.retain(|name| !before_keys.contains(&name.to_ascii_lowercase()));

        Self {
            key: meta.name.to_ascii_lowercase(),
            load: meta.load,
            after,
            before,
            hard,
        }
    }
}

/// Deduplicate names case-insensitively, keeping the first spelling.
fn dedupe_names(names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    names
        .iter()
        .filter(|name| seen.insert(name.to_ascii_lowercase()))
        .cloned()
        .collect()
}

/// Tarjan's strongly connected components, iterative so deep dependency
/// chains cannot overflow the stack. Components of one node are acyclic
/// unless the node points at itself, which plugin metadata cannot express.
fn strongly_connected_components(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: usize = usize::MAX;
    let count = adjacency.len();
    let mut index = vec![UNVISITED; count];
    let mut lowlink = vec![0usize; count];
    let mut on_stack = vec![false; count];
    let mut stack: Vec<usize> = Vec::new();
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut next_index = 0usize;
    let mut frames: Vec<(usize, usize)> = Vec::new();

    for root in 0..count {
        if index[root] != UNVISITED {
            continue;
        }
        frames.push((root, 0));
        while let Some(&(node, position)) = frames.last() {
            if index[node] == UNVISITED {
                index[node] = next_index;
                lowlink[node] = next_index;
                next_index += 1;
                stack.push(node);
                on_stack[node] = true;
            }
            let neighbors = &adjacency[node];
            if position < neighbors.len() {
                let next = neighbors[position];
                frames.last_mut().expect("frame is present").1 = position + 1;
                if index[next] == UNVISITED {
                    frames.push((next, 0));
                } else if on_stack[next] {
                    lowlink[node] = lowlink[node].min(index[next]);
                }
            } else {
                frames.pop();
                if lowlink[node] == index[node] {
                    let mut component = Vec::new();
                    loop {
                        let member = stack.pop().expect("component member is on the stack");
                        on_stack[member] = false;
                        component.push(member);
                        if member == node {
                            break;
                        }
                    }
                    components.push(component);
                }
                if let Some(&(parent, _)) = frames.last() {
                    lowlink[parent] = lowlink[parent].min(lowlink[node]);
                }
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ManifestFormat;
    use std::collections::BTreeMap;

    fn plugin(name: &str) -> PluginMeta {
        PluginMeta {
            name: name.to_owned(),
            version: "1.0".to_owned(),
            main: format!("com.example.{}", name.to_ascii_lowercase()),
            api_version: Some("1.21".to_owned()),
            authors: Vec::new(),
            description: None,
            website: None,
            prefix: None,
            load: PluginLoadOrder::PostWorld,
            depend: Vec::new(),
            softdepend: Vec::new(),
            loadbefore: Vec::new(),
            provides: Vec::new(),
            libraries: Vec::new(),
            commands: BTreeMap::new(),
            permissions: BTreeMap::new(),
            folia_supported: false,
            format: ManifestFormat::PluginYml,
            paper: None,
        }
    }

    fn entry(meta: PluginMeta) -> PluginEntry {
        PluginEntry {
            source: PathBuf::from(format!("{}.jar", meta.name)),
            meta,
        }
    }

    fn names(plan: &LoadPlan) -> Vec<String> {
        plan.postworld
            .iter()
            .map(|plugin| plugin.name().to_owned())
            .collect()
    }

    #[test]
    fn missing_hard_dependency_is_reported_with_every_name() {
        let mut broken = plugin("Broken");
        broken.depend = vec!["Ghost".to_owned(), "Phantom".to_owned()];
        let plan = resolve_load_order(vec![entry(broken), entry(plugin("Fine"))]);
        assert_eq!(names(&plan), ["Fine"]);
        assert_eq!(plan.failed.len(), 1);
        assert_eq!(
            plan.failed[0].error.missing_dependencies(),
            Some(["Ghost".to_owned(), "Phantom".to_owned()].as_slice())
        );
    }

    #[test]
    fn soft_dependency_orders_when_present_and_is_optional_when_absent() {
        let mut consumer = plugin("Consumer");
        consumer.softdepend = vec!["Provider".to_owned(), "Ghost".to_owned()];
        let plan = resolve_load_order(vec![entry(consumer), entry(plugin("Provider"))]);
        assert_eq!(names(&plan), ["Provider", "Consumer"]);
        assert!(plan.failed.is_empty());
    }

    #[test]
    fn loadbefore_orders_plugin_before_target_and_overrides_depend() {
        let mut first = plugin("First");
        first.loadbefore = vec!["Second".to_owned()];
        // Being both a dependency and a loadbefore target must not become a cycle.
        first.depend = vec!["Second".to_owned()];
        let plan = resolve_load_order(vec![entry(plugin("Second")), entry(first)]);
        assert_eq!(names(&plan), ["First", "Second"]);
        assert!(plan.failed.is_empty());
    }

    #[test]
    fn self_dependency_is_ignored() {
        let mut lonely = plugin("Lonely");
        lonely.depend = vec!["lonely".to_owned()];
        let plan = resolve_load_order(vec![entry(lonely)]);
        assert_eq!(names(&plan), ["Lonely"]);
        assert!(plan.failed.is_empty());
    }

    #[test]
    fn cycle_members_are_skipped_and_reported_with_paper_message() {
        let mut a = plugin("A");
        a.depend = vec!["B".to_owned()];
        let mut b = plugin("B");
        b.depend = vec!["A".to_owned()];
        let plan = resolve_load_order(vec![entry(a), entry(b), entry(plugin("C"))]);
        assert_eq!(names(&plan), ["C"]);
        assert_eq!(plan.failed.len(), 2);
        for failed in &plan.failed {
            let message = failed.error.to_string();
            assert!(
                message.starts_with("Circular plugin loading detected: "),
                "unexpected message: {message}"
            );
            assert!(message.contains(" -> "), "unexpected message: {message}");
            assert!(message.contains('A') && message.contains('B'));
        }
    }

    #[test]
    fn dependent_of_skipped_plugin_fails_too() {
        let mut a = plugin("A");
        a.depend = vec!["B".to_owned()];
        let mut b = plugin("B");
        b.depend = vec!["A".to_owned()];
        let mut dependent = plugin("Dependent");
        dependent.depend = vec!["A".to_owned()];
        let plan = resolve_load_order(vec![entry(a), entry(b), entry(dependent)]);
        assert!(plan.is_empty());
        assert_eq!(plan.failed.len(), 3);
        assert_eq!(
            plan.failed[2].error.missing_dependencies(),
            Some(["A".to_owned()].as_slice())
        );
    }

    #[test]
    fn provides_aliases_satisfy_dependencies_case_insensitively() {
        let mut provider = plugin("Provider");
        provider.provides = vec!["Alias".to_owned()];
        let mut consumer = plugin("Consumer");
        consumer.depend = vec!["ALIAS".to_owned()];
        let plan = resolve_load_order(vec![entry(consumer), entry(provider)]);
        assert_eq!(names(&plan), ["Provider", "Consumer"]);
        assert!(plan.failed.is_empty());
    }

    #[test]
    fn duplicate_names_keep_the_first_entry() {
        let mut duplicate = plugin("dupe");
        duplicate.version = "2.0".to_owned();
        let plan = resolve_load_order(vec![entry(plugin("Dupe")), entry(duplicate)]);
        assert_eq!(names(&plan), ["Dupe"]);
        assert_eq!(plan.failed.len(), 1);
        assert!(matches!(
            plan.failed[0].error,
            PluginError::DuplicateName { .. }
        ));
    }

    #[test]
    fn equal_plugins_are_ordered_alphabetically() {
        let plan = resolve_load_order(vec![
            entry(plugin("Charlie")),
            entry(plugin("Alpha")),
            entry(plugin("Bravo")),
        ]);
        assert_eq!(names(&plan), ["Alpha", "Bravo", "Charlie"]);
    }

    #[test]
    fn startup_and_postworld_are_split_but_still_ordered() {
        let mut startup = plugin("Startup");
        startup.load = PluginLoadOrder::Startup;
        let mut postworld = plugin("Postworld");
        postworld.depend = vec!["Startup".to_owned()];
        let plan = resolve_load_order(vec![entry(postworld), entry(startup)]);
        assert_eq!(plan.startup.len(), 1);
        assert_eq!(plan.startup[0].name(), "Startup");
        assert_eq!(plan.postworld.len(), 1);
        assert_eq!(plan.postworld[0].name(), "Postworld");
    }

    #[test]
    fn threading_mode_follows_folia_support() {
        let mut folia = plugin("Folia");
        folia.folia_supported = true;
        let plan = resolve_load_order(vec![entry(folia), entry(plugin("Legacy"))]);
        let modes: Vec<_> = plan.loaded().map(LoadedPlugin::threading_mode).collect();
        assert_eq!(
            modes,
            [ThreadingMode::RegionAware, ThreadingMode::LegacyMainThread]
        );
    }

    #[test]
    fn scc_finds_nested_cycles() {
        // 0 -> 1 -> 2 -> 0 is one component; 3 -> 4 -> 5 -> 3 another.
        let adjacency = vec![vec![1], vec![2], vec![0], vec![4], vec![5], vec![3]];
        let mut components = strongly_connected_components(&adjacency);
        for component in &mut components {
            component.sort_unstable();
        }
        components.sort();
        assert_eq!(components, vec![vec![0, 1, 2], vec![3, 4, 5]]);
    }
}
