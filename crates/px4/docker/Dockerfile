# PX4 SITL for one aircraft, driven by Ripple's Px4Bridge over PX4's simulator
# MAVLink API. Build context: crates/px4/docker (crates/px4/src/px4_launcher.rs
# builds it). The first build compiles PX4 and takes a while.
FROM ubuntu:24.04 AS build

ARG PX4_TAG=v1.17.0
ENV DEBIAN_FRONTEND=noninteractive
# PX4's setup script installs its own sudo and tools when told it is in Docker.
ENV RUNS_IN_DOCKER=true

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates git && \
    rm -rf /var/lib/apt/lists/*
RUN git clone --branch "${PX4_TAG}" --depth 1 --recurse-submodules --shallow-submodules \
    https://github.com/PX4/PX4-Autopilot.git /px4
RUN cd /px4 && bash Tools/setup/ubuntu.sh --no-sim-tools --no-nuttx
RUN cd /px4 && make px4_sitl_default

FROM ubuntu:24.04
RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    bc libstdc++6 python3-minimal python3-venv && \
    rm -rf /var/lib/apt/lists/*
RUN python3 -m venv /opt/px4-venv && \
    /opt/px4-venv/bin/pip install --no-cache-dir pymavlink pyulog
# The SITL binary, its command aliases, and the startup scripts and airframes
# (the startup script scales failsafe timeouts with bc).
COPY --from=build /px4/build/px4_sitl_default/bin /opt/px4/bin
COPY --from=build /px4/build/px4_sitl_default/etc /opt/px4/etc
# Ripple's Rascal airframe, found by PX4_SIM_MODEL=rascal.
COPY 4100_rascal /opt/px4/etc/init.d-posix/airframes/4100_rascal
COPY entrypoint.sh session.py pacing.py /opt/px4/
ENV PATH="/opt/px4/bin:${PATH}"
ENTRYPOINT ["/opt/px4/entrypoint.sh"]
