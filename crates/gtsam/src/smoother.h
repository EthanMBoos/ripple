// The GTSAM calls the GtsamSmoother plugin makes, as one class on plain
// numbers; ffi.rs declares the same functions for Rust. Kept this small so
// GTSAM's templates and Eigen types never cross the bridge.
#pragma once

#include <array>
#include <cstdint>
#include <memory>

#include <gtsam/nonlinear/ISAM2Params.h>
#include <gtsam/nonlinear/IncrementalFixedLagSmoother.h>
#include <gtsam/nonlinear/NonlinearFactorGraph.h>
#include <gtsam/nonlinear/Values.h>

namespace ripple_gtsam {

class Smoother {
 public:
  explicit Smoother(double lag_s);

  void add_initial(uint64_t key, double time_s, const std::array<double, 3>& guess);
  void add_prior(uint64_t key, const std::array<double, 3>& mean,
                 const std::array<double, 9>& covariance);
  void add_between(uint64_t from, uint64_t to, const std::array<double, 3>& delta,
                   const std::array<double, 9>& covariance);
  void update();
  void estimate(uint64_t key, std::array<double, 3>& position) const;
  void marginal(uint64_t key, std::array<double, 9>& covariance) const;

 private:
  gtsam::IncrementalFixedLagSmoother smoother_;
  // Added since the last update.
  gtsam::NonlinearFactorGraph new_factors_;
  gtsam::Values new_values_;
  gtsam::FixedLagSmoother::KeyTimestampMap new_timestamps_;
};

std::unique_ptr<Smoother> new_smoother(double lag_s);

}  // namespace ripple_gtsam
