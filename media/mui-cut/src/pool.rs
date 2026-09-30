//! Frame-parallel CPU export: a few Vello CPU renderers on their own
//! threads, each drawing whole output frames (subframes and shutter
//! included), handed back in order. Several single-threaded frames at once
//! keep every core busy where one multithreaded frame waits on its slowest
//! strip.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::render::Assets;
use crate::{Frame, Renderer};

type Done = (usize, Result<Vec<u8>, String>);

/// A pool of CPU renderers. [`CpuPool::push`] queues an output frame and,
/// once enough are in flight, returns the oldest; [`CpuPool::finish`] the
/// rest, in the order they were pushed.
pub struct CpuPool {
    jobs: Option<mpsc::Sender<(usize, Vec<Frame>)>>,
    done: mpsc::Receiver<Done>,
    workers: Vec<thread::JoinHandle<()>>,
    parked: BTreeMap<usize, Result<Vec<u8>, String>>,
    sent: usize,
    next: usize,
    window: usize,
}

/// One output frame: its subframes averaged in linear light (mui-reel's
/// shutter), or the one frame as drawn.
pub fn shutter(r: &mut Renderer, acc: &mut Vec<f32>, subs: &[Frame]) -> Result<Vec<u8>, String> {
    if let [f] = subs {
        return Ok(r.draw(f)?.0);
    }
    let (w, h) = r.size();
    acc.clear();
    acc.resize(usize::from(w) * usize::from(h) * 4, 0.);
    for f in subs {
        mui_reel::accumulate(acc, &r.draw(f)?.0);
    }
    Ok(mui_reel::resolve(acc, subs.len().max(1)))
}

impl CpuPool {
    /// `workers` renderers of `w` by `h`, each rasterising on `threads`
    /// extra threads of its own (0: on the worker alone).
    pub fn new(w: u16, h: u16, workers: usize, threads: u16, assets: &Assets) -> Self {
        let (jobs, rx) = mpsc::channel::<(usize, Vec<Frame>)>();
        let rx = Arc::new(Mutex::new(rx));
        let (tx, done) = mpsc::channel();
        let workers = workers.max(1);
        let handles = (0..workers)
            .map(|_| {
                let (rx, tx, assets) = (rx.clone(), tx.clone(), assets.clone());
                thread::spawn(move || {
                    let mut r = Renderer::with_threads(w, h, threads);
                    r.assets = assets;
                    let mut acc = Vec::new();
                    loop {
                        // The lock is held while waiting: one idle worker
                        // takes the next job, the others queue on the lock.
                        let job = rx
                            .lock()
                            .map_err(|_| ())
                            .and_then(|q| q.recv().map_err(|_| ()));
                        let Ok((i, subs)) = job else { return };
                        if tx.send((i, shutter(&mut r, &mut acc, &subs))).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        Self {
            jobs: Some(jobs),
            done,
            workers: handles,
            parked: BTreeMap::new(),
            sent: 0,
            next: 0,
            // Two frames per worker in flight: one drawing, one queued.
            window: workers * 2,
        }
    }

    pub fn push(&mut self, subs: Vec<Frame>) -> Result<Option<Vec<u8>>, String> {
        self.jobs
            .as_ref()
            .ok_or("pool finished")?
            .send((self.sent, subs))
            .map_err(|_| "every CPU worker died")?;
        self.sent += 1;
        if self.sent - self.next >= self.window {
            self.take().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Every frame still in flight, oldest first; the workers stop.
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let mut out = Vec::new();
        while self.next < self.sent {
            out.push(self.take()?);
        }
        self.jobs = None;
        for w in self.workers.drain(..) {
            w.join().map_err(|_| "a CPU worker panicked")?;
        }
        Ok(out)
    }

    fn take(&mut self) -> Result<Vec<u8>, String> {
        loop {
            if let Some(px) = self.parked.remove(&self.next) {
                self.next += 1;
                return px;
            }
            let (i, px) = self
                .done
                .recv()
                .map_err(|_| "a CPU worker panicked".to_owned())?;
            self.parked.insert(i, px);
        }
    }
}
