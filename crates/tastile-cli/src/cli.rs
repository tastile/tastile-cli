//! CLI surface definition (clap derive).
//!
//! The CLI exposes operations by user intent, not by raw API names:
//!
//! - `source-tiles` lists, creates, updates, cancels source tiles, and
//!   exposes derived views (completion, placements, reflow).
//! - `executions` starts/pauses/resumes/finishes an execution.
//! - `prompts` lists and resolves pending prompts.
//!
//! Internal operation IDs are surfaced by `tastile version`.

use clap::{Args, Parser, Subcommand, ValueHint};

#[derive(Debug, Parser)]
#[command(
    name = "tastile",
    version,
    about = "Tastile command-line client and TUI",
    long_about = "Tastile's official command-line client. Run with no subcommand to launch the TUI.",
    propagate_version = true
)]
pub struct Cli {
    /// Print verbose diagnostics to stderr.
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sign in, sign out, or check authentication state.
    Auth(AuthArgs),

    /// Show environment diagnostics (no secrets).
    Doctor,

    /// List tiles for the current day.
    Tiles(TilesArgs),

    /// Show the next actionable item.
    Today(TodayArgs),

    /// Schedule operations (regenerate, publish).
    Schedule(ScheduleArgs),

    /// Inspect, create, update, or cancel source tiles.
    SourceTiles(SourceTilesArgs),

    /// Start, pause, resume, or finish an execution.
    Executions(ExecutionsArgs),

    /// List or resolve pending prompts.
    Prompts(PromptsArgs),

    /// Generate a shell completion script.
    Completions(CompletionsArgs),

    /// Print version information.
    Version,
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub sub: AuthCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Open the browser to start the authorization flow.
    Login {
        /// Do not actually open a browser; print the auth URL instead.
        #[arg(long)]
        print_url: bool,
        /// Override the `client_id` sent to the web login page.
        #[arg(long)]
        client_id: Option<String>,
    },

    /// Show whether a bearer token is currently stored.
    Status,

    /// Forget the locally-stored bearer token.
    Logout,
}

#[derive(Debug, Args)]
pub struct TilesArgs {
    /// Output JSON instead of the human-readable table.
    #[arg(long)]
    pub json: bool,
    /// Restrict to a single plan.
    #[arg(long, value_name = "UUID")]
    pub plan: Option<uuid::Uuid>,
    /// Include closed/done tiles.
    #[arg(long)]
    pub include_closed: bool,
    /// Show only the next actionable tile (the one with the earliest
    /// `projected_next_start_at`).
    #[arg(long, conflicts_with = "include_closed")]
    pub next: bool,
}

#[derive(Debug, Args)]
pub struct TodayArgs {
    /// Output JSON instead of the human-readable table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ScheduleArgs {
    #[command(subcommand)]
    pub sub: ScheduleCommand,
}

#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Re-publish the schedule definition for the current period.
    Regenerate {
        /// Publish against the staging API instead of production.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Args)]
pub struct SourceTilesArgs {
    #[command(subcommand)]
    pub sub: SourceTilesCommand,
}

#[derive(Debug, Subcommand)]
pub enum SourceTilesCommand {
    /// List source tiles for the current owner.
    List {
        #[arg(long)]
        json: bool,
        #[arg(long, value_name = "UUID")]
        plan: Option<uuid::Uuid>,
    },
    /// Create a new source tile from a draft (interactive prompts or
    /// `--from-json` for non-interactive use).
    Create {
        /// Read the full draft as JSON from the given file path. Use `-` to
        /// read from stdin. See `docs/architecture.md` for the schema.
        #[arg(long, value_name = "PATH")]
        from_json: Option<String>,
        /// Shortcut: create with just a title and 30-day horizon.
        #[arg(long, value_name = "TITLE", conflicts_with = "from_json")]
        title: Option<String>,
    },
    /// Show a single source tile by id.
    Get {
        id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Replace the schedule, plan, and horizon of an existing source tile.
    Update {
        id: uuid::Uuid,
        #[arg(long, value_name = "PATH")]
        from_json: Option<String>,
    },
    /// Cancel a source tile.
    Cancel {
        id: uuid::Uuid,
        #[arg(long, default_value = "user-requested")]
        reason: String,
    },
    /// Re-derive the per-segment / per-occurrence completion for a source tile.
    Completion {
        id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// List the placements produced by a source tile.
    Placements {
        id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Trigger a reflow of a source tile's placements within a date range.
    Reflow {
        id: uuid::Uuid,
        /// Start of the reflow range (RFC 3339).
        #[arg(long)]
        from: chrono::DateTime<chrono::Utc>,
        /// End of the reflow range (RFC 3339).
        #[arg(long)]
        to: chrono::DateTime<chrono::Utc>,
    },
}

#[derive(Debug, Args)]
pub struct ExecutionsArgs {
    #[command(subcommand)]
    pub sub: ExecutionsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ExecutionsCommand {
    /// Start an execution for the given placement.
    Start {
        placement_id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Pause a running execution.
    Pause {
        execution_id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Resume a paused execution.
    Resume {
        execution_id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Finish an execution.
    Finish {
        execution_id: uuid::Uuid,
        /// Numeric finish kind. Use `--note` for a human-readable summary.
        #[arg(long)]
        kind: i32,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
pub struct PromptsArgs {
    #[command(subcommand)]
    pub sub: PromptsCommand,
}

#[derive(Debug, Subcommand)]
pub enum PromptsCommand {
    /// List prompts awaiting user action.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Resolve a prompt with an action.
    Resolve {
        id: uuid::Uuid,
        /// How to resolve the prompt: `ack` (acknowledged), `dismiss`
        /// (dismissed), or `act` (action taken).
        #[arg(long, value_parser = ["ack", "acknowledged", "dismiss", "dismissed", "act", "action", "action_taken"])]
        resolution: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ask the server to raise a prompt (kind is a numeric enum).
    Request {
        /// Numeric prompt kind (see `tastile docs` for the table).
        #[arg(long, value_name = "N")]
        kind: i32,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Respond to a startup-recovery prompt (alternative to `resolve` —
    /// the prompt id is in the body, not the path).
    StartupRecovery {
        id: uuid::Uuid,
        /// `ack`, `dismiss`, or `act`.
        #[arg(long, value_parser = ["ack", "acknowledged", "dismiss", "dismissed", "act", "action", "action_taken"])]
        resolution: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for.
    #[arg(value_enum, value_hint = ValueHint::Other)]
    pub shell: clap_complete::Shell,
}
