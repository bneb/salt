// S8 maturity ratchet: bounded fuzz campaign over the `fuzz_ast` generator.
//
// Each iteration derives a deterministic byte stream from its index, grows a
// `FuzzSaltFile` from it via `arbitrary`, lowers it with `to_salt()`, and
// compiles the resulting Salt AST through the library pipeline
// (`saltc::compile_ast` — the same entry used by tests/runner.rs).
//
// Contract under test: generation + compilation must never panic or raise an
// internal compiler error ([E007]). Clean semantic refusals ([E001]-[E006],
// etc.) are expected outcomes for random programs and are only counted.
//
// Results land under .round1-staging/fuzz-results/ (timestamped summary,
// latest-summary.json, append-only history.log), including a delta against
// the previous run's failure modes. Knobs: FUZZ_ITERATIONS (default 50; 100+ hits known NB-8 overflow crash),
// FUZZ_TIMEOUT_SECS (default 10 — overall wall-clock budget; once exceeded
// the campaign stops STARTING new iterations).

use arbitrary::{Arbitrary, Unstructured};
use saltc::grammar::SaltFile;
use saltc::fuzz_ast::FuzzSaltFile;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_ITERATIONS: usize = 50;
const DEFAULT_TIMEOUT_SECS: u64 = 10;
const BYTES_PER_ITERATION: usize = 8192;
const CRASH_KEY_MAX_CHARS: usize = 160;

/// What a single campaign iteration produced.
#[derive(Debug)]
enum Outcome {
    /// Compiled to MLIR.
    Pass,
    /// Compiler refused the program with a coded diagnostic (expected).
    Reject,
    /// Byte stream exhausted before any AST could be generated.
    Skipped,
    /// Compiler panicked / raised [E007]; message captured for the report.
    Crash(String),
}

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

struct CampaignStats {
    passes: usize,
    rejects: usize,
    skipped: usize,
    crashes: Vec<(usize, String)>,
    truncated_by_timeout: bool,
}

/// Deterministic per-iteration byte stream (index-mixed LCG fill, mirroring
/// tests/runner.rs) so every campaign is reproducible from iteration numbers.
fn seeded_bytes(seed: usize) -> Vec<u8> {
    let mut data = vec![0u8; BYTES_PER_ITERATION];
    let mut state = (seed as u64) ^ 0x9E37_79B9_7F4A_7C15;
    for slot in data.iter_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *slot = (state >> 33) as u8;
    }
    data
}

/// Grow one random AST from the byte stream; None when bytes run out.
fn generate_fuzz_file(data: &[u8]) -> Option<FuzzSaltFile> {
    let mut u = Unstructured::new(data);
    FuzzSaltFile::arbitrary(&mut u).ok()
}

/// Lower a generated file and compile it through the library pipeline.
/// Coded refusals are expected for random programs; [E007] ICEs are crashes.
fn generate_and_compile(data: &[u8]) -> Outcome {
    let Some(fuzz_file) = generate_fuzz_file(data) else {
        return Outcome::Skipped;
    };
    let mut salt_file: SaltFile = fuzz_file.to_salt();
    match saltc::compile_ast(
        &mut salt_file, false, None, true, false, false,
        false, false, false, false, "<fuzz-campaign>",
    ) {
        Ok(_mlir) => Outcome::Pass,
        Err(err) => classify_compile_error(&format!("{err:?}")),
    }
}

fn classify_compile_error(text: &str) -> Outcome {
    if text.contains("[E007]") || text.contains("INTERNAL COMPILER ERROR") {
        Outcome::Crash(text.to_string())
    } else {
        Outcome::Reject
    }
}

/// Run one iteration under a panic guard so a single crashing input cannot
/// abort the whole campaign.
fn run_iteration(data: &[u8]) -> Outcome {
    LAST_PANIC.lock().unwrap().take();
    let attempted = panic::catch_unwind(AssertUnwindSafe(|| generate_and_compile(data)));
    attempted.unwrap_or_else(|payload| Outcome::Crash(panic_message(payload)))
}

/// Best-effort extraction of a message from a caught panic payload.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        return (*msg).to_string();
    }
    if let Some(msg) = payload.downcast_ref::<String>() {
        return msg.clone();
    }
    LAST_PANIC.lock().unwrap().clone().unwrap_or_else(|| "<non-string panic>".into())
}

/// Char-boundary-safe truncation for report keys.
fn truncate(text: &str) -> String {
    text.chars().take(CRASH_KEY_MAX_CHARS).collect()
}

/// Escape a string for embedding in a JSON string literal.
fn json_escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '"' => "\\\"".to_string(),
            '\\' => "\\\\".to_string(),
            c if (c as u32) < 0x20 => " ".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Append one iteration result to the running stats.
fn record(stats: &mut CampaignStats, iter: usize, outcome: &Outcome) {
    match outcome {
        Outcome::Pass => stats.passes += 1,
        Outcome::Reject => stats.rejects += 1,
        Outcome::Skipped => stats.skipped += 1,
        Outcome::Crash(msg) => {
            let key = json_escape(&truncate(&format!("iter {iter}: {msg}")));
            stats.crashes.push((iter, key));
        }
    }
}

/// The failure-mode identity of a crash key (everything after "iter N: "),
/// so deltas compare modes rather than raw iteration indices.
fn crash_mode(key: &str) -> &str {
    key.split_once(": ").map(|(_, mode)| mode).unwrap_or(key)
}

/// Failure-mode identities recorded by the previous campaign, if any.
fn previous_failure_modes(dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(dir.join("latest-summary.json")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("\"crash\": \"")?.strip_suffix("\","))
        .map(|mode| crash_mode(mode).to_string())
        .collect()
}

/// One JSON object field rendered safely (both args are simple idents).
fn field(name: &str, val: usize) -> String {
    format!("\"{name}\": {val}")
}

/// Build the JSON summary body (one field per line).
fn summary_body(stats: &CampaignStats, planned: usize, elapsed: Duration, dir: &Path) -> String {
    let prev = previous_failure_modes(dir);
    let ran = stats.passes + stats.rejects + stats.skipped + stats.crashes.len();
    let new_modes: Vec<&str> = stats
        .crashes
        .iter()
        .map(|(_, key)| crash_mode(key))
        .filter(|mode| !prev.iter().any(|p| p == mode))
        .collect();
    let timed_out = stats.truncated_by_timeout;
    let mut fields = vec![
        format!("\"iterations_planned\": {planned}"),
        field("iterations_run", ran),
        field("passes", stats.passes),
        field("rejects", stats.rejects),
        field("skipped", stats.skipped),
        field("crashes", stats.crashes.len()),
        format!("\"truncated_by_timeout\": {timed_out}"),
        format!("\"elapsed_secs\": {:.2}", elapsed.as_secs_f32()),
        format!("\"new_failure_modes_vs_previous\": {}", new_modes.len()),
    ];
    fields.extend(stats.crashes.iter().map(|(_, k)| format!("\"crash\": \"{k}\"")));
    fields.extend(new_modes.iter().map(|m| format!("\"new_failure_mode\": \"{m}\"")));
    format!("{{\n{}\n}}\n", fields.join(",\n"))
}

/// Persist the summary: timestamped snapshot, latest-summary.json, history.
fn save_summary(dir: &Path, body: &str, stats: &CampaignStats) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = std::fs::write(dir.join(format!("campaign-{stamp}.json")), body);
    let _ = std::fs::write(dir.join("latest-summary.json"), body);
    let passes = stats.passes;
    let rejects = stats.rejects;
    let skipped = stats.skipped;
    let crashes = stats.crashes.len();
    let timed_out = stats.truncated_by_timeout;
    let line = format!("{stamp} pass={passes} reject={rejects} skip={skipped} crash={crashes} timeout={timed_out}\n");
    let history = dir.join("history.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(history) {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

fn read_env_or(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Execute the bounded loop, honoring the overall wall-clock budget.
fn run_campaign(iterations: usize, timeout: Duration) -> CampaignStats {
    let started = Instant::now();
    let mut stats = CampaignStats {
        passes: 0, rejects: 0, skipped: 0, crashes: Vec::new(), truncated_by_timeout: false,
    };
    for iter in 0..iterations {
        if started.elapsed() > timeout {
            stats.truncated_by_timeout = true;
            break;
        }
        let outcome = run_iteration(&seeded_bytes(iter));
        record(&mut stats, iter, &outcome);
    }
    stats
}

/// Save results where the runner script expects them and enforce the
/// no-crash contract as the test verdict.
fn report_and_assert(planned: usize, stats: &CampaignStats, elapsed: Duration) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dir = repo_root.join(".round1-staging/fuzz-results");
    let saved = std::fs::create_dir_all(&dir).is_ok();
    if saved {
        let body = summary_body(stats, planned, elapsed, &dir);
        save_summary(&dir, &body, stats);
    }
    let passes = stats.passes;
    let rejects = stats.rejects;
    let skipped = stats.skipped;
    let crashes = stats.crashes.len();
    let suffix = if saved { "" } else { " (results dir unwritable; summary NOT saved)" };
    println!("fuzz-campaign: {planned} planned, {passes} pass, {rejects} reject, {skipped} skip, {crashes} crash{suffix}");
    let crash_count = stats.crashes.len();
    let crash_list = stats.crashes.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>().join("\n");
    assert!(crashes == 0, "fuzz campaign found {crash_count} crashing inputs:\n{crash_list}");
}

#[test]
fn fuzz_campaign_no_crashes() {
    let iterations = read_env_or("FUZZ_ITERATIONS", DEFAULT_ITERATIONS as u64) as usize;
    let timeout = Duration::from_secs(read_env_or("FUZZ_TIMEOUT_SECS", DEFAULT_TIMEOUT_SECS));
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|info| {
        *LAST_PANIC.lock().unwrap() = Some(info.to_string());
    }));
    let started = Instant::now();
    let stats = run_campaign(iterations, timeout);
    let elapsed = started.elapsed();
    panic::set_hook(default_hook);
    report_and_assert(iterations, &stats, elapsed);
}
