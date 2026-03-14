//! Terminal display for pkgcheck.
//!
//! Manages a compact status area (≤ 25 % of terminal height) that is
//! redrawn in-place using ANSI cursor movement.  A background thread
//! toggles the status indicator every 500 ms to produce a blinking effect
//! while packages are being checked.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    queue, style, terminal,
};
use tabled::{settings::Style, Table};

use crate::types::{EcosystemSummary, OverallStatus, PackageInfo};

// ---------------------------------------------------------------------------
// Shared state between main thread and blink thread
// ---------------------------------------------------------------------------

/// Internal state that the display thread reads on every tick.
struct DisplayState {
    /// One entry per ecosystem that has finished checking.
    summaries: Vec<EcosystemSummary>,
    /// The ecosystem currently being checked (shown as "checking…").
    current_ecosystem: Option<String>,
    /// Overall health indicator.
    status: OverallStatus,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Live terminal display with a blinking status indicator.
///
/// # Usage
/// ```ignore
/// let mut display = Display::new();
/// display.start_ecosystem("Node.js");
/// // ... do work ...
/// display.finish_ecosystem(summary);
/// display.set_final_status(OverallStatus::AllGood);
/// display.finish();
/// display.print_table(&packages);
/// ```
pub struct Display {
    state: Arc<Mutex<DisplayState>>,
    /// Signals the blink thread to stop.
    is_running: Arc<AtomicBool>,
    blink_handle: Option<thread::JoinHandle<()>>,
    /// Maximum number of lines the status area may occupy.
    max_lines: usize,
    /// How many lines the last render actually wrote (shared with blink thread).
    lines_rendered: Arc<AtomicUsize>,
}

impl Display {
    /// Create a new display and start the background refresh thread.
    pub fn new() -> Self {
        // Determine how many lines we're allowed to use (25 % of terminal).
        let (_, rows) = terminal::size().unwrap_or((80, 24));
        let max_lines = ((rows as usize) / 4).max(4);

        let state = Arc::new(Mutex::new(DisplayState {
            summaries: Vec::new(),
            current_ecosystem: None,
            status: OverallStatus::Processing,
        }));

        let is_running = Arc::new(AtomicBool::new(true));
        let lines_rendered = Arc::new(AtomicUsize::new(0));

        // Spawn a background thread that redraws the status area every 500 ms,
        // alternating the indicator dot to create a blinking effect.
        let state_ref = Arc::clone(&state);
        let running_ref = Arc::clone(&is_running);
        let lr_ref = Arc::clone(&lines_rendered);
        let ml = max_lines;
        let handle = thread::spawn(move || {
            let mut blink_on = true;
            while running_ref.load(Ordering::Relaxed) {
                render_status(&state_ref, ml, blink_on, &lr_ref);
                blink_on = !blink_on;
                thread::sleep(Duration::from_millis(500));
            }
        });

        Self {
            state,
            is_running,
            blink_handle: Some(handle),
            max_lines,
            lines_rendered,
        }
    }

    /// Signal that we are starting to check a specific ecosystem.
    pub fn start_ecosystem(&self, name: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.current_ecosystem = Some(name.to_string());
        }
    }

    /// Record the completed results for one ecosystem.
    pub fn finish_ecosystem(&self, summary: EcosystemSummary) {
        if let Ok(mut s) = self.state.lock() {
            s.current_ecosystem = None;
            s.summaries.push(summary);
        }
    }

    /// Transition to the final (non-processing) status.
    pub fn set_final_status(&self, status: OverallStatus) {
        if let Ok(mut s) = self.state.lock() {
            s.status = status;
        }
    }

    /// Stop the background thread and do one final render (solid indicator).
    pub fn finish(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
        if let Some(h) = self.blink_handle.take() {
            let _ = h.join();
        }
        // Final render with the dot always visible (no blink).
        render_status(&self.state, self.max_lines, true, &self.lines_rendered);
    }

    /// Print the package summary table below the status area.
    ///
    /// If the table fits in half the terminal height it is printed directly.
    /// Otherwise an interactive scrollable view is shown (arrow keys / j-k to
    /// scroll, q / Esc to exit).
    pub fn print_table(&self, packages: &[PackageInfo]) {
        if packages.is_empty() {
            println!("\n  No packages to display.");
            return;
        }

        let rows: Vec<_> = packages.iter().map(|p| p.to_row()).collect();
        let mut table = Table::new(rows);
        table.with(Style::rounded());
        let rendered = format!("\n{}", table);
        let lines: Vec<&str> = rendered.lines().collect();

        let (_, term_rows) = terminal::size().unwrap_or((80, 24));
        let max_visible = (term_rows as usize) / 2;

        if lines.len() <= max_visible {
            println!("{}", rendered);
        } else {
            interactive_scroll(&lines, max_visible);
        }
    }
}

/// Ensure the blink thread is cleaned up if the Display is dropped early
/// (e.g. on an error path).
impl Drop for Display {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
        if let Some(h) = self.blink_handle.take() {
            let _ = h.join();
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Redraw the entire status area.  Called by the background thread on each
/// tick and once more by `Display::finish`.
///
/// Uses `prev_lines` to know how far to move the cursor up from the last
/// render, so the status area only occupies exactly the lines it needs.
fn render_status(
    state: &Arc<Mutex<DisplayState>>,
    max_lines: usize,
    blink_on: bool,
    prev_lines: &AtomicUsize,
) {
    let Ok(s) = state.lock() else {
        return;
    };
    let mut stdout = io::stdout();

    // ── Move cursor back to the top of the previous render ───────────────
    let prev = prev_lines.load(Ordering::Relaxed);
    if prev > 0 {
        let _ = queue!(stdout, cursor::MoveUp(prev as u16));
    }

    let mut lines_written: usize = 0;

    // ── Line 1: status header with coloured indicator ────────────────────
    let (dot, color) = indicator_style(&s.status, blink_on);
    let text = status_text(&s);

    let _ = queue!(
        stdout,
        terminal::Clear(terminal::ClearType::CurrentLine),
        style::Print("  Status:  "),
        style::SetForegroundColor(color),
        style::Print(dot),
        style::ResetColor,
        style::Print(format!(" {}\n", text)),
    );
    lines_written += 1;

    // ── One line per completed ecosystem ─────────────────────────────────
    for summary in &s.summaries {
        if lines_written >= max_lines {
            break;
        }
        let _ = queue!(
            stdout,
            terminal::Clear(terminal::ClearType::CurrentLine),
            style::Print(format!("  {}\n", format_summary(summary))),
        );
        lines_written += 1;
    }

    // ── Currently-checking ecosystem (if any) ────────────────────────────
    if lines_written < max_lines {
        if let Some(ref name) = s.current_ecosystem {
            if s.status == OverallStatus::Processing {
                let _ = queue!(
                    stdout,
                    terminal::Clear(terminal::ClearType::CurrentLine),
                    style::Print(format!("  {:<10} checking…\n", format!("{}:", name))),
                );
                lines_written += 1;
            }
        }
    }

    // ── Clear any leftover lines from a previous (taller) render ─────────
    while lines_written < prev {
        let _ = queue!(
            stdout,
            terminal::Clear(terminal::ClearType::CurrentLine),
            style::Print("\n"),
        );
        lines_written += 1;
    }

    // Remember how many lines we wrote so the next render can MoveUp correctly.
    prev_lines.store(lines_written, Ordering::Relaxed);

    let _ = stdout.flush();
}

/// Choose the indicator character and colour based on overall status.
fn indicator_style(status: &OverallStatus, blink_on: bool) -> (&'static str, style::Color) {
    match status {
        OverallStatus::Processing => {
            if blink_on {
                ("●", style::Color::Green)
            } else {
                (" ", style::Color::Green) // invisible during off-phase
            }
        }
        OverallStatus::AllGood => ("●", style::Color::Green),
        OverallStatus::Partial => ("●", style::Color::Rgb { r: 255, g: 165, b: 0 }), // orange
        OverallStatus::NoneInstalled => ("●", style::Color::Red),
    }
}

/// Human-readable status message for the header line.
fn status_text(state: &DisplayState) -> String {
    match &state.status {
        OverallStatus::Processing => match &state.current_ecosystem {
            Some(name) => format!("checking {} packages…", name),
            None => "checking for packages…".to_string(),
        },
        OverallStatus::AllGood => "all packages installed".to_string(),
        OverallStatus::Partial => "some packages missing or outdated".to_string(),
        OverallStatus::NoneInstalled => "no packages installed".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Interactive scrollable table
// ---------------------------------------------------------------------------

/// Display `lines` in a scrollable viewport of `height` rows.
/// The user can scroll with ↑/↓, j/k, PgUp/PgDn, and exit with q or Esc.
fn interactive_scroll(lines: &[&str], height: usize) {
    // We reserve 1 row for the hint bar at the bottom.
    let view_rows = height.saturating_sub(1).max(1);
    let max_offset = lines.len().saturating_sub(view_rows);
    let mut offset: usize = 0;
    let mut stdout = io::stdout();

    // Draw the initial frame *before* entering raw mode so the space exists.
    draw_scroll_frame(&mut stdout, lines, offset, view_rows, max_offset);

    // Enter raw mode to capture individual key presses.
    let _ = terminal::enable_raw_mode();

    loop {
        if let Ok(evt) = event::read() {
            match evt {
                Event::Key(key) => {
                    match key.code {
                        // Exit
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            break
                        }
                        // Scroll down
                        KeyCode::Down | KeyCode::Char('j') => {
                            if offset < max_offset {
                                offset += 1;
                            }
                        }
                        // Scroll up
                        KeyCode::Up | KeyCode::Char('k') => {
                            offset = offset.saturating_sub(1);
                        }
                        // Page down
                        KeyCode::PageDown | KeyCode::Char(' ') => {
                            offset = (offset + view_rows).min(max_offset);
                        }
                        // Page up
                        KeyCode::PageUp => {
                            offset = offset.saturating_sub(view_rows);
                        }
                        // Home / End
                        KeyCode::Home => offset = 0,
                        KeyCode::End => offset = max_offset,
                        _ => {}
                    }
                    // Redraw: move cursor up to overwrite the previous frame.
                    let total_drawn = view_rows + 1; // view + hint bar
                    let _ = queue!(stdout, cursor::MoveUp(total_drawn as u16));
                    draw_scroll_frame(&mut stdout, lines, offset, view_rows, max_offset);
                }
                _ => {}
            }
        }
    }

    let _ = terminal::disable_raw_mode();
}

/// Render one frame of the scrollable viewport.
fn draw_scroll_frame(
    stdout: &mut io::Stdout,
    lines: &[&str],
    offset: usize,
    view_rows: usize,
    max_offset: usize,
) {
    let end = (offset + view_rows).min(lines.len());

    for i in offset..end {
        let _ = queue!(
            stdout,
            cursor::MoveToColumn(0),
            terminal::Clear(terminal::ClearType::CurrentLine),
            style::Print(lines[i]),
            style::Print("\r\n"),
        );
    }
    // Pad any remaining rows if near the end.
    for _ in (end - offset)..view_rows {
        let _ = queue!(
            stdout,
            cursor::MoveToColumn(0),
            terminal::Clear(terminal::ClearType::CurrentLine),
            style::Print("\r\n"),
        );
    }

    // Hint bar
    let position = format!("[{}-{}/{}]", offset + 1, end, lines.len());
    let can_up = offset > 0;
    let can_down = offset < max_offset;
    let arrows = match (can_up, can_down) {
        (true, true) => "↑↓ scroll",
        (true, false) => "↑ scroll",
        (false, true) => "↓ scroll",
        (false, false) => "",
    };
    let _ = queue!(
        stdout,
        cursor::MoveToColumn(0),
        terminal::Clear(terminal::ClearType::CurrentLine),
        style::SetForegroundColor(style::Color::DarkGrey),
        style::Print(format!("  {} {}  q to exit", position, arrows)),
        style::ResetColor,
        style::Print("\r\n"),
    );
    let _ = stdout.flush();
}

/// Format one ecosystem summary line, e.g. `"Node.js:   5/8 installed, 2 missing"`.
fn format_summary(summary: &EcosystemSummary) -> String {
    let mut parts = vec![format!("{}/{} installed", summary.installed, summary.total)];
    if summary.outdated > 0 {
        parts.push(format!("{} outdated", summary.outdated));
    }
    if summary.missing > 0 {
        parts.push(format!("{} missing", summary.missing));
    }
    format!("{:<10} {}", format!("{}:", summary.name), parts.join(", "))
}
