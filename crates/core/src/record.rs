//! Ripple's self-describing run record. The protobuf source is
//! `proto/record.proto`; its descriptor is embedded in each MCAP file.
//! Keep the message tags below in sync with the schema. After editing it,
//! regenerate the descriptor from the repository root with:
//!
//! ```sh
//! protoc -I crates/core/proto \
//!   --descriptor_set_out=crates/core/proto/record.desc \
//!   crates/core/proto/record.proto
//! ```

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::BufWriter,
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use mcap::{
    Compression, MessageStream, Writer,
    read::ChunkFlattener,
    records::{MessageHeader, Metadata, Record},
    write::WriteOptions,
};
use prost::Message;
use sha2::{Digest, Sha256};

use crate::{
    Terrain,
    entity::EntitySnapshot,
    math::{KinematicState, Quaternion as MathQuaternion, Vec3},
    simcontrol::{Event as SimulationEvent, Simulation, SimulationFrame, TraceSink},
};

const DESCRIPTOR: &[u8] = include_bytes!("../proto/record.desc");

/// Hash the exact header, height grid, and optional image used by a terrain run.
/// Replay uses the scenario's path and rejects changed inputs.
fn terrain_fingerprint(path: &Path) -> Result<String> {
    let header = fs::read(path).with_context(|| format!("read terrain {}", path.display()))?;
    let text = std::str::from_utf8(&header)?;
    let fields: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)?;
    let directory = path.parent().unwrap_or(Path::new("."));
    let heights = fields["heights"]
        .as_str()
        .context("terrain header has no heights file")?;
    let mut hash = Sha256::new();
    hash.update((header.len() as u64).to_le_bytes());
    hash.update(&header);
    for name in [Some(heights), fields["imagery"].as_str()]
        .into_iter()
        .flatten()
    {
        let file = directory.join(name);
        let bytes = fs::read(&file).with_context(|| format!("read terrain {}", file.display()))?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Clone, PartialEq, Message)]
struct Vector3 {
    #[prost(double, tag = "1")]
    x: f64,
    #[prost(double, tag = "2")]
    y: f64,
    #[prost(double, tag = "3")]
    z: f64,
}

#[derive(Clone, PartialEq, Message)]
struct Quaternion {
    #[prost(double, tag = "1")]
    w: f64,
    #[prost(double, tag = "2")]
    x: f64,
    #[prost(double, tag = "3")]
    y: f64,
    #[prost(double, tag = "4")]
    z: f64,
}

#[derive(Clone, PartialEq, Message)]
struct State {
    #[prost(message, optional, tag = "1")]
    position_world_m: Option<Vector3>,
    #[prost(message, optional, tag = "2")]
    orientation_world_from_body: Option<Quaternion>,
    #[prost(message, optional, tag = "3")]
    velocity_world_mps: Option<Vector3>,
    #[prost(message, optional, tag = "4")]
    angular_velocity_world_radps: Option<Vector3>,
}

#[derive(Clone, PartialEq, Message)]
struct EntityDescription {
    #[prost(int32, tag = "1")]
    id: i32,
    #[prost(int32, tag = "2")]
    team_id: i32,
    #[prost(uint32, tag = "3")]
    group_index: u32,
    #[prost(string, tag = "4")]
    group_label: String,
    #[prost(string, tag = "5")]
    visual_model: String,
}

#[derive(Clone, PartialEq, Message)]
struct EntityState {
    #[prost(int32, tag = "1")]
    id: i32,
    #[prost(message, optional, tag = "2")]
    truth: Option<State>,
    #[prost(bool, tag = "3")]
    active: bool,
}

#[derive(Clone, PartialEq, Message)]
struct Frame {
    #[prost(double, tag = "1")]
    time_s: f64,
    #[prost(message, repeated, tag = "2")]
    entities: Vec<EntityState>,
}

#[derive(Clone, PartialEq, Message)]
struct Event {
    #[prost(double, tag = "1")]
    time_s: f64,
    #[prost(string, tag = "2")]
    kind: String,
    #[prost(int32, repeated, tag = "3")]
    entity_ids: Vec<i32>,
}

#[derive(Clone, PartialEq, Message)]
struct Summary {
    #[prost(string, tag = "1")]
    csv: String,
}

/// A single run's channels. The first frame containing an entity supplies its
/// one-time description; later frames contain only changing state.
pub struct RunWriter {
    writer: Writer<BufWriter<fs::File>>,
    origin_s: f64,
    descriptions: BTreeSet<i32>,
    groups: Vec<(String, String)>,
    events_written: usize,
    channels: BTreeMap<&'static str, u16>,
    sequences: BTreeMap<&'static str, u32>,
}

impl RunWriter {
    pub fn create(path: &Path, simulation: &Simulation, trace: bool) -> Result<Self> {
        let scenario = simulation.scenario();
        let terrain_fingerprint = scenario
            .terrain
            .as_deref()
            .map(terrain_fingerprint)
            .transpose()?;
        let file =
            fs::File::create_new(path).with_context(|| format!("create {}", path.display()))?;
        // DEVNOTE: Exporting two identical runs to RRD produced 268,028 and
        // 268,093 bytes. MCAP is the run record because its deterministic
        // bytes let the worker and trace checks compare complete files.
        // DEVNOTE: A traced 4,000-step multirotor run measured 35.9 MB with
        // uncompressed chunks and 4.2 MB with zstd (2026-10-15). Repeating
        // that run and changing to eight workers produced identical bytes.
        // Use zstd to keep observation records small without losing the
        // byte-identical worker check.
        let mut writer = WriteOptions::new()
            .compression(Some(Compression::Zstd))
            .profile("ripple")
            // DEVNOTE: Two identical 3-step plugin_defaults runs differed first
            // at the summary's Schema record: mcap 0.25 repeats schemas and
            // channels from hash maps there. Disabling those repeats leaves
            // their data-section definitions intact and makes bytes stable.
            .repeat_schemas(false)
            .repeat_channels(false)
            .create(BufWriter::new(file))?;
        let mut channels_to_register = vec![
            ("/entities", "ripple.record.EntityDescription"),
            ("/frames", "ripple.record.Frame"),
            ("/events", "ripple.record.Event"),
            ("/summary", "ripple.record.Summary"),
        ];
        if trace {
            channels_to_register.extend([
                ("/beliefs", ""),
                ("/readings", ""),
                ("/deliveries", ""),
                ("/outputs", ""),
            ]);
        }
        let mut channels = BTreeMap::new();
        for (topic, name) in channels_to_register {
            let (schema, encoding) = if name.is_empty() {
                (0, "json")
            } else {
                (writer.add_schema(name, "protobuf", DESCRIPTOR)?, "protobuf")
            };
            let channel = writer.add_channel(schema, topic, encoding, &BTreeMap::new())?;
            channels.insert(topic, channel);
        }
        let origin_s = scenario.run.start_s - scenario.run.dt_s;
        let mut metadata = BTreeMap::from([
            ("initial_time_s".into(), origin_s.to_string()),
            ("scenario".into(), serde_json::to_string(scenario)?),
        ]);
        if let Some(fingerprint) = terrain_fingerprint {
            metadata.insert("terrain_sha256".into(), fingerprint);
        }
        writer.write_metadata(&Metadata {
            name: "ripple.run".into(),
            metadata,
        })?;
        let mut record = Self {
            writer,
            origin_s,
            descriptions: BTreeSet::new(),
            groups: scenario
                .entities
                .iter()
                .map(|group| (group.label.clone(), group.visual_model.clone()))
                .collect(),
            events_written: 0,
            channels,
            sequences: BTreeMap::new(),
        };
        record.new_events(simulation.events())?;
        Ok(record)
    }

    fn message(&mut self, topic: &'static str, time_s: f64, data: &[u8]) -> Result<()> {
        ensure!(time_s.is_finite(), "nonfinite record time");
        let elapsed_ns = ((time_s - self.origin_s) * 1e9).round();
        ensure!(
            elapsed_ns >= 0.0 && elapsed_ns < u64::MAX as f64,
            "record time outside MCAP range"
        );
        let sequence = self.sequences.entry(topic).or_default();
        let header = MessageHeader {
            channel_id: self.channels[topic],
            sequence: *sequence,
            log_time: elapsed_ns as u64,
            publish_time: elapsed_ns as u64,
        };
        *sequence = sequence
            .checked_add(1)
            .context("MCAP channel sequence exhausted")?;
        self.writer.write_to_known_channel(&header, data)?;
        Ok(())
    }

    fn frame(&mut self, frame: &SimulationFrame) -> Result<()> {
        for entity in &frame.entities {
            if self.descriptions.insert(entity.id) {
                let group_index = usize::try_from(entity.sub_swarm_id)
                    .context("entity group index is negative")?;
                let group = self
                    .groups
                    .get(group_index)
                    .context("frame entity has no scenario group")?;
                let description = EntityDescription {
                    id: entity.id,
                    team_id: entity.team_id,
                    group_index: u32::try_from(group_index)?,
                    group_label: group.0.clone(),
                    visual_model: group.1.clone(),
                };
                self.message("/entities", frame.time_s, &description.encode_to_vec())?;
            }
        }
        let frame = Frame {
            time_s: frame.time_s,
            entities: frame
                .entities
                .iter()
                .map(|entity| EntityState {
                    id: entity.id,
                    truth: Some(State::from(&entity.truth)),
                    active: entity.active,
                })
                .collect(),
        };
        self.message("/frames", frame.time_s, &frame.encode_to_vec())
    }

    fn events(&mut self, events: &[SimulationEvent]) -> Result<()> {
        for event in events {
            let value = Event {
                time_s: event.time_s,
                kind: event.kind.topic().into(),
                entity_ids: event.entity_ids.clone(),
            };
            self.message("/events", event.time_s, &value.encode_to_vec())?;
        }
        Ok(())
    }

    fn new_events(&mut self, events: &[SimulationEvent]) -> Result<()> {
        self.events(&events[self.events_written..])?;
        self.events_written = events.len();
        Ok(())
    }

    pub fn step_frame(&mut self, simulation: &Simulation, frame: &SimulationFrame) -> Result<()> {
        self.new_events(simulation.events())?;
        self.frame(frame)
    }

    fn summary(&mut self, time_s: f64, csv: String) -> Result<()> {
        self.message("/summary", time_s, &Summary { csv }.encode_to_vec())
    }

    /// Optional observations use MCAP's native JSON message encoding.
    fn trace(&mut self, kind: &str, time_s: f64, value: &serde_json::Value) -> Result<()> {
        let topic = match kind {
            "belief" => "/beliefs",
            "reading" => "/readings",
            "delivery" | "scheduled_delivery" => "/deliveries",
            "output" => "/outputs",
            other => bail!("unknown trace kind {other}"),
        };
        self.message(topic, time_s, &serde_json::to_vec(value)?)
    }

    pub fn finish_run(
        mut self,
        simulation: &Simulation,
        error: Option<&anyhow::Error>,
    ) -> Result<()> {
        self.new_events(simulation.events())?;
        self.summary(simulation.time_s(), simulation.summary_csv())?;
        let mut metadata = BTreeMap::from([("steps".into(), simulation.step_count().to_string())]);
        if let Some(reason) = simulation.termination() {
            metadata.insert("termination".into(), format!("{reason:?}"));
        }
        if let Some(error) = error {
            metadata.insert("error".into(), format!("{error:#}"));
        }
        self.writer.write_metadata(&Metadata {
            name: "ripple.result".into(),
            metadata,
        })?;
        self.writer.finish()?;
        Ok(())
    }
}

impl TraceSink for RunWriter {
    fn record(&mut self, kind: &str, time_s: f64, value: serde_json::Value) -> Result<()> {
        self.trace(kind, time_s, &value)
    }
}

impl From<Vec3> for Vector3 {
    fn from(value: Vec3) -> Self {
        Self {
            x: value.x,
            y: value.y,
            z: value.z,
        }
    }
}

impl From<&KinematicState> for State {
    fn from(value: &KinematicState) -> Self {
        let q = value.orientation_world_from_body;
        Self {
            position_world_m: Some(value.position_world_m.into()),
            orientation_world_from_body: Some(Quaternion {
                w: q.w,
                x: q.x,
                y: q.y,
                z: q.z,
            }),
            velocity_world_mps: Some(value.velocity_world_mps.into()),
            angular_velocity_world_radps: Some(value.angular_velocity_world_radps.into()),
        }
    }
}

fn vector(value: Option<Vector3>, field: &str) -> Result<Vec3> {
    let value = value.with_context(|| format!("missing {field}"))?;
    let result = Vec3::new(value.x, value.y, value.z);
    ensure!(
        result.iter().all(|part| part.is_finite()),
        "nonfinite {field}"
    );
    Ok(result)
}

fn state(value: Option<State>) -> Result<KinematicState> {
    let value = value.context("missing entity truth")?;
    let q = value
        .orientation_world_from_body
        .context("missing orientation")?;
    let norm = q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z;
    ensure!(
        norm.is_finite() && (norm - 1.0).abs() < 1e-8,
        "orientation is not a unit quaternion"
    );
    Ok(KinematicState {
        position_world_m: vector(value.position_world_m, "position")?,
        orientation_world_from_body: MathQuaternion {
            w: q.w,
            x: q.x,
            y: q.y,
            z: q.z,
        },
        velocity_world_mps: vector(value.velocity_world_mps, "velocity")?,
        angular_velocity_world_radps: vector(
            value.angular_velocity_world_radps,
            "angular velocity",
        )?,
    })
}

/// Decode frames for Rerun replay. Other consumers use the reference reader,
/// which also exposes events, metrics, metadata, and trace channels.
pub fn read_frames(path: &Path) -> Result<Vec<SimulationFrame>> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let scenario = scenario_from_bytes(&bytes)?;
    frames_from_bytes(&bytes, &scenario)
}

/// Decode the MCAP frames and load the terrain files named by its scenario.
pub fn read_replay(path: &Path) -> Result<(Vec<SimulationFrame>, Option<Terrain>)> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let metadata = metadata_from_bytes(&bytes, "ripple.run")?;
    let scenario = scenario_from_metadata(&metadata)?;
    let frames = frames_from_bytes(&bytes, &scenario)?;
    let terrain = scenario["terrain"]
        .as_str()
        .map(|name| {
            let header = Path::new(name);
            let recorded = metadata
                .get("terrain_sha256")
                .context("terrain run has no terrain fingerprint")?;
            let actual = terrain_fingerprint(header)?;
            ensure!(
                &actual == recorded,
                "terrain files changed since this run: {}",
                header.display()
            );
            Terrain::load(header)
        })
        .transpose()?;
    Ok((frames, terrain))
}

fn frames_from_bytes(bytes: &[u8], scenario: &serde_json::Value) -> Result<Vec<SimulationFrame>> {
    let groups = scenario["entities"]
        .as_array()
        .context("recorded scenario has no entity groups")?;
    let mut descriptions = BTreeMap::new();
    let mut frames = Vec::new();
    for message in MessageStream::new(bytes)? {
        let message = message?;
        match message.channel.topic.as_str() {
            "/entities" => {
                let description = EntityDescription::decode(message.data.as_ref())?;
                ensure!(
                    descriptions.insert(description.id, description).is_none(),
                    "duplicate entity description"
                );
            }
            "/frames" => {
                let frame = Frame::decode(message.data.as_ref())?;
                ensure!(frame.time_s.is_finite(), "nonfinite frame time");
                let mut entities = Vec::with_capacity(frame.entities.len());
                let mut ids = BTreeSet::new();
                for entity in frame.entities {
                    ensure!(ids.insert(entity.id), "duplicate entity ID {}", entity.id);
                    let description: &EntityDescription = descriptions
                        .get(&entity.id)
                        .context("frame entity has no description")?;
                    let group_index = usize::try_from(description.group_index)?;
                    let group = groups
                        .get(group_index)
                        .context("entity description has no scenario group")?;
                    ensure!(
                        group["label"].as_str() == Some(description.group_label.as_str()),
                        "entity group label differs from scenario"
                    );
                    entities.push(EntitySnapshot {
                        id: entity.id,
                        sub_swarm_id: i32::try_from(group_index)?,
                        team_id: description.team_id,
                        truth: state(entity.truth)?,
                        active: entity.active,
                    });
                }
                frames.push(SimulationFrame {
                    time_s: frame.time_s,
                    entities,
                });
            }
            _ => {}
        }
    }
    Ok(frames)
}

pub fn read_scenario(path: &Path) -> Result<serde_json::Value> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    scenario_from_bytes(&bytes)
}

fn scenario_from_bytes(bytes: &[u8]) -> Result<serde_json::Value> {
    let metadata = metadata_from_bytes(bytes, "ripple.run")?;
    scenario_from_metadata(&metadata)
}

fn scenario_from_metadata(metadata: &BTreeMap<String, String>) -> Result<serde_json::Value> {
    let scenario = metadata.get("scenario").context("record has no scenario")?;
    serde_json::from_str(scenario).context("invalid recorded scenario")
}

pub fn read_result(path: &Path) -> Result<BTreeMap<String, String>> {
    read_metadata(path, "ripple.result")
}

fn read_metadata(path: &Path, name: &str) -> Result<BTreeMap<String, String>> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    metadata_from_bytes(&bytes, name)
}

fn metadata_from_bytes(bytes: &[u8], name: &str) -> Result<BTreeMap<String, String>> {
    for record in ChunkFlattener::new(bytes)? {
        if let Record::Metadata(metadata) = record?
            && metadata.name == name
        {
            return Ok(metadata.metadata);
        }
    }
    bail!("record has no {name} metadata")
}
