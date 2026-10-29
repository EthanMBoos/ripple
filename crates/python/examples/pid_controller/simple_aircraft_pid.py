# Heading, altitude, and airspeed PID loops for SimpleAircraft. The same
# controller as the engine's SimpleAircraftControllerPID (crates/core/src/
# plugin/controller/simple_aircraft_pid/simple_aircraft_pid.rs), line for
# line, so the two can be compared on the same mission.
from math import fmod, pi, sqrt, tau


def angle_pi(angle_rad):
    """Wrap to (-pi, pi], as the engine does."""
    wrapped = fmod(angle_rad, tau)
    if wrapped < 0.0:
        wrapped += tau
    return wrapped - tau if wrapped > pi else wrapped


class Pid:
    """One loop. The integral accumulates only while the error is within
    `integrate_within`: error units, or degrees for an angular loop."""

    def __init__(self, kp, ki, kd, integrate_within, angular=False):
        self.kp, self.ki, self.kd = kp, ki, kd
        self.band = integrate_within * pi / 180.0 if angular else integrate_within
        self.angular = angular
        self.integral_error = 0.0
        self.previous_error = 0.0

    def step(self, setpoint, measurement, dt_s):
        error = setpoint - measurement
        if self.angular:
            error = angle_pi(error)
        if abs(error) > self.band:
            self.integral_error = 0.0
        else:
            self.integral_error += error * dt_s
        error_rate = (error - self.previous_error) / max(dt_s, 1e-9)
        self.previous_error = error
        return self.kp * error + self.ki * self.integral_error + self.kd * error_rate


class SimpleAircraftPid:
    ports = {"inputs": {"desired_heading": ("rad", "world"),
                        "desired_altitude": ("m", "world"),
                        "desired_speed": ("m/s", "none")},
             "outputs": {"throttle": ("none", "none"),
                         "roll_rate": ("rad/s", "model"),
                         "pitch_rate": ("rad/s", "model")}}

    def __init__(self, heading_pid=None, altitude_pid=None, speed_pid=None):
        heading_pid = heading_pid or dict(kp=1.0, ki=0.01, kd=2.0, integrate_within_deg=9.0)
        altitude_pid = altitude_pid or dict(kp=1.0, ki=0.0, kd=0.8, integrate_within_m=1.0)
        speed_pid = speed_pid or dict(kp=1.0, ki=0.1, kd=0.0, integrate_within_mps=1.0)
        self.heading = Pid(heading_pid["kp"], heading_pid["ki"], heading_pid["kd"],
                           heading_pid["integrate_within_deg"], angular=True)
        self.altitude = Pid(altitude_pid["kp"], altitude_pid["ki"], altitude_pid["kd"],
                            altitude_pid["integrate_within_m"])
        self.speed = Pid(speed_pid["kp"], speed_pid["ki"], speed_pid["kd"],
                         speed_pid["integrate_within_mps"])

    def step(self, ctx):
        state, dt_s = ctx.state, ctx.time.dt_s
        vx, vy, vz = state.velocity
        speed_mps = sqrt(vx * vx + vy * vy + vz * vz)
        # Heading increases counterclockwise, and a right bank (positive roll)
        # turns clockwise, so a heading short of the goal calls for rolling left.
        roll_rate = -self.heading.step(ctx.inputs["desired_heading"], state.heading, dt_s)
        # Positive pitch is nose down.
        pitch_rate = -self.altitude.step(ctx.inputs["desired_altitude"], state.position[2], dt_s)
        throttle = self.speed.step(ctx.inputs["desired_speed"], speed_mps, dt_s)
        return {"throttle": throttle, "roll_rate": roll_rate, "pitch_rate": pitch_rate}
