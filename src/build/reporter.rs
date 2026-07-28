use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar};

use anyhow::Result;

use crate::build::vars::BuildOverrides;
use crate::core::ctx::{Ctx, Runner};
use crate::core::style;
use crate::manifest::Build;
use crate::progress::{fmt_secs, StepFinished, Tracker};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Fancy,
    Stream,
    Inherit,
}

pub(crate) fn output_mode(ctx: &Ctx, ov: &BuildOverrides, count: usize) -> Mode {
    match ov.progress.as_deref() {
        Some("plain") => Mode::Stream,
        Some("tty") if count == 1 || ov.sequential => Mode::Inherit,
        _ => {
            if ctx.color {
                Mode::Fancy
            } else {
                Mode::Stream
            }
        }
    }
}

pub(crate) struct Reporter<'a> {
    pub(crate) ctx: &'a Ctx,
    pub(crate) name: String,
    pub(crate) width: usize,
    pub(crate) mode: Mode,
    pub(crate) multi: Option<&'a MultiProgress>,
    pub(crate) bar: Option<ProgressBar>,
    pub(crate) tracker: Arc<Mutex<Tracker>>,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(
        ctx: &'a Ctx,
        name: &str,
        mode: Mode,
        multi: Option<&'a MultiProgress>,
        bar: Option<ProgressBar>,
    ) -> Self {
        Reporter {
            ctx,
            name: name.to_string(),
            width: name.len(),
            mode,
            multi,
            bar,
            tracker: Arc::new(Mutex::new(Tracker::new())),
        }
    }

    pub(crate) fn padded_to(mut self, width: usize) -> Self {
        self.width = width;
        self
    }

    pub(crate) fn println(&self, line: String) {
        match self.multi {
            Some(multi) => {
                multi.println(line).ok();
            }
            None => self.ctx.log(&line),
        }
    }

    pub(crate) fn echo(&self, runner: &Runner<'_>) {
        if self.ctx.quiet {
            return;
        }
        self.println(style::dim_err(&format!("$ {}", runner.display())));
    }

    pub(crate) fn label(&self) -> String {
        if self.name.is_empty() {
            String::new()
        } else {
            format!("{:<w$} ", format!("[{}]", self.name), w = self.width + 2)
        }
    }

    pub(crate) fn info(&self, msg: &str) {
        self.println(format!("{} {}{msg}", style::blue("==>"), self.label()));
    }

    pub(crate) fn ok(&self, msg: &str) {
        self.println(format!("{} {}{msg}", style::green("ok"), self.label()));
    }

    pub(crate) fn dim(&self, msg: &str) {
        self.println(style::dim(&format!("{}{msg}", self.label())));
    }

    pub(crate) fn phase(&self, phase: &str) {
        if let Ok(mut t) = self.tracker.lock() {
            t.set_phase(phase);
        }
    }

    pub(crate) fn step_line(&self, fin: &StepFinished) {
        let pos = fin.position();
        let line = if let Some(err) = &fin.error {
            format!(
                "{} {}{pos}{}  {}",
                style::red("x"),
                self.label(),
                fin.label,
                err
            )
        } else if fin.cached {
            style::dim(&format!("- {}{pos}{}  cached", self.label(), fin.label))
        } else {
            format!(
                "{} {}{pos}{}  {}",
                style::green("+"),
                self.label(),
                fin.label,
                fin.secs.map(fmt_secs).unwrap_or_default()
            )
        };
        self.println(line);
    }

    pub(crate) fn observe(&self, line: &str) {
        let fin = self.tracker.lock().ok().and_then(|mut t| t.observe(line));
        match self.mode {
            Mode::Fancy => {
                if let Some(fin) = &fin {
                    if fin.index.is_some() || fin.error.is_some() {
                        self.step_line(fin);
                    }
                }
            }
            Mode::Stream | Mode::Inherit => {
                self.println(format!(
                    "{} {line}",
                    style::dim(&format!("{:<w$} |", self.name, w = self.width))
                ));
            }
        }
    }

    pub(crate) fn dump_tail(&self, lines: usize) {
        let Ok(t) = self.tracker.lock() else {
            return;
        };
        let tail = t.tail();
        let start = tail.len().saturating_sub(lines);
        if start >= tail.len() {
            return;
        }
        self.println(style::dim(&format!(
            "{}last {} output lines:",
            self.label(),
            tail.len() - start
        )));
        for l in &tail[start..] {
            self.println(style::dim(&format!("{}{l}", self.label())));
        }
    }

    pub(crate) fn run(&self, runner: Runner<'_>) -> Result<bool> {
        let runner = if self.multi.is_some() {
            self.echo(&runner);
            runner.silent()
        } else {
            runner
        };

        if self.mode == Mode::Inherit {
            return Ok(runner.status()?.success());
        }

        let mut child = runner.spawn_piped()?;
        let (tx, rx) = mpsc::channel::<String>();
        let mut readers = Vec::new();
        if let Some(o) = child.stdout.take() {
            let tx = tx.clone();
            readers.push(thread::spawn(move || {
                for l in BufReader::new(o).lines().map_while(Result::ok) {
                    tx.send(l).ok();
                }
            }));
        }
        if let Some(e) = child.stderr.take() {
            let tx = tx.clone();
            readers.push(thread::spawn(move || {
                for l in BufReader::new(e).lines().map_while(Result::ok) {
                    tx.send(l).ok();
                }
            }));
        }
        drop(tx);

        for line in rx {
            self.observe(&line);
        }
        for r in readers {
            r.join().ok();
        }
        Ok(child.wait()?.success())
    }
}

pub(crate) fn name_width(entries: &[Build]) -> usize {
    entries.iter().map(|b| b.name.len()).max().unwrap_or(0)
}

pub(crate) fn spawn_ticker(
    reporters: &[&Reporter<'_>],
) -> (Arc<AtomicBool>, thread::JoinHandle<()>) {
    let stop = Arc::new(AtomicBool::new(false));
    let pairs: Vec<(Arc<Mutex<Tracker>>, ProgressBar)> = reporters
        .iter()
        .filter_map(|r| r.bar.clone().map(|b| (r.tracker.clone(), b)))
        .collect();
    let stop2 = stop.clone();
    let handle = thread::spawn(move || {
        while !stop2.load(Ordering::Relaxed) {
            for (tracker, bar) in &pairs {
                if bar.is_finished() {
                    continue;
                }
                if let Ok(t) = tracker.lock() {
                    bar.set_message(t.status_line());
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    (stop, handle)
}
