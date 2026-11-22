//! `pstudio`: the command line.
//!
//! `export`, `validate` and `inspect` run exactly the code the app runs: the same importers,
//! rules, compiler, validator and writers. Exit codes: 0 success, 1 the work was refused (the
//! design or a file is invalid), 2 usage or file-system problems. A refused export writes
//! nothing. Diagnostics are printed with their stable keys so scripts and CI can match on them.
//!
//! [`run`] takes its arguments and output streams as parameters, so everything is testable
//! without spawning a process.

mod commands;
mod context;
mod device;
mod report;
mod source;
mod time;

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

pub use time::rfc3339_utc;

/// Where the command writes, and the clock it reads.
pub struct Io<'a> {
    /// Standard output.
    pub out: &'a mut dyn Write,
    /// Standard error.
    pub err: &'a mut dyn Write,
    /// The current time in Unix seconds, used when `--export-time` is not given.
    pub now_unix_secs: u64,
}

impl std::fmt::Debug for Io<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Io")
            .field("now_unix_secs", &self.now_unix_secs)
            .finish_non_exhaustive()
    }
}

/// Why a command did not succeed, and the exit code it maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// Bad usage or a file-system problem (exit 2).
    Usage(String),
    /// The work was refused because the design or a file is invalid (exit 1).
    Refused(String),
}

impl Failure {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    pub(crate) fn refused(message: impl Into<String>) -> Self {
        Self::Refused(message.into())
    }

    fn code(&self) -> i32 {
        match self {
            Self::Refused(_) => 1,
            Self::Usage(_) => 2,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Usage(m) | Self::Refused(m) => m,
        }
    }
}

/// The plotter-studio command line.
#[derive(Debug, Parser)]
#[command(
    name = "pstudio",
    version,
    about = "Plotter Studio on the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile a project or an SVG/DXF file and write a `.psjob` bundle and/or G-code.
    Export(ExportArgs),
    /// Validate a project, a drawing, a library file, a package or a `.psjob` bundle.
    Validate(ValidateArgs),
    /// Summarise a project or a `.psjob` bundle.
    Inspect(InspectArgs),
    /// List the machines the app can reach.
    Devices(DevicesArgs),
    /// Send a `.psjob` bundle to a machine.
    Run(RunArgs),
    /// Work with device plugins.
    Device(DeviceArgs),
}

#[derive(Debug, Args)]
pub(crate) struct DeviceArgs {
    #[command(subcommand)]
    pub command: DeviceCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DeviceCommand {
    /// Check a `.psdevice` plugin against a recorded session or a simulated machine.
    Test(DeviceTestArgs),
}

#[derive(Debug, Args)]
pub(crate) struct DeviceTestArgs {
    /// The `.psdevice` package to check.
    pub plugin: PathBuf,
    /// Replay this recorded session (JSON) to the plugin.
    #[arg(long)]
    pub transcript: Option<PathBuf>,
    /// Run the plugin through a whole job against a simulated serial machine.
    #[arg(long)]
    pub simulate: bool,
}

/// What an export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// A `.psjob` bundle.
    Psjob,
    /// One `.gcode` file per mat.
    Gcode,
    /// Both.
    Both,
}

/// The machine, libraries and calibration every compiling command needs.
#[derive(Debug, Args)]
pub(crate) struct Setup {
    /// The machine profile id, such as `generic.cutter-small`.
    #[arg(long)]
    pub machine: Option<String>,
    /// Mapping rules (a JSON array), required for SVG and DXF inputs.
    #[arg(long)]
    pub rules: Option<PathBuf>,
    /// A calibration file (JSON).
    #[arg(long)]
    pub calibration: Option<PathBuf>,
    /// A library folder to add to the built-in tools, materials and profiles.
    #[arg(long)]
    pub library_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    /// A `.pstudio` project, or an `.svg`, `.svgz` or `.dxf` drawing.
    pub input: PathBuf,
    #[command(flatten)]
    pub setup: Setup,
    /// The folder to write into (default: the current folder).
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// What to write.
    #[arg(long, value_enum, default_value = "psjob")]
    pub format: Format,
    /// The export time (RFC 3339), for reproducible bundles.
    #[arg(long)]
    pub export_time: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct ValidateArgs {
    /// What to validate.
    pub input: PathBuf,
    #[command(flatten)]
    pub setup: Setup,
    /// Fail on warnings as well as errors.
    #[arg(long)]
    pub deny_warnings: bool,
    /// Print machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct InspectArgs {
    /// A `.pstudio` project or a `.psjob` bundle.
    pub input: PathBuf,
    /// Print machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DevicesArgs {
    /// Include the built-in simulated machine.
    #[arg(long)]
    pub simulate: bool,
    /// Print machine-readable JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    /// The `.psjob` bundle to run.
    pub input: PathBuf,
    /// The device id from `pstudio devices`.
    #[arg(long)]
    pub device: String,
    /// Run only this mat (default: every mat in order).
    #[arg(long)]
    pub mat: Option<u32>,
    /// Use the built-in simulated machine.
    #[arg(long)]
    pub simulate: bool,
    /// Confirm the job's prompts (load a tool, unload the mat) without asking.
    #[arg(long)]
    pub yes: bool,
}

/// Runs the command line and returns the exit code.
pub fn run(args: &[String], io: &mut Io<'_>) -> i32 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let help_or_version = !e.use_stderr();
            let text = e.render().to_string();
            let _ = if help_or_version {
                io.out.write_all(text.as_bytes())
            } else {
                io.err.write_all(text.as_bytes())
            };
            return if help_or_version { 0 } else { 2 };
        }
    };
    let result = match cli.command {
        Command::Export(a) => commands::export(&a, io),
        Command::Validate(a) => commands::validate(&a, io),
        Command::Inspect(a) => commands::inspect(&a, io),
        Command::Devices(a) => device::devices(&a, io),
        Command::Run(a) => device::run(&a, io),
        Command::Device(a) => match a.command {
            DeviceCommand::Test(t) => device::test_plugin(&t, io),
        },
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            let _ = writeln!(io.err, "pstudio: {}", failure.message());
            failure.code()
        }
    }
}
