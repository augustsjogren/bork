use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{App, InputMode, MessageKind};
use crate::ui::styles;

/// "bonk" letter-chase busy spinner.
fn spinner_spans(app: &App) -> Vec<Span<'static>> {
    const LETTERS: [&str; 4] = ["b", "o", "n", "k"];
    let lit = (app.spinner_tick / 2) % LETTERS.len();
    let lit_style = Style::default()
        .fg(styles::ACCENT)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(styles::DIM);
    LETTERS
        .iter()
        .enumerate()
        .map(|(i, letter)| Span::styled(*letter, if i == lit { lit_style } else { dim_style }))
        .collect()
}

pub fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let has_swimlanes = app.visible_swimlane_count() > 1;
    let mut title_spans = vec![Span::styled(
        " BONK ",
        Style::default()
            .fg(styles::ACCENT)
            .add_modifier(Modifier::BOLD),
    )];
    if !has_swimlanes {
        title_spans.push(Span::styled(
            format!("- {} ", app.active_project().display_name()),
            Style::default().fg(styles::TEXT),
        ));
    }
    if let Some(mission) = mission_filter_span(app) {
        title_spans.push(mission);
    }
    let needs_you = app.visible_attention_count();
    if needs_you > 0 {
        title_spans.push(Span::styled(
            format!(" \u{25cf} {needs_you} need you "),
            Style::default().fg(ratatui::style::Color::Yellow),
        ));
    }
    if let Some(base) = app.active_project().live.base_sync.as_ref() {
        if base.behind > 0 {
            title_spans.push(Span::styled(
                format!(" {} \u{2193}{} ", base.branch, base.behind),
                Style::default().fg(ratatui::style::Color::Yellow),
            ));
        }
    }

    if app.active_project().config.debug {
        title_spans.push(Span::styled(
            " [DEBUG]",
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // The backup warning keeps its space and drops its key hint when the
    // title wouldn't fit next to it; the title is cut before it after that.
    let title_width: usize = title_spans.iter().map(|span| span.width()).sum();
    let mut title_area = area;
    let warning = backup_warning_span(app, false)
        .filter(|full| title_width + full.width() <= area.width as usize)
        .or_else(|| backup_warning_span(app, true));
    if let Some(warning) = warning {
        let width = (warning.width() as u16).min(area.width);
        title_area.width = area.width - width;
        let warning_area = Rect::new(title_area.right(), area.y, width, area.height);
        frame.render_widget(Paragraph::new(Line::from(warning)), warning_area);
    }

    let title = Line::from(title_spans);
    frame.render_widget(Paragraph::new(title), title_area);
}

/// Longest mission title the header shows before cutting it.
const MISSION_TITLE_MAX: usize = 40;

/// Header label for an active mission filter.
fn mission_filter_span(app: &App) -> Option<Span<'static>> {
    let project = app.active_project();
    let mission_id = project.mission_filter.as_deref()?;
    let label = match project.issues.iter().find(|issue| issue.id == mission_id) {
        Some(mission) => format!(
            " \u{25c6} {} ",
            styles::truncate(&mission.title, MISSION_TITLE_MAX)
        ),
        None => format!(" \u{25c6} {mission_id} "),
    };
    Some(Span::styled(
        label,
        Style::default().fg(ratatui::style::Color::Magenta),
    ))
}

/// Header warning when the last backup failed or is too old; nothing
/// otherwise. `compact` drops the key hint.
fn backup_warning_span(app: &App, compact: bool) -> Option<Span<'static>> {
    use crate::backup::schedule::BackupIndicator;

    let text = app.backup_warning.as_deref()?;
    let (color, hint) = match app.backup {
        BackupIndicator::Failed { .. } => (ratatui::style::Color::Red, "B to retry"),
        _ => (ratatui::style::Color::Yellow, "B to back up"),
    };
    let text = if compact {
        format!(" \u{26a0} {text} ")
    } else {
        format!(" \u{26a0} {text} ({hint}) ")
    };
    Some(Span::styled(text, Style::default().fg(color)))
}

pub const SPINNER_WIDTH: u16 = 8;

pub fn render_loading_spinner(frame: &mut Frame, app: &App, area: Rect) {
    if area.width < SPINNER_WIDTH || area.height == 0 {
        return;
    }
    let target = Rect::new(
        area.right() - SPINNER_WIDTH,
        area.bottom() - 1,
        SPINNER_WIDTH,
        1,
    );
    let mut spans = vec![Span::raw("  ")];
    spans.extend(spinner_spans(app));
    spans.push(Span::raw(" "));
    frame.render_widget(Paragraph::new(Line::from(spans)), target);
}

pub fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let loading = app.is_busy_visible() || app.github_loading_visible();
    let mut content = area;
    if loading {
        content.width = content.width.saturating_sub(SPINNER_WIDTH);
    }
    render_footer_content(frame, app, content);
    if loading {
        render_loading_spinner(frame, app, area);
    }
}

/// One-line text field with a block cursor, used by search and rename.
fn render_text_input(frame: &mut Frame, area: Rect, text: &str) {
    let line = Line::from(vec![
        Span::styled(
            text.to_string(),
            Style::default()
                .fg(styles::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("\u{2588}", Style::default().fg(styles::ACCENT)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_footer_content(frame: &mut Frame, app: &App, area: Rect) {
    // Confirm mode
    if app.input_mode == InputMode::Confirm {
        if let Some(ref msg) = app.confirm_message {
            let line = Line::from(Span::styled(
                format!(" {msg}"),
                Style::default()
                    .fg(ratatui::style::Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
            frame.render_widget(Paragraph::new(line), area);
            return;
        }
    }

    // Search mode: show /query with cursor
    if app.input_mode == InputMode::Search {
        render_text_input(frame, area, &format!(" /{}", app.search_query));
        return;
    }

    if app.input_mode == InputMode::SidebarRename {
        let buffer = app
            .sidebar
            .as_ref()
            .and_then(|sidebar| sidebar.rename.as_ref())
            .map_or("", |rename| rename.buffer.as_str());
        render_text_input(frame, area, &format!(" Rename: {buffer}"));
        return;
    }

    // Sidebar mode
    if app.input_mode == InputMode::Sidebar {
        let line = Line::from(vec![
            Span::styled(" j", styles::statusbar_key_style()),
            Span::styled("/", styles::statusbar_desc_style()),
            Span::styled("k", styles::statusbar_key_style()),
            Span::styled(":nav  ", styles::statusbar_desc_style()),
            Span::styled("Enter", styles::statusbar_key_style()),
            Span::styled(":switch  ", styles::statusbar_desc_style()),
            Span::styled("Space", styles::statusbar_key_style()),
            Span::styled(":swimlane  ", styles::statusbar_desc_style()),
            Span::styled("Shift+J", styles::statusbar_key_style()),
            Span::styled("/", styles::statusbar_desc_style()),
            Span::styled("K", styles::statusbar_key_style()),
            Span::styled(":move  ", styles::statusbar_desc_style()),
            Span::styled("Shift+R", styles::statusbar_key_style()),
            Span::styled(":rename  ", styles::statusbar_desc_style()),
            Span::styled("Shift+D", styles::statusbar_key_style()),
            Span::styled(":remove  ", styles::statusbar_desc_style()),
            Span::styled("Esc", styles::statusbar_key_style()),
            Span::styled(":close", styles::statusbar_desc_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    // Overlay modes: footer is handled by the overlay itself
    if matches!(
        app.input_mode,
        InputMode::Dialog
            | InputMode::StackDetails
            | InputMode::LinearPicker
            | InputMode::LinkPicker
            | InputMode::Help
            | InputMode::Bonk
            | InputMode::DebugInspector
    ) {
        return;
    }

    // Temporary message
    if let Some((ref msg, kind)) = app.message {
        let color = match kind {
            MessageKind::Info => styles::DIM,
            MessageKind::Warning => ratatui::style::Color::Yellow,
            MessageKind::Error => ratatui::style::Color::Red,
        };
        let left = Line::from(Span::styled(format!(" {msg}"), Style::default().fg(color)));
        frame.render_widget(Paragraph::new(left), area);
        render_right_indicators(frame, app, area);
        return;
    }

    // Active link filter indicator
    if let Some(anchor) = &app.active_project().link_filter {
        let line = Line::from(vec![
            Span::styled(
                format!(" \u{221e} linked: {}", anchor),
                Style::default().fg(styles::ACCENT),
            ),
            Span::raw("  "),
            Span::styled("f/Esc", styles::statusbar_key_style()),
            Span::styled(":clear", styles::statusbar_desc_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        render_right_indicators(frame, app, area);
        return;
    }

    if app.attention_filter_active() {
        let line = Line::from(vec![
            Span::styled(
                " \u{25cf} needs you",
                Style::default().fg(ratatui::style::Color::Yellow),
            ),
            Span::raw("  "),
            Span::styled("!/Esc", styles::statusbar_key_style()),
            Span::styled(":clear", styles::statusbar_desc_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        render_right_indicators(frame, app, area);
        return;
    }

    // Active search filter indicator
    if app.has_active_search() {
        let line = Line::from(vec![
            Span::styled(
                format!(" /{}", app.search_query),
                Style::default().fg(styles::ACCENT),
            ),
            Span::raw("  "),
            Span::styled("Esc", styles::statusbar_key_style()),
            Span::styled(":clear", styles::statusbar_desc_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        render_right_indicators(frame, app, area);
        return;
    }

    let swimlane_count = app.visible_swimlane_count();

    let mut bindings: Vec<(&str, &str)> = vec![
        ("h/l", "focus"),
        ("j/k", "nav"),
        ("Enter", "open"),
        ("t", "term"),
        ("a", "add"),
        ("n", "new"),
        ("?", "help"),
        ("q", "quit"),
    ];

    if app
        .active_project()
        .selected_issue(&app.search_query)
        .is_some_and(|issue| issue.github_stack.is_some())
        && !app.active_project().live.gh_missing
        && !app.active_project().live.stacks_unsupported
    {
        bindings.insert(0, ("s", "view stack"));
    }
    if swimlane_count > 1 {
        bindings.insert(0, ("Tab", "lane"));
    }

    let mut spans = vec![Span::raw(" ")];
    for (i, (key, desc)) in bindings.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*key, styles::statusbar_key_style()));
        spans.push(Span::styled(
            format!(":{desc}"),
            styles::statusbar_desc_style(),
        ));
    }

    // Right side: swimlane indicator and/or update banner
    let mut right_spans: Vec<Span> = Vec::new();

    if swimlane_count > 1 {
        let active_name = app.active_project().display_name();
        right_spans.push(Span::styled(
            format!(
                "{} ({}/{}) ",
                active_name,
                app.focused_swimlane + 1,
                swimlane_count
            ),
            Style::default().fg(styles::ACCENT),
        ));
    }

    if app.update_available {
        if !right_spans.is_empty() {
            right_spans.push(Span::raw(" "));
        }
        right_spans.push(update_span());
    }

    if !right_spans.is_empty() {
        let left_width: usize = spans.iter().map(|s| s.width()).sum();
        let right_width: usize = right_spans.iter().map(|s| s.width()).sum();
        let gap = (area.width as usize).saturating_sub(left_width + right_width);
        if gap > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
            spans.extend(right_spans);
        }
    }

    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line), area);
}

fn update_span() -> Span<'static> {
    Span::styled(
        "↑ Update Available ",
        Style::default()
            .fg(ratatui::style::Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

fn render_right_indicators(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    if app.update_available {
        spans.push(update_span());
    }
    if spans.is_empty() {
        return;
    }
    let line = Line::from(spans);
    let right = Paragraph::new(line).alignment(Alignment::Right);
    frame.render_widget(right, area);
}
