use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
};

use crate::{followup_text, App, Look, Reasoning};

pub const HEADER_Y: u16 = 1;
pub const MASCOT_X: u16 = 3;
pub const MASCOT_WIDTH: u16 = 8;
pub const REASONING_X: u16 = 12;
pub const REASONING_WIDTH: u16 = 5;
pub const LOOK_X: u16 = 18;

pub fn look_width(look: Look) -> u16 {
    match look {
        Look::Dont => 12,
        Look::Look => 6,
    }
}

const INK: Color = Color::Rgb(226, 233, 243);
const QUESTION: Color = Color::Rgb(180, 190, 205);
const QUESTION_BG: Color = Color::Rgb(47, 52, 61);
const MUTED: Color = Color::Rgb(122, 138, 158);
const SHORTCUT_TEXT: Color = Color::Rgb(108, 122, 142);
const SHORTCUT_BG: Color = Color::Rgb(40, 44, 52);
const LOADING_TEXT: Color = Color::Rgb(166, 177, 191);
const BLUE: Color = Color::Rgb(110, 180, 255);
const CYAN: Color = Color::Rgb(91, 224, 219);
const GREEN: Color = Color::Rgb(112, 211, 154);
const EYE_WHITE: Color = Color::Rgb(255, 255, 255);
const PURPLE: Color = Color::Rgb(187, 149, 255);
const PINK: Color = Color::Rgb(255, 139, 180);
const GOLD: Color = Color::Rgb(255, 203, 123);
const ORANGE: Color = Color::Rgb(255, 166, 100);

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        let themes = ThemeSet::load_defaults();
        let theme = themes
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .or_else(|| themes.themes.values().next().cloned())
            .expect("syntect ships a theme");
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme,
        }
    }

    fn code_lines(&self, code: &[&str], language: &str) -> Vec<Line<'static>> {
        let syntax = self
            .syntaxes
            .find_syntax_by_token(language)
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        code.iter()
            .map(|line| {
                let spans = highlighter
                    .highlight_line(&format!("{line}\n"), &self.syntaxes)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(style, text)| {
                        let text = text.trim_end_matches('\n');
                        if text.is_empty() {
                            return None;
                        }
                        let color = style.foreground;
                        Some(Span::styled(
                            text.to_owned(),
                            Style::default().fg(Color::Rgb(color.r, color.g, color.b)),
                        ))
                    })
                    .collect::<Vec<_>>();
                Line::from(spans)
            })
            .collect()
    }
}

fn fence(line: &str) -> Option<(char, &str)> {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix("```")
        .map(|rest| ('`', rest))
        .or_else(|| trimmed.strip_prefix("~~~").map(|rest| ('~', rest)))
}

fn prose_spans(line: &str, failed: bool) -> Vec<Span<'static>> {
    line.split('`')
        .enumerate()
        .filter_map(|(index, text)| {
            if text.is_empty() {
                return None;
            }
            let style = if index % 2 == 1 {
                Style::default().fg(GOLD)
            } else {
                Style::default().fg(if failed { PINK } else { INK })
            };
            Some(Span::styled(text.to_owned(), style))
        })
        .collect()
}

fn answer_lines(
    question: &str,
    answer: &str,
    failed: bool,
    highlighter: &Highlighter,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "Q: ",
            Style::default()
                .fg(BLUE)
                .bg(QUESTION_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            question.to_owned(),
            Style::default().fg(QUESTION).bg(QUESTION_BG),
        ),
    ])];
    let mut first_answer_line = true;
    let raw_lines: Vec<&str> = answer.lines().collect();
    let mut index = 0;
    while index < raw_lines.len() {
        if let Some((marker, rest)) = fence(raw_lines[index]) {
            let language = rest.split_whitespace().next().unwrap_or("");
            index += 1;
            let mut code = Vec::new();
            while index < raw_lines.len()
                && !matches!(fence(raw_lines[index]), Some((closing, _)) if closing == marker)
            {
                code.push(raw_lines[index]);
                index += 1;
            }
            if index < raw_lines.len() {
                index += 1;
            }
            for line in highlighter.code_lines(&code, language) {
                push_answer_line(&mut lines, line.spans, &mut first_answer_line);
            }
        } else {
            push_answer_line(
                &mut lines,
                prose_spans(raw_lines[index], failed),
                &mut first_answer_line,
            );
            index += 1;
        }
    }
    if first_answer_line {
        push_answer_line(&mut lines, Vec::new(), &mut first_answer_line);
    }
    lines
}

fn response_lines(app: &App, highlighter: &Highlighter) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for exchange in &app.history {
        lines.extend(answer_lines(
            &exchange.question,
            &exchange.answer,
            false,
            highlighter,
        ));
    }
    if !app.answer.is_empty() {
        lines.extend(answer_lines(
            &app.question,
            &app.answer,
            app.failed,
            highlighter,
        ));
    }
    lines
}

fn push_answer_line(
    lines: &mut Vec<Line<'static>>,
    mut spans: Vec<Span<'static>>,
    first_answer_line: &mut bool,
) {
    let mut prefixed = if *first_answer_line {
        vec![Span::styled(
            "A: ",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )]
    } else {
        vec![Span::raw("   ")]
    };
    prefixed.append(&mut spans);
    lines.push(Line::from(prefixed));
    *first_answer_line = false;
}

fn status_color(app: &App) -> Color {
    if app.working {
        [GOLD, ORANGE][(app.tick / 3) % 2]
    } else if app.failed {
        [PINK, Color::Rgb(235, 105, 128)][(app.tick / 3) % 2]
    } else if !app.answer.is_empty() {
        GREEN
    } else {
        MUTED
    }
}

fn mascot_line(app: &App) -> Line<'static> {
    let body_color = status_color(app);
    Line::from(
        app.mascot
            .frame()
            .chars()
            .map(|character| {
                let color = if matches!(character, 'O' | 'o' | '-') {
                    EYE_WHITE
                } else {
                    body_color
                };
                Span::styled(character.to_string(), Style::default().fg(color))
            })
            .collect::<Vec<_>>(),
    )
}

fn toggle(label: &str, color: Color) -> Paragraph<'_> {
    Paragraph::new(format!("[{label}]"))
        .style(Style::default().fg(color).add_modifier(Modifier::BOLD))
}

pub fn draw(frame: &mut Frame, app: &App, highlighter: &Highlighter) {
    let area = frame.area();
    if area.width < 49 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("Terminal needs a little more room").style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }

    frame.render_widget(
        Paragraph::new(mascot_line(app)),
        Rect::new(MASCOT_X, HEADER_Y, MASCOT_WIDTH, 1),
    );
    let (reasoning_label, reasoning_color) = match app.reasoning {
        Reasoning::Duh => ("duh", CYAN),
        Reasoning::Ooh => ("ooh", PURPLE),
    };
    frame.render_widget(
        toggle(reasoning_label, reasoning_color),
        Rect::new(REASONING_X, HEADER_Y, REASONING_WIDTH, 1),
    );
    let (look_label, look_color) = match app.look {
        Look::Dont => ("don't look", PINK),
        Look::Look => ("look", GOLD),
    };
    frame.render_widget(
        toggle(look_label, look_color),
        Rect::new(LOOK_X, HEADER_Y, look_width(app.look), 1),
    );

    let input_width = area.width.saturating_sub(6) as usize;
    let input = if app.input.is_empty() {
        Line::from(Span::styled(
            "Type your next kroak",
            Style::default().fg(MUTED),
        ))
    } else {
        let visible: Vec<char> = app
            .input
            .chars()
            .rev()
            .take(input_width.saturating_sub(usize::from(!app.working)))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let mut spans = Vec::new();
        let command_end = followup_text(&app.input).map(|_| 4).unwrap_or(0);
        if app.working {
            let letters: Vec<usize> = visible
                .iter()
                .enumerate()
                .filter_map(|(index, c)| {
                    (index >= command_end && c.is_alphanumeric()).then_some(index)
                })
                .collect();
            let active = letters.get(app.tick % letters.len().max(1)).copied();
            for (index, c) in visible.into_iter().enumerate() {
                let style = if index < command_end {
                    Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)
                } else if Some(index) == active {
                    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(LOADING_TEXT)
                };
                spans.push(Span::styled(c.to_string(), style));
            }
        } else {
            if command_end > 0 {
                let text: String = visible.iter().collect();
                let (command, rest) = text.split_at(command_end.min(text.len()));
                spans.push(Span::styled(
                    command.to_owned(),
                    Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(rest.to_owned(), Style::default().fg(INK)));
            } else {
                spans.push(Span::styled(
                    visible.into_iter().collect::<String>(),
                    Style::default().fg(INK),
                ));
            }
            spans.push(Span::styled("▌", Style::default().fg(CYAN)));
        }
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(input), Rect::new(3, 3, area.width - 6, 1));

    if !app.answer.is_empty() || !app.history.is_empty() {
        let lines = response_lines(app, highlighter);
        let content_height = area.height.saturating_sub(7);
        let line_width = area.width.saturating_sub(6).max(1) as usize;
        let line_count: usize = lines
            .iter()
            .map(|line| line.width().max(1).div_ceil(line_width))
            .sum();
        let max_scroll = line_count.saturating_sub(content_height as usize) as u16;
        let response_area = Rect::new(3, 5, area.width - 6, content_height);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((app.scroll.min(max_scroll), 0)),
            response_area,
        );
        let buffer = frame.buffer_mut();
        for y in response_area.top()..response_area.bottom() {
            let question_row = (response_area.left()..response_area.right())
                .any(|x| buffer[(x, y)].bg == QUESTION_BG);
            if question_row {
                buffer.set_style(
                    Rect::new(response_area.x, y, response_area.width, 1),
                    Style::default().bg(QUESTION_BG),
                );
            }
        }
    }

    let shortcuts = if app.selection_mode {
        "F3 back to clicks · drag to select text · Esc quit"
    } else {
        "F1 reasoning · F2 file access · F3 select · Enter ask · Esc quit"
    };
    frame.render_widget(
        Paragraph::new(app.launch_dir.to_string_lossy().into_owned())
            .style(Style::default().fg(MUTED)),
        Rect::new(3, area.height - 2, area.width - 6, 1),
    );
    frame.render_widget(
        Paragraph::new(shortcuts).style(Style::default().fg(SHORTCUT_TEXT).bg(SHORTCUT_BG)),
        Rect::new(3, area.height - 1, area.width - 6, 1),
    );
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::mpsc};

    use ratatui::{backend::TestBackend, Terminal};

    use super::*;

    #[test]
    fn footer_shows_launch_folder_and_selection_shortcut() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/my-project"), sender);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = Highlighter::new();
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        let row = |terminal: &Terminal<TestBackend>, y| -> String {
            (0..80)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_owned())
                .collect()
        };
        assert!(row(&terminal, 22).contains("/tmp/my-project"));
        assert!(row(&terminal, 23).contains("F3 select"));
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 22)].bg, Color::Reset);
        assert_eq!(buffer[(3, 23)].fg, SHORTCUT_TEXT);
        assert_eq!(buffer[(3, 23)].bg, SHORTCUT_BG);
        assert_eq!(buffer[(76, 23)].bg, SHORTCUT_BG);

        app.selection_mode = true;
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        assert!(row(&terminal, 23).contains("drag to select text"));
    }

    #[test]
    fn mascot_body_stays_gray_or_green_except_while_working() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        let idle = status_color(&app);
        assert_eq!(idle, MUTED);
        app.tick = 2;
        assert_eq!(idle, status_color(&app));
        app.tick = 3;
        assert_eq!(idle, status_color(&app));
        app.working = true;
        let working = status_color(&app);
        assert_ne!(working, idle);
        app.tick = 6;
        assert_ne!(working, status_color(&app));
        app.working = false;
        app.answer = "Seven.".into();
        let done = status_color(&app);
        assert_eq!(done, GREEN);
        app.tick = 9;
        assert_eq!(done, status_color(&app));
    }

    #[test]
    fn mascot_eyes_remain_white_during_blinks_and_movement() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        let mut saw_closed_eyes = false;
        for _ in 0..40 {
            let line = mascot_line(&app);
            assert_eq!(line.spans.len(), 7);
            for span in &line.spans {
                let character = span.content.chars().next().unwrap();
                if matches!(character, 'O' | 'o' | '-') {
                    assert_eq!(span.style.fg, Some(EYE_WHITE));
                    saw_closed_eyes |= character == '-';
                } else {
                    assert_eq!(span.style.fg, Some(status_color(&app)));
                }
            }
            app.mascot.tick();
        }
        assert!(saw_closed_eyes);

        app.working = true;
        app.mascot.start_work();
        for _ in 0..6 {
            let line = mascot_line(&app);
            assert!(line
                .spans
                .iter()
                .filter(|span| span.content == "O")
                .all(|span| span.style.fg == Some(EYE_WHITE)));
            app.mascot.tick();
        }
    }

    #[test]
    fn clicked_mascot_fits_beside_the_mode_buttons() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.mascot.play_click();
        app.mascot.tick();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = Highlighter::new();
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let header: String = (3..30).map(|x| buffer[(x, HEADER_Y)].symbol()).collect();
        assert!(header.starts_with("▆▆O▆O▆▂▂ [duh]"));
        assert_eq!(
            buffer[(MASCOT_X + MASCOT_WIDTH - 1, HEADER_Y)].symbol(),
            "▂"
        );
        assert_eq!(buffer[(MASCOT_X + 2, HEADER_Y)].fg, EYE_WHITE);
    }

    #[test]
    fn question_has_subtle_background_and_labels_use_blue_and_green() {
        let lines = answer_lines("Question", "Answer", false, &Highlighter::new());
        assert_eq!(lines[0].spans[0].content, "Q: ");
        assert_eq!(lines[0].spans[0].style.fg, Some(BLUE));
        assert_eq!(lines[0].spans[0].style.bg, Some(QUESTION_BG));
        assert_eq!(lines[1].spans[0].content, "A: ");
        assert_eq!(lines[1].spans[0].style.fg, Some(GREEN));
        assert_eq!(lines[1].spans[0].style.bg, None);
        assert_eq!(lines[0].spans[1].style.fg, Some(QUESTION));
        assert_eq!(lines[0].spans[1].style.bg, Some(QUESTION_BG));
        assert_eq!(lines[1].spans[1].style.fg, Some(INK));
        assert_eq!(lines[1].spans[1].style.bg, None);
    }

    #[test]
    fn question_background_fills_entire_row_including_wrapped_rows() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.question = "frog ".repeat(30);
        app.answer = "Yes.".into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = Highlighter::new();
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let answer_y = (6..20).find(|&y| buffer[(3, y)].symbol() == "A").unwrap();
        assert!(answer_y > 6);
        for y in 5..answer_y {
            assert_eq!(buffer[(3, y)].bg, QUESTION_BG);
            assert_eq!(buffer[(76, y)].bg, QUESTION_BG);
        }
        assert_eq!(buffer[(3, answer_y)].bg, Color::Reset);
        assert_eq!(buffer[(76, answer_y)].bg, Color::Reset);
    }

    #[test]
    fn working_question_highlight_moves_one_letter_at_a_time() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.input = "frog".into();
        app.working = true;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = Highlighter::new();
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(3, 3)].fg, ORANGE);
        assert_eq!(terminal.backend().buffer()[(4, 3)].fg, LOADING_TEXT);
        app.tick = 1;
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(3, 3)].fg, LOADING_TEXT);
        assert_eq!(terminal.backend().buffer()[(4, 3)].fg, ORANGE);
        assert!((3..77).all(|x| terminal.backend().buffer()[(x, 3)].bg == Color::Reset));
    }

    #[test]
    fn fenced_code_uses_language_colors_without_showing_fences() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.question = "Show Rust".into();
        app.answer = "```rust\nfn main() { let n = \"hi\"; }\n```".into();
        let lines = answer_lines(&app.question, &app.answer, app.failed, &Highlighter::new());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].spans[0].content, "A: ");
        let code = &lines[1].spans[1..];
        assert!(!code.iter().any(|span| span.content.contains("```")));
        let colors: Vec<_> = code.iter().filter_map(|span| span.style.fg).collect();
        assert!(colors.iter().any(|color| *color != colors[0]));
    }

    #[test]
    fn inline_code_gets_a_distinct_color() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.answer = "Use `let n = 1;`.".into();
        let lines = answer_lines(&app.question, &app.answer, app.failed, &Highlighter::new());
        let inline = lines[1]
            .spans
            .iter()
            .find(|span| span.content == "let n = 1;")
            .unwrap();
        assert_eq!(inline.style.fg, Some(GOLD));
    }

    #[test]
    fn but_command_is_purple_while_question_is_ordinary_text() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.input = "/but why?".into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = Highlighter::new();
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 3)].fg, PURPLE);
        assert_eq!(buffer[(7, 3)].fg, INK);

        app.working = true;
        terminal
            .draw(|frame| draw(frame, &app, &highlighter))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 3)].fg, PURPLE);
        assert_eq!(buffer[(8, 3)].fg, ORANGE);
    }
}
