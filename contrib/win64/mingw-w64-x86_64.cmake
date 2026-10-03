# CMake toolchain for cross-compiling to 64-bit Windows with MinGW-w64 on Linux.
# The -posix compilers are required: the node uses std::thread.
set( CMAKE_SYSTEM_NAME Windows )
set( CMAKE_SYSTEM_PROCESSOR x86_64 )

set( MINGW_TRIPLE x86_64-w64-mingw32 )
set( CMAKE_C_COMPILER   ${MINGW_TRIPLE}-gcc-posix )
set( CMAKE_CXX_COMPILER ${MINGW_TRIPLE}-g++-posix )
set( CMAKE_RC_COMPILER  ${MINGW_TRIPLE}-windres )

# WIN64_DEPS (environment) = the prefix build-deps.sh installed into; the environment also reaches try_compile
set( CMAKE_FIND_ROOT_PATH /usr/${MINGW_TRIPLE} $ENV{WIN64_DEPS} )
set( CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER )
set( CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY )
set( CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY )
set( CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY )

# Large translation units (database_api, the operation visitors) exceed the default COFF section limit
set( CMAKE_C_FLAGS_INIT   "-Wa,-mbig-obj" )
set( CMAKE_CXX_FLAGS_INIT "-Wa,-mbig-obj" )
# One self-contained .exe: no libstdc++, libgcc or winpthread DLLs next to it.
# -L: Boost's iostreams config links zlib by bare name ("z").
set( CMAKE_EXE_LINKER_FLAGS_INIT "-static -L$ENV{WIN64_DEPS}/lib" )

# The build runs its own helpers (cat-parts, embed_genesis); built for Windows, they need Wine here
set( CMAKE_CROSSCOMPILING_EMULATOR ${CMAKE_CURRENT_LIST_DIR}/wine-run.sh )
