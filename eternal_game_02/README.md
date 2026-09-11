The game will be made with Bevy.

The idea is a puzzle / exploration game where velocity works differently.
In particular, a ball's velocity is built up like a charge by how much it travels and it always travels clockwise.
It consumes charge if delta y is positive or zero when moving in accordance with total distance but builds up if delta y is negative.

The player will be drawing the lines on which the ball will be moving along.

Thus, the following must be created:

A capability to somehow draw.
I am thinking of representing the pixels in an array and manipulate some large sprite to fill in the world.

A capability to move along the surface of the sprite. Movement will not be controlled as per the game idea but rather the movement will simply be forward.
Thus, if we in the array can define the surface as for example 2 then we can move along the 2:s towards the 2:s that have not been marked as for example 3:s (3 for moved on and 2 for surface not yet moved on).
Then, each step the ball can move a certain amount of steps along the 2:s, this way we can have it follow the surface which could be otherwise resource intensive as we would have to define some normal at every point to determine the correct gravity and collision checking.
Certainly possible but still.

Once we establish these two the game is in large complete, I am thinking of only having around 3 levels.
The idea is that the user should figure out of the physics of the game works and the goal is to construct eternal energy.
That is once the ball is infinitely looping (reaching the same position with more or equal accumulated distance) the level is cleared.

## Current implementation

Cell values in the array: `0` empty, `1` solid (painted by the pen), `2` surface
(auto-grown around solids), `3` trail (where the ball has been).

- **Solids are the source of truth.** `Grid.solids: HashSet<IVec2>` holds every
  wall. `Grid::paint` only inserts/removes from that set and raises
  `solids_dirty`; `Grid::regenerate_surfaces` rebuilds every `2` from the solids
  (`apply_solids` runs it right after painting). There is no incremental surface
  bookkeeping.
- **Rendering.** One `Image` (`GRID_W`×`GRID_H`, one pixel per cell) blitted to
  one stretched `Sprite` with nearest sampling. `sync_image` copies `cells`
  into the texture while `dirty`.
- **Movement is orthogonal only** (`NEIGHBORS4`). The ball follows the
  8-connected solid component it started on and uses a right-hand rule
  (right > straight > left, never reversing). `Grid::reachable` is a
  4-connected flood fill of the route; a step must stay on the same component.
- **Charge, per move:**
  - vertical: down `+1`, up `-1`;
  - horizontal: takes the **preceding vertical's** sign if it directly follows a
    vertical (the **combo**: `d+r`/`d+l` `+1`, `u+r`/`u+l` `-1`), otherwise `-1`
    (flat ground costs).
  - Charge clamps at 0; hitting 0 ends the run (`Stuck`).
- **Win:** the ball re-enters a cell it *actually* visited (`Run.visits`) with
  charge ≥ the charge recorded there on first arrival.
- **Start:** `find_start` (topmost surface cell) or a placed start
  (`Placement.start`). The initial heading is derived from the solid anchor so
  the ball always sets off clockwise.

## Controls

Normal (play-only): `Space` pen / run, left-drag draws solids, right-drag
erases, `C` clears. The charge readout and controller hint are hidden; the
player infers charge from the arrows (green accumulates, orange consumes) and
the ball's battery colour (grey depleted, hue shifting as it charges).

`H` toggles **debug / map-editor mode**, which shows the HUD and enables:

- Middle-click places the start; `B` clears it back to the automatic start.
- `[` / `]` adjust the start charge.
- `M` toggles auto / manual stepping; `N` steps once while manual.
- `Y` / `L` save / load `debug_config.txt`.
- Scroll wheel zooms, WASD pans.

## Levels

`levels/*.txt` use the config format. On load, `lock_solids` records the level's
**solid** cells so the eraser cannot remove them (the derived surface is not
locked — but since it is derived, erasing a surface cell is a no-op anyway).
`Tab` cycles levels; the debug HUD shows the current file name. A level's
`place` and `start_charge` are its starting condition.

- `levels/01.txt` — a plain vertical wall. **Currently impossible** (see
  Handoff).
- `levels/02.txt` — a descending spiral that closes net-positive and wins.

## Debugging / replay

- `Y` in-game saves the current drawing, ball placement and start charge to
  `debug_config.txt`.
- `L` loads it back (this one is *not* locked, so it stays editable).
- `cargo run -- --replay [file] [steps]` runs a config headlessly and prints the
  board, every step (with charge), and the outcome. It prints a cropped ASCII
  frame after each step. Use it to reproduce any reported bug.

Config is plain text: `#` solid, `+` surface, `o` trail, `.` empty. Only the
solids and ball placement matter; the surface is rebuilt on load. It is written
`v2`, cropped to the bounding box with an `origin` line; the loader also accepts
old full-grid files (no `origin`).

## Handoff notes (for the next agent)

### Repo / build

- The project lives in the **outer** git repo `/home/darkfrobozz/weekend_jams`
  on branch **`eternal_game_02`** (each game gets its own branch; commits are
  prefixed `Eternal Game 02:`).
- `target` is a symlink to `../game_01_animations/target` so the Bevy build
  cache is shared — `cargo build` takes seconds. Don't delete it casually.
- `cargo test` (13 tests) and `cargo run`. WSLg needs `WAYLAND_DISPLAY=` blank;
  `.cargo/config.toml` already forces that.

### Code map

- `src/main.rs` — app wiring, window, camera zoom/pan, `--replay` entry point.
- `src/grid.rs` — `Grid` (cells, `solids`, `locked`, image), painting, surface
  regeneration, reachability, coordinate helpers, solid components.
- `src/ball.rs` — `Ball`, `Run`, `Tuning`, movement + charge, arrows, battery
  colour, HUD text.
- `src/paint.rs` — `Mode`, `Brush`, `Placement`, `Debug`, mouse painting, mode
  switching, HUD visibility.
- `src/config.rs` — save/load/`parse`/`serialize`, `Levels`, headless replay.
- `src/grid.rs`/`ball.rs` tests are the behavioural spec — read them first.

### Key design decisions

1. **Orthogonal-only movement.** Diagonal moves were removed. A diagonal is
   encoded as a two-step combo; the *horizontal* step carries the extra charge.
2. **The grid is one array, not entities.** No per-cell ECS; `solids` is the
   authoritative set and surface is a pure function of it.
3. **Levels lock solids only.** The eraser checks the target coordinate against
   `Grid.locked`; the pen is always allowed.
4. **The HUD is hidden by default.** Only `H` reveals charge/controls; the game
   communicates charge through arrows + ball colour.

### Known issues / open questions

- **The win check is weak when `start_charge` is 0.** A completing loop returns
  with `≥ 0` because charge clamps at 0 and a drain ends the run, so *any* loop
  that completes currently wins. A net-zero loop should probably be a draw, not
  a win. Consider comparing to the charge at the **start of the lap**, or
  requiring `>` rather than `≥`.
- **`levels/01.txt` is impossible.** Its loop has 27 consuming steps and 26
  gaining steps → net `-1` per lap, so it always returns one short. It is a good
  test case. Likely fix: make a flat that follows a flat inherit the last
  vertical's sign (propagate the combo through horizontal runs), which would
  balance 2-cell caps.
- **Combo only applies to the first flat after a vertical.** A run of `k`
  horizontals gives the first the vertical's sign and the rest `-1`. This is the
  source of the off-by-one above.
- **No explicit start direction.** The heading is always derived (clockwise from
  the anchor). A level cannot yet say "start facing left".
- **`Y` saves to `debug_config.txt`, not back to the level file.** Making a
  level is save-then-copy. A "save to current level" key would help.
- **Erasing surface is a no-op** (it regenerates). Only solids are editable.

### Parked ideas

- **Ball divergence.** At a fork, split the ball (clone per branch) and collapse
  on reconvergence. A prototype exists in git history; it was parked because the
  8-connected band produces lots of *fake* forks (diagonal vs. staircase).
  With orthogonal-only movement this may now be worth revisiting.
- **Contour tracing.** Derive one ordered boundary loop from the solids (Moore
  tracing) instead of a band, for unambiguous routes.
- **"Save to current level"** and a proper level picker/toolbar.
