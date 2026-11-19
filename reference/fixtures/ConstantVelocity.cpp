// Reference-only open-loop velocity driver; this is not an upstream SCRIMMAGE plugin.
// No retained upstream autonomy writes a nonzero velocity command, so this
// supplies the same constant ENU command in both languages to test the
// UNMODIFIED upstream SingleIntegratorControllerSimple and SingleIntegrator.
#include <cmath>
#include <sstream>
#include <stdexcept>
#include <vector>

#include <scrimmage/autonomy/Autonomy.h>
#include <scrimmage/common/VariableIO.h>
#include <scrimmage/plugin_manager/RegisterPlugin.h>

class ConstantVelocity : public scrimmage::Autonomy {
 public:
    void init(std::map<std::string, std::string>& params) override {
        std::istringstream input(params.at("velocity"));
        double value;
        while (input >> value) {
            if (!std::isfinite(value)) throw std::runtime_error("nonfinite velocity");
            velocity_.push_back(value);
        }
        if (velocity_.size() != 3 || !input.eof()) {
            throw std::runtime_error("ConstantVelocity.velocity needs three numbers");
        }
        using Type = scrimmage::VariableIO::Type;
        using Direction = scrimmage::VariableIO::Direction;
        ports_ = {vars_.declare(Type::velocity_x, Direction::Out),
                  vars_.declare(Type::velocity_y, Direction::Out),
                  vars_.declare(Type::velocity_z, Direction::Out)};
    }

    bool step_autonomy(double, double) override {
        for (std::size_t i = 0; i < 3; ++i) vars_.output(ports_[i], velocity_[i]);
        return true;
    }

 private:
    std::vector<int> ports_;
    std::vector<double> velocity_;
};

REGISTER_PLUGIN(scrimmage::Autonomy, ConstantVelocity, ConstantVelocity_plugin)
