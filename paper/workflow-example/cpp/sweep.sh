#!/bin/bash
# C++ speed sweep. run_experiments.py and scrimmage_runner.sh need the
# scrimmage.utils Python package, which is absent from Ubuntu-24.04, and
# scrimmage has no command-line variable overrides, so substitute by hand.
set -e
mkdir -p out/missions
for speed in 18 22 26 30; do
  sed "s/\${speed=30}/$speed/" follow-nearest.xml > out/missions/speed-$speed.xml
  scrimmage out/missions/speed-$speed.xml > out/speed-$speed.log 2>&1
  echo "speed $speed: $(ls -td ~/.scrimmage/logs/*/ | head -1)"
  tail -2 "$(ls -td ~/.scrimmage/logs/*/ | head -1)summary.csv"
done
