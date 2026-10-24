//! The namespace sandbox's method root, probed from inside by a real method
//! process. Needs root (skipped otherwise); Linux only.
use genesis_lab_exec::sandbox::Mode;
use std::io::Read;
use std::os::unix::net::UnixListener;

const PROBE: &str = r#"
import json, os, socket, stat, sys
r = {"cwd": os.getcwd(), "uid": os.getuid(), "root": sorted(os.listdir("/"))}
r["missing"] = [p for p in ["/home", "/proc", "/run", "/var", "/root", "/etc/passwd", "/usr/local"] if os.path.exists(p)]
def writable(p):
    try:
        open(p, "w").write("x"); return True
    except OSError:
        return False
r["write_tmp"] = writable("/tmp/out.txt")
r["write_usr"] = writable("/usr/out.txt")
r["write_root"] = writable("/out.txt")
def reach(family, addr):
    s = socket.socket(family, socket.SOCK_STREAM)
    s.settimeout(1)
    try:
        s.connect(addr); return True
    except OSError:
        return False
    finally:
        s.close()
r["tcp"] = reach(socket.AF_INET, ("1.1.1.1", 443))
r["abstract_unix"] = reach(socket.AF_UNIX, "\0" + sys.argv[0].split("/")[-1] + "-genesis-probe")
r["host_unix"] = reach(socket.AF_UNIX, os.environ.get("HOST_SOCKET", "/tmp/none"))
socks = []
for d, dirs, files in os.walk("/"):
    for f in files:
        try:
            if stat.S_ISSOCK(os.lstat(os.path.join(d, f)).st_mode): socks.append(os.path.join(d, f))
        except OSError:
            pass
r["sockets"] = socks
try:
    import numpy  # noqa: F401
    r["third_party"] = True
except ImportError:
    r["third_party"] = False
r["env"] = sorted(os.environ)
print(json.dumps(r))
"#;

#[test]
fn the_method_root_holds_only_the_interpreter_image_and_the_method() {
    let uid = std::process::Command::new("id").arg("-u").output().unwrap();
    if String::from_utf8_lossy(&uid.stdout).trim() != "0" {
        eprintln!("skipped: the namespace sandbox needs root");
        return;
    }
    let stage = std::env::temp_dir().join(format!("lab-sandbox-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(stage.join("files")).unwrap();
    std::fs::write(stage.join("files/probe.py"), PROBE).unwrap();
    // Services the method must not reach: a filesystem socket on the host and
    // an abstract socket in the host's network namespace.
    let host_sock = stage.join("host.sock");
    let _fs = UnixListener::bind(&host_sock).unwrap();
    let abstract_name = b"\0probe.py-genesis-probe";
    let _abs = std::os::unix::net::UnixListener::bind_addr(
        &<std::os::unix::net::SocketAddr as std::os::linux::net::SocketAddrExt>::from_abstract_name(&abstract_name[1..]).unwrap(),
    )
    .unwrap();

    let mut cmd = Mode::Namespaces.command("python3", "probe.py", &stage);
    // A variable the setup must not pass through.
    cmd.env("HOST_SOCKET", &host_sock)
        .stderr(std::process::Stdio::inherit());
    let mut child = cmd.spawn().unwrap();
    drop(child.stdin.take());
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    assert!(child.wait().unwrap().success(), "probe failed: {out}");
    let r: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    eprintln!("{r}");

    assert_eq!(r["cwd"], "/tmp/work");
    assert_eq!(r["uid"], 65534);
    let root: Vec<&str> = r["root"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for entry in &root {
        assert!(
            ["bin", "sbin", "lib", "lib64", "usr", "etc", "dev", "tmp"].contains(entry),
            "unexpected /{entry} in the method root: {root:?}"
        );
    }
    assert_eq!(r["missing"], serde_json::json!([]), "visible host paths");
    assert_eq!(r["write_tmp"], true);
    assert_eq!(r["write_usr"], false);
    assert_eq!(r["write_root"], false);
    assert_eq!(r["tcp"], false);
    assert_eq!(r["abstract_unix"], false);
    assert_eq!(r["host_unix"], false);
    assert_eq!(r["sockets"], serde_json::json!([]));
    assert_eq!(r["third_party"], false, "host site packages are visible");
    for var in r["env"].as_array().unwrap() {
        let v = var.as_str().unwrap();
        assert!(!v.starts_with("LAB_"), "{v} leaked into the method");
    }
    assert!(Mode::Namespaces.bounds_model_use());
    assert!(!Mode::None.bounds_model_use());
    let _ = std::fs::remove_dir_all(&stage);
}
