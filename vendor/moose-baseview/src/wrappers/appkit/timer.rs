use super::callback;
use block2::RcBlock;
use objc2_core_foundation::{
    kCFAllocatorDefault, kCFRunLoopCommonModes, CFRetained, CFRunLoop, CFRunLoopTimer,
    CFTimeInterval,
};

pub struct TimerHandle {
    timer: CFRetained<CFRunLoopTimer>,
}

impl TimerHandle {
    pub fn new(interval: CFTimeInterval, closure: impl Fn() + 'static) -> Option<Self> {
        Self::schedule(interval, interval, closure)
    }

    pub fn once(delay: CFTimeInterval, closure: impl Fn() + 'static) -> Option<Self> {
        Self::schedule(delay, 0.0, closure)
    }

    fn schedule(
        delay: CFTimeInterval, interval: CFTimeInterval, closure: impl Fn() + 'static,
    ) -> Option<Self> {
        let run_loop = CFRunLoop::current()?;
        let block = RcBlock::new(move |_| callback("run-loop timer", (), &closure));
        // CFDate is not enabled by our minimal bindings; the signature is from CFDate.h.
        extern "C" {
            fn CFAbsoluteTimeGetCurrent() -> f64;
        }
        let fire = unsafe { CFAbsoluteTimeGetCurrent() } + delay.max(0.0);
        let allocator = unsafe { kCFAllocatorDefault };
        let timer =
            unsafe { CFRunLoopTimer::with_handler(allocator, fire, interval, 0, 0, Some(&block)) }?;
        // Common modes allow a deadline to wake during DAW menu tracking/live resize.
        run_loop.add_timer(Some(&timer), unsafe { kCFRunLoopCommonModes });
        Some(Self { timer })
    }
}

impl Drop for TimerHandle {
    fn drop(&mut self) {
        // Invalidate, don't just remove from one run-loop mode: release the block
        // and prevent a queued callback from rearming a detached editor.
        self.timer.invalidate();
    }
}
