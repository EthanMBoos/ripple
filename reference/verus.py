#!/usr/bin/env python3
"""Prove the Verus contracts and check that planted faults fail.

    python3 reference/verus.py [--output DIR] [--no-faults] [--retain]

Runs `cargo verus verify -p delivery`, which proves every contract in the
core crate and then the delivery crate's reserve guard on top of them (the
book's "Verifying the engine" chapter says what is proved). Then, unless
--no-faults, each fault in FAULTS is written into the production source, its
check is rerun and must fail, and the file is restored. A proof fault reruns
the verification; a compile fault, one the types alone must catch, reruns
`cargo check`; a test fault, one in a trusted plugin call that no proof
covers, reruns the two crates' tests. A contract, type, or test that survives
its fault is too weak, so a surviving fault fails this check. The record names
the Verus version, how many items it verified, the time each step took, and
the hash of each source file that carries a contract or that Verus skips;
--retain copies it to
paper/data/verification/. Needs `cargo-verus` and `verus` on the PATH (the
Rust book's verification chapter says where they come from); standard library
only.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

from project import CORE_PACKAGE, DELIVERY_PACKAGE, ROOT, RUNS, allocate

CORE = ROOT / "crates/core/src"
DELIVERY = ROOT / "crates/delivery/src"
PACKAGES = (CORE_PACKAGE, DELIVERY_PACKAGE)
RETAINED = ROOT / "paper/data/verification/record.json"
CHECKS = ("proof", "compile", "test")


class Fault:
    """A planted defect: `old` becomes `new` in `path` under the package's src
    (the core crate's unless `package` says otherwise), and `check` ("proof",
    "compile", or "test") must then fail."""

    def __init__(self, name, path, old, new, check, package=CORE_PACKAGE):
        if check not in CHECKS:
            raise ValueError(f"{name}: check must be one of {CHECKS}")
        self.name, self.path, self.old, self.new, self.check = name, path, old, new, check
        self.package = package


def source_dir(package):
    """Where a package's sources are."""
    return CORE if package == CORE_PACKAGE else DELIVERY


FAULTS = [
    Fault("rate reload deleted", "common/loop_rate.rs",
          "        if due {\n            self.remaining_s += self.period_s;\n        }\n",
          "",
          "proof"),
    Fault("elapsed time not reset when the plugin runs", "common/loop_rate.rs",
          "            let elapsed_s = self.elapsed_s;\n            self.elapsed_s = 0.0;\n",
          "            let elapsed_s = self.elapsed_s;\n",
          "proof"),
    Fault("countdown zeroed for every rate", "common/loop_rate.rs",
          "        if self.period_s == 0.0 {\n",
          "        if self.period_s >= 0.0 {\n",
          "proof"),
    Fault("due compared with a whole step instead of half", "common/loop_rate.rs",
          "        let due = self.remaining_s < dt_s / 2.0;\n",
          "        let due = self.remaining_s < dt_s;\n",
          "proof"),
    Fault("reading an undeclared input is accepted", "ports.rs",
          "        if !self.ports.inputs.contains_key(name) {\n"
          "            bail!(\"undeclared input '{name}'\");\n        }\n",
          "",
          "proof"),
    Fault("nonfinite output accepted", "ports.rs",
          "        if !value.is_finite() {\n            bail!(\"nonfinite output '{name}'\");\n        }\n",
          "",
          "proof"),
    Fault("writing an undeclared output is accepted", "ports.rs",
          "        if !self.ports.outputs.contains_key(name) {\n"
          "            bail!(\"undeclared output '{name}'\");\n        }\n",
          "",
          "proof"),
    Fault("a write to a seeded port is dropped", "ports.rs",
          "            Some(slot) => *slot = value,\n",
          "            Some(_) => {}\n",
          "proof"),
    Fault("a write lands under another name", "ports.rs",
          "                self.signals.insert(name.to_owned(), value);\n",
          "                self.signals.insert(format!(\"{name}_\"), value);\n",
          "proof"),
    Fault("a plugin steps when it is not due", "entity/plugin_stack.rs",
          "        let Some(elapsed_s) = self.rate.due(time.dt_s) else {\n"
          "            return Ok(());\n        };\n",
          "        let elapsed_s = self.rate.due(time.dt_s).unwrap_or(time.dt_s);\n",
          "proof"),
    Fault("the plugin sees the tick's dt instead of the time since it last ran",
          "entity/plugin_stack.rs",
          "            dt_s: elapsed_s,\n",
          "            dt_s: time.dt_s,\n",
          "proof"),
    Fault("the plugin call is deleted", "entity/plugin_stack.rs",
          "        // on a `Result` no contract, because `anyhow` bounds it by a private trait.\n"
          "        self.plugin\n            .step(&mut context, &mut io)\n",
          "        Ok::<(), anyhow::Error>(())\n",
          "proof"),
    Fault("the plugin's error is dropped", "entity/plugin_stack.rs",
          "        // on a `Result` no contract, because `anyhow` bounds it by a private trait.\n"
          "        self.plugin\n            .step(&mut context, &mut io)\n"
          "            .map_err(|error| error.context(format!(\"step '{}' on entity {}\", self.name, entity.id)))\n",
          "        let _ = self.plugin.step(&mut context, &mut io);\n        Ok(())\n",
          "proof"),
    Fault("publish stores every reading in one slot", "plugin/sensor/readings.rs",
          "            sensor.to_owned(),\n",
          "            String::new(),\n",
          "proof"),
    Fault("a reading of another type reads as missing", "plugin/sensor/readings.rs",
          "        Ok(Some(value.downcast_ref::<T>().ok_or_else(|| {\n"
          "            anyhow::anyhow!(\"reading from '{sensor}' has a different type\")\n"
          "        })?))\n",
          "        Ok(value.downcast_ref::<T>())\n",
          "proof"),
    Fault("any fresh reading makes every sensor fresh", "plugin/sensor/readings.rs",
          "        self.fresh.contains(sensor)\n",
          "        !self.fresh.is_empty()\n",
          "proof"),
    Fault("deliver marks no sensor fresh", "plugin/sensor/readings.rs",
          "            self.fresh.insert(sensor.clone());\n",
          "            let _ = sensor;\n",
          "proof"),
    Fault("deliver leaves the readings pending", "plugin/sensor/readings.rs",
          "        self.values.append(&mut pending.values);\n",
          "        let _ = pending;\n",
          "proof"),
    Fault("clear_fresh keeps every sensor fresh", "plugin/sensor/readings.rs",
          "        self.fresh.clear();\n",
          "        let _ = self;\n",
          "proof"),
    Fault("a subscription that changes type is accepted", "pubsub/messages.rs",
          "        if previous.is_some() && previous != Some(&message_type) {\n",
          "        if previous.is_some() && previous == Some(&message_type) {\n",
          "proof"),
    Fault("receive accepts a subscription of another type", "pubsub/messages.rs",
          "        if self.subscriptions.get(&channel) != Some(&MessageType::of::<T>()) {\n",
          "        if self.subscriptions.get(&channel).is_none() {\n",
          "proof"),
    Fault("receive drains the wrong channel", "pubsub/messages.rs",
          "        open_all(self.inbox.remove(&channel).unwrap_or_default())\n",
          "        open_all(self.inbox.remove(&(topic.to_owned(), network.to_owned())).unwrap_or_default())\n",
          "proof"),
    Fault("receive returns nothing and leaves the queue in place", "pubsub/messages.rs",
          "        open_all(self.inbox.remove(&channel).unwrap_or_default())\n",
          "        open_all(Vec::new())\n",
          "proof"),
    Fault("receive returns the queue out of order", "pubsub/messages.rs",
          "        messages.push(envelope.open()?);\n",
          "        messages.insert(0, envelope.open()?);\n",
          "proof"),
    Fault("receive refuses every valid receive", "pubsub/messages.rs",
          "        open_all(self.inbox.remove(&channel).unwrap_or_default())\n",
          "        bail!(\"refused: {network}/{topic}\")\n",
          "proof"),
    Fault("open drops the sender", "pubsub/messages.rs",
          "            sender: self.sender,\n",
          "            sender: MessageEndpoint { entity_id: None, plugin: String::new() },\n",
          "proof"),
    Fault("a delayed message is delivered early", "pubsub/routing.rs",
          "        if message.envelope.delivered_at_s <= self.time.time_s + 1e-12 {\n",
          "        if true {\n",
          "proof"),
    Fault("a delivered message keeps its scheduled time", "pubsub/routing.rs",
          "            message.envelope.delivered_at_s = self.time.time_s;\n",
          "",
          "proof"),
    Fault("a due message stays on the schedule", "pubsub/routing.rs",
          "            receiver.enqueue(message.channel, message.envelope);\n",
          "            self.scheduled.push(message);\n",
          "proof"),
    Fault("a message is scheduled for its sender", "pubsub/routing.rs",
          "            receiver: link.receiver.clone(),\n",
          "            receiver: link.sender.clone(),\n",
          "proof"),
    Fault("a negative delay is accepted", "pubsub/routing.rs",
          "        if !(delay_s.is_finite() && delay_s >= 0.0) {\n",
          "        if !delay_s.is_finite() {\n",
          "proof"),
    Fault("a publication on another network is taken", "pubsub/messages.rs",
          "            if publication.channel.0.as_str() == network {\n",
          "            if true {\n",
          "proof"),
    Fault("the network's publications are taken out of order", "pubsub/messages.rs",
          "                publications.push((sender, publication));\n",
          "                publications.insert(0, (sender, publication));\n",
          "proof"),
    Fault("a mailbox's publications are never collected", "pubsub/routing.rs",
          "        mailbox\n            .messages\n            .take_publications(network, mailbox.endpoint, &mut publications);\n",
          "        let _ = mailbox;\n",
          "proof"),
    Fault("a subscriber is indexed in the wrong group", "pubsub/routing.rs",
          "    let entity = if within_entity {\n        mailbox.endpoint.entity_id\n    } else {\n        None\n    };\n",
          "    let entity = if within_entity {\n        None\n    } else {\n        mailbox.endpoint.entity_id\n    };\n",
          "proof"),
    Fault("every subscriber is indexed as the first mailbox", "pubsub/routing.rs",
          "        add_subscriber(subscribers, (entity, channel), (index, *expected_type));\n",
          "        add_subscriber(subscribers, (entity, channel), (0, *expected_type));\n",
          "proof"),
    Fault("sequence numbers start at one", "pubsub/routing.rs",
          "                sequences.insert(key, 1);\n                0\n",
          "                sequences.insert(key, 1);\n                1\n",
          "proof"),
    Fault("a world plugin's publication is routed within an entity", "pubsub/routing.rs",
          "            (true, None) => continue,\n",
          "            (true, None) => None,\n",
          "proof"),
    Fault("a publication of another type is offered", "pubsub/routing.rs",
          "        if expected_type != publication.payload.message_type() {\n",
          "        if false && expected_type != publication.payload.message_type() {\n",
          "proof"),
    Fault("a dropped link is scheduled", "pubsub/routing.rs",
          "            Delivery::Drop => {}\n",
          "            Delivery::Drop => phase.schedule(&link, publication, 0.0)?,\n",
          "proof"),
    Fault("an endpoint's last mailbox receives", "pubsub/routing.rs",
          "        if !receivers.contains_key(&endpoint) {\n            receivers.insert(endpoint, index);\n        }\n",
          "        receivers.insert(endpoint, index);\n",
          "proof"),
    Fault("a message to a removed receiver is kept", "pubsub/routing.rs",
          "            // A removed entity cannot receive delayed messages.\n            continue;\n",
          "            phase.scheduled.push(message);\n            continue;\n",
          "proof"),
    Fault("a network routes twice in a phase", "pubsub/routing.rs",
          "        if self.routed {\n            bail!(\"network may route once per phase\");\n        }\n",
          "",
          "proof"),
    Fault("scheduled messages are never delivered", "pubsub/routing.rs",
          "        deliver_scheduled(&mut phase, self.mailboxes);\n",
          "        let _ = &mut phase;\n",
          "proof"),
    Fault("an active entity is skipped by the phase", "simulation.rs",
          "        if !entity.active {\n",
          "        if !entity.active || entity.id > 0 {\n",
          "proof"),
    Fault("an entity runs the phase twice", "simulation.rs",
          "        if let Err(error) = entity.step_phase(phase) {\n",
          "        let _ = entity.step_phase(phase);\n"
          "        if let Err(error) = entity.step_phase(phase) {\n",
          "proof"),
    Fault("a phase error is dropped", "simulation.rs",
          "        if let Err(error) = entity.step_phase(phase) {\n"
          "            first_error.get_or_insert(error);\n",
          "        if let Err(error) = entity.step_phase(phase) {\n"
          "            let _ = error;\n",
          "proof"),
    Fault("an inactive entity runs the phase", "simulation.rs",
          "        if !entity.active {\n",
          "        if entity.health < i32::MIN {\n",
          "proof"),
    Fault("the phase stops at the first error", "simulation.rs",
          "        if let Err(error) = entity.step_phase(phase) {\n"
          "            first_error.get_or_insert(error);\n",
          "        if let Err(error) = entity.step_phase(phase) {\n"
          "            return Err(error);\n",
          "proof"),
    Fault("a dead entity is kept", "simulation.rs",
          "    entities.retain(Entity::is_alive);\n",
          "",
          "proof"),
    Fault("a live entity is removed", "entity.rs",
          "        self.health > 0\n",
          "        self.health > 1\n",
          "proof"),
    Fault("a close error is dropped", "simulation.rs",
          "        if let Err(error) = entity.close(time) {\n"
          "            first_error.get_or_insert(error);\n",
          "        if let Err(error) = entity.close(time) {\n"
          "            let _ = error;\n",
          "proof"),
    Fault("a live entity is closed", "simulation.rs",
          "        if entity.health > 0 {\n            continue;\n",
          "        if entity.health > i32::MAX {\n            continue;\n",
          "proof"),
    Fault("a dead entity is not closed", "simulation.rs",
          "        if entity.health > 0 {\n            continue;\n",
          "        if entity.health > 0 || entity.id > 0 {\n            continue;\n",
          "proof"),
    Fault("two tick phases are swapped", "simulation.rs",
          "        self.estimate(time, &contacts)?;\n"
          "        self.score(time, &contacts, &nearby)?;\n",
          "        self.score(time, &contacts, &nearby)?;\n"
          "        self.estimate(time, &contacts)?;\n",
          "proof"),
    Fault("a tick phase is skipped", "simulation.rs",
          "        self.score(time, &contacts, &nearby)?;\n",
          "",
          "proof"),
    Fault("a tick phase runs twice", "simulation.rs",
          "        self.controllers_and_motion(time)?;\n",
          "        self.controllers_and_motion(time)?;\n"
          "        self.controllers_and_motion(time)?;\n",
          "proof"),
    Fault("time advances twice", "simulation.rs",
          "        self.advance_time();\n        Ok(Some(frame))\n",
          "        self.advance_time();\n        self.advance_time();\n        Ok(Some(frame))\n",
          "proof"),
    Fault("time advances after a failed phase", "simulation.rs",
          "        self.estimate(time, &contacts)?;\n",
          "        if let Err(error) = self.estimate(time, &contacts) {\n"
          "            self.advance_time();\n"
          "            return Err(error);\n"
          "        }\n",
          "proof"),
    Fault("a failed phase's error is dropped", "simulation.rs",
          "        self.estimate(time, &contacts)?;\n",
          "        let _ = self.estimate(time, &contacts);\n",
          "proof"),
    Fault("contacts are taken before the interactions", "simulation.rs",
          "        self.apply_interactions(&mut nearby)?;\n"
          "        nearby.refresh(&self.entities);\n"
          "        let contacts = self.snapshot().entities;\n",
          "        let contacts = self.snapshot().entities;\n"
          "        self.apply_interactions(&mut nearby)?;\n"
          "        nearby.refresh(&self.entities);\n",
          "proof"),
    Fault("the frame is taken before the entities start", "simulation.rs",
          "        self.start_entities()?;\n        let mut frame = self.snapshot();\n",
          "        let mut frame = self.snapshot();\n        self.start_entities()?;\n",
          "proof"),
    Fault("a finished run keeps stepping", "simulation.rs",
          "        if self.finished {\n            return Ok(None);\n        }\n",
          "",
          "proof"),
    Fault("a failed step is not marked finished", "simulation.rs",
          "            self.finished = true;\n",
          "",
          "proof"),
    Fault("motion runs before the controllers in a substep", "simulation.rs",
          "            run_phase(\n"
          "                &mut self.entities,\n"
          "                Phase::Controllers {\n"
          "                    time: substep,\n"
          "                    after_motion: i > 0,\n"
          "                },\n"
          "            )?;\n"
          "            run_phase(\n"
          "                &mut self.entities,\n"
          "                Phase::Motion {\n"
          "                    time: substep,\n"
          "                    terrain: self.terrain.as_ref(),\n"
          "                },\n"
          "            )?;\n",
          "            run_phase(\n"
          "                &mut self.entities,\n"
          "                Phase::Motion {\n"
          "                    time: substep,\n"
          "                    terrain: self.terrain.as_ref(),\n"
          "                },\n"
          "            )?;\n"
          "            run_phase(\n"
          "                &mut self.entities,\n"
          "                Phase::Controllers {\n"
          "                    time: substep,\n"
          "                    after_motion: i > 0,\n"
          "                },\n"
          "            )?;\n",
          "proof"),
    Fault("the first substep's controllers are told they follow motion", "simulation.rs",
          "                    after_motion: i > 0,\n",
          "                    after_motion: true,\n",
          "proof"),
    Fault("no substep's controllers are told they follow motion", "simulation.rs",
          "                    after_motion: i > 0,\n",
          "                    after_motion: false,\n",
          "proof"),
    Fault("the substep time does not advance", "simulation.rs",
          "            substep.time_s += substep.dt_s;\n",
          "",
          "proof"),
    Fault("the substeps run once", "simulation.rs",
          "        for i in it: 0..self.config.run.motion_multiplier\n",
          "        for i in it: 0..1\n",
          "proof"),
    Fault("a failed controllers phase does not stop the substeps", "simulation.rs",
          "                    after_motion: i > 0,\n                },\n            )?;\n",
          "                    after_motion: i > 0,\n                },\n            )\n            .ok();\n",
          "proof"),
    Fault("the substeps run with the whole step", "simulation.rs",
          "            dt_s: time.dt_s / self.config.run.motion_multiplier as f64,\n",
          "            dt_s: time.dt_s,\n",
          "proof"),
    Fault("belief follows truth before the first substep too", "entity.rs",
          "        if after_motion {\n            self.plugins.follow_perfect_state(&self.truth);\n        }\n",
          "        self.plugins.follow_perfect_state(&self.truth);\n",
          "proof"),
    Fault("belief never follows truth between substeps", "entity.rs",
          "        if after_motion {\n            self.plugins.follow_perfect_state(&self.truth);\n        }\n",
          "",
          "proof"),
    Fault("belief follows truth without a perfect state measurement", "entity/plugin_stack.rs",
          "        if self.belief_is_perfect_state {\n            self.belief = GaussianState::perfect(*truth);\n",
          "        if self.belief_is_perfect_state || !self.closed {\n            self.belief = GaussianState::perfect(*truth);\n",
          "proof"),
    Fault("the controllers are skipped after belief follows truth", "entity.rs",
          "        self.plugins.controllers(self.info(), time)\n    }\n",
          "        if after_motion {\n            return Ok(());\n        }\n        self.plugins.controllers(self.info(), time)\n    }\n",
          "proof"),
    Fault("the motion model's inputs are not recorded for localization", "entity/plugin_stack.rs",
          "        record_commands(&self.motion.ports, &self.signals, &mut self.command_values)?;\n",
          "",
          "proof"),
    Fault("a motion input's value is not recorded", "entity/plugin_stack.rs",
          "        values.push(*value);\n",
          "        let _ = value;\n",
          "proof"),
    Fault("the substep time is not recorded for localization", "entity/plugin_stack.rs",
          "        self.command_times.push(time);\n",
          "",
          "proof"),
    Fault("the motion model runs without the applied forces", "entity/plugin_stack.rs",
          "            terrain,\n            applied_forces_world_n,\n        };\n",
          "            terrain,\n            applied_forces_world_n: &[],\n        };\n",
          "proof"),
    Fault("the motion model runs without the terrain", "entity/plugin_stack.rs",
          "            terrain,\n            applied_forces_world_n,\n        };\n",
          "            terrain: None,\n            applied_forces_world_n,\n        };\n",
          "proof"),
    Fault("the motion mailbox is not stamped with the substep time", "entity/plugin_stack.rs",
          "        self.messages.time_s = time.time_s;\n        let mut context = MotionContext {\n",
          "        let mut context = MotionContext {\n",
          "proof"),
    Fault("a failed motion step is reported as Ok", "entity/plugin_stack.rs",
          "        self.motion\n            .motion_step(entity, time, truth, applied_forces_world_n, terrain, &mut self.signals)\n",
          "        let _ = self\n            .motion\n            .motion_step(entity, time, truth, applied_forces_world_n, terrain, &mut self.signals);\n        Ok(())\n",
          "proof"),
    Fault("a sensor not due is stepped", "entity/plugin_stack.rs",
          "        if self.rate.due(time.dt_s).is_none() {\n            return Ok(());\n        }\n",
          "        let _ = self.rate.due(time.dt_s);\n",
          "proof"),
    Fault("a sensor's mailbox is not stamped with the tick's time", "entity/plugin_stack.rs",
          "        self.messages.time_s = time.time_s;\n        // A sensor's dt stays the tick's",
          "        // A sensor's dt stays the tick's",
          "proof"),
    Fault("a sensor is told another sensor's name", "entity/plugin_stack.rs",
          "            name: &self.instance_name,\n",
          "            name: &self.name,\n",
          "proof"),
    Fault("a sensor is skipped", "entity/plugin_stack.rs",
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n",
          "        let _ = slot;\n",
          "proof"),
    Fault("a sensor runs twice", "entity/plugin_stack.rs",
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n",
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n"
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n",
          "proof"),
    Fault("a failing sensor's error is dropped", "entity/plugin_stack.rs",
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n",
          "        let _ = slot.sensor_step(entity, time, truth, contacts_truth, pending);\n",
          "proof"),
    Fault("a sensor's readings are dropped before the next sensor runs", "entity/plugin_stack.rs",
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n",
          "        slot.sensor_step(entity, time, truth, contacts_truth, pending)?;\n"
          "        pending.values.clear();\n",
          "proof"),
    Fault("the readings are not delivered before estimation", "entity/plugin_stack.rs",
          "    {\n"
          "        self.readings.deliver(&mut self.pending);\n"
          "        let perfect_state = GaussianState::perfect(*truth);\n"
          "        let commands = Commands {\n",
          "    {\n"
          "        let perfect_state = GaussianState::perfect(*truth);\n"
          "        let commands = Commands {\n",
          "proof"),
    Fault("tracking is given localization's sources", "entity/plugin_stack.rs",
          "            estimator_inputs(&self.tracking_sources, &self.readings, &perfect_state, perfect_contacts, time),\n",
          "            estimator_inputs(&self.localization_sources, &self.readings, &perfect_state, perfect_contacts, time),\n",
          "proof"),
    Fault("tracking is given the truth as its pose", "entity/plugin_stack.rs",
          "            estimator_inputs(&self.tracking_sources, &self.readings, &perfect_state, perfect_contacts, time),\n"
          "            &self.belief,\n",
          "            estimator_inputs(&self.tracking_sources, &self.readings, &perfect_state, perfect_contacts, time),\n"
          "            &perfect_state,\n",
          "proof"),
    Fault("an estimate is made for the start of the step", "entity/plugin_stack.rs",
          "        now_s: time.time_s + time.dt_s,\n",
          "        now_s: time.time_s,\n",
          "proof"),
    Fault("the fresh marks are kept after estimation", "entity/plugin_stack.rs",
          "        self.readings.clear_fresh();\n"
          "        self.command_values.clear();\n",
          "        self.command_values.clear();\n",
          "proof"),
    Fault("the commands are kept after estimation", "entity/plugin_stack.rs",
          "        self.command_values.clear();\n"
          "        self.command_times.clear();\n",
          "        self.command_times.clear();\n",
          "proof"),
    Fault("the localization mailbox is not stamped with the phase time", "entity/plugin_stack.rs",
          "        self.messages.time_s = time.time_s;\n"
          "        let mut context = LocalizationContext {\n",
          "        let mut context = LocalizationContext {\n",
          "proof"),
    Fault("a failed localization step is reported as Ok", "entity/plugin_stack.rs",
          "            commands,\n"
          "            &mut self.belief,\n"
          "        )?;\n",
          "            commands,\n"
          "            &mut self.belief,\n"
          "        )\n"
          "        .ok();\n",
          "proof"),
    Fault("the autonomy phase runs the controllers", "entity.rs",
          "            Phase::Autonomy(time) => self.step_autonomy(time),\n",
          "            Phase::Autonomy(time) => self.step_controller(time, false),\n",
          "proof"),
    Fault("an agent slot is skipped", "entity/plugin_stack.rs",
          "    {\n"
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "    {\n"
          "        if entity.id < 0 {\n"
          "            continue;\n"
          "        }\n"
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "proof"),
    Fault("an agent slot runs twice", "entity/plugin_stack.rs",
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n"
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "proof"),
    Fault("a failing agent slot's error is dropped", "entity/plugin_stack.rs",
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "        let _ = slot.agent_step(entity, time, state, tracks, signals);\n",
          "proof"),
    Fault("the autonomy phase steps the controller slots", "entity/plugin_stack.rs",
          "        step_agents(\n            &mut self.autonomies,\n",
          "        step_agents(\n            &mut self.controllers,\n",
          "proof"),
    Fault("the autonomy phase closes the stack", "entity/plugin_stack.rs",
          "        step_agents(\n            &mut self.autonomies,\n",
          "        self.closed = true;\n"
          "        step_agents(\n            &mut self.autonomies,\n",
          "proof"),
    Fault("the autonomy phase deactivates the entity", "entity.rs",
          "        self.plugins.autonomy(self.info(), time)\n",
          "        self.active = false;\n"
          "        self.plugins.autonomy(self.info(), time)\n",
          "proof"),
    Fault("the port board is cleared before each agent slot", "entity/plugin_stack.rs",
          "    {\n"
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "    {\n"
          "        signals.clear();\n"
          "        slot.agent_step(entity, time, state, tracks, signals)?;\n",
          "proof"),
    Fault("an agent's mailbox is not stamped with the phase time", "entity/plugin_stack.rs",
          "            ..time\n"
          "        };\n"
          "        self.messages.time_s = time.time_s;\n",
          "            ..time\n"
          "        };\n",
          "proof"),
    Fault("the autonomy phase steps its plugins on a copy of the port board", "entity/plugin_stack.rs",
          "            tracks,\n"
          "            &mut self.signals,\n"
          "        )\n"
          "    }\n"
          "    pub fn controllers(",
          "            tracks,\n"
          "            &mut self.signals.clone(),\n"
          "        )\n"
          "    }\n"
          "    pub fn controllers(",
          "proof"),
    Fault("an interaction's mailbox is not stamped", "simulation/world_plugins.rs",
          "        self.messages.time_s = time.time_s;\n        let mut context = InteractionContext {\n",
          "        let mut context = InteractionContext {\n",
          "proof"),
    Fault("the stop flag is raised on Applied", "simulation/world_plugins.rs",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n"
          "        stop |= update == Update::Stop;\n",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n"
          "        stop |= update == Update::Applied;\n",
          "proof"),
    Fault("an interaction's stop request is dropped", "simulation/world_plugins.rs",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n"
          "        stop |= update == Update::Stop;\n",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n",
          "proof"),
    Fault("a failed interaction is reported as Applied", "simulation/world_plugins.rs",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n",
          "        let update = match slot.interaction_step(time, entities, nearby, terrain, events) {\n"
          "            Ok(update) => update,\n"
          "            Err(_) => Update::Applied,\n"
          "        };\n",
          "proof"),
    Fault("an interaction runs twice", "simulation/world_plugins.rs",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n",
          "        let update = slot.interaction_step(time, entities, nearby, terrain, events)?;\n"
          "        slot.interaction_step(time, entities, nearby, terrain, events)?;\n",
          "proof"),
    Fault("interactions are skipped before the start time", "simulation/world_plugins.rs",
          "        nearby.refresh(entities);\n        let update = slot.interaction_step(",
          "        if time.tick < 0 {\n            continue;\n        }\n"
          "        nearby.refresh(entities);\n        let update = slot.interaction_step(",
          "proof"),
    Fault("the applied forces are kept", "simulation.rs",
          "        clear_applied_forces(&mut self.entities);\n",
          "",
          "proof"),
    Fault("the dead are left in play", "simulation.rs",
          "        deactivate_dead(&mut self.entities, self.time_s, &mut self.messages, &mut self.events);\n",
          "",
          "proof"),
    Fault("the interactions' stop request is dropped", "simulation.rs",
          "        self.world_stop |= self.world.interactions(\n",
          "        self.world.interactions(\n",
          "proof"),
    Fault("the mailbox is not stamped before the removals", "simulation.rs",
          "        self.messages.time_s = self.time_s;\n        deactivate_dead(",
          "        deactivate_dead(",
          "proof"),
    Fault("a dead entity stays active", "simulation.rs",
          "        entity.deactivate();\n        let event = Event {\n            time_s,\n",
          "        let event = Event {\n            time_s,\n",
          "proof"),
    Fault("a removal is not announced", "simulation.rs",
          "        messages.publish_engine_event(\"GlobalNetwork\", event.kind.topic(), event.clone());\n",
          "",
          "proof"),
    Fault("a removal is announced on the wrong network", "simulation.rs",
          "        messages.publish_engine_event(\"GlobalNetwork\", event.kind.topic(), event.clone());\n",
          "        messages.publish_engine_event(\"LocalNetwork\", event.kind.topic(), event.clone());\n",
          "proof"),
    Fault("a removal is announced as a generation", "simulation.rs",
          "        messages.publish_engine_event(\"GlobalNetwork\", event.kind.topic(), event.clone());\n",
          "        messages.publish_engine_event(\n"
          "            \"GlobalNetwork\",\n"
          "            EventKind::EntityGenerated.topic(),\n"
          "            event.clone(),\n"
          "        );\n",
          "proof"),
    Fault("a removal event names no entity", "simulation.rs",
          "            entity_ids: vec![entity.id],\n        };\n        messages.publish_engine_event(",
          "            entity_ids: Vec::new(),\n        };\n        messages.publish_engine_event(",
          "proof"),
    Fault("a removal event is not recorded", "simulation.rs",
          "        messages.publish_engine_event(\"GlobalNetwork\", event.kind.topic(), event.clone());\n"
          "        events.push(event);\n",
          "        messages.publish_engine_event(\"GlobalNetwork\", event.kind.topic(), event.clone());\n",
          "proof"),
    Fault("an entity with one health point is removed", "simulation.rs",
          "        if entity.health > 0 || !entity.active {\n",
          "        if entity.health > 1 || !entity.active {\n",
          "proof"),
    Fault("an inactive dead entity is removed again", "simulation.rs",
          "        if entity.health > 0 || !entity.active {\n",
          "        if entity.health > 0 {\n",
          "proof"),
    Fault("a publication is sent at time zero", "pubsub/messages.rs",
          "            .push(Publication::new(network, topic, self.time_s, Payload::new(value)));\n",
          "            .push(Publication::new(network, topic, 0.0, Payload::new(value)));\n",
          "proof"),
    Fault("a publication's network and topic are swapped", "pubsub/messages.rs",
          "            channel: (network.to_owned(), topic.to_owned()),\n            sent_at_s,\n",
          "            channel: (topic.to_owned(), network.to_owned()),\n            sent_at_s,\n",
          "proof"),
    Fault("a cloned event forgets its entities", "simulation.rs",
          "            entity_ids: self.entity_ids.clone(),\n",
          "            entity_ids: Vec::new(),\n",
          "proof"),
    Fault("an event's topic is another kind's", "simulation.rs",
          "            Self::EntityRemoved => \"EntityRemoved\",\n",
          "            Self::EntityRemoved => \"EntityGenerated\",\n",
          "proof"),
    Fault("a metrics plugin's mailbox is not stamped", "simulation/world_plugins.rs",
          "        self.messages.time_s = time.time_s;\n        let mut context = WorldContext {\n",
          "        let mut context = WorldContext {\n",
          "proof"),
    Fault("the metrics stop flag is raised on Applied", "simulation/world_plugins.rs",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n"
          "        stop |= update == Update::Stop;\n",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n"
          "        stop |= update == Update::Applied;\n",
          "proof"),
    Fault("a metrics plugin's stop request is dropped", "simulation/world_plugins.rs",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n"
          "        stop |= update == Update::Stop;\n",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n",
          "proof"),
    Fault("a failed metrics plugin is reported as Applied", "simulation/world_plugins.rs",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n",
          "        let update = match slot.metrics_step(time, contacts, teams, terrain) {\n"
          "            Ok(update) => update,\n"
          "            Err(_) => Update::Applied,\n"
          "        };\n",
          "proof"),
    Fault("a metrics plugin runs twice", "simulation/world_plugins.rs",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n"
          "        slot.metrics_step(time, contacts, teams, terrain)?;\n",
          "proof"),
    Fault("metrics are skipped before the start time", "simulation/world_plugins.rs",
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n",
          "        if time.tick < 0 {\n            continue;\n        }\n"
          "        let update = slot.metrics_step(time, contacts, teams, terrain)?;\n",
          "proof"),
    Fault("the sensor phase runs twice", "simulation.rs",
          "        run_phase(&mut self.entities, Phase::Sensors { time, truth })?;\n",
          "        run_phase(&mut self.entities, Phase::Sensors { time, truth })?;\n"
          "        run_phase(&mut self.entities, Phase::Sensors { time, truth })?;\n",
          "proof"),
    Fault("a failed sensor phase still returns the index", "simulation.rs",
          "        run_phase(&mut self.entities, Phase::Sensors { time, truth })?;\n",
          "        let _ = run_phase(&mut self.entities, Phase::Sensors { time, truth });\n",
          "proof"),
    Fault("the sensors' index is built from no entities", "simulation.rs",
          "        let nearby = SpatialIndex::build(self.config.spatial_lookup, &self.entities);\n",
          "        let nearby = SpatialIndex::build(self.config.spatial_lookup, &[]);\n",
          "proof"),
    Fault("the perfect detections are not kept for the entities that start next", "simulation.rs",
          "        self.perfect_contacts = perfect_contacts(contacts, time.time_s + time.dt_s);\n"
          "        run_phase(\n"
          "            &mut self.entities,\n"
          "            Phase::Estimate {\n"
          "                time,\n"
          "                perfect_contacts: &self.perfect_contacts,\n",
          "        let perfect_contacts = perfect_contacts(contacts, time.time_s + time.dt_s);\n"
          "        run_phase(\n"
          "            &mut self.entities,\n"
          "            Phase::Estimate {\n"
          "                time,\n"
          "                perfect_contacts: &perfect_contacts,\n",
          "proof"),
    Fault("the estimators see the last tick's perfect detections", "simulation.rs",
          "        self.perfect_contacts = perfect_contacts(contacts, time.time_s + time.dt_s);\n"
          "        run_phase(\n"
          "            &mut self.entities,\n"
          "            Phase::Estimate {\n"
          "                time,\n"
          "                perfect_contacts: &self.perfect_contacts,\n"
          "            },\n"
          "        )\n"
          "    }\n",
          "        let result = run_phase(\n"
          "            &mut self.entities,\n"
          "            Phase::Estimate {\n"
          "                time,\n"
          "                perfect_contacts: &self.perfect_contacts,\n"
          "            },\n"
          "        );\n"
          "        self.perfect_contacts = perfect_contacts(contacts, time.time_s + time.dt_s);\n"
          "        result\n"
          "    }\n",
          "proof"),
    Fault("the metrics' stop request is dropped", "simulation.rs",
          "        self.world_stop |= self.world.metrics(\n",
          "        self.world.metrics(\n",
          "proof"),
    Fault("the metrics see no teams", "simulation.rs",
          "            TruthContacts::new(contacts, nearby),\n            &self.entity_teams,\n",
          "            TruthContacts::new(contacts, nearby),\n            &BTreeMap::new(),\n",
          "proof"),
    Fault("an entity ID in use is reused", "simulation/spawning.rs",
          "        while !self.used_ids.insert(id)\n"
          "            invariant proof::choosing_id(old(self), self)\n"
          "        {\n"
          "            // `with_context` rather than `context`: Verus has a contract for the former.\n"
          "            id = self\n"
          "                .used_ids\n"
          "                .last()\n"
          "                .copied()\n"
          "                .unwrap_or(0)\n"
          "                .checked_add(1)\n"
          "                .with_context(|| \"entity ID exhausted\")?;\n"
          "        }\n",
          "        self.used_ids.insert(id);\n",
          "proof"),
    Fault("a new entity's group is zero", "simulation/spawning.rs",
          "            i32::try_from(group_index).map_err(|_| anyhow::anyhow!(\"entity block ID exceeds i32\"))?,\n",
          "            0,\n",
          "proof"),
    Fault("a new entity is born at time zero", "simulation/spawning.rs",
          "            StepTime {\n                time_s: self.time_s,\n                dt_s: 0.0,\n",
          "            StepTime {\n                time_s: 0.0,\n                dt_s: 0.0,\n",
          "proof"),
    Fault("a new entity's team is not recorded", "simulation/spawning.rs",
          "        self.entity_teams.insert(id, entity.team_id);\n",
          "",
          "proof"),
    Fault("a generation is announced as a removal", "simulation/spawning.rs",
          "            kind: EventKind::EntityGenerated,\n",
          "            kind: EventKind::EntityRemoved,\n",
          "proof"),
    Fault("the mailbox is not stamped before the announcement", "simulation/spawning.rs",
          "        self.messages.time_s = self.time_s;\n",
          "",
          "proof"),
    Fault("the generation event is not recorded", "simulation/spawning.rs",
          "        self.events.push(event);\n        self.entities.push(entity);\n",
          "        self.entities.push(entity);\n",
          "proof"),
    Fault("a built entity is dropped", "simulation/spawning.rs",
          "        self.events.push(event);\n        self.entities.push(entity);\n",
          "        self.events.push(event);\n",
          "proof"),
    Fault("a batch builds one entity too few", "simulation/spawning.rs",
          "        for _ in it: 0..count\n",
          "        for _ in it: 1..count\n",
          "proof"),
    Fault("a failed request's error is dropped", "simulation/spawning.rs",
          "            self.spawn_batch(group_index, &definition, request.count)\n"
          "                .map_err(|error| error.context(request_context(&message)))?;\n",
          "            let _ = self.spawn_batch(group_index, &definition, request.count);\n",
          "proof"),
    Fault("the schedules' batches are built before the requests", "simulation/spawning.rs",
          "        self.spawn_requested()?;\n"
          "        for (group_index, count) in it: self.due_now()\n"
          "            invariant proof::due_so_far(old(self), self, it)\n"
          "        {\n"
          "            // Shared, so the definition can be read while the batch changes\n"
          "            // the rest of the simulation.\n"
          "            let definition = Arc::clone(&self.definitions[group_index]);\n"
          "            self.spawn_batch(group_index, &definition, count)?;\n"
          "        }\n",
          "        for (group_index, count) in it: self.due_now()\n"
          "            invariant proof::due_so_far(old(self), self, it)\n"
          "        {\n"
          "            // Shared, so the definition can be read while the batch changes\n"
          "            // the rest of the simulation.\n"
          "            let definition = Arc::clone(&self.definitions[group_index]);\n"
          "            self.spawn_batch(group_index, &definition, count)?;\n"
          "        }\n"
          "        self.spawn_requested()?;\n",
          "proof"),
    Fault("an entity is started twice", "entity.rs",
          "        if self.started {\n            return Ok(());\n        }\n        self.started = true;\n",
          "        self.started = true;\n",
          "proof"),
    Fault("an entity is never marked started", "entity.rs",
          "        if self.started {\n            return Ok(());\n        }\n        self.started = true;\n",
          "        if self.started {\n            return Ok(());\n        }\n",
          "proof"),
    Fault("a failing start's error is dropped", "simulation.rs",
          "        entity.start(perfect_contacts)?;\n",
          "        let _ = entity.start(perfect_contacts);\n",
          "proof"),
    Fault("the tick is not moved on", "simulation.rs",
          "        self.count_step();\n        self.tick = self.step_count as i64;\n",
          "        self.count_step();\n",
          "proof"),
    Fault("time accumulates instead of being counted", "simulation.rs",
          "        self.time_s = self.config.run.start_s + self.step_count as f64 * self.config.run.dt_s;\n",
          "        self.time_s += self.config.run.dt_s;\n",
          "proof"),
    Fault("time is counted from zero instead of the run's start", "simulation.rs",
          "        self.time_s = self.config.run.start_s + self.step_count as f64 * self.config.run.dt_s;\n",
          "        self.time_s = self.step_count as f64 * self.config.run.dt_s;\n",
          "proof"),
    Fault("the end condition is not refreshed", "simulation.rs",
          "        self.termination = self.reached_end_condition();\n    }\n",
          "    }\n",
          "proof"),
    Fault("scoring runs after a failed estimation", "simulation.rs",
          "        self.estimate(time, &contacts)?;\n        self.score(time, &contacts, &nearby)?;\n",
          "        let estimated = self.estimate(time, &contacts);\n        self.score(time, &contacts, &nearby)?;\n        estimated?;\n",
          "proof"),
    Fault("the dead are removed after a failed scoring", "simulation.rs",
          "        self.score(time, &contacts, &nearby)?;\n        remove_dead(&mut self.entities, time)?;\n",
          "        let scored = self.score(time, &contacts, &nearby);\n        remove_dead(&mut self.entities, time)?;\n        scored?;\n",
          "proof"),
    Fault("a failed estimation is run again", "simulation.rs",
          "        self.estimate(time, &contacts)?;\n",
          "        if self.estimate(time, &contacts).is_err() {\n            self.estimate(time, &contacts)?;\n        }\n",
          "proof"),
    Fault("failed interactions are run again", "simulation.rs",
          "        self.apply_interactions(&mut nearby)?;\n",
          "        if self.apply_interactions(&mut nearby).is_err() {\n            self.apply_interactions(&mut nearby)?;\n        }\n",
          "proof"),
    # The delivery crate's reserve guard.
    Fault("the guard counts every hub's vehicles", "dispatcher/guard.rs",
          "            if status.hub == hub && of_class {\n",
          "            if of_class {\n",
          "proof", DELIVERY_PACKAGE),
    Fault("the guard ignores the class", "dispatcher/guard.rs",
          "                Some(class) => status.card.class == class,\n",
          "                Some(_) => true,\n",
          "proof", DELIVERY_PACKAGE),
    Fault("a vehicle already told to move counts as idle", "dispatcher/guard.rs",
          "                if status.free() && !moving.contains_key(&status.vehicle_id) {\n",
          "                if status.free() {\n",
          "proof", DELIVERY_PACKAGE),
    Fault("a vehicle carrying a request is free", "messages.rs",
          "        self.phase == Phase::Idle && self.request_id.is_none()\n",
          "        self.phase == Phase::Idle\n",
          "proof", DELIVERY_PACKAGE),
    Fault("the guard keeps a third instead of half", "dispatcher/guard.rs",
          "        self.idle.saturating_sub(count as usize) >= self.total.div_ceil(2)\n",
          "        self.idle.saturating_sub(count as usize) >= self.total.div_ceil(3)\n",
          "proof", DELIVERY_PACKAGE),
    Fault("the stock guard ignores what leaves", "dispatcher/guard.rs",
          "    have.saturating_sub(units) >= keep\n",
          "    have >= keep\n",
          "proof", DELIVERY_PACKAGE),
]


def prove(env):
    """(exit code, seconds, output) of `cargo verus verify` on the delivery
    crate, which verifies the core crate it depends on first and reports each.
    The crates' builds are removed first: cargo skips a crate whose sources
    have not changed, and a skipped crate reports no verification."""
    subprocess.run(["cargo", "clean", *packages()], cwd=ROOT, env=env, capture_output=True)
    started = time.perf_counter()
    result = subprocess.run(["cargo", "verus", "verify", "-p", DELIVERY_PACKAGE], cwd=ROOT, env=env,
                            capture_output=True, text=True)
    return result.returncode, round(time.perf_counter() - started, 1), result.stdout + result.stderr


def packages():
    """`-p` arguments naming both crates."""
    return [argument for package in PACKAGES for argument in ("-p", package)]


def verified_count(output):
    """How many items Verus reports verified over the crates, or None when it
    did not get that far."""
    found = re.findall(r"verification results:: (\d+) verified", output)
    return sum(int(count) for count in found) if found else None


def compile_check(env):
    """(exit code, seconds, output) of `cargo check` on both crates."""
    return cargo(["check", *packages()], env)


def test_check(env):
    """(exit code, seconds, output) of both crates' tests."""
    return cargo(["test", *packages()], env)


def cargo(arguments, env):
    started = time.perf_counter()
    result = subprocess.run(["cargo"] + arguments, cwd=ROOT, env=env, capture_output=True, text=True)
    return result.returncode, round(time.perf_counter() - started, 1), result.stdout + result.stderr


def proof_failed(output):
    """True when Verus ran and reports at least one unproved item in either
    crate; a fault that only breaks the build does not count."""
    found = re.findall(r"verification results:: \d+ verified, (\d+) errors", output)
    return any(int(errors) > 0 for errors in found)


def check_fault(fault, env, log):
    """True when the fault leaves a contract unproved, or fails to compile for a
    compile fault, or fails a test for a test fault. The source is restored even
    if the run is interrupted."""
    path = source_dir(fault.package) / fault.path
    original = path.read_text()
    if original.count(fault.old) != 1:
        raise RuntimeError(f"{fault.name}: its text is not found once in {path}")
    try:
        path.write_text(original.replace(fault.old, fault.new))
        if fault.check == "proof":
            code, seconds, output = prove(env)
            failed = proof_failed(output)
        elif fault.check == "compile":
            code, seconds, output = compile_check(env)
            failed = code != 0
        else:
            code, seconds, output = test_check(env)
            failed = code != 0
    finally:
        path.write_text(original)
    log.write_text(output)
    return {"fault": fault.name, "check": fault.check, "file": fault.path,
            "fails_as_required": failed, "seconds": seconds}


SKIPPED_MODULE = re.compile(r"verus_verify\(external\)\)\]\s*(?:pub(?:\(crate\))?\s+)?mod (\w+);")


def source_hashes():
    """Every source file of the two crates carrying a contract or proof text,
    and every module Verus skips (trusted code the proofs lean on), with its
    hash."""
    hashes = {}
    for path in sorted(list(CORE.rglob("*.rs")) + list(DELIVERY.rglob("*.rs"))):
        text = path.read_text()
        if "#[verus_spec(" in text or "verus! {" in text:
            hashes[str(path.relative_to(ROOT))] = hashlib.sha256(text.encode()).hexdigest()
        for name in SKIPPED_MODULE.findall(text):
            skipped = path.parent / path.stem / f"{name}.rs"
            hashes[str(skipped.relative_to(ROOT))] = hashlib.sha256(skipped.read_bytes()).hexdigest()
    return hashes


def versions(env):
    """The lines `verus --version` prints: version, platform, toolchain."""
    result = subprocess.run(["verus", "--version"], cwd=ROOT, env=env, capture_output=True, text=True)
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, help="new directory (default runs/verusNNN)")
    parser.add_argument("--no-faults", action="store_true", help="prove only; plant no faults")
    parser.add_argument("--retain", action="store_true",
                        help="copy the record to paper/data/verification/ when everything passes")
    args = parser.parse_args(argv)
    env = os.environ.copy()
    if shutil.which("cargo-verus", path=env.get("PATH")) is None:
        print("cargo-verus is not on the PATH; see the Rust book's verification chapter",
              file=sys.stderr)
        return 2
    output = args.output.resolve() if args.output else allocate(RUNS, "verus")
    output.mkdir(parents=True, exist_ok=True)

    code, seconds, log = prove(env)
    (output / "prove.log").write_text(log)
    verified = verified_count(log)
    proved = code == 0 and verified is not None
    print(f"{'proved' if proved else 'UNPROVED'} in {seconds} s: {verified} items verified",
          flush=True)
    faults = []
    if proved and not args.no_faults:
        for fault in FAULTS:
            result = check_fault(fault, env, output / (fault.name.replace(" ", "-") + ".log"))
            faults.append(result)
            print(f"{'fails as required' if result['fails_as_required'] else 'SURVIVES'}: "
                  f"{fault.name} ({result['seconds']} s)", flush=True)
    passed = proved and all(result["fails_as_required"] for result in faults)
    record = {"tools": versions(env), "passed": passed, "prove_seconds": seconds,
              "verified_items": verified, "sources": source_hashes(), "faults": faults}
    (output / "record.json").write_text(json.dumps(record, indent=1) + "\n")
    if args.retain and passed:
        RETAINED.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(output / "record.json", RETAINED)
        print(f"retained {RETAINED.relative_to(ROOT)}")
    print(f"{'passed' if passed else 'FAILED'}: {output / 'record.json'}")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
