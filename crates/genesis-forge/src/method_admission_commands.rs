use crate::{emit, storage, Arguments};
use genesis_forge::{method_admission::GenerationAdmission, method_roster::GenerationRoster};

pub fn run(args: &Arguments) -> Result<u8, String> {
    let admission: GenerationAdmission = if args.command == "method-admission-prepare" {
        let roster: GenerationRoster = storage::read_json(args.path("--roster")?)?;
        let journal = GenerationAdmission::new(roster).map_err(|e| e.to_string())?;
        let lock = storage::CheckpointLock::acquire(args.path("--admission")?)?;
        lock.create(&journal)?;
        journal
    } else {
        // Status never locks, recovers, refunds or mutates either journal.
        storage::read_json(args.path("--admission")?)?
    };
    emit(&admission.summary().map_err(|e| e.to_string())?)?;
    Ok(0)
}
