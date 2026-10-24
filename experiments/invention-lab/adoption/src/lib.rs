//! Henry's Claude: adopting another organization's exact development method in
//! the invention lab, following `docs/invention-lab/adoption.md`.
//!
//! Two pure steps around the receiver's own acceptance trial:
//!
//! 1. [`precheck`] decides, before anything executes, whether an offered exact
//!    package is one this receiver could run at all: canonical package bytes,
//!    not its own publication, the receiver's exact runtime and protocol,
//!    only dependencies it allows, and no installation of the same exact
//!    version. Reasons name the mismatched requirement, never task data.
//! 2. [`conclude`] turns the receiver's frozen acceptance plan and its
//!    validated evidence into one adoption outcome. Unknown execution or
//!    scoring wins over everything; otherwise the verdict decides
//!    (`Accepted` → adopted, `Rejected` → not applicable, `Insufficient` →
//!    insufficient evidence). Only `adopted` yields an [`Installation`], pinned
//!    to the exact method, plan, criterion, evaluator and environment.
//!
//! Nothing here authorizes mission work or claims improvement. Publication
//! never upgrades an installation: adopting another version is a new intent.
use genesis_core::development_evaluation::EvaluationOutcome;
use genesis_core::development_method::{ExactContentRef, ExecutionOutcome};
use genesis_core::development_package::MethodPackage;
use genesis_core::development_trial::{TrialPlan, TrialPurpose};
use genesis_lab_score::verdict::{decide, SlotEvidence, Verdict};
use serde::{Deserialize, Serialize};

const PACKAGE_DOMAIN: &str = "genesis.development_method_source";
const PACKAGE_SCHEMA: &str = "development-method-source/1";

/// What the receiving organization can run, decided from its own
/// configuration, never from an offer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receiver {
    pub org_id: String,
    pub runtime: ExactContentRef,
    pub protocol: ExactContentRef,
    pub allowed_dependencies: Vec<ExactContentRef>,
    /// Exact method references already installed (checking or accepted).
    pub installed: Vec<ExactContentRef>,
}

/// One offered exact method version and who published it.
#[derive(Debug, Clone)]
pub struct Offer {
    pub publisher: String,
    pub reference: ExactContentRef,
    pub bytes: Vec<u8>,
}

/// The candidate decision of `adoption.md` steps 2–9 that apply to a source
/// package. Budget and evaluator requirements (steps 7–8) need manifest fields
/// the merged package contract doesn't carry; the receiver's own plan fixes both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Selected,
    OwnPublication,
    InvalidManifest { reason: String },
    UnsupportedKind { reason: String },
    IncompatibleEnvironment { reason: String },
    InstallationConflict,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    domain: String,
    version: u8,
    package: MethodPackage,
}

fn sha256(bytes: &[u8]) -> String {
    genesis_lab_exec::refs::sha256_hex(bytes)
}

/// Parses the offered bytes as a canonical package whose identity is exactly
/// the offered reference.
fn parse(offer: &Offer) -> Result<MethodPackage, String> {
    let r = &offer.reference;
    if r.domain != "genesis" || r.schema != PACKAGE_SCHEMA {
        return Err(format!(
            "{}/{} is not a method source package",
            r.domain, r.schema
        ));
    }
    if sha256(&offer.bytes) != r.sha256 {
        return Err("bytes differ from the offered reference".into());
    }
    let e: Envelope = serde_json::from_slice(&offer.bytes).map_err(|e| format!("package: {e}"))?;
    if e.domain != PACKAGE_DOMAIN || e.version != 1 {
        return Err("unknown package format".into());
    }
    let canonical = e
        .package
        .artifact_json()
        .map_err(|e| format!("package: {e}"))?;
    if canonical.as_bytes() != offer.bytes.as_slice() {
        return Err("noncanonical package bytes".into());
    }
    if e.package.reference().map_err(|e| format!("package: {e}"))? != *r {
        return Err("package identity differs from the offered reference".into());
    }
    Ok(e.package)
}

/// Pre-execution checks for one offer, in `adoption.md` order.
pub fn precheck(offer: &Offer, receiver: &Receiver) -> Decision {
    if offer.publisher == receiver.org_id {
        return Decision::OwnPublication;
    }
    if offer.reference.schema != PACKAGE_SCHEMA {
        return Decision::UnsupportedKind {
            reason: format!("this organization runs {PACKAGE_SCHEMA} methods"),
        };
    }
    let package = match parse(offer) {
        Ok(p) => p,
        Err(reason) => return Decision::InvalidManifest { reason },
    };
    if *package.runtime() != receiver.runtime {
        return Decision::IncompatibleEnvironment {
            reason: format!(
                "needs runtime {}@{} ({}), this organization runs {}@{} ({})",
                package.runtime().id,
                package.runtime().version,
                package.runtime().sha256,
                receiver.runtime.id,
                receiver.runtime.version,
                receiver.runtime.sha256
            ),
        };
    }
    if *package.protocol() != receiver.protocol {
        return Decision::IncompatibleEnvironment {
            reason: format!(
                "needs protocol {}@{}, this organization speaks {}@{}",
                package.protocol().id,
                package.protocol().version,
                receiver.protocol.id,
                receiver.protocol.version
            ),
        };
    }
    if let Some(d) = package
        .dependencies()
        .iter()
        .find(|d| !receiver.allowed_dependencies.contains(d))
    {
        return Decision::IncompatibleEnvironment {
            reason: format!("dependency {}@{} is not allowed here", d.id, d.version),
        };
    }
    if receiver.installed.contains(&offer.reference) {
        return Decision::InstallationConflict;
    }
    Decision::Selected
}

/// The receiver's pinned, accepted installation of one exact method version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub org_id: String,
    pub method: ExactContentRef,
    pub dependencies: Vec<ExactContentRef>,
    pub acceptance_plan: ExactContentRef,
    pub criterion: ExactContentRef,
    pub evaluator: ExactContentRef,
    pub environment: ExactContentRef,
    pub verdict: Verdict,
}

/// One adoption intent's conclusion, with the evidence-distinguished outcome
/// vocabulary of `adoption.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    Adopted { installation: Box<Installation> },
    NotApplicable { reason: String },
    InsufficientEvidence { reason: String },
    Unknown { reason: String },
}

fn role_slot(
    plan: &TrialPlan,
    role: genesis_core::development_method::AcceptanceRole,
) -> Option<&genesis_core::development_trial::TrialSlot> {
    plan.slots
        .iter()
        .find(|s| matches!(s.purpose, TrialPurpose::Acceptance { role: r } if r == role))
}

fn receiver_fits(
    receiver: &Receiver,
    plan: &TrialPlan,
    slot: &genesis_core::development_trial::TrialSlot,
) -> Result<(), String> {
    use genesis_lab_exec::{refs, sandbox::Mode};
    if ![Mode::Namespaces, Mode::None].iter().any(|m| {
        refs::environment_for_runtime(*m, &receiver.runtime) == plan.development_environment
    }) {
        return Err("the acceptance trial did not run on this receiver's runtime".into());
    }
    if receiver.protocol != refs::protocol() {
        return Err("this receiver's protocol is not the one the trial's executor speaks".into());
    }
    if let Some(d) = slot
        .dependencies
        .iter()
        .find(|d| !receiver.allowed_dependencies.contains(d))
    {
        return Err(format!(
            "dependency {}@{} is not allowed here",
            d.id, d.version
        ));
    }
    if receiver.installed.contains(&slot.method) {
        return Err("this exact method version is already installed here".into());
    }
    Ok(())
}

/// Concludes an intent for one exact selected offer: the offer must pass
/// [`precheck`] for this receiver and be exactly the plan's candidate method
/// before the evidence is weighed by [`conclude`].
pub fn conclude_selected(
    receiver: &Receiver,
    offer: &Offer,
    plan: &TrialPlan,
    criterion: &[u8],
    candidate: &SlotEvidence,
    control: &SlotEvidence,
) -> Result<Outcome, String> {
    use genesis_core::development_method::AcceptanceRole;
    match precheck(offer, receiver) {
        Decision::Selected => {}
        d => return Err(format!("the offer was not selected: {d:?}")),
    }
    let slot = role_slot(plan, AcceptanceRole::Candidate).ok_or("plan has no candidate slot")?;
    let package = parse(offer)?;
    if slot.method != offer.reference || slot.dependencies != package.dependencies() {
        return Err("the plan's candidate is not the selected offer".into());
    }
    conclude(receiver, plan, criterion, candidate, control)
}

/// Concludes one adoption intent from the receiver's frozen acceptance plan,
/// its criterion bytes and both slots' evidence. The plan must have run on
/// the receiver's runtime and protocol with allowed dependencies, and the
/// candidate must not already be installed. `Err` means the inputs don't
/// belong together (not an outcome); a caller records that as `stopped`.
pub fn conclude(
    receiver: &Receiver,
    plan: &TrialPlan,
    criterion: &[u8],
    candidate: &SlotEvidence,
    control: &SlotEvidence,
) -> Result<Outcome, String> {
    use genesis_core::development_method::AcceptanceRole;
    let slot = role_slot(plan, AcceptanceRole::Candidate).ok_or("plan has no candidate slot")?;
    if slot.org_id.as_str() != receiver.org_id {
        return Err("the acceptance plan belongs to another organization".into());
    }
    // The trial must have run what this receiver can run, and it must not
    // already hold this exact version: the same requirements precheck applies
    // to the offer, checked here against the plan's candidate slot.
    receiver_fits(receiver, plan, slot)?;
    // The verdict re-validates every receipt against the plan first.
    let verdict = decide(plan, criterion, candidate, control)?;
    for (name, e) in [("candidate", candidate), ("control", control)] {
        if e.execution.outcome == ExecutionOutcome::Unknown {
            return Ok(Outcome::Unknown {
                reason: format!("the {name} execution outcome is unknown"),
            });
        }
        if let Some(EvaluationOutcome::Unknown { reason }) = e.scoring.as_ref().map(|s| &s.outcome)
        {
            return Ok(Outcome::Unknown {
                reason: format!("the {name} scoring outcome is unknown: {reason}"),
            });
        }
    }
    Ok(match verdict {
        Verdict::Accepted { .. } => Outcome::Adopted {
            installation: Box::new(Installation {
                org_id: receiver.org_id.clone(),
                method: slot.method.clone(),
                dependencies: slot.dependencies.clone(),
                acceptance_plan: plan.reference().map_err(|e| format!("plan: {e:?}"))?,
                criterion: plan.criterion.clone(),
                evaluator: plan.context.evaluator.clone(),
                environment: plan.development_environment.clone(),
                verdict,
            }),
        },
        Verdict::Rejected { reason } => Outcome::NotApplicable { reason },
        Verdict::Insufficient { reason } => Outcome::InsufficientEvidence { reason },
    })
}
