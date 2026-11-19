#!/bin/sh
# PX4 works in a folder inside the container, so that its parameter and
# mission files, written often, never wait on the mounted run folder (Docker
# Desktop writes to it slowly). Only the flight log goes to the aircraft's
# folder in the run directory, mounted at /run/sitl.
set -eu
mkdir -p /tmp/px4 /run/sitl/log
ln -sfn /run/sitl/log /tmp/px4/log
cd /tmp/px4
exec /opt/px4-venv/bin/python /opt/px4/session.py
