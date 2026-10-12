# RotorS's motor-plugin source, compiled with harness.cpp for
# reference/rotors_motor.py. Ubuntu 22.04 ships the ignition-math6 that
# RotorS's Gazebo 11 plugins use; no Gazebo or ROS is installed.
FROM ubuntu:22.04
ARG ROTORS_COMMIT
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates g++ git libignition-math-dev python3 \
    && rm -rf /var/lib/apt/lists/*
RUN git clone https://github.com/ethz-asl/rotors_simulator.git /rotors \
    && git -C /rotors checkout --quiet "$ROTORS_COMMIT"
COPY extract.py harness.cpp /harness/
RUN python3 /harness/extract.py /rotors /harness \
    && g++ -std=c++17 -O0 -ffp-contract=off -I/usr/include/ignition/math6 \
        -o /usr/local/bin/rotors-harness /harness/harness.cpp
ENTRYPOINT ["/usr/local/bin/rotors-harness"]
