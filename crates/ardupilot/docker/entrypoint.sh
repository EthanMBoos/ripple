#!/bin/sh
# SITL's JSON backend needs a numeric address for the simulator, so resolve the
# host name (host.docker.internal by default) to IPv4 before starting.
set -eu

: "${SCRIMMAGE_HOST:=host.docker.internal}"
set -- $(getent ahostsv4 "${SCRIMMAGE_HOST}")
export SCRIMMAGE_ADDRESS="$1"
exec /opt/ardupilot-venv/bin/python /opt/ardupilot/session.py
