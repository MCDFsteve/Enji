//! Explicit Play Next requests occupy the consecutive slots immediately
//! after the current song. This distinguishes duplicates using one count.

use std::ops::Range;

pub(super) fn range(current: usize, count: usize) -> Range<usize> {
    let first = current + 1;
    first..first + count
}

/// The song before `current` in linear order, never consuming a pending Play
/// Next request when the rest of the queue is only pending requests.
pub(super) fn previous(len: usize, current: usize, count: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let prev = (current + len - 1) % len;
    if range(current, count).contains(&prev) { current } else { prev }
}

/// Keep the unplayed requests after the selected row, even on a manual
/// jump. The remaining songs keep their relative order. Return the selected
/// row's new index and keep the corresponding shuffle indices valid.
pub(super) fn select<T>(
    queue: &mut [T], shuffled: &mut [usize], current: usize, target: usize, count: &mut usize,
) -> usize {
    if *count == 0 || target == current {
        return target;
    }
    let pending = range(current, *count);
    let (span, shift, selected) = if pending.contains(&target) {
        *count -= 1;
        // Move the selected request to the front and consume it.
        (pending.start..target + 1, target - pending.start, pending.start)
    } else if target < current {
        // Pull the pending block forward, just after the selected song.
        (target + 1..pending.end, current - target, target)
    } else {
        // Move the pending block past the selected song.
        (pending.start..target + 1, *count, target - *count)
    };
    if shift != 0 {
        queue[span.clone()].rotate_left(shift);
        for index in shuffled.iter_mut().filter(|i| span.contains(i)) {
            if *index < span.start + shift {
                *index += span.len() - shift;
            } else {
                *index -= shift;
            }
        }
    }
    selected
}

/// The current song is the first row in the displayed shuffle order.
pub(super) fn show(order: &mut Vec<usize>, pending: Range<usize>) {
    if pending.is_empty() || order[1..].iter().copied().take(pending.len()).eq(pending.clone()) {
        return;
    }
    order.retain(|index| !pending.contains(index));
    order.splice(1..1, pending);
}

pub(super) fn discard<T>(queue: &mut Vec<T>, shuffled: &mut Vec<usize>, pending: Range<usize>) {
    queue.drain(pending.clone());
    shuffled.retain_mut(|index| {
        if pending.contains(index) {
            return false;
        }
        if *index >= pending.end {
            *index -= pending.len();
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jumping_forward_or_back_keeps_requests_next_then_resumes_at_the_new_song() {
        let mut queue = ['A', 'P', 'Q', 'B', 'C'];
        let mut shuffled = [4, 3, 2, 1, 0];
        let mut count = 2;
        let at = select(&mut queue, &mut shuffled, 0, 3, &mut count);
        assert_eq!(at, 1);
        assert_eq!(queue, ['A', 'B', 'P', 'Q', 'C']);
        assert_eq!(shuffled.map(|i| queue[i]), ['C', 'B', 'Q', 'P', 'A']);
        let at = select(&mut queue, &mut shuffled, at, 0, &mut count);
        assert_eq!(at, 0);
        assert_eq!(queue, ['A', 'P', 'Q', 'B', 'C']);
        assert_eq!(shuffled, [4, 3, 2, 1, 0]);
        assert_eq!(count, 2);
    }

    #[test]
    fn selecting_a_later_request_consumes_only_that_occurrence() {
        let mut queue = vec!['P', 'P', 'Q', 'P', 'B'];
        let mut order = vec![0, 4, 3, 2, 1];
        let mut count = 3;
        let at = select(&mut queue, &mut order, 0, 2, &mut count);
        assert_eq!(at, 1);
        assert_eq!(queue, ['P', 'Q', 'P', 'P', 'B']);
        assert_eq!(count, 2);
        let mut shown = vec![1, 4, 3, 0, 2];
        show(&mut shown, range(at, count));
        assert_eq!(shown, [1, 2, 3, 4, 0]);
        discard(&mut queue, &mut order, range(at, count));
        assert_eq!(queue, ['P', 'Q', 'B']);
        assert_eq!(order, [0, 2, 1]);
    }

    #[test]
    fn previous_never_consumes_pending_requests_when_queue_has_no_other_songs() {
        assert_eq!(previous(3, 0, 2), 0);
        assert_eq!(previous(4, 0, 2), 3);
        assert_eq!(previous(4, 1, 2), 0);
    }
}
