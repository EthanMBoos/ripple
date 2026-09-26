#!/bin/sh
# Native macOS build of unmodified C++ SCRIMMAGE (Ubuntu-24.04) used by run.py.
# Needs: brew install boost@1.85 protobuf@21 eigen@3 geographiclib libxml2 cmake
# (the versions Ubuntu 24.04 ships), and the rapidxml headers copied from the
# arm64 reference image (run reference/benchmark.py once to build it). No C++
# source is changed. Run from the repository root.
set -eu
N=runs/native-cpp
H=$(brew --prefix)
mkdir -p $N/include
git -C ../scrimmage archive --format=tar --prefix=source/ Ubuntu-24.04 | tar -x -C $N
IMAGE=scrimmage-rs-reference:$(git -C ../scrimmage rev-parse --short=12 Ubuntu-24.04)-arm64
docker run --rm --platform linux/arm64 --entrypoint tar "$IMAGE" \
  -c -C /usr/include rapidxml | tar -x -C $N/include
cmake -S $N/source -B $N/build -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_CXX_COMPILER=/usr/bin/clang++ -DCMAKE_C_COMPILER=/usr/bin/clang \
  -DCMAKE_PREFIX_PATH="$H/opt/boost@1.85;$H/opt/protobuf@21;$H/opt/eigen@3;$H/opt/geographiclib;$H/opt/libxml2" \
  -DBoost_DIR="$H/opt/boost@1.85/lib/cmake/Boost-1.85.0" -DCMAKE_IGNORE_PREFIX_PATH="/opt/anaconda3;$H" \
  -DCMAKE_CXX_FLAGS="-I$PWD/$N/include -isystem $H/opt/boost@1.85/include -isystem $H/opt/eigen@3/include/eigen3 -isystem $H/opt/protobuf@21/include" \
  -DSETUP_HOME_CONFIG=OFF -DCMAKE_EXPORT_NO_PACKAGE_REGISTRY=ON \
  -DBUILD_TESTS=OFF -DBUILD_DOCS=OFF -DBUILD_ROS_PLUGINS=OFF -DENABLE_GPU_ACCELERATION=OFF \
  -DCMAKE_DISABLE_FIND_PACKAGE_VTK=ON -DVTK_FOUND=OFF -DCMAKE_DISABLE_FIND_PACKAGE_GRPC=ON \
  -DCMAKE_DISABLE_FIND_PACKAGE_OpenCV=ON
cmake --build $N/build --parallel --target scrimmage-bin Straight_plugin \
  SimpleAircraftControllerPID_plugin SimpleAircraft_plugin SimpleCollision_plugin \
  SimpleCollisionMetrics_plugin LocalNetwork_plugin GlobalNetwork_plugin NoisyState_plugin \
  NoisyContacts_plugin GroundCollision_plugin
