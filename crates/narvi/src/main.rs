//! `narvi` — CLI client over the daemon socket. Thin: parse args, send, print.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use narvi_core::proto::{Command, Status};
use narvi_core::{Client, ColorParams, Param};
use serde_json::Value;

/// Real-time color management for Hyprland.
#[derive(Parser, Debug)]
#[command(name = "narvi", version, about)]
struct Cli {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    /// Override the daemon socket path.
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,
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
    /// Relative change: `narvi nudge vibrance -- +0.05`.
    Nudge {
        param: String,
        #[arg(allow_hyphen_values = true)]
        delta: f32,
    },
    /// Set several params at once.
    Apply(ApplyArgs),
    /// Enable the shader.
    On,
    /// Disable the shader.
    Off,
    /// Toggle the shader.
    Toggle,
    /// Profile operations (`narvi profile <name>` loads).
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
    /// List profile names.
    List,
    /// Save current params under a name.
    Save { name: String },
    /// Delete a profile.
    Delete { name: String },
    /// Cycle to the next profile.
    Next,
    /// Cycle to the previous profile.
    Prev,
    /// Load a profile by name.
    #[command(external_subcommand)]
    Load(Vec<String>),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if matches!(cli.command, Cmd::Gui) {
        let err = std::process::Command::new("narvi-gui").spawn();
        return err.map(drop).context("failed to spawn narvi-gui");
    }

    let path = match &cli.socket {
        Some(p) => p.clone(),
        None => narvi_core::socket_path()?,
    };
    let mut client = Client::connect(&path).map_err(|e| {
        anyhow!(
            "cannot reach narvid at {} ({e})\nstart it with `systemctl --user start narvi` or run `narvid`",
            path.display()
        )
    })?;

    let command = match &cli.command {
        // Merge apply flags onto CURRENT params so unspecified fields keep their value.
        Cmd::Apply(a) => {
            let current = serde_json::from_value(client.request(Command::Get)?)?;
            Command::Apply {
                params: merge(a, current),
            }
        }
        other => to_command(other)?,
    };
    let data = client.request(command)?;

    if cli.json {
        println!("{data}");
        return Ok(());
    }
    print_human(&cli.command, &data)?;
    Ok(())
}

fn to_command(cmd: &Cmd) -> Result<Command> {
    Ok(match cmd {
        Cmd::Status => Command::Status,
        Cmd::Get { .. } => Command::Get,
        Cmd::Set { param, value } => Command::Set {
            param: Param::from_str(param)?,
            value: *value,
        },
        Cmd::Nudge { param, delta } => Command::Nudge {
            param: Param::from_str(param)?,
            delta: *delta,
        },
        Cmd::Apply(_) => unreachable!("handled in main with current params"),
        Cmd::On => Command::On,
        Cmd::Off => Command::Off,
        Cmd::Toggle => Command::Toggle,
        Cmd::Profile(p) => match p {
            ProfileCmd::List => Command::ProfileList,
            ProfileCmd::Save { name } => Command::ProfileSave {
                name: name.clone(),
                params: None,
            },
            ProfileCmd::Delete { name } => Command::ProfileDelete { name: name.clone() },
            ProfileCmd::Next => Command::ProfileNext,
            ProfileCmd::Prev => Command::ProfilePrev,
            ProfileCmd::Load(args) => match args.first() {
                Some(name) => Command::ProfileLoad { name: name.clone() },
                None => anyhow::bail!("profile name required"),
            },
        },
        Cmd::Reload => Command::Reload,
        Cmd::Gui => unreachable!("handled before connecting"),
    })
}

fn print_human(cmd: &Cmd, data: &Value) -> Result<()> {
    match cmd {
        Cmd::Status | Cmd::On | Cmd::Off | Cmd::Toggle | Cmd::Reload => print_status(data)?,
        Cmd::Get { param } => {
            let params: ColorParams = serde_json::from_value(data.clone())?;
            match param {
                Some(name) => println!("{}", fmt_value(&params, Param::from_str(name)?)),
                None => print_params(&params),
            }
        }
        Cmd::Set { .. } | Cmd::Nudge { .. } | Cmd::Apply(_) => {
            let params: ColorParams = serde_json::from_value(data.clone())?;
            print_params(&params);
        }
        Cmd::Profile(p) => match p {
            ProfileCmd::List | ProfileCmd::Save { .. } | ProfileCmd::Delete { .. } => {
                let names: Vec<String> = serde_json::from_value(data.clone())?;
                for n in names {
                    println!("{n}");
                }
            }
            _ => print_status(data)?,
        },
        Cmd::Gui => {}
    }
    Ok(())
}

fn merge(a: &ApplyArgs, mut p: ColorParams) -> ColorParams {
    p.vibrance = a.vibrance.unwrap_or(p.vibrance);
    p.saturation = a.saturation.unwrap_or(p.saturation);
    p.temperature = a.temperature.unwrap_or(p.temperature);
    p.brightness = a.brightness.unwrap_or(p.brightness);
    p.contrast = a.contrast.unwrap_or(p.contrast);
    p.gamma = a.gamma.unwrap_or(p.gamma);
    p.rgb = [
        a.r.unwrap_or(p.rgb[0]),
        a.g.unwrap_or(p.rgb[1]),
        a.b.unwrap_or(p.rgb[2]),
    ];
    p
}

fn fmt_value(p: &ColorParams, param: Param) -> String {
    match param {
        Param::Temperature => p.temperature.to_string(),
        other => p.get(other).to_string(),
    }
}

fn print_params(p: &ColorParams) {
    println!("vibrance    {}", p.vibrance);
    println!("saturation  {}", p.saturation);
    println!("temperature {}", p.temperature);
    println!("brightness  {}", p.brightness);
    println!("contrast    {}", p.contrast);
    println!("gamma       {}", p.gamma);
    println!("rgb         {} {} {}", p.rgb[0], p.rgb[1], p.rgb[2]);
}

fn print_status(data: &Value) -> Result<()> {
    let s: Status = serde_json::from_value(data.clone())?;
    println!("enabled     {}", s.enabled);
    println!(
        "profile     {}",
        s.active_profile.as_deref().unwrap_or("(unsaved)")
    );
    if let Some(class) = &s.auto_switch {
        println!("auto-switch {class}");
    }
    if s.scheduling.mode != "off" {
        let next = s
            .scheduling
            .next_transition_secs
            .map(|t| format!(", next transition in {}m", t / 60))
            .unwrap_or_default();
        println!(
            "schedule    {} (blend {:.2}{next})",
            s.scheduling.mode, s.scheduling.blend
        );
    }
    print_params(&s.params);
    Ok(())
}
