use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, TargetCandidate, TargetPickerState};
use crate::ui::styles;

const DIALOG_WIDTH: u16 = 72;
const MIN_HEIGHT: u16 = 8;

pub fn render_target_picker(frame: &mut Frame, app: &App) {
    let Some(picker) = app.target_picker.as_ref() else {
        return;
    };

    let area = frame.area();
    let width = DIALOG_WIDTH.min(area.width);
    let list_height = (picker.candidates.len() as u16).clamp(1, 16);
    let footer_height = if picker.error.is_some() { 3 } else { 2 };
    let height = (list_height + footer_height + 2)
        .max(MIN_HEIGHT)
        .min(area.height);

    let x = area.width.saturating_sub(width) / 2;
    let y = area.height.saturating_sub(height) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(styles::ACCENT))
        .title(Span::styled(
            format!(" Worktree target: {} ", picker.issue_id),
            Style::default()
                .fg(styles::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    let list_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer_height),
    );
    render_list(frame, picker, list_area);

    let footer_y = inner.y + inner.height.saturating_sub(footer_height);
    render_footer(
        frame,
        picker,
        Rect::new(inner.x, footer_y, inner.width, footer_height),
    );
}

fn render_list(frame: &mut Frame, picker: &TargetPickerState, area: Rect) {
    let visible = area.height as usize;
    let selected = picker.selected;

    let scroll = if picker.candidates.len() <= visible || selected < visible / 2 {
        0
    } else if selected + visible / 2 >= picker.candidates.len() {
        picker.candidates.len() - visible
    } else {
        selected - visible / 2
    };

    let rows: Vec<Line> = picker
        .candidates
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible)
        .map(|(i, candidate)| format_row(candidate, i == selected, area.width))
        .collect();

    frame.render_widget(Paragraph::new(rows), area);
}

fn format_row(candidate: &TargetCandidate, selected: bool, width: u16) -> Line<'static> {
    let prefix = if selected { "▸ " } else { "  " };
    let name = candidate.display.clone();

    let mut spans = vec![Span::raw(prefix.to_string())];
    let name_style = if selected {
        Style::default()
            .fg(styles::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::TEXT)
    };
    spans.push(Span::styled(format!("{name:<14}"), name_style));

    // A held target cannot serve this card, so say who has it rather than
    // showing its branch as if it were available.
    if let Some(held_by) = candidate.held_by.as_deref() {
        spans.push(Span::styled(
            format!("held by {held_by}"),
            Style::default().fg(ratatui::style::Color::Yellow),
        ));
        return Line::from(spans);
    }

    if let Some(label) = candidate.label.as_deref() {
        let budget = (width as usize).saturating_sub(prefix.len() + 16);
        spans.push(Span::styled(truncate(label, budget), styles::dim_style()));
    }

    Line::from(spans)
}

fn truncate(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let kept: String = text.chars().take(budget.saturating_sub(1)).collect();
    format!("{kept}…")
}

fn render_footer(frame: &mut Frame, picker: &TargetPickerState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(error) = picker.error.as_deref() {
        lines.push(Line::from(Span::styled(
            error.to_string(),
            Style::default()
                .fg(ratatui::style::Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(Span::styled(
        "j/k:move  Enter:set  Esc:cancel",
        styles::dim_style(),
    )));

    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(target: Option<&str>, label: Option<&str>, held: Option<&str>) -> TargetCandidate {
        TargetCandidate {
            display: target.unwrap_or("(no target)").to_string(),
            target: target.map(String::from),
            label: label.map(String::from),
            held_by: held.map(String::from),
        }
    }

    fn row_text(candidate: &TargetCandidate) -> String {
        format_row(candidate, false, 72)
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[test]
    fn clearing_row_is_labelled() {
        assert!(row_text(&candidate(None, None, None)).contains("(no target)"));
    }

    #[test]
    fn held_target_reports_its_holder_instead_of_a_label() {
        // Showing the branch would read as available.
        let text = row_text(&candidate(Some("a"), Some("some-branch"), Some("card-4")));

        assert!(text.contains("held by card-4"), "got: {text}");
        assert!(!text.contains("some-branch"), "got: {text}");
    }

    #[test]
    fn available_target_shows_its_label() {
        let text = row_text(&candidate(Some("a"), Some("some-branch"), None));

        assert!(text.contains("some-branch"), "got: {text}");
    }

    #[test]
    fn truncate_marks_elision() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(truncate("abc", 0), "");
    }
}
