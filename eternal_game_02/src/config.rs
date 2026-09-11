//! Saving/loading debug configurations, plus a headless replay mode.
//!
//! - `Y` in-game writes `debug_config.txt` (grid + ball placement + tuning).
//! - `L` loads it back.
//! - `cargo run -- --replay [file] [steps]` runs it headlessly and prints the
//!   geometry and the ball's exact path, so a reported bug can be reproduced.
use std::fs;

use bevy::prelude::*;

use crate::ball::{Ball, Outcome, Run, Tuning};
use crate::grid::{Cell, Grid};
use crate::paint::{Mode, Placement};

/// Where `F5` writes and `F9` / `--replay` read by default.
pub const CONFIG_PATH: &str = "debug_config.txt";

fn cell_char(cell: Cell) -> char {
    match cell {
        Cell::Empty => '.',
        Cell::Solid => '#',
        Cell::Surface => '+',
        Cell::Trail => 'o',
    }
}

fn char_cell(c: char) -> Option<Cell> {
    match c {
        '.' => Some(Cell::Empty),
        '#' => Some(Cell::Solid),
        '+' => Some(Cell::Surface),
        'o' => Some(Cell::Trail),
        _ => None,
    }
}

/// Render the board as text. The ball (or any marker) shows as `@`.
pub fn ascii(grid: &Grid, marker: Option<IVec2>) -> String {
    let mut out = String::new();
    for y in (0..grid.h).rev() {
        for x in 0..grid.w {
            let cell = IVec2::new(x, y);
            out.push(if Some(cell) == marker {
                '@'
            } else {
                cell_char(grid.get(cell).unwrap_or(Cell::Empty))
            });
        }
        out.push('\n');
    }
    out
}

/// Serialise the whole state to the text format.
pub fn serialize(grid: &Grid, place: Option<IVec2>, tuning: &Tuning) -> String {
    let mut out = String::new();
    out.push_str("# eternal_game_02 config v1\n");
    out.push_str(&format!("start_charge {}\n", tuning.start_charge));
    match place {
        Some(p) => out.push_str(&format!("place {} {}\n", p.x, p.y)),
        None => out.push_str("place none\n"),
    }
    out.push_str(&format!("grid {} {}\n", grid.w, grid.h));
    out.push_str(&ascii(grid, place));
    out
}

/// Parse the text format back into a runnable state.
pub fn parse(text: &str) -> Option<(Grid, Option<IVec2>, Tuning)> {
    let mut start_charge = 0.0;
    let mut place = None;
    let mut dims: Option<(i32, i32)> = None;
    let mut rows: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("start_charge ") {
            start_charge = rest.trim().parse().ok()?;
        } else if let Some(rest) = line.strip_prefix("place ") {
            if rest.trim() != "none" {
                let mut it = rest.split_whitespace();
                let x: i32 = it.next()?.parse().ok()?;
                let y: i32 = it.next()?.parse().ok()?;
                place = Some(IVec2::new(x, y));
            }
        } else if let Some(rest) = line.strip_prefix("grid ") {
            let mut it = rest.split_whitespace();
            let w: i32 = it.next()?.parse().ok()?;
            let h: i32 = it.next()?.parse().ok()?;
            dims = Some((w, h));
        } else {
            rows.push(line);
        }
    }

    let (w, h) = dims?;
    if rows.len() != h as usize {
        return None;
    }

    let mut grid = Grid::new(Handle::default());
    grid.w = w;
    grid.h = h;
    grid.cells = vec![Cell::Empty; (w * h) as usize];
    // Rows are printed top (max y) first.
    for (row, line) in rows.iter().enumerate() {
        let y = h - 1 - row as i32;
        for (x, ch) in line.chars().enumerate() {
            if x as i32 >= w {
                break;
            }
            if let Some(cell) = char_cell(ch) {
                grid.set(IVec2::new(x as i32, y), cell);
            }
        }
    }
    grid.dirty = true;
    // The surface is derived; rebuild it from the solids so a hand-written
    // config (solids + empties only) loads too.
    grid.rebuild_surface();

    Some((grid, place, Tuning { start_charge }))
}

/// `Y` save / `L` load.
pub fn debug_io(
    keys: Res<ButtonInput<KeyCode>>,
    mut grid: ResMut<Grid>,
    mut place: ResMut<Placement>,
    mut tuning: ResMut<Tuning>,
    mut run: ResMut<Run>,
    mut mode: ResMut<Mode>,
) {
    if keys.just_pressed(KeyCode::KeyY) {
        match fs::write(CONFIG_PATH, serialize(&grid, place.start, &tuning)) {
            Ok(()) => info!("Saved config to {CONFIG_PATH}"),
            Err(e) => warn!("Could not save {CONFIG_PATH}: {e}"),
        }
    }
    if keys.just_pressed(KeyCode::KeyL) {
        match fs::read_to_string(CONFIG_PATH).ok().and_then(|t| parse(&t)) {
            Some((loaded, loaded_place, loaded_tuning)) => {
                *grid = loaded;
                place.start = loaded_place;
                *tuning = loaded_tuning;
                *run = Run::default();
                *mode = Mode::Paint;
                info!("Loaded config from {CONFIG_PATH}");
            }
            None => warn!("Could not load {CONFIG_PATH}"),
        }
    }
}

/// Headless replay used by `--replay`.
pub fn replay(path: &str, max_steps: usize) {
    let Ok(text) = fs::read_to_string(path) else {
        eprintln!("cannot read {path}");
        return;
    };
    let Some((mut grid, place, tuning)) = parse(&text) else {
        eprintln!("could not parse {path}");
        return;
    };

    println!("{}", ascii(&grid, place));
    let Some(start) = place
        .filter(|c| grid.is_track(*c))
        .or_else(|| grid.find_start())
    else {
        println!("no start cell");
        return;
    };

    let mut run = Run::default();
    let mut ball = Ball::new(start, tuning.start_charge);
    crate::ball::start_run(&mut run, &grid, start, tuning.start_charge);
    println!(
        "start={start:?} charge={} route={} component={:?}",
        tuning.start_charge,
        run.route.len(),
        ball.component,
    );

    for i in 0..max_steps {
        if run.outcome != Outcome::Running {
            break;
        }
        let from = ball.cell;
        crate::ball::step_once(&mut grid, &mut run, &mut ball);
        println!(
            "{i:4}: {from:?} -> {:?} dir={:?} charge={:.1}",
            ball.cell, ball.dir, ball.charge
        );
    }
    println!("outcome: {:?}", run.outcome);
    println!("{}", ascii(&grid, Some(ball.cell)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_rebuilds_surface() {
        let mut grid = Grid::new(Handle::default());
        for y in 10..20 {
            grid.paint(IVec2::new(20, y), Cell::Solid);
        }
        let tuning = Tuning { start_charge: 4.0 };
        let text = serialize(&grid, Some(IVec2::new(19, 20)), &tuning);

        let (back, place, loaded_tuning) = parse(&text).expect("parses");
        assert_eq!(place, Some(IVec2::new(19, 20)));
        assert_eq!(loaded_tuning.start_charge, 4.0);
        // Solids survive and the surface is regenerated from them.
        assert_eq!(back.get(IVec2::new(20, 15)), Some(Cell::Solid));
        assert!(back.is_track(IVec2::new(19, 15)));
    }
}
