use crate::analyzer;
use crate::context;
use crate::diagnostics::ParsedError;
use crate::explain;
use crate::fixer;
use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use std::io;
use std::time::Duration;

pub struct App {
    errors: Vec<ParsedError>,
    selected: usize,
    list_state: ListState,
    scroll: u16,
    prepared: Vec<Vec<Line<'static>>>,
}

impl App {
    fn new(errors: Vec<ParsedError>, project_dir: &str) -> Self {
        let prepared = errors
            .iter()
            .map(|error| prepare_error(error, project_dir))
            .collect();

        let list_state = ListState::default();

        Self {
            errors,
            selected: 0,
            list_state,
            scroll: 0,
            prepared,
        }
    }

    fn select(&mut self, index: usize) {
        if index < self.errors.len() {
            self.selected = index;
            self.scroll = 0;
            self.list_state.select(Some(index));
        }
    }
}

fn style_code(code: &str) -> Color {
    match code {
        "E0308" | "E0373" | "E0599" | "E0596" => Color::Yellow,
        "E0382" | "E0505" | "E0503" | "E0506" => Color::Red,
        "E0499" | "E0502" | "E0500" => Color::Magenta,
        "E0597" | "E0106" | "E0515" | "E0521" | "E0716" => Color::Cyan,
        "E0277" | "E0282" => Color::Blue,
        "E0432" | "E0433" => Color::LightBlue,
        _ => Color::White,
    }
}

fn prepare_error(error: &ParsedError, project_dir: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            format!(" ERROR {} ", error.code),
            Style::default()
                .fg(Color::Black)
                .bg(style_code(&error.code)),
        ),
        Span::styled(
            format!("  {}", error.raw_message),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(Line::from(""));

    lines.push(Line::from(vec![Span::styled(
        "Compiler evidence",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));

    let analysis = analyzer::analyze(error);

    if analysis.locations.is_empty() {
        lines.push(Line::from("  (no locations reported)"));
    }

    for location in &analysis.locations {
        lines.push(Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Red)),
            Span::raw(format!(
                "{}:{}:{}",
                location.file, location.line, location.column
            )),
        ]));

        if !location.snippet.is_empty() {
            lines.push(Line::from(format!("    {}", location.snippet)));
        }

        if let Some(label) = &location.label {
            lines.push(Line::from(Span::styled(
                format!("    └─ {}", label),
                Style::default().fg(Color::Cyan),
            )));
        }
    }

    let contexts = context::SourceContext::from_error(error, project_dir);

    if !contexts.is_empty() {
        lines.push(Line::from(""));

        lines.push(Line::from(vec![Span::styled(
            "Source context",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));

        for source_context in &contexts {
            for line in &source_context.lines {
                let marker = if line.highlighted { "►" } else { " " };

                let gutter = format!("{:>4}", line.line_number);

                let display = if line.highlighted {
                    Line::from(vec![
                        Span::styled(marker, Style::default().fg(Color::Red)),
                        Span::styled(
                            format!(" {} ", gutter),
                            Style::default().fg(Color::Black).bg(Color::Red),
                        ),
                        Span::styled(
                            format!(" │ {}", line.text),
                            Style::default().fg(Color::White),
                        ),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(marker),
                        Span::styled(
                            format!(" {} ", gutter),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(
                            format!(" │ {}", line.text),
                            Style::default().fg(Color::Gray),
                        ),
                    ])
                };

                lines.push(display);

                if line.highlighted
                    && let Some(label) = &line.label
                {
                    lines.push(Line::from(vec![
                        Span::styled("          │ ", Style::default().fg(Color::Red)),
                        Span::styled(format!("^ {}", label), Style::default().fg(Color::Red)),
                    ]));
                }
            }
        }
    }

    let explanation = explain::explain(error, &analysis);

    lines.push(Line::from(""));

    lines.push(Line::from(vec![Span::styled(
        "Explanation",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));

    lines.push(Line::from(vec![Span::styled(
        explanation.title,
        Style::default().add_modifier(Modifier::BOLD),
    )]));

    if let Some(concept) = &explanation.concept {
        lines.push(Line::from(vec![
            Span::styled("Concept: ", Style::default().fg(Color::Cyan)),
            Span::styled(concept.clone(), Style::default().fg(Color::Cyan)),
        ]));
    }

    if let Some(principle) = &explanation.principle {
        lines.push(Line::from(vec![
            Span::styled("The rule: ", Style::default().fg(Color::Cyan)),
            Span::styled(principle.clone(), Style::default().fg(Color::White)),
        ]));
    }

    lines.push(Line::from(explanation.plain_summary));

    if !explanation.fix_options.is_empty() {
        lines.push(Line::from(""));

        lines.push(Line::from(vec![Span::styled(
            "Possible fixes",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )]));

        for option in &explanation.fix_options {
            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Green)),
                Span::styled(option.clone(), Style::default().fg(Color::Green)),
            ]));
        }
    }

    let fix = fixer::suggest_fix(error);

    lines.push(Line::from(""));

    lines.push(Line::from(vec![Span::styled(
        "Fix classification",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )]));

    match fix.kind {
        fixer::FixKind::CompilerSuggested => {
            lines.push(Line::from(vec![
                Span::styled("✔ ", Style::default().fg(Color::Green)),
                Span::styled(fix.description, Style::default().fg(Color::Green)),
            ]));

            if let Some(suggestion) = &fix.suggestion {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  {}:{}:{} → {}",
                        suggestion.file, suggestion.line, suggestion.column, suggestion.replacement
                    ),
                    Style::default().fg(Color::Green),
                )));
            }
        }

        fixer::FixKind::RequiresHumanJudgment => {
            lines.push(Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(Color::Yellow)),
                Span::styled(fix.description, Style::default().fg(Color::Yellow)),
            ]));
        }
    }

    lines
}

pub fn run(errors: &[ParsedError], project_dir: &str) -> Result<()> {
    if errors.is_empty() {
        println!("✔ No compiler errors found.");
        return Ok(());
    }

    let mut terminal = ratatui::init();

    terminal.clear()?;
    terminal.show_cursor()?;

    let result = event_loop(&mut terminal, errors, project_dir);

    ratatui::restore();

    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    errors: &[ParsedError],
    project_dir: &str,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};

    let mut app = App::new(errors.to_vec(), project_dir);

    app.select(0);

    loop {
        terminal.draw(|frame| {
            ui(frame, &mut app);
        })?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    break;
                }

                KeyCode::Down | KeyCode::Char('j') => {
                    app.select(app.selected.saturating_add(1));
                }

                KeyCode::Up | KeyCode::Char('k') => {
                    app.select(app.selected.saturating_sub(1));
                }

                KeyCode::PageDown | KeyCode::Char(' ') => {
                    app.scroll = app.scroll.saturating_add(10);
                }

                KeyCode::PageUp => {
                    app.scroll = app.scroll.saturating_sub(10);
                }

                _ => {}
            }
        }
    }

    Ok(())
}

fn ui(frame: &mut ratatui::Frame, app: &mut App) {
    let outer = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(outer);

    let list_items: Vec<ListItem> = app
        .errors
        .iter()
        .enumerate()
        .map(|(index, error)| {
            let selected = index == app.selected;

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", error.code),
                    Style::default()
                        .fg(Color::Black)
                        .bg(style_code(&error.code)),
                ),
                Span::styled(
                    format!("  {}:{}", error.raw_message, error.spans.len()),
                    if selected {
                        Style::default().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
            ]))
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Errors ")
                .title_style(
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, chunks[0], &mut app.list_state);

    let panel = Paragraph::new(
        app.prepared[app.selected]
            .iter()
            .skip(app.scroll as usize)
            .cloned()
            .collect::<Vec<_>>(),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Explained ")
            .title_style(
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .wrap(Wrap { trim: false });

    frame.render_widget(panel, chunks[1]);
}
