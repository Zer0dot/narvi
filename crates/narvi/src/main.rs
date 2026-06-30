//! `narvi` — CLI client over the daemon socket. Thin: parse args, send one request, print.
//!
//! M3: connect to the socket, map each subcommand to a `Command`, print human or `--json`.

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Real-time color management for Hyprland.
#[derive(Parser, Debug)]
#[command(name = "narvi", version, about)]
struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    /// Override the daemon socket path.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<String>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Human summary of current state.
    Status,
    /// Print all params, or one value.
    Get { param: Option<String> },
    /// Set one param: `narvi set vibrance 1.3`.
    Set { param: String, value: f32 },
    /// Relative change: `narvi nudge vibrance +0.05`.
    Nudge { param: String, delta: f32 },
    /// Set several params at once.
    Apply(ApplyArgs),
    /// Enable the shader.
    On,
    /// Disable the shader.
    Off,
    /// Toggle the shader.
    Toggle,
    /// Profile operations.
    #[command(subcommand)]
    Profile(ProfileCmd),
    /// Reload config.toml from disk.
    Reload,
    /// Spawn the GUI.
    Gui,
}

#[derive(clap::Args, Debug)]
struct ApplyArgs {
    #[arg(long)]
    vibrance: Option<f32>,
    #[arg(long)]
    saturation: Option<f32>,
    #[arg(long)]
    temperature: Option<u32>,
    #[arg(long)]
    brightness: Option<f32>,
    #[arg(long)]
    contrast: Option<f32>,
    #[arg(long)]
    gamma: Option<f32>,
    #[arg(long)]
    r: Option<f32>,
    #[arg(long)]
    g: Option<f32>,
    #[arg(long)]
    b: Option<f32>,
}

#[derive(Subcommand, Debug)]
enum ProfileCmd {
    /// Load a profile by name.
    #[command(external_subcommand)]
    Load(Vec<String>),
    /// List profile names.
    List,
    /// Save current (or given) params under a name.
    Save { name: String },
    /// Delete a profile.
    Delete { name: String },
    /// Cycle to the next profile.
    Next,
    /// Cycle to the previous profile.
    Prev,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // M3: connect to the socket (cli.socket or default), send the mapped Command, print
    // human or `--json`. For now, echo the parsed intent so the surface is exercised.
    eprintln!(
        "narvi {} — not yet implemented (M3)",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("would run: {} (json={})", describe(&cli.command), cli.json);
    std::process::exit(1);
}

/// One-line description of the parsed command. Replaced by socket dispatch in M3.
fn describe(cmd: &Cmd) -> String {
    match cmd {
        Cmd::Status => "status".into(),
        Cmd::Get { param } => format!("get {}", param.as_deref().unwrap_or("(all)")),
        Cmd::Set { param, value } => format!("set {param} {value}"),
        Cmd::Nudge { param, delta } => format!("nudge {param} {delta:+}"),
        Cmd::Apply(a) => format!("apply {a:?}"),
        Cmd::On => "on".into(),
        Cmd::Off => "off".into(),
        Cmd::Toggle => "toggle".into(),
        Cmd::Profile(p) => match p {
            ProfileCmd::Load(args) => format!("profile load {}", args.join(" ")),
            ProfileCmd::List => "profile list".into(),
            ProfileCmd::Save { name } => format!("profile save {name}"),
            ProfileCmd::Delete { name } => format!("profile delete {name}"),
            ProfileCmd::Next => "profile next".into(),
            ProfileCmd::Prev => "profile prev".into(),
        },
        Cmd::Reload => "reload".into(),
        Cmd::Gui => "gui".into(),
    }
}
