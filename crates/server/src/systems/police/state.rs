//! Wanted-state transitions and police-unit control operations.
use super::*;
pub(crate) fn clear_wanted() -> Wanted {
    Wanted {
        heat: 0,
        pursuit: Pursuit::Clear,
        last_known: [0.0; 3],
        last_seen: 0,
        vehicle_sighting: None,
        search_since: None,
        points: 0,
        last_police_attack: None,
        last_gunfire: None,
        next_officer_shot: 0,
        fire_authorized: false,
        last_noise_increase: None,
        arrest_ticks: 0,
        arrest_pending: false,
        surrendering: false,
        seen: VecDeque::new(),
    }
}

pub(crate) fn observe(wanted: &mut Wanted, event: CrimeId, tick: u32, report: CrimeReport) {
    if wanted.seen.contains(&event) {
        return;
    }
    wanted.seen.push_back(event);
    if wanted.seen.len() > 64 {
        wanted.seen.pop_front();
    }
    if matches!(event, CrimeId::Shot(_))
        && matches!(report.kind, CrimeKind::PoliceCar(_) | CrimeKind::Officer)
    {
        wanted.last_police_attack = Some(tick);
        wanted.fire_authorized = true;
    }
    if matches!(event, CrimeId::Shot(_)) {
        wanted.last_gunfire = Some(tick);
    }
    let noise = matches!(report.kind, CrimeKind::Shot);
    if !noise
        || wanted
            .last_noise_increase
            .is_none_or(|t| tick.saturating_sub(t) >= 30)
    {
        let ceiling = match report.kind {
            CrimeKind::Shot => STAR_POINTS[1] - 1,
            CrimeKind::Theft | CrimeKind::Harm => STAR_POINTS[2] - 1,
            CrimeKind::PoliceCar(_) | CrimeKind::Officer => STAR_POINTS[2],
        }
        .max(wanted.points);
        wanted.points = wanted
            .points
            .saturating_add(report.kind.points())
            .min(ceiling);
        wanted.heat = STAR_POINTS
            .iter()
            .filter(|&&threshold| wanted.points >= threshold)
            .count() as u8;
        if noise {
            wanted.last_noise_increase = Some(tick);
        }
    }
    // A noise report opens an investigation but cannot refresh visual contact.
    if report.confirmed || wanted.pursuit == Pursuit::Clear {
        wanted.last_seen = tick;
        wanted.last_known = report.position;
        wanted.search_since = None;
        wanted.pursuit = Pursuit::Pursuing;
    }
}
pub(crate) fn advance(
    wanted: &mut Wanted,
    tick: u32,
    sight: Option<[f32; 3]>,
    arrest: bool,
) -> bool {
    if wanted.arrest_pending {
        return true;
    }
    if wanted.heat == 0 {
        return false;
    }
    if let Some(position) = sight {
        wanted.last_seen = tick;
        wanted.last_known = position;
        wanted.search_since = None;
        wanted.pursuit = Pursuit::Pursuing;
    } else if tick.saturating_sub(wanted.last_seen) >= 150 && wanted.search_since.is_none() {
        wanted.search_since = Some(tick);
        wanted.pursuit = Pursuit::Searching;
    }
    if wanted.search_since.is_some_and(|t| {
        tick.saturating_sub(t) >= RESPONSE[usize::from(wanted.heat - 1)].search_ticks
    }) {
        wanted.heat -= 1;
        wanted.points = if wanted.heat == 0 {
            0
        } else {
            STAR_POINTS[usize::from(wanted.heat - 1)]
        };
        wanted.search_since = Some(tick);
        if wanted.heat == 0 {
            wanted.last_police_attack = None;
            wanted.fire_authorized = false;
            wanted.last_gunfire = None;
            wanted.pursuit = Pursuit::Clear;
            wanted.search_since = None;
        }
    }
    if arrest && wanted.heat > 0 {
        wanted.arrest_ticks = wanted.arrest_ticks.saturating_add(1);
        wanted.pursuit = Pursuit::Arresting;
    } else {
        wanted.arrest_ticks = 0;
        // Arresting is transient, not a state that survives failed conditions.
        if wanted.pursuit == Pursuit::Arresting {
            wanted.pursuit = if wanted.search_since.is_some() {
                Pursuit::Searching
            } else {
                Pursuit::Pursuing
            };
        }
    }
    wanted.arrest_ticks >= 90
}

pub(crate) fn release_target(unit: &mut Unit) {
    unit.target = None;
    unit.route.clear();
    unit.lane_entry = None;
    unit.blocked_since = None;
    unit.reverse_until = 0;
    unit.avoided_node = None;
    unit.next_plan = 0;
}

pub(crate) fn motor_input(unit: &Unit, tick: u32, speed: f32) -> InputFrame {
    let mut input = unit.input.clone();
    input.tick = tick;
    input.move_z = InputAxis(
        ((unit.target_speed - speed) * 2.0)
            .clamp(-1.0, 1.0)
            .mul_add(32767.0, 0.0) as i16,
    );
    input
}
