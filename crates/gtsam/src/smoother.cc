#include "gtsam/src/smoother.h"

#include <gtsam/geometry/Point3.h>
#include <gtsam/linear/NoiseModel.h>
#include <gtsam/nonlinear/PriorFactor.h>
#include <gtsam/slam/BetweenFactor.h>

namespace ripple_gtsam {

namespace {

gtsam::Point3 point(const std::array<double, 3>& xyz) {
  return gtsam::Point3(xyz[0], xyz[1], xyz[2]);
}

// A Gaussian noise model from a row-major 3x3 covariance. GTSAM inverts it,
// so the plugin never passes a singular one.
gtsam::SharedNoiseModel noise(const std::array<double, 9>& covariance) {
  gtsam::Matrix3 matrix;
  for (int row = 0; row < 3; ++row) {
    for (int column = 0; column < 3; ++column) {
      matrix(row, column) = covariance[3 * row + column];
    }
  }
  return gtsam::noiseModel::Gaussian::Covariance(matrix);
}

// iSAM2 normally stops back-substitution at variables whose change is under a
// millimetre (its wildfire threshold); the window here is small, so solve it
// exactly and the newest estimate matches a Kalman filter to rounding.
gtsam::ISAM2Params exact_solve() {
  gtsam::ISAM2Params params;
  params.optimizationParams = gtsam::ISAM2GaussNewtonParams(0.0);
  return params;
}

}  // namespace

Smoother::Smoother(double lag_s) : smoother_(lag_s, exact_solve()) {}

void Smoother::add_initial(uint64_t key, double time_s, const std::array<double, 3>& guess) {
  new_values_.insert(key, point(guess));
  new_timestamps_[key] = time_s;
}

void Smoother::add_prior(uint64_t key, const std::array<double, 3>& mean,
                         const std::array<double, 9>& covariance) {
  new_factors_.emplace_shared<gtsam::PriorFactor<gtsam::Point3>>(key, point(mean),
                                                                 noise(covariance));
}

void Smoother::add_between(uint64_t from, uint64_t to, const std::array<double, 3>& delta,
                           const std::array<double, 9>& covariance) {
  new_factors_.emplace_shared<gtsam::BetweenFactor<gtsam::Point3>>(from, to, point(delta),
                                                                   noise(covariance));
}

void Smoother::update() {
  smoother_.update(new_factors_, new_values_, new_timestamps_);
  new_factors_.resize(0);
  new_values_.clear();
  new_timestamps_.clear();
}

void Smoother::estimate(uint64_t key, std::array<double, 3>& position) const {
  const gtsam::Point3 solved = smoother_.calculateEstimate<gtsam::Point3>(key);
  position = {solved.x(), solved.y(), solved.z()};
}

void Smoother::marginal(uint64_t key, std::array<double, 9>& covariance) const {
  const gtsam::Matrix matrix = smoother_.marginalCovariance(key);
  for (int row = 0; row < 3; ++row) {
    for (int column = 0; column < 3; ++column) {
      covariance[3 * row + column] = matrix(row, column);
    }
  }
}

std::unique_ptr<Smoother> new_smoother(double lag_s) {
  return std::make_unique<Smoother>(lag_s);
}

}  // namespace ripple_gtsam
