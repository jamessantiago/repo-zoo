use std::collections::{BTreeMap, HashMap, HashSet};

use crate::config::{Config, RepoSettings};
use crate::project::{self, Repo};

pub const NODE_W: f32 = 216.0;
pub const NODE_H: f32 = 54.0;
pub const H_GAP: f32 = 80.0;
pub const V_GAP: f32 = 26.0;
pub const PAD: f32 = 28.0;
/// Horizontal graph padding. Kept smaller than [`PAD`] so the whole three-node
/// row (plus room for the floating scrollbar) fits inside the launcher window
/// without the scrollbar overlapping the rightmost node.
pub const H_PAD: f32 = 16.0;

/// Default graph width in nodes; wider layers wrap onto extra sub-rows.
/// Mirrors the config default (`max_row_width`).
pub const DEFAULT_ROW_WIDTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    /// Index of the dependency (the repo being depended upon).
    pub from: usize,
    /// Index of the dependent repo.
    pub to: usize,
}

#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub nodes: Vec<Repo>,
    pub edges: Vec<Edge>,
    /// Maximum number of nodes allowed in a row before wrapping a layer onto
    /// extra sub-rows. `0` (the default, and what older configs contain) means
    /// the [`DEFAULT_ROW_WIDTH`]; larger values allow wider layers.
    pub max_row_width: usize,
}

impl DependencyGraph {
    /// Builds the graph purely from the config. The config file is the single
    /// source of truth: every node comes from a `[repos.*]` entry (repos
    /// declared in `depends_on` but not configured become placeholder nodes).
    pub fn build(config: &Config) -> Self {
        let mut nodes: Vec<Repo> = Vec::new();
        let mut index_of: HashMap<String, usize> = HashMap::new();
        let settings: &BTreeMap<String, RepoSettings> = &config.repos;

        for (name, settings) in settings {
            let path = settings
                .path
                .as_ref()
                .map(|p| project::expand_tilde(std::path::Path::new(p)));
            let path_known = path.as_ref().is_some_and(|p| p.exists());
            let is_repo =
                settings.remote.is_some() || path.as_ref().is_some_and(|p| p.join(".git").exists());
            index_of.insert(name.clone(), nodes.len());
            nodes.push(Repo {
                name: name.clone(),
                path: path.unwrap_or_default(),
                path_known,
                remote: settings.remote.clone(),
                kind: if is_repo {
                    project::Kind::Vcs
                } else {
                    project::Kind::Dir
                },
                editor: settings.editor.clone(),
                terminal: settings.terminal.clone(),
                sln: settings
                    .sln
                    .as_ref()
                    .map(|s| project::expand_tilde(std::path::Path::new(s))),
            });
        }

        let mut edges = Vec::new();
        let mut seen: HashSet<(usize, usize)> = HashSet::new();
        for (name, settings) in settings {
            let Some(&dependent) = index_of.get(name) else {
                continue;
            };
            for dep in &settings.depends_on {
                let dep = dep.trim();
                if dep.is_empty() {
                    continue;
                }
                let dep_index = match index_of.get(dep) {
                    Some(&index) => index,
                    None => {
                        index_of.insert(dep.to_string(), nodes.len());
                        nodes.push(Repo {
                            name: dep.to_string(),
                            path: std::path::PathBuf::new(),
                            path_known: false,
                            remote: None,
                            kind: project::Kind::Dir,
                            editor: None,
                            terminal: None,
                            sln: None,
                        });
                        nodes.len() - 1
                    }
                };
                if seen.insert((dep_index, dependent)) {
                    edges.push(Edge {
                        from: dep_index,
                        to: dependent,
                    });
                }
            }
        }

        DependencyGraph {
            nodes,
            edges,
            max_row_width: config.max_row_width as usize,
        }
    }

    fn adjacency(&self) -> Vec<Vec<usize>> {
        let mut adj = vec![Vec::new(); self.nodes.len()];
        for edge in &self.edges {
            adj[edge.from].push(edge.to);
        }
        adj
    }

    /// Assigns each node a (layer, index_within_layer) position for a
    /// top-to-bottom layout with the user's own projects on top: a project
    /// that `depends_on` something is drawn above it, so every edge points
    /// downward from a dependent to its dependency (an app at the top, its
    /// libraries further down). Cycles are handled by collapsing
    /// strongly-connected components into a single layer.
    pub fn layering(&self) -> Vec<Vec<usize>> {
        let adj = self.adjacency();
        let sccs = strongly_connected_components(&adj);
        let mut scc_of = vec![0usize; self.nodes.len()];
        for (s, component) in sccs.iter().enumerate() {
            for &node in component {
                scc_of[node] = s;
            }
        }

        let num_scc = sccs.len();
        let mut dag_edges: Vec<(usize, usize)> = Vec::new();
        let mut seen = HashSet::new();
        for edge in &self.edges {
            // Walk the reversed direction (dependent -> dependency) so the
            // longest-path layering puts dependents on top of what they use.
            let (u, v) = (scc_of[edge.to], scc_of[edge.from]);
            if u != v && seen.insert((u, v)) {
                dag_edges.push((u, v));
            }
        }

        // Longest-path layering over the DAG of SCCs.
        let mut scc_layer = vec![0usize; num_scc];
        for _ in 0..num_scc {
            let mut changed = false;
            for &(u, v) in &dag_edges {
                let next = scc_layer[u] + 1;
                if next > scc_layer[v] {
                    scc_layer[v] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Repos that take part in no dependency edge at all are unrelated to
        // the hierarchy. Instead of sharing the top row with the apps that
        // depend on things (making it look goofy), sink them to a layer below
        // everything else so the top of the graph reads as the dependency
        // structure.
        let mut in_edge = vec![false; self.nodes.len()];
        for edge in &self.edges {
            in_edge[edge.from] = true;
            in_edge[edge.to] = true;
        }
        let max_layer = scc_layer.iter().copied().max().unwrap_or(0);
        for (s, component) in sccs.iter().enumerate() {
            if component.iter().all(|&node| !in_edge[node]) {
                scc_layer[s] = max_layer + 1;
            }
        }

        // Order SCCs deterministically: by layer, then by smallest node index.
        let mut order: Vec<usize> = (0..num_scc).collect();
        order.sort_by_key(|&s| {
            (
                scc_layer[s],
                sccs[s].iter().min().copied().unwrap_or(usize::MAX),
            )
        });

        // Flatten SCC members into per-layer node lists, keeping the stable
        // SCC ordering. Nodes inside the same SCC share a layer.
        let mut layers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &s in &order {
            let mut members = sccs[s].clone();
            members.sort_unstable();
            let empty = layers.entry(scc_layer[s]).or_default();
            empty.extend(members);
        }

        let mut result: Vec<Vec<usize>> = layers.into_values().collect();
        // De-duplicate: a node belongs to exactly one SCC and one layer, so
        // this is already exact. Keep the vector typed for the layout pass.
        for layer in &mut result {
            layer.sort_unstable();
        }
        result
    }

    /// Computes pixel positions for every node, plus the total content size.
    pub fn layout(&self) -> GraphLayout {
        let layers = self.layering();
        let mut positions: Vec<Option<(f32, f32)>> = vec![None; self.nodes.len()];

        let cap = if self.max_row_width == 0 {
            DEFAULT_ROW_WIDTH
        } else {
            self.max_row_width
        };

        let mut y = PAD;
        let mut max_right = 0.0f32;
        let mut max_bottom = 0.0f32;
        for layer in &layers {
            for (k, chunk) in layer.chunks(cap).enumerate() {
                let row_y = y + k as f32 * (NODE_H + V_GAP);
                let mut x = H_PAD;
                for &node in chunk {
                    positions[node] = Some((x, row_y));
                    x += NODE_W + H_GAP;
                }
                let right = x - H_GAP;
                max_right = max_right.max(right);
                max_bottom = max_bottom.max(row_y + NODE_H);
            }
            let sub_rows = layer.len().div_ceil(cap).max(1);
            y += sub_rows as f32 * (NODE_H + V_GAP);
        }

        let width = if layers.is_empty() {
            H_PAD * 2.0
        } else {
            max_right + H_PAD
        };
        let height = if layers.is_empty() {
            PAD * 2.0
        } else {
            max_bottom + PAD
        };

        GraphLayout {
            positions: positions
                .into_iter()
                .map(|p| {
                    p.map(|(x, y)| NodePosition { x, y })
                        .unwrap_or(NodePosition { x: 0.0, y: 0.0 })
                })
                .collect(),
            width,
            height,
        }
    }

    /// Node indices in visual reading order: layer by layer (top to bottom),
    /// left to right within a layer. When `query` is non-empty, only nodes
    /// whose name contains it (case-insensitive) are included. This is the
    /// order used to move the selection with ↑/↓ in the graph view.
    pub fn reading_order(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        self.layering()
            .into_iter()
            .flatten()
            .filter(|&index| {
                query.is_empty() || self.nodes[index].name.to_lowercase().contains(&query)
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NodePosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct GraphLayout {
    pub positions: Vec<NodePosition>,
    pub width: f32,
    pub height: f32,
}

/// Tarjan's algorithm returning strongly-connected components. A single node
/// with no self-loop yields a singleton component.
fn strongly_connected_components(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut index = vec![0usize; n];
    let mut lowlink = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut counter = 1usize;

    #[allow(clippy::too_many_arguments)]
    fn strongconnect(
        v: usize,
        adj: &[Vec<usize>],
        index: &mut [usize],
        lowlink: &mut [usize],
        on_stack: &mut [bool],
        stack: &mut Vec<usize>,
        components: &mut Vec<Vec<usize>>,
        counter: &mut usize,
    ) {
        index[v] = *counter;
        lowlink[v] = *counter;
        *counter += 1;
        stack.push(v);
        on_stack[v] = true;

        for &w in &adj[v] {
            if index[w] == 0 {
                strongconnect(w, adj, index, lowlink, on_stack, stack, components, counter);
                lowlink[v] = lowlink[v].min(lowlink[w]);
            } else if on_stack[w] {
                lowlink[v] = lowlink[v].min(index[w]);
            }
        }

        if lowlink[v] == index[v] {
            let mut component = Vec::new();
            loop {
                let w = stack.pop().unwrap();
                on_stack[w] = false;
                component.push(w);
                if w == v {
                    break;
                }
            }
            components.push(component);
        }
    }

    for v in 0..n {
        if index[v] == 0 {
            strongconnect(
                v,
                adj,
                &mut index,
                &mut lowlink,
                &mut on_stack,
                &mut stack,
                &mut components,
                &mut counter,
            );
        }
    }

    components
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(repos: &[(&str, &[&str])]) -> Config {
        let mut map = std::collections::BTreeMap::new();
        for (name, deps) in repos {
            let settings = crate::config::RepoSettings {
                depends_on: deps.iter().map(|d| d.to_string()).collect(),
                ..Default::default()
            };
            map.insert((*name).to_string(), settings);
        }
        Config {
            roots: Vec::new(),
            depth: 1,
            open_mode: crate::config::OpenMode::Editor,
            editor: "code".to_string(),
            max_row_width: 0,
            repos: map,
            ..Config::default()
        }
    }

    #[test]
    fn builds_edges_and_external_deps() {
        let config = config_with(&[("a", &["b", "lib-x"]), ("b", &["lib-x"])]);
        let graph = DependencyGraph::build(&config);

        assert_eq!(graph.nodes.len(), 3);
        assert!(graph.nodes.iter().any(|n| n.name == "lib-x"));

        let index = |name: &str| graph.nodes.iter().position(|n| n.name == name).unwrap();
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.from == index("b") && e.to == index("a"))
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.from == index("lib-x") && e.to == index("a"))
        );
    }

    #[test]
    fn layers_dependents_above_their_dependencies() {
        // a depends on b, b depends on lib-x; a sits on top, lib-x at the
        // bottom.
        let config = config_with(&[("a", &["b"]), ("b", &["lib-x"]), ("lib-x", &[])]);
        let graph = DependencyGraph::build(&config);
        let layers = graph.layering();

        let index = |name: &str| graph.nodes.iter().position(|n| n.name == name).unwrap();
        let layer_of = |name: &str| {
            layers
                .iter()
                .position(|l| l.contains(&index(name)))
                .unwrap()
        };

        assert!(layer_of("a") < layer_of("b"));
        assert!(layer_of("b") < layer_of("lib-x"));
    }

    #[test]
    fn cycles_collapse_into_single_layer() {
        // a <-> b cycle
        let config = config_with(&[("a", &["b"]), ("b", &["a"])]);
        let graph = DependencyGraph::build(&config);
        let layers = graph.layering();

        let layer_of = |name: &str| {
            let i = graph.nodes.iter().position(|n| n.name == name).unwrap();
            layers.iter().position(|l| l.contains(&i)).unwrap()
        };
        assert_eq!(layer_of("a"), layer_of("b"));
    }

    #[test]
    fn layout_size_contains_every_node() {
        // Single layer, many nodes: regression test for a width bug where a
        // single column produced a canvas too narrow to show whole cards.
        let config = config_with(&[("a", &[]), ("b", &[]), ("c", &[]), ("d", &[]), ("e", &[])]);
        let graph = DependencyGraph::build(&config);
        let layout = graph.layout();

        for (i, _) in graph.nodes.iter().enumerate() {
            let pos = layout.positions[i];
            assert!(
                pos.x + NODE_W <= layout.width,
                "node {i} extends past right edge {:.0} vs {:.0}",
                pos.x + NODE_W,
                layout.width
            );
            assert!(
                pos.y + NODE_H <= layout.height,
                "node {i} extends past bottom edge"
            );
        }
    }

    #[test]
    fn positions_are_sorted_and_non_overlapping() {
        let config = config_with(&[("a", &["b", "c"]), ("c", &["b"])]);
        let graph = DependencyGraph::build(&config);
        let layout = graph.layout();

        for (i, node) in graph.nodes.iter().enumerate() {
            let pos = layout.positions[i];
            let x = pos.x;
            let y = pos.y;
            for (j, other) in graph.nodes.iter().enumerate() {
                if i == j {
                    continue;
                }
                let other_pos = layout.positions[j];
                let overlap_x = (pos.x - other_pos.x).abs() < (NODE_W) - 1.0;
                let overlap_y = (pos.y - other_pos.y).abs() < (NODE_H) - 1.0;
                assert!(
                    !(overlap_x && overlap_y),
                    "nodes {} and {} overlap",
                    node.name,
                    other.name
                );
            }
            let _ = y;
            assert!(x >= 0.0);
            assert!(layout.width > x);
        }
    }

    #[test]
    fn reading_order_follows_layout_layers() {
        // a -> b -> lib-x (a depends on b, b on lib-x), plus an unrelated 'z'
        // that sorts first alphabetically. Reading order must follow the
        // layout, not the name.
        let config = config_with(&[("a", &["b"]), ("b", &["lib-x"]), ("lib-x", &[]), ("z", &[])]);
        let graph = DependencyGraph::build(&config);
        let order = graph.reading_order("");

        let index = |name: &str| graph.nodes.iter().position(|n| n.name == name).unwrap();
        let rank = |name: &str| order.iter().position(|&i| i == index(name)).unwrap();
        assert!(rank("a") < rank("b"));
        assert!(rank("b") < rank("lib-x"));
        assert_eq!(order.len(), 4);
    }

    #[test]
    fn reading_order_filters_by_query() {
        let config = config_with(&[("alpha", &[]), ("beta", &[]), ("alpine", &[])]);
        let graph = DependencyGraph::build(&config);
        let order = graph.reading_order("al");

        assert_eq!(order.len(), 2);
        let names: Vec<&str> = order
            .iter()
            .map(|&i| graph.nodes[i].name.as_str())
            .collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"alpine"));
        assert!(!names.contains(&"beta"));
    }

    #[test]
    fn unlinked_nodes_sink_below_the_dependency_structure() {
        // 'z' depends on nothing and nothing depends on it: it must sit below
        // the layered hierarchy (a -> b -> lib-x) instead of cluttering the
        // top row.
        let config = config_with(&[("a", &["b"]), ("b", &["lib-x"]), ("lib-x", &[]), ("z", &[])]);
        let graph = DependencyGraph::build(&config);
        let layers = graph.layering();

        let index = |name: &str| graph.nodes.iter().position(|n| n.name == name).unwrap();
        let layer_of = |name: &str| {
            layers
                .iter()
                .position(|l| l.contains(&index(name)))
                .unwrap()
        };
        assert!(layer_of("z") > layer_of("a"));
        assert!(layer_of("z") > layer_of("lib-x"));
        assert_eq!(
            layer_of("z"),
            layers.len() - 1,
            "unlinked nodes share the bottom layer"
        );
    }

    #[test]
    fn zero_row_width_uses_the_default_cap() {
        // `max_row_width = 0` is what seeded configs contain; it must wrap at
        // the default width rather than lay everything out on one row.
        let config = config_with(&[("a", &[]), ("b", &[]), ("c", &[]), ("d", &[]), ("e", &[])]);
        let graph = DependencyGraph::build(&config);
        assert_eq!(graph.max_row_width, 0);
        let layout = graph.layout();

        let three_wide = PAD * 2.0 + 3.0 * NODE_W + 2.0 * H_GAP;
        assert!(
            layout.width <= three_wide + 0.5,
            "graph too wide: {:.0} > {:.0}",
            layout.width,
            three_wide
        );
    }

    #[test]
    fn max_row_width_wraps_layers_into_sub_rows() {
        // Layer 0 has 7 independent nodes; with a cap of 3 they must wrap onto
        // three sub-rows without overflowing the content width.
        let mut config = config_with(&[
            ("a", &[]),
            ("b", &[]),
            ("c", &[]),
            ("d", &[]),
            ("e", &[]),
            ("f", &[]),
            ("g", &[]),
        ]);
        config.max_row_width = 3;
        let graph = DependencyGraph::build(&config);
        let layout = graph.layout();

        let positions: Vec<(f32, f32)> = (0..7)
            .map(|i| {
                let p = layout.positions[i];
                (p.x, p.y)
            })
            .collect();

        // No node may extend past the right edge, and the whole graph must be
        // narrow enough for three cards plus gaps and padding.
        for (i, (x, _)) in positions.iter().enumerate() {
            assert!(x + NODE_W <= layout.width, "node {i} overflows width");
        }
        let three_wide = PAD * 2.0 + 3.0 * NODE_W + 2.0 * H_GAP;
        assert!(
            layout.width <= three_wide + 0.5,
            "graph too wide: {:.0} > {:.0}",
            layout.width,
            three_wide
        );

        // Nodes must occupy at least three distinct rows.
        let rows: std::collections::HashSet<u32> =
            positions.iter().map(|(_, y)| y.round() as u32).collect();
        assert!(rows.len() >= 3, "expected wrapped sub-rows, got {rows:?}");
    }

    #[test]
    fn carries_the_solution_file_onto_the_node() {
        let mut config = config_with(&[("a", &[])]);
        config.repos.insert(
            "a".to_string(),
            crate::config::RepoSettings {
                path: Some("~/code/a".to_string()),
                sln: Some("~/code/a/a.sln".to_string()),
                ..Default::default()
            },
        );
        let graph = DependencyGraph::build(&config);

        let node = graph.nodes.iter().find(|n| n.name == "a").unwrap();
        let home = dirs::home_dir().unwrap();
        let expected = home.join("code/a/a.sln");
        assert_eq!(node.sln.as_deref(), Some(expected.as_path()));
    }
}
