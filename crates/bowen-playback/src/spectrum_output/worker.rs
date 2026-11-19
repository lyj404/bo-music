use super::{OutputSpectrumFrame, platform::Capture};
use crate::spectrum::BAND_COUNT;
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(16);
const RETRY: Duration = Duration::from_secs(2);
const STALE: Duration = Duration::from_millis(250);

type Latest = Arc<Mutex<(u64, Instant, OutputSpectrumFrame)>>;

pub(super) struct Worker {
    requests: mpsc::Sender<(u64, bool)>,
    latest: Latest,
    active: bool,
    generation: u64,
}

impl Worker {
    pub fn spawn() -> std::io::Result<Self> {
        let (requests, receiver) = mpsc::channel();
        let latest = Arc::new(Mutex::new((
            0,
            Instant::now(),
            OutputSpectrumFrame::Waiting,
        )));
        let output = Arc::clone(&latest);
        thread::Builder::new()
            .name("bowen-spectrum".into())
            .spawn(move || run(receiver, output))?;
        Ok(Self {
            requests,
            latest,
            active: false,
            generation: 0,
        })
    }

    pub fn frame(&mut self) -> OutputSpectrumFrame {
        if !self.active && !self.set_active(true) {
            return OutputSpectrumFrame::Unavailable;
        }
        let latest = self.latest.lock().unwrap();
        match *latest {
            (generation, _, _) if generation != self.generation => OutputSpectrumFrame::Waiting,
            (_, time, OutputSpectrumFrame::Levels(_)) if time.elapsed() > STALE => {
                OutputSpectrumFrame::Levels([0.0; BAND_COUNT])
            }
            (_, _, frame) => frame,
        }
    }

    pub fn stop(&mut self) {
        if self.active {
            self.set_active(false);
        }
    }

    fn set_active(&mut self, active: bool) -> bool {
        self.generation = self.generation.wrapping_add(1);
        self.active = self.requests.send((self.generation, active)).is_ok() && active;
        self.active
    }
}

fn run(requests: mpsc::Receiver<(u64, bool)>, latest: Latest) {
    let mut active = false;
    let mut generation = 0;
    let mut capture = None;
    let mut retry_at = Instant::now();
    loop {
        let next = if active {
            requests.recv_timeout(POLL)
        } else {
            requests
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match next {
            Ok(state) => {
                let (next_generation, requested) = requests.try_iter().last().unwrap_or(state);
                generation = next_generation;
                active = requested;
                capture = None;
                retry_at = Instant::now();
                if !active {
                    continue;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let publish = |frame| *latest.lock().unwrap() = (generation, Instant::now(), frame);
        if capture.is_none() && Instant::now() >= retry_at {
            capture = Capture::open().ok();
            if capture.is_some() {
                publish(OutputSpectrumFrame::Levels([0.0; BAND_COUNT]));
            } else {
                publish(OutputSpectrumFrame::Unavailable);
                retry_at = Instant::now() + RETRY;
            }
        }
        if let Some(endpoint) = capture.as_mut() {
            match endpoint.poll() {
                Ok(Some(levels)) => publish(OutputSpectrumFrame::Levels(levels)),
                Ok(None) => {}
                Err(_) => {
                    capture = None;
                    retry_at = Instant::now() + RETRY;
                    publish(OutputSpectrumFrame::Unavailable);
                }
            }
        }
    }
}
