//! Decoding and validation of `plugin.yml` / `paper-plugin.yml` descriptors.
//!
//! The raw YAML shape mirrors what Bukkit and Paper accept. Values are read as
//! YAML scalars coerced to text, because real descriptors contain unquoted
//! `api-version: 1.21` (a float) and `default: true` (a bool) and both must not
//! fail to decode.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::Unexpected;

use crate::error::{MetadataError, ValidationError};
use crate::model::{
    CommandDescription, ManifestFormat, PaperDependencies, PaperDependency, PaperDependencyLoad,
    PaperMeta, PermissionDefault, PermissionDescription, PluginLoadOrder, PluginMeta,
};

/// Decode and validate a descriptor that was read from a jar.
pub(crate) fn parse(format: ManifestFormat, yaml: &str) -> Result<PluginMeta, MetadataError> {
    let raw = serde_norway::from_str::<RawMeta>(yaml)
        .map_err(|error| MetadataError::Yaml(error.to_string()))?;
    raw.build(format).map_err(MetadataError::Invalid)
}

/// A scalar coerced to a string the way Bukkit reads values out of the YAML map.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Scalar(String);

impl Scalar {
    fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = Scalar;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a YAML scalar")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Scalar(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Scalar(value))
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Scalar(value.to_string()))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Scalar(value.to_string()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Scalar(value.to_string()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(Scalar(float_scalar(value)))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Err(E::invalid_type(Unexpected::Unit, &"a YAML scalar"))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Formats a float keeping a trailing `.0`, matching Java's `Double#toString`
/// for the integral values plugin descriptors use (e.g. `version: 1.0`).
fn float_scalar(value: f64) -> String {
    let text = value.to_string();
    if value.fract() == 0.0 && !text.contains(['.', 'e', 'E']) {
        format!("{text}.0")
    } else {
        text
    }
}

/// A YAML value that may be either one scalar or a list, like Bukkit's
/// `getStringList` helpers accept.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    name: Option<Scalar>,
    version: Option<Scalar>,
    main: Option<Scalar>,
    #[serde(rename = "api-version")]
    api_version: Option<Scalar>,
    author: Option<Scalar>,
    authors: Option<OneOrMany<Scalar>>,
    description: Option<Scalar>,
    website: Option<Scalar>,
    prefix: Option<Scalar>,
    load: Option<Scalar>,
    depend: Option<OneOrMany<Scalar>>,
    softdepend: Option<OneOrMany<Scalar>>,
    loadbefore: Option<OneOrMany<Scalar>>,
    provides: Option<OneOrMany<Scalar>>,
    libraries: Option<OneOrMany<Scalar>>,
    commands: Option<BTreeMap<String, Option<RawCommand>>>,
    permissions: Option<BTreeMap<String, Option<RawPermission>>>,
    #[serde(rename = "folia-supported")]
    folia_supported: Option<bool>,
    bootstrapper: Option<Scalar>,
    loader: Option<Scalar>,
    dependencies: Option<RawDependencies>,
    #[serde(rename = "has-open-classloader")]
    has_open_classloader: Option<bool>,
}

impl RawMeta {
    fn build(self, format: ManifestFormat) -> Result<PluginMeta, ValidationError> {
        let name = match self.name.map(Scalar::into_inner) {
            Some(name) if !valid_plugin_name(&name) => {
                return Err(ValidationError::InvalidName(name));
            }
            // Bukkit replaces spaces with underscores, and uses the normalized
            // form everywhere afterwards.
            Some(name) => name.replace(' ', "_"),
            None => return Err(ValidationError::MissingName),
        };
        let version = self
            .version
            .map(Scalar::into_inner)
            .ok_or(ValidationError::MissingVersion)?;
        let main = self
            .main
            .map(Scalar::into_inner)
            .ok_or(ValidationError::MissingMain)?;

        let mut authors = Vec::new();
        if let Some(author) = self.author {
            authors.push(author.into_inner());
        }
        if let Some(raw_authors) = self.authors {
            authors.extend(raw_authors.into_vec().into_iter().map(Scalar::into_inner));
        }

        let load = match self.load {
            Some(value) => PluginLoadOrder::parse(&value.0)
                .ok_or(ValidationError::InvalidLoad { value: value.0 })?,
            None => PluginLoadOrder::default(),
        };

        let mut commands = BTreeMap::new();
        for (name, raw) in self.commands.unwrap_or_default() {
            commands.insert(name, raw.map(RawCommand::build).unwrap_or_default());
        }

        let mut permissions = BTreeMap::new();
        for (node, raw) in self.permissions.unwrap_or_default() {
            let permission = match raw {
                Some(raw) => raw.build(&node)?,
                None => PermissionDescription::default(),
            };
            permissions.insert(node, permission);
        }

        let paper = match format {
            ManifestFormat::PaperPluginYml => {
                let dependencies = self
                    .dependencies
                    .map(RawDependencies::build)
                    .transpose()?
                    .unwrap_or_default();
                Some(PaperMeta {
                    bootstrapper: self.bootstrapper.map(Scalar::into_inner),
                    loader: self.loader.map(Scalar::into_inner),
                    dependencies,
                    has_open_classloader: self.has_open_classloader.unwrap_or(false),
                })
            }
            ManifestFormat::PluginYml => None,
        };

        Ok(PluginMeta {
            name,
            version,
            main,
            api_version: self.api_version.map(Scalar::into_inner),
            authors,
            description: self.description.map(Scalar::into_inner),
            website: self.website.map(Scalar::into_inner),
            prefix: self.prefix.map(Scalar::into_inner),
            load,
            depend: self.depend.map(name_list).unwrap_or_default(),
            softdepend: self.softdepend.map(name_list).unwrap_or_default(),
            loadbefore: self.loadbefore.map(name_list).unwrap_or_default(),
            provides: self.provides.map(name_list).unwrap_or_default(),
            libraries: self.libraries.map(string_list).unwrap_or_default(),
            commands,
            permissions,
            folia_supported: self.folia_supported.unwrap_or(false),
            format,
            paper,
        })
    }
}

/// Bukkit's plugin name rule: non-empty and only `[A-Za-z0-9 _.-]`.
fn valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-'))
}

/// Plugin-name lists in descriptors get spaces normalized to underscores,
/// matching `PluginDescriptionFile#makePluginNameList`.
fn name_list(values: OneOrMany<Scalar>) -> Vec<String> {
    values
        .into_vec()
        .into_iter()
        .map(|value| value.0.replace(' ', "_"))
        .collect()
}

fn string_list(values: OneOrMany<Scalar>) -> Vec<String> {
    values
        .into_vec()
        .into_iter()
        .map(Scalar::into_inner)
        .collect()
}

#[derive(Debug, Deserialize)]
struct RawCommand {
    description: Option<Scalar>,
    usage: Option<Scalar>,
    aliases: Option<OneOrMany<Scalar>>,
    permission: Option<Scalar>,
}

impl RawCommand {
    fn build(self) -> CommandDescription {
        CommandDescription {
            description: self.description.map(Scalar::into_inner),
            usage: self.usage.map(Scalar::into_inner),
            aliases: self.aliases.map(string_list).unwrap_or_default(),
            permission: self.permission.map(Scalar::into_inner),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawPermission {
    description: Option<Scalar>,
    default: Option<Scalar>,
    children: Option<BTreeMap<String, bool>>,
}

impl RawPermission {
    fn build(self, node: &str) -> Result<PermissionDescription, ValidationError> {
        let default = match self.default {
            Some(value) => PermissionDefault::parse(&value.0).ok_or(
                ValidationError::InvalidPermissionDefault {
                    node: node.to_owned(),
                    value: value.0,
                },
            )?,
            None => PermissionDefault::default(),
        };
        Ok(PermissionDescription {
            description: self.description.map(Scalar::into_inner),
            default,
            children: self.children.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawDependencies {
    bootstrap: Option<BTreeMap<String, RawDependency>>,
    server: Option<BTreeMap<String, RawDependency>>,
}

impl RawDependencies {
    fn build(self) -> Result<PaperDependencies, ValidationError> {
        Ok(PaperDependencies {
            bootstrap: build_dependency_map(self.bootstrap)?,
            server: build_dependency_map(self.server)?,
        })
    }
}

fn build_dependency_map(
    dependencies: Option<BTreeMap<String, RawDependency>>,
) -> Result<BTreeMap<String, PaperDependency>, ValidationError> {
    let mut out = BTreeMap::new();
    for (name, raw) in dependencies.unwrap_or_default() {
        let dependency = raw.build(&name)?;
        out.insert(name, dependency);
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct RawDependency {
    load: Option<Scalar>,
    required: Option<bool>,
    #[serde(rename = "join-classpath")]
    join_classpath: Option<bool>,
}

impl RawDependency {
    fn build(self, name: &str) -> Result<PaperDependency, ValidationError> {
        let load = match self.load {
            Some(value) => PaperDependencyLoad::parse(&value.0).ok_or(
                ValidationError::InvalidDependencyLoad {
                    dependency: name.to_owned(),
                    value: value.0,
                },
            )?,
            None => PaperDependencyLoad::default(),
        };
        Ok(PaperDependency {
            load,
            required: self.required.unwrap_or(true),
            join_classpath: self.join_classpath.unwrap_or(true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(yaml: &str) -> PluginMeta {
        parse(ManifestFormat::PluginYml, yaml).expect("descriptor should parse")
    }

    fn parse_err(yaml: &str) -> MetadataError {
        parse(ManifestFormat::PluginYml, yaml).expect_err("descriptor should be rejected")
    }

    const MINIMAL: &str = "name: Test\nversion: '1.0'\nmain: com.example.Test\n";

    #[test]
    fn parses_minimal_descriptor() {
        let meta = parse_ok(MINIMAL);
        assert_eq!(meta.name, "Test");
        assert_eq!(meta.version, "1.0");
        assert_eq!(meta.main, "com.example.Test");
        assert_eq!(meta.load, PluginLoadOrder::PostWorld);
        assert!(!meta.folia_supported);
        assert_eq!(
            meta.threading_mode(),
            crate::model::ThreadingMode::LegacyMainThread
        );
        assert!(meta.paper.is_none());
    }

    #[test]
    fn unquoted_scalars_are_coerced() {
        let meta =
            parse_ok("name: Test\nversion: 1.0\nmain: com.example.Test\napi-version: 1.21\n");
        assert_eq!(meta.version, "1.0");
        assert_eq!(meta.api_version.as_deref(), Some("1.21"));
    }

    #[test]
    fn name_spaces_are_normalized_like_bukkit() {
        let meta = parse_ok("name: My Plugin\nversion: '1'\nmain: com.example.Test\n");
        assert_eq!(meta.name, "My_Plugin");
    }

    #[test]
    fn author_and_authors_are_merged_author_first() {
        let meta = parse_ok(
            "name: Test\nversion: '1'\nmain: com.example.Test\nauthor: Alice\nauthors: [Bob, Carol]\n",
        );
        assert_eq!(meta.authors, ["Alice", "Bob", "Carol"]);
    }

    #[test]
    fn missing_required_fields_are_typed_errors() {
        assert_eq!(
            parse_err("version: '1'\nmain: com.example.Test\n"),
            MetadataError::Invalid(ValidationError::MissingName)
        );
        assert_eq!(
            parse_err("name: Test\nmain: com.example.Test\n"),
            MetadataError::Invalid(ValidationError::MissingVersion)
        );
        assert_eq!(
            parse_err("name: Test\nversion: '1'\n"),
            MetadataError::Invalid(ValidationError::MissingMain)
        );
    }

    #[test]
    fn invalid_name_characters_are_rejected() {
        assert_eq!(
            parse_err("name: Bad/Name\nversion: '1'\nmain: com.example.Test\n"),
            MetadataError::Invalid(ValidationError::InvalidName("Bad/Name".to_owned()))
        );
    }

    #[test]
    fn invalid_load_is_rejected() {
        assert_eq!(
            parse_err("name: Test\nversion: '1'\nmain: com.example.Test\nload: whenever\n"),
            MetadataError::Invalid(ValidationError::InvalidLoad {
                value: "whenever".to_owned()
            })
        );
    }

    #[test]
    fn permissions_default_to_op_and_reject_unknown_defaults() {
        let meta = parse_ok(
            "name: Test\nversion: '1'\nmain: com.example.Test\npermissions:\n  test.use:\n    default: true\n  test.other:\n    default: not op\n  test.plain: {}\n",
        );
        assert_eq!(
            meta.permissions["test.use"].default,
            PermissionDefault::True
        );
        assert_eq!(
            meta.permissions["test.other"].default,
            PermissionDefault::NotOp
        );
        assert_eq!(
            meta.permissions["test.plain"].default,
            PermissionDefault::Op
        );

        assert_eq!(
            parse_err(
                "name: Test\nversion: '1'\nmain: com.example.Test\npermissions:\n  test.use:\n    default: sometimes\n"
            ),
            MetadataError::Invalid(ValidationError::InvalidPermissionDefault {
                node: "test.use".to_owned(),
                value: "sometimes".to_owned(),
            })
        );
    }

    #[test]
    fn commands_keep_description_usage_aliases_and_permission() {
        let meta = parse_ok(
            "name: Test\nversion: '1'\nmain: com.example.Test\ncommands:\n  test:\n    description: Runs a test\n    usage: /test\n    aliases: [t, tester]\n    permission: test.use\n  bare:\n",
        );
        assert_eq!(
            meta.commands["test"],
            CommandDescription {
                description: Some("Runs a test".to_owned()),
                usage: Some("/test".to_owned()),
                aliases: vec!["t".to_owned(), "tester".to_owned()],
                permission: Some("test.use".to_owned()),
            }
        );
        assert_eq!(meta.commands["bare"], CommandDescription::default());
    }

    #[test]
    fn paper_descriptor_parses_dependencies_and_defaults() {
        let yaml = "name: Paper\nversion: '1'\nmain: com.example.Paper\napi-version: '1.21'\nbootstrapper: com.example.Boot\nloader: com.example.Loader\nhas-open-classloader: true\nfolia-supported: true\ndependencies:\n  server:\n    Before:\n      load: BEFORE\n      required: true\n    After:\n      load: AFTER\n      join-classpath: false\n    Plain: {}\n";
        let meta =
            parse(ManifestFormat::PaperPluginYml, yaml).expect("paper descriptor should parse");
        assert_eq!(meta.format, ManifestFormat::PaperPluginYml);
        assert!(meta.folia_supported);
        let paper = meta.paper.as_ref().expect("paper metadata");
        assert_eq!(paper.bootstrapper.as_deref(), Some("com.example.Boot"));
        assert_eq!(paper.loader.as_deref(), Some("com.example.Loader"));
        assert!(paper.has_open_classloader);
        let server = &paper.dependencies.server;
        assert_eq!(server["Before"].load, PaperDependencyLoad::Before);
        assert!(server["Before"].required && server["Before"].join_classpath);
        assert_eq!(server["After"].load, PaperDependencyLoad::After);
        assert!(!server["After"].join_classpath);
        assert_eq!(server["Plain"], PaperDependency::default());
        assert!(paper.dependencies.bootstrap.is_empty());
    }

    #[test]
    fn paper_dependency_with_invalid_load_is_rejected() {
        let yaml = "name: Paper\nversion: '1'\nmain: com.example.Paper\ndependencies:\n  server:\n    Other:\n      load: SIDEWAYS\n";
        assert_eq!(
            parse(ManifestFormat::PaperPluginYml, yaml),
            Err(MetadataError::Invalid(
                ValidationError::InvalidDependencyLoad {
                    dependency: "Other".to_owned(),
                    value: "SIDEWAYS".to_owned(),
                }
            ))
        );
    }

    #[test]
    fn plugin_yml_ignores_paper_sections() {
        let meta = parse_ok(
            "name: Test\nversion: '1'\nmain: com.example.Test\nbootstrapper: com.example.Boot\ndependencies:\n  server:\n    Other: {}\n",
        );
        assert!(meta.paper.is_none());
    }

    #[test]
    fn malformed_yaml_is_a_metadata_error() {
        assert!(matches!(
            parse_err("name: [unclosed\n"),
            MetadataError::Yaml(_)
        ));
        assert!(matches!(parse_err("just a string"), MetadataError::Yaml(_)));
    }
}
