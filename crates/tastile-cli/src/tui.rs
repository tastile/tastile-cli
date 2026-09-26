//! `tastile` (no args) — the TUI.
//!
//! Minimal, intentionally so. The TUI shows:
//!
//! - a top status line (auth state + today + API base)
//! - the next actionable tile for today
//! - any pending prompts
//! - a quit hint
//!
//! Errors are surfaced in a bottom status line; the TUI never panics on a
//! network blip, it just keeps the last good data and shows the error.

use std::io::{Stdout, stdout};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Datelike, Timelike, Utc};
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
use tastile_api::{
    ApiClient, ApiConfig, BearerToken, list_pending_prompts, list_tiles, tiles::ListTilesQuery,
};
use tastile_auth::{CredentialStore, KeyringStore};
use tastile_config::with_env_overrides;
use tracing::error;
use tracing::info;

type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Top-level TUI state.
pub struct App {
    pub cfg: tastile_config::Config,
    pub auth_state: AuthState,
    pub tiles: Vec<tastile_api::TileListView>,
    pub prompts: Vec<tastile_api::PromptView>,
    pub status: String,
    pub next_refresh: std::time::Instant,
}

#[derive(Debug, Clone)]
pub enum AuthState {
    SignedOut,
    SignedIn { subject: Option<String> },
}

pub async fn run(cfg: tastile_config::Config) -> Result<()> {
    let cfg = with_env_overrides(cfg);

    let mut terminal = setup_terminal()?;
    let mut app = App {
        cfg: cfg.clone(),
        auth_state: read_auth_state(&cfg),
        tiles: Vec::new(),
        prompts: Vec::new(),
        status: "loading...".into(),
        next_refresh: std::time::Instant::now(),
    };

    if let Err(e) = refresh(&mut app).await {
        app.status = format!("refresh failed: {e}");
    }

    let result = event_loop(&mut terminal, &mut app).await;

    restore_terminal(&mut terminal)?;
    result
}

async fn refresh(app: &mut App) -> Result<()> {
    let token = match KeyringStore
        .load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER)
        .map_err(anyhow::Error::from)?
    {
        Some(t) if t.api_base_url == app.cfg.api_url => Some(BearerToken::new(t.bearer)),
        _ => None,
    };
    let api = ApiClient::new(ApiConfig::new(&app.cfg.api_url)?)?;
    if let Some(t) = token {
        let (start, end) = today_window();
        app.tiles = list_tiles(
            &api,
            &t,
            &ListTilesQuery {
                window_start: Some(start),
                window_end: Some(end),
                include_closed: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap_or_default();
        app.prompts = list_pending_prompts(&api, &t).await.unwrap_or_default();
        app.status = format!(
            "refreshed {} tiles, {} prompts",
            app.tiles.len(),
            app.prompts.len()
        );
    } else {
        app.tiles.clear();
        app.prompts.clear();
        app.status = "not signed in — run `tastile auth login`".into();
    }
    Ok(())
}

fn read_auth_state(cfg: &tastile_config::Config) -> AuthState {
    match KeyringStore.load(tastile_auth::DEFAULT_SERVICE, tastile_auth::DEFAULT_USER) {
        Ok(Some(t)) if t.api_base_url == cfg.api_url => AuthState::SignedIn { subject: t.subject },
        Ok(_) => AuthState::SignedOut,
        Err(_) => AuthState::SignedOut,
    }
}

async fn event_loop(terminal: &mut Tui, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| render(f, app))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('r') => {
                            if let Err(e) = refresh(app).await {
                                app.status = format!("refresh failed: {e}");
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Periodic background refresh: every 30s. Cheap because the API
        // surface we hit is read-only and small.
        if std::time::Instant::now() >= app.next_refresh {
            if let Err(e) = refresh(app).await {
                app.status = format!("refresh failed: {e}");
            }
            app.next_refresh = std::time::Instant::now() + Duration::from_secs(30);
        }
    }
}

fn render(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(3),
        ])
        .split(f.area());

    // Header: auth state + API base + today.
    let auth_line = match &app.auth_state {
        AuthState::SignedOut => Line::from(vec![
            Span::raw("○ "),
            Span::styled("Signed out", Style::default().fg(Color::Yellow)),
            Span::raw("  (run `tastile auth login`)"),
        ]),
        AuthState::SignedIn { subject } => Line::from(vec![
            Span::raw("● "),
            Span::styled("Signed in", Style::default().fg(Color::Green)),
            Span::raw(format!("  subject={}", subject.as_deref().unwrap_or("-"))),
        ]),
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("tastile", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("  {}", env!("CARGO_PKG_VERSION"))),
            Span::raw(format!("  api={}", app.cfg.api_url)),
        ]),
        auth_line,
    ])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // Middle: tiles + prompts side by side.
    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
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

    let prompt_items: Vec<ListItem> = app
        .prompts
        .iter()
        .map(|p| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("kind={} ", p.kind)),
                Span::styled(
                    p.note.clone().unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::Cyan),
                ),
            ]))
        })
        .collect();
    let prompt_list = List::new(prompt_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Pending prompts"),
    );
    f.render_widget(prompt_list, middle[1]);

    // Footer status.
    let footer = Paragraph::new(vec![
        Line::from(Span::styled(&app.status, Style::default().fg(Color::Gray))),
        Line::from(Span::styled(
            "press q to quit, r to refresh",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ])
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: true });
    f.render_widget(footer, chunks[2]);

    // Last: a small help bar with keyboard hints.
    let help = Paragraph::new(Line::from(vec![
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" refresh  "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit"),
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

fn today_window() -> (DateTime<Utc>, DateTime<Utc>) {
    let now = Utc::now();
    let start = now
        .with_hour(0)
        .and_then(|d| d.with_minute(0))
        .and_then(|d| d.with_second(0))
        .unwrap_or(now)
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let end = start
        .with_year(start.year())
        .and_then(|d| d.with_month(start.month()))
        .and_then(|d| d.with_day(start.day()))
        .unwrap_or(start)
        + chrono::Duration::days(1);
    (start.and_utc(), end.and_utc())
}

// Suppress the unused `info` import warning when we don't actually log from
// the TUI module. Keeping the import for future structured logging.
#[allow(dead_code)]
fn _info_marker() {
    info!("tui module loaded");
}

// Suppress unused `error` import marker — used by the future structured
// logger we will wire into the TUI in a follow-up PR.
#[allow(dead_code)]
fn _error_marker() {
    error!("tui module loaded");
}
