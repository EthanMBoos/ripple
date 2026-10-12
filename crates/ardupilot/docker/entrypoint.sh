#!/bin/sh
# SITL's JSON backend needs a numeric address for the simulator, so resolve the
# host name (host.docker.internal by default) to IPv4 before starting.
set -eu

: "${RIPPLE_HOST:=host.docker.internal}"
set -- $(getent ahostsv4 "${RIPPLE_HOST}")
export RIPPLE_ADDRESS="$1"
exec /opt/ardupilot-venv/bin/python /opt/ardupilot/session.py
