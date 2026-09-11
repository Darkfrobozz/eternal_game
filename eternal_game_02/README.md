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
  When any alternative exists, a move into a one-cell **dead end** is avoided,
  so a loop does not peel off down a start-up spur and die there.
- **Charge, per move:**
  - vertical: down `+1`, up `-1`;
  - horizontal: takes the **preceding vertical's** sign if it directly follows a
    vertical (the **combo**: `d+r`/`d+l` `+1`, `u+r`/`u+l` `-1`), otherwise `-1`
    (flat ground costs).
  - A move the battery cannot pay for ends the run as `Depleted` *before* the
    ball moves: it bursts into a flash and sparks (see `src/explosion.rs`). An
    empty battery can still roll downhill, since that gains charge.
- **Winning: the loop keeps going — then overloads.** The first cell the ball
  *actually* visits twice (`Run.visits`) is its loop closure. Returning there
  with charge ≥ the charge recorded on first arrival sets `Run.solved` and
  counts a lap; every lap multiplies `Run.speed`, so an eternal loop visibly
  accelerates (capped at [`MAX_SPEED`](src/ball.rs)). A closure that misses the
  guarantee no longer stops the ball as `Stuck` — it is doomed but runs on
  until the battery empties and it explodes. Once a solved loop completes
  [`VICTORY_LAPS`](src/ball.rs) (8) laps it overloads as `Outcome::Victory`.
  The ball then detonates the **whole level**: a shockwave sized to the level's
  bounds, a chain reaction over every cell it occupies, and a dissolve that
  burns the level to ash and then wipes the grid completely clean. When the
  blast finishes the next game level loads automatically; if there is none, a
  **VICTORY** banner appears.
- **Anything that stops the ball explodes.** `Depleted` (empty battery),
  `Stuck` (no track ahead / no solid to follow) and `Victory` all despawn the
  ball. The first two leave a single burst; victory leaves a level-wide blast
  that consumes the level and clears the whole grid.
- **Start:** `find_start` (topmost surface cell) or a placed start
  (`Placement.start`). The initial heading is derived from the solid anchor so
  the ball always sets off clockwise.

## Controls

### Title menu

The game opens on a title menu:

- **Continue Game** (only shown once progress exists) resumes the last game
  level loaded, remembered in `progress.txt`.
- **New Game** starts at the first non-tutorial level.
- **Tutorial** loads the guided level.
- **Map Editor** opens a blank board with the editor HUD on (see the
  [map editor workflow](#map-editor-workflow)).
- `Up`/`Down` or `W`/`S` move the cursor, `1`-`4` pick directly,
  `Enter`/`Space` confirm. `Esc` clears the board and returns to the title menu
  (in the editor, press `Y` to save first — `Esc` does not auto-save).

Progress is written by `load_level` whenever a game level is loaded, including
`PageDown`; the tutorial and the editor never overwrite a `Continue`.

### Playing

Normal (play-only): `Space` starts the ball rolling and pauses/resumes it in
place; `Tab` takes manual control (from pen mode it enters run mode paused and
the next press nudges the ball one cell); `E` leaves run mode from either
state; `PageDown` loads the next game level; left-drag draws solids,
right-drag erases, `C` clears. Run mode is either automatic (`Space`) or
manual (`Tab`). The charge readout and
controller hint are hidden; the player infers charge from the arrows (green
accumulates, orange consumes) and the ball's battery colour, which sweeps from
red when the battery is empty through orange and yellow to green as it charges.
The ball rolls — one full turn per cell — smoothly sliding from cell to cell and
nestling against the surface it hugs.
When the battery is empty and the ball asks for a move it cannot afford, the
ball explodes instead of taking the step; a dead end ends the same way. Once a
loop is solved the ball keeps looping and speeds up a little every lap, and
after a handful of laps the overload detonates the level, wipes the grid and
loads the next level (or shows VICTORY when it was the last).

`H` toggles **debug / map-editor mode**, which shows the HUD and enables:

- Middle-click places the start; `B` clears it back to the automatic start.
- `[` / `]` adjust the start charge.
- `M` toggles auto / manual stepping; `N` steps once while manual (in addition
  to the always-available `Tab`).
- `Y` / `L` save / load `debug_config.txt`.
- Scroll wheel zooms, WASD pans.

## Levels

`levels/*.txt` use the config format. On load, `lock_solids` records the level's
**solid** cells so the eraser cannot remove them (the derived surface is not
locked — but since it is derived, erasing a surface cell is a no-op anyway).
`PageDown` cycles the **game** levels (skipping the tutorial); the debug HUD
shows the current file name. A level's `place` and `start_charge` are its
starting condition.

A level is positioned wherever its cells sit in the 160×120 board. The camera
opens centred on the board centre, so to centre a level on screen its **solid
bounding box** must be centred on cell `(79.5, 59.5)`. `origin` is only the
crop offset used when saving; it does not place the level. Copying
`debug_config.txt` into `levels/` keeps the drawn coordinates, so recentre the
cells if you want it in the middle.

- `levels/01.txt` — a plain vertical wall. **Currently impossible** (see
  Handoff).
- `levels/02.txt` — a descending spiral that closes net-positive and wins.
- `levels/00_tutorial.txt` — a hollow box with one block missing from the top
  wall. The ball starts inside and falls out through the gap (running out of
  charge, so it explodes), until the player draws the missing block, after
  which it completes a net-zero lap and wins. A file whose name contains
  `tutorial` (case-insensitive) switches on the guided control tutorial.

## Tutorial

The tutorial is attached to any level file whose name contains `tutorial`.
Loading such a level activates [`Tutorial`](src/tutorial.rs), which shows a
eight-item checklist:

1. draw on the grid (left-click and drag),
2. `Tab` to nudge the ball one cell,
3. `Space` to start rolling,
4. `Space` again to pause,
5. the scroll wheel to zoom,
6. `W`/`A`/`S`/`D` to pan,
7. make the ball loop forever (sets `Run.solved`),
8. `E` to leave run mode.

Drawing is detected as any solid the level did not lock, and looping as
`run.solved`. The checklist is not a strict sequence: each
item latches the moment it happens, so the goals can be ticked in any order.
The prompt is a `Text2d` pinned to the top-left with `ScreenText`, so it
stays readable while the player is zooming and panning. Loading any other
level deactivates it; reloading the tutorial restarts the checklist.

## Map editor workflow

Choose **Map Editor** from the title menu. The board starts blank and the
debug HUD is on, so the full editor is available:

- left-drag draws solids, right-drag erases, `C` clears;
- middle-click places the ball's start, `B` reverts to the automatic start;
- `[` / `]` set the starting charge;
- `Y` writes the board to `debug_config.txt`, `L` loads it back;
- `Esc` clears the board and returns to the title menu, so save first.

To turn a draft into a level, press `Y`, then copy `debug_config.txt` into
`levels/NN.txt` (it is already in the config format; `parse_level` locks the
file's solids on load). Give it a `tutorial` name to attach the walkthrough.

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
- `src/menu.rs` — title menu and the top-level `Screen` (Menu/Game/Editor).
- `src/screen.rs` — `ScreenText`, a `Text2d` pinned to the window through
  zoom/pan (used by the menu and tutorial).
- `src/tutorial.rs` — the three-step control tutorial.
- `src/grid.rs` — `Grid` (cells, `solids`, `locked`, image), painting, surface
  regeneration, reachability, coordinate helpers, solid components.
- `src/ball.rs` — `Ball`, `Run`, `Tuning`, movement + charge, loop/speed,
  arrows, battery colour, HUD text.
- `src/explosion.rs` — death bursts (`Depleted`/`Stuck`), the victory
  detonation (`Detonation` state, ash dissolve, chain reaction) and the final
  **VICTORY** banner.
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
5. **Screen text is pinned, not UI.** The project deliberately keeps the `2d`
   feature set and avoids `bevy_ui`. `ScreenText` keeps the menu and tutorial
   legible under camera zoom/pan by re-reading the camera every frame and
   compensating position and scale.

### Known issues / open questions

- **Ball animation is forward-roll only.** Each move interpolates the sprite
  straight from `Ball.prev` to `Ball.cell` and rolls one full signed turn around
  the surface normal, which reads well on flat surface runs. The planned extra
  cases — a **drop** (the surface ends into empty space) and a **wall** (the
  ball hits a wall and must change direction, which wants a squash/anticipation
  beat) — are not distinguished yet; they currently use the same straight roll.

- **The win check is weak when `start_charge` is 0.** A completing loop returns
  with `≥ 0`, so *any* loop that completes is marked solved. A net-zero loop
  should probably be a draw, not a win. Consider comparing to the charge at the
  **start of the lap**, or requiring `>` rather than `≥`.
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
