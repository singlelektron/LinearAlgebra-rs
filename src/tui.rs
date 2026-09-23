//! Terminal notebook. Editing and rendering live here; mathematics lives in Session.

use std::io;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthStr;

use crate::core::Value;
use crate::format::{render_output, render_value};
use crate::session::Session;

const MAX_EDITOR_BYTES: usize = 65_536;
const MAX_TRANSCRIPT_LINES: usize = 10_000;
const INK: Color = Color::Rgb(218, 224, 230);
const MUTED: Color = Color::Rgb(129, 144, 159);
const ACCENT: Color = Color::Rgb(136, 187, 180);
const BACKGROUND: Color = Color::Rgb(20, 25, 31);

#[derive(Default)]
struct Editor {
    text: String,
    /// Byte position, always on a UTF-8 boundary.
    cursor: usize,
}

impl Editor {
    fn insert(&mut self, text: &str) -> bool {
        let clean = text.replace("\r\n", "\n").replace('\r', "\n");
        let clean: String = clean
            .chars()
            .filter(|character| !character.is_control() || *character == '\n' || *character == '\t')
            .collect::<String>()
            .replace('\t', "    ");
        if self.text.len().saturating_add(clean.len()) > MAX_EDITOR_BYTES {
            return false;
        }
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        true
    }

    fn replace(&mut self, text: String) {
        self.cursor = text.len();
        self.text = text;
    }

    fn left(&mut self) {
        if let Some((index, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = index;
        }
    }

    fn right(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.drain(self.cursor..end);
    }

    fn delete(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.text
                .drain(self.cursor..self.cursor + character.len_utf8());
        }
    }

    fn home(&mut self) {
        self.cursor = self.text[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
    }

    fn end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
    }

    fn coordinates(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let row = before
            .chars()
            .filter(|character| *character == '\n')
            .count();
        let column = before.rsplit('\n').next().unwrap_or_default().width();
        (row, column)
    }
}

struct Workspace {
    session: Session,
    ascii: bool,
    editor: Editor,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
    transcript: Vec<Line<'static>>,
    scroll: usize,
    horizontal_scroll: u16,
    follow_output: bool,
    help: bool,
    variables: bool,
    variable_scroll: usize,
    variable_max_scroll: usize,
    help_scroll: usize,
    help_max_scroll: usize,
    status: String,
    quit: bool,
}

impl Workspace {
    fn new(session: Session, ascii: bool) -> Self {
        Self {
            session,
            ascii,
            editor: Editor::default(),
            history: Vec::new(),
            history_index: None,
            draft: String::new(),
            transcript: vec![
                Line::styled("A space to think in matrices.", Style::default().fg(ACCENT)),
                Line::from(""),
                Line::styled("A = [1 2; 3 4]     b = [5, 6]", Style::default().fg(MUTED)),
                Line::styled(
                    "solve(A, b)        det(A)        A'",
                    Style::default().fg(MUTED),
                ),
                Line::from(""),
                Line::styled(
                    "Exact rational arithmetic by default. F1 opens the guide.",
                    Style::default().fg(MUTED),
                ),
                Line::from(""),
            ],
            scroll: 0,
            horizontal_scroll: 0,
            follow_output: true,
            help: false,
            variables: false,
            variable_scroll: 0,
            variable_max_scroll: 0,
            help_scroll: 0,
            help_max_scroll: 0,
            status: "Ready".into(),
            quit: false,
        }
    }

    fn submit(&mut self) {
        let input = self.editor.text.trim().to_string();
        if input.is_empty() {
            return;
        }
        if self.history.last() != Some(&input) {
            self.history.push(input.clone());
            if self.history.len() > 200 {
                self.history.remove(0);
            }
        }
        self.history_index = None;
        self.draft.clear();
        self.transcript
            .extend(input.lines().enumerate().map(|(index, line)| {
                Line::styled(
                    format!(
                        "{} {line}",
                        if index == 0 {
                            if self.ascii { ">" } else { "❯" }
                        } else {
                            " "
                        }
                    ),
                    Style::default().fg(ACCENT),
                )
            }));
        match self.session.execute_script(&input) {
            Ok(outputs) => {
                for output in outputs {
                    self.quit |= output.quit;
                    let rendered = render_output(&output, &self.session, self.ascii);
                    self.transcript
                        .extend(rendered.lines().map(|line| Line::from(line.to_owned())));
                    if !rendered.is_empty() {
                        self.transcript.push(Line::from(""));
                    }
                }
                self.editor = Editor::default();
                self.status = "Evaluated".into();
            }
            Err(error) => {
                self.transcript.push(Line::styled(
                    format!("error: {error}"),
                    Style::default().fg(Color::Rgb(225, 157, 147)),
                ));
                self.transcript.push(Line::from(""));
                self.status = "Edit the expression and try again".into();
            }
        }
        if self.transcript.len() > MAX_TRANSCRIPT_LINES {
            self.transcript
                .drain(..self.transcript.len() - MAX_TRANSCRIPT_LINES);
        }
        self.follow_output = true;
        self.horizontal_scroll = 0;
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.draft = self.editor.text.clone();
                self.history.len() - 1
            }
        };
        self.history_index = Some(index);
        self.editor.replace(self.history[index].clone());
    }

    fn history_down(&mut self) {
        if let Some(index) = self.history_index {
            if index + 1 < self.history.len() {
                self.history_index = Some(index + 1);
                self.editor.replace(self.history[index + 1].clone());
            } else {
                self.history_index = None;
                self.editor.replace(self.draft.clone());
            }
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        if self.help {
            self.help_scroll = self.help_scroll.saturating_sub(lines);
            return;
        }
        if self.variables {
            self.variable_scroll = self.variable_scroll.saturating_sub(lines);
            return;
        }
        self.follow_output = false;
        self.scroll = self.scroll.saturating_sub(lines);
    }

    fn scroll_down(&mut self, lines: usize) {
        if self.help {
            self.help_scroll = self
                .help_scroll
                .saturating_add(lines)
                .min(self.help_max_scroll);
            return;
        }
        if self.variables {
            self.variable_scroll = self
                .variable_scroll
                .saturating_add(lines)
                .min(self.variable_max_scroll);
            return;
        }
        self.scroll = self.scroll.saturating_add(lines).min(self.transcript.len());
    }

    fn key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alternate = key.modifiers.contains(KeyModifiers::ALT);
        if control && key.code == KeyCode::Char('q') {
            self.quit = true;
            return;
        }
        if key.code == KeyCode::F(1) {
            self.help = !self.help;
            return;
        }
        if self.help || self.variables {
            let maximum = if self.help {
                self.help_max_scroll
            } else {
                self.variable_max_scroll
            };
            let scroll = if self.help {
                &mut self.help_scroll
            } else {
                &mut self.variable_scroll
            };
            match key.code {
                KeyCode::Esc | KeyCode::Tab => {
                    self.help = false;
                    self.variables = false;
                }
                KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Down => *scroll = scroll.saturating_add(1).min(maximum),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(5),
                KeyCode::PageDown => *scroll = scroll.saturating_add(5).min(maximum),
                KeyCode::Home => *scroll = 0,
                KeyCode::End => *scroll = maximum,
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('c') if control => {
                self.editor = Editor::default();
                self.history_index = None;
                self.status = "Input cleared".into();
            }
            KeyCode::Char('d') if control && self.editor.text.is_empty() => self.quit = true,
            KeyCode::Char('a') if control => self.editor.home(),
            KeyCode::Char('e') if control => self.editor.end(),
            KeyCode::Char('l') if control => {
                self.transcript.clear();
                self.scroll = 0;
                self.follow_output = true;
                self.status = "Notebook cleared; variables preserved".into();
            }
            KeyCode::Char('j') if control => {
                self.editor.insert("\n");
            }
            KeyCode::Enter if alternate || key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.editor.insert("\n");
            }
            KeyCode::Enter => self.submit(),
            KeyCode::Char(character) if !control && !alternate => {
                if !self.editor.insert(&character.to_string()) {
                    self.status = "Input limit: 65,536 bytes".into();
                }
            }
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Delete => self.editor.delete(),
            KeyCode::Left if alternate => {
                self.horizontal_scroll = self.horizontal_scroll.saturating_sub(8)
            }
            KeyCode::Right if alternate => {
                self.horizontal_scroll = self.horizontal_scroll.saturating_add(8)
            }
            KeyCode::Left => self.editor.left(),
            KeyCode::Right => self.editor.right(),
            KeyCode::Home if control => {
                self.scroll = 0;
                self.follow_output = false;
            }
            KeyCode::End if control => self.follow_output = true,
            KeyCode::Home => self.editor.home(),
            KeyCode::End => self.editor.end(),
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            KeyCode::PageUp => self.scroll_up(10),
            KeyCode::PageDown => self.scroll_down(10),
            KeyCode::Tab => self.variables = true,
            KeyCode::Esc => self.status = "F1 help · Ctrl+Q quit".into(),
            _ => {}
        }
    }
}

/// The guard is installed before any terminal mode is changed, so partial
/// initialization failures and every later `?` restore the user's terminal.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}

pub fn run(session: Session, ascii: bool) -> Result<(), String> {
    let _guard = TerminalGuard;
    enable_raw_mode().map_err(|error| error.to_string())?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(|error| error.to_string())?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|error| error.to_string())?;
    let mut workspace = Workspace::new(session, ascii);
    while !workspace.quit {
        terminal
            .draw(|frame| draw(frame, &mut workspace))
            .map_err(|error| error.to_string())?;
        {
            match event::read().map_err(|error| error.to_string())? {
                Event::Key(key) => workspace.key(key),
                Event::Paste(text) => {
                    if !workspace.help && !workspace.variables && !workspace.editor.insert(&text) {
                        workspace.status = "Paste exceeds input limit: 65,536 bytes".into();
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => workspace.scroll_up(3),
                    MouseEventKind::ScrollDown => workspace.scroll_down(3),
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(58, 71, 82)))
        .title(title)
}

fn variable_lines(workspace: &Workspace) -> Vec<Line<'static>> {
    if workspace.session.variables().is_empty() {
        return vec![
            Line::styled("No variables yet", Style::default().fg(MUTED)),
            Line::from(""),
            Line::from("A = [1 2; 3 4]"),
        ];
    }
    let mut lines = Vec::new();
    for (name, value) in workspace.session.variables() {
        let shape = match value {
            Value::Scalar(_) => "scalar".into(),
            Value::Matrix(matrix) => format!("{} × {}", matrix.rows(), matrix.cols()),
        };
        lines.push(Line::from(vec![
            Span::styled(
                name.clone(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("   {shape}"), Style::default().fg(MUTED)),
        ]));
        let rendered = match value {
            Value::Matrix(matrix) if matrix.rows() > 5 || matrix.cols() > 4 => {
                let first = matrix.data()[0]
                    .iter()
                    .take(4)
                    .map(|value| value.format(workspace.session.precision))
                    .collect::<Vec<_>>()
                    .join("  ");
                format!("[ {first} ... ]\nEnter {name} for the full matrix")
            }
            _ => render_value(value, workspace.session.precision, workspace.ascii),
        };
        lines.extend(
            rendered
                .lines()
                .take(5)
                .map(|line| Line::from(line.to_owned())),
        );
        if rendered.lines().count() > 5 {
            lines.push(Line::styled(
                "... enter the name for the full value",
                Style::default().fg(MUTED),
            ));
        }
        lines.push(Line::from(""));
    }
    lines
}

fn draw(frame: &mut Frame<'_>, workspace: &mut Workspace) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND).fg(INK)),
        area,
    );
    if area.width < 24 || area.height < 9 {
        frame.render_widget(
            Paragraph::new("Enlarge terminal to 24 × 9.\nCtrl+Q quits.").wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let input_height = (workspace
        .editor
        .text
        .split('\n')
        .count()
        .saturating_add(2)
        .clamp(3, 8) as u16)
        .min(area.height.saturating_sub(5));
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(2),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(area);
    let mode = format!("{}", workspace.session.mode);
    let detail = if area.width >= 72 {
        format!(
            "  {mode}  ·  {} variables  ·  steps {}",
            workspace.session.variables().len(),
            if workspace.session.show_steps {
                "on"
            } else {
                "off"
            }
        )
    } else {
        format!("  {mode}")
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " LINEAR ALGEBRA",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(detail, Style::default().fg(MUTED)),
        ])),
        vertical[0],
    );
    let wide = area.width >= 96;
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if wide {
            vec![Constraint::Min(40), Constraint::Length(30)]
        } else {
            vec![Constraint::Min(1)]
        })
        .split(vertical[1]);
    let notebook_area = columns[0];
    let visible_lines = notebook_area.height.saturating_sub(2) as usize;
    let maximum_scroll = workspace.transcript.len().saturating_sub(visible_lines);
    if workspace.follow_output {
        workspace.scroll = maximum_scroll;
    } else {
        workspace.scroll = workspace.scroll.min(maximum_scroll);
    }
    let title = if workspace.horizontal_scroll > 0 {
        format!(" Notebook · column {} ", workspace.horizontal_scroll + 1)
    } else {
        " Notebook ".into()
    };
    frame.render_widget(
        Paragraph::new(Text::from(
            workspace.transcript[workspace.scroll
                ..(workspace.scroll + visible_lines).min(workspace.transcript.len())]
                .to_vec(),
        ))
        .block(panel(title))
        .scroll((0, workspace.horizontal_scroll)),
        notebook_area,
    );
    if wide {
        frame.render_widget(
            Paragraph::new(variable_lines(workspace)).block(panel(" Variables · Tab ")),
            columns[1],
        );
    }
    draw_editor(frame, workspace, vertical[2]);
    let footer = if area.width >= 110 {
        format!(
            " {}  |  Enter evaluate · Alt+Enter newline · ↑↓ history · PgUp/PgDn scroll · F1 help · Ctrl+Q quit",
            workspace.status
        )
    } else if area.width >= 65 {
        " Enter evaluate · ↑↓ history · Tab vars · F1 help · Ctrl+Q quit".into()
    } else {
        " Enter run · F1 help · Ctrl+Q quit".into()
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(MUTED)),
        vertical[3],
    );
    if workspace.help {
        draw_help(frame, area, workspace);
    } else if workspace.variables {
        let popup = inset(area, 3, 2);
        let lines = variable_lines(workspace);
        workspace.variable_max_scroll = lines
            .len()
            .saturating_sub(popup.height.saturating_sub(2) as usize);
        workspace.variable_scroll = workspace.variable_scroll.min(workspace.variable_max_scroll);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().bg(BACKGROUND).fg(INK))
                .block(panel(" Variables · ↑↓ scroll · Esc close "))
                .scroll((workspace.variable_scroll.min(u16::MAX as usize) as u16, 0)),
            popup,
        );
    }
}

fn draw_editor(frame: &mut Frame<'_>, workspace: &Workspace, area: Rect) {
    let block = panel(" Expression ").border_style(Style::default().fg(ACCENT));
    let inner = block.inner(area);
    let (row, column) = workspace.editor.coordinates();
    let row_offset = row.saturating_sub(inner.height.saturating_sub(1) as usize);
    let column_offset = column.saturating_sub(inner.width.saturating_sub(1) as usize);
    let text = if workspace.editor.text.is_empty() {
        "A = [1 2; 3 4]"
    } else {
        &workspace.editor.text
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(if workspace.editor.text.is_empty() {
                MUTED
            } else {
                INK
            }))
            .block(block)
            .scroll((
                row_offset.min(u16::MAX as usize) as u16,
                column_offset.min(u16::MAX as usize) as u16,
            )),
        area,
    );
    if inner.width > 0 && inner.height > 0 && !workspace.help && !workspace.variables {
        frame.set_cursor_position((
            inner.x + (column - column_offset) as u16,
            inner.y + (row - row_offset) as u16,
        ));
    }
}

fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect::new(
        area.x + horizontal,
        area.y + vertical,
        area.width.saturating_sub(2 * horizontal),
        area.height.saturating_sub(2 * vertical),
    )
}

fn wrapped_help_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let mut wrapped = Vec::new();
    for line in lines {
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let mut current = String::new();
        for word in text.split_whitespace() {
            if !current.is_empty() && current.width() + 1 + word.width() > width {
                wrapped.push(Line::styled(std::mem::take(&mut current), line.style));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        wrapped.push(Line::styled(current, line.style));
    }
    wrapped
}

fn draw_help(frame: &mut Frame<'_>, area: Rect, workspace: &mut Workspace) {
    let popup = inset(area, 2, 1);
    let lines = vec![
        Line::styled("Notation", Style::default().fg(ACCENT)),
        Line::from("A = [1 2; 3 4]   b = [5, 6]   1/3   A'"),
        Line::from("A + B   A * B   A^3   solve(A, b)"),
        Line::from("det inv rank rref transpose trace eye zeros augment"),
        Line::from(""),
        Line::styled("Workspace", Style::default().fg(ACCENT)),
        Line::from(":mode exact|float|symbolic   :precision 8"),
        Line::from(":steps on|off   :tolerance 1e-10   :vars   :clear"),
        Line::from("Mode changes clear variables; exact values stay exact."),
        Line::from(""),
        Line::styled("Keys", Style::default().fg(ACCENT)),
        Line::from("Enter evaluate    Alt+Enter / Ctrl+J insert newline"),
        Line::from("↑ / ↓ history     ← / →, Home / End edit"),
        Line::from("PgUp / PgDn scroll results   Alt+← / → horizontal"),
        Line::from("Ctrl+Home first result      Ctrl+End latest result"),
        Line::from("Tab variables     Ctrl+C clear input"),
        Line::from("Ctrl+L clear notebook       Ctrl+Q quit"),
        Line::from("Paste inserts text for review; Enter evaluates it."),
        Line::from(""),
        Line::styled(
            "↑↓ / PgUp/PgDn scroll · F1 / Esc close",
            Style::default().fg(ACCENT),
        ),
    ];
    let lines = wrapped_help_lines(lines, popup.width.saturating_sub(2) as usize);
    workspace.help_max_scroll = lines
        .len()
        .saturating_sub(popup.height.saturating_sub(2) as usize);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().bg(BACKGROUND).fg(INK))
        .block(panel(" Guide · ↑↓ scroll "));
    workspace.help_scroll = workspace.help_scroll.min(workspace.help_max_scroll);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        paragraph.scroll((workspace.help_scroll.min(u16::MAX as usize) as u16, 0)),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Mode;
    use ratatui::backend::TestBackend;

    #[test]
    fn editor_preserves_utf8_boundaries_and_multiline_cursor() {
        let mut editor = Editor::default();
        assert!(editor.insert("α + 矩阵\nA"));
        assert_eq!(editor.coordinates(), (1, 1));
        editor.home();
        editor.backspace();
        assert_eq!(editor.text, "α + 矩阵A");
        editor.left();
        editor.delete();
        assert_eq!(editor.text, "α + 矩A");
        editor.home();
        editor.right();
        editor.insert("β");
        assert_eq!(editor.text, "αβ + 矩A");
    }

    #[test]
    fn paste_normalizes_lines_and_filters_terminal_controls() {
        let mut editor = Editor::default();
        editor.insert("[1 2;\r\n3 4]\u{1b}\u{7}");
        assert_eq!(editor.text, "[1 2;\n3 4]");
        assert!(!editor.insert(&"x".repeat(MAX_EDITOR_BYTES)));
        assert_eq!(editor.text, "[1 2;\n3 4]");
    }

    #[test]
    fn history_restores_unsubmitted_draft_and_errors_keep_input() {
        let mut workspace = Workspace::new(Session::new(Mode::Exact), false);
        workspace.editor.insert("A = [1 2; 3 4]");
        workspace.submit();
        assert!(workspace.session.variables().contains_key("A"));
        assert!(workspace.session.variables().contains_key("ans"));
        workspace.editor.insert("det(");
        workspace.history_up();
        assert_eq!(workspace.editor.text, "A = [1 2; 3 4]");
        workspace.history_down();
        assert_eq!(workspace.editor.text, "det(");
        workspace.submit();
        assert_eq!(workspace.editor.text, "det(");
        assert!(workspace.session.variables().contains_key("A"));
        assert!(workspace.session.variables().contains_key("ans"));
    }

    #[test]
    fn rendering_handles_wide_narrow_and_tiny_terminals() {
        for (width, height) in [
            (120, 32),
            (100, 30),
            (65, 20),
            (45, 16),
            (24, 9),
            (10, 4),
            (1, 1),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut workspace = Workspace::new(Session::new(Mode::Exact), false);
            workspace.editor.insert("A = [1 2; 3 4]");
            workspace.submit();
            terminal.draw(|frame| draw(frame, &mut workspace)).unwrap();
            let buffer = terminal.backend().buffer();
            let content = buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            if let Ok(directory) = std::env::var("LA_TUI_SNAPSHOT_DIR") {
                std::fs::create_dir_all(&directory).unwrap();
                let lines = buffer
                    .content()
                    .chunks(width as usize)
                    .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n");
                std::fs::write(format!("{directory}/{width}x{height}.txt"), lines).unwrap();
            }
            if width >= 24 && height >= 9 {
                assert!(content.contains("LINEAR ALGEBRA"));
                assert!(content.contains("Expression"));
            } else {
                assert!(content.contains("E"));
            }
            workspace.help = true;
            terminal.draw(|frame| draw(frame, &mut workspace)).unwrap();
            workspace.help = false;
            workspace.variables = true;
            terminal.draw(|frame| draw(frame, &mut workspace)).unwrap();
        }
    }
}
