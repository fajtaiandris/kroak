mod codex;
mod ui;

use std::{
    env,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reasoning {
    Duh,
    Ooh,
}

impl Reasoning {
    fn effort(self) -> &'static str {
        match self {
            Self::Duh => "low",
            Self::Ooh => "high",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Dont,
    Look,
}

struct App {
    input: String,
    question: String,
    answer: String,
    history: Vec<Exchange>,
    failed: bool,
    reasoning: Reasoning,
    look: Look,
    working: bool,
    scroll: u16,
    tick: usize,
    launch_dir: PathBuf,
    sender: mpsc::Sender<Result<String>>,
}

#[derive(Clone)]
struct Exchange {
    question: String,
    answer: String,
}

struct AskRequest {
    question: String,
    previous: Option<Exchange>,
}

fn followup_text(input: &str) -> Option<&str> {
    input
        .strip_prefix("/but")
        .filter(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
        .map(str::trim)
}

impl App {
    fn new(launch_dir: PathBuf, sender: mpsc::Sender<Result<String>>) -> Self {
        Self {
            input: String::new(),
            question: String::new(),
            answer: String::new(),
            history: Vec::new(),
            failed: false,
            reasoning: Reasoning::Duh,
            look: Look::Dont,
            working: false,
            scroll: 0,
            tick: 0,
            launch_dir,
            sender,
        }
    }

    fn submit(&mut self) {
        let Some(request) = self.begin_question() else {
            return;
        };
        let reasoning = self.reasoning;
        let look = self.look;
        let launch_dir = self.launch_dir.clone();
        let sender = self.sender.clone();
        thread::spawn(move || {
            let previous = request
                .previous
                .as_ref()
                .map(|exchange| (exchange.question.as_str(), exchange.answer.as_str()));
            let _ = sender.send(codex::ask(
                &request.question,
                previous,
                reasoning,
                look,
                &launch_dir,
            ));
        });
    }

    fn begin_question(&mut self) -> Option<AskRequest> {
        if self.working || self.input.trim().is_empty() {
            return None;
        }
        let input = self.input.trim();
        let (question, previous) = if let Some(followup) = followup_text(input) {
            if followup.is_empty() {
                return None;
            }
            if self.answer.is_empty() || self.failed {
                self.question = followup.to_owned();
                self.answer = "Ask a question first before using /but.".to_owned();
                self.failed = true;
                self.input.clear();
                return None;
            }
            let previous = Exchange {
                question: self.question.clone(),
                answer: self.answer.clone(),
            };
            self.history.push(previous.clone());
            (followup.to_owned(), Some(previous))
        } else {
            self.history.clear();
            (input.to_owned(), None)
        };
        self.question = question.clone();
        self.answer.clear();
        self.failed = false;
        self.scroll = 0;
        self.working = true;
        self.tick = 0;
        Some(AskRequest { question, previous })
    }

    fn receive(&mut self, result: Result<String>) {
        self.working = false;
        self.input.clear();
        match result {
            Ok(answer) => {
                self.answer = answer;
                self.failed = false;
            }
            Err(error) => {
                self.answer = format!("Could not answer: {error:#}");
                self.failed = true;
            }
        }
    }

    fn toggle_reasoning(&mut self) {
        self.reasoning = if self.reasoning == Reasoning::Duh {
            Reasoning::Ooh
        } else {
            Reasoning::Duh
        };
    }

    fn toggle_look(&mut self) {
        self.look = if self.look == Look::Dont {
            Look::Look
        } else {
            Look::Dont
        };
    }

    fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return true;
        }
        if key.code == KeyCode::Esc
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
        {
            // Keep the child process attached until it finishes.
            if self.working {
                return true;
            }
            return false;
        }
        if self.working {
            return true;
        }
        match key.code {
            KeyCode::F(1) => self.toggle_reasoning(),
            KeyCode::F(2) => self.toggle_look(),
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear()
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.input.push(c)
            }
            KeyCode::Up | KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(3),
            KeyCode::Down | KeyCode::PageDown => self.scroll = self.scroll.saturating_add(3),
            _ => {}
        }
        true
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll = self.scroll.saturating_add(3),
            MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
            MouseEventKind::Down(MouseButton::Left) if !self.working => {
                let x = mouse.column;
                let y = mouse.row;
                if y == ui::HEADER_Y {
                    if (ui::REASONING_X..ui::REASONING_X + ui::REASONING_WIDTH).contains(&x) {
                        self.toggle_reasoning();
                    } else if (ui::LOOK_X..ui::LOOK_X + ui::look_width(self.look)).contains(&x) {
                        self.toggle_look();
                    }
                }
            }
            _ => {}
        }
    }
}

struct ScreenGuard;
impl ScreenGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        Ok(Self)
    }
}
impl Drop for ScreenGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn run() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(anyhow!("kroak needs an interactive terminal"));
    }
    let launch_dir = env::current_dir()?;
    let (sender, receiver) = mpsc::channel();
    let mut app = App::new(launch_dir, sender);
    let _guard = ScreenGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let highlighter = ui::Highlighter::new();
    let mut last_tick = Instant::now();
    loop {
        while let Ok(result) = receiver.try_recv() {
            app.receive(result);
        }
        terminal.draw(|frame| ui::draw(frame, &app, &highlighter))?;
        if event::poll(Duration::from_millis(80))? {
            match event::read()? {
                Event::Key(key) if !app.key(key) => break,
                Event::Mouse(mouse) => app.mouse(mouse),
                _ => {}
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(240) {
            app.tick = app.tick.wrapping_add(1);
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    match env::args().nth(1).as_deref() {
        Some("--help" | "-h") => {
            println!("kroak — a tiny Codex question helper\n\nRun kroak in a terminal. Codex CLI must be installed and signed in.\nPrefix a question with /but to follow up on the previous answer.\nF1: duh/ooh  F2: don't look/look  Enter: ask  Esc: quit");
            Ok(())
        }
        Some("--version" | "-V") => {
            println!("kroak {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(arg) => Err(anyhow!("unknown argument: {arg}")),
        None => run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEventKind};
    use ratatui::backend::TestBackend;

    #[test]
    fn modes_start_private_and_support_clicks_and_keys() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        assert_eq!(app.reasoning, Reasoning::Duh);
        assert_eq!(app.look, Look::Dont);
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: ui::REASONING_X + 2,
            row: ui::HEADER_Y,
            modifiers: KeyModifiers::NONE,
        });
        app.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: ui::LOOK_X + 2,
            row: ui::HEADER_Y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.reasoning, Reasoning::Ooh);
        assert_eq!(app.look, Look::Look);
        app.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        assert_eq!(app.look, Look::Dont);
        app.key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
        assert_eq!(app.reasoning, Reasoning::Duh);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = ui::Highlighter::new();
        terminal
            .draw(|frame| ui::draw(frame, &app, &highlighter))
            .unwrap();
        let line: String = (0..80)
            .map(|x| terminal.backend().buffer()[(x, 1)].symbol())
            .collect();
        assert!(line.contains("kroak"));
        assert!(line.contains("█ [duh] [don't look]"));
        assert!(line.contains("duh"));
        assert!(!line.contains("ooh"));
        assert!(line.contains("don't look"));
        assert!(!line.contains("  look  "));
        let screen: String = (0..24)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .map(|(x, y)| terminal.backend().buffer()[(x, y)].symbol().to_owned())
            .collect();
        assert!(screen.contains("Type for your toad..."));
        assert!(!screen.contains("YOUR QUESTION"));
        assert!(!screen.contains("ANSWER"));
        assert!(!screen.contains("clear"));
        assert_eq!(
            terminal.backend().buffer()[(ui::REASONING_X, 1)].bg,
            ratatui::style::Color::Reset
        );
    }

    #[test]
    fn question_stays_in_input_until_answer_then_renders_as_qa() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.input = "What is 4+3?".into();
        assert_eq!(app.begin_question().unwrap().question, "What is 4+3?");
        assert_eq!(app.input, "What is 4+3?");
        assert!(app.working);
        app.receive(Ok("Seven.".into()));
        assert!(app.input.is_empty());
        assert!(!app.working);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = ui::Highlighter::new();
        terminal
            .draw(|frame| ui::draw(frame, &app, &highlighter))
            .unwrap();
        let row = |y| -> String {
            (0..80)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_owned())
                .collect()
        };
        assert!(row(4).trim().is_empty());
        assert!(row(5).contains("Q: What is 4+3?"));
        assert!(row(6).contains("A: Seven."));
    }

    #[test]
    fn but_keeps_prior_answers_and_sends_only_latest_pair() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.input = "What is it?".into();
        let first = app.begin_question().unwrap();
        assert!(first.previous.is_none());
        app.receive(Ok("A frog.".into()));

        app.input = "/but what color?".into();
        let second = app.begin_question().unwrap();
        assert_eq!(second.question, "what color?");
        let previous = second.previous.unwrap();
        assert_eq!(previous.question, "What is it?");
        assert_eq!(previous.answer, "A frog.");
        assert_eq!(app.history.len(), 1);
        assert!(app.answer.is_empty());
        assert_eq!(app.input, "/but what color?");
        let mut loading_terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = ui::Highlighter::new();
        loading_terminal
            .draw(|frame| ui::draw(frame, &app, &highlighter))
            .unwrap();
        let loading_row = |y| -> String {
            (0..80)
                .map(|x| {
                    loading_terminal.backend().buffer()[(x, y)]
                        .symbol()
                        .to_owned()
                })
                .collect()
        };
        assert!(loading_row(3).contains("/but what color?"));
        assert!(loading_row(5).contains("Q: What is it?"));
        assert!(loading_row(6).contains("A: A frog."));
        app.receive(Ok("Green.".into()));

        app.input = "/but why?".into();
        let third = app.begin_question().unwrap();
        let previous = third.previous.unwrap();
        assert_eq!(previous.question, "what color?");
        assert_eq!(previous.answer, "Green.");
        assert_eq!(app.history.len(), 2);
        app.receive(Ok("Camouflage.".into()));

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let highlighter = ui::Highlighter::new();
        terminal
            .draw(|frame| ui::draw(frame, &app, &highlighter))
            .unwrap();
        let row = |y| -> String {
            (0..80)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_owned())
                .collect()
        };
        assert!(row(5).contains("Q: What is it?"));
        assert!(row(6).contains("A: A frog."));
        assert!(row(7).contains("Q: what color?"));
        assert!(row(8).contains("A: Green."));
        assert!(row(9).contains("Q: why?"));
        assert!(row(10).contains("A: Camouflage."));

        app.input = "Where?".into();
        let fresh = app.begin_question().unwrap();
        assert!(fresh.previous.is_none());
        assert!(app.history.is_empty());
    }

    #[test]
    fn but_requires_a_previous_answer_and_a_question() {
        let (sender, _) = mpsc::channel();
        let mut app = App::new(PathBuf::from("/tmp/project"), sender);
        app.input = "/but".into();
        assert!(app.begin_question().is_none());
        app.input = "/but why?".into();
        assert!(app.begin_question().is_none());
        assert!(app.answer.contains("Ask a question first"));
        app.input = "/butter is good".into();
        assert_eq!(app.begin_question().unwrap().question, "/butter is good");
    }
}
