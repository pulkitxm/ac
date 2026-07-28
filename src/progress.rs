use std::collections::{HashMap, VecDeque};
use std::time::Instant;

#[derive(Debug, PartialEq)]
pub enum Event<'a> {
    Started { id: u32, name: &'a str },
    Log { id: u32 },
    Done { id: u32, secs: Option<f32> },
    Cached { id: u32 },
    Error { id: u32, text: &'a str },
    Canceled { id: u32 },
    Other,
}

pub fn parse_line(line: &str) -> Event<'_> {
    let Some(rest) = line.strip_prefix('#') else {
        return Event::Other;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Event::Other;
    }
    let Ok(id) = digits.parse::<u32>() else {
        return Event::Other;
    };
    let tail = rest[digits.len()..].trim_start();
    if tail.is_empty() {
        return Event::Log { id };
    }
    if let Some(t) = tail.strip_prefix("DONE") {
        let secs = t
            .trim()
            .strip_suffix('s')
            .and_then(|n| n.parse::<f32>().ok());
        return Event::Done { id, secs };
    }
    if tail == "CACHED" {
        return Event::Cached { id };
    }
    if tail == "CANCELED" {
        return Event::Canceled { id };
    }
    if let Some(t) = tail.strip_prefix("ERROR") {
        return Event::Error {
            id,
            text: t.trim_start_matches(": ").trim(),
        };
    }
    let head = tail.split_whitespace().next().unwrap_or("");
    if head.parse::<f32>().is_ok() {
        return Event::Log { id };
    }
    Event::Started { id, name: tail }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepMarker {
    pub stage: String,
    pub index: u32,
    pub total: u32,
}

pub fn step_marker(name: &str) -> Option<StepMarker> {
    let inner = name.strip_prefix('[')?.split(']').next()?;
    let (stage, frac) = inner.rsplit_once(' ')?;
    let (a, b) = frac.split_once('/')?;
    let index = a.parse::<u32>().ok()?;
    let total = b.parse::<u32>().ok()?;
    Some(StepMarker {
        stage: stage.to_string(),
        index,
        total,
    })
}

pub fn step_label(name: &str) -> String {
    match name.split_once("] ") {
        Some((_, rest)) if name.starts_with('[') => rest.to_string(),
        _ => name.to_string(),
    }
}

#[derive(Debug, Clone)]
struct Current {
    id: u32,
    label: String,
    index: Option<u32>,
    total: Option<u32>,
}

pub struct Tracker {
    pub phase: String,
    started: Instant,
    step_started: Instant,
    current: Option<Current>,
    names: HashMap<u32, String>,
    pub steps_total: u32,
    pub steps_done: u32,
    pub steps_cached: u32,
    tail: VecDeque<String>,
    tail_cap: usize,
}

pub struct StepFinished {
    pub label: String,
    pub index: Option<u32>,
    pub total: Option<u32>,
    pub cached: bool,
    pub secs: Option<f32>,
    pub error: Option<String>,
}

impl StepFinished {
    pub fn position(&self) -> String {
        match (self.index, self.total) {
            (Some(i), Some(t)) => format!("[{i}/{t}] "),
            _ => String::new(),
        }
    }
}

impl Tracker {
    pub fn new() -> Self {
        let now = Instant::now();
        Tracker {
            phase: "starting".to_string(),
            started: now,
            step_started: now,
            current: None,
            names: HashMap::new(),
            steps_total: 0,
            steps_done: 0,
            steps_cached: 0,
            tail: VecDeque::new(),
            tail_cap: 200,
        }
    }

    pub fn set_phase(&mut self, phase: &str) {
        self.phase = phase.to_string();
        self.current = None;
        self.step_started = Instant::now();
    }

    pub fn observe(&mut self, line: &str) -> Option<StepFinished> {
        self.push_tail(line);
        match parse_line(line) {
            Event::Started { id, name } => {
                self.names.insert(id, name.to_string());
                let marker = step_marker(name);
                if let Some(m) = &marker {
                    if m.total > self.steps_total {
                        self.steps_total = m.total;
                    }
                }
                self.current = Some(Current {
                    id,
                    label: step_label(name),
                    index: marker.as_ref().map(|m| m.index),
                    total: marker.as_ref().map(|m| m.total),
                });
                self.step_started = Instant::now();
                None
            }
            Event::Done { id, secs } => self.finish(id, false, secs, None),
            Event::Cached { id } => self.finish(id, true, None, None),
            Event::Error { id, text } => self.finish(id, false, None, Some(text.to_string())),
            Event::Canceled { .. } | Event::Log { .. } | Event::Other => None,
        }
    }

    fn finish(
        &mut self,
        id: u32,
        cached: bool,
        secs: Option<f32>,
        error: Option<String>,
    ) -> Option<StepFinished> {
        let name = self.names.get(&id)?.clone();
        let marker = step_marker(&name);
        if let Some(cur) = &self.current {
            if cur.id == id {
                self.current = None;
            }
        }
        if marker.is_some() || error.is_some() {
            if error.is_none() {
                self.steps_done += 1;
                if cached {
                    self.steps_cached += 1;
                }
            }
            return Some(StepFinished {
                label: step_label(&name),
                index: marker.as_ref().map(|m| m.index),
                total: marker.as_ref().map(|m| m.total),
                cached,
                secs,
                error,
            });
        }
        None
    }

    fn push_tail(&mut self, line: &str) {
        if self.tail.len() >= self.tail_cap {
            self.tail.pop_front();
        }
        self.tail.push_back(line.to_string());
    }

    pub fn tail(&self) -> Vec<String> {
        self.tail.iter().cloned().collect()
    }

    pub fn total_elapsed(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    pub fn status_line(&self) -> String {
        let total = fmt_secs(self.total_elapsed());
        match &self.current {
            Some(cur) => {
                let step = fmt_secs(self.step_started.elapsed().as_secs_f32());
                let pos = match (cur.index, cur.total) {
                    (Some(i), Some(t)) => format!("[{i}/{t}] "),
                    _ => String::new(),
                };
                format!("{}{}  {step} | total {total}", pos, cur.label)
            }
            None => format!("{}  {total}", self.phase),
        }
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn fmt_secs(secs: f32) -> String {
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        let m = (secs / 60.0).floor() as u64;
        let s = (secs - (m as f32) * 60.0).floor() as u64;
        format!("{m}m{s:02}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_started_done_cached_error() {
        assert_eq!(
            parse_line("#5 [linux/arm64 1/3] RUN echo hello > /hello.txt"),
            Event::Started {
                id: 5,
                name: "[linux/arm64 1/3] RUN echo hello > /hello.txt"
            }
        );
        assert_eq!(
            parse_line("#5 DONE 2.1s"),
            Event::Done {
                id: 5,
                secs: Some(2.1)
            }
        );
        assert_eq!(parse_line("#4 CACHED"), Event::Cached { id: 4 });
        assert_eq!(parse_line("#4 CANCELED"), Event::Canceled { id: 4 });
        assert_eq!(parse_line("#5 0.047 web-one"), Event::Log { id: 5 });
        assert_eq!(parse_line("#5 12 lines of output"), Event::Log { id: 5 });
        assert_eq!(
            parse_line("#6 ERROR: process did not complete"),
            Event::Error {
                id: 6,
                text: "process did not complete"
            }
        );
        assert_eq!(parse_line("random text"), Event::Other);
        assert_eq!(parse_line("#abc no digits"), Event::Other);
    }

    #[test]
    fn sub_output_lines_are_logs_not_steps() {
        match parse_line("#7 exporting layers 0.0s done") {
            Event::Started { id, name } => {
                assert_eq!(id, 7);
                assert_eq!(name, "exporting layers 0.0s done");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn markers_extract_stage_and_position() {
        let m = step_marker("[linux/arm64 2/3] RUN sleep 2").unwrap();
        assert_eq!(m.stage, "linux/arm64");
        assert_eq!(m.index, 2);
        assert_eq!(m.total, 3);

        let m = step_marker("[deps 12/14] COPY package.json .").unwrap();
        assert_eq!(m.stage, "deps");
        assert_eq!(m.index, 12);
        assert_eq!(m.total, 14);

        assert!(step_marker("[internal] load build definition").is_none());
        assert!(step_marker("exporting to oci image format").is_none());
        assert!(step_marker("[resolver] fetching image...").is_none());
    }

    #[test]
    fn labels_drop_the_bracketed_prefix() {
        assert_eq!(step_label("[deps 1/4] RUN npm ci"), "RUN npm ci");
        assert_eq!(
            step_label("exporting to oci image format"),
            "exporting to oci image format"
        );
    }

    #[test]
    fn tracker_counts_steps_and_reports_finishes() {
        let mut t = Tracker::new();
        assert!(t
            .observe("#2 [internal] load build definition from Dockerfile")
            .is_none());
        assert!(t.observe("#2 DONE 0.0s").is_none());
        assert!(t.observe("#5 [linux/arm64 1/3] RUN echo hello").is_none());
        assert!(t.observe("#5 0.047 hello").is_none());
        let fin = t.observe("#5 DONE 0.1s").unwrap();
        assert_eq!(fin.index, Some(1));
        assert_eq!(fin.total, Some(3));
        assert_eq!(fin.position(), "[1/3] ");
        assert!(!fin.cached);
        assert_eq!(t.steps_done, 1);
        assert_eq!(t.steps_total, 3);

        assert!(t.observe("#6 [linux/arm64 2/3] RUN sleep 2").is_none());
        let fin = t.observe("#6 CACHED").unwrap();
        assert!(fin.cached);
        assert_eq!(t.steps_cached, 1);
    }

    #[test]
    fn tracker_status_line_shows_position_and_label() {
        let mut t = Tracker::new();
        t.observe("#5 [web 2/9] RUN pnpm install");
        let s = t.status_line();
        assert!(s.contains("[2/9]"), "{s}");
        assert!(s.contains("RUN pnpm install"), "{s}");
        assert!(s.contains("total"), "{s}");
    }

    #[test]
    fn tracker_keeps_a_bounded_tail() {
        let mut t = Tracker::new();
        for i in 0..500 {
            t.observe(&format!("#9 line {i}"));
        }
        let tail = t.tail();
        assert_eq!(tail.len(), 200);
        assert_eq!(tail.last().unwrap(), "#9 line 499");
    }

    #[test]
    fn seconds_format_is_compact() {
        assert_eq!(fmt_secs(2.13), "2.1s");
        assert_eq!(fmt_secs(63.4), "1m03s");
        assert_eq!(fmt_secs(0.0), "0.0s");
    }
}
