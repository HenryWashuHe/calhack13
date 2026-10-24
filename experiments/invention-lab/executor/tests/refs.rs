//! Lab references, the durable store and the example plan. No processes.
use genesis_core::development_method::ResourceGrant;
use genesis_lab_eval::family::Family;
use genesis_lab_exec::store::{write_durable, Store};
use genesis_lab_exec::{example, refs, sandbox::Mode};
use std::path::Path;

fn grant() -> ResourceGrant {
    ResourceGrant {
        model_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        wall_time_ms: 10_000,
        simulation_steps: 7200,
    }
}

#[test]
fn instance_references_resolve_only_to_the_exact_public_world() {
    let r = refs::instance_ref(Family::RavineCrossing, 3);
    assert_eq!(r.id, "ravine-crossing/1/3");
    assert_eq!(refs::resolve_instance(&r).unwrap().seed, 3);
    let mut wrong = r.clone();
    wrong.sha256 = refs::sha256_hex(b"another world");
    assert!(refs::resolve_instance(&wrong).is_none());
    let mut hidden = r.clone();
    hidden.id = format!("ravine-crossing/1/{}", 1u64 << 63);
    assert!(refs::resolve_instance(&hidden).is_none());
    for id in [
        "ravine-crossing/2/3",
        "ravine-crossing/1/3/x",
        "nope/1/3",
        "ravine-crossing/1",
    ] {
        let mut bad = r.clone();
        bad.id = id.into();
        assert!(refs::resolve_instance(&bad).is_none(), "{id}");
    }
}

#[test]
fn method_packages_load_only_against_the_hosts_runtime_and_protocol() {
    use genesis_core::development_package::{MethodPackage, MethodSourceFile};
    let package = |runtime: genesis_core::development_method::ExactContentRef| {
        MethodPackage::new(
            "m".into(),
            "1".into(),
            runtime,
            refs::protocol(),
            "m.py".into(),
            vec![],
            vec![MethodSourceFile {
                path: "m.py".into(),
                content: "print(1)\n".into(),
            }],
        )
        .unwrap()
    };
    let good = package(refs::runtime("Python 3"));
    let methods = example::ExampleMethods {
        candidate: good.reference().unwrap(),
        control: refs::content("lab-method/1", "b", "1", b"b"),
    };
    let plan = example::acceptance_plan(&methods, Mode::None, "Python 3", 2, grant());
    let binding = plan.expected_binding("candidate").unwrap();
    let bytes = good.artifact_json().unwrap();
    let loaded = MethodPackage::load_for(
        bytes.as_bytes(),
        &binding,
        &refs::runtime("Python 3"),
        &refs::protocol(),
    )
    .unwrap();
    assert_eq!(loaded.entrypoint(), "m.py");
    // The host's runtime choice decides; a package can't substitute its own.
    assert!(MethodPackage::load_for(
        bytes.as_bytes(),
        &binding,
        &refs::runtime("Python 4"),
        &refs::protocol()
    )
    .is_err());
    let other = package(refs::runtime("Python 4")).artifact_json().unwrap();
    assert!(MethodPackage::load_for(
        other.as_bytes(),
        &binding,
        &refs::runtime("Python 4"),
        &refs::protocol()
    )
    .is_err());
}

#[test]
fn the_environment_digest_covers_every_runtime_source_file() {
    let covered = refs::source_files();
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src")).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        // example.rs only builds plans; it never runs during an execution.
        if name.ends_with(".rs") && name != "example.rs" {
            assert!(
                covered.contains(&format!("src/{name}").as_str()),
                "src/{name} not covered"
            );
        }
    }
    let a = refs::environment(Mode::None, "Python 3");
    assert_ne!(a, refs::environment(Mode::Namespaces, "Python 3"));
    assert_ne!(a, refs::environment(Mode::None, "Python 4"));
}

#[test]
fn the_store_writes_durably_and_refuses_mismatched_artifacts() {
    let dir = std::env::temp_dir().join(format!("lab-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    store.write("state.json", b"one").unwrap();
    store.write("state.json", b"two").unwrap();
    assert_eq!(store.read("state.json").unwrap(), b"two");
    assert!(!dir.join(".state.json.tmp").exists());
    let r = refs::content("x/1", "x", "1", b"bytes");
    assert!(store.put_artifact(&r, b"other bytes").is_err());
    store.put_artifact(&r, b"bytes").unwrap();
    assert_eq!(
        store.read(&format!("artifacts/{}", r.sha256)).unwrap(),
        b"bytes"
    );
    write_durable(&dir.join("direct"), b"z").unwrap();
    assert_eq!(std::fs::read(dir.join("direct")).unwrap(), b"z");
}

#[test]
fn the_example_plan_is_a_valid_frozen_acceptance_plan() {
    let methods = example::ExampleMethods {
        candidate: refs::content("lab-method/1", "a", "1", b"a"),
        control: refs::content("lab-method/1", "b", "1", b"b"),
    };
    let plan = example::acceptance_plan(&methods, Mode::None, "Python 3", 4, grant());
    plan.reference().unwrap();
    assert_eq!(plan.development_instances.len(), 4);
    let binding = plan.expected_binding("candidate").unwrap();
    assert_eq!(
        binding.environment,
        refs::environment(Mode::None, "Python 3")
    );
    assert_eq!(binding.evaluator, refs::evaluator());
    assert_eq!(binding.method, methods.candidate);
}

#[test]
fn a_slot_store_is_held_by_one_opener_at_a_time() {
    let dir = std::env::temp_dir().join(format!("lab-store-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let first = Store::open(&dir).unwrap();
    let second = Store::open(&dir);
    assert!(
        second.as_ref().is_err_and(genesis_lab_exec::store::is_busy),
        "second opener admitted"
    );
    // From another thread too: the lock belongs to the open file, not the thread.
    let d = dir.clone();
    let busy = std::thread::spawn(move || Store::open(&d).is_err())
        .join()
        .unwrap();
    assert!(busy);
    drop(first);
    Store::open(&dir).unwrap();
}
