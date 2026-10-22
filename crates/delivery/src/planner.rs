//! Who flies which request: a language model, asked in plain language. It
//! sees a `View` of the fleet and returns calls, which the dispatcher checks
//! before sending.
//!
//! Replies are kept in one folder, one readable JSON file per prompt, named by
//! a hash of the model, the sample index, and the prompt. A rerun sends the
//! same prompts, finds every reply in the folder, and so replays the whole
//! episode exactly without calling the model. `sample` selects an independent
//! reply to the same prompt, so samples 0, 1, and 2 are three live reruns of
//! one episode.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail, ensure};
use ripple_core::Vec3;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::directive::{Directive, DirectiveKind};
use crate::messages::{Assignment, Phase, Request, SkillName, Status, VehicleKind};
use crate::skill;

/// What a planner sees at one decision.
pub struct View<'a> {
    pub time_s: f64,
    pub depot_world_m: Vec3,
    /// Every vehicle's latest report, by ID.
    pub vehicles: Vec<&'a Status>,
    /// Open requests nobody is carrying, by ID.
    pub waiting: Vec<&'a Request>,
    /// Instructions from operations in force now, in plain language.
    pub instructions: Vec<String>,
    /// The skills this mission offers the model.
    pub skills: &'a [SkillName],
    /// The largest share of a vehicle's endurance left a round trip may use.
    pub range_margin: f64,
    pub dwell_s: f64,
}

/// One proposed order. `skill` is `None` when the model named a skill that
/// doesn't exist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Call {
    pub vehicle_id: i32,
    pub skill: Option<SkillName>,
    pub request_id: i32,
    /// The waypoint the model gave, if any.
    pub via_world_m: Option<Vec3>,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub calls: Vec<Call>,
    pub parse_failed: bool,
}

/// Why a call cannot be flown, or `Ok` if it can. The dispatcher refuses the
/// call and counts the reason. Minimum speed and turn radius are told to the
/// model but not checked: with these two skills they never make a call
/// infeasible.
pub fn check(call: &Call, view: &View<'_>) -> Result<(), &'static str> {
    let Some(vehicle) = view
        .vehicles
        .iter()
        .find(|v| v.vehicle_id == call.vehicle_id)
    else {
        return Err("unknown_vehicle");
    };
    let Some(request) = view.waiting.iter().find(|r| r.id == call.request_id) else {
        return Err("unknown_request");
    };
    let Some(skill) = call.skill.filter(|skill| view.skills.contains(skill)) else {
        return Err("unknown_skill");
    };
    if vehicle.phase != Phase::Idle || vehicle.request_id.is_some() {
        return Err("busy_vehicle");
    }
    let info = skill::info(skill);
    if info.multirotor_only && vehicle.card.kind != VehicleKind::Multirotor {
        return Err("wrong_skill");
    }
    if request.rooftop && !info.rooftops {
        return Err("rooftop_drop");
    }
    if info.via && call.via_world_m.is_none() {
        return Err("missing_via");
    }
    if round_trip_s(vehicle, request, call, view) > vehicle.endurance_left_s * view.range_margin {
        return Err("out_of_range");
    }
    Ok(())
}

/// Flight time out (through any waypoint) and back from the vehicle's
/// position, plus any landing.
fn round_trip_s(vehicle: &Status, request: &Request, call: &Call, view: &View<'_>) -> f64 {
    let out_m = out_m(vehicle, request, call.via_world_m);
    let back_m = match call.via_world_m {
        Some(via) => {
            (request.site_world_m - via).xy().norm() + (via - view.depot_world_m).xy().norm()
        }
        None => (request.site_world_m - view.depot_world_m).xy().norm(),
    };
    let dwell_s = if call.skill.is_some_and(|skill| skill::info(skill).lands) {
        view.dwell_s
    } else {
        0.0
    };
    (out_m + back_m) / vehicle.card.cruise_speed_mps + dwell_s
}

/// Whether a call, flown as planned, would break the instruction in its
/// window: deliver to the closed site, fly the grounded vehicle, cross the
/// no-fly zone, or send out the last idle multirotor (counting `sent`, the
/// orders already made in this decision). Times are straight-line estimates
/// at cruise speed through any waypoint, with a landing's dwell at the site.
pub fn breaks_instruction(
    directive: &Directive,
    call: &Call,
    view: &View<'_>,
    sent: &[Assignment],
) -> bool {
    let vehicle = view
        .vehicles
        .iter()
        .find(|v| v.vehicle_id == call.vehicle_id);
    let request = view.waiting.iter().find(|r| r.id == call.request_id);
    let (Some(vehicle), Some(request), Some(skill)) = (vehicle, request, call.skill) else {
        return false;
    };
    // The route as (point, time reached), out through any waypoint and back.
    let speed_mps = vehicle.card.cruise_speed_mps;
    let mut points = vec![vehicle.position_world_m];
    points.extend(call.via_world_m);
    points.push(request.site_world_m);
    let site = points.len() - 1;
    points.extend(call.via_world_m);
    points.push(view.depot_world_m);
    let mut route = vec![(points[0], view.time_s)];
    for (index, pair) in points.windows(2).enumerate() {
        let mut time_s = route[index].1 + (pair[1] - pair[0]).xy().norm() / speed_mps;
        if index + 1 > site && skill::info(skill).lands {
            time_s += view.dwell_s;
        }
        route.push((pair[1], time_s));
    }
    let arrive_s = route[site].1;
    let home_s = route[route.len() - 1].1;
    let (start_s, end_s) = (directive.start_s, directive.end_s);
    let flies_in_window = view.time_s <= end_s && home_s >= start_s;
    match directive.kind {
        DirectiveKind::None => false,
        DirectiveKind::Closure => {
            request.site == directive.site && (start_s..=end_s).contains(&arrive_s)
        }
        DirectiveKind::GroundVehicle => vehicle.vehicle_id == directive.vehicle && flies_in_window,
        DirectiveKind::NoFly => route.windows(2).any(|leg| {
            let ((a, a_s), (b, b_s)) = (leg[0], leg[1]);
            let path = (b - a).xy();
            let offset = (directive.center_world_m() - a).xy();
            let along = (offset.dot(&path) / path.norm_squared().max(1e-9)).clamp(0.0, 1.0);
            let time_s = a_s + along * (b_s - a_s);
            (offset - path * along).norm() < directive.radius_m
                && (start_s..=end_s).contains(&time_s)
        }),
        DirectiveKind::ReserveMultirotor => {
            let still_idle = view.vehicles.iter().any(|v| {
                v.card.kind == VehicleKind::Multirotor
                    && v.phase == Phase::Idle
                    && v.request_id.is_none()
                    && v.vehicle_id != vehicle.vehicle_id
                    && !sent.iter().any(|order| order.vehicle_id == v.vehicle_id)
            });
            vehicle.card.kind == VehicleKind::Multirotor && flies_in_window && !still_idle
        }
    }
}

/// Horizontal distance from the vehicle to the site, through any waypoint.
fn out_m(vehicle: &Status, request: &Request, via_world_m: Option<Vec3>) -> f64 {
    match via_world_m {
        Some(via) => {
            (via - vehicle.position_world_m).xy().norm() + (request.site_world_m - via).xy().norm()
        }
        None => (request.site_world_m - vehicle.position_world_m)
            .xy()
            .norm(),
    }
}

/// Flight time from the vehicle to the request's site, through any waypoint.
pub fn arrival_s(vehicle: &Status, request: &Request, via_world_m: Option<Vec3>) -> f64 {
    out_m(vehicle, request, via_world_m) / vehicle.card.cruise_speed_mps
}

pub const SYSTEM_PROMPT: &str = "You dispatch a fleet of medical delivery drones from \
one depot. Assign idle vehicles to waiting requests. Reply with JSON only, no other text.";

/// The decision in plain language. Times are minutes and distances kilometers,
/// rounded, so that the prompt (and the cache key) depends only on the state.
pub fn prompt(view: &View<'_>, limits_in_prompt: bool) -> String {
    let km = |m: f64| m / 1000.0;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Time: minute {:.1}. The depot is at (0.0, 0.0) km.",
        view.time_s / 60.0
    );
    let _ = writeln!(text, "\nSkills:");
    let offered: Vec<_> = skill::LIBRARY
        .iter()
        .filter(|skill| view.skills.contains(&skill.name))
        .collect();
    for skill in &offered {
        let _ = writeln!(text, "- {}: {}", skill.name.as_str(), skill.description);
    }
    let _ = writeln!(
        text,
        "Each vehicle carries one package and flies straight to the site and straight \
         back, unless its skill says otherwise. Rooftop sites accept only a landing."
    );
    let _ = writeln!(text, "\nVehicles:");
    for vehicle in &view.vehicles {
        let kind = match vehicle.card.kind {
            VehicleKind::FixedWing => "fixed-wing",
            VehicleKind::Multirotor => "multirotor",
        };
        let state = if vehicle.phase == Phase::Idle && vehicle.request_id.is_none() {
            "idle".to_string()
        } else if vehicle.phase == Phase::Lost {
            "lost (out of endurance)".to_string()
        } else {
            format!("busy ({:?})", vehicle.phase).to_lowercase()
        };
        let p = vehicle.position_world_m;
        let _ = write!(
            text,
            "- vehicle {}: {kind}, {state}, at ({:.1}, {:.1}) km",
            vehicle.vehicle_id,
            km(p.x),
            km(p.y)
        );
        if limits_in_prompt {
            let hover = if vehicle.card.min_speed_mps > 0.0 {
                format!(
                    "cannot hover or land, minimum speed {:.0} m/s, turn radius {:.0} m",
                    vehicle.card.min_speed_mps, vehicle.card.turn_radius_m
                )
            } else {
                "can hover and land".to_string()
            };
            let _ = write!(
                text,
                "; cruise {:.0} m/s; {hover}; {:.1} min of flight left",
                vehicle.card.cruise_speed_mps,
                vehicle.endurance_left_s / 60.0
            );
        }
        let _ = writeln!(text);
    }
    let _ = writeln!(text, "\nWaiting requests:");
    for request in &view.waiting {
        let p = request.site_world_m;
        let _ = writeln!(
            text,
            "- request {}: {} {} of {} for {}{} at ({:.1}, {:.1}) km, {}, due by minute {:.1}",
            request.id,
            request.units,
            if request.units == 1 { "unit" } else { "units" },
            request.item,
            request.site,
            if request.rooftop { " (rooftop)" } else { "" },
            km(p.x),
            km(p.y),
            if request.urgent { "urgent" } else { "routine" },
            request.deadline_s / 60.0
        );
    }
    if !view.instructions.is_empty() {
        let _ = writeln!(text, "\nInstructions from operations:");
        for instruction in &view.instructions {
            let _ = writeln!(text, "- {instruction}");
        }
    }
    let names: Vec<String> = offered
        .iter()
        .map(|skill| format!("\"{}\"", skill.name.as_str()))
        .collect();
    let _ = writeln!(
        text,
        "\nReply with {{\"assignments\": [{{\"vehicle\": <id>, \"skill\": {}, \
         \"request\": <id>}}, ...]}}. Assign only idle vehicles, at most one request each. \
         Leave a request unassigned if no idle vehicle should take it.",
        names.join(" or ")
    );
    for skill in offered.iter().filter(|skill| skill.via) {
        let _ = writeln!(
            text,
            "For {}, also give \"via\": [x, y], the waypoint in km.",
            skill.name.as_str()
        );
    }
    text
}

#[derive(Deserialize)]
struct Reply {
    assignments: Vec<RawCall>,
}

#[derive(Deserialize)]
struct RawCall {
    vehicle: i32,
    skill: String,
    request: i32,
    /// A waypoint in km, for skills that take one.
    #[serde(default)]
    via: Option<[f64; 2]>,
}

/// The assignments in a reply, taking the outermost braces so a model that
/// wraps its JSON in text or a code fence still parses. `None` if unreadable.
pub fn parse(reply: &str) -> Option<Vec<Call>> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let parsed: Reply = serde_json::from_str(reply.get(start..=end)?).ok()?;
    Some(
        parsed
            .assignments
            .into_iter()
            .map(|raw| Call {
                vehicle_id: raw.vehicle,
                skill: skill::named(&raw.skill),
                request_id: raw.request,
                // Kept only for a skill that takes a waypoint.
                via_world_m: raw
                    .via
                    .filter(|_| skill::named(&raw.skill).is_some_and(|s| skill::info(s).via))
                    .map(|[x, y]| Vec3::new(x * 1000.0, y * 1000.0, 0.0)),
            })
            .collect(),
    )
}

/// A language model, called through a command-line client, with its replies
/// kept on disk.
pub struct ModelPlanner {
    pub model: String,
    pub sample: u32,
    pub limits_in_prompt: bool,
    pub cache_dir: PathBuf,
    /// The client: run as `command -p --model <model> ...` with the prompt on
    /// stdin, printing the reply. `none` uses only replies already in the
    /// folder, and a missing one is an error.
    pub command: String,
}

#[derive(Serialize, Deserialize)]
struct CachedReply {
    model: String,
    sample: u32,
    system: String,
    prompt: String,
    reply: String,
}

impl ModelPlanner {
    pub fn plan(&self, view: &View<'_>) -> Result<Plan> {
        let reply = self.reply(&prompt(view, self.limits_in_prompt))?;
        let calls = parse(&reply);
        Ok(Plan {
            parse_failed: calls.is_none(),
            calls: calls.unwrap_or_default(),
        })
    }

    fn reply(&self, prompt: &str) -> Result<String> {
        let key = Sha256::digest(
            format!("{}\n{}\n{SYSTEM_PROMPT}\n{prompt}", self.model, self.sample).as_bytes(),
        );
        let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = self.cache_dir.join(format!("{hex}.json"));
        let read_cached = || -> Result<String> {
            let cached: CachedReply = serde_json::from_str(&std::fs::read_to_string(&path)?)
                .with_context(|| format!("reading cached reply {}", path.display()))?;
            Ok(cached.reply)
        };
        if path.exists() {
            return read_cached();
        }
        ensure!(
            self.command != "none",
            "no saved reply {} (command: none)",
            path.display()
        );
        let reply = self.ask(prompt)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        let cached = CachedReply {
            model: self.model.clone(),
            sample: self.sample,
            system: SYSTEM_PROMPT.into(),
            prompt: prompt.into(),
            reply: reply.clone(),
        };
        // Write, then link into place, so a crash never leaves half a reply in
        // the cache. Runs in parallel can ask the same prompt at once (cases
        // that differ only in a later instruction share their first prompts):
        // the first reply stored wins and the others use it, so every run
        // replays from the cache.
        let partial = path.with_extension(format!(
            "part{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&partial, serde_json::to_string_pretty(&cached)?)?;
        let linked = std::fs::hard_link(&partial, &path);
        std::fs::remove_file(&partial)?;
        match linked {
            Ok(()) => Ok(reply),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_cached(),
            Err(error) => Err(error.into()),
        }
    }

    /// Tries three times, a minute apart.
    fn ask(&self, prompt: &str) -> Result<String> {
        let mut last_error = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
            let spawned = Command::new(&self.command)
                .args(["-p", "--model", &self.model, "--output-format", "text"])
                .args(["--no-session-persistence", "--tools", ""])
                .args(["--system-prompt", SYSTEM_PROMPT])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            // The client can be missing for a moment while it updates itself.
            let mut child = match spawned {
                Ok(child) => child,
                Err(error) => {
                    last_error = format!("starting {}: {error}", self.command);
                    continue;
                }
            };
            child
                .stdin
                .take()
                .context("model client stdin")?
                .write_all(prompt.as_bytes())?;
            let output = child.wait_with_output()?;
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
            // The client may report its error on either stream.
            last_error = format!(
                "{} {}",
                String::from_utf8_lossy(&output.stderr).trim(),
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }
        bail!("{} failed three times: {last_error}", self.command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_wrapped_in_text_still_parses() {
        let reply = "Here you go:\n```json\n{\"assignments\": [{\"vehicle\": 2, \
                     \"skill\": \"Drop\", \"request\": 1}, {\"vehicle\": 4, \"skill\": \
                     \"fly\", \"request\": 3}, {\"vehicle\": 3, \"skill\": \"drop_via\", \
                     \"request\": 5, \"via\": [-2.0, 1.5]}]}\n```";
        let calls = parse(reply).expect("parses");
        assert_eq!(
            calls,
            vec![
                Call {
                    vehicle_id: 2,
                    skill: Some(SkillName::Drop),
                    request_id: 1,
                    via_world_m: None,
                },
                Call {
                    vehicle_id: 4,
                    skill: None,
                    request_id: 3,
                    via_world_m: None,
                },
                Call {
                    vehicle_id: 3,
                    skill: Some(SkillName::DropVia),
                    request_id: 5,
                    via_world_m: Some(Vec3::new(-2000.0, 1500.0, 0.0)),
                },
            ]
        );
        assert!(parse("no json here").is_none());
    }

    /// Two runs that ask the same new prompt at once get different replies
    /// from the model; both must go on with the one the cache keeps.
    #[cfg(unix)]
    #[test]
    fn runs_asking_the_same_prompt_at_once_use_the_same_reply() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("delivery-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder)?;
        // Replies with its own process ID, after the other call has started.
        let client = folder.join("fake-model");
        std::fs::write(&client, "#!/bin/sh\ncat > /dev/null\nsleep 0.5\necho $$\n")?;
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755))?;
        let planner = || ModelPlanner {
            model: "fake".into(),
            sample: 0,
            limits_in_prompt: false,
            cache_dir: folder.join("cache"),
            command: client.display().to_string(),
        };
        let asks: Vec<_> = (0..2)
            .map(|_| {
                let planner = planner();
                std::thread::spawn(move || planner.reply("the same prompt"))
            })
            .collect();
        let replies = asks
            .into_iter()
            .map(|ask| ask.join().expect("no panic"))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(replies[0], replies[1]);
        let saved = ModelPlanner {
            command: "none".into(),
            ..planner()
        };
        assert_eq!(saved.reply("the same prompt")?, replies[0]);
        std::fs::remove_dir_all(&folder)?;
        Ok(())
    }
}
