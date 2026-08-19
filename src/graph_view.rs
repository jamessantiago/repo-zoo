use crate::app::Message;
use crate::config::OpenMode;
use crate::graph::{DependencyGraph, Edge, GraphLayout, NODE_H, NODE_W};
use crate::project::Repo;
use iced::keyboard::{Key, key};
use iced::widget::canvas::{self, Action, Event, Frame, Path, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Size, Vector, mouse};
use iced::{alignment, keyboard};

const EDGE_WIDTH: f32 = 1.6;
const ARROW_LEN: f32 = 9.0;
const ARROW_W: f32 = 4.5;
const CORNER_RADIUS: f32 = 8.0;
const BADGE_RADIUS: f32 = 4.0;
const REMOTE_TRUNCATE: usize = 26;
const ICON_SIZE: f32 = 16.0;
const ICON_GAP: f32 = 5.0;
const ICON_RIGHT: f32 = 8.0;
/// Optical correction for the icon glyphs: iced vertically centers text by its
/// line box (which includes descender space), so the glyph's visual center
/// lands a couple of pixels above the box's center. Nudge it back down so the
/// glyph looks centered inside its 16px background.
const ICON_GLYPH_Y_ADJUST: f32 = 1.5;

#[derive(Default)]
pub struct ProgramState {
    hover: Option<usize>,
}

pub struct GraphProgram<'a> {
    pub graph: &'a DependencyGraph,
    pub layout: GraphLayout,
    pub query: String,
    pub selected: usize,
}

impl<'a> canvas::Program<Message> for GraphProgram<'a> {
    type State = ProgramState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let local = cursor.position().map(|p| Point {
            x: p.x - bounds.x,
            y: p.y - bounds.y,
        });

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let hit = local.and_then(|point| self.hit_test(point));
                if let Some(message) = hit {
                    return Some(Action::publish(message));
                }
                None
            }
            Event::Mouse(_) => {
                let hover = local.and_then(|point| self.node_at(point));
                if hover != state.hover {
                    state.hover = hover;
                    Some(Action::request_redraw())
                } else {
                    None
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(key::Named::ArrowDown),
                ..
            }) => {
                // Move the selection to the next repo in the graph's reading
                // order. Capturing keeps the app-level keyboard subscription
                // (used by the list view) from advancing the selection too.
                Some(Action::publish(Message::Next).and_capture())
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(key::Named::ArrowUp),
                ..
            }) => Some(Action::publish(Message::Previous).and_capture()),
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &iced::Renderer,
        theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let _ = (bounds, cursor);
        let mut frame = Frame::new(
            renderer,
            Size::new(self.layout.width.max(1.0), self.layout.height.max(1.0)),
        );
        let palette = theme.extended_palette();
        let node_color = palette.primary.base.color;
        let node_border = palette.primary.strong.color;
        let edge_color = palette.primary.weak.color;
        let text_color = if palette.is_dark {
            Color::from_rgb(0.94, 0.95, 0.98)
        } else {
            Color::from_rgb(0.08, 0.08, 0.1)
        };

        // A node whose edges and dependencies are emphasized: the hovered node
        // takes over the selection emphasis while the pointer is over it (so
        // the previously selected node renders as unselected for a moment), and
        // its dependencies are its "children".
        let active = state.hover.or(Some(self.selected));
        let mut children: std::collections::HashSet<usize> = std::collections::HashSet::new();
        if let Some(active) = active {
            for edge in &self.graph.edges {
                if edge.to == active {
                    children.insert(edge.from);
                }
            }
        }

        for edge in &self.graph.edges {
            let routed = route_edge(*edge, &self.layout);
            let path = Path::new(|b| {
                b.move_to(routed.start);
                b.quadratic_curve_to(routed.control, routed.end);
            });
            let emphasized = active == Some(edge.to);
            let alpha = if emphasized { 0.95 } else { 0.55 };
            let width = if emphasized { 2.2 } else { EDGE_WIDTH };
            frame.stroke(
                &path,
                Stroke {
                    width,
                    style: canvas::Style::Solid(Color {
                        a: alpha,
                        ..edge_color
                    }),
                    ..Default::default()
                },
            );
            let dir = edge_direction(&routed);
            frame.fill(
                &arrowhead_path(routed.end, dir),
                Color {
                    a: alpha,
                    ..edge_color
                },
            );
        }

        for (index, repo) in self.graph.nodes.iter().enumerate() {
            let pos = self.layout.positions[index];
            let matched = node_has(&self.query, &repo.name);
            let selected_now = active == Some(index);
            let child = children.contains(&index);

            let (fill, alpha) = if selected_now {
                (node_border, 0.9)
            } else if child {
                // A dependency of the selected/hovered node: emphasized, but
                // less distinctly than the node itself.
                (node_color, 0.8)
            } else if matched {
                (node_color, 0.55)
            } else {
                (dim(node_color, 0.4), 0.55)
            };
            let fill = Color { a: alpha, ..fill };

            let rect = Rectangle {
                x: pos.x,
                y: pos.y,
                width: NODE_W,
                height: NODE_H,
            };
            let border = if selected_now {
                Stroke {
                    width: 2.0,
                    style: canvas::Style::Solid(node_border),
                    ..Default::default()
                }
            } else if child {
                Stroke {
                    width: 1.5,
                    style: canvas::Style::Solid(Color {
                        a: 0.8,
                        ..node_border
                    }),
                    ..Default::default()
                }
            } else {
                Stroke {
                    width: 1.0,
                    style: canvas::Style::Solid(Color {
                        a: 0.5,
                        ..node_border
                    }),
                    ..Default::default()
                }
            };

            let node_path = Path::rounded_rectangle(
                rect.position(),
                rect.size(),
                iced::border::Radius::from(CORNER_RADIUS),
            );
            frame.fill(&node_path, fill);
            frame.stroke(&node_path, border);

            let center = rect.center();
            let badge_color = match repo.kind {
                crate::project::Kind::Vcs => Color::from_rgba(0.35, 0.8, 0.45, 0.85),
                crate::project::Kind::Dir => Color::from_rgba(0.55, 0.55, 0.6, 0.85),
            };
            frame.fill(
                &Path::circle(
                    Point {
                        x: rect.x + 16.0,
                        y: center.y,
                    },
                    BADGE_RADIUS,
                ),
                badge_color,
            );

            let label_color = if selected_now || child || matched {
                text_color
            } else {
                Color {
                    a: text_color.a * 0.45,
                    ..text_color
                }
            };

            let icons = self.icons(index, rect);
            // Reserve the right-hand icon strip so labels don't run into it.
            let text_max = icons.first().map_or(NODE_W - 40.0, |icon| {
                (icon.rect.x - (rect.x + 26.0) - 4.0).max(20.0)
            });

            frame.fill_text(Text {
                content: repo.name.clone(),
                position: Point {
                    x: rect.x + 26.0,
                    y: center.y - 8.0,
                },
                color: label_color,
                size: Pixels(14.0),
                max_width: text_max,
                align_y: alignment::Vertical::Center,
                ..Default::default()
            });

            if let Some(remote) = &repo.remote {
                frame.fill_text(Text {
                    content: truncate(remote.clone()),
                    position: Point {
                        x: rect.x + 26.0,
                        y: rect.y + NODE_H - 12.0,
                    },
                    color: Color {
                        a: label_color.a * 0.6,
                        ..label_color
                    },
                    size: Pixels(9.0),
                    max_width: text_max,
                    align_y: alignment::Vertical::Center,
                    ..Default::default()
                });
            }

            for icon in &icons {
                let bg = Path::rounded_rectangle(
                    icon.rect.position(),
                    icon.rect.size(),
                    iced::border::Radius::from(4.0),
                );
                frame.fill(
                    &bg,
                    Color {
                        a: 0.12,
                        ..text_color
                    },
                );
                frame.stroke(
                    &bg,
                    Stroke {
                        width: 1.0,
                        style: canvas::Style::Solid(Color {
                            a: 0.3,
                            ..text_color
                        }),
                        ..Default::default()
                    },
                );
                frame.fill_text(Text {
                    content: icon.glyph.to_string(),
                    position: Point {
                        x: icon.rect.center().x,
                        y: icon.rect.center().y + ICON_GLYPH_Y_ADJUST,
                    },
                    color: label_color,
                    size: Pixels(11.0),
                    align_x: alignment::Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}

impl GraphProgram<'_> {
    pub fn node_rects(&self) -> Vec<Rectangle> {
        self.layout
            .positions
            .iter()
            .map(|pos| Rectangle {
                x: pos.x,
                y: pos.y,
                width: NODE_W,
                height: NODE_H,
            })
            .collect()
    }

    fn node_at(&self, point: Point) -> Option<usize> {
        self.node_rects()
            .iter()
            .position(|rect| contains(*rect, point))
    }

    /// Which action a click at `point` triggers: an icon inside a node, or the
    /// node body itself (default open).
    fn hit_test(&self, point: Point) -> Option<Message> {
        for index in 0..self.graph.nodes.len() {
            let rect = self.node_rects()[index];
            if !contains(rect, point) {
                continue;
            }
            let icons = self.icons(index, rect);
            for icon in &icons {
                if contains(icon.rect, point) {
                    return Some(icon.message.clone());
                }
            }
            return Some(Message::OpenGraphNode(index));
        }
        None
    }

    /// The clickable icons shown inside a node: editor / terminal / manager
    /// for local projects, a clone icon for remote-only placeholders.
    fn icons(&self, index: usize, rect: Rectangle) -> Vec<IconSpec> {
        let repo = &self.graph.nodes[index];
        let count = icon_count(repo);
        let rects = icon_rects(rect, count);
        let mut icons = Vec::with_capacity(count);
        let mut slot = 0;
        if repo.path_known {
            let specs = [
                ("✎", Message::OpenGraphNodeMode(index, OpenMode::Editor)),
                ("▸", Message::OpenGraphNodeMode(index, OpenMode::Terminal)),
                ("▣", Message::OpenGraphNodeMode(index, OpenMode::Manager)),
            ];
            for (glyph, message) in specs {
                icons.push(IconSpec {
                    glyph,
                    message,
                    rect: rects[slot],
                });
                slot += 1;
            }
        } else if repo.remote.is_some() {
            icons.push(IconSpec {
                glyph: "⬇",
                message: Message::CloneGraphNode(index),
                rect: rects[0],
            });
        }
        icons
    }
}

struct IconSpec {
    glyph: &'static str,
    message: Message,
    rect: Rectangle,
}

fn icon_count(repo: &Repo) -> usize {
    if repo.path_known {
        3
    } else if repo.remote.is_some() {
        1
    } else {
        0
    }
}

fn icon_rects(rect: Rectangle, count: usize) -> Vec<Rectangle> {
    let total = count as f32 * ICON_SIZE + count.saturating_sub(1) as f32 * ICON_GAP;
    let start_x = rect.x + rect.width - ICON_RIGHT - total;
    let cy = rect.center().y;
    (0..count)
        .map(|i| Rectangle {
            x: start_x + i as f32 * (ICON_SIZE + ICON_GAP),
            y: cy - ICON_SIZE / 2.0,
            width: ICON_SIZE,
            height: ICON_SIZE,
        })
        .collect()
}

pub fn graph_canvas<'a>(
    graph: &'a DependencyGraph,
    layout: GraphLayout,
    query: &'a str,
    selected: usize,
) -> canvas::Canvas<GraphProgram<'a>, Message> {
    let (width, height) = (layout.width.max(1.0), layout.height.max(1.0));
    canvas::Canvas::new(GraphProgram {
        graph,
        layout,
        query: query.to_string(),
        selected,
    })
    .width(width)
    .height(height)
}

fn contains(rect: Rectangle, point: Point) -> bool {
    rect.x <= point.x
        && point.x <= rect.x + rect.width
        && rect.y <= point.y
        && point.y <= rect.y + rect.height
}

fn node_has(query: &str, name: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || name.to_lowercase().contains(&query)
}

fn truncate(mut s: String) -> String {
    if s.chars().count() > REMOTE_TRUNCATE {
        let out: String = s.drain(..REMOTE_TRUNCATE).collect();
        format!("{out}…")
    } else {
        s
    }
}

fn dim(color: Color, factor: f32) -> Color {
    Color {
        r: color.r * factor,
        g: color.g * factor,
        b: color.b * factor,
        a: color.a,
    }
}

fn arrowhead_path(tip: Point, dir: Vector) -> Path {
    let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
    if len < 1e-4 {
        return Path::new(|b| {
            b.move_to(tip);
        });
    }
    let unit = Vector::new(dir.x / len, dir.y / len);
    let perp = Vector::new(-unit.y, unit.x);
    let back = Point {
        x: tip.x - unit.x * ARROW_LEN,
        y: tip.y - unit.y * ARROW_LEN,
    };
    let p1 = Point {
        x: back.x + perp.x * ARROW_W,
        y: back.y + perp.y * ARROW_W,
    };
    let p2 = Point {
        x: back.x - perp.x * ARROW_W,
        y: back.y - perp.y * ARROW_W,
    };
    Path::new(|b| {
        b.move_to(tip);
        b.line_to(p1);
        b.line_to(p2);
        b.close();
    })
}

struct Routed {
    start: Point,
    end: Point,
    control: Point,
}

fn route_edge(edge: Edge, layout: &GraphLayout) -> Routed {
    let a = layout.positions[edge.from];
    let b = layout.positions[edge.to];

    let a_cx = a.x + NODE_W / 2.0;
    let a_cy = a.y + NODE_H / 2.0;
    let b_cx = b.x + NODE_W / 2.0;
    let b_cy = b.y + NODE_H / 2.0;

    let dx = b.x - a.x;
    let dy = b.y - a.y;

    if dx.abs() > dy.abs() {
        // Horizontal-ish: exit the right/left side of the source.
        if dx >= 0.0 {
            let start = Point {
                x: a.x + NODE_W,
                y: a_cy,
            };
            let end = Point { x: b.x, y: b_cy };
            let mid_x = (start.x + end.x) / 2.0;
            Routed {
                start,
                end,
                control: Point {
                    x: mid_x,
                    y: start.y,
                },
            }
        } else {
            let start = Point { x: a.x, y: a_cy };
            let end = Point {
                x: b.x + NODE_W,
                y: b_cy,
            };
            let mid_x = (start.x + end.x) / 2.0;
            Routed {
                start,
                end,
                control: Point {
                    x: mid_x,
                    y: start.y,
                },
            }
        }
    } else {
        // Vertical-ish: exit the bottom/top of the source.
        if dy >= 0.0 {
            let start = Point {
                x: a_cx,
                y: a.y + NODE_H,
            };
            let end = Point { x: b_cx, y: b.y };
            let mid_y = (start.y + end.y) / 2.0;
            Routed {
                start,
                end,
                control: Point {
                    x: start.x,
                    y: mid_y,
                },
            }
        } else {
            let start = Point { x: a_cx, y: a.y };
            let end = Point {
                x: b_cx,
                y: b.y + NODE_H,
            };
            let mid_y = (start.y + end.y) / 2.0;
            Routed {
                start,
                end,
                control: Point {
                    x: start.x,
                    y: mid_y,
                },
            }
        }
    }
}

fn edge_direction(routed: &Routed) -> Vector {
    Vector::new(routed.end.x - routed.start.x, routed.end.y - routed.start.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, RepoSettings};
    use iced::keyboard;
    use iced::keyboard::key::{Code, Named, Physical};
    use iced::widget::canvas::{Event, Program};
    use iced::{Rectangle, mouse};
    use std::collections::BTreeMap;

    fn test_graph() -> &'static DependencyGraph {
        let config = Config {
            repos: BTreeMap::from([
                ("alpha".to_string(), RepoSettings::default()),
                ("beta".to_string(), RepoSettings::default()),
                ("gamma".to_string(), RepoSettings::default()),
            ]),
            ..Config::default()
        };
        Box::leak(Box::new(DependencyGraph::build(&config)))
    }

    fn key_event(named: Named) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            modified_key: keyboard::Key::Named(named),
            physical_key: Physical::Code(Code::ArrowDown),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    }

    fn program(selected: usize) -> GraphProgram<'static> {
        let graph = test_graph();
        let layout = graph.layout();
        GraphProgram {
            graph,
            layout,
            query: String::new(),
            selected,
        }
    }

    #[test]
    fn arrow_down_publishes_next_and_captures() {
        let mut state = ProgramState::default();
        let action = program(0)
            .update(
                &mut state,
                &key_event(Named::ArrowDown),
                Rectangle::default(),
                mouse::Cursor::default(),
            )
            .expect("arrow down should produce an action");

        let (message, _, status) = action.into_inner();
        assert!(matches!(message, Some(Message::Next)));
        assert_eq!(status, iced::event::Status::Captured);
    }

    #[test]
    fn arrow_up_publishes_previous_and_captures() {
        let mut state = ProgramState::default();
        let action = program(1)
            .update(
                &mut state,
                &key_event(Named::ArrowUp),
                Rectangle::default(),
                mouse::Cursor::default(),
            )
            .expect("arrow up should produce an action");

        let (message, _, status) = action.into_inner();
        assert!(matches!(message, Some(Message::Previous)));
        assert_eq!(status, iced::event::Status::Captured);
    }

    #[test]
    fn other_keys_are_ignored() {
        let mut state = ProgramState::default();
        let action = program(0).update(
            &mut state,
            &key_event(Named::Enter),
            Rectangle::default(),
            mouse::Cursor::default(),
        );
        assert!(action.is_none());
    }

    #[test]
    fn node_body_click_opens_default_mode() {
        let program = program(0);
        let rect = program.node_rects()[0];
        let message = program
            .hit_test(Point {
                x: rect.x + 20.0,
                y: rect.center().y,
            })
            .expect("click on the node body should hit");
        assert!(matches!(message, Message::OpenGraphNode(0)));
    }

    #[test]
    fn click_outside_nodes_is_ignored() {
        let program = program(0);
        assert!(program.hit_test(Point::new(-100.0, -100.0)).is_none());
    }

    fn remote_only_graph() -> &'static DependencyGraph {
        let config = Config {
            repos: BTreeMap::from([(
                "remote-repo".to_string(),
                RepoSettings {
                    remote: Some("https://example.com/r.git".to_string()),
                    ..Default::default()
                },
            )]),
            ..Config::default()
        };
        Box::leak(Box::new(DependencyGraph::build(&config)))
    }

    #[test]
    fn remote_only_node_has_clone_icon() {
        let graph = remote_only_graph();
        let layout = graph.layout();
        let program = GraphProgram {
            graph,
            layout,
            query: String::new(),
            selected: 0,
        };
        let rect = program.node_rects()[0];
        let icons = program.icons(0, rect);
        assert_eq!(icons.len(), 1);
        let message = program
            .hit_test(icons[0].rect.center())
            .expect("clicking the clone icon should hit");
        assert!(matches!(message, Message::CloneGraphNode(0)));
    }

    fn local_graph() -> &'static DependencyGraph {
        let dir = std::env::temp_dir().join(format!("repo-zoo-graph-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let config = Config {
            repos: BTreeMap::from([(
                "local".to_string(),
                RepoSettings {
                    path: Some(dir.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )]),
            ..Config::default()
        };
        Box::leak(Box::new(DependencyGraph::build(&config)))
    }

    #[test]
    fn local_node_has_three_action_icons() {
        let graph = local_graph();
        let layout = graph.layout();
        let program = GraphProgram {
            graph,
            layout,
            query: String::new(),
            selected: 0,
        };
        let rect = program.node_rects()[0];
        let icons = program.icons(0, rect);
        assert_eq!(icons.len(), 3);

        let editor = program.hit_test(icons[0].rect.center()).unwrap();
        assert!(matches!(
            editor,
            Message::OpenGraphNodeMode(0, OpenMode::Editor)
        ));
        let terminal = program.hit_test(icons[1].rect.center()).unwrap();
        assert!(matches!(
            terminal,
            Message::OpenGraphNodeMode(0, OpenMode::Terminal)
        ));
        let manager = program.hit_test(icons[2].rect.center()).unwrap();
        assert!(matches!(
            manager,
            Message::OpenGraphNodeMode(0, OpenMode::Manager)
        ));
    }
}
