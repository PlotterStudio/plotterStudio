//! The `pstudio` binary: a thin wrapper that wires the real streams and clock into
//! [`ps_cli::run`].

use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let now_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    let mut io = ps_cli::Io {
        out: &mut out,
        err: &mut err,
        now_unix_secs,
    };
    let code = ps_cli::run(&args, &mut io);
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
