use std::collections::{HashMap, HashSet};

use serde::Serialize;

use grapha_core::graph::{Graph, Node, NodeKind, NodeRole, TerminalKind};

use super::flow::{is_dataflow_edge, terminal_kind_to_string};
use super::{QueryResolveError, SymbolRef, normalize_symbol_name, strip_accessor_prefix};

#[derive(Debug, Serialize)]
pub struct OriginResult {
    pub symbol: String,
    pub origins: Vec<OriginPath>,
    pub total_origins: usize,
    #[serde(skip)]
    pub(crate) target_ref: SymbolRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct OriginPath {
    pub api: SymbolRef,
    pub terminal_kind: String,
    pub path: Vec<String>,
    pub field_candidates: Vec<String>,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

struct StackFrame<'a> {
    node_id: &'a str,
    path_ids: Vec<&'a str>,
    visited: HashSet<&'a str>,
}

fn to_symbol_ref(node: &Node) -> SymbolRef {
    SymbolRef::from_node(node)
}

fn is_network_terminal(node: &Node) -> bool {
    matches!(
        node.role,
        Some(NodeRole::Terminal {
            kind: TerminalKind::Network
        })
    )
}

fn fieldish_name(node: &Node) -> Option<String> {
    match node.kind {
        NodeKind::Property | NodeKind::Field | NodeKind::Constant => {
            let name = normalize_symbol_name(&node.name).trim();
            (!name.is_empty() && name != "body").then(|| name.to_string())
        }
        NodeKind::Function => {
            let stripped = strip_accessor_prefix(&node.name);
            let normalized = normalize_symbol_name(stripped).trim();
            (stripped != node.name && !normalized.is_empty() && normalized != "body")
                .then(|| normalized.to_string())
        }
        _ => None,
    }
}

fn candidate_field_paths(path_nodes: &[&Node]) -> Vec<String> {
    let mut names = Vec::new();
    for node in path_nodes {
        if let Some(name) = fieldish_name(node)
            && names.last() != Some(&name)
        {
            names.push(name);
        }
    }

    let mut candidates = Vec::new();
    if !names.is_empty() {
        candidates.push(names.join("."));
        if names.len() > 1 {
            candidates.push(names[names.len() - 2..].join("."));
        }
        candidates.push(names[names.len() - 1].clone());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn path_display(path_nodes: &[&Node]) -> Vec<String> {
    path_nodes.iter().map(|node| node.name.clone()).collect()
}

fn confidence_for(path_nodes: &[&Node], field_candidates: &[String]) -> f32 {
    let mut confidence = 0.35f32;
    if !field_candidates.is_empty() {
        confidence += 0.25;
    }
    if path_nodes.len() <= 6 {
        confidence += 0.15;
    }
    if path_nodes
        .iter()
        .any(|node| matches!(node.kind, NodeKind::Property | NodeKind::Field))
    {
        confidence += 0.15;
    }
    if path_nodes.iter().any(|node| is_network_terminal(node)) {
        confidence += 0.1;
    }
    confidence.min(0.95)
}

fn notes_for(path_nodes: &[&Node], field_candidates: &[String]) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(network_node) = path_nodes.iter().find(|node| is_network_terminal(node)) {
        notes.push(format!("reached network terminal {}", network_node.name));
    }
    if let Some(candidate) = field_candidates.first() {
        notes.push(format!("candidate field path {}", candidate));
    }
    if path_nodes.iter().any(|node| {
        node.kind == NodeKind::Function && strip_accessor_prefix(&node.name) != node.name
    }) {
        notes.push("path crosses accessor/computed-property logic".to_string());
    }
    notes
}

pub fn query_origin(
    graph: &Graph,
    symbol: &str,
    max_depth: usize,
) -> Result<OriginResult, QueryResolveError> {
    let target_node = crate::query::resolve_node(&graph.nodes, symbol)?;
    let node_index: HashMap<&str, &Node> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut forward_adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if is_dataflow_edge(edge.kind)
            && node_index.contains_key(edge.source.as_str())
            && node_index.contains_key(edge.target.as_str())
        {
            forward_adj
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }

    let mut stack = vec![StackFrame {
        node_id: target_node.id.as_str(),
        path_ids: vec![target_node.id.as_str()],
        visited: HashSet::from([target_node.id.as_str()]),
    }];
    let mut origins = Vec::new();
    let mut seen_api_ids = HashSet::new();

    while let Some(frame) = stack.pop() {
        let Some(node) = node_index.get(frame.node_id).copied() else {
            continue;
        };

        if frame.path_ids.len() > 1
            && is_network_terminal(node)
            && seen_api_ids.insert(node.id.as_str())
        {
            let path_nodes: Vec<&Node> = frame
                .path_ids
                .iter()
                .filter_map(|node_id| node_index.get(*node_id).copied())
                .collect();
            let field_candidates = candidate_field_paths(&path_nodes);
            let confidence = confidence_for(&path_nodes, &field_candidates);
            let notes = notes_for(&path_nodes, &field_candidates);
            origins.push(OriginPath {
                api: to_symbol_ref(node),
                terminal_kind: terminal_kind_to_string(&TerminalKind::Network),
                path: path_display(&path_nodes),
                field_candidates,
                confidence,
                notes,
            });
            continue;
        }

        if frame.path_ids.len() > max_depth + 1 {
            continue;
        }

        if let Some(targets) = forward_adj.get(frame.node_id) {
            for target_id in targets {
                if frame.visited.contains(target_id) {
                    continue;
                }
                let mut visited = frame.visited.clone();
                visited.insert(target_id);
                let mut path_ids = frame.path_ids.clone();
                path_ids.push(target_id);
                stack.push(StackFrame {
                    node_id: target_id,
                    path_ids,
                    visited,
                });
            }
        }
    }

    origins.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.api.name.cmp(&right.api.name))
            .then_with(|| left.path.len().cmp(&right.path.len()))
    });

    Ok(OriginResult {
        symbol: target_node.id.clone(),
        total_origins: origins.len(),
        origins,
        target_ref: SymbolRef::from_node(target_node),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use grapha_core::graph::{Edge, EdgeKind, FlowDirection, NodeKind, Span, Visibility};
    use std::collections::HashMap as StdHashMap;
    use std::path::PathBuf;

    fn node(id: &str, name: &str, kind: NodeKind, role: Option<NodeRole>) -> Node {
        Node {
            id: id.into(),
            kind,
            name: name.into(),
            file: PathBuf::from("test.swift"),
            span: Span {
                start: [0, 0],
                end: [1, 0],
            },
            visibility: Visibility::Public,
            metadata: StdHashMap::new(),
            role,
            signature: None,
            doc_comment: None,
            module: Some("App".into()),
            snippet: None,
        }
    }

    fn edge(source: &str, target: &str, kind: EdgeKind) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            kind,
            confidence: 1.0,
            direction: Some(FlowDirection::Read),
            operation: None,
            condition: None,
            async_boundary: None,
            provenance: Vec::new(),
        }
    }

    #[test]
    fn origin_finds_network_terminal_and_field_candidates() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                node("ui::titleText", "titleText", NodeKind::Property, None),
                node("vm::displayName", "displayName", NodeKind::Property, None),
                node("model::nickname", "nickname", NodeKind::Property, None),
                node(
                    "api::fetchProfile",
                    "fetchProfile",
                    NodeKind::Function,
                    Some(NodeRole::Terminal {
                        kind: TerminalKind::Network,
                    }),
                ),
            ],
            edges: vec![
                edge("ui::titleText", "vm::displayName", EdgeKind::Reads),
                edge("vm::displayName", "model::nickname", EdgeKind::Reads),
                edge("model::nickname", "api::fetchProfile", EdgeKind::Reads),
            ],
        };

        let result = query_origin(&graph, "titleText", 10).unwrap();
        assert_eq!(result.total_origins, 1);
        let origin = &result.origins[0];
        assert_eq!(origin.api.name, "fetchProfile");
        assert!(
            origin
                .field_candidates
                .iter()
                .any(|v| v.contains("nickname"))
        );
        assert_eq!(
            origin.path,
            vec!["titleText", "displayName", "nickname", "fetchProfile"]
        );
    }
}
