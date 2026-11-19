//! Durable repair transitions, submission and private status publication.
//! These functions run per profile in stable order inside the repair ECS system.
use super::*;
use std::result::Result;
type Results = BTreeMap<String, Result<ProfileView, EconomicError>>;

/// Collects repair transaction completions indexed by event ID.
pub(super) fn collect_results(profiles: &mut Profiles, tick: u32) -> Results {
    profiles
        .poll_economic(tick)
        .into_iter()
        .map(|r| (r.event, r.result))
        .collect()
}
/// Hides completed repair status after its display deadline.
pub(super) fn expire_status(sessions: &mut Sessions, tick: u32) {
    for s in sessions.peers.values_mut() {
        if s.repair_clear.is_some_and(|end| tick >= end) {
            s.repair_view = None;
            s.repair_clear = None;
            s.status_dirty = true;
        }
    }
}
/// Advances repair dwell, debit, or refund state using committed transaction results.
pub(super) fn advance(
    r: &mut PendingRepair,
    profile: &str,
    eligible: bool,
    results: &mut Results,
    profiles: &mut Profiles,
    cars: &mut BTreeMap<u32, crate::component::queries::VehicleQueryItem<'_, '_>>,
) {
    if !eligible {
        r.invalidated = true;
    }
    match r.phase {
        RepairPhase::Dwell => {
            if !eligible {
                r.phase = RepairPhase::Cancelled;
                r.code = ActionCode::Unavailable;
            } else {
                r.dwell += 1;
                if r.dwell >= 60 {
                    r.phase = RepairPhase::DebitPending;
                }
            }
        }
        RepairPhase::DebitPending => {
            if let Some(result) = results.remove(&r.event) {
                r.queued = false;
                match result {
                    Ok(_) if eligible && !r.invalidated => {
                        let v = cars.get_mut(&r.car).expect("eligible repair car");
                        v.state.health.0 = v.template.health;
                        v.idle_ticks = 0;
                        profiles.finish_repair_debit(profile, &r.event);
                        r.phase = RepairPhase::Completed;
                        r.code = ActionCode::Ok;
                    }
                    Ok(_) => {
                        r.phase = RepairPhase::RefundPending;
                    }
                    Err(EconomicError::InsufficientFunds) => {
                        r.phase = RepairPhase::Failed;
                        r.code = ActionCode::InsufficientFunds;
                    }
                    Err(EconomicError::Unavailable) => {
                        r.code = ActionCode::Unavailable;
                    }
                }
            }
        }
        RepairPhase::RefundPending => {
            if let Some(result) = results.remove(&r.operation()) {
                r.queued = false;
                if result.is_ok() {
                    r.phase = RepairPhase::Cancelled;
                    r.code = ActionCode::Ok;
                } else {
                    r.code = ActionCode::Unavailable;
                }
            }
        }
        _ => {}
    }
}
/// Queues the repair debit or refund once for its current phase.
pub(super) fn submit(r: &mut PendingRepair, profile: &str, profiles: &mut Profiles, tick: u32) {
    if matches!(
        r.phase,
        RepairPhase::DebitPending | RepairPhase::RefundPending
    ) && !r.queued
    {
        r.queued = profiles
            .submit_repair_event(
                profile,
                &r.operation(),
                r.phase == RepairPhase::RefundPending,
                tick,
            )
            .is_ok();
        if !r.queued {
            r.code = ActionCode::Unavailable;
        }
    }
}
/// Updates private repair status and committed profile data for matching sessions.
pub(super) fn publish(
    r: &PendingRepair,
    profile: &str,
    pending: bool,
    profiles: &Profiles,
    sessions: &mut Sessions,
    tick: u32,
) {
    for s in sessions.peers.values_mut().filter(|s| {
        s.profile
            .as_ref()
            .is_some_and(|p| p.profile_id.0 == profile)
    }) {
        if let Some(committed) = profiles.committed(profile)
            && s.profile.as_ref() != Some(&committed)
        {
            s.profile = Some(committed);
            s.status_dirty = true;
        }
        let view = r.view();
        if (!pending || r.phase != RepairPhase::Dwell || tick.is_multiple_of(6))
            && s.repair_view.as_ref() != Some(&view)
        {
            s.repair_view = Some(view);
            s.status_dirty = true;
        }
        if !pending {
            s.repair_clear = Some(tick.saturating_add(150));
        }
    }
}
