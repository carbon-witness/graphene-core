# witness_node.exe for Windows (cross-build)

Builds a single static `witness_node.exe` for 64-bit Windows 10+ on a Linux host with MinGW-w64.
It depends only on DLLs that ship with Windows.

## Host packages (Ubuntu)

    apt-get install g++-mingw-w64-x86-64-posix cmake make perl git wine64

Wine runs the build's own helpers (`cat-parts`, `embed_genesis`) through `wine-run.sh`.

## Build

    contrib/win64/build-deps.sh ~/win64-deps          # Boost 1.90, OpenSSL 3.5, zlib, curl; ~30 min, once
    contrib/win64/build.sh ~/win64-deps build-win64   # -> build-win64/programs/witness_node/witness_node.exe

## Files

| File | Role |
| --- | --- |
| `mingw-w64-x86_64.cmake` | CMake toolchain: `-posix` MinGW compilers, static link, `-mbig-obj`, Wine as emulator |
| `build-deps.sh` | Clones the dependencies at fixed tags and installs them as static libraries |
| `build.sh` | Configures and builds the node with the toolchain |
| `fc-windows.patch` | Fixes `libraries/fc` needs for this build; apply in graphene-fc with `git am` until fc carries them |
| `wine-run.sh` | Runs a build helper under Wine, rewriting `/abs/path` arguments to `Z:/abs/path` |

## Stopping the node from a GUI

Windows only: `--shutdown-event <name>` (a named event the GUI creates and signals) and
`--parent-pid <pid>` (the node exits when that process does). Both take the same clean exit path as
Ctrl+C, so the database is not left dirty.
