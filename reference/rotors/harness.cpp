// Runs RotorS's own motor-plugin code on prescribed states, for
// reference/rotors_motor.py. Not part of Ripple.
//
// rotors_extract.inc holds, copied verbatim by extract.py from
// ethz-asl/rotors_simulator (Apache-2.0) at the pinned commit:
// FirstOrderFilter (rotors_gazebo_plugins/common.h) and
// GazeboMotorModel::NormalizeAngle and ::UpdateForcesAndMoments
// (src/gazebo_motor_model.cpp). This file supplies only what those need from
// Gazebo: links and a joint that record the forces and torques the plugin
// applies, with the kinematics (link poses and velocities) set from the input.
//
// Input (stdin), whitespace separated:
//   motor_constant moment_constant time_constant_up time_constant_down
//   rotor_drag_coefficient rolling_moment_coefficient slowdown
//   turning_direction(1 = CCW, -1 = CW) initial_speed
//   rotor offset x y z, rotor quaternion w x y z (body from rotor)
//   sample count, then per sample:
//     dt, command (rad/s), vehicle quaternion w x y z (world from body),
//     vehicle velocity (world, m/s), vehicle angular velocity (world, rad/s)
// Output (stdout), one line per sample, body frame:
//   speed_used force_x y z moment_about_center_of_mass_x y z speed_after
// where speed_used is the shaft speed the loads were computed with and
// speed_after the filter's output for the next step.

#include <cmath>
#include <cstdio>
#include <iostream>
#include <memory>
#include <vector>

#include <ignition/math/Pose3.hh>
#include <ignition/math/Quaternion.hh>
#include <ignition/math/Vector3.hh>

#define gzerr std::cerr

namespace gazebo {
namespace physics {

class Link;
typedef Link* LinkPtr;
typedef std::vector<LinkPtr> Link_V;

class Link {
 public:
  ignition::math::Pose3d pose;
  ignition::math::Vector3d velocity;
  ignition::math::Vector3d force;   // world frame, at the link origin
  ignition::math::Vector3d torque;  // world frame
  Link* parent = nullptr;

  ignition::math::Vector3d WorldLinearVel() const { return velocity; }
  ignition::math::Pose3d WorldCoGPose() const { return pose; }
  void AddRelativeForce(const ignition::math::Vector3d& f) { force += pose.Rot().RotateVector(f); }
  void AddForce(const ignition::math::Vector3d& f) { force += f; }
  void AddRelativeTorque(const ignition::math::Vector3d& t) { torque += pose.Rot().RotateVector(t); }
  void AddTorque(const ignition::math::Vector3d& t) { torque += t; }
  Link_V GetParentJointsLinks() { return Link_V{parent}; }
};

class Joint {
 public:
  double velocity = 0.0;
  ignition::math::Vector3d axis;
  double GetVelocity(unsigned int) const { return velocity; }
  ignition::math::Vector3d GlobalAxis(unsigned int) const { return axis; }
  void SetVelocity(unsigned int, double value) { velocity = value; }
  double Position(unsigned int) const { return 0.0; }
  void SetForce(unsigned int, double) {}
};
typedef Joint* JointPtr;

}  // namespace physics

namespace common {
struct PID {
  double Update(double, double) { return 0.0; }
};
}  // namespace common

#include "rotors_filter.inc"

namespace turning_direction {
const static int CCW = 1;
const static int CW = -1;
}  // namespace turning_direction

enum class MotorType { kVelocity, kPosition, kForce };

class GazeboMotorModel {
 public:
  void UpdateForcesAndMoments();
  double NormalizeAngle(double input);

  int motor_number_ = 0;
  int turning_direction_ = turning_direction::CW;
  MotorType motor_type_ = MotorType::kVelocity;
  double moment_constant_ = 0.0;
  double motor_constant_ = 0.0;
  double ref_motor_input_ = 0.0;
  double rolling_moment_coefficient_ = 0.0;
  double rotor_drag_coefficient_ = 0.0;
  double rotor_velocity_slowdown_sim_ = 10.0;
  double motor_rot_vel_ = 0.0;
  double sampling_time_ = 0.001;
  ignition::math::Vector3d wind_speed_W_ = ignition::math::Vector3d::Zero;
  common::PID pids_;
  physics::JointPtr joint_ = nullptr;
  physics::LinkPtr link_ = nullptr;
  std::unique_ptr<FirstOrderFilter<double>> rotor_velocity_filter_;
};

#include "rotors_methods.inc"

}  // namespace gazebo

int main() {
  using ignition::math::Pose3d;
  using ignition::math::Quaterniond;
  using ignition::math::Vector3d;

  gazebo::GazeboMotorModel model;
  double time_constant_up, time_constant_down, initial_speed;
  double ox, oy, oz, rw, rx, ry, rz;
  int samples;
  std::cin >> model.motor_constant_ >> model.moment_constant_ >> time_constant_up >> time_constant_down >>
      model.rotor_drag_coefficient_ >> model.rolling_moment_coefficient_ >> model.rotor_velocity_slowdown_sim_ >>
      model.turning_direction_ >> initial_speed >> ox >> oy >> oz >> rw >> rx >> ry >> rz >> samples;
  const Vector3d offset_body(ox, oy, oz);
  const Quaterniond body_from_rotor(rw, rx, ry, rz);

  gazebo::physics::Link vehicle, rotor;
  gazebo::physics::Joint joint;
  rotor.parent = &vehicle;
  model.link_ = &rotor;
  model.joint_ = &joint;
  // Load() starts the filter at the reference input; here, at the rotor's
  // first speed, with the joint already turning at it.
  model.ref_motor_input_ = initial_speed;
  model.rotor_velocity_filter_.reset(
      new gazebo::FirstOrderFilter<double>(time_constant_up, time_constant_down, initial_speed));
  joint.velocity = model.turning_direction_ * initial_speed / model.rotor_velocity_slowdown_sim_;

  for (int k = 0; k < samples; ++k) {
    double dt, command, qw, qx, qy, qz, vx, vy, vz, wx, wy, wz;
    std::cin >> dt >> command >> qw >> qx >> qy >> qz >> vx >> vy >> vz >> wx >> wy >> wz;
    const Quaterniond world_from_body(qw, qx, qy, qz);
    const Vector3d offset_world = world_from_body.RotateVector(offset_body);
    const Vector3d angular_velocity_world(wx, wy, wz);
    vehicle.pose = Pose3d(Vector3d(1.0, 2.0, 3.0), world_from_body);
    rotor.pose = Pose3d(vehicle.pose.Pos() + offset_world, world_from_body * body_from_rotor);
    vehicle.velocity = Vector3d(vx, vy, vz);
    rotor.velocity = vehicle.velocity + angular_velocity_world.Cross(offset_world);
    joint.axis = rotor.pose.Rot().RotateVector(Vector3d::UnitZ);
    vehicle.force = vehicle.torque = rotor.force = rotor.torque = Vector3d::Zero;

    // ControlCommandCallback for a velocity motor (max_rot_velocity is not
    // reached here), then one world update.
    model.ref_motor_input_ = command;
    model.sampling_time_ = dt;
    const double speed_used =
        joint.velocity * model.rotor_velocity_slowdown_sim_ * model.turning_direction_;
    model.UpdateForcesAndMoments();
    const double speed_after =
        joint.velocity * model.rotor_velocity_slowdown_sim_ * model.turning_direction_;

    // The rigid body sums the rotor link's force (at its origin) and the
    // torques on the vehicle about the center of mass.
    const Vector3d force_world = rotor.force + vehicle.force;
    const Vector3d moment_world = offset_world.Cross(rotor.force) + rotor.torque + vehicle.torque;
    const Vector3d force_body = world_from_body.RotateVectorReverse(force_world);
    const Vector3d moment_body = world_from_body.RotateVectorReverse(moment_world);
    std::printf("%.17g %.17g %.17g %.17g %.17g %.17g %.17g %.17g\n", speed_used, force_body.X(),
                force_body.Y(), force_body.Z(), moment_body.X(), moment_body.Y(), moment_body.Z(),
                speed_after);
  }
  return 0;
}
