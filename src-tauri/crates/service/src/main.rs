//! The `deskmate-service` program. The service wrapper and the command line
//! arrive with the Windows layer; until then there is nothing to start.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("deskmate-service: this build has no platform layer yet");
    ExitCode::FAILURE
}
