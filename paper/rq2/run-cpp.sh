#!/bin/bash
# Build and sweep the C++ FollowNearest experiment with SCRIMMAGE's own tools,
# inside the unmodified reference image (python3 reference/reference_check.py
# builds it). Usage: paper/rq2/run-cpp.sh scrimmage-rs-reference:<Ubuntu-24.04 commit>
set -e
IMAGE=${1:?usage: $0 IMAGE}
HERE=$(cd "$(dirname "$0")" && pwd)
docker run --rm --platform linux/amd64 --entrypoint bash -v "$HERE/cpp:/rq2:ro" "$IMAGE" -c '
set -e
cd /tmp
python3 /opt/source/scripts/create-scrimmage-project.py rq2-project . > /dev/null
/opt/source/scripts/generate-plugin.sh autonomy FollowNearest rq2-project > /dev/null
cd rq2-project
# The upstream templates were reformatted so the generator no longer matches
# its placeholders; the generated code does not compile until they are fixed.
grep -c "PLUGIN_NAME < < <" src/plugins/autonomy/FollowNearest/FollowNearest.cpp | sed "s/^/unreplaced placeholders: /"
cp /rq2/FollowNearest.cpp src/plugins/autonomy/FollowNearest/
cp /rq2/FollowNearest.h /rq2/FollowNearest.xml include/rq2-project/plugins/autonomy/FollowNearest/
cp /rq2/follow-nearest.xml /rq2/sweep.sh .
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -Dscrimmage_DIR=/opt/build -DSETUP_LOCAL_CONFIG_DIR=OFF > /dev/null
cmake --build build --target FollowNearest_plugin -j4 > /dev/null
export SCRIMMAGE_PLUGIN_PATH=$PWD/build/plugin_libs:$PWD/include/rq2-project/plugins:$SCRIMMAGE_PLUGIN_PATH
export LD_LIBRARY_PATH=$PWD/build/plugin_libs:$LD_LIBRARY_PATH PATH=/opt/build/bin:$PATH
./sweep.sh'
