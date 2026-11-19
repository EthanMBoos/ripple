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
    drawing::PluginDrawings,
    entity::EntitySnapshot,
    math::{GaussianState, KinematicState, Quaternion as MathQuaternion, StateCovariance, Vec3},
    simulation::{Event as SimulationEvent, Simulation, SimulationFrame, TraceSink},
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
struct MetricValue {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(double, tag = "2")]
    value: f64,
}

#[derive(Clone, PartialEq, Message)]
struct TeamMetrics {
    #[prost(int32, tag = "1")]
    team_id: i32,
    #[prost(double, tag = "2")]
    score: f64,
    #[prost(message, repeated, tag = "3")]
    values: Vec<MetricValue>,
}

#[derive(Clone, PartialEq, Message)]
struct MetricReport {
    #[prost(string, tag = "1")]
    plugin: String,
    #[prost(string, repeated, tag = "2")]
    columns: Vec<String>,
    #[prost(message, repeated, tag = "3")]
    teams: Vec<TeamMetrics>,
}

#[derive(Clone, PartialEq, Message)]
struct Summary {
    #[prost(message, repeated, tag = "1")]
    metrics: Vec<MetricReport>,
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
    /// The drawings last written, so a frame's are written only on change.
    drawings_written: Vec<PluginDrawings>,
}

/// One `/drawings` message: every plugin's drawings from `time_s` until the
/// next message. JSON, like the trace channels, so a reader needs no schema.
#[derive(serde::Serialize, serde::Deserialize)]
struct DrawingsRecord {
    time_s: f64,
    drawings: Vec<PluginDrawings>,
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
        // DEVNOTE: MCAP is the run record because its bytes are deterministic
        // (two identical runs exported to RRD differed in size), which lets
        // the determinism check compare complete files. zstd chunks took a
        // traced 4,000-step multirotor run from 35.9 MB to 4.2 MB
        // (2026-10-15) with the bytes still identical run to run.
        let mut writer = WriteOptions::new()
            .compression(Some(Compression::Zstd))
            // mcap's default starts a zstd thread pool for every chunk. A
            // writer thread of its own, fed over a bounded channel, took a
            // 64-hub hour from 17.2 s to 14.4 s with identical bytes; not
            // done, to keep the recorder a plain writer.
            .compression_threads(1)
            .profile("ripple")
            // mcap 0.25 repeats schemas and channels from hash maps in the
            // summary, which made identical runs differ there.
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
                .groups()
                .map(|group| (group.label.clone(), group.visual_model.clone()))
                .collect(),
            events_written: 0,
            channels,
            sequences: BTreeMap::new(),
            drawings_written: Vec::new(),
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
                let group_index = usize::try_from(entity.group_index)
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

    /// The frame's drawings, when they differ from the last written. The
    /// channel is added at the first message, so a run in which nothing
    /// draws has no `/drawings` channel.
    fn drawings(&mut self, frame: &SimulationFrame) -> Result<()> {
        if frame.drawings == self.drawings_written {
            return Ok(());
        }
        if !self.channels.contains_key("/drawings") {
            let channel = self
                .writer
                .add_channel(0, "/drawings", "json", &BTreeMap::new())?;
            self.channels.insert("/drawings", channel);
        }
        let record = DrawingsRecord {
            time_s: frame.time_s,
            drawings: frame.drawings.clone(),
        };
        self.message("/drawings", frame.time_s, &serde_json::to_vec(&record)?)?;
        self.drawings_written = record.drawings;
        Ok(())
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
        self.frame(frame)?;
        self.drawings(frame)
    }

    /// Each metric plugin's own score and columns; a column a team has no
    /// value for is left out, not written as zero.
    fn summary(&mut self, simulation: &Simulation) -> Result<()> {
        let metrics = simulation
            .metric_reports()
            .into_iter()
            .map(|(plugin, report)| MetricReport {
                teams: report
                    .teams
                    .iter()
                    .map(|(&team_id, team)| TeamMetrics {
                        team_id,
                        score: team.score,
                        values: report
                            .headers
                            .iter()
                            .filter_map(|name| {
                                let value = *team.values.get(name)?;
                                Some(MetricValue {
                                    name: name.clone(),
                                    value,
                                })
                            })
                            .collect(),
                    })
                    .collect(),
                plugin,
                columns: report.headers,
            })
            .collect();
        let summary = Summary { metrics };
        self.message("/summary", simulation.time_s(), &summary.encode_to_vec())
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
        self.summary(simulation)?;
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

/// What a replay draws: the frames, the terrain files named by the scenario,
/// and its Boundary regions, plus the beliefs when the run was traced.
pub struct Replay {
    /// Each frame carries the drawings in force at its time.
    pub frames: Vec<SimulationFrame>,
    pub terrain: Option<Terrain>,
    /// For each frame, every entity's belief at that frame's time, from the
    /// `/beliefs` channel; `None` when the run was not recorded with `--trace`.
    pub beliefs: Option<Vec<Vec<Belief>>>,
    /// Each group's label and visual model, entity groups then recipe
    /// groups, indexed by an entity's group index.
    pub groups: Vec<(String, String)>,
    /// For each frame, the positions the sensors measured at it, from the
    /// `/readings` channel; `None` when the run was not recorded with
    /// `--trace`.
    pub fixes: Option<Vec<Vec<Fix>>>,
}

/// One measured position from a sensor reading: whose sensor took it and
/// whose position it is (the sensor's own entity for an own-state fix,
/// which the trace records with a null entity id).
#[derive(Clone, Debug)]
pub struct Fix {
    pub entity_id: i32,
    pub measured_id: i32,
    pub position_world_m: Vec3,
}

impl Fix {
    /// The measured positions in one `reading` trace record.
    pub fn all_from_trace(record: &serde_json::Value) -> Result<Vec<Self>> {
        let entity_id = trace_id(&record["sensor"]["entity_id"], "sensor entity_id")?;
        record["states"]
            .as_array()
            .context("reading has no states")?
            .iter()
            .map(|measured| {
                Ok(Self {
                    entity_id,
                    measured_id: if measured["entity_id"].is_null() {
                        entity_id
                    } else {
                        trace_id(&measured["entity_id"], "measured entity_id")?
                    },
                    position_world_m: trace_numbers::<3>(
                        &measured["state"]["position_world_m"],
                        "position_world_m",
                    )?
                    .into(),
                })
            })
            .collect()
    }
}

/// One entity's belief as the `/beliefs` channel recorded it: its own state
/// and the tracks it kept, each a mean with its covariance.
#[derive(Clone, Debug)]
pub struct Belief {
    pub entity_id: i32,
    pub team_id: i32,
    pub state: GaussianState,
    pub tracks: Vec<TrackBelief>,
}

/// One recorded track: the tracker's id, who it is if known, and its state.
#[derive(Clone, Debug)]
pub struct TrackBelief {
    pub track_id: u32,
    pub entity_id: Option<i32>,
    pub team_id: Option<i32>,
    pub state: GaussianState,
}

/// A trace number: JSON has no NaN or infinity, so the trace writes them as
/// strings (`simulation/trace.rs`).
fn trace_number(value: &serde_json::Value) -> Result<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64().context("trace number"),
        serde_json::Value::String(text) => match text.as_str() {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            other => bail!("trace value {other:?} is not a number"),
        },
        other => bail!("trace value {other} is not a number"),
    }
}

fn trace_numbers<const N: usize>(value: &serde_json::Value, name: &str) -> Result<[f64; N]> {
    let items = value
        .as_array()
        .with_context(|| format!("trace {name} is not a list"))?;
    ensure!(
        items.len() == N,
        "trace {name} has {} numbers, not {N}",
        items.len()
    );
    let mut numbers = [0.0; N];
    for (number, item) in numbers.iter_mut().zip(items) {
        *number = trace_number(item)?;
    }
    Ok(numbers)
}

/// A recorded mean and covariance, the inverse of the trace's `state_record`.
fn trace_state(value: &serde_json::Value) -> Result<GaussianState> {
    let [x, y, z] = trace_numbers(&value["position_world_m"], "position")?;
    let [vx, vy, vz] = trace_numbers(&value["velocity_world_mps"], "velocity")?;
    let [w, qx, qy, qz] = trace_numbers(&value["orientation_world_from_body"], "orientation")?;
    let [wx, wy, wz] = trace_numbers(&value["angular_velocity_world_radps"], "angular velocity")?;
    let covariance: [f64; 144] = trace_numbers(&value["covariance"], "covariance")?;
    Ok(GaussianState {
        mean: KinematicState {
            position_world_m: Vec3::new(x, y, z),
            orientation_world_from_body: MathQuaternion {
                w,
                x: qx,
                y: qy,
                z: qz,
            },
            velocity_world_mps: Vec3::new(vx, vy, vz),
            angular_velocity_world_radps: Vec3::new(wx, wy, wz),
        },
        covariance: StateCovariance(nalgebra::SMatrix::from_row_slice(&covariance)),
    })
}

impl Belief {
    /// One `belief` trace record, as the trace writes it: the live viewer
    /// reads it from the sink and replay from the `/beliefs` channel, so both
    /// draw the same thing.
    pub fn from_trace(record: &serde_json::Value) -> Result<Self> {
        trace_belief(record)
    }
}

fn trace_belief(record: &serde_json::Value) -> Result<Belief> {
    let id = |value: &serde_json::Value, name: &str| -> Result<Option<i32>> {
        if value.is_null() {
            return Ok(None);
        }
        let id = value
            .as_i64()
            .with_context(|| format!("belief {name} is not an integer"))?;
        Ok(Some(i32::try_from(id)?))
    };
    let tracks = record["tracks"]
        .as_array()
        .context("belief record has no tracks")?
        .iter()
        .map(|track| {
            Ok(TrackBelief {
                track_id: u32::try_from(
                    track["track_id"]
                        .as_u64()
                        .context("track has no track_id")?,
                )?,
                entity_id: id(&track["entity_id"], "track entity_id")?,
                team_id: id(&track["team_id"], "track team_id")?,
                state: trace_state(&track["state"])?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Belief {
        entity_id: id(&record["entity_id"], "entity_id")?.context("belief has no entity_id")?,
        team_id: id(&record["team_id"], "team_id")?.context("belief has no team_id")?,
        state: trace_state(&record["state"])?,
        tracks,
    })
}

/// The beliefs at each frame, by the frames' and belief records' shared
/// log time; `None` for a run without a `/beliefs` channel.
fn beliefs_from_bytes(bytes: &[u8]) -> Result<Option<Vec<Vec<Belief>>>> {
    let mut frame_times = Vec::new();
    let mut beliefs: BTreeMap<u64, Vec<Belief>> = BTreeMap::new();
    let mut traced = false;
    for message in MessageStream::new(bytes)? {
        let message = message?;
        match message.channel.topic.as_str() {
            "/frames" => frame_times.push(message.log_time),
            "/beliefs" => {
                traced = true;
                beliefs
                    .entry(message.log_time)
                    .or_default()
                    .push(trace_belief(&serde_json::from_slice(
                        message.data.as_ref(),
                    )?)?);
            }
            _ => {}
        }
    }
    if !traced {
        return Ok(None);
    }
    Ok(Some(
        frame_times
            .iter()
            .map(|time| beliefs.get(time).cloned().unwrap_or_default())
            .collect(),
    ))
}

/// An entity id in a trace record.
fn trace_id(value: &serde_json::Value, name: &str) -> Result<i32> {
    let id = value
        .as_i64()
        .with_context(|| format!("reading {name} is not an integer"))?;
    Ok(i32::try_from(id)?)
}

/// The measured positions at each frame, by the frames' and reading
/// records' shared log time; `None` for a run without a `/readings` channel.
fn fixes_from_bytes(bytes: &[u8]) -> Result<Option<Vec<Vec<Fix>>>> {
    let mut frame_times = Vec::new();
    let mut fixes: BTreeMap<u64, Vec<Fix>> = BTreeMap::new();
    let mut traced = false;
    for message in MessageStream::new(bytes)? {
        let message = message?;
        match message.channel.topic.as_str() {
            "/frames" => frame_times.push(message.log_time),
            "/readings" => {
                traced = true;
                fixes
                    .entry(message.log_time)
                    .or_default()
                    .extend(Fix::all_from_trace(&serde_json::from_slice(
                        message.data.as_ref(),
                    )?)?);
            }
            _ => {}
        }
    }
    if !traced {
        return Ok(None);
    }
    Ok(Some(
        frame_times
            .iter()
            .map(|time| fixes.get(time).cloned().unwrap_or_default())
            .collect(),
    ))
}

/// Decode the MCAP frames and drawings and load the terrain files named by
/// its scenario.
pub fn read_replay(path: &Path) -> Result<Replay> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let metadata = metadata_from_bytes(&bytes, "ripple.run")?;
    let scenario = scenario_from_metadata(&metadata)?;
    let frames = frames_from_bytes(&bytes, &scenario)?;
    let beliefs = beliefs_from_bytes(&bytes)?;
    let fixes = fixes_from_bytes(&bytes)?;
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
    Ok(Replay {
        frames,
        terrain,
        beliefs,
        groups: groups_from_scenario(&scenario)?,
        fixes,
    })
}

/// The recorded scenario's groups: its entity groups, then the recipe
/// groups plugins spawn from, as the record numbers them.
fn groups_from_scenario(scenario: &serde_json::Value) -> Result<Vec<(String, String)>> {
    let mut groups = Vec::new();
    for key in ["entities", "recipes"] {
        for group in scenario[key].as_array().into_iter().flatten() {
            let text = |field: &str| -> Result<String> {
                group[field]
                    .as_str()
                    .map(str::to_owned)
                    .with_context(|| format!("scenario group has no {field}"))
            };
            groups.push((text("label")?, text("visual_model")?));
        }
    }
    Ok(groups)
}

fn frames_from_bytes(bytes: &[u8], scenario: &serde_json::Value) -> Result<Vec<SimulationFrame>> {
    let groups = scenario["entities"]
        .as_array()
        .context("recorded scenario has no entity groups")?;
    let mut descriptions = BTreeMap::new();
    let mut frames = Vec::new();
    // A drawings message holds from its time until the next one; frames at
    // or after it carry its drawings.
    let mut drawings: Vec<DrawingsRecord> = Vec::new();
    for message in MessageStream::new(bytes)? {
        let message = message?;
        match message.channel.topic.as_str() {
            "/drawings" => {
                let record: DrawingsRecord = serde_json::from_slice(message.data.as_ref())?;
                ensure!(record.time_s.is_finite(), "nonfinite drawings time");
                drawings.push(record);
            }
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
                        group_index: i32::try_from(group_index)?,
                        team_id: description.team_id,
                        truth: state(entity.truth)?,
                        active: entity.active,
                    });
                }
                frames.push(SimulationFrame {
                    time_s: frame.time_s,
                    entities,
                    drawings: Vec::new(),
                });
            }
            _ => {}
        }
    }
    let mut in_force = drawings.iter().peekable();
    let mut current: &[PluginDrawings] = &[];
    for frame in &mut frames {
        while let Some(record) = in_force.next_if(|record| record.time_s <= frame.time_s) {
            current = &record.drawings;
        }
        frame.drawings = current.to_vec();
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
