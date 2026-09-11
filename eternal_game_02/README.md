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

- The pen draws `1`s. Every adjacent empty cell becomes a `2`.
- Movement is orthogonal only. The ball follows the 8-connected solid mass it
  started on (its `component`) and uses a right-hand rule (right > straight >
  left, never reversing).
- Charge is per move: a **vertical** step is signed by its direction (down
  `+1`, up `-1`). A **horizontal** step is normally `0`, but if it follows a
  vertical it is converted to that vertical's sign — the **combo**. Charge
  hitting zero ends the run.
- A run is won when the ball reaches a previously visited cell with at least
  the charge it had on the previous visit (a self-sustaining loop).

## Controls

Normal (play-only): `Space` pen / run, left-drag draws solids, right-drag
erases, `C` clears. The charge readout and controller hint are hidden; the
player infers charge from the arrows (green accumulates, orange consumes) and
the ball's battery colour (grey when depleted, hue shifting as it charges).

`H` toggles **debug / map-editor mode**, which shows the HUD and enables:

- Middle-click places the start; `B` clears it back to the automatic start.
- `[` / `]` adjust the start charge.
- `M` toggles auto / manual stepping; `N` steps once while manual.
- `Y` / `L` save / load `debug_config.txt`.
- Scroll wheel zooms, WASD pans.

## Levels

`levels/*.txt` are loaded with the config format. Level geometry (every
non-empty cell at load) is **locked**: the player can draw their own strokes
but cannot erase the level. `Tab` cycles levels; the debug HUD shows the
current file name. A level's `place` and `start_charge` are its starting
condition.

## Debugging / replay

- `Y` in-game saves the current drawing, ball placement and start charge to
  `debug_config.txt`.
- `L` loads it back.
- `cargo run -- --replay [file] [steps]` runs it headlessly and prints the board
  and the ball's exact path, step by step, so a reported bug can be reproduced.

The config is plain text: `#` solid, `+` surface, `o` trail, `.` empty. Only the
solids and the ball placement really matter; the surface is rebuilt on load.

## Potential ideas (parked)

- **Ball divergence.** At a fork in the route, instead of picking one arm,
  split the ball: spawn a clone down every branch and collapse them back to one
  when the branches reconverge. See the git history on the `eternal_game_02`
  branch for a prototype. The blocker is that the 8-connected surface produces
  many *fake* forks (diagonal vs. staircase paths that are really the same
  route), so it needs a single-width route generation first. It would also need
  a decision on how splitting affects charge and what the win condition is.
- **Contour-tracing the route.** Derive a single ordered boundary loop from the
  solid cells (Moore neighbourhood tracing) instead of growing a band, which
  would remove route ambiguity at the source.


