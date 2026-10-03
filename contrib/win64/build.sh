#!/usr/bin/env bash
# Cross-build witness_node.exe for 64-bit Windows with MinGW-w64 on Linux.
#
#   contrib/win64/build-deps.sh /path/to/deps      # once
#   contrib/win64/build.sh /path/to/deps [BUILD_DIR] [TARGET...]
#
# BUILD_DIR defaults to ./build-win64, TARGET to witness_node.
set -euo pipefail

SRC_DIR="$(realpath "$(dirname "$(realpath "$0")")/../..")"
export WIN64_DEPS="$(realpath "${1:?usage: build.sh DEPS_PREFIX [BUILD_DIR] [TARGET...]}")"
BUILD_DIR="$(realpath -m "${2:-build-win64}")"
shift $(( $# >= 2 ? 2 : 1 ))
TARGETS=( "${@:-witness_node}" )

cmake -S "$SRC_DIR" -B "$BUILD_DIR" \
   -DCMAKE_TOOLCHAIN_FILE="$SRC_DIR/contrib/win64/mingw-w64-x86_64.cmake" \
   -DCMAKE_BUILD_TYPE=Release \
   -DCMAKE_PREFIX_PATH="$WIN64_DEPS" \
   -DOPENSSL_ROOT_DIR="$WIN64_DEPS" -DOPENSSL_USE_STATIC_LIBS=ON \
   -DBoost_USE_STATIC_LIBS=ON -DBoost_USE_STATIC_RUNTIME=ON \
   -DCURL_STATICLIB=ON
cmake --build "$BUILD_DIR" -j"${JOBS:-$(nproc)}" --target "${TARGETS[@]}"
