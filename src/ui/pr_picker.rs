use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, PrPickerRow};
use crate::ui::styles;

const DIALOG_WIDTH: u16 = 80;
const MAX_LIST_HEIGHT: u16 = 16;
const FOOTER_HEIGHT: u16 = 2;

pub fn render_pr_picker(frame: &mut Frame, app: &App) {
    let Some(picker) = app.pr_picker.as_ref() else {
        return;
    };

    let area = frame.area();
    let width = DIALOG_WIDTH.min(area.width);
    let list_height = (picker.rows.len() as u16).clamp(1, MAX_LIST_HEIGHT);
    let height = (list_height + FOOTER_HEIGHT + 3).min(area.height);
    let popup_area = Rect::new(
        area.width.saturating_sub(width) / 2,
        area.height.saturating_sub(height) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, popup_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::ACCENT))
        .title(Span::styled(
            " Open PRs ",
            Style::default()
                .fg(styles::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height <= FOOTER_HEIGHT + 1 || inner.width < 20 {
        return;
    }

    let list_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height - FOOTER_HEIGHT - 1,
    );
    let visible = list_area.height as usize;
    let scroll = (picker.selected + 1).saturating_sub(visible);
    let rows: Vec<Line> = picker
        .rows
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible)
        .map(|(index, row)| format_row(row, index == picker.selected, inner.width))
        .collect();
    frame.render_widget(Paragraph::new(rows), list_area);

    let footer_area = Rect::new(
        inner.x,
        inner.y + inner.height - FOOTER_HEIGHT,
        inner.width,
        FOOTER_HEIGHT,
    );
    frame.render_widget(Paragraph::new(footer_lines()), footer_area);
}

fn format_row(row: &PrPickerRow, selected: bool, width: u16) -> Line<'static> {
    let row_style = if selected {
        Style::default()
            .fg(styles::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let prefix = if selected { "▸ " } else { "  " };
    let checkbox = if row.checked { "[x] " } else { "[ ] " };
    let number = format!("#{:<7}", row.number);
    let used = prefix.chars().count() + checkbox.len() + number.len();
    let title_width = (width as usize).saturating_sub(used).max(8);

    Line::from(vec![
        Span::styled(prefix, row_style),
        Span::styled(checkbox, row_style),
        Span::styled(number, styles::dim_style()),
        Span::styled(
            styles::truncate_to_width(&row.title, title_width),
            row_style,
        ),
    ])
}

fn footer_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("  Space", styles::statusbar_key_style()),
            Span::styled(" toggle  ", styles::statusbar_desc_style()),
            Span::styled("j/k", styles::statusbar_key_style()),
            Span::styled(" move", styles::statusbar_desc_style()),
        ]),
        Line::from(vec![
            Span::styled("  Enter", styles::statusbar_key_style()),
            Span::styled(" open selected  ", styles::statusbar_desc_style()),
            Span::styled("o", styles::statusbar_key_style()),
            Span::styled(" open all  ", styles::statusbar_desc_style()),
            Span::styled("Esc", styles::statusbar_key_style()),
            Span::styled(" cancel", styles::statusbar_desc_style()),
        ]),
    ]
}
