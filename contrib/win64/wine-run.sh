#!/usr/bin/env bash
# CMAKE_CROSSCOMPILING_EMULATOR for the build's own helpers (cat-parts, embed_genesis).
# Windows reads "/home/x" as relative to the current drive, so absolute Unix paths are rewritten to Z:/...
# (Wine maps Z: to /), both as whole arguments and inside embed_genesis's "template---output" pairs.
exe=$1; shift
args=()
for a in "$@"; do
   a=$(sed -E 's#(^|---)/#\1Z:/#g' <<<"$a")
   args+=("$a")
done
WINEDEBUG=${WINEDEBUG:--all} exec "${WINE:-wine}" "$exe" "${args[@]}"
