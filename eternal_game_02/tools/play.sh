#!/usr/bin/env bash
# Play a generated SFX through the default audio output.
#
# Usage:
#   tools/play.sh explosion_crack_dissolve_fast   # looks in assets/sfx then assets/sfx_partials
#   tools/play.sh all                             # play every wav in both folders
#
# Under WSLg, ffplay can hang on exit; ffmpeg's native Pulse output is used
# first and is real-time paced with -re. Falls back to ffplay if that fails.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEARCH_DIRS=("$ROOT/assets/sfx" "$ROOT/assets/sfx_partials")

find_sfx() {
  local name="${1%.wav}" dir
  for dir in "${SEARCH_DIRS[@]}"; do
    if [ -f "$dir/$name.wav" ]; then echo "$dir/$name.wav"; return 0; fi
  done
  return 1
}

play_file() {
  local file="$1"
  echo ">>> $(basename "$file" .wav)  [$file]"
  if ffmpeg -hide_banner -loglevel error -re -i "$file" -f pulse default 2>/dev/null; then
    return 0
  fi
  echo "(falling back to ffplay)" >&2
  SDL_AUDIODRIVER=pulse ffplay -nodisp -autoexit -loglevel error "$file"
}

case "${1:-}" in
  ""|-h|--help) echo "usage: tools/play.sh <name|all>"; exit 0 ;;
  all)
    for dir in "${SEARCH_DIRS[@]}"; do
      for f in "$dir"/*.wav; do [ -e "$f" ] && play_file "$f"; done
    done
    ;;
  *)
    file="$(find_sfx "$1")" || { echo "no such sfx: $1" >&2; exit 1; }
    play_file "$file"
    ;;
esac
