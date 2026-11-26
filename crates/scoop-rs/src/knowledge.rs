use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path},
};

/// The version of the repository knowledge model emitted by Scooper.
pub const SCHEMA_VERSION: &str = "scooper.semantic.v1";

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Repository,
    Directory,
    Crate,
    Source,
    Symbol,
    Command,
    Test,
    Document,
    Workflow,
    Script,
    Manifest,
    File,
    Integration,
    Agent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceLocation {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SemanticObject {
    pub id: String,
    pub kind: ObjectKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceLocation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Contains,
    Defines,
    Documents,
    Tests,
    Builds,
    DependsOn,
    IntegratesWith,
    Specializes,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Relationship {
    pub kind: RelationKind,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentContext {
    pub matches: Vec<SemanticObject>,
    pub relationships: Vec<Relationship>,
}

/// A stable, serializable graph of semantic objects and their filesystem topology.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpatialKnowledgeGraph {
    pub schema: String,
    pub repository: String,
    pub objects: Vec<SemanticObject>,
    pub relationships: Vec<Relationship>,
}

impl SpatialKnowledgeGraph {
    pub fn new(repository: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_VERSION.to_owned(),
            repository: repository.into(),
            objects: Vec::new(),
            relationships: Vec::new(),
        }
    }

    /// Adds or replaces an object by its stable identifier.
    pub fn add_object(&mut self, object: SemanticObject) {
        match self.objects.iter().position(|item| item.id == object.id) {
            Some(index) => self.objects[index] = object,
            None => self.objects.push(object),
        }
    }

    /// Adds a relationship and rejects dangling or self-referential edges.
    pub fn connect(
        &mut self,
        kind: RelationKind,
        from: impl Into<String>,
        to: impl Into<String>,
    ) -> io::Result<()> {
        let from = from.into();
        let to = to.into();
        if from == to || !self.contains(&from) || !self.contains(&to) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid relationship {from} -> {to}"),
            ));
        }
        let relationship = Relationship { kind, from, to };
        if !self.relationships.contains(&relationship) {
            self.relationships.push(relationship);
        }
        Ok(())
    }

    pub fn object(&self, id: &str) -> Option<&SemanticObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.object(id).is_some()
    }

    /// Searches names, identifiers, paths, and attributes for agent context retrieval.
    pub fn search(&self, query: &str) -> Vec<&SemanticObject> {
        let query = query.to_lowercase();
        self.objects
            .iter()
            .filter(|object| {
                object.id.to_lowercase().contains(&query)
                    || object.name.to_lowercase().contains(&query)
                    || object
                        .location
                        .as_ref()
                        .is_some_and(|location| location.path.to_lowercase().contains(&query))
                    || object
                        .attributes
                        .values()
                        .any(|value| value.to_lowercase().contains(&query))
            })
            .collect()
    }

    /// Returns direct relationships and neighboring objects for semantic queries.
    pub fn agent_context(&self, query: &str) -> AgentContext {
        let matches = self.search(query).into_iter().cloned().collect::<Vec<_>>();
        let ids = matches
            .iter()
            .map(|object| object.id.as_str())
            .collect::<BTreeSet<_>>();
        let relationships = self
            .relationships
            .iter()
            .filter(|edge| ids.contains(edge.from.as_str()) || ids.contains(edge.to.as_str()))
            .cloned()
            .collect();
        AgentContext {
            matches,
            relationships,
        }
    }

    pub fn bitscoop_document(&self) -> &Self {
        self
    }

    /// Imports a Bitscoop-compatible Scooper semantic envelope.
    pub fn from_bitscoop(document: &[u8]) -> serde_json::Result<Self> {
        let graph: Self = serde_json::from_slice(document)?;
        if graph.schema != SCHEMA_VERSION {
            return Err(serde::de::Error::custom(format!(
                "unsupported semantic schema: {}",
                graph.schema
            )));
        }
        graph.validate().map_err(serde::de::Error::custom)?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), String> {
        let ids = self
            .objects
            .iter()
            .map(|object| object.id.as_str())
            .collect::<BTreeSet<_>>();
        if ids.len() != self.objects.len() {
            return Err("duplicate semantic object identifier".into());
        }
        if self.relationships.iter().any(|edge| {
            edge.from == edge.to
                || !ids.contains(edge.from.as_str())
                || !ids.contains(edge.to.as_str())
        }) {
            return Err("relationship references a missing object".into());
        }
        Ok(())
    }

    /// Indexes a repository without following symlinks or generated/vendor trees.
    pub fn discover(root: &Path, repository: impl Into<String>) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let mut graph = Self::new(repository);
        graph.add_object(SemanticObject {
            id: "repository:root".into(),
            kind: ObjectKind::Repository,
            name: root
                .file_name()
                .unwrap_or_else(|| root.as_os_str())
                .to_string_lossy()
                .into_owned(),
            location: Some(SourceLocation {
                path: ".".into(),
                line: None,
            }),
            attributes: BTreeMap::new(),
        });
        scan_directory(&root, &root, "repository:root", &mut graph)?;
        add_integrations(&mut graph);
        graph.objects.sort_by(|a, b| a.id.cmp(&b.id));
        graph.relationships.sort_by(|a, b| {
            (a.from.as_str(), &a.kind, a.to.as_str()).cmp(&(
                b.from.as_str(),
                &b.kind,
                b.to.as_str(),
            ))
        });
        Ok(graph)
    }
}

fn scan_directory(
    root: &Path,
    directory: &Path,
    parent_id: &str,
    graph: &mut SpatialKnowledgeGraph,
) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type()?;
        if name.starts_with('.') && name != ".github" || ignored_directory(&name, &file_type) {
            continue;
        }
        let relative = normalized_relative(root, &path);
        if file_type.is_dir() {
            let id = format!("directory:{relative}");
            graph.add_object(SemanticObject {
                id: id.clone(),
                kind: ObjectKind::Directory,
                name,
                location: Some(SourceLocation {
                    path: relative,
                    line: None,
                }),
                attributes: BTreeMap::new(),
            });
            graph.connect(RelationKind::Contains, parent_id, &id)?;
            scan_directory(root, &path, &id, graph)?;
        } else if file_type.is_file() {
            index_file(&path, parent_id, relative, graph)?;
        }
    }
    Ok(())
}

fn ignored_directory(name: &str, file_type: &fs::FileType) -> bool {
    file_type.is_dir()
        && matches!(
            name,
            "target" | "node_modules" | "dist" | "out" | "bin" | "obj"
        )
}

fn normalized_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn index_file(
    path: &Path,
    parent_id: &str,
    relative: String,
    graph: &mut SpatialKnowledgeGraph,
) -> io::Result<()> {
    let kind = classify_file(&relative);
    let id = format!("{:?}:{relative}", kind).to_lowercase();
    let mut attributes = BTreeMap::new();
    if kind == ObjectKind::Crate {
        attributes.insert("manifest".into(), "cargo".into());
    }
    graph.add_object(SemanticObject {
        id: id.clone(),
        kind: kind.clone(),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| relative.clone()),
        location: Some(SourceLocation {
            path: relative.clone(),
            line: None,
        }),
        attributes,
    });
    graph.connect(RelationKind::Contains, parent_id, &id)?;

    if kind == ObjectKind::Source {
        let source = fs::read_to_string(path)?;
        let mut next_symbol_is_test = false;
        for (offset, line) in source.lines().enumerate() {
            if line.trim() == "#[test]" {
                next_symbol_is_test = true;
                continue;
            }
            if let Some(name) = rust_symbol(line) {
                let symbol_id = format!("symbol:{relative}:{name}");
                graph.add_object(SemanticObject {
                    id: symbol_id.clone(),
                    kind: if next_symbol_is_test {
                        ObjectKind::Test
                    } else {
                        ObjectKind::Symbol
                    },
                    name,
                    location: Some(SourceLocation {
                        path: relative.clone(),
                        line: Some(offset + 1),
                    }),
                    attributes: BTreeMap::new(),
                });
                graph.connect(
                    if next_symbol_is_test {
                        RelationKind::Tests
                    } else {
                        RelationKind::Defines
                    },
                    &id,
                    &symbol_id,
                )?;
                next_symbol_is_test = false;
            } else if !line.trim().is_empty() {
                next_symbol_is_test = false;
            }
        }
    } else if kind == ObjectKind::Document {
        graph.connect(RelationKind::Documents, &id, "repository:root")?;
    }
    Ok(())
}

fn classify_file(path: &str) -> ObjectKind {
    if path.ends_with("Cargo.toml") {
        ObjectKind::Crate
    } else if path.starts_with(".github/workflows/") {
        ObjectKind::Workflow
    } else if path.ends_with(".rs") {
        ObjectKind::Source
    } else if path.ends_with(".md") {
        ObjectKind::Document
    } else if path.ends_with(".ps1") || path.ends_with(".tape") {
        ObjectKind::Script
    } else if path.ends_with(".json") {
        ObjectKind::Manifest
    } else {
        ObjectKind::File
    }
}

fn rust_symbol(line: &str) -> Option<String> {
    let line = line.trim_start();
    let line = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub(super) "))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    ["fn ", "struct ", "enum ", "trait ", "type ", "mod "]
        .iter()
        .find_map(|prefix| {
            line.strip_prefix(prefix).map(|rest| {
                rest.chars()
                    .take_while(|character| character.is_alphanumeric() || *character == '_')
                    .collect()
            })
        })
}

fn add_integrations(graph: &mut SpatialKnowledgeGraph) {
    for (id, name, relation) in [
        (
            "integration:bitscoop",
            "DarbotLM/bitscoop",
            RelationKind::IntegratesWith,
        ),
        (
            "agent:scooper-swe",
            "Scooper specialized SWE agent",
            RelationKind::Specializes,
        ),
    ] {
        graph.add_object(SemanticObject {
            id: id.into(),
            kind: if id.starts_with("agent:") {
                ObjectKind::Agent
            } else {
                ObjectKind::Integration
            },
            name: name.into(),
            location: None,
            attributes: BTreeMap::from([("schema".into(), SCHEMA_VERSION.into())]),
        });
        let _ = graph.connect(relation, "repository:root", id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_spatial_and_symbol_relationships() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let graph = SpatialKnowledgeGraph::discover(root, "DarbotLM/scooper").unwrap();
        let source = graph
            .search("crates/scoop-rs/src/knowledge.rs")
            .first()
            .copied()
            .unwrap();
        assert!(graph
            .relationships
            .iter()
            .any(|edge| edge.to == source.id && edge.kind == RelationKind::Contains));
        assert!(graph.search("rust_symbol").iter().any(|object| {
            object
                .location
                .as_ref()
                .and_then(|location| location.line)
                .is_some()
        }));
        assert!(graph.search("agent_context_includes_direct_evidence").len() == 1);
    }

    #[test]
    fn exports_and_imports_bitscoop_envelope() {
        let mut graph = SpatialKnowledgeGraph::new("example");
        graph.add_object(SemanticObject {
            id: "repository:root".into(),
            kind: ObjectKind::Repository,
            name: "example".into(),
            location: None,
            attributes: BTreeMap::new(),
        });
        let json = serde_json::to_vec(graph.bitscoop_document()).unwrap();
        let imported = SpatialKnowledgeGraph::from_bitscoop(&json).unwrap();
        assert_eq!(imported.repository, graph.repository);
        assert!(graph
            .connect(RelationKind::Contains, "repository:root", "missing")
            .is_err());
    }

    #[test]
    fn agent_context_includes_direct_evidence() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let context = SpatialKnowledgeGraph::discover(root, "DarbotLM/scooper")
            .unwrap()
            .agent_context("scooper specialized swe");
        assert!(!context.matches.is_empty());
        assert!(!context.relationships.is_empty());
    }
}
