//! The bridge to the C++ shim in smoother.cc: the few GTSAM calls the plugin
//! makes, on plain numbers. GTSAM's templates and Eigen types stay in C++;
//! positions cross as `[f64; 3]` and 3x3 covariances as row-major `[f64; 9]`.
//! A GTSAM exception comes back as the `Err` of the call that raised it.

#[cxx::bridge(namespace = "ripple_gtsam")]
pub(crate) mod bridge {
    unsafe extern "C++" {
        include!("gtsam/src/smoother.h");

        /// A GTSAM `IncrementalFixedLagSmoother` over `Point3` variables keyed
        /// by integers, with the factors and values added since its last update.
        type Smoother;

        /// A smoother that keeps the variables of the last `lag_s` seconds.
        fn new_smoother(lag_s: f64) -> UniquePtr<Smoother>;
        /// A new variable with its time and the guess the solver starts from.
        fn add_initial(
            self: Pin<&mut Smoother>,
            key: u64,
            time_s: f64,
            guess: &[f64; 3],
        ) -> Result<()>;
        /// A factor saying variable `key` is `mean`, with a Gaussian error of
        /// `covariance`: a position fix.
        fn add_prior(
            self: Pin<&mut Smoother>,
            key: u64,
            mean: &[f64; 3],
            covariance: &[f64; 9],
        ) -> Result<()>;
        /// A factor saying variable `to` is `from` moved by `delta`, with a
        /// Gaussian error of `covariance`: the commanded motion over a tick.
        fn add_between(
            self: Pin<&mut Smoother>,
            from: u64,
            to: u64,
            delta: &[f64; 3],
            covariance: &[f64; 9],
        ) -> Result<()>;
        /// Solves with everything added since the last update and drops the
        /// variables older than the lag, folding their information into a prior.
        fn update(self: Pin<&mut Smoother>) -> Result<()>;
        /// The solved position of variable `key`.
        fn estimate(&self, key: u64, position: &mut [f64; 3]) -> Result<()>;
        /// The marginal covariance of variable `key`, row-major.
        fn marginal(&self, key: u64, covariance: &mut [f64; 9]) -> Result<()>;
    }
}

// The engine runs plugins on one thread (simulation.rs says why) and only
// moves them between threads whole; the C++ object has no thread affinity.
unsafe impl Send for bridge::Smoother {}
