#!/usr/bin/env bash
# Cross-build the static dependencies of witness_node.exe with MinGW-w64 on Linux.
#
#   contrib/win64/build-deps.sh [PREFIX]     (default: ./win64-deps)
#
# Needs: g++-mingw-w64-x86-64-posix, cmake, make, perl, git; building the node also needs wine.
# Sources are cloned from GitHub at fixed tags; the result is PREFIX/{include,lib}.
set -euo pipefail

PREFIX="$(realpath -m "${1:-win64-deps}")"
SRC="$PREFIX/src"
JOBS="${JOBS:-$(nproc)}"
HOST=x86_64-w64-mingw32
TOOLCHAIN="$(dirname "$(realpath "$0")")/mingw-w64-x86_64.cmake"

BOOST_TAG=boost-1.90.0
OPENSSL_TAG=openssl-3.5.9
ZLIB_TAG=v1.3.1
CURL_TAG=curl-8_19_0

export WIN64_DEPS="$PREFIX"
export CC=$HOST-gcc-posix CXX=$HOST-g++-posix AR=$HOST-ar RANLIB=$HOST-ranlib RC=$HOST-windres

mkdir -p "$SRC"

fetch() { # repo tag dir [extra git args]
   local repo=$1 tag=$2 dir=$SRC/$3; shift 3
   [ -d "$dir" ] || git clone -q --depth 1 --branch "$tag" "$@" "https://github.com/$repo.git" "$dir"
}

# --- zlib ---------------------------------------------------------------------
if [ ! -f "$PREFIX/lib/libz.a" ]; then
   fetch madler/zlib $ZLIB_TAG zlib
   make -C "$SRC/zlib" -f win32/Makefile.gcc -j"$JOBS" PREFIX=$HOST- CC=$CC libz.a
   install -D -m644 "$SRC/zlib/libz.a" "$PREFIX/lib/libz.a"
   install -D -m644 -t "$PREFIX/include" "$SRC/zlib/zlib.h" "$SRC/zlib/zconf.h"
fi

# --- OpenSSL ------------------------------------------------------------------
if [ ! -f "$PREFIX/lib/libcrypto.a" ]; then
   fetch openssl/openssl $OPENSSL_TAG openssl
   ( cd "$SRC/openssl"
     ./Configure mingw64 no-shared no-tests no-docs no-apps --prefix="$PREFIX" --libdir=lib
     make -j"$JOBS" build_libs
     make install_dev )
fi

# --- curl (Schannel, so it needs no OpenSSL) ------------------------------------
if [ ! -f "$PREFIX/lib/libcurl.a" ]; then
   fetch curl/curl $CURL_TAG curl
   cmake -S "$SRC/curl" -B "$SRC/curl/build" -DCMAKE_TOOLCHAIN_FILE="$TOOLCHAIN" \
      -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_PREFIX_PATH="$PREFIX" \
      -DBUILD_SHARED_LIBS=OFF -DBUILD_STATIC_LIBS=ON -DBUILD_CURL_EXE=OFF -DBUILD_TESTING=OFF \
      -DBUILD_LIBCURL_DOCS=OFF -DBUILD_MISC_DOCS=OFF -DENABLE_CURL_MANUAL=OFF \
      -DCURL_USE_SCHANNEL=ON -DCURL_USE_OPENSSL=OFF -DCURL_USE_LIBPSL=OFF -DCURL_USE_LIBSSH2=OFF \
      -DCURL_ZLIB=OFF -DCURL_BROTLI=OFF -DCURL_ZSTD=OFF -DUSE_NGHTTP2=OFF -DUSE_LIBIDN2=OFF \
      -DCURL_DISABLE_LDAP=ON
   cmake --build "$SRC/curl/build" -j"$JOBS"
   cmake --install "$SRC/curl/build"
fi

# --- Boost --------------------------------------------------------------------
if [ ! -f "$PREFIX/lib/cmake/Boost-${BOOST_TAG#boost-}/BoostConfig.cmake" ]; then
   fetch boostorg/boost $BOOST_TAG boost --recurse-submodules --shallow-submodules -j8
   ( cd "$SRC/boost"
     [ -x b2 ] || CC= CXX= ./bootstrap.sh --with-toolset=gcc
     echo "using gcc : mingw : $CXX : <archiver>$AR <ranlib>$RANLIB <rc>$RC ;" > user-config.jam
     ./b2 -j"$JOBS" -q --user-config=user-config.jam --prefix="$PREFIX" \
        toolset=gcc-mingw target-os=windows address-model=64 architecture=x86 \
        binary-format=pe abi=ms threadapi=win32 \
        variant=release link=static runtime-link=static threading=multi \
        -sNO_BZIP2=1 -sNO_ZSTD=1 -sNO_LZMA=1 -sZLIB_INCLUDE="$PREFIX/include" -sZLIB_LIBPATH="$PREFIX/lib" \
        --with-thread --with-iostreams --with-date_time --with-filesystem --with-program_options \
        --with-chrono --with-test --with-context --with-coroutine --with-regex --with-system \
        --layout=system install )
fi

echo "Dependencies installed in $PREFIX"
