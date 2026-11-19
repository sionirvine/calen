//! Development aids for rendering work (debug builds, or with `CALEN_DEV` set):
//!
//! - `--demo bench`: switch views and scroll the week grid on a fixed script while
//!   timing the gap between frames, then log p50 / p95 / max per phase and quit.
//! - `--demo shot:<day|week|month>:<path.png>`: show that view, save the main window
//!   to a PNG (drawn by the active renderer), and quit.
//! - `CALEN_PROFILE=1`: log `update`, `view` and canvas painting that take over 8 ms.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::Task;
use iced::widget::operation::{self, AbsoluteOffset};

use crate::app::{App, Message, WEEK_SCROLL};
use crate::settings::ViewKind;

/// Frames to wait after each step, so the step's frame and the settled frames
/// after it are both measured.
const FRAMES_PER_STEP: u32 = 3;

enum Step {
    Phase(&'static str),
    View(ViewKind),
    Scroll(f32),
    /// Do nothing for one step (frames where nothing changed).
    Wait,
}

pub struct Bench {
    steps: VecDeque<Step>,
    phase: &'static str,
    last: Option<Instant>,
    waited: u32,
    started: Instant,
    samples: BTreeMap<&'static str, Vec<f32>>,
}

impl Bench {
    fn new() -> Self {
        let mut steps = VecDeque::new();
        steps.push_back(Step::View(ViewKind::Week));
        steps.push_back(Step::Phase("idle"));
        for _ in 0..10 {
            steps.push_back(Step::Wait);
        }
        steps.push_back(Step::Phase("switch views"));
        for _ in 0..5 {
            for v in [ViewKind::Month, ViewKind::Day, ViewKind::Week] {
                steps.push_back(Step::View(v));
            }
        }
        steps.push_back(Step::Phase("scroll week"));
        for i in 0..40 {
            steps.push_back(Step::Scroll(if (i / 10) % 2 == 0 { 60.0 } else { -60.0 }));
        }
        Self { steps, phase: "warm-up", last: None, waited: 0, started: Instant::now(), samples: BTreeMap::new() }
    }
}

/// Runs maximized unless `CALEN_BENCH_WINDOWED` is set, since a full-screen
/// window is the slow case for the CPU renderer.
pub fn start_bench(app: &mut App) -> Task<Message> {
    log::info!("bench: started (renderer {})", std::env::var("ICED_BACKEND").unwrap_or_default());
    app.bench = Some(Bench::new());
    match app.main_window {
        Some(id) if std::env::var_os("CALEN_BENCH_WINDOWED").is_none() => iced::window::maximize(id, true),
        _ => Task::none(),
    }
}

/// One frame went by while the bench runs.
pub fn frame(app: &mut App, at: Instant) -> Task<Message> {
    let Some(b) = app.bench.as_mut() else { return Task::none() };
    if let Some(last) = b.last {
        let ms = (at - last).as_secs_f32() * 1000.0;
        if std::env::var_os("CALEN_BENCH_TRACE").is_some() {
            log::info!("bench: frame {} {ms:.1} ms", b.phase);
        }
        b.samples.entry(b.phase).or_default().push(ms);
    }
    b.last = Some(at);
    b.waited += 1;
    if b.waited < FRAMES_PER_STEP {
        return Task::none();
    }
    b.waited = 0;
    loop {
        match b.steps.pop_front() {
            Some(Step::Phase(p)) => b.phase = p,
            Some(Step::View(v)) => return app.update(Message::SetView(v)),
            Some(Step::Scroll(dy)) => return operation::scroll_by(WEEK_SCROLL, AbsoluteOffset { x: 0.0, y: dy }),
            Some(Step::Wait) => return Task::none(),
            None => {
                report(b);
                return iced::exit();
            }
        }
    }
}

fn report(b: &Bench) {
    for (phase, ms) in &b.samples {
        if *phase == "warm-up" {
            continue;
        }
        let mut s = ms.clone();
        s.sort_by(f32::total_cmp);
        let pct = |p: f32| s[((s.len() - 1) as f32 * p).round() as usize];
        log::info!(
            "bench: {phase}: {} frames, p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms",
            s.len(),
            pct(0.5),
            pct(0.95),
            s[s.len() - 1]
        );
    }
    log::info!("bench: done in {:.1} s", b.started.elapsed().as_secs_f32());
}

/// `shot:<view>:<path>`
pub fn shot(app: &mut App, spec: &str) -> Task<Message> {
    let Some((view, path)) = spec.split_once(':') else {
        log::warn!("shot: expected shot:<day|week|month>:<path>");
        return Task::none();
    };
    let view = match view {
        "day" => ViewKind::Day,
        "month" => ViewKind::Month,
        _ => ViewKind::Week,
    };
    let Some(id) = app.main_window else {
        log::warn!("shot: the main window isn't open");
        return Task::none();
    };
    let path = PathBuf::from(path);
    let set = app.update(Message::SetView(view));
    // Let the view settle (scroll to the morning, first paints) before capturing.
    let capture = Task::perform(tokio::time::sleep(Duration::from_millis(1500)), |_| ())
        .then(move |_| iced::window::screenshot(id))
        .then(move |s| {
            match image::RgbaImage::from_raw(s.size.width, s.size.height, s.rgba.to_vec()) {
                Some(img) => match img.save(&path) {
                    Ok(()) => log::info!("shot: saved {}", path.display()),
                    Err(e) => log::warn!("shot: {e}"),
                },
                None => log::warn!("shot: bad screenshot buffer"),
            }
            iced::exit()
        });
    Task::batch([set, capture])
}

fn profiling() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CALEN_PROFILE").is_some())
}

/// Run `f`, logging its duration when profiling and it took over 8 ms.
pub fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if !profiling() {
        return f();
    }
    let t = Instant::now();
    let out = f();
    let ms = t.elapsed().as_secs_f32() * 1000.0;
    if ms > 8.0 {
        log::info!("profile: {label} {ms:.1} ms");
    }
    out
}
