//! The variable aggregator: an ordered pass-through that collapses mutually exclusive branch
//! outputs into one variable.
//!
//! V1 contract (frozen): selectors are an ordered, non-empty list; the node passes the first
//! **assigned** selector's pool value through unchanged as `{node_id}.output`. Assigned means
//! the variable's key exists in the pool — never a truthiness check, so `null`, `false`, `0`,
//! `""`, `[]`, and `{}` all hit. Zero assigned selectors is a stable failure. The node never
//! reads branch decisions and does not know Condition exists; branch selection is expressed by
//! the scheduler's readiness (inactive branch edges) and pool facts alone.

use super::variable_pool::{VariableSelector, WorkflowVariablePool, WorkflowVariablePoolError};
use serde::Deserialize;
use std::fmt;
use thiserror::Error;

/// Wire shape of the `data.aggregatorConfig` field on an aggregator node.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireAggregatorConfig {
    #[serde(default)]
    variables: Vec<Vec<String>>,
}

/// The executable contract of an `aggregator` node.
///
/// Selectors keep their declaration order, which is the priority order for multiple assigned
/// candidates (first-assigned-wins). Only root variables are selectable: nested paths cannot be
/// statically typed against the catalog, which the type-equality rule depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatorConfig {
    pub variables: Vec<VariableSelector>,
}

/// Failures raised while compiling or executing an aggregator node.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AggregatorError {
    #[error("aggregator node has no config")]
    MissingConfig,
    #[error("aggregator node must declare at least one selector")]
    EmptySelectors,
    #[error("aggregator selector {selector:?} is not a node id and root variable")]
    InvalidSelector { selector: Vec<String> },
    #[error("aggregator selector {selector} must reference a root variable, not a nested path")]
    NestedSelector { selector: String },
    #[error("aggregator declares duplicate selector {selector}")]
    DuplicateSelector { selector: String },
    #[error("aggregator found no assigned variable among: {selectors}")]
    NoAssignedVariable { selectors: String },
    #[error(transparent)]
    Pool(#[from] WorkflowVariablePoolError),
}

impl fmt::Display for AggregatorConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "[{}]",
            self.variables
                .iter()
                .map(VariableSelector::qualified)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl AggregatorConfig {
    /// Compiles the wire `aggregatorConfig` into an executable config, rejecting empty lists,
    /// unparseable selectors, nested paths, and duplicates.
    pub fn from_wire(wire: Option<WireAggregatorConfig>) -> Result<Self, AggregatorError> {
        let wire = wire.ok_or(AggregatorError::MissingConfig)?;
        if wire.variables.is_empty() {
            return Err(AggregatorError::EmptySelectors);
        }
        let mut variables = Vec::with_capacity(wire.variables.len());
        for parts in wire.variables {
            let selector = VariableSelector::try_from_parts(&parts).ok_or_else(|| {
                AggregatorError::InvalidSelector {
                    selector: parts.clone(),
                }
            })?;
            if !selector.nested.is_empty() {
                return Err(AggregatorError::NestedSelector {
                    selector: selector.qualified(),
                });
            }
            let qualified = selector.qualified();
            if variables
                .iter()
                .any(|existing: &VariableSelector| existing.qualified() == qualified)
            {
                return Err(AggregatorError::DuplicateSelector {
                    selector: qualified,
                });
            }
            variables.push(selector);
        }
        Ok(Self { variables })
    }

    /// Returns the first assigned selector's pool value in declaration order.
    ///
    /// Assigned is key existence: `Ok(None)` from the pool means declared-but-unassigned and the
    /// scan continues, while a pool read error is a hard failure. Every selector unassigned is
    /// the stable no-match failure; the message lists every scanned selector so authors can see
    /// which candidates were considered.
    pub fn select_output(
        &self,
        pool: &WorkflowVariablePool,
    ) -> Result<serde_json::Value, AggregatorError> {
        for selector in &self.variables {
            if let Some(value) = pool.resolve(selector)? {
                return Ok(value.clone());
            }
        }
        Err(AggregatorError::NoAssignedVariable {
            selectors: self.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Builds a pool with the given declared selector types and assigned values.
    fn pool_with(
        declarations: &[(&str, &str)],
        assigned: &[(&str, serde_json::Value)],
    ) -> WorkflowVariablePool {
        let mut pool = WorkflowVariablePool::default();
        for (selector, value_type) in declarations {
            pool.declare(
                selector,
                value_type,
                selector.split('.').next().unwrap_or(""),
            );
        }
        for (selector, value) in assigned {
            let writer = selector.split('.').next().unwrap_or("");
            pool.set(selector, writer, value.clone()).unwrap();
        }
        pool
    }

    /// Builds a config from fully qualified selector strings.
    fn config(selectors: &[&str]) -> AggregatorConfig {
        AggregatorConfig::from_wire(Some(WireAggregatorConfig {
            variables: selectors
                .iter()
                .map(|selector| {
                    selector
                        .split('.')
                        .map(str::to_string)
                        .collect::<Vec<String>>()
                })
                .collect(),
        }))
        .unwrap()
    }

    /// An empty selector list is rejected before any runtime work.
    #[test]
    fn rejects_an_empty_selector_list() {
        assert_eq!(
            AggregatorConfig::from_wire(Some(WireAggregatorConfig {
                variables: Vec::new()
            })),
            Err(AggregatorError::EmptySelectors)
        );
        assert_eq!(
            AggregatorConfig::from_wire(None),
            Err(AggregatorError::MissingConfig)
        );
    }

    /// Nested paths and duplicates are rejected at compile time.
    #[test]
    fn rejects_nested_and_duplicate_selectors() {
        assert_eq!(
            AggregatorConfig::from_wire(Some(WireAggregatorConfig {
                variables: vec![vec!["a".into(), "output".into(), "leaf".into()]],
            })),
            Err(AggregatorError::NestedSelector {
                selector: "a.output".into()
            })
        );
        assert_eq!(
            AggregatorConfig::from_wire(Some(WireAggregatorConfig {
                variables: vec![
                    vec!["a".into(), "output".into()],
                    vec!["a".into(), "output".into()]
                ],
            })),
            Err(AggregatorError::DuplicateSelector {
                selector: "a.output".into()
            })
        );
    }

    /// The first assigned selector wins even when a later one is also assigned.
    #[test]
    fn first_assigned_selector_wins() {
        let pool = pool_with(
            &[("a.output", "string"), ("b.output", "string")],
            &[("a.output", json!("from-a")), ("b.output", json!("from-b"))],
        );
        assert_eq!(
            config(&["a.output", "b.output"]).select_output(&pool),
            Ok(json!("from-a"))
        );
        assert_eq!(
            config(&["b.output", "a.output"]).select_output(&pool),
            Ok(json!("from-b"))
        );
    }

    /// Declared-but-unassigned candidates are skipped; a later assigned candidate hits.
    #[test]
    fn skips_unassigned_candidates() {
        let pool = pool_with(
            &[("a.output", "string"), ("b.output", "string")],
            &[("b.output", json!("from-b"))],
        );
        assert_eq!(
            config(&["a.output", "b.output"]).select_output(&pool),
            Ok(json!("from-b"))
        );
    }

    /// Falsy business values are assigned: the key exists, so they hit and pass through.
    #[test]
    fn falsy_values_are_assigned() {
        let cases: Vec<(&str, &str, serde_json::Value)> = vec![
            ("false", "boolean", json!(false)),
            ("zero", "number", json!(0)),
            ("empty", "string", json!("")),
            ("empty-array", "array[string]", json!([])),
            ("empty-object", "object", json!({})),
            ("nullable", "any", serde_json::Value::Null),
        ];
        for (node, value_type, value) in cases {
            let pool = pool_with(
                &[(&format!("{node}.flag"), value_type)],
                &[(&format!("{node}.flag"), value.clone())],
            );
            assert_eq!(
                config(&[&format!("{node}.flag")]).select_output(&pool),
                Ok(value),
                "{node} must count as assigned"
            );
        }
    }

    /// Every selector unassigned is the stable no-match failure naming all candidates.
    #[test]
    fn no_assigned_selector_fails_naming_every_candidate() {
        let pool = pool_with(&[("a.output", "string"), ("b.output", "string")], &[]);
        assert_eq!(
            config(&["a.output", "b.output"]).select_output(&pool),
            Err(AggregatorError::NoAssignedVariable {
                selectors: "[a.output, b.output]".into()
            })
        );
    }

    /// An undeclared candidate fails closed instead of being skipped.
    #[test]
    fn undeclared_candidate_fails_closed() {
        let pool = pool_with(&[("a.output", "string")], &[]);
        assert_eq!(
            config(&["a.output", "ghost.output"]).select_output(&pool),
            Err(AggregatorError::Pool(
                WorkflowVariablePoolError::Undeclared {
                    selector: "ghost.output".into()
                }
            ))
        );
    }
}

#[cfg(test)]
mod parse_tests {
    use crate::workflow_run::engine::graph::{GraphError, WorkflowGraph};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    /// Parses a JSON value as a frozen workflow graph, failing the test on error.
    fn parse(value: serde_json::Value) -> Result<WorkflowGraph, GraphError> {
        WorkflowGraph::parse(&value.to_string())
    }

    /// The canonical branch-join fixture: condition routes to two sibling agents whose outputs
    /// feed one aggregator. First/second control the selector declaration order.
    fn join_graph(selector_order: [&str; 2]) -> serde_json::Value {
        let selector = |node: &str| vec![node.to_string(), "output".to_string()];
        let variables = match selector_order {
            ["a", "b"] => vec![selector("a"), selector("b")],
            _ => vec![selector("b"), selector("a")],
        };
        json!({
            "nodes": [
                {"id":"start","data":{"kind":"start"}},
                {"id":"cond","data":{"kind":"condition","cases":[
                    {"id":"yes","logic":"and","conditions":[
                        {"variableSelector":["start","input"],"operator":"is","value":"a"}]}]}},
                {"id":"a","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"a"}}},
                {"id":"b","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"b"}}},
                {"id":"agg","data":{"kind":"aggregator","aggregatorConfig":{"variables":variables}}},
                {"id":"out","data":{"kind":"output","outputs":[{"name":"text","variableSelector":["agg","output"]}]}}
            ],
            "edges": [
                {"source":"start","target":"cond"},
                {"source":"cond","sourceHandle":"yes","target":"a"},
                {"source":"cond","sourceHandle":"else","target":"b"},
                {"source":"a","target":"agg"},
                {"source":"b","target":"agg"},
                {"source":"agg","target":"out"}
            ]
        })
    }

    /// Two sibling-branch sources parse: each is a transitive predecessor via its own edge, and
    /// the aggregator's output is declared with the shared type.
    #[test]
    fn accepts_sibling_branch_sources_and_declares_the_common_type() {
        let graph = parse(join_graph(["a", "b"])).unwrap();
        let aggregator = graph.node("agg").unwrap();
        let selectors: Vec<String> = aggregator
            .aggregator_config
            .as_ref()
            .unwrap()
            .variables
            .iter()
            .map(|selector| selector.qualified())
            .collect();
        assert_eq!(selectors, vec!["a.output", "b.output"]);

        let pool = super::super::variable_pool::WorkflowVariablePool::from_graph(&graph);
        let definition = pool.catalog.get("agg.output").unwrap();
        assert_eq!(definition.value_type, "string");
        assert_eq!(definition.writer, "agg");
    }

    /// Declaration order survives the parse unchanged: it is the priority contract.
    #[test]
    fn keeps_the_declared_selector_order_through_parse() {
        for order in [["a", "b"], ["b", "a"]] {
            let graph = parse(join_graph(order)).unwrap();
            let selectors: Vec<String> = graph
                .node("agg")
                .unwrap()
                .aggregator_config
                .as_ref()
                .unwrap()
                .variables
                .iter()
                .map(|selector| selector.qualified())
                .collect();
            let first = format!("{}.output", order[0]);
            let second = format!("{}.output", order[1]);
            assert_eq!(selectors, vec![first, second]);
        }
    }

    /// Mixed candidate types are rejected at parse; the aggregator output type must be static.
    #[test]
    fn rejects_type_mismatched_selectors() {
        let mut graph = join_graph(["a", "b"]);
        graph["nodes"][4]["data"]["aggregatorConfig"]["variables"] =
            json!([["a", "output"], ["start", "limit"]]);
        graph["nodes"][0]["data"]["inputVariables"] =
            json!([{"name":"limit","valueType":"number"}]);
        assert_eq!(
            parse(graph).unwrap_err(),
            GraphError::InvalidAggregator {
                node_id: "agg".to_string(),
                reason: "aggregator selector start.limit has type number, but the aggregation requires string"
                    .to_string(),
            }
        );
    }

    /// A selector referencing a downstream node is rejected: its variable can never be
    /// assigned when the aggregator runs.
    #[test]
    fn rejects_downstream_selectors() {
        let mut graph = join_graph(["a", "b"]);
        graph["nodes"][4]["data"]["aggregatorConfig"]["variables"] =
            json!([["a", "output"], ["out", "output"]]);
        let error = parse(graph).unwrap_err();
        assert!(
            error.to_string().contains("not a transitive predecessor"),
            "unexpected error: {error}"
        );
    }

    /// A selector referencing an unrelated node (reachable but not upstream of the aggregator)
    /// is rejected too.
    #[test]
    fn rejects_unrelated_selectors() {
        let mut graph = join_graph(["a", "b"]);
        graph["nodes"].as_array_mut().unwrap().push(json!(
            {"id":"stray","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"stray"}}}
        ));
        graph["edges"].as_array_mut().unwrap().push(json!(
            {"source":"start","target":"stray"}
        ));
        graph["nodes"][4]["data"]["aggregatorConfig"]["variables"] =
            json!([["a", "output"], ["stray", "output"]]);
        let error = parse(graph).unwrap_err();
        assert!(
            error.to_string().contains("not a transitive predecessor"),
            "unexpected error: {error}"
        );
    }

    /// A selector referencing the aggregator itself is rejected (not in its own upstream).
    #[test]
    fn rejects_self_referencing_selectors() {
        let mut graph = join_graph(["a", "b"]);
        graph["nodes"][4]["data"]["aggregatorConfig"]["variables"] =
            json!([["a", "output"], ["agg", "output"]]);
        let error = parse(graph).unwrap_err();
        assert!(
            error.to_string().contains("not a transitive predecessor"),
            "unexpected error: {error}"
        );
    }

    /// A global variable is a legal candidate alongside node outputs (type equality applies).
    #[test]
    fn accepts_global_variables_as_candidates() {
        let mut graph = join_graph(["a", "b"]);
        graph["globalVariables"] =
            json!([{"name":"summary.title","valueType":"string","value":""}]);
        graph["nodes"][4]["data"]["aggregatorConfig"]["variables"] =
            json!([["summary", "title"], ["a", "output"]]);
        let graph = parse(graph).unwrap();
        let pool = super::super::variable_pool::WorkflowVariablePool::from_graph(&graph);
        assert_eq!(pool.catalog.get("agg.output").unwrap().value_type, "string");
    }

    /// An aggregator that is unreachable from start never runs, so its config still must
    /// compile but the unused-node reference rule wins for spare producers.
    #[test]
    fn rejects_selectors_referencing_unused_spare_nodes() {
        let mut graph = join_graph(["a", "b"]);
        // The aggregator keeps only the `a` edge in the authoring document and gains a spare
        // producer spare that is referenced by a selector but wired to nothing.
        graph["nodes"].as_array_mut().unwrap().push(json!(
            {"id":"spare","data":{"kind":"agent","agentConfig":{"executor":{"agentCli":"c","modelId":"m"},"prompt":"spare"}}}
        ));
        graph["nodes"][4]["data"]["aggregatorConfig"]["variables"] =
            json!([["a", "output"], ["spare", "output"]]);
        let error = parse(graph).unwrap_err();
        assert!(
            error.to_string().contains("is not declared"),
            "unexpected error: {error}"
        );
    }
}
