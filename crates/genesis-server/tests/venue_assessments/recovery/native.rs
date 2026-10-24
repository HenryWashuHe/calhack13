//! Explicit opt-in composition using two real Python processes and native SDK
//! signed envelopes. Apify/CREAO and initial provisioning remain the parent
//! loopback fixtures; this is not cloud registration or live Apify/CREAO execution.
//!
//! Setup from the repository root (no provider credentials):
//! `uv sync --project services/fetch-bridge --frozen --python 3.12`
//! Run after the normal Rust test build:
//! `GENESIS_FETCH_TEST_PYTHON="$PWD/services/fetch-bridge/.venv/bin/python" cargo test -p genesis-server --test venue_assessments recovery::native::signed_specialist_selection_completes_real_recovery_and_retires -- --ignored --exact`
//! The automatic-intake variant is
//! `recovery::native::automatic_intake_completes_signed_recovery_and_retires`.
//! It explicitly drains the real intake worker; it does not test timer scheduling.
//! The interpreter must be absolute. Missing setup fails an explicit run; the
//! ordinary Cargo suite ignores this test. PYTHONPATH pins the source checkout,
//! even when an installed editable package points at a different worktree.

use super::*;
use genesis_server::venue_recovery_intake;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::{
    fs::File,
    io::Read,
    net::TcpListener as PortReservation,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Instant,
};

struct Process(Option<Child>);
impl Process {
    fn spawn(command: &mut Command) -> Self {
        Self(Some(
            command
                .spawn()
                .expect("could not start the configured Python interpreter"),
        ))
    }
    fn assert_running(&mut self) {
        assert!(
            self.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "native bridge process exited; check the frozen Python installation and loopback port"
        );
    }
    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // No shell or grandchildren: kill the owned interpreter, then reap
            // it. This also runs on assertion failure and timeout cancellation.
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

struct NativePeer {
    process: Option<Process>,
    python: PathBuf,
    project: PathBuf,
    seed: String,
    token: String,
    address: String,
    db: PathBuf,
    peers: PathBuf,
    port: u16,
    specialist: bool,
}
impl NativePeer {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.python);
        command
            .env_clear()
            .env("PYTHONPATH", &self.project)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("GENESIS_FETCH_SEED", &self.seed)
            .env("GENESIS_FETCH_BRIDGE_TOKEN", &self.token)
            .current_dir(&self.project)
            .args(["-B", "-m", "genesis_fetch_bridge"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    async fn identity(&mut self) {
        // A regular temporary file avoids an undrained stdout pipe. It contains
        // only the public address, never the environment-only seed/token.
        let mut output = tempfile::tempfile().unwrap();
        let mut command = self.command();
        command.arg("identity").stdout(output.try_clone().unwrap());
        let mut process = Process::spawn(&mut command);
        let limit = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = process.0.as_mut().unwrap().try_wait().unwrap() {
                assert!(
                    status.success(),
                    "native identity command failed; install the frozen Python dependencies"
                );
                break;
            }
            assert!(Instant::now() < limit, "native identity command timed out");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        use std::io::{Seek, SeekFrom};
        output.seek(SeekFrom::Start(0)).unwrap();
        let mut text = String::new();
        output.take(256).read_to_string(&mut text).unwrap();
        self.address = text.trim().to_owned();
        assert!(
            genesis_fetch::protocol::address(&self.address),
            "identity command did not return a native address"
        );
    }
    async fn start(&mut self) {
        assert!(self.process.is_none());
        let mut command = self.command();
        command
            .arg("serve")
            .arg("--db")
            .arg(&self.db)
            .arg("--peers")
            .arg(&self.peers)
            .arg("--port")
            .arg(self.port.to_string());
        if self.specialist {
            command.args(["--specialist-mode", "venue_selection"]);
        }
        // Install the RAII owner before any readiness request/assertion.
        self.process = Some(Process::spawn(&mut command));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .no_proxy()
            .build()
            .unwrap();
        let limit = Instant::now() + Duration::from_secs(15);
        loop {
            self.process.as_mut().unwrap().assert_running();
            match client
                .get(format!("{}/v1/identity", self.base()))
                .bearer_auth(&self.token)
                .send()
                .await
            {
                Ok(response) if response.status() == StatusCode::OK => {
                    let identity: Value = response.json().await.unwrap();
                    assert_eq!(identity["address"], self.address);
                    assert_eq!(identity["protocol_digest"], PROTOCOL_DIGEST);
                    return;
                }
                _ => {}
            }
            assert!(Instant::now() < limit, "native bridge startup timed out");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    fn stop(&mut self) {
        self.process.take();
    }
    fn db(&self) -> Connection {
        let db = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        db.busy_timeout(Duration::from_secs(1)).unwrap();
        db
    }
}

struct NativePair {
    // Drop processes before their temporary journals and peer configuration.
    org: NativePeer,
    specialist: NativePeer,
    _directory: tempfile::TempDir,
}
impl NativePair {
    async fn start() -> Self {
        let python = PathBuf::from(std::env::var_os("GENESIS_FETCH_TEST_PYTHON")
            .expect("explicit test requires GENESIS_FETCH_TEST_PYTHON; run uv sync --project services/fetch-bridge --frozen --python 3.12"));
        assert!(
            python.is_absolute() && python.is_file(),
            "GENESIS_FETCH_TEST_PYTHON must name an absolute installed interpreter"
        );
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../services/fetch-bridge")
            .canonicalize()
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        // Reserve both simultaneously, so OS ephemeral-port reuse cannot give
        // the organization and specialist the same port.
        let ports = [
            PortReservation::bind("127.0.0.1:0").unwrap(),
            PortReservation::bind("127.0.0.1:0").unwrap(),
        ];
        let peer = |index: usize| NativePeer {
            process: None,
            python: python.clone(),
            project: project.clone(),
            seed: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            address: String::new(),
            db: directory.path().join(format!("native-{index}.sqlite")),
            peers: directory.path().join(format!("peers-{index}.json")),
            port: ports[index].local_addr().unwrap().port(),
            specialist: index == 1,
        };
        let mut org = peer(0);
        let mut specialist = peer(1);
        org.identity().await;
        specialist.identity().await;
        assert_ne!(org.address, specialist.address);
        serde_json::to_writer(
            File::create(&org.peers).unwrap(),
            &BTreeMap::from([(
                specialist.address.clone(),
                format!("{}/submit", specialist.base()),
            )]),
        )
        .unwrap();
        serde_json::to_writer(
            File::create(&specialist.peers).unwrap(),
            &BTreeMap::from([(org.address.clone(), format!("{}/submit", org.base()))]),
        )
        .unwrap();
        drop(ports);
        org.start().await;
        specialist.start().await;
        Self {
            org,
            specialist,
            _directory: directory,
        }
    }
    async fn attach(&self, fixture: &mut Fixture) -> Arc<FetchStore> {
        fixture.server.stop().await;
        let journal = Arc::new(
            FetchStore::open(
                fixture.directory.path().join("fetch-recovery.sqlite"),
                ORG,
                &self.org.address,
            )
            .unwrap(),
        );
        let client = BridgeClient::new(
            &format!("{}/v1", self.org.base()),
            &self.org.token,
            &self.org.address,
            PROTOCOL_DIGEST,
            ClientLimits {
                timeout: Duration::from_secs(3),
                ..Default::default()
            },
        )
        .unwrap();
        fixture.app.fetch_recovery = Some(Arc::new(
            FetchRecoveryRuntime::new(client, journal.clone(), self.specialist.address.clone())
                .unwrap(),
        ));
        fixture.server = Server::org(fixture.app.clone(), false).await;
        journal
    }
}

fn message_from(db: &Connection, incoming: bool, id: &str) -> (String, Message) {
    let sql = if incoming {
        "SELECT sender,payload FROM inbox WHERE message_id=?1"
    } else {
        "SELECT recipient,payload FROM outbox WHERE message_id=?1"
    };
    let (peer, payload): (String, String) = db
        .query_row(sql, [id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    let message = Message::decode(payload.as_bytes()).unwrap();
    assert_eq!(message.message_id, id);
    (peer, message)
}

fn assert_native_exchange(
    pair: &NativePair,
    journal: &FetchStore,
    record: &Value,
) -> genesis_fetch::TaskReceipt {
    let org = pair.org.db();
    let specialist = pair.specialist.db();
    let mut sent = Vec::new();
    for field in ["recruit_message_id", "accept_message_id", "task_message_id"] {
        let id = record[field].as_str().unwrap();
        let (recipient, request) = message_from(&org, false, id);
        let (sender, received) = message_from(&specialist, true, id);
        assert_eq!(recipient, pair.specialist.address);
        assert_eq!(sender, pair.org.address);
        assert_eq!(
            request.canonical_sha256().unwrap(),
            received.canonical_sha256().unwrap()
        );
        sent.push(request);
    }
    let receipt = journal
        .task_receipt(&pair.specialist.address, &sent[0], &sent[1], &sent[2])
        .unwrap();
    assert_eq!(record["receipt_sha256"], receipt.receipt_sha256());
    assert_eq!(receipt.recruitment().bridge_address(), pair.org.address);
    assert_eq!(
        receipt.recruitment().peer_address(),
        pair.specialist.address
    );
    assert_eq!(receipt.recruitment().org_id(), ORG);
    assert_eq!(
        receipt.recruitment().contract_id(),
        record["contract_id"].as_str().unwrap()
    );
    for (id, expected_hash) in [
        (
            receipt.recruitment().offer_message_id(),
            receipt.recruitment().offer_sha256(),
        ),
        (
            receipt.recruitment().accepted_message_id(),
            receipt.recruitment().accepted_sha256(),
        ),
    ] {
        let (recipient, sent) = message_from(&specialist, false, id);
        let (sender, received) = message_from(&org, true, id);
        assert_eq!(recipient, pair.org.address);
        assert_eq!(sender, pair.specialist.address);
        assert_eq!(sent.canonical_sha256().unwrap(), expected_hash);
        assert_eq!(received.canonical_sha256().unwrap(), expected_hash);
    }
    let (recipient, result) = message_from(&specialist, false, receipt.result_message_id());
    let (sender, received) = message_from(&org, true, receipt.result_message_id());
    // The actual bridge verified native envelope signatures before these inbox
    // rows were committed. It retains sender/payload, not the raw signature.
    assert_eq!(recipient, pair.org.address);
    assert_eq!(sender, pair.specialist.address);
    assert_eq!(
        result.canonical_sha256().unwrap(),
        received.canonical_sha256().unwrap()
    );
    assert_eq!(result.canonical_sha256().unwrap(), receipt.result_sha256());
    let FetchBody::TaskResult {
        task_request_id,
        job_id,
        attempt_id,
        epoch,
        output,
    } = &result.body
    else {
        panic!("native result has wrong message kind")
    };
    assert_eq!(task_request_id, receipt.task_request_id());
    assert_eq!(job_id, receipt.job_id());
    assert_eq!(attempt_id, receipt.attempt_id());
    assert_eq!(*epoch, receipt.epoch());
    assert_eq!(json_sha256(output), receipt.output_sha256());
    assert_eq!(output["selected"]["venue_id"], "replacement-hall");
    assert!(output.get("synthetic").is_none());
    let FetchBody::TaskRequest { task } = &sent[2].body else {
        panic!("native request has wrong message kind")
    };
    let saved: (String,String,String,String) = specialist.query_row(
        "SELECT job_id,attempt_id,idempotency_key,task FROM specialist_tasks WHERE org_id=?1 AND contract_id=?2",
        rusqlite::params![ORG,record["contract_id"].as_str().unwrap()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
    assert_eq!(
        (&saved.0, &saved.1, &saved.2),
        (&task.job_id, &task.attempt_id, &task.idempotency_key)
    );
    assert_eq!(
        json_sha256(&decode_json::<Value>(saved.3.as_bytes()).unwrap()),
        json_sha256(&json!(task))
    );
    let tasks: u64 = specialist
        .query_row("SELECT count(*) FROM specialist_tasks", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(tasks, 1);
    for (db, table) in [(&org, "inbox"), (&specialist, "outbox")] {
        let count: u64 = db.query_row(&format!("SELECT count(*) FROM {table} WHERE json_extract(payload,'$.body.kind')='task_result'"), [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1);
    }
    receipt
}

enum RecoveryEntry {
    Manual(Value),
    Automatic(Value),
}

async fn step_recovery(fixture: &Fixture, path: &str, entry: &RecoveryEntry) -> Value {
    match entry {
        RecoveryEntry::Manual(_) => step(fixture, path).await,
        RecoveryEntry::Automatic(linked) => {
            // Advance the same coordinator used by its worker, without an
            // operator HTTP POST. Scheduling remains explicit in this test.
            let record = genesis_server::venue_recovery::step(
                &fixture.app,
                linked["recovery_id"].as_str().unwrap(),
            )
            .await
            .unwrap();
            serde_json::to_value(record).unwrap()
        }
    }
}

fn recovery_count(fixture: &Fixture) -> u64 {
    sqlite(fixture)
        .query_row("SELECT COUNT(*) FROM venue_recoveries", [], |row| {
            row.get(0)
        })
        .unwrap()
}

async fn intake_record(fixture: &Fixture, assessment_id: &Value) -> Option<Value> {
    fixture
        .get("/venue-recovery-intakes")
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|record| &record["assessment_id"] == assessment_id)
        .cloned()
}

async fn until_intake(fixture: &Fixture, assessment_id: &Value, phase: &str) -> Value {
    let mut last = None;
    for _ in 0..256 {
        venue_recovery_intake::drain_once(&fixture.app)
            .await
            .unwrap();
        last = intake_record(fixture, assessment_id).await;
        if let Some(record) = &last {
            if record["phase"] == phase {
                return record.clone();
            }
        }
    }
    panic!("native intake did not reach {phase}: {last:?}");
}

async fn assert_entry_replay(
    fixture: &Fixture,
    path: &str,
    expected: &Value,
    entry: &RecoveryEntry,
) {
    match entry {
        RecoveryEntry::Manual(body) => {
            assert_eq!(
                fixture.post("/venue-recoveries", body.clone()).await,
                (StatusCode::OK, expected.clone())
            );
        }
        RecoveryEntry::Automatic(linked) => {
            // The automatic path never issues a recovery POST.
            // Repeated intake drains must preserve its original coordinator.
            for _ in 0..16 {
                venue_recovery_intake::drain_once(&fixture.app)
                    .await
                    .unwrap();
            }
            assert_eq!(
                intake_record(fixture, &linked["assessment_id"]).await,
                Some(linked.clone())
            );
            assert_eq!(recovery_count(fixture), 1);
            assert_eq!(fixture.get(path).await, *expected);
        }
    }
}

async fn run_native_recovery(automatic_intake: bool) {
    let mut pair = NativePair::start().await;
    let mut fixture = Fixture::start().await;
    let mut journal = pair.attach(&mut fixture).await;
    let (source, reopened) = reopened(&fixture).await;
    fixture
        .mutate(|state| state.constitution.max_active_contracts = 1)
        .await;
    let (path, registered, entry) = if automatic_intake {
        fixture.app.automatic_venue_recovery = true;
        let prepared = until_intake(&fixture, &reopened["assessment_id"], "prepared").await;
        assert_eq!(prepared["candidate_job_id"], source);
        assert!(prepared["client_request_id"]
            .as_str()
            .unwrap()
            .starts_with("venue-recovery-auto:"));
        assert!(prepared["deadline"].is_string());
        assert!(prepared["recovery_id"].is_null());
        assert_eq!(recovery_count(&fixture), 0);
        assert_eq!(fixture.app.current_state().unwrap().active_contracts(), 0);
        let outgoing: u64 = pair
            .org
            .db()
            .query_row("SELECT count(*) FROM outbox", [], |row| row.get(0))
            .unwrap();
        assert_eq!(outgoing, 0, "prepared intake has not recruited a peer");

        // Restart at the durable intent boundary, before the first recovery
        // exists. Re-enable the explicit opt-in and restore the same bridge.
        fixture.restart(false).await;
        assert!(!fixture.app.automatic_venue_recovery);
        assert!(fixture.app.fetch_recovery.is_none());
        fixture.app.automatic_venue_recovery = true;
        journal = pair.attach(&mut fixture).await;
        assert_eq!(
            intake_record(&fixture, &reopened["assessment_id"]).await,
            Some(prepared.clone())
        );
        let newer = fixture.source("native-intake-after-preparation", 3).await;
        assert_ne!(newer, source);
        let linked = until_intake(&fixture, &reopened["assessment_id"], "linked").await;
        for field in ["client_request_id", "candidate_job_id", "deadline"] {
            assert_eq!(linked[field], prepared[field]);
        }
        assert_eq!(linked["externally_managed"], false);
        assert!(linked["attention"].is_null());
        assert_eq!(recovery_count(&fixture), 1);
        let path = format!(
            "/venue-recoveries/{}",
            linked["recovery_id"].as_str().unwrap()
        );
        let registered = fixture.get(&path).await;
        (path, registered, RecoveryEntry::Automatic(linked))
    } else {
        let body = recovery_request(&source, &reopened);
        let (path, registered) = register(&fixture, body.clone()).await;
        (path, registered, RecoveryEntry::Manual(body))
    };
    assert_eq!(fixture.app.current_state().unwrap().active_contracts(), 1);
    assert_entry_replay(&fixture, &path, &registered, &entry).await;
    let limit = Instant::now() + Duration::from_secs(45);
    let selected = loop {
        let record = step_recovery(&fixture, &path, &entry).await;
        assert!(record["attention"].is_null(), "{record}");
        if record["phase"] == "assessing" {
            break record;
        }
        assert!(
            Instant::now() < limit,
            "native Fetch exchange did not produce a verified selection"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(selected["assessment_id"].is_null());
    let original_receipt = assert_native_exchange(&pair, &journal, &selected);
    let job = fixture
        .app
        .store
        .get_job(&JobId::new(registered["job_id"].as_str().unwrap()))
        .unwrap();
    assert_eq!(job.status, JobStatus::Succeeded);
    assert_eq!(original_receipt.job_id(), job.job_id.as_str());
    assert_eq!(original_receipt.attempt_id(), job.attempt_id.as_str());
    assert_eq!(original_receipt.assignment_id(), job.assignment_id.as_str());
    assert_eq!(original_receipt.epoch(), job.epoch);
    if let RecoveryEntry::Automatic(linked) = &entry {
        assert_eq!(
            job.deadline,
            Some(serde_json::from_value(linked["deadline"].clone()).unwrap())
        );
    }

    // Restart both signed transports and the Rust organization/private journal
    // at the durable selection boundary, before creating its CREAO child.
    // Fixture::restart owns shutdown of its current server/JoinHandle.
    pair.org.stop();
    pair.specialist.stop();
    fixture.restart(false).await;
    fixture.app.automatic_venue_recovery = automatic_intake;
    pair.org.start().await;
    pair.specialist.start().await;
    let journal = pair.attach(&mut fixture).await;
    assert_eq!(fixture.get(&path).await, selected);
    assert_eq!(
        assert_native_exchange(&pair, &journal, &selected),
        original_receipt
    );
    assert_entry_replay(&fixture, &path, &selected, &entry).await;
    let prepared = step_recovery(&fixture, &path, &entry).await;
    assert!(prepared["attention"].is_null(), "{prepared}");
    let child_path = format!(
        "/venue-assessments/{}",
        prepared["assessment_id"].as_str().unwrap()
    );
    let child = fixture.get(&child_path).await;
    assert_eq!(child["responsibility_id"], registered["responsibility_id"]);
    assert_eq!(child["source_job_id"], source);
    assert_eq!(child["policy"]["selected_venue_id"], "replacement-hall");
    fixture.collected(&child).await;
    assert!(fixture.get(&child_path).await["proof"].is_null());
    let accepted = fixture.verify(&child).await;
    assert_eq!(proof(&accepted)["decision"], "accepted");
    assert_eq!(proof(&accepted)["output"]["verdict"], "feasible");
    assert_eq!(proof(&accepted)["provider_agent_id"], AGENT);

    // Deliberately stop the peer: queue ACK and local retirement must not be
    // mislabeled as proof that the independent specialist has released work.
    pair.specialist.stop();
    let completed = step_recovery(&fixture, &path, &entry).await;
    assert_eq!(completed["phase"], "completed");
    assert_eq!(completed["retirement"]["local_contract_state"], "RELEASED");
    assert_eq!(completed["retirement"]["delivery"], "bridge_queued");
    assert_eq!(fixture.app.current_state().unwrap().active_contracts(), 0);
    let release_id = completed["retirement"]["release_message_id"]
        .as_str()
        .unwrap();
    let (recipient, release) = message_from(&pair.org.db(), false, release_id);
    assert_eq!(recipient, pair.specialist.address);
    assert!(matches!(release.body, FetchBody::ReleaseContract { .. }));
    assert_eq!(
        journal
            .outgoing_queued(&pair.specialist.address, &release)
            .unwrap(),
        Some(true)
    );
    let peer_state: String = pair
        .specialist
        .db()
        .query_row(
            "SELECT state FROM specialist_contracts WHERE org_id=?1 AND contract_id=?2",
            rusqlite::params![ORG, registered["contract_id"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(peer_state, "active");

    pair.specialist.start().await;
    let limit = Instant::now() + Duration::from_secs(20);
    loop {
        let reply: Option<String> = pair.org.db().query_row("SELECT payload FROM inbox WHERE json_extract(payload,'$.body.kind')='contract_released' AND json_extract(payload,'$.body.release_message_id')=?1", [release_id], |row|row.get(0)).optional().unwrap();
        if let Some(payload) = reply {
            let reply = Message::decode(payload.as_bytes()).unwrap();
            let (sender, received) = message_from(&pair.org.db(), true, &reply.message_id);
            let (recipient, sent) = message_from(&pair.specialist.db(), false, &reply.message_id);
            assert_eq!(sender, pair.specialist.address);
            assert_eq!(recipient, pair.org.address);
            assert_eq!(
                received.canonical_sha256().unwrap(),
                sent.canonical_sha256().unwrap()
            );
            break;
        }
        assert!(
            Instant::now() < limit,
            "native peer did not acknowledge the exact release"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(step_recovery(&fixture, &path, &entry).await, completed);
    assert_entry_replay(&fixture, &path, &completed, &entry).await;
    assert_eq!(
        assert_native_exchange(&pair, &journal, &completed),
        original_receipt
    );
    assert_eq!(assessment_count(&fixture), 2);
    assert_eq!(fixture.providers.execution_posts().len(), 2);
    let outgoing: u64 = pair
        .org
        .db()
        .query_row("SELECT count(*) FROM outbox", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        outgoing, 4,
        "one recruit, consent, task and exact release intent"
    );
}

#[tokio::test]
#[ignore = "requires GENESIS_FETCH_TEST_PYTHON with uv --frozen dependencies; launches two native signed loopback processes"]
async fn signed_specialist_selection_completes_real_recovery_and_retires() {
    tokio::time::timeout(Duration::from_secs(90), run_native_recovery(false))
        .await
        .expect("native recovery integration exceeded its total time bound");
}

#[tokio::test]
#[ignore = "requires GENESIS_FETCH_TEST_PYTHON with uv --frozen dependencies; launches two native signed loopback processes"]
async fn automatic_intake_completes_signed_recovery_and_retires() {
    tokio::time::timeout(Duration::from_secs(90), run_native_recovery(true))
        .await
        .expect("native automatic intake integration exceeded its total time bound");
}
