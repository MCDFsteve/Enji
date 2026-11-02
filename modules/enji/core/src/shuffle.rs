//! Shuffle as Misuzu Music has it (`audio_player_service_impl.dart`),
//! function for function: the whole queue shuffled up front with the
//! playing song first, next and previous walking that order, a new order
//! once it runs out, and a new order whenever it no longer fits the queue.

use rand::Rng;
use std::collections::HashSet;

#[derive(Default)]
pub struct Shuffle {
    /// The queue's places in the order they play.
    pub order: Vec<usize>,
    /// Where the playing song is in `order`.
    pub position: usize,
}

impl Shuffle {
    pub fn clear(&mut self) {
        self.order.clear();
        self.position = 0;
    }

    /// A new order of a queue of `len`: Fisher–Yates, then `current`
    /// moved to the front (`_generateShuffleOrder`).
    pub fn generate(&mut self, len: usize, current: usize) {
        if len == 0 {
            return;
        }
        let mut random = rand::thread_rng();
        self.order = (0..len).collect();
        for i in (1..len).rev() {
            let j = random.gen_range(0..=i);
            self.order.swap(i, j);
        }
        let current = current.min(len - 1);
        if let Some(at) = self.order.iter().position(|&i| i == current)
            && at > 0
        {
            self.order.remove(at);
            self.order.insert(0, current);
        }
        self.position = 0;
    }

    /// `position` pointed at `current`, or a new order when this one no
    /// longer fits the queue (`_syncShuffleOrderWithCurrent`).
    pub fn sync(&mut self, len: usize, current: usize) {
        if len <= 1 {
            return self.clear();
        }
        if self.order.len() != len {
            return self.generate(len, current);
        }
        let mut seen = HashSet::new();
        if self.order.iter().any(|&i| i >= len || !seen.insert(i)) {
            return self.generate(len, current);
        }
        match self.order.iter().position(|&i| i == current) {
            Some(at) => self.position = at,
            None => self.generate(len, current),
        }
    }

    /// `index` moved to just after the playing song; where it is now
    /// (`_placeShuffleIndexNext`).
    pub fn place_next(&mut self, len: usize, current: usize, index: usize) -> usize {
        if len <= 1 || index >= len {
            return 0;
        }
        self.sync(len, current);
        if self.order.is_empty() {
            return 0;
        }
        let current_at = self.position.min(self.order.len() - 1);
        let from = match self.order.iter().position(|&i| i == index) {
            Some(from) => from,
            None => {
                self.generate(len, current);
                return self.place_next(len, current, index);
            }
        };
        let wanted = (current_at + 1).min(self.order.len() - 1);
        if from == wanted {
            return wanted;
        }
        self.order.remove(from);
        let current_at = if from < current_at { current_at - 1 } else { current_at };
        let to = (current_at + 1).min(self.order.len());
        self.order.insert(to, index);
        self.position = current_at;
        to
    }

    /// The song after `current`, a new order once this one is through
    /// (`_getRandomIndex`).
    pub fn next(&mut self, len: usize, current: usize) -> usize {
        if len <= 1 {
            return 0;
        }
        self.sync(len, current);
        if self.order.len() != len {
            self.generate(len, current);
        }
        if self.position + 1 >= self.order.len() {
            self.generate(len, current);
        }
        if self.order.len() <= 1 {
            self.position = 0;
            return self.order.first().copied().unwrap_or(0);
        }
        self.position = (self.position + 1).min(self.order.len() - 1);
        self.order[self.position]
    }

    /// The song before `current`; the first one stays put
    /// (`_getPreviousShuffleIndex`).
    pub fn previous(&mut self, len: usize, current: usize) -> usize {
        if len <= 1 {
            return 0;
        }
        self.sync(len, current);
        if self.order.len() != len {
            self.generate(len, current);
        }
        if self.order.is_empty() {
            return 0;
        }
        if self.position == 0 {
            return self.order[0];
        }
        self.position -= 1;
        self.order[self.position]
    }
}

#[cfg(test)]
mod tests {
    use super::Shuffle;

    fn sorted(order: &[usize]) -> Vec<usize> {
        let mut all = order.to_vec();
        all.sort();
        all
    }

    #[test]
    fn a_new_order_starts_with_the_playing_song() {
        let mut shuffle = Shuffle::default();
        for current in 0..8 {
            shuffle.generate(8, current);
            assert_eq!(shuffle.order[0], current);
            assert_eq!(shuffle.position, 0);
            assert_eq!(sorted(&shuffle.order), (0..8).collect::<Vec<_>>());
        }
    }

    #[test]
    fn a_round_plays_every_song_once_then_shuffles_again() {
        let mut shuffle = Shuffle::default();
        shuffle.generate(6, 2);
        let first = shuffle.order.clone();
        let mut current = 2;
        let mut played = vec![current];
        for _ in 0..5 {
            current = shuffle.next(6, current);
            played.push(current);
        }
        assert_eq!(played, first);
        // The round is through: the next one starts a new order from the
        // song playing now, so it is never the same song twice in a row.
        let last = current;
        current = shuffle.next(6, current);
        assert_ne!(current, last);
        assert_eq!(shuffle.order[0], last);
        assert_eq!(shuffle.position, 1);
    }

    #[test]
    fn previous_walks_back_and_stays_at_the_first() {
        let mut shuffle = Shuffle::default();
        shuffle.generate(5, 0);
        let order = shuffle.order.clone();
        let second = shuffle.next(5, 0);
        let third = shuffle.next(5, second);
        assert_eq!(shuffle.previous(5, third), order[1]);
        assert_eq!(shuffle.previous(5, order[1]), order[0]);
        assert_eq!(shuffle.previous(5, order[0]), order[0]);
        assert_eq!(shuffle.position, 0);
    }

    #[test]
    fn sync_follows_a_song_played_from_the_queue() {
        let mut shuffle = Shuffle::default();
        shuffle.generate(5, 0);
        let order = shuffle.order.clone();
        shuffle.sync(5, order[3]);
        assert_eq!(shuffle.position, 3);
        assert_eq!(shuffle.order, order);
        assert_eq!(shuffle.next(5, order[3]), order[4]);
    }

    #[test]
    fn an_order_that_no_longer_fits_is_made_again() {
        let mut shuffle = Shuffle::default();
        shuffle.generate(5, 1);
        // A song left the queue.
        shuffle.sync(4, 1);
        assert_eq!(shuffle.order.len(), 4);
        assert_eq!(shuffle.order[0], 1);
        assert_eq!(sorted(&shuffle.order), vec![0, 1, 2, 3]);
        shuffle.order = vec![0, 0, 1, 2];
        shuffle.sync(4, 2);
        assert_eq!(sorted(&shuffle.order), vec![0, 1, 2, 3]);
        assert_eq!(shuffle.order[0], 2);
        shuffle.sync(1, 0);
        assert!(shuffle.order.is_empty());
    }

    #[test]
    fn a_song_to_play_next_moves_behind_the_playing_one() {
        let mut shuffle = Shuffle {
            order: vec![3, 0, 4, 1, 2],
            position: 2,
        };
        // 1 sits after the playing 4 already.
        assert_eq!(shuffle.place_next(5, 4, 1), 3);
        assert_eq!(shuffle.order, vec![3, 0, 4, 1, 2]);
        // 3, before the playing one, moves to just after it.
        assert_eq!(shuffle.place_next(5, 4, 3), 2);
        assert_eq!(shuffle.order, vec![0, 4, 3, 1, 2]);
        assert_eq!(shuffle.position, 1);
        // 2, further on, moves up.
        assert_eq!(shuffle.place_next(5, 4, 2), 2);
        assert_eq!(shuffle.order, vec![0, 4, 2, 3, 1]);
    }

    #[test]
    fn a_queue_of_one_has_no_order() {
        let mut shuffle = Shuffle::default();
        assert_eq!(shuffle.next(1, 0), 0);
        assert_eq!(shuffle.previous(1, 0), 0);
        assert_eq!(shuffle.place_next(1, 0, 0), 0);
        assert!(shuffle.order.is_empty());
    }
}
