//! Henry's Claude: offline preflight for the no-operator capability beat.
//!
//! Reads the operator files the demo will actually start with, parses them
//! with the server's own types (unknown fields rejected, as at startup), and
//! cross-checks what no single file can: suite pages inside the source
//! allowlist, finalization permissions held by a role, adoption suites that
//! exist, https tunnel URLs, and the environment the run needs. Makes no
//! network call and never prints a secret (only whether it is set).
//!
//! ```sh
//! cargo run -p genesis-server --example preflight_autonomy -- \
//!   --tunnel https://<tunnel-host> \
//!   --a-mission runtime/mission-a.json --a-constitution runtime/constitution-a.json \
//!   --a-suite runtime/suite-a.json --a-gaps runtime/gaps-a.json --a-finalize runtime/finalize-a.json \
//!   --b-mission runtime/mission-b.json --b-constitution runtime/constitution-b.json \
//!   --b-suite runtime/suite-b.json --b-adopt runtime/adopt-b.json
//! ```
//! Every flag is optional; checks that need a missing file are skipped and
//! reported as such. Exit status is 1 if anything FAILs.
use genesis_core::engine::MissionBootstrap;
use genesis_core::model::Constitution;
use genesis_server::capability_gap_adoption::GapAdoptionPolicy;
use genesis_server::capability_gap_finalizer::GapFinalizationPolicy;
use genesis_server::capability_gap_policy::GapAcquisitionPolicy;
use genesis_verification::OracleSuite;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const ACQUIRE: &str = "capability.acquire";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Pass,
    Warn,
    Fail,
    Skip,
}

#[derive(Default)]
struct Inputs {
    tunnel: Option<String>,
    a_mission: Option<MissionBootstrap>,
    a_constitution: Option<Constitution>,
    a_suites: Vec<OracleSuite>,
    a_gaps: Option<GapAcquisitionPolicy>,
    a_finalize: Option<GapFinalizationPolicy>,
    b_mission: Option<MissionBootstrap>,
    b_constitution: Option<Constitution>,
    b_suites: Vec<OracleSuite>,
    b_adopt: Option<GapAdoptionPolicy>,
    env: BTreeMap<String, bool>,
}

type Report = Vec<(Level, String)>;

fn roles_holding<'a>(mission: &'a MissionBootstrap, scopes: &BTreeSet<String>) -> Vec<&'a str> {
    mission
        .roles
        .iter()
        .filter(|r| scopes.is_subset(&r.authority.scopes))
        .map(|r| r.key.as_str())
        .collect()
}

fn host_of(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
}

fn check(i: &Inputs) -> Report {
    let mut r: Report = Vec::new();
    let mut add = |l: Level, m: String| r.push((l, m));
    let tunnel_host = i.tunnel.as_deref().and_then(host_of);
    if let Some(t) = &i.tunnel {
        if !t.starts_with("https://") || tunnel_host.is_none() {
            add(Level::Fail, format!("--tunnel {t} is not an https URL"));
        }
    }

    // ---- Org A: synthesis, then finalization ----
    match &i.a_gaps {
        None => add(Level::Skip, "A gap policy not given".into()),
        Some(gaps) => {
            match gaps.validate() {
                Ok(()) => add(
                    Level::Pass,
                    format!("A gap policy valid ({} rule(s))", gaps.rules.len()),
                ),
                Err(e) => add(Level::Fail, format!("A gap policy invalid: {e}")),
            }
            for rule in &gaps.rules {
                let id = &rule.capability_id;
                let allowed: BTreeSet<&str> = rule
                    .allowed_source_urls
                    .iter()
                    .map(String::as_str)
                    .collect();
                for url in &rule.allowed_source_urls {
                    if !url.starts_with("https://") {
                        add(Level::Fail, format!("A rule {id}: source {url} is not https (the live portal must be on the tunnel)"));
                    }
                    if let (Some(t), Some(h)) = (&tunnel_host, host_of(url)) {
                        if &h != t {
                            add(
                                Level::Fail,
                                format!("A rule {id}: source {url} is not on the tunnel host {t}"),
                            );
                        }
                    }
                }
                if !rule
                    .examples
                    .iter()
                    .any(|e| allowed.contains(e.url.as_str()))
                {
                    add(
                        Level::Fail,
                        format!("A rule {id}: no example page is inside allowed_source_urls"),
                    );
                }
                match i.a_suites.iter().find(|s| s.suite_id == rule.suite_id) {
                    None if i.a_suites.is_empty() => add(
                        Level::Skip,
                        format!("A rule {id}: no A suite given to check pages"),
                    ),
                    None => add(
                        Level::Fail,
                        format!(
                            "A rule {id}: suite {} is not among the A suites given",
                            rule.suite_id
                        ),
                    ),
                    Some(suite) => {
                        let missing: Vec<&str> = suite
                            .cases
                            .iter()
                            .map(|c| c.source.url.as_str())
                            .filter(|u| !allowed.contains(u))
                            .collect();
                        if missing.is_empty() {
                            add(
                                Level::Pass,
                                format!(
                                    "A rule {id}: all {} suite pages are allowed sources",
                                    suite.cases.len()
                                ),
                            );
                        } else {
                            add(Level::Fail, format!("A rule {id}: suite pages outside allowed_source_urls (synthesis will refuse): {}", missing.join(", ")));
                        }
                    }
                }
                match &i.a_finalize {
                    None => add(Level::Warn, format!("A rule {id}: no finalization policy, so the gap stops at ACQUIRED (no INSTALLED)")),
                    Some(fin) => match fin.rules.iter().find(|f| f.capability_id.as_str() == id) {
                        None => add(Level::Fail, format!("A rule {id}: no finalization rule with the same capability_id")),
                        Some(f) if f.suite_id != rule.suite_id => add(Level::Warn, format!("A rule {id}: finalization suite {} differs from the acquisition suite {}", f.suite_id, rule.suite_id)),
                        Some(_) => add(Level::Pass, format!("A rule {id}: finalization rule present")),
                    },
                }
            }
        }
    }
    if let Some(c) = &i.a_constitution {
        if c.scopes.contains(ACQUIRE) {
            add(Level::Pass, format!("A constitution allows {ACQUIRE}"));
        } else {
            add(
                Level::Fail,
                format!("A constitution lacks {ACQUIRE}: no role can request a capability"),
            );
        }
    }
    if let Some(fin) = &i.a_finalize {
        for f in &fin.rules {
            let id = f.capability_id.as_str();
            let mut needed = f.required_permissions.clone();
            needed.insert(ACQUIRE.into());
            if let Some(c) = &i.a_constitution {
                let outside: Vec<&String> = f
                    .required_permissions
                    .iter()
                    .filter(|p| !c.scopes.contains(*p))
                    .collect();
                if !outside.is_empty() {
                    add(
                        Level::Fail,
                        format!(
                            "A finalize {id}: permissions outside A's constitution: {outside:?}"
                        ),
                    );
                }
            }
            match &i.a_mission {
                None => add(
                    Level::Skip,
                    format!("A finalize {id}: no A mission given to check role scopes"),
                ),
                Some(m) => {
                    let holders = roles_holding(m, &needed);
                    if holders.is_empty() {
                        add(Level::Fail, format!("A finalize {id}: no starting role holds {needed:?} (the planner may create one, but the demo path assumes an owner)"));
                    } else {
                        add(
                            Level::Pass,
                            format!("A finalize {id}: role(s) {holders:?} hold {needed:?}"),
                        );
                    }
                }
            }
        }
    }

    // ---- Org B: adopt-first ----
    match &i.b_adopt {
        None => add(Level::Skip, "B adoption policy not given".into()),
        Some(adopt) => {
            let a_perms: BTreeSet<String> = i
                .a_finalize
                .iter()
                .flat_map(|f| {
                    f.rules
                        .iter()
                        .flat_map(|r| r.required_permissions.iter().cloned())
                })
                .collect();
            for rule in &adopt.rules {
                let id = rule.capability_id.as_str();
                if i.b_suites.is_empty() {
                    add(Level::Skip, format!("B adopt {id}: no B suite given"));
                } else if i.b_suites.iter().any(|s| s.suite_id == rule.suite_id) {
                    add(
                        Level::Pass,
                        format!("B adopt {id}: suite {} present", rule.suite_id),
                    );
                } else {
                    add(Level::Fail, format!("B adopt {id}: suite {} is not among the B suites given (adoption stops at configuration)", rule.suite_id));
                }
                if rule.synthesis_fallback {
                    add(Level::Warn, format!("B adopt {id}: synthesis_fallback is on; an empty search makes B synthesize (model + Actor calls), which the beat claims it doesn't"));
                }
                if let Some(c) = &i.b_constitution {
                    let outside: Vec<&String> =
                        a_perms.iter().filter(|p| !c.scopes.contains(*p)).collect();
                    if !outside.is_empty() {
                        add(Level::Fail, format!("B adopt {id}: A's manifest needs {outside:?}, outside B's constitution"));
                    }
                }
                match &i.b_mission {
                    None => add(
                        Level::Skip,
                        format!("B adopt {id}: no B mission given to check role scopes"),
                    ),
                    Some(m) if !a_perms.is_empty() => {
                        let holders = roles_holding(m, &a_perms);
                        if holders.is_empty() {
                            add(Level::Fail, format!("B adopt {id}: no starting B role holds A's manifest permissions {a_perms:?}"));
                        } else {
                            add(Level::Pass, format!("B adopt {id}: role(s) {holders:?} hold A's manifest permissions"));
                        }
                    }
                    Some(_) => {}
                }
            }
        }
    }
    if let (Some(gaps), Some(adopt)) = (&i.a_gaps, &i.b_adopt) {
        let a: BTreeSet<&str> = gaps
            .rules
            .iter()
            .map(|r| r.capability_id.as_str())
            .collect();
        let b: BTreeSet<&str> = adopt
            .rules
            .iter()
            .map(|r| r.capability_id.as_str())
            .collect();
        if a.is_disjoint(&b) {
            add(Level::Pass, format!("A requests {a:?}, B adopts under its own names {b:?} (cross-org inheritance, not a shared name)"));
        }
    }

    // ---- Environment (presence only; values never printed) ----
    for (name, set) in &i.env {
        let lvl = if *set {
            Level::Pass
        } else if name == "GENESIS_AUDIT_COMMITS" {
            Level::Warn
        } else {
            Level::Fail
        };
        add(
            lvl,
            format!("env {name}: {}", if *set { "set" } else { "missing" }),
        );
    }
    r
}

fn load<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let bytes = std::fs::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = Inputs::default();
    let mut errors = Vec::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let Some(v) = it.next() else {
            errors.push(format!("{flag} needs a value"));
            break;
        };
        let res: Result<(), String> = match flag.as_str() {
            "--tunnel" => {
                i.tunnel = Some(v.clone());
                Ok(())
            }
            "--a-mission" => load(v).map(|x| i.a_mission = Some(x)),
            "--a-constitution" => load(v).map(|x| i.a_constitution = Some(x)),
            "--a-suite" => load(v).map(|x| i.a_suites.push(x)),
            "--a-gaps" => load(v).map(|x| i.a_gaps = Some(x)),
            "--a-finalize" => load(v).map(|x| i.a_finalize = Some(x)),
            "--b-mission" => load(v).map(|x| i.b_mission = Some(x)),
            "--b-constitution" => load(v).map(|x| i.b_constitution = Some(x)),
            "--b-suite" => load(v).map(|x| i.b_suites.push(x)),
            "--b-adopt" => load(v).map(|x| i.b_adopt = Some(x)),
            other => Err(format!("unknown flag {other}")),
        };
        if let Err(e) = res {
            errors.push(e);
        }
    }
    for name in ["ANTHROPIC_API_KEY", "APIFY_TOKEN", "GENESIS_AUDIT_COMMITS"] {
        let set = std::env::var(name)
            .is_ok_and(|v| !v.is_empty() && (name != "GENESIS_AUDIT_COMMITS" || v == "1"));
        i.env.insert(name.into(), set);
    }
    let mut report: Report = errors
        .into_iter()
        .map(|e| (Level::Fail, format!("could not load: {e}")))
        .collect();
    report.extend(check(&i));
    let mut fails = 0;
    for (level, msg) in &report {
        let tag = match level {
            Level::Pass => "PASS",
            Level::Warn => "WARN",
            Level::Fail => {
                fails += 1;
                "FAIL"
            }
            Level::Skip => "skip",
        };
        println!("{tag}  {msg}");
    }
    println!("\n{fails} failure(s). This checks configuration only; it is not evidence that any live run succeeded.");
    std::process::exit(i32::from(fails > 0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn suite() -> OracleSuite {
        serde_json::from_slice(include_bytes!(
            "../../genesis-verification/examples/fixtures/trusted/suite.json"
        ))
        .unwrap()
    }

    fn good() -> Inputs {
        let s = suite();
        let urls: Vec<String> = s.cases.iter().map(|c| c.source.url.clone()).collect();
        let host = host_of(&urls[0]).unwrap();
        let mission = |scopes: Vec<&str>| -> MissionBootstrap {
            serde_json::from_value(json!({"goal":"g","roles":[{"key":"lead","name":"Lead","charter":"c",
                "authority":{"scopes":scopes,"budget_cents":0}}],"responsibilities":[{"purpose":"p","owner":"lead"}]}))
            .unwrap()
        };
        let constitution = |scopes: Vec<&str>| -> Constitution {
            serde_json::from_value(json!({"scopes":scopes,"budget_cents":0,"max_active_roles":4,"max_active_contracts":1})).unwrap()
        };
        Inputs {
            tunnel: Some(format!("https://{host}")),
            a_mission: Some(mission(vec![ACQUIRE, "capability.venue_lookup"])),
            a_constitution: Some(constitution(vec![ACQUIRE, "capability.venue_lookup"])),
            a_suites: vec![s.clone()],
            a_gaps: Some(serde_json::from_value(json!({"rules":[{"capability_id":"venue-reader","suite_id":s.suite_id,
                "actor_name_prefix":"gap-reader","allowed_source_urls":urls,
                "examples":[{"url":s.cases[0].source.url,"html":"<html>x</html>"}],"repair":false}]})).unwrap()),
            a_finalize: Some(serde_json::from_value(json!({"rules":[{"capability_id":"venue-reader","suite_id":s.suite_id,
                "intent":"i","applicability_conditions":[],"known_limitations":[],"input_schema":{},"output_schema":{},
                "required_permissions":["capability.venue_lookup"],"responsibility_template":null,"workflow_procedure":null,
                "local_configuration_schema":{}}]})).unwrap()),
            b_mission: Some(mission(vec!["capability.venue_lookup"])),
            b_constitution: Some(constitution(vec!["capability.venue_lookup"])),
            b_suites: vec![s.clone()],
            b_adopt: Some(serde_json::from_value(json!({"rules":[{"capability_id":"capacity-checker","suite_id":s.suite_id,"synthesis_fallback":false}]})).unwrap()),
            env: BTreeMap::from([("APIFY_TOKEN".into(), true)]),
        }
    }

    fn fails(r: &Report) -> Vec<&String> {
        r.iter()
            .filter(|(l, _)| *l == Level::Fail)
            .map(|(_, m)| m)
            .collect()
    }

    #[test]
    fn a_consistent_setup_has_no_failures() {
        let r = check(&good());
        assert!(fails(&r).is_empty(), "{r:#?}");
    }

    #[test]
    fn review_preflight_rejects_empty_adoption_policy_that_server_refuses() {
        let mut input = good();
        input.b_adopt.as_mut().unwrap().rules.clear();

        // Check the real startup boundary, not a duplicated local predicate.
        let mut config = genesis_server::OrgConfig::dev("review-b", "Review B");
        config.registry_url = Some("http://127.0.0.1:7100".into());
        config.oracle_suites = input.b_suites.clone();
        config.gap_adoption_policy = input.b_adopt.clone();
        let startup = genesis_server::build_org(&config, None)
            .err()
            .expect("the server must refuse an empty adoption policy before starting");
        assert!(
            startup
                .to_string()
                .contains("gap adoption policy needs 1-32 rules"),
            "unexpected startup refusal: {startup}"
        );

        let report = check(&input);
        assert!(
            !fails(&report).is_empty(),
            "preflight must fail a supplied policy that startup rejects: {report:#?}"
        );
    }

    #[test]
    fn each_silent_misconfiguration_is_a_failure() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Inputs)>)> = vec![
            (
                "suite pages outside allowed_source_urls",
                Box::new(|i| {
                    i.a_gaps.as_mut().unwrap().rules[0]
                        .allowed_source_urls
                        .truncate(1);
                    i.a_gaps.as_mut().unwrap().rules[0].examples.truncate(1);
                }),
            ),
            (
                "not on the tunnel host",
                Box::new(|i| i.tunnel = Some("https://elsewhere.example".into())),
            ),
            (
                "lacks capability.acquire",
                Box::new(|i| {
                    i.a_constitution.as_mut().unwrap().scopes.remove(ACQUIRE);
                }),
            ),
            (
                "no starting role holds",
                Box::new(|i| {
                    i.a_mission.as_mut().unwrap().roles[0]
                        .authority
                        .scopes
                        .remove("capability.venue_lookup");
                }),
            ),
            (
                "no finalization rule",
                Box::new(|i| {
                    i.a_finalize.as_mut().unwrap().rules[0].capability_id =
                        genesis_core::ids::CapabilityId::new("other")
                }),
            ),
            (
                "is not among the B suites",
                Box::new(|i| i.b_adopt.as_mut().unwrap().rules[0].suite_id = "missing".into()),
            ),
            (
                "outside B's constitution",
                Box::new(|i| {
                    i.b_constitution.as_mut().unwrap().scopes.clear();
                }),
            ),
            (
                "no starting B role holds",
                Box::new(|i| {
                    i.b_mission.as_mut().unwrap().roles[0]
                        .authority
                        .scopes
                        .clear();
                }),
            ),
            (
                "env APIFY_TOKEN: missing",
                Box::new(|i| {
                    i.env.insert("APIFY_TOKEN".into(), false);
                }),
            ),
        ];
        for (needle, breakit) in cases {
            let mut i = good();
            breakit(&mut i);
            let r = check(&i);
            assert!(
                fails(&r).iter().any(|m| m.contains(needle)),
                "{needle}: {r:#?}"
            );
        }
    }

    #[test]
    fn fallback_on_is_a_warning_not_silent() {
        let mut i = good();
        i.b_adopt.as_mut().unwrap().rules[0].synthesis_fallback = true;
        assert!(check(&i)
            .iter()
            .any(|(l, m)| *l == Level::Warn && m.contains("synthesis_fallback")));
    }
}
