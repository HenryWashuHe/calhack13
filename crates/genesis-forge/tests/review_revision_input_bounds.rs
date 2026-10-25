//! Exact-boundary checks for the revision bundle's two size limits. The existing
//! cases only exercise inputs far past each limit, so `>` could become `>=`
//! (refusing a bundle exactly at the documented limit) without any test
//! failing. Independent review by Henry's Claude; pure local checks only.
use genesis_core::development_package::{MethodPackage, MethodSourceFile};
use genesis_forge::method_revision::{MethodRevisionInput, MethodRevisionSpec};
#[path = "common/method_plan.rs"]
mod plan_fixture;

const PARENT_LIMIT: usize = 128 * 1024;

fn parent_of_len(target: usize) -> (MethodRevisionSpec, String) {
    let mut generation = plan_fixture::plan();
    generation.org_id = "org_b".into();
    generation.target_version = "2".into();
    let build = |pad: usize| {
        MethodPackage::new(
            "parent".into(),
            "1".into(),
            generation.runtime.clone(),
            generation.protocol.reference.clone(),
            "parent.py".into(),
            vec![],
            vec![MethodSourceFile {
                path: "parent.py".into(),
                content: format!("# {}\n", "x".repeat(pad)),
            }],
        )
        .unwrap()
    };
    let base = build(0).artifact_json().unwrap().len();
    let package = build(target - base);
    let json = package.artifact_json().unwrap();
    assert_eq!(json.len(), target);
    (
        MethodRevisionSpec {
            generation: generation.clone(),
            parent: package.reference().unwrap(),
        },
        json,
    )
}

#[test]
fn a_parent_exactly_at_the_limit_is_accepted_and_one_byte_more_is_not() {
    let (spec, json) = parent_of_len(PARENT_LIMIT);
    let at = MethodRevisionInput::new(spec, json).expect("exactly 128 KiB parent");
    at.validate().unwrap();
    at.prepare().unwrap();
    let (spec, json) = parent_of_len(PARENT_LIMIT + 1);
    assert!(MethodRevisionInput::new(spec, json).is_err());
}

// The 1 MiB encoded-bundle check (line 46) is not independently reachable: the
// plan inside is already capped at 256 KiB encoded and the parent at 128 KiB raw
// (at most about 2x after re-escaping), so every bundle that passes the later
// plan check is far below 1 MiB. The existing "large-input" case is refused by
// the 16 KiB instruction cap as well. No exact-boundary test is possible there.
