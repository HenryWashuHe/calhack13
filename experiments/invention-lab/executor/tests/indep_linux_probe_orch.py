#!/usr/bin/env python3.14
# Independent adversarial probe harness for lab-exec (invention-lab executor).
# Drives the built `lab-exec run` binary in `namespaces` and `none` (control)
# modes, leaking fds/sockets/env into lab-exec, and checks for host-observable
# side effects (escape). A probe "passes" (sandbox holds) only if the namespaces
# run produces NO host-observable effect while the `none` control DOES.
import os, sys, socket, subprocess, time, json, shutil, threading, signal

ROOT = "/tmp/claude-0/probe"
DROP = ROOT + "/drop"
SECRET_PATH = ROOT + "/HOST_SECRET"
SECRET = b"GENESIS-HOST-SECRET-7f3a9c21-do-not-leak\n"
B = "/tmp/claude-0/target-indep/debug/lab-exec"
PORT = 38291
WALL_FAST = 8000
WALL_DEADLINE = 2500

os.makedirs(DROP, exist_ok=True)
with open(SECRET_PATH, "wb") as f:
    f.write(SECRET)

results = []

def log(name, mode, detail):
    results.append({"probe": name, "mode": mode, **detail})

def build_and_run(name, mode, method_src, wall=WALL_FAST, pass_fds=(), fd_setup=None, timeout=60):
    """Create package+plan for method_src and run one slot. fd_setup() runs in
    parent right before spawn and must place the desired fds at their numbers."""
    srcdir = f"{ROOT}/src/{name}-{mode}"
    plandir = f"{ROOT}/plan/{name}-{mode}"
    statedir = f"{ROOT}/state/{name}-{mode}"
    for d in (srcdir, plandir, statedir):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d, exist_ok=True)
    with open(f"{srcdir}/method.py", "w") as f:
        f.write(method_src)
    env = dict(os.environ)
    env["LAB_SECRET"] = "env-secret-must-not-leak"
    # package
    r = subprocess.run([B, "package", srcdir, f"probe-{name}", "0.1.0", "method.py",
                        f"{plandir}/pkg.json"], capture_output=True, text=True, env=env)
    if r.returncode != 0:
        return {"error": "package failed", "stderr": r.stderr.strip()}
    # plan (candidate = my package)
    r = subprocess.run([B, "example-plan", mode, "1", "72000", str(wall), plandir,
                        "ravine-crossing", f"{plandir}/pkg.json"],
                       capture_output=True, text=True, env=env)
    if r.returncode != 0:
        return {"error": "plan failed", "stderr": r.stderr.strip()}
    argv = [B, "run", f"{plandir}/plan.json", "candidate",
            f"{plandir}/candidate.package.json", statedir, mode]
    if fd_setup:
        fd_setup()
    try:
        p = subprocess.run(argv, capture_output=True, text=True, env=env,
                           pass_fds=pass_fds, timeout=timeout)
        out = {"rc": p.returncode}
        try:
            out["outcome"] = json.loads(p.stdout).get("outcome")
        except Exception:
            out["stdout"] = p.stdout[-300:]
            out["stderr"] = p.stderr[-300:]
        return out
    except subprocess.TimeoutExpired:
        return {"error": "run timed out"}

def clean_drop():
    shutil.rmtree(DROP, ignore_errors=True)
    os.makedirs(DROP, exist_ok=True)

def read_breach(path):
    try:
        with open(path, "rb") as f:
            return f.read()
    except Exception:
        return None

# ---------- P1: leaked directory fd (fchdir chroot escape) ----------
M_FDDIR = f'''import os,sys
try:
    os.fchdir(3)
except Exception as e:
    sys.exit(0)
try:
    data=open("HOST_SECRET","rb").read()
    open("drop/PFDDIR.breach","wb").write(b"fddir:"+data)
except Exception:
    pass
'''
def probe_fddir(mode):
    clean_drop()
    def setup():
        fd = os.open(ROOT, os.O_RDONLY | os.O_DIRECTORY)
        os.dup2(fd, 3, inheritable=True)
        if fd != 3: os.close(fd)
    out = build_and_run("fddir", mode, M_FDDIR, pass_fds=(3,), fd_setup=setup)
    b = read_breach(f"{DROP}/PFDDIR.breach")
    escaped = b is not None and SECRET in b
    log("fddir", mode, {"run": out, "breach": b.decode(errors='replace') if b else None,
                         "escaped": escaped})

# ---------- P2: leaked readable+writable fds ----------
M_FDFILE = '''import os
try:
    data=os.read(3,4096)
    os.write(4,b"fdfile:"+data)
except Exception:
    pass
'''
def probe_fdfile(mode):
    outpath = f"{DROP}/PFDFILE.out"
    clean_drop()
    def setup():
        rfd = os.open(SECRET_PATH, os.O_RDONLY)
        wfd = os.open(outpath, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
        os.dup2(rfd, 3, inheritable=True); os.dup2(wfd, 4, inheritable=True)
        if rfd != 3: os.close(rfd)
        if wfd != 4: os.close(wfd)
    out = build_and_run("fdfile", mode, M_FDFILE, pass_fds=(3, 4), fd_setup=setup)
    b = read_breach(outpath)
    escaped = b is not None and SECRET in b
    log("fdfile", mode, {"run": out, "breach": b.decode(errors='replace') if b else None,
                         "escaped": escaped})

# ---------- P3: leaked unix socketpair fd ----------
M_FDSOCK = '''import os
try:
    os.write(3,b"fdsock-token-reached-host")
except Exception:
    pass
'''
def probe_fdsock(mode):
    a, b = socket.socketpair()
    got = {"data": None}
    def reader():
        try:
            b.settimeout(15)
            got["data"] = b.recv(64)
        except Exception:
            got["data"] = None
    th = threading.Thread(target=reader); th.start()
    def setup():
        os.dup2(a.fileno(), 3, inheritable=True)
    out = build_and_run("fdsock", mode, M_FDSOCK, pass_fds=(3,), fd_setup=setup)
    th.join(timeout=16)
    a.close(); b.close()
    escaped = got["data"] == b"fdsock-token-reached-host"
    log("fdsock", mode, {"run": out, "socket_received": got["data"].decode() if got["data"] else None,
                         "escaped": escaped})

# ---------- P4: /proc/self/root and /proc/1/root chroot escape ----------
M_PROC = f'''import os
S="{SECRET_PATH}"
D="{DROP}"
for pre in ("","/proc/self/root","/proc/1/root"):
    try:
        data=open(pre+S,"rb").read()
        open(D+"/PPROC.breach","ab").write(pre.encode()+b":"+data+b"\\n")
    except Exception:
        pass
'''
def probe_proc(mode):
    clean_drop()
    out = build_and_run("proc", mode, M_PROC)
    b = read_breach(f"{DROP}/PPROC.breach")
    escaped = b is not None and SECRET in b
    log("proc", mode, {"run": out, "breach": b.decode(errors='replace') if b else None,
                       "escaped": escaped})

# ---------- P5: unshare(CLONE_NEWUSER)+chroot escape, setns, mount ----------
M_NS = f'''import os,ctypes,ctypes.util
D="{DROP}"
res=[]
libc=ctypes.CDLL(ctypes.util.find_library("c") or "libc.so.6",use_errno=True)
CLONE_NEWUSER=0x10000000; CLONE_NEWNS=0x00020000; CLONE_NEWPID=0x20000000
for name,flag in (("user",CLONE_NEWUSER),("mount",CLONE_NEWNS),("pid",CLONE_NEWPID)):
    rc=libc.unshare(flag)
    res.append(name+"="+("ok" if rc==0 else "errno"+str(ctypes.get_errno())))
# try mount tmpfs over / (needs CAP_SYS_ADMIN)
rc=libc.mount(b"none",b"/",b"tmpfs",0,None)
res.append("mount="+("ok" if rc==0 else "errno"+str(ctypes.get_errno())))
# if a user namespace was gained, chroot escape may now be possible
try:
    data=open("{SECRET_PATH}","rb").read()
    open(D+"/PNS.breach","wb").write(b"ns:"+data)
    res.append("secret-read")
except Exception:
    pass
try:
    open(D+"/PNS.result","w").write(",".join(res))
except Exception:
    pass
'''
def probe_ns(mode):
    clean_drop()
    out = build_and_run("ns", mode, M_NS)
    res = read_breach(f"{DROP}/PNS.result")
    breach = read_breach(f"{DROP}/PNS.breach")
    escaped = breach is not None and SECRET in breach
    log("ns", mode, {"run": out, "result": res.decode() if res else None,
                     "escaped": escaped})

# ---------- P6: reach host loopback TCP + abstract unix socket by name ----------
M_NET = f'''import socket
D="{DROP}"
ok=[]
try:
    s=socket.socket(); s.settimeout(3); s.connect(("127.0.0.1",{PORT})); s.sendall(b"tcp-probe"); ok.append("tcp"); s.close()
except Exception: pass
try:
    a=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); a.settimeout(3); a.connect("\\0genesisprobe"); a.sendall(b"abs-probe"); ok.append("abs"); a.close()
except Exception: pass
try:
    open(D+"/PNET.breach","w").write(",".join(ok))
except Exception: pass
'''
def probe_net(mode):
    clean_drop()
    hits = {"tcp": False, "abs": False}
    stop = threading.Event()
    def tcp_srv():
        s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind(("127.0.0.1", PORT)); s.listen(5); s.settimeout(0.5)
        while not stop.is_set():
            try:
                c, _ = s.accept(); hits["tcp"] = True; c.close()
            except socket.timeout:
                pass
            except Exception:
                break
        s.close()
    def abs_srv():
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.bind("\0genesisprobe"); s.listen(5); s.settimeout(0.5)
        while not stop.is_set():
            try:
                c, _ = s.accept(); hits["abs"] = True; c.close()
            except socket.timeout:
                pass
            except Exception:
                break
        s.close()
    t1 = threading.Thread(target=tcp_srv); t2 = threading.Thread(target=abs_srv)
    t1.start(); t2.start(); time.sleep(0.3)
    out = build_and_run("net", mode, M_NET)
    time.sleep(0.3); stop.set(); t1.join(2); t2.join(2)
    escaped = hits["tcp"] or hits["abs"]
    log("net", mode, {"run": out, "listener_hits": dict(hits),
                      "breach_file": (read_breach(f"{DROP}/PNET.breach") or b"").decode(errors='replace'),
                      "escaped": escaped})

# ---------- P7: write outside allowed area (absolute host paths) ----------
M_WRITE = f'''import os
D="{DROP}"
S=open("{SECRET_PATH}","rb").read() if os.path.exists("{SECRET_PATH}") else b"x"
targets=[D+"/PWRITE_drop.breach","/PWRITE_root.breach","/tmp/PWRITE_hosttmp.breach"]
for p in targets:
    try:
        open(p,"wb").write(b"write:"+S)
    except Exception: pass
'''
def probe_write(mode):
    clean_drop()
    for p in ("/PWRITE_root.breach", "/tmp/PWRITE_hosttmp.breach"):
        try: os.remove(p)
        except Exception: pass
    out = build_and_run("write", mode, M_WRITE)
    found = {}
    for p in (f"{DROP}/PWRITE_drop.breach", "/PWRITE_root.breach", "/tmp/PWRITE_hosttmp.breach"):
        b = read_breach(p)
        found[p] = (b is not None and SECRET in b)
    for p in ("/PWRITE_root.breach", "/tmp/PWRITE_hosttmp.breach"):
        try: os.remove(p)
        except Exception: pass
    escaped = any(found.values())
    log("write", mode, {"run": out, "host_writes": found, "escaped": escaped})

# ---------- P8: env var leak (LAB_SECRET set in lab-exec environment) ----------
# exfil via leaked writable fd 4 so it works in both modes if env leaks.
M_ENV = '''import os
try:
    env=";".join(f"{k}={v}" for k,v in sorted(os.environ.items()))
    os.write(4,env.encode()[:3000])
except Exception:
    pass
'''
def probe_env(mode):
    outpath = f"{DROP}/PENV.out"
    clean_drop()
    def setup():
        wfd = os.open(outpath, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
        os.dup2(wfd, 4, inheritable=True)
        if wfd != 4: os.close(wfd)
    out = build_and_run("env", mode, M_ENV, pass_fds=(4,), fd_setup=setup)
    b = read_breach(outpath)
    env_text = b.decode(errors='replace') if b else None
    lab_secret_leaked = env_text is not None and "LAB_SECRET" in env_text
    log("env", mode, {"run": out, "child_env": env_text, "LAB_SECRET_leaked": lab_secret_leaked})

# ---------- P9/P10: detached daemon lifecycle (normal exit + deadline kill) ----------
def daemon_src(beat, pidfile, main_mode):
    # main_mode: "exit" -> parent exits fast; "loop" -> parent loops to hit deadline
    main_body = "os._exit(0)" if main_mode == "exit" else "time.sleep(3600)"
    return f'''import os,sys,time,signal
BEAT="{beat}"; PIDF="{pidfile}"
pid=os.fork()
if pid>0:
    {main_body}
    sys.exit(0)
os.setsid()
if os.fork()>0:
    os._exit(0)
for s in (signal.SIGTERM,signal.SIGHUP,signal.SIGINT):
    try: signal.signal(s,signal.SIG_IGN)
    except Exception: pass
try: open(PIDF,"w").write(str(os.getpid()))
except Exception: pass
for fd in (0,1,2):
    try: os.close(fd)
    except Exception: pass
t0=time.time()
while time.time()-t0<30:
    try: open(BEAT,"a").write("x")
    except Exception: pass
    time.sleep(0.1)
os._exit(0)
'''

def kill_pidfile(pidfile):
    try:
        pid = int(open(pidfile).read().strip())
        os.kill(pid, signal.SIGKILL)
        return pid
    except Exception:
        return None

def probe_daemon(mode, main_mode, label):
    clean_drop()
    beat = f"{DROP}/{label}.beat"; pidfile = f"{DROP}/{label}.pid"
    wall = WALL_DEADLINE if main_mode == "loop" else WALL_FAST
    nobody_before = count_nobody()
    out = build_and_run("daemon-" + label, mode, daemon_src(beat, pidfile, main_mode),
                        wall=wall, timeout=40)
    time.sleep(1.0)
    size1 = os.path.getsize(beat) if os.path.exists(beat) else -1
    time.sleep(0.6)
    size2 = os.path.getsize(beat) if os.path.exists(beat) else -1
    nobody_after = count_nobody()
    survived = (size2 > size1 >= 0)  # beat file still growing after run returned
    killed_pid = kill_pidfile(pidfile)
    log("daemon-" + label, mode, {"run": out, "beat_size_t1": size1, "beat_size_t2": size2,
                                  "daemon_survived": survived, "killed_leftover_pid": killed_pid,
                                  "nobody_before": nobody_before, "nobody_after": nobody_after})

def count_nobody():
    try:
        r = subprocess.run(["ps", "-u", "nobody", "-o", "pid="], capture_output=True, text=True)
        return len([x for x in r.stdout.split() if x.strip()])
    except Exception:
        return -1

def main():
    only = sys.argv[1] if len(sys.argv) > 1 else "all"
    modes = ["namespaces", "none"]
    for mode in modes:
        if only in ("all", "daemon"):
            probe_daemon(mode, "exit", "dexit")
            probe_daemon(mode, "loop", "ddead")
            if only == "daemon":
                continue
        probe_fddir(mode)
        probe_fdfile(mode)
        probe_fdsock(mode)
        probe_proc(mode)
        probe_ns(mode)
        probe_net(mode)
        probe_write(mode)
        probe_env(mode)
    print(json.dumps(results, indent=1))
    # final leftover check
    leftover = subprocess.run(["ps", "-u", "nobody", "-o", "pid,comm,args"],
                              capture_output=True, text=True).stdout
    sys.stderr.write("=== FINAL ps -u nobody ===\n" + leftover + "\n")

if __name__ == "__main__":
    main()
