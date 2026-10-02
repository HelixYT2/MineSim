//! `cargo xtask oracle`: replay the corpus through the kernel and report how far it agrees.
//!
//! ```text
//! cargo xtask oracle                         table over every scenario (free-run)
//! cargo xtask oracle walk_basic water_pool   only those scenarios
//! cargo xtask oracle walk_basic --verbose    per-tick field diffs for one scenario
//! cargo xtask oracle --resync                judge every tick from a correct start
//! cargo xtask oracle --strict                exit non-zero unless every scenario is exact
//! cargo xtask oracle --projectiles           list the recorded projectiles per scenario
//! cargo xtask oracle --bless [names]         (re)write corpus/golden-hashes.json
//! cargo xtask oracle --bless --oracle-only   corpus fingerprints only, without the kernel
//! ```
//!
//! The exit status is zero unless `--strict` is given (and some scenario diverges) or the
//! arguments/corpus are bad.

use crate::golden::{bless, replay_kernel_with};
use crate::projectiles::projectile_tracks;
use crate::replay::{Mode, Report};
use crate::{client_scenarios, Scenario};
use std::fmt::Write as _;

const USAGE: &str = "usage: cargo xtask oracle [SCENARIO...] [--verbose] [--resync] [--strict] \
[--from TICK] [--limit N] [--projectiles] [--bless [--oracle-only]]";

#[derive(Debug, Default)]
struct Options {
    names: Vec<String>,
    verbose: bool,
    resync: bool,
    strict: bool,
    projectiles: bool,
    bless: bool,
    oracle_only: bool,
    from: usize,
    limit: Option<usize>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut number = |flag: &str| -> Result<usize, String> {
            it.next()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("{flag} needs a number"))
        };
        match a.as_str() {
            "--verbose" | "-v" => o.verbose = true,
            "--resync" => o.resync = true,
            "--strict" => o.strict = true,
            "--projectiles" => o.projectiles = true,
            "--bless" => o.bless = true,
            "--oracle-only" => o.oracle_only = true,
            "--from" => o.from = number("--from")?,
            "--limit" => o.limit = Some(number("--limit")?),
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            name => o.names.push(name.to_string()),
        }
    }
    if o.oracle_only && !o.bless {
        return Err("--oracle-only only makes sense with --bless".into());
    }
    if o.bless && (o.verbose || o.resync || o.strict || o.projectiles) {
        return Err("--bless cannot be combined with replay options".into());
    }
    Ok(o)
}

/// Entry point for `cargo xtask oracle <args>`; returns the process exit code.
pub fn oracle_main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("oracle: {msg}\n{USAGE}");
            2
        }
    }
}

fn run(args: &[String]) -> Result<i32, String> {
    let o = parse(args)?;
    let all = client_scenarios();
    if all.is_empty() {
        return Err("no scenarios found under corpus/client".into());
    }
    for n in &o.names {
        if !all.contains(n) {
            return Err(format!("unknown scenario `{n}` (see corpus/client)"));
        }
    }
    if o.bless {
        let summary = bless(&o.names, o.oracle_only)?;
        println!(
            "wrote corpus/golden-hashes.json: {} scenarios, {} frozen (bit-exact), {}",
            summary.scenarios,
            summary.frozen,
            if summary.locked {
                "lock armed (every corpus scenario has a simulated hash)"
            } else {
                "lock not armed (some scenarios have no simulated hash; run --bless without --oracle-only)"
            }
        );
        println!("commit corpus/golden-hashes.json to make the lock effective in CI");
        return Ok(0);
    }
    let names = if o.names.is_empty() {
        all
    } else {
        o.names.clone()
    };
    if o.verbose && names.len() != 1 {
        return Err("--verbose shows one scenario at a time; name exactly one".into());
    }
    let mode = if o.resync {
        Mode::Resync
    } else {
        Mode::FreeRun
    };

    let mut outcomes = Vec::new();
    for n in &names {
        let scenario = Scenario::load(n)?;
        if o.projectiles {
            print_projectiles(&scenario);
        }
        outcomes.push(run_scenario(&scenario, mode));
    }
    match (&outcomes[0], o.verbose) {
        (Outcome::Ran(r), true) => print!("{}", verbose(r, o.from, o.limit)),
        (Outcome::Panicked { message, .. }, true) => {
            println!("{}: the kernel panicked: {message}", names[0])
        }
        _ => print!("{}", table(&outcomes)),
    }
    let all_exact = outcomes.iter().all(Outcome::is_exact);
    Ok(if o.strict && !all_exact { 1 } else { 0 })
}

/// How one scenario's replay ended: a [`Report`], or the kernel panicked (an unimplemented
/// module, say) — which must not hide the other scenarios from the table.
#[derive(Debug)]
pub enum Outcome {
    Ran(Report),
    Panicked {
        scenario: String,
        ticks: usize,
        message: String,
    },
}

impl Outcome {
    pub fn is_exact(&self) -> bool {
        matches!(self, Outcome::Ran(r) if r.is_exact())
    }
}

/// Replay one scenario through the kernel, catching a kernel panic.
pub fn run_scenario(scenario: &Scenario, mode: Mode) -> Outcome {
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        replay_kernel_with(scenario, mode)
    }));
    match run {
        Ok(report) => Outcome::Ran(report),
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "(non-string panic payload)".to_string());
            Outcome::Panicked {
                scenario: scenario.name.clone(),
                ticks: scenario.rows.len(),
                message,
            }
        }
    }
}

fn print_projectiles(scenario: &Scenario) {
    let tracks = projectile_tracks(scenario);
    if tracks.is_empty() {
        return;
    }
    println!("{}: {} projectiles", scenario.name, tracks.len());
    for t in &tracks {
        println!(
            "  id {:>4} {:<26} spawn row {:>3} server tick {:>6}  {} samples",
            t.id,
            t.kind,
            t.spawn_row.map_or("-".into(), |r| r.to_string()),
            t.spawn_server_tick.map_or("-".into(), |r| r.to_string()),
            t.samples.len()
        );
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

/// The summary table over a set of outcomes.
pub fn table(outcomes: &[Outcome]) -> String {
    let name_w = outcomes
        .iter()
        .map(|o| match o {
            Outcome::Ran(r) => r.scenario.len(),
            Outcome::Panicked { scenario, .. } => scenario.len(),
        })
        .max()
        .unwrap_or(8)
        .max("scenario".len());
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<name_w$}  {:>5}  {:>5}  {:>6}  {:>11}  first divergence",
        "scenario", "ticks", "exact", "exact%", "streak@tick"
    );
    let (mut ticks, mut exact, mut exact_scenarios) = (0usize, 0usize, 0usize);
    for o in outcomes {
        match o {
            Outcome::Ran(r) => {
                ticks += r.ticks;
                exact += r.exact_ticks;
                exact_scenarios += usize::from(r.is_exact());
                let streak = format!("{}@{}", r.longest_streak, r.longest_streak_start);
                let pct = if r.ticks == 0 {
                    100.0
                } else {
                    100.0 * r.exact_ticks as f64 / r.ticks as f64
                };
                let first = match &r.first_divergence {
                    Some(d) => clip(&d.to_string(), 90),
                    None => "none (bit-exact)".to_string(),
                };
                let _ = writeln!(
                    out,
                    "{:<name_w$}  {:>5}  {:>5}  {:>5.1}%  {:>11}  {first}",
                    r.scenario, r.ticks, r.exact_ticks, pct, streak
                );
            }
            Outcome::Panicked {
                scenario,
                ticks: n,
                message,
            } => {
                ticks += n;
                let _ = writeln!(
                    out,
                    "{scenario:<name_w$}  {n:>5}  {:>5}  {:>6}  {:>11}  KERNEL PANIC: {}",
                    "-",
                    "-",
                    "-",
                    clip(message, 70)
                );
            }
        }
    }
    let mode = outcomes
        .iter()
        .find_map(|o| match o {
            Outcome::Ran(r) => Some(r.mode.name()),
            Outcome::Panicked { .. } => None,
        })
        .unwrap_or("free-run");
    let pct = if ticks == 0 {
        100.0
    } else {
        100.0 * exact as f64 / ticks as f64
    };
    let _ = writeln!(
        out,
        "\n{} scenarios, {ticks} ticks: {exact} exact ({pct:.1}%); {exact_scenarios}/{} scenarios bit-exact [{mode}, ms_kernel::player::tick]",
        outcomes.len(),
        outcomes.len()
    );
    out
}

/// The per-tick diff listing of one report: every tick with a divergence (from `from`, at most
/// `limit` ticks), then the summary line.
pub fn verbose(r: &Report, from: usize, limit: Option<usize>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} [{}]: {} ticks, {} exact, longest exact streak {} (from tick {}), {} ticks with matching state hash",
        r.scenario,
        r.mode.name(),
        r.ticks,
        r.exact_ticks,
        r.longest_streak,
        r.longest_streak_start,
        r.hash_exact_ticks()
    );
    let mut shown = 0usize;
    for (t, diffs) in r.diffs.iter().enumerate().skip(from) {
        if diffs.is_empty() {
            continue;
        }
        if limit.is_some_and(|l| shown >= l) {
            let _ = writeln!(
                out,
                "... (stopped after {shown} diverging ticks; raise --limit)"
            );
            break;
        }
        shown += 1;
        let _ = writeln!(
            out,
            "t={t}: {} field{} differ  (H_sim {:016x}, H_oracle {:016x})",
            diffs.len(),
            if diffs.len() == 1 { "" } else { "s" },
            r.sim_hashes[t],
            r.oracle_hashes[t]
        );
        let w = diffs.iter().map(|d| d.field.len()).max().unwrap_or(0);
        for d in diffs {
            let _ = writeln!(
                out,
                "    {:<w$}  expected {}  actual {}",
                d.field, d.expected, d.actual
            );
        }
    }
    if shown == 0 {
        let _ = writeln!(out, "no diverging ticks from tick {from}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_names() {
        let o = parse(&args(&[
            "walk_basic",
            "--verbose",
            "--from",
            "10",
            "--limit",
            "3",
        ]))
        .unwrap();
        assert_eq!(o.names, ["walk_basic"]);
        assert!(o.verbose);
        assert_eq!((o.from, o.limit), (10, Some(3)));
        assert!(parse(&args(&["--bogus"])).is_err());
        assert!(parse(&args(&["--from"])).is_err());
        assert!(parse(&args(&["--oracle-only"])).is_err());
        assert!(parse(&args(&["--bless", "--strict"])).is_err());
        assert!(parse(&args(&["--bless", "--oracle-only"])).is_ok());
    }

    #[test]
    fn rejects_unknown_scenarios_and_multi_scenario_verbose() {
        assert_eq!(oracle_main(&args(&["no_such_scenario"])), 2);
        assert_eq!(
            oracle_main(&args(&["walk_basic", "water_pool", "--verbose"])),
            2
        );
    }

    #[test]
    fn table_and_verbose_render_a_report() {
        let s = Scenario::load("walk_basic").unwrap();
        let r = crate::replay(&s, |_, _, _| {});
        let t = table(&[Outcome::Ran(r.clone())]);
        assert!(t.contains("scenario"));
        assert!(t.contains("walk_basic"));
        assert!(t.contains("0/1 scenarios bit-exact"), "{t}");
        let v = verbose(&r, 0, Some(2));
        assert!(v.contains("t="), "{v}");
        assert!(v.contains("expected"), "{v}");
        assert!(v.contains("stopped after 2"), "{v}");
        let none = verbose(&r, r.ticks, None);
        assert!(none.contains("no diverging ticks"));
    }

    #[test]
    fn a_panicking_kernel_is_reported_not_propagated() {
        let s = Scenario::load("walk_basic").unwrap();
        // `run_scenario` goes through the real kernel, which must not panic on the corpus; the
        // table path for a panic is exercised with a synthetic outcome.
        let ok = run_scenario(&s, Mode::FreeRun);
        assert!(matches!(ok, Outcome::Ran(_)));
        let bad = Outcome::Panicked {
            scenario: "other".into(),
            ticks: 10,
            message: "not yet implemented".into(),
        };
        assert!(!bad.is_exact());
        let t = table(&[ok, bad]);
        assert!(t.contains("KERNEL PANIC: not yet implemented"), "{t}");
        assert!(t.contains("2 scenarios"), "{t}");
    }
}
