//! Empty declaration storage only. Never retain a callback, font or UI owner.
use crate::{Element, Extras, State, StateStyle};
use std::cell::RefCell;
use std::mem::size_of;

type States = Vec<(State, StateStyle)>;
const MAX_ENTRIES: usize = 4096;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_STATE_CAPACITY: usize = 8;

#[derive(Default)]
struct Pool {
    extras: Vec<Box<Extras>>,
    states: Vec<States>,
    // Payload bytes, including actual state-vector capacity. The two index
    // vectors are separately bounded by MAX_ENTRIES each (128 KiB on 64 bit).
    bytes: usize,
}

thread_local! {
    static EMPTY: RefCell<Pool> = RefCell::new(Pool::default());
}

pub(crate) fn extras() -> Box<Extras> {
    EMPTY
        .try_with(|pool| {
            let mut pool = pool.try_borrow_mut().ok()?;
            let value = pool.extras.pop()?;
            pool.bytes -= size_of::<Extras>();
            Some(value)
        })
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub(crate) fn states() -> States {
    EMPTY
        .try_with(|pool| {
            let mut pool = pool.try_borrow_mut().ok()?;
            let value = pool.states.pop()?;
            pool.bytes -= value.capacity() * size_of::<(State, StateStyle)>();
            Some(value)
        })
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub(crate) fn recycle(element: &mut Element) {
    if element.extras.is_none() && element.states.capacity() == 0 {
        return;
    }
    let mut extras = element.extras.take();
    let mut states = std::mem::take(&mut element.states);
    // User destructors may build another element, borrow a memo table or drop
    // a UI. Run all of them BEFORE borrowing EMPTY. Leave only default boxes
    // and empty vector allocations, including when TLS is already destroyed.
    if let Some(value) = &mut extras {
        **value = Extras::default();
    }
    states.clear();
    if extras.is_none() && !(1..=MAX_STATE_CAPACITY).contains(&states.capacity()) {
        return;
    }
    let _ = EMPTY.try_with(|pool| {
        let Ok(mut pool) = pool.try_borrow_mut() else {
            return;
        };
        if extras.is_some()
            && pool.extras.len() < MAX_ENTRIES
            && pool.bytes + size_of::<Extras>() <= MAX_BYTES
        {
            pool.bytes += size_of::<Extras>();
            pool.extras.push(extras.take().expect("checked above"));
        }
        let bytes = states.capacity() * size_of::<(State, StateStyle)>();
        if (1..=MAX_STATE_CAPACITY).contains(&states.capacity())
            && pool.states.len() < MAX_ENTRIES
            && pool.bytes + bytes <= MAX_BYTES
        {
            pool.bytes += bytes;
            pool.states.push(std::mem::take(&mut states));
        }
    });
}

/// Release empty declaration buffers on the calling thread.
///
/// The UI runtime calls this when releasing thread-owned memo trees. Buffers
/// are shared between UIs on that thread but hold no UI data or callbacks;
/// clearing them can only make another UI's next build allocate again.
#[doc(hidden)]
pub fn clear_declaration_storage() {
    let removed = EMPTY.try_with(|pool| {
        pool.try_borrow_mut()
            .ok()
            .map(|mut pool| std::mem::take(&mut *pool))
    });
    drop(removed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Outline, Style};
    use mui_geometry::Path;
    use std::{cell::Cell, rc::Rc, sync::Arc};

    #[test]
    fn warm_storage_reuses_boxes_and_state_capacity_without_stale_fields() {
        clear_declaration_storage();
        let mut first = Element::default();
        first.extras_mut().measure_as = Some("-88.8 dB".into());
        let address = &**first.extras.as_ref().unwrap() as *const Extras;
        first.states = Vec::with_capacity(4);
        first
            .states
            .push((State::Hover, StateStyle(Arc::new(|s| s))));
        let buffer = first.states.as_ptr();
        recycle(&mut first);
        assert!(first.extras.is_none() && first.states.is_empty());
        let mut second = Element::default();
        assert_eq!(second.extras_mut() as *const Extras, address);
        assert_eq!(second.extras(), &Extras::default());
        second.states = states();
        assert!(second.states.is_empty());
        assert_eq!(second.states.as_ptr(), buffer);
        clear_declaration_storage();
    }

    #[test]
    fn recycling_drops_captures_before_borrowing_the_pool() {
        struct Capture(Rc<Cell<usize>>);
        impl Drop for Capture {
            fn drop(&mut self) {
                EMPTY.with(|pool| assert!(pool.try_borrow_mut().is_ok()));
                // Exercise actual construction and pool cleanup reentrantly.
                let mut fresh = Element::default();
                fresh.extras_mut().memo = None;
                clear_declaration_storage();
                self.0.set(self.0.get() + 1);
            }
        }
        clear_declaration_storage();
        let dropped = Rc::new(Cell::new(0));
        let mut element = Element::default();
        let a = Capture(dropped.clone());
        element.extras_mut().outline = Some(Outline(Arc::new(move |_| {
            let _ = &a;
            Path::default()
        })));
        let b = Capture(dropped.clone());
        element.states.push((
            State::Hover,
            StateStyle(Arc::new(move |s: Style| {
                let _ = &b;
                s
            })),
        ));
        recycle(&mut element);
        assert_eq!(dropped.get(), 2);
        clear_declaration_storage();
    }

    #[test]
    fn storage_is_bounded_by_entries_and_actual_capacity() {
        clear_declaration_storage();
        let mut elements: Vec<_> = (0..MAX_ENTRIES + 100)
            .map(|_| {
                let mut e = Element::default();
                e.extras_mut().memo = None;
                e.states = Vec::with_capacity(MAX_STATE_CAPACITY);
                e
            })
            .collect();
        for e in &mut elements {
            recycle(e);
        }
        EMPTY.with(|pool| {
            let pool = pool.borrow();
            assert!(!pool.extras.is_empty());
            assert!(pool.extras.len() <= MAX_ENTRIES);
            assert!(pool.states.len() <= MAX_ENTRIES);
            assert!(pool.bytes <= MAX_BYTES);
            assert_eq!(
                pool.bytes,
                pool.extras.len() * size_of::<Extras>()
                    + pool
                        .states
                        .iter()
                        .map(|s| s.capacity() * size_of::<(State, StateStyle)>())
                        .sum::<usize>()
            );
        });
        clear_declaration_storage();
        let mut huge = Element::default();
        huge.states = Vec::with_capacity(MAX_STATE_CAPACITY + 1);
        recycle(&mut huge);
        EMPTY.with(|pool| assert_eq!(pool.borrow().bytes, 0));
    }

    #[test]
    fn clearing_releases_all_retained_storage() {
        let mut element = Element::default();
        element.extras_mut().memo = None;
        recycle(&mut element);
        clear_declaration_storage();
        EMPTY.with(|pool| {
            let pool = pool.borrow();
            assert_eq!(pool.bytes, 0);
            assert_eq!(pool.extras.capacity(), 0);
            assert_eq!(pool.states.capacity(), 0);
        });
    }

    #[test]
    fn construction_and_retirement_work_after_pool_tls_destruction() {
        struct Late;
        impl Drop for Late {
            fn drop(&mut self) {
                let mut element = Element::default();
                element.extras_mut().memo = None;
                element.states = states();
                recycle(&mut element);
                clear_declaration_storage();
            }
        }
        thread_local! {
            static LATE: Late = const { Late };
        }
        std::thread::spawn(|| {
            // TLS destructs in reverse initialization order: Late runs after
            // EMPTY, so every pool operation must tolerate AccessError.
            LATE.with(|_| {});
            clear_declaration_storage();
        })
        .join()
        .unwrap();
    }
}
