#!/usr/bin/env bash
# Procedural game SFX generator built on ffmpeg (no external assets needed).
#
# Usage:
#   tools/make_sfx.sh explosion          # -> assets/sfx_partials/explosion.wav
#   tools/make_sfx.sh all                # generate every preset (partials)
#   tools/make_sfx.sh finalize           # promote the curated finals -> assets/sfx/
#   tools/make_sfx.sh ls                 # list presets
#
# Requires: ffmpeg (already installed). Output is 44.1kHz mono 16-bit WAV.
#
# All generated/working results go to assets/sfx_partials/.
# assets/sfx/ is kept clean and contains ONLY the curated FINALS below.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/assets/sfx_partials"
FINAL_DIR="$ROOT/assets/sfx"
mkdir -p "$OUT_DIR" "$FINAL_DIR"

# Names promoted to assets/sfx/ by `make_sfx.sh finalize`.
FINALS="explosion_small explosion_boom explosion_crack_dissolve_fast"

ff() { ffmpeg -y -hide_banner -loglevel error "$@"; }

# Big low explosion: brown-noise body + sharp crack + sub rumble.
explosion() {
  ff -f lavfi -i "anoisesrc=d=2:c=brown:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2:c=white:a=1.0:r=44100" \
     -f lavfi -i "sine=frequency=55:sample_rate=44100:duration=2" \
     -filter_complex "\
       [0:a]lowpass=f=700,volume=1.3,afade=t=out:st=0:d=2:curve=exp[body];\
       [1:a]highpass=f=1500,afade=t=out:st=0:d=0.09:curve=exp[crk];\
       [2:a]afade=t=out:st=0:d=0.5:curve=exp[sub];\
       [body][crk][sub]amix=inputs=3:normalize=0,acompressor,alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion.wav"
}

# Retro laser: fast downward sweep.
laser() {
  ff -f lavfi -i "sine=frequency=1200:sample_rate=44100:duration=0.35" \
     -af "asetrate=44100,aeval=val(0)*exp(-6*t),volume=0.9,afade=t=out:st=0.25:d=0.1" \
     -c:a pcm_s16le "$OUT_DIR/laser.wav" 2>/dev/null || \
  ff -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=0.35" \
     -af "highpass=f=200,volume=0.9,afade=t=out:st=0:d=0.35:curve=exp" \
     -c:a pcm_s16le "$OUT_DIR/laser.wav"
}

# Short percussive hit.
hit() {
  ff -f lavfi -i "anoisesrc=d=0.25:c=white:a=1.0:r=44100" \
     -af "highpass=f=800,lowpass=f=6000,afade=t=out:st=0:d=0.25:curve=exp,volume=0.9" \
     -c:a pcm_s16le "$OUT_DIR/hit.wav"
}

# Bright rising pickup blip.
pickup() {
  ff -f lavfi -i "sine=frequency=880:sample_rate=44100:duration=0.2" \
     -af "volume=0.7,afade=t=in:st=0:d=0.01,afade=t=out:st=0.12:d=0.08" \
     -c:a pcm_s16le "$OUT_DIR/pickup.wav"
}

# Soft whoosh (UI / dash).
whoosh() {
  ff -f lavfi -i "anoisesrc=d=0.6:c=pink:a=0.8:r=44100" \
     -af "highpass=f=400,lowpass=f=4000,afade=t=in:st=0:d=0.15,afade=t=out:st=0.3:d=0.3,volume=0.8" \
     -c:a pcm_s16le "$OUT_DIR/whoosh.wav"
}

# Tight, punchy blast (~0.75s) - good for frequent gameplay hits.
explosion_short() {
  ff -f lavfi -i "anoisesrc=d=0.8:c=brown:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=0.8:c=white:a=1.0:r=44100" \
     -f lavfi -i "sine=frequency=70:sample_rate=44100:duration=0.8" \
     -filter_complex "\
       [0:a]lowpass=f=500,volume=1.5,afade=t=out:st=0:d=0.7:curve=exp[body];\
       [1:a]highpass=f=2000,afade=t=out:st=0:d=0.05:curve=exp[crk];\
       [2:a]afade=t=out:st=0:d=0.25:curve=exp[sub];\
       [body][crk][sub]amix=inputs=3:normalize=0,acompressor,alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_short.wav"
}

# Distant, rumbling blast (~3.9s) - muffled low end with a long echo tail.
explosion_deep() {
  ff -f lavfi -i "anoisesrc=d=3.5:c=brown:a=1.0:r=44100" \
     -f lavfi -i "aevalsrc='0.9*exp(-1.2*t)*sin(2*PI*(60*t-13.75*t*t))':d=3.5:s=44100" \
     -filter_complex "\
       [0:a]lowpass=f=220,volume=1.6,afade=t=out:st=0:d=3.5:curve=exp[body];\
       [1:a]afade=t=out:st=0:d=2.0:curve=exp[sub];\
       [body][sub]amix=inputs=2:normalize=0,\
       aecho=0.8:0.7:180|420:0.3|0.15,\
       lowpass=f=600,acompressor,alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_deep.wav"
}

# Retro 8-bit blast (~0.6s) - pitch-dropping square wave + crushed noise.
explosion_retro() {
  ff -f lavfi -i "aevalsrc='0.55*exp(-3*t)*sgn(sin(2*PI*(700*t-610*t*t)))':d=0.6:s=44100" \
     -f lavfi -i "anoisesrc=d=0.6:c=white:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]volume=0.9[ton];\
       [1:a]highpass=f=600,lowpass=f=8000,afade=t=out:st=0:d=0.4:curve=exp,\
       acrusher=bits=3:mode=log:aa=1:level_out=0.6[noi];\
       [ton][noi]amix=inputs=2:normalize=0,\
       acrusher=bits=6:mode=log,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_retro.wav"
}

# Airy hiss dissolving into nothing (~2.5s) - ash / dust / fade-away.
dissolve_ash() {
  ff -f lavfi -i "anoisesrc=d=2.5:c=white:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2.5:c=pink:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2.5:c=brown:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]highpass=f=4000,afade=t=out:st=0:d=2.5:curve=exp,volume=0.5[air];\
       [1:a]highpass=f=1200,lowpass=f=6000,tremolo=f=22:d=0.7,\
       afade=t=out:st=0:d=2.0:curve=exp,volume=0.7[fizz];\
       [2:a]lowpass=f=300,afade=t=out:st=0:d=0.8:curve=exp,volume=0.8[poof];\
       [air][fizz][poof]amix=inputs=3:normalize=0,\
       aecho=0.6:0.5:90|220:0.25|0.12,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/dissolve_ash.wav"
}

# Cracking / splitting wall (~1.6s) - sparse snaps + low creak + rumble.
crack_wall() {
  ff -f lavfi -i "aevalsrc='(random(0)*2-1)*(lt(random(0),0.00035)+lt(mod(t,0.47),0.004)+lt(mod(t+0.19,0.71),0.005))':d=1.6:s=44100" \
     -f lavfi -i "sine=frequency=85:sample_rate=44100:duration=1.6" \
     -f lavfi -i "anoisesrc=d=1.6:c=brown:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]highpass=f=1500,afade=t=out:st=0:d=1.6:curve=exp,volume=0.7[crack];\
       [1:a]vibrato=f=6:d=0.4,tremolo=f=8:d=0.5,lowpass=f=500,\
       afade=t=in:st=0:d=0.05,afade=t=out:st=1.0:d=0.6,volume=0.5[creak];\
       [2:a]lowpass=f=250,afade=t=out:st=0:d=1.4:curve=exp,volume=0.9[rumble];\
       [crack][creak][rumble]amix=inputs=3:normalize=0,\
       acompressor,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/crack_wall.wav"
}

# --- crack_wall alternatives ---

# Heavy structural split: big initial snap, crumbling debris, deep rumble (~2.4s).
crack_wall_02() {
  ff -f lavfi -i "anoisesrc=d=2.4:c=white:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2.4:c=brown:a=1.0:r=44100" \
     -f lavfi -i "aevalsrc='(random(0)*2-1)*(lt(random(0),0.0006)+lt(mod(t,0.31),0.005)+lt(mod(t+0.13,0.53),0.006)+lt(mod(t+0.27,0.83),0.004))':d=2.4:s=44100" \
     -f lavfi -i "sine=frequency=58:sample_rate=44100:duration=2.4" \
     -filter_complex "\
       [0:a]highpass=f=500,lowpass=f=9000,afade=t=out:st=0:d=0.08:curve=exp,volume=1.2[snap];\
       [1:a]lowpass=f=200,volume=1.4,afade=t=out:st=0:d=2.4:curve=exp[rumb];\
       [2:a]highpass=f=1200,afade=t=out:st=0:d=2.4:curve=exp,volume=0.6[crack];\
       [3:a]vibrato=f=5:d=0.5,tremolo=f=7:d=0.6,lowpass=f=450,\
       afade=t=in:st=0.05:d=0.1,afade=t=out:st=1.2:d=1.0,volume=0.5[creak];\
       [snap][rumb][crack][creak]amix=inputs=4:normalize=0,\
       acompressor,alimiter=limit=0.92[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/crack_wall_02.wav"
}

# Dry, brittle crack - stone / tile / pottery, snappy and high (~1.2s).
crack_wall_03() {
  ff -f lavfi -i "aevalsrc='(random(0)*2-1)*(lt(random(0),0.00025)+lt(mod(t,0.28),0.003)+lt(mod(t+0.09,0.41),0.004)+lt(mod(t+0.22,0.67),0.003))':d=1.2:s=44100" \
     -f lavfi -i "anoisesrc=d=1.2:c=brown:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]highpass=f=2200,lowpass=f=12000,afade=t=out:st=0:d=1.2:curve=exp,volume=0.8[crack];\
       [1:a]lowpass=f=400,afade=t=out:st=0:d=0.5:curve=exp,volume=0.5[rumb];\
       [crack][rumb]amix=inputs=2:normalize=0,acompressor,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/crack_wall_03.wav"
}

# Slow structural groan, then a split partway through and falling debris (~3s).
crack_wall_04() {
  ff -f lavfi -i "sine=frequency=52:sample_rate=44100:duration=3.0" \
     -f lavfi -i "anoisesrc=d=1.8:c=white:a=1.0:r=44100" \
     -f lavfi -i "aevalsrc='(random(0)*2-1)*lt(random(0),0.0004)':d=1.6:s=44100" \
     -f lavfi -i "anoisesrc=d=3.0:c=brown:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]vibrato=f=4:d=0.6,tremolo=f=5:d=0.5,lowpass=f=350,\
       afade=t=in:st=0:d=0.3,afade=t=out:st=1.8:d=1.2,volume=0.55[groan];\
       [1:a]highpass=f=700,lowpass=f=9000,afade=t=out:st=0:d=0.09:curve=exp,\
       volume=1.1,adelay=1200[snap];\
       [2:a]highpass=f=1800,afade=t=in:st=0:d=0.05,afade=t=out:st=0.8:d=0.8,\
       volume=0.5,adelay=1200[debris];\
       [3:a]lowpass=f=220,afade=t=out:st=0:d=3.0:curve=exp[rumb];\
       [groan][snap][debris][rumb]amix=inputs=4:normalize=0,\
       acompressor,alimiter=limit=0.92[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/crack_wall_04.wav"
}

# --- dissolve_ash alternatives ---

# Soft dust collapse: muffled low poof + gentle hiss (~1.8s).
dissolve_ash_02() {
  ff -f lavfi -i "anoisesrc=d=1.8:c=brown:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=1.8:c=pink:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]lowpass=f=400,afade=t=out:st=0:d=0.9:curve=exp,volume=1.0[poof];\
       [1:a]highpass=f=900,lowpass=f=5000,afade=t=out:st=0:d=1.8:curve=exp,volume=0.5[hiss];\
       [poof][hiss]amix=inputs=2:normalize=0,lowpass=f=6000,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/dissolve_ash_02.wav"
}

# Shimmering magical dissolve: airy hiss + twinkling high tones + phaser (~2.2s).
dissolve_ash_03() {
  ff -f lavfi -i "anoisesrc=d=2.2:c=white:a=1.0:r=44100" \
     -f lavfi -i "sine=frequency=2400:sample_rate=44100:duration=2.2" \
     -f lavfi -i "sine=frequency=3600:sample_rate=44100:duration=2.2" \
     -f lavfi -i "anoisesrc=d=2.2:c=pink:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]highpass=f=3000,afade=t=out:st=0:d=2.2:curve=exp,volume=0.45[air];\
       [1:a]tremolo=f=13:d=0.9,volume=0.12,afade=t=out:st=0:d=2.0:curve=exp[s1];\
       [2:a]tremolo=f=19:d=0.9,volume=0.08,afade=t=out:st=0:d=1.8:curve=exp[s2];\
       [3:a]highpass=f=600,lowpass=f=7000,afade=t=out:st=0:d=2.2:curve=exp,volume=0.4[body];\
       [air][s1][s2][body]amix=inputs=4:normalize=0,\
       aphaser=type=t:decay=0.4:speed=0.8,\
       aecho=0.5:0.4:70|160:0.2|0.1,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/dissolve_ash_03.wav"
}

# Fast vaporize: quick hiss + falling tone that vanishes (~0.8s).
dissolve_ash_04() {
  ff -f lavfi -i "anoisesrc=d=0.8:c=white:a=1.0:r=44100" \
     -f lavfi -i "aevalsrc='0.4*exp(-7*t)*sin(2*PI*(500*t-300*t*t))':d=0.8:s=44100" \
     -filter_complex "\
       [0:a]highpass=f=1000,lowpass=f=9000,afade=t=in:st=0:d=0.03,\
       afade=t=out:st=0.15:d=0.65:curve=exp,volume=0.8[air];\
       [1:a]afade=t=out:st=0:d=0.5:curve=exp[tone];\
       [air][tone]amix=inputs=2:normalize=0,\
       aecho=0.4:0.3:60:0.2,alimiter=limit=0.9[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/dissolve_ash_04.wav"
}

# Long decelerating dissolve: dense at first, slows to a sparse fizzle (~6.2s).
dissolve_ash_slow() {
  ff -f lavfi -i "anoisesrc=d=6:c=white:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=6:c=pink:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=6:c=brown:a=1.0:r=44100" \
     -f lavfi -i "aevalsrc='(random(0)*2-1)*(lt(random(0),0.006*exp(-0.9*t)+0.00002))':d=6:s=44100" \
     -f lavfi -i "aevalsrc='0.7+0.3*sin(2*PI*(7*t-0.5*t*t))':d=6:s=44100" \
     -filter_complex "\
       [0:a]highpass=f=2500,afade=t=out:st=0:d=6:curve=exp,volume=0.5[air];\
       [1:a]highpass=f=800,lowpass=f=6000,afade=t=out:st=0:d=6:curve=exp,volume=0.5[body];\
       [2:a]lowpass=f=350,afade=t=out:st=0:d=1.5:curve=exp,volume=0.9[poof];\
       [3:a]highpass=f=1500,lowpass=f=11000,volume=0.7[crackle];\
       [air][body][crackle]amix=inputs=3:normalize=0[mix];\
       [4:a]volume=1.6[mod];\
       [mix][mod]amultiply[mixed];\
       [mixed][poof]amix=inputs=2:normalize=0,\
       aecho=0.5:0.4:90|220:0.2|0.1,volume=3.2,alimiter=limit=0.92[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/dissolve_ash_slow.wav"
}

# Cracking wall, a beat of silence, then dissolving to ash.
# Uses crack_wall_02 + dissolve_ash_02; generates them first if missing.
crack_dissolve() {
  [ -f "$OUT_DIR/crack_wall_02.wav" ] || crack_wall_02
  [ -f "$OUT_DIR/dissolve_ash_02.wav" ] || dissolve_ash_02
  ff -f lavfi -t 0.6 -i "anullsrc=r=44100:cl=mono" \
     -i "$OUT_DIR/crack_wall_02.wav" \
     -i "$OUT_DIR/dissolve_ash_02.wav" \
     -filter_complex "[1:a][0:a][2:a]concat=n=3:v=0:a=1[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/crack_dissolve.wav"
}

# Same sequence but with the long decelerating dissolve (~8s).
crack_dissolve_slow() {
  [ -f "$OUT_DIR/crack_wall_02.wav" ] || crack_wall_02
  [ -f "$OUT_DIR/dissolve_ash_slow.wav" ] || dissolve_ash_slow
  ff -f lavfi -t 0.6 -i "anullsrc=r=44100:cl=mono" \
     -i "$OUT_DIR/crack_wall_02.wav" \
     -i "$OUT_DIR/dissolve_ash_slow.wav" \
     -filter_complex "[1:a][0:a][2:a]concat=n=3:v=0:a=1[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/crack_dissolve_slow.wav"
}

# Full sequence: deep explosion, beat, cracking wall, beat, slow dissolve (~14s).
explosion_crack_dissolve() {
  [ -f "$OUT_DIR/explosion_deep.wav" ] || explosion_deep
  [ -f "$OUT_DIR/crack_dissolve_slow.wav" ] || crack_dissolve_slow
  ff -f lavfi -t 0.4 -i "anullsrc=r=44100:cl=mono" \
     -i "$OUT_DIR/explosion_deep.wav" \
     -i "$OUT_DIR/crack_dissolve_slow.wav" \
     -filter_complex "[1:a][0:a][2:a]concat=n=3:v=0:a=1[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_crack_dissolve.wav"
}

# Punchy mix: close layered explosion (punch + body) up front, crack/ash tucked under it.
explosion_crack_dissolve_punch() {
  [ -f "$OUT_DIR/explosion_short.wav" ] || explosion_short
  [ -f "$OUT_DIR/explosion.wav" ] || explosion
  [ -f "$OUT_DIR/crack_wall_02.wav" ] || crack_wall_02
  [ -f "$OUT_DIR/dissolve_ash_slow.wav" ] || dissolve_ash_slow
  ff -f lavfi -t 0.5 -i "anullsrc=r=44100:cl=mono" \
     -f lavfi -t 0.6 -i "anullsrc=r=44100:cl=mono" \
     -i "$OUT_DIR/explosion_short.wav" \
     -i "$OUT_DIR/explosion.wav" \
     -i "$OUT_DIR/crack_wall_02.wav" \
     -i "$OUT_DIR/dissolve_ash_slow.wav" \
     -filter_complex "\
       [2:a]volume=1.7[punch];\
       [3:a]volume=1.3[body];\
       [punch][body]amix=inputs=2:normalize=0,asubboost=boost=4[boom];\
       [4:a]volume=0.5[crack];\
       [5:a]volume=0.4[ash];\
       [boom][0:a][crack][1:a][ash]concat=n=5:v=0:a=1[cat];\
       [cat]alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_crack_dissolve_punch.wav"
}

# Close, punchy explosion built purely from noise (no tonal sub -> not a "drum").
explosion_boom() {
  ff -f lavfi -i "anoisesrc=d=2.2:c=white:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2.2:c=brown:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2.2:c=pink:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=2.2:c=white:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]highpass=f=250,lowpass=f=9000,afade=t=out:st=0:d=0.35:curve=exp,volume=1.1[blast];\
       [1:a]lowpass=f=500,afade=t=out:st=0:d=1.8:curve=exp,volume=1.5[body];\
       [2:a]highpass=f=120,lowpass=f=3500,afade=t=out:st=0:d=1.2:curve=exp,volume=0.9[mid];\
       [3:a]highpass=f=3000,afade=t=out:st=0:d=0.06:curve=exp,volume=0.9[crack];\
       [blast][body][mid][crack]amix=inputs=4:normalize=0,\
       acompressor,alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_boom.wav"
}

# Final sequence: close noise explosion, beat, cracking wall, beat, slow dissolve (~12s).
explosion_crack_dissolve_boom() {
  [ -f "$OUT_DIR/explosion_boom.wav" ] || explosion_boom
  [ -f "$OUT_DIR/crack_wall_02.wav" ] || crack_wall_02
  [ -f "$OUT_DIR/dissolve_ash_slow.wav" ] || dissolve_ash_slow
  ff -f lavfi -t 0.5 -i "anullsrc=r=44100:cl=mono" \
     -f lavfi -t 0.6 -i "anullsrc=r=44100:cl=mono" \
     -i "$OUT_DIR/explosion_boom.wav" \
     -i "$OUT_DIR/crack_wall_02.wav" \
     -i "$OUT_DIR/dissolve_ash_slow.wav" \
     -filter_complex "\
       [2:a]volume=1.5[boom];\
       [3:a]volume=0.5[crack];\
       [4:a]volume=0.25[ash];\
       [boom][0:a][crack][1:a][ash]concat=n=5:v=0:a=1[cat];\
       [cat]alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_crack_dissolve_boom.wav"
}

# Small, tight explosion (~0.5s) - quick blast, light thump, short crack.
explosion_small() {
  ff -f lavfi -i "anoisesrc=d=0.5:c=white:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=0.5:c=pink:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=0.5:c=brown:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=0.5:c=white:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]highpass=f=400,lowpass=f=8000,afade=t=out:st=0:d=0.15:curve=exp,volume=1.0[blast];\
       [1:a]highpass=f=200,lowpass=f=4500,afade=t=out:st=0:d=0.3:curve=exp,volume=0.8[body];\
       [2:a]lowpass=f=350,afade=t=out:st=0:d=0.22:curve=exp,volume=0.9[thump];\
       [3:a]highpass=f=3500,afade=t=out:st=0:d=0.03:curve=exp,volume=0.8[crack];\
       [blast][body][thump][crack]amix=inputs=4:normalize=0,\
       acompressor,alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_small.wav"
}

# Short decelerating dissolve (~3s): same character, faster timeline.
dissolve_ash_fast() {
  ff -f lavfi -i "anoisesrc=d=3:c=white:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=3:c=pink:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=3:c=brown:a=1.0:r=44100" \
     -f lavfi -i "aevalsrc='(random(0)*2-1)*(lt(random(0),0.008*exp(-1.6*t)+0.00002))':d=3:s=44100" \
     -f lavfi -i "aevalsrc='0.7+0.3*sin(2*PI*(9*t-1.0*t*t))':d=3:s=44100" \
     -filter_complex "\
       [0:a]highpass=f=2500,afade=t=out:st=0:d=3:curve=exp,volume=0.5[air];\
       [1:a]highpass=f=800,lowpass=f=6000,afade=t=out:st=0:d=3:curve=exp,volume=0.5[body];\
       [2:a]lowpass=f=350,afade=t=out:st=0:d=1.0:curve=exp,volume=0.9[poof];\
       [3:a]highpass=f=1500,lowpass=f=11000,volume=0.7[crackle];\
       [air][body][crackle]amix=inputs=3:normalize=0[mix];\
       [4:a]volume=1.6[mod];\
       [mix][mod]amultiply[mixed];\
       [mixed][poof]amix=inputs=2:normalize=0,\
       aecho=0.5:0.4:90|220:0.2|0.1,volume=3.2,alimiter=limit=0.92[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/dissolve_ash_fast.wav"
}

# Fast interleaved sequence (~6s): crack starts under the explosion's tail,
# then the short dissolve comes in before the crack fully finishes.
explosion_crack_dissolve_fast() {
  [ -f "$OUT_DIR/explosion_boom.wav" ] || explosion_boom
  [ -f "$OUT_DIR/crack_wall_02.wav" ] || crack_wall_02
  [ -f "$OUT_DIR/dissolve_ash_fast.wav" ] || dissolve_ash_fast
  ff -i "$OUT_DIR/explosion_boom.wav" \
     -i "$OUT_DIR/crack_wall_02.wav" \
     -i "$OUT_DIR/dissolve_ash_fast.wav" \
     -filter_complex "\
       [0:a]volume=1.5[boom];\
       [1:a]volume=0.5,adelay=1200[crack];\
       [2:a]volume=0.25,adelay=3000[ash];\
       [boom][crack][ash]amix=inputs=3:normalize=0,\
       alimiter=limit=0.95[out]" \
     -map "[out]" -c:a pcm_s16le "$OUT_DIR/explosion_crack_dissolve_fast.wav"
}

# Seamless rolling loop builder. mode: smooth | textured
_make_rolling() {
  local out="$1" mode="$2" tex="$OUT_DIR/.rolling_tex.wav"
  local core_mod="" surf_mod=""
  if [ "$mode" = "textured" ]; then
    core_mod=",tremolo=f=0.5:d=0.15"   # slow swell, not a beat
    surf_mod=",tremolo=f=18:d=0.35"    # fast flutter = surface grain
  fi
  # pass 1: layered rolling texture (loop length + crossfade margin)
  ff -f lavfi -i "anoisesrc=d=4.5:c=brown:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=4.5:c=pink:a=1.0:r=44100" \
     -f lavfi -i "anoisesrc=d=4.5:c=white:a=1.0:r=44100" \
     -filter_complex "\
       [0:a]lowpass=f=180,volume=0.25[low];\
       [1:a]highpass=f=300,lowpass=f=4000${core_mod},volume=1.0[core];\
       [2:a]highpass=f=3500,lowpass=f=8000${surf_mod},volume=0.12[surf];\
       [low][core][surf]amix=inputs=3:normalize=0[tex]" \
     -map "[tex]" -c:a pcm_s16le "$tex"
  # pass 2: loop = middle + crossfade(tail -> head); end and start meet at source[0.5]
  ff -ss 0.5 -t 3.5 -i "$tex" \
     -ss 4   -t 0.5 -i "$tex" \
     -ss 0   -t 0.5 -i "$tex" \
     -filter_complex "\
       [1:a][2:a]acrossfade=d=0.5[wrap];\
       [0:a][wrap]concat=n=2:v=0:a=1[out]" \
     -map "[out]" -c:a pcm_s16le "$out"
  rm -f "$tex"
}

# Steady ball rolling on a surface - no periodic pulse (that read as a steam train).
rolling() { _make_rolling "$OUT_DIR/rolling_loop.wav" smooth; }

# Rougher surface: adds a fast flutter on the high band for grain.
rolling_textured() { _make_rolling "$OUT_DIR/rolling_textured_loop.wav" textured; }

# 20s versions of the loops, for auditioning continuity in a normal player.
rolling_extended() {
  [ -f "$OUT_DIR/rolling_loop.wav" ] || rolling
  ff -stream_loop 4 -i "$OUT_DIR/rolling_loop.wav" -c:a pcm_s16le "$OUT_DIR/rolling_extended.wav"
}
rolling_textured_extended() {
  [ -f "$OUT_DIR/rolling_textured_loop.wav" ] || rolling_textured
  ff -stream_loop 4 -i "$OUT_DIR/rolling_textured_loop.wav" -c:a pcm_s16le "$OUT_DIR/rolling_textured_extended.wav"
}

PRESETS="explosion explosion_short explosion_small explosion_deep explosion_boom explosion_retro crack_wall crack_wall_02 crack_wall_03 crack_wall_04 crack_dissolve crack_dissolve_slow explosion_crack_dissolve explosion_crack_dissolve_punch explosion_crack_dissolve_boom explosion_crack_dissolve_fast dissolve_ash dissolve_ash_02 dissolve_ash_03 dissolve_ash_04 dissolve_ash_slow dissolve_ash_fast rolling rolling_textured rolling_extended rolling_textured_extended laser hit pickup whoosh"

case "${1:-ls}" in
  ls) printf 'Presets: %s\n' "$PRESETS" ;;
  finals) printf 'Finals: %s\n' "$FINALS" ;;
  all) for p in $PRESETS; do "$p"; done; ls -la "$OUT_DIR" ;;
  finalize)
    for p in $FINALS; do
      [ -f "$OUT_DIR/$p.wav" ] || "$p"
      cp "$OUT_DIR/$p.wav" "$FINAL_DIR/"
    done
    echo "promoted into $FINAL_DIR:"; ls -1 "$FINAL_DIR"/*.wav
    ;;
  rolling) rolling; echo "wrote $OUT_DIR/rolling_loop.wav" ;;
  rolling_textured) rolling_textured; echo "wrote $OUT_DIR/rolling_textured_loop.wav" ;;
  explosion|explosion_short|explosion_small|explosion_deep|explosion_boom|explosion_retro|crack_wall|crack_wall_02|crack_wall_03|crack_wall_04|crack_dissolve|crack_dissolve_slow|explosion_crack_dissolve|explosion_crack_dissolve_punch|explosion_crack_dissolve_boom|explosion_crack_dissolve_fast|dissolve_ash|dissolve_ash_02|dissolve_ash_03|dissolve_ash_04|dissolve_ash_slow|dissolve_ash_fast|rolling_extended|rolling_textured_extended|laser|hit|pickup|whoosh) "$1"; echo "wrote $OUT_DIR/$1.wav" ;;
  *) echo "unknown preset: $1"; echo "presets: $PRESETS"; exit 1 ;;
esac
