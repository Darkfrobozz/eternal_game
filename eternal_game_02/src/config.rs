//! Saving/loading debug configurations, plus a headless replay mode.
//!
//! - `Y` in-game writes `debug_config.txt` (grid + ball placement + tuning).
//! - `L` loads it back.
//! - `cargo run -- --replay [file] [steps]` runs it headlessly and prints the
//!   geometry and the ball's exact path, so a reported bug can be reproduced.
//!
//! Only the bounding box of non-empty cells is written, so a small drawing
//! makes a small file.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::ball::{Ball, Outcome, Run, Tuning};
use crate::explosion::Detonation;
use crate::grid::{CELL_PX, Cell, GRID_H, Grid};
use crate::paint::{Debug, Mode, Placement};
use crate::tutorial::Tutorial;

/// Where `Y` writes and `L` / `--replay` read by default.
pub const CONFIG_PATH: &str = "debug_config.txt";

/// Where `Continue` remembers the last game level the player loaded.
pub const PROGRESS_PATH: &str = "progress.txt";

/// The last game level the player loaded, for the menu's Continue option.
/// Mirrors `progress.txt`.
#[derive(Resource, Default)]
pub struct Progress {
    pub saved: Option<String>,
}

/// Read the saved level file name, if there is one.
pub fn saved_level() -> Option<String> {
    let name = fs::read_to_string(PROGRESS_PATH).ok()?;
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Remember `path` as the current game level. The tutorial and the editor do
/// not call this, so they never clobber a `Continue`.
fn save_progress(path: &Path) {
    let Some(name) = path.file_name() else {
        return;
    };
    if let Err(error) = fs::write(PROGRESS_PATH, name.to_string_lossy().as_bytes()) {
        warn!("Could not save progress to {PROGRESS_PATH}: {error}");
    }
}

fn cell_char(cell: Cell) -> char {
    match cell {
        Cell::Empty => '.',
        Cell::Solid => '#',
    }
}

fn char_cell(c: char) -> Option<Cell> {
    match c {
        '.' => Some(Cell::Empty),
        '#' => Some(Cell::Solid),
        // Legacy surface and trail markers: there is no stored surface, so both
        // read as empty and the track is derived from the solids on load.
        '+' | 'o' => Some(Cell::Empty),
        _ => None,
    }
}

/// Bounding box of every non-empty cell, or `None` if the board is blank.
fn content_bounds(grid: &Grid) -> Option<(IVec2, IVec2)> {
    grid.content_bounds()
}

/// Render the solid region as text. The marker (ball) shows as `@`. The crop
/// includes a one-cell ring so the open cells the ball walks are visible too.
pub fn ascii(grid: &Grid, marker: Option<IVec2>) -> String {
    let Some((min, max)) = content_bounds(grid) else {
        return String::new();
    };
    let mut min = min - IVec2::ONE;
    let mut max = max + IVec2::ONE;
    if let Some(m) = marker {
        min = min.min(m);
        max = max.max(m);
    }
    let mut out = String::new();
    for y in (min.y..=max.y).rev() {
        for x in min.x..=max.x {
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
    let (min, max) = content_bounds(grid).unwrap_or((IVec2::ZERO, IVec2::ZERO));

    let mut out = String::new();
    out.push_str("# eternal_game_02 config v2\n");
    out.push_str(&format!("start_charge {}\n", tuning.start_charge));
    match place {
        Some(p) => out.push_str(&format!("place {} {}\n", p.x, p.y)),
        None => out.push_str("place none\n"),
    }
    out.push_str(&format!("origin {} {}\n", min.x, min.y));
    out.push_str(&format!("grid {} {}\n", max.x - min.x + 1, max.y - min.y + 1));
    for y in (min.y..=max.y).rev() {
        for x in min.x..=max.x {
            out.push(cell_char(grid.get(IVec2::new(x, y)).unwrap_or(Cell::Empty)));
        }
        out.push('\n');
    }
    out
}

/// Parse the text format back into a runnable state. `image` is the live grid
/// texture handle (the headless replay passes the default handle).
pub fn parse(text: &str, image: Handle<Image>) -> Option<(Grid, Option<IVec2>, Tuning)> {
    let mut start_charge = 0.0;
    let mut place = None;
    let mut origin = IVec2::ZERO;
    let mut dims: Option<(i32, i32)> = None;
    let mut rows: Vec<&str> = Vec::new();

    for line in text.lines() {
        // Only the header uses `# `; a solid row is `#` with no space after it.
        if line.starts_with("# ") || line.trim().is_empty() {
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
        } else if let Some(rest) = line.strip_prefix("origin ") {
            let mut it = rest.split_whitespace();
            origin.x = it.next()?.parse().ok()?;
            origin.y = it.next()?.parse().ok()?;
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

    let mut grid = Grid::new(image);
    // Rows are printed top (max y) first.
    for (row, line) in rows.iter().enumerate() {
        let y = origin.y + (h - 1 - row as i32);
        for (x, ch) in line.chars().take(w as usize).enumerate() {
            if let Some(cell) = char_cell(ch) {
                grid.set(IVec2::new(origin.x + x as i32, y), cell);
            }
        }
    }

    Some((
        grid,
        place,
        Tuning {
            start_charge,
            manual: false,
            speed: 1.0,
        },
    ))
}

/// Load a level: parse, recentre it on the board, then lock every non-empty
/// cell so the geometry can't be erased (only the player's own strokes can be).
pub fn parse_level(text: &str, image: Handle<Image>) -> Option<(Grid, Option<IVec2>, Tuning)> {
    let (mut grid, place, tuning) = parse(text, image)?;
    let shift = grid.recenter_solids();
    let place = place.map(|cell| cell + shift);
    grid.lock_solids();
    Some((grid, place, tuning))
}

/// The level files found under `levels/`, and which one is loaded.
#[derive(Resource, Default)]
pub struct Levels {
    pub files: Vec<PathBuf>,
    pub current: isize,
}

impl Levels {
    pub fn scan() -> Self {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir("levels") {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "txt") {
                    files.push(path);
                }
            }
        }
        files.sort();
        Self { files, current: -1 }
    }

    /// The next non-tutorial level, wrapping around. `Tab` uses this so the
    /// tutorial stays a menu-only experience.
    pub fn next_game(&mut self) -> Option<PathBuf> {
        if self.files.is_empty() {
            return None;
        }
        let len = self.files.len() as isize;
        for _ in 0..self.files.len() {
            self.current = (self.current + 1).rem_euclid(len);
            let path = &self.files[self.current as usize];
            if !is_tutorial(path) {
                return Some(path.clone());
            }
        }
        None
    }

    /// The first non-tutorial level — where **New Game** starts.
    pub fn first_game(&mut self) -> Option<PathBuf> {
        let index = self.files.iter().position(|path| !is_tutorial(path))?;
        self.current = index as isize;
        Some(self.files[index].clone())
    }

    /// The next non-tutorial level *after* the current one, without wrapping.
    /// `None` means the current level was the last: the game is won.
    pub fn next_game_after(&mut self) -> Option<PathBuf> {
        let start = self.current.max(0) as usize;
        let (index, _) = self
            .files
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, path)| !is_tutorial(path))?;
        self.current = index as isize;
        Some(self.files[index].clone())
    }

    /// The tutorial level, for the menu's **Tutorial** entry.
    pub fn tutorial(&mut self) -> Option<PathBuf> {
        let index = self.files.iter().position(|path| is_tutorial(path))?;
        self.current = index as isize;
        Some(self.files[index].clone())
    }

    /// Find a level by file name (used by **Continue**).
    pub fn find(&mut self, name: &str) -> Option<PathBuf> {
        let index = self.files.iter().position(|path| {
            path.file_name()
                .map(|file| file.to_string_lossy() == name)
                .unwrap_or(false)
        })?;
        self.current = index as isize;
        Some(self.files[index].clone())
    }

    pub fn label(&self) -> String {
        self.files
            .get(self.current.max(0) as usize)
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .unwrap_or_else(|| "no levels".into())
    }
}

/// A level is a tutorial if its file name says so, e.g. `00_tutorial.txt`.
fn is_tutorial(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase().contains("tutorial"))
        .unwrap_or(false)
}

/// Replace the running state with the level in `path`.
///
/// Also used by the main menu to start a new game, so it is `pub`.
pub fn load_level(
    path: &Path,
    grid: &mut Grid,
    place: &mut Placement,
    tuning: &mut Tuning,
    run: &mut Run,
    mode: &mut Mode,
    tutorial: &mut Tutorial,
    progress: &mut Progress,
) {
    let Ok(text) = fs::read_to_string(path) else {
        warn!("Could not read level {}", path.display());
        return;
    };
    let Some((loaded, loaded_place, loaded_tuning)) = parse_level(&text, grid.image.clone()) else {
        warn!("Could not parse level {}", path.display());
        return;
    };
    *grid = loaded;
    place.start = loaded_place;
    // Keep the player's speed preference across level changes.
    let speed = tuning.speed;
    *tuning = loaded_tuning;
    tuning.speed = speed;
    *run = Run::default();
    *mode = Mode::Paint;
    let tutorial_level = is_tutorial(path);
    tutorial.set_level(tutorial_level);
    // Remember game progress, but not the tutorial or the editor.
    if !tutorial_level {
        progress.saved = path.file_name().map(|name| name.to_string_lossy().into_owned());
        save_progress(path);
    }
    info!("Loaded level {}", path.display());
}

/// Marks the level-name line of the debug HUD.
#[derive(Component)]
pub struct LevelText;

/// Keep the HUD label in sync with the loaded level.
pub fn update_level_text(levels: Res<Levels>, mut texts: Query<&mut Text2d, With<LevelText>>) {
    if !levels.is_changed() {
        return;
    }
    for mut text in &mut texts {
        text.0 = format!("Level: {}", levels.label());
    }
}

/// Startup: scan `levels/` and spawn the (hidden) level-name label. No level
/// is loaded yet — the main menu decides whether to start a game or open the
/// map editor.
pub fn setup_levels(
    mut commands: Commands,
    mut levels: ResMut<Levels>,
    mut progress: ResMut<Progress>,
) {
    commands.spawn((
        Text2d::new("Level: -"),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.65, 0.70, 0.82)),
        Transform::from_xyz(0.0, GRID_H as f32 * CELL_PX / 2.0 - 64.0, 10.0),
        LevelText,
        crate::paint::HudText,
        Visibility::Hidden,
    ));
    *levels = Levels::scan();
    progress.saved = saved_level();
    info!("Found {} level(s) in levels/", levels.files.len());
}

/// `PageDown` loads the next game level. (`Tab` is reserved for nudging the
/// ball one step, see [`crate::ball::manual_step`].)
#[allow(clippy::too_many_arguments)]
pub fn cycle_level(
    keys: Res<ButtonInput<KeyCode>>,
    mut levels: ResMut<Levels>,
    mut grid: ResMut<Grid>,
    mut place: ResMut<Placement>,
    mut tuning: ResMut<Tuning>,
    mut run: ResMut<Run>,
    mut mode: ResMut<Mode>,
    mut tutorial: ResMut<Tutorial>,
    mut progress: ResMut<Progress>,
) {
    if !keys.just_pressed(KeyCode::PageDown) {
        return;
    }
    if let Some(path) = levels.next_game() {
        load_level(
            &path,
            &mut grid,
            &mut place,
            &mut tuning,
            &mut run,
            &mut mode,
            &mut tutorial,
            &mut progress,
        );
    }
}

/// After the victory detonation has consumed the level, load the next game
/// level — or, if that was the last one, leave the final VICTORY banner up.
#[allow(clippy::too_many_arguments)]
pub fn advance_detonation(
    mut detonation: ResMut<Detonation>,
    mut levels: ResMut<Levels>,
    mut grid: ResMut<Grid>,
    mut place: ResMut<Placement>,
    mut tuning: ResMut<Tuning>,
    mut run: ResMut<Run>,
    mut mode: ResMut<Mode>,
    mut tutorial: ResMut<Tutorial>,
    mut progress: ResMut<Progress>,
) {
    if *detonation != Detonation::Complete {
        return;
    }
    let Some(path) = levels.next_game_after() else {
        return; // last level: the VICTORY banner stays up
    };
    load_level(
        &path,
        &mut grid,
        &mut place,
        &mut tuning,
        &mut run,
        &mut mode,
        &mut tutorial,
        &mut progress,
    );
    *detonation = Detonation::Idle;
    info!("Victory — loaded next level {}", path.display());
}

/// `Y` save / `L` load.
pub fn debug_io(
    keys: Res<ButtonInput<KeyCode>>,
    debug: Res<Debug>,
    mut grid: ResMut<Grid>,
    mut place: ResMut<Placement>,
    mut tuning: ResMut<Tuning>,
    mut run: ResMut<Run>,
    mut mode: ResMut<Mode>,
) {
    if !debug.0 {
        return;
    }
    if keys.just_pressed(KeyCode::KeyY) {
        match fs::write(CONFIG_PATH, serialize(&grid, place.start, &tuning)) {
            Ok(()) => info!("Saved config to {CONFIG_PATH}"),
            Err(e) => warn!("Could not save {CONFIG_PATH}: {e}"),
        }
    }
    if keys.just_pressed(KeyCode::KeyL) {
        match fs::read_to_string(CONFIG_PATH)
            .ok()
            .and_then(|t| parse(&t, grid.image.clone()))
        {
            Some((loaded, loaded_place, loaded_tuning)) => {
                *grid = loaded;
                place.start = loaded_place;
                // Keep the player's speed preference across loads.
                let speed = tuning.speed;
                *tuning = loaded_tuning;
                tuning.speed = speed;
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
    let Some((grid, place, tuning)) = parse(&text, Handle::default()) else {
        eprintln!("could not parse {path}");
        return;
    };

    println!("{}", ascii(&grid, place));
    let Some(start) = place
        .filter(|c| grid.is_open(*c))
        .or_else(|| grid.find_start())
    else {
        println!("no start cell");
        return;
    };

    let mut run = Run::default();
    let mut ball = Ball::new(start, tuning.start_charge);
    crate::ball::start_run(&mut run, &grid);
    println!("start={start:?} charge={}", tuning.start_charge);

    for i in 0..max_steps {
        if run.outcome != Outcome::Running {
            break;
        }
        let from = ball.cell;
        crate::ball::step_once(&grid, &mut run, &mut ball);
        println!(
            "{i:4}: {from:?} -> {:?} dir={:?} charge={:.1}",
            ball.cell, ball.dir, ball.charge
        );
        println!("{}", ascii(&grid, Some(ball.cell)));
    }
    println!(
        "solved={} laps={} speed={:.2}",
        run.solved, run.laps, run.speed
    );
    println!("outcome: {:?}", run.outcome);
    println!("{}", ascii(&grid, Some(ball.cell)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips() {
        let mut grid = Grid::new(Handle::default());
        for y in 10..20 {
            grid.paint(IVec2::new(20, y), Cell::Solid);
        }
        let tuning = Tuning {
            start_charge: 4.0,
            manual: false,
            speed: 1.0,
        };
        let text = serialize(&grid, Some(IVec2::new(19, 20)), &tuning);
        // Cropped to the shape, not the whole 160x120 board.
        assert!(text.lines().count() < 20, "should be cropped:\n{text}");

        let (back, place, loaded_tuning) = parse(&text, Handle::default()).expect("parses");
        assert_eq!(place, Some(IVec2::new(19, 20)));
        assert_eq!(loaded_tuning.start_charge, 4.0);
        assert_eq!(back.get(IVec2::new(20, 15)), Some(Cell::Solid));
        assert!(back.is_open(IVec2::new(19, 15)));
    }

    /// The loader must keep the live texture handle, or `sync_image` writes
    /// into a placeholder texture.
    #[test]
    fn parse_uses_the_given_image_handle() {
        let text = "# eternal_game_02 config v2\nstart_charge 0\nplace none\norigin 0 0\ngrid 1 1\n#\n";
        let handle = Handle::<Image>::default();
        let (grid, _, _) = parse(text, handle.clone()).expect("parses");
        assert_eq!(grid.image.id(), handle.id());
    }

    fn levels(names: &[&str]) -> Levels {
        Levels {
            files: names.iter().map(PathBuf::from).collect(),
            current: -1,
        }
    }

    #[test]
    fn new_game_skips_the_tutorial() {
        let mut list = levels(&["levels/00_tutorial.txt", "levels/01.txt", "levels/02.txt"]);
        assert_eq!(list.first_game(), Some(PathBuf::from("levels/01.txt")));
        assert_eq!(list.current, 1);
    }

    #[test]
    fn tutorial_finds_the_tutorial_level() {
        let mut list = levels(&["levels/00_tutorial.txt", "levels/01.txt"]);
        assert_eq!(list.tutorial(), Some(PathBuf::from("levels/00_tutorial.txt")));
        assert_eq!(list.current, 0);
    }

    #[test]
    fn tab_skips_the_tutorial_and_wraps() {
        let mut list = levels(&["levels/00_tutorial.txt", "levels/01.txt", "levels/02.txt"]);
        list.current = 2; // sitting on the last level
        // Wrapping forward must not land on the tutorial.
        assert_eq!(list.next_game(), Some(PathBuf::from("levels/01.txt")));
    }

    #[test]
    fn next_game_after_stops_at_the_last_level() {
        let mut list = levels(&["levels/00_tutorial.txt", "levels/01.txt", "levels/02.txt"]);
        // From the tutorial, the next game level is 01.
        list.current = 0;
        assert_eq!(list.next_game_after(), Some(PathBuf::from("levels/01.txt")));
        // From the middle it finds 02, and then there is nothing after it.
        assert_eq!(list.next_game_after(), Some(PathBuf::from("levels/02.txt")));
        assert_eq!(list.next_game_after(), None);
        assert_eq!(list.current, 2);
    }

    #[test]
    fn continue_finds_a_level_by_file_name() {
        let mut list = levels(&["levels/00_tutorial.txt", "levels/01.txt", "levels/02.txt"]);
        assert_eq!(list.find("02.txt"), Some(PathBuf::from("levels/02.txt")));
        assert_eq!(list.current, 2);
        assert_eq!(list.find("missing.txt"), None);
    }
}
