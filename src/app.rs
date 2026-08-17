use iced::widget::{
    Column, Row, button, column, container, pick_list, scrollable, text, text_input,
};
use iced::{
    Color, Element, Length, Subscription, Task, Theme, alignment,
    keyboard::{self, Key, key},
    widget,
    window::{self, Id, Mode},
};

use crate::config::{Config, DefaultView, OpenMode};
use crate::graph::{DependencyGraph, GraphLayout, NODE_H, NODE_W};
use crate::graph_view;
use crate::hotkey;
use crate::kde;
use crate::launch;
use crate::project::{self, Repo};
use crate::tray;

const SEARCH_ID: &str = "repo-zoo-search";
const GRAPH_SCROLL_ID: &str = "repo-zoo-graph-scroll";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Graph,
    List,
}

impl std::fmt::Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            View::Graph => write!(f, "Graph"),
            View::List => write!(f, "List"),
        }
    }
}

const VIEWS: [View; 2] = [View::Graph, View::List];

#[derive(Debug, Clone)]
pub enum Message {
    SearchInputChanged(String),
    SearchSubmitted,
    FocusSearch,
    OpenAt(usize),
    OpenGraphNode(usize),
    OpenAtMode(usize, OpenMode),
    OpenGraphNodeMode(usize, OpenMode),
    CloneAt(usize),
    CloneGraphNode(usize),
    Cloned(Result<String, String>),
    Previous,
    Next,
    Escape,
    Refresh,
    OpenConfig,
    ViewChanged(View),
    GraphScrolled(scrollable::Viewport),
    Opened(Result<String, String>),
    WindowReady(Option<Id>),
    Hotkey(hotkey::Event),
    Kde(kde::Event),
    Tray(tray::Event),
    CloseRequested,
}

pub struct App {
    config: Config,
    repos: Vec<Repo>,
    graph: DependencyGraph,
    layout: GraphLayout,
    view: View,
    query: String,
    selected: usize,
    graph_viewport: Option<scrollable::Viewport>,
    status: Option<String>,
    window: Option<Id>,
    hidden: bool,
    quitting: bool,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = Config::load();

        let view = match std::env::var("REPO_ZOO_VIEW").as_deref() {
            Ok("graph") => View::Graph,
            Ok("list") => View::List,
            _ => match config.default_view {
                DefaultView::Graph => View::Graph,
                DefaultView::List => View::List,
            },
        };

        let mut app = App {
            config,
            repos: Vec::new(),
            graph: DependencyGraph::build(&Config::default()),
            layout: DependencyGraph::build(&Config::default()).layout(),
            view,
            query: String::new(),
            selected: 0,
            graph_viewport: None,
            status: None,
            window: None,
            hidden: false,
            quitting: false,
        };
        app.rebuild_graph();

        let focus = widget::operation::focus(SEARCH_ID);
        let window_id = window::oldest().map(Message::WindowReady);
        (app, Task::batch([focus, window_id]))
    }

    fn rebuild_graph(&mut self) {
        let graph = DependencyGraph::build(&self.config);
        self.repos = graph.nodes.clone();
        self.layout = graph.layout();
        self.graph = graph;
    }

    fn filtered(&self) -> impl Iterator<Item = &Repo> {
        let query = self.query.trim().to_lowercase();
        self.repos.iter().filter(move |p| {
            query.is_empty()
                || p.name.to_lowercase().contains(&query)
                || p.path.to_string_lossy().to_lowercase().contains(&query)
        })
    }

    fn clamp_selection(&mut self) {
        match self.view {
            View::List => {
                let count = self.filtered().count();
                self.selected = if count == 0 {
                    0
                } else {
                    self.selected.min(count - 1)
                };
            }
            View::Graph => {
                let count = self.graph.nodes.len();
                self.selected = if count == 0 {
                    0
                } else {
                    self.selected.min(count - 1)
                };
            }
        }
    }

    fn selected_repo(&self) -> Option<&Repo> {
        match self.view {
            View::List => self.filtered().nth(self.selected),
            View::Graph => self.graph.nodes.get(self.selected),
        }
    }

    /// Graph node indices in reading order (layer by layer, left to right),
    /// restricted to nodes matching the current query.
    fn graph_order(&self) -> Vec<usize> {
        self.graph.reading_order(&self.query)
    }

    /// Scrolls the graph so the selected node is visible, if the graph
    /// overflows its viewport.
    fn scroll_to_selection(&self) -> Task<Message> {
        if self.view != View::Graph {
            return Task::none();
        }
        let Some(viewport) = self.graph_viewport else {
            return Task::none();
        };
        let Some(pos) = self.layout.positions.get(self.selected).copied() else {
            return Task::none();
        };

        let current = viewport.absolute_offset();
        let view_w = viewport.bounds().width;
        let view_h = viewport.bounds().height;
        let content_w = viewport.content_bounds().width;
        let content_h = viewport.content_bounds().height;

        let left = pos.x;
        let right = pos.x + NODE_W;
        let top = pos.y;
        let bottom = pos.y + NODE_H;

        let mut target_x = current.x;
        let mut target_y = current.y;
        if left < current.x || right > current.x + view_w {
            target_x = if left < current.x {
                left
            } else {
                right - view_w
            };
        }
        if top < current.y || bottom > current.y + view_h {
            target_y = if top < current.y {
                top
            } else {
                bottom - view_h
            };
        }
        target_x = target_x.clamp(0.0, (content_w - view_w).max(0.0));
        target_y = target_y.clamp(0.0, (content_h - view_h).max(0.0));

        if (target_x - current.x).abs() < 0.5 && (target_y - current.y).abs() < 0.5 {
            return Task::none();
        }
        widget::operation::scroll_to(
            GRAPH_SCROLL_ID,
            scrollable::AbsoluteOffset {
                x: target_x,
                y: target_y,
            },
        )
    }

    fn open_repo(&mut self, repo: &Repo) -> Task<Message> {
        self.open_repo_with(repo, self.config.open_mode)
    }

    fn open_repo_with(&mut self, repo: &Repo, mode: OpenMode) -> Task<Message> {
        if !repo.path_known {
            self.status = Some(format!(
                "{} is an external dependency (no local path)",
                repo.name
            ));
            return Task::none();
        }
        let repo = repo.clone();
        let config = self.config.clone();
        self.status = Some(format!("opening {}…", repo.name));
        Task::perform(
            async move { launch::open_project_with_mode(&repo, &config, mode) },
            Message::Opened,
        )
    }

    fn clone_repo(&mut self, repo: &Repo) -> Task<Message> {
        let Some(remote) = repo.remote.clone() else {
            return Task::none();
        };
        let dest = clone_destination(&self.config, repo);
        let name = repo.name.clone();
        self.status = Some(format!("cloning {}…", name));
        Task::perform(
            async move { launch::clone_repo(&remote, &dest) },
            Message::Cloned,
        )
    }

    fn open(&mut self, index: usize) -> Task<Message> {
        let repo = self.filtered().nth(index).cloned();
        match repo {
            Some(repo) => self.open_repo(&repo),
            None => Task::none(),
        }
    }

    fn open_with(&mut self, index: usize, mode: OpenMode) -> Task<Message> {
        let repo = self.filtered().nth(index).cloned();
        match repo {
            Some(repo) => self.open_repo_with(&repo, mode),
            None => Task::none(),
        }
    }

    fn clone(&mut self, index: usize) -> Task<Message> {
        let repo = self.filtered().nth(index).cloned();
        match repo {
            Some(repo) => self.clone_repo(&repo),
            None => Task::none(),
        }
    }
}

/// Where a clone should land: the configured (but not yet existing) path if
/// one was given, otherwise the first configured root joined with the repo
/// name, falling back to the current directory.
fn clone_destination(config: &Config, repo: &Repo) -> std::path::PathBuf {
    if !repo.path.as_os_str().is_empty() {
        repo.path.clone()
    } else if let Some(root) = config.resolved_roots().first() {
        root.join(&repo.name)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(&repo.name)
    }
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::SearchInputChanged(query) => {
            app.query = query;
            app.status = None;
            app.clamp_selection();
            Task::none()
        }
        Message::SearchSubmitted => match app.selected_repo().cloned() {
            Some(repo) => app.open_repo(&repo),
            None => Task::none(),
        },
        Message::FocusSearch => {
            // Select the existing query so typing replaces it.
            Task::batch([
                widget::operation::focus(SEARCH_ID),
                widget::operation::select_all(SEARCH_ID),
            ])
        }
        Message::OpenAt(index) => app.open(index),
        Message::OpenGraphNode(index) => match app.graph.nodes.get(index).cloned() {
            Some(repo) => app.open_repo(&repo),
            None => Task::none(),
        },
        Message::OpenAtMode(index, mode) => app.open_with(index, mode),
        Message::OpenGraphNodeMode(index, mode) => match app.graph.nodes.get(index).cloned() {
            Some(repo) => app.open_repo_with(&repo, mode),
            None => Task::none(),
        },
        Message::CloneAt(index) => app.clone(index),
        Message::CloneGraphNode(index) => match app.graph.nodes.get(index).cloned() {
            Some(repo) => app.clone_repo(&repo),
            None => Task::none(),
        },
        Message::Cloned(Ok(detail)) => {
            app.status = Some(detail);
            // The clone now exists on disk, so re-scan so the repo becomes a
            // real, openable project.
            app.config = Config::reload();
            app.rebuild_graph();
            app.clamp_selection();
            Task::none()
        }
        Message::Cloned(Err(err)) => {
            app.status = Some(format!("clone failed: {err}"));
            Task::none()
        }
        Message::Previous => {
            app.status = None;
            match app.view {
                View::List => {
                    let count = app.filtered().count();
                    if count > 0 {
                        app.selected = if app.selected == 0 {
                            count - 1
                        } else {
                            app.selected - 1
                        };
                    }
                }
                View::Graph => {
                    let order = app.graph_order();
                    if order.is_empty() {
                        app.selected = 0;
                    } else {
                        let pos = order.iter().position(|&i| i == app.selected).unwrap_or(0);
                        app.selected = order[(pos + order.len() - 1) % order.len()];
                    }
                }
            }
            app.scroll_to_selection()
        }
        Message::Next => {
            app.status = None;
            match app.view {
                View::List => {
                    let count = app.filtered().count();
                    if count > 0 {
                        app.selected = (app.selected + 1) % count;
                    }
                }
                View::Graph => {
                    let order = app.graph_order();
                    if order.is_empty() {
                        app.selected = 0;
                    } else {
                        let pos = order.iter().position(|&i| i == app.selected).unwrap_or(0);
                        app.selected = order[(pos + 1) % order.len()];
                    }
                }
            }
            app.scroll_to_selection()
        }
        Message::Escape => {
            if !app.query.is_empty() {
                app.query.clear();
                app.selected = 0;
                widget::operation::focus(SEARCH_ID)
            } else {
                Task::none()
            }
        }
        Message::Refresh => {
            app.config = Config::reload();
            app.rebuild_graph();
            app.selected = 0;
            app.clamp_selection();
            app.status = Some(format!("reloaded {} repos from config", app.repos.len()));
            Task::none()
        }
        Message::OpenConfig => {
            let config = app.config.clone();
            app.status = Some("opening config…".to_string());
            Task::perform(async move { launch::open_config(&config) }, Message::Opened)
        }
        Message::ViewChanged(view) => {
            if app.view != view {
                // The selection index means different things per view (a
                // filtered-list index vs. a graph node index), so carry the
                // highlighted repo across the switch by name.
                let name = app.selected_repo().map(|repo| repo.name.clone());
                app.view = view;
                app.selected = 0;
                if let Some(name) = name {
                    let found = match view {
                        View::Graph => app.graph.nodes.iter().position(|n| n.name == name),
                        View::List => app.filtered().position(|repo| repo.name == name),
                    };
                    if let Some(index) = found {
                        app.selected = index;
                    }
                }
                app.clamp_selection();
            }
            Task::none()
        }
        Message::GraphScrolled(viewport) => {
            app.graph_viewport = Some(viewport);
            Task::none()
        }
        Message::Opened(Ok(detail)) => {
            app.status = Some(detail);
            Task::none()
        }
        Message::Opened(Err(err)) => {
            app.status = Some(format!("failed to open: {err}"));
            Task::none()
        }
        Message::WindowReady(id) => {
            app.window = id;
            Task::none()
        }
        Message::Hotkey(hotkey::Event::Toggle) => toggle_window(app),
        Message::Kde(kde::Event::Toggle) => toggle_window(app),
        Message::Tray(tray::Event::Toggle) => toggle_window(app),
        Message::Tray(tray::Event::Quit) => quit(app),
        Message::CloseRequested => {
            // With a tray icon enabled the close button hides to the tray
            // instead of quitting. A real quit is signalled through the tray's
            // Quit item (or a second close while already quitting).
            if app.config.tray && !app.quitting {
                app.hidden = true;
                match app.window {
                    Some(id) => window::set_mode(id, Mode::Hidden),
                    None => Task::none(),
                }
            } else {
                quit(app)
            }
        }
    }
}

fn toggle_window(app: &mut App) -> Task<Message> {
    let Some(id) = app.window else {
        return Task::none();
    };
    if app.hidden {
        app.hidden = false;
        let mut task: Task<()> = window::set_mode(id, Mode::Windowed);
        // Re-anchor above the toolbar; Wayland ignores this, X11/XWayland
        // honors it. Best-effort.
        if let Some(position) = crate::geometry::window_position() {
            task = Task::batch([window::move_to(id, position), task]);
        }
        task.map(|_| Message::Escape)
    } else {
        app.hidden = true;
        window::set_mode(id, Mode::Hidden)
    }
}

fn quit(app: &mut App) -> Task<Message> {
    app.quitting = true;
    match app.window {
        Some(id) => window::close(id),
        None => std::process::exit(0),
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let App {
        repos,
        query,
        selected,
        status,
        view,
        ..
    } = app;

    let search = text_input("Search projects…", query)
        .id(SEARCH_ID)
        .padding(12)
        .on_input(Message::SearchInputChanged)
        .on_submit(Message::SearchSubmitted);

    let body: Row<'_, Message> = Row::new().push(container(search).width(Length::Fill));

    let content: Element<'_, Message> = match view {
        View::List => {
            let count = app.filtered().count();
            let list: Column<'_, Message> = if count == 0 {
                let msg = if repos.is_empty() {
                    "No projects found — configure roots or repos in \
                     ~/.config/repo-zoo/config.toml"
                } else {
                    "No matches"
                };
                column![
                    container(text(msg).size(15))
                        .width(Length::Fill)
                        .padding(24)
                ]
            } else {
                let items: Vec<_> = app
                    .filtered()
                    .enumerate()
                    .map(|(i, repo)| repo_row(repo, i == *selected, i))
                    .collect();
                column(items).width(Length::Fill).padding(4).spacing(2)
            };
            scrollable(list)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        }
        View::Graph => {
            let scrolled = scrollable(graph_view::graph_canvas(
                &app.graph,
                app.layout.clone(),
                query,
                app.selected,
            ))
            .id(GRAPH_SCROLL_ID)
            .on_scroll(Message::GraphScrolled)
            .width(Length::Fill)
            .height(Length::Fill);
            let hint = container(
                text("click node to open · ✎ editor · ▸ terminal · ▣ manager · ⬇ clone").size(13),
            )
            .padding([0, 8]);
            column![hint, scrolled]
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(4)
                .into()
        }
    };

    let footer_status = match status {
        Some(status) => text(status).size(13),
        None => text("↑/↓ navigate · Enter open · Esc clears · ↻ reloads config").size(13),
    };

    let footer = Row::new()
        .push(footer_status)
        .push(
            text(format!(
                "{} project{}",
                app.filtered().count(),
                if app.filtered().count() == 1 { "" } else { "s" }
            ))
            .size(13)
            .width(Length::Fill)
            .align_x(alignment::Horizontal::Right),
        )
        .spacing(8)
        .padding(8);

    container(column![header(*view), body, content, footer,])
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(12)
        .into()
}

fn header(_view: View) -> Row<'static, Message> {
    Row::new()
        .push(text("repo-zoo").size(22))
        .push(
            text("code project launcher")
                .size(13)
                .width(Length::Fill)
                .color(Color::from_rgb(0.55, 0.55, 0.6)),
        )
        .push(container(pick_list(
            &VIEWS[..],
            Some(_view),
            Message::ViewChanged,
        )))
        .push(button(container(text("⚙").size(16)).padding([2, 5])).on_press(Message::OpenConfig))
        .push(button(container(text("↻").size(16)).padding([2, 5])).on_press(Message::Refresh))
        .align_y(alignment::Vertical::Center)
        .spacing(10)
        .padding([5, 0])
}

fn repo_row(repo: &Repo, is_selected: bool, index: usize) -> Element<'_, Message> {
    let badge = match repo.kind {
        project::Kind::Vcs => "git",
        project::Kind::Dir => "ext",
    };

    let remote_hint = repo
        .remote
        .as_ref()
        .map(|r| format!("remote: {r}"))
        .unwrap_or_else(|| "no remote".to_string());

    let content: Row<'_, Message> = Row::new()
        .push(text(&repo.name).size(16))
        .push(
            text(repo.path.to_string_lossy())
                .size(12)
                .color(Color::from_rgb(0.55, 0.55, 0.6)),
        )
        .push(text(badge).size(11))
        .align_y(alignment::Vertical::Center)
        .spacing(10)
        .padding(8);

    let body = button(content)
        .width(Length::Fill)
        .style(iced::widget::button::text)
        .on_press(Message::OpenAt(index));

    let mut actions = Row::new().spacing(4);
    if repo.path_known {
        actions = actions
            .push(action_button(
                "✎",
                "Open in editor",
                Message::OpenAtMode(index, OpenMode::Editor),
            ))
            .push(action_button(
                "▸",
                "Open in terminal",
                Message::OpenAtMode(index, OpenMode::Terminal),
            ))
            .push(action_button(
                "▣",
                "Open in file manager",
                Message::OpenAtMode(index, OpenMode::Manager),
            ));
    } else if repo.remote.is_some() {
        actions = actions.push(action_button(
            "⬇",
            "Clone repository",
            Message::CloneAt(index),
        ));
    }

    let row = Row::new()
        .push(body)
        .push(actions)
        .align_y(alignment::Vertical::Center)
        .spacing(4)
        .padding(4);

    let styled = container(row)
        .width(Length::Fill)
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            let mut style = iced::widget::container::Style {
                border: iced::Border::default().rounded(4.0),
                ..Default::default()
            };
            if is_selected {
                style.background = Some(palette.primary.weak.color.into());
            }
            style
        });

    iced::widget::tooltip(
        styled,
        text(remote_hint).size(11),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

fn action_button<'a>(
    glyph: &'static str,
    tip: &'static str,
    message: Message,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        button(container(text(glyph).size(14)).padding([4, 6]))
            .style(iced::widget::button::subtle)
            .on_press(message),
        text(tip).size(11),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

pub fn subscription(app: &App) -> Subscription<Message> {
    let keyboard: Subscription<Message> = keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => {
            if modifiers.command() && key.as_ref() == Key::Character("f") {
                return Some(Message::FocusSearch);
            }
            match key {
                Key::Named(key::Named::ArrowDown) => Some(Message::Next),
                Key::Named(key::Named::ArrowUp) => Some(Message::Previous),
                Key::Named(key::Named::Escape) => Some(Message::Escape),
                _ => None,
            }
        }
        _ => None,
    });

    // On Plasma the hotkey is handled natively by KWin (see `kde`); using a
    // plain X11 grab there too would toggle twice per press. Elsewhere the X11
    // listener is used.
    let hotkey = if crate::kde::is_plasma() {
        Subscription::none()
    } else {
        crate::hotkey::subscription(&app.config).map(Message::Hotkey)
    };
    let kde = crate::kde::subscription(&app.config).map(Message::Kde);
    let tray = crate::tray::subscription(&app.config).map(Message::Tray);
    let close = window::close_requests().map(|_| Message::CloseRequested);

    Subscription::batch([keyboard, hotkey, kde, tray, close])
}
