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
use uuid::Uuid;

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

    /// List placements intersecting a time range.
    Timeline(TimelineArgs),

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
    /// Lifecycle filter: `ready`, `started`, `done` or `closed`.
    #[arg(long, value_name = "STATE", value_parser = ["ready", "started", "done", "closed"])]
    pub lifecycle: Option<String>,
    /// Free-text search across title, description and labels.
    #[arg(long, value_name = "TEXT")]
    pub search: Option<String>,
    /// View mode: `flat` (default) or `by_state`.
    #[arg(long, value_name = "MODE", value_parser = ["flat", "by_state"])]
    pub view_mode: Option<String>,
    /// Maximum number of rows to request.
    #[arg(long, value_name = "N")]
    pub limit: Option<i64>,
    /// Show only the next actionable tile (the one with the earliest
    /// `projected_next_start_at`).
    #[arg(long, conflicts_with = "lifecycle")]
    pub next: bool,
}

#[derive(Debug, Args)]
pub struct TodayArgs {
    /// Output JSON instead of the human-readable table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TimelineArgs {
    /// Range start (RFC 3339); required, must be before --end. Window at most 31 days.
    #[arg(long)]
    pub start: String,

    /// Range end (RFC 3339); required.
    #[arg(long)]
    pub end: String,

    /// Include non-blocking LABEL (role = 1) periods; they never count as busy time.
    #[arg(long)]
    pub include_labels: bool,

    /// Include CLOSED placements.
    #[arg(long)]
    pub include_closed: bool,

    /// Include BLOCKED placements (with their violations).
    #[arg(long)]
    pub include_blocked: bool,

    /// Include placements nested under a parent.
    #[arg(long)]
    pub include_nested: bool,

    /// Restrict to placements nested under this parent (requires --include-nested).
    #[arg(long)]
    pub parent_placement_id: Option<Uuid>,

    /// Comma-separated or repeated owner UUIDs; defaults to the caller.
    #[arg(long = "owner", value_name = "UUID", value_delimiter = ',')]
    pub owners: Vec<Uuid>,

    /// Page size; omitted returns the complete in-range set.
    #[arg(long)]
    pub limit: Option<u32>,

    /// Row offset applied independently of --limit.
    #[arg(long)]
    pub offset: Option<u32>,

    /// Collapse overlapping relation-derived items into one representative per month.
    #[arg(long)]
    pub month: bool,

    /// Hide placements shorter than this many milliseconds.
    #[arg(long)]
    pub min_duration_ms: Option<i64>,

    /// Hide recurring placements whose cadence is at or below this step (ms).
    #[arg(long)]
    pub min_recurring_step_ms: Option<i64>,

    /// Comma-separated numeric PlacementSource values to exclude.
    #[arg(long, value_name = "KIND[,KIND...]")]
    pub exclude_source_kinds: Option<String>,

    /// Print the raw JSON response.
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
    /// Legacy placeholder; never silently reports success.
    Regenerate {
        /// Retained for compatibility; no API request will be issued.
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate a full schedule payload offline; send only with --apply.
    Publish {
        /// A JSON object with the required PublishScheduleDefinitionPayload fields.
        /// Use '-' to read standard input.
        #[arg(long, value_name = "PATH")]
        from_json: String,
        /// Actually send the schedule to the configured local Core API.
        #[arg(long)]
        apply: bool,
        /// Stable idempotency key to safely retry after an uncertain response.
        #[arg(long, value_name = "UUID")]
        idempotency_key: Option<uuid::Uuid>,
        /// Reject the publish unless the target is still at this revision.
        #[arg(long, value_name = "N")]
        expect_revision: Option<i64>,
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
        /// One-time occurrence at an exact instant (RFC 3339).
        #[arg(long, value_name = "RFC3339", conflicts_with_all = ["from_json", "daily", "weekly"])]
        once: Option<String>,
        /// Daily recurrence at a local wall time (HH:MM).
        #[arg(long, value_name = "HH:MM", conflicts_with_all = ["from_json", "once", "weekly"])]
        daily: Option<String>,
        /// Weekly recurrence at a local wall time (HH:MM).
        #[arg(long, value_name = "HH:MM", conflicts_with_all = ["from_json", "once", "daily"])]
        weekly: Option<String>,
        /// Weekdays for `--weekly`, e.g. `mon,wed,fri`.
        #[arg(
            long,
            value_name = "DAYS",
            requires = "weekly",
            conflicts_with = "daily"
        )]
        days: Option<String>,
        /// Anchor date for `--daily` / `--weekly` (YYYY-MM-DD, local).
        #[arg(long, value_name = "DATE", conflicts_with = "from_json")]
        start_date: Option<String>,
        /// Fixed local calendar offset in minutes east of UTC (default 0; 540 = Asia/Tokyo).
        #[arg(long, value_name = "MINUTES", conflicts_with = "from_json")]
        utc_offset: Option<i32>,
        /// Excluded local calendar dates (repeatable, YYYY-MM-DD).
        #[arg(long, value_name = "DATE", conflicts_with = "from_json")]
        exclude: Vec<String>,
        /// Required duration in minutes.
        #[arg(
            long,
            value_name = "MINUTES",
            default_value = "30",
            conflicts_with = "from_json"
        )]
        duration: i64,
        /// Placement window in minutes: how long after its nominal time an
        /// occurrence may still be placed.
        #[arg(
            long,
            value_name = "MINUTES",
            default_value = "240",
            conflicts_with = "from_json"
        )]
        window_minutes: i64,
        /// Horizon length in days.
        #[arg(
            long,
            value_name = "DAYS",
            default_value = "30",
            conflicts_with = "from_json"
        )]
        horizon_days: i64,
        /// Placement priority (higher wins a contended slot).
        #[arg(
            long,
            value_name = "N",
            default_value = "0",
            conflicts_with = "from_json"
        )]
        priority: i32,
        /// Request a monthly recurrence (not expressible in the pinned
        /// contract; the command refuses instead of approximating it).
        #[arg(long, conflicts_with_all = ["from_json", "once", "daily", "weekly"])]
        monthly: bool,
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
        /// Show the field-level diff against the stored source tile and stop.
        #[arg(long)]
        dry_run: bool,
        /// Override the freshly-read revision used to reject stale writes.
        #[arg(long, value_name = "N")]
        expect_revision: Option<i64>,
    },
    /// Cancel a source tile.
    Cancel {
        id: uuid::Uuid,
        /// Numeric cancel-reason registry code (0 = user-requested).
        #[arg(long, value_name = "CODE", default_value = "0")]
        reason_code: i32,
        /// Show the affected placements and stop.
        #[arg(long)]
        dry_run: bool,
        /// Override the freshly-read revision used to reject stale writes.
        #[arg(long, value_name = "N")]
        expect_revision: Option<i64>,
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
        /// Show the affected placements and stop.
        #[arg(long)]
        dry_run: bool,
        /// Override the freshly-read revision used to reject stale writes.
        #[arg(long, value_name = "N")]
        expect_revision: Option<i64>,
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
