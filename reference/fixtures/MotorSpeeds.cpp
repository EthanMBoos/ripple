// Reference-only open-loop controller; this is not an upstream SCRIMMAGE plugin.
// The old MultirotorControllerPID writes u_, but current Multirotor reads
// VariableIO motor_N channels. Supply identical constant rad/s commands in both
// languages to test the UNMODIFIED upstream physics without ArduPilot.
#include <cmath>
#include <sstream>
#include <stdexcept>
#include <vector>

#include <scrimmage/common/VariableIO.h>
#include <scrimmage/motion/Controller.h>
#include <scrimmage/plugin_manager/RegisterPlugin.h>

class MotorSpeeds : public scrimmage::Controller {
 public:
    void init(std::map<std::string, std::string>& params) override {
        std::istringstream input(params.at("speeds"));
        double speed;
        while (input >> speed) {
            if (!std::isfinite(speed)) throw std::runtime_error("nonfinite motor speed");
            const auto name = "motor_" + std::to_string(speeds_.size());
            ports_.push_back(vars_.declare(name, scrimmage::VariableIO::Direction::Out));
            speeds_.push_back(speed);
        }
        if (speeds_.empty() || !input.eof()) throw std::runtime_error("invalid motor speeds");
    }

    bool step(double, double) override {
        for (std::size_t i = 0; i < speeds_.size(); ++i) vars_.output(ports_[i], speeds_[i]);
        return true;
    }

 private:
    std::vector<int> ports_;
    std::vector<double> speeds_;
};

REGISTER_PLUGIN(scrimmage::Controller, MotorSpeeds, MotorSpeeds_plugin)
