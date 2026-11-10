use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::{App, SidebarState};
use crate::ui::styles;

pub const SIDEBAR_WIDTH: u16 = 22;
const SIDEBAR_NAME_PADDING: u16 = 5;
/// Two border rows plus one mission row.
const MIN_MISSIONS_HEIGHT: u16 = 3;

pub fn render_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let sidebar = match &app.sidebar {
        Some(s) => s,
        None => return,
    };

    let border_color = if sidebar.focused {
        styles::ACCENT
    } else {
        Color::DarkGray
    };

    if app.active_project().open_missions().next().is_none() {
        render_projects(frame, app, sidebar, border_color, area);
        return;
    }

    let projects_height =
        (app.projects.len() as u16 + 2).min(area.height.saturating_sub(MIN_MISSIONS_HEIGHT));
    let [projects_area, missions_area] =
        Layout::vertical([Constraint::Length(projects_height), Constraint::Min(0)]).areas(area);
    render_projects(frame, app, sidebar, border_color, projects_area);
    render_missions(frame, app, sidebar, border_color, missions_area);
}

fn render_projects(
    frame: &mut Frame,
    app: &App,
    sidebar: &SidebarState,
    border_color: Color,
    area: Rect,
) {
    let block = Block::default()
        .title(" Projects ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .enumerate()
        .map(|(i, project)| {
            let proj_id = project.id();
            let is_focused = proj_id == app.focused_project;
            let is_swimlane = sidebar.swimlanes.contains(&proj_id);
            let needs_you = project.attention_count() > 0;

            let marker = if is_focused {
                "◆"
            } else if is_swimlane {
                "▪"
            } else if needs_you {
                "●"
            } else {
                " "
            };

            let marker_style = if is_focused || is_swimlane {
                Style::default().fg(styles::ACCENT)
            } else if needs_you {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let has_ports = project.has_listening_ports();

            let name = project.display_name();
            // The 🔌 is 2 cells wide plus a leading space; reserve room so the
            // name truncation never pushes the icon past the sidebar edge.
            let port_reserve = if has_ports { 3 } else { 0 };
            let max_name =
                area.width
                    .saturating_sub(SIDEBAR_NAME_PADDING + port_reserve) as usize;
            let display_name = styles::truncate(name, max_name);

            let name_style = if is_focused {
                Style::default()
                    .fg(styles::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if is_swimlane {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let mut spans = vec![
                Span::styled(format!(" {} ", marker), marker_style),
                Span::styled(display_name, name_style),
            ];
            if has_ports {
                spans.push(Span::styled(" \u{1f50c}", Style::default()));
            }

            ListItem::new(Line::from(spans)).style(row_style(sidebar, i))
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

fn render_missions(
    frame: &mut Frame,
    app: &App,
    sidebar: &SidebarState,
    border_color: Color,
    area: Rect,
) {
    let block = Block::default()
        .title(" Missions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let project = app.active_project();
    let first_row = app.projects.len();

    let items: Vec<ListItem> = project
        .open_missions()
        .enumerate()
        .map(|(i, mission)| {
            let progress = project.mission_progress(&mission.id);
            let is_filtered = project.is_mission_filtered(&mission.id);

            let (marker, marker_style) = if is_filtered {
                ("◆", Style::default().fg(styles::ACCENT))
            } else if progress.needs_you {
                ("●", Style::default().fg(Color::Yellow))
            } else {
                (" ", Style::default().fg(Color::DarkGray))
            };

            let count = format!(" {}/{}", progress.done, progress.members);
            let max_title = area
                .width
                .saturating_sub(SIDEBAR_NAME_PADDING + count.len() as u16)
                as usize;
            let title_style = if is_filtered {
                Style::default()
                    .fg(styles::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let spans = vec![
                Span::styled(format!(" {} ", marker), marker_style),
                Span::styled(styles::truncate(&mission.title, max_title), title_style),
                Span::styled(count, Style::default().fg(Color::DarkGray)),
            ];
            ListItem::new(Line::from(spans)).style(row_style(sidebar, first_row + i))
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

fn row_style(sidebar: &SidebarState, row: usize) -> Style {
    if sidebar.focused && row == sidebar.selected {
        return Style::default().add_modifier(Modifier::REVERSED);
    }
    Style::default()
}
