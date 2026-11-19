# A GTSAM smoother (optional)

The optional `gtsam` crate puts [GTSAM](https://gtsam.org), the factor-graph
library, into a Ripple mission as a localization plugin. `GtsamSmoother` is a
fixed-lag smoother over position: every tick is a variable, the commanded
displacement ties it to the previous one, a position fix ties it to the world,
and GTSAM's iSAM2 solves the window incrementally, folding what leaves the
window into a prior. The newest variable's solution and marginal covariance
are the belief. It poses the same problem the starter's
[PositionKalman](../tutorial/noisy-localization.md) filter solves, and on
that problem the two agree to rounding, which its tests check.

GTSAM is a C++ library that you install once. The crate is outside the
default build and the workspace, so `cargo build --workspace` passes
without it; `ripple` takes it in with `--features gtsam`.

## Install GTSAM

Build GTSAM 4.3 into a prefix of its own. The flags turn off Boost, which
4.3 no longer needs, and threading (TBB), so a run's output does not depend
on how threads happened to sum, which Ripple's byte-identical runs require.
The library is static, so `ripple` carries GTSAM and needs no library path
at run time.

```sh
git clone --depth 1 --branch 4.3.0 https://github.com/borglab/gtsam.git
cd gtsam
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$HOME/.local/gtsam-4.3.0" \
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DBUILD_SHARED_LIBS=OFF \
  -DGTSAM_USE_BOOST_FEATURES=OFF -DGTSAM_ENABLE_BOOST_SERIALIZATION=OFF \
  -DGTSAM_WITH_TBB=OFF -DGTSAM_SUPPORT_NESTED_DISSECTION=OFF \
  -DGTSAM_BUILD_WITH_MARCH_NATIVE=OFF -DGTSAM_BUILD_UNSTABLE=OFF \
  -DGTSAM_BUILD_TESTS=OFF -DGTSAM_BUILD_EXAMPLES_ALWAYS=OFF \
  -DGTSAM_BUILD_TIMING_ALWAYS=OFF
cmake --build build --parallel 6
cmake --build build --target install
```

It needs CMake 3.16 or newer and a C++17 compiler; on a Mac, Xcode's. The
build takes about ten minutes on a laptop. GTSAM installs the Eigen it was
built with under the prefix, and the crate compiles against that copy.

## Build and run

Point `GTSAM_PREFIX` at the install and build `ripple` with the feature.
From the repository root:

```sh
export GTSAM_PREFIX="$HOME/.local/gtsam-4.3.0"
cargo build --release -p ripple --features gtsam
target/release/ripple run crates/gtsam/missions/position-smoother.yaml --headless
```

The mission drives three point robots around a 100 m square: one believing
the GTSAM smoother, one believing the Kalman filter, and one with perfect
localization. Both estimators assume the robot moves as commanded while its
motors fall 2 m/s short, so the beliefs drift between the fixes every 5 s and
snap back at each one. Open the run in the viewer to watch the two
estimators' covariance ellipsoids grow and collapse together.

The crate's tests run from its folder, since it is a workspace of its own:

```sh
cd crates/gtsam && cargo test
```

They compare the smoother with the filter tick by tick on exact and on noisy
fixes, check that a one-second lag marginalizes without changing the newest
estimate, and check the drift between fixes.

## In a mission

```yaml
localization:
  GtsamSmoother:
    speed_gain: 1.0          # the fraction of the commanded velocity it moves at
    process_noise_m2ps: 1.0  # position variance a tick's motion adds per second
    lag_s: 10                # ticks older than this leave the window
    min_fix_stddev_m: 0.001  # a fix's sigma on each axis is at least this
```

It reads one `fix` input, a `State` reading such as `NoisyState`, and uses
its position and position covariance. The newest estimate is the same for
any lag; a longer lag costs more per tick. GTSAM's Gaussian noise model
inverts a covariance, so an exact fix (zero covariance) is raised to
`min_fix_stddev_m` on each axis, and `process_noise_m2ps` must be positive.

## How the bridge is built

Three files carry GTSAM into Rust, all in `crates/gtsam/src`:

- `smoother.h` and `smoother.cc`: one C++ class, `Smoother`, holding GTSAM's
  `IncrementalFixedLagSmoother` and the factors and values added since its
  last update. Its functions take plain numbers: a position as three doubles,
  a 3x3 covariance as nine, row by row. GTSAM's templates and Eigen types stay
  on this side.
- `ffi.rs`: the [cxx](https://cxx.rs) bridge declaring the same functions for
  Rust. A GTSAM exception comes back as the `Err` of the call that raised it.
- `gtsam_smoother.rs`: the plugin, written like any other `Localization`.

`build.rs` compiles the shim against the headers under `GTSAM_PREFIX` and
links the static `libgtsam.a` into whatever uses the crate. There is no
official GTSAM binding for Rust; its generated bindings are for Python and
MATLAB, and anyone binding it from another language writes a shim like this
one, because template libraries defeat automatic binding generators.

iSAM2 normally stops back-substitution at variables whose change is under a
millimetre. The shim turns that off, so the window is solved exactly and the
newest estimate matches the Kalman filter to rounding; the window is small
enough that this costs nothing noticeable.

## What it does not do

It estimates position only, as the Kalman filter does. Attitude and velocity
would make the variable a GTSAM `Pose3` or `NavState` and the motion factor
an IMU preintegration or a between factor on poses; the shim's functions
would take six or nine numbers instead of three. Ripple's attitude
covariance is over a body-side small rotation, the convention of GTSAM's
`Rot3::retract`, so a pose marginal would drop into `StateCovariance` as it
is ([Uncertainty](../concepts/uncertainty.md)).
