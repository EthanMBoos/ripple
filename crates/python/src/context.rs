//! What a Python plugin's `step(ctx)` receives, and how its answers come back.
//!
//! `ctx` is plain data, floats and tuples, no numpy, built fresh for every
//! call: the same attributes a Rust plugin's context has, named for the slot
//! (`bridge.rs` lists them). A plugin answers with plain data too: a dict of
//! port values, a state as a dict of the fields to change, a contact list.
use pyo3::exceptions::{PyAttributeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};
use ripple_core::plugin::{
    Contact as KnownContact, ContactList, MessageEndpoint, PluginRandom, StepTime,
};
use ripple_core::{EntitySnapshot, KinematicState, Quaternion, Vec3};
use std::cmp::Ordering;

type Vector = (f64, f64, f64);

fn vector(v: Vec3) -> Vector {
    (v.x, v.y, v.z)
}
fn vec3((x, y, z): Vector) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A kinematic state: positions in meters and velocities in meters per
/// second, both in the world frame; `heading` is the yaw of `orientation` in
/// radians. Own-state belief, a sensor's truth, or a reading.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct State {
    position: Vector,
    velocity: Vector,
    heading: f64,
    /// `(w, x, y, z)`, world from body.
    orientation: (f64, f64, f64, f64),
    angular_velocity: Vector,
}
impl State {
    pub fn new(state: &KinematicState) -> Self {
        let q = state.orientation_world_from_body;
        Self {
            position: vector(state.position_world_m),
            velocity: vector(state.velocity_world_mps),
            heading: q.yaw_world_from_body_rad(),
            orientation: (q.w, q.x, q.y, q.z),
            angular_velocity: vector(state.angular_velocity_world_radps),
        }
    }
    fn kinematic(&self) -> KinematicState {
        let (w, x, y, z) = self.orientation;
        KinematicState {
            position_world_m: vec3(self.position),
            velocity_world_mps: vec3(self.velocity),
            orientation_world_from_body: Quaternion { w, x, y, z },
            angular_velocity_world_radps: vec3(self.angular_velocity),
        }
    }
}

/// A kinematic state from a plugin's answer: a `State`, or a dict naming the
/// fields to change (`position`, `velocity`, `orientation`,
/// `angular_velocity`) on top of `base`.
pub fn kinematic_state(
    base: &KinematicState,
    value: &Bound<'_, PyAny>,
) -> PyResult<KinematicState> {
    if let Ok(state) = value.cast::<State>() {
        return Ok(state.get().kinematic());
    }
    let fields = value.cast::<PyDict>().map_err(|_| {
        PyTypeError::new_err("a state is a ripple.State or a dict of the fields to change")
    })?;
    let mut state = base.clone();
    for (key, item) in fields.iter() {
        let key: String = key.extract()?;
        match key.as_str() {
            "position" => state.position_world_m = vec3(item.extract()?),
            "velocity" => state.velocity_world_mps = vec3(item.extract()?),
            "angular_velocity" => state.angular_velocity_world_radps = vec3(item.extract()?),
            "orientation" => {
                let (w, x, y, z) = item.extract()?;
                state.orientation_world_from_body = Quaternion { w, x, y, z };
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown state field '{other}' (position, velocity, orientation, angular_velocity)"
                )));
            }
        }
    }
    Ok(state)
}

/// One contact: a tracker's estimate, or a sensor's view of another vehicle's
/// truth. The same fields as `State`, plus who it is and when it was sampled.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct Contact {
    id: i32,
    team_id: i32,
    group_index: i32,
    position: Vector,
    velocity: Vector,
    heading: f64,
    orientation: (f64, f64, f64, f64),
    angular_velocity: Vector,
    sampled_at_s: f64,
}
impl Contact {
    fn new(
        id: i32,
        team_id: i32,
        group_index: i32,
        state: &KinematicState,
        sampled_at_s: f64,
    ) -> Self {
        let state = State::new(state);
        Self {
            id,
            team_id,
            group_index,
            position: state.position,
            velocity: state.velocity,
            heading: state.heading,
            orientation: state.orientation,
            angular_velocity: state.angular_velocity,
            sampled_at_s,
        }
    }
    pub fn known(contact: &KnownContact) -> Self {
        Self::new(
            contact.id,
            contact.team_id,
            contact.group_index,
            &contact.state,
            contact.sampled_at_s,
        )
    }
    pub fn truth(snapshot: &EntitySnapshot, sampled_at_s: f64) -> Self {
        Self::new(
            snapshot.id,
            snapshot.team_id,
            snapshot.group_index,
            &snapshot.truth,
            sampled_at_s,
        )
    }
    fn known_contact(&self) -> KnownContact {
        let (w, x, y, z) = self.orientation;
        KnownContact {
            id: self.id,
            team_id: self.team_id,
            group_index: self.group_index,
            state: KinematicState {
                position_world_m: vec3(self.position),
                velocity_world_mps: vec3(self.velocity),
                orientation_world_from_body: Quaternion { w, x, y, z },
                angular_velocity_world_radps: vec3(self.angular_velocity),
            },
            sampled_at_s: self.sampled_at_s,
        }
    }
}

/// A contact list from a plugin's answer: a list of `Contact`s, or of dicts
/// with `id` and `team_id` and any of a state's fields, `group_index`, and
/// `sampled_at_s` (default `sampled_at_s`).
pub fn contact_list(value: &Bound<'_, PyAny>, sampled_at_s: f64) -> PyResult<ContactList> {
    let items = value
        .cast::<PyList>()
        .map_err(|_| PyTypeError::new_err("contacts are a list of ripple.Contact or dicts"))?;
    let mut contacts = Vec::with_capacity(items.len());
    for item in items.iter() {
        if let Ok(contact) = item.cast::<Contact>() {
            contacts.push(contact.get().known_contact());
            continue;
        }
        let fields = item.cast::<PyDict>().map_err(|_| {
            PyTypeError::new_err("a contact is a ripple.Contact or a dict with id and team_id")
        })?;
        let field = |name: &str| fields.get_item(name);
        let id: i32 = field("id")?
            .ok_or_else(|| PyValueError::new_err("a contact dict needs an `id`"))?
            .extract()?;
        let team_id: i32 = field("team_id")?
            .ok_or_else(|| PyValueError::new_err("a contact dict needs a `team_id`"))?
            .extract()?;
        let group_index: i32 = match field("group_index")? {
            Some(value) => value.extract()?,
            None => 0,
        };
        let sampled_at_s: f64 = match field("sampled_at_s")? {
            Some(value) => value.extract()?,
            None => sampled_at_s,
        };
        let state_fields = PyDict::new(item.py());
        for (key, value) in fields.iter() {
            let key: String = key.extract()?;
            if !matches!(
                key.as_str(),
                "id" | "team_id" | "group_index" | "sampled_at_s"
            ) {
                state_fields.set_item(key, value)?;
            }
        }
        let state = kinematic_state(&KinematicState::default(), state_fields.as_any())?;
        contacts.push(KnownContact {
            id,
            team_id,
            group_index,
            state,
            sampled_at_s,
        });
    }
    Ok(ContactList::from(contacts))
}

#[pyclass(frozen, get_all, module = "ripple")]
pub struct Time {
    time_s: f64,
    dt_s: f64,
    tick: i64,
}
impl From<StepTime> for Time {
    fn from(time: StepTime) -> Self {
        Self {
            time_s: time.time_s,
            dt_s: time.dt_s,
            tick: time.tick,
        }
    }
}

#[pyclass(frozen, get_all, module = "ripple")]
pub struct Entity {
    pub id: i32,
    pub team_id: i32,
}

/// One input's latest reading, for an estimator: `value` is a `State`, a list
/// of `Contact`, or whatever a Python sensor published; `fresh` says it
/// arrived since the last estimation step.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct Sample {
    pub value: Py<PyAny>,
    pub sampled_at_s: f64,
    pub fresh: bool,
}

/// What the motion model read on one substep of the tick just finished, for
/// a localization: `values` by port name, and the substep's `time`.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct Command {
    pub time: Py<Time>,
    pub values: Py<PyDict>,
}

/// The mission's terrain, for a motion model: heights in meters at an east,
/// north position in the world frame.
#[pyclass(frozen, module = "ripple")]
pub struct Terrain {
    pub terrain: ripple_core::Terrain,
}
#[pymethods]
impl Terrain {
    /// The ground height, or `None` outside the terrain or where it has no sample.
    fn height_at(&self, east_m: f64, north_m: f64) -> Option<f64> {
        self.terrain.height_at(east_m, north_m)
    }
    /// The ground height, with the terrain's fallback where it has no sample.
    fn ground_height(&self, east_m: f64, north_m: f64) -> f64 {
        self.terrain.ground_height(east_m, north_m)
    }
    fn ground_normal(&self, east_m: f64, north_m: f64) -> Vector {
        vector(self.terrain.ground_normal(east_m, north_m))
    }
}

/// One entity as an interaction sees it: who it is, its true state, and its
/// health. `index` is its place in `ctx.entities`, which `ctx.nearby` uses.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct WorldEntity {
    index: usize,
    id: i32,
    team_id: i32,
    group_index: i32,
    truth: Py<State>,
    health: i32,
    active: bool,
}
impl WorldEntity {
    pub fn new(py: Python<'_>, index: usize, entity: &ripple_core::Entity) -> PyResult<Self> {
        Ok(Self {
            index,
            id: entity.id(),
            team_id: entity.team_id(),
            group_index: entity.group_index(),
            truth: Py::new(py, State::new(entity.truth()))?,
            health: entity.health(),
            active: entity.is_active(),
        })
    }
}

/// Which entities are near which, by their index in `ctx.entities`, at the
/// positions they had when the interaction's step began. The same answers,
/// in the same order, as the engine's spatial index gives a Rust interaction.
#[pyclass(frozen, module = "ripple")]
pub struct Nearby {
    positions_world_m: Vec<Vec3>,
}
impl Nearby {
    pub fn new(entities: &[ripple_core::Entity]) -> Self {
        Self {
            positions_world_m: entities
                .iter()
                .map(|entity| entity.truth().position_world_m)
                .collect(),
        }
    }
    fn is_within(&self, first: usize, second: usize, range_m: f64) -> bool {
        is_within(
            self.positions_world_m[first],
            self.positions_world_m[second],
            range_m,
        )
    }
}
/// The engine's test, including its answer for a NaN distance (within).
fn is_within(position_world_m: Vec3, other_world_m: Vec3, range_m: f64) -> bool {
    let distance_m = (position_world_m - other_world_m).norm();
    !matches!(
        distance_m.partial_cmp(&range_m),
        Some(Ordering::Equal | Ordering::Greater)
    )
}
#[pymethods]
impl Nearby {
    /// Every pair `(first, second)` with `first < second` closer than
    /// `range_m`, by `first` then `second`.
    fn pairs_within(&self, range_m: f64) -> Vec<(usize, usize)> {
        let count = self.positions_world_m.len();
        let mut pairs = Vec::new();
        for first in 0..count {
            for second in first + 1..count {
                if self.is_within(first, second, range_m) {
                    pairs.push((first, second));
                }
            }
        }
        pairs
    }
    /// The indices of the entities within `range_m` of a position, ascending.
    fn within(&self, position_world_m: Vector, range_m: f64) -> Vec<usize> {
        let position_world_m = vec3(position_world_m);
        (0..self.positions_world_m.len())
            .filter(|&index| is_within(position_world_m, self.positions_world_m[index], range_m))
            .collect()
    }
}

/// A message's sender or receiver: the plugin, and its entity if it has one.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct Endpoint {
    entity_id: Option<i32>,
    plugin: String,
}
impl Endpoint {
    pub fn new(endpoint: &MessageEndpoint) -> Self {
        Self {
            entity_id: endpoint.entity_id,
            plugin: endpoint.plugin.clone(),
        }
    }
}

/// One message on its way from a publisher to one subscriber, for a network
/// to deliver or drop. `sequence` numbers the sender's messages on this topic
/// this tick; with the two endpoints and the topic it names the message, as a
/// random key. `random` is the network's numbers at this tick.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct Link {
    pub time: Py<Time>,
    pub sender: Py<Endpoint>,
    pub receiver: Py<Endpoint>,
    pub topic: String,
    pub sequence: u64,
    pub random: Py<Random>,
    /// Every entity's true state, for a network that depends on positions.
    pub contacts_truth: Py<PyList>,
}

/// An engine event a metrics plugin received: when it happened, which kind
/// (its topic name), the entities involved, and when it was delivered.
#[pyclass(frozen, get_all, module = "ripple")]
pub struct Event {
    pub time_s: f64,
    pub kind: String,
    pub entity_ids: Vec<i32>,
    pub delivered_at_s: f64,
}

/// This plugin's random numbers at this tick, keyed like the Rust side: a key
/// is a string, an integer, or a tuple of them, and the same key gives the
/// same number within a tick. Never by draw order.
#[pyclass(frozen, module = "ripple")]
pub struct Random {
    pub random: PluginRandom,
}
#[pymethods]
impl Random {
    /// The uniform number for `key`, strictly inside (0, 1).
    fn uniform(&self, key: &Bound<'_, PyAny>) -> PyResult<f64> {
        Ok(self.random.uniform(&parts(key)?))
    }
    /// The normal number for `key`.
    fn normal(&self, key: &Bound<'_, PyAny>, mean: f64, stddev: f64) -> PyResult<f64> {
        self.random
            .normal(&parts(key)?, mean, stddev)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }
    /// An integer for `key`, to seed a library's own generator.
    fn seed(&self, key: &Bound<'_, PyAny>) -> PyResult<u64> {
        Ok(self.random.seed(&parts(key)?))
    }
}

fn parts(key: &Bound<'_, PyAny>) -> PyResult<Vec<u64>> {
    if let Ok(tuple) = key.cast::<PyTuple>() {
        return tuple.iter().map(|item| part(&item)).collect();
    }
    Ok(vec![part(key)?])
}
fn part(item: &Bound<'_, PyAny>) -> PyResult<u64> {
    if let Ok(text) = item.cast::<PyString>() {
        return Ok(PluginRandom::name(&text.to_cow()?));
    }
    if let Ok(integer) = item.extract::<i64>() {
        return Ok(integer as u64);
    }
    Err(PyTypeError::new_err(
        "a random key is a string, an integer, or a tuple of them",
    ))
}

/// Everything one call sees, as attributes. Which attributes exist depends on
/// the slot; asking for one the slot does not have is an `AttributeError`.
#[pyclass(frozen, module = "ripple")]
pub struct Context {
    attributes: Py<PyDict>,
}
impl Context {
    pub fn new(attributes: Bound<'_, PyDict>) -> Self {
        Self {
            attributes: attributes.unbind(),
        }
    }
    pub fn set<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        value: impl IntoPyObject<'py>,
    ) -> PyResult<()> {
        self.attributes.bind(py).set_item(name, value)
    }
}
#[pymethods]
impl Context {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.attributes
            .bind(py)
            .get_item(name)?
            .map(Bound::unbind)
            .ok_or_else(|| PyAttributeError::new_err(format!("ctx has no '{name}' in this slot")))
    }
    fn __dir__(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(self.attributes.bind(py).keys().unbind())
    }
}
