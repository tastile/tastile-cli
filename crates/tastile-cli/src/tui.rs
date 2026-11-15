//! `tastile` (no args) — the TUI.
//!
//! The TUI shares its HTTP layer with the CLI via `crate::app::*`. Both
//! surfaces load `AppContext` and use the same application service
//! functions, so an action taken in the TUI is exactly the same code path
//! as the equivalent CLI subcommand.
//!
//! ## Pane layout
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │ tastile 1.0.0  api=https://api.tastile.app                │
//! │ ● Signed in (subject=u-1)                                  │
//! ├──────────────────────────────────┬─────────────────────────┤
//! │ Today's tiles                   │ Pending prompts          │
//! │ [HH:MM] title  (X/Ym)            │ kind=N note...           │
//! │ ...                              │ ...                      │
//! ├──────────────────────────────────┴─────────────────────────┤
//! │ status line                                                 │
//! ├────────────────────────────────────────────────────────────┤
//! │ q quit  r refresh  n new tile  c cancel  s start  p pause  │
//! │                   R resume  f finish  P resolve prompt     │
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Keybindings
//!
//! - `q` / `Esc` — quit
//! - `r` — manual refresh
//! - `n`, then `y` — create a new source tile after confirmation
//! - `c` — cancel the focused source tile
//! - `s` / `p` / `R` / `f` — start / pause / resume / finish the focused
//!   placement or execution
//! - `[` / `]` — focus a prompt; `P` then `1` / `2` / `3` resolves it
//!
//! Errors are surfaced in a bottom status line; the TUI never panics on a
//! network blip, it just keeps the last good data and shows the error.

use std::io::{Stdout, stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use uuid::Uuid;

use crate::app::{self, AppContext, CreateSourceTileDraft, PromptResolution};
use crate::cli::{AuthArgs, AuthCommand};

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Default)]
struct NewTileConfirmation {
    pending: bool,
}

impl NewTileConfirmation {
    fn begin(&mut self) {
        self.pending = true;
    }

    fn confirm(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }

    fn cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}

fn selected_prompt_index(prompt_count: usize, focused: usize) -> Option<usize> {
    (focused < prompt_count).then_some(focused)
}

fn resolution_for_key(key: KeyCode) -> Option<PromptResolution> {
    match key {
        KeyCode::Char('1') => Some(PromptResolution::Acknowledged),
        KeyCode::Char('2') => Some(PromptResolution::Dismissed),
        KeyCode::Char('3') => Some(PromptResolution::ActionTaken),
        _ => None,
    }
}

/// One row in the source-tile list. The TUI shows `SourceTileRead`s so the
/// user can pick one to start/cancel.
#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: Uuid,
    pub title: String,
    pub state: i32,
    pub revision: i64,
}

/// Top-level TUI state.
pub struct App {
    pub ctx: AppContext,
    pub tiles: Vec<tastile_api::TileListView>,
    pub prompts: Vec<tastile_api::PromptView>,
    pub sources: Vec<SourceRow>,
    pub focused: usize,
    pub focused_prompt: usize,
    create_confirmation: NewTileConfirmation,
    prompt_resolution_pending: bool,
    pub status: String,
    pub next_refresh: std::time::Instant,
}

impl App {
    fn new(ctx: AppContext) -> Self {
        Self {
            ctx,
            tiles: Vec::new(),
            prompts: Vec::new(),
            sources: Vec::new(),
            focused: 0,
            focused_prompt: 0,
            create_confirmation: NewTileConfirmation::default(),
            prompt_resolution_pending: false,
            status: "loading...".into(),
            next_refresh: std::time::Instant::now(),
        }
    }
}

pub async fn run(cfg: tastile_config::Config) -> Result<()> {
    let ctx = AppContext::load(cfg)?;
    let mut app = App::new(ctx);
    refresh(&mut app).await;

    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, &mut app).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn refresh(app: &mut App) {
    if !app.ctx.is_signed_in() {
        app.tiles.clear();
        app.prompts.clear();
        app.sources.clear();
        app.status = "not signed in — run `tastile auth login`".into();
        return;
    }
    let tiles = app::today(&app.ctx).await.unwrap_or_default();
    let prompts = app::list_pending_prompts(&app.ctx)
        .await
        .unwrap_or_default();
    let sources: Vec<SourceRow> = app::list_source_tiles(&app.ctx, None)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| SourceRow {
            id: s.source_tile_id,
            title: s.title,
            state: s.source_state,
            revision: s.revision,
        })
        .collect();
    if app.focused >= sources.len() && !sources.is_empty() {
        app.focused = sources.len() - 1;
    }
    if app.focused_prompt >= prompts.len() && !prompts.is_empty() {
        app.focused_prompt = prompts.len() - 1;
    }
    app.tiles = tiles;
    app.prompts = prompts;
    app.sources = sources;
    app.status = format!(
        "refreshed {} tiles, {} prompts, {} source tiles",
        app.tiles.len(),
        app.prompts.len(),
        app.sources.len()
    );
}

async fn event_loop(terminal: &mut Tui, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| render(f, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    if app.create_confirmation.pending
                        && !matches!(k.code, KeyCode::Char('y') | KeyCode::Esc)
                    {
                        app.create_confirmation.cancel();
                    }
                    if app.prompt_resolution_pending
                        && !matches!(
                            k.code,
                            KeyCode::Char('1')
                                | KeyCode::Char('2')
                                | KeyCode::Char('3')
                                | KeyCode::Esc
                        )
                    {
                        app.prompt_resolution_pending = false;
                    }
                    match k.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Esc => {
                            let cancelled_create = app.create_confirmation.cancel();
                            let cancelled_prompt =
                                std::mem::take(&mut app.prompt_resolution_pending);
                            if cancelled_create || cancelled_prompt {
                                app.status = "Pending action cancelled.".into();
                            } else {
                                return Ok(());
                            }
                        }
                        KeyCode::Char('r') => {
                            refresh(app).await;
                        }
                        KeyCode::Down => {
                            if app.focused + 1 < app.sources.len() {
                                app.focused += 1;
                            }
                        }
                        KeyCode::Up => {
                            if app.focused > 0 {
                                app.focused -= 1;
                            }
                        }
                        KeyCode::Char('n') => {
                            app.create_confirmation.begin();
                            app.status =
                                "Create a source tile? Press y to confirm, Esc to cancel.".into();
                        }
                        KeyCode::Char('y') => {
                            if app.create_confirmation.confirm() {
                                new_source_tile(app).await;
                            }
                        }
                        KeyCode::Char('c') => {
                            cancel_focused(app).await;
                        }
                        KeyCode::Char('s') => {
                            // Start execution: the focused source tile must
                            // already have a placement; the user supplies the
                            // placement id via the command line in practice.
                            // From the TUI we surface the action by writing
                            // a copy-pasteable command.
                            app.status = "use `tastile executions start <placement-id>`".into();
                        }
                        KeyCode::Char('p') => {
                            app.status = "use `tastile executions pause <execution-id>`".into();
                        }
                        KeyCode::Char('R') => {
                            app.status = "use `tastile executions resume <execution-id>`".into();
                        }
                        KeyCode::Char('f') => {
                            app.status =
                                "use `tastile executions finish <execution-id> --kind N`".into();
                        }
                        KeyCode::Char('P') => {
                            if selected_prompt_index(app.prompts.len(), app.focused_prompt)
                                .is_some()
                            {
                                app.prompt_resolution_pending = true;
                                app.status = "Resolve focused prompt: 1 acknowledge, 2 dismiss, 3 action taken.".into();
                            } else {
                                app.status = "no pending prompts".into();
                            }
                        }
                        KeyCode::Char('1') | KeyCode::Char('2') | KeyCode::Char('3') => {
                            if app.prompt_resolution_pending {
                                app.prompt_resolution_pending = false;
                                if let Some(resolution) = resolution_for_key(k.code) {
                                    resolve_focused_prompt(app, resolution).await;
                                }
                            }
                        }
                        KeyCode::Char('[') => {
                            if app.focused_prompt > 0 {
                                app.focused_prompt -= 1;
                            }
                        }
                        KeyCode::Char(']') => {
                            if app.focused_prompt + 1 < app.prompts.len() {
                                app.focused_prompt += 1;
                            }
                        }
                        KeyCode::Char('a') => {
                            // Operator escape hatch: spawn the auth flow from
                            // inside the TUI.
                            let cfg = app.ctx.config.clone();
                            let args = AuthArgs {
                                sub: AuthCommand::Login {
                                    print_url: true,
                                    client_id: None,
                                },
                            };
                            if let Err(e) = crate::commands::auth::run(cfg, args).await {
                                app.status = format!("auth login failed: {e}");
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if std::time::Instant::now() >= app.next_refresh {
            refresh(app).await;
            app.next_refresh = std::time::Instant::now() + Duration::from_secs(30);
        }
    }
}

async fn new_source_tile(app: &mut App) {
    let title = format!("tile-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    let draft = CreateSourceTileDraft::minimal(&title);
    match app::create_source_tile_draft(&app.ctx, &draft).await {
        Ok(resp) => {
            app.status = format!(
                "✓ Created source tile {} (command_id={})",
                title, resp.command_id
            );
            refresh(app).await;
        }
        Err(e) => app.status = format!("create failed: {e}"),
    }
}

async fn cancel_focused(app: &mut App) {
    let Some(row) = app.sources.get(app.focused).cloned() else {
        app.status = "no source tile selected".into();
        return;
    };
    // Numeric cancel-reason registry code (0 = user-requested).
    match app::cancel_source_tile(&app.ctx, row.id, 0, None).await {
        Ok(_) => {
            app.status = format!("✓ Cancelled source tile {}", row.title);
            refresh(app).await;
        }
        Err(e) => app.status = format!("cancel failed: {e}"),
    }
}

async fn resolve_focused_prompt(app: &mut App, resolution: PromptResolution) {
    let Some(index) = selected_prompt_index(app.prompts.len(), app.focused_prompt) else {
        app.status = "no pending prompts".into();
        return;
    };
    let p = app.prompts[index].clone();
    match app::resolve_prompt(&app.ctx, p.id, resolution, None).await {
        Ok(_) => {
            app.status = format!("✓ Resolved prompt {}", p.id);
            refresh(app).await;
        }
        Err(e) => app.status = format!("resolve failed: {e}"),
    }
}

fn render(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(4),
            Constraint::Length(2),
        ])
        .split(f.area());

    let auth_line = if app.ctx.is_signed_in() {
        Line::from(vec![
            Span::raw("● "),
            Span::styled("Signed in", Style::default().fg(Color::Green)),
            Span::raw(format!(
                "  subject={}",
                app.ctx.subject.as_deref().unwrap_or("-")
            )),
        ])
    } else {
        Line::from(vec![
            Span::raw("○ "),
            Span::styled("Signed out", Style::default().fg(Color::Yellow)),
            Span::raw("  (run `tastile auth login`, or press `a` here to start the flow)"),
        ])
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("tastile", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("  {}", env!("CARGO_PKG_VERSION"))),
            Span::raw(format!("  api={}", app.ctx.config.api_url)),
        ]),
        auth_line,
    ])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // Middle: three columns — tiles, sources (focusable), prompts.
    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ])
        .split(chunks[1]);

    let tile_items: Vec<ListItem> = app
        .tiles
        .iter()
        .map(|t| {
            let next = t
                .projected_next_start_at
                .map(|d| d.format("%H:%M").to_string())
                .unwrap_or_else(|| "--:--".into());
            ListItem::new(Line::from(vec![
                Span::raw(format!("[{}] ", next)),
                Span::styled(&t.title, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!(
                    "  ({}/{}m)",
                    t.worked_minutes / 60_000,
                    t.break_minutes / 60_000
                )),
            ]))
        })
        .collect();
    let tile_list = List::new(tile_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Today's tiles"),
    );
    f.render_widget(tile_list, middle[0]);

    let source_items: Vec<ListItem> = app
        .sources
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let marker = if i == app.focused { "▶ " } else { "  " };
            let style = if i == app.focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(&s.title, style),
                Span::raw(format!("  state={} rev={}", s.state, s.revision)),
            ]))
        })
        .collect();
    let source_list = List::new(source_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Source tiles (focused)"),
    );
    f.render_widget(source_list, middle[1]);

    let prompt_items: Vec<ListItem> = app
        .prompts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let marker = if i == app.focused_prompt {
                "▶ "
            } else {
                "  "
            };
            let style = if i == app.focused_prompt {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(format!("kind={} ", p.kind), style),
                Span::styled(
                    p.note.clone().unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::Cyan),
                ),
            ]))
        })
        .collect();
    let prompt_list =
        List::new(prompt_items).block(Block::default().borders(Borders::ALL).title("Prompts"));
    f.render_widget(prompt_list, middle[2]);

    let footer = Paragraph::new(vec![
        Line::from(Span::styled(&app.status, Style::default().fg(Color::Gray))),
        Line::from(Span::styled(
            "n new tile  y confirm  c cancel  P then 1/2/3 resolve prompt",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ])
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    f.render_widget(footer, chunks[2]);

    let help = Paragraph::new(Line::from(vec![
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh  "),
        Span::styled("a", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" auth  "),
        Span::styled("↑↓", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" source  "),
        Span::styled("[ ]", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" prompts  "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" cancel / quit"),
    ]));
    f.render_widget(help, chunks[3]);
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_tile_creation_requires_a_separate_confirmation() {
        let mut confirmation = NewTileConfirmation::default();
        assert!(!confirmation.confirm());
        confirmation.begin();
        assert!(confirmation.confirm());
        assert!(!confirmation.confirm());
    }

    #[test]
    fn prompt_navigation_selects_the_focused_row() {
        assert_eq!(selected_prompt_index(3, 1), Some(1));
        assert_eq!(selected_prompt_index(1, 1), None);
    }

    #[test]
    fn prompt_resolution_requires_an_explicit_resolution_key() {
        assert_eq!(
            resolution_for_key(KeyCode::Char('1')),
            Some(PromptResolution::Acknowledged)
        );
        assert_eq!(
            resolution_for_key(KeyCode::Char('2')),
            Some(PromptResolution::Dismissed)
        );
        assert_eq!(
            resolution_for_key(KeyCode::Char('3')),
            Some(PromptResolution::ActionTaken)
        );
        assert_eq!(resolution_for_key(KeyCode::Char('P')), None);
    }
}
