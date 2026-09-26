//! CLI surface definition (clap derive).

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

    /// Show the next actionable item (alias for `tiles --next`).
    Today(TodayArgs),

    /// Schedule operations (regenerate, publish).
    Schedule(ScheduleArgs),

    /// Inspect or cancel source tiles.
    SourceTiles(SourceTilesArgs),

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

    /// Re-run the token exchange against an existing authorization code
    /// (intended for `tastile doctor --verbose` debugging).
    Exchange {
        #[arg(long)]
        code: String,
        #[arg(long)]
        state: String,
    },
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
    /// Show a single source tile by id.
    Get {
        id: uuid::Uuid,
        #[arg(long)]
        json: bool,
    },
    /// Cancel a source tile.
    Cancel {
        id: uuid::Uuid,
        #[arg(long, default_value = "user-requested")]
        reason: String,
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
    /// Resolve a prompt with an answer.
    Resolve {
        id: uuid::Uuid,
        /// Numeric answer kind. Use `--json` to discover the available kinds.
        #[arg(long)]
        answer_kind: i32,
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for.
    #[arg(value_enum, value_hint = ValueHint::Other)]
    pub shell: clap_complete::Shell,
}
