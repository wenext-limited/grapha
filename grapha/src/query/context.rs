use std::collections::HashMap;

use grapha_core::graph::{EdgeKind, Graph};

use super::{ContextResult, QueryResolveError, SymbolInfo, SymbolRef};

fn to_symbol_ref(node: &grapha_core::graph::Node) -> SymbolRef {
    SymbolRef {
        id: node.id.clone(),
        name: node.name.clone(),
        kind: node.kind,
        file: node.file.to_string_lossy().to_string(),
    }
}

fn sort_refs_by_name(symbols: &mut [SymbolRef]) {
    symbols.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn sort_ids_by_span<'a>(
    node_ids: &mut [&'a str],
    node_index: &HashMap<&'a str, &'a grapha_core::graph::Node>,
) {
    node_ids.sort_by(
        |left, right| match (node_index.get(*left), node_index.get(*right)) {
            (Some(left_node), Some(right_node)) => left_node
                .span
                .start
                .cmp(&right_node.span.start)
                .then_with(|| left_node.span.end.cmp(&right_node.span.end))
                .then_with(|| left_node.name.cmp(&right_node.name))
                .then_with(|| left_node.id.cmp(&right_node.id)),
            _ => left.cmp(right),
        },
    );
}

pub fn query_context(graph: &Graph, query: &str) -> Result<ContextResult, QueryResolveError> {
    let node = crate::query::resolve_node(&graph.nodes, query)?;

    let node_index: HashMap<&str, &grapha_core::graph::Node> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut callers = Vec::new();
    let mut callees = Vec::new();
    let mut contains_ids = Vec::new();
    let mut contained_by_ids = Vec::new();
    let mut implementors = Vec::new();
    let mut implements = Vec::new();
    let mut type_refs = Vec::new();

    for edge in &graph.edges {
        if edge.source == node.id
            && let Some(target) = node_index.get(edge.target.as_str())
        {
            let sym_ref = to_symbol_ref(target);
            match edge.kind {
                EdgeKind::Calls => callees.push(sym_ref),
                EdgeKind::Contains => contains_ids.push(target.id.as_str()),
                EdgeKind::Implements => implements.push(sym_ref),
                EdgeKind::TypeRef => type_refs.push(sym_ref),
                _ => {}
            }
        }
        if edge.target == node.id
            && let Some(source) = node_index.get(edge.source.as_str())
        {
            let sym_ref = to_symbol_ref(source);
            match edge.kind {
                EdgeKind::Calls => callers.push(sym_ref),
                EdgeKind::Contains => contained_by_ids.push(source.id.as_str()),
                EdgeKind::Implements => implementors.push(sym_ref),
                _ => {}
            }
        }
    }

    sort_refs_by_name(&mut callers);
    sort_refs_by_name(&mut callees);
    sort_refs_by_name(&mut implementors);
    sort_refs_by_name(&mut implements);
    sort_refs_by_name(&mut type_refs);
    sort_ids_by_span(&mut contains_ids, &node_index);
    sort_ids_by_span(&mut contained_by_ids, &node_index);

    let contains = contains_ids
        .into_iter()
        .filter_map(|node_id| node_index.get(node_id).copied())
        .map(to_symbol_ref)
        .collect();
    let contained_by = contained_by_ids
        .into_iter()
        .filter_map(|node_id| node_index.get(node_id).copied())
        .map(to_symbol_ref)
        .collect();

    Ok(ContextResult {
        symbol: SymbolInfo {
            id: node.id.clone(),
            name: node.name.clone(),
            kind: node.kind,
            file: node.file.to_string_lossy().to_string(),
            span: [node.span.start[0], node.span.end[0]],
        },
        callers,
        callees,
        contains,
        contained_by,
        implementors,
        implements,
        type_refs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use grapha_core::graph::*;
    use std::collections::HashMap as StdHashMap;

    fn make_graph() -> Graph {
        Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                Node {
                    id: "a.rs::main".into(),
                    kind: NodeKind::Function,
                    name: "main".into(),
                    file: "a.rs".into(),
                    span: Span {
                        start: [0, 0],
                        end: [10, 0],
                    },
                    visibility: Visibility::Public,
                    metadata: StdHashMap::new(),
                    role: None,
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
                Node {
                    id: "a.rs::helper".into(),
                    kind: NodeKind::Function,
                    name: "helper".into(),
                    file: "a.rs".into(),
                    span: Span {
                        start: [12, 0],
                        end: [15, 0],
                    },
                    visibility: Visibility::Public,
                    metadata: StdHashMap::new(),
                    role: None,
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
            ],
            edges: vec![Edge {
                source: "a.rs::main".into(),
                target: "a.rs::helper".into(),
                kind: EdgeKind::Calls,
                confidence: 0.9,
                direction: None,
                operation: None,
                condition: None,
                async_boundary: None,
            }],
        }
    }

    #[test]
    fn context_finds_callers_and_callees() {
        let graph = make_graph();
        let ctx = query_context(&graph, "main").unwrap();
        assert_eq!(ctx.callees.len(), 1);
        assert_eq!(ctx.callees[0].name, "helper");
        assert_eq!(ctx.callers.len(), 0);

        let ctx2 = query_context(&graph, "helper").unwrap();
        assert_eq!(ctx2.callers.len(), 1);
        assert_eq!(ctx2.callers[0].name, "main");
    }

    #[test]
    fn context_includes_structural_relationships_in_span_order() {
        let graph = Graph {
            version: "0.1.0".to_string(),
            nodes: vec![
                Node {
                    id: "view.swift::ContentView".into(),
                    kind: NodeKind::Struct,
                    name: "ContentView".into(),
                    file: "view.swift".into(),
                    span: Span {
                        start: [0, 0],
                        end: [20, 0],
                    },
                    visibility: Visibility::Public,
                    metadata: StdHashMap::new(),
                    role: None,
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
                Node {
                    id: "view.swift::ContentView::body".into(),
                    kind: NodeKind::Property,
                    name: "body".into(),
                    file: "view.swift".into(),
                    span: Span {
                        start: [2, 4],
                        end: [12, 4],
                    },
                    visibility: Visibility::Public,
                    metadata: StdHashMap::new(),
                    role: Some(NodeRole::EntryPoint),
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
                Node {
                    id: "view.swift::ContentView::body::view:VStack@3:8".into(),
                    kind: NodeKind::View,
                    name: "VStack".into(),
                    file: "view.swift".into(),
                    span: Span {
                        start: [3, 8],
                        end: [10, 9],
                    },
                    visibility: Visibility::Private,
                    metadata: StdHashMap::new(),
                    role: None,
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
                Node {
                    id: "view.swift::ContentView::body::view:Text@4:12".into(),
                    kind: NodeKind::View,
                    name: "Text".into(),
                    file: "view.swift".into(),
                    span: Span {
                        start: [4, 12],
                        end: [4, 25],
                    },
                    visibility: Visibility::Private,
                    metadata: StdHashMap::new(),
                    role: None,
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
                Node {
                    id: "view.swift::ContentView::body::view:Row@5:12".into(),
                    kind: NodeKind::View,
                    name: "Row".into(),
                    file: "view.swift".into(),
                    span: Span {
                        start: [5, 12],
                        end: [5, 28],
                    },
                    visibility: Visibility::Private,
                    metadata: StdHashMap::new(),
                    role: None,
                    signature: None,
                    doc_comment: None,
                    module: None,
                },
            ],
            edges: vec![
                Edge {
                    source: "view.swift::ContentView".into(),
                    target: "view.swift::ContentView::body".into(),
                    kind: EdgeKind::Contains,
                    confidence: 1.0,
                    direction: None,
                    operation: None,
                    condition: None,
                    async_boundary: None,
                },
                Edge {
                    source: "view.swift::ContentView::body".into(),
                    target: "view.swift::ContentView::body::view:Row@5:12".into(),
                    kind: EdgeKind::Contains,
                    confidence: 1.0,
                    direction: None,
                    operation: None,
                    condition: None,
                    async_boundary: None,
                },
                Edge {
                    source: "view.swift::ContentView::body".into(),
                    target: "view.swift::ContentView::body::view:Text@4:12".into(),
                    kind: EdgeKind::Contains,
                    confidence: 1.0,
                    direction: None,
                    operation: None,
                    condition: None,
                    async_boundary: None,
                },
                Edge {
                    source: "view.swift::ContentView::body".into(),
                    target: "view.swift::ContentView::body::view:VStack@3:8".into(),
                    kind: EdgeKind::Contains,
                    confidence: 1.0,
                    direction: None,
                    operation: None,
                    condition: None,
                    async_boundary: None,
                },
            ],
        };

        let ctx = query_context(&graph, "body").unwrap();
        assert_eq!(ctx.contains.len(), 3);
        assert_eq!(
            ctx.contains
                .iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            vec!["VStack", "Text", "Row"]
        );
        assert_eq!(ctx.contained_by.len(), 1);
        assert_eq!(ctx.contained_by[0].name, "ContentView");
    }

    #[test]
    fn context_returns_none_for_unknown() {
        let graph = make_graph();
        assert!(matches!(
            query_context(&graph, "nonexistent"),
            Err(QueryResolveError::NotFound { .. })
        ));
    }
}
