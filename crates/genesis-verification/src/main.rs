use genesis_verification::{verify, ArtifactPin, ExecutionEvidence, OracleSuite};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::process::ExitCode;

const HELP: &str = "Usage: genesis-verify --suite <oracle.json> --artifact <pin.json> --runs <receipts.json>\n\nPrints a scoped JSON report. Exit codes: 0 accepted, 1 rejected, 2 invalid input.\nSuite and artifact must be trusted; receipt metadata must come from your adapter.\nOffline JSON receipts do not prove an authenticated provider execution.\n";
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("genesis-verify: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--help"] || args.as_slice() == ["-h"] {
        print!("{HELP}");
        return Ok(0);
    }
    let mut paths = BTreeMap::new();
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        if !["--suite", "--artifact", "--runs"].contains(&flag.as_str()) {
            return Err(format!("unknown argument {flag:?}\n{HELP}"));
        }
        let path = iter.next().filter(|p| !p.starts_with("--"))
            .ok_or_else(|| format!("missing path for {flag}"))?;
        if paths.insert(flag.as_str(), path.as_str()).is_some() {
            return Err(format!("duplicate argument {flag}"));
        }
    }
    let get = |key| paths.get(key).copied().ok_or_else(|| format!("missing {key}\n{HELP}"));
    let suite: OracleSuite = read_json(get("--suite")?)?;
    let artifact: ArtifactPin = read_json(get("--artifact")?)?;
    let runs: Vec<ExecutionEvidence> = read_json(get("--runs")?)?;
    let outcome = verify(&suite, &artifact, &runs).map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, outcome.report())
        .map_err(|e| format!("cannot write report: {e}"))?;
    writeln!(stdout).map_err(|e| format!("cannot write report: {e}"))?;
    Ok(if outcome.is_accepted() { 0 } else { 1 })
}

fn read_json<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {path:?}: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {path:?}: {e}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("{path:?} exceeds the 8 MiB input limit"));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        // Don't echo raw data/unknown field names from an untrusted JSON payload.
        format!("invalid JSON/schema in {path:?} at line {}, column {}", e.line(), e.column())
    })
}
